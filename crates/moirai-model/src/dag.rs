//! Commits and refs of the model ([AR §5a.1]–§5a.3; [F06 §4.3]; [F11 §3]; [F12 §2]): every commit keeps its net
//! changeset against its first parent, and the state of a commit is the fold of the changesets along its first-parent
//! chain ([60 §4.2] `state_at`; [AR §4.6] "Net changeset = state diff"). Refs carry their kind, tip, `ref_seq_next`,
//! reflog and deletion; the absorbed vector is computed by its definition (VR-001).
//!
//! `state_at` is memoised within a bounded cache: the tips of the live refs, at most [`MAX_CHECKPOINTS`] checkpoint
//! states over all chains together (commits whose first-parent depth is a multiple of an interval that doubles as the
//! deepest chain grows, the least recently used dropped first) and the [`RECENT`] states read last; any other state
//! is re-folded from its nearest cached ancestor. So the cache holds at most (live refs + [`MAX_CHECKPOINTS`] +
//! [`RECENT`]) whole states, however many chains the history has, and the checkpoint part is independent of the number
//! of lanes ([PLAN] WP-91: ≤ 512 MB per case). A key's value at a commit is also read without a state: the newest
//! commit of the first-parent chain that changed it, found through an index of the commits that change each node's
//! existence and status and first-parent jump pointers ([`Dag::hold_keys_at`]).
//!
//! The merge engine, LCAs, the recursive virtual base, revert and cherry-pick, the canonical encoder and commit ids are
//! WP-91's; a commit is identified here by its store sequence number.

use crate::err::{Refusal, Res};
use crate::state::{Alloc, Aspect, Changeset, EdgeKey, KState, Key, State};
use crate::status::History;
use crate::value::Nid;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::mem::size_of;
use std::rc::Rc;

/// The most checkpoint states the cache keeps, over all chains together.
pub const MAX_CHECKPOINTS: usize = 32;

/// The number of recently read states the cache keeps besides tips and checkpoints.
pub const RECENT: usize = 8;

/// A ref's kind, fixed by its namespace ([F12 §2.2]).
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub enum RefKind {
    /// 1: `main`, `lane/*`.
    Work,
    /// 2: `plan/*`.
    Plan,
    /// 3: `merge/*` staging refs.
    Merge,
    /// 4: `import/*`.
    Import,
    /// 5: `tags/*`.
    Tag,
    /// 6: `orphans/*`.
    Orphans,
}

impl RefKind {
    /// The kind of a name by its namespace.
    pub fn of(name: &str) -> RefKind {
        match name.split('/').next().unwrap_or("") {
            "plan" => RefKind::Plan,
            "merge" => RefKind::Merge,
            "import" => RefKind::Import,
            "tags" => RefKind::Tag,
            "orphans" => RefKind::Orphans,
            _ => RefKind::Work,
        }
    }

    /// The token of [RULES/state-definition] `view-kinds` and [RULES/status-machines] `branch-mask`.
    pub fn token(self) -> &'static str {
        match self {
            RefKind::Work => "work",
            RefKind::Plan => "plan",
            RefKind::Merge => "merge",
            RefKind::Import => "import",
            RefKind::Tag => "tag",
            RefKind::Orphans => "orphans",
        }
    }
}

/// The reason of a ref move ([F05 §9.2] field 1).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MoveReason {
    /// A commit landed.
    Commit,
    /// 1 create.
    Create,
    /// 2 delete.
    Delete,
    /// 3 undo.
    Undo,
    /// 4 op restore.
    OpRestore,
}

impl MoveReason {
    /// The reason byte of [F05 §9.2] field 1; 0 for a move a commit carries.
    pub fn code(self) -> u8 {
        match self {
            MoveReason::Commit => 0,
            MoveReason::Create => 1,
            MoveReason::Delete => 2,
            MoveReason::Undo => 3,
            MoveReason::OpRestore => 4,
        }
    }

    /// The name of [API §15.7] `moves`.
    pub fn name(self) -> &'static str {
        match self {
            MoveReason::Commit => "commit",
            MoveReason::Create => "create",
            MoveReason::Delete => "delete",
            MoveReason::Undo => "undo",
            MoveReason::OpRestore => "op-restore",
        }
    }
}

/// One entry of a ref's reflog: (old tip, new tip, reason, store seq of the move, HLC).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RefMove {
    /// The tip before.
    pub old: Option<u64>,
    /// The tip after.
    pub new: Option<u64>,
    /// Why.
    pub reason: MoveReason,
    /// The actor.
    pub actor: String,
    /// The HLC of the move's record.
    pub hlc: u64,
}

