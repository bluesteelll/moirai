//! The file-link commands of group F on the store ([API §12]; [40 §3]; [60 §4.2] row "File links") and the environment
//! commands that give them their input, `EnvTree` and `EnvGit` ([API §6.5], §6.6): link intent as data over the
//! simulated trees and the abstract git histories, with exact-evidence resolution by [`crate::r4`].
//!
//! - this module: the store's R4 environment and runtime rows ([`Files`]), `EnvTree`, `EnvGit`, the policy functions
//!   of the `files.*` and `roots.<name>` keys ([RULES/policy-keys] KF-033 to KF-044), the command's tree (CX-5) with
//!   its eligibility and the designation relation ([F18 §3.4]), the reading view as the resolver reads it, the commit
//!   header's git group ([API §4.4]) and the landing of a file command's commit;
//! - [`verbs`]: `FileAdd`, `LinkFile`, `UnlinkFile`, `FileRelink`, `Check`;
//! - [`intent`]: the intent protocol of `FileMv`, `FileRm` and `FileRevert` ([API §12.4]; [40 §3.4]–§3.6);
//! - [`fix`]: `LinksFix` ([API §12.5]; [40 §3.7]);
//! - [`sync`]: `LinksSync` and `Complete`'s link settle ([API §12.6], §10.5 step 4; [40 §4.2]).
//!
//! The runtime rows (`FILEOBS`, `PENDING`, `TREES` epochs, `FPRINT`, `PREFIXEV`, `DIRMAP`, `FSINTENT`) are never in
//! `state(ref)` (I-F4); of them only `FSINTENT` is in the runtime snapshot ([API §15.7] `intents`).

pub mod fix;
pub mod intent;
pub mod sync;
pub mod verbs;

use crate::api::{Caller, Ctx, Data, Outcome, Reply, Store};
use crate::err::{Refusal, Res};
use crate::idem::{Cj, Recorded};
use crate::r4::cascade::{AnchorConflict, FileNode, Intent, Params, Runtime, Side, View};
use crate::r4::git::{Commit as GitCommit, Git, Head, Repo};
use crate::r4::path::Os;
use crate::r4::settle::{Binding, Designated, designation, writer_tree};
use crate::r4::tree::{Fs, OpError, TreeOp, VolumeCaps};
use crate::r4::uid::FileStatus;
use crate::registry::{Conf, Proc};
use crate::state::{Aspect, Changeset, Creator, KState, KVal, Key, Node, State, diff};
use crate::value::{Algo, MoveClass, Nid, Oid, PathMove, PathVal, Uid, Value, hex};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

// ---------------------------------------------------------------------------------------------------------------
// The store's R4 environment and runtime rows
// ---------------------------------------------------------------------------------------------------------------

/// `op` of an intent ([F11 §12.7]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntentOp {
    /// 1 `mv`.
    Mv,
    /// 2 `rm`.
    Rm,
    /// 3 `rm-trash`.
    RmTrash,
}

impl IntentOp {
    /// The name the runtime snapshot shows ([API §15.7] `intents`).
    pub fn name(self) -> &'static str {
        match self {
            IntentOp::Mv => "mv",
            IntentOp::Rm => "rm",
            IntentOp::RmTrash => "rm-trash",
        }
    }
}

/// `state` of an intent ([F11 §12.7]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntentState {
    /// 1 `open`.
    Open,
    /// 2 `done` (`FsIntentDone`).
    Done,
    /// 3 `aborted` (`FsIntentAborted`) with its reason ([F11 §12.7]: 1 not renamed, 2 cross-volume, 3 every item
    /// failed before a rename, 4 ambiguous, 5 missing).
    Aborted(u8),
}

impl IntentState {
    /// The name the runtime snapshot shows.
    pub fn name(self) -> &'static str {
        match self {
            IntentState::Open => "open",
            IntentState::Done => "done",
            IntentState::Aborted(_) => "aborted",
        }
    }
}

/// One item of an intent ([F11 §12.7] `IntentItem`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IntentItem {
    /// `itflags` bit 0: the item is a directory (no `oid`).
    pub dir: bool,
    /// `outcome`: 0 while open and in an aborted intent, else 1 done, 2 busy, 3 destination exists, 4 source missing,
    /// 5 other.
    pub outcome: u8,
    /// `src`.
    pub src: PathVal,
    /// `dst` (`op` = `mv`).
    pub dst: Option<PathVal>,
    /// `oid` at plan time, of a file item.
    pub oid: Option<Oid>,
}

/// An `FSINTENT` row ([F11 §12.7]), with the commit whose group carried its `FsIntentDone`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IntentRow {
    /// The intent id `i-<n>`: the model's number for the lsn of the `FsIntent` record ([API §16.3] excludes it from
    /// the comparison; intents compare in the order they were opened).
    pub id: u64,
    /// `op`.
    pub op: IntentOp,
    /// `flags` bit 0 `git`.
    pub git: bool,
    /// `flags` bit 1 `recursive`.
    pub recursive: bool,
    /// `flags` bit 2 `recovered`.
    pub recovered: bool,
    /// The caller's branch.
    pub branch: String,
    /// The writer tree's root.
    pub tree: String,
    /// `hlc` of the `FsIntent` record.
    pub hlc: u64,
    /// `closed_hlc`: the HLC of the closing record; 0 while open.
    pub closed_hlc: u64,
    /// `state`.
    pub state: IntentState,
    /// The items, in command-line order.
    pub items: Vec<IntentItem>,
    /// The commit whose group carried the `FsIntentDone` (none for an aborted intent or a done one that committed
    /// nothing).
    pub commit: Option<u64>,
}

/// R4's environment and runtime rows in the store: the simulated trees and git histories the environment commands set,
/// the runtime rows the file commands and settles write, the intents, and the store-wide `aN` of every anchor uid.
#[derive(Clone, Debug, Default)]
pub struct Files {
    /// The simulated project trees ([API §6.5]).
    pub fs: Fs,
    /// The abstract git histories ([API §6.6]).
    pub git: Git,
    /// `FILEOBS`, `PENDING`, `TREES`, `FPRINT`, `PREFIXEV` and `DIRMAP` ([40 §2.6]); its `intents` are derived from
    /// [`Files::intents`] for each resolution.
    pub rt: Runtime,
    /// `FSINTENT`, in the order the intents were opened.
    pub intents: Vec<IntentRow>,
    /// The next intent number.
    pub next_intent: u64,
    /// `aN` of every anchor uid the store knows: a capture, merge, sync or import that lands a known uid reuses its
    /// number ([40 §2.7]; [F18 §2.2]).
    pub anchors: BTreeMap<Uid, u64>,
    /// The `GITFACTS` ancestry facts `Check` appended: (repository, commit, tip) → ancestor ([F05 §9.25]).
    pub facts: BTreeMap<(String, String, String), bool>,
    /// The result data of every intent command (`FileMv`, `FileRm`, `FileRevert`) by its intent id, which a replay
    /// rebuilds from the intent's records ([API §7.5]).
    pub results: BTreeMap<u64, Data>,
    /// `SKEW` in ns ([F20 §5.1], HOLE F20-clock-skew): a parameter of the model at M0.
    pub skew_ns: i128,
}

