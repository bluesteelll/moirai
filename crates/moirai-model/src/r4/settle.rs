//! Who may write a re-bind, and what a settle writes ([40 §4.2], §4.3 step 6, §5.3; [F18 §2.6] I-F6, §3; [F20 §5.16],
//! §5.17): the designation relation D and the writer-tree predicate, the write rule with freshness and `main`'s
//! committed-only rule, `PENDING` promotion, the rows a reader tree writes, `FILEOBS` upkeep, automatic `path_moves`
//! entries, and the settles of link conflicts after a merge ([RULES/link-merge-rules] LV rows); the explicit answers to
//! a link conflict (LV-003, LV-006, LV-007) are `LinksFix`'s ([`crate::links::fix`]).
//!
//! A simulated tree is constant within a command, so the quiescence re-check of [F20 §5.17] always passes ([API §12.6]).

use crate::r4::anchor::{AState, Anchor, Consts, Content, resolve};
use crate::r4::cascade::{
    FileNode, FileObs, FileResult, PClass, Params, PendingRow, PendingSource, PrefixKey, Prep,
    Runtime, View, read_content,
};
use crate::r4::git::{Chain, Class, Git, Head, Hist};
use crate::r4::strings::{Here, State, There, evidence_token, reader_note, score_text};
use crate::r4::text::{Fingerprint, fingerprint, oid};
use crate::r4::tree::{Fs, StatOut, Tree};
use crate::r4::uid::FileStatus;
use crate::value::{Algo, Oid, Uid};
use std::collections::{BTreeMap, BTreeSet};

/// A binding row as the designation reads it ([F18 §3.1], §3.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding {
    /// The bound directory's canonical text.
    pub dir: String,
    /// The ref.
    pub branch: String,
    /// `designated`.
    pub designated: bool,
    /// `expected_ref`, short form.
    pub expected_ref: Option<String>,
    /// `base`.
    pub base: Option<String>,
}

/// One pair of the designation relation D ([F18 §3.4]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Designated {
    /// The branch.
    pub branch: String,
    /// The tree.
    pub tree: String,
    /// The expected git ref, short form.
    pub expected_ref: Option<String>,
    /// The base commit.
    pub base: Option<String>,
}

/// D = D_heads ∪ {(`main`, T_m)} when that pair keeps D injective ([F18 §3.4]); pairs that share a branch or a tree with
/// another row pair are dropped (fail closed), so their trees read only.
// spec: [F18 §3.4]; [F18 §2.12] I-F12
pub fn designation(
    bindings: &[Binding],
    main_tree: Option<&str>,
    main_ref: Option<&str>,
) -> Vec<Designated> {
    let rows: Vec<&Binding> = bindings.iter().filter(|b| b.designated).collect();
    let shared = |b: &Binding| {
        rows.iter()
            .filter(|o| o.branch == b.branch || o.dir == b.dir)
            .count()
            > 1
    };
    let mut d: Vec<Designated> = rows
        .iter()
        .filter(|b| !shared(b))
        .map(|b| Designated {
            branch: b.branch.clone(),
            tree: b.dir.clone(),
            expected_ref: b.expected_ref.clone(),
            base: b.base.clone(),
        })
        .collect();
    if let Some(tm) = main_tree
        && !rows.iter().any(|b| b.branch == "main" || b.dir == tm)
    {
        d.push(Designated {
            branch: "main".into(),
            tree: tm.to_string(),
            expected_ref: main_ref
                .filter(|r| !r.is_empty())
                .map(|r| r.strip_prefix("refs/heads/").unwrap_or(r).to_string()),
            base: None,
        });
    }
    d
}

/// Whether a designated tree is on its branch's git line ([F18 §3.6]): no git and no expected ref or base; a symbolic
/// HEAD naming the expected ref; or a detached HEAD at or below the base and in line with the expected ref's tip.
// spec: [F18 §3.6]
pub fn on_line(pair: &Designated, git: &Git) -> bool {
    let Some((repo, head)) = git.of_tree(&pair.tree) else {
        return pair.expected_ref.is_none() && pair.base.is_none();
    };
    match head {
        Head::Ref(r) => r
            .strip_prefix("refs/heads/")
            .is_some_and(|n| pair.expected_ref.as_deref() == Some(n)),
        Head::Detached(h) => {
            let (Some(b), Some(e)) = (&pair.base, &pair.expected_ref) else {
                return false;
            };
            let Some(t) = repo.refs.get(&format!("refs/heads/{e}")) else {
                return false;
            };
            repo.is_ancestor(b, h) && (repo.is_ancestor(h, t) || repo.is_ancestor(t, h))
        }
    }
}

/// Whether tree T is the writer tree of branch B: B's designated tree, on the line ([F18 §3.6]; [40 §5.3]).
// spec: [F18 §3.6]; [40 §5.3] writer tree
pub fn writer_tree(branch: &str, tree: &str, d: &[Designated], git: &Git) -> bool {
    d.iter()
        .find(|p| p.branch == branch)
        .is_some_and(|p| p.tree == tree && on_line(p, git))
}

/// The reader note of a result read in tree T for branch B, `None` in B's writer tree ([F18 §4.8] item 2).
// spec: [F18 §4.8] item 2
pub fn reader_note_for(
    branch: &str,
    tree: &str,
    d: &[Designated],
    git: &Git,
    label: &str,
) -> Option<String> {
    if writer_tree(branch, tree, d, git) {
        return None;
    }
    let here = match git.of_tree(tree) {
        None => Here::NoGit,
        Some((_, Head::Ref(r))) => {
            Here::Ref(r.strip_prefix("refs/heads/").unwrap_or(r).to_string())
        }
        Some((_, Head::Detached(c))) => Here::Detached(c.clone()),
    };
    let there = match d.iter().find(|p| p.branch == branch) {
        None => There::BoundTree,
        Some(p) if p.tree != tree => There::Tree(label.to_string()),
        Some(p) => match &p.expected_ref {
            Some(e) => There::Ref(e.clone()),
            None => There::GitLine,
        },
    };
    Some(reader_note(&here, &there))
}

/// The write rule of an automatic re-bind (I-F6, [F18 §2.6]): exact evidence, or under `files.policy.auto = strong` a
/// unique strong proposal of a class policy B may apply ([F18 §5.4]: never a merged, split, identical-copy, weak,
/// directory-replaced or moved-differently proposal); the writer tree; fresh for the node; the quiescence re-check
/// passed; on `main`, the new path committed in τ(H).
// spec: [F18 §2.6] I-F6; [40 §4.3] step 6; [F18 §5.4]
pub fn if6_rebind_rule(r: &FileResult, p: &Params, quiescent: bool) -> bool {
    let guess = p.policy_strong
        && r.state == State::MovedNeedsConfirm
        && r.guess.is_some()
        && r.proposals.len() == 1
        && r.proposals[0].auto;
    let evidence = r.state == State::MovedAuto || guess;
    evidence && p.settle && p.writer && r.fresh && quiescent && (!p.main || r.committed)
}

/// The `relink` value a re-bind records ([F18 §5.4]): `how/evidence` for exact evidence, `policy/<class>[/<score>]` for a
/// strong re-bind under policy B.
// spec: [F18 §5.4]
pub fn relink_of(r: &FileResult) -> Option<String> {
    if let Some((ev, how)) = r.evidence {
        return Some(format!("{how}/{}", evidence_token(ev)));
    }
    let (class, score) = r.guess?;
    Some(match score {
        Some(s) => format!("policy/{}/{}", evidence_token(class), score_text(s)),
        None => format!("policy/{}", evidence_token(class)),
    })
}

/// A re-bind a settle writes: one `SetField(observation)` plus the old path in `aliases` ([40 §4.3] "What settle
/// writes").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rebind {
    /// `#F`.
    pub n: u32,
    /// The old path (joins `aliases`).
    pub from: String,
    /// The new path.
    pub to: String,
    /// The new `relink`.
    pub relink: String,
    /// The content's `oid` at the new path.
    pub oid: Option<Oid>,
    /// The raw size.
    pub bytes: u64,
    /// `observed_git` = H.
    pub observed_git: Option<String>,
    /// `observed_blob`: git's blob id at the new path in τ(H), empty when uncommitted.
    pub observed_blob: Option<String>,
}

/// A `planned → present` binding ([40 §3.2]): the first observation, no `relink`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binds {
    /// `#F`.
    pub n: u32,
    /// The path.
    pub path: String,
    /// The `oid`.
    pub oid: Option<Oid>,
    /// The size.
    pub bytes: u64,
    /// `observed_git`.
    pub observed_git: Option<String>,
    /// `observed_blob`.
    pub observed_blob: Option<String>,
}

