//! Measurement 1 (WP-52, [60 §5.2] item 1, `60-roadmap.md:757`): the CLI commit cycle on a 64 MiB log — open, append,
//! data-only flush, close — on the real `Vfs` with the toy log's production profile, the Defender close cost, and the
//! data-only-flush floor the cycle is gated against (`60-roadmap.md:788`), in the same run ([MP §4.1], [MP §4.3]).
//! It decides whether CLI commits exceed 20 ms, the trigger of the optional leader (`ARCHITECTURE-RESEARCH.md:140`),
//! judged on p99 at tier t2 (R-24 Q9). The arms are generic over [`store::Rig`]; the composition root passes
//! `store::ToyRig` over `moirai_os::OsVfs`.
//!
//! **The quantities** (R-31 Q3; names by [MP §4.1]), in run order, each with minimum tier t2:
//!
//! | Quantity | Arms (arm 0 first) | Timed region of one sample |
//! |---|---|---|
//! | `cycle.e64mib` | `cycle`, `floor.flush` | `cycle`: open (`Toy::open` with the production profile, as a new CLI process), one commit (`Toy::run`: phase 1's replay of the generation, the append, the data-only flush, the publish, P-88's two directory flushes), close (the drop: every handle closed, Defender's scan of the written files). `floor.flush`: one `write_at` of the drawn size and one `sync(Data)` on a raw 64 MiB extent |
//! | `close.e64mib` | `close` | the close of the handle that just committed: the Defender close cost of a written `log.1`, `HEAD` and `LOCK` (G8, U10) |
//!
//! **Setup and teardown stay outside every timed region** ([MP §4.1]): the store generation's retirement and `init`
//! ([`store::Generations`]), the draw of the sizes, the counters' first reading, the floor extent's creation, and for
//! `close` the open and the commit; after the second tick, the counters check and the counting of the commit. A sample
//! with a batch of k makes the setup of all k operations before its first tick ([MP §4.5]); every operation takes
//! milliseconds, so the batch is 1 in practice.
//!
//! **The counters checks** (on the real OS, every run and smoke): a `cycle` sample makes exactly one class-`durable`
//! flush per commit and no `durable+meta` flush (the phase-2b flush, `write.rs:1186`; a commit without rotation
//! prepares no extent, `write.rs:738-763`); its two directory flushes (P-88, `store.rs:529-544`) are reported by the
//! smoke, not asserted. A `floor.flush` sample makes exactly one `durable` flush and no other.
//!
//! **Sizes** (R-31 Q2): typical commits only, `moirai_toylog::commit_size(draw, 600)`, 300–600 B (`config.rs:112-113`);
//! the size tail is measurement 2's. The draws are XXH3-seeded by `m1:<quantity>` for the fillers and by
//! `m1-floor:<quantity>` for the floor ([`store::draw64`]). The floor's extent is E = 64 MiB, zero-filled at creation; a
//! run writes at most about 55,000 × 600 B ≈ 33 MB to it, so the new extent its arm makes when one is full is never
//! needed in a run, and when made it is made in setup.
//!
//! **Generations** ([`store::Generations`]): each operation commits to the current `store.<g>`, retired after 4,096
//! commits (P06) or 4 MiB of fillers (P07); at typical sizes the commit bound binds. Every file lies below the run's
//! directory `<cwd>/m1.<pid>/`, removed on every path out of the run ([MP §9.7] "The working directory").

pub mod driver;
pub mod store;

use crate::arm::{Arm, ArmError, Ticker};
use crate::host::{CommandRunner, HostKind};
use crate::run::{Context, RunSpec, measure};
use crate::tier::Tier;
use crate::units::Unit;
use driver::{Args, EXIT_DONE, EXIT_FAILED, EXIT_USAGE};
use moirai_vfs::{Meter, SyncKind};
use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use store::{GEN_BYTES, GEN_COMMITS, Generations, Rig, draw64, seed};

/// The measurement's row number.
pub const MEASUREMENT: u32 = 1;

/// The quantities, in run order (R-31 Q3): the decisive cycle with its floor first, then the close.
pub fn quantities() -> Vec<&'static str> {
    vec!["cycle.e64mib", "close.e64mib"]
}

/// The usage text.
pub const USAGE: &str = "usage: m1 --host laptop|hosted --condition idle|loaded|synthetic [--load-log <file>]
          [--quantity cycle.e64mib|close.e64mib] --commit <sha> --toolchain <text> --out <dir>
       m1 [--host ...] [--condition ...] [--quantity <name>] --commit <sha> --toolchain <text> --smoke
       m1 --help
Measurement 1: the CLI commit cycle on a 64 MiB log with its data-only-flush floor, and the close
(docs/spec/measurement-protocol.md §9.7). Every file but the records lies below the working directory.
Exit: 0 done; 1 a run or arm failed or a record could not be written; 2 usage error.
";
/// The largest filler: typical commits only (R-31 Q2), `commit_size(draw, 600)`.
pub const FILLER_MAX: u32 = 600;
/// The floor extent's length, E = 64 MiB (P01, the production profile).
pub const FLOOR_LEN: u64 = 64 << 20;

