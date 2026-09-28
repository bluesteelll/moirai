//! The simulated world: one kernel (namespace, file contents, lock bytes, clocks, boots), its simulated processes, the
//! adversary, the trace, and the deterministic scheduler that runs simulated threads ("tasks") one at a time.
//!
//! **Scheduling.** A task is a real OS thread running harness code against a [`crate::SimVfs`]. Exactly one task runs at
//! a time: it holds the baton until its next scheduling point — the start of every `Vfs` call and the middle of every
//! call that has an interval (reads, writes, flushes, `sync_dir`) — where the adversary picks the next runnable task
//! ([`Site::Schedule`]), may pause the current one (FM-6), may suspend the system (FM-6.3) or crash it. Blocking waits
//! (lock waits, retry sleeps, pauses, the parent watch) block the task on the virtual clock; when no task is runnable
//! the clock jumps to the next timer. Every step is under one mutex, so a seed and a call sequence replay
//! byte-identically ([F15 §6.4]). A thread that is not a task (the test's own thread, "the driver") may use the
//! simulator directly while no task runs: its calls execute at once and its waits advance the virtual clock.
//!
//! **Processes** ([F15 §2.1]). A simulated process owns its tasks, handles, mappings, lock-grant tables (one per `LOCK`,
//! [OS/lock §5.1]), clocks' wall offset and counters, and dies as a unit (§2.5): its tasks unwind at their next
//! scheduling point with [`SimUnwind::Died`], its in-flight write applies partially, its in-flight flush resolves to one
//! of three outcomes, and its lock bytes stay held for a delay drawn from FM-8.1's classes.

use std::cell::Cell;
use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;

use moirai_vfs::{
    BootId, EnvWarning, ExtentMethod, FsKind, GrantTable, LockByte, OsCode, OsTag, OsVersion,
    Refusal, VfsCounters, VfsErrorKind, WaitMode,
};

use crate::adversary::{Adversary, Choice, FaultRates, PartialWrite, ReleaseDelayLaw, Site};
use crate::content::{FlushMark, ZEROS};
use crate::locks::KernelLocks;
use crate::namespace::{Kind, Ns, NsOp, ROOT, ReadError, abs_components};
use crate::rng::{Rng, splitmix};
use crate::trace::{Event, EventKind, Trace, TraceMode};

// ---------------------------------------------------------------------------------------------------------------------
// Configuration

/// A simulated volume: its file system as the environment guard reports it ([OS/env §2]) and its size.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VolumeProfile {
    /// The file system `classify` reports.
    pub fs: FsKind,
    /// How extents get their zeros.
    pub extent_method: ExtentMethod,
    /// A read-only volume.
    pub read_only: bool,
    /// A removable drive.
    pub removable: bool,
    /// `Some` makes `classify` refuse the location.
    pub refusal: Option<Refusal>,
    /// Total bytes (`free_space`); the free bytes are this minus the sizes of the volume's files.
    pub total_bytes: u64,
    /// `doctor` warnings.
    pub warnings: Vec<EnvWarning>,
}

impl Default for VolumeProfile {
    /// NTFS with zero-filled extents, 1 TiB: the only file system gated in M0–M11 ([F15 §6.3] A-2).
    fn default() -> VolumeProfile {
        VolumeProfile {
            fs: FsKind::Ntfs,
            extent_method: ExtentMethod::ZeroFill,
            read_only: false,
            removable: false,
            refusal: None,
            total_bytes: 1 << 40,
            warnings: Vec::new(),
        }
    }
}

/// The configuration of a simulated world. Everything that varies between runs derives from `seed`.
#[derive(Clone, Debug)]
pub struct SimConfig {
    /// The seed of every adversary choice ([F15 §6.4]).
    pub seed: u64,
    /// Fault rates of the default adversary.
    pub rates: FaultRates,
    /// Lock-release delays (FM-8.1): classes (a), (b) and (c).
    pub release_law: ReleaseDelayLaw,
    /// The OS the simulated processes report ([OS/proc §2]); it selects OS error codes and call names.
    pub os: OsTag,
    /// The OS version `check_os_version` reports.
    pub os_version: OsVersion,
    /// The grant tables' wait mode; `None` draws it from the seed ([OS/lock §5.1]).
    pub wait_mode: Option<WaitMode>,
    /// Virtual time that passes at every scheduling point, in ns.
    pub tick_ns: u64,
    /// Whether the trace keeps its events.
    pub trace: TraceMode,
    /// The first boot's identity; `None` derives it from the seed.
    pub first_boot: Option<BootId>,
    /// The wall clock at the first boot, in ms since the Unix epoch.
    pub wall_origin_ms: i64,
    /// The volume of the world root.
    pub root_volume: VolumeProfile,
    /// Further volumes, each mounted at an absolute directory created at world start.
    pub volumes: Vec<(PathBuf, VolumeProfile)>,
}

