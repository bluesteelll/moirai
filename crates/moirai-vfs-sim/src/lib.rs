//! The in-memory `Vfs` that enforces fault-model items 1–12 ([F15 §3]), for the crash enumerator (WP-32), the toy log
//! (WP-40) and every crash gate after them.
//!
//! Test-only crate, never linked into a product binary; checked by GT20 (e) on every target (PLAN §2.2). Filled by
//! WP-31 (the simulator) and WP-32 (the crash enumerator, module [`enumerate`]), both R-HARN-S; the enumerator's author
//! is never the author of the seeded bugs (S4). Sources: [F15 §2–§6] (`docs/spec/format/15-fault-model.md`), [OS/README
//! §2.3, §4.6, §5.4], [OS/lock §5.1, §7.3, §8, §9], [OS/proc §10.1], [OS/clock §5], [OS/map] (simulator row), [OS/fs
//! §4.4, §4.9, §4.12, §4.13, §6.2–§6.4]; [60 §3.1] item 3, §3.13 GT1; [80 §2.4.4]; `docs/m0/PLAN.md` §2.2, §3.2 WP-31.
//!
//! # Shape
//!
//! A [`SimWorld`] is one simulated kernel: one namespace and page cache, one set of lock bytes, one boot with its three
//! clocks. [`SimWorld::process`] starts a simulated process and returns its [`SimVfs`], which implements
//! `moirai_vfs::Vfs`; clones of a `SimVfs` are the same process, so a process may run several lock clients and several
//! tasks ([F15 §2.1]; [OS/lock §5.1]: one real `GrantTable` per `LOCK` per simulated process, in the wait mode the seed
//! picks). Code runs either on the test's own thread (the driver, one call at a time) or as tasks
//! ([`SimWorld::spawn`], [`SimWorld::run`]) that the seeded scheduler interleaves at every call boundary. Each simulated
//! process draws `fill_random` from its own seeded stream ([OS/README §4.6] "Simulator form"; [`SimWorld::script_random`]
//! scripts the next bytes for a draw-again branch).
//!
//! # The fault model, item by item
//!
//! | Item | Where | What the simulator does |
//! |---|---|---|
//! | FM-1 | `content` | keeps every dirty sector's baseline and versions (a write of unchanged bytes included: rewritten runs); a crash keeps any one per sector, at most one torn sector per file (sub-sectors mixed) |
//! | FM-2 | `namespace`, `vfs` | `sync(Data)` below ds, `sync(DataAndMeta)` with the size (never moved back by an older flush); namespace operations pending until `sync_dir` of every parent; any subset lost at a crash, in any order |
//! | FM-3 | `content`, `vfs` | a failed flush poisons every sector dirty at any instant of its interval (rewritten ones, those written during it, and those a concurrent successful flush cleaned during it included); reads, mapped reads too, draw each sub-sector from K afresh; poisoning survives successful flushes and crashes; only a re-write ends it |
//! | FM-4 | `vfs` | reads and writes have intervals; an overlapping read shows each sub-sector's content from any moment of its interval |
//! | FM-5 | `vfs` | `DiskFull` (and `Io`) at every write, flush, create, size change and namespace operation; a failed write applies any subset of its bytes; a failed create may leave an empty file or an empty directory |
//! | FM-6 | `world` | pauses of any task at any scheduling point, inside calls too; system suspends |
//! | FM-7 | `world`, `vfs` | wall-clock steps per process, monotonic and boot clocks per boot (a suspend may skip the monotonic clock), a boot identity per boot, Known or Unknown per process and per read |
//! | FM-8 | `locks`, `vfs` | release delays per byte from classes (a), (b), (c), counted per world ([`RunReport::release_classes`]); sharing violations (errors 32 and 5) with a persistence, retried per [OS/fs §6.3] on Windows only; delete-pending unlinks |
//! | FM-9 | `vfs` (`SimMap`) | a mapped read of a truncated file ends the process or reads zeros; injected media faults end it; mappings see the file's current content |
//! | FM-10 | [`SimWorld`], `locks` | external truncation, rewrite, extension, flush and `LOCK` replacement, which every later `LOCK` handle's identity check detects |
//! | FM-11 | `world`, `locks` | several processes and clients, death at every event including inside a flush with each of its three outcomes, seeded interleaving |
//! | FM-12 | `vfs` | transient and persistent read errors, injected or drawn |
//!
//! Every freedom is an adversary choice ([`Site`]) recorded in the trace; [`SimWorld::queue_choice`] forces the next one
//! (by site, process and node), and a custom [`Adversary`] drives them all. System crashes go through [`CrashImage`],
//! [`CrashSurface`] and [`CrashPlan`] (module `crash`). `swap_dirs` and `swap_recover` follow [OS/fs §4.9] (module
//! `swap`): the native exchange on Linux and macOS worlds whose volume has it, else the renames guarded by the intent
//! file, each step a call of its own.
//!
//! # Driving it (for the enumerator)
//!
//! Module [`enumerate`] is the crash enumerator (WP-32) built on what follows.
//!
//! Every call boundary is a numbered scheduling point ([`SimWorld::points`]; the trace's `Point` events), the crash
//! points of [F15 §6.4]. [`SimWorld::record_points`] logs them ([`PointInfo`]), and with them every process that is
//! inside a flush or a lock wait at each one ([`BusyAt`], [`SimWorld::take_busy_log`]); [`SimWorld::capture_calls`]
//! captures a [`CrashImage`] at every point of the calls that change what a crash leaves ([`CallKind::changes_storage`]),
//! and [`SimWorld::kill_proc_at`] kills a process named by its number — at its own point or at another process's.
//! [`SimWorld::capture_at`] takes a [`CrashImage`] at a point without disturbing the run, [`SimWorld::crash_at`] crashes
//! the system there, [`SimWorld::kill_at`] kills one process there — inside a flush too, with the outcome its
//! [`DeathPlan`] names, each member of a `sync_group` separately ([`DeathPlan::flush_each`]) — and
//! [`SimWorld::truncate_at`] lets an external actor truncate a file there (FM-10). An image's [`CrashSurface`] lists
//! every file's size history and non-clean sectors with their candidate counts, every pending namespace operation with
//! the parents that synced it, and every write in flight; [`CrashImage::materialize`] builds the post-crash world a
//! [`CrashPlan`] picks, as often as the caller likes, and [`CrashImage::reseeded`] draws fresh random states.
//! [`SimWorld::put_file`] and [`SimWorld::mkdir_all`] build the pre-existing environment without events.
//! [`SimWorld::poisoned_below`] (by path) and [`SimWorld::poisoned_below_node`] (by node) tell a checker whether reads of
//! a file's prefix may differ from one read to the next because a sector there is poisoned (FM-3.2), so that it judges
//! two reads of one range only where FM-3 lets them agree. Protocol violations the simulator can see ([F15 §3.13],
//! OP-20) are listed by [`SimWorld::violations`]; the grant table's programming errors panic in the task.
//!
//! # Cost
//!
//! File contents are 4 KiB pages shared copy-on-write, and the trace keeps frozen chunks shared by every copy, so a
//! crash image captured at every scheduling point copies no file data and no trace prefix: its cost is the kernel's
//! maps (one pointer per page, one entry per non-clean sector and per name). The namespace indexes names both ways and
//! keeps per-volume size totals, so no operation scans the namespace. A write in flight keeps a copy of its bytes (zeros
//! excepted), because a death or crash inside it is resolved by another thread.