impl Files {
    /// The simulated tree a canonical absolute path lies in: the tree whose root is its longest prefix ([API §6.5]).
    pub fn tree_of(&self, path: &str) -> Option<String> {
        self.fs
            .trees
            .keys()
            .filter(|r| {
                path == r.as_str() || path.starts_with(&format!("{}/", r.trim_end_matches('/')))
            })
            .max_by_key(|r| r.len())
            .cloned()
    }

    /// The intents as E1 reads them ([F11 §12.7]; [F20 §5.6]): the items of every open intent and of every intent that
    /// intent recovery closed, by their writer tree.
    pub fn e1_intents(&self) -> Vec<Intent> {
        self.intents
            .iter()
            .map(|i| Intent {
                tree: i.tree.clone(),
                items: i
                    .items
                    .iter()
                    .map(|x| (x.src.text.clone(), x.dst.as_ref().map(|d| d.text.clone())))
                    .collect(),
                open: i.state == IntentState::Open,
                recovered: i.recovered,
            })
            .collect()
    }

    /// The runtime rows one resolution reads ([F18 §2.10] I-F10: `FILEOBS`, `PENDING`, `FSINTENT`, `TREES`, `DIRMAP`,
    /// with `FPRINT` and `PREFIXEV`).
    pub fn runtime(&self) -> Runtime {
        let mut rt = self.rt.clone();
        rt.intents = self.e1_intents();
        rt
    }
}

/// The canonical form of an absolute path argument ([API §4.1]: both absolute forms are accepted whatever the host
/// OS, and results write the canonical form with `/`): `\` read as `/`, a drive letter upper-cased, repeated and
/// trailing `/` dropped (a root keeps its own).
pub fn canon_abs(p: &str) -> String {
    let t = p.replace('\\', "/");
    let mut b: Vec<char> = t.chars().collect();
    if b.len() >= 2 && b[1] == ':' && b[0].is_ascii_alphabetic() {
        b[0] = b[0].to_ascii_uppercase();
    }
    let s: String = b.into_iter().collect();
    let unc = s.starts_with("//");
    let mut out = String::with_capacity(s.len());
    for (i, seg) in s.split('/').enumerate() {
        if seg.is_empty() && i > 0 {
            continue;
        }
        if i > 0 {
            out.push('/');
        }
        out.push_str(seg);
    }
    if unc {
        out = format!("/{out}");
    }
    if out.is_empty() || (out.len() == 2 && out.ends_with(':')) {
        out.push('/');
    }
    out
}

/// The OS a simulated tree is read on, from its root's form ([OS/path §2.2]): a drive or UNC root is Windows, a `/`
/// root Linux.
pub fn os_of(root: &str) -> Os {
    let b = root.as_bytes();
    if root.starts_with("//") || (b.len() >= 2 && b[1] == b':') {
        Os::Windows
    } else {
        Os::Linux
    }
}

/// A git id argument (`sha1:<hex>` or `sha256:<hex>`, [API §5.2]) as the abstract history keeps it: its hex digits.
pub fn git_hex(id: &str) -> String {
    id.split_once(':')
        .map_or(id, |(_, h)| h)
        .to_ascii_lowercase()
}

/// An `oid` value of a git object id kept as hex under an algorithm.
pub fn git_oid(algo: Algo, h: &str) -> Option<Oid> {
    let s = h.as_bytes();
    if !s.len().is_multiple_of(2) {
        return None;
    }
    let digest: Option<Vec<u8>> = (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(h.get(i..i + 2)?, 16).ok())
        .collect();
    let digest = digest?;
    (digest.len() == algo.digest_len()).then_some(Oid { algo, digest })
}

/// A path value's text in a result ([API §5.2]): `"<root name>:<path text>"`.
pub fn path_text(p: &PathVal) -> String {
    format!("{}:{}", p.root, p.text)
}

// ---------------------------------------------------------------------------------------------------------------
// The policy functions of the `files.*` and `roots.<name>` keys ([RULES/policy-keys] KF-033 to KF-044)
// ---------------------------------------------------------------------------------------------------------------

/// KF-033 `roots.<name>`: the directory a named root maps to, canonical; `None` renders `unmapped root` ([40 §2.4]).
// spec: [CFG §10.1] roots.<name>; [40 §2.4]
pub fn root_dir(conf: &Conf, name: &str) -> Option<String> {
    if name == "project" || name == "abs" {
        return None;
    }
    conf.effective(&format!("roots.{name}"), Proc::Cli)
        .map(|(v, _)| canon_abs(&v))
}

/// KF-034 `files.main-tree`: the tree the configuration pairs with `main` in the designation relation ([F18 §3.4]);
/// unset, no pair (the model's `Init` runs in no tree).
// spec: [CFG §10.1] files.main-tree; [F18 §3.4]
pub fn designated_tree(conf: &Conf) -> Option<String> {
    conf.effective("files.main-tree", Proc::Cli)
        .map(|(v, _)| canon_abs(&v))
}

/// KF-035 `files.main-ref`: the expected git ref of the configuration pair of `main`, in the short form of
/// [F18 §3.2] rule 1; `None` when unset or empty.
// spec: [CFG §10.1] files.main-ref; [F18 §3.4]; [F18 §3.6]
pub fn tree_gate(conf: &Conf) -> Option<String> {
    conf.effective("files.main-ref", Proc::Cli)
        .map(|(v, _)| v.strip_prefix("refs/heads/").unwrap_or(&v).to_string())
        .filter(|v| !v.is_empty())
}

/// KF-036 `files.cloud`: `true` when link creation refuses a cloud-only placeholder and every automatic step leaves
/// cloud content alone (`refuse`); `metadata-only` reads metadata only ([40 §4.6], I-F11).
// spec: [CFG §10.1] files.cloud; [40 §4.6]
pub fn cloud_policy(conf: &Conf) -> bool {
    conf.text("files.cloud") == "refuse"
}

/// KF-037 `files.max-read-bytes`: whether a project file of `len` bytes has its content examined; a larger one is
/// `Unavailable(size)` ([F20 §1.5]).
// spec: [CFG §10.4] files.max-read-bytes; [F20 §1.5]
pub fn content_available(conf: &Conf, len: usize) -> bool {
    (len as u64) <= conf.number("files.max-read-bytes")
}

