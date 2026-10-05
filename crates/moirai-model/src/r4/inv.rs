//! The R4 invariants I-F1…I-F14 as predicates of the model ([F18 §2]; [F13 §3.9] names the `r4::` model functions).
//! Each takes the data its statement is about and reports the violations it finds; the trace predicates of I-F5 and
//! I-F11 (appends and `ProjectFs` calls) are M6's, over the engine's trace ([F13 §3.9]).

use crate::r4::anchor::{AResult, Anchor, Consts, Content, Kind, resolve};
use crate::r4::cascade::{FileNode, FileObs, FileResult};
use crate::r4::git::Git;
use crate::r4::path::{abspath, dir_prefix, stored_rel_path};
use crate::r4::settle::{Designated, Rebind};
use crate::r4::strings::{State, relink_valid};
use crate::r4::text::oid;
use crate::r4::tree::{Btime, Fs};
use crate::r4::uid::{FileStatus, captured, uid_anchor, uid_file, uid_root};
use crate::value::{Algo, PathMove, Uid};
use std::collections::{BTreeMap, BTreeSet};

pub use crate::r4::settle::if6_rebind_rule;

/// I-F1: at most one live file node with status `present` or `planned` per (root, exact path), unless every node of
/// the key carries an unresolved `PathClaim`; the keys that break it.
// spec: [F18 §2.1] I-F1
pub fn if1_one_live_file_per_path(files: &[FileNode]) -> Vec<(String, String)> {
    let mut keys: BTreeMap<(&str, &str), Vec<&FileNode>> = BTreeMap::new();
    for f in files {
        if !f.tombstone && matches!(f.status, FileStatus::Present | FileStatus::Planned) {
            keys.entry((&f.root, &f.path)).or_default().push(f);
        }
    }
    keys.into_iter()
        .filter(|(_, v)| v.len() > 1 && !v.iter().all(|f| f.path_claim))
        .map(|((r, p), _)| (r.to_string(), p.to_string()))
        .collect()
}

/// A file node's identity inputs for I-F2.
#[derive(Clone, Debug)]
pub struct Identity {
    /// The uid.
    pub uid: Uid,
    /// `root`.
    pub root: String,
    /// `origin_path`.
    pub origin_path: String,
    /// `origin_pred`.
    pub origin_pred: Option<Uid>,
}

/// I-F2: every file uid equals `uid_file` over its stored inputs, every root node's uid equals `uid_root`, every anchor
/// uid equals `uid_anchor` over its source uid, `captured` and `pred`; the uids that do not (foreign nodes, which an
/// importer accepts and flags, [F18 §2.2]).
// spec: [F18 §2.2] I-F2
pub fn if2_uid_derivations(
    files: &[Identity],
    roots: &[(Uid, String)],
    anchors: &[(Uid, &Anchor)],
) -> Vec<Uid> {
    let mut bad = Vec::new();
    for f in files {
        if uid_file(&f.root, &f.origin_path, f.origin_pred) != f.uid {
            bad.push(f.uid);
        }
    }
    for (u, r) in roots {
        if uid_root(r) != *u {
            bad.push(*u);
        }
    }
    for (src, a) in anchors {
        if uid_anchor(*src, a.captured, a.pred) != a.uid {
            bad.push(a.uid);
        }
    }
    bad
}

/// I-F3: every `at` edge key carries an anchor whose uid is the key's discriminator, and anchor uids are unique per
/// (src, dst). `keys` are (src, dst, discriminator, anchor); the violations as (src, dst).
// spec: [F18 §2.3] I-F3
pub fn if3_at_edge_anchors(keys: &[(Uid, Uid, Option<Uid>, Option<&Anchor>)]) -> Vec<(Uid, Uid)> {
    let mut seen: BTreeSet<(Uid, Uid, Uid)> = BTreeSet::new();
    let mut bad = Vec::new();
    for (s, d, disc, a) in keys {
        let ok = match (disc, a) {
            (Some(u), Some(a)) => a.uid == *u && seen.insert((*s, *d, *u)),
            _ => false,
        };
        if !ok {
            bad.push((*s, *d));
        }
    }
    bad
}

