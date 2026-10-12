//! The committed aggregate of a measurement, `docs/measurements/m0/<n>.md` ([MP §7.4]).
//!
//! An aggregate holds summaries only: no samples, no paths, and no user name, host name, volume serial, machine GUID,
//! `BootId` or process command line (the pre-commit scrub refuses them, `docs/m0/PLAN.md` §3.2 WP-03). The run
//! records it is rendered from carry none of those either ([MP §7.1]).
//!
//! [`cli`] is `moirai-probes-bin aggregate` (WP-50): it reads a measurement's records from files and directories and
//! renders its aggregate with [`render`].

use crate::host::{HostRecord, HostSnapshot};
use crate::idle::{IDLE_SCHEMA, IdleRecord, IdleWindow};
use crate::record::{Header, RUN_SCHEMA, RunRecord};
use crate::units::{format_ns, format_value};
use serde_json::Value;
use std::ffi::OsString;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// `yes` or `no`.
fn yn(b: bool) -> &'static str {
    if b { "yes" } else { "no" }
}

/// A table cell: `|` and line breaks would break the row, so they are replaced.
fn cell(s: &str) -> String {
    s.replace('|', "/").replace(['\r', '\n'], " ")
}

fn windows_cell(s: &HostSnapshot) -> String {
    match &s.windows {
        Ok(v) => cell(v),
        Err(_) => "unread".to_string(),
    }
}

fn defender_cell(s: &HostSnapshot) -> String {
    match &s.defender {
        Ok(d) => cell(&format!(
            "{} / {} / {}, real-time {}",
            d.product,
            d.engine,
            d.signatures,
            if d.realtime { "on" } else { "off" }
        )),
        Err(_) => "unread".to_string(),
    }
}

fn host_cells(h: &HostRecord) -> (String, String) {
    let w = if h.start.windows == h.end.windows {
        windows_cell(&h.start)
    } else {
        format!("{} → {}", windows_cell(&h.start), windows_cell(&h.end))
    };
    let d = if h.start.defender == h.end.defender {
        defender_cell(&h.start)
    } else {
        format!("{} → {}", defender_cell(&h.start), defender_cell(&h.end))
    };
    (w, d)
}

/// One row of the runs table.
fn run_row(w: &mut String, i: usize, h: &Header<'_>, valid: bool, exit_grade: bool) {
    let (win, def) = host_cells(h.host);
    let commit: String = h.commit.chars().take(12).collect();
    let _ = writeln!(
        w,
        "| {i} | {} | {} | {} | {win} | {def} | {} | {} | {} | {} | {} |",
        cell(h.quantity),
        h.condition.kind().as_str(),
        h.host.kind.as_str(),
        cell(h.toolchain),
        cell(&commit),
        cell(h.started),
        yn(valid),
        yn(exit_grade),
    );
}

