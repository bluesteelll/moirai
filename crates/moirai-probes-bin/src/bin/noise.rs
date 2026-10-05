//! `noise`: measurement 16's noise bands ([MP §6], [MP §9.7], `docs/spec/measurement-protocol.md`). It reads run
//! records and prints one noise-band record per gated statistic of every arm, combining the runs of one quantity, arm,
//! statistic, condition and host into the widest band. `noise.yml` runs it on the hosted runners' records; the laptop's
//! bands come from exit-grade laptop runs.
//!
//! Wiring only: the logic and its tests are `moirai_probes::noise`. It reads no counter, so it needs no `Meter`.
//! Sources: `docs/m0/PLAN.md` §3.2 WP-51, [60 §5.1] (the Noise band row), [60 §5.2] item 16.

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    let code = moirai_probes::noise::cli(
        &args,
        &mut std::io::stdout().lock(),
        &mut std::io::stderr().lock(),
    );
    ExitCode::from(code)
}