/// KF-039 `files.policy.auto`: `true` for `strong` (a unique strong candidate is applied as a marked guess, I-F6).
// spec: [CFG §10.6] files.policy.auto; [F18 §2.6] I-F6
pub fn auto_policy(conf: &Conf) -> bool {
    conf.text("files.policy.auto") == "strong"
}

/// KF-040 `files.scratchpads`: `true` when `link --at` and `file add` may name a path in a session scratchpad
/// (`allow`).
// spec: [CFG §10.6] files.scratchpads; [40 §2.4]
pub fn scratchpad_policy(conf: &Conf) -> bool {
    conf.text("files.scratchpads") == "allow"
}

/// KF-041 `files.ignore`: the patterns of a tree with no git and no ignore file ([F20 §4.4]).
// spec: [CFG §10.6] files.ignore; [F20 §4.4]
pub fn ignored(conf: &Conf) -> Vec<String> {
    conf.text("files.ignore")
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty() && *s != crate::registry::EMPTY)
        .map(str::to_string)
        .collect()
}

/// KF-042 `files.deletion-inference`: `true` for `main-tree-commits` (a settle in `main`'s writer tree that sees a
/// linked file's deletion committed in HEAD records `removed`, I-F7).
// spec: [CFG §10.6] files.deletion-inference; [F18 §2.7] I-F7
pub fn deletion_inference(conf: &Conf) -> bool {
    conf.text("files.deletion-inference") == "main-tree-commits"
}

/// KF-043 `files.confirm-roles`: the roles that may confirm an agent's guess (`links fix --confirm`, WV-038).
// spec: [CFG §10.6] files.confirm-roles; [F18 §5.5]
pub fn confirm_rights(conf: &Conf) -> Vec<String> {
    conf.text("files.confirm-roles")
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty() && *s != crate::registry::EMPTY)
        .map(str::to_string)
        .collect()
}

/// KF-044 `files.portable-names`: `true` for `refuse` (`file mv` refuses a name some OS cannot hold; `link` and
/// `file add` warn either way, [OS/path §8.2]).
// spec: [CFG §10.6] files.portable-names; [OS/path §8.2]
pub fn portable_name_policy(conf: &Conf) -> bool {
    conf.text("files.portable-names") == "refuse"
}

/// KF-038 `files.max-line-hashes`: the line-hash cap of [F20 §2.4]; beyond it window-only anchors are
/// `unverified (size)`.
// spec: [CFG §10.4] files.max-line-hashes; [F20 §2.4]
pub fn window_available(conf: &Conf) -> Option<usize> {
    Some(conf.number("files.max-line-hashes") as usize)
}

/// KF-037 `files.max-read-bytes` as a resolution's read limit ([F20 §2.4] item 1): beyond it a file's content is
/// `Unavailable(size)` to the file cascade, to the anchor cascade and to a settle's records.
// spec: [CFG §10.4] files.max-read-bytes; [F20 §2.4]
pub fn read_limit(conf: &Conf) -> Option<u64> {
    Some(conf.number("files.max-read-bytes"))
}

/// The anchor constants a resolution evaluates: the drafts of [F20 §7] with the keys' line-hash cap and read limit.
pub fn anchor_consts(conf: &Conf) -> crate::r4::anchor::Consts {
    crate::r4::anchor::Consts {
        max_line_hashes: window_available(conf),
        max_read_bytes: read_limit(conf),
        ..crate::r4::anchor::Consts::DRAFT
    }
}

// ---------------------------------------------------------------------------------------------------------------
// `EnvTree` and `EnvGit`
// ---------------------------------------------------------------------------------------------------------------

/// One commit of `EnvGit` ([API §6.6] `commits`); ids as the API writes them (`sha1:<hex>`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnvGitCommit {
    /// `id`.
    pub id: String,
    /// `parents`.
    pub parents: Vec<String>,
    /// `committer_time`, seconds.
    pub committer_time: i64,
    /// `author_time`, seconds.
    pub author_time: i64,
    /// `tree`: path → git blob id.
    pub tree: Vec<(String, String)>,
}

/// A tree's HEAD in `EnvGit` ([API §6.6] `heads`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EnvHead {
    /// `{"ref": "refs/heads/<name>"}`.
    Ref(String),
    /// `{"detached": <git id>}`.
    Detached(String),
}

/// The data of `EnvTree` ([API §6.5]): the tree's root, its file count and its directory count (the root excluded).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeData {
    /// `tree`.
    pub tree: String,
    /// `files`.
    pub files: usize,
    /// `dirs`.
    pub dirs: usize,
}

impl Store {
    /// `EnvTree` ([API §6.5]): the tree, created on first use with `volume` and `caps` (default: the NTFS row), then
    /// `ops` in order, at `wall_ms` × 10^6. An operation that fails refuses the command (`usage`, exit 2) and the
    /// environment stays as it was: the operations apply together or not at all.
    // spec: [API §6.5]
    pub fn env_tree(
        &mut self,
        tree: &str,
        volume: Option<&str>,
        caps: Option<VolumeCaps>,
        ops: &[TreeOp],
    ) -> Res<Reply> {
        let root = canon_abs(tree);
        let now_ns = self.env.wall_ms.saturating_mul(1_000_000);
        let volume = volume.map_or_else(
            || {
                root.split('/')
                    .next()
                    .filter(|s| !s.is_empty())
                    .unwrap_or("/")
                    .trim_end_matches(':')
                    .to_string()
            },
            str::to_string,
        );
        let mut fs = self.files.fs.clone();
        fs.ensure_tree(
            &root,
            &volume,
            caps.unwrap_or(VolumeCaps::NTFS),
            os_of(&root),
        );
        for (i, op) in ops.iter().enumerate() {
            fs.apply(&root, op, now_ns).map_err(|e| {
                let what = match e {
                    OpError::NotFound(p) => format!("{p} does not exist"),
                    OpError::Exists(p) => format!("{p} exists"),
                    OpError::CrossVolume => "the rename crosses volumes".into(),
                    OpError::NoTree(t) => format!("no tree {t}"),
                };
                Refusal::usage_arg("ops", format!("op {}: {what}", i + 1))
            })?;
        }
        self.files.fs = fs;
        let t = &self.files.fs.trees[&root];
        Ok(Reply::ok(Data::Tree(TreeData {
            tree: root.clone(),
            files: t.files.len(),
            dirs: t.dirs.keys().filter(|k| !k.is_empty()).count(),
        })))
    }