/// One ref ([F11 §3]).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Ref {
    /// `ref_id`, never reused; `main` is 0.
    pub id: u32,
    /// The name.
    pub name: String,
    /// The kind.
    pub kind: RefKind,
    /// The tip commit's seq, or `None` for a ref with no commit.
    pub tip: Option<u64>,
    /// The next `ref_seq` of a commit landing on the ref.
    pub ref_seq_next: u64,
    /// The fork commit.
    pub fork: Option<u64>,
    /// Deleted.
    pub deleted: bool,
    /// A tag's message.
    pub message: Option<String>,
    /// A tag's pin.
    pub pinned: bool,
    /// The reflog, oldest first.
    pub moves: Vec<RefMove>,
}

/// One commit's header fields the model keeps ([F06 §4.3]; [API §15.8]) and its net changeset.
#[derive(Clone, PartialEq, Debug)]
pub struct Commit {
    /// Store `seq`, from 1.
    pub seq: u64,
    /// The ref it landed on.
    pub ref_id: u32,
    /// Its position on that ref.
    pub ref_seq: u64,
    /// The commit kind ([F06 §3.1]).
    pub kind: &'static str,
    /// Parents in stated order.
    pub parents: Vec<u64>,
    /// `hlc`, which is also `append_hlc` for a local commit.
    pub hlc: u64,
    /// `append_hlc`.
    pub append_hlc: u64,
    /// The actor.
    pub actor: String,
    /// `actor_src`.
    pub actor_src: String,
    /// The role; empty with no lease.
    pub role: String,
    /// The session identity; empty with none.
    pub session: String,
    /// The message, normalised by [F07 §5.1]; empty for the absent `msg` group.
    pub message: String,
    /// `stmt_origin` ([F06 §3.4]).
    pub stmt_origin: &'static str,
    /// `stmt_sym`.
    pub stmt_sym: Option<String>,
    /// `stmt_hash`: `H` of the block.
    pub stmt_hash: Option<[u8; 16]>,
    /// The idempotency pair (`idem_key`, `idem_payload`).
    pub idem: Option<([u8; 16], [u8; 16])>,
    /// The net changeset against the first parent.
    pub changeset: Changeset,
    /// `affected`.
    pub affected: Vec<Nid>,
    /// `affected_complete`.
    pub affected_complete: bool,
    /// `commit_id`: BLAKE3-256 of the canonical form ([F07 §3]); set when the commit is appended.
    pub id: [u8; 32],
    /// `changeset_digest` ([F07 §10.4]).
    pub digest: [u8; 32],
    /// `gen` = 1 + the greatest `gen` of the parents, 1 for a root ([F12 §4.1]).
    pub generation: u32,
    /// The reverted or picked commit (canonical item 8).
    pub origin: Option<u64>,
    /// `sync_base`: the second parent of a `sync`.
    pub sync_base: Option<u64>,
    /// The git provenance group (canonical item 5): the resolved tree's simulated git state ([API §4.4], §6.6), absent
    /// when the tree has no git.
    pub git: Option<crate::canon::Git>,
    /// `schema_version` (canonical item 7): 1 in format v1.
    pub schema_version: u32,
    /// The `Violation` ops of a staged commit ([F06 §7.7]; [F12 §9.2]), in emission order; never hashed, never
    /// exported; empty on every other commit.
    pub violations: Vec<crate::merge::Violation>,
    /// The keys the commit's `Resolve` ops name ([F06 §7.7]; [F12 §6.5]), in key order, a resolution that left its
    /// key's value as it was included: `merge --continue` overlays each one ([F12 §9.4] step 2).
    pub resolves: Vec<Key>,
    /// The `stage` group of a staged `merge` or `sync` ([F06 §4.4.16]): the arguments `merge --continue` recomputes
    /// with ([F12 §9.4] step 1); `None` for the absent group (no `--base`, no override, `strict` false). Never hashed.
    pub stage: Option<Stage>,
}

/// A staged merge's arguments ([F06 §4.4.16] `stage`): its `--base` commit, its policy override and its effective
/// `strict`. A staged step-0 sync records the merge's override and `strict` but never its `--base` ([F12 §9.6]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stage {
    /// `base`: the `--base` commit (`sgflags` bit 3).
    pub base: Option<u64>,
    /// The policy override: `delete-wins` or `resurrect` (`sgflags` bits 1–2).
    pub policy: Option<String>,
    /// `strict` (`sgflags` bit 0): `--strict`, else `merge.strict`.
    pub strict: bool,
}

impl Stage {
    /// The group a staged commit records for these arguments: `None` when all three are absent or false ([F06 §4.4.16]
    /// "not 0").
    pub fn of(base: Option<u64>, policy: Option<&str>, strict: bool) -> Option<Stage> {
        (base.is_some() || policy.is_some() || strict).then(|| Stage {
            base,
            policy: policy.map(str::to_string),
            strict,
        })
    }
}

