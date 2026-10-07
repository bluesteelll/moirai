//! The marker cache of I26′ and the absorbed vectors, maintained incrementally by the rules of
//! [RULES/state-definition] §6 (`marker-fields`, `marker-events`, `absorption`, `vector-rules`; [F13 §4.2] MC-1 to
//! MC-7; [F11 §7]; [F05 §9.5]). The model keeps the cache beside its from-scratch definition ([`crate::coord`], PD-012
//! over all live refs) and compares the two after every command ([60 §4.4] item 5; [PLAN §6.2] R12), so a flaw in a
//! cache rule shows as a disagreement inside the model.
//!
//! Every event returns the `Marker` entries the log carries for it ([F05 §9.5]); one `Marker` record per group carries
//! them, and its HLC is every entry's `hlc` ([API §6.2] CK-4). The origin of a hold is computed incrementally at each
//! landing commit by the `origin-rules` rows ([RULES/state-definition] OR rows, read as data), from the (hold, origin)
//! facts the cache records per commit, never by walking the DAG.

use crate::clock::Hlc;
use crate::coord::{self, closed};
use crate::dag::{Dag, Ref, RefKind};
use crate::rules::rules;
use crate::state::{Key, State};
use crate::value::Nid;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

/// A marker's identity: (`#N`, origin ref id, origin commit) ([RULES/state-definition] MF-001, MF-003, MF-004).
pub type MKey = (Nid, u32, u64);

/// The kind of a marker row ([RULES/state-definition] MF-002).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum MKind {
    /// A `done` or `cancelled` hold.
    Settled,
    /// A `deleted` hold.
    Deleted,
    /// The holder set emptied.
    Cleared,
}

impl MKind {
    /// The result's name ([API §10.8]).
    pub fn name(self) -> &'static str {
        match self {
            MKind::Settled => "settled",
            MKind::Deleted => "deleted",
            MKind::Cleared => "cleared",
        }
    }
}

/// The event that wrote an entry ([F05 §9.5] field 8 `cause`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cause {
    /// 1: a commit landing on a work ref.
    Ops,
    /// 2: `undo`.
    Undo,
    /// 3: `op restore`.
    OpRestore,
    /// 4: a ref deletion.
    BranchDelete,
    /// 5: a fork.
    Fork,
}

impl Cause {
    /// The result's name ([API §10.8]).
    pub fn name(self) -> &'static str {
        match self {
            Cause::Ops => "ops",
            Cause::Undo => "undo",
            Cause::OpRestore => "op-restore",
            Cause::BranchDelete => "branch-delete",
            Cause::Fork => "fork",
        }
    }
}

/// One `MarkerEntry` of a `Marker` record ([F05 §9.5]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// `mkind`: 1 settled, 2 deleted, 3 cleared, 4 holders, 5 nonlinear.
    pub mkind: u8,
    /// The marker's identity.
    pub key: MKey,
    /// `ref_seq` of the origin commit.
    pub ref_seq: u64,
    /// The event.
    pub cause: Cause,
    /// `holder` (MF-009): the lease holder of the completion, on the entry a `complete` writes.
    pub holder: Option<String>,
    /// `status`: `done` or `cancelled` for a settled entry.
    pub status: Option<&'static str>,
    /// `outcome` (MF-009).
    pub outcome: Option<String>,
    /// `holders`: the complete holder set after the entry (`mkind` 1, 2, 4), ref ids ascending.
    pub holders: Vec<u32>,
    /// The record's HLC.
    pub hlc: u64,
}

impl Entry {
    /// The result kind of a listed entry (`settled`, `deleted`, `cleared`); `None` for the cache bookkeeping of
    /// `mkind` 4 and 5, which results do not list ([API §10.8]).
    pub fn listed(&self) -> Option<MKind> {
        match self.mkind {
            1 => Some(MKind::Settled),
            2 => Some(MKind::Deleted),
            3 => Some(MKind::Cleared),
            _ => None,
        }
    }
}

/// One row of `MARKERS` or `MARKERS_OLD` ([F11 §7]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Marker {
    /// The identity.
    pub key: MKey,
    /// `ref_seq` of the origin commit (MF-005).
    pub ref_seq: u64,
    /// The kind (MF-002).
    pub kind: MKind,
    /// `done` or `cancelled` for a settled row.
    pub status: Option<&'static str>,
    /// The event of the newest entry that changed the row.
    pub cause: Cause,
    /// MF-007.
    pub nonlinear: bool,
    /// MF-009: the completion's lease holder.
    pub actor: Option<String>,
    /// MF-009: the completion's outcome.
    pub outcome: Option<String>,
    /// MF-008: the HLC of the newest settled, deleted or cleared entry.
    pub hlc: u64,
    /// MF-006: the live work refs that hold `#N` with this origin.
    pub holders: BTreeSet<u32>,
}

impl Marker {
    /// Active while its holder set is non-empty (MF-006); a cleared row never is.
    pub fn active(&self) -> bool {
        self.kind != MKind::Cleared && !self.holders.is_empty()
    }
}

/// A (hold, origin) fact: the hold value (`done`, `cancelled`, `deleted`, `none`) and, for a closed hold, its origin
/// commit (0 otherwise).
pub type Fact = (&'static str, u64);

/// The group a `Marker` record belongs to: the commit group of a commit, or the ref group of a ref move by the HLC of
/// its `RefUpdate` ([F05 §4.7]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Group {
    /// The commit group of this commit.
    Commit(u64),
    /// The ref group of the `RefUpdate` with this HLC.
    Move(u64),
}

/// A completion a commit carries: the task, the holder of the lease `complete` presented and its outcome (MF-009).
pub type Completion = (Nid, String, String);

