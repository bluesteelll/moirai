//! The ledger: the harness-side record of every effect a subject attempts, acknowledges and observes.
//!
//! [F15 §6.1] G-14: harness-side records are outside the fault model and are never lost. [60 §4.4] item 4: "an
//! acknowledged commit is never missing — acknowledgements are recorded outside the store, by the harness … before the
//! process prints them". [60 §3.13] GT4 names the kinds of acknowledged durable effect: commit, lease token, ref move,
//! marker effect, intent outcome, `gitmap` entry, backup ([`EffectKind`]).
//!
//! A subject reports through four hooks, each called from harness code (a task body or the driver), never from inside a
//! `Vfs` call:
//! - [`Ledger::begin`] before the call that may apply an operation: the operation's id (its idempotency key, so a retry
//!   calls `begin` again with the same id), its durability class and the effects it writes, all or nothing;
//! - [`Ledger::ack`] when the operation is acknowledged (for a durable class: after its covering flush, publish and
//!   identity check; for a lazy class: after its publish);
//! - [`Ledger::observe`] when a reader sees a value (the pre-crash half of read freshness: a durable-class value a reader
//!   saw was flushed, so it must survive — [F13 §3.8] I-G2; [60 §4.4] item 3, F-A1);
//! - [`Ledger::fail`] for a violation the subject detects itself.
//!
//! Every entry is stamped with the world's scheduling point and failed-flush count at the moment it is recorded, so a
//! crash image taken at point P is judged against exactly the entries recorded before P. Every `begin` and `ack` also
//! goes into the world's event trace as a harness note ([`NOTE_BEGIN`], [`NOTE_ACK`]): [F13 §1.4]'s trace predicates
//! read the acknowledgements from the same replayable trace as the flush and lock events.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

use crate::SimWorld;

/// The kind of an effect. The kinds are labels for reports; every kind follows the same rules (module
/// [`crate::enumerate`] "Assertions").
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum EffectKind {
    /// A commit: key the commit (or its idempotency key), value a digest of its content.
    Commit,
    /// A ref: key the ref, value its target.
    Ref,
    /// A marker ([72 M1]): key the marker, value its content; written in the same operation as its commit.
    Marker,
    /// A lease ([90 §10.1]): key the lease, value its token; a release writes `None`.
    Lease,
    /// An `FsIntent` outcome ([40 §3.4]).
    Intent,
    /// A `gitmap` entry.
    GitMap,
    /// A backup.
    Backup,
    /// An idempotency record: key the idempotency key, value the result it returns (I14′, I27′).
    Idempotency,
    /// A subject-defined kind.
    Other(u16),
}

/// The tag of the trace note a [`Ledger::begin`] emits ([`crate::EventKind::Note`] `a`): `b` the operation's id, `c`
/// 0 for [`Class::Durable`], 1 for [`Class::Lazy`].
pub const NOTE_BEGIN: u64 = 0x4C45_4447_4552_0001;

/// The tag of the trace note a [`Ledger::ack`] emits: `b` the operation's id, `c` 0.
pub const NOTE_ACK: u64 = 0x4C45_4447_4552_0002;

/// One effect's place: a register that an operation sets to a value or clears.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct EffectKey {
    /// The kind.
    pub kind: EffectKind,
    /// The key within the kind.
    pub key: u64,
}

impl EffectKey {
    /// The key `key` of kind `kind`.
    pub const fn new(kind: EffectKind, key: u64) -> EffectKey {
        EffectKey { kind, key }
    }
}

/// A store's state as the subject reads it: every register that holds a value. An absent key is clear.
pub type EffectSet = BTreeMap<EffectKey, u64>;

/// One effect of an operation: the register and the value it sets (`None`: clears it).
pub type EffectWrite = (EffectKey, Option<u64>);

/// The durability class of an operation ([F15 §4.1]).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Class {
    /// Acknowledged only once durable: it survives every crash and every failed flush.
    Durable,
    /// Acknowledged once published: it survives the death of any process, but may be lost at a system crash or through a
    /// failed flush in any process (FM-3.6).
    Lazy,
}