/// A stream of filler sizes: `commit_size(draw64(seed, i), FILLER_MAX)` for i = 0, 1, ….
#[derive(Clone, Debug)]
pub struct Sizes {
    seed: u64,
    i: u64,
}

impl Sizes {
    /// The stream named `name` (`m1:<quantity>` or `m1-floor:<quantity>`).
    pub fn new(name: &str) -> Sizes {
        Sizes {
            seed: seed(name),
            i: 0,
        }
    }

    /// The next size.
    pub fn next_size(&mut self) -> u32 {
        let d = draw64(self.seed, self.i);
        self.i += 1;
        moirai_toylog::commit_size(d, FILLER_MAX)
    }
}

/// The flushes between two counter readings: (data, data+meta, directory).
fn flushes(c0: &moirai_vfs::VfsCounters, c1: &moirai_vfs::VfsCounters) -> (u64, u64, u64) {
    (
        c1.sync_data.saturating_sub(c0.sync_data),
        c1.sync_meta.saturating_sub(c0.sync_meta),
        c1.sync_dir.saturating_sub(c0.sync_dir),
    )
}

/// Arm 0 of `cycle.e64mib`: open, one commit, close.
pub struct CycleArm<'r, R: Rig, T: Ticker> {
    rig: &'r R,
    gens: Generations,
    sizes: Sizes,
    next_op: u64,
    ticker: T,
    batch: u32,
    /// The flushes of the last sample: (durable, durable+meta, directory), reported by the smoke.
    pub last: (u64, u64, u64),
}

impl<'r, R: Rig, T: Ticker> CycleArm<'r, R, T> {
    /// The arm over `rig`, committing to `gens` with fillers from `sizes`, timed by `ticker`.
    pub fn new(rig: &'r R, gens: Generations, sizes: Sizes, ticker: T) -> CycleArm<'r, R, T> {
        CycleArm {
            rig,
            gens,
            sizes,
            next_op: 1,
            ticker,
            batch: 1,
            last: (0, 0, 0),
        }
    }
}

impl<R: Rig, T: Ticker> Arm for CycleArm<'_, R, T> {
    fn name(&self) -> &str {
        "cycle"
    }

    fn unit(&self) -> Unit {
        Unit::Ns
    }

    fn sample(&mut self) -> Result<u64, ArmError> {
        let k = u64::from(self.batch);
        let (dir, _) = self.gens.ready(self.rig).map_err(ArmError)?;
        let fillers: Vec<u32> = (0..k).map(|_| self.sizes.next_size()).collect();
        let ops: Vec<u64> = (0..k).map(|j| self.next_op + j).collect();
        let c0 = self.rig.counters();
        let t0 = self.ticker.now_ns();
        for (&op, &filler) in ops.iter().zip(&fillers) {
            let mut h = self.rig.open(&dir, 0).map_err(ArmError)?;
            self.rig.commit(&mut h, op, filler).map_err(ArmError)?;
            self.rig.close(h);
        }
        let t1 = self.ticker.now_ns();
        let c1 = self.rig.counters();
        self.next_op += k;
        let (data, meta, dirs) = flushes(&c0, &c1);
        self.last = (data, meta, dirs);
        if data != k || meta != 0 {
            return Err(ArmError(format!(
                "cycle: {k} commits made {data} durable and {meta} durable+meta flushes; one durable flush each and no \
                 durable+meta flush expected (write.rs:1186)"
            )));
        }
        self.gens
            .add(k, fillers.iter().map(|&f| u64::from(f)).sum());
        Ok(t1.saturating_sub(t0) / k)
    }

    fn set_batch(&mut self, k: u32) {
        self.batch = k.max(1);
    }

    fn batch(&self) -> u32 {
        self.batch
    }
}

/// Arm 0 of `close.e64mib`: the close of the handle that committed; its open and commit are setup.
pub struct CloseArm<'r, R: Rig, T: Ticker> {
    rig: &'r R,
    gens: Generations,
    sizes: Sizes,
    next_op: u64,
    ticker: T,
    batch: u32,
}

impl<'r, R: Rig, T: Ticker> CloseArm<'r, R, T> {
    /// The arm over `rig`, committing to `gens` with fillers from `sizes`, timed by `ticker`.
    pub fn new(rig: &'r R, gens: Generations, sizes: Sizes, ticker: T) -> CloseArm<'r, R, T> {
        CloseArm {
            rig,
            gens,
            sizes,
            next_op: 1,
            ticker,
            batch: 1,
        }
    }
}

impl<R: Rig, T: Ticker> Arm for CloseArm<'_, R, T> {
    fn name(&self) -> &str {
        "close"
    }

    fn unit(&self) -> Unit {
        Unit::Ns
    }

