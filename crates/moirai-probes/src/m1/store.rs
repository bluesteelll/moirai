//! The store seam of measurements 1 and 2: [`Rig`], the operations a probe makes on the toy log and on a raw extent,
//! its real implementation [`ToyRig`] over a `Vfs`, the commit's [`commit_config`], the store [`Generations`] and the
//! seeded draws.
//!
//! Every decision of the probes (what is timed, in what order, from which input) is made in code generic over [`Rig`]
//! and tested with the test double `FakeRig`. [`ToyRig`] is a pass-through: each method is one toy or `Vfs` call, so
//! the real path is checked by the runtime invariants of every run and smoke (the counters checks of measurement 1,
//! the burst checks of measurement 2), never by a unit test: probes may not depend on `moirai-os`
//! (`xtask/roots.toml`).

use moirai_toylog::{CommitOp, Config, MAIN, NoTap, Op, ProcLocks, Tap, Toy};
use moirai_vfs::{Classification, ClassifyDepth, RelPath, RootRole, SyncKind, Vfs, VfsCounters};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// P06 `store.checkpoint.ops` at its design figure, 4,096 (`docs/spec/format/17-store-parameters.md:171`): a generation
/// retires once it holds this many commits.
pub const GEN_COMMITS: u64 = 4_096;
/// P07 `store.checkpoint.bytes` at its design figure, 4 MiB (`17-store-parameters.md:172`): a generation retires once
/// its fillers reach this many bytes. The CLI checkpoints at P13 = 2 times the figures (`:329`), so a CLI-only store's
/// tail can reach twice this; the design figure is the lower bound, and the replay a probe's open makes stays within
/// what a product open replays after a checkpoint.
pub const GEN_BYTES: u64 = 4 << 20;

/// The toy's configuration of one CLI commit: [`Config::production`] with measurement 2's injected in-lock cost
/// (`Config::in_lock_cost_ns`, 0 for measurement 1).
pub fn commit_config(cost_ns: u64) -> Config {
    Config {
        in_lock_cost_ns: cost_ns,
        ..Config::production()
    }
}

/// The seed of a stream named `name`: XXH3-64 of its bytes.
pub fn seed(name: &str) -> u64 {
    xxhash_rust::xxh3::xxh3_64(name.as_bytes())
}

/// The `index`-th uniform draw of the stream `seed`: XXH3-64 of `seed ‖ index` (little-endian).
pub fn draw64(seed: u64, index: u64) -> u64 {
    let mut b = [0u8; 16];
    b[..8].copy_from_slice(&seed.to_le_bytes());
    b[8..].copy_from_slice(&index.to_le_bytes());
    xxhash_rust::xxh3::xxh3_64(&b)
}

/// The operations a probe of measurements 1 and 2 makes. Every method takes `&self`; the errors are texts for an
/// `ArmError`.
pub trait Rig {
    /// An open store handle: one CLI process's opening of the store.
    type Handle;
    /// A reader's handle on a store.
    type Reader;
    /// A raw extent of the data-only-flush floor.
    type Floor;
    /// Creates a directory and its missing parents.
    fn make_dir(&self, path: &Path) -> Result<(), String>;
    /// `moirai_toylog::init` of a new store in `dir` (which must not exist) with [`Config::production`].
    fn init(&self, dir: &Path) -> Result<(), String>;
    /// Opens the store in `dir` as a new CLI process does: `Toy::open` with [`commit_config`]`(cost_ns)`, the rig's `Tap` and
    /// a new `ProcLocks`.
    fn open(&self, dir: &Path, cost_ns: u64) -> Result<Self::Handle, String>;
    /// One commit on `main` through the handle: operation `op`, `filler` filler bytes. A replayed result is an error.
    fn commit(&self, h: &mut Self::Handle, op: u64, filler: u32) -> Result<(), String>;
    /// Closes the handle (drops it).
    fn close(&self, h: Self::Handle);
    /// Opens a reader on the store in `dir`.
    fn reader(&self, dir: &Path) -> Result<Self::Reader, String>;
    /// The operations of every commit the reader sees now (`Toy::read`).
    fn commits(&self, r: &mut Self::Reader) -> Result<BTreeSet<u64>, String>;
    /// The process-wide `Vfs` counters.
    fn counters(&self) -> VfsCounters;
    /// Creates the directory `dir` as a store root and a zero-filled extent of `len` bytes in it, made durable, so a
    /// later flush writes only what the probe wrote.
    fn floor_create(&self, dir: &Path, len: u64) -> Result<Self::Floor, String>;
    /// Writes `bytes` at `off` of the floor's extent (class `lazy`).
    fn write(&self, f: &Self::Floor, off: u64, bytes: &[u8]) -> Result<(), String>;
    /// Flushes the floor's extent with `kind`, which the caller chooses.
    fn sync(&self, f: &Self::Floor, kind: SyncKind) -> Result<(), String>;
    /// The `Vfs` monotonic clock, comparable across the processes of one boot (`docs/spec/os/clock.md`).
    fn mono_ns(&self) -> u64;
    /// Returns once [`Rig::mono_ns`] reaches `t_ns`.
    fn wait_until(&self, t_ns: u64);
    /// Removes `dir` and everything below it, best effort.
    fn remove(&self, dir: &Path);
}

