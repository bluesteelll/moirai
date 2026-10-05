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
//!   calls `begin` again with the same id), its durability class and the effects it writes, all or nothing; or
//!   [`Ledger::begin_kept_in_head`] for a durable publish of a field kept only in `HEAD` ([F04 §6]: a flag, `boot_id`);
//! - [`Ledger::ack`] when the operation is acknowledged (for a durable class: after its covering flush, publish and
//!   identity check; for a lazy class: after its publish);
//! - [`Ledger::observe`] when a reader sees a value (the pre-crash half of read freshness: a durable-class value a reader
//!   saw was flushed, so it must survive — [F13 §3.8] I-G2; [60 §4.4] item 3, F-A1);
//! - [`Ledger::fail`] for a violation the subject detects itself.
//!
//! and through six more that feed the enumerator's own verdicts ([F16 §17.2]):
//! - [`Ledger::refused`] when a workload operation refuses (exit 7): judged against the run's faults (avail, module
//!   `refusal`);
//! - [`Ledger::group_bytes`] with an acknowledged group's identity — the 8 chain bytes before it and its trailer — at
//!   their places: they stay there while the file exists (chain, I-G3);
//! - [`Ledger::observe_covered`] for an observation with the position of the group it came from and the `durable_lsn`
//!   of the slot the reader read (fresh, I-G2: a reader never sees an uncovered durable group);
//! - [`Ledger::expect_names`] with where a file must be, by node identity, for each recovered value of a register (ns);
//! - [`Ledger::setup_fault`] for a file the setup put in place damaged (an FM-10 act before the run, which explains
//!   refusals and diagnoses that name it), and [`Ledger::setup_ignored`] for a file the setup put in place that the
//!   protocol must ignore (it explains nothing).
//!
//! Every entry is stamped with the world's scheduling point and failed-flush count at the moment it is recorded, so a
//! crash image taken at point P is judged against exactly the entries recorded before P. Every `begin` and `ack` also
//! goes into the world's event trace as a harness note ([`NOTE_BEGIN`], [`NOTE_ACK`]): [F13 §1.4]'s trace predicates
//! read the acknowledgements from the same replayable trace as the flush and lock events.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use super::refusal::Refused;
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
/// 0 for [`Class::Durable`], 1 for [`Class::Lazy`]. A [`Ledger::begin_kept_in_head`] emits it too, with `c` 0: its
/// operation is of the durable class.
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
        /// The effects are fields kept only in `HEAD` (FM-3.6; [`Ledger::begin_kept_in_head`]).
        head: bool,
        writes: Vec<EffectWrite>,
    },
    Ack {
        op: u64,
    },
    Observe {
        key: EffectKey,
        value: Option<u64>,
        /// The end of the group the value came from and the `durable_lsn` of the slot the reader read
        /// ([`Ledger::observe_covered`]).
        covered: Option<(u64, u64)>,
    },
    Fail(String),
    Refused {
        who: String,
        refusal: Refused,
    },
    Bytes {
        op: u64,
        path: PathBuf,
        offset: u64,
        bytes: Vec<u8>,
    },
    Names {
        path: PathBuf,
        node: Option<u64>,
        key: EffectKey,
        rules: Vec<(Option<u64>, Vec<PathBuf>)>,
    },
    SetupFault(PathBuf),
    SetupIgnored(PathBuf),
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
            _ => None,
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
            head: false,
            writes: writes.to_vec(),
        });
    }

    /// Operation `op`, a durable publish ([F04 §9.2]) whose effects `writes` are fields kept only in `HEAD` ([F04 §6]: a
    /// flag, `boot_id`), is about to be attempted; it is acknowledged ([`Ledger::ack`]) only after its `HEAD` flush
    /// returned. It is of the durable class, with one allowance ([F15 §3.3] FM-3.6 "Fields kept only in `HEAD`"; module
    /// `verdict`): until it is acknowledged, its value may vanish once a flush failed after it began (the slot sectors are
    /// poisoned: reads may alternate, and a crash may leave either value) or once the system crashed (its slot writes,
    /// which readers see before the flush, were never flushed: FM-1.1). A reader's observation of the value then does not
    /// make the operation required. A retry calls this method again with the same id and the same writes.
    pub fn begin_kept_in_head(&self, op: u64, writes: &[EffectWrite]) {
        self.push(Rec::Begin {
            op,
            class: Class::Durable,
            head: true,
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
        self.push(Rec::Observe {
            key,
            value,
            covered: None,
        });
    }

    /// As [`Ledger::observe`], for a value read from the log tail: the group that holds it ends at `group_end`, and the
    /// slot the reader read had `durable_lsn`. A durable-class value from a group ending beyond that `durable_lsn` is a
    /// read of an uncovered durable group (I-G2, [F16] P-49, P-57: `committed_lsn` never passes a pending durable group;
    /// fresh, [F16 §17.2]).
    pub fn observe_covered(
        &self,
        key: EffectKey,
        value: Option<u64>,
        group_end: u64,
        durable_lsn: u64,
    ) {
        self.push(Rec::Observe {
            key,
            value,
            covered: Some((group_end, durable_lsn)),
        });
    }

    /// The subject detected a violation itself (for example a read that went backwards).
    pub fn fail(&self, message: impl Into<String>) {
        self.push(Rec::Fail(message.into()));
    }

    /// A workload operation run by `who` refused (exit 7) with `refusal`. The enumerator judges it against the faults
    /// the run injected (avail, [F16 §17.2]); a refusal no fault explains fails the run.
    pub fn refused(&self, who: &str, refusal: Refused) {
        self.push(Rec::Refused {
            who: who.to_owned(),
            refusal,
        });
    }

    /// Part of operation `op`'s group identity: `bytes` at `offset` of the file at the absolute `path` — the 8 chain
    /// bytes before the group, or its trailer ([F05 §4.2], [F16] P-53). Once `op` is acknowledged as a durable
    /// operation, every recovered world holds these bytes there while a file exists at `path` (chain, I-G3: the valid
    /// log stays a prefix of the chain, and no extent file is rewritten in place, [F16] P-74). Called before or after
    /// the acknowledgement; the trailer is checked even when the predecessor's place no longer exists.
    pub fn group_bytes(&self, op: u64, path: &Path, offset: u64, bytes: &[u8]) {
        self.push(Rec::Bytes {
            op,
            path: path.to_owned(),
            offset,
            bytes: bytes.to_vec(),
        });
    }

    /// The file at the absolute `path` now (its node, whatever names it later) must, in a recovered world whose state
    /// gives register `key` a value one of `rules` names, be at one of that rule's paths — or have no name, when the
    /// rule lists none ([F16 §17.2] ns: the simulator's namespace check, by node identity). For an intent: `None`
    /// (no `FsIntent`) at the source, open at the source or the destination, done at the destination or in the trash
    /// ([40 §3.4], [F16] P-16–P-18). A register value no rule names expects nothing.
    pub fn expect_names(&self, path: &Path, key: EffectKey, rules: &[(Option<u64>, &[PathBuf])]) {
        let node = self.world.node_at(path);
        self.push(Rec::Names {
            path: path.to_owned(),
            node,
            key,
            rules: rules.iter().map(|(v, p)| (*v, p.to_vec())).collect(),
        });
    }

    /// The setup put the file at the absolute `path` in place damaged, where the store needs it — an FM-10 act before
    /// the run ([F15] FM-10.1: a fatal slot, a rewritten or truncated store file): it explains only the refusals that
    /// concern that file ([`super::Refusal::Damaged`] naming it, or [`super::Refused::concerning`] it) and the diagnoses
    /// that name it, and, on a slot file, [`super::Refusal::NoValidSlot`] and [`super::Refusal::FatalSlot`] (module
    /// `refusal`). A refusal that names no file is never explained by a setup fault.
    pub fn setup_fault(&self, path: &Path) {
        self.push(Rec::SetupFault(path.to_owned()));
    }

    /// The setup put in place, at the absolute `path`, a file that the protocol must ignore — an FM-10 act before the
    /// run that a correct store keeps out of every state: an extent of another epoch or a misplaced copy of an extent,
    /// which the epoch and position checks at a scan start reject ([F16] P-54, P-55), a stray file below the lowest
    /// extent the store keeps. It explains no refusal, diagnosis or other finding: a refusal that concerns it shows that
    /// the store did not ignore it (module `refusal`). The chain check skips it, as any file the setup wrote.
    pub fn setup_ignored(&self, path: &Path) {
        self.push(Rec::SetupIgnored(path.to_owned()));
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
    /// Its effects are fields kept only in `HEAD` ([`Ledger::begin_kept_in_head`]).
    pub(crate) head: bool,
    pub(crate) writes: Vec<EffectWrite>,
    /// The index of its first `begin` entry: the earliest instant it could take effect.
    pub(crate) begin: u64,
    /// The failed-flush count at its first `begin`: a failed flush counted after it may have followed its effect, and
    /// may then have lost a lazy effect (FM-3.6).
    pub(crate) begin_ff: u64,
    /// The index of its first acknowledgement, or of the first observation of a value that only it wrote (never of a
    /// clear register): it had taken effect by then.
    pub(crate) done: Option<u64>,
    /// The index of its first acknowledgement.
    pub(crate) acked: Option<u64>,
}

/// Part of an operation's group identity ([`Ledger::group_bytes`]).
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct GroupBytes {
    /// The operation's index in [`OpTable::ops`].
    pub(crate) op: usize,
    pub(crate) path: PathBuf,
    pub(crate) offset: u64,
    pub(crate) bytes: Vec<u8>,
}

/// A namespace expectation ([`Ledger::expect_names`]).
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NameRule {
    /// The path that named the file when the expectation was recorded, and its node then.
    pub(crate) path: PathBuf,
    pub(crate) node: Option<u64>,
    pub(crate) key: EffectKey,
    pub(crate) rules: Vec<(Option<u64>, Vec<PathBuf>)>,
}

/// The operations of a ledger prefix, indexed by register.
#[derive(Clone, Debug, Default)]
pub(crate) struct OpTable {
    pub(crate) ops: Vec<Op>,
    /// Per register, the operations that write it and the value each writes.
    pub(crate) by_key: BTreeMap<EffectKey, Vec<(usize, Option<u64>)>>,
    /// Problems of the run itself (a phantom observation, an acknowledgement of an operation never begun, a reused id,
    /// a subject's own failure, a read of an uncovered durable group); reported once per run, from the whole ledger.
    pub(crate) problems: Vec<String>,
    /// The workload's refusals: who, and why.
    pub(crate) refusals: Vec<(String, Refused)>,
    /// The group identities recorded.
    pub(crate) bytes: Vec<GroupBytes>,
    /// The namespace expectations recorded.
    pub(crate) names: Vec<NameRule>,
    /// The files the setup declared damaged.
    pub(crate) setup_faults: Vec<PathBuf>,
    /// The files the setup declared the protocol must ignore.
    pub(crate) setup_ignored: Vec<PathBuf>,
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
                Rec::Begin {
                    op,
                    class,
                    head,
                    writes,
                } => {
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
                            if o.class != *class || o.head != *head || o.writes != dedup {
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
                                head: *head,
                                writes: dedup,
                                begin: seq,
                                begin_ff: e.ff,
                                done: None,
                                acked: None,
                            });
                        }
                    }
                }
                Rec::Ack { op } => match index.get(op) {
                    Some(&i) => {
                        t.mark_done(i, seq);
                        let o = &mut t.ops[i];
                        if o.acked.is_none() {
                            o.acked = Some(seq);
                        }
                    }
                    None => t.problems.push(format!(
                        "ledger: operation {op} acknowledged but never begun"
                    )),
                },
                Rec::Observe {
                    key,
                    value,
                    covered,
                } => {
                    let writers: &[(usize, Option<u64>)] =
                        t.by_key.get(key).map_or(&[], |w| w.as_slice());
                    let mut matching = writers.iter().filter(|(_, v)| v == value).map(|&(i, _)| i);
                    let (first, second) = (matching.next(), matching.next());
                    // A durable-class value (every possible writer durable, none kept only in `HEAD`) from a group
                    // beyond the reader's `durable_lsn`: a read of an uncovered durable group (I-G2).
                    if let (Some((end, durable)), Some(v)) = (covered, value)
                        && end > durable
                        && first.is_some()
                        && writers
                            .iter()
                            .filter(|(_, w)| w == value)
                            .all(|&(i, _)| t.ops[i].class == Class::Durable && !t.ops[i].head)
                    {
                        t.problems.push(format!(
                            "fresh (I-G2): a reader saw {key:?} = {v:#x} from a group ending at {end}, beyond the \
                             durable_lsn {durable} of the slot it read: no flush had covered it ([F16] P-49, P-57)"
                        ));
                    }
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
                Rec::Refused { who, refusal } => t.refusals.push((who.clone(), refusal.clone())),
                Rec::Bytes {
                    op,
                    path,
                    offset,
                    bytes,
                } => match index.get(op) {
                    Some(&i) => t.bytes.push(GroupBytes {
                        op: i,
                        path: path.clone(),
                        offset: *offset,
                        bytes: bytes.clone(),
                    }),
                    None => t.problems.push(format!(
                        "ledger: group bytes of operation {op}, which was never begun"
                    )),
                },
                Rec::Names {
                    path,
                    node,
                    key,
                    rules,
                } => {
                    if node.is_none() {
                        t.problems.push(format!(
                            "ledger: a namespace expectation names {}, where no file is",
                            path.display()
                        ));
                    }
                    t.names.push(NameRule {
                        path: path.clone(),
                        node: *node,
                        key: *key,
                        rules: rules.clone(),
                    });
                }
                Rec::SetupFault(path) => t.setup_faults.push(path.clone()),
                Rec::SetupIgnored(path) => t.setup_ignored.push(path.clone()),
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
