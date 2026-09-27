# OS layer: process identity, boot identity and liveness (`os::proc`)

| Field | Value |
|---|---|
| Title | OS layer specification, part 2: `ProcId`, the boot identity and Unknown-boot mode, liveness, the parent watch, `peak_of_child`; the modules `os::spawn`, `os::ipc` and `os::test_host` |
| Status | draft, pass 1 pending |
| Work package | WP-17b (role R-SPEC-P), part 2 of WP-17 ([PLAN §3.2] item 1) |
| Files of part 2 | `proc.md` (this file), `clock.md`, `mem.md`, `project.md`, `path.md`, `shell.md`, `mapping-appendix.md` |
| Sources | [80 §1] X5, X6, X9; [80 §2.1] (`os::proc` row); [80 §2.2.3] (slot bytes); [80 §2.7.1] (`ProcId`, `boot_id`, `pidns`, boot clock, `alive`, the boot-identity rule, Unknown-boot mode, the private-API position); [80 §2.7.2] (lock-anchored liveness, the anchor table, "How it works", the MCP server lifetime); [80 §2.9] (child peak); [80 §3.1] X-F1, X-F2; [80 §8.1] M2, M3, m13; [80 §8.7] (the X-F2 amendment); [90 §4.1] (session row), [90 §4.4] (slots per harness, the frozen amendment), [90 §10.1] row "Holder anchor (X-F2)"; [AR §4.2] (`boot_id`, boot check), [AR §6.2] (holder liveness); [AR §8.2] item 22; [60 §2.5] rows `HEAD` (audits), `LOCK`, "Cross-platform", fault-model item (7); [60 §5.2] item 22; for §11–§13: [80 §2.6] (no `gc` child from a foreign PID namespace), [80 §2.8] (IPC endpoint), [80 §2.12] rows "Detached `moirai gc --rollup` child", "Bulk passes", [AR §4.9], [AR §8.2] (GT4 variants), [60 §3.5] M4 gate (the one product spawn), [60 §3.13] GT4; [F01 §3.2, §6.3, §7] (OS tags, `lp()`, hash framing); [F15] FM-7.4; [X18 §8.1] (research input only) |
| Reconciled with | [OS/README] (§3 `os::proc` row, §4.1 `Vfs`, §4.3 `Meter`, §4.4 `Clock`, §5.1 rule 3) |

Citation forms: `[OS/<file> §x]` for `docs/spec/os/`, `[Fnn]` for format chapters (cited by chapter only; their section
numbers are not fixed yet), `[CFG]`, and the design-document forms.

---

## 1. Scope and placement

This file specifies what a process can learn about itself and about other processes, and nothing that needs a store
file. It owns:

- the **OS tag** byte (§2);
- **`ProcId`**, the 32-byte diagnostic process identity (§3);
- the **boot identity** (`BootId`, `boot_hash`), its per-OS sources and its frozen rule (§4), and **Unknown-boot mode** (§5);
- **liveness**: `alive(ProcId)` for diagnostics (§6.1) and the decision procedure of lock-anchored liveness (§6.2);
- the **parent watch** of the MCP server (§7) and the parent image name used by one shell hint (§8);
- **`peak_of_child`**, the child-peak reading behind `Meter::peak_of_child` (§9);
- the Rust surface: the `ProcHost` trait and its types (§10).

The clocks, the lease deadline form and the fault-model clock rules are in [OS/clock]. The byte layouts of `LOCK`,
`SlotRec`, `WriterDiag` and the 32-byte `Anchor` belong to [F03]; `HEAD.boot_id` belongs to [F04]; the `LEASES` and
`FSINTENT` rows belong to [F11]. This file repeats the `ProcId` layout because its field values are OS-defined (§3.1);
[F03] and [F11] embed it unchanged.

It also specifies the three process-level modules that [OS/README §3] leaves to part 2 without a file: `os::spawn`
(§11), `os::ipc` (§12) and `os::test_host` (§13). `os::term` is specified in [OS/shell §10].

**Placement** ([OS/README §2, §4.1]): every type and the `ProcHost` trait are in `moirai-vfs`; `ProcHost` is a supertrait
of `Vfs`, so store code needs no extra bound; the Windows implementation is `moirai-os`'s `windows/proc.rs` (and
`spawn.rs`, `ipc.rs`, `test_host.rs`), implemented on `moirai_os::OsVfs`; the simulator implements `ProcHost` on its
`SimVfs` per simulated process.

## 2. The OS tag

One byte names the OS that wrote a runtime value ([80] X1 kind tags). It appears in `ProcId.os` (§3), in `SlotRec.os` and
`Anchor.os` ([F03]) and in the `TREES` row's `os` tag ([F11], [80 §2.11.2]). The registry is [F01 §3.2]'s, which restates
the values this file first fixed: **0 `unspecified`** (never written by a process as its own tag; uninterpretable),
**1 `windows`**, **2 `linux`**, **3 `macos`**; 4–255 reserved and uninterpretable.