/// The per-window deltas of an idle observation, or `unread`.
fn window_deltas(windows: &[IdleWindow], f: impl Fn(&IdleWindow) -> Result<String, ()>) -> String {
    windows
        .iter()
        .map(|w| f(w).unwrap_or_else(|()| "unread".to_string()))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Renders the aggregate of measurement `measurement` from its run records and idle observations ([MP §7.4]).
/// `title` is the measurement's name; `decision` is the drafted decision for WP-81a, or empty while none is drafted.
/// Records of other measurements are refused. Runs are numbered in order, the run records first.
pub fn render(
    measurement: u32,
    title: &str,
    records: &[RunRecord],
    idle: &[IdleRecord],
    decision: &str,
) -> Result<String, String> {
    if let Some(m) = records
        .iter()
        .map(|r| r.measurement)
        .chain(idle.iter().map(|r| r.measurement))
        .find(|&m| m != measurement)
    {
        return Err(format!(
            "a record of measurement {m} is not part of measurement {measurement}"
        ));
    }
    let mut out = String::new();
    let w = &mut out;
    let _ = writeln!(w, "# Measurement {measurement}: {}", cell(title));
    let _ = writeln!(w);
    let _ = writeln!(
        w,
        "Aggregates under `docs/spec/measurement-protocol.md` ([MP §7.4]). Values are medians over the repetitions \
         ([MP §3.4]); raw records stay in `/private/measurements/{measurement}/` and are not committed ([MP §7.2])."
    );
    let _ = writeln!(w);
    let _ = writeln!(w, "## Runs");
    let _ = writeln!(w);
    let _ = writeln!(
        w,
        "| Run | Quantity | Condition | Host | Windows | Defender (product / engine / signatures) | Toolchain | Commit | Started (UTC) | Valid | Exit grade |"
    );
    let _ = writeln!(w, "|---|---|---|---|---|---|---|---|---|---|---|");
    for (i, r) in records.iter().enumerate() {
        run_row(w, i + 1, &r.header(), r.valid(), r.exit_grade());
    }
    for (i, r) in idle.iter().enumerate() {
        run_row(
            w,
            records.len() + i + 1,
            &r.header(),
            r.valid(),
            r.exit_grade(),
        );
    }
    if !idle.is_empty() {
        let _ = writeln!(w);
        let _ = writeln!(w, "## Idle CPU");
        let _ = writeln!(w);
        let _ = writeln!(
            w,
            "Three windows of 10 minutes, each from 15 s after a request; the gate holds when every window reads zero \
             CPU time and zero context switches ([MP §4.7])."
        );
        let _ = writeln!(w);
        let _ = writeln!(
            w,
            "| Run | Quantity | CPU time per window | Context switches per window | Zero |"
        );
        let _ = writeln!(w, "|---|---|---|---|---|");
        for (i, r) in idle.iter().enumerate() {
            let cpu = window_deltas(&r.windows, |x| {
                x.cpu_ns.as_ref().map(|&v| format_ns(v)).map_err(|_| ())
            });
            let switches = window_deltas(&r.windows, |x| {
                x.context_switches
                    .as_ref()
                    .map(u64::to_string)
                    .map_err(|_| ())
            });
            let _ = writeln!(
                w,
                "| {} | {} | {cpu} | {switches} | {} |",
                records.len() + i + 1,
                cell(&r.quantity),
                yn(r.outcome().holds),
            );
        }
    }
    if !records.is_empty() {
        arms_section(w, records);
    }
    let invalid: Vec<(usize, Vec<String>)> = records
        .iter()
        .map(|r| (!r.exit_grade()).then(|| r.disqualifications()))
        .chain(
            idle.iter()
                .map(|r| (!r.exit_grade()).then(|| r.disqualifications())),
        )
        .enumerate()
        .filter_map(|(i, d)| d.map(|d| (i + 1, d)))
        .collect();
    if !invalid.is_empty() {
        let _ = writeln!(w);
        let _ = writeln!(w, "## Runs that decide nothing");
        let _ = writeln!(w);
        for (i, why) in invalid {
            let _ = writeln!(w, "- Run {i}: {}.", cell(&why.join("; ")));
        }
    }
    let _ = writeln!(w);
    let _ = writeln!(w, "## Decision for WP-81a");
    let _ = writeln!(w);
    if decision.trim().is_empty() {
        let _ = writeln!(w, "Not drafted yet.");
    } else {
        let _ = writeln!(w, "{}", decision.trim_end());
    }
    Ok(out)
}

/// The arms table of the run records ([MP §7.4]).
fn arms_section(w: &mut String, records: &[RunRecord]) {
    let _ = writeln!(w);
    let _ = writeln!(w, "## Arms");
    let _ = writeln!(w);
    let _ = writeln!(
        w,
        "| Run | Arm | Unit | Tier | n × r | Batch | p50 | p95 | p99 | max | Gated statistic | Spread over repetitions |"
    );
    let _ = writeln!(w, "|---|---|---|---|---|---|---|---|---|---|---|---|");
    let mut batched = false;
    for (i, r) in records.iter().enumerate() {
        for a in &r.arms {
            let Some(m) = a.medians() else { continue };
            let v = |x: u64| format_value(a.unit, x);
            let gated: Vec<String> = a
                .gated()
                .iter()
                .map(|s| format!("{} {}", s.as_str(), v(m.get(*s))))
                .collect();
            let spread: Vec<String> = a
                .gated()
                .iter()
                .map(|s| {
                    let per = a.per_rep(*s);
                    let lo = per.iter().copied().min().unwrap_or(0);
                    let hi = per.iter().copied().max().unwrap_or(0);
                    format!("{} {}", s.as_str(), v(hi - lo))
                })
                .collect();
            batched |= a.batch > 1;
            let _ = writeln!(
                w,
                "| {} | {} | {} | {} | {} × {} | {} | {} | {} | {} | {} | {} | {} |",
                i + 1,
                cell(&a.name),
                a.unit.as_str(),
                a.tier.as_str(),
                r.plan.n,
                r.plan.repetitions,
                a.batch,
                v(m.p50),
                v(m.p95),
                v(m.p99),
                v(m.max),
                gated.join("; "),
                spread.join("; "),
            );
        }
    }
    if batched {
        let _ = writeln!(w);
        let _ = writeln!(
            w,
            "A batch above 1 means each sample is the mean of that many consecutive operations, so the percentiles \
             describe batch means ([MP §4.5])."
        );
    }
}

/// The usage text of `moirai-probes-bin aggregate` ([MP §7.4]).
pub const AGGREGATE_USAGE: &str = "usage: aggregate --measurement <n> --title <text> [--decision <file>] [--out <file>] <record or directory>...
       aggregate --help
Renders a measurement's aggregate, docs/measurements/m0/<n>.md (docs/spec/measurement-protocol.md §7.4), from its run
records (moirai-probes/run/1) and idle observations (moirai-probes/idle/1), one JSON object per file; a directory's
*.json files are read in name order. --decision names a file holding the drafted decision for WP-81a. The text goes to
stdout, or with --out to that file, replaced, as UTF-8 with LF line endings. Exit: 0 rendered; 1 a file unreadable
or refused, a record of another measurement, or --out unwritable; 2 usage error.
";

/// The arguments of [`cli`].
#[derive(Debug, Default)]
struct AggregateArgs {
    measurement: Option<u32>,
    title: Option<String>,
    decision: Option<PathBuf>,
    out: Option<PathBuf>,
    inputs: Vec<PathBuf>,
}

fn parse_aggregate(args: &[OsString]) -> Result<AggregateArgs, String> {
    let mut a = AggregateArgs::default();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let flag = arg.to_string_lossy();
        let mut value = |what: &str| -> Result<OsString, String> {
            it.next()
                .cloned()
                .ok_or_else(|| format!("{what} needs a value"))
        };
        match flag.as_ref() {
            "--measurement" => {
                let v = value("--measurement")?;
                let n = v
                    .to_str()
                    .and_then(|s| s.parse::<u32>().ok())
                    .filter(|&n| n >= 1)
                    .ok_or_else(|| {
                        format!("--measurement '{}' is not a row number", v.display())
                    })?;
                a.measurement = Some(n);
            }
            "--title" => {
                let v = value("--title")?;
                a.title = Some(
                    v.to_str()
                        .ok_or("the title is not valid Unicode")?
                        .to_string(),
                );
            }
            "--decision" => a.decision = Some(value("--decision")?.into()),
            "--out" => a.out = Some(value("--out")?.into()),
            f if f.starts_with("--") => return Err(format!("unknown argument '{f}'")),
            _ => a.inputs.push(arg.into()),
        }
    }
    if a.measurement.is_none() || a.title.is_none() {
        return Err("give --measurement and --title".into());
    }
    if a.inputs.is_empty() {
        return Err("give at least one record or directory".into());
    }
    Ok(a)
}