/// How V(c) is stored: in full, or as its first parent's vector with the commit's own entry (VR-002), so a chain of
/// one-parent commits shares one full vector every [`FULL_EVERY`] commits instead of copying it at each.
#[derive(Clone, Debug)]
enum Stored {
    /// The whole vector: a root, a two-parent commit (VR-003), or every [`FULL_EVERY`]-th link of a chain.
    Full(Rc<BTreeMap<u32, u64>>),
    /// V(parent) with `V[ref_id] = ref_seq`; `depth` counts the links back to the nearest full vector.
    Own {
        /// The first parent.
        parent: u64,
        /// ref(c).
        ref_id: u32,
        /// ref_seq(c).
        ref_seq: u64,
        /// The links back to a full vector, 1 for a commit whose parent is stored in full.
        depth: u32,
    },
}

/// The longest chain of [`Stored::Own`] links before a vector is stored in full: it bounds a lookup to that many steps.
const FULL_EVERY: u32 = 32;

/// Ancestor sets of commits, each computed once per evaluation: the walks of AB-002 and of ME-013's test.
#[derive(Default)]
pub struct Anc(BTreeMap<Option<u64>, BTreeSet<u64>>);

impl Anc {
    /// Whether `c` ∈ anc*(`tip`).
    pub fn contains(&mut self, dag: &Dag, tip: Option<u64>, c: u64) -> bool {
        self.0
            .entry(tip)
            .or_insert_with(|| dag.ancestors(tip))
            .contains(&c)
    }
}

/// The marker cache and the absorbed vectors of one store.
#[derive(Clone, Debug, Default)]
pub struct Markers {
    /// `MARKERS`: one row per identity.
    pub hot: BTreeMap<MKey, Marker>,
    /// `MARKERS_OLD`: rows ME-012 moved there, each with the holder set ME-002 to ME-007 keep maintaining for it. The
    /// model runs no checkpoint fold in a stream; [`Markers::fold_inert`] is the fold, which the suites run between the
    /// commands of the I26′ scenarios and of GT18's histories and which changes no record ([RULES/state-definition]
    /// ME-012, open point 17).
    pub old: BTreeMap<MKey, Marker>,
    /// Per commit, the nodes whose (hold, origin) differs from the first parent's.
    facts: BTreeMap<u64, BTreeMap<Nid, Fact>>,
    /// Per node, the commits (ascending) that hold a fact of it.
    fact_ix: BTreeMap<Nid, Vec<u64>>,
    /// V(c) per commit (VR-002, VR-003).
    vectors: BTreeMap<u64, Stored>,
    /// Per ref id, `absorbed_R` = V(tip(R)) as the rules maintain it; kept after the ref is deleted (VR-006).
    pub absorbed: BTreeMap<u32, BTreeMap<u32, u64>>,
    /// The entries of each group's `Marker` record.
    groups: BTreeMap<Group, Vec<Entry>>,
}

/// The origin rule of a commit with `parents` parents whose hold equals its first (second) parent's
/// ([RULES/state-definition] `origin-rules`, first match in order): `c`, `org-p1` or `org-p2`.
fn origin_rule(parents: usize, p1_same: bool, p2_same: bool) -> &'static str {
    let t = rules().table("origin-rules");
    let r = t
        .rows
        .iter()
        .find(|r| {
            r.int("parents") as usize == parents.min(2)
                && match r.tok("condition") {
                    "always" => true,
                    "p1-same" => p1_same,
                    "p1-differs" => !p1_same,
                    "p2-same" => p2_same,
                    "both-differ" => !p1_same && !p2_same,
                    other => panic!("origin condition {other} has no implementation"),
                }
        })
        .unwrap_or_else(|| panic!("origin-rules has no row for {parents} parents"));
    match r.tok("origin") {
        "c" => "c",
        "org-p1" => "org-p1",
        "org-p2" => "org-p2",
        other => panic!("origin {other} has no implementation"),
    }
}

fn holds_count(kind: RefKind) -> bool {
    coord::view_kind(kind).0
}

impl Markers {
    /// The (hold, origin) of `#N` at a commit: the newest fact on the commit's first-parent chain, else `none`.
    pub fn fact_at(&self, dag: &Dag, c: Option<u64>, n: Nid) -> Fact {
        let Some(c) = c else { return ("none", 0) };
        if let Some(v) = self.fact_ix.get(&n) {
            for d in v.iter().rev().filter(|d| **d <= c) {
                if dag.on_fp_chain(*d, c) {
                    return self.facts[d][&n];
                }
            }
        }
        ("none", 0)
    }

    /// V(c), empty for no commit: the nearest full vector with the own entries of the links after it, the newest link's
    /// entry winning for its ref (VR-002 applied link by link).
    pub fn vector(&self, c: Option<u64>) -> BTreeMap<u32, u64> {
        let mut own: Vec<(u32, u64)> = Vec::new();
        let mut cur = c;
        while let Some(x) = cur {
            match self.vectors.get(&x) {
                None => break,
                Some(Stored::Full(v)) => {
                    let mut out = (**v).clone();
                    for (r, s) in own.into_iter().rev() {
                        out.insert(r, s);
                    }
                    return out;
                }
                Some(Stored::Own {
                    parent,
                    ref_id,
                    ref_seq,
                    ..
                }) => {
                    own.push((*ref_id, *ref_seq));
                    cur = Some(*parent);
                }
            }
        }
        assert!(
            own.is_empty(),
            "a vector chain ends in a stored full vector"
        );
        BTreeMap::new()
    }

    /// The entries a group's `Marker` record carried.
    pub fn group(&self, g: Group) -> &[Entry] {
        self.groups.get(&g).map_or(&[], Vec::as_slice)
    }

