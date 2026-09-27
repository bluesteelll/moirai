# OS layer: clocks, stamps, deadlines and the fault-model clock rules

| Field | Value |
|---|---|
| Title | OS layer specification, part 2: the wall, monotonic and boot clocks; stamps and lease deadlines; which rule uses which clock |
| Status | draft, pass 1 pending |
| Work package | WP-17b (role R-SPEC-P), part 2 of WP-17 ([PLAN §3.2] item 1) |
| Sources | [80 §2.3.5] item (7); [80 §2.7.1] (boot clock row, "Lease deadlines", "Verification"); [80 §2.11.3] (racy threshold from a file-system timestamp); [80 §3.1] X-F2; [80 §3.2] row "HLC and timestamps"; [60 §2.5] `Vfs` fault-model item (7) and protocol decision (e); [60 §3.1] item 2 (the Store API's injected clock); [60 §5.2] item 22; [AR §4.3] (`hlc`, `append_hlc`), [AR §4.9] (GC grace), [AR §6.2] (lease deadlines, renewal), [AR §6.4] (idempotency windows), [AR §8.2] item 22, [AR §8.3] GT4 clock-step variant, [AR §13] (duration keys); [40 §4.2] (quiescence), [40 §4.3] (E7, G4 window, copy rule); [90 §4.4] (`session-ttl` renewal); review `a1-S.md` S-17 (clock domains) |
| Reconciled with | [OS/README §4.4] (the `Clock` signature, fixed there and unchanged here), [OS/proc] (boot identity), [F15 §3.7] (FM-6, FM-7), [F20 §1.2, §5.1, §5.12.1] (time comparisons, `SKEW`, the racy threshold), [F01 §5.7] (the HLC layout) |

---

## 1. Scope and placement

- The **`Clock` trait** is a sub-trait of `Vfs`; its signature is [OS/README §4.4]'s and is not changed here (README open
  point 7). This file owns its semantics and per-OS sources (§2).
- The **stamp** and **deadline** value type (`Stamp`, 24 bytes) and its pure evaluation functions (§3, §4) are in
  `moirai-vfs`; the `LEASES` row that stores a deadline is [F11]'s.
- The **clock rules** of fault-model item (7) and protocol decision (e) (§5, §6), the HLC's physical input (§7) and the
  clock-domain conversions (§8) are normative here; the HLC field is [F06]'s and the protocol step that assigns it is
  [F16]'s.
- The Store API's "injected deterministic clock" ([60 §3.1] item 2, [API]) is an implementation of `Clock` plus a
  simulated boot identity; the reference model keeps its own ([60 §4]).

## 2. The three clocks

```rust
// [OS/README §4.4], unchanged:
pub trait Clock {
    /// Wall clock: milliseconds since the Unix epoch, UTC. May step backward or forward between two calls.
    fn wall_ms(&self) -> i64;
    /// Monotonic clock in nanoseconds from an unspecified origin; never goes backward within a process.
    fn mono_ns(&self) -> u64;
    /// Boot clock in nanoseconds since boot: monotonic and including time spent in suspend ([80 §2.7.1]).
    fn boot_ns(&self) -> u64;
}
```

| Clock | Unit and origin | Guarantee | Comparable across | Used for | Never used for |
|---|---|---|---|---|---|
| wall | ms since 1970-01-01T00:00:00Z | none: may step backward or forward by any amount between two calls (fault-model item (7)) | every process on every machine, as far as clocks agree | the HLC's physical input (§7); the wall component of a stamp (§3); displayed times | an interval that decides correctness on its own |
| mono | ns from an unspecified origin that is fixed for one boot | never decreases within one boot; comparable across every process of one boot; **may exclude time spent in suspend** ([F15] FM-7.2) | processes of one boot | in-process intervals: lock-wait deadlines, retry bounds, budgets, the 50 ms quiescence (§6) | any value written to disk; any interval that must count suspend (those use the boot clock, [F15] OP-14) |
| boot | ns since the kernel booted | never decreases within one boot; **includes time spent in suspend and hibernation**; identical for every process of one boot | processes of one boot only, which is why every stored boot-clock value carries `boot_hash` beside it | the boot component of a stamp: lease deadlines, grace intervals across processes (§4) | anything across boots |

