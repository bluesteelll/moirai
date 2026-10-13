//! `m10`: measurement 10's driver ([60 §5.2] item 10, `docs/research/design/60-roadmap.md:766`): full-tail replay at
//! the candidate checkpoint thresholds, decoding every record into the F09/F11-shaped overlay, and the open floor of
//! `HEAD` and 8 sealed files ([60 §5.3], `:790`). It records one run per quantity under the driver contract of
//! [MP §9.7] (`docs/spec/measurement-protocol.md`), each in a child process of its own, one point of each family
//! first; `--smoke` runs each requested arm once at the test profile and records nothing; `--list` prints each grid
//! tail's measures. Every fixture lies below the working directory.
//!
//! Wiring only: the logic and its tests are `moirai_probes::m10`. This file passes it the Windows store seam
//! (`moirai_os::OsVfs`, [OS/fs], [OS/map]), the `Meter` (`moirai_os::OsMeter`, [OS/mem]), the monotonic timer, the
//! host-snapshot command runner, and a spawn of this executable for the per-run children; it installs
//! `moirai_os::CountingAlloc` so a record's heap high-water mark and `--list`'s `ovl_counted` are read ([OS/mem §6]).
//! Sources: `docs/m0/PLAN.md` §3.2 WP-53d, [MP §4.6], [MP §9.7].

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
    let code = moirai_probes::m10::cli(
        &args,
        moirai_probes::m10::System {
            vfs: &moirai_os::OsVfs::new(),
            meter: &moirai_os::OsMeter::new(),
            ticker: moirai_probes::arm::MonoTicker::new(),
            runner: moirai_probes::host::SystemRunner::default(),
            spawn: &mut spawner,
            profile: moirai_probes::m10::Profile::Production,
        },
        &mut std::io::stdout().lock(),
        &mut std::io::stderr().lock(),
    );
    ExitCode::from(code)
}