impl Commit {
    /// A commit with its header fields and changeset, identity fields zero until [`Dag::identify`] sets them.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        seq: u64,
        ref_id: u32,
        ref_seq: u64,
        kind: &'static str,
        parents: Vec<u64>,
        hlc: u64,
        changeset: Changeset,
    ) -> Commit {
        Commit {
            seq,
            ref_id,
            ref_seq,
            kind,
            parents,
            hlc,
            append_hlc: hlc,
            actor: String::new(),
            actor_src: String::new(),
            role: String::new(),
            session: String::new(),
            message: String::new(),
            stmt_origin: "tx",
            stmt_sym: None,
            stmt_hash: None,
            idem: None,
            changeset,
            affected: Vec::new(),
            affected_complete: true,
            id: [0; 32],
            digest: [0; 32],
            generation: 0,
            origin: None,
            sync_base: None,
            git: None,
            schema_version: 1,
            violations: Vec::new(),
            resolves: Vec::new(),
            stage: None,
        }
    }
}

/// The bounded `state_at` cache.
#[derive(Clone, Debug, Default)]
struct Cache {
    /// The cached states by commit.
    states: BTreeMap<u64, Rc<State>>,
    /// The commits read last, newest at the back.
    recent: VecDeque<u64>,
    /// The checkpoint commits whose states are kept, least recently used at the front; at most [`MAX_CHECKPOINTS`].
    checkpoints: VecDeque<u64>,
}

/// The commit DAG and the refs of one store.
#[derive(Debug, Default)]
pub struct Dag {
    /// Every commit by seq; commits are added through [`Dag::insert`].
    pub commits: BTreeMap<u64, Commit>,
    /// Every ref ever created, by id.
    pub refs: BTreeMap<u32, Ref>,
    cache: RefCell<Cache>,
    /// First-parent depth of each commit (a root is 0).
    fp_depth: BTreeMap<u64, u32>,
    /// The greatest first-parent depth, kept as commits are inserted.
    max_depth: u32,
    /// First-parent jump pointers: entry k is the 2^k-th first-parent ancestor.
    fp_jump: BTreeMap<u64, Vec<u64>>,
    /// For each node, the commits (ascending) whose changesets change its existence or status.
    hold_ix: BTreeMap<Nid, Vec<u64>>,
}

impl Clone for Dag {
    fn clone(&self) -> Dag {
        Dag {
            commits: self.commits.clone(),
            refs: self.refs.clone(),
            cache: RefCell::new(self.cache.borrow().clone()),
            fp_depth: self.fp_depth.clone(),
            max_depth: self.max_depth,
            fp_jump: self.fp_jump.clone(),
            hold_ix: self.hold_ix.clone(),
        }
    }
}

impl Dag {
    /// The live ref with this name.
    pub fn live(&self, name: &str) -> Option<&Ref> {
        self.refs.values().find(|r| !r.deleted && r.name == name)
    }

    /// The live ref with this name, mutably.
    pub fn live_mut(&mut self, name: &str) -> Option<&mut Ref> {
        self.refs
            .values_mut()
            .find(|r| !r.deleted && r.name == name)
    }

    /// Every live ref, by id.
    pub fn live_refs(&self) -> impl Iterator<Item = &Ref> {
        self.refs.values().filter(|r| !r.deleted)
    }

    /// Adds a commit and indexes it: its first-parent depth and jump pointers, and the nodes whose existence or status
    /// its changeset changes. Its parents must be in the DAG already.
    pub fn insert(&mut self, mut c: Commit) {
        let seq = c.seq;
        // gen = 1 + max gen of the actual parents, 1 for a root ([F12 §4.1]).
        c.generation = 1 + c
            .parents
            .iter()
            .filter_map(|p| self.commits.get(p).map(|x| x.generation))
            .max()
            .unwrap_or(0);
        let parent = c.parents.first().copied();
        let depth = parent.map_or(0, |p| self.fp_depth[&p] + 1);
        let mut jumps = Vec::new();
        if let Some(p) = parent {
            jumps.push(p);
            let mut k = 0;
            while let Some(next) = self.fp_jump.get(&jumps[k]).and_then(|j| j.get(k)).copied() {
                jumps.push(next);
                k += 1;
            }
        }
        self.fp_depth.insert(seq, depth);
        self.max_depth = self.max_depth.max(depth);
        self.fp_jump.insert(seq, jumps);
        for k in c.changeset.keys() {
            if let Key::Node(n, Aspect::Existence | Aspect::Status) = k {
                let v = self.hold_ix.entry(*n).or_default();
                if v.last() != Some(&seq) {
                    v.push(seq);
                }
            }
        }
        self.commits.insert(seq, c);
    }

    /// Whether `d` lies on the first-parent chain of `c` (`d = c` included).
    pub fn on_fp_chain(&self, d: u64, c: u64) -> bool {
        let (Some(&dd), Some(&dc)) = (self.fp_depth.get(&d), self.fp_depth.get(&c)) else {
            return false;
        };
        if dd > dc {
            return false;
        }
        let mut cur = c;
        let mut up = dc - dd;
        let mut k = 0;
        while up > 0 {
            if up & 1 == 1 {
                cur = self.fp_jump[&cur][k];
            }
            up >>= 1;
            k += 1;
        }
        cur == d
    }