    /// Every `Marker` record written, by group.
    pub fn records(&self) -> &BTreeMap<Group, Vec<Entry>> {
        &self.groups
    }

    /// Applies one entry to `MARKERS` as replay does ([F11 §7] "Records"): a settled or deleted entry writes or
    /// re-emits the row with its holder set; `holders` replaces the set; `cleared` sets kind 3 and empties it;
    /// `nonlinear` sets the flag. An identity found only in `MARKERS_OLD` returns to `MARKERS` with the entry (ME-013).
    fn apply(&mut self, e: &Entry) {
        if !self.hot.contains_key(&e.key)
            && let Some(m) = self.old.remove(&e.key)
        {
            self.hot.insert(e.key, m);
        }
        let row = self.hot.entry(e.key).or_insert_with(|| Marker {
            key: e.key,
            ref_seq: e.ref_seq,
            kind: MKind::Settled,
            status: None,
            cause: e.cause,
            nonlinear: false,
            actor: None,
            outcome: None,
            hlc: e.hlc,
            holders: BTreeSet::new(),
        });
        row.cause = e.cause;
        match e.mkind {
            1 | 2 => {
                row.kind = if e.mkind == 1 {
                    MKind::Settled
                } else {
                    MKind::Deleted
                };
                row.status = e.status;
                row.actor = e.holder.clone();
                row.outcome = e.outcome.clone();
                row.hlc = e.hlc;
                row.holders = e.holders.iter().copied().collect();
            }
            3 => {
                row.kind = MKind::Cleared;
                row.hlc = e.hlc;
                row.holders.clear();
            }
            4 => row.holders = e.holders.iter().copied().collect(),
            5 => row.nonlinear = true,
            other => panic!("mkind {other} is invalid"),
        }
    }

    /// Emits and applies the entries of one record.
    fn emit(&mut self, g: Group, entries: Vec<Entry>) -> Vec<Entry> {
        for e in &entries {
            self.apply(e);
        }
        if !entries.is_empty() {
            self.groups.insert(g, entries.clone());
        }
        entries
    }

    /// The settled-or-deleted entry that writes or re-emits a marker with the given holders (ME-001, ME-003).
    fn write_entry(
        key: MKey,
        ref_seq: u64,
        hold: &'static str,
        holders: Vec<u32>,
        cause: Cause,
    ) -> Entry {
        let settled = hold != "deleted";
        Entry {
            mkind: if settled { 1 } else { 2 },
            key,
            ref_seq,
            cause,
            holder: None,
            status: settled.then_some(hold),
            outcome: None,
            holders,
            hlc: 0,
        }
    }

    /// The bookkeeping entry of `mkind` 4 (`holders`) or 5 (`flag-nonlinear`).
    fn keep_entry(mkind: u8, key: MKey, ref_seq: u64, holders: Vec<u32>, cause: Cause) -> Entry {
        Entry {
            mkind,
            key,
            ref_seq,
            cause,
            holder: None,
            status: None,
            outcome: None,
            holders,
            hlc: 0,
        }
    }

    /// The row of an identity, in `MARKERS` or in `MARKERS_OLD`: ME-012's move changes only its storage, so the rules
    /// read and write it wherever it lies ([RULES/state-definition] ME-012).
    fn row(&self, key: &MKey) -> Option<&Marker> {
        self.hot.get(key).or_else(|| self.old.get(key))
    }

    /// A holder `x` joins the marker of (`#N`, origin): `holders` for an active marker (ME-002), else the marker is
    /// written or re-emitted with holders {x} (ME-003), in either section; a `MARKERS_OLD` row the entry names returns
    /// with it (ME-013).
    // rule: ME-002, ME-003
    fn join(&self, dag: &Dag, n: Nid, (hold, o): Fact, x: u32, cause: Cause, out: &mut Vec<Entry>) {
        let c = &dag.commits[&o];
        let key = (n, c.ref_id, o);
        match self.row(&key) {
            Some(m) if m.active() => {
                let mut h = m.holders.clone();
                h.insert(x);
                if h != m.holders {
                    out.push(Self::keep_entry(
                        4,
                        key,
                        c.ref_seq,
                        h.into_iter().collect(),
                        cause,
                    ));
                }
            }
            _ => out.push(Self::write_entry(key, c.ref_seq, hold, vec![x], cause)),
        }
    }

    /// ME-013 after a ref move or a fork: a storage move that writes no record. Every `MARKERS_OLD` row that is active
    /// and that some live ref has not absorbed returns to `MARKERS`, where readers probe, with the holder set and flag
    /// ME-012 kept maintained: nothing is recomputed, re-emitted or flagged.
    // rule: ME-013
    fn return_unabsorbed(&mut self, dag: &Dag) {
        let mut anc = Anc::default();
        let back: Vec<MKey> = self
            .old
            .values()
            .filter(|m| {
                m.active()
                    && dag
                        .live_refs()
                        .any(|r| !self.absorbed_in(dag, r, m, &mut anc))
            })
            .map(|m| m.key)
            .collect();
        for k in back {
            let m = self.old.remove(&k).expect("an old row");
            self.hot.insert(k, m);
        }
    }

    /// A holder `x` leaves the marker of (`#N`, origin): `holders`, or `cleared` when none remains (ME-004, ME-005).
    // rule: ME-004
    fn leave(&self, dag: &Dag, n: Nid, o: u64, x: u32, cause: Cause, out: &mut Vec<Entry>) {
        let c = &dag.commits[&o];
        let key = (n, c.ref_id, o);
        let Some(m) = self.row(&key).filter(|m| m.active()) else {
            return;
        };
        let mut h = m.holders.clone();
        if !h.remove(&x) {
            return;
        }
        out.push(Entry {
            mkind: if h.is_empty() { 3 } else { 4 },
            key,
            ref_seq: c.ref_seq,
            cause,
            holder: None,
            status: None,
            outcome: None,
            holders: h.into_iter().collect(),
            hlc: 0,
        });
    }