    fn sample(&mut self) -> Result<u64, ArmError> {
        let k = u64::from(self.batch);
        let (dir, _) = self.gens.ready(self.rig).map_err(ArmError)?;
        let mut handles = Vec::with_capacity(self.batch as usize);
        let mut bytes = 0;
        for _ in 0..k {
            let filler = self.sizes.next_size();
            let mut h = self.rig.open(&dir, 0).map_err(ArmError)?;
            self.rig
                .commit(&mut h, self.next_op, filler)
                .map_err(ArmError)?;
            self.next_op += 1;
            bytes += u64::from(filler);
            handles.push(h);
        }
        let t0 = self.ticker.now_ns();
        for h in handles {
            self.rig.close(h);
        }
        let t1 = self.ticker.now_ns();
        self.gens.add(k, bytes);
        Ok(t1.saturating_sub(t0) / k)
    }

    fn set_batch(&mut self, k: u32) {
        self.batch = k.max(1);
    }

    fn batch(&self) -> u32 {
        self.batch
    }
}

/// Arm 1 of `cycle.e64mib`: one write of the drawn size and one data-only flush on a raw extent
/// (`60-roadmap.md:788`).
pub struct FloorArm<'r, R: Rig, T: Ticker> {
    rig: &'r R,
    base: PathBuf,
    len: u64,
    made: u32,
    floor: Option<R::Floor>,
    off: u64,
    sizes: Sizes,
    buf: Vec<u8>,
    ticker: T,
    batch: u32,
    /// The flushes of the last sample: (durable, durable+meta, directory), reported by the smoke.
    pub last: (u64, u64, u64),
}

impl<'r, R: Rig, T: Ticker> FloorArm<'r, R, T> {
    /// The arm over `rig`: its extents of `len` bytes are `<base>/floor.<n>/floor.1`, its sizes from `sizes`.
    pub fn new(rig: &'r R, base: &Path, len: u64, sizes: Sizes, ticker: T) -> FloorArm<'r, R, T> {
        FloorArm {
            rig,
            base: base.to_path_buf(),
            len,
            made: 0,
            floor: None,
            off: 0,
            sizes,
            buf: vec![0x5A; FILLER_MAX as usize],
            ticker,
            batch: 1,
            last: (0, 0, 0),
        }
    }

    /// The offset of the next write.
    pub fn offset(&self) -> u64 {
        self.off
    }
}

impl<R: Rig, T: Ticker> Arm for FloorArm<'_, R, T> {
    fn name(&self) -> &str {
        "floor.flush"
    }

    fn unit(&self) -> Unit {
        Unit::Ns
    }

    fn sample(&mut self) -> Result<u64, ArmError> {
        let k = u64::from(self.batch);
        let sizes: Vec<usize> = (0..k).map(|_| self.sizes.next_size() as usize).collect();
        let need: u64 = sizes.iter().map(|&s| s as u64).sum();
        if self.floor.is_none() || self.off + need > self.len {
            self.made += 1;
            let dir = self.base.join(format!("floor.{}", self.made));
            self.floor = Some(self.rig.floor_create(&dir, self.len).map_err(ArmError)?);
            self.off = 0;
        }
        let floor = self.floor.as_ref().expect("made above");
        let mut offs = Vec::with_capacity(sizes.len());
        let mut off = self.off;
        for &s in &sizes {
            offs.push(off);
            off += s as u64;
        }
        let c0 = self.rig.counters();
        let t0 = self.ticker.now_ns();
        for (&o, &s) in offs.iter().zip(&sizes) {
            self.rig.write(floor, o, &self.buf[..s]).map_err(ArmError)?;
            self.rig.sync(floor, SyncKind::Data).map_err(ArmError)?;
        }
        let t1 = self.ticker.now_ns();
        let c1 = self.rig.counters();
        self.off = off;
        let (data, meta, dirs) = flushes(&c0, &c1);
        self.last = (data, meta, dirs);
        if data != k || meta != 0 || dirs != 0 {
            return Err(ArmError(format!(
                "floor.flush: {k} writes made {data} durable, {meta} durable+meta and {dirs} directory flushes; one \
                 durable flush each and no other expected"
            )));
        }
        Ok(t1.saturating_sub(t0) / k)
    }

    fn set_batch(&mut self, k: u32) {
        self.batch = k.max(1);
    }

    fn batch(&self) -> u32 {
        self.batch
    }
}

/// A run's directory `<cwd>/m<k>.<pid>/`, removed with everything below it on every path out of the run, an unwinding
/// panic included ([MP §9.7] "The working directory").
pub struct RunDir<'r, R: Rig> {
    rig: &'r R,
    dir: PathBuf,
}

impl<'r, R: Rig> RunDir<'r, R> {
    /// Makes `<cwd>/<tag>.<pid>/`.
    pub fn make(rig: &'r R, cwd: &Path, tag: &str) -> Result<RunDir<'r, R>, String> {
        let dir = cwd.join(format!("{tag}.{}", std::process::id()));
        rig.make_dir(&dir)?;
        Ok(RunDir { rig, dir })
    }

    /// The directory.
    pub fn path(&self) -> &Path {
        &self.dir
    }
}

impl<R: Rig> Drop for RunDir<'_, R> {
    fn drop(&mut self) {
        self.rig.remove(&self.dir);
    }
}

