//! The idle-CPU observation ([MP §4.7], [60 §5.1] "Idle CPU"): three windows of 10 minutes, each starting 15 s
//! after the observed process answered a request, each recording the process's CPU-time delta and context-switch
//! delta; the gate holds when the maximum of both over the windows is zero. It is an observation, not a
//! distribution, so it stays outside the tiered runner of [MP §3]–[MP §4.5] and has its own record,
//! `moirai-probes/idle/1` ([MP §7.1]), with the validity and exit grade of [MP §7.3]. It has no noise band; its gate
//! is absolute in nightly runs too ([MP §6]).

use crate::arm::{ArmError, Ticker};
use crate::condition::{Condition, MemoryWatch};
use crate::host::{CommandRunner, HostRecord, HostSnapshot};
use crate::record::{
    Header, OwnedHeader, array_of, spec_problem, strings_of, u64_of, utc_stamp, write_raw_file,
};
use crate::run::{Context, ProbeError};
use crate::units::{SEC, format_ns};
use moirai_vfs::Meter;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// The schema name of an idle observation ([MP §7.1]).
pub const IDLE_SCHEMA: &str = "moirai-probes/idle/1";
/// The windows of one observation ([MP §4.7]).
pub const WINDOWS: usize = 3;
/// The wait between the request's answer and the start of a window: a blocking-pool thread may wake once at its
/// keep-alive ([MP §4.7], [71 RAM-m2]).
pub const SETTLE_NS: u64 = 15 * SEC;
/// The length of one window ([MP §4.7]).
pub const WINDOW_NS: u64 = 600 * SEC;
/// The most top-up waits after a wait that the monotonic clock shows short ([MP §4.7]): the OS timer behind a
/// [`Sleeper`] is not the clock of [MP §4.4] and may wake a hair early against it, which one top-up absorbs; a clock
/// still short after these misbehaves, and a window it leaves short is a validity reason.
pub const TOP_UPS: u32 = 4;

/// What an idle observation watches ([MP §4.7], [MP §7.1]).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IdleSpec {
    /// The measurement's row number (≥ 1).
    pub measurement: u32,
    /// The quantity's name ([MP §4.1]).
    pub quantity: String,
    /// The condition the driver attests ([MP §2.3]).
    pub condition: Condition,
}

/// The process an idle observation watches, as the driver reaches it ([MP §4.7]).
pub trait IdleSubject {
    /// Sends the process one request and returns once it has answered; the settle period starts then.
    fn request(&mut self) -> Result<(), ArmError>;
    /// The process's cumulative CPU time, user plus kernel, in nanoseconds: `Meter::cpu_times` ([OS/mem §7];
    /// [`cpu_ns_of`]).
    fn cpu_ns(&mut self) -> Result<u64, ArmError>;
    /// The cumulative context switches of the process's threads, which the harness reads outside `Meter` (ETW or
    /// `typeperf` on Windows, [OS/mem §7]).
    fn context_switches(&mut self) -> Result<u64, ArmError>;
}

/// User plus kernel CPU time of `child` (or of this process) through `Meter::cpu_times` ([OS/mem §7]).
pub fn cpu_ns_of<M: Meter + ?Sized>(
    meter: &M,
    child: Option<&std::process::Child>,
) -> Result<u64, ArmError> {
    meter
        .cpu_times(child)
        .map(|t| t.user_ns.saturating_add(t.kernel_ns))
        .map_err(|e| ArmError(e.to_string()))
}

/// Waits; the observation's only source of delay, so tests need not wait 31 minutes.
pub trait Sleeper {
    /// Returns after about `ns` nanoseconds by its own timer; against the monotonic clock of [MP §4.4] it may return
    /// a little early, which [`observe_idle_cpu`] tops up ([`TOP_UPS`]).
    fn sleep_ns(&mut self, ns: u64);
}

/// [`Sleeper`] over `std::thread::sleep`, which never returns early by the OS timer it waits on (a waitable timer on
/// Windows) but may by `QueryPerformanceCounter`, the monotonic clock ([MP §4.4]).
#[derive(Clone, Copy, Debug, Default)]
pub struct ThreadSleeper;

impl Sleeper for ThreadSleeper {
    fn sleep_ns(&mut self, ns: u64) {
        std::thread::sleep(Duration::from_nanos(ns));
    }
}

/// One window of an idle observation ([MP §4.7]); each delta is its value or why it could not be read.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IdleWindow {
    /// The CPU-time delta over the window, in nanoseconds.
    pub cpu_ns: Result<u64, String>,
    /// The context-switch delta over the window.
    pub context_switches: Result<u64, String>,
    /// The window's length on the monotonic clock ([MP §4.4]).
    pub elapsed_ns: u64,
}

