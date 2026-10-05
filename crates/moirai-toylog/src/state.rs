//! The toy's replayed state: the tables its records fold into ([F05 §10.3], in the toy's minimal form), the `HEAD` fold
//! of [F05 §10.2], the raw facts every record contributes, and the segment snapshot a checkpoint seals.
//!
//! Two layers are kept apart on purpose. The **tables** are what the toy's own logic reads (tips, leases, idempotency
//! records, markers, intents); seeded bugs in replay change them. The **facts** are the records' fields as the log
//! holds them, appended in log order and never interpreted; [`crate::verify`] re-derives its checks from them alone, as
//! the reference model would from its own rules, so a replay bug cannot hide itself. Facts are kept only when the
//! configuration asks for them ([`crate::Config::facts`]: a harness does, a measurement does not), so nothing in a
//! measured run grows with the log's history.
//!
//! A group is applied all or nothing ([F05 §4.1], §10.1): every record is decoded and checked first, then applied in
//! place. A writer's scratch layer ([F16] P-30) is the view with the pending groups applied through an [`Undo`] log and
//! taken back afterwards, so the cost under the writer byte is that of the pending groups, never of the whole state.

use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet};

use moirai_vfs::Stamp;

use crate::bugs::{Bug, Bugs};
use crate::codec::{Reader, Short, Writer};
use crate::format::{
    COMMIT_IMPORTED, CheckpointRec, CommitRec, Counters, ExtentHeadRec, IdemRec, IntentAbortRec,
    IntentDoneRec, IntentRec, LEASE_CLAIM, LEASE_RELEASE, LeaseRec, MarkerEntry, MarkerRec, PinRec,
    REASON_CREATE, REASON_DELETE, REASON_MOVE, REASON_PARK, RecView, RefTableRec, RefUpdateRec,
    RuntimeRec, SegRef, SymDefs, family, kind,
};
use crate::head::Slot;

/// One group as a scan read it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Group {
    /// Its first byte (a group boundary).
    pub start: u64,
    /// The byte after its end.
    pub end: u64,
    /// The chain value at `start`.
    pub chain_in: u64,
    /// The chain value at `end` (its trailer).
    pub chain_out: u64,
    /// Its records, in order.
    pub recs: Vec<RecView>,
    /// Its bytes as read, when the scan keeps them (a flush holder's re-write, [F16] P-42); else empty.
    pub raw: Vec<u8>,
}

impl Group {
    /// Whether some record is durable ([F05 §4.7]).
    pub fn durable(&self) -> bool {
        self.recs.iter().any(|r| !r.lazy)
    }
}

/// A payload that does not follow its kind's rules: corrupt wherever it lies ([F05 §5.4]).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Malformed {
    /// The record's lsn.
    pub lsn: u64,
    /// Its kind.
    pub kind: u8,
}

impl From<(u64, u8, Short)> for Malformed {
    fn from((lsn, kind, _): (u64, u8, Short)) -> Malformed {
        Malformed { lsn, kind }
    }
}

/// A commit as the tables hold it.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CommitRow {
    /// The content digest.
    pub digest: u64,
    /// Its `seq`.
    pub seq: u64,
    /// Its ref.
    pub ref_id: u32,
    /// Its record's lsn.
    pub lsn: u64,
    /// Its `hlc`.
    pub hlc: u64,
    /// Parked: its ref CAS failed at replay (I27′).
    pub parked: bool,
}

/// A ref.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RefRow {
    /// Its name (a harness key).
    pub name: u64,
    /// Its kind: 1 work, 6 orphans.
    pub rkind: u8,
    /// Its tip (the op of the commit), 0 = empty.
    pub tip: u64,
    /// The op of the record that last moved it (a commit, a `RefUpdate`).
    pub mover: u64,
    /// Deleted.
    pub deleted: bool,
    /// The pinned checkpoint set its view starts from (0 = `main`'s current set).
    pub base_pin: u64,
}

/// A lease.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LeaseRow {
    /// The fencing token.
    pub token: u64,
    /// The task's uid.
    pub uid: u64,
    /// The holder.
    pub holder: u64,
    /// The deadline.
    pub expires: Stamp,
    /// Released.
    pub released: bool,
}

/// An idempotency record.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct IdemRow {
    /// The payload hash.
    pub payload: u64,
    /// The recorded commit's op (0 with `no_commit`).
    pub op: u64,
    /// The stored result.
    pub result: u64,
    /// The lsn of the `Idem` record.
    pub lsn: u64,
}

/// A marker.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MarkerRow {
    /// The settling commit's op.
    pub op: u64,
    /// Its ref.
    pub ref_id: u32,
    /// Its seq.
    pub seq: u64,
}

/// A pin.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PinRow {
    /// The checkpoint set.
    pub set_lsn: u64,
    /// The set's files.
    pub files: Vec<(u8, u32)>,
}

/// The state of an `FsIntent`.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum IntentState {
    /// Open.
    Open,
    /// Done (`FsIntentDone`), with its `recovered` flag.
    Done { recovered: bool },
    /// Aborted, with its reason.
    Aborted { reason: u8 },
}

/// An `FsIntent` and its outcome.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IntentRow {
    /// The record.
    pub rec: IntentRec,
    /// Its state.
    pub state: IntentState,
}

/// A record's fields as the log holds them, in log order ([`crate::verify`] reads only these).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Fact {
    /// A `Commit`.
    Commit {
        /// Its lsn.
        lsn: u64,
        /// The record.
        rec: CommitRec,
    },
    /// A `RefUpdate`.
    RefUpdate {
        /// Its lsn.
        lsn: u64,
        /// The record.
        rec: RefUpdateRec,
    },
    /// A `RefTable`.
    RefTable {
        /// Its lsn.
        lsn: u64,
        /// The record.
        rec: RefTableRec,
    },
    /// A `Lease`.
    Lease {
        /// Its lsn.
        lsn: u64,
        /// The record.
        rec: LeaseRec,
    },
    /// Another record of the HLC sequence (`Idem`, `FsIntent*`; a `Marker` entry is [`Fact::Marker`]): its HLC.
    Semantic {
        /// Its lsn.
        lsn: u64,
        /// Its HLC.
        hlc: u64,
    },
    /// A `Checkpoint` (its `append_hlc` is outside the HLC sequence, [F16] P-36).
    Checkpoint {
        /// Its lsn.
        lsn: u64,
        /// The record.
        rec: CheckpointRec,
    },
    /// An `Idem` record's key and recorded op.
    Idem {
        /// Its lsn.
        lsn: u64,
        /// The key.
        key: u64,
        /// The recorded op.
        op: u64,
    },
    /// A `Pin`.
    Pin {
        /// Its lsn.
        lsn: u64,
        /// The record.
        rec: PinRec,
    },
    /// One entry of a `Marker` record: its HLC is in the HLC sequence ([F16] P-36), and its origin commit names the group
    /// it must share ([F05 §4.7], [F16] P-52).
    Marker {
        /// The record's lsn.
        lsn: u64,
        /// The entry.
        entry: MarkerEntry,
    },
    /// The bounds of a group, recorded before the facts of its records when it has any ([F05 §4.1]): the composition
    /// rules of [F05 §4.7] ([F16] P-52) relate the records that share a group.
    Group {
        /// Its first byte.
        start: u64,
        /// The byte after its end.
        end: u64,
    },
}