/// A `path_moves` entry a settle records ([F20 §5.16]): (from/, to/, class `committed` or `observed`, git commit).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct NewMove {
    /// `from`.
    pub from: String,
    /// `to`.
    pub to: String,
    /// `committed` (true) or `observed`.
    pub committed: bool,
    /// The git commit, for `committed`.
    pub git: Option<String>,
}

/// A `PathClaim` a settle unified ([RULES/link-merge-rules] LV-005): the node that becomes `removed{reason: same-as,
/// replaced_by}` the kept node, and the provenance of the unification.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unified {
    /// The node removed (`#N`): the one whose alias is the rename's source.
    pub removed: u32,
    /// The node kept (`#N`): the one registered at the claimed path.
    pub kept: u32,
    /// `git/r100`.
    pub relink: String,
}

/// What one settle writes ([40 §4.2]; [API §12.6]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SettleOut {
    /// Every node's result.
    pub results: BTreeMap<u32, FileResult>,
    /// Versioned re-binds (writer tree only).
    pub rebinds: Vec<Rebind>,
    /// `planned → present` bindings.
    pub binds: Vec<Binds>,
    /// Composite conflicts resolved by observation: `#F` with the re-bind ([RULES/link-merge-rules] LV-001).
    pub resolved: Vec<Rebind>,
    /// Path claims unified ([RULES/link-merge-rules] LV-005).
    pub unified: Vec<Unified>,
    /// Anchor selector conflicts resolved ([RULES/link-merge-rules] LV-004): (`#F`, anchor uid, the side kept: 0 ours,
    /// 1 theirs).
    pub anchors: Vec<(u32, Uid, usize)>,
    /// Automatic `path_moves` entries.
    pub moves: Vec<NewMove>,
    /// `PENDING` rows (a reader tree's observations).
    pub pending: Vec<PendingRow>,
    /// `FILEOBS` rows written: (`#F`, tree) → row.
    pub fileobs: Vec<((u32, String), FileObs)>,
    /// `PREFIXEV` rows written: (tree, root, from/, to/) → the nodes re-bound so far.
    pub prefixev: Vec<(PrefixKey, BTreeSet<u32>)>,
    /// `FPRINT` rows: the fingerprint of every text content the settle read at a node's path, by `oid` ([40 §2.5],
    /// §2.6: written by settles).
    pub fprint: Vec<(Oid, Fingerprint)>,
    /// `DIRMAP` rows: (tree, directory id) → (path, mtime) of every directory the settle enumerated, except a racy one,
    /// whose previous row is kept ([F20 §5.12.1]).
    pub dirmap: Vec<((String, u64), (String, i64))>,
}

/// The (X/, Y/) splits of a move p → q: every pair of directory prefixes after which the paths agree.
fn splits(p: &str, q: &str) -> Vec<(String, String)> {
    let ps: Vec<&str> = p.split('/').collect();
    let qs: Vec<&str> = q.split('/').collect();
    let mut out = Vec::new();
    let mut k = 1;
    while k < ps.len() && k < qs.len() && ps[ps.len() - k..] == qs[qs.len() - k..] {
        let x = format!("{}/", ps[..ps.len() - k].join("/"));
        let y = format!("{}/", qs[..qs.len() - k].join("/"));
        if x != y {
            out.push((x, y));
        }
        k += 1;
    }
    out
}

/// The side of a composite conflict as a file node, for resolution by observation.
fn side_node(f: &FileNode, i: usize) -> FileNode {
    let s = &f.conflict.as_ref().expect("a conflict")[i];
    FileNode {
        path: s.path.clone(),
        oid: s.oid.clone(),
        observed_git: s.observed_git.clone(),
        observed_blob: s.observed_blob.clone(),
        conflict: None,
        ..f.clone()
    }
}

/// LV-001: a composite `FieldEdit` conflict is resolved by observation only in a writer tree fresh for every side's
/// value and with exact evidence for the chosen path: both sides resolve to one path q, one of them by exact evidence
/// (a `lazy`, `git` or `hook` token); the re-bind records `merge-observation/<evidence>`. "The newest side" is never
/// taken ([40 §5.3]). Being an automatic re-bind, the resolution also needs every condition of I-F6 ([F18 §2.6]: "a
/// resolution … additionally requires"), so on `main` q must be committed in τ(H), as for every re-bind; a named
/// root has no git, so on `main` its conflicts never resolve by observation.
// rule: LV-001
pub fn settle_observation(
    f: &FileNode,
    view: &View,
    fs: &Fs,
    git: &Git,
    rt: &Runtime,
    tree: &str,
    p: &Params,
) -> Option<(String, String)> {
    observation_in(f, &Prep::new(view, fs, git, rt, tree, p), p)
}

/// [`settle_observation`] with the command's prepared inputs.
fn observation_in(f: &FileNode, prep: &Prep<'_>, p: &Params) -> Option<(String, String)> {
    f.conflict.as_ref()?;
    if !(p.settle && p.writer) {
        return None;
    }
    let rs: Vec<FileResult> = (0..2).map(|i| prep.resolve(&side_node(f, i), p)).collect();
    if !rs.iter().all(|r| r.fresh) {
        return None;
    }
    let at: Vec<Option<&String>> = rs
        .iter()
        .map(|r| match r.state {
            State::Ok | State::MovedAuto => r.at.as_ref(),
            _ => None,
        })
        .collect();
    let (Some(a), Some(b)) = (at[0], at[1]) else {
        return None;
    };
    if a != b {
        return None;
    }
    let ev = rs.iter().find_map(|r| {
        r.evidence
            .filter(|(_, how)| matches!(*how, "lazy" | "git" | "hook"))
            .map(|(e, _)| evidence_token(e))
    })?;
    // I-F6 item 5: on `main`, q committed in τ(H). Only root `project` has git (E6 and τ(H) are the project
    // repository's, [F20] open point 16), so a named root's q is never committed — as for its re-binds, whose
    // `committed` is false without git ([`if6_rebind_rule`]) — and a root-relative q that the project repository
    // happens to hold says nothing about it.
    if p.main {
        let (Some(hist), Some(h)) = prep.git() else {
            return None;
        };
        if f.root != "project" || !hist.repo.tau(h).contains_key(a.as_str()) {
            return None;
        }
    }
    Some((a.clone(), format!("merge-observation/{ev}")))
}

/// LV-004: an anchor `FieldEdit` conflict keeps the candidate that resolves `fresh` in the merged tree's content; when
/// both or neither do, the conflict stays (`None`). Index 0 is ours, 1 theirs. [`settle`] runs it for every conflicted
/// anchor of a node whose file resolved `ok` or `moved-auto` in a writer tree fresh for it.
// rule: LV-004
pub fn settle_anchor_conflict(
    sides: &[Anchor; 2],
    content: &[u8],
    algo: Algo,
    c: &Consts,
) -> Option<usize> {
    let fresh: Vec<bool> = sides
        .iter()
        .map(|a| resolve(a, Content::Bytes(content), algo, c).state == AState::Fresh)
        .collect();
    match (fresh[0], fresh[1]) {
        (true, false) => Some(0),
        (false, true) => Some(1),
        _ => None,
    }
}

/// E6's window for a node in a tree: g..H when the node's `observed_git` g is in the store, else the time-bounded
/// window with the margin `SKEW` ([F20 §5.11.2]).
fn window_for(hist: &Hist<'_>, h: &str, f: &FileNode, skew_ns: i128) -> Vec<String> {
    match &f.observed_git {
        Some(g) if hist.repo.has(g) => hist.window_since(g, h),
        _ => hist.repo.window_timed(h, f.obs_hlc, skew_ns),
    }
}

/// LV-005: a `PathClaim` between two nodes at one (root, exact path) is unified by a settle only in a writer tree that
/// is fresh for both nodes, when E6 shows an exact rename inside one commit from one node's former path (an alias) to
/// the path both claim; the node whose alias is the rename's source — a `present` one, by TR-081 — becomes
/// `removed{reason: same-as, replaced_by}` the other, with provenance `git/r100`. Returns (removed `#N`, kept `#N`).
// rule: LV-005
#[allow(clippy::too_many_arguments)]
pub fn settle_path_claim(
    a: &FileNode,
    b: &FileNode,
    view: &View,
    fs: &Fs,
    git: &Git,
    rt: &Runtime,
    tree: &str,
    p: &Params,
) -> Option<(u32, u32)> {
    path_claim_in(a, b, &Prep::new(view, fs, git, rt, tree, p), p)
}

