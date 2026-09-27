# 80 — Cross-platform design: one format and one protocol for Windows, Linux and macOS

*moirai design, 2026-09-26, **revision 2**. Status: design only; nothing is implemented. It turns the four cross-platform research reports [X17]–[X20] into the design of record's operating-system layer, following owner decision #32 of 2026-09-26. It is normative for the OS layer, the per-OS mappings, the M0 format and protocol items listed in §3, the shell rules of §4 and the port phase of §5. Revision 2 answers the adversarial review [81] (2 blockers, 7 majors, 15 minors): every finding is fixed, none is rejected, and §8 (Review log) records each disposition. §6 records the edits this revision applied to [AR], [40], [50] and [60] on 2026-09-26. Amended on 2026-09-26 for the owner's answers to [AR §11]: the repository is public (#36), so the port phase's hosted Linux runners are free, and there is no test host (#34), so floor-grade Linux defaults to a dual-boot partition of the owner's laptop (§5.4, §8.5). Amended on 2026-09-27 for the owner review of the approval checklist ([AR] binding inputs): the Windows OS-crash rig (GT15 and its calibration) is deferred to after the release and runs in the same post-release phase as the ports; until its calibration, Windows `file mv` keeps `MOVEFILE_WRITE_THROUGH` besides the directory flush (§2.3, §5.1, §5.4, §8.8).*

**Owner decision #32 — DECIDED 2026-09-26** (verbatim translations):
1. "Take into account that the system must work under macOS and Linux too."
2. "Don't build binaries for Linux and Mac for now, just take into account that this will need to be implemented. But we are not testing this for now — there is no possibility."

**What the decision means for the design.**
- moirai is designed now for all three operating systems. There is **one on-disk format** and **one protocol semantics**, and both map natively to Windows, Linux and macOS. They are frozen at M0, so the later ports never change the format or the protocol.
- OS differences live only behind the OS layer (§2). Complete per-OS mapping specifications are written now.
- Build, CI, test and crash gates stay **Windows-only** in M0–M11.
- The Linux and macOS implementations, binaries, runners and crash rigs form a later **port phase** (§5). It is documented with its gates and estimates. It is **not scheduled**, and it does not block the release.

**Inputs, read in full:** [X17] `research/17-xplat-durability-mmap-memory.md`, [X18] `research/18-xplat-locking-ipc-processes.md`, [X19] `research/19-xplat-file-identity-change-tracking.md`, [X20] `research/20-xplat-toolchain-shells-ci-crash-testing.md`, and the review [81] `design/81-cross-platform-critique.md`. **Also read:** [AR] `docs/ARCHITECTURE-RESEARCH.md` §0–§2, §4, §5a–§5e, §6, §7, §8, §9–§13 and the Review log; [40] `design/40-file-links-design.md` §0.1, §2.4, §2.6, §2.11, §3.4–§3.5, §4.3–§4.7, §8.3, §9.2; [50] `design/50-query-language-design.md` §2.2, §4.4, §6.1–§6.2, §8.1, §10; [60] `design/60-roadmap.md` §0–§3, §5–§8, §10; [74 A01].

**Evidence tags:**

| Tag | Meaning |
|---|---|
| [M] | measured on the owner's Windows machine, quoted from the cited report |
| [D] | vendor documentation, a man page or a specification, as quoted by the cited report |
| [S] | source code read by the cited report |
| [C] | third-party claim or measurement |
| [I] | inference, of the cited report or of this document |
| est. | arithmetic with its inputs shown |

- No Linux or macOS behaviour was measured by anyone: WSL is not installed, and no Mac is available [X17 §0.1], [X18], [X19 §1], [X20 §1].
- No web request was made while writing this document. **Untagged statements are design decisions.**

---

## 0. Summary

1. **One format, one protocol, three mappings.** Every byte on disk and every protocol rule is the same on all three OSes. OS differences are confined to one crate, `moirai-os`, with twelve modules behind the `Vfs` and `ProjectFs` seams (§2.1) and implementations chosen at compile time. Windows is built and gated in M0–M11. Linux and macOS are specified here and built in the unscheduled port phase.

2. **Locks.** Every OS provides the same lock model: an exclusive single-byte lock owned by one open handle or open file description (OFD), with **ownership decided in user space first**: a process-global table gives a second in-process client `Busy` (or an in-process wait) before any kernel call, so Windows' non-reentrant refusal and the OFD merge never happen and two clients in one process behave identically on every OS (§2.2).
   - Windows uses `LockFileEx`. Linux (≥ 3.15) and macOS use OFD locks (`F_OFD_*`).
   - **macOS 14 is the minimum**, the first release whose public SDK declares OFD locks; the binary is linked for macOS 14 and checks the version at open, so the private range 10.13–13 is never reached.
   - Timed waits: overlapped `LockFileEx` + `CancelIoEx` on Windows; a waiter thread in `F_OFD_SETLKW` on its own OFD on Unix. One fixed lock order; fairness is not part of the contract.

3. **Lock bytes move beyond EOF.** In `LOCK` layout v1 the role bytes sit at 2^62 + i and the liveness slots at 2^62 + 2^16 + i. No data byte of `LOCK` is ever locked (§2.2, §3 X-F1).

4. **Durability classes with exact per-OS calls** (§2.3): `lazy`; `durable` (`NtFlushBuffersFileEx(DATA_SYNC_ONLY)`, `fdatasync`, `fcntl(F_FULLFSYNC)`); `durable+meta`; `durable-name` (a directory entry); `sync_group`. An `ENOTSUP` or any other refusal is never downgraded: the store is refused. Any error from a non-lazy class aborts the process without an acknowledgement. `O_DIRECT` + `RWF_DSYNC` is rejected (it would need an aligned format). Every protocol point that renames uses a no-replace rename followed by `durable-name` on both parents, on every OS; replace-renames and directory swaps have their own operations.

5. **Group commit on every OS: leaderless, through a flush byte, acknowledged by identity** (§2.4). A writer appends under the writer byte and releases it. A flush holder takes the flush byte, then the writer byte to scan and re-write the unflushed range, releases the writer byte, flushes once for every writer that appended before, and publishes `HEAD` by a read-modify-write that folds every covered group's effects. Each group's checksum is **chained** to its predecessor's, and a writer acknowledges only after re-reading its own chained trailer behind the covering publish. There is no IPC, so it works for sandboxed agents.
   - **Why it is needed.** On Apple silicon `F_FULLFSYNC` costs 3–20 ms [C]; serial flushing inside the writer lock puts the 16-writer last acknowledgement at about 50–320 ms.
   - **Result.** A 16-writer burst costs about 2–3 flushes on every OS; the writer-byte hold is append plus re-validation, and for the flush holder a page-cache re-write of the pending range.
   - **Why chained and by identity** ([81] B1). After a failed flush the unflushed pages may revert, be invalidated or be evicted on btrfs, macOS, ext4 and XFS while other writers keep appending. Acknowledging by LSN position, or accepting a group behind a predecessor it never saw, could then acknowledge a lost commit or make two exclusive claims durable. The chain and the identity check make both impossible.
   - **Rejected:** a mandatory leader (sandboxed CLIs on Linux and macOS cannot reach it) and fully concurrent flushes (more flushes, weaker failure semantics).

6. **Mapping policy** (§2.5). Only sealed files are mapped, read-only and whole-file; sealed files are read-only on disk; each sealed file states its own length in its header, and the size is checked against it before mapping; every read through a mapping uses types for which every bit pattern is valid; a `SIGBUS` or `EXCEPTION_IN_PAGE_ERROR` inside a mapping prints one line and exits 7.

7. **Environment guard** (§2.6). A per-OS allow-list in which every allowed file system is crash-gated: NTFS on Windows (M1); ext4, XFS and btrfs on Linux and APFS on macOS (gated in the port). Everything else is refused with its name, including ReFS/Dev Drive, ZFS, f2fs, bcachefs and HFS+ until a gate covers them, every overlayfs, network and FUSE file systems, 9p (WSL2 `/mnt/*`), tmpfs, `\\wsl$`, and cloud-managed folders (OneDrive, iCloud Drive and File Provider roots, iCloud-managed `~/Desktop` and `~/Documents`). In M0–M11 NTFS is admitted on the GT1/GT3 fault-model evidence and GT4 on real NTFS; its GT15 variant runs with the OS-crash rig, deferred to after the release ([AR §10] risk 17). A store is touched by one kernel only.

8. **Liveness is anchored in locks** (§2.7). A session anchor is live when a held slot's record names the session; an intent anchor when a held slot's record carries the intent's nonce. This works across PID namespaces, sandboxes and PID reuse, and answers `Alive`, `Dead` or `Unknown`; `Unknown` never reclaims anything. `file mv` and `file rm` hold their own intent slot. **The boot identity is constant for one boot and invariant under clock changes, suspend and hibernation** (a per-boot counter on Windows, not the boot time, which moves when the clock is set, [81] M2); a process that cannot read it runs in a defined **Unknown-boot mode** instead of refusing the store ([81] M3).

9. **IPC** (§2.8, used only if the leader is ever built): a named pipe on Windows; a pathname Unix-domain socket in a 0700 per-user directory on Unix, never an abstract socket; the endpoint is published in `LOCK`; a sandboxed client uses the direct path, which is complete.

10. **Memory accounting per OS** (§2.9): `PeakPagefileUsage` (Windows), `ledger_phys_footprint_peak` (macOS, public Mach API), cgroup-v2 `memory.peak` with `smaps_rollup` at exit (Linux). Port gates are floor-relative; a portable `heap_peak` is reported beside them.

11. **Paths** (§2.10). Rules P1–P12 make a stored path, a derived uid and an image byte-identical on all three OSes: git's spelling for tracked files; NFC for untracked names on macOS, as git records them; `\`, control characters and non-UTF-8 names refused; names some OS cannot hold refused at creation; `PATHIDX` folds with `fold_v1 = NFD(full_casefold(NFD(x)))` at Unicode 17.0.0; the root is canonicalized per component with its on-disk names, and bindings are found by the root's file id first; hashed named-query file names; ref names that are portable and never fold-equal.

12. **R4 on three OSes** (§2.11). One resolver, fed with normalized evidence and a per-volume capability record. Windows: `FILE_ID_128`, the parent-directory id and `OpenFileById`. macOS: the file id, `fsgetpath` and `getattrlistbulk`. Linux: `(volume key, inode, file-handle digest)` — the handle carries the inode generation, so a reused inode number is a different object ([81] B2) — with E3 and E3d found by a changed-directory frontier. A missing source contributes nothing and never changes a rule. Two rules close cross-OS wrong binds: case and normalization **twins** from another OS never resolve on a spelling match alone ([81] M4), and an equal creation time is corroboration only where it is unique and cannot be copied ([81] M5). On every OS no link ever re-binds wrongly; what differs is how often a link is exact at read time rather than at the next settle (§2.11.5).

13. **Frozen at M0** (§3), items X-F1–X-F12: `LOCK` v1; anchors, `ProcId`, the boot-identity rule and Unknown-boot mode; the lease deadline clock; group commit with chained group validity; the lock contract; the durability-class mapping and the fault-model amendments; the mapping policy with self-describing sealed-file lengths and the environment guard; the path-key canonical form; the tagged R4 runtime layouts and resolver rules; named-query file names and the ref-name rule; the store-layout rule; the per-OS config locations (their keys are registered in [AR §13], not frozen); the shell transport rules. §3.2 lists what was checked and needs no change.

14. **Shells** (§4). Rules T1–T10 hold in bash, zsh, dash, fish, PowerShell and cmd: bare ids in argv; no token starts with `#`, `~`, `=`, `@` or `!`; no glob or brace characters in argv; free text on stdin from a quoted heredoc (agents on Linux and macOS never write query files); UTF-8 with LF, byte-identical on every OS apart from defined golden-file substitutions; hooks in exec form with an absolute path.

15. **The port phase** (§5) is unscheduled, with its gates listed there; the Windows OS-crash rig that the owner review of 2026-09-27 deferred to after the release runs in the same post-release phase (§5.1). Size ≈ 52–83 units, about 6.5–16.5 weeks with one lane. Prerequisites: the Windows release gate, hosted Linux CI with `/dev/kvm` (free, because the repository is public, [AR §11] #36, decided 2026-09-26) or self-hosted, an Apple-silicon Mac, and the owner's one new decision, **#42 (port-phase hardware, CI and platform coverage — money)**. Designing for three OSes adds ≈ 9.5–16.5 units to M0–M11. Two guards against Windows-only code build no binary and run no test for another OS: the OS-layer lint GT20 (d), a gate from M1, and — by owner decision #44 — the cross-target type check GT20 (e), a gate from M0 in the local pre-merge gate and PR CI, which also makes every dependency pure Rust (§5.5, [90 §11]).

---

## 1. Principles

| # | Principle | Consequence |
|---|---|---|
| X1 | **One on-disk format.** Every file, record, section, image object and runtime row has one byte layout. That layout is little-endian and 64-bit, and it does not depend on page size or path separator. OS-specific runtime data carries an OS **kind tag**; a row whose tag this OS cannot interpret is ignored, as if absent [X19 §8.1]. | A backup restores on another OS, an image imports anywhere, and a store written on one OS opens on another (where the environment guard admits it) as if it had its first settle. Big-endian and 32-bit targets are unsupported. |
| X2 | **One protocol semantics.** The commit, lock, publish, recovery, liveness and acknowledgement rules are the same on every OS. They are written against the **weakest** OS: explicit directory durability, failed flushes that leave old or new bytes (and let pages revert or vanish), truncation of mapped files, lock-release lag of any length, inode numbers that are reused. | Everything that differs sits behind the OS layer (§2). The fault model the simulator enforces is the weakest of the three (§2.3.5), so the Windows-hosted GT1 and GT3 exercise Unix failure semantics from M1 on. |
| X3 | **OS differences live only behind the OS layer.** Only `moirai-os` may use `cfg(target_os)`, `std::os::windows`, `std::os::unix`, `windows-sys` or `libc`. Every other crate is target-independent. | Enforced by a structural lint (GT20 d, §5.5), which runs on Windows and builds nothing for another OS. |
| X4 | **Implementations are selected at compile time, never swapped.** Each target has exactly one implementation of each seam. The `Vfs` and `ProjectFs` traits exist because each has a simulator ([60] P1). | The commit path has no dynamic dispatch. A port adds implementations of a frozen contract, never a second implementation of one concern for one target. |
| X5 | **Nothing is silently weakened on any OS.** Where an OS cannot give a guarantee, the store is refused (durability, locks, environment) or the answer is marked (`unverified`, `Unknown`, Unknown-boot mode). A missing evidence source contributes nothing and never changes a rule. | There is no `F_BARRIERFSYNC` class, no fallback to `fsync` on `ENOTSUP`, no PID-based expiry, no flock fallback, no ungated file system and no guessing. Every per-OS difference in what users see is listed (§2.11.5, §2.13, [AR §8.3]'s per-OS notes). |
| X6 | **The weakest client defines the contract.** A client may run in another PID or user namespace (Linux bubblewrap), be confined to its own sandbox (macOS Seatbelt), run as a different principal (Windows `srt-win`), or be unable to open sockets or read some system identifiers [X18 §7.2], [X20 §3.3–§3.4]. | The direct path is complete, the leader is only an optimisation, liveness can answer `Unknown`, and an unreadable boot identity degrades to Unknown-boot mode. |
| X7 | **Windows is implemented and gated in M0–M11; Linux and macOS are specified now and ported later.** | M0 freezes the three-OS format and protocol. M1–M11 build and gate the Windows implementation. The port phase (§5) adds the Linux and macOS implementations, with their own gates, and never reopens M0. |
| X8 | **Ports never change format or protocol.** A port finding that needs a format or protocol change is a specification defect. It follows [60] P4: M0 reopens, and every passed milestone re-runs its gates. | This is why every per-OS item that shapes bytes or rules is frozen now (§3), including items only Linux or macOS use (`DIRMAP`, the Linux handle digest, the macOS `docid` field, `JOURNALCUR`'s FSEvents kind, the Unix endpoint record). |
| X9 | **Public, documented interfaces only in product code** ([81] M3). A private or undocumented interface may be read only where its absence degrades to a defined mode; it never gates correctness. | macOS 10.13–13's private OFD constants are not used; libproc is used only by the measure harness; the one undocumented read, macOS `kern.bootsessionuuid`, falls back to Unknown-boot mode (§2.7.1). |

---

## 2. The OS abstraction layer

### 2.1 Modules and boundary

The crate `moirai-os` is the only place where code differs by target. It has one submodule per target family (`windows/`, `unix/`, `linux/`, `macos/`, where `unix/` holds the code Linux and macOS share) and a target-independent part (the in-process lock table, the evidence records). Everything above it is target-independent.

| Module | Surface (operations; names are the contract, signatures are fixed in the M0 specification) | Seam | Sections |
|---|---|---|---|
| `os::fs` | `open_store_file`, `read_at`, `write_at`, `sync(Data \| DataAndMeta)`, `sync_dir`, `sync_group`, `create_extent`, `seal`, `unlink`, `rename_noreplace`, `rename_replace`, `swap_dirs`, `file_size`, `identity(file)` | `Vfs` | §2.3 |
| `os::lock` | `LockFile::open`, `try_acquire(byte)`, `acquire_within(byte, T) → Granted \| Busy`, `probe(byte) → Held \| Free \| Unknown`, `release`; the process-global grant table | `Vfs` | §2.2 |
| `os::map` | `map_sealed(file, expected_len)`, `unmap`; the process-global mapping registry; the fault handler | `Vfs` | §2.5 |
| `os::env` | `classify(store_dir) → Local{fs, caps} \| Refused{reason}`; `probe_store(dir)` at `init`; the OS-version check | `Vfs` | §2.6, §2.13 |
| `os::proc` | `ProcId` of self and parent; `boot_id() → Known(16 B) \| Unknown`; `boot_clock_ns()` (monotonic, includes suspend); `alive(ProcId) → Alive \| Dead \| Unknown` (diagnostics only); `watch_parent()` | injected | §2.7 |
| `os::ipc` | `bind_endpoint`, `connect`, peer credentials (leader only) | — | §2.8 |
| `os::mem` | `private_peak()`, `private_now()`, idle-CPU and context-switch counters (measure builds and the harness) | — | §2.9 |
| `os::path` | conversion between OS path and stored path (UTF-16 ↔ UTF-8 on Windows, bytes ↔ UTF-8 on Unix); `canonical_root` (per component, on-disk names); checks for reserved and unrepresentable names | `ProjectFs` | §2.10 |
| `os::project` | `volume(dir) → (VolumeKey, VolumeCaps)`, `stat`, `enumerate`, `locate_id`, `file_handle_digest`, `journal_since`, `case_equivalent`, `read_for_hash`, `rename_noreplace`, `trash_dirs`, `durable_rename`, `durable_unlink` | `ProjectFs` | §2.11 |
| `os::spawn` | the detached low-priority `moirai gc` child (never from a foreign PID namespace); memory and I/O priority for bulk passes; no inheritable handles | — | §2.12 |
| `os::term` | console versus pipe detection; UTF-8 output (`WriteConsoleW` on a Windows console); a broken pipe exits quietly with 0 | — | §2.12, §4 |
| `os::test_host` (test builds only) | `kill`, `suspend`, `resume`, `small_volume(bytes)`, clock offset | — | §5.2 |

**Boundary rules.**
- **Allowed dependencies.** `windows-sys` and `libc` are declared as direct dependencies only by `moirai-os`, as [AR §2.10] already allows them (third-party crates may reach them transitively only through the reviewed allow-list of GT20 d, §5.5); the binary crate carries only the process entry glue; `std::fs` and `std::process` above the layer only for operations whose semantics are identical on all OSes (§2.12). Every store file and every project file goes through `Vfs` or `ProjectFs`.
- **Never used anywhere:** `File::lock` (it is `flock` on Unix, which conflicts with OFD locks on macOS and is invisible to them on Linux [X18 §2.4–§2.5]) and `std::fs::rename` (on Unix it silently replaces an existing destination [X19 §8.6]; `rename_replace` is the explicit form).
- **Threads.** The layer adds one kind of thread, only on Unix: a lock-waiter thread with a 64 KiB stack [I]. It exists while a contended acquisition is pending; an **abandoned** waiter (its caller timed out) stays parked until it obtains the byte, which it then releases at once, so it lives at most as long as another process holds that role byte — seconds for the writer and flush bytes; the maintenance byte is never waited on ([81] m5). This clarifies [AR §2.2]'s "no thread outside a command", like [40 §4.8]'s scoped worker threads; the idle rule counts such a waiter while it exists. At idle there are no timers and no threads beyond the MCP server's stdin reader.

### 2.2 `LockBytes`

#### 2.2.1 Contract (identical on every OS)

1. **Lock bytes carry no data.** Every lock byte lies beyond the end of `LOCK` (at 2^62 and above), so Windows' mandatory byte-range locking never blocks a read of real bytes. Every lock is one byte and exclusive [X18 §6.1 items 1, 3].
2. **Ownership is decided in user space first** ([81] M1). A process-global grant table in `moirai-os` records, per lock byte, the client instance (the `LockFile` value) that holds it and the in-process clients that wait for it.
   - A second in-process client asking for a byte this process holds gets `Busy` from `try_acquire`, or waits in-process for the holder's release from `acquire_within`, **before any kernel call**, identically on every OS. The kernel therefore never sees two in-process acquisitions of one byte: Windows' non-reentrant refusal and the OFD merge (whose `F_UNLCK` would drop the other client's grant) never occur.
   - The kernel lock is taken on one handle or OFD per role per process, opened lazily and reused across grants (a Windows open is costly); every slot grant has its own handle or OFD.
   - Release always unlocks in the kernel; in-process waiters then compete through a fresh kernel acquisition like any other process.
   - **Waiter joining** (Unix): at most one kernel waiter thread per role per process; a grant it obtains is handed to exactly one waiting client (the oldest in the in-process queue); the others keep waiting on the table.
   - Two clients inside one process therefore conflict identically on every OS, so the simulator and tests run several clients in one process; M1's `Vfs` conformance suite has an in-process two-client case.
3. **Non-reentrant.** A client acquiring a byte it already holds is a programming error, asserted in user space.
4. **Fixed lock order** (§2.2.3). Neither OFD nor `LockFileEx` detects deadlocks.
5. **`acquire_within(byte, T)`** returns `Granted` or `Busy`, never both. A grant that races the deadline is either returned or released.
6. **`probe(byte)`** returns `Held`, `Free` or `Unknown`. The kernel never reveals the holder: `l_pid` is −1 for OFD, and Windows has no query. Holder identity comes only from moirai's records in `LOCK`. An `EPERM`, `EACCES` or `EBADF` from the probe (a sandbox rule, a read-only descriptor) is `Unknown`, never `Free` ([81] m12).
7. **No spawn while holding a role byte.** Every descriptor is `O_CLOEXEC` or non-inheritable, the Rust std default. An inherited OFD would keep a lock alive in a child that outlives its parent [X18 §6.1 item 7].
8. **Never `flock` or `File::lock` a store file.** `doctor` warns when `LOCK` is flock-held; on Unix it tests this with `flock(LOCK_NB)`.
9. **`LOCK` identity.** `LOCK` is created only by `init` or `restore` (`O_CREAT | O_EXCL`, or `CREATE_NEW`) and never deleted by moirai. After opening it, a process compares the handle's identity with a fresh stat of the path: `(st_dev, st_ino)` on Unix, `FileIdInfo` on Windows. A mismatch means the file was replaced: close and retry once, then refuse with exit 7 (the Beads #2933 class [X18 §4, §6.1 item 9]).
10. **Fairness is not part of the contract** ([81] m5). Windows grants roughly in arrival order; Unix wakes every waiter at once [S, X18 §2.3]. The only guarantees are the bound of item 5 and exit 7 naming the holder; the port probes measure 16-writer fairness, and a spurious timeout is an exit 7 the caller retries.

#### 2.2.2 Per-OS mapping

| Contract item | Windows 11 | Linux ≥ 3.15, 64-bit | macOS ≥ 14 |
|---|---|---|---|
| Open `LOCK` | `CreateFileW(GENERIC_READ\|GENERIC_WRITE, FILE_SHARE_READ\|WRITE\|DELETE, OPEN_EXISTING, FILE_FLAG_OVERLAPPED)`, once per role and per slot grant | `open(O_RDWR\|O_CLOEXEC)`; `F_WRLCK` needs write access (`EBADF` otherwise) | as Linux; XNU checks `FWRITE` for `F_WRLCK` [S, X18 §2.3] |
| Try-acquire byte b (after the grant table) | `LockFileEx(EXCLUSIVE\|FAIL_IMMEDIATELY, off b, len 1)`; error 33 means busy | `fcntl(F_OFD_SETLK, {F_WRLCK, SEEK_SET, b, 1, l_pid 0})`; `EAGAIN`/`EACCES` mean busy | `fcntl(F_OFD_SETLK = 90, …)` from the public SDK; same errors |
| Bounded wait | overlapped `LockFileEx`, `WaitForSingleObject(T)`, `CancelIoEx`, `GetOverlappedResult(TRUE)` to settle a grant that raced the cancel (`Granted`, or 995 → `Busy`) [M, X18 §5] | **waiter thread** blocked in `F_OFD_SETLKW` on its own OFD (retrying on `EINTR`); the caller waits on a condvar until the deadline, with hand-off under a mutex; an abandoned waiter's grant is released at once [X18 §3] | as Linux. `F_OFD_SETLKWTIMEOUT` (93) is **not** used: its bound applies to each sleep inside the kernel's retry loop [S, X18 §2.3] |
| Release | `UnlockFile(b, 1)` | `F_OFD_SETLK` with `F_UNLCK` | same |
| Probe | try-lock and `UnlockFile` through a dedicated probe handle (it may hold the byte for a few µs; harmless) | `F_OFD_GETLK` on an `O_RDWR` descriptor; `l_type != F_UNLCK` means held | `F_OFD_GETLK` (92) |
| Release after the holder dies | by the OS; ≤ 32 ms observed after `TerminateProcess` (p99 1–8 ms) [M, X18 §5], but unbounded while a crash reporter holds a crashing process | when the last reference to the OFD closes at process exit; unbounded while `systemd-coredump` or a debugger holds the process | same; ReportCrash can hold a crashing process for seconds [I] |
| Unsupported | `DRIVE_REMOTE` or a UNC path → refused by the guard (§2.6) | `EINVAL`, `ENOTSUP` or `EOPNOTSUPP` from `F_OFD_*` → store refused (exit 7) | same, plus volumes without `MNT_LOCAL` |

- **macOS version (decided).** The minimum is **macOS 14 Sonoma**, the first release whose public SDK declares `F_OFD_SETLK`, `F_OFD_SETLKW` and `F_OFD_GETLK` and whose `fcntl(2)` documents them (xnu-10002.1.13) [S, X18 §2.3]. XNU has implemented OFD locks since macOS 10.13, behind `#ifdef PRIVATE` up to macOS 13; that range is **not used** (X9). Because the `init` probe would pass on macOS 13 too, the floor is enforced twice: the binary is linked with a minimum OS version of 14 (`MACOSX_DEPLOYMENT_TARGET=14.0`, where Rust's default is 11 [D, X20 §4.1]), and `os::env` checks `kern.osproductversion` at open and refuses an older system with exit 7 ([81] m15).
- **Linux version.** OFD locks have existed since Linux 3.15 and are standardised in POSIX.1-2024 [D]. The supported floor is set in §2.13. Only 64-bit targets are supported, which avoids glibc's 32-bit `off_t` problem at 2^62 [S, X18 §2.2].
- **Precedent.** redb uses this model (`F_OFD_SETLK` on Linux and Apple, `LockFileEx` on Windows) for its multi-process protocol; that code is new and not yet battle-tested [S, X18 §4]. It is cited as a precedent only; no third-party database is used anywhere.

#### 2.2.3 The lock map and lock order

| Byte (offset in `LOCK`) | Role | Held by | Acquired with |
|---|---|---|---|
| 2^62 + 0 | **writer** | the appender (scan, re-validate, append), the flush holder (scan and re-write the pending range; publish), every publisher | blocking wait, `lock.writer-wait-ms` |
| 2^62 + 1 | leader | the optional leader, for its lifetime (only if built) | try |
| 2^62 + 2 | maintenance | checkpoint, promotion, rollup, GC | try (maintenance never waits for it) |
| 2^62 + 3 | quiet-advisory | as in [AR §4.1] | probe or try only |
| 2^62 + 4 | **flush** (§2.4) | the process that flushes the log and publishes `HEAD` for everyone; boot-change recovery | blocking wait, `lock.flush-wait-ms` |
| 2^62 + 5 … +63 | reserved | — | — |
| 2^62 + 2^16 + i, i < 256 | **liveness slot i** | a session's MCP server (Codex: a thread's) from the moment it knows that identity until it exits — at start, or lazily at its first call carrying `_meta.threadId` ([90 §4.4]) — or a CLI for the life of one file intent (§2.7) | try only; a busy slot moves to the next |

**Total order:** slot < leader < maintenance < flush < writer.
- A process may **wait** only on a role byte, and only if it holds no byte of equal or higher order.
- Slots are only ever try-locked, and the quiet byte only probed.
- The writer byte is the innermost. Nobody holding it waits for any lock or flushes: the appender releases it before it waits for the flush byte, and the flush holder releases it before it flushes (§2.4).

### 2.3 Durability classes

#### 2.3.1 Classes, guarantees and calls

| Class | Guarantee (identical on every OS) | Windows | Linux | macOS |
|---|---|---|---|---|
| `lazy` | visible to every process after its publish; survives a process crash; may be lost after an OS crash, a power loss or **a failed flush in any process** (§2.3.5 item 3); becomes durable at the next covering flush | `WriteFile` at an offset | `pwrite` | `pwrite` |
| `durable` | acknowledged only after the bytes are on stable media, assuming the drive honours FLUSH | `NtFlushBuffersFileEx(FLUSH_FLAGS_FILE_DATA_SYNC_ONLY)` | `fdatasync` | `fcntl(F_FULLFSYNC)`, never plain `fsync` (data stays in the drive cache [D]) and never `F_BARRIERFSYNC` (ordering only [D]) |
| `durable+meta` | as `durable`, plus the file's size and allocation | `FlushFileBuffers` | `fsync` | `fcntl(F_FULLFSYNC)` |
| `durable-name` | a create, rename or unlink in a directory survives power loss | `FlushFileBuffers` on a directory handle opened with `FILE_FLAG_BACKUP_SEMANTICS` and `GENERIC_WRITE` (0.072 ms p50 [M, X17 §3.4]); `MOVEFILE_WRITE_THROUGH` on the rename is also used until the rig calibration (item 17, deferred to after the release by the owner review of 2026-09-27) shows it unnecessary; the directory barrier is never skipped | `fsync(open(dir, O_RDONLY\|O_DIRECTORY))`; on ext4 this forces a full journal commit [S, X17 §3.4] | `fsync(dirfd)`, then a device barrier: `F_FULLFSYNC` on the dirfd, or folded into the group's last `F_FULLFSYNC` |
| `sync_group(members)` | every listed file and directory is durable when it returns | flush each member by its class | flush each member by its class (each is a device flush) | plain `fsync` of each member, then **one** `F_FULLFSYNC` last on a member of the same volume; the man page guarantees it persists everything `fsync`'d on that device before it [D, X17 §3.8.1] |

- **No `ordered` class.** Only macOS has a cheap ordering-only primitive (`F_BARRIERFSYNC`). The protocol does not need one, and offering it would break X2 [X17 §3.8.1].
- **No direct-I/O path** ([81] m3). `O_DIRECT` + `RWF_DSYNC` (a FUA write) is **rejected**: it needs block-aligned offsets and lengths, which groups do not have. Admitting it would require padding groups to alignment (a format change) or a read-modify-write of the tail block that races the flush holder's re-write; both break X8.
- **One error policy.**
  - Any error from any class other than `lazy` aborts the process without an acknowledgement: `EIO`, `ENOSPC`, `EDQUOT`, `EROFS`, `ENOTSUP`, and the Windows equivalents including `ERROR_DISK_FULL`. The flush is never retried on the same handle.
  - The next flush holder repairs the log by re-writing the unflushed range (§2.4).
  - Go and SQLite silently degrade `F_FULLFSYNC` to `fsync` on `ENOTSUP` [X17 §3.3]; moirai **refuses**: at `init` and `restore` through the probe of §2.6, at every open through the classification, and at run time by aborting with exit 7, "location cannot provide durable commits".
- **Windows `FILE_FLAG_WRITE_THROUGH`** is never used as durability [AR §2.8].

#### 2.3.2 Which protocol point uses which class (frozen with the protocol)

| Protocol point | Class | Notes |
|---|---|---|
| Commit group (every durable record kind) | `durable` on each log extent holding bytes of the flushed range, issued by the flush holder (§2.4) | lazy groups are flushed by the next covering flush |
| Log extent creation and rotation | `create_extent` (§2.3.3), then `durable+meta` on the extent and `durable-name` on the store directory, before the first commit in the extent is acknowledged; the old extent is padded with one `Noop` group ending at its last byte (§2.4.3) | on macOS one `sync_group` |
| Segment, `hist`, `blobs`, `cs.NNNN`, `gitmap` page, `dict` | write, `durable+meta`, `seal` (§2.5), then `durable-name` on the store directory, **before** the `Checkpoint` or commit that names the file is appended | created under its final monotonic number, so no rename is needed; a temporary name followed by a rename is allowed only with `durable-name` after the rename [X17 §3.8.4] — the form a bulk commit's `cs.NNNN` uses, the one rename on the commit path ([AR §4.3, §4.10]) |
| `HEAD` publish | none on the commit path (1PC+C) | a read-modify-write under the writer byte (§2.4.3) |
| `HEAD` barrier before a deletion, retirement or recycling | after maintenance's own `Checkpoint` passed its identity check: a no-op publish under the writer byte if the other slot does not yet name the post-change set, then `durable+meta` on `HEAD` **outside the writer byte**; then `unlink`, then `durable-name` | one flush, never under the writer byte (a macOS flush costs 3–20 ms). Correct under the fault model because both on-disk slots then name post-change states, every later publish is a read-modify-write of the newest slot, and at most one slot can be torn (§2.3.5) |
| Boot-change recovery | `durable` on the log, then `durable+meta` on `HEAD`, once per boot | [AR §4.2]; flush byte, then writer byte (§2.4.3) |
| `FsIntent` | a durable group of its own | unchanged |
| `file mv` rename of a project file | **every OS:** a no-replace rename, then `durable-name` on **both** parent directories, before the commit that records the move ([81] m1). Windows: `MoveFileExW` without `REPLACE_EXISTING` (with `MOVEFILE_WRITE_THROUGH` until the post-release rig calibration shows it unnecessary) and the directory flush; Linux: `renameat2(RENAME_NOREPLACE)`; macOS: `renamex_np(RENAME_EXCL)`; macOS: one `sync_group` | Linux's `EINVAL` fallback `link` + `unlink` (files only) leaves the crash state "both names, one inode", which the `FsIntent` recovery rolls forward; a macOS volume without `VOL_CAP_INT_RENAME_EXCL` refuses `file mv` (exit 7) ([81] m2) |
| `file rm` and `--trash` | `unlink` or a no-replace rename into `trash/`, then `durable-name` of the parent(s) | before the commit |
| Store `config` rewrite | the new text written to `<store>/tmp/`, `durable+meta`, then `rename_replace` onto `config`, then `durable-name` | `rename_replace` = `MoveFileExW(REPLACE_EXISTING)` / `renameat` ([81] m2) |
| `restore` swap of the store directory | `swap_dirs` under the writer and maintenance bytes: `renameat2(RENAME_EXCHANGE)` on Linux, `renamex_np(RENAME_SWAP)` on macOS; on Windows, and wherever the exchange is unsupported, two renames guarded by an intent file in the parent directory that `doctor` completes or rolls back; each step followed by `durable-name` | [AR §4.10] |
| `backup` | every copied file `durable+meta`, the backup directory `durable-name`, then the durable `Backup` record | a backup never uses reflink or `clonefile` (it must not share blocks with the store) [I] |
| Image export | pack and idx `durable+meta`, a no-replace rename into `objects/pack/` with `durable-name`, the `packed-refs.lock` file `durable+meta`, `rename_replace` onto `packed-refs` (git's lock protocol; loose `<ref>.lock` → `<ref>` likewise) and `durable-name`, and only then the `gitmap` commit | [AR §5b.6 step 5] |

#### 2.3.3 Log extent creation (G11) per file system

- **The format invariant** is the same everywhere: an extent is exactly `store.log-extent-bytes` long, and every byte beyond the durable tail reads as zero [X17 §3.6]. Recovery scans up to the first group whose length, checksum, epoch, position or chain fails.
- **How** the zeros arise is chosen by `create_extent` according to the file system:

| File system | Method | Why |
|---|---|---|
| NTFS (gated) | write zeros, then `FlushFileBuffers` (as today) | every later commit is a pure overwrite, and the valid data length is already advanced |
| ext4, XFS (port) | `fallocate(FALLOC_FL_WRITE_ZEROES)` where the kernel has it (ext4 from 6.17, XFS from 7.3 [C]), else write zeros; then `fsync` and `sync_dir` | plain `fallocate` leaves unwritten extents, which cost about 5× in O_DSYNC overwrite IOPS [C, X17 §3.6] |
| btrfs (port) | a sparse `ftruncate`; the port measures whether `FS_NOCOW_FL` on the empty file pays (moirai has its own checksums) | copy-on-write: an overwrite always allocates |
| APFS (port) | a sparse `ftruncate`; `F_PREALLOCATE` is optional | copy-on-write; zero-fill buys nothing [D] |

- **On copy-on-write file systems** any write and any flush may fail with `ENOSPC` (fault-model item 5).
- **Early warning:** a rotation onto a sparse extent requires `statvfs` to show at least 2 × the extent size free; otherwise it refuses with exit 7 [X17 §3.6].

#### 2.3.4 Flush failure

- **Behaviour after a failed flush differs by OS** [D, ATC'20 via X17 §3.5]: ext4 and XFS keep the new bytes in pages now marked clean, which memory pressure — or moirai's own `POSIX_FADV_DONTNEED` in a concurrent bulk pass — can evict; btrfs reverts the pages to their old content; macOS invalidates the buffers, and a later call may report success; Windows is unmeasured (M0 item 18).
- **One rule works on all of them:** a flush failure aborts the process; the next flush holder **re-writes** every byte of the unflushed range from what it reads, under the writer byte, before it flushes; the chain rule makes whatever it reads either the true log or a shorter valid prefix; and no writer acknowledges without its identity check (§2.4). This is protocol decision (a) of [60 §2.5], generalised from adoption to every flush.
- **Rejected:** verifying bytes through an unbuffered read is not portable: `O_DIRECT` needs alignment, and macOS `F_NOCACHE` is only a hint [X17 §3.5].

#### 2.3.5 Fault-model amendments (frozen at M0; the weakest of the three OSes)

These replace or add items in [60 §2.5]'s fault model.

- **(2), replaced.** `sync(Data)` makes durable the file's data within its current size; `sync(DataAndMeta)` also its size and allocation. **A create, rename or unlink becomes durable only after `sync_dir` of its parent; before that, any subset of the unsynced metadata operations may be lost, in any order.** The NTFS "prefix of issue order" rule is dropped: it does not hold on btrfs or on ext4 with fast-commit [I, X17 §3.4]. `durable-name` and `sync_group` follow §2.3.1.
- **(3), widened.** After a failed flush, reads of the range — and of any unflushed range of the same file — may return old **or** new bytes, may change between reads (a revert, an invalidation, an eviction of clean-but-unwritten pages), and a later successful flush proves nothing about them. Lazy records published beyond `durable_lsn` may vanish with them.
- **(5), widened.** **Any** write and **any** flush may fail with disk-full, including an overwrite of a written or zero-filled range, as on copy-on-write file systems.
- **(7), extended.** Besides the wall clock and a monotonic clock that never goes backward, the model has a **boot clock**, monotonic and including time spent in suspend, used for lease deadlines (§2.7.1); and a **boot identity** that may be unreadable to a process (Unknown-boot mode).
- **(8), widened** ([81] m5). Lock release after process death may be delayed by an **unbounded** amount on every OS: a crash reporter (Windows Error Reporting, systemd-coredump, macOS ReportCrash) or a debugger keeps a crashing process and its handles or OFDs alive for seconds. The Windows lag after `TerminateProcess` was ≤ 32 ms [M]. The simulator draws from the measured distribution plus a heavy tail beyond the 2 s bound.
- **(9), new.** A read through a mapping of a sealed file may terminate the process, if the file was truncated externally or the medium failed. The simulator treats this as a process crash at that point.
- **(10), new.** Another process with permission may truncate or rewrite a store file; Unix has no share modes. Sealed files are read-only on disk, so this requires deliberate action. The simulator injects an external truncation of a sealed file.
- **(11), new.** Several processes may have appended groups that no flush has yet covered; the process that appended a group may die before any flush; the flush holder may die before, during or after its flush, with or without an error; and a failed flush may be followed by reverted, invalidated or evicted pages while other processes keep appending and retrying (the group-commit crash surface of §2.4).
- **(12), new** ([81] m6). A read may fail (`EIO`; a btrfs or ZFS checksum error; a failing medium). An unreadable range below `durable_lsn` is corruption: exit 7 and `moirai repair`. Above `durable_lsn` it ends the log under the chain rule: the groups there were never acknowledged, or their writers fail the identity check. The simulator injects read errors.

### 2.4 Group commit: leaderless, through a flush byte, acknowledged by identity

#### 2.4.1 Why it must be decided at M0 for every OS

- As [AR §4.5] was written, each writer held the writer byte through append, flush and publish, so the last acknowledgement of a 16-writer burst costs about 16 flush times.
- With the 3.1–3.9 ms p50 and 11.6 ms p99 of `F_FULLFSYNC` on M3 and M5 machines, and about 17–20 ms on M1, that is about 50–65 ms at p50 and about 290–320 ms on M1. The 50 ms gate fails, and T8's own revisit trigger ("sustained flush p99 > 10 ms") already fires on a current MacBook Pro [C, X17 §1 item 2, §3.9].
- The protocol is frozen at M0 and identical on every OS, so the choice cannot wait for a Windows measurement.

#### 2.4.2 Options, with evidence

| Option | Serves sandboxed agents' CLIs | IPC needed | Device flushes per 16-writer burst | macOS M3/M5 last-ack (est.) | Failure semantics | Complexity |
|---|---|---|---|---|---|---|
| A. Serial flush inside the writer byte (the former [AR §4.5]) | yes | none | 16 | ≈ 50–65 ms p50, p99 far above; M1 ≈ 290–320 ms [C, X17 §3.9] | simple: the failing writer dies holding the byte, and the next writer adopts | lowest |
| B. Mandatory leader group commit | **no**: on Linux, seccomp makes `socket(AF_UNIX)` return `EPERM` inside the sandbox; Seatbelt denies Unix sockets unless allow-listed; a future Windows `srt-win` principal fails the pipe DACL [S/D, X18 §7.2], [X20 §3.3–§3.4]; those clients fall back to A | pipe or Unix socket, failover, forwarding keys | 1–2 for forwarded clients, 16 for direct ones | fails for exactly the sandboxed agents on macOS | a leader death leaves forwarded writes with an unknown outcome | highest |
| C. Leaderless, concurrent flushes outside the lock | yes | none | up to 16 concurrent | ≈ 10.7 ms median with 12 writers [C, pnpm, X17 §3.2] | a flush that succeeds after another process's failed flush may leave that range lost on macOS | medium |
| **D. Leaderless, flush byte, chained groups, acknowledged by identity (chosen)** | **yes** | **none** | **≈ 2–3** | ≈ 2 × flush p50 ≈ 8 ms p50 and ≈ 2–3 × p99 ≈ 25–35 ms p99; M1 ≈ 40–60 ms (est.) | one flush in flight at a time; the next holder re-writes the unflushed range under the writer byte; a group lost after a failed flush is never acknowledged (identity check) and never resurrected behind the wrong predecessor (chain) | medium |

**Choice: D.**
- It is the only option that coalesces flushes for every client with no IPC, and it issues the fewest device flushes, which matters most on macOS and is gentler on consumer SSDs.
- It keeps "exactly one flush in flight", so the failure argument is the adoption argument the design already uses.
- It shortens the writer-byte hold on every OS to scan, re-validation and append (tens of µs), plus the flush holder's page-cache re-write of the pending range.
- It is PostgreSQL's group-commit shape (`XLogFlush`: a backend whose LSN a concurrent flush already covered returns without flushing [X17 §3.9 option 2]), transposed to byte-range locks.
- **Why a later flush covers earlier appends on all three OSes.** `NtFlushBuffersFileEx`, `fdatasync` and `F_FULLFSYNC` each flush the whole file (and `F_FULLFSYNC` the device queue), so a flush that starts after a group's bytes are in the page cache covers that group, whichever process wrote them [D, X17 §3.9].
- **Why positions alone are not enough** ([81] B1). Group commit creates a state the serial protocol never had: live writers' groups sit unflushed while others append behind them. After a failed flush those pages may revert (btrfs), be invalidated (macOS) or be evicted as clean pages that were never written (ext4, XFS; also through moirai's own `POSIX_FADV_DONTNEED` in a concurrent bulk pass). Three failures follow unless the protocol prevents them: an acknowledgement by LSN position covers a commit that is no longer in the log (B1-a); a flush holder re-writes an old snapshot over a group appended behind a hole, and a group validated against that group survives behind a predecessor it never saw (B1-b: two durable claims of one task); a same-length group refills a hole and resurrects a later group behind the wrong predecessor (B1-c). The chain, the identity check and scan-and-re-write under the writer byte close all three.
- **Leader.** The optional leader keeps its Windows-driven trigger (the Defender close cost, M0 item 1). If ever built, its commits use this same protocol.

#### 2.4.3 The protocol (frozen at M0; replaces [AR §4.5] steps 5–10 and the publish of step 12)

**State and format.**
- `HEAD` keeps `committed_lsn` (the bound readers use) and `durable_lsn` (the end of the last successful flush) [AR §4.2]. **No `HEAD` field is added.**
- **Chained group validity** (a format item; it changes [60 §2.5]'s Log row). The record that carries `group_end` ends with an 8-byte trailer, `chain` = XXH3-64 over every byte of the group before the trailer, **seeded with the chain value that precedes the group**. The chain value at a group boundary p is the 8 bytes at [p − 8, p), so it is read by position. A group is valid only if every record is valid by the existing rule (length, kind, epoch, `lsn` = position, xxh3) **and** its trailer matches; a group therefore validates only behind the exact predecessor its writer validated it against, and the chain covers, transitively, the whole prefix.
  - A group never spans two extents. A rotation first pads the old extent with one lazy `Noop` group that ends at the extent's last byte, so the chain value at an extent start is the last 8 bytes of the previous extent. The first group after `init`, and after every epoch re-roll (`restore`, `repair`), is seeded with XXH3-64(epoch).
  - `RecHdr` is unchanged (32 B); the trailer is counted in the `group_end` record's length.
- **Pending groups**: complete valid groups beyond `committed_lsn`, appended but not yet covered by a published flush. Only in-flight groups lie there, and every appender scans them anyway.

**Phase 2a — append, under the writer byte.**
1. Acquire the writer byte (`acquire_within(lock.writer-wait-ms)`). A timeout exits 7 with nothing appended (G1).
2. `pread` the newest valid `HEAD` slot. **Boot check** (when this process can read its boot identity, §2.7.1): if `boot_id` differs, release the writer byte, run boot-change recovery (below) and start again at step 1.
3. **Scan and validate** from L0 — the bound up to which this process has already validated the log, with the chain value there — to the end of the log, by the chain rule.
   - Groups up to the published `committed_lsn` are applied to the process's overlay. **Pending groups are replayed into a scratch layer that is discarded when the writer byte is released**: no process's read overlay ever passes the published `committed_lsn`.
   - The `HEAD` counters (`next_id`, `next_anchor`, `fence`, `commit_seq`) advance from the scanned groups exactly as recovery derives them.
   - If the chain value at L0 no longer matches the log (a lost tail was replaced), the process drops its overlay and replays from its segment set. If the valid log ends below the published `committed_lsn` (a lazy tail lost after an OS crash or a failed flush), that end is the end of the log.
4. **Re-validate** the candidate against the scanned state (unchanged rules, [AR §4.5] step 7). Step 7's shortcut — "the candidate stands" — applies only when the scan found nothing beyond L0 ([81] M7). The idempotency key is evaluated against the scanned log, pending groups included; a hit on a pending group is returned only after that group passes phase 2b's identity check.
5. Allocate `#N`s, then append the group in one write at the end of the valid log, its trailer seeded from the preceding chain value. Remember the group's end E_g and its chain value.
6. **Lazy group, no durable group pending:** publish now (below); `committed_lsn` becomes E_g. Otherwise the group is published by the flush that covers it.
7. Release the writer byte.

**Phase 2b — make durable, then acknowledge.** Runs for every durable group, and for a lazy group appended behind a pending durable group ([81] m4: a pending group whose writer died must never strand later groups). Best-effort evidence appends (R4's evidence hooks, which already drop their record when the writer byte is busy) skip it and become visible with the next covering flush.
1. `pread` `HEAD`. If its bound covers the group (`durable_lsn` ≥ E_g; `committed_lsn` ≥ E_g for a lazy group), go to step 5.
2. Acquire the **flush byte** (`acquire_within(lock.flush-wait-ms)`). On timeout the process exits 7 with outcome `pending`: the group is appended, and a retry with the same idempotency key finds it and completes phase 2b for it.
3. Acquire the writer byte (flush, then writer: the lock order) and `pread` `HEAD`. If the group is covered, release both bytes and go to step 5.
4. **Scan, re-write, flush, publish.**
   - Scan `(durable_lsn, E]` by the chain rule, E being the end of the last complete valid group. If the own group is not inside it with its own chain value, release both bytes and go to step 6.
   - **Re-write** every byte of `(durable_lsn, E]` from the scan buffer — under the writer byte, so appenders and the flush holder share one view of the pending range. This repairs a predecessor that died or whose flush failed.
   - Release the writer byte and flush (`durable`) every extent file that holds bytes of the range. A flush error aborts the process; its flush byte is released at death, and the next holder re-writes and flushes.
   - Re-acquire the writer byte, **publish** (below) with `durable_lsn` = E, release the writer byte, then the flush byte.
5. **Identity check.** `pread` the 8 trailer bytes that end at E_g. If they equal the remembered chain value, **acknowledge**: the group is in the durable log behind exactly the prefix it was validated against. This holds for an idempotent replay too: a retry acknowledges a pending group only through this check.
6. **Lost group.** The group vanished before it became durable (after another process's failed flush its pages reverted, were invalidated or were evicted, and the hole may have been refilled). It is **not** acknowledged. The process re-runs phases 1–2 with the same idempotency key (the key is no longer in the log), at most twice, then exits 7 with `outcome unknown: retry with the same idempotency key`.

**Publish** (every publish: a lazy publish, a flush holder's publish, maintenance's no-op publish, boot recovery). Under the writer byte, a **read-modify-write of the newest valid slot** into the other slot ([81] M6):
- `durable_lsn` = max(the slot's value, the flushed E).
- `committed_lsn` = the end of the valid log as scanned at publish time, stopping before the first pending durable group this flush does not cover, so lazy groups appended during the flush are published too. It decreases only when a lazy tail was lost — the valid log ends below it — and then to that end.
- The `HEAD` effects of every group between the slot's `committed_lsn` and the new one are **folded in log order**: `commit_seq`, `next_id`, `next_anchor` and `fence` take the maximum; `refs_lsn`, `pins_lsn`, `heads_lsn` and `markers_lsn` advance to the newest covered record of their kind; a covered `Checkpoint` sets `segments`, `n_segments`, `checkpoint_lsn` and `active_log`; a covered `GitMap` advances `image_cursor`; `seq_ring` gains the covered commits. Lazy kinds have no `HEAD` effect beyond `committed_lsn`.
- `slot_seq` increments; `boot_id` is kept, except by boot-change recovery.
- Apart from the lost-lazy-tail case of `committed_lsn`, no field ever decreases, so no publisher can republish a stale segment set.

**Maintenance.** Its `Checkpoint` and promotion records are durable groups (phases 2a and 2b), so the segment-set change becomes visible with the publish that covers the `Checkpoint`. Maintenance runs its `HEAD` barrier and GC **only after its own `Checkpoint` has passed the identity check**. The barrier: under the writer byte, a no-op publish when the other slot does not yet name the post-change set; then `durable+meta` on `HEAD` outside the writer byte; then `unlink`; then `durable-name`.

**Boot-change recovery** ([AR §4.2]; [81] M7 fixes its lock order). Flush byte, then writer byte; scan from `min(durable_lsn, checkpoint_lsn)` by the chain rule and re-write every complete group in `(durable_lsn, end]`; release the writer byte; flush the log; re-acquire the writer byte and publish (read-modify-write, with the current `boot_id`); release the writer byte; `durable+meta` on `HEAD`; release the flush byte. Once per boot, off the commit path. A process in Unknown-boot mode never runs it (§2.7.1).

**Readers** are unchanged: they never read past `committed_lsn`, and an invalid record in `(durable_lsn, committed_lsn]` ends the visible log. New: a reader that observes a new `slot_seq` checks the chain value at its own replay bound L; if it no longer matches, or `committed_lsn` < L, it drops its overlay and replays from its segment set (only after a failed flush or a crash lost a lazy tail).

**Other writers of durable records** use the same phases: maintenance's `Checkpoint` and promotion records, `RefUpdate`, `FsIntent`, `Backup`, `GitMap`, `Pin`.

**Invariants.**
- **I-G1.** An acknowledgement implies a successful flush that began after the group's bytes were last written, a publish covering the group, and a passed identity check. A replayed idempotent result is acknowledged the same way.
- **I-G2.** Readers never see a durable-class group before a flush covers it; `committed_lsn` never passes a pending durable group; no process's overlay holds a group beyond the published `committed_lsn`.
- **I-G3.** The log is a chain: every group is valid only behind the exact predecessor it was validated against. After any crash or failed flush the valid log is a prefix of that chain, and nothing acknowledged depends on a lost group.
- **I-G4.** At most one log flush is in flight per store. The flush holder scans and re-writes the pending range under the writer byte and never flushes or waits for a lock while holding it.
- **I-G5.** An appended group whose writer dies is adopted by the next flush holder (re-write, flush, publish), or lost with everything after it; it is acknowledged only by a process that passes its identity check. Its idempotency key makes a retry exact.
- **I-G6.** Every publish is a read-modify-write of the newest valid slot under the writer byte that folds the `HEAD` effects of every newly covered group in log order; `durable_lsn`, the counters and the lsn pointers never decrease; `committed_lsn` decreases only to the valid end after a lost lazy tail.

**Costs.**
- A lone writer pays two extra lock operations (µs [M, X18 §5]), one extra `HEAD` pread, one 8-byte identity pread and one chained XXH3 over its group (≈ 30 GB/s; tens of ns for a small commit, est.).
- The writer-byte hold is the appender's scan, re-validation and append — tens of µs for a plain write — and the flush holder's scan and page-cache re-write of the pending range: µs for small groups, est. ≤ 4 ms in the worst case of 16 pending 1 MiB inline commits. Both are inside the hold gate (p99 ≤ 5 ms, max ≤ 20 ms).
- Under a 16-writer burst the device sees about 2–3 flushes. A lazy group appended behind a pending durable group waits at most one flush time for its publish.
- On Unix, each contended wait runs one waiter thread (§2.2.2).

#### 2.4.4 What changes in gates and seeded bugs

- **"Exactly one flush per durable commit"** becomes: at most one flush per durable commit; exactly one for a lone writer; ≤ 3 per 16-writer burst; and every acknowledgement preceded by a covering flush, its publish and a passed identity check (the `Vfs` counter and GT1).
- **The 16-writer last-acknowledgement gate** stays at p99 ≤ 50 ms on Windows. For the ports it becomes p99 ≤ max(50 ms, 3 × flush p99 + 5 ms), with the flush floor re-measured in the same run ([AR §8.3] per-OS notes).
- **The writer-byte hold** stays at p99 ≤ 5 ms and max ≤ 20 ms.
- **Thirteen seeded group-commit bugs**, added to [AR §8.2] and [60 §3.1 item 4]: (1) acknowledge before a covering flush; (2) publish `committed_lsn` past a pending durable group; (3) flush without re-writing a dead predecessor's range; (4) wait for the flush byte while holding the writer byte; (5) publish a smaller `durable_lsn`; (6) acknowledge an idempotent replay of a pending group before its durability; (7) acknowledge by position, without the identity check; (8) scan or re-write the pending range outside the writer byte; (9) accept a group whose predecessor differs (no chain check); (10) replay a pending group into a read overlay; (11) publish from a stale `HEAD` snapshot instead of a read-modify-write of the newest slot; (12) cover a `Checkpoint` without publishing its segment set, or run the barrier before it is published; (13) leave a lazy group stranded behind a dead writer's pending durable group.
- **GT1 and GT3** enumerate crash points between append, scan, re-write, flush, publish, identity check and release, for several concurrent pending groups, for a flush holder that dies before or after its flush and with a failed flush, and for **a failed flush followed by reverted, invalidated or evicted pages while ≥ 3 live writers are pending and appends and idempotent retries continue** (fault-model items 3 and 11). The reference model gains the invariant "every acknowledged group is in the final log behind the prefix it was validated against".

### 2.5 Mapping policy (one policy for every OS)

1. **Map only sealed files**: read-only and whole-file, from offset 0.
   - A file is sealed when it is completely written, `durable+meta` and `durable-name` have returned, and a durable `Checkpoint` or commit names it. Log extents and `HEAD` are never mapped.
   - Windows: `CreateFileMappingW(PAGE_READONLY)` + `MapViewOfFile(FILE_MAP_READ)`. Unix: `mmap(PROT_READ, MAP_SHARED)`.
   - No page size enters the format: whole-file mapping needs no offset alignment, and macOS arm64 uses 16 KiB pages [S, X17 §4.4].
2. **Sealed means read-only on disk**: `fchmod(0444)` on Unix, `FILE_ATTRIBUTE_READONLY` on Windows, applied by `seal` [X17 §4.6]. A casual `cp`, `truncate`, editor save or `O_TRUNC` open then fails with `EACCES` instead of crashing readers. GC clears the attribute before `DeleteFileW` on Windows; Unix `unlink` needs only directory permission. Neither macOS `UF_IMMUTABLE` nor Linux `chattr +i` is used (the first blocks moirai's own GC, the second needs a capability).
3. **Never truncate, extend, rename over or reuse the name of a sealed file.** File numbers are monotonic; this rule already exists [AR §4.1].
4. **Every sealed file states its own length, and the size is checked before mapping** ([81] m15). Every sealed kind's header carries `total_len u64`: `SegHdr` (segments, `cs`, `hist`, `blobs`) gains the field, the `gitmap` page header carries it, and each `dict` file begins with a 32-byte header `{magic "MDIC", total_len u64, blake3_16, _ [4]}` before the dictionary bytes (the form frozen at M0 by [AR §8.2] item 6: raw content, or a formatted zstd dictionary if M0 chooses [90 §11.3] option (3)). Before mapping, one `pread` of the header and one `fstat` or `GetFileSizeEx` must agree with `total_len`; otherwise re-read `HEAD`, retry once, then exit 7 naming the file and `moirai doctor --fsck`. This is the Unix counterpart of the Windows delete-pending retry — and Windows never needed a length only because a mapped file cannot be truncated there.
5. **Typed, bounds-checked access.** Mapped bytes are read only through types for which every bit pattern is valid (`zerocopy::FromBytes`: integers and byte arrays; never `bool`, enums or `char`); every offset or length taken from mapped bytes is checked before use (`get()`, never `get_unchecked`). A changed byte then yields a checksum failure or a wrong answer, never an out-of-bounds access. This argument is recorded in the `Vfs` safety comment [X17 §4.3].
6. **Fault handler.**
   - Every mapping is registered in a process-global, lock-free, fixed-size table of address ranges.
   - **Unix:** a `SIGBUS` handler (`SA_SIGINFO`) that chains to the handler that was there before, which is Rust std's stack-overflow handler [I]. **Windows:** a vectored exception handler for `EXCEPTION_IN_PAGE_ERROR`.
   - If the fault address lies inside a registered mapping, the handler writes one line to stderr with an async-signal-safe `write` or `WriteFile` — `store I/O fault in <file> at <offset>: run moirai doctor --fsck` — and ends the process with **exit code 7**, "store unavailable" (`_exit(7)` or `TerminateProcess(self, 7)`). Otherwise it passes the fault on. Exit 7 already means "store unavailable" in the frozen code set 0–10 [AR §7.1].
   - The protocol treats this like any crash: readers hold no locks, and a writer's pending group is adopted by the next flush holder.
   - **The MCP server** behaves the same way. Command hooks keep working, since each spawns a fresh process; `mcp_tool` hooks become non-blocking errors until Claude Code restarts the server — automatic reconnection is not established [AR §7.5] — so the context of those events is missing, never wrong ([81] m12). A `pread`-only server would give up the shared mapped pages and raise private RAM; it is rejected [X17 §9 item 8].
7. **Advice is a hint only.** `MADV_RANDOM` on index sections and `MADV_WILLNEED` before a tier-1 scan on Unix, `PrefetchVirtualMemory` on Windows. Never `MAP_POPULATE`, `mlock` or `mremap`.
8. **Tests.** In M1, the Windows kill loop and GT1 cover the in-page handler through fault-model item 9. In the port phase, the Unix kill loop adds external truncation of a random sealed file: the affected readers exit 7, no acknowledged commit is lost, `doctor --fsck` names the file, and `repair --rebuild-from-log` restores it, because sealed files are derived.

### 2.6 Environment guard

A store is used only where every one of its guarantees holds. The guard runs:
- at `init` and `restore`, as the full probe: classification, a durable write, `sync_dir`, the OFD or `LockFileEx` probe at 2^62, and on macOS an `F_FULLFSYNC` that must not return `ENOTSUP`;
- at every open, as a cheap classification: one `statfs` or volume query, est. ≤ 20 µs, and the OS-version check of §2.13.

**Decision:** an allow-list per OS in which **every allowed file system is crash-gated** [X17 §3.7, X18 §9]. An unknown or ungated local type is refused with its name. Adding a type is a code change backed by that type's crash evidence (the GT4 and GT15 variants on a volume of that type), never a configuration key ([81] m14). In M0–M11 NTFS is admitted on the GT1/GT3 fault-model evidence and GT4 on real NTFS; its GT15 variant runs with the OS-crash rig, deferred to after the release by the owner review of 2026-09-27 ([AR §10] risk 17). That keeps X5.

| OS | Allowed (each gated) | Refused, with the reason printed |
|---|---|---|
| Windows | NTFS (M1) | ReFS and Dev Drive (not yet gated); FAT32 and exFAT (no journal, so a rename is not crash-atomic [I]); `GetDriveTypeW = DRIVE_REMOTE`; UNC paths, including `\\wsl$` and `\\wsl.localhost` (two kernels); a path whose final path (`GetFinalPathNameByHandleW`) resolves to UNC, as a mapped network drive does (a `subst` of a local folder resolves locally and is allowed); OneDrive roots and `RECALL_ON_*` directories |
| Linux (port) | ext4, XFS, btrfs (`statfs.f_type`; each gated in the port phase) | ZFS, f2fs and bcachefs (not yet gated); NFS `0x6969`; SMB/CIFS `0x517B`/`0xFE534D42`/`0xFF534D42`; FUSE `0x65735546` (sshfs; virtiofs and the gRPC-FUSE mounts of Docker Desktop and Lima report it); 9p `0x01021997` (WSL2 `/mnt/*`); tmpfs and ramfs; **every overlayfs** — a dev container must bind-mount its workspace from an allowed file system, and a repository cloned inside the container's own layer cannot hold a store; anything not on the list |
| macOS (port) | APFS (case-insensitive and case-sensitive) with `MNT_LOCAL` (gated in the port phase) | HFS+ (not yet gated); any volume without `MNT_LOCAL` (SMB, AFP, NFS, WebDAV); macFUSE; FAT and exFAT; iCloud Drive (`~/Library/Mobile Documents`), File Provider roots (`~/Library/CloudStorage/*`) and `SF_DATALESS` directories; `~/Desktop` and `~/Documents` when iCloud "Desktop & Documents" manages them — detected by their canonical path resolving into `~/Library/Mobile Documents` under `F_GETPATH_NOFIRMLINK`, or by the File Provider domain of the folder, whichever the port-phase probe confirms [I] ([81] m14); `ENOTSUP` from `F_FULLFSYNC` |

**Also:**
- **Dev Drive.** A Dev Drive is ReFS, so a repository moved onto one cannot hold a store (`<git-common-dir>/moirai`) until ReFS variants of GT4 and GT15 admit ReFS, which is not scheduled; in M0–M11 every repository that holds a store stays on NTFS ([AR §11] #15). A Defender exclusion on an NTFS folder is unaffected.
- **Cross-kernel sharing is refused both ways.** A Windows moirai and a WSL2 moirai on one store would not exclude each other, because their byte-range locks live in different kernels [I, X18 §9]. A repository used from WSL2 keeps its store on the WSL ext4 disk.
- **`doctor` warnings** (never refusals, because none can be detected reliably): ext4 mounted with `barrier=0`/`nobarrier`; a device whose `queue/write_cache` is set to write-through; the Windows setting "turn off write-cache buffer flushing"; external USB drives [X17 §3.7].
- **Sandboxes.**
  - A writer that cannot create or write the store gets exit 7 with the fix, for example a Claude Code session started in a subdirectory of the main checkout, whose `.git` is outside the sandbox's write set [I, X20 §3.3]: the exact `sandbox.filesystem.allowWrite` entry (`//<abs>/.git/moirai`). Readers still work, because they open read-only and take no locks.
  - **Writes outside the store** ([81] m11). `image export` to its default destination (beside the main worktree), `backup DIR` outside the store and the restore swap need write access a sandboxed Bash command lacks on Linux and macOS. A sandboxed CLI exits 7 printing the exact `allowWrite` entry for the destination's parent; the daily export of `SessionStart` runs in an unsandboxed hook anyway ([AR §7.5]).
  - **Codex** ([90 §5]). `workspace-write` keeps `.git` (directory, pointer file and resolved gitdir) read-only on every OS, so a sandboxed CLI cannot write `<git-common-dir>/moirai`; Codex's MCP servers run outside the command sandbox and write normally; `moirai integrate codex` adds exactly the store directory as a writable root (default) or, opt-in, an execpolicy rule; exit 7 prints the equivalent MCP call and the owner's fix. On Windows the elevated sandbox runs commands as separate local users: files they create under the writable root inherit the store directory's owner ACE (set by `init`), and their liveness probes answer Unknown.
  - **The rollup child** is never spawned by a CLI that runs in a foreign PID namespace (`getppid()` = 0, or a PID-namespace inode other than the one the session's MCP server recorded in its slot): bubblewrap would kill it with the command's namespace. The MCP server or an explicit `moirai gc` runs the rollup instead.
- **Tests.** Test builds only may admit a LazyFS FUSE mount or a tmpfs `--ephemeral` store (§5.2). Product builds cannot.

### 2.7 Process identity and liveness

#### 2.7.1 `ProcId` (diagnostics), boot identity and clocks

`ProcId`, 32 B, is the same layout on every OS:

| Field | Size |
|---|---|
| `os` | u8 |
| `flags` | u8 |
| reserved | u16 |
| `pid` | u32 |
| `start` | u64 |
| `boot_hash` | u64 |
| `pidns` | u64 |

| Item | Windows | Linux | macOS |
|---|---|---|---|
| `start` | `GetProcessTimes` creation time (FILETIME) | `/proc/<pid>/stat` field 22, parsed after the last `)`, converted to ns since boot [D] | `sysctl(KERN_PROC_PID)` `p_starttime` — public, and allowed under Seatbelt [S] (libproc is not used, X9) |
| **`boot_id`** (16 B in `HEAD`; its hash in anchors) | BLAKE3-128 of (the per-boot counter `BootId` from `KUSER_SHARED_DATA`, or where absent the Session Manager's `PrefetchParameters\BootId` registry value, ‖ `MachineGuid`) [I; verified by M0 item 22] | `/proc/sys/kernel/random/boot_id` [D] | `sysctl kern.bootsessionuuid` [S; undocumented, so its absence only selects Unknown-boot mode] |
| `pidns` | 0 | inode of `/proc/self/ns/pid` | 0 |
| **Boot clock** (lease deadlines, fault-model item 7) | `QueryInterruptTimePrecise` (includes sleep) | `clock_gettime(CLOCK_BOOTTIME)` | `mach_continuous_time()` |
| `alive(ProcId)`, diagnostics only | `OpenProcess(QUERY_LIMITED)` plus the creation time; `ACCESS_DENIED` (another principal) → Unknown | a different boot or `pidns` → Unknown; the process gone or its start time different → Dead | `EPERM` inside Seatbelt → Unknown; `ESRCH` or a different start time → Dead |

- **The boot-identity rule (frozen, X-F2)** ([81] M2). `boot_id` is constant for exactly one boot of the kernel and invariant under wall-clock changes, suspend and hibernation. The Windows system boot time is **not** used: the kernel adds the delta to its boot time whenever the system time is set, so a w32time step, a manual change or the clock re-read on resume would read as a reboot, and every non-run-scoped lease would become Dead under a live agent. `MachineGuid` in the Windows hash keeps a small counter from colliding with another machine's store copy. M0 item 22 verifies the Windows source across a manual clock step, a sleep and a hibernation, and across a reboot (which must change it); if it fails, Windows runs in Unknown-boot mode until a conforming source is found — `boot_id` is opaque, so no format changes.
- **Unknown-boot mode (frozen, X-F2)** ([81] M3). A process that cannot read its boot identity — a sandbox that denies the sysctl, a source that failed M0 item 22 — does not refuse the store:
  - it never republishes `boot_id` (a publish keeps the slot's value) and never runs boot-change recovery;
  - every lease boot test it makes answers `Unknown`, which never ends a lease; the lease's wall-clock deadline applies;
  - its readers stay correct through the rule that an invalid record in `(durable_lsn, committed_lsn]` ends the visible log, and the chain check of §2.4.3;
  - its writers scan to the end of the log, so an acknowledged commit whose publish was lost in an OS crash is published by the next flush.
  - The only loss: until an unsandboxed process (the MCP server, a hook) or any writer runs, such a commit stays invisible to that process's readers.
- **Private-API position (X9)**, per OS: Windows — documented Win32 and the WDK-documented `NtFlushBuffersFileEx` and `KUSER_SHARED_DATA`; the former `NtQuerySystemInformation(SystemTimeOfDayInformation)` boot time is dropped. Linux — documented syscalls and `/proc` files only. macOS — the public SDK of macOS 14 (OFD locks, `F_FULLFSYNC`, `fsgetpath`, `getattrlistbulk`, FSEvents, `sysctl(KERN_PROC_PID)`, `task_info`) plus one undocumented read, `kern.bootsessionuuid`, whose absence selects Unknown-boot mode; libproc (`proc_pidinfo`, `proc_pid_rusage`), which describes itself as private, is used only by the measure harness; the private OFD constants of macOS 10.13–13 are not used.
- **Lease deadlines** `{wall, boot_hash, mono}` use the boot clock, so a suspended laptop's 15-minute lease expires by elapsed time, identically on every OS; in Unknown-boot mode the wall component decides. This is a semantic, so it is frozen (X-F2).
- **Verification.** Each clock's suspend behaviour is taken from vendor documentation [I]; M0 item 22 checks the boot clock and the boot identity on Windows across a clock step, a sleep and a hibernation; the port probes check them on Linux and macOS, including readability inside both Claude Code sandboxes (§5.2).

#### 2.7.2 Lock-anchored liveness (the correctness mechanism)

**The problem.** Inside Claude Code's Linux sandbox every command runs in a fresh PID namespace. A `(pid, start)` recorded there names a process nobody else can find, and the small PIDs repeat in every command, so a checker would conclude a **false Dead**, recover an in-flight `FsIntent` or expire a live lease. On macOS, Seatbelt makes the lookup `EPERM` [S, X18 §7.2, §8.3], [X20 §3.3].

**The design.** Locks belong to the inode, and bubblewrap bind-mounts the same inode, so a lock is visible across namespaces and sandboxes [I, X18 §8.3].

| Anchor kind | Who holds the slot | Anchor stored in the record (32 B) | Alive when | Dead when | Unknown when |
|---|---|---|---|---|---|
| `session` (1); `session-ttl` (4, [90 §4.4]) | the MCP server whose lifetime the identity tracks (Claude Code: the session's, from start; Codex: the thread's, from its first call carrying `_meta.threadId`), to exit | `{kind, os, slot hint, session_hash, boot_hash}`, `session_hash` being BLAKE3-128 of the namespaced identity `<harness>:<id>` (16 B; the server's primary hash) | the boot matches (or is unknown to the checker), **and** some held slot has a valid record of kind `session` whose primary hash equals the anchor's; for `session-ttl` also the lease's deadline, renewed by the thread's own calls, has not passed | the boot differs, or no held slot's record names the session, or (`session-ttl`) the deadline has passed | `LOCK` cannot be read or probed (for example `ACCESS_DENIED` for another principal, `EPERM` from a sandbox) |
| `intent` (2) | the CLI running `file mv`/`file rm`, from before its `FsIntent` append until `FsIntentDone`/`Aborted` | `{kind, os, slot, nonce, session_hash_lo (diagnostic, 8 B), boot_hash}` | the boot matches, the record at `slot` carries `nonce`, and the slot is held | the boot differs, the nonce differs, or the slot is free | as above |
| `leader` (3) | the optional leader | the leader byte is its anchor; `LeaderRec` holds `nonce` | leader byte held and nonce equal | otherwise | as above |
| `none` (0) | — | — | — | — | leases live by TTL or run scope alone ([AR §6.2]) |

**How it works.**
- **Finding a session's slot.** A CLI or hook reads the 32 KiB slot table with one `pread`, matches the BLAKE3-128 hash of its namespaced harness identity (`claude:` + `CLAUDE_CODE_SESSION_ID`, `codex:` + `CODEX_THREAD_ID`, [90 §4.4]) against each record's primary and alias hashes, and probes the matching slot.
- **`/clear`.** Claude Code changes the session id for Bash and hooks but not for the running MCP server [D, X18 §7.2]. The `SessionStart(clear)` hook runs as an `mcp_tool` handler on that server ([AR §7.5]), which stores the new id in its record as `alias_hash`. A lease stores the **primary** hash, so it survives any number of clears.
- **Restarts.** A server that restarts in the same session takes a slot again and writes the same primary hash; a check made in the gap sees Dead, as [AR §6.2] already specifies for a server death.
- **Choosing a slot.** A server tries slots from `hash(session) mod 256`, an intent from `hash(nonce) mod 256`, taking the first free one. With the slots full, a lease gets anchor `none` and `file mv` exits 7 "no liveness slot free". 256 slots against ≈ 16–50 concurrent holders gives a load factor ≤ 0.2 [I].
- **Race windows** (every error in them is in the safe direction). A server writes its record right after taking its slot, before it serves anything; a CLI that looks in that window finds no anchor, and its lease gets kind `none`. A slot whose record still names the previous owner makes that owner look Alive for microseconds. On Windows a dead holder's slot reads Alive for the release lag, and on every OS while a crash reporter holds a crashing process (fault-model item 8).
- **Rules unchanged from [AR §6.2].** `Unknown` never ends a lease early; only a deadline, the run scope or an explicit `reclaim` does. A boot change, seen by a process that can read its boot identity, makes every non-run-scoped lease Dead.
- **What changes.** The session is found by record match, not by slot index. **`FsIntent` gets its own intent anchor**, so recovery of a crashed CLI's intent runs at the next writer's open instead of when the whole session ends.
- **MCP server lifetime.** The server ends when its parent, the Claude process, ends, even if stdin EOF never arrives, so that its slot mirrors the session. The watch is event-driven, with no timers:
  - Windows: wait on the parent's process handle together with stdin;
  - Linux: `pidfd_open(getppid())` (Linux ≥ 5.3), polled together with stdin, after checking that `getppid()` still returns that pid (if the parent died first, the server exits at once). `PR_SET_PDEATHSIG` is **not** used: it fires when the spawning *thread* exits, so a runtime that spawns the server from a worker thread would kill it early and make the session's leases Dead ([81] m13);
  - macOS: `kqueue EVFILT_PROC NOTE_EXIT` on the parent [X18 §8.2].
- **Cost.** A probe is 2–9 µs on Windows [M, X18 §5] and one `fcntl` on Unix (unmeasured); the slot-table read is one `pread` of 32 KiB.

### 2.8 IPC endpoint (used only if the leader is built)

- **Rendezvous through `LOCK`.** The leader, while holding the leader byte, unlinks any stale socket, binds, and writes `LeaderRec{seq, proc ProcId, proto, endpoint_kind (1 pipe | 2 Unix socket), endpoint ≤ 200 B, nonce u128, xxh3}` at `LOCK` offset 3072. A client reads `LeaderRec`, probes the leader byte, connects, checks the peer, and requires the nonce echoed back. The leader byte replaces the usual "is this socket stale?" race [X18 §7.3].
- **Endpoint names.**
  - **Windows:** `\\.\pipe\moirai-<h(user SID)>-<h(store)>`, with a per-user DACL, `FILE_FLAG_FIRST_PIPE_INSTANCE` and `PIPE_REJECT_REMOTE_CLIENTS`; peers checked with `GetNamedPipeClientProcessId` and `GetNamedPipeServerProcessId`.
  - **Linux:** `$XDG_RUNTIME_DIR/moirai/<h16>.s` when `XDG_RUNTIME_DIR` is set and owned by the user with mode 0700, else `/tmp/moirai-<uid>/<h16>.s`; peer checked with `SO_PEERCRED`.
  - **macOS:** `confstr(_CS_DARWIN_USER_TEMP_DIR)/moirai/<h16>.s`; peer checked with `getpeereid` and `LOCAL_PEERPID`.
  - `<h16>` is 16 hex characters of the BLAKE3 of the canonical store path. The directory is created 0700 and checked with `lstat` (not a symlink, uid equal to euid, mode 0700). The path is ≈ 45–80 bytes, under the 104-byte `sun_path` limit of macOS and the 108 bytes of Linux [S/D, X18 §7.1].
- **Never used:** Linux abstract sockets (no permissions; scoped to a network namespace); a location derived from `$TMPDIR` (Claude Code gives sandboxed and unsandboxed commands different values [D]); a socket inside `.git/moirai` (unbounded path length; a socket inode breaks copy tools [X18 §7.3]).
- **Sandboxes.** A sandboxed client falls back to the direct path, which is complete: `EPERM` from `socket(AF_UNIX)` on Linux, a denied connect under Seatbelt, a DACL mismatch for `srt-win`. moirai never asks the owner to set `allowUnixSockets` or `allowAllUnixSockets` [X18 §11 item 8].

### 2.9 Metering per OS (gates and diagnostics)

| Quantity | Windows (M0–M11 gates) | Linux (port) | macOS (port) |
|---|---|---|---|
| Private peak | `PROCESS_MEMORY_COUNTERS_EX.PeakPagefileUsage` at exit | cgroup-v2 `memory.peak` of a per-run leaf cgroup; the store created and pre-read by the harness **outside** the leaf, on ext4 or XFS, never tmpfs; cross-checked at exit with `smaps_rollup` (`Anonymous + Swap − LazyFree`) + `VmPTE`; without a delegated cgroup, the at-exit value, flagged "at-exit, not peak" | `task_vm_info.ledger_phys_footprint_peak` at exit (`task_info`, public Mach API); the harness cross-checks with `proc_pid_rusage(RUSAGE_INFO_V6).ri_lifetime_max_phys_footprint` (libproc, harness only, X9) [S, X17 §5.5] |
| Never the gate | — | `ru_maxrss` (kilobytes on Linux, bytes on macOS; it includes file-backed pages); `/proc/*/status` RSS, which is approximate [D] | same |
| `heap_peak` (portable) | a counting global allocator in the measure build (live bytes and high-water mark), reported beside every RSS gate | same | same |
| Idle CPU and context switches | `GetProcessTimes`; ETW context switches | `/proc/<pid>/stat` utime/stime; `/proc/<pid>/status` voluntary and nonvoluntary context switches | `task_info` thread times and the harness's `proc_pid_rusage` |
| Spawn floor | `hyperfine -N --warmup 5` | same | same, **after** a warm-up launch; the first-launch XProtect scan (0.3–5 s [C]) is reported separately and never gated |

- **Gate form in the ports:** `private_peak(command) − private_peak(empty Rust binary, same toolchain, allocator, OS and run)` ≤ the Windows gate minus the Windows floor. For the CLI row at 1e5 that is 4 MB − 0.69 MB ≈ 3.3 MB [X17 §5.5].
- **Why a floor.** A minimal macOS process already has a footprint of about 2 MB [C]; Linux `RssAnon` is est. 0.2–0.5 MB. The moirai-attributable memory is the same everywhere.
- **Windows keeps its absolute gates** ([AR §8.3] unchanged). The floor-relative value is reported beside them from M0.
- **Link only the system library.** On macOS the binary links only `libSystem`: no CoreFoundation or Security, which added 6 ms to startup and dirty pages in the rand #733 case [C]; checked with `otool -L`. On Linux the build is static musl (§2.13).
- **Allocator.** The global allocator is the system allocator on every OS, with moirai's region arenas carrying the hot allocations ([AR §6.1]): owner decision #44 makes every dependency pure Rust, which excludes mimalloc and jemalloc (both C) [X20 §4.2]. musl's allocator is slow mainly under many threads, and moirai's processes run one or two; the port's probes measure it, and a pure-Rust allocator is added only if a gate needs it. The M0 measurement on Windows confirms the system allocator against the RSS and speed gates ([90 §11.3]).

### 2.10 Paths: one stored form on every OS

Three equivalence rules meet in one repository [X19 §0 item 7]:
- **NTFS** is case-insensitive per directory and normalization-*sensitive*: an NFC and an NFD `café.txt` were two files [M].
- **Linux** compares bytes exactly, except in casefold directories, which compare NFD plus a full case fold (ß equals ss) [D].
- **APFS** is case-insensitive by default and normalization-insensitive but normalization-preserving; HFS+ stores NFD [D].

**Git's spelling is the one all three share.** These rules make a link's stored bytes, its derived uid and every image file byte-identical everywhere ([40] DR7, DR8). They are frozen at M0 as part of the path-key canonical form (X-F7).

| # | Rule | Why |
|---|---|---|
| P1 | A stored path is root-relative, `/`-separated, has no empty, `.` or `..` segment and no leading `/`, and is valid UTF-8 stored as exact bytes (unchanged, I-F8) | one string type in the format |
| P2 | **Tracked file:** git's HEAD-tree spelling, as git stores the bytes, on every OS (unchanged, [40 §2.3]) | git's tree is the spelling the three OSes share |
| P3 | **Untracked file:** the OS's enumerated spelling, except that on a normalization-insensitive volume (APFS, HFS+) the name is stored as `NFC(name)` when the repository has `core.precomposeUnicode = true`, or always when there is no git. On Windows and Linux names are never normalized | git's init and clone probe sets `core.precomposeUnicode` on macOS, and git then records NFC [S, X19 §4.6]. Without P3, a file linked before its first commit would derive one uid on macOS and another once committed or seen on Linux |
| P4 | **Refused at link time, on every OS:** a component containing `\` or a C0 control character; a name that is not UTF-8 (Linux); a name with an unpaired surrogate (Windows). Such a path already in git renders `unrepresentable path` and is never a candidate | `\` is a separator on Windows, and Git for Windows rejects it [M, X19 §5]; the format has one string type |
| P5 | **Portable by default:** `file mv`, `file add` and `link` warn about, and `file mv` refuses to **create**, a name some supported OS cannot hold: Windows device names (`CON`, `PRN`, `AUX`, `NUL`, `COM0`–`COM9`, `LPT0`–`LPT9`, with any extension); a trailing dot or space; any of `<>:"\|?*`; a component over 255 UTF-8 bytes; a name equal to a sibling under `fold_v1`. The policy is the key `files.portable-names = refuse \| warn` (store, hot, default `refuse`, [AR §13]); `--allow-nonportable` overrides it per command. Paths already in git stay linkable and render `missing (not representable on this OS)` where they cannot exist, by the twin rule of §2.11.4 | one repository is checked out on three OSes; Git for Windows rejects these names, and a case collision keeps only one file on checkout [M, X19 §5] |
| P6 | **`PATHIDX` fold, one frozen function (R-14):** `fold_v1(x) = NFD(full_casefold(NFD(x)))`, with full case folding (CaseFolding.txt statuses C and F) and normalization **at Unicode 17.0.0**, named in the frozen constant ([81] m8). Used for collision detection, twin detection and index order only, never for identity | one key covers NTFS and APFS case-insensitivity, APFS/HFS+ normalization and ext4 casefold, which folds fully (ß = ss), so P5's sibling check misses no pair an OS merges. Each OS's real behaviour is observed at resolve time (§2.11.4 rule 2) |
| P7 | `origin_path`, the uid derivation input ([40 §2.3]), follows P2 and P3 | the same file derives the same uid in every store |
| P8 | Symlinks: `lstat` semantics; a link names the link itself; a symlink's `oid` is taken over its target text, as git does. With `core.symlinks = false` on Windows, git checks the symlink out as a text file with the same bytes, so the `oid` is equal [I] | the same `oid` on every OS |
| P9 | **Canonical root, per component** ([81] m10). The `TREES` key, the `HEADS` directory-binding key and the `git.worktree` provenance string are taken over the OS-canonical top-level with `/` separators and the **on-disk spelling of every component**: Windows `GetFinalPathNameByHandleW`, drive letter upper-cased; macOS `F_GETPATH` for the firmlinked form, then each component replaced by its on-disk name through `getattrlist(ATTR_CMN_NAME)`, as Apple's `realpath(3)` does [S, X19 §4.6]; Linux `realpath`, then, in casefold directories, each component replaced by the entry name enumeration returns. The root directory's `OsFileId` is stored too, and **bindings, trees and root containment are looked up by that id first and by spelling second**: two spellings with one root id are one tree, and a second binding is refused (I-F12) | `D:\` and `d:\` both occur in practice [M]; `/tmp` and `/private/tmp` on macOS; case-variant `cd` spellings on APFS and in Linux casefold directories; bind mounts on Linux [X19 §9] |
| P10 | Walks are relative (`openat`/`fstatat` on Unix; `\\?\` or handle-relative opens on Windows) | macOS `PATH_MAX` is 1024 bytes and Linux's 4096 [S]; Windows' is 260 characters without `\\?\` |
| P11 | **Names moirai writes into git or the store are portable by construction:**<br>(a) image file names are generated only from hex digits and fixed ASCII words. The one exception today, `schema/queries/<name>.moi`, becomes `schema/queries/<q>.moi`, where `q` is 32 lower-case hex digits of BLAKE3-256 over the query name's bytes; the name stays on the file's `name:` line.<br>(b) A ref-name segment may not be a Windows device name (case-folded, before its first `.`) and may not end in `.lock`; a new ref name is NFC-normalised on input and refused (exit 2) when it is equal under `fold_v1` to a live ref name (`lane/Foo` beside `lane/foo`); `doctor image` reports fold-equal refs in a destination ([81] m9).<br>(c) Every store file name is built from decimal numbers and fixed ASCII words (`seg.b<ref_id>.K`, never a ref name). | LQ query names allow both cases and back-quoted arbitrary text [50 §2.2 rule 4], so `Foo` and `foo`, or a name containing `:`, cannot be checked out on Windows or macOS [X19 §9]. A loose ref is a file on the cloner's OS, and fetches create loose refs after the initial `packed-refs`; git refuses `.lock` endings [D, git-check-ref-format] |
| P12 | **Root `abs`: a machine-local absolute path** (verification pass, 2026-09-26). The `abs` root ([40 §2.4]: `lane.worktree_path`, `run.script_path`, `run.journal_path`) stores the OS-canonical absolute path with `/` separators, valid UTF-8 as exact bytes, with no empty, `.` or `..` segment: canonicalized per component as in P9 when the path exists at record time, otherwise made absolute and lexically normalized. **Windows:** `X:/…` with the drive letter upper-cased and no `\\?\` prefix; a UNC path as `//server/share/…`. **Linux and macOS:** the path keeps its leading `/` — the one exception to P1's "no leading `/`", allowed for root `abs` only. An `abs` path is **machine-local**: existence and `oid` checks only, never re-bound, never a candidate, never compared across machines or OSes; where it does not exist — any path written on another OS included — it renders `missing` with no candidate search. Its bytes are versioned, hashed and exported unchanged, so an `abs` artifact's derived uid ([40 §2.3]) is machine-specific by construction | P1 and P9 cover root-relative paths and canonical roots only; without P12 a port would have to invent the Windows drive-letter case and the Unix leading `/` of versioned, hashed and exported bytes — a format change under X8 |

**At the CLI boundary** (§4): on Windows a path argument may use `\` or `/` and is converted to `/`; on Unix `\` in an argument is a literal name character, so P4 refuses it rather than guessing; displays escape non-UTF-8 bytes as `\xNN`.

### 2.11 File identity and change tracking for R4 (`ProjectFs`)

#### 2.11.1 The capability record

The resolver's rules, states, thresholds and strings live in one target-independent module. It sees **evidence records**, never OS calls ([X19 §8.1]). Each volume reports a `VolumeCaps`, computed once per volume and command. A missing capability turns a source off and never changes a rule (X5).

| Capability | Windows | Linux | macOS |
|---|---|---|---|
| `id_kind` | `ntfs128` (NTFS: the 16-bit sequence number in `FILE_ID_128` makes a reused MFT slot a different id), `refs128` (ReFS) | `linux_ino` on ext4, XFS, btrfs, f2fs, bcachefs, ZFS and tmpfs (within one boot) **only where unprivileged `name_to_handle_at` succeeds**, because the file handle carries the inode generation ([81] B2); `none` on overlayfs (unless verified `xino`), NFS, CIFS, vfat, exFAT, FUSE, 9p, and wherever handles are unsupported | `darwin_fileid` on APFS (counter-allocated ids [D]) and HFS+ (catalog ids, reused only after 2^32 allocations [I]) when `VOL_CAP_FMT_PERSISTENTOBJECTIDS` is set; `none` otherwise |
| `id_locate` | `ById` (`OpenFileById` + `GetFinalPathNameByHandleW`) | `Frontier` (§2.11.3); `open_by_handle_at` needs `CAP_DAC_READ_SEARCH` [D] | `ById` (`fsgetpath(fsid, objid)`, unprivileged with search permission on each component [S]; needs `VOL_CAP_FMT_PATH_FROM_ID`). `openbyid_np` needs an entitlement and is not used |
| `journal` | `usn` (unprivileged read; E2 is not built, #41) | `none`: inotify and fanotify need a resident process, and btrfs and ZFS tools need privilege [D/S] | `fsevents` (per-device history with inodes since 10.13; E2 is not built, #41 — §2.11.4) |
| `btime` | `TunneledNotCopied`; tick-granular, so equality corroborates only when unique in the E4 scope (§2.11.4 rule 1) | `Unforgeable` when `stx_mask` has it, else `Absent`; **never unique**: one kernel tick covers many files of a checkout ([81] M5) | **`CopiedByClones`** (`clonefile` and `cp -c` keep the creation time, and `ATTR_CMN_CRTIME` is writable [D]) |
| `ctime_on_rename` | ChangeTime, set by rename | `true` on ext4, btrfs and XFS [S] | probe (unverified) |
| `case_rule` | `PerDirFlag` (`FileCaseSensitiveInfo`) | `PerDirFlag` (`FS_CASEFOLD_FL`) or `Sensitive`; `Volume` for vfat and exFAT | `Volume` (`VOL_CAP_FMT_CASE_SENSITIVE`) |
| `norm_insensitive` | `false` [M] | `true` only in casefold directories | `true` on APFS and HFS+ |
| `cloud` | `RecallAttrs` (OneDrive `RECALL_ON_*`, `OFFLINE`) | `none` | `Dataless` (`SF_DATALESS`, checked on every read path; the process policy `IOPOL_MATERIALIZE_DATALESS_FILES_OFF` is a second line) |
| `rename_noreplace` | `MoveFileExW` without `REPLACE_EXISTING` | `renameat2(RENAME_NOREPLACE)`, since 3.15 on ext4 and 4.0 on XFS; `EINVAL` elsewhere → `link` + `unlink` for files (crash state "both names, one inode": an `FsIntent` recovery state), refusal for directories | `renamex_np(RENAME_EXCL)` when `VOL_CAP_INT_RENAME_EXCL` is set; otherwise `file mv` is refused on that volume (exit 7) |
| `mtime_granularity_ns` | the **effective** granularity measured per volume at its first settle and recorded in `TREES` ([81] m7); nominal 100 ns | nominal 1 ns, but jiffy-coarse (1–10 ms) before Linux 6.13, so the measured value | nominal 1 ns on APFS, 1 s on HFS+ |
| Read-path stat | `GetFileAttributesExW`, which gives no id | `statx(AT_SYMLINK_NOFOLLOW)`, whose inode comes free; plus one `name_to_handle_at` for a linked file whose identity a read relies on | `lstat`/`getattrlist`, id included; `st_flags` shows `SF_DATALESS` |
| Settle enumeration | `FileIdExtdDirectoryInfo` | `getdents64` (`d_ino`, `d_type`), then `statx` and `name_to_handle_at` for recorded files and directories | `getattrlistbulk` (name, type, file id, parent id, size, mtime, crtime, added time, extended flags) |
| Trash (rule P6 of [40]) | `$Recycle.Bin` | `$XDG_DATA_HOME/Trash`, `$topdir/.Trash/$uid`, `$topdir/.Trash-$uid` [D] | `~/.Trash`, `/.Trashes/<uid>` [I] |
| No-atime reads | not applicable | `O_NOATIME` when the process owns the file (the kernel returns `EPERM` otherwise; the file is then opened without it) | `IOPOL_ATIME_UPDATES_OFF` for the process |
| Sharing, busy files, denials | errors 5 and 32; `RmGetList` | none; `EXDEV`, `EBUSY`, `EACCES`/`EPERM` | same as Linux, plus `EPERM` from TCC, SIP and sandbox MAC hooks; every denial maps to `Unknown` or "source absent" (§2.11.4 rule 9) |

#### 2.11.2 Tagged runtime layouts (frozen at M0, X-F8; runtime only, never versioned, hashed or exported — I-F4)

| Reservation | Layout |
|---|---|
| **`OsFileId`** (57 B packed; replaces `FILEOBS`'s volume serial, `FILE_ID_128` and parent `FILE_ID_128`) | `{kind u8 (0 none, 1 ntfs128, 2 refs128, 3 linux_ino, 4 darwin_fileid), vol_key [16], id [16], parent [16], aux u32 (reserved, 0), docid u32}`.<br>**`vol_key`** = BLAKE3-128(a fixed source tag ‖ one fixed source per kind) ([81] m15): Windows — the 64-bit volume serial; Linux — `statfs.f_fsid` (available on every kernel; btrfs mixes the subvolume into it; XFS derives it from the device number, so rows may go stale across boots, which makes them absent, never wrong); macOS — `ATTR_VOL_UUID`. A kernel upgrade therefore never switches the source.<br>**`id`** — Windows: `FILE_ID_128`; Linux: `ino` (u64) ‖ `hgen` (u64), where `hgen` is the first 8 bytes of BLAKE3-256 over the `handle_type` (u32) and `f_handle` bytes that unprivileged `name_to_handle_at(AT_SYMLINK_NOFOLLOW)` returns — the handle encodes the inode generation on every file system Linux ids are trusted on, so a reused inode number yields a different `hgen` ([81] B2); macOS: the file id (u64) ‖ 0.<br>**`parent`**: the parent directory's id in the same encoding.<br>**`docid`**: the macOS document id, used only if owner decision #21 (d) enables it.<br>**Identity is equality of the whole `OsFileId`** (kind, `vol_key`, `id`); on Linux an inode number alone is never identity (§2.11.4 rule 8) |
| **Timestamps** in `FILEOBS` and `PENDING` | i64 ns since the Unix epoch plus a granularity byte; fields `mtime`, `ctime`, `btime` (optional) and `added` (optional; macOS `ATTR_CMN_ADDEDTIME`) |
| **`JOURNALCUR`** (replaces `USNCUR`) | `{kind u8 (0 none, 1 usn, 2 fsevents), vol_key [16], instance [16] (the UsnJournalID or the FSEvents device UUID), cursor u64}`; record kind `JournalCursor` (was `UsnCursor`) |
| **`DIRMAP`** (new; lazy record kind `DirMap`; section marked `derived-optional`) | `(tree key, directory OsFileId) → (root-relative path, mtime ns, granularity)`, ≈ 50–70 B per directory; written by settles, read by reads (I-F5) |
| **`TREES`** additions | canonical root bytes (P9), the root directory's `OsFileId`, `os` tag, a 16-byte `VolumeCaps` snapshot `{flags u32, id_kind u8, btime u8, case_rule u8, cloud u8, mtime_granularity_ns u64}` (the former u32 bit field could not hold the granularity, [81] m15) |
| **`FSINTENT`** holder | the intent anchor of §2.7.2 plus a diagnostic `ProcId` (replaces `pid, pid_start`) |

A row whose `kind` or `os` tag this OS cannot interpret is treated as absent. A store opened under another OS (a restored backup, for example) therefore behaves like a first settle and never produces a false hit [X19 §8.1].

#### 2.11.3 The changed-directory frontier (how Linux gets E3 and E3d without privilege)

**Why it is complete, and where it is not.** POSIX requires `rename` to update the mtime of both parent directories [D], and git's untracked cache relies on the same property [D]. So every new entry lies in a directory whose mtime changed, or inside a directory created since the last settle.
- A directory counts as changed when its mtime differs from `DIRMAP`, or when its mtime is at or after the **racy threshold**. The threshold is a file-system timestamp, never a process clock, which can lead a coarse file-system clock and miss a same-tick change ([81] m7): at the start of a settle the process rewrites one byte of `<store>/tmp/settle.stamp` and takes that file's mtime (as git takes its index file's mtime); when the tree lies on another volume than the store, it uses the newest directory mtime of the previous scan. Coarse volumes (HFS+ 1 s, FAT 2 s, Linux before 6.13) are covered by the measured `mtime_granularity_ns`.
- **The documented hole.** Tools that restore directory mtimes — `tar -p`, `rsync -t`, `cp -a` into existing directories — can hide a changed directory. A move hidden that way renders `missing` or `unverified`, never a wrong binding, until `links check --all` or the next full settle enumerates the tree.

**Algorithm, on a read or a settle:**
1. `statx` each `DIRMAP` directory of the tree. A missing directory is a moved or deleted source.
2. `getdents64` each changed directory, and recursively each new subdirectory found there.
3. Build `d_ino → path` for the entries found.
4. E3d: a stored parent-directory id found among the entries gives the directory's new path; E3: a stored file id gives the file's new path. **A `d_ino` hit counts only after `name_to_handle_at` on it yields the stored `hgen`** (one call per hit, est. µs): ext4 hands out the lowest free inode, and a directory and a file deleted together can come back as two unrelated objects with the same numbers ([81] B2). Only then do [40]'s verification rules (size and mtime, or `oid`) apply.
5. Budget exhausted → `unverified (budget)`.

**On Windows and macOS** `locate_id` makes the direct call, and the frontier feeds only E7, where it is a cheap tree-wide filter. Whether Windows M6 also uses `DIRMAP` for E7 on D:, which has no journal, is an M6 measurement; the format admits it either way.

#### 2.11.4 E1–E8 per OS (the cascade of [40 §4.3] is unchanged)

| Source | Windows (M6) | Linux (port) | macOS (port) |
|---|---|---|---|
| E1 intent, hooks | `FSINTENT`; `PENDING`; `PostToolUse` on `Bash(mv *)`, `Bash(rm *)` and `PowerShell(Move-Item \| Rename-Item \| Remove-Item *)` | same; the `Bash` patterns only | same; the `Bash` patterns only |
| E2 journal | not built (#41) | **none** | FSEvents per-device replay (`sinceWhen` → `HistoryDone`; cursor = device UUID + event id; `MustScanSubDirs`, a dropped event, a changed UUID or a cursor above the current id → history lost → frontier). Not built ([AR §11] #41, decided 2026-09-26; a later decision may add it); its trigger is likely to fire on macOS [I, X19 §4.5] |
| E3d parent-directory id | `OpenFileById(dir id)` | frontier search for the directory's `d_ino`, confirmed by its `hgen` | `fsgetpath(fsid, parent id)` |
| E3 file id | `OpenFileById(FILE_ID_128)` + path; exact on equal size and mtime or `oid` | frontier search for `d_ino`, confirmed by `hgen`; then the same verification | `fsgetpath(fsid, fileid)`; same verification |
| E4 near + copy rule | the creation-time line counts only when unique (rule 1) | the creation-time line never counts: at most STRONG | the creation-time line never counts: at most STRONG, and only when the candidate carries no clone indicator (`EF_MAY_SHARE_BLOCKS` clear, `ATTR_CMNEXT_CLONE_REFCNT` = 0) |
| E5 prefix, E6 git | OS-independent | same | same |
| E7 changed since the last settle | ChangeTime or CreationTime > last settle (plus `DIRMAP` if M6 adopts it) | ctime > last settle (rename sets ctime [S]), or inside a frontier directory | `ADDEDTIME` or ctime > last settle, or inside a frontier directory |
| E8 edited + moved | basename + sketch | same | same; with #21 (d) enabled, an equal document id in the scanned scope is **exact** |

**Rules clarified per OS.** These are frozen in R-14:
1. **Copy rule** ([81] M5). The line "`q` from E4 and `q.creation = FILEOBS.creation` → exact" applies only when `VolumeCaps.btime = TunneledNotCopied`, `q` shows no clone indicator, **`q`'s creation time is unique among the files the E4 enumeration saw, and it differs from the recorded creation time of every other file node in that scope**. Creation times are tick-granular, and a checkout creates many files per tick, so identical siblings (`pkg/LICENSE`, `pkg/sub/LICENSE`) share one. On Linux (`Unforgeable`) and macOS (`CopiedByClones`) the line never makes a candidate exact; at most STRONG. The Windows restriction also fixes the same pre-existing defect in [40]. (This supersedes [X19 §3.2]'s "equal btime with a different inode never happens".)
2. **Twins, then "spelling differs on disk"** ([81] M4). Nodes whose paths are equal under the directory's **actual** equivalence (case, normalization or both, as this volume observes it) and that another OS holds as distinct files — two live file nodes of the view, or a node and another entry of τ(H) — form a **twin set**. In a twin set, only the node whose recorded content (its `oid` in τ(H), or its `last_oid`) equals the `oid` of the file on disk resolves normally, even if the enumerated spelling is another twin's (git keeps the first name and writes the last content on a colliding checkout); every other twin renders `missing (not representable on this OS)` and is never re-bound; when no twin's content matches, or more than one does, every twin renders `ambiguous (case collision)` or `ambiguous (normalization collision)`. Outside twin sets: when the stored spelling stats but enumeration returns another spelling that is equal under the directory's equivalence, the link is `ok (spelling differs on disk)` and nothing is written; on a normalization-*sensitive* directory (NTFS, Linux without casefold), a missing path plus **exactly one** entry whose NFC form equals NFC(p) renders `ok (normalization differs on disk)`, and two such entries render `ambiguous (normalization collision)` [X19 §8.5]. No link is `ok` on a spelling match alone.
3. **Reads may use ids where stat returns them** (Linux, macOS). This adds exactness under the same "stat quadruple equals `FILEOBS`" rule; on Linux the id counts only with its `hgen` (one `name_to_handle_at` beside the `statx`).
4. **Sort before tie-breaking.** Every candidate list is sorted by exact path bytes, because enumeration order differs by file system (hash order on APFS and ext4, collation order on NTFS) [D/S, X19 §4.1].
5. **Never-candidate patterns** gain these temporaries and metadata files [I]: `._*` (AppleDouble), `.DS_Store`, `.fuse_hidden*`, `.nfs*`, `.goutputstream-*` (GIO atomic saves), `.~lock.*#` (LibreOffice). The per-OS trash locations join the Recycle Bin rule.
6. **`EXDEV` is the Unix form of "cross-volume".** It covers bind mounts, btrfs subvolumes and overlay lower directories [D/S]. [40 §3.4]'s cross-volume rules apply: a directory move is refused, and a file is copied → flushed → `oid` verified → deleted.
7. **Unix has no sharing violations.** [40 §8.3.1] matrix row 21 gains Unix variants: `EBUSY`, `EACCES`/`EPERM`, and a process whose cwd is inside a directory being renamed, which does not block the rename on Unix [I].
8. **An inode number alone is never identity** ([81] B2). E3, E3d and the path-reuse check of [40 §4.3] ([72 M13]) compare the whole `OsFileId`; on Linux that includes `hgen`, so `rm` and a re-create at the same path, or the lowest-free reuse of a directory's and a file's inode numbers, never yields an exact match.
9. **Denials are never absence** ([81] m12). `EPERM` or `EACCES` from `fsgetpath` (a MAC hook), from enumerating a TCC-protected folder, from `setiopolicy_np` or from a lock probe maps to `Unknown` or "source absent" — the source contributes nothing — never to `Gone` or `missing`.

#### 2.11.5 The resulting guarantee per OS

[40 §0.2]'s guarantee holds on every OS as written: **no link ever points silently at the wrong file or the wrong text**. Every non-`ok` link carries a state and a one-line next step, and a moved or deleted file is visible at the next read. What differs is when a move becomes exact. These differences are documented, not silent:

| Situation | Windows | Linux | macOS |
|---|---|---|---|
| Move through `file mv`/`file rm` | exact, one commit | same | same |
| Raw rename or move of a file on one volume | exact at read (E3, within the 20 ms read budget) and at settle | exact at settle (frontier), except after a tool that restores directory mtimes (then `missing` or `unverified` until a full settle). At read: exact when the frontier fits `files.read-budget-ms`, else `unverified (budget)` | exact at read and settle (`fsgetpath`) |
| Directory rename | one `OpenFileById` (E3d) | frontier: the parent's mtime changed, so one enumeration finds the directory inode, confirmed by its `hgen` | one `fsgetpath` (E3d) |
| Inode or file-id reuse (`rm` + re-create, lowest-free allocation) | a reused MFT slot has a new sequence number: a different id | a new generation: a different `hgen` | counter-allocated ids |
| Rename-over save (Claude Code `Edit`/`Write`, `sed -i`) | same path; `ok` with content changed; the id is refreshed at settle | same | same |
| Edit, then move | exact with the edit-evidence hook (`mcp_tool` transport), else an E8 proposal | same | same; exact through document ids if #21 (d) is enabled |
| Clone or copy, then delete the original | `identical copy` proposal unless git, intent or a **unique** equal creation time corroborates | `identical copy` proposal unless git or intent corroborates (birth times are not unique) | `identical copy` proposal: an equal creation time does not corroborate (clones) |
| Case or normalization twins from another OS | the twin whose content is on disk resolves; the others `missing (not representable on this OS)` | same, in casefold directories; distinct files elsewhere | same |
| Move across volumes or mounts (`EXDEV`) | copy + delete → proposal unless git or intent | same; this includes bind mounts and btrfs subvolumes | same, across volumes |
| Case-only or normalization-only renames | `ok (spelling differs on disk)` | same, in casefold directories; distinct files elsewhere | `ok (spelling differs on disk)` |
| Cloud placeholders | never hydrated (`RECALL_*`) | no standard; untrusted file-system types get no ids | never materialized (`SF_DATALESS`) |
| Deletion | absence never deletes | same | same |

### 2.12 Other OS-touching items

| Item | Windows | Linux | macOS |
|---|---|---|---|
| Detached `moirai gc --rollup` child | `BELOW_NORMAL_PRIORITY_CLASS`, `MEMORY_PRIORITY_LOW`, background I/O [AR §4.9] | `posix_spawn`; `nice` 10; `ioprio_set(IOPRIO_CLASS_IDLE)`; never spawned from a foreign PID namespace (§2.6) | `posix_spawn`; `setpriority(PRIO_DARWIN_PROCESS, PRIO_DARWIN_BG)`; `setiopolicy_np(IOPOL_THROTTLE)` |
| Bulk passes (rollup, backup, full export, retirement, repair, `links check --all`) | `MEMORY_PRIORITY_LOW`, `FILE_FLAG_SEQUENTIAL_SCAN`, `COPY_FILE_NO_BUFFERING` | `posix_fadvise(SEQUENTIAL)`, then `POSIX_FADV_DONTNEED` behind each chunk of files other than log extents (never on a log extent with unflushed groups) | `F_NOCACHE` and `F_RDAHEAD` on the descriptors |
| Store files' sharing | `FILE_SHARE_READ\|WRITE\|DELETE`; delete-pending tolerated; bounded retries on errors 5 and 32 | `unlink` is immediate; open and mapped inodes stay valid | same |
| Project-file reads (DR9, I-F11) | full sharing; closed before the next file | `O_RDONLY\|O_CLOEXEC\|O_NOFOLLOW`; `O_NOATIME` when the process owns the file (`EPERM` otherwise) | same, plus `IOPOL_ATIME_UPDATES_OFF` and `IOPOL_MATERIALIZE_DATALESS_FILES_OFF` for the process, and an `SF_DATALESS` check before every read |
| Console output | `WriteConsoleW` to a console, raw UTF-8 bytes to a pipe | UTF-8 bytes | UTF-8 bytes |
| Temporary files | always inside the store (`<store>/tmp/`), never `%TEMP%` | never `$TMPDIR`: sandboxed and unsandboxed commands see different values, and a rename across file systems fails with `EXDEV` [X20 §3.3] | same |
| User-scope config | `%APPDATA%\moirai\config` | `$XDG_CONFIG_HOME/moirai/config` (default `~/.config/moirai/config`) | same as Linux |
| Store `config` rewrite | `<store>/tmp/` then `rename_replace` and `durable-name` (§2.3.2) | same | same |
| Binary install | `%LOCALAPPDATA%\Programs\moirai\moirai.exe` (stable path, for Defender) | `~/.local/bin/moirai` | `~/.local/bin/moirai`, or a tap; ad-hoc signed by the linker; linked for macOS 14; never run from `target/` (XProtect scans every new binary [C]) |
| Hook and MCP entry point | Claude Code: `${CLAUDE_PLUGIN_DATA}/bin/moirai.exe`, a hardlink or copy (exec form needs a real `.exe`); Codex: `moirai` on `PATH` for the MCP entry's `command` in the Codex plugin's `.mcp.json` and for command hooks, which run under `cmd.exe /C` without inner quotes ([90 §3.7]) | `${CLAUDE_PLUGIN_DATA}/bin/moirai`, a symlink | same as Linux. A Dock-launched Claude Desktop has `PATH=/usr/bin:/bin:/usr/sbin:/sbin` [C, X20 §3.2], hence absolute paths; `SessionStart` appends a `PATH` export to `CLAUDE_ENV_FILE` (key `hooks.session-start.path-export`, user scope, default `true`, [AR §13]). Whether `${CLAUDE_PLUGIN_DATA}` is substituted in an exec-form hook `command` and in `.mcp.json` is **[I; verified by M0 item 7 and re-checked in M8]** ([81] m12); if it is not, `moirai hooks install` writes the expanded absolute path |
| Per-user writable state on the command path | none | none | none |
| Future Windows sandbox (`srt-win`: a separate local user [S, X20 §3.4]) | its files inherit the store directory's ACL; `OpenProcess` denied → Unknown; the pipe DACL denies it → direct path; its user-scope config differs, so a named root may render `unmapped root`, never a wrong answer | — | — |

### 2.13 Minimum OS versions, architectures and file systems

| | Windows | Linux | macOS |
|---|---|---|---|
| **Minimum** | **Windows 11, x64**, any feature release Microsoft still services; the gated build is the owner's (25H2, build 26200). The technical floor of the APIs used is Windows 10 1803 (`FileCaseSensitiveInfo`), but nothing older than Windows 11 is supported or tested. The former "22H2 or later" is dropped: no API needs it ([81] m15). Windows 10 reached end of support on 2025-10-14 [I] | **kernel ≥ 5.10**, 64-bit. The required path needs nothing newer than 5.3: OFD locks 3.15, `renameat2` 3.15 (ext4) and 4.0 (XFS), `statx` 4.11, `smaps_rollup` 4.14, errseq write-back error reporting 4.13–4.16, `pidfd_open` 5.3 (the MCP server's parent watch), `name_to_handle_at` 2.6.39 | **macOS 14 Sonoma**, the first public-SDK OFD release (§2.2.2); enforced by linking with a minimum OS of 14 and by a version check at open |
| Optional features, probed at run time; they only accelerate and never change semantics | — | `FALLOC_FL_WRITE_ZEROES` (ext4 6.17, XFS 7.3), `MADV_POPULATE_READ` 5.14, multigrain timestamps 6.13 (finer effective granularity) | — |
| Architectures | x64 gated in M0–M11; arm64 untested | x86_64 and aarch64, as **static musl** binaries (no glibc floor, no loader, no NSS) [X20 §4.2] | **arm64 only** (design default; macOS 26 is the last Intel release [C], and `x86_64-apple-darwin` is Rust Tier 2 [D]); an Intel slice would add a runner, which is part of the money decision #42 |
| File systems (store; §2.6) | NTFS gated; ReFS/Dev Drive refused until gated | ext4, XFS and btrfs gated in the port; ZFS, f2fs and bcachefs refused until gated | APFS (case-insensitive and case-sensitive) gated in the port; HFS+ refused until gated |
| Byte order and word size | little-endian, 64-bit | same | same |

- **Where these numbers live.** The minimums are a support statement recorded in [AR §1] row 14 and [AR §14]. Only macOS 14 shapes code (the OFD API), and nothing here shapes a frozen byte. The port phase may raise a floor, never lower it below the technical floors shown; a lower floor would need a flock or private-API fallback (macOS < 14) or lose errseq error reporting (Linux < 4.16).
- **Rust targets.** `x86_64-pc-windows-msvc` (Tier 1), `x86_64-unknown-linux-musl` and `aarch64-unknown-linux-musl` (Tier 2 with host tools, musl 1.2.5), and `aarch64-apple-darwin` (Tier 1) [D, X20 §4.1]. From M0 all four are type-checked on every merge (GT20 (e), owner decision #44, §5.5).

---

## 3. What M0 freezes for the ports

### 3.1 The exact list

Each item enters format v1, the protocol specification or the configuration specification at M0 exit, alongside [60 §2.5]'s rows. A port can then implement it without changing a byte or a rule.

| # | Item | Content (normative here) | Replaces or extends |
|---|---|---|---|
| **X-F1** | **`LOCK` file layout v1** (36 KiB, created only by `init`/`restore`, never deleted, never resized; the same bytes on every OS) | offset 0: `LockHdr` (64 B) `{magic "MLCK", format u16, flags u16, n_slots u16 = 256, slot_rec_size u16 = 128, role_base u64 = 2^62, slot_base u64 = 2^62 + 2^16, created_hlc u64, _ [20], xxh3 u64}`<br>offset 2048: `WriterDiag` (≤ 512 B) `{seq u64, proc ProcId, session_hash [16], cmd ≤ 400 B, hlc u64, xxh3 u64}`, written by the holder of the writer byte<br>offset 3072: `LeaderRec` (≤ 512 B, reserved; §2.8)<br>offset 4096: `SlotRec[256]` × 128 B `{ver u8, kind u8 (0 free \| 1 session \| 2 intent), os u8, flags u8, slot u16, _ u16, nonce u64, session_hash [16] (primary; BLAKE3-128 of the namespaced identity, [90 §4.4]), alias_hash [16], proc ProcId (32 B), parent {pid u32, _ u32, start u64}, acquired_hlc u64, _ [16], xxh3 u64}`, written only by the holder of slot i<br>**Lock bytes** (no data; beyond EOF): §2.2.3 (writer +0, leader +1, maintenance +2, quiet +3, **flush +4**, reserved to +63; slot i at 2^62 + 2^16 + i)<br>A reader accepts a record only if its checksum is valid, and re-reads it after probing (seqlock style) | [AR §4.1] `LOCK` row (bytes 0–3, 64–1087, the 1,024 × 32 B slot table) and [60 §2.5]'s two `LOCK` rows [72 B2]. Lock bytes beyond EOF keep a Windows backup, antivirus scan or `doctor` read of `LOCK` from failing [X18 §6.3] |
| **X-F2** | **Anchors, `ProcId`, boot identity and liveness** | `ProcId` (32 B, §2.7.1). `Anchor` (32 B, in `LEASES` and `FSINTENT`) `{kind u8 (0 none, 1 session, 2 intent, 3 leader, 4 session-ttl), os u8, slot u16, _ u32, id [16], boot_hash u64}`, where `id` is the 16 B `session_hash` (BLAKE3-128 of the namespaced process-lifetime identity `<harness>:<id>`, [90 §4.4]) for kinds 1 and 4, and `{nonce u64, session_hash_lo u64}` (the nonce, and the first 8 bytes of that hash as a diagnostic) for kinds 2 and 3, with a diagnostic `ProcId` in place of `pid, pid_start`. The liveness rules of §2.7.2, with `Unknown` never ending anything; `file mv` and `file rm` hold an intent slot. Lease `expires = {wall, boot_hash, mono}`, `mono` being the boot clock, which includes suspend. **The boot-identity rule** (constant for exactly one boot; invariant under wall-clock changes, suspend and hibernation; the per-OS sources of §2.7.1, never the boot time) and **Unknown-boot mode** for a process that cannot read it (no republish of `boot_id`, no boot recovery, lease boot tests `Unknown`, the wall deadline deciding) | [AR §4.2] `boot_id`, [AR §4.4] `LEASES` anchor, [AR §6.2], [40 §2.6] `FSINTENT`, [40 §3.4] step 5 |
| **X-F3** | **Group commit and chained group validity** | the protocol of §2.4.3: the flush byte; pending groups replayed only into a scratch layer; scan and re-write of the pending range under the writer byte; one flush outside it; the publish as a read-modify-write of the newest slot folding every covered group's `HEAD` effects; acknowledgement by identity after the covering publish; adoption of a stranded pending group by the next appender; maintenance's barrier only after its own `Checkpoint` is published; boot-change recovery in the lock order; invariants I-G1–I-G6. **Format:** the `group_end` record's 8-byte `chain` trailer (XXH3-64 over the group, seeded with the preceding chain value), groups never spanning extents (rotation pads with one `Noop` group), the seed XXH3-64(epoch) after `init` and every epoch re-roll; a group is valid only when its trailer matches | [AR §2.8] T8, [AR §4.2] boot check, [AR §4.3] validity rule, [AR §4.5] phase 2 and step 12, [AR §6.1], [AR §6.5]; [60 §2.5] Log row (validity) and decision (a) generalised |
| **X-F4** | **Lock contract and order** | §2.2.1 items 1–10 — including the user-space grant table (a second in-process client gets `Busy` or an in-process wait before any kernel call), waiter joining, probe denials as `Unknown`, and fairness outside the contract — and §2.2.3's order (slot < leader < maintenance < flush < writer; waits only on role bytes, only upward) | [60 §2.5] protocol decisions (new item (j)) |
| **X-F5** | **Durability-class tags and the fault model** | Record kinds keep one tag in the registry, `lazy \| durable` (`RecHdr.flags` bit 0 on disk, unchanged). The protocol points of §2.3.2 each carry one `Vfs` class (`durable`, `durable+meta`, `durable-name`, `sync_group`) with the per-OS calls of §2.3.1; every rename point is a no-replace rename plus `durable-name` on both parents, with `rename_replace` and `swap_dirs` where a point replaces or exchanges. No downgrade (`ENOTSUP` refuses); no direct-I/O path. `HEAD` barrier flushes outside the writer byte. `lazy` may be lost after a failed flush. Fault-model items (2), (3), (5), (7) and (8) amended, and (9)–(12) added (§2.3.5) | [60 §2.5] fault-model row and decisions (a), (c), (h); [AR §4.10] namespace bullet |
| **X-F6** | **Mapping policy and environment guard** | §2.5 rules 1–6 (sealed files read-only on disk; **`total_len` in every sealed file's header** and the size check against it; `FromBytes`-only access; the fault handler exiting 7). §2.6's allow-lists — every allowed file system crash-gated — and refusals, the `init` probe, the cheap check at open and the OS-version check | [AR §4.1] rules, [AR §4.4] `SegHdr`, [AR §4.10] |
| **X-F7** | **Path-key canonical form** | P1–P10 and P12 of §2.10 (P12: the machine-local form of root `abs`, drive letter upper-cased on Windows, leading `/` kept on Unix): P3 (NFC on normalization-insensitive volumes), P4, P5, the P6 `fold_v1 = NFD(full_casefold(NFD(x)))` at Unicode 17.0.0 in R-14, and the P9 canonical root per component with on-disk names, its root `OsFileId`, and lookups by root id first — for the `TREES` key, the `HEADS` binding key (`blake3_16(canonical directory)`) and the `git.worktree` provenance string | [40 §2.4], [40 R-14], [AR §5a.1] client-head key, [AR §5e.2] |
| **X-F8** | **R4 runtime layouts and resolver rules** | §2.11.2 (`OsFileId` with the fixed-source `vol_key`, the Linux `ino ‖ hgen` id and whole-id identity; timestamps; `JOURNALCUR`/`JournalCursor`; `DIRMAP`/`DirMap`; the `TREES` additions with the 16-byte `VolumeCaps` snapshot; the `FSINTENT` holder). R-14 gains rules 1–9 of §2.11.4: the copy rule's unique-creation-time condition, the twin rule and "spelling differs on disk", reads with ids, sorted candidates, the never-candidate additions and per-OS trash locations, `EXDEV`, Unix busy states, "an inode number alone is never identity", and denials as `Unknown`; the frontier's racy threshold from a file-system timestamp | [40 §2.6], [40 R-7, R-8, R-14, R-16, R-18] |
| **X-F9** | **Image and ref names** | P11 (a) `schema/queries/<q>.moi` with q = hex(BLAKE3-256(name))[0..32]; P11 (b) ref segments are not Windows device names and do not end in `.lock`, ref-name input is NFC-normalised, and a ref name fold-equal to a live one is refused | [AR §5b.1], [50 §4.4], [50 F3], [60 §2.5] F3 row; LQ `ref_word` stays as is |
| **X-F10** | **Store-layout rule** | every store file name is built from decimal numbers and fixed ASCII words (P11 c); `seg.b<ref_id>.K` | [60 §2.5] "Store layout" row |
| **X-F11** | **Configuration locations** | **Frozen:** the user-scope configuration file per OS (§2.12: `%APPDATA%\moirai\config`; `$XDG_CONFIG_HOME/moirai/config`, default `~/.config/moirai/config`, on Linux and macOS). **Registered in [AR §13] at M0, not frozen** — keys and their defaults stay changeable like every operational key, since [AR §13] freezes the syntax, precedence, unknown-key rule and registry format, not the key set: `lock.flush-wait-ms` (int, 2,000, store, hot); the `image.dest.<name>.path` default `<parent of the main worktree>/<project>-moirai.git`; `files.portable-names` (§2.10 P5); `hooks.session-start.path-export` (§2.12). "Never a key" gains the durability mapping, the lock layout and order, the liveness and boot-identity rules, and the environment allow-list | [AR §13] |
| **X-F12** | **CLI transport contract** | T1–T10 of §4 (argv alphabet, stdin decoding, output bytes) join the frozen output contract; output is byte-identical across OSes (LF, UTF-8) apart from the golden-file substitutions of T7 | [AR §7.1], [50 §6.2] |

### 3.2 Checked, and needing no change for the ports

Each item was checked against X1–X9. **Confirmed unchanged:**

| Area | Why the ports need nothing |
|---|---|
| Canonical commit form [AR §4.6], trailers [AR §5b.4], `.moi` codec [AR §5b.2] | bytes are defined without reference to the OS. The provenance strings are data carried by trailers; X-F7 only fixes their spelling |
| Record header, commit record, ops, values, section layouts, `hist` frames | little-endian and fixed-width; no page-size dependence (whole-file maps; 8-byte section alignment is enough on 4 KiB and 16 KiB pages). Two additions are frozen with X-F3 and X-F6, not changes to these layouts' fields: the `group_end` record's chain trailer and `total_len` in sealed-file headers |
| `HEAD` slot | `boot_id [16]` is opaque per OS (§2.7.1); `durable_lsn` and `committed_lsn` already carry group commit. **No field is added**; the publish rule changes (X-F3) |
| Epoch, the recovery scan, the boot check | OS-independent, and extended, not replaced, by X-F3's chain rule and lock order |
| Tree names in the image (except X-F9), refs, side refs, the `.moirai-image` marker | hex and fixed ASCII; git's own rules apply, and X-F9 adds the fold rule for ref names |
| The git object layer (loose objects, packs, idx, commit-graph, refs `.lock` protocol, bundles) | git's formats are OS-independent. Its file I/O goes through `Vfs` classes (§2.3.2), with `rename_replace` for the lock protocol. File modes in the image are `100644` and `040000` only |
| `oid`, `is_text`, fingerprints, anchors, LQ grammar, error table, envelope, exit codes 0–10, MCP protocol, hook JSON | byte-defined. A store I/O fault reuses exit 7 (§2.5) |
| HLC and timestamps | milliseconds since the Unix epoch, UTC; FILETIME is converted in `os::proc` |
| Config syntax (git-config) | LF or CRLF accepted on read, LF written; path values written with `/` |
| Pointer files (`moiraidir:`, `store-id:`) | machine-local like bindings. `init --link` writes a relative path whenever the target is reachable relatively [I] |
| The `Vfs` and `ProjectFs` simulators | they implement the weakest fault model (§2.3.5). Per-OS `ProjectFs` simulator profiles (ext4 with lowest-free inode reuse and random generations, APFS) belong to the port (§5.2); from M6, GT2 sweeps `VolumeCaps` over the three OS profiles as pure data (§5.5 c) |
| `Store` API, reference model | OS-independent. The model takes `VolumeCaps` as an input for the R4 rules above |

---

## 4. Shells and CLI rules (bash, zsh, dash, fish, PowerShell, cmd)

**Which shells actually run moirai** [S, X20 §2.1, §3.1]:
- **The agent's Bash tool.** Claude Code runs it only in **bash or zsh**: `CLAUDE_CODE_SHELL`, else `$SHELL` if it is one of those, else a search. That means zsh on macOS, bash on Linux, and Git Bash on Windows. Each command runs non-interactively through a wrapper that sources a snapshot and then turns off `extglob` (bash) or `EXTENDED_GLOB` and `BARE_GLOB_QUAL` (zsh). zsh's `NOMATCH` stays on. Stdin is `/dev/null`.
- **The PowerShell tool** runs Windows PowerShell 5.1 or PowerShell 7.
- **Hooks:** in shell form, `sh -c` on Unix (dash on Debian and Ubuntu, bash 3.2 in POSIX mode on macOS) and Git Bash on Windows. In exec form, no shell at all.
- **Humans** may also use interactive zsh (possibly with `EXTENDED_GLOB`), fish, PowerShell 7 on Unix, and cmd.

### 4.1 Behaviour of the characters moirai's syntax uses

| Construct | bash, Git Bash | dash (`sh -c`) | zsh as the agent runs it | interactive zsh, `EXTENDED_GLOB` | fish | PowerShell 5.1 / 7 | cmd |
|---|---|---|---|---|---|---|---|
| `#` at the start of a word | comment [M] | comment [M] | comment [S] | literal unless `INTERACTIVE_COMMENTS` [D] | comment | comment [M] | literal |
| `#` inside a word (`scope=#88`) | literal [M] | literal [M] | literal [S] | **glob operator**; no match is an error [D] | literal | literal [M] | literal |
| `~x`, `=x`, `!x` at the start of a word | literal if no such user [M]; `!` is history only interactively | literal [M] | `~name`: **error**; `=40`: **error** [S] | same | `~` expands | literal | literal |
| `main~5`, `main^2` | literal [M] | literal [M] | literal [S] | **glob operators** [D] | literal [C] | literal [M] | `^` is **eaten** (escape character) → `main2` |
| `*`, `?`, `[x]` unquoted | **silently replaced** by matching file names [M] | same [M] | **error "no matches found"** when nothing matches [D] | same | error (`*`); `?` depends on the version [C] | literal | literal (the program receives them) |
| `{a,b}` | expands [M] | literal [M] | expands | expands | expands | script block [M] | literal |
| `<`, `>`, `\|`, `&`, `;` | redirections and separators | same | same | same | same | pipe; `<` is an error in 5.1 [M] | same |
| `$x`, `` `x` ``, `(x)` | expand | expand | expand | expand | `$x`, `(cmd)` expand | `$x`, `(x)` at a token start | `%x%` expands, even inside quotes |
| `@x` at the start of a token | literal | literal | literal | literal | literal | **splat**: silently vanishes, or a parse error [M, 16 §6.10] | literal |
| `'single quotes'` | literal [M] | literal | literal | literal | literal | literal; inner `"` stripped by 5.1 [M] | **not quotes**: `'a b'` becomes two arguments |
| `"double quotes"` | `$` and `` ` `` expand | same | same | same | `$` expands | `$` expands | quotes; `%` still expands |
| `<<'EOF'` quoted heredoc | byte-exact [M] | **corrupts UTF-8** in dash 0.5.13.x [M, X20 §2.4] | byte-exact [D] | byte-exact | **none** | here-string `@'…'@` (UTF-8 with a BOM through Claude's tool [M]) | none |

### 4.2 The rules (normative; they join the frozen contract, X-F12)

| # | Rule | What it replaces or tightens |
|---|---|---|
| T1 | **Ids in argv are bare integers** (`show 40`, `scope=88`, `ids=40,41`). `#N` appears only in stdin, in `-f` files and in MCP strings. moirai still **accepts** `#40` and `scope=#88` when they arrive, but the skills never teach them | [50 §6.1] called `scope=#88` safe [M on Windows]; it is not safe in a human zsh with `EXTENDED_GLOB` [X20 §2.6] |
| T2 | No argv token **starts** with `#`, `~`, `=`, `@` or `!`, and none starts with `-` unless it is a flag. A token that is not a file path never starts with `/` (Git Bash rewrites such tokens into Windows paths [M, 16 §6.10]); a genuine file path may. The `~main` marker appears only in output: revision input never starts with `~` | extends [AR §7.1]'s "no argument starting with `/`" |
| T3 | Argv never contains an unquoted `*`, `?`, `[`, `]`, `{`, `}`, `(`, `)`, `<`, `>`, `\|`, `&`, `;`, `$`, a backtick, `"`, `\` or `%`. **Values with spaces are avoided in argv.** The skills pass free text on stdin; where a value with spaces must be in argv, the form is `'…'` in POSIX shells, fish and PowerShell, and `"…"` in cmd | [50 §6.2] rules 1 and 3; cmd added |
| T4 | `main~5`, `main^2`, `a..b` and `a...b` are safe for agents on every OS. The human documentation says to quote them in zsh with `EXTENDED_GLOB`, and in cmd (`"main^2"`) | new |
| T5 | **Free text travels on stdin** from a **quoted** heredoc (`moirai q - <<'EOF'`) in bash and zsh, which covers every agent shell on every OS. **On Linux and macOS agents never write query files**: a sandboxed and an unsandboxed process see different `TMPDIR`s, and the Write tool cannot expand the variable ([81] m11). Scripts and humans use `-f PATH` or a pipe: `@'…'@ \| moirai q -` in PowerShell, `moirai q - < file` in cmd, `printf '%s\n' '…' \| moirai q -` or `-f` in fish, `-f` in `sh` scripts and hooks; a script's temporary query file goes under the store's `tmp/`. On Windows the PowerShell tool may still write `%TEMP%\moirai\q.lq` for queries with non-ASCII literals ([50 §6.2] rule 2). MCP strings are immune. An unquoted `<<EOF` is never shown | [50 §6.2] rule 2, made cross-OS |
| T6 | **Stdin decoding is identical on every OS:** one leading BOM is stripped; invalid UTF-8 exits 2 (this turns the dash `CTLMBCHAR` corruption into a loud error); the PowerShell `?` warning is kept; stdin is never read without `-`/`--stdin`, because the Bash tool's stdin is `/dev/null` | [AR §7.1], [50 §6.2] rule 6 |
| T7 | **Output** is UTF-8 with LF, never ANSI off a TTY, and **byte-identical across OSes**: one set of golden files, with three defined substitutions applied before comparison ([81] m15) — the tree root (`<ROOT>`), OS error texts (`<OSERR>`), and the OS-specific detail of an R4 state (`<OS-DETAIL>`, e.g. the name of the trash location). A broken pipe on stdout exits quietly with 0 | new; [AR §7.1] |
| T8 | **Hooks and MCP use the exec form** (an argv array) with an absolute path (`${CLAUDE_PLUGIN_DATA}/bin/moirai[.exe]`, or the expanded path if the substitution fails its M0 check). No shell sits on any hook path | [07 §9.5], [AR §7.5] |
| T9 | Error texts that name a shell problem say which one (`hint: '#' starts a shell comment; write 40`; `zsh: no matches found → quote the value or use stdin`) | [50 §6.2] rule 4, widened |
| T10 | GT12 runs T1–T9 through the real shells with Claude Code's wrapper. In M0–M11 that is Windows: Git Bash, PowerShell 5.1 and PowerShell 7, plus cmd for the human documentation's forms. The port adds `bash -c` on Ubuntu, `dash -c` for shell-form hooks, and `zsh -c` on macOS both with and without a user `EXTENDED_GLOB` set before the wrapper's reset | [60 §3.13] GT12 |

---

## 5. The port phase (documented, not scheduled)

### 5.1 Status and entry

- **Not scheduled.** The port phase is not in the M0–M11 calendar and not part of the release gate RG1–RG12 [60 §6]. It never blocks the release or the cutover. The same post-release phase carries the Windows OS-crash rig that the owner review of 2026-09-27 deferred ([AR §11] #34, V7: GT15 with its calibration, [AR §8.2] item 17, ≥ 1,000 then ≥ 5,000 cumulative cycles on the Server 2025 Core guest), which never blocks the release either; every format field and protocol rule it tests is frozen at M0, like the ports'. That follows owner decision #32: "Don't build binaries for Linux and Mac for now … we are not testing this for now".
- **Entry criteria.** The Windows release gate is met, so the ports build on a certified engine and re-run gates rather than design anything; the owner has answered #42 (port-phase hardware, CI and platform coverage); the prerequisites of §5.4 are available.
- **What a port never does.** It never changes the format or the protocol (X8); it never adds a key that weakens a guarantee (X5); it never lowers a gate volume without an owner decision — the same RG criteria apply per OS.

### 5.2 Scope per OS

| Work | Linux | macOS |
|---|---|---|
| **First: probes before any code** | the M0-style measurements on the port's reference hardware: flush floors (`fdatasync` on zero-filled, `WRITE_ZEROES` and unwritten extents; `sync_dir`); the 16-writer group-commit burst **and its fairness**; lock release after `SIGKILL`, **and after `SIGSEGV`/`abort` with `systemd-coredump` on**; the `statx`, `getdents64` and `name_to_handle_at` walk and frontier costs on ext4, XFS and btrfs; the **effective mtime granularity** per file system and kernel; the empty-binary RSS floor; spawn through `bash -c` with a realistic snapshot; the system allocator on glibc and on musl under the one- and two-thread load moirai's processes run (mimalloc and jemalloc are excluded by owner decision #44, §2.9) [X17 §8], [X18 §10], [X19 §8.7], [X20 §4.2] | `F_FULLFSYNC` against `fsync` and `F_BARRIERFSYNC` at 1, 4 and 16 processes (reported only; the class is fixed); the group-commit burst and its fairness; OFD behaviour on the target macOS versions; lock release after a crash **with ReportCrash on**; `getattrlistbulk`, `fsgetpath` and FSEvents replay latency (cursors 1 h, 1 day, 1 week old; whether history carries inodes); APFS ctime on rename, clone creation time, document-id transfer; the RSS floor; spawn through `zsh -c` after a warm-up launch; the minimum-OS enforcement (a macOS 13 machine or image refuses) |
| **Sandbox probes** (Claude Code) | inside bubblewrap: OFD acquisition on `.git/moirai/LOCK`; the session anchor seen across two sandboxed commands; `boot_id` readable; `socket(AF_UNIX)` refused → direct path; writes in the main checkout and a linked worktree; the refusal text in a subdirectory session; `image export`'s `allowWrite` text; no rollup child from a foreign PID namespace | inside Seatbelt: whether `fcntl` locks are permitted (a "file-lock" operation) [X18 §7.2]; the anchor across commands; **whether `kern.bootsessionuuid` is readable** (else Unknown-boot mode, verified end to end); `sysctl(KERN_PROC_PID)` allowed; Unix sockets denied → direct path; TCC-protected roots (`~/Documents`) mapping to `Unknown`; iCloud-managed `~/Desktop`/`~/Documents` detection |
| Shared Unix layer (`os/unix`) | file I/O and the durability classes; `rename_replace` and `swap_dirs`; `LockBytes` over OFD with the waiter thread; `map_sealed` with the `SIGBUS` handler chained to Rust std's; `ProcId`; `TestHost` (`SIGKILL`, `SIGSTOP`/`SIGCONT`, loop-image disk-full, injected clock); CLOEXEC discipline | reused |
| OS-specific `Vfs` | `fdatasync`/`fsync`; `create_extent` (`WRITE_ZEROES` or zero-fill; btrfs sparse, `NOCOW` if measured); the `statfs` allow-list with `mountinfo` checks; `/proc` and `boot_id`; `CLOCK_BOOTTIME`; the `pidfd` parent watch; cgroup-v2 and `smaps_rollup` metering | `F_FULLFSYNC` everywhere durable; sparse extents and `F_PREALLOCATE`; `MNT_LOCAL`, File Provider and iCloud-folder refusal; `kern.bootsessionuuid` with Unknown-boot mode; `mach_continuous_time`; `kqueue` parent watch; `phys_footprint` metering; `otool -L` link check; the minimum-OS link setting and version check |
| `ProjectFs` | `statx`, `getdents64`, `name_to_handle_at` handle digests, the `DIRMAP` frontier with the file-system racy threshold, casefold detection and per-component canonical roots, `renameat2`, `EXDEV` handling, the freedesktop trash, the file-system trust table, `O_NOATIME`; the **ext4 profile** of the `ProjectFs` simulator (lowest-free inode reuse with random generations) | `getattrlistbulk`, `fsgetpath`, `VolumeCaps` from `VOL_CAP_*`, NFC precomposition (P3), per-component canonical roots, clone indicators and the copy-rule restriction, `SF_DATALESS` and I/O policies, `renamex_np`, trash, TCC and sandbox denials as `Unknown`; the **APFS profile** of the simulator; FSEvents E2 only if a later decision reverses #41 (not built, 2026-09-26); document ids only if #21 (d) says so |
| Distribution | static musl for x86_64 and aarch64; the allocator from the matrix | arm64 binary linked for macOS 14 (an Intel slice only if #42 buys its runner); ad-hoc signature by the linker; notarization only if distributed (#39, #42) |
| Common | `doctor` per OS; install and `hooks install` per OS; GT12 per shell (§4 T10); the G-X cross-OS format gate (§5.3) | same |

### 5.3 Port gates (per OS; the same volumes as Windows unless the owner decides otherwise)

| Gate | Linux | macOS |
|---|---|---|
| `Vfs` conformance (the analogue of M1's certification point) | durability per class, `sync_dir`, `sync_group`, `rename_replace`, `swap_dirs`; the `LockBytes` contract items 1–10, including the in-process two-client case, waiter hand-off and an abandoned waiter's release; release at death; `ENOTSUP` and environment refusals; the mapping fault handler; the `total_len` size check | same, plus the `F_FULLFSYNC` `ENOTSUP` path on an SMB or FAT volume and the macOS 13 refusal |
| GT1, GT3 (simulator; OS-independent) | already certified in M1, the chained-group and lost-group scenarios included; re-run on the port CI as a regression signal | same |
| GT2 differential | engine against model on this OS: commit ids byte-identical; the link differential with this OS's `VolumeCaps` | same |
| GT4 kill loops (via `TestHost`) | `SIGKILL`; `SIGSTOP`/`SIGCONT` of random processes for 1–120 s; disk-full on a loop-mounted ext4, XFS or btrfs image; clock steps through the injected clock; **external truncation of a sealed file** (readers exit 7, nothing lost, `doctor --fsck` names the file); a lease held from a sandboxed PID namespace stays Alive or Unknown, never expires early | same with `hdiutil` APFS images |
| GT15 OS-crash (calibrated per rig: (a) unflushed data lost at least once, (b) flushed never, (c) whether issued-but-unflushed writes are lost [X20 §7.5]) | L2: **LazyFS** loop (page-cache loss without reboot; nightly; test build only, since the guard refuses FUSE in product builds). L3: **dm-log-writes** replay to every flush and to random points between flushes on ext4, XFS and btrfs, plus **dm-flakey** `error_writes` (flush errors, followed by continued appends — the B1 scenario on real kernels). L4: **QEMU/KVM** power-off loop (`cache=none` or `unsafe` on disposable overlays; SIGKILL, QMP `quit`, `sysrq-c`), ≥ 1,000 cycles at the port exit and ≥ 5,000 cumulative for the port release | a **Virtualization.framework** guest (`VZDiskImageSynchronizationMode.full`; power-off = SIGKILL of the VM process; tart or UTM) on Apple-silicon hardware, ≥ 1,000 cycles at the port exit; hosted arm64 runners cannot nest VMs [D, X20 §5.1]. Issued-but-unflushed loss is not observable here; that is covered by the simulator and by the Linux dm-log-writes runs of the shared Unix code [X20 §7.3] |
| GT11 budgets | [AR §8.3]'s per-OS notes on floor-grade hardware (consumer NVMe, not a hosted runner) | same on the Mac (M3-or-newer; M1 as the worst case if available) |
| GT12 shells | `bash -c` (Claude's wrapper), `dash -c` (shell-form hooks), fish forms from the documentation | `zsh -c`, with and without a user `EXTENDED_GLOB` set before the reset; bash 3.2 `sh -c` |
| GT17 pattern matrix | ext4, XFS, btrfs with a subvolume boundary, a casefold directory (privileged job), overlay lower-directory rename, bind-mount `EXDEV`, an inode ping-pong under atomic saves, **directory-and-file inode reuse, empty-file reuse, `rm` + re-create at the same path**, NFC/NFD twins, **case twins from a Linux commit**, identical files created in one tick, a non-UTF-8 name, an mtime-restoring `tar -p` | case-insensitive APFS, case-sensitive APFS (`hdiutil`), `cp -c` then `rm`, **case and NFC/NFD twins**, `/tmp` against `/private/tmp` roots and a case-variant `cd`, NFD names, a dataless iCloud file, a root under `~/Documents` [X19 §8.7] |
| GT14 soak | 72 h, as RG3 | 72 h |
| **G-X cross-OS format identity** (every port PR once two OSes exist) | each OS writes a seeded store, a `backup` and an image. Every other OS restores the backup, opens the store where the guard admits it, and imports the image. Canonical commit ids, image objects and `doctor --verify` must agree byte for byte, and runtime rows with a foreign OS tag must read as absent [X20 §8] | same |

### 5.4 Size, calendar and prerequisites

**Size of the port phase** (est.; [22 §7.1] units at ≈ 3 per 1k lines including tests, as [60 §3]):

| Work | Units |
|---|---|
| Probes and sandbox probes (§5.2) | 2.5–4.5 |
| Shared Unix OS layer | 7–11 |
| Linux-specific `Vfs` and metering | 3–5 |
| Linux `ProjectFs` + ext4 simulator profile (handle digests, per-component roots, the twin rule's observations) | 9.5–13 |
| Linux CI and crash layers (hosted runners, LazyFS, dm-log-writes, dm-flakey, QEMU/KVM loop and calibration) | 5–8 |
| Linux gate runs and fixes (GT2, GT4, GT11, GT12, GT17, G-X) | 4–7 |
| Linux distribution (musl, allocator) | 1–2 |
| **Linux subtotal** | **32–50.5** |
| macOS-specific `Vfs` and metering | 3–5 |
| macOS `ProjectFs` + APFS simulator profile | 9.5–12.5 |
| macOS CI and VZ rig with calibration | 4–7 |
| macOS gate runs and fixes | 4–7 |
| Signing and notarization (only if distributed) | 0–1 |
| **macOS subtotal** (after the shared layer exists) | **20.5–32.5** |
| **Both** | **≈ 52–83 units** |

- **Calendar.** At 5–8 units per week: Linux ≈ 4–10 weeks, and macOS ≈ 2.5–6.5 weeks after it. Both together ≈ 6.5–16.5 weeks with one lane, or ≈ 5–10 weeks with two lanes once the shared Unix layer exists.
- **Not included:** hardware lead time, and fixing any engine defect the ports reveal (each such defect re-runs the Windows gates of the crates it touches, [60 §6] RG3).

**What designing for three OSes adds to M0–M11 now** (est.; included, with the audits' delta and [90]'s, in [60 §7]'s calendar as re-issued on 2026-09-26; the type-check row moved to M0 by owner decision #44):

| Milestone | Added work | Units |
|---|---|---|
| M0 | the three-OS OS-layer specification and mapping appendix; `LOCK` v1; group commit with chained validity, identity acknowledgement and read-modify-write publish in the specification, the model and the simulator (fault-model items 3, 11 and 12) with the thirteen seeded bugs and the lost-group GT1 scenario; the fault-model amendments; the boot-identity rule and Unknown-boot mode; path rules P3–P12, the twin rule and the copy-rule restriction in FL-1 and its tests; the tagged R4 layouts; the review | 5–7.5 |
| M1 | the flush byte and group commit (scan and re-write under the writer byte, the chain trailer, identity checks, read-modify-write publishes); the in-process grant table; per-role handles; the in-page-error handler; `total_len` checks; read-only seal and GC's clear-then-delete; `rename_replace` and `swap_dirs`; the NTFS allow-list and identity check; the Windows boot counter; the OS-layer lint | 2.5–5 |
| M6 | `VolumeCaps` as the resolver's input; tagged `OsFileId` rows; the twin rule and the unique-creation-time copy rule on Windows; the `VolumeCaps` sweep in GT2 | 1–2.5 |
| M10 | the MCP server's parent-process watch | 0.5 |
| M0 | the cross-target type check GT20 (e), a gate (§5.5 b; owner decision #44) | 0.5–1 |
| **Total** | | **≈ 9.5–16.5** (≈ 1.2–3.3 weeks on the one-lane critical path) |

Group commit also makes it less likely that the M0 measurements require the leader. If it removes the leader from M1, it saves [60 §3.2]'s 3–4 units.

**Prerequisites of the port phase:**

| Need | Linux | macOS |
|---|---|---|
| CI | GitHub-hosted `ubuntu-24.04` x64 and arm64 runners with `/dev/kvm` (free for a public repository; ≈ $0.006/min private, and the nightly crash layers ≈ $54/month) [D, X20 §5.2], or a self-hosted Linux machine. Repository hosting was decided on 2026-09-26 ([AR §11] #36): public GitHub, so these runners are free | hosted `macos-14`/`macos-15`/`macos-26` arm64 runners for PR-level runs (free if public, $0.062/min private) [D] |
| Floor-grade hardware for GT11 | a Linux install on consumer NVMe without power-loss protection (a VM is not floor-grade); #34 bought no test host (profile L, 2026-09-26), so a dual-boot partition on the owner's laptop, or a machine bought under #42 | an Apple-silicon Mac: a Mac mini M4 ≈ $599 once, or cloud Macs (Scaleway M4 ≈ €0.22/h, AWS `mac2.metal` ≈ $0.65/h, each with a 24 h minimum) [C, X20 §7.3] |
| OS-crash rig | hosted runners are enough (KVM, dm-*, LazyFS) | a physical or bare-metal Mac (VZ; nested virtualization is unavailable on hosted arm64 runners); the macOS licence allows two extra VMs per Mac [C] |
| Claude Code | a Linux session with the sandbox enabled, for the sandbox probes | a macOS session with Seatbelt |
| Money (#42) | none: the repository is public ([AR §11] #36); (≈ $270/month would have applied to a private one at [X20 §5.2]'s volumes) | the Mac, plus $99/year for the Apple Developer Program only if moirai is distributed; an Intel runner only if Intel Macs are to be supported |
| Windows rig | deferred to after the release by the owner review of 2026-09-27 (V7), with the WSL2 and Memory Integrity choices it raises; when it runs on the owner's laptop ([AR §11] #34, profile L), WSL2 forces VirtualBox onto the slower Windows Hypervisor Platform backend [C, X20 §7.4], so the rig is faster with WSL2 off, but switching it off on the laptop is the owner's call, and item 17's calibration decides whether the rig is sound in the laptop's state | — |

### 5.5 Guards against Windows-only code creeping in during M0–M11

The owner ruled out Linux and macOS binaries and tests now. None of the measures below builds a binary for another OS or runs a test there.

| Option | What it is | Cost | What it catches | What it cannot catch |
|---|---|---|---|---|
| **(a) OS-layer lint: a gate, GT20 (d), from M1** (it runs on Windows and builds nothing for another OS) | **Dependency part**, over the **direct** dependencies of workspace crates (`cargo metadata`): only `moirai-os` and the binary crate's entry glue may declare `windows-sys` or `libc`. Transitive use by third-party crates — tokio in the MCP front-end (T10) — is allowed only through a reviewed allow-list in the repository (crate, version range, reason), which a dependency update must extend by review; an unlisted third-party path to either crate fails the lint. **Source part:** a scan finds `cfg(windows)`, `cfg(unix)`, `cfg(target_os)`, `std::os::windows` and `std::os::unix` only inside `moirai-os`, and bans `File::lock` and `std::fs::rename` on store and project files | ≈ 0.5 unit to write; seconds per build | OS APIs and conditional compilation leaking into shared crates | semantic assumptions |
| **(b) Cross-target type check: a gate, GT20 (e), from M0** (owner decision #44, 2026-09-26: "Do cargo check for Linux and Mac.") | `cargo check --workspace --all-targets --locked` for `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl` and `aarch64-apple-darwin` beside the Windows host, over every workspace crate except the binary crate (a composition root wiring `moirai-os` into the checked `moirai-app`) and the reviewed host-only oracle and tool crates, with every target's C, C++ and assembler poisoned, in the local pre-merge gate and PR CI ([90 §11]). It produces `.rmeta` metadata only: **no binary, no linking, no test run**, so it is inside decision #32's words. Shared crates depend on the `Vfs` and `ProjectFs` traits, not on `moirai-os`; the binary crate is a composition root only — its `main.rs` wires `moirai-os` into the checked `moirai-app` ([90 §11.1]). `cargo check` runs build scripts, so the check also enforces decision #44's pure-Rust rule: `zstd` leaves (the codec is decided among pure-Rust options at M0, [90 §11.3]), `blake3` uses `pure`, every build script needs a reviewed allow-list entry, and no crate is excluded to hide native code | `rustup target add` three times (≈ 100–200 MB installed each, est.); a separate target directory (≈ 0.5–1 GB over time, est.); ≈ 1–3 min per target cold and ≈ 5–30 s incremental per run (est.); ≈ 0.5–1 unit to set up | Windows-only `std` APIs (`std::os::windows::*`, `OsStrExt` from the Windows prelude), Windows types in shared signatures, `cfg` mistakes, trait items missing for other targets, and dependency features that exist only on Windows; any C, C++ or assembly in a checked crate's dependency graph | case-insensitivity or `\` assumptions in string handling, lock or durability semantics (these rest on the frozen rules, the weakest-OS simulator and the `VolumeCaps` sweep) |
| **(c) `VolumeCaps` sweep in GT2: adopted in M6** | the link differential runs with the Windows, Linux and macOS capability profiles as input data (pure logic, on Windows) | ≈ 1 unit | a resolver rule that silently depends on a Windows capability (the copy rule on Linux birth times and macOS clones, the twin rule on APFS) | real OS behaviour |
| (d) Nothing further | — | 0 | — | the port starts with an unknown amount of Windows-specific code in shared crates; [74 A01] notes that fixing it later is a larger change |

**Decision:** (a) is a gate, GT20 (d), from M1; (c) is a design rule; and (b) is a gate, GT20 (e), from M0 by owner decision #44 (2026-09-26). With (a) and (b), a port starts by adding the `os/unix`, `os/linux` and `os/macos` modules, and the rest of the workspace already type-checks for those targets.

---

## 6. Edit record (applied 2026-09-26)

- **Status.** The first revision of this section was an edit list to apply after acceptance. Revision 2 fixed the design ([81]; §8) and **applied** the edits in the same pass, each anchor matched exactly once and replaced by a script. The entries below record what changed where; the normative text is now the one in the target document. This is a historical record: decision states it quotes (for example "Due before M0") are superseded by later passes and by the owner's answers of 2026-09-26 (§8.5). "[80]" means this file.
- **Conventions.** Labels AR-n, 40-n, 50-n and 60-n are kept from revision 1 where the edit existed there; new edits carry new numbers. Where [81] caused an edit or changed its text, the finding is named.
- **Checks after applying** are in §6.6.

### 6.1 `docs/ARCHITECTURE-RESEARCH.md` [AR]

| Label | Section | Change |
|---|---|---|
| AR-1 | header | fifth amendment: owner decision #32 and [80] rev. 2, summarised in the new §14 |
| AR-2 | sources; how to read | the [X17]–[X20] row records #32 as decided; rows [80] and [81] added; "For implementers" names §14 |
| AR-3, AR-4, AR-5 | §0 bullets 2 and 10, "Why this design" | the writer lock per OS; group commit through the flush byte with chained groups and identity acknowledgement; the 3–20 ms Apple-silicon flush; the OS layer in M0; the port phase after M11 |
| AR-6 | §1 row 14 | replaced: "Windows, Linux and macOS" with the OS layer, the minimums and the budget and gate columns (GT1 lost-group scenario, GT4, GT15, GT20 d; the port gates, unscheduled) |
| AR-6b ([81] M7) | §1 row 18 | "exactly one flush per durable commit" → group commit acknowledged by identity after a covering flush and publish |
| AR-7 | §2.2 T2 | the bounded wait per OS; the flush byte; the waiter thread; the writer byte held to scan, re-validate and append; the leader's endpoint per OS; the revisit trigger "after G1 and group commit" |
| AR-8 ([81] B1, M6) | §2.8 T8 | Decision, Chosen from, Rejected and Revisit trigger replaced: durability classes per OS, leaderless group commit with the chain, scan and re-write under the writer byte, read-modify-write publish and identity acknowledgement; lazy loss after a failed flush; no downgrade; rejected serial flushing, leader-only group commit, concurrent flushes, acknowledgement by position, `F_BARRIERFSYNC`, `O_DIRECT` |
| AR-9, AR-10, AR-11, AR-12 ([81] m14, m15) | §4.1 | `LOCK` row = layout v1; `log.NNNN` row = extents per file system, rotation padding; `config` row = per-OS user scope and the durable replace-rename; Rules = sealed files read-only with `total_len`, and the crash-gated allow-list per OS |
| AR-13 ([81] M2, M3, M7) | §4.2 | `boot_id` comment (the invariance rule, per-OS sources, Unknown-boot mode); boot check takes the **flush byte, then the writer byte**; the barrier only after maintenance's own `Checkpoint` is published, one `HEAD` flush outside the writer byte |
| AR-16 ([81] B1, B2, m15) | §4.3, §4.4 | group validity with the chained `group_end` trailer; R4 kinds `JournalCursor`, `DirMap`; `SegHdr.total_len`; `LEASES` anchor and `ProcId`; `FILEOBS` with the tagged `OsFileId`; `JOURNALCUR`, `DIRMAP` |
| AR-14 ([81] B1, M6, M7, m4) | §4.5 | phase-2 heading and intro; steps 5–7 (scan from L0 into a scratch layer; step 7's shortcut only when nothing lies beyond L0); step 9 (append with the trailer; lazy publish); step 10 = group commit with re-write under the writer byte, read-modify-write publish, identity check and the lost-group path; step 12 (the `Checkpoint` made durable by group commit); "Flush grouping" |
| AR-15 | §4.6 | image row `schema/queries/<q>.moi`; R4 kinds and sections renamed; the audits' `LOCK` entry; new paragraph "Reserved in format v1 for the ports" |
| AR-17 ([81] m15) | §4.7 | size check against `total_len` before mapping; the reader's chain check after a new `slot_seq` |
| AR-18 ([81] m11) | §4.9 | rollup priorities per OS, never from a foreign PID namespace; bulk-pass advice per OS, never on a log extent with unflushed groups |
| AR-19 ([81] m1, m2, M7) | §4.10 | renamed "Crash safety and the OS layer"; acknowledgement after a covering flush, publish and identity check; the next flush holder re-writes; namespace durability per OS with the uniform no-replace rename; the simulator's amended item (2); the `restore` swap; the OS-layer bullet replaces "Unix builds swap …" |
| AR-20, AR-21, AR-25 | §5a.1, §5b.1, §5d.1 | canonical per-component directory key; hashed named-query file names; `JOURNALCUR`, `DIRMAP` |
| AR-22, AR-23, AR-24 ([81] B2, M4, M5) | §5e.2, §5e.3, §5e.7, §5e.8 | tagged ids and volume keys; the path rules with P3–P5, P10 and the twin rule; E3 and E3d per OS with whole-id identity; the copy rule only with a unique, uncopyable creation time; trash per OS; the resolver constants; sharing and cloud per OS; I-F4 volume keys; I-F8 with P3 |
| AR-26 | §6.1 | MCP row (anonymous `mmap`; liveness slot); leader row (endpoint per OS); the writer protocol with the flush byte; maintenance's durable records by group commit; the burst cost ≈ 2–3 flush times |
| AR-27 ([81] m13) | §6.2 | the 32-byte session anchor, `/clear` alias, three-valued liveness without PIDs; the boot-clock deadline and Unknown-boot mode; the intent anchor; the parent-process watch |
| AR-28 | §6.5 | `durable` guarantee with the covering flush and identity check; `lazy` publication and loss; the closing paragraph on group commit |
| AR-29 | §7.1 | argv rules T1–T10; exit 7 gains "refused location, store I/O fault in a mapping, durability outcome pending or unknown" |
| AR-30 ([81] m12) | §7.5 | hooks and `.mcp.json` by absolute path with the `${CLAUDE_PLUGIN_DATA}` substitution verified at M0 item 7 and M8; the `PATH` export |
| AR-31 | §8.1 | Windows-only numbers noted; durable-commit and writer-wait rows for group commit; the leader's RSS row |
| AR-32 | §8.2 | item 2 (group-commit contention); items 11 and 12 extended; item 22 (the Windows OS-layer probes, incl. the boot identity across a clock step, sleep, hibernation and reboot); the simulation bullet (amended fault model, pending groups, read errors); the Unix kill loops as port gates; the thirteen seeded group-commit bugs; the reference-model paragraph |
| AR-33 | §8.3 | intro (Windows-only gates); flushes per verb; delta checkpoint outside the writer byte; GT1's lost-group criterion; seeded bugs; durable-commit semantics; new row GT20 (d); GT12 with cmd; **per-OS notes** after the TOKENS table |
| AR-37 | §9 | M0 scope (the OS layer, X-F1–X-F12), measurements 1–22, entry (#32 decided), exit (the three-OS specification reviewed); M1 scope, exit (≤ 1 flush, identity) and gates (GT20 d); M6 scope (`VolumeCaps`, tagged ids, twin and copy rules, the sweep); M10 scope (parent watch); "Not built" (group commit in M1; the Linux and macOS implementations in the port phase); the release gate is Windows' |
| AR-38 | §10 | risk 29 rewritten; risks 31 (group commit), 32 (macOS flush cost), 33 (sandbox denials) added |
| AR-34 | §11 | intro "#32–#42"; **#32 DECIDED with both quotes**; the "Due before M0" sentence; the old #32 row deleted; #21 (d); #39 notarization; #41 FSEvents; **#42 port-phase hardware, CI and platform coverage (money)** |
| AR-35 | §12 | row: a weaker guarantee on any OS |
| AR-36 | §13 | user scope per OS; `durability.lazy-kinds` trade-off; `lock.flush-wait-ms`; `image.dest.<name>.path` default; "Never a key" gains the OS-layer rules |
| AR-40 (new) | **§14 Cross-platform** | the OS layer, the compact per-OS mapping table, the frozen items X-F1–X-F12, the minimum versions and the port phase, linking to [80] |
| AR-39 | Review log | the entry "Owner decision #32 (2026-09-26): cross-platform design"; the closing line |

### 6.2 `docs/research/design/40-file-links-design.md` [40]

| Label | Section | Change |
|---|---|---|
| 40-1 | header, §1.4 | the #32 amendment; "spelling differs on disk" |
| 40-2 | §0.1 DR4 | "Windows, Linux and macOS; non-admin on all three" |
| 40-3 ([81] M4, m8) | §2.4 | NFC for untracked macOS names; P4 refusals; long paths per OS; P5; `fold_v1` with full folding at Unicode 17.0.0; the directory's equivalence per OS; **twin sets**; "spelling differs on disk" and the normalization rule |
| 40-4 ([81] B2) | §2.6, §2.9 | `TREES` (canonical root, root id, `VolumeCaps` snapshot); `FILEOBS` (tagged `OsFileId`); `FSINTENT` (intent anchor); `JOURNALCUR`; new `DIRMAP`; the lazy-hook sentence; the OS-id paragraph with "an inode number alone is never identity"; reads with ids; the `ok` detail strings |
| 40-5 | §2.11 | R-7, R-8, R-14 (the per-OS rules), R-16 (new strings), R-18 |
| 40-6 ([81] m1, m2) | §3.4 | Windows and Unix renames as the uniform no-replace rename plus `durable-name` of both parents; the Linux `link` + `unlink` fallback and its recovery row; recovery by the intent anchor |
| 40-7 | §3.5 | `durable-name` of the parent on every OS |
| 40-8 ([81] B2, M5) | §4.3 | per-OS STAT; the twin line; E3d, E3 and E7 per OS; the copy rule's unique-creation-time condition and its note; the OS trash in step 5 |
| 40-9, 40-10 | §4.6, §4.7 | the case row; a twin row; macOS dataless files; FSEvents as E2 (not built) |
| 40-11 | §8.3 | §8.3.1 heading with the port matrix; row 4; §8.3.2 the `VolumeCaps` sweep, the port simulator profiles and the one-tick fixture |
| 40-12 | §9.2 | decision 3 gains (d) |
| 40-13 | Review log | "Cross-platform design (2026-09-26)" |
| 40-14 (new) | §4.7, §5.7, §8.4 | `JournalCursor`/`JOURNALCUR`, `DIRMAP` and volume keys in the remaining lists |

### 6.3 `docs/research/design/50-query-language-design.md` [50]

| Label | Section | Change |
|---|---|---|
| 50-1 | header | the #32 amendment |
| 50-2 | §2.2 rule 5 | `#` per shell |
| 50-3 | §4.4, Q21, §8.1 F3, §10 | `schema/queries/<q>.moi` |
| 50-4 ([81] m11) | §6.1 | bare ids taught; cmd quoting; query files per OS (heredoc only for agents on Linux and macOS) |
| 50-5 | §6.2 | rule 5 "non-path"; new rule 7 (every other shell) |
| 50-6 | Review log | §12.7 |

### 6.4 `docs/research/design/60-roadmap.md` [60]

| Label | Section | Change |
|---|---|---|
| 60-0 (new) | header, sources | the fourth amendment; the owner's #32 quotes beside the standing preferences; the [80] source row |
| 60-1 | §0 | the #32 bullet |
| 60-2 | §1.2 P1 | the seams per OS |
| 60-3 | §1.3 | the Linux/macOS row (port phase); the leader row without group commit |
| 60-4 ([81] B1, m15) | §2.5 | [AR] rows `LOCK`, Log (chain trailer), Segments (`total_len`), Store layout; the fault model (2), (3), (5), (7), (8) amended and (9)–(12) added; decisions (j)–(m); audits' rows `HEAD`, `LOCK`, Log (chained validity), `Vfs`/`ProjectFs`, resolver constants; a new "Cross-platform" row; restated (a), (c), (h); R-7, R-8, R-14, R-16, R-18, F3; the §2.6 registry row |
| 60-5 | §3.1 M0 | entry; items 1, 3, 4 (thirteen bugs), 5 (1–22), 7 (Windows-only gates); exit (the three-OS specification; twenty-two measurements); size (+ 5–7.5) |
| 60-6 | §3.2 M1 | the Windows OS layer and its conformance suite; group commit in the protocol bullet; the leader; exit (flush counts, group commit, seeded bugs); gates (GT20 d); size (+ 2.5–5, + 0.5–1) |
| 60-7 | §3.7 M6, §3.11 M10 | the rename point; `VolumeCaps`, tagged ids, twin and copy rules, the sweep (+ 1–2.5); the parent watch (+ 0.5) |
| 60-8 | §3.13 | GT1 (lost-group scenario), GT4 and GT15 (port rigs), GT12 (cmd), GT20 (d) |
| 60-9 | §3.14 | #32 decided; #42 and #21 (d) rows |
| 60-10 | §5.2–§5.4 | item 2 (group commit), item 22; floor rows for group commit; flushes per commit |
| 60-11, 60-12, 60-13 | §6, §7.1, §8 | the ports' gate; the calendar note (+ 9.5–16.5; port ≈ 52–83); risk 11 wording; risk 18 |
| 60-14 | Review log | §10.7 |

### 6.5 Review-log entries

The four entries appended by this pass are in the target documents: [AR] Review log, "Owner decision #32 (2026-09-26): cross-platform design"; [40] Review log, "Cross-platform design (2026-09-26)"; [50] §12.7; [60] §10.7. This file's own Review log is §8.

### 6.6 Checks after applying

Run on the four documents after the edits:
1. **No Linux or macOS build, test, CI or crash gate inside M0–M11.** Every match of "Linux", "macOS", "Unix", "port" and "cross-target" in [AR] §8.3, §9 and [60] §3 is either a per-OS note labelled "port phase", a statement that nothing Linux or macOS is built or tested before the release ("Windows-only", "no binary, no test"), the cross-target type check GT20 (e) (a type check only — no binary, no test — made a gate by owner decision #44), or the OS-layer lint, which runs on Windows. The only Linux or macOS gates are in [80 §5.3] and [AR §8.3]'s per-OS notes, both labelled as port-phase gates.
2. **No third-party database anywhere.** The only database names in the four documents are the exclusion list and the dependency lint GT20 (b), the owner's quoted decision, citations of precedents (SQLite's WAL-reset race, the SQLite session extension, redb's lock model cited in [80 §2.2.2]) and rejected positions. Nothing builds, links or runs one, benchmarks included.
3. **#32 is decided.** [AR §11] lists #32 under "Decided" with both verbatim quotes; the former "Due before M0" row is deleted; [60 §3.14] shows it as decided; no document calls it open.
4. **Stale protocol terms** ([81] M7): `byte 0`, `byte 2`, `exactly one flush`, `two slot flushes`, `takes the writer byte`, `L0`, `USNCUR`, `UsnCursor`, `case differs on disk`, `Unix builds swap`, `Windows 11 first`. What remains: the new Review-log entries that list these very terms; [60 §9]'s historical edit list of issue 2 and the historical rows of the earlier Review-log entries; renamed-from notes ("was `UsnCursor`", "replacing `case differs on disk`"); the new texts in which a process takes or re-takes the writer byte under the new lock order ([AR] §4.2 boot check, §6.1 maintenance) and the §2.17 ledger's `gitmap` append, which are correct as they stand; and `L0` only as the process's validated bound in [AR §4.5] steps 1, 4, 6 and 7 and [50 §5.9] steps 2–4, the pack levels L0–L2 of [AR §7.4] and the staleness levels L0–L3 of [AR §1] row 11.
5. **Stale terms, second search** (verification pass, 2026-09-26; re-run after its edits): `flush once`, `held for the flush`, `hold is the flush`, `LockFileEx` wait, `byte-0 holder`, `byte-2 holder`, `zero-filled extents`, `FlushFileBuffers(HEAD)`, `1 per verb`, `seg.b<refsym>`, `seg.b<X>`, `seg.b<ref>`, `boot_id, mono`, `files.usn`, `renames nothing`, `never renames`, `write-through rename`, `where the owner permits`. What remains: `flush once` in [AR §4.5] step 10.3 and [50 §5.9] step 5, where the flush holder flushes once outside the writer byte (correct); `zero-filled extents` in [60 §3.2] M1 scope, labelled as the NTFS method, and in the [AR §2.17] ledger (F-A4, historical), which also keeps G1's "blocking overlapped `LockFileEx` wait" (F-A2); `seg.b<refsym>` in [60 §9]'s historical edit list; `files.usn` as the name of the dropped key in [40] R-13 and in historical Review-log rows; `MOVEFILE_WRITE_THROUGH` only conditional on M0 item 17. No normative text keeps a stale term.

---

## 7. Open items and sources

**Open items.** Nothing is left open for M0. Everything else is decided here, is a port-phase decision (#21 d, #42), or is an M0 measurement that selects among frozen alternatives without changing a byte (the Windows boot-identity source, M0 item 22; whether Windows can drop `MOVEFILE_WRITE_THROUGH` besides the directory flush, item 17 — deferred with the rig to after the release by the owner review of 2026-09-27, both used until then). Items only a Linux or macOS machine can settle are the first task of the port phase (§5.2), and none changes a frozen byte unless it reveals a specification defect (X8):
- whether Seatbelt permits `fcntl` locks and `kern.bootsessionuuid`;
- FSEvents replay cost and inodes;
- APFS clone creation times;
- the detection of iCloud-managed `~/Desktop` and `~/Documents`;
- the frontier's cost on large trees and the effective mtime granularity per kernel;
- lock release under crash reporters, and 16-writer fairness;
- the allocator × libc matrix.

**Sources.**
- **Cross-platform reports:** [X17] `docs/research/17-xplat-durability-mmap-memory.md`, [X18] `docs/research/18-xplat-locking-ipc-processes.md`, [X19] `docs/research/19-xplat-file-identity-change-tracking.md`, [X20] `docs/research/20-xplat-toolchain-shells-ci-crash-testing.md`. Every [D], [S] and [C] above is cited through them, and their source lists give the URLs: Linux man pages and kernel source; XNU, HFS, Libc and copyfile source; Apple and Microsoft documentation; PostgreSQL, LMDB, SQLite, RocksDB, redb and Go source (read as precedents only); Claude Code 2.1.281's bundled code and sandbox-runtime; GitHub Actions documentation.
- **Review:** [81] `docs/research/design/81-cross-platform-critique.md`.
- **Design documents:** [AR] `docs/ARCHITECTURE-RESEARCH.md`; [40] `docs/research/design/40-file-links-design.md`; [50] `docs/research/design/50-query-language-design.md`; [60] `docs/research/design/60-roadmap.md`; [72] and [74] `docs/research/design/72-audit-correctness.md` and `74-audit-feasibility-config.md`.
- **Earlier reports:** [05], [08], [09], [13], [16] as cited by the above.

---

## 8. Review log

### 8.0 Verification pass (2026-09-26)

A cross-check against X-F1–X-F12 found gaps in the documents this design edited and in this file ([AR] Review log, "Verification pass after decision #32", XV1–XV16: 4 majors, 12 minors, all fixed). Changes here: **P12** (the machine-local form of root `abs`) joins X-F7; X-F11 freezes only the per-OS config locations, and its keys are registered in [AR §13]; P5's policy is the key `files.portable-names` and the `PATH` export the key `hooks.session-start.path-export`; `O_NOATIME` applies "when the process owns the file"; the lint GT20 (d) is defined on direct dependencies with a reviewed allow-list (§2.1, §5.5); the Dev Drive consequence (§2.6); the `cs.NNNN` rename (§2.3.2); the M4 and M5 dispositions name [40 §8.3.1] rows 34–36; §6.6 item 5 extends and re-runs the stale-term search.

### 8.1 Revision 2 (2026-09-26): disposition of the review [81]

[81] returned **REVISE BEFORE ACCEPTANCE: 2 blockers, 7 majors, 15 minors**. It confirmed the architecture (one page-size-free format, a protocol written against the weakest OS, OS code confined to `moirai-os`, lock-anchored liveness, durability classes with no downgrade, an unscheduled port phase), that decision #32 is recorded as decided with the owner's quotes, and that nothing in M0–M11 builds or tests Linux or macOS. Every finding is dispositioned below: **A** accepted as proposed; **A\*** accepted with the modification stated. None is rejected.

| Id | Severity | Finding (short) | Disposition | Where |
|---|---|---|---|---|
| B1 | blocker | group commit acknowledged by position, re-wrote the pending range outside the writer byte and had no predecessor chaining, so after a failed flush it could acknowledge a lost commit or make two exclusive commits durable | **A\*.** Acknowledgement by identity (the chained trailer re-read after the covering publish; a mismatch re-runs with the same key or exits 7 "outcome unknown"); scan and re-write under the writer byte (flush, then writer; release to flush; re-take to publish); **chained group validity**: the `group_end` record ends with an XXH3-64 trailer over the group seeded with the preceding chain value, read by position — the modification: the chain value lives in the log, not in `HEAD`, so any scan can start at any group boundary, and rotation pads the old extent so a chain never crosses an extent without a readable seed; pending groups only in a scratch layer; readers check the chain at their replay bound; lost lazy tails lower `committed_lsn`; four seeded bugs; the GT1 lost-group scenario and the model invariant | §2.3.4, §2.3.5 (3), (11), §2.4.2–§2.4.4, §3.1 X-F3; [AR] §2.8, §4.3, §4.5, §4.10; [60] §2.5 Log rows |
| B2 | blocker | Linux `(dev, ino)` treated as identity, but ext4 reuses the lowest free inode; `gen` optional | **A\*.** The Linux id is `ino ‖ hgen`, `hgen` a digest of the whole file handle from unprivileged `name_to_handle_at` — the modification: a digest of the handle rather than a parsed generation field, which works for every exportable file system without per-FS parsing; taken at every settle for recorded files and directories and for every frontier hit; E3, E3d and the path-reuse check compare the whole `OsFileId`; a file system without handles gets `id_kind = none`; R-14 rule 8 "an inode number alone is never identity"; GT17 and ext4-profile rows | §2.11.1–§2.11.5, §3.1 X-F8; [40] §2.6, §4.3, R-14; [AR] §4.4, §5e.3 |
| M1 | major | one handle/OFD per role per process merges on Unix and conflicts on Windows, so in-process semantics differ | **A.** A process-global grant table decides in-process ownership before any kernel call (`Busy` or an in-process wait) on every OS; per-role handles stay (Windows opens are costly); waiter joining defined; an in-process two-client case in M1's conformance suite | §2.2.1 items 2–3, §2.1, §3.1 X-F4; [60] §3.2 |
| M2 | major | the Windows boot identity (the kernel boot time) moves when the clock is set, so a clock step reads as a reboot and kills live leases | **A.** Frozen rule: constant for one boot, invariant under clock changes, suspend and hibernation; Windows uses the `BootId` counter (hashed with `MachineGuid`), never the boot time; M0 item 22 checks a clock step, sleep, hibernation and reboot, and a failing source selects Unknown-boot mode without a format change | §2.7.1, §3.1 X-F2; [AR] §4.2, §8.2 item 22; [60] §5.2 item 22 |
| M3 | major | the macOS boot identity is an undocumented sysctl Seatbelt may deny, and "unreadable refuses the store" left the port no legal fix; libproc is private too | **A.** A frozen Unknown-boot mode instead of refusal (no republish, no boot recovery, lease boot tests `Unknown`, readers by the validity rules, writers publishing at the next flush); principle X9 (public interfaces only; a private read may only select a degraded mode); process start times from `sysctl(KERN_PROC_PID)`; libproc only in the measure harness; the private-API position stated per OS | §1 X9, §2.7.1, §2.9, §3.1 X-F2 |
| M4 | major | case and normalization twins from another OS resolve as `ok (spelling differs on disk)` on the other file | **A\*.** The twin rule — the modification: the twin whose **recorded content** equals the file on disk resolves, not the one whose spelling is enumerated, because a colliding git checkout keeps the first name and writes the last content; the others `missing (not representable on this OS)`; no match or several → `ambiguous (… collision)`; never `ok` on a spelling match alone; GT17 rows on NTFS (M6: [40 §8.3.1] rows 34 and 36) and APFS (port) | §2.10 P5, §2.11.4 rule 2, §2.11.5; [40] §2.4, §4.3, §4.6, R-14, R-16 |
| M5 | major | the copy rule's creation-time line is exact on identical files created in one tick (Linux, and Windows too) | **A.** `Unforgeable` never makes a candidate exact (at most STRONG); on Windows the creation time must be unique among the E4 candidates and differ from every other node's recorded creation time; [X19 §3.2]'s "never happens" superseded here; an NTFS one-tick row in M6 ([40 §8.3.1] row 35) and a real-checkout corpus case in the port | §2.11.1, §2.11.4 rule 1, §2.11.5; [40] §4.3; [AR] §5e.3 |
| M6 | major | under group commit the content of the `HEAD` publish was unspecified: covered `Checkpoint` effects went unpublished and a stale snapshot could republish an old segment set | **A.** Every publish is a read-modify-write of the newest valid slot under the writer byte, folding every covered group's `HEAD` effects in log order; fields only advance (except `committed_lsn` after a lost lazy tail); maintenance's barrier and GC only after its own `Checkpoint` passed its identity check; two seeded bugs | §2.3.2, §2.4.3, §2.4.4, §3.1 X-F3; [AR] §4.2, §4.5 |
| M7 | major | the edit list missed [AR] text contradicting the new protocol (§4.2 lock order, §4.5 step 7's L0 shortcut, step 12, the phase-2 heading and intro, "Flush grouping", §1 row 18, §4.10) | **A.** All added and applied (AR-6b, AR-13, AR-14, AR-19); the post-edit search re-run (§6.6 item 4) | §6; [AR] §1, §4.2, §4.5, §4.10 |
| m1 | minor | the `file mv` protocol point differed by OS | **A.** A no-replace rename, then `durable-name` on both parents, on every OS; M0 item 17 decides only how Windows implements `durable-name` | §2.3.1, §2.3.2; [40] §3.4 |
| m2 | minor | no replace or exchange rename; the Linux `link` + `unlink` fallback's crash state; no macOS fallback | **A.** `rename_replace` and `swap_dirs` with their protocol points (`config`, git's `.lock` protocol, `restore`); the "both names, one inode" state is an `FsIntent` recovery state; a macOS volume without `RENAME_EXCL` refuses `file mv` | §2.1, §2.3.2, §2.11.1; [40] §3.4 |
| m3 | minor | `O_DIRECT` + `RWF_DSYNC` would need a format or protocol change | **A.** Rejected now | §2.3.1 |
| m4 | minor | a stranded pending durable group blocked visibility | **A\*.** A lazy group appended behind a pending durable group runs phase 2b (waits for the flush byte; adopts if nobody covered it) — the modification: no age heuristic is needed, because the wait costs at most one flush time; best-effort evidence records are exempt and become visible with the next flush | §2.4.3 |
| m5 | minor | lock-release delay "Unix 0"; abandoned waiter threads; Unix fairness | **A.** The delay is unbounded on every OS in the model; crash-reporter and fairness probes in the port; fairness outside the contract; an abandoned waiter counted in the idle rule | §2.1, §2.2.1 item 10, §2.2.2, §2.3.5 (8), §5.2 |
| m6 | minor | no fault item for a failing `pread` | **A.** Item (12): corruption below `durable_lsn`, end of log above it under the chain rule; injected by the simulator | §2.3.5 |
| m7 | minor | coarse Linux timestamps; the racy threshold's clock; mtime-restoring tools | **A.** The racy threshold from a file-system timestamp; the effective granularity measured per volume; the hole documented (never a wrong bind) | §2.11.1, §2.11.3, §2.11.5 |
| m8 | minor | `fold_v1` used simple folding at an unnamed Unicode version | **A.** Full folding (C + F) and NFD at Unicode 17.0.0 | §2.10 P6; [40] §2.4, R-14 |
| m9 | minor | ref names had no fold-collision rule | **A.** NFC on input; a fold-equal ref name refused; `doctor image` reports | §2.10 P11 (b), X-F9 |
| m10 | minor | the macOS canonical root's "on-disk case" was unsupported | **A.** Per-component canonicalization with on-disk names; lookups by the root's `OsFileId` first | §2.10 P9, X-F7; [AR] §5a.1 |
| m11 | minor | the sandbox write set covers only the store; the rollup child dies with the namespace; T5 used `TMPDIR` | **A.** Exports and backups outside the store exit 7 with the `allowWrite` entry (the daily export runs in the unsandboxed hook); no rollup child from a foreign PID namespace; agents use heredocs only on Linux and macOS, scripts' `-f` files go to the store's `tmp/` | §2.6, §2.12, §4 T5; [50] §6.1 |
| m12 | minor | denials unmapped; a wrong hook-fallback claim; `${CLAUDE_PLUGIN_DATA}` substitution unverified | **A.** Denials → `Unknown` or source absent (rule 9, probe item 6); `SF_DATALESS` checked before every read; the MCP-exit claim corrected; the substitution tagged and verified at M0 item 7 and M8 | §2.2.1, §2.5 rule 6, §2.11.4, §2.12 |
| m13 | minor | `PR_SET_PDEATHSIG` is thread-scoped | **A.** `pidfd_open(getppid())` with `poll` (Linux ≥ 5.3) | §2.7.2, §2.13 |
| m14 | minor | ungated types allowed; dead overlayfs rule; no ZFS row; iCloud-managed Documents | **A.** Every allowed type crash-gated, the others refused (ReFS, ZFS, f2fs, bcachefs, HFS+ until gated); every overlayfs refused, with the dev-container consequence stated; ZFS in the id trust list; iCloud-managed `~/Desktop`/`~/Documents` refused once detected | §2.6, §2.11.1, §2.13 |
| m15 | minor | size check without a size; `vol_key` and `VolumeCaps` layouts; T7 golden files; the macOS 14 floor unenforced; "22H2" | **A.** `total_len` in every sealed header; `vol_key` = BLAKE3-128 of a tag and one fixed source per kind; a 16-byte `VolumeCaps` snapshot; three golden-file substitutions; minos 14 plus a version check at open; "22H2" dropped | §2.5 rule 4, §2.11.2, §2.13, §4 T7, X-F6, X-F8 |

**Other changes in revision 2.**
- **Owner decisions pruned to real ones.** Revision 1 put three new questions to the owner. Under the owner's rule (only format, identity, semantics, money and data leaving the machine are owner decisions), the cross-target type check (former #42) is a design default *(superseded by owner decision #44: GT20 (e) is a gate from M0; the cost is in the calendar, [60 §7.1])* — it builds no binary, runs no test, spends no money and moves no data — and the platform matrix (former #43) is a design default recorded in §2.13; its only money aspect, an Intel macOS runner, joins the hardware and CI decision, now **#42 (port-phase hardware, CI and platform coverage)**, due before the port phase.
- **Costs re-estimated:** M0–M11 ≈ 9.5–16.5 units (was 8–13.5); the port phase ≈ 52–83 units (was 51–81).
- **The edit list was applied** in this pass (§6), with the [AR] "Cross-platform" section (§14) the task of this pass required, and every stale term searched (§6.6).

### 8.2 Revision 1 (2026-09-26)

The first issue turned [X17]–[X20] into the OS-layer design after owner decision #32, with the edit list for [AR], [40], [50] and [60]. Reviewed by [81]; superseded by revision 2.

### 8.3 Owner decision #44 (2026-09-26)

"Do cargo check for Linux and Mac." (verbatim translation). §5.5 (b) becomes a gate, GT20 (e), from M0: `cargo check --workspace --all-targets --locked` for `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl` and `aarch64-apple-darwin` (the port's release targets, musl rather than glibc) beside the Windows host, with every target's C, C++ and assembler poisoned; the binary crate is a composition root only; every dependency is pure Rust, with a reviewed entry for every build script and no checked crate depending on a host-only crate, so §2.9's C allocator option is withdrawn (the system allocator everywhere) and T10's `zstd` leaves (the codec is decided among pure-Rust options at M0, [90 §11.3]). The Codex sandbox joins §2.6 and the Codex entry point §2.12 ([90], revision 2 after its review [91]). §5.4's delta is now in [60 §7]'s calendar. No frozen item changes. **Numbering:** "former #42" and "former #43" in §8.1 name questions revision 2 withdrew; they are not [AR §11]'s decisions #42 (port-phase hardware, CI and platform coverage — money), #43 (harness-agnostic interface) and #44 (this decision). Edited: §0 (15), §2.6, §2.9, §2.12, §2.13, §5.2, §5.4, §5.5, §6.6 and this log.

### 8.4 Verification pass after decisions #43 and #44 (2026-09-26)

[AR]'s Review log (HV1–HV22) lists the pass. Changes here: §2.5 rule 4 says "the dictionary bytes", whose form M0 freezes (HV13); §8.1's former type-check default carries a superseded note (HV7); §8.3 describes #42 as port-phase hardware, CI and platform coverage (HV10); the end marker follows §8.4 (HV21). No frozen item changes.

### 8.5 The owner's answers of 2026-09-26 on [AR §11]

The owner answered (verbatim translation, [AR] binding inputs): "For now no additional machine will be used, everything is here. Two lanes in parallel. For now benchmarks only on Opus 5.5. The moirai project itself will be stored in a public repository on GitHub. Record that all commits must be made WITHOUT Claude co-authorship. Everything else I approve as you wrote it." Changes here, all in the port phase's prerequisites: the repository is public (#36), so the GitHub-hosted Linux runners with `/dev/kvm` that carry the port's CI and Linux crash layers (LazyFS, dm-log-writes, dm-flakey, the QEMU/KVM loop) are free (§0 item 15, §5.4; §2.11's E2 row and §5.2's macOS row now say that #41 decided "not built"); the Windows rig row names the laptop as the rig machine; no test host exists (#34, profile L), so floor-grade Linux for GT11 defaults to a dual-boot partition of the owner's laptop unless #42 buys a machine (§5.4; [AR §11] #42's recorded default follows). #42 and #21 (d) stay port-phase decisions with their defaults recorded. The M0–M11 gates stay Windows-only, on the owner's laptop for every timing, crash and kill-loop gate. No frozen item changes.

### 8.6 Verification pass after the owner's answers (2026-09-26)

[AR]'s Review log lists the pass. Change here: §6's status says that the edit record is historical and that the decision states it quotes are superseded (§8.5). The Windows GT4 of M0–M11 now runs three variants in profile L, disk-full through the simulator and an owner-run drill ([60 §3.13]); the port's GT4 row of §5.3 is unchanged, because its rigs are not the owner's Windows Home laptop. No frozen item changes.

### 8.7 Cross-document consistency pass after the Russian approval review (2026-09-27)

The Russian owner-approval description (`docs/architecture-approval-ru/`) marked where this file disagreed with [AR] and [90]. Items fixed here: **XF2-anchor-hash-and-kinds** (resolved by the design team; the M0 specification review confirms it at the format freeze) — X-F2 takes the amendment of [90 §4.4, §10.1]: the anchor's session hash is BLAKE3-128 of the namespaced process-lifetime identity `<harness>:<id>` (16 B, the width of the `bound` hash), kind 4 `session-ttl` is appended with the existing kind values unchanged, and a Codex thread's server takes its slot lazily at its first call carrying `_meta.threadId`; a CLI or hook matches the hash of `claude:` + `CLAUDE_CODE_SESSION_ID` or `codex:` + `CODEX_THREAD_ID` (§2.2.3's lock map, §2.7.2's anchor table and slot lookup, §3.1 X-F1 and X-F2). The sizes stay: the 32 B anchor holds the 16 B hash in place of the diagnostic nonce and the u64 hash for kinds 1 and 4, and keeps its load-bearing nonce with an 8-byte diagnostic hash prefix for kinds 2 and 3; `SlotRec` widens its primary and alias hashes to 16 B out of its reserve (32 B → 16 B), so the record stays 128 B, the slot table 32 KiB and `xxh3` at the record's last 8 bytes; `WriterDiag` carries the 16 B hash within its ≤ 512 B. `LEASES`, `FSINTENT` and `LOCK` keep their sizes; nothing is frozen before M0. X-F2 places the anchor in `LEASES` and `FSINTENT` for every kind (the where-clause no longer reads as kind-specific), and §2.7.2's `intent` row names the diagnostic field `session_hash_lo` (8 B), as X-F2 does. **lint-a-gate-80** — §5.5's decision line calls lint (a) a gate, GT20 (d), from M1, as its table row, §0 item 15 and [AR §8.3] GT20 (d) do. The earlier entries, including §8.4's and §8.6's "No frozen item changes", are historical. No line above this entry was added or removed.

*End of 80-cross-platform-design.md.*

### 8.8 The owner review of 2026-09-27

The owner answered the approval checklist of the Russian description (`docs/architecture-approval-ru/15-approval-checklist.md`; items А1–А8, Б1–Б14 and В1–В10, cited as A1–A8, B1–B14 and V1–V10); [AR]'s binding inputs record the answers and its Review log entry of the same date lists the whole change. **A4**: [AR] with this design and [40], [50], [60] and [90] is approved as the M0 specification. **V7** ("No way to install it for now; such a deep test is postponed until the release, like mac and linux"): the Windows OS-crash rig — GT15 on the Server 2025 Core guest in VirtualBox 7 with VMware Workstation Pro as the cross-check, its calibration ([AR §8.2] item 17, formerly an M0 measurement), the guest-licence question and the laptop's WSL2 and Memory Integrity choices — is deferred to after the release and runs in the same unscheduled post-release phase as the ports, never blocking the release (§0 item 15, §5.1, §5.4's Windows rig row). Until that calibration, Windows `file mv` keeps `MOVEFILE_WRITE_THROUGH` besides the `durable-name` directory flush (§2.3.1, §2.3.2, §7). The environment guard (§0 item 7, §2.6) states that in M0–M11 NTFS is admitted on the GT1/GT3 fault-model evidence and GT4 on real NTFS, its GT15 variant running with the deferred rig ([AR §10] risk 17); the allow-list rule itself is unchanged. The M1 gates GT1, GT3 and GT4 and every frozen item of §3 are unchanged; no frozen byte changes. The rest of the review changes nothing here.