    /// `EnvGit` ([API §6.6]): the repository, created on first use with `algo`; commits added once and never changed;
    /// refs set or deleted; heads that bind simulated trees to the repository.
    // spec: [API §6.6]
    pub fn env_git(
        &mut self,
        repo: &str,
        algo: Option<Algo>,
        commits: &[EnvGitCommit],
        refs: &[(String, Option<String>)],
        heads: &[(String, EnvHead)],
    ) -> Res<Reply> {
        let mut git = self.files.git.clone();
        let r = git.repos.entry(repo.to_string()).or_insert_with(|| Repo {
            algo: algo.unwrap_or(Algo::Sha1),
            commits: BTreeMap::new(),
            refs: BTreeMap::new(),
            blobs: BTreeMap::new(),
        });
        for c in commits {
            let id = git_hex(&c.id);
            let x = GitCommit {
                id: id.clone(),
                parents: c.parents.iter().map(|p| git_hex(p)).collect(),
                committer_time: c.committer_time,
                author_time: c.author_time,
                tree: c
                    .tree
                    .iter()
                    .map(|(p, b)| (p.clone(), git_hex(b)))
                    .collect(),
            };
            match r.commits.get(&id) {
                Some(old) if *old != x => {
                    return Err(Refusal::usage_arg(
                        "commits",
                        format!("commit {} is added once and never changes", c.id),
                    ));
                }
                Some(_) => {}
                None => {
                    r.commits.insert(id, x);
                }
            }
        }
        for (name, to) in refs {
            match to {
                Some(id) => {
                    r.refs.insert(name.clone(), git_hex(id));
                }
                None => {
                    r.refs.remove(name);
                }
            }
        }
        let count = r.commits.len();
        for (tree, h) in heads {
            let head = match h {
                EnvHead::Ref(x) => Head::Ref(x.clone()),
                EnvHead::Detached(c) => Head::Detached(git_hex(c)),
            };
            git.heads.insert(canon_abs(tree), (repo.to_string(), head));
        }
        self.files.git = git;
        Ok(Reply::ok(Data::Git(repo.to_string(), count)))
    }
}

// ---------------------------------------------------------------------------------------------------------------
// The command's tree, the designation relation and the parameters of a resolution
// ---------------------------------------------------------------------------------------------------------------

/// A command's tree ([API §4.2] CX-5; [40 §5.1]) as the file commands read it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeCtx {
    /// The simulated tree's root.
    pub root: String,
    /// The tree is eligible ([40 §5.1]): designated for some branch, holding `FILEOBS` rows, or named by `ctx.tree`.
    pub eligible: bool,
    /// The tree is the writer tree of the caller's branch ([F18 §3.6]).
    pub writer: bool,
    /// The branch's designated tree, when D pairs it with one.
    pub writer_tree: Option<String>,
    /// The canonical current directory inside the tree, for relative path arguments ([OS/path §7]).
    pub cwd: String,
}

impl Store {
    /// The designation relation D over the bindings and the configuration pair of `main` ([F18 §3.4]).
    // spec: [F18 §3.4]
    pub fn designation(&self) -> Vec<Designated> {
        let rows: Vec<Binding> = self
            .heads
            .rows
            .values()
            .filter(|h| h.kind == crate::heads::HeadKind::Directory)
            .filter_map(|h| match &h.target {
                crate::heads::Target::Ref(r) => Some(Binding {
                    dir: h.key.clone(),
                    branch: r.clone(),
                    designated: h.designated,
                    expected_ref: h.expected_ref.clone(),
                    base: h.base.as_deref().map(git_hex),
                }),
                crate::heads::Target::Detached(_) => None,
            })
            .collect();
        let main_tree = designated_tree(&self.conf);
        let main_ref = tree_gate(&self.conf);
        designation(&rows, main_tree.as_deref(), main_ref.as_deref())
    }

    /// The command's tree (CX-5): the simulated tree that holds the resolved tree path, with its eligibility and
    /// whether it is the caller's branch's writer tree; `None` when CX-5 names no simulated tree.
    pub fn tree_ctx(&self, caller: &Caller, ctx: &Ctx) -> Option<TreeCtx> {
        let t = canon_abs(caller.tree.as_deref()?);
        let root = self.files.tree_of(&t)?;
        let d = self.designation();
        let designated_any = d.iter().any(|p| p.tree == root);
        let has_rows = self.files.rt.fileobs.keys().any(|(_, r)| *r == root);
        let explicit = ctx.tree.as_deref().is_some_and(|x| canon_abs(x) == t);
        let writer = writer_tree(&caller.branch, &root, &d, &self.files.git);
        let writer_tree = d
            .iter()
            .find(|p| p.branch == caller.branch)
            .map(|p| p.tree.clone());
        let cwd = ctx
            .cwd
            .as_deref()
            .map(canon_abs)
            .filter(|c| *c == root || c.starts_with(&format!("{}/", root.trim_end_matches('/'))))
            .unwrap_or_else(|| root.clone());
        Some(TreeCtx {
            root,
            eligible: designated_any || has_rows || explicit,
            writer,
            writer_tree,
            cwd,
        })
    }

    /// The `oid` algorithm of the `project` root: the object format of the tree's repository, SHA-1 without one
    /// ([40 §2.5]; the model's `Init` reads no repository, so it takes the command tree's); every other root SHA-1.
    pub fn root_algo(&self, root_name: &str, tree: &str) -> Algo {
        match (root_name, self.files.git.of_tree(tree)) {
            ("project", Some((r, _))) => r.algo,
            _ => Algo::Sha1,
        }
    }

    /// The parameters of one resolution in a tree for the caller's branch ([F20 §1.3]; [F18 §2.10] I-F10): the tree
    /// predicates, the policy keys, the named roots and the anchor constants.
    pub fn params(&self, tc: &TreeCtx, branch: &str, settle: bool, view: &View) -> Params {
        let named: BTreeMap<String, String> = view
            .files
            .iter()
            .map(|f| f.root.clone())
            .collect::<BTreeSet<String>>()
            .into_iter()
            .filter_map(|r| root_dir(&self.conf, &r).map(|d| (r, d)))
            .collect();
        Params {
            settle,
            eligible: tc.eligible,
            writer: tc.writer,
            main: branch == "main",
            policy_strong: auto_policy(&self.conf),
            skew_ns: self.files.skew_ns,
            algo: self.root_algo("project", &tc.root),
            ignore_defaults: ignored(&self.conf),
            lane_scope: None,
            named_roots: named,
            siblings: Vec::new(),
            no_pending: false,
            stamp_ns: None,
            anchor: anchor_consts(&self.conf),
            max_read_bytes: read_limit(&self.conf),
        }
    }

