//! Measurement 2 (WP-52, [60 §5.2] item 2, `60-roadmap.md:758`): the group-commit protocol under contention. 16 writer
//! processes each make one CLI commit per burst through the toy log on the real `Vfs` — the writer and flush bytes on
//! the overlapped lock (`LockFileEx`) — with an in-lock cost drawn per commit from the hold budget (R-24 Q2, R-31 Q1).
//! It decides the M1 leader: out only if the last acknowledgement's p99 stays ≤ 50 ms at the maximum in-lock cost
//! (`60-roadmap.md:758`, `ARCHITECTURE-RESEARCH.md:1947`; a 25 ms guard on the idle run), judged at tier t2 (R-24 Q9).
//! Its writer wait is T2's value for measurement 14, which builds no second driver (R-24 Q7).
//!
//! **The burst.** The driver process is the coordinator; it keeps a pool of 16 child writers (`m2 --writer`, the same
//! executable) for the whole run. Burst k:
//! 1. Setup, untimed: retire the store generation when it is due (the coordinator's reader of the old one is closed
//!    first), open the reader of the current one, and draw child i's cost (`cost::draw`, index 16k + i, the quantity's
//!    shape and scale) and filler (`commit_size(draw, 1 MiB)`, the full spec-derived distribution, which R-31 Q2 leaves
//!    to this measurement), each stream XXH3-seeded by the quantity; child i commits op 16k + i + 1.
//! 2. T = now + 5 ms (the lead).
//! 3. `go` to every child, in an order that starts at child k mod 16 and rotates by one each burst, so no child is
//!    systematically the first to hear its `go` and the values a block discards fall on every child alike.
//! 4. Each child sleeps until 3 ms before T and spins to T, stamps `t_start`, makes its CLI commit — `Toy::open` with a
//!    new `Config` carrying its draw (`Config::in_lock_cost_ns`) and a new `ProcLocks`, as a new CLI process does, then
//!    `Toy::run` — stamps `t_ack` when `run` returns (the acknowledgement), reads its counters, lock events and phase-1
//!    time, closes the handle and replies `ack`; nothing of it is open when the coordinator reads.
//! 5. The coordinator collects the 16 replies (60 s at most), then teardown, untimed: `Toy::read` on its reader.
//!
//! **The runtime checks of every burst** (every run and smoke; a failed check fails the arm, so the run fails):
//! 1. 16 acks, each for its own op.
//! 2. Every start of a measured burst lies in [T, T + 1 ms]. A late start would add its lateness to the last
//!    acknowledgement, so a burst in which a child started more than 1 ms after T is not measured but re-run (its
//!    commits still count for the generation); the arm fails once the late bursts exceed the larger of 20 and 5 % of
//!    its bursts.
//!    Every burst's latest start goes to the run's report, so the dispatch adds less than 1 ms to any value.
//! 3. Each child's append held the writer byte at least its cost: from its first writer grant to the release that
//!    follows it. The cost is spent inside that holding (`write.rs:188-200`, called at `:843`), so a commit whose
//!    `Config` lost its draw fails here as soon as a draw exceeds the append's own hold. `t_ack − t_start ≥ cost` too.
//! 4. Each ack carries a writer grant requested at or after its `t_start`: its append.
//! 5. `Toy::read` sees the 16 ops, and the generation grew by exactly 16 commits.
//! 6. The burst made at least one log data flush.
//!
//! **The measures** (per burst; every time on the system-wide monotonic clock, comparable across the processes of one
//! boot, `docs/spec/os/clock.md:42`):
//!
//! | Measure (quantity prefix) | Arm, unit | Definition |
//! |---|---|---|
//! | Last acknowledgement (`last-ack`) | `last-ack`, ns | max(`t_ack`) − T |
//! | Flushes per burst (`flushes`) | `flushes`, count | the sum of the 16 children's class-`durable` flushes: the log data flushes (`moirai-vfs/src/fs.rs:283-284`; AR:1797 "≤ 3 per 16-writer burst"); P-88's 32 directory flushes are not counted |
//! | Fairness (`fairness`) | `overtakes`, count | over each child's first writer grant of the commit (its append's), the pairs (i, j) with req_i < req_j and grant_j < grant_i: requests overtaken by a later request. 0 is arrival order, which [80 §2.2.1] item 10 says Windows roughly keeps; at most 120; ties count nothing |
//! | Flush-byte hand-off (`handoff`) | `handoff`, ns | for each flush grant g, the latest flush release r of the burst (any child) with r ≤ grant(g); when req(g) ≤ r the waiter was waiting at the release, and grant(g) − r is one hand-off; otherwise none |
//! | Writer wait (`writer-wait`, T2) | `writer-wait`, ns | grant − req of every writer grant of the burst: every acquisition `lock.writer-wait-ms` bounds, the flush holder's scan and publish re-acquisitions included (`write.rs:1073`, `:1123`) |
//!
//! `last-ack`, `flushes` and `overtakes` take one value per burst; `writer-wait` and `handoff` take every value of a
//! burst, in the burst's `go` order and then event order. An arm runs bursts until it holds its batch of values ([MP
//! §4.5]); a new block discards what an earlier block left. 100 bursts in a row without a value fail the arm.
//!
//! **The cost** (R-31 Q1 (C), `cost`): a draw per commit, `round(scale / 100 × Q(u))`, from one of two shapes, each a
//! table of anchors. `Design` (the decision's): linear from 0 to 50 µs below the median ("appends of tens of µs under
//! the writer byte", AR:1717), log-linear to 5 ms at p99, linear to 20 ms at the maximum (the hold budget, AR:1798).
//! `Uniform` (the sensitivity run): uniform from 0 to 5 ms below p99, linear to 20 ms: the maximum-entropy body. A
//! quantity is `<measure>.<shape><scale>`: shape `c` is `Design`, `a` is `Uniform`, scale NN is NN % of the draw, a
//! sweep from 0 to the hold budget (`60-roadmap.md:758`), never a constant (R-24 Q2). The decision follows `Design`;
//! `last-ack.a100` shows how far it depends on the unmeasured body. When the two disagree on "leader in", that is an
//! owner question at WP-81a (R-31).
//!
//! **Generations and the replay** (the critique's I-1). A child's commit opens a fresh handle, whose phase 1 replays the
//! generation's log from its start (`store.rs:1124-1180`), outside the writer byte but between T and `t_ack`: it is part
//! of the last acknowledgement, as a product open's replay of its tail is. A generation retires at 4,096 commits (P06)
//! or 4 MiB of fillers (P07), the design figures (`17-store-parameters.md:171-172`); a CLI-only store checkpoints at
//! P13 = 2 times them (`:329`), so the bound is the lower reading. With this measurement's sizes (a mean filler of about
//! 14 KB, about 230 KB a burst) a generation lasts about 18 bursts, and an open replays from 0 to about 4 MiB plus the
//! burst's earlier commits. The toy reads every filler byte at replay, where the product may load bodies lazily, so the
//! replay's share is reported for the results analyst: each ack carries the child's bytes read and its phase-1 time
//! (`Note::Phase1`), and the driver prints their p50, p99 and maximum after every run, into the attempt's log.
//!
//! **Pool and setup.** The arm and its coordinator are built per run; the pool is started once per process and is
//! reused by the smoke's quantities. Every file lies below the run's directory `<cwd>/m2.<pid>/`, removed on every path
//! out of the run ([MP §9.7]); a retired generation is removed in setup.

