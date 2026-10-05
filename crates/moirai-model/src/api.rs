//! The logical `Store` API on the reference model ([API]): one executor runs a stream of commands in order
//! ([API §2.1]); each command is resolved against its caller context ([API §4]), checked for idempotency ([API §7]) and
//! run on a candidate ([`crate::tx`]); results are typed values that `moirai-testkit` converts into the `--json v1`
//! data of [API §3] (the model has no JSON code, [PLAN §3.2] item 9).
//!
//! The commands of this module are groups E (`EnvClock`, `EnvSlots`, `EnvCrash`), S (`Init`, `ConfigSet`,
//! `ConfigUnset` in [`crate::confcmd`], `Quiet`, `Maintain`, `Gc` in [`crate::gc`]), G (`Tx` in its data-level form,
//! `Mutation`, `Schema`, `Apply` in [`crate::apply`]), C (`Claim`, `Heartbeat`, `Release`, `Reclaim`, `Complete`,
//! `RunOpen`, `RunClose`), the ref commands `BranchCreate`, `BranchDelete`, `Checkout`, `WorktreeBind` and
//! `WorktreeUnbind` ([`crate::heads`]), and O (`State`, `Runtime`, `History`). Every commit and ref move feeds the
//! marker cache ([`crate::markers`]) and the change feed ([`crate::feed`]). The merge family and history verbs are
//! WP-91's (on [`crate::refmove`] and the marker events); the file-link group F (`FileAdd`, `LinkFile`, `UnlinkFile`,
//! `FileMv`, `FileRm`, `FileRevert`, `FileRelink`, `LinksFix`, `LinksSync`, `Check`) and the environment commands
//! `EnvTree` and `EnvGit` ([API §12], §6.5, §6.6) are WP-92's, in [`crate::links`] over [`crate::r4`]; `Tx` in its
//! `lq`/`ir` forms and `Query` are WP-93b's.
//!
//! Every keyed command looks its key up right after the resolution of its caller context ([API §4.3], §7.4; [AR §4.5]
//! step 2), before any precondition of the command, so a retry of a command that succeeded replays its result.

use crate::clock::{Env, EnvClock, EnvSlots, Hlc, SlotsResult};
use crate::confcmd::{ConfigData, FileScope};
use crate::config;
use crate::context;
use crate::coord::{self, Oracle};
use crate::dag::{self, Commit, Dag, MoveReason, Ref, RefKind, RefMove};
use crate::derived::{self, Row};
use crate::err::{Kv, Refusal, Res};
use crate::feed::Feed;
use crate::heads::{BindData, CheckoutData, Heads};
use crate::idem::{self, Cj, Entry, Lookup, Recorded, ResultItem, Windows};
use crate::lease::{self, Lease, Live};
use crate::lq::ctx::{Caller as LqCaller, Params, Profile, Value as P};
use crate::lqh;
use crate::markers::{self, Group, Markers};
use crate::mutation;
use crate::policy::{self, PolicyData, Presented, Rights, Surface};
use crate::registry::{self, Conf};
use crate::schema::{Item, ItemKey, Schema, item_cj};
use crate::state::{Alloc, Changeset, Creator, KState, KVal, Key, State, diff};
use crate::tx::{self, Cand, KernelCfg, LeaseEvent, Stmt, Target, Yield};
use crate::value::{Nid, Uid, blake3_128};
use std::cell::OnceCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

/// A named mutation run through [`Store::run`]'s write path: its name, its expansion and its own parameters.
type Named<'a> = (&'a str, &'a mutation::Expansion, &'a [(String, P)]);

/// The door of a command ([API §1.4]).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Door {
    /// A CLI verb or `moirai tx`.
    #[default]
    Cli,
    /// An MCP tool call.
    Mcp,
    /// A hook.
    Hook,
    /// An entry of a batch.
    Apply,
}

/// Codex `_meta` of an MCP call ([API §4.1] `meta`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Meta {
    /// `threadId`.
    pub thread_id: Option<String>,
    /// `sessionId`.
    pub session_id: Option<String>,
    /// `sandboxCwd`.
    pub sandbox_cwd: Option<String>,
}

/// The Claude Code stamp ([API §4.1] `stamp`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StampCtx {
    /// `session_id`.
    pub session_id: Option<String>,
    /// `agent_id`.
    pub agent_id: Option<String>,
    /// `agent_type`.
    pub agent_type: Option<String>,
    /// `cwd`.
    pub cwd: Option<String>,
}

/// The caller context of a command, the raw inputs of [90 §4.1]'s resolver ([API §4.1]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Ctx {
    /// `door`.
    pub door: Door,
    /// `branch`.
    pub branch: Option<String>,
    /// `lease`: `L-<n>`.
    pub lease: Option<String>,
    /// `agent`.
    pub agent: Option<String>,
    /// `client`.
    pub client: Option<String>,
    /// `tree`.
    pub tree: Option<String>,
    /// `model`.
    pub model: Option<String>,
    /// `cwd`.
    pub cwd: Option<String>,
    /// `env`: only the names [API §4.1] lists are read.
    pub env: BTreeMap<String, String>,
    /// `meta`.
    pub meta: Option<Meta>,
    /// `stamp`.
    pub stamp: Option<StampCtx>,
    /// `marker`.
    pub marker: Option<String>,
    /// `client_info`.
    pub client_info: Option<String>,
    /// `hook_label`.
    pub hook_label: Option<String>,
    /// `hook_model`.
    pub hook_model: Option<String>,
    /// `key`.
    pub key: Option<String>,
    /// `no_dedupe`.
    pub no_dedupe: bool,
    /// `dry`.
    pub dry: bool,
    /// `if_tip`: the commit the block expects the branch's tip to be, by its seq (the model's commit identity until
    /// WP-91's commit ids).
    pub if_tip: Option<u64>,
}

/// The resolved caller ([API §4.2] CX-1 to CX-9).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Caller {
    /// CX-1: the presented lease.
    pub lease: Option<u64>,
    /// Whether the lease came from the environment (CX-9).
    pub env_lease: bool,
    /// CX-2: the branch; empty when the view is a detached client head.
    pub branch: String,
    /// CX-2: the detached commit a client head names, when it names one.
    pub detached: Option<u64>,
    /// CX-5: the tree.
    pub tree: Option<String>,
    /// CX-6: the declared model, when some source declares one.
    pub model: Option<String>,
    /// The model profile ([90 §8.2]).
    pub profile: crate::lq::ctx::Profile,
    /// CX-3: the actor.
    pub actor: String,
    /// CX-3: `actor_src`.
    pub actor_src: &'static str,
    /// CX-4: the session identity.
    pub session: Option<String>,
    /// CX-7: the client profile.
    pub client: &'static str,
    /// CX-8: the effective role.
    pub role: String,
    /// CX-8: the narrowing label.
    pub label: Option<String>,
    /// The attested thread (`codex:` + thread id), for CX-9 and the session role lease's `bound`.
    pub thread: Option<String>,
    /// Warnings the resolution raised (`two_harnesses`, `hook_label_narrowed`).
    pub warnings: Vec<String>,
    /// The first of §4.3 rows 1–4 the call breaks, which a keyed command raises only after its idempotency lookup
    /// found no entry ([API §4.3] "The idempotency pre-check comes first"; [`Store::keyed`]); `None` when it breaks none.
    pub pending: Option<Refusal>,
    /// The presented lease when it has ended: the lookup binds the block's `H` with its role, as the original call
    /// did, since `H` does not depend on who presents it ([LQ/canonical-ast §7.2]).
    pub ended_lease: Option<u64>,
}

/// A command of the model's stream ([API §2.2]).
#[derive(Clone, Debug, PartialEq)]
pub enum Cmd {
    /// `EnvClock` ([API §6.2]).
    EnvClock(EnvClock),
    /// `EnvSlots` ([API §6.3]).
    EnvSlots(EnvSlots),
    /// `EnvCrash` ([API §6.7]): `true` for `in-next`, `false` for `between`.
    EnvCrash {
        /// `at = "in-next"`.
        in_next: bool,
    },
    /// `EnvTree` ([API §6.5]).
    EnvTree {
        /// `tree`: the root.
        tree: String,
        /// `volume`, on first use.
        volume: Option<String>,
        /// `caps` by member name, on first use; the NTFS row when `None`.
        caps: Option<crate::r4::tree::VolumeCaps>,
        /// `ops`, in order (`text` is given as its UTF-8 bytes).
        ops: Vec<crate::r4::tree::TreeOp>,
    },
    /// `EnvGit` ([API §6.6]).
    EnvGit {
        /// `repo`.
        repo: String,
        /// `algo`, on first use.
        algo: Option<crate::value::Algo>,
        /// `commits`.
        commits: Vec<crate::links::EnvGitCommit>,
        /// `refs`: `refs/heads/<name>` to a git id, or `None` to delete.
        refs: Vec<(String, Option<String>)>,
        /// `heads`: tree root to HEAD.
        heads: Vec<(String, crate::links::EnvHead)>,
    },
    /// `Init` ([API §8.1]).
    Init {
        /// `seed`.
        seed: u64,
        /// `params`: `key=value`.
        params: Vec<String>,
        /// `default_branch`.
        default_branch: Option<String>,
    },
    /// `ConfigSet` ([API §8.2]).
    ConfigSet {
        /// `key`: a key instance, or a store-qualified user entry.
        key: String,
        /// `value`, in [CFG]'s syntax.
        value: String,
        /// `scope`.
        scope: FileScope,
    },
    /// `ConfigUnset` ([API §8.2]).
    ConfigUnset {
        /// `key`.
        key: String,
        /// `scope`.
        scope: FileScope,
    },
    /// `Quiet` ([API §8.3]).
    Quiet {
        /// `on`.
        on: bool,
    },
    /// `Maintain` ([API §8.4]).
    Maintain {
        /// `op`.
        op: String,
        /// `ref`, with `op` = `promote`.
        ref_: Option<String>,
    },
    /// `Mutation` ([API §9.7]): a named mutation of [LQ/std §7] by name.
    Mutation {
        /// `name`: `tx.add`, `tx.set`, … or a procedure.
        name: String,
        /// `params` by name.
        params: Vec<(String, P)>,
        /// `message`.
        message: String,
        /// `move_lease`: `--move-lease <ref>` for `tx.set` and `tx.complete` ([AR §5a.4]).
        move_lease: Option<String>,
    },
    /// `Tx` with data-level `stmts` ([API §9.1]).
    Tx {
        /// `stmts`.
        stmts: Vec<Stmt>,
        /// `message`.
        message: String,
    },
    /// `Apply` ([API §9.4]).
    Apply {
        /// `run`.
        run: Option<String>,
        /// `results`.
        results: Vec<crate::apply::ResultV1>,
        /// `stmts`.
        stmts: Vec<Stmt>,
        /// `message`.
        message: String,
    },
    /// `Schema` ([API §9.8]).
    Schema {
        /// `items`: the store-allocated members (`decl`, kind and edge ids) of an item are the store's.
        items: Vec<Item>,
        /// `message`.
        message: String,
    },
    /// `Claim` ([API §10.1]).
    Claim {
        /// `ids`.
        ids: Vec<Target>,
        /// `next`.
        next: bool,
        /// `scope`.
        scope: Option<Target>,
        /// `role`.
        role: Option<String>,
        /// `agent`.
        agent: Option<String>,
        /// `ttl`: integer milliseconds, an LQ duration text, or the text `run`.
        ttl: Option<P>,
        /// `start`.
        start: bool,
        /// `run`.
        run: Option<String>,
        /// `session`.
        session: bool,
    },
    /// `Heartbeat` ([API §10.2]).
    Heartbeat {
        /// `lease`.
        lease: String,
    },
    /// `Release` ([API §10.3]).
    Release {
        /// `lease`.
        lease: String,
    },
    /// `Reclaim` ([API §10.4]).
    Reclaim {
        /// `older_than` in ms.
        older_than_ms: Option<u64>,
        /// `run`.
        run: Option<String>,
    },
    /// `Complete` ([API §10.5]). Its `pack_digest` (the staleness notice of [RULES/pack-classes] NR-001 to NR-010)
    /// is WP-93b's; without it `changed_since_pack` is null.
    Complete {
        /// `id`.
        id: Target,
        /// `outcome`.
        outcome: String,
        /// `summary`.
        summary: String,
        /// `evidence`.
        evidence: Vec<String>,
        /// `move_lease` ([AR §5a.4]).
        move_lease: Option<String>,
    },
    /// `RunOpen` ([API §10.7]).
    RunOpen {
        /// `name`.
        name: String,
        /// Further fields: `lane`, `harness`, `model`, `wf_id`, … as (field, value).
        fields: Vec<(String, P)>,
    },
    /// `RunClose` ([API §10.7]).
    RunClose {
        /// `name`.
        name: String,
        /// `outcome`: `green`, `red`, `stopped`, `died`.
        outcome: String,
    },
    /// `BranchCreate` ([API §11.1]).
    BranchCreate {
        /// `name`.
        name: String,
        /// `from`: a ref name.
        from: Option<String>,
        /// `kind`.
        kind: Option<RefKind>,
    },
    /// `BranchDelete` ([API §11.2]).
    BranchDelete {
        /// `name`.
        name: String,
        /// `force`.
        force: bool,
    },
    /// `Checkout` ([API §11.3]).
    Checkout {
        /// `target`: a ref name, or a commit as `s<seq>`.
        target: String,
        /// `branch_new`.
        branch_new: Option<String>,
    },
    /// `WorktreeBind` ([API §11.4]).
    WorktreeBind {
        /// `dir`.
        dir: String,
        /// `ref`.
        ref_: String,
        /// `replace`.
        replace: bool,
    },
    /// `WorktreeUnbind` ([API §11.4]).
    WorktreeUnbind {
        /// `dir`.
        dir: String,
    },
    /// `LaneOpen` ([API §11.5]).
    LaneOpen {
        /// `name`: the lane's name, its ref `lane/<name>`.
        name: String,
        /// `worktree`: an absolute path, a tree ([F18 §3.5]).
        worktree: String,
        /// `git_branch`.
        git_branch: Option<String>,
        /// `base`: a git commit prefix.
        base: Option<String>,
    },
    /// `LaneClose` ([API §11.5]).
    LaneClose {
        /// `name`.
        name: String,
        /// `mode`: `close` (the default) or `freeze`.
        mode: Option<String>,
    },
    /// `Merge` ([API §11.7]).
    Merge {
        /// `src`: the source ref.
        src: String,
        /// `into`: the destination; the resolved branch when `None`.
        into: Option<String>,
        /// `policy`: `delete-wins` or `resurrect`.
        policy: Option<String>,
        /// `strict`: `merge.strict` when `None`.
        strict: Option<bool>,
        /// `base`: a revision that overrides the LCA.
        base: Option<String>,
        /// `message`.
        message: String,
    },
    /// `MergeContinue` ([API §11.8]).
    MergeContinue {
        /// `src`.
        src: Option<String>,
        /// `into`.
        into: Option<String>,
    },
    /// `MergeAbort` ([API §11.8]).
    MergeAbort {
        /// `src`.
        src: Option<String>,
        /// `into`.
        into: Option<String>,
    },
    /// `Sync` ([API §11.9]).
    Sync {
        /// `lane`: the resolved branch when `None`.
        lane: Option<String>,
        /// `check`: preview only.
        check: bool,
    },
    /// `Revert` ([API §11.10]).
    Revert {
        /// `commit`: a revision.
        commit: String,
        /// `onto`: the resolved branch when `None`.
        onto: Option<String>,
        /// `mainline`: 1 only.
        mainline: Option<u32>,
        /// `message`.
        message: String,
    },
    /// `CherryPick` ([API §11.10]).
    CherryPick {
        /// `commit`: a revision.
        commit: String,
        /// `onto`: the resolved branch when `None`.
        onto: Option<String>,
        /// `message`.
        message: String,
    },
    /// `Undo` ([API §11.11]).
    Undo {
        /// `ref`: the resolved branch when `None`.
        ref_: Option<String>,
        /// `n`: 1 when `None`.
        n: Option<u32>,
        /// `expect`: a revision the tip must be.
        expect: Option<String>,
    },
    /// `OpRestore` ([API §11.11]).
    OpRestore {
        /// `seq`.
        seq: u64,
    },
    /// `Gc` ([API §8.5]); `rollup` and `prune` are class I and change nothing the model shows.
    Gc {
        /// `reflog_expire` in ms.
        reflog_expire_ms: Option<u64>,
        /// `cruft_delay` in ms.
        cruft_delay_ms: Option<u64>,
        /// `force`.
        force: bool,
    },
    /// `FileAdd` ([API §12.2]).
    FileAdd {
        /// `paths`.
        paths: Vec<String>,
        /// `kind`: a file kind of `artifact_kind`.
        kind: Option<String>,
        /// `root`.
        root: Option<String>,
    },
    /// `LinkFile` ([API §12.3]; `tx.link_file`).
    LinkFile {
        /// `node`.
        node: Target,
        /// `specs`: anchor specs.
        specs: Vec<String>,
        /// `watch`: `header` or `span`.
        watch: Option<String>,
        /// `planned`.
        planned: bool,
        /// `quote`: a quoted text for the spec's path ([LQ/std §7.4] `$quote`).
        quote: Option<String>,
        /// `end`: the end text of a quoted range ([LQ/std §7.4] `$end`).
        end: Option<String>,
    },
    /// `UnlinkFile` ([API §12.3]; `tx.unlink_file`).
    UnlinkFile {
        /// `node`.
        node: Target,
        /// `anchor`: `aN`.
        anchor: Option<String>,
        /// `path`.
        path: Option<String>,
    },
    /// `FileMv` ([API §12.4]).
    FileMv {
        /// `srcs`.
        srcs: Vec<String>,
        /// `dst`.
        dst: String,
        /// `git`.
        git: bool,
        /// `retry_ms`.
        retry_ms: Option<u64>,
    },
    /// `FileRm` ([API §12.4]).
    FileRm {
        /// `paths`.
        paths: Vec<String>,
        /// `reason`.
        reason: Option<String>,
        /// `replaced_by`: a path or a node.
        replaced_by: Option<String>,
        /// `trash`.
        trash: bool,
        /// `recursive`.
        recursive: bool,
        /// `yes`: without it the command is a dry run of the impact.
        yes: bool,
    },
    /// `FileRevert` ([API §12.4]).
    FileRevert {
        /// `commit`: a revision whose group carried an `FsIntentDone`.
        commit: String,
    },
    /// `FileRelink` ([API §12.5]; `tx.record_move`).
    FileRelink {
        /// `from`: a path or a node.
        from: String,
        /// `to`.
        to: String,
    },
    /// `LinksFix` ([API §12.5]; `tx.links_fix`).
    LinksFix {
        /// `target`: a node or `aN`.
        target: String,
        /// `action`.
        action: String,
        /// `expect`.
        expect: Option<String>,
        /// `to`.
        to: Option<String>,
        /// `at`.
        at: Option<String>,
        /// `same_as`.
        same_as: Option<Target>,
        /// `reason`.
        reason: Option<String>,
        /// `replaced_by`.
        replaced_by: Option<String>,
        /// `from` (the `prefix` action's source directory).
        from: Option<String>,
    },
    /// `LinksSync` ([API §12.6]; `tx.links_sync`).
    LinksSync {
        /// `scope`.
        scope: Option<Target>,
        /// `budget_ms`.
        budget_ms: Option<u64>,
        /// `since`.
        since: Option<String>,
        /// `deep`.
        deep: bool,
        /// `all`.
        all: bool,
        /// `force`.
        force: bool,
    },
    /// `Check` ([API §12.7]).
    Check {
        /// `id`.
        id: Target,
    },
    /// `State` ([API §14.2]).
    State {
        /// `ref`.
        ref_: Option<String>,
        /// `at`: a commit by its seq (revision texts are WP-91's).
        at: Option<u64>,
        /// `parts`: `content`, `local`, `derived`; all when `None`.
        parts: Option<Vec<String>>,
    },
    /// `Runtime` ([API §14.3]).
    Runtime,
    /// `History` ([API §14.4]).
    History {
        /// `ref`.
        ref_: Option<String>,
        /// `since_seq`.
        since_seq: u64,
    },
}

/// A command's outcome ([API §2.3]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome {
    /// `ok`.
    Ok,
    /// `replayed`.
    Replayed,
    /// `dry`.
    Dry,
    /// `staged` ([API §2.3]): the commit landed on a staging ref; exit 6 with the success keys and `errors`.
    Staged,
    /// `refused`.
    Refused,
}

/// One LQ statement of a family-T result ([API §3.3] `statements`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StmtResult {
    /// The 1-based index.
    pub index: usize,
    /// The statement text (excluded from the comparison).
    pub text: String,
    /// The nodes matched, created or changed, ascending.
    pub targets: Vec<Nid>,
}

