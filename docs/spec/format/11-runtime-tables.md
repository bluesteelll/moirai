# 11 — Runtime tables

| | |
|---|---|
| Title | Runtime tables: the store-level runtime state folded into segment sections — `REFS` (with the absorbed vectors, the move cache and the promotion counters `overlay_ops` and `overlay_bytes`), `PINS`, `HEADS` (client heads and the R-15 bindings), `LEASES` ([90 §10.1]), `MARKERS` and `MARKERS_OLD`, `IDEM`, `ALLOC` (F17) and `UIDX`; the versioned index sections `CONFLICTS` (F11) and `GLOBIDX`; R4's runtime tables `TREES` (with `VolumeCaps`, the settle epochs and the dirty row), `FILEOBS` (R-18), `PENDING`, `FSINTENT`, `FPRINT`, `JOURNALCUR`, `DIRMAP`, `PREFIXEV`, `ANCESTRY` and `GITRENAMES` (`GITFACTS`), `ANCHORRES`; the session tables `CURSORS` and `SESSMARKS` and the `BACKUPS` registry; the row images log records carry; the embedded `OsFileId` per OS (X-F8) |
| Chapter | [F11], `docs/spec/format/11-runtime-tables.md` |
| Status | draft, pass 1 pending |
| Work package | WP-13b, the runtime-table part of WP-13 ([PLAN §3.2] item 1), author role R-SPEC-R |
| Sources | [AR §2.16] (T16: what is store-level runtime); [AR §3.4] I1, I14′, I17′, I26′, I27′, I36′; [AR §4.2] (the ref-table paragraph with `overlay_ops`/`overlay_bytes` and the absorbed vector; `next_id`, `fence`; `refs_lsn`, `pins_lsn`, `heads_lsn`, `markers_lsn`); [AR §4.3] (record kinds; commit-header fields `ref_id`, `ref_seq`, `idem_key`, `idem_payload`, absorbed vector); [AR §4.4] (section rows `IDEM`, `LEASES`/`MARKERS`/`MARKERS_OLD`, `REFS`/`PINS`/`HEADS`, `ANCESTRY`, "R4 link sections", "R5 sections"; the `derived-optional` flag); [AR §4.5] steps 4 (markers from net ops), 6 (replay by record kind), 8 (`ALLOC`, `UIDX`), 12 (runtime-only fold); [AR §4.6] ("Not hashed"); [AR §4.8] (indexes); [AR §4.9] (inert markers at the fold, pins, GC of the runtime tables); [AR §5a.1] (object model: ref, reflog, client head, tag, pin, runtime records); [AR §5a.2] (`ref_seq`, the 32 cached moves); [AR §5a.3] (fork pins, promotion and its counters, sync); [AR §5a.4] (client keys, bindings, R-15); [AR §5a.5] (`undo` and the absorbed vector; marker recomputation); [AR §5a.7] steps 7–8 (absorbed vector at merge; staging refs); [AR §5a.9] (`branch -d`/`-D`); [AR §5d.1] (the versioned-versus-runtime table); [AR §6.2] (leases, holder anchor, deadline, renewal); [AR §6.4] (idempotency); [AR §6.5] (durability classes). [40 §2.1], [40 §2.5] (`FPRINT` retention), [40 §2.6] (runtime tables, stat quadruple, OS id paragraph), [40 §2.9] (states), [40 §2.11] R-7, R-8, R-9, R-15, R-18 (authoritative), [40 §3.4] (`FsIntent`, recovery table), [40 §5.1], [40 §5.3] (tree identity, writer tree, expected ref). [50 §8.1] F11, F17. [80 §2.7.1], [80 §2.7.2] (deadline form, anchors), [80 §2.10] P9, [80 §2.11.1]–[80 §2.11.3] (capabilities, tagged layouts, frontier), [80 §3.1] X-F2, X-F5, X-F7, X-F8. [90 §4.1] (the environment-lease binding rule), [90 §4.3] (lease kinds), [90 §4.4] (anchors per harness), [90 §10.1] (rows "`LEASES` runtime rows", "Holder anchor (X-F2)"). [60 §2.5] rows "Segments" ([AR] rows and audit rows), "Ref table", "`Vfs`/`ProjectFs`", "Cross-platform", "Harness-agnostic interface and pure Rust", and the R4 rows R-7, R-8, R-9, R-15, R-18 and the R5 rows F11, F17. Audits [70] S1, S4, S5, S7, S8, S17; [72] B2, M4, M7. Reviews `docs/spec/reviews/a1-P.md` A1P-04, A1P-11, A1P-16 and `a1-S.md` S-09 (dispositions in the open points). [PLAN §3.2] WP-13; [PLAN §3.3] (the `MARKERS` field set) |
| Depends on | [F01], [F02]; cites [F03] (`Anchor`), [F04] (`HEAD` counters and the `*_lsn` pointers), [F05] (record kinds and payloads), [F06] (commit header, the `Conflict` op key), [F07] (key classes), [F08] (value encodings, schema), [F09] (`SegHdr`, section tags, the versioned delta rule), [F10] (the fingerprint blob class), [F12] (ref names, conflict classes), [F13] (I26′ and the marker-cache rules MC-1–MC-7), [F14] (export of refs), [F16] (protocol, folds), [F17] (retention and promotion parameters), [F18] (strings, the `relink` vocabulary, I-F12), [F19] (exit codes), [F20] (the resolver), [OS/proc], [OS/clock], [OS/project], [OS/path], [API], [CFG] |

## 1. Scope

### 1.1 What this chapter owns

This chapter gives the byte layout of every runtime table the store folds into segment sections, and of two versioned
index sections that the work package assigns here (`CONFLICTS`, `GLOBIDX`). For each table it fixes the row layout,
the sort order and key, the section body around the rows (§2), how a reader finds the current table across the segment
set and the log tail (§2.4), the value registries its enumerations use, the rules that maintain its computed fields,
and its retention.

It does not own:
- the log record kinds whose replay produces the rows, their frames, their durability tags and every payload field that
  is not a row image ([F05]; the class of each source record is listed in §1.3 for orientation only). Where a payload
  carries a row of this chapter, its bytes are the row image of §2.9, which this chapter owns;
- the segment header, the section directory, section tags and alignment, and the delta rule of versioned sections
  ([F09]; the tags this chapter proposes are in §2.8);
- the protocol steps that append, fold and publish ([F16]); the store parameters that trigger folds, promotions and
  expiries ([F17]);
- the meaning of the resolver fields that `FILEOBS`, `ANCHORRES` and `TREES` record ([F20]);
- the `Anchor` layout ([F03]), `ProcId` ([OS/proc §3.1]), the deadline `Stamp` ([OS/clock §3.1]) and the per-OS sources
  of `OsFileId`, `FsTime`, `FileAttrs` and `VolumeCaps` ([OS/project §3–§4]).

### 1.2 Runtime state

**Runtime** state is store-level: one table for the whole store, visible from every branch, never versioned, never
merged, never part of a canonical form and never exported, except where [F14] exports refs and, with `--with-oplog`,
heads and the reflog ([AR §5d.1], [AR §4.6] "Not hashed", I36′, I-F4). **Versioned** sections (`CONFLICTS`, `GLOBIDX`)
belong to a view and follow [F09]'s rules for base, delta, branch and changeset segments.

Only writer paths append the records that runtime rows come from: verbs, settles, file verbs and evidence hooks. Reads
append nothing (I-F5).

### 1.3 Table map

"Form" names the section form in a base segment and in a delta segment (§2.4). Row sizes are in bytes. The record kinds
are [F05]'s; their durability tags are [F05]'s too ([80] X-F5) and are given here as the design states them.

| Table | Section(s) | Class | Form (base / delta) | Key and sort order | Folded from ([F05]) | Source records | Derived-optional | Row | § |
|---|---|---|---|---|---|---|---|---|---|
| refs | `REFS` | runtime | snapshot / snapshot | `ref_id` (+ name index) | `Commit` (implied ref move), `RefUpdate`, `RefTable` | durable | no | 164 | 3 |
| pins | `PINS` | runtime | snapshot / snapshot | `FileRef` | `Pin` | durable | no | 21 | 4 |
| client heads and bindings | `HEADS` | runtime | snapshot / snapshot | (`kind`, `key`) | `ClientHead` | durable | no | 161 | 5 |
| leases | `LEASES` | runtime | snapshot / snapshot | (`n`, `lease_id`) | `Lease` | durable; renewals and heartbeats lazy ([AR §6.2]) | no | 180 | 6 |
| active markers | `MARKERS` | runtime | snapshot / snapshot | (`n`, `ref_id`, `commit`, `emit_lsn`) | `Marker` | durable | no | 72 | 7 |
| inert markers | `MARKERS_OLD` | runtime | snapshot / layer | as `MARKERS` | moved from `MARKERS` at folds | — | no | 72 | 7 |
| idempotency | `IDEM` | runtime | snapshot / layer (hash tables) | `key` | `Commit` (`idem_key`, `idem_payload`), `Idem` | durable | no | 72 | 8 |
| id allocation | `ALLOC` | runtime | snapshot / layer (dense) | `#N` | `Commit` (`Create` ops); `Reserve` (reserved ids, holes until their commit folds, §9.1) | durable | no | 24 | 9.1 |
| uid index | `UIDX` | runtime | snapshot / layer | `uid` | `Commit` (`Create` ops) | durable | no | 20 | 9.2 |
| conflict values | `CONFLICTS` | versioned | [F09] | (`n`, key bytes) | `Commit` (`Conflict`, `Resolve` ops) | durable | no | 56 | 10 |
| glob index | `GLOBIDX` | versioned | [F09] | (prefix, `n`, `field`, glob) | `Commit` (ops on glob fields) | durable | no | 20 | 11 |
| trees | `TREES` | runtime | snapshot / snapshot | `key` | `TreeReg` | lazy | no | 201 | 12.4 |
| file observations | `FILEOBS` | runtime | snapshot / layer | (`n`, `tree`) | `FileObs` | lazy | no | 187 | 12.5 |
| pending observations | `PENDING` | runtime | snapshot / layer | (`n`, `tree`, from, to) | `Pending` | lazy | no | 123 | 12.6 |
| file-operation intents | `FSINTENT` | runtime | snapshot / snapshot | `intent_id` | `FsIntent`, `FsIntentDone`, `FsIntentAborted` | durable | no | 132 | 12.7 |
| fingerprints | `FPRINT` | runtime cache | snapshot / layer | `oid` | `FPrint` | lazy | yes (proposed) | 54 | 12.8 |
| journal cursors | `JOURNALCUR` | runtime, reserved | snapshot / snapshot | `vol_key` | `JournalCursor` | lazy | no | 41 | 12.9 |
| directory map | `DIRMAP` | runtime | snapshot / layer | (`tree`, directory id) | `DirMap` | lazy | yes ([80 §2.11.2]) | 91 | 12.10 |
| prefix evidence | `PREFIXEV` | runtime | snapshot / snapshot | (`tree`, `root`, from, to) | `PrefixEv` | lazy | no | 52 | 12.11 |
| git facts | `ANCESTRY`, `GITRENAMES` | runtime cache | snapshot / layer | (`algo`, a, b); (`algo`, commit) | `GitFacts` | lazy | yes (proposed) | 67, 98 | 12.12 |
| anchor results | `ANCHORRES` | runtime cache | snapshot / layer | (anchor, `oid`, resolver version) | `AnchorRes` | lazy | yes (proposed) | 78 | 12.13 |
| change-feed and pack cursors | `CURSORS` | runtime | snapshot / layer | (`session`, `agent`, `feed`, `task`) | `Lazy` (`cursor`) | configurable ([F05 §6.1]) | no | 56 | 13.1 |
| session marks | `SESSMARKS` | runtime | snapshot / layer | (`session`, `agent`) | `SessionMark` | configurable | no | 56 | 13.2 |
| backups | `BACKUPS` | runtime | snapshot / snapshot | `dir` | `Backup` | durable | no | 56 | 13.3 |

`PENDING`, `FSINTENT`, `FPRINT`, `JOURNALCUR`, `PREFIXEV`, `ANCESTRY` and `GITRENAMES` are beyond the work package's
list; they are here because [F20], [OS/proc], [OS/project] and review A1P-16 cite this chapter for them (open point 30).
`CURSORS`, `SESSMARKS` and `BACKUPS` are the fold targets [F05 §7] names for cursors, session marks and the backup
registry (pass 1, A1-23; open point 39). `PATHIDX`, `ALIASIDX`, `ANCHORS` and `ANCHOR_UID` (R-8) are versioned sections
of [F09].

## 2. Common encodings

### 2.1 The section body

Every section of this chapter has the same body, which fills the section's `[off, off + len)` range of the segment
([F09]). The body opens with `RtHdr`:

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 4 | `u32` | `n_rows` | number of rows (for `IDEM`: the number of slots, §8) |
| 4 | 2 | `u16` | `row_size` | the row size this chapter gives for the section's tag (§1.3); any other value makes the section invalid |
| 6 | 1 | `u8` | `form` | 1 `snapshot`, 2 `layer` (§2.4); other values invalid |
| 7 | 1 | `u8` | `_reserved` | zero |
| 8 | 4 | `u32` | `aux` | section-specific: `REFS` `next_ref_id` (§3.7); `IDEM` the number of used slots (§8); `ALLOC` the first `#N` (§9.1); zero in every other section |
| 12 | 4 | `u32` | `index_len` | byte length of the index region: `4 × n_rows` for `REFS` (§3.3); zero in every other section |
| 16 | 8 | `u64` | `heap_len` | byte length of the heap (§2.3) |
| total | 24 | | | |

The body is `RtHdr`, then `n_rows` rows of `row_size` bytes (row i at offset `24 + i × row_size`), then the index
region, then the heap, with no gap. The section entry's `len` must equal `24 + n_rows × row_size + index_len + heap_len`;
otherwise the section is invalid. A section with no rows has `n_rows` = 0 and is still written where §2.4 requires it.

### 2.2 Rows, keys and order

- Every row layout has an offset table. Rows are byte-packed ([F01 §4.3]); reserved bytes and bits are zero ([F01 §10]).
- Each section names its key. Rows are sorted by the key in the order of [F01 §6.6] (integers numerically, byte strings
  and texts bytewise), with no two equal keys. A key component held in the heap is compared by its bytes.
- The exceptions are `IDEM` (a hash table, §8) and `ALLOC` (a dense array, §9.1).

### 2.3 The heap and `HeapRef`

Variable-length fields live in the heap. A row refers to them by `HeapRef`:

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 4 | `u32` | `off` | byte offset of the slice from the start of the heap |
| 4 | 4 | `u32` | `len` | byte length of the slice; 0 = empty |
| total | 8 | | | |

- **Slice types.** A `HeapRef` typed `text` addresses the UTF-8 bytes of one string ([F01 §6.1]) with no length prefix.
  A `HeapRef` typed `path` addresses one stored path value of [F08 §5.2] (`root` `u16`, then the path text as a `vstr`),
  so a path always carries its root (pass 1, P1-2). A `HeapRef` typed `list<T>` addresses the concatenation of its
  elements, each encoded as T; a list of fixed-size elements has `len` = count × element size; a list of `vstr`
  ([F01 §6.2]) elements ends at the slice end. A `HeapRef` typed as a sequence table (§12.5, §12.7, §12.12) addresses
  exactly that sequence.
- **Canonical order.** The heap is the concatenation of every slice in row order and, within a row, in the order of its
  `HeapRef` fields, with no gap, no padding and no shared bytes. Each `off` is therefore the sum of the lengths of all
  earlier slices, also when `len` is 0. So a section has exactly one encoding for its content (E3).