/// The replayed state.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct State {
    /// The log-derived counters, as the `HEAD` fold derives them from every record applied.
    pub counters: Counters,
    /// Commits by op.
    pub commits: BTreeMap<u64, CommitRow>,
    /// Refs by id.
    pub refs: BTreeMap<u32, RefRow>,
    /// Ref ids by name.
    pub names: BTreeMap<u64, u32>,
    /// The parked tip of `orphans/<R>` per origin ref R (I27′): a failing commit is parked at replay; the park
    /// `RefUpdate` records it explicitly ([F16] P-70).
    pub orphans: BTreeMap<u32, u64>,
    /// Failing commits that no park `RefUpdate` records yet: (op, origin ref).
    pub unparked: BTreeMap<u64, u32>,
    /// Nodes: `#N` → uid.
    pub nodes: BTreeMap<u32, u64>,
    /// Nodes: uid → `#N`.
    pub uids: BTreeMap<u64, u32>,
    /// Markers by task uid.
    pub markers: BTreeMap<u64, MarkerRow>,
    /// Leases by id.
    pub leases: BTreeMap<u64, LeaseRow>,
    /// The live lease of each task uid.
    pub lease_of: BTreeMap<u64, u64>,
    /// Idempotency records by key.
    pub idem: BTreeMap<u64, IdemRow>,
    /// Fork pins by ref id.
    pub pins: BTreeMap<u32, PinRow>,
    /// Intents by intent id (lsn).
    pub intents: BTreeMap<u64, IntentRow>,
    /// Runtime rows (lazy).
    pub runtime: BTreeMap<u64, u64>,
    /// Symbols of class `text`: id − 1 → text.
    pub symbols: Vec<String>,
    /// The segment set of the newest applied `Checkpoint` with a set change.
    pub segments: Vec<SegRef>,
    /// The id (the `Checkpoint` record's lsn) of that set; 0 = the set `init` left (none).
    pub set_lsn: u64,
    /// The sealed files named by applied `Checkpoint` records and not released since.
    pub files: BTreeSet<(u8, u32)>,
    /// The sealed files an applied `Checkpoint` released ([F05 §9.9] `released`; [F16] P-77 condition 1).
    pub released: BTreeSet<(u8, u32)>,
    /// Retired extents: extent → its `hist` file.
    pub retired: BTreeMap<u32, u32>,
    /// The `append_hlc` of the newest applied `Checkpoint` ([F16] P-77 condition 4's grace starts there); 0 = none.
    pub last_checkpoint_hlc: u64,
    /// Whether [`State::facts`] is kept ([`crate::Config::facts`]).
    pub keep_facts: bool,
    /// The raw facts, in log order; empty unless `keep_facts`.
    pub facts: Vec<Fact>,
}

/// The toy's `main`: ref name 1 ([F16] P-88).
pub const MAIN: u64 = 1;

/// A record decoded and checked, ready to apply.
enum Dec {
    Commit(CommitRec),
    RefUpdate(RefUpdateRec),
    RefTable(RefTableRec),
    Lease(LeaseRec),
    Marker(MarkerRec),
    Idem(IdemRec),
    Pin(PinRec),
    Checkpoint(CheckpointRec),
    Intent(IntentRec),
    IntentDone(IntentDoneRec),
    IntentAbort(IntentAbortRec),
    Runtime(RuntimeRec),
    ExtentHead(ExtentHeadRec),
    Noop,
}

/// The raw facts of one decoded record at `lsn`, appended to `out` in the record's order ([`crate::verify`]).
fn facts_of(lsn: u64, d: &Dec, out: &mut Vec<Fact>) {
    match d {
        Dec::Commit(v) => out.push(Fact::Commit {
            lsn,
            rec: v.clone(),
        }),
        Dec::RefUpdate(v) => out.push(Fact::RefUpdate {
            lsn,
            rec: v.clone(),
        }),
        Dec::RefTable(v) => out.push(Fact::RefTable {
            lsn,
            rec: v.clone(),
        }),
        Dec::Lease(v) => out.push(Fact::Lease {
            lsn,
            rec: v.clone(),
        }),
        Dec::Marker(v) => out.extend(v.entries.iter().map(|e| Fact::Marker {
            lsn,
            entry: e.clone(),
        })),
        Dec::Idem(v) => {
            out.push(Fact::Semantic {
                lsn,
                hlc: v.append_hlc,
            });
            out.push(Fact::Idem {
                lsn,
                key: v.key,
                op: v.op,
            });
        }
        Dec::Pin(v) => out.push(Fact::Pin {
            lsn,
            rec: v.clone(),
        }),
        Dec::Checkpoint(v) => out.push(Fact::Checkpoint {
            lsn,
            rec: v.clone(),
        }),
        Dec::Intent(v) => out.push(Fact::Semantic { lsn, hlc: v.hlc }),
        Dec::IntentDone(v) => out.push(Fact::Semantic { lsn, hlc: v.hlc }),
        Dec::IntentAbort(v) => out.push(Fact::Semantic { lsn, hlc: v.hlc }),
        Dec::Runtime(_) | Dec::ExtentHead(_) | Dec::Noop => {}
    }
}

/// Decodes one record: its symbol definitions and its payload by kind ([F05 §5.4]: `Short` is a malformed payload).
fn decode(r: &RecView) -> Result<(SymDefs, Dec), Short> {
    let (defs, p) = r.split()?;
    let d = match r.kind {
        kind::COMMIT => Dec::Commit(CommitRec::decode(p)?),
        kind::REF_UPDATE => Dec::RefUpdate(RefUpdateRec::decode(p)?),
        kind::REF_TABLE => Dec::RefTable(RefTableRec::decode(p)?),
        kind::LEASE => Dec::Lease(LeaseRec::decode(p)?),
        kind::MARKER => Dec::Marker(MarkerRec::decode(p)?),
        kind::IDEM => Dec::Idem(IdemRec::decode(p)?),
        kind::PIN => Dec::Pin(PinRec::decode(p)?),
        kind::CHECKPOINT => Dec::Checkpoint(CheckpointRec::decode(p)?),
        kind::FS_INTENT => Dec::Intent(IntentRec::decode(p)?),
        kind::FS_INTENT_DONE => Dec::IntentDone(IntentDoneRec::decode(p)?),
        kind::FS_INTENT_ABORTED => Dec::IntentAbort(IntentAbortRec::decode(p)?),
        kind::FILE_OBS => Dec::Runtime(RuntimeRec::decode(p)?),
        kind::EXTENT_HEAD => Dec::ExtentHead(ExtentHeadRec::decode(p)?),
        kind::NOOP => {
            if p.iter().any(|&b| b != 0) {
                return Err(Short);
            }
            Dec::Noop
        }
        _ => return Err(Short),
    };
    Ok((defs, d))
}

/// What [`State::apply_logged`] changed, so that [`State::undo`] takes it back: the writer's scratch layer ([F16] P-30)
/// as a log over the view, whose size is that of the pending groups.
#[derive(Debug, Default)]
pub struct Undo(Vec<Step>);

impl Undo {
    /// Whether nothing was logged.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// One logged change: the value before it.
#[derive(Debug)]
enum Step {
    Counters(Counters),
    Symbols(usize),
    Facts(usize),
    Commit(u64, Option<CommitRow>),
    Ref(u32, Option<RefRow>),
    Name(u64, Option<u32>),
    Orphan(u32, Option<u64>),
    Unparked(u64, Option<u32>),
    Node(u32, Option<u64>),
    Uid(u64, Option<u32>),
    Marker(u64, Option<MarkerRow>),
    Lease(u64, Option<LeaseRow>),
    LeaseOf(u64, Option<u64>),
    Idem(u64, Option<IdemRow>),
    Pin(u32, Option<PinRow>),
    Intent(u64, Option<IntentRow>),
    Runtime(u64, Option<u64>),
    Set(Vec<SegRef>, u64),
    File((u8, u32), bool),
    Released((u8, u32), bool),
    Retired(u32, Option<u32>),
    CheckpointHlc(u64),
}

/// Logs `f()` when a log is kept.
fn log(undo: &mut Option<&mut Undo>, f: impl FnOnce() -> Step) {
    if let Some(u) = undo {
        u.0.push(f());
    }
}

fn restore<K: Ord, V>(m: &mut BTreeMap<K, V>, k: K, v: Option<V>) {
    match v {
        Some(v) => {
            m.insert(k, v);
        }
        None => {
            m.remove(&k);
        }
    }
}

fn restore_set<K: Ord>(s: &mut BTreeSet<K>, k: K, was: bool) {
    if was {
        s.insert(k);
    } else {
        s.remove(&k);
    }
}

impl State {
    /// The state of a store `init` just made, before its groups: empty counters, no facts kept.
    pub fn new() -> State {
        State {
            counters: Counters::EMPTY,
            ..State::default()
        }
    }

