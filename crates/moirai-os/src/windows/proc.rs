//! `os::proc` on Windows: the three clocks ([OS/clock §2.1]), the process seam `ProcHost` ([OS/proc §2–§10]), the
//! random source `Entropy` ([OS/README §4.6]) and the child-peak reading behind `Meter::peak_of_child` ([OS/proc §9]).
//!
//! - **Clocks.** wall = `GetSystemTimePreciseAsFileTime` (plus the test-host offset in `test-host` builds, [OS/clock
//!   §9]); mono = `QueryPerformanceCounter` scaled by `QueryPerformanceFrequency` (the system-wide counter, no per-process
//!   origin, so it is comparable across the processes of one boot, [OS/clock] open point 7); boot =
//!   `QueryInterruptTimePrecise` × 100 (HOLE(OS-win-boot-clock), design choice (a)).
//! - **Boot identity** ([OS/proc §4.2], HOLE(OS-win-boot-source) design choice (a)): `BLAKE3-128(lp("moirai-boot-id-v1")
//!   ‖ lp("win-bootid-machineguid") ‖ lp(u32le(B)) ‖ lp(G))` with `B` the `BootId` member of `KUSER_SHARED_DATA` (the
//!   registry's `PrefetchParameters\BootId` when it reads 0) and `G` the `MachineGuid`; read once and cached; any failure
//!   is Unknown-boot mode, never a refusal.
//! - **Liveness** ([OS/proc §6.1]) is diagnostics only.
//! - **The parent watch** ([OS/proc §7]) is event-driven: `WaitForMultipleObjects({parent, wake event}, INFINITE)`.

#![allow(unsafe_code)]

use std::os::windows::io::{AsRawHandle, OwnedHandle};
use std::path::Path;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

use moirai_vfs::{
    BootId, BootIdentity, ChildPeak, Clock, Entropy, Liveness, MeterError, OsCode, OsTag,
    ParentRec, PeakKind, ProcHost, ProcId, UnknownBoot, VfsError, Wake, WatchEvent,
};
use windows_sys::Wdk::System::SystemServices::KUSER_SHARED_DATA;
use windows_sys::Win32::Foundation::{
    ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER, ERROR_MORE_DATA, FILETIME, HANDLE, WAIT_OBJECT_0,
};
use windows_sys::Win32::Security::Cryptography::{
    BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom,
};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
use windows_sys::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX};
use windows_sys::Win32::System::Registry::{
    HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RRF_SUBKEY_WOW6464KEY, RegGetValueW,
};
use windows_sys::Win32::System::SystemInformation::GetSystemTimePreciseAsFileTime;
use windows_sys::Win32::System::Threading::{
    CreateEventW, GetCurrentProcess, GetCurrentProcessId, GetProcessTimes, INFINITE, OpenProcess,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, SetEvent, WaitForMultipleObjects,
    WaitForSingleObject,
};
use windows_sys::Win32::System::WindowsProgramming::QueryInterruptTimePrecise;

use super::fs::OsVfs;
use super::sys::{self, last_error, owned, raw};

// ---------------------------------------------------------------------------------------------------------------------
// Clocks ([OS/clock §2.1])

/// FILETIME of 1970-01-01T00:00:00Z.
const EPOCH_FILETIME: i128 = 116_444_736_000_000_000;

fn qpc_frequency() -> u64 {
    static FREQ: AtomicU64 = AtomicU64::new(0);
    let f = FREQ.load(Ordering::Relaxed);
    if f != 0 {
        return f;
    }
    let mut v = 0i64;
    // SAFETY: `v` is a live local the call writes; the call cannot fail on Windows XP and later.
    unsafe { QueryPerformanceFrequency(&mut v) };
    let v = v.max(1) as u64;
    FREQ.store(v, Ordering::Relaxed);
    v
}

