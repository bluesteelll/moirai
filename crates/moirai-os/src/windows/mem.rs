//! `os::mem` on Windows: the `Meter` implementation [`OsMeter`] and the counting global allocator [`CountingAlloc`]
//! ([OS/mem]; [OS/README §4.3]).
//!
//! | Method | Windows source |
//! |---|---|
//! | `free_space` | `GetDiskFreeSpaceExW` ([OS/fs §4.11]) |
//! | `private_now` / `private_peak` | `GetProcessMemoryInfo(GetCurrentProcess(), PROCESS_MEMORY_COUNTERS_EX)` → `PrivateUsage` / `PeakPagefileUsage` |
//! | `available_physical` | `GlobalMemoryStatusEx` → `ullAvailPhys` |
//! | `peak_of_child` | `WaitForSingleObject(child, INFINITE)` → `PeakPagefileUsage` ([OS/proc §9]) |
//! | `child_private_now` | `GetProcessMemoryInfo(child)` → `PrivateUsage` |
//! | `child_threads` | `CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD)`, entries owned by the child's pid |
//! | `cpu_times` | `GetProcessTimes` user and kernel `FILETIME` × 100 ns |
//! | `prepare_child` / `bind_child` | nothing to arrange: `ChildTicket(0)` / nothing |
//! | `heap_counts` / `reset_heap_high_water` | [`CountingAlloc`]'s atomics, when installed |
//!
//! A reading that cannot be taken is a [`MeterError`], never a guess and never a store error.

#![allow(unsafe_code)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use moirai_vfs::{
    ChildPeak, ChildTicket, CpuTimes, FreeSpace, HeapCounts, Meter, MeterError, OsCode, VfsError,
    VfsErrorKind,
};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
};
use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows_sys::Win32::System::Threading::GetCurrentProcess;

use super::fs::free_space_of;
use super::proc::{filetime_span_ns, memory_counters, peak_of_child, process_times};
use super::sys::{last_error, owned, raw, verbatim};

/// The measurement seam for the running Windows process ([OS/README §4.3]); zero-sized.
#[derive(Copy, Clone, Debug, Default)]
pub struct OsMeter;

impl OsMeter {
    /// The seam value.
    pub const fn new() -> OsMeter {
        OsMeter
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// CountingAlloc ([OS/mem §6])

static LIVE: AtomicU64 = AtomicU64::new(0);
static HIGH: AtomicU64 = AtomicU64::new(0);
static ACTIVE: AtomicBool = AtomicBool::new(false);

/// A global allocator that wraps `std::alloc::System` and counts requested bytes ([OS/mem §6]). Installed only by probe
/// roots, never by the product root: `#[global_allocator] static A: CountingAlloc = CountingAlloc;`.
///
/// Three process-global relaxed atomics: live bytes, their high-water mark, and whether any allocation was counted (so
/// `Meter::heap_counts` answers `None` in a binary that did not install it). `size` is the requested `layout.size()`:
/// allocator headers and rounding are not counted.
#[derive(Copy, Clone, Debug, Default)]
pub struct CountingAlloc;

impl CountingAlloc {
    /// Sets the high-water mark to the current live count, so a probe can measure one phase.
    pub fn reset_high_water() {
        HIGH.store(LIVE.load(Ordering::Relaxed), Ordering::Relaxed);
    }

    fn grew(by: u64) {
        let live = LIVE.fetch_add(by, Ordering::Relaxed) + by;
        HIGH.fetch_max(live, Ordering::Relaxed);
        // Stored once per process, only when a relaxed load reads `false` ([OS/mem §6.2]): an unconditional store would
        // write a shared cache line on every allocation and skew the measured allocation cost.
        if !ACTIVE.load(Ordering::Relaxed) {
            ACTIVE.store(true, Ordering::Relaxed);
        }
    }

    fn shrank(by: u64) {
        LIVE.fetch_sub(by, Ordering::Relaxed);
    }
}

// SAFETY: every method forwards to `System` with the caller's arguments unchanged and returns its result unchanged; the
// counting touches only atomics, never the memory, so `System`'s guarantees carry over.
unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: forwarded under the caller's `GlobalAlloc::alloc` contract.
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            Self::grew(layout.size() as u64);
        }
        p
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: forwarded under the caller's `GlobalAlloc::alloc_zeroed` contract.
        let p = unsafe { System.alloc_zeroed(layout) };
        if !p.is_null() {
            Self::grew(layout.size() as u64);
        }
        p
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: forwarded under the caller's `GlobalAlloc::dealloc` contract.
        unsafe { System.dealloc(ptr, layout) };
        Self::shrank(layout.size() as u64);
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: forwarded under the caller's `GlobalAlloc::realloc` contract.
        let p = unsafe { System.realloc(ptr, layout, new_size) };
        if !p.is_null() {
            let (old, new) = (layout.size() as u64, new_size as u64);
            if new > old {
                Self::grew(new - old);
            } else {
                Self::shrank(old - new);
            }
        }
        p
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// The Meter

fn meter_err(what: &'static str) -> MeterError {
    MeterError {
        os: OsCode(last_error() as i32),
        what,
    }
}

impl Meter for OsMeter {
    fn free_space(&self, dir: &Path) -> Result<FreeSpace, VfsError> {
        let v = verbatim(dir).ok_or_else(|| {
            VfsError::new(
                VfsErrorKind::InvalidName,
                OsCode::NONE,
                "GetDiskFreeSpaceExW",
            )
        })?;
        free_space_of(&v)
    }

    fn private_now(&self) -> Result<u64, MeterError> {
        // SAFETY: the pseudo-handle of the current process is always valid.
        Ok(memory_counters(unsafe { GetCurrentProcess() })?.PrivateUsage as u64)
    }

