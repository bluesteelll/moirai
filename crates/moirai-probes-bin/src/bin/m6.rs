//! `m6`: measurement 6's driver ([60 §5.2] item 6, `docs/research/design/60-roadmap.md:762`): size, encode and decode
//! speed and RSS of `lz4_flex` with and without the raw-content dictionary, of `ruzstd` at `Fastest`, and of the `zstd`
//! CLI as option (3)'s proxy, on the owner's notes, the plan sections of `docs/**` and `hist` frames
//! (`docs/research/design/90-harness-agnostic-design.md:959-974`). It records one run per quantity under the driver
//! contract of [MP §9.7] (`docs/spec/measurement-protocol.md`), each in a child process of its own; `--smoke` runs every
//! arm once and records nothing. Every file but the records lies below the working directory.
//!
//! Wiring only: the logic and its tests are `moirai_probes::m6`. This file passes it the `Meter`
//! (`moirai_os::OsMeter`, [OS/mem]), the monotonic timer, the host-snapshot command runner, a spawn of this executable
//! for the per-run children, the production profile, and three facts of the process:
//! - the `zstd` CLI found on `PATH` (`moirai_probes::m6::codec::zstd_on_path`), which the runner's `PATH` provides;
//! - no working directory, so the library takes the process's own;
//! - the checkout this binary was compiled in, `CARGO_MANIFEST_DIR`'s grandparent, where `--smoke` reads the real
//!   corpora when they are present (`private/codec-bodies/`, `docs/`) and prints which it read.
//!
//! It installs `moirai_os::CountingAlloc` so a record's heap high-water mark is read ([OS/mem §6]). Sources:
//! `docs/m0/PLAN.md` §3.2 WP-54, [MP §4.6], [MP §9.7].

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

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let mut spawner = spawn;
    let checkout = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .map(Path::to_path_buf);
    let code = moirai_probes::m6::cli(
        &args,
        moirai_probes::m6::System {
            meter: &moirai_os::OsMeter::new(),
            ticker: moirai_probes::arm::MonoTicker::new(),
            runner: moirai_probes::host::SystemRunner::default(),
            spawn: &mut spawner,
            profile: moirai_probes::m6::Profile::Production,
            zstd: moirai_probes::m6::codec::zstd_on_path(std::env::var_os("PATH").as_deref()),
            work: None,
            checkout,
        },
        &mut std::io::stdout().lock(),
        &mut std::io::stderr().lock(),
    );
    ExitCode::from(code)
}
