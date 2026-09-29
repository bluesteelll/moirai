//! The harness's own checks (WP-40, E4): every verdict the toy-log subject adds to the crash enumerator's own ones — the
//! trace predicates of [F13 §1.4] over the `Vfs` events and the toy's protocol notes, the read checks of I-G2 ([F16 §8]),
//! the availability checks of an operation's exit and a reader's refusal, the chain check of acknowledged groups (I-G3)
//! and the namespace check of closed intents ([F16 §17.2] "ns"). They read only what a command returns, what the store
//! and the project directories hold, and the events the simulator records; `doctor --verify` itself is the toy's
//! ([`moirai_toylog::verify`], `Toy::head_fold_problem`), called from the scenarios and the recovery.
//!
//! **Authorship (PLAN §3.1 S4).** S4 keeps the seeded-bug author (R-TOY) from being the enumerator's author (R-HARN-S),
//! so that the harness cannot be tuned to its own bugs. These checks judge the seeded bugs, and R-TOY wrote them; neither
//! the plan nor `docs/m0/authors.md` records a rule for them yet (WP-40 review, finding 2). Their S4 disposition is open:
//! either R-HARN-S authors or adopts them (the generic ones — lock holdings from `Granted` and `Released`, flushes and
//! namespace operations under the writer byte, I-G6 over decoded `HEAD` slot writes where this file reads the toy's own
//! publish notes, the namespace check — for example in `moirai-vfs-sim`, which [F16 §17.2] names for "ns"), or a plan
//! issue records R-HARN-S's review of them as a WP-40 acceptance step. WP-40 is not accepted before that disposition.
//! Such a review covers this file, `moirai_toylog::verify`, `Toy::head_fold_problem`, `moirai_toylog::TOY_DETECTION`,
//! and the two judgements `mod.rs` makes besides running the scenarios: the effects a state shows to the ledger
//! (`effects`, `ns_location`) and the refusals the operator answers with `repair` in the recovery (the fault-model cases
//! of `head_may_be_damaged` and the fixtures' fatal slot). Every predicate the harness evaluates itself is in this file.
//! Until the disposition, a change to it after a missed bug cites a fault-model item, as S4 asks of the enumerator.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use moirai_toylog::head::Slot;
use moirai_toylog::{Note, State, Tap, ToyError, file_name};
use moirai_vfs::{Access, OpenHint, ProcHost, RelPath, RootAccess, RootRole, StoreFs};
use moirai_vfs_sim::{Event, EventKind, SimVfs, SimWorld};

use super::{BAD, PROJ, STORE, TRASH, at_dir, content};

// ---------------------------------------------------------------------------------------------------------------------
// The protocol notes as trace notes ([F13 §1.4])

const TAG: u64 = 0x544F_594C_0000_0000;
const T_PHASE1: u64 = TAG | 1;
const T_PHASE3: u64 = TAG | 2;
const T_REWRITE: u64 = TAG | 3;
const T_LOGFLUSH: u64 = TAG | 4;
const T_HEADFLUSH: u64 = TAG | 5;
const T_CKPT: u64 = TAG | 6;
/// The harness's note at the start of every task: `b` the pid, `c` the task's index in its process.
pub const T_TASK: u64 = TAG | 7;
const T_PUB_SEQ: u64 = TAG | 0x10;
const T_PUB_DUR: u64 = TAG | 0x11;
const T_PUB_CK: u64 = TAG | 0x12;
const T_PUB_BOOT: u64 = TAG | 0x13;
const T_PUB_FLAGS: u64 = TAG | 0x14;

/// The tap that turns the toy's notes into trace notes: `b` carries the process's pid where the predicate needs it.
#[derive(Clone)]
pub struct SimTap {
    world: SimWorld,
    pid: u64,
}

impl SimTap {
    pub fn new(world: &SimWorld, vfs: &SimVfs) -> SimTap {
        SimTap {
            world: world.clone(),
            pid: u64::from(vfs.self_id().pid),
        }
    }
}