/// I-F4 for what a settle versions: every field of a re-bind recomputes from the tree's content and the git history
/// alone — the path, `oid` over the bytes, `bytes`, `observed_git` = H, `observed_blob` = τ(H) at the path — and
/// `relink` is a value of the closed grammar, so no file id, volume key, time or runtime row reaches versioned data.
// spec: [F18 §2.4] I-F4
pub fn if4_no_machine_local_in_canonical(
    rb: &Rebind,
    fs: &Fs,
    git: &Git,
    tree: &str,
    algo: Algo,
) -> bool {
    let Some(t) = fs.trees.get(tree) else {
        return false;
    };
    let Ok(b) = t.read(&rb.to) else {
        return false;
    };
    let (head, blob) = match git.of_tree(tree) {
        Some((r, h)) => {
            let hc = r.head_commit(h).map(str::to_string);
            let bl = hc.as_deref().and_then(|c| r.tau(c).get(&rb.to).cloned());
            (hc, bl)
        }
        None => (None, None),
    };
    rb.oid.as_ref() == Some(&oid(algo, b))
        && rb.bytes == b.len() as u64
        && rb.observed_git == head
        && rb.observed_blob == blob
        && (rb.relink.is_empty() || relink_valid(&rb.relink))
}

/// The explicit doors that may write `removed` ([F18 §2.7]; [RULES/status-machines] TR-081 adds the settle's
/// `PathClaim` unification, which [F18 §2.7] does not list yet: open point 26 there).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Door {
    /// `file rm`.
    FileRm,
    /// `links fix --drop`.
    LinksFixDrop,
    /// `links fix --same-as`.
    LinksFixSameAs,
    /// `links fix --split`.
    LinksFixSplit,
    /// A settle of `main`'s writer tree under `files.deletion-inference = main-tree-commits`.
    MainTreeDeletion,
    /// A settle that unifies a `PathClaim` by an exact rename inside one commit (LV-005, TR-081): the claimant whose
    /// alias is the rename's source becomes `removed{reason: same-as}`, the automatic form of `links fix --same-as`.
    SettlePathClaim,
    /// Any other writer: any other settle, a merge, a sync, an import, a history verb.
    Other,
}

/// I-F7: `removed` is never inferred from absence: a status change to `removed` by anything but an explicit door (or a
/// history verb carrying such a change, which is not an origin) is a violation.
// spec: [F18 §2.7] I-F7
pub fn if7_removed_explicit(before: FileStatus, after: FileStatus, door: Door) -> bool {
    before == FileStatus::Removed || after != FileStatus::Removed || door != Door::Other
}

/// I-F8: every path value of a file node is a stored path of its root (a non-empty `rel-path`, or an `abs-path` for
/// root `abs`), and every `path_moves` entry's `from` and `to` are directory prefixes of the root node's root.
// spec: [F18 §2.8] I-F8
pub fn if8_path_rules(
    f: &FileNode,
    origin_path: &str,
    moves: &[PathMove],
    root_of_moves: &str,
) -> bool {
    let ok = |p: &str| {
        if f.root == "abs" {
            abspath(p).is_ok()
        } else {
            stored_rel_path(p).is_ok()
        }
    };
    ok(&f.path)
        && ok(origin_path)
        && f.aliases.iter().all(|a| ok(a))
        && moves.iter().all(|m| {
            m.from.root == root_of_moves
                && m.to.root == root_of_moves
                && root_of_moves != "abs"
                && dir_prefix(&m.from.text).is_ok()
                && dir_prefix(&m.to.text).is_ok()
        })
}