    /// The existence and status key states of node `n` at commit `c` ([`Key::Node`] with [`Aspect::Existence`] and
    /// [`Aspect::Status`]): each is the `after` state of the newest commit of `c`'s first-parent chain that changed it,
    /// or absent — the value `state_at(c)` holds for the key, read without a state.
    pub fn hold_keys_at(&self, c: Option<u64>, n: Nid) -> (KState, KState) {
        let (mut exist, mut status) = (None, None);
        if let (Some(c), Some(v)) = (c, self.hold_ix.get(&n)) {
            for d in v.iter().rev().filter(|d| **d <= c) {
                if exist.is_some() && status.is_some() {
                    break;
                }
                if !self.on_fp_chain(*d, c) {
                    continue;
                }
                let cs = &self.commits[d].changeset;
                if exist.is_none()
                    && let Some((_, after)) = cs.get(&Key::Node(n, Aspect::Existence))
                {
                    exist = Some(after.clone());
                }
                if status.is_none()
                    && let Some((_, after)) = cs.get(&Key::Node(n, Aspect::Status))
                {
                    status = Some(after.clone());
                }
            }
        }
        (
            exist.unwrap_or(KState::ABSENT),
            status.unwrap_or(KState::ABSENT),
        )
    }

    /// The checkpoint interval: the least power of two ≥ 64 that puts at most [`MAX_CHECKPOINTS`] checkpoint depths on
    /// the deepest first-parent chain.
    fn interval(&self) -> u32 {
        let mut i: u32 = 64;
        while self.max_depth / i >= MAX_CHECKPOINTS as u32 {
            i = i.saturating_mul(2);
        }
        i
    }

    /// Whether a commit lies at a checkpoint depth.
    fn checkpoint(&self, seq: u64, interval: u32) -> bool {
        self.fp_depth
            .get(&seq)
            .is_some_and(|d| *d > 0 && d % interval == 0)
    }

    /// Keeps only the live refs' tips, the [`RECENT`] states read last and at most [`MAX_CHECKPOINTS`] checkpoints: those
    /// still at a checkpoint depth of the current interval, the least recently used dropped first.
    fn prune(&self, cache: &mut Cache) {
        let interval = self.interval();
        let tips: BTreeSet<u64> = self.live_refs().filter_map(|r| r.tip).collect();
        while cache.recent.len() > RECENT {
            cache.recent.pop_front();
        }
        cache.checkpoints.retain(|c| self.checkpoint(*c, interval));
        while cache.checkpoints.len() > MAX_CHECKPOINTS {
            cache.checkpoints.pop_front();
        }
        let recent: BTreeSet<u64> = cache.recent.iter().copied().collect();
        let checkpoints: BTreeSet<u64> = cache.checkpoints.iter().copied().collect();
        cache
            .states
            .retain(|s, _| tips.contains(s) || recent.contains(s) || checkpoints.contains(s));
    }

    fn touch(cache: &mut Cache, seq: u64) {
        if cache.recent.back() != Some(&seq) {
            cache.recent.retain(|x| *x != seq);
            cache.recent.push_back(seq);
        }
    }

    /// Marks a checkpoint state as used last: kept, or moved to the back of the checkpoints' order.
    fn use_checkpoint(cache: &mut Cache, seq: u64) {
        if cache.checkpoints.back() != Some(&seq) {
            cache.checkpoints.retain(|x| *x != seq);
            cache.checkpoints.push_back(seq);
        }
    }

    /// The state of a commit: the fold of the changesets along its first-parent chain; the empty state for `None`.
    // spec: [60 §4.2] state_at
    pub fn state_at(&self, seq: Option<u64>, alloc: &dyn Alloc) -> Rc<State> {
        let Some(seq) = seq else {
            return Rc::new(State::default());
        };
        let interval = self.interval();
        {
            let mut cache = self.cache.borrow_mut();
            if let Some(s) = cache.states.get(&seq).cloned() {
                Dag::touch(&mut cache, seq);
                if self.checkpoint(seq, interval) {
                    Dag::use_checkpoint(&mut cache, seq);
                }
                return s;
            }
        }
        // Walk back to the nearest cached ancestor, then fold forward, keeping the checkpoints met on the way.
        let mut chain = Vec::new();
        let mut cur = Some(seq);
        let mut base: Option<Rc<State>> = None;
        while let Some(c) = cur {
            let mut cache = self.cache.borrow_mut();
            if let Some(s) = cache.states.get(&c).cloned() {
                if self.checkpoint(c, interval) {
                    Dag::use_checkpoint(&mut cache, c);
                }
                base = Some(s);
                break;
            }
            chain.push(c);
            cur = self
                .commits
                .get(&c)
                .and_then(|x| x.parents.first().copied());
        }
        let mut st = base.map_or_else(State::default, |b| (*b).clone());
        let mut out = None;
        for c in chain.into_iter().rev() {
            st.apply(&self.commits[&c].changeset, alloc);
            if c == seq {
                out = Some(Rc::new(std::mem::take(&mut st)));
            } else if self.checkpoint(c, interval) {
                let mut cache = self.cache.borrow_mut();
                cache.states.insert(c, Rc::new(st.clone()));
                Dag::use_checkpoint(&mut cache, c);
            }
        }
        let out = out.expect("the chain ends at the commit asked for");
        let mut cache = self.cache.borrow_mut();
        cache.states.insert(seq, out.clone());
        Dag::touch(&mut cache, seq);
        if self.checkpoint(seq, interval) {
            Dag::use_checkpoint(&mut cache, seq);
        }
        self.prune(&mut cache);
        out
    }