/// The monotonic clock in ns ([OS/clock §2.1]): `QueryPerformanceCounter` × 10^9 / frequency, u128 intermediate,
/// saturating.
pub(crate) fn mono_ns() -> u64 {
    let mut c = 0i64;
    // SAFETY: `c` is a live local the call writes; the call cannot fail on Windows XP and later.
    unsafe { QueryPerformanceCounter(&mut c) };
    let ns = u128::from(c.max(0) as u64) * 1_000_000_000 / u128::from(qpc_frequency());
    u64::try_from(ns).unwrap_or(u64::MAX)
}

/// The boot clock in ns: `QueryInterruptTimePrecise` (100 ns units since boot, sleep and hibernation included) × 100.
pub(crate) fn boot_ns() -> u64 {
    let mut t = 0u64;
    // SAFETY: `t` is a live local the call writes.
    unsafe { QueryInterruptTimePrecise(&mut t) };
    t.saturating_mul(100)
}

/// The OS wall clock in ms since the Unix epoch: `(f − 116 444 736 000 000 000).div_euclid(10 000)`.
fn os_wall_ms() -> i64 {
    let mut ft = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    // SAFETY: `ft` is a live local the call writes.
    unsafe { GetSystemTimePreciseAsFileTime(&mut ft) };
    let f = i128::from((u64::from(ft.dwHighDateTime) << 32) | u64::from(ft.dwLowDateTime));
    let ms = (f - EPOCH_FILETIME).div_euclid(10_000);
    i64::try_from(ms).unwrap_or(if ms < 0 { i64::MIN } else { i64::MAX })
}

impl Clock for OsVfs {
    fn wall_ms(&self) -> i64 {
        let w = os_wall_ms();
        #[cfg(feature = "test-host")]
        let w = w.saturating_add(super::test_host::wall_offset_ms());
        w
    }

    fn mono_ns(&self) -> u64 {
        mono_ns()
    }