pub mod burst;
pub mod child;
pub mod cost;
pub mod locks;

use crate::arm::{Arm, ArmError, Ticker};
use crate::host::{CommandRunner, HostKind};
use crate::m1::RunDir;
use crate::m1::driver::{self, Args, EXIT_DONE, EXIT_FAILED, EXIT_USAGE};
use crate::m1::store::{GEN_BYTES, GEN_COMMITS, Generations, Rig};
use crate::run::{Context, RunSpec, measure};
use crate::tier::Tier;
use crate::units::Unit;
use burst::{Burst, Coordinator, Writers};
use child::ChildRig;
use cost::Shape;
use moirai_vfs::Meter;
use std::collections::VecDeque;
use std::ffi::OsString;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

/// The measurement's row number.
pub const MEASUREMENT: u32 = 2;
/// Bursts in a row without a value that fail an arm.
pub const DRY_BURSTS: u32 = 100;

/// A measure of the burst (module docs, "The measures").
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Measure {
    /// The last acknowledgement.
    LastAck,
    /// The log data flushes.
    Flushes,
    /// Fairness: the overtakes.
    Fairness,
    /// The flush-byte hand-offs.
    Handoff,
    /// The writer waits (T2).
    WriterWait,
}

impl Measure {
    const ALL: [Measure; 5] = [
        Measure::LastAck,
        Measure::Flushes,
        Measure::Fairness,
        Measure::Handoff,
        Measure::WriterWait,
    ];