/// One idle observation ([MP §4.7], [MP §7.1] `moirai-probes/idle/1`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IdleRecord {
    /// The measurement's row number.
    pub measurement: u32,
    /// The quantity's name.
    pub quantity: String,
    /// The condition.
    pub condition: Condition,
    /// The replay verdict of a loaded observation ([MP §2.3]); `None` until the driver sets it.
    pub load_replay_valid: Option<bool>,
    /// The host kind and snapshots.
    pub host: HostRecord,
    /// The `rustc` version of the observed binary.
    pub toolchain: String,
    /// The observed commit.
    pub commit: String,
    /// The start, RFC 3339 UTC with whole seconds.
    pub started: String,
    /// The end, likewise.
    pub ended: String,
    /// The [`WINDOWS`] windows, in order.
    pub windows: Vec<IdleWindow>,
    /// The readings of `Meter::available_physical` at the start of every window and after the last ([MP §2.3]).
    pub memory: MemoryWatch,
    /// Why the observation is invalid; empty for a valid one ([MP §7.3]).
    pub reasons: Vec<String>,
}

/// The outcome of the idle-CPU gate on one observation ([MP §4.7], [MP §5]).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct IdleOutcome {
    /// The largest CPU-time delta, if every window was read.
    pub max_cpu_ns: Option<u64>,
    /// The largest context-switch delta, if every window was read.
    pub max_context_switches: Option<u64>,
    /// Whether both maxima are zero.
    pub holds: bool,
    /// Whether the observation may decide the gate: it is exit-grade ([MP §7.3]).
    pub decides: bool,
}

/// What a nightly run reports for an idle observation ([MP §6]): it has no noise band, so its gate is absolute.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NightlyIdle {
    /// The observation may decide and both maxima are zero.
    Holds,
    /// The observation may decide and a maximum is not zero: the nightly run reports a failure, as a regression does.
    Fails {
        /// The largest CPU-time delta, in nanoseconds.
        max_cpu_ns: u64,
        /// The largest context-switch delta.
        max_context_switches: u64,
    },
    /// The observation is not exit-grade, so it may not decide: reported with its disqualifications, as a refused
    /// comparison is ([MP §6.1]); neither a pass nor a failure.
    Undecided(Vec<String>),
}

impl NightlyIdle {
    /// Whether the nightly run reports a failure for this observation.
    pub fn fails(&self) -> bool {
        matches!(self, NightlyIdle::Fails { .. })
    }
}

/// The validity reasons of the windows ([MP §4.7], [MP §7.3]): a delta that could not be read, and a window shorter
/// than [`WINDOW_NS`].
pub fn window_reasons(windows: &[IdleWindow]) -> Vec<String> {
    let mut out = Vec::new();
    for (i, w) in windows.iter().enumerate() {
        if let Err(e) = &w.cpu_ns {
            out.push(format!(
                "idle window {i}: the CPU time could not be read: {e}"
            ));
        }
        if let Err(e) = &w.context_switches {
            out.push(format!(
                "idle window {i}: the context switches could not be read: {e}"
            ));
        }
        if w.elapsed_ns < WINDOW_NS {
            out.push(format!(
                "idle window {i} lasted {} ns, less than {}",
                w.elapsed_ns,
                format_ns(WINDOW_NS)
            ));
        }
    }
    out
}

/// Waits until `ticker` shows at least `ns` since `from` ([MP §4.7]): one wait of `ns`, then up to [`TOP_UPS`] waits
/// of the remainder while the clock shows the wait short. Returns the last reading.
fn wait_on_clock<T, Z>(ticker: &mut T, sleeper: &mut Z, from: u64, ns: u64) -> u64
where
    T: Ticker + ?Sized,
    Z: Sleeper + ?Sized,
{
    sleeper.sleep_ns(ns);
    let mut now = ticker.now_ns();
    for _ in 0..TOP_UPS {
        let elapsed = now.saturating_sub(from);
        if elapsed >= ns {
            break;
        }
        sleeper.sleep_ns(ns - elapsed);
        now = ticker.now_ns();
    }
    now
}

/// The delta of a cumulative counter between two readings.
fn delta(
    before: Result<u64, ArmError>,
    after: Result<u64, ArmError>,
    what: &str,
) -> Result<u64, String> {
    match (before, after) {
        (Ok(a), Ok(b)) if b >= a => Ok(b - a),
        (Ok(a), Ok(b)) => Err(format!("the {what} went down from {a} to {b}")),
        (Err(e), _) | (_, Err(e)) => Err(e.0),
    }
}