    /// The state of a commit folded from the empty state along its whole first-parent chain, with no cache: the
    /// definition that the cached [`Dag::state_at`] must equal.
    pub fn state_from_scratch(&self, seq: Option<u64>, alloc: &dyn Alloc) -> State {
        let mut st = State::default();
        for c in self.chain(seq).into_iter().rev() {
            st.apply(&self.commits[&c].changeset, alloc);
        }
        st
    }

    /// Records the state of a new commit, its candidate, so the next read needs no fold. That the fold of its net
    /// changeset equals the candidate is the property `apply(a, diff(a, b)) = b` ([`crate::state`]'s tests) and is
    /// checked against [`Dag::state_from_scratch`] by the stream properties.
    pub fn remember(&self, seq: u64, st: Rc<State>) {
        let interval = self.interval();
        let mut cache = self.cache.borrow_mut();
        cache.states.insert(seq, st);
        Dag::touch(&mut cache, seq);
        if self.checkpoint(seq, interval) {
            Dag::use_checkpoint(&mut cache, seq);
        }
        self.prune(&mut cache);
    }

    /// The number of states the cache holds.
    pub fn cached_states(&self) -> usize {
        self.cache.borrow().states.len()
    }

    /// The commits at a checkpoint depth of the current interval, over every chain: the states a cache that kept every
    /// chain's checkpoints would hold.
    pub fn checkpoint_depth_commits(&self) -> usize {
        let interval = self.interval();
        self.fp_depth
            .keys()
            .filter(|c| self.checkpoint(**c, interval))
            .count()
    }

    /// The number of checkpoint states the cache holds; at most [`MAX_CHECKPOINTS`].
    pub fn checkpoint_states(&self) -> usize {
        let cache = self.cache.borrow();
        cache
            .checkpoints
            .iter()
            .filter(|c| cache.states.contains_key(c))
            .count()
    }

    /// The bytes the `state_at` cache holds ([`crate::heap`]): each distinct cached state once (a state the cache shares
    /// with a caller counts here), its map slots, and the recent list.
    pub fn cache_bytes(&self) -> usize {
        let cache = self.cache.borrow();
        let mut seen = BTreeSet::new();
        let states: usize = cache
            .states
            .values()
            .filter(|s| seen.insert(Rc::as_ptr(s)))
            .map(|s| crate::heap::state_bytes(s))
            .sum();
        let slots = cache.states.len()
            * (size_of::<(u64, Rc<State>)>() * crate::heap::BTREE_SLOT_NUM
                / crate::heap::BTREE_SLOT_DEN
                + crate::heap::BTREE_ENTRY_EXTRA);
        states + slots + (cache.recent.capacity() + cache.checkpoints.capacity()) * size_of::<u64>()
    }

    /// The bytes the commits and the DAG's indexes hold ([`crate::heap`]): every commit with its net changeset, the
    /// first-parent depths and jump pointers, and the hold index.
    pub fn commit_bytes(&self) -> usize {
        use crate::heap::Heap;
        self.commits.heap()
            + self.fp_depth.heap()
            + self.fp_jump.heap()
            + self.hold_ix.heap()
            + self
                .refs
                .values()
                .map(|r| r.moves.capacity() * size_of::<RefMove>())
                .sum::<usize>()
    }

    /// `anc*(c)`: the commit and every ancestor.
    pub fn ancestors(&self, seq: Option<u64>) -> BTreeSet<u64> {
        let mut out = BTreeSet::new();
        let mut stack: Vec<u64> = seq.into_iter().collect();
        while let Some(c) = stack.pop() {
            if out.insert(c)
                && let Some(x) = self.commits.get(&c)
            {
                stack.extend(x.parents.iter().copied());
            }
        }
        out
    }

    /// The first-parent chain of a commit, newest first.
    pub fn chain(&self, seq: Option<u64>) -> Vec<u64> {
        let mut v = Vec::new();
        let mut cur = seq;
        while let Some(c) = cur {
            v.push(c);
            cur = self
                .commits
                .get(&c)
                .and_then(|x| x.parents.first().copied());
        }
        v
    }