    fn boot_ns(&self) -> u64 {
        boot_ns()
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Entropy ([OS/README §4.6])

/// The failure rule of [OS/README §4.6]: a failed `BCryptGenRandom` panics with a message naming the call and the OS code
/// (the product's panic hook then prints the `internal` text and exits 1, [F19 §7.2] item 8). No retry with another
/// call and no weaker source; the panic is a process death, which every protocol point tolerates ([F15 §6.4]).
fn rng_status(status: windows_sys::Win32::Foundation::NTSTATUS) {
    assert!(
        sys::nt_success(status),
        "BCryptGenRandom failed: {}",
        OsCode(sys::nt_code(status) as i32).unit(OsTag::Windows)
    );
}

impl Entropy for OsVfs {
    /// `BCryptGenRandom(NULL, buf, n, BCRYPT_USE_SYSTEM_PREFERRED_RNG)`, in chunks of at most `u32::MAX` bytes
    /// ([OS/README §4.6] "Per-OS calls").
    fn fill_random(&self, buf: &mut [u8]) {
        for chunk in buf.chunks_mut(u32::MAX as usize) {
            // SAFETY: `chunk` is writable for `chunk.len()` bytes (≤ u32::MAX); the null algorithm handle is allowed with
            // `BCRYPT_USE_SYSTEM_PREFERRED_RNG`.
            let status = unsafe {
                BCryptGenRandom(
                    core::ptr::null_mut(),
                    chunk.as_mut_ptr(),
                    chunk.len() as u32,
                    BCRYPT_USE_SYSTEM_PREFERRED_RNG,
                )
            };
            rng_status(status);
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Process times and the snapshot walk

/// A FILETIME as ns since the Unix epoch; `None` before 1970 or outside u64 ([OS/proc §3.2]).
fn filetime_ns(ft: &FILETIME) -> Option<u64> {
    let f = i128::from((u64::from(ft.dwHighDateTime) << 32) | u64::from(ft.dwLowDateTime));
    u64::try_from((f - EPOCH_FILETIME) * 100).ok()
}

/// A FILETIME duration (user or kernel time) in ns.
pub(crate) fn filetime_span_ns(ft: &FILETIME) -> u64 {
    ((u64::from(ft.dwHighDateTime) << 32) | u64::from(ft.dwLowDateTime)).saturating_mul(100)
}

/// `GetProcessTimes(h)`: (creation, exit, kernel, user).
pub(crate) fn process_times(h: HANDLE) -> Result<[FILETIME; 4], u32> {
    let z = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let mut t = [z; 4];
    let [c, e, k, u] = &mut t;
    // SAFETY: the four out-pointers are distinct live locals; `h` is a process handle with query access.
    let ok = unsafe { GetProcessTimes(h, c, e, k, u) };
    if ok != 0 { Ok(t) } else { Err(last_error()) }
}

/// Opens a process by pid with `access` (non-inheritable).
pub(crate) fn open_process(pid: u32, access: u32) -> Result<OwnedHandle, u32> {
    // SAFETY: `OpenProcess` has no pointer arguments; the result is checked.
    let h = unsafe { OpenProcess(access, 0, pid) };
    owned(h).ok_or_else(last_error)
}

/// Walks a process snapshot; `each` returns `false` to stop.
pub(crate) fn walk_processes(mut each: impl FnMut(&PROCESSENTRY32W) -> bool) -> Result<(), u32> {
    // SAFETY: `CreateToolhelp32Snapshot` has no pointer arguments; the handle is checked.
    let snap =
        owned(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }).ok_or_else(last_error)?;
    let mut e = PROCESSENTRY32W {
        dwSize: core::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    // SAFETY: `e` is a live entry with `dwSize` set, as the call requires.
    let mut ok = unsafe { Process32FirstW(raw(&snap), &mut e) };
    while ok != 0 {
        if !each(&e) {
            return Ok(());
        }
        // SAFETY: as above.
        ok = unsafe { Process32NextW(raw(&snap), &mut e) };
    }
    Ok(())
}

/// The parent's pid and executable name from one snapshot ([OS/proc §7, §8]).
fn parent_entry() -> Result<(u32, Option<String>), u32> {
    // SAFETY: `GetCurrentProcessId` has no preconditions.
    let me = unsafe { GetCurrentProcessId() };
    let mut ppid = None;
    let mut names: Vec<(u32, String)> = Vec::new();
    walk_processes(|e| {
        if e.th32ProcessID == me {
            ppid = Some(e.th32ParentProcessID);
        }
        let len = e
            .szExeFile
            .iter()
            .position(|&u| u == 0)
            .unwrap_or(e.szExeFile.len());
        names.push((
            e.th32ProcessID,
            String::from_utf16_lossy(&e.szExeFile[..len]),
        ));
        true
    })?;
    let ppid = ppid.ok_or(ERROR_INVALID_PARAMETER)?;
    let name = names.into_iter().find(|(p, _)| *p == ppid).map(|(_, n)| n);
    Ok((ppid, name))
}

// ---------------------------------------------------------------------------------------------------------------------
// The boot identity ([OS/proc §4])

/// The WDK offset of `KUSER_SHARED_DATA.BootId` (`ntddk.h`), taken from the binding and checked against the declared
/// value 0x2C4 ([OS/proc] open point 8).
const BOOT_ID_OFFSET: usize = core::mem::offset_of!(KUSER_SHARED_DATA, BootId);
const _: () = assert!(BOOT_ID_OFFSET == 0x2C4);
/// The fixed user-mode address of `KUSER_SHARED_DATA`.
const KUSER_SHARED_DATA_ADDRESS: usize = 0x7FFE_0000;

/// `u32le(len(x)) ‖ x` ([F01 §6.3]).
pub(crate) fn lp(h: &mut blake3::Hasher, x: &[u8]) {
    h.update(&(x.len() as u32).to_le_bytes());
    h.update(x);
}

/// Maps a registry failure to the reason of Unknown-boot mode.
fn registry_reason(rc: u32) -> UnknownBoot {
    match rc {
        ERROR_ACCESS_DENIED => UnknownBoot::Denied,
        // ERROR_UNSUPPORTED_TYPE: the value exists with another type.
        1630 => UnknownBoot::Malformed,
        _ => UnknownBoot::SourceAbsent,
    }
}

fn reg_dword(subkey: &str, value: &str) -> Result<u32, UnknownBoot> {
    let (k, v) = (sys::wide_z(subkey.as_ref()), sys::wide_z(value.as_ref()));
    let mut data = 0u32;
    let mut size = 4u32;
    // SAFETY: the key and value names are NUL-terminated and outlive the call; `data` is writable for `size` bytes.
    let rc = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            k.as_ptr(),
            v.as_ptr(),
            RRF_RT_REG_DWORD,
            core::ptr::null_mut(),
            (&mut data as *mut u32).cast(),
            &mut size,
        )
    };
    if rc == 0 {
        Ok(data)
    } else {
        Err(registry_reason(rc))
    }
}

fn reg_sz(subkey: &str, value: &str) -> Result<Vec<u16>, UnknownBoot> {
    let (k, v) = (sys::wide_z(subkey.as_ref()), sys::wide_z(value.as_ref()));
    let mut buf: Vec<u16> = vec![0; 64];
    loop {
        let mut size = (buf.len() * 2) as u32;
        // SAFETY: the names are NUL-terminated; `buf` is writable for `size` bytes.
        let rc = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                k.as_ptr(),
                v.as_ptr(),
                RRF_RT_REG_SZ | RRF_SUBKEY_WOW6464KEY,
                core::ptr::null_mut(),
                buf.as_mut_ptr().cast(),
                &mut size,
            )
        };
        match rc {
            0 => {
                buf.truncate(size as usize / 2);
                while buf.last() == Some(&0) {
                    buf.pop();
                }
                return Ok(buf);
            }
            ERROR_MORE_DATA if buf.len() < 4096 => buf.resize(buf.len() * 4, 0),
            rc => return Err(registry_reason(rc)),
        }
    }
}

/// Reads the Windows boot identity ([OS/proc §4.2]).
fn read_boot_identity() -> BootIdentity {
    let addr = KUSER_SHARED_DATA_ADDRESS + BOOT_ID_OFFSET;
    // SAFETY: `KUSER_SHARED_DATA` is mapped read-only at 0x7FFE0000 in every Windows user-mode process for its whole
    // life; `BootId` is a naturally aligned u32 at its declared offset inside it; the kernel updates it, hence the
    // volatile read.
    let mut b = unsafe { core::ptr::read_volatile(addr as *const u32) };
    if b == 0 {
        b = match reg_dword(
            "SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Memory Management\\PrefetchParameters",
            "BootId",
        ) {
            Ok(v) => v,
            Err(r) => return BootIdentity::Unknown(r),
        };
        if b == 0 {
            return BootIdentity::Unknown(UnknownBoot::SourceAbsent);
        }
    }
    let g = match reg_sz("SOFTWARE\\Microsoft\\Cryptography", "MachineGuid") {
        Ok(g) => g,
        Err(r) => return BootIdentity::Unknown(r),
    };
    let Ok(g) = String::from_utf16(&g) else {
        return BootIdentity::Unknown(UnknownBoot::Malformed);
    };
    if g.is_empty() {
        return BootIdentity::Unknown(UnknownBoot::Malformed);
    }
    let mut h = blake3::Hasher::new();
    lp(&mut h, b"moirai-boot-id-v1");
    lp(&mut h, b"win-bootid-machineguid");
    lp(&mut h, &b.to_le_bytes());
    lp(&mut h, g.as_bytes());
    let mut id = [0u8; 16];
    id.copy_from_slice(&h.finalize().as_bytes()[..16]);
    BootIdentity::Known(BootId(id))
}

/// The process's boot identity, read once and cached ([OS/proc §4.4]).
pub(crate) fn boot_identity() -> BootIdentity {
    static BOOT: OnceLock<BootIdentity> = OnceLock::new();
    *BOOT.get_or_init(read_boot_identity)
}

// ---------------------------------------------------------------------------------------------------------------------
// ProcHost ([OS/proc §10])

/// A watch of the parent process ([OS/proc §7]).
#[derive(Debug)]
pub struct OsParentWatch {
    /// `SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION` on the parent; `None` when the watch was created fired.
    parent: Option<OwnedHandle>,
}

/// The wake object of the parent watch: an auto-reset event ([OS/proc §7]).
#[derive(Debug)]
pub struct OsWake(OwnedHandle);

impl Wake for OsWake {
    fn signal(&self) {
        // SAFETY: the event handle is owned by `self` and valid for the call.
        unsafe { SetEvent(raw(&self.0)) };
    }
}

fn vfs_err(code: u32, call: &'static str) -> VfsError {
    sys::error(code, sys::Domain::Store, call)
}

impl ProcHost for OsVfs {
    type ParentWatch = OsParentWatch;
    type Wake = OsWake;