/// The real [`Rig`] over a `Vfs` (the composition root passes `moirai_os::OsVfs`, or measurement 2's lock-logging
/// wrapper over it), its commits' handles reporting to the toy `Tap` `T` (`NoTap`, or measurement 2's phase timer).
#[derive(Clone, Debug)]
pub struct ToyRig<V, T = NoTap> {
    pub(crate) vfs: V,
    pub(crate) tap: T,
}

impl<V> ToyRig<V> {
    /// The rig over `vfs`.
    pub fn new(vfs: V) -> ToyRig<V> {
        ToyRig { vfs, tap: NoTap }
    }
}

impl<V, T> ToyRig<V, T> {
    /// The rig over `vfs` whose commits' handles report to `tap`.
    pub fn with_tap(vfs: V, tap: T) -> ToyRig<V, T> {
        ToyRig { vfs, tap }
    }
}

/// The last stretch before a start time that a waiter spins through on the clock (`spin_loop`, never yielding the
/// processor) after sleeping through the rest: a sleep overshoots by up to about a millisecond even with Windows'
/// high-resolution timers, and a yield can hand the processor to another thread for a whole quantum.
const SPIN_NS: u64 = 3_000_000;

impl<V: Vfs + Clone, T: Tap> Rig for ToyRig<V, T> {
    type Handle = Toy<V, T>;
    type Reader = Toy<V>;
    type Floor = (V::Root, V::File);

    fn make_dir(&self, path: &Path) -> Result<(), String> {
        std::fs::create_dir_all(path).map_err(|e| format!("{}: {e}", path.display()))
    }

    fn init(&self, dir: &Path) -> Result<(), String> {
        moirai_toylog::init(&self.vfs, dir, &Config::production()).map_err(|e| format!("init: {e}"))
    }

    fn open(&self, dir: &Path, cost_ns: u64) -> Result<Toy<V, T>, String> {
        Toy::open(
            self.vfs.clone(),
            dir,
            commit_config(cost_ns),
            self.tap.clone(),
            ProcLocks::new(),
        )
        .map_err(|e| format!("open: {e}"))
    }

    fn commit(&self, h: &mut Toy<V, T>, op: u64, filler: u32) -> Result<(), String> {
        let done = h
            .run(&Op::Commit(CommitOp {
                op,
                digest: op,
                ref_name: MAIN,
                filler,
                ..CommitOp::default()
            }))
            .map_err(|e| format!("commit {op}: {e}"))?;
        if done.replayed {
            return Err(format!("commit {op} was a replay"));
        }
        Ok(())
    }

    fn close(&self, h: Toy<V, T>) {
        drop(h);
    }