impl SimConfig {
    /// A fault-free Windows world with `seed`: faults come only from queued choices and crash plans. The adversary still
    /// decides every freedom that is not a fault: the schedule, crash resolutions, the per-OS alternatives, and the
    /// release-delay class of a dead process's bytes (the default [`ReleaseDelayLaw`], tail classes included).
    pub fn new(seed: u64) -> SimConfig {
        SimConfig {
            seed,
            rates: FaultRates::default(),
            release_law: ReleaseDelayLaw::default(),
            os: OsTag::Windows,
            os_version: OsVersion {
                major: 10,
                minor: 0,
                build: 26_100,
            },
            wait_mode: None,
            tick_ns: 1_000,
            trace: TraceMode::Full,
            first_boot: None,
            wall_origin_ms: 1_790_000_000_000,
            root_volume: VolumeProfile::default(),
            volumes: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Public enums

/// Why a simulated process ended.
#[repr(u8)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum DeathCause {
    /// Killed by the harness ([`crate::SimWorld::kill`]; `TerminateProcess`, a kill, an abort).
    Killed = 1,
    /// `fail_stop` after a durability failure ([OS/fs §4.4.5]).
    FailStop = 2,
    /// A mapped read faulted (FM-9.1): exit 7 from the fault handler.
    MapFault = 3,
    /// A system crash.
    SystemCrash = 4,
    /// A normal exit ([`crate::SimWorld::exit`]).
    Exit = 5,
}

/// How a process death resolves the process's in-flight calls and lock bytes; `None` fields are left to the adversary.
#[derive(Clone, Debug, Default)]
pub struct DeathPlan {
    /// How much of an in-flight write applied (§2.5).
    pub write: Option<PartialWrite>,
    /// The outcome of an in-flight flush: 0 succeeded, 1 failed, 2 not performed (FM-11.2). For a `sync_dir` (or a
    /// directory member of `sync_group`) 0 makes its operations durable by FM-2.3 and 1 and 2 leave them pending.
    pub flush: Option<u8>,
    /// The release-delay class of every byte it holds: 0 measured, 1 tail, 2 never (FM-8.1).
    pub release_class: Option<u8>,
    /// A fixed release delay in ns for classes 0 and 1, instead of a drawn one.
    pub release_delay_ns: Option<u64>,
}

/// The kinds of calls, as the trace names them.
#[repr(u8)]
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
#[allow(missing_docs)]
pub enum CallKind {
    OpenRoot = 1,
    CreateRoot,
    Open,
    CreateNew,
    CreateDir,
    RemoveDir,
    ListDir,
    Read,
    Write,
    Sync,
    SyncDir,
    SyncGroup,
    FailStop,
    CreateExtent,
    RecycleExtent,
    Seal,
    Unlink,
    RenameNoreplace,
    RenameReplace,
    SwapDirs,
    SwapRecover,
    FileSize,
    Identity,
    PathIdentity,
    FreeSpace,
    Advise,
    SetLen,
    LockClient,
    TryAcquire,
    AcquireWithin,
    Release,
    Probe,
    ForeignCheck,
    MapSealed,
    Classify,
    ProbeStore,
    BootIdentity,
    WaitParent,
    SpawnGc,
    Close,
}

/// A protocol violation the simulator detects and reports as a harness failure ([F15 §3.13], OP-20; [OS/fs §4.12, §6.3]).
#[repr(u8)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ViolationKind {
    /// A write, flush, create or namespace call by a process after a non-lazy class returned an error to it: the product
    /// must `fail_stop`, never retry ([80 §2.3.1]).
    CallAfterDurabilityFailure = 1,
    /// A bounded sharing-violation retry by a process that holds the writer or flush byte ([OS/fs §6.3]).
    RetryUnderWriterOrFlush = 2,
    /// `advise_dontneed` on a file that holds unflushed or poisoned sectors ([OS/fs §4.12]).
    AdviseOnUnflushed = 3,
    /// A write to a sealed file ([80 §2.5] rules 1–3).
    WriteToSealed = 4,
}

/// One detected violation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Violation {
    /// What was violated.
    pub kind: ViolationKind,
    /// The simulated process.
    pub proc: u32,
    /// The node concerned, or 0.
    pub node: u64,
    /// The scheduling point at which it happened.
    pub point: u64,
}

/// How a simulated thread's stack is unwound by the simulator (never through the panic hook: the payload is raised with
/// `std::panic::resume_unwind`).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum SimUnwind {
    /// The thread's process died (a kill, `fail_stop`, a mapping fault, a system crash).
    Died,
    /// Every task was blocked with no timer: the run was aborted.
    Deadlock,
}

/// Runs `f`, turning the simulator's unwinding into `Err`. For driver-thread code that may die (`fail_stop`, a mapping
/// fault, a queued crash).
pub fn catch_death<R>(f: impl FnOnce() -> R) -> Result<R, SimUnwind> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(r) => Ok(r),
        Err(payload) => match payload.downcast::<SimUnwind>() {
            Ok(u) => Err(*u),
            Err(other) => std::panic::resume_unwind(other),
        },
    }
}

pub(crate) fn unwind(u: SimUnwind) -> ! {
    std::panic::resume_unwind(Box::new(u))
}

/// A choice queued for the next matching adversary site ([`crate::SimWorld::queue_choice`]): the site, and optionally
/// the process and the node ([`Choice::node`]) it must concern.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct Queued {
    pub(crate) site: Site,
    pub(crate) proc: Option<u32>,
    pub(crate) node: Option<u64>,
    pub(crate) value: u64,
}

/// A `spawn_gc_child` request a simulated process made ([OS/proc §10.1]): the harness runs the child's body as tasks of
/// `child`.
#[derive(Clone, Debug)]
pub struct SpawnRequest {
    /// The new simulated process.
    pub child: crate::SimVfs,
    /// The executable path passed.
    pub exe: PathBuf,
    /// The arguments.
    pub args: Vec<String>,
    /// The working directory.
    pub cwd: PathBuf,
}

/// The 1-based code of an error kind in the trace's `Return` events (0 is success).
pub fn error_code(kind: VfsErrorKind) -> u64 {
    1 + match kind {
        VfsErrorKind::NotFound => 0,
        VfsErrorKind::AlreadyExists => 1,
        VfsErrorKind::NotEmpty => 2,
        VfsErrorKind::AccessDenied => 3,
        VfsErrorKind::SharingViolation => 4,
        VfsErrorKind::DeletePending => 5,
        VfsErrorKind::DiskFull => 6,
        VfsErrorKind::InsufficientSpace => 7,
        VfsErrorKind::ReadOnlyVolume => 8,
        VfsErrorKind::Unsupported => 9,
        VfsErrorKind::CrossDevice => 10,
        VfsErrorKind::Busy => 11,
        VfsErrorKind::InvalidName => 12,
        VfsErrorKind::UnexpectedEof => 13,
        VfsErrorKind::Io => 14,
        VfsErrorKind::Other => 15,
        VfsErrorKind::CloudOnly => 16,
        VfsErrorKind::IsSymlink => 17,
        VfsErrorKind::IsDirectory => 18,
        VfsErrorKind::OutsideRoot => 19,
        VfsErrorKind::Stale => 20,
        _ => 21,
    }
}

/// The OS code the simulated OS reports for an error kind ([OS/fs §6.2], read backwards).
pub(crate) fn os_code(os: OsTag, kind: VfsErrorKind) -> OsCode {
    let (win, linux, mac) = match kind {
        VfsErrorKind::NotFound => (2, 2, 2),
        VfsErrorKind::AlreadyExists => (80, 17, 17),
        VfsErrorKind::NotEmpty => (145, 39, 66),
        VfsErrorKind::AccessDenied => (5, 13, 13),
        VfsErrorKind::SharingViolation => (32, 0, 0),
        VfsErrorKind::DeletePending => (303, 0, 0),
        VfsErrorKind::DiskFull => (112, 28, 28),
        VfsErrorKind::ReadOnlyVolume => (19, 30, 30),
        VfsErrorKind::Unsupported => (50, 95, 45),
        VfsErrorKind::CrossDevice => (17, 18, 18),
        VfsErrorKind::Busy => (170, 16, 16),
        VfsErrorKind::InvalidName => (123, 36, 63),
        VfsErrorKind::Io => (1117, 5, 5),
        VfsErrorKind::IsDirectory => (267, 21, 21),
        _ => (0, 0, 0),
    };
    OsCode(match os {
        OsTag::Windows => win,
        OsTag::Linux => linux,
        OsTag::MacOs => mac,
        OsTag::Unspecified => 0,
    })
}