    /// The quantity prefix.
    pub const fn prefix(self) -> &'static str {
        match self {
            Measure::LastAck => "last-ack",
            Measure::Flushes => "flushes",
            Measure::Fairness => "fairness",
            Measure::Handoff => "handoff",
            Measure::WriterWait => "writer-wait",
        }
    }

    /// The arm's name.
    pub const fn arm(self) -> &'static str {
        match self {
            Measure::Fairness => "overtakes",
            m => m.prefix(),
        }
    }

    /// The arm's unit.
    pub const fn unit(self) -> Unit {
        match self {
            Measure::Flushes | Measure::Fairness => Unit::Count,
            _ => Unit::Ns,
        }
    }

    /// The measure's values of one burst.
    pub fn values(self, b: &Burst) -> Vec<u64> {
        match self {
            Measure::LastAck => vec![burst::last_ack(b)],
            Measure::Flushes => vec![burst::flushes(b)],
            Measure::Fairness => vec![burst::overtakes(b)],
            Measure::Handoff => burst::handoffs(b),
            Measure::WriterWait => burst::writer_waits(b),
        }
    }
}

/// A quantity: its measure, cost shape and scale.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Quantity {
    /// The name, `<measure>.<shape><scale>`.
    pub name: String,
    /// The measure.
    pub measure: Measure,
    /// The cost's shape.
    pub shape: Shape,
    /// The cost's scale, in % of the draw.
    pub scale: u32,
}

/// The 12 quantities, in run order: by importance, so a measurement that runs out of budget loses the least (R-31
/// Q1 (C)).
pub fn quantities() -> Vec<&'static str> {
    vec![
        "last-ack.c100",
        "last-ack.a100",
        "writer-wait.c100",
        "flushes.c100",
        "last-ack.c0",
        "last-ack.c50",
        "fairness.c100",
        "handoff.c100",
        "writer-wait.c0",
        "flushes.c0",
        "fairness.c0",
        "handoff.c0",
    ]
}

/// The quantity a name spells, `<measure>.<shape letter><scale>`.
pub fn parse_quantity(name: &str) -> Option<Quantity> {
    let (prefix, rest) = name.split_once('.')?;
    let measure = Measure::ALL.into_iter().find(|m| m.prefix() == prefix)?;
    let mut chars = rest.chars();
    let shape = Shape::from_letter(chars.next()?)?;
    let scale: u32 = chars.as_str().parse().ok().filter(|&s| s <= 100)?;
    Some(Quantity {
        name: name.to_string(),
        measure,
        shape,
        scale,
    })
}

/// A quantity's one arm: it runs bursts until it holds its batch of values, then returns their mean (a time arm) or
/// the value (a count arm, batch 1) ([MP §4.5]); a new block discards what an earlier one left.
pub struct BurstArm<'a, R: Rig> {
    measure: Measure,
    /// The run's coordinator.
    pub coord: Coordinator<'a, R>,
    buffer: VecDeque<u64>,
    batch: u32,
    /// The last burst run.
    pub last: Option<Burst>,
}

impl<'a, R: Rig> BurstArm<'a, R> {
    /// The arm of `measure` over `coord`.
    pub fn new(measure: Measure, coord: Coordinator<'a, R>) -> BurstArm<'a, R> {
        BurstArm {
            measure,
            coord,
            buffer: VecDeque::new(),
            batch: 1,
            last: None,
        }
    }
}

impl<R: Rig> Arm for BurstArm<'_, R> {
    fn name(&self) -> &str {
        self.measure.arm()
    }

    fn unit(&self) -> Unit {
        self.measure.unit()
    }

    fn begin_block(&mut self, _len: u32) -> Result<(), ArmError> {
        self.buffer.clear();
        Ok(())
    }

    fn sample(&mut self) -> Result<u64, ArmError> {
        let k = self.batch as usize;
        let mut dry = 0;
        while self.buffer.len() < k {
            let b = self.coord.burst()?;
            let v = self.measure.values(&b);
            if v.is_empty() {
                dry += 1;
                if dry >= DRY_BURSTS {
                    return Err(ArmError(format!(
                        "{DRY_BURSTS} bursts in a row gave no {} value",
                        self.measure.arm()
                    )));
                }
            } else {
                dry = 0;
            }
            self.buffer.extend(v);
            self.last = Some(b);
        }
        let sum: u64 = self.buffer.drain(..k).sum();
        Ok(sum / k as u64)
    }