    /// Ends an event: when it produced entries, one `Marker` record draws the next value of the HLC sequence
    /// ([API §6.2] CK-4), every entry carries it, and the entries are applied.
    fn finish(&mut self, g: Group, mut out: Vec<Entry>, hlc: &mut Hlc, wall_ms: i64) -> Vec<Entry> {
        if out.is_empty() {
            return out;
        }
        let h = hlc.record(wall_ms);
        for e in &mut out {
            e.hlc = h;
        }
        self.emit(g, out)
    }

    /// The facts and the vector of a new commit: for every node whose existence or status the commit changes, its
    /// hold at the commit and the origin by the OR rows over its parents' facts; V(c) from the parents' vectors
    /// (VR-002, VR-003). Called for every commit, whatever ref it lands on.
    // rule: VR-002, VR-003
    pub fn record_commit(&mut self, dag: &Dag, st: &State, c: u64) {
        let commit = &dag.commits[&c];
        let parents = commit.parents.clone();
        let mut nodes = BTreeSet::new();
        for k in commit.changeset.keys() {
            if let Key::Node(n, crate::state::Aspect::Existence | crate::state::Aspect::Status) = k
            {
                nodes.insert(*n);
            }
        }
        let mut f = BTreeMap::new();
        for n in nodes {
            let v1 = coord::hold(st, n);
            let p1 = self.fact_at(dag, parents.first().copied(), n);
            let p2 = parents.get(1).map(|p| self.fact_at(dag, Some(*p), n));
            let o1 = if !closed(v1) {
                0
            } else {
                match origin_rule(parents.len(), p1.0 == v1, p2.is_some_and(|p| p.0 == v1)) {
                    "c" => c,
                    "org-p1" => p1.1,
                    _ => p2.expect("a second parent").1,
                }
            };
            if (v1, o1) != p1 {
                f.insert(n, (v1, o1));
                self.fact_ix.entry(n).or_default().push(c);
            }
        }
        self.facts.insert(c, f);
        let stored = match parents.as_slice() {
            // VR-002: the first parent's vector with the own entry, stored as a link unless the chain is long.
            [p] if self.vectors.contains_key(p) => {
                let depth = match &self.vectors[p] {
                    Stored::Full(_) => 1,
                    Stored::Own { depth, .. } => depth + 1,
                };
                if depth <= FULL_EVERY {
                    Stored::Own {
                        parent: *p,
                        ref_id: commit.ref_id,
                        ref_seq: commit.ref_seq,
                        depth,
                    }
                } else {
                    let mut v = self.vector(Some(*p));
                    v.insert(commit.ref_id, commit.ref_seq);
                    Stored::Full(Rc::new(v))
                }
            }
            // VR-003: the pointwise maximum of both parents' vectors, then the own entry; a root has only its own.
            _ => {
                let mut v = self.vector(parents.first().copied());
                if let Some(p2) = parents.get(1) {
                    for (r, s) in self.vector(Some(*p2)) {
                        let e = v.entry(r).or_insert(0);
                        *e = (*e).max(s);
                    }
                }
                v.insert(commit.ref_id, commit.ref_seq);
                Stored::Full(Rc::new(v))
            }
        };
        self.vectors.insert(c, stored);
    }

    /// A commit landed on ref X, whose tip moved from `old_tip` to it (ME-001 to ME-004, ME-008, ME-010, ME-011;
    /// VR-002, VR-003): the entries of the group's `Marker` record. `completes` names the completions the commit carries
    /// (MF-009). The facts and vector of the commit must be recorded first ([`Markers::record_commit`]).
    ///
    /// ME-001 to ME-004 apply on a work ref only (ME-008: a commit on a `plan/*`, `merge/*`, `import/*` or `orphans/*`
    /// ref changes no holder set); ME-011 applies on every ref, since a marker's origin may lie on a ref of any kind
    /// (a lane forked from `plan/*` holds a `deleted` origin there) and AB-001 reads that ref's vector entry.
    // spec: [F13 §4.2] MC-1
    // rule: ME-001, ME-008, ME-010, ME-011, DS-008
    pub fn commit_lands(
        &mut self,
        dag: &Dag,
        c: u64,
        old_tip: Option<u64>,
        completes: &[Completion],
        hlc: &mut Hlc,
        wall_ms: i64,
    ) -> Vec<Entry> {
        let commit = &dag.commits[&c];
        let x = dag.refs[&commit.ref_id].clone();
        self.absorbed.insert(x.id, self.vector(Some(c)));
        let mut out = Vec::new();
        if holds_count(x.kind) {
            let candidates: BTreeSet<Nid> = if old_tip == commit.parents.first().copied() {
                self.facts[&c].keys().copied().collect()
            } else {
                self.fact_ix.keys().copied().collect()
            };
            for n in candidates {
                let (v0, o0) = self.fact_at(dag, old_tip, n);
                let (v1, o1) = self.fact_at(dag, Some(c), n);
                if (v0, o0) == (v1, o1) {
                    // ME-010: markers follow net state; an unchanged hold emits nothing.
                    continue;
                }
                if closed(v0) {
                    self.leave(dag, n, o0, x.id, Cause::Ops, &mut out);
                }
                if closed(v1) {
                    if o1 == c {
                        // ME-001: this commit originates the hold.
                        let mut e = Self::write_entry(
                            (n, x.id, c),
                            commit.ref_seq,
                            v1,
                            vec![x.id],
                            Cause::Ops,
                        );
                        if v1 == "done"
                            && let Some((_, holder, outcome)) = completes.iter().find(|t| t.0 == n)
                        {
                            e.holder = Some(holder.clone());
                            e.outcome = Some(outcome.clone());
                        }
                        out.push(e);
                    } else {
                        self.join(dag, n, (v1, o1), x.id, Cause::Ops, &mut out);
                    }
                }
            }
        }
        // ME-011: a commit landing on X that does not descend from the origin of an X-landed marker, in `MARKERS` or
        // `MARKERS_OLD` (ME-012).
        let mut anc = Anc::default();
        let diverged: Vec<(MKey, u64)> = self
            .rows()
            .filter(|m| m.key.1 == x.id && !m.nonlinear)
            .filter(|m| !dag.on_fp_chain(m.key.2, c) && !anc.contains(dag, Some(c), m.key.2))
            .map(|m| (m.key, m.ref_seq))
            .collect();
        for (key, ref_seq) in diverged {
            out.push(Self::keep_entry(5, key, ref_seq, Vec::new(), Cause::Ops));
        }
        self.finish(Group::Commit(c), out, hlc, wall_ms)
    }