    /// VR-001: `V(c)[Y] = max{ref_seq(d) : d ∈ anc*(c), ref(d) = Y}`: the absorbed vector of a commit, by ref id.
    // rule: VR-001
    pub fn absorbed(&self, seq: Option<u64>) -> BTreeMap<u32, u64> {
        let mut v: BTreeMap<u32, u64> = BTreeMap::new();
        for c in self.ancestors(seq) {
            let x = &self.commits[&c];
            let e = v.entry(x.ref_id).or_insert(0);
            *e = (*e).max(x.ref_seq);
        }
        v
    }

    /// The local seqs of every node at a commit ([API §15.4]): `rev`, `created`, `updated`, each the seq of the newest
    /// commit of the first-parent chain whose net changeset changes the named keys; 0 when none. One walk of the chain.
    // spec: [API §15.4]
    pub fn local_seqs_all(&self, seq: Option<u64>) -> BTreeMap<Nid, (u64, u64, u64)> {
        let mut out: BTreeMap<Nid, (u64, u64, u64)> = BTreeMap::new();
        for c in self.chain(seq) {
            for (k, (before, after)) in &self.commits[&c].changeset {
                let Key::Node(m, a) = k else { continue };
                let e = out.entry(*m).or_insert((0, 0, 0));
                if e.0 == 0 {
                    e.0 = c;
                }
                if e.2 == 0 && !matches!(a, Aspect::Edge(_)) {
                    e.2 = c;
                }
                if e.1 == 0
                    && *a == Aspect::Existence
                    && *before == KState::ABSENT
                    && *after != KState::ABSENT
                {
                    e.1 = c;
                }
            }
        }
        out
    }

    /// The local seqs of one node at a commit ([API §15.4]); see [`Dag::local_seqs_all`].
    pub fn local_seqs(&self, seq: Option<u64>, n: Nid) -> (u64, u64, u64) {
        let (mut rev, mut created, mut updated) = (0, 0, 0);
        for c in self.chain(seq) {
            let x = &self.commits[&c];
            for (k, (before, after)) in &x.changeset {
                let Key::Node(m, a) = k else { continue };
                if *m != n {
                    continue;
                }
                if rev == 0 {
                    rev = c;
                }
                if updated == 0 && !matches!(a, Aspect::Edge(_)) {
                    updated = c;
                }
                if created == 0
                    && *a == Aspect::Existence
                    && *before == KState::ABSENT
                    && *after != KState::ABSENT
                {
                    created = c;
                }
            }
            if rev != 0 && created != 0 && updated != 0 {
                break;
            }
        }
        (rev, created, updated)
    }

    /// The history facts of one view for the status guards ([`History`]).
    pub fn history(&self, tip: Option<u64>) -> ViewHistory<'_> {
        ViewHistory { dag: self, tip }
    }

    /// The ref moves no commit carries, with their ref names, in the order they happened (by HLC, then ref id)
    /// ([API §15.7] `moves`).
    pub fn moves(&self) -> Vec<(String, RefMove)> {
        let mut v: Vec<(u64, u32, String, RefMove)> = self
            .refs
            .values()
            .flat_map(|r| {
                r.moves
                    .iter()
                    .filter(|m| m.reason != MoveReason::Commit)
                    .map(move |m| (m.hlc, r.id, r.name.clone(), m.clone()))
            })
            .collect();
        v.sort_by_key(|(h, id, _, _)| (*h, *id));
        v.into_iter().map(|(_, _, n, m)| (n, m)).collect()
    }
}

/// The history of one view: its first-parent chain.
pub struct ViewHistory<'a> {
    dag: &'a Dag,
    tip: Option<u64>,
}

impl History for ViewHistory<'_> {
    /// The actor of the newest commit of the chain whose net changeset adds the edge key (absent before, present
    /// after): the commit that added the edge the view holds.
    fn edge_actor(&self, src: Nid, key: &EdgeKey) -> Option<String> {
        let k = Key::Node(src, Aspect::Edge(key.clone()));
        self.dag.chain(self.tip).into_iter().find_map(|c| {
            let x = &self.dag.commits[&c];
            x.changeset
                .get(&k)
                .filter(|(before, after)| *before == KState::ABSENT && *after != KState::ABSENT)
                .map(|_| x.actor.clone())
        })
    }

    fn changed_ms(&self, n: Nid, aspect: &Aspect) -> Option<u64> {
        let k = Key::Node(n, aspect.clone());
        self.dag.chain(self.tip).into_iter().find_map(|c| {
            let x = &self.dag.commits[&c];
            x.changeset.get(&k).map(|_| x.hlc >> 16)
        })
    }
}

fn bad_name(written: &str, rule: &str, why: &str) -> Refusal {
    Refusal::new(
        "bad_ref_name",
        2,
        format!("{written} is not a valid ref name: {why}"),
    )
    .key("name", written)
    .key("rule", rule)
}