/// [`settle_path_claim`] with the command's prepared inputs.
fn path_claim_in(a: &FileNode, b: &FileNode, prep: &Prep<'_>, p: &Params) -> Option<(u32, u32)> {
    if !(p.settle && p.writer) || a.root != b.root || a.path != b.path {
        return None;
    }
    let fresh = |f: &FileNode| prep.resolve(f, p).fresh;
    if !fresh(a) || !fresh(b) {
        return None;
    }
    let (Some(hist), Some(h)) = prep.git() else {
        return None;
    };
    for (x, y) in [(a, b), (b, a)] {
        // The claimant removed must be `present`: the status machine's only settle door to `removed` is TR-081
        // (`present → removed`); a `planned` claimant keeps the conflict for `links fix`.
        if x.status != FileStatus::Present {
            continue;
        }
        let w = window_for(hist, h, x, p.skew_ns);
        if x.aliases
            .iter()
            .any(|al| hist.renamed_in_one_commit(&w, al, &y.path))
        {
            return Some((x.n, y.n));
        }
    }
    None
}

/// LV-010: `merge-check --strict-links` refuses a lane whose nodes hold a link in state `missing`, `ambiguous`,
/// `replaced` or `stale-anchor` ([40 §5.4]); part of the merge ritual, never of the merge engine.
// rule: LV-010
pub fn strict_links_refuses(states: &[State]) -> bool {
    states.iter().any(|s| {
        matches!(
            s,
            State::Missing | State::Ambiguous | State::Replaced | State::StaleAnchor
        )
    })
}

/// LV-011: `merge-check` lists every lane re-bind that rests on an uncommitted observation (an empty `observed_blob`
/// with an observation made in a git tree): "commit the move first" ([40 §5.4]).
// rule: LV-011
pub fn uncommitted_rebinds(nodes: &[FileNode]) -> Vec<u32> {
    nodes
        .iter()
        .filter(|f| {
            f.relink.is_some()
                && f.observed_git.is_some()
                && f.observed_blob.is_none()
                && !f.tombstone
        })
        .map(|f| f.n)
        .collect()
}

/// The `FILEOBS` row of a node after a settle saw it ([40 §2.6]; [F11 §12.5]). The row follows the file to the path the
/// result names only when `follow` holds: a `moved-auto` that rests on a `PENDING` row alone, which [40 §5.3]'s
/// promotion rule does not let this tree write, keeps the previous identity and stat tuple, so the next settle's own
/// evidence stays independent of the other observation (else E3 would compare the file with its own copied tuple and
/// find it exact). The row's `creation` is absent on a volume without creation times (`VolumeCaps.btime = absent`,
/// [F20 §5.18]), which leaves the copy rule's lines 2 and 3 nothing to compare; its `last_oid` is absent for content
/// beyond `files.max-read-bytes` ([F20 §2.4]).
#[allow(clippy::too_many_arguments)]
fn row_after(
    f: &FileNode,
    r: &FileResult,
    fs: &Fs,
    tree: &str,
    rt: &Runtime,
    hlc: u64,
    p: &Params,
    follow: bool,
) -> FileObs {
    let old = rt
        .fileobs
        .get(&(f.n, tree.to_string()))
        .cloned()
        .unwrap_or_default();
    let t = &fs.trees[tree];
    let at = match r.state {
        State::Ok | State::MovedAuto | State::Replaced | State::Ambiguous if follow => r.at.clone(),
        // A guess policy B applies: the row follows the file to the new path, as for `moved-auto`.
        State::MovedNeedsConfirm if r.guess.is_some() => r.at.clone(),
        _ => None,
    };
    let present_at_own = r.state == State::Ok;
    let mut row = old.clone();
    if let Some(q) = &at
        && let StatOut::Present(s) = t.stat(q)
    {
        row.file_id = Some(s.id.clone());
        row.parent_dir = Some(s.parent);
        row.size = s.size;
        row.mtime_ns = s.mtime_ns;
        row.creation_ns = t.recorded_creation(s.btime_ns);
        row.last_oid = read_content(t, &s.disk_path, p.max_read_bytes)
            .ok()
            .map(|b| oid(p.algo, b));
        row.path_seen = (s.disk_path != f.path).then(|| s.disk_path.clone());
    }
    if present_at_own {
        row.verified_at = hlc;
        row.missing_since = 0;
    } else if r.state == State::Missing {
        if row.missing_since == 0 {
            row.missing_since = hlc;
        }
    } else {
        row.missing_since = 0;
    }
    row.recorded = r.state.recordable().then(|| (r.state, r.details.clone()));
    row
}

/// `tge(a, b)` at granularity G ([F20 §5.1]).
fn tge(a: i64, b: i64, g: u64) -> bool {
    let g = g as i64;
    a.div_euclid(g) >= b.div_euclid(g)
}

/// The `DIRMAP` rows a settle records ([F20 §5.12.1]): every directory of the tree with its path and mtime, except one
/// whose mtime is `tge` the racy threshold T0 (the settle stamp's mtime, or on another volume the largest previous row
/// mtime of the tree), for which the previous row, if any, is kept.
// spec: [F20 §5.12.1] racy rows are not recorded
fn dirmap_rows(t: &Tree, rt: &Runtime, p: &Params) -> Vec<((String, u64), (String, i64))> {
    let g = t.caps.granularity();
    let t0 = p.stamp_ns.or_else(|| {
        rt.dirmap
            .iter()
            .filter(|((tree, _), _)| *tree == t.root)
            .map(|(_, (_, m))| *m)
            .max()
    });
    t.dirs
        .iter()
        .filter_map(|(path, d)| {
            let key = (t.root.clone(), d.id);
            if t0.is_some_and(|t0| tge(d.mtime_ns, t0, g)) {
                rt.dirmap.get(&key).map(|row| (key, row.clone()))
            } else {
                Some((key, (path.clone(), d.mtime_ns)))
            }
        })
        .collect()
}

/// The fingerprints a settle keys by `oid` for the contents it read ([40 §2.5] `FPRINT`): the text content at every
/// path of `paths` that is available ([F20 §2.4]: none beyond `files.max-read-bytes`), each `oid` once.
pub fn fingerprints_seen(
    fs: &Fs,
    tree: &str,
    paths: &[String],
    algo: Algo,
    max_read_bytes: Option<u64>,
) -> Vec<(Oid, Fingerprint)> {
    let t = &fs.trees[tree];
    let mut out: BTreeMap<Oid, Fingerprint> = BTreeMap::new();
    for p in paths {
        if let Ok(b) = read_content(t, p, max_read_bytes)
            && let Some(fp) = fingerprint(b)
        {
            out.entry(oid(algo, b)).or_insert(fp);
        }
    }
    out.into_iter().collect()
}