// ---------------------------------------------------------------------------------------------------------------------
// Kernel state (cloned into crash images)

/// The three clocks and the boot ([F15 §3.7]).
#[derive(Clone, Debug)]
pub(crate) struct Clocks {
    pub(crate) boot_seq: u32,
    pub(crate) boot_id: BootId,
    pub(crate) boot_ns: u64,
    pub(crate) mono_ns: u64,
    pub(crate) boot_origin_ns: u64,
    pub(crate) wall_at_boot_ms: i64,
}

impl Clocks {
    pub(crate) fn wall_ms(&self, offset: i64) -> i64 {
        let since = ((self.boot_ns - self.boot_origin_ns) / 1_000_000) as i64;
        self.wall_at_boot_ms
            .saturating_add(since)
            .saturating_add(offset)
    }
}

/// A simulated process.
#[derive(Clone, Debug)]
pub(crate) struct ProcRec {
    pub(crate) name: String,
    pub(crate) pid: u32,
    pub(crate) start_ns: u64,
    pub(crate) boot_seq: u32,
    pub(crate) alive: bool,
    pub(crate) parent: Option<u32>,
    pub(crate) boot_known: bool,
    pub(crate) wall_offset_ms: i64,
    pub(crate) counters: VfsCounters,
    pub(crate) maps: usize,
    pub(crate) failed_nonlazy: bool,
    pub(crate) death: Option<DeathCause>,
    /// The process's `fill_random` stream ([OS/README §4.6] "Simulator form"): derived from the seed and the process
    /// index, never shared.
    pub(crate) rand: Rng,
    /// Bytes a test scripted for the next draws ([`crate::SimWorld::script_random`]).
    pub(crate) scripted: VecDeque<u8>,
}

/// An open handle or mapping.
#[derive(Clone, Debug)]
pub(crate) struct HandleRec {
    pub(crate) node: u64,
    pub(crate) proc: u32,
    pub(crate) map: bool,
}

/// The bytes of a write in flight: a copy, or (the extent zero-fill, truncation and extension case) zeros, which need
/// none.
#[derive(Clone, Debug)]
pub(crate) enum WriteBytes {
    Zeros(usize),
    Copy(Box<[u8]>),
}

impl WriteBytes {
    /// The record of `buf`: zeros of its length when every byte is zero and it fits the shared zero buffer.
    pub(crate) fn of(buf: &[u8]) -> WriteBytes {
        if buf.len() <= ZEROS.len() && buf.iter().all(|&b| b == 0) {
            WriteBytes::Zeros(buf.len())
        } else {
            WriteBytes::Copy(buf.into())
        }
    }

    pub(crate) fn as_slice(&self) -> &[u8] {
        match self {
            WriteBytes::Zeros(n) => &ZEROS[..*n],
            WriteBytes::Copy(b) => b,
        }
    }
}

/// A write between its start and its return.
#[derive(Clone, Debug)]
pub(crate) struct InFlightWrite {
    pub(crate) id: u64,
    pub(crate) proc: u32,
    pub(crate) node: u64,
    pub(crate) offset: u64,
    pub(crate) data: WriteBytes,
}

/// A read between its start and its return, with the contents overlapping writes may show (FM-4.1).
#[derive(Clone, Debug)]
pub(crate) struct InFlightRead {
    pub(crate) id: u64,
    pub(crate) proc: u32,
    pub(crate) node: u64,
    pub(crate) offset: u64,
    pub(crate) len: u64,
    pub(crate) alts: Vec<(u64, Box<[u8; 512]>)>,
}

/// What a flush in flight is.
#[derive(Clone, Debug)]
pub(crate) enum FlushWhat {
    File { mark: FlushMark, meta: bool },
    Dir { limit: u64 },
}

/// A flush between its start and its return.
#[derive(Clone, Debug)]
pub(crate) struct InFlightFlush {
    pub(crate) id: u64,
    pub(crate) proc: u32,
    pub(crate) node: u64,
    pub(crate) what: FlushWhat,
}

/// Everything a crash image copies.
#[derive(Clone, Debug)]
pub(crate) struct Kernel {
    pub(crate) ns: Ns,
    pub(crate) clock: Clocks,
    pub(crate) locks: KernelLocks,
    pub(crate) procs: Vec<ProcRec>,
    pub(crate) handles: BTreeMap<u64, HandleRec>,
    pub(crate) next_handle: u64,
    pub(crate) writes: Vec<InFlightWrite>,
    pub(crate) reads: Vec<InFlightRead>,
    pub(crate) flushes: Vec<InFlightFlush>,
    pub(crate) next_io: u64,
    pub(crate) points: u64,
    pub(crate) volumes: Vec<VolumeProfile>,
    pub(crate) wakes: BTreeMap<u64, bool>,
    pub(crate) next_wake: u64,
}

// ---------------------------------------------------------------------------------------------------------------------
// Chooser

/// The adversary, its generator, the queued choices and the trace.
pub(crate) struct Chooser {
    pub(crate) adv: Box<dyn Adversary>,
    pub(crate) rng: Rng,
    pub(crate) trace: Trace,
    pub(crate) queue: Vec<Queued>,
}