A process writes the tag of the target it was compiled for. A reader that finds a tag other than its own treats the
OS-specific fields of that value as uninterpretable (X1); OS-independent fields (for example a record's checksum) keep
their meaning, and `doctor` may still print a foreign `ProcId` for diagnostics ([F01 §3.2]).

## 3. `ProcId`

### 3.1 Layout (32 bytes, little-endian, packed, no padding)

| Offset | Width | Type | Name | Meaning |
|---|---|---|---|---|
| 0 | 1 | u8 | `os` | OS tag of the process (§2) |
| 1 | 1 | u8 | `flags` | bit 0 `start_known`; bit 1 `start_boot_relative`; bit 2 `boot_known`; bit 3 `pidns_known`; bits 4–7 reserved, zero |
| 2 | 2 | u16 | reserved | zero |
| 4 | 4 | u32 | `pid` | the OS process id |
| 8 | 8 | u64 | `start` | the process start time in nanoseconds (§3.2); 0 when `start_known` = 0 |
| 16 | 8 | u64 | `boot_hash` | `boot_hash` of the process's boot identity (§4.3); 0 when `boot_known` = 0 (Unknown-boot mode) |
| 24 | 8 | u64 | `pidns` | Linux: the inode number of `/proc/self/ns/pid`; 0 elsewhere and when `pidns_known` = 0 |

A value whose reserved bits or bytes are non-zero, or whose `os` is not 1–3, is **uninterpretable**: it is displayed as
`?` and `alive` answers `Unknown` for it (§6.1). Writers set every reserved bit and byte to zero.

### 3.2 Field values per OS

| Field | Windows (built) | Linux (port) | macOS (port) |
|---|---|---|---|
| `pid` | `GetCurrentProcessId()` | `getpid()` | `getpid()` |
| `start` | `GetProcessTimes` creation `FILETIME` `c`, converted to ns since the Unix epoch: `(c − 116 444 736 000 000 000) × 100` | `/proc/<pid>/stat` field 22 (`starttime`, clock ticks since boot), parsed after the last `)` of the line; converted to ns since boot as `⌊starttime × 10^9 / sysconf(_SC_CLK_TCK)⌋` (u128 intermediate) | `sysctl({CTL_KERN, KERN_PROC, KERN_PROC_PID, pid})` → `kinfo_proc.kp_proc.p_starttime` (`timeval`): `tv_sec × 10^9 + tv_usec × 10^3`, ns since the Unix epoch. libproc is not used (X9) |
| `start_boot_relative` | 0 | 1 | 0 |
| `boot_hash` | §4.3 | §4.3 | §4.3 |
| `pidns` | 0 (`pidns_known` = 0) | `stat("/proc/self/ns/pid").st_ino`; unreadable → 0 and `pidns_known` = 0 | 0 (`pidns_known` = 0) |

- `start` is compared for **equality only**, and only between two `ProcId`s of one OS tag; its unit is nanoseconds on
  every OS, its origin is the Unix epoch except where `start_boot_relative` = 1. A start time that cannot be read (a denied
  query of another process) leaves `start_known` = 0 and `start` = 0.
- The Windows conversion is exact (FILETIME counts 100 ns). A value outside the u64 range after conversion (a creation
  time before 1970) is recorded with `start_known` = 0.

### 3.3 The parent record

`SlotRec` carries a 16-byte parent record `{pid u32, _ u32, start u64}` ([80] X-F1; layout in [F03]). Its `pid` and
`start` are the parent's, with `start` in the unit of §3.2 for the writer's OS; the reserved u32 is zero. It is read by
`ProcHost::parent()` (§10).

### 3.4 What `ProcId` is and is not

- `ProcId` is **diagnostics only** ([80] X-F2, [AR §6.2]): it is printed by `doctor`, exit-7 lock diagnostics and `show
  --provenance`, and it is stored in `WriterDiag`, `SlotRec`, `LeaderRec` and the `FSINTENT` holder ([F03], [F11]).
- **No correctness decision reads it.** Liveness of leases and intents is lock-anchored (§6.2); PID reuse, PID namespaces
  and sandboxes therefore change nothing.

## 4. The boot identity

### 4.1 The rule (frozen, X-F2)

`boot_id` is 16 opaque bytes that are **constant for exactly one boot of the kernel** and **invariant under wall-clock
changes, suspend and hibernation** ([80 §2.7.1], [81] M2). A reboot changes it; nothing else may. The system boot time is
never a source: Windows moves it whenever the system time is set, so a clock step would read as a reboot and make every
non-run-scoped lease Dead under a live agent. A Windows Fast Startup "shutdown" is a hibernation of the kernel session and
keeps the boot identity (it is not a reboot; the page cache survives it), consistently with the rule.

### 4.2 Derivation per OS

Every OS derives the 16 bytes the same way, from one source per OS, with the hash-input framing of [F01 §6.3, §7.3]
(each operand length-prefixed by `lp()`, a fixed `lp("moirai-…-v1")` domain prefix):

```
boot_id = BLAKE3-128( lp("moirai-boot-id-v1") ‖ lp(N) ‖ lp(S1) [‖ lp(S2)] )
```

`N` names the source (ASCII); `S1` and, on Windows, `S2` are the source values, encoded as the table states. BLAKE3-128
is the first 16 bytes of BLAKE3-256 ([F01 §7.1]). Hashing on every OS makes the 16 bytes uniformly distributed and
**domain-separated by OS**: a store copied to another OS sees a boot change, which is the correct reading (another
kernel), and two OSes never produce equal values.

| OS | `N` | Operands | Absent, denied or malformed → |
|---|---|---|---|
| Windows | `win-bootid-machineguid` | `S1` = `u32le(B)`, where `B` is the per-boot counter: the `BootId` member of `KUSER_SHARED_DATA` (WDK `ntddk.h`), read from the fixed user-mode mapping at `0x7FFE0000` at the member's declared offset; if it reads 0, the `REG_DWORD` value `BootId` of `HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management\PrefetchParameters`. `S2` = `G`, the `REG_SZ` value `MachineGuid` of `HKLM\SOFTWARE\Microsoft\Cryptography` (64-bit view), converted from UTF-16 to UTF-8 bytes exactly as stored, without its terminating NUL and without case change | Unknown-boot mode; also when `B` is 0 from both sources, `G` is empty or not valid UTF-16, or HOLE(OS-win-boot-source) selects "none" |
| Linux | `linux-proc-boot_id` | `S1` = the bytes of `/proc/sys/kernel/random/boot_id` with the single trailing LF removed; they must be exactly 36 ASCII bytes of the form 8-4-4-4-12 lower-case hex digits and `-` | Unknown-boot mode |
| macOS | `darwin-kern-bootsessionuuid` | `S1` = the bytes of `sysctlbyname("kern.bootsessionuuid")` without the terminating NUL; they must be exactly 36 ASCII bytes of the form 8-4-4-4-12 hex digits (either case, kept as returned) and `-` | Unknown-boot mode (the sysctl is undocumented, X9; its absence or a Seatbelt denial only selects the mode) |

- `MachineGuid` keeps a small Windows counter from colliding with another machine's store copy ([80 §2.7.1]).
- The Windows source is subject to measurement 22 (§4.5). Until HOLE(OS-win-boot-source) is filled, the Windows
  implementation reads the source above; if the measurement rejects every candidate, the Windows build is fixed to
  Unknown-boot mode (`UnknownBoot::DisabledByBuild`), with no format change (`boot_id` is opaque).

### 4.3 `boot_hash`

```
boot_hash(boot_id) = u64::from_le_bytes(boot_id[0..8]) | 1
```

The integer is the first 8 bytes of the digest read little-endian, as [F01 §7.2] prescribes for an integer taken from a
digest; this file adds the forced low bit.

The low bit is forced to 1, so a known boot never hashes to 0, and 0 can mean "unknown" in every field that carries a
`boot_hash`: `ProcId.boot_hash` (§3.1), `Anchor.boot_hash` ([F03]) and the deadline form `{wall, boot_hash, mono}`
([OS/clock §4]). Two boots collide with probability ≈ 2^-63 per pair; a collision can only make a Dead anchor of the
earlier boot look boot-matching, after which the slot and record checks of §6.2 still decide, so it never keeps a dead
lease alive by itself.

### 4.4 Reading it

- A process reads its boot identity **once**, at its first need, and caches the result (`Known(BootId)` or
  `Unknown(reason)`) for its lifetime: a boot cannot change while a process lives, and a process that survives a
  hibernation keeps the same boot by §4.1.
- The read is cheap and never blocks: one memory read plus at most two registry reads on Windows; one file read on Linux;
  one `sysctl` on macOS.
- A read that fails for any reason (absent, denied by a sandbox or another principal, malformed) yields
  `Unknown(reason)`. It never refuses the store ([80] X6, [81] M3).

### 4.5 Verification (measurement 22)

Measurement 22 ([AR §8.2] item 22, [60 §5.2]) records, on the owner's Windows 11 laptop, the Windows `boot_id` of §4.2
(and each candidate source separately) across: a manual wall-clock step of ±1 h, a sleep (S3 or modern standby), a
hibernation, a Fast Startup shutdown and power-on, and a reboot. It also records the values read by a process running as
another principal (the Codex elevated-sandbox user where available, V9) to confirm readability. The acceptance rule is
§4.1: the reboot changes the value and nothing else does. The chosen source fills HOLE(OS-win-boot-source) at WP-81a
(E2 of PLAN §7).

## 5. Unknown-boot mode (frozen, X-F2)

A process whose boot identity is `Unknown` ([80 §2.7.1], [81] M3) keeps using the store under these rules. Each rule
names the chapter that enforces it.

| # | Rule | Enforced in |
|---|---|---|
| U1 | It never republishes `boot_id`: every publish it makes keeps the newest slot's `boot_id` unchanged | [F16] publish; [F04] |
| U2 | It never runs boot-change recovery | [F16] boot check |
| U3 | Every lease boot test it makes answers `Unknown` (never Dead); the lease's wall-clock deadline decides | §6.2 step B; [OS/clock §4.3] |
| U4 | It writes `boot_hash` = 0 in every `ProcId`, anchor and deadline it creates (`boot_known` = 0), so every later checker judges those values by their wall and slot components | §3.1; [F03]; [OS/clock §4.2] |
| U5 | Its readers stay correct through the validity rules: an invalid record in `(durable_lsn, committed_lsn]` ends the visible log, and the chain check at the reader's replay bound | [F05], [F16] |
| U6 | Its writers scan to the end of the log, so an acknowledged commit whose publish an OS crash lost is published by its next flush | [F16] phase 2a |

The only loss ([80 §2.7.1]): until an unsandboxed process or any writer runs, such a commit stays invisible to that
process's readers. A process never switches mode during its life (§4.4).

## 6. Liveness

Liveness is three-valued everywhere:

```rust
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Liveness { Alive, Dead, Unknown }
```

`Unknown` never ends a lease, never recovers an intent and never reclaims anything ([AR §6.2], [80 §2.7.2]).

### 6.1 `alive(ProcId)` — diagnostics only

`ProcHost::alive(&ProcId)` is used only for diagnostics (`doctor`, exit-7 holder lines). It is evaluated in this order;
the first matching row answers:

| # | Condition | Answer |
|---|---|---|
| 1 | the value is uninterpretable (§3.1), or `os` differs from the checker's own tag | `Unknown` |
| 2 | both `boot_known`, and `boot_hash` differs from the checker's | `Unknown` ([80 §2.7.1] Linux row; applied on every OS, open point 4) |
| 3 | Linux: both `pidns_known`, and `pidns` differs from the checker's | `Unknown` |
| 4 | the per-OS lookup is denied (Windows `ERROR_ACCESS_DENIED`, Linux `EACCES` on `/proc/<pid>/stat`, macOS `EPERM`) | `Unknown` |
| 5 | the per-OS lookup reports that no such process exists (Windows `OpenProcess` → `ERROR_INVALID_PARAMETER`; Linux `ENOENT`; macOS `ESRCH` or a zero-length result) | `Dead` |
| 6 | the process has exited but its object remains (Windows `WaitForSingleObject(h, 0) = WAIT_OBJECT_0`; Linux state `Z` or `X`; macOS `p_stat = SZOMB`) | `Dead` |
| 7 | `start_known` on both sides and the looked-up start differs from `start` | `Dead` |
| 8 | otherwise | `Alive` |

The Windows lookup opens the process with `PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE` and closes it before
returning. Per-OS calls are in [OS/mapping-appendix].

### 6.2 Lock-anchored liveness: the decision procedure (frozen with X-F2)

The byte layouts are [F03]'s (`SlotRec`, `Anchor`, `LeaderRec`, the lock-byte map); this section is the normative decision
procedure over them ([80 §2.7.2], [90 §4.4]). Its inputs are an anchor `a`, the checker's boot identity, the checker's
current stamp ([OS/clock §3]) and, for kind 4, the lease's deadline. It reads `LOCK` with one `pread` of the 32 KiB slot
table and probes slot bytes with `Locks::probe` ([OS/lock]), whose result is `Held`, `Free` or `Unknown`.

**Step A — kind.**
- Kind 0 (`none`): the anchor contributes nothing; the lease lives by its TTL or run scope alone ([AR §6.2]). The
  procedure returns `NotAnchored`, which the lease layer treats as neither Alive nor Dead.
- A kind value other than 0–4 is uninterpretable: `Unknown`.

**Step B — boot.** If the checker's boot is `Known(b)` and `a.boot_hash ≠ 0` and `a.boot_hash ≠ boot_hash(b)`, return
`Dead` (the boot differs). Otherwise (the checker is in Unknown-boot mode, or the anchor was written in it) the boot test
is skipped ([80] X-F2: "the boot matches, or is unknown to the checker").

**Step C — `LOCK` readable.** If `LOCK` cannot be opened or read (`ERROR_ACCESS_DENIED` for another principal, `EPERM`
from a sandbox), return `Unknown`.

**Step D — per kind.**

| Kind | Procedure | Answer |
|---|---|---|
| 1 `session` | For every slot record `r` (i = 0…255, in any order) with a valid checksum, `r.kind = 1` and `r.session_hash = a.id` (the **primary** hash; the alias hash is never matched for an anchor): probe slot byte i. `Held` → re-read `r` (seqlock rule of [80] X-F1); if still valid and still matching, the answer is `Alive` and the scan stops. `Unknown` → remember it. `Free` → continue | `Alive` if some matching slot is held; else `Unknown` if some probe of a matching slot answered `Unknown`; else `Dead` |
| 4 `session-ttl` | as kind 1; then, if the result is `Alive`, the lease's deadline is evaluated by [OS/clock §4.3]: `Passed` or `BootChanged` → `Dead` | as kind 1, with the deadline check |
| 2 `intent` | `i = a.slot`; if `i ≥ 256` → `Dead`. Read record `r` at slot i: invalid checksum, `r.kind ≠ 2` or `r.nonce ≠ a.nonce` → `Dead`. Probe byte i: `Held` → re-read `r`; if it still carries `a.nonce` → `Alive`, else `Dead`. `Free` → `Dead`. `Unknown` → `Unknown` | as stated |
| 3 `leader` | Probe the leader byte (2^62 + 1). `Held` → read `LeaderRec`; valid and `nonce = a.nonce` → `Alive`, else `Dead`. `Free` → `Dead`. `Unknown` → `Unknown` | as stated |

- **Safe direction of every race** ([80 §2.7.2] "Race windows"). A server writes its record right after taking its slot,
  before it serves anything; a CLI that looks in that window finds no anchor, and its lease gets kind `none`. A slot whose
  record still names a previous owner makes that owner look `Alive` for microseconds. A dead holder's slot reads `Held`
  for the OS release lag (≤ 32 ms observed on Windows after `TerminateProcess`, unbounded under a crash reporter,
  fault-model item (8)). Each of these errs towards `Alive` or `Unknown`, never towards a false `Dead`.
- **Cost.** One 32 KiB `pread`; one probe per matching slot (2–9 µs on Windows [M, X18 §5]).

### 6.3 The identity a slot and an anchor carry

The primary session hash is BLAKE3-128 of the UTF-8 bytes of the namespaced process-lifetime identity `<harness>:<id>`:
`claude:` + `CLAUDE_CODE_SESSION_ID` for Claude Code's per-session server, `codex:` + the thread id for Codex's per-thread
servers ([90 §4.4], [80 §8.7]). The derivation and the 16-byte fields are [F03]'s; this file only consumes them. The
identity comes from the caller-context resolver's session row ([90 §4.1]); a process with no session identity takes no
slot, and its leases get anchor `none`.

### 6.4 Slot selection

A server tries slots starting at `hash(session) mod 256`, an intent starting at `hash(nonce) mod 256`, in increasing
order modulo 256, taking the first free one by `try_acquire` ([80 §2.7.2]). `hash(session)` is the u64 read
little-endian from the first 8 bytes of the 16-byte session hash; `hash(nonce)` is the u64 nonce itself. With every slot
busy, a lease gets anchor `none` and `file mv` exits 7 "no liveness slot free".

## 7. The parent watch (MCP server lifetime)

The MCP server ends when its parent process ends, even if stdin EOF never arrives, so that its liveness slot mirrors the
session ([80 §2.7.2], [81] m13). The watch is event-driven, with no timer and no polling.

**Contract.**
1. `watch_parent()` identifies the parent once, at server start, and returns a `ParentWatch`. If the parent has already
   exited (Windows: the parent's creation time is not earlier than the server's own; Linux and macOS: `getppid()` no longer
   returns the recorded pid after the watch is registered), the watch is created already fired.
2. `wait_parent_or_wake(&watch, &wake)` blocks until the parent exits (`WatchEvent::ParentExited`) or another thread of the
   server signals `wake` (`WatchEvent::Woken`). The stdin reader thread signals `wake` when a request is ready or stdin
   reaches EOF; the server has at most these two threads ([AR §6.1]).
3. On `ParentExited` the server releases its slot byte (by exiting; the OS releases the lock) and exits with code 0.

| OS | Mechanism |
|---|---|
| Windows | parent pid from a `CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS)` walk (the own entry's `th32ParentProcessID`); `OpenProcess(SYNCHRONIZE \| PROCESS_QUERY_LIMITED_INFORMATION)`; `GetProcessTimes` on it — a parent created after the server is a reused pid, so the watch is fired; `WaitForMultipleObjects({parent, wake event}, FALSE, INFINITE)` |
| Linux (port) | `pid = getppid()`; `pidfd_open(pid, 0)` (Linux ≥ 5.3); then `getppid() = pid` must still hold, else fired; `poll({pidfd, POLLIN}, {eventfd, POLLIN})`. `PR_SET_PDEATHSIG` is never used: it fires when the spawning *thread* exits ([81] m13) |
| macOS (port) | `kqueue()`; `EVFILT_PROC` with `NOTE_EXIT` on `getppid()` (registration failing with `ESRCH` → fired); then `getppid()` must be unchanged (a re-parent to `launchd` means the parent died); `EVFILT_USER` as the wake; `kevent` blocks |

The wake object is a Windows auto-reset event (`CreateEventW`), a Linux `eventfd(0, EFD_CLOEXEC)`, or a macOS
`EVFILT_USER` registration triggered with `NOTE_TRIGGER`.

## 8. The parent image name

`parent_image()` returns the lower-case base name of the parent process's executable without a trailing `.exe`
(`powershell`, `pwsh`, `bash`, `cmd`, `claude`, …), or `None` if it cannot be read. It is diagnostics-grade: PID reuse
between the lookup and the read can make it wrong, and nothing but a warning depends on it. Its one consumer is the
PowerShell `?` warning of [OS/shell §5] (rule T6). Sources: Windows — the parent's `PROCESSENTRY32W.szExeFile` from the
snapshot of §7; Linux — `/proc/<ppid>/comm`; macOS — `kinfo_proc.kp_proc.p_comm` from `sysctl(KERN_PROC_PID)`.


## 9. `peak_of_child`

`peak_of_child` is the OS reading behind `Meter::peak_of_child` ([OS/README §4.3], [OS/mem §4]). It lives in `os::proc`
because it needs the child's process handle.

**Contract** ([OS/README §4.3]): before the spawn the caller calls `Meter::prepare_child(&mut command)`, which returns a
`ChildTicket`; after `Command::spawn` it calls `bind_child(ticket, &child)`; after `Child::wait` has returned, and before
`child` is dropped, `peak_of_child(&child)` takes the reading from the still-open process handle. The caller spawns; the
`Meter` never does (GT20 (a)).

| OS | `prepare_child` / `bind_child` | Reading | `PeakKind` |
|---|---|---|---|
| Windows | nothing to arrange: `ChildTicket(0)`; `bind_child` records nothing | `GetProcessMemoryInfo(child handle, PROCESS_MEMORY_COUNTERS_EX)` → `PeakPagefileUsage` (peak private commit), read after `WaitForSingleObject(child handle, INFINITE)`; the process object stays queryable while the handle is open | `Peak` |
| Linux (port) | create a leaf cgroup under the harness's delegated cgroup and make the child join it before `exec`; `bind_child` maps the child's pid to it | cgroup-v2 `memory.peak` of that leaf, which is then removed; with no delegated cgroup: `MeterError`, never a guess ([OS/mem] open point 2) | `Peak` |
| macOS (port) | nothing to arrange | `proc_pid_rusage(pid, RUSAGE_INFO_V6).ri_lifetime_max_phys_footprint` (libproc: measure harness only, X9); it must be read before the child is reaped, which `Child::wait` does, so the port reads it in a non-reaping wait (`waitid(…, WNOWAIT)`) inside `bind_child`'s bookkeeping or reports `MeterError` (open point 6) | `Peak` |

`ru_maxrss` and `/proc/*/status` RSS are never the reading ([80 §2.9]). Measurement 11 records the Windows reading beside
VMMap for each process kind ([60 §5.1] RSS row).

## 10. The Rust surface

```rust
/// OS tag byte (§2, [F01 §3.2]).
#[repr(u8)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum OsTag { Unspecified = 0, Windows = 1, Linux = 2, MacOs = 3 }

/// 32-byte diagnostic process identity (§3). Plain data; every field is public.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct ProcId { pub os: u8, pub flags: u8, pub pid: u32, pub start: u64, pub boot_hash: u64, pub pidns: u64 }

impl ProcId {
    pub const LEN: usize = 32;
    pub const START_KNOWN: u8 = 1 << 0;
    pub const START_BOOT_RELATIVE: u8 = 1 << 1;
    pub const BOOT_KNOWN: u8 = 1 << 2;
    pub const PIDNS_KNOWN: u8 = 1 << 3;
    /// Encodes §3.1 exactly; reserved bits and bytes are written as zero.
    pub fn to_bytes(&self) -> [u8; 32];
    /// Decodes §3.1; `None` if the value is uninterpretable (reserved bits set, `os` not 1–3).
    pub fn from_bytes(b: &[u8; 32]) -> Option<ProcId>;
}

/// 16 opaque bytes (§4.2).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct BootId(pub [u8; 16]);

impl BootId {
    /// §4.3: never 0.
    pub fn hash(&self) -> u64;
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum BootIdentity { Known(BootId), Unknown(UnknownBoot) }

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum UnknownBoot { SourceAbsent, Denied, Malformed, DisabledByBuild }

/// The parent record of §3.3.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct ParentRec { pub pid: u32, pub start: u64, pub start_known: bool }

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum WatchEvent { ParentExited, Woken }

/// Signals a waiting `wait_parent_or_wake` from another thread.
pub trait Wake: Send + Sync { fn signal(&self); }

/// The process seam ([80 §2.1] `os::proc`, `os::spawn`); a supertrait of `Vfs` ([OS/README §4.1]).
/// Implemented by `moirai_os::OsVfs` and by the simulator's `SimVfs`.
pub trait ProcHost {
    type ParentWatch: Send;
    type Wake: Wake;
    /// The OS tag of this build (§2).
    fn os_tag(&self) -> OsTag;
    /// This process's `ProcId` (§3); never fails — unreadable fields are flagged unknown.
    fn self_id(&self) -> ProcId;
    /// The parent record (§3.3).
    fn parent(&self) -> Result<ParentRec, VfsError>;
    /// The boot identity, read once per process and cached (§4.4).
    fn boot_identity(&self) -> BootIdentity;
    /// Diagnostics only (§6.1).
    fn alive(&self, p: &ProcId) -> Liveness;
    /// §7.
    fn watch_parent(&self) -> Result<Self::ParentWatch, VfsError>;
    fn new_wake(&self) -> Result<Self::Wake, VfsError>;
    fn wait_parent_or_wake(&self, w: &Self::ParentWatch, wake: &Self::Wake) -> Result<WatchEvent, VfsError>;
    /// §8.
    fn parent_image(&self) -> Option<Box<str>>;
    /// §11: spawn the detached, low-priority `moirai gc` child; returns its pid. The only spawn in product code.
    fn spawn_gc_child(&self, exe: &std::path::Path, args: &[&str], cwd: &std::path::Path) -> Result<u32, VfsError>;
    /// §11: lower this process's own CPU, memory and I/O priority (the `gc` child calls it first).
    fn enter_background(&self);
}
```

- `VfsError` is [OS/fs §6.1]'s error type ([OS/README §4.5]).
- `peak_of_child` is not a `ProcHost` method: it is reached through `Meter` ([OS/README §4.3]), which `moirai-os`
  implements by calling `os::proc::peak_of_child`.
- The lock-anchored procedure of §6.2 is a pure function over `LOCK` bytes, probe results and the two stamps; it lives
  with the `LOCK` decoder (`moirai-store` from M1; the toy log and the model each have their own), not in `moirai-os`.

### 10.1 The simulator's obligations

`moirai-vfs-sim` implements `ProcHost` per simulated process: each simulated process has its own `ProcId`, its own
`BootIdentity` (a simulated boot is a 16-byte value the test chooses; a simulated reboot changes it; any simulated process
can be put in Unknown-boot mode, and any single read may be made to answer `Unknown`, [F15] FM-7.4), and `alive` reads
the simulated process table. The simulated parent watch fires when the test ends the simulated parent. Liveness in the
simulator uses the real procedure of §6.2 over the simulated `LOCK`. `spawn_gc_child` creates a simulated process.

## 11. `os::spawn`: the detached `gc` child

[80 §2.12] and [AR §4.9]: a CLI that finds a rollup due spawns `moirai gc --rollup` as a detached, low-priority child and
exits; the child runs the rollup and exits. It is the **only** process a product crate spawns ([60 §3.5] M4 gate: "no
other process spawn except the detached `moirai gc` child"); the GT20 (a) spawn lint names `moirai-os`'s `spawn`
module as its one allowed site. Not built at M0 ([OS/README §3]).

**Contract of `spawn_gc_child(exe, args, cwd)`.**
1. **No role byte held.** It asserts `Locks::holds_any_role() == false` ([OS/README §5.2], [80 §2.2.1] item 7); a
   violation is a programming error (panic).
2. **Never from a foreign PID namespace** ([80 §2.6]). The caller does not call it when `parent().pid = 0` or when its own
   `self_id().pidns` differs from the `pidns` of the session's MCP server's slot record ([F03]); then the MCP server or an
   explicit `moirai gc` runs the rollup. (Both tests are only meaningful on Linux; elsewhere `pidns` is 0.)
3. **Detached.** The child gets no inheritable handle, its standard streams are the null device, it outlives the parent's
   exit and the closing of the parent's console or terminal, and nothing waits for it.
4. **Low priority.** It is created at lowered priority where the OS allows, and its first action is `enter_background()`.

| OS | `spawn_gc_child` | `enter_background` (in the child) |
|---|---|---|
| Windows | `CreateProcessW(exe, command line built with the Microsoft C runtime's quoting rules, NULL, NULL, FALSE, BELOW_NORMAL_PRIORITY_CLASS \| DETACHED_PROCESS \| CREATE_NEW_PROCESS_GROUP \| CREATE_BREAKAWAY_FROM_JOB, …, cwd)`; standard handles not set; if breakaway is refused (`ERROR_ACCESS_DENIED` from a job that forbids it), the same call without `CREATE_BREAKAWAY_FROM_JOB` (open point 10); the process and thread handles are closed at once | `SetPriorityClass(GetCurrentProcess(), PROCESS_MODE_BACKGROUND_BEGIN)` (background CPU, I/O and memory priority), then `SetProcessInformation(ProcessMemoryPriority, MEMORY_PRIORITY_LOW)` |
| Linux (port) | `posix_spawn` with `POSIX_SPAWN_SETSID`, the three standard streams opened on `/dev/null`, every descriptor `O_CLOEXEC` | `setpriority(PRIO_PROCESS, 0, 10)`; `ioprio_set(IOPRIO_WHO_PROCESS, 0, IOPRIO_PRIO_VALUE(IOPRIO_CLASS_IDLE, 0))` |
| macOS (port) | `posix_spawn` with `POSIX_SPAWN_SETSID` and the streams on `/dev/null` | `setpriority(PRIO_DARWIN_PROCESS, 0, PRIO_DARWIN_BG)`; `setiopolicy_np(IOPOL_TYPE_DISK, IOPOL_SCOPE_PROCESS, IOPOL_THROTTLE)` |

Bulk passes in the **current** process (rollup, backup, full export, retirement, repair, `links check --all`) use the
sequential open hint and `advise_dontneed` of [OS/fs §4.12]; on Windows they also lower their own memory priority with
`SetProcessInformation(ProcessMemoryPriority, MEMORY_PRIORITY_LOW)` through `enter_background` ([80 §2.12] "Bulk
passes").

## 12. `os::ipc`: the leader's endpoint (built only if the leader is)

[80 §2.8]. The endpoint exists only if the M0 measurements 1 and 2 put the optional leader into M1 ([60 §3.1] "Decisions
fixed at M0 exit"). Its contract is frozen now so that a later leader changes no byte; the `LeaderRec` record at `LOCK`
offset 3072 is [F03]'s.

**Surface** (a sub-trait `LeaderIpc` of `Vfs`, added only if the leader is built): `bind_endpoint(store) → Listener`
(taken while holding the leader byte), `accept(&Listener) → Conn`, `connect(&LeaderRec) → Conn`, `peer(&Conn) →
PeerCred { pid, same_user }`.

**Rendezvous.** The leader, holding the leader byte (2^62 + 1), removes a stale socket, binds, and writes `LeaderRec {seq,
proc ProcId, proto, endpoint_kind (1 pipe | 2 Unix socket), endpoint ≤ 200 B, nonce u128, xxh3}`. A client reads
`LeaderRec`, probes the leader byte, connects, checks the peer, and requires the nonce echoed back. A sandboxed client
that cannot connect uses the direct path, which is complete ([80 §2.8]).

| | Windows | Linux (port) | macOS (port) |
|---|---|---|---|
| Endpoint | `\\.\pipe\moirai-<u>-<s>` | `$XDG_RUNTIME_DIR/moirai/<s>.s` when `XDG_RUNTIME_DIR` is set, owned by the user and mode 0700; else `/tmp/moirai-<uid>/<s>.s` | `<confstr(_CS_DARWIN_USER_TEMP_DIR)>/moirai/<s>.s` |
| Name parts | `<u>` = 16 lower-case hex digits of the first 8 bytes of BLAKE3-128(`lp("moirai-ipc-user-v1") ‖ lp(user SID in "S-1-…" form)`); `<s>` as on Unix | `<s>` = 16 lower-case hex digits of the first 8 bytes of BLAKE3-128(`lp("moirai-ipc-store-v1") ‖ lp(canonical store path, [OS/path §5])`) | as Linux |
| Creation | `CreateNamedPipeW` with `FILE_FLAG_FIRST_PIPE_INSTANCE`, `PIPE_REJECT_REMOTE_CLIENTS`, a DACL granting only the user's SID | the directory created mode 0700 and checked with `lstat` (not a link; uid = euid; mode 0700); `socket(AF_UNIX, SOCK_STREAM \| SOCK_CLOEXEC)`, `bind`, `listen` | as Linux |
| Peer check | `GetNamedPipeClientProcessId`, `GetNamedPipeServerProcessId` | `getsockopt(SO_PEERCRED)` | `getpeereid`, `getsockopt(LOCAL_PEERPID)` |
| Never | — | an abstract socket; a location derived from `$TMPDIR`; a socket inside `.git/moirai` | same |

The path is ≈ 45–80 bytes, under the 104-byte `sun_path` limit of macOS and the 108 bytes of Linux ([80 §2.8]). moirai
never asks the owner to set `allowUnixSockets` or `allowAllUnixSockets`.

## 13. `os::test_host` (cargo feature `test-host`)

Test and probe builds only: the composition-root lint refuses the feature in `moirai` ([OS/README §2.4]). [80] X9's
public-interface rule binds product code; these functions may use a test-only interface where the harness needs one.

```rust
#[cfg(feature = "test-host")]
pub mod test_host {
    /// Ends a child at once (GT4's `TerminateProcess` variant).
    pub fn kill(child: &mut std::process::Child) -> std::io::Result<()>;
    /// Suspends and resumes every thread of a process (GT4's 1–120 s pauses).
    pub fn suspend(pid: u32) -> std::io::Result<()>;
    pub fn resume(pid: u32) -> std::io::Result<()>;
    /// A small, separately mounted volume for disk-full tests; owner-run on Windows (elevation).
    pub fn small_volume(bytes: u64) -> std::io::Result<SmallVolume>;
    /// Shifts this process's wall clock ([OS/clock §9]).
    pub fn set_wall_offset_ms(offset_ms: i64);
    /// The environment variable a harness sets on a child to shift its wall clock: decimal i64, optional leading '-'.
    pub const WALL_OFFSET_ENV: &str = "MOIRAI_TEST_WALL_OFFSET_MS";
}
```

| Function | Windows (built at M0) | Linux (port) | macOS (port) |
|---|---|---|---|
| `kill` | `TerminateProcess(child, 1)` | `kill(pid, SIGKILL)` | same |
| `suspend`, `resume` | `NtSuspendProcess`, `NtResumeProcess` (ntdll; test-only) | `kill(pid, SIGSTOP)`, `kill(pid, SIGCONT)` | same |
| `small_volume` | `CreateVirtualDisk` + `AttachVirtualDisk` of a VHDX (needs elevation: owner-run only; profile L's nightly never calls it, [AR §8.2]) | a loop-mounted ext4, XFS or btrfs image | `hdiutil create` + `attach` of an APFS image |
| wall offset | read once at process start from `MOIRAI_TEST_WALL_OFFSET_MS`; `set_wall_offset_ms` overrides it for the current process | same | same |

## Holes

| Id | What | Decided by | Candidates | Constraint the value must meet |
|---|---|---|---|---|
| HOLE(OS-win-boot-source) | The Windows source of the counter `B` in §4.2 | measurement 22 (WP-52) | (a) `KUSER_SHARED_DATA.BootId`, falling back to the `PrefetchParameters\BootId` registry value when it reads 0 (the design's choice); (b) the registry value only; (c) none: the Windows build runs in Unknown-boot mode | the derived `boot_id` is identical across a ±1 h manual clock step, a sleep, a hibernation and a Fast Startup cycle, differs across a reboot, is identical in every process of one boot, and is readable by the sandbox principals measurement 22 can create (§4.5); if no candidate passes, (c) |

## Open points for the review

| # | Point | Resolution in this file | For |
|---|---|---|---|
| 1 | [80 §2.1] calls `os::proc` "injected"; where does the process seam sit? | resolved with [OS/README §4.1]: `ProcHost` is a supertrait of `Vfs`, so every store process reaches the boot identity through `V: Vfs` | — |
| 2 | The OS-tag values were needed by [F03] and [F11] | fixed here first; [F01 §3.2] restates them and proposes itself as the owner; this file now cites [F01 §3.2] (§2) | WP-10 |
| 3 | [80 §2.7.1] names the sources of `boot_id` but not the bytes, and `boot_hash` ("its hash in anchors") had no definition | `boot_id` = BLAKE3-128 over `lp()`-framed operands with a `moirai-boot-id-v1` domain prefix on every OS ([F01 §6.3, §7.3]); `boot_hash` = the first 8 bytes read LE ([F01 §7.2]) with bit 0 forced to 1, so 0 means unknown (§4.3). Linux and macOS UUIDs are hashed rather than stored raw, which separates the OSes | R-REV-P, WP-11 |
| 4 | [80 §2.7.1] answers `Unknown` for a different boot only in its Linux row; [X18 §8.1] (research) proposed `Dead` | `Unknown` on every OS (§6.1 row 2): `alive` is diagnostics only, and the conservative answer costs nothing | R-REV-P |
| 5 | `ProcId.start` units: [80 §2.7.1] names the Windows and macOS sources but no unit | nanoseconds on every OS, since the Unix epoch on Windows and macOS and since boot on Linux, the latter flagged by `start_boot_relative` (§3.2); [80 §3.2]'s "FILETIME is converted in `os::proc`" is followed | WP-11 |
| 6 | Child peaks on Unix need work before the spawn (Linux) or before the reap (macOS) | [OS/README §4.3] adopted `prepare_child`/`bind_child`; Windows needs neither; the Linux leaf cgroup and the macOS non-reaping read are port-phase items (§9) | port phase |
| 7 | [90 §4.4] and [80 §2.7.2] give slot selection as `hash(x) mod 256` without defining `hash` or the probe order | §6.4 fixes both; [OS/lock §10] cites it; [F03] may restate it | WP-11 |
| 8 | The Windows `KUSER_SHARED_DATA.BootId` offset is taken from the WDK declaration, not hard-coded in this spec | §4.2; measurement 22 confirms the value behaves; WP-33 takes the offset from the `windows-sys`/WDK binding | WP-33 |
| 9 | [OS/README §1.3, §3] list `os::spawn`, `os::ipc` and `os::test_host` as part 2 "not yet named", and WP-17b's brief names no file for them | specified here (§11–§13); README §1.3 and §3 point to this file | WP-17a |
| 10 | Whether the `gc` child survives a harness that runs commands inside a Windows job object with kill-on-close is not stated in [80] | request `CREATE_BREAKAWAY_FROM_JOB`, fall back without it; a child killed with the job only cuts a rollup short, which is safe (maintenance is crash-safe, [F16]); measurement 7's hook probes and M8 record whether Claude Code and Codex jobs allow breakaway | WP-56, M8 |
| 11 | [F01] open point 15 proposes the prefix `OS-` for hole ids in `docs/spec/os/` | adopted: HOLE(OS-win-boot-source) | WP-10 |
| 12 | `os::ipc`'s name parts `<u>` and `<s>` are not defined byte-exactly in [80 §2.8] | §12 fixes them with `lp()`-framed BLAKE3-128; they are published in `LeaderRec`, so clients read rather than recompute them | WP-11 |