/// A node of the `content` snapshot ([API §15.3]), with its `local` and `derived` members ([API §15.4], §15.5).
#[derive(Clone, Debug, PartialEq)]
pub struct SnapNode {
    /// `#N`.
    pub id: Nid,
    /// The node.
    pub node: crate::state::Node,
    /// `rev`, `created`, `updated`.
    pub local: (u64, u64, u64),
    /// The derived row of a live node.
    pub derived: Option<Row>,
}

/// The data of `State` ([API §15.2]).
#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    /// The view's ref; `None` for a commit view.
    pub ref_: Option<String>,
    /// The view's commit.
    pub commit: Option<u64>,
    /// The parts `State.parts` asked for: `content`, `local`, `derived` ([API §15.2]: a part not asked for is null,
    /// and the digests are computed over all three).
    pub parts: [bool; 3],
    /// The schema items, by key.
    pub schema: Vec<(ItemKey, Item)>,
    /// Every node the view holds, by uid.
    pub nodes: Vec<SnapNode>,
}

/// A lease of the runtime snapshot ([API §15.7] `leases`).
#[derive(Clone, Debug, PartialEq)]
pub struct LeaseSnap {
    /// The row.
    pub lease: Lease,
    /// `live`.
    pub live: Live,
}

/// One entry of a result's `markers` ([API §10.8]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarkerOut {
    /// `kind`.
    pub kind: markers::MKind,
    /// `id`.
    pub id: Nid,
    /// `ref`: the origin ref's name.
    pub ref_: String,
    /// `commit`: the origin commit.
    pub commit: u64,
    /// `outcome`.
    pub outcome: Option<String>,
    /// `cause`.
    pub cause: markers::Cause,
}

/// The `data` of `LaneOpen` ([API §11.5]): `{"lane","ref","ref_id","fork","binding"}`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaneOpenData {
    /// `lane`: the lane node.
    pub lane: Nid,
    /// `ref`: `lane/<name>`.
    pub ref_: String,
    /// `ref_id`.
    pub ref_id: u32,
    /// `fork`: the lane node's commit, which the ref forks from.
    pub fork: u64,
    /// `binding`: the `WorktreeBind` data.
    pub binding: BindData,
}

/// One row of the runtime snapshot's `markers` ([API §15.7]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarkerSnap {
    /// The row.
    pub marker: markers::Marker,
    /// The origin ref's name.
    pub ref_: String,
    /// `holders` by name, bytewise.
    pub holders: Vec<String>,
    /// `active_on`: the live refs that have not absorbed the marker while it is active, bytewise.
    pub active_on: Vec<String>,
}

/// The data of `Runtime` ([API §15.7]); heads are the client heads' and bindings' ([`crate::heads`]), intents the
/// `FSINTENT` rows of the intent protocol ([API §12.4]; [`crate::links::intent`]).
#[derive(Clone, Debug, PartialEq)]
pub struct RuntimeSnap {
    /// `commit_seq`, `next_id`, `next_anchor`, `fence`, `next_ref_id`.
    pub counters: (u64, u32, u64, u64, u32),
    /// Every ref by id.
    pub refs: Vec<Ref>,
    /// The ref moves no commit carries, with their ref names, in the order they happened.
    pub moves: Vec<(String, RefMove)>,
    /// Every lease that has not ended, by (`#N`, id).
    pub leases: Vec<LeaseSnap>,
    /// Every pair (live ref, task) with `excluded`, by (ref name, `#N`), with the refs that hold it.
    pub exclusions: Vec<(String, Nid, Vec<String>)>,
    /// `refs[].absorbed`: each ref's absorbed vector as the cache keeps it (VR rows), by ref id.
    pub absorbed: BTreeMap<u32, BTreeMap<u32, u64>>,
    /// `markers`, by (`#N`, origin ref name, commit, kind).
    pub markers: Vec<MarkerSnap>,
    /// `heads`, by (kind, key text).
    pub heads: Vec<crate::heads::Head>,
    /// Every idempotency entry within its window, by key.
    pub idem: Vec<([u8; 16], Entry)>,
    /// Every allocated `#N` bound to a uid: (id, uid, ref name, create seq).
    pub alloc: Vec<(Nid, Uid, String, u64)>,
    /// `intents`: every intent of the last `gc.trash-expire` window, open ones always, in the order opened.
    pub intents: Vec<crate::links::IntentRow>,
    /// The quiet flag.
    pub quiet: bool,
}

/// A command's result data by command ([API §6]–§14).
#[derive(Clone, Debug, PartialEq)]
pub enum Data {
    /// No data (family T keeps its members in the envelope).
    None,
    /// `EnvClock`: `wall_ms`, known boot, boot number, `boot_hash`, `boot_ns`.
    Clock(i64, bool, u64, u64, u64),
    /// `EnvSlots`.
    Slots(SlotsResult),
    /// `EnvCrash`.
    Crash(bool),
    /// `EnvTree`.
    Tree(crate::links::TreeData),
    /// `EnvGit`: the repository and its commit count.
    Git(String, usize),
    /// `FileAdd`: each path with its file node and whether the command created it.
    FileAdd(Vec<(crate::value::PathVal, Nid, bool)>),
    /// `FileMv`, `FileRm`, `FileRevert`.
    Intent(Box<crate::links::intent::IntentData>),
    /// `Check`.
    Check(Box<crate::links::verbs::CheckData>),
    /// `Init`: store id, the three init-fixed values, the configuration the command set (canonical forms).
    Init([u8; 16], BTreeMap<String, u64>, BTreeMap<String, String>),
    /// `ConfigSet`, `ConfigUnset`.
    Config(Box<ConfigData>),
    /// `Quiet`.
    Quiet(bool),
    /// `Maintain`: op, ran.
    Maintain(String, bool),
    /// `Schema`: the key texts, bytewise ([API §5.5]).
    Schema(Vec<String>),
    /// `RunOpen`: run, name.
    RunOpen(Nid, String),
    /// `RunClose`: run, status, released leases.
    RunClose(Nid, String, Vec<u64>),
    /// `BranchCreate`: ref, ref id, kind, fork.
    BranchCreate(String, u32, RefKind, Option<u64>),
    /// `BranchDelete`: ref, ref id, `dropped` (commits, completions, deletions; `None` on a replay, which does not
    /// rebuild it, [API §11.2]), released leases.
    BranchDelete(String, u32, Option<(u64, u64, u64)>, Vec<u64>),
    /// `Checkout`.
    Checkout(Box<CheckoutData>),
    /// `WorktreeBind`, `WorktreeUnbind`.
    Bind(Box<BindData>),
    /// `LaneOpen` ([API §11.5]).
    LaneOpen(Box<LaneOpenData>),
    /// `LaneClose` ([API §11.5]): the lane node, its status, the unbound directory.
    LaneClose(Nid, String, Option<String>),
    /// `Gc`: reachable commits, commits that stopped resolving.
    Gc(u64, u64),
    /// `Apply`.
    Apply(Box<crate::apply::ApplyData>),
    /// `State`.
    State(Box<Snapshot>),
    /// `Runtime`.
    Runtime(Box<RuntimeSnap>),
    /// `History`: the commits by seq, and the ref moves no commit carries, in the order they happened.
    /// `History` ([API §15.8]): the commits after `since_seq`, then the ref moves no commit carries that lie after it,
    /// each with its ref and `after_seq`.
    History(Vec<Commit>, Vec<(String, RefMove, u64)>),
    /// `Merge`, `MergeContinue`, `Sync`, `Revert`, `CherryPick`.
    Merge(Box<crate::history::MergeData>),
    /// `MergeAbort`: the staging ref deleted.
    MergeAbort(String),
    /// `Undo`.
    Undo(Box<crate::history::UndoData>),
    /// `OpRestore`.
    OpRestore(Box<crate::history::RestoreData>),
}

/// A command's result: the envelope members of families T, W and X ([API §3]) as typed values.
#[derive(Clone, Debug, PartialEq)]
pub struct Reply {
    /// The outcome.
    pub outcome: Outcome,
    /// The exit code.
    pub exit: u8,
    /// `branch`.
    pub branch: Option<String>,
    /// `rev`: the tip seq before the command.
    pub rev: Option<u64>,
    /// `commit`: the created commit, or the tip read.
    pub commit: Option<u64>,
    /// `rev_new`.
    pub rev_new: Option<u64>,
    /// `key`: the explicit key.
    pub key: Option<String>,
    /// `lease`: the presented lease.
    pub lease: Option<String>,
    /// `statements`.
    pub statements: Vec<StmtResult>,
    /// `affected.ready`: the newly ready ids (§5.8).
    pub ready: Vec<Nid>,
    /// `affected.other`.
    pub other: Vec<Nid>,
    /// The created commit's net changeset (the `diff` rows of §5.7).
    pub diff: Changeset,
    /// `yields`.
    pub yields: Vec<Yield>,
    /// `warnings` (codes).
    pub warnings: Vec<String>,
    /// `markers` ([API §10.8]): the `settled`, `deleted` and `cleared` entries the command's commits and ref moves
    /// wrote, by (`#N`, origin ref name, commit, kind); family W carries them in `data.markers`.
    pub markers: Vec<MarkerOut>,
    /// The command's data.
    pub data: Data,
    /// `hints` ([F19 §8.2] row 9, §12.3): (class, text), present when non-empty; family T carries them after
    /// `yields` ([API §3.3]).
    pub hints: Vec<(String, String)>,
    /// The refusal of a `refused` outcome.
    pub error: Option<Refusal>,
}

impl Reply {
    /// A successful reply with its data and every other member empty.
    pub(crate) fn ok(data: Data) -> Reply {
        Reply {
            outcome: Outcome::Ok,
            exit: 0,
            branch: None,
            rev: None,
            commit: None,
            rev_new: None,
            key: None,
            lease: None,
            statements: Vec::new(),
            ready: Vec::new(),
            other: Vec::new(),
            diff: Changeset::new(),
            yields: Vec::new(),
            warnings: Vec::new(),
            markers: Vec::new(),
            data,
            hints: Vec::new(),
            error: None,
        }
    }

    /// A refused reply.
    pub fn refused(e: Refusal) -> Reply {
        let mut r = Reply::ok(Data::None);
        r.outcome = Outcome::Refused;
        r.exit = e.exit;
        r.error = Some(e);
        r
    }
}

/// The store-wide allocation: `#N` ↔ uid with the node's `CREATOR`, the ref it was created on and the creating seq
/// ([F11 §9]; I1, I35′).
#[derive(Clone, Debug, Default)]
pub struct AllocTable {
    /// `#N` → (uid, creator, ref name, create seq).
    pub rows: BTreeMap<Nid, (Uid, Creator, String, u64)>,
    /// uid → `#N`.
    pub uidx: BTreeMap<Uid, Nid>,
    /// `#N` → uid, for the binder.
    pub uids: BTreeMap<Nid, Uid>,
}

impl Alloc for AllocTable {
    fn uid(&self, n: Nid) -> Uid {
        self.rows
            .get(&n)
            .map(|r| r.0)
            .unwrap_or_else(|| panic!("{n} is not allocated"))
    }
    fn creator(&self, n: Nid) -> Creator {
        self.rows.get(&n).map(|r| r.1.clone()).unwrap_or_default()
    }
}

/// The store after `Init` ([API §8.1]).
#[derive(Clone, Debug)]
pub struct Inited {
    /// The seed.
    pub seed: u64,
    /// The store id ([API §17.3]).
    pub store_id: [u8; 16],
    /// The three init-fixed values.
    pub init: BTreeMap<String, u64>,
    /// The store keys `Init` set, canonical values.
    pub config: BTreeMap<String, String>,
    /// `default-branch`.
    pub default_branch: String,
    /// `idempotency.retention` and `idempotency.default-window`.
    pub windows: Windows,
}

/// The reference model's store: every part of the logical store of [API §2.4].
#[derive(Clone, Debug)]
pub struct Store {
    /// The injected environment.
    pub env: Env,
    /// Set by `Init`.
    pub inited: Option<Inited>,
    /// The kernel's configuration.
    pub cfg: KernelCfg,
    /// The simulated configuration files and the `init`-recorded values ([CFG §2.1]; [API §8.2]).
    pub conf: Conf,
    /// Commits and refs.
    pub dag: Dag,
    /// The allocation.
    pub alloc: AllocTable,
    /// `HEAD.next_id`.
    pub next_id: u32,
    /// `HEAD.next_anchor`.
    pub next_anchor: u64,
    /// `HEAD.fence`.
    pub fence: u64,
    /// `commit_seq`.
    pub commit_seq: u64,
    /// `next_ref_id`.
    pub next_ref_id: u32,
    /// `LEASES`.
    pub leases: BTreeMap<u64, Lease>,
    /// `MARKERS`, `MARKERS_OLD` and the absorbed vectors.
    pub markers: Markers,
    /// The change feed.
    pub feed: Feed,
    /// `HEADS`: client heads and bindings.
    pub heads: Heads,
    /// The commits a `Gc` dropped: they stop resolving ([API §8.5]).
    pub pruned: BTreeSet<u64>,
    /// The refs some of whose moves a `Gc` run no longer holds, with the newest move dropped ([F12 §3.5]; [API §11.11]
    /// `OpRestore`'s second E301).
    pub(crate) moves_dropped: BTreeMap<u32, crate::dag::RefMove>,
    /// `IDEM`.
    pub idem: idem::Table,
    /// The HLC sequence.
    pub hlc: Hlc,
    /// `HEAD.flags.quiet`.
    pub quiet: bool,
    /// The stream position of the running command.
    pub n: u64,
    /// `EnvCrash in-next` armed.
    pub crash_next: bool,
    /// The store without the interrupted command, kept for the harness to adopt ([API §6.7]).
    pub without: Option<Box<Store>>,
    /// For an interrupted bulk-class command, the third candidate: the store without the command but with its durable
    /// reservation, so `next_id` and `next_anchor` stand where the applied candidate leaves them ([API §6.7], §9.10).
    pub reserved: Option<Box<Store>>,
    /// The from-scratch derived rows of the live refs' tips, by commit, so a write computes its parent's rows once.
    pub(crate) rows: BTreeMap<u64, Rc<BTreeMap<Nid, Row>>>,
    /// The result data of every merge-family commit, by its seq, that a replay rebuilds ([API §7.5]).
    pub(crate) vcs_results: BTreeMap<u64, crate::history::MergeData>,
    /// `moves_back` of every `undo` `RefUpdate`, by (ref id, the move's HLC) ([API §11.11]).
    pub(crate) moves_back: BTreeMap<(u32, u64), u32>,
    /// R4's environment and runtime rows: the simulated trees and git histories, `FILEOBS`, `PENDING`, `TREES`,
    /// `FPRINT`, `PREFIXEV`, `DIRMAP`, `FSINTENT` and the anchors' `aN` ([`crate::links`]).
    pub files: crate::links::Files,
    /// Of every `op restore` `RefUpdate`, by (ref id, the move's HLC): its `restore_seq` ([F05 §9.2] field 10) and
    /// whether it deleted the ref, which a later `op restore` replays ([API §11.11]).
    pub(crate) restores: BTreeMap<(u32, u64), (u64, bool)>,
}

impl Default for Store {
    fn default() -> Store {
        Store::new()
    }
}

/// How a write through [`Store::tx`] is keyed.
pub(crate) enum Keying {
    /// By the block: the payload is `H` of its LQ form, looked up inside the write path ([API §7.3]).
    Block,
    /// By a family-W command that looked its key up already: its key, its payload `payload(c)`, the command's name
    /// and its `stmt_sym` ([API §10.7]: `stmt_origin` `verb`, no `stmt_hash`).
    Given {
        key: Option<([u8; 16], bool)>,
        payload: [u8; 16],
        cmd: &'static str,
        sym: &'static str,
    },
}

/// What a write's group does after its statements, besides them.
pub(crate) enum After {
    /// Nothing.
    Nothing,
    /// `RunClose`: every lease scoped to the run is released (reason 7, LE-004).
    RunClose(Nid),
    /// `--move-lease <ref>` ([AR §5a.4]; [API §4.3] row 3): the presented task lease moves to the explicit branch, a
    /// `Lease` record of event 3 with mask bit 1 ([F05 §9.4]) in the command's group, with the warning `lease_moved`.
    MoveLease(String),
    /// `Apply` ([API §9.4]): the verb row that admits the batch (`apply-from` with results, else `apply-batch`), and
    /// step 4's run-scoped leases to release (reason 6, LE-003); the commit's `stmt_origin` is `tx` and its
    /// `stmt_sym` `apply`.
    Apply {
        /// The `role-verbs` row.
        verb: &'static str,
        /// The run-scoped leases the batch names.
        release: Vec<u64>,
    },
}

/// The payload of a non-`TX` keyed command, over `args′` ([API §7.3]).
fn cmd_payload(name: &str, args: BTreeMap<String, Cj>) -> [u8; 16] {
    idem::payload(name, &args)
}

/// A parameter value as canonical JSON ([API §5.6]).
fn p_cj(v: &P) -> Cj {
    match v {
        P::Null => Cj::Null,
        P::Bool(b) => Cj::Bool(*b),
        P::Int(i) => Cj::Int(*i),
        P::Float(x) => Cj::F64(*x),
        P::Text(s) => Cj::Str(s.clone()),
        P::List(v) => Cj::Arr(v.iter().map(p_cj).collect()),
    }
}

/// N(m) of [F07 §5.1] with the refusals of §5.2 (`bad_value`, exit 2): U+0000; CR LF and every remaining CR become
/// LF; the bytes HT, VT, FF and SP are stripped from the end of every line; every trailing LF is removed; the result is
/// at most 65,535 bytes and its last paragraph does not begin with `Moirai-`. The empty result is the absent message.
// spec: [F07 §5.1]
// spec: [F07 §5.2]
pub fn normalize_message(m: &str) -> Res<String> {
    crate::canon::normalise_message(m)
}

impl Store {
    /// A stream at its start: the initial environment and no store.
    pub fn new() -> Store {
        Store {
            env: Env::default(),
            inited: None,
            cfg: KernelCfg::default(),
            conf: Conf::default(),
            dag: Dag::default(),
            alloc: AllocTable::default(),
            next_id: 1,
            next_anchor: 1,
            fence: 0,
            commit_seq: 0,
            next_ref_id: 0,
            leases: BTreeMap::new(),
            markers: Markers::default(),
            feed: Feed::default(),
            heads: Heads::default(),
            pruned: BTreeSet::new(),
            moves_dropped: BTreeMap::new(),
            idem: idem::Table::default(),
            hlc: Hlc::default(),
            quiet: false,
            n: 0,
            crash_next: false,
            without: None,
            reserved: None,
            rows: BTreeMap::new(),
            vcs_results: BTreeMap::new(),
            moves_back: BTreeMap::new(),
            restores: BTreeMap::new(),
            files: crate::links::Files::default(),
        }
    }

    /// Runs one command of the stream ([API §2.1]) and returns its result.
    pub fn run(&mut self, cmd: &Cmd, ctx: &Ctx) -> Reply {
        self.n += 1;
        let write = !matches!(
            cmd,
            Cmd::EnvClock(_)
                | Cmd::EnvSlots(_)
                | Cmd::EnvCrash { .. }
                | Cmd::EnvTree { .. }
                | Cmd::EnvGit { .. }
                | Cmd::State { .. }
                | Cmd::Runtime
                | Cmd::History { .. }
                | Cmd::Sync { check: true, .. }
        );
        if write && self.crash_next {
            // EnvCrash in-next: the next write command is applied, its result is `outcome_unknown`, and the store
            // without it is kept for the harness to adopt ([API §6.7]).
            self.crash_next = false;
            let mut before = self.clone();
            before.without = None;
            before.reserved = None;
            let bulk = bulk_class(cmd, ctx) || self.moves_a_directory(cmd, ctx);
            let _ = self.dispatch(cmd, ctx);
            self.reserved = bulk.then(|| {
                let mut r = before.clone();
                r.next_id = self.next_id;
                r.next_anchor = self.next_anchor;
                Box::new(r)
            });
            self.without = Some(Box::new(before));
            return Reply::refused(
                Refusal::new(
                    "outcome_unknown",
                    7,
                    "outcome unknown: re-run with the same key or check moirai changes",
                )
                .key("key", ctx.key.clone()),
            );
        }
        match self.dispatch(cmd, ctx) {
            Ok(r) => r,
            Err(e) => Reply::refused(e),
        }
    }

    /// Adopts the candidate without the command an `EnvCrash in-next` interrupted ([API §6.7]).
    pub fn adopt_without(&mut self) {
        if let Some(w) = self.without.take() {
            let n = self.n;
            *self = *w;
            self.n = n;
        }
    }

    /// Adopts the candidate of an interrupted bulk-class command whose durable reservation survived without its commit
    /// ([API §6.7]): the reserved `#N`s and `aN`s are skipped for good ([F11 §9.1]); nothing else of the command
    /// applies. Without such a candidate this is [`Store::adopt_without`].
    pub fn adopt_reserved(&mut self) {
        match self.reserved.take() {
            Some(r) => {
                let n = self.n;
                *self = *r;
                self.n = n;
            }
            None => self.adopt_without(),
        }
    }

