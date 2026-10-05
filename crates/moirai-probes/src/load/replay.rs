//! The replay loop ([MP §9.4]): one step per sample — the generator's own share of the interval, the log line, the
//! settle check, the stop check, and the commands for the next interval — and the replay log, which the validation
//! reads ([MP §9.5]).
//!
//! The log is JSON lines, written and flushed one line per sample so a driver can read it while the replay runs:
//! a header (`moirai-probes/replay/1`), one line per sample, and an end line; `moirai-probes-bin loadgen` appends its
//! verdict (`moirai-probes/replay-verdict/1`) after the end line. Each sample carries its wall-clock time `t`, which
//! places a run's window, and its monotonic time `mono`, which orders and spaces the samples ([MP §9.5]).

use super::actuate::Actuators;
use super::control::{Commands, Controller, Own};
use super::fixture::{Profile, Source};
use super::sample::{S_AVAILABLE, S_CPU, S_READ, S_WRITE, SAMPLE_MS, Sampler};
use crate::condition::{LOADED_HIGH, LOADED_LOW};
use moirai_vfs::Meter;
use serde_json::{Value, json};
use std::io::Write;

/// The schema name of the replay log's header ([MP §9.4]).
pub const LOG_SCHEMA: &str = "moirai-probes/replay/1";
/// The shortest replay before the generator may report itself settled ([MP §9.4]).
pub const SETTLE_MS: u64 = 60_000;
/// The consecutive samples with available physical memory in the loaded band that settling needs.
pub const SETTLE_SAMPLES: usize = 10;

/// How the replay is run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Settings {
    /// The logical processors the processor counter spans, which is also the number of CPU workers.
    pub workers: usize,
    /// The available physical memory the hold keeps (written to the log header).
    pub ram_target: u64,
    /// The shortest replay before settling ([`SETTLE_MS`]).
    pub settle_ms: u64,
    /// Stop after this long, counted from the first sample.
    pub max_ms: Option<u64>,
}

/// Why the replay ended.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Ending {
    /// The stop request was seen.
    StopRequest,
    /// The maximum duration passed.
    MaxDuration,
    /// The sampler ended.
    SamplerEnded,
}

impl Ending {
    /// The log spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Ending::StopRequest => "stop-request",
            Ending::MaxDuration => "max-duration",
            Ending::SamplerEnded => "sampler-ended",
        }
    }
}

/// What a finished replay reports.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Outcome {
    /// The wall-clock time of the first sample, in milliseconds since 1970.
    pub started: u64,
    /// When the replay settled, likewise; `None` if it never did.
    pub settled: Option<u64>,
    /// The wall-clock time of the last sample.
    pub ended: u64,
    /// The samples taken after the first.
    pub samples: u64,
    /// The sampler's lines skipped for their width ([`Sampler::skipped`]).
    pub skipped: u64,
    /// Why it ended.
    pub ending: Ending,
}

/// Where the replay writes and whom it asks: the log, the stop request, and the settle hook.
pub struct Io<'a> {
    /// The replay log.
    pub log: &'a mut dyn Write,
    /// Whether a stop has been requested; asked after every sample.
    pub stop: &'a mut dyn FnMut() -> bool,
    /// Called once when the replay settles, with the sample's wall-clock time.
    pub on_settle: &'a mut dyn FnMut(u64) -> Result<(), String>,
}

/// The JSON form of a profile's source in the log header.
pub fn source_json(source: &Source) -> Value {
    match source {
        Source::Fixture { id } => json!({ "kind": "fixture", "id": id }),
        Source::Synthetic { spec } => json!({ "kind": "synthetic", "spec": spec }),
    }
}

/// This process's user plus kernel time, in nanoseconds.
fn cpu_ns<M: Meter>(meter: &M) -> Result<u64, String> {
    meter
        .cpu_times(None)
        .map(|t| t.user_ns.saturating_add(t.kernel_ns))
        .map_err(|e| format!("the generator's own processor time: {e}"))
}

/// Writes one log line and flushes it.
fn line(log: &mut dyn Write, v: &Value) -> Result<(), String> {
    writeln!(log, "{v}")
        .and_then(|()| log.flush())
        .map_err(|e| format!("the replay log: {e}"))
}

