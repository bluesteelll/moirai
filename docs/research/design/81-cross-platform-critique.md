# 81 — Critique of the cross-platform design [80]

*moirai design review, 2026-09-26. Status: research only; nothing is implemented, and this file is the only file written. It attacks [80] `design/80-cross-platform-design.md` against the four cross-platform reports [X17]–[X20], the design of record [AR] and the designs [40], [50] and [60], along the six axes the review brief names: (a) identical protocol semantics, (b) portable format, (c) the R4 resolver on Linux, (d) the Claude Code sandboxes, (e) no Linux or macOS building or testing in M0–M11 and no hidden Windows-only format assumption, (f) minimum versions and the macOS private-API question.*

**Inputs, read in full:** [80]; [X17] `research/17-xplat-durability-mmap-memory.md`; [X18] `research/18-xplat-locking-ipc-processes.md`; [X19] `research/19-xplat-file-identity-change-tracking.md`; [X20] `research/20-xplat-toolchain-shells-ci-crash-testing.md`. **Read for the checks:** [AR] §1, §4.1–§4.10, §6.1–§6.5, §7.1, §7.5 and the edit targets of [80 §6.1]; [40] §4.3–§4.4 and the R-14 row; [60] §2.5 (fault model, protocol decisions, frozen rows). [72] and [74] were searched for earlier treatment of the boot identity.

**Evidence tags** (as in [80]): [M] measured on the owner's Windows machine, quoted from the cited report; [D] documentation; [S] source code, as read by the cited report; [C] third-party claim; [I] inference of this review. No measurement was made and no web request was sent while writing this file. No Linux or macOS machine exists here, so every Linux or macOS statement below is [D], [S] or [I].

---

## 0. Verdict

**Revise before acceptance. Two blockers, seven major findings, fifteen minor ones.**

The architecture of [80] is right, and most of it survives the attack:
- one little-endian, 64-bit, page-size-free format;
- a protocol written against the weakest OS;
- OS differences confined to `moirai-os`;
- lock-anchored liveness instead of PIDs;
- durability classes with no silent downgrade;
- a port phase that is documented but unscheduled.

The owner's decision #32 is recorded as DECIDED with the verbatim quotes (AR-34). Nothing in M0–M11 builds or tests a Linux or macOS binary: the cross-target `cargo check` is put to the owner as #42.

Two defects reach bytes or rules that X-F3 and X-F8 freeze at M0. Under [80]'s own rule X8 they cannot wait for the port:

1. **Group commit (X-F3) is unsound under the fault model [80] itself adopts (X-F5 item 3).** After a failed flush, pages can revert or be evicted while other writers are still appending. The protocol then:
   - acknowledges groups by LSN position alone;
   - re-writes the pending range outside the writer byte;
   - accepts a group after a predecessor it was never validated against.

   As a result it can acknowledge a lost commit, and it can make durable two commits that exclude each other (§2, B1).
2. **On Linux `(dev, ino)` is treated as identity, but ext4 reuses the lowest free inode.** R-14's E3d and E3 exact rules then re-bind a link to an unrelated file, and the link reads `moved-auto`. That breaks R4's core guarantee on the one OS where [80] claims the resolver degrades safely (§2, B2).

The seven major findings are:
- a lock-ownership rule that behaves differently in-process on Unix and on Windows;
- boot identities that are unstable on Windows, and possibly unreadable under macOS Seatbelt;
- case and normalization twins that resolve silently to the wrong file;
- a creation-time copy rule that is unsound for batch-created duplicates;
- an unspecified `HEAD` publish under group commit;
- gaps in the edit list that would leave the design of record deadlock-prone and double-claim-prone.

Every one of them can be fixed inside M0's existing scope. None needs a Linux or macOS machine.

| Axis | Result |
|---|---|
| (a) identical protocol semantics | **fails**: B1, M1, M2, M6, M7; minor m1–m5 |
| (b) portable format | mostly holds; M4, m7–m9 |
| (c) R4 on Linux never re-binds wrongly | **fails**: B2, M5; minor m7, m10 |
| (d) sandboxes | mostly holds; M3, m11–m13 |
| (e) nothing Linux/macOS scheduled; no silent Windows-only format assumption | scheduling holds; format: m14, m15 (and M1, which passes on Windows only) |
| (f) minimum versions, private API | OFD handled honestly; boot identity and libproc are not (M3, m15) |

---

## 1. Findings at a glance