#![forbid(unsafe_code)]

mod adversary;
mod content;
mod crash;
pub mod enumerate;
mod locks;
mod namespace;
mod rng;
mod swap;
mod trace;
mod vfs;
mod world;

use std::path::Path;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

pub use adversary::{
    Adversary, Choice, FaultRates, PartialWrite, ReleaseDelayLaw, SeededAdversary, Site,
};
pub use content::{BeyondFill, SECTOR, SUBSECTOR, SectorKind, SectorView};
pub use crash::{
    CrashImage, CrashPlan, CrashSurface, FilePlan, FileSurface, OpSurface, PlanError, Resolve,
    SectorPick, WriteSurface,
};
pub use locks::SimClient;
pub use namespace::NsKind;
pub use rng::Rng;
pub use trace::{
    CLASS_DISCOVERABLE, CLASS_LOG, CLASS_SLOT, EVENT_LEN, Event, EventKind, NOTE_CLASS,
    NOTE_COMPOSITE, NOTE_COMPOSITE_END, NOTE_PATH, NOTE_PROBE, NOTE_SLOT_WRITE, TraceMode,
    path_hash, path_hash_of,
};
pub use vfs::{SimFile, SimMap, SimParentWatch, SimRoot, SimVfs, SimWake};
pub use world::{
    BusyAt, CallKind, DeathCause, DeathPlan, PointInfo, RunReport, SimConfig, SimUnwind,
    SpawnRequest, Task, TaskEnd, Violation, ViolationKind, VolumeProfile, Watch, catch_death,
    error_code,
};

use moirai_vfs::{BootId, VfsErrorKind, WaitMode};

use crate::content::{Content, ZEROS};
use crate::namespace::{Kind, NsOp, abs_components};
use crate::world::{
    Queued, Shared, State, TState, TaskRec, Trigger, current_task, new_state, task_main,
};

/// One simulated kernel with its processes, adversary, trace and scheduler. Cheap to clone; clones share the world.
#[derive(Clone)]
pub struct SimWorld {
    sh: Arc<Shared>,
}

impl core::fmt::Debug for SimWorld {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let g = self.sh.lock();
        f.debug_struct("SimWorld")
            .field("boot", &g.k.clock.boot_seq)
            .field("points", &g.k.points)
            .field("processes", &g.k.procs.len())
            .finish()
    }
}

impl SimWorld {
    /// A world with the default seeded adversary of `cfg`.
    pub fn new(cfg: SimConfig) -> SimWorld {
        let adv = SeededAdversary::new(cfg.rates, cfg.release_law.clone());
        SimWorld::with_adversary(cfg, Box::new(adv))
    }