    fn reader(&self, dir: &Path) -> Result<Toy<V>, String> {
        Toy::open(
            self.vfs.clone(),
            dir,
            Config::production(),
            NoTap,
            ProcLocks::new(),
        )
        .map_err(|e| format!("reader: {e}"))
    }

    fn commits(&self, r: &mut Toy<V>) -> Result<BTreeSet<u64>, String> {
        r.read()
            .map(|s| s.commits.keys().copied().collect())
            .map_err(|e| format!("Toy::read: {e}"))
    }

    fn counters(&self) -> VfsCounters {
        self.vfs.counters()
    }

    fn floor_create(&self, dir: &Path, len: u64) -> Result<(V::Root, V::File), String> {
        let root = self
            .vfs
            .create_root(dir, RootRole::Store)
            .map_err(|e| format!("floor: {e}"))?;
        let vol = match self.vfs.classify(&root, ClassifyDepth::Open) {
            Ok(Classification::Local(v)) => v,
            Ok(Classification::Refused(r)) => return Err(format!("floor: refused {r:?}")),
            Err(e) => return Err(format!("floor: {e}")),
        };
        let file = self
            .vfs
            .create_extent(&root, RelPath::literal("floor.1"), len, &vol)
            .map_err(|e| format!("floor: {e}"))?;
        // `create_extent` makes nothing durable: without this flush the first timed `sync(Data)` would write the whole
        // zero-filled extent (about 47 ms for 64 MiB in the first smoke), which is setup.
        self.vfs
            .sync(&file, SyncKind::DataAndMeta)
            .map_err(|e| format!("floor: {e}"))?;
        self.vfs
            .sync_dir(&root, None)
            .map_err(|e| format!("floor: {e}"))?;
        Ok((root, file))
    }

    fn write(&self, f: &(V::Root, V::File), off: u64, bytes: &[u8]) -> Result<(), String> {
        self.vfs
            .write_at(&f.1, off, bytes)
            .map_err(|e| format!("floor write: {e}"))
    }

    fn sync(&self, f: &(V::Root, V::File), kind: SyncKind) -> Result<(), String> {
        self.vfs
            .sync(&f.1, kind)
            .map_err(|e| format!("floor flush: {e}"))
    }

    fn mono_ns(&self) -> u64 {
        self.vfs.mono_ns()
    }

    fn wait_until(&self, t_ns: u64) {
        loop {
            let now = self.vfs.mono_ns();
            if now >= t_ns {
                return;
            }
            let left = t_ns - now;
            if left > SPIN_NS + SPIN_NS / 4 {
                std::thread::sleep(std::time::Duration::from_nanos(left - SPIN_NS));
            } else {
                std::hint::spin_loop();
            }
        }
    }

    fn remove(&self, dir: &Path) {
        let _ = std::fs::remove_dir_all(dir);
    }
}

/// The store generations of a run. A fresh toy handle replays its store's whole log (`Toy::read`'s refresh), so a
/// store that grows through a run drifts the open cost from repetition to repetition. A run therefore commits to
/// `<base>/store.<g>`, and before an operation [`Generations::ready`] retires the current generation once it holds
/// `max_commits` commits or `max_bytes` of fillers, `init`s `store.<g+1>` and removes the retired one: an open replays
/// a tail of 0 to `max_commits` commits and below `max_bytes` (plus the operation's own), as a product open does after
/// a checkpoint ([`GEN_COMMITS`], [`GEN_BYTES`]). The log never reaches E/2 (at most 4 MiB + 16 × P05 of fillers), so
/// no commit rotates, which the counters check of measurement 1 relies on. Every call is setup, outside a timed region.
#[derive(Clone, Debug)]
pub struct Generations {
    base: PathBuf,
    max_commits: u64,
    max_bytes: u64,
    g: u32,
    commits: u64,
    bytes: u64,
}

impl Generations {
    /// No generation yet; the first [`Generations::ready`] makes `store.1` below `base`.
    pub fn new(base: &Path, max_commits: u64, max_bytes: u64) -> Generations {
        Generations {
            base: base.to_path_buf(),
            max_commits,
            max_bytes,
            g: 0,
            commits: 0,
            bytes: 0,
        }
    }