/// The record files an input names: the file itself, or a directory's `*.json` files in name order.
fn record_files(input: &Path) -> Result<Vec<PathBuf>, String> {
    if !input.is_dir() {
        return Ok(vec![input.to_path_buf()]);
    }
    let mut files = Vec::new();
    for e in std::fs::read_dir(input).map_err(|e| e.to_string())? {
        let p = e.map_err(|e| e.to_string())?.path();
        if p.is_file() && p.extension().is_some_and(|x| x == "json") {
            files.push(p);
        }
    }
    files.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
    Ok(files)
}

/// One record file, read by its `schema`: a run record or an idle observation, each checked ([MP §7.3]).
enum AnyRecord {
    Run(Box<RunRecord>),
    Idle(Box<IdleRecord>),
}

fn read_record(path: &Path) -> Result<AnyRecord, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let v: Value = serde_json::from_slice(&bytes).map_err(|e| format!("not JSON: {e}"))?;
    match v.get("schema").and_then(Value::as_str) {
        Some(RUN_SCHEMA) => RunRecord::from_json(&v).map(|r| AnyRecord::Run(Box::new(r))),
        Some(IDLE_SCHEMA) => IdleRecord::from_json(&v).map(|r| AnyRecord::Idle(Box::new(r))),
        other => Err(format!("unknown schema {other:?}")),
    }
}

