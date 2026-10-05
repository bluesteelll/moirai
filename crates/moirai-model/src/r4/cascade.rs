//! Exact-evidence resolution of a file node in a tree, by definition ([40 §4.3], §4.4, §5.2; [F20 §3.5]–§5;
//! `docs/m0/PLAN.md` §6.2 R16): a pure function of the versioned link, the simulated tree with its volume, the abstract
//! git history and HEAD of the tree, the tree's runtime rows (`FILEOBS`, `PENDING`, `FSINTENT`, `TREES`, and the
//! fingerprints by `oid`), the resolver version and the parameters (I-F10). Every state of [40 §2.9] has an answer:
//! `ok` with its spelling details, `moved-auto` on exact evidence, proposals, `ambiguous`, `deleted`, `replaced`,
//! `missing` with its place, `absent-in-tree`, `pending`, `planned` and `unverified` with its reason.
//!
//! Every candidate list is enumerated whole and sorted in path order before any selection ([F20 §4.1]); every source
//! runs by definition over the whole tree where the design scopes it (E4's directories, E7's time predicate). The
//! similarity search of `--deep` ([F20 §5.14]) is not modelled: its quality is checked by the replay corpora and GT17
//! ([60 §4.2] row "File links"), and its outcomes are proposals, never re-binds.

use crate::r4::anchor::{Anchor, Consts};
use crate::r4::fold::{ceq, fold_v1};
use crate::r4::git::{Chain, Class, Git, Hist, Repo};
use crate::r4::ignore::Matcher;
use crate::r4::path::{Os, glob_match, representable};
use crate::r4::strings::State;
use crate::r4::text::{Fingerprint, Ratio, estimates, fingerprint, is_text, oid, oid_in};
use crate::r4::tree::{Btime, FileId, Fs, Location, Stat, StatOut, Tree, basename, dirname};
use crate::r4::uid::FileStatus;
use crate::value::{Algo, MoveClass, Oid, PathMove, Uid};
use std::cell::{OnceCell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

/// A file node of the reading view, as the resolver reads it ([40 §2.2]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileNode {
    /// `#N`.
    pub n: u32,
    /// `root`.
    pub root: String,
    /// `path`.
    pub path: String,
    /// `oid`.
    pub oid: Option<Oid>,
    /// `bytes`.
    pub bytes: Option<u64>,
    /// `observed_git`: a git commit id (hex).
    pub observed_git: Option<String>,
    /// `observed_blob`: a git blob id (hex).
    pub observed_blob: Option<String>,
    /// `relink`.
    pub relink: Option<String>,
    /// `aliases`.
    pub aliases: Vec<String>,
    /// `status`.
    pub status: FileStatus,
    /// `artifact_kind`.
    pub artifact_kind: Option<String>,
    /// The node is engine-deleted (a tombstone).
    pub tombstone: bool,
    /// The `hlc` of the commit that last set the observation composite (G4's time window, [F20 §5.11.2]).
    pub obs_hlc: u64,
    /// A `FieldEdit` conflict value on the composite: the ours and theirs sides ([40 §5.3]).
    pub conflict: Option<Box<[Side; 2]>>,
    /// A `PathClaim` conflict value on the composite.
    pub path_claim: bool,
}

/// One side of a composite conflict value: the observation fields the settle needs ([40 §5.3]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Side {
    /// `path`.
    pub path: String,
    /// `oid`.
    pub oid: Option<Oid>,
    /// `observed_git`.
    pub observed_git: Option<String>,
    /// `observed_blob`.
    pub observed_blob: Option<String>,
}

/// The reading view around a node: its live file nodes, each root node's `path_moves`, and the anchors whose selectors
/// hold a `FieldEdit` conflict value after a merge.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct View {
    /// The live file nodes of the view (the node itself included).
    pub files: Vec<FileNode>,
    /// Root name → the root node's `path_moves` entries.
    pub moves: BTreeMap<String, Vec<PathMove>>,
    /// Anchor selector conflicts ([RULES/link-merge-rules] LM-021): both sides repinned differently.
    pub anchor_conflicts: Vec<AnchorConflict>,
}

/// An anchor whose selectors hold a `FieldEdit` conflict value ([40 §5.5] "both repinned differently").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnchorConflict {
    /// The file node the `at` edge points at (`#F`).
    pub file: u32,
    /// The anchor uid.
    pub anchor: Uid,
    /// The two sides' anchors: ours, then theirs.
    pub sides: Box<[Anchor; 2]>,
}

/// A `FILEOBS` row of a (node, tree) ([F11 §12.5]).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct FileObs {
    /// `file_id`, with the parent directory's id beside it.
    pub file_id: Option<FileId>,
    /// `parent_dir_id`.
    pub parent_dir: Option<u64>,
    /// `size`.
    pub size: u64,
    /// `mtime`, ns.
    pub mtime_ns: i64,
    /// `creation`, ns; `None` when absent.
    pub creation_ns: Option<i64>,
    /// `last_oid`.
    pub last_oid: Option<Oid>,
    /// `verified_at` (an hlc).
    pub verified_at: u64,
    /// `missing_since` (an hlc); 0 = not missing.
    pub missing_since: u64,
    /// The recorded state and its detail parts, shown while the stat tuple equals the row.
    pub recorded: Option<(State, Vec<Detail>)>,
    /// `path_seen`.
    pub path_seen: Option<String>,
}

/// A settle epoch of `TREES` ([F11 §12.4]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EpochKind {
    /// `full-tree`.
    Full,
    /// `lane-owned`: the scope digest and the globs the settle used.
    LaneOwned {
        /// The scope digest.
        digest: [u8; 16],
        /// The globs.
        globs: Vec<String>,
    },
    /// `partial`.
    Partial,
}

/// A `TREES` row's resolver inputs ([F11 §12.4]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TreeRow {
    /// `first_settle_done`.
    pub first_settle_done: bool,
    /// `last_settle_hlc`.
    pub last_settle_hlc: u64,
    /// The newest epoch per scope: (kind, hlc).
    pub epochs: Vec<(EpochKind, u64)>,
}

/// The source of a `PENDING` row ([F11 §12.6] `source`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PendingSource {
    /// 1 `evidence-hook`: a hook's intent, file-id chain, `*** Move to:` line or argument parse.
    Hook,
    /// 2 `reader-settle`: a reader tree's observation.
    ReaderSettle,
}

/// A `PENDING` row ([F11 §12.6]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingRow {
    /// `#F`.
    pub n: u32,
    /// The observing tree's root.
    pub tree: String,
    /// `class`: the observation's evidence class, numbered as a proposal's ([F18 §4.10]).
    pub class: PClass,
    /// `source`.
    pub source: PendingSource,
    /// `evidence`: the token code of [F18 §5.2] the observation carries (an exact token 1–10 for class `exact`, else
    /// a proposal class 13–26).
    pub evidence: u8,
    /// The captured `oid`.
    pub oid: Option<Oid>,
    /// The path observed gone.
    pub from: String,
    /// The path observed.
    pub to: String,
    /// When observed.
    pub hlc: u64,
}

/// An `FSINTENT` row's items as E1 reads them ([F11 §12.7]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Intent {
    /// The writer tree's root.
    pub tree: String,
    /// The items: (source, destination) paths.
    pub items: Vec<(String, Option<String>)>,
    /// Still open.
    pub open: bool,
    /// Closed by intent recovery.
    pub recovered: bool,
}

/// The key of a `PREFIXEV` row: (tree root, root name, from/, to/) ([F11 §12.11]).
pub type PrefixKey = (String, String, String, String);

/// The runtime rows of the store ([40 §2.6]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Runtime {
    /// `FILEOBS` by (`#F`, tree root).
    pub fileobs: BTreeMap<(u32, String), FileObs>,
    /// `PENDING`.
    pub pending: Vec<PendingRow>,
    /// `FSINTENT`.
    pub intents: Vec<Intent>,
    /// `TREES` by tree root.
    pub trees: BTreeMap<String, TreeRow>,
    /// Fingerprints by `oid` (`FPRINT`).
    pub fprint: BTreeMap<Oid, Fingerprint>,
    /// `PREFIXEV`: per (tree, root, from/, to/), the linked nodes re-bound exactly from under `from/` to `to/` so far,
    /// across settles ([F11 §12.11]; [F20 §5.16]).
    pub prefixev: BTreeMap<PrefixKey, BTreeSet<u32>>,
    /// `DIRMAP`: (tree root, directory id) → (root-relative path, mtime ns) of each directory a settle enumerated, the
    /// frontier's input ([F11 §12.10]; [F20 §5.12.1]).
    pub dirmap: BTreeMap<(String, u64), (String, i64)>,
}

/// The parameters of one resolution.
#[derive(Clone, Debug)]
pub struct Params {
    /// A settle point (else a read).
    pub settle: bool,
    /// The tree is eligible ([40 §5.1]).
    pub eligible: bool,
    /// The tree is the writer tree of the reading branch ([F18 §3.6]).
    pub writer: bool,
    /// The reading branch is `main` (the committed-only rule).
    pub main: bool,
    /// `files.policy.auto = strong`.
    pub policy_strong: bool,
    /// `SKEW` in ns (HOLE F20-clock-skew; a parameter of the model at M0).
    pub skew_ns: i128,
    /// A(R): the `oid` algorithm of the node's root.
    pub algo: Algo,
    /// `files.ignore`.
    pub ignore_defaults: Vec<String>,
    /// The lane scope of the reading branch for `lane-owned` epoch coverage: its digest and globs.
    pub lane_scope: Option<([u8; 16], Vec<String>)>,
    /// The tree of a named root, by root name (`roots.<name>`); `project` is the tree itself.
    pub named_roots: BTreeMap<String, String>,
    /// The directory moves the settle's first pass inferred from sibling nodes (X/, Y/) ([F20 §5.10]).
    pub siblings: Vec<(String, String)>,
    /// `PENDING` rows take no part (a promotion's own-evidence test, [40 §5.3]).
    pub no_pending: bool,
    /// The settle's racy threshold T0 ([F20 §5.12.1]): the mtime of `<store>/tmp/settle.stamp` read at the settle's
    /// start; `None` when the tree lies on another volume than the store, where T0 is the largest `DIRMAP` mtime of the
    /// tree (with no rows, every directory counts as changed).
    pub stamp_ns: Option<i64>,
    /// The anchor constants a settle's anchor steps evaluate ([F20 §7]).
    pub anchor: Consts,
    /// `files.max-read-bytes` ([CFG]): a project file larger than this has no content available, `Unavailable(size)`
    /// ([F20 §2.4] item 1); `None` for no limit.
    pub max_read_bytes: Option<u64>,
}

/// The default of `files.max-read-bytes` ([CFG]: 16 MiB).
pub const MAX_READ_BYTES: u64 = 16 << 20;

impl Default for Params {
    fn default() -> Params {
        Params {
            settle: false,
            eligible: true,
            writer: false,
            main: false,
            policy_strong: false,
            skew_ns: 0,
            algo: Algo::Sha1,
            ignore_defaults: vec!["target/".into(), "node_modules/".into(), "build/".into()],
            lane_scope: None,
            named_roots: BTreeMap::new(),
            siblings: Vec::new(),
            no_pending: false,
            stamp_ns: None,
            anchor: Consts::DRAFT,
            max_read_bytes: Some(MAX_READ_BYTES),
        }
    }
}

/// The content of a project file at its on-disk path as a command may read it ([F20 §2.4] "Availability"): a denied
/// entry is `unreadable` (59); a file larger than `files.max-read-bytes` is `size` (58), its content never examined; a
/// cloud-only entry is `cloud-only` (54).
// spec: [F20 §2.4] items 1–3; [F20 §1.5]
pub fn read_content<'t>(tree: &'t Tree, disk: &str, max: Option<u64>) -> Result<&'t [u8], u8> {
    match tree.files.get(disk) {
        Some(f) if !f.denied && max.is_some_and(|m| f.bytes.len() as u64 > m) => Err(58),
        _ => tree.read(disk),
    }
}

/// A detail part as the cascade decides it ([F18 §4.6]; [F11 §12.5] `Detail`): its code and slot values.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Detail {
    /// The detail code.
    pub code: u8,
    /// `<path>` slots.
    pub paths: Vec<String>,
    /// `<score>` slots.
    pub scores: Vec<Ratio>,
    /// `<g7>` slots (git ids).
    pub git: Vec<String>,
    /// `<n>`.
    pub n: Option<u32>,
    /// `<quote>` (detail 66): the captured quote's bytes, unescaped and uncut ([F18 §4.7] rule 5 is the renderer's).
    pub quote: Vec<u8>,
}

impl Detail {
    /// A detail without slots.
    pub fn code(code: u8) -> Detail {
        Detail {
            code,
            paths: Vec::new(),
            scores: Vec::new(),
            git: Vec::new(),
            n: None,
            quote: Vec::new(),
        }
    }

    fn path(code: u8, p: &str) -> Detail {
        Detail {
            paths: vec![p.to_string()],
            ..Detail::code(code)
        }
    }

    /// A detail with one `<score>` slot.
    pub fn score(code: u8, s: Ratio) -> Detail {
        Detail {
            scores: vec![s.reduced()],
            ..Detail::code(code)
        }
    }

    fn git(code: u8, g: &str) -> Detail {
        Detail {
            git: vec![g.to_string()],
            ..Detail::code(code)
        }
    }
}

/// The evidence class of a proposal ([F20 §1.5]; [F18 §4.10]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PClass {
    /// 1 `exact`.
    Exact,
    /// 2 `strong`.
    Strong,
    /// 3 `copy`.
    Copy,
    /// 4 `weak`.
    Weak,
}

/// The principal details of the proposals `files.policy.auto = strong` may apply as a marked guess: exactly the classes
/// [F18 §5.4] lists for `policy/<class>` — 11 `file-id-edited`, 12 `prefix-strong` ("inferred directory move"), 13
/// `git-pair`, 14 `edited+moved`, 15 `similarity`, 19 `argv`. The proposals rendered 10 `identical copy`, 16 `weak`,
/// 17 `split`, 18 `merged`, 20 `moved differently on this line` and 21 `directory moved, file replaced` are never
/// applied automatically, under either policy ([F18 §5.4]; [F20 §5.7] step 4; [40 §4.4] "Auto-applied: no").
pub const POLICY_DETAILS: [u8; 6] = [11, 12, 13, 14, 15, 19];

/// A proposal ([F18 §4.10]): its class and token, the principal detail it renders as `moved-needs-confirm`, the target
/// (for a split, the first piece in path order, with every piece listed), the score, and whether policy B may apply it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Proposal {
    /// The class.
    pub class: PClass,
    /// The proposal-class token code (13–26).
    pub evidence: u8,
    /// The principal detail code (10–21) of `moved-needs-confirm` ([F18 §4.6]).
    pub detail: u8,
    /// The candidate path.
    pub path: String,
    /// The score, when the token is scored.
    pub score: Option<Ratio>,
    /// A split's pieces in path order ([F20 §5.11.4] row 2); empty otherwise.
    pub pieces: Vec<String>,
    /// `files.policy.auto = strong` may apply it as a marked guess: a `strong` proposal whose detail is one of
    /// [`POLICY_DETAILS`], resting on this tree's own evidence.
    pub auto: bool,
}

impl Proposal {
    /// A proposal; `auto` holds for a `strong` class with a detail of [`POLICY_DETAILS`].
    // spec: [F18 §5.4] policy classes; [F20 §5.5] step 3
    pub fn new(
        class: PClass,
        evidence: u8,
        detail: u8,
        path: String,
        score: Option<Ratio>,
    ) -> Proposal {
        Proposal {
            class,
            evidence,
            detail,
            path,
            score,
            pieces: Vec::new(),
            auto: class == PClass::Strong && POLICY_DETAILS.contains(&detail),
        }
    }

    /// The same proposal, never applied automatically.
    pub fn manual(self) -> Proposal {
        Proposal {
            auto: false,
            ..self
        }
    }
}

/// The principal detail a proposal-class token renders when nothing else qualifies it ([F18 §4.6]; §5.2): 13 → 10,
/// 14 → 11, 15 → 12, 16 → 13, 17 → 14, 18 → 15, 19 → 16, 20 → 17, 21 → 18, 22 → 19; the tokens 23–26 name answers to
/// `ambiguous` states and render 22's `candidates` there.
pub fn token_detail(token: u8) -> u8 {
    match token {
        13..=22 => token - 3,
        _ => 22,
    }
}

/// The outcome of resolving one file node in one tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileResult {
    /// The file state.
    pub state: State,
    /// The detail parts the cascade decided, in [F18 §4.7] rule-1 order.
    pub details: Vec<Detail>,
    /// The path the file is at: `ok`'s path (the on-disk spelling, or the normalization rule's entry), `moved-auto`'s
    /// target; `None` otherwise.
    pub at: Option<String>,
    /// `moved-auto`: the exact evidence's token code (1–12) and its `how` (`lazy`, `git`, `hook`, `explicit`).
    pub evidence: Option<(u8, &'static str)>,
    /// The proposals kept (at most 3, [F20] `LIST_MAX`).
    pub proposals: Vec<Proposal>,
    /// The candidates an `ambiguous` state lists, or the pieces a split lists (at most 3).
    pub candidates: Vec<String>,
    /// The tree is fresh for the node ([40 §5.3]).
    pub fresh: bool,
    /// The candidate path is committed in τ(H) (`observed_blob` would be set).
    pub committed: bool,
    /// The `committed` directory moves the window shows (from/, to/, git commit), for a writer-tree settle
    /// ([F20 §5.16]).
    pub committed_moves: Vec<(String, String, String)>,
    /// A strong re-bind under `files.policy.auto = strong`: the class token and score of the guess.
    pub guess: Option<(u8, Option<Ratio>)>,
    /// The exact candidates E1, E3d, E3 and E6 yielded, whatever decided the state: sibling inference's input
    /// ([F20 §5.10]).
    pub sibling_exact: Vec<String>,
}

impl FileResult {
    /// A result with a state and its details and nothing else set.
    pub fn of(state: State, details: Vec<Detail>) -> FileResult {
        FileResult {
            state,
            details,
            at: None,
            evidence: None,
            proposals: Vec::new(),
            candidates: Vec::new(),
            fresh: false,
            committed: false,
            committed_moves: Vec::new(),
            guess: None,
            sibling_exact: Vec::new(),
        }
    }
}