    /// The directory of generation `g`.
    pub fn dir_of(&self, g: u32) -> PathBuf {
        self.base.join(format!("store.{g}"))
    }

    /// The current generation's directory (`store.0` before the first).
    pub fn dir(&self) -> PathBuf {
        self.dir_of(self.g)
    }

    /// Whether the next operation needs a new generation.
    pub fn due(&self) -> bool {
        self.g == 0 || self.commits >= self.max_commits || self.bytes >= self.max_bytes
    }

    /// The current generation's commit count.
    pub fn commits(&self) -> u64 {
        self.commits
    }

    /// Makes the generation the next operation commits to ready, retiring the current one when it is due; returns its
    /// directory and whether a new one was made.
    pub fn ready<R: Rig>(&mut self, rig: &R) -> Result<(PathBuf, bool), String> {
        if !self.due() {
            return Ok((self.dir(), false));
        }
        let old = (self.g > 0).then(|| self.dir());
        self.g += 1;
        self.commits = 0;
        self.bytes = 0;
        let dir = self.dir();
        rig.init(&dir)?;
        if let Some(old) = old {
            rig.remove(&old);
        }
        Ok((dir, true))
    }

    /// Counts `commits` commits of `bytes` filler bytes in the current generation.
    pub fn add(&mut self, commits: u64, bytes: u64) {
        self.commits += commits;
        self.bytes += bytes;
    }
}

/// The test double of [`Rig`] and its shared world: a trace of every call, a fake clock that each operation advances
/// by its scripted latency, the stores' committed operations, the directories made and removed, per-process counters,
/// and for measurement 2's children their own clocks and the writer and flush bytes' events.
#[cfg(test)]
pub(crate) mod fake {
    use super::Rig;
    use crate::arm::Ticker;
    use crate::m2::locks::{Byte, LockEvent};
    use crate::testkit::FakeTicker;
    use moirai_vfs::{SyncKind, VfsCounters};
    use std::cell::RefCell;
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::{Path, PathBuf};
    use std::rc::Rc;

    /// One call the double saw.
    #[derive(Clone, Debug, Eq, PartialEq)]
    pub(crate) enum Ev {
        /// A reading of a [`TraceTicker`].
        Tick,
        MakeDir(PathBuf),
        Init(PathBuf),
        Open {
            id: u64,
            dir: PathBuf,
            cost: u64,
        },
        Commit {
            id: u64,
            op: u64,
            filler: u32,
        },
        Close(u64),
        ReaderOpen(PathBuf),
        Read(PathBuf),
        FloorCreate(PathBuf, u64),
        Write(PathBuf, u64, usize),
        Sync(PathBuf, SyncKind),
        WaitUntil(u64),
        Remove(PathBuf),
        /// A reading of the world's clock by the coordinator's rig.
        Stamp(u64),
        /// Measurement 2's coordinator sent `go` to a child.
        Go {
            child: usize,
            op: u64,
        },
        /// Measurement 2's coordinator received a child's reply.
        Ack {
            child: usize,
            op: u64,
        },
    }

    /// The scripted latencies, in ns: each distinct, so a sample names the operations it timed. A commit holds the
    /// writer byte `commit` ns plus its cost (unless the world ignores the cost), then the flush byte `flush` ns.
    #[derive(Clone, Copy, Debug)]
    pub(crate) struct Lat {
        pub open: u64,
        pub commit: u64,
        pub flush: u64,
        pub close: u64,
        pub init: u64,
        pub write: u64,
        pub sync: u64,
    }

    impl Default for Lat {
        fn default() -> Lat {
            Lat {
                open: 1_000,
                commit: 20_000,
                flush: 0,
                close: 300,
                init: 1_000_000_000,
                write: 50,
                sync: 700,
            }
        }
    }