### 2.1 Per-OS sources

| Clock | Windows (built) | Linux (port) | macOS (port) |
|---|---|---|---|
| wall | `GetSystemTimePreciseAsFileTime` → FILETIME `f`; `wall_ms = (f − 116 444 736 000 000 000).div_euclid(10 000)` | `clock_gettime(CLOCK_REALTIME)` → `sec × 1000 + nsec / 10^6` (floor) | `clock_gettime(CLOCK_REALTIME)`, same conversion |
| mono | `QueryPerformanceCounter` scaled by `QueryPerformanceFrequency` to ns (u128 intermediate); the system-wide counter, with no per-process origin subtracted | `clock_gettime(CLOCK_MONOTONIC)` | `clock_gettime_nsec_np(CLOCK_UPTIME_RAW)` |
| boot | `QueryInterruptTimePrecise` (100 ns units since boot, including sleep and hibernation) × 100; HOLE(OS-win-boot-clock) confirms the source | `clock_gettime(CLOCK_BOOTTIME)` | `mach_continuous_time()` × `numer` / `denom` of `mach_timebase_info` (u128 intermediate) |

- Arithmetic saturates: a conversion that would overflow its type returns the type's maximum; a negative wall value is
  impossible for a correctly set clock and is passed through as returned (the stamp clamps it, §3).
- In builds with the `test-host` feature the wall value is shifted by the process's test-host wall offset (§9). The boot
  and mono clocks are never shifted: they cannot step.

## 3. Stamps

A **stamp** records "now" in a form another process of the same boot can compare with its own now, and that a process of
another boot, or one without a boot identity, can still judge by the wall clock.

### 3.1 Layout (24 bytes, little-endian, packed)

| Offset | Width | Type | Name | Meaning |
|---|---|---|---|---|
| 0 | 8 | u64 | `wall` | wall clock in ms since the Unix epoch; a negative `wall_ms` is stored as 0 |
| 8 | 8 | u64 | `boot_hash` | `boot_hash` of the writer's boot ([OS/proc §4.3]); 0 = the writer was in Unknown-boot mode |
| 16 | 8 | u64 | `mono` | the boot clock in ns (named `mono` in [80] X-F2, "mono being the boot clock"); 0 when `boot_hash` = 0 |

This is the lease deadline form `expires = {wall, boot_hash, mono}` of [80] X-F2 and [AR §6.2]; [F11] stores it in
`LEASES` unchanged. In Rust the third field is named `boot_ns`, so it cannot be confused with `Clock::mono_ns`, which is
never stored.

### 3.2 Constructing now

```
now(clock, boot) = Stamp {
  wall:      max(0, clock.wall_ms()) as u64,
  boot_hash: if boot = Known(b) { b.hash() } else { 0 },
  boot_ns:   if boot = Known(_) { clock.boot_ns() } else { 0 },
}
```

## 4. Deadlines and intervals

### 4.1 The never-expiring deadline

`Stamp::NEVER = { wall: u64::MAX, boot_hash: 0, boot_ns: u64::MAX }`. It is the deadline of a run-scoped lease ([AR §6.2]:
released only by `apply`, `run close` or `reclaim`). With `boot_hash` = 0 it is judged by the wall clock, which never
reaches `u64::MAX`, so it neither expires nor turns Dead at a boot change; run-scoped leases keep their own rule after a
reboot ([AR §6.2]).

### 4.2 Construction

```
after(now, ttl) = Stamp {
  wall:      now.wall.saturating_add(ttl_ms),
  boot_hash: now.boot_hash,
  boot_ns:   if now.boot_hash ≠ 0 { now.boot_ns.saturating_add(ttl_ns) } else { 0 },
}
```

`ttl_ms` and `ttl_ns` are the same duration in the two units (`ttl` is a duration key of [CFG], e.g. `lease.ttl-default`
15 min).

