# 04 — The `HEAD` file

| | |
|---|---|
| Title | The `HEAD` file: two 4 KiB checksummed slots in the G18 layout (`refs_lsn`/`pins_lsn`/`heads_lsn`/`markers_lsn`, `image_cursor[4]`, `seq_ring`, `flags` bits 0–3, `durable_lsn`, `boot_id`, `config_gen`, [40] R-6's `next_anchor`), the home of the parameters fixed at `init` (`InitParams`, with the store id, and `project_oid_algo`), the HLC maxima `hlc_seq` and `hlc_commit`, the slot validity, torn-slot and two-slot rules, and what every publish writes |
| Chapter | [F04], `docs/spec/format/04-head.md` |
| Status | draft, pass 1 pending |
| Work package | WP-11 (R-SPEC-P), [60 §3.1] item 1 |
| Sources | [AR §4.2] (the `HeadSlot` G18 layout, the durable bound, the boot check, the two-slot barrier, `retired`); [AR §4.1] `HEAD` row; [AR §4.5] steps 1, 6, 10 (read, scan, publish); [AR §4.7] (open path); [AR §4.9], [AR §4.10]; [AR §5a.2] (refs), [AR §5a.3] (pins, promotion), [AR §5b.6] (`image_cursor`), [AR §6.3] (`seq_ring`), [AR §6.6] (quiet mode), [AR §13] (`config_gen`); [80 §2.3.2] (the `HEAD` publish and barrier rows), [80 §2.3.5] items (1), (3), (7); [80 §2.4.3] (the publish, boot-change recovery, readers, I-G6); [80 §2.7.1] (`boot_id`, Unknown-boot mode); [80 §3.1] X-F2, X-F3; [40 §2.11] R-6; [60 §2.5] the [AR] row "`HEAD`", the audit row "`HEAD`", the issue-2 row "Store parameters", the "Protocol decisions" row (decisions (c), (d), (g), (k)), the "Cross-platform" row; [72 B1], [72 M2]; [F02] open points 2, 4, 14; [F15] OP-1; [F17 §2] and OP-17-06, OP-17-21; [PLAN §3.2] WP-11, [PLAN §3.3] (the `HeadSlot` order and `SegRef` padding; the home of `init`-scope parameters) |
| Depends on | [F01], [F02], [F03], [F05], [F17]; cites [F09], [F10], [F11], [F15], [F16], [F19], [OS/proc], [OS/clock], [OS/fs] |

## 1. Scope

This chapter owns every byte of the `HEAD` file: the slot layout, its sub-structures, the meaning and the valid range of
every field, the rules by which a process chooses a slot, and the rules by which a publish writes one. The events that
change the log-derived fields are records of [F05], and [F05 §10] states each record's effect on `HEAD`. When a process
publishes, flushes `HEAD` or runs boot-change recovery is [F16]'s protocol. The values of the parameters fixed at `init`
are [F17]'s.

`HEAD` is a **cache with a durable bound**: the log is the truth, every field that the log determines is re-derived by a
scan ([F05 §10]), and `HEAD` is not flushed on the commit path ([AR §4.2]; 1PC+C).

## 2. The file

- `HEAD` is exactly **8,192 bytes**: slot A at offset 0 and slot B at offset 4,096, each 4,096 bytes ([AR §4.1]).
  The two slots are two separate sectors of the fault model ([F15 §1.4]).
- It is never mapped: every access is a `read_at` or `write_at` at an offset ([AR §4.1], [80 §2.5]).
- It is created last by `init` ([F02 §5.5]) and by `restore`, never resized, never truncated, and never deleted while the
  store exists. A process that finds a size other than 8,192 bytes exits 7 naming `HEAD` and `moirai doctor --fsck`
  ([F19]).
- A reader reads both slots with one `read_at` of 8,192 bytes. A publisher writes one whole slot with one `write_at` of
  4,096 bytes (§9).

## 3. `HeadSlot` (4,096 B)

### 3.1 Layout

Fields appear in the order of [AR §4.2]'s G18 layout, byte-packed with no implicit padding ([F01 §4.3]); several `u64`
fields are therefore not 8-aligned, which readers tolerate ([F01 §4.3]). Offsets are relative to the start of the slot.

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 4 | `[4]u8` | `magic` | `"MOIR"`: `4D 4F 49 52` ([F01 §4.5]) |
| 4 | 2 | `u16` | `format` | the format version ([F01 §9.1]); 1 |
| 6 | 2 | `u16` | `flags` | §5.2 |
| 8 | 8 | `u64` | `slot_seq` | the publish counter; the valid slot with the greater value is the newest (§8) |
| 16 | 8 | `u64` | `epoch` | the store epoch (§5.3); never 0 |
| 24 | 8 | `u64` | `committed_lsn` | the visibility bound: readers never read at or beyond it (§5.4) |
| 32 | 8 | `u64` | `durable_lsn` | the end of the last flushed group (§5.4) |
| 40 | 16 | `[16]u8` | `boot_id` | the boot identity of the last boot-change recovery, or of `init` (§5.5); all zero = none recorded |
| 56 | 4 | `u32` | `config_gen` | the configuration generation (§5.6) |
| 60 | 8 | `u64` | `checkpoint_lsn` | the first log position not folded into a segment (§5.4) |
| 68 | 8 | `u64` | `commit_seq` | the `seq` of the newest covered commit; 0 when none (§5.7) |
| 76 | 4 | `u32` | `next_id` | the next `#N` to allocate; ≥ 1 (§5.7) |
| 80 | 4 | `u32` | `next_anchor` | the next anchor handle `aN` to allocate ([40] R-6); ≥ 1 (§5.7) |
| 84 | 8 | `u64` | `fence` | the greatest lease fencing token allocated; 0 when none (§5.7) |
| 92 | 4 | `u32` | `active_log` | the file number of the oldest log extent not retired (§5.8); ≥ 1 |
| 96 | 1 | `u8` | `n_segments` | the number of valid entries in `segments`; 0 to 8 |
| 97 | 3 | `[3]u8` | `_pad0` | reserved-zero ([F01 §10]) |
| 100 | 232 | `[8]SegRef` | `segments` | the current segment set of `main` (§4.1, §5.9); entries at index ≥ `n_segments` are all zero |
| 332 | 8 | `u64` | `refs_lsn` | the lsn of the newest covered `RefTable` record; 0 = none (§5.10) |
| 340 | 8 | `u64` | `pins_lsn` | the lsn of the newest covered `Pin` record; 0 = none |
| 348 | 8 | `u64` | `heads_lsn` | the lsn of the newest covered `ClientHead` record; 0 = none |
| 356 | 8 | `u64` | `markers_lsn` | the lsn of the newest covered `Marker` record; 0 = none |
| 364 | 40 | `[4]ImageCursor` | `image_cursor` | export cursors per (destination, algorithm) (§4.2, §5.11) |
| 404 | 512 | `[32]SeqRingEntry` | `seq_ring` | the newest commits, a ring indexed by `seq mod 32` (§4.3, §5.12) |
| 916 | 108 | `[108]u8` | `_reserved0` | reserved-zero |
| 1024 | 32 | `InitParams` | `init` | the parameters fixed at `init`, and the store id (§4.4) |
| 1056 | 8 | `u64` | `epoch_lsn` | the lsn of the first group of the current epoch, the epoch-start group ([F05 §4.5]) (§5.3) |
| 1064 | 4 | `u32` | `next_file_no` | the store-wide allocator of sealed-file numbers (§5.13); ≥ 1 |
| 1068 | 4 | `u32` | `next_ref_id` | the allocator of never-reused ref ids (§5.14) |
| 1072 | 1 | `u8` | `project_oid_algo` | the `project` root's content-hash algorithm, fixed at `init` (§5.16); 1 `sha1` or 2 `sha256` |
| 1073 | 7 | `[7]u8` | `_pad1` | reserved-zero |
| 1080 | 8 | `u64` | `hlc_seq` | the greatest value of the store's HLC sequence (§5.15) |
| 1088 | 8 | `u64` | `hlc_commit` | the greatest `hlc` of any commit the store holds (§5.15) |
| 1096 | 2984 | `[2984]u8` | `_reserved1` | reserved-zero |
| 4080 | 16 | `XXH3-128` | `xxh3_128` | XXH3-128, seed 0, over bytes `[0, 4080)` of the slot, stored as (`low64`, `high64`), each a little-endian `u64` ([F01 §7.2]) |
| total | 4096 | | | |

### 3.2 Offsets inside the slot of every sub-structure entry

| Entry | Offset of entry k |
|---|---|
| `segments[k]`, 0 ≤ k < 8 | 100 + 29·k |
| `image_cursor[k]`, 0 ≤ k < 4 | 364 + 10·k |
| `seq_ring[k]`, 0 ≤ k < 32 | 404 + 16·k |

## 4. Sub-structures

### 4.1 `SegRef` (29 B)

Byte-packed, as [AR §4.2] states (the 29-byte size is the [PLAN §3.3] gap this chapter closes).

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 4 | `u32` | `file_no` | the file number: `G` of `seg.base.<G>`, `K` of `seg.d<K>`, `D` of `dict.<D>` ([F02 §6]); ≥ 1 |
| 4 | 1 | `u8` | `kind` | the enumeration below |
| 5 | 8 | `u64` | `upto_lsn` | base and delta: the log position up to which the segment folds the log, equal to the `upto_lsn` in its header ([F09]); dict: 0 |
| 13 | 16 | `b16` | `blake3_16` | the first 16 bytes of the BLAKE3-256 content digest that the file's own header or footer records ([F09] for segments, [F10] for the dictionary's `MDIC` header) |
| total | 29 | | | |

