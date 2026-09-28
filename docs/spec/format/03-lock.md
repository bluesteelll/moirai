# 03 — The `LOCK` file

| | |
|---|---|
| Title | The `LOCK` file, layout v1 (X-F1): the lock-byte map beyond the end of the file, `LockHdr`, the writer diagnostics `WriterDiag`, the leader record `LeaderRec`, the 256 liveness-slot records `SlotRec`, the embedded `ProcId`, and the 32-byte holder `Anchor` of X-F2 with the [90 §10.1] amendment (the namespaced process-lifetime identity, lazily taken slots, `session-ttl`) |
| Chapter | [F03], `docs/spec/format/03-lock.md` |
| Status | draft, pass 1 pending |
| Work package | WP-11 (R-SPEC-P), [60 §3.1] item 1 |
| Sources | [80 §3.1] X-F1 (the `LOCK` layout v1 and the lock bytes) and X-F2 (`ProcId`, `Anchor`, the liveness rules, the boot-identity rule, Unknown-boot mode); [80 §2.2.1] items 1, 6, 9; [80 §2.2.3] (the lock map and order); [80 §2.7.1] (the `ProcId` table, `boot_id`, `boot_hash` in anchors); [80 §2.7.2] (the anchor table, "How it works", race windows, slot choice, the MCP server lifetime); [80 §2.8] (`LeaderRec`); [80 §8.7] (the X-F2 amendment); [90 §4.1] (the session and actor rows of the caller-context resolver, harness detection); [90 §4.4] (slots per harness, the frozen amendment, slot pressure); [90 §10.1] row "Holder anchor (X-F2)" and row "`LEASES` runtime rows"; [AR §4.1] `LOCK` row; [AR §6.1] (processes and locks); [AR §6.2] (holder liveness, lease kinds and anchors); [AR §4.4] `LEASES` row (the anchor); [40 §2.6] `FSINTENT` row, [40 §3.4] step 5; [60 §2.5] the [AR] row "`LOCK`", the audit row "`LOCK`", the "Cross-platform" row and the "Harness-agnostic interface" row; `docs/spec/reviews/a1-P.md` (no `LOCK` finding; §4 checks); [PLAN §3.2] WP-11, [PLAN §3.3] |
| Depends on | [F01]; cites [F02], [F04], [F05], [F11], [F15], [F16], [F17], [F19], [OS/lock], [OS/proc], [OS/clock], [OS/fs] |

## 1. Scope

This chapter owns every byte of the `LOCK` file and the byte offsets of every lock byte. It also owns the 32-byte
`Anchor` that `LEASES` rows ([F11]) and `FsIntent` records ([F05]) embed, and the byte layout of `ProcId`.

| Topic | Owner |
|---|---|
| The `LOCK` file, its regions, its records and their checksums; the lock-byte offsets | this chapter |
| The operations on lock bytes (try, bounded wait, release, probe), the in-process grant table, the lock order and the per-OS calls (X-F4) | [OS/lock] |
| The per-OS values of `ProcId`'s fields, the boot identity and `boot_hash`, Unknown-boot mode, the liveness decision procedure | [OS/proc] |
| The deadline form `Stamp` of lease rows | [OS/clock] |
| Which protocol step takes which byte and when; the `init` and `restore` sequences that create `LOCK` | [F16] |
| The `LEASES` and `FSINTENT` rows that embed an `Anchor` and a `ProcId` | [F11]; their log records are [F05]'s |
| The texts of the exit-7 lock diagnostics | [F19] |

**Terms.**
- **Holder** of a byte: the client that the byte's grant belongs to ([OS/lock §4]).
- **Holding**: one continuous period during which one client holds one byte.
- **Session identity**: the namespaced process-lifetime identity of §9, and its **session hash**.
- **Slot i**: liveness slot number i, 0 ≤ i < 256: its lock byte (§3) and its record `SlotRec[i]` (§8).

## 2. The file

### 2.1 Properties

- `LOCK` is exactly **36,864 bytes** (36 KiB) long. `init` and `restore` create it with create-new semantics and write
  all of its bytes ([80] X-F1, [AR §4.1]). It is never deleted, truncated, extended, renamed or mapped by moirai
  ([F02 §5.1]; [OS/lock §3] item 9).
- A process that opens `LOCK` checks that its size is 36,864 bytes. Any other size makes the store unavailable: exit 7
  naming `LOCK` and `moirai doctor --fsck` ([F19]).
- The identity check of `LOCK` (compare the open handle with a fresh query of the path; retry once; then exit 7) is
  [OS/lock §9.1]'s.
- Every lock is one byte, exclusive, and lies beyond the end of the file (§3). No lock ever covers a data byte, so a
  mandatory Windows range lock never blocks a read of any record below.
- `LOCK`'s content is runtime state. It is never flushed after `init` or `restore` (§12), never copied into an image, and
  never hashed into a canonical form. `backup` copies it only because it copies the whole store; `restore` writes a fresh
  one ([F16]).

### 2.2 Regions

Offsets are relative to the start of the file. Integers are little-endian ([F01 §4.1]).

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 64 | `LockHdr` | `hdr` | the header, §4 |
| 64 | 1984 | `[1984]u8` | `_reserved_a` | reserved, **ignored on read** ([F01 §10] rule 4); writers write zero |
| 2048 | 512 | `WriterDiag` | `writer_diag` | the writer diagnostics, §6 |
| 2560 | 512 | `[512]u8` | `_reserved_b` | reserved, ignored on read; writers write zero |
| 3072 | 512 | `LeaderRec` | `leader` | the leader record, §7; all zero unless the optional leader is built and running |
| 3584 | 512 | `[512]u8` | `_reserved_c` | reserved, ignored on read; writers write zero |
| 4096 | 32768 | `[256]SlotRec` | `slots` | the liveness-slot records, §8; `SlotRec[i]` at `4096 + 128 × i` |
| total | 36864 | | | |

