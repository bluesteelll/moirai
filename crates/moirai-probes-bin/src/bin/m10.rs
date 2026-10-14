//! `m10`: measurement 10's driver ([60 §5.2] item 10, `docs/research/design/60-roadmap.md:766`): full-tail replay at
//! the candidate checkpoint thresholds, decoding every record into the F09/F11-shaped overlay, and the open floor of
//! `HEAD` and 8 sealed files ([60 §5.3], `:790`); design-A A3's small tails in item 1a's A1 form with the offset build
//! timed, 16 concurrent opens, an open after another process modified the log, one command's file opens, and B-1 (u20). It
//! records one run per quantity under the driver contract of [MP §9.7] (`docs/spec/measurement-protocol.md`), each in
//! a child process of its own, one point of each family first; `--smoke` runs each requested arm once at the test
//! profile and records nothing; `--list` prints each grid tail's measures; `--opener` serves the pool's line protocol.
//! Every fixture lies below the working directory.
//!
//! Wiring only: the logic and its tests are `moirai_probes::m10`. This file passes it the Windows store seam
//! (`moirai_os::OsVfs`, [OS/fs], [OS/map]), the `Meter` (`moirai_os::OsMeter`, [OS/mem]), the monotonic timer, the
//! host-snapshot command runner, a spawn of this executable for the per-run children, the pool of 17 children of this
//! executable with `--opener` (`moirai_probes::m10::ProcessPool`), B-1's PowerShell modifier (Windows PowerShell below
//! `%SystemRoot%` with its encoded script, a pool of one), the system-wide monotonic clock (`OsVfs`'s
//! `Clock::mono_ns`, [OS/clock]) and stdin as a pool child's input; it installs `moirai_os::CountingAlloc` so a
//! record's heap high-water mark and `--list`'s `ovl_counted` are read ([OS/mem §6]). Sources: `docs/m0/PLAN.md` §3.2
//! WP-53d, WP-53e, [MP §4.6], [MP §9.7].

use moirai_probes::m10::{Modifier, POOL, Pool, ProcessPool};
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

/// Starts the pool: 17 children of this executable with `--opener` in `cwd`.
fn pool(cwd: &Path) -> Result<Box<dyn Pool>, String> {
    let exe = std::env::current_exe().map_err(|e| format!("the driver's own path: {e}"))?;
    Ok(Box::new(ProcessPool::spawn(
        &exe,
        &["--opener"],
        POOL,
        cwd,
    )?))
}

/// Starts B-1's PowerShell modifier in `cwd`: Windows PowerShell below `%SystemRoot%` with the encoded script of `m`.
fn ps(cwd: &Path, m: Modifier) -> Result<Box<dyn Pool>, String> {
    let exe = moirai_probes::m10::ps_program(std::env::var_os("SystemRoot").as_deref())?;
    let args = moirai_probes::m10::ps_args(m);
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    Ok(Box::new(ProcessPool::spawn(&exe, &args, 1, cwd)?))
}

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let vfs = moirai_os::OsVfs::new();
    let mono = move || moirai_probes::m10::mono_of(&vfs);
    let mut spawner = spawn;
    let mut pooler = pool;
    let mut pser = ps;
    let code = moirai_probes::m10::cli(
        &args,
        moirai_probes::m10::System {
            vfs: &vfs,
            meter: &moirai_os::OsMeter::new(),
            ticker: moirai_probes::arm::MonoTicker::new(),
            runner: moirai_probes::host::SystemRunner::default(),
            spawn: &mut spawner,
            profile: moirai_probes::m10::Profile::Production,
            pool: &mut pooler,
            mono: &mono,
            input: &mut std::io::stdin().lock(),
            ps: &mut pser,
        },
        &mut std::io::stdout().lock(),
        &mut std::io::stderr().lock(),
    );
    ExitCode::from(code)
}