    fn os_tag(&self) -> OsTag {
        OsTag::Windows
    }

    fn self_id(&self) -> ProcId {
        static SELF: OnceLock<ProcId> = OnceLock::new();
        *SELF.get_or_init(|| {
            // SAFETY: `GetCurrentProcessId` has no preconditions.
            let pid = unsafe { GetCurrentProcessId() };
            // SAFETY: the pseudo-handle of the current process is always valid.
            let start = process_times(unsafe { GetCurrentProcess() })
                .ok()
                .and_then(|t| filetime_ns(&t[0]));
            let boot = boot_identity();
            let mut flags = 0u8;
            if start.is_some() {
                flags |= ProcId::START_KNOWN;
            }
            if matches!(boot, BootIdentity::Known(_)) {
                flags |= ProcId::BOOT_KNOWN;
            }
            ProcId {
                os: OsTag::Windows as u8,
                flags,
                pid,
                start: start.unwrap_or(0),
                boot_hash: boot.boot_hash(),
                pidns: 0,
            }
        })
    }

    fn parent(&self) -> Result<ParentRec, VfsError> {
        let (ppid, _) = parent_entry().map_err(|e| vfs_err(e, "CreateToolhelp32Snapshot"))?;
        let start = open_process(ppid, PROCESS_QUERY_LIMITED_INFORMATION)
            .ok()
            .and_then(|h| process_times(raw(&h)).ok())
            .and_then(|t| filetime_ns(&t[0]));
        Ok(ParentRec {
            pid: ppid,
            start: start.unwrap_or(0),
            start_known: start.is_some(),
        })
    }

