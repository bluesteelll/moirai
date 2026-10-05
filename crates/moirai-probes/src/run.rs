//! The runner: the pilot, the plan, the interleaved repetitions and the tier check ([MP §3.2–§3.3],
//! [MP §4.2–§4.6]).

use crate::arm::{Arm, ArmError, Ticker, batch_for, timer_resolution, valid_name};
use crate::condition::{Condition, MemoryWatch};
use crate::host::{CommandRunner, HostKind, HostRecord, HostSnapshot};
use crate::record::{
    ArmRecord, ProcessReadings, RepRecord, RunRecord, gate_tier, rep_duration, spec_problem,
    tier_reasons, utc_stamp,
};
use crate::stats::{Summary, median};
use crate::tier::{Plan, Tier};
use crate::units::{SEC, Unit, format_ns};
use moirai_vfs::Meter;
use std::time::SystemTime;

/// What a run measures ([MP §4.1], [MP §7.1]).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunSpec {
    /// The measurement's row number (≥ 1).
    pub measurement: u32,
    /// The quantity's name.
    pub quantity: String,
    /// The condition the driver attests.
    pub condition: Condition,
    /// The probe's declared minimum tier ([MP §3.2]), for a gated quantity the tier of its budget.
    pub min_tier: Option<Tier>,
}

/// What the runner reads and records besides the arms.
pub struct Context<'m, M: Meter, T: Ticker, R: CommandRunner> {
    /// The measurement seam ([OS/README §4.3]).
    pub meter: &'m M,
    /// The timer ([MP §4.4]).
    pub ticker: T,
    /// Runs the host-snapshot commands ([MP §2.2]).
    pub runner: R,
    /// Where the run is made.
    pub host: HostKind,
    /// The `rustc` version of the measured binaries.
    pub toolchain: String,
    /// The measured commit.
    pub commit: String,
}

/// Why a run could not be recorded at all.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProbeError {
    /// The spec or the arms break [MP §2], [MP §4.1] or [MP §4.5].
    Spec(String),
    /// The timer did not advance over the resolution readings ([MP §4.4]).
    TimerStalled,
    /// An arm failed to take a sample or a block.
    Arm {
        /// The arm's name.
        arm: String,
        /// Its error.
        error: ArmError,
    },
}

impl core::fmt::Display for ProbeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ProbeError::Spec(s) => write!(f, "invalid run: {s}"),
            ProbeError::TimerStalled => {
                f.write_str("the timer did not advance over 10,000 readings")
            }
            ProbeError::Arm { arm, error } => write!(f, "arm {arm} failed: {error}"),
        }
    }
}

impl std::error::Error for ProbeError {}

/// The most samples a pilot takes ([MP §4.2]); the pilot is announced to its arm as one block of this length.
pub const PILOT_SAMPLES: usize = 5;
/// The pilot's time limit in nanoseconds.
pub const PILOT_NS: u64 = SEC;

fn check_spec(spec: &RunSpec, host: HostKind, arms: &[&mut dyn Arm]) -> Result<(), ProbeError> {
    let bad = |s: String| Err(ProbeError::Spec(s));
    if let Some(p) = spec_problem(spec.measurement, &spec.quantity, &spec.condition, host) {
        return bad(p);
    }
    if arms.is_empty() {
        return bad("a run has at least one arm".into());
    }
    for (i, a) in arms.iter().enumerate() {
        if !valid_name(a.name()) || arms[..i].iter().any(|b| b.name() == a.name()) {
            return bad(format!("arm name '{}' is invalid or repeated", a.name()));
        }
    }
    Ok(())
}

fn arm_err(arm: &dyn Arm, error: ArmError) -> ProbeError {
    ProbeError::Arm {
        arm: arm.name().to_string(),
        error,
    }
}

/// Announces a block of `len` samples to `arm` ([MP §4.3], [MP §4.9]).
fn begin_block(arm: &mut dyn Arm, len: usize) -> Result<(), ProbeError> {
    let len = u32::try_from(len).unwrap_or(u32::MAX);
    arm.begin_block(len).map_err(|e| arm_err(arm, e))
}

/// Takes one sample and, for a bytes or counts arm, its elapsed time.
fn take<T: Ticker>(ticker: &mut T, arm: &mut dyn Arm) -> Result<(u64, u64), ProbeError> {
    if arm.unit() == Unit::Ns {
        let v = arm.sample().map_err(|e| arm_err(arm, e))?;
        Ok((v, v))
    } else {
        let t0 = ticker.now_ns();
        let v = arm.sample().map_err(|e| arm_err(arm, e))?;
        Ok((v, ticker.now_ns().saturating_sub(t0)))
    }
}

