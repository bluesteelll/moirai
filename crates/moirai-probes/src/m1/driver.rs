//! The driver contract of [MP §9.7] as merged (R-27 Q3), shared by the
//! drivers of measurements 1 and 2: the arguments, the condition from the load log, the record written directly in
//! `--out`, one process per run, and a loaded run's replay verdict. It lives under `m1/` only because this unit's lock
//! set holds no shared path; the follow-up of measurements 10 and 14 may move it to a `crate::driver` (R-31 F-4).
//!
//! **Arguments.** `--host laptop|hosted` and `--condition idle|loaded|synthetic`, required for a recording run and
//! accepted with `--smoke`; `--load-log <file>`, required for a loaded or synthetic run; `--quantity <name>`, one of the
//! driver's; `--commit <sha>` and `--toolchain <text>`, required in every mode but `--help`; `--out <dir>`, required
//! unless `--smoke` and refused with it; `--smoke`; `--help`. Anything else, `--scratch` included, is a usage error: every
//! file other than the records lies below the working directory. Exit 0 done, 1 a run or arm failed or a record could
//! not be written, 2 a usage error.

use crate::condition::{Condition, ConditionKind};
use crate::host::HostKind;
use crate::load::validate::{COVER_WAIT, read_log_file, verdict_when_covered};
use crate::record::RunRecord;
use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Exit: done.
pub const EXIT_DONE: u8 = 0;
/// Exit: a run or arm failed, or a record could not be written.
pub const EXIT_FAILED: u8 = 1;
/// Exit: a usage error.
pub const EXIT_USAGE: u8 = 2;

/// A driver's parsed arguments.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Args {
    /// `--host`.
    pub host: Option<HostKind>,
    /// `--condition`.
    pub condition: Option<ConditionKind>,
    /// `--load-log`.
    pub load_log: Option<PathBuf>,
    /// `--out`.
    pub out: Option<PathBuf>,
    /// `--quantity`.
    pub quantity: Option<String>,
    /// `--commit`.
    pub commit: Option<String>,
    /// `--toolchain`.
    pub toolchain: Option<String>,
    /// `--smoke`.
    pub smoke: bool,
    /// `--help`.
    pub help: bool,
}

/// Parses `args` (without the program name) against the contract; `quantities` are the driver's.
pub fn parse(args: &[OsString], quantities: &[&str]) -> Result<Args, String> {
    let mut a = Args::default();
    let mut it = args.iter();
    while let Some(flag) = it.next() {
        let flag = flag.to_str().ok_or("an argument is not valid Unicode")?;
        let mut value = |what: &str| -> Result<String, String> {
            it.next()
                .and_then(|v| v.to_str())
                .map(str::to_string)
                .ok_or_else(|| format!("{what} needs a value"))
        };
        match flag {
            "--host" => {
                let v = value("--host")?;
                a.host = Some(HostKind::parse(&v).ok_or(format!("unknown host '{v}'"))?);
            }
            "--condition" => {
                let v = value("--condition")?;
                a.condition =
                    Some(ConditionKind::parse(&v).ok_or(format!("unknown condition '{v}'"))?);
            }
            "--load-log" => a.load_log = Some(value("--load-log")?.into()),
            "--out" => a.out = Some(value("--out")?.into()),
            "--quantity" => a.quantity = Some(value("--quantity")?),
            "--commit" => a.commit = Some(value("--commit")?),
            "--toolchain" => a.toolchain = Some(value("--toolchain")?),
            "--smoke" => a.smoke = true,
            "--help" | "-h" => a.help = true,
            other => return Err(format!("unknown argument '{other}'")),
        }
    }
    if let Some(q) = &a.quantity
        && !quantities.contains(&q.as_str())
    {
        return Err(format!("unknown quantity '{q}'"));
    }
    if a.help {
        return Ok(a);
    }
    if a.commit.is_none() || a.toolchain.is_none() {
        return Err("--commit and --toolchain are required".into());
    }
    if a.smoke && a.out.is_some() {
        return Err("--smoke writes no record and takes no --out".into());
    }
    if !a.smoke && a.out.is_none() {
        return Err("a recording run needs --out".into());
    }
    if !a.smoke && (a.host.is_none() || a.condition.is_none()) {
        return Err("a recording run needs --host and --condition".into());
    }
    if a.condition.is_some_and(|c| c != ConditionKind::Idle) && a.load_log.is_none() {
        return Err("a loaded or synthetic run needs --load-log".into());
    }
    Ok(a)
}