    fn set_batch(&mut self, k: u32) {
        if self.measure.unit() == Unit::Ns {
            self.batch = k.max(1);
        }
    }

    fn batch(&self) -> u32 {
        self.batch
    }
}

/// The usage text.
pub const USAGE: &str = "usage: m2 --host laptop|hosted --condition idle|loaded|synthetic [--load-log <file>]
          [--quantity <name>] --commit <sha> --toolchain <text> --out <dir>
       m2 [--host ...] [--condition ...] [--quantity <name>] --commit <sha> --toolchain <text> --smoke
       m2 --help
       m2 --writer      (a child writer: the line protocol on stdin and stdout; started by the driver)
Measurement 2: 16 child writers in bursts on the toy log; quantities <measure>.<shape><scale>, measures last-ack,
writer-wait, flushes, fairness and handoff, shape c (the design's) or a (uniform below p99), scale 0-100 % of the
in-lock cost draw (docs/spec/measurement-protocol.md §9.7). Every file but the records lies below the working
directory. Exit: 0 done; 1 a run or arm failed or a record could not be written; 2 usage error.
";

/// The pool of child writers a run starts in its working directory.
pub type Pool<'a> = &'a mut dyn FnMut(&Path) -> Result<Box<dyn Writers>, String>;

/// What the binary passes in: the seams, the child writer's rig and input, the pool, the working directory and the
/// process spawner.
pub struct System<'a, R, C, M, T, Ru> {
    /// The coordinator's store seam.
    pub rig: &'a R,
    /// A child writer's store seam (`--writer`).
    pub child: &'a C,
    /// A child writer's input (`--writer`).
    pub input: &'a mut dyn BufRead,
    /// The measurement seam.
    pub meter: &'a M,
    /// The timer.
    pub ticker: T,
    /// The host-snapshot command runner.
    pub runner: Ru,
    /// Starts the 16 child writers.
    pub pool: Pool<'a>,
    /// Runs this driver again with the given arguments and returns its exit code.
    pub spawn: &'a mut dyn FnMut(&[OsString]) -> Result<u8, String>,
    /// The working directory: every file but the records lies below it.
    pub cwd: PathBuf,
}

/// One line about the last burst of an arm: its acks, the store's growth, the flushes and the latest start.
fn burst_line<R: Rig>(arm: &BurstArm<'_, R>) -> String {
    match &arm.last {
        Some(b) => format!(
            "{} bursts; last: {} acks, Toy::read +{}, {} data flushes, latest start T+{} ns",
            arm.coord.bursts(),
            b.acks.len(),
            b.acks.len(),
            burst::flushes(b),
            b.acks.iter().map(|a| a.t_start - b.t0).max().unwrap_or(0)
        ),
        None => "no burst".into(),
    }
}

/// `--smoke`: every requested quantity's arm once (`begin_block(1)`, one sample: at least one burst) with every burst
/// check, over one pool of real child processes; no pilot, plan, statistics, host snapshot or record ([MP §9.7]).
fn smoke<R: Rig>(
    rig: &R,
    pool: Pool<'_>,
    cwd: &Path,
    qs: &[&str],
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    let mut writers = match pool(cwd) {
        Ok(w) => w,
        Err(e) => {
            let _ = writeln!(err, "m2 smoke: the writers: {e}");
            return EXIT_FAILED;
        }
    };
    let _ = writeln!(out, "m2 smoke writers {:?}", writers.pids());
    let mut code = EXIT_DONE;
    for name in qs {
        let Some(q) = parse_quantity(name) else {
            continue;
        };
        let run = |writers: &mut dyn Writers| -> Result<String, String> {
            let dir = RunDir::make(rig, cwd, "m2")?;
            let gens = Generations::new(dir.path(), GEN_COMMITS, GEN_BYTES);
            let coord = Coordinator::new(rig, writers, gens, name, q.shape, q.scale);
            let mut arm = BurstArm::new(q.measure, coord);
            let v = arm
                .begin_block(1)
                .and_then(|()| arm.sample())
                .map_err(|e| format!("{}: {e}", arm.name()))?;
            Ok(format!(
                "{} ok {v} {} ({}) {}",
                arm.name(),
                arm.unit().as_str(),
                burst_line(&arm),
                arm.coord.report.summary()
            ))
        };
        match run(writers.as_mut()) {
            Ok(line) => {
                let _ = writeln!(out, "m2 smoke {name} {line}");
            }
            Err(e) => {
                let _ = writeln!(err, "m2 smoke {name} FAILED: {e}");
                code = EXIT_FAILED;
            }
        }
    }
    code
}