/// The pilot of one arm ([MP §4.2]): announced as one block of [`PILOT_SAMPLES`], then samples until that many are
/// taken or [`PILOT_NS`] has passed since the announcement returned, and at least one. Returns the median duration
/// per operation (time arms) or per sample (others).
fn pilot<T: Ticker>(ticker: &mut T, arm: &mut dyn Arm) -> Result<u64, ProbeError> {
    begin_block(arm, PILOT_SAMPLES)?;
    let begin = ticker.now_ns();
    let mut durations = Vec::with_capacity(PILOT_SAMPLES);
    loop {
        let (_, d) = take(ticker, arm)?;
        durations.push(d);
        if durations.len() >= PILOT_SAMPLES || ticker.now_ns().saturating_sub(begin) >= PILOT_NS {
            break;
        }
    }
    Ok(median(&mut durations).unwrap_or(0))
}

/// One repetition's raw data of one arm.
struct RepData {
    samples: Vec<u64>,
    elapsed: Vec<u64>,
}

/// The repetition's record: its summary computed once, through a sort of `scratch` (the samples keep their sampling
/// order), and for a bytes or counts arm the median of its elapsed times ([MP §3.3], [MP §3.4]).
fn summarize(
    unit: Unit,
    mut data: RepData,
    scratch: &mut Vec<u64>,
) -> Result<RepRecord, ProbeError> {
    scratch.clear();
    scratch.extend_from_slice(&data.samples);
    let summary = Summary::of(scratch)
        .ok_or_else(|| ProbeError::Spec("a repetition took no sample".into()))?;
    Ok(RepRecord {
        summary,
        median_elapsed_ns: (unit != Unit::Ns).then(|| median(&mut data.elapsed).unwrap_or(0)),
        samples: Some(data.samples),
    })
}

/// Runs every repetition of `plan` with the arms interleaved in rotated blocks, each announced to its arm
/// ([MP §4.3]), and returns each arm's repetition records.
fn repetitions<M: Meter, T: Ticker>(
    meter: &M,
    ticker: &mut T,
    condition: &Condition,
    plan: &Plan,
    arms: &mut [&mut dyn Arm],
    watch: &mut MemoryWatch,
) -> Result<Vec<Vec<RepRecord>>, ProbeError> {
    let k = arms.len();
    let n = plan.n as usize;
    let mut out: Vec<Vec<RepRecord>> = (0..k)
        .map(|_| Vec::with_capacity(plan.repetitions as usize))
        .collect();
    let mut scratch = Vec::with_capacity(n);
    for r in 0..plan.repetitions as usize {
        let mut data: Vec<RepData> = arms
            .iter()
            .map(|a| RepData {
                samples: Vec::with_capacity(n),
                elapsed: if a.unit() == Unit::Ns {
                    Vec::new()
                } else {
                    Vec::with_capacity(n)
                },
            })
            .collect();
        let mut taken = 0;
        while taken < n {
            watch.observe(condition, meter.available_physical());
            let len = (plan.block as usize).min(n - taken).max(1);
            for i in 0..k {
                let a = (r + i) % k;
                let arm = &mut *arms[a];
                begin_block(arm, len)?;
                for _ in 0..len {
                    let (v, d) = take(ticker, arm)?;
                    data[a].samples.push(v);
                    if arm.unit() != Unit::Ns {
                        data[a].elapsed.push(d);
                    }
                }
            }
            taken += len;
        }
        watch.observe(condition, meter.available_physical());
        for (a, d) in data.into_iter().enumerate() {
            out[a].push(summarize(arms[a].unit(), d, &mut scratch)?);
        }
    }
    Ok(out)
}