    /// The same state, keeping facts from now on when `keep`.
    pub fn keeping_facts(mut self, keep: bool) -> State {
        self.keep_facts = keep;
        if !keep {
            self.facts = Vec::new();
        }
        self
    }

    /// The ref id of the ref called `name`, if it exists and is not deleted.
    pub fn ref_id(&self, name: u64) -> Option<u32> {
        let id = *self.names.get(&name)?;
        (!self.refs.get(&id)?.deleted).then_some(id)
    }

    /// The live lease of task `uid`.
    pub fn live_lease(&self, uid: u64) -> Option<(u64, &LeaseRow)> {
        let id = *self.lease_of.get(&uid)?;
        let row = self.leases.get(&id)?;
        (!row.released).then_some((id, row))
    }

    /// Applies one group all or nothing ([F05 §4.1], §10.1): every record by kind ([F16] P-65). `adopted`: the group lies
    /// beyond the slot's `committed_lsn` of the process that applies it (a pending group it adopts or replays into a
    /// scratch layer); P-65's seeded bug skips its non-commit records.
    pub fn apply(&mut self, g: &Group, bugs: Bugs, adopted: bool) -> Result<(), Malformed> {
        self.apply_inner(g, bugs, adopted, None)
    }

    /// [`State::apply`], logging every change into `undo` ([F16] P-30's scratch layer).
    pub fn apply_logged(
        &mut self,
        g: &Group,
        bugs: Bugs,
        adopted: bool,
        undo: &mut Undo,
    ) -> Result<(), Malformed> {
        self.apply_inner(g, bugs, adopted, Some(undo))
    }

    /// Takes back every change `undo` logged, newest first.
    pub fn undo(&mut self, undo: Undo) {
        for s in undo.0.into_iter().rev() {
            match s {
                Step::Counters(c) => self.counters = c,
                Step::Symbols(n) => self.symbols.truncate(n),
                Step::Facts(n) => self.facts.truncate(n),
                Step::Commit(k, v) => restore(&mut self.commits, k, v),
                Step::Ref(k, v) => restore(&mut self.refs, k, v),
                Step::Name(k, v) => restore(&mut self.names, k, v),
                Step::Orphan(k, v) => restore(&mut self.orphans, k, v),
                Step::Unparked(k, v) => restore(&mut self.unparked, k, v),
                Step::Node(k, v) => restore(&mut self.nodes, k, v),
                Step::Uid(k, v) => restore(&mut self.uids, k, v),
                Step::Marker(k, v) => restore(&mut self.markers, k, v),
                Step::Lease(k, v) => restore(&mut self.leases, k, v),
                Step::LeaseOf(k, v) => restore(&mut self.lease_of, k, v),
                Step::Idem(k, v) => restore(&mut self.idem, k, v),
                Step::Pin(k, v) => restore(&mut self.pins, k, v),
                Step::Intent(k, v) => restore(&mut self.intents, k, v),
                Step::Runtime(k, v) => restore(&mut self.runtime, k, v),
                Step::Set(segments, set_lsn) => {
                    self.segments = segments;
                    self.set_lsn = set_lsn;
                }
                Step::File(k, was) => restore_set(&mut self.files, k, was),
                Step::Released(k, was) => restore_set(&mut self.released, k, was),
                Step::Retired(k, v) => restore(&mut self.retired, k, v),
                Step::CheckpointHlc(h) => self.last_checkpoint_hlc = h,
            }
        }
    }

    fn apply_inner(
        &mut self,
        g: &Group,
        bugs: Bugs,
        adopted: bool,
        mut undo: Option<&mut Undo>,
    ) -> Result<(), Malformed> {
        // Pass 1: decode every record and check its symbol definitions (SD-1: exactly the next id; SD-2: a new
        // string), so that a malformed payload leaves the state untouched.
        let mut decoded: Vec<(u64, SymDefs, Dec)> = Vec::with_capacity(g.recs.len());
        let mut next_sym = self.symbols.len();
        for r in &g.recs {
            if adopted && bugs.on(Bug::P65T11SkipNonCommitRecord) && r.kind != kind::COMMIT {
                continue;
            }
            let bad = |e: Short| Malformed::from((r.lsn, r.kind, e));
            let (defs, d) = decode(r).map_err(bad)?;
            for (id, text) in &defs {
                next_sym += 1;
                let seen = self.symbols.iter().any(|s| s == text)
                    || decoded
                        .iter()
                        .flat_map(|x| x.1.iter())
                        .chain(defs.iter().take_while(|(i, _)| i != id))
                        .any(|(_, t)| t == text);
                if *id as usize != next_sym || seen {
                    return Err(bad(Short));
                }
            }
            decoded.push((r.lsn, defs, d));
        }
        // Pass 2: apply in place.
        log(&mut undo, || Step::Counters(self.counters));
        log(&mut undo, || Step::Symbols(self.symbols.len()));
        log(&mut undo, || Step::Facts(self.facts.len()));
        let mark = self.facts.len();
        for (lsn, defs, d) in decoded {
            self.symbols.extend(defs.into_iter().map(|(_, t)| t));
            // The raw facts are recorded as the record was decoded, before it is dispatched by kind, so that no
            // defect of a table's replay can drop a fact (WP-40 S4 review).
            if self.keep_facts {
                facts_of(lsn, &d, &mut self.facts);
            }
            self.put(lsn, d, &mut undo);
        }
        // The group's bounds go before its records' facts, and only for a group that has some (a lazy or an
        // extent-head group adds none), so the facts grow with the semantic records alone.
        if self.facts.len() > mark {
            self.facts.insert(
                mark,
                Fact::Group {
                    start: g.start,
                    end: g.end,
                },
            );
        }
        Ok(())
    }

    /// Changes the ref row `id` (creating it with defaults) through `f`.
    fn with_ref(&mut self, id: u32, undo: &mut Option<&mut Undo>, f: impl FnOnce(&mut RefRow)) {
        log(undo, || Step::Ref(id, self.refs.get(&id).cloned()));
        f(self.refs.entry(id).or_default());
    }

    /// Changes the ref row `id` through `f` when it exists.
    fn with_existing_ref(
        &mut self,
        id: u32,
        undo: &mut Option<&mut Undo>,
        f: impl FnOnce(&mut RefRow),
    ) {
        if let Some(row) = self.refs.get(&id) {
            log(undo, || Step::Ref(id, Some(row.clone())));
        }
        if let Some(row) = self.refs.get_mut(&id) {
            f(row);
        }
    }