/// What one ledger entry records.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Rec {
    Begin {
        op: u64,
        class: Class,
        writes: Vec<EffectWrite>,
    },
    Ack {
        op: u64,
    },
    Observe {
        key: EffectKey,
        value: Option<u64>,
    },
    Fail(String),
}

/// One ledger entry with its stamps.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Entry {
    /// The world's scheduling points passed when it was recorded: a crash at point P follows exactly the entries with
    /// `at < P`.
    pub(crate) at: u64,
    /// The world's failed file flushes when it was recorded.
    pub(crate) ff: u64,
    pub(crate) rec: Rec,
}

/// The harness-side record of one run (see the module documentation). Cheap to clone; clones share the record, so a
/// subject moves clones into its task closures.
#[derive(Clone)]
pub struct Ledger {
    log: Arc<Mutex<Vec<Entry>>>,
    world: SimWorld,
}

impl core::fmt::Debug for Ledger {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Ledger")
            .field("entries", &self.lock().len())
            .finish()
    }
}

impl Ledger {
    /// An empty ledger stamping its entries from `world`.
    pub fn new(world: &SimWorld) -> Ledger {
        Ledger {
            log: Arc::new(Mutex::new(Vec::new())),
            world: world.clone(),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Entry>> {
        self.log.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn push(&self, rec: Rec) {
        let note = match &rec {
            Rec::Begin { op, class, .. } => {
                Some((NOTE_BEGIN, *op, u64::from(*class == Class::Lazy)))
            }
            Rec::Ack { op } => Some((NOTE_ACK, *op, 0)),
            Rec::Observe { .. } | Rec::Fail(_) => None,
        };
        let (at, ff) = self.world.stamp(note);
        self.lock().push(Entry { at, ff, rec });
    }

    /// Operation `op` is about to be attempted: if it takes effect, it sets every register of `writes` at once, with
    /// durability `class`. A retry calls `begin` again with the same id and the same writes. Effects that the protocol
    /// may apply without each other are separate operations: a commit's ref move, which a failed CAS at replay refuses
    /// while the commit stays (parked on `orphans/<R>`, [F16 §10] P-70), is not in the commit's operation; its markers,
    /// which travel in the commit's group ([72 M1]), are.
    pub fn begin(&self, op: u64, class: Class, writes: &[EffectWrite]) {
        self.push(Rec::Begin {
            op,
            class,
            writes: writes.to_vec(),
        });
    }

    /// Operation `op` was acknowledged to its caller.
    pub fn ack(&self, op: u64) {
        self.push(Rec::Ack { op });
    }

    /// A reader saw register `key` hold `value` (`None`: clear). A value marks its one writer done (it had taken effect,
    /// so a durable one must survive: I-G2); a clear register marks nothing, since the read may have come before any
    /// operation set it.
    pub fn observe(&self, key: EffectKey, value: Option<u64>) {
        self.push(Rec::Observe { key, value });
    }

    /// The subject detected a violation itself (for example a read that went backwards).
    pub fn fail(&self, message: impl Into<String>) {
        self.push(Rec::Fail(message.into()));
    }

    /// The number of acknowledgements recorded so far.
    pub fn acks(&self) -> usize {
        self.lock()
            .iter()
            .filter(|e| matches!(e.rec, Rec::Ack { .. }))
            .count()
    }

    /// The number of observations recorded so far.
    pub fn observations(&self) -> usize {
        self.lock()
            .iter()
            .filter(|e| matches!(e.rec, Rec::Observe { .. }))
            .count()
    }

    pub(crate) fn entries(&self) -> Vec<Entry> {
        self.lock().clone()
    }
}

/// One operation as the entries before a crash point describe it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Op {
    pub(crate) id: u64,
    pub(crate) class: Class,
    pub(crate) writes: Vec<EffectWrite>,
    /// The index of its first `begin` entry: the earliest instant it could take effect.
    pub(crate) begin: u64,
    /// The failed-flush count at its first `begin`: a failed flush counted after it may have followed its effect, and
    /// may then have lost a lazy effect (FM-3.6).
    pub(crate) begin_ff: u64,
    /// The index of its first acknowledgement, or of the first observation of a value that only it wrote (never of a
    /// clear register): it had taken effect by then.
    pub(crate) done: Option<u64>,
}

/// The operations of a ledger prefix, indexed by register.
#[derive(Clone, Debug, Default)]
pub(crate) struct OpTable {
    pub(crate) ops: Vec<Op>,
    /// Per register, the operations that write it and the value each writes.
    pub(crate) by_key: BTreeMap<EffectKey, Vec<(usize, Option<u64>)>>,
    /// Problems of the run itself (a phantom observation, an acknowledgement of an operation never begun, a reused id,
    /// a subject's own failure); reported once per run, from the whole ledger.
    pub(crate) problems: Vec<String>,
}

impl OpTable {
    /// The table of the entries recorded before scheduling point `before` (`u64::MAX`: all of them).
    pub(crate) fn build(entries: &[Entry], before: u64) -> OpTable {
        let mut t = OpTable::default();
        let mut index: BTreeMap<u64, usize> = BTreeMap::new();
        for (seq, e) in entries.iter().enumerate() {
            if e.at >= before {
                break;
            }
            let seq = seq as u64;
            match &e.rec {
                Rec::Begin { op, class, writes } => {
                    // The last write of a register within one operation is the one it leaves.
                    let mut dedup: Vec<EffectWrite> = Vec::with_capacity(writes.len());
                    for &(k, v) in writes {
                        if let Some(w) = dedup.iter_mut().find(|w| w.0 == k) {
                            w.1 = v;
                        } else {
                            dedup.push((k, v));
                        }
                    }
                    match index.get(op) {
                        Some(&i) => {
                            let o = &t.ops[i];
                            if o.class != *class || o.writes != dedup {
                                t.problems.push(format!(
                                    "ledger: operation {op} begun again with other effects or another class"
                                ));
                            }
                        }
                        None => {
                            let i = t.ops.len();
                            index.insert(*op, i);
                            for &(k, v) in &dedup {
                                t.by_key.entry(k).or_default().push((i, v));
                            }
                            t.ops.push(Op {
                                id: *op,
                                class: *class,
                                writes: dedup,
                                begin: seq,
                                begin_ff: e.ff,
                                done: None,
                            });
                        }
                    }
                }
                Rec::Ack { op } => match index.get(op) {
                    Some(&i) => t.mark_done(i, seq),
                    None => t.problems.push(format!(
                        "ledger: operation {op} acknowledged but never begun"
                    )),
                },
                Rec::Observe { key, value } => {
                    let writers: &[(usize, Option<u64>)] =
                        t.by_key.get(key).map_or(&[], |w| w.as_slice());
                    let mut matching = writers.iter().filter(|(_, v)| v == value).map(|&(i, _)| i);
                    let (first, second) = (matching.next(), matching.next());
                    // Only a value is evidence that its one writer took effect. A clear register may be clear from the
                    // start: the entry is recorded after the read, which may have preceded every operation that set the
                    // register, so its clearing operation need not have run.
                    match (first, second, value) {
                        (Some(i), None, Some(_)) => t.mark_done(i, seq),
                        (None, _, Some(v)) => t.problems.push(format!(
                            "read: a reader saw {key:?} = {v:#x}, a value no operation had written (a phantom read)"
                        )),
                        _ => {}
                    }
                }
                Rec::Fail(m) => t.problems.push(format!("subject: {m}")),
            }
        }
        t
    }

    fn mark_done(&mut self, i: usize, seq: u64) {
        let o = &mut self.ops[i];
        if o.done.is_none() {
            o.done = Some(seq);
        }
    }
}