/// IN-3, the namespace completion of a creating verb ([F12 §2.5]): `lane/<u>` or `plan/<u>` as written, with a given
/// kind that must agree, else RN-1 fails; any other name becomes `lane/<name>`, or `plan/<name>` under kind `plan`; a
/// tag name `tags/<name>`.
// spec: [F12 §2.5] IN-3
pub fn complete_name(written: &str, tag: bool, kind: Option<RefKind>) -> Res<(String, RefKind)> {
    if tag {
        let n = if written.starts_with("tags/") {
            written.to_string()
        } else {
            format!("tags/{written}")
        };
        return Ok((n, RefKind::Tag));
    }
    if written.starts_with("lane/") {
        if kind == Some(RefKind::Plan) {
            return Err(bad_name(
                written,
                "RN-1",
                "--kind plan does not match lane/",
            ));
        }
        return Ok((written.to_string(), RefKind::Work));
    }
    if written.starts_with("plan/") {
        if kind == Some(RefKind::Work) {
            return Err(bad_name(
                written,
                "RN-1",
                "--kind work does not match plan/",
            ));
        }
        return Ok((written.to_string(), RefKind::Plan));
    }
    Ok(if kind == Some(RefKind::Plan) {
        (format!("plan/{written}"), RefKind::Plan)
    } else {
        (format!("lane/{written}"), RefKind::Work)
    })
}

/// RN-1 to RN-6 on a complete name with a namespace ([F12 §2.4]), in rule order: the first rule any part of the name
/// fails is the refusal (each of RN-2 to RN-5 is checked on every segment of the user part before the next rule);
/// `written` is what the refusal quotes.
// spec: [F12 §2.4]
pub fn check_rules(name: &str, written: &str) -> Res<()> {
    let bad = |rule: &str, why: &str| bad_name(written, rule, why);
    // RN-1: the grammar of §2.1, every byte ASCII.
    let word_ok = |w: &str| {
        let b = w.as_bytes();
        !b.is_empty()
            && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit() || b[0] == b'_')
            && b[1..]
                .iter()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'_' || *c == b'-')
    };
    let grammar = name.split('/').all(|seg| seg.split('.').all(word_ok));
    let Some((_, user)) = name.split_once('/').filter(|_| grammar) else {
        return Err(bad(
            "RN-1",
            "use lower-case a-z, 0-9, _ and - in segments joined by . and /, after a namespace",
        ));
    };
    let segs: Vec<&str> = user.split('/').collect();
    if let Some(seg) = segs.iter().find(|s| {
        [
            "main", "lane", "plan", "merge", "import", "orphans", "tags", "from",
        ]
        .contains(s)
    }) {
        return Err(bad("RN-2", &format!("the segment {seg} is reserved")));
    }
    let literal = |seg: &str| {
        let b = seg.as_bytes();
        let commitish = b.len() >= 8
            && b.len() <= 65
            && b[0] == b'c'
            && b[1..]
                .iter()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(c));
        let seqish = b.len() >= 2 && b[0] == b's' && b[1..].iter().all(u8::is_ascii_digit);
        commitish || seqish
    };
    if let Some(seg) = segs.iter().find(|s| literal(s)) {
        return Err(bad(
            "RN-3",
            &format!("the segment {seg} reads as a commit or sequence number"),
        ));
    }
    let device = |seg: &str| {
        let stem = seg.split('.').next().unwrap_or(seg);
        ["con", "prn", "aux", "nul"].contains(&stem)
            || ((stem.starts_with("com") || stem.starts_with("lpt"))
                && stem.len() == 4
                && stem.as_bytes()[3].is_ascii_digit())
    };
    if let Some(seg) = segs.iter().find(|s| device(s)) {
        return Err(bad(
            "RN-4",
            &format!("the segment {seg} is a Windows device name"),
        ));
    }
    if let Some(seg) = segs.iter().find(|s| s.ends_with(".lock")) {
        return Err(bad("RN-5", &format!("the segment {seg} ends in .lock")));
    }
    if name.len() > 128 {
        return Err(bad("RN-6", "it is longer than 128 bytes"));
    }
    Ok(())
}

/// The ref-name rules of a created branch or tag, RN-1 to RN-6, after the namespace completion IN-3 ([F12 §2.4],
/// §2.5): `Ok(name)` with its kind, or the refusal of the first failing rule.
// spec: [F12 §2.4]
// spec: [F12 §2.5]
pub fn check_new_name(written: &str, tag: bool, kind: Option<RefKind>) -> Res<(String, RefKind)> {
    let (name, k) = complete_name(written, tag, kind)?;
    check_rules(&name, written)?;
    Ok((name, k))
}

/// IN-4: a `ref`-typed configuration value (`default-branch`) is a `branch-name` — `main`, or a `lane/` or `plan/`
/// name — that satisfies RN-1 to RN-6, never completed; the ref need not exist ([F12 §2.5]).
// spec: [F12 §2.5] IN-4
pub fn check_branch_name(name: &str) -> Res<()> {
    if name == "main" {
        return Ok(());
    }
    if !(name.starts_with("lane/") || name.starts_with("plan/")) {
        return Err(bad_name(name, "RN-1", "branches start with lane/ or plan/"));
    }
    check_rules(name, name)
}