    fn put(&mut self, lsn: u64, d: Dec, undo: &mut Option<&mut Undo>) {
        match d {
            Dec::Commit(v) => {
                let c = &mut self.counters;
                c.commit_seq = c.commit_seq.max(v.seq);
                c.hlc_seq = c.hlc_seq.max(v.append_hlc);
                c.hlc_commit = c.hlc_commit.max(v.hlc);
                for &(n, uid) in &v.creates {
                    self.counters.next_id = self.counters.next_id.max(n.saturating_add(1));
                    if let Entry::Vacant(slot) = self.nodes.entry(n) {
                        log(undo, || Step::Node(n, None));
                        slot.insert(uid);
                    }
                    if let Entry::Vacant(slot) = self.uids.entry(uid) {
                        log(undo, || Step::Uid(uid, None));
                        slot.insert(n);
                    }
                }
                let tip = self.refs.get(&v.ref_id).map_or(0, |x| x.tip);
                let detached = v.flags & crate::format::COMMIT_DETACHED != 0;
                let cas_ok = detached || (self.refs.contains_key(&v.ref_id) && tip == v.ref_old);
                log(undo, || {
                    Step::Commit(v.op, self.commits.get(&v.op).cloned())
                });
                self.commits.insert(
                    v.op,
                    CommitRow {
                        digest: v.digest,
                        seq: v.seq,
                        ref_id: v.ref_id,
                        lsn,
                        hlc: v.hlc,
                        parked: !cas_ok,
                    },
                );
                if detached {
                } else if cas_ok {
                    self.with_existing_ref(v.ref_id, undo, |row| {
                        row.tip = v.op;
                        row.mover = v.op;
                    });
                } else {
                    // I27′: parked on orphans/<R> at every replay; the park RefUpdate records it (P-70).
                    log(undo, || {
                        Step::Orphan(v.ref_id, self.orphans.get(&v.ref_id).copied())
                    });
                    self.orphans.insert(v.ref_id, v.op);
                    log(undo, || {
                        Step::Unparked(v.op, self.unparked.get(&v.op).copied())
                    });
                    self.unparked.insert(v.op, v.ref_id);
                }
            }
            Dec::RefUpdate(v) => {
                let c = &mut self.counters;
                c.hlc_seq = c.hlc_seq.max(v.hlc);
                if matches!(v.reason, REASON_CREATE | REASON_PARK) {
                    c.next_ref_id = c.next_ref_id.max(v.ref_id.saturating_add(1));
                }
                match v.reason {
                    REASON_CREATE => self.with_ref(v.ref_id, undo, |row| {
                        row.tip = v.new;
                        row.mover = v.op;
                        row.deleted = false;
                    }),
                    REASON_PARK => {
                        log(undo, || {
                            Step::Unparked(v.new, self.unparked.get(&v.new).copied())
                        });
                        self.unparked.remove(&v.new);
                        if let Some(orig) = self.commits.get(&v.new).map(|x| x.ref_id) {
                            log(undo, || {
                                Step::Orphan(orig, self.orphans.get(&orig).copied())
                            });
                            self.orphans.insert(orig, v.new);
                        }
                        self.with_existing_ref(v.ref_id, undo, |row| {
                            row.tip = v.new;
                            row.mover = v.op;
                        });
                    }
                    REASON_MOVE => self.with_existing_ref(v.ref_id, undo, |row| {
                        row.tip = v.new;
                        row.mover = v.op;
                    }),
                    REASON_DELETE => self.with_existing_ref(v.ref_id, undo, |row| {
                        row.deleted = true;
                        row.mover = v.op;
                    }),
                    _ => {}
                }
            }
            Dec::RefTable(v) => {
                for e in &v.entries {
                    let c = &mut self.counters;
                    c.next_ref_id = c.next_ref_id.max(e.ref_id.saturating_add(1));
                    self.with_ref(e.ref_id, undo, |row| {
                        row.name = e.name;
                        row.rkind = e.rkind;
                        row.deleted = e.eflags & 1 != 0;
                        row.base_pin = e.base_pin;
                        if row.tip == 0 {
                            row.tip = e.tip;
                        }
                    });
                    log(undo, || {
                        Step::Name(e.name, self.names.get(&e.name).copied())
                    });
                    self.names.insert(e.name, e.ref_id);
                }
            }
            Dec::Lease(v) => {
                let c = &mut self.counters;
                c.fence = c.fence.max(v.token);
                c.hlc_seq = c.hlc_seq.max(v.hlc);
                match v.event {
                    LEASE_CLAIM => {
                        if v.reclaimed != 0
                            && let Some(old) = self.leases.get(&v.reclaimed)
                        {
                            log(undo, || Step::Lease(v.reclaimed, Some(old.clone())));
                            if let Some(old) = self.leases.get_mut(&v.reclaimed) {
                                old.released = true;
                            }
                        }
                        log(undo, || {
                            Step::Lease(v.lease_id, self.leases.get(&v.lease_id).cloned())
                        });
                        self.leases.insert(
                            v.lease_id,
                            LeaseRow {
                                token: v.token,
                                uid: v.uid,
                                holder: v.holder,
                                expires: v.expires,
                                released: false,
                            },
                        );
                        log(undo, || {
                            Step::LeaseOf(v.uid, self.lease_of.get(&v.uid).copied())
                        });
                        self.lease_of.insert(v.uid, v.lease_id);
                    }
                    LEASE_RELEASE => {
                        // Applies only to the lease with that id and token ([F05 §9.4]).
                        if let Some(row) = self.leases.get(&v.lease_id)
                            && row.token == v.token
                        {
                            log(undo, || Step::Lease(v.lease_id, Some(row.clone())));
                            if let Some(row) = self.leases.get_mut(&v.lease_id) {
                                row.released = true;
                            }
                        }
                    }
                    // LeaseRec::decode admits only the two events.
                    _ => {}
                }
            }
            Dec::Marker(v) => {
                for e in &v.entries {
                    self.counters.hlc_seq = self.counters.hlc_seq.max(e.hlc);
                    log(undo, || {
                        Step::Marker(e.uid, self.markers.get(&e.uid).cloned())
                    });
                    self.markers.insert(
                        e.uid,
                        MarkerRow {
                            op: e.op,
                            ref_id: e.ref_id,
                            seq: e.seq,
                        },
                    );
                }
            }
            Dec::Idem(v) => {
                self.counters.hlc_seq = self.counters.hlc_seq.max(v.append_hlc);
                log(undo, || Step::Idem(v.key, self.idem.get(&v.key).cloned()));
                self.idem.insert(
                    v.key,
                    IdemRow {
                        payload: v.payload,
                        op: v.op,
                        result: v.result,
                        lsn,
                    },
                );
            }
            Dec::Pin(v) => {
                log(undo, || {
                    Step::Pin(v.ref_id, self.pins.get(&v.ref_id).cloned())
                });
                if v.op == 1 {
                    self.pins.insert(
                        v.ref_id,
                        PinRow {
                            set_lsn: v.set_lsn,
                            files: v.files.clone(),
                        },
                    );
                } else {
                    self.pins.remove(&v.ref_id);
                }
            }
            Dec::Checkpoint(v) => {
                let c = &mut self.counters;
                c.next_file_no = c.next_file_no.max(v.next_file_no);
                for s in &v.segments {
                    c.next_file_no = c.next_file_no.max(s.file_no.saturating_add(1));
                }
                for t in &v.retirements {
                    c.next_file_no = c.next_file_no.max(t.hist_file.saturating_add(1));
                }
                if v.ckflags & crate::format::CK_SET_CHANGE != 0 {
                    for s in &v.segments {
                        let k = (family::SEG_BASE, s.file_no);
                        log(undo, || Step::File(k, self.files.contains(&k)));
                        self.files.insert(k);
                    }
                    log(undo, || Step::Set(self.segments.clone(), self.set_lsn));
                    self.segments = v.segments.clone();
                    self.set_lsn = lsn;
                }
                for t in &v.retirements {
                    log(undo, || {
                        Step::Retired(t.extent, self.retired.get(&t.extent).copied())
                    });
                    self.retired.insert(t.extent, t.hist_file);
                    let k = (family::HIST, t.hist_file);
                    log(undo, || Step::File(k, self.files.contains(&k)));
                    self.files.insert(k);
                }
                for &f in &v.released {
                    log(undo, || Step::File(f, self.files.contains(&f)));
                    self.files.remove(&f);
                    log(undo, || Step::Released(f, self.released.contains(&f)));
                    self.released.insert(f);
                }
                log(undo, || Step::CheckpointHlc(self.last_checkpoint_hlc));
                self.last_checkpoint_hlc = v.append_hlc;
            }
            Dec::Intent(v) => {
                self.counters.hlc_seq = self.counters.hlc_seq.max(v.hlc);
                log(undo, || Step::Intent(lsn, self.intents.get(&lsn).cloned()));
                self.intents.insert(
                    lsn,
                    IntentRow {
                        rec: v,
                        state: IntentState::Open,
                    },
                );
            }
            Dec::IntentDone(v) => {
                self.counters.hlc_seq = self.counters.hlc_seq.max(v.hlc);
                if let Some(row) = self.intents.get(&v.intent_lsn) {
                    log(undo, || Step::Intent(v.intent_lsn, Some(row.clone())));
                }
                if let Some(row) = self.intents.get_mut(&v.intent_lsn) {
                    row.state = IntentState::Done {
                        recovered: v.dflags & 1 != 0,
                    };
                }
            }
            Dec::IntentAbort(v) => {
                self.counters.hlc_seq = self.counters.hlc_seq.max(v.hlc);
                if let Some(row) = self.intents.get(&v.intent_lsn) {
                    log(undo, || Step::Intent(v.intent_lsn, Some(row.clone())));
                }
                if let Some(row) = self.intents.get_mut(&v.intent_lsn) {
                    row.state = IntentState::Aborted { reason: v.reason };
                }
            }
            Dec::Runtime(v) => {
                for &(k, val, _) in &v.rows {
                    log(undo, || Step::Runtime(k, self.runtime.get(&k).copied()));
                    self.runtime.insert(k, val);
                }
            }
            Dec::ExtentHead(v) => {
                self.counters = self.counters.max(v.counters);
            }
            Dec::Noop => {}
        }
    }