/// `moirai-probes-bin aggregate` ([MP §7.4]): reads the records the arguments name, renders the aggregate with
/// [`render`] and writes it to `out` or to the `--out` file; returns the exit code (0 rendered, 1 a file unreadable or
/// refused, a record of another measurement or an unwritable `--out`, 2 usage error). A file's name is reported on
/// `err` only, so no path reaches the aggregate.
pub fn cli(args: &[OsString], out: &mut dyn std::io::Write, err: &mut dyn std::io::Write) -> u8 {
    if args.first().is_some_and(|a| a == "--help") {
        let _ = out.write_all(AGGREGATE_USAGE.as_bytes());
        return 0;
    }
    let a = match parse_aggregate(args) {
        Ok(a) => a,
        Err(e) => {
            let _ = writeln!(err, "aggregate: {e}");
            let _ = err.write_all(AGGREGATE_USAGE.as_bytes());
            return 2;
        }
    };
    let (mut runs, mut idle) = (Vec::new(), Vec::new());
    for input in &a.inputs {
        let files = match record_files(input) {
            Ok(f) => f,
            Err(e) => {
                let _ = writeln!(err, "aggregate: {}: {e}", input.display());
                return 1;
            }
        };
        for f in files {
            match read_record(&f) {
                Ok(AnyRecord::Run(r)) => runs.push(*r),
                Ok(AnyRecord::Idle(r)) => idle.push(*r),
                Err(e) => {
                    let _ = writeln!(err, "aggregate: {}: refused: {e}", f.display());
                    return 1;
                }
            }
        }
    }
    let decision = match &a.decision {
        Some(p) => match std::fs::read_to_string(p) {
            Ok(t) => t,
            Err(e) => {
                let _ = writeln!(err, "aggregate: {}: {e}", p.display());
                return 1;
            }
        },
        None => String::new(),
    };
    let measurement = a.measurement.unwrap_or_default();
    let title = a.title.unwrap_or_default();
    let text = match render(measurement, &title, &runs, &idle, &decision) {
        Ok(t) => t,
        Err(e) => {
            let _ = writeln!(err, "aggregate: refused: {e}");
            return 1;
        }
    };
    match &a.out {
        Some(p) => match std::fs::write(p, text.as_bytes()) {
            Ok(()) => 0,
            Err(e) => {
                let _ = writeln!(err, "aggregate: {}: {e}", p.display());
                1
            }
        },
        None => match out.write_all(text.as_bytes()) {
            Ok(()) => 0,
            Err(e) => {
                let _ = writeln!(err, "aggregate: stdout: {e}");
                1
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::idle::tests::sample_idle;
    use crate::record::tests::sample_record;

    #[test]
    fn aggregate_lists_runs_and_arms_without_samples() {
        let a = sample_record();
        let mut b = sample_record();
        b.host.kind = crate::host::HostKind::Hosted;
        b.arms[1].batch = 3;
        let text = render(11, "physical floors", &[a.clone(), b], &[], "").unwrap();
        assert!(text.starts_with("# Measurement 11: physical floors\n"));
        assert!(text.contains("| 1 | spawn.empty | idle | laptop | 10.0.26200.6584 | 4.18.25080.5 / 1.1.25080.4 / 1.437.82.0, real-time on | 1.98.1 | 0123456789ab | 2026-10-04T09:30:00Z | yes | yes |"), "{text}");
        assert!(text.contains("| 1 | op | ns | t4 | 20 × 3 | 1 | 2.009 s | 2.018 s | 2.019 s | 2.019 s | max 2.019 s | max 2 ns |"), "{text}");
        assert!(text.contains("- Run 2: host kind hosted never decides."));
        assert!(text.contains("batch means"));
        assert!(text.contains("Not drafted yet."));
        assert!(!text.contains("2009000000"), "no sample appears");
        assert!(
            !text.contains("0123456789abcdef"),
            "only 12 digits of the commit"
        );
        assert!(!text.contains("## Idle CPU"));
        let drafted = render(11, "x", std::slice::from_ref(&a), &[], "Leader in M1.").unwrap();
        assert!(drafted.ends_with("Leader in M1.\n"));
        let mut other = a;
        other.measurement = 12;
        assert!(render(11, "x", &[other], &[], "").is_err());
    }

    #[test]
    fn idle_observations_get_their_own_table() {
        let quiet = sample_idle();
        let mut busy = sample_idle();
        busy.windows[1].cpu_ns = Ok(15_625_000);
        busy.windows[2].context_switches = Err("ETW session lost".into());
        busy.reasons = crate::idle::window_reasons(&busy.windows);
        let text = render(19, "MCP server", &[], &[quiet, busy], "").unwrap();
        assert!(
            text.contains("| 1 | idle-cpu.mcp | idle | laptop |"),
            "{text}"
        );
        assert!(
            text.contains("| 1 | idle-cpu.mcp | 0 ns, 0 ns, 0 ns | 0, 0, 0 | yes |"),
            "{text}"
        );
        assert!(
            text.contains("| 2 | idle-cpu.mcp | 0 ns, 15.625 ms, 0 ns | 0, 0, unread | no |"),
            "{text}"
        );
        assert!(text.contains("- Run 2: idle window 2: the context switches could not be read"));
        assert!(!text.contains("## Arms"), "no run record, no arms table");
        let mut other = sample_idle();
        other.measurement = 1;
        assert!(render(19, "x", &[], &[other], "").is_err());
        // Run records come first, then idle observations.
        let mut run = sample_record();
        run.measurement = 19;
        let text = render(19, "x", &[run], &[sample_idle()], "").unwrap();
        assert!(
            text.contains("| 2 | idle-cpu.mcp | 0 ns, 0 ns, 0 ns | 0, 0, 0 | yes |"),
            "{text}"
        );
    }

    #[test]
    fn version_changes_are_shown() {
        let mut r = sample_record();
        if let Ok(d) = &mut r.host.end.defender {
            d.signatures = "1.437.90.0".into();
        }
        let text = render(11, "t", &[r], &[], "").unwrap();
        assert!(text.contains(
            "1.437.82.0, real-time on → 4.18.25080.5 / 1.1.25080.4 / 1.437.90.0, real-time on"
        ));
        assert!(text.contains("changed during the run"));
    }

    fn os(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    /// Runs the command line; returns (exit, stdout, stderr).
    fn run(args: &[OsString]) -> (u8, String, String) {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = cli(args, &mut out, &mut err);
        (
            code,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    fn put(path: &Path, v: &Value) {
        std::fs::write(path, format!("{v}\n")).unwrap();
    }

    /// Two run records (one hosted) and an idle observation of measurement 11 in a directory, written out of name
    /// order beside a file that is no record.
    fn records_dir() -> (PathBuf, RunRecord, RunRecord, IdleRecord) {
        let dir = crate::testkit::scratch_dir("aggregate-cli");
        let a = sample_record();
        let mut b = sample_record();
        b.host.kind = crate::host::HostKind::Hosted;
        let mut idle = sample_idle();
        idle.measurement = 11;
        put(&dir.join("c-idle.json"), &idle.to_json());
        put(&dir.join("b-hosted.json"), &b.to_json());
        put(&dir.join("a-laptop.json"), &a.to_json());
        std::fs::write(dir.join("notes.txt"), "not a record").unwrap();
        (dir, a, b, idle)
    }

    /// `aggregate --measurement 11 --title t <dir>` prints `render` of the directory's records, read in name order:
    /// every element of [MP §7.4], and no sample and no path.
    #[test]
    fn cli_renders_the_records_of_a_directory_in_name_order() {
        let (dir, a, b, idle) = records_dir();
        let d = dir.to_str().unwrap();
        let (code, out, err) = run(&os(&["--measurement", "11", "--title", "t", d]));
        assert_eq!(code, 0, "{err}");
        let want = render(11, "t", &[a, b], &[idle], "").unwrap();
        assert_eq!(out, want);
        for element in [
            "# Measurement 11: t\n",
            "## Runs",
            "| 1 | spawn.empty | idle | laptop |",
            "| 2 | spawn.empty | idle | hosted |",
            "## Idle CPU",
            "## Arms",
            "## Runs that decide nothing",
            "## Decision for WP-81a",
            "Not drafted yet.",
        ] {
            assert!(out.contains(element), "{element}: {out}");
        }
        assert!(!out.contains("2009000000"), "no sample appears");
        assert!(
            !out.contains(d) && !out.contains("aggregate-cli"),
            "no path appears"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `--out` replaces the file with `render`'s text, UTF-8 with LF line endings, carrying the `--decision` text; nothing
    /// goes to stdout.
    #[test]
    fn cli_writes_the_out_file_with_the_decision() {
        let (dir, a, b, idle) = records_dir();
        let decision = dir.join("decision.txt");
        std::fs::write(&decision, "Leader in M1.\n").unwrap();
        let target = dir.join("11.md");
        std::fs::write(
            &target,
            "an older aggregate, longer than the new one".repeat(200),
        )
        .unwrap();
        let file = |p: &Path| p.to_str().unwrap().to_string();
        let (code, out, err) = run(&os(&[
            "--measurement",
            "11",
            "--title",
            "t",
            "--decision",
            &file(&decision),
            "--out",
            &file(&target),
            &file(&dir.join("a-laptop.json")),
            &file(&dir.join("b-hosted.json")),
            &file(&dir.join("c-idle.json")),
        ]));
        assert_eq!(code, 0, "{err}");
        assert_eq!(out, "", "nothing on stdout");
        let bytes = std::fs::read(&target).unwrap();
        let want = render(11, "t", &[a, b], &[idle], "Leader in M1.\n").unwrap();
        assert_eq!(bytes, want.as_bytes());
        assert!(want.ends_with("Leader in M1.\n"));
        assert!(!bytes.windows(2).any(|w| w == b"\r\n"), "LF line endings");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A file that is unreadable or refused, a record of another measurement and an unwritable `--out` exit 1; usage
    /// errors exit 2; `--help` prints the usage and exits 0.
    #[test]
    fn cli_refuses_bad_records_and_usage_errors() {
        let dir = crate::testkit::scratch_dir("aggregate-bad");
        let good = dir.join("good.json");
        put(&good, &sample_record().to_json());
        let unknown = dir.join("unknown.json");
        put(
            &unknown,
            &serde_json::json!({"schema": "moirai-probes/other/1"}),
        );
        let broken = dir.join("broken.json");
        std::fs::write(&broken, "{\"schema\": ").unwrap();
        let other = dir.join("other.json");
        let mut r12 = sample_record();
        r12.measurement = 12;
        put(&other, &r12.to_json());
        let s = |p: &Path| p.to_str().unwrap().to_string();
        let base = |rest: &[String]| {
            let mut v = os(&["--measurement", "11", "--title", "t"]);
            v.extend(rest.iter().map(OsString::from));
            v
        };
        for (args, why) in [
            (base(&[s(&unknown)]), "unknown schema"),
            (base(&[s(&broken)]), "not JSON"),
            (base(&[s(&dir.join("missing.json"))]), "missing.json"),
            (base(&[s(&good), s(&other)]), "measurement 12"),
            (
                base(&[
                    "--out".into(),
                    s(&dir.join("no-dir").join("11.md")),
                    s(&good),
                ]),
                "11.md",
            ),
        ] {
            let (code, out, err) = run(&args);
            assert_eq!(code, 1, "{args:?}: {err}");
            assert!(err.contains(why), "{args:?}: {err}");
            assert_eq!(out, "", "{args:?}");
        }
        for (args, why) in [
            (
                os(&["--title", "t", &s(&good)]),
                "--measurement and --title",
            ),
            (
                os(&["--measurement", "11", &s(&good)]),
                "--measurement and --title",
            ),
            (
                os(&["--measurement", "11", "--title", "t"]),
                "at least one record",
            ),
            (base(&["--bogus".into(), s(&good)]), "unknown argument"),
            (
                os(&["--measurement", "x", "--title", "t", &s(&good)]),
                "not a row number",
            ),
            (
                os(&["--measurement", "0", "--title", "t", &s(&good)]),
                "not a row number",
            ),
            (base(&["--out".into()]), "needs a value"),
        ] {
            let (code, _, err) = run(&args);
            assert_eq!(code, 2, "{args:?}: {err}");
            assert!(
                err.contains(why) && err.contains("usage: aggregate"),
                "{args:?}: {err}"
            );
        }
        let (code, out, _) = run(&os(&["--help"]));
        assert_eq!((code, out.as_str()), (0, AGGREGATE_USAGE));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