/// What the binary passes in: the seams, the working directory and the process spawner.
pub struct System<'a, R, M, T, Ru> {
    /// The store seam.
    pub rig: &'a R,
    /// The measurement seam.
    pub meter: &'a M,
    /// The timer.
    pub ticker: T,
    /// The host-snapshot command runner.
    pub runner: Ru,
    /// Runs this driver again with the given arguments and returns its exit code.
    pub spawn: &'a mut dyn FnMut(&[OsString]) -> Result<u8, String>,
    /// The working directory: every file but the records lies below it.
    pub cwd: PathBuf,
}

/// The arms of quantity `q` over `rig` in `dir`: `[cycle, floor.flush]` or `[close]`.
fn arms<'r, R: Rig, T: Ticker + Clone + 'r>(
    rig: &'r R,
    dir: &Path,
    q: &str,
    ticker: &T,
) -> Vec<Box<dyn Arm + 'r>> {
    let gens = Generations::new(dir, GEN_COMMITS, GEN_BYTES);
    let sizes = Sizes::new(&format!("m1:{q}"));
    if q == "close.e64mib" {
        vec![Box::new(CloseArm::new(rig, gens, sizes, ticker.clone()))]
    } else {
        let floor = Sizes::new(&format!("m1-floor:{q}"));
        vec![
            Box::new(CycleArm::new(rig, gens, sizes, ticker.clone())),
            Box::new(FloorArm::new(rig, dir, FLOOR_LEN, floor, ticker.clone())),
        ]
    }
}

/// `--smoke`: every arm of each requested quantity once (`begin_block(1)`, one sample), at the production profile, with
/// the counters checks; no pilot, plan, statistics, host snapshot or record ([MP §9.7]).
fn smoke<R: Rig, T: Ticker + Clone>(
    rig: &R,
    ticker: &T,
    cwd: &Path,
    qs: &[&str],
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    fn once(arm: &mut dyn Arm) -> Result<u64, String> {
        arm.begin_block(1)
            .and_then(|()| arm.sample())
            .map_err(|e| format!("{}: {e}", arm.name()))
    }
    let flushes = |f: (u64, u64, u64)| {
        format!(
            "({} durable, {} durable+meta, {} directory flushes)",
            f.0, f.1, f.2
        )
    };
    let mut code = EXIT_DONE;
    for q in qs {
        let run = || -> Result<Vec<String>, String> {
            let dir = RunDir::make(rig, cwd, "m1")?;
            let gens = Generations::new(dir.path(), GEN_COMMITS, GEN_BYTES);
            let sizes = Sizes::new(&format!("m1:{q}"));
            let mut lines = Vec::new();
            if *q == "close.e64mib" {
                let mut close = CloseArm::new(rig, gens, sizes, ticker.clone());
                lines.push(format!("close ok {} ns", once(&mut close)?));
            } else {
                let mut cycle = CycleArm::new(rig, gens, sizes, ticker.clone());
                let ns = once(&mut cycle)?;
                lines.push(format!("cycle ok {ns} ns {}", flushes(cycle.last)));
                let floor_sizes = Sizes::new(&format!("m1-floor:{q}"));
                let mut floor =
                    FloorArm::new(rig, dir.path(), FLOOR_LEN, floor_sizes, ticker.clone());
                let ns = once(&mut floor)?;
                lines.push(format!("floor.flush ok {ns} ns {}", flushes(floor.last)));
            }
            Ok(lines)
        };
        match run() {
            Ok(lines) => {
                for l in lines {
                    let _ = writeln!(out, "m1 smoke {q} {l}");
                }
            }
            Err(e) => {
                let _ = writeln!(err, "m1 smoke {q} FAILED: {e}");
                code = EXIT_FAILED;
            }
        }
    }
    code
}

/// One recorded run of `q` in this process.
fn run_one<R: Rig, M: Meter, T: Ticker + Clone, Ru: CommandRunner>(
    sys: System<'_, R, M, T, Ru>,
    a: &Args,
    q: &str,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    let condition = match driver::condition(a) {
        Ok(c) => c,
        Err((code, e)) => {
            let _ = writeln!(err, "m1 {q}: {e}");
            return code;
        }
    };
    let rec = {
        let dir = match RunDir::make(sys.rig, &sys.cwd, "m1") {
            Ok(d) => d,
            Err(e) => {
                let _ = writeln!(err, "m1 {q}: {e}");
                return EXIT_FAILED;
            }
        };
        let mut arms = arms(sys.rig, dir.path(), q, &sys.ticker);
        let mut refs: Vec<&mut dyn Arm> =
            arms.iter_mut().map(|a| &mut **a as &mut dyn Arm).collect();
        let spec = RunSpec {
            measurement: MEASUREMENT,
            quantity: q.to_string(),
            condition,
            min_tier: Some(Tier::T2),
        };
        let mut ctx = Context {
            meter: sys.meter,
            ticker: sys.ticker.clone(),
            runner: sys.runner,
            host: a.host.unwrap_or(HostKind::Laptop),
            toolchain: a.toolchain.clone().unwrap_or_default(),
            commit: a.commit.clone().unwrap_or_default(),
        };
        let t0 = driver::now_ms();
        let r = measure(&mut ctx, &spec, &mut refs);
        let t1 = driver::now_ms();
        r.map(|mut rec| {
            driver::set_load_verdict(a, &mut rec, t0, t1, err, &format!("m1 {q}"));
            rec
        })
    };
    let rec = match rec {
        Ok(r) => r,
        Err(e) => {
            let _ = writeln!(err, "m1 {q}: {e}");
            return EXIT_FAILED;
        }
    };
    match driver::write_record(&rec, a.out.as_deref().unwrap_or(Path::new("."))) {
        Ok(p) => {
            let _ = writeln!(out, "m1 {q} recorded {}", p.display());
            EXIT_DONE
        }
        Err(e) => {
            let _ = writeln!(err, "m1 {q}: the record could not be written: {e}");
            EXIT_FAILED
        }
    }
}