/// Observes the idle CPU of `subject` ([MP §4.7]) and returns its record. A loaded observation still needs the
/// driver's replay verdict ([`IdleRecord::set_load_replay`]).
///
/// Steps: check the spec; take the start snapshot; for each of the [`WINDOWS`] windows, have the subject answer one
/// request, wait [`SETTLE_NS`], read available physical memory, read the subject's CPU time and context switches,
/// wait [`WINDOW_NS`] and read them again; read available physical memory once more; take the end snapshot. Both
/// waits are measured on the monotonic clock and topped up while it shows them short ([`TOP_UPS`]). A request that
/// fails stops the observation.
pub fn observe_idle_cpu<M, T, R, S, Z>(
    ctx: &mut Context<'_, M, T, R>,
    spec: &IdleSpec,
    subject: &mut S,
    sleeper: &mut Z,
) -> Result<IdleRecord, ProbeError>
where
    M: Meter,
    T: Ticker,
    R: CommandRunner,
    S: IdleSubject + ?Sized,
    Z: Sleeper + ?Sized,
{
    if let Some(p) = spec_problem(spec.measurement, &spec.quantity, &spec.condition, ctx.host) {
        return Err(ProbeError::Spec(p));
    }
    let started = utc_stamp(SystemTime::now());
    let start = HostSnapshot::take(&mut ctx.runner);
    let mut memory = MemoryWatch::default();
    let mut windows = Vec::with_capacity(WINDOWS);
    for _ in 0..WINDOWS {
        subject.request().map_err(|error| ProbeError::Arm {
            arm: spec.quantity.clone(),
            error,
        })?;
        let answered = ctx.ticker.now_ns();
        wait_on_clock(&mut ctx.ticker, sleeper, answered, SETTLE_NS);
        memory.observe(&spec.condition, ctx.meter.available_physical());
        let (cpu0, switches0) = (subject.cpu_ns(), subject.context_switches());
        let t0 = ctx.ticker.now_ns();
        let t1 = wait_on_clock(&mut ctx.ticker, sleeper, t0, WINDOW_NS);
        let (cpu1, switches1) = (subject.cpu_ns(), subject.context_switches());
        windows.push(IdleWindow {
            cpu_ns: delta(cpu0, cpu1, "CPU time"),
            context_switches: delta(switches0, switches1, "context-switch count"),
            elapsed_ns: t1.saturating_sub(t0),
        });
    }
    memory.observe(&spec.condition, ctx.meter.available_physical());
    let end = HostSnapshot::take(&mut ctx.runner);
    let ended = utc_stamp(SystemTime::now());
    let mut reasons = memory.reasons(&spec.condition);
    reasons.extend(window_reasons(&windows));
    Ok(IdleRecord {
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
        windows,
        memory,
        reasons,
    })
}

/// The largest of some deltas; `None` when there is none or one could not be read.
fn max_read<'a>(deltas: impl Iterator<Item = &'a Result<u64, String>>) -> Option<u64> {
    let mut max: Option<u64> = None;
    for d in deltas {
        let v = *d.as_ref().ok()?;
        max = Some(max.map_or(v, |m| m.max(v)));
    }
    max
}

/// A delta as `{"ok": n}` or `{"error": "…"}` ([MP §7.1]).
fn either_json(r: &Result<u64, String>) -> Value {
    match r {
        Ok(v) => json!({ "ok": v }),
        Err(e) => json!({ "error": e }),
    }
}

fn either_of(v: Option<&Value>, what: &str) -> Result<Result<u64, String>, String> {
    let o = v
        .and_then(Value::as_object)
        .ok_or_else(|| format!("idle window: '{what}' is not an object"))?;
    match (o.get("ok"), o.get("error"), o.len()) {
        (Some(x), None, 1) => x
            .as_u64()
            .map(Ok)
            .ok_or_else(|| format!("idle window: '{what}' is not an unsigned integer")),
        (None, Some(Value::String(e)), 1) => Ok(Err(e.clone())),
        _ => Err(format!(
            "idle window: '{what}' needs exactly one of 'ok' and 'error'"
        )),
    }
}