    /// What every process of a test shares: the trace, the file system and the lock bytes.
    #[derive(Debug, Default)]
    pub(crate) struct World {
        pub trace: Vec<Ev>,
        pub lat: Lat,
        /// Committed operations by store directory.
        pub stores: BTreeMap<PathBuf, BTreeSet<u64>>,
        /// Directories that exist, and every directory ever made or created.
        pub live: BTreeSet<PathBuf>,
        pub made: Vec<PathBuf>,
        /// Extra flushes each commit makes: (data, meta) beyond its one data flush.
        pub commit_extra: (u64, u64),
        pub next_id: u64,
        pub handles: BTreeMap<u64, PathBuf>,
        /// When the writer and the flush byte are next free.
        pub writer_free: u64,
        pub flush_free: u64,
        /// Operations whose commit fails, and those acknowledged without being committed.
        pub fail_ops: BTreeSet<u64>,
        pub lost_ops: BTreeSet<u64>,
        /// The commit's hold leaves out its cost (a `Config` that lost its draw).
        pub ignore_cost: bool,
        /// Operations whose commit logs no lock event.
        pub quiet_ops: BTreeSet<u64>,
        /// Keep no trace (long runs).
        pub untraced: bool,
    }

    /// The test double. [`FakeRig::new`] runs on the world's clock (the coordinator's, and measurement 1's); each
    /// [`FakeRig::child`] is a process of its own, with its own clock, counters and lock events.
    #[derive(Clone, Debug)]
    pub(crate) struct FakeRig {
        pub world: Rc<RefCell<World>>,
        pub clock: FakeTicker,
        pub counters: Rc<RefCell<VfsCounters>>,
        pub events: Rc<RefCell<Vec<LockEvent>>>,
        /// Log [`Ev::Stamp`] at each `mono_ns` (the coordinator's rig only).
        pub stamps: bool,
    }

    /// A ticker that logs [`Ev::Tick`] and reads the rig's clock.
    #[derive(Clone, Debug)]
    pub(crate) struct TraceTicker {
        pub world: Rc<RefCell<World>>,
        pub clock: FakeTicker,
    }

    impl Ticker for TraceTicker {
        fn now_ns(&mut self) -> u64 {
            let mut w = self.world.borrow_mut();
            if !w.untraced {
                w.trace.push(Ev::Tick);
            }
            drop(w);
            self.clock.clone().now_ns()
        }
    }

    impl FakeRig {
        pub fn new() -> FakeRig {
            FakeRig {
                world: Rc::default(),
                clock: FakeTicker::manual(),
                counters: Rc::default(),
                events: Rc::default(),
                stamps: true,
            }
        }

        /// A child process on the same world, with its own clock (at the world's time), counters and events.
        pub fn child(&self) -> FakeRig {
            let clock = FakeTicker::manual();
            clock.advance(self.now());
            FakeRig {
                world: self.world.clone(),
                clock,
                counters: Rc::default(),
                events: Rc::default(),
                stamps: false,
            }
        }

        pub fn ticker(&self) -> TraceTicker {
            TraceTicker {
                world: self.world.clone(),
                clock: self.clock.clone(),
            }
        }

        pub fn trace(&self) -> Vec<Ev> {
            self.world.borrow().trace.clone()
        }

        pub fn clear(&self) {
            self.world.borrow_mut().trace.clear();
        }

        pub fn now(&self) -> u64 {
            self.clock.clone().now_ns()
        }

        /// Moves this rig's clock forward to `t`, never back.
        pub fn catch_up(&self, t: u64) {
            let now = self.now();
            if t > now {
                self.clock.advance(t - now);
            }
        }

        /// Takes the lock events of this process.
        pub fn drain_events(&self) -> Vec<LockEvent> {
            std::mem::take(&mut *self.events.borrow_mut())
        }

        /// Logs `e` unless the world is untraced.
        pub fn note(&self, e: Ev) {
            let mut w = self.world.borrow_mut();
            if !w.untraced {
                w.trace.push(e);
            }
        }

        fn ev(&self, e: Ev) {
            self.note(e);
        }