/// The run's condition: idle, or the load log header's, which must agree with `--condition` ([MP §9.7]). The error
/// carries its exit code.
pub fn condition(a: &Args) -> Result<Condition, (u8, String)> {
    match (a.condition, &a.load_log) {
        (Some(ConditionKind::Idle) | None, _) => Ok(Condition::Idle),
        (Some(kind), Some(log)) => match read_log_file(log) {
            Ok(l) if l.header.condition().kind() == kind => Ok(l.header.condition()),
            Ok(l) => Err((
                EXIT_USAGE,
                format!(
                    "--condition {} but the load log's header gives {}",
                    kind.as_str(),
                    l.header.condition().kind().as_str()
                ),
            )),
            Err(e) => Err((EXIT_FAILED, format!("the load log: {e}"))),
        },
        (Some(_), None) => Err((
            EXIT_USAGE,
            "a loaded or synthetic run needs --load-log".into(),
        )),
    }
}

/// Milliseconds since the Unix epoch.
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// Records a loaded run's replay verdict over its window `[t0, t1]` (ms), read from the load log ([MP §9.7]); other
/// conditions are left alone.
pub fn set_load_verdict(
    a: &Args,
    rec: &mut RunRecord,
    t0: u64,
    t1: u64,
    err: &mut dyn Write,
    tag: &str,
) {
    if a.condition == Some(ConditionKind::Loaded)
        && let Some(log) = &a.load_log
    {
        match verdict_when_covered(log, t0, t1, COVER_WAIT) {
            Ok((_, v)) => rec.set_load_replay(v.pass()),
            Err(e) => {
                let _ = writeln!(err, "{tag}: the load log's verdict: {e}");
                rec.set_load_replay(false);
            }
        }
    }
}

/// Writes `rec` directly in `dir`, created when missing, as the new file `<raw file name>` ([MP §7.2], [MP §9.7]):
/// never over an existing file, streamed, synced, and removed when its writing fails.
pub fn write_record(rec: &RunRecord, dir: &Path) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let name = rec.raw_file_name().map_err(std::io::Error::other)?;
    let path = dir.join(name);
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    let finish = |file: std::fs::File| -> std::io::Result<()> {
        let mut w = std::io::BufWriter::new(file);
        rec.write_json(&mut w)?;
        w.write_all(b"\n")?;
        w.into_inner().map_err(|e| e.into_error())?.sync_all()
    };
    match finish(file) {
        Ok(()) => Ok(path),
        Err(e) => {
            let _ = std::fs::remove_file(&path);
            Err(e)
        }
    }
}

/// Without `--quantity`: each quantity runs in a child process of its own, in list order, with `args` plus
/// `--quantity <q>` ([MP §9.7] "One process per run"); returns the worst exit code, a child that cannot start counting
/// as failed.
pub fn each_in_own_process(
    args: &[OsString],
    quantities: &[&str],
    spawn: &mut dyn FnMut(&[OsString]) -> Result<u8, String>,
    out: &mut dyn Write,
    err: &mut dyn Write,
    tag: &str,
) -> u8 {
    let mut worst = EXIT_DONE;
    for q in quantities {
        let mut child = args.to_vec();
        child.push("--quantity".into());
        child.push(OsString::from(q));
        let code = match spawn(&child) {
            Ok(c) => c,
            Err(e) => {
                let _ = writeln!(err, "{tag} {q}: {e}");
                EXIT_FAILED
            }
        };
        let _ = writeln!(out, "{tag} {q} exit {code}");
        worst = worst.max(code);
    }
    worst
}