    /// The commit header's git group of a command's commits ([API §4.4]; [F06 §4.4.6]; [F07 §3.6] item 5): the resolved
    /// tree's repository format, its HEAD commit, the short name of its symbolic HEAD, the tree's canonical root text
    /// and the base of its designated binding; `None` when the tree has no git.
    // spec: [API §4.4]; [F07 §3.6]
    pub fn git_group(&self, caller: &Caller) -> Option<crate::canon::Git> {
        let t = canon_abs(caller.tree.as_deref()?);
        let root = self.files.tree_of(&t)?;
        let (repo, head) = self.files.git.of_tree(&root)?;
        let h = repo.head_commit(head).and_then(|c| git_oid(repo.algo, c));
        let branch = match head {
            Head::Ref(r) => r.strip_prefix("refs/heads/").unwrap_or(r).to_string(),
            Head::Detached(_) => String::new(),
        };
        let base = self
            .designation()
            .into_iter()
            .find(|p| p.tree == root)
            .and_then(|p| p.base)
            .and_then(|b| git_oid(repo.algo, &b));
        Some(crate::canon::Git {
            algo: repo.algo,
            head: h.map(|o| o.digest),
            branch,
            worktree: root,
            base: base.map(|o| o.digest),
        })
    }
}

// ---------------------------------------------------------------------------------------------------------------
// The reading view as the resolver reads it
// ---------------------------------------------------------------------------------------------------------------

/// The text of a `path` field value.
fn path_of(v: Option<&Value>) -> Option<&PathVal> {
    match v? {
        Value::Path(p) => Some(p),
        _ => None,
    }
}

/// The hex digits of an `oid` field value.
fn hex_of(v: Option<&Value>) -> Option<String> {
    match v? {
        Value::Oid(o) => Some(hex(&o.digest)),
        _ => None,
    }
}

/// The observation side of a composite value, as a settle reads it.
fn side_of(v: &Option<KVal>) -> Option<Side> {
    let Some(KVal::Observation(vs)) = v else {
        return None;
    };
    Some(Side {
        path: path_of(vs.first().and_then(Option::as_ref))?.text.clone(),
        oid: match vs.get(1).and_then(Option::as_ref) {
            Some(Value::Oid(o)) => Some(o.clone()),
            _ => None,
        },
        observed_git: hex_of(vs.get(3).and_then(Option::as_ref)),
        observed_blob: hex_of(vs.get(4).and_then(Option::as_ref)),
    })
}

/// The hlc of the newest commit of the first-parent chain from `tip` that changed each node's observation composite
/// (G4's time window, [F20 §5.11.2]).
fn obs_hlcs(st: &Store, tip: Option<u64>) -> BTreeMap<Nid, u64> {
    let mut out = BTreeMap::new();
    for c in st.dag.chain(tip) {
        let x = &st.dag.commits[&c];
        for k in x.changeset.keys() {
            if let Key::Node(n, Aspect::Observation) = k {
                out.entry(*n).or_insert(x.hlc);
            }
        }
    }
    out
}

/// A file node of a view as the resolver reads it ([40 §2.2]).
pub fn file_node(n: Nid, x: &Node, obs_hlc: u64) -> FileNode {
    let root = x.text("root").unwrap_or("project").to_string();
    let path = path_of(x.fields.get("path"))
        .or_else(|| path_of(x.fields.get("origin_path")))
        .map(|p| p.text.clone())
        .unwrap_or_default();
    let status = FileStatus::from_name(&x.status).unwrap_or(FileStatus::Present);
    let conflict = x.conflicts.get(&Aspect::Observation);
    FileNode {
        n: n.0,
        root,
        path,
        oid: match x.fields.get("oid") {
            Some(Value::Oid(o)) => Some(o.clone()),
            _ => None,
        },
        bytes: match x.fields.get("bytes") {
            Some(Value::Int(b)) => Some(*b as u64),
            _ => None,
        },
        observed_git: hex_of(x.fields.get("observed_git")),
        observed_blob: hex_of(x.fields.get("observed_blob")),
        relink: x.text("relink").map(str::to_string),
        aliases: x
            .fields
            .get("aliases")
            .map(|v| {
                v.elems()
                    .iter()
                    .filter_map(|e| path_of(Some(e)).map(|p| p.text.clone()))
                    .collect()
            })
            .unwrap_or_default(),
        status,
        artifact_kind: x.text("artifact_kind").map(str::to_string).or_else(|| {
            match x.fields.get("artifact_kind") {
                Some(Value::Enum(s)) => Some(s.clone()),
                _ => None,
            }
        }),
        tombstone: !x.live(),
        obs_hlc,
        conflict: conflict
            .filter(|c| c.class == "FieldEdit")
            .and_then(|c| Some(Box::new([side_of(&c.ours)?, side_of(&c.theirs)?]))),
        path_claim: conflict.is_some_and(|c| c.class == "PathClaim"),
    }
}