| # | Sev. | Axis | Finding | Frozen item touched |
|---|---|---|---|---|
| B1 | blocker | a | Group commit acknowledges by position, re-writes outside the writer byte, and has no predecessor chaining, so under the amended fault item (3) it can acknowledge a lost commit or resurrect a group behind the wrong prefix | X-F3, X-F5, [60 §2.5] Log row |
| B2 | blocker | c | Linux inode reuse makes E3d and E3 exact on unrelated files; `gen` is optional | X-F8, R-14 |
| M1 | major | a, e | "One handle/OFD per role per process" merges on Unix and conflicts on Windows, so the in-process semantics differ | X-F4 |
| M2 | major | a | The Windows `boot_id` (the kernel boot time) moves on clock steps; a false boot change kills every lease | X-F2 |
| M3 | major | d, f | The macOS boot identity comes from an undocumented sysctl that Seatbelt may deny, and "unreadable → refuse" leaves the port no escape | X-F2 |
| M4 | major | b, c | Case and normalization twins resolve as `ok (spelling differs on disk)` on the other file | X-F7, X-F8 |
| M5 | major | c | Coarse creation times make the copy rule's creation-time line exact on batch-created duplicates (Linux, and Windows too) | X-F8, R-14 |
| M6 | major | a | Under group commit, the content of the `HEAD` publish is unspecified: a stale segment set can be republished, and covered `Checkpoint` effects can go unpublished | X-F3 |
| M7 | major | a, e | The AR edit list misses §4.2 (lock order), §4.5 step 7 (the `L0` shortcut), step 12, §1 row 18 and "Flush grouping" | edit list |
| m1 | minor | a | The `file mv` protocol point differs by OS: the Windows directory flush is optional | X-F5 |
| m2 | minor | a | No replace-rename or exchange-rename in `os::fs`; the Unix no-replace fallbacks add crash states | X-F5, [40 §3.4] |
| m3 | minor | a | The `O_DIRECT` + `RWF_DSYNC` "port optimisation" would need a format and protocol change (X8) | X-F5 |
| m4 | minor | a | A stranded pending durable group blocks visibility indefinitely | X-F3 |
| m5 | minor | a | Lock-release delay is "Unix 0"; crash reporters hold locks on every OS; waiter-thread and fairness details | X-F5 item 8, X-F4 |
| m6 | minor | a | The fault model has no item for a failing `pread` (EIO, checksum failure) | X-F5 |
| m7 | minor | b, c | Linux timestamps are coarse; the frontier's racy rule and its completeness claim are overstated | X-F8 |
| m8 | minor | b | `fold_v1` uses simple case folding at an unnamed Unicode version | X-F7 |
| m9 | minor | b | Ref names have no fold-collision rule | X-F9 |
| m10 | minor | b, c | The macOS canonical root (the "on-disk case" of `F_GETPATH`) is unsupported; bindings split by case | X-F7 |
| m11 | minor | d | The sandbox write set covers only the store: `image export` and `backup` fail, the rollup child is killed, and T5 uses `TMPDIR` | X-F11, X-F12 |
| m12 | minor | d | Error mapping under the sandboxes and TCC; a wrong hook-fallback claim; the `${CLAUDE_PLUGIN_DATA}` substitution is unverified | — |
| m13 | minor | d | `PR_SET_PDEATHSIG` is thread-scoped: an early MCP exit makes session leases Dead | — |
| m14 | minor | a, e | Environment-guard inconsistencies (ungated types, dead overlayfs rule, no ZFS id row, iCloud Documents) | X-F6 |
| m15 | minor | e, f | Size-check source missing from `SegRef`; `vol_key` and `VolumeCaps` layout slips; T7 golden files; the macOS 14 floor is not enforced; libproc is private | X-F6, X-F8, X-F12 |

---

## 2. Blockers

### B1 — Group commit is unsound under the amended fault model (X-F3 with X-F5 item 3)

**What [80] says.**
- §2.4.3 phase 2b:
  - a waiting writer acknowledges when `durable_lsn ≥ the group's end` (steps 1 and 3);
  - the flush holder scans `(durable_lsn, E]`, re-writes it from its read buffer and flushes, all while it holds only the flush byte (step 4);
  - after its own flush it publishes and "acknowledge[s]" unconditionally.