    /// A world whose every choice `adv` makes.
    pub fn with_adversary(cfg: SimConfig, adv: Box<dyn Adversary>) -> SimWorld {
        SimWorld::from_state(new_state(cfg, adv))
    }

    pub(crate) fn from_state(st: State) -> SimWorld {
        SimWorld {
            sh: Arc::new(Shared {
                st: Mutex::new(st),
                cv: Condvar::new(),
            }),
        }
    }

    /// The state, for a call made outside the scheduler (from the driver while no task runs, or from a task).
    fn driver(&self) -> MutexGuard<'_, State> {
        let g = self.sh.lock();
        if g.sched.active && current_task(&self.sh).is_none() {
            drop(g);
            panic!("simulator: the driver thread used the world while tasks run");
        }
        g
    }

    fn check(&self, vfs: &SimVfs) {
        assert!(
            Arc::ptr_eq(&self.sh, &vfs.sh),
            "simulator: a SimVfs of another world"
        );
    }

    // ---- environment and processes ----

    /// Creates the directories of the absolute `path` that do not exist, durably and without events: the environment
    /// that exists before the scenario (for example the parent of a store directory).
    pub fn mkdir_all(&self, path: &Path) {
        let mut g = self.driver();
        let comps =
            abs_components(path).unwrap_or_else(|e| panic!("simulator: mkdir_all {path:?}: {e}"));
        let mut at = namespace::ROOT;
        for c in comps {
            let vol = g.k.ns.node(at).vol;
            at = g.k.ns.mkdir_durable(at, &c, vol);
        }
    }

    /// Creates a file at the absolute `path` holding `data`, durably (content, size and name) and without events: a file
    /// that exists before the scenario, such as a fixture store's `LOCK`. The parent directory must exist.
    pub fn put_file(&self, path: &Path, data: &[u8]) -> Result<(), VfsErrorKind> {
        let mut g = self.driver();
        let dir = match path.parent() {
            Some(p) => g.k.ns.lookup_abs(p)?,
            None => return Err(VfsErrorKind::InvalidName),
        };
        if !g.k.ns.node(dir).is_dir() {
            return Err(VfsErrorKind::NotFound);
        }
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or(VfsErrorKind::InvalidName)?;
        moirai_vfs::RelPath::new(name).map_err(|_| VfsErrorKind::InvalidName)?;
        if g.k.ns.child(dir, name).is_some() {
            return Err(VfsErrorKind::AlreadyExists);
        }
        let vol = g.k.ns.node(dir).vol;
        let file = namespace::FileNode {
            content: Content::durable(data),
            ..namespace::FileNode::default()
        };
        let node = g.k.ns.new_node(Kind::File(Box::new(file)), vol);
        g.k.ns.name_durable(dir, name, node);
        Ok(())
    }

    /// Starts a simulated process; its boot-identity mode is drawn (FM-7.4).
    pub fn process(&self, name: &str) -> SimVfs {
        self.process_with(name, None, None)
    }

    /// Starts a simulated process with a parent (for the parent watch) and, if given, a fixed boot-identity mode.
    pub fn process_with(
        &self,
        name: &str,
        parent: Option<&SimVfs>,
        boot_known: Option<bool>,
    ) -> SimVfs {
        if let Some(p) = parent {
            self.check(p);
        }
        let mut g = self.driver();
        let idx = g.start_proc(name, parent.map(|p| p.proc), boot_known);
        SimVfs {
            sh: Arc::clone(&self.sh),
            proc: idx,
        }
    }

    /// Spawns a task (a simulated thread) of `vfs`'s process running `f`. It starts at the next [`SimWorld::run`].
    pub fn spawn<R: Send + 'static>(
        &self,
        vfs: &SimVfs,
        f: impl FnOnce(SimVfs) -> R + Send + 'static,
    ) -> Task<R> {
        self.check(vfs);
        let slot = Arc::new(Mutex::new(None));
        let mut g = self.sh.lock();
        let tid = g.sched.next_task;
        g.sched.next_task += 1;
        g.sched.tasks.insert(
            tid,
            TaskRec {
                proc: vfs.proc,
                state: TState::Runnable,
                block: world::Block::None,
                wake_at: None,
                abort: false,
            },
        );
        let sh = Arc::clone(&self.sh);
        let v = vfs.clone();
        let s = Arc::clone(&slot);
        let body: Box<dyn FnOnce() -> R + Send> = Box::new(move || f(v));
        let handle = std::thread::Builder::new()
            .name(format!("sim-task-{tid}"))
            .spawn(move || task_main(sh, tid, body, s))
            .expect("simulator: a task thread starts");
        g.sched.threads.push(handle);
        Task { slot, id: tid }
    }

    /// Runs every spawned task to its end under the seeded scheduler and joins their threads.
    pub fn run(&self) -> RunReport {
        let mut g = self.sh.lock();
        assert!(
            !g.sched.active,
            "simulator: run() while a run is in progress"
        );
        assert!(
            current_task(&self.sh).is_none(),
            "simulator: run() from a task"
        );
        g.sched.active = true;
        g.sched.deadlock = false;
        let first = g.pick_next(None);
        g.sched.running = first;
        self.sh.cv.notify_all();
        while !g.sched.tasks.is_empty() {
            g = self
                .sh
                .cv
                .wait(g)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        g.sched.active = false;
        g.sched.running = None;
        let threads = core::mem::take(&mut g.sched.threads);
        let report = RunReport {
            deadlock: g.sched.deadlock,
            points: g.k.points,
            release_classes: g.release_drawn,
        };
        drop(g);
        for t in threads {
            let _ = t.join();
        }
        report
    }

    /// Kills the process of `vfs` ([F15 §2.5] "Process death"), resolving its in-flight calls and lock delays by `plan`.
    pub fn kill(&self, vfs: &SimVfs, plan: DeathPlan) {
        self.check(vfs);
        let mut g = self.driver();
        g.kill_proc(vfs.proc, DeathCause::Killed, &plan);
    }

    /// Ends the process of `vfs` normally (its parent watchers see it exit; lock bytes follow FM-8.1 as for any end).
    pub fn exit(&self, vfs: &SimVfs) {
        self.check(vfs);
        let mut g = self.driver();
        g.kill_proc(vfs.proc, DeathCause::Exit, &DeathPlan::default());
    }

    /// How the process of `vfs` ended, if it has.
    pub fn death(&self, vfs: &SimVfs) -> Option<DeathCause> {
        self.check(vfs);
        self.sh.lock().k.procs[vfs.proc as usize].death
    }

    // ---- crashes ----

    /// A system crash now, resolved by `plan`; the world continues in its next boot.
    pub fn crash(&self, plan: &CrashPlan) -> Result<(), PlanError> {
        let mut g = self.driver();
        crash::crash_in_place(&mut g, plan)
    }

    /// A copy of the kernel now, to materialise post-crash worlds from.
    pub fn crash_image(&self) -> CrashImage {
        CrashImage::capture(&self.sh.lock())
    }

    /// What a crash now may decide.
    pub fn surface(&self) -> CrashSurface {
        crash::surface_of(&self.sh.lock().k)
    }

    /// Crashes the system at scheduling point `point` (the trace's `Point` number) with `plan`: the calling task and
    /// every other task unwind with [`SimUnwind::Died`].
    pub fn crash_at(&self, point: u64, plan: CrashPlan) {
        self.sh.lock().triggers.push(Trigger::Crash(point, plan));
    }

    /// Kills the process of `vfs` at scheduling point `point` (process death at every event, inside a call too:
    /// [F15 §6.4] "Process death"), resolving its in-flight calls by `plan`.
    pub fn kill_at(&self, point: u64, vfs: &SimVfs, plan: DeathPlan) {
        self.check(vfs);
        self.sh
            .lock()
            .triggers
            .push(Trigger::Kill(point, vfs.proc, plan));
    }

    /// Kills process number `proc` (as [`PointInfo::proc`] and the trace name it) at scheduling point `point`, like
    /// [`SimWorld::kill_at`]: the crash enumerator names the process a point belongs to before the workload has made it.
    pub fn kill_proc_at(&self, point: u64, proc: u32, plan: DeathPlan) {
        self.sh
            .lock()
            .triggers
            .push(Trigger::Kill(point, proc, plan));
    }

    /// An external actor truncates (or extends) file node `node` (as [`PointInfo::node`] and
    /// [`SimWorld::node_at`] name it) to `len` at scheduling point `point`, before a capture or a death there sees the
    /// world (FM-10.1; FM-10.2 for a sealed file): the crash enumerator truncates a sealed file while readers still run.
    /// Nothing happens if the node is no longer a file then.
    pub fn truncate_at(&self, point: u64, node: u64, len: u64) {
        self.sh
            .lock()
            .triggers
            .push(Trigger::Truncate(point, node, len));
    }

    /// Captures a [`CrashImage`] at scheduling point `point`; the run continues.
    pub fn capture_at(&self, point: u64) {
        self.sh.lock().triggers.push(Trigger::Capture(point));
    }

    /// Captures a [`CrashImage`] at every scheduling point from `from_point` on whose call is one of `calls`, at most
    /// `limit` times; each image names its point ([`CrashImage::origin`]). The run continues. The crash enumerator takes
    /// its crash points this way ([F15 §6.4]: every write, flush, publish, create, rename and unlink).
    pub fn capture_calls(&self, from_point: u64, calls: &[CallKind], limit: usize) {
        if limit == 0 {
            return;
        }
        let mask = calls.iter().fold(0u64, |m, &c| m | (1u64 << (c as u64)));
        self.sh.lock().triggers.push(Trigger::CaptureCalls {
            from: from_point,
            mask,
            left: limit,
        });
    }

    /// Starts logging every scheduling point ([`SimWorld::take_point_log`]).
    pub fn record_points(&self) {
        self.sh.lock().point_log.get_or_insert_with(Vec::new);
    }

    /// The scheduling points logged since [`SimWorld::record_points`] (or the last take); logging continues.
    pub fn take_point_log(&self) -> Vec<PointInfo> {
        self.sh
            .lock()
            .point_log
            .as_mut()
            .map(core::mem::take)
            .unwrap_or_default()
    }

    /// The processes busy in a flush or a lock wait at the points logged since [`SimWorld::record_points`] (or the last
    /// take), in point order ([`BusyAt`]).
    pub fn take_busy_log(&self) -> Vec<BusyAt> {
        core::mem::take(&mut self.sh.lock().busy_log)
    }

    /// The images captured so far, with their points.
    pub fn take_captures(&self) -> Vec<(u64, CrashImage)> {
        core::mem::take(&mut self.sh.lock().captures)
    }

    // ---- time ----

    /// Lets `ns` of virtual time pass (every process may be paused meanwhile; timers fire).
    pub fn advance(&self, ns: u64) {
        let mut g = self.driver();
        let target = g.k.clock.boot_ns.saturating_add(ns);
        g.run_time_to(target);
    }

    /// A system suspend of `ns` (FM-6.3); the monotonic clock counts it only if `mono_counts` (FM-7.2).
    pub fn suspend(&self, ns: u64, mono_counts: bool) {
        let mut g = self.driver();
        g.suspend(ns, mono_counts);
    }

    /// Steps the wall clock of `vfs`'s process by `delta_ms` (FM-7.1).
    pub fn step_wall(&self, vfs: &SimVfs, delta_ms: i64) {
        self.check(vfs);
        let mut g = self.driver();
        let rec = &mut g.k.procs[vfs.proc as usize];
        rec.wall_offset_ms = rec.wall_offset_ms.saturating_add(delta_ms);
    }

    // ---- the adversary ----

    /// The next choice at `site` (by any process) answers `value` instead of asking the adversary. A queued choice is
    /// consumed by the next matching choice even where that choice has a single answer (then recorded as value 0).
    pub fn queue_choice(&self, site: Site, value: u64) {
        self.sh.lock().ch.queue.push(Queued {
            site,
            proc: None,
            node: None,
            value,
        });
    }

    /// The next choice at `site` made for `vfs`'s process answers `value`.
    pub fn queue_choice_for(&self, vfs: &SimVfs, site: Site, value: u64) {
        self.check(vfs);
        self.sh.lock().ch.queue.push(Queued {
            site,
            proc: Some(vfs.proc),
            node: None,
            value,
        });
    }

    /// The next choice at `site` about `node` ([`Choice::node`]: a file or directory node, or for `Schedule` and `Pause`
    /// the current task), made for `vfs`'s process if given, answers `value`.
    pub fn queue_choice_on(&self, vfs: Option<&SimVfs>, site: Site, node: u64, value: u64) {
        if let Some(v) = vfs {
            self.check(v);
        }
        self.sh.lock().ch.queue.push(Queued {
            site,
            proc: vfs.map(|v| v.proc),
            node: Some(node),
            value,
        });
    }

    /// The next `fill_random` draws of `vfs`'s process take `bytes` first, in order ([OS/README §4.6] "Simulator form":
    /// an all-zero value, or a repeat of a drawn one, to reach a caller's draw-again branch). The crash gates and the
    /// enumerator never script values ([F15 §6.3] A-7).
    pub fn script_random(&self, vfs: &SimVfs, bytes: &[u8]) {
        self.check(vfs);
        self.sh.lock().k.procs[vfs.proc as usize]
            .scripted
            .extend(bytes.iter().copied());
    }

    /// The release-delay classes drawn so far for dead processes' bytes: (a), (b), (c) (FM-8.1).
    pub fn release_classes(&self) -> [u64; 3] {
        self.sh.lock().release_drawn
    }

    // ---- external actors (FM-8.2, FM-9, FM-10, FM-12) ----

    fn file_at(g: &State, path: &Path) -> Result<u64, VfsErrorKind> {
        let n = g.k.ns.lookup_abs(path)?;
        if g.k.ns.node(n).is_dir() {
            return Err(VfsErrorKind::IsDirectory);
        }
        Ok(n)
    }

    /// An external actor truncates (or extends) the file at `path` to `len` (FM-10.1; FM-10.2 for a sealed file).
    pub fn external_truncate(&self, path: &Path, len: u64) -> Result<(), VfsErrorKind> {
        let mut g = self.driver();
        let node = SimWorld::file_at(&g, path)?;
        truncate_node(&mut g, node, len);
        Ok(())
    }

    /// An external actor writes `data` at `offset` of the file at `path` (FM-10.1): a write by a foreign client.
    pub fn external_write(
        &self,
        path: &Path,
        offset: u64,
        data: &[u8],
    ) -> Result<(), VfsErrorKind> {
        let mut g = self.driver();
        let node = SimWorld::file_at(&g, path)?;
        external_overlap(&mut g, node, offset, data);
        let State { ch, k, .. } = &mut *g;
        k.ns.edit(node, |c| {
            c.write(offset, data, &mut |site, aux, n| {
                ch.pick(site, u32::MAX, node, aux, n)
            });
        });
        g.note_class(None, u32::MAX, node);
        g.note_path(None, u32::MAX, node);
        g.ev(EventKind::External, None, u32::MAX, node, 1, offset);
        Ok(())
    }

    /// An external actor flushes the file at `path` (`FlushFileBuffers`); the flush may fail like any other (FM-3).
    pub fn external_flush(&self, path: &Path) -> Result<(), VfsErrorKind> {
        let mut g = self.driver();
        let node = SimWorld::file_at(&g, path)?;
        let mark = g.k.ns.file(node).content.flush_mark();
        let fault = g.pick(Site::FlushFault, u32::MAX, node, 1, 4);
        if fault == 0 {
            g.flush_succeeded(node, &mark, true);
        } else {
            g.flush_did_fail(node, &mark);
        }
        g.note_class(None, u32::MAX, node);
        g.note_path(None, u32::MAX, node);
        g.ev(EventKind::External, None, u32::MAX, node, 2, fault);
        Ok(())
    }

    /// An external actor replaces the file at `path` with a new file holding `data` (FM-10.3, `LOCK` replacement): a
    /// create under a temporary name and a replacing rename, both pending until their directory is synced.
    pub fn external_replace(&self, path: &Path, data: &[u8]) -> Result<(), VfsErrorKind> {
        let mut g = self.driver();
        let old = SimWorld::file_at(&g, path)?;
        let dir = match path.parent() {
            Some(p) => g.k.ns.lookup_abs(p)?,
            None => return Err(VfsErrorKind::InvalidName),
        };
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or(VfsErrorKind::InvalidName)?
            .to_owned();
        let vol = g.k.ns.node(dir).vol;
        let node = g.k.ns.new_node(Kind::File(Box::default()), vol);
        {
            let State { ch, k, .. } = &mut *g;
            k.ns.edit(node, |c| {
                c.write(0, data, &mut |site, aux, n| {
                    ch.pick(site, u32::MAX, node, aux, n)
                });
            });
        }
        let tmp = format!(".sim-replace-{node}");
        let a = g.k.ns.push(NsOp::Create {
            dir,
            name: tmp.clone(),
            node,
        });
        let b = g.k.ns.push(NsOp::Rename {
            from: (dir, tmp),
            to: (dir, name),
            node,
            replace: true,
            replaced: Some(old),
        });
        g.ev(
            EventKind::NsOp,
            None,
            u32::MAX,
            a,
            NsKind::Create as u64,
            node,
        );
        g.ev(
            EventKind::NsOp,
            None,
            u32::MAX,
            b,
            NsKind::RenameReplace as u64,
            node,
        );
        g.note_class(None, u32::MAX, node);
        g.note_path(None, u32::MAX, node);
        g.ev(EventKind::External, None, u32::MAX, old, 3, node);
        g.k.ns.gc(old);
        Ok(())
    }

    /// An external actor holds the file or directory at `path` without the share mode moirai needs: the next
    /// `attempts` opens, unlinks and renames of it fail — each with Windows error 32 (`SharingViolation`) or 5
    /// (`AccessDenied`), as [`Site::SharingKind`] picks — or every one when `None` (FM-8.2).
    pub fn hold_exclusive(&self, path: &Path, attempts: Option<u64>) -> Result<(), VfsErrorKind> {
        let mut g = self.driver();
        let node = g.k.ns.lookup_abs(path)?;
        g.k.ns.node_mut(node).share_block = attempts.unwrap_or(u64::MAX);
        g.note_class(None, u32::MAX, node);
        g.note_path(None, u32::MAX, node);
        g.ev(
            EventKind::External,
            None,
            u32::MAX,
            node,
            4,
            attempts.unwrap_or(u64::MAX),
        );
        Ok(())
    }

    /// Makes reads of `[offset, offset + len)` of the file at `path` fail with `Io` (FM-12): the next one only, or every
    /// one from now on, across crashes, when `persistent`.
    pub fn inject_read_error(
        &self,
        path: &Path,
        offset: u64,
        len: u64,
        persistent: bool,
    ) -> Result<(), VfsErrorKind> {
        let mut g = self.driver();
        let node = SimWorld::file_at(&g, path)?;
        world::add_read_error(
            &mut g,
            node,
            offset,
            len,
            if persistent { None } else { Some(1) },
        );
        g.note_class(None, u32::MAX, node);
        g.note_path(None, u32::MAX, node);
        g.ev(EventKind::External, None, u32::MAX, node, 5, offset);
        Ok(())
    }

    /// The next mapped read of the file at `path` meets a media fault: the reading process dies (FM-9.1).
    pub fn inject_map_fault(&self, path: &Path) -> Result<(), VfsErrorKind> {
        let mut g = self.driver();
        let node = SimWorld::file_at(&g, path)?;
        g.k.ns.file_mut(node).map_fault = true;
        g.note_class(None, u32::MAX, node);
        g.note_path(None, u32::MAX, node);
        g.ev(EventKind::External, None, u32::MAX, node, 6, 0);
        Ok(())
    }

    // ---- observation ----

    /// The cache image C(f) of the file at `path`: what a read returns where no sector is poisoned. For assertions.
    pub fn peek(&self, path: &Path) -> Result<Vec<u8>, VfsErrorKind> {
        let g = self.sh.lock();
        let node = SimWorld::file_at(&g, path)?;
        Ok(g.k.ns.file(node).content.to_vec())
    }

    /// Every sealed file of the current namespace, by path, with its size (the targets of FM-10.2's external
    /// truncation).
    pub fn sealed_files(&self) -> Vec<(std::path::PathBuf, u64)> {
        let g = self.sh.lock();
        let mut out = Vec::new();
        for (&n, node) in &g.k.ns.nodes {
            if let Some(f) = node.file()
                && f.sealed
            {
                let size = f.content.cs();
                out.extend(
                    g.k.ns
                        .paths_of(n)
                        .into_iter()
                        .map(|p| (std::path::PathBuf::from(p), size)),
                );
            }
        }
        out
    }

    /// Whether `path` names something in the current namespace.
    pub fn exists(&self, path: &Path) -> bool {
        self.sh.lock().k.ns.lookup_abs(path).is_ok()
    }

    /// The node the absolute `path` names in the current namespace (as [`PointInfo::node`] names it), if any.
    pub fn node_at(&self, path: &Path) -> Option<u64> {
        self.sh.lock().k.ns.lookup_abs(path).ok()
    }

    /// The current size cs(f) of the file at `path`, if it names a file.
    pub fn file_len(&self, path: &Path) -> Option<u64> {
        let g = self.sh.lock();
        let node = SimWorld::file_at(&g, path).ok()?;
        Some(g.k.ns.file(node).content.cs())
    }

    /// Whether a read of the file at `path` that reaches below byte `len` may return different bytes each time: a
    /// sector overlapping `[0, min(len, cs(f)))` is `poisoned` ([F15 §2.2]), so every read of it, mapped reads too,
    /// draws each sub-sector from its candidate set K afresh (FM-3.2; [F15 §6.2] N-4). A failed flush makes a sector so
    /// (FM-3.1); a successful flush (FM-3.4) and a crash (FM-3.3) leave it so; only a write to the sector ends it
    /// (FM-3.5: `dirty-over-poison` reads its cache content until a later failed flush poisons it again). `false` when
    /// `path` names no file. The answer is the state now: a flush in flight changes it if it fails, and a write in flight
    /// when it applies. Callable from the driver and from tasks.
    pub fn poisoned_below(&self, path: &Path, len: u64) -> bool {
        let g = self.sh.lock();
        SimWorld::file_at(&g, path).is_ok_and(|n| g.k.ns.file(n).content.reads_poisoned(len))
    }

    /// [`SimWorld::poisoned_below`] for the file node `node` (as [`SimFile::node`], [`SimWorld::node_at`] and
    /// [`PointInfo::node`] name it), whether or not a name still reaches it: `false` when `node` is not a file.
    pub fn poisoned_below_node(&self, node: u64, len: u64) -> bool {
        let g = self.sh.lock();
        g.k.ns
            .nodes
            .get(&node)
            .and_then(|n| n.file())
            .is_some_and(|f| f.content.reads_poisoned(len))
    }

    /// The events kept so far (none in [`TraceMode::DigestOnly`]).
    pub fn trace(&self) -> Vec<Event> {
        self.sh.lock().ch.trace.events()
    }

    /// The kept events from the `from`-th on (0-based): the part of the trace after a prefix already read.
    pub fn trace_from(&self, from: u64) -> Vec<Event> {
        self.sh.lock().ch.trace.events_from(from)
    }

    /// The slot-write capture a [`NOTE_SLOT_WRITE`] note names.
    pub(crate) fn slot_capture(&self, index: u64) -> Option<Arc<trace::SlotCapture>> {
        self.sh
            .lock()
            .ch
            .trace
            .captures
            .get(index as usize)
            .cloned()
    }

    /// The absolute paths that name node `node` (as [`PointInfo::node`] and [`SimWorld::node_at`] name it) in the
    /// current namespace: none for a node with no name (unlinked, or never named), else its one path, `/`-separated
    /// from the root.
    pub fn paths_of_node(&self, node: u64) -> Vec<std::path::PathBuf> {
        self.sh
            .lock()
            .k
            .ns
            .paths_of(node)
            .into_iter()
            .map(std::path::PathBuf::from)
            .collect()
    }

    /// `len` bytes of the cache image C(f) of the file at `path` from `offset` (fewer where the file ends first), if
    /// `path` names a file. For assertions.
    pub fn peek_range(&self, path: &Path, offset: u64, len: u64) -> Option<Vec<u8>> {
        let g = self.sh.lock();
        let node = SimWorld::file_at(&g, path).ok()?;
        let c = &g.k.ns.file(node).content;
        let mut buf = vec![0u8; len.min(c.cs().saturating_sub(offset)) as usize];
        let n = c.read_plain(offset, &mut buf);
        buf.truncate(n);
        Some(buf)
    }

    /// Whether `path` names a sealed file ([OS/fs §4.6]).
    pub fn is_sealed(&self, path: &Path) -> bool {
        let g = self.sh.lock();
        SimWorld::file_at(&g, path).is_ok_and(|n| g.k.ns.file(n).sealed)
    }

    /// The byte form of the kept events: equal for equal seeds and call sequences.
    pub fn trace_bytes(&self) -> Vec<u8> {
        self.sh.lock().ch.trace.bytes()
    }

    /// The running FNV-1a digest of every event's bytes (kept in both trace modes).
    pub fn trace_digest(&self) -> u64 {
        self.sh.lock().ch.trace.digest()
    }

    /// The number of events so far.
    pub fn event_count(&self) -> u64 {
        self.sh.lock().ch.trace.count()
    }

    /// The scheduling points passed so far (the crash points of [F15 §6.4]).
    pub fn points(&self) -> u64 {
        self.sh.lock().k.points
    }

    /// The file flushes that have failed so far, in any process or by an external actor, a death's failed outcome
    /// included (FM-3.1). Crash images carry the count ([`CrashImage::failed_flushes`]).
    pub fn failed_flushes(&self) -> u64 {
        self.sh.lock().k.failed_flushes
    }

    /// The scheduling points passed and the failed file flushes, read together (the enumerator's ledger stamps), after
    /// appending the harness note `(tag, b, c)` if given, under the same lock.
    pub(crate) fn stamp(&self, note: Option<(u64, u64, u64)>) -> (u64, u64) {
        let mut g = self.sh.lock();
        if let Some((tag, b, c)) = note {
            let task = current_task(&self.sh);
            g.ev(EventKind::Note, task, u32::MAX, tag, b, c);
        }
        (g.k.points, g.k.failed_flushes)
    }

    /// The protocol violations detected so far ([F15 §3.13]).
    pub fn violations(&self) -> Vec<Violation> {
        self.sh.lock().violations.clone()
    }

    /// The protocol violations detected so far, removed from the world (a long run drains them as it goes, so they do not
    /// accumulate).
    pub fn take_violations(&self) -> Vec<Violation> {
        core::mem::take(&mut self.sh.lock().violations)
    }

    /// The lines `fail_stop` wrote ([F19 §10.2] `durability_failure`).
    pub fn stderr_lines(&self) -> Vec<String> {
        self.sh.lock().stderr.clone()
    }

    /// The lines `fail_stop` wrote, removed from the world (as [`SimWorld::take_violations`]).
    pub fn take_stderr_lines(&self) -> Vec<String> {
        core::mem::take(&mut self.sh.lock().stderr)
    }

    /// The current boot: its sequence number (1 for the first) and identity.
    pub fn boot(&self) -> (u32, BootId) {
        let g = self.sh.lock();
        (g.k.clock.boot_seq, g.k.clock.boot_id)
    }

    /// The wait mode of every grant table of this world.
    pub fn wait_mode(&self) -> WaitMode {
        self.sh.lock().wait_mode
    }

    /// The `spawn_gc_child` requests made so far.
    pub fn take_spawn_requests(&self) -> Vec<SpawnRequest> {
        core::mem::take(&mut self.sh.lock().spawns)
    }

    /// Appends a harness note to the trace on behalf of no process ([F13 §1.4]).
    pub fn note(&self, tag: u64, b: u64, c: u64) {
        self.sh
            .lock()
            .ev(EventKind::Note, None, u32::MAX, tag, b, c);
    }
}