impl Chooser {
    /// One adversary choice. A choice with arity ≤ 1 is no choice: the adversary is not asked, and nothing is recorded
    /// unless a queued choice matches it, which it then consumes (recorded as `Injected` with value 0), so that a queued
    /// value never drifts to a later, unrelated choice at the same site.
    pub(crate) fn pick(&mut self, site: Site, proc: u32, node: u64, aux: u64, arity: u64) -> u64 {
        let clamp = |v: u64| {
            if arity != u64::MAX && v >= arity {
                v % arity.max(1)
            } else {
                v
            }
        };
        let queued = self.queue.iter().position(|q| {
            q.site == site && q.proc.is_none_or(|p| p == proc) && q.node.is_none_or(|n| n == node)
        });
        if arity <= 1 {
            if let Some(i) = queued {
                self.queue.remove(i);
                self.trace.push(Event {
                    kind: EventKind::Injected,
                    task: u32::MAX,
                    proc,
                    a: u64::from(site.code()),
                    b: arity,
                    c: 0,
                });
            }
            return 0;
        }
        if let Some(i) = queued {
            let v = clamp(self.queue.remove(i).value);
            self.trace.push(Event {
                kind: EventKind::Injected,
                task: u32::MAX,
                proc,
                a: u64::from(site.code()),
                b: arity,
                c: v,
            });
            return v;
        }
        let c = Choice {
            site,
            proc,
            node,
            aux,
            arity,
        };
        let v = clamp(self.adv.choose(&c, &mut self.rng));
        self.trace.push(Event {
            kind: EventKind::Choice,
            task: u32::MAX,
            proc,
            a: u64::from(site.code()),
            b: arity,
            c: v,
        });
        v
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Scheduler

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum TState {
    Runnable,
    Running,
    Blocked,
    Done,
}

/// Why a task is blocked.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum Block {
    None,
    Timer,
    /// In a grant table of (process, `LOCK` node).
    Table(u32, u64),
    /// Driving a kernel wait.
    KernelWait(u64),
    /// In `wait_parent_or_wake` with this wake object.
    Parent(u64),
}

#[derive(Debug)]
pub(crate) struct TaskRec {
    pub(crate) proc: u32,
    pub(crate) state: TState,
    pub(crate) block: Block,
    pub(crate) wake_at: Option<u64>,
    pub(crate) abort: bool,
}

#[derive(Debug, Default)]
pub(crate) struct Sched {
    pub(crate) tasks: Vec<TaskRec>,
    pub(crate) running: Option<u32>,
    pub(crate) active: bool,
    pub(crate) deadlock: bool,
    pub(crate) threads: Vec<JoinHandle<()>>,
}

/// One grant table of one simulated process ([OS/lock §5.1]), with the `LOCK` handles it has opened ([OS/lock §9.2]).
pub(crate) struct TableRec {
    pub(crate) table: GrantTable,
    /// The store directory whose `LOCK` the table's handles open ([OS/lock §9.1] step 4 checks each against it).
    pub(crate) store: u64,
    /// The role handles opened so far, one bit per role byte (bit = the byte's offset minus `ROLE_BASE`).
    pub(crate) roles_open: u8,
    /// Whether the probe handle is open.
    pub(crate) probe_open: bool,
}

/// A crash trigger at a scheduling point.
pub(crate) enum Trigger {
    Capture(u64),
    Crash(u64, crate::crash::CrashPlan),
    Kill(u64, u32, DeathPlan),
}

/// The whole mutable state of a world, under one mutex.
pub(crate) struct State {
    pub(crate) cfg: SimConfig,
    pub(crate) ch: Chooser,
    pub(crate) k: Kernel,
    pub(crate) sched: Sched,
    pub(crate) tables: BTreeMap<(u32, u64), TableRec>,
    pub(crate) next_table: u64,
    pub(crate) wait_mode: WaitMode,
    pub(crate) triggers: Vec<Trigger>,
    pub(crate) captures: Vec<(u64, crate::crash::CrashImage)>,
    pub(crate) violations: Vec<Violation>,
    pub(crate) stderr: Vec<String>,
    pub(crate) spawns: Vec<SpawnRequest>,
    /// The release-delay classes drawn for dead processes' bytes so far: (a), (b), (c) (FM-8.1, [F15 §3.8]).
    pub(crate) release_drawn: [u64; 3],
}

pub(crate) struct Shared {
    pub(crate) st: Mutex<State>,
    pub(crate) cv: Condvar,
}

impl Shared {
    pub(crate) fn lock(&self) -> MutexGuard<'_, State> {
        self.st.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn addr(&self) -> usize {
        self as *const Shared as usize
    }
}

thread_local! {
    /// The world (by address) and task this thread runs as; `(0, u32::MAX)` for a thread that is not a task.
    static CURRENT: Cell<(usize, u32)> = const { Cell::new((0, u32::MAX)) };
}

/// The task the current thread runs as in world `sh`, if any.
pub(crate) fn current_task(sh: &Shared) -> Option<u32> {
    let (w, t) = CURRENT.with(Cell::get);
    (w == sh.addr() && t != u32::MAX).then_some(t)
}

impl State {
    pub(crate) fn ev(
        &mut self,
        kind: EventKind,
        task: Option<u32>,
        proc: u32,
        a: u64,
        b: u64,
        c: u64,
    ) {
        self.ch.trace.push(Event {
            kind,
            task: task.unwrap_or(u32::MAX),
            proc,
            a,
            b,
            c,
        });
    }

    pub(crate) fn pick(&mut self, site: Site, proc: u32, node: u64, aux: u64, arity: u64) -> u64 {
        self.ch.pick(site, proc, node, aux, arity)
    }

    pub(crate) fn violation(&mut self, kind: ViolationKind, proc: u32, node: u64) {
        let point = self.k.points;
        self.violations.push(Violation {
            kind,
            proc,
            node,
            point,
        });
        self.ev(EventKind::Violation, None, proc, kind as u64, node, point);
    }

    pub(crate) fn alive(&self, proc: u32) -> bool {
        self.k.procs.get(proc as usize).is_some_and(|p| p.alive)
    }

    // ---- time ----

    /// Advances both clocks to boot-clock instant `t` (never backward).
    pub(crate) fn advance_to(&mut self, t: u64) {
        let now = self.k.clock.boot_ns;
        if t > now {
            self.k.clock.boot_ns = t;
            self.k.clock.mono_ns += t - now;
        }
    }

    /// The earliest pending timer: a blocked task's wake-up or a dead process's byte release.
    pub(crate) fn next_timer(&self) -> Option<u64> {
        let tasks = self
            .sched
            .tasks
            .iter()
            .filter(|t| t.state == TState::Blocked)
            .filter_map(|t| t.wake_at);
        tasks.chain(self.k.locks.next_release()).min()
    }

    /// Fires every timer due at the current instant.
    pub(crate) fn fire_timers(&mut self) {
        let now = self.k.clock.boot_ns;
        for (node, byte) in self.k.locks.due_releases(now) {
            self.ev(EventKind::LockZombieFree, None, u32::MAX, node, byte, 0);
            self.kernel_freed(node, byte);
        }
        for t in &mut self.sched.tasks {
            if t.state == TState::Blocked && t.wake_at.is_some_and(|w| w <= now) {
                t.state = TState::Runnable;
            }
        }
    }

    /// Lets virtual time run to `target`, firing every timer on the way in order.
    pub(crate) fn run_time_to(&mut self, target: u64) {
        while let Some(t) = self.next_timer() {
            if t > target {
                break;
            }
            self.advance_to(t);
            self.fire_timers();
            if t == target {
                break;
            }
        }
        self.advance_to(target);
        self.fire_timers();
        let (b, m) = (self.k.clock.boot_ns, self.k.clock.mono_ns);
        self.ev(EventKind::Time, None, u32::MAX, b, m, 0);
    }

    /// A system suspend of `ns` (FM-6.3): the boot clock advances; the monotonic clock only if `mono_counts` (FM-7.2).
    pub(crate) fn suspend(&mut self, ns: u64, mono_counts: bool) {
        let target = self.k.clock.boot_ns.saturating_add(ns);
        let mono = self.k.clock.mono_ns;
        self.run_time_to(target);
        if !mono_counts {
            self.k.clock.mono_ns = mono;
        }
    }

