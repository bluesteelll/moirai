# 16 — The protocol

| | |
|---|---|
| Title | The storage protocol of format v1: the durability class of every protocol point; protocol decisions (a)–(m); the three-phase write; leaderless group commit through the flush byte with the invariants I-G1–I-G6 and the thirteen group-commit bugs they exclude; the publish; the chain rule; readers; the two-slot `HEAD` barrier; recovery, adoption by re-writing and boot-change recovery; log-extent preparation, spare extents, extent heads, rotation, retirement and epochs; maintenance with its long-holding yield, deletion and the orphan sweep; the namespace points (the Windows `file mv` rule included); clocks; the error policy; the mapping policy and the environment guard; the purge of dropped bodies and the readers' dropped-set check. Every rule is numbered P-1…P-102 and carries the seeded bug that violates it |
| Chapter | [F16], `docs/spec/format/16-protocol.md` |
| Status | draft, pass 1 pending |
| Work package | WP-16b, the protocol part of WP-16 ([PLAN §3.2] item 1), author role R-SPEC-P |
| Sources | [60 §2.5]: the issue-2 row "Protocol decisions" (decisions (a)–(f), (j)–(m)) and the paragraph after the audit-rows table that restates (a)–(c) and adds (g)–(i); the audit rows "`HEAD`", "Log", "`Vfs`/`ProjectFs`" and "Cross-platform" (X-F2–X-F6, protocol part); [60 §3.1] items 3 and 4 (the seeded-bug list) and the exit criteria; [60 §3.13] GT1, GT3, GT4; [60 §4.4] item 4. [80 §2.2.1] items 2–5, 7, 9; [80 §2.2.3]; [80 §2.3.1]–[80 §2.3.5]; [80 §2.4.2] (choice D and why positions are not enough); [80 §2.4.3] (the whole protocol, "Invariants" and "Costs"); [80 §2.4.4]; [80 §2.5] rules 1–6 and 8; [80 §2.6]; [80 §2.7.1] (boot-identity rule, Unknown-boot mode); [80 §3.1] X-F2–X-F6. [AR §2.8]; [AR §4.1] "Rules"; [AR §4.2] (durable bound, boot check, two-slot barrier, `retired`); [AR §4.3] (validity, groups, bulk commits); [AR §4.5] steps 1–12 and "Flush grouping"; [AR §4.7]; [AR §4.9]; [AR §4.10]; [AR §6.1]; [AR §6.2] (claims, fencing, holder liveness); [AR §6.4]; [AR §6.5]. [40 §3.4] (the `file mv` protocol and its recovery table, step 5's re-barrier), [40 §3.5]. [72 B1], [72 M1], [72 M2], [72 M3], [72 M8], [72 M9]; [61 B2]; [81 B1], [81 M6], [81 M7], [81 m4]. Reviews: `a1-P.md` A1P-01, A1P-02, A1P-06; `a1-S.md` S-06; `a1-dispositions.md` FS-4 and the chapter-16 rows. [PLAN §3.2] WP-16 and WP-40; [PLAN §3.3] (the `MOVEFILE_WRITE_THROUGH` gap); [PLAN §6.1] #3 and #14; [PLAN §7] E4 |
| Depends on | [F01], [F02], [F03], [F04], [F05], [F13], [F15], [F17]; cites [F06], [F07], [F09], [F10], [F11], [F12], [F14], [F18], [F19], [F20], [API], [CFG], [OS/fs], [OS/lock], [OS/map], [OS/env], [OS/proc], [OS/clock], [OS/project] |

## 1. Scope

### 1.1 What this chapter owns

This chapter owns the **steps**: who takes which lock byte, what each step reads, writes and flushes, in which order, and
what a process may report to its caller after each step. It owns no byte. The structures the steps read and write are
owned elsewhere, and this chapter cites them.

| Topic | Owner |
|---|---|
| Every rule of this chapter (P-1…P-102), the durability class of every protocol point (§3), the seeded-bug catalogue (§17) | this chapter |
| The fault model the rules are proved against; the durability classes and their per-OS calls; the namespace operations | [F15], [OS/fs] |
| `LOCK`, the lock-byte offsets, `WriterDiag`, `SlotRec`, `Anchor` | [F03] |
| The `HEAD` slot, its validity, slot selection, the publish as bytes, the initial slot values | [F04] |
| Extents, `RecHdr`, groups, the chain trailer, record validity, the scan's end-of-log rule, record payloads, the `HEAD` fold | [F05] |
| The invariants I-G1–I-G6 with their enforcement points, model functions and gates | [F13 §3.8] |
| Thresholds, lock-wait bounds, retention windows, the deletion grace | [F17] |
| The lock contract (X-F4), the grant table, bounded waits, probes | [OS/lock] |
| Boot identity, Unknown-boot mode, liveness | [OS/proc] |
| Clocks, stamps, deadlines, the HLC rule | [OS/clock] |
| Mapping registry and fault handler; environment guard | [OS/map], [OS/env] |
| Exit codes and texts | [F19] |

### 1.2 Terms

| Term | Meaning |
|---|---|
| **appender** | a process that appends a group under the writer byte (phase 2a, §5.2) |
| **flush holder** | the process that holds the flush byte and flushes the log (phase 2b, §5.3) |
| **publisher** | a process that writes a `HEAD` slot (§5.4) |
| **scan** | the reading of the log by the chain rule ([F05 §5], §7) from a group boundary whose chain value the process knows |
| **valid log** | the longest run of valid groups from the scan's start ([F05 §5.3]); its **end** is E_v |
| **pending group** | a complete valid group beyond the published `committed_lsn` ([F05 §1]) |
| **scratch layer** | the per-holding replay of pending groups that is discarded when the writer byte is released (P-30) |
| **L0** | a process's replay bound: the group boundary up to which it has validated and applied the log, with the chain value there |
| **E_g** | the end of the group a process appended; with it the process remembers the chain value at E_g (the group's trailer) |
| **covered** | a durable group is covered when a published slot has `durable_lsn` ≥ E_g; a lazy group when a published slot has `committed_lsn` ≥ E_g |
| **identity check** | the read of the 8 trailer bytes at [E_g − 8, E_g) and their comparison with the remembered chain value (P-46) |
| **acknowledge** | report a write's success to its caller (a CLI exit 0 with its result, an MCP result, a hook's success). A durable effect is acknowledged only under I-G1; a lazy effect is reported as published, which promises visibility, not durability |
| **durable publish** | two publishes under one holding of the writer byte, then `durable+meta` on `HEAD` outside it (P-13) |
| **barrier** | the durable publish that precedes a deletion, retirement or recycling (decision (c), P-62) |
| **preparation** | making a log extent's file exist at full length, zero-filled and durable before any group lands in it (P-8) |
| **spare** | the next extent, prepared ahead by maintenance under a temporary name and renamed into place (P-96) |
| **extent head** | the `ExtentHead` group that begins every extent; the epoch-start group is the first of an epoch (P-97, [F05 §4.5]) |
| **long job**, **yield checkpoint** | a maintenance job longer than one delta checkpoint, and the release-free delta checkpoint it runs between its steps to keep the tail bounded (P-98) |
| **E**, **n(L)** | `HEAD.init.log_extent_bytes` and the extent number of lsn L ([F05 §2.3]) |

### 1.3 How the rules are written

- Each rule is written **P-n (name).** followed by its text. The text is normative ([F01 §2.1]).
- Every rule has exactly one **primary seeded bug** B-n: the smallest change of the product or the toy log that violates
  P-n. The catalogue of §17 lists every B-n with the source bugs of [60 §3.1] item 4 and [80 §2.4.4] that coincide with
  it, the gate assertion that detects it and the vehicle that carries it. WP-40 builds one `Bug` switch per B-n whose
  vehicle is the toy log; the others are seeded in the gate §17.3 names ([PLAN §3.2] WP-40, [PLAN §7] E4; open point 1).
- A rule that restates another chapter's rule cites it; where they differ, the owner of the bytes wins for bytes and this
  chapter wins for steps ([F01 §2.4] rule 4).
- The fault model is [F15]'s. "Crash" means a system crash (§2.5 of [F15]); "death" means a process death.

## 2. Roles, locks and the order of acquisitions

The lock bytes are [F03 §3]'s, the operations [OS/lock §4]'s, and the order slot < leader < maintenance < flush < writer
is X-F4's ([80 §2.2.3], [OS/lock §6]).

**P-1 (lock order and waits).** A client waits (`acquire_within`) only for the writer byte and the flush byte, and only
while it holds no byte of equal or higher rank; every other byte is taken with `try_acquire` or probed. The acquisition
sequences of this chapter are exactly these:

| Sequence | Bytes, in order | Rules |
|---|---|---|
| append (phase 2a) | wait writer → release writer | P-27–P-39 |
| group commit (phase 2b) | wait flush → wait writer → release writer → flush → wait writer → publish → release writer → release flush | P-40–P-47 |
| rotation | wait flush → make the extent ready (re-issue the flushes of a spare, or prepare it) → wait writer → append the pad, the extent head and the group → release writer → phase 2b continues with the flush byte held; a purge's forced rotation takes the same sequence inside its maintenance holding and appends no group of its own | P-72, P-97, P-101 |
| spare extent | try maintenance → prepare `tmp/extent.<nonce>` → rename onto `log.<n+1>` → `durable-name` → release maintenance; no writer or flush byte | P-96 |
| boot-change recovery | wait flush → wait writer → re-write → release writer → flush → wait writer → durable publish's two writes → release writer → `HEAD` flush → release flush | P-66 |
| maintenance | try maintenance → its records by phases 2a/2b (a long job: yield checkpoints between its steps) → barrier: wait writer → two publishes → release writer → `HEAD` flush → deletions → release maintenance | P-62, P-76, P-98 |
| `restore` | try maintenance → wait writer → durable publish of `retired` → release both → swap | P-85 |
| `file mv`, `file rm` | try slot → `FsIntent` by phases 2a/2b → rename or unlink → commit by phases 2a/2b → release slot | P-16–P-18 |
| session server | try slot (next slot on `Busy`) → held until exit | [F03 §8.7] |

**P-2 (the writer byte is innermost).** While a client holds the writer byte it never flushes (`sync`, `sync_dir`,
`sync_group`), never waits for any lock, never renames or unlinks, never sleeps (no share-violation retry, no quiescence
wait), never spawns a process and never prepares an extent. It only reads `HEAD` and the log, validates, allocates, writes
groups and slots, and re-writes the pending range (P-43). A client that holds the flush byte never sleeps for a
share-violation retry ([OS/fs §6.3]).

**P-3 (ownership is decided in user space first).** Each product process opens one `Locks` client per store, and every
acquisition passes through the process's grant table before any kernel call ([80 §2.2.1] item 2, [OS/lock §5]), so two
clients of one process never both hold one byte.

**P-4 (`HEAD` changes only by a publish).** Every write of a `HEAD` slot is a publish (P-48) made under the writer byte.
The only exceptions are `init` (P-24) and the building of a restored or repaired store (P-75, P-85), which write `HEAD`
before any other process can discover the store.

## 3. Durability class per protocol point

The classes are [F15 §4]'s (`lazy`, `durable`, `durable+meta`, `durable-name`, `sync_group`) with the per-OS calls of
[F15 §4.2] and [OS/fs §4.4]. On macOS every consecutive run of `durable+meta` and `durable-name` steps before one record
is issued as one `sync_group` ([80 §2.3.2]). "Before R" means that the step's call has returned successfully before the
record R is appended. Every rename step is followed by `durable-name` on **every** parent it touched ([F15] FM-2.4, NS-5).

| P | Protocol point | Steps and classes | Before | Source |
|---|---|---|---|---|
| P-5 | record class tag | a record's `RecHdr.flags` bit 0 is set at append by its kind's class ([F05 §6.1]): 0 for every durable kind; for the configurable sub-kinds, the writer's `durability.lazy-kinds` at that moment | — | [80] X-F5, [AR §6.5] |
| P-6 | durable group (a group with a record whose bit 0 is 0: the kinds of class `durable` — `Commit`, `RefUpdate`, `RefTable`, `ClientHead`, `Lease`, `Marker`, `Idem`, `GitMap`, `Pin`, `Checkpoint`, `Backup`, `FsIntent`, `FsIntentDone`, `FsIntentAborted`, `Reserve` (P-84), `ExtentHead` (P-97), `BodyDrop` (P-101), the reserved `Harvest` ([F05 §9.30], from M9–M10) — and a configurable kind written durable, [F05 §4.7]) | `write_at` by the appender (`lazy`); then, by the flush holder after its re-write (P-42), `durable` on **every** extent file that holds a byte of the flushed range `(durable_lsn, E]` | its acknowledgement (I-G1) | [80 §2.3.2] row 1, [80 §2.4.3] |
| P-7 | lazy group | `write_at` only; published by P-38 or by a covering publish; durable at the next covering flush. A lazy publish never changes `durable_lsn` | — | [80 §2.3.1] `lazy` row, [F05 §6.2] |
| P-8 | log-extent preparation (rotation, spare, epoch start, `init`) | at rotation, under the flush byte (P-72): nothing written for a full-length spare, `create_extent` if the file is absent, or re-preparation in place (`recycle_extent`) if it is shorter than E; then, in every case, `durable+meta` on the extent and `durable-name` on the store directory, and also on `tmp/` when the file existed at full length (a spare's rename has `tmp/` as its other parent, [F15] FM-2.4). Ahead of rotation, under the maintenance byte only (P-96): `create_extent` of `tmp/extent.<nonce>` → `durable+meta` → `rename_noreplace` onto `log.<n+1>` → `durable-name` on `tmp/` and on the store directory. By `init`, `restore` and `repair` in a store no other process can open yet: as at rotation | the first group appended into the extent (its extent head, P-97) | [80 §2.3.2] row 2, [80 §2.3.3], [F05 §2.4] |
| P-9 | rotation pad | one lazy `Noop` group of length r at the old extent's tail, written by the rotating appender immediately before the next extent's head (P-97) and its own group (none at a purge's forced rotation, P-72, P-101) ([F05 §4.4] G-3) | — | [80 §2.4.3] |
| P-10 | sealed file written by a maintenance holder (`seg.base`, `seg.d`, `seg.b`, `hist`, `blobs` of a checkpoint, `dict`, `gitmap`) | `create_new` under its final number (P-78) → `write_at`… → `durable+meta` → `seal` → `durable-name` on the store directory | the `Checkpoint` (or promotion) group that names it | [80 §2.3.2] row 3, [F02 §5.2] rule 2 |
| P-11 | a bulk commit's `cs.<n>` and its `blobs.<n>` | `cs`: `create_new(tmp/cs.<nonce>)` → `write_at`… → `durable+meta` → `rename_noreplace` onto `cs.<n>` → `seal` → `durable-name` on `tmp/` **and** on the store directory; `blobs`: as P-10 | the `Commit` group that names them | [80 §2.3.2] row 3, [AR §4.3], [F06 §9] BK-3, [F15] OP-5 |
| P-12 | `HEAD` publish | one `write_at` of the 4,096-byte slot that does not hold the newest valid state ([F04 §9.1] step 4); never flushed on the commit path (1PC+C) | — | [80 §2.3.2] row 4 |
| P-13 | durable publish (barrier, flag changes, `retired`, boot-change recovery) | under one holding of the writer byte, two publishes (P-48) so both slots hold the newest state; release the writer byte; `durable+meta` on `HEAD` | the deletion, the success report, or the first read (P-14, P-63, P-66) | [80 §2.3.2] row 5, [F04 §9.2], [72 M2], [F15] OP-1 |
| P-14 | deletion of a store file | P-77's conditions → a barrier (P-13) begun after the releasing or claiming `Checkpoint` passed its identity check → `unlink` ([OS/fs §4.7]; `ShareRetry::None`) → `durable-name` on its directory | — | [60 §2.5] (c), [80 §2.3.2] row 5, [AR §4.1] |
| P-15 | boot-change recovery | re-write `(durable_lsn, E_v]` → `durable` on every extent holding a byte of it → durable publish (P-13) with the current `boot_id` | the recovering process's first read of the store | [80 §2.3.2] row 6, [AR §4.2] |
| P-16 | `FsIntent` | its own durable group, acknowledged (P-46) | the first rename or unlink of the intent | [80 §2.3.2] row 7, [40 §3.4] step 2 |
| P-17 | `file mv` rename of a project file | per item, `rename_noreplace` (Windows: `MoveFileExW` with `MOVEFILE_WRITE_THROUGH`, never `MOVEFILE_REPLACE_EXISTING` or `MOVEFILE_COPY_ALLOWED`) → `durable-name` on **both** parent directories | the commit group with `FsIntentDone` | [80 §2.3.2] row 8, [40 §3.4] step 3, [PLAN §6.1] #3 |
| P-18 | `file rm`, `file rm --trash` | `unlink` → `durable-name` on the parent; `--trash`: `rename_noreplace` into `trash/<intent>/<i>` → `durable-name` on the source parent and on `trash/<intent>/` | the commit group with `FsIntentDone` | [80 §2.3.2] row 9, [40 §3.5] |
| P-19 | intent recovery by roll-forward | `durable-name` on every parent of every rolled-forward item (the re-barrier) | the recovery commit and `FsIntentDone` with `recovered` | [40 §3.4] step 5, A1P-02 |
| P-20 | store `config` rewrite | `create_new(tmp/config.<nonce>)` → `write_at` → `durable+meta` → `rename_replace` onto `config` → `durable-name` on the store directory **and** on `tmp/`; then the `config_gen` bump, an ordinary publish (P-48), not flushed | the verb's success report (the bump excepted, [CFG §7.4] step 8) | [80 §2.3.2] row 10, [CFG §7.4], [F15] OP-5 |
| P-21 | `restore` | the restored store complete in its directory — every file `durable+meta`, every name `durable-name`, `HEAD` last and flushed — then `swap_dirs` ([OS/fs §4.9]: the intent `durable+meta` and `durable-name`; every rename followed by `durable-name` on its parents) | the swap; the verb's success report | [80 §2.3.2] row 11, [AR §4.10] |
| P-22 | `backup` | every copied file `durable+meta` → `durable-name` on the backup directory → the backup's `HEAD` last, `durable+meta`, `durable-name` → the durable `Backup` group | the verb's success report | [80 §2.3.2] row 12, [72 M9] |
| P-23 | image export, pack path | pack and idx `durable+meta` → `rename_noreplace` into `objects/pack/` → `durable-name`; `packed-refs.lock` (or `<ref>.lock`) `durable+meta` → `rename_replace` → `durable-name` | the `GitMap` group | [80 §2.3.2] row 13, [AR §5b.6] step 5, [72 M9] |
| P-100 | image export, loose-object path (a run of at most `store.image.loose-pack-threshold` objects, [F17 §9.1]) | per object: its temporary file `durable+meta` → `rename_noreplace` into `objects/<xx>/` (`AlreadyExists` is success: a loose object's name is its content id) → `durable-name` on `objects/<xx>/` and, when the run created that directory, on `objects/`; all before the ref's `.lock` step, which then runs as in P-23 | the ref update and the `GitMap` group | [80 §2.3.2] row 13, [AR §5b.6] step 5, [F14 §13] (pass 1, P1-18) |
| P-24 | `init` | §13.7: every initial entry durable and named before `HEAD`, which is created last through `tmp/head.<nonce>`; each process then runs `durable-name` on `tmp/` and on the store directory before its first acknowledgement (P-88) | discovery of the store ([F02 §5.5]); each process's first acknowledgement | [F02 §2.4], [F02 §5.5] |
| — | `LOCK` records | `lazy` only, never flushed; `LockHdr` made durable by P-24 or P-85 (the owner's rule, [F03 §12]). No P-rule: a record is interpreted only with a lock byte its writer holds, so no crash state of a record can mislead ([F03 §12]'s argument; pass 1, S1-41); the rules that read records have their seeded bugs in §17.4 | — | [80] X-F1 |
| P-99 | pointer file of `init --link` | create-new, one write, `durable+meta` on the file, `durable-name` on its directory, then the success report ([F02 §3.3] rule 6; pass 1, S1-41) | the verb's success report | [F02] open point 1, [80 §2.3.1] |

## 4. The protocol decisions (a)–(m)

[60 §2.5] lists decisions (a)–(f) and (j)–(m) in its "Protocol decisions" row, and the paragraph after its audit-rows
table restates (a)–(c) and adds (g)–(i). The restated text governs. Each decision is implemented by the rules named here;
the review may add decisions, and each added decision becomes a P-rule with its seeded bug (WP-40b).

| # | Decision as restated | Rules |
|---|---|---|
| (a) | Adoption **re-writes** every complete record in `(durable_lsn, end]` from the read buffer, then flushes; under group commit every flush holder does this, under the writer byte, before every flush; each record is applied by kind; flushed groups are adopted all or nothing. The unbuffered-read alternative is dropped | P-42, P-43, P-44, P-65, P-52 |
| (b) | An invalid record in `(durable_lsn, committed_lsn]` is the end of the log (a lost lazy tail); one below `durable_lsn` is corruption and reports `repair`, never silent truncation | P-29, P-58, P-64, P-92 |
| (c) | The barrier makes **both** `HEAD` slots name the post-deletion state and flushes `HEAD` once, outside the writer byte, before every file deletion, extent retirement or recycling; recovery rebuilds the segment set from durable `Checkpoint` records if a slot names a missing file | P-13, P-14, P-62, P-68, P-73, P-74, P-77, P-101 |
| (d) | A lock-free reader that sees a torn `HEAD` slot uses the other slot | P-12, P-61 |
| (e) | Lease TTLs, the HLC and the GC grace are specified against a clock that the fault model's steps cannot break | P-36, P-89 |
| (f) | `ERROR_DISK_FULL` on any write aborts the command without acknowledging it and leaves only files the orphan sweep removes | P-90, P-91 |
| (g) | The first process after a boot change recovers and flushes `HEAD` once before any read | P-15, P-60, P-66, P-67 |
| (h) | Every rename or delete a durable record depends on is followed by a directory barrier; `file mv` is a no-replace rename followed by `durable-name` on both parents on every OS; on Windows also `MOVEFILE_WRITE_THROUGH` until the post-release calibration (measurement 17) shows it unnecessary | P-11, P-17, P-18, P-19, P-20, P-82, P-83, P-96, P-99, P-100 |
| (i) | Every write is three-phase: computed before the lock, re-validated by key and committed under it | P-25, P-34, P-51 |
| (j) | The lock contract and order of [80 §2.2] | P-1, P-2, P-3 |
| (k) | Leaderless group commit through the flush byte with chained group validity, scan and re-write of the pending range under the writer byte, read-modify-write publishes and acknowledgement by identity; I-G1–I-G6 | P-27–P-50, P-53–P-56, P-97 |
| (l) | The `HEAD` barrier flushes outside the writer byte, only after maintenance's own `Checkpoint` is published | P-13, P-62 |
| (m) | The mapping policy and the crash-gated environment guard ([80 §2.5–§2.6]) | P-93, P-94 |

## 5. The write

Every write verb runs in three phases ([AR §4.5], decision (i)); every other writer of durable records — maintenance,
`RefUpdate`, `FsIntent`, `Backup`, `GitMap`, `Pin`, `BodyDrop`, from M9–M10 `Harvest` — uses phases 2a and 2b
([80 §2.4.3] "Other writers").

### 5.1 Phase 1: compute, before any lock

**P-25 (phase 1 holds no lock byte).** Before it takes any lock byte, a writer: opens the store and replays the log up to
the published `committed_lsn` into its overlay, which sets its L0 ([AR §4.5] step 1; the boot check of P-60 first); runs
the idempotency pre-check
at L0, whose hit returns the stored result at once and whose miss is re-evaluated by P-32; resolves the branch, binding
and provenance; computes the candidate, its marker changes from the hold changes its net ops make ([F13 §4.2] MC-1), `affected`, the canonical form with a provisional
`hlc` and the `changeset_digest` ([AR §4.5] steps 2–4); charges `wmem` and applies [F17 §4.4] W1–W4 to the phase-1
encoding; and performs every file-system read and wait of a settle ([AR §4.5] step 4, [72 M11]). Nothing computed in
phase 1 is appended without P-34.

**P-26 (settles on hook and server paths never sleep).** On a hook path and in the MCP server a settle never sleeps: a
re-bind whose quiescence window ([F20]) has not elapsed when the path's time slice ends is dropped and left to the next
settle; every lock wait on these paths is `acquire_within(min(lock.writer-wait-ms, remaining cap))`, and `Busy` drops the
commit, never the evidence already appended (A1P-06).

### 5.2 Phase 2a: append, under the writer byte

**P-27 (bounded writer wait).** The appender acquires the writer byte with `acquire_within(lock.writer-wait-ms)`
([F17 §10]). On timeout it exits 7 `store_locked` ([F19]) with nothing appended. Right after the grant it writes
`WriterDiag` ([F03 §6.3] WD-1).

**P-28 (the slot is read and checked under the writer byte).** The appender reads both slots and selects the newest
valid one S (P-61). Then, in this order:
1. If its boot identity is `Known(b)` and `b` ≠ S.`boot_id` (a zero `boot_id` included), it releases the writer byte, runs
   P-66 and restarts at P-27.
2. If S.`flags.retired` is set, it releases the writer byte; if it has appended nothing for the operation it runs
   discovery again ([F02 §3.6]); if it appended a group for the operation that is not acknowledged, it exits 7
   `outcome_unknown`. Every holder of the writer byte (a flush holder, a maintenance publisher) applies the same check
   after every read of `HEAD`. The one exception is `restore` itself, between the durable publish that sets `retired`
   and the one that clears it after a failed swap (P-85).
3. If S.`flags.readonly` is set and the operation writes, it releases the writer byte and exits 7 `readonly_flag`.

**P-29 (the scan goes to the end of the valid log).** The appender scans from its L0, with the chain value it remembered
there, to the end E_v of the valid log by the chain rule (§7). The scan does not stop at the published `committed_lsn`:
groups beyond it (pending groups, and groups whose publish an OS crash lost) are part of the valid log. At the first
invalid group, at boundary p: if p ≥ S.`durable_lsn`, p is the end of the log; if p < S.`durable_lsn`, the store is
corrupt and the process exits 7 `store_corrupt` naming `moirai repair`, and nothing below `durable_lsn` is ever
overwritten ([F05 §5.3], decision (b)). A missing next extent is such an invalid group at its first byte, so below
`durable_lsn` it is corruption too. A failed read is not an invalid group: at any position it stops the appender by
P-92, which then appends nothing. If the valid log ends at a boundary p with S.`durable_lsn` ≤ p < S.`committed_lsn`
(a lazy tail lost after a crash or a failed flush), the appender first publishes (P-48) `committed_lsn` = p (P-49),
under the same holding, and restarts at P-28. Until that publish other appenders would treat a refill of the hole as
already published, so their allocators and HLC maxima would miss it (I1, I43′) and a reader could see an uncovered
durable group (I-G2).

**P-30 (pending groups go only to a scratch layer).** Groups up to the published `committed_lsn` are applied to the
process's overlay. Groups beyond it are replayed into a scratch layer that is discarded when the writer byte is released.
No process's read overlay ever holds a group beyond the published `committed_lsn` (I-G2).

**P-31 (allocators follow the scanned log).** Every value allocated for the new group is the maximum of the newest slot's
counter and what the whole scanned log implies, pending groups included, exactly as recovery derives it ([F05 §10.2]):
`seq` (from `commit_seq`), `#N` (from `next_id`, with `UIDX` reuse of a known derived uid, I1), `aN` (from
`next_anchor`), a claim's fencing token and lease id (`fence + 1`, [F05 §9.4]), a new ref id (`next_ref_id`), symbol ids
([F05 §8.1] SD-1, SD-4), store-local schema ids ([F08 §8.3]) and sealed-file numbers (P-78). The ranges of every
`Reserve` record in the scanned log count as allocated ([F05 §9.27]), and an extent head's counters are a lower bound
([F05 §9.28]). A value that would exceed its space is refused with exit 7 `id_space_exhausted` and nothing is appended
([F04 §5.7]).

**P-32 (idempotency is evaluated after the scan).** The idempotency key is evaluated only after P-29, against the scanned
log with its pending groups ([AR §4.5] step 6, F-B7). Only an `Idem` record, or a local commit's key pair ([F06]), can be
a hit; a commit parked by P-70 and an imported commit never satisfy a lookup (I27′); the windows are [F17 §11.1]'s.

**P-33 (a hit on a pending group waits for that group's identity).** A hit on a record in a pending group is returned
only after that group passes the identity check: the process runs phase 2b (P-40–P-47) with that group's end and chain
value as E_g, and returns the stored result, `replayed` = true, only on success ([80 §2.4.3] phase 2a step 4).

**P-34 (re-validation by key; a bulk commit by node).** If the scan found no group beyond L0 — no newly published and no
pending group — the candidate stands. `committed_lsn` = L0 is not enough, because pending groups lie beyond it ([81 M7]).
Otherwise the appender replays `(L0, E_v]` and re-validates: the candidate is re-parented in O(1) only if no node, edge,
marker or lease it read or wrote changed, and no commit in the window touched a kind, field or edge kind that a `TX`
`MATCH` reads, runtime predicates (`ready`, `claimed`, `settled_elsewhere`, `deleted_elsewhere`) included ([AR §4.5]
step 7, S-06); else it is recomputed under the lock within `tx.max-work-in-lock`, or the writer byte is released and
phase 1 re-run at most twice before exit 4 with the current values. An inline commit is re-validated **by key**: its
`prev` values are re-serialised under the writer byte ([F06 §7.3]). A **bulk** commit is re-validated **by node**: every
node that owns a row of its `cs.<n>` counts as read and written as a whole, so any commit in the window that touched any
key of such a node forces a phase-1 re-run, which re-streams the file; the sealed `prev` of each row ([F09 §16.4],
[F06 §9] BK-5) therefore still names that node's newest earlier op (pass 1, S1-20). A `BodyDrop` record in the window
([F05 §9.29]) counts as a change of every body key whose value is a hash it drops: a candidate that supplies the bytes of
such a hash, or carries an entry for one (in its record, or for a bulk commit in its `blobs.<n>`), is recomputed by the
paths above with those hashes counted as dropped, the re-run included (a bulk commit re-streams). The recomputation
refuses a supplied body with `body_dropped` and carries no entry for an introduced one ([F06 §8.1] DB-7), so no `Commit`
appended after a `BodyDrop` carries a body it drops ([F13] I-D1 (a); spec sync 3).

**P-35 (final checks on the final encoding).** After the ids of P-31 are filled in, and before the append: a commit whose
`seq` would exceed 2^32 − 1 is refused (exit 7 `id_space_exhausted`); [F17 §4.4] W1 is re-checked on the final
encoding — an inline commit that now exceeds `store.commit.inline-max-bytes` releases the writer byte and re-runs
phase 1 on the bulk path (bulk-class verbs; this counts as one of P-34's re-runs) or is refused with E501 (agent verbs);
and W3 is re-checked — a group that no extent can hold is refused with exit 7 `commit_too_large` ([F06 §4.6]). In every
refusal nothing is appended.

**P-36 (the HLC is assigned at append, one sequence over the semantic records).** Every append-time HLC of the group is
assigned here, in group order, by [OS/clock §7]'s rule `hlc_next(wall_ms, h) = max((max(0, wall_ms) as u64) << 16,
h + 1)`, over two maxima that the appender derives from its scan exactly as the `HEAD` fold does ([F05 §10.2],
[F04 §5.15]):

- `h_seq`, the greatest value of the store's **HLC sequence**: the HLCs of the **semantic durable records** — `Commit`
  (`append_hlc`), `RefUpdate`, `ClientHead`, `Lease`, `Marker` (one value per record, carried by each entry), `Idem`,
  `Backup`, `FsIntent`, `FsIntentDone`, `FsIntentAborted`, `BodyDrop` ([F05 §9.29]) and, from M9–M10, the reserved
  `Harvest` ([F05 §9.30]), whose commands the model executes and whose effect is visible (spec sync 3);
- `h_commit`, the greatest `hlc` of any commit the store holds, local or imported.

Each is the maximum of the newest slot's field (`hlc_seq`, `hlc_commit`), of the records of the groups the scan found
beyond that slot's `committed_lsn` (pending groups included) and of the records of the group already built; no scan
below `committed_lsn` is needed, because P-29 first lowers a `committed_lsn` that lies beyond the end of the valid log.
Then:

1. A semantic durable record takes `hlc_next(wall_ms, h_seq)` and raises `h_seq` to it. A local commit takes
   `hlc_next(wall_ms, max(h_seq, h_commit))` as its `hlc`, which is also its `append_hlc` ([F06 §4.4.4]), so it lies
   above every commit the store holds; an imported commit keeps its own `hlc` and takes only its `append_hlc` from the
   sequence.
2. Every other record with an HLC field — `Checkpoint.append_hlc`, `Reserve.hlc`, `Lazy`, `SessionMark`, the HLC values
   of the lazy runtime rows — carries `hlc_next(wall_ms, h_seq)` and raises nothing. So class-I maintenance, a lazy
   record, or the loss of a lazy record in a crash never changes a later commit's `hlc`, hence no commit id
   ([F17 §1.5] SP-1).

This is [API §6.2] CK-4, the rule of record (pass 1, S1-13, P1-5, A1-17). The semantic records' HLCs are strictly
increasing in log order, and `append_hlc` strictly in `seq` order (I43′), which [F05 §9.9]'s unsigned `dhlc` needs. A
commit's `commit_id` is computed from its final `hlc` here, in O(1) over the unchanged `changeset_digest` ([F07]). The
maxima never restart: an epoch re-roll carries them in its epoch-start extent head (P-75, P-97).

**P-37 (the append).** Filling the ids of P-31 into an inline commit re-serialises its `prev` deltas, `#N` placeholders,
record checksum and chain under the writer byte, which is O(`cs_bytes`), up to P05 ([F06] open point 16); measurement 2
sweeps inline sizes up to P05 and reports the writer hold, and [F06] names the fallback if the hold gate fails (pass 1,
P1-26). The appender writes the group in one `write_at` at E_v, the end of the valid log it scanned —
never at the published `committed_lsn` and never elsewhere — with every record's `lsn` equal to its position and the
trailer seeded with the chain value at E_v ([F05 §4.3]). It remembers E_g and the chain value at E_g. A group that does not
fit the rest of the extent goes through P-72.

**P-38 (a lazy group is published at once only when no durable group is pending).** After appending a lazy group, the
appender publishes at once (P-48; `committed_lsn` = E_g) if and only if the scanned log beyond the published
`committed_lsn` holds no durable group. Otherwise the group is published by the publish that covers it. The group is
covered only when that publish's `committed_lsn` ≥ E_g; otherwise it was lost (P-47): the publish's own scan can stop
before E_g at a predecessor sector that a failed flush poisoned ([F15] FM-3.2).

**P-39 (a lazy group behind a pending durable group runs phase 2b).** An appender whose lazy group was not published by
P-38 runs phase 2b for it, so that a pending group whose writer died never strands it ([81 m4]). The one exception is a
best-effort evidence append (R4's evidence hooks), which drops its record when the writer byte is busy and skips phase 2b;
its record becomes visible with the next covering publish ([80 §2.4.3]).

Then the appender releases the writer byte (P-1, P-2).

### 5.3 Phase 2b: make durable, then acknowledge

Phase 2b runs for every durable group and for every lazy group of P-39.

**P-40 (the covered test).** The process reads `HEAD`. A durable group is covered only when the newest valid slot has
`durable_lsn` ≥ E_g; a lazy group when it has `committed_lsn` ≥ E_g. A covered group goes to the identity check (P-46);
`committed_lsn` never stands in for `durable_lsn`.

**P-41 (bounded flush wait).** Otherwise the process acquires the flush byte with `acquire_within(lock.flush-wait-ms)`
([F17 §10]). On timeout it exits 7 `outcome_pending` ([F19]): the group is appended and not acknowledged, and a retry with
the same idempotency key finds it (P-32) and completes phase 2b for it (P-33). It then acquires the writer byte (the lock
order, P-1), reads `HEAD`, applies P-28's `retired` check and, if the group is covered now, releases both bytes and goes to
P-46.

**P-42 (re-write before every flush).** The flush holder scans `(durable_lsn, E]` by the chain rule, E being the end of
the last complete valid group, and re-writes **every byte** of `(durable_lsn, E]` from its scan buffer before it flushes.
It never flushes a range it did not re-write in the same holding of the flush byte, and never relies on an earlier
successful flush of the range: a flush after a failed one proves nothing ([F15] FM-3.4, FM-3.5; decision (a)). If its own
group is not inside `(durable_lsn, E]` with its own chain value at E_g, it releases both bytes and goes to P-47. A failed
read of the range stops it by P-92: it re-writes and flushes nothing.

**P-43 (the scan and the re-write are under the writer byte).** P-42's scan and re-write happen while the flush holder
holds the writer byte, so that appenders and the flush holder share one view of the pending range ([80 §2.4.3]).

**P-44 (one flush, outside the writer byte, and the error policy).** The flush holder releases the writer byte and
flushes (`durable`) every extent file that holds a byte of `(durable_lsn, E]` (P-6). Any error from the flush ends the
process through `fail_stop` ([OS/fs §4.4.5]; exit 7 `durability_failure`) without an acknowledgement; the flush is never
retried on the same handle; the flush byte is released by the process's death, and the next flush holder repairs the
range by P-42.

**P-45 (the publish after a flush).** The flush holder re-acquires the writer byte and publishes (P-48) with
`durable_lsn` = max(the slot's `durable_lsn`, E) and `committed_lsn` by P-49; it releases the writer byte, then the flush
byte. `durable_lsn` never decreases.

**P-46 (acknowledgement by identity).** A process acknowledges its group only after reading the 8 bytes at
[E_g − 8, E_g) and finding them equal to the chain value it remembered, after the group was covered. A read error there
(FM-12) counts as a mismatch. This holds for an idempotent replay too (P-33). Acknowledgement by position alone is
forbidden.

**P-47 (a lost group is never acknowledged).** A group that P-42 does not find, or that fails P-46, was lost before it
became durable (its pages reverted, were invalidated or evicted after another process's failed flush, and the hole may
have been refilled). The process does not acknowledge it and does not re-append its old bytes: it re-runs phases 1–2 with
the same idempotency key at most twice, then exits 7 `outcome_unknown` ([80 §2.4.3] phase 2b step 6).

### 5.4 The publish

**P-48 (every publish is a read-modify-write of the newest valid slot).** A publish — a lazy publish (P-38), a flush
holder's publish (P-45), either write of a durable publish (P-13), maintenance's no-op publish, a `config_gen` bump —
is made under the writer byte: read both slots, select the newest valid slot S by P-61, build S′ from S, and write S′
into the slot that does not hold S ([F04 §9.1]). `slot_seq` = S.`slot_seq` + 1. `boot_id` is copied from S except by
P-66. No publisher writes a slot from a state it read before the current holding of the writer byte. A publisher that
would write a slot failing [F04 §7] check 4 or 5 (for example `committed_lsn` < `durable_lsn`, when after a failed flush
its own scan reads a shorter log than it flushed) writes nothing and exits 7 `store_corrupt`.

**P-49 (`committed_lsn`).** A publish sets `committed_lsn` to the end of the valid log as the publisher scanned it at
publish time, stopping before the first pending durable group that its flush does not cover (a publish without a flush
covers none). Lazy groups appended during a flush are therefore published with it. `committed_lsn` decreases only when the
valid log ends below it (a lazy tail lost after a crash or a failed flush), and then to that end ([80 §2.4.3], I-G6).

**P-50 (the fold).** A publish folds the `HEAD` effects of every group between S.`durable_lsn` and the new
`committed_lsn`, in log order, by [F05 §10.2] (a group S already folded changes nothing: every field takes the maximum or
advances). It starts at `durable_lsn`, not `committed_lsn`: after a crash `HEAD` can revert to a slot whose
`committed_lsn` covers a lazy tail that was lost and refilled before the crash, and a fold from `committed_lsn` would
miss the refill (repeated counters and HLCs, I1, I43′). The fold: counters take the maximum; the table pointers advance
to the newest covered record of their kind; a covered `Checkpoint` sets the segment set, `checkpoint_lsn`, `active_log`,
`flags` bit 1 and `next_file_no`; a covered `GitMap` advances `image_cursor`; `seq_ring` gains the covered commits;
`hlc_seq` and `hlc_commit` take the maximum over the covered semantic records and commits (P-36). Fields kept in `HEAD`
([F04 §6]) are copied unless the publish is the one that changes them. No field decreases except by P-49, so no publish
can republish an older segment set.

### 5.5 Phase 3

**P-51 (phase 3 runs after every byte is released).** After its acknowledgement the process prints its result; then, if
quiet mode is off, it evaluates the maintenance triggers of [F17 §5] and, if one holds, runs maintenance under P-76 — never
while it still holds the writer or flush byte. A write made while the process holds the maintenance byte (its own
`Checkpoint`, intent recovery's commits) runs no phase 3, since it would acquire a byte it already holds ([OS/lock]
contract item 3). A rollup never runs in the MCP server or in a CLI or hook process: the
process spawns the detached `moirai gc --rollup --if-needed` child after releasing every role byte ([AR §4.9],
[80 §2.2.1] item 7).

### 5.6 Group composition

**P-52 (effects adopted together are in one group).** A writer puts every record that must be adopted with another into
the same group, as [F05 §4.7] lists: a commit with the `Marker` records of the marker changes it causes, its `Lease`, `Idem`, `RefUpdate`
and `RefTable` records and, for `file mv` and `file rm`, its `FsIntentDone`; a `RefUpdate` with the `RefTable` entries of
the refs it moves and with the fork's `Pin` (P-81); a sync-first merge's two commits; a `Checkpoint` with the `Pin`
records its promotions move, and a purge's `Checkpoint` with the `Pin` records that move pins to its replacements
(P-101 step 7). An `FsIntent` is alone in its group (P-16), and so is a `BodyDrop` command's record; an import's
`BodyDrop` record stands before the first `Commit` of the import that names one of its hashes ([F05 §9.29]). From
M9–M10, an `Apply` batch's `Harvest` records follow, in its group, the `Commit` record their range names, and a
`HarvestMark` or `HarvestForget` record is alone in its group ([F05 §9.30]). Recovery adopts a group all or nothing
([F05 §4.1]), so a completion is never adopted without its marker ([72 M1]), and a harvested range never without the
records harvested from it.

## 6. Invariants I-G1–I-G6 and the bugs each excludes

I-G1–I-G6 are stated in [80 §2.4.3] and entered, with enforcement points, model functions and gates, in [F13 §3.8]. The
thirteen group-commit bugs of [80 §2.4.4] (G1–G13, §17.1) are the negative space: each is a minimal change that breaks
exactly one invariant, and the rules named here exclude it.

| Invariant | Statement (summary; [F13 §3.8] is normative) | Excluded bugs → excluding rule | Other rules it rests on |
|---|---|---|---|
| I-G1 | An acknowledgement implies a successful flush begun after the group's bytes were last written, a covering publish and a passed identity check; also for an idempotent replay | G1 → P-40; G6 → P-33; G7 → P-46 | P-42, P-44, P-45 |
| I-G2 | Readers never see a durable-class group before a flush covers it; `committed_lsn` never passes a pending durable group; no overlay holds a group beyond the published `committed_lsn` | G2 → P-38; G10 → P-30 | P-49, P-57, P-60 |
| I-G3 | The log is a chain: a group is valid only behind the exact predecessor it was validated against; after any crash or failed flush the valid log is a prefix of that chain, and nothing acknowledged depends on a lost group | G9 → P-53 | P-37, P-56, P-64 |
| I-G4 | At most one log flush in flight per store; the flush holder scans and re-writes under the writer byte and never flushes or waits while holding it | G4 → P-1; G8 → P-43 | P-2, P-44 |
| I-G5 | An appended group whose writer dies is adopted by the next flush holder or lost with everything after it, acknowledged only through an identity check; its key makes a retry exact | G3 → P-42; G13 → P-39 | P-33, P-47 |
| I-G6 | Every publish is a read-modify-write of the newest valid slot under the writer byte that folds, in log order, every group above the slot's `durable_lsn` that it covers (P-50); `durable_lsn`, the counters and the lsn pointers never decrease; `committed_lsn` decreases only after a lost lazy tail | G5 → P-45; G11 → P-48; G12 → P-50 (the set not published) and P-62 (the barrier before the publish) | P-49 |

## 7. The chain rule

**P-53 (group validity).** A group is valid only when every record in it is valid ([F05 §5.2]) and its trailer equals
XXH3-64 over the group's bytes before the trailer, seeded with the chain value at its first byte: `XXH3-64(epoch)` at
`HEAD.epoch_lsn`, else the 8 bytes before the group ([F05 §4.2], [F05 §4.3], X-F3). A scan stops at the first invalid
group; it never skips one to accept a later valid-looking group.

**P-54 (a record is valid only at its own position).** A record whose `lsn` differs from its position is invalid, even
when its epoch and checksum match ([F05 §5.2] check 5, [72 M1]).

**P-55 (the epoch).** A record whose `epoch` differs from the `epoch` of the slot the scanning process uses is invalid,
and no scan starts below `HEAD.epoch_lsn` ([F05 §5.2] check 6, [F05 §5.1]).

**P-56 (a remembered replay bound is re-checked).** A process that keeps an overlay remembers its bound L0 with the chain
value there. Whenever it reads a slot with a new `slot_seq`, and before any scan from L0, it re-reads the 8 bytes before
L0 (or recomputes the epoch seed at `epoch_lsn`); if they differ, or if the slot's `committed_lsn` < L0, it drops its
overlay and replays from its segment set ([80 §2.4.3] "Readers").

## 8. Readers

Readers take no lock byte ([AR §6.1]); their correctness comes from immutable sealed files, `committed_lsn`, the chain
rule and P-56.

**P-57 (readers never read past `committed_lsn`).** A reader replays only `[checkpoint_lsn, committed_lsn)` of the slot
it selected ([F05 §5.3], F-A1).

**P-58 (an invalid group is judged by its position).** An invalid group at boundary p with `durable_lsn` ≤ p <
`committed_lsn` ends the reader's visible log — a lazy tail lost after a crash or a failed flush — and is never an
error. One below `durable_lsn` is corruption: exit 7 `store_corrupt` naming `moirai repair`. A reader never truncates its
view silently below `durable_lsn` ([72 B1], decision (b)).

**P-59 (a missing named file).** A reader that fails to open or map a file its selected slot names (`NotFound`,
`DeletePending`, `AccessDenied`, or a size that differs from `total_len`) re-reads `HEAD` once and retries with the newest
slot; if that slot still names the file and the failure repeats, it exits 7 (`store_corrupt`, or `sealed_size` for the
size case) ([AR §4.7], [OS/fs §6.4], [OS/map §4]). It never falls back to an older segment set.

**P-60 (the boot check comes before the first read).** A process whose boot identity is `Known(b)` compares `b` with the
selected slot's `boot_id` before its first read of the store. If they differ (a zero `boot_id` included), it runs
boot-change recovery (P-66) before it reads ([AR §4.2], decision (g)). A process in Unknown-boot mode skips the check
(P-67). A process that cannot run P-66 because the store is read-only to it (`store_read_only`, [OS/env §7]) reads under
the Unknown-boot rules U5–U6 of [OS/proc §5] for that operation.

**P-102 (readers look up the dropped set first).** A process that resolves a body hash — a reader, a writer's phase 1, a
merge, an export — first looks it up in the **dropped set of its view**: the `DROPPED` table of its segment set
([F11 §13.4]) and the hashes of the `BodyDrop` records ([F05 §9.29]) in the log range it replays (P-57; for a writer, its
scanned log, P-29, P-34). A dropped hash resolves to no bytes, even while a file of its view still holds them (a purge
in progress, P-101), and the process renders or treats the body as [F06 §8.1] DB-6 and DB-7 state. Only a hash that is
not dropped is resolved through the tail records and then `BLOBTAB` ([F06 §8] BD-6, [F10 §5.5]); BD-6's obligation does
not cover a dropped hash, so a `BLOBTAB` may hold no entry or a dropped `BlobRef` for it ([F09 §6.3]; spec sync 3).

## 9. `HEAD`: slot selection, the durable publish and the barrier

**P-61 (slot selection).** Every process selects a slot by [F04 §8.1]: a slot that passes its checksum but fails a
validity check is fatal (exit 7 `store_corrupt`), and the process never falls back to the other slot; with one slot
absent it uses the other (decision (d)); with both absent it reads again at most twice more and then exits 7
`store_corrupt` ("`HEAD` has no valid slot"), for `moirai repair`. A failed `HEAD` flush can leave both slots failing
validation ([F15] OP-1); P-13's second write ends that state at the next durable publish, and until then P-61 applies.
So can a publish whose write failed (`DiskFull`, [F15] FM-5.2) or was cut by its writer's death ([F15 §2.5]), which
leaves its slot any mix of bytes, followed by a crash that tears the other, dirty slot (FM-1.2) ([F04 §8.1]). A fatal
slot's repair path is plain `moirai repair`, which treats it as absent and rebuilds both slots from the extent heads
(P-85).

**P-62 (the barrier comes after maintenance's own `Checkpoint` and before every deletion).** Before any file deletion,
extent retirement or recycling, the deleting process runs a barrier (P-13), and it starts that barrier only after the
`Checkpoint` that released the file, or that claimed its number (P-78), has passed its identity check (P-46). The barrier
carries no change of its own: both slots then name states at or after that `Checkpoint` ([AR §4.2], decision (l),
[80 §2.4.3] "Maintenance").

**P-63 (flag changes are durable publishes).** `quiet`, `readonly` and `retired` change only by a durable publish (P-13)
whose first write carries the flag, and the verb reports success only after the `HEAD` flush returned ([F04 §9.3]). A
`config_gen` bump is an ordinary publish (P-20).

## 10. Recovery

"Recovery" is what a process does with the log beyond its own replay bound: the scan of P-29 and P-64, adoption by the next
flush holder, and boot-change recovery. There is no separate recovery pass on the commit path.

**P-64 (the recovery scan starts at the durable bound).** A process without an overlay, and boot-change recovery, scan
from `min(durable_lsn, checkpoint_lsn)` of the selected slot, which equals `checkpoint_lsn` in a valid slot
([F04 §5.4]), with the chain value there ([F05 §5.1]). They never start at `committed_lsn` and never treat bytes below
`committed_lsn` as durable: `committed_lsn` may have been published past a lazy group that a crash lost ([72 B1] scenario B).

**P-65 (adoption applies every record kind).** A group beyond the published `committed_lsn` whose writer died is adopted
only by a flush holder's re-write, flush and publish (P-42–P-45) and then applied by every process's replay in log order,
record by record, by kind ([F05 §10]): commits with their implied ref moves (P-69), `RefUpdate`, `RefTable`, `Lease`,
`ClientHead`, `Pin`, `GitMap`, `Checkpoint`, `Idem`, `Backup`, `FsIntent*`, `BodyDrop`, `Harvest` and the runtime kinds.
The `MARKERS` fold applies the `Marker` records, which carry every change of a marker's holder set or flag (ME-001 to
ME-011; the storage moves of ME-012 and ME-013 write none), and derives nothing from net ops ([F05 §9.5], [F05 §10.3];
[72 M1] fix 2 keeps them in the commit's group). No record kind is skipped.

**P-66 (boot-change recovery).** A process that P-28 or P-60 sends here, and that is not in Unknown-boot mode:
1. acquires the flush byte, then the writer byte (P-1);
2. reads `HEAD`; if the newest valid slot's `boot_id` now equals its boot identity, another process recovered: it releases
   both bytes and continues;
3. scans from P-64's start to E_v by the chain rule and re-writes every byte of every complete group in
   `(durable_lsn, E_v]` (a failed read stops it by P-92);
4. releases the writer byte and flushes (`durable`) every extent holding a byte of that range (an error: P-44);
5. acquires the writer byte and makes the two writes of a durable publish (P-13), the first setting `boot_id` to its boot
   identity, `durable_lsn` = max(the slot's value, E_v) and `committed_lsn` by P-49;
6. releases the writer byte, flushes `HEAD` (`durable+meta`) and releases the flush byte.

It runs once per boot, off the commit path, before the process's first read ([AR §4.2], [80 §2.4.3]). Afterwards every
read by any process reflects every acknowledged durable record (I-G2's post-crash read freshness).

**P-67 (Unknown-boot mode).** A process whose boot identity is `Unknown` ([OS/proc §5]) never runs P-66 (U2), never
writes a `boot_id` other than the one its selected slot holds (U1), and scans to the end of the valid log whenever it
writes (P-29; U6), so a group acknowledged before a crash whose publish was lost is published by its next covering flush.

**P-68 (a slot that names a missing file).** When recovery or maintenance finds that a file named by the selected slot is
missing or fails its identity check against `SegRef.blake3_16` ([F04 §4.1]) after the retry of P-59, it does not continue
on a partial set: under the maintenance byte it rebuilds the segment set from the newest durable `Checkpoint` record whose
files all exist, replaying the log (and `hist`) from that set's bound, writes the rebuilt files by P-10 and publishes them
by a `Checkpoint` through phases 2a and 2b; when no such record exists it exits 7 `store_corrupt` naming
`moirai repair` ([AR §4.2], [72 M2], decision (c)).

**P-69 (a commit's ref move is implied by its own record).** No record separate from a `Commit` moves its ref: adopting
the `Commit` implies the move `ref_old → commit_id`, CAS-checked against the ref table as replayed in log order
([F06] `ref_old`, [F05 §10.3]). A `RefTable` record is written only in a `RefUpdate` group or by `gc` ([F05 §9.10]).

**P-70 (a failed ref CAS parks the commit).** A commit whose CAS fails at replay is not applied to its ref. Every replay
treats it as parked on `orphans/<R>` (I27′): it is on no live branch view and never satisfies an idempotency lookup
(P-32). The first appender of a durable group whose scan meets an unparked failing commit appends, before its own group
and under the same holding, a durable ref group that moves `orphans/<R>` to the commit, creating it on its first use:
one `RefUpdate` with reason 5 `park` (`old` = the previous orphans tip, or zero when the record creates the ref; `new` =
the parked commit) and its `RefTable` entry ([F05 §9.2], §9.10, [F12 §8.2]; pass 1, P1-3, S1-11, A1-11).

**P-71 (intent recovery).** For every open `FsIntent` whose intent anchor is Dead by [OS/proc §6.2], the next writer
(in its phase 3, under the maintenance byte taken by try) or `doctor` examines the paths and decides by [40 §3.4]'s
recovery table; a roll-forward first runs P-19's re-barrier, then appends the recovery commit with `FsIntentDone`
(`recovered`); an abort appends `FsIntentAborted` with its reason (1–5, [F05 §9.17]). Before appending, under the writer
byte, it re-checks that the intent is still open. An intent whose anchor is Alive or Unknown is left alone. A re-barrier
whose `sync_dir` fails with `Unsupported` or `AccessDenied` — a project volume that cannot flush a directory, which the
plan step normally refuses first ([API §12.4] step 1) — appends nothing, leaves the intent open, and `doctor` names it
with the `no_dir_flush` text ([F19 §10.2]); recovery never fails the process that runs it for that reason, since it runs
in phase 3 after that process's own write. Any other `sync_dir` failure is P-91's `fail_stop` (pass 1, P1-16).

## 11. Log extents

### 11.1 Preparation and rotation

**P-72 (rotation).** The first group of an extent m — its extent head (P-97) — is appended only by a process that holds
the flush byte and has made log.<m> ready (P-8) during that holding:
1. An appender whose group would begin in extent m at or after its first byte with no group of m in the valid log —
   because E_v is the first byte of extent m, or because its group does not fit the rest of the extent holding E_v
   ([F05 §4.4] G-3) and m = n(E_v) + 1 — releases the writer byte and acquires the flush byte
   (`acquire_within(lock.flush-wait-ms)`; on timeout exit 7 `store_locked` with nothing appended).
2. It makes log.<m> ready: if the file exists with length E (a spare of P-96, or a preparation that a dead process
   completed), it writes nothing to it; if the file is absent, `create_extent` ([OS/fs §4.5]); if it exists shorter than
   E, `recycle_extent` rewrites it in place to length E and zero content, for every extent method ([OS/fs §4.5]); if it
   exists longer than E, it exits 7 `store_corrupt`. A `Sparse` preparation first applies the free-space check of
   [OS/fs §4.5] (exit 7 `disk_full` on failure). Then, in every case, `durable+meta` on log.<m> and `durable-name` on the
   store directory — re-issued on a spare too, because the durability of a size or a name that another process set is not
   known ([F15] FM-2.1; on a clean spare the calls return in well under a millisecond, pass 1, P1-7). When log.<m>
   existed at full length it also runs `durable-name` on `tmp/`: a spare's rename (P-96) has `tmp/` as its other parent
   and is durable only once both parents are synced ([F15] FM-2.4), and a preparer that died between its rename and its
   `tmp/` sync leaves that unknown.
3. It acquires the writer byte and restarts phase 2a at P-28. If its group would still begin extent m, it appends the pad
   of P-9 when G-3 requires one, then the extent head of m at its first byte (P-97), then its own group; otherwise it
   appends by P-37 (and if its group would now begin another extent, it releases both bytes and starts again at step 1).
4. It continues with phase 2b holding the flush byte (P-41's wait is skipped).

A purge's **forced rotation** (P-101 step 1, [F05 §4.4] G-3) takes the same steps with no group of its own: m is
n(E_v) + 1, or n(E_v) when E_v is an extent's first byte; under the writer byte it appends the pad (when E_v is not an
extent's first byte) and the extent head of m, and nothing after them, then runs phase 2b for the head, a durable group.
When E_v lies right after an extent head, nothing is forced.

A process that holds only the writer byte appends into extent m only when its scanned valid log already holds a group in
extent m. The expensive part of a preparation — writing E zero bytes — happens under the flush byte only when no spare
exists (P-96 prepares one ahead), so a rotation normally adds two sub-millisecond flushes to the flush holder's hold
([AR §8.3] SPEED rows "writer hold" and "last acknowledgement"; measurement 2's workload includes rotations, [F17] Holes
`F17-lock-flush`).

A file of extent m that is shorter than E and lies beyond the end of the valid log is the leftover of an interrupted
preparation (a death, a crash, or `DiskFull` during `create_extent`, [F15] FM-5.4); a full-length one is a spare or a
completed preparation. Neither ever held a group, because P-8 completes before any append, neither is read as log (a scan
that reaches it meets zeros, an invalid record), and the length rule of [F05 §2.2] does not apply to them.

**P-96 (a spare extent is prepared ahead, off the commit path).** A maintenance holder (P-76; in background mode, holding
neither the writer nor the flush byte) that finds the end of the valid log in extent n at or beyond offset E / 2 of it,
and no file `log.<n+1>`, prepares the spare: `create_extent` of `tmp/extent.<nonce>` ([F02 §5.3], [OS/fs §4.5]),
`durable+meta` on it, `rename_noreplace` onto `log.<n+1>`, and `durable-name` on `tmp/` and on the store directory.
`AlreadyExists` at the rename means a rotation made the extent ready first; the holder then deletes its temporary. After
the rename it never writes to `log.<n+1>` again, so a rotator that finds the name finds a complete file that no preparer
still writes, and it only re-issues the flushes of P-72 step 2 before the extent head enters it. Preparing under a
temporary name is what makes this safe without the flush byte: a preparer paused for any time (FM-6) can never write
zeros over a group, because it writes only to a name no appender uses. The trigger point E / 2 is a protocol constant
([F17 §13.2]; pass 1, P1-7, closing open point 3).

**P-97 (every extent begins with its extent head).** The first group of every extent is one durable `ExtentHead` record
([F05 §4.5], §9.28): the chain value at its own lsn, `epoch_lsn`, `init`, `project_oid_algo`, the `quiet` and `readonly`
flags of the newest slot the writer read, the counters P-31 derives before it, and `hlc_seq` and `hlc_commit` as P-36
derives them before it. The rotating appender writes it (P-72 step 3); `init` (P-88), `restore` and `repair` (P-75) write
it as the epoch-start group. Retirement keeps it in `hist` ([F10 §4.1]). It is what a `repair` without a valid `HEAD` slot
starts from (P-85): the head of the lowest surviving extent of the epoch authenticates itself by its record checksum, its
position and its trailer recomputed with the `chain_in` it carries, and every later group follows by the chain rule
(pass 1, P1-8).

### 11.2 Retirement, reuse and epochs

**P-73 (retirement).** A maintenance holder retires extent n only through a `Checkpoint` retirement entry ([F05 §2.5]
EX-3) when every record in it lies below the new `checkpoint_lsn` (EX-4) and that `checkpoint_lsn` > n·E (EX-5), oldest
first, while more than `store.log-active-extents` extents are unretired ([F17 §4.2]). The `hist` file that receives its
history is written by P-10 before the `Checkpoint`. The extent's file is deleted only by P-14 and P-77. An extent that
holds a `BodyDrop` record of origin `command` ([F05 §9.29]) is retired only by a purge's last `Checkpoint` (P-101
step 7), which retires it whatever `store.log-active-extents` says, or by the build of P-75, which purges first; until
then it and every later extent stay active, so "an active extent holds a `BodyDrop` record of origin `command`" is the
test for a pending purge (spec sync 3). A record of origin `import` names no bytes the store holds ([F05 §9.29]
"Origin"), needs no purge, and holds no retirement back.

**P-74 (nothing is reused).** An lsn, an extent number and a sealed-file number are used once in a store's life. The file
of a retired or deleted extent is never renamed, zero-filled or otherwise reused in the store; in format v1
`recycle_extent` serves only P-72 step 2's re-preparation of an extent that never held a group, and no live store reuses
an extent file (G25's "zero-fill recycled extents" is met by building every new epoch in a new extent, P-75). A spare
(P-96) is not a reuse: it is a new file under a number no group has used.

**P-75 (epoch re-roll).** `restore` and `repair --rebuild-from-log` install a new epoch only in a store they build
completely before any process can open it (P-85): every extent below the new first extent m is retired; m is greater
than every extent number the store used; the new epoch is drawn from the OS's cryptographically secure source
(`Entropy::fill_random`, [OS/README §4.6]), non-zero and different from the old; log.<m> is prepared (P-8) and holds the
epoch-start group — the extent head of m (P-97) — at `epoch_lsn` = (m − 1)·E; and both `HEAD` slots carry the new epoch
and are durable before the store becomes discoverable ([F05 §2.6], [F04 §5.3], [F04 §9.5]). The epoch-start head
carries `hlc_seq` and `hlc_commit` of at least the store it replaces (for `restore`, the maximum of the backup's values
and those of the live store's newest slot), so the HLC sequence never restarts and a machine whose clock is behind never
assigns a new commit an `hlc` below a restored one ([API §6.2] CK-6; pass 1, P1-8, P1-44). Because the build retires
every extent, it ends every pending purge, so it applies P-101's rewrites (steps 3–6) to the files it builds and copies
before it writes `HEAD`: a rebuilt store holds no byte of a body its log drops.

## 12. Maintenance, deletion and the orphan sweep

**P-76 (one maintenance holder).** Every delta checkpoint, runtime-only fold, tiered fold, promotion, retirement, rollup,
GC, orphan sweep, spare-extent preparation (P-96), intent recovery (P-71), backup (P-87) and body purge (P-101) runs
under the maintenance byte, taken by `try_acquire` only: `Busy` makes an automatic trigger skip its work and an explicit
verb exit 7 `maintenance_busy` ([F19 §10.2]; pass 1, P1-31; `BodyDrop`'s purge excepted, P-101). Its durable records
go through phases 2a and 2b. A new segment set becomes visible only with the publish that covers its `Checkpoint`;
readers keep the old set until then ([AR §4.5] step 12, [80 §2.4.3] "Maintenance").
No automatic trigger ([F17 §5], P-51) runs a body purge, whatever the number of active extents: a purge is a long job of
a rollup's size (P-98), which no CLI, hook or MCP process runs (P-51, [F17 §6.2]), and no trigger spawns a detached run
for it (a `gc` run spawned for a rollup finishes it as every `gc` run does, [F17 §11.2]). A purge is left pending only
when `BodyDrop` found the maintenance byte busy, a run was cut short, or bulk commits kept it from publishing (P-101);
`BodyDrop`, `gc` and `backup` finish it (P-101), and until then `doctor` reports it (`purge_pending`, [F19],
[API §8.6]) and more than `store.log-active-extents` extents may stay active ([F17 §4.2]).

**P-98 (a long holding keeps the tail bounded).** A **long job** — a rollup, a GC rewrite of `hist`, `blobs` or `gitmap`
files, a `backup` copy, a body purge's rewrites (P-101) — divides its work into **steps** of at most one output file
written and sealed, or one file copied. At every step boundary it evaluates C1 of [F17 §5.2] over the current tail with
m = 1 (and the quiet cap of [F17 §5.3] while quiet mode is on), and when C1 holds it runs one **yield checkpoint** under
its own holding before its next step: a delta checkpoint over the segment set of the newest slot that folds the tail up to the
published `committed_lsn`, by phases 2a and 2b, and does nothing else — no promotion, retirement, rollup or GC, and no
tiered fold except of the yield deltas of the same holding, which it folds into its own new delta and releases. It
therefore never releases a file the job reads or copies (the job's inputs are the set of the slot it started from, which
no yield delta belongs to), and the job's yield deltas occupy at most one `HEAD.segments` entry ([F17] C-4) and one
`gitmap` page per pair ([F10 §7.1]) at any time. Its `next_file_no` exceeds the numbers of the outputs the job has
written and not yet named, but it claims none of them (P-78), so the job names them in its publish.
A rollup whose input set gained yield deltas re-folds their window over its new base in the `Checkpoint` that publishes
the rollup, replaying the window from the log, so no delta built over the old base survives it (the old base's `TOPO`
positions do not carry over, [F09 §5.4]). A body purge rewrites the yield deltas of its holding before its publish,
since they may name the `blobs` files the purge replaces (P-101 step 7). While a long job runs, the tail therefore
exceeds C1's bound by at most the tail that writers append during one of its steps ([F17 §5.2]; pass 1, P1-9, P1-43).

**P-77 (when a file may be deleted).** A store file is deleted only when all of these hold:
1. a covered `Checkpoint` released it ([F05 §9.9] `released`, or a retirement entry for an extent), or claimed its number
   as an orphan (P-78, P-79);
2. the newest valid slot does not name it — neither its segment set nor that set's `FILES` registry ([F09 §14.4]); for a
   log extent, its number is below that slot's `active_log` — and no record of the scanned valid log names it, pending
   groups included (condition 5 covers the other slot);
3. no pin references it ([F11] `PINS`, the pins of pending groups included);
4. for a released file, `gc.delete-grace` has elapsed since the `append_hlc` of the releasing `Checkpoint`, measured by
   P-89 ([F17 §11.4]); a purge's deletions (P-101 step 8) do not wait for it;
5. P-62's barrier has returned after the `Checkpoint` of condition 1 passed its identity check.

A delete that fails with a sharing violation, leaves the file delete-pending or finds it absent is harmless and is
repeated by the next run ([OS/fs §6.4]). The grace only spares readers a retry: no correctness condition depends on it
(P-89).

**P-78 (file numbers and claims).** A process that creates a numbered sealed file (every family except `log`) takes the
number n = max(`next_file_no` of the newest slot, the greatest `next_file_no` and 1 + the greatest file number that its
scanned log implies) and creates the file with create-new semantics; if the name exists it tries n + 1 ([F04 §5.13]). A
number is **claimed** by whichever durable group lands first: a group that names the file (a `Checkpoint`, a bulk `Commit`,
a reservation of P-84), or a `Checkpoint` whose `next_file_no` exceeds n while no group names the file. Before appending a
group that names a file it created, the creator checks under the writer byte that the scanned log has not claimed n by
the second form; if it has, the creator releases the writer byte and re-runs phase 1 with a new file. A sweeper deletes
only numbers it claimed by the second form.
A `Checkpoint` appended under one holding of the maintenance byte (P-76) claims, by the second form, no number created
under the same holding and not yet named: the creator's check ignores such claims, which no sweeper can act on while
the holding lasts (P-76, P-79), and a sweep under that holding leaves those files alone. After the holding ends, a
sweeper claims an unnamed number by its own `Checkpoint` as before. So a long job (P-98) names, in a later `Checkpoint`
of its holding, files it wrote before its yield checkpoints, and a purge names files it wrote before a restart's step-2
checkpoint (P-101 step 7), although each of those `Checkpoint` records carries a `next_file_no` above their numbers
([F05 §9.9]; spec sync 3).

**P-79 (the orphan sweep).** The orphan sweep runs under the maintenance byte (P-76) and touches only names of the grammar
of [F02 §6.3]; a foreign entry is never opened, renamed or deleted ([F02 §5.6]).
- A numbered file that no slot, record or pin names (P-77 conditions 2 and 3, pending groups included) is claimed by the
  sweeper's next `Checkpoint` (P-78; never a number created under the sweeper's own holding, which a later holding's
  sweep claims), then deleted by P-14 and P-77 once that `Checkpoint` passed its identity check.
- A log extent beyond the end of the valid log — a spare (P-96) or an interrupted preparation (P-72) — is never swept:
  the next rotation makes it ready and appends into it.
- An entry of `tmp/` is deleted when its last-modification time is more than `gc.delete-grace` before the sweeper's wall
  clock. Deleting a live process's temporary is safe: that process's next rename of it fails, and it aborts without an
  acknowledgement (a spare preparer included, P-96). This covers `probe.<nonce>` files a dead probe left and the fixed
  `settle.stamp`, which the next settle recreates ([F02 §5.3]; pass 1, P1-32, S1-38).
- An interrupted `init`'s directory, which has no `HEAD` and so no store, is `init`'s and `doctor`'s ([F02 §2.4],
  [OS/env §5]).

**P-80 (what a checkpoint folds).** A delta checkpoint or runtime-only fold folds only groups up to a group boundary u at
or below the published `committed_lsn` of the slot it read; its `Checkpoint` carries u as `upto_lsn` (or `rt_upto_lsn`,
[F05 §9.9]); the segment set it publishes keeps in its `BLOBTAB` every body that a `Commit` record after u references
without carrying it, except a dropped one ([F06 §8] BD-6, [F06 §8.1]); and a promotion writes its `seg.b<ref_id>.<K>` by
P-10 before its record. No checkpoint, fold, rollup, promotion, blob GC or retirement seals the bytes of a body in the
dropped set of its scanned log, indexes such a body as text, or trains a dictionary on it: a `BLOBTAB` entry it keeps for
the body is a dropped `BlobRef` ([F09 §6.3], §12.1), and a retirement leaves the body's entries out of the `hist` file it
writes ([F10 §4.1]). A body dropped after the holder read its view is removed by the purge (P-101).

**P-81 (pins are written with what they protect).** A fork's `Pin` is in the fork's `RefUpdate` group; the pins a
promotion moves are in its `Checkpoint` group; the pin of a merge into `main` (holder 3) is written in the group of the
first `Checkpoint` whose set folds that merge commit ([F12 §8.1], [F05 §4.7] checkpoint group); a tag's `--pin` is in
the tag's `RefUpdate` group. GC's reachability ([F17 §11.2]) and P-77 condition 3 count every pin of the scanned log.

**P-101 (the body purge).** After a `BodyDrop` record ([F05 §9.29]; [F06 §8.1] DB-1) is published, the **purge** removes
the bytes of the bodies it drops from every store file ([F06 §8.1] DB-8). The purge is maintenance under the maintenance
byte (P-76) and a long job (P-98), of class I: it changes no result, commit id or digest ([F17 §1.5] SP-1). A purge is
**pending** while an active extent ([F05 §2.5]) holds a `BodyDrop` record of origin `command` (P-73); an import's record
(origin `import`) names no bytes the store holds and leaves nothing to purge ([F05 §9.29] "Origin"). The `BodyDrop`
command runs a pending purge after its record's acknowledgement (P-46), or after its checks when it writes no record
because every hash was already dropped, taking the maintenance byte by `try_acquire`: `Busy` leaves it pending, and the
command still exits 0, since the drop is acknowledged, with `purged` false ([API §8.7]). Quiet mode does not stop that
run, because quiet mode does not refuse `BodyDrop` ([API §8.7]). `gc` runs a pending purge before its other work, under
its own quiet-mode rule ([F17 §11.2], [API §8.5]), and so does `backup` (P-87); no automatic trigger runs one (P-76).
The steps, in this order:
1. **Rotate.** Holding the maintenance byte, the purge waits for the flush byte, makes the next extent ready, and then
   waits for the writer byte, in the order of P-1's rotation sequence (the writer byte is innermost, P-2). Under both it
   ends the extent that holds the end of the valid log by a forced rotation (P-72; [F05 §4.4] G-3): it appends the pad
   and the extent head of m and no group of its own, releases the writer byte, and makes the head durable by phase 2b
   with the flush byte held, so that the valid log continues in extent m. Its **purge set** H is every hash dropped by a
   `BodyDrop` record below (m − 1)·E: the `DROPPED` table's ([F11 §13.4]) and the tail's (an import's hashes among them,
   which no file holds and which cost one lookup each). Every record that carries a body entry of H lies below
   (m − 1)·E ([F13] I-D1 (a)).
2. **Checkpoint.** A delta checkpoint (P-80) folds the tail up to a boundary after extent m's head, so that
   `checkpoint_lsn` > (m − 1)·E and every extent below m can be retired ([F05 §2.5] EX-4, EX-5). Its fold enters H in
   `DROPPED`, and, like every fold (P-80), it seals no body of H.
3. **`blobs` files.** For every live `blobs` file — named by the segment set or its `FILES` registry ([F09 §14.4]), or
   by a pinned set ([F11 §4]) — that holds a blob of class `body` whose hash is in H, it writes by P-10 a replacement
   without those blobs under a new number ([F10 §5.4]). Where the store keeps dictionaries ([F10 §6]), it also replaces
   every live `dict.<D>`, since a trained dictionary may hold fragments of its sample, by one trained on a sample that
   holds no dropped body ([F17 §6.3]), and rewrites every `blobs` file coded with a replaced dictionary. The
   `blobs.<n>` of a reservation whose bulk `Commit` has not landed (P-84) is not replaced: its producer's `cs.<n>` names
   its offsets, and that `Commit` never lands carrying a body of H (P-34; [F13] I-D1 (a)), so step 7 releases the
   reservation's files instead.
4. **Graph segments.** It rewrites, under new numbers, every live graph segment — the segment set's base and deltas,
   each ref's promoted `seg.b<ref_id>.<K>`, and the segments of every pinned set of its scanned log, the yield deltas of
   its own holding excepted (step 7 rewrites those) — whose `BLOBTAB` holds a live `BlobRef` of a hash in H or names a
   `blobs` file that step 3 replaced, or whose full-text sections index a body of H.
   The rewrite writes dropped `BlobRef`s for H, points every other `BlobRef` at its replacement file, and leaves the
   dropped bodies' postings out of the full-text sections ([F09 §6.3], §12.1); every other section keeps its content.
5. **`cs.<n>` files.** It rewrites under a new number, through `tmp/` as P-11 writes one, every live `cs.<n>` whose
   `BLOBTAB` holds a live `BlobRef` of a hash in H or names a replaced `blobs` file ([F10 §8], [F09 §16.4]).
6. **`hist` files.** It reads every live `hist` file and rewrites each one that holds a `Commit` record with a body
   entry whose hash is in H, or a bulk `Commit` whose `cs.<n>` step 5 rewrote: without those entries, and with
   `cs_ref` (`file`, `len`, `b3`) naming the rewritten `cs.<n>` ([F10 §4.6], [F06 §8.1] DB-9). It writes the `hist` file
   of every active extent below m by the same rule, for step 7's retirements ([F10 §4.1]).
7. **Publish and move the pins.** Steps 4 and 5 rewrote the files that were live when they ran; structures written
   since then still name the files step 3 replaced, because step 3's replacements are named by no published record
   before this step. Two kinds can exist; this step handles them before its append, together with the reservations
   that step 3 leaves alone:
   - **Yield deltas.** The delta of a yield checkpoint of this holding (P-98) may name a replaced `blobs` file in its
     `BLOBTAB`, for a row whose body did not change (never a live `BlobRef` of H: its fold wrote dropped ones, P-80).
     The purge rewrites, by step 4's rule, every yield delta of this holding that the current set or a pin of the
     scanned log names, and runs no yield checkpoint between those rewrites and this step's append. A pin appended
     during the purge — a fork, a tag's `--pin`, a merge's pin in a yield checkpoint's group (P-81) — names a set
     published during the purge, whose files other than such a yield delta are those step 4 read and rewrote.
   - **Bulk commits at or after (m − 1)·E.** The `cs.<n>` of a bulk `Commit` appended after step 1 was streamed by
     another process against a published set, so its `BLOBTAB` may name a `blobs` file step 3 replaced. Its record stays
     in the log after this step, so its `cs_ref` cannot follow a rewrite (step 6), and the purge cannot replace the
     file. The purge reads the `BLOBTAB` of each such `cs.<n>` outside the writer byte (P-2 lets a holder of the writer
     byte read only `HEAD` and the log). Under the writer byte of this step's append it appends only when its scan finds
     no such bulk `Commit` whose `cs.<n>` it has not read; otherwise it releases the writer byte, reads the new ones and
     tries again. When one names a file step 3 replaced, the purge appends nothing and starts again at step 1 under the
     same holding: the new forced rotation puts that commit below the new (m − 1)·E, so the new pass's steps 5 and 6
     rewrite its `cs.<n>` and its extent's `hist` file, and the new H takes the drops appended meanwhile. The files the
     purge wrote stay its own and unnamed, and no `Checkpoint` of its holding claims them (P-78): it may reuse those
     whose inputs did not change, and the orphan sweep of a later holding removes the rest (P-79). A run makes at most
     three attempts at this step's append, each restart and each return to the writer byte counting as one; after the
     third, the run stops and the purge stays pending: `BodyDrop` reports `purged` false, `gc` goes on with its other
     work, and `backup` exits 7 `maintenance_busy` with nothing copied (P-87). A bulk `Commit` appended after this
     step's append is P-84 step 3's: its producer finds that this `Checkpoint` released a `blobs` file its `cs.<n>`
     names and starts again with a new reservation, so no such commit names a file step 8 deletes.
   - **Reservations whose bulk `Commit` has not landed** (P-84). The `blobs.<n>` of such a reservation, streamed by a
     producer whose view preceded a drop, may hold a body of H, and no rewrite applies to it (step 3). The purge reads,
     outside the writer byte, the `BLOBIDX` of the `blobs.<n>` of every reservation of its scanned log whose bulk
     `Commit` has not landed and whose `blobs.<n>` exists; this step's `Checkpoint` releases both files of each one that
     holds a blob of class `body` with a hash in H. A producer that still runs then starts again with a new reservation
     (P-84 step 3), as P-34 would make it do. A reservation whose `blobs.<n>` this step did not read is released by the
     next `gc` or purge if it holds a dropped body (P-84).

   Then one `Checkpoint` record ([F05 §9.9]) names all of it: a set change with the current `upto_lsn` (the rewritten
   base, deltas, yield delta and dictionary), the replacements as `added`, a `Promotion` entry for each rewritten
   `seg.b<ref_id>.<K>` that repeats the replaced entry's tip and base, the retirement of every active extent below m,
   and every replaced file, with the files of the reservations above, as `released`. Its group carries, for every pin
   of the scanned log whose set names a replaced file, the unpin and the pin that move it onto the replacements with its
   `set_lsn` unchanged ([F05 §9.8]; P-52, P-81).
   It releases only files that nothing kept live names: no `BlobRef` of a segment of its set, of a pinned set after the
   moves or of the `cs.<n>` of a `Commit` the log keeps, no `cs_ref` of a kept record and no `FILES` row of its set names
   a file it releases. With this record's publish no active extent holds a `BodyDrop` record of origin `command` below
   m, and the purge is no longer pending.
8. **Delete.** It runs a delta checkpoint that folds step 7's group (P-80), so that no record of the tail names a
   replaced file (P-77 condition 2), then the barrier (P-62), and deletes every file step 7 released and every extent
   it retired (P-14, P-77, the grace excepted: the grace only spares a reader a retry, and a reader that loses the race
   re-reads `HEAD`, P-59, P-89). The files of a released reservation whose `Reserve` record lies at or after
   (m − 1)·E wait until its extent is retired (P-77 condition 2), as a deletion still due ([F13] I-D1 (c)). The purge is
   then **complete** (`purged`, [API §8.7]). A deletion that fails, or a crash, leaves the file to the next maintenance
   run (P-77, P-79).

Steps 3 to 6 run in that order because a `BlobRef` names a `blobs` file's offsets and a `cs_ref` names a `cs.<n>`'s length
and digest; the yield checkpoints between them (P-98) release no file the purge reads (only earlier yield deltas of the
same holding, P-98) and claim none of the files it wrote (P-78), and they and every other structure written during
the purge name only published files, so step 7 re-points them rather than letting them name step 3's unpublished
replacements. A purge cut short before step 7's publish has named none of its files, which the orphan sweep removes
(P-79), and the extents stay active, so the purge stays pending and the next run starts again at step 1. A `BodyDrop`
record appended during a purge lies in extent m or later and leaves the purge pending after step 7, for the next run.
While a purge runs, readers already treat every dropped hash as dropped (P-102), so no result depends on how far it got
(spec sync 3; [AR §11] #33, OQ-A-7).

## 13. Namespace points

### 13.1 Renames, and the Windows `file mv` rule

**P-82 (every rename point).** Every rename that moirai issues on store or project files is a no-replace rename
(`rename_noreplace`), except where the point replaces a file (`rename_replace`: the store `config`, git's `.lock`
protocol in export) or exchanges directories (`swap_dirs`: `restore`). Every rename is followed by `durable-name` on
**every** parent it touched before any record that depends on it ([F15] FM-2.3, FM-2.4; [80] X-F5). On Windows every
rename passes `MOVEFILE_WRITE_THROUGH` in addition, never instead of, that directory flush — for `file mv` and for every
other rename `Vfs` and `ProjectFs` issue — and the flag stays until the post-release rig calibration (measurement 17,
deferred with the rig) shows it unnecessary ([PLAN §6.1] #3, [AR §4.10], [F15 §5.8]). The model credits the flag with
nothing (FM-2.5). `std::fs::rename` is never used.

**P-83 (a cross-volume `file mv` is refused).** `file mv` refuses a move across volumes — the two parents have different
volume keys — before it appends its intent, with exit 7 `cross_volume`; a rename that fails with `CrossDevice` although
planning saw one volume aborts the intent (`FsIntentAborted` reason 2) and changes nothing. moirai never copies a project
file and then deletes the original ([40 §3.4] step 1, A1P-01, FS-4).

### 13.2 Bulk commits

**P-84 (a bulk commit reserves its ids before it streams).** A bulk producer ([F17 §4.4] W1):
1. appends, by phases 2a and 2b, a durable **reservation group** — one `Reserve` record, kind 27 ([F05 §9.27]) — that
   allocates by P-31 the `#N`s, `aN`s, new symbols (its `SymDefs` block) and schema ids its file will use, and the file
   numbers of its `cs.<n>` and `blobs.<n>`, which the record claims (P-78); and waits for its acknowledgement;
2. streams `tmp/cs.<nonce>` with those final ids and makes it durable and named by P-11 (its bodies go to `blobs.<n>` by
   P-10);
3. appends the bulk `Commit` group by phases 2a and 2b, after checking under the writer byte that no `Checkpoint` of the
   scanned log — the window that P-34 re-validates, from the view its stream was computed against — released its
   reservation's files or a `blobs` file that its `cs.<n>`'s `BLOBTAB` names (else it starts again at step 1 with a new
   reservation); the producer keeps those file numbers from step 2, so the check reads only the log (P-2). So no bulk
   `Commit` names a `blobs` file that a `Checkpoint` appended before it released, whichever maintenance released it (a
   rollup, a tiered fold, blob GC, or a purge, P-101 step 7). The re-validation of P-34 treats the reserved uids and
   symbols as read keys and the changeset's nodes by node.

Ids of a reservation whose commit never lands are skipped, never reused ([F09 §16.4] "Ids", [F11 §9.1]). Its files stay
named by the `Reserve` record until `gc` releases them, in its `Checkpoint`'s `released` list, once the reservation's
`hlc` is older than `gc.cruft-delay` and no `Commit` names the `cs.<n>` ([F17 §11.2]); P-77 then deletes them (pass 1,
P1-3, S1-11, A1-12, closing open point 5).
A reservation whose bulk `Commit` has not landed and whose `blobs.<n>` holds a blob of class `body` with a dropped hash
([F05 §9.29], of either origin) is released without waiting for `gc.cruft-delay`: its producer's view preceded the
drop, so its `Commit` never lands carrying those bytes (P-34; [F13] I-D1 (a)). The next `gc` run releases its files once
it finds such a blob, and a purge releases them in its step 7 when it read them (P-101); until then [F13] I-D1 (b) and
(c) count them as a deletion still due, not as files the store holds, and [F05 §9.29] "Origin" does not count such bytes
as held (spec sync 3).

### 13.3 `restore`, `repair` and discovery during a swap

**P-85 (`restore`).**
1. `restore` builds the restored store completely in an empty directory b on the store's volume: the environment probe
   ([OS/env §5]); copies of the backup's files; a fresh `LOCK` (P-88 step 3); recovery of the copied log up to the
   backup's `committed_lsn`; the fold of every valid record into a new segment set with every extent retired; a new epoch
   by P-75; and `HEAD` last, both slots flushed, `readonly` clear, `boot_id` the restorer's.
2. On the live store a it takes the maintenance byte (try; `Busy` → exit 7 `maintenance_busy`) and the writer byte,
   reads the newest slot's `hlc_seq` and `hlc_commit` for the new epoch-start head (P-75), makes `retired` durable
   by P-13 (so every later holder of the writer byte stops, P-28), releases both bytes and closes every handle it holds
   into a (a Windows directory rename fails while a handle inside is open, [OS/fs §4.9.2]).
3. It runs `swap_dirs(a, b)` with the bounded share retry. On success the old store, now at b, keeps `retired`.
4. If the swap fails before its first rename, it runs `swap_recover` ([OS/fs §4.9.4]), clears `retired` on the store at a
   by P-13 under the maintenance and writer bytes, and exits 7 (`fs_busy`). If it fails later, the intent stays for
   `doctor`. A failure of kind `FlushFailed` (a flush embedded in the swap failed, [OS/fs §4.1]) exits 7 at once in
   either case: `restore` issues no further call, and the intent and `retired` stay for `doctor`.

`repair --rebuild-from-log` builds its rebuilt store beside the store and puts it in place by steps 2–4. Plain `repair` of
a store with no valid `HEAD` slot, or with a fatal slot (treated as absent, and the other slot is not trusted either;
P-61), holds the maintenance and flush bytes throughout (`Busy` on the maintenance byte:
exit 7 `maintenance_busy`) and rebuilds the slot state from the extent heads (P-97; [F04 §8.1], [F05 §4.5], §9.28):
1. it reads the extent head of every existing `log.<n>` file; the head with the greatest n that validates by itself
   (checksum, position, trailer with its `chain_in`) gives the epoch (its `RecHdr.epoch`) and `epoch_lsn`;
2. it scans by the chain rule from the head of the lowest-numbered extent of that epoch from which every later extent
   file exists, to the end of the valid log (a failed read: exit 7 by P-92). The head at the scan start, the head of a
   lower extent, validates by itself and carries step 1's epoch: it is the one scan start whose chain value comes from a
   record instead of `HEAD`, so its position check (P-54) and epoch check (P-55) keep a misplaced or foreign extent out
   of the rebuilt state;
3. it takes `epoch_lsn`, `init`, `project_oid_algo` and the counters from the newest head it scanned, and the `quiet` and
   `readonly` flags from the head of step 1, and folds every group after the newest scanned head into them
   ([F05 §10.2]). Step 1's head is at least as new as any scanned head; it differs only when a later extent's head
   survives beyond the end of the valid log. Repair thus resets both flags to that head's values: a flag change made
   after that head was written lives only in `HEAD` ([F04 §6]) and is lost, and the operator re-issues it. The rest of
   the state: the segment set and
   `checkpoint_lsn` come from the newest `Checkpoint` with a set change, which lies in the scanned range (EX-4, EX-5);
   `durable_lsn` = `committed_lsn` = the end of the valid log after the flush of step 4; `boot_id` is the repairer's;
   `config_gen` is 0; a table pointer with no record of its kind in the scanned range is 0, which a reader treats like
   one below `checkpoint_lsn` ([F04 §5.10]);
4. it flushes every extent it scanned, writes both slots under the writer byte and flushes `HEAD` after releasing it, as
   a durable publish (P-13) whose source state comes from the log instead of a slot.

Without the extent heads, a store whose first extent had been retired could not be repaired this way (pass 1, P1-8).

**P-86 (discovery during a swap never recovers the swap).** A discovering process that finds no store directory at `a`
while the swap intent `<a>.swap` exists ([F02 §3.2]) probes again after 10 ms, 20 ms and 40 ms on its monotonic clock;
if the intent is still there and `a` is still not a store, it exits 7 `swap_in_progress`. Discovery never runs
`swap_recover`; only `doctor` and `restore` do ([F02] open point 17, [F15] OP-11).

### 13.4 `backup`

**P-87 (`backup`).** `backup DIR`:
1. takes the maintenance byte (try; `Busy` → exit 7 `maintenance_busy`), so that no file is deleted or replaced during
   the copy; the copy is a long job whose steps are one file each, and the yield checkpoints of P-98 keep the tail
   bounded meanwhile without releasing a file it copies (pass 1, P1-43); when a purge is pending (P-101), it runs the
   purge first under the same holding, so a backup whose state holds a `BodyDrop` record holds none of its bodies' bytes
   (spec sync 3); when bulk commits keep that purge from publishing (P-101 step 7), it stays pending and `backup` exits
   7 `maintenance_busy` with nothing copied;
2. reads the newest valid slot S and copies by reads and writes only, never by clone or reflink: `HEAD`'s state S, `LOCK`,
   `config`, every log extent from `active_log` to the extent holding S.`committed_lsn`, and every sealed file S's view
   names (its segment set, its `FILES` registry, pinned sets, and the `cs` files named by records below
   S.`committed_lsn`). A reservation's `cs.<n>` and `blobs.<n>` ([F05 §9.27]) are copied only when its bulk `Commit`
   lies below S.`committed_lsn`; otherwise they belong to no commit of state S and are left out, whether the reservation
   is still in the log or its `FILES` row carries the `reserved` flag, which names no content ([F09 §14.4]; pass 1,
   round 2);
3. makes every copied file durable and the directory's names durable by P-22, and writes the backup's `HEAD` last: both
   slots S with `slot_seq` s and s + 1 and `flags.readonly` set, so a backup opened in place serves reads and refuses
   writes;
4. computes the manifest digest below and appends the durable `Backup` group `{dir, committed_lsn = S.committed_lsn,
   digest}` ([F05 §9.13]); it reports success only after P-46; then it releases the maintenance byte.

The **manifest** is the byte string `lp("moirai-backup-manifest-v1") ‖ u32-le(k) ‖ e_1 ‖ … ‖ e_k`, where the k entries,
one per file of the backup directory sorted bytewise by name ([F01 §6.6]), are `e_i = lp(name_i) ‖ u64-le(len_i) ‖
BLAKE3-256(content_i)` and `name_i` is the store-relative name with `/` separators. `digest` = BLAKE3-256(manifest)
([F01 §7.1]). Every operand except the names has a fixed width, so the framing is unambiguous ([F01 §7.3]).

### 13.5 Image export

The export steps and their order are P-23's for packs and P-100's for loose objects; the frontier walk and the `GitMap`
records are [F14]'s and [F05 §9.7]'s.

**P-100 (loose objects are durable and named before any ref names them).** An export run that writes loose objects
([F17 §9.1]) writes each object to a temporary file in the destination's `objects/` tree, makes it `durable+meta`,
renames it by `rename_noreplace` to `objects/<xx>/<rest>` — an existing name is success, because a loose object's name
is its content id — and runs `durable-name` on `objects/<xx>/` and, when the run created that directory, on `objects/`,
all before the ref's `.lock` step of P-23 and before the `GitMap` group. A crash can then leave an unreferenced object,
which git ignores, but never a ref that names an object whose name was lost (pass 1, P1-18).

### 13.6 The store `config`

The rewrite steps are [CFG §7.4]'s with P-20's classes: `config set` reports success only after the `durable-name` on
the store directory and on `tmp/`; the `config_gen` bump is an ordinary publish under the writer byte, not flushed,
because a lost bump needs an OS crash, after which every process reads `config` afresh ([F04 §5.6]).

### 13.7 `init`

**P-88 (what `init` creates, in this order).**
1. `create_root` creates the store directory and makes its name durable on the parent ([OS/fs §4.1]); `create_dir`
   creates `tmp/`, followed by `durable-name` on the store directory.
2. `probe_store` runs the full environment probe (P-94); a refusal removes what `init` created and exits 7.
3. `LOCK`: create-new, all 36,864 bytes written (`LockHdr`, then zeros, [F03 §2.3]), `durable+meta`, then `durable-name`
   on the store directory.
4. `config`: `tmp/config.<nonce>` with the initial text ([CFG §7.6]), `durable+meta`, `rename_noreplace` onto `config`,
   `durable-name` on `tmp/` and on the store directory.
5. log.1: prepared by P-8; then the epoch-start group, the extent head of log.1 with the counters of an empty store
   (`commit_seq` and `fence` 0; `next_id`, `next_anchor`, `next_file_no` 1; `next_ref_id` 0; both HLC maxima 0; [F05 §4.5],
   P-97), is written at lsn 0 and, right after it, one durable
   group that creates `main`: `RefUpdate` reason 1 with `ref_id` 0, `old` and `new` zero, then `RefTable` with `main`'s
   entry (kind `work`, empty tip, [F11]), with the symbol definitions they need and, when the store lies inside a git
   repository, the `ClientHead` binding of `main` to the main worktree ([40 §5.3], [F05 §9.3]); then `durable` on log.1.
6. `HEAD`: `tmp/head.<nonce>` holding both slots ([F04 §10]: `slot_seq` 1 and 2; `committed_lsn` = `durable_lsn` = the
   end of the `main` group; `refs_lsn` (and `heads_lsn` with a binding) the lsn of its record; `next_ref_id` = 1; `boot_id`
   `init`'s or zero), `durable+meta`, `rename_noreplace` onto `HEAD`, `durable-name` on `tmp/` and on the store directory.

`init` writes no segment: `n_segments` = 0, and the first checkpoint writes the first base or delta. `init` takes no lock
byte: until step 6 no process can discover the store ([F02 §3.2]).

**The window after step 6's rename.** Between the rename of `tmp/head.<nonce>` onto `HEAD` and the `durable-name` that
follows, a discovering process can open the store and acknowledge writes that a crash then loses together with `HEAD`'s
name; an `init` that died in that window leaves it open for good. The rename has two parents, so it is durable only once
`tmp/` and the store directory are both synced ([F15] FM-2.4). Therefore every process, before it first acknowledges a
durable effect through a store it opened (a durable group, P-46; a durable publish's success, P-13), runs `durable-name`
on `tmp/` and on the store directory, once per opening of the store, holding no role byte. On clean directories the two
calls return in well under a millisecond; measurements 1 and 2 include them.

**P-99 (the pointer file of `init --link` is durable before success).** `init --link` creates `./.moirai` with
create-new semantics, writes its bytes in one write, runs `durable+meta` on it and `durable-name` on its directory, and
only then reports success ([F02 §3.3] rule 6, [80 §2.3.1]). A crash before the `durable-name` may lose the file, which the
user sees as a missing link and repeats; it can never leave a reported success without a durable pointer (pass 1,
S1-41).

## 14. Clocks (decision (e))

**P-89 (each rule uses one clock).** The clock of every time-dependent rule is [OS/clock §6]'s, with this chapter's
reading for the deletion grace:

| Rule | Clock | Why a wall step cannot break it |
|---|---|---|
| lease deadlines, `session-ttl`, half-TTL renewal | the stamp `{wall, boot_hash, mono}` evaluated by [OS/clock §4.3]: the boot clock on the same boot; the wall clock only in Unknown-boot mode | the boot clock includes suspend and ignores wall steps ([F15] FM-7.3, OP-14) |
| boot change | the boot identity, never a clock | invariant under steps, suspend and hibernation (X-F2) |
| append-time HLCs (P-36) | [OS/clock §7]'s rule over the maxima `hlc_seq` and `hlc_commit` of the newest slot and the scanned log | a backward step advances the counter, never decreases the HLC |
| retention windows ([F17 §11]) and the deletion grace (P-77 condition 4) | elapsed = `hlc_ms(now)` − `hlc_ms(t0)`, `now` = max(`wall_ms` << 16, `hlc_seq`, `hlc_commit`) as P-36 derives the two maxima, `t0` the opening record's `append_hlc` or `hlc` ([F17 §1.6], [API §6.2] CK-6) | a backward step pauses a window; a forward step shortens it, which is acceptable because no window is a safety condition (the deletion grace included: P-77). The maxima never restart across an epoch re-roll (P-75), so a window never opens early after `restore` or `repair` on a machine whose clock is behind (pass 1, P1-44) |
| lock waits, share-violation retries, the swap-discovery probes (P-86), settle slices | the waiting process's monotonic clock, in process | a wait needs no cross-process comparison |

## 15. Errors

**P-90 (disk full aborts without acknowledgement).** A `DiskFull` from any `write_at`, create, rename or unlink ([F15]
FM-5) ends the command without an acknowledgement: the process releases its lock bytes and exits 7 `disk_full`; from a
flush it is P-91's `fail_stop`. What it leaves is one of: bytes beyond the end of the valid log (invalid or a pending
group, adopted or lost by §5.3 like a dead writer's group), a numbered file that no record names (P-79), a `tmp/` entry
(P-79), or a short log extent (P-72 step 2). Nothing below `durable_lsn` is ever written ([60 §2.5] decision (f)).

**P-91 (non-lazy class errors stop the process).** Any error from `durable`, `durable+meta`, `durable-name` or
`sync_group` — `Io`, `DiskFull`, `Unsupported` or another — goes to `fail_stop` ([OS/fs §4.4.5]): exit 7
`durability_failure`, no acknowledgement, no retry on the same handle. A class that the location cannot provide is never
replaced by a weaker call; the environment guard refuses such a location (P-94) ([80 §2.3.1] "No downgrade", [F15 §4.3]).
Two calls made before any state they protect exists are exceptions: `probe_store`, where a durability failure is a
refusal of the location ([OS/env §5]); and the plan step of `file mv`, `file rm` and `file revert`, whose `sync_dir` on
the project parents runs before the `FsIntent` group and turns `Unsupported` or `AccessDenied` into the refusal
`no_dir_flush` with nothing changed ([API §12.4] step 1, [OS/project §6.2]; pass 1, P1-16). Intent recovery's re-barrier
on such a volume is P-71's case.

**P-92 (read errors are judged by position and by reader or writer).** A failed read ([F15] FM-12) of the log below
`durable_lsn` is corruption (exit 7 `store_corrupt`, `moirai repair`). At or above `durable_lsn`:
- a **reader** (P-57, P-58) ends its visible log at the group that contains it;
- a **writer's scan** — an appender's (P-29), a flush holder's (P-42), boot-change recovery's (P-66), a `repair`'s
  (P-85) — appends, re-writes and flushes nothing, releases its bytes and exits 7 `store_io_fault` naming the extent and
  offset ([F19 §10.2]). `durable_lsn` is only a lower bound of the durable end after an OS crash ([72 B1]), so the
  unreadable bytes may be acknowledged groups, and treating them as the end of the log would let the next append overwrite
  them (X5: refuse rather than lose; [F05 §5.3]; pass 1, S1-25, closing open point 13). A transient error clears on the
  next try; a persistent one keeps the store read-only in effect until `repair`.

A failed read of a `HEAD` slot makes that slot absent for P-61. A failed read of a sealed file's header, or a mapping
fault, is P-59's or P-93's case. A failed identity read is a mismatch (P-46).

## 16. Mapping, the environment guard and the leader

**P-93 (the mapping policy).** A process maps a file only when it is sealed and named: completely written,
`durable+meta` and `durable-name` returned, sealed read-only on disk, and named by a durable record of the valid log
([80 §2.5] rule 1, [OS/map §2]). It maps whole files read-only from offset 0 and never maps a log extent or `HEAD`. Before
mapping it checks the header's `total_len` against the file's size, with P-59's one re-read of `HEAD` ([OS/map §4]).
Mapped bytes are read only through typed, bounds-checked access ([80 §2.5] rule 5). A fault inside a mapping ends the
process with exit 7 `store_io_fault` ([OS/map §8]), which the protocol treats as a crash at that point: readers hold no
lock, and a writer's pending group is adopted or lost by §5.3.

**P-94 (the environment guard).** `init` and `restore` run the full probe on their target before creating `LOCK` or
`HEAD` (`probe_store`, [OS/env §5]); every process runs `classify(Open)` and the OS-version check at every open of a
store ([OS/env §4], [OS/env §6]). A refused location is exit 7 `refused_location` and nothing is created or written; no
configuration key admits a location; only crash-gated file systems are on the allow-list ([80 §2.6], [OS/env §3]).

**P-95 (the leader, if built, adds no durability path).** If the M0 measurements build the optional leader ([60 §3.1]),
its own commits and every write forwarded to it go through phases 2a and 2b exactly as a direct client's, and it
acknowledges a forwarded write only by P-46 ([80 §2.4.2] "Leader", [AR §6.1]).

## 17. The seeded-bug catalogue

### 17.1 Source bug labels

- **T1–T14**, the bugs of [60 §3.1] item 4, in its order: T1 a checkpoint/reset race of the SQLite WAL-reset class
  [08 §3.1]; T2 a torn ref move (N3); T3 a reader past `committed_lsn` (F-A1); T4 idempotency evaluated before republish
  (F-B7); T5 adoption by re-flush after a failed flush ([61 B2], scenario 1); T6 deletion of a file before a durable `HEAD`
  barrier ([61 B2], scenario 2); T7 trusting a lazily published `committed_lsn` ([72 B1]); T8 a reader serving a pre-crash
  view ([72 B1]); T9 a single-slot barrier ([72 M2]); T10 a commit adopted without its group's markers ([72 M1]); T11 a
  skipped non-commit record ([72 M1]); T12 a rename without a namespace barrier ([72 M8]); T13 a same-epoch record at the
  wrong position ([72 M1]); T14 an intent roll-forward without the re-barrier ([40 §3.4], A1P-02).
- **G1–G13**, the group-commit bugs of [80 §2.4.4], in its order.
- **LG**, the lost-group scenario, and **DF**, decision (f)'s acknowledgement after `ERROR_DISK_FULL` ([PLAN §3.2] WP-40).

### 17.2 Detection and vehicles

- **Detected by** names the assertion that reports the bug: **ack** — an acknowledged durable effect absent from a
  recovered state ([F13] `crash::ig1_ack_implies_durable`); **fresh** — post-crash read freshness or a read of an
  uncovered durable group (`crash::ig2_read_freshness`); **chain** — `crash::ig3_chain_prefix`; **trace** — a predicate
  over the simulator's lock, flush and publish events (`crash::trace::ig4_flush_discipline`,
  `crash::trace::ig6_publish_monotone`, and the protocol-violation checks of [F15 §3.13]); **model** — the reference
  model's comparison of state, markers, leases, ids or results; on the toy vehicle, which has no reference model, the
  toy's own `doctor --verify` (below); **ns** — the namespace check of the `Vfs` or `ProjectFs`
  simulator (a durable record whose file or name a crash lost); **avail** — a store that refuses to open or loops after
  a state the protocol must accept, or an operation that ends `outcome_unknown` in a run without a failed flush or read
  (P-47); **fsck** — `doctor --fsck`'s check of [F13] I-D1 on the store at the end of a run and in every recovered
  state (a dropped body's bytes left in a store file, or carried after its drop).
- **Vehicle**: **toy** — WP-40's toy log over the in-memory `Vfs` at M0 (group commit, two-slot `HEAD`, epoch, extents,
  checkpoint, barrier and deletion, ref moves, idempotency, minimal markers and leases, recovery, plain `repair` from the
  extent heads (P-85), and a namespace model of intents over `Vfs` renames); a milestone gate — the mechanism is built
  there and its seeded bug is carried by that gate ([60 §3.13], [60 §2.6] "How M1 certifies"): **M1** the storage
  driver's GT1/GT3, **M2** the re-certification of GT1/GT3 with bodies and `BodyDrop` in the streams and GT2 ([60 §3.3]),
  **M5** GT8, **M6** the
  `FsIntent` crash enumeration of [40 §8.3.5] and GT17. **none (masked)** — no reachable state of the vehicle can show the
  bug, because another rule named in the row dominates it; the row names how its detector is still tested.
- **Where the detectors live for the toy vehicle.** PLAN §2.2 forbids `moirai-toylog` → `moirai-model`, and PLAN §3.1 S4
  makes the seeded-bug author (R-TOY) someone other than the enumerator's author (R-HARN-S), so that the harness cannot
  be tuned to its own bugs. At M0 the generic families are therefore checked in `moirai-vfs-sim`, the enumerator's
  crate, as part of that harness: ack, fresh, chain and avail by the enumerator's verdicts (avail over the outcome the
  subject reports and the faults the run injected); trace by generic predicates over the simulator's lock, flush and
  namespace events and over the decoded `HEAD` slot writes ([F04 §3]; I-G4, I-G6); ns by the simulator's namespace
  check, which judges any subject's durable records against the simulator's own namespace model, so one check serves the
  toy at M0 and the storage driver from M1. The `model` family has no reference model on the toy, since the comparison
  with `moirai-model` is what PLAN §2.2 keeps out of the toy harness: it is checked by the toy's own `doctor --verify`,
  which re-derives from the log's records the invariants that the comparison would find broken in the rows below (state,
  markers, leases, ids and results). That check and the other checks that need the toy's own state (read visibility
  against its replayed view) are the toy's, written in the toy and reviewed by R-HARN-S as a WP-40 acceptance step. S4
  names only the enumerator's author, so these checks do not break it; the review is what keeps them from being fitted
  to the toy's bugs. From M1 the `crash` functions of `moirai-model` ([F13 §1.4]) and the comparison with that reference
  model are the functions of record ([F13] OP-13-02; OWNER O-2 confirms the authorship).

### 17.3 Catalogue

| P | Rule | Primary seeded bug B-n (violates P-n) | Source bugs | Detected by | Vehicle |
|---|---|---|---|---|---|
| P-1 | lock order and waits | the appender waits for the flush byte while holding the writer byte | G4 | trace | toy |
| P-2 | writer byte innermost | the flush holder flushes the log while holding the writer byte | — | trace | toy |
| P-3 | ownership in user space | a second in-process client is granted the writer byte by the kernel path while the first holds it; both append at one lsn | — | ack | toy |
| P-4 | `HEAD` only by publish | a `config_gen` bump written without the writer byte, over a concurrent publish, so `durable_lsn` decreases | — | trace (ig6) | toy |
| P-5 | class tags | a `Lease` claim appended with `lazy` = 1 and acknowledged at its publish | — | ack | toy |
| P-6 | durable flush covers every extent | a flushed range that spans a rotation flushes only the extent that holds E | — | ack | toy |
| P-7 | lazy never advances `durable_lsn` | a lazy publish sets `durable_lsn` to its end | — | trace (ig6); avail (false corruption below `durable_lsn`) | toy |
| P-8 | extent preparation | the first group is appended into a new extent before `durable+meta` on it | — | ack | toy |
| P-9 | rotation pad | a group is written across an extent boundary | — | avail (the extent grows past E, which [F05 §2.2] makes corrupt, so every scan refuses and nothing is acknowledged) | toy |
| P-10 | maintenance sealed files | a `Checkpoint` is appended before its segment's `durable+meta` | — | ack; avail | toy |
| P-11 | bulk `cs` via `tmp/` | after the rename from `tmp/`, only the store directory is synced | — | ns | M1 |
| P-12 | publish writes the other slot | a publish overwrites the slot that holds the newest valid state | — | avail (after a torn publish the other slot names files a later checkpoint deleted; the log is intact, so nothing acknowledged is lost) | toy |
| P-13 | durable publish writes both slots | a durable publish of a flag change writes one slot and flushes; a crash that tears the newer slot while the older reverts brings back the replaced flag (the barrier form is masked by P-62: the covering publish of the `Checkpoint` already wrote the new set) | T9 | ack | toy |
| P-14 | deletion after the barrier | a released file is deleted before the barrier's `HEAD` flush | T6 | avail; ack | toy |
| P-15 | boot recovery flushes the log first | boot-change recovery publishes before flushing the re-written range | — | avail (a later process finds an invalid group below `durable_lsn` and refuses, P-58) | toy |
| P-16 | intent before rename | the rename is issued before the `FsIntent` group's identity check | — | ns | toy |
| P-17 | `file mv` barrier on both parents | the move's commit is appended after the rename without `sync_dir` of both parents | T12 | ns | toy |
| P-18 | `file rm` barrier | the removal's commit is appended before `sync_dir` of the parent | — | ns | toy |
| P-19 | roll-forward re-barrier | intent recovery commits a roll-forward without `sync_dir` of both parents | T14 | ns | toy |
| P-20 | `config` rewrite barrier | only the store directory is synced after the replace-rename; the acknowledged change is lost at a crash | — | ns | M1 |
| P-21 | `restore` completes before the swap | the swap runs before the restored store's `HEAD` is durable | — | avail | M1 |
| P-22 | `backup` durable before its record | the `Backup` record is acknowledged before the copied files are durable | — | ns | M1 |
| P-23 | export order, pack path | the `GitMap` group is appended before the pack's `durable+meta` and the rename's `durable-name` | — | ns | M5 |
| P-24 | `init` creates `HEAD` last | `HEAD` is renamed into place before log.1 is durable | — | avail | toy |
| P-25 | phase 1 lock-free | phase-1 work (the candidate's computation, or a simulated file-system wait) runs while the writer byte is held | — | trace | toy |
| P-26 | settles never sleep on hook and server paths | a server settle waits 50 ms for quiescence inside a request | — | trace | M6 |
| P-27 | bounded writer wait | on a writer-wait timeout the process appends without the byte | — | ack | toy |
| P-28 | slot checks under the writer byte | an appender ignores `retired` and appends to the old store after `restore` | — | model | M1 |
| P-29 | scan to the end of the valid log | an appender appends at the published `committed_lsn` without scanning beyond it, overwriting a durable group whose publish a crash lost | — | ack | toy |
| P-30 | scratch layer | a pending group is replayed into the read overlay | G10 | fresh | toy |
| P-31 | allocators from the scanned log | `#N` is allocated from `HEAD.next_id` alone, ignoring a pending group | — | model (I1) | toy |
| P-32 | idempotency after the scan | the key is evaluated before the scan | T4 | model (duplicate commit) | toy |
| P-33 | pending hit waits for identity | an idempotent replay of a pending group is acknowledged before its durability | G6 | ack | toy |
| P-34 | re-validation by key; a bulk commit by node | "the candidate stands" when `committed_lsn` = L0 although pending groups exist; two exclusive claims succeed | — | model | toy |
| P-35 | final encoding checks | W3 is not re-checked; a group longer than E is appended and acknowledged | — | avail (the extent grows past E and every scan refuses, [F05 §2.2]) | toy |
| P-36 | one HLC sequence over the semantic records | a `Checkpoint` advances the sequence: a class-I checkpoint appended between two commits in one millisecond raises the next commit's `hlc`, so its commit id differs from the model's (pass 1, P1-5) | — | model (commit id, I43′) | toy |
| P-37 | append at E_v, chained | the trailer is seeded with the chain value at `committed_lsn` instead of at E_v | — | avail (the chain rule rejects the group, so it is never acknowledged and the operation ends `outcome_unknown` without a failed flush) | toy |
| P-38 | lazy publish only without pending durable | a lazy publish moves `committed_lsn` past a pending durable group | G2 | fresh | toy |
| P-39 | lazy behind durable runs phase 2b | a lazy group behind a dead writer's pending durable group is left unpublished | G13 | fresh | toy |
| P-40 | covered test | a durable group is acknowledged after its append and identity check, without a covering flush (covered by the committed position of its own append); the form "covered when `committed_lsn` ≥ E_g" cannot occur, because the published `committed_lsn` never passes a pending durable group (P-38, P-49) | G1 | ack | toy |
| P-41 | bounded flush wait | a flush-byte timeout is acknowledged as success | — | ack | toy |
| P-42 | re-write before every flush | the flush holder flushes without re-writing a dead predecessor's range after a failed flush | G3, T5 | avail (the unrewritten sectors stay poisoned, so the holder's publish scans a shorter log than it flushed and refuses by P-48) | toy |
| P-43 | scan and re-write under the writer byte | the pending range is scanned and re-written outside the writer byte | G8 | trace (ig4 states the rule itself; the chain or acknowledgement damage needs an append into the scanned range, which appenders never make) | toy |
| P-44 | flush error policy | a failed flush is retried on the same handle and its success acknowledged | — | ack | toy |
| P-45 | `durable_lsn` never decreases | a publish writes a smaller `durable_lsn` | G5 | trace (ig6): defence in depth, masked by the flush byte: every publish that raises `durable_lsn` holds it (P-41, P-45, P-66, P-85), and its E ends a scan that starts at the `durable_lsn` read under it, so max(`durable_lsn`, E) = E in every reachable state, also after a failed `HEAD` flush. The ig6 predicate is asserted at every publish and is itself unit-tested on a synthetic publish sequence that lowers `durable_lsn` | none (masked) |
| P-46 | identity check | acknowledgement by position, without reading the trailer | G7 | ack | toy |
| P-47 | lost group re-runs | a writer whose group vanished re-appends its old bytes at the old position | LG | chain; model | toy |
| P-48 | read-modify-write publish | a publish from a stale `HEAD` snapshot | G11 | trace (ig6) | toy |
| P-49 | `committed_lsn` rule | `committed_lsn` is kept above the valid end after a lost lazy tail, so a later unflushed group becomes visible | — | fresh | toy |
| P-50 | the fold | a covered `Checkpoint` without its segment set published | G12 | trace (ig6); avail | toy |
| P-51 | phase 3 after release | the delta checkpoint runs while the writer byte is still held | — | trace | toy |
| P-52 | group composition | a commit and its `Marker` records are appended as two groups | T10 | model (markers) | toy |
| P-53 | group validity | a group is accepted whose predecessor differs (no chain check) | G9 | model (only the stale tail of a lost group surviving a refill passes, which revives a value no acknowledged operation wrote, I14′; an acknowledged group never loses its predecessor) | toy |
| P-54 | lsn = position | the position check is skipped; `repair` without a valid slot (P-85 step 2) takes a copy of another extent, put in place by setup as an external rewrite of the lowest extent's file ([F15] FM-10.1), as the lowest extent of the epoch and rebuilds the slots from it. Every other scan start is seeded from `HEAD`, where the chain rule (P-53) rejects a misplaced group first | T13 | avail (every later process refuses the rebuilt store, P-61 or P-58) | toy |
| P-55 | epoch | a record of another epoch is accepted; `repair` without a valid slot (P-85 step 2) takes an extent of another epoch, put in place by setup as an external rewrite ([F15] FM-10.1), as the lowest extent of its epoch. Every other scan start is seeded from `HEAD`, where the chain rule rejects such a group first | — | avail (readers seed the chain at `epoch_lsn` with the slot's epoch and refuse the rebuilt store, P-58) | toy |
| P-56 | replay-bound re-check | an overlay is kept after a lost tail was refilled | — | model | toy |
| P-57 | readers stop at `committed_lsn` | a reader replays past `committed_lsn` | T3 | fresh | toy |
| P-58 | invalid group by position | a reader treats an invalid group below `durable_lsn` as the end of its view | — | fresh; ack | toy |
| P-59 | missing named file | a reader falls back to an older segment set when a named file is missing | — | model | M1 (P-68; masked in the toy: a fallback set whose file and log still exist replays to the same state, and after every barrier both slots name one set, P-13, P-62) |
| P-60 | boot check before the first read | a reader serves a pre-crash view | T8 | fresh | toy |
| P-61 | slot selection | a slot that passes its checksum but fails validity is skipped for the other slot; only a defective writer produces such a slot, so setup injects one as an external rewrite of the slot with a checksummed slot that fails [F04 §7] check 5 ([F15] FM-10.1) | — | model | toy |
| P-62 | barrier after the own `Checkpoint` | the barrier runs before the `Checkpoint` is published | G12 | avail; ack | toy |
| P-63 | flags by durable publish | `quiet on` is reported before the `HEAD` flush | — | ack (acknowledged-effect list) | toy |
| P-64 | recovery scan start | recovery scans from `committed_lsn` | T7 | avail (recovery publishes a `durable_lsn` over bytes the log does not hold, or refuses that fatal slot, and every later process refuses) | toy |
| P-65 | every record kind applied | recovery skips a non-commit durable record | T11 | model; ack | toy |
| P-66 | boot-change recovery | recovery publishes the new `boot_id` without re-writing `(durable_lsn, E_v]` | — | avail (the flushed range keeps poisoned sectors, so the recovery's publish scans another end than it flushed and refuses by P-48) | toy |
| P-67 | Unknown-boot mode | an Unknown-boot publisher writes a `boot_id` other than its slot's | — | trace (ig6) | toy |
| P-68 | a slot names a missing file | recovery continues on a partial segment set | — | model; avail | M1 |
| P-69 | ref move implied by the commit | the ref move is written as a separate record in a later group | T2 | model | toy |
| P-70 | parking | a commit whose ref CAS failed moves its ref anyway, where the rule appends a `RefUpdate` reason 5 `park` of `orphans/<R>` ([F05 §9.2]) | — | model (I27′) | toy |
| P-71 | intent recovery | recovery treats an `Unknown` anchor as Dead and rolls a live move back | — | ns; model | toy |
| P-72 | rotation under the flush byte | an appender that holds only the writer byte appends the first group into an extent that another process is preparing | — | trace (the preparation's flushes and namespace calls run under the writer byte, which P-2's predicate reports) | toy |
| P-73 | retirement bounds | extent n is retired while `checkpoint_lsn` ≤ n·E | — | avail | toy |
| P-74 | nothing reused | a retired extent's file is zero-filled and reused under its old number while a reader still replays from it | T1 | avail (the reader meets an invalid group below `durable_lsn` and refuses, P-58) | toy |
| P-75 | epoch re-roll | the new epoch is installed without retiring every older extent | — | chain; avail | M1 |
| P-76 | one maintenance holder | a checkpoint runs without the maintenance byte beside another; the later publish drops the earlier delta | — | model | toy |
| P-77 | deletion conditions | a file still referenced by a pin is deleted | — | avail | toy |
| P-78 | file-number claims | the sweeper deletes a live bulk writer's unnamed `cs.<n>` without claiming its number; the commit then names a deleted file | — | avail | M1 |
| P-79 | orphan sweep | the sweeper deletes a file named only by a pending group | — | avail; ack | M1 (the pending namer that matters is a bulk writer's `cs.<n>`, P-11, P-78; the toy has no orphan sweep, and its Checkpoints and fork Pins are covered by P-34 and P-62) |
| P-80 | what a checkpoint folds | a checkpoint drops a body that a later tail record references without carrying | — | model; avail | M1 |
| P-81 | pins with what they protect | a fork's `Pin` is written in a later group than its `RefUpdate`; after a crash GC deletes the fork base | — | ack; model (the Pin is one of the fork's acknowledged effects, lost with the later group; the deletion needs a later checkpoint) | toy |
| P-82 | every rename point | a Windows rename passes `MOVEFILE_WRITE_THROUGH` and skips the directory flush | — | ns | toy |
| P-83 | no cross-volume `file mv` | a cross-volume move copies, then deletes the source | — | ns | toy |
| P-84 | bulk reservation | a bulk file is streamed with ids allocated in phase 1 from `HEAD`, with no `Reserve` record (kind 27, [F05 §9.27]) before it | — | model (I1) | M1 |
| P-85 | `restore` sequence | a failed swap leaves `retired` set on the live store | — | avail | M1 |
| P-86 | discovery never recovers a swap | discovery runs `swap_recover` on a running swap | — | avail | M1 |
| P-87 | `backup` sequence | `backup` copies without the maintenance byte; a file deleted mid-copy leaves an acknowledged incomplete backup | — | avail (restore of the backup) | M1 |
| P-88 | `init` contents | `init`'s `HEAD` keeps `next_ref_id` = 0 after creating `main` with ref id 0; the next branch reuses id 0 | — | model | toy |
| P-89 | clocks | a lease deadline is evaluated on the wall clock on a known boot; a ±1 h step expires a live lease | — | model (lease assertion) | toy |
| P-90 | disk full aborts | the command is acknowledged after `ERROR_DISK_FULL` | DF | ack | toy |
| P-91 | non-lazy errors stop | an `Unsupported` from a flush is treated as success (a downgrade) | — | ack | toy |
| P-92 | read errors by position and by reader or writer | an appender's scan treats a read error above `durable_lsn` as the end of the log and appends over an acknowledged group that an OS crash left above a stale `durable_lsn` (pass 1, S1-25) | — | ack | toy |
| P-93 | mapping policy | a sealed file is mapped without the `total_len` check; zeros after an external truncation are read as data | — | model | M1 |
| P-94 | environment guard | a location whose probe failed the no-replace rename is admitted | — | ns | M1 |
| P-95 | leader adds no durability path | the leader acknowledges a forwarded write before its identity check | — | ack | M1 (only if built) |
| P-96 | spare extent | a rotation appends into a spare without re-issuing `durable+meta` and `durable-name`; a crash loses the spare's size or name under an acknowledged group (pass 1, P1-7) | — | ack; ns | toy |
| P-97 | extent heads | a rotation begins a new extent without its extent head; after one retirement a `repair` without a valid slot (P-85 step 2) cannot validate the active log (pass 1, P1-8). The toy reaches it: it rotates, retires (P-73) and repairs from the extent heads, and [F05 §5.4] makes an extent whose first group is not its head corrupt at every scan | — | avail (every later scan that reaches the extent refuses the store, so the group after the missing head is never acknowledged, and a slot-less `repair` refuses too) | toy |
| P-98 | long holdings yield | a rollup holds the maintenance byte for its whole run without yield checkpoints while writers append; every process's overlay passes P09 by more than one step's tail (pass 1, P1-9) | — | trace (overlay allocator count against the bound) | M1 |
| P-99 | `init --link` pointer file | success is reported before `durable-name` on the pointer file's directory; a crash then loses an acknowledged link (pass 1, S1-41) | — | ns | M1 |
| P-100 | export, loose-object path | the ref is updated before a loose object's name is durable; a crash leaves the ref naming a missing object (pass 1, P1-18) | — | ns | M5 |
| P-101 | the body purge | step 3 rewrites the `blobs` files that `FILES` names and skips those of pinned sets; after the purge completes, a pinned fork base's `blobs` file still holds the dropped body (spec sync 3) | — | fsck | M2 |
| P-102 | readers look up the dropped set first | a reader resolves a body hash through `BLOBTAB` before the dropped set; between a drop's publish and the end of its purge, `show` prints the dropped body (spec sync 3) | — | model (the model renders `[body dropped: <reason>]`, [F06 §8.1] DB-6) | M2 |

Every source bug of §17.1 appears in exactly one row, except G12, whose two halves are P-50's and P-62's bugs.

### 17.4 Seeded bugs of protocol rules owned by other chapters

Some protocol rules that the lock and liveness layers rely on are stated outside this chapter, and so is the rule of
[F02 §3.6] for a store left `retired` after `restore` (P-85, P-86). Each gets one seeded bug here, so that E4 reaches
them and GT18 (lease liveness) has bugs to catch (pass 1, A1-27; L-9 pass 1, round 3). The rule text stays with its
owner.

| Id | Rule | Primary seeded bug | Detected by | Vehicle |
|---|---|---|---|---|
| L-1 | [F03 §8.4] SR-2: the slot record is written before the holder serves anything | a server anchors a lease to its slot before writing its `SlotRec`; a checker in between reads no live record, finds no match and reclaims the live holder's lease | model (lease liveness) | GT18 |
| L-2 | [F03 §8.4] SR-5: zero the record, then release the byte | the holder releases the slot byte first and zeroes the record after; a process that took the slot in between loses its fresh record, and its leases look dead | model (lease liveness) | GT18 |
| L-3 | [F03 §8.6]: the re-read after a `Held` probe, with the same `nonce` and session hash | a checker trusts its first read of the slot table without the re-read; a slot freed and taken by another session between the read and the probe keeps a dead holder's lease alive | model (lease liveness) | GT18 |
| L-4 | [F03 §10.3] step 2: an anchor stores the matched record's primary hash | a CLI that matched through the alias hash stores the alias; after the server's next `/clear` the anchor matches nothing and the live lease is reclaimed | model (lease liveness) | GT18 |
| L-5 | [OS/proc §6.2]: `Unknown` never ends a lease or recovers an intent | a probe that answers `Unknown` (a sandbox denial) is read as Dead; a live holder's lease is reclaimed, or its `file mv` intent rolled back | model; ns | GT18; M6 (`FsIntent` crash enumeration) |
| L-6 | [OS/lock §5.4] I-L4 with I-L2: a kernel grant goes to exactly one in-process waiter | a grant obtained by one kernel wait is handed to two waiting clients of one process; both append at one lsn | trace (ig4: the second client scans and re-writes without a grant of the writer byte; the first client's identity check fails and it re-runs, P-46, P-47, so no acknowledged group is overwritten) | toy (the in-process two-client case) |
| L-7 | [OS/lock §5.4] I-L6: a grant that races the deadline is returned or released | a grant that arrives after the waiter's deadline is neither returned nor released; the byte stays held by a client that returned `Busy`, and every later writer times out | avail | toy |
| L-8 | [F03 §3.1] rule 2: quiet mode is on while any quiet byte is `Held` or `Unknown` | the maintenance decider probes only the first quiet byte; a checkpoint runs while another requester holds a later quiet byte (pass 1, P1-10) | trace (a checkpoint during quiet mode) | toy |
| L-9 | [F02 §3.6]: a store that discovery yields again with `HEAD.retired` set and no swap intent is probed with P-86's delays, then refused with exit 7 `store_retired` ([F19 §10.2]) | a process that finds `retired` set re-runs discovery without a bound; after a `restore` that ended without clearing the flag, every command on the store loops instead of exiting 7 (pass 1, round 3; open point 10) | avail | M1 |

## Coverage

| Item | Part covered here | Section |
|---|---|---|
| [60 §2.5] issue-2 row "Protocol decisions": (a)–(f) and (j)–(m) | complete as protocol steps, each decision with its implementing rules and seeded bugs; the fault model is [F15]'s | §4, §17 |
| [60 §2.5] the paragraph after the audit-rows table: (a)–(c) restated, (g), (h), (i) | complete | §4, §5, §9, §10, §13 |
| [60 §2.5] audit row "`HEAD`": the boot-change recovery rule and the two-slot barrier; every publish a read-modify-write of the newest slot | the protocol steps; the fields and the slot side are [F04]'s | §5.4, §9, P-66 |
| [60 §2.5] audit row "Log": per-group chain validity | its use by the scan and the append; the format is [F05 §4]'s | §5.2, §7 |
| [60 §2.5] audit row "`Vfs`/`ProjectFs`": the four durability classes with `sync_dir` and `sync_group` | which class each protocol point uses; the classes and calls are [F15 §4]'s | §3 |
| [60 §2.5] "Cross-platform" row, X-F3 (group commit) | complete as protocol: phases 2a and 2b, the publish, maintenance's barrier after its own `Checkpoint`, boot-change recovery in the lock order, I-G1–I-G6 with the bugs they exclude; the trailer bytes are [F05]'s | §5, §6, §7, P-66 |
| [60 §2.5] "Cross-platform" row, X-F5 | the protocol-point classes, every rename point with `durable-name` on both parents, the `HEAD` barrier outside the writer byte, `lazy` lost after a failed flush, no downgrade; the fault-model items are [F15]'s and the record tag [F05]'s | §3, P-44, P-82, P-91 |
| [60 §2.5] "Cross-platform" row, X-F6 | the protocol side of the mapping policy and the environment guard; the mechanisms are [OS/map]'s and [OS/env]'s | §16 |
| [60 §2.5] "Cross-platform" row, X-F4 | the acquisition sequences and their order; the contract is [OS/lock]'s | §2 |
| [60 §2.5] "Cross-platform" row, X-F2 | the boot check, boot-change recovery and Unknown-boot mode as protocol steps; the identity and the liveness procedure are [OS/proc]'s | P-28, P-60, P-66, P-67, P-71 |
| [60 §3.1] item 4: the seeded protocol bugs, both [61 B2] scenarios among them, and the thirteen group-commit bugs of [80 §2.4.4] | one seeded bug per rule, with every source bug mapped | §17 |
| [PLAN §3.3] gap "the `MOVEFILE_WRITE_THROUGH` rule without measurement 17" (WP-16) | closed: every Windows rename, in addition to the directory flush, until the post-release calibration | P-17, P-82 |
| [40] R-6 | the allocation of `aN` from the scanned log, which makes `next_anchor` recoverable; the field is [F04]'s | P-31 |
| [40] R-7 | the `FsIntent` protocol points and intent recovery with the re-barrier; the records are [F05]'s | P-16–P-19, P-71 |
| [50] F14 | the append check: `append_hlc` assigned at append by the HLC rule, strictly increasing | P-36 |
| [50] F17 | none beyond P-31 (`#N` allocation); `ALLOC` is [F11]'s | — |
| [AR §11] #33 and OQ-A-7 (bodies droppable by hash without changing commit ids) | the protocol side: `BodyDrop` in the HLC sequence and in its own group, the re-validation against a `BodyDrop` in the window, the purge's steps with its forced rotation, pending test (records of origin `command` only) and deletions, its handling of yield deltas and of bulk commits appended during it, no automatic start, folds that seal no dropped body, the readers' dropped-set lookup. What a drop is, is [F06 §8.1]'s; the record [F05 §9.29]'s; the invariant [F13] I-D1's | P-1, P-34, P-36, P-52, P-72, P-73, P-76, P-80, P-87, P-98, P-101, P-102 |
| [AR §11] #46 and OQ-A-10 (the harvest cursor, reserved before the freeze) | the protocol side of the reserved `Harvest` record: durable, in the HLC sequence, in an `Apply` group after the `Commit` its range names or alone for its commands. The record is [F05 §9.30]'s, the table [F11 §13.5]'s, the verbs [API §8.8]'s | P-6, P-36, P-52, P-65 |
| [80] X-F1, X-F7–X-F12 | none | — |
| [90 §10.1] | none | — |

## Holes

None. No value in this chapter is decided by an M0 measurement. Values that the rules use and that measurements decide are
holes of their owners: `HOLE(F17-lock-writer)` and `HOLE(F17-lock-flush)` (P-27, P-41, P-72), the checkpoint and
promotion thresholds of [F17] (P-51), `HOLE(F15-lock-release)` (the release delays the gates inject), `HOLE(OS-share-retry-ms)`
of [OS/fs §6.3] (P-85), and `HOLE(OS-win-boot-source)` of [OS/proc] (P-60). Measurement 17 is deferred and decides no value
here (P-82). Whether the optional leader is built (measurements 1 and 2) changes no rule (P-95).

## Open points for the review

1. **One seeded bug per rule, and E4.** [PLAN §3.2] WP-40 asks for "one bug per protocol decision of `16-protocol.md`",
   and [PLAN §7] E4 for "the bug list equals `16-protocol.md`'s protocol-decision list". This chapter numbers every rule
   (102) and gives each one primary bug. It reads E4 as: every P-rule has its bug, carried by the toy log where the rule's
   mechanism is in WP-40's scope (76 rules, P-97 among them since the toy rotates, retires and repairs from the extent
   heads), and otherwise by the named gate of the milestone that builds the mechanism (25 rules: P-59 and P-79,
   re-vehicled in spec sync 2b; P-11 and P-84 bulk commits, P-20 `config`, P-21, P-28, P-75, P-85 and P-86 `restore` and
   epoch re-rolls, P-22 and P-87 `backup`, P-23 and P-100 export, P-68 and P-80 segment content, P-78 file-number claims
   of writer-created files, P-93 mapping, P-94 the guard, P-95 the leader, P-26 server settles, P-98 long holdings, P-99
   the pointer file, P-101 and P-102 the body purge and the dropped-set lookup at M2, spec sync 3). P-45, which every
   reachable state masks, is carried by a unit test of its detector (spec sync 2b; the owner accepted that for E4 on
   2026-10-06, OQ-A-1 (a)). §17.4 adds nine bugs for rules of [F03], [OS/proc]
   and [OS/lock] that the lock and liveness layers rely on and for [F02 §3.6]'s retired store (three in the toy log,
   five in GT18, L-9 at M1 with P-85 and P-86). If the review wants all 102 in the toy log at M0,
   WP-40 must model those mechanisms minimally, and WP-40's estimate (2–3 u) grows.
2. **[60 §3.1] item 4 lists fourteen bugs**, not thirteen as [PLAN §3.2] WP-40 says: the A1 re-review added T14 (the
   intent roll-forward without the re-barrier). §17 carries all fourteen.
3. **Extent preparation under the flush byte** (P-8, P-72). [80 §2.3.2] requires the extent to be durable "before the
   first commit in the extent is acknowledged" and names no process or lock. Preparing under the writer byte would break
   P-2 (a 64 MiB zero-fill and a flush). This chapter prepares under the flush byte, before any group lands in the extent,
   and always re-issues `durable+meta` and `durable-name`, because a file of full length left by a dead preparer may have
   no durable size (FM-2.1: bytes beyond the durable size get nothing from `sync(Data)`). The cost is one extent
   preparation inside the flush byte per `E` bytes of log (once per ≈ 150,000 small commits at 64 MiB), which delays the
   other flush holders by the zero-fill once. The alternative, a spare extent prepared ahead by maintenance under the flush
   byte, costs the same wait at another moment; M1's writer-hold and last-acknowledgement gates decide whether it is
   needed. The flush-byte timeout before an append is exit 7 `store_locked`; [F19] should widen that code's text to name
   the flush byte. **Pass 1 (P1-7): resolved differently.** A spare needs no flush byte when it is prepared under a
   temporary name and renamed into place (P-96): a paused preparer then writes only to a name no appender uses, and a
   rotator that finds the full-length name only re-issues the two flushes (P-72 step 2). The zero-fill leaves the commit
   path except when no spare exists (the first rotation, or maintenance never ran), where P-72 step 2 still prepares under
   the flush byte. Measurement 2's workload includes rotations ([F17] Holes `F17-lock-flush`), and M1's writer-hold and
   last-acknowledgement gates are measured with them (for WP-81a, [60 §5.2] item 2). [F19 §10.2] `store_locked` names the
   flush byte (P1-31).
4. **The extent length rule and leftovers of an interrupted preparation.** [F05 §2.2] and [F17 §2.2] IP-6 make an extent
   of another length exit 7. A crash, a death or `DiskFull` during `create_extent` can leave the next extent shorter than
   E (FM-5.4), which P-72 re-prepares. Proposal for [F05] and [F17] (WP-11, WP-16c): the length rule applies to extents at
   or below the one that holds the end of the valid log; a shorter file beyond it is an interrupted preparation, never
   read as log. Similarly [F05 §2.4]'s "reads as zero" holds for every prepared extent; after a crash during a
   preparation the bytes of a full-length file may not be zero (FM-2.2), which is harmless because the position, epoch,
   checksum and chain checks reject them (P-53–P-55). [OS/fs §4.5] should state that re-preparation sets the length to E
   for every method. **Pass 1 (S1-24, A1-24): adopted** in [F05 §2.2], [F17 §2.2] IP-6 and [OS/fs §4.5].
5. **The reservation record of a bulk commit** (P-84; [F09] OP-09-17). The ids and file numbers a bulk file uses must be
   allocated under the writer byte before the file is streamed. This chapter makes that a durable reservation group.
   No record kind of [F05 §7] carries it: [F05] (WP-11) is asked for kind 27 `Reserve` (durable), carrying the file
   numbers of the `cs` and `blobs` files, the `#N` and `aN` ranges, and a `SymDefs` block for the symbols. Its `HEAD`
   effect is the fold of those counters. A durable class costs one extra log flush per bulk commit, which is rare; a lazy
   reservation would be safe too (a lost reservation takes its commit with it by the chain), but would let a failed flush
   in another process make the reservation vanish while the file streams. **Pass 1 (P1-3, S1-11, A1-12): closed** by
   [F05 §9.27], which also carries the schema ids and an `hlc` from which `gc` releases the files of a reservation whose
   commit never landed (P-84).
6. **File-number claims** (P-78) make [F04 §5.13] rule 3 precise. "Advance the counter past an orphan" alone races with
   a live creator between its create and its record; the check under the writer byte closes the race. [F04] (WP-11) should
   cite P-78. **Done:** [F04 §5.13] cites it.
7. **The orphan sweep of `tmp/`** (P-79). No design document says when a temporary is an orphan ([F02] open point 5).
   This chapter uses its age against `gc.delete-grace` and shows that deleting a live temporary is safe (its owner's
   rename fails and it aborts without an acknowledgement). [F17 §11.4] says the grace "does not apply to orphan-sweep
   candidates that no record ever named"; that sentence concerns numbered files, which P-79 deletes by claim without a
   grace, and [F17] should say so. **Done (pass 1, round 2):** [F17 §11.4] "Other deletion paths" states both cases.
8. **The clock of the deletion grace** (P-89; a conflict between spec files). [F17 §11.4] and OP-17-10 measure the grace
   on the HLC from the releasing `Checkpoint`'s `append_hlc`; [OS/clock §6] lists the grace under §4.5's stamp form.
   `Checkpoint` records carry an HLC, not a stamp ([F05 §9.9]), and the grace is not a safety condition (P-77: the barrier
   and the delete-pending and inode rules protect readers). This chapter follows [F17]; [OS/clock §6]'s row should cite the
   HLC rule, or [F05] would have to add a stamp to `Checkpoint`. **Pass 1 (P1-34, S1-43): closed**; [OS/clock §4.5] and §6
   now measure the grace on the HLC.
9. **`MOVEFILE_WRITE_THROUGH` on every Windows rename** (P-82). [PLAN §6.1] #3 and [60 §2.5] (h) name `file mv`;
   [80 §2.3.1]'s `durable-name` row, [F15] OP-2 and [OS/fs] open point 5 read "every rename". This chapter keeps every
   rename, which satisfies all of them; it does not narrow the rule.
10. **`restore` and Windows directory renames** (P-85; [OS/fs] open point 6). A Windows directory rename fails while any
    handle inside is open, so `restore` cannot hold the store's lock bytes across the swap. This chapter makes `retired`
    durable first, which stops every later writer (P-28), then closes its own handles and swaps. While another process
    (for example a running MCP server) holds a file of the store open, the swap fails after the bounded retry, `restore`
    clears `retired` and exits 7; the owner stops the sessions and retries. A store found `retired` at its own discovery
    path with no swap intent is cleared only by `doctor`. **Pass 1, round 3: done.** [F02 §3.6] states what a process
    does then (it repeats the probe with P-86's delays, because a running `restore` sets `retired` before it creates its
    intent, and then exits 7 `store_retired` without using the store), [F19 §10.2] `store_retired` is its text, and
    §17.4 L-9 seeds the bug of a process that loops instead. The rule stays [F02 §3.6]'s; P-86 lends it only its delays.
11. **The discovery probes during a swap** (P-86): 10, 20 and 40 ms. No design document gives a value, and no measurement
    decides it; a two-rename swap takes a few renames and directory flushes, and a longer swap is reported, not waited for.
12. **A reader that cannot run boot-change recovery** (P-60). A read-only principal after a reboot cannot write the store.
    It reads under the Unknown-boot rules, whose documented loss ([80 §2.7.1]) is that a commit acknowledged before the
    crash stays invisible until a writer runs. X5 would prefer a mark; the review decides whether [F19] adds a notice.
13. **`durable_lsn` is a lower bound after a crash.** An OS crash can leave a `HEAD` slot older than the durable log
    ([72 B1]), and a failed `HEAD` flush can do the same (FM-3). `durable_lsn` then understates the durable end. A
    persistent read error ([F15] FM-12) in `(durable_lsn, true end]` ends the log there (P-92), and the next append would
    overwrite acknowledged groups. This follows [80 §2.3.5] (12) and needs a media failure after a crash; it is recorded as
    a residual beside [AR §10] risk 17. A stricter rule — any read error in the log is exit 7 until `repair` — would remove
    it at the cost of availability on a transient error. **Pass 1 (S1-25): the stricter rule is taken for writers only**
    (P-92): a reader still ends its view, and a writer refuses with `store_io_fault` rather than overwrite what may be
    acknowledged (X5). The residual is gone; the cost is that a persistent read error above `durable_lsn` blocks writes
    until `repair`.
14. **`init`'s initial groups** (P-88; [F04] open point 11, [F11] open points 9 and 35, [F09 §16.1]). `init` writes the
    epoch-start group and one group creating `main` (ref id 0, and the binding inside a repository), and no segment. So
    `next_ref_id` = 1 and `refs_lsn` ≠ 0 after `init`, and [F04 §10]'s table needs the corresponding values. **Pass 1
    (A1-25, S1-45): done** in [F04 §10].
15. **The merge pin** (P-81): the pin of a merge into `main` is written with the first checkpoint that folds the merge,
    as [F12 §8.1] proposes; [F05 §4.7]'s checkpoint group lists "the `Pin` records its promotions move" and should add these.
    **Pass 1 (S1-11): done** in [F05 §4.7].
16. **The `park` move** (P-70) adopts [F12] open point 14's `RefUpdate` reason 5, written by the first appender that meets
    the failing commit; [F05 §9.2] (WP-11) adds the reason. **Pass 1 (P1-3, A1-11): done** in [F05 §9.2] and §9.10.
17. **The backup manifest and layout** (P-87; [F05 §9.13], [F02] open point 18). The manifest is defined here. A backup
    is a complete store image whose `HEAD` is written last with `flags.readonly` set, so it is never used for writes in
    place; [F04 §5.2] describes `readonly` as set by an administrative verb, and this is a second setter. `restore` writes a
    fresh `HEAD` and clears it.
18. **Strict HLC** (P-36; [F13] OP-13-10). [OS/clock §7]'s rule makes append-time HLCs strictly increasing in log order;
    I43′'s check at EP-W9 may be strict. **Pass 1 (S1-13, P1-5, A1-17):** strictness now holds for the semantic durable
    records (the HLC sequence of [API §6.2] CK-4), and so for `append_hlc` in `seq` order; `Checkpoint`, `Reserve`, lazy
    and runtime records carry a value without advancing the sequence, so class-I maintenance never changes a commit id.
19. **W1 and W3 on the final encoding** (P-35; [F06] open point 16): re-checked under the writer byte; an inline commit
    that grows past the bound re-runs phase 1 on the bulk path.
20. **Two-parent `durable-name` for renames out of `tmp/`** (P-11, P-20, P-88; [F15] OP-5). The store `config` rewrite,
    `cs.<n>` and `init`'s `config` and `HEAD` sync both `tmp/` and the store directory. [80 §2.3.2]'s store-`config` row
    names one `durable-name`; FM-2.4 needs both.
21. **Temporary names for segments** ([F02] open point 5; [AR §4.1] "segments under construction use a temp name"
    against [80 §2.3.2] and [AR §4.10]). This chapter follows [80]: every sealed file except `cs.<n>` is created under its
    final number (P-10), and P-78 handles the crash state "file written, record not".
22. **A slot that names a missing file** (P-68). Recovery and maintenance rebuild from the newest `Checkpoint` whose files
    exist; readers exit 7 instead (P-59), because a reader cannot write the rebuilt set. The detailed rebuild is M1's
    (`repair`, `doctor --repair`).
23. **Intent recovery under the maintenance byte** (P-71). [40 §3.4] says "at any writer's next open, or in `doctor`".
    Taking the maintenance byte by try makes one recoverer at a time and keeps recovery out of the commit path.
24. **Rules whose violation changes no stored state** (P-2, P-25, P-51 and the trace rows of §17.3) are detected by
    predicates over the simulator's event trace, as [F13] OP-13-02 proposes for I-G4 and I-G6; S4 still holds, since the
    seeded-bug author is not the enumerator's author.
25. **Settles on hook and server paths** (P-26) record A1P-06's rule here; [40 §4.2] and [F20]'s disposition 29 cite it.
26. **Extent heads** (P-97; pass 1, P1-8). The review asked for a durable anchor at every epoch start and at the start of
    every extent. [F05 §9.28] makes it one record kind, `ExtentHead`, whose first instance is the epoch-start group
    (replacing the former 40-byte `Noop`). Beyond the chain value and the `HEAD`-only fields the review listed, it carries
    the log-derived counters as of its append, so that a slot-less `repair` (P-85) starts from exact counters instead of
    re-deriving them from retired history. `retired` and `config_gen` are left out: a repaired store is not retired, and
    `config_gen` is compared for inequality only. `hist` keeps the heads ([F10 §4.1] keeps every record but the pad).
27. **Long holdings** (P-98; pass 1, P1-9, P1-43). Of the review's two variants — work outside the maintenance byte on a
    pinned set, or keep the byte and yield — this chapter takes the second: it needs no new pin holder for a rollup's
    inputs or a backup's copy set, and a yield checkpoint that releases nothing cannot invalidate what the job reads. The
    cost is one extra `HEAD.segments` entry (C-4 of [F17 §3] now leaves P14 ≤ 5) and a rollup that re-folds the yield
    deltas' window over its new base. The bound is stated in [F17 §5.2] and measured by measurement 10.
28. **The quiet bytes** ([F03 §3.1]; pass 1, P1-10). A requester of process-lifetime quiet mode holds one quiet byte of
    its own for its whole run, and every maintenance decider probes all of them; the old rule "a `Busy` answer means quiet
    is already on" failed when the `Busy` came from a Windows probe or when the first holder exited early. §17.4 L-8 seeds
    the decider that probes one byte only.
29. **Read errors in a writer's scan** (P-92; pass 1, S1-25). See open point 13. [F19 §10.2]'s `store_io_fault` row lists
    this trigger beside the mapping fault; its frozen line names the file and offset.
30. **Spec sync 2b** (WP-40 and its reviews). The correct toy took steps the text did not state, and the enumeration
    showed catalogue bugs that no reachable state can show. Adopted before WP-52, since they feed measurements 1 and 2:
    P-29 publishes a lowered `committed_lsn` before appending over a lost lazy tail; P-50 folds from `durable_lsn`; P-8 and
    P-72 re-issue `durable-name` on `tmp/` for a spare; P-88's window after the `HEAD` rename is closed by a per-opening
    `durable-name` on `tmp/` and the store directory (the alternatives, the store directory alone and "the first writer",
    were rejected: the rename has two parents, FM-2.4, and no process can know it is first). Also: P-38's covered test for
    an immediate lazy publish, P-48's refusal to write a fatal slot, P-51's no phase 3 under the maintenance byte, P-61's
    second way to lose both slots and a fatal slot's repair path, P-85's flags from step 1's head and its scan-start
    checks. §17.3: P-40 and P-13 re-formed, P-54, P-55 and P-61 carried through setup-injected states, P-59 and P-79
    re-vehicled to M1, P-97 re-vehicled to the toy (its retirement and slot-less `repair` reach the bug, and [F05 §5.4]
    refuses an extent without its head at every scan), P-45 masked and carried by a unit test of ig6 (E4's acceptance of
    that is OWNER O-1), and sixteen "Detected by" cells corrected to what reports the bug in the toy (the reasons are in
    `moirai-toylog`'s `TOY_DETECTION`); §17.2 widens avail to an unexplained `outcome_unknown`, places the toy's generic
    detectors in `moirai-vfs-sim` and its `model` family in its own `doctor --verify` (OWNER O-2). The independent check
    of the sync aligned [F04 §9.1], [F05 §10.2] and the I-G6 cells with P-50's fold from `durable_lsn`; its second round
    restated PLAN S4 as PLAN does (it names only the enumerator's author; the generic ns check sits with the simulator's
    namespace model, and the toy's own checks are kept honest by R-HARN-S's review), named `doctor --verify` in the
    `model` bullet for the toy, and said in P-65 that the `Marker` records carry the holder-set and flag changes of
    ME-001 to ME-011, not the storage moves of ME-012 and ME-013.
31. **Spec sync 3** ([AR §11] #33, OQ-A-7 (a), decided 2026-10-06; [F06 §8.1] DB-8). The protocol of a body drop:
    - **The record.** `BodyDrop` ([F05 §9.29]) is a semantic durable record (P-36, P-6): it is a command's record, the
      model executes it, and it raises the HLC sequence as every other such record does. A `BodyDrop` command writes it
      alone (P-52); an import writes it before its first commit that names the hash ([F06 §8.1] DB-11). P-34 treats a
      `BodyDrop` in the window as a change of every body key it drops, so no later `Commit` carries a dropped body
      ([F13] I-D1 (a)); P-80 makes every fold seal none and index none as text.
    - **The purge (P-101).** R-SPEC-F's design listed the steps rotate, checkpoint, retire, rewrite segments, rewrite
      `blobs`, rewrite `hist`, move pins, delete. Two changes of order, both forced by the bytes: the `blobs` files are
      rewritten before the segments and `cs.<n>` files, because a `BlobRef` names a `blobs` file's offsets and the
      `hist` rewrite's `cs_ref` names a `cs.<n>`'s length and digest; and the retirements are published last, in the one
      `Checkpoint` that names every replacement, so that an extent holding a `BodyDrop` record stays active until the
      purge is published. That gives `gc` an O(1) test for an unfinished purge — an active extent holds a `BodyDrop`
      record — which survives `repair` (the log keeps the record) without a new `HEAD` field or table column. The cost:
      until a purge runs, retirement stops at that extent, and a store whose purge is pending keeps more active extents
      than `store.log-active-extents` ([F17 §4.2]); `doctor` should report it (a finding for [F19] and [API]). A forced
      rotation ([F05 §4.4] G-3) lets every extent that may hold a carrying record be retired at once: after it, EX-5 needs
      only the checkpoint of step 2. The purge's set H is fixed at that rotation, so a `BodyDrop` appended later lies in
      a later extent and keeps the purge pending for the next run.
    - **What else holds a body's bytes.** Beyond R-SPEC-F's list, step 3 replaces the store's dictionaries when it keeps
      any ([F10 §6]): a raw-content LZ4 dictionary is literal sample bytes, and a trained zstd dictionary holds a content
      section of sample fragments. No record says which bodies a training read, so every live dictionary is replaced and
      every `blobs` file coded with it rewritten; a dictionary-free store (HOLE(F02-dict-file)) pays nothing.
      [F06 §8.1] DB-8's end state should list dictionaries (a finding for R-SPEC-F), and [F10 §6.3]'s "trained at `init`
      or at a rollup" gains the purge (R-SPEC-R).
    - **Deletion without the grace.** The grace only spares readers a retry (P-77, P-89), so a purge deletes after the
      barrier without waiting for it, and `BodyDrop` can report `purged` true when it returns. `backup` runs a pending
      purge first (P-87), so only backups whose copied state precedes the drop hold the bytes ([F06 §8.1] DB-10).
    - **Rebuilt stores.** `restore` and `repair --rebuild-from-log` retire every extent, which would end a pending purge
      without its rewrites; P-75 makes the build apply them first.
    - **Readers (P-102).** Every process that resolves a body hash consults the dropped set of its view before the tail
      and `BLOBTAB`, so readers never depend on how far a purge got, and BD-6's obligation leaves dropped hashes out.
    - **Vehicles.** P-101 and P-102 are carried at M2, where bodies and `BodyDrop` are built ([API §8.7]): P-101's bug by
      the new detection family **fsck** ([F13] I-D1 through `doctor --fsck`), P-102's by the model. The toy log has no
      bodies, so neither is a WP-40 bug; open point 1's counts are 76 toy, 25 gate and one masked rule.
    - **Independent check of spec sync 3** (`docs/spec/reviews/spec-sync-3.md`).
      - *Only the owner drops held bytes.* An import's `BodyDrop` now names only hashes its store neither holds nor has
        dropped and whose bytes the import supplies nowhere ([F05 §9.29] "Origin", [F06 §8.1] DB-11), so a foreign
        commit or a hand-made image can no longer make a non-owner import drop held bytes. Such a record leaves nothing
        to purge, so the record's new `origin` byte keeps it out of the pending test (P-73): an import that met a
        dropped line no longer keeps every later extent active until the next `gc`.
      - *Step 1's bytes.* The check proposed "takes the writer byte and then the flush byte"; that order would break
        P-1 (flush before writer: the writer byte is innermost, P-2). Step 1 now names the bytes in P-1's rotation order.
      - *Structures written during the purge.* After step 3 the old `blobs` files are still the published ones, so a
        yield delta of the purge's holding (P-98) and the `cs.<n>` of a bulk commit appended after the forced rotation
        may name a file step 3 replaced; step 8 would then delete a file a live `BlobRef` names. The check proposed
        writing them against step 3's replacement map. That is not taken: the replacements are named by no published
        record before step 7, so a published yield delta pointing into one would name a file outside its set's `FILES`
        registry, which the orphan sweep removes when the purge is cut short (P-79); and another process cannot know
        the map at all. Instead step 7 rewrites the purge's yield deltas by step 4's rule, and a bulk commit whose
        `cs.<n>` names a replaced file sends the purge back to step 1, because its record stays in the log and its
        `cs_ref` cannot follow a rewrite: the new rotation puts it below the retirement line, where steps 5 and 6
        rewrite its `cs.<n>` and `hist` entry. The `cs.<n>` files are read outside the writer byte (P-2), and at most
        three attempts at step 7's append per run bound the work; a purge that bulk commits keep from publishing stays
        pending, which `BodyDrop` reports as `purged` false and `backup` refuses with `maintenance_busy` (a code whose
        text [F19] may widen). Step 7 releases no file that a live `BlobRef`, `cs_ref` or `FILES` row names.
      - *No automatic purge* (P-76). Of the check's two options (run a pending purge from the automatic retirement
        trigger once more than 2 × P02 extents are active, or state that nothing automatic runs one), the second is
        taken: the purge is a long job of a rollup's size, which no CLI, hook or MCP process runs (P-51), and starting
        it detached would need a verb [API] does not define. With import records out of the pending test, only a busy
        maintenance byte at `BodyDrop`, a crash or the bound above leaves a purge pending; `BodyDrop`, `gc` and
        `backup` finish it, and `doctor`'s `purge_pending` ([F19], [API §8.6], a finding for R-SPEC-F) names it.
      - *`Harvest`.* The reserved record kind 30 of the harvest cursor ([F05 §9.30]) is durable (P-6), a semantic
        record of the HLC sequence (P-36: its commands are executed by the model and its effect is visible), applied at
        adoption (P-65), and placed in an `Apply` group after the `Commit` its range names or alone for `HarvestMark`
        and `HarvestForget` (P-52). It adds no rule and no seeded bug: its writers are M9–M10's.
    - **Closure check of spec sync 3** (`docs/spec/reviews/spec-sync-3.md`, R-SPEC-P).
      - *Bulk commits after the publish* (P-84 step 3). Step 7 checked only the bulk `Commit` records appended before
        its own append. A producer that streamed against the pre-purge set and whose `Commit` lands after the publish
        would name a `blobs` file step 7 released and step 8 deleted without the grace, since P-84 step 3 checked only
        its own reservation's files. P-84 step 3 now also checks every `blobs` file its `cs.<n>`'s `BLOBTAB` names, from
        file numbers the producer kept while streaming, so the check reads only the log. That closes the same gap for a
        rollup, a tiered fold and blob GC, whose releases a later bulk commit could otherwise name after the grace.
      - *Claims under one holding* (P-78). The purge's outputs stay unnamed until step 7, and every yield checkpoint of
        its holding, and the step-2 checkpoint of a restart, carries a `next_file_no` above their numbers ([F05 §9.9]),
        so P-78's second form claimed them and step 7's creator check failed, against "it may reuse those whose inputs
        did not change"; under write load the purge spent its three attempts and stayed pending. Rollups with yield
        checkpoints had the same problem. A `Checkpoint` now claims by the second form no number created under its own
        holding of the maintenance byte and not yet named: no sweeper runs while that holding lasts, and after it ends
        a later holding's sweep claims such a number as before. The other option (step 7 rewrites every claimed
        output) is not taken: it would make every purge with a yield checkpoint start its rewrites again.
      - *Reservations that have not landed* (P-84, step 3 and step 7's third item; [F05 §9.29]; [F13] I-D1). Bytes that
        only the `blobs.<n>` of such a reservation holds are not held for "Origin", so an import may drop such a hash
        without a purge; and a command's purge did not reach that file either. Such a reservation's `Commit` cannot land
        carrying a dropped body (P-34), so `gc` and a purge's step 7 release its files without `gc.cruft-delay`, and
        I-D1 (b) and (c) treat them as a deletion still due. Counting the bytes as held is not taken: the reservation's
        `Commit` may never land, which would leave an imported body key with no bytes that is not dropped.
      - *Yield checkpoints release only yield deltas.* The paragraph after step 8 said they "release nothing"; P-98
        lets a yield checkpoint fold and release the earlier yield deltas of its holding. It now says they release no
        file the purge reads.
