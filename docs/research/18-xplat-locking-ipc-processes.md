# 18 — Cross-platform multi-process locking, IPC and process liveness (Windows, Linux, macOS)

Status: research report, 2026-09-26. Lens: multi-process locking, IPC and process liveness.
Trigger: owner requirement of 2026-09-26 — macOS and Linux are first-class alongside Windows.
Scope: what the current design ([AR §4.1, §4.5, §4.10, §6.1–6.2, §7], [40 §2.6, §3.4, §4.7], [60 §5.2, GT4],
[05 §6.3], [08 §3–§5, §9]) does with Windows-only mechanisms in this lens, the Linux and macOS
equivalents with their hazards, and one platform abstraction that keeps **one on-disk format and one
protocol semantics** on all three OSes.

Evidence tags: **[M]** measured here (Windows 11 26200 only; see §5), **[D]** vendor/maintainer
documentation, **[S]** source code read for this report, **[C]** third-party claim, **[I]** inference.

Environment limits, stated up front:
- **No Linux and no macOS machine was available.** `wsl --status` reports "The Windows Subsystem for Linux
  is not installed" [M]; there is no Docker. Every Linux/macOS fact below is [D] or [S], never [M].
- Two probes ran on the owner's Windows machine (loaded: 16 `claude` processes, CPU ≈ 85 % [M]):
  `winprobe` (Windows lock semantics and release delay) and `xlock` (a ~250-line sketch of the proposed
  abstraction, **tested on Windows and compile-checked for `x86_64-unknown-linux-gnu` and
  `aarch64-apple-darwin`**) (probe scripts are not published).
- No repository was modified.

---

## 0. Summary

1. **macOS has open-file-description (OFD) locks.** The research brief assumed it does not. XNU has
   implemented `F_OFD_SETLK` (90), `F_OFD_SETLKW` (91), `F_OFD_GETLK` (92) and `F_OFD_SETLKWTIMEOUT` (93)
   since xnu-4570 (macOS 10.13, 2017) [S].
   - Up to xnu-8792 (macOS 13) they sat under `#ifdef PRIVATE`.
   - From xnu-10002 (macOS 14 Sonoma, 2023) they are in the public header under
     `__DARWIN_C_LEVEL >= __DARWIN_C_FULL` [S], and `fcntl(2)` documents them [S].
   - The `libc` crate (0.2.189) exports the first three for Apple targets [S].
   - redb master already takes `F_OFD_SETLK` on `target_vendor = "apple"` and on Linux, and
     `LockFileEx` on Windows, for its multi-process protocol [S].
   - Consequence: **one lock model, per-open-file byte-range locks, exists natively on all three
     OSes.** No flock-per-role files and no fcntl fd-hygiene are needed.
2. **Windows `LockFileEx` has the OFD ownership model, with two differences.** Measured here [M]: a
   second handle in the same process conflicts, and closing an unrelated handle does not drop the lock.
   The two differences are:
   - **Re-entrancy.** Re-locking a held byte through the same handle fails on Windows but silently
     succeeds (merges) under OFD.
   - **Release lag.** Windows releases a killed holder's lock after 1–8 ms p99, max 32 ms [M].
     Unix releases it when the process exits and its last descriptor closes [D].

   The abstraction removes the first difference by construction: one owner handle per grant, and no
   re-entrant acquire. The simulator already models the second.
3. **No Unix platform has a clean timed blocking lock.**
   - Linux has none [D].
   - macOS `F_OFD_SETLKWTIMEOUT` exists, but its timeout is **per sleep inside the kernel's retry loop**,
     and a zero timeout means "forever" [S]. Under contention the total wait can exceed the bound.
   - Recommendation for both Unixes: a waiter thread blocked in `F_OFD_SETLKW` on its own open file
     description, with a mutex hand-off. A grant that races the deadline is used or dropped; dropping
     closes the description, which releases the lock.
   - This matches Windows' `LockFileEx` + `WaitForSingleObject` + `CancelIoEx` + `GetOverlappedResult`
     semantics exactly.
4. **Claude Code's sandboxes break PID-based liveness and Unix-socket IPC for agent Bash calls.**
   - **Linux** (bubblewrap): every sandboxed command runs with `--unshare-pid`, a fresh `/proc`,
     `--unshare-net`, `--new-session --die-with-parent`, and a seccomp filter that makes
     `socket(AF_UNIX, …)` fail with `EPERM` [S].
     - A `(pid, start time)` recorded inside the sandbox names a process in a private namespace. PIDs
       like 2 and 3 repeat in every sandboxed command.
     - The Claude process is invisible from inside (`getppid()` returns 0 [D]).
   - **macOS** (Seatbelt): `process-info*` and `signal` are allowed only for the same sandbox, and Unix
     sockets are denied unless listed in `allowUnixSockets` [S][D].
   - **MCP servers and hooks are not sandboxed** [D].
   - [AR §6.2] expires leases whose `(pid, start time)` is dead, and [40 §3.4] recovers file intents
     whose pid is dead. Both are **unsound** when the recording process is a sandboxed CLI on Linux.
5. **Recommendation: lock-anchored liveness.**
   - Liveness becomes "is slot byte *i* of `LOCK` still held, and does slot record *i* still carry my
     nonce".
   - This works across PID namespaces, sandboxes and PID reuse, with identical semantics on the three
     OSes.
   - `(pid, start, boot id, pid namespace)` stays only as diagnostics and as a fallback.
6. **IPC.**
   - Named pipes stay on Windows; Unix uses pathname Unix-domain sockets in a per-user 0700 runtime
     directory.
   - Abstract sockets are Linux-only, have no permissions and are scoped to a network namespace, so they
     are rejected.
   - The endpoint is published in the `LOCK` leader record and bound only by the leader-byte holder.
   - Peers are verified with `SO_PEERCRED` / `getpeereid` + `LOCAL_PEERPID` / pipe DACL +
     `GetNamedPipe*ProcessId`.
   - Sandboxed Unix CLIs cannot reach the leader by default, so the direct path must stay first-class.
     The design already allows for this; the leader is never needed for correctness.
7. **New cross-kernel hazard: a store must be touched by one kernel only.**
   - Refuse stores on 9p/drvfs (WSL2 `/mnt/c`, `/mnt/d`), virtiofs/FUSE (Docker Desktop, Lima), NFS
     and SMB, and on Windows the `\\wsl$` / `\\wsl.localhost` UNC paths.
   - Byte-range locks are local to one kernel, so a Windows process and a WSL2 process on the same
     `.git/moirai` would not exclude each other [I]. Claude Code's docs themselves tell Windows users to
     run sandboxed sessions inside WSL2 [D].

---

## 1. Inventory: which mechanisms in the current design are OS-specific (this lens)

| # | Mechanism in the design | Where | Linux equivalent | macOS equivalent | Parity |
|---|---|---|---|---|---|
| 1 | Exclusive byte locks on `LOCK`: byte 0 writer, 1 leader, 2 maintenance, 3 quiet-advisory; diagnostics `{pid, start_ms, command}` at offset 2048 | [AR §4.1], [60 §2.5 table] | `fcntl(F_OFD_SETLK)` single-byte `F_WRLCK` | same (`F_OFD_SETLK` = 90, public since macOS 14) | **exact**, given the rules of §6.1 |
| 2 | Blocking wait: overlapped `LockFileEx` + `WaitForSingleObject(2 s)` + `CancelIoEx`, exit 7 naming the holder (G1) | [AR §4.5 step 1, §6.1] | no timed wait; `F_OFD_SETLKW` in a waiter thread + deadline | same; `F_OFD_SETLKWTIMEOUT` exists but bounds each sleep, not the total (§3) | exact through the abstraction |
| 3 | "Lock release after a crash may lag" (W2), 2 s bound, lock-delay injection in GT3/GT4 | [AR §4.10], [60 item 12] | release at process exit, when the last reference to the description closes | same | Unix is *better*; the simulator keeps the delay as a Windows-only parameter |
| 4 | Rust `File::lock` locks the whole range on Windows, so a dedicated `LOCK` file is used | [05 §6.3] | `File::lock` = `flock(2)` [S], which does **not** interact with fcntl/OFD locks on Linux [D] | `flock` **does** interact with OFD/POSIX locks (one lock list per vnode) [S] | divergent; rule: never `flock`/`File::lock` store files |
| 5 | redb-style 2^62 lock bytes and per-transaction reader bytes | [08 §3.3, §9 A] (superseded: [AR] readers take no locks) | OFD `F_RDLCK`/`F_WRLCK` on the same offsets (redb does exactly this) | same | exact; mapping in §6.4 |
| 6 | Leader: named pipe with per-user DACL, `FILE_FLAG_FIRST_PIPE_INSTANCE`, `PIPE_REJECT_REMOTE_CLIENTS` | [AR §6.1], [08 W8] | pathname UDS in a 0700 dir; `SO_PEERCRED` | pathname UDS (sun_path 104); `getpeereid`/`LOCAL_PEERPID` | semantic parity through §7.3 |
| 7 | Lease liveness `(pid, pid_start)` at read time; `FsIntent{…, pid, pid_start}` recovery | [AR §6.2, §3.4 lease row], [40 §2.6, §3.4 step 5] | `/proc/<pid>/stat` field 22 + boot_id; **broken inside the Linux sandbox's PID namespace** | `proc_pidinfo(PROC_PIDTBSDINFO)`; **denied inside Seatbelt for outside processes** | **not portable**; replaced by §8.3 |
| 8 | Harness kills: job object (W9), `taskkill` | [08 W9], [AR §7.1 rule] | bwrap PID-ns teardown = SIGKILL of everything inside [D][S]; unsandboxed orphans survive [C] | orphans reparented to launchd [C] | covered by crash-only design (§8.2) |
| 9 | Store refused on network and OneDrive paths | [AR §4.1] | + NFS/SMB/CIFS/9p/FUSE/virtiofs by `statfs.f_type` | + `MNT_LOCAL`, `f_fstypename` | extend (§9) |
| 10 | Git-Bash/PowerShell quoting, Defender close cost | [AR §7.1], [60 items 1, 8, 11] | not in this lens; POSIX argv, no Defender; the leader trigger of M0 item 1 is Windows-specific [I] | same; macOS EDR/XProtect costs unmeasured [I] | other lenses |