    fn boot_identity(&self) -> BootIdentity {
        boot_identity()
    }

    fn alive(&self, p: &ProcId) -> Liveness {
        // Row 1: uninterpretable or another OS.
        if p.os != OsTag::Windows as u8 || p.flags & 0xF0 != 0 {
            return Liveness::Unknown;
        }
        // Row 2: both boots known and different.
        let mine = self.self_id();
        if p.flags & ProcId::BOOT_KNOWN != 0
            && mine.flags & ProcId::BOOT_KNOWN != 0
            && p.boot_hash != mine.boot_hash
        {
            return Liveness::Unknown;
        }
        // Rows 4–5.
        let h = match open_process(
            p.pid,
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
        ) {
            Ok(h) => h,
            Err(ERROR_INVALID_PARAMETER) => return Liveness::Dead,
            Err(_) => return Liveness::Unknown,
        };
        // Row 6: exited, its object remains.
        // SAFETY: `h` has `SYNCHRONIZE`; a zero timeout only polls.
        if unsafe { WaitForSingleObject(raw(&h), 0) } == WAIT_OBJECT_0 {
            return Liveness::Dead;
        }
        // Row 7: a reused pid.
        if p.flags & ProcId::START_KNOWN != 0
            && let Some(start) = process_times(raw(&h)).ok().and_then(|t| filetime_ns(&t[0]))
            && start != p.start
        {
            return Liveness::Dead;
        }
        Liveness::Alive
    }

