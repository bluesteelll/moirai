# OS layer: memory metering (`os::mem`, `CountingAlloc`, the `Meter` implementation)

| Field | Value |
|---|---|
| Title | OS layer specification, part 2: private bytes, available physical memory, child peaks, the counting allocator and the `Meter` implementation |
| Status | draft, pass 1 pending |
| Work package | WP-17b (role R-SPEC-P), part 2 of WP-17 ([PLAN §3.2] item 1) |
| Sources | [80 §2.1] (`os::mem` row); [80 §2.9] (metering per OS, gate form, floors, allocator, system-library linking); [60 §5.1] (RSS, idle-CPU and spawn rows of the measurement protocol); [60 §5.2] items 11, 19, 21; [60 §5.3] (floor-relative gates); [AR §8.2] items 11, 19; [AR §8.3] RAM rows; [50 §5.10] (`mem` from private bytes); [90 §11.3] (system allocator; pure Rust); PLAN §2.2 (`moirai-vfs` `Meter`; `moirai-os` `mem`, `CountingAlloc`), §3.2 WP-05 (guard), WP-51 (load generator), WP-52 (measurement 11), WP-56 (measurement 19), §3.3 gap "`os::fs`/`mem`/`proc` additions"; review `a1-P.md` A1P-09 and its R1 condition 2 |
| Reconciled with | [OS/README §2.4, §3, §4.3] (the `Meter` trait and its types are fixed there and unchanged here), [OS/proc §9] (`peak_of_child`, `prepare_child`, `bind_child`), [OS/fs §4.11] (`free_space`) |

---

## 1. Scope and placement

- **The `Meter` trait** is [OS/README §4.3]'s, with its types `ChildPeak`, `PeakKind`, `HeapCounts`, `CpuTimes`,
  `ChildTicket` and `MeterError`; this file does not change a signature. It fixes each method's meaning and per-OS source:
  the process's own readings (§3), the child peak and the pre-spawn hook (§4), available memory (§5), the heap counters
  (§6) and the running-child readings that measurement 19 and the measurement protocol need (§7), which README §4.3
  adopted from this file's first draft.
- **Implementation:** `moirai_os::OsMeter` ([OS/README §2.2]) implements `Meter` by calling `os::fs::free_space`
  ([OS/fs §4.11]), the `os::mem` functions of this file and `os::proc::peak_of_child` ([OS/proc §9]), which `os::mem`
  re-exports so that [OS/README §3]'s `os::mem` surface lists it.
- **`CountingAlloc`** is a type of `moirai-os` (it needs `unsafe impl GlobalAlloc`); it is installed as
  `#[global_allocator]` only by probe roots, never by `moirai` ([OS/README §2.4], `a1-P.md` R1 condition 2).
- **Consumers.** The probes (`moirai-probes`, generic over `M: Meter`), the guard and load generator of
  `moirai-probes-bin` (WP-05, WP-51), and the product: the query engine sets its `mem` budget from `private_now`
  ([50 §5.10], A1P-09) and the store's guards read free space.

## 2. Quantities

| Quantity | Definition | Gate or use |
|---|---|---|
| **private bytes** (`private_now`) | the process's committed private memory now: pages it alone owns, resident or paged out, excluding shared file-backed and image pages | every RSS gate of [AR §8.3] is a peak of this quantity; `mem` budgets |
| **private peak** (`private_peak`) | the maximum of private bytes over the process's life so far | the RSS gates, read at exit |
| **child peak** (`peak_of_child`) | the private peak of a child process that has exited | measurement 11 (floors), the `peak` probe, every "peak private bytes at exit" row of [60 §5.1] |
| **available physical** (`available_physical`) | physical memory the OS can give to processes without paging anything out: free and zeroed pages plus reclaimable cache, in bytes | WP-05's guard (refuse below 1.5 GB), WP-51's load generator (hold ≈ 1.8 GB) |
| **heap live and high-water** (`heap_counts`) | bytes requested through the global allocator and not yet freed, and their maximum | reported beside every RSS gate as the portable `heap_peak` ([80 §2.9]) |
| **never a gate** | `ru_maxrss` (kilobytes on Linux, bytes on macOS, includes file-backed pages); `/proc/*/status` RSS; the working set | — ([80 §2.9]); the working set peak is reported, not gated ([60 §5.1]) |