/// What one evidence source yields ([F20 §5.5]).
#[derive(Clone, Debug, Default)]
struct Yield {
    /// Exact candidates with their evidence (token code, `how`).
    exact: Vec<(String, u8, &'static str)>,
    /// A tie that is `ambiguous` whatever its size (an identical-blob group of E6).
    tie: Option<Vec<String>>,
    /// Strong, copy and weak proposals.
    proposals: Vec<Proposal>,
    /// A tiny F's equal-`oid` candidates of E4, E5 and E7, which make F `ambiguous` ([F20 §5.5] "Tiny files").
    tiny_eq: Vec<String>,
    /// The source was unavailable, with the reason's detail code.
    unavailable: Option<u8>,
    /// A place where the file was located that makes it `missing` (a detail code 38–42).
    place: Option<u8>,
    /// E6 saw the path deleted in this commit.
    deleted_in: Option<String>,
    /// E3d's replaced verdict at the located path.
    replaced: Option<(Ratio, Ratio)>,
}

/// The never-candidate patterns of resolver version 1 ([F20 §4.7.2]).
// spec: [F20 §4.7.2]
pub const NEVER: [&str; 21] = [
    "*.tmp",
    "*.tmp.*",
    "*___jb_tmp___",
    "*___jb_old___",
    "*~",
    "*.bak",
    "*.orig",
    "*.old",
    "*.rej",
    "*.swp",
    "*.swo",
    "4913",
    ".#*",
    "~$*",
    "sed??????",
    "._*",
    ".DS_Store",
    ".fuse_hidden*",
    ".nfs*",
    ".goutputstream-*",
    ".~lock.*#",
];

/// A never-candidate pattern against a basename: whole-name, `*` any bytes, `?` one byte, every other byte `eqi`
/// ([F20 §4.7.1]).
// spec: [F20 §4.7.1]
pub fn never_match(pat: &[u8], name: &[u8]) -> bool {
    match pat.first() {
        None => name.is_empty(),
        Some(b'*') => (0..=name.len()).any(|i| never_match(&pat[1..], &name[i..])),
        Some(b'?') => !name.is_empty() && never_match(&pat[1..], &name[1..]),
        Some(c) => {
            !name.is_empty()
                && c.eq_ignore_ascii_case(&name[0])
                && never_match(&pat[1..], &name[1..])
        }
    }
}

/// The bytes of a hexadecimal git id.
fn unhex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

/// `teq(a, b)`: equal at the granularity G ([F20 §5.1]).
fn teq(a: i64, b: i64, g: u64) -> bool {
    let g = g as i64;
    a.div_euclid(g) == b.div_euclid(g)
}

/// `hlc_ns(h)` ([F20 §1.2]).
fn hlc_ns(h: u64) -> i128 {
    i128::from(h >> 16) * 1_000_000
}

/// What every resolution of one root's nodes in one tree shares within one command, computed once: the ignore matcher
/// ([F20 §4.4]), the spellings of the root by `fold_v1` key ([F20 §3.5]) and τ(H)'s paths by key ([F20 §3.6]
/// `git/case`), the paths and file ids live `present` or `planned` nodes of the root hold ([F20 §4.2]), the
/// changed-directory frontier ([F20 §5.12.1]), the `oid`s of the contents read, and the `committed` directory moves of
/// each window ([F20 §5.16]). A tree, a view and the runtime rows are constant within a command, so every prepared
/// value equals the definition's.
struct RootPrep<'a> {
    /// The root's tree.
    tree: &'a Tree,
    /// The root is `project` of a git worktree.
    git: bool,
    /// τ(H) of a `project` root of a git worktree whose HEAD can be read.
    tau: Option<&'a BTreeMap<String, String>>,
    /// The ignore matcher.
    matcher: Matcher,
    /// The spellings and τ(H)'s paths by `fold_v1` key, once the twin rule or `git/case` needs them.
    folds: OnceCell<Folds>,
    /// Path → the root's live `present` or `planned` nodes that hold it.
    holders: BTreeMap<String, Vec<u32>>,
    /// File id → the root's live `present` or `planned` nodes whose path stats to it.
    id_holders: BTreeMap<FileId, Vec<u32>>,
    /// The changed-directory frontier, once a Linux or macOS E7 needs it.
    frontier: OnceCell<BTreeSet<String>>,
    /// (algorithm, read limit, on-disk path) → the content's `oid`, or why the content is unavailable.
    oids: RefCell<BTreeMap<OidKey, Result<Oid, u8>>>,
    /// Window → its `committed` directory moves.
    moves: RefCell<BTreeMap<Vec<String>, Rc<Vec<DirMove>>>>,
}

/// A `committed` directory move: (from/, to/, git commit) ([F20 §5.16]).
type DirMove = (String, String, String);

/// The key of a memoised `oid`: (A(R), `files.max-read-bytes`, on-disk path).
type OidKey = (Algo, Option<u64>, String);

/// The spellings of a root and τ(H)'s paths by `fold_v1` key ([F20 §3.5], §3.6).
struct Folds {
    /// Key → the spellings with that key: the paths of the root's live `present` or `planned` nodes and, with git,
    /// τ(H)'s paths.
    spellings: BTreeMap<String, BTreeSet<String>>,
    /// Key → τ(H)'s paths with that key (empty without git).
    tau: BTreeMap<String, Vec<String>>,
}

impl RootPrep<'_> {
    /// The spellings and τ(H)'s paths by `fold_v1` key, computed once.
    fn folds(&self) -> &Folds {
        self.folds.get_or_init(|| {
            let mut spellings: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
            for q in self.holders.keys() {
                spellings.entry(fold_v1(q)).or_default().insert(q.clone());
            }
            let mut tau: BTreeMap<String, Vec<String>> = BTreeMap::new();
            for q in self.tau.into_iter().flat_map(|t| t.keys()) {
                let k = fold_v1(q);
                spellings.entry(k.clone()).or_default().insert(q.clone());
                tau.entry(k).or_default().push(q.clone());
            }
            Folds { spellings, tau }
        })
    }
}

/// The shared inputs of the resolutions of one command in one tree ([`RootPrep`] per root): the view, the trees, the git
/// history with its memo ([`Hist`]) and the tree's HEAD, and the runtime rows.
pub struct Prep<'a> {
    view: &'a View,
    fs: &'a Fs,
    rt: &'a Runtime,
    tree_root: &'a str,
    hist: Option<Hist<'a>>,
    head: Option<&'a str>,
    ignore_defaults: &'a [String],
    stamp_ns: Option<i64>,
    roots: RefCell<BTreeMap<(String, String), Rc<RootPrep<'a>>>>,
}

impl<'a> Prep<'a> {
    /// The shared inputs of a command's resolutions in tree `tree_root` under the parameters `p` (whose `siblings` and
    /// `no_pending` may differ between the resolutions; nothing prepared depends on them).
    pub fn new(
        view: &'a View,
        fs: &'a Fs,
        git: &'a Git,
        rt: &'a Runtime,
        tree_root: &'a str,
        p: &'a Params,
    ) -> Prep<'a> {
        let (hist, head) = match git.of_tree(tree_root) {
            Some((r, h)) => (Some(Hist::new(r)), r.head_commit(h)),
            None => (None, None),
        };
        Prep {
            view,
            fs,
            rt,
            tree_root,
            hist,
            head,
            ignore_defaults: &p.ignore_defaults,
            stamp_ns: p.stamp_ns,
            roots: RefCell::new(BTreeMap::new()),
        }
    }

    /// The git history of the tree, memoised, and its HEAD commit.
    pub fn git(&self) -> (Option<&Hist<'a>>, Option<&'a str>) {
        (self.hist.as_ref(), self.head)
    }

    /// The prepared root `name` whose tree is `dir`; `None` when the tree does not exist.
    fn root(&self, name: &str, dir: &str) -> Option<Rc<RootPrep<'a>>> {
        let key = (name.to_string(), dir.to_string());
        if let Some(r) = self.roots.borrow().get(&key) {
            return Some(Rc::clone(r));
        }
        let tree = self.fs.trees.get(dir)?;
        let git = name == "project" && self.hist.is_some();
        let tau = match (git, &self.hist, self.head) {
            (true, Some(h), Some(head)) => Some(h.repo.tau(head)),
            _ => None,
        };
        let ignore: BTreeMap<String, Vec<u8>> = tree
            .files
            .iter()
            .filter(|(k, _)| k.as_str() == ".gitignore" || k.ends_with("/.gitignore"))
            .map(|(k, v)| (k.clone(), v.bytes.clone()))
            .collect();
        let mut holders: BTreeMap<String, Vec<u32>> = BTreeMap::new();
        let mut id_holders: BTreeMap<FileId, Vec<u32>> = BTreeMap::new();
        for g in self.view.files.iter().filter(|g| {
            g.root == name
                && !g.tombstone
                && matches!(g.status, FileStatus::Present | FileStatus::Planned)
        }) {
            holders.entry(g.path.clone()).or_default().push(g.n);
            if let StatOut::Present(s) = tree.stat(&g.path)
                && s.id.kind != 0
            {
                id_holders.entry(s.id.clone()).or_default().push(g.n);
            }
        }
        let r = Rc::new(RootPrep {
            tree,
            git,
            tau,
            matcher: Matcher::of(&ignore, git, self.ignore_defaults),
            folds: OnceCell::new(),
            holders,
            id_holders,
            frontier: OnceCell::new(),
            oids: RefCell::new(BTreeMap::new()),
            moves: RefCell::new(BTreeMap::new()),
        });
        self.roots.borrow_mut().insert(key, Rc::clone(&r));
        Some(r)
    }

    /// Resolves one file node ([`resolve_file`]) with the prepared inputs.
    // spec: [40 §4.3]; [40 §4.4]; [40 §5.2]; [F20 §5]; [F18 §2.10] I-F10
    pub fn resolve(&self, f: &FileNode, p: &Params) -> FileResult {
        // Step 0: eligibility and status.
        if f.tombstone {
            return FileResult::of(State::Deleted, vec![Detail::code(31)]);
        }
        if f.status == FileStatus::Removed {
            return FileResult::of(State::Deleted, vec![Detail::code(30)]);
        }
        if !p.eligible {
            return FileResult::of(State::Unverified, vec![Detail::code(56)]);
        }
        let root = match f.root.as_str() {
            "project" => self.tree_root.to_string(),
            "abs" => return resolve_abs(f, self.fs, p),
            name => match p.named_roots.get(name) {
                Some(r) => r.clone(),
                None => return FileResult::of(State::Unverified, vec![Detail::code(60)]),
            },
        };
        let Some(pre) = self.root(&f.root, &root) else {
            return FileResult::of(State::Unverified, vec![Detail::code(56)]);
        };
        let cx = Cx {
            f,
            view: self.view,
            fs: self.fs,
            tree: pre.tree,
            hist: if pre.git { self.hist.as_ref() } else { None },
            head: if pre.git { self.head } else { None },
            rt: self.rt,
            obs: self.rt.fileobs.get(&(f.n, root.clone())),
            p,
            pre: &pre,
            stamp_ns: self.stamp_ns,
            window: OnceCell::new(),
            chain_p: OnceCell::new(),
        };
        cx.resolve()
    }
}

/// The inputs of one resolution, borrowed.
struct Cx<'a> {
    f: &'a FileNode,
    view: &'a View,
    fs: &'a Fs,
    tree: &'a Tree,
    /// The tree's git history, memoised, for root `project` of a git worktree.
    hist: Option<&'a Hist<'a>>,
    /// The tree's HEAD commit, when it can be read.
    head: Option<&'a str>,
    rt: &'a Runtime,
    obs: Option<&'a FileObs>,
    p: &'a Params,
    pre: &'a RootPrep<'a>,
    /// The settle's racy threshold T0, as prepared ([`Params::stamp_ns`]).
    stamp_ns: Option<i64>,
    /// F's integration window W ([F20 §5.11.2]), once needed: g..H when F's `observed_git` g is in the local object
    /// store (G2's window and G4's alike), else the time-bounded window; empty without git or without g.
    window: OnceCell<Vec<String>>,
    /// E6's chain from p over W, once needed ([F20 §5.11.3]).
    chain_p: OnceCell<Chain>,
}

impl Cx<'_> {
    fn g(&self) -> u64 {
        self.tree.caps.granularity()
    }

    fn repo(&self) -> Option<&Repo> {
        self.hist.map(|h| h.repo)
    }

    fn tau_h(&self) -> Option<&BTreeMap<String, String>> {
        Some(self.repo()?.tau(self.head?))
    }

    fn in_tau(&self, q: &str) -> bool {
        self.tau_h().is_some_and(|t| t.contains_key(q))
    }

    /// F's `observed_git`, `None` when empty.
    fn g_obs(&self) -> Option<&str> {
        self.f.observed_git.as_deref().filter(|g| !g.is_empty())
    }

    /// The integration window W ([F20 §5.11.2]; [40 §4.3] G2, G4), computed once per resolution.
    fn window(&self) -> &[String] {
        self.window
            .get_or_init(|| match (self.hist, self.head, self.g_obs()) {
                (Some(h), Some(head), Some(g)) if h.repo.has(g) => h.window_since(g, head),
                (Some(h), Some(head), Some(_)) => {
                    h.repo.window_timed(head, self.f.obs_hlc, self.p.skew_ns)
                }
                _ => Vec::new(),
            })
    }

    /// E6's chain from p over W ([F20 §5.11.3]), computed once per resolution.
    fn chain_p(&self) -> &Chain {
        self.chain_p.get_or_init(|| match self.hist {
            Some(h) => h.chain(self.window(), &self.f.path),
            None => Chain::NoStep,
        })
    }

    /// F's recorded contents {o, last_oid}.
    fn recorded(&self) -> Vec<&Oid> {
        let mut v: Vec<&Oid> = Vec::new();
        if let Some(o) = &self.f.oid {
            v.push(o);
        }
        if let Some(o) = self.obs.and_then(|o| o.last_oid.as_ref()) {
            v.push(o);
        }
        v
    }

    /// The content of a present path (its on-disk spelling), or the unavailable reason ([F20 §2.4]: `size` beyond
    /// `files.max-read-bytes`, `cloud-only`, `unreadable`).
    fn content(&self, disk: &str) -> Result<&[u8], u8> {
        read_content(self.tree, disk, self.p.max_read_bytes)
    }

    /// The `oid` under A(R) of the content of a present path ([F20 §2.3]), or the unavailable reason; each content is
    /// read and hashed once per command.
    fn oid_at(&self, disk: &str) -> Result<Oid, u8> {
        let key = (self.p.algo, self.p.max_read_bytes, disk.to_string());
        if let Some(r) = self.pre.oids.borrow().get(&key) {
            return r.clone();
        }
        let r = self.content(disk).map(|b| oid(self.p.algo, b));
        self.pre.oids.borrow_mut().insert(key, r.clone());
        r
    }

    /// "`oid(q) ∈ {o, last_oid}`", three-valued ([F20 §2.3]); an unreadable content is unknown.
    fn oid_matches(&self, disk: &str) -> Option<bool> {
        let rec = self.recorded();
        if rec.is_empty() {
            return None;
        }
        oid_in(&self.oid_at(disk).ok()?, &rec)
    }

    /// The effective `verified_at` V ([F20 §5.9]).
    fn verified_at(&self) -> u64 {
        let own = self.obs.map_or(0, |o| o.verified_at);
        let epoch = self
            .rt
            .trees
            .get(&self.tree.root)
            .map(|t| {
                t.epochs
                    .iter()
                    .filter(|(k, _)| match k {
                        EpochKind::Full => true,
                        EpochKind::LaneOwned { digest, globs } => {
                            self.p.lane_scope.as_ref().is_some_and(|(d, _)| d == digest)
                                && globs.iter().any(|g| glob_match(g, &self.f.path))
                        }
                        EpochKind::Partial => false,
                    })
                    .map(|(_, h)| *h)
                    .max()
                    .unwrap_or(0)
            })
            .unwrap_or(0);
        own.max(epoch)
    }

    /// Whether a path is bound: another live present or planned node holds it bytewise, or both stat to one file
    /// ([F20 §4.2]).
    // spec: [F20 §4.2]
    fn bound(&self, q: &str) -> bool {
        let other = |ns: &Vec<u32>| ns.iter().any(|n| *n != self.f.n);
        self.pre.holders.get(q).is_some_and(other)
            || match self.tree.stat(q) {
                StatOut::Present(s) => self.pre.id_holders.get(&s.id).is_some_and(other),
                _ => false,
            }
    }

    /// Whether a basename is a never-candidate for F: a listed pattern, F's basename plus a suffix, or a cloud conflict
    /// copy beside a live file node ([F20 §4.7]). A cloud conflict copy is `stem ‖ "-" ‖ X ‖ ext` for a live node's
    /// basename `stem ‖ ext` in q's directory, where ext is empty or `.` and the bytes after the basename's last `.`:
    /// both readings are tried.
    // spec: [F20 §4.7]
    fn never(&self, q: &str) -> bool {
        let name = basename(q).as_bytes();
        if NEVER.iter().any(|p| never_match(p.as_bytes(), name)) {
            return true;
        }
        let pb = basename(&self.f.path).as_bytes();
        if name.len() > pb.len() && name[..pb.len()].eq_ignore_ascii_case(pb) {
            return true;
        }
        if self.tree.cloud_root {
            let dir = dirname(q);
            let n = basename(q);
            let conflict_copy = |stem: &str, ext: &str| {
                n.strip_prefix(stem)
                    .and_then(|r| r.strip_prefix('-'))
                    .and_then(|r| r.strip_suffix(ext))
                    .is_some_and(|mid| !mid.is_empty() && !mid.contains('.') && !mid.contains('/'))
            };
            for g in &self.view.files {
                if g.root != self.f.root || g.tombstone || dirname(&g.path) != dir {
                    continue;
                }
                let gb = basename(&g.path);
                let dotted = gb.rfind('.').map(|i| (&gb[..i], &gb[i..]));
                if conflict_copy(gb, "") || dotted.is_some_and(|(s, e)| conflict_copy(s, e)) {
                    return true;
                }
            }
        }
        false
    }

    fn ignored(&self, q: &str) -> bool {
        !self.in_tau(q) && self.pre.matcher.ignored(q)
    }

    /// Candidate eligibility of an in-tree path ([F20 §4]): representable, unbound, not ignored, not cloud-only, not in
    /// a trash location, not a never-candidate. Readability is not a test of §4: a candidate whose content a source
    /// needs and cannot read makes that source `Unavailable(unreadable)` ([F20 §4.8], §1.5).
    // spec: [F20 §4]; [40 §4.1] P4, P6
    fn candidate(&self, q: &str) -> bool {
        q.split('/').all(|s| representable(self.tree.os, s))
            && q != self.f.path
            && !self.bound(q)
            && !self.ignored(q)
            && !self.tree.cloud_only(q)
            && !self
                .fs
                .in_trash(self.tree.os, &format!("{}/{q}", self.tree.root))
            && !self.never(q)
    }

    /// Whether an enumerated file's size admits equal content ([F20 §5.9] "Candidates": for a recorded content X with a
    /// fingerprint, `nbytes_X ≤ size ≤ nbytes_X + nlines_X`); a recorded content without a fingerprint admits every
    /// size. Only a size-compatible file must be hashed, so only its unreadable content makes a source unavailable.
    fn size_admits(&self, size: u64) -> bool {
        self.recorded()
            .iter()
            .any(|o| match self.rt.fprint.get(*o) {
                Some(fp) => {
                    let nb = u64::from(fp.nbytes);
                    nb <= size && size <= nb + u64::from(fp.nlines)
                }
                None => true,
            })
    }

    /// "`oid(q) ∈ {o, last_oid}`" for a located path (E3, E3d, E5): `Ok(false)` with nothing recorded (no read), else
    /// the test over q's content, `Err(reason)` when it cannot be read ([F20 §4.8]); an unknown result (another
    /// algorithm) is `Ok(false)`: it contributes nothing.
    fn oid_test(&self, disk: &str) -> Result<bool, u8> {
        let rec = self.recorded();
        if rec.is_empty() {
            return Ok(false);
        }
        Ok(oid_in(&self.oid_at(disk)?, &rec) == Some(true))
    }

    /// The equal-content test of an enumerated candidate (E4, E7): `Ok(true)` for `oid(q) ∈ {o, last_oid}`,
    /// `Ok(false)` for a different or unknown content that needs no read, `Err(reason)` for a size-compatible file
    /// whose content cannot be read ([F20 §4.8]).
    fn equal_content(&self, q: &str) -> Result<bool, u8> {
        let size = self.tree.files.get(q).map_or(0, |f| f.bytes.len() as u64);
        if self.recorded().is_empty() || !self.size_admits(size) {
            return Ok(false);
        }
        Ok(oid_in(&self.oid_at(q)?, &self.recorded()) == Some(true))
    }

    /// The place a located in-tree path makes F `missing` ([40 §4.3] step 5): ignored output 39, trash 40,
    /// never-candidate 41, cloud-only 42; `None` for a path that is a candidate place.
    fn place_of(&self, q: &str) -> Option<u8> {
        if self
            .fs
            .in_trash(self.tree.os, &format!("{}/{q}", self.tree.root))
        {
            Some(40)
        } else if self.ignored(q) {
            Some(39)
        } else if self.never(q) {
            Some(41)
        } else if self.tree.cloud_only(q) {
            Some(42)
        } else {
            None
        }
    }
}