    /// The boot-clock instant at which the monotonic clock reaches `mono_deadline` (assuming no suspend on the way).
    pub(crate) fn boot_at_mono(&self, mono_deadline: u64) -> u64 {
        let c = &self.k.clock;
        c.boot_ns
            .saturating_add(mono_deadline.saturating_sub(c.mono_ns))
    }

    // ---- scheduling ----

    /// Marks every task blocked for `why` runnable.
    pub(crate) fn wake(&mut self, why: Block) {
        for t in &mut self.sched.tasks {
            if t.state == TState::Blocked && t.block == why {
                t.state = TState::Runnable;
            }
        }
    }

    /// Marks every task blocked in a parent watch runnable (they re-check).
    pub(crate) fn wake_parent_watchers(&mut self) {
        for t in &mut self.sched.tasks {
            if t.state == TState::Blocked && matches!(t.block, Block::Parent(_)) {
                t.state = TState::Runnable;
            }
        }
    }

    /// The next task to hold the baton, or `None` when every task is done. Advances the clock when every live task is
    /// blocked; aborts the run when none can ever wake.
    pub(crate) fn pick_next(&mut self, me: Option<u32>) -> Option<u32> {
        loop {
            let runnable: Vec<u32> = self
                .sched
                .tasks
                .iter()
                .enumerate()
                .filter(|(_, t)| t.state == TState::Runnable)
                .map(|(i, _)| i as u32)
                .collect();
            if !runnable.is_empty() {
                let proc = me.map_or(u32::MAX, |t| self.sched.tasks[t as usize].proc);
                let i = self.pick(
                    Site::Schedule,
                    proc,
                    u64::from(me.unwrap_or(u32::MAX)),
                    0,
                    runnable.len() as u64,
                );
                return Some(runnable[i as usize]);
            }
            if !self.sched.tasks.iter().any(|t| t.state == TState::Blocked) {
                return None;
            }
            match self.next_timer() {
                Some(t) => {
                    self.advance_to(t);
                    let (b, m) = (self.k.clock.boot_ns, self.k.clock.mono_ns);
                    self.ev(EventKind::Time, None, u32::MAX, b, m, 0);
                    self.fire_timers();
                }
                None => {
                    self.sched.deadlock = true;
                    for t in &mut self.sched.tasks {
                        if t.state == TState::Blocked {
                            t.state = TState::Runnable;
                            t.abort = true;
                        }
                    }
                }
            }
        }
    }

    // ---- processes ----

    /// Starts a simulated process.
    pub(crate) fn start_proc(
        &mut self,
        name: &str,
        parent: Option<u32>,
        boot_known: Option<bool>,
    ) -> u32 {
        let idx = self.k.procs.len() as u32;
        let known = match boot_known {
            Some(k) => k,
            None => self.pick(Site::BootMode, idx, 0, 0, 2) == 0,
        };
        let pid = 1_000 + 4 * idx;
        let mut sm = self.cfg.seed
            ^ 0x656E_7472_6F70_7900
            ^ u64::from(idx).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        let rand = Rng::new(splitmix(&mut sm));
        self.k.procs.push(ProcRec {
            name: name.to_owned(),
            pid,
            start_ns: self.k.clock.boot_ns,
            boot_seq: self.k.clock.boot_seq,
            alive: true,
            parent,
            boot_known: known,
            wall_offset_ms: 0,
            counters: VfsCounters::default(),
            maps: 0,
            failed_nonlazy: false,
            death: None,
            rand,
            scripted: VecDeque::new(),
        });
        self.ev(
            EventKind::ProcStart,
            None,
            idx,
            u64::from(pid),
            parent.map_or(u64::MAX, u64::from),
            u64::from(known),
        );
        idx
    }

    /// Closes handle `h`: the node's last close completes a delete-pending unlink (FM-8.3).
    pub(crate) fn close_handle(&mut self, h: u64) {
        let Some(rec) = self.k.handles.remove(&h) else {
            return;
        };
        if rec.map
            && let Some(p) = self.k.procs.get_mut(rec.proc as usize)
        {
            p.maps = p.maps.saturating_sub(1);
        }
        let node = rec.node;
        let Some(n) = self.k.ns.nodes.get_mut(&node) else {
            return;
        };
        n.open = n.open.saturating_sub(1);
        if n.open == 0
            && let Some((dir, name)) = n.delete_pending.take()
            && self.k.ns.child(dir, &name) == Some(node)
        {
            let id = self.k.ns.push(NsOp::Remove { dir, name, node });
            self.ev(EventKind::NsOp, None, rec.proc, id, 1, node);
        }
        self.k.ns.gc(node);
    }