/// One recorded run of `name` in this process.
fn run_one<R: Rig, C, M: Meter, T: Ticker + Clone, Ru: CommandRunner>(
    sys: System<'_, R, C, M, T, Ru>,
    a: &Args,
    name: &str,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    let condition = match driver::condition(a) {
        Ok(c) => c,
        Err((code, e)) => {
            let _ = writeln!(err, "m2 {name}: {e}");
            return code;
        }
    };
    let q = parse_quantity(name).expect("checked by parse");
    let dir = match RunDir::make(sys.rig, &sys.cwd, "m2") {
        Ok(d) => d,
        Err(e) => {
            let _ = writeln!(err, "m2 {name}: {e}");
            return EXIT_FAILED;
        }
    };
    let mut writers = match (sys.pool)(&sys.cwd) {
        Ok(w) => w,
        Err(e) => {
            let _ = writeln!(err, "m2 {name}: the writers: {e}");
            return EXIT_FAILED;
        }
    };
    let gens = Generations::new(dir.path(), GEN_COMMITS, GEN_BYTES);
    let coord = Coordinator::new(sys.rig, writers.as_mut(), gens, name, q.shape, q.scale);
    let mut arm = BurstArm::new(q.measure, coord);
    let spec = RunSpec {
        measurement: MEASUREMENT,
        quantity: name.to_string(),
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
    let r = measure(&mut ctx, &spec, &mut [&mut arm]);
    let t1 = driver::now_ms();
    let _ = writeln!(out, "m2 {name} {}", arm.coord.report.summary());
    drop(arm);
    drop(writers);
    drop(dir);
    let mut rec = match r {
        Ok(rec) => rec,
        Err(e) => {
            let _ = writeln!(err, "m2 {name}: {e}");
            return EXIT_FAILED;
        }
    };
    driver::set_load_verdict(a, &mut rec, t0, t1, err, &format!("m2 {name}"));
    match driver::write_record(&rec, a.out.as_deref().unwrap_or(Path::new("."))) {
        Ok(p) => {
            let _ = writeln!(out, "m2 {name} recorded {}", p.display());
            EXIT_DONE
        }
        Err(e) => {
            let _ = writeln!(err, "m2 {name}: the record could not be written: {e}");
            EXIT_FAILED
        }
    }
}

/// The driver's command line ([MP §9.7]): `--writer` alone serves the child protocol on the input; otherwise it parses
/// `args` (without the program name) and runs the smoke, one quantity, or every quantity in a child process of its
/// own; returns the exit code.
pub fn cli<R: Rig, C: ChildRig, M: Meter, T: Ticker + Clone, Ru: CommandRunner>(
    args: &[OsString],
    sys: System<'_, R, C, M, T, Ru>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    if args.len() == 1 && args[0] == "--writer" {
        return child::serve(sys.child, sys.input, out);
    }
    let qs = quantities();
    let a = match driver::parse(args, &qs) {
        Ok(a) => a,
        Err(e) => {
            let _ = writeln!(err, "m2: {e}");
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
        return smoke(sys.rig, sys.pool, &sys.cwd, &only, out, err);
    }
    if let Some(q) = a.quantity.clone() {
        return run_one(sys, &a, &q, out, err);
    }
    driver::each_in_own_process(args, &qs, sys.spawn, out, err, "m2")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arm::valid_name;
    use crate::host::tests::Canned;
    use crate::m1::store::fake::FakeRig;
    use crate::m2::burst::CHILDREN;
    use crate::m2::child::fake::FakeWriters;
    use crate::record::RunRecord;
    use crate::testkit::{FakeMeter, scratch_dir};
    use std::collections::BTreeSet;

    fn os(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    fn no_spawn(_: &[OsString]) -> Result<u8, String> {
        Err("no child process in this test".into())
    }

    /// Runs the command line over `rig` with 16 fake children, in `/w`, the ticker on the rig's clock stepping 1 ns
    /// per reading, and `input` as a child writer's input; returns (exit, stdout, stderr).
    fn run_cli(rig: &FakeRig, args: &[&str], input: &str) -> (u8, String, String) {
        let meter = FakeMeter::new(8_000_000_000);
        let mut spawn = no_spawn;
        let r = rig.clone();
        let mut pool = move |_: &Path| -> Result<Box<dyn Writers>, String> {
            Ok(Box::new(FakeWriters::new(&r, CHILDREN)))
        };
        let child = rig.child();
        let mut input = input.as_bytes();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = cli(
            &os(args),
            System {
                rig,
                child: &child,
                input: &mut input,
                meter: &meter,
                ticker: rig.clock.with_steps(&[1]),
                runner: Canned::good(),
                pool: &mut pool,
                spawn: &mut spawn,
                cwd: PathBuf::from("/w"),
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

    /// A coordinator's arm of `q` over 16 fake children of `rig` and `pool`.
    fn arm<'a>(rig: &'a FakeRig, pool: &'a mut FakeWriters, q: &str) -> BurstArm<'a, FakeRig> {
        let q = parse_quantity(q).unwrap();
        let gens = Generations::new(Path::new("/w/m2.1"), GEN_COMMITS, GEN_BYTES);
        let coord = Coordinator::new(rig, pool, gens, &q.name, q.shape, q.scale);
        BurstArm::new(q.measure, coord)
    }

    /// The 12 quantities are valid and unique, in the importance order of R-31; each parses back to its measure, shape
    /// and scale, `last-ack.a100` is the only `Uniform` one, and the scales sweep 0, 50 and 100.
    #[test]
    fn quantities_are_the_sweep_points_of_each_measure() {
        let qs = quantities();
        assert_eq!(qs.len(), 12);
        assert!(qs.iter().all(|q| valid_name(q)), "{qs:?}");
        assert_eq!(qs.iter().collect::<BTreeSet<_>>().len(), 12, "unique");
        assert_eq!(
            &qs[..4],
            [
                "last-ack.c100",
                "last-ack.a100",
                "writer-wait.c100",
                "flushes.c100"
            ]
        );
        let parsed: Vec<Quantity> = qs.iter().map(|q| parse_quantity(q).unwrap()).collect();
        let uniform: Vec<&str> = parsed
            .iter()
            .filter(|q| q.shape == Shape::Uniform)
            .map(|q| q.name.as_str())
            .collect();
        assert_eq!(uniform, ["last-ack.a100"]);
        for q in &parsed {
            assert_eq!(
                format!("{}.{}{}", q.measure.prefix(), q.shape.letter(), q.scale),
                q.name
            );
            assert!([0, 50, 100].contains(&q.scale));
        }
        let at = |name: &str| parsed.iter().find(|q| q.name == name).unwrap().clone();
        assert_eq!(
            at("fairness.c100"),
            Quantity {
                name: "fairness.c100".into(),
                measure: Measure::Fairness,
                shape: Shape::Design,
                scale: 100
            }
        );
        assert_eq!(Measure::Fairness.arm(), "overtakes");
        assert_eq!(at("last-ack.c50").scale, 50);
        for m in Measure::ALL {
            assert!(
                parsed.iter().any(|q| q.measure == m && q.scale == 100),
                "{m:?} at 100"
            );
            assert!(
                parsed.iter().any(|q| q.measure == m && q.scale == 0),
                "{m:?} at 0"
            );
        }
        for bad in [
            "last-ack.b100",
            "last-ack.c101",
            "lastack.c1",
            "last-ack.c",
            "last-ack",
        ] {
            assert_eq!(parse_quantity(bad), None, "{bad}");
        }
    }

    /// Each arm's sample is its own measure of the burst it ran: not the previous burst's, not another measure's.
    #[test]
    fn each_arm_takes_its_values_from_its_own_bursts() {
        for (q, measure) in [
            ("last-ack.c0", Measure::LastAck),
            ("flushes.c0", Measure::Flushes),
            ("fairness.c0", Measure::Fairness),
        ] {
            let rig = FakeRig::new();
            let mut pool = FakeWriters::new(&rig, CHILDREN);
            let mut a = arm(&rig, &mut pool, q);
            assert_eq!((a.name(), a.unit()), (measure.arm(), measure.unit()));
            let mut seen = Vec::new();
            for step in 0..3u64 {
                {
                    let mut w = rig.world.borrow_mut();
                    w.lat.commit = 20_000 + 7_000 * step;
                    w.commit_extra = (step, 0);
                }
                a.begin_block(1).unwrap();
                let v = a.sample().unwrap();
                let b = a.last.clone().unwrap();
                assert_eq!(v, measure.values(&b)[0], "{q} step {step}");
                seen.push(v);
            }
            if measure != Measure::Fairness {
                assert!(seen[0] != seen[1] && seen[1] != seen[2], "{q}: {seen:?}");
            }
            let b = a.last.clone().unwrap();
            let others: Vec<u64> = Measure::ALL
                .into_iter()
                .filter(|&m| m != measure && m != Measure::Handoff && m != Measure::WriterWait)
                .map(|m| m.values(&b)[0])
                .collect();
            assert!(
                !others.contains(&seen[2]) || measure == Measure::Fairness,
                "{q}: {seen:?} {others:?}"
            );
        }
    }

    /// A bulk arm starts each block afresh, takes the mean of its batch of consecutive values, and fails after 100
    /// bursts in a row without a value.
    #[test]
    fn bulk_arms_discard_leftovers_and_batch_on_request() {
        let rig = FakeRig::new();
        let mut pool = FakeWriters::new(&rig, CHILDREN);
        let mut a = arm(&rig, &mut pool, "writer-wait.c0");
        a.begin_block(1).unwrap();
        // In go order the children wait 0, 20,000, 40,000, … ns for the writer byte.
        assert_eq!(a.sample(), Ok(0));
        assert_eq!(a.sample(), Ok(20_000), "the same burst's next value");
        a.begin_block(1).unwrap();
        assert_eq!(
            a.sample(),
            Ok(0),
            "a new block runs a new burst, not the leftovers"
        );
        assert_eq!(a.coord.bursts(), 2);
        a.set_batch(4);
        assert_eq!(a.batch(), 4);
        a.begin_block(1).unwrap();
        assert_eq!(a.sample(), Ok((20_000 + 40_000 + 60_000) / 4));
        // Each flush holding lasts 1 ns and the next is asked 20,000 ns later: no flush grant ever waits.
        let rig = FakeRig::new();
        rig.world.borrow_mut().lat.flush = 1;
        let mut pool = FakeWriters::new(&rig, CHILDREN);
        let mut h = arm(&rig, &mut pool, "handoff.c0");
        let e = h.sample().unwrap_err();
        assert!(
            e.0.contains("100 bursts in a row gave no handoff value"),
            "{e}"
        );
        assert_eq!(h.coord.bursts(), 100);
        let rig = FakeRig::new();
        let mut pool = FakeWriters::new(&rig, CHILDREN);
        let mut f = arm(&rig, &mut pool, "flushes.c0");
        f.set_batch(4);
        assert_eq!(f.batch(), 1, "a count arm keeps batch 1");
    }

    /// `--smoke` runs one sample of every quantity's arm (at least one burst each) over one pool, prints each with the
    /// last burst's checks, and records nothing.
    #[test]
    fn smoke_runs_one_burst_per_quantity_and_records_nothing() {
        let rig = FakeRig::new();
        rig.world.borrow_mut().lat.flush = 30_000;
        let (code, out, err) = run_cli(
            &rig,
            &[
                "--host",
                "laptop",
                "--condition",
                "idle",
                "--commit",
                "c",
                "--toolchain",
                "t",
                "--smoke",
            ],
            "",
        );
        assert_eq!(code, EXIT_DONE, "{err}");
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 13, "{out}");
        assert!(lines[0].starts_with("m2 smoke writers "));
        for (l, q) in lines[1..].iter().zip(quantities()) {
            let p = parse_quantity(q).unwrap();
            let w: Vec<&str> = l.split(' ').collect();
            assert_eq!(
                (w[0], w[1], w[2], w[3], w[4]),
                ("m2", "smoke", q, p.measure.arm(), "ok"),
                "{l}"
            );
            assert!(l.contains("last: 16 acks, Toy::read +16"), "{l}");
            assert!(l.contains("latest start T+0 ns"), "{l}");
            assert!(l.contains("replay: bytes read per commit"), "{l}");
        }
        assert!(out.contains("m2 smoke last-ack.a100 last-ack ok"));
        assert!(!out.contains("recorded"));
        assert!(
            rig.world.borrow().live.is_empty(),
            "every run directory removed"
        );
    }

    /// A `--quantity` run writes one record directly in `--out` that reads back: measurement 2, `last-ack.c100`, the
    /// arm `last-ack`, t2's 1,000 × 5 plan; the run's replay report goes to stdout.
    #[test]
    fn a_run_writes_one_valid_record() {
        let out_dir = scratch_dir("m2-run");
        let o = out_dir.to_str().unwrap();
        let rig = FakeRig::new();
        {
            let mut w = rig.world.borrow_mut();
            w.untraced = true;
            w.lat.commit = 100_000;
        }
        let (code, out, err) = run_cli(
            &rig,
            &[
                "--quantity",
                "last-ack.c100",
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
            "",
        );
        assert_eq!(code, EXIT_DONE, "{err}");
        assert!(
            out.contains("m2 last-ack.c100 replay: bytes read per commit"),
            "{out}"
        );
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
            (2, "last-ack.c100")
        );
        let arms: Vec<&str> = rec.arms.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(arms, ["last-ack"]);
        assert_eq!(rec.plan, crate::tier::Plan::new(1_000, 5));
        assert!(rec.valid(), "{:?}", rec.disqualifications());
        assert!(
            rig.world.borrow().live.is_empty(),
            "the run directory is removed"
        );
        let _ = std::fs::remove_dir_all(&out_dir);
    }

    /// Without `--quantity` each quantity runs in a child of its own, in list order, with the same arguments plus
    /// `--quantity <q>`; the exit is the worst child's.
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
        let mut calls: Vec<Vec<OsString>> = Vec::new();
        let mut spawn = |a: &[OsString]| -> Result<u8, String> {
            calls.push(a.to_vec());
            Ok(if calls.len() == 5 { 1 } else { 0 })
        };
        let mut pool =
            |_: &Path| -> Result<Box<dyn Writers>, String> { Err("no pool in the parent".into()) };
        let child = rig.child();
        let mut input = "".as_bytes();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = cli(
            &base,
            System {
                rig: &rig,
                child: &child,
                input: &mut input,
                meter: &meter,
                ticker: rig.clock.with_steps(&[1]),
                runner: Canned::good(),
                pool: &mut pool,
                spawn: &mut spawn,
                cwd: PathBuf::from("/w"),
            },
            &mut out,
            &mut err,
        );
        assert_eq!(code, EXIT_FAILED);
        assert_eq!(calls.len(), 12);
        for (c, q) in calls.iter().zip(quantities()) {
            let mut want = base.clone();
            want.extend(["--quantity".into(), q.into()]);
            assert_eq!(c, &want);
        }
        assert!(rig.trace().is_empty(), "the parent touches no store");
    }

    /// `--writer` alone serves the child protocol on its input: a `go` gets its `ack`, `quit` ends with 0; `--writer`
    /// beside other arguments is a usage error.
    #[test]
    fn writer_mode_serves_the_child_protocol() {
        let rig = FakeRig::new();
        rig.init(Path::new("/w/m2.9/store.1")).unwrap();
        let go = child::Go {
            op: 4,
            start_ns: 0,
            cost_ns: 0,
            filler: 300,
            dir: PathBuf::from("/w/m2.9/store.1"),
        };
        let (code, out, err) = run_cli(&rig, &["--writer"], &format!("{}\nquit\n", go.line()));
        assert_eq!(code, 0, "{err}");
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 1, "{out}");
        assert!(lines[0].starts_with("ack 4 "), "{out}");
        assert!(matches!(
            child::Reply::parse(lines[0]),
            Ok(child::Reply::Ack(_))
        ));
        let (code, _, err) = run_cli(
            &rig,
            &["--writer", "--commit", "c", "--toolchain", "t", "--smoke"],
            "",
        );
        assert_eq!(code, EXIT_USAGE);
        assert!(err.contains("unknown argument '--writer'"), "{err}");
    }

    /// The night's plan lists exactly this driver's quantities, in run order, and the driver accepts each of them with
    /// `--quantity` ([MP §4.1], R-27 Q2).
    #[test]
    fn the_plan_lists_exactly_the_driver_quantities() {
        let plan = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../xtask/measure-night.toml");
        let text = std::fs::read_to_string(plan).unwrap();
        let list = driver::plan_quantities(&text, MEASUREMENT).unwrap();
        assert_eq!(list, quantities());
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
            assert!(parse_quantity(q).is_some(), "{q}");
        }
    }
}