The OFD-vs-Windows ownership model and the timed wait (rows 1–3) are the heart of the multi-process
protocol. Row 7 is the only place where the current design is **unsound** on Linux, not merely
unported.

---

## 2. Unix lock primitives, compared with Windows `LockFileEx`

### 2.1 Traditional POSIX record locks (`F_SETLK`/`F_SETLKW`/`F_GETLK`, `lockf(3)`)

- **Ownership:** process-associated. Closing *any* descriptor for the file releases **all** of the
  process's locks on it: "all of the process's locks on that file are released, regardless of the file
  descriptor(s) on which the locks were obtained" [D, [fcntl_locking(2)](https://man7.org/linux/man-pages/man2/fcntl_locking.2.html)].
  macOS's own man page calls this "the completely stupid semantics of System V and IEEE Std 1003.1-1988"
  [S, [xnu fcntl.2](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/man/man2/fcntl.2)].
- **fork:** "Record locks are not inherited by a child created via fork(2)" [D].
- **Beyond EOF:** "Bytes past the end of the file may be locked" [D].
- **Same process:** there is no conflict between two descriptors of one process, and a re-lock
  merges/splits ranges. RocksDB therefore keeps a process-global set of locked paths because "fcntl()
  does not detect lock conflict if the fcntl is issued by the same thread that earlier acquired this
  lock" [S, [rocksdb env/fs_posix.cc](https://github.com/facebook/rocksdb/blob/main/env/fs_posix.cc)].
- **Deadlock detection:** `EDEADLK` is detected for traditional locks only [D].
- **Holder query:** `F_GETLK` returns `l_pid` for traditional locks. It "returns details about **one of**
  those locks", not necessarily the lowest-offset one [D]. Do not use a range `F_GETLK` to find the
  minimum.
- **`lockf(3)`** is a wrapper over the same process-associated locks on glibc and macOS [I]. It gains
  nothing.

### 2.2 Linux OFD locks (`F_OFD_SETLK`/`F_OFD_SETLKW`/`F_OFD_GETLK`)

- Since **Linux 3.15** [D]; standardised in **POSIX.1-2024** (the man page lists all three under
  POSIX.1-2024) [D].
- **Owner:** the open file description. Locks from different descriptions "conflict even when they are
  acquired by the same process"; they are "inherited across fork(2) (and clone(2) with CLONE_FILES)" and
  "only automatically released on the last close of the open file description" [D].
- **Rules:**
  - `l_pid` must be 0 (`EINVAL` otherwise).
  - There is no deadlock detection.
  - `F_OFD_SETLKW` returns `EINTR` when a caught signal arrives.
  - For a conflicting OFD lock, `F_OFD_GETLK` reports `l_pid = -1` [D].
- **OFD and traditional fcntl locks conflict with each other; `flock(2)` locks are independent of both**
  ("Since Linux 2.0, there is no interaction between the types of lock placed by flock(2) and fcntl()")
  [D].
- **Mandatory locking** was removed entirely in Linux 5.15 [D]. Locks are advisory only.
- **Large offsets:** glibc's `fcntl` on 32-bit targets translates through a 32-bit `off_t`. redb calls
  the raw `SYS_fcntl`/`SYS_fcntl64` with `flock64` ("Fix file locking on 32-bit GNU/Linux", commit of
  2026-09-07) [S]. moirai should support 64-bit targets only, or copy that shim.

### 2.3 macOS OFD locks — history, API status, implementation

Header status of `F_OFD_*` across XNU tags (downloaded and grepped for this report) [S]:

| XNU tag | macOS | `F_OFD_SETLK/W/GETLK/SETLKWTIMEOUT` guard | `fcntl.2` documents OFD |
|---|---|---|---|
| xnu-4570.1.46 | 10.13 High Sierra (2017) | `#ifdef PRIVATE` | no |
| xnu-6153.11.26 | 10.15 | `#ifdef PRIVATE` | no |
| xnu-7195.50.7.100.1 | 11 | `#ifdef PRIVATE` | no |
| xnu-8792.41.9 | 13 Ventura | `#ifdef PRIVATE` | no (0 mentions) |
| **xnu-10002.1.13** | **14.0 Sonoma (2023)** | **`#if __DARWIN_C_LEVEL >= __DARWIN_C_FULL`** (public SDK) | **yes (23 mentions)** |
| xnu-11215.1.10 | 15 Sequoia | public | yes |
| main (xnu-12377.x) | 26 | public | yes |