    /// Ends process `p` ([F15 §2.5] "Process death").
    pub(crate) fn kill_proc(&mut self, p: u32, cause: DeathCause, plan: &DeathPlan) {
        if !self.alive(p) {
            return;
        }
        {
            let rec = &mut self.k.procs[p as usize];
            rec.alive = false;
            rec.death = Some(cause);
        }
        // An in-flight write applies partially.
        let writes: Vec<InFlightWrite> = self
            .k
            .writes
            .iter()
            .filter(|w| w.proc == p)
            .cloned()
            .collect();
        self.k.writes.retain(|w| w.proc != p);
        for w in writes {
            let pw = match plan.write {
                Some(pw) => pw,
                None => PartialWrite::from_choice(self.pick(
                    Site::PartialWrite,
                    p,
                    w.node,
                    w.offset,
                    u64::MAX,
                )),
            };
            self.apply_partial(p, &w, pw);
            self.ev(EventKind::InFlight, None, p, w.node, 0, pw.to_choice());
        }
        self.k.reads.retain(|r| r.proc != p);
        // An in-flight flush resolves to succeeded, failed or not performed.
        let flushes: Vec<InFlightFlush> = self
            .k
            .flushes
            .iter()
            .filter(|f| f.proc == p)
            .cloned()
            .collect();
        self.k.flushes.retain(|f| f.proc != p);
        for f in flushes {
            match f.what {
                FlushWhat::File { mark, meta } => {
                    let outcome = match plan.flush {
                        Some(o) => u64::from(o.min(2)),
                        None => self.pick(Site::FlushAtDeath, p, f.node, 0, 3),
                    };
                    if let Some(file) = self.k.ns.nodes.get_mut(&f.node).and_then(|n| n.file_mut())
                    {
                        match outcome {
                            0 => file.content.flush_ok(&mark, meta),
                            1 => file.content.flush_failed(&mark),
                            _ => {}
                        }
                    }
                    self.ev(EventKind::InFlight, None, p, f.node, 1, outcome);
                }
                FlushWhat::Dir { limit } => {
                    let outcome = match plan.flush {
                        Some(o) => u64::from(o.min(1)),
                        None => self.pick(Site::SyncDirAtDeath, p, f.node, 0, 2),
                    };
                    if outcome == 0 {
                        for id in self.k.ns.sync_dir_ok(f.node, limit) {
                            self.ev(EventKind::NsDurable, None, p, id, 0, 0);
                        }
                    }
                    self.ev(EventKind::InFlight, None, p, f.node, 2, outcome);
                }
            }
        }
        // Grant tables and kernel waits go; held bytes stay held for a delay (FM-8.1).
        self.tables.retain(|&(tp, _), _| tp != p);
        self.k.locks.drop_waits_of(p);
        let held = self.k.locks.held_by(p);
        let n_held = held.len() as u64;
        for (node, byte) in held {
            let kind = match LockByte::from_offset(byte) {
                Some(LockByte::Flush) => 1,
                Some(LockByte::Slot(_)) => 2,
                _ => 0,
            };
            let class = match plan.release_class {
                Some(c) => u64::from(c.min(2)),
                None => self.pick(Site::ReleaseClass, p, node, kind, 3),
            };
            self.release_drawn[class as usize] += 1;
            let release_at = if class == 2 {
                None
            } else {
                let d = match plan.release_delay_ns {
                    Some(d) => d,
                    None => self.pick(Site::ReleaseDelay, p, node, (class << 8) | kind, u64::MAX),
                };
                Some(self.k.clock.boot_ns.saturating_add(d))
            };
            self.k.locks.make_zombie(node, byte, p, release_at);
            self.ev(
                EventKind::LockZombie,
                None,
                p,
                node,
                byte,
                release_at.unwrap_or(u64::MAX),
            );
        }
        // Handles and mappings close.
        let mine: Vec<u64> = self
            .k
            .handles
            .iter()
            .filter(|(_, h)| h.proc == p)
            .map(|(&id, _)| id)
            .collect();
        for h in mine {
            self.close_handle(h);
        }
        self.k.procs[p as usize].maps = 0;
        // Its tasks unwind when they next run.
        for t in &mut self.sched.tasks {
            if t.proc == p && t.state == TState::Blocked {
                t.state = TState::Runnable;
            }
        }
        self.wake_parent_watchers();
        self.ev(EventKind::ProcEnd, None, p, cause as u64, n_held, 0);
    }

    /// Applies an in-flight write partially (§2.5, FM-5.2).
    pub(crate) fn apply_partial(&mut self, proc: u32, w: &InFlightWrite, pw: PartialWrite) {
        let State { ch, k, .. } = self;
        let mut mask = pw.mask_rng();
        let node = w.node;
        let offset = w.offset;
        k.ns.edit(node, |c| {
            c.write_partial(
                offset,
                w.data.as_slice(),
                &mut |i| pw.applies(offset, i, &mut mask),
                &mut |site, aux, n| ch.pick(site, proc, node, aux, n),
            );
        });
    }

    /// The overlap alternatives a write of `data` at `offset` adds for a read of `[r_off, r_off + r_len)` of the same
    /// file (FM-4.1): per overlapping sub-sector, the cache sub-sector with the write's bytes laid over it.
    pub(crate) fn overlap_alts(
        &self,
        node: u64,
        offset: u64,
        data: &[u8],
        r_off: u64,
        r_len: u64,
    ) -> Vec<(u64, Box<[u8; 512]>)> {
        let mut out = Vec::new();
        let w_end = offset + data.len() as u64;
        let r_end = r_off + r_len;
        let lo = offset.max(r_off);
        let hi = w_end.min(r_end);
        if lo >= hi {
            return out;
        }
        let Some(file) = self.k.ns.nodes.get(&node).and_then(|n| n.file()) else {
            return out;
        };
        for sub in lo / 512..hi.div_ceil(512) {
            let mut b = Box::new([0u8; 512]);
            let a = sub * 512;
            file.content.read_plain(a, &mut b[..]);
            let from = a.max(offset);
            let to = (a + 512).min(w_end);
            b[(from - a) as usize..(to - a) as usize]
                .copy_from_slice(&data[(from - offset) as usize..(to - offset) as usize]);
            out.push((sub, b));
        }
        out
    }

