# 05 — The log

| | |
|---|---|
| Title | The log: extents and their size parameter, log positions (lsn), the 32-byte `RecHdr` with `lsn`, `epoch`, `xxh3_64` and the `group_end` flag, flushed groups and the `chain` trailer (X-F3), groups that never span extents and the rotation padding rule, the scan's validity and end-of-log rules, and every record kind with its payload layout and durability class |
| Chapter | [F05], `docs/spec/format/05-log.md` |
| Status | draft, pass 1 pending |
| Work package | WP-11 (R-SPEC-P), [60 §3.1] item 1 |
| Sources | [AR §4.1] `log`, `hist`, `cs` and `gitmap` rows and the rules paragraph; [AR §4.2] (the log-derived `HEAD` fields and the table pointers); [AR §4.3] (`RecHdr`, validity, groups, the chain, the record kinds, the commit body's place); [AR §4.4] (the sections records fold into, the `LEASES`, `MARKERS`, `IDEM` rows); [AR §4.5] steps 6, 8, 9, 10 and 12; [AR §4.9] (checkpoint, retirement, pins, GC); [AR §4.10]; [AR §5a.1]–[AR §5a.5] (refs, reflog, client heads, pins, promotion); [AR §5b.6] (`gitmap`, `image_cursor`, last-seen oids); [AR §5d.1] (runtime state); [AR §6.2] (leases, heartbeats, anchors); [AR §6.3] (cursors); [AR §6.4] (idempotency); [AR §6.5] (durability classes); [AR §13] `durability.lazy-kinds`; [80 §2.3.1]–[80 §2.3.5]; [80 §2.4.3] (the chain, pending groups, the publish fold); [80 §2.7.2]; [80 §2.11.2] (`OsFileId`, timestamps, `JOURNALCUR`, `DIRMAP`, `TREES`, the `FSINTENT` holder); [80 §3.1] X-F2, X-F3, X-F5, X-F8; [40 §2.6] (every runtime table, the `TreeReg` epoch layout); [40 §2.7] (anchor handles); [40 §2.11] R-6, R-7, R-15, R-18; [40 §3.4], [40 §3.5] (the intent protocol); [50 §8.1] F8, F9, F14, F17; [90 §4.1], [90 §4.3], [90 §4.4], [90 §10.1] (`LEASES` rows); [60 §2.5] the [AR] row "Log", the audit row "Log", the "Protocol decisions" row (decisions (a), (b), (k)), the issue-2 row "Store parameters"; [70 S5], [70 S7], [70 S8], [70 S12]; [71 RAM-M5]; [72 M1], [72 M9]; [73 F4]; `docs/spec/reviews/a1-P.md` A1P-04, A1P-12; [PLAN §3.2] WP-11, [PLAN §3.3] (rotation padding when fewer than 40 B remain; record kinds for lazy `ANCESTRY`, heartbeat and cursor; the stale [60 §2.5] copy of R-7) |
| Depends on | [F01], [F02], [F03], [F04], [F17]; cites [F06], [F09], [F10], [F11], [F13], [F15], [F16], [F18], [F19], [F20], [API], [CFG], [OS/clock], [OS/fs], [OS/proc], [OS/project] |

## 1. Scope

This chapter owns the bytes of the log: how extents are named and sized, how a log position maps to a file and an offset,
the record header, the group and its chain trailer, the rules that decide which bytes form the valid log, and the payload
of every record kind except the commit body, which is [F06]'s. It states each record kind's durability class, its effect
on `HEAD` ([F04]) and the runtime table it folds into ([F09], [F11]).

When a process appends, flushes, publishes, re-writes, recovers or checkpoints is [F16]'s protocol. The layouts of the
sections that records fold into are [F09]'s and [F11]'s; the `hist` files that retired extents become are [F10]'s.

**Terms.**

| Term | Meaning |
|---|---|
| **lsn** | a log position: the byte offset in the store's single log stream (§2.3) |
| **extent** | one file `log.<n>` holding `E` consecutive bytes of the log stream |
| **record** | a `RecHdr` (§3) and its payload, at one lsn |
| **group** | a run of consecutive records whose last record carries `group_end` and the chain trailer (§4) |
| **group boundary** | the lsn of the first byte of a group: `HEAD.epoch_lsn` or the end of the previous group |
| **chain value at p** | the value that seeds the chain of the group beginning at boundary p (§4.3) |
| **valid log** | the longest run of valid groups from the scan's start (§5) |
| **pending group** | a complete valid group beyond `HEAD.committed_lsn` that no published flush covers yet ([80 §2.4.3]) |
| **tail** | the valid records in `[HEAD.checkpoint_lsn, HEAD.committed_lsn)` |
| **E**, **k** | `E` = `HEAD.init.log_extent_bytes` ([F04 §4.4], [F17 §4.1]); `k` = log2 `E` |

## 2. Extents

### 2.1 Names

An extent is the file `log.<n>` of the store directory, with `n` a file number from 1 to 4,294,967,295 written in
decimal ([F02 §6]). The number follows from the lsns the extent holds (§2.3), so extents never use the sealed-file
allocator of [F04 §5.13], and no number is ever used twice.

### 2.2 Size

- Every extent is exactly `E` bytes long ([F17 §2.2] IP-6, [80 §2.3.3]).
- `E` is init-fixed ([F17 §4.1], P01): a power of two in [2^16, 2^30]. This chapter confirms that range ([F17]
  OP-17-04): a power of two makes the lsn mapping a shift and a mask, 2^16 keeps room for the test profile's commits, and
  2^30 keeps every offset inside an extent below 2^30.
- **The length rule.** A process that finds an extent of another length exits 7, naming the file and
  `moirai doctor --fsck` ([F17 §2.2]), when the extent is at or below the extent that holds the end of the valid log, or
  when it is longer than `E`. A file `log.<m>` shorter than `E` beyond that extent is the leftover of an interrupted
  preparation (a death, a crash or a `DiskFull` during `create_extent`, [F15] FM-5.4): it never held a group, is never
  read as log, and the next rotation re-prepares it ([F16] P-72). A full-length file beyond it is a spare or a completed
  preparation ([F16] P-96), equally never read as log until a rotation appends into it (pass 1, S1-24, A1-24).

### 2.3 Log positions

The log is one byte stream. Its lsn L lies in extent `n(L) = (L >> k) + 1` at offset `o(L) = L & (E − 1)`: extent n
holds the lsns `[(n − 1)·E, n·E)`. lsn 0 is the first byte of `log.1`.

- Every lsn in a record header, a payload ([F06], §9) or a `HEAD` field ([F04]) is a position in this stream.
- lsns are never reused: a later epoch starts at a new extent (§2.6).
- The last usable extent is `log.4294967295`. A write that would need a later extent is refused with exit 7 and nothing
  is written ([F19]).
- An lsn is a `u64`. The greatest lsn, `(2^32 − 1)·2^30`, is below 2^62.

### 2.4 Content

- **Format invariant** ([80 §2.3.3], [AR §4.1]): every byte of an extent that has not been written since the extent was
  created reads as zero. `create_extent` produces that state per file system ([OS/fs §4.5]).
- Bytes beyond the end of the valid log that were written earlier (a group lost after an OS crash or a failed flush, a
  lost lazy tail) may hold anything. The next append overwrites them; until then the rules of §5 make them invisible.
- An extent must exist, be exactly `E` bytes long and be durable (`durable+meta` on the extent, `durable-name` on the
  store directory, [80 §2.3.2]) before any group in it is acknowledged. [F16] fixes who creates the next extent and when
  (P-72, P-96).
- The first group of every extent is its extent-head group (§4.5).

### 2.5 Active extents and retirement

- **EX-1.** The extents from `HEAD.active_log` up to the extent that holds the end of the valid log all exist; they are
  the **active** extents. No active extent is missing.
- **EX-2.** An extent below `HEAD.active_log` is **retired**: its history lives in a `hist` file ([F10], which states
  which record kinds a `hist` file keeps), and its file may be deleted after the two-slot barrier, the deletion grace and
  the pin check ([F04 §9.3], [F17 §11.4], [F16]).
- **EX-3.** An extent is retired only by a `Checkpoint` record's retirement entry (§9.9), which names the `hist` file that
  now holds its history; the publish that covers the record advances `active_log`. Extents are retired oldest first.
- **EX-4.** Extent n is retired only when every record in it lies below `HEAD.checkpoint_lsn`.
- **EX-5.** And only when `HEAD.checkpoint_lsn` > `n·E`, that is, when at least one group of extent n + 1 is folded too.
  The chain value at `checkpoint_lsn` (§4.3) then lies in an active extent, and every scan can start there. The one
  exception is an epoch start, where `checkpoint_lsn` = `epoch_lsn` and the chain value is the epoch seed.
- How many extents stay active is `store.log-active-extents` ([F17 §4.2], P02).

### 2.6 A new epoch

`restore` and `repair --rebuild-from-log` re-roll the epoch ([F04 §5.3]). The format requires:

1. The new epoch's first group starts at the first byte of an extent m whose number is greater than every extent number
   the store has used; `HEAD.epoch_lsn` = `(m − 1)·E`.
2. Every extent below m is retired before the publish that installs the new epoch ([F16]), so no scan meets a record of
   another epoch.
3. The first group of the new epoch is its epoch-start group (§4.5).
4. An older extent file may serve as `log.<m>` only after it has been renamed to that name and zero-filled
   (`recycle_extent`, [OS/fs §4.5]); whether [F16] reuses files at all is its decision (open point 2).

### 2.7 Rotation

A group that does not fit the rest of the current extent goes to the next one after the current extent is padded (§4.4).
The pad, the next extent's extent-head group (§4.5) and the group are three groups: the extent ends with the pad's chain
trailer, whose 8 bytes are the chain value at the next extent's first byte, and the extent head that begins the next
extent carries that value (`chain_in`, §9.28), so the next extent can be validated after the old one is retired.

## 3. The record header

### 3.1 `RecHdr` (32 B)

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 4 | `u32` | `len` | the record's total length in bytes: header, payload and, when `group_end` is set, the 8-byte chain trailer. 32 ≤ `len` ≤ `E`; `len` ≥ 40 when `group_end` is set |
| 4 | 1 | `u8` | `kind` | the record kind (§7); 0 and 29–255 are invalid |
| 5 | 1 | `u8` | `flags` | §3.2 |
| 6 | 2 | `u16` | `_reserved` | reserved-zero ([F01 §10]) |
| 8 | 8 | `u64` | `lsn` | the record's own position in the log (§2.3) |
| 16 | 8 | `u64` | `epoch` | the store epoch when the record was written ([F04 §5.3]) |
| 24 | 8 | `u64` | `xxh3_64` | the record checksum (§3.4) |
| total | 32 | | | |

### 3.2 Flags

| bit | name | meaning |
|---|---|---|
| 0 | `lazy` | 1: the record's durability class is `lazy`; 0: `durable` (§6, X-F5) |
| 1 | `group_end` | the record is the last of its group and ends with the 8-byte chain trailer (§4.2) |
| 2 | `symdefs` | the payload begins with a symbol-definition block (§8.1) |

Bits 3–7 are reserved-zero.

### 3.3 The record's bytes

With `t` = 8 when `group_end` is set and 0 otherwise:

| Range (offsets in the record) | Content |
|---|---|
| `[0, 32)` | `RecHdr` |
| `[32, len − t)` | the **payload**: a `SymDefs` block when `symdefs` is set (§8.1), then the kind's payload (§9) |
| `[len − t, len)` | the chain trailer, when `group_end` is set (§4.2) |

Records are byte-packed: a record begins at the byte after the previous record ends. There is no alignment padding.

### 3.4 The record checksum

`xxh3_64` = XXH3-64 with seed 0 ([F01 §7.1]) over the concatenation of the record's bytes `[0, 24)` and `[32, len − t)`:
the header without its checksum field, then the payload. It excludes the trailer, which is computed over the group's
bytes and therefore over this checksum. It is stored as a little-endian `u64` ([F01 §7.2]).

## 4. Groups and the chain

### 4.1 Groups

A **group** is a run of one or more consecutive records r1 … rk in which rk, and only rk, carries `group_end`. It
begins at a group boundary p and ends at `e = p + Σ len(ri)`. Records are written in groups ([AR §4.3]); recovery and
readers adopt a group all or nothing ([72 M1]).

### 4.2 The chain trailer

The last 8 bytes of rk hold

```
chain = XXH3-64( bytes [p, e − 8) of the log ; seed = chain value at p )
```

the seeded form of [F01 §7.1] over every byte of the group before the trailer, headers and record checksums included,
stored as a little-endian `u64` ([80 §2.4.3], X-F3). The value is the **chain value at e**.

### 4.3 The chain value at a boundary

For a group boundary p:

- if p = `HEAD.epoch_lsn`: `XXH3-64(epoch)` with seed 0 over the 8 little-endian bytes of `HEAD.epoch` ([F01 §7.3]);
- otherwise: the `u64` read little-endian from the 8 bytes at `[p − 8, p)`, the trailer of the group that ends at p.
  When p is the first byte of an extent, those bytes are the last 8 bytes of the previous extent.

A group therefore validates only behind the exact predecessor its writer validated it against, and the chain covers the
whole prefix transitively ([80 §2.4.3], [81] B1).

### 4.4 Placement: groups never span extents

With H = 138, the length of an extent-head group (§4.5, §9.28), and the **rotation reserve** R = H + 40 = 178 bytes:

- **G-1.** A group lies inside one extent: `n(p) = n(e − 1)`.
- **G-2.** A pad group (below) is at most `E` bytes and an extent-head group exactly H bytes. Every other group is at most
  `E − R` bytes long.
- **G-3 (placement and rotation padding).** Let a writer append a group of length g at boundary p, and let
  `r = n(p)·E − p` be the bytes left in the extent (0 < r ≤ E).
  - If p is an extent's first byte (r = E), the extent-head group of that extent is written at p first (§4.5); p then
    moves past it and r = E − H.
  - If g = r, or g ≤ r − 40, the group is written at p.
  - Otherwise the writer first writes a **pad group** at p: exactly one `Noop` record with `group_end` and `len` = r,
    whose payload is r − 40 zero bytes (§9.12). It then writes the next extent's extent-head group at `n(p)·E`, the first
    byte of the next extent, and its group right after it.
- **G-4 (invariant).** At every group boundary p, either p is an extent's first byte or at least 40 bytes are left in
  its extent. An extent-head group leaves E − H ≥ 40 bytes (E ≥ 2^16), and G-3 keeps the invariant: a group written at p
  leaves 0 or at least 40 bytes, and the pad of G-3 always has r ≥ 40.
- **G-5.** Because of G-3 and G-4 a pad group is always long enough for its 40-byte minimum, and because of G-2 every
  group fits the extent it goes to right after that extent's head (g ≤ E − H − 40 = r − 40). A group is therefore padded
  at most once and never loops. The rotation reserve of [F17 §4.4] W3 is R: the largest group other than a pad or an
  extent head is `E − R` bytes (pass 1, S1-18; the earlier reserve of 0 left groups of `E − 39` to `E − 1` bytes that no
  boundary could take).

The 40-byte threshold is the smallest possible group: one record with an empty payload and the 8-byte trailer.

### 4.5 The extent-head group and the epoch-start group

The first group of every extent is its **extent-head group**: exactly one `ExtentHead` record (§9.28), durable,
`group_end` = 1, `symdefs` = 0, `len` = H = 138, at the extent's first byte (pass 1, P1-8). Its payload repeats what a
reader of this extent alone needs to validate and rebuild from it: the chain value at its own lsn (`chain_in`), the
epoch's `epoch_lsn`, the store's `init` parameters and `project_oid_algo`, the persistent `HEAD` flags `quiet` and
`readonly`, and the log-derived counters and HLC maxima of `HEAD` as they stood before it.

- The rotating appender writes it (§2.7, §4.4 G-3; [F16] P-72, P-97). It is durable, so a lost extent head takes every
  later group of its extent with it by the chain, and the next appender at that first byte writes it again.
- The first group of every epoch is the extent head at lsn `HEAD.epoch_lsn` (an epoch always starts at an extent's first
  byte, §2.6), seeded with `XXH3-64(epoch)` (§4.3): the **epoch-start group**. `init` writes it at lsn 0 of `log.1` (the
  first group after `init`, [80] X-F3); each epoch re-roll writes it at the new `epoch_lsn` (§2.6). The sequences are
  [F16]'s.
- Its `chain_in` and its trailer let a process that has lost every `HEAD` slot, or every earlier extent, validate the log
  from the lowest surviving extent of the epoch: the head's record checksum, its position (`lsn` = p) and its trailer
  recomputed with the carried `chain_in` authenticate it, and every later group follows by §4.2. `repair` uses this to
  recognise the start of an epoch and to rebuild the slot state ([F15] OP-1, [F04 §8.1], [F16] P-85).
- lsn 0 therefore always holds an `ExtentHead` record, which lets `HEAD`'s table pointers use 0 for "none" ([F04 §5.10]).

### 4.6 Group validity

A group is **valid** when every record in it is valid (§5.2) and its trailer equals the chain value computed by §4.2
with the chain value at p. Every other group is invalid.

### 4.7 Group composition

A group's class is `durable` when at least one of its records has `lazy` = 0, and `lazy` otherwise (§6). Writers form
groups by these rules; readers validate groups by §4.6 alone and do not check composition, except the two placement
defects of §5.4 (a valid group that breaks G-4, and an extent's first group that is not one `ExtentHead`).

| Group | Records, in order | Rule |
|---|---|---|
| commit group | one or more `Commit` records, then the records they imply: `RefUpdate` and `RefTable` (for example `lane open`), `Lease`, `Marker`, `Idem`, `Pin`, `FsIntentDone`; optionally lazy evidence records of the same settle | a commit and the `Marker` records its net ops imply are in one group ([AR §4.3]: a completion is never adopted without its marker); a `file mv` or `file rm` commit and its `FsIntentDone` are in one group ([40 §3.4] step 4); a sync-first merge writes both commits in one group ([AR §5a.7] step 0) |
| ref group | `RefUpdate` records, each with the `RefTable` record that carries the new entries, then `Pin`, `Marker`, `Lease` and `Idem` records it implies | every `RefUpdate` and the `RefTable` entries of the refs it changes are in one group (§9.10); a park (reason 5) is a ref group of its own, appended before the appender's group ([F16] P-70) |
| reservation group | one `Reserve` | a bulk commit's reservation, acknowledged before its `cs.<n>` streams ([F16] P-84) |
| lease group | `Lease`, then `Idem` | — |
| client-head group | `ClientHead`, then `Idem` | — |
| intent group | one `FsIntent` | alone, so its durability precedes the rename ([40 §3.4] step 2) |
| abort group | one `FsIntentAborted` | — |
| checkpoint group | `Checkpoint`, then the `Pin` records its promotions move and the holder-3 `Pin` records of the merges into `main` whose commits its set folds first | [F16] P-81, [F12 §8.1] (pass 1, S1-11) |
| export group | `GitMap` records | one durable group per export run ([AR §5b.6] step 5) |
| backup group | one `Backup` | — |
| lazy group | one or more records of the lazy or configurable kinds (`Lazy`, `SessionMark`, kinds 18–26) | a hook's evidence and a settle's `TreeReg` epoch and changed rows ([40 §2.6]) |
| pad group | one `Noop` | §4.4 |
| extent-head group (the epoch-start group included) | one `ExtentHead` | §4.5; the first group of every extent |

## 5. The scan

### 5.1 Where a scan starts

A scan starts at a group boundary L whose chain value it knows:

- `HEAD.checkpoint_lsn`, with the chain value of §4.3 (recovery and every fresh open; `min(durable_lsn, checkpoint_lsn)`
  of [AR §4.2] equals `checkpoint_lsn`, [F04 §5.4]);
- a process's own replay bound L0 with the chain value it remembered there ([80 §2.4.3] phase 2a step 3);
- `HEAD.durable_lsn`, for the flush holder's scan of the pending range ([80 §2.4.3] phase 2b step 4);
- the first byte of an extent, with the `chain_in` its extent head carries (§4.5), for `repair` without a valid `HEAD`
  slot ([F16] P-85, P-97); the head's own trailer must match that value first.

No scan starts below `HEAD.epoch_lsn`. A process whose remembered chain value at L0 no longer matches the log drops its
overlay and replays from its segment set ([80 §2.4.3]).

### 5.2 Record validity

At a position p inside a group, the scanner reads the record header and checks, in order:

1. **Extent.** If p is an extent's first byte and the extent file `log.<n(p)>` does not exist, there is no record.
2. **Length.** 32 ≤ `len`, `o(p) + len ≤ E` (the record lies inside its extent), and `len` ≥ 40 when `group_end` is set.
3. **Kind.** `kind` is in the registry (§7).
4. **Header bits.** `flags` bits 3–7 and `_reserved` are zero; `lazy` agrees with the kind's class (§6.1); a `Noop` or an `ExtentHead` has
   `symdefs` = 0.
5. **Position.** `lsn` = p. This rejects a record left at another position by an earlier write of the same epoch
   ([72 M1]).
6. **Epoch.** `epoch` = `HEAD.epoch` of the slot the scanning process uses.
7. **Checksum.** `xxh3_64` matches (§3.4).

A record that passes is **valid** ([AR §4.3]). Payload well-formedness is not part of validity (§5.4).

### 5.3 The end of the valid log, and corruption

The scan applies §4.6 to one group after another. At a boundary p where the next group is invalid — a record fails
§5.2, the extent ends before a record carrying `group_end`, the trailer does not match, or the next extent is missing:

- if p ≥ `HEAD.durable_lsn`: p is the **end of the valid log**. The invalid bytes are a lost lazy tail, a group lost
  before any flush covered it, or never written; the next append overwrites them ([60 §2.5] decision (b) as restated,
  [72 B1]);
- if p < `HEAD.durable_lsn`: the store is **corrupt**. The process exits 7 naming the log extent and `moirai repair`
  ([F19]). It never truncates below `durable_lsn`.

A **failed read** (fault-model item (12), [F15] FM-12) is not an invalid group, because it says nothing about the bytes:
below `durable_lsn` it is corruption as above; at or above it, a reader's visible log ends before it (next paragraph),
and a writer's scan — an appender's, a flush holder's, boot-change recovery's, a `repair`'s — appends and re-writes
nothing and exits 7 `store_io_fault` ([F16] P-92, [F19 §10.2]). `durable_lsn` understates the durable end after an OS crash ([72 B1]), so bytes above it may hold
acknowledged groups, and a writer that treated an unreadable range as the end of the log would overwrite them (pass 1,
S1-25).

A **reader** reads only the visible log `[checkpoint_lsn, committed_lsn)`. An invalid group, or a failed read, at a
boundary p with `durable_lsn` ≤ p < `committed_lsn` ends the visible log (a lazy tail lost after an OS crash or a failed
flush), never an error; below `durable_lsn` it is corruption as above ([AR §4.7], [80 §2.4.3] "Readers").

### 5.4 Payload well-formedness

A record's payload is decoded when it is used; runtime records are indexed first and decoded on use ([71 RAM-M5]). A
valid record whose payload does not follow its kind's rules — a varint refused by [F01 §5.2], a length that runs past the
payload, a non-zero reserved bit or byte, a value outside its enumeration, bytes left after the last field, an undefined
symbol id (§8.1) — is **corrupt wherever it lies**: exit 7 naming the extent and `moirai doctor --fsck`. A torn or stale
write cannot produce a matching checksum except with probability 2^-64, so such a record can only come from a defective
writer, and hiding it would weaken the store silently (X5).

The same holds for two placement defects of a valid group (§4.6), for the same reason (only a defective writer produces
a checksummed, chained group like that): a group that leaves 1–39 bytes in its extent (G-4 broken), and a first group of
an extent that is not one `ExtentHead` record (§4.5). Either is **corrupt wherever it lies**, above `durable_lsn`
included: every scan, a reader's, a writer's and a `repair`'s, stops with exit 7 naming the extent and
`moirai doctor --fsck`, and no writer appends over it.

### 5.5 What a scan yields

The valid log from the scan's start, its end E_v, and the chain value at E_v. The groups in `(committed_lsn, E_v]` are
pending groups; which process may apply which of them, and to what, is [F16]'s ([80 §2.4.3]: pending groups go only to a
scratch layer).

## 6. Durability classes

### 6.1 The class of a record

`RecHdr.flags` bit 0 carries one class per record ([80] X-F5, [AR §6.5]):

| Registry class (§7) | Bit 0 on disk |
|---|---|
| `durable` | must be 0 |
| `lazy` | must be 1 |
| `configurable` | 1 when the writer's `durability.lazy-kinds` ([AR §13], [CFG]) contains the record's configurable kind at the time of the append, else 0 |

The configurable kinds are `heartbeat` and `cursor` (the `Lazy` record, §9.11) and `session-mark` (`SessionMark`,
§9.14), the members of the key's set ⊆ {heartbeat, cursor, session-mark}, all three lazy by default. A reader never
consults the configuration: bit 0 as written decides ([F17 §1.4] SP-R2). Graph mutations are always durable, a rule no
key changes ([AR §6.5]): `Commit` is fixed `durable`.

### 6.2 What the classes mean

The guarantees are [80 §2.3.1]'s and [F15 §4.1]'s; the protocol that provides them is [F16]'s:

- a **durable** group is acknowledged only after a flush that covers it returned, a publish covered it, and its writer's
  identity check passed ([80 §2.4.3], I-G1);
- a **lazy** group is published at once when no durable group is pending, else by the publish that covers it; it may be
  lost after an OS crash, a power loss or a failed flush in any process, and a lost lazy tail is the end of the log (§5.3).

## 7. The record kinds

| value | kind | class | runtime (`K_RT`) | payload | folds into | effect on `HEAD` ([F04]; §10.2) |
|---|---|---|---|---|---|---|
| 1 | `Commit` | durable | no | [F06]; §9.1 | the graph (segments, [F09]); per-ref lists; `ALLOC` ([50] F17) | `commit_seq`, `next_id`, `next_anchor`, `seq_ring`, `next_file_no` |
| 2 | `RefUpdate` | durable | no | §9.2 | the reflog ([F11] `REFS`) | `next_ref_id` |
| 3 | `ClientHead` | durable | no | §9.3 | `HEADS` ([F11]) | `heads_lsn` |
| 4 | `Lease` | durable | no | §9.4 | `LEASES` ([F11]) | `fence` |
| 5 | `Marker` | durable | no | §9.5 | `MARKERS` ([F11]) | `markers_lsn` |
| 6 | `Idem` | durable | no | §9.6 | `IDEM` ([F11]) | — |
| 7 | `GitMap` | durable | no | §9.7 | `gitmap` pages ([F10]) | `image_cursor` |
| 8 | `Pin` | durable | no | §9.8 | `PINS` ([F11]) | `pins_lsn` |
| 9 | `Checkpoint` | durable | no | §9.9 | the segment set and the file registries ([F09], [F10]) | `segments`, `n_segments`, `checkpoint_lsn`, `active_log`, `flags.fts_tier2`, `next_file_no` |
| 10 | `RefTable` | durable | no | §9.10 | `REFS` ([F11]) | `refs_lsn`, `next_ref_id` |
| 11 | `Lazy` | configurable | no | §9.11 | `LEASES` deadlines; `CURSORS` ([F11 §13]) | — |
| 12 | `Noop` | lazy | no | §9.12 | nothing (the rotation pad) | — |
| 13 | `Backup` | durable | no | §9.13 | `BACKUPS` ([F11 §13]) | — |
| 14 | `SessionMark` | configurable | no | §9.14 | `SESSMARKS` ([F11 §13]) | — |
| 15 | `FsIntent` | durable | no | §9.15 | `FSINTENT` ([F11]) | — |
| 16 | `FsIntentDone` | durable | no | §9.16 | `FSINTENT` | — |
| 17 | `FsIntentAborted` | durable | no | §9.17 | `FSINTENT` | — |
| 18 | `FileObs` | lazy | yes | §9.18 | `FILEOBS` ([F11], R-18) | — |
| 19 | `Pending` | lazy | yes | §9.19 | `PENDING` | — |
| 20 | `FPrint` | lazy | yes | §9.20 | `FPRINT` and fingerprint blobs ([40] R-9, [F10]) | — |
| 21 | `JournalCursor` | lazy | yes | §9.21 | `JOURNALCUR` | — |
| 22 | `DirMap` | lazy | yes | §9.22 | `DIRMAP` | — |
| 23 | `TreeReg` | lazy | yes | §9.23 | `TREES` | — |
| 24 | `PrefixEv` | lazy | yes | §9.24 | `PREFIXEV` | — |
| 25 | `GitFacts` | lazy | yes | §9.25 | `ANCESTRY`, `GITRENAMES` ([F09]) | — |
| 26 | `AnchorRes` | lazy | yes | §9.26 | `ANCHORRES` | — |
| 27 | `Reserve` | durable | no | §9.27 | the store-wide schema-id map ([F08 §8.3]); the allocators | `next_id`, `next_anchor`, `next_file_no` |
| 28 | `ExtentHead` | durable | no | §9.28 | nothing (read by `repair`) | `hlc_seq`, `hlc_commit`, the counters it carries |

- Values 0 and 29–255 are invalid in format v1 ([F01 §9.3]).
- The set `K_RT` is [F17 §5.1]'s: the lazy runtime kinds of [40] R-7 plus `AnchorRes`, which the runtime-only fold
  targets. Records outside `K_RT` count in `rec_bytes` ([F17 §5.1]).
- Kinds 1–12 are [AR §4.3]'s base kinds, kinds 15–26 [40 §2.11] R-7's (authoritative, with `AnchorRes`; the [60 §2.5]
  copy of R-7 omits it, open point 13), and kinds 13 and 14 the audits' `Backup` ([72 M9]) and `SessionMark` ([73 F4]).
  Kinds 27 and 28 are the protocol's: the bulk-commit reservation of [F16] P-84 and the extent head of [F16] P-97
  (pass 1, P1-3, S1-11, A1-12, P1-8; open points 20, 21).
- `JournalCursor` is reserved: E2 is not built ([AR §11] #41), so format-v1 writers never append it; readers decode it.

## 8. Common payload encodings

### 8.1 The symbol-definition block

A record that uses a symbol not yet defined in `SYMTAB` ([F09]) or earlier in the log defines it in a `SymDefs` block at
the start of its payload and sets `flags` bit 2 ([F01 §8]: "new symbols of the log tail, [F05]").

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `n_defs` | `uvar16` | always | 1 to 65,535 definitions |
| 2 | `defs` | `n_defs` × `SymDef` | always | in order |

`SymDef`:

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `class` | `u8` | always | the symbol class, the enumeration below |
| 2 | `id` | `uvar16` for classes `role` and `root`, `uvar32` otherwise ([F01 §8.1] S3) | always | the new id |
| 3 | `text` | `vstr` | always | the symbol's string; not empty |

| value | class ([F01 §8.2]) |
|---|---|
| 1 | `actor` |
| 2 | `role` |
| 3 | `session` |
| 4 | `ref` |
| 5 | `git-branch` |
| 6 | `git-worktree` |
| 7 | `stmt` |
| 8 | `root` |
| 9 | `reason` |
| 10 | `name` |
| 11 | `text` |

Rules:
- **SD-1.** `id` is exactly the next id of its class: one more than the greatest id of the class defined in `SYMTAB` and in
  every earlier definition of the valid log (ids are dense, [F01 §8.1] S2). Within a block, the definitions of one class
  have consecutive increasing ids.
- **SD-2.** `text` is not already the string of another id of the same class.
- **SD-3.** A payload field refers to a symbol only if it is defined in `SYMTAB`, by an earlier record of the valid log,
  or by this record's own block. Any other reference, and any breach of SD-1 or SD-2, makes the payload malformed (§5.4).
- **SD-4.** Ids are allocated under the writer byte, after the scan ([F16]). A definition in a lost group is lost with
  everything after it, so a later definition may take the same id.

### 8.2 Scalars and short forms

| Name | Encoding | Meaning |
|---|---|---|
| `nodeid` | `uvar32` | a node id `#N`; 0 means none where a table allows it |
| `refid` | `uvar32` | a ref id ([F04 §5.14]) |
| `seqv` | `uvar64` | a commit `seq` |
| `lsnv` | `uvar64` | an lsn |
| `hlc` | `u64` | an `hlc` value ([F01 §5.7]), fixed width ([AR §4.3]) |
| `sym(C)` | `uvar16` or `uvar32` by class C's width ([F01 §8.2]) | a symbol id of class C; 0 is the empty string |
| `cid32` | `b32` | a full commit id ([F07]); all zero = none |
| `cid16` | `b16` | an `id16`, the first 16 bytes of a commit id ([F01 §5.6]); all zero = none |
| `oidv` | `algo u8` then the digest bytes of `algo` ([F01 §7.5], variable-width form) | a content id, git blob id or git commit id ([40] R-1); `algo` 0 = empty |
| `digest(a)` | 20 bytes when `a` = 1 (`sha1`), 32 when `a` = 2 (`sha256`) | a git object id whose algorithm a field of the same payload gives |
| `pathv` | `root u16` (a fixed-width symbol id of class `root`) then `vstr` | a stored path: [40] R-1's "root sym u16 + varint-length UTF-8", exact bytes; the path rules (I-F8) are [F18]'s |
| `fstime` | `i64` then `gran u8` (9 bytes) | a file-system timestamp: nanoseconds since the Unix epoch and a granularity byte, as `FsTime` of [OS/project §3.3] ([80 §2.11.2]) |
| `b16`, `b32`, `vstr`, `vbytes` | [F01 §5.6], [F01 §6.2] | |

### 8.3 Embedded fixed structures

| Name | Width | Owner |
|---|---|---|
| `Stamp` | 24 | [OS/clock §3.1]: the deadline form `{wall, boot_hash, mono}` of X-F2 |
| `Anchor` | 32 | [F03 §10] |
| `ProcId` | 32 | [F03 §5.1] |
| `OsFileId` | 57 | [F11] after [80 §2.11.2]: `{kind u8, vol_key [16], id [16], parent [16], aux u32, docid u32}` |
| `VolumeCaps` | 16 | [F11] after [80 §2.11.2]: `{flags u32, id_kind u8, btime u8, case_rule u8, cloud u8, mtime_granularity_ns u64}` |
| `JournalCursorRow` | 41 | [F11] after [80 §2.11.2]: `{kind u8, vol_key [16], instance [16], cursor u64}` |
| `SegRef` | 29 | [F04 §4.1] |

### 8.4 File references

`FileRefV` names one store file in a record; `FileEntry` also states its length and digest. `FileRefV` is the
variable-width record form of [F11 §2.5]'s 9-byte `FileRef`, which rows use; both encode the same three parts (pass 1,
P1-25, S1-37).

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `family` | `u8` | always | the family: a value of [F11 §2.5]'s `FileFamily` registry (1 `log.<n>` … 9 `cs.<n>`), which is the one owner of the values |
| 2 | `ref_id` | `refid` | `family` = 5 | the `<ref_id>` of `seg.b<ref_id>.<K>` |
| 3 | `file_no` | `uvar32` | always | the file number, ≥ 1 |

`FileEntry` = `FileRefV`, then `total_len uvar64` (the file's `total_len`, [80 §2.5] rule 4) and `digest b16` (the first
16 bytes of the digest its header or footer records, [F09], [F10]).

### 8.5 Row batches

The runtime kinds that carry table rows (`FileObs`, `Pending`, `FPrint`, `JournalCursor`, `DirMap`, `PrefixEv`,
`AnchorRes`) use one layout:

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `n_rows` | `uvar32` | always | ≥ 1 |
| 2 | `rows` | `n_rows` × `vbytes` | always | each row framed by its length, so a replay can index a row's key without decoding its value |

A row is `op u8` (1 = upsert, 2 = delete), then its body. For `FileObs`, `Pending`, `DirMap`, `PrefixEv` and
`AnchorRes` the body is the row image of [F11 §2.9]: the image of the [F11] row for an upsert, the image of its dead row
for a delete; [F11] owns every field and code of it (pass 1, A1-6, S1-8, P1-2). For `FPrint` and `JournalCursor` the
body is the key fields of the kind, then, for an upsert, the value fields (§9.20, §9.21). A row's bytes must be consumed
exactly. Rows apply in order; a later row for the same key replaces an earlier one; a delete row removes the key.

### 8.6 Glob lists

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `n` | `uvar32` | always | the number of globs, 0 allowed |
| 2 | `globs` | `n` × (`root u16`, `pattern vstr`) | always | each a root symbol and a root-relative glob pattern ([F08] glob rules) |

### 8.7 Enumerations owned by other chapters

Some payload fields hold codes that another chapter enumerates. This chapter fixes their width and position only:

| Field | Width | Owner of the values |
|---|---|---|
| link state, detail and evidence token (inside the `FileObs`, `Pending` and `AnchorRes` row images) | `u8` | [F18 §4.10] (R-16, R-17), stored as [F11 §12.5], §12.6 and §12.13 lay them out |
| evidence class (the same row images) | `u8` | [F18 §4.10], numbering [F20 §1.5]'s classes (R-14) |
| pin holder (`Pin.holder`), intent item outcome and abort reason (`FsIntentDone`, `FsIntentAborted`), settle-epoch kind (`TreeReg`) | `u8` | [F11 §4], §12.7, §12.4 |
| file attributes (`FileObs.attrs`) | `u32` | [F11] |
| resolver version | `u16` | [F20] (R-14) |
| `fstime.gran` | `u8` | [OS/project §3.3] |
| the fingerprint bytes (`FPrint.fprint`) | `vbytes` | [F20] and [F10] (R-9) |
| the stored idempotency result (`Idem.result`) | `vbytes` | [API], [F11] |

### 8.8 Payload end

A payload ends exactly after its last field. Remaining bytes make it malformed (§5.4). Every field marked
reserved-zero in §9 is checked when the payload is decoded.

## 9. Payloads

Tables are sequence tables ([F01 §2.6]). Every payload may be preceded by a `SymDefs` block (§8.1) except `Noop`'s and `ExtentHead`'s.
Every bit a table does not name is reserved-zero; every enumeration value not listed is invalid.

### 9.1 `Commit` (1) — durable

The payload is the commit body of [F06]: the header with its presence bitmap, the message, `affected`,
`changeset_digest`, `cs_ref` and the ops. This chapter adds no field. The fold of §10 reads these commit fields, which
[F06] must carry: `seq`, `ref_id`, `append_hlc` ([50] F14), the `#N` each `Create` op allocates, the anchor handle `aN`
each anchor-creating op carries ([40 §2.7], A1P-12), and `cs_ref.file` of a bulk commit. The tail entries of `ALLOC`
([50] F17, `#N → (uid, ref_id, create_seq)`) are derived from the `Create` ops and the header, so `ALLOC` needs no record
kind of its own (open point 15).

### 9.2 `RefUpdate` (2) — durable

A ref move that no commit carries ([AR §5a.2]): branch or tag creation, deletion, `undo`, `op restore` ([AR §4.3]), and
the park of a commit whose ref CAS failed at replay ([F16] P-70, [F12 §8.2]).

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `reason` | `u8` | always | 1 = create (a branch, lane, tag or staging ref; a fork), 2 = delete (`branch -d`/`-D`, `merge --abort` of a staging ref), 3 = `undo`, 4 = `op restore`, 5 = `park`: `orphans/<R>` moves to a commit whose CAS against R failed (I27′); `old` is its previous tip, or zero when this record creates it (its first use), `new` the parked commit (pass 1, P1-3, S1-11, A1-11) |
| 2 | `ref_id` | `refid` | always | the ref that moves |
| 3 | `actor` | `sym(actor)` | always | who moved it |
| 4 | `hlc` | `hlc` | always | the store's HLC at append ([OS/clock §7]); opens the reflog window ([F17 §11.2]) |
| 5 | `old` | `cid32` | always | the tip before the move; zero for a create |
| 6 | `new` | `cid32` | always | the tip after the move; zero for a delete |
| 7 | `n_absorbed` | `uvar32` | `reason` ∈ {1, 3, 4} | the length of the ref's absorbed vector after the move ([AR §5d.1]: fork and undo records carry it) |
| 8 | `absorbed` | `n_absorbed` × (`refid`, `ref_seq uvar32`) | `reason` ∈ {1, 3, 4} | sorted by ref id, each ref id once |
| 9 | `moves_back` | `uvar32` | `reason` = 3 | the N of `undo N` (≥ 1) |
| 10 | `restore_seq` | `seqv` | `reason` = 4 | the `seq` that `op restore` restores |

The `RefTable` record with the ref's new entry follows in the same group (§4.7, §9.10). `op restore` writes one
`RefUpdate` per ref it moves. A `park` carries no absorbed vector: a parked commit is on no live branch view, and
`orphans/<R>` is never merged or absorbed; its `RefTable` entry is a full entry (kind 6 `orphans`) when `old` is zero and
otherwise sets `tip`, `tip_lsn` and `gen` (§9.10).

**Where a `RefUpdate` lies among commit seqs** (`restore_seq`). A `RefUpdate` carries no `seq`; it lies after the newest
commit appended before it. `op restore s` restores every ref to its value just after commit s was appended, so the ref
moves made since commit s, an earlier `op restore` among them, are taken back. Addressing a position between two commits
needs an op-log position, which format v1 does not carry ([API §11.11]).

### 9.3 `ClientHead` (3) — durable

A client head or a directory binding ([AR §5a.4], [40] R-15). The record that sets a head carries the full new `HEADS`
row as its row image ([F11 §5], [F11 §2.9]), so the binding's `BindingExt` travels as [F18 §3.2]'s 40 bytes and replay
rebuilds `HEADS` exactly ([F18 §3.7]; pass 1, S1-10, A1-8, P1-2). A `--replace` writes both affected rows in one
durable group.

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `op` | `u8` | always | 1 = set, 2 = remove |
| 2 | `row` | the image of a [F11 §5] `HEADS` row | `op` = 1 | the row's new value: its `kind` (1 directory, 2 client name, 3 session), `key`, target (`ref_id`, or `detached` with its lsn), the directory's `root_id` and `os`, the `BindingExt`, `hlc` = this record's HLC at append, and `text` (§5.1 of [F11]: the canonical directory, which `doctor store` prints ([F02 §3.7]); the client name; `session:<harness>:<id>` with the identity string of [F03 §9.1]) |
| 3 | `key_kind` | `u8` | `op` = 2 | the removed row's `kind` |
| 4 | `key` | `b16` | `op` = 2 | the removed row's `key` |
| 5 | `hlc` | `hlc` | `op` = 2 | HLC at append |

A session head expires `idempotency.retention` after its `hlc` ([F11 §5.2]).

### 9.4 `Lease` (4) — durable

A lease event ([AR §6.2], [90 §4.3], [90 §10.1]). Routine renewals are `Lazy` heartbeat records (§9.11).

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `event` | `u8` | always | 1 = claim, 2 = release, 3 = set, 4 = renew |
| 2 | `lease_id` | `uvar64` | always | the n of `L-<n>`, ≥ 1; a claim takes `lease_id` = `token` (open point 11) |
| 3 | `token` | `u64` | always | the lease's fencing token; a claim allocates `HEAD.fence + 1` under the writer byte |
| 4 | `hlc` | `hlc` | always | HLC at append |
| 5 | `node` | `nodeid` | `event` = 1 | the task `#N`; 0 for a role lease |
| 6 | `lkind` | `u8` | `event` = 1 | 1 = task, 2 = role |
| 7 | `role` | `sym(role)` | `event` = 1 | the role the lease grants; never 0 (a role-less self-claim is `developer`, [90 §4.3]) |
| 8 | `holder` | `sym(actor)` | `event` = 1 | the holder (the actor rule of [90 §4.1]) |
| 9 | `anchor` | `Anchor` | `event` = 1 | the holder anchor ([F03 §10.3]); kind 0, 1 or 4 |
| 10 | `expires` | `Stamp` | `event` = 1 | the deadline; `Stamp::NEVER` for a run-scoped lease ([OS/clock §4.1]) |
| 11 | `ttl_ms` | `uvar64` | `event` = 1 | the TTL in milliseconds, for the half-TTL renewal rule ([OS/clock §4.4]); 0 for a run-scoped lease |
| 12 | `run` | `nodeid` | `event` = 1 | the run node the lease is scoped to; 0 = none |
| 13 | `branch` | `refid` | `event` = 1 | the branch the claim was made on |
| 14 | `bound` | `b16` | `event` = 1 | the hash of the thread identity the lease is bound to ([90 §4.1]); zero = unbound |
| 15 | `root_session` | `b16` | `event` = 1 | the hash of a Codex holder's root session, for grouping; zero otherwise |
| 16 | `files_owned` | glob list (§8.6) | `event` = 1 | the leased task's `files_owned` globs, captured at the claim ([70 S5]) |
| 17 | `proc` | `ProcId` | `event` = 1 | the claiming process, diagnostics only ([80] X-F2) |
| 18 | `reason` | `u8` | `event` = 2 | 1 = `release`, 2 = `complete` (released into `settled`), 3 = `reclaim`, 4 = dead (its anchor Dead, its deadline passed or its boot changed; ended by the next claim of its task, in that claim's group, [RULES/state-definition] LE-012: a read appends nothing, I-F5; so only a task lease ends with reason 4, and a role lease never does (C, as [F06 §2.4] defines it: it needs the lease's claim record, so it is not a §5.4 check)), 5 = its branch deleted, 6 = `apply`, 7 = `run close`, 8 is not assigned (the `SubagentStop` hook releases through `Release`, reason 1, [API §10.3]; a record with 8 is malformed, §5.4), 9 = `rm --release` |
| 19 | `mask` | `u8` | `event` = 3 | bit 0 `files_owned`, bit 1 `branch` (`--move-lease`), bit 2 `bound` (the binding rule's first use), bit 3 `anchor`; at least one bit |
| 20 | `set_files_owned` | glob list | `event` = 3, `mask` bit 0 | |
| 21 | `set_branch` | `refid` | `event` = 3, `mask` bit 1 | |
| 22 | `set_bound` | `b16` | `event` = 3, `mask` bit 2 | |
| 23 | `set_anchor` | `Anchor` | `event` = 3, `mask` bit 3 | kind 0, 1 or 4 |
| 24 | `renew_expires` | `Stamp` | `event` = 4 | the new deadline (a same-holder renewal of an expired, unreclaimed lease, I17′) |
| 25 | `renew_ttl_ms` | `uvar64` | `event` = 4 | |
| 26 | `renew_anchor` | `Anchor` | `event` = 4 | kind 0, 1 or 4 |
| 27 | `lflags` | `u8` | `event` = 1 | bit 0 `session_role`: the orchestrator's session role lease (`claim --role R --session`, [90 §4.3]), which [F11 §6] `LEASES.flags` bit 1 stores; bits 1–7 reserved-zero (pass 1, A1-6) |

A release, set or renew applies only to the lease with that `lease_id` and `token`; at replay a record that names no
live lease with that pair changes nothing (the writer's fencing check refused it first, [AR §6.2]).

### 9.5 `Marker` (5) — durable

Every change of a marker's holder set or flag ([AR §4.5] step 4, [AR §5d.1], [72 M4]): the entries of
[RULES/state-definition] ME-001 to ME-011, which [F13 §4.2] cites, whichever section holds the row. A marker's identity
is (`#N`, origin ref, origin commit), and its state is a holder set and a `nonlinear` flag ([F11 §7]). Replay cannot
recompute a holder set from net ops — a `sync` stores only its residue, and a fork or a ref move changes holds with no op
at all — so every such change is recorded here, and replay applies the entries in log order ([F11 §7] "Records"; review
pass 1 S1-16). ME-012's move of a row to `MARKERS_OLD` and ME-013's return of one to `MARKERS` are storage moves that
change neither: each is derived from the rows and the absorbed vectors and writes no entry, so the records are the same
whether or not a fold ran (spec sync 2b).

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `n` | `uvar32` | always | ≥ 1 |
| 2 | `markers` | `n` × `MarkerEntry` | always | applied in order |

`MarkerEntry`:

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `mkind` | `u8` | always | 1 = `settled` and 2 = `deleted`: write or re-emit the marker with the holder set of field 12; 3 = `cleared`: its holder set emptied; 4 = `holders`: replace the holder set of an existing marker with field 12; 5 = `nonlinear`: set the marker's `nonlinear` flag. Other values invalid |
| 2 | `node` | `nodeid` | always | `#N` |
| 3 | `ref_id` | `refid` | always | the origin ref of the marker's identity (`#N`, `ref_id`, `commit`) ([RULES/state-definition] MF-004) |
| 4 | `ref_seq` | `uvar32` | always | `ref_seq` of the origin commit on that ref (MF-005) |
| 5 | `commit` | `cid16` | always | the origin commit (MF-003) |
| 6 | `seq` | `seqv` | always | that commit's `seq` |
| 7 | `hlc` | `hlc` | always | HLC at append |
| 8 | `cause` | `u8` | always | the event that wrote the entry: 1 = a commit landing on a work ref (whatever verb, merge, sync, cherry-pick, revert or import produced it), 2 = `undo`, 3 = `op restore`, 4 = a ref deletion (`branch -d` or `-D`), 5 = a fork (a ref created at a commit) |
| 9 | `holder` | `sym(actor)` | `mkind` = 1 | [RULES/state-definition] MF-009: for the entry that the commit of a `complete` writes for the task it settles (ME-001), the holder of the task lease that `complete` presented and released ([API §10.5] step 2; the `holder` of the lease's claim, §9.4 field 8); 0 for every other entry, a re-emit (ME-003, ME-006, ME-007) and a `cancelled` hold included |
| 10 | `status` | `u8` | `mkind` = 1 | 1 = `done`, 2 = `cancelled` |
| 11 | `outcome` | `u8` | `mkind` = 1 | MF-009: for the same entry as field 9, that `complete`'s `--outcome`: 1 = `done`, 2 = `failed`, 3 = `abandoned`; 0 = none for every other entry |
| 12 | `holders` | `uvar32` count, then count × `refid` | `mkind` ∈ {1, 2, 4} | the marker's complete holder set after the entry (MF-006): the live work refs that hold `#N` with this origin, `ref_id` ascending and unique, count ≥ 1 |

### 9.6 `Idem` (6) — durable

A committed result recorded under an idempotency key ([AR §6.4], [F17 §11.1]).

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `key` | `b16` | always | BLAKE3-128 of the key ([AR §6.4]) |
| 2 | `payload` | `b16` | always | BLAKE3-128 of the payload (the canonical bound AST) |
| 3 | `ref_id` | `refid` | always | the branch the result belongs to |
| 4 | `iflags` | `u8` | always | bit 0 `default_key` (the key is the default key, which matches only within `idempotency.default-window`, [F17] OP-17-16); bit 1 `no_commit` (the result records no commit, for example a claim) |
| 5 | `commit` | `cid16` | always | the recorded commit; zero when `no_commit` |
| 6 | `append_hlc` | `hlc` | always | the recorded commit's `append_hlc`, or this record's HLC when `no_commit`; opens the retention windows |
| 7 | `result` | `vbytes` | always | the stored result, encoded as [API] states |

### 9.7 `GitMap` (7) — durable

Image export and import mappings ([AR §5b.6], [AR §4.1] `gitmap` row).

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `dest` | `u8` | always | the destination number, 1 to 255 |
| 2 | `algo` | `u8` | always | the object format ([F01 §7.5]): 1 or 2 |
| 3 | `gflags` | `u8` | always | bit 0 `declare`, bit 1 `last_seen` |
| 4 | `cursor_seq` | `seqv` | always | the greatest `seq` among the commits this record maps; 0 when it maps none |
| 5 | `dest_name` | `vstr` | `gflags` bit 0 | the destination's name (`image.dest.<name>`, [CFG]); `dest` stands for it from this record on |
| 6 | `n` | `uvar32` | always | 0 allowed |
| 7 | `entries` | `n` × (`commit cid16`, `oid digest(algo)`) | always | `(commit_id16, dest, algo, git_oid)` for each exported or imported commit |
| 8 | `n_seen` | `uvar32` | `gflags` bit 1 | |
| 9 | `seen` | `n_seen` × (`refid`, `oid digest(algo)`) | `gflags` bit 1 | the last-seen oid per ref on this destination ([AR §5b.6] import step 1, export step 4 CAS) |

### 9.8 `Pin` (8) — durable

A pin on a checkpoint set ([AR §4.9] "Pins", [AR §5a.3]).

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `op` | `u8` | always | 1 = pin, 2 = unpin |
| 2 | `holder` | `u8` | always | [F11 §4] `PinHolder.kind`: 1 = a branch's fork base, 2 = a promotion's base, 3 = a merge into `main`, 4 = a tag (`--pin`) |
| 3 | `ref_id` | `refid` | always | the ref that holds the pin |
| 4 | `set_lsn` | `lsnv` | always | the checkpoint set: the lsn of the `Checkpoint` record that published it (§9.9) |
| 5 | `n` | `uvar32` | always | the number of files of the set |
| 6 | `files` | `n` × `FileRefV` | always | the set's files, so the fold needs no other record ([F11] `PINS`: file → refcount and holders) |

### 9.9 `Checkpoint` (9) — durable

Every change of the segment set and of the sealed-file registries: a delta checkpoint, a tiered fold, a rollup, a
runtime-only fold, a promotion, a retirement, a GC rewrite ([AR §4.9], [F17 §5]–[F17 §7]). A **checkpoint set** is the
segment set a record with a set change publishes; its id is that record's lsn.

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `ckflags` | `u16` | always | the bit table below; at least one of bits 0, 3, 5, 6 |
| 2 | `append_hlc` | `hlc` | always | the store's HLC at append; opens the deletion grace of every file the record releases ([F17 §11.4]) |
| 3 | `next_file_no` | `uvar32` | always | the sealed-file allocator after this record: greater than every number this record names ([F04 §5.13]) |
| 4 | `upto_lsn` | `lsnv` | bit 0 | the new `checkpoint_lsn`: records below it are folded. It equals the greater of `HEAD.epoch_lsn` and the greatest `upto_lsn` of the base and delta entries of `segments` ([F04 §5.4]), and it never decreases |
| 5 | `active_log` | `uvar32` | bit 0 | the new `HEAD.active_log` (§2.5) |
| 6 | `n_segments` | `u8` | bit 0 | 0 to 8 |
| 7 | `segments` | `n_segments` × `SegRef` | bit 0 | the complete new segment set, in [F04 §4.1]'s order |
| 8 | `window_start` | `lsnv` | bit 1 | the previous `checkpoint_lsn`: the window is `[window_start, upto_lsn)` |
| 9 | `n_refs` | `uvar32` | bit 1 | |
| 10 | `ref_lists` | `n_refs` × `RefList` | bit 1 | the per-ref lsn lists of the window (G15, [50] F9), sorted by ref id |
| 11 | `rt_upto_lsn` | `lsnv` | bit 2 | a runtime-only fold: the `K_RT` records below it are in the new set's runtime sections (§10.4) |
| 12 | `n_promotions` | `uvar32` | bit 3 | ≥ 1 |
| 13 | `promotions` | `n_promotions` × `Promotion` | bit 3 | sorted by ref id |
| 14 | `n_retirements` | `uvar32` | bit 4 | ≥ 1 |
| 15 | `retirements` | `n_retirements` × `Retirement` | bit 4 | increasing extent numbers, starting at the current `active_log` |
| 16 | `n_added` | `uvar32` | bit 5 | ≥ 1 |
| 17 | `added` | `n_added` × `FileEntry` | bit 5 | sealed files this record names that are not in `segments`, `promotions` or `retirements`: the checkpoint's `blobs` file, `gitmap` pages, `hist` files that replace others |
| 18 | `n_released` | `uvar32` | bit 6 | ≥ 1 |
| 19 | `released` | `n_released` × `FileRefV` | bit 6 | every file this record stops naming: former set members, folded `cs` files, replaced `hist` and `gitmap` files; retired extents are released by their retirement entry and not listed again |

| bit | name | meaning |
|---|---|---|
| 0 | `set_change` | the segment set changes (fields 4–7) |
| 1 | `window` | per-ref lists present; set exactly when bit 0 is set and `upto_lsn` > `window_start` |
| 2 | `runtime_fold` | a runtime-only fold ([F17 §5.4]); requires bit 0 and `rt_upto_lsn` > `upto_lsn` |
| 3 | `promotions` | fields 12–13 |
| 4 | `retirements` | fields 14–15; requires bit 0 |
| 5 | `files_added` | fields 16–17 |
| 6 | `files_released` | fields 18–19 |
| 7 | `fts_tier2` | the publish that covers the record sets `HEAD.flags` bit 1 ([F17 §6.4]) |
| 8 | `quiet_bounded` | the one bounded checkpoint of quiet mode ([F17 §5.3]); informational |
| 9 | `rollup` | the set's base is new, from a rollup; informational |

Bits 10–15 are reserved-zero.

`RefList`:

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `ref_id` | `refid` | always | the ref the commits land on |
| 2 | `n` | `uvar32` | always | ≥ 1 |
| 3 | `entries` | `n` × (`dlsn uvar64`, `dhlc uvar64`) | always | entry j's lsn = (j = 0 ? `window_start` : lsn j−1) + `dlsn`; its `append_hlc` = (j = 0 ? 0 : hlc j−1) + `dhlc`. Each lsn is the lsn of a `Commit` record with that `ref_id` in the window; the lsns increase strictly and every such commit appears once |

`Promotion` ([AR §5a.3], [F17 §7]):

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `ref_id` | `refid` | always | the promoted ref |
| 2 | `seg_file` | `uvar32` | always | K of the new `seg.b<ref_id>.<K>` |
| 3 | `total_len` | `uvar64` | always | its `total_len` |
| 4 | `digest` | `b16` | always | the first 16 bytes of its recorded digest ([F09]) |
| 5 | `base_pin` | `lsnv` | always | the ref's new base pin, a checkpoint set id; 0 = the set this record publishes (then bit 0 is set) |
| 6 | `tip_lsn` | `lsnv` | always | the lsn of the ref's tip commit that the segment includes |
| 7 | `tip_ref_seq` | `uvar32` | always | that commit's `ref_seq` |

A promotion resets the ref's `overlay_ops` and `overlay_bytes` for the commits up to `tip_lsn` ([F17 §7]); the ref's
entry is updated at replay (§10.3).

`Retirement`:

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `extent` | `uvar32` | always | the retired `log.<extent>` |
| 2 | `hist_file` | `uvar32` | always | the `hist.<hist_file>` that holds its history ([F10]) |
| 3 | `total_len` | `uvar64` | always | the `hist` file's `total_len` |
| 4 | `digest` | `b16` | always | its recorded digest |

A record with only bit 3 set is a standalone promotion, the "promotion record" of [80 §2.4.3]. A record with bit 0 whose
`upto_lsn` equals the current `checkpoint_lsn` and without bit 2 changes the set without folding log records (a GC or
rollup rewrite).

### 9.10 `RefTable` (10) — durable

Ref-table entries ([AR §4.2]). A `RefTable` record follows every `RefUpdate` in its group with the new entry of each ref
the group creates, deletes or moves; `gc` writes one to remove an expired deleted ref's entry. Commits do not write
`RefTable` records: a commit's effect on its ref's entry (tip, `ref_seq_next`, counters, absorbed vector) is derived at
replay (§10.3). An upsert entry is a **partial upsert** ([F11 §3.9]; pass 1, A1-6, S1-8, P1-2): the fold takes from it
only the fields that its group's `RefUpdate` reason writes — every field for a create (reason 1) and for a `park` that
creates its orphans ref (reason 5 with `old` zero), `eflags` bit 0 for a delete (2), `tip`, `tip_lsn`, `gen` and the
absorbed vector for an `undo` or `op restore` (3, 4), `tip`, `tip_lsn` and `gen` for any other `park` (5) — and the writer copies
the ref's current values into the others, which the fold ignores. So a ref move never resets the counters that commits
and promotions maintain ([F17 §7]).

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `n` | `uvar32` | always | ≥ 1 |
| 2 | `entries` | `n` × `RefEntry` | always | |

`RefEntry`:

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `op` | `u8` | always | 1 = upsert, 2 = remove (after the reflog window of a deleted ref) |
| 2 | `ref_id` | `refid` | always | |
| 3 | `name` | `sym(ref)` | `op` = 1 | the ref name |
| 4 | `rkind` | `u8` | `op` = 1 | 1 = `work`, 2 = `plan`, 3 = `merge`, 4 = `import`, 5 = `tag`, 6 = `orphans` |
| 5 | `eflags` | `u8` | `op` = 1 | bit 0 `deleted` (the ref is deleted; its entry stays for the reflog window, [AR §5a.9]), bit 1 `has_message`, bit 2 `pinned` (a tag created with `--pin`, [F11 §3.1] `flags` bit 1) |
| 6 | `tip` | `cid32` | `op` = 1 | the tip commit; zero for an empty ref |
| 7 | `tip_lsn` | `lsnv` | `op` = 1 | the tip's lsn; 0 when `tip` is zero |
| 8 | `base_pin` | `lsnv` | `op` = 1 | the checkpoint set the ref's view starts from; 0 = `main`'s current set |
| 9 | `fork_commit` | `cid32` | `op` = 1 | zero for a ref without a fork |
| 10 | `fork_seq` | `seqv` | `op` = 1 | |
| 11 | `ops_since_fork` | `uvar64` | `op` = 1 | |
| 12 | `overlay_ops` | `uvar32` | `op` = 1 | ([AR §4.2], [F17 §7]) |
| 13 | `overlay_bytes` | `uvar32` | `op` = 1 | |
| 14 | `ref_seq_next` | `uvar32` | `op` = 1 | never decreases, even at `undo` (CM1) |
| 15 | `promoted_seg` | `uvar32` | `op` = 1 | K of `seg.b<ref_id>.<K>`; 0 = not promoted |
| 16 | `gen` | `uvar32` | `op` = 1 | the tip's generation; 0 for an empty ref |
| 17 | `n_absorbed` | `uvar32` | `op` = 1 | |
| 18 | `absorbed` | `n_absorbed` × (`refid`, `ref_seq uvar32`) | `op` = 1 | sorted by ref id, each once |
| 19 | `message` | `vstr` | `eflags` bit 1 | a tag's message |
| 20 | `fork_lsn` | `lsnv` | `op` = 1 | the lsn of `fork_commit`'s record; 0 without a fork |
| 21 | `fork_ref_id` | `refid` | `op` = 1 | the ref whose chain holds the fork commit; 0 without a fork |
| 22 | `trunk_mark_ops` | `uvar64` | `op` = 1 | [F11 §3.6] |
| 23 | `trunk_mark_bytes` | `uvar64` | `op` = 1 | [F11 §3.6] |

The entry's fields map one to one onto the [F11 §3.1] row; `ops_total` and `bytes_total` are 0 at a create and are
never carried, and `moves` is maintained by replay from every record that moves the ref ([F11 §3.5]).

### 9.11 `Lazy` (11) — configurable

Small runtime updates whose loss is tolerable: lease heartbeats, change-feed cursors and pack cursors ([AR §6.2],
[AR §6.3], [AR §7.4] C8, [70 S12]; [PLAN §3.3] gap).

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `sub` | `u8` | always | 1 = `heartbeat`, 2 = `cursor` |
| 2 | `lease_id` | `uvar64` | `sub` = 1 | |
| 3 | `token` | `u64` | `sub` = 1 | the lease's token; a heartbeat for another token changes nothing |
| 4 | `expires` | `Stamp` | `sub` = 1 | the renewed deadline ([OS/clock §4.4]) |
| 5 | `cause` | `u8` | `sub` = 1 | 1 = `moirai heartbeat`, 2 = a write that presented the lease, 3 = a Codex thread server serving its thread (`session-ttl`, [90 §4.4]) |
| 6 | `session_hash` | `b16` | `sub` = 2 | the session hash of [F03 §9.2] |
| 7 | `agent_hash` | `b16` | `sub` = 2 | BLAKE3-128 of the namespaced agent identity (`claude:<agent_id>`, `codex:<thread>`, [90 §4.1]); zero for the session itself |
| 8 | `feed` | `u8` | `sub` = 2 | 1 = the change feed ([AR §6.3]); 2 = the pack cursor of [AR §7.4] C8 ([RULES/pack-classes] PX-011); other values invalid |
| 9 | `task` | `nodeid` | `sub` = 2 and `feed` = 2 | the `#N` of the node T the pack was built for ([RULES/pack-classes] PT-001: the pack's target, of any kind, usually a task); never 0 |
| 10 | `cursor_seq` | `seqv` | `sub` = 2 | `feed` 1: the position the session (or agent) has seen; `feed` 2: the pack's `rev`, the `seq` of the commit at tip(B) that the pack read ([RULES/pack-classes] PT-032) |
| 11 | `hlc` | `hlc` | always | HLC at append |

Bit 0 of the header follows `durability.lazy-kinds` for `heartbeat` or `cursor` (§6.1); a pack cursor is a `cursor`.
A heartbeat updates the lease's deadline in `LEASES`. A cursor replaces the row of its key (session, agent, `feed`,
`task`) in [F11 §13.1] `CURSORS`, `task` taken as 0 for `feed` 1; the row's fields take the record's. A `feed` 2 record
is the cursor of a pack of T for agent A, and cursor(A, T) of [RULES/pack-classes] PT-028 is the `cursor_seq` of the row
(session, A, 2, `#N` of T) of the session the pack runs in ([F11 §13.1]; pass 1, closure NC-10; open point 14). The
layer that delivers the pack appends the `feed` 2 record after delivery: the `SubagentStart` hook or the MCP server, as
[AR §5d.1] stores the per-session cursors (OQ-F-3 decided 2026-09-28, option (b)). The `pack` verb never appends one; it
stays a read that appends nothing (I-F5, [F18 §2.5]; pass 1, closure NC-11). No M0 command is a delivering layer
([API] open point 48, [RULES/pack-classes] PX-011), so at M0 cursor(A, T) is absent and C8 is empty. A `feed` 1 record
carries no `task`, so its bytes are those of the record before pass 1.

### 9.12 `Noop` (12) — lazy

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `zeros` | `len − 32 − t` zero bytes | always | every payload byte is zero; the length may be 0 |

A `Noop` carries no `SymDefs` block and changes nothing. It forms the rotation pad (§4.4); since pass 1 (P1-8) the
epoch-start group is an extent head (§4.5), not a `Noop`.

### 9.13 `Backup` (13) — durable

Written by `backup` after every copied file and the backup directory are durable ([72 M9], [AR §4.10]).

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `dir` | `vstr` | always | the backup directory, absolute, in the machine-local form of [80 §2.10] P12 |
| 2 | `committed_lsn` | `lsnv` | always | the `committed_lsn` of the `HEAD` state the backup copied |
| 3 | `digest` | `b32` | always | BLAKE3-256 of the backup's manifest ([F16] defines the manifest) |
| 4 | `hlc` | `hlc` | always | HLC at append; the backup age of `backup.max-age` ([CFG]) is measured from it |

### 9.14 `SessionMark` (14) — configurable

The rules a `SubagentStart` hook showed an agent, so that `pack` renders them as one line ([73 F4]).

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `session_hash` | `b16` | always | [F03 §9.2] |
| 2 | `agent_hash` | `b16` | always | as in §9.11 |
| 3 | `rev` | `seqv` | always | the `commit_seq` at which the rules were shown |
| 4 | `hlc` | `hlc` | always | HLC at append |
| 5 | `n` | `uvar32` | always | |
| 6 | `rules` | `n` × `nodeid` | always | the rule nodes shown, in the order shown |

### 9.15 `FsIntent` (15) — durable

The intent of `file mv` or `file rm` ([40 §3.4], [40 §3.5], [40 §2.6] `FSINTENT`). The **intent id** is the lsn of this
record (closing [F02] open point 7); `file rm --trash` moves items to `trash/<intent id>/<i>` ([F02 §5.4]).

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `op` | `u8` | always | 1 = `mv`, 2 = `rm`, 3 = `rm --trash` |
| 2 | `iflags` | `u8` | always | bit 0 `git` (`--git` delegates the move to `git mv`), bit 1 `recursive` (`rm --recursive`) |
| 3 | `branch` | `refid` | always | the caller's branch |
| 4 | `tree` | `b16` | always | the tree key ([40 §2.6] `TREES`) of the writer tree |
| 5 | `anchor` | `Anchor` | always | the intent anchor, kind 2 ([F03 §10.3]) |
| 6 | `proc` | `ProcId` | always | the CLI, diagnostics only ([80 §2.11.2] "`FSINTENT` holder") |
| 7 | `hlc` | `hlc` | always | HLC at append |
| 8 | `n` | `uvar32` | always | ≥ 1 |
| 9 | `items` | `n` × `IntentItem` | always | in command-line order; item i is `<i>` of `trash/<intent>/<i>` |

`IntentItem` ([F11 §12.7]'s item without its `outcome`, which `FsIntentDone` supplies; the `FSINTENT` row keeps
`iflags` bits 0–1 as its `flags` bits 0–1):

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `itflags` | `u8` | always | bit 0 `dir` (the item is a directory: no `oid`) |
| 2 | `src` | `pathv` | always | the source path |
| 3 | `dst` | `pathv` | `op` = 1 | the destination path |
| 4 | `oid` | `oidv` | `itflags` bit 0 = 0 | the source file's content `oid` at planning ([40 §2.5]) |

There is no copy path: a move across volumes is refused before the intent, or aborts it (§9.17 reason 2; [40 §3.4],
A1P-01).

### 9.16 `FsIntentDone` (16) — durable

Written in the same group as the commit that records the move or removal (§4.7).

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `intent_lsn` | `lsnv` | always | the intent id |
| 2 | `dflags` | `u8` | always | bit 0 `recovered` (written by intent recovery: provenance `explicit/intent-recovered`, [40 §3.4] step 5) |
| 3 | `hlc` | `hlc` | always | HLC at append; opens `gc.trash-expire` ([F17 §11.3]) |
| 4 | `n` | `uvar32` | always | equal to the intent's item count |
| 5 | `outcomes` | `n` × `u8` | always | per item, the item outcome of [F11 §12.7]: 1 = done, 2 = failed: busy (sharing violation after the retries), 3 = failed: destination exists, 4 = failed: source missing, 5 = failed: other. A command with a failed item exits 8 ([40 §3.4]) |

### 9.17 `FsIntentAborted` (17) — durable

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `intent_lsn` | `lsnv` | always | the intent id |
| 2 | `reason` | `u8` | always | the abort reason of [F11 §12.7]: 1 = not renamed (the source is present, the destination absent), 2 = cross-volume (`ERROR_NOT_SAME_DEVICE` or `EXDEV` at the rename), 3 = every item failed before a rename, 4 = ambiguous (both paths present with different file ids), 5 = missing (neither path present) |
| 3 | `aflags` | `u8` | always | bit 0 `recovered` (written by intent recovery) |
| 4 | `hlc` | `hlc` | always | HLC at append |

Reasons 4 and 5 close an intent whose outcome recovery could not decide; `brief` and `doctor` show them ([40 §3.4]
step 5; open point 10).

### 9.18 `FileObs` (18) — lazy, `K_RT`

`FILEOBS` rows ([40 §2.6], R-18, [80 §2.11.2]). Row batch (§8.5): each row is the image of a [F11 §12.5] `FILEOBS` row
([F11 §2.9]) — the stat quadruple with the tagged file id and the parent-directory id, `verified_at`, `missing_since`,
`last_oid`, the recorded state with its detail parts ([F18 §4.2], §4.6), the moved-auto target, the proposals as
[F18 §4.10]'s tuple (class, evidence token, path with its root, exact score) and the resolver version. The earlier
payload of this section (a `u16` score in 1/10,000, a `state_path` field, [F18]-less evidence classes) is replaced by the
row image (pass 1, A1-6, A1-7, S1-8, S1-9, P1-2).

### 9.19 `Pending` (19) — lazy, `K_RT`

`PENDING` rows ([40 §2.6]). Row batch (§8.5): each row is the image of a [F11 §12.6] `PENDING` row ([F11 §2.9]):
the key (`#F`, tree, `from` and `to` as paths with their roots), the evidence class and the evidence token ([F18 §4.10],
§5.2), the source (1 = an evidence hook, 2 = a reader-tree settle), the captured `oid` and creation time, the observing
tree's git HEAD, and the `hlc` that opens `files.pending-escalate` and the 30-day retention (pass 1, A1-6, S1-8, P1-2).

### 9.20 `FPrint` (20) — lazy, `K_RT`

`FPRINT` rows ([40 §2.5], [40 §2.6], R-9). Row batch. Key: `oid oidv`. Value: `fprint vbytes`, the fingerprint of one
content version ([F20]); the checkpoint seals it into a fingerprint blob ([F10]).

### 9.21 `JournalCursor` (21) — lazy, `K_RT`, reserved

`JOURNALCUR` rows ([80 §2.11.2]). Row batch. Upsert: the 41-byte `JournalCursorRow`, whose `vol_key` is the key. Delete:
`vol_key b16`. Format-v1 writers never append this kind (§7).

### 9.22 `DirMap` (22) — lazy, `K_RT`

`DIRMAP` rows ([80 §2.11.2], [80 §2.11.3]). Row batch (§8.5): each row is the image of a [F11 §12.10] `DIRMAP` row
([F11 §2.9]): the key (tree, the directory's file id), the directory's mtime as enumerated and its root-relative path
(empty for the root).

### 9.23 `TreeReg` (23) — lazy, `K_RT`

Tree registrations, settle epochs and dirty rows of `TREES` ([40 §2.6], [70 S5], [70 S8], A1P-04).

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `sub` | `u8` | always | 1 = register or update, 2 = settle epoch, 3 = dirty row, 4 = forget the tree |
| 2 | `tree` | `b16` | always | the tree key |
| 3 | `os` | `u8` | `sub` = 1 | the OS tag of the process that registered it ([F01 §3.2]) |
| 4 | `tflags` | `u8` | `sub` = 1 | bit 0 `first_settle_done`, bit 1 `cloud_root`, bit 3 `git` (the tree is a git worktree top-level): [F11 §12.4] `TREES.flags` bits 0, 1 and 3; bit 2 (`dirty_present`), which the fold sets from `sub` = 3, and bits 4–7 are reserved-zero |
| 5 | `root` | `vstr` | `sub` = 1 | the canonical root path ([80] P9), machine-local form |
| 6 | `root_id` | `OsFileId` | `sub` = 1 | the root directory's file id |
| 7 | `caps` | `VolumeCaps` | `sub` = 1 | the volume capability snapshot |
| 8 | `last_head` | `oidv` | `sub` = 1 | the last git HEAD seen; `algo` 0 without git |
| 9 | `last_settle` | `hlc` | `sub` = 1 | |
| 10 | `n_sens` | `uvar32` | `sub` = 1 | |
| 11 | `sens` | `n_sens` × [F11 §12.4] `SensEntry` (`equiv u8`, `path vstr`) | `sub` = 1 | the case- and normalization-sensitivity map: per root-relative directory whose equivalence differs from the volume default, `equiv` bit 0 `case_insensitive`, bit 1 `norm_insensitive` |
| 12 | `epoch` | [F11 §12.4] `Epoch` (32 B) | `sub` = 2 | below |
| 13 | `count` | `u32` | `sub` = 3 | the dirty count |
| 14 | `head` | `oidv` | `sub` = 3 | the git HEAD the count was taken at |
| 15 | `dirty_hlc` | `hlc` | `sub` = 3 | when it was taken |

`epoch` is the 32-byte settle epoch of [40 §2.6] as [F11 §12.4] lays it out (`scope_kind` 0 = full-tree, 1 = lane-owned,
2 = partial; `scope_ref`; the scope digest [F11 §12.4] defines; the settle's HLC); with the tree key before it the epoch
payload is 48 bytes, as [AR §4.3] states. Only the newest epoch per (tree, `scope_kind`, `scope_ref`) is kept; a
checkpoint folds the older ones away ([40 §2.6]). A tree's journal cursor is found by its root's volume key and is not
carried ([F11] open point 17). Which `TREES` field each `sub` writes is [F11 §12.4]'s (pass 1, A1-6, S1-8).

### 9.24 `PrefixEv` (24) — lazy, `K_RT`

`PREFIXEV` rows ([40 §2.6], [40 §4.6]). Row batch (§8.5): each row is the image of a [F11 §12.11] `PREFIXEV` row
([F11 §2.9]): the key (tree, root symbol, the directory prefixes moved from and to), the linked nodes re-bound exactly
so far and those remaining, and the first contributing settle's HLC.

### 9.25 `GitFacts` (25) — lazy, `K_RT`

Cached pure functions of git objects ([40 §2.6] `GITFACTS`), including [AR]'s lazy `ANCESTRY` facts, which `moirai check`
and `stale` append ([AR §2.14]; [PLAN §3.3] gap). Facts never change and are never deleted by a record; `gc` drops them
([40 §2.6]).

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `n` | `uvar32` | always | ≥ 1 |
| 2 | `facts` | `n` × `vbytes` | always | each a `Fact` |

`Fact`:

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `ftype` | `u8` | always | 1 = ancestry, 2 = renames, 3 = commit time |
| 2 | `algo` | `u8` | always | 1 or 2 |
| 3 | `a` | `digest(algo)` | always | ancestry: the candidate ancestor; renames and commit time: the commit |
| 4 | `b` | `digest(algo)` | `ftype` ∈ {1, 2} | ancestry: the descendant; renames: the parent the renames are relative to |
| 5 | `answer` | `u8` | `ftype` = 1 | 0 = not an ancestor, 1 = an ancestor (`ANCESTRY` `(sha32, sha32) → bool`, [AR §4.4]) |
| 6 | `n_pairs` | `uvar32` | `ftype` = 2 | |
| 7 | `pairs` | `n_pairs` × (`from vbytes`, `to vbytes`) | `ftype` = 2 | exact renames; git paths are bytes |
| 8 | `n_groups` | `uvar32` | `ftype` = 2 | |
| 9 | `groups` | `n_groups` × (`blob digest(algo)`, `n_from uvar32`, `n_from` × `vbytes`, `n_to uvar32`, `n_to` × `vbytes`) | `ftype` = 2 | ambiguous identical-blob groups, never paired by order |
| 10 | `committer_time` | `svar64` | `ftype` = 3 | seconds since the Unix epoch |
| 11 | `author_time` | `svar64` | `ftype` = 3 | seconds since the Unix epoch |

The key of a fact is (`ftype`, `algo`, `a`, `b`) for types 1 and 2 and (`ftype`, `algo`, `a`) for type 3.

### 9.26 `AnchorRes` (26) — lazy, `K_RT`

`ANCHORRES` rows ([40 §2.6], [70 S7]). Row batch (§8.5): each row is the image of a [F11 §12.13] `ANCHORRES` row
([F11 §2.9]): the key (anchor uid, the content's `oid`, the resolver version), the anchor state and its detail as
[F18 §4.3] and §4.6 codes, the resolved span's lines and the fuzzy score as an exact rational (pass 1, A1-6, S1-9,
P1-2).

### 9.27 `Reserve` (27) — durable

The reservation a bulk producer appends before it streams its `cs.<n>` ([F16] P-84; pass 1, P1-3, S1-11, A1-12). It
allocates, under the writer byte and from the scanned log ([F16] P-31), every id the file will carry, so the file is
streamed with final ids and the bulk `Commit` needs no id of its own. New symbols the file uses are defined by this
record's `SymDefs` block (§8.1), which allocates their ids by SD-1 and SD-4.

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `ref_id` | `refid` | always | the ref the bulk commit will land on |
| 2 | `hlc` | `hlc` | always | HLC at append, carried without advancing the store's HLC sequence ([F16] P-36); opens `gc.cruft-delay` for a reservation whose commit never lands ([F16] P-84) |
| 3 | `cs_file` | `uvar32` | always | the `n` of the `cs.<n>` the producer streams; ≥ 1 |
| 4 | `blobs_file` | `uvar32` | always | the `n` of the `blobs.<n>` that takes its bodies; 0 = none |
| 5 | `first_id` | `nodeid` | always | the first reserved `#N`; 0 when `n_ids` = 0 |
| 6 | `n_ids` | `uvar32` | always | the number of consecutive `#N`s reserved from `first_id`, for uids the store does not know yet (a known uid keeps its `#N`, I1) |
| 7 | `first_anchor` | `uvar32` | always | the first reserved anchor handle `aN`; 0 when `n_anchors` = 0 |
| 8 | `n_anchors` | `uvar32` | always | the number of consecutive `aN`s reserved from `first_anchor` |
| 9 | `n_schema` | `uvar32` | always | 0 allowed |
| 10 | `schema` | `n_schema` × `SchemaRes` | always | the store-local ids ([F08 §8.3]) the file gives to schema items the store has never numbered |

`SchemaRes`:

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `class` | `u8` | always | [F08 §8.5]'s item class: 1 kind, 3 enumeration value, 4 edge kind; other values invalid |
| 2 | `key` | one `vstr` for classes 1 and 4; three `vstr` (kind name or `*`, field name, value name) for class 3 | always | the item key of [F08 §8.5], as name strings |
| 3 | `id` | `uvar16` | always | the reserved id, inside its space's range ([F08 §8.3]); the smallest the store has never given |

Rules:
- The ranges are fresh: `first_id` ≥ the `next_id` and `first_anchor` ≥ the `next_anchor` that the scanned log implies
  before the record, and each `SchemaRes` names a key the store has never numbered with an id it has never given.
- The record claims `cs.<n>` and `blobs.<n>` by [F16] P-78's first form.
- Ids of a reservation whose bulk `Commit` never lands are skipped, never reused ([F09 §16.4] "Ids", [F11 §9.1]); its files
  are released as [F16] P-84 states.

### 9.28 `ExtentHead` (28) — durable

The first record of every extent, alone in its group (§4.5; [F16] P-97; pass 1, P1-8). Its payload has a fixed length of
98 bytes, so the group is always H = 32 + 98 + 8 = 138 bytes.

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `epoch_lsn` | `u64` | always | `HEAD.epoch_lsn` of the epoch the record belongs to ([F04 §5.3]) |
| 2 | `chain_in` | `u64` | always | the chain value at the record's own lsn (§4.3): `XXH3-64(epoch)` when its lsn is `epoch_lsn`, else the trailer that ends the previous extent |
| 3 | `init` | `InitParams` (32 B) | always | `HEAD.init` ([F04 §4.4]) |
| 4 | `project_oid_algo` | `u8` | always | `HEAD.project_oid_algo` ([F04 §5.16]) |
| 5 | `hflags` | `u8` | always | bit 0 `quiet`, bit 1 `readonly`: `HEAD.flags` bits 0 and 2 of the newest slot the writer read; bits 2–7 reserved-zero |
| 6 | `commit_seq` | `u64` | always | the value [F16] P-31 derives before this record: the `seq` of the newest commit in the log before it |
| 7 | `next_id` | `u32` | always | as field 6, for `#N` |
| 8 | `next_anchor` | `u32` | always | as field 6, for `aN` |
| 9 | `fence` | `u64` | always | as field 6, for the fencing token |
| 10 | `next_file_no` | `u32` | always | as field 6, for sealed-file numbers |
| 11 | `next_ref_id` | `u32` | always | as field 6, for ref ids |
| 12 | `hlc_seq` | `hlc` | always | the greatest value of the store's HLC sequence before this record ([F16] P-36, [F04 §5.15]) |
| 13 | `hlc_commit` | `hlc` | always | the greatest `hlc` of any commit the store holds before this record ([F04 §5.15]) |

Rules:
- A record of this kind at an lsn that is not an extent's first byte, or whose `chain_in` differs from the chain value at
  its lsn, or whose `epoch_lsn`, `init` or `project_oid_algo` differ from those of the slot the scanning process uses, is
  malformed (§5.4).
- Fields 6–13 are the values of the log before the record; the groups after it raise them. They give a `repair` that has
  no valid `HEAD` slot the base its fold starts from ([F16] P-85): at the head of the lowest surviving extent they cover
  every record of the retired extents before it. After an epoch re-roll the epoch-start head carries the values of the
  restored or repaired store, so the HLC sequence never restarts ([API §6.2] CK-6).

## 10. Replay and the effects on `HEAD`

### 10.1 Order

Groups apply in log order, records in group order, rows and entries in record order. A group applies all or nothing
(§4.1).

### 10.2 The `HEAD` fold

A publish folds every group between the selected slot's `durable_lsn` and the new `committed_lsn`, in log order, into the
slot it writes ([F04 §9.1], [F16] P-50, [80 §2.4.3]); a group the selected slot already folded changes nothing, since every
field below takes the maximum or ends at the newest covered record. Recovery applies the same fold to the slot it
selected, over the groups it scanned. Each record contributes:

| Kind | Effect on the slot |
|---|---|
| `Commit` | `commit_seq` ← max(`commit_seq`, `seq`); `next_id` ← max(`next_id`, 1 + every `#N` its `Create` ops allocate); `next_anchor` ← max(`next_anchor`, 1 + every `aN` its ops carry); `seq_ring[seq mod 32]` ← (`seq`, the record's lsn); `next_file_no` ← max(`next_file_no`, 1 + `cs_ref.file`) for a bulk commit; `hlc_seq` ← max(`hlc_seq`, `append_hlc`); `hlc_commit` ← max(`hlc_commit`, `hlc`) |
| `RefUpdate` | `next_ref_id` ← max(`next_ref_id`, 1 + `ref_id`) when `reason` ∈ {1, 5}; `hlc_seq` ← max(`hlc_seq`, `hlc`) |
| `ClientHead` | `heads_lsn` ← the record's lsn; `hlc_seq` ← max(`hlc_seq`, its HLC) |
| `Lease` | `fence` ← max(`fence`, `token`); `hlc_seq` ← max(`hlc_seq`, `hlc`) |
| `Marker` | `markers_lsn` ← the record's lsn; `hlc_seq` ← max(`hlc_seq`, every entry's `hlc`) |
| `Idem` | `hlc_seq` ← max(`hlc_seq`, `append_hlc`) |
| `Backup`, `FsIntent`, `FsIntentDone`, `FsIntentAborted` | `hlc_seq` ← max(`hlc_seq`, `hlc`) |
| `GitMap` | `image_cursor` by [F04 §5.11] with (`dest`, `algo`, `cursor_seq`) |
| `Pin` | `pins_lsn` ← the record's lsn |
| `Checkpoint` | bit 0: `segments`, `n_segments`, `checkpoint_lsn` ← `upto_lsn`, `active_log`; bit 7: `flags` bit 1 ← 1; `next_file_no` ← max(`next_file_no`, the record's `next_file_no`, 1 + every file number it names) |
| `RefTable` | `refs_lsn` ← the record's lsn; `next_ref_id` ← max(`next_ref_id`, 1 + every upserted `ref_id`) |
| `Reserve` | `next_id` ← max(`next_id`, `first_id` + `n_ids`); `next_anchor` ← max(`next_anchor`, `first_anchor` + `n_anchors`); `next_file_no` ← max(`next_file_no`, 1 + `cs_file`, 1 + `blobs_file`) |
| `ExtentHead` | `commit_seq`, `next_id`, `next_anchor`, `fence`, `next_file_no`, `next_ref_id`, `hlc_seq` and `hlc_commit` each ← max(the slot's value, the record's field) |
| every other kind | none |

Counters and pointers therefore never decrease (I-G6). Only durable kinds affect `HEAD`, so no covered effect can be
lost with a lazy tail. The records that raise `hlc_seq` are exactly the semantic durable records of the store's HLC
sequence ([F16] P-36, [API §6.2] CK-4); `Checkpoint.append_hlc`, `Reserve.hlc`, `Lazy.hlc`, `SessionMark.hlc` and the HLC
values of runtime rows carry a value without raising it (pass 1, S1-13, P1-5, A1-17).

### 10.3 Table effects

Replay applies each record to the runtime tables it folds into (§7), as their owning chapters define ([F11], [F09]); in
particular:

- a `Commit` moves its ref's tip, `tip_lsn`, `gen`, `ref_seq_next`, `ops_since_fork`, `overlay_ops` and `overlay_bytes`,
  and for a merge or sync commit its absorbed vector, CAS-checked against `ref_old` ([AR §5a.2]; a failed CAS parks the
  commit on `orphans/<ref>`, I27′); it adds its per-ref index entry and its `ALLOC` entries. Its marker changes are the
  `Marker` entries of its group (§9.5), which a group adoption takes all or nothing with the commit ([72 M1] fix 2): the
  `MARKERS` fold applies those entries and derives nothing from net ops, because a holder set cannot be recomputed from
  them ([RULES/state-definition] ME rows); the reference model recomputes the cache from its own rules and GT2 compares
  ([60 §4.4] item 5);
- a `RefTable` upsert entry writes the fields its group's `RefUpdate` reason sets and a remove entry drops the row
  ([F11 §3.9], §9.10); every `Commit` and `RefUpdate` naming a ref prepends its lsn to the ref's `moves` ([F11 §3.5]);
- a `Checkpoint` promotion sets the ref's `base_pin` and `promoted_seg` and resets its overlay counters;
- `Lease`, `Lazy` heartbeats, `Idem`, `Pin`, `ClientHead`, `GitMap`, `FsIntent*`, `Backup`, `SessionMark` and cursors
  update their tables (the last three: [F11 §13] `BACKUPS`, `SESSMARKS` and `CURSORS`);
- a `RefUpdate` with reason 5 moves `orphans/<R>` to the parked commit, creating the ref when `old` is zero (§9.2); the
  parked commit's own ref is not moved (I27′);
- a `Reserve` adds each `SchemaRes` to the store-wide map (space, key) → id of [F08 §8.3], so no other item takes the id,
  and its ranges to the allocators (§10.2); an `ExtentHead` changes no table.

### 10.4 Runtime records

Records of the `K_RT` kinds are indexed by key → (lsn, row index) during the scan and decoded only when a key is used
([AR §4.3], [71 RAM-M5]); the newest row per key wins. After a runtime-only fold (§9.9 bit 2), the `K_RT` records in
`[checkpoint_lsn, rt_upto_lsn)` are already in the segment set's runtime sections and are skipped, so that an older tail
row never overrides the folded one. [F09] records `rt_upto_lsn` where a reader that starts from `HEAD` finds it
(open point 8).

### 10.5 Symbols

Symbol definitions (§8.1) register in log order during every scan. A reader resolves an id first in the tail's
definitions and then in `SYMTAB` ([F09]; probed in place, [AR §4.7]).

## Coverage

| Item | Part covered here | Section |
|---|---|---|
| [60 §2.5] [AR] row "Log": extents reading as zero beyond the tail, created per [80 §2.3.3], of a store-parameter size; the 32 B `RecHdr` with lsn, epoch, xxh3; the `group_end` chain trailer, groups never spanning extents; the kinds `Commit`, `RefUpdate`, `ClientHead`, `Lease`, `Marker` (settled/deleted/cleared), `Idem`, `GitMap`, `Pin`, `Checkpoint` (per-ref lsn lists, promotion entries), `RefTable`, `Lazy`, `Noop` | complete; the `Commit` body is [F06]'s | §2–§5, §7, §9 |
| [60 §2.5] audit row "Log": `RecHdr.flags` bit 1 `group_end`; validity = sane length and kind ∧ epoch ∧ lsn = position ∧ xxh3, and per group the chain trailer; kinds `AnchorRes` (lazy), `Backup` (durable), `SessionMark` (lazy); the `TreeReg` epoch and dirty-row payloads | complete | §3, §4.6, §5.2, §9.13, §9.14, §9.23, §9.26 |
| [60 §2.5] issue-2 row "Store parameters": the extent size | its use: every extent `E` bytes, the lsn mapping, the range confirmed; the value is [F17]'s | §2.2, §2.3 |
| [60 §2.5] "Protocol decisions" (b) | the format side: an invalid group below `durable_lsn` is corruption, at or above it the end of the log | §5.3 |
| [60 §2.5] "Protocol decisions" (k) | the format side: the chain trailer, the seed, group validity; the protocol is [F16]'s | §4 |
| [60 §2.5] "Protocol decisions" (a) | none: the re-write rule is [F16]'s; §5.3 gives what it re-writes | — |
| [40] R-6 | the log side: `next_anchor` re-derived from the `aN` that anchor-creating ops carry; the field is [F04]'s, the op field [F06]'s | §9.1, §10.2 |
| [40] R-7 | complete: durable `FsIntent`, `FsIntentDone`, `FsIntentAborted`; lazy `FileObs`, `Pending`, `FPrint`, `JournalCursor`, `DirMap`, `TreeReg` (with settle epochs and the dirty row), `PrefixEv`, `GitFacts`, `AnchorRes` | §7, §9.15–§9.26 |
| [40] R-15 | the log form of a binding row (the expected git ref and base commit per designated tree); the `HEADS` row and I-F12 are [F11]'s and [F18]'s | §9.3 |
| [40] R-18 | the log form of a `FILEOBS` row with every field of R-18; the section row layout is [F11]'s | §9.18 |
| [50] F9 | complete: `append_hlc` per entry of the per-ref lsn lists | §9.9 |
| [50] F14 | the use of `append_hlc` in the per-ref lists; the header field is [F06]'s | §9.1, §9.9 |
| [50] F17 | the tail side: `ALLOC` entries derived from `Commit` records; the section is [F11]'s | §9.1, §10.3 |
| [80] X-F2 | the intent anchor in `FsIntent`; the lease anchor and the `{wall, boot_hash, mono}` deadline in `Lease`; the layouts are [F03]'s and [OS/clock]'s | §9.4, §9.15 |
| [80] X-F3 | the format: the chain trailer, the seed after `init` and every re-roll, groups never spanning extents with `Noop` padding, group validity | §4, §5 |
| [80] X-F5 | the record-kind durability tag, `RecHdr.flags` bit 0 | §3.2, §6, §7 |
| [80] X-F8 | the record kinds `JournalCursor` and `DirMap`, the `FSINTENT` holder (the intent anchor plus a diagnostic `ProcId`); the tagged layouts are [F11]'s | §9.15, §9.21, §9.22 |
| [80] X-F10 | the file numbers that records name, and the extents' lsn-derived numbers | §2.1, §8.4, §9.9 |
| [90 §10.1] "`LEASES` runtime rows" | the log form carrying `kind`, `role`, `run`, `anchor`, `bound` and the root session; the row is [F11]'s | §9.4 |
| [90 §10.1] "Holder anchor (X-F2)" | its use in `Lease` and `FsIntent`; the anchor is [F03]'s | §9.4, §9.15 |

## Holes

None. No value in this chapter is decided by an M0 measurement. The extent size `E` is P01 of [F17] (design-fixed,
64 MiB). Whether commit bodies travel raw or compressed in the tail is decided by measurement 6 inside the commit body
([F06], [F10]); it changes no byte of this chapter.

## Open points for the review

1. **Rotation padding when fewer than 40 bytes remain** ([PLAN §3.3] gap; §4.4). A group is never placed so that 1–39
   bytes would remain in its extent: then the writer pads the whole remainder with one `Noop` group and moves on. The
   invariant G-4 guarantees that every pad is at least 40 bytes, the smallest group. **Pass 1 (S1-18):** the earlier
   reserve of 0 was wrong — a group of `E − 39` to `E − 1` bytes met neither `g = r` nor `g ≤ r − 40` at a fresh extent
   and would have been padded forever. With the extent head of §4.5 the rotation reserve is R = H + 40 = 178 bytes, the
   largest ordinary group is `E − R` bytes (G-2), and [F17 §4.4] W3 uses R.
2. **Epoch starts and extent reuse** (§2.6, §4.5). A new epoch starts at a new extent beyond every lsn used before, with
   its extent head as the epoch-start group, and `HEAD.epoch_lsn` marks it ([F04] open point 3). lsns are never reused,
   so a pointer into history never becomes ambiguous. [AR §4.10]'s "zero-fill recycled extents" and `recycle_extent` of
   [OS/fs §4.5] remain possible only as a reuse of a file under a new extent number; [F16] P-74 reuses no file that held a
   group.
3. **The retirement bound EX-5** (§2.5). An extent is retired only once the checkpoint lies strictly beyond it, so the
   chain value at `checkpoint_lsn` is always readable from an active extent. [F16] P-73 cites EX-4 and EX-5, and so does
   [F17 §4.2] (pass 1, S1-49).
4. **Kind numbering** (§7). Values 1–26 in the order base kinds, audit kinds, R-7 kinds, then 27 and 28 for the
   protocol's reservation and extent head; 0 is invalid so that a zero-filled extent ends every scan.
5. **Configurable durability** (§6.1; a conflict between design rows). X-F5 says "record kinds keep one tag in the
   registry", while [AR §13]'s `durability.lazy-kinds` lets the owner make heartbeats, cursors and session marks durable.
   Resolution: every kind has a fixed class except those three configurable sub-kinds; bit 0 on disk decides, and no
   reader consults the configuration. [80] X-F5 is followed for every kind the key does not name.
6. **Lazy `ANCESTRY`, heartbeat and cursor** ([PLAN §3.3] gap). `ANCESTRY` facts are `GitFacts` records of type 1, as
   [40 §2.6] describes `GITFACTS` ("ancestry answers ([AR]'s `ANCESTRY` facts)"); heartbeats and cursors are `Lazy`
   sub-kinds 1 and 2. Consequence for [F17 §5.4]: the runtime-only fold must also fold `GitFacts` ancestry facts into
   `ANCESTRY`. **Done (pass 1, A1-46):** [F17 §5.4] lists [F09 §15.1]'s ten runtime-window sections, `ANCESTRY` included.
7. **Symbol definitions in the tail** (§8.1). [F01 §8.1] leaves "new symbols of the log tail" to this chapter. Any
   record may define symbols in a leading block flagged by `RecHdr.flags` bit 2. The class codes 1–11 follow [F01 §8.2]'s
   table order; [F09]'s `SYMTAB` must use the same codes, and the review may move the code column into [F01 §8.2].
8. **`Checkpoint` as the one record of segment-set and registry changes** (§9.9). Promotions (with or without a set
   change), retirements, runtime-only folds, GC rewrites and file releases share the record; a checkpoint set's id is the
   record's lsn, and 0 in a promotion's `base_pin` means "this record". Consequences:
   - [F09] and [F10] must define the folded registries of `hist`, `gitmap` and `blobs` files that `added`, `released` and
     `retirements` maintain, since `HEAD` names only `main`'s set ([F04] open point 4);
   - [F09] must record `rt_upto_lsn` where a reader finds it, or [F11]'s runtime rows must carry their source lsn, so
     that §10.4's skip rule is decidable.

   **Done:** [F09 §14.4] `FILES` is the folded registry of the sealed files a segment set names (with the `reserved` flag
   for a `Reserve` record's numbers, pass 1), and [F09 §2.1] `SegHdr.rt_upto_lsn` with [F09 §15.1] gives the skip bound.
9. **`RefTable` entries** (§9.10), written with every `RefUpdate` and by `gc`; commits never write one. [AR §4.2] says
   the ref table "lives in" `RefTable` records without saying when they are written; this reading keeps a commit's
   record small. Pass 1 (A1-6, S1-8, P1-2): an entry is a partial upsert by the rule of [F11 §3.9], and carries the
   fork and trunk-mark fields and the `pinned` bit the `REFS` row needs (fields 20–23, `eflags` bit 2).
10. **Intent ids and closed intents** (§9.15, §9.17). The intent id is the `FsIntent` record's lsn (closing [F02] open
    point 7). Recovery closes an intent whose outcome is `ambiguous` or `missing` with `FsIntentAborted` reasons 4 and 5,
    so no writer re-examines it at every open; `brief`/`doctor` read the closed row. [F16] P-71 adopts it (reasons 1–5).
11. **Lease ids** (§9.4). A claim takes `lease_id` = its fencing token, so `HEAD.fence` allocates both and recovery
    derives both from one counter. [F11] and [API] may choose a separate counter; the record keeps both fields either way.
    The run scope is the run node's `#N`.
12. **Enumerations owned elsewhere** (§8.7). Link states, anchor states, evidence classes, attribute bits, resolver
    versions and the `fstime` granularity keep their owners' codes; this chapter fixes only their widths. Ref kinds
    (§9.10), marker fields (§9.5) and release reasons (§9.4) are numbered here; pin holders, intent item outcomes and
    abort reasons, and settle-epoch kinds are [F11]'s (§4, §12.7, §12.4), with the values these payloads already used
    (pass 1, A1-6).
13. **The stale [60 §2.5] copy of R-7** ([PLAN §3.3] gap). [60 §2.5]'s R-7 row lacks `AnchorRes`; [40 §2.11] is
    authoritative and lists it, and so does [AR §4.3]. This chapter follows [40].
14. **Fold targets of cursors and session marks** (§9.11, §9.14). No section of [AR §4.4] holds them. Pass 1 (A1-23):
    [F11 §13] adds `CURSORS`, `SESSMARKS` and `BACKUPS` (the backup registry of §9.13), one row per (session, agent,
    feed, task), per (session, agent) or per directory, dropped after `idempotency.retention` (cursors and marks).
    Losing them is harmless: the next prompt re-shows changes, a lost pack cursor leaves C8 empty, and `pack` renders
    the rules in full. **Pass 1, round 3 (closure NC-10):** the per-(agent, task) pack cursor of [AR §7.4] C8, which
    [RULES/pack-classes] PT-028 reads, is `Lazy` `sub` 2 with `feed` 2 and the `#N` of the pack's target T (PT-001) in
    the new field `task` (§9.11), folded into [F11 §13.1]'s row of the same key. T is named by its `#N`, as a lease's
    task is (§9.4), because the row has room for 4 bytes and not for a 16-byte uid; a `#N` is never reused ([F11 §9]),
    so a re-keyed node starts with no cursor. **Closure NC-11, decided 2026-09-28 (OQ-F-3 (b)):** the layer that
    delivers the pack (the `SubagentStart` hook or the MCP server, [AR §5d.1]) appends the record; the `pack` verb, which
    I-F5 ([F18 §2.5]) lists among the reads that append nothing, never does. No M0 command is a delivering layer ([API]
    open point 48, PX-011), so at M0 cursor(A, T) is absent and C8 is empty. The record's bytes and fold are unchanged.
15. **`ALLOC` has no record kind** (§9.1). Its tail entries are derived from `Create` ops and the commit header
    ([AR §4.5] step 8); [50] F17's "tail records" are read as the `Commit` records.
16. **Payload corruption is fatal everywhere** (§5.4). A checksummed record with a malformed payload is a defective
    writer, not a torn write, so it is exit 7 even above `durable_lsn`.
17. **Per-ref lists key by `ref_id`** (§9.9), not by the ref symbol of [20] G15: ref ids are never reused, names are.
18. **Git paths are bytes** (§9.25): git tree paths need not be UTF-8, so `GitFacts` stores them as `vbytes`; stored
    project paths elsewhere stay `vstr` (I-F8).
19. **Row batches** (§8.5). The runtime kinds carry several rows per record with a length per row, so a settle's many
    rows cost one header and a replay can index keys without decoding values.
20. **The reservation record** (§9.27; pass 1, P1-3, S1-11, A1-12; [F16] open point 5). Kind 27 `Reserve` is durable, so
    a failed flush in another process cannot make it vanish while the file streams. It carries the `#N` and `aN` ranges,
    the `cs` and `blobs` file numbers, the schema ids and, in its `SymDefs` block, the symbols; its `HEAD` effect is the
    fold of those allocators. Schema reservations name the item by its key's name strings, because a store-local id is
    exactly what is being reserved. The `hlc` lets `gc` release the files of a reservation whose commit never lands.
21. **Extent heads** (§4.5, §9.28; pass 1, P1-8). Before this pass the log carried nothing from which a `repair` without a
    valid slot could validate the active log after the first retirement: the epoch-start group lay in a retired extent and
    [F10 §4.1] drops the rotation pad whose trailer seeds the next extent. The `HEAD`-only fields (`init` with the store
    id, `epoch_lsn`, the `quiet` and `readonly` flags) and the HLC floor were not in the log at all. Every extent now
    begins with a 138-byte durable `ExtentHead` that carries them, the chain value at its own lsn and the log-derived
    counters as of its append; the epoch-start group is the first of them. The cost is one small durable group per
    extent (per 64 MiB of log) and 178 bytes of rotation reserve. `retired` and `config_gen` are not carried: a store
    that `repair` rebuilds is not retired, and `config_gen` is compared for inequality only ([F04 §5.6]).
22. **Read errors in a writer's scan** (§5.3; pass 1, S1-25; [F16] open point 13). A failed read at or above
    `durable_lsn` ends a reader's view but stops a writer: because `durable_lsn` is only a lower bound after an OS crash,
    treating the unreadable range as the end of the log would let the next append overwrite acknowledged groups (X5:
    refuse rather than lose). A persistent read error there makes the store refuse writes until `repair`.
23. **Spec sync 2b** (WP-91, WP-95, WP-90b). A valid group that leaves 1–39 bytes in its extent, and an extent's first
    group that is not one `ExtentHead`, are corrupt wherever they lie (§5.4), as the oracle's scan already reads them; the
    alternative (§4.6 calls them invalid, so §5.3 classifies them by `durable_lsn`) was rejected, because above
    `durable_lsn` it would let the next append overwrite a checksummed group that an OS crash may have left acknowledged
    (X5). §9.2 states where a `RefUpdate` lies among commit seqs for `restore_seq`. §9.4 reason 4 is ended by the next
    claim of the task (LE-012), not at a read, so only a task lease carries it (a C-rule: no decoder checks it);
    reason 8 is not assigned, since the `SubagentStop` hook releases through `Release` (reason 1). §10.2 states the
    fold's range as [F16] P-50 does: from the selected slot's `durable_lsn`, not from its `committed_lsn`. §9.5 names
    ME-001 to ME-011 as the events that write `Marker` entries; ME-012's move and ME-013's return write none
    ([RULES/state-definition] open point 17 (a)).