- **Bounds.** A reader that follows a `HeapRef` checks `off + len ≤ heap_len`; a slice out of bounds makes the section
  invalid (§2.7).

### 2.4 Forms and lookup across the segment set

Runtime sections appear only in base segments (`seg.base.<G>`) and delta segments (`seg.d<K>`), never in promoted-branch
segments, changeset segments (`cs.<n>`) or sealed files other than segments; the versioned sections are the exception
(below). Two forms exist:

- **`snapshot`** (form 1): the complete table as of the segment's `upto_lsn` ([F09]). Every base segment and every
  delta segment carries every runtime section of §1.3 (every class other than "versioned"), in the form its "Form"
  column gives for that segment kind, even with no rows (open point 27). For a table whose delta form is `snapshot`, the
  copy in the newest segment of the current set is authoritative and older copies are ignored.
- **`layer`** (form 2): only the rows whose value changed after the next-older segment of the set, as of this
  segment's `upto_lsn`. A reader finds the value of a key by probing, in order, the log tail, the delta segments newest
  first, then the base segment; the first row with that key decides. Base segments always carry the snapshot form.
- **Versioned sections.** `CONFLICTS` (§10) and `GLOBIDX` (§11) use this chapter's body in every graph segment kind and
  compose along a view's layers by [F09 §4.7] ("row" and "index"), not by the lookup above. Their `form` is 1
  `snapshot` in a base segment and 2 `layer` in a delta, promoted-branch or changeset segment ([F09] OP-09-13).