/// I-F9: a live span anchor never has a bare line number as its only selector: `quote`, `range`, `symbol` and `heading`
/// anchors carry a non-empty quote (or its digest when the text is unavailable), `lines` anchors a window with at least
/// one hash.
// spec: [F18 §2.9] I-F9
pub fn if9_no_bare_line_anchor(a: &Anchor) -> bool {
    match a.kind {
        Kind::File => true,
        Kind::Lines => crate::r4::text::parse_window(&a.window, Consts::DRAFT.win)
            .is_some_and(|(b, af)| !b.is_empty() || !af.is_empty()),
        _ => {
            if a.text_unavailable {
                a.stored_digests[0].is_some()
            } else {
                !a.quote.is_empty()
            }
        }
    }
}

/// I-F10: resolution is a pure function: the brute-force resolver gives the same result on the same inputs.
// spec: [F18 §2.10] I-F10
pub fn if10_resolve_pure(a: &Anchor, content: &[u8], algo: Algo, c: &Consts) -> bool {
    let r1: AResult = resolve(a, Content::Bytes(content), algo, c);
    let r2 = resolve(a, Content::Bytes(content), algo, c);
    r1 == r2
}

/// I-F12 over D: no two pairs share a branch, and no two share a tree.
// spec: [F18 §2.12] I-F12
pub fn if12_binding_unique(d: &[Designated]) -> bool {
    let branches: BTreeSet<&str> = d.iter().map(|p| p.branch.as_str()).collect();
    let trees: BTreeSet<&str> = d.iter().map(|p| p.tree.as_str()).collect();
    branches.len() == d.len() && trees.len() == d.len()
}

/// I-F13, the copy rule by definition over the simulated creation times ([F18 §2.13]; [F13 §3.9]): a `moved-auto`
/// result never rests on equal content alone. Its evidence is one of the corroborating exact tokens — captured intent
/// (`intent`, `intent-recovered`, a hook's `move`), identity (`file-id`, `dir-id`), a recorded entry (`prefix`), an
/// exact observation (`pending`), a git rename inside one commit (`r100`) or git's spelling (`case`) — or the copy
/// rule's creation-time line (`oid+ctime`), which holds only on a `TunneledNotCopied` volume, for a target without a
/// clone indicator whose simulated creation time is `teq` the recorded one of `FILEOBS` ([F20 §5.9] line 2). The
/// target's time comes from the tree `fs` holds under `tree`; `obs` is F's `FILEOBS` row there.
// spec: [F18 §2.13] I-F13; [F20 §5.9]
pub fn if13_copy_rule(r: &FileResult, fs: &Fs, tree: &str, obs: Option<&FileObs>) -> bool {
    if r.state != State::MovedAuto {
        return true;
    }
    match r.evidence {
        Some((1..=4 | 6..=10, _)) => true,
        Some((5, _)) => {
            let (Some(t), Some(q), Some(fc)) = (
                fs.trees.get(tree),
                r.at.as_deref(),
                obs.and_then(|o| o.creation_ns),
            ) else {
                return false;
            };
            let g = t.caps.granularity() as i64;
            t.caps.btime == Btime::TunneledNotCopied
                && t.files.get(q).is_some_and(|f| {
                    !f.attrs.contains("clone") && f.btime_ns.div_euclid(g) == fc.div_euclid(g)
                })
        }
        _ => false,
    }
}

/// P1 against ground truth ([40 §8.3.2]: "no wrong automatic re-bind"): a `moved-auto` result, and a guess applied under
/// `files.policy.auto = strong`, point at the path where the file really is (`truth`, `None` when it left the tree).
// spec: [40 §8.3.2] P1
pub fn p1_ground_truth(r: &FileResult, truth: Option<&str>) -> bool {
    let rebinds = r.state == State::MovedAuto || r.guess.is_some();
    !rebinds || r.at.as_deref() == truth
}