/// A settle over the file nodes `scope` in tree `tree` at `hlc` ([40 §4.2]; [API §12.6]): pass 1 resolves every node;
/// sibling inference takes, for each node, the directory moves (X/, Y/) that the exact candidates of E1, E3d, E3 and E6
/// of at least two other nodes show ([F20 §5.10]); pass 2 resolves again with them. The writer tree then writes every
/// re-bind the write rule allows (a `PENDING` promotion also needing [40 §5.3]'s own rule), the `planned → present`
/// bindings, the conflicts resolved by observation (LV-001), the path claims unified (LV-005), the anchor conflicts
/// settled (LV-004) and the automatic `path_moves` entries; a reader tree writes `PENDING` rows only; `FILEOBS` rows
/// follow every node the settle saw, `FPRINT` rows every content it read at a node's path, `DIRMAP` rows every
/// directory.
///
/// After a history verb changed a path, this is the settle that re-binds the link to where the file is on exact
/// evidence, or leaves it `missing` (LH-002); a path composed by a merge (`merge-compose/prefix`) is an expectation this
/// settle verifies like any other: where git did not compose the move, the pre-composition alias leads E4 to the file,
/// and the link re-binds only on exact evidence (LV-008).
// spec: [40 §4.2]; [40 §5.3] promotion; [API §12.6]; [F20 §5.10]; [F20 §5.16]
// rule: LH-002, LV-008
#[allow(clippy::too_many_arguments)]
pub fn settle(
    scope: &[u32],
    view: &View,
    fs: &Fs,
    git: &Git,
    rt: &Runtime,
    tree: &str,
    p: &Params,
    hlc: u64,
) -> SettleOut {
    let mut out = SettleOut::default();
    let by_n: BTreeMap<u32, &FileNode> = view.files.iter().map(|f| (f.n, f)).collect();
    let nodes: Vec<&FileNode> = scope.iter().filter_map(|n| by_n.get(n).copied()).collect();
    // Every resolution of this settle shares the prepared inputs: the git memo, the matcher, the spellings, the bound
    // paths and ids, the frontier and the contents' `oid`s.
    let prep = Prep::new(view, fs, git, rt, tree, p);
    // Pass 1 and sibling inference, from every exact candidate of E1, E3d, E3 and E6, whatever decided each state.
    let mut pass1: BTreeMap<u32, FileResult> =
        nodes.iter().map(|f| (f.n, prep.resolve(f, p))).collect();
    let mut support: BTreeMap<(String, String), BTreeSet<u32>> = BTreeMap::new();
    for f in &nodes {
        for q in &pass1[&f.n].sibling_exact {
            for s in splits(&f.path, q) {
                support.entry(s).or_default().insert(f.n);
            }
        }
    }
    let siblings_for = |n: u32| -> Vec<(String, String)> {
        support
            .iter()
            .filter(|(_, ns)| ns.iter().filter(|m| **m != n).count() >= 2)
            .map(|(k, _)| k.clone())
            .collect()
    };
    let (hist, head) = prep.git();
    let t = &fs.trees[tree];
    let mut seen_paths: Vec<String> = Vec::new();
    // The nodes this settle re-binds on exact evidence, for `PREFIXEV` ([F20 §5.16]: "re-bound exactly").
    let mut exact_rebinds: BTreeSet<u32> = BTreeSet::new();
    for f in &nodes {
        let p2 = Params {
            siblings: siblings_for(f.n),
            ..p.clone()
        };
        // Pass 2 differs from pass 1 only through the inferred sibling moves: without any, pass 1's result stands.
        let r = match pass1.remove(&f.n) {
            Some(r1) if p2.siblings == p.siblings => r1,
            _ => prep.resolve(f, &p2),
        };
        if let Some(q) = &r.at {
            seen_paths.push(q.clone());
        }
        // Composite conflicts: resolution by observation, under I-F6 like every automatic re-bind (on `main` only with
        // the chosen path committed in τ(H), [F18 §2.6] item 5).
        if f.conflict.is_some() {
            if let Some((q, relink)) = observation_in(f, &prep, p) {
                out.resolved
                    .push(rebind_of(f, &q, relink, t, hist, head, p));
            }
            out.results.insert(f.n, r);
            continue;
        }
        if f.status == FileStatus::Planned {
            if p.settle
                && p.writer
                && let Some(q) = &r.at
            {
                let rb = rebind_of(f, q, String::new(), t, hist, head, p);
                // On `main` the binding, too, waits for the commit (a spec finding of WP-92 for [F18 §2.6]).
                if !p.main || rb.observed_blob.is_some() {
                    out.binds.push(Binds {
                        n: f.n,
                        path: rb.to,
                        oid: rb.oid,
                        bytes: rb.bytes,
                        observed_git: rb.observed_git,
                        observed_blob: rb.observed_blob,
                    });
                }
            }
            out.results.insert(f.n, r);
            continue;
        }
        let promotion_ok = match r.evidence {
            Some((7, _)) => {
                // A promoted `PENDING` row (its class exact, [F20 §5.6]): T's own evidence exact for q, or E6 in T's
                // window showing p renamed to q by exact renames ([40 §5.3]).
                let own = prep.resolve(
                    f,
                    &Params {
                        no_pending: true,
                        ..p2.clone()
                    },
                );
                let e6 = match (hist, head, &r.at) {
                    (Some(hist), Some(h), Some(q)) => matches!(
                        hist.chain(&window_for(hist, h, f, p.skew_ns), &f.path),
                        Chain::Path { ref path, class: Class::Exact, .. } if path == q
                    ),
                    _ => false,
                };
                (own.state == State::MovedAuto && own.at == r.at) || e6
            }
            _ => true,
        };
        if if6_rebind_rule(&r, &p2, true)
            && promotion_ok
            && let (Some(q), Some(relink)) = (&r.at, relink_of(&r))
        {
            out.rebinds.push(rebind_of(f, q, relink, t, hist, head, p));
            if r.evidence.is_some() && r.guess.is_none() {
                exact_rebinds.insert(f.n);
            }
            for m in &r.committed_moves {
                let nm = NewMove {
                    from: m.0.clone(),
                    to: m.1.clone(),
                    committed: true,
                    git: Some(m.2.clone()),
                };
                if !out.moves.contains(&nm) {
                    out.moves.push(nm);
                }
            }
        } else if p.settle && !p.writer {
            // A reader tree keeps its observations as `PENDING` rows with their class and token ([F11 §12.6]).
            let seen = match (r.state, &r.at, r.evidence) {
                (State::MovedAuto, Some(q), Some((ev, _))) => Some((q.clone(), PClass::Exact, ev)),
                (State::MovedNeedsConfirm, _, _) => r
                    .proposals
                    .first()
                    .map(|x| (x.path.clone(), x.class, x.evidence)),
                _ => None,
            };
            if let Some((q, class, evidence)) = seen {
                out.pending.push(PendingRow {
                    n: f.n,
                    tree: tree.to_string(),
                    class,
                    source: PendingSource::ReaderSettle,
                    evidence,
                    oid: read_content(t, &q, p.max_read_bytes)
                        .ok()
                        .map(|b| oid(p.algo, b)),
                    from: f.path.clone(),
                    to: q,
                    hlc,
                });
            }
        }
        // LV-004: the node's conflicted anchors, against the content at the path it resolved to.
        if p.settle
            && p.writer
            && r.fresh
            && matches!(r.state, State::Ok | State::MovedAuto)
            && let Some(Ok(content)) =
                r.at.as_deref()
                    .map(|q| read_content(t, q, p.max_read_bytes))
        {
            for c in view.anchor_conflicts.iter().filter(|c| c.file == f.n) {
                if let Some(i) = settle_anchor_conflict(&c.sides, content, p.algo, &p.anchor) {
                    out.anchors.push((f.n, c.anchor, i));
                }
            }
        }
        out.fileobs.push((
            (f.n, tree.to_string()),
            row_after(f, &r, fs, tree, rt, hlc, p, promotion_ok),
        ));
        out.results.insert(f.n, r);
    }
    // LV-005: every (root, exact path) claimed by exactly two nodes of the scope.
    let mut claims: BTreeMap<(&str, &str), Vec<&FileNode>> = BTreeMap::new();
    for f in nodes.iter().filter(|f| f.path_claim && !f.tombstone) {
        claims.entry((&f.root, &f.path)).or_default().push(f);
    }
    for pair in claims.values().filter(|v| v.len() == 2) {
        if let Some((removed, kept)) = path_claim_in(pair[0], pair[1], &prep, p) {
            out.unified.push(Unified {
                removed,
                kept,
                relink: "git/r100".into(),
            });
        }
    }
    // `observed` path_moves entries ([F20 §5.16]): `PREFIXEV` accumulates, across settles, the nodes re-bound exactly
    // (never a guess policy B applied) from under from/ to to/; an entry is recorded once no linked present node is left
    // under from/, from/ is absent in T and at least 2 nodes moved.
    if p.settle && p.writer {
        let mut acc: BTreeMap<(String, String), BTreeSet<u32>> = BTreeMap::new();
        for rb in out
            .rebinds
            .iter()
            .filter(|rb| exact_rebinds.contains(&rb.n))
        {
            let root = by_n
                .get(&rb.n)
                .map_or_else(|| "project".to_string(), |g| g.root.clone());
            for (x, y) in splits(&rb.from, &rb.to) {
                let key = (tree.to_string(), root.clone(), x.clone(), y.clone());
                let e = acc
                    .entry((x, y))
                    .or_insert_with(|| rt.prefixev.get(&key).cloned().unwrap_or_default());
                e.insert(rb.n);
            }
        }
        let moved: BTreeMap<u32, String> =
            out.rebinds.iter().map(|rb| (rb.n, rb.to.clone())).collect();
        let mut observed: Vec<(String, String)> = Vec::new();
        for ((x, y), ns) in &acc {
            let left = view.files.iter().any(|g| {
                !g.tombstone
                    && g.status == FileStatus::Present
                    && moved.get(&g.n).unwrap_or(&g.path).starts_with(x.as_str())
            });
            let dir = x.trim_end_matches('/');
            if ns.len() >= 2 && !left && t.dir_id(dir).is_none() {
                observed.push((x.clone(), y.clone()));
            }
            let root = ns
                .iter()
                .find_map(|n| by_n.get(n))
                .map_or_else(|| "project".to_string(), |g| g.root.clone());
            out.prefixev
                .push(((tree.to_string(), root, x.clone(), y.clone()), ns.clone()));
        }
        for (x, y) in &observed {
            let implied = observed.iter().any(|(x2, y2)| {
                x2.len() < x.len()
                    && x.starts_with(x2.as_str())
                    && *y == format!("{y2}{}", &x[x2.len()..])
            });
            let recorded = out.moves.iter().any(|m| m.from == *x && m.to == *y);
            if !implied && !recorded {
                out.moves.push(NewMove {
                    from: x.clone(),
                    to: y.clone(),
                    committed: false,
                    git: None,
                });
            }
        }
        out.moves.sort();
    }
    out.fprint = fingerprints_seen(fs, tree, &seen_paths, p.algo, p.max_read_bytes);
    out.dirmap = dirmap_rows(t, rt, p);
    out
}

