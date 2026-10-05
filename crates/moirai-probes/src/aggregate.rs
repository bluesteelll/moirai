//! The committed aggregate of a measurement, `docs/measurements/m0/<n>.md` ([MP §7.4]).
//!
//! An aggregate holds summaries only: no samples, no paths, and no user name, host name, volume serial, machine GUID,
//! `BootId` or process command line (the pre-commit scrub refuses them, `docs/m0/PLAN.md` §3.2 WP-03). The run
//! records it is rendered from carry none of those either ([MP §7.1]).

use crate::host::{HostRecord, HostSnapshot};
use crate::idle::{IdleRecord, IdleWindow};
use crate::record::{Header, RunRecord};
use crate::units::{format_ns, format_value};
use std::fmt::Write as _;

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
}