/// A number for the log: whole for large values, three decimals below 1,000.
fn num(v: f64) -> Value {
    if v.abs() >= 1_000.0 {
        json!(v.round())
    } else {
        json!((v * 1_000.0).round() / 1_000.0)
    }
}

/// Replays `profile` until a stop is requested, the maximum duration passes, or the sampler ends ([MP §9.4]). Each
/// sample closes an interval: the generator's own processor share (from `meter.cpu_times`, over all `workers`) and
/// disk bytes are measured over it, the line is logged, settling is checked (at least `settle_ms` since the first
/// sample and the last [`SETTLE_SAMPLES`] availability readings in the loaded band; the settle hook is called once),
/// and the controller sets the next interval's commands from the profile at the elapsed replay time. The load is left
/// idle at the end.
pub fn replay<S: Sampler, A: Actuators, M: Meter>(
    profile: &Profile,
    sampler: &mut S,
    load: &mut A,
    meter: &M,
    settings: &Settings,
    io: Io<'_>,
) -> Result<Outcome, String> {
    let Io {
        log,
        stop,
        on_settle,
    } = io;
    let first = sampler
        .next()?
        .ok_or("the sampler ended before its first sample")?;
    line(
        log,
        &json!({
            "schema": LOG_SCHEMA,
            "source": source_json(&profile.source),
            "interval_ms": profile.interval_ms,
            "sample_ms": SAMPLE_MS,
            "duration_ms": profile.duration_ms(),
            "workers": settings.workers,
            "ram_target": settings.ram_target,
            "started": first.unix_ms,
        }),
    )?;
    let workers = settings.workers.max(1) as f64;
    let (t0, mut prev_mono) = (first.mono_ms, first.mono_ms);
    let mut prev_cpu = cpu_ns(meter)?;
    let mut prev_io = load.own_io();
    let mut ctl = Controller::new();
    let mut tau = 0;
    let mut target = profile.at(0);
    load.apply(&ctl.step(&target, [None; 3], &Own::default()));
    let (mut in_band, mut settled, mut samples, mut last) = (0, None, 0, first.unix_ms);
    let band = LOADED_LOW as f64..=LOADED_HIGH as f64;
    let ending = loop {
        if let Some(f) = load.failure() {
            return Err(f);
        }
        let Some(s) = sampler.next()? else {
            break Ending::SamplerEnded;
        };
        let dt = s.mono_ms.saturating_sub(prev_mono).max(1) as f64;
        let cpu = cpu_ns(meter)?;
        let io = load.own_io();
        let own = Own {
            cpu: cpu.saturating_sub(prev_cpu) as f64 / (dt * 1e6 * workers) * 100.0,
            read: io.0.saturating_sub(prev_io.0) as f64 * 1_000.0 / dt,
            write: io.1.saturating_sub(prev_io.1) as f64 * 1_000.0 / dt,
        };
        (prev_mono, prev_cpu, prev_io, last) = (s.mono_ms, cpu, io, s.unix_ms);
        samples += 1;
        let elapsed = s.mono_ms.saturating_sub(t0);
        in_band = if s.values[S_AVAILABLE].is_some_and(|a| band.contains(&a)) {
            in_band + 1
        } else {
            0
        };
        if settled.is_none() && elapsed >= settings.settle_ms && in_band >= SETTLE_SAMPLES {
            settled = Some(s.unix_ms);
            on_settle(s.unix_ms)?;
        }
        line(
            log,
            &json!({
                "t": s.unix_ms,
                "mono": s.mono_ms,
                "tau": tau,
                "target": [num(target.cpu), num(target.read), num(target.write)],
                "observed": s.values.map(|v| v.map(num)),
                "own": [num(own.cpu), num(own.read), num(own.write)],
                "held": load.held(),
                "settled": settled.is_some(),
            }),
        )?;
        if stop() {
            break Ending::StopRequest;
        }
        if settings.max_ms.is_some_and(|m| elapsed >= m) {
            break Ending::MaxDuration;
        }
        tau = elapsed;
        target = profile.at(tau);
        let observed = [s.values[S_CPU], s.values[S_READ], s.values[S_WRITE]];
        load.apply(&ctl.step(&target, observed, &own));
    };
    load.apply(&Commands::IDLE);
    let skipped = sampler.skipped();
    line(
        log,
        &json!({
            "end": last,
            "settled": settled,
            "samples": samples,
            "skipped": skipped,
            "ending": ending.as_str(),
        }),
    )?;
    Ok(Outcome {
        started: first.unix_ms,
        settled,
        ended: last,
        samples,
        skipped,
        ending,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    //! A simulated machine for the replay and its validation: the rest of the machine's load is fixed, the generator's
    //! own load is what its commands asked (the CPU workers delivering a share of their duty), and available memory is
    //! a base minus what the hold holds.

    use super::*;
    use crate::load::control::{CHUNK, MemoryStep, memory_step};
    use crate::load::sample::Sample;
    use crate::load::synthetic::Spec;
    use moirai_vfs::{
        ChildPeak, ChildTicket, CpuTimes, FreeSpace, HeapCounts, MeterError, OsCode, VfsError,
    };
    use std::sync::{Arc, Mutex};

    /// The simulated machine.
    #[derive(Debug)]
    pub struct Plant {
        pub rest: [f64; 3],
        pub base_avail: u64,
        pub efficiency: f64,
        pub workers: usize,
        pub commands: Commands,
        pub held: u64,
        pub io: (u64, u64),
        pub cpu_ns: u64,
        pub mono_ms: u64,
        pub unix0: u64,
        pub samples: u64,
        pub limit: u64,
        pub missing_every: u64,
        pub skipped: u64,
    }

    impl Plant {
        pub fn new(rest: [f64; 3], base_avail: u64, efficiency: f64) -> Arc<Mutex<Plant>> {
            Arc::new(Mutex::new(Plant {
                rest,
                base_avail,
                efficiency,
                workers: 4,
                commands: Commands::IDLE,
                held: 0,
                io: (0, 0),
                cpu_ns: 0,
                mono_ms: 0,
                unix0: 1_791_106_225_000,
                samples: 0,
                limit: u64::MAX,
                missing_every: 0,
                skipped: 0,
            }))
        }
    }

    pub struct SimSampler(pub Arc<Mutex<Plant>>);
    pub struct SimLoad(pub Arc<Mutex<Plant>>);
    pub struct SimMeter(pub Arc<Mutex<Plant>>);

    impl Sampler for SimSampler {
        fn next(&mut self) -> Result<Option<Sample>, String> {
            let mut p = self.0.lock().unwrap();
            if p.samples >= p.limit {
                return Ok(None);
            }
            p.samples += 1;
            if p.samples > 1 {
                // One second under the commands in force.
                let own_cpu = p.commands.duty * 100.0 * p.efficiency;
                p.cpu_ns += (own_cpu / 100.0 * 1e9 * p.workers as f64) as u64;
                let (r, w) = (p.commands.read as u64, p.commands.write as u64);
                p.io = (p.io.0 + r, p.io.1 + w);
                p.mono_ms += 1_000;
                let step = memory_step(
                    p.base_avail.saturating_sub(p.held),
                    p.held,
                    crate::condition::LOADED_TARGET,
                    12_000_000_000,
                );
                match step {
                    MemoryStep::Grow(n) => p.held += n * CHUNK,
                    MemoryStep::Shrink(n) => p.held -= n * CHUNK,
                    MemoryStep::Hold => {}
                }
            }
            let own = [
                p.commands.duty * 100.0 * p.efficiency,
                p.commands.read,
                p.commands.write,
            ];
            let mut values = [
                Some((p.rest[0] + own[0]).min(100.0)),
                Some(p.rest[1] + own[1]),
                Some(p.rest[2] + own[2]),
                Some(p.base_avail.saturating_sub(p.held) as f64),
            ];
            if p.missing_every > 0 && p.samples.is_multiple_of(p.missing_every) {
                values[1] = None;
            }
            Ok(Some(Sample {
                mono_ms: p.mono_ms,
                unix_ms: p.unix0 + p.mono_ms,
                values,
            }))
        }

        fn skipped(&self) -> u64 {
            self.0.lock().unwrap().skipped
        }
    }

    impl Actuators for SimLoad {
        fn apply(&mut self, c: &Commands) {
            self.0.lock().unwrap().commands = *c;
        }
        fn own_io(&self) -> (u64, u64) {
            self.0.lock().unwrap().io
        }
        fn held(&self) -> u64 {
            self.0.lock().unwrap().held
        }
        fn failure(&self) -> Option<String> {
            None
        }
        fn finish(&mut self) -> Result<(), String> {
            self.0.lock().unwrap().held = 0;
            Ok(())
        }
    }

    fn err(what: &'static str) -> MeterError {
        MeterError {
            os: OsCode::NONE,
            what,
        }
    }

    impl Meter for SimMeter {
        fn free_space(&self, _dir: &std::path::Path) -> Result<FreeSpace, VfsError> {
            Ok(FreeSpace {
                available: 100_000_000_000,
                total: 500_000_000_000,
            })
        }
        fn private_now(&self) -> Result<u64, MeterError> {
            Err(err("sim"))
        }
        fn private_peak(&self) -> Result<u64, MeterError> {
            Err(err("sim"))
        }
        fn available_physical(&self) -> Result<u64, MeterError> {
            let p = self.0.lock().unwrap();
            Ok(p.base_avail.saturating_sub(p.held))
        }
        fn peak_of_child(&self, _c: &std::process::Child) -> Result<ChildPeak, MeterError> {
            Err(err("sim"))
        }
        fn heap_counts(&self) -> Option<HeapCounts> {
            None
        }
        fn reset_heap_high_water(&self) {}
        fn child_private_now(&self, _c: &std::process::Child) -> Result<u64, MeterError> {
            Err(err("sim"))
        }
        fn child_threads(&self, _c: &std::process::Child) -> Result<u32, MeterError> {
            Err(err("sim"))
        }
        fn cpu_times(&self, _c: Option<&std::process::Child>) -> Result<CpuTimes, MeterError> {
            Ok(CpuTimes {
                user_ns: self.0.lock().unwrap().cpu_ns,
                kernel_ns: 0,
            })
        }
        fn prepare_child(&self, _c: &mut std::process::Command) -> Result<ChildTicket, MeterError> {
            Ok(ChildTicket(0))
        }
        fn bind_child(&self, _t: ChildTicket, _c: &std::process::Child) {}
    }

    pub fn settings(max_ms: Option<u64>) -> Settings {
        Settings {
            workers: 4,
            ram_target: crate::condition::LOADED_TARGET,
            settle_ms: SETTLE_MS,
            max_ms,
        }
    }

    /// Runs a simulated replay of `profile` for `seconds` and returns the log text and the outcome.
    pub fn simulate(
        plant: &Arc<Mutex<Plant>>,
        profile: &Profile,
        seconds: u64,
    ) -> (String, Outcome) {
        let mut log = Vec::new();
        let mut settles = Vec::new();
        let out = replay(
            profile,
            &mut SimSampler(plant.clone()),
            &mut SimLoad(plant.clone()),
            &SimMeter(plant.clone()),
            &settings(Some(seconds * 1_000)),
            Io {
                log: &mut log,
                stop: &mut || false,
                on_settle: &mut |t| {
                    settles.push(t);
                    Ok(())
                },
            },
        )
        .unwrap();
        assert_eq!(settles.len(), usize::from(out.settled.is_some()));
        (String::from_utf8(log).unwrap(), out)
    }

    #[test]
    fn logs_every_sample_and_settles() {
        let plant = Plant::new([10.0, 1e6, 2e6], 9_000_000_000, 0.9);
        plant.lock().unwrap().skipped = 2;
        let profile = Spec::parse("seconds=120,cpu=50,read=10MB,write=10MB,step=15,seed=4")
            .unwrap()
            .profile()
            .unwrap();
        let (log, out) = simulate(&plant, &profile, 200);
        let lines: Vec<Value> = log
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(lines[0]["schema"], LOG_SCHEMA);
        assert_eq!(lines[0]["source"]["kind"], "synthetic");
        assert_eq!(lines[0]["duration_ms"], 120_000);
        assert_eq!(lines[0]["sample_ms"], SAMPLE_MS);
        assert_eq!(
            (lines[1]["mono"].as_u64(), lines[2]["mono"].as_u64()),
            (Some(1_000), Some(2_000))
        );
        assert_eq!(
            lines[2]["t"].as_u64().unwrap() - lines[1]["t"].as_u64().unwrap(),
            1_000
        );
        assert_eq!(out.samples, 200);
        assert_eq!(out.skipped, 2);
        assert_eq!(out.ending, Ending::MaxDuration);
        assert_eq!(lines.len(), 202);
        assert_eq!(lines[201]["ending"], "max-duration");
        assert_eq!(lines[201]["skipped"], 2);
        let settled = out.settled.unwrap();
        assert!(settled >= out.started + SETTLE_MS);
        assert_eq!(lines[201]["settled"], settled);
        // The replay loops: the second pass commands the first pass's targets.
        assert_eq!(lines[1]["tau"], 0);
        assert_eq!(lines[2]["tau"], 1_000);
        assert_eq!(lines[121]["tau"], 120_000);
        assert_eq!(lines[121]["target"], lines[1]["target"]);
        // Once settled, the machine's processor total follows the target.
        let last = &lines[200];
        let (t, o) = (
            last["target"][0].as_f64().unwrap(),
            last["observed"][0].as_f64().unwrap(),
        );
        assert!((t - o).abs() < 2.0, "{last}");
    }

    #[test]
    fn stops_on_request_or_when_the_sampler_ends() {
        let profile = Spec::default().profile().unwrap();
        let plant = Plant::new([0.0; 3], 4_000_000_000, 1.0);
        let mut n = 0;
        let out = replay(
            &profile,
            &mut SimSampler(plant.clone()),
            &mut SimLoad(plant.clone()),
            &SimMeter(plant.clone()),
            &settings(None),
            Io {
                log: &mut Vec::new(),
                stop: &mut || {
                    n += 1;
                    n == 5
                },
                on_settle: &mut |_| Ok(()),
            },
        )
        .unwrap();
        assert_eq!(
            (out.samples, out.ending, out.settled),
            (5, Ending::StopRequest, None)
        );
        assert_eq!(plant.lock().unwrap().commands, Commands::IDLE);
        let taken = plant.lock().unwrap().samples;
        plant.lock().unwrap().limit = taken + 3;
        let out = replay(
            &profile,
            &mut SimSampler(plant.clone()),
            &mut SimLoad(plant.clone()),
            &SimMeter(plant.clone()),
            &settings(None),
            Io {
                log: &mut Vec::new(),
                stop: &mut || false,
                on_settle: &mut |_| Ok(()),
            },
        )
        .unwrap();
        assert_eq!((out.samples, out.ending), (2, Ending::SamplerEnded));
        plant.lock().unwrap().limit = 0;
        let e = replay(
            &profile,
            &mut SimSampler(plant.clone()),
            &mut SimLoad(plant.clone()),
            &SimMeter(plant),
            &settings(None),
            Io {
                log: &mut Vec::new(),
                stop: &mut || false,
                on_settle: &mut |_| Ok(()),
            },
        )
        .unwrap_err();
        assert!(e.contains("first sample"));
    }

    #[test]
    fn a_failing_settle_hook_stops_the_replay() {
        let profile = Spec::default().profile().unwrap();
        let plant = Plant::new([0.0; 3], 4_000_000_000, 1.0);
        let e = replay(
            &profile,
            &mut SimSampler(plant.clone()),
            &mut SimLoad(plant.clone()),
            &SimMeter(plant),
            &settings(None),
            Io {
                log: &mut Vec::new(),
                stop: &mut || false,
                on_settle: &mut |_| Err("the ready file could not be written".into()),
            },
        )
        .unwrap_err();
        assert!(e.contains("ready file"));
    }

    #[test]
    fn numbers_are_rounded_for_the_log() {
        assert_eq!(num(12.345_678), json!(12.346));
        assert_eq!(num(1_234_567.89), json!(1_234_568.0));
        assert_eq!(num(0.0), json!(0.0));
    }
}