    fn watch_parent(&self) -> Result<OsParentWatch, VfsError> {
        let (ppid, _) = parent_entry().map_err(|e| vfs_err(e, "CreateToolhelp32Snapshot"))?;
        let h = match open_process(
            ppid,
            PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
        ) {
            Ok(h) => h,
            // The parent is gone already.
            Err(ERROR_INVALID_PARAMETER) => return Ok(OsParentWatch { parent: None }),
            Err(e) => return Err(vfs_err(e, "OpenProcess")),
        };
        // A parent created after this process is a reused pid: the real parent exited.
        // SAFETY: the pseudo-handle of the current process is always valid.
        let mine = process_times(unsafe { GetCurrentProcess() })
            .map_err(|e| vfs_err(e, "GetProcessTimes"))?;
        let theirs = process_times(raw(&h)).map_err(|e| vfs_err(e, "GetProcessTimes"))?;
        let key = |t: &FILETIME| (u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime);
        if key(&theirs[0]) >= key(&mine[0]) {
            return Ok(OsParentWatch { parent: None });
        }
        Ok(OsParentWatch { parent: Some(h) })
    }

    fn new_wake(&self) -> Result<OsWake, VfsError> {
        // SAFETY: no security attributes, auto-reset, initially unsignalled, unnamed.
        let h = unsafe { CreateEventW(core::ptr::null(), 0, 0, core::ptr::null()) };
        owned(h)
            .map(OsWake)
            .ok_or_else(|| vfs_err(last_error(), "CreateEventW"))
    }

    fn wait_parent_or_wake(
        &self,
        w: &OsParentWatch,
        wake: &OsWake,
    ) -> Result<WatchEvent, VfsError> {
        let Some(parent) = &w.parent else {
            return Ok(WatchEvent::ParentExited);
        };
        let handles = [raw(parent), raw(&wake.0)];
        // SAFETY: both handles are valid and have `SYNCHRONIZE`; the array outlives the call.
        let r = unsafe { WaitForMultipleObjects(2, handles.as_ptr(), 0, INFINITE) };
        match r {
            WAIT_OBJECT_0 => Ok(WatchEvent::ParentExited),
            r if r == WAIT_OBJECT_0 + 1 => Ok(WatchEvent::Woken),
            _ => Err(vfs_err(last_error(), "WaitForMultipleObjects")),
        }
    }

    fn parent_image(&self) -> Option<Box<str>> {
        let (_, name) = parent_entry().ok()?;
        let name = name?.to_lowercase();
        let base = name.strip_suffix(".exe").unwrap_or(&name);
        Some(Box::from(base))
    }

    fn spawn_gc_child(&self, exe: &Path, args: &[&str], cwd: &Path) -> Result<u32, VfsError> {
        super::spawn::spawn_gc_child(exe, args, cwd)
    }