    fn dispatch(&mut self, cmd: &Cmd, ctx: &Ctx) -> Res<Reply> {
        match cmd {
            Cmd::EnvClock(a) => {
                self.env.clock(a);
                Ok(Reply::ok(Data::Clock(
                    self.env.wall_ms,
                    !self.env.unknown,
                    self.env.boot_no,
                    self.env.boot_hash(),
                    self.env.boot_ns,
                )))
            }
            Cmd::EnvSlots(a) => Ok(Reply::ok(Data::Slots(self.env.slots(a)))),
            Cmd::EnvTree {
                tree,
                volume,
                caps,
                ops,
            } => self.env_tree(tree, volume.as_deref(), *caps, ops),
            Cmd::EnvGit {
                repo,
                algo,
                commits,
                refs,
                heads,
            } => self.env_git(repo, *algo, commits, refs, heads),
            Cmd::EnvCrash { in_next } => {
                if *in_next {
                    self.crash_next = true;
                } else {
                    self.env.crash();
                }
                Ok(Reply::ok(Data::Crash(*in_next)))
            }
            Cmd::Init {
                seed,
                params,
                default_branch,
            } => self.init(*seed, params, default_branch.as_deref()),
            _ if self.inited.is_none() => Err(Refusal::usage("the first store command is Init")),
            Cmd::ConfigSet { key, value, scope } => self.config_set(key, value, *scope),
            Cmd::ConfigUnset { key, scope } => self.config_unset(key, *scope),
            Cmd::Quiet { on } => {
                let caller = self.resolve(ctx, false)?;
                self.rights(&caller, ctx).verb("store-admin")?;
                self.quiet = *on;
                let mut r = Reply::ok(Data::Quiet(*on));
                r.warnings = caller.warnings;
                Ok(r)
            }
            Cmd::Maintain { op, ref_ } => {
                if !["checkpoint", "runtime-fold", "fold", "rollup", "promote"]
                    .contains(&op.as_str())
                {
                    return Err(Refusal::usage_arg(
                        "op",
                        format!("unknown maintenance op {op}"),
                    ));
                }
                match (op.as_str(), ref_) {
                    ("promote", None) => {
                        return Err(Refusal::usage_arg("ref", "promote needs a ref"));
                    }
                    ("promote", Some(r)) if self.dag.live(r).is_none() => {
                        return Err(Refusal::lq("E301", format!("no ref {r}")));
                    }
                    ("promote", Some(_)) => {}
                    (_, Some(_)) => {
                        return Err(Refusal::usage_arg("ref", "ref goes with promote only"));
                    }
                    _ => {}
                }
                // Class I: the model executes nothing ([API §8.4]).
                Ok(Reply::ok(Data::Maintain(op.clone(), false)))
            }
            Cmd::Mutation {
                name,
                params,
                message,
                move_lease,
            } => self.mutation(name, params, message, move_lease.as_deref(), ctx),
            Cmd::Tx { stmts, message } => self.tx(
                stmts,
                message,
                ctx,
                None,
                None,
                Keying::Block,
                After::Nothing,
            ),
            Cmd::Schema { items, message } => self.schema(items, message, ctx),
            Cmd::Apply {
                run,
                results,
                stmts,
                message,
            } => self.apply(run.as_deref(), results, stmts, message, ctx),
            Cmd::Claim {
                ids,
                next,
                scope,
                role,
                agent,
                ttl,
                start,
                run,
                session,
            } => {
                let mut args = Vec::new();
                if !ids.is_empty() {
                    args.push((
                        "ids".to_string(),
                        P::List(ids.iter().map(|t| P::Text(target_text(t))).collect()),
                    ));
                }
                if *next {
                    args.push(("next".into(), P::Bool(true)));
                }
                if let Some(s) = scope {
                    args.push(("scope".into(), P::Text(target_text(s))));
                }
                for (k, v) in [("role", role), ("agent", agent)] {
                    if let Some(v) = v {
                        args.push((k.into(), P::Text(v.clone())));
                    }
                }
                if let Some(t) = ttl {
                    args.push(("ttl".into(), t.clone()));
                }
                if *start {
                    args.push(("start".into(), P::Bool(true)));
                }
                if let Some(r) = run {
                    args.push(("run".into(), P::Text(r.clone())));
                }
                if *session {
                    args.push(("session".into(), P::Bool(true)));
                }
                self.procedure("tx.claim", args, ctx)
            }
            Cmd::Heartbeat { lease } => self.procedure(
                "tx.heartbeat",
                vec![("lease".into(), P::Text(lease.clone()))],
                ctx,
            ),
            Cmd::Release { lease } => self.procedure(
                "tx.release",
                vec![("lease".into(), P::Text(lease.clone()))],
                ctx,
            ),
            Cmd::Reclaim { older_than_ms, run } => {
                let mut args = Vec::new();
                if let Some(o) = older_than_ms {
                    args.push(("older_than".into(), P::Int(*o as i64)));
                }
                if let Some(r) = run {
                    args.push(("run".into(), P::Text(r.clone())));
                }
                self.procedure("tx.reclaim", args, ctx)
            }
            Cmd::Complete {
                id,
                outcome,
                summary,
                evidence,
                move_lease,
            } => {
                let mut args = vec![
                    ("id".to_string(), P::Text(target_text(id))),
                    ("outcome".into(), P::Text(outcome.clone())),
                    ("summary".into(), P::Text(summary.clone())),
                ];
                if !evidence.is_empty() {
                    args.push((
                        "evidence".into(),
                        P::List(evidence.iter().map(|e| P::Text(e.clone())).collect()),
                    ));
                }
                self.procedure_moving("tx.complete", args, move_lease.as_deref(), ctx)
            }
            Cmd::RunOpen { name, fields } => self.run_open(name, fields, ctx),
            Cmd::RunClose { name, outcome } => self.run_close(name, outcome, ctx),
            Cmd::BranchCreate { name, from, kind } => {
                self.branch_create(name, from.as_deref(), *kind, ctx)
            }
            Cmd::BranchDelete { name, force } => self.branch_delete(name, *force, ctx),
            Cmd::Checkout { target, branch_new } => {
                self.checkout(target, branch_new.as_deref(), ctx)
            }
            Cmd::WorktreeBind { dir, ref_, replace } => {
                self.worktree_bind(dir, ref_, *replace, ctx)
            }
            Cmd::WorktreeUnbind { dir } => self.worktree_unbind(dir, ctx),
            Cmd::LaneOpen {
                name,
                worktree,
                git_branch,
                base,
            } => self.lane_open(name, worktree, git_branch.as_deref(), base.as_deref(), ctx),
            Cmd::LaneClose { name, mode } => self.lane_close(name, mode.as_deref(), ctx),
            Cmd::Merge {
                src,
                into,
                policy,
                strict,
                base,
                message,
            } => self.merge_cmd(
                src,
                into.as_deref(),
                policy.as_deref(),
                *strict,
                base.as_deref(),
                message,
                ctx,
            ),
            Cmd::MergeContinue { src, into } => {
                self.merge_continue(src.as_deref(), into.as_deref(), ctx)
            }
            Cmd::MergeAbort { src, into } => self.merge_abort(src.as_deref(), into.as_deref(), ctx),
            Cmd::Sync { lane, check } => self.sync_cmd(lane.as_deref(), *check, ctx),
            Cmd::Revert {
                commit,
                onto,
                mainline,
                message,
            } => self.pick_cmd(commit, onto.as_deref(), *mainline, message, true, ctx),
            Cmd::CherryPick {
                commit,
                onto,
                message,
            } => self.pick_cmd(commit, onto.as_deref(), None, message, false, ctx),
            Cmd::Undo { ref_, n, expect } => {
                self.undo_cmd(ref_.as_deref(), n.unwrap_or(1), expect.as_deref(), ctx)
            }
            Cmd::OpRestore { seq } => self.op_restore_cmd(*seq, ctx),
            Cmd::FileAdd { paths, kind, root } => {
                self.file_add(paths, kind.as_deref(), root.as_deref(), ctx)
            }
            Cmd::LinkFile {
                node,
                specs,
                watch,
                planned,
                quote,
                end,
            } => self.link_file(
                node,
                specs,
                watch.as_deref(),
                *planned,
                (quote.as_deref(), end.as_deref()),
                ctx,
            ),
            Cmd::UnlinkFile { node, anchor, path } => {
                self.unlink_file(node, anchor.as_deref(), path.as_deref(), ctx)
            }
            Cmd::FileMv {
                srcs,
                dst,
                git,
                retry_ms,
            } => self.file_mv(srcs, dst, *git, *retry_ms, ctx),
            Cmd::FileRm {
                paths,
                reason,
                replaced_by,
                trash,
                recursive,
                yes,
            } => self.file_rm(
                paths,
                reason.as_deref(),
                replaced_by.as_deref(),
                (*trash, *recursive, *yes),
                ctx,
            ),
            Cmd::FileRevert { commit } => self.file_revert(commit, ctx),
            Cmd::FileRelink { from, to } => self.file_relink(from, to, ctx),
            Cmd::LinksFix {
                target,
                action,
                expect,
                to,
                at,
                same_as,
                reason,
                replaced_by,
                from,
            } => self.links_fix(
                &crate::links::fix::FixArgs {
                    target: target.clone(),
                    action: action.clone(),
                    expect: expect.clone(),
                    to: to.clone(),
                    at: at.clone(),
                    same_as: same_as.clone(),
                    reason: reason.clone(),
                    replaced_by: replaced_by.clone(),
                    from: from.clone(),
                },
                ctx,
            ),
            Cmd::LinksSync {
                scope,
                budget_ms,
                since,
                deep,
                all,
                force,
            } => self.links_sync(
                scope.as_ref(),
                *budget_ms,
                since.as_deref(),
                (*deep, *all, *force),
                ctx,
            ),
            Cmd::Check { id } => self.check(id, ctx),
            Cmd::Gc {
                reflog_expire_ms,
                cruft_delay_ms,
                force,
            } => self.gc(*reflog_expire_ms, *cruft_delay_ms, *force),
            Cmd::State { ref_, at, parts } => {
                let caller = self.resolve(ctx, false)?;
                let mut flags = [parts.is_none(); 3];
                for p in parts.iter().flatten() {
                    let i = ["content", "local", "derived"]
                        .iter()
                        .position(|x| x == p)
                        .ok_or_else(|| Refusal::usage_arg("parts", format!("{p} is not a part")))?;
                    flags[i] = true;
                }
                let (name, commit) = match (ref_, at) {
                    (Some(_), Some(_)) => {
                        return Err(Refusal::usage("State takes ref or at, not both"));
                    }
                    (_, Some(c)) => {
                        if !self.dag.commits.contains_key(c) || self.pruned.contains(c) {
                            return Err(Refusal::lq("E301", format!("no commit s{c}")));
                        }
                        (None, Some(*c))
                    }
                    (r, None) => {
                        let name = r.clone().unwrap_or(caller.branch);
                        let tip = self
                            .dag
                            .live(&name)
                            .ok_or_else(|| Refusal::lq("E301", format!("no ref {name}")))?
                            .tip;
                        (Some(name), tip)
                    }
                };
                let mut snap = self.snapshot(name.as_deref().unwrap_or(""), commit);
                snap.ref_ = name.clone();
                snap.parts = flags;
                let mut reply = Reply::ok(Data::State(Box::new(snap)));
                reply.rev = Some(commit.unwrap_or(0));
                reply.branch = name;
                Ok(reply)
            }
            Cmd::Runtime => Ok(Reply::ok(Data::Runtime(Box::new(self.runtime())))),
            Cmd::History { ref_, since_seq } => {
                let reach: BTreeSet<u64> = match ref_ {
                    Some(r) => {
                        let x = self
                            .dag
                            .live(r)
                            .ok_or_else(|| Refusal::lq("E301", format!("no ref {r}")))?;
                        self.dag.ancestors(x.tip)
                    }
                    None => self.dag.commits.keys().copied().collect(),
                };
                let commits = self
                    .dag
                    .commits
                    .range(since_seq + 1..)
                    .filter(|(s, _)| reach.contains(s) && !self.pruned.contains(s))
                    .map(|(_, c)| c.clone())
                    .collect();
                // [API §15.8]: the moves that lie after commit `since_seq` ([F05 §9.2]), each with `after_seq`, the seq
                // of the newest commit appended before it.
                let after = |m: &RefMove| {
                    self.dag
                        .commits
                        .values()
                        .filter(|c| c.append_hlc < m.hlc)
                        .map(|c| c.seq)
                        .max()
                        .unwrap_or(0)
                };
                let moves = self
                    .dag
                    .moves()
                    .into_iter()
                    .filter(|(n, _)| ref_.as_ref().is_none_or(|r| r == n))
                    .map(|(n, m)| {
                        let a = after(&m);
                        (n, m, a)
                    })
                    .filter(|(_, _, a)| *a >= *since_seq)
                    .collect();
                Ok(Reply::ok(Data::History(commits, moves)))
            }
        }
    }

    /// `Init` ([API §8.1]): the store id of §17.3, the init-fixed values, the validated configuration ([CFG §7.6];
    /// [`config`]), `main` with no commit.
    // spec: [API §8.1]
    // spec: [API §17.3]
    fn init(&mut self, seed: u64, params: &[String], default_branch: Option<&str>) -> Res<Reply> {
        if self.inited.is_some() {
            return Err(Refusal::usage("a stream holds one Init"));
        }
        let vals = config::parse_init(params, default_branch)?;
        let mut t: u32 = 0;
        let store_id = loop {
            let id = blake3_128(&[
                b"moirai-api-store-id-v1",
                &seed.to_le_bytes(),
                &t.to_le_bytes(),
            ]);
            if id != [0; 16] {
                break id;
            }
            t += 1;
        };
        let init: BTreeMap<String, u64> = registry::DEFS
            .iter()
            .filter(|d| config::is_init(d.key))
            .map(|d| (d.key.to_string(), config::number(&vals, d.key)))
            .collect();
        let config: BTreeMap<String, String> = vals
            .iter()
            .filter(|(k, _)| !config::is_init(k))
            .map(|(k, p)| (k.clone(), p.canonical.clone()))
            .collect();
        self.conf = Conf {
            store: config.clone(),
            user: BTreeMap::new(),
            init: registry::DEFS
                .iter()
                .filter(|d| config::is_init(d.key))
                .map(|d| {
                    let v = vals.get(d.key).map_or_else(
                        || registry::default_text(d.key, registry::Proc::Cli).unwrap_or_default(),
                        |p| p.canonical.clone(),
                    );
                    (d.key.to_string(), v)
                })
                .collect(),
            store_id: crate::value::hex(&store_id),
        };
        self.inited = Some(Inited {
            seed,
            store_id,
            init: init.clone(),
            config: config.clone(),
            default_branch: "main".into(),
            windows: Windows::default(),
        });
        self.refresh_cfg();
        self.dag.refs.insert(
            0,
            Ref {
                id: 0,
                name: "main".into(),
                kind: RefKind::Work,
                tip: None,
                ref_seq_next: 1,
                fork: None,
                deleted: false,
                message: None,
                pinned: false,
                moves: Vec::new(),
            },
        );
        self.next_ref_id = 1;
        let mut r = Reply::ok(Data::Init(store_id, init, config));
        r.branch = Some("main".into());
        r.rev = Some(0);
        Ok(r)
    }