    /// The closed holds of a commit with their origins: every node whose hold at the commit is in S.
    fn closed_holds(&self, dag: &Dag, st: &State, c: Option<u64>) -> Vec<(Nid, &'static str, u64)> {
        st.nodes
            .keys()
            .filter_map(|n| {
                let (h, o) = self.fact_at(dag, c, *n);
                closed(h).then_some((*n, h, o))
            })
            .collect()
    }

    /// A ref Y was created at its fork commit `at` = tip(Y) (ME-007, ME-013; VR-004): Y joins the holder set of every
    /// closed hold at `at` when Y is a work ref, and, whatever Y's kind, an active `MARKERS_OLD` row Y has not absorbed
    /// returns to `MARKERS` (a fork from a commit other than a tip), writing nothing.
    /// `st` is `state_at(at)`; `group` names the ref group.
    // rule: ME-007, ME-013, VR-004
    pub fn fork(
        &mut self,
        dag: &Dag,
        y: &Ref,
        st: &State,
        group: Group,
        hlc: &mut Hlc,
        wall_ms: i64,
    ) -> Vec<Entry> {
        let at = y.tip;
        self.absorbed.insert(y.id, self.vector(at));
        let mut out = Vec::new();
        if holds_count(y.kind) {
            for (n, h, o) in self.closed_holds(dag, st, at) {
                self.join(dag, n, (h, o), y.id, Cause::Fork, &mut out);
            }
        }
        let out = self.finish(group, out, hlc, wall_ms);
        self.return_unabsorbed(dag);
        out
    }

    /// A ref was deleted (ME-005, ME-009; VR-006): a work ref leaves every holder set, in either section, and a marker
    /// left with no holder is cleared. Its vector entries stay.
    // rule: ME-005, ME-009, VR-006
    pub fn ref_deleted(
        &mut self,
        dag: &Dag,
        r: &Ref,
        group: Group,
        hlc: &mut Hlc,
        wall_ms: i64,
    ) -> Vec<Entry> {
        if !holds_count(r.kind) {
            return Vec::new();
        }
        let mut out = Vec::new();
        let held: Vec<MKey> = self
            .rows()
            .filter(|m| m.active() && m.holders.contains(&r.id))
            .map(|m| m.key)
            .collect();
        for (n, _, o) in held {
            self.leave(dag, n, o, r.id, Cause::BranchDelete, &mut out);
        }
        self.finish(group, out, hlc, wall_ms)
    }

    /// A ref moved from `old` to `new` without a commit landing (`undo`, `op restore`; ME-006, ME-013; VR-005): on a
    /// work ref, for every node whose (hold, origin) differs between the two tips, ME-004 for the old hold and ME-002
    /// or ME-003 for the new one; on a ref of any kind, ME-013's return of the active `MARKERS_OLD` rows some live ref
    /// has not absorbed. WP-91's history verbs call it.
    // rule: ME-006, ME-013, VR-005
    #[allow(clippy::too_many_arguments)]
    pub fn ref_moved(
        &mut self,
        dag: &Dag,
        r: &Ref,
        old: Option<u64>,
        new: Option<u64>,
        cause: Cause,
        group: Group,
        hlc: &mut Hlc,
        wall_ms: i64,
    ) -> Vec<Entry> {
        self.absorbed.insert(r.id, self.vector(new));
        let mut out = Vec::new();
        if holds_count(r.kind) {
            let nodes: Vec<Nid> = self.fact_ix.keys().copied().collect();
            for n in nodes {
                let (v0, o0) = self.fact_at(dag, old, n);
                let (v1, o1) = self.fact_at(dag, new, n);
                if (v0, o0) == (v1, o1) {
                    continue;
                }
                if closed(v0) {
                    self.leave(dag, n, o0, r.id, cause, &mut out);
                }
                if closed(v1) {
                    self.join(dag, n, (v1, o1), r.id, cause, &mut out);
                }
            }
        }
        let out = self.finish(group, out, hlc, wall_ms);
        self.return_unabsorbed(dag);
        out
    }

    /// ME-012: the checkpoint fold's move of inert rows to `MARKERS_OLD` — cleared rows and rows every live ref (of
    /// every kind) has absorbed — each with its holder set, which ME-002 to ME-007 go on maintaining there. It writes no
    /// record, so the records and the runtime snapshot are the same whenever a fold ran (open point 17, decided (a));
    /// the suites run it between commands and require the same records and answers.
    // rule: ME-012
    pub fn fold_inert(&mut self, dag: &Dag) {
        let mut anc = Anc::default();
        let inert: Vec<MKey> = self
            .hot
            .values()
            .filter(|m| {
                m.kind == MKind::Cleared
                    || dag
                        .live_refs()
                        .all(|r| self.absorbed_in(dag, r, m, &mut anc))
            })
            .map(|m| m.key)
            .collect();
        for k in inert {
            let m = self.hot.remove(&k).expect("a hot row");
            self.old.insert(k, m);
        }
    }