    fn enter_background(&self) {
        super::spawn::enter_background();
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// peak_of_child ([OS/proc §9])

/// `PROCESS_MEMORY_COUNTERS_EX` of a process handle ([OS/mem §3, §7]).
pub(crate) fn memory_counters(h: HANDLE) -> Result<PROCESS_MEMORY_COUNTERS_EX, MeterError> {
    let mut c = PROCESS_MEMORY_COUNTERS_EX {
        cb: core::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
        ..Default::default()
    };
    // SAFETY: `c` is a live `PROCESS_MEMORY_COUNTERS_EX` whose `cb` gives its size; the EX form is accepted where the
    // plain one is declared.
    let ok = unsafe {
        GetProcessMemoryInfo(h, (&mut c as *mut PROCESS_MEMORY_COUNTERS_EX).cast(), c.cb)
    };
    if ok != 0 {
        Ok(c)
    } else {
        Err(MeterError {
            os: OsCode(last_error() as i32),
            what: "GetProcessMemoryInfo",
        })
    }
}

/// The peak private bytes of an exited child ([OS/proc §9]): `WaitForSingleObject(child, INFINITE)`, then
/// `PeakPagefileUsage` of `GetProcessMemoryInfo` through the still-open process handle.
pub(crate) fn peak_of_child(child: &std::process::Child) -> Result<ChildPeak, MeterError> {
    let h = child.as_raw_handle();
    // SAFETY: `h` is the child's process handle, owned by `child`, which the caller keeps alive for the call.
    if unsafe { WaitForSingleObject(h, INFINITE) } != WAIT_OBJECT_0 {
        return Err(MeterError {
            os: OsCode(last_error() as i32),
            what: "WaitForSingleObject",
        });
    }
    let c = memory_counters(h)?;
    Ok(ChildPeak {
        private_peak_bytes: c.PeakPagefileUsage as u64,
        kind: PeakKind::Peak,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clocks_move_the_right_way() {
        let v = OsVfs;
        let (m1, b1) = (v.mono_ns(), v.boot_ns());
        std::thread::sleep(std::time::Duration::from_millis(20));
        let (m2, b2) = (v.mono_ns(), v.boot_ns());
        assert!(m2 >= m1 + 15_000_000, "mono advanced {m1} -> {m2}");
        assert!(b2 >= b1 + 15_000_000, "boot advanced {b1} -> {b2}");
        let w = v.wall_ms();
        // After 2020-01-01 and before 2200.
        assert!(w > 1_577_836_800_000 && w < 7_258_118_400_000, "{w}");
    }

    #[test]
    fn filetime_conversions() {
        let ft = |f: u64| FILETIME {
            dwLowDateTime: f as u32,
            dwHighDateTime: (f >> 32) as u32,
        };
        assert_eq!(filetime_ns(&ft(116_444_736_000_000_000)), Some(0));
        assert_eq!(filetime_ns(&ft(116_444_736_000_000_001)), Some(100));
        assert_eq!(filetime_ns(&ft(1)), None, "before 1970");
        assert_eq!(filetime_span_ns(&ft(3)), 300);
    }

    #[test]
    fn boot_identity_is_stable_and_hashes_odd() {
        let a = boot_identity();
        assert_eq!(a, read_boot_identity(), "one boot, one identity");
        if let BootIdentity::Known(b) = a {
            assert_eq!(b.hash() & 1, 1);
        }
        let me = OsVfs.self_id();
        assert_eq!(me.os, 1);
        assert_eq!(me.pid, std::process::id());
        assert_ne!(me.flags & ProcId::START_KNOWN, 0);
        assert_eq!(me.boot_hash, a.boot_hash());
        assert_eq!(OsVfs.alive(&me), Liveness::Alive);
    }

    #[test]
    fn liveness_rows() {
        let v = OsVfs;
        let me = v.self_id();
        assert_eq!(
            v.alive(&ProcId { os: 2, ..me }),
            Liveness::Unknown,
            "another OS"
        );
        assert_eq!(
            v.alive(&ProcId {
                flags: me.flags | 0x10,
                ..me
            }),
            Liveness::Unknown
        );
        assert_eq!(
            v.alive(&ProcId {
                start: me.start ^ 1,
                ..me
            }),
            Liveness::Dead,
            "a reused pid"
        );
        if me.flags & ProcId::BOOT_KNOWN != 0 {
            assert_eq!(
                v.alive(&ProcId {
                    boot_hash: me.boot_hash ^ 2,
                    ..me
                }),
                Liveness::Unknown
            );
        }
    }

    #[test]
    fn random_bytes_fill_the_buffer() {
        let mut a = [0u8; 64];
        let mut b = [0u8; 64];
        OsVfs.fill_random(&mut a);
        OsVfs.fill_random(&mut b);
        assert_ne!(a, b);
        assert_ne!(a, [0u8; 64]);
        OsVfs.fill_random(&mut []);
        // The draw widths of [OS/README §4.6]'s table: 8 and 16 bytes, one call each.
        let (mut n8, mut n16) = ([0u8; 8], [0u8; 16]);
        OsVfs.fill_random(&mut n8);
        OsVfs.fill_random(&mut n16);
        assert!(n8 != [0; 8] || n16 != [0; 16]);
        rng_status(0);
    }

    /// The failure rule ([OS/README §4.6]): a failed call panics, naming the call and the OS code.
    #[test]
    #[should_panic(expected = "BCryptGenRandom failed: os 87 ERROR_INVALID_PARAMETER")]
    fn a_failed_draw_panics_with_the_call_and_the_code() {
        rng_status(windows_sys::Win32::Foundation::STATUS_INVALID_PARAMETER);
    }

    #[test]
    fn parent_is_found() {
        let p = OsVfs.parent().unwrap();
        assert_ne!(p.pid, 0);
        assert!(OsVfs.parent_image().is_some());
    }
}
