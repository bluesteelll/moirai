//! `m2`: measurement 2's driver ([60 §5.2] item 2, `docs/research/design/60-roadmap.md:758`): 16 child writer
//! processes (this executable with `--writer`) in bursts through the toy log's writer and flush bytes on the overlapped
//! lock, each commit with its in-lock cost drawn from the hold budget; the last acknowledgement, the flushes per burst,
//! fairness, the flush-byte hand-off and T2's writer wait. It records one run per quantity under the driver contract of
//! [MP §9.7] (`docs/spec/measurement-protocol.md`), each in a child process of its own; `--smoke` runs every arm once
//! over real child writers and records nothing. Every file but the records lies below the working directory.
//!
//! Wiring only: the logic and its tests are `moirai_probes::m2`. This file passes it the toy log over the Windows store
//! seam for the coordinator (`moirai_probes::m1::store::ToyRig` over `moirai_os::OsVfs`, [OS/fs]) and for a child
//! writer (`moirai_probes::m2::child::child_rig`: the same seam with its lock bytes logged), stdin as the child's input,
//! the `Meter` (`moirai_os::OsMeter`, [OS/mem]), the monotonic timer, the host-snapshot command runner, the working
//! directory, the pool of child writers and a spawn of this executable for the per-run children; it installs
//! `moirai_os::CountingAlloc` so a record's heap high-water mark is read ([OS/mem §6]). Sources: `docs/m0/PLAN.md`
//! §3.2 WP-52, [MP §4.6], [MP §9.7].

use moirai_probes::m2::burst::{CHILDREN, Writers};
use std::ffi::OsString;
use std::path::Path;
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

/// Starts the 16 child writers, this executable with `--writer`, in `cwd`.
fn pool(cwd: &Path) -> Result<Box<dyn Writers>, String> {
    let exe = std::env::current_exe().map_err(|e| format!("the driver's own path: {e}"))?;
    let writers = moirai_probes::m2::child::ProcessWriters::spawn(&exe, CHILDREN, cwd)?;
    Ok(Box::new(writers))
}

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let cwd = match std::env::current_dir() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("m2: the working directory: {e}");
            return ExitCode::from(1);
        }
    };
    let vfs = moirai_os::OsVfs::new();
    let mut spawner = spawn;
    let mut pooler = pool;
    let code = moirai_probes::m2::cli(
        &args,
        moirai_probes::m2::System {
            rig: &moirai_probes::m1::store::ToyRig::new(vfs),
            child: &moirai_probes::m2::child::child_rig(vfs),
            input: &mut std::io::stdin().lock(),
            meter: &moirai_os::OsMeter::new(),
            ticker: moirai_probes::arm::MonoTicker::new(),
            runner: moirai_probes::host::SystemRunner::default(),
            pool: &mut pooler,
            spawn: &mut spawner,
            cwd,
        },
        &mut std::io::stdout().lock(),
        &mut std::io::stderr().lock(),
    );
    ExitCode::from(code)
}