    /// Whether view R has absorbed a marker: AB-001 for a linear marker (`absorbed_R[origin ref] ≥ ref_seq`, one
    /// lookup), AB-002 for a nonlinear one (the origin commit is in anc*(tip(R))).
    // rule: AB-001, AB-002
    pub fn absorbed_by(&self, dag: &Dag, r: &Ref, m: &Marker) -> bool {
        self.absorbed_in(dag, r, m, &mut Anc::default())
    }

    /// [`Markers::absorbed_by`] with the ancestor sets of one evaluation, so AB-002 walks each tip once.
    pub fn absorbed_in(&self, dag: &Dag, r: &Ref, m: &Marker, anc: &mut Anc) -> bool {
        if m.nonlinear {
            anc.contains(dag, r.tip, m.key.2)
        } else {
            self.absorbed
                .get(&r.id)
                .and_then(|v| v.get(&m.key.1))
                .is_some_and(|s| *s >= m.ref_seq)
        }
    }

    /// The active markers of `#N` that view R has not absorbed (AB-003: an inactive marker never excludes).
    // rule: AB-003
    pub fn unabsorbed<'a>(&'a self, dag: &Dag, r: &Ref, n: Nid) -> Vec<&'a Marker> {
        let mut anc = Anc::default();
        self.hot
            .range((n, 0, 0)..=(n, u32::MAX, u64::MAX))
            .map(|(_, m)| m)
            .filter(|m| m.active() && !self.absorbed_in(dag, r, m, &mut anc))
            .collect()
    }

    /// AB-004: `#N` is excluded on R iff some active marker of it is not absorbed by R ([F13 §4.2] MC-4).
    // spec: [F13 §4.2] MC-4
    // rule: AB-004
    pub fn excluded(&self, dag: &Dag, r: &Ref, n: Nid) -> bool {
        !self.unabsorbed(dag, r, n).is_empty()
    }

    /// `settled_elsewhere` and `deleted_elsewhere` by the cache: the unabsorbed active markers split by kind.
    pub fn elsewhere(&self, dag: &Dag, r: &Ref, n: Nid) -> (bool, bool) {
        let v = self.unabsorbed(dag, r, n);
        (
            v.iter().any(|m| m.kind == MKind::Settled),
            v.iter().any(|m| m.kind == MKind::Deleted),
        )
    }

    /// Every marker row, ascending by identity (`#N`, origin ref, origin commit) whichever section holds it: the order
    /// of `MARKERS`, whose sort key `MARKERS_OLD` shares ([F11 §7]). ME-012's move changes only a row's storage layer,
    /// so the rules that scan the rows (ME-005, ME-011) write their entries in the order a store without a fold
    /// writes them, and the fold reaches no record nor, through a record's entry order, the change feed
    /// ([LQ/std §2.15] "Ties"; [API §2.5] DT-2). The model holds an identity in one section at most.
    // spec: [F11 §7]
    // rule: ME-012
    pub fn rows(&self) -> impl Iterator<Item = &Marker> {
        let (mut hot, mut old) = (self.hot.values().peekable(), self.old.values().peekable());
        std::iter::from_fn(move || match (hot.peek(), old.peek()) {
            (Some(h), Some(o)) if o.key < h.key => old.next(),
            (Some(_), _) => hot.next(),
            (None, _) => old.next(),
        })
    }

    /// The facts [`Markers::record_commit`] recorded for a commit: the nodes whose (hold, origin) differs from its first
    /// parent's.
    pub fn facts_of(&self, c: u64) -> Option<&BTreeMap<Nid, Fact>> {
        self.facts.get(&c)
    }
}