(xnu-10002.1.13 = macOS 14.0, per [C] [Wikipedia: macOS Sonoma](https://en.wikipedia.org/wiki/MacOS_Sonoma) and release reporting.)

What the current macOS `fcntl(2)` says [S, xnu `bsd/man/man2/fcntl.2`]:
- OFD locks "are locks on the file associated with the open file description used to acquire them, and
  not with the process that created them … conceptually similar to locks managed by flock(2), with the
  addition of record locking capabilities."
- "Only the last close of the last file descriptor in any process still referencing the open file
  description causes an automatic unlock."
- "No deadlock detection is performed for OFD file locks."
- `F_OFD_GETLK` sets `l_pid` to −1 for OFD and flock holders.

Implementation facts from `bsd/kern/kern_descrip.c`, `kern_lockf.c` and `kern_synch.c` [S]:
- `F_OFD_*` uses `ofd_to_id(fileglob)` as the owner, sets `FG_HAS_OFDLOCK` so the last close unlocks,
  and requires `FREAD` for `F_RDLCK` and `FWRITE` for `F_WRLCK` (`EBADF` otherwise). Open `LOCK` with
  `O_RDWR`.
- POSIX, flock and OFD locks share **one lock list per vnode** (`lf_advlock`). On macOS a whole-file
  `flock` held by anyone conflicts with our byte locks; on Linux it does not (§2.4).
- Waiters sleep in `msleep(lock, …, timeout)` and are woken **all at once** (`lf_wakelock`).
  `F_WAKE1_SAFE` applies only to pure-flock lists. Expect a small thundering herd with 16 writers, as on
  Linux.
- `F_SETLKWTIMEOUT` (10, traditional) and `F_OFD_SETLKWTIMEOUT` (93) pass a `struct flocktimeout
  { struct flock fl; struct timespec timeout; }`. `msleep` treats the timespec as a **relative interval,
  and a zero timespec as no timeout** [S]. `lf_setlock` loops: after every wake-up in which another
  waiter won, it sleeps again with the **full** timeout. The bound therefore applies per sleep, not to
  the whole call [S→I]. On timeout the call returns `ETIMEDOUT`.
- `libc` 0.2.189 exports `F_OFD_SETLK`, `F_OFD_SETLKW` and `F_OFD_GETLK` for Apple targets (comment:
  "See https://github.com/apple/darwin-xnu/…"). It exports neither `F_OFD_SETLKWTIMEOUT` nor
  `flocktimeout` [S].

**Assessment.** On macOS ≥ 14 the OFD API is public and documented. Kernels since 10.13 implement it
behind a private header. No macOS run confirmed behaviour here. The first macOS probe in M0 must verify
it (§10).

### 2.4 BSD `flock(2)`

- **Linux** [D, [flock(2)](https://man7.org/linux/man-pages/man2/flock.2.html)]:
  - associated with the open file description; `dup` and `fork` share the lock; two `open`s are
    independent;
  - conversion between shared and exclusive is not atomic;
  - no deadlock detection;
  - no interaction with fcntl locks;
  - NFS clients emulate flock as whole-file fcntl byte-range locks since 2.6.12, and CIFS/SMB emulates
    it with SMB byte-range locks since 5.5.
- **macOS:** the same OFD-like ownership, but the lock lives in the same vnode list as POSIX and OFD
  locks. The man page says "flock(2) and fcntl locks may be safely used concurrently", yet they conflict
  with one another [S].
- **Whole file only.** Four role bytes plus N liveness slots would need 4 + N files.

### 2.5 Rust std

- `File::lock`, `try_lock`, `lock_shared` and `unlock` call `flock(fd, LOCK_EX/SH[/NB])` on Linux, the
  BSDs, illumos and Apple, and `Unsupported` elsewhere [S, `library/std/src/sys/fs/unix.rs`, toolchain
  1.98.1].
- On Windows they call `LockFileEx` over the whole range [05 §6.3].
- Rust std opens files with `O_CLOEXEC` and spawns with `posix_spawn` where possible [S]. Locks held by
  moirai therefore do not leak across `exec`.

### 2.6 Side-by-side semantics

| Property | Windows `LockFileEx` | POSIX fcntl (process) | Linux OFD | macOS OFD (≥ 14) | flock (Linux / macOS) |
|---|---|---|---|---|---|
| Owner | handle + process [D][M] | process | open file description | open file description | open file description |
| 2nd open in same process conflicts | **yes** [M] | no | yes | yes | yes |
| Closing an unrelated handle drops locks | **no** [M] | **yes** | no | no | no |
| Re-lock of a held range, same owner | **fails** (non-reentrant) [M][D] | merges | merges | merges | converts |
| Byte ranges | yes | yes | yes | yes | whole file |
| Lock beyond EOF | yes (4 KiB, 2^32, 2^62 [M]) | yes [D] | yes | yes [S] | n/a |
| Mandatory vs advisory | **mandatory** for `ReadFile`/`WriteFile`, not for mapped views [D] | advisory | advisory | advisory | advisory |
| Inherited by child | no access for the child [D] | no | yes, fork shares it (CLOEXEC closes at exec) | yes | yes |
| Crash release | yes, **with lag** [D]: 1–8 ms p99, max 32 ms [M] | at exit | at the last close of the description | same | same |
| Timed blocking wait | overlapped + wait + `CancelIoEx` [D][M] | none (signals) | none | `F_OFD_SETLKWTIMEOUT` (per-sleep bound) [S] | none (signals) |
| Holder PID from the kernel | none | `l_pid` | −1 | −1 | −1 |
| Deadlock detection | none | `EDEADLK` | none | none | none |
| Interaction with flock | `File::lock` = whole-range LockFileEx: conflicts | Linux: none; macOS: conflicts | Linux: none; macOS: conflicts | conflicts | — |

---

## 3. Timed blocking waits

G1 requires a *blocking* wait: no sleep-backoff convoy. The wait is bounded at 2 s and ends in exit 7
naming the holder [AR §2.2].

| Option | Linux | macOS | Race-free? | Notes |
|---|---|---|---|---|
| A. Kernel-timed call | — | `F_OFD_SETLKWTIMEOUT` | yes | Each internal re-sleep restarts the full timeout, so the total wait under churn can exceed T [S→I]. A zero timespec means infinite [S]. The constant and struct must be declared locally. |
| B. Timer signal interrupts `F_OFD_SETLKW` (util-linux `flock -w`: `timer_create(CLOCK_MONOTONIC, SIGEV_SIGNAL)` + `SIGALRM` handler, fallback `setitimer`) [S, [flock.c](https://github.com/util-linux/util-linux/blob/master/sys-utils/flock.c), [lib/timer.c](https://github.com/util-linux/util-linux/blob/master/lib/timer.c)] | yes | yes | **no**: a signal that fires before the thread enters the syscall is lost unless the timer repeats | Needs a process-wide signal handler. Hostile to a library, and to the "no timers" rule. |
| C. Poll `F_OFD_SETLK` with backoff | yes | yes | yes | Rejected by G1 (convoy, p99 100–300 ms est. [AR §2.2]). SQLite's unix VFS has no real blocking-with-timeout: its `SQLITE_ENABLE_SETLK_TIMEOUT` path is "placeholder code" that polls every 1 ms in test builds [S, [os_unix.c](https://github.com/sqlite/sqlite/blob/master/src/os_unix.c)]. |
| **D. Waiter thread** blocked in `F_OFD_SETLKW` on its **own** open file description; the caller waits on a condvar until the deadline; hand-off under a mutex | yes | yes | **yes** | Zero CPU while parked. If the caller gave up, the waiter drops its description, and closing it releases the lock. |

**Recommendation: D on both Unixes.** It is the same semantics as Windows (overlapped `LockFileEx`,
`WaitForSingleObject`, `CancelIoEx`, then `GetOverlappedResult(wait=TRUE)` to settle a grant that raced
the cancel).
- The waiter must own a **separate** open file description, never a `dup` of a description that holds
  other grants. OFD locks of one description merge, so an abandoned waiter unlocking "its" byte could
  release a later grant made through the same description [I]. The first draft of the `xlock` sketch
  had exactly this bug.
- Cap it at one outstanding waiter per role per process. A later acquire joins the parked waiter
  instead of creating another.
- In the CLI, a timeout simply returns exit 7. Process exit discards the pending kernel request.
- Option A remains a possible macOS optimisation, used only after an M0 probe measures the per-sleep
  overrun (§10).
- The thread exists only while a contended acquisition is pending. This is a clarification to the
  "no threads, no timers" rule, like [40 §4.8]'s scoped worker threads.

---

## 4. How other engines lock across processes on Unix

| Engine | Unix mechanism | Crash behaviour | Lesson for moirai |
|---|---|---|---|
| **redb 4.3 / master** (`experimental-multiprocess`) | `F_OFD_SETLK`/`F_OFD_SETLKW`/`F_OFD_GETLK` on **Linux and Apple**, `LockFileEx` on Windows, in `range_lock.rs`. Comment: "LockFileEx region locks: per-handle, so they carry the same ownership model as open file description locks, and mandatory rather than advisory. Unlike fcntl's, they do not split or merge … each range is taken and released whole." Lock bytes at 2^62: writer, shared-writer, shared-reader, whole-file-reader, consistent byte, header lock `0..320`, `TXN_BASE + txn` reader bytes. Platforms without range locks fall back to the exclusive whole-file mode. [S, [range_lock.rs](https://github.com/cberner/redb/blob/master/src/tree_store/page_store/file_backend/range_lock.rs), [design.md](https://github.com/cberner/redb/blob/master/docs/design.md)] | "The OS automatically releases file locks when a process crashes" [S] | **The exact blueprint: OFD on both Unixes plus LockFileEx.** It is new code: "Give Windows the byte-range locks it has always had" (2026-08-29), "Fix file locking on 32-bit GNU/Linux" (2026-09-07), "Reserve backend lock bytes and serialize Windows lock queries" (2026-09-11) [S, commit log]. It is therefore not yet battle-tested. |
| **LMDB** | Lock file with a shared-memory reader table. Writer mutex: robust pthread mutex on Linux/FreeBSD. On Apple, **0.9 uses POSIX named semaphores** and mdb.master uses **SysV semaphores with `SEM_UNDO`**. Reader liveness: a 1-byte `fcntl` lock at **offset = pid** on the lock file (`mdb_reader_pid`) [S, [mdb.c master](https://github.com/LMDB/lmdb/blob/mdb.master/libraries/liblmdb/mdb.c), [0.9](https://github.com/LMDB/lmdb/blob/mdb.RE/0.9/libraries/liblmdb/mdb.c)] | 0.9 caveat: stale writers are cleared automatically on Windows and with robust mutexes, "not on BSD, systems using POSIX semaphores" [S, [lmdb.h 0.9](https://github.com/LMDB/lmdb/blob/mdb.RE/0.9/libraries/liblmdb/lmdb.h)] | Shared-memory synchronisation is not crash-safe on macOS. LMDB's **pid-byte lock is the idea behind §8.3's slot locks**: use locks, not PIDs, to prove liveness. |
| **SQLite** unix VFS | Traditional `fcntl` locks. A per-inode `unixInodeInfo` keyed by `(dev, ino)` counts locks and **defers `close()`** of other descriptors on the same inode until the locks clear (`pUnused`), to survive the close pitfall [S, os_unix.c]. Blocking-with-timeout is a placeholder (§3). | lock released at exit | This hygiene is what OFD makes unnecessary. |
| **RocksDB** | `fcntl(F_SETLK)` whole-file on `LOCK`, plus a process-global map of locked **paths** to catch same-process double-locks [S] | at exit | The path-keyed map misses hard links and symlinks. Another argument for OFD. |
| **Dolt / Beads** | Advisory `flock()` on `.dolt/noms/LOCK` for the store's lifetime; 100 ms then read-only [C, search summary of DoltHub docs] | at exit | **Deleting the LOCK file while held** let a second process lock a *new inode* and corrupt the chunk journal [C, [beads #2933](https://github.com/gastownhall/beads/issues/2933), 2026-03-31]. Hence the identity check in §6.2. |

---

## 5. Measurements on the owner's Windows machine [M]

Load during the runs: 16 `claude` processes, average CPU load ≈ 85 % (`Win32_Processor.LoadPercentage`).
The numbers are therefore upper bounds.

| Test (`winprobe`) | Result |
|---|---|
| Two handles to `LOCK` in one process: second `LockFileEx` on byte 0 | **fails** (`ERROR_LOCK_VIOLATION`): per-handle ownership, like OFD |
| Same handle re-locks byte 0 | **fails**: non-reentrant (OFD would silently succeed) |
| Opening and closing a third handle while handle 1 holds byte 1 | lock kept (POSIX `F_SETLK` would have lost it) |
| Lock bytes at 4096 (EOF), 2^32, 2^32 + 4095, 2^62 on a 4 KiB file | all succeed |
| Liveness probe = try-lock + `UnlockFile` on a free byte | p50 2.7–3.2 µs, p99 5.7–8.6 µs (n = 20,000) |
| Try-lock on a held byte | p50 1.6–1.9 µs, p99 3.4–5.1 µs |
| Overlapped wait of 300 ms while another process holds the byte | returns at 301–313 ms; `CancelIoEx` → `GetOverlappedResult` = 995 (`ERROR_OPERATION_ABORTED`) |
| Overlapped wait of 5 s; holder exits at ≈ 1.2 s | granted at 1.19–1.20 s |
| Lock free after `TerminateProcess` of the holder (timed from the `kill()` call, spinning on try-lock) | three runs: n = 60 p50 1.9 / p99 5.3 / max 9.0 ms; n = 200 p50 1.5 / p99 7.1 / max 8.4 ms; n = 100 p50 1.1 / p99 7.7 / **max 32.0 ms** |
| `OpenProcess(QUERY_LIMITED)` + `GetProcessTimes` (self) | p50 2.3–2.5 µs; a nonexistent PID returns `None` |

**`xlock` sketch.** It implements the §6.1 contract (≈ 250 lines, `libc = 0.2.189` on Unix):
- Its semantics test passes on Windows: two `LockFile`s in one process conflict, a 200 ms timed wait
  expires at ≥ 190 ms, a beyond-EOF slot byte works, and `held_elsewhere` sees it [M].
- It compiles with `cargo check --target x86_64-unknown-linux-gnu` and with
  `cargo +nightly check -Zbuild-std --target aarch64-apple-darwin`, the latter including the locally
  declared `F_OFD_SETLKWTIMEOUT`/`flocktimeout` [M, compile-only].
- It has **not been run on Linux or macOS**.

The W2 release lag stayed ≤ 32 ms in 360 kills, far below the 2 s bound. M0 item 12 still has to
measure it under the frozen protocol.

---

## 6. The recommended lock abstraction: one contract, three mappings

### 6.1 Contract (identical on every OS)

1. **Lock bytes carry no data.** They lie beyond the end of the file, so Windows' mandatory locking can
   never block a read of real bytes. Every lock is a single byte and exclusive. A shared byte is allowed
   only if a future reader-registration is added (§6.4).
2. **One owner per grant.** Each successful acquisition owns one open handle or open file description,
   kept in the grant. Two grants never share an owner. Releasing means unlock followed by close; dropping
   the grant also releases. This makes Windows (no merging) and OFD (merging) behave identically, and
   makes "two clients in one process" conflict on every OS. The deterministic simulator and tests can
   then run several clients in one process.
3. **Non-reentrant.** Acquiring a byte this process already holds through the same `LockFile` is a
   programming error, asserted in user space.
4. **Fixed lock order** (no OS detects deadlocks for OFD or `LockFileEx`): maintenance (2) → writer (0);
   slot bytes are never taken while waiting on a role byte. This is already implied by [AR §4.5 step 10].
5. **`acquire_within(byte, T)`** returns `Granted | Busy`, never both. A grant that races the deadline is
   either returned or released.
6. **`probe(byte)` → `Held | Free`.** On Unix it is side-effect-free (`F_OFD_GETLK`). On Windows it is
   try-lock + unlock, which can momentarily hold the byte (harmless for slots, §8.3). The kernel never
   reveals the holder: `l_pid = −1` for OFD, and Windows has no query. Holder identity comes only from
   moirai's own records in `LOCK`.
7. **The process never spawns a child while holding a role byte.**
   - All descriptors are `O_CLOEXEC` / non-inheritable (Rust std default).
   - Rationale: an OFD shared into a child that outlives the parent would keep the lock alive [D].
     Windows gives an inheriting child no access to the region [D], so behaviour diverges exactly
     there.
8. **Never `flock`/`File::lock` a store file.** On macOS it conflicts with OFD; on Linux it does not.
9. **`LOCK` identity.**
   - `LOCK` is created only by `init` (`O_CREAT|O_EXCL`) and never deleted by moirai; `doctor` refuses.
   - After opening, compare `fstat(fd)` (dev, ino) — on Windows `FileIdInfo` — with a fresh
     `stat(path)`. On mismatch the file was replaced: close and retry, then refuse.
   - This defends against the Beads #2933 class.

### 6.2 Per-OS mapping

| Contract item | Windows 10/11 | Linux ≥ 3.15 (glibc ≥ 2.20 or musl; 64-bit) | macOS ≥ 14 |
|---|---|---|---|
| Open `LOCK` | `CreateFileW(GENERIC_READ\|GENERIC_WRITE, FILE_SHARE_READ\|WRITE\|DELETE, OPEN_EXISTING, FILE_FLAG_OVERLAPPED)` | `open(O_RDWR\|O_CLOEXEC)`; `F_WRLCK` requires write access (`EBADF`) | same as Linux (`FWRITE` check in xnu) |
| Try-acquire byte b | `LockFileEx(EXCLUSIVE\|FAIL_IMMEDIATELY, len 1, off b)`; 33 = busy | `fcntl(F_OFD_SETLK, {F_WRLCK, SEEK_SET, b, 1, l_pid 0})`; `EAGAIN`/`EACCES` = busy | `fcntl(F_OFD_SETLK=90, …)`; same errors |
| Bounded wait | overlapped `LockFileEx`, `WaitForSingleObject(T)`, `CancelIoEx`, `GetOverlappedResult(TRUE)`: granted or 995 | waiter thread on a fresh description, `F_OFD_SETLKW` (EINTR loop), condvar deadline, hand-off/drop | same as Linux; optional `F_OFD_SETLKWTIMEOUT=93` with local `flocktimeout` (per-sleep bound) |
| Release | `UnlockFile(b, 1)` then close | `F_OFD_SETLK F_UNLCK` then close | same |
| Probe | try + `UnlockFile` | `F_OFD_GETLK`: `l_type != F_UNLCK` → held | `F_OFD_GETLK=92` |
| Crash release | OS, with lag (§5) | at exit, when the last reference to the description closes | same |
| Unsupported FS | refuse `DRIVE_REMOTE`, UNC including `\\wsl$` | `EINVAL`/`ENOTSUP`/`EOPNOTSUPP` → refuse (redb maps these to `Unsupported` [S]); plus the `statfs` gate (§9) | same, plus `MNT_LOCAL` |

### 6.3 `LOCK` file layout v1 — the same bytes on every OS (proposal; a format-v1 reservation)

```
offset 0      LockHdr  { magic "MLCK", format u16, flags u16, store_epoch u64, n_slots u16, … }  (≤ 64 B)
offset 2048   WriterDiag { seq u64, proc ProcId, session_hash u64, cmd[≤ 400 B], hlc u64, xxh3 }  ← written by the byte-0 holder
offset 3072   LeaderRec  { seq u64, proc ProcId, proto u16, endpoint_kind u8 (1 pipe | 2 uds),
                           endpoint[≤ 200 B], nonce u128, xxh3 }                               ← written by the leader-byte holder
offset 4096   SlotRec[256] × 128 B { nonce u128, proc ProcId, parent ProcId-lite{pid u32, start u64},
                           kind u8 (session | intent | leader), session_hash u64, hlc u64, xxh3 }  ← written by slot i's holder
lock bytes (no data, beyond EOF on every OS):
  ROLE_BASE = 2^62        : +0 writer, +1 leader, +2 maintenance, +3 quiet-advisory, +4..+63 reserved
  SLOT_BASE = 2^62 + 2^16 : +i liveness slot i (i < n_slots)
ProcId (40 B) { os u8, flags u8, _ u16, pid u32, start u64, boot_id [16], pidns u64 }  (§8.1)
```

Changes relative to [AR §4.1]:
- **(a)** Role bytes move from 0–3 to 2^62 + 0–3. On Windows, a backup, copy, `doctor` hex dump or AV
  read of `LOCK` then never hits `ERROR_LOCK_VIOLATION` on bytes 0–3. The move costs nothing on Unix and
  is measured working at 2^62 [M]. It also matches redb.
- **(b)** Diagnostics become a checksummed `ProcId` record instead of `{pid, start_ms}`.
- **(c)** A leader record and 256 liveness slots are added: 36 KiB in total, fixed at `init`, never
  resized.

Records are written only by the holder of the matching byte. Readers accept a record only if its
checksum is valid, and re-read it after probing (seqlock style).

### 6.4 If per-transaction reader bytes are ever added (the redb protocol of [08 §3.3])

| redb/[08] element | Windows | Linux | macOS ≥ 14 |
|---|---|---|---|
| Writer byte `BASE` (X) | `LockFileEx` X | `F_OFD_SETLK` `F_WRLCK` | same |
| Reader byte `TXN_BASE + txn` (S) | `LockFileEx` shared | `F_OFD_SETLK` `F_RDLCK` (fd readable) | same |
| Header lock `0..320` (X/S) | mandatory: blocks other processes' `ReadFile` of the header while held (redb relies on it) | advisory: every reader must take it | advisory |
| "Oldest live reader" scan | per-byte try-exclusive (side effect) | `F_OFD_GETLK` over a range as a **boolean**, then bisection. The kernel returns "one of" the conflicting locks, not the lowest [D]. | same |

[AR] currently needs none of this: readers take no locks, and GC uses pins plus a 60 s grace. The
mapping shows that the choice stays open on all three OSes.

### 6.5 Failure modes of the abstraction

| Failure | Windows | Linux | macOS | Handling |
|---|---|---|---|---|
| Holder killed (`TerminateProcess` / `SIGKILL` / OOM / sandbox teardown) | release after ≤ 32 ms observed [M] | at exit | at exit | waiters proceed; recovery scan (unchanged) |
| Holder suspended (`NtSuspendProcess`, `SIGSTOP`, debugger, laptop sleep) | lock held | held | held | 2 s bound → exit 7 naming the holder (liveness = alive) |
| Descriptor leaked into a child that outlives the parent | child has no access; lock released with the parent | **lock survives** until the child closes it | same | CLOEXEC everywhere; no spawn while holding |
| Another tool uses `flock`/`File::lock` on `LOCK` | conflicts (whole range) | invisible | conflicts | never done by moirai; `doctor` warns if `LOCK` is flock-held (Unix probe via `flock(LOCK_NB)`) |
| `LOCK` deleted or replaced while held | POSIX-semantics delete removes the name [I] | new inode | new inode | identity check (§6.1 item 9); never delete |
| Abandoned waiter wins after the deadline | settled by `GetOverlappedResult` | dropping its own description releases the lock | same | contract item 5 |
| Signals | n/a | `EINTR` → retry | `EINTR` → retry (sketch restarts the timeout; the product computes the remainder) | loop |
| Store on a FS without cross-client lock coherence | SMB, `\\wsl$` | NFS, SMB, 9p (WSL drvfs), FUSE/virtiofs | NFS, SMB, AFP, WebDAV, macFUSE | refuse at open (§9) |
| 32-bit glibc | — | `off_t` truncation | — | 64-bit only, or the `flock64`/raw-syscall shim |

### 6.6 Rejected alternatives

| Alternative | Why rejected |
|---|---|
| `flock` on per-role files (`LOCK.w`, `LOCK.l`, …) on macOS | Unnecessary on macOS ≥ 14. Liveness slots would need one file per slot. On Linux flock is invisible to fcntl, so mixed tooling diverges. Kept only as the fallback if the owner requires macOS ≤ 13 (open question 1). |
| Process-associated `fcntl` plus SQLite-style fd hygiene | No conflict between two clients in one process, so the in-process simulator and tests diverge from Windows. The close pitfall across all code paths. Lost on fork. Merging semantics. |
| Shared-memory robust mutexes / semaphores (LMDB) | macOS has no robust pthread mutexes. POSIX semaphores are not released on crash (LMDB caveat [S]). SysV `SEM_UNDO` is macOS/BSD-only in LMDB's matrix. Adds a crash-state surface the simulator must model. |
| `LockFileEx` through Rust `File::lock` | whole-range; blocks every other reader of the file [05 §6.3] |

---

## 7. IPC: Unix-domain sockets vs named pipes, and the sandboxes

### 7.1 Facts

| Property | Windows named pipe | Linux UDS | macOS UDS |
|---|---|---|---|
| Name limit | 256 chars | `sun_path` **108** bytes including NUL [D, [unix(7)](https://man7.org/linux/man-pages/man7/unix.7.html)] | **104** [S, xnu `bsd/sys/un.h`]; no `bindat`/`connectat` in xnu `syscalls.master` [S] |
| Namespace without files | pipe namespace (process-lifetime) | abstract namespace: Linux-only, "Socket permissions have no meaning", scoped per network namespace [D] | none |
| Access control | DACL; the default grants Everyone read [08 W8] | "connecting … requires write permission on that socket", plus directory search permission; some BSDs ignored socket permissions historically [D] | use a 0700 directory (portable) [I] |
| Peer identity | `GetNamedPipeClientProcessId` / `…ServerProcessId` | `SO_PEERCRED` (credentials at `connect`/`listen`), `SO_PEERPIDFD` since **6.5** [C, [LWN](https://lwn.net/Articles/926312/)]; PIDs are translated across PID namespaces [D] | `getpeereid`, `LOCAL_PEERCRED`, `LOCAL_PEERPID`, `LOCAL_PEEREPID`, `LOCAL_PEERUUID`, `LOCAL_PEERTOKEN` (audit token, which carries the pid version) [S, un.h] |
| Stale endpoint after a crash | disappears with the last handle | socket file remains → `EADDRINUSE` on `bind` | same |
| Rust | no AF_UNIX in std on Windows [08 W7]; `windows-sys` | `std::os::unix::net` | same |
| Round trip | ≈ 60 µs measured through PowerShell [08 §2] | not measured (no Linux) | not measured |

Windows also has AF_UNIX since Windows 10 1803, with a peer-PID ioctl `SIO_AF_UNIX_GETPEERPID`
[C, [WSL #4676](https://github.com/microsoft/WSL/issues/4676)]. It would unify the transport, but it is
not needed: the wire protocol is transport-independent, and the named-pipe design is specified and
measured.

### 7.2 Claude Code sandboxes (docs re-fetched 2026-09-26; sandbox-runtime source at `main`, pushed 2026-09-25)

| Aspect | Linux / WSL2 (bubblewrap) | macOS (Seatbelt) | Windows |
|---|---|---|---|
| What is sandboxed | Bash, PowerShell and Monitor commands and their children; **not** MCP servers or hooks [D, [sandboxing](https://code.claude.com/docs/en/sandboxing) "Scope"] | same | Claude Code: "Native Windows is not supported" [D] |
| PID namespace | `--unshare-pid` + fresh `--proc /proc` in secure mode; the comment reads "If we don't have --unshare-pid, it is possible to escape the sandbox" [S, [linux-sandbox-utils.ts](https://github.com/anthropics/sandbox-runtime/blob/main/src/sandbox/linux-sandbox-utils.ts)]. `enableWeakerNestedSandbox` bind-mounts the host `/proc` instead [D]. | none | — |
| Process lifetime | `--new-session --die-with-parent` [S]; when a namespace's init exits, "the kernel terminates all of the processes in the namespace via a SIGKILL signal" [D, [pid_namespaces(7)](https://man7.org/linux/man-pages/man7/pid_namespaces.7.html)] | normal | — |
| Seeing other processes | only its own namespace; `getppid()` = 0 for a parent outside [D] | `(allow process-info* (target same-sandbox))`, `(allow signal (target same-sandbox))`; the sysctl prefixes `kern.proc.pid.`, `kern.proc.all`, `kern.proc.pgrp.` are readable [S, [macos-sandbox-utils.ts](https://github.com/anthropics/sandbox-runtime/blob/main/src/sandbox/macos-sandbox-utils.ts)] | — |
| Unix sockets | seccomp `socket(AF_UNIX, …)` → `EPERM`; `io_uring_*` blocked entirely [S, [seccomp-unix-block.c](https://github.com/anthropics/sandbox-runtime/blob/main/vendor/seccomp-src/seccomp-unix-block.c)]; only `allowAllUnixSockets: true` permits them ("the seccomp filter can't inspect socket paths") [D, [settings-reference](https://code.claude.com/docs/en/settings-reference)] | denied unless the path is under `allowUnixSockets` (a `subpath` rule for bind and connect) or `allowAllUnixSockets` [S][D] | — |
| Network namespace | `--unshare-net` (abstract sockets of the host are unreachable) [S] | proxy-filtered | — |
| Writes | cwd, per-user temp (`$TMPDIR` is **rewritten** for sandboxed commands), `--add-dir`, the main repo's shared `.git` for linked worktrees except `hooks/` and `config` [D] | same | — |
| Environment | inherited; `CLAUDE_CODE_SESSION_ID` is set in Bash, hooks and stdio MCP servers (an MCP server "retains the ID it was spawned with", while Bash and hooks change it on `/clear`) [D, [env-vars](https://code.claude.com/docs/en/env-vars)] | same | same |
| Future Windows sandbox (`srt-win`, sandbox-runtime source, not shipped in Claude Code) | — | — | commands run as a **separate local user `srt-sandbox`** with a restricted token; "it cannot reach real-user processes" [S, [windows-sandbox-utils.ts](https://github.com/anthropics/sandbox-runtime/blob/main/src/sandbox/windows-sandbox-utils.ts)]. A pipe DACL for the owner's SID would reject it, and `OpenProcess` on owner processes would fail [I]. |

Whether Seatbelt gates `fcntl`/OFD locking separately (a "file-lock" operation) could not be determined.
The profile contains no lock rule [S]. Must be probed on a Mac (§10) [I].

### 7.3 What this means for the MCP server and the optional leader

- **Correctness never depends on IPC** [AR §2.2]. The Unix sandboxes make this mandatory rather than
  optional:
  - An agent's sandboxed Linux CLI cannot create an AF_UNIX socket at all.
  - A sandboxed macOS CLI can connect only if the owner lists the directory in `allowUnixSockets`.
  - Hooks and MCP servers, being unsandboxed, can use IPC.
- **Endpoint rendezvous through `LOCK`, not through name derivation.**
  - The leader, while holding the leader byte, first unlinks any stale socket and binds.
  - It then writes `LeaderRec{proc, proto, endpoint, nonce}`.
  - Clients read `LeaderRec`, `probe(LEADER)` and connect. Both sides authenticate: the server checks
    `peer uid == euid` (`SO_PEERCRED` / `getpeereid`; on Windows, DACL + `GetNamedPipeClientProcessId`).
    The client checks that the server PID equals `LeaderRec.proc.pid` (`SO_PEERCRED`, `LOCAL_PEERPID`,
    `GetNamedPipeServerProcessId`) and that the server echoes `nonce`.
  - The leader byte replaces the classic "is this socket stale?" connect-probe race.
- **Endpoint location:**
  - Windows: `\\.\pipe\moirai-<h(user SID)>-<h(store)>`, as specified.
  - Linux: `$XDG_RUNTIME_DIR/moirai/<h16>.s` when `XDG_RUNTIME_DIR` is set, owned by the user and 0700,
    else `/tmp/moirai-<uid>/<h16>.s`.
  - macOS: `confstr(_CS_DARWIN_USER_TEMP_DIR)/moirai/<h16>.s`.
  - In every case the directory is created 0700 and verified with `lstat` (not a symlink, uid, mode):
    ≈ 45–80 bytes, under 104.
  - Do not derive the location from `$TMPDIR`: Claude Code gives sandboxed and unsandboxed commands
    different values [D].
  - Do not put the socket in `<git-common-dir>/moirai/`: the path length is unbounded, and a socket
    inode in `.git` breaks copy and backup tools [I].
- **The leader decision stays Windows-driven** [I]. Its M0 triggers are the Defender close cost and the
  16-writer `LockFileEx` wait [60 items 1–2]. Linux and macOS have no Defender, and their sandboxed CLIs
  could not forward anyway. Measure the Unix 16-writer OFD wait (§10) before extending the trigger to
  them.

---

## 8. Process liveness

### 8.1 Per-OS identity and lookup

| | Windows | Linux | macOS |
|---|---|---|---|
| Start time | `GetProcessTimes` creation `FILETIME`: absolute, 100 ns [D]; 2.3–2.5 µs [M] | `/proc/<pid>/stat` field 22 `starttime`, "clock ticks" since boot, divide by `sysconf(_SC_CLK_TCK)`. Field 2 `comm` is parenthesised and may contain spaces or `)`, so parse from the last `)` [D, [proc_pid_stat(5)](https://man7.org/linux/man-pages/man5/proc_pid_stat.5.html)] | `proc_pidinfo(PROC_PIDTBSDINFO)` → `pbi_start_tvsec/usec`: absolute epoch time [S, `proc_info.h`]; or `sysctl KERN_PROC_PID` → `kp_proc.p_starttime` |
| Boot identity (needed for Linux, whose start time is boot-relative) | not needed (absolute) | `/proc/sys/kernel/random/boot_id` [I: standard; not re-verified here] | `sysctl kern.bootsessionuuid` [I] (absolute start time makes it optional) |
| Namespace | none | PID namespace: record `st_ino` of `/proc/self/ns/pid`. A different namespace means the process cannot be observed. | none |
| PID space and reuse | multiples of 4, reused aggressively [08] | `pid_max` 32,768 by default and up to 2^22; systemd raises it [I, not re-verified] | `PID_MAX 99999`, sequential with wrap; `p_idversion` is incremented per process [S, `proc_internal.h`, `kern_fork.c`] and exposed through audit tokens (`LOCAL_PEERTOKEN`) |
| Stable handle | process handle (`OpenProcess`) | `pidfd_open` since **5.3**; "stable reference immune to PID reuse", readable at termination [D, [pidfd_open(2)](https://man7.org/linux/man-pages/man2/pidfd_open.2.html)]; `SO_PEERPIDFD` 6.5 | `kqueue EVFILT_PROC NOTE_EXIT` on a pid (race with reuse unless combined with a start-time check) [I] |
| "Dead" | `ERROR_INVALID_PARAMETER` from `OpenProcess`, or an exit code other than `STILL_ACTIVE` | `ENOENT`, state `Z`/`X`, or start time differs | `ESRCH`, `SZOMB`, or start time differs |
| "Cannot tell" | `ERROR_ACCESS_DENIED` (other user, or the future `srt-sandbox` user) | different PID namespace; `/proc` hidden or unmounted | `EPERM` inside Seatbelt |

**Decision rule** `alive(ProcId) → Alive | Dead | Unknown`, used only for diagnostics and as a fallback:
- a different OS tag or boot id → **Dead**: the process cannot be running on this boot, because stores
  are single-kernel (§9);
- a different PID namespace → **Unknown**;
- a lookup failure that means "dead" → **Dead**;
- a lookup failure that means "denied" → **Unknown**;
- a start-time mismatch → **Dead**;
- otherwise → **Alive**.

`Unknown` never expires anything. TTL and run-scoped rules still apply.

### 8.2 What the harness does to processes, per OS

| Event | Windows | Linux, sandboxed Bash | Linux/macOS, unsandboxed Bash | MCP server (all OSes) |
|---|---|---|---|---|
| Tool command stopped or killed | job-object kill of the tree [08 W9] | bwrap `--die-with-parent`; PID-ns init exit → SIGKILL of every descendant, including `nohup`/`setsid` daemons [S][D] | a timeout moves the command to the background, not a kill [D, [tools-reference](https://code.claude.com/docs/en/tools-reference)]; orphans reparented to PID 1/launchd survive [C, claude-code [#84647](https://github.com/anthropics/claude-code/issues/84647), [#81462](https://github.com/anthropics/claude-code/issues/81462), [#82433](https://github.com/anthropics/claude-code/issues/82433), [#96625](https://github.com/anthropics/claude-code/issues/96625)] | stdio shutdown: close stdin → `SIGTERM` → `SIGKILL` [D, [MCP lifecycle 2025-11-25](https://modelcontextprotocol.io/specification/2025-11-25/basic/lifecycle)] |
| Locks held at death | released with lag (§5) | released at exit | released at exit | same |

Consequences [I]:
- Every moirai CLI operation is bounded: a 2 s lock wait plus bounded work. An orphaned CLI therefore
  finishes or crashes harmlessly, and nothing else needs doing.
- No process started from sandboxed Linux Bash can outlive the command, so a leader can never be
  "spawned by the CLI". The design already forbids that.
- The MCP server should exit when its parent (Claude) dies, event-driven and without timers:
  - Linux: `prctl(PR_SET_PDEATHSIG, SIGTERM)` plus a re-check of `getppid()`;
  - macOS: `kqueue EVFILT_PROC NOTE_EXIT` on the parent;
  - Windows: wait on the parent's process handle together with stdin.

  Its liveness slot (§8.3) then mirrors the Claude process's lifetime even if stdin EOF never arrives,
  for example when the pipe's write end leaked into a surviving grandchild [I].

### 8.3 Recommendation: lock-anchored liveness slots

**Problem.** [AR §6.2] records `(pid, start)` on leases, and [40 §2.6/§3.4] records it on `FsIntent`.
- When the recorder is an agent's CLI inside the Linux sandbox, the PID belongs to a private namespace,
  and the next sandboxed command reuses the same small PIDs. The "holder's Claude process" cannot be
  found at all (`getppid()` = 0).
- A checker outside the sandbox compares against an unrelated host PID. A checker inside another sandbox
  compares against its own namespace.
- With start time also compared, the likely result is **false "dead"**, which would recover or roll
  back an in-flight `FsIntent` or expire a live lease [I].
- On macOS, Seatbelt makes the lookup `EPERM`, which is merely unobservable.

**Design (identical on every OS):**
1. A long-lived process takes the first free slot byte `SLOT_BASE + i`. It then writes `SlotRec[i]` with
   a fresh random `nonce`, its `ProcId`, its parent's pid and start time, `kind`, and the hash of
   `CLAUDE_CODE_SESSION_ID`. It holds the byte for its lifetime.
2. **Anchor liveness** of `(i, nonce)`:
   - read `SlotRec[i]`; a checksum or nonce mismatch → **Dead**;
   - else `probe(SLOT_BASE + i)`: free → **Dead**;
   - held → re-read; nonce changed → **Dead**;
   - else → **Alive**.

   A new owner that has taken slot *i* but not yet rewritten the record looks alive for microseconds.
   That errs in the safe direction.
3. **Who anchors:**
   - the **MCP server** (`kind = session`) for the whole session;
   - a **CLI during a file intent** (`kind = intent`), from `FsIntent` until `FsIntentDone/Aborted`,
     so [40 §3.4] step 5 checks the anchor, not the PID;
   - the **leader**, whose leader byte already is its anchor.
4. **Lease holder** = the anchor of the session's MCP server.
   - A CLI finds it by matching `CLAUDE_CODE_SESSION_ID` against `SlotRec.session_hash`.
   - After `/clear`, the unsandboxed `SessionStart` hook appends the new session id as an alias to the
     slot whose `parent` equals the hook's Claude ancestor.
   - With no MCP server, a lease has no anchor and is governed by TTL and run scope only, as today.
5. **Properties:**
   - it works across PID namespaces, bind mounts and both sandboxes, because locks belong to the inode
     and bwrap bind-mounts the same inode [I];
   - it is immune to PID reuse and needs no `/proc` or `proc_pidinfo`;
   - the cost is one probe: 2–9 µs on Windows [M], and a single `fcntl` on Unix (unmeasured);
   - on Windows a dead anchor reads "alive" for up to the release lag (≤ 32 ms observed), which errs in
     the safe direction.
6. `ProcId` stays in every record for `doctor`, for exit-7 messages and for the §8.1 fallback.

---

## 9. Single-kernel rule: filesystems on which the store must be refused

Byte-range locks coordinate processes of **one kernel**. The design already refuses network and OneDrive
paths [AR §4.1]. Cross-OS use adds cases where two kernels see one directory:

| Case | Detection | Why |
|---|---|---|
| WSL2 `/mnt/c`, `/mnt/d` (drvfs = 9p) | Linux `statfs.f_type == V9FS_MAGIC 0x01021997` [D, [statfs(2)](https://man7.org/linux/man-pages/man2/statfs.2.html)] | Windows processes use `LockFileEx` on NTFS, WSL processes use OFD locks in the Linux VM, and neither sees the other [I, unverified: WSL is not installed]. Claude Code recommends WSL2 for sandboxing on Windows [D]. |
| Windows access to WSL files (`\\wsl$\…`, `\\wsl.localhost\…`) | UNC → already refused as remote | same, reversed |
| Docker Desktop / Lima / Podman-machine bind mounts (virtiofs, gRPC-FUSE) | `FUSE_SUPER_MAGIC 0x65735546` (virtiofs reports the FUSE magic [I]) | the container kernel is not the host kernel |
| NFS / SMB / CIFS | `0x6969`, `0x517b`, `0xff534d42`, `0xfe534d42` [D]; macOS: `MNT_LOCAL` clear or `f_fstypename ∈ {nfs, smbfs, afpfs, webdav}` [I] | lock coherence is server-dependent; LMDB and SQLite WAL forbid remote FS [08] |
| iCloud Drive / Dropbox on macOS | path prefix under `~/Library/Mobile Documents` or `CloudStorage`; `SF_DATALESS` [I] | the sync daemon may replace inodes, which defeats the identity check (other lens) |

A local FUSE store (for example an encrypted home directory) would be refused by the FUSE rule. An
explicit `store.allow-fs` override is an owner decision (open question 10).

---

## 10. Consequential edits and new gates (for [AR], [40], [60])

| Document | Edit |
|---|---|
| [AR §4.1] `LOCK` row | Lock bytes at `2^62 + {0 writer, 1 leader, 2 maintenance, 3 quiet}`; slots at `2^62 + 2^16 + i`; records per §6.3; file = 36 KiB fixed; created only by `init`; never deleted. |
| [AR §4.5 step 1], [AR §6.1] | "Acquire the writer byte through `LockBytes::acquire_within(2 s)`" with the per-OS mapping of §6.2, instead of the literal `LockFileEx` calls. |
| [AR §4.10] | Replace "Unix builds swap `fdatasync`, `fcntl` byte locks and `mmap`" with: **OFD locks** (`F_OFD_*`, Linux ≥ 3.15, macOS ≥ 14), never process-associated `fcntl` and never `flock`; the §6.1 contract; the §9 filesystem gate. |
| [AR §6.2], [AR §3.4/§5d] lease row, `LEASES` section | The lease holder is an **anchor** `(slot u16, nonce u128)` plus a diagnostic `ProcId`; liveness per §8.3; `Unknown` never expires. |
| [40 §2.6] `FSINTENT`, [40 §3.4 step 5] | `pid, pid_start` → `(slot, nonce)` + `ProcId`; recovery only when the anchor is dead. |
| [AR §6.1] leader row, [08 W8] | The endpoint is published in `LeaderRec`; UDS in a per-user 0700 runtime dir on Unix; peer checks per §7.3. |
| [60 §2.5] format freeze | Add the §6.3 `LOCK` layout and `ProcId` to the format-v1 reservations (they are frozen at M0 exit). |
| [60 §5.2] M0 measurements | Add, for **Linux (x86-64, ext4 and btrfs) and macOS (Apple silicon, APFS)**: (a) 16-writer wait p50/p99 and fairness through the waiter-thread path (the item-2 analogue); (b) lock-free time after `SIGKILL` of a holder (the item-12 analogue; expected "at exit"); (c) macOS `F_OFD_SETLKWTIMEOUT` overrun under 16-way churn; (d) **inside Claude Code's sandbox**: OFD acquire on `.git/moirai/LOCK` (Seatbelt "file-lock"?), anchor probe across two sandboxed commands, UDS connect refused → direct fallback; (e) the `/proc` stat and `proc_pidinfo` costs. |
| [60 GT4] "Windows kill loop" | Rename it to "kill loop" with per-OS drivers: `TerminateProcess` / `SIGKILL`; `NtSuspendProcess` / `SIGSTOP`+`SIGCONT` (macOS `task_suspend` is equivalent [I]); disk-full via VHDX / loop-mounted ext4 of fixed size / `hdiutil` APFS image; clock steps inside a VM (they need root or `CAP_SYS_TIME`). Lock-delay injection in GT3 becomes a per-OS parameter (Windows: measured distribution; Unix: 0). |
| [60 §3.1] test host, OS-crash rig | Needs Linux and macOS hosts. macOS guests are licensed only on Apple hardware, so a macOS OS-crash rig needs a Mac (open question 7). |

---

## 11. Open questions (owner decisions)

1. **Minimum macOS version.** Require macOS 14+ (public OFD API)? Or also support 10.13–13 by
   declaring the private constants, or through a flock-per-role fallback with extra files?
2. **Minimum Linux.** Kernel ≥ 3.15 with glibc ≥ 2.20 or musl, 64-bit only?
3. **WSL2 policy.** Refuse stores on drvfs (`/mnt/*`) and `\\wsl$`? That means repositories used from
   WSL2 Claude sessions must live on the WSL ext4 disk, not on `D:`.
4. **Liveness anchors.** Replace `(pid, start)` with lock-anchored slots as the correctness mechanism
   for leases and file intents? This is a format-v1 change to `LOCK`, frozen at M0.
5. **`LOCK` byte relocation.** Move role bytes from 0–3 to 2^62+ so `LOCK` stays readable on Windows?
6. **Leader scope.** Keep the M0 leader trigger Windows-only (Defender), or evaluate it per OS?
7. **Test infrastructure.**
   - Provide a Linux host and a Mac (Apple silicon) for nightly gates and the OS-crash rig? Otherwise,
     accept hosted CI runners (no power-cut testing on macOS).
   - Install WSL2 on the owner machine for Linux probes?
8. **Sandboxed forwarding.** Allow agents' sandboxed CLIs to reach the leader, which needs
   `allowUnixSockets` on macOS and the broad `allowAllUnixSockets` on Linux? Or keep them always
   direct (recommended)?
9. **Future Windows sandbox** (`srt-win`, separate `srt-sandbox` user). Plan now for pipe DACLs and file
   ACLs that admit it, or treat it as out of scope until Claude Code ships it?
10. **Filesystem refusal strictness.** Allow a local FUSE filesystem through an explicit
    `store.allow-fs` override?

---

## 12. Sources

Primary (source code or man-page source read for this report; local copies are in the probe directory):
- XNU, Apple open source: [`bsd/sys/fcntl.h` (main)](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/sys/fcntl.h); tags [xnu-4570.1.46](https://github.com/apple-oss-distributions/xnu/blob/xnu-4570.1.46/bsd/sys/fcntl.h), [xnu-8792.41.9](https://github.com/apple-oss-distributions/xnu/blob/xnu-8792.41.9/bsd/sys/fcntl.h), [xnu-10002.1.13](https://github.com/apple-oss-distributions/xnu/blob/xnu-10002.1.13/bsd/sys/fcntl.h), [xnu-11215.1.10](https://github.com/apple-oss-distributions/xnu/blob/xnu-11215.1.10/bsd/sys/fcntl.h); [`bsd/man/man2/fcntl.2`](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/man/man2/fcntl.2); [`bsd/kern/kern_descrip.c`](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/kern_descrip.c); [`bsd/kern/kern_lockf.c`](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/kern_lockf.c); [`bsd/kern/kern_synch.c`](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/kern_synch.c); [`bsd/sys/un.h`](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/sys/un.h); [`bsd/sys/proc_info.h`](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/sys/proc_info.h); [`bsd/sys/proc_internal.h`](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/sys/proc_internal.h); [`bsd/kern/kern_fork.c`](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/kern_fork.c); [`bsd/kern/syscalls.master`](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/syscalls.master).
- Linux man-pages 6.19: [fcntl_locking(2)](https://man7.org/linux/man-pages/man2/fcntl_locking.2.html), [flock(2)](https://man7.org/linux/man-pages/man2/flock.2.html), [unix(7)](https://man7.org/linux/man-pages/man7/unix.7.html), [pid_namespaces(7)](https://man7.org/linux/man-pages/man7/pid_namespaces.7.html), [proc_pid_stat(5)](https://man7.org/linux/man-pages/man5/proc_pid_stat.5.html), [pidfd_open(2)](https://man7.org/linux/man-pages/man2/pidfd_open.2.html), [statfs(2)](https://man7.org/linux/man-pages/man2/statfs.2.html).
- Microsoft: [LockFileEx](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-lockfileex).
- redb: [range_lock.rs](https://github.com/cberner/redb/blob/master/src/tree_store/page_store/file_backend/range_lock.rs), [docs/design.md "Multi-process concurrency"](https://github.com/cberner/redb/blob/master/docs/design.md), [CHANGELOG](https://github.com/cberner/redb/blob/master/CHANGELOG.md).
- LMDB: [mdb.c master](https://github.com/LMDB/lmdb/blob/mdb.master/libraries/liblmdb/mdb.c), [mdb.c 0.9](https://github.com/LMDB/lmdb/blob/mdb.RE/0.9/libraries/liblmdb/mdb.c), [lmdb.h 0.9](https://github.com/LMDB/lmdb/blob/mdb.RE/0.9/libraries/liblmdb/lmdb.h).
- SQLite: [src/os_unix.c](https://github.com/sqlite/sqlite/blob/master/src/os_unix.c). RocksDB: [env/fs_posix.cc](https://github.com/facebook/rocksdb/blob/main/env/fs_posix.cc). util-linux: [sys-utils/flock.c](https://github.com/util-linux/util-linux/blob/master/sys-utils/flock.c), [lib/timer.c](https://github.com/util-linux/util-linux/blob/master/lib/timer.c).
- Rust: `library/std/src/sys/fs/unix.rs` (toolchain 1.98.1, local rust-src: `File::lock` = `flock`, `O_CLOEXEC`); `libc` 0.2.189 `src/unix/bsd/apple/mod.rs` (`F_OFD_*` = 90/91/92); rustix 1.1.5 `fcntl_lock` (process-associated `F_SETLK`).
- Anthropic sandbox-runtime (`anthropics/sandbox-runtime`, main, pushed 2026-09-25): [linux-sandbox-utils.ts](https://github.com/anthropics/sandbox-runtime/blob/main/src/sandbox/linux-sandbox-utils.ts), [macos-sandbox-utils.ts](https://github.com/anthropics/sandbox-runtime/blob/main/src/sandbox/macos-sandbox-utils.ts), [windows-sandbox-utils.ts](https://github.com/anthropics/sandbox-runtime/blob/main/src/sandbox/windows-sandbox-utils.ts), [seccomp-unix-block.c](https://github.com/anthropics/sandbox-runtime/blob/main/vendor/seccomp-src/seccomp-unix-block.c).

Documentation:
- Claude Code: [sandboxing](https://code.claude.com/docs/en/sandboxing), [settings-reference](https://code.claude.com/docs/en/settings-reference) (`allowUnixSockets`, `allowAllUnixSockets`, `enableWeakerNestedSandbox`), [env-vars](https://code.claude.com/docs/en/env-vars) (`CLAUDE_CODE_SESSION_ID`, `CLAUDE_CODE_CHILD_SESSION`), [tools-reference](https://code.claude.com/docs/en/tools-reference) (timeouts, background commands). All fetched 2026-09-26.
- MCP: [lifecycle, 2025-11-25](https://modelcontextprotocol.io/specification/2025-11-25/basic/lifecycle).

Third-party claims:
- LWN, [Add SCM_PIDFD and SO_PEERPIDFD](https://lwn.net/Articles/926312/).
- Beads [#2933](https://github.com/gastownhall/beads/issues/2933).
- claude-code issues [#84647](https://github.com/anthropics/claude-code/issues/84647), [#81462](https://github.com/anthropics/claude-code/issues/81462), [#82433](https://github.com/anthropics/claude-code/issues/82433), [#96625](https://github.com/anthropics/claude-code/issues/96625).
- WSL [#4676](https://github.com/microsoft/WSL/issues/4676) (`SIO_AF_UNIX_GETPEERPID`).
- FreeRADIUS [#5489](https://github.com/FreeRADIUS/freeradius-server/issues/5489) ("Linux and apparently OSX support F_OFD_SETLK").
- [Wikipedia: macOS Sonoma](https://en.wikipedia.org/wiki/MacOS_Sonoma) (xnu-10002 ↔ 14.0).

Measurements: `winprobe` (Rust, raw `kernel32` FFI) and `xlock` (sketch plus test). The probe scripts
and their raw output are not published.