/// The driver's command line ([MP §9.7]): parses `args` (without the program name), then runs the smoke, one quantity,
/// or every quantity in a child process of its own; returns the exit code.
pub fn cli<R: Rig, M: Meter, T: Ticker + Clone, Ru: CommandRunner>(
    args: &[OsString],
    sys: System<'_, R, M, T, Ru>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    let qs = quantities();
    let a = match driver::parse(args, &qs) {
        Ok(a) => a,
        Err(e) => {
            let _ = writeln!(err, "m1: {e}");
            let _ = err.write_all(USAGE.as_bytes());
            return EXIT_USAGE;
        }
    };
    if a.help {
        let _ = out.write_all(USAGE.as_bytes());
        return EXIT_DONE;
    }
    if a.smoke {
        let only: Vec<&str> = match &a.quantity {
            Some(q) => qs.iter().copied().filter(|x| x == q).collect(),
            None => qs.clone(),
        };
        return smoke(sys.rig, &sys.ticker, &sys.cwd, &only, out, err);
    }
    if let Some(q) = a.quantity.clone() {
        return run_one(sys, &a, &q, out, err);
    }
    driver::each_in_own_process(args, &qs, sys.spawn, out, err, "m1")
}

#[cfg(test)]
mod tests {
    use super::store::fake::{Ev, FakeRig, TraceTicker};
    use super::store::{GEN_BYTES, GEN_COMMITS};
    use super::*;
    use crate::host::tests::Canned;
    use crate::record::RunRecord;
    use crate::testkit::{FakeMeter, scratch_dir};
    use crate::tier::Plan;

    fn base() -> PathBuf {
        PathBuf::from("/w/m1.7")
    }