| value | name | meaning |
|---|---|---|
| 0 | empty | only in entries at index ≥ `n_segments`, which are all zero |
| 1 | `base` | `seg.base.<G>`, the base segment of `main` |
| 2 | `delta` | `seg.d<K>`, a delta segment of `main` |
| 3 | `dict` | `dict.<D>`, the compression dictionary; valid only when `HOLE(F02-dict-file)` decides that a dictionary exists ([F02 §5.1]) |

**Order and multiplicity** of the entries `[0, n_segments)`: at most one `base`, which is entry 0 when present; then the
`delta` entries, oldest first (increasing `file_no`, the fold order d1..dk of [AR §4.9]); then at most one `dict`, last.
`upto_lsn` does not decrease along the base and delta entries.

**Capacity.** Eight entries hold one base, up to `store.fold-width` deltas ([F17 §6.1], at most 5), the one yield delta
a long maintenance holding may add above them ([F16] P-98) and one dictionary, so [F17]'s constraint C-4 holds with
`n_other` = 1 (open point 4; pass 1, P1-9). Every other sealed file (`hist`, `blobs`, `gitmap`,
`cs`, a promoted branch's `seg.b<ref_id>.<K>`) is named by log records ([F05 §9.1], [F05 §9.9]) and the segment sections
that fold them ([F09], [F10], [F11]), never by `HEAD`.

**Identity check.** A process that maps a file named by a `SegRef` compares `blake3_16` with the digest the file's
header records ([F09], [F10]); a mismatch is handled like a missing file ([AR §4.2] "recovery ... rebuilds the segment set
from the durable `Checkpoint` records"; [F16]). Content verification is `doctor --fsck`'s.

### 4.2 `ImageCursor` (10 B)

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 1 | `u8` | `dest` | the destination number ([F05 §9.7]); 0 = empty entry |
| 1 | 1 | `u8` | `algo` | the git object format ([F01 §7.5]); 1 `sha1` or 2 `sha256`; 0 in an empty entry |
| 2 | 8 | `u64` | `seq` | the greatest commit `seq` exported to (`dest`, `algo`), from the covered `GitMap` records |
| total | 10 | | | |

An empty entry is 10 zero bytes. No two entries carry the same (`dest`, `algo`).

### 4.3 `SeqRingEntry` (16 B)

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 8 | `u64` | `seq` | a commit's `seq`; 0 = empty entry |
| 8 | 8 | `u64` | `lsn` | the lsn of that commit's `Commit` record |
| total | 16 | | | |

### 4.4 `InitParams` (32 B at slot offset 1024)

The parameters fixed at `init`, whose set, meaning, ranges and rules IP-1 to IP-6 are [F17 §2]'s, and the store id of
[F02 §4]. This chapter fixes the block's place in the slot (closing OP-17-21 of [F17] and open point 2 of [F02]).

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 8 | `u64` | `log_extent_bytes` | `store.log-extent-bytes` ([F17 §4.1]): a power of two in [2^16, 2^30] |
| 8 | 4 | `u32` | `hist_frame_commits` | `store.hist-frame-commits` ([F17 §4.3]): 1 to 65,536 |
| 12 | 4 | `u32` | `hist_frame_bytes` | `store.hist-frame-bytes` ([F17 §4.3]): 4,096 to 1,048,576 |
| 16 | 16 | `b16` | `store_id` | the store id ([F02 §4]); never all zero |
| total | 32 | | | |

This is exactly [F17 §2.1]'s layout (pass 1, A1-13, S1-12, P1-4). The one further `init`-fixed value,
`project_oid_algo`, does not fit the block and sits at slot offset 1072 (§5.16), under the same rules IP-1 to IP-3.

## 5. Field semantics

### 5.1 `magic` and `format`

`magic` identifies a slot; `format` is 1 in format v1. A slot whose `format` is above 1 makes the store unavailable
(§7 check 3).

### 5.2 `flags`

| bit | name | meaning | changed by |
|---|---|---|---|
| 0 | `quiet` | quiet mode is on ([AR §6.6], [F17 §5.3]); the quiet-advisory byte of [F03 §3.1] adds a process-lifetime form | `moirai quiet on` and `off`, by a durable publish (§9.2) |
| 1 | `fts_tier2` | full-text tier 2 is built for new segments; once set, never cleared ([F17 §6.4]) | the publish that covers a `Checkpoint` record carrying `fts_tier2` ([F05 §9.9]) |
| 2 | `readonly` | the store refuses every write verb, maintenance and GC with exit 7 ("store is read-only", [F19]); reads are served | an explicit administrative verb, by a durable publish (§9.2; open point 8) |
| 3 | `retired` | this store was replaced by `restore`'s swap; a process that sees the bit stops using this store directory and runs discovery again ([F02 §3.6], [AR §4.10]) | `restore`, on the old store, by a durable publish (§9.2) |

Bits 4–15 are reserved-zero.

### 5.3 `epoch` and `epoch_lsn`

- `epoch` is a `u64` drawn from the OS's cryptographically secure random source (`Entropy::fill_random`,
  [OS/README §4.6]) at `init`, never 0. `restore` and `repair --rebuild-from-log` re-roll it to a new random non-zero
  value different from the previous one ([AR §4.2], G25). A draw that breaks either condition is drawn again.
- A log record is valid only if its `epoch` equals the `epoch` of the slot the scanning process uses ([F05 §5.2]).
- The chain seed of the first group of an epoch is `XXH3-64(epoch)` over the 8 little-endian bytes of `epoch`
  ([F01 §7.3], [80] X-F3).
- `epoch_lsn` is the lsn at which the current epoch's first group, the epoch-start group of [F05 §4.5], begins: 0 after
  `init`; after a re-roll, the first lsn of a new log extent beyond every lsn used before ([F05 §2.6]). A group that
  begins at `epoch_lsn` is seeded with `XXH3-64(epoch)`; every other group is seeded with the 8 bytes before it
  ([F05 §4.3]). No scan starts below `epoch_lsn`.

### 5.4 The log bounds

All log positions are lsns ([F05 §2.3]): byte positions in the store's log stream. `checkpoint_lsn`, `durable_lsn` and
`committed_lsn` are group boundaries: each is the position of the first byte after a group, or `epoch_lsn`.

| Field | Meaning | Changes |
|---|---|---|
| `checkpoint_lsn` | the first log position whose records are not folded into the segment set of `segments`; equal to the greater of `epoch_lsn` and the greatest `upto_lsn` of the base and delta entries (after an epoch re-roll the set folds only lsns below `epoch_lsn`, [F05 §2.6]) | set by the publish that covers a `Checkpoint` record with a set change ([F05 §9.9]) |
| `durable_lsn` | the end of the last group made durable by a flush that returned successfully ([72 B1]) | advanced only by publishes that follow a successful flush; never decreases |
| `committed_lsn` | the end of the visible log: readers read records in `[checkpoint_lsn, committed_lsn)` and never beyond | set at every publish to the end of the valid log as the publisher scanned it, stopping before the first pending durable group its flush does not cover ([80 §2.4.3]); decreases only when a lazy tail was lost, and then to the valid end |

In every valid slot `epoch_lsn ≤ checkpoint_lsn ≤ durable_lsn ≤ committed_lsn`. Recovery and writers scan from
`min(durable_lsn, checkpoint_lsn)`, which equals `checkpoint_lsn` under this ordering ([AR §4.2], [F05 §5.1]).

### 5.5 `boot_id`

- `boot_id` holds the 16-byte boot identity ([OS/proc §4]) of the process that last ran boot-change recovery, or of the
  `init` or `restore` that created the store. All zero bytes mean that no boot identity is recorded (the creating
  process was in Unknown-boot mode).
- **Boot check** ([AR §4.2], [80 §2.7.1]): a process whose boot identity is `Known(b)` compares `b` with the selected
  slot's `boot_id` before its first read. They differ, a zero `boot_id` included, → boot-change recovery ([F16]; §9.4).
  A process in Unknown-boot mode skips the check.
- Only boot-change recovery changes `boot_id`. Every other publish copies it unchanged from the newest valid slot, and a
  process in Unknown-boot mode never writes a `boot_id` other than the one it read (rule U1 of [OS/proc §5]).

### 5.6 `config_gen`

A `u32` that `moirai config set` increments by 1, modulo 2^32, after it has replaced the store's `config` file
([AR §13], [80 §2.3.2]). The long-lived MCP server compares it with the value it last read and re-reads `config` when they
differ. It is compared for inequality only; wrap-around is harmless. It changes by an ordinary publish (§9.1): a lost
update needs an OS crash, after which every process starts again and reads `config` fresh.

### 5.7 Counters

| Field | Meaning | Initial value | Re-derived from ([F05 §10]) |
|---|---|---|---|
| `commit_seq` | the `seq` of the newest covered commit (the change-feed position, [AR §6.3]) | 0 | `Commit` records |
| `next_id` | the next `#N` to allocate ([AR §3.1]); `#0` is never a node | 1 | the ids that `Create` ops allocate ([F06]) |
| `next_anchor` | the next anchor handle number `aN` ([40] R-6, [40 §2.7]) | 1 | the `aN` that anchor-creating ops carry ([F06]; A1P-12) |
| `fence` | the greatest fencing token allocated; a claim takes `fence + 1` ([AR §5d.1]) | 0 | `Lease` records |

A counter never decreases (I-G6). A publish sets each to the maximum of the newest slot's value and the value the covered
records imply. A write that would need `next_id` or `next_anchor` beyond 2^32 − 1 is refused with exit 7 and nothing is
written ([AR §4.5] step 4 states the rule for `seq`; [F19] assigns the codes).

### 5.8 `active_log`

The file number of the oldest log extent that is not retired ([F05 §2.5]). Every lsn at or above `(active_log − 1) × E`
lies in a log extent (`E` = `init.log_extent_bytes`); every lsn below it lies in a `hist` file ([F10]). It is set by the
publish that covers a `Checkpoint` record with a set change ([F05 §9.9]; [80 §2.4.3] "a covered `Checkpoint` sets …
`active_log`"). In every valid slot `epoch_lsn ≤ (active_log − 1) × E ≤ checkpoint_lsn`, and the second inequality is
strict unless `checkpoint_lsn` = `epoch_lsn` ([F05 §2.5] rule EX-5).

### 5.9 `segments` and `n_segments`

The current segment set of `main` (§4.1). A reader maps these files ([AR §4.7]); the tail beyond `checkpoint_lsn` is
replayed over them. A promoted branch's own segments are found through the ref table ([F11]). `n_segments` = 0 is valid:
the view is then the tail alone.

### 5.10 The table pointers

`refs_lsn`, `pins_lsn`, `heads_lsn` and `markers_lsn` hold the lsn of the newest covered record of kind `RefTable`,
`Pin`, `ClientHead` and `Marker` respectively ([AR §4.2]). The value 0 means that no such record has been covered in the
store's life; no record of these kinds can lie at lsn 0, which always holds the epoch-start group ([F05 §4.5]). A pointer
at or above `checkpoint_lsn` tells a process that the tail holds records of that kind beyond the folded section
(`REFS`, `PINS`, `HEADS`, `MARKERS`, [F11]); a pointer below it tells the process that the section is current. A pointer
never decreases.

### 5.11 `image_cursor`

The starting point of an export's frontier walk for up to four (`dest`, `algo`) pairs ([AR §5b.6] step 2). A publish that
covers a `GitMap` record for (`dest`, `algo`) with `cursor_seq` S ([F05 §9.7]):

1. if an entry for (`dest`, `algo`) exists, its `seq` becomes `max(seq, S)`;
2. else, if an empty entry exists, the first empty entry (lowest index) takes (`dest`, `algo`, S);
3. else the entry with the smallest `seq` (lowest index on a tie) is replaced by (`dest`, `algo`, S).

A pair without an entry is exported from the `gitmap` walk alone; the cursor only shortens the walk. `image_cursor` is a
cache, not a counter of I-G6.

### 5.12 `seq_ring`

The 32 newest covered commits, for `changes --since` without a `hist` read ([AR §6.3]). The commit with `seq` s occupies
entry `s mod 32` as (s, lsn of its `Commit` record). A publish writes an entry for every covered commit in log order, so an
entry holds the newest commit whose `seq` is congruent to its index. A reader treats entry k as valid when its `seq` is in
`(commit_seq − 32, commit_seq]` and `seq mod 32` = k; any other entry is stale.

### 5.13 `next_file_no`

A store-wide allocator of file numbers for every sealed-file family (`hist`, `seg.base`, `seg.d`, `seg.b<ref_id>`,
`blobs`, `dict`, `gitmap`, `cs`). It closes [F02] open point 4, so that no number is ever used twice in any family
([F02 §6.2], [80 §2.5] rule 3):

1. A process that creates a sealed file takes the number `n = max(next_file_no of the newest slot, 1 + every number named
   by the valid log it scanned)`, and creates the file with create-new semantics. If the name exists (an orphan), it tries
   `n + 1`, and so on.
2. The record that names the file carries its number ([F05 §9.1] `cs_ref`, [F05 §9.9]). A publish sets `next_file_no` to
   the maximum of the newest slot's value, every covered `Checkpoint` record's `next_file_no`, and one plus every covered
   file number.
3. The orphan sweep ([F02 §5.6], [F16]) deletes an unnamed numbered file only after a published `next_file_no` exceeds
   its number, so a deleted orphan's number is never taken again. Maintenance advances the counter past an orphan through
   the `next_file_no` field of its next `Checkpoint` record ([F05 §9.9]). Which durable group claims a number first, and
   the creator's check under the writer byte that closes the race with a sweeper, are [F16] P-78's; a bulk producer's
   `Reserve` record claims its files by the same rule ([F05 §9.27]).

Log extents do not use this allocator: their numbers follow from lsns ([F05 §2.3]) and never repeat.

### 5.14 `next_ref_id`

The next `ref_id` to allocate ([AR §4.2] "`ref_id u32 (never reused)`"). A publish sets it to the maximum of the newest
slot's value and one plus every `ref_id` that covered `RefUpdate` and `RefTable` records create ([F05 §9.2],
[F05 §9.10]). Because the counter lives in `HEAD`, a ref id stays unused after `gc` drops the deleted ref's entry
(open point 3). `main`'s ref id and the initial value follow from the groups `init` writes ([F16] P-88; §10).

### 5.15 `hlc_seq` and `hlc_commit`

The two maxima from which every append-time HLC is drawn ([F16] P-36, [OS/clock §7], [API §6.2] CK-4; pass 1, S1-13,
P1-5, A1-17):

- `hlc_seq` is the greatest HLC of the store's **HLC sequence**: the values carried by the semantic durable records
  (`Commit` `append_hlc`, `RefUpdate`, `ClientHead`, `Lease`, `Marker`, `Idem`, `Backup`, `FsIntent`, `FsIntentDone`,
  `FsIntentAborted`, `BodyDrop`, and the reserved `Harvest` from M9–M10; [F05 §10.2]). Records of other kinds carry an
  HLC without raising it.
- `hlc_commit` is the greatest `hlc` of any commit the store holds, local or imported (an imported commit keeps its own
  `hlc`, which can lie ahead of this store's clock).

A publish folds both from the covered records ([F05 §10.2]); a writer takes the maximum of the newest slot's values and
of the groups its scan finds beyond that slot's `committed_lsn` (P-36), so no append needs a scan below `committed_lsn`.
Both are 0 in a new store. An epoch re-roll carries them into the new epoch through the epoch-start extent head
([F05 §9.28]), so they never restart ([API §6.2] CK-6). Retention windows measure `now` from max(`wall_ms`,
max(`hlc_seq`, `hlc_commit`) >> 16) ([F17 §1.6]).

### 5.16 `project_oid_algo`

The content-hash algorithm A(`project`) of the `project` root ([F20 §2.3]), one value of [F01 §7.5]'s `algo` registry:
1 `sha1` or 2 `sha256`. `init` takes it from the object format of the store's repository (`extensions.objectFormat`) and
takes 1 when the store has no repository ([CFG §7.6]); it is never a configuration key. It is `init`-fixed ([F17 §2]):
written into both slots by `init`, kept by every publish, `restore` and `repair`, never recomputed from the repository, and
validated with the `init` block ([F17 §2.2] IP-1–IP-3). Every other root uses `sha1` ([F20 §2.3]). (Pass 1, A1-15,
S1-28, P1-4: the 32-byte block has no spare byte, so the value takes the first byte of the former reserved area.)

## 6. Field classes

| Class | Fields | Source of truth | How a change becomes durable |
|---|---|---|---|
| **log-derived** | `committed_lsn`, `durable_lsn`, `checkpoint_lsn`, `active_log`, `n_segments`, `segments`, `commit_seq`, `next_id`, `next_anchor`, `fence`, `refs_lsn`, `pins_lsn`, `heads_lsn`, `markers_lsn`, `image_cursor`, `seq_ring`, `next_file_no`, `next_ref_id`, `hlc_seq`, `hlc_commit`, `flags.fts_tier2` | the log: each is a fold of records ([F05 §10]) | the records are durable; a lost slot update is re-derived by the next scan ([AR §4.2] "durable bound") |
| **kept in `HEAD`** | `slot_seq`, `epoch`, `epoch_lsn`, `boot_id`, `config_gen`, `flags.quiet`, `flags.readonly`, `flags.retired`, `init`, `project_oid_algo` | `HEAD` itself | `epoch`, `epoch_lsn`, `init`, `project_oid_algo`: written by `init`, `restore` and `repair`, which flush `HEAD` ([F16]); `boot_id`: boot-change recovery, a durable publish (§9.4); `quiet`, `readonly`, `retired`: a durable publish (§9.2); `config_gen`: an ordinary publish (§5.6); `slot_seq`: every publish |

A field kept in `HEAD` is never re-derived from the log by a publish. Every publish copies it from the newest valid slot
unless the publish is the one that changes it. The log's extent heads repeat `epoch_lsn`, `init`, `project_oid_algo`
and the `quiet` and `readonly` flags, and the log-derived counters as of each head ([F05 §9.28]), for `repair` alone
(§8.1; pass 1, P1-8).

## 7. Slot validity

A process classifies each slot it reads by these checks, in order:

1. **Magic.** `magic` ≠ `"MOIR"` → the slot is **absent**.
2. **Checksum.** `xxh3_128` does not match bytes `[0, 4080)` → the slot is **absent** (torn, never written, or damaged).
3. **Version.** `format` = 0 → **absent**. `format` > 1 → **fatal**: exit 7, naming `HEAD` and both versions
   ([F01 §9.1]).
4. **Reserved bytes.** `flags` bits 4–15, `_pad0`, `_pad1`, `_reserved0` and `_reserved1` are zero; every `segments` entry at
   index ≥ `n_segments`, every empty `image_cursor` entry and every empty `seq_ring` entry is all zero. Otherwise
   **fatal**.
5. **Ranges.** Otherwise **fatal** unless all hold:
   - `epoch` ≠ 0; `n_segments` ≤ 8; `next_id` ≥ 1; `next_anchor` ≥ 1; `active_log` ≥ 1; `next_file_no` ≥ 1;
   - `init` passes [F17 §2.2] IP-2 (which includes `init.store_id` not all zero) and `project_oid_algo` ∈ {1, 2};
   - `epoch_lsn` is a multiple of `E`, and the orderings of §5.4 and §5.8 hold;
   - every `segments` entry below `n_segments` has `kind` ∈ 1–3 and `file_no` ≥ 1, the entries follow §4.1's order and
     multiplicity, and `checkpoint_lsn` equals the greater of `epoch_lsn` and the greatest base or delta `upto_lsn`;
   - every non-empty `image_cursor` entry has `dest` ≥ 1 and `algo` ∈ {1, 2}, with no duplicate pair;
   - every non-empty `seq_ring` entry k has `seq` ≠ 0, `seq mod 32` = k and `seq` ≤ `commit_seq`.
6. Otherwise the slot is **valid**.

A slot that passes the checksum but fails check 4 or 5 was written that way by a process: it is a defect, not a torn
write, and the store stops rather than fall back to an older state (X5, [F17 §2.2] IP-2). The text is [F19]'s:
exit 7, naming `HEAD` and `moirai doctor --fsck`. The repair path is plain `moirai repair`: it treats a fatal slot as
absent and rebuilds both slots from the log's extent heads ([F16] P-85), trusting neither slot, since a fatal slot shows
a defective writer. A publisher never writes a fatal slot ([F16] P-48).

## 8. Choosing a slot

### 8.1 The rule

A process reads both slots with one `read_at` and classifies them (§7):

| Slot A | Slot B | Result |
|---|---|---|
| fatal | any | exit 7 (§7) |
| any | fatal | exit 7 |
| valid | valid | the slot with the greater `slot_seq`; equal `slot_seq` requires byte-identical slots, else exit 7 |
| valid | absent | slot A |
| absent | valid | slot B |
| absent | absent | read again, at most twice more; if both are still absent, exit 7: "`HEAD` has no valid slot; run `moirai repair`" ([F15] OP-1, [F19]) |

- **Torn-slot rule** (decision (d) of [60 §2.5]): a reader that finds one slot absent uses the other. Because every
  publish writes only the slot that does not hold the newest valid state (§9.1), a torn publish never destroys the newest
  valid state.
- **Both slots valid** additionally requires byte-identical `init` blocks and equal `project_oid_algo` values
  ([F17 §2.2] IP-3); otherwise exit 7.
- **Both slots absent.** A failed `HEAD` flush can leave both slots failing validation ([F15] OP-1). So can a publish
  whose write failed (`DiskFull`, [F15] FM-5.2) or was cut by its writer's death ([F15 §2.5]), which leaves its slot any
  mix of bytes, followed by a crash that tears the other, dirty slot (FM-1.2): no flush failed, yet no slot is valid.
- The re-reads of the last row cover a read that raced two consecutive publishes (fault-model item (4), [F15]); a store
  in that state after a crash needs `repair`, which rebuilds the slot state from the log's extent heads ([F05 §4.5],
  §9.28): the epoch and `epoch_lsn` of the newest extent head that validates by itself, with its `quiet` and `readonly`
  flags; `init`, `project_oid_algo` and the counters of the newest head the scan reaches; then the fold of the log after
  it ([F15] OP-1, [F16] P-85; pass 1, P1-8). A flag change made after that extent head was written is lost by the
  repair, since flags are kept only in `HEAD` (§6), and the operator re-issues it.

### 8.2 The nine two-slot states (informative)

At a barrier point ([F16]), each slot holds the state before the barrier's writes (old), the state after them (new), or
is torn. Selection gives:

| Slot A \ Slot B | old | new | torn |
|---|---|---|---|
| **old** | the newer old state | new (B) | old (A) |
| **new** | new (A) | the newer new state | new (A) |
| **torn** | old (B) | new (B) | no valid slot: exit 7 after the re-reads |

(torn, torn) needs two torn sectors in one file, which FM-1.2 forbids at one crash while both slots are dirty; it is
reached only when a slot is poisoned by a failed flush ([F15] FM-3.3). A barrier point therefore has 8 reachable states
when both slots are dirty, and 9 with a poisoned slot ([F15 §6.4]). A publish write that failed or was cut ([F15] FM-5.2,
[F15 §2.5]) makes its slot's "new" content invalid, so (new, torn) can also leave no valid slot (§8.1).

The barrier (§9.3; [AR §4.2], [80 §2.3.2]) makes every "old" cell safe: before it deletes anything, both slots on
disk name the post-change state, so "old" there already means "post-change". [F16] and the crash enumerator
([PLAN §3.2] WP-32) use this table, and WP-20 writes a fixture per state.

## 9. Writing a slot

### 9.1 Publish (read-modify-write)

Every change of `HEAD` is a publish, made under the writer byte ([80 §2.4.3], X-F3):

1. `read_at` both slots and select the newest valid slot S by §8 (its exit-7 cases apply).
2. Build S′ as a copy of S and apply the publish's changes: the fold of every group from S.`durable_lsn` to the new
   `committed_lsn`, in log order ([F05 §10.2], [F16] P-50; a group S already folded changes nothing), the new
   `committed_lsn` and, after a flush, `durable_lsn` = max(S.`durable_lsn`, the flushed end); or the one field kept in
   `HEAD` that this publish changes (§6). The fold starts at S.`durable_lsn`, not at S.`committed_lsn`: after a crash S
   can be an older slot whose `committed_lsn` covers a lazy tail that was lost and refilled, and a fold from there would
   miss the refill ([F16] P-50).
3. `slot_seq` = S.`slot_seq` + 1. Recompute `xxh3_128`.
4. `write_at` S′ into the slot that does **not** hold S (slot B when S is in slot A, and conversely), 4,096 bytes at its
   offset. Never write the slot that holds S.
5. Do not flush `HEAD` (1PC+C), except in the cases of §9.2–§9.5.

No field decreases in a publish except `committed_lsn` after a lost lazy tail ([80 §2.4.3], I-G6); a publish never writes
a smaller `durable_lsn`, counter or table pointer, and never an older segment set.

### 9.2 Durable publish

A **durable publish** makes a state survive any crash:

1. Under one holding of the writer byte, two consecutive publishes by §9.1. The first carries the change, if there is
   one; the second is a no-op publish (§9.1 with the same `committed_lsn`, so that its fold changes nothing, and no
   changed field). Because each publish writes the slot that does not hold the newest valid state, the two write
   **both** slots, and both then hold the newest state. The second write also ends any poisoning of the other slot's
   sector left by an earlier failed flush ([F15] OP-1).
2. Release the writer byte, then `durable+meta` on `HEAD` (`sync(DataAndMeta)`, [F15 §4]), outside the writer byte
   ([80 §2.3.2]).
3. The operation that needed the durable state proceeds, or reports success, only after the flush returns.

After step 2 every slot that survives a crash names the new state, whichever of the nine states of §8.2 the crash
leaves.

### 9.3 Uses of the durable publish

| Use | The change carried by the first publish | Then |
|---|---|---|
| the two-slot barrier before a file deletion, an extent retirement or the reuse of an extent file ([AR §4.2], decision (c) of [60 §2.5], [80 §2.3.2]) | none: the maintenance's `Checkpoint` was published earlier, and the barrier runs only after it passed its identity check ([F16]) | delete, retire or reuse ([F16]) |
| `flags.quiet`, `flags.readonly`, `flags.retired` | the flag | the verb reports success |
| boot-change recovery (§9.4) | `boot_id` | the process reads |

This chapter fixes what the slots hold; [F16] fixes when each use runs and what precedes it.

### 9.4 Boot-change recovery

The recovering process ([AR §4.2], [80 §2.4.3]; the procedure and lock order are [F16]'s) makes a durable publish
(§9.2) whose first publish sets `boot_id` to its own boot identity. It is the only publish that changes `boot_id`. A
process in Unknown-boot mode never runs it (rule U2 of [OS/proc §5]).

### 9.5 Creation and epoch changes

`init` writes the initial slots (§10) into `tmp/head.<nonce>`, makes them durable and moves the file to `HEAD`
([F02 §5.5]; the sequence is [F16]'s). `restore` builds a new store the same way. `restore` and `repair` write a new
`epoch` and `epoch_lsn` into both slots and flush `HEAD` before any process uses the new epoch ([F16]).

## 10. Initial contents

`init` writes the store's first log extent with its epoch-start group ([F05 §4.5]) at lsn 0 and then one durable group
that creates `main` (ref id 0), with the `ClientHead` binding of `main` when the store lies inside a git repository
([F16] P-88 step 5). Both slots then hold the fold of those groups (pass 1, A1-25, S1-45):

| Field | Value |
|---|---|
| `magic`, `format`, `flags` | `"MOIR"`, 1, 0 |
| `slot_seq` | 1 in slot A, 2 in slot B; every other field equal (the two `xxh3_128` values differ with `slot_seq`) |
| `epoch` | a random non-zero `u64` |
| `committed_lsn`, `durable_lsn` | the end of the `main` group, which begins at lsn 138, right after the 138-byte epoch-start group |
| `checkpoint_lsn`, `epoch_lsn` | 0 |
| `boot_id` | `init`'s boot identity, or zero in Unknown-boot mode |
| `config_gen` | 0 |
| `commit_seq`, `fence` | 0 |
| `next_id`, `next_anchor`, `next_file_no` | 1 |
| `active_log` | 1 |
| `n_segments`, `segments` | 0, all zero |
| `refs_lsn` | the lsn of the `RefTable` record of the `main` group |
| `heads_lsn` | the lsn of its `ClientHead` record when `init` bound `main` to the main worktree, else 0 |
| `pins_lsn`, `markers_lsn`, `image_cursor`, `seq_ring` | zero |
| `next_ref_id` | 1 (`main` took ref id 0) |
| `hlc_seq` | the HLC of the `main` group's semantic records ([F05 §10.2]): its `RefUpdate`, and its `ClientHead` when present |
| `hlc_commit` | 0 |
| `init` | the values of [F17 §2.2] IP-5 and a fresh store id ([F02 §4]) |
| `project_oid_algo` | the repository's object format at `init`, or 1 without a repository (§5.16) |

`next_ref_id` also lives, as a fold bound, in [F11 §3.7]'s `REFS.aux`, which equals the covering slot's `next_ref_id` at the
segment's bound and never exceeds it; `HEAD` is the allocator ([F11 §3.7]; pass 1, S1-45).

## Coverage

| Item | Part covered here | Section |
|---|---|---|
| [60 §2.5] [AR] row "`HEAD`": two 4 KiB slots, G18 layout incl. `refs_lsn`/`pins_lsn`/`heads_lsn`/`markers_lsn`, `image_cursor[4]`, `seq_ring`, `flags` bits 0–2, "the store parameters of the row below", other bits reserved-zero | complete | §2–§5, §7 |
| [60 §2.5] audit row "`HEAD`": `durable_lsn`, `boot_id`, `config_gen`, flag bit 3 `retired`; the boot-change recovery rule and the two-slot barrier; the boot-identity rule and Unknown-boot mode; every publish a read-modify-write of the newest slot | the fields, the slot side of the barrier and of boot-change recovery, the publish rule. The protocol steps are [F16]'s; the boot identity is [OS/proc §4]'s | §5.4, §5.5, §5.6, §5.2, §9 |
| [60 §2.5] issue-2 row "Store parameters": init-fixed parameters live in `HEAD` | their place (`InitParams` at 1024, `project_oid_algo` at 1072) and the store id; the parameters and rules IP-1–IP-6 are [F17 §2]'s | §4.4, §5.16 |
| [60 §2.5] "Protocol decisions" (c) and (d) | (d) complete (the torn-slot rule); (c) the slot side of the barrier; the procedure is [F16]'s | §8, §9.2, §9.3 |
| [60 §2.5] "Protocol decisions" (g), (k) | the slot side: boot-change republish; the publish fold and I-G6's monotone fields | §9.1, §9.4 |
| [40] R-6 | complete: `next_anchor u32` at offset 80 (the former reserved `u32` after `next_id`), re-derived from the log | §3.1, §5.7 |
| [80] X-F2 | `HEAD.boot_id` and Unknown-boot mode's rule of never republishing it | §5.5, §9.4 |
| [80] X-F3 | the publish as a read-modify-write of the newest slot, the fields a publish folds, `durable_lsn` and `committed_lsn`; the chain and the protocol are [F05]'s and [F16]'s | §5.4, §9.1 |
| [80] X-F10 | the file-number allocator that keeps every family's numbers unique ([F02] open point 4) | §5.13 |
| [90 §10.1] | none | — |

## Holes

None. No value in this chapter is decided by an M0 measurement. The production values of the `InitParams` fields are
design-fixed values of [F17 §3] (P01, P03, P04). `HOLE(F02-dict-file)` ([F02 §5.1]) decides whether a `dict` entry ever
appears in `segments`.

## Open points for the review

1. **Slot order and packing** (§3; [PLAN §3.3] gap). The G18 field order of [AR §4.2] is kept exactly, byte-packed: the
   fields after `config_gen` are not 8-aligned (offsets 60, 68, 84), and `SegRef` is 29 bytes. [F01 §4.3] allows this;
   the product codec's little-endian types have alignment 1. Re-ordering for alignment was rejected because the design
   gives the order and fixtures are written from it.
2. **`InitParams` at slot offset 1024 with the store id** (§4.4). This closes [F17] OP-17-21 (the block keeps its
   32-byte form, at a fixed offset) and [F02] open point 2 (the store id in `HEAD`). [F17 §2.1]'s former reserved bytes
   16–31 are `store_id`, validated non-zero with IP-2 and kept by IP-1 through `restore` and `repair`; **pass 1
   (A1-13, S1-12, P1-4):** [F17 §2.1] now has this layout, and the `project` root's algorithm is the separate init-fixed
   byte `project_oid_algo` at offset 1072 (§5.16; A1-15, S1-28).
3. **Six fields added in reserved space** (§5.3, §5.13, §5.14, §5.15, §5.16). Conflict recorded with [80 §2.4.3]'s "No
   `HEAD` field is added", which concerns group commit; none of the six changes the group-commit protocol. Pass 1 added
   `project_oid_algo` (an init-fixed value with nowhere else to live) and the HLC maxima `hlc_seq` and `hlc_commit`
   (P1-5: one sequence over the semantic records, kept in the fold so that an append needs no scan below
   `committed_lsn`).
   - `epoch_lsn` fixes where "the first group after an epoch re-roll" is ([80] X-F3): without it a scan starting at the
     re-roll point cannot tell whether to seed with `XXH3-64(epoch)` or with the preceding 8 bytes.
   - `next_file_no` closes [F02] open point 4: a number taken from the directory listing could be reused after `gc`
     deletes a family's highest file.
   - `next_ref_id` keeps ref ids never reused after `gc` drops an expired deleted ref ([AR §4.2]); the alternative, a
     counter in the `REFS` section ([F11]), is equivalent, and the review may move it there.
4. **What `segments` lists** (§4.1; closes [F17] OP-17-06). Only `main`'s base, its deltas, the one yield delta of
   [F16] P-98 and the dictionary: C-4 (`2 + P14 + n_other ≤ 8`) holds with `n_other` = 1 and P14 ≤ 5, the range of
   [F17 §3] (pass 1, P1-9; closure NC-4). `hist`, `blobs`, `gitmap`, `cs` and promoted-branch files are named by records and
   folded registries: [F09 §14.4] `FILES` (see [F05] open point 8).
5. **`active_log` is the oldest unretired extent**, not the extent being appended to (§5.8). This follows [80 §2.4.3]'s
   "a covered `Checkpoint` sets … `active_log`": only a checkpoint's retirement changes it. The append extent follows
   from `committed_lsn`.
6. **`seq_ring` as a ring indexed by `seq mod 32`** (§5.12). The design says "recent commits". A ring keeps each publish
   O(covered commits) with no shifting, and a reader validates entries against `commit_seq`.
7. **`image_cursor` with more than four destinations** (§5.11). The cursor is a cache; a pair without an entry is exported
   from the `gitmap` walk ([AR §5b.6] step 2). Destination numbers are declared by `GitMap` records ([F05 §9.7]).
8. **`readonly`** (§5.2). [AR §4.2] names the bit without a meaning or a verb. Proposed: write verbs, maintenance and GC
   exit 7 while it is set; reads are served; an administrative verb sets and clears it by a durable publish. The CLI
   names the verb; the text is [F19 §10.2] `readonly_flag`.
9. **Fields kept in `HEAD` and their durability** (§6, §9.2). `quiet`, `readonly` and `retired` must survive an OS
   crash, so they change by a durable publish shaped like the barrier. `config_gen` needs no flush (§5.6). This keeps the
   commit path free of `HEAD` flushes.
10. **Semantic failures stop the store** (§7). A slot whose checksum matches but whose fields break the rules of §7 is a
    writer defect; the store exits 7 instead of falling back to the other slot, as [F17 §2.2] IP-2 already does for the
    `init` block. Both slots absent after three reads is exit 7 for `repair` ([F15] OP-1).
11. **Initial contents** (§10; [F02] open point 14). **Pass 1 (A1-25, S1-45):** [F16] P-88 has `init` write the
    epoch-start group and one group creating `main`, so §10 gives `next_ref_id` = 1, `refs_lsn` ≠ 0 and, with a binding,
    `heads_lsn` ≠ 0; slot A has `slot_seq` 1 and slot B 2.
12. **A zero `boot_id`** means "none recorded" (§5.5): a process that can read its boot identity treats it as a
    mismatch and runs boot-change recovery once, which is harmless and records its boot.
13. **Table pointers use 0 for "none"** (§5.10), relying on [F05 §4.5]'s rule that lsn 0 always holds the epoch-start
    group.