/// An external actor truncates (or extends) file node `node` to `len` (FM-10.1): the cut bytes show through reads in
/// flight as zeros (FM-4.1), then the size changes. Nothing happens if `node` is not a file.
pub(crate) fn truncate_node(g: &mut State, node: u64, len: u64) {
    let Some(cs) =
        g.k.ns
            .nodes
            .get(&node)
            .and_then(|n| n.file())
            .map(|f| f.content.cs())
    else {
        return;
    };
    let mut off = len;
    while off < cs {
        let n = (cs - off).min(ZEROS.len() as u64);
        external_overlap(g, node, off, &ZEROS[..n as usize]);
        off += n;
    }
    let State { ch, k, .. } = g;
    k.ns.edit(node, |c| {
        c.set_len(len, &mut |site, aux, n| {
            ch.pick(site, u32::MAX, node, aux, n)
        });
    });
    g.note_class(None, u32::MAX, node);
    g.note_path(None, u32::MAX, node);
    g.ev(EventKind::External, None, u32::MAX, node, 0, len);
}

/// An external write overlapping reads in flight may show through them (FM-4.1 applies to foreign writers).
fn external_overlap(g: &mut State, node: u64, offset: u64, data: &[u8]) {
    let reads: Vec<(u64, u64, u64)> =
        g.k.reads
            .iter()
            .filter(|r| r.node == node)
            .map(|r| (r.id, r.offset, r.len))
            .collect();
    for (id, roff, rlen) in reads {
        let alts = g.overlap_alts(node, offset, data, roff, rlen);
        if let Some(r) = g.k.reads.iter_mut().find(|r| r.id == id) {
            r.alts.extend(alts);
        }
    }
}