/// The stat tuple of a present path against `FILEOBS` ([F20 §5.2]): every component both hold, size exactly,
/// timestamps by `teq`, ids by identity; a Windows read has no id.
// spec: [F20 §5.2]
fn tuple_equals(s: &Stat, o: &FileObs, settle: bool, os: Os, g: u64) -> bool {
    let id_ok = if settle || os != Os::Windows {
        match &o.file_id {
            Some(fid) => fid.same(&s.id),
            None => true,
        }
    } else {
        true
    };
    s.size == o.size
        && teq(s.mtime_ns, o.mtime_ns, g)
        && o.creation_ns.is_none_or(|c| teq(s.btime_ns, c, g))
        && id_ok
}

/// The `replaced` test at a present path q ([F20 §5.4.1]); `Some((eoin, enio))` when it holds.
// spec: [F20 §5.4.1]
fn replaced_test(cx: &Cx<'_>, disk: &str, sid: &FileId) -> Option<(Ratio, Ratio)> {
    let o = cx.obs?;
    if cx.oid_matches(disk) != Some(false) {
        return None;
    }
    if o.file_id
        .as_ref()
        .is_none_or(|f| f.same(sid) || sid.kind == 0)
    {
        return None;
    }
    if cx.f.artifact_kind.as_deref() == Some("generated") {
        return None;
    }
    let reference = o
        .last_oid
        .as_ref()
        .or(cx.f.oid.as_ref())
        .and_then(|r| cx.rt.fprint.get(r))?;
    let b = cx.content(disk).ok()?;
    let cur = fingerprint(b)?;
    if reference.nlines < 5 || cur.nlines < 5 {
        return None;
    }
    let (eoin, enio) = estimates(reference, b);
    (eoin < Ratio::new(29, 100) && enio < Ratio::new(29, 100)).then_some((eoin, enio))
}

/// Resolves one file node in one tree ([40 §4.3] steps 0–5; [F20 §3.5]–§5.16).
// spec: [40 §4.3]; [40 §4.4]; [40 §5.2]; [F20 §5]; [F18 §2.10] I-F10
pub fn resolve_file(
    f: &FileNode,
    view: &View,
    fs: &Fs,
    git: &Git,
    rt: &Runtime,
    tree_root: &str,
    p: &Params,
) -> FileResult {
    Prep::new(view, fs, git, rt, tree_root, p).resolve(f, p)
}

impl Cx<'_> {
    /// Steps 0–5 once the root's tree is known ([40 §4.3]).
    fn resolve(&self) -> FileResult {
        let f = self.f;
        if f.status == FileStatus::Planned {
            return resolve_planned(self);
        }
        // A composite conflict renders `ambiguous (merge conflict: a | b)` until a settle resolves it ([40 §5.3]).
        // rule: LV-002
        if let Some(sides) = &f.conflict {
            let mut r = FileResult::of(
                State::Ambiguous,
                vec![Detail {
                    paths: vec![sides[0].path.clone(), sides[1].path.clone()],
                    ..Detail::code(25)
                }],
            );
            r.candidates = vec![sides[0].path.clone(), sides[1].path.clone()];
            r.fresh = freshness(self);
            return r;
        }
        if f.path_claim {
            let mut r = FileResult::of(State::Ambiguous, vec![Detail::code(28)]);
            r.fresh = freshness(self);
            return r;
        }
        // [F20 §4.9]: a path this OS cannot represent is decided with no OS call.
        if !f.path.split('/').all(|s| representable(self.tree.os, s)) {
            return FileResult::of(State::Missing, vec![Detail::code(44), Detail::code(46)]);
        }
        let mut r = match self.tree.stat(&f.path) {
            StatOut::Denied => FileResult::of(State::Unverified, vec![Detail::code(59)]),
            StatOut::Present(s) => present(self, &s),
            StatOut::Absent => absent(self),
        };
        r.fresh = freshness(self);
        r
    }
}

/// Root `abs`: existence and `oid` checks only, never a search ([40 §2.4]).
// spec: [40 §2.4] root abs
fn resolve_abs(f: &FileNode, fs: &Fs, p: &Params) -> FileResult {
    for t in fs.trees.values() {
        if let Some(rest) = f.path.strip_prefix(&format!("{}/", t.root))
            && let StatOut::Present(s) = t.stat(rest)
        {
            let mut r = FileResult::of(State::Ok, Vec::new());
            if let (Some(o), Ok(b)) = (&f.oid, read_content(t, &s.disk_path, p.max_read_bytes))
                && oid_in(&oid(p.algo, b), &[o]) == Some(false)
            {
                r.details.push(Detail::code(1));
            }
            r.at = Some(f.path.clone());
            return r;
        }
    }
    FileResult::of(State::Missing, vec![Detail::code(37), Detail::code(46)])
}

/// A `planned` node ([40 §3.2]; [F20 §5.18]): `planned`, with detail 52 when the file is present but the binding rule
/// refuses it; a writer-tree settle that may bind it reports the path. The binding rule: the tree's HEAD descends from
/// the planning commit, or the file's creation time is clearly after the planning commit's `hlc`; on a volume without
/// creation times (`VolumeCaps.btime = absent`, FAT) only the descent test applies.
// spec: [F20 §5.18]; [40 §3.2]
fn resolve_planned(cx: &Cx<'_>) -> FileResult {
    let mut r = FileResult::of(State::Planned, Vec::new());
    let StatOut::Present(s) = cx.tree.stat(&cx.f.path) else {
        return r;
    };
    let descends = match (cx.g_obs(), cx.hist, cx.head) {
        (Some(g), Some(h), Some(head)) => h.is_ancestor(g, head),
        _ => false,
    };
    let after = cx.tree.caps.btime != Btime::Absent
        && i128::from(s.btime_ns) > hlc_ns(cx.f.obs_hlc) + cx.p.skew_ns;
    if descends || after {
        r.at = Some(s.disk_path.clone());
        r.fresh = true;
        r.committed = cx.in_tau(&s.disk_path);
    } else {
        r.details.push(Detail::code(52));
    }
    r
}

/// Freshness of the tree for F ([40 §5.3]; [F18 §1.2] "fresh").
// spec: [40 §5.3] freshness rule; [F18 §1.2]
fn freshness(cx: &Cx<'_>) -> bool {
    let Some(g) = cx.g_obs() else {
        return true;
    };
    // A tree without git is trivially fresh ([40 §5.8]); an unreadable HEAD never is.
    let Some(hist) = cx.hist else {
        return true;
    };
    let Some(h) = cx.head else {
        return false;
    };
    if hist.is_ancestor(g, h) || hist.repo.tau(h).contains_key(&cx.f.path) {
        return true;
    }
    starts_at_p(cx.chain_p())
}

/// Whether E6's chain from p "starts at p" ([F20 §5.11.3]; [40 §5.3] "the tree's history held the stored path"): the
/// window holds an event of p — a pair step (after which the chain may still end deleted, split, merged or in a group),
/// an ambiguous group, a split, a merge or p's deletion; `NoStep` holds none, and `Unavailable` cannot tell. The tree
/// gate's G4 row and the freshness rule use this one predicate.
// spec: [F20 §5.11.3] "starts at p"; [40 §5.3] freshness rule
fn starts_at_p(c: &Chain) -> bool {
    !matches!(c, Chain::NoStep | Chain::Unavailable)
}

/// The checks at a present path ([F20 §5.4]).
// spec: [F20 §5.4]
fn present(cx: &Cx<'_>, s: &Stat) -> FileResult {
    let f = cx.f;
    // 1. Twins.
    if let Some(r) = twins(cx, s) {
        return r;
    }
    let spelling = (s.disk_path != f.path).then(|| Detail::path(2, &s.disk_path));
    let ok = |mut details: Vec<Detail>| {
        if let Some(sp) = &spelling {
            details.push(sp.clone());
        }
        let mut r = FileResult::of(State::Ok, details);
        r.at = Some(s.disk_path.clone());
        r
    };
    // 2. A cloud-only p decides from size, mtime and id only.
    if cx.tree.cloud_only(&s.disk_path) {
        return ok(Vec::new());
    }
    // 3. Unchanged: the recorded state with its details; a recorded `ok` (or none) with the spelling detail of §3.6,
    //    which [F18 §4.7] rule 1 admits in `ok` only.
    if let Some(o) = cx.obs
        && tuple_equals(s, o, cx.p.settle, cx.tree.os, cx.g())
    {
        return match &o.recorded {
            Some((st, d)) if *st != State::Ok => {
                let mut r = FileResult::of(*st, d.clone());
                r.at = Some(s.disk_path.clone());
                r
            }
            Some((_, d)) => ok(d
                .iter()
                .filter(|x| !matches!(x.code, 2 | 3))
                .cloned()
                .collect()),
            None => ok(Vec::new()),
        };
    }
    // 4. At a settle, when FILEOBS exists and the tuple differs.
    if cx.p.settle
        && let Some(o) = cx.obs
    {
        if let Some((eoin, enio)) = replaced_test(cx, &s.disk_path, &s.id) {
            let mut d = vec![Detail {
                scores: vec![eoin.reduced(), enio.reduced()],
                ..Detail::code(34)
            }];
            if let Some((c1, c2)) = readded(cx) {
                d.push(Detail {
                    git: vec![c1, c2],
                    ..Detail::code(35)
                });
            }
            let mut r = FileResult::of(State::Replaced, d);
            r.at = Some(s.disk_path.clone());
            return r;
        }
        if let Some(q) = path_reuse(cx, s, o) {
            let mut r = FileResult::of(State::Ambiguous, vec![Detail::path(29, &q)]);
            r.at = Some(s.disk_path.clone());
            r.candidates = vec![q];
            return r;
        }
        if let Some(r) = rename_over(cx, &s.disk_path) {
            return r;
        }
        if cx.p.writer
            && let Some(q) = git_case(cx, s)
        {
            let mut r = FileResult::of(State::MovedAuto, vec![Detail::code(5)]);
            r.at = Some(q);
            r.evidence = Some((9, "git"));
            r.committed = true;
            r.fresh = true;
            return r;
        }
    }
    // 5. Otherwise `ok`, with "changed since …" when the content is not a recorded one.
    let mut d = Vec::new();
    if cx.oid_matches(&s.disk_path) == Some(false) {
        d.push(Detail::code(1));
    }
    ok(d)
}

/// E6 showing the path deleted in one commit and re-added in a later one ([40 §4.4]): the two commits.
fn readded(cx: &Cx<'_>) -> Option<(String, String)> {
    let (hist, _, _) = (cx.hist?, cx.head?, cx.g_obs()?);
    let mut deleted = None;
    for c in cx.window() {
        let ch = hist.changes(c);
        if ch.deleted.contains_key(&cx.f.path) {
            deleted = Some(c.clone());
        } else if ch.added.contains_key(&cx.f.path)
            && let Some(d) = deleted.clone()
        {
            return Some((d, c.clone()));
        }
    }
    None
}

/// The path-reuse check ([F20 §5.4.2]): the original alive at q ≠ p inside the root with a size and mtime or `oid`
/// match.
// spec: [F20 §5.4.2]
fn path_reuse(cx: &Cx<'_>, s: &Stat, o: &FileObs) -> Option<String> {
    let fid = o.file_id.as_ref()?;
    if fid.same(&s.id) || s.id.kind == 0 {
        return None;
    }
    match cx.fs.locate_id(fid)? {
        Location::Tree(r, q) if r == cx.tree.root && q != s.disk_path => {
            let StatOut::Present(qs) = cx.tree.stat(&q) else {
                return None;
            };
            let size_mtime = qs.size == o.size && teq(qs.mtime_ns, o.mtime_ns, cx.g());
            (size_mtime || cx.oid_matches(&q) == Some(true)).then_some(q)
        }
        _ => None,
    }
}

/// Rename-over and swap ([F20 §5.4.3]).
// spec: [F20 §5.4.3]
fn rename_over(cx: &Cx<'_>, disk: &str) -> Option<FileResult> {
    let here = cx.oid_at(disk).ok()?;
    for g in &cx.view.files {
        if g.n == cx.f.n || g.root != cx.f.root || g.tombstone || g.status == FileStatus::Removed {
            continue;
        }
        let mut rec_g: Vec<&Oid> = g.oid.iter().collect();
        if let Some(lo) = cx
            .rt
            .fileobs
            .get(&(g.n, cx.tree.root.clone()))
            .and_then(|o| o.last_oid.as_ref())
        {
            rec_g.push(lo);
        }
        if rec_g.is_empty() || oid_in(&here, &rec_g) != Some(true) {
            continue;
        }
        match cx.tree.stat(&g.path) {
            StatOut::Absent => {
                let mut r = FileResult::of(State::Ambiguous, vec![Detail::code(23)]);
                r.candidates = vec![g.path.clone()];
                return Some(r);
            }
            StatOut::Present(gs) => {
                let back = cx.oid_matches(&gs.disk_path) == Some(true);
                if back {
                    let mut r = FileResult::of(State::Ambiguous, vec![Detail::code(24)]);
                    r.candidates = vec![g.path.clone()];
                    return Some(r);
                }
            }
            StatOut::Denied => {}
        }
    }
    None
}

/// `git/case` ([F20 §3.6]): at a writer-tree settle, p ∉ τ(H) and exactly one q ∈ τ(H) equal to p under `fold_v1`, other
/// than p, denoting the same file.
// spec: [F20 §3.6] git/case
fn git_case(cx: &Cx<'_>, s: &Stat) -> Option<String> {
    let tau = cx.tau_h()?;
    if tau.contains_key(&cx.f.path) {
        return None;
    }
    let qs: Vec<&String> = cx
        .pre
        .folds()
        .tau
        .get(&fold_v1(&cx.f.path))
        .into_iter()
        .flatten()
        .filter(|q| **q != cx.f.path)
        .collect();
    if qs.len() != 1 {
        return None;
    }
    match cx.tree.stat(qs[0]) {
        StatOut::Present(qs2) if qs2.id.same(&s.id) => Some(qs[0].clone()),
        _ => None,
    }
}

/// The twin rule ([F20 §3.5]): `None` when F's path is in no twin set. A member is "a node's path" when a live
/// `present` or `planned` node of F's root holds it (the nodes whose paths are spellings); its recorded contents are
/// the blob id at it in τ(H) and the `FILEOBS.last_oid` of each such node, and it matches when the content's `oid`
/// under A(R) is in that set by [F20 §2.3]'s three-valued test — a value of another algorithm is *unknown*, never a
/// match.
// spec: [F20 §3.5]; [F20 §2.3] comparison
fn twins(cx: &Cx<'_>, s: &Stat) -> Option<FileResult> {
    let f = cx.f;
    let group = cx.pre.folds().spellings.get(&fold_v1(&f.path))?;
    if group.len() < 2 {
        return None;
    }
    // The class of members denoting the same file as p.
    let z: Vec<&String> = group
        .iter()
        .filter(|m| match cx.tree.stat(m) {
            StatOut::Present(ms) => ms.id.same(&s.id),
            _ => false,
        })
        .collect();
    let node_path = |m: &str| cx.pre.holders.contains_key(m);
    if z.len() < 2 || !z.iter().any(|m| node_path(m)) {
        return None;
    }
    let here = match cx.oid_at(&s.disk_path) {
        Ok(o) => o,
        Err(reason) => {
            return Some(FileResult::of(
                State::Unverified,
                vec![Detail::code(reason)],
            ));
        }
    };
    let matches = |m: &str| -> bool {
        let mut rec: Vec<Oid> = Vec::new();
        if let (Some(t), Some(repo)) = (cx.tau_h(), cx.repo())
            && let Some(blob) = t.get(m)
            && let Some(bytes) = unhex(blob)
        {
            rec.push(Oid {
                algo: repo.algo,
                digest: bytes,
            });
        }
        for n in cx.pre.holders.get(m).into_iter().flatten() {
            if let Some(lo) = cx
                .rt
                .fileobs
                .get(&(*n, cx.tree.root.clone()))
                .and_then(|o| o.last_oid.clone())
            {
                rec.push(lo);
            }
        }
        let refs: Vec<&Oid> = rec.iter().collect();
        !refs.is_empty() && oid_in(&here, &refs) == Some(true)
    };
    let matching: Vec<&&String> = z.iter().filter(|m| matches(m)).collect();
    if matching.len() == 1 {
        if **matching[0] == f.path {
            return None;
        }
        return Some(FileResult::of(
            State::Missing,
            vec![Detail::code(44), Detail::code(46)],
        ));
    }
    let all_ceq = z.iter().all(|a| z.iter().all(|b| ceq(a, b)));
    let mut r = FileResult::of(
        State::Ambiguous,
        vec![Detail::code(if all_ceq { 27 } else { 26 })],
    );
    r.candidates = z.iter().take(3).map(|x| (*x).clone()).collect();
    Some(r)
}