    /// Registers a new handle on `node`.
    pub(crate) fn open_handle(&mut self, node: u64, proc: u32, map: bool) -> u64 {
        let h = self.k.next_handle;
        self.k.next_handle += 1;
        self.k.handles.insert(h, HandleRec { node, proc, map });
        self.k.ns.node_mut(node).open += 1;
        h
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// The call context: lock, death check, scheduling point

/// The simulator's view of one call in progress: the state lock, the calling task and process.
pub(crate) struct Ctx<'a> {
    pub(crate) sh: &'a Shared,
    g: Option<MutexGuard<'a, State>>,
    pub(crate) me: Option<u32>,
    pub(crate) proc: u32,
}

impl<'a> Ctx<'a> {
    /// Locks the world for a call of process `proc` without a scheduling point.
    pub(crate) fn quiet(sh: &'a Shared, proc: u32) -> Ctx<'a> {
        let g = sh.lock();
        let me = current_task(sh);
        if me.is_none() && g.sched.active {
            drop(g);
            panic!("simulator: a thread that is not a task used the simulated Vfs while tasks run");
        }
        if let Some(t) = me {
            let tp = g.sched.tasks[t as usize].proc;
            if tp != proc && proc != u32::MAX {
                drop(g);
                panic!("simulator: task {t} of process {tp} used the Vfs of process {proc}");
            }
        }
        let mut ctx = Ctx {
            sh,
            g: Some(g),
            me,
            proc,
        };
        if proc != u32::MAX && !ctx.st().alive(proc) {
            ctx.die();
        }
        ctx
    }

    /// Locks the world for call `call` of process `proc` and passes its start's scheduling point.
    pub(crate) fn enter(sh: &'a Shared, proc: u32, call: CallKind, node: u64) -> Ctx<'a> {
        let mut ctx = Ctx::quiet(sh, proc);
        ctx.point(call, node, 0);
        ctx
    }

    /// The `Return` event of call `call` on `node` with result code `code` (0 success, else [`error_code`]).
    pub(crate) fn ret_code(&mut self, call: CallKind, node: u64, code: u64) {
        let (me, proc) = (self.me, self.proc);
        self.st()
            .ev(EventKind::Return, me, proc, call as u64, node, code);
    }

    pub(crate) fn st(&mut self) -> &mut State {
        self.g
            .as_deref_mut()
            .expect("simulator: the call context holds the lock")
    }

    /// Unwinds the calling thread: its process is dead.
    pub(crate) fn die(&mut self) -> ! {
        self.g = None;
        unwind(SimUnwind::Died)
    }

    /// Kills the calling process with `cause`, then unwinds.
    pub(crate) fn kill_self(&mut self, cause: DeathCause) -> ! {
        let p = self.proc;
        self.st().kill_proc(p, cause, &DeathPlan::default());
        self.die()
    }

    /// A scheduling point ([F15 §6.4] "Crash points"): the trace records it, crash triggers and a seeded system crash may
    /// fire, time ticks, the adversary may suspend the system or pause the caller, and another task may run.
    pub(crate) fn point(&mut self, call: CallKind, node: u64, phase: u64) {
        let me = self.me;
        let proc = self.proc;
        let st = self.st();
        st.k.points += 1;
        let n = st.k.points;
        st.ev(EventKind::Point, me, proc, n, call as u64, phase);
        // Crash triggers.
        let mut fire: Option<crate::crash::CrashPlan> = None;
        let mut capture = false;
        let mut kills: Vec<(u32, DeathPlan)> = Vec::new();
        st.triggers.retain(|t| match t {
            Trigger::Capture(at) if *at == n => {
                capture = true;
                false
            }
            Trigger::Crash(at, plan) if *at == n => {
                fire = Some(plan.clone());
                false
            }
            Trigger::Kill(at, p, plan) if *at == n => {
                kills.push((*p, plan.clone()));
                false
            }
            _ => true,
        });
        if capture {
            let img = crate::crash::CrashImage::capture(st);
            st.captures.push((n, img));
        }
        let mut self_killed = false;
        for (p, plan) in kills {
            st.kill_proc(p, DeathCause::Killed, &plan);
            self_killed |= p == proc;
        }
        if self_killed {
            self.die();
        }
        let st = self.st();
        if fire.is_none() && st.pick(Site::SystemCrash, proc, node, n, 2) == 1 {
            fire = Some(crate::crash::CrashPlan::seeded());
        }
        if let Some(plan) = fire {
            crate::crash::crash_in_place(st, &plan)
                .unwrap_or_else(|e| panic!("simulator: crash plan at point {n} rejected: {e}"));
            self.die();
        }
        // Time.
        let tick = st.cfg.tick_ns;
        let now = st.k.clock.boot_ns;
        st.run_time_to_quiet(now + tick);
        let s = st.pick(Site::Suspend, proc, node, n, u64::MAX);
        if s > 0 {
            let counts = st.pick(Site::SuspendMono, proc, node, n, 2) == 1;
            st.suspend(s, counts);
        }
        crate::locks::reoffer(st);
        let pause = st.pick(
            Site::Pause,
            proc,
            u64::from(me.unwrap_or(u32::MAX)),
            n,
            u64::MAX,
        );
        if pause > 0 {
            let until = st.k.clock.boot_ns.saturating_add(pause);
            self.block(Block::Timer, Some(until));
        } else {
            self.yield_now();
        }
    }

    /// Gives other runnable tasks a chance to run (task mode only).
    pub(crate) fn yield_now(&mut self) {
        let Some(me) = self.me else {
            return;
        };
        let st = self.st();
        if !st.sched.active {
            return;
        }
        st.sched.tasks[me as usize].state = TState::Runnable;
        self.switch(me);
    }

    /// Blocks the caller for `why` until woken or until boot-clock instant `until`. In direct mode the virtual clock runs
    /// to the earlier of `until` and the next timer. Returns when the caller should re-check its condition.
    pub(crate) fn block(&mut self, why: Block, until: Option<u64>) {
        let me = self.me;
        let st = self.st();
        if until.is_some_and(|u| u <= st.k.clock.boot_ns) {
            return;
        }
        match me {
            Some(t) if st.sched.active => {
                let rec = &mut st.sched.tasks[t as usize];
                rec.state = TState::Blocked;
                rec.block = why;
                rec.wake_at = until;
                self.switch(t);
            }
            _ => {
                let target = match (until, st.next_timer()) {
                    (Some(u), Some(t)) => u.min(t),
                    (Some(u), None) => u,
                    (None, Some(t)) => t,
                    (None, None) => {
                        self.g = None;
                        panic!("simulator: the driver thread would wait forever ({why:?})");
                    }
                };
                st.run_time_to(target);
            }
        }
    }

    /// Hands the baton to the task the adversary picks and waits for it to come back.
    fn switch(&mut self, me: u32) {
        let st = self.st();
        let next = st.pick_next(Some(me));
        if next != Some(me) {
            st.sched.running = next;
            self.sh.cv.notify_all();
            let mut g = self
                .g
                .take()
                .expect("simulator: the call context holds the lock");
            while g.sched.running != Some(me) {
                g = self.sh.cv.wait(g).unwrap_or_else(PoisonError::into_inner);
            }
            self.g = Some(g);
        }
        let proc = self.proc;
        let st = self.st();
        let rec = &mut st.sched.tasks[me as usize];
        rec.state = TState::Running;
        rec.block = Block::None;
        rec.wake_at = None;
        if rec.abort {
            self.g = None;
            unwind(SimUnwind::Deadlock);
        }
        if proc != u32::MAX && !st.alive(proc) {
            self.die();
        }
    }
}

impl State {
    /// `run_time_to` without a `Time` event (the per-point tick).
    pub(crate) fn run_time_to_quiet(&mut self, target: u64) {
        while let Some(t) = self.next_timer() {
            if t > target {
                break;
            }
            self.advance_to(t);
            self.fire_timers();
        }
        self.advance_to(target);
        self.fire_timers();
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// World construction and tasks

/// Builds the initial state of a world.
pub(crate) fn new_state(cfg: SimConfig, adv: Box<dyn Adversary>) -> State {
    let mut sm = cfg.seed ^ 0x6D6F_6972_6169_0001;
    let boot_id = cfg.first_boot.unwrap_or_else(|| {
        let mut b = [0u8; 16];
        b[..8].copy_from_slice(&splitmix(&mut sm).to_le_bytes());
        b[8..].copy_from_slice(&splitmix(&mut sm).to_le_bytes());
        BootId(b)
    });
    let boot_origin = 1_000_000_000 + (splitmix(&mut sm) % 1_000_000_000);
    let mono_origin = splitmix(&mut sm) % (1 << 40);
    let mut volumes = vec![cfg.root_volume.clone()];
    let mut ns = Ns::new();
    for (path, profile) in &cfg.volumes {
        let vol = volumes.len() as u32;
        volumes.push(profile.clone());
        let comps =
            abs_components(path).unwrap_or_else(|e| panic!("simulator: volume path {path:?}: {e}"));
        let mut at = ROOT;
        for (i, c) in comps.iter().enumerate() {
            let parent_vol = ns.node(at).vol;
            let v = if i + 1 == comps.len() {
                vol
            } else {
                parent_vol
            };
            at = ns.mkdir_durable(at, c, v);
        }
        ns.node_mut(at).vol = vol;
    }
    let trace = Trace::new(cfg.trace);
    let mut ch = Chooser {
        adv,
        rng: Rng::new(cfg.seed),
        trace,
        queue: Vec::new(),
    };
    let wait_mode = match cfg.wait_mode {
        Some(m) => m,
        None => {
            if ch.pick(Site::WaitMode, u32::MAX, 0, 0, 2) == 0 {
                WaitMode::CallerDriven
            } else {
                WaitMode::WaiterThread
            }
        }
    };
    let clock = Clocks {
        boot_seq: 1,
        boot_id,
        boot_ns: boot_origin,
        mono_ns: mono_origin,
        boot_origin_ns: boot_origin,
        wall_at_boot_ms: cfg.wall_origin_ms,
    };
    let mut st = State {
        cfg,
        ch,
        k: Kernel {
            ns,
            clock,
            locks: KernelLocks::default(),
            procs: Vec::new(),
            handles: BTreeMap::new(),
            next_handle: 1,
            writes: Vec::new(),
            reads: Vec::new(),
            flushes: Vec::new(),
            next_io: 1,
            points: 0,
            volumes,
            wakes: BTreeMap::new(),
            next_wake: 1,
        },
        sched: Sched::default(),
        tables: BTreeMap::new(),
        next_table: 1,
        wait_mode,
        triggers: Vec::new(),
        captures: Vec::new(),
        violations: Vec::new(),
        stderr: Vec::new(),
        spawns: Vec::new(),
        release_drawn: [0; 3],
    };
    let h = boot_id.hash();
    st.ev(EventKind::Boot, None, u32::MAX, 1, h, boot_origin);
    st
}

/// How a task ended.
#[derive(Debug)]
pub enum TaskEnd<R> {
    /// The task's closure returned.
    Returned(R),
    /// The task's process died (a kill, `fail_stop`, a mapping fault, a crash) or the run deadlocked.
    Unwound(SimUnwind),
    /// The task panicked (a harness assertion, or a programming error the grant table reports); the message.
    Panicked(String),
}

impl<R> TaskEnd<R> {
    /// The returned value; panics with the task's end otherwise.
    pub fn unwrap(self) -> R {
        match self {
            TaskEnd::Returned(r) => r,
            TaskEnd::Unwound(u) => panic!("simulated task unwound: {u:?}"),
            TaskEnd::Panicked(m) => panic!("simulated task panicked: {m}"),
        }
    }
}

/// A spawned task; its end is available after [`crate::SimWorld::run`] returns.
pub struct Task<R> {
    pub(crate) slot: Arc<Mutex<Option<TaskEnd<R>>>>,
    pub(crate) id: u32,
}

impl<R> Task<R> {
    /// The task number (as the trace shows it).
    pub fn id(&self) -> u32 {
        self.id
    }

    /// How the task ended; `None` if it has not run to its end.
    pub fn end(self) -> Option<TaskEnd<R>> {
        self.slot
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
    }
}

/// The result of [`crate::SimWorld::run`].
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct RunReport {
    /// Every task was blocked with no timer at some point, and the blocked tasks were unwound.
    pub deadlock: bool,
    /// The scheduling points passed so far in this world.
    pub points: u64,
    /// The release-delay classes drawn so far in this world for the bytes of dead processes, indexed by class: (a)
    /// measured, (b) beyond every wait bound, (c) never released (FM-8.1). The nightly harness asserts [F15 §3.8]'s
    /// obligation — at least one of (b) and one of (c) per run — from these counts.
    pub release_classes: [u64; 3],
}

/// The body of a task thread.
pub(crate) fn task_main<R: Send + 'static>(
    sh: Arc<Shared>,
    tid: u32,
    body: Box<dyn FnOnce() -> R + Send>,
    slot: Arc<Mutex<Option<TaskEnd<R>>>>,
) {
    CURRENT.with(|c| c.set((sh.addr(), tid)));
    {
        let mut g = sh.lock();
        while g.sched.running != Some(tid) {
            g = sh.cv.wait(g).unwrap_or_else(PoisonError::into_inner);
        }
        let proc = g.sched.tasks[tid as usize].proc;
        g.sched.tasks[tid as usize].state = TState::Running;
        g.ev(EventKind::TaskStart, Some(tid), proc, u64::from(tid), 0, 0);
        let dead = !g.alive(proc);
        drop(g);
        if dead {
            let end = TaskEnd::Unwound(SimUnwind::Died);
            finish_task(&sh, tid, end, &slot);
            return;
        }
    }
    let end = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(body)) {
        Ok(r) => TaskEnd::Returned(r),
        Err(payload) => match payload.downcast::<SimUnwind>() {
            Ok(u) => TaskEnd::Unwound(*u),
            Err(p) => {
                let msg = if let Some(s) = p.downcast_ref::<&str>() {
                    (*s).to_owned()
                } else if let Some(s) = p.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "non-string panic payload".to_owned()
                };
                TaskEnd::Panicked(msg)
            }
        },
    };
    finish_task(&sh, tid, end, &slot);
}

fn finish_task<R>(sh: &Shared, tid: u32, end: TaskEnd<R>, slot: &Mutex<Option<TaskEnd<R>>>) {
    let code = match &end {
        TaskEnd::Returned(_) => 0,
        TaskEnd::Unwound(SimUnwind::Died) => 1,
        TaskEnd::Panicked(_) => 2,
        TaskEnd::Unwound(SimUnwind::Deadlock) => 3,
    };
    *slot.lock().unwrap_or_else(PoisonError::into_inner) = Some(end);
    let mut g = sh.lock();
    let proc = g.sched.tasks[tid as usize].proc;
    g.sched.tasks[tid as usize].state = TState::Done;
    g.ev(EventKind::TaskEnd, Some(tid), proc, u64::from(tid), code, 0);
    let next = g.pick_next(None);
    g.sched.running = next;
    sh.cv.notify_all();
    drop(g);
    CURRENT.with(|c| c.set((0, u32::MAX)));
}

/// Injects a read error on a file range (FM-12).
pub(crate) fn add_read_error(
    st: &mut State,
    node: u64,
    offset: u64,
    len: u64,
    remaining: Option<u32>,
) {
    if let Some(Kind::File(f)) = st.k.ns.nodes.get_mut(&node).map(|n| &mut n.kind) {
        f.read_errors.push(ReadError {
            offset,
            len,
            remaining,
        });
    }
}
