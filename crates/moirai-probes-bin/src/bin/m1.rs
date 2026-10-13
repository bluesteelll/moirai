//! `m1`: measurement 1's driver ([60 §5.2] item 1, `docs/research/design/60-roadmap.md:757`): the CLI commit cycle on
//! a 64 MiB log (open, append, data-only flush, close) with its data-only-flush floor (`:788`), and the Defender close
//! cost, on the toy log's production profile. It records one run per quantity under the driver contract of [MP §9.7]
//! (`docs/spec/measurement-protocol.md`), each in a child process of its own; `--smoke` runs every arm once and records
//! nothing. Every file but the records lies below the working directory.
//!
//! Wiring only: the logic and its tests are `moirai_probes::m1`. This file passes it the toy log over the Windows store
//! seam (`moirai_probes::m1::store::ToyRig` over `moirai_os::OsVfs`, [OS/fs]), the `Meter` (`moirai_os::OsMeter`,
//! [OS/mem]), the monotonic timer, the host-snapshot command runner, the working directory, and a spawn of this
//! executable for the per-run children; it installs `moirai_os::CountingAlloc` so a record's heap high-water mark is
//! read ([OS/mem §6]). Sources: `docs/m0/PLAN.md` §3.2 WP-52, [MP §4.6], [MP §9.7].

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
    let cwd = match std::env::current_dir() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("m1: the working directory: {e}");
            return ExitCode::from(1);
        }
    };
    let mut spawner = spawn;
    let code = moirai_probes::m1::cli(
        &args,
        moirai_probes::m1::System {
            rig: &moirai_probes::m1::store::ToyRig::new(moirai_os::OsVfs::new()),
            meter: &moirai_os::OsMeter::new(),
            ticker: moirai_probes::arm::MonoTicker::new(),
            runner: moirai_probes::host::SystemRunner::default(),
            spawn: &mut spawner,
            cwd,
        },
        &mut std::io::stdout().lock(),
        &mut std::io::stderr().lock(),
    );
    ExitCode::from(code)
}