- **Dead rows.** In the layered sections `FILEOBS`, `PENDING`, `FPRINT`, `DIRMAP`, `ANCESTRY`, `GITRENAMES` and
  `ANCHORRES`, bit 7 of the row's `flags` byte is `dead`: the key is deleted as of this layer. In a dead row every field
  outside the key and `flags` is zero (enumeration fields included, which are then exempt from their value tables), the
  other bits of `flags` are zero, and every `HeapRef` outside the key has `len` 0 (a key component held in the heap, as
  `PENDING`'s paths, keeps its slice). A dead row never appears in a snapshot. The append-only layered sections
  (`MARKERS_OLD`, `IDEM`, `ALLOC`, `UIDX`) have no dead rows. `PREFIXEV`, a snapshot table, uses bit 7 of its `flags` the
  same way in a log record's delete image only (§2.9).
- **The tail.** Records beyond the `upto_lsn` of the newest segment of the set (the bound is [F09]'s) and below the
  published `committed_lsn` apply on top, per record kind ([F05], [AR §4.5] step 6; [F04]'s `refs_lsn`, `pins_lsn`,
  `heads_lsn` and `markers_lsn` name the newest such record of four tables). Replay indexes lazy runtime records by key
  and decodes them on use ([AR §4.3]).
- **Folds.** A delta checkpoint writes new layers and new snapshots. A tiered fold of d1..d3 merges their layers into one
  (keeping dead rows that shadow base rows). A rollup writes every table as a snapshot and drops dead rows. Retention
  (each section's "Retention" paragraph) is applied by the fold that rewrites the layer holding a row; a fold may also
  write a dead row for a row of a layer it keeps.

### 2.5 Shared field types

| Type | Width | Definition |
|---|---|---|
| `HeapRef` | 8 | §2.3 |
| `OidSlot` | 33 | `algo` (`u8`, the git object-format registry of [F01 §7.5]) followed by the fixed 32-byte slot of [F01 §7.5]; `algo` 0 = none, with 32 zero bytes. Used for content `oid`s ([40] R-1), git blob ids and git commit ids |
| `FileRef` | 9 | a store file: offset 0 `family` (`u8`, `FileFamily` below), offset 1 `ref_id` (`u32`; the `<ref_id>` of `seg.b<ref_id>.<K>` for family 5, else 0), offset 5 `file_no` (`u32`, ≥ 1; [F02 §6.2]) |
| `Stamp` | 24 | the deadline form `{wall u64, boot_hash u64, mono u64}` of [OS/clock §3.1] ([80] X-F2), embedded unchanged |
| `Anchor` | 32 | the holder anchor of [F03] ([80] X-F2 as amended by [90 §4.4]), embedded unchanged |
| `ProcId` | 32 | [OS/proc §3.1], embedded unchanged; diagnostics only |
| `OsFileId` | 57 | §12.1 |
| `FsTime` | 9 | §12.2 |
| `FileAttrs` | 4 | §12.2 |
| `VolumeCaps` | 16 | the snapshot of §12.3 |

`FileFamily` (`u8`): values follow the name families of [F02 §6.3]. This table is the one owner of the values
(pass 1, S1-37): `SegHdr.seg_kind` ([F09 §2.1]), `FILES` ([F09 §14.4]) and the variable-width file reference of
[F05 §8.4] use them. `FileRef` here is the fixed 9-byte form of rows; [F05 §8.4]'s record form `FileRefV` encodes the
same three parts with varints (pass 1, P1-25).

| value | name | files |
|---|---|---|
| 1 | `log` | `log.<n>` |
| 2 | `hist` | `hist.<n>` |
| 3 | `seg-base` | `seg.base.<G>` |
| 4 | `seg-delta` | `seg.d<K>` |
| 5 | `seg-branch` | `seg.b<ref_id>.<K>` |
| 6 | `blobs` | `blobs.<n>` |
| 7 | `dict` | `dict.<D>` |
| 8 | `gitmap` | `gitmap.<n>` |
| 9 | `cs` | `cs.<n>` |

Commit references in rows use `id16` ([F01 §5.6]: the first 16 bytes of the commit id) with the `u64` lsn of the
commit's record where the row needs to read the commit, as the commit header's parents do ([AR §4.3]).

### 2.6 Symbol fields

Symbol ids are store-local ([F01 §8]). Every symbol field of this chapter names its class of [F01 §8.2]:

| Field | Class | Width |
|---|---|---|
| `IDEM.branch_sym` | `ref` | `u32` |
| `LEASES.role` | `role` | `u16` |
| `LEASES.holder`, `MARKERS.actor` | `actor` | `u32` |
| `PREFIXEV.root` | `root` | `u16` |
| `GLOBIDX.field` | `name` | `u32` |

Id 0 is the empty string in every class; in these fields it means "none".

### 2.7 Validation

- **At open** a reader checks `RtHdr` (§2.1) of each section it uses: `row_size`, a `form` allowed for the segment kind
  (§1.3), and the length identity. It checks the bounds of every `HeapRef` it follows (§2.3).
- **Full validation** — reserved bytes and bits zero, enumeration values in their tables, keys sorted and unique, the
  canonical heap order, and the section-specific rules of §3–§13 — is done by the format oracle (WP-95) and by
  `doctor --fsck`, not on every read (the proposal of [F01] open point 6).
- A section that fails either check is invalid, and so is its segment; the consequence is [F09]'s and [F16]'s (the
  segment set is rebuilt from the durable `Checkpoint` records or by `repair --rebuild-from-log`, [AR §4.2], [AR §4.10]).
- **Foreign OS data** ([F01 §3.2], [80] X1). A reader interprets an `OsFileId` only when its `kind` belongs to the
  reader's OS (kinds 1 and 2 to Windows, 3 to Linux, 4 to macOS) and a `TREES` row's OS-specific fields only when its
  `os` tag equals the reader's own. Otherwise the value is absent: a tree behaves as on its first settle, and every
  evidence source that needs the id contributes nothing ([OS/project §4.2], [F20 §4.8]). Such rows are not invalid.

### 2.8 Section tags

The tag values and the section directory are [F09]'s; [F09 §3.1] adopted the values below (open point 28). The
`derived-optional` column is the entry flag bit 0 of [AR §4.4].

| Section | Tag | Derived-optional |
|---|---|---|
| `REFS` | `0x0201` | no |
| `PINS` | `0x0202` | no |
| `HEADS` | `0x0203` | no |
| `LEASES` | `0x0204` | no |
| `MARKERS` | `0x0205` | no |
| `MARKERS_OLD` | `0x0206` | no |
| `IDEM` | `0x0207` | no |
| `ALLOC` | `0x0208` | no |
| `UIDX` | `0x0209` | no |
| `CURSORS` | `0x020A` | no |
| `SESSMARKS` | `0x020B` | no |
| `BACKUPS` | `0x020C` | no |
| `TREES` | `0x0210` | no |
| `FILEOBS` | `0x0211` | no |
| `PENDING` | `0x0212` | no |
| `FSINTENT` | `0x0213` | no |
| `FPRINT` | `0x0214` | yes |
| `JOURNALCUR` | `0x0215` | no |
| `DIRMAP` | `0x0216` | yes |
| `PREFIXEV` | `0x0217` | no |
| `ANCESTRY` | `0x0218` | yes |
| `GITRENAMES` | `0x0219` | yes |
| `ANCHORRES` | `0x021A` | yes |
| `CONFLICTS` | `0x0031` ([F09]'s versioned range) | no |
| `GLOBIDX` | `0x0084` ([F09]'s versioned range) | no |

### 2.9 Row images in log records

Pass 1 adopted open point 33 (A1-6, S1-8, P1-2): where a log record carries a row of this chapter, its bytes are the
row's **image**, so that the fold into a section and the replay of the tail decode one codec and build one row.

- **Image.** The row's fixed bytes as its offset table lays them out, every `HeapRef` holding `off` = 0 and `len` = the
  length of its slice; then the slices, in the order of the row's `HeapRef` fields, with no gap. The image is
  `row_size + Σ len` bytes. A fold copies the fixed bytes, sets each `off` by §2.3 and appends the slices to the heap.
- **Delete image.** A record that deletes a key carries the image of the key's dead row (§2.4): the key fields, `flags`
  bit 7, every other field zero, and the slices of the key's heap components only.
- **Which records.** The row batches of `FileObs`, `Pending`, `DirMap`, `PrefixEv` and `AnchorRes` ([F05 §8.5]): an
  upsert row is the image of a `FILEOBS` (§12.5), `PENDING` (§12.6), `DIRMAP` (§12.10), `PREFIXEV` (§12.11) or
  `ANCHORRES` (§12.13) row, a delete row its delete image; and the set form of `ClientHead` ([F05 §9.3]), which is the
  image of the `HEADS` row (§5). `JournalCursor` already carries the 41-byte `JOURNALCUR` row (§12.9). Records that
  describe events rather than rows — `RefUpdate`/`RefTable`, `Lease`, `Marker`, `Pin`, `FsIntent*`, `TreeReg`,
  `GitFacts`, `FPrint`, `Lazy`, `SessionMark`, `Backup` — keep [F05]'s fields, and each table's section says which
  record field sets which row field, with the enumerations this chapter owns cited by [F05].
- **Enumerations of other chapters.** Rows store link states, anchor states, detail codes and evidence tokens as the
  `u8` codes of [F18 §4.2]–§4.6 and §5.2, which [F18 §4.10] owns; evidence classes as 1 `exact`, 2 `strong`, 3 `copy`,
  4 `weak` ([F20 §1.5]'s order, numbered by [F18 §4.10]); resolver versions as [F20 §1.3]'s `u16`.

## 3. `REFS`: the ref table

### 3.1 Row

One row per ref ever created and not yet expired, sorted by `ref_id` ([AR §4.2], [AR §5a.1]).

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 4 | `u32` | `ref_id` | the ref's id, never reused (§3.7) |
| 4 | 1 | `u8` | `kind` | §3.2 |
| 5 | 1 | `u8` | `flags` | bit 0 `deleted`: the ref was deleted by `branch -d`/`-D` or a staging `--abort`; the row stays until its reflog expires (§3.8); bit 1 `pinned`: a tag created with `--pin` ([AR §5a.9]); other bits reserved-zero |
| 6 | 2 | `[2]u8` | `_reserved` | zero |
| 8 | 16 | `b16` | `tip` | `id16` of the tip commit; zero with `tip_lsn` = 0 for a ref that has no commit yet |
| 24 | 8 | `u64` | `tip_lsn` | lsn of the tip commit's record |
| 32 | 4 | `u32` | `gen` | the tip commit's generation (`gen` of [AR §4.3]); 0 with no tip |
| 36 | 4 | `u32` | `ref_seq_next` | the next `ref_seq` on this ref; starts at 1 and never decreases, also after `undo` (CM1, [AR §5a.2]) |
| 40 | 8 | `u64` | `base_pin` | the pinned checkpoint set of the ref's view: the lsn of the `Checkpoint` record that published it (§4); 0 = none, the view has no pinned set (`main` reads the current set, [AR §5a.3]) |
| 48 | 16 | `b16` | `fork_commit` | `id16` of the commit the ref was forked from; zero for a ref created without a fork (`main` at `init`) |
| 64 | 8 | `u64` | `fork_lsn` | lsn of that commit's record; 0 with no fork |
| 72 | 8 | `u64` | `fork_seq` | `seq` of that commit; 0 with no fork |
| 80 | 4 | `u32` | `fork_ref_id` | the ref whose chain holds the fork commit (the commit's own `ref_id`); 0 with no fork |
| 84 | 4 | `u32` | `ops_since_fork` | §3.6 |
| 88 | 4 | `u32` | `overlay_ops` | §3.6 |
| 92 | 4 | `u32` | `overlay_bytes` | §3.6 |
| 96 | 4 | `u32` | `promoted_seg` | the `<K>` of the ref's newest promoted segment `seg.b<ref_id>.<K>`; 0 = never promoted |
| 100 | 8 | `u64` | `ops_total` | §3.6 |
| 108 | 8 | `u64` | `bytes_total` | §3.6 |
| 116 | 8 | `u64` | `trunk_mark_ops` | §3.6 |
| 124 | 8 | `u64` | `trunk_mark_bytes` | §3.6 |
| 132 | 8 | `HeapRef` | `name` | `text`: the ref name as [F12] spells it (for example `lane/l5np`, `merge/main/from/lane/l5np`, `tags/v1`) |
| 140 | 8 | `HeapRef` | `absorbed` | `list<AbsorbedEntry>` (§3.4) |
| 148 | 8 | `HeapRef` | `moves` | `list<u64>` (§3.5) |
| 156 | 8 | `HeapRef` | `message` | `text`: the tag message of a `tag` ref; empty otherwise |
| total | 164 | | | |

Rules: names are unique among rows without `deleted`; a `deleted` row keeps its name, and a later ref may reuse the name
with a new `ref_id` ([AR §5a.9], [F02 §6.1]).

### 3.2 Ref kinds

| value | name | refs ([AR §5a.1]; names [F12]) |
|---|---|---|
| 1 | `work` | `main`, `lane/*` — everything writable |
| 2 | `plan` | `plan/*` — the write mask of I33′ |
| 3 | `merge` | `merge/<dst>/from/<src>` — staging, written only by the merge machinery |
| 4 | `import` | `import/<n>` — staging, written only by import |
| 5 | `tag` | `tags/<n>` — immutable |
| 6 | `orphans` | `orphans/<n>` — commits parked by a failed ref CAS at replay (I27′); written only by the park of [F16] P-70, a `RefUpdate` with reason 5 that the first appender whose scan meets the failed CAS appends ([F05 §9.2]; §3.9; open point 7) |

### 3.3 Name index

The index region of `REFS` holds `n_rows` `u32` values: the row positions (0-based) of every row, ordered by the row's
`name` bytes and then by `ref_id`. A reader resolves a name by binary search over it. The index is a permutation of
`0 … n_rows − 1`.

### 3.4 The absorbed vector

`AbsorbedEntry` (8 bytes): offset 0 `ref_id` (`u32`), offset 4 `ref_seq` (`u32`). The entries are sorted by `ref_id`
and unique; a missing entry means `ref_seq` 0 (nothing of that ref absorbed). `absorbed_R[X]` is the highest `ref_seq` of
ref X reachable from `tip(R)` ([AR §5d.1]). It is maintained by the rules of [AR §5d.1] and [F13 §4.2] MC-3:
- a commit on R sets `absorbed_R[R]` to its `ref_seq`;
- a fork of Y from X at `ref_seq` f copies X's vector and sets `absorbed_Y[X] = f`;
- a merge or sync of src into dst sets `absorbed_dst[src] = ref_seq(tip src)` and every other entry to the maximum of
  both sides; the merge or sync record carries the new vector ([AR §4.3]);
- `undo` and `op restore` restore the vector recorded on the newest merge, sync or fork record at or below the restored
  tip and set `absorbed_R[R]` to the restored tip's `ref_seq` ([AR §5a.5]).

Entries of deleted refs stay until the row of the deleted ref expires; the inertness test of MC-6 excludes deleted refs.

### 3.5 The move cache

`moves` is a list of at most 32 `u64` values, newest first: the lsns of the last records that moved the ref — a `Commit`
landing on it, or a `RefUpdate` naming it ([AR §5a.2]; the 32 is [F17]'s constant). Each move prepends its lsn and drops
the 33rd. `undo N` for N ≤ 32 reads the record at `moves[N − 1]` and needs no chain walk.

### 3.6 Counters

Definitions, for a commit c: `ops(c)` = the number of ops in c's stored op list ([F06]); `bytes(c)` = the byte length of
that stored op list, or `cs_ref.len` for a bulk commit ([AR §4.3]). The `u32` counters saturate at 2^32 − 1 and never
wrap; the `u64` totals cannot overflow in practice and saturate likewise.

| Event on ref X | Effect |
|---|---|
| a commit c lands on X (any commit kind, the sync residue of [AR §5a.3] included) | `ops_total += ops(c)`, `bytes_total += bytes(c)`, `ops_since_fork += ops(c)`, `overlay_ops += ops(c)`, `overlay_bytes += bytes(c)` |
| additionally, c is a `sync` commit (merge of `main` into X) | with M the ref `main` as of c's `sync_base`: `overlay_ops += M.ops_total − trunk_mark_ops`, `overlay_bytes += M.bytes_total − trunk_mark_bytes`, then `trunk_mark_ops = M.ops_total`, `trunk_mark_bytes = M.bytes_total` |
| X is forked from `main` at commit f with pinned set P | `trunk_mark_* =` `main`'s totals as of f (its current totals minus the ops and bytes of `main`'s commits after f); `overlay_* = trunk_mark_* −` the totals of `main`'s row in the `REFS` snapshot of P's newest segment (0 when P is none); `ops_since_fork = 0`; `ops_total = bytes_total = 0` |
| X is forked from a ref Y other than `main` | `overlay_*`, `trunk_mark_*` copy Y's current values; `ops_since_fork = 0`; `ops_total = bytes_total = 0` |
| X is promoted ([AR §5a.3], [F17 §7]) | `overlay_ops = overlay_bytes = 0`; `promoted_seg` = the new K; `base_pin` = the newest sealed set of `main`; the other counters are unchanged |

This keeps `overlay_ops` and `overlay_bytes` equal to what a view of X must replay beyond its promoted segment — trunk
ops from the pin to the fork, X's own ops, and every synced window since the last promotion — in O(1) per commit and per
sync ([AR §4.2], [AR §5a.3], [70] S1). The trigger that compares them with `store.promotion.overlay-ops` and
`store.promotion.overlay-bytes` is [F17 §7]'s. `ops_since_fork` is kept for `branch --list` ([AR §5a.4]); no trigger uses
it. `main`'s totals serve the marks of every lane; its own overlay counters are never compared, since `main` is never
promoted.

### 3.7 Ref ids

- `ref_id` values are allocated densely from 0 in creation order; `init` creates `main` with `ref_id` 0 (open point 9).
- `RtHdr.aux` of `REFS` holds `next_ref_id`: the smallest id never allocated. It never decreases, so an id is never
  reused after its row expires ([AR §4.1], [F02 §6.2]). In the tail, the `RefUpdate` that creates a ref carries its id
  ([F05]). The two homes of the counter agree: a segment's `REFS.aux` equals the `next_ref_id` that the `HEAD` fold of
  [F05 §10.2] gives at the segment's `upto_lsn`, and `HEAD.next_ref_id` ([F04 §5.14]) is never below the `aux` of the
  current set's newest segment (pass 1, S1-45).
- The id of a promoted-branch segment's name is the decimal `ref_id` ([F02 §6.3], [80] X-F10).

### 3.8 Retention

A row with `deleted` is dropped by the first fold at which the newest lsn in `moves` belongs to a record older than
`gc.reflog-expire` ([F17 §11.2]; the reflog keeps the tip for that window, [AR §5a.9]). Its entries in other refs'
absorbed vectors are dropped with it.

### 3.9 Where each field comes from

Replay and folds build a row from these records only (pass 1, A1-6, S1-8, P1-2). A `RefTable` entry ([F05 §9.10]) goes
with the `RefUpdate` of its group, whose `reason` decides which of the entry's fields the fold takes:

| Field | Written by |
|---|---|
| `ref_id`, `kind`, `name`, `flags.pinned`, `fork_commit`, `fork_lsn`, `fork_seq`, `fork_ref_id`, `message` | the entry of a create (`reason` 1, or a `park`, `reason` 5, whose `old` is zero: it creates `orphans/<R>`, kind 6, with no fork, [F05 §9.2]); never changed afterwards |
| `flags.deleted` | the entry of a delete (`reason` 2) |
| `tip`, `tip_lsn`, `gen` | a `Commit` landing on the ref; the entry of a create, an `undo`, an `op restore` or a `park` (`reason` 1, 3, 4, 5; a park sets them to the parked commit, whose own `Commit` record moves no ref, I27′) |
| `absorbed` | a `Commit` (§3.4); the entry of a create, an `undo` or an `op restore`; a `park` carries none, so an `orphans` row's vector stays empty ([F05 §9.2]) |
| `ref_seq_next` | the entry of a create (a creating `park` included); a `Commit` on the ref (its `ref_seq` + 1) |
| `base_pin`, `promoted_seg` | the entry of a create; a promotion of a `Checkpoint` record ([F05 §9.9]) |
| `ops_since_fork`, `overlay_ops`, `overlay_bytes`, `ops_total`, `bytes_total`, `trunk_mark_ops`, `trunk_mark_bytes` | the entry of a create (the fork values of §3.6; the totals are 0; all seven are 0 for a creating `park`); `Commit`s by §3.6 (a park is not a commit landing on the ref and changes none); a promotion resets the overlay pair |
| `moves` | every `Commit` on the ref and every `RefUpdate` naming it (§3.5), never an entry |

- **Partial upsert.** The fold takes from an upsert entry only the fields its reason writes in the table above; every
  other field of the entry must equal the row's current value (C: the writer copies it) and is ignored. So a ref move
  never resets a counter that commits and promotions maintain ([F17 §7]). A remove entry drops the row (§3.8).
- **Widths.** An entry's integer wider than its row field saturates at the field's maximum (§3.6); an entry's full
  commit ids (`cid32`) are stored as their `id16`.

## 4. `PINS`

One row per store file that at least one pin holds, sorted by `file` (`family`, `ref_id`, `file_no`) ([AR §4.2]
"segment file → refcount + holders", [AR §4.9] "Pins").

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 9 | `FileRef` | `file` | the pinned file |
| 9 | 4 | `u32` | `refcount` | the number of holders; equals `holders.len / 13`, else the section is invalid; ≥ 1 |
| 13 | 8 | `HeapRef` | `holders` | `list<PinHolder>`, sorted by (`kind`, `ref_id`, `set_lsn`), unique |
| total | 21 | | | |

`PinHolder` (13 bytes):

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 1 | `u8` | `kind` | 1 `fork-base`: the `base_pin` of ref `ref_id` set at its fork; 2 `promotion-base`: the `base_pin` a promotion of `ref_id` moved it to; 3 `merge`: a merge into `main` ([AR §4.9]); 4 `tag`: a tag with `--pin`. [F05 §9.8] `Pin.holder` takes these values (pass 1, A1-6) |
| 1 | 4 | `u32` | `ref_id` | the holding ref (for `merge`, `main`'s id) |
| 5 | 8 | `u64` | `set_lsn` | the lsn of the `Checkpoint` record that published the pinned set |
| total | 13 | | | |

- A pin holds a whole checkpoint set: every file the `Checkpoint` record at `set_lsn` names ([F05]) gets the holder, so
  one pin adds one holder to several rows.
- A `Pin` record ([F05 §9.8]) with `op` 1 adds the holder (`holder`, `ref_id`, `set_lsn`) to the row of every file it
  lists, creating rows as needed; `op` 2 removes it and drops a row whose `refcount` reaches 0. A promotion unpins the
  ref's previous base holder and pins the new one as `promotion-base`, so a ref holds at most one base holder.
- A file with a row is never deleted ([AR §4.1]). `branch -d` removes its `fork-base` and `promotion-base` holders;
  `doctor` lists holders whose ref is deleted or abandoned ([AR §5a.3]). How long `merge` pins are kept is the pin
  policy measurement 5 decides (WP-53c, [F17]); the layout holds any policy.

## 5. `HEADS`: client heads and bindings

One row per client key, sorted by (`kind`, `key`) ([AR §5a.1] "client head", [AR §5a.4]; the binding rows of [40 §2.6]
and R-15).

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 16 | `b16` | `key` | the client key (§5.1) |
| 16 | 1 | `u8` | `kind` | 1 `directory`, 2 `client` (`--client <name>`, `MOIRAI_CLIENT`), 3 `session` (`session:<harness>:<id>`, [90 §4.1]) |
| 17 | 1 | `u8` | `flags` | bit 0 `detached`: the head is a commit, not a ref; other bits reserved-zero |
| 18 | 1 | `u8` | `os` | OS tag ([F01 §3.2]) of the process that wrote `root_id`; 0 when `root_id.kind` = 0 |
| 19 | 1 | `u8` | `_reserved` | zero |
| 20 | 4 | `u32` | `ref_id` | the head's ref; 0 when `detached` |
| 24 | 16 | `b16` | `detached` | `id16` of the detached commit; zero when not `detached` |
| 40 | 8 | `u64` | `detached_lsn` | lsn of that commit's record; 0 when not `detached` |
| 48 | 57 | `OsFileId` | `root_id` | `directory` kind: the directory's `OsFileId` ([80 §2.10] P9: lookup by id first, [OS/path §4.4]); kind 0 when unknown or for other kinds |
| 105 | 40 | `BindingExt` | `binding` | R-15: [F18 §3.2]'s 40 bytes, embedded verbatim ([F18 §3.7]): the `designated` flag, the expected git ref in its short form and the base commit. All zero (valid, not designated) in every row of kind 2 or 3 and in a directory row that designates nothing |
| 145 | 8 | `u64` | `hlc` | `append_hlc` of the `ClientHead` record that wrote the row |
| 153 | 8 | `HeapRef` | `text` | `text`: the key's source (§5.1) |
| total | 161 | | | |

Every row of kind 1 is a **binding row** ([F18 §3.1]): it maps a directory to a branch, whether a binding verb or a
`checkout` wrote it; only `binding.designated` makes its tree a writer (R-15). The row holds no designation, expected-ref
or base field of its own (pass 1, S1-10, A1-8, P1-2). The `ClientHead` record that sets a row carries the row's image
(§2.9), so `BindingExt` travels as the same 40 bytes.

### 5.1 Keys

| `kind` | `text` | `key` |
|---|---|---|
| 1 `directory` | the canonical directory text of [OS/path §4] (the P9 form, per component with on-disk names) | `blake3_16(text)` ([OS/path §4.5]); for a tree top-level this equals the `TREES` key (§12.4) |
| 2 `client` | the client name as given | `blake3_16(text)` |
| 3 `session` | `session:<harness>:<id>` | `blake3_16(text)` |

`blake3_16` is BLAKE3-128 ([F01 §7.1]). The sort key includes `kind`, so equal hashes of different kinds never collide.

### 5.2 Rules

- `binding` is non-zero only in a row of kind `directory` that is not `detached` ([F18 §3.3] item 2); its own validity
  rules are [F18 §3.2]'s (an invalid extension makes the row non-designated, not the section invalid).
- **R-15, I-F12.** At most one row whose `binding.designated` is set names a given `ref_id`, and `key` is unique, so each
  tree designates at most one branch. A section that violates either is invalid; the writer refuses the second binding
  ([40 §5.3], exit 5). [F18] states I-F12.
- Two directory rows whose `root_id`s are the same object (whole-id identity, [OS/project §3.2]) name one tree; the writer
  refuses the second binding ([OS/path §4.4]).
- **Retention.** A `session` row is dropped by the first fold after `idempotency.retention` ([F17 §11.1]) has passed
  since its `hlc` ([AR §5a.1]). Other rows stay until an explicit `unbind` or `checkout` replaces or removes them.

## 6. `LEASES`

One row per lease that is not released, sorted by (`n`, `lease_id`), with `n` = 0 for a role lease ([AR §4.4],
[90 §10.1]). An expired lease that no one has reclaimed keeps its row, because its holder may renew it (I17′).

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 4 | `u32` | `n` | the task's `#N`; 0 for a role lease |
| 4 | 8 | `u64` | `lease_id` | the lease id printed as `L-<lease_id>`: the fencing token allocated by the claim that created the lease (§6.2) |
| 12 | 1 | `u8` | `kind` | 1 `task`, 2 `role` ([90 §4.3]) |
| 13 | 1 | `u8` | `flags` | bit 0 `run_scoped` (`--ttl run`: released only by `apply`, `run close` or `reclaim --run`; set iff the claim's `expires` is `Stamp::NEVER`); bit 1 `session_role` (`claim --role R --session`, the orchestrator's session role lease; the claim's `lflags` bit 0, [F05 §9.4], pass 1, A1-6); other bits reserved-zero |
| 14 | 2 | `u16` | `role` | `role` symbol: the role the lease grants ([AR §7.3], [90 §4.3]); for a self-claim without a role, `developer` |
| 16 | 4 | `u32` | `holder` | `actor` symbol of the holder |
| 20 | 4 | `u32` | `branch` | the `ref_id` of the lease's branch ([AR §5d.1]: a lease always carries its branch) |
| 24 | 4 | `u32` | `run` | the `#N` of the run node the lease is scoped to; 0 = none |
| 28 | 8 | `u64` | `token` | the current fencing token (§6.2) |
| 36 | 8 | `u64` | `claimed_hlc` | `append_hlc` of the claim's `Lease` record (`reclaim --older-than`) |
| 44 | 8 | `u64` | `ttl_ms` | the TTL the deadline is renewed by ([OS/clock §4.4]); 0 when `run_scoped` |
| 52 | 24 | `Stamp` | `expires` | the deadline `{wall, boot_hash, mono}` ([80] X-F2, [OS/clock §4]); `Stamp::NEVER` when `run_scoped` |
| 76 | 32 | `Anchor` | `anchor` | the holder anchor ([F03]); its `kind` is 0 `none`, 1 `session` or 4 `session-ttl`; kinds 2 and 3 are invalid here |
| 108 | 16 | `b16` | `bound` | BLAKE3-128 of the thread a session role lease or an environment lease is bound to ([90 §4.1]); zero = unbound |
| 124 | 16 | `b16` | `root_session` | BLAKE3-128 of a Codex holder's root session (`_meta.sessionId`), for grouping only, never matched; zero otherwise ([90 §4.4]) |
| 140 | 32 | `ProcId` | `proc` | the process that wrote the claim; diagnostics only ([AR §6.2]) |
| 172 | 8 | `HeapRef` | `files_owned` | `list<vstr>`: the leased task's `files_owned` globs captured at `claim` and at any `set files_owned` under the lease ([70] S5); sorted, unique; empty for a role lease |
| total | 180 | | | |

### 6.1 Rules

- `kind` = 1 ⇔ `n` ≠ 0. `session_role` implies `kind` = 2. `run_scoped` ⇔ `expires` = `Stamp::NEVER`, and then `run` ≠ 0
  and `ttl_ms` = 0; otherwise `ttl_ms` > 0.
- Liveness is judged from `anchor` and `expires` by [OS/proc §6.2] and [OS/clock §4.3]; `proc` never decides anything
  ([80 §2.7.2]).
- A renewal (a write presenting the lease past half its TTL, `moirai heartbeat`, or a `session-ttl` server's call,
  [AR §6.2], [90 §4.4]) rewrites `expires` only. `--move-lease` rewrites `branch`. A `set files_owned` under the lease
  rewrites `files_owned`. Each is a `Lease` record ([F05]).
- A release, a `complete` (which releases into `settled`, [AR §6.2]), a reclaim, the release at `branch -d` and a
  boot-change release remove the row at the next fold; in the tail, the `Lease` record's release form removes it at once.

### 6.2 Lease id and token

`HEAD.fence` allocates fencing tokens ([AR §4.2], [F04]). A claim that creates a lease allocates one token t and sets
`lease_id` = `token` = t. Renewal, expiry and a same-holder renewal of an expired lease leave both unchanged (I17′:
"expiry never bumps the token"). A reclaim by another holder is a new claim: a new row with a new id and token. No
format-v1 operation changes `token` without creating a new lease; the separate field keeps that possible without a
format change (open point 3).

## 7. `MARKERS` and `MARKERS_OLD`

`MARKERS` holds the markers that can still exclude a node on some view; `MARKERS_OLD` holds the globally inert ones
([AR §4.4], [70] S4). The semantics — which events change a marker, when it is active, how a reader tests it — are
[RULES/state-definition] §6 (MF, ME, AB and VR rows), which [F13 §4.2] cites; this chapter stores them. Both sections
have the same row and the same sort key (`n`, `ref_id`, `commit`, `emit_lsn`). The identity of a marker is (`#N`, origin
ref, origin commit) ([AR §4.4], [60 §2.5]; MF-001, MF-003, MF-004): the origin commit is the commit where the node's
closed hold first appears on the way back through its parents ([F13 §4.1]; [RULES/state-definition] OR rows), and the
origin ref is the ref that commit landed on. `MARKERS` holds at most one row per identity, its newest state;
`MARKERS_OLD` may hold several, told apart by `emit_lsn` (open point 2).

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 4 | `u32` | `n` | the node's `#N`: a task for `settled`, a node of any kind for `deleted` (MF-001) |
| 4 | 4 | `u32` | `ref_id` | the origin ref: the ref the origin commit landed on (MF-004); it may since have been deleted, or be a staging or import ref |
| 8 | 16 | `b16` | `commit` | `id16` of the origin commit (MF-003); a `cleared` row keeps the origin of the marker it ended |
| 24 | 1 | `u8` | `kind` | 1 `settled` (a `done` or `cancelled` hold), 2 `deleted` (a `deleted` hold), 3 `cleared` (the holder set emptied) (MF-002) |
| 25 | 1 | `u8` | `status` | `settled`: 1 `done`, 2 `cancelled` (the hold's status, [F05 §9.5] field 10); 0 for the other kinds |
| 26 | 1 | `u8` | `cause` | the event of the newest `Marker` entry that changed the row ([F05 §9.5] `cause`): 1 `ops` (a commit landing on a work ref), 2 `undo`, 3 `op-restore`, 4 `branch-delete` (`branch -d` or `-D`), 5 `fork` |
| 27 | 1 | `u8` | `flags` | bit 0 `nonlinear` (MF-007): set by ME-011 or ME-013 and never cleared; it selects the exact test AB-002. Bits 1–7 reserved-zero |
| 28 | 4 | `u32` | `ref_seq` | `ref_seq` of the origin commit on `ref_id` (MF-005): a linear marker is absorbed by a view R when `absorbed_R[ref_id] ≥ ref_seq` (AB-001) |
| 32 | 4 | `u32` | `actor` | `settled`: the `actor` symbol of the lease holder of the completion, [F05 §9.5] field 9 `holder` (MF-009; the `holder` of [AR §5d.1] and [AR §2.16]); 0 when the entry has none, and 0 for the other kinds, whose entries carry no holder |
| 36 | 1 | `u8` | `outcome` | `settled`: the `complete --outcome` of the entry that wrote it (MF-009; [F05 §9.5] field 11): 0 none, 1 `done`, 2 `failed`, 3 `abandoned`; 0 for the other kinds |
| 37 | 3 | `[3]u8` | `_reserved` | zero |
| 40 | 8 | `u64` | `hlc` | `append_hlc` of the newest `settled`, `deleted` or `cleared` entry of this identity (MF-008) |
| 48 | 8 | `u64` | `seq` | `seq` of `commit` (the change-feed position of the completion or deletion) |
| 56 | 8 | `u64` | `emit_lsn` | lsn of the newest `Marker` record that changed this row |
| 64 | 8 | `HeapRef` | `holders` | `list<u32>`: the `ref_id`s of the live work refs that hold `n` with this origin (MF-006), ascending and unique; empty in a `cleared` row and in every `MARKERS_OLD` row |
| total | 72 | | | |

Rules:
- **Records.** Every change of a row is an entry of a `Marker` record ([F05 §9.5]), appended in the flushed group of the
  commit or `RefUpdate` that caused it; the events that write them are ME-001 to ME-013 ([F13 §4.2] MC-1, MC-5). Replay
  applies the entries in log order and derives no holder change itself: a `settled` or `deleted` entry writes the row, or
  re-emits it, with the entry's holder set; a `holders` entry replaces the holder set; a `cleared` entry sets `kind` 3 and
  empties the holder set; a `nonlinear` entry sets `flags` bit 0. An entry whose identity is found only in `MARKERS_OLD`
  moves that identity back to `MARKERS` (ME-013).
- **Active.** A `settled` or `deleted` row is active while `holders` is non-empty (MF-006); a `cleared` row never is.
  A node is excluded on a view R while some active row of it is not absorbed by R: a linear row when
  `absorbed_R[ref_id] < ref_seq`, a `nonlinear` row when `commit` ∉ ancestors-or-self(tip(R)) (AB-001 to AB-004,
  [F13 §4.2] MC-4).
- **Inertness** (ME-012, [F13 §4.2] MC-6). At each checkpoint fold, `cleared` rows and rows absorbed by every live ref of
  every kind (deleted refs excluded) move from `MARKERS` to the `MARKERS_OLD` layer of the new segment, with `holders`
  emptied. The fold decides this from the rows and the absorbed vectors alone and writes no record. `MARKERS` is a
  snapshot; `MARKERS_OLD` is layered and append-only.
- **Retention.** `gc` drops `MARKERS_OLD` rows whose `hlc` is older than `gc.reflog-expire` ([F17 §11.2]).

## 8. `IDEM`

An open-addressing hash table per section ([AR §4.4], [AR §6.4]). `n_rows` is the capacity C; `RtHdr.aux` is the number
of used slots.

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 16 | `b16` | `key` | BLAKE3-128 of the idempotency key ([AR §4.3] `idem_key`) |
| 16 | 16 | `b16` | `payload` | BLAKE3-128 of the payload, the canonical bound AST ([AR §6.4], I14′) |
| 32 | 4 | `u32` | `ref_id` | the branch the entry is bound to |
| 36 | 4 | `u32` | `branch_sym` | `ref` symbol of that branch's name at the time, for the exit-9 text after the ref is gone |
| 40 | 4 | `u32` | `ref_seq` | the result commit's `ref_seq` on `ref_id`; 0 when the result is not a commit |
| 44 | 1 | `u8` | `flags` | bit 0 `used`; bit 1 `default_key`: the key was a default key ([AR §6.4], [F17 §11.1] P29, OP-17-16); bit 2 `result_inline`: the result bytes are in the heap; other bits reserved-zero |
| 45 | 3 | `[3]u8` | `_reserved` | zero |
| 48 | 8 | `u64` | `append_hlc` | `append_hlc` of the origin record; opens the retention windows ([F17 §11.1]) |
| 56 | 8 | `u64` | `origin_lsn` | lsn of the record that recorded the entry: a `Commit` with `idem_key`/`idem_payload`, or an `Idem` record ([F05]) |
| 64 | 8 | `HeapRef` | `result` | with `result_inline`: the bytes of the `Idem` record's result field ([F05], the result data of [API]); otherwise zero, and the result is derived from the commit at `origin_lsn` |
| total | 72 | | | |

- **Empty slot.** A slot with `used` = 0 is all zero bytes.
- **Capacity.** C = the smallest power of two ≥ max(16, 2 × n), where n is the number of entries; so the load factor is
  at most 1/2.
- **Placement.** h(key) = the `u64` read little-endian from `key[0..8]`; an entry's home slot is `h(key) & (C − 1)`.
  Entries are inserted in ascending `key` order by linear probing (slot i, then i + 1 modulo C). This makes the layout
  canonical (E3).
- **Lookup.** Probe from the home slot until a slot with an equal `key` (a hit) or an unused slot (a miss). A layered
  lookup probes the tail, then each delta's table newest first, then the base's (§2.4). A key has at most one entry in
  the store: the first recorded result stands ([AR §6.4]).
- **`result_inline`** is set exactly when the origin is an `Idem` record; the fold copies its result bytes, so the entry
  never depends on a retired non-commit record. A commit result is re-read from the commit ([AR §4.1] `hist` keeps
  commits).
- **Branch rule.** A hit on another branch is exit 9, unless `ref_id` was merged into the caller's branch
  (`absorbed_caller[ref_id] ≥ ref_seq`) or deleted after merge, which returns the original result (N13e, [AR §6.4]).
- **Retention.** A lookup ignores an entry older than `idempotency.retention`, or, with `default_key`, older than
  `idempotency.default-window`; the fold that rewrites its layer drops it ([F17 §11.1]).

## 9. `ALLOC` and `UIDX`

### 9.1 `ALLOC`

A dense array `#N → (uid, ref_id, create_seq)` over every `#N` the store has allocated on any branch ([AR §4.4],
[AR §5d.1], [72] M7; [50] F17 widened, open point 15). `RtHdr.aux` holds the first `#N` of the section; row i describes
`#N = aux + i`.

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 16 | `b16` | `uid` | the node's uid ([F08]) |
| 16 | 4 | `u32` | `ref_id` | the ref of the commit that created `#N` |
| 20 | 4 | `u32` | `create_seq` | that commit's `seq` (≤ 2^32 − 1 by the store's commit limit, [AR §4.5] step 4) |
| total | 24 | | | |

- The base segment's section starts at `aux` = 1 (`#N` 0 names no node) and covers every id below the fold's `next_id`;
  a delta's section covers the contiguous range of ids allocated after the next-older segment, extended down to the lowest
  reserved id whose hole it fills (below).
- An id allocated by a group that was lost before it became durable is not allocated ([AR §4.5] step 10). If a later id
  was allocated, the lost id's row is all zero bytes (a hole); a row with `create_seq` = 0 is a hole.
- **Reserved ids** (pass 1, P1-3). The `#N`s of a durable `Reserve` record ([F05 §9.27], [F16] P-84) are allocated by
  the record itself (its fold raises `next_id` past them, [F05 §10.2]), before the bulk `Commit` that uses them exists; in
  a segment that covers the record but not that `Commit`, their rows are holes. A fold that covers the bulk `Commit` fills the rows of the reserved ids it created (`uid`, `ref_id`,
  `create_seq`); its layer's range then starts at the lowest id it fills and re-states every row above it, unchanged
  rows byte for byte, so the one-range-per-layer form and the newest-first lookup of §2.4 hold. Reserved ids that the
  commit does not use (a uid the store already knew keeps its `#N`, I1), and every id of a reservation whose commit never
  lands (P-84 re-runs with a new reservation, or `gc` releases its files), stay holes and are never reused ([F09]
  OP-09-17).
- Rows are never changed, except a reserved id's hole filled as above. A re-keyed node gets a new `#N`; the old row stays
  with the removed node ([40 §2.3]).
- Rebuilt from the log by `repair` ([50] F17).

### 9.2 `UIDX`

Sorted `uid → #N` over every uid in `ALLOC`, so a `Create` of a known uid reuses its `#N` on any branch (I1, I-F2,
[AR §4.5] step 8).

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 16 | `b16` | `uid` | the key |
| 16 | 4 | `u32` | `n` | its `#N` |
| total | 20 | | | |

The base holds every uid; a delta holds the uids first allocated after the next-older segment. A uid appears once in the
store.

## 10. `CONFLICTS` (F11)

Sorted unresolved conflict values of a view ([50] F11, [AR §4.4]), one row per conflict key. A versioned section: in a
delta, branch or changeset segment it holds the complete set of rows of every node in the segment's touched-id list, and
a reader masks older layers' rows of those nodes ([F09]). Rows of schema keys (`n` = 0) follow the `SCHEMA` section
instead ([F09 §8.3], fold "full (view)"): a layer that carries `SCHEMA` carries every schema-key row of its view, and
a layer without `SCHEMA` carries none.

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 4 | `u32` | `n` | the key's owner `#N` ([F06 §7.9]); 0 for a schema key ([F06 §6.1] class 9, [F12] open point 18) |
| 4 | 1 | `u8` | `class` | the value-conflict class code of [F12 §6.1] |
| 5 | 1 | `u8` | `prov` | the provisional side of an existence conflict ([F06 §6.2], [F12 §6.3]): 0 `ours`, 1 `theirs`; 0 for every other key class, whose provisional value follows from the sides |
| 6 | 2 | `[2]u8` | `_reserved` | zero |
| 8 | 16 | `b16` | `commit` | `id16` of the commit that introduced the conflict value |
| 24 | 8 | `HeapRef` | `key` | the conflict key: the `ckey` bytes of [F06 §6.1] (class byte and components) |
| 32 | 8 | `HeapRef` | `base` | the base side: the `kval` bytes of [F06 §6.2] for the key's class, exactly as the `Conflict` op holds them |
| 40 | 8 | `HeapRef` | `ours` | as `base` |
| 48 | 8 | `HeapRef` | `theirs` | as `base` |
| total | 56 | | | |

Sort key: (`n`, key bytes). The row is the conflict part of the key's `cstate` ([F06 §6.2] orders 3–5) with the key in
front, so every conflict class a `Conflict` op can hold — an existence side with its node image, a status and resolution
pair, a body hash, an edge property block with its anchor record, a schema item — keeps its bytes after a checkpoint
(pass 1, S1-7, S1-5, P1-1). A side is never an empty slice: every `kval` encodes to at least one byte, an absent side
included (`ex` 0, `st` 0, type 0 of [F08 §5.1], `parent` 0 with an empty `order`, and so on). A `Resolve` removes the
row in the view where it lands.

## 11. `GLOBIDX`

Path globs of versioned fields by literal prefix, so pack class C7 and `notes --path` are range probes ([70] S17,
[AR §4.4]). One row per (node, field, glob) for the fields `task.files_owned`, the `globs` part of `note.applies_to` and
`rule.applies_to`, and `area.path_globs`, whose globs are rooted at `project` ([40 §2.4]). A versioned section with the
rule of §10.

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 4 | `u32` | `n` | the node's `#N` |
| 4 | 4 | `u32` | `field` | `name` symbol of the field (`files_owned`, `applies_to`, `path_globs`) |
| 8 | 4 | `u32` | `prefix_len` | the length of the glob's literal prefix: the bytes up to and including the last `/` before the first wildcard byte (`*`, `?`, `[`), or 0 when there is no such `/` ([40 §2.4]); for a glob without wildcards, up to and including its last `/` |
| 12 | 8 | `HeapRef` | `glob` | `text`: the glob |
| total | 20 | | | |

Sort key: (`glob[0 .. prefix_len]`, `n`, `field`, `glob`). A reader matching a path p probes, for each prefix of p that
ends at a `/` (and for the empty prefix), the rows with exactly that literal prefix.

## 12. R4 runtime tables

### 12.1 `OsFileId` (X-F8)

The tagged file identity of [80 §2.11.2], 57 bytes. [OS/project §3.1] states the same bytes with the per-OS sources; the
two tables are one structure (open point 21).

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 1 | `u8` | `kind` | 0 `none`, 1 `ntfs128`, 2 `refs128`, 3 `linux_ino`, 4 `darwin_fileid`; 5–255 reserved |
| 1 | 16 | `[16]u8` | `vol_key` | the volume key, BLAKE3-128 of one fixed source per OS ([OS/project §4.1]) |
| 17 | 16 | `[16]u8` | `id` | the object's id, by kind (below) |
| 33 | 16 | `[16]u8` | `parent` | the parent directory's id in the same encoding; zero when unknown |
| 49 | 4 | `u32` | `aux` | reserved, zero |
| 53 | 4 | `u32` | `docid` | the macOS document id when owner decision #21 (d) enables it; else zero |
| total | 57 | | | |

| `kind` | OS | `id` |
|---|---|---|
| 0 `none` | — | zero, with every other field zero |
| 1 `ntfs128`, 2 `refs128` | Windows | `FILE_ID_128.Identifier`, the 16 bytes as returned |
| 3 `linux_ino` | Linux | `u64le(ino) ‖ hgen`, `hgen` the first 8 bytes of BLAKE3-256 over `u32le(handle_type) ‖ f_handle` from unprivileged `name_to_handle_at` ([80 §2.11.2], [81] B2) |
| 4 `darwin_fileid` | macOS | `u64le(fileid) ‖` 8 zero bytes |

- **Identity** ([80 §2.11.2], [OS/project §3.2]): two ids are the same object iff `kind` (≠ 0), `vol_key` and `id` are
  equal; `parent`, `aux` and `docid` take no part. On Linux an inode number alone is never identity.
- A value with a reserved kind, kind 0 with a non-zero byte, or a non-zero `aux` is uninterpretable and absent ([80] X1).
- The **parent-directory id** of R-18 is `parent`; [F20] calls it `FILEOBS.parent_dir_id`.

### 12.2 `FsTime` and `FileAttrs`

`FsTime` (9 bytes; [80 §2.11.2] "i64 ns since the Unix epoch plus a granularity byte"; sources [OS/project §3.3]):

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 8 | `i64` | `ns` | `unix_ns` ([F01 §5.7]); 0 when absent |
| 8 | 1 | `u8` | `gran` | the source's nominal resolution as a decimal exponent e (units of at most 10^e ns), e = 0…10; `0xFF` = absent; 11–254 reserved (absent) |
| total | 9 | | | |

The granularity G in nanoseconds that [F20 §5.1] uses is `max(10^gran, VolumeCaps.mtime_granularity_ns)` of the tree's
volume ([OS/project §3.3]; open point 20).

`FileAttrs` is a `u32` with bits 0 `READONLY`, 1 `HIDDEN`, 2 `REPARSE_POINT`, 3 `RECALL_ON_OPEN`, 4 `RECALL_ON_DATA_ACCESS`,
5 `OFFLINE`, 6 `DATALESS`, 7 `PINNED`, 8 `UNPINNED`, 9 `CLOUD_REPARSE`, 10 `CLONE_MAY_SHARE`; bits 11–31 reserved-zero.
The per-OS sources are [OS/project §3.4]. Cloud-only is any of bits 3–6 ([40 §4.6]).

### 12.3 `VolumeCaps` snapshot

The 16-byte capability snapshot of [80 §2.11.2] ("`{flags u32, id_kind u8, btime u8, case_rule u8, cloud u8,
mtime_granularity_ns u64}`"), stored in `TREES`. The flag bits and values are [OS/project §4.2]'s and are restated here
because this chapter embeds the bytes.

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 4 | `u32` | `flags` | bit 0 `case_insensitive_default`; bit 1 `norm_insensitive_always`; bit 2 `norm_follows_case`; bit 3 `ctime_on_rename` known; bit 4 `ctime_on_rename` value (0 when bit 3 is 0); bits 5–6 `id_locate` (0 none, 1 by-id, 2 frontier, 3 reserved); bits 7–8 `journal` (0 none, 1 usn, 2 fsevents, 3 reserved); bits 9–10 `rename_noreplace` (0 unsupported, 1 native, 2 link-unlink for files, 3 reserved); bit 11 `clone_indicators`; bit 12 `ids_persistent`; bit 13 `docids`; bit 14 `dir_flush_doubtful` (set by file-system class, never by a probe: a Windows network redirector, `\\wsl$` and other non-local volumes, Linux NFS, CIFS, FUSE and 9p; [OS/project §4.2], §4.3); bits 15–31 reserved-zero |
| 4 | 1 | `u8` | `id_kind` | 0–4 as `OsFileId.kind`; 5–255 reserved |
| 5 | 1 | `u8` | `btime` | 0 absent, 1 tunneled-not-copied, 2 unforgeable, 3 copied-by-clones; 4–255 reserved |
| 6 | 1 | `u8` | `case_rule` | 0 sensitive (then `flags` bit 0 is 0), 1 per-directory flag, 2 volume; 3–255 reserved |
| 7 | 1 | `u8` | `cloud` | 0 none, 1 recall attributes, 2 dataless; 3–255 reserved |
| 8 | 8 | `u64` | `mtime_granularity_ns` | effective granularity in ns; 0 = not measured ([OS/project §4.4]) |
| total | 16 | | | |

A snapshot with a reserved bit or value set is uninterpretable, and its `TREES` row behaves as on a first settle.

### 12.4 `TREES`

One row per tree, sorted by `key` ([40 §2.6], [80 §2.11.2], [70] S5, S8).

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 16 | `b16` | `key` | `blake3_16` of the canonical root text ([OS/path §4.5], [80 §2.10] P9) |
| 16 | 1 | `u8` | `os` | OS tag of the writer ([F01 §3.2]); governs `root_id`, `caps` and `sens` (§2.7) |
| 17 | 1 | `u8` | `flags` | bit 0 `first_settle_done`; bit 1 `cloud_root` (the root or an ancestor is a cloud sync root, [40 §4.6], [OS/project §7.3]); bit 2 `dirty_present` (the dirty row holds a value); bit 3 `git` (the tree is a git worktree top-level; else a bound directory without git); other bits reserved-zero |
| 18 | 57 | `OsFileId` | `root_id` | the root directory's id ([80 §2.10] P9: trees are looked up by it first) |
| 75 | 16 | `VolumeCaps` | `caps` | the root volume's capabilities (§12.3) |
| 91 | 33 | `OidSlot` | `last_head` | the tree's git `HEAD` commit at its last settle; `algo` 0 without git |
| 124 | 8 | `u64` | `last_settle_hlc` | the `hlc` of the tree's newest settle of any scope ([F20 §5.12]); 0 before the first |
| 132 | 4 | `u32` | `dirty_count` | the dirty row's count ([70] S5); 0 unless `dirty_present` |
| 136 | 33 | `OidSlot` | `dirty_head` | the git `HEAD` the count was taken against; zero unless `dirty_present` |
| 169 | 8 | `u64` | `dirty_hlc` | when the count was taken (printed as its age); 0 unless `dirty_present` |
| 177 | 8 | `HeapRef` | `root_text` | `text`: the canonical root text ([OS/path §4]) |
| 185 | 8 | `HeapRef` | `sens` | `list<SensEntry>`: the case- and normalization-sensitivity map (below) |
| 193 | 8 | `HeapRef` | `epochs` | `list<Epoch>`: the settle epochs (below) |
| total | 201 | | | |

**Sensitivity map.** Entries only for directories holding linked files whose equivalence ([OS/project §4.5]) differs from
the volume default in `caps`; sorted by path, unique.

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `equiv` | `u8` | always | bit 0 `case_insensitive`, bit 1 `norm_insensitive`; other bits reserved-zero |
| 2 | `path` | `vstr` | always | the directory's root-relative path ([40 §2.4]; the root itself is the empty string) |

**Settle epochs** (A1P-04; [70] S8). `Epoch` is 32 bytes:

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 1 | `u8` | `scope_kind` | 0 `full-tree`, 1 `lane-owned`, 2 `partial` ([40 §2.6], FB-1: a settle of the brief's items, a `--scope` or a `--path` settle); 3–255 invalid |
| 1 | 3 | `[3]u8` | `_reserved` | zero |
| 4 | 4 | `u32` | `scope_ref` | `lane-owned`: the lane's `ref_id`; else 0 |
| 8 | 16 | `b16` | `digest` | the scope digest (below) |
| 24 | 8 | `u64` | `hlc` | the settle's `hlc` |
| total | 32 | | | |

This is the epoch of [40 §2.6] byte for byte; [F05 §9.23]'s `TreeReg` carries it (pass 1, A1-6, S1-8, whose alignment
replaced this chapter's earlier 1-based numbering and its `brief` value).

- **Digests.** `full-tree`: zero. `lane-owned`: `BLAKE3-128(lp("moirai-scope-lane-v1") ‖ lp(g1) ‖ … ‖ lp(gk))` over the
  distinct globs g1 < … < gk of the lane-owned scope the settle used ([F01 §6.3], order [F01 §6.6]). `partial`: zero,
  since a partial epoch is never decidable. [F16] and [40 §4.2] define which globs and items a settle's scope holds.
- **Coverage** (the effective `verified_at` of [F20 §5.9]). An epoch covers a file F of the tree when its coverage is
  decidable and holds: `full-tree` always covers every file of the tree's `project` root; `lane-owned` is decidable only
  when the checker, computing the lane's current scope with the same definition, gets the epoch's `digest`, and then
  covers F iff F's path matches one of those globs; `partial` is never decidable and never advances `verified_at`. This
  errs only towards a smaller `verified_at`, which turns copy-rule outcomes into proposals, never into re-binds
  (A1P-04).
- **Retention.** The list holds at most one epoch per (`scope_kind`, `scope_ref`): the newest. It is sorted by
  (`scope_kind`, `scope_ref`). A settle's `TreeReg` record ([F05 §9.23] `sub` 2) replaces the epoch of its own scope; a
  fold drops the `lane-owned` epochs whose `scope_ref` names a deleted ref ([40 §2.6]).
- **Journal cursor.** A tree's `JOURNALCUR` row is the one whose `vol_key` equals `root_id.vol_key`; no field refers to
  it, and no record carries one (open point 17).
- **Sources** ([F05 §9.23] `TreeReg`). `sub` 1 (register or update) writes `os`, `root_id`, `caps`, `last_head`,
  `last_settle_hlc`, `root_text`, `sens` and `flags` bits 0, 1 and 3 (its `tflags`); `sub` 2 writes one epoch and raises
  `last_settle_hlc` to the epoch's `hlc`; `sub` 3 writes the dirty row (`dirty_count`, `dirty_head`, `dirty_hlc`) and sets
  `flags` bit 2; `sub` 4 (forget) drops the row.
- **Retention of rows.** A `TREES` row is dropped with its tree's `FILEOBS` rows (§12.5), or by a forget record.

### 12.5 `FILEOBS` (R-18)

One row per (file node, tree) with an observation, sorted by (`n`, `tree`) ([40 §2.6], [40] R-18, [80 §2.11.2]). Rows
are written only when a file's stat quadruple, state or proposals change ([70] S8). A `FileObs` record ([F05 §9.18])
carries the row's image (§2.9).

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 4 | `u32` | `n` | the file node's `#F` |
| 4 | 16 | `b16` | `tree` | the tree's key (§12.4) |
| 20 | 1 | `u8` | `state` | the recorded state (below) |
| 21 | 1 | `u8` | `flags` | bit 0 `has_path_seen`; bit 1 `has_detail`; bit 7 `dead` (§2.4); other bits reserved-zero |
| 22 | 2 | `u16` | `resolver_version` | the resolver version that wrote the row ([F20 §1.3]); a row of another version is absent |
| 24 | 1 | `u8` | `n_proposals` | 0–3 ([F20] `LIST_MAX`) |
| 25 | 57 | `OsFileId` | `file_id` | the tagged id, with the parent-directory id in `parent` ([F20] `FILEOBS.file_id`, `FILEOBS.parent_dir_id`) |
| 82 | 8 | `u64` | `size` | the raw size |
| 90 | 9 | `FsTime` | `mtime` | last-write time |
| 99 | 9 | `FsTime` | `ctime` | change time (Windows `ChangeTime`) |
| 108 | 9 | `FsTime` | `creation` | creation time (`btime`; [F20] `FILEOBS.creation`) |
| 117 | 9 | `FsTime` | `added` | added time (macOS `ATTR_CMN_ADDEDTIME`); absent elsewhere |
| 126 | 4 | `FileAttrs` | `attrs` | attributes, with the cloud bits (§12.2) |
| 130 | 33 | `OidSlot` | `last_oid` | the latest observed `oid` at the path; `algo` 0 when none |
| 163 | 8 | `u64` | `verified_at` | `hlc` of the last settle that saw F present at its path in this tree and wrote the row (a settle writes it only on a change of the quadruple, state or proposals); the effective value is the larger of this and the `hlc` of the newest covering epoch (§12.4) |
| 171 | 8 | `u64` | `missing_since` | `hlc` since which F is `missing` in this tree; 0 = not missing |
| 179 | 8 | `HeapRef` | `var` | the sequence below |
| total | 187 | | | |

`state` is a file-state code of [F18 §4.2], cited verbatim ([F18 §4.10]; pass 1, S1-9, A1-7): a settle records only the
states the file cascade decides from what it observes ([F20 §5]) — 1 `ok`, 2 `moved-auto` (not yet recorded), 3
`moved-needs-confirm`, 4 `ambiguous`, 6 `replaced`, 8 `missing`, and 12 `unverified` (a settle whose search a budget
cut keeps the proposals it found, [F20 §5.14]). 5 `deleted`, 9 `absent-in-tree`, 10 `pending` and 11 `planned` follow
from versioned data and the view and are computed when a link is rendered, never recorded; 7 and 13 are link-only codes.
Every other value is invalid.

`var` sequence:

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `path_seen` | `vstr` | `has_path_seen` | the enumerated spelling of the node's own path, in the node's root, when it differs from the branch value ([40 §2.6], [F20 §3.6]) |
| 2 | `target` | `path` ([F08 §5.2]) | `state` = 2 | the path the exact evidence found, with its root |
| 3 | `target_ev` | `u8` | `state` = 2 | the exact evidence's token, a code 1–12 of [F18 §5.2]; detail 5 renders it |
| 4 | `n_details` | `u8` | `has_detail` | 1 or 2 |
| 5 | details | `n_details` × `Detail` | `has_detail` | the recorded detail parts (below) |
| 6 | proposals | `n_proposals` × `Proposal` | `n_proposals` > 0 | ordered by the resolver's order ([F20 §4.1], §5.5) |

`Detail` (the codes and templates are [F18 §4.6]'s; which parts a state has is [F18 §4.7] rule 1):

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `code` | `u8` | always | a detail code of [F18 §4.6] |
| 2 | `args` | per slot | always | the values of the slots of the code's text template, in template order: `<path>` as a [F08 §5.2] `path`; `<score>` as `score_num u64` then `score_den u64` (lowest terms, `score_den` ≥ 1); `<g7>` as an `oidv` git commit id ([F01 §7.5] variable-width form; the text prints 7 digits); `<n>` as a `u32`; the optional `, runner-up <score>` of code 15 as a `u8` 0 or 1, followed by the score when 1. `<c8>`, `<age>`, `<ev>`, `<text>` and `<quote>` are never stored: a reader takes them from the view, `missing_since`, `target_ev`, the tombstone and the anchor |

A row records the parts of [F18 §4.7] rule 1 for its `state` that the cascade decided, in rule-1 order, and not the parts
a reader derives when it renders the link: 1, 4–9, 36, 46 and 66–69. So a row has at most two: one principal part for
`ok` (2 or 3, or none), `moved-needs-confirm` (10–21), `ambiguous` (22–29), `missing` (37–45) and `unverified` (53–61);
34 and optionally 35 for `replaced`; none for `moved-auto`. `has_detail` is set iff at least one part is recorded. A
recorded state and its parts render unchanged while the stat tuple equals the row ([F20 §5.4] step 3), for example
`ambiguous (path reused; original at q)` ([72] M13).

`Proposal` (the tuple of [F18 §4.10]):

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `class` | `u8` | always | evidence class: 1 `exact`, 2 `strong`, 3 `copy`, 4 `weak` ([F20 §1.5], numbered by [F18 §4.10]) |
| 2 | `evidence` | `u8` | always | the proposal-class token an acceptance would record, a code 13–26 of [F18 §5.2] |
| 3 | `path` | `path` ([F08 §5.2]) | always | the candidate path with its root |
| 4 | `score_num` | `u64` | always | the score as an exact rational in lowest terms ([F20 §1.2]); 0 when the token has no score |
| 5 | `score_den` | `u64` | always | its denominator, ≥ 1; 1 when the token has no score |

**Retention.** `gc` drops the rows of every tree whose newest epoch is older than `gc.fileobs-idle-expire`
([F17 §11.3], [AR §4.9]), with the tree's `TREES`, `DIRMAP` and `PENDING` rows. A dropped row only makes the resolver's
answer more conservative ([F17 §11.3]).

### 12.6 `PENDING`

Observations a reader tree made, promoted at a later settle of the writer tree ([40 §2.6], [40 §5.3]). Sorted by
(`n`, `tree`, `from`, `to`), the paths compared by their slice bytes. A `Pending` record ([F05 §9.19]) carries the row's
image (§2.9).

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 4 | `u32` | `n` | the file node's `#F` |
| 4 | 16 | `b16` | `tree` | the observing tree's key |
| 20 | 1 | `u8` | `class` | evidence class of the observation, numbered as `Proposal.class` (§12.5) |
| 21 | 1 | `u8` | `source` | 1 `evidence-hook`, 2 `reader-settle` |
| 22 | 1 | `u8` | `flags` | bit 7 `dead`; other bits reserved-zero |
| 23 | 1 | `u8` | `evidence` | the evidence token the observation carries, a code of [F18 §5.2] that its source can yield: from a hook 1 `intent`, 3 `file-id`, 10 `move` or 22 `argv`; from a reader settle a `lazy` or `git` token (3–9) or a proposal-class token (13–26) ([F20 §5.6]; pass 1, S1-9) |
| 24 | 33 | `OidSlot` | `oid` | the captured `oid` |
| 57 | 9 | `FsTime` | `creation` | the captured creation time of the source |
| 66 | 33 | `OidSlot` | `head` | the observing tree's git `HEAD`; `algo` 0 without git |
| 99 | 8 | `u64` | `hlc` | when observed |
| 107 | 8 | `HeapRef` | `from` | `path`: the path observed gone, with its root |
| 115 | 8 | `HeapRef` | `to` | `path`: the path observed, with its root |
| total | 123 | | | |

**Retention.** 30 days after `hlc` ([40 §2.6]); a row promoted by a writer-tree settle is written dead.

### 12.7 `FSINTENT`

File-operation intents and their outcomes ([40 §3.4], [40 §3.5], [80 §2.11.2] "`FSINTENT` holder"). Sorted by
`intent_id`.

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 8 | `u64` | `intent_id` | the lsn of the `FsIntent` record; also the `<intent>` of `trash/<intent>/` ([F02 §5.4]) |
| 8 | 1 | `u8` | `op` | 1 `mv`, 2 `rm`, 3 `rm-trash` |
| 9 | 1 | `u8` | `state` | 1 `open`, 2 `done` (`FsIntentDone`), 3 `aborted` (`FsIntentAborted`) |
| 10 | 1 | `u8` | `flags` | bit 0 `git` (`--git`); bit 1 `recursive` (`rm --recursive`); bit 2 `recovered` (intent recovery wrote the closing record); other bits reserved-zero |
| 11 | 1 | `u8` | `reason` | `aborted`: the abort reason (below); 0 otherwise |
| 12 | 4 | `u32` | `branch` | `ref_id` of the caller's branch |
| 16 | 16 | `b16` | `tree` | the writer tree's key |
| 32 | 32 | `Anchor` | `anchor` | the intent anchor ([F03]; kind 2 `intent`, the CLI's own slot and nonce, [80 §2.7.2]) |
| 64 | 32 | `ProcId` | `proc` | diagnostics only (replaces `pid, pid_start`, [80] X-F8) |
| 96 | 8 | `u64` | `hlc` | `append_hlc` of the `FsIntent` record |
| 104 | 8 | `u64` | `closed_lsn` | lsn of the `FsIntentDone` or `FsIntentAborted` record; 0 while `open` |
| 112 | 8 | `u64` | `closed_hlc` | its `append_hlc`; 0 while `open` |
| 120 | 4 | `u32` | `n_items` | the number of items, ≥ 1 |
| 124 | 8 | `HeapRef` | `items` | `n_items` × `IntentItem`, in the intent's item order (`<i>` of `trash/<intent>/<i>`) |
| total | 132 | | | |

`IntentItem` ([F05 §9.15]'s item with `outcome` added; pass 1, A1-6, S1-8, P1-2):

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `itflags` | `u8` | always | bit 0 `dir` (the item is a directory: no `oid`); other bits reserved-zero |
| 2 | `outcome` | `u8` | always | 0 while the intent is open, and in an aborted intent; else the item outcome of its `FsIntentDone` (below) |
| 3 | `src` | `path` ([F08 §5.2]) | always | the source path with its root, so recovery can redo or verify an intent on any root ([40 §3.4]) |
| 4 | `dst` | `path` ([F08 §5.2]) | `op` = 1 | the destination path with its root (`rm` and `rm-trash` have none; their destination is `trash/<intent>/<i>`) |
| 5 | `oid` | `u8` `algo` + digest bytes ([F01 §7.5] variable-width form) | `itflags` bit 0 = 0 | the file's `oid` at plan time |

This chapter owns the two code tables; [F05 §9.16] and [F05 §9.17] write them.

| item outcome | meaning | abort reason | meaning |
|---|---|---|---|
| 1 | done | 1 | not renamed (the source is present, the destination absent) |
| 2 | failed: busy (a sharing violation after the retries) | 2 | cross-volume (`ERROR_NOT_SAME_DEVICE` or `EXDEV` at the rename) |
| 3 | failed: the destination exists | 3 | every item failed before a rename |
| 4 | failed: the source is missing | 4 | ambiguous: both paths present with different file ids ([40 §3.4] recovery table) |
| 5 | failed: other | 5 | missing: neither path present |

Other values are invalid. Recovery closes an intent it cannot decide with reason 4 or 5; `brief` and `doctor` render
them with [F18 §4.6] texts 72 and 73.

**Retention.** Open intents stay until closed. A closed row is kept for `gc.trash-expire` after `closed_hlc` (the trash
purge and `doctor`'s post-boot comparison read it, [F17 §11.3], [AR §4.10]) and then dropped (open point 22).

### 12.8 `FPRINT`

`oid` → fingerprint blob ([40 §2.5], R-8, R-9). Sorted by `oid` (`algo`, digest).

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 33 | `OidSlot` | `oid` | the content id |
| 33 | 16 | `b16` | `blob` | BLAKE3-128 of the fingerprint value ([F20 §2.6.4]), the id of its blob of class "fingerprint" ([F10], R-9) |
| 49 | 4 | `u32` | `file` | the number n of the `blobs.<n>` file that holds the blob, so a lookup opens one file ([F10 §5.5]; [F09] OP-09-14 adopted, pass 1, P1-22); 0 in a dead row |
| 53 | 1 | `u8` | `flags` | bit 7 `dead`; other bits reserved-zero |
| total | 54 | | | |

The fold that seals `FPrint` records ([F05 §9.20]) writes the fingerprint blob and the row together. Whatever replaces
a `blobs` file (a tiered fold or a rollup that merges files, blob GC) writes, in the segment it publishes, every
`FPRINT` row of the current set that names the replaced file, with the new file's number, so no row names a file that
is no longer live.

**Retention.** Kept only while `oid` is the current or latest-observed content of a live file node on a live branch head
(the node's `oid` or a `FILEOBS.last_oid`); garbage-collected with blobs ([40 §2.5], [AR §4.9]).

### 12.9 `JOURNALCUR` (reserved)

41 bytes per volume, sorted by `vol_key` ([80 §2.11.2], [OS/project §7.1]). No M0–M11 code writes it: E2 is not built
([AR §11] #41), so the section has 0 rows.

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 1 | `u8` | `kind` | 0 none, 1 `usn`, 2 `fsevents`; 3–255 reserved |
| 1 | 16 | `[16]u8` | `vol_key` | the volume key |
| 17 | 16 | `[16]u8` | `instance` | `usn`: `u64le(UsnJournalID)` ‖ 8 zero bytes; `fsevents`: the device UUID; none: zero |
| 33 | 8 | `u64` | `cursor` | `usn`: the next USN to read; `fsevents`: the event id after the last processed event; none: 0 |
| total | 41 | | | |

### 12.10 `DIRMAP`

`(tree, directory OsFileId) → (root-relative path, mtime)` ([80 §2.11.2], [80 §2.11.3]). Sorted by (`tree`,
`dir.kind`, `dir.vol_key`, `dir.id`). Derived-optional ([80 §2.11.2]). A `DirMap` record ([F05 §9.22]) carries the
row's image (§2.9).

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 16 | `b16` | `tree` | the tree's key |
| 16 | 57 | `OsFileId` | `dir` | the directory's id; its `parent` is the parent directory's id |
| 73 | 9 | `FsTime` | `mtime` | the directory's mtime at the settle that enumerated it; a settle never records a racy mtime ([F20] open point 17) |
| 82 | 1 | `u8` | `flags` | bit 7 `dead`; other bits reserved-zero |
| 83 | 8 | `HeapRef` | `path` | `text`: the directory's root-relative path (the root itself is empty) |
| total | 91 | | | |

**Retention.** Dropped with the tree's `FILEOBS` rows (§12.5); a settle writes a dead row for a directory it found gone.

### 12.11 `PREFIXEV`

Evidence that a directory moved, accumulated across settles ([40 §2.4] `observed` class, [40 §2.6]). Sorted by (`tree`,
`root`, `from`, `to`). A `PrefixEv` record ([F05 §9.24]) carries the row's image (§2.9).

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 16 | `b16` | `tree` | the tree's key |
| 16 | 2 | `u16` | `root` | `root` symbol of the root the prefixes are relative to |
| 18 | 1 | `u8` | `flags` | bit 7 `dead` in a record's delete image only (§2.4, §2.9), never in a section; other bits reserved-zero |
| 19 | 1 | `u8` | `_reserved` | zero |
| 20 | 4 | `u32` | `rebound` | linked nodes under `from` re-bound exactly to `to` so far |
| 24 | 4 | `u32` | `remaining` | linked nodes under `from` not yet re-bound |
| 28 | 8 | `u64` | `first_hlc` | the first settle that contributed |
| 36 | 8 | `HeapRef` | `from` | `text`: the directory prefix, ending in `/` |
| 44 | 8 | `HeapRef` | `to` | `text`: the directory prefix, ending in `/` |
| total | 52 | | | |

**Retention.** The row is removed when the settle that brings `remaining` to 0 writes the `observed` `path_moves` entry,
and dropped by a fold 30 days after `first_hlc` (the `PENDING` window; open point 22).

### 12.12 `ANCESTRY` and `GITRENAMES` (`GITFACTS`)

The git-fact cache of [40 §2.6] (`GITFACTS`, record kind `GitFacts`) is two sections: [AR §4.4]'s `ANCESTRY` and R-8's
`GITRENAMES`. Both are pure functions of git objects and output-neutral ([F20 §1.3]).

`ANCESTRY`, sorted by (`algo`, `a`, `b`):

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 1 | `u8` | `algo` | object format of both commits ([F01 §7.5]) |
| 1 | 32 | `[32]u8` | `a` | commit a (fixed slot) |
| 33 | 32 | `[32]u8` | `b` | commit b (fixed slot) |
| 65 | 1 | `u8` | `answer` | 1 when a is an ancestor of b or equal to it; 0 otherwise |
| 66 | 1 | `u8` | `flags` | bit 7 `dead`; other bits reserved-zero |
| total | 67 | | | |

`GITRENAMES`, sorted by (`algo`, `commit`):

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 1 | `u8` | `algo` | object format |
| 1 | 32 | `[32]u8` | `commit` | the commit (fixed slot) |
| 33 | 32 | `[32]u8` | `parent` | `renames`: its first parent, against which the renames are computed, zero for a root commit; zero without `renames` |
| 65 | 8 | `i64` | `commit_time` | `times`: committer time in seconds since the epoch, as the commit states it; 0 without `times` |
| 73 | 8 | `i64` | `author_time` | `times`: author time, likewise; 0 without `times` |
| 81 | 1 | `u8` | `flags` | bit 0 `renames` (the rename part holds a fact); bit 1 `times` (the two times hold a fact); bit 7 `dead`; other bits reserved-zero; a live row has bit 0 or bit 1 |
| 82 | 4 | `u32` | `n_renames` | exact renames; 0 without `renames` |
| 86 | 4 | `u32` | `n_groups` | ambiguous identical-blob groups ([AR §5e.3] E6: never paired by order); 0 without `renames` |
| 90 | 8 | `HeapRef` | `list` | the sequence below; empty without `renames` |
| total | 98 | | | |

`list` sequence: `n_renames` × (`from` `vbytes`, `to` `vbytes`), sorted by `from`; then `n_groups` × (`blob`
`digest(algo)`, `n_from` `uvar32`, `n_from` × `vbytes`, `n_to` `uvar32`, `n_to` × `vbytes`), each group's paths sorted,
groups sorted by their first `from` path. Paths are repository-relative bytes as git's trees spell them (git paths need
not be UTF-8, [F05] open point 18).

**Sources** ([F05 §9.25] `GitFacts`; pass 1, A1-6, P1-2). A `Fact` of type 1 (`algo`, `a`, `b`, `answer`) is an
`ANCESTRY` row with the same fields. A `Fact` of type 2 (`algo`, `a` = the commit, `b` = its first parent) writes the
rename part of the commit's `GITRENAMES` row (`parent` = `b`, the pairs and groups, their bytes unchanged) and sets bit 0;
a `Fact` of type 3 writes `commit_time` and `author_time` and sets bit 1. The two parts of one row come from different
facts and are merged by the fold; a fact never changes a part already present, since facts are pure functions of git
objects.

**Retention** (A1P-16). A fold keeps a row only for commits reachable from a bound tree's `HEAD` within the E6 window
(the bound of [F20 §5.11]); `gc` drops the rest.

### 12.13 `ANCHORRES`

Anchor results per (anchor uid, file `oid`, resolver version) ([40 §2.6], [70] S7). Sorted by (`anchor`, `oid`,
`resolver_version`). An `AnchorRes` record ([F05 §9.26]) carries the row's image (§2.9).

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 16 | `b16` | `anchor` | the anchor uid ([40 §2.7]) |
| 16 | 33 | `OidSlot` | `oid` | the `oid` of the file content the result holds for |
| 49 | 2 | `u16` | `resolver_version` | [F20 §1.3]; a row of another version is absent |
| 51 | 1 | `u8` | `state` | an anchor-state code of [F18 §4.3], cited verbatim ([F18 §4.10]): 1 `fresh`, 2 `moved`, 3 `edited`, 4 `ambiguous`, 5 `orphaned`; 6 `unverified` and 7 `unresolved` are never stored |
| 52 | 1 | `u8` | `detail` | `edited`: the detail code of [F18 §4.6], 62 `edited` (a fuzzy match, [F20 §6.4], or a changed span under `span` watch) or 63 `edited-scope` (scope only, [F20 §6.5]); 0 for every other state |
| 53 | 1 | `u8` | `flags` | bit 7 `dead`; other bits reserved-zero |
| 54 | 4 | `u32` | `first_line` | the span's first line in that content, 1-based; 0 for `ambiguous` and `orphaned` |
| 58 | 4 | `u32` | `last_line` | the span's last line, ≥ `first_line`; 0 with `first_line` 0 |
| 62 | 8 | `u64` | `score_num` | the fuzzy score of [F20 §6.4] as an exact rational in lowest terms, which detail 62 prints; 0 unless a fuzzy match decided |
| 70 | 8 | `u64` | `score_den` | its denominator, ≥ 1; 1 when `score_num` is 0 |
| total | 78 | | | |

An `unverified` result is never stored: it depends on budgets and keys, not only on the content and the version.

**Retention** (A1P-16). A fold keeps a row only while its `oid` is the current or latest-observed content of a live file
node on a live branch head, as for `FPRINT` (§12.8).

## 13. Session and backup tables

The fold targets that [F05 §7] names for change-feed and pack cursors, session marks and backups (pass 1, A1-23,
closure NC-10). Cursors and session marks let delta hooks and `pack`'s C8 show only what changed and `pack` render the
rules a `SubagentStart` hook showed as one line ([AR §6.3], [AR §7.4], [73 F4]); the backup table gives `backup.max-age` ([CFG]) its newest backup. Losing a cursor or a
session mark is harmless: the next prompt re-shows changes, and `pack` renders the rules in full ([F05] open point 14).

### 13.1 `CURSORS`

One row per (session, agent, feed, task), sorted by that key (`session` and `agent` bytewise, then `feed` and `task`
numerically, §2.2). Written by `Lazy` records of `sub` 2 ([F05 §9.11]): a record replaces its key's row, each row field
taking the record field of the same name (`session` from `session_hash`, `agent` from `agent_hash`).

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 16 | `b16` | `session` | the session hash of [F03 §9.2] |
| 16 | 16 | `b16` | `agent` | BLAKE3-128 of the namespaced agent identity ([90 §4.1]); zero for the session itself |
| 32 | 1 | `u8` | `feed` | 1 the change feed ([AR §6.3]); 2 the pack cursor of [AR §7.4] C8 ([RULES/pack-classes] PX-011); other values invalid |
| 33 | 3 | `[3]u8` | `_reserved` | zero |
| 36 | 4 | `u32` | `task` | `feed` 2: the `#N` of the node T the pack was built for (the record's `task`, a `nodeid`; T is [RULES/pack-classes] PT-001's target, of any kind, usually a task); 0 for `feed` 1. A `feed` 2 row with `task` 0 is invalid |
| 40 | 8 | `u64` | `cursor_seq` | `feed` 1: the `seq` the session or agent has seen; `feed` 2: the pack's `rev`, the `seq` of the commit at tip(B) that the pack read ([RULES/pack-classes] PT-032) |
| 48 | 8 | `u64` | `hlc` | the record's HLC |
| total | 56 | | | |

**The pack cursor** (pass 1, closure NC-10, NC-11). A `feed` 2 row is the cursor of a pack of T for agent A:
cursor(A, T) of [RULES/pack-classes] PT-028 is the `cursor_seq` of the row (session, A, 2, `#N` of T) of the session
the pack runs in, and absent when that row is absent, so C8 is empty (PM-025). `#N` names T store-wide and is never
reused (§9); a re-keyed task has a new `#N` and so starts with no cursor. The record's bytes and fold are [F05 §9.11]'s.
The layer that delivers the pack (the `SubagentStart` hook or the MCP server, [AR §5d.1]) appends it, never the `pack`
verb (owner question OQ-F-3, decided 2026-09-28, option (b); `reviews/owner-questions.md`); no M0 command is such a layer
([API] open point 48, [RULES/pack-classes] PX-011), so cursor(A, T) is absent and C8 is empty at M0, and `pack` stays a
read that appends nothing (I-F5, §1.2).

**Retention.** A row is dropped by the first fold after `idempotency.retention` ([F17 §11.1]) has passed since its
`hlc`, as a `HEADS` session row is (§5.2); a dropped pack cursor leaves C8 empty, as a missing one does.

### 13.2 `SESSMARKS`

One row per (session, agent), sorted by that key. Written by `SessionMark` records ([F05 §9.14]): a record replaces its
key's row.

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 16 | `b16` | `session` | as in `CURSORS` |
| 16 | 16 | `b16` | `agent` | as in `CURSORS` |
| 32 | 8 | `u64` | `rev` | the `commit_seq` at which the rules were shown |
| 40 | 8 | `u64` | `hlc` | the record's HLC |
| 48 | 8 | `HeapRef` | `rules` | `list<u32>`: the rule nodes' `#N`, in the order shown |
| total | 56 | | | |

**Retention.** As `CURSORS`.

### 13.3 `BACKUPS`

One row per backup directory, sorted by `dir`. Written by `Backup` records ([F05 §9.13]): a record replaces the row of
its directory.

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 8 | `u64` | `committed_lsn` | the `committed_lsn` of the `HEAD` state the backup copied |
| 8 | 8 | `u64` | `hlc` | the record's HLC; `backup.max-age` is measured from the greatest `hlc` of the table |
| 16 | 32 | `b32` | `digest` | BLAKE3-256 of the backup's manifest ([F16]) |
| 48 | 8 | `HeapRef` | `dir` | `text`: the backup directory, absolute, in the machine-local form of [80 §2.10] P12 |
| total | 56 | | | |

**Retention.** `gc` drops every row except the newest (by `hlc`) whose `hlc` is older than `gc.reflog-expire`
([F17 §11.2]).

## 14. Worked example *(informative)*

A `MARKERS` snapshot section with one row: task `#40`, `settled` with status `done` (no `complete` outcome), origin
commit `id16` = `00112233445566778899aabbccddeeff` (synthetic) on ref 3 at `ref_seq` 17, written by a commit's ops that
is not a `complete`, so the entry carries no holder and `actor` is 0 ([F05 §9.5] field 9, MF-009; pass 1, closure
NC-12), linear, held by ref 3 alone, `hlc` = `0x01A0C4506C000003` (the example of [F01 §5.7]), `seq` = 4,470,
`emit_lsn` = 1,193,024.
The body is 100 bytes: `RtHdr`, the row, and a 4-byte heap holding the holder list.

```
RtHdr   01 00 00 00  48 00  01  00  00 00 00 00  00 00 00 00  04 00 00 00 00 00 00 00
row  0  28 00 00 00                                      n = 40
     4  03 00 00 00                                      ref_id = 3
     8  00 11 22 33 44 55 66 77 88 99 aa bb cc dd ee ff  commit
    24  01 01 01 00                                      settled, status done, cause ops, flags = 0 (linear)
    28  11 00 00 00                                      ref_seq = 17
    32  00 00 00 00                                      actor = 0 (not a complete)
    36  00 00 00 00                                      outcome 0 (none: not a complete), reserved
    40  03 00 00 6c 50 c4 a0 01                          hlc
    48  76 11 00 00 00 00 00 00                          seq = 4470
    56  40 34 12 00 00 00 00 00                          emit_lsn = 1193024
    64  00 00 00 00  04 00 00 00                         holders: heap off = 0, len = 4
heap 0  03 00 00 00                                      holders = [3]
```

## Coverage

| Item | Part covered here | Section |
|---|---|---|
| [60 §2.5] [AR] row "Segments": `REFS`/`PINS`/`HEADS`/`MARKERS` | the four folded tables' layouts, keys, forms and lookup; `SegHdr` and tags are [F09]'s | §2, §3, §4, §5, §7 |
| [60 §2.5] audit row "Segments": `MARKERS`/`MARKERS_OLD` with the marker key `(#N, ref_id, commit)` | complete: rows, key, split, inertness move, retention | §7 |
| [60 §2.5] audit row "Segments": `LEASES` with the holder anchor, the deadline form `{wall, boot_hash, mono}` and captured `files_owned` globs | complete (the `Anchor` bytes are [F03]'s, the `Stamp` bytes [OS/clock §3.1]'s) | §6 |
| [60 §2.5] audit row "Segments": `ALLOC` with uid and `UIDX` | complete | §9 |
| [60 §2.5] audit row "Segments": `ANCHORRES`, `GLOBIDX`, the `TREES` epoch list and dirty row | complete | §11, §12.4, §12.13 |
| [60 §2.5] audit row "Ref table": `overlay_ops`, `overlay_bytes` | complete: fields, O(1) maintenance, reset at promotion; the thresholds are [F17 §7]'s | §3.1, §3.6 |
| [60 §2.5] audit row "`Vfs`/`ProjectFs`": `VolumeCaps`, `OsFileId`, `JOURNALCUR`, `DIRMAP` | the byte layouts; the trait and the per-OS sources are [OS/project]'s | §12.1, §12.3, §12.9, §12.10 |
| [60 §2.5] "Cross-platform" row | the X-F8 layouts; the X-F2 embeddings in `LEASES` and `FSINTENT` | §6, §12 |
| [60 §2.5] "Harness-agnostic interface and pure Rust" row | the `LEASES` fields `kind`, `role`, `run`, `anchor` (session \| session-ttl \| none), `bound`, root session | §6 |
| [60 §2.5] "Derived-state semantics" row | the runtime tables that `ready`'s runtime clauses read (`LEASES`, `MARKERS`); the semantics are [F13 §4, §6]'s | §6, §7 |
| [40] R-7 | the rows each lazy and durable R4 record kind folds into, the row images the row-carrying kinds hold and the field sources of the others; the record kinds and frames are [F05]'s | §1.3, §2.9, §12 |
| [40] R-8 | `FILEOBS`, `PENDING`, `FSINTENT`, `FPRINT`, `JOURNALCUR`, `DIRMAP` (derived-optional), `TREES`, `PREFIXEV`, `GITRENAMES` (with `ANCESTRY`), `ANCHORRES`, `GLOBIDX`; `PATHIDX`, `ALIASIDX`, `ANCHORS`, `ANCHOR_UID` are [F09]'s | §11, §12 |
| [40] R-9 | the `FPRINT` key `oid` → fingerprint blob id; the blob class is [F10]'s and the fingerprint value [F20 §2.6.4]'s | §12.8 |
| [40] R-15 | the binding row: [F18 §3.2]'s `BindingExt` embedded verbatim (expected git ref, base commit, `designated`) and the section-level uniqueness check; the extension's bytes and I-F12's text are [F18]'s | §5 |
| [40] R-18 | complete: the `FILEOBS` row with the tagged `OsFileId` and parent-directory id, i64 ns timestamps with granularity incl. ctime and added time, attributes, `verified_at` | §12.1, §12.2, §12.5 |
| [50] F11 | complete: the `CONFLICTS` section | §10 |
| [50] F17 | complete, widened to [AR §4.4]'s `(uid, ref_id, create_seq)` with `UIDX` (conflict recorded) | §9 |
| [80] X-F2 | the lease deadline form and the anchor and `ProcId` embeddings in `LEASES` and `FSINTENT`; the anchor layout is [F03]'s and the liveness rules [OS/proc §6.2]'s | §6, §12.7 |
| [80] X-F5 | the durability class of the records each table folds, for orientation; the record tags are [F05]'s, the classes [F15]'s and [F16]'s | §1.3 |
| [80] X-F7 | the `TREES` key and the `HEADS` binding key over the P9 canonical root, and the stored root `OsFileId` for lookup by id first; the algorithm is [OS/path §4]'s | §5, §12.4 |
| [80] X-F8 | complete for the layouts: `OsFileId`, timestamps, `JOURNALCUR`, `DIRMAP`, the `TREES` additions with the 16-byte `VolumeCaps` snapshot, the `FSINTENT` holder; the R-14 rules are [F20]'s | §12 |
| [90 §10.1] `LEASES` runtime rows | complete | §6 |
| [90 §10.1] holder anchor (X-F2 amendment) | the anchor field of `LEASES` and its allowed kinds (0, 1, 4); the anchor layout is [F03]'s | §6 |

## Holes

None. The M0 measurements that touch these tables decide values owned by other chapters — the promotion thresholds and
G15/G16 (measurement 3, [F17 §7], [F05]), the pin and GC policy (measurement 5, [F17]), and the R-14 constants the rows
record ([F20]). Every byte of this chapter is fixed by the design or decided here.

## Open points for the review

1. **The `MARKERS` field set** (gap of [PLAN §3.3], WP-13). [AR §4.4] lists `{#N, ref_id, commit id16, kind, ref_seq,
   hlc, seq}`; [AR §2.16] and [AR §5d.1] add `holder` and `outcome` to `settled`. Resolution (§7): one 72-byte row for
   all three kinds with `status` (the hold's status: `done` or `cancelled`), `outcome` (the `complete --outcome` value
   of MF-009, which [API §10.5] and the `markers` member of [API §15.7] print; pass 1, A1-6), `actor` (the design's
   `holder`: the holder of the lease a `complete` presented, else 0, as [F05 §9.5] field 9 carries it; pass 1, round 3,
   closure NC-12), and the fields the cache of [RULES/state-definition] §6 needs, which review pass 1 adopted (S1-16):
   `ref_id` and `commit` name the hold's origin (MF-003, MF-004), `holders` lists the live work refs that hold it
   (MF-006, a heap list that replaces the first draft's `orig_ref_id` and "re-attribution"), `flags` carries `nonlinear`
   (MF-007), `cause` says which event last changed the row, and `emit_lsn` names the newest record that did. Every field
   has its source in [F05 §9.5]. Cost: 72 B per row plus 4 B per holder in `MARKERS` (dozens of active rows, [AR §8.3]),
   72 B per row in `MARKERS_OLD` against [AR §4.4]'s 40 B estimate; ≈ 2.2 MB a year in `MARKERS_OLD` at ≈ 30,000 markers
   a year.
2. **The marker key is not unique over time.** A marker can be cleared and later re-emitted (ME-003, ME-006), and an
   inert one can return from `MARKERS_OLD` (ME-013). The identity stays `(#N, ref_id, commit)` ([60 §2.5]); `MARKERS`
   keeps one row per identity (its newest state), and `MARKERS_OLD` rows sharing it are ordered by `emit_lsn`, which is
   part of the sort key. A hold has one value, so one identity never needs a `settled` and a `deleted` row at once.
3. **Lease id.** No lease-id allocator exists in the design. Resolution (§6.2): `lease_id` is the fencing token the
   claim allocates from `HEAD.fence`; `token` is kept as its own field because [AR §4.4] lists both. [F04] and [F16] state
   that each claim allocates exactly one token.
4. **`LEASES.run`** is the `#N` of the run node; the displayed run name is [F19]'s and [API]'s. The design names runs
   (`r7`, `run:<id>`) without saying what the id is.
5. **`LEASES.ttl_ms` and `claimed_hlc`** are added: the half-TTL renewal rule needs the lease's own TTL, which a later
   `config` change must not alter, and `reclaim --older-than` needs the claim time.
6. **O(1) overlay counters** (§3.6). [AR §4.2] requires `overlay_ops`/`overlay_bytes` in O(1) per commit and per sync;
   a sync adds `main`'s whole window, which needs cumulative totals. Resolution: `ops_total`, `bytes_total` per ref and
   the marks `trunk_mark_ops`, `trunk_mark_bytes`; the fork value reads `main`'s totals from the pinned set's `REFS`
   snapshot. A fork from a ref other than `main` copies that ref's counters. The counters are class I ([F17 §7]), so the
   definition affects only when promotion happens.
7. **Ref kind `orphans`** (value 6) is added: [AR §5a.1] names `orphans/<n>` refs but lists only five kinds, none of
   which fits refs written only by recovery. [F12] confirms or maps them onto another kind.
8. **The move cache** stores the lsns of the last 32 moving records instead of copies of the moves (8 B instead of
   ≈ 40 B each). `REFS` is then ≈ 0.8 KB per ref with 50 absorbed entries, ≈ 42 KB at 50 refs (est.), against
   [AR §5a.3]'s ≈ 18 KB, which counted no move cache and no cumulative counters; copies of the moves would add ≈ 1 KB
   per ref more.
9. **Ref ids start at 0 with `main`** ([F02 §6.2] allows 0), and `REFS.aux` keeps `next_ref_id` so that ids stay unique
   after expired rows are dropped. [F04] and [F16] state `init`'s initial ref table.
10. **Pins** name a checkpoint set by the lsn of the `Checkpoint` record that published it; `REFS.base_pin` uses the same
    value. [F05] gives that record the file list of the set.
11. **`FileFamily`** (§2.5) is defined here because `PINS` needs it, and is the one owner of the values (pass 1, S1-37):
    [F09] (`seg_kind`, `FILES`) and [F10] reuse them; [F04]'s `SegRef.kind` keeps its own three values for what
    `HEAD.segments` may hold ([F10] OP-10-16).
12. **Bindings are `HEADS` rows** of kind `directory` carrying [F18 §3.2]'s `BindingExt` verbatim (§5; pass 1, S1-10,
    A1-8, P1-2 replaced the earlier `binding`/`designated` flags, `OidSlot` base and full-name `git_ref`; the short ref
    form is [F18 §3.2] rule 1's): [AR §5a.4] resolves a branch through both checkouts and bindings by the same directory
    key, and [40 §2.6] places the R-15 binding rows "in [AR]'s `HEADS` bindings". The keys of kinds `client` and
    `session` are `blake3_16` of their text, like [OS/path §4.5]'s directory key; the `kind` in the sort key separates
    them, instead of an `lp()` domain prefix ([F01 §7.3]).
13. **`IDEM` fields** (§8). [AR §4.4] gives `(key16, payload16, branch_sym, result blob ref)`. Added: `ref_id` and
    `ref_seq`, because the exception "merged into the caller's branch or deleted after merge" needs a never-reused ref
    id and a position to test against the absorbed vector (a name symbol can be reused by a new ref); `append_hlc` for
    both retention windows; the `default_key` bit that [F17] OP-17-16 asks for; and `origin_lsn` with an inline result
    for non-commit results (a `claim` writes a `Lease` record, not a commit). The hash layout is made canonical by
    inserting in key order.
14. **`IDEM` size**: 72 B per slot at load ≤ 1/2 against [AR §4.4]'s 40 B per key; ≈ 4.3 MB for 30,000 live entries,
    spread over layers, cold pages only. The slot is not narrowed: every added field is needed (open point 13). Pass 1
    (P1-28, S1-36): [F01 §8.3] now cites §8, and WP-81a updates the size rows of [AR §4.4] and [AR §8.1] to 72 B per slot
    at load ≤ 1/2.
15. **`ALLOC` widened** (A1P-11, S-09). [50] F17 gives `#N → (ref_id, create_seq)`, 8 B; [AR §4.4], after [72] M7, gives
    `(uid, ref_id, create_seq)` plus `UIDX`. The precedence rule would select [50]'s form for its own reservation, but I1
    and I-F2 need the uid across unmerged branches and [50 §5.9] already relies on `UIDX`. This chapter follows [AR §4.4]
    and records the conflict: 24 MB instead of 8 MB at 1e6 ids, cold.
16. **Settle epochs** (A1P-04, adopted). The 32-byte epoch of [40 §2.6] with `scope_kind` 0 `full-tree`, 1
    `lane-owned`, 2 `partial` (FB-1: the brief's items, `--scope`, `--path`) and `scope_ref`, decidable coverage for
    `full-tree` and `lane-owned`, `partial` never advancing `verified_at` (its digest is zero), and one epoch per
    (`scope_kind`, `scope_ref`). Pass 1 (A1-6, S1-8) replaced this chapter's earlier 1-based numbering and its `brief`
    value with the design's, which [F05 §9.23] already used. The digest functions are fixed here (§12.4); which globs and
    items a settle's scope holds is for [F16] and [40 §4.2] to state. The `TreeReg` frame is [F05]'s (WP-11), the byte cost
    per settle [F17]'s (WP-16).
17. **The journal-cursor reference** of [40 §2.6]'s `TREES` row is the tree's root `vol_key`; no field is stored, since
    `JOURNALCUR` is keyed by volume, and [F05 §9.23] no longer carries one (pass 1, A1-6).
18. **The sensitivity map** stores only directories whose equivalence differs from the volume default, so a tree on an
    ordinary NTFS volume stores none.
19. **`FILEOBS` names and size.** Field names follow [F20] (`file_id`, `parent_dir_id` = `file_id.parent`, `creation`).
    The row is 187 B plus its heap slice against [40 §2.6]'s 96–150 B: the X-F8 layouts (a 57-byte `OsFileId` and four
    9-byte `FsTime`s) postdate that estimate. Rows are written only on change ([70] S8), so the cost is per changed file.
20. **Granularity.** [F20 §1.2] reads "a granularity G in nanoseconds as [F11] records it"; the stored byte is a decimal
    exponent ([OS/project §3.3]). §12.2 states G = `max(10^gran, VolumeCaps.mtime_granularity_ns)`, and [F20 §1.2]
    cites it (pass 1, round 3). Closed.
21. **Two statements of the X-F8 bytes.** `OsFileId`, `FsTime`, `FileAttrs`, the `VolumeCaps` snapshot and `JOURNALCUR`
    are laid out both here (the chapter map of `docs/spec/README.md` assigns them to [F11]) and in [OS/project §3–§4,
    §7.1] (which fixes their OS-defined values). The tables agree byte for byte at the time of writing: re-checked field
    by field in pass 1, round 2, after [OS/project §4.2] added `VolumeCaps` flag bit 14 `dir_flush_doubtful` (P1-16),
    which §12.3 now restates with bits 15–31 reserved-zero (closure NC-1). Proposal: [F11] owns the bytes and
    [OS/project] the values, following [F01 §2.4] rule 4; one of the two then cites the other only.
22. **Retention this chapter decides** where the design is silent: closed `FSINTENT` rows for `gc.trash-expire` after
    closing; `PREFIXEV` rows until the `observed` move is recorded, at most 30 days; `DIRMAP` and `PENDING` rows dropped
    with the tree's `FILEOBS` rows; `HEADS` session rows, `CURSORS` and `SESSMARKS` rows after `idempotency.retention`;
    `BACKUPS` rows other than the newest after `gc.reflog-expire`.
23. **Intent ids** are the lsn of the `FsIntent` record, as [F02] open point 7 proposes; `trash/<intent>/` uses it.
24. **`GITFACTS` is two sections**, `ANCESTRY` ([AR §4.4]) and `GITRENAMES` (R-8), with commit and author times in
    `GITRENAMES`. Pass 1 (A1-6, P1-2) aligned `GITRENAMES` with [F05 §9.25]'s facts: paths are `vbytes`, groups carry
    their blob id, and the rename and time parts come from facts of types 2 and 3. Retention per A1P-16 (§12.12).
25. **`ANCHORRES`** stores state, detail, a line span and an exact score: 78 B against [70] S7's ≈ 32 B (the key alone,
    anchor uid plus `oid` plus version, is 51 B). `unverified` is never cached. The state and detail are [F18]'s codes
    (pass 1, S1-9: detail 62 or 63 replaced this chapter's own 1 and 2), and [F05 §9.26] carries the row image, so its
    score is the exact rational here. Retention per A1P-16 (§12.13).
26. **Derived-optional** (§2.8): `DIRMAP` by [80 §2.11.2]; `FPRINT`, `ANCHORRES`, `ANCESTRY` and `GITRENAMES` proposed,
    because [40 §2.6] calls them derivable and [F20 §1.3] output-neutral. Every other section is required.
27. **Every checkpoint segment carries every snapshot section** (§2.4). This keeps "the newest segment is authoritative"
    true across runtime-only folds ([AR §4.5] step 12), which otherwise would need a per-section fold bound. The snapshot
    tables are small (≈ 150 KB together at the owner's scale, est.: `REFS` ≈ 42 KB, `LEASES` ≈ 50 KB at a few hundred
    live leases, `HEADS` ≈ 25 KB, `TREES` ≈ 20 KB at 46 trees, the rest a few KB). Consequence for [F16]: a
    runtime-only fold writes a delta segment with every snapshot section and its layered runtime sections, and no graph
    section.
28. **Section tags.** §2.8's `0x0201`–`0x021A` (with `0x020A`–`0x020C` for §13's tables, pass 1) are the values
    [F09 §3.1] registers. Closed.
29. **Symbol classes.** [F01 §8.2]'s list of fields gains `LEASES.holder` and `MARKERS.actor` (`actor`), `PREFIXEV.root`
    (`root`) and `GLOBIDX.field` (`name`).
30. **Tables beyond the work package's list.** `PENDING`, `FSINTENT`, `FPRINT`, `JOURNALCUR`, `PREFIXEV`, `ANCESTRY` and
    `GITRENAMES` are specified here because [F20], [OS/proc §1], [OS/project §1] and A1P-16 cite [F11] for them and no
    other chapter holds them. The review confirms the placement.
31. **`CONFLICTS` encodings** — closed in pass 1 (S1-7, S1-5, P1-1). The key is [F06 §6.1]'s `ckey`, each side
    [F06 §6.2]'s `kval` of the key's class (not [F08]'s value encoding, which holds field values only), `prov` follows the
    class, and the class is [F12 §6.1]'s code. An absent side is its `kval`'s own `absent` form, never an empty slice;
    schema keys have `n` = 0 and follow `SCHEMA`'s fold ([F12] open point 18).
32. **`GLOBIDX` literal prefix.** [40 §2.4] defines it "up to the last `/` before the first wildcard"; for a glob without
    a wildcard this chapter takes its last `/`. `[` counts as a wildcard, as in git's glob syntax.
33. **Record payloads** ([F05], WP-11) — adopted in pass 1 (A1-6, S1-8, P1-2) as §2.9: the row-carrying kinds hold the
    row's image (fixed part, then heap slices), so replay and fold share one codec; the event kinds keep [F05]'s fields,
    and each table here names the record field behind every row field (§3.9, §4, §6, §7, §12.4, §12.7, §12.12, §13).
    Codes are cited from their owners, never renumbered: [F18 §4.10] for states, details and evidence, this chapter for
    pin holders, epoch kinds, intent outcomes and abort reasons.
34. **Validation split** (§2.7): cheap header and bounds checks on every open; full row checks by the oracle and
    `doctor --fsck`, following [F01] open point 6.
35. **`REFS.tip` of a ref with no commit.** A ref may exist before its first commit (`main` after `init`, if [F16]'s
    initial state creates no commit); `tip` and `tip_lsn` are zero then.
36. **The `ref_seq` of a re-attributed marker** — closed in review pass 1 (S1-16). The holder-set cache never moves a
    marker to another ref: `branch -D` of X only removes X from the holder sets, and a marker keeps the `ref_id` and
    `ref_seq` of its origin ([RULES/state-definition] ME-005), so the question no longer arises.
37. **Anchor kinds per table.** `LEASES` admits anchor kinds 0, 1 and 4; `FSINTENT` admits kind 2 only ([80 §2.7.2]).
38. **`ops_since_fork`** is kept (it is in [AR §4.2]'s field list and printed by `branch --list`), although [70] S1
    replaced it as a promotion trigger.
39. **Session and backup tables** (§13; pass 1, A1-23). [F05 §7] named "[F11]" tables for `Lazy` cursors, `SessionMark`
    and `Backup` that did not exist. They are added with the tags `0x020A`–`0x020C` of [F09 §3.1]'s reserved range, rather
    than read by replay only: after retirement a record leaves the active log, and `backup.max-age` needs the newest
    backup's HLC. [F05] open point 14's alternative (the cursor and mark in the `HEADS` session row) cannot hold one per
    agent of a session.
40. **`FILEOBS` details** (§12.5; pass 1, S1-9, A1-6). The row stores [F18 §4.6] codes with their slot values instead of
    a rendered detail string, and the moved-auto target with its exact token, so a recorded state renders unchanged from
    the row ([F20 §5.4] step 3) and no text is parsed. Stored states are the ones the cascade decides from observation;
    `deleted`, `absent-in-tree`, `pending` and `planned` are computed at render.
41. **Pass 1, round 2.** `VolumeCaps` flag bit 14 `dir_flush_doubtful` restated from [OS/project §4.2] (§12.3; P1-16,
    closure NC-1); the `ALLOC` rows of ids a `Reserve` record allocated are holes until the fold of the bulk `Commit`
    fills them, and stay holes if it never lands (§9.1; P1-3, [F09] OP-09-17); `REFS` field sources for `RefUpdate`
    reason 5 `park` (§3.9; P1-3, A1-11); the `RtHdr.form` of the versioned sections `CONFLICTS` and `GLOBIDX` (§2.4;
    [F09] OP-09-13).
42. **Pass 1, round 3.** `CURSORS` holds the per-(agent, task) pack cursor of [AR §7.4] C8 that [RULES/pack-classes]
    PT-028 reads (closure NC-10, the residue of A1-23): `feed` 2 and the task's `#N` in the former reserved bytes 36–39,
    so the row stays 56 B and the key becomes (session, agent, feed, task) (§13.1, §1.3). The task is held as its `#N`,
    as `SESSMARKS` holds its rules and [F05 §9.4] a lease's task, not as its 16-byte uid, which would not fit the row.
    The record field is [F05 §9.11]'s. **Decided 2026-09-28** (OQ-F-3 (b); closure NC-11): the layer that delivers the
    pack appends the record; no M0 command is such a layer ([API] open point 48, PX-011), so no `feed` 2 row exists and
    C8 is empty at M0. The decision changes no byte of the row (spec sync 2a, consistency).