/// RN-7 and RN-8 over names ([F12 §2.4]): the first rule that fails, with the live name it fails against. `live`
/// holds every live ref name with whether its kind takes part in RN-8 (work, plan and tag refs); RN-7 compares with
/// every live ref, and a prefix ends at a segment boundary.
// spec: [F12 §2.4] RN-7, RN-8
pub fn unique_rule<'a>(
    name: &str,
    live: impl IntoIterator<Item = (&'a str, bool)> + Clone,
) -> Option<(&'static str, &'a str)> {
    if let Some((n, _)) = live.clone().into_iter().find(|(n, _)| *n == name) {
        return Some(("RN-7", n));
    }
    let prefix = |a: &str, b: &str| b.starts_with(a) && b.as_bytes().get(a.len()) == Some(&b'/');
    live.into_iter()
        .find(|(n, counts)| *counts && (prefix(n, name) || prefix(name, n)))
        .map(|(n, _)| ("RN-8", n))
}

/// RN-7 and RN-8 against the live refs ([F12 §2.4]), by [`unique_rule`].
// spec: [F12 §2.4] RN-7, RN-8
pub fn check_unique(dag: &Dag, name: &str) -> Res<()> {
    let live: Vec<(&str, bool)> = dag
        .live_refs()
        .map(|r| {
            (
                r.name.as_str(),
                matches!(r.kind, RefKind::Work | RefKind::Plan | RefKind::Tag),
            )
        })
        .collect();
    match unique_rule(name, live.iter().copied()) {
        None => Ok(()),
        Some(("RN-7", _)) => {
            Err(Refusal::new("ref_exists", 2, format!("{name} already exists")).key("name", name))
        }
        Some((_, other)) => Err(Refusal::new(
            "ref_prefix",
            2,
            format!("{name} and {other} cannot both exist: one is a prefix of the other"),
        )
        .key("name", name)
        .key("other", other.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ref_names_follow_rn_1_to_rn_6() {
        assert_eq!(check_new_name("l5np", false, None).unwrap().0, "lane/l5np");
        assert_eq!(
            check_new_name("q3.v2", false, Some(RefKind::Plan))
                .unwrap()
                .0,
            "plan/q3.v2"
        );
        assert_eq!(check_new_name("v1.2", true, None).unwrap().0, "tags/v1.2");
        for (bad, rule) in [
            ("lane/L5np", "RN-1"),
            ("lane/merge", "RN-2"),
            ("lane/x/from/y", "RN-2"),
            ("lane/cafebabe", "RN-3"),
            ("tags/s4400", "RN-3"),
            ("lane/nul.txt", "RN-4"),
            ("tags/v1.lock", "RN-5"),
            ("lane/a..b", "RN-1"),
            ("lane/.x", "RN-1"),
            // Rule order over the whole user part: RN-2 before RN-5 on an earlier segment.
            ("lane/a.lock/merge", "RN-2"),
            ("lane/nul/cafebabe", "RN-3"),
            ("lane/x.lock/prn", "RN-4"),
        ] {
            let tag = bad.starts_with("tags/");
            assert_eq!(
                check_new_name(bad, tag, None).unwrap_err().get_str("rule"),
                Some(rule),
                "{bad}"
            );
        }
        // IN-3: a `--kind` that disagrees with the written namespace fails RN-1.
        for (name, kind) in [("lane/x", RefKind::Plan), ("plan/x", RefKind::Work)] {
            assert_eq!(
                check_new_name(name, false, Some(kind))
                    .unwrap_err()
                    .get_str("rule"),
                Some("RN-1"),
                "{name}"
            );
        }
        assert!(check_branch_name("main").is_ok());
        assert!(check_branch_name("lane/dev").is_ok());
        assert_eq!(
            check_branch_name("dev").unwrap_err().get_str("rule"),
            Some("RN-1")
        );
        assert_eq!(
            check_branch_name("lane/merge").unwrap_err().get_str("rule"),
            Some("RN-2")
        );
    }

    fn commit(seq: u64, parents: Vec<u64>) -> Commit {
        Commit::new(seq, 0, seq, "ordinary", parents, seq, Changeset::new())
    }

    /// The jump pointers answer first-parent ancestry as the chain walk does.
    #[test]
    fn first_parent_ancestry_agrees_with_the_chain() {
        let mut d = Dag::default();
        d.insert(commit(1, vec![]));
        for s in 2..200u64 {
            // A second line forks at 50 and merges back at 120.
            let parents = match s {
                51 => vec![50],
                52..=119 => vec![s - 2],
                120 => vec![119, 118],
                _ => vec![s - 1],
            };
            d.insert(commit(s, parents));
        }
        for c in [1u64, 50, 77, 118, 119, 120, 199] {
            let chain: BTreeSet<u64> = d.chain(Some(c)).into_iter().collect();
            for x in 1..200u64 {
                assert_eq!(d.on_fp_chain(x, c), chain.contains(&x), "{x} on {c}");
            }
        }
    }
}