    /// The segment snapshot's bytes: the state folded up to `upto`, with every table (and the facts, when kept).
    pub fn snapshot(&self, upto: u64) -> Vec<u8> {
        let mut w = Writer::with_capacity(4096);
        w.bytes(SNAP_MAGIC).u64(upto);
        let c = &self.counters;
        w.u64(c.commit_seq)
            .u32(c.next_id)
            .u32(c.next_anchor)
            .u64(c.fence)
            .u32(c.next_file_no)
            .u32(c.next_ref_id)
            .u64(c.hlc_seq)
            .u64(c.hlc_commit);
        w.uvar(self.commits.len() as u64);
        for (op, x) in &self.commits {
            w.u64(*op)
                .u64(x.digest)
                .u64(x.seq)
                .u32(x.ref_id)
                .u64(x.lsn)
                .u64(x.hlc)
                .u8(u8::from(x.parked));
        }
        w.uvar(self.refs.len() as u64);
        for (id, x) in &self.refs {
            w.u32(*id)
                .u64(x.name)
                .u8(x.rkind)
                .u64(x.tip)
                .u64(x.mover)
                .u8(u8::from(x.deleted))
                .u64(x.base_pin);
        }
        w.uvar(self.names.len() as u64);
        for (name, id) in &self.names {
            w.u64(*name).u32(*id);
        }
        w.uvar(self.uids.len() as u64);
        for (uid, n) in &self.uids {
            w.u64(*uid).u32(*n);
        }
        w.uvar(self.lease_of.len() as u64);
        for (uid, id) in &self.lease_of {
            w.u64(*uid).u64(*id);
        }
        w.uvar(self.orphans.len() as u64);
        for (r, op) in &self.orphans {
            w.u32(*r).u64(*op);
        }
        w.uvar(self.unparked.len() as u64);
        for (op, r) in &self.unparked {
            w.u64(*op).u32(*r);
        }
        w.uvar(self.nodes.len() as u64);
        for (n, uid) in &self.nodes {
            w.u32(*n).u64(*uid);
        }
        w.uvar(self.markers.len() as u64);
        for (uid, m) in &self.markers {
            w.u64(*uid).u64(m.op).u32(m.ref_id).u64(m.seq);
        }
        w.uvar(self.leases.len() as u64);
        for (id, l) in &self.leases {
            w.u64(*id)
                .u64(l.token)
                .u64(l.uid)
                .u64(l.holder)
                .bytes(&l.expires.to_bytes())
                .u8(u8::from(l.released));
        }
        w.uvar(self.idem.len() as u64);
        for (k, i) in &self.idem {
            w.u64(*k).u64(i.payload).u64(i.op).u64(i.result).u64(i.lsn);
        }
        w.uvar(self.pins.len() as u64);
        for (r, p) in &self.pins {
            w.u32(*r).u64(p.set_lsn).uvar(p.files.len() as u64);
            for &(fam, no) in &p.files {
                w.u8(fam).u32(no);
            }
        }
        w.uvar(self.intents.len() as u64);
        for (lsn, i) in &self.intents {
            let enc = i.rec.encode();
            w.u64(*lsn).vbytes(&enc);
            match i.state {
                IntentState::Open => w.u8(0).u8(0),
                IntentState::Done { recovered } => w.u8(1).u8(u8::from(recovered)),
                IntentState::Aborted { reason } => w.u8(2).u8(reason),
            };
        }
        w.uvar(self.runtime.len() as u64);
        for (k, v) in &self.runtime {
            w.u64(*k).u64(*v);
        }
        w.uvar(self.symbols.len() as u64);
        for s in &self.symbols {
            w.vbytes(s.as_bytes());
        }
        w.uvar(self.segments.len() as u64);
        for s in &self.segments {
            w.bytes(&s.to_bytes());
        }
        w.u64(self.set_lsn);
        for set in [&self.files, &self.released] {
            w.uvar(set.len() as u64);
            for &(fam, no) in set {
                w.u8(fam).u32(no);
            }
        }
        w.uvar(self.retired.len() as u64);
        for (e, h) in &self.retired {
            w.u32(*e).u32(*h);
        }
        w.u64(self.last_checkpoint_hlc);
        let facts: &[Fact] = if self.keep_facts { &self.facts } else { &[] };
        w.uvar(facts.len() as u64);
        for f in facts {
            encode_fact(f, &mut w);
        }
        w.buf
    }

