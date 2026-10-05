//! `moirai-probes-bin loadgen` ([MP §9.4]): the load generator's command line, over a [`Rig`] — the sampler, the load
//! and the `Meter` — so that the binary only wires [`SystemRig`] to the Windows `Meter`.

use super::actuate::{
    Actuators, DEFAULT_MAX_HOLD, DEFAULT_READ_POOL, Load, LoadConfig, MIN_READ_POOL,
};
use super::fixture;
use super::replay::{Io, SETTLE_MS, Settings, replay};
use super::sample::{Sampler, TypeperfSampler};
use super::synthetic::Spec;
use super::validate::{Verdict, read_log_file, replay_verdict};
use crate::condition::LOADED_TARGET;
use crate::units::parse_bytes;
use moirai_vfs::Meter;
use serde_json::json;
use std::ffi::{OsStr, OsString};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The usage text ([MP §9.4]).
pub const USAGE: &str = "\
usage: loadgen (--profile <fixture> | --synthetic <spec>) --scratch <dir> --log <file>
               [--ready-file <path>] [--stop-file <path>] [--max-duration <seconds>]
               [--max-hold <bytes>] [--read-pool <bytes>]
       loadgen --help
Replays a load profile on this machine: the system-wide processor time and disk read and write bytes of the profile,
with available physical memory held at 1.8 GB (docs/spec/measurement-protocol.md §9.4). --profile names a fixture
of `cargo xtask loadrec`; --synthetic takes key=value pairs (seconds, cpu, read, write, step, seed; the empty spec is
the default synthetic profile). The profile repeats until the stop file appears, the maximum duration passes, or the
sampler ends. The replay log (JSON lines, never overwritten) gets one line a second; the ready file is written once
the replay has settled. The verdict of the whole settled replay is printed and appended to the log.
Exit: 0 the replay reproduced the profile; 1 it did not; 2 usage error; 3 the replay could not run.
";

/// Exit code: the replay reproduced the profile (and `--help`).
pub const EXIT_PASS: u8 = 0;
/// Exit code: the replay ran and did not reproduce the profile.
pub const EXIT_FAIL: u8 = 1;
/// Exit code: usage error; nothing ran.
pub const EXIT_USAGE: u8 = 2;
/// Exit code: the replay could not run or was cut short by a failure.
pub const EXIT_ERROR: u8 = 3;

/// The profile to replay.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProfileArg {
    /// A fixture file.
    File(PathBuf),
    /// A synthetic profile.
    Synthetic(Spec),
}

/// The parsed command line.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Args {
    /// The profile.
    pub profile: ProfileArg,
    /// The scratch directory: the write file and the read pool.
    pub scratch: PathBuf,
    /// The replay log, created new.
    pub log: PathBuf,
    /// Written when the replay settles.
    pub ready_file: Option<PathBuf>,
    /// The replay stops when this appears.
    pub stop_file: Option<PathBuf>,
    /// Stop after this many seconds.
    pub max_duration: Option<u64>,
    /// The most memory the hold holds.
    pub max_hold: u64,
    /// The read pool's size.
    pub read_pool: u64,
}

/// What the command line asks for.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Invocation {
    /// `--help`.
    Help,
    /// A replay.
    Run(Box<Args>),
}

fn utf8<'a>(v: &'a OsStr, what: &str) -> Result<&'a str, String> {
    v.to_str()
        .ok_or_else(|| format!("{what} is not valid Unicode"))
}

fn once<T>(slot: &mut Option<T>, flag: &str, v: T) -> Result<(), String> {
    if slot.replace(v).is_some() {
        Err(format!("{flag} is given twice"))
    } else {
        Ok(())
    }
}

