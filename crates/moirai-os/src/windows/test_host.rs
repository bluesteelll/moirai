//! `os::test_host` on Windows (feature `test-host`): ending, pausing and resuming a process, and the process's wall-clock
//! offset ([OS/proc §13], [OS/clock §9]).
//!
//! Test and probe builds only: the composition-root lint refuses the feature in the product root `moirai`
//! ([OS/README §2.4]). [80] X9's public-interface rule binds product code; `NtSuspendProcess` and `NtResumeProcess` are
//! the test-only interface GT4's pause variant needs ([OS/mapping-appendix] open point 3), declared here because
//! `windows-sys` does not bind them.
//!
//! `small_volume` (a VHDX for disk-full tests, owner-run with elevation) is not part of WP-33's scope.

#![allow(unsafe_code)]

use std::io;
use std::os::windows::io::AsRawHandle;
use std::sync::Once;
use std::sync::atomic::{AtomicI64, Ordering};

use windows_sys::Win32::Foundation::{HANDLE, NTSTATUS};
use windows_sys::Win32::System::Threading::{PROCESS_SUSPEND_RESUME, TerminateProcess};

use super::proc::open_process;
use super::sys::{nt_code, nt_success, raw};

#[link(name = "ntdll", kind = "raw-dylib")]
unsafe extern "system" {
    fn NtSuspendProcess(process: HANDLE) -> NTSTATUS;
    fn NtResumeProcess(process: HANDLE) -> NTSTATUS;
}

/// The environment variable a harness sets on a child to shift its wall clock: a decimal i64 with an optional leading
/// `-` ([OS/proc §13]).
pub const WALL_OFFSET_ENV: &str = "MOIRAI_TEST_WALL_OFFSET_MS";

static OFFSET: AtomicI64 = AtomicI64::new(0);
static INIT: Once = Once::new();

/// Parses the offset's text: an optional `-` followed by one or more ASCII digits; anything else is 0.
fn parse_offset(s: &str) -> i64 {
    let digits = s.strip_prefix('-').unwrap_or(s);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return 0;
    }
    s.parse().unwrap_or(0)
}

fn init() {
    INIT.call_once(|| {
        let v = std::env::var(WALL_OFFSET_ENV).map_or(0, |s| parse_offset(&s));
        OFFSET.store(v, Ordering::Relaxed);
    });
}

/// The process's wall-clock offset in ms: read once from [`WALL_OFFSET_ENV`] at the first use, or as last set.
pub(crate) fn wall_offset_ms() -> i64 {
    init();
    OFFSET.load(Ordering::Relaxed)
}

/// Shifts this process's wall clock by `offset_ms` from now on ([OS/clock §9]); the monotonic and boot clocks are never
/// shifted.
pub fn set_wall_offset_ms(offset_ms: i64) {
    init();
    OFFSET.store(offset_ms, Ordering::Relaxed);
}

/// Ends a child at once: `TerminateProcess(child, 1)` (GT4's kill variant).
pub fn kill(child: &mut std::process::Child) -> io::Result<()> {
    // SAFETY: the handle is the child's process handle, owned by `child`, valid for the call.
    if unsafe { TerminateProcess(child.as_raw_handle(), 1) } != 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

fn nt_call(pid: u32, f: unsafe extern "system" fn(HANDLE) -> NTSTATUS) -> io::Result<()> {
    let h = open_process(pid, PROCESS_SUSPEND_RESUME)
        .map_err(|e| io::Error::from_raw_os_error(e as i32))?;
    // SAFETY: `h` is a process handle with `PROCESS_SUSPEND_RESUME`, valid for the call.
    let status = unsafe { f(raw(&h)) };
    if nt_success(status) {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(nt_code(status) as i32))
    }
}

/// Suspends every thread of the process `pid` (`NtSuspendProcess`; GT4's 1–120 s pauses).
pub fn suspend(pid: u32) -> io::Result<()> {
    nt_call(pid, NtSuspendProcess)
}

/// Resumes a process suspended by [`suspend`] (`NtResumeProcess`).
pub fn resume(pid: u32) -> io::Result<()> {
    nt_call(pid, NtResumeProcess)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offsets_parse_strictly() {
        assert_eq!(parse_offset("3600000"), 3_600_000);
        assert_eq!(parse_offset("-3600000"), -3_600_000);
        for bad in [
            "",
            "-",
            "+5",
            " 5",
            "5 ",
            "1e3",
            "0x10",
            "99999999999999999999",
        ] {
            assert_eq!(parse_offset(bad), 0, "{bad:?}");
        }
    }
}