/// Measures one quantity under the protocol ([MP §3], [MP §4]) and returns its record. The record of a loaded run
/// still needs the driver's replay verdict ([`RunRecord::set_load_replay`]).
///
/// Steps: check the spec; take the start snapshot; measure the timer's resolution; reset the heap high-water mark,
/// so the record's is this run's ([MP §4.6]); run each arm's pilot with batch 1; set the batch of time arms below
/// 20 × resolution and refuse an arm that does not take it ([MP §4.5]); plan for the pilot tiers and the declared
/// minimum; run the repetitions; repeat with a raised plan while the tier check finds an under-sampled repetition;
/// read the probe process; take the end snapshot.
pub fn measure<M: Meter, T: Ticker, R: CommandRunner>(
    ctx: &mut Context<'_, M, T, R>,
    spec: &RunSpec,
    arms: &mut [&mut dyn Arm],
) -> Result<RunRecord, ProbeError> {
    check_spec(spec, ctx.host, arms)?;
    let started = utc_stamp(SystemTime::now());
    let start = HostSnapshot::take(&mut ctx.runner);
    let resolution = timer_resolution(&mut ctx.ticker).ok_or(ProbeError::TimerStalled)?;
    ctx.meter.reset_heap_high_water();

    let mut pilots = Vec::with_capacity(arms.len());
    for arm in arms.iter_mut() {
        let arm = &mut **arm;
        let timed = arm.unit() == Unit::Ns;
        if timed {
            arm.set_batch(1);
        }
        let d = pilot(&mut ctx.ticker, arm)?;
        if timed {
            let k = batch_for(d, resolution);
            arm.set_batch(k);
            if arm.batch() != k {
                return Err(ProbeError::Spec(format!(
                    "arm {} keeps batch {} where [MP §4.5] needs {k}: its operation takes {} and the timer's \
                     resolution is {}",
                    arm.name(),
                    arm.batch(),
                    format_ns(d),
                    format_ns(resolution)
                )));
            }
        }
        pilots.push(d);
    }
    let pilot_tiers: Vec<Tier> = pilots.iter().map(|&d| Tier::of_duration(d)).collect();
    let mut plan = Plan::covering(pilot_tiers.iter().copied().chain(spec.min_tier))
        .ok_or_else(|| ProbeError::Spec("a run has at least one arm".into()))?;

    let mut escalations = Vec::new();
    let (reps, watch) = loop {
        let mut watch = MemoryWatch::default();
        let reps = repetitions(
            ctx.meter,
            &mut ctx.ticker,
            &spec.condition,
            &plan,
            arms,
            &mut watch,
        )?;
        let need = arms
            .iter()
            .zip(&reps)
            .flat_map(|(a, rs)| {
                let unit = a.unit();
                rs.iter()
                    .map(move |rep| Tier::of_duration(rep_duration(unit, rep)))
            })
            .filter(|&t| !plan.covers(t))
            .min();
        match need {
            Some(t) => {
                escalations.push(plan);
                plan = plan.raised_to(t);
            }
            None => break (reps, watch),
        }
    };

    let process = ProcessReadings {
        private_peak: ctx.meter.private_peak().ok(),
        heap_high_water: ctx.meter.heap_counts().map(|h| h.high_water_bytes),
    };
    let end = HostSnapshot::take(&mut ctx.runner);
    let ended = utc_stamp(SystemTime::now());

    let arm_records: Vec<ArmRecord> = arms
        .iter()
        .zip(reps)
        .zip(pilots.iter().zip(&pilot_tiers))
        .map(|((a, reps), (&pilot_median, &pilot_tier))| {
            let mut rec = ArmRecord {
                name: a.name().to_string(),
                unit: a.unit(),
                batch: a.batch(),
                pilot_median,
                pilot_tier,
                tier: pilot_tier,
                reps,
            };
            rec.tier = gate_tier(&rec);
            rec
        })
        .collect();

    let mut reasons = watch.reasons(&spec.condition);
    reasons.extend(tier_reasons(&plan, &arm_records));
    Ok(RunRecord {
        measurement: spec.measurement,
        quantity: spec.quantity.clone(),
        condition: spec.condition.clone(),
        load_replay_valid: None,
        host: HostRecord {
            kind: ctx.host,
            start,
            end,
        },
        toolchain: ctx.toolchain.clone(),
        commit: ctx.commit.clone(),
        started,
        ended,
        timer_resolution_ns: resolution,
        plan,
        escalations,
        arms: arm_records,
        memory: watch,
        process,
        reasons,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arm::{BulkFn, TimedFn, ValueFn};
    use crate::condition::{IDLE_FLOOR, LOADED_TARGET};
    use crate::host::tests::Canned;
    use crate::stats::Statistic;
    use crate::testkit::{FakeMeter, FakeTicker, meter_err};
    use crate::units::MS;
    use moirai_vfs::HeapCounts;
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    fn ctx(meter: &FakeMeter, ticker: FakeTicker) -> Context<'_, FakeMeter, FakeTicker, Canned> {
        Context {
            meter,
            ticker,
            runner: Canned::good(),
            host: HostKind::Laptop,
            toolchain: "1.98.1".into(),
            commit: "0123456789ab".into(),
        }
    }

    fn spec(condition: Condition) -> RunSpec {
        RunSpec {
            measurement: 11,
            quantity: "floor.flush".into(),
            condition,
            min_tier: None,
        }
    }

    /// The record reads back unchanged, from its JSON tree and from its streamed form: the runner's records pass
    /// [`RunRecord::check`].
    fn reads_back(rec: &RunRecord) {
        assert_eq!(RunRecord::from_json(&rec.to_json()), Ok(rec.clone()));
        let mut buf = Vec::new();
        rec.write_json(&mut buf).unwrap();
        let v: serde_json::Value = serde_json::from_slice(&buf).unwrap();
        assert_eq!(RunRecord::from_json(&v), Ok(rec.clone()));
    }

    /// The clock of the arms (it moves only when an operation advances it) and the runner's handle on the same
    /// time, which also steps 1 ns per reading, so the timer's resolution is 1 ns.
    fn clocks() -> (FakeTicker, FakeTicker) {
        let arms = FakeTicker::manual();
        let runner = arms.with_steps(&[1]);
        (arms, runner)
    }

    /// A time arm whose operation takes `ns` and logs `tag`.
    fn timed(
        name: &str,
        clock: &FakeTicker,
        ns: u64,
        log: &Rc<RefCell<Vec<u8>>>,
        tag: u8,
    ) -> TimedFn<impl FnMut() -> Result<(), ArmError> + use<>, FakeTicker> {
        let (c, l) = (clock.clone(), log.clone());
        TimedFn::new(name, clock.clone(), move || {
            c.advance(ns);
            l.borrow_mut().push(tag);
            Ok(())
        })
    }

    #[test]
    fn interleaving_follows_rotated_blocks() {
        let (arms, runner) = clocks();
        let log: Rc<RefCell<Vec<u8>>> = Rc::default();
        let mut op = timed("op", &arms, 2 * SEC, &log, 0);
        let mut floor = timed("floor", &arms, 3 * SEC, &log, 1);
        let meter = FakeMeter::new(8_000_000_000);
        let mut c = ctx(&meter, runner);
        let rec = measure(&mut c, &spec(Condition::Idle), &mut [&mut op, &mut floor]).unwrap();
        assert_eq!(rec.plan, Plan::new(20, 3));
        assert!(rec.escalations.is_empty());
        let log = log.borrow();
        // Pilots: one sample each (2 s and 3 s pass the 1 s limit), then 3 repetitions of 20 rounds of one sample.
        assert_eq!(log.len(), 2 + 3 * 2 * 20);
        assert_eq!(&log[..2], &[0, 1]);
        let rep = |r: usize| &log[2 + r * 40..2 + (r + 1) * 40];
        assert!(
            rep(0).chunks(2).all(|c| c == [0, 1]),
            "repetition 0 starts with the operation"
        );
        assert!(
            rep(1).chunks(2).all(|c| c == [1, 0]),
            "repetition 1 is rotated"
        );
        assert!(rep(2).chunks(2).all(|c| c == [0, 1]));
        assert_eq!(rec.memory.readings, 3 * 21);
        assert!(
            rec.valid() && rec.exit_grade(),
            "{:?}",
            rec.disqualifications()
        );
        assert_eq!(rec.timer_resolution_ns, 1);
        assert_eq!(rec.arms[0].tier, Tier::T4);
        assert_eq!(rec.arms[0].medians().unwrap().max, 2 * SEC);
        assert_eq!(rec.arms[1].reps[2].summary.p50, 3 * SEC);
        assert_eq!(rec.process.private_peak, Some(3_000_000));
        reads_back(&rec);
    }

    #[test]
    fn blocks_hold_many_samples_for_fast_operations() {
        let (arms, runner) = clocks();
        let log: Rc<RefCell<Vec<u8>>> = Rc::default();
        let mut op = timed("op", &arms, 100 * MS, &log, 0);
        let mut floor = timed("floor", &arms, 60 * MS, &log, 1);
        let meter = FakeMeter::new(8_000_000_000);
        let mut c = ctx(&meter, runner);
        let rec = measure(&mut c, &spec(Condition::Idle), &mut [&mut op, &mut floor]).unwrap();
        assert_eq!(rec.plan, Plan::new(200, 5));
        assert_eq!(rec.plan.block, 10);
        let log = log.borrow();
        // Each pilot takes its 5 samples before the 1 s limit.
        assert_eq!(&log[..10], &[0, 0, 0, 0, 0, 1, 1, 1, 1, 1]);
        let first_rep = &log[10..10 + 400];
        assert!(
            first_rep
                .chunks(10)
                .enumerate()
                .all(|(i, c)| c.iter().all(|&x| usize::from(x) == i % 2))
        );
        assert_eq!(rec.arms[0].gated(), &[Statistic::P95, Statistic::Max]);
        assert_eq!(rec.arms[1].tier, Tier::T3);
        reads_back(&rec);
    }

    #[test]
    fn blocks_are_announced_to_the_arms() {
        let (arms, runner) = clocks();
        // A bulk arm, as a hyperfine invocation per block: each block takes len × 60 ms of wall time.
        let lens: Rc<RefCell<Vec<u32>>> = Rc::default();
        let (l, c1) = (lens.clone(), arms.clone());
        let mut spawn = BulkFn::new("spawn.empty", move |len| {
            l.borrow_mut().push(len);
            c1.advance(u64::from(len) * 60 * MS);
            Ok(vec![60 * MS; len as usize])
        });
        let log: Rc<RefCell<Vec<u8>>> = Rc::default();
        let mut floor = timed("floor", &arms, 70 * MS, &log, 1);
        let meter = FakeMeter::new(8_000_000_000);
        let rec = measure(
            &mut ctx(&meter, runner),
            &spec(Condition::Idle),
            &mut [&mut spawn, &mut floor],
        )
        .unwrap();
        assert_eq!(rec.plan, Plan::new(200, 5));
        let lens = lens.borrow();
        // The pilot is one block of 5, then 5 repetitions of 20 blocks of 10.
        assert_eq!(lens.len(), 1 + 5 * 20);
        assert_eq!(lens[0], 5);
        assert!(lens[1..].iter().all(|&l| l == 10));
        let a = &rec.arms[0];
        assert_eq!((a.pilot_median, a.batch, a.tier), (60 * MS, 1, Tier::T3));
        assert!(
            a.reps
                .iter()
                .all(|r| r.summary.n == 200 && r.summary.max == 60 * MS)
        );
        reads_back(&rec);
    }

    #[test]
    fn a_slow_bulk_pilot_keeps_its_whole_block() {
        let (arms, runner) = clocks();
        let lens: Rc<RefCell<Vec<u32>>> = Rc::default();
        let (l, c1) = (lens.clone(), arms.clone());
        // 2 s per spawn: the pilot's block of 5 takes 10 s, and the pilot still reads all 5.
        let mut spawn = BulkFn::new("spawn.slow", move |len| {
            l.borrow_mut().push(len);
            c1.advance(u64::from(len) * 2 * SEC);
            Ok((0..u64::from(len)).map(|i| 2 * SEC + i).collect())
        });
        let meter = FakeMeter::new(8_000_000_000);
        let rec = measure(
            &mut ctx(&meter, runner),
            &spec(Condition::Idle),
            &mut [&mut spawn],
        )
        .unwrap();
        assert_eq!(rec.arms[0].pilot_median, 2 * SEC + 2);
        assert_eq!(rec.plan, Plan::new(20, 3));
        assert_eq!(*lens.borrow(), [vec![5], vec![1; 60]].concat());
        reads_back(&rec);
    }

    #[test]
    fn a_faster_than_planned_operation_escalates() {
        let (arms, runner) = clocks();
        let calls = Rc::new(Cell::new(0u32));
        let (c1, n1) = (arms.clone(), calls.clone());
        // 60 ms in the pilot (t3), then 2 ms (t2): the 200-sample plan is under-sampled and is raised.
        let mut op = TimedFn::new("op", arms, move || {
            n1.set(n1.get() + 1);
            c1.advance(if n1.get() <= 5 { 60 * MS } else { 2 * MS });
            Ok(())
        });
        let meter = FakeMeter::new(8_000_000_000);
        let mut c = ctx(&meter, runner);
        let rec = measure(&mut c, &spec(Condition::Idle), &mut [&mut op]).unwrap();
        assert_eq!(rec.escalations, vec![Plan::new(200, 5)]);
        assert_eq!(rec.plan, Plan::new(1_000, 5));
        assert_eq!(rec.arms[0].pilot_tier, Tier::T3);
        assert_eq!(rec.arms[0].tier, Tier::T2);
        assert_eq!(calls.get(), 5 + 200 * 5 + 1_000 * 5);
        assert!(rec.valid());
        reads_back(&rec);
    }

    #[test]
    fn escalation_climbs_every_tier() {
        let (arms, runner) = clocks();
        let calls = Rc::new(Cell::new(0u64));
        let (c1, n1) = (arms.clone(), calls.clone());
        // Pilot 2 s (t4); the 20 × 3 attempt sees 60 ms (t3), the 200 × 5 one 2 ms (t2), and from then on 100 µs (t1).
        let mut op = TimedFn::new("op", arms, move || {
            let i = n1.get() + 1;
            n1.set(i);
            c1.advance(match i {
                1 => 2 * SEC,
                2..=61 => 60 * MS,
                62..=1_061 => 2 * MS,
                _ => MS / 10,
            });
            Ok(())
        });
        let meter = FakeMeter::new(8_000_000_000);
        let rec = measure(
            &mut ctx(&meter, runner),
            &spec(Condition::Idle),
            &mut [&mut op],
        )
        .unwrap();
        assert_eq!(
            rec.escalations,
            vec![Plan::new(20, 3), Plan::new(200, 5), Plan::new(1_000, 5)]
        );
        assert_eq!(rec.plan, Plan::new(10_000, 5));
        assert_eq!(calls.get(), 1 + 60 + 1_000 + 5_000 + 50_000);
        let a = &rec.arms[0];
        assert_eq!((a.pilot_tier, a.tier, a.batch), (Tier::T4, Tier::T1, 1));
        assert_eq!(
            rec.memory.readings,
            5 * 101,
            "only the recorded attempt's readings"
        );
        assert!(rec.valid());
        reads_back(&rec);
    }

    #[test]
    fn time_and_value_arms_share_one_plan() {
        let (arms, runner) = clocks();
        let log: Rc<RefCell<Vec<u8>>> = Rc::default();
        let mut op = timed("op", &arms, 60 * MS, &log, 0);
        let (c1, l1) = (arms.clone(), log.clone());
        let mut bytes = ValueFn::new("bytes-read", Unit::Bytes, move || {
            c1.advance(2 * SEC);
            l1.borrow_mut().push(1);
            Ok(4_096)
        });
        let meter = FakeMeter::new(8_000_000_000);
        let rec = measure(
            &mut ctx(&meter, runner),
            &spec(Condition::Idle),
            &mut [&mut op, &mut bytes],
        )
        .unwrap();
        // t3 (the operation) and t4 (the value arm's elapsed time): 200 × 5, blocks of 10.
        assert_eq!(rec.plan, Plan::new(200, 5));
        let log = log.borrow();
        // Pilots: 5 operations, then one 2 s value sample; repetition 1 starts with the value arm.
        assert_eq!(&log[..6], &[0, 0, 0, 0, 0, 1]);
        let rep1 = &log[6 + 400..6 + 800];
        assert!(
            rep1.chunks(10)
                .enumerate()
                .all(|(i, c)| c.iter().all(|&x| usize::from(x) == 1 - i % 2))
        );
        let (t, v) = (&rec.arms[0], &rec.arms[1]);
        assert_eq!(
            (t.unit, t.tier, t.reps[0].median_elapsed_ns),
            (Unit::Ns, Tier::T3, None)
        );
        assert_eq!((v.unit, v.tier, v.batch), (Unit::Bytes, Tier::T4, 1));
        // The runner's own reading around each sample adds its 1 ns step to the elapsed time.
        assert!(
            v.reps
                .iter()
                .all(|r| r.summary.n == 200 && r.median_elapsed_ns == Some(2 * SEC + 1))
        );
        assert_eq!(v.gated(), &[Statistic::Max]);
        reads_back(&rec);
    }

    #[test]
    fn declared_minimum_tier_and_value_arms() {
        let (arms, runner) = clocks();
        let samples = Rc::new(Cell::new(0u64));
        let (c1, s1) = (arms.clone(), samples.clone());
        let mut count = ValueFn::new("flushes", Unit::Count, move || {
            c1.advance(3 * SEC);
            s1.set(s1.get() + 1);
            Ok(1)
        });
        let meter = FakeMeter::new(8_000_000_000);
        let mut c = ctx(&meter, runner);
        let mut s = spec(Condition::Idle);
        s.min_tier = Some(Tier::T3);
        let rec = measure(&mut c, &s, &mut [&mut count]).unwrap();
        assert_eq!(rec.plan, Plan::new(200, 5));
        let a = &rec.arms[0];
        assert_eq!(
            (a.unit, a.batch, a.pilot_tier, a.tier),
            (Unit::Count, 1, Tier::T4, Tier::T4)
        );
        assert!(
            a.reps
                .iter()
                .all(|r| r.median_elapsed_ns.is_some_and(|e| e >= 3 * SEC))
        );
        assert_eq!(a.medians().unwrap().max, 1);
        assert_eq!(a.gated(), &[Statistic::Max]);
        assert_eq!(samples.get(), 1 + 1_000);
        reads_back(&rec);
    }

    #[test]
    fn operations_below_the_resolution_are_batched() {
        let arms = FakeTicker::manual();
        let runner = arms.with_steps(&[100]);
        let calls = Rc::new(Cell::new(0u64));
        let (c1, n1) = (arms.clone(), calls.clone());
        let mut op = TimedFn::new("op", arms, move || {
            c1.advance(150);
            n1.set(n1.get() + 1);
            Ok(())
        });
        let meter = FakeMeter::new(8_000_000_000);
        let mut c = ctx(&meter, runner);
        let rec = measure(&mut c, &spec(Condition::Idle), &mut [&mut op]).unwrap();
        assert_eq!(rec.timer_resolution_ns, 100);
        // ⌈20 × 100 / 150⌉ = 14 operations per sample; each sample is their mean, 150 ns.
        assert_eq!(rec.arms[0].batch, 14);
        assert_eq!(rec.arms[0].medians().unwrap().p99, 150);
        assert_eq!(rec.plan, Plan::new(10_000, 5));
        assert_eq!(calls.get(), 5 + 10_000 * 5 * 14);
        reads_back(&rec);
    }

    #[test]
    fn an_arm_that_cannot_batch_is_refused_below_the_resolution() {
        let arms = FakeTicker::manual();
        let runner = arms.with_steps(&[100]);
        // A bulk arm cannot batch; at 150 ns per operation [MP §4.5] needs a batch of 14.
        let mut fast = BulkFn::new("fast", |len| Ok(vec![150; len as usize]));
        let meter = FakeMeter::new(8_000_000_000);
        let e = measure(
            &mut ctx(&meter, runner),
            &spec(Condition::Idle),
            &mut [&mut fast],
        )
        .unwrap_err();
        assert!(
            matches!(&e, ProbeError::Spec(s) if s.contains("keeps batch 1 where [MP §4.5] needs 14")),
            "{e}"
        );
    }

    #[test]
    fn the_heap_high_water_mark_is_per_run() {
        let (arms, runner) = clocks();
        let log: Rc<RefCell<Vec<u8>>> = Rc::default();
        let mut op = timed("op", &arms, 2 * SEC, &log, 0);
        let mut meter = FakeMeter::new(8_000_000_000);
        meter.heap = std::sync::Mutex::new(Some(HeapCounts {
            live_bytes: 4_096,
            high_water_bytes: 900_000_000,
        }));
        let rec = measure(
            &mut ctx(&meter, runner),
            &spec(Condition::Idle),
            &mut [&mut op],
        )
        .unwrap();
        // The lifetime mark of an earlier run in the same process is reset before the pilot.
        assert_eq!(meter.resets.load(std::sync::atomic::Ordering::Relaxed), 1);
        assert_eq!(rec.process.heap_high_water, Some(4_096));
        reads_back(&rec);
    }

    #[test]
    fn conditions_are_checked_at_round_boundaries() {
        let log: Rc<RefCell<Vec<u8>>> = Rc::default();
        // Idle: one reading below 1.5 GB invalidates the run.
        let (arms, runner) = clocks();
        let mut op = timed("op", &arms, 2 * SEC, &log, 0);
        let meter = FakeMeter::scripted(vec![
            Ok(8_000_000_000),
            Ok(IDLE_FLOOR - 1),
            Ok(8_000_000_000),
        ]);
        let rec = measure(
            &mut ctx(&meter, runner),
            &spec(Condition::Idle),
            &mut [&mut op],
        )
        .unwrap();
        assert!(!rec.valid());
        assert_eq!(rec.memory.out_of_band, 1);
        assert!(
            rec.reasons[0].contains("outside at least 1.500 GB"),
            "{:?}",
            rec.reasons
        );
        reads_back(&rec);

        // Loaded within the band is valid once the driver records the replay verdict.
        let (arms, runner) = clocks();
        let mut op = timed("op", &arms, 2 * SEC, &log, 0);
        let meter = FakeMeter::new(LOADED_TARGET);
        let loaded = Condition::Loaded {
            fixture: "a1".repeat(32),
        };
        let mut rec = measure(
            &mut ctx(&meter, runner),
            &spec(loaded.clone()),
            &mut [&mut op],
        )
        .unwrap();
        assert!(rec.reasons.is_empty() && !rec.valid());
        rec.set_load_replay(true);
        assert!(rec.valid() && rec.exit_grade());
        reads_back(&rec);

        // A failed reading invalidates a loaded run, and its error reads back.
        let (arms, runner) = clocks();
        let mut op = timed("op", &arms, 2 * SEC, &log, 0);
        let meter = FakeMeter::scripted(vec![
            Ok(LOADED_TARGET),
            Err(meter_err("GlobalMemoryStatusEx")),
            Ok(LOADED_TARGET),
        ]);
        let rec = measure(&mut ctx(&meter, runner), &spec(loaded), &mut [&mut op]).unwrap();
        assert_eq!(rec.memory.failures, 1);
        assert!(!rec.reasons.is_empty());
        reads_back(&rec);

        // A synthetic run records without checking, and never decides.
        let (arms, runner) = clocks();
        let mut op = timed("op", &arms, 2 * SEC, &log, 0);
        let meter = FakeMeter::new(100);
        let s = Condition::Synthetic {
            description: "noise.yml cpu stress".into(),
        };
        let rec = measure(&mut ctx(&meter, runner), &spec(s), &mut [&mut op]).unwrap();
        assert!(rec.valid() && !rec.exit_grade());
        reads_back(&rec);
    }

    #[test]
    fn refusals() {
        let meter = FakeMeter::new(8_000_000_000);
        let (arms, runner) = clocks();
        let mut ok = TimedFn::new("op", arms.clone(), || Ok(()));
        let mut dup = TimedFn::new("op", arms.clone(), || Ok(()));
        let idle = spec(Condition::Idle);
        let e = measure(
            &mut ctx(&meter, runner.clone()),
            &idle,
            &mut [&mut ok, &mut dup],
        );
        assert!(matches!(e, Err(ProbeError::Spec(_))));
        let e = measure(&mut ctx(&meter, runner.clone()), &idle, &mut []);
        assert!(matches!(e, Err(ProbeError::Spec(_))));
        let mut s = spec(Condition::Idle);
        s.quantity = "Bad Name".into();
        let e = measure(&mut ctx(&meter, runner.clone()), &s, &mut [&mut ok]);
        assert!(matches!(e, Err(ProbeError::Spec(_))));
        let mut s = spec(Condition::Idle);
        s.measurement = 0;
        let e = measure(&mut ctx(&meter, runner.clone()), &s, &mut [&mut ok]);
        assert!(matches!(e, Err(ProbeError::Spec(_))));
        let s = spec(Condition::Loaded {
            fixture: "nope".into(),
        });
        let e = measure(&mut ctx(&meter, runner.clone()), &s, &mut [&mut ok]);
        assert!(matches!(e, Err(ProbeError::Spec(_))));
        // A hosted runner never runs the loaded condition ([MP §2.1]).
        let s = spec(Condition::Loaded {
            fixture: "0f".repeat(32),
        });
        let mut hosted = ctx(&meter, runner.clone());
        hosted.host = HostKind::Hosted;
        let e = measure(&mut hosted, &s, &mut [&mut ok]);
        assert!(
            matches!(&e, Err(ProbeError::Spec(m)) if m.contains("hosted")),
            "{e:?}"
        );
        assert!(
            hosted.runner.calls.is_empty(),
            "refused before the snapshot"
        );
        let mut failing = TimedFn::new("op", arms, || -> Result<(), ArmError> {
            Err(ArmError("disk gone".into()))
        });
        let e = measure(&mut ctx(&meter, runner.clone()), &idle, &mut [&mut failing]).unwrap_err();
        assert_eq!(e.to_string(), "arm op failed: disk gone");
        let mut no_block = BulkFn::new("spawn", |_| Err(ArmError("hyperfine missing".into())));
        let e = measure(&mut ctx(&meter, runner), &idle, &mut [&mut no_block]).unwrap_err();
        assert_eq!(e.to_string(), "arm spawn failed: hyperfine missing");
        let mut still = TimedFn::new("op", FakeTicker::manual(), || Ok(()));
        let e = measure(
            &mut ctx(&meter, FakeTicker::manual()),
            &idle,
            &mut [&mut still],
        );
        assert_eq!(e, Err(ProbeError::TimerStalled));
    }

    #[test]
    fn host_snapshots_bracket_the_run() {
        let (arms, runner) = clocks();
        let log: Rc<RefCell<Vec<u8>>> = Rc::default();
        let mut op = timed("op", &arms, 2 * SEC, &log, 0);
        let meter = FakeMeter::new(8_000_000_000);
        let mut c = ctx(&meter, runner);
        c.host = HostKind::Hosted;
        let rec = measure(&mut c, &spec(Condition::Idle), &mut [&mut op]).unwrap();
        assert_eq!(
            c.runner.calls.len(),
            4,
            "two commands at the start and two at the end"
        );
        assert!(rec.valid() && !rec.exit_grade());
        assert_eq!(rec.host.start, rec.host.end);
        reads_back(&rec);
    }
}