/// Parses the arguments, without the program name.
pub fn parse_args(args: &[OsString]) -> Result<Invocation, String> {
    let mut profile = None;
    let (mut scratch, mut log, mut ready, mut stop) = (None, None, None, None);
    let (mut max_duration, mut max_hold, mut read_pool) = (None, None, None);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let flag = utf8(a, "an argument")?;
        if flag == "--help" {
            return Ok(Invocation::Help);
        }
        let v = it
            .next()
            .ok_or_else(|| format!("{flag} needs a value"))?
            .as_os_str();
        let path = || {
            if v.is_empty() {
                Err(format!("{flag} needs a non-empty path"))
            } else {
                Ok(PathBuf::from(v))
            }
        };
        match flag {
            "--profile" => once(&mut profile, flag, ProfileArg::File(path()?))?,
            "--synthetic" => once(
                &mut profile,
                flag,
                ProfileArg::Synthetic(Spec::parse(utf8(v, flag)?)?),
            )?,
            "--scratch" => once(&mut scratch, flag, path()?)?,
            "--log" => once(&mut log, flag, path()?)?,
            "--ready-file" => once(&mut ready, flag, path()?)?,
            "--stop-file" => once(&mut stop, flag, path()?)?,
            "--max-duration" => {
                let s = utf8(v, flag)?
                    .parse::<u64>()
                    .ok()
                    .filter(|&s| s > 0)
                    .ok_or_else(|| format!("{flag} takes a positive number of seconds"))?;
                once(&mut max_duration, flag, s)?;
            }
            "--max-hold" => once(&mut max_hold, flag, parse_bytes(utf8(v, flag)?)?)?,
            "--read-pool" => {
                let b = parse_bytes(utf8(v, flag)?)?;
                if b < MIN_READ_POOL {
                    return Err(format!("{flag} is at least 64 MiB"));
                }
                once(&mut read_pool, flag, b)?;
            }
            other => return Err(format!("unknown argument '{other}'")),
        }
    }
    Ok(Invocation::Run(Box::new(Args {
        profile: profile.ok_or("one of --profile and --synthetic is required")?,
        scratch: scratch.ok_or("--scratch is required")?,
        log: log.ok_or("--log is required")?,
        ready_file: ready,
        stop_file: stop,
        max_duration,
        max_hold: max_hold.unwrap_or(DEFAULT_MAX_HOLD),
        read_pool: read_pool.unwrap_or(DEFAULT_READ_POOL),
    })))
}

/// What the generator needs from the machine.
pub trait Rig {
    /// The sampler.
    type S: Sampler;
    /// The load.
    type A: Actuators;
    /// The meter.
    type M: Meter;
    /// The meter: the generator's own processor time, and available physical memory for the hold.
    fn meter(&self) -> &Self::M;
    /// The logical processors (one CPU worker each).
    fn workers(&self) -> usize;
    /// Starts the sampler.
    fn sampler(&mut self) -> Result<Self::S, String>;
    /// Starts the load.
    fn load(&mut self, cfg: &LoadConfig) -> Result<Self::A, String>;
}

/// The real machine: typeperf, the real load, and the given `Meter`.
#[derive(Debug)]
pub struct SystemRig<M: Meter> {
    meter: Arc<M>,
}

impl<M: Meter> SystemRig<M> {
    /// Wraps the machine's `Meter`.
    pub fn new(meter: M) -> SystemRig<M> {
        SystemRig {
            meter: Arc::new(meter),
        }
    }
}

impl<M: Meter> Rig for SystemRig<M> {
    type S = TypeperfSampler;
    type A = Load;
    type M = M;

    fn meter(&self) -> &M {
        &self.meter
    }

    fn workers(&self) -> usize {
        std::thread::available_parallelism().map_or(1, std::num::NonZero::get)
    }

    fn sampler(&mut self) -> Result<TypeperfSampler, String> {
        TypeperfSampler::start()
    }

    fn load(&mut self, cfg: &LoadConfig) -> Result<Load, String> {
        Load::start(self.meter.clone(), cfg)
    }
}

/// Writes the ready file `{"settled":<ms>}` ([MP §9.4]) atomically: into `<path>.tmp`, synced, then renamed into
/// place, so a driver that polls for the file never reads it empty or half written.
fn write_ready(path: &Path, settled: u64) -> Result<(), String> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    let io = |e: std::io::Error| format!("the ready file: {e}");
    let mut f = std::fs::File::create(&tmp).map_err(io)?;
    writeln!(f, "{}", json!({ "settled": settled }))
        .and_then(|()| f.sync_all())
        .map_err(io)?;
    drop(f);
    std::fs::rename(&tmp, path).map_err(io)
}