    fn private_peak(&self) -> Result<u64, MeterError> {
        // SAFETY: the pseudo-handle of the current process is always valid.
        Ok(memory_counters(unsafe { GetCurrentProcess() })?.PeakPagefileUsage as u64)
    }

    fn available_physical(&self) -> Result<u64, MeterError> {
        let mut m = MEMORYSTATUSEX {
            dwLength: core::mem::size_of::<MEMORYSTATUSEX>() as u32,
            ..Default::default()
        };
        // SAFETY: `m` is a live `MEMORYSTATUSEX` whose length field is set.
        if unsafe { GlobalMemoryStatusEx(&mut m) } == 0 {
            return Err(meter_err("GlobalMemoryStatusEx"));
        }
        Ok(m.ullAvailPhys)
    }

    fn peak_of_child(&self, child: &std::process::Child) -> Result<ChildPeak, MeterError> {
        peak_of_child(child)
    }

    fn heap_counts(&self) -> Option<HeapCounts> {
        ACTIVE.load(Ordering::Relaxed).then(|| HeapCounts {
            live_bytes: LIVE.load(Ordering::Relaxed),
            high_water_bytes: HIGH.load(Ordering::Relaxed),
        })
    }

    fn reset_heap_high_water(&self) {
        if ACTIVE.load(Ordering::Relaxed) {
            CountingAlloc::reset_high_water();
        }
    }

    fn child_private_now(&self, child: &std::process::Child) -> Result<u64, MeterError> {
        Ok(memory_counters(child.as_raw_handle())?.PrivateUsage as u64)
    }

    fn child_threads(&self, child: &std::process::Child) -> Result<u32, MeterError> {
        let pid = child.id();
        // SAFETY: `CreateToolhelp32Snapshot` has no pointer arguments; the handle is checked.
        let snap = owned(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) })
            .ok_or_else(|| meter_err("CreateToolhelp32Snapshot"))?;
        let mut e = THREADENTRY32 {
            dwSize: core::mem::size_of::<THREADENTRY32>() as u32,
            ..Default::default()
        };
        let mut n = 0u32;
        // SAFETY: `e` is a live entry with `dwSize` set, as the call requires.
        let mut ok = unsafe { Thread32First(raw(&snap), &mut e) };
        while ok != 0 {
            if e.th32OwnerProcessID == pid {
                n += 1;
            }
            // SAFETY: as above.
            ok = unsafe { Thread32Next(raw(&snap), &mut e) };
        }
        Ok(n)
    }

    fn cpu_times(&self, child: Option<&std::process::Child>) -> Result<CpuTimes, MeterError> {
        let h = match child {
            Some(c) => c.as_raw_handle(),
            // SAFETY: the pseudo-handle of the current process is always valid.
            None => unsafe { GetCurrentProcess() },
        };
        let t = process_times(h).map_err(|e| MeterError {
            os: OsCode(e as i32),
            what: "GetProcessTimes",
        })?;
        Ok(CpuTimes {
            user_ns: filetime_span_ns(&t[3]),
            kernel_ns: filetime_span_ns(&t[2]),
        })
    }

    fn prepare_child(&self, _cmd: &mut std::process::Command) -> Result<ChildTicket, MeterError> {
        // Windows reads the peak from the process handle: nothing to arrange ([OS/proc §9]).
        Ok(ChildTicket(0))
    }

    fn bind_child(&self, _ticket: ChildTicket, _child: &std::process::Child) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn own_readings() {
        let m = OsMeter;
        let now = m.private_now().unwrap();
        let peak = m.private_peak().unwrap();
        assert!(now > 0 && peak >= now, "{now} {peak}");
        assert!(m.available_physical().unwrap() > 0);
        // Process times advance in scheduler ticks (≈ 15.6 ms): spin until one has passed.
        let start = std::time::Instant::now();
        let mut x = 0u64;
        while m.cpu_times(None).unwrap().user_ns == 0 && start.elapsed().as_secs() < 5 {
            x = std::hint::black_box(x.wrapping_mul(31).wrapping_add(7));
        }
        let t = m.cpu_times(None).unwrap();
        assert!(t.user_ns > 0, "{t:?}");
        m.reset_heap_high_water();
        let fs = m.free_space(Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();
        assert!(fs.total > 0 && fs.available <= fs.total);
    }

    #[test]
    fn counting_rules() {
        let a = CountingAlloc;
        let before = LIVE.load(Ordering::Relaxed);
        let layout = Layout::from_size_align(1000, 8).unwrap();
        // SAFETY: a non-zero-size layout; the block is freed below with the same layout.
        let p = unsafe { a.alloc(layout) };
        assert!(!p.is_null());
        assert_eq!(LIVE.load(Ordering::Relaxed), before + 1000);
        assert!(HIGH.load(Ordering::Relaxed) >= before + 1000);
        // SAFETY: `p` was allocated with `layout`; the new size is non-zero.
        let q = unsafe { a.realloc(p, layout, 400) };
        assert!(!q.is_null());
        assert_eq!(LIVE.load(Ordering::Relaxed), before + 400);
        CountingAlloc::reset_high_water();
        assert_eq!(HIGH.load(Ordering::Relaxed), before + 400);
        // SAFETY: `q` holds 400 bytes with the original alignment.
        unsafe { a.dealloc(q, Layout::from_size_align(400, 8).unwrap()) };
        assert_eq!(LIVE.load(Ordering::Relaxed), before);
        assert!(
            OsMeter.heap_counts().is_some(),
            "counted once, installed from now on"
        );
    }
}