        fn mkdir(&self, p: &Path) {
            let mut w = self.world.borrow_mut();
            w.live.insert(p.to_path_buf());
            w.made.push(p.to_path_buf());
        }
    }

    /// A handle: its id and store.
    #[derive(Debug)]
    pub(crate) struct FakeHandle {
        pub id: u64,
        pub dir: PathBuf,
        pub cost: u64,
    }

    impl Rig for FakeRig {
        type Handle = FakeHandle;
        type Reader = PathBuf;
        type Floor = PathBuf;

        fn make_dir(&self, path: &Path) -> Result<(), String> {
            self.ev(Ev::MakeDir(path.to_path_buf()));
            self.mkdir(path);
            Ok(())
        }

        fn init(&self, dir: &Path) -> Result<(), String> {
            self.ev(Ev::Init(dir.to_path_buf()));
            self.mkdir(dir);
            self.world
                .borrow_mut()
                .stores
                .insert(dir.to_path_buf(), BTreeSet::new());
            self.clock.advance(self.world.borrow().lat.init);
            Ok(())
        }

        fn open(&self, dir: &Path, cost_ns: u64) -> Result<FakeHandle, String> {
            let id = {
                let mut w = self.world.borrow_mut();
                if !w.stores.contains_key(dir) {
                    return Err(format!("open: no store {}", dir.display()));
                }
                w.next_id += 1;
                let id = w.next_id;
                w.handles.insert(id, dir.to_path_buf());
                id
            };
            self.ev(Ev::Open {
                id,
                dir: dir.to_path_buf(),
                cost: cost_ns,
            });
            self.clock.advance(self.world.borrow().lat.open);
            Ok(FakeHandle {
                id,
                dir: dir.to_path_buf(),
                cost: cost_ns,
            })
        }

        /// The append holds the writer byte `commit` ns plus the cost, from when it is free; then the flush byte `flush`
        /// ns, from when it is free; the acknowledgement follows the flush.
        fn commit(&self, h: &mut FakeHandle, op: u64, filler: u32) -> Result<(), String> {
            self.ev(Ev::Commit {
                id: h.id,
                op,
                filler,
            });
            let now = self.now();
            let (end, extra) = {
                let mut w = self.world.borrow_mut();
                if w.fail_ops.contains(&op) {
                    return Err(format!("commit {op}: scripted failure"));
                }
                if !w.lost_ops.contains(&op) {
                    w.stores.entry(h.dir.clone()).or_default().insert(op);
                }
                let cost = if w.ignore_cost { 0 } else { h.cost };
                let grant = now.max(w.writer_free);
                let rel = grant + w.lat.commit + cost;
                w.writer_free = rel;
                let fgrant = rel.max(w.flush_free);
                let frel = fgrant + w.lat.flush;
                w.flush_free = frel;
                let mut e = self.events.borrow_mut();
                e.push(LockEvent::Grant {
                    byte: Byte::Writer,
                    req: now,
                    grant,
                });
                e.push(LockEvent::Release {
                    byte: Byte::Writer,
                    t: rel,
                });
                e.push(LockEvent::Grant {
                    byte: Byte::Flush,
                    req: rel,
                    grant: fgrant,
                });
                e.push(LockEvent::Release {
                    byte: Byte::Flush,
                    t: frel,
                });
                if w.quiet_ops.contains(&op) {
                    e.clear();
                }
                (frel, w.commit_extra)
            };
            self.catch_up(end);
            let mut c = self.counters.borrow_mut();
            c.sync_data += 1 + extra.0;
            c.sync_meta += extra.1;
            c.bytes_written += u64::from(filler);
            Ok(())
        }

        fn close(&self, h: FakeHandle) {
            self.ev(Ev::Close(h.id));
            self.world.borrow_mut().handles.remove(&h.id);
            self.clock.advance(self.world.borrow().lat.close);
        }

        fn reader(&self, dir: &Path) -> Result<PathBuf, String> {
            self.ev(Ev::ReaderOpen(dir.to_path_buf()));
            Ok(dir.to_path_buf())
        }

