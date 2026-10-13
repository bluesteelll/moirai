//! `aggregate`: a measurement's committed aggregate, `docs/measurements/m0/<n>.md` ([MP §7.4],
//! `docs/spec/measurement-protocol.md`). It reads the measurement's run records and idle observations from files and
//! directories, and its notes file `<n>.notes.md` (`--notes`), and renders the runs table with each run record's
//! process readings, the idle table, the arms table, the runs that decide nothing, the per-quantity notes and the
//! decision for WP-81a, to stdout or to `--out` (UTF-8, LF: PowerShell 5.1's `>` would write UTF-16).
//!
//! Wiring only: the logic and its tests are `moirai_probes::aggregate`. It reads no counter, so it needs no `Meter`.
//! Sources: `docs/m0/PLAN.md` §3.2 WP-50, [MP §7.2]–§7.4.

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    let code = moirai_probes::aggregate::cli(
        &args,
        &mut std::io::stdout().lock(),
        &mut std::io::stderr().lock(),
    );
    ExitCode::from(code)
}