### 4.3 Evaluation (frozen with X-F2)

`state(d, now) → NotPassed | Passed | BootChanged`, first matching row:

| # | Condition | Result |
|---|---|---|
| 1 | `d.boot_hash ≠ 0` and `now.boot_hash ≠ 0` and `d.boot_hash = now.boot_hash` | `Passed` iff `now.boot_ns ≥ d.boot_ns`, else `NotPassed` |
| 2 | `d.boot_hash ≠ 0` and `now.boot_hash ≠ 0` and they differ | `BootChanged` |
| 3 | otherwise (either side in Unknown-boot mode) | `Passed` iff `now.wall ≥ d.wall`, else `NotPassed` |

- Row 1 is why a suspended laptop's 15-minute lease expires by elapsed time and a wall-clock step changes no expiry
  ([80 §2.7.1]; GT4's ±1 h clock-step variant, [AR §8.2]).
- Row 2 feeds the boot rule of [AR §6.2]: after a reboot every non-run-scoped lease is Dead and is released at the first
  read with a triage line. The lease layer maps `BootChanged` to Dead for TTL leases; `Stamp::NEVER` never reaches row 2.
- Row 3 is Unknown-boot mode's rule ([OS/proc §5] U3): "the lease's wall-clock deadline applies".

### 4.4 Renewal (half-TTL rule)

A TTL lease is renewed by use when more than half its TTL has elapsed ([AR §6.2], [90 §4.4]). With `d` the current
deadline and `ttl` the lease's TTL:

```
due_for_renewal(d, ttl, now) =
  if d.boot_hash ≠ 0 and d.boot_hash = now.boot_hash { d.boot_ns.saturating_sub(now.boot_ns) < ttl_ns / 2 }
  else                                                 { d.wall.saturating_sub(now.wall)       < ttl_ms / 2 }
```

The renewal writes `after(now, ttl)` (one lazy runtime record, [AR §6.2]). A `session-ttl` anchor's deadline is renewed
by the same rule when the thread's own server serves a call ([90 §4.4]).

### 4.5 Elapsed intervals across processes (grace)

`elapsed_at_least(start, g, now)`, for an interval `g` measured from a stamp another process wrote:

| # | Condition | Result |
|---|---|---|
| 1 | both `boot_hash` known and equal | `now.boot_ns − start.boot_ns ≥ g_ns` (saturating) |
| 2 | both known and different | `true`: every process of the earlier boot is gone |
| 3 | otherwise | `now.wall − start.wall ≥ g_ms` (saturating: a backward step yields 0, never a negative interval) |

This rule serves the 60 s delete-pending grace of GC ([AR §4.9]) and any other cross-process grace. Its only
wall-clock case (row 3, Unknown-boot mode) can end a grace early after a forward wall step; the consequence is benign
(the file's deletion is still behind the two-slot barrier; on Windows a delete of a mapped file fails and is retried, on
Unix an unlinked mapped inode stays valid), and it is recorded here as the one place the wall clock shortens an interval.

## 5. Fault-model item (7) and the simulator

Fault-model item (7) as amended ([80 §2.3.5], [60 §2.5]): *the wall clock may step backward or forward by any amount
between two calls; a monotonic clock never goes backward; a boot clock, monotonic and including suspend, serves lease
deadlines; and a process may be unable to read its boot identity (Unknown-boot mode).* [F15 §3.7] (FM-7.1–FM-7.6) is its
normative statement; this file's clocks satisfy it, and the simulator (`moirai-vfs-sim`, WP-31) implements it as
[F15 §3.7]'s "In-memory `Vfs`" paragraph states. In terms of the three clocks of §2:

| Obligation | Simulator behaviour |
|---|---|
| wall steps (FM-7.1) | between any two events the adversary may set any simulated process's wall clock to any value, backward or forward, within the `i64` millisecond domain |
| mono (FM-7.2) | one per simulated boot, shared by its processes; never decreases; a simulated suspend may or may not advance it |
| boot (FM-7.3) | one per simulated boot, shared by its processes; never decreases; a simulated suspend advances it by the suspended duration |
| reboot | a new simulated boot: both clocks restart, the boot identity changes, every simulated process ends |
| Unknown-boot mode (FM-7.4) | Known or Unknown fixed per simulated process at its start; any single read may also answer Unknown ([OS/proc §4.4] caches the first answer, which keeps the product correct either way) |
| pauses (FM-6) | any process may pause for any time between two events; both clocks keep advancing |

GT4's ±1 h clock-step variant runs on real Windows through the test-host wall offset (§9), never by setting the system
clock (that needs elevation, which profile L never uses).

## 6. Which rule uses which clock (protocol decision (e))

Decision (e) of [60 §2.5]: *lease TTLs, the HLC and the GC grace are specified against the monotonic clock where the
fault model's clock steps would otherwise break them.* Every time-dependent rule of the design uses exactly one of these
sources:

| Rule | Clock | Consequence of a wall step | Source |
|---|---|---|---|
| lease deadline, `session-ttl` deadline, half-TTL renewal | stamp (§4.3, §4.4): the boot clock on the same boot; the wall clock only in Unknown-boot mode | none on a known boot; in Unknown-boot mode a forward step can end a lease early — the documented cost of the mode | [AR §6.2], [80] X-F2 |
| boot change (every non-run-scoped lease Dead; boot-change recovery) | the boot identity, never a clock | none | [AR §4.2], [OS/proc §4] |
| GC delete-pending grace (60 s) | §4.5 | none on a known boot | [AR §4.9] |
| lock waits (`lock.writer-wait-ms`, `lock.flush-wait-ms`), the share-violation retry bound, `file mv --retry-ms`, read and settle budgets (`files.read-budget-ms`, `files.session-start-cap-ms`, `files.links-sync-ms`), the MCP server's ≤ 5 ms slices, the 50 ms quiescence | mono, in-process | none | [OS/lock], [OS/fs], [40 §4.2], [AR §6.1] |
| `hlc` of a commit, `append_hlc`, every `hlc` stamp in runtime records (`acquired_hlc`, `created_hlc`, `verified_at`, `missing_since`, `PENDING` and `path_moves` entries) | the HLC (§7): the wall clock made monotonic in log order | a backward step never decreases the HLC; a forward step moves it forward for good | [AR §4.3], [50] F14 |
| retention windows: `idempotency.default-window` (10 min), `gc.reflog-expire`, `gc.cruft-delay`, `gc.trash-expire`, `gc.fileobs-idle-expire`, `files.pending-escalate`, the 30-day explicit-key idempotency retention, the `PENDING` 30-day retention | HLC milliseconds: an entry's `hlc >> 16` against `max(now.wall, newest hlc >> 16 of the view)` | a forward step can end a window early; for `idempotency.default-window` that can turn a key-less retry after ≥ the step into a new commit. These are retention policies, not safety rules; the explicit-key path is unaffected within its 30 days | [AR §6.4], [AR §13] |
| the Linux and macOS frontier's racy threshold (a directory changed since the last settle) | a **file-system timestamp**: the mtime of `<store>/tmp/settle.stamp` rewritten at the start of a settle ([OS/project §5.9], [F20 §5.12.1]), never a process clock | none (both sides are file-system time) | [80 §2.11.3] |
| comparisons between file times, HLCs and git committer times (copy-rule lines 2–3, E7 on Windows, E8, G4's window, `planned` binding) | the conversions of §8 and the margin `SKEW` = HOLE(F20-clock-skew), both applied by [F20 §5.1] | bounded by the margin; each comparison takes the side that never adds an automatic re-bind | [40 §4.3], `a1-S.md` S-17 |
| `ProcId.start` | the OS process start clock, equality only | none | [OS/proc §3] |

## 7. The HLC's use of the clock

The `hlc` of a commit is a hybrid logical clock `ms << 16 | counter` ([AR §4.3]); its field is [F06]'s and its assignment
step [F16]'s. The clock rule it must satisfy is fixed here:

```
hlc_next = max( (max(0, wall_ms) as u64) << 16 ,  hlc_last + 1 )
```

where `hlc_last` is the greatest `hlc` (for `append_hlc`: the greatest `append_hlc`) in the log as scanned under the
writer byte ([80 §2.4.3] phase 2a step 3). Consequences: `hlc` never decreases in log order whatever the wall clock does
(a backward step advances the counter; 65,536 commits in one millisecond carry into the millisecond, which is harmless);
a forward step moves the physical part forward for good. `append_hlc` follows the same rule and is therefore monotonic in
`seq` order ([50] F14, I43′). A foreign or imported commit keeps its own `hlc`; only `append_hlc` is this store's.

## 8. Clock domains and conversions

Review finding S-17 (`a1-S.md`) requires one conversion between the clock domains the resolver compares. The OS layer
fixes each domain's unit and origin; [F20 §5.1] fixes the comparison rules (`teq`, `tge`, "clearly after", "possibly
after") and the margin `SKEW` = HOLE(F20-clock-skew).

| Domain | Unit and origin | To ns since the Unix epoch | Produced by |
|---|---|---|---|
| wall | ms since the Unix epoch | `× 10^6` | `Clock::wall_ms` |
| HLC | `ms << 16 \| counter` | `(hlc >> 16) × 10^6` (the counter is dropped) | §7 |
| file-system timestamp (`FsTime`) | i64 ns since the Unix epoch plus a granularity byte | identity | [OS/project §3.3] |
| git committer or author time | seconds since the Unix epoch (plus a zone, ignored) | `× 10^9` | the git object reader (M4) |
| boot clock, mono clock | ns since boot; ns since a per-process origin | **never converted**: they have no wall meaning | §2 |
| `ProcId.start` | ns (§3.2 of [OS/proc]) | never compared with other domains | [OS/proc §3] |

A file timestamp's granularity G in nanoseconds, which [F20 §5.1] uses, is `max(10^gran, VolumeCaps.mtime_granularity_ns)`
([OS/project §3.3]). The skew margin between file-system time and wall/HLC time is HOLE(F20-clock-skew) (S-17). Its
OS-level evidence comes
from measurement 15 (the difference between a file's mtime after a write and the wall clock read just before and after
the write, on the project volume) and measurement 22 (clock behaviour across a sleep); this file adds those two readings
to the measurements' row lists (open point 3).

## 9. The test-host wall offset

In builds with `moirai-os`'s `test-host` feature ([OS/README §2.4]), `wall_ms()` returns the OS wall clock plus a
process-wide offset in milliseconds (i64, default 0). The offset is set by `os::test_host` (the kill-loop harness sets it
in the child's environment before spawn: `MOIRAI_TEST_WALL_OFFSET_MS`, a decimal i64, [OS/proc §13]). `mono_ns` and
`boot_ns` are never offset. The product root `moirai` cannot enable the feature ([OS/README §2.4]), so no product build
has an offset.

## 10. The Rust surface (additions to [OS/README §4.4])

```rust
/// The 24-byte stamp and deadline form (§3.1).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Stamp { pub wall: u64, pub boot_hash: u64, pub boot_ns: u64 }

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum DeadlineState { NotPassed, Passed, BootChanged }

impl Stamp {
    pub const LEN: usize = 24;
    pub const NEVER: Stamp = Stamp { wall: u64::MAX, boot_hash: 0, boot_ns: u64::MAX };
    /// §3.2.
    pub fn now<C: Clock + ?Sized>(clock: &C, boot: &BootIdentity) -> Stamp;
    /// §4.2.
    pub fn after(&self, ttl: core::time::Duration) -> Stamp;
    /// §4.3; `self` is the deadline.
    pub fn state(&self, now: &Stamp) -> DeadlineState;
    /// §4.4; `self` is the current deadline.
    pub fn due_for_renewal(&self, ttl: core::time::Duration, now: &Stamp) -> bool;
    /// §4.5; `self` is the start of the interval.
    pub fn elapsed_at_least(&self, g: core::time::Duration, now: &Stamp) -> bool;
    pub fn to_bytes(&self) -> [u8; 24];
    pub fn from_bytes(b: &[u8; 24]) -> Stamp;
}

/// §7: the next HLC after `last`, from the wall clock.
pub fn hlc_next(wall_ms: i64, last: u64) -> u64;
```

All of these are pure functions in `moirai-vfs`; only `Clock` reaches the OS.

## 11. Verification (measurement 22)

Measurement 22 ([AR §8.2] item 22) records, on the owner's laptop, `boot_ns` and `wall_ms` read just before and just after
a sleep of known length, a hibernation, a Fast Startup cycle and a ±1 h manual wall step, in two processes, and checks
that `boot_ns` advanced by the elapsed wall time across the sleep and the hibernation (within the tick granularity),
never decreased, and agreed between the two processes; the result fills HOLE(OS-win-boot-clock).

## Holes

| Id | What | Decided by | Candidates | Constraint the value must meet |
|---|---|---|---|---|
| HOLE(OS-win-boot-clock) | The Windows source of `boot_ns` | measurement 22 (WP-52) | (a) `QueryInterruptTimePrecise` (the design's choice); (b) `QueryInterruptTime` (tick-granular); (c) `GetTickCount64` × 10^6 [I] | advances by the elapsed wall time across a sleep and a hibernation (within one clock tick); never decreases within a boot; equal in every process of the boot at the same instant; unaffected by a manual wall step. If no candidate passes, the review decides (open point 4) |

## Open points for the review

| # | Point | Resolution in this file | For |
|---|---|---|---|
| 1 | [80] X-F2 fixes the deadline's components but not their units or bytes | 24 bytes: `wall` ms since the epoch, `boot_hash`, `mono` = boot-clock ns (§3.1); `Stamp::NEVER` for run-scoped leases (§4.1); [F11]'s `LEASES` row embeds it | WP-13 |
| 2 | [60 §2.5] decision (e) lists three rules; the design has more time-dependent rules | the complete table of §6, with the one wall-clock shortening (Unknown-boot grace) and the retention windows' HLC basis stated | R-REV-P, WP-16 |
| 3 | S-17 (`a1-S.md`) puts a skew margin in chapter 20 decided by measurements 15 and 22 | the domains and units are fixed here (§8) and agree with [F20 §1.2, §5.1] (`hlc_ns(h) = (h >> 16) × 10^6`, committer seconds × 10^9); measurement 15 gains the mtime-versus-wall row ([OS/project §10] item 7) and measurement 22 the clock rows of §11; the margin is HOLE(F20-clock-skew) | WP-14b, WP-52, WP-55 |
| 4 | If measurement 22 finds no Windows clock that includes sleep, [80 §2.7.1]'s "a suspended laptop's lease expires by elapsed time" cannot hold on Windows | a clock that excludes sleep is still immune to wall steps and errs towards later expiry (safe); the review decides whether that is acceptable or Windows deadlines fall back to Unknown-boot rules | R-REV-P |
| 5 | The HLC rule (§7) is stated here as the clock rule because [AR §4.3] gives only the field shape | [F06] and [F16] adopt it or state an equivalent rule that keeps `hlc` and `append_hlc` monotonic under wall steps | WP-12, WP-16 |
| 6 | Where the clocks are specified | resolved: [OS/README §1.3, §3] list this file | — |
| 7 | [F15] FM-7.2 makes the monotonic clock comparable across the processes of one boot; [OS/README §4.4]'s doc comment says only "never goes backward within a process" | the stronger reading is adopted (§2): the implementation reads the system-wide counter and subtracts no per-process origin, which every source of §2.1 allows; README §4.4's comment may say "within one boot" | WP-17a |
| 8 | [F01] open point 15 proposes the prefix `OS-` for hole ids in `docs/spec/os/` | adopted: HOLE(OS-win-boot-clock) | WP-10 |
