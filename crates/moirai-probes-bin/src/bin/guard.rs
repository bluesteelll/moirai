//! `guard`: the pre-run guard ([MP §8], `docs/spec/measurement-protocol.md` §8). It refuses (exit 1) below 1.5 GB of
//! available physical memory or below 25 GB of disk headroom on the work volume, counted after the growth left to the
//! caps of the directories given with `--dir` (WP-05 passes the two lane target directories and the fuzz and mutants
//! directories), and fails closed on a reading it cannot take. It prints one JSON line (`moirai-probes/guard/1`) to
//! stdout and one line per refusal to stderr; exit 2 is a usage error.
//!
//! Wiring only: the logic and its tests are `moirai_probes::guard`; this file passes it the Windows `Meter`
//! (`moirai_os::OsMeter`, [OS/mem §5], [OS/fs §4.11]) and the `std::fs` directory sizer. Sources: `docs/m0/PLAN.md`
//! §3.2 WP-05 and WP-50, [60 §3.15].

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    let code = moirai_probes::guard::cli(
        &args,
        &moirai_os::OsMeter::new(),
        &moirai_probes::guard::FsSizer,
        &mut std::io::stdout().lock(),
        &mut std::io::stderr().lock(),
    );
    ExitCode::from(code)
}