## 3. `private_now` and `private_peak`

| Method | Windows (built) | Linux (port) | macOS (port) |
|---|---|---|---|
| `private_now` | `GetProcessMemoryInfo(GetCurrentProcess(), PROCESS_MEMORY_COUNTERS_EX)` → `PrivateUsage` | `/proc/self/smaps_rollup`: `(Anonymous + Swap − LazyFree) × 1024` (kB fields), plus `VmPTE × 1024` from `/proc/self/status` | `task_info(mach_task_self(), TASK_VM_INFO)` → `task_vm_info.phys_footprint` |
| `private_peak` | same call → `PeakPagefileUsage` | cgroup-v2 `memory.peak` of the process's own cgroup, only when the harness placed the process alone in a delegated leaf cgroup (`/proc/self/cgroup` names a cgroup whose `cgroup.procs` lists only this pid); otherwise `Err(MeterError { what: "no private peak without a leaf cgroup" })` | `task_vm_info.ledger_phys_footprint_peak` (public Mach API) |

- Units are bytes (u64). Each call is one syscall or one small file read; neither allocates on the heap beyond a fixed
  stack buffer (Linux reads `smaps_rollup` into a 4 KiB stack buffer).
- `private_now` is cheap enough to call at the start of every query ([50 §5.10]): one `GetProcessMemoryInfo` on Windows
  (≈ µs). On Linux, `smaps_rollup` walks every mapping of the process under its memory-map lock, which costs tens to
  hundreds of microseconds with hundreds of mappings; the port measures it (port-phase probe) and, if it exceeds the
  per-query budget, caches the reading per request: a long-lived MCP server reads it once per request, not per query of
  a request (pass 1, P1-36). macOS `task_info` is one Mach call.
- A failed reading is a `MeterError`, never a store error ([OS/README §4.3]). The query engine then uses the default that
  [CFG] and [LQ] give for `mem` when private bytes are unknown; this file sets no value.

## 4. `peak_of_child`

The contract and per-OS readings are [OS/proc §9]. In order: `prepare_child(&mut command)` before the spawn (returns a
`ChildTicket`; on Windows `ChildTicket(0)` and nothing else happens), the caller's `Command::spawn`, `bind_child(ticket,
&child)` (Windows: records nothing), the caller's `Child::wait`, then `peak_of_child(&child)` before `child` is dropped.
On Windows the result is `ChildPeak { private_peak_bytes: PeakPagefileUsage, kind: Peak }`. On Linux without a delegated
leaf cgroup, and on macOS after the child was reaped, the result is `Err(MeterError)`: a peak that cannot be read is
never replaced by a guess. `PeakKind::AtExit` is reserved for the Linux at-exit reading of [80 §2.9] if the port adds a
child self-report.

Measurement 11 checks, for each process kind, that the Windows child reading equals the value the child itself reads
through `private_peak` just before it exits (within one page), and records VMMap's private, page-table and shareable
breakdown beside it ([60 §5.1] RSS row, [71 RAM-m9]).

## 5. `available_physical`

| Windows (built) | Linux (port) | macOS (port) |
|---|---|---|
| `GlobalMemoryStatusEx` → `MEMORYSTATUSEX.ullAvailPhys` (Task Manager's "Available": free, zeroed and standby pages) | `/proc/meminfo` `MemAvailable` × 1024 | `host_statistics64(mach_host_self(), HOST_VM_INFO64)` → `(free_count + inactive_count + purgeable_count) × vm_kernel_page_size` [I]; the port's probes compare it with Activity Monitor before any gate uses it |

WP-05's guard and WP-51's load generator use it on Windows only at M0. It is a measurement input, never a store rule.

## 6. `heap_counts` and `CountingAlloc`

### 6.1 The type

```rust
/// A global allocator that wraps `std::alloc::System` and counts requested bytes.
/// Installed only by probe roots: `#[global_allocator] static A: CountingAlloc = CountingAlloc;`
pub struct CountingAlloc;