/// Runs the replay of `a` and returns the verdict of its whole settled replay.
fn run<R: Rig>(a: &Args, rig: &mut R, out: &mut dyn Write) -> Result<Verdict, String> {
    let profile = match &a.profile {
        ProfileArg::File(p) => {
            let bytes = std::fs::read(p).map_err(|e| format!("the profile: {e}"))?;
            fixture::decode(&bytes).map_err(|e| format!("the profile: {e}"))?
        }
        ProfileArg::Synthetic(s) => s.profile()?,
    };
    for p in [&a.stop_file, &a.ready_file].into_iter().flatten() {
        if p.exists() {
            return Err(format!(
                "{} exists already: remove it before starting",
                p.display()
            ));
        }
    }
    let mut log = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&a.log)
        .map_err(|e| format!("the replay log {}: {e}", a.log.display()))?;
    let cfg = LoadConfig {
        scratch: a.scratch.clone(),
        workers: rig.workers(),
        ram_target: LOADED_TARGET,
        max_hold: a.max_hold,
        read_pool: profile.reads_disk().then_some(a.read_pool),
    };
    let _ = writeln!(
        out,
        "loadgen: {} workers; preparing the load{}",
        cfg.workers,
        if cfg.read_pool.is_some() {
            " and its read pool"
        } else {
            ""
        }
    );
    let mut load = rig.load(&cfg)?;
    let mut sampler = match rig.sampler() {
        Ok(s) => s,
        Err(e) => {
            let _ = load.finish();
            return Err(e);
        }
    };
    let _ = writeln!(out, "loadgen: replaying");
    let settings = Settings {
        workers: cfg.workers,
        ram_target: LOADED_TARGET,
        settle_ms: SETTLE_MS,
        max_ms: a.max_duration.map(|s| s.saturating_mul(1_000)),
    };
    let stop_file = a.stop_file.clone();
    let ready_file = a.ready_file.clone();
    let outcome = replay(
        &profile,
        &mut sampler,
        &mut load,
        rig.meter(),
        &settings,
        Io {
            log: &mut log,
            stop: &mut || stop_file.as_ref().is_some_and(|p| p.exists()),
            on_settle: &mut |t| {
                if let Some(p) = &ready_file {
                    write_ready(p, t)?;
                }
                let _ = writeln!(out, "loadgen: settled");
                Ok(())
            },
        },
    );
    drop(sampler);
    let finished = load.finish();
    let outcome = outcome?;
    finished?;
    drop(log);
    let v = replay_verdict(&read_log_file(&a.log)?);
    OpenOptions::new()
        .append(true)
        .open(&a.log)
        .and_then(|mut f| writeln!(f, "{}", v.to_json()))
        .map_err(|e| format!("the replay log: {e}"))?;
    let _ = writeln!(out, "loadgen: stopped ({})", outcome.ending.as_str());
    Ok(v)
}