/// The re-bind of a node to q: the content's `oid` (absent when the content is unavailable, [F20 §2.4]) and the raw
/// size, `observed_git` = H, `observed_blob` = git's blob at q in τ(H) or empty ([40 §4.3] "What settle writes").
fn rebind_of(
    f: &FileNode,
    q: &str,
    relink: String,
    t: &Tree,
    hist: Option<&Hist<'_>>,
    head: Option<&str>,
    p: &Params,
) -> Rebind {
    Rebind {
        n: f.n,
        from: f.path.clone(),
        to: q.to_string(),
        relink,
        oid: read_content(t, q, p.max_read_bytes)
            .ok()
            .map(|b| oid(p.algo, b)),
        bytes: match t.stat(q) {
            StatOut::Present(s) => s.size,
            _ => 0,
        },
        observed_git: head.map(str::to_string),
        observed_blob: match (hist, head) {
            (Some(hist), Some(h)) => hist.repo.tau(h).get(q).cloned(),
            _ => None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::r4::cascade::Side;
    use crate::r4::git::{Commit, Repo};
    use crate::r4::path::Os;
    use crate::r4::tree::{TreeOp, VolumeCaps};

    const R: &str = "C:/work/trunk";

    type CommitSpec<'a> = (&'a str, &'a [&'a str], &'a [(&'a str, &'a str)]);

    fn repo(cs: &[CommitSpec<'_>], head: &str) -> Git {
        let mut g = Git::default();
        g.repos.insert(
            "r".into(),
            Repo {
                algo: Algo::Sha1,
                commits: cs
                    .iter()
                    .map(|(id, ps, tr)| {
                        (
                            id.to_string(),
                            Commit {
                                id: id.to_string(),
                                parents: ps.iter().map(|x| x.to_string()).collect(),
                                committer_time: 100,
                                author_time: 100,
                                tree: tr
                                    .iter()
                                    .map(|(a, b)| (a.to_string(), b.to_string()))
                                    .collect(),
                            },
                        )
                    })
                    .collect(),
                refs: BTreeMap::from([("refs/heads/integ".to_string(), head.to_string())]),
                blobs: BTreeMap::new(),
            },
        );
        g.heads
            .insert(R.into(), ("r".into(), Head::Ref("refs/heads/integ".into())));
        g
    }

    fn node(n: u32, path: &str, g: &str) -> FileNode {
        FileNode {
            n,
            root: "project".into(),
            path: path.into(),
            oid: Some(oid(Algo::Sha1, b"body\n")),
            bytes: Some(5),
            observed_git: Some(g.into()),
            observed_blob: None,
            relink: None,
            aliases: Vec::new(),
            status: FileStatus::Present,
            artifact_kind: None,
            tombstone: false,
            obs_hlc: 0,
            conflict: None,
            path_claim: false,
        }
    }

    fn fs(files: &[&str]) -> Fs {
        let mut f = Fs::default();
        f.ensure_tree(R, "D", VolumeCaps::NTFS, Os::Windows);
        for p in files {
            f.apply(
                R,
                &TreeOp::Write {
                    path: p.to_string(),
                    bytes: b"body\n".to_vec(),
                    btime_ns: None,
                },
                1,
            )
            .unwrap();
        }
        f
    }

    #[test]
    fn designation_and_the_writer_tree() {
        let git = repo(&[("c1", &[], &[])], "c1");
        let b = vec![Binding {
            dir: R.into(),
            branch: "main".into(),
            designated: true,
            expected_ref: Some("integ".into()),
            base: None,
        }];
        let d = designation(&b, None, None);
        assert!(writer_tree("main", R, &d, &git));
        assert!(!writer_tree("lane/x", R, &d, &git));
        let d2 = designation(&[], Some(R), Some("refs/heads/other"));
        assert!(
            !writer_tree("main", R, &d2, &git),
            "HEAD names another branch"
        );
        assert_eq!(
            reader_note_for("main", R, &d2, &git, "trunk").as_deref(),
            Some("reading only: tree on integ, branch expects other")
        );
        // Two rows designating one branch: fail closed.
        let mut b2 = b.clone();
        b2.push(Binding {
            dir: "C:/other".into(),
            ..b[0].clone()
        });
        assert!(designation(&b2, None, None).is_empty());
    }

    #[test]
    fn a_settle_writes_exact_rebinds_and_a_committed_move() {
        let git = repo(
            &[
                ("c1", &[], &[("docs/a.md", "1"), ("docs/b.md", "2")]),
                ("c2", &["c1"], &[("arch/a.md", "1"), ("arch/b.md", "2")]),
            ],
            "c2",
        );
        let f = fs(&["arch/a.md", "arch/b.md"]);
        let view = View {
            files: vec![node(1, "docs/a.md", "c1"), node(2, "docs/b.md", "c1")],
            moves: BTreeMap::new(),
            anchor_conflicts: Vec::new(),
        };
        let p = Params {
            settle: true,
            writer: true,
            main: true,
            ..Params::default()
        };
        let out = settle(
            &[1, 2],
            &view,
            &f,
            &git,
            &Runtime::default(),
            R,
            &p,
            7 << 16,
        );
        assert_eq!(out.rebinds.len(), 2);
        assert_eq!(out.rebinds[0].relink, "git/r100");
        assert_eq!(out.rebinds[0].observed_blob.as_deref(), Some("1"));
        assert_eq!(
            out.moves,
            vec![NewMove {
                from: "docs/".into(),
                to: "arch/".into(),
                committed: true,
                git: Some("c2".into())
            }]
        );
        // A reader tree writes PENDING rows only.
        let pr = Params {
            settle: true,
            writer: false,
            ..Params::default()
        };
        let out = settle(
            &[1, 2],
            &view,
            &f,
            &git,
            &Runtime::default(),
            R,
            &pr,
            7 << 16,
        );
        assert!(out.rebinds.is_empty());
        assert_eq!(out.pending.len(), 2);
    }

    #[test]
    fn prefixev_accumulates_observed_moves_across_settles() {
        // Three linked files under old/; two move by file id before the first settle, the third before the second.
        let git = Git::default();
        let mut f = fs(&["old/a.md", "old/b.md", "old/c.md"]);
        let mut rt = Runtime::default();
        let view = View {
            files: vec![
                node(1, "old/a.md", "c"),
                node(2, "old/b.md", "c"),
                node(3, "old/c.md", "c"),
            ]
            .into_iter()
            .map(|mut n| {
                n.observed_git = None;
                n
            })
            .collect(),
            moves: BTreeMap::new(),
            anchor_conflicts: Vec::new(),
        };
        for (n, path) in [(1u32, "old/a.md"), (2, "old/b.md"), (3, "old/c.md")] {
            let StatOut::Present(s) = f.trees[R].stat(path) else {
                panic!()
            };
            rt.fileobs.insert(
                (n, R.into()),
                FileObs {
                    file_id: Some(s.id.clone()),
                    size: s.size,
                    mtime_ns: s.mtime_ns,
                    creation_ns: Some(s.btime_ns),
                    ..FileObs::default()
                },
            );
        }
        f.apply(
            R,
            &TreeOp::Mv {
                from: "old/a.md".into(),
                to: "new/a.md".into(),
            },
            2,
        )
        .unwrap();
        f.apply(
            R,
            &TreeOp::Mv {
                from: "old/b.md".into(),
                to: "new/b.md".into(),
            },
            2,
        )
        .unwrap();
        let p = Params {
            settle: true,
            writer: true,
            ..Params::default()
        };
        let out = settle(&[1, 2, 3], &view, &f, &git, &rt, R, &p, 3 << 16);
        assert_eq!(out.rebinds.len(), 2);
        assert!(out.moves.is_empty(), "old/c.md is still under old/");
        for (k, v) in &out.prefixev {
            rt.prefixev.insert(k.clone(), v.clone());
        }
        let mut files = view.files.clone();
        for rb in &out.rebinds {
            let x = files.iter_mut().find(|x| x.n == rb.n).unwrap();
            x.aliases.push(rb.from.clone());
            x.path = rb.to.clone();
        }
        f.apply(
            R,
            &TreeOp::Mv {
                from: "old/c.md".into(),
                to: "new/c.md".into(),
            },
            4,
        )
        .unwrap();
        f.apply(R, &TreeOp::Rm { path: "old".into() }, 4).unwrap();
        let view2 = View {
            files,
            moves: BTreeMap::new(),
            anchor_conflicts: Vec::new(),
        };
        let out2 = settle(&[1, 2, 3], &view2, &f, &git, &rt, R, &p, 5 << 16);
        assert_eq!(out2.rebinds.len(), 1);
        assert_eq!(
            out2.moves,
            vec![NewMove {
                from: "old/".into(),
                to: "new/".into(),
                committed: false,
                git: None
            }]
        );
    }

    #[test]
    fn main_records_committed_observations_only() {
        let git = repo(&[("c1", &[], &[("a.md", "1")])], "c1");
        let mut f = fs(&["a.md"]);
        f.apply(
            R,
            &TreeOp::Mv {
                from: "a.md".into(),
                to: "b.md".into(),
            },
            2,
        )
        .unwrap();
        let mut n = node(1, "a.md", "c1");
        n.observed_git = Some("c1".into());
        let view = View {
            files: vec![n.clone()],
            moves: BTreeMap::new(),
            anchor_conflicts: Vec::new(),
        };
        let mut rt = Runtime::default();
        let StatOut::Present(s) = f.trees[R].stat("b.md") else {
            panic!()
        };
        rt.fileobs.insert(
            (1, R.into()),
            FileObs {
                file_id: Some(s.id.clone()),
                size: s.size,
                mtime_ns: s.mtime_ns,
                creation_ns: Some(s.btime_ns),
                ..FileObs::default()
            },
        );
        let p = Params {
            settle: true,
            writer: true,
            main: true,
            ..Params::default()
        };
        let out = settle(&[1], &view, &f, &git, &rt, R, &p, 9 << 16);
        assert!(out.rebinds.is_empty(), "b.md is not in τ(H)");
        assert_eq!(out.results[&1].details[1].code, 9);
        let p_lane = Params { main: false, ..p };
        assert_eq!(
            settle(&[1], &view, &f, &git, &rt, R, &p_lane, 9 << 16)
                .rebinds
                .len(),
            1
        );
    }

    #[test]
    fn conflicts_resolve_by_observation_when_fresh_for_every_side() {
        let git = repo(
            &[
                ("c1", &[], &[("b.rs", "1")]),
                ("c2", &["c1"], &[("c.rs", "1")]),
            ],
            "c2",
        );
        let f = fs(&["c.rs"]);
        let mut n = node(1, "b.rs", "c1");
        n.conflict = Some(Box::new([
            Side {
                path: "b.rs".into(),
                oid: n.oid.clone(),
                observed_git: Some("c1".into()),
                observed_blob: None,
            },
            Side {
                path: "c.rs".into(),
                oid: n.oid.clone(),
                observed_git: Some("c2".into()),
                observed_blob: None,
            },
        ]));
        let view = View {
            files: vec![n.clone()],
            moves: BTreeMap::new(),
            anchor_conflicts: Vec::new(),
        };
        let p = Params {
            settle: true,
            writer: true,
            ..Params::default()
        };
        assert_eq!(
            settle_observation(&n, &view, &f, &git, &Runtime::default(), R, &p),
            Some(("c.rs".into(), "merge-observation/r100".into()))
        );
        // Not fresh for the c.rs side (its commit is unknown here): the conflict stays.
        let mut n2 = n.clone();
        n2.conflict.as_mut().unwrap()[1].observed_git = Some("zz".into());
        let git_old = repo(&[("c1", &[], &[("b.rs", "1")])], "c1");
        assert_eq!(
            settle_observation(&n2, &view, &f, &git_old, &Runtime::default(), R, &p),
            None
        );
    }

    #[test]
    fn strict_links_and_uncommitted() {
        assert!(strict_links_refuses(&[State::Ok, State::Replaced]));
        assert!(!strict_links_refuses(&[State::Ok, State::Pending]));
        let mut a = node(1, "a", "c1");
        a.relink = Some("explicit/intent".into());
        let mut b = node(2, "b", "c1");
        b.relink = Some("git/r100".into());
        b.observed_blob = Some("x".into());
        assert_eq!(uncommitted_rebinds(&[a, b]), vec![1]);
    }

    fn text(tag: &str) -> String {
        (0..8)
            .map(|i| format!("{tag} line number {i} of a file\n"))
            .collect()
    }

    fn fs_of(files: &[(&str, &str)]) -> Fs {
        let mut f = Fs::default();
        f.ensure_tree(R, "D", VolumeCaps::NTFS, Os::Windows);
        for (p, b) in files {
            f.apply(
                R,
                &TreeOp::Write {
                    path: p.to_string(),
                    bytes: b.as_bytes().to_vec(),
                    btime_ns: None,
                },
                1,
            )
            .unwrap();
        }
        f
    }

    fn node_of(n: u32, path: &str, content: &str) -> FileNode {
        FileNode {
            oid: Some(oid(Algo::Sha1, content.as_bytes())),
            bytes: Some(content.len() as u64),
            observed_git: None,
            ..node(n, path, "c1")
        }
    }

    fn view_of(files: Vec<FileNode>) -> View {
        View {
            files,
            moves: BTreeMap::new(),
            anchor_conflicts: Vec::new(),
        }
    }

    fn writer() -> Params {
        Params {
            settle: true,
            writer: true,
            ..Params::default()
        }
    }

    fn obs_at(fs: &Fs, path: &str) -> FileObs {
        let StatOut::Present(s) = fs.trees[R].stat(path) else {
            panic!("{path}")
        };
        FileObs {
            file_id: Some(s.id.clone()),
            parent_dir: Some(s.parent),
            size: s.size,
            mtime_ns: s.mtime_ns,
            creation_ns: Some(s.btime_ns),
            ..FileObs::default()
        }
    }

    #[test]
    fn lv_004_a_settle_keeps_the_anchor_side_that_resolves_fresh() {
        use crate::r4::anchor::{Form, capture};
        use crate::r4::cascade::AnchorConflict;
        let v1 = "fn a() {}\nlet ours = 1;\nfn b() {}\n";
        let v2 = "fn a() {}\nlet theirs = 2;\nfn b() {}\n";
        let cap_on = |content: &str| {
            capture(
                Uid([1; 16]),
                Uid([2; 16]),
                &Form::Lines(2, 2),
                Some(content.as_bytes()),
                None,
                None,
                Algo::Sha1,
                &[],
                &Consts::DRAFT,
            )
            .unwrap()
            .0
        };
        let (ours, theirs) = (cap_on(v1), cap_on(v2));
        // The merged tree holds theirs: only their selectors resolve fresh.
        let fs = fs_of(&[("a.rs", v2)]);
        let f = node_of(1, "a.rs", v2);
        let mut view = view_of(vec![f.clone()]);
        view.anchor_conflicts.push(AnchorConflict {
            file: 1,
            anchor: ours.uid,
            sides: Box::new([ours.clone(), theirs.clone()]),
        });
        let out = settle(
            &[1],
            &view,
            &fs,
            &Git::default(),
            &Runtime::default(),
            R,
            &writer(),
            5 << 16,
        );
        assert_eq!(out.anchors, vec![(1, ours.uid, 1)]);
        // Neither side fresh, or a reader tree: the conflict stays.
        let fs3 = fs_of(&[("a.rs", "fn a() {}\nlet both = 3;\nfn b() {}\n")]);
        assert!(
            settle(
                &[1],
                &view,
                &fs3,
                &Git::default(),
                &Runtime::default(),
                R,
                &writer(),
                5 << 16
            )
            .anchors
            .is_empty()
        );
        let reader = Params {
            writer: false,
            ..writer()
        };
        assert!(
            settle(
                &[1],
                &view,
                &fs,
                &Git::default(),
                &Runtime::default(),
                R,
                &reader,
                5 << 16
            )
            .anchors
            .is_empty()
        );
    }

    #[test]
    fn lv_005_a_path_claim_unifies_only_in_a_fresh_writer_tree() {
        let body = text("claimed");
        // c2 renames a.md to the claimed b.md; c3 renames it on to c.md, so b.md is no longer committed at H.
        let git = repo(
            &[
                ("c1", &[], &[("a.md", "B")]),
                ("c2", &["c1"], &[("b.md", "B")]),
                ("c3", &["c2"], &[("c.md", "B")]),
            ],
            "c3",
        );
        let fs = fs_of(&[("c.md", &body)]);
        let mut a = node_of(1, "b.md", &body);
        a.aliases = vec!["a.md".into()];
        a.observed_git = Some("c1".into());
        a.path_claim = true;
        let mut b = node_of(2, "b.md", &body);
        b.observed_git = Some("c2".into());
        b.path_claim = true;
        let view = view_of(vec![a.clone(), b.clone()]);
        let out = settle(
            &[1, 2],
            &view,
            &fs,
            &git,
            &Runtime::default(),
            R,
            &writer(),
            5 << 16,
        );
        assert_eq!(
            out.unified,
            vec![Unified {
                removed: 1,
                kept: 2,
                relink: "git/r100".into()
            }]
        );
        assert_eq!(
            out.results[&1].details[0].code, 28,
            "rendered PathClaim until then"
        );
        // A reader tree never unifies.
        let reader = Params {
            writer: false,
            ..writer()
        };
        assert!(
            settle(
                &[1, 2],
                &view,
                &fs,
                &git,
                &Runtime::default(),
                R,
                &reader,
                5 << 16
            )
            .unified
            .is_empty()
        );
        // A writer tree that is not fresh for one of them (its observation's commit is unknown here, its path is not
        // committed at H, and its time-bounded window holds no event of its path) does not either.
        let mut b2 = b.clone();
        b2.observed_git = Some("elsewhere".into());
        b2.obs_hlc = u64::MAX >> 1;
        let view2 = view_of(vec![a, b2]);
        assert!(
            settle(
                &[1, 2],
                &view2,
                &fs,
                &git,
                &Runtime::default(),
                R,
                &writer(),
                5 << 16
            )
            .unified
            .is_empty()
        );
    }

    #[test]
    fn lv_008_a_composed_path_is_verified_by_the_next_settle_on_exact_evidence() {
        let body = text("composed");
        // git did not compose the move: the file is still at the pre-composition path.
        let fs = fs_of(&[("docs/new.md", &body)]);
        let mut f = node_of(1, "arch/new.md", &body);
        f.aliases = vec!["docs/new.md".into()];
        f.relink = Some("merge-compose/prefix".into());
        let mut rt = Runtime::default();
        rt.fileobs.insert((1, R.into()), obs_at(&fs, "docs/new.md"));
        let out = settle(
            &[1],
            &view_of(vec![f.clone()]),
            &fs,
            &Git::default(),
            &rt,
            R,
            &writer(),
            5 << 16,
        );
        assert_eq!(out.rebinds.len(), 1);
        // arch/ does not exist, so E3d's parent-directory id finds docs/new.md first.
        assert_eq!(
            (out.rebinds[0].to.as_str(), out.rebinds[0].relink.as_str()),
            ("docs/new.md", "lazy/dir-id")
        );
        // Without exact evidence (no FILEOBS row here) the alias leads E4 to an identical-copy proposal only.
        let out = settle(
            &[1],
            &view_of(vec![f.clone()]),
            &fs,
            &Git::default(),
            &Runtime::default(),
            R,
            &writer(),
            5 << 16,
        );
        assert!(out.rebinds.is_empty());
        assert_eq!(out.results[&1].state, State::MovedNeedsConfirm);
        // git composed it: the path holds, nothing is written.
        let fs2 = fs_of(&[("arch/new.md", &body)]);
        let out = settle(
            &[1],
            &view_of(vec![f]),
            &fs2,
            &Git::default(),
            &Runtime::default(),
            R,
            &writer(),
            5 << 16,
        );
        assert!(out.rebinds.is_empty());
        assert_eq!(out.results[&1].state, State::Ok);
    }

    #[test]
    fn a_pending_row_is_promoted_only_by_own_exact_evidence_or_an_exact_rename() {
        let old = text("old");
        let near = old.replacen("line number 7 of a file", "line number 7 of a fil", 1);
        let mk = |blob_b: &str| {
            let mut g = repo(
                &[
                    ("c1", &[], &[("a.md", "X")]),
                    ("c2", &["c1"], &[("b.md", blob_b)]),
                ],
                "c2",
            );
            let r = g.repos.get_mut("r").unwrap();
            r.blobs.insert("X".into(), old.clone().into_bytes());
            r.blobs.insert("Y".into(), near.clone().into_bytes());
            g
        };
        let mut f = node_of(1, "a.md", &old);
        f.observed_git = Some("c1".into());
        let pending = |content: &str| Runtime {
            pending: vec![PendingRow {
                n: 1,
                tree: "E:/other".into(),
                class: PClass::Exact,
                source: PendingSource::ReaderSettle,
                evidence: 3,
                oid: Some(oid(Algo::Sha1, content.as_bytes())),
                from: "a.md".into(),
                to: "b.md".into(),
                hlc: 1,
            }],
            ..Runtime::default()
        };
        // E6 shows only a strong pair (not an exact rename) and this tree has no exact evidence of its own.
        let fs = fs_of(&[("b.md", &near)]);
        let out = settle(
            &[1],
            &view_of(vec![f.clone()]),
            &fs,
            &mk("Y"),
            &pending(&near),
            R,
            &writer(),
            5 << 16,
        );
        assert_eq!(out.results[&1].evidence, Some((7, "lazy")));
        assert!(out.rebinds.is_empty(), "{:?}", out.rebinds);
        // The file id row does not follow the unpromoted observation.
        assert_eq!(out.fileobs[0].1.file_id, None);
        // An exact rename in the window promotes it.
        let fs = fs_of(&[("b.md", &old)]);
        let out = settle(
            &[1],
            &view_of(vec![f]),
            &fs,
            &mk("X"),
            &pending(&old),
            R,
            &writer(),
            5 << 16,
        );
        assert_eq!(out.rebinds.len(), 1);
        assert_eq!(out.rebinds[0].relink, "lazy/pending");
    }

    #[test]
    fn sibling_inference_counts_every_exact_candidate_of_other_nodes() {
        use crate::r4::cascade::Intent;
        let (a, b, c) = (text("a"), text("b"), text("c"));
        // A and B each have two exact candidates from E1 (two intents): both `ambiguous`, both support X/ → Y/.
        let fs = fs_of(&[
            ("Y/a.md", &a),
            ("Z/a.md", &a),
            ("Y/b.md", &b),
            ("Z/b.md", &b),
            ("Y/c.md", &text("c changed")),
        ]);
        let intents = ["a", "b"]
            .iter()
            .flat_map(|x| {
                ["Y", "Z"].map(|d| Intent {
                    tree: R.into(),
                    items: vec![(format!("X/{x}.md"), Some(format!("{d}/{x}.md")))],
                    open: true,
                    recovered: false,
                })
            })
            .collect();
        let rt = Runtime {
            intents,
            ..Runtime::default()
        };
        let view = view_of(vec![
            node_of(1, "X/a.md", &a),
            node_of(2, "X/b.md", &b),
            node_of(3, "X/c.md", &c),
        ]);
        let out = settle(
            &[1, 2, 3],
            &view,
            &fs,
            &Git::default(),
            &rt,
            R,
            &writer(),
            5 << 16,
        );
        assert_eq!(out.results[&1].state, State::Ambiguous);
        let r3 = &out.results[&3];
        assert_eq!(
            (r3.state, r3.details[0].code),
            (State::MovedNeedsConfirm, 12)
        );
        assert_eq!(r3.proposals[0].path, "Y/c.md");
    }

    #[test]
    fn a_reader_tree_records_its_observations_with_their_class() {
        let body = text("copied");
        let mut fs = fs_of(&[("a.md", &body)]);
        fs.apply(
            R,
            &TreeOp::Cp {
                from: "a.md".into(),
                to: "b.md".into(),
                keep_btime: false,
            },
            2,
        )
        .unwrap();
        fs.apply(
            R,
            &TreeOp::Rm {
                path: "a.md".into(),
            },
            2,
        )
        .unwrap();
        let reader = Params {
            settle: true,
            writer: false,
            ..Params::default()
        };
        let f = node_of(1, "a.md", &body);
        let out = settle(
            &[1],
            &view_of(vec![f]),
            &fs,
            &Git::default(),
            &Runtime::default(),
            R,
            &reader,
            5 << 16,
        );
        assert!(out.rebinds.is_empty());
        assert_eq!(out.pending.len(), 1);
        let row = &out.pending[0];
        assert_eq!(
            (row.class, row.source, row.evidence, row.to.as_str()),
            (PClass::Copy, PendingSource::ReaderSettle, 13, "b.md")
        );
    }

    #[test]
    fn a_settle_records_fingerprints_and_directory_rows_except_racy_ones() {
        let body = text("seen");
        let mut fs = fs_of(&[("d/a.md", &body)]);
        fs.apply(
            R,
            &TreeOp::Write {
                path: "e/new.md".into(),
                bytes: b"x\n".to_vec(),
                btime_ns: None,
            },
            9_000,
        )
        .unwrap();
        let f = node_of(1, "d/a.md", &body);
        let p = Params {
            stamp_ns: Some(9_000),
            ..writer()
        };
        let out = settle(
            &[1],
            &view_of(vec![f]),
            &fs,
            &Git::default(),
            &Runtime::default(),
            R,
            &p,
            5 << 16,
        );
        assert_eq!(
            out.fprint,
            vec![(
                oid(Algo::Sha1, body.as_bytes()),
                fingerprint(body.as_bytes()).unwrap()
            )]
        );
        let paths: Vec<&str> = out.dirmap.iter().map(|(_, (p, _))| p.as_str()).collect();
        assert_eq!(
            paths,
            ["d"],
            "the root and e/ changed at the stamp's tick: racy, not recorded"
        );
    }

    /// LV-001 under I-F6 ([F18 §2.6] item 5): on `main` a composite conflict resolves by observation only to a path
    /// committed in τ(H) of root `project`, exactly as a re-bind would; a lane resolves it at once.
    #[test]
    fn lv_001_on_main_resolves_only_a_committed_observation() {
        let mut f = fs(&["a.rs"]);
        f.apply(
            R,
            &TreeOp::Mv {
                from: "a.rs".into(),
                to: "b.rs".into(),
            },
            2,
        )
        .unwrap();
        let mut n = node(1, "a.rs", "c1");
        n.conflict = Some(Box::new([
            Side {
                path: "a.rs".into(),
                oid: n.oid.clone(),
                observed_git: Some("c1".into()),
                observed_blob: Some("1".into()),
            },
            Side {
                path: "b.rs".into(),
                oid: n.oid.clone(),
                observed_git: Some("c1".into()),
                observed_blob: None,
            },
        ]));
        let view = view_of(vec![n]);
        let mut rt = Runtime::default();
        rt.fileobs.insert((1, R.into()), obs_at(&f, "b.rs"));
        let main = Params {
            main: true,
            ..writer()
        };
        // b.rs is present but not committed: the same move without a conflict would not be recorded on `main`.
        let uncommitted = repo(&[("c1", &[], &[("a.rs", "1")])], "c1");
        let out = settle(&[1], &view, &f, &uncommitted, &rt, R, &main, 5 << 16);
        assert!(out.resolved.is_empty(), "{:?}", out.resolved);
        assert_eq!(
            settle_observation(&view.files[0], &view, &f, &uncommitted, &rt, R, &main),
            None
        );
        let lane = settle(&[1], &view, &f, &uncommitted, &rt, R, &writer(), 5 << 16);
        assert_eq!(lane.resolved.len(), 1);
        assert_eq!(lane.resolved[0].relink, "merge-observation/file-id");
        // Once the move is committed, `main` resolves it, with the blob recorded.
        let committed = repo(
            &[
                ("c1", &[], &[("a.rs", "1")]),
                ("c2", &["c1"], &[("b.rs", "1")]),
            ],
            "c2",
        );
        let out = settle(&[1], &view, &f, &committed, &rt, R, &main, 5 << 16);
        assert_eq!(out.resolved.len(), 1);
        assert_eq!(
            (
                out.resolved[0].to.as_str(),
                out.resolved[0].observed_blob.as_deref()
            ),
            ("b.rs", Some("1"))
        );
        // The same conflict on a named root, whose tree has no git: on `main` it stays although the project
        // repository holds the same root-relative b.rs at H (a re-bind of the node would not be recorded either); a
        // lane resolves it at once.
        const M: &str = "C:/memory";
        let mut fm = f.clone();
        fm.ensure_tree(M, "M", VolumeCaps::NTFS, Os::Windows);
        fm.apply(
            M,
            &TreeOp::Write {
                path: "a.rs".into(),
                bytes: b"body\n".to_vec(),
                btime_ns: None,
            },
            1,
        )
        .unwrap();
        fm.apply(
            M,
            &TreeOp::Mv {
                from: "a.rs".into(),
                to: "b.rs".into(),
            },
            2,
        )
        .unwrap();
        let mut m = view.files[0].clone();
        m.root = "memory".into();
        let mview = view_of(vec![m]);
        let StatOut::Present(s) = fm.trees[M].stat("b.rs") else {
            panic!("b.rs")
        };
        let mut mrt = Runtime::default();
        mrt.fileobs.insert(
            (1, M.into()),
            FileObs {
                file_id: Some(s.id.clone()),
                parent_dir: Some(s.parent),
                size: s.size,
                mtime_ns: s.mtime_ns,
                creation_ns: Some(s.btime_ns),
                ..FileObs::default()
            },
        );
        let named = |p: &Params| Params {
            named_roots: BTreeMap::from([("memory".to_string(), M.to_string())]),
            ..p.clone()
        };
        let on_main = named(&main);
        assert_eq!(
            settle_observation(&mview.files[0], &mview, &fm, &committed, &mrt, R, &on_main),
            None
        );
        let out = settle(&[1], &mview, &fm, &committed, &mrt, R, &on_main, 5 << 16);
        assert!(out.resolved.is_empty(), "{:?}", out.resolved);
        assert_eq!(
            settle_observation(
                &mview.files[0],
                &mview,
                &fm,
                &committed,
                &mrt,
                R,
                &named(&writer())
            ),
            Some(("b.rs".into(), "merge-observation/file-id".into()))
        );
    }

    /// A volume without creation times (`VolumeCaps.btime = absent`) records no `FILEOBS.creation` ([F20 §5.18]).
    #[test]
    fn a_volume_without_creation_times_records_no_creation_time() {
        let mut fat = Fs::default();
        fat.ensure_tree(R, "F", VolumeCaps::FAT, Os::Windows);
        fat.apply(
            R,
            &TreeOp::Write {
                path: "a.md".into(),
                bytes: b"body\n".to_vec(),
                btime_ns: None,
            },
            1,
        )
        .unwrap();
        let mut n = node(1, "a.md", "c1");
        n.observed_git = None;
        let view = view_of(vec![n]);
        let run = |fs: &Fs| {
            settle(
                &[1],
                &view,
                fs,
                &Git::default(),
                &Runtime::default(),
                R,
                &writer(),
                5 << 16,
            )
        };
        let out = run(&fat);
        assert!(out.fileobs[0].1.file_id.is_some());
        assert_eq!(out.fileobs[0].1.creation_ns, None);
        assert!(run(&fs(&["a.md"])).fileobs[0].1.creation_ns.is_some());
    }

    /// `PREFIXEV` counts nodes re-bound exactly ([F20 §5.16]); a guess `files.policy.auto = strong` applies is no
    /// evidence of a directory move.
    #[test]
    fn prefixev_counts_exact_rebinds_only_never_policy_guesses() {
        let mut f = fs(&["old/a.md", "old/b.md"]);
        let mut rt = Runtime::default();
        for (n, p) in [(1u32, "old/a.md"), (2, "old/b.md")] {
            rt.fileobs.insert((n, R.into()), obs_at(&f, p));
        }
        for x in ["a", "b"] {
            f.apply(
                R,
                &TreeOp::Mv {
                    from: format!("old/{x}.md"),
                    to: format!("new/{x}.md"),
                },
                2,
            )
            .unwrap();
            // Edited in place after the move: E3 finds the id with other content, a strong `file-id-edited`.
            f.apply(
                R,
                &TreeOp::Write {
                    path: format!("new/{x}.md"),
                    bytes: b"body\nand more\n".to_vec(),
                    btime_ns: None,
                },
                3,
            )
            .unwrap();
        }
        f.apply(R, &TreeOp::Rm { path: "old".into() }, 3).unwrap();
        let mut a = node(1, "old/a.md", "c1");
        let mut b = node(2, "old/b.md", "c1");
        a.observed_git = None;
        b.observed_git = None;
        let view = view_of(vec![a, b]);
        let pb = Params {
            policy_strong: true,
            ..writer()
        };
        let out = settle(&[1, 2], &view, &f, &Git::default(), &rt, R, &pb, 5 << 16);
        assert_eq!(out.rebinds.len(), 2);
        assert!(
            out.rebinds
                .iter()
                .all(|rb| rb.relink == "policy/file-id-edited"),
            "{:?}",
            out.rebinds
        );
        assert!(out.prefixev.is_empty(), "{:?}", out.prefixev);
        assert!(out.moves.is_empty(), "{:?}", out.moves);
    }

    /// LV-005 removes a `present` claimant only (TR-081): a `planned` claimant whose alias is the rename's source keeps
    /// the conflict for `links fix`.
    #[test]
    fn lv_005_never_removes_a_planned_claimant() {
        let body = text("claimed");
        let git = repo(
            &[
                ("c1", &[], &[("a.md", "B")]),
                ("c2", &["c1"], &[("b.md", "B")]),
            ],
            "c2",
        );
        let fs = fs_of(&[("b.md", &body)]);
        let mut a = node_of(1, "b.md", &body);
        a.aliases = vec!["a.md".into()];
        a.observed_git = Some("c1".into());
        a.path_claim = true;
        let mut b = node_of(2, "b.md", &body);
        b.observed_git = Some("c2".into());
        b.path_claim = true;
        let run = |a: &FileNode| {
            settle(
                &[1, 2],
                &view_of(vec![a.clone(), b.clone()]),
                &fs,
                &git,
                &Runtime::default(),
                R,
                &writer(),
                5 << 16,
            )
            .unified
        };
        assert_eq!(
            run(&a),
            vec![Unified {
                removed: 1,
                kept: 2,
                relink: "git/r100".into()
            }]
        );
        a.status = FileStatus::Planned;
        assert!(run(&a).is_empty());
    }
}