/// An absent path: the normalization rule, then the tree gate and the sources ([F20 §3.6], §5.5; [40 §4.3] steps 2–5).
fn absent(cx: &Cx<'_>) -> FileResult {
    let f = cx.f;
    // The normalization rule runs before the tree gate ([F20 §3.6]).
    let dir = dirname(&f.path);
    let (_, norm_insensitive) = cx.tree.dir_equivalence(dir);
    if (dir.is_empty() || cx.tree.dir_id(dir).is_some()) && !norm_insensitive {
        let base = basename(&f.path);
        let e: Vec<String> = cx
            .tree
            .entries(dir)
            .into_iter()
            .filter(|n| n != base && ceq(n, base))
            .map(|n| {
                if dir.is_empty() {
                    n
                } else {
                    format!("{dir}/{n}")
                }
            })
            .filter(|q| cx.tree.files.contains_key(q) && !cx.bound(q))
            .collect();
        if e.len() == 1 {
            let mut r = FileResult::of(State::Ok, vec![Detail::code(3)]);
            r.at = Some(e[0].clone());
            return r;
        }
        if e.len() >= 2 {
            let mut r = FileResult::of(State::Ambiguous, vec![Detail::code(27)]);
            r.candidates = e.into_iter().take(3).collect();
            return r;
        }
    }
    match gate(cx) {
        Gate::Pending => FileResult::of(State::Pending, vec![Detail::code(50)]),
        Gate::Absent(code) => {
            let mut d = vec![Detail::code(code)];
            if let Some(g) = &f.observed_git {
                d.push(Detail::git(49, g));
            }
            FileResult::of(State::AbsentInTree, d)
        }
        Gate::Unverified(code) => FileResult::of(State::Unverified, vec![Detail::code(code)]),
        // One distinct target: `moved-needs-confirm (moved differently on this line)`; two or more: `ambiguous`, as
        // the selection's step 3 counts targets ([F20 §5.5]).
        Gate::AliasOnly(proposals) if proposals.len() == 1 => {
            let mut r = FileResult::of(State::MovedNeedsConfirm, vec![Detail::code(20)]);
            r.proposals = proposals;
            r
        }
        Gate::AliasOnly(proposals) => {
            let mut r = FileResult::of(
                State::Ambiguous,
                vec![Detail {
                    n: Some(proposals.len() as u32),
                    ..Detail::code(22)
                }],
            );
            r.candidates = proposals.iter().take(3).map(|p| p.path.clone()).collect();
            r.proposals = proposals.into_iter().take(3).collect();
            r
        }
        Gate::Search { e6, e6_only } => search(cx, e6.then(|| cx.window()), e6_only),
    }
}

/// The outcome of the tree gate ([40 §4.3] step 2; [40 §5.2]).
enum Gate {
    /// G1, G2, G4 with a chain from p, an observation made without git, or no git: the sources run; `e6` when E6 runs
    /// over F's integration window (G2, G4).
    Search { e6: bool, e6_only: bool },
    /// G3.
    Pending,
    /// G4, a chain from an alias only: the proposals, of class `git-pair` with each chain's lowest pair score
    /// ([F18 §5.4]).
    AliasOnly(Vec<Proposal>),
    /// G4, nothing: `behind` 47 or `diverged` 48.
    Absent(u8),
    /// G4 with an unreadable input: the reason.
    Unverified(u8),
}

/// The tree gate ([40 §4.3] step 2): the first matching row wins. A node whose `observed_git` is empty (an observation
/// made without git) takes the row of a tree without git after G1 — the full cascade without E6 — since no row from G2
/// on can read a commit it does not have, and the freshness rule's first bullet already makes every tree fresh for it
/// (a spec finding of WP-92 for [40 §4.3] step 2 and [F20 §5.5]).
// spec: [40 §4.3] step 2; [40 §5.2]; [F20 §5.5] which sources run
fn gate(cx: &Cx<'_>) -> Gate {
    let f = cx.f;
    let (Some(hist), Some(h)) = (cx.hist, cx.head) else {
        // No git: the cascade; freshness is trivially true ([40 §5.8]).
        return Gate::Search {
            e6: false,
            e6_only: false,
        };
    };
    let tau = hist.repo.tau(h);
    // G1.
    if tau.contains_key(&f.path) {
        return Gate::Search {
            e6: false,
            e6_only: false,
        };
    }
    let Some(g) = cx.g_obs() else {
        // An observation made without git: the row of a tree without git.
        return Gate::Search {
            e6: false,
            e6_only: false,
        };
    };
    // G2 (its window g..H is F's integration window: g is in the store).
    if hist.is_ancestor(g, h) {
        return Gate::Search {
            e6: true,
            e6_only: false,
        };
    }
    // G3.
    if f.aliases.iter().any(|a| tau.contains_key(a)) {
        return Gate::Pending;
    }
    // G4.
    let w = cx.window();
    let from_p = cx.chain_p();
    if matches!(from_p, Chain::Unavailable) {
        return Gate::Unverified(57);
    }
    if starts_at_p(from_p) {
        return Gate::Search {
            e6: true,
            e6_only: true,
        };
    }
    // Chains that start at an alias only: one proposal per distinct target, class `git-pair` with the chain's lowest
    // pair score, never applied automatically ([F18 §5.4]).
    let mut alias_hits: BTreeMap<String, u32> = BTreeMap::new();
    for a in &f.aliases {
        if let Chain::Path { path, score, .. } = hist.chain(w, a)
            && cx.tree.files.contains_key(&path)
            && cx.candidate(&path)
        {
            let s = alias_hits.entry(path).or_insert(score);
            *s = (*s).min(score);
        }
    }
    if !alias_hits.is_empty() {
        let props = alias_hits
            .into_iter()
            .map(|(path, score)| {
                Proposal::new(
                    PClass::Strong,
                    16,
                    20,
                    path,
                    Some(Ratio::new(u128::from(score), 100)),
                )
                .manual()
            })
            .collect();
        return Gate::AliasOnly(props);
    }
    if hist.repo.has(g) && hist.is_ancestor(h, g) {
        return Gate::Absent(47);
    }
    if !hist.repo.has(g) {
        return Gate::Unverified(55);
    }
    Gate::Absent(48)
}