impl Tap for SimTap {
    fn note(&self, n: Note) {
        let w = &self.world;
        let pid = self.pid;
        let step = |s: moirai_toylog::Step| u64::from(s.begin) | s.client << 1;
        match n {
            Note::Phase1(s) => w.note(T_PHASE1, pid, step(s)),
            Note::Phase3(s) => w.note(T_PHASE3, pid, step(s)),
            Note::Rewrite(s) => w.note(T_REWRITE, pid, step(s)),
            Note::LogFlush(s) => w.note(T_LOGFLUSH, pid, step(s)),
            Note::HeadFlush(s) => w.note(T_HEADFLUSH, pid, step(s)),
            Note::CheckpointAppended(s) => w.note(T_CKPT, pid, step(s)),
            Note::Publish(p) => {
                w.note(T_PUB_SEQ, p.base_seq, p.new_seq);
                w.note(T_PUB_DUR, p.base_durable, p.new_durable);
                w.note(T_PUB_CK, p.base_checkpoint, p.new_checkpoint);
                w.note(T_PUB_BOOT, p.base_boot, p.new_boot);
                let flags = u64::from(p.flushed)
                    | u64::from(p.boot_change) << 1
                    | u64::from(p.known_boot) << 2;
                w.note(T_PUB_FLAGS, flags, pid);
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// The faults a run saw

/// Whether a flush of `HEAD` failed in the run, in a live process or at a death ([F15] FM-3.1, OP-1).
pub fn head_flush_failed(w: &SimWorld) -> bool {
    if w.failed_flushes() == 0 {
        return false;
    }
    let Some(head) = w.node_at(&Path::new(STORE).join("HEAD")) else {
        return true;
    };
    w.trace().iter().any(|e| match e.kind {
        EventKind::FlushEnd => e.a == head && e.c == 1,
        EventKind::InFlight => e.a == head && e.b == 1 && e.c == 1,
        _ => false,
    })
}

/// Whether the adversary injected a fault into the run: a failed flush, a write, flush, `sync_dir`, creation, namespace
/// or read fault, a sharing violation, or an external actor's act ([F15] FM-3, FM-5, FM-8.2, FM-10, FM-12).
pub fn fault_injected(w: &SimWorld) -> bool {
    use moirai_vfs_sim::Site;
    if w.failed_flushes() > 0 {
        return true;
    }
    let sites = [
        Site::WriteFault,
        Site::FlushFault,
        Site::SyncDirFault,
        Site::CreateFault,
        Site::NsFault,
        Site::Sharing,
        Site::ReadFault,
        Site::MapFault,
    ]
    .map(|s| s as u64);
    w.trace().iter().any(|e| match e.kind {
        EventKind::Choice | EventKind::Injected => sites.contains(&e.a) && e.c != 0,
        EventKind::External => true,
        _ => false,
    })
}

/// Whether some read of the run failed (an injected read error, [F15] FM-12).
pub fn read_failed(w: &SimWorld) -> bool {
    let read = moirai_vfs_sim::CallKind::Read as u64;
    w.trace()
        .iter()
        .any(|e| e.kind == EventKind::Return && e.a == read && e.c != 0)
}

/// Whether a `HEAD` slot may hold no valid state by the fault model alone, so that a store with no valid slot is a
/// correct outcome ([F16] P-61: exit 7 for `moirai repair`): a failed flush in any process ([F15] FM-3.2, OP-1: both slots
/// may fail validation), or a write to `HEAD` that failed or that a death interrupted (FM-5.2, §2.5: the slot it wrote
/// holds any mix of old and new bytes) — after which a crash may tear the other slot (FM-1.2). The trace is read when the
/// run keeps it; without it only the failed flushes count.
pub fn head_may_be_damaged(w: &SimWorld) -> bool {
    if w.failed_flushes() > 0 {
        return true;
    }
    let Some(head) = w.node_at(&Path::new(STORE).join("HEAD")) else {
        return false;
    };
    let write = moirai_vfs_sim::CallKind::Write as u64;
    w.trace().iter().any(|e| match e.kind {
        EventKind::Return => e.a == write && e.b == head && e.c != 0,
        EventKind::InFlight => e.a == head && e.b == 0,
        _ => false,
    })
}

// ---------------------------------------------------------------------------------------------------------------------
// Reads, operation exits and acknowledged groups

/// An acknowledged operation's durable group: (op, start, end, chain_in, chain_out).
pub type AckedGroup = (u64, u64, u64, u64, u64);

/// I-G2 over a read ([F16 §17.2] "fresh": a read of an uncovered durable group): every durable record a view shows lies
/// below the `durable_lsn` of the newest slot — a durable group at or below `committed_lsn` is covered by a flush whose
/// publish raised `durable_lsn` past it (P-40, P-49), and `durable_lsn` never decreases. The check reads the commit
/// positions of the view `st` a read returned and the published slot `s`. After a failed flush of `HEAD` a read may
/// return an older slot (FM-3.2, OP-1): the check is then not made.
pub fn uncovered(w: &SimWorld, st: &State, s: &Slot) -> Vec<String> {
    if head_flush_failed(w) {
        return Vec::new();
    }
    st.commits
        .iter()
        .filter(|(_, c)| c.lsn >= s.durable_lsn)
        .map(|(op, c)| {
            format!(
                "read freshness (I-G2): a view shows commit {op:#x} at lsn {}, at or above durable_lsn {}: a durable \
                 group no flush has covered",
                c.lsn, s.durable_lsn
            )
        })
        .collect()
}

/// What one reader view ([F16 §8]) must show, taken when the view begins: the commits and the lazy rows acknowledged so
/// far, and the flush failures up to then.
pub struct ReadWatch {
    before: BTreeSet<u64>,
    lazy: BTreeMap<u64, u64>,
    failed_flushes: u64,
    head_ok: bool,
}

impl ReadWatch {
    /// The watch of a view that begins now, after the acknowledgement of the commits `before` and the lazy rows `lazy`
    /// (key, value).
    pub fn begin(w: &SimWorld, before: BTreeSet<u64>, lazy: BTreeMap<u64, u64>) -> ReadWatch {
        ReadWatch {
            before,
            lazy,
            failed_flushes: w.failed_flushes(),
            head_ok: !head_flush_failed(w),
        }
    }

    /// The view `st` against the watch; `whole`: the view reached the slot's `committed_lsn`.
    ///
    /// Every commit acknowledged before the view began is in it (I-G2). An acknowledged commit lies below `durable_lsn`
    /// of every slot published after its acknowledgement, in sectors a failed log flush never poisons (FM-3.1 reaches
    /// only sectors dirty during the flush); only a failed `HEAD` flush can make a read return an older slot (FM-3.2,
    /// OP-1), and the check is not made after one.
    ///
    /// A lazy effect is reported as published, which promises visibility ([F16 §1.2] "acknowledge"), until a failed
    /// flush may take it back (FM-3.6). The view ends below `committed_lsn` only where the reader rules end it: a lazy
    /// tail lost after a crash or a failed flush, or a failed read at or above `durable_lsn` ([F16] P-58, P-92) — a view
    /// the lazy rows need not reach.
    pub fn judge(&self, w: &SimWorld, st: &State, whole: bool) -> Vec<String> {
        let mut out = Vec::new();
        if self.head_ok && !head_flush_failed(w) {
            for op in &self.before {
                if !st.commits.contains_key(op) {
                    out.push(format!(
                        "read freshness (I-G2): a view that began after the acknowledgement of commit {op:#x} does not \
                         show it"
                    ));
                }
            }
        }
        if whole && self.failed_flushes == 0 && w.failed_flushes() == 0 {
            for (k, v) in &self.lazy {
                if st.runtime.get(k) != Some(v) {
                    out.push(format!(
                        "visibility: a view that began after the acknowledgement of lazy row {k:#x} does not show it"
                    ));
                }
            }
        }
        out
    }
}

/// A reader's refusal `e` (exit 7): a reader refuses only a store that some fault damaged, so in a run without an
/// injected fault a refusal is a store the protocol must accept ([F16 §17.2] "avail"). `fixture`: the scenario's store
/// carries a setup-injected defect that every process must refuse ([F16] P-61).
pub fn reader_refusal(w: &SimWorld, e: &ToyError, fixture: bool) -> Option<String> {
    (matches!(e, ToyError::Corrupt(_)) && !fault_injected(w) && !fixture).then(|| {
        format!("avail: a reader refused the store in a run without an injected fault: {e}")
    })
}

/// An operation that ended with `e`. An operation that exits 7 `outcome_unknown` ([F16] P-47: its group was lost three
/// times) in a run without a failed flush or a failed read is an availability failure ([F16 §17.2] "avail"): the
/// protocol loses a live writer's group only after a failed flush (its pages reverted, invalidated or evicted) or a
/// failed read at its identity check. The check reads the operation's result (its exit), nothing inside the toy.
pub fn outcome_unknown(w: &SimWorld, e: &ToyError) -> Option<String> {
    (*e == ToyError::OutcomeUnknown && w.failed_flushes() == 0 && !read_failed(w)).then(|| {
        "avail (P-47): an operation exited 7 outcome_unknown in a run without a failed flush or read".to_owned()
    })
}

/// I-G3 over the acknowledged groups `acked` ([F13 §1.4]): every one is still in the log behind the predecessor it was
/// validated against — its trailer at its end, and the 8 bytes before its start (its `chain_in`) unless it starts an
/// extent. Only groups whose extents are still live log files are judged (a retired extent's history is in its `hist`
/// file). `e` is the extent size E; the bytes are read through `v`, outside the toy.
pub fn chain_breaks(v: &SimVfs, e: u64, acked: &[AckedGroup]) -> Vec<String> {
    let Ok(root) = v.open_root(Path::new(STORE), RootRole::Other, RootAccess::Read) else {
        return Vec::new();
    };
    let read8 = |lsn: u64| -> Option<u64> {
        let n = lsn / e + 1;
        let f = v
            .open(
                &root,
                RelPath::new(&format!("log.{n}")).ok()?,
                Access::Read,
                OpenHint::Normal,
            )
            .ok()?;
        let mut b = [0u8; 8];
        v.read_exact_at(&f, lsn % e, &mut b).ok()?;
        Some(u64::from_le_bytes(b))
    };
    let mut out = Vec::new();
    for &(op, start, end, chain_in, chain_out) in acked {
        let (Some(tr), Some(pre)) = (
            read8(end - 8),
            (start >= 8).then(|| read8(start - 8)).flatten(),
        ) else {
            continue;
        };
        if tr != chain_out || (start % e != 0 && pre != chain_in) {
            out.push(format!(
                "I-G3: the acknowledged group of op {op:#x} at [{start}, {end}) is no longer in the log behind the \
                 predecessor it was validated against"
            ));
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------------------------------
// The namespace check ([F16 §17.2] "ns")

/// Where project file `f` is now, as seen through `v`: `at_dir(i)` with its content in directory i alone, `TRASH`, `BAD`
/// (two places or other content), or none. A file whose name exists but whose bytes cannot be read (an injected read
/// error, [F15] FM-12) counts as present with its content: the check judges names, and content only where it is readable.
pub fn ns_location(v: &SimVfs, f: u64) -> Option<u64> {
    let name = file_name(f);
    let want = content(f);
    let mut found: Vec<u64> = Vec::new();
    let mut bad = false;
    let mut judge = |at: u64, got: Found| match got {
        Found::Absent => {}
        Found::Unreadable => found.push(at),
        Found::Bytes(b) if b == want => found.push(at),
        Found::Bytes(_) => bad = true,
    };
    for (i, d) in PROJ.iter().enumerate() {
        let Ok(root) = v.open_root(Path::new(d), RootRole::Other, RootAccess::Read) else {
            continue;
        };
        judge(at_dir(i as u8 + 1), read_all(v, &root, &name));
    }
    if let Ok(root) = v.open_root(Path::new(STORE), RootRole::Other, RootAccess::Read)
        && let Ok(entries) = v.list_dir(&root, Some(RelPath::literal("trash")))
    {
        for e in entries {
            let Some(dir) = e.name.as_segment() else {
                continue;
            };
            // A trash entry is this file's only when its bytes say so (the toy never reads a trash entry, so no
            // injected read error can make one unreadable).
            if let Found::Bytes(b) = read_all(v, &root, &format!("trash/{dir}/0"))
                && b == want
            {
                judge(TRASH, Found::Bytes(b));
            }
        }
    }
    if bad || found.len() > 1 {
        return Some(BAD);
    }
    found.first().copied()
}

/// The namespace check of closed intents ([F16 §17.2] "ns"; [40 §3.4]'s recovery table): a file whose intent is done is
/// where the intent moved it (its destination, the trash, or gone), and a file whose intent was aborted is back at its
/// source (an abort rolls the item back or finds it unmoved, P-71).
pub fn intent_namespace(v: &SimVfs, st: &State) -> Vec<String> {
    use moirai_toylog::format::{INTENT_MV, INTENT_RM, INTENT_RM_TRASH};
    use moirai_toylog::state::IntentState;
    let mut out = Vec::new();
    for row in st.intents.values() {
        let Some(item) = row.rec.items.first() else {
            continue;
        };
        let want = match (row.state, row.rec.op) {
            (IntentState::Open, _) => continue,
            (IntentState::Aborted { .. }, _) => Some(at_dir(item.src_dir)),
            (IntentState::Done { .. }, INTENT_MV) => Some(at_dir(item.dst_dir)),
            (IntentState::Done { .. }, INTENT_RM) => None,
            (IntentState::Done { .. }, INTENT_RM_TRASH) => Some(TRASH),
            (IntentState::Done { .. }, _) => continue,
        };
        let got = ns_location(v, item.file);
        if got != want {
            out.push(format!(
                "ns: the intent of key {:#x} is {:?} but its file {} is at {got:?}, not {want:?}",
                row.rec.key, row.state, item.file
            ));
        }
    }
    out
}

/// What a read of a project file found.
pub enum Found {
    Absent,
    Unreadable,
    Bytes(Vec<u8>),
}

/// Reads the whole file `name` under `root` through `v`.
pub fn read_all(v: &SimVfs, root: &moirai_vfs_sim::SimRoot, name: &str) -> Found {
    let Ok(rel) = RelPath::new(name) else {
        return Found::Absent;
    };
    let f = match v.open(root, rel, Access::Read, OpenHint::Normal) {
        Ok(f) => f,
        Err(e)
            if matches!(
                e.kind,
                moirai_vfs::VfsErrorKind::NotFound | moirai_vfs::VfsErrorKind::DeletePending
            ) =>
        {
            return Found::Absent;
        }
        Err(_) => return Found::Unreadable,
    };
    let Ok(n) = v.file_size(&f) else {
        return Found::Unreadable;
    };
    let mut b = vec![0u8; n as usize];
    match v.read_exact_at(&f, 0, &mut b) {
        Ok(()) => Found::Bytes(b),
        Err(_) => Found::Unreadable,
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// The trace predicates ([F13 §1.4]: I-G4 and I-G6; [F16] P-2, P-25, P-51, L-8)

/// The violations of the trace predicates in `events`.
///
/// Lock holdings are tracked per (process, client) from the `Granted` and `Released` events; the toy's step notes carry
/// the client of the handle that performs the step, so a step of one client is never blamed on another client of the
/// same process (the in-process clients of [F16] P-3). Raw `Vfs` events (a flush, a lock wait, a namespace change) name
/// only the process: they are judged for processes that have used a single client.
pub fn trace_violations(events: &[Event]) -> Vec<String> {
    let writer = moirai_vfs::LockByte::Writer.offset();
    let maintenance = moirai_vfs::LockByte::Maintenance.offset();
    let quiet: BTreeSet<u64> = (0..moirai_vfs::N_QUIET)
        .filter_map(moirai_vfs::QuietIndex::new)
        .map(|q| moirai_vfs::LockByte::Quiet(q).offset())
        .collect();
    let mut out = Vec::new();
    let mut pid_proc: BTreeMap<u64, u32> = BTreeMap::new();
    // (process, client) that hold the writer byte; the clients each process has used; quiet bytes held.
    let mut writer_held: BTreeSet<(u32, u64)> = BTreeSet::new();
    let mut clients: BTreeMap<u32, BTreeSet<u64>> = BTreeMap::new();
    // The tasks each process has started (a task is a client of its own, known before its first grant).
    let mut tasks: BTreeMap<u32, u64> = BTreeMap::new();
    let mut quiet_held: BTreeMap<(u32, u64), u64> = BTreeMap::new();
    let mut maint_held: BTreeSet<(u32, u64)> = BTreeSet::new();
    let mut log_flushing: BTreeSet<(u32, u64)> = BTreeSet::new();
    // I-G6 over the publishes: the newest slot_seq written since the last disturbance.
    let mut last_seq: Option<u64> = None;
    let mut last_ck: Option<u64> = None;
    let mut clean = true;
    // A failed flush poisons its sectors for good ([F15] FM-3.3, OP-15: poisoning survives later flushes and system
    // crashes, and only a re-write ends it): from then on a read of `HEAD` may return any candidate, so the publishes
    // after it are not judged against each other.
    let mut poisoned = false;
    let mut pending_pub: [u64; 8] = [0; 8];
    let single = |clients: &BTreeMap<u32, BTreeSet<u64>>, tasks: &BTreeMap<u32, u64>, p: u32| {
        clients.get(&p).is_none_or(|c| c.len() <= 1) && tasks.get(&p).is_none_or(|&n| n <= 1)
    };
    let proc_holds = |held: &BTreeSet<(u32, u64)>, p: u32| held.iter().any(|&(q, _)| q == p);
    for e in events {
        match e.kind {
            EventKind::ProcStart => {
                pid_proc.insert(e.a, e.proc);
            }
            EventKind::ProcEnd => {
                writer_held.retain(|&(p, _)| p != e.proc);
                maint_held.retain(|&(p, _)| p != e.proc);
                quiet_held.retain(|k, _| k.0 != e.proc);
                log_flushing.retain(|&(p, _)| p != e.proc);
                if e.a != 5 {
                    // A death may leave a publish half written: the next publisher reads the other slot.
                    last_seq = None;
                    clean = false;
                }
            }
            EventKind::Crash | EventKind::Boot => {
                writer_held.clear();
                maint_held.clear();
                quiet_held.clear();
                log_flushing.clear();
                last_seq = None;
                last_ck = None;
                clean = !poisoned;
            }
            EventKind::FlushEnd if e.c == 1 => {
                // A failed flush: reads of poisoned sectors may return older bytes (FM-3.2), after a crash too.
                last_seq = None;
                clean = false;
                poisoned = true;
            }
            EventKind::InFlight if e.b == 1 && e.c == 1 => {
                // A death's failed flush outcome poisons like a failed flush (FM-3.1).
                last_seq = None;
                clean = false;
                poisoned = true;
            }
            EventKind::InFlight if e.b == 0 => {
                // A death's partial write (§2.5): a publish in it may or may not have taken effect.
                last_seq = None;
                clean = false;
            }
            EventKind::External => {
                last_seq = None;
                clean = false;
            }
            EventKind::Return if e.c != 0 && e.a == moirai_vfs_sim::CallKind::Write as u64 => {
                // A failed write may have applied any part of its bytes (FM-5.2): a publish in it may or may not have
                // taken effect.
                last_seq = None;
                clean = false;
            }
            EventKind::Granted => {
                clients.entry(e.proc).or_default().insert(e.c);
                if e.b == writer {
                    writer_held.insert((e.proc, e.c));
                } else if e.b == maintenance {
                    maint_held.insert((e.proc, e.c));
                } else if quiet.contains(&e.b) {
                    *quiet_held.entry((e.proc, e.b)).or_insert(0) += 1;
                }
            }
            EventKind::Released => {
                if e.b == writer {
                    writer_held.remove(&(e.proc, e.c));
                } else if e.b == maintenance {
                    maint_held.remove(&(e.proc, e.c));
                } else if quiet.contains(&e.b)
                    && let Some(n) = quiet_held.get_mut(&(e.proc, e.b))
                {
                    *n = n.saturating_sub(1);
                    if *n == 0 {
                        quiet_held.remove(&(e.proc, e.b));
                    }
                }
            }
            EventKind::FlushStart
                if single(&clients, &tasks, e.proc) && proc_holds(&writer_held, e.proc) =>
            {
                out.push(format!(
                    "I-G4 (P-2): process {} flushes (node {}) while it holds the writer byte",
                    e.proc, e.a
                ));
            }
            EventKind::LockWait
                if single(&clients, &tasks, e.proc) && proc_holds(&writer_held, e.proc) =>
            {
                out.push(format!(
                    "I-G4 (P-1, P-2): process {} waits for lock byte {:#x} while it holds the writer byte",
                    e.proc, e.b
                ));
            }
            EventKind::NsOp
                if e.proc != u32::MAX
                    && single(&clients, &tasks, e.proc)
                    && proc_holds(&writer_held, e.proc) =>
            {
                out.push(format!(
                    "P-2: process {} changes the namespace while it holds the writer byte",
                    e.proc
                ));
            }
            EventKind::Note if e.a == T_TASK => {
                if let Some(&p) = pid_proc.get(&e.b) {
                    *tasks.entry(p).or_insert(0) += 1;
                }
            }
            EventKind::Note => {
                let p = pid_proc.get(&e.b).copied().unwrap_or(u32::MAX);
                let (begin, client) = (e.c & 1 == 1, e.c >> 1);
                let holds = writer_held.contains(&(p, client));
                match e.a {
                    T_PHASE1 if begin && holds => {
                        out.push(format!(
                            "P-25: process {p} runs phase 1 while it holds the writer byte"
                        ));
                    }
                    T_PHASE3 if begin && holds => {
                        out.push(format!(
                            "P-51: process {p} runs phase 3 (maintenance) while it holds the writer byte"
                        ));
                    }
                    T_REWRITE if begin && !holds => {
                        out.push(format!(
                            "I-G4 (P-43): process {p} scans and re-writes the pending range outside the writer byte"
                        ));
                    }
                    T_LOGFLUSH | T_HEADFLUSH if begin && holds => {
                        out.push(format!(
                            "I-G4 (P-2): process {p} flushes while it holds the writer byte"
                        ));
                    }
                    _ => {}
                }
                match e.a {
                    T_LOGFLUSH if begin => {
                        if !log_flushing.is_empty() && !log_flushing.contains(&(p, client)) {
                            out.push(format!(
                                "I-G4: process {p} starts a log flush while another is in flight"
                            ));
                        }
                        log_flushing.insert((p, client));
                    }
                    T_LOGFLUSH => {
                        log_flushing.remove(&(p, client));
                    }
                    T_CKPT => {
                        if !maint_held.contains(&(p, client)) {
                            out.push(format!(
                                "P-76: process {p} appends a Checkpoint without holding the maintenance byte"
                            ));
                        }
                        if !quiet_held.is_empty() {
                            out.push(format!(
                                "L-8 ([F03 §3.1] rule 2): process {p} appends a Checkpoint while a quiet byte is held \
                                 (quiet mode)"
                            ));
                        }
                    }
                    T_PUB_SEQ => {
                        pending_pub[0] = e.b;
                        pending_pub[1] = e.c;
                    }
                    T_PUB_DUR => {
                        pending_pub[2] = e.b;
                        pending_pub[3] = e.c;
                    }
                    T_PUB_CK => {
                        pending_pub[4] = e.b;
                        pending_pub[5] = e.c;
                    }
                    T_PUB_BOOT => {
                        pending_pub[6] = e.b;
                        pending_pub[7] = e.c;
                    }
                    T_PUB_FLAGS => {
                        let [bseq, nseq, bdur, ndur, bck, nck, bboot, nboot] = pending_pub;
                        let flushed = e.b & 1 != 0;
                        let boot_change = e.b & 2 != 0;
                        let known = e.b & 4 != 0;
                        if nseq != bseq + 1 {
                            out.push(format!(
                                "I-G6: a publish writes slot_seq {nseq} over {bseq}"
                            ));
                        }
                        if ndur < bdur {
                            out.push(format!(
                                "I-G6 (P-45): a publish lowers durable_lsn from {bdur} to {ndur}"
                            ));
                        }
                        if !flushed && ndur != bdur {
                            out.push(format!(
                                "I-G6 (P-7): a publish without a flush moves durable_lsn from {bdur} to {ndur}"
                            ));
                        }
                        if nck < bck {
                            out.push(format!(
                                "I-G6: a publish moves checkpoint_lsn back from {bck} to {nck}"
                            ));
                        }
                        if clean
                            && let Some(c) = last_ck
                            && nck < c
                        {
                            out.push(format!(
                                "I-G6 (P-76): the published checkpoint_lsn goes back from {c} to {nck}"
                            ));
                        }
                        if nboot != bboot && !(boot_change && known) {
                            out.push(format!(
                                "I-G6 (P-67): a publish that is not boot-change recovery changes boot_id \
                                 ({bboot:#x} to {nboot:#x})"
                            ));
                        }
                        if clean
                            && let Some(l) = last_seq
                            && bseq != l
                        {
                            out.push(format!(
                                "I-G6 (P-48): a publish read-modify-writes slot_seq {bseq}, not the newest slot ({l})"
                            ));
                        }
                        last_seq = Some(nseq.max(last_seq.unwrap_or(0)));
                        last_ck = Some(nck.max(last_ck.unwrap_or(0)));
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    out
}
