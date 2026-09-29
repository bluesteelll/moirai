//! The measurement seam: the `Meter` trait and its reading types ([OS/README §4.3]; sources in [OS/mem] and
//! [OS/proc §9]).
//!
//! The product reads `Meter` too: the query engine sets its `mem` budget from `private_now` ([50 §5.10], A1P-09) and the
//! store's guards read free space ([OS/fs §4.11]). A `MeterError` is never a store error: a failed reading makes a
//! measurement or a budget fall back as [OS/mem] states, never a store refusal, and a reading that cannot be taken is
//! never replaced by a guess. `moirai-probes` is generic over `M: Meter`; `moirai-probes-bin` passes
//! `moirai_os::OsMeter`.

use core::fmt;

use crate::error::{OsCode, VfsError};
use crate::fs::FreeSpace;

/// The measurement seam ([OS/README §4.3]).
pub trait Meter: Send + Sync + 'static {
    /// Free and total bytes of the volume that holds `dir` ([OS/fs §4.11]).
    fn free_space(&self, dir: &std::path::Path) -> Result<FreeSpace, VfsError>;
    /// This process's private bytes now (the quantity every RSS gate uses, [80 §2.9]; [OS/mem §3]).
    fn private_now(&self) -> Result<u64, MeterError>;
    /// This process's peak private bytes so far ([OS/mem §3]).
    fn private_peak(&self) -> Result<u64, MeterError>;
    /// Physical memory the OS reports as available without paging, in bytes ([OS/mem §5]).
    fn available_physical(&self) -> Result<u64, MeterError>;
    /// Peak private bytes of a child that has exited and been waited for; call before `child` is dropped
    /// ([OS/proc §9]).
    fn peak_of_child(&self, child: &std::process::Child) -> Result<ChildPeak, MeterError>;
    /// Live and high-water heap bytes counted by `CountingAlloc`; `None` if this binary did not install it
    /// ([OS/mem §6]).
    fn heap_counts(&self) -> Option<HeapCounts>;
    /// Sets the heap high-water mark to the live count; a no-op when `CountingAlloc` is not installed.
    fn reset_heap_high_water(&self);

    // Running children (measurement 19 and [60 §5.1]'s idle-CPU and MCP steady-state rows; [OS/mem §7]).

    /// Private bytes of a running child now.
    fn child_private_now(&self, child: &std::process::Child) -> Result<u64, MeterError>;
    /// Number of threads of a running child.
    fn child_threads(&self, child: &std::process::Child) -> Result<u32, MeterError>;
    /// User and kernel CPU time of this process (`None`) or of a child.
    fn cpu_times(&self, child: Option<&std::process::Child>) -> Result<CpuTimes, MeterError>;

    // The pre-spawn hook ([OS/proc] open point 6, [OS/mem] open point 2). It never spawns: GT20 (a) keeps spawns out of
    // product crates; the caller spawns with `std::process::Command`.

    /// Before spawning a child whose `peak_of_child` will be read: arrange for the OS to track its peak (Linux: a
    /// delegated leaf cgroup the child joins before `exec`). Windows and macOS: nothing to arrange.
    fn prepare_child(&self, cmd: &mut std::process::Command) -> Result<ChildTicket, MeterError>;
    /// After the spawn: binds the ticket to the child, so `peak_of_child(child)` finds what `prepare_child` set up.
    fn bind_child(&self, ticket: ChildTicket, child: &std::process::Child);
}

/// Opaque; `Copy` so that a caller that drops a child without reading its peak loses nothing ([OS/README §4.3]).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct ChildTicket(pub u64);

/// User and kernel CPU time in nanoseconds ([OS/mem §7]).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct CpuTimes {
    /// User-mode time.
    pub user_ns: u64,
    /// Kernel-mode time.
    pub kernel_ns: u64,
}

/// The private peak of an exited child ([OS/proc §9]).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct ChildPeak {
    /// Peak private bytes.
    pub private_peak_bytes: u64,
    /// Whether it is a true peak.
    pub kind: PeakKind,
}

/// What a child peak measures ([OS/README §4.3]).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum PeakKind {
    /// A true peak (Windows `PeakPagefileUsage`, Linux cgroup-v2 `memory.peak`, macOS `ledger_phys_footprint_peak`).
    Peak,
    /// Read at exit only (Linux without a delegated cgroup): reported as "at-exit, not peak" ([80 §2.9]).
    AtExit,
}

/// The `CountingAlloc` counters ([OS/mem §6]): requested bytes, not allocator overhead.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct HeapCounts {
    /// Bytes allocated and not yet freed.
    pub live_bytes: u64,
    /// The maximum of `live_bytes` since the start or the last reset.
    pub high_water_bytes: u64,
}

/// A reading that could not be taken ([OS/README §4.3]); never a store error.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MeterError {
    /// The raw OS code, if any.
    pub os: OsCode,
    /// What failed, for example `"GetProcessMemoryInfo"` or `"no private peak without a leaf cgroup"`.
    pub what: &'static str,
}

impl fmt::Display for MeterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "meter reading failed: {} ({})", self.what, self.os)
    }
}

impl std::error::Error for MeterError {}