impl Store {
    /// The reading view of a state as the resolver reads it ([40 §2.2], §2.4, §5.5): every artifact node, each root
    /// node's `path_moves`, and the anchors whose selectors hold a `FieldEdit` conflict value.
    pub fn view_of(&self, st: &State, tip: Option<u64>) -> View {
        let hlcs = obs_hlcs(self, tip);
        let mut v = View::default();
        for (n, x) in &st.nodes {
            if x.kind == "artifact" {
                v.files
                    .push(file_node(*n, x, hlcs.get(n).copied().unwrap_or(0)));
            }
            if x.kind == "area"
                && x.live()
                && let Some(r) = x.text("root")
                && let Some(ms) = x.fields.get("path_moves")
            {
                v.moves.insert(
                    r.to_string(),
                    ms.elems()
                        .iter()
                        .filter_map(|m| match m {
                            Value::PathMove(m) => Some((**m).clone()),
                            _ => None,
                        })
                        .collect(),
                );
            }
            for (a, c) in &x.conflicts {
                let Aspect::Edge(k) = a else { continue };
                if k.kind != "at" || c.class != "FieldEdit" {
                    continue;
                }
                let anchor = |s: &Option<KVal>| match s {
                    Some(KVal::Edge(p)) => p.anchor.as_deref().map(|a| {
                        crate::r4::anchor::Anchor::from_canon(k.disc.unwrap_or(Uid::ZERO), a)
                    }),
                    _ => None,
                };
                if let (Some(o), Some(t), Some(d)) = (anchor(&c.ours), anchor(&c.theirs), k.disc) {
                    v.anchor_conflicts.push(AnchorConflict {
                        file: k.dst.0,
                        anchor: d,
                        sides: Box::new([o, t]),
                    });
                }
            }
        }
        v
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Writing file nodes, root nodes and observations on a candidate state
// ---------------------------------------------------------------------------------------------------------------

/// The observation composite of a file node: `path`, `oid`, `bytes`, `observed_git`, `observed_blob`, `relink`
/// ([F08 §9.3]; [40 §2.2]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Observation {
    /// `path`.
    pub path: PathVal,
    /// `oid`.
    pub oid: Option<Oid>,
    /// `bytes`.
    pub bytes: Option<u64>,
    /// `observed_git`.
    pub observed_git: Option<Oid>,
    /// `observed_blob`.
    pub observed_blob: Option<Oid>,
    /// `relink`.
    pub relink: Option<String>,
}

/// Sets a node's observation composite in canonical form (an empty value is absent).
pub fn set_observation(st: &mut State, n: Nid, o: &Observation) {
    let schema = st.schema.clone();
    let x = st.nodes.get_mut(&n).expect("a file node");
    x.set_field(&schema, "path", Some(Value::Path(o.path.clone())));
    x.set_field(&schema, "oid", o.oid.clone().map(Value::Oid));
    x.set_field(&schema, "bytes", o.bytes.map(|b| Value::Int(b as i64)));
    x.set_field(
        &schema,
        "observed_git",
        o.observed_git.clone().map(Value::Oid),
    );
    x.set_field(
        &schema,
        "observed_blob",
        o.observed_blob.clone().map(Value::Oid),
    );
    x.set_field(&schema, "relink", o.relink.clone().map(Value::Text));
    x.conflicts.remove(&Aspect::Observation);
}

/// Adds a path to a node's `aliases` (an add-wins set, [40 §2.2]); a path equal to the current one is not an alias.
pub fn add_alias(st: &mut State, n: Nid, p: &PathVal) {
    let schema = st.schema.clone();
    let x = st.nodes.get_mut(&n).expect("a file node");
    if path_of(x.fields.get("path")) == Some(p) {
        return;
    }
    let mut v: Vec<Value> = x
        .fields
        .get("aliases")
        .map(|s| s.elems().to_vec())
        .unwrap_or_default();
    v.push(Value::Path(p.clone()));
    x.set_field(&schema, "aliases", Value::set(v));
}

/// A candidate of a file command: the state, the new allocations and the next free `#N`.
pub struct FileCand {
    /// The view before the command.
    pub base: Rc<State>,
    /// The candidate state.
    pub st: State,
    /// New allocations: `#N` → (uid, creator).
    pub new_alloc: BTreeMap<Nid, (Uid, Creator)>,
    /// The next `#N`.
    pub next_id: u32,
    /// The creator of the nodes the command registers.
    pub creator: Creator,
    /// The nodes created, in order.
    pub created: Vec<Nid>,
}

impl FileCand {
    /// The `#N` of a derived uid: its store-wide number, a number this candidate gave it, or `next_id`
    /// ([F08 §11.2] step 5, I-F2).
    pub fn number(&mut self, uidx: &BTreeMap<Uid, Nid>, u: Uid) -> Res<Nid> {
        if let Some(n) = uidx.get(&u) {
            return Ok(*n);
        }
        if let Some((n, _)) = self.new_alloc.iter().find(|(_, (x, _))| *x == u) {
            return Ok(*n);
        }
        if self.next_id == u32::MAX {
            return Err(Refusal::new(
                "id_space_exhausted",
                7,
                "the node id space of this store is full",
            ));
        }
        let n = Nid(self.next_id);
        self.next_id += 1;
        self.new_alloc.insert(n, (u, self.creator.clone()));
        Ok(n)
    }

    /// Creates a live node of `kind` with a derived uid on the candidate (or brings the view's node of that uid, which
    /// is not live, back by an explicit door); returns its `#N`.
    pub fn create(&mut self, uidx: &BTreeMap<Uid, Nid>, u: Uid, kind: &str) -> Res<Nid> {
        let n = self.number(uidx, u)?;
        let node = Node::new(u, kind, &self.st.schema, self.creator.clone());
        self.st.nodes.insert(n, node);
        self.created.push(n);
        Ok(n)
    }

    /// The root node of a root name ([40 §2.4]; [F08 §11.3]): the view's live node of `uid_root(name)`, else a new
    /// `area` with `root` and the title `root:<name>`, created in the same commit as the root's first file node.
    pub fn root_node(&mut self, uidx: &BTreeMap<Uid, Nid>, name: &str) -> Res<Nid> {
        let u = crate::r4::uid::uid_root(name);
        if let Some((n, _)) = self.st.nodes.iter().find(|(_, x)| x.uid == u && x.live()) {
            return Ok(*n);
        }
        let n = self.create(uidx, u, "area")?;
        let schema = self.st.schema.clone();
        let x = self.st.nodes.get_mut(&n).expect("created");
        x.set_field(&schema, "root", Some(Value::Text(name.to_string())));
        x.set_field(&schema, "title", Some(Value::Text(format!("root:{name}"))));
        Ok(n)
    }