unsafe impl core::alloc::GlobalAlloc for CountingAlloc { /* §6.2 */ }

impl CountingAlloc {
    /// Sets the high-water mark to the current live count, so a probe can measure one phase.
    pub fn reset_high_water();
}
```

### 6.2 Counting rules

Three process-global atomics, all accessed with `Ordering::Relaxed`: `LIVE: AtomicU64` (bytes), `HIGH: AtomicU64` (bytes),
`ACTIVE: AtomicBool`.

| Operation | After calling `System` | Counts |
|---|---|---|
| `alloc(layout)`, `alloc_zeroed(layout)` | on a non-null result | `live = LIVE.fetch_add(size) + size`; `HIGH.fetch_max(live)`; `ACTIVE.store(true)` only when a relaxed `ACTIVE.load()` reads false (once per process) |
| `dealloc(ptr, layout)` | always | `LIVE.fetch_sub(size)` |
| `realloc(ptr, layout, new_size)` | on a non-null result | if `new_size > size`: `live = LIVE.fetch_add(new_size − size) + (new_size − size)`, `HIGH.fetch_max(live)`; else `LIVE.fetch_sub(size − new_size)` |
| `realloc` | on a null result | nothing (the old block is still allocated) |

- `size` is `layout.size()`: the **requested** bytes. Allocator headers, size-class rounding and alignment padding are not
  counted; the difference from private bytes is what VMMap's breakdown shows (measurement 11).
- With `Relaxed` ordering and the ≤ 2 threads a moirai process runs, a concurrent allocation can make `HIGH` miss a
  transient peak by at most the bytes of allocations in flight on the other thread; the counters are a report, not a gate.
- The cost per allocation is two relaxed atomic read-modify-writes and one relaxed load (`ACTIVE` is stored once, after
  a load reads false, so it adds no third read-modify-write; pass 1, P1-36); this is why the product binary never
  installs it.
- `Meter::heap_counts()` returns `Some(HeapCounts { live_bytes: LIVE, high_water_bytes: HIGH })` when `ACTIVE` is set and
  `None` otherwise: in a binary that installed `CountingAlloc`, the first allocation of the process sets `ACTIVE`, so
  `None` means "not installed" ([OS/README §4.3]).
- `reset_high_water()` stores `LIVE` into `HIGH`. `Meter::reset_heap_high_water` calls it, and is a no-op when `ACTIVE` is
  clear ([OS/README §4.3]).

### 6.3 The allocator itself

The global allocator is the system allocator on every OS ([80 §2.9], [90 §11.3]): owner decision #44 makes every
dependency pure Rust, which excludes mimalloc and jemalloc. `CountingAlloc` changes the accounting, not the allocator.
Region arenas carry the hot allocations ([AR §6.1]); on Windows they are `VirtualAlloc` chunks of ≥ 256 KiB and are
therefore counted in private bytes but not in `heap_counts` (they bypass the global allocator), which the measurement
reports state.

## 7. Running-child readings and the heap reset

Measurement 19 ([60 §5.2]: "private bytes and thread count" of `moirai mcp`), [60 §5.1]'s idle-CPU row ("zero CPU-time
delta … and zero context switches … over 10 minutes") and its MCP steady-state row read a **running** child. The probe
bodies are generic over `M: Meter`, so [OS/README §4.3] carries these methods (`child_private_now`, `child_threads`,
`cpu_times`, `reset_heap_high_water`; signatures there). Their sources:

| Method | Windows (built) | Linux (port) | macOS (port) |
|---|---|---|---|
| `child_private_now` | `GetProcessMemoryInfo(child)` → `PrivateUsage` | `/proc/<pid>/smaps_rollup` as §3 | `proc_pid_rusage(pid, RUSAGE_INFO_V6).ri_phys_footprint` (harness only, X9) |
| `child_threads` | `CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD)`, count entries whose `th32OwnerProcessID` is the child's pid | `/proc/<pid>/status` `Threads` | `proc_pidinfo(PROC_PIDTASKINFO)` → `pti_threadnum` (harness only) |
| `cpu_times` | `GetProcessTimes` → user and kernel `FILETIME` × 100 ns | `/proc/<pid>/stat` fields 14–15 (`utime`, `stime`) × `10^9 / sysconf(_SC_CLK_TCK)` | `proc_pid_rusage` `ri_user_time`, `ri_system_time` (mach ticks → ns) |

**Context switches** stay outside `Meter`: on Windows they come from ETW through WP-51's counters-only WPR profile (or
`typeperf`'s `Thread(*)\Context Switches/sec` for the child's threads), which the measurement harness reads, not
`moirai-os`; on Linux from `/proc/<pid>/status` `voluntary_ctxt_switches` and `nonvoluntary_ctxt_switches`; on macOS from
`proc_pid_rusage` in the harness.

## 8. Metering rules for gates ([80 §2.9])

- **Windows keeps its absolute gates** ([AR §8.3]); the floor-relative value is reported beside each from M0.
- **Floor-relative form** (the ports' gate; reported on Windows): `peak(command) − peak(empty Rust binary, same
  toolchain, allocator, OS and run)`, the floor re-measured interleaved in the same run ([60 §5.1], [60 §5.3]); the empty
  binary is `moirai-probes-bin empty` (PLAN §3.2 WP-50).
- **`heap_peak`** (the `CountingAlloc` high-water) is reported beside every RSS gate in measure builds.
- **Linking.** On macOS the binary links only `libSystem` (checked with `otool -L`); on Linux it is static musl
  ([80 §2.13]). Nothing here adds a native library.

## Coverage

No `COVERAGE.md` row cites this file ([F01 §2.7]): metering freezes no byte and no rule of [60 §2.5], [40 §2.11],
[50 §8.1], [80 §3] or [90 §10.1]. The file specifies the sources of the `Meter` seam ([OS/README §4.3]), which the
memory and CPU gates of [60 §3.13] and [60 §5] read.

| Item | Part covered here | Section |
|---|---|---|
| — | none | — |

## Holes

| Id | What | Decided by | Candidates | Constraint the value must meet |
|---|---|---|---|---|
| — | none: every source is fixed by [80 §2.9]; measurement 11 verifies the Windows readings but decides no value | — | — | — |

## Open points for the review

| # | Point | Resolution in this file | For |
|---|---|---|---|
| 1 | The first `Meter` could not read a running child (measurement 19's private bytes and thread count, [60 §5.1]'s idle-CPU and MCP steady-state rows) or reset the heap high-water mark for a phase | resolved: [OS/README §4.3] adopted the four methods and `CpuTimes` of §7 | — |
| 2 | Linux `peak_of_child` and `private_peak` need a delegated leaf cgroup created before the spawn ([80 §2.9]); macOS's reading needs the child unreaped | [OS/README §4.3] adopted `prepare_child`/`bind_child` (no-ops on Windows); the Linux leaf cgroup and the macOS non-reaping read are port-phase items ([OS/proc §9]); until then those readings return `MeterError`, never a guess | port phase |
| 3 | PLAN WP-17's line names `mem::peak_of_child`, PLAN §2.2 puts `peak_of_child` in `proc` | implemented in `os::proc` (it needs the process handle, [OS/proc §9]) and re-exported by `os::mem`; reached by generic code only through `Meter::peak_of_child` | R-REV-P |
| 4 | The macOS `available_physical` formula is not in [80] | proposed in §5 with an [I] tag; the port's probes confirm it before any gate reads it | port phase |
| 5 | Region arenas bypass the global allocator, so `heap_counts` understates heap use on paths that use them | stated in §6.3; measurement reports print both `heap_peak` and private bytes | WP-50, WP-52 |
| 6 | PLAN §3.3 gap "`os::fs`/`mem`/`proc` additions" (the part of it that falls to part 2) | `os::mem`: `private_now`, `private_peak`, `available_physical`, `CountingAlloc` (and the §7 proposals); `os::proc`: `peak_of_child`, `parent_image`, the parent watch ([OS/proc]) | R-REV-P |