/// MC-7 and VR-001 on one store: for every live ref R and every allocated `#N`, the cache's answer (AB-004) equals
/// `excluded(R, #N)` by the definition over all live refs (PD-012), and every live ref's maintained vector equals
/// V(tip(R)) by its definition ([RULES/state-definition] "Why the cache equals the definition"). The first
/// disagreement is the error.
// spec: [F13 §4.2] MC-7
pub fn agrees_with_definition(
    dag: &Dag,
    alloc: &dyn crate::state::Alloc,
    ids: impl IntoIterator<Item = Nid> + Clone,
    m: &Markers,
) -> Result<(), String> {
    let mut o = coord::Oracle::new(dag, alloc);
    for r in dag.live_refs() {
        let want = dag.absorbed(r.tip);
        let got = m.absorbed.get(&r.id).cloned().unwrap_or_default();
        if want != got {
            return Err(format!(
                "VR: the vector of {} is {got:?}, its definition {want:?}",
                r.name
            ));
        }
        for n in ids.clone() {
            let def = o.i26p_excluded(&r.name, n);
            let cache = m.excluded(dag, r, n);
            if def != cache {
                return Err(format!(
                    "MC-7: {n} on {}: the definition says {def}, the cache {cache}; holders elsewhere {:?}",
                    r.name,
                    o.holders_elsewhere(&r.name, n)
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origins_follow_the_table() {
        assert_eq!(origin_rule(0, false, false), "c");
        assert_eq!(origin_rule(1, true, false), "org-p1");
        assert_eq!(origin_rule(1, false, false), "c");
        assert_eq!(origin_rule(2, true, true), "org-p1");
        assert_eq!(origin_rule(2, false, true), "org-p2");
        assert_eq!(origin_rule(2, false, false), "c");
    }

    #[test]
    fn entries_apply_as_replay_does() {
        let mut m = Markers::default();
        let key = (Nid(7), 1, 3);
        let e = |mkind: u8, holders: Vec<u32>, hlc: u64| Entry {
            mkind,
            key,
            ref_seq: 2,
            cause: Cause::Ops,
            holder: None,
            status: Some("done"),
            outcome: None,
            holders,
            hlc,
        };
        m.apply(&e(1, vec![1], 10));
        assert!(m.hot[&key].active());
        m.apply(&e(4, vec![1, 2], 11));
        assert_eq!(
            m.hot[&key].hlc, 10,
            "MF-008: a holders entry keeps the row's hlc"
        );
        m.apply(&e(3, vec![], 12));
        assert!(!m.hot[&key].active() && m.hot[&key].kind == MKind::Cleared);
        m.apply(&e(5, vec![], 13));
        assert!(m.hot[&key].nonlinear);
        // ME-013: an identity found only in MARKERS_OLD returns to MARKERS.
        let row = m.hot.remove(&key).unwrap();
        m.old.insert(key, row);
        m.apply(&e(4, vec![2], 14));
        assert!(m.old.is_empty() && m.hot[&key].holders.contains(&2));
    }

    use crate::coord::Oracle;
    use crate::dag::{Commit, Ref};
    use crate::state::{Aspect, Changeset, Creator, KState, KVal};
    use crate::value::Uid;
    use proptest::prelude::*;

    struct Fixed;
    impl crate::state::Alloc for Fixed {
        fn uid(&self, n: Nid) -> Uid {
            let mut b = [0u8; 16];
            b[..4].copy_from_slice(&n.0.to_le_bytes());
            Uid(b)
        }
        fn creator(&self, _: Nid) -> Creator {
            Creator::default()
        }
    }

    /// A hand-built DAG over refs 0..`refs` (all work refs) whose root commit 1 on ref 0 creates tasks #1 to #`nodes`.
    struct Built {
        dag: Dag,
        m: Markers,
        next_seq: BTreeMap<u32, u64>,
        nodes: u32,
    }

    impl Built {
        fn new(refs: u32, nodes: u32) -> Built {
            let mut dag = Dag::default();
            for id in 0..refs {
                dag.refs.insert(
                    id,
                    Ref {
                        id,
                        name: if id == 0 {
                            "main".into()
                        } else {
                            format!("lane/{id}")
                        },
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
            }
            let mut b = Built {
                dag,
                m: Markers::default(),
                next_seq: BTreeMap::new(),
                nodes,
            };
            let cs: Changeset = (1..=nodes)
                .map(|n| {
                    (
                        Key::Node(Nid(n), Aspect::Existence),
                        (
                            KState::ABSENT,
                            KState::Plain(Some(KVal::Live("task".into()))),
                        ),
                    )
                })
                .collect();
            b.add(0, vec![], cs);
            for id in 0..refs {
                b.dag.refs.get_mut(&id).unwrap().tip = Some(1);
            }
            b
        }

        /// Adds a commit on ref `r` with these parents and changeset (against the first parent); records its facts
        /// and vector; moves the ref's tip.
        fn add(&mut self, r: u32, parents: Vec<u64>, changeset: Changeset) -> u64 {
            let seq = self.dag.commits.len() as u64 + 1;
            let rs = self.next_seq.entry(r).or_insert(1);
            let ref_seq = *rs;
            *rs += 1;
            let mut c = Commit::new(seq, r, ref_seq, "ordinary", parents, seq, changeset);
            c.actor = "a".into();
            c.actor_src = "flag".into();
            self.dag.insert(c);
            let st = self.dag.state_at(Some(seq), &Fixed);
            self.m.record_commit(&self.dag, &st, seq);
            self.dag.refs.get_mut(&r).unwrap().tip = Some(seq);
            seq
        }

        /// The key states of `#N` at a commit.
        fn keys(&self, c: u64, n: Nid) -> (KState, KState) {
            self.dag.hold_keys_at(Some(c), n)
        }

        /// The changeset against `p1` that gives `#N` these existence and status states.
        fn to(&self, p1: u64, want: &[(Nid, (KState, KState))]) -> Changeset {
            let mut cs = Changeset::new();
            for (n, (e, s)) in want {
                let (e0, s0) = self.keys(p1, *n);
                if e0 != *e {
                    cs.insert(Key::Node(*n, Aspect::Existence), (e0, e.clone()));
                }
                if s0 != *s {
                    cs.insert(Key::Node(*n, Aspect::Status), (s0, s.clone()));
                }
            }
            cs
        }

        /// Every commit's recorded (hold, origin) of every node and its vector against the definitions: the OR rows by
        /// [`Oracle::origin`] and VR-001 by [`Dag::absorbed`].
        fn check(&self) -> Result<(), String> {
            let mut o = Oracle::new(&self.dag, &Fixed);
            for c in self.dag.commits.keys().copied() {
                for n in (1..=self.nodes).map(Nid) {
                    let h = o.hold_at(c, n);
                    let want = if closed(h) {
                        (h, o.origin(c, n))
                    } else {
                        ("none", 0)
                    };
                    let got = self.m.fact_at(&self.dag, Some(c), n);
                    if got != want {
                        return Err(format!("s{c} {n}: recorded {got:?}, defined {want:?}"));
                    }
                }
                let (got, want) = (self.m.vector(Some(c)), self.dag.absorbed(Some(c)));
                if got != want {
                    return Err(format!("V(s{c}): recorded {got:?}, defined {want:?}"));
                }
            }
            Ok(())
        }
    }

    fn status(s: &str) -> KState {
        KState::Plain(Some(KVal::Status {
            status: s.into(),
            resolution: String::new(),
        }))
    }

    fn live() -> KState {
        KState::Plain(Some(KVal::Live("task".into())))
    }

    fn tomb() -> KState {
        KState::Plain(Some(KVal::Deleted {
            kind: "task".into(),
            reason: None,
            replaced_by: None,
        }))
    }

    /// OR-004 to OR-006 and VR-003 on two-parent commits: a merge whose destination already held the value keeps its
    /// origin (p1-same), one that takes the source's value takes the source's origin (p2-same), and one that produced
    /// the value itself is the origin (both-differ).
    #[test]
    fn two_parent_commits_follow_the_origin_rules() {
        let mut b = Built::new(2, 1);
        let n = Nid(1);
        // lane/1 completes #1 at c2.
        let cs = b.to(1, &[(n, (live(), status("done")))]);
        let c2 = b.add(1, vec![1], cs);
        // main moves on at c3.
        let c3 = b.add(0, vec![1], Changeset::new());
        // p2-same: main merges lane/1 and takes `done` from the source; the origin is c2.
        let cs = b.to(c3, &[(n, (live(), status("done")))]);
        let c4 = b.add(0, vec![c3, c2], cs);
        assert_eq!(b.m.fact_at(&b.dag, Some(c4), n), ("done", c2), "OR-005");
        // p1-same: lane/1 cancels at c5; main merges it again but keeps its own `done`; the origin stays c2.
        let cs = b.to(c2, &[(n, (live(), status("cancelled")))]);
        let c5 = b.add(1, vec![c2], cs);
        let c6 = b.add(0, vec![c4, c5], Changeset::new());
        assert_eq!(b.m.fact_at(&b.dag, Some(c6), n), ("done", c2), "OR-004");
        // both-differ: lane/1 reopens at c7; main (done) merges it and resolves to `cancelled` itself: the origin is c8.
        let cs = b.to(c5, &[(n, (live(), status("open")))]);
        let c7 = b.add(1, vec![c5], cs);
        let cs = b.to(c6, &[(n, (live(), status("cancelled")))]);
        let c8 = b.add(0, vec![c6, c7], cs);
        assert_eq!(
            b.m.fact_at(&b.dag, Some(c8), n),
            ("cancelled", c8),
            "OR-006"
        );
        // VR-003: the pointwise maximum of both parents, then the own entry.
        assert_eq!(b.m.vector(Some(c8)), BTreeMap::from([(0, 5), (1, 3)]));
        b.check().unwrap();
    }

    /// VR-002 over a chain longer than a stored link run: the links and the full vectors between them give V(c).
    #[test]
    fn long_chains_keep_their_vectors() {
        let mut b = Built::new(3, 1);
        let mut tips = [1u64; 3];
        for i in 0..(3 * FULL_EVERY as usize + 7) {
            let r = (i % 3) as u32;
            let p = tips[r as usize];
            tips[r as usize] = b.add(r, vec![p], Changeset::new());
            if i % 5 == 4 {
                // A merge of the next ref's tip every fifth commit: a full vector in the middle of a chain.
                let q = (r + 1) % 3;
                let (p1, p2) = (tips[r as usize], tips[q as usize]);
                tips[r as usize] = b.add(r, vec![p1, p2], Changeset::new());
            }
        }
        b.check().unwrap();
    }

    #[derive(Clone, Debug)]
    enum Step {
        /// A commit on ref r setting #n's (existence, status) to choice k.
        Commit(u32, u32, usize),
        /// A merge of ref q's tip into ref r; for #n, `k` picks p1's value, p2's value or another one.
        Merge(u32, u32, u32, usize),
    }

    fn value(k: usize) -> (KState, KState) {
        match k % 5 {
            0 => (live(), status("done")),
            1 => (live(), status("cancelled")),
            2 => (live(), status("open")),
            3 => (tomb(), status("done")),
            _ => (tomb(), status("open")),
        }
    }

    fn steps() -> impl Strategy<Value = Vec<Step>> {
        let step = prop_oneof![
            3 => (0..3u32, 1..=2u32, 0..5usize).prop_map(|(r, n, k)| Step::Commit(r, n, k)),
            2 => (0..3u32, 0..3u32, 1..=2u32, 0..7usize).prop_map(|(r, q, n, k)| Step::Merge(r, q, n, k)),
        ];
        proptest::collection::vec(step, 1..40)
    }

    proptest! {
        /// The facts and vectors [`Markers::record_commit`] keeps equal the OR rows and VR-001 by definition on random
        /// DAGs with merges (OR-001 to OR-006, VR-002, VR-003).
        #[test]
        fn recorded_facts_equal_the_definition(steps in steps()) {
            let mut b = Built::new(3, 2);
            for s in steps {
                match s {
                    Step::Commit(r, n, k) => {
                        let p = b.dag.refs[&r].tip.unwrap();
                        let cs = b.to(p, &[(Nid(n), value(k))]);
                        b.add(r, vec![p], cs);
                    }
                    Step::Merge(r, q, n, k) if r != q => {
                        let (p1, p2) = (b.dag.refs[&r].tip.unwrap(), b.dag.refs[&q].tip.unwrap());
                        let want = match k {
                            0 | 1 => b.keys(p1, Nid(n)),
                            2 | 3 => b.keys(p2, Nid(n)),
                            k => value(k),
                        };
                        let cs = b.to(p1, &[(Nid(n), want)]);
                        b.add(r, vec![p1, p2], cs);
                    }
                    Step::Merge(..) => {}
                }
            }
            prop_assert_eq!(b.check(), Ok(()));
        }
    }
}
