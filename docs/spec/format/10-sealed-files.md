# 10 — Sealed files

| | |
|---|---|
| Title | Sealed files: the sealed-file kinds and which of them carry `SegHdr`; the codec byte and the compressed-payload encoding; `hist` files (frames of at most `store.hist-frame-commits` commits and `store.hist-frame-bytes` raw bytes, split frames, the frame header of [50] F8, the commit index); `blobs` files (body blobs and R-9's fingerprint blob class, keyed through `FPRINT`); `dict.<D>`; `gitmap` pages; the sealed-file side of `cs.<n>`; what retirement and the body purge leave out of these files for a dropped body (spec sync 3). Codec values, the dictionary form and where bodies are compressed are named holes decided by measurement 6 |
| Chapter | [F10], `docs/spec/format/10-sealed-files.md` |
| Status | draft, pass 1 pending |
| Work package | WP-13a (the `10-sealed-files.md` part of WP-13, [PLAN §3.2] item 1), author role R-SPEC-R |
| Sources | [AR §4.1] (rows `hist.NNNN`, `blobs.NNNN`, `dict.D`, `gitmap.NNNN`, `cs.NNNN`; the rules paragraph: sealing, `total_len`, CL3); [AR §4.3] (bodies in the tail raw with a codec byte; bulk commits; "if the M0 measurement shows the tail must hold compressed bodies…"; ≈ 1.5–2.5× `hist` ratio); [AR §4.4] (`SegHdr` `seg_kind` `hist`, `blobs`; `BLOBTAB`); [AR §4.7] (one reused 64 KiB body buffer); [AR §4.8] ("commit id → lsn: per-frame fan-out index in `hist`"); [AR §4.9] (delta checkpoint sealing bodies; tiered fold and rollup merging blob files; dictionary retraining; history retirement; GC: `hist` rewrite, blob GC, `gitmap` compaction); [AR §4.6] "Reserved in format v1" (byte-bounded `hist` frames; the harness list's codec items); [AR §5a.6] (a cold `hist` frame decodes in ≤ 0.1–0.5 ms); [AR §5b.6] (export frontier through `gitmap`); [AR §2.6] T6 (bodies ≤ 64 KiB, content-addressed BLAKE3-128); [AR §2.10] T10 (the pure-Rust codec); [40 §2.5] (fingerprints: ≈ 300 B, runtime, retention), [40 §2.11] R-9; [50 §8.1] F8; [80 §2.5] rules 1–4, [80 §3.1] X-F6; [90 §10.1] (codec row), [90 §11.3] (options (1)–(4) and the decision rule), [90 §11.4]; [60 §2.5] ([AR] row "Segments"; audit rows "Segments" and "Commit body"; the harness row), [60 §3.1] "Decisions fixed at M0 exit", [60 §5.2] row 6; [71 RAM-B1] (≤ 2 MB per commit decode), [71 RAM-m3] (`gitmap` sorted with a fan-out table), [71 RAM-M6] (≤ 0.5 MB compression context); [PLAN §3.2] WP-13, WP-54, [PLAN §3.3] (WP-13's gaps: `gitmap` entry size 41 vs 50 B; which sealed files carry `SegHdr`; R-9's fingerprint blob class), [PLAN §6.2] R3; spec sync 3: [AR §11] #33 and OQ-A-7 with [F06 §8.1] DB-8, DB-9 and [F16] P-80, P-101, P-102; external: [RFC 8878] (zstd frames and dictionaries), [LZ4-block] (the LZ4 block format) |
| Depends on | [F01], [F02], [F09]; cites [F04], [F05], [F06], [F11], [F13], [F14], [F15], [F16], [F17], [F19], [F20], [OS/fs], [OS/map] |

## 1. Scope

This chapter specifies the sealed files other than the graph segments: `hist.<n>`, `blobs.<n>`, `dict.<D>` and
`gitmap.<n>`, and the sealed-file properties of `cs.<n>` (whose sections are [F09 §16.4]'s). It fixes:

- the inventory of sealed files, the kind registries they use, and which of them carry `SegHdr` (§2);
- the codec byte, the compressed-payload encoding and the frame formats inside it (§3);
- `hist` files: what retirement keeps, frame boundaries and split frames, the frame header, the commit index (§4);
- `blobs` files, including R-9's fingerprint blob class (§5);
- dictionary files (§6) and `gitmap` pages (§7);
- `cs.<n>` as a sealed file (§8), and validation (§9).

The values that measurement 6 decides — which codec values exist, which codec each structure uses, the dictionary form and
whether the log tail holds compressed bodies — are named holes (§3, Holes). Every width, offset and field around them is
fixed here.

`SegHdr`, the section table, the layout classes and the canonical placement rules are [F09 §2]–[F09 §4]'s. The sealing
protocol (create under the final number, `durable+meta`, `seal`, `durable-name`, then the record that names the file) is
[F02 §5.2], [OS/fs §4.4.6, §4.6] and [F16]'s. Log records, which `hist` frames hold, are [F05]'s; the `Commit` body and
its header-only form are [F06]'s; the `FPRINT` row is [F11 §12.8]'s and the `FILES` registry [F09 §14.4]'s.

## 2. Sealed files

### 2.1 Inventory

| family | `FileFamily` ([F11 §2.5]) | header | layout | created by | named by | holds |
|---|---|---|---|---|---|---|
| `seg.base.<G>` | 3 `seg-base` | `SegHdr` | [F09 §16.1] | rollup, repair | `HEAD.segments` ([F04 §4.1]); `PINS` while pinned | `main`'s state and the store-level tables |
| `seg.d<K>` | 4 `seg-delta` | `SegHdr` | [F09 §16.2] | delta checkpoint, runtime-only fold, tiered fold | `HEAD.segments`; `PINS` while pinned | a change of `main` and of the store-level tables |
| `seg.b<ref_id>.<K>` | 5 `seg-branch` | `SegHdr` | [F09 §16.3] | promotion | `REFS.promoted_seg` ([F11 §3.1]); `FILES` | a promoted branch's overrides and `TOUCH` |
| `hist.<n>` | 2 `hist` | `SegHdr` | §4 | history retirement, `gc` rewrite, body purge | `FILES` ([F09 §14.4]) | a retired log extent as compressed frames with a commit index |
| `blobs.<n>` | 6 `blobs` | `SegHdr` | §5 | delta checkpoint, runtime-only fold, bulk commit, fold, rollup, blob GC, body purge | `FILES`; `BLOBTAB` entries ([F09 §6.3]) | body blobs and fingerprint blobs |
| `cs.<n>` | 9 `cs` | `SegHdr` | [F09 §16.4] | a bulk commit; a body purge's rewrite (§8) | the `Commit` record's `cs_ref` ([F06 §9]); before it lands, the `Reserve` record that claimed the number ([F05 §9.27]); `FILES` | one bulk commit's changeset |
| `dict.<D>` | 7 `dict` | `DictHdr` (§6) | §6 | `init`, a rollup that retrains, or a body purge (§6.3) | `HEAD.segments` (the current one, `SegRef` kind 3); `FILES` | the compression dictionary, when `HOLE(F02-dict-file)` says one exists |
| `gitmap.<n>` | 8 `gitmap` | `GitmapHdr` (§7) | §7 | checkpoint fold of `GitMap` records, `gitmap` compaction | `FILES` | one page of the git id map |

`HEAD.segments`, `PINS` and `FILES` together name every live sealed file ([F04 §4.1], [F11 §4], [F09 §14.4]).

### 2.2 Kind registries

This chapter defines no registry of its own. The family numbers are [F11 §2.5]'s `FileFamily` (1 `log` … 9 `cs`), which
`SegHdr.seg_kind` uses (values 2, 3, 4, 5, 6 and 9 only, [F09 §2.1]), and so do `PINS` ([F11 §4]) and `FILES`
([F09 §14.4]). `HEAD`'s `SegRef` has its own three-value `kind` ([F04 §4.1]: 1 `base`, 2 `delta`, 3 `dict`) (OP-10-16).

### 2.3 Which sealed files carry `SegHdr`

Gap of [PLAN §3.3], closed here (OP-10-01): `seg.base`, `seg.d`, `seg.b`, `cs`, `hist` and `blobs` begin with `SegHdr`
([AR §4.4] lists `hist` and `blobs` among the `seg_kind` values and adds `changeset`; [80 §2.5] rule 4 names "segments,
`cs`, `hist`, `blobs`"). `gitmap` pages and `dict` files have their own headers, because [80 §2.5] rule 4 gives each its own
("the `gitmap` page header carries it"; "each `dict` file begins with a 32-byte header"). `LOCK`, `HEAD`, `config` and log
extents are not sealed files.

### 2.4 Rules common to every sealed file

- **Length.** Every sealed file's header states its exact length, `total_len u64` ([80] X-F6). Before mapping, a reader
  checks the header's magic and checksum with one read, then the file size against `total_len` ([OS/map §4]); a mismatch
  re-reads `HEAD` and retries once, then exits 7 naming the file and `moirai doctor --fsck` ([F19]).
- **Immutability.** A sealed file is complete, durable and read-only on disk before any record names it; it is never
  truncated, extended, renamed over or reused, and its number is never used again in its family ([F02 §5.2], [F02 §6.2],
  [80 §2.5] rules 2–3).
- **Mapping.** Sealed files are mapped read-only and whole, from offset 0; a fault inside a mapping ends the process with
  exit 7 ([80 §2.5] rules 1, 6; [F15] FM-9). `blobs` files are mapped lazily, on the first body read ([AR §4.1]).
- **Self-identification.** A sealed file's header names its own number (`file_no` in `SegHdr` and `GitmapHdr`); `dict`
  files are named by their number alone (§6.3).
- **Canonical encoding.** Everything outside a compressed payload is encoded uniquely by the rules of this chapter and
  [F09 §17.3]. A compressed payload is the output of the codec implementation the product pins; the format oracle treats it
  as opaque at M0 ([PLAN §6.2] R3) and decodes it from M1 ([90 §10.2]) (OP-10-12).

## 3. Codec

### 3.1 The codec byte

A `u8` naming how a payload is compressed ([90 §10.1] "the codec byte's values"). It appears in every compressed-payload
encoding (§3.2) and in every `hist` frame header (§4.3).

| value | name | payload |
|---|---|---|
| 0 | `none` | the raw bytes, uncompressed |
| other | — | the values HOLE(F10-codec-values) admits, with the meanings its candidates give |

- Value 0 is fixed: every option of [90 §11.3] keeps uncompressed bodies in the log tail or as a fallback ([AR §4.3] "a
  body travels in the log tail raw, with a codec byte"), and the fingerprint class never compresses (§5.3).
- A value not admitted by the filled hole is invalid ([F01 §5.4]).
- *(Informative)* The candidate numbering, fixed in the Holes table so that fixtures after the fill need no renumbering:
  1 `lz4`, 2 `lz4-dict`, 3 `zstd`, 4 `zstd-dict`.

### 3.2 `BlobEnc`: the compressed-payload encoding

A payload in a `blobs` file (a body or a fingerprint) is encoded as:

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `codec` | `u8` | always | §3.1 |
| 2 | `data` | the rest of the payload | always | codec 0: the raw bytes; otherwise the codec's own format (§3.3) |

The payload's length and its raw length are stored by whoever points at it: `BlobRef.len` (the codec byte included) and
`BlobRef.raw_len` ([F09 §6.3]), or `BlobIdx` (§5.2). `BlobEnc` therefore carries no length of its own, and a `BlobRef` needs
no codec field. A body carried by a log record is not a `BlobEnc`: [F06 §8]'s body entry holds `hash`, `codec`, `raw_len`
and `data` as fields of its own, with the codec values of §3.1 and the formats of §3.3.

**Decoding.** A decoder refuses the payload, which makes the structure that points at it invalid, if: the codec value is not
admitted; the codec needs a dictionary that the containing `blobs` file does not name (§5.1); the decoded length differs
from the stated raw length; for codec 0, `len − 1` differs from the raw length; or the decoded bytes' BLAKE3-128 differs from
the blob's `hash` (§5.2). A decoder writes at most the stated raw length and reads no byte outside the payload.

### 3.3 Frame formats of the candidate codecs

These are the formats each candidate value would admit. Only the values HOLE(F10-codec-values) admits are part of format v1.

| candidate | format of `data` | dictionary |
|---|---|---|
| 1 `lz4` | one LZ4 block ([LZ4-block]): a sequence of LZ4 sequences, no frame header, no checksum; it decodes to exactly the stated raw length | none |
| 2 `lz4-dict` | one LZ4 block compressed with the raw-content dictionary of the containing file's `dict_no` as preceding history (the last 65,536 bytes of the dictionary are addressable by match offsets) | a raw-content `dict.<D>` (§6.2) |
| 3 `zstd` | exactly one zstd frame ([RFC 8878] §3.1.1) with no dictionary (`Dictionary_ID` absent or 0); no skippable frame; `Frame_Content_Size`, if present, equals the raw length | none |
| 4 `zstd-dict` | exactly one zstd frame whose `Dictionary_ID` equals the containing file's `dict_no`, decoded with the formatted dictionary of that `dict.<D>` | a formatted zstd dictionary (§6.2) |

The decoders are pure Rust in the product ([90 §11.2]): `lz4_flex` for 1–2, `ruzstd` for 3–4; the oracle has an own LZ4 block
decoder and the `zstd` CLI ([90 §11.3]).

### 3.4 Which codec each structure uses

| structure | codec | fixed or hole |
|---|---|---|
| body blob sealed into `blobs` (§5.3) | HOLE(F10-blob-codec), or 0 when the codec's output is not shorter than the raw bytes | hole (measurement 6) |
| fingerprint blob (§5.3) | 0 | fixed (OP-10-05) |
| `hist` frame and its blocks (§4.3) | HOLE(F10-hist-codec), or 0 for a frame whose compressed blocks are not together shorter than its raw bytes | hole (measurement 6); never a dictionary codec ([90 §11.3]: "frames without a dictionary") |
| a body carried by a log record ([F06 §8]) | 0, or also HOLE(F10-blob-codec) if HOLE(F10-body-placement) puts compressed bodies in the tail | hole (measurement 6) |
| changeset segments, graph segments, `gitmap`, `dict` | not compressed | fixed: mapped in place ([AR §4.3] "readers map it as one more delta layer") |

The fallback to 0 is decided per payload by comparing lengths, so it is deterministic.

### 3.5 Dictionaries

- A dictionary codec (candidates 2 and 4) uses the dictionary of the `dict.<D>` whose number the containing `blobs` file's
  `SegHdr.dict_no` names ([F09 §2.1]). All dictionary-coded payloads of one `blobs` file use that one dictionary; a file with
  `dict_no` = 0 holds none.
- A merge (a tiered fold or a rollup) that copies a payload from a `blobs` file with another `dict_no` re-encodes it; a
  payload whose codec needs no dictionary is copied byte for byte.
- The form of the dictionary is HOLE(F10-dict-form), which equals `HOLE(F02-dict-file)` ([F02 §5.1]).

## 4. `hist` files

### 4.1 Content

`hist.<n>` holds the records of one retired log extent ([AR §4.1], [AR §4.9] "History retirement"), compressed in frames,
with a frame directory and a commit index. Retirement keeps every record of the extent in `lsn` order, byte for byte as
[F05] wrote it (its `RecHdr` included), except the rotation padding and dropped bodies:
- the `Noop` group that pads the extent after its last group ([80 §2.4.3], [F05 §4.4] G-3) is not kept;
- a body entry ([F06 §8]) of a `Commit` record whose hash is in the dropped set of the retirement's scanned log
  ([F06 §8.1], [F11 §13.4]) is not kept (spec sync 3). The record keeps every other byte: its other entries in their
  order (BD-1 still holds), `n_bodies` decreased by the entries removed, and `RecHdr.len` and `RecHdr.xxh3_64`
  recomputed ([F05 §3.1], §3.4). Its `RecHdr.lsn` is unchanged, so `HCIDX` and every lsn that names it stay valid, and
  the record is valid without the entries ([F06 §8.1] DB-9). A retirement in a purge's publish ([F16] P-101 steps 6–7)
  also writes, in a bulk `Commit` whose `cs.<n>` that purge rewrote, the `cs_ref` (`file`, `len`, `b3`) of the
  rewritten file (§8).

Nothing else is dropped by retirement ([AR §4.9] "nothing is dropped by default"). In particular the extent's first
record, its `ExtentHead` (kind 28, [F05 §4.5], §9.28), is kept as the first record of the `hist` file's first frame.
Its `chain_in` is the chain value at the extent's first byte (the trailer of the previous extent's pad, or
`XXH3-64(epoch)` at an epoch start), so the chain value that this extent's own dropped pad held survives as the next
extent's `ExtentHead.chain_in`. The head also carries the epoch's `epoch_lsn`, the `init`
parameters, `project_oid_algo` and the counters and HLC maxima a slot-less `repair` starts from ([F16] P-85, P-97;
pass 1, P1-8). A bulk commit's `Commit` record keeps its `cs_ref`, and the `cs.<n>` it names stays alive with it
([AR §4.9], §8); a purge that rewrites that `cs.<n>` updates the `cs_ref` (§4.6). A `BodyDrop` record ([F05 §9.29]) is
kept like every other record.

*(Informative)* Records other than commits — ref moves, leases, markers, idempotency results, checkpoints with their per-ref
`lsn` lists, pins, client heads, the lazy runtime records — stay readable, which reflog, `op log`, overlay builds of old
branches and `doctor` need.

### 4.2 Frames

Retirement cuts the kept records into **frames**. With P03 = `store.hist-frame-commits` and P04 = `store.hist-frame-bytes`
from `HEAD`'s `InitParams` ([F17 §2.1], [F17 §4.3]):

1. Records are taken in `lsn` order. A frame holds whole records only.
2. Before adding record r to the current frame, the frame is closed if it is not empty and either the frame's raw bytes plus
   `len(r)` would exceed P04, or r is a `Commit` record and the frame already holds P03 `Commit` records.
3. A record longer than P04 is a frame of its own: its **split frame** consists of ⌈len(r) / P04⌉ blocks, every block but
   the last holding exactly P04 raw bytes ([AR §4.1], [71 RAM-B1]).
4. Every other frame is one block.

The raw bytes of a frame are the concatenation of its records. A reader never needs P03 or P04: the frame header states the
counts and every block's lengths ([F17 §1.4] SP-R2). A block's raw length never exceeds 1,048,576, the upper bound of P04's
range, so decoding one commit never needs more than two such buffers ([AR §8.3] RAM "≤ 2 MB").

This rule counts records, not commits, for the byte bound, and counts commits for the count bound. It is the one frame
rule: [F17 §4.3] gives P03's and P04's values and cites this section for the cut (OP-10-03; pass 1, S1-29, P1-30).

### 4.3 Sections of a `hist` file

`SegHdr` ([F09 §2.1]) with `seg_kind` = 2 (`hist`), `tok_ver` = 0 and the fields of [F09 §2.3]'s `hist` column, then
exactly three sections:

| tag | name | class ([F09 §4]) | count |
|---|---|---|---|
| `0x0300` | `HFRAMES` | fixed table of `FrameHdr`, `row_w` = 88, key `first_lsn` | frames |
| `0x0301` | `HCIDX` | commit index (§4.4) | `Commit` records |
| `0x0302` | `HDATA` | the frames' bytes, contiguous in `HFRAMES` order | frames |

**`FrameHdr`** (the frame header of [50] F8):

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 8 | `u64` | `first_lsn` | `lsn` of the frame's first record |
| 8 | 8 | `u64` | `last_lsn` | `lsn` of the frame's last record |
| 16 | 8 | `u64` | `first_seq` | `seq` of the frame's first `Commit` record; 0 if it holds none ([50] F8) |
| 24 | 8 | `u64` | `last_seq` | `seq` of its last `Commit` record; 0 if none ([50] F8) |
| 32 | 8 | `u64` | `first_append_hlc` | `append_hlc` ([F06], [50] F14) of its first `Commit` record; 0 if none ([50] F8) |
| 40 | 8 | `u64` | `last_append_hlc` | `append_hlc` of its last `Commit` record; 0 if none ([50] F8) |
| 48 | 8 | `u64` | `off` | absolute file offset of the frame's block table in `HDATA` |
| 56 | 4 | `u32` | `stored_len` | the frame's bytes in `HDATA`: block table plus block payloads |
| 60 | 4 | `u32` | `raw_len` | the frame's raw bytes: Σ of its blocks' raw lengths |
| 64 | 4 | `u32` | `n_records` | records in the frame, ≥ 1 |
| 68 | 4 | `u32` | `n_commits` | `Commit` records in the frame |
| 72 | 2 | `u16` | `n_blocks` | ≥ 1; > 1 only for a split frame (§4.2 rule 3) |
| 74 | 1 | `u8` | `codec` | the codec of every block of the frame (§3.4): 0 or HOLE(F10-hist-codec); never a dictionary codec |
| 75 | 1 | `u8` | `_reserved` | reserved-zero |
| 76 | 4 | `u32` | `_reserved` | reserved-zero |
| 80 | 8 | `u64` | `raw_xxh3` | XXH3-64, seed 0, over the frame's raw bytes; checked after every decode |
| total | 88 | | | |

Frames are ascending and disjoint in `lsn`: `first_lsn` of frame i + 1 is after `last_lsn` of frame i. `HFRAMES` rows are
sorted by `first_lsn` strictly ascending (the fixed-table key).

**A frame in `HDATA`**, at `off`: the block table, `n_blocks` entries of 8 bytes, then the blocks' payloads in order.

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 4 | `u32` | `stored_len` | the block's payload length |
| 4 | 4 | `u32` | `raw_len` | the block's raw length, 1 to 1,048,576 |
| total | 8 | | | |

A block's payload is the codec's `data` (§3.3) for that block's raw bytes, with no codec byte (the frame header carries it);
with codec 0 it is the raw bytes. Blocks decode independently. `FrameHdr.stored_len` = 8 × `n_blocks` + Σ block
`stored_len`; `FrameHdr.raw_len` = Σ block `raw_len`. Frames follow each other in `HDATA` with no gap, the first at the
section's first byte, so every `off` is determined.

### 4.4 `HCIDX`: the commit index

The index `id16 → lsn` of every `Commit` record the file holds ([AR §4.1], [AR §4.8]).

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 1,024 | `[256]u32` | `fan` | `fan[b]` = the number of entries whose `id16[0] ≤ b`; `fan[255]` = n |
| total | 1,024 | | | |

Then n entries of 24 bytes, sorted by `(id16, lsn)`, `id16` bytewise:

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 16 | `b16` | `id16` | the first 16 bytes of the commit id ([F01 §5.6], [F06]) |
| 16 | 8 | `u64` | `lsn` | the `lsn` of its `Commit` record |
| total | 24 | | | |

`len` = 1,024 + 24n. The entries of the first-byte value b are `[fan[b − 1], fan[b])` (with `fan[−1]` = 0), searched in place
([71 RAM-m3]).

### 4.5 Lookups

- **Commit id → record:** the `HCIDX` of each `hist` file (a caller knows from `from_lsn`/`upto_lsn` which files can hold an
  `lsn`), then the frame whose `[first_lsn, last_lsn]` contains the `lsn` (binary search in `HFRAMES`), then a decode of that
  frame and a scan of its records by `RecHdr.lsn`.
- **`lsn` → record:** the file whose `[from_lsn, upto_lsn)` contains it, then as above.
- **`seq` → record and time → `seq`:** binary search over `HFRAMES` by `first_seq`/`last_seq` or by
  `first_append_hlc`/`last_append_hlc` ([50] F8: "`s<seq>` revisions, `changes(since:)`, time → seq"), then a frame decode.
  `append_hlc` is monotonic in `seq` order (I43′), so both searches are valid.

### 4.6 `gc` and purge rewrites

`moirai gc` rewrites `hist` files to drop unreachable commits older than `gc.cruft-delay` ([AR §4.9], [F17 §11.2]). A
rewrite writes a new `hist` file under a new number and never changes an existing one:

- it keeps every record it does not drop, byte for byte, except that a dropped commit whose header is kept (the default;
  `--prune-headers` drops it) is replaced by [F06 §4.4.15]'s header-only form of its `Commit` record (presence bit
  `pruned`, `n_ops` = `n_bodies` = 0, `changeset_digest` and every other header field kept), with `RecHdr.len` and
  `RecHdr.xxh3_64` recomputed ([F05]); a bulk commit's pruned form drops `cs_ref`, so its `cs.<n>` leaves `FILES` with
  the rewrite (§8);
- it leaves out every body entry whose hash is dropped, as retirement does (§4.1; spec sync 3);
- it cuts frames by §4.2 again and rebuilds `HCIDX`; a header-only commit stays in `HCIDX`, a dropped one leaves it;
- the group chain trailers of [80] X-F3 are a rule of the log and are not verified inside `hist`; the frame's `raw_xxh3`,
  the section checksums and `seg_digest` protect `hist` bytes.

**The purge rewrite** ([F06 §8.1] DB-8, DB-9; [F16] P-101 step 6; spec sync 3). The body purge reads every live `hist`
file and rewrites, by the rules above and under a new number, each one that holds a `Commit` record with a body entry
whose hash it purges, or a bulk `Commit` whose `cs.<n>` it rewrote (§8): the body entries of those hashes are left out as
§4.1 states, and the bulk commit's `cs_ref` (`file`, `len`, `b3`) names the rewritten `cs.<n>`, with `RecHdr.len` and
`RecHdr.xxh3_64` recomputed. It drops no commit and changes no other byte; the replaced file is released by the purge's
`Checkpoint` ([F05 §9.9]).

`hist` file numbers are their own family ([F02 §6.2]); they do not equal the number of the retired extent, and `from_lsn`
and `upto_lsn` say which log range a file covers (OP-10-14).

## 5. `blobs` files

### 5.1 Header

`SegHdr` with `seg_kind` = 6 (`blobs`), `tok_ver` = 0, `n_rows` = the number of blobs, `dict_no` = the dictionary its
dictionary-coded payloads use (0 if none), and the other fields of [F09 §2.3]'s `blobs` column.

### 5.2 Sections

Exactly two:

| tag | name | class | count |
|---|---|---|---|
| `0x0310` | `BLOBIDX` | fixed table of `BlobIdx`, `row_w` = 36, key `(hash, class)` | blobs |
| `0x0311` | `BLOBDATA` | the payloads, contiguous in `BLOBIDX` order | blobs |

**`BlobIdx`:**

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 16 | `b16` | `hash` | BLAKE3-128 of the blob's raw bytes |
| 16 | 1 | `u8` | `class` | the blob class (§5.3) |
| 17 | 3 | `[3]u8` | `_reserved` | reserved-zero |
| 20 | 8 | `u64` | `off` | the absolute file offset of the blob's `BlobEnc` payload |
| 28 | 4 | `u32` | `len` | the payload's length, codec byte included |
| 32 | 4 | `u32` | `raw_len` | the raw length |
| total | 36 | | | |

- `(hash, class)` is unique within the file. The same blob may exist in several `blobs` files; a `BlobRef` names one.
- `BLOBDATA` is the concatenation of the payloads in `BLOBIDX` order, the first at the section's first byte, with no gap;
  every `off` is determined.
- A `BlobRef` ([F09 §6.3]) that points into this file carries the same `hash`, `off`, `len` and `raw_len` as its `BlobIdx`
  row. A reader follows a `BlobRef` directly; `BLOBIDX` serves content-addressed lookups, merges, GC and `doctor --fsck`.
  A dropped `BlobRef` (`file` = 0, [F09 §6.3]; spec sync 3) points into no file and has no `BlobIdx` row.

### 5.3 Blob classes ([40] R-9)

| value | name | raw bytes | codec | where the reference lives |
|---|---|---|---|---|
| 1 | `body` | a node body, ≤ 65,536 bytes ([AR §2.6], [F17 §13.2]), after the store's CRLF → LF normalisation (I40′) | §3.4: HOLE(F10-blob-codec) or 0 | `BLOBTAB` of a graph segment ([F09 §6.3]), through `NodeHdr.body_ref` |
| 2 | `fingerprint` | a fingerprint value ([F20 §2.6.4]): 20 + 4 × `n_sketch` bytes, ≤ 276 | 0 | `FPRINT` ([F11 §12.8]): the content's `oid` → the blob's `hash`, found through §5.5 |

Values 0 and 3–255 are reserved.

- **The fingerprint class** is R-9's "blob class 'fingerprint' in `blobs.NNNN`, keyed through `FPRINT`". Its content address is
  BLAKE3-128 of the fingerprint value's bytes, so equal fingerprints of two contents share one blob. The `oid` → blob link is
  `FPRINT`'s row; this class has no key of its own beyond its hash. A fingerprint is runtime data: never versioned, hashed or
  exported (I-F4), and retained only while its `oid` is the current or latest-observed content of a live file node
  ([40 §2.5], [F11]).
- A fingerprint blob of a resolver version other than the store's current one is absent for the resolver ([F20 §1.3]).
- Fingerprints are written by the fold that seals `FPrint` records: a runtime-only fold or a delta checkpoint ([F09 §15.1]),
  into the same `blobs` file as the bodies that fold seals, or a file of their own when no body is sealed.

### 5.4 Where `blobs` files come from

- A **delta checkpoint** seals the bodies of its window into one new `blobs` file, compressing them here ([AR §4.9]).
- A **bulk commit** writes its bodies into a `blobs` file before its changeset segment ([AR §4.3] "bodies straight to
  `blobs`"); both are sealed before the `Commit` record, under the numbers its `Reserve` record claimed ([F05 §9.27],
  [F16] P-84).
- A **tiered fold** merges the `blobs` files of the deltas it folds into one; a **rollup** merges all of them ([AR §4.1],
  CL3); **blob GC** writes a file without the blobs that no live row, retained history or ref references ([AR §4.9]). Each
  merge writes a new file under a new number and re-encodes payloads only as §3.5 requires.
- A **body purge** ([F16] P-101 step 3; spec sync 3) writes, for every live `blobs` file (named by the segment set, by
  `FILES` or by a pinned set's files, [F11 §4]) that holds a blob of class `body` whose hash it purges, a replacement
  under a new number without those blobs; every other blob is copied, re-encoded only as §3.5 requires. When the purge
  replaces the dictionaries (§6.3), it also rewrites every `blobs` file whose `dict_no` names a replaced one, against its
  replacement. The purge's `Checkpoint` names the replacements and releases the replaced files ([F05 §9.9]).
- **Dropped bodies** ([F06 §8.1] DB-7, DB-8; spec sync 3). No `blobs` file holds a blob of class `body` whose hash was in
  its writer's dropped set when it was written: a seal, merge or blob GC leaves such a blob out ([F16] P-80), and the
  purge removes the ones that files written before the drop still hold. Fingerprint blobs are never dropped.
- Canonical order: a writer places blobs in `BLOBIDX` key order, so the file is a function of its blob set, `dict_no` and
  the codec (§2.4).

### 5.5 Finding a blob by content address

- **A body** is found by its `BlobRef`: through `NodeHdr.body_ref` and `BLOBTAB`, or, for a hash alone, by a binary search
  of `BLOBTAB` in the layers of the view after the tail records that carry bodies ([F09 §6.3], [F06 §8] BD-6). Before
  either, the reader looks the hash up in the dropped set of its view ([F11 §13.4], [F06 §8.1] DB-6, [F16] P-102): a
  dropped hash resolves to no bytes, even while a `blobs` file still holds them, and a dropped `BlobRef` is never followed
  (spec sync 3).
- **A fingerprint** is found by its `FPRINT` row ([F11 §12.8]), which names the `blobs` file and the `hash`: one binary
  search for `(hash, 2)` in that file's `BLOBIDX`, so a lookup opens one file however many `blobs` files the store holds
  ([F09] OP-09-14 adopted; pass 1, P1-22). A row whose file is not live, or whose file holds no such entry, is a defect
  that `doctor --fsck` reports; the resolver then treats the fingerprint as absent and recomputes it ([F20 §1.3]).
- A `blobs` file that no live structure names is garbage; GC removes it from `FILES` and deletes it ([F16]).

## 6. `dict.<D>`

### 6.1 `DictHdr`

The 32-byte header of [80 §2.5] rule 4, `{magic "MDIC", total_len u64, blake3_16, _ [4]}`:

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 4 | `[4]u8` | `magic` | `"MDIC"` = `4D 44 49 43` |
| 4 | 8 | `u64` | `total_len` | the file's exact length ([80] X-F6) |
| 12 | 16 | `b16` | `digest` | BLAKE3-128 over `[32, total_len)`, the dictionary bytes |
| 28 | 4 | `[4]u8` | `_reserved` | reserved-zero |
| total | 32 | | | |

The header has no format field; the store's format version applies ([F01 §9.1]). A reader verifies `digest` when it loads
the dictionary (≤ 110 KiB, [AR §4.1]), before any payload is decoded with it (OP-10-15).

### 6.2 Content

The bytes after the header are the dictionary in the form HOLE(F10-dict-form) fixes:

- **raw content** (option (1) of [90 §11.3] with a dictionary): plain bytes that `lz4_flex` blocks use as preceding history;
  at most 65,536 bytes ([AR §4.1] "≤ 64 KiB with LZ4");
- **formatted zstd dictionary** (option (3)): a dictionary in the format of [RFC 8878] §5, beginning with its magic number,
  whose `Dictionary_ID` equals D; 32–110 KiB ([AR §4.1]). The upper bound is a validity rule (§9): at most 112,640 bytes,
  which a reader's load budget relies on (§6.1, OP-10-15). The lower bound is the trainer's target size and informative:
  a smaller trained dictionary is valid (spec sync 2b). *(Informative)* D ≥ 1, and a D of at most 32,767 (or of at least
  2^31) lies in the ranges [RFC 8878] §5 reserves for dictionaries registered for public distribution; store
  dictionaries are never distributed, which is the private use the RFC allows, so the reservation does not apply;
- **absent**: no `dict` file exists ([F02 §5.1]), and no payload uses a dictionary codec.

### 6.3 Lifetime and naming

- A dictionary is trained at `init`, retrained at a rollup ([AR §4.1], [F17 §6.3]) or replaced by a body purge, and written
  as a new `dict.<D>` under a new number ([F02 §6.2]). A dictionary is never rewritten.
- **Dropped bodies** (spec sync 3). No training sample holds a body in the trainer's dropped set ([F16] P-80, [F17 §6.3]).
  Since a trained dictionary may hold fragments of its sample, a body purge replaces every live dictionary by one trained
  on a sample without dropped bodies, and the `blobs` files coded with a replaced one are rewritten (§5.4; [F16] P-101
  step 3).
- A `dict.<D>` stays alive while any live `blobs` file has `dict_no` = D ([F16], GC).
- The **current** dictionary — the one a new seal and a merge that re-encodes use — is the `dict` entry of `HEAD.segments`
  ([F04 §4.1], `SegRef` kind 3), whose `blake3_16` is `DictHdr.digest`. Every live dictionary, the current one included, is
  also listed in `FILES` ([F09 §14.4]), because a `blobs` file of a pinned set may still name an older one (OP-10-06).

## 7. `gitmap.<n>`

### 7.1 One page per file

A `gitmap.<n>` file is one sealed **page** of the git id map: the entries of one (destination, algorithm) pair, sorted by
commit id with a fan-out table, probed in place and never loaded into a per-process hash set ([AR §4.1], [71 RAM-m3]). The
entries not yet folded are `GitMap` log records ([F05]); a checkpoint folds them into new pages, one per (destination,
algorithm) with entries, and `gitmap` compaction merges pages ([AR §4.9]). The map is store-level runtime state ([AR §5d.1]).

**Page bound** (pass 1, P1-22). The live pages of a pair, ordered by file number, are tiered like `main`'s segment set
([F17 §6.1]): with P14 = `store.fold-width`,

- a rollup writes one page per pair, holding every entry of the pair's pages, and releases the others;
- a checkpoint that folds `GitMap` entries of a pair that already has 1 + P14 live pages writes one page holding the
  entries of every page but the oldest, together with the new entries, and releases the pages it merged; otherwise it
  adds one page.

So a pair never has more than 1 + P14 pages, and a lookup opens at most that many whatever the store's age; while a long
maintenance job holds the maintenance byte, its yield checkpoints merge no page but the one page per pair that the same
holding's earlier yield checkpoint wrote (which the new one replaces and releases), so the bound is 2 + P14 until the next
ordinary checkpoint ([F16] P-98; pass 1, P1-9). A merge
copies entries (a commit maps to one git id per pair, §7.3, so entries never conflict) and writes a new file number; the
`Checkpoint` record names the new page in `added` and the merged ones in `released` ([F05 §9.9]).

### 7.2 `GitmapHdr`

The `gitmap` page header of [80 §2.5] rule 4:

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 4 | `[4]u8` | `magic` | `"MGMP"` = `4D 47 4D 50` ([F01 §4.5]) |
| 4 | 2 | `u16` | `format` | format version, 1 ([F01 §9.1]) |
| 6 | 1 | `u8` | `dest` | the image destination's number, as `HEAD.image_cursor` uses it ([AR §4.2]); numbers are declared by `GitMap` records ([F04] open point 7, [F05]) |
| 7 | 1 | `u8` | `algo` | the git object format of the destination: 1 `sha1` or 2 `sha256` ([F01 §7.5]); 0 is invalid |
| 8 | 4 | `u32` | `n` | entries |
| 12 | 4 | `u32` | `file_no` | the number in the file's name |
| 16 | 8 | `u64` | `total_len` | the file's exact length: 1,072 + n × w (§7.3) |
| 24 | 16 | `b16` | `digest` | BLAKE3-128 over `[48, total_len)` |
| 40 | 8 | `u64` | `hdr_xxh3` | XXH3-64, seed 0, over `[0, 40)` |
| total | 48 | | | |

### 7.3 Fan-out and entries

At offset 48, `fan: [256]u32` (1,024 bytes): `fan[b]` = the number of entries whose `id16[0] ≤ b`; `fan[255]` = n. At offset
1,072, n entries of w bytes, w = 16 + the digest width of `algo` ([F01 §7.5]): **36 bytes for `sha1`, 48 for `sha256`**.

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 16 | `b16` | `id16` | the first 16 bytes of the moirai commit id |
| 16 | 20 or 32 | `b20` or `b32` | `git_oid` | the git commit id this commit has in the destination ([AR §5b.4]) |
| total | 36 or 48 | | | |

Entries are sorted by `id16` strictly ascending. One moirai commit has one git id per (destination, algorithm): the git id
is a function of the commit's content, its parents' git ids and the object format ([AR §5b.4]), so a SHA-1 and a SHA-256
destination give it two, one in each page ([D §5b]). A lookup reads the fan-out range and binary-searches it, in every page
of the pair (newest first) and then the tail.

**Entry size** (gap of [PLAN §3.3], OP-10-02). [AR §4.1] lists the entry as `(commit_id16, dest u8, algo u8, git_oid[32])`,
which is 50 bytes, and sizes it at 41 bytes. This chapter moves `dest` and `algo` into the page header and stores the digest
at its own width: 36 bytes per entry for SHA-1 (within the 41-byte sizing of [AR §8.1]'s `gitmap` row) and 48 bytes for
SHA-256.

## 8. `cs.<n>` as a sealed file

The sections of a changeset segment are [F09 §16.4]'s. As a sealed file:

- It is built as `tmp/cs.<nonce>` ([F02 §5.3]), written, made durable (`durable+meta`), moved to `cs.<n>` by
  `rename_noreplace`, and both names are made durable (`durable-name` on `tmp/` and on the store directory) before the
  `Commit` record that names it ([F02 §5.2] rule 2, [AR §4.3], [80 §2.3.2]); the file is sealed before the rename or after
  it, as [OS/fs §4.4.6] orders.
- Its `SegHdr.file_no` is n, chosen before the rename; `total_len` and `seg_digest` are final when it is sealed.
- The `Commit` record's `cs_ref {file, len, b3}` ([AR §4.3], [F06 §9] BK-2) names it: `file` = n, `len` = `total_len`,
  and `b3` = `seg_digest[0..16]`, the first 16 bytes of the digest its own header records and the value `FILES.digest16`
  holds ([F06 §9] BK-2, [F09 §14.4]; pass 1, S1-19, P1-6, A1-21).
- It is never compressed: readers map it as one more delta layer ([AR §4.3]).
- It lives while the `Commit` record that names it is kept (in the log or in a `hist` file, §4.1); the next checkpoint folds
  its rows into a delta, but the file stays the commit's changeset for history: its rows (the state after the commit,
  with `PREV`), its `VIOLATIONS` and its `CKIMG` ([F09 §16.4]). A bulk commit stores no op list; history, `revert`, `blame`
  and `show` read it as a state delta against its base state ([F06 §9] BK-5; pass 1, A1-22, P1-27, S1-35). Before its
  `Commit` lands, the file is named by the `Reserve` record that claimed its number ([F05 §9.27], [F16] P-84), or by a
  `FILES` row with the `reserved` flag once a fold covers that record ([F09 §14.4]). A `cs` file no record names is an
  orphan that the sweep removes ([AR §4.3] "the crash state 'segment flushed, record not'", [F16]); a
  `gc` rewrite that keeps only the pruned header of its commit (§4.6) releases it.
- **The purge rewrite** ([F06 §8.1] DB-8; [F16] P-101 step 5; spec sync 3). A body purge rewrites every live `cs.<n>`
  of a bulk `Commit` below its forced rotation ([F16] P-101 step 1) whose `BLOBTAB` holds a live `BlobRef` of a hash it
  purges, or names a `blobs` file it replaced (§5.4), as a new `cs.<n′>` under a new number, built through `tmp/` as
  above: the `BLOBTAB` holds dropped `BlobRef`s for the purged hashes and points every other entry at its replacement
  file ([F09 §6.3], §16.4), and every other section keeps its bytes. The `hist` file that the purge writes for the
  commit's record (§4.1, §4.6) carries the new `cs_ref` (`file` = n′, `len`, `b3`), and the purge's `Checkpoint` names
  `cs.<n′>` and releases `cs.<n>` ([F05 §9.9]). A `cs.<n>` named by a bulk `Commit` appended after a purge's forced
  rotation is never rewritten by that purge: the commit's record stays in the log, so no `hist` file of this pass
  carries it and its `cs_ref` cannot follow a rewrite. If it names a `blobs` file the purge replaces, the purge starts
  again at its step 1 ([F16] P-101 step 7), whose new rotation puts the commit below the retirement line, and the
  commit's `cs_ref` follows through the `hist` file of the later pass. No codec value is needed: a changeset segment is
  never compressed.

## 9. Validation

A sealed file of this chapter is invalid when any check fails; the consequences are [F09 §17.2]'s (exit 7 on open for a file
a process needs, `doctor --fsck` and `repair --rebuild-from-log` otherwise; a `blobs` or `hist` file is derived, [AR §4.10]).

| file | open checks (before mapping or use) | full checks (`doctor --fsck`, the oracle, a merge reading it) |
|---|---|---|
| `hist`, `blobs` | V-1 to V-8 of [F09 §17.1]; exactly the sections of §4.3 or §5.2 | V-9, V-10; `HFRAMES` continuity, ascending disjoint frames, block tables summing to `FrameHdr`, `off` chaining, `n_records`/`n_commits` against the decoded records, `first_*`/`last_*` against them, `raw_xxh3`; `HCIDX` fan-out, order and completeness; `BLOBIDX` order and uniqueness, `off` chaining, every payload decodes (§3.2) and hashes to `hash`; a `Commit` record without some of its body entries is valid (§4.1, [F06 §8.1] DB-9), and which dropped hashes a file may still hold is [F13] I-D1's check |
| `dict` | magic; `total_len` against the file size; reserved bytes zero | `digest` (also checked at load, §6.1); the content is in the form HOLE(F10-dict-form) fixes: raw content of at most 65,536 bytes, or a formatted dictionary of at most 112,640 bytes that begins with the [RFC 8878] §5 magic number and whose `Dictionary_ID` equals D (§6.2) |
| `gitmap` | magic; `format` ([F01 §9.1]); `hdr_xxh3`; `algo` ∈ {1, 2}; `file_no` against the name; `total_len` = 1,072 + n × w and against the file size | `digest`; `fan` consistent with the entries; entries strictly ascending |
| `cs` | as graph segments ([F09 §17.1]) | as graph segments |

A decode of a `hist` frame also checks `raw_xxh3` at run time; a mismatch is exit 7 naming the file and `moirai doctor
--fsck`, because the frame's bytes are not the ones sealed (OP-10-08).

## Coverage

| Item | Part covered here | Section |
|---|---|---|
| [60 §2.5] [AR] row "Segments": "`hist` frames with commit index; `blobs` frames and dictionary; `gitmap` pages" | complete, with the codec values, dictionary form and body placement as holes | §3–§7 |
| [60 §2.5] audit row "Segments": "`hist` frames ≤ 256 commits and ≤ 1 MiB raw" | complete: the frame rule and split frames; the two values are [F17 §4.3]'s | §4.2 |
| [60 §2.5] audit row "Commit body": `cs_ref`, the `cs.NNNN` changeset segment | the sealed-file lifecycle and the `cs_ref` link; the sections are [F09 §16.4]'s, the header fields [F06]'s | §8 |
| [60 §2.5] harness row: "the codec chosen by M0 item 6 among pure-Rust options (codec byte values, frame formats, `dict.D` as raw content or absent, or a formatted zstd dictionary)" | complete as named holes with every alternative laid out | §3, §6, Holes |
| [90 §10.1] codec | complete as above | §3, §6 |
| [50] F8 | complete: `first_seq`, `last_seq`, `first_append_hlc`, `last_append_hlc` in the frame header, and the lookups they serve; the overlay's `seq → lsn` is process state, not format | §4.3, §4.5 |
| [40] R-9 | complete: the fingerprint blob class, its content and codec, and how `FPRINT`'s `hash` ([F11 §12.8]) finds it through `FILES` | §5.3, §5.5 |
| [80] X-F6 | `total_len` in the `hist` and `blobs` `SegHdr`, the `gitmap` page header and the `dict` header, and the check before mapping; `SegHdr` itself is [F09 §2]'s, the mapping policy [OS/map]'s | §2.4, §6.1, §7.2 |
| [60 §2.5] issue-2 row "Store parameters": `hist` frame size | how P03 and P04 cut frames; the values are [F17]'s | §4.2 |
| [AR §11] #33 and OQ-A-7 (bodies droppable by hash; spec sync 3) | the sealed-file side: body entries left out at retirement and in `hist` rewrites, the purge's replacement `blobs`, `dict` and `cs.<n>` files, the reader's dropped-set check; the purge's steps are [F16] P-101's, the end state [F06 §8.1] DB-8's | §2.1, §4.1, §4.6, §5.4, §5.5, §6.3, §8 |

## Holes

| id | what | decided by | candidates | constraint the value must meet |
|---|---|---|---|---|
| `F10-codec-values` | the codec values admitted in format v1 besides 0 `none`, and the payload format of each (§3.1, §3.3) | measurement 6 (WP-54), filled by WP-81a ([60 §3.1] "the codec among pure-Rust options", [90 §11.3]) | numbering fixed for every candidate: 1 `lz4`, 2 `lz4-dict`, 3 `zstd`, 4 `zstd-dict`, formats as §3.3. The admitted set is {0} ∪ {`F10-blob-codec`} ∪ {`F10-hist-codec`}: option (1) with a dictionary {0, 2, 3} (expected); option (1) without a dictionary {0, 1, 3}; option (2) {0, 3}; option (3) {0, 3, 4}; with `lz4` chosen for `hist`, 1 replaces 3 in the set. Values outside the admitted set stay reserved | exactly the values `F10-blob-codec` and `F10-hist-codec` name, plus 0 (`F10-body-placement` adds none); a dictionary value only if `F10-dict-form` is not "absent"; every admitted value decodable by a pure-Rust product decoder and by the oracle ([90 §11.2–§11.3]) |
| `F10-blob-codec` | the codec a seal writes for body blobs (§3.4) | measurement 6 (WP-54) | 2 `lz4-dict` (option (1), expected); 1 `lz4` (option (1) without a dictionary); 3 `zstd` (option (2)); 4 `zstd-dict` (option (3)) | [90 §11.3]'s decision rule: option (1) unless a disk, page-cache or body-read budget of [AR §8.1]/[AR §8.3] fails with it and option (3), measured by the `zstd` CLI proxy, meets it; option (2) only if a dictionary gains < 1.2× over none on the owner's bodies; body decode in µs within one reused 64 KiB buffer ([AR §4.7]) |
| `F10-hist-codec` | the codec retirement writes for `hist` frames (§3.4, §4.3) | measurement 6 (WP-54) | 3 `zstd` (expected, [90 §11.3]); 1 `lz4` ("the first lever" if `ruzstd`'s encoder misses a budget, [90 §11.4]) | never a dictionary codec; a 1 MiB frame decodes within the as-of and history budgets (est. 2–7 ms for `ruzstd`, [90 §11.4]; a cold frame ≤ 0.1–0.5 ms per [AR §5a.6] for typical frames); retirement and rollup compression within their budgets (rollup ≤ 3 s at 1e6, [AR §4.9]) |
| `F10-dict-form` | the dictionary form of `dict.<D>` files (§6.2) | measurement 6 (WP-54) | absent; raw content (≤ 64 KiB); formatted zstd dictionary (32–110 KiB, `Dictionary_ID` = D) | equals `HOLE(F02-dict-file)`; "absent" iff `F10-blob-codec` is not a dictionary codec; raw content iff it is 2; formatted iff it is 4 |
| `F10-body-placement` | whether bodies in the log tail may be compressed (§3.4) | measurement 6 (WP-54: "where bodies are compressed (log tail raw or compressed)") | **raw**: a body in a log record always has codec 0 and is compressed only when a checkpoint seals it (the design's default, [AR §4.3]); **compressed**: a body in a log record may also carry `F10-blob-codec`, encoded with that codec's smallest-window parameters and a by-reference dictionary | "compressed" only if the measurement shows the tail must hold compressed bodies, and then no writing process builds a compression context above 0.5 MB ([AR §4.3], [71 RAM-M6]); [F17 §5.1]'s `body_bytes` counts what this decides |

`F02-dict-file` ([F02]) and `F10-dict-form` are one decision recorded in two chapters; WP-81a fills both together. [F17]'s
`store.dict.*` parameters are inert when the form is "absent" ([F17 §6.3]).

## Open points for the review

- **OP-10-01 (which sealed files carry `SegHdr`; gap of [PLAN §3.3]).** Segments, `cs`, `hist` and `blobs` do; `gitmap` and
  `dict` do not (§2.3). The alternative — `SegHdr` on every sealed file — would give `dict` files a header [80 §2.5] rule 4
  does not have, and a 120-byte header whose `seq` and `lsn` fields mean nothing for a dictionary or a `gitmap` page.
- **OP-10-02 (`gitmap` entry 41 vs 50 B; gap of [PLAN §3.3]).** The fields [AR §4.1] lists make 50 bytes; its sizing says 41.
  Resolved by one page per (destination, algorithm): `dest` and `algo` move into the header, and entries are 36 bytes (SHA-1)
  or 48 bytes (SHA-256). SHA-1 meets the 41-byte sizing; SHA-256 exceeds it by 7 bytes, which [AR §8.1]'s `gitmap` row
  should state at WP-81a ("36 B/entry SHA-1, 48 B SHA-256"). The 32-byte slot of [F01 §7.5] was not used here, because
  entries of one page share one algorithm.
- **OP-10-03 (the frame rule counts records) — closed in pass 1** (S1-29, P1-30, A1-46). [AR §4.1] bounds frames by
  commits and raw bytes. A retired extent also holds non-commit records, which retirement keeps ([AR §4.9] "nothing is
  dropped"). This chapter counts every record's bytes toward P04 and only `Commit` records toward P03, and applies the
  split rule to any record above P04; [F17 §4.3] now cites §4.2 for the rule and keeps only the values.
- **OP-10-04 (one commit index per file).** [AR §4.1] says "a per-frame commit index (`id16 → lsn`)", [AR §4.8] "per-frame
  fan-out index". A fan-out table per frame of at most 256 commits costs 1 KiB per frame and saves nothing over one index
  per file whose entries give the `lsn`, from which the frame follows by binary search. This chapter keeps one `HCIDX` per
  file. The review should confirm the reading.
- **OP-10-05 (codec 0 as a fallback; fingerprints uncompressed).** A seal or a retirement writes codec 0 when the chosen codec
  would not shrink the payload; the rule compares lengths, so it is deterministic. Fingerprint blobs (≤ 276 bytes of hash
  values) are always codec 0: compression cannot shrink them and a context would cost more than the blob.
- **OP-10-06 (the current dictionary; closed with [F04]).** A seal must know which `dict.<D>` is current. [F04 §4.1] lists it
  as the `dict` entry of `HEAD.segments` (closing [F17] OP-17-06). Older dictionaries that `blobs` files of pinned sets still
  name are live too; `FILES` lists every live dictionary so GC and `doctor` can find them.
- **OP-10-07 (the codec byte lives in the payload).** `BLOBTAB`'s entry ([AR §4.4]) has no codec field. Putting the codec
  byte first in each payload (`BlobEnc`) keeps that entry as designed and makes a log-tail body and a sealed body the same
  encoding; the `len` of a `BlobRef` includes the byte.
- **OP-10-08 (`raw_xxh3` per frame; an added field).** The section checksum covers stored bytes but is not checked on every
  read. A frame is decoded on every history read; checking XXH3-64 of ≤ 1 MiB after decompression costs ≈ 0.1 ms and turns a
  corrupted frame into exit 7 instead of wrong history.
- **OP-10-09 (`dest` numbering).** `gitmap` and `HEAD.image_cursor` use a `dest u8`. [F04] open point 7 says destination
  numbers are declared by `GitMap` records ([F05]); [F14] (WP-15) names the destinations. This chapter only requires that a
  number is never reused for another destination, since `gitmap` pages outlive a destination's configuration.
- **OP-10-10 (header-only commits) — closed in pass 1** (S1-21, A1-5). [F06 §4.4.15] defines the form (presence bit
  `pruned`, `n_ops` = `n_bodies` = 0, the digest kept); §4.6 writes it.
- **OP-10-11 (`cs` lifetime).** A changeset segment outlives the checkpoint that folds its rows, because its rows,
  `VIOLATIONS` and `CKIMG` are the bulk commit's changeset for history (§8; pass 1, A1-22: an earlier text named an
  `OPS` section, which [F09 §16.4] does not have). GC removes it when no kept record names it. [F16] should state this
  with the orphan sweep rule of [F02] open point 5.
- **OP-10-12 (determinism of compressed bytes).** Byte-identical rebuilds of `hist` and `blobs` hold only for one pinned codec
  implementation and version; a codec upgrade changes compressed bytes, never raw content, ids or `hash`es. The format oracle
  treats compressed payloads as opaque at M0 ([PLAN §6.2] R3).
- **OP-10-13 (zstd frame constraints).** A payload is exactly one zstd frame, with no skippable frame, and a stated
  `Frame_Content_Size` must equal the raw length. Whether a frame carries a content checksum is left to the encoder, since
  `raw_xxh3` and the blob `hash` already verify content.
- **OP-10-14 (`hist` numbers).** `hist` numbers are a family of their own ([F02 §6.2]); a `gc` rewrite gets a new number, so a
  `hist` number cannot equal the retired extent's `log` number. The covered range is `SegHdr.from_lsn`/`upto_lsn`.
- **OP-10-15 (dictionary integrity).** `DictHdr` has no header checksum ([80 §2.5] rule 4 fixes its 32 bytes). Its content
  digest is checked at every load, which reads at most 110 KiB once per process, so a damaged dictionary never decodes a
  payload.
- **OP-10-16 (kind registries reconciled).** An earlier draft of this chapter defined its own sealed-file kind registry.
  [F11 §2.5]'s `FileFamily` (drafted in parallel, for `PINS`) already numbers every file family, and [F11] open point 11
  asks [F09] and [F04] to reuse it. `SegHdr.seg_kind` now takes `FileFamily` values; `HEAD`'s `SegRef.kind` keeps [F04]'s
  three values, which name only what `HEAD.segments` may hold. The review may unify the two; nothing here depends on it.
- **OP-10-17 (`hist` and `blobs` tags).** [F11 §2.8] proposed `0x0201`–`0x021A` for the runtime tables, so this chapter's
  sections moved to `0x0300`–`0x0302` (`hist`) and `0x0310`–`0x0311` (`blobs`), registered in [F09 §3.1].
- **OP-10-18 (lookups bounded by file count; pass 1, P1-22).** A fingerprint lookup opens the one `blobs` file its
  `FPRINT` row names (§5.5), and a pair's `gitmap` pages are tiered by `store.fold-width` (§7.1), so neither lookup grows
  with the store's age. [F17 §6.1] states P14 for segments and, since pass 1, for `gitmap` pages too (1 + P14, or
  2 + P14 while a long job yields). Closed.
- **OP-10-19 (the anchor record of P1-8) — closed in pass 1.** Review pass 1 (P1-8) asks that retirement keep, in `hist`,
  a durable anchor record written at every epoch start and at the start of every extent. That record is [F05 §9.28]'s
  `ExtentHead` (kind 28, the first group of every extent, [F05 §4.5]; [F16] P-97), and §4.1 keeps it by the existing rule
  (every record but the rotation pad), as the first record of each retired extent; the chain value a dropped pad's
  trailer held is the next extent's `ExtentHead.chain_in`. No byte of this chapter changes.
- **OP-10-20 (spec sync 2b, WP-20).** [AR §4.1]'s "32–110 KiB" for a formatted dictionary was stated neither as a rule nor
  as informative. The upper bound is now a full check (§9), since the load budget of §6.1 and OP-10-15 relies on it; the
  lower bound stays the trainer's target, because no reader depends on it and a rule would make a store whose corpus
  trains a smaller dictionary invalid. An informative note records that a small D falls in [RFC 8878] §5's registered
  range, which does not apply to dictionaries that are never distributed (§6.2).
- **OP-10-21 (spec sync 3; [AR §11] #33, OQ-A-7 (a), decided 2026-10-06).** Retirement and every `hist` rewrite leave
  out the body entries of dropped hashes and recompute `n_bodies`, `RecHdr.len` and `RecHdr.xxh3_64`, keeping
  `RecHdr.lsn` (§4.1, §4.6); the record stays valid by [F06 §8.1] DB-9, and the chain trailers are not verified inside
  `hist`, so nothing else moves. The body purge writes replacement `blobs` files without the purged blobs (§5.4),
  replaces every live dictionary (§6.3, as [F16] P-101 step 3 decided), rewrites the `cs.<n>` files that name a purged
  hash or a replaced `blobs` file under new numbers (§8), and rewrites the `hist` files that hold such entries or such
  a bulk commit, with the new `cs_ref` (§4.6). No codec value is reserved: a dropped body is marked in `BLOBTAB`
  ([F09 §6.3]), not in a payload. "Dropped" in §4.6's first bullet (a commit `gc` drops) and in a dropped body are two
  different removals; the second never removes a record.