The reserved regions are ignored on read because nothing validates them on any path and because refusing a store over
bytes no record uses would give no protection. `doctor --fsck` reports non-zero bytes there (open point 1).

### 2.3 Initial content

`init` and `restore` write `LockHdr` (§4) at offset 0 and zero bytes everywhere else. An all-zero `WriterDiag`,
`LeaderRec` or `SlotRec` fails its checksum and therefore reads as absent (§6.3, §7.3, §8.6). The durability steps of
creation (`durable+meta` on `LOCK`, `durable-name` on the store directory) and their place in the `init` and `restore`
sequences are [F16]'s.

## 3. The lock bytes

`ROLE_BASE` = 2^62 = `0x4000_0000_0000_0000`. `SLOT_BASE` = 2^62 + 2^16 = `0x4000_0000_0001_0000`. These offsets are
frozen by X-F1 and owned by this chapter; [OS/lock §2] restates them for the operations it defines.

| Byte | Offset | Role | Held by ([80 §2.2.3]) | Acquired with ([OS/lock]) | Rank in the lock order |
|---|---|---|---|---|---|
| writer | `ROLE_BASE` + 0 | the innermost byte: append, the flush holder's scan and re-write, every publish of `HEAD` | appenders, the flush holder, every publisher ([F16]) | bounded wait (`lock.writer-wait-ms`, [F17 §10]) | 4 |
| leader | `ROLE_BASE` + 1 | the optional leader ([80 §2.8]) | the leader for its lifetime, only if the leader is built | try | 1 |
| maintenance | `ROLE_BASE` + 2 | checkpoint, promotion, rollup, GC, `restore` | the maintenance holder | try | 2 |
| quiet 0 (quiet-advisory) | `ROLE_BASE` + 3 | process-lifetime quiet mode (§3.1) | a process that requests quiet mode for its own lifetime | try; probed by every maintenance decision | — (never waited for) |
| flush | `ROLE_BASE` + 4 | group commit and boot-change recovery | the flush holder ([80 §2.4.3]) | bounded wait (`lock.flush-wait-ms`) | 3 |
| quiet 1 … quiet 8 | `ROLE_BASE` + 5 … `ROLE_BASE` + 12 | process-lifetime quiet mode (§3.1) | further requesters of quiet mode, one byte each (pass 1, P1-10) | try; probed by every maintenance decision | — (never waited for) |
| reserved | `ROLE_BASE` + 13 … `ROLE_BASE` + 63 | none | never locked by moirai; `ROLE_BASE` + 63 is probed by the foreign-lock check ([OS/lock §11]) | — | — |
| slot i, 0 ≤ i < 256 | `SLOT_BASE` + i | liveness slot i (§8) | a session's MCP server from the moment it knows its session identity until it exits, or a CLI for the life of one file intent | try only; a busy slot sends the caller to the next one (§8.7) | 0 |

- No other byte offset is ever locked by moirai. The ranges `ROLE_BASE + 64 … SLOT_BASE − 1` and
  `SLOT_BASE + 256` and above are unused in format v1.
- The lock order, slot < leader < maintenance < flush < writer, and the rule that waits go only upward and only on the
  writer and flush bytes are [OS/lock §6]'s (X-F4).

### 3.1 The quiet bytes