- §2.3.5 item (3), amended: after a failed flush, reads of the range "may return old **or** new bytes, may change between reads".
- Per OS: btrfs reverts the pages; macOS invalidates the buffers; ext4 and XFS keep new bytes in clean pages, which eviction can drop [D, ATC'20 via X17 §3.5].

In the serial protocol this was harmless, because no live writer's group ever sat unflushed while others appended behind it. Group commit creates exactly that state.

**Scenario B1-a: an acknowledgement by position covers a commit that is gone.**
1. `durable_lsn = D`. Writers W1–W5 append G1–G5 at `[D, E5)`, release the writer byte and queue on the flush byte.
2. W1 takes the flush byte, re-writes and flushes. The flush fails (EIO, or `ENOSPC` at write-back on a sparse APFS or btrfs extent, which [80 §2.3.3] itself expects). W1 aborts.
3. The pages of `[D, E5)` now read old content (zeros in a fresh extent). This happens:
   - at once on btrfs and macOS;
   - on ext4 and XFS after eviction, which [80 §2.12]'s own `POSIX_FADV_DONTNEED` in a concurrent `backup` of the log extent can trigger.
4. Appender A7 takes the writer byte. Its scan from `committed_lsn` finds the log ending at D, so it appends G7 at D.
5. W2 takes the flush byte. `durable_lsn = D < E2`, so W2 scans `(D, E]`, finds G7, flushes and publishes `durable_lsn = D + |G7|`, and then acknowledges G2, which is not in the log.
6. Every other Wi whose end ≤ `D + |G7|` sees the bound and acknowledges as well.

Acknowledged commits are lost with no crash at all.

**Scenario B1-b: the re-write outside the writer byte resurrects a group behind the wrong prefix.**
1. Flush holder F1 scans while G1–G5 are still readable.
2. The pages are evicted. A7, holding the writer byte, reads zeros, appends G7 at D, and releases.
3. A8 appends G8 at `D + |G7|`, validated against G7 and not against G1–G5. Take `D + |G7| ≥ E5`.
4. F1 re-writes G1–G5 from its buffer over `[D, E5)`, flushes and publishes up to E5.
5. After the next flush the log is G1–G5, then garbage or G8:
   - if `D + |G7| = E5` exactly, G8 is valid right after G5: its length, epoch, lsn-as-position and xxh3 all check.
   - Example: G3 is `claim 12` by agent X, and G8 is `claim 12` by agent Y, validated without G3. Both are durable and both are acknowledged, which violates I17′.
   - Example: G8 edits a `#N` that only G7 created, leaving a dangling reference.
6. A7 acknowledges too, because the published bound passes its end, although G7 was overwritten.

**Scenario B1-c: a same-length refill.**
1. After a revert, the hole is at G5, and G8 (validated against G5) survives after it.
2. A later appender writes G9 into the hole with `|G9| = |G5|`. Same-shaped commits have equal lengths, for example two `claim`s ([AR §4.3] sizes are deterministic).
3. G8 is contiguous and valid again, now behind a predecessor it never saw.

An identity check alone does not catch this, because G8's own bytes are intact.

**Why the simulator is not enough.** GT1 finds these cases only if the M0 specification tells it what "correct" is. As frozen, the protocol text is what the simulator and the model would both implement. The validity rule of `RecHdr` is a format item [60 §2.5], so the fix must land at M0 (X8).

**Fix (all OSes, M0).**
1. **Acknowledge by identity, never by position.**
   - Every writer remembers `(lsn, len, xxh3 of its group_end record)`. So does every process that acknowledges an idempotent replay of a pending group.
   - After it observes a covering publish, it `pread`s its range and compares.
   - A mismatch means the group was lost before durability. The writer does not acknowledge. It re-runs phase 2 with the same idempotency key, or exits 7 with "outcome unknown: retry with the same key".
   - Seeded bug: "acknowledge by position only".
2. **Scan and re-write under the writer byte.**
   - The flush holder takes the flush byte, then the writer byte (the existing order).
   - It scans `(durable_lsn, E]` and re-writes it, then releases the writer byte, flushes, and re-takes the writer byte to publish.
   - Appenders and the flush holder then share one serialized view of the pending range. The re-write is a copy into the page cache of the pending groups only (est. µs to low ms), so the writer-byte gate (p99 ≤ 5 ms) holds.
   - Seeded bug: "re-write outside the writer byte".
3. **Chain the validity of each group to its predecessor.**
   - Seed each group's xxh3 with the checksum of the previous group's `group_end` record. `RecHdr` has no spare 8 bytes, and seeding needs none.
   - A group is then valid only after the exact group it was appended behind. The recovery scan and the pending scan already run sequentially.
   - This is a change to the validity rule, so it enters [60 §2.5]'s Log row at M0.
   - Seeded bug: "accept a group whose predecessor differs".
4. **Keep pending groups out of the read overlay.**
   - Replaying pending groups for re-validation goes into a scratch layer that is discarded when the writer byte is released.
   - A process's read overlay, including the long-lived MCP server's, never advances past the published `committed_lsn`. Otherwise one process serves a group that another process can later lose.
5. **GT1 scenario:** a failed flush, then page revert or eviction, while ≥ 3 live writers are pending and appends continue; checked with idempotent retries. Add "every acknowledged commit is in the final log with the prefix it was validated against" to the model's invariants.

---

### B2 — Linux inode reuse makes E3d and E3 exact on unrelated files (X-F8, R-14)

**What [80] says.**
- `linux_ino` holds `(vol_key, ino)`.
- The generation `gen` is "taken where available" [80 §2.11.2].
- E3 on Linux means "frontier search for `d_ino`; same verification, plus `gen` when stored" [80 §2.11.4]. The frontier says "every hit is verified by size and mtime or by `oid`" [80 §2.11.3].
- The R-14 E3d rule is "exact only if q has `FILEOBS.file_id`, or equal size and mtime, or `oid(q) ∈ {o, last_oid}`" [40 §4.3], and tiny files may be exact from E3 and E3d [40 §4.4].
- ext4 hands out the lowest free inode of the group, and a journaled ext4 does not skip recently freed ones [S, X19 §3.1]. XFS reuses location-derived numbers [I, X19 §3.1].

**Scenario: directory and file reuse together.**
1. The linked file is `src/pkg_a/__init__.py`, inode 5001, in directory `src/pkg_a`, inode 5000. `FILEOBS` stores both, and `DIRMAP` stores 5000.
2. Someone runs `git rm -r src/pkg_a` (or `rm -r`), then `mkdir src/pkg_b` and `printf 'X = 1\n' > src/pkg_b/__init__.py`. The subdirectory and the file are allocated in the parent's group from the lowest free bits, so they plausibly receive 5000 and 5001 again [I from S].
3. At the next read or settle, p and its parent are absent. The frontier sees that `src/` changed and enumerates it. The entry `pkg_b` has `d_ino` 5000, the stored parent id, so D′ = `src/pkg_b`.
4. `q = src/pkg_b/__init__.py` is present, and `statx(q).ino` = 5001, which equals `FILEOBS.file_id`. The rule says exact.
5. The settle writes `moved-auto` as a versioned observation, which merges and exports. The link now reads "ok, content changed" on an unrelated file, and its anchors re-resolve there.
6. For an empty `__init__.py` the `oid` arm alone suffices.

**Related scenario: rm and recreate at the same path.** When the new file gets the same inode number, the path-reuse check of [72 M13] (`file id ≠ FILEOBS.file_id`) never fires either.

**Why this is Linux-only.**
- NTFS's file reference carries a 16-bit sequence number, so `OpenFileById` of a reused slot fails [M, 09 §2.1].
- APFS allocates object ids from a counter [D, X19 §4.1].
- The design therefore imports a Windows-safe rule into an OS where the id is not an identity. That is the definition of axis (c)'s failure: [80 §2.11.5]'s "on every OS, no link ever re-binds wrongly" is false as frozen.

**Fix (M0; R-14 and X-F8).**
1. **Linux file identity is `(vol_key, ino, gen)`, and `gen` is mandatory.**
   - A settle reads `gen` with `name_to_handle_at` for every recorded file and directory. This is unprivileged and has been available since 2.6.39.
   - Every frontier hit is checked the same way before it counts: one call per hit (est. µs).
2. **E3, E3d and the path-reuse check compare the full `OsFileId`.**
   - A hit whose `gen` differs is a different object.
   - A volume whose file handles carry no generation gets `id_kind = none`. Its ids are then never evidence, or at most STRONG.
3. **State in R-14 that `d_ino` equality alone is never identity.**
4. **GT17 and the ext4 simulator profile gain rows** for directory+file inode reuse, empty-file reuse, and rm+recreate at the same path. [X19 §8.7] already asks the profile to model "lowest-free inode reuse with random generations"; the rule has to use it.

---

## 3. Major findings

### M1 — In-process lock ownership differs between Unix and Windows (X-F4)

**What [80] says.**
- §2.2.1 item 2: "one handle or OFD per role per process, opened lazily and reused across successive grants", and "two clients inside one process conflict on every OS, so the simulator and tests can run several clients in one process".
- Item 3 makes re-acquisition "a programming error, asserted in user space".
- [X18 §6.1] instead specified one owner handle per *grant*.

**Divergent scenario.**
1. Client A in process P holds the maintenance byte through the per-role OFD. Client B in P calls `try_acquire(maintenance)`. The byte is try-only by design.
2. On Windows, the same handle's re-lock fails (non-reentrant [M, X18 §5]), so B gets `Busy`.
3. On Linux and macOS, OFD locks of one description merge [D, X18 §2.2–§2.3], so B gets `Granted`.
4. When B releases, `F_UNLCK` on the shared OFD drops A's grant too. The `gc --rollup` child or a CLI then takes maintenance while A still believes it holds it: two concurrent checkpoints.
5. If the item-3 assertion is keyed per `LockFile` (per role), B's call panics instead. Either way, Unix differs from Windows.

Where this can happen:
- the resident MCP server, whose maintenance runs as "resumable ≤ 5 ms slices between requests" [AR §4.5 step 12];
- any test that runs several clients in one process, which is item 2's stated purpose.

The Windows gates would pass, and the defect would appear only in the port. That makes it also a hidden Windows-only assumption (axis e).

**Fix.**
- Ownership is per client instance. A per-process, per-byte table in user space returns `Busy` to a second in-process client before touching the kernel, identically on every OS. Alternatively, one handle or OFD per grant, as [X18 §6.1] item 2 specified.
- Define waiter joining: a hand-off goes to one client, and the others keep waiting on a fresh waiter.
- Freeze this in X-F4, and add an in-process two-client case to the `Vfs` conformance suite of M1.

### M2 — The Windows boot identity moves with the wall clock (X-F2)

**What [80] says.** §2.7.1 takes Windows `boot_id` from "boot time from `NtQuerySystemInformation(SystemTimeOfDayInformation)`". This is inherited from [AR §4.2] and [X20 §8], and neither [72] nor [74] examined its stability.

**Why it is unstable.**
- The NT kernel keeps boot time as `SystemTime − InterruptTime` and adds the delta to it whenever the system time is set: `KeSetSystemTime` adjusts `KeBootTime`, as ReactOS's re-implementation does [I].
- A w32time step correction, a manual clock change, or the clock re-read on resume from sleep or hibernation therefore changes the reported boot time. A commonly reported symptom is WMI's `LastBootUpTime` shifting after time synchronisation [C, unverified here].
- Linux's `boot_id` and macOS's `kern.bootsessionuuid` are random per boot and invariant [D/I].

**Divergent scenario.**
1. The owner's laptop wakes, and w32time steps the clock by 2 s.
2. Every process now sees "boot changed". It runs boot-change recovery, with one extra `HEAD` flush.
3. Every non-run-scoped lease is **Dead** and is released at the first read [80 §2.7.2], [AR §6.2].
4. A second agent claims the live task. The first agent's `complete` then fails with exit 5 (stale token): work is lost or duplicated.
5. The same clock step on Linux or macOS changes nothing. This affects the Windows implementation in M1, not only the ports.

**Fix.**
- X-F2 should require that the boot identity is invariant under wall-clock changes, suspend and hibernation, and constant for exactly one boot.
- On Windows, derive it from a per-boot counter, for example the `BootId` value Windows keeps under the Session Manager's prefetch parameters, or its copy in `KUSER_SHARED_DATA` [I, verify].
- M0 item 22 tests stability across a manual clock step, a sleep and a hibernation, in addition to the boot clock it already covers.

### M3 — The macOS boot identity may be unreadable under Seatbelt, and X-F2 leaves the port no legal fix (X-F2; axes d and f)

**What [80] says.** §2.7.1 uses `sysctl kern.bootsessionuuid` and freezes "An unreadable boot identity refuses the store with exit 7".

**Why it may be unreadable.**
- [X18 §7.2] and [X20 §3.3] read sandbox-runtime's Seatbelt profile as allowing `sysctl-read` for listed prefixes (`kern.proc.pid.`, `kern.proc.all`, `kern.proc.pgrp.`). `kern.bootsessionuuid` is not among those reported [S as quoted, I].
- The sysctl is undocumented.
- [X20 §8] had proposed "`kern.boottime` if readable, else zero"; [80] dropped that fallback.
- [80 §5.2] lists readability as a port probe. But if the probe fails, every sandboxed `moirai` command on macOS exits 7, readers included, because the boot check precedes the first read. X8 forbids the port from changing the rule. The port would then need to reopen M0.

**The private-API standard is applied unevenly (axis f).** [80] rejects macOS 10.13–13 because OFD constants were undocumented there. The same design relies on:
- the undocumented `kern.bootsessionuuid`;
- libproc (`proc_pidinfo`, `proc_pid_rusage`), whose header describes itself as private interfaces subject to change [I];
- on Windows, a `SystemTimeOfDayInformation` query outside the documented API.

**Fix (freeze now; it is OS-independent).**
- Give X-F2 a defined *Unknown-boot* mode:
  - a process that cannot read its boot identity never republishes `boot_id`;
  - it treats every lease's boot test as `Unknown`, which never ends a lease;
  - its readers stay correct, because a record that is invalid in `(durable_lsn, committed_lsn]` already ends the visible log [AR §4.7];
  - its writers already scan to the end of the log under group commit.
- The only loss is that an acknowledged commit whose publish was lost stays invisible to that process until an unsandboxed process (the MCP server or a hook) runs boot recovery.
- Choose the macOS source among documented calls where one exists. Take process start times from `sysctl(KERN_PROC_PID)` (public, allowed under Seatbelt [S]) rather than libproc.
- Record the private-API position for every OS, not only for OFD.

### M4 — Case and normalization twins silently resolve to the other file (X-F7, X-F8)

**What [80] says.** §2.11.4 rule 2: "When the stored spelling stats but enumeration returns another spelling that is equal under the directory's equivalence … the link is `ok (spelling differs on disk)`." P5 guards only the *creation* of such names. The `missing (not representable on this OS)` state exists [80 §2.10], but no resolver rule ever reaches it.

**Scenario.**
1. A Linux author commits `docs/Plan.md` and `docs/plan.md` with different content, and links anchors into both.
2. On a Windows checkout, or a default case-insensitive APFS checkout, only one file exists: git warns and keeps one [M, X19 §5].
3. `stat("docs/Plan.md")` succeeds through the case-insensitive lookup, and enumeration returns `plan.md`. Rule 2 says both links are `ok`.
4. One of them now reads the other file's text, and its anchors re-resolve there as "content changed".
5. The same happens on APFS for NFC and NFD twins that Linux or NTFS hold as two files.

This violates [40 §0.2] ("never the wrong file or the wrong text"). It is exactly the cross-OS collision axis (b) asks about.

**Fix (R-14 and R-16, M0).**
- Rule 2 applies only when no other file node in `PATHIDX` and no other entry of τ(H) is equal to p under the directory's *actual* equivalence.
- Otherwise the node whose exact bytes the enumeration returns resolves normally, and every other fold-equal node is `missing (not representable on this OS)`.
- Never `ok` on a spelling match alone.
- GT17 rows: case twins on NTFS (runnable in M6 on Windows) and on APFS; NFC/NFD twins on APFS.

### M5 — The copy rule's creation-time line is exact on batch-created duplicates (X-F8, R-14)

**What [80] says.**
- Linux birth time is classified `Unforgeable`, and the copy rule keeps "q from E4 and `q.creation = FILEOBS.creation` → exact" for it [80 §2.11.1, §2.11.4 rule 1].
- §2.11.5 says of Linux: "the creation time cannot be forged".
- [X19 §3.2]: "equal btime with a different inode never happens" [I].

**Why that is wrong.**
- Linux file timestamps come from the kernel's coarse clock (one tick, 1–10 ms). Multigrain timestamps since 6.13 cover the ctime and mtime of queried inodes, not the birth time [I].
- NTFS creation times come from the tick-updated system time [I].
- A `git clone` or checkout creates many files per tick.

**Scenario.**
1. `pkg/LICENSE` and `pkg/sub/LICENSE` are identical and not tiny ([40 §4.4]'s tiny-file rule does not apply). The same checkout created both in one tick.
2. The link points to `pkg/sub/LICENSE`, and `pkg/sub` is deleted (a committed `git rm -r`, so there is no E6 rename and no intent).
3. E4 enumerates the parent and finds `pkg/LICENSE`: equal `oid`, equal creation time. The copy rule says exact, and the link re-binds wrongly.

On Linux the line cannot fire *correctly* at all:
- a same-mount move keeps the inode, so E3 finds it first;
- an `EXDEV` move creates a new birth time;
- Linux has no tunneling.

The line therefore only ever fires wrongly on Linux. On Windows it also exists for `MoveFileEx(REPLACE_EXISTING)` tunneling, and the same batch coincidence applies, so this is a pre-existing [40] defect that [80] extends to Linux.

**Fix.**
- `Unforgeable` disables the creation-time line; the result is at most STRONG.
- On Windows (`TunneledNotCopied`), the line counts only if q's creation time is unique among the files the E4 enumeration saw, and differs from the creation time `FILEOBS` recorded for every other node in that scope.
- Correct [80 §2.11.5] and the capability description.
- Add a replay-corpus case built from a real checkout.

### M6 — Under group commit, what the `HEAD` publish contains is unspecified (X-F3)

In the serial protocol, the process that appended a record published that record's effect on `HEAD` in the same writer-byte hold. Under X-F3 the flush holder publishes for everyone. But [80 §2.4.3] step 4, and AR-14 step 10.4, publish only `durable_lsn`, `committed_lsn`, `commit_seq`, `next_id`, `fence` and the ref-table lsn. Step 3 also takes its `HEAD` view outside the writer byte.

**Two failure modes.**
1. **Unpublished effects.** A covered `Checkpoint` (a segment-set change), `Pin`, `ClientHead`, `Marker` or `GitMap` record changes `HEAD` fields: `segments`, `pins_lsn`, `heads_lsn`, `markers_lsn`, `image_cursor`. Nobody is specified to publish them. [80] says maintenance's durable records "use the same phase 2b", but it removes the step where maintenance published its own segment set.
2. **Stale republish.**
   - A flush holder read `HEAD` before maintenance made its "no-op publish" for the two-slot barrier.
   - The flush holder then publishes a slot built from that stale snapshot, which names the old segment set.
   - After GC deletes the old files, the newest slot names missing files. Readers loop on "re-read `HEAD` and retry" and then exit 7.
   - The barrier's guarantee, that no surviving slot names a deleted file, is broken without any crash.

**Fix.**
- A publish is a read-modify-write of the newest valid slot under the writer byte. It folds every covered group's `HEAD` effects onto that slot in log order, and fields only advance.
- Maintenance runs its barrier and GC only after it has seen its own `Checkpoint` covered and published.
- Freeze this in X-F3.
- Seeded bugs: "publish from a stale `HEAD` snapshot" and "cover a `Checkpoint` without publishing its segment set".

### M7 — The edit list leaves the design of record's protocol text inconsistent

[80 §6.1] replaces [AR §4.5] steps 5, 6, 9 and 10 and several rows. It misses text that contradicts the new protocol:

| Place in [AR] | Unedited text | Consequence if implemented as written |
|---|---|---|
| §4.2 "Boot check" | "takes the writer byte (G1 wait), runs recovery" | recovery now needs the flush byte (AR-14: flush, then writer). A process that holds the writer byte and waits for the flush byte, against a flush holder that holds the flush byte and waits for the writer byte, is a deadlock that ends only in two 2 s timeouts and two exit 7s |
| §4.5 step 7 | "If `committed_lsn` = L0, the candidate stands" | pending groups lie *beyond* `committed_lsn`. A candidate computed at L0 = `committed_lsn` is never re-validated against them, so two agents' `claim 12` are both appended and both acknowledged. [80 §2.4.3] step 4 cites step 7's "unchanged rules", but its shortcut condition must become "L0 = the end of the scanned log" |
| §4.5 step 12 | maintenance "re-takes byte 0 … to append the `Checkpoint` record … and publish" | contradicts phase 2b and M6 |
| §4.5 phase-2 heading and intro | "under `LOCK` byte 0"; "held only to re-validate, append, flush and publish" | stale |
| §4.5 "Flush grouping" | "the GC barrier's two slot flushes"; "one flush, ~2 ms" | contradicts AR-13 (one barrier flush) and the flush costs per OS |
| §1 row 18 | "exactly one flush per durable commit" | contradicts the new gate "≤ 1; ≤ 3 per 16-writer burst" |
| §4.10 first bullet | "A commit is acknowledged only after its flush" | should say "after a covering flush and publish" (I-G1) |

**Fix:** add these edits to [80 §6.1] (as AR-40 onward). Re-run the "checks after applying" pass of [60 §9.4] with the search terms `byte 0`, `L0`, `exactly one flush`, `two slot flushes` and `takes the writer byte`.

---

## 4. Minor findings

**m1 — The `file mv` protocol point differs by OS (a; X-F5).**
- [80 §2.3.2] specifies "Windows: `MoveFileExW(MOVEFILE_WRITE_THROUGH)`, plus the directory flush *if M0 item 17 requires it*", while Unix gets "`durable-name` on **both** parents". The protocol point itself therefore depends on the OS.
- `MOVEFILE_WRITE_THROUGH` is documented for moves performed as copy and delete; its effect on a same-volume rename is not established [I].
- The frozen weakest-OS fault item (2) says a rename is durable only after `sync_dir`, so a Windows implementation that skips it fails GT1.
- **Fix:** freeze "no-replace rename, then `durable-name` on both parents" on every OS. M0 item 17 then decides only how Windows *implements* `durable-name`: the directory flush, the flag, or both.

**m2 — No replace-rename or exchange-rename in `os::fs`, and the Unix no-replace fallbacks add crash states (a).**
- `std::fs::rename` is banned [80 §2.1], and `os::fs` offers only `rename_noreplace`.
- Several operations need replace or exchange semantics:
  - git's lock protocol: `packed-refs.lock` → `packed-refs` [80 §2.3.2 image export], and loose `<ref>.lock`;
  - rewriting the store `config`;
  - `restore`'s swap of the store directory [AR §4.10].
- Linux's `EINVAL` fallback `link` + `unlink` [80 §2.11.1] creates a crash state in which both names exist with one inode.
- macOS has no specified fallback when `VOL_CAP_INT_RENAME_EXCL` is clear.
- **Fix:**
  - add `rename_replace` (`MoveFileExW(REPLACE_EXISTING)` / `renameat`) with `durable-name`, and a documented swap (`renameat2(RENAME_EXCHANGE)`, `renamex_np(RENAME_SWAP)`, or two renames with an intent on Windows);
  - list the both-names state among the `FsIntent` recovery states, which [40 §3.4] freezes;
  - define the macOS fallback, or refuse such volumes.

**m3 — The `O_DIRECT` + `RWF_DSYNC` "port optimisation" is not admissible under X8 (a).**
- [80 §2.3.1] admits it "in the port only if a measurement shows it wins; semantics unchanged".
- Direct I/O needs block-aligned offsets and lengths. Groups are not aligned, so the path needs either padding (a format rule) or a read-modify-write of the tail block.
- A read-modify-write races the flush holder's partial-block re-write and the appender's next group.
- **Fix:** reject it now, or freeze its preconditions now (group padding with `Noop` records, and re-write only under the writer byte, as in B1 fix 2).

**m4 — A stranded pending durable group blocks visibility indefinitely (a; X-F3).**
- Take a CLI killed between its append and its flush-byte wait. The harness timeout does this [X18 §8.2].
- Its durable group stays pending until *another durable writer* flushes. Until then no lazy group is published, and readers see nothing new.
- "It becomes visible at the next flush publish, a few ms later" [80 §2.4.3 step 6] is false in that case.
- **Fix:** an appender that finds a pending durable group whose record `hlc` is older than `lock.flush-wait-ms` adopts it: it runs phase 2b. Freeze this with X-F3.

**m5 — Lock-release delay and waiter details (a; X-F4, X-F5 item 8).**
- Item (8) sets "Unix gets 0". The 32 ms Windows figure was measured with `TerminateProcess` [M, X18 §5].
- On every OS a crash reporter keeps a *crashing* process, and therefore its locks and its OFDs, alive for seconds: Windows Error Reporting, systemd-coredump, macOS ReportCrash [I].
- Other details:
  - an abandoned waiter thread stays parked in the MCP server after its caller timed out, contrary to "exists only while a contended acquisition is pending" [80 §2.1];
  - Unix wakes every waiter at once, with no FIFO order [S, X18 §2.3], so spurious 2 s timeouts under 16 writers are a Unix-only risk.
- **Fix:**
  - model the release delay as unbounded on every OS;
  - add "release after `SIGSEGV`/`abort` with crash reporting on" and 16-writer fairness to the port probes;
  - state that fairness is not part of the lock contract;
  - count a parked, abandoned waiter in the idle-thread rule.

**m6 — The fault model has no item for a failing `pread` (a; X-F5).**
- btrfs and ZFS return `EIO` on a checksum failure; so does a failing medium.
- Nothing says whether an unreadable log range is the end of the log or corruption.
- **Fix:**
  - an unreadable range below `durable_lsn` is corruption: exit 7 and `repair`;
  - above `durable_lsn` it ends the log only under B1's identity and chaining rules;
  - the simulator injects read errors.

**m7 — Linux timestamp granularity, and the completeness claim for the frontier (b, c).**
- `mtime_granularity_ns = 1` for Linux [80 §2.11.1] is wrong for kernels before 6.13 (coarse ticks) [I].
- The "racy" rule ("mtime at or after the previous scan's start") must compare against a file-system timestamp, as git compares with the index file's mtime. A process clock can lead the coarse file-system clock and miss a same-tick change.
- Tools that restore directory mtimes defeat "why it is complete" [80 §2.11.3]: `tar -p`, `rsync -t`, `cp -a` into existing directories.
- These failures render `missing` or `unverified`, never a wrong binding. But [80 §2.11.5]'s "exact at settle (frontier)" overstates the guarantee.
- **Fix:** take the racy threshold from a file-system timestamp, record the effective granularity per volume, and document the mtime-restore hole.

**m8 — `fold_v1` is under-specified (b; X-F7).**
- `NFD(simple_casefold(NFD(x)))` [80 P6] uses *simple* case folding, and the "fixed Unicode version" is unnamed in a frozen constant.
- ext4 casefold uses full folding: ß and ss compare equal [I, kernel utf8 tables], so P5's sibling-collision check misses such pairs.
- **Fix:** name the Unicode version, and use full (C + F) case folding. `fold_v1` stays a non-identity key.

**m9 — Ref names have no fold-collision rule (b; X-F9).**
- P11 (b) forbids Windows device names and a `.lock` suffix, but allows `lane/Foo` next to `lane/foo`.
- The image destination is a git repository whose clones on Windows or default macOS store loose refs as files. Fetches create them after the initial `packed-refs`, so the two refs collide.
- **Fix:** refuse fold-equal ref names at creation, NFC-normalise ref-name input, and check it in `doctor image`.

**m10 — The macOS canonical root, and root containment (b, c; X-F7).**
- P9 claims `F_GETPATH` gives "the firmlinked form and the on-disk case". [X19 §4.6] supports only the firmlinked form.
- Apple's own `realpath(3)` replaces each component through `getattrlist(ATTR_CMN_NAME)` *because* lookup names can differ in case [S, X19].
- A case-variant `cd` on APFS could then derive a second `HEADS` binding key, and a directory bound to a lane would look unbound. The same holds in Linux casefold directories.
- The same concern applies to E3 results from `fsgetpath` checked against the root spelling ("resolving outside R → missing").
- **Fix:**
  - canonicalize per component with the on-disk name;
  - look up bindings and root containment by the root's `OsFileId` first (P9 already stores it) and by spelling second.

**m11 — The sandbox write set covers the store only (d; X-F11, X-F12).**
- The default `image.dest.<name>.path` sits beside the main worktree, outside the project, so it is outside the sandbox write set. The same goes for arbitrary `backup DIR` targets. The orchestrator's merge ritual runs `image export` from Bash, which is sandboxed on Linux and macOS.
- On Linux, the detached `gc --rollup` child that a sandboxed CLI spawns [AR §4.5 step 12] dies with the command's PID namespace [S, X18 §8.2], so it never completes and leaves orphan files. [X18] noted the same about a leader "spawned by the CLI".
- T5 puts query files under `${TMPDIR:-/tmp}/moirai/` [80 §4.2]. [80 §2.12] itself says `TMPDIR` differs between sandboxed and unsandboxed processes. The Write tool also cannot expand the variable.
- **Fix:**
  - image exports, backups and rollups run in the unsandboxed MCP server or a hook, or else exit 7 printing the `allowWrite` entry;
  - a CLI that detects it runs in a foreign PID namespace (`getppid() == 0`, or a PID-namespace inode different from the anchor's) never spawns the rollup child;
  - agents pass query text by heredoc only, and `-f` files for scripts go to the store's `tmp/`.

**m12 — Error mapping under the sandboxes and TCC, and two unsupported claims (d).**
- `fsgetpath` (MAC hook), `getattrlistbulk` on TCC-protected folders, `F_OFD_GETLK` from a read-only descriptor, and `setiopolicy_np` may be refused inside Seatbelt or TCC. `EPERM` and `EACCES` must map to `Unknown` or source-absent, never to `Gone` or `missing`.
- `SF_DATALESS` must be checked before every read, not only through the process I/O policy, in case setting that policy is refused.
- [80 §2.5] rule 6 says that after an MCP exit 7 "hooks fall back to the command transport, and the session reconnects the server". [AR §7.5] says a disconnected server makes `mcp_tool` hooks non-blocking errors, and automatic reconnection is not established.
- `${CLAUDE_PLUGIN_DATA}` substitution in an exec-form hook `command` and in `.mcp.json` is "[I, verify at M0/M8]" in [X20 §3.2], but [80 §2.12] and AR-30 state it as fact.
- **Fix:** add the mappings, correct the claim, and tag the substitution for verification in M0 and M8.

**m13 — The Linux parent-death watch is thread-scoped (d).**
- `PR_SET_PDEATHSIG` fires when the *thread* that created the child exits, not the process [D, prctl(2)].
- A Claude runtime that spawns the MCP server from a worker thread would kill the server early. The server's slot is freed and the session's leases become Dead.
- **Fix:** use `pidfd_open(getppid())` with `poll`. It needs 5.3, inside the 5.10 floor, and is event-driven with no timer.

**m14 — Environment-guard inconsistencies (a, e; X-F6).**
- "Allowed, not gated" admits ZFS, f2fs, bcachefs and ReFS/Dev Drive. That contradicts "adding a type is … backed by that type's crash evidence" [80 §2.6].
- The overlayfs `volatile` refusal is dead text under an allow-list that already refuses every overlayfs. A dev container whose workspace is not bind-mounted cannot use moirai at all; say so, or admit overlay on an allowed upper layer.
- `VolumeCaps` has no ZFS `id_kind` row.
- iCloud's "Desktop & Documents" sync of `~/Documents` is not caught by the `~/Library/Mobile Documents` and `CloudStorage` prefixes [X19 §4.7].
- **Fix:** gate or refuse each listed type explicitly, resolve the overlayfs rule, add the ZFS row, and detect cloud-managed `~/Documents` and `~/Desktop`.

**m15 — Format slips, the golden-file claim, and version enforcement (e, f; X-F6, X-F8, X-F12).**
1. **Size check without a size.** X-F6 requires `fstat` to equal "the length the durable record names". But `HEAD`'s `SegRef` has no length, [80 §3.2] says no `HEAD` field is added, and nothing names the lengths of `blobs`, `hist`, `gitmap` or `dict`.
   - Windows never needed a length, because a mapped file cannot be truncated there: a silent Windows-only assumption.
   - Fix: define the expected length per file kind (`SegHdr` section extents, footers), or add `len` to `SegRef` at M0.
2. **`vol_key [16]`.** It cannot hold a 16-byte file-system UUID plus a btrfs subvolume id, and its Linux source switches when the kernel crosses 6.9 (`f_fsid` → `FS_IOC_GETFSUUID`).
   - Fix: `vol_key` = BLAKE3-128 over a source tag and the inputs, with one fixed source per `id_kind`.
3. **The `TREES` `VolumeCaps` snapshot**, "u32 bit field", cannot hold `mtime_granularity_ns`.
4. **T7's "byte-identical output across OSes"** cannot hold for results that print the tree root, OS error texts or per-OS states. Golden files need defined substitutions.
5. **The macOS 14 floor is not enforced.**
   - Rust's `aarch64-apple-darwin` default deployment target is 11.0 [D, X20 §4.1].
   - XNU has implemented OFD locks privately since 10.13, so the `init` probe passes on macOS 13, and "the private range is not used" fails silently.
   - Fix: link with a minimum OS of 14, and check the OS version at open.
   - The Windows floor "22H2" has no stated reason.

---

## 5. What was checked and holds

| Axis | Holds |
|---|---|
| (a) | The durability mapping per class: `NtFlushBuffersFileEx(DATA_SYNC_ONLY)`, `fdatasync`, `F_FULLFSYNC`, never `F_BARRIERFSYNC`, and `ENOTSUP` refuses. A failed flush is fatal and the next holder re-writes; this is correct once B1 is fixed. Explicit `durable-name`. `sync_group` on macOS with one closing barrier. Lock bytes beyond EOF, OFD on both Unixes, no `flock`, CLOEXEC, no spawn while holding a role byte. The waiter-thread timed wait is equivalent to overlapped `LockFileEx` + `CancelIoEx`, and `F_OFD_SETLKWTIMEOUT` is correctly rejected. The lock order is acyclic. The mapping policy (sealed, read-only, whole-file, `FromBytes`, a fault handler). Exit 7 for a store I/O fault, correctly not [X17]'s exit 10, which already means "incomplete result". Choosing leaderless group commit (option D) is right: it is the only option that serves sandboxed CLIs. |
| (b) | Little-endian, 64-bit, no page size in any layout. HLC in UTC milliseconds. OS file ids only in tagged runtime rows, never hashed or exported. Root node uid from the root *name*, not the path [40 §2.3]. EOL-normalised `oid` [40 §2.5]. Image attributes in `info/attributes`. Hashed named-query file names. Numeric store file names. P2/P3 give the same uid for tracked files on every OS. |
| (c) | A missing source contributes nothing. Frontier failures degrade to `unverified (budget)` or `missing`. The macOS clone demotion. `EXDEV` treated as cross-volume. Candidates sorted by exact bytes. B2 and M5 are the exceptions. |
| (d) | Lock-anchored liveness is the right answer to bubblewrap PID namespaces and to Seatbelt's `EPERM`. The direct path is complete, and sandboxed clients never need IPC. Store temporaries live inside the store. Hooks and MCP use the exec form with absolute paths. No per-user writable state on the command path. The `/clear` alias. The refusal plus `allowWrite` fix for sessions started in a subdirectory. |
| (e) | No Linux or macOS build, binary, runner, crash rig or test in M0–M11 ([80 §5.1], 60-5 item 7). The cross-target `cargo check` is put to the owner (#42), builds no binary and runs no test. The GT2 `VolumeCaps` sweep is pure data on Windows. Decision #32 is recorded as DECIDED with the owner's two quotes (AR-34), and its former options are withdrawn. |
| (f) | macOS 14 is correctly identified as the first public-SDK OFD release, and the 10.13–13 private range is refused with reasons. The Linux floor is stated with the version of each feature, and optional features only accelerate. arm64-only macOS is put to the owner (#43). |

---

## 6. Required changes to [80] before it is integrated

1. §2.4.3, X-F3, [60 §2.5] Log row:
   - acknowledge by identity;
   - scan and re-write under the writer byte;
   - chain group validity to the predecessor;
   - no pending group in the read overlay;
   - adopt a stale pending group;
   - `HEAD` publish as a read-modify-write fold of the covered groups;
   - seven new seeded bugs and a GT1 scenario (B1, M6, m4).
2. §2.11.2–§2.11.4, X-F8, R-14:
   - mandatory `gen` for `linux_ino`, compared as a full `OsFileId`, with `d_ino` never identity on its own;
   - the copy-rule creation-time line off for `Unforgeable`, and guarded on Windows;
   - the twin-aware "spelling differs" rule;
   - new GT17 and simulator rows (B2, M4, M5).
3. §2.2.1, X-F4: per-client ownership with a user-space `Busy`, and waiter joining (M1).
4. §2.7.1, X-F2:
   - the boot identity is invariant under clock changes and suspend;
   - a Windows per-boot counter;
   - an Unknown-boot mode instead of refusal;
   - M0 item 22 extended (M2, M3).
5. §6.1: the missing AR edits of M7.
6. The minor fixes m1–m15. None changes the scope of the port phase or the cost estimate by more than about 1 unit, except B1 and M6, which add est. 0.5–1 unit to M0 (specification, model, seeded bugs) and 0.5–1 unit to M1.

After these changes, [80]'s claim that "ports never change format or protocol" becomes credible for the items reviewed here. Without them, B1 and B2 would force an M0 re-opening at the port at the latest, or ship a Windows engine whose group commit can lose acknowledged commits after a failed flush.

---

## 7. Sources

- **Reviewed design:** [80] `docs/research/design/80-cross-platform-design.md`.
- **Cross-platform reports:** [X17] `docs/research/17-xplat-durability-mmap-memory.md` (§3.2, §3.4–§3.6, §3.9, §4.2–§4.6); [X18] `docs/research/18-xplat-locking-ipc-processes.md` (§2.2–§2.6, §3, §5, §6.1, §7.2, §8.1–§8.3); [X19] `docs/research/19-xplat-file-identity-change-tracking.md` (§3.1–§3.3, §4.1–§4.7, §5, §8.3–§8.7, §9); [X20] `docs/research/20-xplat-toolchain-shells-ci-crash-testing.md` (§2, §3.1–§3.4, §4.1, §8).
- **Design of record and designs:** [AR] `docs/ARCHITECTURE-RESEARCH.md` (§1 row 18, §4.1–§4.10, §6.1–§6.5, §7.1, §7.5); [40] `docs/research/design/40-file-links-design.md` (§4.3 cascade and copy rule, §4.4 classification, R-14); [60] `docs/research/design/60-roadmap.md` (§2.5 fault model, protocol decisions, frozen rows); [72] and [74] (boot identity, searched).
- **Inferences marked [I] that the M0 specification review should confirm, all on Windows or from documentation:**
  - the NT kernel adjusts its boot time on every system-time set;
  - the kernel's timestamp granularity per file system;
  - ext4's full case folding;
  - the thread scope of `PR_SET_PDEATHSIG` (documented in prctl(2));
  - libproc's self-description as private.

*End of 81-cross-platform-critique.md.*