    /// Decodes a snapshot: the state (keeping facts when `keep_facts`) and the lsn it folds up to.
    pub fn from_snapshot(b: &[u8], keep_facts: bool) -> Result<(State, u64), Short> {
        let mut r = Reader::new(b);
        if r.bytes(SNAP_MAGIC.len())? != SNAP_MAGIC {
            return Err(Short);
        }
        let upto = r.u64()?;
        let mut s = State {
            counters: Counters {
                commit_seq: r.u64()?,
                next_id: r.u32()?,
                next_anchor: r.u32()?,
                fence: r.u64()?,
                next_file_no: r.u32()?,
                next_ref_id: r.u32()?,
                hlc_seq: r.u64()?,
                hlc_commit: r.u64()?,
            },
            keep_facts,
            ..State::default()
        };
        for _ in 0..r.uvar(32)? {
            let op = r.u64()?;
            s.commits.insert(
                op,
                CommitRow {
                    digest: r.u64()?,
                    seq: r.u64()?,
                    ref_id: r.u32()?,
                    lsn: r.u64()?,
                    hlc: r.u64()?,
                    parked: r.u8()? != 0,
                },
            );
        }
        for _ in 0..r.uvar(32)? {
            let id = r.u32()?;
            let row = RefRow {
                name: r.u64()?,
                rkind: r.u8()?,
                tip: r.u64()?,
                mover: r.u64()?,
                deleted: r.u8()? != 0,
                base_pin: r.u64()?,
            };
            s.refs.insert(id, row);
        }
        for _ in 0..r.uvar(32)? {
            let name = r.u64()?;
            s.names.insert(name, r.u32()?);
        }
        for _ in 0..r.uvar(32)? {
            let uid = r.u64()?;
            s.uids.insert(uid, r.u32()?);
        }
        for _ in 0..r.uvar(32)? {
            let uid = r.u64()?;
            s.lease_of.insert(uid, r.u64()?);
        }
        for _ in 0..r.uvar(32)? {
            let k = r.u32()?;
            s.orphans.insert(k, r.u64()?);
        }
        for _ in 0..r.uvar(32)? {
            let k = r.u64()?;
            s.unparked.insert(k, r.u32()?);
        }
        for _ in 0..r.uvar(32)? {
            let n = r.u32()?;
            s.nodes.insert(n, r.u64()?);
        }
        for _ in 0..r.uvar(32)? {
            let uid = r.u64()?;
            s.markers.insert(
                uid,
                MarkerRow {
                    op: r.u64()?,
                    ref_id: r.u32()?,
                    seq: r.u64()?,
                },
            );
        }
        for _ in 0..r.uvar(32)? {
            let id = r.u64()?;
            let row = LeaseRow {
                token: r.u64()?,
                uid: r.u64()?,
                holder: r.u64()?,
                expires: Stamp::from_bytes(&r.array()?),
                released: r.u8()? != 0,
            };
            s.leases.insert(id, row);
        }
        for _ in 0..r.uvar(32)? {
            let k = r.u64()?;
            s.idem.insert(
                k,
                IdemRow {
                    payload: r.u64()?,
                    op: r.u64()?,
                    result: r.u64()?,
                    lsn: r.u64()?,
                },
            );
        }
        for _ in 0..r.uvar(32)? {
            let id = r.u32()?;
            let set_lsn = r.u64()?;
            let mut files = Vec::new();
            for _ in 0..r.uvar(32)? {
                let fam = r.u8()?;
                files.push((fam, r.u32()?));
            }
            s.pins.insert(id, PinRow { set_lsn, files });
        }
        for _ in 0..r.uvar(32)? {
            let lsn = r.u64()?;
            let rec = IntentRec::decode(r.vbytes()?)?;
            let state = match (r.u8()?, r.u8()?) {
                (0, _) => IntentState::Open,
                (1, x) => IntentState::Done { recovered: x != 0 },
                (2, x) => IntentState::Aborted { reason: x },
                _ => return Err(Short),
            };
            s.intents.insert(lsn, IntentRow { rec, state });
        }
        for _ in 0..r.uvar(32)? {
            let k = r.u64()?;
            s.runtime.insert(k, r.u64()?);
        }
        for _ in 0..r.uvar(32)? {
            let t = core::str::from_utf8(r.vbytes()?).map_err(|_| Short)?;
            s.symbols.push(t.to_owned());
        }
        for _ in 0..r.uvar(8)? {
            s.segments.push(SegRef::from_bytes(&r.array()?));
        }
        s.set_lsn = r.u64()?;
        for set in [&mut s.files, &mut s.released] {
            for _ in 0..r.uvar(32)? {
                let fam = r.u8()?;
                set.insert((fam, r.u32()?));
            }
        }
        for _ in 0..r.uvar(32)? {
            let e = r.u32()?;
            s.retired.insert(e, r.u32()?);
        }
        s.last_checkpoint_hlc = r.u64()?;
        for _ in 0..r.uvar(32)? {
            let f = decode_fact(&mut r)?;
            if keep_facts {
                s.facts.push(f);
            }
        }
        if !r.done() {
            return Err(Short);
        }
        Ok((s, upto))
    }
}

/// The toy's segment snapshot magic.
pub const SNAP_MAGIC: &[u8; 8] = b"TOYSEG03";

fn encode_fact(f: &Fact, w: &mut Writer) {
    match f {
        Fact::Commit { lsn, rec } => {
            w.u8(1).u64(*lsn).vbytes(&rec.encode());
        }
        Fact::RefUpdate { lsn, rec } => {
            w.u8(2).u64(*lsn).vbytes(&rec.encode());
        }
        Fact::Lease { lsn, rec } => {
            w.u8(3).u64(*lsn).vbytes(&rec.encode());
        }
        Fact::Semantic { lsn, hlc } => {
            w.u8(4).u64(*lsn).u64(*hlc);
        }
        Fact::Checkpoint { lsn, rec } => {
            w.u8(5).u64(*lsn).vbytes(&rec.encode());
        }
        Fact::Idem { lsn, key, op } => {
            w.u8(6).u64(*lsn).u64(*key).u64(*op);
        }
        Fact::Pin { lsn, rec } => {
            w.u8(7).u64(*lsn).vbytes(&rec.encode());
        }
        Fact::RefTable { lsn, rec } => {
            w.u8(8).u64(*lsn).vbytes(&rec.encode());
        }
        Fact::Marker { lsn, entry } => {
            let one = MarkerRec {
                entries: vec![entry.clone()],
            };
            w.u8(9).u64(*lsn).vbytes(&one.encode());
        }
        Fact::Group { start, end } => {
            w.u8(10).u64(*start).u64(*end);
        }
    }
}

fn decode_fact(r: &mut Reader<'_>) -> Result<Fact, Short> {
    let tag = r.u8()?;
    let lsn = r.u64()?;
    Ok(match tag {
        1 => Fact::Commit {
            lsn,
            rec: CommitRec::decode(r.vbytes()?)?,
        },
        2 => Fact::RefUpdate {
            lsn,
            rec: RefUpdateRec::decode(r.vbytes()?)?,
        },
        3 => Fact::Lease {
            lsn,
            rec: LeaseRec::decode(r.vbytes()?)?,
        },
        4 => Fact::Semantic { lsn, hlc: r.u64()? },
        5 => Fact::Checkpoint {
            lsn,
            rec: CheckpointRec::decode(r.vbytes()?)?,
        },
        6 => Fact::Idem {
            lsn,
            key: r.u64()?,
            op: r.u64()?,
        },
        7 => Fact::Pin {
            lsn,
            rec: PinRec::decode(r.vbytes()?)?,
        },
        8 => Fact::RefTable {
            lsn,
            rec: RefTableRec::decode(r.vbytes()?)?,
        },
        9 => {
            let entries = MarkerRec::decode(r.vbytes()?)?.entries;
            let [entry] = <[MarkerEntry; 1]>::try_from(entries).map_err(|_| Short)?;
            Fact::Marker { lsn, entry }
        }
        10 => Fact::Group {
            start: lsn,
            end: r.u64()?,
        },
        _ => return Err(Short),
    })
}

/// Folds one covered group into the slot being published ([F05 §10.2], [F16] P-50): counters take the maximum, table
/// pointers advance, a covered `Checkpoint` sets the segment set. `adopted`: the group is adopted by this publish
/// (P-65's seeded bug folds only its commits); P-50's seeded bug skips the set change of a covered `Checkpoint`.
pub fn fold_slot(s: &mut Slot, g: &Group, bugs: Bugs, adopted: bool) -> Result<(), Malformed> {
    for r in &g.recs {
        if adopted && bugs.on(Bug::P65T11SkipNonCommitRecord) && r.kind != kind::COMMIT {
            continue;
        }
        let bad = |e: Short| Malformed::from((r.lsn, r.kind, e));
        let c = &mut s.counters;
        match r.kind {
            kind::COMMIT => {
                let v = CommitRec::decode(r.payload().map_err(bad)?).map_err(bad)?;
                c.commit_seq = c.commit_seq.max(v.seq);
                c.hlc_seq = c.hlc_seq.max(v.append_hlc);
                c.hlc_commit = c.hlc_commit.max(v.hlc);
                for &(n, _) in &v.creates {
                    c.next_id = c.next_id.max(n.saturating_add(1));
                }
                s.seq_ring[(v.seq % 32) as usize] = (v.seq, r.lsn);
            }
            kind::REF_UPDATE => {
                let v = RefUpdateRec::decode(r.payload().map_err(bad)?).map_err(bad)?;
                c.hlc_seq = c.hlc_seq.max(v.hlc);
                if matches!(v.reason, REASON_CREATE | REASON_PARK) {
                    c.next_ref_id = c.next_ref_id.max(v.ref_id.saturating_add(1));
                }
            }
            kind::REF_TABLE => {
                let v = RefTableRec::decode(r.payload().map_err(bad)?).map_err(bad)?;
                s.refs_lsn = s.refs_lsn.max(r.lsn);
                for e in &v.entries {
                    c.next_ref_id = c.next_ref_id.max(e.ref_id.saturating_add(1));
                }
            }
            kind::LEASE => {
                let v = LeaseRec::decode(r.payload().map_err(bad)?).map_err(bad)?;
                c.fence = c.fence.max(v.token);
                c.hlc_seq = c.hlc_seq.max(v.hlc);
            }
            kind::MARKER => {
                let v = MarkerRec::decode(r.payload().map_err(bad)?).map_err(bad)?;
                s.markers_lsn = s.markers_lsn.max(r.lsn);
                for e in &v.entries {
                    c.hlc_seq = c.hlc_seq.max(e.hlc);
                }
            }
            kind::IDEM => {
                let v = IdemRec::decode(r.payload().map_err(bad)?).map_err(bad)?;
                c.hlc_seq = c.hlc_seq.max(v.append_hlc);
            }
            kind::PIN => {
                PinRec::decode(r.payload().map_err(bad)?).map_err(bad)?;
                s.pins_lsn = s.pins_lsn.max(r.lsn);
            }
            kind::CHECKPOINT => {
                let v = CheckpointRec::decode(r.payload().map_err(bad)?).map_err(bad)?;
                c.next_file_no = c.next_file_no.max(v.next_file_no);
                for x in &v.segments {
                    c.next_file_no = c.next_file_no.max(x.file_no.saturating_add(1));
                }
                for t in &v.retirements {
                    c.next_file_no = c.next_file_no.max(t.hist_file.saturating_add(1));
                }
                if bugs.on(Bug::P36CheckpointAdvancesHlc) {
                    c.hlc_seq = c.hlc_seq.max(v.append_hlc);
                }
                if v.ckflags & crate::format::CK_SET_CHANGE != 0
                    && !bugs.on(Bug::P50G12CheckpointSetUnpublished)
                {
                    s.segments = v.segments.clone();
                    s.checkpoint_lsn = v.upto_lsn;
                    s.active_log = v.active_log;
                }
            }
            kind::FS_INTENT => {
                let v = IntentRec::decode(r.payload().map_err(bad)?).map_err(bad)?;
                c.hlc_seq = c.hlc_seq.max(v.hlc);
            }
            kind::FS_INTENT_DONE => {
                let v = IntentDoneRec::decode(r.payload().map_err(bad)?).map_err(bad)?;
                c.hlc_seq = c.hlc_seq.max(v.hlc);
            }
            kind::FS_INTENT_ABORTED => {
                let v = IntentAbortRec::decode(r.payload().map_err(bad)?).map_err(bad)?;
                c.hlc_seq = c.hlc_seq.max(v.hlc);
            }
            kind::EXTENT_HEAD => {
                let v = ExtentHeadRec::decode(r.payload().map_err(bad)?).map_err(bad)?;
                *c = c.max(v.counters);
            }
            _ => {}
        }
    }
    Ok(())
}

/// Whether the commit carries `COMMIT_IMPORTED`.
pub fn imported(c: &CommitRec) -> bool {
    c.flags & COMMIT_IMPORTED != 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::{LEASE_CLAIM, RefEntry, Retirement};
    use proptest::prelude::*;

    fn rec(kind: u8, payload: Vec<u8>, lsn: u64) -> RecView {
        RecView {
            kind,
            lazy: false,
            lsn,
            has_symdefs: false,
            body: payload,
        }
    }

    fn group(recs: Vec<RecView>) -> Group {
        let start = recs.first().map_or(0, |r| r.lsn);
        Group {
            start,
            end: start + 100,
            chain_in: 0,
            chain_out: 0,
            recs,
            raw: Vec::new(),
        }
    }

    fn main_group() -> Group {
        let t = RefTableRec {
            entries: vec![RefEntry {
                ref_id: 0,
                name: MAIN,
                rkind: 1,
                ..RefEntry::default()
            }],
        };
        group(vec![rec(kind::REF_TABLE, t.encode(), 138)])
    }

    fn commit(op: u64, seq: u64, ref_old: u64, creates: Vec<(u32, u64)>, lsn: u64) -> Group {
        let c = CommitRec {
            op,
            seq,
            ref_id: 0,
            ref_old,
            creates,
            ..CommitRec::default()
        };
        group(vec![rec(kind::COMMIT, c.encode(), lsn)])
    }

    #[test]
    fn a_commit_moves_its_ref_only_when_its_cas_holds() {
        let mut s = State::new();
        s.apply(&main_group(), Bugs::NONE, false).unwrap();
        s.apply(&commit(10, 1, 0, vec![(1, 100)], 300), Bugs::NONE, false)
            .unwrap();
        assert_eq!(s.refs[&0].tip, 10);
        assert_eq!(s.nodes[&1], 100);
        assert_eq!(s.counters.next_id, 2);
        // A commit whose ref_old is stale is parked (I27′), not applied to its ref.
        s.apply(&commit(11, 2, 0, Vec::new(), 400), Bugs::NONE, false)
            .unwrap();
        assert_eq!(s.refs[&0].tip, 10);
        assert!(s.commits[&11].parked);
        assert_eq!(s.orphans[&0], 11);
        assert_eq!(s.unparked[&11], 0);
    }

    #[test]
    fn a_malformed_record_leaves_the_state_untouched() {
        let mut s = State::new().keeping_facts(true);
        let before = s.clone();
        let g = group(vec![
            rec(kind::REF_TABLE, main_group().recs[0].body.clone(), 138),
            rec(kind::COMMIT, vec![1, 2, 3], 200),
        ]);
        assert_eq!(
            s.apply(&g, Bugs::NONE, false),
            Err(Malformed {
                lsn: 200,
                kind: kind::COMMIT
            })
        );
        assert_eq!(s, before);
    }

    #[test]
    fn a_malformed_symbol_block_is_a_malformed_payload() {
        let rt = RuntimeRec {
            rows: vec![(1, 2, 0)],
        };
        // A SymDefs block whose count promises two definitions and holds one.
        let mut body = vec![2u8, 11, 1, 1, b'a'];
        body.extend_from_slice(&rt.encode());
        let r = RecView {
            kind: kind::FILE_OBS,
            lazy: true,
            lsn: 500,
            has_symdefs: true,
            body,
        };
        let mut s = State::new();
        assert!(s.apply(&group(vec![r]), Bugs::NONE, false).is_err());
        assert_eq!(s, State::new());
    }

    #[test]
    fn symbol_definitions_take_the_next_ids_and_new_strings() {
        let defs = |d: &[(u32, &str)]| {
            let mut w = Writer::default();
            w.uvar(d.len() as u64);
            for (id, t) in d {
                w.u8(11).uvar(u64::from(*id)).vbytes(t.as_bytes());
            }
            w.bytes(
                &RuntimeRec {
                    rows: vec![(1, 2, 0)],
                }
                .encode(),
            );
            RecView {
                kind: kind::FILE_OBS,
                lazy: true,
                lsn: 600,
                has_symdefs: true,
                body: w.buf,
            }
        };
        let mut s = State::new();
        s.apply(&group(vec![defs(&[(1, "a"), (2, "b")])]), Bugs::NONE, false)
            .unwrap();
        assert_eq!(s.symbols, ["a", "b"]);
        // SD-1: not the next id; SD-2: a string already defined (in the table or earlier in the group).
        for bad in [
            vec![defs(&[(4, "c")])],
            vec![defs(&[(3, "a")])],
            vec![defs(&[(3, "c"), (4, "c")])],
            vec![defs(&[(3, "c")]), defs(&[(4, "c")])],
        ] {
            let before = s.clone();
            assert!(s.apply(&group(bad), Bugs::NONE, false).is_err());
            assert_eq!(s, before);
        }
        s.apply(
            &group(vec![defs(&[(3, "c")]), defs(&[(4, "d")])]),
            Bugs::NONE,
            false,
        )
        .unwrap();
        assert_eq!(s.symbols, ["a", "b", "c", "d"]);
    }

    #[test]
    fn p65_skips_non_commit_records_of_adopted_groups() {
        let lease = LeaseRec {
            event: LEASE_CLAIM,
            lease_id: 1,
            token: 1,
            hlc: 5,
            uid: 9,
            holder: 3,
            expires: Stamp::NEVER,
            ttl_ms: 0,
            decided_at: Stamp::NEVER,
            reclaimed: 0,
            reason: 0,
        };
        let g = group(vec![rec(kind::LEASE, lease.encode(), 500)]);
        let mut s = State::new();
        s.apply(&g, Bugs::only(Bug::P65T11SkipNonCommitRecord), false)
            .unwrap();
        assert_eq!(s.counters.fence, 1);
        let mut t = State::new();
        t.apply(&g, Bugs::only(Bug::P65T11SkipNonCommitRecord), true)
            .unwrap();
        assert_eq!(t.counters.fence, 0);
    }

    #[test]
    fn facts_are_kept_only_when_asked() {
        let mut off = State::new();
        let mut on = State::new().keeping_facts(true);
        let entry = MarkerEntry {
            mkind: 1,
            uid: 0x900,
            ref_id: 0,
            op: 11,
            seq: 2,
            hlc: 7,
            status: 1,
        };
        let marker = MarkerRec {
            entries: vec![entry.clone()],
        };
        let c = CommitRec {
            op: 11,
            seq: 2,
            ref_old: 10,
            ..CommitRec::default()
        };
        let completion = group(vec![
            rec(kind::COMMIT, c.encode(), 500),
            rec(kind::MARKER, marker.encode(), 560),
        ]);
        let lazy = Group {
            recs: vec![RecView {
                kind: kind::FILE_OBS,
                lazy: true,
                lsn: 700,
                has_symdefs: false,
                body: RuntimeRec {
                    rows: vec![(1, 2, 0)],
                }
                .encode(),
            }],
            ..group(Vec::new())
        };
        for s in [&mut off, &mut on] {
            s.apply(&main_group(), Bugs::NONE, false).unwrap();
            s.apply(&commit(10, 1, 0, vec![(1, 100)], 300), Bugs::NONE, false)
                .unwrap();
            s.apply(&completion, Bugs::NONE, false).unwrap();
            s.apply(&lazy, Bugs::NONE, false).unwrap();
        }
        assert!(off.facts.is_empty());
        // Every group with facts is announced by its bounds; a lazy group has none (P-52's composition check).
        assert_eq!(on.facts.len(), 7, "{:?}", on.facts);
        assert_eq!(
            on.facts[0],
            Fact::Group {
                start: 138,
                end: 238
            }
        );
        assert!(matches!(on.facts[1], Fact::RefTable { lsn: 138, .. }));
        assert_eq!(
            on.facts[2],
            Fact::Group {
                start: 300,
                end: 400
            }
        );
        assert!(matches!(on.facts[3], Fact::Commit { lsn: 300, .. }));
        assert_eq!(
            on.facts[4],
            Fact::Group {
                start: 500,
                end: 600
            }
        );
        assert!(matches!(on.facts[5], Fact::Commit { lsn: 500, .. }));
        assert_eq!(on.facts[6], Fact::Marker { lsn: 560, entry });
        // A snapshot carries the facts only when they are kept, and a reader keeps them only when asked.
        let (a, _) = State::from_snapshot(&on.snapshot(1), true).unwrap();
        assert_eq!(a.facts, on.facts);
        let (b, _) = State::from_snapshot(&on.snapshot(1), false).unwrap();
        assert!(b.facts.is_empty());
        let (c, _) = State::from_snapshot(&off.snapshot(1), true).unwrap();
        assert!(c.facts.is_empty());
    }

    #[test]
    fn snapshots_round_trip() {
        let mut s = State::new().keeping_facts(true);
        s.apply(&main_group(), Bugs::NONE, false).unwrap();
        s.apply(&commit(10, 1, 0, vec![(1, 100)], 300), Bugs::NONE, false)
            .unwrap();
        let ck = CheckpointRec {
            ckflags: crate::format::CK_SET_CHANGE
                | crate::format::CK_RETIREMENTS
                | crate::format::CK_RELEASED,
            append_hlc: 77,
            next_file_no: 5,
            upto_lsn: 300,
            active_log: 2,
            segments: vec![SegRef {
                file_no: 3,
                kind: 1,
                upto_lsn: 300,
                digest: [1; 16],
            }],
            retirements: vec![Retirement {
                extent: 1,
                hist_file: 4,
                total_len: 9,
                digest: [2; 16],
            }],
            released: vec![(family::SEG_BASE, 2)],
        };
        s.apply(
            &group(vec![rec(kind::CHECKPOINT, ck.encode(), 400)]),
            Bugs::NONE,
            false,
        )
        .unwrap();
        assert_eq!(s.last_checkpoint_hlc, 77);
        assert!(s.released.contains(&(family::SEG_BASE, 2)));
        assert_eq!(s.retired[&1], 4);
        s.runtime.insert(4, 5);
        s.symbols.push("s1".to_owned());
        let b = s.snapshot(777);
        let (t, upto) = State::from_snapshot(&b, true).unwrap();
        assert_eq!(upto, 777);
        assert_eq!(t, s);
        assert!(State::from_snapshot(&b[..b.len() - 1], true).is_err());
    }

    /// A small random group: records of the kinds whose application touches the most tables.
    fn arb_group(lsn: u64) -> impl Strategy<Value = Group> {
        let r = (0u8..6, 1u64..8, 0u32..6, any::<bool>());
        proptest::collection::vec(r, 1..5).prop_map(move |specs| {
            let recs = specs
                .into_iter()
                .enumerate()
                .map(|(i, (k, a, b, flag))| {
                    let at = lsn + i as u64 * 10;
                    match k {
                        0 => rec(
                            kind::COMMIT,
                            CommitRec {
                                op: a,
                                seq: a,
                                ref_id: b % 2,
                                ref_old: if flag { a - 1 } else { 0 },
                                creates: vec![(b + 1, a * 100)],
                                ..CommitRec::default()
                            }
                            .encode(),
                            at,
                        ),
                        1 => rec(
                            kind::REF_UPDATE,
                            RefUpdateRec {
                                reason: [REASON_CREATE, REASON_PARK, REASON_MOVE, REASON_DELETE]
                                    [b as usize % 4],
                                ref_id: b % 3,
                                op: a,
                                hlc: a,
                                old: 0,
                                new: a,
                            }
                            .encode(),
                            at,
                        ),
                        2 => rec(
                            kind::LEASE,
                            LeaseRec {
                                event: if flag { LEASE_CLAIM } else { LEASE_RELEASE },
                                lease_id: a,
                                token: a,
                                hlc: a,
                                uid: u64::from(b),
                                holder: 1,
                                expires: Stamp::NEVER,
                                ttl_ms: 0,
                                decided_at: Stamp::NEVER,
                                reclaimed: if flag { a - 1 } else { 0 },
                                reason: 1,
                            }
                            .encode(),
                            at,
                        ),
                        3 => rec(
                            kind::PIN,
                            PinRec {
                                op: if flag { 1 } else { 2 },
                                holder: 1,
                                ref_id: b,
                                set_lsn: a,
                                files: vec![(family::SEG_BASE, b)],
                            }
                            .encode(),
                            at,
                        ),
                        4 => rec(
                            kind::CHECKPOINT,
                            CheckpointRec {
                                ckflags: crate::format::CK_SET_CHANGE | crate::format::CK_RELEASED,
                                append_hlc: a,
                                next_file_no: b + 1,
                                upto_lsn: a,
                                active_log: 1,
                                segments: vec![SegRef {
                                    file_no: b + 1,
                                    kind: 1,
                                    upto_lsn: a,
                                    digest: [0; 16],
                                }],
                                retirements: Vec::new(),
                                released: vec![(family::SEG_BASE, b)],
                            }
                            .encode(),
                            at,
                        ),
                        _ => RecView {
                            kind: kind::FILE_OBS,
                            lazy: true,
                            lsn: at,
                            has_symdefs: false,
                            body: RuntimeRec {
                                rows: vec![(a, u64::from(b), 0)],
                            }
                            .encode(),
                        },
                    }
                })
                .collect();
            group(recs)
        })
    }

    proptest! {
        #![proptest_config(crate::testing::proptest_config())]

        /// The scratch layer: applying groups through an undo log and taking them back restores the state exactly, and
        /// applying them logged gives the same state as applying them plainly.
        #[test]
        fn undo_restores_the_state(base in proptest::collection::vec(arb_group(1000), 0..4),
                                   pending in proptest::collection::vec(arb_group(5000), 1..4),
                                   facts in any::<bool>()) {
            let mut s = State::new().keeping_facts(facts);
            s.apply(&main_group(), Bugs::NONE, false).unwrap();
            for g in &base {
                let _ = s.apply(g, Bugs::NONE, false);
            }
            let before = s.clone();
            let mut plain = s.clone();
            let mut undo = Undo::default();
            for g in &pending {
                let a = s.apply_logged(g, Bugs::NONE, true, &mut undo);
                let b = plain.apply(g, Bugs::NONE, true);
                prop_assert_eq!(a.is_ok(), b.is_ok());
            }
            prop_assert_eq!(&s, &plain);
            s.undo(undo);
            prop_assert_eq!(s, before);
        }
    }
}