/// The sources and the selection ([F20 §5.5]–§5.13; [40 §4.3] steps 3–5).
// spec: [F20 §5.5]
fn search(cx: &Cx<'_>, window: Option<&[String]>, e6_only: bool) -> FileResult {
    let tiny = tiny_node(cx);
    let mut ys: Vec<(&'static str, Yield)> = Vec::new();
    // E1's exact candidates also feed the copy rule's line 1 in E4 and E7.
    let first = (!e6_only).then(|| e1(cx));
    let named: Vec<(String, u8, &'static str)> =
        first.as_ref().map_or_else(Vec::new, |y| y.exact.clone());
    if let Some(y1) = first {
        ys.push(("E1", y1));
        ys.push(("E3d", e3d(cx)));
        ys.push(("E3", e3(cx)));
        ys.push(("E4", e4(cx, window, tiny, &named)));
        ys.push(("E5", e5(cx, tiny)));
    }
    if window.is_some() {
        ys.push(("E6", e6(cx)));
    }
    if !e6_only && cx.p.settle {
        let first_settle = cx
            .rt
            .trees
            .get(&cx.tree.root)
            .is_none_or(|t| !t.first_settle_done);
        if !first_settle {
            let empty = BTreeSet::new();
            let front = if cx.tree.os == Os::Windows {
                &empty
            } else {
                cx.pre.frontier.get_or_init(|| frontier(cx))
            };
            ys.push(("E7", e7(cx, window, tiny, front, &named)));
            if !tiny {
                ys.push(("E8", e8(cx, front)));
            }
        }
    }
    let mut sibling_exact: Vec<String> = ys
        .iter()
        .filter(|(src, _)| matches!(*src, "E1" | "E3d" | "E3" | "E6"))
        .flat_map(|(_, y)| y.exact.iter().map(|x| x.0.clone()))
        .collect();
    sibling_exact.sort();
    sibling_exact.dedup();
    let mut r = select(cx, ys, window);
    r.sibling_exact = sibling_exact;
    r
}

/// Whether F is tiny ([F20 §2.6.5]).
fn tiny_node(cx: &Cx<'_>) -> bool {
    let fp = cx
        .obs
        .and_then(|o| o.last_oid.as_ref())
        .or(cx.f.oid.as_ref())
        .and_then(|o| cx.rt.fprint.get(o));
    match fp {
        Some(fp) => fp.nlines < 5 || fp.nbytes < 64,
        None => cx.f.bytes.is_none_or(|b| b < 64),
    }
}

/// E1: intent and hook evidence ([F20 §5.6]). A `PENDING` row yields by its recorded class and token ([F11 §12.6]): an
/// exact row an exact candidate (`hook/<token>` for this tree's own hook row with `file-id` or `move`, else
/// `lazy/pending`); a hook's argument parse (`argv`, class `strong`) a strong proposal that policy B may apply; any
/// other row — a reader settle's proposal of class `strong`, `copy` or `weak` (an identical copy, a split, a merge, a
/// directory replaced, …) — a proposal of its own class, token and rendering that is never applied automatically, since
/// it is another observation's proposal, not this tree's evidence (a spec finding of WP-92 for [F20 §5.6]).
// spec: [F20 §5.6]; [F11 §12.6] class, source, evidence
fn e1(cx: &Cx<'_>) -> Yield {
    let mut y = Yield::default();
    for i in &cx.rt.intents {
        if i.tree != cx.tree.root || !(i.open || i.recovered) {
            continue;
        }
        for (src, dst) in &i.items {
            if *src == cx.f.path
                && let Some(q) = dst
                && cx.tree.files.contains_key(q)
            {
                y.exact
                    .push((q.clone(), if i.recovered { 2 } else { 1 }, "explicit"));
            }
        }
    }
    for pr in cx
        .rt
        .pending
        .iter()
        .filter(|p| !cx.p.no_pending && p.n == cx.f.n && p.from == cx.f.path)
    {
        if !cx.tree.files.contains_key(&pr.to) || !cx.candidate(&pr.to) {
            continue;
        }
        if pr.tree != cx.tree.root {
            let here = match cx.oid_at(&pr.to) {
                Ok(o) => o,
                Err(reason) => {
                    y.unavailable.get_or_insert(reason);
                    continue;
                }
            };
            if pr.oid.as_ref() != Some(&here) {
                continue;
            }
        }
        match (pr.class, pr.source) {
            (PClass::Exact, source) => {
                let own_hook = source == PendingSource::Hook
                    && pr.tree == cx.tree.root
                    && matches!(pr.evidence, 3 | 10);
                y.exact.push(if own_hook {
                    (pr.to.clone(), pr.evidence, "hook")
                } else {
                    (pr.to.clone(), 7, "lazy")
                });
            }
            (PClass::Strong, PendingSource::Hook) if pr.evidence == 22 => {
                y.proposals
                    .push(Proposal::new(PClass::Strong, 22, 19, pr.to.clone(), None));
            }
            (class, _) => y.proposals.push(
                Proposal::new(
                    class,
                    pr.evidence,
                    token_detail(pr.evidence),
                    pr.to.clone(),
                    None,
                )
                .manual(),
            ),
        }
    }
    y
}

/// A located path of E3 or E3d: in this tree, or elsewhere (a place that makes F `missing`).
enum Located {
    Here(String),
    Place(u8),
    Nowhere,
}

fn locate(cx: &Cx<'_>, loc: Option<Location>) -> Located {
    match loc {
        None => Located::Nowhere,
        Some(Location::Tree(r, q)) if r == cx.tree.root => match cx.place_of(&q) {
            Some(pl) => Located::Place(pl),
            None => Located::Here(q),
        },
        Some(Location::Tree(r, q)) => {
            if cx.fs.in_trash(cx.tree.os, &format!("{r}/{q}")) {
                Located::Place(40)
            } else {
                Located::Place(38)
            }
        }
        Some(Location::Elsewhere(abs)) => {
            if cx.fs.in_trash(cx.tree.os, &abs) {
                Located::Place(40)
            } else {
                Located::Place(38)
            }
        }
    }
}

/// E3d: the parent directory's id ([F20 §5.7]). It needs `FILEOBS.parent_dir_id` only: the directory id is looked up
/// on the tree's volume with the volume's id kind (`VolumeCaps.id_kind`), and a row without `file_id` leaves step 3
/// its size-and-mtime and `oid` tests.
// spec: [F20 §5.7]
fn e3d(cx: &Cx<'_>) -> Yield {
    let mut y = Yield::default();
    let dir = dirname(&cx.f.path);
    if dir.is_empty() || cx.tree.dir_id(dir).is_some() {
        return y;
    }
    let (Some(o), true) = (cx.obs, cx.tree.caps.id_locate) else {
        return y;
    };
    let Some(pd) = o.parent_dir else {
        return y;
    };
    let Some(Location::Tree(r, d2)) = cx.fs.locate_dir(&cx.tree.volume, cx.tree.caps.id_kind, pd)
    else {
        return y;
    };
    if r != cx.tree.root {
        y.place = Some(38);
        return y;
    }
    let q = if d2.is_empty() {
        basename(&cx.f.path).to_string()
    } else {
        format!("{d2}/{}", basename(&cx.f.path))
    };
    let qs = match cx.tree.stat(&q) {
        StatOut::Present(qs) => qs,
        StatOut::Denied => {
            y.unavailable = Some(59);
            return y;
        }
        StatOut::Absent => return y,
    };
    if let Some(pl) = cx.place_of(&qs.disk_path) {
        y.place = Some(pl);
        return y;
    }
    if !cx.candidate(&qs.disk_path) {
        return y;
    }
    // Step 3: the id, or size and mtime, need no read; the `oid` test does.
    let by_stat = o.file_id.as_ref().is_some_and(|fid| fid.same(&qs.id))
        || (qs.size == o.size && teq(qs.mtime_ns, o.mtime_ns, cx.g()));
    let exact = by_stat
        || match cx.oid_test(&qs.disk_path) {
            Ok(eq) => eq,
            Err(reason) => {
                y.unavailable = Some(reason);
                return y;
            }
        };
    if exact && qs.nlink <= 1 {
        y.exact.push((qs.disk_path.clone(), 4, "lazy"));
    } else if exact {
        // A hard-linked file yields at most `strong` ([F20 §5.3]).
        y.proposals.push(Proposal::new(
            PClass::Strong,
            15,
            12,
            qs.disk_path.clone(),
            None,
        ));
    } else if let Some(c) = replaced_test(cx, &qs.disk_path, &qs.id) {
        y.replaced = Some(c);
    } else {
        // Step 4: `moved-needs-confirm (directory moved, file replaced)`, class `prefix-strong` ([F18 §5.4]), never
        // applied automatically under either policy (open point 14).
        y.proposals
            .push(Proposal::new(PClass::Strong, 15, 21, qs.disk_path.clone(), None).manual());
    }
    y
}

/// E3: the file's own id ([F20 §5.8]).
// spec: [F20 §5.8]
fn e3(cx: &Cx<'_>) -> Yield {
    let mut y = Yield::default();
    let Some(o) = cx.obs else { return y };
    let Some(fid) = &o.file_id else { return y };
    if !cx.tree.caps.id_locate {
        return y;
    }
    match locate(cx, cx.fs.locate_id(fid)) {
        Located::Nowhere => {}
        Located::Place(pl) => y.place = Some(pl),
        Located::Here(q) => {
            if !cx.candidate(&q) {
                return y;
            }
            let qs = match cx.tree.stat(&q) {
                StatOut::Present(qs) => qs,
                StatOut::Denied => {
                    y.unavailable = Some(59);
                    return y;
                }
                StatOut::Absent => return y,
            };
            // Size and mtime need no read; the `oid` test does, and an unknown `oid` decides nothing.
            let exact = (qs.size == o.size && teq(qs.mtime_ns, o.mtime_ns, cx.g()))
                || match cx.oid_test(&q) {
                    Ok(eq) => eq,
                    Err(reason) => {
                        y.unavailable = Some(reason);
                        return y;
                    }
                };
            if exact && qs.nlink <= 1 {
                y.exact.push((q, 3, "lazy"));
            } else {
                // Moved and edited in place, or a hard-linked file, which yields at most `strong` ([F20 §5.3]).
                y.proposals
                    .push(Proposal::new(PClass::Strong, 14, 11, q, None));
            }
        }
    }
    y
}

/// The outcome of the copy rule for an equal-`oid` candidate ([F20 §5.9]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CopyOutcome {
    /// Exact, with the evidence (token code, `how`): line 1 (E6 `git/r100`, or E1's own token) or line 2
    /// (`lazy/oid+ctime`).
    Exact(u8, &'static str),
    /// Line 4: an identical-copy proposal.
    Proposal,
    /// Line 3: a copy that coexisted with F, never a candidate.
    Never,
}

/// E4's enumerated scope: its directories (those that exist) and the regular files directly inside them.
struct Near<'s> {
    dirs: &'s BTreeSet<String>,
    files: &'s [String],
}

/// The copy rule for an equal-`oid` candidate from E4 (with `near`) or E7 (without): the first matching line decides
/// ([F20 §5.9]; [40 §4.3]; I-F13). `named` are E1's exact candidates.
// spec: [F20 §5.9] copy rule; [F18 §2.13] I-F13
fn copy_rule(
    cx: &Cx<'_>,
    q: &str,
    window: Option<&[String]>,
    near: Option<&Near<'_>>,
    named: &[(String, u8, &'static str)],
) -> CopyOutcome {
    // 1. E6 shows p → q inside one commit of the window, or E1 names q.
    if let (Some(hist), Some(w)) = (cx.hist, window)
        && hist.renamed_in_one_commit(w, &cx.f.path, q)
    {
        return CopyOutcome::Exact(8, "git");
    }
    if let Some(&(_, ev, how)) = named.iter().find(|(x, _, _)| x == q) {
        return CopyOutcome::Exact(ev, how);
    }
    let Some(o) = cx.obs else {
        return CopyOutcome::Proposal;
    };
    // Lines 2 and 3 read creation times, which a volume without them (`btime = absent`) never supplies.
    let Some(fc) = o
        .creation_ns
        .filter(|_| cx.tree.caps.btime != Btime::Absent)
    else {
        return CopyOutcome::Proposal;
    };
    // Times as the enumeration reads them (a denied read does not hide an entry's times).
    let Some(qf) = cx.tree.files.get(q) else {
        return CopyOutcome::Proposal;
    };
    let g = cx.g();
    let v = cx.verified_at();
    // 2. The creation-time line, for E4's candidates on a TunneledNotCopied volume only.
    if let Some(near) = near {
        let unique_here = near.files.iter().filter(|x| x.as_str() != q).all(|x| {
            cx.tree
                .files
                .get(x)
                .is_none_or(|xf| !teq(xf.btime_ns, qf.btime_ns, g))
        });
        let unique_nodes = cx.view.files.iter().all(|h| {
            h.n == cx.f.n
                || h.root != cx.f.root
                || !near.dirs.contains(dirname(&h.path))
                || cx
                    .rt
                    .fileobs
                    .get(&(h.n, cx.tree.root.clone()))
                    .and_then(|ho| ho.creation_ns)
                    .is_none_or(|hc| !teq(hc, qf.btime_ns, g))
        });
        let ctime_ok = match cx.tree.caps.ctime_on_rename {
            Some(true) => i128::from(qf.ctime_ns) > hlc_ns(v) + cx.p.skew_ns,
            _ => false,
        };
        if cx.tree.caps.btime == Btime::TunneledNotCopied
            && !qf.attrs.contains("clone")
            && teq(qf.btime_ns, fc, g)
            && unique_here
            && unique_nodes
            && ctime_ok
        {
            return CopyOutcome::Exact(5, "lazy");
        }
    }
    // 3. A copy that coexisted with F.
    if !teq(qf.btime_ns, fc, g) && i128::from(qf.btime_ns) < hlc_ns(v) + cx.p.skew_ns {
        return CopyOutcome::Never;
    }
    // 4. Otherwise an identical-copy proposal.
    CopyOutcome::Proposal
}

/// E4: near candidates and the copy rule ([F20 §5.9]). A tiny F's equal-`oid` candidates only make it `ambiguous`
/// ([F20 §5.5] "Tiny files").
// spec: [F20 §5.9] E4
fn e4(
    cx: &Cx<'_>,
    window: Option<&[String]>,
    tiny: bool,
    named: &[(String, u8, &'static str)],
) -> Yield {
    let mut y = Yield::default();
    let p = &cx.f.path;
    let mut dirs: BTreeSet<String> = BTreeSet::new();
    let d = dirname(p).to_string();
    dirs.insert(d.clone());
    if !d.is_empty() {
        dirs.insert(dirname(&d).to_string());
    }
    for a in &cx.f.aliases {
        dirs.insert(dirname(a).to_string());
    }
    for m in cx.view.moves.get(&cx.f.root).into_iter().flatten() {
        if let Some(rest) = p.strip_prefix(m.from.text.as_str()) {
            dirs.insert(dirname(&format!("{}{rest}", m.to.text)).to_string());
        }
    }
    dirs.retain(|x| x.is_empty() || cx.tree.dir_id(x).is_some());
    let files: Vec<String> = dirs.iter().flat_map(|x| cx.tree.files_in(x)).collect();
    let near = Near {
        dirs: &dirs,
        files: &files,
    };
    for q in &files {
        if !cx.candidate(q) {
            continue;
        }
        match cx.equal_content(q) {
            Ok(true) => {}
            Ok(false) => continue,
            Err(reason) => {
                y.unavailable.get_or_insert(reason);
                continue;
            }
        }
        match copy_rule(cx, q, window, Some(&near), named) {
            CopyOutcome::Never => {}
            _ if tiny => y.tiny_eq.push(q.clone()),
            CopyOutcome::Exact(ev, how) => y.exact.push((q.clone(), ev, how)),
            CopyOutcome::Proposal => {
                y.proposals
                    .push(Proposal::new(PClass::Copy, 13, 10, q.clone(), None))
            }
        }
    }
    y
}

/// E5: recorded directory moves, and sibling inference supplied by the settle ([F20 §5.10]). A tiny F's equal-`oid`
/// candidates only make it `ambiguous` ([F20 §5.5] "Tiny files").
// spec: [F20 §5.10]
fn e5(cx: &Cx<'_>, tiny: bool) -> Yield {
    let mut y = Yield::default();
    let mut moves: Vec<&PathMove> = cx
        .view
        .moves
        .get(&cx.f.root)
        .into_iter()
        .flatten()
        .collect();
    moves.sort();
    let compose = |recorded: bool| -> Option<String> {
        let mut x = cx.f.path.clone();
        for m in &moves {
            let rec = matches!(
                m.class,
                MoveClass::Explicit | MoveClass::Confirmed | MoveClass::Committed
            );
            if rec != recorded {
                continue;
            }
            if let Some(rest) = x.strip_prefix(m.from.text.as_str()) {
                x = format!("{}{rest}", m.to.text);
            }
        }
        (x != cx.f.path).then_some(x)
    };
    let mut targets: Vec<(String, bool)> = Vec::new();
    for recorded in [true, false] {
        if let Some(q) = compose(recorded) {
            targets.push((q, recorded));
        }
    }
    // Sibling inference: a directory move the settle inferred from at least two other nodes' exact candidates.
    for (x, yy) in &cx.p.siblings {
        if let Some(rest) = cx.f.path.strip_prefix(x.as_str()) {
            targets.push((format!("{yy}{rest}"), false));
        }
    }
    for (q, recorded) in targets {
        let qs = match cx.tree.stat(&q) {
            StatOut::Present(qs) => qs,
            StatOut::Denied => {
                y.unavailable.get_or_insert(59);
                continue;
            }
            StatOut::Absent => continue,
        };
        if !cx.candidate(&qs.disk_path) {
            continue;
        }
        let same = match cx.oid_test(&qs.disk_path) {
            Ok(same) => same,
            Err(reason) => {
                y.unavailable.get_or_insert(reason);
                continue;
            }
        };
        if tiny {
            if same {
                y.tiny_eq.push(qs.disk_path.clone());
            }
        } else if recorded && same {
            y.exact.push((qs.disk_path.clone(), 6, "lazy"));
        } else {
            y.proposals.push(Proposal::new(
                PClass::Strong,
                15,
                12,
                qs.disk_path.clone(),
                None,
            ));
        }
    }
    y
}

/// E6: per-commit renames over F's integration window ([F20 §5.11]): an exact chain is exact (`git/r100`); a strong
/// chain a `git-pair` proposal; a weak one a `weak` proposal; an identical-blob group a tie over its candidates that
/// are present in T and pass §4 (none: nothing), which is `ambiguous` whatever its size, since git's evidence cannot
/// tell its members apart (a spec finding of WP-92 for [F20 §5.11.3]); a split one proposal listing its pieces and a
/// merge a proposal into the host, neither ever applied automatically ([F20 §5.11.4]; [40 §4.4]).
// spec: [F20 §5.11]
fn e6(cx: &Cx<'_>) -> Yield {
    let mut y = Yield::default();
    if cx.hist.is_none() {
        return y;
    }
    let eligible = |q: &str| cx.tree.files.contains_key(q) && cx.candidate(q);
    match cx.chain_p().clone() {
        Chain::Path { path, class, score } if eligible(&path) => {
            let s = Some(Ratio::new(u128::from(score), 100));
            match class {
                Class::Exact => y.exact.push((path, 8, "git")),
                Class::Strong => y
                    .proposals
                    .push(Proposal::new(PClass::Strong, 16, 13, path, s)),
                Class::Weak => y
                    .proposals
                    .push(Proposal::new(PClass::Weak, 19, 16, path, s)),
            }
        }
        Chain::Path { .. } | Chain::NoStep => {}
        Chain::Ambiguous(c) => {
            let present: Vec<String> = c.into_iter().filter(|q| eligible(q)).collect();
            if !present.is_empty() {
                y.tie = Some(present);
            }
        }
        Chain::Split(pieces) => {
            let present: Vec<String> = pieces.into_iter().filter(|q| eligible(q)).collect();
            if let Some(first) = present.first() {
                let mut sp = Proposal::new(PClass::Strong, 20, 17, first.clone(), None).manual();
                sp.pieces = present;
                y.proposals.push(sp);
            }
        }
        Chain::Merged(host, s) if eligible(&host) => y
            .proposals
            .push(Proposal::new(PClass::Strong, 21, 18, host, Some(s)).manual()),
        Chain::Merged(..) => {}
        Chain::Deleted(c) => y.deleted_in = Some(c),
        Chain::Unavailable => y.unavailable = Some(57),
    }
    y
}

/// `tge(a, b)` ([F20 §5.1]).
fn tge(a: i64, b: i64, g: u64) -> bool {
    let g = g as i64;
    a.div_euclid(g) >= b.div_euclid(g)
}

/// The changed-directory frontier of a Linux or macOS tree ([F20 §5.12.1]; [80 §2.11.3]): every directory with a
/// `DIRMAP` row whose mtime is not `teq` the row's or is `tge` the racy threshold T0, and, recursively, every
/// directory without a row found inside a frontier directory (the root without a row starts there). T0 is the settle
/// stamp's mtime, or on another volume than the store's the largest row mtime of the tree; without T0 every
/// directory counts as changed.
// spec: [F20 §5.12.1]
fn frontier(cx: &Cx<'_>) -> BTreeSet<String> {
    let t = cx.tree;
    let g = cx.g();
    let rows: BTreeMap<u64, i64> = cx
        .rt
        .dirmap
        .iter()
        .filter(|((tree, _), _)| *tree == t.root)
        .map(|((_, id), (_, m))| (*id, *m))
        .collect();
    let t0 = cx.stamp_ns.or_else(|| rows.values().copied().max());
    let mut stack: Vec<String> = t
        .dirs
        .iter()
        .filter(|(_, d)| {
            rows.get(&d.id).is_some_and(|m| match t0 {
                None => true,
                Some(t0) => !teq(d.mtime_ns, *m, g) || tge(d.mtime_ns, t0, g),
            })
        })
        .map(|(p, _)| p.clone())
        .collect();
    if t.dirs.get("").is_some_and(|d| !rows.contains_key(&d.id)) {
        stack.push(String::new());
    }
    let mut out = BTreeSet::new();
    while let Some(d) = stack.pop() {
        if out.insert(d.clone()) {
            stack.extend(
                t.dirs_in(&d)
                    .into_iter()
                    .filter(|s| t.dirs.get(s).is_some_and(|x| !rows.contains_key(&x.id))),
            );
        }
    }
    out
}

/// The E7 time predicate of a file ([F20 §5.12]), from the times the enumeration reads: a relevant time of q is
/// possibly after the last settle `hlc` h (Windows: ChangeTime or CreationTime; Linux: ctime; macOS: `ADDEDTIME` or
/// ctime), or q lies under a directory whose ChangeTime or CreationTime is possibly after h (Windows), or directly in a
/// directory of the changed-directory frontier (Linux, macOS).
// spec: [F20 §5.12] candidate set
fn e7_time(cx: &Cx<'_>, q: &str, front: &BTreeSet<String>) -> bool {
    let h = cx
        .rt
        .trees
        .get(&cx.tree.root)
        .map_or(0, |t| t.last_settle_hlc);
    let lo = hlc_ns(h) - cx.p.skew_ns;
    let after = |x: i64| i128::from(x) > lo;
    let Some(f) = cx.tree.files.get(q) else {
        return false;
    };
    match cx.tree.os {
        Os::Windows => {
            let mut d = q;
            let mut dir_after = false;
            while !d.is_empty() && !dir_after {
                d = dirname(d);
                dir_after = cx
                    .tree
                    .dirs
                    .get(d)
                    .is_some_and(|x| after(x.ctime_ns) || after(x.btime_ns));
            }
            after(f.ctime_ns) || after(f.btime_ns) || dir_after
        }
        Os::Linux => after(f.ctime_ns) || front.contains(dirname(q)),
        Os::Macos => after(f.added_ns) || after(f.ctime_ns) || front.contains(dirname(q)),
    }
}

/// E7: changes since the last settle, through the copy rule without its line 2 ([F20 §5.12]). A tiny F's equal-`oid`
/// candidates only make it `ambiguous` ([F20 §5.5] "Tiny files").
// spec: [F20 §5.12]
fn e7(
    cx: &Cx<'_>,
    window: Option<&[String]>,
    tiny: bool,
    front: &BTreeSet<String>,
    named: &[(String, u8, &'static str)],
) -> Yield {
    let mut y = Yield::default();
    for q in cx.tree.files.keys() {
        if !e7_time(cx, q, front) || !cx.candidate(q) {
            continue;
        }
        match cx.equal_content(q) {
            Ok(true) => {}
            Ok(false) => continue,
            Err(reason) => {
                y.unavailable.get_or_insert(reason);
                continue;
            }
        }
        match copy_rule(cx, q, window, None, named) {
            CopyOutcome::Never => {}
            _ if tiny => y.tiny_eq.push(q.clone()),
            CopyOutcome::Exact(ev, how) => y.exact.push((q.clone(), ev, how)),
            CopyOutcome::Proposal => {
                y.proposals
                    .push(Proposal::new(PClass::Copy, 13, 10, q.clone(), None))
            }
        }
    }
    y
}

/// E8: edited and moved ([F20 §5.13]).
// spec: [F20 §5.13]
fn e8(cx: &Cx<'_>, front: &BTreeSet<String>) -> Yield {
    let mut y = Yield::default();
    let reference = cx
        .obs
        .and_then(|o| o.last_oid.as_ref())
        .or(cx.f.oid.as_ref())
        .and_then(|o| cx.rt.fprint.get(o));
    let Some(r) = reference else { return y };
    let base = basename(&cx.f.path);
    for q in cx.tree.files.keys() {
        if basename(q) != base || !e7_time(cx, q, front) || !cx.candidate(q) {
            continue;
        }
        let b = match cx.content(q) {
            Ok(b) => b,
            Err(reason) => {
                y.unavailable.get_or_insert(reason);
                continue;
            }
        };
        if !is_text(b) {
            continue;
        }
        let (eoin, enio) = estimates(r, b);
        if eoin >= Ratio::new(4, 5) && enio >= Ratio::new(4, 5) {
            y.proposals.push(Proposal::new(
                PClass::Strong,
                17,
                14,
                q.clone(),
                Some(eoin.min(enio).reduced()),
            ));
        }
    }
    y
}

/// The principal detail of a proposal's `moved-needs-confirm` ([F18 §4.6]): the detail the proposal carries, with its
/// score when the token is scored.
fn proposal_detail(p: &Proposal) -> Detail {
    match p.score {
        Some(s) => Detail::score(p.detail, s),
        None => Detail::code(p.detail),
    }
}

/// An `ambiguous (<n> candidates)` result listing the first 3 targets.
fn candidates_result(targets: Vec<String>, proposals: Vec<Proposal>) -> FileResult {
    let mut r = FileResult::of(
        State::Ambiguous,
        vec![Detail {
            n: Some(targets.len() as u32),
            ..Detail::code(22)
        }],
    );
    r.candidates = targets.into_iter().take(3).collect();
    r.proposals = proposals.into_iter().take(3).collect();
    r
}

/// Selection and classification ([F20 §5.5] steps 1–5, [F20 §1.5]; [40 §4.3] step 5).
// spec: [F20 §5.5] selection; [F20 §1.5]
fn select(cx: &Cx<'_>, ys: Vec<(&'static str, Yield)>, window: Option<&[String]>) -> FileResult {
    // 1. The first source with exactly one exact candidate.
    for (_, y) in &ys {
        let mut ex = y.exact.clone();
        ex.sort_by(|a, b| a.0.cmp(&b.0));
        ex.dedup_by(|a, b| a.0 == b.0);
        if ex.len() == 1 && y.tie.is_none() {
            let (q, ev, how) = ex.remove(0);
            let mut r = FileResult::of(State::MovedAuto, vec![Detail::code(5)]);
            r.committed = cx.in_tau(&q);
            let rec = if !cx.p.writer {
                8
            } else if cx.p.main && !r.committed {
                9
            } else {
                7
            };
            r.details.push(Detail::code(rec));
            r.at = Some(q);
            r.evidence = Some((ev, how));
            r.committed_moves = committed_moves(cx, window);
            return r;
        }
    }
    // 2. A source with two or more exact candidates, or an identical-blob group.
    for (_, y) in &ys {
        let mut ex: Vec<String> = y.exact.iter().map(|x| x.0.clone()).collect();
        ex.sort();
        ex.dedup();
        if let Some(t) = &y.tie {
            return candidates_result(t.clone(), Vec::new());
        }
        if ex.len() >= 2 {
            return candidates_result(ex, Vec::new());
        }
    }
    // E3d's replaced verdict at the located path.
    for (_, y) in &ys {
        if let Some((eoin, enio)) = y.replaced {
            return FileResult::of(
                State::Replaced,
                vec![Detail {
                    scores: vec![eoin.reduced(), enio.reduced()],
                    ..Detail::code(34)
                }],
            );
        }
    }
    // 3. Strong and copy proposals of all sources, in source order then path order, without duplicates.
    let mut props: Vec<Proposal> = Vec::new();
    for (_, y) in &ys {
        let mut ps: Vec<&Proposal> = y
            .proposals
            .iter()
            .filter(|p| matches!(p.class, PClass::Strong | PClass::Copy))
            .collect();
        ps.sort_by(|a, b| a.path.cmp(&b.path));
        for p in ps {
            if !props.iter().any(|x| x.path == p.path) {
                props.push(p.clone());
            }
        }
    }
    // A tiny F's equal-`oid` candidates of E4, E5 and E7 make it `ambiguous`, listed with the other targets
    // ([F20 §5.5] "Tiny files").
    let mut tiny_eq: Vec<String> = Vec::new();
    for (_, y) in &ys {
        let mut t = y.tiny_eq.clone();
        t.sort();
        for q in t {
            if !tiny_eq.contains(&q) {
                tiny_eq.push(q);
            }
        }
    }
    if !tiny_eq.is_empty() {
        let mut targets: Vec<String> = props.iter().map(|p| p.path.clone()).collect();
        for q in tiny_eq {
            if !targets.contains(&q) {
                targets.push(q);
            }
        }
        return candidates_result(targets, props);
    }
    if props.len() == 1 {
        let p0 = props[0].clone();
        let mut r = FileResult::of(State::MovedNeedsConfirm, vec![proposal_detail(&p0)]);
        r.candidates = p0.pieces.iter().take(3).cloned().collect();
        // Under `files.policy.auto = strong`, one distinct target whose proposal policy B may apply (a strong proposal
        // of a class [F18 §5.4] lists) is applied as a marked guess ([40 §9.2] #1).
        if cx.p.policy_strong && p0.auto {
            r.guess = Some((p0.evidence, p0.score));
            r.at = Some(p0.path.clone());
            r.committed = cx.in_tau(&p0.path);
        }
        r.proposals = props;
        return r;
    }
    if props.len() >= 2 {
        let targets = props.iter().map(|p| p.path.clone()).collect();
        return candidates_result(targets, props);
    }
    // 4. Weak proposals.
    let mut weak: Vec<Proposal> = Vec::new();
    for (_, y) in &ys {
        for p in y.proposals.iter().filter(|p| p.class == PClass::Weak) {
            if !weak.iter().any(|x| x.path == p.path) {
                weak.push(p.clone());
            }
        }
    }
    if weak.len() == 1 {
        let mut r = FileResult::of(State::MovedNeedsConfirm, vec![proposal_detail(&weak[0])]);
        r.proposals = weak;
        return r;
    }
    if weak.len() >= 2 {
        let targets = weak.iter().map(|p| p.path.clone()).collect();
        return candidates_result(targets, weak);
    }
    // 5. `missing`, with the place; `unverified` when a source that applied was unavailable.
    if let Some(u) = ys.iter().find_map(|(_, y)| y.unavailable) {
        return FileResult::of(State::Unverified, vec![Detail::code(u)]);
    }
    let mut d = Vec::new();
    if let Some(o) = cx.obs
        && o.missing_since != 0
    {
        d.push(Detail::code(36));
    }
    if let Some(pl) = ys.iter().find_map(|(_, y)| y.place) {
        d.push(Detail::code(pl));
    } else if let Some(c) = ys.iter().find_map(|(_, y)| y.deleted_in.clone()) {
        d.push(Detail::git(43, &c));
    } else {
        d.push(Detail::code(37));
    }
    d.push(Detail::code(46));
    FileResult::of(State::Missing, d)
}

/// The `committed` directory moves a writer-tree settle records with its re-binds ([F20 §5.16]).
fn committed_moves(cx: &Cx<'_>, window: Option<&[String]>) -> Vec<(String, String, String)> {
    let (Some(hist), Some(h), Some(w)) = (cx.hist, cx.head, window) else {
        return Vec::new();
    };
    if !(cx.p.settle && cx.p.writer) {
        return Vec::new();
    }
    if let Some(m) = cx.pre.moves.borrow().get(w) {
        return m.as_ref().clone();
    }
    let m = Rc::new(hist.committed_moves(w, h));
    cx.pre.moves.borrow_mut().insert(w.to_vec(), Rc::clone(&m));
    m.as_ref().clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::r4::git::{Commit, Head};
    use crate::r4::tree::{TreeOp, VolumeCaps};

    const R: &str = "C:/work/repo";

    fn node(path: &str, content: &[u8]) -> FileNode {
        FileNode {
            n: 1,
            root: "project".into(),
            path: path.into(),
            oid: Some(oid(Algo::Sha1, content)),
            bytes: Some(content.len() as u64),
            observed_git: None,
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

    fn fs_with(files: &[(&str, &[u8])]) -> Fs {
        let mut fs = Fs::default();
        fs.ensure_tree(R, "C", VolumeCaps::NTFS, Os::Windows);
        for (p, b) in files {
            fs.apply(
                R,
                &TreeOp::Write {
                    path: p.to_string(),
                    bytes: b.to_vec(),
                    btime_ns: None,
                },
                1_000,
            )
            .unwrap();
        }
        fs
    }

    fn obs_of(fs: &Fs, path: &str, verified_at: u64) -> FileObs {
        let StatOut::Present(s) = fs.trees[R].stat(path) else {
            panic!()
        };
        FileObs {
            file_id: Some(s.id.clone()),
            parent_dir: Some(s.parent),
            size: s.size,
            mtime_ns: s.mtime_ns,
            creation_ns: Some(s.btime_ns),
            last_oid: None,
            verified_at,
            missing_since: 0,
            recorded: None,
            path_seen: None,
        }
    }

    const BODY: &[u8] =
        b"fn a() {}\nfn b() {}\nfn c() {}\nfn d() {}\nfn e() {}\nfn f() {}\nfn g() {}\n";

    #[test]
    fn present_paths_are_ok_and_a_file_id_rename_is_moved_auto() {
        let mut fs = fs_with(&[("src/a.rs", BODY)]);
        let f = node("src/a.rs", BODY);
        let view = View {
            files: vec![f.clone()],
            moves: BTreeMap::new(),
            anchor_conflicts: Vec::new(),
        };
        let mut rt = Runtime::default();
        let p = Params {
            settle: true,
            writer: true,
            ..Params::default()
        };
        assert_eq!(
            resolve_file(&f, &view, &fs, &Git::default(), &rt, R, &p).state,
            State::Ok
        );
        rt.fileobs
            .insert((1, R.into()), obs_of(&fs, "src/a.rs", 5 << 16));
        fs.apply(
            R,
            &TreeOp::Mv {
                from: "src/a.rs".into(),
                to: "src/b.rs".into(),
            },
            2_000,
        )
        .unwrap();
        let r = resolve_file(&f, &view, &fs, &Git::default(), &rt, R, &p);
        assert_eq!(r.state, State::MovedAuto);
        assert_eq!(r.at.as_deref(), Some("src/b.rs"));
        assert_eq!(r.evidence, Some((3, "lazy")));
        assert!(r.fresh);
    }

    #[test]
    fn a_directory_rename_is_found_by_the_parent_id() {
        let mut fs = fs_with(&[("docs/x.md", BODY), ("docs/y.md", b"other\n")]);
        let f = node("docs/x.md", BODY);
        let view = View {
            files: vec![f.clone()],
            moves: BTreeMap::new(),
            anchor_conflicts: Vec::new(),
        };
        let mut rt = Runtime::default();
        let mut o = obs_of(&fs, "docs/x.md", 5 << 16);
        // The file's own id is stale (an edit by replace-by-rename), so only the directory's id finds it.
        o.file_id = Some(FileId {
            kind: 1,
            volume: "C".into(),
            id: 999,
        });
        rt.fileobs.insert((1, R.into()), o);
        fs.apply(
            R,
            &TreeOp::Mv {
                from: "docs".into(),
                to: "archive".into(),
            },
            2_000,
        )
        .unwrap();
        let r = resolve_file(
            &f,
            &view,
            &fs,
            &Git::default(),
            &rt,
            R,
            &Params {
                settle: true,
                writer: true,
                ..Params::default()
            },
        );
        assert_eq!(
            (r.state, r.at.as_deref()),
            (State::MovedAuto, Some("archive/x.md"))
        );
        assert_eq!(r.evidence, Some((4, "lazy")));
    }

    #[test]
    fn an_identical_copy_without_corroboration_is_a_proposal() {
        let mut fs = fs_with(&[("a/x.rs", BODY)]);
        let f = node("a/x.rs", BODY);
        let view = View {
            files: vec![f.clone()],
            moves: BTreeMap::new(),
            anchor_conflicts: Vec::new(),
        };
        fs.apply(
            R,
            &TreeOp::Cp {
                from: "a/x.rs".into(),
                to: "a/y.rs".into(),
                keep_btime: false,
            },
            3_000,
        )
        .unwrap();
        fs.apply(
            R,
            &TreeOp::Rm {
                path: "a/x.rs".into(),
            },
            3_000,
        )
        .unwrap();
        // No FILEOBS row: the copy rule never yields exact.
        let r = resolve_file(
            &f,
            &view,
            &fs,
            &Git::default(),
            &Runtime::default(),
            R,
            &Params::default(),
        );
        assert_eq!(r.state, State::MovedNeedsConfirm);
        assert_eq!(r.details[0].code, 10);
    }

    #[test]
    fn a_backup_copy_is_never_a_candidate() {
        let mut fs = fs_with(&[("x.rs", BODY)]);
        let f = node("x.rs", BODY);
        let view = View {
            files: vec![f.clone()],
            moves: BTreeMap::new(),
            anchor_conflicts: Vec::new(),
        };
        fs.apply(
            R,
            &TreeOp::Cp {
                from: "x.rs".into(),
                to: "x.rs.bak".into(),
                keep_btime: true,
            },
            3_000,
        )
        .unwrap();
        fs.apply(
            R,
            &TreeOp::Rm {
                path: "x.rs".into(),
            },
            3_000,
        )
        .unwrap();
        let r = resolve_file(
            &f,
            &view,
            &fs,
            &Git::default(),
            &Runtime::default(),
            R,
            &Params::default(),
        );
        assert_eq!(r.state, State::Missing);
        assert_eq!(
            r.details.iter().map(|d| d.code).collect::<Vec<_>>(),
            [37, 46]
        );
    }

    fn repo_with(cs: Vec<Commit>, head: &str) -> Git {
        let mut g = Git::default();
        g.repos.insert(
            "r".into(),
            Repo {
                algo: Algo::Sha1,
                commits: cs.into_iter().map(|c| (c.id.clone(), c)).collect(),
                refs: BTreeMap::from([("refs/heads/main".to_string(), head.to_string())]),
                blobs: BTreeMap::new(),
            },
        );
        g.heads
            .insert(R.into(), ("r".into(), Head::Ref("refs/heads/main".into())));
        g
    }

    fn commit(id: &str, parents: &[&str], tree: &[(&str, &str)]) -> Commit {
        Commit {
            id: id.into(),
            parents: parents.iter().map(|s| s.to_string()).collect(),
            committer_time: 100,
            author_time: 100,
            tree: tree
                .iter()
                .map(|(p, b)| (p.to_string(), b.to_string()))
                .collect(),
        }
    }

    #[test]
    fn the_tree_gate_rows() {
        let fs = fs_with(&[("new/a.rs", BODY)]);
        let mut f = node("old/a.rs", BODY);
        f.observed_git = Some("c1".into());
        let view = View {
            files: vec![f.clone()],
            moves: BTreeMap::new(),
            anchor_conflicts: Vec::new(),
        };
        let rt = Runtime::default();
        let p = Params {
            settle: true,
            writer: true,
            ..Params::default()
        };
        // G2: the observation's commit is an ancestor of H, and E6 renames old/ → new/.
        let git = repo_with(
            vec![
                commit("c1", &[], &[("old/a.rs", "b1")]),
                commit("c2", &["c1"], &[("new/a.rs", "b1")]),
            ],
            "c2",
        );
        let r = resolve_file(&f, &view, &fs, &git, &rt, R, &p);
        assert_eq!(
            (r.state, r.at.as_deref(), r.evidence),
            (State::MovedAuto, Some("new/a.rs"), Some((8, "git")))
        );
        assert!(r.committed && r.fresh);
        // G3: an alias is in τ(H) and the move has not reached this tree.
        let mut f3 = node("new/a.rs", BODY);
        f3.path = "moved/a.rs".into();
        f3.aliases = vec!["new/a.rs".into()];
        f3.observed_git = Some("zz".into());
        let r = resolve_file(&f3, &view, &fs, &git, &rt, R, &p);
        assert_eq!(r.state, State::Pending);
        // G4: behind — H is an ancestor of the observation.
        let git_behind = repo_with(
            vec![
                commit("c1", &[], &[("x.rs", "b0")]),
                commit("c2", &["c1"], &[("old/a.rs", "b1")]),
            ],
            "c1",
        );
        let r = resolve_file(&f, &view, &fs, &git_behind, &rt, R, &Params::default());
        let mut fb = f.clone();
        fb.observed_git = Some("c2".into());
        let r2 = resolve_file(&fb, &view, &fs, &git_behind, &rt, R, &Params::default());
        assert_eq!(
            r.state,
            State::Missing,
            "G2 holds (c1 is H) and E6's empty window finds nothing"
        );
        assert_eq!(r2.state, State::AbsentInTree);
        assert_eq!(r2.details[0].code, 47);
        // G4: the observation's commit is not in this repository.
        let mut fx = f.clone();
        fx.observed_git = Some("unknown".into());
        let r = resolve_file(&fx, &view, &fs, &git_behind, &rt, R, &Params::default());
        assert_eq!((r.state, r.details[0].code), (State::Unverified, 55));
    }

    #[test]
    fn twins_and_spellings() {
        let fs = fs_with(&[("Docs/Plan.md", BODY)]);
        let f = node("docs/plan.md", BODY);
        let view = View {
            files: vec![f.clone()],
            moves: BTreeMap::new(),
            anchor_conflicts: Vec::new(),
        };
        let r = resolve_file(
            &f,
            &view,
            &fs,
            &Git::default(),
            &Runtime::default(),
            R,
            &Params::default(),
        );
        assert_eq!(r.state, State::Ok);
        assert_eq!(r.details.last().map(|d| d.code), Some(2));
        // Two nodes, one file on a case-insensitive volume: only the twin whose content matches resolves.
        let mut g = node("Docs/Plan.md", b"other content\n");
        g.n = 2;
        let view2 = View {
            files: vec![f.clone(), g.clone()],
            moves: BTreeMap::new(),
            anchor_conflicts: Vec::new(),
        };
        let mut rt = Runtime::default();
        rt.fileobs.insert(
            (1, R.into()),
            FileObs {
                last_oid: Some(oid(Algo::Sha1, BODY)),
                ..FileObs::default()
            },
        );
        rt.fileobs.insert(
            (2, R.into()),
            FileObs {
                last_oid: Some(oid(Algo::Sha1, b"other content\n")),
                ..FileObs::default()
            },
        );
        let r1 = resolve_file(&f, &view2, &fs, &Git::default(), &rt, R, &Params::default());
        let r2 = resolve_file(&g, &view2, &fs, &Git::default(), &rt, R, &Params::default());
        assert_eq!(r1.state, State::Ok);
        assert_eq!((r2.state, r2.details[0].code), (State::Missing, 44));
        // The NFD spelling of an NFC name on NTFS: the normalization rule.
        let fs2 = fs_with(&[("caf\u{e9}.md", BODY)]);
        let fn_ = node("cafe\u{301}.md", BODY);
        let r = resolve_file(
            &fn_,
            &View {
                files: vec![fn_.clone()],
                moves: BTreeMap::new(),
                anchor_conflicts: Vec::new(),
            },
            &fs2,
            &Git::default(),
            &Runtime::default(),
            R,
            &Params::default(),
        );
        assert_eq!(
            (r.state, r.details[0].code, r.at.as_deref()),
            (State::Ok, 3, Some("caf\u{e9}.md"))
        );
    }

    #[test]
    fn status_and_eligibility() {
        let fs = fs_with(&[]);
        let mut f = node("a.rs", BODY);
        let view = View::default();
        f.status = FileStatus::Removed;
        assert_eq!(
            resolve_file(
                &f,
                &view,
                &fs,
                &Git::default(),
                &Runtime::default(),
                R,
                &Params::default()
            )
            .state,
            State::Deleted
        );
        f.status = FileStatus::Present;
        let p = Params {
            eligible: false,
            ..Params::default()
        };
        let r = resolve_file(&f, &view, &fs, &Git::default(), &Runtime::default(), R, &p);
        assert_eq!((r.state, r.details[0].code), (State::Unverified, 56));
        f.root = "memory".into();
        let r = resolve_file(
            &f,
            &view,
            &fs,
            &Git::default(),
            &Runtime::default(),
            R,
            &Params::default(),
        );
        assert_eq!((r.state, r.details[0].code), (State::Unverified, 60));
        let mut w = node("con.txt", BODY);
        w.root = "project".into();
        let r = resolve_file(
            &w,
            &view,
            &fs,
            &Git::default(),
            &Runtime::default(),
            R,
            &Params::default(),
        );
        assert_eq!((r.state, r.details[0].code), (State::Missing, 44));
    }

    fn settle_p() -> Params {
        Params {
            settle: true,
            writer: true,
            ..Params::default()
        }
    }

    fn view1(f: &FileNode) -> View {
        View {
            files: vec![f.clone()],
            moves: BTreeMap::new(),
            anchor_conflicts: Vec::new(),
        }
    }

    #[test]
    fn the_copy_rule_creation_time_line_is_exact_on_a_tunneling_volume() {
        let mut fs = fs_with(&[("a/x.rs", BODY)]);
        let f = node("a/x.rs", BODY);
        let mut rt = Runtime::default();
        let o = obs_of(&fs, "a/x.rs", 1 << 16);
        let btime = o.creation_ns.unwrap();
        rt.fileobs.insert((1, R.into()), o);
        // The original is replaced by a new file of equal content that kept the creation time (a tunneled rewrite),
        // under a new name in the same directory: no file id leads to it.
        fs.apply(
            R,
            &TreeOp::Rm {
                path: "a/x.rs".into(),
            },
            5_000_000_000,
        )
        .unwrap();
        fs.apply(
            R,
            &TreeOp::Write {
                path: "a/y.rs".into(),
                bytes: BODY.to_vec(),
                btime_ns: Some(btime),
            },
            5_000_000_000,
        )
        .unwrap();
        let r = resolve_file(&f, &view1(&f), &fs, &Git::default(), &rt, R, &settle_p());
        assert_eq!(
            (r.state, r.at.as_deref(), r.evidence),
            (State::MovedAuto, Some("a/y.rs"), Some((5, "lazy")))
        );
        // On a volume whose creation times are copied by clones the same candidate is only a proposal.
        let mut fs2 = fs.clone();
        fs2.trees.get_mut(R).unwrap().caps.btime = Btime::CopiedByClones;
        let r = resolve_file(&f, &view1(&f), &fs2, &Git::default(), &rt, R, &settle_p());
        assert_eq!(r.state, State::MovedNeedsConfirm);
    }

    #[test]
    fn a_recorded_directory_move_is_exact_through_e5() {
        let fs = fs_with(&[("arch/x.md", BODY)]);
        let f = node("docs/x.md", BODY);
        let view = View {
            files: vec![f.clone()],
            moves: BTreeMap::from([(
                "project".to_string(),
                vec![PathMove {
                    hlc: 1,
                    class: MoveClass::Explicit,
                    from: crate::value::PathVal {
                        root: "project".into(),
                        text: "docs/".into(),
                    },
                    to: crate::value::PathVal {
                        root: "project".into(),
                        text: "arch/".into(),
                    },
                    git: None,
                }],
            )]),
            anchor_conflicts: Vec::new(),
        };
        let r = resolve_file(
            &f,
            &view,
            &fs,
            &Git::default(),
            &Runtime::default(),
            R,
            &settle_p(),
        );
        assert_eq!(
            (r.state, r.at.as_deref(), r.evidence),
            (State::MovedAuto, Some("arch/x.md"), Some((6, "lazy")))
        );
    }

    #[test]
    fn replaced_and_path_reuse_and_rename_over_at_a_settle() {
        // Replaced: rm, then an unrelated write at the same path.
        let mut fs = fs_with(&[("n.md", BODY)]);
        let f = node("n.md", BODY);
        let mut rt = Runtime::default();
        let mut o = obs_of(&fs, "n.md", 1 << 16);
        o.last_oid = Some(oid(Algo::Sha1, BODY));
        rt.fileobs.insert((1, R.into()), o);
        rt.fprint.insert(
            oid(Algo::Sha1, BODY),
            crate::r4::text::fingerprint(BODY).unwrap(),
        );
        fs.apply(
            R,
            &TreeOp::Rm {
                path: "n.md".into(),
            },
            2_000,
        )
        .unwrap();
        let other = b"entirely\ndifferent\ncontent\nof\nthis\nfile\nnow\n".to_vec();
        fs.apply(
            R,
            &TreeOp::Write {
                path: "n.md".into(),
                bytes: other,
                btime_ns: None,
            },
            3_000,
        )
        .unwrap();
        let r = resolve_file(&f, &view1(&f), &fs, &Git::default(), &rt, R, &settle_p());
        assert_eq!((r.state, r.details[0].code), (State::Replaced, 34));
        // A read does not run the test.
        assert_eq!(
            resolve_file(
                &f,
                &view1(&f),
                &fs,
                &Git::default(),
                &rt,
                R,
                &Params::default()
            )
            .state,
            State::Ok
        );
        // Path reuse: a directory promote-replace.
        let mut fs = fs_with(&[("storage/log.rs", BODY)]);
        let f = node("storage/log.rs", BODY);
        let mut rt = Runtime::default();
        rt.fileobs
            .insert((1, R.into()), obs_of(&fs, "storage/log.rs", 1 << 16));
        fs.apply(
            R,
            &TreeOp::Mv {
                from: "storage".into(),
                to: "storage_old".into(),
            },
            2_000,
        )
        .unwrap();
        fs.apply(
            R,
            &TreeOp::Write {
                path: "storage/log.rs".into(),
                bytes: b"v2\n".to_vec(),
                btime_ns: None,
            },
            3_000,
        )
        .unwrap();
        let r = resolve_file(&f, &view1(&f), &fs, &Git::default(), &rt, R, &settle_p());
        assert_eq!((r.state, r.details[0].code), (State::Ambiguous, 29));
        assert_eq!(r.details[0].paths, ["storage_old/log.rs"]);
        // Rename-over: `mv b a`.
        let body_b: &[u8] =
            b"fn b1() {}\nfn b2() {}\nfn b3() {}\nfn b4() {}\nfn b5() {}\nfn b6() {}\n";
        let mut fs = fs_with(&[("a.rs", BODY), ("b.rs", body_b)]);
        let fa = node("a.rs", BODY);
        let mut fb = node("b.rs", body_b);
        fb.n = 2;
        let mut rt = Runtime::default();
        rt.fileobs
            .insert((1, R.into()), obs_of(&fs, "a.rs", 1 << 16));
        fs.apply(
            R,
            &TreeOp::Rm {
                path: "a.rs".into(),
            },
            2_000,
        )
        .unwrap();
        fs.apply(
            R,
            &TreeOp::Mv {
                from: "b.rs".into(),
                to: "a.rs".into(),
            },
            2_000,
        )
        .unwrap();
        let view = View {
            files: vec![fa.clone(), fb],
            moves: BTreeMap::new(),
            anchor_conflicts: Vec::new(),
        };
        let r = resolve_file(&fa, &view, &fs, &Git::default(), &rt, R, &settle_p());
        assert_eq!((r.state, r.details[0].code), (State::Ambiguous, 23));
    }

    #[test]
    fn a_strong_candidate_is_a_guess_under_policy_b() {
        let mut fs = fs_with(&[("x.rs", BODY)]);
        let f = node("x.rs", BODY);
        let mut rt = Runtime::default();
        rt.fileobs
            .insert((1, R.into()), obs_of(&fs, "x.rs", 1 << 16));
        fs.apply(
            R,
            &TreeOp::Mv {
                from: "x.rs".into(),
                to: "y.rs".into(),
            },
            2_000,
        )
        .unwrap();
        let mut b = BODY.to_vec();
        b.extend_from_slice(b"fn extra() {}\n");
        fs.apply(
            R,
            &TreeOp::Write {
                path: "y.rs".into(),
                bytes: b,
                btime_ns: None,
            },
            3_000,
        )
        .unwrap();
        let r = resolve_file(&f, &view1(&f), &fs, &Git::default(), &rt, R, &settle_p());
        assert_eq!(
            (r.state, r.details[0].code, r.guess),
            (State::MovedNeedsConfirm, 11, None)
        );
        let pb = Params {
            policy_strong: true,
            ..settle_p()
        };
        let r = resolve_file(&f, &view1(&f), &fs, &Git::default(), &rt, R, &pb);
        assert_eq!(r.guess, Some((14, None)));
        assert_eq!(
            crate::r4::settle::relink_of(&r).as_deref(),
            Some("policy/file-id-edited")
        );
    }

    #[test]
    fn a_tiny_file_is_never_rebound_by_content() {
        let mut fs = fs_with(&[("t/x.txt", b"hi\n")]);
        let f = node("t/x.txt", b"hi\n");
        fs.apply(
            R,
            &TreeOp::Cp {
                from: "t/x.txt".into(),
                to: "t/y.txt".into(),
                keep_btime: true,
            },
            2_000,
        )
        .unwrap();
        fs.apply(
            R,
            &TreeOp::Rm {
                path: "t/x.txt".into(),
            },
            2_000,
        )
        .unwrap();
        let r = resolve_file(
            &f,
            &view1(&f),
            &fs,
            &Git::default(),
            &Runtime::default(),
            R,
            &settle_p(),
        );
        assert_eq!(r.state, State::Ambiguous);
        assert_eq!(r.candidates, ["t/y.txt"]);
    }

    #[test]
    fn a_planned_node_binds_when_the_file_appears_after_the_plan() {
        let fs = fs_with(&[("new.rs", BODY)]);
        let mut f = node("new.rs", BODY);
        f.status = FileStatus::Planned;
        f.obs_hlc = 0;
        let r = resolve_file(
            &f,
            &view1(&f),
            &fs,
            &Git::default(),
            &Runtime::default(),
            R,
            &settle_p(),
        );
        assert_eq!((r.state, r.at.as_deref()), (State::Planned, Some("new.rs")));
        f.obs_hlc = 10_000u64 << 16;
        let r = resolve_file(
            &f,
            &view1(&f),
            &fs,
            &Git::default(),
            &Runtime::default(),
            R,
            &settle_p(),
        );
        assert_eq!((r.state, r.details[0].code), (State::Planned, 52));
    }

    fn lines(tag: &str, n: usize) -> String {
        (0..n)
            .map(|i| format!("{tag} line number {i} of this content\n"))
            .collect()
    }

    fn git_with_blobs(cs: Vec<Commit>, head: &str, blobs: &[(&str, &str)]) -> Git {
        let mut g = repo_with(cs, head);
        let r = g.repos.get_mut("r").unwrap();
        for (id, b) in blobs {
            r.blobs.insert(id.to_string(), b.as_bytes().to_vec());
        }
        g
    }

    fn policy_b() -> Params {
        Params {
            policy_strong: true,
            ..settle_p()
        }
    }

    fn codes(r: &FileResult) -> Vec<u8> {
        r.details.iter().map(|d| d.code).collect()
    }

    #[test]
    fn a_split_is_one_proposal_listing_its_pieces_and_is_never_applied() {
        let old = lines("old", 10);
        let (a, b): (String, String) = (
            old.lines().take(5).map(|l| format!("{l}\n")).collect(),
            old.lines().skip(5).map(|l| format!("{l}\n")).collect(),
        );
        let fs = fs_with(&[("p1.txt", a.as_bytes()), ("p2.txt", b.as_bytes())]);
        let mut f = node("x.txt", old.as_bytes());
        f.observed_git = Some("c1".into());
        let git = git_with_blobs(
            vec![
                commit("c1", &[], &[("x.txt", "X")]),
                commit("c2", &["c1"], &[("p1.txt", "A"), ("p2.txt", "B")]),
            ],
            "c2",
            &[("X", &old), ("A", &a), ("B", &b)],
        );
        for p in [settle_p(), policy_b()] {
            let r = resolve_file(&f, &view1(&f), &fs, &git, &Runtime::default(), R, &p);
            assert_eq!((r.state, codes(&r)), (State::MovedNeedsConfirm, vec![17]));
            assert_eq!(r.candidates, ["p1.txt", "p2.txt"], "the pieces are listed");
            assert_eq!(r.proposals.len(), 1);
            assert_eq!(r.proposals[0].pieces, ["p1.txt", "p2.txt"]);
            assert_eq!((r.guess, r.proposals[0].auto), (None, false));
        }
    }

    #[test]
    fn a_merged_file_is_a_proposal_into_its_host_and_never_a_guess() {
        let old = lines("old", 8);
        let host = lines("host", 20);
        let merged = format!("{host}{old}");
        let fs = fs_with(&[("host.txt", merged.as_bytes())]);
        let mut f = node("x.txt", old.as_bytes());
        f.observed_git = Some("c1".into());
        let git = git_with_blobs(
            vec![
                commit("c1", &[], &[("x.txt", "X"), ("host.txt", "H0")]),
                commit("c2", &["c1"], &[("host.txt", "H1")]),
            ],
            "c2",
            &[("X", &old), ("H0", &host), ("H1", &merged)],
        );
        let r = resolve_file(
            &f,
            &view1(&f),
            &fs,
            &git,
            &Runtime::default(),
            R,
            &policy_b(),
        );
        assert_eq!(
            (r.state, codes(&r), r.guess),
            (State::MovedNeedsConfirm, vec![18], None)
        );
        assert_eq!(r.details[0].scores, vec![Ratio::int(1)]);
        let out = crate::r4::settle::settle(
            &[1],
            &view1(&f),
            &fs,
            &git,
            &Runtime::default(),
            R,
            &policy_b(),
            5 << 16,
        );
        assert!(out.rebinds.is_empty());
    }

    #[test]
    fn a_directory_moved_with_its_file_replaced_renders_21_and_is_never_a_guess() {
        let old = lines("old", 10);
        let mut fs = fs_with(&[("docs/x.md", old.as_bytes())]);
        let f = node("docs/x.md", old.as_bytes());
        let mut rt = Runtime::default();
        let mut o = obs_of(&fs, "docs/x.md", 1 << 16);
        o.last_oid = f.oid.clone();
        rt.fileobs.insert((1, R.into()), o);
        rt.fprint
            .insert(f.oid.clone().unwrap(), fingerprint(old.as_bytes()).unwrap());
        fs.apply(
            R,
            &TreeOp::Mv {
                from: "docs".into(),
                to: "arch".into(),
            },
            2_000,
        )
        .unwrap();
        fs.apply(
            R,
            &TreeOp::Rm {
                path: "arch/x.md".into(),
            },
            2_000,
        )
        .unwrap();
        let new = old.replacen("old line number 0", "a new first line", 1);
        fs.apply(
            R,
            &TreeOp::Write {
                path: "arch/x.md".into(),
                bytes: new.into_bytes(),
                btime_ns: None,
            },
            2_000,
        )
        .unwrap();
        let r = resolve_file(&f, &view1(&f), &fs, &Git::default(), &rt, R, &policy_b());
        assert_eq!(
            (r.state, codes(&r), r.guess),
            (State::MovedNeedsConfirm, vec![21], None)
        );
        assert_eq!(
            (r.proposals[0].evidence, r.proposals[0].auto),
            (15, false),
            "class prefix-strong, never applied ([F18 §5.4])"
        );
    }

    #[test]
    fn a_hard_linked_file_located_by_id_is_at_most_strong() {
        let mut fs = fs_with(&[("a/x.rs", BODY)]);
        let f = node("a/x.rs", BODY);
        let mut rt = Runtime::default();
        rt.fileobs
            .insert((1, R.into()), obs_of(&fs, "a/x.rs", 1 << 16));
        fs.hard_link(R, "a/x.rs", "b/other.rs");
        fs.apply(
            R,
            &TreeOp::Mv {
                from: "a/x.rs".into(),
                to: "b/x.rs".into(),
            },
            2_000,
        )
        .unwrap();
        // E3 locates the id at its first name in path order; with two names it yields `strong` only.
        let r = resolve_file(&f, &view1(&f), &fs, &Git::default(), &rt, R, &settle_p());
        assert_eq!(r.state, State::MovedNeedsConfirm, "{r:?}");
        assert_eq!(r.proposals[0].class, PClass::Strong);
        // The same under E3d: the parent directory's id finds e/x.rs, a name of a hard-linked file.
        let mut fs2 = fs_with(&[("d/x.rs", BODY)]);
        let mut rt2 = Runtime::default();
        let mut o = obs_of(&fs2, "d/x.rs", 1 << 16);
        // The file's own id is stale, so only the directory's id finds it.
        o.file_id = Some(FileId {
            kind: 1,
            volume: "C".into(),
            id: 999,
        });
        rt2.fileobs.insert((1, R.into()), o);
        let f2 = node("d/x.rs", BODY);
        fs2.hard_link(R, "d/x.rs", "far/away/other.rs");
        fs2.apply(
            R,
            &TreeOp::Mv {
                from: "d".into(),
                to: "e".into(),
            },
            2_000,
        )
        .unwrap();
        let r = resolve_file(
            &f2,
            &view1(&f2),
            &fs2,
            &Git::default(),
            &rt2,
            R,
            &settle_p(),
        );
        assert_eq!(
            (r.state, codes(&r)),
            (State::MovedNeedsConfirm, vec![12]),
            "{r:?}"
        );
    }

    #[test]
    fn tiny_files_are_ambiguous_only_by_e4_e5_and_e7_candidates() {
        let tiny = b"hi\n";
        // E5: a recorded directory move leads to an equal-content file: ambiguous, never exact, for a tiny F.
        let fs = fs_with(&[("arch/t.txt", tiny)]);
        let f = node("docs/t.txt", tiny);
        let view = View {
            files: vec![f.clone()],
            moves: BTreeMap::from([(
                "project".to_string(),
                vec![PathMove {
                    hlc: 1,
                    class: MoveClass::Explicit,
                    from: crate::value::PathVal {
                        root: "project".into(),
                        text: "docs/".into(),
                    },
                    to: crate::value::PathVal {
                        root: "project".into(),
                        text: "arch/".into(),
                    },
                    git: None,
                }],
            )]),
            anchor_conflicts: Vec::new(),
        };
        let r = resolve_file(
            &f,
            &view,
            &fs,
            &Git::default(),
            &Runtime::default(),
            R,
            &settle_p(),
        );
        assert_eq!(
            (r.state, r.candidates.clone()),
            (State::Ambiguous, vec!["arch/t.txt".to_string()])
        );
        // E3's strong proposal for a tiny F that moved and changed is a proposal, not an ambiguity.
        let mut fs = fs_with(&[("t.txt", tiny)]);
        let mut rt = Runtime::default();
        rt.fileobs
            .insert((1, R.into()), obs_of(&fs, "t.txt", 1 << 16));
        fs.apply(
            R,
            &TreeOp::Mv {
                from: "t.txt".into(),
                to: "u.txt".into(),
            },
            2_000,
        )
        .unwrap();
        fs.apply(
            R,
            &TreeOp::Write {
                path: "u.txt".into(),
                bytes: b"hello\n".to_vec(),
                btime_ns: None,
            },
            3_000,
        )
        .unwrap();
        let t = node("t.txt", tiny);
        let r = resolve_file(&t, &view1(&t), &fs, &Git::default(), &rt, R, &settle_p());
        assert_eq!((r.state, codes(&r)), (State::MovedNeedsConfirm, vec![11]));
    }

    #[test]
    fn an_unreadable_candidate_makes_the_link_unverified_not_missing() {
        let mut fs = fs_with(&[("a/x.rs", BODY)]);
        let f = node("a/x.rs", BODY);
        fs.apply(
            R,
            &TreeOp::Cp {
                from: "a/x.rs".into(),
                to: "a/y.rs".into(),
                keep_btime: false,
            },
            2_000,
        )
        .unwrap();
        fs.apply(
            R,
            &TreeOp::Rm {
                path: "a/x.rs".into(),
            },
            2_000,
        )
        .unwrap();
        fs.apply(
            R,
            &TreeOp::Deny {
                path: "a/y.rs".into(),
                on: true,
            },
            2_000,
        )
        .unwrap();
        let r = resolve_file(
            &f,
            &view1(&f),
            &fs,
            &Git::default(),
            &Runtime::default(),
            R,
            &settle_p(),
        );
        assert_eq!((r.state, codes(&r)), (State::Unverified, vec![59]));
        // A file whose size proves other content need not be read: it leaves `missing`.
        let mut rt = Runtime::default();
        rt.fprint
            .insert(f.oid.clone().unwrap(), fingerprint(BODY).unwrap());
        let mut fs2 = fs_with(&[("a/x.rs", BODY), ("a/big.rs", &[b'z'; 4000])]);
        fs2.apply(
            R,
            &TreeOp::Rm {
                path: "a/x.rs".into(),
            },
            2_000,
        )
        .unwrap();
        fs2.apply(
            R,
            &TreeOp::Deny {
                path: "a/big.rs".into(),
                on: true,
            },
            2_000,
        )
        .unwrap();
        let r = resolve_file(&f, &view1(&f), &fs2, &Git::default(), &rt, R, &settle_p());
        assert_eq!((r.state, codes(&r)), (State::Missing, vec![37, 46]));
        // A file E3 locates by id but cannot stat: `unverified (unreadable)`.
        let mut fs3 = fs_with(&[("a/x.rs", BODY)]);
        let mut rt3 = Runtime::default();
        rt3.fileobs
            .insert((1, R.into()), obs_of(&fs3, "a/x.rs", 1 << 16));
        fs3.apply(
            R,
            &TreeOp::Mv {
                from: "a/x.rs".into(),
                to: "b/x.rs".into(),
            },
            2_000,
        )
        .unwrap();
        fs3.apply(
            R,
            &TreeOp::Deny {
                path: "b/x.rs".into(),
                on: true,
            },
            2_000,
        )
        .unwrap();
        let r = resolve_file(&f, &view1(&f), &fs3, &Git::default(), &rt3, R, &settle_p());
        assert_eq!((r.state, codes(&r)), (State::Unverified, vec![59]));
    }

    #[test]
    fn an_unchanged_recorded_ok_keeps_its_details_and_the_spelling() {
        let fs = fs_with(&[("Docs/Plan.md", BODY)]);
        let f = node("docs/plan.md", BODY);
        let mut rt = Runtime::default();
        let mut o = obs_of(&fs, "Docs/Plan.md", 1 << 16);
        o.recorded = Some((State::Ok, vec![Detail::code(1)]));
        rt.fileobs.insert((1, R.into()), o.clone());
        let r = resolve_file(
            &f,
            &view1(&f),
            &fs,
            &Git::default(),
            &rt,
            R,
            &Params::default(),
        );
        assert_eq!((r.state, codes(&r)), (State::Ok, vec![1, 2]));
        assert_eq!(r.details[1].paths, ["Docs/Plan.md"]);
        // A recorded non-`ok` state shows as recorded: [F18 §4.7] admits the spelling part in `ok` only.
        o.recorded = Some((State::Ambiguous, vec![Detail::path(29, "old/plan.md")]));
        rt.fileobs.insert((1, R.into()), o);
        let r = resolve_file(
            &f,
            &view1(&f),
            &fs,
            &Git::default(),
            &rt,
            R,
            &Params::default(),
        );
        assert_eq!((r.state, codes(&r)), (State::Ambiguous, vec![29]));
    }

    #[test]
    fn cloud_conflict_copies_try_both_extension_readings_beside_live_nodes() {
        let mut fs = fs_with(&[("docs/x.md", BODY), ("docs/report.md", b"report\n")]);
        fs.trees.get_mut(R).unwrap().cloud_root = true;
        let f = node("docs/x.md", BODY);
        let mut g = node("docs/report.md", b"report\n");
        g.n = 2;
        // A copy of F's content named as a conflict copy of `report.md` under the empty-extension reading.
        fs.apply(
            R,
            &TreeOp::Cp {
                from: "docs/x.md".into(),
                to: "docs/report.md-Laptop".into(),
                keep_btime: false,
            },
            2_000,
        )
        .unwrap();
        fs.apply(
            R,
            &TreeOp::Rm {
                path: "docs/x.md".into(),
            },
            2_000,
        )
        .unwrap();
        let view = |g: &FileNode| View {
            files: vec![f.clone(), g.clone()],
            moves: BTreeMap::new(),
            anchor_conflicts: Vec::new(),
        };
        let run = |g: &FileNode| {
            resolve_file(
                &f,
                &view(g),
                &fs,
                &Git::default(),
                &Runtime::default(),
                R,
                &settle_p(),
            )
        };
        assert_eq!(run(&g).state, State::Missing, "a never-candidate");
        // A tombstoned node is not live: the name is an ordinary candidate then.
        let mut dead = g.clone();
        dead.tombstone = true;
        let r = run(&dead);
        assert_eq!((r.state, codes(&r)), (State::MovedNeedsConfirm, vec![10]));
    }

    /// A tree whose last settle was at `hlc` 1 ms, with F's copy made before it and the copy's directory renamed after
    /// it: only the rename says the copy may be new ([F20 §5.12]).
    fn e7_case(os: Os, caps: VolumeCaps) -> (Fs, FileNode, Runtime) {
        let mut fs = Fs::default();
        fs.ensure_tree(R, "C", caps, os);
        for (p, b) in [("a/x.rs", BODY), ("b/keep.txt", &b"k\n"[..])] {
            fs.apply(
                R,
                &TreeOp::Write {
                    path: p.to_string(),
                    bytes: b.to_vec(),
                    btime_ns: None,
                },
                1_000,
            )
            .unwrap();
        }
        fs.apply(
            R,
            &TreeOp::Cp {
                from: "a/x.rs".into(),
                to: "b/sub/y.rs".into(),
                keep_btime: true,
            },
            1_000,
        )
        .unwrap();
        let f = node("a/x.rs", BODY);
        let mut rt = Runtime::default();
        let mut o = obs_of(&fs, "a/x.rs", 1 << 16);
        o.file_id = None;
        o.parent_dir = None;
        rt.fileobs.insert((1, R.into()), o);
        rt.trees.insert(
            R.into(),
            TreeRow {
                first_settle_done: true,
                last_settle_hlc: 1 << 16,
                epochs: Vec::new(),
            },
        );
        fs.apply(
            R,
            &TreeOp::Rm {
                path: "a/x.rs".into(),
            },
            5_000_000,
        )
        .unwrap();
        fs.apply(
            R,
            &TreeOp::Mv {
                from: "b".into(),
                to: "c".into(),
            },
            5_000_000,
        )
        .unwrap();
        (fs, f, rt)
    }

    #[test]
    fn e7_takes_the_time_predicate_of_each_os() {
        // Windows: the renamed directory's ChangeTime is after the last settle, so the file under it is a candidate.
        let (fs, f, rt) = e7_case(Os::Windows, VolumeCaps::NTFS);
        let r = resolve_file(&f, &view1(&f), &fs, &Git::default(), &rt, R, &settle_p());
        assert_eq!((r.state, codes(&r)), (State::MovedNeedsConfirm, vec![10]));
        assert_eq!(r.proposals[0].path, "c/sub/y.rs");
        // Linux without `DIRMAP` rows: every directory counts as changed, so the frontier holds c/sub.
        let (fs, f, mut rt) = e7_case(Os::Linux, VolumeCaps::EXT4);
        let r = resolve_file(&f, &view1(&f), &fs, &Git::default(), &rt, R, &settle_p());
        assert_eq!(r.state, State::MovedNeedsConfirm);
        // Linux with rows recorded before the rename: c/sub keeps its id and mtime, so it is not in the frontier, and
        // its file's ctime is old: not a candidate.
        for (p, d) in &fs.trees[R].dirs {
            let m = if p.is_empty() || p == "c" {
                1_000
            } else {
                d.mtime_ns
            };
            rt.dirmap.insert((R.into(), d.id), (p.clone(), m));
        }
        let p = Params {
            stamp_ns: Some(3_000_000_000),
            ..settle_p()
        };
        let r = resolve_file(&f, &view1(&f), &fs, &Git::default(), &rt, R, &p);
        assert_eq!((r.state, codes(&r)), (State::Missing, vec![37, 46]));
        // macOS: a rename into another directory sets the file's ADDEDTIME.
        let (mut fs, f, mut rt) = e7_case(Os::Macos, VolumeCaps::APFS);
        fs.apply(
            R,
            &TreeOp::Mv {
                from: "c/sub/y.rs".into(),
                to: "c/y.rs".into(),
            },
            6_000_000,
        )
        .unwrap();
        assert_eq!(fs.trees[R].files["c/y.rs"].added_ns, 6_000_000);
        for (p, d) in &fs.trees[R].dirs {
            rt.dirmap.insert((R.into(), d.id), (p.clone(), d.mtime_ns));
        }
        let r = resolve_file(&f, &view1(&f), &fs, &Git::default(), &rt, R, &p);
        assert_eq!((r.state, codes(&r)), (State::MovedNeedsConfirm, vec![10]));
    }

    #[test]
    fn pending_rows_yield_by_their_class_and_token() {
        let fs = fs_with(&[("b.rs", BODY)]);
        let f = node("a.rs", BODY);
        let with = |class, source, evidence| Runtime {
            pending: vec![PendingRow {
                n: 1,
                tree: R.into(),
                class,
                source,
                evidence,
                oid: f.oid.clone(),
                from: "a.rs".into(),
                to: "b.rs".into(),
                hlc: 1,
            }],
            ..Runtime::default()
        };
        let run =
            |rt: &Runtime, p: &Params| resolve_file(&f, &view1(&f), &fs, &Git::default(), rt, R, p);
        // A reader settle's identical copy stays a copy proposal; policy B never applies it.
        let r = run(
            &with(PClass::Copy, PendingSource::ReaderSettle, 13),
            &policy_b(),
        );
        assert_eq!(
            (r.state, codes(&r), r.guess),
            (State::MovedNeedsConfirm, vec![10], None)
        );
        // A reader settle's strong proposal is never applied either: it is another observation's proposal.
        let r = run(
            &with(PClass::Strong, PendingSource::ReaderSettle, 14),
            &policy_b(),
        );
        assert_eq!(
            (r.state, codes(&r), r.guess),
            (State::MovedNeedsConfirm, vec![11], None)
        );
        // A hook's argument parse is strong, and policy B applies it as `policy/argv`.
        let r = run(&with(PClass::Strong, PendingSource::Hook, 22), &policy_b());
        assert_eq!((r.state, codes(&r)), (State::MovedNeedsConfirm, vec![19]));
        assert_eq!(
            crate::r4::settle::relink_of(&r).as_deref(),
            Some("policy/argv")
        );
        // An exact row: this tree's hook row keeps its token, any other exact row is `lazy/pending`.
        let r = run(&with(PClass::Exact, PendingSource::Hook, 3), &settle_p());
        assert_eq!(r.evidence, Some((3, "hook")));
        let r = run(
            &with(PClass::Exact, PendingSource::ReaderSettle, 3),
            &settle_p(),
        );
        assert_eq!(r.evidence, Some((7, "lazy")));
    }

    #[test]
    fn g4_alias_chains_and_freshness_after_a_step() {
        let fs = fs_with(&[("sub/y.rs", BODY), ("sub/z.rs", BODY)]);
        // Two aliases whose chains reach two targets: `ambiguous`, as selection step 3 counts targets.
        let mut f = node("elsewhere.rs", BODY);
        f.aliases = vec!["x1.rs".into(), "x2.rs".into()];
        f.observed_git = Some("lane".into());
        let git = repo_with(
            vec![
                commit("c1", &[], &[("x1.rs", "B1"), ("x2.rs", "B2")]),
                commit("lane", &["c1"], &[("elsewhere.rs", "B1")]),
                commit("c2", &["c1"], &[("sub/y.rs", "B1"), ("sub/z.rs", "B2")]),
            ],
            "c2",
        );
        let r = resolve_file(
            &f,
            &view1(&f),
            &fs,
            &git,
            &Runtime::default(),
            R,
            &settle_p(),
        );
        assert_eq!(
            (r.state, r.candidates.clone()),
            (
                State::Ambiguous,
                vec!["sub/y.rs".to_string(), "sub/z.rs".to_string()]
            )
        );
        // A chain from p that took a step and then ended deleted starts at p: the tree is fresh for F.
        let mut g = node("x1.rs", BODY);
        g.observed_git = Some("lane".into());
        let git = git_with_blobs(
            vec![
                commit("c1", &[], &[("x1.rs", "B1")]),
                commit("lane", &["c1"], &[("x1.rs", "B1"), ("n.rs", "N")]),
                commit("c2", &["c1"], &[("mid.rs", "B1")]),
                commit("c3", &["c2"], &[("other.rs", "O")]),
            ],
            "c3",
            &[
                ("B1", std::str::from_utf8(BODY).unwrap()),
                ("O", "unrelated\n"),
            ],
        );
        let r = resolve_file(
            &g,
            &view1(&g),
            &fs_with(&[]),
            &git,
            &Runtime::default(),
            R,
            &settle_p(),
        );
        assert!(r.fresh, "{r:?}");
        assert_eq!((r.state, r.details[0].code), (State::Missing, 43));
    }

    /// [F20 §5.18]: without creation times (`VolumeCaps.btime = absent`, FAT) a `planned` node binds only when the
    /// tree's HEAD descends from the planning commit.
    #[test]
    fn a_planned_node_on_a_volume_without_creation_times_binds_by_descent_only() {
        let mut fs = Fs::default();
        fs.ensure_tree(R, "F", VolumeCaps::FAT, Os::Windows);
        fs.apply(
            R,
            &TreeOp::Write {
                path: "new.rs".into(),
                bytes: BODY.to_vec(),
                btime_ns: None,
            },
            5_000_000_000,
        )
        .unwrap();
        let mut f = node("new.rs", BODY);
        f.status = FileStatus::Planned;
        // The file appeared long after the planning commit's hlc …
        f.obs_hlc = 0;
        f.observed_git = Some("plan".into());
        // … but HEAD was made on another line than the planning commit.
        let stale = repo_with(
            vec![
                commit("c1", &[], &[]),
                commit("plan", &["c1"], &[]),
                commit("c2", &["c1"], &[]),
            ],
            "c2",
        );
        let run = |fs: &Fs, git: &Git| {
            resolve_file(&f, &view1(&f), fs, git, &Runtime::default(), R, &settle_p())
        };
        let r = run(&fs, &stale);
        assert_eq!(
            (r.state, codes(&r), r.at.clone()),
            (State::Planned, vec![52], None)
        );
        let out = crate::r4::settle::settle(
            &[1],
            &view1(&f),
            &fs,
            &stale,
            &Runtime::default(),
            R,
            &settle_p(),
            9 << 16,
        );
        assert!(out.binds.is_empty(), "{:?}", out.binds);
        // On NTFS the creation time binds it.
        let mut ntfs = fs.clone();
        ntfs.trees.get_mut(R).unwrap().caps = VolumeCaps::NTFS;
        assert_eq!(run(&ntfs, &stale).at.as_deref(), Some("new.rs"));
        // On FAT a HEAD that descends from the planning commit binds it.
        let line = repo_with(
            vec![
                commit("c1", &[], &[]),
                commit("plan", &["c1"], &[]),
                commit("c3", &["plan"], &[]),
            ],
            "c3",
        );
        assert_eq!(run(&fs, &line).at.as_deref(), Some("new.rs"));
    }

    /// A node observed without git (empty `observed_git`) takes the full cascade in a git tree, as the freshness rule
    /// already treats it: a move by file id is exact, never `absent-in-tree (diverged)`.
    #[test]
    fn an_observation_made_without_git_takes_the_full_cascade_in_a_git_tree() {
        let mut fs = fs_with(&[("a/x.rs", BODY)]);
        let f = node("a/x.rs", BODY);
        assert_eq!(f.observed_git, None);
        let mut rt = Runtime::default();
        rt.fileobs
            .insert((1, R.into()), obs_of(&fs, "a/x.rs", 1 << 16));
        fs.apply(
            R,
            &TreeOp::Mv {
                from: "a/x.rs".into(),
                to: "b/x.rs".into(),
            },
            2_000,
        )
        .unwrap();
        let git = repo_with(vec![commit("c1", &[], &[("README", "r")])], "c1");
        let r = resolve_file(&f, &view1(&f), &fs, &git, &rt, R, &settle_p());
        assert_eq!(
            (r.state, r.at.as_deref(), r.evidence),
            (State::MovedAuto, Some("b/x.rs"), Some((3, "lazy")))
        );
        assert!(r.fresh);
    }

    /// `files.max-read-bytes` ([F20 §2.4] item 1): a candidate whose content is beyond it is `Unavailable(size)`, so
    /// the link is `unverified (size)`, never `missing`.
    #[test]
    fn content_beyond_max_read_bytes_is_unverified_size_not_missing() {
        let mut fs = fs_with(&[("a/x.rs", BODY)]);
        let f = node("a/x.rs", BODY);
        fs.apply(
            R,
            &TreeOp::Cp {
                from: "a/x.rs".into(),
                to: "a/y.rs".into(),
                keep_btime: false,
            },
            2_000,
        )
        .unwrap();
        fs.apply(
            R,
            &TreeOp::Rm {
                path: "a/x.rs".into(),
            },
            2_000,
        )
        .unwrap();
        let run = |p: &Params| {
            resolve_file(
                &f,
                &view1(&f),
                &fs,
                &Git::default(),
                &Runtime::default(),
                R,
                p,
            )
        };
        let r = run(&settle_p());
        assert_eq!((r.state, codes(&r)), (State::MovedNeedsConfirm, vec![10]));
        let small = Params {
            max_read_bytes: Some(16),
            ..settle_p()
        };
        let r = run(&small);
        assert_eq!((r.state, codes(&r)), (State::Unverified, vec![58]));
        // A settle under the limit records no `oid` it could not read.
        let fs2 = fs_with(&[("a/x.rs", BODY)]);
        let out = crate::r4::settle::settle(
            &[1],
            &view1(&f),
            &fs2,
            &Git::default(),
            &Runtime::default(),
            R,
            &small,
            5 << 16,
        );
        assert_eq!(out.fileobs[0].1.last_oid, None);
        assert!(out.fprint.is_empty());
    }

    /// The twin rule's content match is [F20 §2.3]'s three-valued test under A(R): a recorded value of another
    /// algorithm is unknown, never a match; and only the paths of live `present` or `planned` nodes count as node
    /// paths.
    #[test]
    fn twins_compare_recorded_values_under_the_roots_algorithm() {
        let fs = fs_with(&[("Docs/Plan.md", BODY)]);
        let blob = crate::value::hex(&oid(Algo::Sha1, BODY).digest);
        let git = repo_with(
            vec![commit(
                "c1",
                &[],
                &[("Docs/Plan.md", blob.as_str()), ("docs/plan.md", "ffff")],
            )],
            "c1",
        );
        let mut f = node("docs/plan.md", BODY);
        f.observed_git = Some("c1".into());
        let run =
            |p: &Params, view: &View| resolve_file(&f, view, &fs, &git, &Runtime::default(), R, p);
        // A(R) = sha1: Docs/Plan.md's blob matches the content, so F's spelling is the other one.
        let r = run(&Params::default(), &view1(&f));
        assert_eq!((r.state, codes(&r)), (State::Missing, vec![44, 46]));
        // A(R) = sha256: no sha1 value can match, so nothing tells the spellings apart.
        let p256 = Params {
            algo: Algo::Sha256,
            ..Params::default()
        };
        let r = run(&p256, &view1(&f));
        assert_eq!((r.state, codes(&r)), (State::Ambiguous, vec![26]));
        // F outside the view, and only a removed node at a twin spelling: no node path is in the class, so F resolves
        // as a plain present path.
        let mut gone = node("Docs/Plan.md", BODY);
        gone.n = 2;
        gone.status = FileStatus::Removed;
        let view = View {
            files: vec![gone],
            moves: BTreeMap::new(),
            anchor_conflicts: Vec::new(),
        };
        let r = run(&Params::default(), &view);
        assert_eq!(r.state, State::Ok, "{r:?}");
    }

    /// E3d needs `FILEOBS.parent_dir_id` only ([F20 §5.7]): without a recorded file id, size and mtime still decide.
    #[test]
    fn e3d_needs_only_the_parent_directory_id() {
        let mut fs = fs_with(&[("docs/x.md", BODY), ("docs/y.md", b"other\n")]);
        let f = node("docs/x.md", BODY);
        let mut rt = Runtime::default();
        let mut o = obs_of(&fs, "docs/x.md", 5 << 16);
        o.file_id = None;
        rt.fileobs.insert((1, R.into()), o);
        fs.apply(
            R,
            &TreeOp::Mv {
                from: "docs".into(),
                to: "archive".into(),
            },
            2_000,
        )
        .unwrap();
        let r = resolve_file(&f, &view1(&f), &fs, &Git::default(), &rt, R, &settle_p());
        assert_eq!(
            (r.state, r.at.as_deref(), r.evidence),
            (State::MovedAuto, Some("archive/x.md"), Some((4, "lazy")))
        );
    }

    /// An identical-blob group of E6 lists its candidates present in T that pass §4, and stays `ambiguous` whatever
    /// their number; with none, E6 yields nothing.
    #[test]
    fn an_identical_blob_group_lists_only_present_eligible_candidates() {
        let mut f = node("x.rs", BODY);
        f.observed_git = Some("c1".into());
        let git = repo_with(
            vec![
                commit("c1", &[], &[("x.rs", "B")]),
                commit("c2", &["c1"], &[("y1.rs", "B"), ("y2.rs", "B")]),
            ],
            "c2",
        );
        let run = |fs: &Fs| {
            resolve_file(
                &f,
                &view1(&f),
                fs,
                &git,
                &Runtime::default(),
                R,
                &settle_p(),
            )
        };
        let r = run(&fs_with(&[]));
        assert_eq!((r.state, codes(&r)), (State::Missing, vec![37, 46]));
        let r = run(&fs_with(&[("y1.rs", BODY)]));
        assert_eq!(
            (r.state, r.candidates.clone()),
            (State::Ambiguous, vec!["y1.rs".to_string()])
        );
    }
}