The design reserves the quiet-advisory byte ([AR §4.1], [80 §2.2.3]) without stating its meaning. This chapter fixes it
(open point 9) and, in review pass 1 (P1-10), adds eight more: the **quiet bytes** are quiet 0 = `ROLE_BASE` + 3 (the
design's quiet-advisory byte) and quiet 1 … quiet 8 = `ROLE_BASE` + 5 … `ROLE_BASE` + 12, nine bytes in all.

1. While any process holds any quiet byte, **quiet mode is in effect** for the store exactly as when `HEAD.flags` bit 0
   is set ([F04 §5.2], [AR §6.6], [F17 §5.3]).
2. A process that decides whether to run automatic maintenance ([F17 §5.2]) evaluates quiet mode as: `HEAD.flags` bit 0
   set, **or** a probe of **any** quiet byte answering `Held` or `Unknown` ([OS/lock §8]); it probes all nine. `Unknown`
   counts as quiet because quiet mode only defers optional maintenance up to its hard cap ([F17 §5.3]); treating an
   unprobeable byte as quiet never weakens a guarantee.
3. A byte ends with its holder: a crashed holder never leaves the store in quiet mode, unlike the persistent flag.
4. **Each requester holds a quiet byte of its own for its whole run.** It tries quiet 0, then quiet 1 … quiet 8 in
   order, with `try_acquire` only, and keeps the first byte granted until it ends. If all nine answer `Busy`, it tries all
   nine again every 10 ms on its monotonic clock for at most `lock.writer-wait-ms` ([F17 §10]), then exits 7
   `store_locked` naming the quiet bytes. A `Busy` answer proves nothing by itself: on Windows it may come from a
   maintenance decider's probe, which holds the byte for a moment ([OS/lock §8]), and another requester may end before
   this one does. Holding its own byte makes the requester's quiet mode last exactly as long as its run.
5. Which verbs take a quiet byte is the CLI's and [CFG]'s (proposal: `moirai quiet hold -- <command>`, and the M0
   measurement drivers of [PLAN §3.2] item 5, whose numbers decide `HOLE(F17-ckpt-ops)` and `HOLE(F17-tail-overlay)` and
   must be taken without maintenance noise).

## 4. `LockHdr`

### 4.1 Layout (64 B at offset 0)

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 4 | `[4]u8` | `magic` | `"MLCK"`: `4D 4C 43 4B` ([F01 §4.5]) |
| 4 | 2 | `u16` | `format` | the format version ([F01 §9.1]); 1 |
| 6 | 2 | `u16` | `flags` | no bit is defined in format v1; every bit reserved-zero |
| 8 | 2 | `u16` | `n_slots` | 256 |
| 10 | 2 | `u16` | `slot_rec_size` | 128 |
| 12 | 8 | `u64` | `role_base` | `ROLE_BASE` = 2^62 (`00 00 00 00 00 00 00 40`) |
| 20 | 8 | `u64` | `slot_base` | `SLOT_BASE` = 2^62 + 2^16 (`00 00 01 00 00 00 00 40`) |
| 28 | 8 | `u64` | `created_hlc` | the creating process's `hlc`-form time at creation: `(max(0, wall_ms) as u64) << 16` ([F01 §5.7], [OS/clock §7]); diagnostics only |
| 36 | 20 | `[20]u8` | `_reserved` | reserved-zero ([F01 §10]) |
| 56 | 8 | `u64` | `xxh3` | XXH3-64, seed 0, over bytes `[0, 56)` of the header, stored as a little-endian `u64` ([F01 §7.2]) |
| total | 64 | | | |

The fields `n_slots`, `slot_rec_size`, `role_base` and `slot_base` restate constants of X-F1. Readers check them and
then use the constants; no reader computes an offset from a header field.

### 4.2 Rules

- **LH-1 (written once).** Only `init` and `restore` write `LockHdr`, before `HEAD` exists in the store directory
  ([F02 §5.5]). No later write changes it.
- **LH-2 (validated before use).** A process validates `LockHdr` before it takes any byte or reads any record of `LOCK`:
  `magic`, the checksum, `format` = 1, the four constants, `flags` = 0 and `_reserved` = 0. A checksum or constant
  failure makes the store unavailable: exit 7 naming `LOCK` and `moirai doctor --fsck` ([F19]). `format` 0 is invalid;
  `format` > 1 is exit 7 naming both versions ([F01 §9.1]).
- **LH-3 (cost).** Validation reads the 64 header bytes once per process and store; the result is cached for the life of
  the `Locks::Client` ([OS/lock §4]).

## 5. `ProcId` and the parent record

### 5.1 `ProcId` (32 B)

`ProcId` is the diagnostic process identity of X-F2. This chapter owns its bytes; [OS/proc §3] owns the value of each
field on each OS and states the same layout (open point 10). No correctness decision reads it ([OS/proc §3.4]).

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 1 | `u8` | `os` | the OS tag of the process ([F01 §3.2]): 1, 2 or 3 |
| 1 | 1 | `u8` | `flags` | the bit table below |
| 2 | 2 | `u16` | `_reserved` | reserved-zero |
| 4 | 4 | `u32` | `pid` | the OS process id |
| 8 | 8 | `u64` | `start` | the process start time in nanoseconds ([OS/proc §3.2]); 0 when `start_known` = 0 |
| 16 | 8 | `u64` | `boot_hash` | `boot_hash` of the process's boot identity ([OS/proc §4.3]); 0 when `boot_known` = 0 (Unknown-boot mode) |
| 24 | 8 | `u64` | `pidns` | Linux: the inode number of `/proc/self/ns/pid`; 0 on Windows and macOS and when `pidns_known` = 0 |
| total | 32 | | | |

| bit | name | meaning |
|---|---|---|
| 0 | `start_known` | `start` holds a value |
| 1 | `start_boot_relative` | `start` counts from the boot (Linux), not from the Unix epoch |
| 2 | `boot_known` | `boot_hash` holds a value |
| 3 | `pidns_known` | `pidns` holds a value |

Bits 4–7 are reserved-zero. A `ProcId` whose `os` is not 1–3 or whose reserved bits or bytes are non-zero is
**uninterpretable**: it is displayed as `?` and never interpreted ([OS/proc §3.1]); the record that contains it stays
valid.

### 5.2 `ParentRec` (16 B)

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 4 | `u32` | `pid` | the parent process id |
| 4 | 4 | `u32` | `_reserved` | reserved-zero |
| 8 | 8 | `u64` | `start` | the parent's start time in the unit of §5.1 for the writer's OS; 0 when unknown |
| total | 16 | | | |

Whether the record holds values is stated by `SlotRec.flags` bits 0 and 1 (§8.3). The values are [OS/proc §3.3]'s.

## 6. `WriterDiag`

### 6.1 Layout (512 B at offset 2048)

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 8 | `u64` | `seq` | the writing client's acquisition counter: 1 at its first acquisition of the writer byte, incremented by 1 at each later one. `(proc, seq)` names one holding |
| 8 | 32 | `ProcId` | `proc` | the holder process (§5.1) |
| 40 | 16 | `b16` | `session_hash` | the primary session hash of the holder's session identity (§9); zero when the holder has none |
| 56 | 400 | `fstr<400>` | `cmd` | the holder's command summary ([F01 §6.2]): at most 398 bytes of UTF-8, §6.2 |
| 456 | 8 | `u64` | `hlc` | `hlc`-form time of the acquisition: `(max(0, wall_ms) as u64) << 16` |
| 464 | 1 | `u8` | `activity` | why the byte is held, the enumeration below |
| 465 | 39 | `[39]u8` | `_reserved` | reserved-zero |
| 504 | 8 | `u64` | `xxh3` | XXH3-64, seed 0, over bytes `[0, 504)` of the record |
| total | 512 | | | |

| value | name | meaning |
|---|---|---|
| 1 | `append` | phase 2a: scan, re-validate and append ([80 §2.4.3]) |
| 2 | `flush-scan` | the flush holder scans and re-writes the pending range |
| 3 | `flush-publish` | the flush holder publishes after its flush |
| 4 | `maintenance` | a maintenance publish or the barrier's no-op publish ([F16]) |
| 5 | `boot-recovery` | boot-change recovery ([F04 §9.4], [F16]) |
| 6 | `restore` | the `restore` swap ([F16]) |
| 7 | `head-update` | a publish that changes only fields kept in `HEAD` (`config_gen`, `quiet`, `readonly`, `retired`; [F04 §6]) |

### 6.2 `cmd`

`cmd` is the verb path of the command, followed by the names of the options given, without their values, each joined by
one space: `tx`, `claim --agent --ttl`, `gc --rollup --if-needed`, `mcp claim`, `file mv --git`. The MCP server writes
`mcp` and the tool name of the request it serves; maintenance inside another command appends ` (maintenance)`. A longer
summary is cut at the last scalar-value boundary at or before 398 bytes ([F01 §6.2]). Values are never written, so no
body, path, key or other user datum reaches `LOCK` (open point 2).

### 6.3 Rules

- **WD-1 (who writes).** Only the holder of the writer byte writes `WriterDiag`, once per holding, immediately after the
  grant and before any other step under the byte. It writes all 512 bytes in one `write_at` of class `lazy`
  ([F15 §4.1]); it never flushes them. **Cost** (pass 1, P1-24): one positional write of 512 B to a page of `LOCK` that
  stays in the page cache, about one system call per holding (three per durable commit: the append, the flush holder's
  scan and its publish); measurement 2 includes it in the writer-hold time it reports ([AR §8.3] SPEED row "writer
  hold"). Writing it only for long holdings would lose the holder in exactly the case WD-4 serves (a holder that dies
  early in a holding), so the record is written every time.
- **WD-2 (failure).** A failed write of `WriterDiag` is not an error of the command: the record is diagnostics only
  ([80] X-F5's error policy covers classes other than `lazy`). The holder continues.
- **WD-3 (no clearing).** The holder does not clear the record at release. A record therefore names the most recent
  holder that wrote one.
- **WD-4 (reading).** A process whose bounded wait for the writer byte timed out ([OS/lock §4]) reads `WriterDiag` once.
  If the checksum matches and `_reserved` is zero, the exit-7 text names the holder from `proc`, `cmd`, `activity` and
  `hlc`, and the liveness of `session_hash` by the session procedure of [OS/proc §6.2] (kind 1, with the record's
  `session_hash` as the anchor id; a zero hash prints "no session"). Otherwise it prints "holder not recorded". The texts
  are [F19]'s.
- **WD-5 (staleness).** Between a grant and the holder's write, and after a holder died before writing, the record names
  an earlier holder. The diagnostic prints the record's `hlc`; nothing decides on it.

## 7. `LeaderRec`

### 7.1 Layout (512 B at offset 3072)

The leader is built only if the M0 measurements require it ([AR §2.2], [60 §3.1] "leader in or out"). The record is
reserved in format v1 either way.

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 8 | `u64` | `seq` | the leader election counter: the `seq` of the valid record the new leader found, plus 1; 1 when none was valid |
| 8 | 32 | `ProcId` | `proc` | the leader process |
| 40 | 2 | `u16` | `proto` | the version of the leader's endpoint protocol; 1 in format v1 |
| 42 | 1 | `u8` | `endpoint_kind` | 1 = named pipe (Windows), 2 = pathname Unix socket (Linux, macOS) ([80 §2.8]); other values invalid |
| 43 | 5 | `[5]u8` | `_reserved` | reserved-zero |
| 48 | 200 | `fstr<200>` | `endpoint` | the endpoint name ([80 §2.8]): at most 198 bytes of UTF-8 |
| 248 | 16 | `b16` | `nonce` | a random 128-bit value drawn from the OS's cryptographically secure source (`Entropy::fill_random`, [OS/README §4.6]) at each election; never all zero (an all-zero draw is drawn again) |
| 264 | 240 | `[240]u8` | `_reserved2` | reserved-zero |
| 504 | 8 | `u64` | `xxh3` | XXH3-64, seed 0, over bytes `[0, 504)` of the record |
| total | 512 | | | |

### 7.2 Rules

- **LR-1.** Only the holder of the leader byte writes `LeaderRec`: after the grant, and before it accepts a connection
  ([80 §2.8]). It reads the previous record to compute `seq`.
- **LR-2.** A client uses `LeaderRec` only if its checksum matches, its reserved bytes are zero, a probe of the leader byte
  answers `Held`, a re-read after the probe gives the same `nonce`, and the connected peer echoes `nonce` ([80 §2.8]).
- **LR-3.** All zero bytes (the initial content) mean "no leader record". While the leader is not built, the region stays
  zero.

### 7.3 Absence

A `LeaderRec` whose checksum fails is absent. Absence sends every client to the direct path, which is complete
([80 §2.8]).

## 8. `SlotRec`

### 8.1 Layout (128 B at offset `4096 + 128 × i`)

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 1 | `u8` | `ver` | the record version ([F01 §9.2]); 1 in format v1 |
| 1 | 1 | `u8` | `kind` | 0 = free, 1 = `session`, 2 = `intent` (§8.2) |
| 2 | 1 | `u8` | `os` | the writer's OS tag ([F01 §3.2]); 1, 2 or 3 |
| 3 | 1 | `u8` | `flags` | §8.3 |
| 4 | 2 | `u16` | `slot` | the slot index i of this record; equal to its position |
| 6 | 2 | `u16` | `_reserved` | reserved-zero |
| 8 | 8 | `u64` | `nonce` | a random non-zero value from the OS's cryptographically secure source (`Entropy::fill_random`, [OS/README §4.6]; a zero draw is drawn again), drawn once per holding: by an intent holder **before** it chooses its slot, because §8.7 starts the search at `nonce mod 256`, and by a server when it takes its slot; kept unchanged for the whole holding (SR-3; pass 1, S1-42) |
| 16 | 16 | `b16` | `session_hash` | kind 1: the **primary** session hash (§9) of the identity whose lifetime the server tracks. Kind 2: the session hash of the CLI's own session identity, zero when it has none (diagnostics) |
| 32 | 16 | `b16` | `alias_hash` | kind 1: the alias hash after Claude Code's `/clear` (§9.3); zero when none. Kind 2: zero |
| 48 | 32 | `ProcId` | `proc` | the holder process (§5.1) |
| 80 | 16 | `ParentRec` | `parent` | the holder's parent process (§5.2) |
| 96 | 8 | `u64` | `acquired_hlc` | `hlc`-form time at which the slot was taken: `(max(0, wall_ms) as u64) << 16` |
| 104 | 16 | `[16]u8` | `_reserved2` | reserved-zero |
| 120 | 8 | `u64` | `xxh3` | XXH3-64, seed 0, over bytes `[0, 120)` of the record |
| total | 128 | | | |

### 8.2 Kinds

| value | name | holder | lifetime of the holding |
|---|---|---|---|
| 0 | free | — | a record with `kind` 0 is free whatever its other bytes |
| 1 | `session` | the MCP server whose process lifetime the session identity tracks ([90 §4.4]): Claude Code's per-session server, Codex's per-thread server | from the moment the server knows its identity (§9.4) until it exits ([OS/proc §7]) |
| 2 | `intent` | the CLI running `file mv` or `file rm` ([40 §3.4], [40 §3.5]) | from before its `FsIntent` append until its `FsIntentDone` or `FsIntentAborted` is durable ([F05 §9.15]–[F05 §9.17]) |

Values 3–255 are invalid. The leader's anchor is its own byte and `LeaderRec` (§10.2); no slot record has a leader kind.

### 8.3 Flags

| bit | name | meaning |
|---|---|---|
| 0 | `parent_known` | `parent.pid` holds a value |
| 1 | `parent_start_known` | `parent.start` holds a value; 0 when bit 0 is 0 |
| 2–3 | `harness` | the harness namespace of the identity that `session_hash` hashes: 0 = none, 1 = `claude`, 2 = `codex`; 3 is invalid |

Bits 4–7 are reserved-zero. For kind 1, `harness` is 1 or 2. `harness` is diagnostics: `doctor agents` uses it to count
slots per harness and to list leaked Codex thread servers ([90 §4.4]); no liveness decision reads it.

### 8.4 Writing a record

- **SR-1 (who writes).** Only the holder of slot i writes `SlotRec[i]`.
- **SR-2 (when).** Right after the slot grant, and **before** the holder serves any request or appends its `FsIntent`,
  the holder writes the complete record in one `write_at` of 128 bytes ([80 §2.7.2] "Race windows"). It never flushes
  it (§12).
- **SR-3 (alias).** A Claude Code server that handles `SessionStart(clear)` rewrites its own record with the new
  `alias_hash` and every other byte unchanged, including `nonce` ([80 §2.7.2] "`/clear`"). Nothing else is ever
  rewritten during a holding.
- **SR-4 (write failure).** If the record cannot be written, the holder releases the slot byte at once and works without
  a slot: a server's leases get anchor `none` (§10.3); `file mv` and `file rm` exit 7 with the "no liveness slot free"
  text ([F19]).
- **SR-5 (orderly release).** Before an orderly release, the holder writes 128 zero bytes over its record, then releases
  the byte. A holder that dies releases the byte through the OS, possibly after an unbounded delay (fault-model item (8),
  [F15 §3.8]), and leaves its record behind.

### 8.5 Validity

`SlotRec[i]` is **live** when all of the following hold; otherwise it is **absent**:

1. `xxh3` matches;
2. `ver` = 1, `kind` ∈ {1, 2}, `os` ∈ {1, 2, 3}, `slot` = i, `nonce` ≠ 0;
3. every reserved bit and byte is zero (`flags` bits 4–7, `_reserved`, `_reserved2`; `parent._reserved`);
4. `harness` ≠ 3; for kind 1, `harness` ∈ {1, 2} and `session_hash` ≠ 0; for kind 2, `alias_hash` = 0.

An absent record is never an error. It matches no anchor, so the liveness procedure treats its slot like a free one.

### 8.6 Reading records (the seqlock rule of X-F1)

- A reader reads the whole slot table with one `read_at` of 32,768 bytes at offset 4096 ([80 §2.7.2] "Cost").
- It considers only live records (§8.5).
- For each record it relies on, it probes the slot byte ([OS/lock §8]) and, when the probe answers `Held`, **re-reads**
  `SlotRec[i]` (one `read_at` of 128 bytes). The record counts as naming the current holding only if the re-read record is
  live and carries the same `nonce` and the same `session_hash` as the first read. `alias_hash` may differ (SR-3).
- The decision procedure that uses these records is [OS/proc §6.2]'s.

### 8.7 Choosing a slot

([80 §2.7.2] "Choosing a slot", [OS/proc §6.4].)

- A server tries slot `s0 = H mod 256` first, where H is the `u64` read little-endian from bytes 0–7 of its primary
  session hash; an intent holder tries `s0 = nonce mod 256`.
- It then tries `s0 + 1`, `s0 + 2`, … modulo 256, with `try_acquire` only, and takes the first byte granted. It never
  waits for a slot.
- With every slot busy: a server works without a slot and its leases get anchor `none`; `file mv` and `file rm` exit 7,
  "no liveness slot free" ([F19]).
- A process holds at most one slot per store at a time.

## 9. The session identity (the X-F2 amendment of [90 §10.1])

### 9.1 The identity string

The session identity is the **namespaced process-lifetime identity** `<harness>:<id>` ([90 §4.4], [80 §8.7]): the
identity whose lifetime one server process tracks.

| Harness | `<harness>` | `<id>` in the server | `<id>` in a CLI or hook |
|---|---|---|---|
| Claude Code | `claude` | the server's `CLAUDE_CODE_SESSION_ID` at start | `CLAUDE_CODE_SESSION_ID`; a hook's `session_id` |
| Codex | `codex` | the `_meta.threadId` of the first call that carries one | `CODEX_THREAD_ID`; a hook's `session_id` (which is the thread id) |

- `<id>` is the value exactly as the harness supplies it: no trimming, no case change, no normalisation. It must be valid
  UTF-8, 1 to 256 bytes long and free of C0 controls; any other value means "no identity".
- A process has **no** identity when neither row applies, when its detection is ambiguous (the variables of more than one
  harness are present, [90 §4.1] "Detection"), or when it runs under a generic harness. Such a process takes no slot, and
  its leases get anchor `none`.
- The identity is never a `MOIRAI_*` variable ([90 §4.1] session row).
- The string `session:<harness>:<id>` that keys a session's client head ([AR §5a.1]) is a different key of [F05 §9.3] and
  [F11]; it is not hashed here.

### 9.2 The session hash

```
session_hash(<harness>:<id>) = BLAKE3-128( UTF-8 bytes of "<harness>:<id>" )
```

The input is one operand, the UTF-8 bytes of the identity string with no length prefix and no terminator ([F01 §7.3]
allows another framing when the owning chapter shows it is unambiguous: `<harness>` never contains `:`, so the first `:`
splits the two parts). The digest is a `b16` ([F01 §7.1]). A digest of 16 zero bytes is treated as "no identity" (its
probability is 2^-128).

### 9.3 Primary and alias hashes

- The **primary** hash of a kind-1 record is the hash of the identity the server took its slot with. It never changes
  during the holding.
- The **alias** hash is used only by Claude Code servers: `SessionStart(clear)` gives the running server a new session id
  for Bash and hooks but not for itself; the handler stores the new identity's hash in `alias_hash` (SR-3). Codex servers
  keep `alias_hash` zero ([90 §4.4]).
- A CLI or hook matches its own hash against both the primary and the alias hash (§10.3 step 2). An anchor always
  stores the **primary** hash, so a lease survives any number of clears ([80 §2.7.2]).

### 9.4 Lazy slots

- A Claude Code server takes its slot at start, because its environment carries its identity.
- A Codex server takes its slot at the first call that carries `_meta.threadId`: its environment carries no identity
  ([90 §4.4]). Its identity is then fixed for the life of the process. A later call that carries a different
  `_meta.threadId` is served, but anchors that call's leases `none`, never on this server's slot.
- A server without an identity never takes a slot.
- A lease taken by a thread whose own server holds no slot gets anchor `none`, never an anchor on another thread's slot
  ([90 §10.1]); §10.3's matching rule enforces it, since a caller matches only its own identity's hash.

## 10. `Anchor`

### 10.1 Layout (32 B)

The holder anchor that `LEASES` rows and `FsIntent` records embed ([80] X-F2 as amended by [90 §10.1]).

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 1 | `u8` | `kind` | 0 = `none`, 1 = `session`, 2 = `intent`, 3 = `leader`, 4 = `session-ttl`; other values invalid |
| 1 | 1 | `u8` | `os` | kind 0: 0. Kinds 1–4: the OS tag of the process that made the anchor ([F01 §3.2]) |
| 2 | 2 | `u16` | `slot` | kinds 1 and 4: the slot hint, the index of the slot whose record matched when the anchor was made; kind 2: the intent's slot, exact; kinds 0 and 3: 0 |
| 4 | 4 | `u32` | `_reserved` | reserved-zero |
| 8 | 16 | `[16]u8` | `id` | kinds 1 and 4: the primary session hash (`b16`). Kinds 2 and 3: the two `u64` fields of §10.2. Kind 0: zero |
| 24 | 8 | `u64` | `boot_hash` | `boot_hash` of the anchoring process's boot ([OS/proc §4.3]); 0 when that process is in Unknown-boot mode (rule U4 of [OS/proc §5]); kind 0: 0 |
| total | 32 | | | |

### 10.2 `id` for kinds 2 and 3

| offset in `Anchor` | width | type | name | meaning |
|---|---|---|---|---|
| 8 | 8 | `u64` | `nonce` | kind 2: the `nonce` of the intent's `SlotRec`. Kind 3: the first 8 bytes of `LeaderRec.nonce` read as a little-endian `u64` (open point 4) |
| 16 | 8 | `u64` | `session_hash_lo` | bytes 0–7 of the anchoring process's own session hash, read as a little-endian `u64`; 0 when it has no identity. Diagnostics only |

### 10.3 Making an anchor

**For an `FsIntent`** (kind 2; [40 §3.4] step 2): the CLI draws its `nonce` (§8.1), takes a slot by §8.7 starting at
`nonce mod 256`, writes its `SlotRec` with that `nonce` (SR-2), and builds `{kind 2, os, slot i, nonce, session_hash_lo,
boot_hash}`. Only then does it append the
`FsIntent` record ([F05 §9.15]).

**For a lease** (kinds 0, 1, 4; [AR §6.2], [90 §4.4]):

1. A process that is itself the session's server and holds slot i builds the anchor from its own record: kind 1 for a
   Claude Code server, kind 4 for a Codex server, `slot` = i, `id` = its primary hash.
2. Any other process (a CLI, a command hook) computes its own session hash h (§9). Without one, the anchor is kind 0.
   Otherwise it reads the slot table once (§8.6) and looks for live kind-1 records r with `r.session_hash` = h or
   `r.alias_hash` = h. It probes each such slot in increasing slot order and re-reads it (§8.6). The first that is held
   and confirmed gives the anchor: kind 1 when the caller's harness is `claude`, kind 4 when it is `codex`; `slot` = that
   index; `id` = **`r.session_hash`** (the primary hash, even when h matched the alias).
3. When no matching record is held and confirmed, the anchor is kind 0 (`none`): the lease lives by its TTL, renewal by
   use and run scope alone ([AR §6.2]).
4. `boot_hash` is the anchoring process's own `boot_hash`, or 0 in Unknown-boot mode.
5. A kind-0 anchor is 32 zero bytes.

Kind 3 is made only by the optional leader for its own records; `LEASES` rows carry kinds 0, 1 and 4 only, and
`FsIntent` records kind 2 only ([90 §10.1]; [F05 §9.4], [F05 §9.15]).

### 10.4 Validity

An anchor is **interpretable** when `kind` ∈ 0–4, `_reserved` = 0, and: for kind 0 every other byte is zero; for kinds
1–4 `os` ∈ {1, 2, 3}; for kinds 1 and 4 `id` is not all zero and `slot` < 256; for kind 2 `nonce` ≠ 0 and `slot` < 256;
for kind 3 `slot` = 0. An uninterpretable anchor answers `Unknown` in every liveness check ([OS/proc §6.2] step A), which
never ends a lease and never recovers an intent.

### 10.5 Liveness

The decision procedure over anchors, slot records, probes and the checker's boot identity is [OS/proc §6.2]'s, and the
deadline test of kind 4 is [OS/clock §4.3]'s. This chapter guarantees their inputs: every slot record a check relies on
is live (§8.5) and confirmed by the re-read of §8.6; a kind-2 check reads exactly `SlotRec[a.slot]`; a kind-3 check reads
`LeaderRec` under LR-2. `Unknown` never ends a lease early and never recovers an intent ([AR §6.2]).

## 11. Checksums

Every checksum of this chapter is XXH3-64 with seed 0, stored as a little-endian `u64` ([F01 §7.1], [F01 §7.2]). No
checksum covers its own field ([F01 §7.4]).

| Structure | Field | Covers | A mismatch means |
|---|---|---|---|
| `LockHdr` | `xxh3` at 56 | `LockHdr` bytes `[0, 56)` | `LOCK` is damaged: exit 7 (LH-2) |
| `WriterDiag` | `xxh3` at 504 | `WriterDiag` bytes `[0, 504)` | the holder is not recorded (WD-4) |
| `LeaderRec` | `xxh3` at 504 | `LeaderRec` bytes `[0, 504)` | no leader record (§7.3) |
| `SlotRec[i]` | `xxh3` at 120 | `SlotRec[i]` bytes `[0, 120)` | the record is absent (§8.5) |

A concurrent write can make a read return a mix of old and new bytes (fault-model item (4), [F15]); the checksum turns
every such read into "absent" or "not recorded", never into a wrong holder.

## 12. Durability

- `LOCK`'s records are written with class `lazy` only ([F15 §4.1]). No record write is ever followed by a flush.
- This is safe because every record is interpreted only together with a lock byte that its writer holds: after a process
  crash the byte is released and the record matches nothing; after an OS crash every byte is released and every record
  matches nothing; a torn record fails its checksum.
- `LockHdr` is made durable once, by the creating `init` or `restore` ([F16]).
- Store files other than `LOCK` never depend on `LOCK`'s record bytes.

## Coverage

| Item | Part covered here | Section |
|---|---|---|
| [60 §2.5] [AR] row "`LOCK`": layout v1, lock bytes beyond EOF at 2^62 + {0 writer, 1 leader, 2 maintenance, 3 quiet-advisory, 4 flush} and 2^62 + 2^16 + i for 256 slots; `LockHdr` at 0, writer diagnostics at 2048, leader record at 3072, slot records at 4096 | complete | §2, §3, §4, §6, §7, §8 |
| [60 §2.5] audit row "`LOCK`": 36 KiB, 256 slot records × 128 B at 4096 with kind, nonce, primary and alias session hashes, `ProcId`; lock bytes beyond EOF | complete | §2, §8 |
| [60 §2.5] "Cross-platform" row: `LOCK` v1; anchors and `ProcId` | the `LOCK` bytes and the anchor and `ProcId` layouts. The boot-identity rule, Unknown-boot mode and the liveness procedure are [OS/proc]'s; the deadline is [OS/clock]'s | §3–§10 |
| [60 §2.5] "Harness-agnostic interface" row: the X-F2 amendment (the namespaced process-lifetime identity hashed; slots taken lazily; no anchor on another thread's slot) | complete | §9, §10.3 |
| [80] X-F1 | complete: the layout, the lock-byte offsets, the seqlock rule | §2–§8, §11 |
| [80] X-F2 | the `ProcId` bytes, the `Anchor` bytes and kinds, the slot records liveness reads, the construction rules; the per-OS values, the boot-identity rule, Unknown-boot mode and the liveness procedure are [OS/proc]'s; `HEAD.boot_id` is [F04]'s; the lease deadline is [OS/clock]'s and [F11]'s | §5, §8–§10 |
| [80] X-F4 | the byte map and ranks only; the contract and the order are [OS/lock]'s | §3 |
| [90 §10.1] "Holder anchor (X-F2)" | complete | §9, §10 |
| [90 §10.1] "`LEASES` runtime rows" | the `anchor ∈ {session, session-ttl, none}` field only; the row is [F11]'s and its log record [F05 §9.4]'s | §10.3 |
| [40] R-7 (`FsIntent` holder) | the intent anchor and its slot; the record is [F05 §9.15]'s | §8.2, §10.3 |
| [80] X-F8 (`FSINTENT` holder: the intent anchor plus a diagnostic `ProcId`) | the two embedded layouts; the row is [F11]'s | §5.1, §10 |

## Holes

None. No value in this chapter is decided by an M0 measurement. Related holes live elsewhere:
`HOLE(F17-lock-writer)` and `HOLE(F17-lock-flush)` ([F17 §10]) bound the waits on the writer and flush bytes;
`HOLE(F15-lock-release)` models release after death; `HOLE(OS-win-boot-source)` ([OS/proc §4.2]) decides the Windows
source of `boot_hash`. Measurement 22 checks `LOCK` v1's bytes on NTFS ([60 §5.2] item 22) but decides no value here.

## Open points for the review

1. **Fixed record sizes and ignored gaps** (§2.2). X-F1 gives `WriterDiag` and `LeaderRec` as "≤ 512 B"; this chapter
   fixes each at exactly 512 bytes, so every checksum range is a constant. The three gaps between records are reserved
   and ignored on read (an explicit [F01 §10] rule-4 exception): refusing a store over bytes no record uses would protect
   nothing. `doctor --fsck` reports them.
2. **`cmd` is `fstr<400>` with option names only** (§6.2; closes [F01] open point 11). "cmd ≤ 400 B" is read as the
   field's width, so the text is at most 398 bytes. Option values are never written: a command line can carry node text,
   paths or keys, and `LOCK` is copied by `backup`. The review may prefer values for a fixed allow-list of options
   (`--branch`, `--lease`).
3. **`WriterDiag.seq` and `activity`** (§6.1). The design names `seq` without a rule. Taking it from the previous record
   would cost a read under the writer byte on every acquisition (three per durable commit), so `seq` is the client's own
   acquisition counter and `(proc, seq)` names a holding. `activity` is added in reserved space so that an exit-7 text
   can say whether the byte is held by an appender, a flush holder or maintenance.
4. **The leader nonce in anchors** (§10.2; a conflict between design rows). [80 §2.8] gives `LeaderRec.nonce` 128 bits,
   while X-F2's anchor holds a `u64` nonce for kind 3. Resolution: the anchor holds the first 8 bytes of the leader's
   nonce, compared with them. Kind 3 exists only if the leader is built.
5. **`SlotRec.flags`** (§8.3). The design names the byte but no bit. This chapter defines `parent_known`,
   `parent_start_known` and the 2-bit `harness`, which `doctor agents` needs to count slots per harness and to find leaked
   Codex servers ([90 §4.4]); no liveness decision reads them.
6. **Free records and release** (§8.2, SR-5). "0 free" is kept as a kind value; an orderly release writes 128 zero bytes,
   which fail the checksum. Liveness never depends on it: after release the byte is free, and a free byte ends every match
   ([OS/proc §6.2]).
7. **The session-hash input** (§9.2). The design says "BLAKE3-128 of the namespaced identity". The input is the identity
   string's UTF-8 bytes with no `lp()` framing and no domain prefix: it is one operand, its first `:` is unambiguous, and
   the hash never leaves the store (it is never exported or compared across stores). Constraints on `<id>` (1–256 bytes,
   UTF-8, no C0 controls) are this chapter's; an id outside them is "no identity", never a refusal.
8. **Anchor construction details** (§10.3). The anchor stores the matched record's primary hash (so a CLI that matched
   through the alias anchors to the primary, as [AR §6.2] requires); kind 1 or 4 follows the caller's harness; the slot
   hint is the lowest confirmed matching slot. A Codex server's identity is fixed at its first `_meta.threadId` (§9.4);
   later calls with another thread id are anchored `none`.
9. **The quiet-advisory byte's meaning** (§3.1). The design reserves the byte ([AR §4.1], [80 §2.2.3] "as in [AR §4.1]")
   but never says what holding it does. Proposed: holding it puts the store in quiet mode for the holder's lifetime; a
   probe answering `Unknown` counts as quiet. [F16] and [F17 §5.3] should cite §3.1, and [CFG] or the CLI should name the
   verb that holds it (proposal `moirai quiet hold -- <command>`), used by the M0 measurement drivers. **Pass 1 (P1-10):**
   the first draft's rule 4 ("a requester that gets `Busy` knows quiet mode is already in effect") was unsound — the
   `Busy` could come from a Windows probe, and the first holder could exit while the second requester still measured.
   Eight more quiet bytes, taken from the reserved range `ROLE_BASE` + 5 … + 12, let each requester hold one of its own;
   the decider probes all nine. X-F1's five role offsets are unchanged; the reserved range shrinks to + 13 … + 63.
   [F17 §5.3] cites §3.1, and [F16] §17.4 L-8 seeds the decider that probes one byte only. Code follow-up for WP-30:
   [OS/lock §2]'s `LockByte::Quiet` takes an index 0–8.
10. **One owner for `ProcId`'s bytes** (§5.1). [OS/proc §3.1] says it "repeats" the layout, and [PLAN §3.2] WP-11 lists
    `ProcId` in this chapter. Proposal: this chapter owns the bytes and [OS/proc] owns the per-OS values, as §5.1 states;
    both tables are identical today. If the review prefers [OS/proc] as owner, §5.1 becomes informative.
11. **No flush of `LOCK` records** (§12). X-F1 names no durability class for them. Every record is interpreted only
    together with a lock byte held by its writer, so `lazy` writes are sufficient, and a flush on a slot take would cost
    a device flush per server start.
12. **A damaged `LockHdr`** (LH-2) makes the store unavailable. `LOCK` is created only by `init` and `restore`
    ([OS/lock §3] item 9), so the repair path is [F16]'s. Proposal: `doctor --fsck --repair-lock` rewrites the 64 header
    bytes in place while holding the maintenance, flush and writer bytes; the lock bytes lie beyond EOF and are unaffected.
13. **Probe `Unknown` for the session match of WD-4** prints "unknown"; no decision depends on `WriterDiag` (WD-5).
</content>
</invoke>
