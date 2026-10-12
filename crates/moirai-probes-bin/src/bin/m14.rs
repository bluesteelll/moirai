//! `m14`: measurement 14's driver ([60 §5.2] item 14, `docs/research/design/60-roadmap.md:770`): the layout probes
//! over the F09 segment at 1e5 and 1e6 nodes (CSR, frozen-bitset AND/popcount, column scans, overlay probe), T1's
//! point read after a full 4,096-op tail and delta checkpoint at 0.5 M nodes, and T2's MCP overlay catch-up model. It
//! records one run per quantity under the driver contract of [MP §9.7] (`docs/spec/measurement-protocol.md`), each in
//! a child process of its own; `--smoke` runs every arm once at the test profile and records nothing.
//!
//! Wiring only: the logic and its tests are `moirai_probes::m14`. This file passes it the Windows store seam
//! (`moirai_os::OsVfs`, [OS/fs], [OS/map]), the `Meter` (`moirai_os::OsMeter`, [OS/mem]), the monotonic timer, the
//! host-snapshot command runner, and a spawn of this executable for the per-run children; it installs
//! `moirai_os::CountingAlloc` so a record's heap high-water mark is read ([OS/mem §6]). Sources: `docs/m0/PLAN.md`
//! §3.2 WP-53e, [MP §4.6], [MP §9.7].

use std::ffi::OsString;
use std::process::ExitCode;

#[global_allocator]
static ALLOC: moirai_os::CountingAlloc = moirai_os::CountingAlloc;

/// Runs this executable again with `args` and returns its exit code (1 when it has none).
fn spawn(args: &[OsString]) -> Result<u8, String> {
    let exe = std::env::current_exe().map_err(|e| format!("the driver's own path: {e}"))?;
    let status = std::process::Command::new(exe)
        .args(args)
        .status()
        .map_err(|e| format!("starting a run: {e}"))?;
    Ok(status.code().map_or(1, |c| u8::try_from(c).unwrap_or(1)))
}

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let mut spawner = spawn;
    let code = moirai_probes::m14::cli(
        &args,
        moirai_probes::m14::System {
            vfs: &moirai_os::OsVfs::new(),
            meter: &moirai_os::OsMeter::new(),
            ticker: moirai_probes::arm::MonoTicker::new(),
            runner: moirai_probes::host::SystemRunner::default(),
            spawn: &mut spawner,
            profile: moirai_probes::m14::Profile::Production,
        },
        &mut std::io::stdout().lock(),
        &mut std::io::stderr().lock(),
    );
    ExitCode::from(code)
}