    /// Adds a `path_moves` entry to the root node of `root` ([40 §2.4]; CK-5).
    pub fn add_path_move(&mut self, uidx: &BTreeMap<Uid, Nid>, m: PathMove) -> Res<()> {
        let n = self.root_node(uidx, &m.from.root.clone())?;
        let schema = self.st.schema.clone();
        let x = self.st.nodes.get_mut(&n).expect("the root node");
        let mut v: Vec<Value> = x
            .fields
            .get("path_moves")
            .map(|s| s.elems().to_vec())
            .unwrap_or_default();
        v.push(Value::PathMove(Box::new(m)));
        x.set_field(&schema, "path_moves", Value::set(v));
        Ok(())
    }
}

/// A new `pathmove` entry ([40 §2.4]).
pub fn path_move(
    hlc: u64,
    class: MoveClass,
    root: &str,
    from: &str,
    to: &str,
    git: Option<Oid>,
) -> PathMove {
    PathMove {
        hlc,
        class,
        from: PathVal {
            root: root.to_string(),
            text: from.to_string(),
        },
        to: PathVal {
            root: root.to_string(),
            text: to.to_string(),
        },
        git,
    }
}

/// The literal prefix of a glob up to the last `/` before its first wildcard ([40 §2.4] "Globs").
pub fn literal_prefix(g: &str) -> &str {
    let cut = g.find(['*', '?', '[', '{']).unwrap_or(g.len());
    let lit = &g[..cut];
    match lit.rfind('/') {
        Some(i) => &g[..=i],
        None => "",
    }
}

/// The globs a directory move rewrites ([40 §2.4] "Globs"): in every live node, every element of a field of merge class
/// `glob-set` — a `files_owned` or `path_globs` glob, the glob of an `applies_to` element tagged `path:`
/// ([F08 §5.4.3], §5.4.6) — whose literal prefix starts with `from`, with `from` replaced by `to`. Returns (node,
/// field, old element, new element) in (`#N`, field, element) order.
pub fn rewrite_globs(st: &mut State, from: &str, to: &str) -> Vec<(Nid, String, String, String)> {
    let schema = st.schema.clone();
    let mut out = Vec::new();
    let ns: Vec<Nid> = st.nodes.keys().copied().collect();
    for n in ns {
        let x = &st.nodes[&n];
        if !x.live() {
            continue;
        }
        let fields: Vec<(String, Value, bool)> = x
            .fields
            .iter()
            .filter_map(|(f, v)| {
                let fi = schema.field(&x.kind, f)?;
                (fi.class == "glob-set").then(|| {
                    (
                        f.clone(),
                        v.clone(),
                        fi.shape == crate::schema::Shape::Tagged,
                    )
                })
            })
            .collect();
        for (field, v, tagged) in fields {
            let mut changed = false;
            let elems: Vec<Value> = v
                .elems()
                .iter()
                .map(|e| {
                    let Some(t) = e.as_str() else {
                        return e.clone();
                    };
                    let (tag, g) = if tagged {
                        match t.strip_prefix("path:") {
                            Some(g) => ("path:", g),
                            None => return e.clone(),
                        }
                    } else {
                        ("", t)
                    };
                    if !literal_prefix(g).starts_with(from) {
                        return e.clone();
                    }
                    let ne = format!("{tag}{to}{}", &g[from.len()..]);
                    out.push((n, field.clone(), t.to_string(), ne.clone()));
                    changed = true;
                    match e {
                        Value::Enum(_) => Value::Enum(ne),
                        _ => Value::Text(ne),
                    }
                })
                .collect();
            if changed {
                st.nodes.get_mut(&n).expect("a live node").set_field(
                    &schema,
                    &field,
                    Value::set(elems),
                );
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------------------------
// Landing a file command's commit
// ---------------------------------------------------------------------------------------------------------------

/// How a file command's commit is written: its keying, statement origin and message.
pub struct Landing<'a> {
    /// The resolved caller.
    pub caller: &'a Caller,
    /// The command's context.
    pub ctx: &'a Ctx,
    /// The idempotency key.
    pub key: Option<([u8; 16], bool)>,
    /// The payload.
    pub payload: [u8; 16],
    /// The command name ([API §2.2]).
    pub cmd: &'static str,
    /// `stmt_origin` ([F06 §3.4]).
    pub origin: &'static str,
    /// `stmt_sym`.
    pub sym: String,
    /// The yields a family-T file named mutation records ([API §3.3]).
    pub yields: Vec<crate::tx::Yield>,
}

impl Store {
    /// The `stmt_origin` and `stmt_sym` of a file named mutation ([F06 §3.4]): `named-mutation` through MCP, else
    /// `verb`, with the mutation's name; a file named mutation is no `TX` block, so it has no `stmt_hash`.
    pub fn named_origin(ctx: &Ctx) -> &'static str {
        if ctx.door == crate::api::Door::Mcp {
            "named-mutation"
        } else {
            "verb"
        }
    }

    /// Lands a file command's candidate as one commit on the caller's branch ([API §12.1] "Commits"): the net
    /// changeset, the new allocations, the commit with its origin and idempotency pair, its `Marker` record, the change
    /// feed and the idempotency entry. A dry run and an empty changeset write nothing. `after_commit` draws the HLC of
    /// the records the group carries after its `Marker` record (an intent's `FsIntentDone`, CK-4). Returns the reply
    /// with `branch`, `rev`, `commit`, `rev_new`, `key`, `diff`, `ready`, `other`, `markers` and `yields` set.
    pub fn land_file(
        &mut self,
        l: Landing<'_>,
        c: FileCand,
        tip: Option<u64>,
        message: &str,
        after_commit: usize,
    ) -> Res<(Reply, Option<u64>)> {
        let cs: Changeset = diff(&c.base, &c.st);
        crate::budget::check_caps(
            self.cfg.max_statements,
            self.cfg.max_ops,
            0,
            cs.len() as u64,
        )?;
        let mut reply = Reply::ok(Data::None);
        reply.branch = Some(l.caller.branch.clone());
        reply.rev = Some(tip.unwrap_or(0));
        reply.commit = tip;
        reply.key = l.ctx.key.clone();
        reply.lease = l.ctx.lease.clone();
        reply.warnings = l.caller.warnings.clone();
        reply.yields = l.yields.clone();
        let msg = crate::api::normalize_message(message)?;
        if l.ctx.dry {
            reply.outcome = Outcome::Dry;
            reply.diff = cs;
            return Ok((reply, None));
        }
        if cs.is_empty() {
            // No commit: the `Idem` record, then the records the group carries after it (CK-4; [F05 §4.7]).
            self.record_idem(
                l.key,
                l.payload,
                &l.caller.branch,
                None,
                l.ctx,
                Recorded {
                    cmd: l.cmd.into(),
                    items: Vec::new(),
                    yields: l.yields.clone(),
                },
            );
            for _ in 0..after_commit {
                self.hlc.record(self.env.wall_ms);
            }
            return Ok((reply, None));
        }
        let parent_rows = self.rows_at(tip);
        let ready_before = crate::coord::ready_set(
            &self.dag,
            &self.alloc,
            &l.caller.branch,
            &self.leases,
            &self.env,
            Some(&l.caller.actor),
            Some(&parent_rows),
        );
        for (n, (u, cr)) in &c.new_alloc {
            self.alloc.rows.insert(
                *n,
                (*u, cr.clone(), l.caller.branch.clone(), self.commit_seq + 1),
            );
            self.alloc.uidx.insert(*u, *n);
            self.alloc.uids.insert(*n, *u);
        }
        self.next_id = self.next_id.max(c.next_id);
        let st_after = Rc::new(c.st);
        let child_rows = Rc::new(crate::derived::recompute_all(&st_after, &|_| None));
        let (affected, complete) =
            crate::derived::affected_rows(&parent_rows, &child_rows, self.cfg.suspect_budget);
        let seq = self.append_commit(
            l.caller,
            st_after,
            cs.clone(),
            msg,
            l.origin,
            Some(l.sym.clone()),
            None,
            l.key.map(|(k, _)| (k, l.payload)),
            affected,
            complete,
        );
        self.rows.insert(seq, child_rows);
        self.prune_rows();
        let entries =
            self.markers
                .commit_lands(&self.dag, seq, tip, &[], &mut self.hlc, self.env.wall_ms);
        reply.markers = self.listed(&entries);
        self.feed_markers(&entries, Some(seq));
        for _ in 0..after_commit {
            self.hlc.record(self.env.wall_ms);
        }
        self.record_idem(
            l.key,
            l.payload,
            &l.caller.branch,
            Some(seq),
            l.ctx,
            Recorded {
                cmd: l.cmd.into(),
                items: Vec::new(),
                yields: l.yields.clone(),
            },
        );
        let rows_after = self.rows_at(Some(seq));
        let ready_after = crate::coord::ready_set(
            &self.dag,
            &self.alloc,
            &l.caller.branch,
            &self.leases,
            &self.env,
            Some(&l.caller.actor),
            Some(&rows_after),
        );
        reply.ready = ready_after
            .iter()
            .filter(|n| !ready_before.contains(n))
            .copied()
            .collect();
        reply.other = self.dag.commits[&seq]
            .affected
            .iter()
            .filter(|n| !reply.ready.contains(n))
            .copied()
            .collect();
        reply.commit = Some(seq);
        reply.rev_new = Some(seq);
        reply.diff = cs;
        Ok((reply, Some(seq)))
    }

    /// A new candidate over the caller's branch tip, creating nodes as the caller.
    pub fn file_cand(&self, caller: &Caller, tip: Option<u64>) -> FileCand {
        let base = self.dag.state_at(tip, &self.alloc);
        FileCand {
            st: (*base).clone(),
            base,
            new_alloc: BTreeMap::new(),
            next_id: self.next_id,
            creator: Creator {
                actor: caller.actor.clone(),
                role: caller
                    .lease
                    .and_then(|l| self.leases.get(&l))
                    .map(|l| l.role.clone())
                    .unwrap_or_default(),
            },
            created: Vec::new(),
        }
    }

    /// The idempotency payload of a group-F command ([API §7.3] `payload(c)` over its arguments; the model's reading
    /// for every file command, the file named mutations included, whose arguments no single `CALL` carries).
    pub fn file_payload(name: &str, args: BTreeMap<String, Cj>) -> [u8; 16] {
        crate::idem::payload(name, &args)
    }

    /// The `tree_mismatch` refusal of [API §4.3] row 5: a tree-derived write with `ctx.tree` outside the presented task
    /// or run-scoped role lease's lane tree ([F19 §10.2]).
    // spec: [API §4.3] row 5
    pub fn tree_mismatch(&self, caller: &Caller, ctx: &Ctx) -> Res<()> {
        let (Some(t), Some(id)) = (ctx.tree.as_deref(), caller.lease) else {
            return Ok(());
        };
        let Some(l) = self.leases.get(&id).filter(|l| !l.session_role) else {
            return Ok(());
        };
        let main_tip = self.dag.live("main").and_then(|r| r.tip);
        let main = self.dag.state_at(main_tip, &self.alloc);
        let Some(lane) = main
            .nodes
            .values()
            .find(|x| {
                x.live() && x.kind == "lane" && x.text("moirai_branch") == Some(l.branch.as_str())
            })
            .and_then(|x| path_of(x.fields.get("worktree_path")).map(|p| canon_abs(&p.text)))
        else {
            return Ok(());
        };
        let t = canon_abs(t);
        if t == lane || t.starts_with(&format!("{}/", lane.trim_end_matches('/'))) {
            return Ok(());
        }
        Err(Refusal::new(
            "tree_mismatch",
            5,
            format!("tree {t} is outside lease L-{id}'s lane {}", l.branch),
        )
        .key("tree", t)
        .key("lease", format!("L-{id}"))
        .key("lane_tree", lane))
    }

    /// The `not_writer_tree` refusal ([F19 §10.2]; WZ-009): the command's tree is not the writer tree of the caller's
    /// branch.
    pub fn not_writer_tree(&self, tc: &TreeCtx, branch: &str) -> Refusal {
        let msg = match &tc.writer_tree {
            Some(w) => format!(
                "{} is not the writer tree of {branch} (writer tree: {w})",
                tc.root
            ),
            None => format!("{branch} has no writer tree"),
        };
        Refusal::new("not_writer_tree", 5, msg)
            .key("tree", tc.root.clone())
            .key(
                "writer_tree",
                tc.writer_tree
                    .clone()
                    .map_or(crate::err::Kv::Null, crate::err::Kv::Str),
            )
            .key("ref", branch)
    }
}

impl Store {
    /// The `FILEOBS` rows a file verb writes for the file nodes it observed at their paths in a tree ([40 §2.6]: "Written
    /// only by writer paths: settles, the file verbs, and hooks"; [F11 §12.5]): the file's identity with its parent
    /// directory, its size and times (the creation time by the settle's own rule, `Tree::recorded_creation`: absent on
    /// a volume without creation times, [F20 §5.2]), `last_oid`, and `verified_at` = the command's HLC; a node whose
    /// path no longer holds a file loses its row in the tree. A dry run writes none.
    pub fn observe_rows(&mut self, ctx: &Ctx, root: &str, seen: &[(Nid, Option<String>)]) {
        if ctx.dry {
            return;
        }
        let hlc = self.hlc.peek(self.env.wall_ms);
        let algo = self.root_algo("project", root);
        let Some(t) = self.files.fs.trees.get(root) else {
            return;
        };
        let mut rows = Vec::new();
        for (n, disk) in seen {
            let key = (n.0, root.to_string());
            let stat = disk.as_deref().map(|d| t.stat(d));
            match stat {
                Some(crate::r4::tree::StatOut::Present(s)) => {
                    let last_oid = t
                        .read(&s.disk_path)
                        .ok()
                        .map(|b| crate::r4::text::oid(algo, b));
                    rows.push((
                        key,
                        Some(crate::r4::cascade::FileObs {
                            file_id: Some(s.id.clone()),
                            parent_dir: Some(s.parent),
                            size: s.size,
                            mtime_ns: s.mtime_ns,
                            creation_ns: t.recorded_creation(s.btime_ns),
                            last_oid,
                            verified_at: hlc,
                            missing_since: 0,
                            recorded: None,
                            path_seen: None,
                        }),
                    ));
                }
                _ => rows.push((key, None)),
            }
        }
        for (k, row) in rows {
            match row {
                Some(r) => {
                    self.files.rt.fileobs.insert(k, r);
                }
                None => {
                    self.files.rt.fileobs.remove(&k);
                }
            }
        }
    }
}

/// The state of an artifact node's observation key: a conflict value it holds.
pub fn obs_conflict(x: &Node) -> Option<&crate::state::Conflict> {
    x.conflicts.get(&Aspect::Observation)
}

/// Whether a key state is a conflict value.
pub fn is_conflict(k: &KState) -> bool {
    matches!(k, KState::Conflict(_))
}