    fn cycle(rig: &FakeRig, gens: Generations) -> CycleArm<'_, FakeRig, TraceTicker> {
        CycleArm::new(rig, gens, Sizes::new("m1:cycle.e64mib"), rig.ticker())
    }

    fn gens() -> Generations {
        Generations::new(&base(), GEN_COMMITS, GEN_BYTES)
    }

    /// The trace from the first tick on.
    fn from_tick(t: &[Ev]) -> (Vec<Ev>, Vec<Ev>) {
        let i = t.iter().position(|e| *e == Ev::Tick).expect("a tick");
        (t[..i].to_vec(), t[i..].to_vec())
    }

    /// `cycle` times exactly open, commit and close of the current generation: the sample is 1,000 + 20,000 + 300 ns,
    /// and the generation's `init` lies before the first tick.
    #[test]
    fn cycle_times_open_commit_and_close_and_nothing_else() {
        let rig = FakeRig::new();
        let mut arm = cycle(&rig, gens());
        let store = base().join("store.1");
        let mut sizes = Sizes::new("m1:cycle.e64mib");
        for (i, id) in [(0u64, 1u64), (1, 2)] {
            rig.clear();
            assert_eq!(arm.sample(), Ok(21_300));
            let (setup, timed) = from_tick(&rig.trace());
            let want_setup = if i == 0 {
                vec![Ev::Init(store.clone())]
            } else {
                vec![]
            };
            assert_eq!(setup, want_setup);
            assert_eq!(
                timed,
                [
                    Ev::Tick,
                    Ev::Open {
                        id,
                        dir: store.clone(),
                        cost: 0
                    },
                    Ev::Commit {
                        id,
                        op: i + 1,
                        filler: sizes.next_size()
                    },
                    Ev::Close(id),
                    Ev::Tick,
                ]
            );
        }
        assert_eq!(rig.world.borrow().stores[&store].len(), 2);
    }

    /// `close` times the close of the handle that committed, and only it: the open and the commit lie before the
    /// first tick, and the sample is the close's 300 ns.
    #[test]
    fn close_times_the_close_of_the_handle_that_committed() {
        let rig = FakeRig::new();
        let mut arm = CloseArm::new(&rig, gens(), Sizes::new("m1:close.e64mib"), rig.ticker());
        assert_eq!(arm.sample(), Ok(300));
        let (setup, timed) = from_tick(&rig.trace());
        let store = base().join("store.1");
        assert_eq!(setup.len(), 3);
        assert_eq!(setup[0], Ev::Init(store.clone()));
        assert_eq!(
            setup[1],
            Ev::Open {
                id: 1,
                dir: store,
                cost: 0
            }
        );
        assert!(matches!(setup[2], Ev::Commit { id: 1, op: 1, .. }));
        assert_eq!(timed, [Ev::Tick, Ev::Close(1), Ev::Tick]);
        assert_eq!(arm.name(), "close");
    }

    /// `floor.flush` times one write of the drawn size and one data-only flush (750 ns); the offsets advance by the
    /// drawn sizes; the store is never touched; a full extent is replaced in setup, before the first tick.
    #[test]
    fn floor_flush_times_one_write_and_one_data_only_flush() {
        let rig = FakeRig::new();
        let mut arm = FloorArm::new(
            &rig,
            &base(),
            1_200,
            Sizes::new("m1-floor:cycle.e64mib"),
            rig.ticker(),
        );
        let mut sizes = Sizes::new("m1-floor:cycle.e64mib");
        let (f1, f2) = (base().join("floor.1"), base().join("floor.2"));
        let mut off = 0u64;
        let mut floor = f1.clone();
        for i in 0..4 {
            rig.clear();
            assert_eq!(arm.sample(), Ok(750), "sample {i}");
            let s = sizes.next_size();
            let (setup, timed) = from_tick(&rig.trace());
            if i == 0 {
                assert_eq!(setup, [Ev::FloorCreate(f1.clone(), 1_200)]);
            } else if off + u64::from(s) > 1_200 {
                floor = f2.clone();
                off = 0;
                assert_eq!(setup, [Ev::FloorCreate(f2.clone(), 1_200)], "sample {i}");
            } else {
                assert!(setup.is_empty(), "sample {i}: {setup:?}");
            }
            assert_eq!(
                timed,
                [
                    Ev::Tick,
                    Ev::Write(floor.clone(), off, s as usize),
                    Ev::Sync(floor.clone(), SyncKind::Data),
                    Ev::Tick,
                ],
                "sample {i}"
            );
            off += u64::from(s);
            assert_eq!(arm.offset(), off);
        }
        assert_eq!(floor, f2, "four sizes of 300-600 B overflow 1,200 B once");
        let w = rig.world.borrow();
        assert!(
            w.stores.is_empty() && w.handles.is_empty(),
            "no store touched"
        );
        assert_eq!(arm.name(), "floor.flush");
    }

    /// A `cycle` whose commit makes a `durable+meta` flush, or two `durable` flushes, fails the arm with the counts.
    #[test]
    fn a_cycle_that_is_not_one_data_only_flush_fails() {
        for (extra, says) in [
            ((0, 1), "1 durable and 1 durable+meta"),
            ((1, 0), "2 durable and 0"),
        ] {
            let rig = FakeRig::new();
            rig.world.borrow_mut().commit_extra = extra;
            let mut arm = cycle(&rig, gens());
            let e = arm.sample().unwrap_err();
            assert!(e.0.contains(says), "{e}");
        }
    }

    /// The fillers are `commit_size(draw64(seed("m1:<q>"), i), 600)`, each in [300, 600] (R-31 Q2), and the floor's
    /// sizes come from their own seed.
    #[test]
    fn fillers_and_floor_writes_follow_their_seeded_streams() {
        let rig = FakeRig::new();
        let mut arm = cycle(&rig, gens());
        let mut floor = FloorArm::new(
            &rig,
            &base(),
            FLOOR_LEN,
            Sizes::new("m1-floor:cycle.e64mib"),
            rig.ticker(),
        );
        let s = seed("m1:cycle.e64mib");
        let f = seed("m1-floor:cycle.e64mib");
        assert_ne!(s, f);
        for i in 0..200u64 {
            arm.sample().unwrap();
            floor.sample().unwrap();
            let want = moirai_toylog::commit_size(draw64(s, i), 600);
            let want_floor = moirai_toylog::commit_size(draw64(f, i), 600) as usize;
            assert!((300..=600).contains(&want));
            let t = rig.trace();
            let filler = t.iter().rev().find_map(|e| match e {
                Ev::Commit { filler, .. } => Some(*filler),
                _ => None,
            });
            let len = t.iter().rev().find_map(|e| match e {
                Ev::Write(_, _, len) => Some(*len),
                _ => None,
            });
            assert_eq!((filler, len), (Some(want), Some(want_floor)), "draw {i}");
            rig.clear();
        }
    }

    /// A due generation is retired and the next one `init`ed before the sample's first tick: the sample excludes
    /// `init`'s 10^9 ns.
    #[test]
    fn a_new_generation_is_made_before_the_sample_that_needs_it() {
        let rig = FakeRig::new();
        let mut arm = cycle(&rig, Generations::new(&base(), 2, GEN_BYTES));
        assert_eq!(arm.sample(), Ok(21_300));
        assert_eq!(arm.sample(), Ok(21_300));
        rig.clear();
        assert_eq!(arm.sample(), Ok(21_300));
        let (setup, timed) = from_tick(&rig.trace());
        assert_eq!(
            setup,
            [
                Ev::Init(base().join("store.2")),
                Ev::Remove(base().join("store.1"))
            ]
        );
        assert!(matches!(&timed[1], Ev::Open { dir, .. } if *dir == base().join("store.2")));
        assert!(matches!(timed[2], Ev::Commit { op: 3, .. }));
    }

    fn os(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    fn no_spawn(_: &[OsString]) -> Result<u8, String> {
        Err("no child process in this test".into())
    }

    /// The working directory of the command-line tests.
    fn cwd() -> PathBuf {
        PathBuf::from("/w")
    }

    /// Runs the command line over `rig`, in `/w`, with a ticker on the rig's clock that steps 1 ns per reading (a
    /// timer resolution of 1 ns); returns (exit, stdout, stderr).
    fn run_cli(rig: &FakeRig, args: &[&str]) -> (u8, String, String) {
        let meter = FakeMeter::new(8_000_000_000);
        let mut spawn = no_spawn;
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = cli(
            &os(args),
            System {
                rig,
                meter: &meter,
                ticker: rig.clock.with_steps(&[1]),
                runner: Canned::good(),
                spawn: &mut spawn,
                cwd: cwd(),
            },
            &mut out,
            &mut err,
        );
        (
            code,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    /// A rig whose commit and flush take 2 ms, so both arms of `cycle.e64mib` pilot in t2 (1,000 × 5).
    fn slow_rig() -> FakeRig {
        let rig = FakeRig::new();
        rig.world.borrow_mut().lat.commit = 2_000_000;
        rig.world.borrow_mut().lat.sync = 2_000_000;
        rig
    }

    const SMOKE: [&str; 9] = [
        "--host",
        "laptop",
        "--condition",
        "idle",
        "--commit",
        "c",
        "--toolchain",
        "t",
        "--smoke",
    ];

    /// `--smoke` runs every arm of each quantity once, with its flush counts, and records nothing; its run directories
    /// are removed. The values carry the stepping ticker's 1 ns per reading.
    #[test]
    fn smoke_runs_every_arm_once_and_records_nothing() {
        let rig = FakeRig::new();
        let (code, out, err) = run_cli(&rig, &SMOKE);
        assert_eq!(code, EXIT_DONE, "{err}");
        let oks: Vec<(String, String)> = out
            .lines()
            .map(|l| {
                let w: Vec<&str> = l.split(' ').collect();
                assert_eq!((w[0], w[1], w[4]), ("m1", "smoke", "ok"), "{l}");
                (w[2].to_string(), w[3].to_string())
            })
            .collect();
        let want = [
            ("cycle.e64mib", "cycle"),
            ("cycle.e64mib", "floor.flush"),
            ("close.e64mib", "close"),
        ]
        .map(|(q, a)| (q.to_string(), a.to_string()));
        assert_eq!(oks, want);
        assert!(
            out.contains("cycle ok 21301 ns (1 durable, 0 durable+meta, 0 directory flushes)"),
            "{out}"
        );
        assert!(
            out.contains("floor.flush ok 751 ns (1 durable, 0 durable+meta, 0 directory flushes)"),
            "{out}"
        );
        let t = rig.trace();
        let commits = t.iter().filter(|e| matches!(e, Ev::Commit { .. })).count();
        let writes = t.iter().filter(|e| matches!(e, Ev::Write(..))).count();
        assert_eq!((commits, writes), (2, 1), "each arm once");
        assert!(!out.contains("recorded"));
        assert!(
            rig.world.borrow().live.is_empty(),
            "the run directories are removed"
        );
        let rig = FakeRig::new();
        let mut one = SMOKE.to_vec();
        one.extend(["--quantity", "close.e64mib"]);
        let (code, out, _) = run_cli(&rig, &one);
        assert_eq!((code, out.lines().count()), (EXIT_DONE, 1), "{out}");
        assert!(out.starts_with("m1 smoke close.e64mib close ok 301 ns"));
    }

    /// A `--quantity` run writes exactly one record directly in `--out` that reads back: measurement 1, the arms `cycle`
    /// and `floor.flush`, t2's 1,000 × 5 plan, the given commit and toolchain.
    #[test]
    fn a_run_writes_one_valid_record() {
        let out_dir = scratch_dir("m1-run");
        let o = out_dir.join("measurements").join("1");
        let o = o.to_str().unwrap();
        let rig = slow_rig();
        let (code, out, err) = run_cli(
            &rig,
            &[
                "--quantity",
                "cycle.e64mib",
                "--host",
                "laptop",
                "--condition",
                "idle",
                "--commit",
                "0123abcd",
                "--toolchain",
                "1.98.1",
                "--out",
                o,
            ],
        );
        assert_eq!(code, EXIT_DONE, "{err}");
        assert!(out.contains("recorded"), "{out}");
        let files: Vec<PathBuf> = std::fs::read_dir(o)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        assert_eq!(files.len(), 1);
        let v: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&files[0]).unwrap()).unwrap();
        let rec = RunRecord::from_json(&v).unwrap();
        assert_eq!(
            (rec.measurement, rec.quantity.as_str()),
            (1, "cycle.e64mib")
        );
        let arms: Vec<&str> = rec.arms.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(arms, ["cycle", "floor.flush"]);
        assert_eq!(rec.plan, Plan::new(1_000, 5));
        assert!(
            rec.arms.iter().all(|a| a.tier == Tier::T2),
            "both arms in t2"
        );
        assert_eq!(
            (rec.commit.as_str(), rec.toolchain.as_str()),
            ("0123abcd", "1.98.1")
        );
        assert!(rec.valid(), "{:?}", rec.disqualifications());
        let _ = std::fs::remove_dir_all(&out_dir);
    }

    /// Without `--quantity` each quantity runs in a child of its own, in list order, with the same arguments plus
    /// `--quantity <q>`; the exit is the worst child's, and a child that cannot start counts as failed.
    #[test]
    fn without_quantity_each_quantity_runs_in_its_own_process() {
        let rig = FakeRig::new();
        let meter = FakeMeter::new(8_000_000_000);
        let base = os(&[
            "--host",
            "laptop",
            "--condition",
            "idle",
            "--commit",
            "c",
            "--toolchain",
            "t",
            "--out",
            "/o",
        ]);
        for (bad, expect) in [
            (None, EXIT_DONE),
            (Some(Ok(1)), EXIT_FAILED),
            (Some(Err(())), EXIT_FAILED),
        ] {
            let mut calls: Vec<Vec<OsString>> = Vec::new();
            let mut spawn = |a: &[OsString]| -> Result<u8, String> {
                calls.push(a.to_vec());
                match (calls.len(), bad) {
                    (2, Some(Ok(c))) => Ok(c),
                    (2, Some(Err(()))) => Err("cannot start".into()),
                    _ => Ok(0),
                }
            };
            let (mut out, mut err) = (Vec::new(), Vec::new());
            let code = cli(
                &base,
                System {
                    rig: &rig,
                    meter: &meter,
                    ticker: rig.clock.with_steps(&[1]),
                    runner: Canned::good(),
                    spawn: &mut spawn,
                    cwd: cwd(),
                },
                &mut out,
                &mut err,
            );
            assert_eq!(code, expect);
            assert_eq!(calls.len(), 2);
            for (c, q) in calls.iter().zip(quantities()) {
                let mut want = base.clone();
                want.extend(["--quantity".into(), q.into()]);
                assert_eq!(c, &want);
            }
        }
        assert!(rig.trace().is_empty(), "the parent touches no store");
    }

    /// Every directory a run or a smoke makes lies below the working directory, and none is left after it: the double
    /// records every directory made (`World::made`) and those that still exist (`World::live`).
    #[test]
    fn every_file_stays_below_the_working_directory_and_is_removed() {
        let out_dir = scratch_dir("m1-cwd");
        let o = out_dir.to_str().unwrap();
        let rig = slow_rig();
        let (code, _, err) = run_cli(&rig, &SMOKE);
        assert_eq!(code, EXIT_DONE, "{err}");
        for q in quantities() {
            let (code, _, err) = run_cli(
                &rig,
                &[
                    "--quantity",
                    q,
                    "--host",
                    "laptop",
                    "--condition",
                    "idle",
                    "--commit",
                    "c",
                    "--toolchain",
                    "t",
                    "--out",
                    o,
                ],
            );
            assert_eq!(code, EXIT_DONE, "{q}: {err}");
        }
        let w = rig.world.borrow();
        let run = cwd().join(format!("m1.{}", std::process::id()));
        for p in [
            run.clone(),
            run.join("store.1"),
            run.join("store.2"),
            run.join("floor.1"),
        ] {
            assert!(w.made.contains(&p), "{} made", p.display());
        }
        assert!(w.made.iter().all(|p| p.starts_with(cwd())), "{:?}", w.made);
        assert!(w.live.is_empty(), "left: {:?}", w.live);
        assert!(w.handles.is_empty(), "every handle closed");
        drop(w);
        let _ = std::fs::remove_dir_all(&out_dir);
    }

    /// The night's plan lists exactly this driver's quantities, in run order, and the driver accepts each of them with
    /// `--quantity` ([MP §4.1], R-27 Q2); the scan reads an array split over lines.
    #[test]
    fn the_plan_lists_exactly_the_driver_quantities() {
        let plan = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../xtask/measure-night.toml");
        let text = std::fs::read_to_string(plan).unwrap();
        let list = driver::plan_quantities(&text, MEASUREMENT).unwrap();
        assert_eq!(list, quantities());
        assert_eq!(list, ["cycle.e64mib", "close.e64mib"]);
        for q in &list {
            let a = driver::parse(
                &os(&[
                    "--quantity",
                    q,
                    "--commit",
                    "c",
                    "--toolchain",
                    "t",
                    "--smoke",
                ]),
                &quantities(),
            );
            assert!(a.is_ok(), "{q}: {a:?}");
        }
        let wrapped = "[[measurement]]\nn = 2 # two\nquantities = [\n  \"a.b\", # first\n  \"c\",\n]\n\
                       [[measurement]]\nn = 1\ntitle = \"x # y\"\nquantities = [\"d\"]\n";
        assert_eq!(
            driver::plan_quantities(wrapped, 2),
            Some(vec!["a.b".into(), "c".into()])
        );
        assert_eq!(driver::plan_quantities(wrapped, 1), Some(vec!["d".into()]));
        assert_eq!(driver::plan_quantities(wrapped, 3), None);
    }
}
