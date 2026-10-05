//! `loadgen`: measurement 16's load generator ([MP §9.4], `docs/spec/measurement-protocol.md` §9). It replays a load
//! profile — the fixture `cargo xtask loadrec` recorded, or a synthetic profile — on this machine: the system-wide
//! processor time and disk read and write bytes of the profile, with available physical memory held at 1.8 GB, read
//! back by `typeperf` once a second; it logs every second, writes the ready file once settled, stops when the stop
//! file appears, and prints the verdict of its replay against the profile ([MP §9.5]).
//!
//! Wiring only: the logic and its tests are `moirai_probes::load`; this file passes it the Windows `Meter`
//! (`moirai_os::OsMeter`: the generator's own processor time, [OS/mem §7], and available physical memory,
//! [OS/mem §5]). Sources: `docs/m0/PLAN.md` §3.2 WP-51, [60 §5.1] (the Load row), [60 §5.2] item 16.

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    let code = moirai_probes::load::cli::cli(
        &args,
        &mut moirai_probes::load::cli::SystemRig::new(moirai_os::OsMeter::new()),
        &mut std::io::stdout().lock(),
        &mut std::io::stderr().lock(),
    );
    ExitCode::from(code)
}