/// The `quantities` of the `[[measurement]]` block with `n = <n>` in the night's plan (`xtask/measure-night.toml`): a
/// minimal scan for the tests that bind a driver to its list. An array may span lines; comments are skipped.
#[cfg(test)]
pub(crate) fn plan_quantities(text: &str, n: u32) -> Option<Vec<String>> {
    let lines: Vec<&str> = text
        .lines()
        .map(|l| match l.find('#') {
            Some(i) if !l[..i].contains('"') => &l[..i],
            _ => l,
        })
        .collect();
    for block in lines.join("\n").split("[[measurement]]").skip(1) {
        let block = block.split("\n[").next().unwrap_or(block);
        let has_n = block.lines().any(|l| {
            let mut kv = l.splitn(2, '=');
            kv.next().map(str::trim) == Some("n")
                && kv.next().map(str::trim) == Some(n.to_string().as_str())
        });
        if !has_n {
            continue;
        }
        let at = block.find("quantities")?;
        let rest = &block[at..];
        let open = rest.find('[')?;
        let close = rest[open..].find(']')? + open;
        return Some(
            rest[open + 1..close]
                .split('"')
                .skip(1)
                .step_by(2)
                .map(str::to_string)
                .collect(),
        );
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::tests::sample_record;
    use crate::testkit::scratch_dir;

    const QS: [&str; 2] = ["cycle.e64mib", "close.e64mib"];

    fn os(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    /// The runner's recording and smoke command lines (`xtask/src/measure/mod.rs` `driver_cmd`) parse as given, and a
    /// loaded run takes `--load-log`.
    #[test]
    fn parse_takes_the_runner_command_lines() {
        let rec = os(&[
            "--host",
            "laptop",
            "--condition",
            "idle",
            "--quantity",
            "close.e64mib",
            "--commit",
            "0123abcd",
            "--toolchain",
            "rustc 1.98.1",
            "--out",
            "D:/private/measurements/1",
        ]);
        let a = parse(&rec, &QS).unwrap();
        assert_eq!(
            a,
            Args {
                host: Some(HostKind::Laptop),
                condition: Some(ConditionKind::Idle),
                load_log: None,
                out: Some("D:/private/measurements/1".into()),
                quantity: Some("close.e64mib".into()),
                commit: Some("0123abcd".into()),
                toolchain: Some("rustc 1.98.1".into()),
                smoke: false,
                help: false,
            }
        );
        let mut smoke = rec[..10].to_vec();
        smoke.push("--smoke".into());
        let a = parse(&smoke, &QS).unwrap();
        assert!(a.smoke && a.out.is_none() && a.host == Some(HostKind::Laptop));
        let a = parse(&os(&["--commit", "c", "--toolchain", "t", "--smoke"]), &QS).unwrap();
        assert!(a.smoke && a.quantity.is_none() && a.host.is_none());
        let loaded = os(&[
            "--host",
            "laptop",
            "--condition",
            "loaded",
            "--load-log",
            "l.jsonl",
            "--commit",
            "c",
            "--toolchain",
            "t",
            "--out",
            "o",
        ]);
        assert_eq!(
            parse(&loaded, &QS).unwrap().load_log,
            Some(PathBuf::from("l.jsonl"))
        );
        assert!(parse(&os(&["--help"]), &QS).unwrap().help);
    }

    /// Each break of the contract is a usage error with its reason.
    #[test]
    fn parse_refuses_what_the_contract_refuses() {
        let ct = ["--commit", "c", "--toolchain", "t"];
        let run = ["--host", "laptop", "--condition", "idle", "--out", "o"];
        let cases: Vec<(Vec<&str>, &str)> = vec![
            (
                [&run[..], &["--toolchain", "t"]].concat(),
                "--commit and --toolchain",
            ),
            (
                [&run[..], &["--commit", "c"]].concat(),
                "--commit and --toolchain",
            ),
            (vec!["--smoke"], "--commit and --toolchain"),
            (vec!["--smoke", "--commit", "c"], "--commit and --toolchain"),
            (
                [&ct[..], &["--smoke", "--out", "o"]].concat(),
                "takes no --out",
            ),
            (
                [&ct[..], &["--host", "laptop", "--condition", "idle"]].concat(),
                "needs --out",
            ),
            (
                [&ct[..], &run[..], &["--quantity", "cycle.e1"]].concat(),
                "unknown quantity",
            ),
            (
                [&ct[..], &run[..], &["--bogus"]].concat(),
                "unknown argument '--bogus'",
            ),
            (
                [&ct[..], &run[..], &["--scratch", "s"]].concat(),
                "unknown argument '--scratch'",
            ),
            (
                [&ct[..], &run[..], &["--writer"]].concat(),
                "unknown argument '--writer'",
            ),
            (
                [&ct[..], &["--condition", "idle", "--out", "o"]].concat(),
                "needs --host and --condition",
            ),
            (
                [&ct[..], &["--host", "laptop", "--out", "o"]].concat(),
                "needs --host and --condition",
            ),
            (
                [
                    &ct[..],
                    &["--host", "laptop", "--condition", "loaded", "--out", "o"],
                ]
                .concat(),
                "needs --load-log",
            ),
            (
                [&ct[..], &["--smoke", "--condition", "synthetic"]].concat(),
                "needs --load-log",
            ),
            (
                [&ct[..], &["--host", "desk", "--smoke"]].concat(),
                "unknown host",
            ),
            ([&ct[..], &["--out"]].concat(), "needs a value"),
        ];
        for (args, why) in cases {
            let e = parse(&os(&args), &QS).unwrap_err();
            assert!(e.contains(why), "{args:?}: {e}");
        }
    }

    /// A record goes directly in `--out`, which is created when missing, as a new file; an existing file of that name
    /// is an error and stays unchanged.
    #[test]
    fn records_go_directly_into_out_and_never_over_a_file() {
        let dir = scratch_dir("m1-records");
        let out = dir.join("measurements").join("1");
        let rec = sample_record();
        let p = write_record(&rec, &out).unwrap();
        assert_eq!(p.parent(), Some(out.as_path()));
        assert_eq!(
            p.file_name().unwrap().to_str().unwrap(),
            rec.raw_file_name().unwrap()
        );
        let v: serde_json::Value = serde_json::from_slice(&std::fs::read(&p).unwrap()).unwrap();
        assert_eq!(RunRecord::from_json(&v).unwrap(), rec);
        std::fs::write(&p, "kept").unwrap();
        assert!(write_record(&rec, &out).is_err());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "kept");
        assert_eq!(std::fs::read_dir(&out).unwrap().count(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A synthetic or loaded run's condition is the load log header's; a `--condition` the header contradicts is a usage
    /// error, an unreadable log a failure ([MP §9.7]).
    #[test]
    fn the_condition_comes_from_the_load_log() {
        let dir = scratch_dir("m1-cond");
        let log = dir.join("load.log");
        let header = serde_json::json!({
            "schema": crate::load::replay::LOG_SCHEMA,
            "source": {"kind": "synthetic", "spec": "seconds=300,cpu=50"},
            "interval_ms": 1000,
            "sample_ms": 1000,
            "started": 0,
        });
        std::fs::write(&log, format!("{header}\n")).unwrap();
        let args = |cond: ConditionKind, log: &Path| Args {
            condition: Some(cond),
            load_log: Some(log.to_path_buf()),
            ..Args::default()
        };
        assert_eq!(
            condition(&args(ConditionKind::Synthetic, &log)),
            Ok(Condition::Synthetic {
                description: "moirai-probes-bin loadgen, synthetic profile seconds=300,cpu=50"
                    .into()
            })
        );
        let e = condition(&args(ConditionKind::Loaded, &log)).unwrap_err();
        assert_eq!(e.0, EXIT_USAGE);
        assert!(e.1.contains("header gives synthetic"), "{}", e.1);
        let e = condition(&args(ConditionKind::Loaded, &dir.join("none.log"))).unwrap_err();
        assert_eq!(e.0, EXIT_FAILED);
        assert_eq!(
            condition(&Args {
                condition: Some(ConditionKind::Idle),
                ..Args::default()
            }),
            Ok(Condition::Idle)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