/// The command line ([MP §9.4]): parses `args` (without the program name), runs the replay on `rig`, prints the
/// verdict as one JSON line to `out`, and returns the exit code.
pub fn cli<R: Rig>(args: &[OsString], rig: &mut R, out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    let a = match parse_args(args) {
        Ok(Invocation::Help) => {
            let _ = out.write_all(USAGE.as_bytes());
            return EXIT_PASS;
        }
        Ok(Invocation::Run(a)) => a,
        Err(e) => {
            let _ = writeln!(err, "loadgen: {e}");
            let _ = err.write_all(USAGE.as_bytes());
            return EXIT_USAGE;
        }
    };
    match run(&a, rig, out) {
        Ok(v) => {
            let _ = writeln!(out, "{}", v.to_json());
            if v.pass() { EXIT_PASS } else { EXIT_FAIL }
        }
        Err(e) => {
            let _ = writeln!(err, "loadgen: {e}");
            EXIT_ERROR
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::load::replay::tests::{Plant, SimLoad, SimMeter, SimSampler};
    use crate::testkit::scratch_dir;
    use std::sync::Mutex;

    struct SimRig {
        plant: Arc<Mutex<Plant>>,
        meter: SimMeter,
        started: Vec<LoadConfig>,
    }

    impl SimRig {
        fn new(plant: Arc<Mutex<Plant>>) -> SimRig {
            SimRig {
                meter: SimMeter(plant.clone()),
                plant,
                started: Vec::new(),
            }
        }
    }

    impl Rig for SimRig {
        type S = SimSampler;
        type A = SimLoad;
        type M = SimMeter;
        fn meter(&self) -> &SimMeter {
            &self.meter
        }
        fn workers(&self) -> usize {
            4
        }
        fn sampler(&mut self) -> Result<SimSampler, String> {
            Ok(SimSampler(self.plant.clone()))
        }
        fn load(&mut self, cfg: &LoadConfig) -> Result<SimLoad, String> {
            self.started.push(cfg.clone());
            Ok(SimLoad(self.plant.clone()))
        }
    }

    fn os(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    fn run_cli(args: &[&str], rig: &mut SimRig) -> (u8, String, String) {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = cli(&os(args), rig, &mut out, &mut err);
        (
            code,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    #[test]
    fn parses_its_arguments() {
        let Invocation::Run(a) = parse_args(&os(&[
            "--synthetic",
            "cpu=20",
            "--scratch",
            "s",
            "--log",
            "l",
            "--stop-file",
            "x",
            "--max-duration",
            "90",
            "--read-pool",
            "128MiB",
        ]))
        .unwrap() else {
            panic!("help")
        };
        assert_eq!(
            a.profile,
            ProfileArg::Synthetic(Spec::parse("cpu=20").unwrap())
        );
        assert_eq!(
            (a.max_duration, a.read_pool, a.max_hold),
            (Some(90), 128 << 20, DEFAULT_MAX_HOLD)
        );
        assert_eq!(a.stop_file, Some(PathBuf::from("x")));
        assert_eq!(
            parse_args(&os(&["--log", "l", "--help"])),
            Ok(Invocation::Help)
        );
        for (bad, why) in [
            (&["--scratch", "s", "--log", "l"][..], "required"),
            (
                &[
                    "--profile",
                    "p",
                    "--synthetic",
                    "",
                    "--scratch",
                    "s",
                    "--log",
                    "l",
                ][..],
                "twice",
            ),
            (&["--profile", "p", "--log", "l"][..], "--scratch"),
            (&["--profile", "p", "--scratch", "s"][..], "--log"),
            (&["--profile", ""][..], "non-empty"),
            (&["--max-duration", "0"][..], "positive"),
            (&["--read-pool", "1MiB"][..], "64 MiB"),
            (&["--synthetic", "cpu=900"][..], "percentage"),
            (&["--log"][..], "needs a value"),
            (&["--frobnicate", "1"][..], "unknown"),
        ] {
            let e = parse_args(&os(bad)).unwrap_err();
            assert!(e.contains(why), "{bad:?}: {e}");
        }
    }

    #[test]
    fn replays_settles_and_passes() {
        let d = scratch_dir("loadgen-cli");
        let log = d.join("replay.jsonl");
        let ready = d.join("ready");
        let plant = Plant::new([6.0, 1e6, 1e6], 9_000_000_000, 0.9);
        let mut rig = SimRig::new(plant);
        let args = [
            "--synthetic",
            "seconds=120,step=20,read=8MB,write=8MB",
            "--scratch",
            d.to_str().unwrap(),
            "--log",
            log.to_str().unwrap(),
            "--ready-file",
            ready.to_str().unwrap(),
            "--max-duration",
            "240",
        ];
        let (code, out, err) = run_cli(&args, &mut rig);
        assert_eq!(code, EXIT_PASS, "{out}{err}");
        assert!(
            out.contains("loadgen: settled") && out.contains("stopped (max-duration)"),
            "{out}"
        );
        let verdict: serde_json::Value = serde_json::from_str(out.lines().last().unwrap()).unwrap();
        assert_eq!(verdict["pass"], true);
        assert_eq!(verdict["samples"], 180);
        let text = std::fs::read_to_string(&log).unwrap();
        assert_eq!(text.lines().last().unwrap(), out.lines().last().unwrap());
        let settled: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&ready).unwrap()).unwrap();
        assert_eq!(settled["settled"], verdict["settled"]);
        assert!(!d.join("ready.tmp").exists());
        assert_eq!(rig.started[0].read_pool, Some(DEFAULT_READ_POOL));
        assert_eq!(rig.started[0].workers, 4);
        // The log and the ready file are never overwritten.
        let (code, _, err) = run_cli(&args, &mut rig);
        assert_eq!(code, EXIT_ERROR);
        assert!(err.contains("exists already"), "{err}");
        std::fs::remove_file(&ready).unwrap();
        let (code, _, err) = run_cli(&args, &mut rig);
        assert_eq!(code, EXIT_ERROR);
        assert!(err.contains("the replay log"), "{err}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn an_unreproduced_profile_exits_one() {
        let d = scratch_dir("loadgen-fail");
        let log = d.join("replay.jsonl");
        let plant = Plant::new([90.0, 0.0, 0.0], 9_000_000_000, 1.0);
        let mut rig = SimRig::new(plant);
        let (code, out, _) = run_cli(
            &[
                "--synthetic",
                "read=0,write=0",
                "--scratch",
                d.to_str().unwrap(),
                "--log",
                log.to_str().unwrap(),
                "--max-duration",
                "150",
            ],
            &mut rig,
        );
        assert_eq!(code, EXIT_FAIL, "{out}");
        assert!(out.contains("\"pass\":false"));
        assert_eq!(rig.started[0].read_pool, None);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_bad_fixture_or_help() {
        let d = scratch_dir("loadgen-bad");
        let bad = d.join("bad.load");
        std::fs::write(&bad, b"not a fixture").unwrap();
        let mut rig = SimRig::new(Plant::new([0.0; 3], 9_000_000_000, 1.0));
        let (code, _, err) = run_cli(
            &[
                "--profile",
                bad.to_str().unwrap(),
                "--scratch",
                d.to_str().unwrap(),
                "--log",
                "unused",
            ],
            &mut rig,
        );
        assert_eq!(code, EXIT_ERROR);
        assert!(err.contains("the profile"), "{err}");
        let (code, out, _) = run_cli(&["--help"], &mut rig);
        assert_eq!((code, out.starts_with("usage: loadgen")), (EXIT_PASS, true));
        let (code, _, err) = run_cli(&["--scratch"], &mut rig);
        assert_eq!(code, EXIT_USAGE);
        assert!(err.contains("usage:"));
        let _ = std::fs::remove_dir_all(&d);
    }
}