impl IdleRecord {
    /// The shared fields.
    pub(crate) fn header(&self) -> Header<'_> {
        Header {
            measurement: self.measurement,
            quantity: &self.quantity,
            condition: &self.condition,
            load_replay_valid: self.load_replay_valid,
            host: &self.host,
            toolchain: &self.toolchain,
            commit: &self.commit,
            started: &self.started,
            ended: &self.ended,
        }
    }

    /// Whether the observation is valid ([MP §7.3]).
    pub fn valid(&self) -> bool {
        self.header().valid(&self.reasons)
    }

    /// Why the observation may not decide anything ([MP §7.3]); empty for an exit-grade one.
    pub fn disqualifications(&self) -> Vec<String> {
        self.header().disqualifications(&self.reasons)
    }

    /// Whether the observation is exit-grade ([MP §7.3]).
    pub fn exit_grade(&self) -> bool {
        self.disqualifications().is_empty()
    }

    /// Records a loaded observation's replay verdict ([MP §2.3]); ignored for other conditions.
    pub fn set_load_replay(&mut self, valid: bool) {
        if matches!(self.condition, Condition::Loaded { .. }) {
            self.load_replay_valid = Some(valid);
        }
    }

    /// The idle-CPU gate ([MP §4.7]): the maxima over the windows, whether both are zero, and whether the
    /// observation decides.
    pub fn outcome(&self) -> IdleOutcome {
        let max_cpu_ns = max_read(self.windows.iter().map(|w| &w.cpu_ns));
        let max_context_switches = max_read(self.windows.iter().map(|w| &w.context_switches));
        IdleOutcome {
            max_cpu_ns,
            max_context_switches,
            holds: max_cpu_ns == Some(0) && max_context_switches == Some(0),
            decides: self.exit_grade(),
        }
    }

    /// The nightly verdict of [MP §6]: the gate is absolute (an idle observation has no noise band, baseline or
    /// regression rule); an observation that may decide holds or fails on its own maxima, and one that is not
    /// exit-grade is undecided, with its disqualifications.
    // spec: [MP §6] (the idle observation in nightly runs), [MP §4.7] (the gate), [MP §7.3] (who may decide)
    pub fn nightly(&self) -> NightlyIdle {
        let reasons = self.disqualifications();
        if !reasons.is_empty() {
            return NightlyIdle::Undecided(reasons);
        }
        // An exit-grade observation is valid, so every delta was read (an unreadable one is a validity reason).
        let o = self.outcome();
        match (o.max_cpu_ns, o.max_context_switches) {
            (Some(0), Some(0)) => NightlyIdle::Holds,
            (Some(cpu), Some(cs)) => NightlyIdle::Fails {
                max_cpu_ns: cpu,
                max_context_switches: cs,
            },
            _ => NightlyIdle::Undecided(vec![
                "a window's delta could not be read, but the observation has no reason for it"
                    .into(),
            ]),
        }
    }

    /// The JSON form of [MP §7.1] `moirai-probes/idle/1`.
    pub fn to_json(&self) -> Value {
        let mut m = self.header().json_fields(IDLE_SCHEMA);
        m.insert("settle_ns".into(), json!(SETTLE_NS));
        m.insert("window_ns".into(), json!(WINDOW_NS));
        m.insert(
            "windows".into(),
            Value::Array(
                self.windows
                    .iter()
                    .map(|w| {
                        json!({
                            "cpu_ns": either_json(&w.cpu_ns),
                            "context_switches": either_json(&w.context_switches),
                            "elapsed_ns": w.elapsed_ns,
                        })
                    })
                    .collect(),
            ),
        );
        m.insert("memory".into(), self.memory.to_json());
        m.insert("reasons".into(), json!(self.reasons));
        Value::Object(m)
    }

    /// Reads an observation written by [`IdleRecord::to_json`] and checks it against the protocol ([MP §7.3]).
    pub fn from_json(v: &Value) -> Result<IdleRecord, String> {
        let h = OwnedHeader::from_json(v, IDLE_SCHEMA)?;
        for (k, want) in [("settle_ns", SETTLE_NS), ("window_ns", WINDOW_NS)] {
            if u64_of(v, k)? != want {
                return Err(format!("'{k}' is not {want} ([MP §4.7])"));
            }
        }
        let rec = IdleRecord {
            measurement: h.measurement,
            quantity: h.quantity,
            condition: h.condition,
            load_replay_valid: h.load_replay_valid,
            host: h.host,
            toolchain: h.toolchain,
            commit: h.commit,
            started: h.started,
            ended: h.ended,
            windows: array_of(v, "windows")?
                .iter()
                .map(|w| {
                    Ok(IdleWindow {
                        cpu_ns: either_of(w.get("cpu_ns"), "cpu_ns")?,
                        context_switches: either_of(w.get("context_switches"), "context_switches")?,
                        elapsed_ns: u64_of(w, "elapsed_ns")?,
                    })
                })
                .collect::<Result<_, String>>()?,
            memory: MemoryWatch::from_json(v.get("memory").ok_or("no 'memory'")?)?,
            reasons: strings_of(v, "reasons")?,
        };
        rec.check()?;
        Ok(rec)
    }

    /// The protocol's rules for an observation ([MP §4.7], [MP §7.3]).
    pub fn check(&self) -> Result<(), String> {
        self.header().check()?;
        if self.windows.len() != WINDOWS {
            return Err(format!(
                "an idle observation has {WINDOWS} windows, not {}",
                self.windows.len()
            ));
        }
        self.memory.check(&self.condition, WINDOWS as u64 + 1)?;
        let required = self
            .memory
            .reasons(&self.condition)
            .into_iter()
            .chain(window_reasons(&self.windows));
        for r in required {
            if !self.reasons.contains(&r) {
                return Err(format!("the record omits the reason '{r}'"));
            }
        }
        Ok(())
    }

    /// The raw file name of [MP §7.2].
    pub fn raw_file_name(&self) -> Result<String, String> {
        self.header().raw_file_name()
    }

    /// Writes the observation to `<private_root>/measurements/<n>/<raw file name>` ([MP §7.2]); an existing file is
    /// never overwritten.
    pub fn write_raw(&self, private_root: &Path) -> std::io::Result<PathBuf> {
        let name = self.raw_file_name().map_err(std::io::Error::other)?;
        write_raw_file(private_root, self.measurement, &name, |w| {
            serde_json::to_writer(&mut *w, &self.to_json()).map_err(std::io::Error::from)
        })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::condition::{IDLE_FLOOR, LOADED_TARGET};
    use crate::host::HostKind;
    use crate::host::tests::Canned;
    use crate::testkit::{FakeMeter, FakeTicker, meter_err, scratch_dir};
    use moirai_vfs::CpuTimes;
    use proptest::prelude::*;

    /// A subject whose counters follow scripts, one entry per reading (the last repeats).
    #[derive(Default)]
    struct Scripted {
        cpu: Vec<Result<u64, ArmError>>,
        switches: Vec<Result<u64, ArmError>>,
        cpu_reads: usize,
        switch_reads: usize,
        requests: usize,
        fail_request: bool,
    }

    fn at<T: Clone>(v: &[T], i: usize) -> T {
        v[i.min(v.len() - 1)].clone()
    }

    impl IdleSubject for Scripted {
        fn request(&mut self) -> Result<(), ArmError> {
            self.requests += 1;
            if self.fail_request {
                return Err(ArmError("the server did not answer".into()));
            }
            Ok(())
        }

        fn cpu_ns(&mut self) -> Result<u64, ArmError> {
            self.cpu_reads += 1;
            at(&self.cpu, self.cpu_reads - 1)
        }

        fn context_switches(&mut self) -> Result<u64, ArmError> {
            self.switch_reads += 1;
            at(&self.switches, self.switch_reads - 1)
        }
    }

    /// Advances a fake clock instead of sleeping, and logs each wait. Wait i falls short of what it was asked by
    /// `shortfalls[i]` (the last repeats; none is no shortfall).
    struct FakeSleeper {
        clock: FakeTicker,
        shortfalls: Vec<u64>,
        log: Vec<u64>,
    }

    impl Sleeper for FakeSleeper {
        fn sleep_ns(&mut self, ns: u64) {
            let short = match self.shortfalls.as_slice() {
                [] => 0,
                s => at(s, self.log.len()),
            };
            self.log.push(ns);
            self.clock.advance(ns.saturating_sub(short));
        }
    }

    fn ctx(meter: &FakeMeter, clock: FakeTicker) -> Context<'_, FakeMeter, FakeTicker, Canned> {
        Context {
            meter,
            ticker: clock,
            runner: Canned::good(),
            host: HostKind::Laptop,
            toolchain: "1.98.1".into(),
            commit: "0123456789ab".into(),
        }
    }

    fn spec(condition: Condition) -> IdleSpec {
        IdleSpec {
            measurement: 19,
            quantity: "idle-cpu.mcp".into(),
            condition,
        }
    }

    fn quiet() -> Scripted {
        Scripted {
            cpu: vec![Ok(40_000_000)],
            switches: vec![Ok(1_234)],
            ..Scripted::default()
        }
    }

    fn observe(
        meter: &FakeMeter,
        s: &IdleSpec,
        subject: &mut Scripted,
        shortfalls: &[u64],
    ) -> (Result<IdleRecord, ProbeError>, Vec<u64>) {
        let clock = FakeTicker::manual();
        let mut sleeper = FakeSleeper {
            clock: clock.clone(),
            shortfalls: shortfalls.to_vec(),
            log: Vec::new(),
        };
        let r = observe_idle_cpu(&mut ctx(meter, clock), s, subject, &mut sleeper);
        (r, sleeper.log)
    }

    /// A small, valid, exit-grade observation.
    pub(crate) fn sample_idle() -> IdleRecord {
        let meter = FakeMeter::new(8_000_000_000);
        observe(&meter, &spec(Condition::Idle), &mut quiet(), &[])
            .0
            .unwrap()
    }

    fn reads_back(rec: &IdleRecord) {
        assert_eq!(IdleRecord::from_json(&rec.to_json()), Ok(rec.clone()));
    }

    #[test]
    fn a_quiet_process_holds() {
        let meter = FakeMeter::new(8_000_000_000);
        let mut subject = quiet();
        let (rec, waits) = observe(&meter, &spec(Condition::Idle), &mut subject, &[]);
        let rec = rec.unwrap();
        assert_eq!(waits, [SETTLE_NS, WINDOW_NS].repeat(WINDOWS));
        assert_eq!(subject.requests, WINDOWS);
        assert_eq!((subject.cpu_reads, subject.switch_reads), (6, 6));
        assert_eq!(rec.memory.readings, WINDOWS as u64 + 1);
        assert!(rec.windows.iter().all(|w| w.cpu_ns == Ok(0)
            && w.context_switches == Ok(0)
            && w.elapsed_ns == WINDOW_NS));
        assert!(
            rec.valid() && rec.exit_grade(),
            "{:?}",
            rec.disqualifications()
        );
        let o = rec.outcome();
        assert_eq!(
            o,
            IdleOutcome {
                max_cpu_ns: Some(0),
                max_context_switches: Some(0),
                holds: true,
                decides: true
            }
        );
        reads_back(&rec);
    }

    #[test]
    fn a_busy_window_fails_the_gate_but_stays_valid() {
        let meter = FakeMeter::new(8_000_000_000);
        // Readings come in pairs per window (start, end); the second window's CPU time grows by one tick.
        let mut subject = Scripted {
            cpu: vec![Ok(10), Ok(10), Ok(10), Ok(15_625_010)],
            switches: vec![Ok(5), Ok(5), Ok(5), Ok(7)],
            ..Scripted::default()
        };
        let (rec, _) = observe(&meter, &spec(Condition::Idle), &mut subject, &[]);
        let rec = rec.unwrap();
        assert!(rec.valid());
        let o = rec.outcome();
        assert_eq!(
            (o.max_cpu_ns, o.max_context_switches, o.holds, o.decides),
            (Some(15_625_000), Some(2), false, true)
        );
        assert_eq!(rec.windows[1].cpu_ns, Ok(15_625_000));
        assert_eq!(rec.windows[2].cpu_ns, Ok(0));
        reads_back(&rec);
    }

    #[test]
    fn nightly_runs_judge_the_gate_absolutely() {
        // [MP §6]: no band and no baseline. An exit-grade observation holds or fails on its own maxima.
        let calm = sample_idle();
        assert_eq!(calm.nightly(), NightlyIdle::Holds);
        assert!(!calm.nightly().fails());
        let meter = FakeMeter::new(8_000_000_000);
        let mut busy = Scripted {
            cpu: vec![Ok(10), Ok(10), Ok(10), Ok(15_625_010)],
            switches: vec![Ok(5), Ok(5), Ok(5), Ok(7)],
            ..Scripted::default()
        };
        let busy = observe(&meter, &spec(Condition::Idle), &mut busy, &[])
            .0
            .unwrap();
        assert_eq!(
            busy.nightly(),
            NightlyIdle::Fails {
                max_cpu_ns: 15_625_000,
                max_context_switches: 2
            }
        );
        assert!(busy.nightly().fails());
        // One that may not decide is undecided with its disqualifications, even when its maxima are zero: a synthetic
        // condition never decides, nor does a hosted runner or an invalid observation.
        let synthetic = observe(
            &meter,
            &spec(Condition::Synthetic {
                description: "hosted runner, synthetic load".into(),
            }),
            &mut quiet(),
            &[],
        )
        .0
        .unwrap();
        assert!(synthetic.outcome().holds);
        match synthetic.nightly() {
            NightlyIdle::Undecided(r) => assert!(!r.is_empty(), "{r:?}"),
            other => panic!("{other:?}"),
        }
        let mut hosted = calm.clone();
        hosted.host.kind = HostKind::Hosted;
        assert!(matches!(hosted.nightly(), NightlyIdle::Undecided(_)));
        let mut short = calm.clone();
        short.windows[1].elapsed_ns = WINDOW_NS - 1;
        short.reasons = window_reasons(&short.windows);
        assert!(
            matches!(short.nightly(), NightlyIdle::Undecided(r) if r.iter().any(|x| x.contains("idle window 1")))
        );
        // A record assembled by hand with an unreadable delta but no reason for it never passes or fails.
        let mut odd = calm;
        odd.windows[0].cpu_ns = Err("lost".into());
        assert!(matches!(odd.nightly(), NightlyIdle::Undecided(_)));
    }

    #[test]
    fn unreadable_counters_and_short_windows_invalidate() {
        let meter = FakeMeter::new(8_000_000_000);
        let mut subject = Scripted {
            cpu: vec![Ok(10), Ok(9), Ok(9)],
            switches: vec![Ok(5), Err(ArmError("ETW session lost".into())), Ok(5)],
            ..Scripted::default()
        };
        // Every wait falls 1 ns short, a top-up too, so the clock never catches up: a clock that misbehaves.
        let (rec, waits) = observe(&meter, &spec(Condition::Idle), &mut subject, &[1]);
        let rec = rec.unwrap();
        let top_ups = [1].repeat(TOP_UPS as usize);
        let window = [&[SETTLE_NS][..], &top_ups, &[WINDOW_NS], &top_ups].concat();
        assert_eq!(waits, window.repeat(WINDOWS));
        assert!(rec.windows.iter().all(|w| w.elapsed_ns == WINDOW_NS - 1));
        assert!(!rec.valid());
        assert_eq!(
            rec.windows[0].cpu_ns,
            Err("the CPU time went down from 10 to 9".into())
        );
        assert_eq!(
            rec.windows[0].context_switches,
            Err("ETW session lost".into())
        );
        assert!(
            rec.reasons
                .iter()
                .any(|r| r.contains("could not be read: ETW session lost"))
        );
        assert!(
            rec.reasons
                .iter()
                .any(|r| r.contains("lasted 599999999999 ns, less than 600.000 s"))
        );
        let o = rec.outcome();
        assert_eq!((o.max_cpu_ns, o.holds, o.decides), (None, false, false));
        reads_back(&rec);
    }

    #[test]
    fn a_wait_that_wakes_early_is_topped_up() {
        let meter = FakeMeter::new(8_000_000_000);
        // Window 0's wait wakes 1 ms early; window 1's settle 2 ms early; window 2's wait one 15.625 ms tick early,
        // and its first top-up 7 ms early again.
        let shortfalls = [
            0, 1_000_000, 0, 2_000_000, 0, 0, 0, 15_625_000, 7_000_000, 0,
        ];
        let (rec, waits) = observe(&meter, &spec(Condition::Idle), &mut quiet(), &shortfalls);
        let rec = rec.unwrap();
        assert_eq!(
            waits,
            [
                SETTLE_NS, WINDOW_NS, 1_000_000, SETTLE_NS, 2_000_000, WINDOW_NS, SETTLE_NS,
                WINDOW_NS, 15_625_000, 7_000_000
            ]
        );
        assert!(rec.windows.iter().all(|w| w.elapsed_ns == WINDOW_NS));
        assert!(
            rec.reasons.is_empty() && rec.exit_grade() && rec.outcome().holds,
            "{:?}",
            rec.reasons
        );
        reads_back(&rec);
    }

    proptest! {
        /// A wait ends at or past its length unless the top-ups ran out, never makes more than 1 + [`TOP_UPS`] waits,
        /// and asks each top-up for exactly the remainder.
        #[test]
        fn waits_reach_their_length_or_run_out_of_top_ups(
            ns in 1u64..=WINDOW_NS,
            shortfalls in proptest::collection::vec(0u64..=WINDOW_NS, 1..8),
        ) {
            let mut clock = FakeTicker::manual();
            clock.advance(1_000);
            let mut sleeper = FakeSleeper { clock: clock.clone(), shortfalls, log: Vec::new() };
            let from = clock.now_ns();
            let end = wait_on_clock(&mut clock, &mut sleeper, from, ns);
            prop_assert_eq!(end, clock.now_ns());
            prop_assert!(!sleeper.log.is_empty() && sleeper.log.len() <= 1 + TOP_UPS as usize);
            prop_assert!(end - from >= ns || sleeper.log.len() == 1 + TOP_UPS as usize);
            prop_assert_eq!(sleeper.log[0], ns);
            let mut elapsed = 0;
            for (i, asked) in sleeper.log.iter().enumerate() {
                if i > 0 {
                    prop_assert_eq!(*asked, ns - elapsed);
                }
                elapsed += asked.saturating_sub(at(&sleeper.shortfalls, i));
            }
            prop_assert_eq!(elapsed, end - from);
        }
    }

    #[test]
    fn conditions_are_read_at_window_boundaries() {
        let meter = FakeMeter::scripted(vec![
            Ok(8_000_000_000),
            Ok(IDLE_FLOOR - 1),
            Ok(8_000_000_000),
        ]);
        let (rec, _) = observe(&meter, &spec(Condition::Idle), &mut quiet(), &[]);
        let rec = rec.unwrap();
        assert_eq!((rec.memory.readings, rec.memory.out_of_band), (4, 1));
        assert!(!rec.valid() && !rec.outcome().decides);
        reads_back(&rec);
        let loaded = Condition::Loaded {
            fixture: "a1".repeat(32),
        };
        let meter = FakeMeter::new(LOADED_TARGET);
        let mut rec = observe(&meter, &spec(loaded), &mut quiet(), &[]).0.unwrap();
        assert!(rec.reasons.is_empty() && !rec.exit_grade());
        rec.set_load_replay(true);
        assert!(rec.exit_grade() && rec.outcome().holds);
        reads_back(&rec);
        let mut hosted = sample_idle();
        hosted.host.kind = HostKind::Hosted;
        assert!(hosted.valid() && !hosted.outcome().decides);
        let mut idle = sample_idle();
        idle.set_load_replay(true);
        assert_eq!(
            idle.load_replay_valid, None,
            "only a loaded observation has a verdict"
        );
    }

    #[test]
    fn refusals() {
        let meter = FakeMeter::new(8_000_000_000);
        let mut subject = quiet();
        subject.fail_request = true;
        let (e, waits) = observe(&meter, &spec(Condition::Idle), &mut subject, &[]);
        assert_eq!(
            e.unwrap_err().to_string(),
            "arm idle-cpu.mcp failed: the server did not answer"
        );
        assert!(waits.is_empty());
        let mut s = spec(Condition::Idle);
        s.quantity = "Idle CPU".into();
        assert!(matches!(
            observe(&meter, &s, &mut quiet(), &[]).0,
            Err(ProbeError::Spec(_))
        ));
        let clock = FakeTicker::manual();
        let mut c = ctx(&meter, clock.clone());
        c.host = HostKind::Hosted;
        let mut sleeper = FakeSleeper {
            clock,
            shortfalls: Vec::new(),
            log: Vec::new(),
        };
        let loaded = spec(Condition::Loaded {
            fixture: "0f".repeat(32),
        });
        let e = observe_idle_cpu(&mut c, &loaded, &mut quiet(), &mut sleeper);
        assert!(matches!(e, Err(ProbeError::Spec(m)) if m.contains("hosted")));
    }

    #[test]
    fn from_json_refuses_broken_records() {
        let r = sample_idle();
        // Each case: the substring its refusal must contain, and the edit.
        let mut cases: Vec<(&str, Value)> = Vec::new();
        let mut edit = |expect: &'static str, f: &dyn Fn(&mut Value)| {
            let mut v = r.to_json();
            f(&mut v);
            cases.push((expect, v));
        };
        edit("not a moirai-probes/idle/1 record", &|v| {
            v["schema"] = json!("moirai-probes/run/1");
        });
        edit("'window_ns' is not 600000000000", &|v| {
            v["window_ns"] = json!(60 * SEC);
        });
        edit("'settle_ns' is not 15000000000", &|v| {
            v["settle_ns"] = json!(0)
        });
        edit("has 3 windows, not 2", &|v| {
            v["windows"].as_array_mut().unwrap().pop();
        });
        edit("omits the reason 'idle window 1 lasted", &|v| {
            v["windows"][1]["elapsed_ns"] = json!(WINDOW_NS - 1);
        });
        edit(
            "omits the reason 'idle window 0: the CPU time could not be read",
            &|v| {
                v["windows"][0]["cpu_ns"] = json!({"error": "GetProcessTimes failed"});
            },
        );
        edit("needs exactly one of 'ok' and 'error'", &|v| {
            v["windows"][0]["cpu_ns"] = json!({"ok": 0, "error": "x"});
        });
        edit("3 readings, the protocol takes 4", &|v| {
            v["memory"]["readings"] = json!(3);
        });
        edit("only a loaded run carries", &|v| {
            v["load_replay_valid"] = json!(false);
        });
        edit("measurement number must be at least 1", &|v| {
            v["measurement"] = json!(0);
        });
        for (expect, v) in cases {
            let e = IdleRecord::from_json(&v).unwrap_err();
            assert!(e.contains(expect), "expected '{expect}', got '{e}'");
        }
    }

    #[test]
    fn cpu_time_comes_from_the_meter() {
        let meter = FakeMeter::new(1);
        *meter.cpu.lock().unwrap() = vec![
            Ok(CpuTimes {
                user_ns: 300,
                kernel_ns: 200,
            }),
            Err(meter_err("GetProcessTimes")),
        ];
        assert_eq!(cpu_ns_of(&meter, None), Ok(500));
        assert!(
            cpu_ns_of(&meter, None)
                .unwrap_err()
                .0
                .contains("GetProcessTimes")
        );
    }

    #[test]
    fn thread_sleeper_waits_at_least_as_asked() {
        let t = std::time::Instant::now();
        ThreadSleeper.sleep_ns(2_000_000);
        assert!(t.elapsed() >= Duration::from_millis(2));
    }

    #[test]
    fn raw_files() {
        let root = scratch_dir("idle-raw");
        let r = sample_idle();
        let p = r.write_raw(&root).unwrap();
        assert!(
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("idle-cpu.mcp.idle.")
        );
        let back = IdleRecord::from_json(
            &serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap(),
        );
        assert_eq!(back, Ok(r.clone()));
        assert!(r.write_raw(&root).is_err());
        std::fs::remove_dir_all(&root).unwrap();
    }
}