    /// The env variable of a name, empty counted as unset ([CFG §5.1] rule 2).
    fn env_var<'a>(ctx: &'a Ctx, k: &str) -> Option<&'a str> {
        ctx.env.get(k).map(String::as_str).filter(|v| !v.is_empty())
    }

    /// The caller context resolution of [API §4.2] (CX-1 to CX-9, CX-2 in its order of record through the heads and
    /// bindings, [`crate::context`]) with the refusals of §4.3 rows 1–4; row 6 (a detached head) is the write path's
    /// (`writable`), row 8 (quiet mode) `Gc`'s, row 5 (`tree_mismatch`) the tree-derived writes' of group F
    /// ([`Store::tree_mismatch`]). CX-2's git-worktree hint reads the simulated trees and git histories of `EnvTree`
    /// and `EnvGit` ([API §6.5], §6.6).
    // spec: [API §4.2]
    // spec: [API §4.3]
    // rule: WR-002, WR-003, WR-004, WR-005
    pub fn resolve(&self, ctx: &Ctx, claim: bool) -> Res<Caller> {
        self.resolve_moving(ctx, claim, None)
    }

    /// [`Store::resolve`] for a command with `move_lease` ([API §4.3] row 3): an explicit branch that differs from the
    /// presented task lease's is allowed when `move_lease` names it, with the warning `lease_moved`.
    pub fn resolve_moving(&self, ctx: &Ctx, claim: bool, move_lease: Option<&str>) -> Res<Caller> {
        let mut c = self.resolve_keyed(ctx, claim, move_lease)?;
        match c.pending.take() {
            Some(e) => Err(e),
            None => Ok(c),
        }
    }

    /// The caller of a keyed command: [`Store::resolve_moving`] with §4.3 rows 1–4 held in `pending`, so that the
    /// idempotency lookup runs first ([API §4.3] "The idempotency pre-check comes first", §7.4); for that lookup the
    /// branch (CX-2) takes an ended lease's recorded branch. [`Store::keyed`] raises `pending` when the lookup finds no
    /// entry.
    // spec: [API §4.3]
    pub(crate) fn resolve_keyed(
        &self,
        ctx: &Ctx,
        claim: bool,
        move_lease: Option<&str>,
    ) -> Res<Caller> {
        let mut pending: Option<Refusal> = None;
        let mut warnings = Vec::new();
        // CX-7: the client profile: `ctx.client`, then `MOIRAI_CLIENT`, then `client.profile` when it is not `auto`
        // ([CFG §10.9], class V), then `client_info`; the harness detection decides only without all four, and only
        // then do two harnesses' variables give no session identity.
        let claude_env = Self::env_var(ctx, "CLAUDECODE").is_some()
            || Self::env_var(ctx, "CLAUDE_CODE_SESSION_ID").is_some()
            || Self::env_var(ctx, "AI_AGENT").is_some_and(|v| v.starts_with("claude-code"));
        let codex_env = Self::env_var(ctx, "CODEX_THREAD_ID").is_some();
        let other_env = ["GEMINI_CLI", "CURSOR_AGENT", "AGENT"]
            .iter()
            .any(|k| Self::env_var(ctx, k).is_some());
        let harnesses = [claude_env, codex_env, other_env]
            .iter()
            .filter(|x| **x)
            .count();
        let mut two = false;
        let configured = self.conf.text("client.profile");
        let client: &'static str = match ctx
            .client
            .as_deref()
            .or(Self::env_var(ctx, "MOIRAI_CLIENT"))
            .or(Some(configured.as_str()).filter(|p| !p.is_empty() && *p != "auto"))
        {
            Some("claude") => "claude",
            Some("codex") => "codex",
            Some(_) => "generic",
            None => match ctx.client_info.as_deref() {
                Some("codex-mcp-client") => "codex",
                Some(c) if c.to_ascii_lowercase().contains("claude") => "claude",
                Some(_) => "generic",
                None if harnesses > 1 => {
                    two = true;
                    warnings.push("two_harnesses".to_string());
                    "generic"
                }
                None if claude_env => "claude",
                None if codex_env => "codex",
                None => "generic",
            },
        };
        // CX-4: the session identity; never a MOIRAI_* variable; none when the detection met two harnesses.
        let session = if two {
            None
        } else if let Some(t) = ctx.meta.as_ref().and_then(|m| m.thread_id.clone()) {
            Some(format!("codex:{t}"))
        } else if let Some(s) = ctx.stamp.as_ref().and_then(|s| s.session_id.clone()) {
            Some(format!(
                "{}:{s}",
                if client == "codex" { "codex" } else { "claude" }
            ))
        } else if let Some(t) = Self::env_var(ctx, "CODEX_THREAD_ID") {
            Some(format!("codex:{t}"))
        } else {
            Self::env_var(ctx, "CLAUDE_CODE_SESSION_ID").map(|s| format!("claude:{s}"))
        };
        // The attested thread of CX-9.
        let thread = ctx
            .meta
            .as_ref()
            .and_then(|m| m.thread_id.clone())
            .or_else(|| Self::env_var(ctx, "CODEX_THREAD_ID").map(str::to_string))
            .map(|t| format!("codex:{t}"));
        // CX-1: the presented lease.
        let (lease_text, env_lease) = match (&ctx.lease, Self::env_var(ctx, "MOIRAI_LEASE")) {
            (Some(l), _) => (Some(l.clone()), false),
            (None, Some(l)) => (Some(l.to_string()), true),
            _ => (None, false),
        };
        // §4.3 rows 1 and 2 are held for the lookup; a lease that has ended still names its branch for it.
        let mut ended_branch: Option<String> = None;
        let mut ended_lease: Option<u64> = None;
        let lease = match lease_text {
            None => None,
            Some(t) => match tx::parse_lease(&t) {
                None => {
                    pending = Some(Refusal::e407(
                        Some(t.clone()),
                        None,
                        format!("{t} is not a lease"),
                    ));
                    None
                }
                Some(id) => {
                    let row = self.leases.get(&id);
                    match row.filter(|l| lease::is_live(l, &self.env).is_live()) {
                        None => {
                            pending = Some(Refusal::e407(
                                Some(t.clone()),
                                row.map(|l| l.holder.clone()),
                                format!("lease {t} is lost"),
                            ));
                            ended_branch =
                                row.filter(|l| !l.session_role).map(|l| l.branch.clone());
                            ended_lease = row.map(|l| l.id);
                            None
                        }
                        // §4.3 row 2: an environment lease bound to another thread.
                        Some(l)
                            if env_lease
                                && matches!((l.bound, &thread), (Some(b), Some(th)) if b != blake3_128(&[th.as_bytes()])) =>
                        {
                            pending = Some(Refusal::e407(
                                Some(t.clone()),
                                Some(l.holder.clone()),
                                format!("{t} is bound to another thread; pass your own lease"),
                            ));
                            None
                        }
                        Some(l) => Some(l.clone()),
                    }
                }
            },
        };
        // CX-2: the branch, in the order of record ([`crate::context::CX2`]).
        let lease_branch = lease
            .as_ref()
            .filter(|l| !l.session_role)
            .map(|l| l.branch.clone())
            .or(ended_branch);
        let marker_branch = ctx.marker.as_deref().and_then(|m| {
            m.strip_prefix("moirai:")?
                .split(' ')
                .find_map(|f| f.strip_prefix("branch="))
                .map(str::to_string)
        });
        let default = self
            .inited
            .as_ref()
            .map_or("main".to_string(), |i| i.default_branch.clone());
        let sandbox = ctx
            .meta
            .as_ref()
            .and_then(|m| m.sandbox_cwd.clone())
            .or_else(|| ctx.stamp.as_ref().and_then(|x| x.cwd.clone()));
        let inputs = context::BranchInputs {
            explicit: ctx.branch.clone(),
            lease: lease_branch.clone(),
            sandbox_cwd: sandbox,
            env: Self::env_var(ctx, "MOIRAI_BRANCH").map(str::to_string),
            marker: marker_branch,
            client: ctx
                .client
                .clone()
                .or_else(|| Self::env_var(ctx, "MOIRAI_CLIENT").map(str::to_string)),
            cwd: ctx.cwd.clone(),
            git_top: ctx.cwd.as_deref().and_then(|c| {
                let root = self.files.tree_of(&crate::links::canon_abs(c))?;
                self.files.git.of_tree(&root).map(|_| root)
            }),
            session_key: session
                .as_ref()
                .filter(|_| ctx.door == Door::Mcp)
                .map(|s| format!("session:{s}")),
            default_branch: default,
        };
        let (branch, detached) = match context::resolve_branch(&self.heads, &inputs) {
            crate::heads::Target::Ref(r) => (r, None),
            crate::heads::Target::Detached(c) => (String::new(), Some(c)),
        };
        // §4.3 row 3: a task lease or run-scoped role lease fixes the branch, unless `move_lease` names the explicit
        // branch, which moves the lease there with a warning.
        if let (Some(lb), Some(b), Some(l)) = (&lease_branch, &ctx.branch, &lease)
            && lb != b
        {
            if move_lease == Some(b.as_str()) {
                warnings.push("lease_moved".to_string());
            } else if pending.is_none() {
                pending = Some(Refusal::e407(
                    Some(format!("L-{}", l.id)),
                    Some(l.holder.clone()),
                    format!("--branch {b} differs from lease L-{}'s branch {lb}", l.id),
                ));
            }
        }
        // CX-3: the actor and actor_src.
        let (actor, actor_src) = if let Some(l) = &lease {
            (l.holder.clone(), "lease")
        } else if let Some(t) = ctx.meta.as_ref().and_then(|m| m.thread_id.clone()) {
            (format!("codex:{t}"), "meta")
        } else if let Some(a) = ctx.stamp.as_ref().and_then(|s| s.agent_id.clone()) {
            (format!("claude:{a}"), "stamp")
        } else if let Some(a) = &ctx.agent {
            (a.clone(), "declared")
        } else if let Some(a) = Self::env_var(ctx, "MOIRAI_AGENT") {
            (a.to_string(), "env")
        } else if let Some(t) = Self::env_var(ctx, "CODEX_THREAD_ID") {
            (format!("codex:{t}"), "env")
        } else if let Some(s) = Self::env_var(ctx, "CLAUDE_CODE_SESSION_ID") {
            (format!("session:claude:{s}"), "env")
        } else if let Some(c) = &ctx.client_info {
            (format!("client:{c}"), "client")
        } else if let Some(s) = &session {
            (format!("session:{s}"), "none")
        } else {
            (String::new(), "none")
        };
        // §4.3 row 4: outside Claim, a declared agent that differs from the lease's holder.
        if !claim
            && pending.is_none()
            && let (Some(l), Some(a)) = (&lease, &ctx.agent)
            && *a != l.holder
        {
            pending = Some(Refusal::e407(
                Some(format!("L-{}", l.id)),
                Some(l.holder.clone()),
                format!(
                    "declared agent {a} differs from lease L-{}'s holder {}",
                    l.id, l.holder
                ),
            ));
        }
        // CX-8: the effective role and the narrowing label.
        let presented = lease.as_ref().map(|l| Presented {
            id: l.id,
            task: l.task,
            run: l.run,
            role: l.role.clone(),
            holder: l.holder.clone(),
            session_role: l.session_role,
        });
        let role = policy::effective_role(presented.as_ref(), false);
        let label = policy::narrowing_label(&role, lease.is_some(), ctx.hook_label.as_deref());
        if label.is_some() {
            warnings.push("hook_label_narrowed".to_string());
        }
        // CX-5 and CX-6 read the lane nodes of `main` and the run nodes of the view.
        let main_tip = self.dag.live("main").and_then(|r| r.tip);
        let view_tip = if branch.is_empty() {
            detached
        } else {
            self.dag.live(&branch).and_then(|r| r.tip)
        };
        let lane_tree = lease_branch.as_deref().and_then(|b| {
            let main = self.dag.state_at(main_tip, &self.alloc);
            main.nodes
                .values()
                .find(|x| x.live() && x.kind == "lane" && x.text("moirai_branch") == Some(b))
                .and_then(|x| match x.fields.get("worktree_path") {
                    // A `path` value under root `abs` ([F08 §9.3] `lane`).
                    Some(crate::value::Value::Path(p)) => Some(p.text.clone()),
                    _ => None,
                })
        });
        let tree = context::resolve_tree(
            ctx.tree.as_deref(),
            ctx.meta.as_ref().and_then(|m| m.sandbox_cwd.as_deref()),
            ctx.stamp.as_ref().and_then(|x| x.cwd.as_deref()),
            lane_tree.as_deref(),
            ctx.cwd.as_deref(),
        );
        let run_model = {
            let view = self.dag.state_at(view_tip, &self.alloc);
            lease
                .as_ref()
                .and_then(|l| l.run)
                .and_then(|r| view.nodes.get(&r).cloned())
                .or_else(|| {
                    let name = Self::env_var(ctx, "MOIRAI_RUN")?;
                    view.nodes
                        .values()
                        .find(|x| x.live() && x.kind == "run" && x.text("title") == Some(name))
                        .cloned()
                })
                .and_then(|x| x.text("model").map(str::to_string))
        };
        let model = context::resolve_model(
            run_model.as_deref(),
            ctx.marker.as_deref(),
            ctx.model.as_deref().or(Self::env_var(ctx, "MOIRAI_MODEL")),
            None,
            ctx.hook_model.as_deref(),
        );
        let profile = crate::profile::model_profile(&self.conf, model.as_deref(), client);
        Ok(Caller {
            lease: lease.map(|l| l.id),
            env_lease,
            branch,
            detached,
            tree,
            model,
            profile,
            actor,
            actor_src,
            session,
            client,
            role,
            label,
            thread,
            warnings,
            pending,
            ended_lease,
        })
    }

    pub(crate) fn rights(&self, c: &Caller, ctx: &Ctx) -> Rights {
        let presented = c
            .lease
            .and_then(|id| self.leases.get(&id))
            .map(|l| Presented {
                id: l.id,
                task: l.task,
                run: l.run,
                role: l.role.clone(),
                holder: l.holder.clone(),
                session_role: l.session_role,
            });
        Rights {
            role: c.role.clone(),
            label: c.label.clone(),
            surface: if ctx.door == Door::Mcp {
                Surface::Mcp
            } else {
                Surface::Cli
            },
            lease: presented,
            actor: Some(c.actor.clone()).filter(|a| !a.is_empty()),
            owner_attested: false,
            acceptor: None,
            // The policy data of the caller's view: its `policy` items over the defaults ([CFG §10.13]).
            data: PolicyData::of(
                &self
                    .dag
                    .state_at(self.dag.live(&c.branch).and_then(|r| r.tip), &self.alloc)
                    .schema,
            ),
            confirm_roles: crate::links::confirm_rights(&self.conf),
        }
    }

    /// §4.3 row 7: the ref the command writes must be writable — not a tag, an `import/*` or `orphans/*` ref, and a
    /// staging `merge/*` ref only for `RESOLVE` ([50 §3.9] item 6; [RULES/status-machines] BM-003).
    pub(crate) fn writable(&self, branch: &str, resolve_only: bool) -> Res<RefKind> {
        if branch.is_empty() {
            // §4.3 row 6: a detached client head is read-only; a write needs `branch_new`.
            return Err(Refusal::lq(
                "E305",
                "the view is a detached client head: read-only; check out a branch or use branch_new",
            ));
        }
        let r = self
            .dag
            .live(branch)
            .ok_or_else(|| Refusal::lq("E301", format!("no ref {branch}")))?;
        match r.kind {
            RefKind::Tag | RefKind::Import | RefKind::Orphans => {
                Err(Refusal::lq("E305", format!("{branch} is a read-only view")))
            }
            RefKind::Merge if !resolve_only => Err(Refusal::lq(
                "E305",
                format!("{branch} is a staging ref: read-only except RESOLVE"),
            )),
            k => Ok(k),
        }
    }

    pub(crate) fn windows(&self) -> Windows {
        self.inited.as_ref().map(|i| i.windows).unwrap_or_default()
    }

    /// The key of a command that takes an explicit key only ([API §7.1]: `LinksSync`, `Sync`, `Merge`, `MergeContinue`
    /// and `Undo`, whose effect depends on tips their arguments do not name).
    // spec: [API §7.1]
    pub(crate) fn explicit_key(ctx: &Ctx) -> Option<([u8; 16], bool)> {
        ctx.key.as_ref().map(|k| (idem::explicit_key(k), false))
    }

    /// The idempotency key of a keyed command ([API §7.1], §7.2): the explicit key, else the default key unless
    /// `no_dedupe`. The default key hashes s (the session), a (`codex:` + `ctx.meta.threadId`, else `claude:` +
    /// `ctx.stamp.agent_id`, else the resolved actor, CX-3), b (`branch`, the command's branch, §7.4) and the payload.
    // spec: [API §7.2]
    pub(crate) fn key_of(
        &self,
        ctx: &Ctx,
        c: &Caller,
        branch: &str,
        payload: &[u8; 16],
    ) -> Option<([u8; 16], bool)> {
        if let Some(k) = &ctx.key {
            return Some((idem::explicit_key(k), false));
        }
        if ctx.no_dedupe {
            return None;
        }
        let a = ctx
            .meta
            .as_ref()
            .and_then(|m| m.thread_id.as_ref())
            .map(|t| format!("codex:{t}"))
            .or_else(|| {
                ctx.stamp
                    .as_ref()
                    .and_then(|s| s.agent_id.as_ref())
                    .map(|a| format!("claude:{a}"))
            })
            .unwrap_or_else(|| c.actor.clone());
        Some((
            idem::default_key(c.session.as_deref().unwrap_or(""), &a, branch, payload),
            true,
        ))
    }

    /// The ref of a branch name for the lookup: the live ref, else the newest deleted ref of that name (a retry of a
    /// `BranchDelete`), with its tip.
    pub(crate) fn branch_ref(&self, name: &str) -> Option<&Ref> {
        self.dag.live(name).or_else(|| {
            self.dag
                .refs
                .values()
                .rev()
                .find(|r| r.deleted && r.name == name)
        })
    }

    /// The lookup of [API §7.4] against the branch the command writes; row 3's absorbed vector is computed only when
    /// it is needed.
    fn lookup(&self, key: &[u8; 16], payload: &[u8; 16], branch: &str) -> Lookup {
        let r = self.branch_ref(branch);
        let (ref_id, tip) = (r.map(|r| r.id), r.and_then(|r| r.tip));
        let absorbed: OnceCell<BTreeMap<u32, u64>> = OnceCell::new();
        let dag = &self.dag;
        self.idem.lookup(
            key,
            payload,
            ref_id,
            self.env.wall_ms,
            &self.hlc,
            self.windows(),
            &|rid, rs| {
                absorbed
                    .get_or_init(|| dag.absorbed(tip))
                    .get(&rid)
                    .is_some_and(|v| *v >= rs)
            },
        )
    }

    /// Looks up a keyed command: `Ok(None)` to execute, `Ok(Some(reply))` for a replay, or E408.
    pub(crate) fn keyed(
        &self,
        key: &Option<([u8; 16], bool)>,
        payload: &[u8; 16],
        branch: &str,
        ctx: &Ctx,
        caller: &Caller,
    ) -> Res<Option<Reply>> {
        // [API §4.3]: the lookup first; §4.3 rows 1–4 only when it found no entry.
        let lookup = match key {
            Some((k, _)) => self.lookup(k, payload, branch),
            None => Lookup::Execute,
        };
        match lookup {
            Lookup::Execute => match &caller.pending {
                Some(e) => Err(e.clone()),
                None => Ok(None),
            },
            Lookup::Replay(e) => Ok(Some(self.replay(&e, branch, ctx, caller))),
            Lookup::Mismatch(e) => Err(*e),
        }
    }

    /// The result of a replay ([API §7.5]), rebuilt from the original commit, its group's records and the
    /// `IdemResult` items (§17.1).
    // spec: [API §7.5]
    fn replay(&self, e: &Entry, branch: &str, ctx: &Ctx, caller: &Caller) -> Reply {
        let mut r = Reply::ok(Data::None);
        r.outcome = Outcome::Replayed;
        r.branch = Some(branch.to_string());
        let current = self.branch_ref(branch);
        r.rev = Some(current.and_then(|x| x.tip).unwrap_or(0));
        r.commit = e.commit.map(|c| c.0);
        r.rev_new = e.commit.map(|c| c.0);
        r.key = ctx.key.clone();
        r.lease = ctx.lease.clone();
        r.warnings = caller.warnings.clone();
        let commit = e.commit.map(|(seq, _)| &self.dag.commits[&seq]);
        let ended = |reason: u8| -> Vec<u64> {
            e.result
                .items
                .iter()
                .filter_map(|i| match i {
                    ResultItem::LeaseEnd { lease, reason: r } if *r == reason => Some(*lease),
                    _ => None,
                })
                .collect()
        };
        let moved = e.result.items.iter().find_map(|i| match i {
            ResultItem::RefMove {
                ref_id,
                reason,
                old,
                new,
            } => Some((*ref_id, *reason, *old, *new)),
            _ => None,
        });
        match e.result.cmd.as_str() {
            "Schema" => {
                let mut keys: Vec<String> = commit
                    .map(|c| {
                        c.changeset
                            .keys()
                            .filter_map(|k| match k {
                                Key::Schema(i) => Some(i.text()),
                                Key::Node(..) => None,
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                keys.sort();
                r.data = Data::Schema(keys);
            }
            "RunOpen" => {
                if let Some(c) = commit {
                    let run = created_of_kind(&c.changeset, "run");
                    let st = self.dag.state_at(Some(c.seq), &self.alloc);
                    let name = run
                        .and_then(|n| st.nodes.get(&n))
                        .and_then(|x| x.text("title"))
                        .unwrap_or("")
                        .to_string();
                    r.data = Data::RunOpen(run.unwrap_or(Nid(0)), name);
                }
            }
            "RunClose" => {
                if let Some(c) = commit {
                    let (run, status) = c
                        .changeset
                        .iter()
                        .find_map(|(k, (_, after))| match (k, after) {
                            (
                                Key::Node(n, crate::state::Aspect::Status),
                                KState::Plain(Some(KVal::Status { status, .. })),
                            ) => Some((*n, status.clone())),
                            _ => None,
                        })
                        .unwrap_or((Nid(0), String::new()));
                    r.data = Data::RunClose(run, status, ended(lease::EndReason::RunClose.code()));
                }
            }
            "LaneOpen" => {
                // The lane node of the commit, the ref it names and the binding of its tree ([API §11.5]).
                if let Some(c) = commit
                    && let Some(lane) = created_of_kind(&c.changeset, "lane")
                {
                    let st = self.dag.state_at(Some(c.seq), &self.alloc);
                    let x = &st.nodes[&lane];
                    let full = x.text("moirai_branch").unwrap_or("").to_string();
                    let dir = match x.fields.get("worktree_path") {
                        Some(crate::value::Value::Path(p)) => p.text.clone(),
                        _ => String::new(),
                    };
                    let ref_id = self
                        .dag
                        .refs
                        .values()
                        .filter(|r| r.name == full)
                        .map(|r| r.id)
                        .max()
                        .unwrap_or(0);
                    let row = self.heads.get(crate::heads::HeadKind::Directory, &dir);
                    r.data = Data::LaneOpen(Box::new(LaneOpenData {
                        lane,
                        ref_: full.clone(),
                        ref_id,
                        fork: c.seq,
                        binding: BindData {
                            dir: dir.clone(),
                            ref_: Some(full),
                            designated: row.is_some_and(|h| h.designated),
                            expected_ref: row.and_then(|h| h.expected_ref.clone()),
                            base: row.and_then(|h| h.base.clone()),
                            replaced: Vec::new(),
                            removed: false,
                        },
                    }));
                }
            }
            "LaneClose" => {
                if let Some(c) = commit {
                    let (lane, status) = c
                        .changeset
                        .iter()
                        .find_map(|(k, (_, after))| match (k, after) {
                            (
                                Key::Node(n, crate::state::Aspect::Status),
                                KState::Plain(Some(KVal::Status { status, .. })),
                            ) => Some((*n, status.clone())),
                            _ => None,
                        })
                        .unwrap_or((Nid(0), String::new()));
                    let st = self.dag.state_at(Some(c.seq), &self.alloc);
                    let dir =
                        st.nodes
                            .get(&lane)
                            .and_then(|x| match x.fields.get("worktree_path") {
                                Some(crate::value::Value::Path(p)) => Some(p.text.clone()),
                                _ => None,
                            });
                    r.data = Data::LaneClose(lane, status, dir);
                }
            }
            "BranchCreate" => {
                if let Some((ref_id, _, _, new)) = moved
                    && let Some(x) = self.dag.refs.get(&ref_id)
                {
                    r.branch = Some(x.name.clone());
                    r.rev = Some(x.tip.unwrap_or(0));
                    r.data = Data::BranchCreate(x.name.clone(), ref_id, x.kind, new);
                }
            }
            "BranchDelete" => {
                if let Some((ref_id, _, old, _)) = moved
                    && let Some(x) = self.dag.refs.get(&ref_id)
                {
                    r.rev = Some(old.unwrap_or(0));
                    // `dropped` is not replayed: a replay gives null ([API §11.2]).
                    let _ = old;
                    r.data = Data::BranchDelete(
                        x.name.clone(),
                        ref_id,
                        None,
                        ended(lease::EndReason::BranchDeleted.code()),
                    );
                }
            }
            "Merge" | "Sync" | "MergeContinue" | "Revert" | "CherryPick" => {
                // Family W: the data rebuilt from the original commit and its group ([API §7.5]); a replayed staged
                // outcome keeps its `errors` and exit 6.
                if let Some(d) = self.replay_merge(e.commit.map(|c| c.0)) {
                    if let Some(g) = &d.staging_ref {
                        r.outcome = Outcome::Staged;
                        r.exit = 6;
                        r.error = Some(
                            Refusal::new("staged", 6, format!("staged on {g}"))
                                .key("staging_ref", g.clone())
                                .key("conflicts", d.conflicts.len() as i64),
                        );
                    }
                    r.markers = d.markers.clone();
                    r.data = Data::Merge(Box::new(d));
                }
            }
            "FileAdd" => {
                // The data the original result listed, kept in its recorded rows ([API §7.5]).
                r.data = Data::FileAdd(
                    e.result
                        .yields
                        .iter()
                        .flat_map(|y| &y.rows)
                        .filter_map(|row| {
                            let get =
                                |k: &str| row.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
                            let (root, text) = get("path")?.split_once(':')?;
                            let id = get("id")?.strip_prefix('#')?.parse::<u32>().ok()?;
                            Some((
                                crate::value::PathVal {
                                    root: root.to_string(),
                                    text: text.to_string(),
                                },
                                Nid(id),
                                get("created") == Some("true"),
                            ))
                        })
                        .collect(),
                );
            }
            "FileMv" | "FileRm" | "FileRevert" => {
                // The intent the recorded result names, and the data its records hold.
                let id = e
                    .result
                    .yields
                    .iter()
                    .flat_map(|y| &y.rows)
                    .flat_map(|row| row.iter())
                    .find(|(k, _)| k == "intent")
                    .and_then(|(_, v)| v.strip_prefix("i-")?.parse::<u64>().ok());
                if let Some(d) = id.and_then(|i| self.files.results.get(&i)) {
                    r.data = d.clone();
                }
                r.yields.clear();
            }
            "MergeAbort" => {
                if let Some((ref_id, ..)) = moved
                    && let Some(x) = self.dag.refs.get(&ref_id)
                {
                    r.data = Data::MergeAbort(x.name.clone());
                }
            }
            "Undo" => {
                if let Some((ref_id, _, old, new)) = moved
                    && let Some(x) = self.dag.refs.get(&ref_id)
                {
                    let back = x
                        .moves
                        .iter()
                        .rev()
                        .find(|m| m.reason == MoveReason::Undo && m.old == old && m.new == new)
                        .and_then(|m| self.moves_back.get(&(ref_id, m.hlc)).copied())
                        .unwrap_or(1);
                    r.data = Data::Undo(Box::new(crate::history::UndoData {
                        ref_: x.name.clone(),
                        old,
                        new,
                        moved_back: back,
                        markers: Vec::new(),
                        triage: Vec::new(),
                    }));
                }
            }
            "OpRestore" => {
                let moved: Vec<(String, Option<u64>, Option<u64>)> = e
                    .result
                    .items
                    .iter()
                    .filter_map(|i| match i {
                        ResultItem::RefMove {
                            ref_id, old, new, ..
                        } => self
                            .dag
                            .refs
                            .get(ref_id)
                            .map(|x| (x.name.clone(), *old, *new)),
                        _ => None,
                    })
                    .collect();
                r.branch = None;
                r.rev = None;
                r.data = Data::OpRestore(Box::new(crate::history::RestoreData {
                    seq: 0,
                    moved,
                    markers: Vec::new(),
                    triage: Vec::new(),
                }));
            }
            _ => {
                // Family T: the commit's diff rows and affected set, the yields its group's records give.
                if let Some(c) = commit {
                    r.diff = c.changeset.clone();
                    r.other = c.affected.clone();
                }
                r.yields = e.result.yields.clone();
            }
        }
        // `markers`: the entries of the original group's Marker record ([API §7.5]).
        let group = match (e.commit, moved) {
            (Some((seq, _)), _) => Some(Group::Commit(seq)),
            (None, Some((ref_id, reason, old, new))) => self.dag.refs.get(&ref_id).and_then(|x| {
                x.moves
                    .iter()
                    .find(|m| m.reason.code() == reason && m.old == old && m.new == new)
                    .map(|m| Group::Move(m.hlc))
            }),
            _ => None,
        };
        if let Some(g) = group {
            r.markers = self.listed(self.markers.group(g));
        }
        r
    }

    /// The LQ binder's caller ([LQ/canonical-ast §5.9]): the effective role and surface; for a free-form `TX` the
    /// session's model profile under its write rule (WR-012, [`crate::profile::binder_profile`]), otherwise
    /// `compatible` (a named mutation's expansion is not free-form); the read safelist of the role (WQ-003).
    // rule: WR-012
    fn lq_caller(&self, c: &Caller, ctx: &Ctx, rights: &Rights, free_form: bool) -> LqCaller {
        let (profile, dry_targets) = if free_form {
            crate::profile::binder_profile(
                c.profile,
                crate::profile::model_write_rule(&self.conf, c.profile),
            )
        } else {
            (Profile::Compatible, false)
        };
        LqCaller {
            role: rights.role.clone(),
            surface: if ctx.door == Door::Mcp {
                crate::lq::ctx::Surface::Mcp
            } else {
                crate::lq::ctx::Surface::Cli
            },
            profile,
            branch: c.branch.clone(),
            named_only: crate::profile::read_safelist(&self.conf, &rights.role),
            unknown_dry_targets: dry_targets,
            write_profile: c.profile,
            ..LqCaller::default()
        }
    }

    /// The from-scratch derived rows of a commit's state, cached for the live refs' tips.
    pub(crate) fn rows_at(&mut self, seq: Option<u64>) -> Rc<BTreeMap<Nid, Row>> {
        let Some(s) = seq else {
            return Rc::new(BTreeMap::new());
        };
        if let Some(r) = self.rows.get(&s) {
            return r.clone();
        }
        let st = self.dag.state_at(Some(s), &self.alloc);
        let rows = Rc::new(derived::recompute_all(&st, &|_| None));
        self.rows.insert(s, rows.clone());
        self.prune_rows();
        rows
    }

    pub(crate) fn prune_rows(&mut self) {
        let tips: BTreeSet<u64> = self.dag.live_refs().filter_map(|r| r.tip).collect();
        self.rows.retain(|s, _| tips.contains(s));
    }

    /// A `TX` block, data-level ([API §9.1]), or one procedure call (the group-C commands, [API §10]), or the kernel
    /// statements of a family-W verb (`Keying::Given`). The order: the caller context (§4.3), the static phase (the
    /// LQ form bound by LQ-3; the cap `tx.max-statements`), the idempotency lookup (§7.4), the verb's role row, IF TIP,
    /// the message, the statements, the role-create checks, the deferred validators, then the commit and its group.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn tx(
        &mut self,
        stmts: &[Stmt],
        message: &str,
        ctx: &Ctx,
        proc: Option<&str>,
        exp: Option<Named<'_>>,
        keying: Keying,
        after: After,
    ) -> Res<Reply> {
        let claim = proc == Some("tx.claim");
        let moving = match &after {
            After::MoveLease(b) => Some(b.as_str()),
            _ => None,
        };
        // §4.3 rows 1–4 wait for the lookup ([API §4.3] "The idempotency pre-check comes first"): a family-W command
        // that keyed its lookup already raises them itself.
        let caller = self.resolve_keyed(ctx, claim, moving)?;
        if matches!(keying, Keying::Given { .. })
            && let Some(e) = &caller.pending
        {
            return Err(e.clone());
        }
        let resolve_only = match exp {
            Some((n, _, _)) => n == "tx.resolve",
            None => !stmts.is_empty() && stmts.iter().all(|s| matches!(s, Stmt::Resolve { .. })),
        };
        // Row 7, after the lookup too.
        let view_ok = self.writable(&caller.branch, resolve_only);
        let mut rights = self.rights(&caller, ctx);
        // The rights `H` is bound with for the lookup: an ended presented lease's role, as the original call had.
        let bind_rights = match caller.ended_lease.and_then(|id| self.leases.get(&id)) {
            Some(l) => {
                let mut r = rights.clone();
                r.role = policy::effective_role(
                    Some(&Presented {
                        id: l.id,
                        task: l.task,
                        run: l.run,
                        role: l.role.clone(),
                        holder: l.holder.clone(),
                        session_role: l.session_role,
                    }),
                    false,
                );
                r
            }
            None => rights.clone(),
        };
        // WT-012: `answer --by owner` presented with the orchestrator's session role lease is owner-attested.
        let attested = exp.is_some_and(|(n, _, ps)| {
            n == "tx.answer"
                && ps
                    .iter()
                    .find(|(k, _)| k == "by")
                    .is_none_or(|(_, v)| *v == P::Text("owner".into()))
        }) && rights.lease.as_ref().is_some_and(|l| l.session_role);
        if attested {
            rights.owner_attested = true;
            rights.role = "owner".into();
        }
        let tip = self.dag.live(&caller.branch).and_then(|r| r.tip);
        let base = self.dag.state_at(tip, &self.alloc);
        // Static phase: the LQ equivalent, bound by LQ-3 ([API §9.3]; WR-010).
        let eq = match exp {
            Some((_, e, _)) => tx::Equivalent {
                text: e.text.clone(),
                stmts: e.lq_stmts.clone(),
                params: e.params.clone(),
            },
            None => tx::equivalent(&base.schema, stmts),
        };
        // `tx.max-statements` ([CFG §10.5]), at binding.
        crate::budget::check_caps(
            self.cfg.max_statements,
            self.cfg.max_ops,
            eq.stmts.len() as u64,
            0,
        )?;
        // Idempotency ([API §7]): the block's `H`, then the lookup; Heartbeat is never keyed.
        let mut e411: Option<Refusal> = None;
        let (key, h, cmd_name) = match keying {
            Keying::Block => {
                let params = Params(eq.params.iter().cloned().collect());
                let lq_schema = lqh::lq_schema(&base.schema);
                let ids = lqh::ViewIds {
                    uids: &self.alloc.uids,
                    uidx: &self.alloc.uidx,
                    st: &base,
                    next_id: self.next_id,
                };
                // The statement of a binder refusal: the LQ statement its span starts in.
                let static_stmt = |at: Option<usize>| -> Option<usize> {
                    let at = at?;
                    let mut pos = "TX { ".len();
                    for (i, (_, t)) in eq.stmts.iter().enumerate() {
                        let end = pos + t.len();
                        if at < end {
                            return Some(i + 1);
                        }
                        pos = end + "; ".len();
                    }
                    None
                };
                let h = if proc == Some("tx.heartbeat") {
                    [0; 16]
                } else if let Some((name, _, orig)) = exp.filter(|_| ctx.door == Door::Mcp) {
                    // R4: an MCP `write` with `name` is `TX { CALL tx.<name>(…) }` ([LQ/canonical-ast §5.9]).
                    let p4 = Params(orig.iter().cloned().collect());
                    lqh::h_of_mutation(
                        name,
                        &p4,
                        normalize_message(message)
                            .ok()
                            .as_deref()
                            .filter(|m| !m.is_empty()),
                        &lq_schema,
                        &ids,
                        &self.lq_caller(&caller, ctx, &bind_rights, false),
                    )
                    // The call is the block's one statement: a located refusal names it; an unlocated one refuses
                    // the call as a whole (E406's MCP case) unless it names its statement itself ([LQ/errors §5.5]).
                    .map_err(|(e, at)| e.finish(at.map(|_| 1)))?
                } else {
                    let free_form =
                        proc.is_none() && exp.is_none() && matches!(after, After::Nothing);
                    // A non-empty `message` is the block's `MESSAGE` option, inside `H` ([API §7.3], §9.3).
                    let text = match normalize_message(message).ok().filter(|m| !m.is_empty()) {
                        Some(m) => {
                            let lit = crate::lq::printer::string_lit(&m);
                            match eq.text.strip_prefix("TX {") {
                                Some(rest) => format!("TX MESSAGE {lit} {{{rest}"),
                                None => eq.text.clone(),
                            }
                        }
                        None => eq.text.clone(),
                    };
                    let bound = |free: bool| {
                        lqh::h_of_tx(
                            &text,
                            &params,
                            &lq_schema,
                            &ids,
                            &self.lq_caller(&caller, ctx, &bind_rights, free),
                        )
                        .map_err(|(e, at)| {
                            let s = static_stmt(at);
                            e.finish(s)
                        })
                    };
                    match bound(free_form) {
                        // E411 is the block's policy refusal after E406 in code order ([LQ/errors §3.1]): the static
                        // role rows run first (WR-008), then E411 ([RULES/role-write-policy] WR-012). `H` does not
                        // depend on the profile, so the lookup reads it from the bind without the write rule. Under
                        // `dry-targets` a `DRY` run is allowed (WQ-005): `ctx.dry` is the block's `DRY`, which the
                        // block's LQ equivalent does not carry.
                        // WR-012's scope ([API §9.1] E411 row; spec sync 2b): a caller with a session identity (CX-4) or
                        // `door` = `mcp`; a CLI block with neither is the owner's and is not refused.
                        Err(e) if e.code == "E411" => {
                            let dry_ok = ctx.dry
                                && crate::profile::model_write_rule(&self.conf, caller.profile)
                                    == crate::profile::WriteRule::DryTargets;
                            let scoped = caller.session.is_some() || ctx.door == Door::Mcp;
                            if !dry_ok && scoped {
                                e411 = Some(e);
                            }
                            bound(false)?
                        }
                        other => other?,
                    }
                };
                let key = if proc != Some("tx.heartbeat") {
                    self.key_of(ctx, &caller, &caller.branch, &h)
                } else {
                    None
                };
                if let Some(r) = self.keyed(&key, &h, &caller.branch, ctx, &caller)? {
                    return Ok(r);
                }
                let name = proc.or(exp.map(|e| e.0)).unwrap_or("Tx").to_string();
                (key, Some(h), name)
            }
            Keying::Given { key, cmd, .. } => (key, None, cmd.to_string()),
        };
        let view = view_ok?;
        // WR-008: the verb's surface row ([RULES/role-write-policy] `role-verbs`), after the lookup ([AR §4.5]).
        let mcp = ctx.door == Door::Mcp;
        let verb = match (proc, exp.map(|e| e.0), &keying) {
            _ if matches!(after, After::Apply { .. }) => match &after {
                After::Apply { verb, .. } => Some(*verb),
                _ => None,
            },
            (_, _, Keying::Given { .. }) => None,
            (Some("tx.claim"), _, _) => Some(if mcp { "mcp-claim" } else { "claim" }),
            (Some("tx.complete"), _, _) => Some(if mcp { "mcp-complete" } else { "complete" }),
            (Some("tx.reclaim"), _, _) => Some("reclaim"),
            (Some(_), _, _) => None,
            (None, Some("tx.remember"), _) => {
                Some(if mcp { "mcp-remember" } else { "write-verbs" })
            }
            (None, Some(_), _) if mcp => Some("mcp-write"),
            (None, Some("tx.rm"), _) => Some("rm"),
            (None, Some("tx.resolve"), _) => Some("resolve"),
            (None, Some("tx.answer"), _) => Some("answer"),
            (None, Some(_), _) => Some("write-verbs"),
            (None, None, _) => Some(if mcp { "mcp-write" } else { "tx" }),
        };
        if let Some(v) = verb {
            rights.verb(v).map_err(|e| e.finish(None))?;
        }
        // IF TIP ([API §9.1]; [LQ/errors §5.7] E402).
        if let Some(t) = ctx.if_tip
            && tip != Some(t)
        {
            return Err(Refusal::lq(
                "E402",
                format!(
                    "{} moved: IF TIP s{t}, the tip is s{}",
                    caller.branch,
                    tip.unwrap_or(0)
                ),
            )
            .key("statement", Kv::Null)
            .key("tip", tip.map_or(Kv::Null, Kv::Commit))
            .key("expected_tip", Kv::Commit(t))
            .key("targets", Kv::Null)
            .key("written", false));
        }
        // The message argument ([API §9.1]; [F07 §5.2]).
        normalize_message(message)?;
        let seed = self.inited.as_ref().map_or(0, |i| i.seed);
        let codex_root = ctx
            .meta
            .as_ref()
            .and_then(|m| m.session_id.as_ref())
            .map(|s| blake3_128(&[format!("codex:{s}").as_bytes()]));
        let cx = tx::Ctx {
            dag: &self.dag,
            alloc: &self.alloc,
            uidx: &self.alloc.uidx,
            env: &self.env,
            branch: caller.branch.clone(),
            view,
            rights,
            actor: caller.actor.clone(),
            session: caller.session.clone(),
            seed,
            n: self.n,
            next_id: self.next_id,
            fence: self.fence,
            cfg: &self.cfg,
            thread_hash: caller.thread.as_ref().map(|t| blake3_128(&[t.as_bytes()])),
            root_session: codex_root,
            subagent_or_worker: ctx.stamp.as_ref().is_some_and(|s| s.agent_id.is_some())
                || ctx
                    .meta
                    .as_ref()
                    .is_some_and(|m| m.thread_id.is_some() && m.thread_id != m.session_id)
                || Self::env_var(ctx, "MOIRAI_LEASE").is_some()
                || Self::env_var(ctx, "MOIRAI_RUN").is_some(),
        };
        let mut cand = Cand::new(cx, base.clone(), self.leases.clone());
        cand.door = exp.and_then(|(_, e, _)| e.door);
        let kernel_of: Vec<usize> = eq.stmts.iter().map(|(d, _)| *d).collect();
        cand.set_lq_map(&kernel_of);
        if !claim && proc != Some("tx.heartbeat") && proc != Some("tx.release") {
            cand.presented().map_err(|e| e.finish(None))?;
        }
        // E406 precedes a pending E411 in code order ([LQ/errors §3.1]); any other refusal of the block yields to it.
        let policy_first = |e: Refusal, e411: &Option<Refusal>| match e411 {
            Some(x) if e.code != "E406" => x.clone(),
            _ => e,
        };
        for (i, s) in stmts.iter().enumerate() {
            cand.run(i + 1, s, &self.hlc)
                .map_err(|e| policy_first(e, &e411))?;
        }
        match &after {
            After::Nothing => {}
            After::RunClose(run) => {
                // RunClose: every lease scoped to the run is released in the group (reason 7).
                let ids: Vec<u64> = cand
                    .leases
                    .values()
                    .filter(|l| l.run == Some(*run) && l.ended.is_none())
                    .map(|l| l.id)
                    .collect();
                for id in ids {
                    cand.end_lease(id, lease::EndReason::RunClose);
                }
            }
            After::MoveLease(b) => {
                // The presented lease moves to the explicit branch (event 3, mask bit 1).
                if let Some(id) = caller.lease
                    && let Some(l) = cand.leases.get_mut(&id)
                {
                    l.branch = b.clone();
                    cand.events.push(LeaseEvent::Moved { id });
                }
            }
            After::Apply { release, .. } => {
                // [API §9.4] step 4: every run-scoped lease the batch names that is still live (reason 6, LE-003).
                for id in release {
                    let live = cand.leases.get(id).is_some_and(|l| {
                        l.ended.is_none() && lease::is_live(l, &self.env).is_live()
                    });
                    if live {
                        cand.end_lease(*id, lease::EndReason::Apply);
                    }
                }
            }
        }
        cand.refresh_mentions();
        cand.check_creates().map_err(|e| policy_first(e, &e411))?;
        // WR-012: the model profile's write rule, after the role policy.
        if let Some(e) = e411 {
            return Err(e);
        }
        cand.deferred()?;
        cand.renew_by_use();
        let cs = diff(&base, &cand.st);
        // [F12 §9.3]: on a staging ref, `Resolve` ops and their companions only.
        cand.staging_ops(&cs)
            .map_err(|e| e.finish(Some(cand.last_lq_stmt())))?;
        // `tx.max-ops` ([CFG §10.5]): one op per changed key of the net changeset.
        crate::budget::check_caps(
            self.cfg.max_statements,
            self.cfg.max_ops,
            0,
            cs.len() as u64,
        )?;
        let statements = self.statement_results(&eq, &cand);
        let yields = cand.yields.clone();
        let events = cand.events.clone();
        let cand_message = cand.message.clone();
        let new_alloc = cand.new_alloc.clone();
        let resolves: Vec<Key> = cand.resolves.iter().cloned().collect();
        let (next_id, fence, mut leases_after, notified) = (
            cand.next_id,
            cand.fence,
            cand.leases.clone(),
            cand.notified.clone(),
        );
        let st_after = Rc::new(std::mem::take(&mut cand.st));
        drop(cand);
        let mut reply = Reply::ok(Data::None);
        reply.branch = Some(caller.branch.clone());
        reply.rev = Some(tip.unwrap_or(0));
        reply.commit = tip;
        reply.key = ctx.key.clone();
        reply.lease = ctx.lease.clone();
        reply.statements = statements;
        reply.yields = yields;
        reply.warnings = caller.warnings.clone();
        // The commit's message: the block's, or the one a `complete` writes ([API §10.5] step 3), or, for the `reopen`
        // verb with no message, its `REASON` ([API §9.2] `reopen`), by [F07 §5].
        let reopen_reason = match exp {
            Some(("tx.reopen", _, ps)) if message.is_empty() => ps
                .iter()
                .find(|(k, _)| k == "reason")
                .and_then(|(_, v)| match v {
                    P::Text(t) => Some(t.clone()),
                    _ => None,
                }),
            _ => None,
        };
        let msg = normalize_message(
            &cand_message
                .or(reopen_reason)
                .unwrap_or_else(|| message.to_string()),
        )?;
        // DRY runs every check and writes nothing ([API §9.1]); a delete's dry run shows its impact as the diff.
        // rule: DS-011
        if ctx.dry {
            reply.outcome = Outcome::Dry;
            reply.diff = cs;
            return Ok(reply);
        }
        // A `Resolve` that left its key's value as it was still writes its commit: `merge --continue` overlays the key
        // it names ([F12 §9.4] step 2).
        if cs.is_empty() && events.is_empty() && resolves.is_empty() {
            return Ok(reply);
        }
        let parent_rows = self.rows_at(tip);
        let ready_before = coord::ready_set(
            &self.dag,
            &self.alloc,
            &caller.branch,
            &self.leases,
            &self.env,
            Some(&caller.actor),
            Some(&parent_rows),
        );
        // Allocation (EP-W8): the new `#N`s are bound for good.
        for (n, (u, cr)) in &new_alloc {
            self.alloc.rows.insert(
                *n,
                (*u, cr.clone(), caller.branch.clone(), self.commit_seq + 1),
            );
            self.alloc.uidx.insert(*u, *n);
            self.alloc.uids.insert(*n, *u);
        }
        self.next_id = next_id;
        self.fence = fence;
        let mut commit_seq = None;
        if !cs.is_empty() || !resolves.is_empty() {
            let child_rows = Rc::new(derived::recompute_all(&st_after, &|_| None));
            let (affected, complete) =
                derived::affected_rows(&parent_rows, &child_rows, self.cfg.suspect_budget);
            // DS-010: beyond the budget the command prints the hint `SuspectBudget` and keeps its exit code
            // ([F19 §12.3] class 131).
            if !complete {
                reply.hints.push((
                    "SuspectBudget".into(),
                    format!(
                        "suspect changed on {} nodes, above store.suspect-budget ({}); affected is incomplete and past views recompute derived state",
                        derived::suspect_changes(&parent_rows, &child_rows),
                        self.cfg.suspect_budget
                    ),
                ));
            }
            let mut affected: BTreeSet<Nid> = affected.into_iter().collect();
            affected.extend(notified);
            let (stmt_origin, sym, stmt_hash) = match &keying {
                Keying::Given { sym, .. } => ("verb", Some(sym.to_string()), None),
                Keying::Block if matches!(after, After::Apply { .. }) => {
                    ("tx", Some("apply".to_string()), h)
                }
                Keying::Block => {
                    let origin = match (proc.or(exp.map(|e| e.0)), ctx.door) {
                        (Some(_), Door::Mcp) => "named-mutation",
                        (Some(_), _) => "verb",
                        (None, _) if stmts.len() == 1 && matches!(stmts[0], Stmt::Call { .. }) => {
                            "named-mutation"
                        }
                        (None, Door::Mcp) => "mcp-write",
                        (None, _) => "tx",
                    };
                    let sym = match (proc.or(exp.map(|e| e.0)), &stmts.first()) {
                        (Some(p), _) => Some(p.to_string()),
                        (None, Some(Stmt::Call { proc, .. })) if stmts.len() == 1 => {
                            Some(proc.clone())
                        }
                        _ => None,
                    };
                    (origin, sym, h)
                }
            };
            let payload = match &keying {
                Keying::Given { payload, .. } => *payload,
                Keying::Block => h.expect("a block has H"),
            };
            let seq = self.append_commit(
                &caller,
                st_after.clone(),
                cs.clone(),
                msg,
                stmt_origin,
                sym,
                stmt_hash,
                key.map(|(k, _)| (k, payload)),
                affected.into_iter().collect(),
                complete,
            );
            // The keys its `Resolve` ops name; unhashed, as the canonical form records values only ([F06 §7.7]).
            if let Some(c) = self.dag.commits.get_mut(&seq) {
                c.resolves = resolves;
            }
            self.rows.insert(seq, child_rows);
            self.prune_rows();
            commit_seq = Some(seq);
            reply.commit = Some(seq);
            reply.rev_new = Some(seq);
        }
        // The group's Lease records draw from the HLC sequence in append order, after the commit (CK-4); a grant's
        // record is its `claimed_hlc` ([F11 §6] field 36).
        let mut items = Vec::new();
        for e in &events {
            match e {
                LeaseEvent::Grant { id, reused: false } => {
                    let h = self.hlc.record(self.env.wall_ms);
                    if let Some(l) = leases_after.get_mut(id) {
                        l.claimed_hlc = h;
                    }
                }
                LeaseEvent::End { id, reason } => {
                    self.hlc.record(self.env.wall_ms);
                    items.push(ResultItem::LeaseEnd {
                        lease: *id,
                        reason: reason.code(),
                    });
                }
                LeaseEvent::Renew { durable: true, .. } | LeaseEvent::Moved { .. } => {
                    self.hlc.record(self.env.wall_ms);
                }
                _ => {}
            }
        }
        self.leases = leases_after;
        self.bind_env_lease(&caller);
        for e in &events {
            let (id, op) = match e {
                LeaseEvent::Grant { id, reused: false } => (*id, "grant"),
                LeaseEvent::Grant { reused: true, .. } => continue,
                LeaseEvent::End { id, .. } => (*id, "end"),
                LeaseEvent::Moved { id } => (*id, "move"),
                LeaseEvent::Renew { id, .. } => (*id, "renew"),
            };
            if let Some(l) = self.leases.get(&id) {
                let (branch, task, holder) = (l.branch.clone(), l.task, l.holder.clone());
                // [LQ/std §2.15]: the group's commit (none for a renewal), the holder as actor.
                let commit = commit_seq.filter(|_| op != "renew");
                self.feed.event(
                    self.commit_seq,
                    &branch,
                    commit,
                    task,
                    op,
                    "lease",
                    format!("L-{id}"),
                    &holder,
                    commit_seq,
                );
            }
        }
        // The group's Marker record follows its Lease records ([F05 §4.7] commit group).
        if let Some(s) = commit_seq {
            let completes = completions(&events, &self.leases, &reply.yields);
            let entries = self.markers.commit_lands(
                &self.dag,
                s,
                tip,
                &completes,
                &mut self.hlc,
                self.env.wall_ms,
            );
            reply.markers = self.listed(&entries);
            self.feed_markers(&entries, Some(s));
        }
        let payload = match &keying {
            Keying::Given { payload, .. } => *payload,
            Keying::Block => h.unwrap_or([0; 16]),
        };
        self.record_idem(
            key,
            payload,
            &caller.branch,
            commit_seq,
            ctx,
            Recorded {
                cmd: cmd_name,
                items,
                yields: reply.yields.clone(),
            },
        );
        let child_rows = self.rows_at(self.dag.live(&caller.branch).and_then(|r| r.tip));
        let ready_after = coord::ready_set(
            &self.dag,
            &self.alloc,
            &caller.branch,
            &self.leases,
            &self.env,
            Some(&caller.actor),
            Some(&child_rows),
        );
        reply.ready = ready_after
            .iter()
            .filter(|n| !ready_before.contains(n))
            .copied()
            .collect();
        if let Some(s) = commit_seq {
            let c = &self.dag.commits[&s];
            reply.other = c
                .affected
                .iter()
                .filter(|n| !reply.ready.contains(n))
                .copied()
                .collect();
        }
        if let Some(y) = reply.yields.iter_mut().find(|y| y.proc == "tx.complete") {
            for row in &mut y.rows {
                if let Some(slot) = row.iter_mut().find(|(k, _)| k == "ready") {
                    slot.1 = reply
                        .ready
                        .iter()
                        .map(|n| n.to_string())
                        .collect::<Vec<_>>()
                        .join(",");
                }
            }
        }
        reply.diff = cs;
        // [API §10.5] step 4: the completed tasks' link settle, a separate commit of the same command.
        self.complete_settle(&caller, &mut reply, ctx)?;
        Ok(reply)
    }

    /// CX-9: an environment lease binds, at its first use, to the attested thread that uses it (`LEASES.bound`, a
    /// `Lease` record of event 3 with mask bit 2 in the command's group).
    // spec: [API §4.2] CX-9
    fn bind_env_lease(&mut self, caller: &Caller) {
        let (Some(id), Some(th)) = (caller.lease, &caller.thread) else {
            return;
        };
        if !caller.env_lease {
            return;
        }
        if let Some(l) = self.leases.get_mut(&id)
            && l.bound.is_none()
        {
            l.bound = Some(blake3_128(&[th.as_bytes()]));
            self.hlc.record(self.env.wall_ms);
        }
    }

    /// Appends a local commit on the caller's branch ([F06 §4.3]; CK-4) and returns its seq.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn append_commit(
        &mut self,
        caller: &Caller,
        st: Rc<State>,
        cs: Changeset,
        message: String,
        stmt_origin: &'static str,
        stmt_sym: Option<String>,
        stmt_hash: Option<[u8; 16]>,
        idem: Option<([u8; 16], [u8; 16])>,
        affected: Vec<Nid>,
        affected_complete: bool,
    ) -> u64 {
        let parents: Vec<u64> = self
            .dag
            .live(&caller.branch)
            .expect("the written ref is live")
            .tip
            .into_iter()
            .collect();
        let mut c = Commit::new(0, 0, 0, "ordinary", parents, 0, cs);
        c.message = message;
        c.stmt_origin = stmt_origin;
        c.stmt_sym = stmt_sym;
        c.stmt_hash = stmt_hash;
        c.idem = idem;
        c.affected = affected;
        c.affected_complete = affected_complete;
        self.append_on(caller, &caller.branch.clone(), c, st)
    }

    /// Appends a commit on the live ref `on` ([F06 §4.3]; CK-4): its seq, `ref_seq` and `hlc`, the caller's actor,
    /// role and session, its id from the state at its first parent and its own state ([F07 §12.1]), the ref move, the
    /// marker facts and the change feed. `c` carries its kind, parents, changeset and the other header fields. Returns
    /// its seq.
    pub(crate) fn append_on(
        &mut self,
        caller: &Caller,
        on: &str,
        mut c: Commit,
        st: Rc<State>,
    ) -> u64 {
        let seq = self.commit_seq + 1;
        self.commit_seq = seq;
        let hlc = self.hlc.commit(self.env.wall_ms);
        let r = self.dag.live_mut(on).expect("the written ref is live");
        // [F12 §2.2]: a tag never moves after its creation and an orphans ref moves only by parking; no commit lands on
        // either.
        assert!(
            !matches!(r.kind, RefKind::Tag | RefKind::Orphans),
            "internal error: a commit appended to {on}, a ref of kind {} that takes no commit",
            r.kind.token()
        );
        let ref_seq = r.ref_seq_next;
        r.ref_seq_next += 1;
        let old = r.tip;
        r.tip = Some(seq);
        r.moves.push(RefMove {
            old,
            new: Some(seq),
            reason: MoveReason::Commit,
            actor: caller.actor.clone(),
            hlc,
        });
        let ref_id = r.id;
        let lease_role = caller
            .lease
            .and_then(|l| self.leases.get(&l))
            .map(|l| l.role.clone())
            .unwrap_or_default();
        c.seq = seq;
        c.ref_id = ref_id;
        c.ref_seq = ref_seq;
        c.hlc = hlc;
        c.append_hlc = hlc;
        c.actor = caller.actor.clone();
        c.actor_src = caller.actor_src.to_string();
        c.role = lease_role;
        c.session = caller.session.clone().unwrap_or_default();
        if c.git.is_none() {
            c.git = self.git_group(caller);
        }
        let parent = c.parents.first().copied();
        let before = self.dag.state_at(parent, &self.alloc);
        self.dag
            .identify(&mut c, &before, &st, &|n: Nid| self.alloc.uid(n));
        self.dag.insert(c);
        self.markers.record_commit(&self.dag, &st, seq);
        self.feed.commit(&self.dag, &st, seq);
        self.dag.remember(seq, st);
        seq
    }

    fn statement_results(&self, eq: &tx::Equivalent, cand: &Cand<'_>) -> Vec<StmtResult> {
        let mut seen = BTreeSet::new();
        eq.stmts
            .iter()
            .enumerate()
            .map(|(i, (d, text))| {
                let targets = if seen.insert(*d) {
                    cand.targets
                        .get(d)
                        .map(|t| t.iter().copied().collect())
                        .unwrap_or_default()
                } else {
                    Vec::new()
                };
                StmtResult {
                    index: i + 1,
                    text: text.clone(),
                    targets,
                }
            })
            .collect()
    }

    /// A group-C command as one procedure call ([API §10]: "each equals `Mutation` with the procedure's name").
    fn procedure(&mut self, name: &str, args: Vec<(String, P)>, ctx: &Ctx) -> Res<Reply> {
        self.procedure_moving(name, args, None, ctx)
    }

    /// A procedure call with `--move-lease` ([AR §5a.4]).
    fn procedure_moving(
        &mut self,
        name: &str,
        args: Vec<(String, P)>,
        move_lease: Option<&str>,
        ctx: &Ctx,
    ) -> Res<Reply> {
        let stmts = vec![Stmt::Call {
            proc: name.to_string(),
            args,
        }];
        let after = move_lease.map_or(After::Nothing, |b| After::MoveLease(b.to_string()));
        self.tx(&stmts, "", ctx, Some(name), None, Keying::Block, after)
    }

    /// `Mutation` ([API §9.7]): a named mutation by name; the procedures are the group-C commands.
    // spec: [API §9.7]
    fn mutation(
        &mut self,
        name: &str,
        params: &[(String, P)],
        message: &str,
        move_lease: Option<&str>,
        ctx: &Ctx,
    ) -> Res<Reply> {
        if move_lease.is_some() && name != "tx.set" && name != "tx.complete" {
            return Err(Refusal::usage_arg(
                "move_lease",
                "--move-lease goes with tx.set and tx.complete",
            ));
        }
        if [
            "tx.claim",
            "tx.complete",
            "tx.heartbeat",
            "tx.release",
            "tx.reclaim",
        ]
        .contains(&name)
        {
            return self.procedure_moving(name, params.to_vec(), move_lease, ctx);
        }
        if let Some(cmd) = file_mutation(name, params)? {
            return self.dispatch(&cmd, ctx);
        }
        let caller = self.resolve_keyed(ctx, false, move_lease)?;
        let tip = self.dag.live(&caller.branch).and_then(|r| r.tip);
        let base = self.dag.state_at(tip, &self.alloc);
        let uidx = &self.alloc.uidx;
        let kind_of = |t: &Target| -> Option<String> {
            let n = match t {
                Target::Id(n) => *n,
                Target::Uid(u) => *uidx.get(u)?,
                Target::Var(_) => return None,
            };
            base.nodes.get(&n).map(|x| x.kind.clone())
        };
        let e = mutation::expand(&base.schema, name, params, &kind_of)?;
        drop(base);
        let stmts = e.stmts.clone();
        self.tx(
            &stmts,
            message,
            ctx,
            None,
            Some((name, &e, params)),
            Keying::Block,
            move_lease.map_or(After::Nothing, |b| After::MoveLease(b.to_string())),
        )
    }

    /// `Schema` ([API §9.8]): weakening items, and policy rows set, changed or removed. After the caller context and the
    /// lookup: the role row, then per item [F08 §8.2]'s names and §8.5's rules ([`Schema::check_item`]), an item of
    /// classes 1–4 whose key the view holds or that is a core key (E405 `schema weakening`); a project field takes the
    /// view's largest `decl` + 1 ([F08 §8.5.2]). A policy item's value is written in its canonical form, and a `null`
    /// value or the row's default removes the item ([F08 §8.5.6]: absence is the default's one form).
    // spec: [API §9.8]
    fn schema(&mut self, items: &[Item], message: &str, ctx: &Ctx) -> Res<Reply> {
        let caller = self.resolve_keyed(ctx, false, None)?;
        self.writable(&caller.branch, false)?;
        let mut args = BTreeMap::new();
        let uid = |n: Nid| {
            self.alloc
                .uids
                .get(&n)
                .map_or_else(|| n.to_string(), |u| format!("#u:{}", u.hex()))
        };
        args.insert(
            "items".to_string(),
            Cj::Arr(items.iter().map(|i| item_cj(i, &uid)).collect()),
        );
        if !message.is_empty() {
            args.insert("message".to_string(), Cj::Str(message.to_string()));
        }
        let payload = cmd_payload("Schema", args);
        let key = self.key_of(ctx, &caller, &caller.branch, &payload);
        if let Some(r) = self.keyed(&key, &payload, &caller.branch, ctx, &caller)? {
            return Ok(r);
        }
        let rights = self.rights(&caller, ctx);
        rights
            .statement(1, "schema-change")
            .map_err(|e| e.finish(Some(1)))?;
        let tip = self.dag.live(&caller.branch).and_then(|r| r.tip);
        let base = self.dag.state_at(tip, &self.alloc);
        let mut st = (*base).clone();
        let mut keys = Vec::new();
        for (i, it) in items.iter().enumerate() {
            let at = Some(i + 1);
            let k = it.key();
            if let Item::Policy(p) = it {
                st.schema.check_item(it).map_err(|e| e.finish(at))?;
                let default = policy::default_policy(&p.name).expect("a checked policy row");
                let value = p
                    .value
                    .as_deref()
                    .and_then(|v| policy::canonical_policy(&p.name, v))
                    .filter(|v| *v != default);
                match value {
                    Some(v) => {
                        st.schema.items.insert(
                            k.clone(),
                            Item::Policy(crate::schema::PolicyItem {
                                name: p.name.clone(),
                                value: Some(v),
                            }),
                        );
                    }
                    None => {
                        st.schema.items.remove(&k);
                    }
                }
                keys.push(k.text());
                continue;
            }
            if Schema::is_core(&k) || st.schema.items.contains_key(&k) {
                return Err(Refusal::lq(
                    "E405",
                    format!("schema weakening: {} is held already", k.text()),
                )
                .finish(at));
            }
            st.schema.check_item(it).map_err(|e| e.finish(at))?;
            let mut it = it.clone();
            if let Item::Field(f) = &mut it {
                let most = crate::schema::core()
                    .fields
                    .iter()
                    .map(|x| x.decl)
                    .chain(st.schema.items.values().filter_map(|x| match x {
                        Item::Field(x) => Some(x.decl),
                        _ => None,
                    }))
                    .max()
                    .unwrap_or(0);
                f.decl = most + 1;
            }
            keys.push(k.text());
            st.schema.items.insert(k, it);
        }
        // A project kind declares at least one status ([F08 §8.5.1]).
        for it in items {
            if let Item::Kind(k) = it
                && st.schema.values(&k.name, "status").is_empty()
            {
                return Err(Refusal::lq(
                    "E405",
                    format!(
                        "schema weakening: project kind {} declares no status",
                        k.name
                    ),
                )
                .finish(Some(items.len())));
            }
        }
        let msg = normalize_message(message)?;
        keys.sort();
        let cs = diff(&base, &st);
        let mut reply = Reply::ok(Data::Schema(keys));
        reply.branch = Some(caller.branch.clone());
        reply.rev = Some(tip.unwrap_or(0));
        reply.key = ctx.key.clone();
        reply.warnings = caller.warnings.clone();
        if ctx.dry {
            reply.outcome = Outcome::Dry;
            return Ok(reply);
        }
        let seq = self.append_commit(
            &caller,
            Rc::new(st),
            cs,
            msg,
            "verb",
            Some("schema".into()),
            None,
            key.map(|(k, _)| (k, payload)),
            Vec::new(),
            true,
        );
        let entries =
            self.markers
                .commit_lands(&self.dag, seq, tip, &[], &mut self.hlc, self.env.wall_ms);
        reply.markers = self.listed(&entries);
        self.feed_markers(&entries, Some(seq));
        self.prune_rows();
        self.record_idem(
            key,
            payload,
            &caller.branch,
            Some(seq),
            ctx,
            Recorded {
                cmd: "Schema".into(),
                ..Recorded::default()
            },
        );
        reply.commit = Some(seq);
        reply.rev_new = Some(seq);
        Ok(reply)
    }

    /// Records the idempotency entry of an outcome that appended records ([API §7.4] "What is recorded"): in the
    /// header of the commit that carries the result, else as an `Idem` record whose HLC opens the window.
    pub(crate) fn record_idem(
        &mut self,
        key: Option<([u8; 16], bool)>,
        payload: [u8; 16],
        branch: &str,
        seq: Option<u64>,
        ctx: &Ctx,
        result: Recorded,
    ) {
        let Some((k, default_key)) = key else { return };
        let append_hlc = match seq {
            Some(s) => self.dag.commits[&s].append_hlc,
            None => self.hlc.record(self.env.wall_ms),
        };
        let r = self.branch_ref(branch);
        let ref_id = r.map_or(0, |r| r.id);
        let rs = seq.map(|s| self.dag.commits[&s].ref_seq);
        self.idem.record(
            k,
            Entry {
                payload,
                ref_id,
                branch: branch.to_string(),
                commit: seq.zip(rs),
                default_key,
                append_hlc,
                key_text: ctx.key.clone(),
                result,
            },
        );
    }

    /// The payload of `RunOpen` or `RunClose` over its arguments as given ([API §7.3]): an argument that is `null` is
    /// left out, as an omitted one is.
    fn verb_payload(name: &str, args: &[(&str, P)]) -> [u8; 16] {
        cmd_payload(
            name,
            args.iter()
                .filter(|(_, v)| *v != P::Null)
                .map(|(k, v)| (k.to_string(), p_cj(v)))
                .collect(),
        )
    }

    /// `RunOpen` ([API §10.7]): a `run` node titled `name`, status `running`, `started` = now, and a `runs_in` edge to
    /// the lane of `lane`; keyed by `payload(c)` and looked up before `name_taken`.
    // spec: [API §10.7]
    fn run_open(&mut self, name: &str, fields: &[(String, P)], ctx: &Ctx) -> Res<Reply> {
        let caller = self.resolve_keyed(ctx, false, None)?;
        self.writable(&caller.branch, false)?;
        let mut args: Vec<(&str, P)> = vec![("name", P::Text(name.to_string()))];
        args.extend(fields.iter().map(|(k, v)| (k.as_str(), v.clone())));
        let payload = Self::verb_payload("RunOpen", &args);
        let key = self.key_of(ctx, &caller, &caller.branch, &payload);
        if let Some(r) = self.keyed(&key, &payload, &caller.branch, ctx, &caller)? {
            return Ok(r);
        }
        self.rights(&caller, ctx)
            .verb("run")
            .map_err(|e| e.finish(None))?;
        let tip = self.dag.live(&caller.branch).and_then(|r| r.tip);
        let base = self.dag.state_at(tip, &self.alloc);
        if base
            .nodes
            .values()
            .any(|x| x.live() && x.kind == "run" && x.text("title") == Some(name))
        {
            return Err(Refusal::new(
                "name_taken",
                6,
                format!("run {name} already exists on {}", caller.branch),
            )
            .key("name", name));
        }
        let mut fs = vec![
            ("title".to_string(), P::Text(name.to_string())),
            ("started".into(), P::Int(self.env.now_s())),
        ];
        let mut lane = None;
        for (k, v) in fields {
            if k == "lane" {
                lane = Some(v.clone());
            } else {
                // `harness` and `model` are the run's fields (decl 29, 30; [F08 §9.3]), which CX-6 reads.
                fs.push((k.clone(), v.clone()));
            }
        }
        let edges_out = match lane {
            Some(P::Text(l)) => {
                let n = base
                    .nodes
                    .iter()
                    .find(|(_, x)| {
                        x.live() && x.kind == "lane" && x.text("title") == Some(l.as_str())
                    })
                    .map(|(n, _)| *n)
                    .ok_or_else(|| Refusal::not_found("lane", l.clone()))?;
                vec![("runs_in".to_string(), Target::Id(n))]
            }
            _ => Vec::new(),
        };
        drop(base);
        let stmts = vec![Stmt::Create {
            name: Some("r".into()),
            kind: "run".into(),
            fields: fs,
            body: None,
            under: None,
            position: None,
            edges_out,
            edges_in: Vec::new(),
        }];
        let before = self.next_id;
        let mut reply = self.tx(
            &stmts,
            "",
            ctx,
            None,
            None,
            Keying::Given {
                key,
                payload,
                cmd: "RunOpen",
                sym: "run open",
            },
            After::Nothing,
        )?;
        reply.statements.clear();
        reply.data = Data::RunOpen(Nid(before), name.to_string());
        Ok(reply)
    }

    /// `RunClose` ([API §10.7]): the run's status `outcome`, `ended` = now, I14's guard for `green`, and every lease
    /// scoped to the run released in the same group (reason 7).
    // spec: [API §10.7]
    // rule: LE-004
    fn run_close(&mut self, name: &str, outcome: &str, ctx: &Ctx) -> Res<Reply> {
        let caller = self.resolve_keyed(ctx, false, None)?;
        self.writable(&caller.branch, false)?;
        let payload = Self::verb_payload(
            "RunClose",
            &[
                ("name", P::Text(name.to_string())),
                ("outcome", P::Text(outcome.to_string())),
            ],
        );
        let key = self.key_of(ctx, &caller, &caller.branch, &payload);
        if let Some(r) = self.keyed(&key, &payload, &caller.branch, ctx, &caller)? {
            return Ok(r);
        }
        self.rights(&caller, ctx)
            .verb("run")
            .map_err(|e| e.finish(None))?;
        if !["green", "red", "stopped", "died"].contains(&outcome) {
            return Err(Refusal::usage_arg(
                "outcome",
                format!("run close takes green, red, stopped or died, not {outcome}"),
            ));
        }
        let tip = self.dag.live(&caller.branch).and_then(|r| r.tip);
        let base = self.dag.state_at(tip, &self.alloc);
        let run = base
            .nodes
            .iter()
            .find(|(_, x)| x.live() && x.kind == "run" && x.text("title") == Some(name))
            .map(|(n, _)| *n)
            .ok_or_else(|| Refusal::not_found("run", name))?;
        drop(base);
        let stmts = vec![Stmt::Set {
            target: Target::Id(run),
            fields: vec![
                ("ended".into(), P::Int(self.env.now_s())),
                ("status".into(), P::Text(outcome.to_string())),
            ],
            incr: Vec::new(),
            body: None,
            guard: None,
        }];
        let before: BTreeSet<u64> = self
            .leases
            .values()
            .filter(|l| l.ended.is_none())
            .map(|l| l.id)
            .collect();
        let mut reply = self.tx(
            &stmts,
            "",
            ctx,
            None,
            None,
            Keying::Given {
                key,
                payload,
                cmd: "RunClose",
                sym: "run close",
            },
            After::RunClose(run),
        )?;
        reply.statements.clear();
        let released: Vec<u64> = if reply.outcome == Outcome::Ok {
            self.leases
                .values()
                .filter(|l| before.contains(&l.id) && l.ended == Some(lease::EndReason::RunClose))
                .map(|l| l.id)
                .collect()
        } else {
            Vec::new()
        };
        reply.data = Data::RunClose(run, outcome.to_string(), released);
        Ok(reply)
    }

    /// `BranchCreate` ([API §11.1]): after the lookup, RN-1 to RN-8, a `RefUpdate` (create) with the fork commit as
    /// the tip.
    // spec: [API §11.1]
    fn branch_create(
        &mut self,
        name: &str,
        from: Option<&str>,
        kind: Option<RefKind>,
        ctx: &Ctx,
    ) -> Res<Reply> {
        let caller = self.resolve_keyed(ctx, false, None)?;
        let mut args = BTreeMap::new();
        args.insert("name".to_string(), Cj::Str(name.to_string()));
        if let Some(f) = from {
            args.insert("from".to_string(), Cj::Str(f.to_string()));
        }
        if let Some(k) = kind {
            args.insert("kind".to_string(), Cj::Str(k.token().to_string()));
        }
        let payload = cmd_payload("BranchCreate", args);
        // The branch of the command is the ref it creates.
        let lookup_name = dag::complete_name(name, false, kind)
            .map(|(n, _)| n)
            .unwrap_or_else(|_| name.to_string());
        let key = self.key_of(ctx, &caller, &lookup_name, &payload);
        if let Some(r) = self.keyed(&key, &payload, &lookup_name, ctx, &caller)? {
            return Ok(r);
        }
        self.rights(&caller, ctx)
            .verb("branch")
            .map_err(|e| e.finish(None))?;
        let (full, _) = dag::check_new_name(name, false, kind)?;
        dag::check_unique(&self.dag, &full)?;
        let src = from.unwrap_or(&caller.branch).to_string();
        self.fork_commit(&src)?;
        let mut reply = Reply::ok(Data::None);
        reply.warnings = caller.warnings.clone();
        reply.branch = Some(full.clone());
        if ctx.dry {
            reply.rev = self.fork_commit(&src).ok();
            reply.outcome = Outcome::Dry;
            return Ok(reply);
        }
        let (full, id, k, fork, markers) = self.create_ref(name, &src, kind, &caller.actor)?;
        reply.rev = Some(fork);
        reply.markers = markers;
        self.record_idem(
            key,
            payload,
            &full,
            None,
            ctx,
            Recorded {
                cmd: "BranchCreate".into(),
                items: vec![ResultItem::RefMove {
                    ref_id: id,
                    reason: MoveReason::Create.code(),
                    old: None,
                    new: Some(fork),
                }],
                yields: Vec::new(),
            },
        );
        reply.data = Data::BranchCreate(full, id, k, Some(fork));
        Ok(reply)
    }

    /// `LaneOpen` ([API §11.5]): one group, in order — a commit on `main` creating the `lane` node (`title` = `name`,
    /// status `active`, `worktree_path` the path value of `worktree` under root `abs`, `git_branch`, `base_sha`,
    /// `moirai_branch` = `lane/<name>`), which carries the idempotency pair; then `BranchCreate(lane/<name>, from:
    /// main)`, which forks the lane from that commit, so the lane holds its own lane node; then `WorktreeBind(worktree,
    /// lane/<name>)` with the designation of [F18 §3.5]. Refusals: `BranchCreate`'s and `WorktreeBind`'s, checked
    /// before anything is written; a `worktree` that is not a tree is usage (exit 2); a `base` that names no commit of
    /// the tree's repository is E301.
    // spec: [API §11.5]
    // rule: WV-004
    fn lane_open(
        &mut self,
        name: &str,
        worktree: &str,
        git_branch: Option<&str>,
        base: Option<&str>,
        ctx: &Ctx,
    ) -> Res<Reply> {
        let caller = self.resolve_keyed(ctx, false, None)?;
        let dir = crate::links::canon_abs(worktree);
        let mut args = BTreeMap::new();
        args.insert("name".to_string(), Cj::Str(name.to_string()));
        args.insert("worktree".to_string(), Cj::Str(dir.clone()));
        if let Some(g) = git_branch {
            args.insert("git_branch".to_string(), Cj::Str(g.to_string()));
        }
        if let Some(b) = base {
            args.insert("base".to_string(), Cj::Str(b.to_string()));
        }
        let payload = cmd_payload("LaneOpen", args);
        let key = self.key_of(ctx, &caller, "main", &payload);
        if let Some(r) = self.keyed(&key, &payload, "main", ctx, &caller)? {
            return Ok(r);
        }
        self.rights(&caller, ctx)
            .verb("lane")
            .map_err(|e| e.finish(None))?;
        let (full, _) = dag::check_new_name(name, false, Some(RefKind::Work))?;
        dag::check_unique(&self.dag, &full)?;
        if !self.files.fs.trees.contains_key(&dir) {
            return Err(Refusal::usage_arg(
                "worktree",
                format!("{dir} is not a tree ([F18 §3.5])"),
            ));
        }
        let base_sha = match base {
            None => None,
            Some(b) => {
                let prefix = b.split_once(':').map_or(b, |(_, h)| h).to_ascii_lowercase();
                let (repo, _) = self
                    .files
                    .git
                    .of_tree(&dir)
                    .ok_or_else(|| Refusal::lq("E301", format!("{dir} has no git history")))?;
                let hits: Vec<&String> = repo
                    .commits
                    .keys()
                    .filter(|id| {
                        id.split_once(':')
                            .map_or(id.as_str(), |(_, h)| h)
                            .starts_with(&prefix)
                    })
                    .collect();
                match hits.as_slice() {
                    [id] => {
                        let hex = id.split_once(':').map_or(id.as_str(), |(_, h)| h);
                        Some(format!("{}:{hex}", repo.algo.name()))
                    }
                    [] => return Err(Refusal::lq("E301", format!("{b} names no commit"))),
                    _ => {
                        return Err(Refusal::usage_arg(
                            "base",
                            format!("{b} names several commits"),
                        ));
                    }
                }
            }
        };
        // `WorktreeBind`'s refusals first: the group writes nothing when the binding would be refused.
        let quiet = Ctx {
            key: None,
            no_dedupe: true,
            ..ctx.clone()
        };
        let main_ctx = Ctx {
            branch: Some("main".into()),
            ..ctx.clone()
        };
        let mut fields = vec![
            ("title".to_string(), P::Text(name.to_string())),
            ("worktree_path".into(), P::Text(format!("abs:{dir}"))),
            ("moirai_branch".into(), P::Text(full.clone())),
        ];
        if let Some(g) = git_branch {
            fields.push(("git_branch".into(), P::Text(g.to_string())));
        }
        if let Some(b) = base_sha {
            fields.push(("base_sha".into(), P::Text(b)));
        }
        let stmts = vec![Stmt::Create {
            name: Some("l".into()),
            kind: "lane".into(),
            fields,
            body: None,
            under: None,
            position: None,
            edges_out: Vec::new(),
            edges_in: Vec::new(),
        }];
        if ctx.dry {
            let mut reply = Reply::ok(Data::None);
            reply.branch = Some("main".into());
            reply.outcome = Outcome::Dry;
            return Ok(reply);
        }
        let lane = Nid(self.next_id);
        let mut reply = self.tx(
            &stmts,
            "",
            &main_ctx,
            None,
            None,
            Keying::Given {
                key,
                payload,
                cmd: "LaneOpen",
                sym: "lane open",
            },
            After::Nothing,
        )?;
        reply.statements.clear();
        let (full, ref_id, _, fork, markers) =
            self.create_ref(name, "main", Some(RefKind::Work), &caller.actor)?;
        reply.markers.extend(markers);
        let bind = self.worktree_bind(&dir, &full, false, &quiet)?;
        let binding = match bind.data {
            Data::Bind(b) => *b,
            other => panic!("WorktreeBind answers with its binding, not {other:?}"),
        };
        reply.warnings.extend(bind.warnings);
        reply.data = Data::LaneOpen(Box::new(LaneOpenData {
            lane,
            ref_: full,
            ref_id,
            fork,
            binding,
        }));
        Ok(reply)
    }

    /// `LaneClose` ([API §11.5]): one commit on `main` setting the lane node's status through the door `lane-close`
    /// ([RULES/status-machines] DR-013, TR-072 to TR-080) — `merged` when tip(`main`) contains the lane's tip, `abandoned`
    /// otherwise, `frozen` for `freeze` — and the removal of the lane's directory binding, in one group. The
    /// idempotency pair is on the commit.
    // spec: [API §11.5]
    // rule: DR-013, WV-004
    fn lane_close(&mut self, name: &str, mode: Option<&str>, ctx: &Ctx) -> Res<Reply> {
        let caller = self.resolve_keyed(ctx, false, None)?;
        let mut args = BTreeMap::new();
        args.insert("name".to_string(), Cj::Str(name.to_string()));
        if let Some(m) = mode.filter(|m| *m != "close") {
            args.insert("mode".to_string(), Cj::Str(m.to_string()));
        }
        let payload = cmd_payload("LaneClose", args);
        let key = self.key_of(ctx, &caller, "main", &payload);
        if let Some(r) = self.keyed(&key, &payload, "main", ctx, &caller)? {
            return Ok(r);
        }
        self.rights(&caller, ctx)
            .verb("lane")
            .map_err(|e| e.finish(None))?;
        let freeze = match mode.unwrap_or("close") {
            "close" => false,
            "freeze" => true,
            m => {
                return Err(Refusal::usage_arg(
                    "mode",
                    format!("lane close takes close or freeze, not {m}"),
                ));
            }
        };
        let full = dag::complete_name(name, false, Some(RefKind::Work))
            .map(|(n, _)| n)
            .unwrap_or_else(|_| format!("lane/{name}"));
        let main_tip = self.dag.live("main").and_then(|r| r.tip);
        let main_st = self.dag.state_at(main_tip, &self.alloc);
        let (lane, dir) = main_st
            .nodes
            .iter()
            .find(|(_, x)| x.live() && x.kind == "lane" && x.text("moirai_branch") == Some(&full))
            .map(|(n, x)| {
                let dir = match x.fields.get("worktree_path") {
                    Some(crate::value::Value::Path(p)) => Some(p.text.clone()),
                    _ => None,
                };
                (*n, dir)
            })
            .ok_or_else(|| Refusal::not_found("lane", name))?;
        drop(main_st);
        let lane_tip = self
            .dag
            .refs
            .values()
            .filter(|r| r.name == full)
            .max_by_key(|r| r.id)
            .and_then(|r| r.tip);
        let status = if freeze {
            "frozen"
        } else if lane_tip.is_none_or(|t| self.dag.ancestors(main_tip).contains(&t)) {
            "merged"
        } else {
            "abandoned"
        };
        let stmts = vec![Stmt::Set {
            target: Target::Id(lane),
            fields: vec![("status".into(), P::Text(status.to_string()))],
            incr: Vec::new(),
            body: None,
            guard: None,
        }];
        let expansion = mutation::Expansion {
            stmts: stmts.clone(),
            text: format!("TX {{ SET {lane}.status = '{status}' }}"),
            lq_stmts: vec![(1, format!("SET {lane}.status = '{status}'"))],
            params: Vec::new(),
            door: Some(crate::status::Door::LaneClose),
        };
        let main_ctx = Ctx {
            branch: Some("main".into()),
            ..ctx.clone()
        };
        if ctx.dry {
            let mut reply = Reply::ok(Data::LaneClose(lane, status.to_string(), dir));
            reply.branch = Some("main".into());
            reply.outcome = Outcome::Dry;
            return Ok(reply);
        }
        let mut reply = self.tx(
            &stmts,
            "",
            &main_ctx,
            None,
            Some(("tx.lane-close", &expansion, &[])),
            Keying::Given {
                key,
                payload,
                cmd: "LaneClose",
                sym: "lane close",
            },
            After::Nothing,
        )?;
        reply.statements.clear();
        let unbound = match &dir {
            Some(d)
                if self
                    .heads
                    .get(crate::heads::HeadKind::Directory, d)
                    .is_some() =>
            {
                self.heads
                    .rows
                    .remove(&(crate::heads::HeadKind::Directory, d.clone()));
                self.hlc.record(self.env.wall_ms);
                Some(d.clone())
            }
            _ => None,
        };
        reply.data = Data::LaneClose(lane, status.to_string(), unbound);
        Ok(reply)
    }

    /// The fork commit a revision names: a live ref's tip, or a commit by its seq `s<seq>` (E301 when it names nothing
    /// or a ref without a commit).
    fn fork_commit(&self, src: &str) -> Res<u64> {
        if let Some(n) = src.strip_prefix('s').and_then(|x| x.parse::<u64>().ok())
            && self.dag.commits.contains_key(&n)
            && !self.pruned.contains(&n)
        {
            return Ok(n);
        }
        self.dag
            .live(src)
            .ok_or_else(|| Refusal::lq("E301", format!("{src} names nothing")))?
            .tip
            .ok_or_else(|| Refusal::lq("E301", format!("{src} has no commit")))
    }

    /// Creates a ref at the commit `src` names ([API §11.1] "Effect"): RN-1 to RN-8, the `RefUpdate` (create) with the
    /// fork commit as the tip and `ref_id` = `next_ref_id`, then the ref group's `Marker` record (ME-007). Returns the
    /// full name, the ref id, the kind, the fork commit and the listed marker entries. `BranchCreate` and `Checkout`
    /// with `branch_new` compose from it.
    pub(crate) fn create_ref(
        &mut self,
        name: &str,
        src: &str,
        kind: Option<RefKind>,
        actor: &str,
    ) -> Res<(String, u32, RefKind, u64, Vec<MarkerOut>)> {
        let (full, k) = dag::check_new_name(name, false, kind)?;
        dag::check_unique(&self.dag, &full)?;
        let fork = self.fork_commit(src)?;
        let id = self.next_ref_id;
        self.next_ref_id += 1;
        let hlc = self.hlc.record(self.env.wall_ms);
        self.dag.refs.insert(
            id,
            Ref {
                id,
                name: full.clone(),
                kind: k,
                tip: Some(fork),
                ref_seq_next: 1,
                fork: Some(fork),
                deleted: false,
                message: None,
                pinned: false,
                moves: vec![RefMove {
                    old: None,
                    new: Some(fork),
                    reason: MoveReason::Create,
                    actor: actor.to_string(),
                    hlc,
                }],
            },
        );
        // ME-007: the fork joins the holders of every closed hold at its fork commit (the ref group's Marker record).
        let st = self.dag.state_at(Some(fork), &self.alloc);
        let y = self.dag.refs[&id].clone();
        let entries = self.markers.fork(
            &self.dag,
            &y,
            &st,
            Group::Move(hlc),
            &mut self.hlc,
            self.env.wall_ms,
        );
        let markers = self.listed(&entries);
        // [LQ/std §2.15]: a create's commit column is the new tip.
        self.feed.event(
            self.commit_seq,
            &full,
            Some(fork),
            None,
            "create",
            "ref",
            full.clone(),
            actor,
            None,
        );
        self.feed_markers(&entries, None);
        Ok((full, id, k, fork, markers))
    }

    /// `dropped` of a `BranchDelete` ([API §11.2]): the commits reachable from the ref's tip and from no other live
    /// ref, and the completions and deletions among them.
    fn dropped(&self, ref_id: u32, tip: Option<u64>) -> (u64, u64, u64) {
        let mine = self.dag.ancestors(tip);
        let others: BTreeSet<u64> = self
            .dag
            .live_refs()
            .filter(|x| x.id != ref_id)
            .flat_map(|x| self.dag.ancestors(x.tip))
            .collect();
        let dropped: Vec<u64> = mine.difference(&others).copied().collect();
        let (mut completions, mut deletions) = (0, 0);
        for c in &dropped {
            for (k, (_, after)) in &self.dag.commits[c].changeset {
                match (k, after) {
                    (
                        Key::Node(_, crate::state::Aspect::Status),
                        KState::Plain(Some(KVal::Status { status, .. })),
                    ) if status == "done" => completions += 1,
                    (
                        Key::Node(_, crate::state::Aspect::Existence),
                        KState::Plain(Some(KVal::Deleted { .. })),
                    ) => deletions += 1,
                    _ => {}
                }
            }
        }
        (dropped.len() as u64, completions, deletions)
    }

    /// The leases a deleted branch releases (LE-008): every task lease on the branch that has not ended, whatever its
    /// liveness (an expired one could otherwise be renewed on a deleted branch, LE-011); role leases hold no node of the
    /// branch and stay (the session role lease's branch "fixes nothing", [API §10.1]; a run role lease ends with its
    /// run). One `Lease` record each, after the Marker record ([F05 §4.7] ref group), each an `end` entry of the feed.
    /// `BranchDelete` and the deletions of `OpRestore` call it. Returns the released lease ids.
    // rule: LE-008
    pub(crate) fn release_branch_leases(&mut self, name: &str) -> Vec<u64> {
        let released: Vec<u64> = self
            .leases
            .values()
            .filter(|l| l.branch == name && l.kind == lease::LeaseKind::Task && l.ended.is_none())
            .map(|l| l.id)
            .collect();
        for id in &released {
            let l = self.leases.get_mut(id).expect("a lease");
            l.ended = Some(lease::EndReason::BranchDeleted);
            let (task, holder) = (l.task, l.holder.clone());
            self.hlc.record(self.env.wall_ms);
            self.feed.event(
                self.commit_seq,
                name,
                None,
                task,
                "end",
                "lease",
                format!("L-{id}"),
                &holder,
                None,
            );
        }
        released
    }

    /// `BranchDelete` ([API §11.2]): after the lookup, the ref marked deleted and the task leases on it released
    /// (LE-008).
    // spec: [API §11.2]
    // rule: LE-008
    fn branch_delete(&mut self, name: &str, force: bool, ctx: &Ctx) -> Res<Reply> {
        let caller = self.resolve_keyed(ctx, false, None)?;
        let mut args = BTreeMap::new();
        args.insert("name".to_string(), Cj::Str(name.to_string()));
        if force {
            args.insert("force".to_string(), Cj::Bool(true));
        }
        let payload = cmd_payload("BranchDelete", args);
        let key = self.key_of(ctx, &caller, name, &payload);
        if let Some(r) = self.keyed(&key, &payload, name, ctx, &caller)? {
            return Ok(r);
        }
        self.rights(&caller, ctx)
            .verb("branch")
            .map_err(|e| e.finish(None))?;
        let r = self
            .dag
            .live(name)
            .ok_or_else(|| Refusal::lq("E301", format!("no live ref {name}")))?
            .clone();
        if name == "main" || !matches!(r.kind, RefKind::Work | RefKind::Plan) {
            return Err(Refusal::usage_arg(
                "name",
                format!("{name} is not deleted with branch -d"),
            ));
        }
        let main_anc = self
            .dag
            .ancestors(self.dag.live("main").and_then(|m| m.tip));
        if !force && r.tip.is_some_and(|t| !main_anc.contains(&t)) {
            return Err(
                Refusal::new("not_merged", 6, format!("{name} is not merged into main"))
                    .key("ref", name),
            );
        }
        let dropped = self.dropped(r.id, r.tip);
        let mut reply = Reply::ok(Data::None);
        reply.warnings = caller.warnings.clone();
        reply.branch = Some(name.to_string());
        reply.rev = Some(r.tip.unwrap_or(0));
        if ctx.dry {
            reply.outcome = Outcome::Dry;
            return Ok(reply);
        }
        let hlc = self.hlc.record(self.env.wall_ms);
        // ME-005: the ref leaves every holder set (the ref group's Marker record precedes its Lease records).
        let entries = self.markers.ref_deleted(
            &self.dag,
            &r,
            Group::Move(hlc),
            &mut self.hlc,
            self.env.wall_ms,
        );
        reply.markers = self.listed(&entries);
        self.feed.event(
            self.commit_seq,
            name,
            None,
            None,
            "delete",
            "ref",
            name.to_string(),
            &caller.actor,
            None,
        );
        self.feed_markers(&entries, None);
        let released = self.release_branch_leases(name);
        let x = self.dag.live_mut(name).expect("live");
        x.deleted = true;
        x.moves.push(RefMove {
            old: r.tip,
            new: None,
            reason: MoveReason::Delete,
            actor: caller.actor.clone(),
            hlc,
        });
        let mut items = vec![ResultItem::RefMove {
            ref_id: r.id,
            reason: MoveReason::Delete.code(),
            old: r.tip,
            new: None,
        }];
        items.extend(released.iter().map(|id| ResultItem::LeaseEnd {
            lease: *id,
            reason: lease::EndReason::BranchDeleted.code(),
        }));
        self.record_idem(
            key,
            payload,
            name,
            None,
            ctx,
            Recorded {
                cmd: "BranchDelete".into(),
                items,
                yields: Vec::new(),
            },
        );
        self.prune_rows();
        reply.data = Data::BranchDelete(name.to_string(), r.id, Some(dropped), released);
        Ok(reply)
    }

    /// The listed entries of a `Marker` record as a result's `markers` ([API §10.8]): `settled`, `deleted` and
    /// `cleared`, by (`#N`, origin ref name, commit, kind).
    // spec: [API §10.8]
    pub fn listed(&self, entries: &[markers::Entry]) -> Vec<MarkerOut> {
        let mut v: Vec<MarkerOut> = entries
            .iter()
            .filter_map(|e| {
                Some(MarkerOut {
                    kind: e.listed()?,
                    id: e.key.0,
                    ref_: self.ref_name(e.key.1),
                    commit: e.key.2,
                    outcome: e.outcome.clone(),
                    cause: e.cause,
                })
            })
            .collect();
        v.sort_by(|a, b| (a.id, &a.ref_, a.commit, a.kind).cmp(&(b.id, &b.ref_, b.commit, b.kind)));
        v
    }

    /// The name of a ref by id; empty for a deleted ref whose row a `gc` run expired ([F11 §3.8]), which only a marker
    /// that a later fork re-emits at an origin on that ref can still name.
    pub(crate) fn ref_name(&self, id: u32) -> String {
        self.dag
            .refs
            .get(&id)
            .map(|r| r.name.clone())
            .unwrap_or_default()
    }

    /// Records the listed entries of a `Marker` record in the change feed ([LQ/std §2.15]): the origin ref and commit,
    /// the entry's actor (a completion's holder, MF-009), else the origin commit's. `group` is the commit whose group
    /// carries the record, `None` for a ref group.
    pub(crate) fn feed_markers(&mut self, entries: &[markers::Entry], group: Option<u64>) {
        for e in entries {
            if let Some(k) = e.listed() {
                let origin = self.ref_name(e.key.1);
                let actor = e.holder.clone().unwrap_or_else(|| {
                    self.dag
                        .commits
                        .get(&e.key.2)
                        .map(|c| c.actor.clone())
                        .unwrap_or_default()
                });
                self.feed.event(
                    self.commit_seq,
                    &origin,
                    Some(e.key.2),
                    Some(e.key.0),
                    k.name(),
                    "marker",
                    format!("s{}", e.key.2),
                    &actor,
                    group,
                );
            }
        }
    }

    /// The runtime snapshot's `markers` ([API §15.7]): every row of `MARKERS` and `MARKERS_OLD`, by (`#N`, origin ref
    /// name, commit, kind), with its holders and the live refs it is active on (MC-4).
    pub fn marker_rows(&self) -> Vec<MarkerSnap> {
        let name = |id: &u32| self.ref_name(*id);
        let mut anc = markers::Anc::default();
        let mut v: Vec<MarkerSnap> = self
            .markers
            .rows()
            .map(|m| {
                let mut holders: Vec<String> = m.holders.iter().map(name).collect();
                holders.sort();
                let mut active_on: Vec<String> = if m.active() {
                    self.dag
                        .live_refs()
                        .filter(|r| !self.markers.absorbed_in(&self.dag, r, m, &mut anc))
                        .map(|r| r.name.clone())
                        .collect()
                } else {
                    Vec::new()
                };
                active_on.sort();
                MarkerSnap {
                    marker: m.clone(),
                    ref_: name(&m.key.1),
                    holders,
                    active_on,
                }
            })
            .collect();
        v.sort_by(|a, b| {
            (a.marker.key.0, &a.ref_, a.marker.key.2, a.marker.kind).cmp(&(
                b.marker.key.0,
                &b.ref_,
                b.marker.key.2,
                b.marker.kind,
            ))
        });
        v
    }

    /// `state(V)` ([API §15]): content, local and derived at a view.
    // spec: [API §15.1]
    pub fn snapshot(&self, ref_: &str, commit: Option<u64>) -> Snapshot {
        let st = self.dag.state_at(commit, &self.alloc);
        let rows = derived::recompute_all(&st, &|_| None);
        let local = self.dag.local_seqs_all(commit);
        let mut nodes: Vec<SnapNode> = st
            .nodes
            .iter()
            .map(|(n, x)| {
                let mut node = x.clone();
                // [API §15.3]: a tombstone keeps its kind, title and retained edges; its status and resolution are the
                // kind's initial values and its header enumerations their defaults (TB-012; spec sync 2b), since the
                // canonical form carries none of them.
                if !node.live() {
                    node.fields.retain(|f, _| f == "title");
                    node.status = st.schema.initial_status(&node.kind).unwrap_or_default();
                    node.resolution = "none".into();
                }
                SnapNode {
                    id: *n,
                    node,
                    local: local.get(n).copied().unwrap_or((0, 0, 0)),
                    derived: rows.get(n).cloned(),
                }
            })
            .collect();
        nodes.sort_by_key(|x| x.node.uid);
        Snapshot {
            ref_: Some(ref_.to_string()),
            commit,
            parts: [true; 3],
            schema: st
                .schema
                .items
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            nodes,
        }
    }

    /// The runtime snapshot ([API §15.7]); `exclusions` from I26′'s definition (MC-7).
    // spec: [API §15.7]
    pub fn runtime(&self) -> RuntimeSnap {
        let mut leases: Vec<LeaseSnap> = self
            .leases
            .values()
            .filter(|l| l.ended.is_none())
            .map(|l| LeaseSnap {
                lease: l.clone(),
                live: lease::is_live(l, &self.env),
            })
            .collect();
        leases.sort_by_key(|x| (x.lease.task.map_or(0, |t| t.0), x.lease.id));
        let mut exclusions = Vec::new();
        let mut o = Oracle::new(&self.dag, &self.alloc);
        let mut refs: Vec<&Ref> = self.dag.live_refs().collect();
        refs.sort_by(|a, b| a.name.cmp(&b.name));
        // The kind of each allocated `#N`, fixed at its creation: the pairs listed are of tasks only ([API §15.7]), even
        // where the node is absent on R and only a holder's state has it.
        let kinds: BTreeMap<Nid, String> = self
            .alloc
            .rows
            .iter()
            .filter_map(
                |(n, (_, _, _, seq))| match self.dag.hold_keys_at(Some(*seq), *n).0 {
                    KState::Plain(Some(KVal::Live(k) | KVal::Deleted { kind: k, .. })) => {
                        Some((*n, k))
                    }
                    _ => None,
                },
            )
            .collect();
        for r in refs {
            for n in kinds.iter().filter(|(_, k)| *k == "task").map(|(n, _)| *n) {
                let held = o.holders_elsewhere(&r.name, n);
                if !held.is_empty() {
                    exclusions.push((r.name.clone(), n, held.into_iter().map(|h| h.0).collect()));
                }
            }
        }
        RuntimeSnap {
            counters: (
                self.commit_seq,
                self.next_id,
                self.next_anchor,
                self.fence,
                self.next_ref_id,
            ),
            refs: self.dag.refs.values().cloned().collect(),
            moves: self.dag.moves(),
            leases,
            exclusions,
            absorbed: self.markers.absorbed.clone(),
            markers: self.marker_rows(),
            heads: self
                .heads
                .listed(self.windows().retention_ms, &self.hlc, self.env.wall_ms)
                .into_iter()
                .cloned()
                .collect(),
            idem: self
                .idem
                .live(self.env.wall_ms, &self.hlc, self.windows())
                .into_iter()
                .map(|(k, e)| (k, e.clone()))
                .collect(),
            alloc: self
                .alloc
                .rows
                .iter()
                .map(|(n, (u, _, r, s))| (*n, *u, r.clone(), *s))
                .collect(),
            intents: self.listed_intents(),
            quiet: self.quiet,
        }
    }
}

/// The node a changeset creates with a kind: its existence key goes from absent to live of that kind.
fn created_of_kind(cs: &Changeset, kind: &str) -> Option<Nid> {
    cs.iter().find_map(|(k, (before, after))| match (k, after) {
        (Key::Node(n, crate::state::Aspect::Existence), KState::Plain(Some(KVal::Live(x))))
            if x == kind && *before == KState::ABSENT =>
        {
            Some(*n)
        }
        _ => None,
    })
}

/// The completions a group's lease events carry (MF-009): for each lease ended into `settled`, its task, its holder
/// and the outcome of the `tx.complete` that ended it.
fn completions(
    events: &[LeaseEvent],
    leases: &BTreeMap<u64, Lease>,
    yields: &[Yield],
) -> Vec<markers::Completion> {
    events
        .iter()
        .filter_map(|e| match e {
            LeaseEvent::End {
                id,
                reason: lease::EndReason::Complete,
            } => {
                let l = leases.get(id)?;
                let task = l.task?;
                let outcome = yields
                    .iter()
                    .filter(|y| y.proc == "tx.complete")
                    .flat_map(|y| &y.rows)
                    .find(|row| {
                        row.iter()
                            .any(|(k, v)| k == "task" && *v == task.to_string())
                    })
                    .and_then(|row| row.iter().find(|(k, _)| k == "outcome"))
                    .map(|(_, v)| v.clone())
                    .unwrap_or_else(|| "done".into());
                Some((task, l.holder.clone(), outcome))
            }
            _ => None,
        })
        .collect()
}

/// Whether a command is of the bulk class ([API §9.10]) by its arguments alone: the merge family, and `Mutation`
/// `tx.rm` with `policy` = `cascade` through the CLI; a `FileMv` of a directory is by the tree
/// ([`Store::moves_a_directory`]); `Migrate` and `ImageImport` join with their packages.
pub fn bulk_class(cmd: &Cmd, ctx: &Ctx) -> bool {
    match cmd {
        Cmd::Merge { .. }
        | Cmd::MergeContinue { .. }
        | Cmd::Sync { check: false, .. }
        | Cmd::Revert { .. }
        | Cmd::CherryPick { .. } => true,
        Cmd::Mutation { name, params, .. } => {
            name == "tx.rm"
                && ctx.door == Door::Cli
                && params
                    .iter()
                    .any(|(k, v)| k == "policy" && *v == P::Text("cascade".into()))
        }
        _ => false,
    }
}

/// The command a file named mutation is ([API §9.7]: `tx.link_file`, `tx.unlink_file`, `tx.record_move`,
/// `tx.links_fix`, `tx.links_sync` are §12's commands), with its parameters of [LQ/std §7.4]; `None` for any other name.
fn file_mutation(name: &str, params: &[(String, P)]) -> Res<Option<Cmd>> {
    let get = |k: &str| params.iter().find(|(n, _)| n == k).map(|(_, v)| v);
    let text = |k: &str| -> Option<String> {
        match get(k) {
            Some(P::Text(s)) => Some(s.clone()),
            _ => None,
        }
    };
    let node = |k: &str| -> Res<Target> {
        match get(k) {
            Some(P::Text(s)) => tx::parse_node(s),
            Some(P::Int(i)) if *i > 0 => Some(Target::Id(Nid(*i as u32))),
            _ => None,
        }
        .ok_or_else(|| Refusal::usage_arg(k, format!("{name} needs {k}")))
    };
    let flag = |k: &str| matches!(get(k), Some(P::Bool(true)));
    Ok(Some(match name {
        "tx.link_file" => Cmd::LinkFile {
            node: node("node")?,
            specs: vec![
                text("spec")
                    .ok_or_else(|| Refusal::usage_arg("spec", "tx.link_file needs spec"))?,
            ],
            watch: text("watch"),
            planned: flag("planned"),
            quote: text("quote"),
            end: text("end"),
        },
        "tx.unlink_file" => Cmd::UnlinkFile {
            node: node("node")?,
            anchor: text("anchor"),
            path: text("path"),
        },
        "tx.record_move" => Cmd::FileRelink {
            from: text("from")
                .ok_or_else(|| Refusal::usage_arg("from", "tx.record_move needs from"))?,
            to: text("to").ok_or_else(|| Refusal::usage_arg("to", "tx.record_move needs to"))?,
        },
        "tx.links_fix" => Cmd::LinksFix {
            target: text("target")
                .ok_or_else(|| Refusal::usage_arg("target", "tx.links_fix needs target"))?,
            action: text("action")
                .ok_or_else(|| Refusal::usage_arg("action", "tx.links_fix needs action"))?,
            expect: text("expect"),
            to: text("to"),
            at: text("at"),
            same_as: get("same_as").map(|_| node("same_as")).transpose()?,
            reason: None,
            replaced_by: None,
            from: None,
        },
        "tx.links_sync" => Cmd::LinksSync {
            scope: get("scope").map(|_| node("scope")).transpose()?,
            budget_ms: match get("budget_ms") {
                Some(P::Int(i)) if *i >= 0 => Some(*i as u64),
                _ => None,
            },
            since: None,
            deep: false,
            all: false,
            force: false,
        },
        _ => return Ok(None),
    }))
}

fn target_text(t: &Target) -> String {
    match t {
        Target::Id(n) => n.to_string(),
        Target::Uid(u) => format!("#u:{}", u.hex()),
        Target::Var(v) => format!("${v}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payloads_are_over_the_arguments_as_given() {
        let with = |from: Option<&str>| {
            let mut a = BTreeMap::new();
            a.insert("name".to_string(), Cj::Str("a".into()));
            if let Some(f) = from {
                a.insert("from".to_string(), Cj::Str(f.into()));
            }
            cmd_payload("BranchCreate", a)
        };
        assert_ne!(
            with(None),
            with(Some("main")),
            "an omitted argument stays omitted"
        );
        assert_eq!(
            p_cj(&P::List(vec![P::Int(1), P::Text("x".into()), P::Null])).text(),
            "[1,\"x\",null]"
        );
    }

    #[test]
    fn a_created_node_is_found_by_its_existence_key() {
        let mut cs = Changeset::new();
        cs.insert(
            Key::Node(Nid(4), crate::state::Aspect::Existence),
            (
                KState::ABSENT,
                KState::Plain(Some(KVal::Live("run".into()))),
            ),
        );
        assert_eq!(created_of_kind(&cs, "run"), Some(Nid(4)));
        assert_eq!(created_of_kind(&cs, "task"), None);
    }

    #[test]
    fn keyed_commands_are_looked_up_before_their_preconditions() {
        let mut st = Store::new();
        st.run(
            &Cmd::Init {
                seed: 7,
                params: vec![
                    "store.log-extent-bytes=64KiB".into(),
                    "store.commit.inline-max-bytes=4KiB".into(),
                ],
                default_branch: None,
            },
            &Ctx::default(),
        );
        let ctx = Ctx {
            agent: Some("a".into()),
            client: Some("claude".into()),
            ..Ctx::default()
        };
        let note = Cmd::Tx {
            stmts: vec![Stmt::Create {
                name: None,
                kind: "note".into(),
                fields: vec![("title".into(), P::Text("n".into()))],
                body: None,
                under: None,
                position: None,
                edges_out: vec![],
                edges_in: vec![],
            }],
            message: String::new(),
        };
        assert_eq!(st.run(&note, &ctx).outcome, Outcome::Ok);
        let again = st.run(&note, &ctx);
        assert_eq!(again.outcome, Outcome::Replayed);
        assert_eq!(again.rev_new, Some(1));
        assert_eq!(st.idem.entries.len(), 1);
    }
}