/// I-F14: a derived uid that is `removed` or engine-deleted on a view before an operation is not live (`present` or
/// `planned`, no tombstone) after it, unless an explicit door ran; the uids that came back.
// spec: [F18 §2.14] I-F14
pub fn if14_no_resurrection(
    before: &[FileNode],
    after: &[(Uid, FileNode)],
    uids_before: &[(Uid, u32)],
) -> Vec<Uid> {
    let dead: BTreeSet<u32> = before
        .iter()
        .filter(|f| f.tombstone || f.status == FileStatus::Removed)
        .map(|f| f.n)
        .collect();
    let dead_uids: BTreeSet<Uid> = uids_before
        .iter()
        .filter(|(_, n)| dead.contains(n))
        .map(|(u, _)| *u)
        .collect();
    after
        .iter()
        .filter(|(u, f)| {
            dead_uids.contains(u)
                && !f.tombstone
                && matches!(f.status, FileStatus::Present | FileStatus::Planned)
        })
        .map(|(u, _)| *u)
        .collect()
}

/// The capture digest an anchor's texts give, for I-F2's recomputation where the texts are held ([F18 §2.2]:
/// `captured` is trusted as stored otherwise).
pub fn captured_of(file: Uid, a: &Anchor) -> Option<[u8; 16]> {
    (!a.text_unavailable).then(|| captured(file, &a.selectors()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::r4::anchor::{Form, Mode, Watch, capture};
    use crate::value::{MoveClass, PathVal};

    fn node(n: u32, path: &str, status: FileStatus) -> FileNode {
        FileNode {
            n,
            root: "project".into(),
            path: path.into(),
            oid: None,
            bytes: None,
            observed_git: None,
            observed_blob: None,
            relink: None,
            aliases: Vec::new(),
            status,
            artifact_kind: None,
            tombstone: false,
            obs_hlc: 0,
            conflict: None,
            path_claim: false,
        }
    }

    #[test]
    fn the_invariants_find_their_violations() {
        let a = node(1, "a.md", FileStatus::Present);
        let b = node(2, "a.md", FileStatus::Planned);
        let c = node(3, "a.md", FileStatus::Removed);
        assert_eq!(if1_one_live_file_per_path(&[a.clone(), c.clone()]), vec![]);
        assert_eq!(if1_one_live_file_per_path(&[a.clone(), b.clone()]).len(), 1);
        let (mut a1, mut b1) = (a.clone(), b.clone());
        a1.path_claim = true;
        b1.path_claim = true;
        assert!(if1_one_live_file_per_path(&[a1, b1]).is_empty());
        let u = uid_file("project", "a.md", None);
        let id = Identity {
            uid: u,
            root: "project".into(),
            origin_path: "a.md".into(),
            origin_pred: None,
        };
        assert!(
            if2_uid_derivations(
                std::slice::from_ref(&id),
                &[(uid_root("project"), "project".into())],
                &[]
            )
            .is_empty()
        );
        let foreign = Identity {
            uid: Uid([9; 16]),
            ..id
        };
        assert_eq!(
            if2_uid_derivations(&[foreign], &[], &[]),
            vec![Uid([9; 16])]
        );
        assert!(if7_removed_explicit(
            FileStatus::Present,
            FileStatus::Removed,
            Door::FileRm
        ));
        assert!(if7_removed_explicit(
            FileStatus::Present,
            FileStatus::Removed,
            Door::SettlePathClaim
        ));
        assert!(!if7_removed_explicit(
            FileStatus::Present,
            FileStatus::Removed,
            Door::Other
        ));
        let mv = PathMove {
            hlc: 1,
            class: MoveClass::Explicit,
            from: PathVal {
                root: "project".into(),
                text: "a/".into(),
            },
            to: PathVal {
                root: "project".into(),
                text: "b/".into(),
            },
            git: None,
        };
        assert!(if8_path_rules(
            &a,
            "a.md",
            std::slice::from_ref(&mv),
            "project"
        ));
        let mut bad = a.clone();
        bad.path = "../a.md".into();
        assert!(!if8_path_rules(&bad, "a.md", &[], "project"));
        let (an, _) = capture(
            Uid([1; 16]),
            u,
            &Form::Lines(1, 1),
            Some(b"hello world\n"),
            Some(Watch::Span),
            None,
            Algo::Sha1,
            &[],
            &Consts::DRAFT,
        )
        .unwrap();
        assert!(if9_no_bare_line_anchor(&an));
        assert_eq!(an.mode, Mode::Live);
        assert!(if10_resolve_pure(
            &an,
            b"x\nhello world\n",
            Algo::Sha1,
            &Consts::DRAFT
        ));
        assert!(if2_uid_derivations(&[], &[], &[(Uid([1; 16]), &an)]).is_empty());
        assert_eq!(captured_of(u, &an), Some(an.captured));
        let d = vec![
            Designated {
                branch: "main".into(),
                tree: "T".into(),
                expected_ref: None,
                base: None,
            },
            Designated {
                branch: "lane/x".into(),
                tree: "T".into(),
                expected_ref: None,
                base: None,
            },
        ];
        assert!(!if12_binding_unique(&d));
        let mut dead = a.clone();
        dead.status = FileStatus::Removed;
        let back = if14_no_resurrection(&[dead], &[(Uid([5; 16]), a)], &[(Uid([5; 16]), 1)]);
        assert_eq!(back, vec![Uid([5; 16])]);
    }

    /// I-F13 by definition: every `moved-auto` result carries corroborating exact evidence, and the creation-time line
    /// holds only where the simulated volume and creation times allow it; P1 separately against ground truth.
    #[test]
    fn if13_needs_corroborating_evidence_and_p1_needs_the_true_path() {
        use crate::r4::cascade::{Detail, FileResult};
        use crate::r4::path::Os;
        use crate::r4::tree::{TreeOp, VolumeCaps};
        const R: &str = "C:/r";
        let mut fs = Fs::default();
        fs.ensure_tree(R, "C", VolumeCaps::NTFS, Os::Windows);
        fs.apply(
            R,
            &TreeOp::Write {
                path: "y.rs".into(),
                bytes: b"x\n".to_vec(),
                btime_ns: Some(7_000),
            },
            9_000,
        )
        .unwrap();
        let moved = |ev: Option<(u8, &'static str)>| {
            let mut r = FileResult::of(State::MovedAuto, vec![Detail::code(5)]);
            r.at = Some("y.rs".into());
            r.evidence = ev;
            r
        };
        let obs = |c: i64| FileObs {
            creation_ns: Some(c),
            ..FileObs::default()
        };
        for ev in [1, 2, 3, 4, 6, 7, 8, 9, 10] {
            assert!(
                if13_copy_rule(&moved(Some((ev, "lazy"))), &fs, R, None),
                "{ev}"
            );
        }
        assert!(!if13_copy_rule(&moved(None), &fs, R, None), "no evidence");
        assert!(
            !if13_copy_rule(&moved(Some((13, "policy"))), &fs, R, None),
            "a proposal class"
        );
        // The creation-time line: equal creation time on a TunneledNotCopied volume.
        let ctime = moved(Some((5, "lazy")));
        assert!(if13_copy_rule(&ctime, &fs, R, Some(&obs(7_000))));
        assert!(
            !if13_copy_rule(&ctime, &fs, R, Some(&obs(8_000))),
            "another creation time"
        );
        assert!(!if13_copy_rule(&ctime, &fs, R, None), "no FILEOBS row");
        fs.trees.get_mut(R).unwrap().caps.btime = Btime::CopiedByClones;
        assert!(
            !if13_copy_rule(&ctime, &fs, R, Some(&obs(7_000))),
            "clones copy creation times"
        );
        // Anything but a re-bind holds trivially.
        assert!(if13_copy_rule(
            &FileResult::of(State::Missing, Vec::new()),
            &fs,
            R,
            None
        ));
        // P1: a re-bind or a guess points at the true path.
        assert!(p1_ground_truth(&moved(Some((3, "lazy"))), Some("y.rs")));
        assert!(!p1_ground_truth(&moved(Some((3, "lazy"))), Some("z.rs")));
        let mut guess = FileResult::of(State::MovedNeedsConfirm, vec![Detail::code(11)]);
        guess.guess = Some((14, None));
        guess.at = Some("q.rs".into());
        assert!(!p1_ground_truth(&guess, None));
        guess.guess = None;
        assert!(p1_ground_truth(&guess, None), "a proposal is no re-bind");
    }
}