        fn commits(&self, r: &mut PathBuf) -> Result<BTreeSet<u64>, String> {
            self.ev(Ev::Read(r.clone()));
            Ok(self
                .world
                .borrow()
                .stores
                .get(r)
                .cloned()
                .unwrap_or_default())
        }

        fn counters(&self) -> VfsCounters {
            *self.counters.borrow()
        }

        fn floor_create(&self, dir: &Path, len: u64) -> Result<PathBuf, String> {
            self.ev(Ev::FloorCreate(dir.to_path_buf(), len));
            self.mkdir(dir);
            Ok(dir.to_path_buf())
        }

        fn write(&self, f: &PathBuf, off: u64, bytes: &[u8]) -> Result<(), String> {
            self.ev(Ev::Write(f.clone(), off, bytes.len()));
            self.clock.advance(self.world.borrow().lat.write);
            self.counters.borrow_mut().bytes_written += bytes.len() as u64;
            Ok(())
        }

        fn sync(&self, f: &PathBuf, kind: SyncKind) -> Result<(), String> {
            self.ev(Ev::Sync(f.clone(), kind));
            self.clock.advance(self.world.borrow().lat.sync);
            let mut c = self.counters.borrow_mut();
            match kind {
                SyncKind::Data => c.sync_data += 1,
                SyncKind::DataAndMeta => c.sync_meta += 1,
            }
            Ok(())
        }

        fn mono_ns(&self) -> u64 {
            let t = self.now();
            if self.stamps {
                self.ev(Ev::Stamp(t));
            }
            t
        }

        fn wait_until(&self, t_ns: u64) {
            self.ev(Ev::WaitUntil(t_ns));
            self.catch_up(t_ns);
        }

        fn remove(&self, dir: &Path) {
            self.ev(Ev::Remove(dir.to_path_buf()));
            let mut w = self.world.borrow_mut();
            w.live.retain(|p| !p.starts_with(dir));
            w.stores.retain(|p, _| !p.starts_with(dir));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::{Ev, FakeRig};
    use super::*;

    /// A commit's configuration is the production profile with the draw as its in-lock cost, and nothing else changed.
    #[test]
    fn commit_config_is_the_production_profile_with_its_cost() {
        let c = commit_config(1_234);
        assert_eq!(c.in_lock_cost_ns, 1_234);
        assert_eq!(
            Config {
                in_lock_cost_ns: 0,
                ..c
            },
            Config::production()
        );
        assert_eq!(commit_config(0), Config::production());
    }

    /// 4,096 commits, or 4 MiB of fillers, make the next operation retire the generation: `store.<g+1>` is made and the
    /// retired one removed; below both bounds the generation is kept.
    #[test]
    fn generations_retire_at_the_commit_or_byte_bound() {
        let rig = FakeRig::new();
        let base = PathBuf::from("/w/m1.7");
        let mut g = Generations::new(&base, GEN_COMMITS, GEN_BYTES);
        assert!(g.due());
        assert_eq!(g.ready(&rig).unwrap(), (base.join("store.1"), true));
        g.add(GEN_COMMITS - 1, GEN_BYTES - 1);
        assert_eq!(g.ready(&rig).unwrap(), (base.join("store.1"), false));
        g.add(1, 0);
        assert_eq!(g.ready(&rig).unwrap(), (base.join("store.2"), true));
        g.add(1, GEN_BYTES - 1);
        assert_eq!(g.ready(&rig).unwrap(), (base.join("store.2"), false));
        g.add(0, 1);
        assert_eq!(g.ready(&rig).unwrap(), (base.join("store.3"), true));
        assert_eq!(g.commits(), 0);
        assert_eq!(
            rig.trace(),
            [
                Ev::Init(base.join("store.1")),
                Ev::Init(base.join("store.2")),
                Ev::Remove(base.join("store.1")),
                Ev::Init(base.join("store.3")),
                Ev::Remove(base.join("store.2")),
            ]
        );
        assert_eq!((GEN_COMMITS, GEN_BYTES), (4_096, 4 * 1_024 * 1_024));
    }
}
