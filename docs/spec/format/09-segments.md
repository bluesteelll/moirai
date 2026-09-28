# 09 — Segments

| | |
|---|---|
| Title | Segments: the segment header `SegHdr` (with `total_len`, X-F6) and its section table, the section tag registry with the `derived-optional` flag, the section layout classes and how layers compose into a view, every section of [AR §4.4] (node headers, cold columns, `CREATOR`, forward and reverse CSR, `EDGE_PROPS`, frozen bitsets with per-chunk cardinality, `TOPO`, `TOMB`, the view's schema, `FCOL`/`FIDX`, `STATS`, `TERMS`/`POST` with `DOCLEN` and the tokenizer, R4's link sections, `SYMTAB`, the store-level sections and where the runtime tables sit), and the four graph segment kinds: base, delta, promoted branch (with `TOUCH`) and changeset |
| Chapter | [F09], `docs/spec/format/09-segments.md` |
| Status | draft, pass 1 pending |
| Work package | WP-13a (the `09-segments.md` part of WP-13, [PLAN §3.2] item 1), author role R-SPEC-R |
| Sources | [AR §3.1] (the 60-byte header, row = `#N` − 1, cold columns `uid`, `topo`, `defer_until`, `due`, `CREATOR`; the field block); [AR §3.3] (edge key with the R4 discriminator, one CSR entry per (src, dst) for `at`, `pinned_commit` props); [AR §3.5] (the persisted bitsets); [AR §4.1] (rows `seg.base.G`, `seg.dK`, `seg.b<ref_id>.K`, `cs.NNNN`; the rules paragraph: sealing, `total_len`, file count); [AR §4.2] (`SegRef`, `refs_lsn`/`pins_lsn`/`heads_lsn`/`markers_lsn`, `next_id`, `next_anchor`); [AR §4.3] (bulk commits: "delta-segment layout", "readers map it as one more delta layer"; lazy runtime records decoded on use); [AR §4.4] (the whole section: `SegHdr`, the section tables, frozen bitsets, delta segments, hot bytes per node); [AR §4.5] step 12 (delta checkpoint, runtime-only fold, promotion); [AR §4.6] "Reserved in format v1" (the R4/R5 table and the audit, port and harness lists as they touch segments); [AR §4.7] (size check before mapping, `SYMTAB` probed in place); [AR §4.8] (the index table); [AR §4.9] (delta checkpoint, tiered fold, rollup, pins, `MARKERS_OLD`, GC); [AR §4.10] (`doctor --fsck`, `repair --rebuild-from-log`); [AR §5a.1] (ref, pin); [AR §5a.3] (the view formula, promotion, `TOUCH`); [AR §5d.1] (versioned versus store-level runtime state); [AR §2.11] T11 (tier-2 text search: front-coded dictionary, delta-varint postings per delta segment); [AR §2.12] T12 (schema as data per branch); [40 §2.4] (`PATHIDX` order), [40 §2.6] (runtime tables), [40 §2.7] (anchor handles `aN`, anchor uid), [40 §2.8] (`at` edges and the `ANCHORS` key), [40 §2.11] R-4, R-8, R-9, R-18 (authoritative); [50 §5.5] (search tokenisation "fixed in the format spec with a version byte", ranking statistics on the view), [50 §8.1] F3, F4, F5, F6, F7, F11, F12, F13, F17; [80 §2.5] rule 4, [80 §3.1] X-F6, X-F8, [80 §3.2] (8-byte section alignment); [60 §2.5] ([AR] row "Segments"; audit row "Segments"; the R4 and R5 tables), [60 §2.6] (the section-producer registry and its layout classes); [PLAN §3.2] WP-13, [PLAN §3.3] (WP-13's gaps: `MARKERS` field set, which sealed files carry `SegHdr`); `docs/spec/reviews/a1-S.md` S-09, S-22 and `a1-P.md` A1P-16 (dispositions in the open points); the sibling drafts [F04 §4.1] (`SegRef`), [F06 §7.3, §8, §9] (`prev`, bodies, bulk commits BK-1–BK-6), [F08 §3, §8.3, §8.4.3, §10.1] (`NodeHdr`, store-local schema ids, `index`, edges) and [F11 §1.3, §2] (runtime section bodies, forms and proposed tags), with which this chapter is reconciled (OP-09-23) |
| Depends on | [F01], [F02]; cites [F04], [F05], [F06], [F07], [F08], [F10], [F11], [F12], [F13], [F15], [F16], [F17], [F19], [F20], [OS/map], [OS/fs], [LQ/std] |

## 1. Scope

This chapter specifies the byte layout of the four **graph segment kinds** — the base segment `seg.base.<G>`, the delta
segment `seg.d<K>`, the promoted-branch segment `seg.b<ref_id>.<K>` and the changeset segment `cs.<n>` of a bulk commit —
and of the segment header `SegHdr` that these and the `hist` and `blobs` files share ([F10] owns those two files' sections).
It fixes:

- `SegHdr` and the section table (§2);
- the section tag registry, the placement of every section in every segment kind, and the `derived-optional` flag (§3);
- the layout classes of the sections this chapter owns, and the rules by which the layers of a view compose (§4);
- every versioned section of [AR §4.4], `SYMTAB`, and the two store-level registries `SCHEMAIDS` and `FILES` (§5–§15);
- what distinguishes each segment kind: tiered folds, promotion and `TOUCH`, the bulk-commit sections (§16);
- validation, and the canonical-encoding rule that makes a rebuild byte-identical (§17).

It does not specify: the `NodeHdr` bytes, the field-block encoding, the value encodings, the anchor record and the schema
items ([F08]); the op, violation and conflict-key encodings ([F06]); log records and record kinds ([F05]); the section body,
rows and forms of the runtime tables and of `CONFLICTS` and `GLOBIDX` ([F11]); `HEAD` and its `SegRef` ([F04]); the
protocol that writes, publishes, pins and deletes segments ([F16]); the thresholds that trigger them ([F17]).

**Terms.**

- **Layer**: one mapped graph segment. A **view** is the ordered stack of layers plus the log tail that a reader combines
  (§4.7).
- **Main set**: the base and deltas that `HEAD.segments` names ([F04 §4.1]), oldest first. A **pinned set** is an older
  main set that a pin keeps alive ([AR §5a.3], [F11 §4]).
- **Row**: the state of one `#N` in one layer. A **touched row** of an upper layer (delta, branch, changeset) is a row
  whose state that layer replaces; the upper layer's `IDS` lists them.
- **Versioned section**: a section that holds per-branch state and composes along a view's layer stack. **Store-level
  section**: a section that holds store-level state ([AR §5d.1]); it lives only in main-set layers and is always read from
  the current main set, whatever branch a process reads (§4.8).

## 2. The segment header

### 2.1 `SegHdr`

Every graph segment, every `hist` file and every `blobs` file begins with `SegHdr` at offset 0 ([80 §2.5] rule 4;
[F10 §2.3] resolves which sealed files carry it). Integers are little-endian ([F01 §4.1]); the structure is byte-packed
([F01 §4.3]).

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 4 | `[4]u8` | `magic` | `"MSEG"` = `4D 53 45 47` ([F01 §4.5]) |
| 4 | 2 | `u16` | `format` | format version, 1 ([F01 §9.1]) |
| 6 | 1 | `u8` | `seg_kind` | the file's family in [F11 §2.5]'s `FileFamily` registry: 3 `seg-base`, 4 `seg-delta`, 5 `seg-branch`, 9 `cs` (this chapter), 2 `hist`, 6 `blobs` ([F10]); every other value is invalid here |
| 7 | 1 | `u8` | `tok_ver` | the tokenizer version byte ([50] F12): 0 = the segment carries no tier-2 text sections; 1 = it carries them, built with tokenizer v1 (§12.1). Values 2–255 are reserved |
| 8 | 4 | `u32` | `n_rows` | the row count of §2.3 |
| 12 | 2 | `u16` | `n_sections` | the number of section-table entries |
| 14 | 2 | `u16` | `_reserved` | reserved-zero ([F01 §10]) |
| 16 | 4 | `u32` | `file_no` | the number in the file's own name ([F02 §6]): G, K or n; 1 to 2^32 − 1 |
| 20 | 4 | `u32` | `ref_id` | for `seg-branch`: the ref id in the file name `seg.b<ref_id>.<K>`; 0 for every other kind |
| 24 | 4 | `u32` | `dict_no` | for `blobs`: the number D of the `dict.<D>` whose dictionary this file's dictionary-coded payloads use, or 0 ([F10 §3.5]); 0 for every other kind |
| 28 | 4 | `u32` | `_reserved` | reserved-zero |
| 32 | 8 | `u64` | `base_seq` | a commit `seq` bound, per kind (§2.3) |
| 40 | 8 | `u64` | `from_lsn` | the first log position folded, per kind (§2.3) |
| 48 | 8 | `u64` | `upto_lsn` | the log position below which the segment folds every record, per kind (§2.3) |
| 56 | 8 | `u64` | `rt_upto_lsn` | the log position below which the segment folds every lazy runtime record of the kinds `K_RT` ([F17 §5.1]), per kind (§2.3, §15.1) |
| 64 | 8 | `u64` | `total_len` | the file's exact length in bytes ([80] X-F6); checked against the file size before mapping ([OS/map §4]) |
| 72 | 32 | `b32` | `seg_digest` | BLAKE3-256 ([F01 §7.1]) over the byte range `[data_off, total_len)` (§2.2): every section and every padding byte between sections |
| 104 | 8 | `u64` | `table_xxh3` | XXH3-64, seed 0, over the section table `[120, data_off)` |
| 112 | 8 | `u64` | `hdr_xxh3` | XXH3-64, seed 0, over `[0, 112)` |
| total | 120 | | | |

`hdr_xxh3` covers `table_xxh3` and `seg_digest`, so one checksum over the fixed header protects the whole file
transitively: `hdr_xxh3` → `table_xxh3` → section table → each section's `xxh3`, and `hdr_xxh3` → `seg_digest` → every
byte after the table.

### 2.2 The section table and the placement of sections

The section table starts at offset 120 and holds `n_sections` entries of 32 bytes. `data_off = 120 + 32 × n_sections`
is a multiple of 8.

**`SecEnt`:**

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 2 | `u16` | `tag` | the section tag (§3) |
| 2 | 2 | `u16` | `flags` | bit table below |
| 4 | 4 | `u32` | `count` | the number of items the section holds, as its layout class defines it (§4; for an [F11 §2.1] body, its `n_rows`); for a frozen bitset its total cardinality ([50] F6) |
| 8 | 8 | `u64` | `off` | the absolute file offset of the section's first byte |
| 16 | 8 | `u64` | `len` | the section's length in bytes; 0 is allowed |
| 24 | 8 | `u64` | `xxh3` | XXH3-64, seed 0, over `[off, off + len)` |
| total | 32 | | | |

**`SecEnt.flags`:**

| bit | name | meaning |
|---|---|---|
| 0 | `derived-optional` | the section is rebuildable from the log; a reader that does not know its tag ignores it; a reader that knows it may find it absent and derives the data otherwise (§3.3) |

Bits 1–15 are reserved-zero.

**Canonical placement** (every rule is checked by validation, §17.1):

- P-1. Entries are sorted by `tag`, strictly ascending; no tag appears twice.
- P-2. Sections are laid out in table order. The first section starts at `data_off`; each later section starts at the
  first multiple of 8 at or after the end of the previous one ([80 §3.2]). The 0–7 bytes between two sections are zero
  (reserved-zero, [F01 §4.3]).
- P-3. `total_len` equals the end of the last section (no trailing bytes). With no section, `total_len = data_off`.
- P-4. A section's `off` is therefore fully determined by the lengths of the sections before it.

*(Informative)* Offsets are absolute file offsets, so a section is addressed directly in the mapping. Readers never assume
alignment ([F01 §4.3]); the 8-byte placement only keeps 64-bit loads over bitmaps within one cache line.

### 2.3 Header fields per segment kind

A field this table marks 0 must be zero. `lsn` values are log positions as [F05] defines them.

| field | `seg-base` | `seg-delta` | `seg-branch` | `cs` | `hist` ([F10 §4]) | `blobs` ([F10 §5]) |
|---|---|---|---|---|---|---|
| `n_rows` | the largest `#N` present in the view it materialises (0 if none) | the number of touched rows (the length of `IDS`) | as `seg-delta` | as `seg-delta` | the number of `Commit` records the file holds | the number of blobs |
| `file_no` | G | K | K | n | n | n |
| `ref_id` | 0 | 0 | the promoted ref's id | 0 | 0 | 0 |
| `dict_no` | 0 | 0 | 0 | 0 | 0 | D or 0 |
| `base_seq` | the largest `seq` of a `Commit` record with `lsn < upto_lsn`, store-wide (0 if none) | as `seg-base` | the `seq` of the ref's tip commit that the segment folds | 0 | the largest `seq` held (0 if none) | 0 |
| `from_lsn` | 0 | the `upto_lsn` of the main-set layer below it | the `upto_lsn` of the newest layer of the pinned set it overlays | 0 | the `lsn` of the first record held | 0 |
| `upto_lsn` | every record with `lsn` below it is folded, none at or above it; equals the `upto_lsn` of the segment's `SegRef` ([F04 §4.1]) | as `seg-base` | the end of the ref's tip commit record | 0 | the end of the last record held | 0 |
| `rt_upto_lsn` | ≥ `upto_lsn`; every `K_RT` record below it is folded into the runtime-window sections (§15.1) | as `seg-base` | 0 | 0 | 0 | 0 |
| `tok_ver` | 0 or 1 | 0 or 1 | 0 or 1 | 0 or 1 | 0 | 0 |

- **Pins.** A branch fork pins the newest main set whose newest layer has `base_seq ≤ fork_seq` ([AR §4.9] "upto_seq ≤
  fork_seq"). `base_seq` is that "upto_seq".
- **`SegRef`.** `SegRef.blake3_16` = `seg_digest[0..16]` ([F04 §4.1]: "the first 16 bytes of the BLAKE3-256 content digest
  that the file's own header … records").
- **Main-set continuity.** In a main set, `seg-base.from_lsn = 0`; each delta's `from_lsn` equals the `upto_lsn` of the
  layer below it; `upto_lsn` and `rt_upto_lsn` never decrease going up; `rt_upto_lsn ≥ upto_lsn` in every layer.

### 2.4 Row numbering

- In a `seg-base`, row `i` (0-based) of every column section is `#N = i + 1` ([AR §3.1]); `n_rows` rows exist.
- In an upper segment (`seg-delta`, `seg-branch`, `cs`), row `i` is the `#N` at `IDS[i]` (§5.1). Column sections are
  parallel to `IDS`.
- `#N` 0 names no node and never has a row ([F08 §2]).

## 3. The section tag registry

### 3.1 Tags

Tags are `u16`. Tag 0 and every tag not listed are reserved: a segment that carries one without `derived-optional` is
invalid ([F01 §9.3]). The class column names the layout class (§4, or "RtHdr body" for [F11 §2.1]'s body); the fold column
names the composition rule of §4.7; the placement columns give, per segment kind, **R** required (present, possibly with
`count` = 0), **C** conditional (the condition is in the section's own subsection), **O** optional and `derived-optional`,
**—** forbidden. "Rows" names the chapter that owns the bytes of one row where this chapter owns only the tag and the
placement.

| tag | name | class | fold | base | delta | branch | cs | rows | section |
|---|---|---|---|---|---|---|---|---|---|
| `0x0001` | `IDS` | column `u32` | — | — | R | R | R | this chapter | §5.1 |
| `0x0002` | `NODE` | column `NodeHdr` (60 B) | row | R | R | R | R | [F08 §3] | §5.2 |
| `0x0003` | `CREATOR` | column (6 B) | row | R | R | R | R | this chapter | §5.3 |
| `0x0004` | `TOPO` | column `u32` | row | R | R | R | R | this chapter | §5.4 |
| `0x0005` | `DEFER` | column `u32` | row | R | R | R | R | [F08] (value) | §5.4 |
| `0x0006` | `DUE` | column `u32` | row | R | R | R | R | [F08] (value) | §5.4 |
| `0x0007` | `UID` | fixed table (20 B) | index | R | R | R | R | this chapter | §5.5 |
| `0x0008` | `TOUCH` | frozen bitset | — | — | — | R | — | this chapter | §16.3 |
| `0x0010` | `TITLE_BLOB` | pool | row | R | R | R | R | this chapter | §6.1 |
| `0x0011` | `FIELDS_BLOB` | pool | row | R | R | R | R | [F08 §6] (block) | §6.2 |
| `0x0012` | `BLOBTAB` | fixed table (36 B) | row | R | R | R | R | this chapter | §6.3 |
| `0x0020` | `OUT_OFF` | column `u32` | row | R | R | R | R | this chapter | §7.1 |
| `0x0021` | `OUT_DST` | column `u32` | row | R | R | R | R | this chapter | §7.1 |
| `0x0022` | `OUT_KIND` | column `u8` | row | R | R | R | R | [F08 §8.3] (edge id) | §7.1 |
| `0x0023` | `IN_OFF` | column `u32` | row | R | R | R | R | this chapter | §7.1 |
| `0x0024` | `IN_SRC` | column `u32` | row | R | R | R | R | this chapter | §7.1 |
| `0x0025` | `IN_KIND` | column `u8` | row | R | R | R | R | [F08 §8.3] (edge id) | §7.1 |
| `0x0026` | `EDGE_PROPS` | fixed table (40 B) | row | R | R | R | R | this chapter; block [F08 §10.2] | §7.2 |
| `0x0030` | `TOMB` | fixed table (16 B) | row | R | R | R | R | this chapter | §8.1 |
| `0x0031` | `CONFLICTS` | RtHdr body | row | R | R | R | R | [F11 §10] | §8.2 |
| `0x0032` | `SCHEMA` | variable table | full (view) | R | C | C | C | [F08 §8] (items) | §8.3 |
| `0x0040` | `BMDIR` | fixed table (4 B) | — | R | C | C | C | this chapter | §9.2 |
| `0x0050` | `TERMS` | term dictionary | index | C | C | C | C | this chapter | §12.2 |
| `0x0051` | `POST` | postings | index | C | C | C | C | this chapter | §12.3 |
| `0x0052` | `DOCLEN` | column (6 B) | row | C | C | C | C | this chapter | §12.4 |
| `0x0053` | `FTSSTAT` | fixed record (48 B) | full (view) | C | C | C | — | this chapter | §12.4 |
| `0x0060` | `STATS` | statistics | full (view) | R | — | — | — | this chapter | §11 |
| `0x0061` | `FPROMO` | fixed table (12 B) | — | C | C | C | C | this chapter | §10.1 |
| `0x0080` | `PATHIDX` | variable table | index | R | R | R | R | this chapter | §13.1 |
| `0x0081` | `ALIASIDX` | variable table | index | R | R | R | R | this chapter | §13.2 |
| `0x0082` | `ANCHORS` | variable table | index | R | R | R | R | this chapter; record [F08 §10.3] | §13.3 |
| `0x0083` | `ANCHOR_UID` | fixed table (28 B) | index | R | R | R | R | this chapter | §13.4 |
| `0x0084` | `GLOBIDX` | RtHdr body | index | R | R | R | R | [F11 §11] | §13.5 |
| `0x00C0` | `PREV` | column `u64` | — | — | — | — | R | this chapter | §16.4 |
| `0x00C1` | `VIOLATIONS` | variable table | — | — | — | — | C | [F06] (op) | §16.4 |
| `0x00C2` | `CKIMG` | variable table | — | — | — | — | C | [F06 §4.4.14] (entry) | §16.4 |
| `0x0100` | `SYMTAB` | symbol table | symbols | R | R | — | — | this chapter | §14.2 |
| `0x0101` | `SCHEMAIDS` | RtHdr body | snapshot / layer | R | R | — | — | this chapter | §14.3 |
| `0x0102` | `FILES` | RtHdr body | snapshot | R | R | — | — | this chapter | §14.4 |
| `0x0201`–`0x020C` | `REFS`, `PINS`, `HEADS`, `LEASES`, `MARKERS`, `MARKERS_OLD`, `IDEM`, `ALLOC`, `UIDX`, `CURSORS`, `SESSMARKS`, `BACKUPS` | RtHdr body | [F11 §1.3] | R | R | — | — | [F11] | §14.5 |
| `0x0210`–`0x021A` | `TREES`, `FILEOBS`, `PENDING`, `FSINTENT`, `FPRINT`, `JOURNALCUR`, `DIRMAP`, `PREFIXEV`, `ANCESTRY`, `GITRENAMES`, `ANCHORRES` | RtHdr body | [F11 §1.3]; runtime window (§15.1) except `FSINTENT` | R or O (§3.3) | R or O | — | — | [F11 §12] | §15.2 |
| `0x0300`–`0x0302` | `HFRAMES`, `HCIDX`, `HDATA` | [F10 §4.3] | — | — | — | — | — | [F10] | `hist` only |
| `0x0310`–`0x0311` | `BLOBIDX`, `BLOBDATA` | [F10 §5.2] | — | — | — | — | — | [F10] | `blobs` only |
| `0x4000`–`0x4FFF` | `FCOL.<slot>` | promoted column | row | C | C | C | C | this chapter | §10.2 |
| `0x5000`–`0x5FFF` | `FIDX.<slot>` | value index | bitset | C | C | C | C | this chapter | §10.3 |
| `0x8000`–`0xFFFE` | `BM.<i>` | frozen bitset (base) or ± list (upper) | bitset | C | C | C | C | this chapter | §9.3 |
| `0xFFFF` | — | reserved | | | | | | | |

The individual tags of the two runtime ranges are those [F11 §2.8] proposes, adopted here:

| tag | name | tag | name | tag | name |
|---|---|---|---|---|---|
| `0x0201` | `REFS` | `0x0207` | `IDEM` | `0x0213` | `FSINTENT` |
| `0x0202` | `PINS` | `0x0208` | `ALLOC` | `0x0214` | `FPRINT` |
| `0x0203` | `HEADS` | `0x0209` | `UIDX` | `0x0215` | `JOURNALCUR` |
| `0x0204` | `LEASES` | `0x0210` | `TREES` | `0x0216` | `DIRMAP` |
| `0x0205` | `MARKERS` | `0x0211` | `FILEOBS` | `0x0217` | `PREFIXEV` |
| `0x0206` | `MARKERS_OLD` | `0x0212` | `PENDING` | `0x0218` | `ANCESTRY` |
| `0x020A` | `CURSORS` | | | `0x0219` | `GITRENAMES` |
| `0x020B` | `SESSMARKS` | | | `0x021A` | `ANCHORRES` |
| `0x020C` | `BACKUPS` | | | | |

`0x020D`–`0x020F` are reserved. `0x020A`–`0x020C` are [F11 §13]'s session and backup tables (pass 1, A1-23). A `hist` file carries only the tags `0x0300`–`0x0302` and a `blobs` file only
`0x0310`–`0x0311` ([F10]); a graph segment never carries them.

### 3.2 Tag ranges

- `FCOL.<slot>` = `0x4000 + slot` and `FIDX.<slot>` = `0x5000 + slot`, slot 0–4,095, where `FPROMO` (§10.1) maps each slot
  to its field ([50] F5, "section-tag ranges").
- `BM.<i>` = `0x8000 + i`, i 0–32,766, where `BMDIR` entry i (§9.2) names the bitset.

### 3.3 `derived-optional`

- In format v1 the flag is set on exactly these sections: `DIRMAP` ([80] X-F8, [80 §2.11.2]) and the four runtime caches
  `FPRINT`, `ANCESTRY`, `GITRENAMES` and `ANCHORRES` ([F11 §2.8]; [40 §2.6] calls them derivable, [F20 §1.3]
  output-neutral). A writer sets it on these and on no other v1 tag; a v1 reader treats the flag on any other known tag as a
  format violation. The five are marked O in §3.1: present or absent in a base or delta.
- A reader that finds one of them absent treats the table as empty: `DIRMAP`'s Linux frontier then enumerates the tree and
  E7 loses its filter ([80 §2.11.3]); the caches recompute on use. Nothing becomes wrong.
- A later format may add a derived index with a new tag and this flag; a v1 reader ignores it, and `repair
  --rebuild-from-log` rebuilds it ([AR §4.4], [74 A23]). A section that is not derivable from the log never carries the flag.

## 4. Layout classes and layer composition

Every section this chapter owns is an instance of one of the layout classes below, or of a special layout its own
subsection defines (`SYMTAB`, `STATS`, `TERMS`, `POST`, `FTSSTAT`, `FCOL`, `FIDX`). The class fixes the section's container
bytes and the meaning of `SecEnt.count`; the section's subsection fixes its rows, keys and order. Sections whose rows [F11]
owns use [F11 §2.1]'s body (`RtHdr`, rows, index region, heap), and so do the two store-level registries of §14.3–§14.4.
These classes are the "layout class" entries of the section-producer registry of [60 §2.6].

### 4.1 Column

`[T; n]`: n elements of the fixed width of T, with no header. n is `n_rows` (base) or the length of `IDS` (upper
segments), or as the section states (the CSR arrays). `count = n`; `len = n × width(T)`.

### 4.2 Fixed table

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 4 | `u32` | `n` | number of rows |
| 4 | 4 | `u32` | `row_w` | the width of one row in bytes; must equal the width the section gives |
| total | 8 | | | |

Then `n × row_w` bytes of rows, sorted strictly ascending by the table's key in tuple order ([F01 §6.6]; each key field
compared as its type). `count = n`; `len = 8 + n × row_w`.

### 4.3 Variable table

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 4 | `u32` | `n` | number of rows |
| 4 | 4 | `u32` | `_reserved` | reserved-zero |
| total | 8 | | | |

Then `off: [u32; n + 1]` with `off[0] = 0` and `off[i] < off[i + 1]` (no empty row), then the row area of `off[n]` bytes.
Row i is the row area's bytes `[off[i], off[i + 1])`. Rows are sorted strictly ascending by the table's key in tuple
order, a byte-string key field compared bytewise ([F01 §6.6]), unless the section states another canonical order.
`count = n`; `len = 8 + 4(n + 1) + off[n]`.

### 4.4 Pool

A byte string whose entries `NodeHdr` addresses by offset ([F08 §3]). The pool is the concatenation of its entries, with
no gap, in the canonical order its section states; an empty pool has `len` 0. `count` is the number of entries.

### 4.5 Frozen bitset

A set of `#N` values ([AR §4.4], [50] F6). The ids are split into chunks of 65,536: chunk `hi = #N >> 16` holds the low
halves `#N & 0xFFFF`.

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 4 | `u32` | `n_chunks` | number of non-empty chunks |
| 4 | 4 | `u32` | `_reserved` | reserved-zero |
| total | 8 | | | |

Then the **chunk index**, `n_chunks` entries of 16 bytes sorted by `hi` strictly ascending:

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 2 | `u16` | `hi` | chunk number |
| 2 | 2 | `u16` | `_reserved` | reserved-zero |
| 4 | 4 | `u32` | `card` | the chunk's cardinality, 1 to 65,536 ([50] F6) |
| 8 | 8 | `u64` | `off` | offset of the chunk's container from the first byte of the section |
| total | 16 | | | |

Then the containers, in chunk-index order, each at the first multiple of 8 (relative to the section's first byte) at or
after the end of the previous item, with zero bytes between:

- **array** when `card ≤ 4,096`: `card` × `u16` low halves, strictly ascending;
- **bitmap** when `card > 4,096`: 8,192 bytes; bit `l` ([F01 §4.4]) is set iff `(hi << 16) | l` is a member.

The form is decided by `card` alone, so the encoding of a set is unique ([F17 §13.2]). The bit for `#N` 0 is never set.
`count` = Σ `card` = the bitset's total cardinality: the segment header's per-bitset total of [50] F6, read from `SecEnt`
without touching the section.

### 4.6 ± list

The change of one set from the layer below to this layer (upper segments only).

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 4 | `u32` | `n_plus` | members added |
| 4 | 4 | `u32` | `n_minus` | members removed |
| total | 8 | | | |

Then `plus: [u32; n_plus]` and `minus: [u32; n_minus]`, each strictly ascending, disjoint, with no 0. Every `plus` id is
not a member below this layer and every `minus` id is one, so the exact total of a view is the base total plus Σ
(`n_plus − n_minus`) over its upper layers ([50] F6, [50 §5.6]). `count = n_plus + n_minus`. A ± list with `count = 0` is
never written; the set is then unchanged.

### 4.7 Layer composition

A reader combines the layers of a view, oldest first, then the tail ([AR §4.4] "overlay → deltas newest-first → base",
[AR §5a.3]):

| view | layers, oldest first |
|---|---|
| `main` | the main set (base, then deltas in `upto_lsn` order), then the tail |
| a branch X | the pinned set of X (`REFS.base_pin`, [F11 §3.1]), then `seg.b<ref_id(X)>.<K>` if X is promoted (`REFS.promoted_seg`), then X's tail as [AR §5a.3] defines it |
| any view | a changeset segment sits at the position of its bulk commit in the replay order: the reader maps it instead of replaying that commit ([AR §4.3], [F06 §9] BK-6) |

"Newer" means later in this order. The fold column of §3.1 names one of these rules:

- **row.** The state of `#N` r is the one held by the newest layer that has r as a row: an upper layer whose `IDS` holds r,
  else the base if `r ≤ n_rows` there. That layer supplies *every* row-scoped section of r: `NODE`, `CREATOR`, `TOPO`,
  `DEFER`, `DUE`, the title, the field block, the body reference, both adjacency lists with their `EDGE_PROPS`, `TOMB`,
  `CONFLICTS` ([F11 §10]: "a reader masks older layers' rows of those nodes"), `DOCLEN`, and each promoted column the layer
  carries (list-replacement semantics, [AR §4.4]). No layer holds a partial row.
- **index.** An entry of a value-keyed index (`UID`, `PATHIDX`, `ALIASIDX`, `ANCHORS`, `ANCHOR_UID`, `GLOBIDX`, `TERMS`/`POST`)
  in layer L belongs to one row r. It is **live** iff L is r's newest layer; a lookup probes every layer and discards
  entries that are not live.
- **bitset.** Membership in the base's frozen bitset, then each upper layer's ± list applied in order. A layer without a
  ± list for a set leaves it unchanged. A set with no frozen bitset in the base is empty there.
- **full (view).** The section describes the whole view as of its layer; the newest layer that carries it wins.
- **snapshot, layer.** The forms of [F11 §2.4], for store-level sections: a snapshot is the complete table as of its
  segment's bound, and the newest main-set layer's snapshot is authoritative; a layer holds the rows that changed after the
  next-older segment (dead rows included where [F11] allows them), probed newest first after the tail. "Snapshot / layer"
  means a snapshot in a base and a layer in a delta.
- **runtime window.** Store-level sections folded from `K_RT` records use `rt_upto_lsn` as their bound instead of
  `upto_lsn` (§15.1).
- **symbols.** `SYMTAB`'s per-class id ranges (§14.2).

Records of the tail at or after the newest layer's `upto_lsn` (`rt_upto_lsn` for `K_RT` kinds) apply on top, as [F05] and
[F16] specify.

### 4.8 Store-level sections

Store-level sections (tags `0x0100`–`0x02FF`) appear only in `seg-base` and `seg-delta` segments, that is in main sets
([F11 §2.4]). A process reads them from the **current** main set that the `HEAD` slot it uses names, whatever view it
reads; the copies in a pinned older set are ignored. This keeps one store-wide value per table ([AR §5d.1]) and keeps
runtime state out of versioned layers (I-F4, I36′).

## 5. Row identity, the node header and cold columns

### 5.1 `IDS`

Column of `u32`: the `#N` of every touched row of an upper segment, strictly ascending, none 0. `n_rows` = its length. A
row is touched, and must be listed, iff any row-scoped section (§4.7) of that `#N` differs from its state in the layers
below; a writer lists no other row. Every row-scoped and index entry of an upper segment belongs to a listed row.

### 5.2 `NODE`

Column of `NodeHdr`, 60 bytes per row ([AR §3.1]; the bytes are [F08 §3]'s). This chapter fixes how the section uses three
of its fields:

- `title_off` is an offset into this segment's `TITLE_BLOB` (§6.1), or `NONE32` (`0xFFFFFFFF`) when the node stores no
  title; `fields_off` is an offset into its `FIELDS_BLOB` (§6.2), or `NONE32` when the node has no stored field; `body_ref`
  is 0 (no body) or i ≥ 1, naming `BLOBTAB` entry i − 1 of this segment (§6.3) ([F08 §3]). [AR §4.4]'s separate
  `TITLE_OFF`, `FIELDS_OFF` and `BODY_REF` sections are not written: `NodeHdr` already carries the three offsets
  (OP-09-05).
- **Absent row.** A row whose `#N` has no node in the view — an id allocated on another branch, or, in a branch segment, a
  node its pinned set holds and the branch never had — has `kind` = 0 and all 60 bytes zero ([F08 §3]). Every other
  row-scoped section holds its zero value for it: zero column elements, empty adjacency lists, no `TOMB`, `CONFLICTS`,
  index or ± entry, no promoted value.
- **Deleted row.** A deleted node keeps its row with its `deleted` flag set and its retained out-edges ([AR §3.1], I39′).

### 5.3 `CREATOR` ([50] F4)

Column of 6-byte elements, set at `Create` and never changed ([AR §3.1]; the value is [F08 §4]'s):

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 4 | `u32` | `actor` | symbol of class `actor` ([F01 §8.2]); 0 for an absent row |
| 4 | 2 | `u16` | `role` | symbol of class `role`; 0 for an absent row |
| total | 6 | | | |

### 5.4 `TOPO`, `DEFER`, `DUE`

Columns of `u32` ([AR §3.1], [AR §4.4]).

- `TOPO`: the node's Pearce–Kelly position in the view's combined precedence graph (I5′); 0 for an absent row. Its
  properties are [F13]'s I9 ("a topological order", OP-13-03). A rollup renumbers the positions ([AR §4.9]).
- `DEFER` holds `defer_until`, `DUE` holds `due`; unit and "none" value are [F08]'s.

### 5.5 `UID`

Fixed table, `row_w` = 20, fold "index" ([AR §4.4], [AR §4.8]):

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 16 | `b16` | `uid` | the node's uid ([F01 §5.6]) |
| 16 | 4 | `u32` | `id` | its `#N` |
| total | 20 | | | |

Key `(uid, id)`, `uid` compared bytewise. The base holds one entry per present row (live or deleted); an upper segment one
entry per touched row that is not absent. `#N → uid` is read from the store-wide `ALLOC` ([F11 §9.1]), which [AR §4.4]
widened with the uid; [AR §3.1]'s cold column `uid` is realised by these two tables and is not written as a third copy
([F08 §4]: "[F09] fixes how a row finds its uid"; OP-09-07).

## 6. Titles, field blocks and bodies

### 6.1 `TITLE_BLOB`

Pool of `vstr` entries ([F01 §6.2]), each the title of one row, 1–200 bytes ([AR §2.6], [F08 §7.1]). Canonical order: the
title of every row that stores one, in row order, one entry per row. `count` = that number of rows.

### 6.2 `FIELDS_BLOB`

Pool of `vbytes` entries; each entry's bytes are one [F08 §6] field block of one row. Canonical order: the block of every
row with a stored field, in row order. `count` = that number of rows.

### 6.3 `BLOBTAB` and `BlobRef`

Fixed table of `BlobRef`, `row_w` = 36, key `hash` ([AR §4.4] `(blake3_16, file u32, off u64, len u32, raw_len u32)`):

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 16 | `b16` | `hash` | BLAKE3-128 of the body's raw stored bytes, its content address ([AR §2.6], [F06 §8] BD-2) |
| 16 | 4 | `u32` | `file` | the number n of the `blobs.<n>` file that holds it ([F10 §5]) |
| 20 | 8 | `u64` | `off` | the absolute offset of its `BlobEnc` payload in that file ([F10 §3.2]) |
| 28 | 4 | `u32` | `len` | the payload's stored length, the codec byte included |
| 32 | 4 | `u32` | `raw_len` | the raw length, at most the body cap of 65,536 ([F17 §13.2]) |
| total | 36 | | | |

- **Entries.** One per distinct `hash` that `body_ref` of this segment's rows names, plus every body that [F06 §8] BD-6
  requires the set to keep ("every body that a `Commit` record after u references without carrying it"). Sorted by `hash`
  strictly ascending, so a reader resolves a body hash by binary search, as BD-6 asks.
- `body_ref` indexes the table 1-based (§5.2).
- `BlobRef` is also the type of any other structure of the format that points at a blob by location.

## 7. Adjacency

### 7.1 Forward and reverse CSR

Six column sections ([AR §4.4]):

| section | element | length |
|---|---|---|
| `OUT_OFF` | `u32` | rows + 1 |
| `OUT_DST` | `u32` (`#N`) | e_out |
| `OUT_KIND` | `u8` (edge id, [F08 §8.3]) | e_out |
| `IN_OFF` | `u32` | rows + 1 |
| `IN_SRC` | `u32` (`#N`) | e_in |
| `IN_KIND` | `u8` | e_in |

- Row i's out-list is `OUT_DST[OUT_OFF[i] .. OUT_OFF[i + 1])` with the parallel kinds; `OUT_OFF[0] = 0`, `OUT_OFF` is
  non-decreasing, and `OUT_OFF[rows] = e_out`. The same for `IN_*`.
- Each list is sorted strictly ascending by `(kind, dst)` (by `(kind, src)` for in-lists), so the slice of one kind is
  contiguous ([50 §5.5] `Expand`).
- An edge key is `(src, kind, dst, disc)` ([AR §3.3], [40] R-4, [F08 §10.1]). The CSR holds **one** entry per
  `(src, kind, dst)`: for `at` the anchors, whose uids are the discriminators, live in `ANCHORS` (§13.3); every other kind
  has an empty discriminator. A symmetric kind is stored once, from the endpoint [F08 §10.1] names as the source.
- **`parent`.** The forward direction of `parent` is `NodeHdr.parent` and is not in the out-lists ([F08 §10.1]). Its
  reverse direction is indexed here: the in-list of a node p holds one entry `(parent, c)` for every child c whose
  `NodeHdr.parent` is p, so a node's children are one contiguous slice of `IN_SRC` (OP-09-24).
- **I-P3.** In the view, `(s, k, d)` is in s's out-list iff `(d, k, s)` is in d's in-list, for every kind k except
  `parent`, for which `NodeHdr.parent` of s equals d iff `(parent, s)` is in d's in-list ([AR §3.4]). An upper segment that
  adds or removes an edge therefore touches both endpoints.
- `SecEnt.count` is rows + 1 for `OUT_OFF`/`IN_OFF`, e_out or e_in for the others.

### 7.2 `EDGE_PROPS`

Fixed table, `row_w` = 40, key `edge` ([AR §4.4] "(edge idx u32, pinned_commit id16)", widened by pass 1: S1-2, A1-3,
P1-1). The row stores the parts of [F08 §10.2]'s edge property block other than the anchor, at fixed places:

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 4 | `u32` | `edge` | the index of the edge in this segment's `OUT_DST`; less than e_out |
| 4 | 1 | `u8` | `pflags` | [F08 §10.2]'s `pflags` with bit 2 (`anchor`) clear: bit 0 `has_pin`, bit 1 `flagged`; not 0 |
| 5 | 3 | `[3]u8` | `_reserved` | zero |
| 8 | 32 | `b32` | `pinned_commit` | with `has_pin`: the full 32-byte id of the pinned commit ([F08 §10.2]; [F07 §8.1] hashes it and the image writes it); all zero otherwise |
| total | 40 | | | |

- **Entries.** One row per out-edge of this segment's rows whose property block, without its anchor, is not the single
  byte `00`: an edge of a kind whose `props` ([F08 §8.4.6]) is `pinned` — `derived_from`, `cites`, `implements` and
  `consumed` among the core kinds ([F08 §9.6]) — that carries a pin, and an edge of a kind whose `props` is `flagged` —
  `blocks` and `gates` — retained with `flagged` by a tombstone source (I39′, [AR §5b.2] rule 8). An edge with `pflags`
  0 has no row.
- **Composition.** The block of an edge i whose kind is not `at` is its row's `pflags` (`00` when it has no row),
  then `pinned_commit` when `has_pin`. An `at` entry of the CSR stands for one edge key per `ANCHORS` row of its
  (src, dst) (§7.1, §13.3; the key's disc is that anchor's uid, §13.4), and each such key's block is `pflags` = `04`
  (`anchor` alone, as [F08 §10.2] requires of kind `at`) followed by that row's anchor record; an `at` edge never has an
  `EDGE_PROPS` row. So a view rebuilt from segments reproduces every byte [F07 §8.1] hashes: the `flagged` bit, the
  whole pin and each anchor record, from which [F07 §8.2]'s selector block is derived.

## 8. Versioned records

### 8.1 `TOMB`

Fixed table, `row_w` = 16, key `id` ([AR §4.4] "`{id, tx, reason_sym, replaced_by}` 16 B"):

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 4 | `u32` | `id` | the deleted node's `#N` |
| 4 | 4 | `u32` | `tx` | the `seq` of the deleting commit (u32, like `created_tx`, [AR §3.1]) |
| 8 | 4 | `u32` | `reason_sym` | symbol of class `reason` ([F01 §8.2]); 0 = no reason |
| 12 | 4 | `u32` | `replaced_by` | `#N` of the replacement, or 0 |
| total | 16 | | | |

A layer holds an entry for a row iff the row is deleted in that layer's state (§4.7 "row"), as [F08 §3] states for the
deleted `NodeHdr`.

### 8.2 `CONFLICTS` ([50] F11)

The unresolved conflict values of the view. Its body, rows and key `(n, key bytes)` are [F11 §10]'s: each side is the
[F06 §6.2] `kval` of its key's class, with `prov` for an existence key (pass 1, S1-7). This chapter fixes its tag
(`0x0031`), its presence in every graph segment kind, and its fold "row": a layer holds the complete set of conflict
values of each of its rows and no other. Rows of schema keys (`n` = 0) are not row-scoped: they fold with `SCHEMA`
("full (view)", §8.3), so a layer that carries `SCHEMA` carries every schema-key conflict row of its view and a layer
without `SCHEMA` carries none. Structural violations never land ([AR §5a.8]) and have no row here.

### 8.3 `SCHEMA`

Variable table of the view's schema ([AR §2.12] T12; [50] F1–F3): kinds, fields, edges and `QUERIES` items, each row one
schema item in [F08 §8]'s item encoding, sorted by [F08 §8.5]'s item key order (class, then the component name strings
bytewise, `*` = `2A`; pass 1, A1-41). Fold "full (view)": the base carries the whole schema of
its view; an upper segment carries `SCHEMA` iff a `Schema` op in its window changed the schema, and then carries the whole
schema of its view. [AR §4.4] lists no schema section although schema is versioned per branch; this section closes that gap
([F08 §8.1] "[F09] stores the view's items in a segment section"; OP-09-08). A named query's canonical-AST hash, derived and
unhashed ([50] F3), is part of its row as [F08] lays it out.

## 9. Bitsets

### 9.1 What is persisted

The persisted bitsets are those of [AR §4.4] `BM_*`: per kind, per (kind, status), and the predicates `unblocked`,
`is_blocker`, `deleted`, `suspect`, `conflicted`, `container`, `has_dangling` ([AR §3.5]; `ready` is never persisted,
[50 §3.8]). Their membership definitions are [F08]'s and [F13]'s; this chapter fixes identities and bytes.

### 9.2 `BMDIR`

Fixed table, `row_w` = 4, of bitset keys sorted strictly ascending (bytewise, which equals tuple order for four `u8`):

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 1 | `u8` | `family` | 1 kind, 2 kind-status, 3 predicate |
| 1 | 1 | `u8` | `kind` | the kind id ([F08 §8.3]) for families 1 and 2; 0 for family 3 |
| 2 | 1 | `u8` | `status` | the status value ([F08]) for family 2; 0 otherwise |
| 3 | 1 | `u8` | `pred` | family 3: 1 `unblocked`, 2 `is_blocker`, 3 `deleted`, 4 `suspect`, 5 `conflicted`, 6 `container`, 7 `has_dangling`; 0 otherwise |
| total | 4 | | | |

Entry i names the bitset whose section tag is `0x8000 + i` (§3.2). Values not listed are invalid.

- **Base.** `BMDIR` lists exactly the non-empty bitsets of the view; each is a frozen bitset (§4.5) whose `SecEnt.count` is
  its total ([50] F6).
- **Upper segments.** `BMDIR` lists exactly the sets whose ± list is non-empty in this layer; each `BM.<i>` is a ± list
  (§4.6). `BMDIR` is absent when no set changed.

### 9.3 `BM.<i>`

A frozen bitset in a base segment, a ± list in an upper segment, for the key at `BMDIR` entry i. Both encodings are
canonical, so the section bytes are a function of the set (or of the two sets it relates).

## 10. Promoted fields ([50] F5)

### 10.1 `FPROMO`

Fixed table, `row_w` = 12, key `field_sym`, present iff the segment carries any `FCOL` or `FIDX`:

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 4 | `u32` | `field_sym` | the field's name, symbol class `name` ([F01 §8.2]) |
| 4 | 2 | `u16` | `slot` | 0–4,095; the tags `FCOL.<slot>` and `FIDX.<slot>` |
| 6 | 1 | `u8` | `vtype` | the type id ([F08 §5.1]) of the promoted element: the field's type, or for a `set` field its element type |
| 7 | 1 | `u8` | `index` | [F08 §8.4.3]: 1 `column`, 2 `bitmap` |
| 8 | 1 | `u8` | `form` | 0 scalar field, 1 `set` field |
| 9 | 3 | `[3]u8` | `_reserved` | reserved-zero |
| total | 12 | | | |

- The rows are the fields with `index ≠ none` in the view's schema as of this layer. `slot` is the row's position in the
  table, so slots are dense and follow `field_sym` order (canonical).
- **Which sections** ([F08 §8.4.3]): `index` = 1 gives an `FCOL.<slot>`; `index` = 2 gives an `FIDX.<slot>`, and also an
  `FCOL.<slot>` unless the field's type is `set`. A base carries every section its rows imply; an upper segment carries
  every `FCOL` its rows imply and an `FIDX` only when some value's set changed (§10.3).
- A promoted field has one value type in the whole view. Its element must have a fixed-width **promoted encoding**:

| `vtype` ([F08 §5.1]) | `elem_w` | promoted encoding |
|---|---|---|
| 1 `bool` | 1 | `00` false, `01` true |
| 2 `int`, 3 `counter` | 8 | `i64` |
| 4 `f64` | 8 | `f64` ([F01 §5.5]; −0.0 is stored as +0.0 and NaN never occurs, [F08 §5.3]) |
| 5 `enum` | 2 | `u16`: the enumeration value's integer ([F08 §8.3]) |
| 7 `sym` | 4 | `u32`: the symbol id, class `text` |
| 9 `ref` | 4 | `u32`: the `#N` |
| 10 `commitref` | 16 | `b16`: the `id16`, an index key only: the 32-byte value stays in the row's field block, which a reader reads and confirms a match against ([F08 §5.1]; pass 1, P1-1) |

  `text`, `path`, `oid` and `pathmove` have none and are never promoted, alone or as set elements ([F08] refuses
  `index ≠ none` for them).
- The default promotions of [50] F5 (`labels`, `assignee`, `work_kind`, `phase_state`, `severity`, `f_kind`, `round`,
  `local_id`, `outcome`, `metric`) are schema data ([F08]), not a list in this chapter.
- Slots are per segment. A reader resolves a field to a slot in each layer separately; a layer whose schema does not
  promote the field has no section for it and supplies no promoted value for its rows, which are then read from their field
  blocks.

### 10.2 `FCOL.<slot>`

A dense typed column with an absent bitmap ([50] F5), fold "row". One element per row of the segment (`n` = `n_rows` or
the length of `IDS`).

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 4 | `u32` | `field_sym` | as in `FPROMO` |
| 4 | 4 | `u32` | `n` | rows |
| 8 | 4 | `u32` | `n_vals` | set form: total elements; scalar form: 0 |
| 12 | 1 | `u8` | `vtype` | as in `FPROMO` |
| 13 | 1 | `u8` | `form` | as in `FPROMO`: 0 scalar, 1 set |
| 14 | 1 | `u8` | `elem_w` | the `elem_w` of `vtype` (§10.1): 1, 2, 4, 8 or 16 |
| 15 | 1 | `u8` | `_reserved` | reserved-zero |
| total | 16 | | | |

Then the **absent bitmap**: ⌈n/8⌉ bytes, then zero bytes up to a multiple of 8; bit i ([F01 §4.4]) is set iff row i has no
value for the field (absent rows included). Then:

- **scalar form**: `n × elem_w` bytes; element i is row i's value in its promoted encoding, or zero bytes when bit i is
  set;
- **set form**: `off: [u32; n + 1]` (`off[0] = 0`, non-decreasing, `off[n] = n_vals`), then `n_vals × elem_w` bytes;
  row i's elements are `[off[i], off[i + 1])`, unique and ascending in the value order of §10.4; a row whose bit is set has
  an empty range. Uniqueness is of the promoted encoding: two `commitref` elements of one row whose full ids share an
  `id16` are stored once, and the field block holds both (§10.1).

`count = n`.

### 10.3 `FIDX.<slot>`

Value → bitset, walkable in value order ([50] F5), fold "bitset" per value.

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 4 | `u32` | `field_sym` | as in `FPROMO` |
| 4 | 4 | `u32` | `n_values` | directory entries |
| 8 | 1 | `u8` | `vtype` | as in `FPROMO` |
| 9 | 1 | `u8` | `elem_w` | as in `FCOL` |
| 10 | 1 | `u8` | `form` | 0 frozen bitsets (base), 1 ± lists (upper segments) |
| 11 | 5 | `[5]u8` | `_reserved` | reserved-zero |
| total | 16 | | | |

Then `n_values` directory entries of 32 bytes, strictly ascending in the value order of §10.4:

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 16 | `[16]u8` | `value` | the value's `elem_w` bytes, then zero bytes |
| 16 | 4 | `u32` | `count` | frozen: the set's cardinality; ±: `n_plus + n_minus` |
| 20 | 4 | `u32` | `_reserved` | reserved-zero |
| 24 | 8 | `u64` | `off` | offset of the value's body from the section's first byte |
| total | 32 | | | |

Then the bodies in directory order, each at the first multiple of 8 at or after the previous item: a frozen bitset (§4.5)
or a ± list (§4.6) of the `#N`s holding that value (for a set-valued field, every row holding it as an element). Base:
exactly the values with a non-empty set. Upper: exactly the values whose set changed. `count` (in `SecEnt`) = `n_values`.

### 10.4 Value order

The stored order of [F08 §5.5], extended to the scalar types a set cannot hold: `int` and `counter` numerically (signed);
`enum`, `ref` and `sym` by the stored integer (for `sym` the symbol id, store-local); `commitref` bytewise; `bool` false
before true; `f64` numerically (a total order, since NaN is refused and −0.0 stored as +0.0, [F08 §5.3]). A query that
needs another order — `sort_rank` for enumerations ([50] F2) or string order for `sym` — re-sorts the value directory, which
holds one entry per distinct value (OP-09-27).

## 11. `STATS` ([50] F7)

Present in base segments only; a rollup writes it over its whole view ([AR §4.9]). Upper segments carry none: the
statistics are estimates for anchor choice ([50 §5.6]), and exact counts come from the bitsets (OP-09-10).

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 4 | `u32` | `n_edge` | edge-statistics rows |
| 4 | 4 | `u32` | `n_field` | field-statistics rows |
| total | 8 | | | |

Then `n_edge` rows of `EdgeStat`, sorted by `(edge_kind, dir)`, then `n_field` rows of `FieldStat`, sorted by
`(kind, field_sym)`. `count = n_edge + n_field`.

**`EdgeStat`** — one per (edge kind, direction) with at least one edge:

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 1 | `u8` | `edge_kind` | the edge id ([F08 §8.3]) |
| 1 | 1 | `u8` | `dir` | 0 out, 1 in |
| 2 | 2 | `u16` | `_reserved` | reserved-zero |
| 4 | 4 | `u32` | `max_degree` | the largest degree |
| 8 | 8 | `u64` | `edges` | Σ degree |
| 16 | 64 | `[16]u32` | `hist` | `hist[b]` for b = 0–14 counts rows with ⌊log2(degree)⌋ = b; `hist[15]` counts rows with degree ≥ 2^15; rows with degree 0 are not counted |
| total | 80 | | | |

The degree of a row for (kind k, direction d) is the number of edges of kind k in its out-list (d = 0) or in-list (d = 1),
with `parent` counted out of `NodeHdr.parent` (d = 0) and out of the in-list (d = 1); every non-absent row counts, deleted
rows with retained edges included.

**`FieldStat`** — one per (kind, field) with `present > 0`:

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 1 | `u8` | `kind` | the kind id ([F08 §8.3]) |
| 1 | 3 | `[3]u8` | `_reserved` | reserved-zero |
| 4 | 4 | `u32` | `field_sym` | symbol of class `name` |
| 8 | 4 | `u32` | `present` | non-absent rows of that kind whose field block holds the field |
| 12 | 4 | `u32` | `distinct` | the distinct estimate below |
| total | 16 | | | |

**Distinct estimate.** V is the set of `XXH3-64(e)` over the field's values e — each element for a set-valued field —
where e is [F08]'s encoding of the value with its type byte. If |V| ≤ 64, `distinct` = |V|. Otherwise, with s the 64th
smallest element of V, `distinct` = min(2^32 − 1, ⌊63 × 2^64 / (s + 1)⌋), computed exactly in integer arithmetic (the
k-minimum-values estimator with k = 64, the construction of [F20 §2.6.3]). The value is a deterministic function of the
view, so a rebuild reproduces it.

## 12. Full-text tier 2 ([AR §2.11] T11, [50] F12)

`TERMS`, `POST` and (if kept) `DOCLEN` and `FTSSTAT` exist in a segment iff its `tok_ver` is 1. When they are written is
[F17 §6.4]'s rule (`store.fts.tier2-nodes`, `HEAD.flags` bit 1); a segment set may mix segments with and without them.

### 12.1 Tokenizer v1

Search tokenisation is fixed in the format with a version byte, because postings depend on it ([50 §5.5], [50] F12). The
query side (`search()`, `term*`, `-term`) is [LQ/std]'s and uses this tokenizer for its terms.

- **Input.** The UTF-8 bytes of one text field of a document: its title, its abstract ([F08 §9]) or its body.
- **Unicode data.** Unicode 17.0.0, from the pinned UCD files of [PLAN §2.4] (the version of `fold_v1`, [F20 §3]):
  `General_Category` (UnicodeData.txt field 2) and the simple lowercase mapping (field 13).
- **Tokens.** A token is a maximal run of scalar values whose `General_Category` is a letter (`Lu`, `Ll`, `Lt`, `Lm`, `Lo`)
  or a number (`Nd`, `Nl`, `No`) ("Unicode letter/digit runs"). Every other scalar value separates tokens.
- **Mapping.** Each scalar value of a token is replaced by its simple lowercase mapping, if it has one; then U+0451 `ё` is
  replaced by U+0435 `е` (`Ё` reaches `ё` by lowercasing first). No normalisation and no stemming is applied.
- **Length.** The term is the UTF-8 encoding of the mapped token. A term longer than 64 bytes is cut to its longest prefix
  of at most 64 bytes that ends on a scalar-value boundary.
- **Counts.** A field's term frequency `tf(t)` is the number of its tokens whose term is t; its length is its token count.

The decisions in this list that [50 §5.5] leaves open (the Unicode version, simple lowercase, combining marks as
separators, the 64-byte cut) are recorded as OP-09-11.

### 12.2 `TERMS`

A front-coded term dictionary ([AR §2.11]): the distinct terms of this segment's postings, strictly ascending bytewise.

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 4 | `u32` | `n_terms` | distinct terms |
| 4 | 4 | `u32` | `n_blocks` | ⌈n_terms / 16⌉ |
| total | 8 | | | |

Then `block_off: [u32; n_blocks]`, the offset of each block from the section's first byte, then the blocks, contiguous.
Block k holds terms 16k to min(16k + 15, n_terms − 1) (sequence table):

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `len` | `u8` | first term of the block | the term's length, 1–64 |
| 2 | `shared` | `u8` | every later term | the length of the longest common prefix with the previous term |
| 3 | `suffix_len` | `u8` | every later term | 1 to 64 − `shared` |
| 4 | `bytes` | `len` or `suffix_len` bytes | always | the term, or its bytes after the shared prefix |
| 5 | `post_off` | `uvar64` | always | the offset of the term's posting list from the first byte of `POST` |

Fields 1–5 repeat per term, with field 1 only for the first term of a block and fields 2–3 only for the others. `shared` is
maximal, so the encoding is unique. `count = n_terms`.

### 12.3 `POST`

The posting lists in term order, contiguous from offset 0. One list:

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `n_docs` | `uvar32` | always | the documents of this segment that contain the term (its df in this layer), ≥ 1 |
| 2 | `ddoc` | `uvar32` | per posting | the first posting's `#N`, then the difference to the previous posting's `#N` (≥ 1) |
| 3 | `fmask` | `u8` | per posting | bit 0 title, bit 1 abstract, bit 2 body: the fields that contain the term; at least one set; bits 3–7 reserved-zero |
| 4 | `tf` | `uvar16` | per set bit of `fmask`, in bit order | the term frequency in that field, ≥ 1 |

Fields 2–4 repeat `n_docs` times, in ascending `#N`. A document is a non-absent, non-deleted row of the segment; the
postings of an upper segment cover its touched rows only, and fold "index" applies per posting (§4.7). `count = n_terms`.

### 12.4 `DOCLEN` and `FTSSTAT`

Both exist only if `HOLE(F09-doclen)` keeps them; otherwise neither tag is written, both stay reserved, and `TERMS` and
`POST` are unchanged ([AR §2.11], [74 A15], LQ-Bench's BM25 ablation).

- **`DOCLEN`**: column of `[3]u16` per row, fold "row": the token counts of title, abstract and body, each saturating at
  65,535 ([50] F12, "u16 tokens for title, abstract, body"; [F08 §4] lists the column); zero for absent and deleted rows.
- **`FTSSTAT`**: exactly 48 bytes, no header, fold "full (view)", in base, delta and branch segments that carry `TERMS`
  (never in a changeset segment); `count` = 3. Three records, for title, abstract and body in that order:

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 4 | `u32` | `docs` | non-deleted documents of the view, as of this layer, with a non-empty field |
| 4 | 4 | `u32` | `_reserved` | reserved-zero |
| 8 | 8 | `u64` | `total_len` | Σ of the field's token count over the view's non-deleted documents, as of this layer |
| total | 16 | | | |

A reader obtains the view's statistics from the newest layer's `FTSSTAT` and adjusts them for changeset layers and the
tail through `DOCLEN` and re-tokenisation ([50 §5.5] "Ranking statistics are defined on the view"). The arithmetic of BM25
itself is [LQ/std]'s (review S-22, OP-09-12).

## 13. R4 link sections, versioned ([40] R-8, [50] F13)

All five are fold "index": each entry belongs to one row and is live only in that row's newest layer.

### 13.1 `PATHIDX`

Variable table: one row per present or planned file node ([40] R-8: "sorted (root, fold(path), path) → `#N`"):

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `root` | `u16` | always | the path's root, symbol class `root` ([40] R-1, [F01 §8.2]) |
| 2 | `id` | `u32` | always | the file node's `#N` |
| 3 | `fold` | `vbytes` | always | `fold_v1(path)` ([F20 §3.1]), UTF-8 |
| 4 | `path` | `vbytes` | always | the stored path, exact bytes ([40 §2.4] I-F8) |

Key `(root, fold, path, id)`, `root` numerically, the byte strings bytewise. Case and normalization variants are adjacent,
so a collision probe is one index step ([40 §2.4]). Two live rows with one `(root, path)` exist only under a `PathClaim`
conflict value (I-F1).

### 13.2 `ALIASIDX`

Variable table with the row format of `PATHIDX`: one row per element of the `aliases` set of every non-deleted file node
([40 §2.2]), with `path` the alias. Same key.

### 13.3 `ANCHORS`

Variable table ([40] R-8: "sorted (src#, dst#, anchor#) → record"):

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `src` | `u32` | always | the referrer's `#N` |
| 2 | `dst` | `u32` | always | the file node's `#N` |
| 3 | `anchor` | `u32` | always | the anchor's store-local handle `aN` from `HEAD.next_anchor` ([40] R-6) |
| 4 | `rec` | `vbytes` | always | the anchor record ([40] R-4, [40 §2.7]) in [F08 §10.3]'s encoding, `captured` included |

Key `(src, dst, anchor)`; the record does not repeat it ([F08 §10.3]). The rows of `src` are the anchors of its `at` edges;
the entry belongs to row `src`. Every `at` edge of the CSR has at least one row here (I-F3).

### 13.4 `ANCHOR_UID`

Fixed table, `row_w` = 28, key `(uid, src, dst, anchor)`:

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 16 | `b16` | `uid` | the anchor uid, the edge-key discriminator ([40 §2.7], [40] R-4) |
| 16 | 4 | `u32` | `src` | as in `ANCHORS` |
| 20 | 4 | `u32` | `dst` | as in `ANCHORS` |
| 24 | 4 | `u32` | `anchor` | as in `ANCHORS` |
| total | 28 | | | |

One entry per `ANCHORS` row; the entry belongs to row `src`.

### 13.5 `GLOBIDX`

Path globs by literal prefix ([AR §4.4], [70 S17]). Its body, rows, key (literal prefix, `n`, `field`, glob) and the
literal-prefix rule are [F11 §11]'s. This chapter fixes its tag (`0x0084`), its presence in every graph segment kind, and its
fold "index": a row belongs to the node `n`. Pack class C7 is then a range probe ([AR §4.8]).

## 14. Store-level sections

### 14.1 Symbol class codes

`SYMTAB` stores each symbol with its class ([F01 §8.1] S1). The class codes:

| value | name | width W ([F01 §8.2]) |
|---|---|---|
| 1 | `actor` | `u32` |
| 2 | `role` | `u16` |
| 3 | `session` | `u32` |
| 4 | `ref` | `u32` |
| 5 | `git-branch` | `u32` |
| 6 | `git-worktree` | `u32` |
| 7 | `stmt` | `u32` |
| 8 | `root` | `u16` |
| 9 | `reason` | `u32` |
| 10 | `name` | `u32` |
| 11 | `text` | `u32` |

Values 0 and 12–255 are reserved. A class added before the freeze ([F01 §8.2]) takes the next value. The same codes are
used wherever a record names a class ([F05]'s new-symbol records).

### 14.2 `SYMTAB`

The store-wide symbol table ([AR §4.4] "sorted string table", [F01 §8]), fold "symbols", probed in place and never loaded
into a per-process map ([AR §4.7], [71 RAM-m6]).

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 2 | `u16` | `n_classes` | class entries |
| 2 | 2 | `u16` | `_reserved` | reserved-zero |
| 4 | 4 | `u32` | `pool_len` | length of the string pool |
| total | 8 | | | |

Then `n_classes` class entries of 20 bytes, sorted by `class` strictly ascending:

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 1 | `u8` | `class` | §14.1 |
| 1 | 3 | `[3]u8` | `_reserved` | reserved-zero |
| 4 | 4 | `u32` | `first_id` | the first symbol id this layer holds for the class, ≥ 1 |
| 8 | 4 | `u32` | `n` | ids `first_id … first_id + n − 1`, n ≥ 1 |
| 12 | 4 | `u32` | `by_id_off` | offset from the section's first byte of `[u32; n]`: element j is the pool offset of symbol `first_id + j` |
| 16 | 4 | `u32` | `by_str_off` | offset from the section's first byte of `[u32; n]`: the class's ids of this layer sorted by string, bytewise |
| total | 20 | | | |

Then, per class in entry order, its `by_id` array followed by its `by_str` array; then the pool of `pool_len` bytes: one
`vstr` per symbol, in (class, id) order. Pool offsets are relative to the pool's first byte.

- **Ranges.** For each class, the layers of a main set hold contiguous, disjoint id ranges in layer order: the base
  `[1, a)`, the next delta `[a, b)`, and so on; the tail's new symbols continue the sequence ([F05]). Id 0, the empty
  string, is never stored ([F01 §8.1] S2). A layer holds exactly the symbols allocated in its window.
- **Uniqueness.** A string appears at most once per class across the table and the tail.
- **Lookups.** By (class, id): the layer whose range contains id, then `by_id`. By (class, string): a binary search of each
  layer's `by_str`, newest first, then the tail.
- `count` = Σ n.

### 14.3 `SCHEMAIDS`

The store-wide map of store-local schema ids ([F08 §8.3]: "checkpoints fold the map into a section … the proposed name is
`SCHEMAIDS`"): for every project kind, project edge kind and store-local enumeration value the store has ever allocated, on
any branch, its id. Core items have the fixed ids of [F08 §9] and no row. The body is [F11 §2.1]'s; form snapshot in a base,
layer in a delta ([F11 §2.4]; append-only: ids are never reused, retired items keep theirs, so no row is ever dead).

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 1 | `u8` | `space` | 1 kind, 2 edge kind, 3 enumeration value |
| 1 | 1 | `u8` | `kind` | space 3: the kind id of the enumeration's (kind, field); 0 otherwise |
| 2 | 2 | `u16` | `id` | the store-local id: 64–254 for spaces 1–2, 0–65,535 for space 3 ([F08 §8.3]) |
| 4 | 4 | `u32` | `name` | spaces 1–2: the item's name; space 3: the field's name; symbol class `name` |
| 8 | 4 | `u32` | `value` | space 3: the enumeration value's name, class `name`; 0 otherwise |
| 12 | 1 | `u8` | `flags` | reserved-zero |
| 13 | 3 | `[3]u8` | `_reserved` | reserved-zero |
| total | 16 | | | |

Key `(space, kind, name, value)`; unique; for each space and (for space 3) each (kind, field), `id` is unique too. No heap.
A reverse lookup (id → name) scans the few rows of its space. Rebuilt from the log by `repair --rebuild-from-log`.

### 14.4 `FILES`

The registry of every live sealed file that `HEAD` does not name ([F04 §4.1] "every other sealed file … is named by log
records … and the segment sections that fold them ([F09], [F10], [F11])"; [F04] open point 4): `hist`, `blobs`, `gitmap`,
`cs`, `seg-branch` and `dict` files. `HEAD.segments` names the current base, deltas and dictionary; `PINS` names the files
of pinned sets ([F11 §4]); `FILES` names the rest and every live dictionary (a `blobs` file of a pinned set may name an
older one, [F10 §6.3]), so the three together name every live sealed file. The body is [F11 §2.1]'s, form
snapshot in every base and delta: the complete list as of the segment's `upto_lsn`.

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 9 | `FileRef` | `file` | [F11 §2.5]: family 2 `hist`, 5 `seg-branch`, 6 `blobs`, 7 `dict`, 8 `gitmap` or 9 `cs`, its `ref_id` and number |
| 9 | 1 | `u8` | `dest` | `gitmap`: the page's destination ([F10 §7.2]); 0 otherwise |
| 10 | 1 | `u8` | `algo` | `gitmap`: the page's object format ([F01 §7.5]); 0 otherwise |
| 11 | 1 | `u8` | `flags` | bit 0 `reserved`: a `cs` or `blobs` number claimed by a `Reserve` record ([F05 §9.27]) whose bulk `Commit` the segment does not fold; `upto_lsn`, `total_len` and `digest16` are then zero (the file may not exist yet). Bits 1–7 reserved-zero |
| 12 | 4 | `u32` | `_reserved` | reserved-zero |
| 16 | 8 | `u64` | `from_lsn` | `hist`: its `SegHdr.from_lsn`; 0 otherwise |
| 24 | 8 | `u64` | `upto_lsn` | `hist` and `seg-branch`: its `SegHdr.upto_lsn`; `cs`: the lsn of the `Commit` record that names it; 0 otherwise |
| 32 | 8 | `u64` | `total_len` | the file's length |
| 40 | 16 | `b16` | `digest16` | `SegHdr` files: `seg_digest[0..16]`; `gitmap` and `dict`: its header's `digest` |
| total | 56 | | | |

Key `file` (`family`, `ref_id`, `file_no`). No heap. A file enters the list with the record that makes it live (a
`Checkpoint`, a retirement, a promotion, a bulk `Commit`, a `gitmap` fold, [F05]) and leaves it when GC deletes it
([F16]). Readers use it to find the `hist` file of an lsn, the `blobs` files that may hold a blob by content address
([F10 §5.5]) and the `gitmap` pages of a (destination, algorithm) pair; GC and `doctor --fsck` use it to enumerate the
store's files (OP-09-25).

**Reserved numbers** (pass 1, P1-3). The `cs.<n>` and `blobs.<n>` numbers of a `Reserve` record ([F05 §9.27]) enter with
`flags` bit 0 set when a fold covers the record but not the bulk `Commit` that names them, so they stay named ([F16] P-77
condition 2) while the file streams; the fold that covers that `Commit` writes their real `upto_lsn`, `total_len` and
`digest16` and clears the bit, and a `gc` `Checkpoint` that releases the reservation's files ([F16] P-84) removes the
rows. A `reserved` row is no part of any view: readers, a backup's copy ([F16] P-87) and `doctor --fsck`'s file checks
skip it; only the deletion conditions of [F16] P-77 read it. The file itself becomes part of a view through the bulk
`Commit` that names it, once that record lies in the view's log range: P-87 copies it exactly then, whatever the row's
bit says.

### 14.5 Runtime tables whose sections [F11] owns

`REFS`, `PINS`, `HEADS`, `LEASES`, `MARKERS`, `MARKERS_OLD`, `IDEM`, `ALLOC`, `UIDX`, `CURSORS`, `SESSMARKS`, `BACKUPS`
(tags `0x0201`–`0x020C`): bodies, rows, keys, forms, retention and lookup are [F11 §2–§9] and [F11 §13]'s. This chapter
fixes their tags (§3.1, adopting [F11 §2.8]),
their placement in every `seg-base` and `seg-delta` and nowhere else, and that they fold against `upto_lsn`. The marker key
`(#N, ref_id, commit)` ([AR §4.4], [60 §2.5]) and the field set are [F11 §7]'s (the [PLAN §3.3] gap "the `MARKERS` field
set", closed there).

## 15. R4 runtime sections ([40] R-8, R-18; [80] X-F8)

### 15.1 The runtime window

The lazy runtime record kinds `K_RT` = {`FileObs`, `Pending`, `FPrint`, `JournalCursor`, `DirMap`, `TreeReg`, `PrefixEv`,
`GitFacts`, `AnchorRes`} ([F17 §5.1], [40] R-7) fold into `TREES`, `FILEOBS`, `PENDING`, `FPRINT`, `JOURNALCUR`, `DIRMAP`,
`PREFIXEV`, `ANCESTRY`, `GITRENAMES` and `ANCHORRES` against `rt_upto_lsn`; every other record folds against `upto_lsn`.

- A **runtime-only fold** ([AR §4.5] step 12, [F17 §5.4]) writes a delta whose `from_lsn = upto_lsn` = the `upto_lsn` of the
  layer below (an empty graph window: `IDS` empty, every row-scoped section empty, no ± list) and whose `rt_upto_lsn`
  advances. It carries every section a delta requires, the snapshot sections unchanged apart from the runtime-window ones
  ([F11] open point 27: "every snapshot section … and no graph section" with content).
- A regular delta checkpoint sets `rt_upto_lsn = upto_lsn`.
- A reader applies a tail record of a `K_RT` kind only if its `lsn ≥` the newest layer's `rt_upto_lsn`, and any other tail
  record only if its `lsn ≥` that layer's `upto_lsn` ([F11 §2.4] "the bound is [F09]'s").
- `FSINTENT` is not in the runtime window: its records are durable ([40] R-7) and fold against `upto_lsn`.

### 15.2 Runtime tables whose sections [F11] owns

`TREES`, `FILEOBS` (R-18), `PENDING`, `FSINTENT`, `FPRINT`, `JOURNALCUR`, `DIRMAP`, `PREFIXEV`, `ANCESTRY` and `GITRENAMES`
(together [40 §2.6]'s `GITFACTS`) and `ANCHORRES` (tags `0x0210`–`0x021A`): bodies, rows (the tagged `OsFileId`, `FsTime`,
`VolumeCaps` of X-F8 included), keys, forms, retention (review A1P-16) and lookup are [F11 §2.4, §12]'s. This chapter fixes
their tags, their placement in every `seg-base` and `seg-delta` (R, or O for the five `derived-optional` sections of §3.3)
and nowhere else, and their fold window (§15.1).

**`FPRINT` and R-9.** `FPRINT`'s row maps an `oid` to the content address of a fingerprint blob and the number of the
`blobs` file that holds it ([F11 §12.8]; OP-09-14 adopted, pass 1, P1-22); the blob itself is [F10 §5.3]'s fingerprint
class in that file, found by one binary search of its `BLOBIDX` ([F10 §5.5]).

## 16. Segment kinds

### 16.1 Base (`seg.base.<G>`)

The materialised state of `main` at `upto_lsn` ([AR §4.1]), and the store-level tables at `upto_lsn` (`rt_upto_lsn` for the
runtime window). Rows are dense from `#1` to `n_rows`; absent rows are zero (§5.2). Bitsets and `FIDX` are frozen. Written
only by a rollup in a `moirai gc` process, or by `repair --rebuild-from-log` ([AR §4.9], [AR §4.10]), streaming: columns and
CSR merged row by row, bitsets rebuilt per 65,536-id chunk, `TOPO` renumbered, `STATS` computed in the same pass. Whether
`init` writes an empty base (`n_rows` = 0) is [F16]'s decision with the initial file set ([F02 §2.4]).

### 16.2 Delta (`seg.d<K>`)

The change of `main` over `[from_lsn, upto_lsn)` (and of the runtime-window tables up to `rt_upto_lsn`): touched rows with
complete states, ± lists, new symbols, and the store-level tables in their forms ([AR §4.4] "Delta segments hold only
touched rows…"). Written by a delta checkpoint or a runtime-only fold under the maintenance byte ([AR §4.5] step 12, [F16]).

**Tiered fold** ([AR §4.9], [F17 §6.1]). Folding deltas d1…dk into one new delta d′ (always a new file number):

- `d′.from_lsn = d1.from_lsn`, `d′.upto_lsn = dk.upto_lsn`, `d′.rt_upto_lsn = dk.rt_upto_lsn`, `d′.base_seq = dk.base_seq`;
- `IDS(d′)` = the union of the `IDS`; each row's state is its state in the newest of d1…dk that has it;
- each ± list of d′ is the composition of the inputs' lists over the base's membership, dropped if empty;
- snapshot sections from dk; layer sections merged as [F11 §2.4] "Folds" states; `SYMTAB` ranges concatenated per class;
- the result is the canonical encoding of that content (§17.3), so it equals what one delta checkpoint over the same window
  would have written.

### 16.3 Promoted branch (`seg.b<ref_id>.<K>`) and `TOUCH`

A promotion of ref X ([AR §5a.3], [F17 §7]) writes one branch segment and moves X's `base_pin` to the newest sealed main
set P ([F11 §3.6]). The segment holds every row whose state in X's view at X's tip differs from its state in P — X's own
work, the synced windows, and the rows P changed after X's fork that X has not absorbed, overridden back (an absent row
where X has no node) — with ± lists relative to P and the versioned index entries of those rows. It carries no store-level
section.

- Header: `ref_id` = X's id; `file_no` = K, the new `REFS.promoted_seg` ([F11 §3.1]); `from_lsn` = the `upto_lsn` of P's
  newest layer; `upto_lsn` = the end of X's tip commit record; `base_seq` = that commit's `seq`.
- **`TOUCH`** (required): a frozen bitset (§4.5) whose members are exactly the `IDS` of the segment — the rows the branch
  overrides ([D §4.4], [AR §4.1]). `--across` over refs and the all-refs form use it by bitset AND ([AR §2.17] D11,
  [60 §1.3]).
- One branch segment per promoted ref is live: a later promotion writes a new K against the new pin, and the old one leaves
  `FILES` when GC deletes it ([F16]).

### 16.4 Changeset (`cs.<n>`)

The sealed changeset of one bulk commit ([AR §4.3], [71 RAM-B1], [F06 §9]): its sections are those of a delta segment, with
the row states **after** the commit for every row it touches and ± lists relative to the state it applies to, plus `PREV`
and, when needed, `VIOLATIONS`. It carries no store-level section. [F10 §8] gives its sealed-file lifecycle. It holds every
effect [F06 §9] BK-5 requires and no before-image (BK-5: a revert, cherry-pick, `blame` or `show` of a bulk commit derives
them from its base state):

| effect ([F06 §9] BK-5) | where |
|---|---|
| created nodes with uid, kind and creator | `IDS`, `NODE`, `UID`, `CREATOR` |
| deleted nodes with reason and replacement; undeleted nodes | `NODE` (`deleted`), `TOMB`; an undelete is a touched row no longer deleted |
| nodes that go from absent to deleted ([F06 §7.4] `CreateDeleted`) with kind, reason, replacement and creator | a touched row that is deleted here and absent in the state the segment applies to: `NODE` (`deleted`, `kind`, the retained title), `UID`, `CREATOR`, `TOMB`, and its retained out-edges in the CSR and `EDGE_PROPS` (pass 1, S1-6) |
| per touched row, its `prev` | `PREV` (below) |
| conflict values; resolutions | `CONFLICTS`; a resolution is the touched row without the conflict and with the chosen value |
| violations (a bulk merge that stages) | `VIOLATIONS` (below) |
| schema items | `SCHEMA` |
| anchors with their `aN` and edge props | `ANCHORS`, `ANCHOR_UID`, `EDGE_PROPS` |
| bodies | `BLOBTAB`, pointing into the `blobs` file sealed before it ([F06 §9] BK-3) |
| a bulk import-checkpoint's image-only data ([F06 §4.4.14]) | `CKIMG` (below; pass 1, S1-23, A1-10) |

- **`PREV`** (tag `0x00C0`, required): column of `u64`, parallel to `IDS`: the **absolute** lsn of the newest earlier
  `Commit` record with the same `ref_id` whose changeset touches the row, 0 when there is none — the lsn that [F06 §7.3]'s
  `prev` would subtract from the commit's own. The commit's own lsn is unknown when the file is sealed, so the delta cannot
  be written; a walker computes it from the bulk commit's lsn (OP-09-15). The sealed values stay right because [F16] P-34
  re-validates a bulk commit **by node**: every node owning a row of the segment counts as read and written, and an
  intervening commit that touches one forces phase 1 to re-run and the file to be re-streamed ([F06 §7.3]; pass 1,
  S1-20).
- **`VIOLATIONS`** (tag `0x00C1`): variable table whose rows are the commit's `Violation` ops, each exactly as [F06] encodes
  it in an inline op list, in [F06]'s op order (not re-sorted; the variable-table order rule of §4.3 is replaced by it).
  Present iff the commit carries at least one; `Violation` ops exist only on staging refs ([AR §4.6]).
- **`CKIMG`** (tag `0x00C2`): variable table whose rows are the entries of [F06 §4.4.14]'s `ckimg` group, each exactly
  as [F06] encodes one entry (`id`, `iflags`, the provenance values, the ledger lines), sorted by `id` strictly ascending
  (the variable-table order of §4.3 with `id` as the key). Present iff the bulk commit is an import-checkpoint whose
  checkpoint tree holds at least one node file that differs from its parent checkpoint's tree, the condition under which
  an inline commit sets presence bit 17; the bulk commit's record never sets that bit ([F06 §4.2]). Its rows obey
  [F06 §4.4.14]'s V- and C-rules; `CKIMG` is not row-scoped (§4.7): it belongs to the commit, not to a layer's view, and
  a checkpoint that folds the segment's rows does not fold it. A re-export reads it from the `cs.<n>` that the kept
  record names ([F10 §8]; [F14 §11.3]).
- **Values fixed at append.** The commit's `seq` and `lsn` are allocated when its `Commit` record is appended, after the file
  is sealed ([AR §4.5] steps 8–9). In a changeset segment's `NODE` rows, therefore: `rev_seq` = 0, `updated_tx` = 0,
  `created_tx` = 0 for a row the commit creates, and `last_op_lsn` = 2^64 − 1. A reader substitutes the bulk commit's `seq`
  and `lsn` for these values; a row that existed before keeps its real `created_tx` (`seq` starts at 1, so 0 is never a real
  value; OP-09-16).
- **Ids.** Every `#N`, `aN`, symbol id and store-local schema id in the file is final. The bulk producer must hold them
  before it writes the file ([AR §4.5] step 8 allocates `#N`s under the writer byte; [F06 §7.3] fills the placeholders of an
  inline commit there). It obtains them from its durable reservation group: one `Reserve` record (kind 27, [F05 §9.27])
  appended under the writer byte before the file is streamed, which allocates the `#N`s, `aN`s, new symbols (its `SymDefs`
  block), schema ids and the file numbers of the `cs.<n>` and its `blobs.<n>` ([F16] P-84). Ids of a reservation whose
  commit never lands are skipped, never reused ([F11 §9.1]; OP-09-17, closed in pass 1).
- `base_seq`, `from_lsn`, `upto_lsn` and `rt_upto_lsn` are 0 (§2.3). `FTSSTAT` is never written (§12.4).
- The commit's `cs_ref` names the file by number, length and digest: `cs_ref.b3` = `seg_digest[0..16]`, the value
  `FILES.digest16` holds ([F06 §9] BK-2; OP-09-03, closed in pass 1).

## 17. Validation, errors and canonical encoding

### 17.1 What makes a segment invalid

A segment is invalid when any of these fails. The **open checks** (V-1–V-8) run before a process maps a segment and before
it uses the section table; the **full checks** (V-9–V-13) run in `doctor --fsck`, `repair`, the format oracle ([PLAN §3.2]
WP-95), and in maintenance for every section it reads in full as input to a fold or rollup, so a corrupt section is never
copied into a new segment. [F11 §2.7] adds the checks of the `RtHdr` bodies.

| # | check |
|---|---|
| V-1 | `magic`; `format` = 1 (a higher value refuses the store with exit 7 naming both versions; 0 is invalid, [F01 §9.1]) |
| V-2 | `hdr_xxh3` |
| V-3 | `seg_kind` matches the file's name family, `file_no` (and `ref_id` for `seg-branch`) match the name ([F02 §6]) |
| V-4 | `total_len` equals the file size ([OS/map §4]) |
| V-5 | every reserved field and bit of the header is zero; §2.3's zero fields are zero; `tok_ver` ∈ {0, 1} (0 for `hist` and `blobs`) |
| V-6 | `table_xxh3` |
| V-7 | P-1–P-4 of §2.2; every tag is registered for this kind with its §3.1 placement, or is unknown and `derived-optional`; every required section present; `flags` as §3.3 |
| V-8 | the header fields against the reference that names the file: `HEAD`'s `SegRef` (`upto_lsn`, `blake3_16`, [F04 §4.1]), a `FILES` row (§14.4), a commit's `cs_ref` ([F06 §9]), a ref's `promoted_seg` ([F11 §3.1]) |
| V-9 | each section's `xxh3`; `seg_digest` |
| V-10 | each section's container rules (§4, [F11 §2]) and its own rules: sort orders, uniqueness, offset arrays, reserved bytes, canonical forms (bitset containers, pool order) |
| V-11 | cross-section rules: column lengths equal `n_rows` or the `IDS` length; `IDS` covers every row-scoped and index entry; `NodeHdr` offsets fall inside their pools or are `NONE32`; `body_ref` within `BLOBTAB`; `EDGE_PROPS.edge` < e_out, its `pflags` admitted by the edge's kind ([F08 §10.2]) and `pinned_commit` zero without `has_pin`; `BMDIR` and `FPROMO` match the tags present |
| V-12 | view-level rules over a whole stack: I-P3 of §7.1, `TOUCH` = `IDS`, main-set continuity (§2.3), ± list preconditions (§4.6) |
| V-13 | derived content equals a recomputation (`doctor --verify`, I9) |

### 17.2 Consequences

- A segment that `HEAD` names and that fails an open check makes the process exit 7, naming the file and `moirai doctor
  --fsck` (texts in [F19]); after a size mismatch the process first re-reads `HEAD` and retries once ([OS/map §4]). A
  `SegRef` mismatch is handled like a missing file ([F04 §4.1]): recovery rebuilds the segment set from durable
  `Checkpoint` records ([AR §4.2], [F16]).
- A failed full check is reported by `doctor --fsck` with the file and section; `repair --rebuild-from-log` rebuilds every
  segment from the log, since all are derived ([AR §4.10]).
- A read through a mapping that faults ends the process with exit 7 ([80 §2.5] rule 6, [F15] FM-9).
- Runtime readers do not test reserved bytes inside mapped section rows on every read; the oracle and `doctor --fsck` test
  every one ([F01 §10] rule 3, [F01] open point 6).

### 17.3 Canonical encoding

Every rule of this chapter that orders, places or chooses an encoding does so uniquely: section order and placement (P-1–P-4),
row orders, pool orders, `BLOBTAB` order, the frozen-bitset container choice, slot assignment, front coding with maximal
`shared`, varints in their shortest form ([F01 §5.2]), zero padding; [F11 §2.3] does the same for the `RtHdr` bodies. The
bytes of a segment are therefore a function of its kind, its header fields and its content. This is what lets the
section-producer registry's byte-identical rebuild ([60 §2.6]) and the format oracle's byte-identical re-encode (E3) hold.

## Coverage

| Item | Part covered here | Section |
|---|---|---|
| [60 §2.5] [AR] row "Segments": `SegHdr` with `total_len` and every section of [AR §4.4] incl. `REFS`/`PINS`/`HEADS`/`MARKERS`, `EDGE_PROPS`, `TERMS`/`POST`; the promoted-branch segment and `TOUCH` | complete for the segment header, the tag registry and placement of every section, and every versioned section this chapter owns; the runtime bodies and rows (and `CONFLICTS`, `GLOBIDX`) are [F11]'s; `hist`, `blobs`, `gitmap` are [F10]'s | §2–§16 |
| [60 §2.5] audit row "Segments" | `seg_kind` `cs`; the `derived-optional` flag; the tags and placement of `MARKERS`/`MARKERS_OLD`, `LEASES`, `ALLOC`/`UIDX`, `ANCHORRES`, `GLOBIDX`, `TREES` (their rows, the marker key, the epoch list and dirty row are [F11]'s); the `hist` bounds are [F10]'s and [F17]'s | §2.1, §3, §14.5, §15.2 |
| [60 §2.5] audit row "Commit body": the `cs.NNNN` changeset segment kind | the segment's sections for [F06 §9] BK-5, `PREV`, `VIOLATIONS`, `CKIMG`, the absent-to-deleted row, values fixed at append; the header fields are [F06]'s | §16.4 |
| [60 §2.5] issue-2 row "Store parameters" | the formats the FTS tier-2 threshold and the fold width act on; the values are [F17]'s | §12, §16.2 |
| [60 §2.5] "Schema as data" row | where a view's schema items live (`SCHEMA`) and the store-local id map (`SCHEMAIDS`); the items are [F08]'s | §8.3, §14.3 |
| [50] F3 | where `QUERIES` items live in a segment (`SCHEMA`); the item is [F08]'s | §8.3 |
| [50] F4 | complete: `CREATOR` column | §5.3 |
| [50] F5 | complete: `FCOL`/`FIDX` tag ranges, `FPROMO`, both layouts, value order | §3.2, §10 |
| [50] F6 | complete: per-chunk `card` and per-bitset totals in `SecEnt.count` | §4.5, §9 |
| [50] F7 | complete: `STATS` | §11 |
| [50] F11 | the `CONFLICTS` tag, placement and fold; the rows are [F11 §10]'s | §8.2 |
| [50] F12 | complete except the kept-or-dropped decision (`HOLE(F09-doclen)`): `TERMS`, `POST`, `DOCLEN`, `FTSSTAT`, the tokenizer version byte and tokenizer v1 | §2.1, §12 |
| [50] F13 | complete: `PATHIDX`, `ALIASIDX`, `ANCHORS` (the `at` discriminator is [F08]'s R-4 part) | §13.1–§13.3 |
| [50] F17 | the `ALLOC` tag and placement; the section is [F11 §9.1]'s | §14.5 |
| [40] R-4 | the CSR's one entry per (src, kind, dst), anchors in `ANCHORS` keyed by the handle and by uid; the record and op are [F08]'s and [F06]'s | §7.1, §13.3, §13.4 |
| [40] R-8 | complete for `PATHIDX`, `ALIASIDX`, `ANCHORS`, `ANCHOR_UID`; tags, placement, fold windows and `derived-optional` for `FILEOBS`, `PENDING`, `FSINTENT`, `FPRINT`, `JOURNALCUR`, `DIRMAP`, `TREES`, `PREFIXEV`, `GITRENAMES`, `ANCHORRES`, `GLOBIDX` (sections [F11]) | §3, §13, §15 |
| [40] R-9 | how `FPRINT` reaches the fingerprint blob (through `FILES`); the row is [F11 §12.8]'s, the blob class [F10]'s | §14.4, §15.2 |
| [40] R-18 | the `FILEOBS` tag, placement and fold window; the row is [F11 §12.5]'s | §15.2 |
| [80] X-F6 | `total_len` in `SegHdr` and the checks before mapping; the other sealed files are [F10]'s; the mapping policy is [OS/map]'s | §2.1, §17 |
| [80] X-F8 | the `JOURNALCUR`, `DIRMAP` (`derived-optional`) and `TREES` tags and placement; the layouts are [F11 §12]'s | §3.3, §15.2 |
| [90 §10.1] `LEASES` runtime rows | the `LEASES` tag and placement; the section is [F11 §6]'s | §14.5 |
| [90 §10.1] codec | none here: [F10] | — |

## Holes

| id | what | decided by | candidates | constraint the value must meet |
|---|---|---|---|---|
| `F09-doclen` | whether the sections `DOCLEN` (`0x0052`) and `FTSSTAT` (`0x0053`) exist in format v1 | LQ-Bench's search-stratum ablation, BM25 against the statistics-free scorer (WP-72, GT13), filled by WP-81a ([60 §3.1] "BM25 or the statistics-free scorer", [74 A15], [AR §2.11]) | **kept**: both sections exist with the layouts of §12.4 in every segment whose `tok_ver` is 1 (`FTSSTAT` not in changeset segments); **dropped**: neither tag is written, both stay reserved (a v1 reader treats them as unknown tags without `derived-optional`, so invalid), and `TERMS`, `POST`, the tokenizer and every other section are unchanged | kept only if BM25 beats the statistics-free scorer beyond the benchmark's noise; if kept, [LQ/std] fixes BM25's arithmetic before the freeze (review S-22) |

The following decisions of other work packages condition this chapter without being holes of it:

- **T1's structure** ([60 §3.1], measurement 14, WP-53e): this chapter specifies T1's Option A, which [AR §2.1] decides.
  If measurement 14 selects Option B before the freeze, this chapter is replaced, not filled (OP-09-01).
- **Production thresholds** ([F17]): when checkpoints, folds, promotions and tier 2 happen. No layout here depends on their
  values (SP-R2 of [F17 §1.4]).
- **The codec** ([F10]'s holes): graph segments hold no compressed bytes; `BLOBTAB` points into `blobs` files.

## Open points for the review

- **OP-09-01 (T1 Option A).** The chapter is the byte-level form of [AR §2.1]'s Option A (sealed columnar segments plus a
  tail overlay). Measurement 14 "confirms the §2.5 layouts" and decides T1 before the freeze ([60 §3.1], [60 §5.2] row 14).
  Option B (copy-on-write B+tree pages) has no layout in the design, so it cannot be a hole with complete candidates. If it
  were chosen, this chapter would be rewritten before the freeze. The review should confirm that this is recorded in WP-53e's
  decision draft.
- **OP-09-02 (`SegHdr` field order and additions; gap of [PLAN §3.3] "explicit offsets").** [AR §4.4] lists the header's
  fields without offsets: `magic`, `format`, `seg_kind`, `n_rows`, `base_seq`, `upto_lsn`, `total_len`, `n_sections`, the
  table, `blake3`. This chapter orders them for natural alignment and adds, each as a closed gap:
  - `tok_ver` — [50] F12 places "a tokenizer version byte in the segment header" ([50] owns its reservations);
  - `file_no`, `ref_id` — self-identification against the file name, so a renamed or copied file is detected (V-3);
  - `dict_no` — a `blobs` file must say which dictionary its payloads need ([F10 §3.5]);
  - `from_lsn` — a delta and a branch segment must say which layer they apply on (main-set continuity, V-12);
  - `rt_upto_lsn` — the runtime-only fold of [AR §4.5] step 12 and [F17 §5.4] folds the lazy runtime records without the
    graph window, and [F11 §2.4] leaves "the bound" to this chapter; one header must carry both bounds. The alternative, a
    separate segment kind for runtime-only segments, was rejected because it would add a kind to `HEAD.segments` and to
    every set rule;
  - `table_xxh3`, `hdr_xxh3` — [OS/map §4] step 1 checks "the header's own magic and checksum" with one read before
    mapping; the design's BLAKE3 digest covers the file and cannot be checked without reading it;
  - the per-entry `count` — [50] F6 requires "the segment header carries each bitset's total"; giving every entry a count
    makes that uniform.
  The design's `blake3 [32]` is `seg_digest`, placed in the header ([F01 §7.1] calls it a "footer"); it covers every byte
  after the section table, and `hdr_xxh3` covers it in turn. Hashing the bytes after the table, not the whole file, lets a
  streaming writer hash sections as it writes them and write the header last.
- **OP-09-03 (`cs_ref.b3`) — closed in pass 1** (P1-6, S1-19, A1-21). [F06 §9] BK-2 adopted `cs_ref.b3` =
  `seg_digest[0..16]`, the value `FILES.digest16` (§14.4) and [F04 §4.1]'s `SegRef.blake3_16` also use, so V-8 compares
  header fields without reading the file and every `SegHdr` file has one digest; `doctor --fsck` recomputes `seg_digest`.
- **OP-09-04 (which sealed files carry `SegHdr`).** Gap of [PLAN §3.3]; closed in [F10 §2.3]: segments, `cs`, `hist` and
  `blobs` carry it ([AR §4.4] `seg_kind` lists `hist` and `blobs`; [80 §2.5] rule 4 says the same); `gitmap` pages and
  `dict` files have their own headers.
- **OP-09-05 (`TITLE_OFF`, `FIELDS_OFF`, `BODY_REF`).** [AR §4.4] lists them as sections, while [AR §3.1]'s 60-byte
  `NodeHdr` already carries `title_off`, `fields_off` and `body_ref`, which [F08 §3] keeps. Keeping both would store the
  same offsets twice and let them disagree. This chapter writes no separate sections; the byte budget of [AR §4.4]
  (4 + ~60 per row for titles) is then the pool alone.
- **OP-09-06 (absent rows; closed with [F08]).** A dense base and a branch segment that overrides a node its pinned set
  holds both need a row for an id with no node in the view. [F08 §3] makes it `kind` = 0 with all 60 bytes zero; this
  chapter applies the same "zero" to every other row-scoped section.
- **OP-09-07 (no dense `uid` column).** [AR §3.1] lists `uid` among the cold columns; [AR §4.4] lists `UID` as sorted
  `(u128, u32)` and widens the store-wide `ALLOC` with the uid ([72 M7]). A uid never changes for a `#N` (I1), so `ALLOC`
  answers `#N → uid` for every view and `UID` answers `uid → #N` per view. A third, per-view dense copy (16 B per row) is
  not written. [F08 §4] leaves this to this chapter.
- **OP-09-08 (the `SCHEMA` section; a gap).** [AR §2.12] versions the schema per branch and [50] F1–F3 extend its rows, but
  [AR §4.4] names no section that holds a view's schema. Without one, every reader would replay `Schema` ops from genesis.
  This chapter adds `SCHEMA` (fold "full (view)"), as [F08 §8.1] expects. [F08] owns the item encoding and order.
- **OP-09-09 (store-level sections only in main sets).** [AR §5d.1] makes refs, pins, heads, leases, markers, idempotency,
  `ALLOC`/`UIDX`, `gitmap` and R4's evidence store-level. Their sections live in `seg-base` and `seg-delta` only, and readers
  take them from the current main set, never from a pinned set or a branch segment (§4.8), as [F11 §2.4] states.
- **OP-09-10 (`STATS` only in bases).** [50] F7 says "written at checkpoint". Recomputing degree histograms and distinct
  estimates over the whole view at every delta checkpoint is O(nodes) and would break the ≤ 30 / 50 / 100 ms delta budget
  ([AR §4.9]). This chapter writes `STATS` at rollup only; the planner uses exact bitset counts for decisions and `STATS`
  only for estimates ([50 §5.6]). The review should confirm that estimates one rollup old are acceptable.
- **OP-09-11 (tokenizer v1).** [50 §5.5] fixes lower-casing, letter/digit runs, `ё → е`, no stemming, and asks the format
  spec to fix the rest. This chapter decides: Unicode 17.0.0 (the version of `fold_v1`); the simple lowercase mapping, not
  full case mapping or case folding, so one scalar value maps to one; combining marks separate tokens (a strict reading of
  "letter/digit runs"; only text in decomposed form tokenises differently); a 64-byte cut per term. [LQ/std] (WP-19)
  should cite §12.1 for query terms.
- **OP-09-12 (BM25 arithmetic; review S-22).** If `HOLE(F09-doclen)` keeps BM25, [LQ/std] fixes the formula, summation
  order and rounding; this chapter supplies exact per-layer inputs (`tf`, `n_docs`, `DOCLEN`, `FTSSTAT`). If the
  statistics-free scorer wins, S-22 is moot.
- **OP-09-13 (`CONFLICTS` and `GLOBIDX` rows).** [PLAN §3.2] lists `CONFLICTS` under both `09-segments.md` ("F11–F13") and
  `11-runtime-tables.md`. [F11 §10–§11] specifies both sections' rows; this chapter adopts them and keeps only the tag,
  placement and fold. Both are versioned, not runtime: they compose along a view's layers ("row" and "index", §4.7), which
  [F11 §10] states the same way. [F11 §2.4] now states their `RtHdr.form`: `snapshot` in a base, `layer` in a delta,
  promoted-branch or changeset segment (pass 1, round 2). Closed.
- **OP-09-14 (the fingerprint path) — adopted in pass 1** (P1-22). [F11 §12.8]'s `FPRINT` row carries the `blobs` file
  number beside the blob's content address (4 bytes more), so a fingerprint lookup opens one file instead of searching
  every `blobs` file `FILES` lists; whatever replaces a `blobs` file rewrites the rows that name it ([F11 §12.8]).
- **OP-09-15 (`prev` of a bulk commit).** [F06 §7.3] defines `prev` relative to the record's own lsn and [F06 §9] BK-5
  requires "per touched row, its `prev`" in the changeset segment. The segment is sealed before that lsn exists, so `PREV`
  stores the absolute lsn of the previous commit; it stays right under [F16] P-34's node-granular re-validation, which
  [F06 §7.3] and §16.4 cite (pass 1, S1-20). With BK-5's "no before-images", history, merge folds and sync-window
  expansion read a bulk commit as a state delta, and the design's "ops streamed into `cs`" ([AR §4.3]) is read as "their
  effects"; no op list is stored besides `VIOLATIONS` (and [F10 §8] says so, pass 1, A1-22).
- **OP-09-16 (values fixed at append in a changeset segment).** `rev_seq`, `updated_tx`, `created_tx` of created rows and
  `last_op_lsn` cannot be known when the file is sealed. The sentinels (0, and 2^64 − 1 for `last_op_lsn`) require that
  commit `seq` starts at 1 ([F06], [F08]); `lsn` 2^64 − 1 is never a position.
- **OP-09-17 (final ids in a changeset segment) — closed in pass 1** (P1-3, S1-11, A1-12): [F05 §9.27] (kind 27
  `Reserve`) and [F16] P-84 adopted the proposal below, with schema ids and an `hlc` for releasing the files of a
  reservation whose commit never lands; §16.4 cites them and [F11 §9.1] states the `ALLOC` rows of reserved ids. The
  original text: A bulk commit's file is sealed in phase 1,
  before [AR §4.5] step 8 allocates `#N`s and before new symbols, `aN`s and schema ids are allocated under the writer byte.
  The file must still hold final ids, because every reference inside it (CSR, field blocks, `CREATOR`, `ANCHORS`) uses them.
  Proposal for [F16] and [F05] (WP-16, WP-11): the bulk producer first appends, under the writer byte, a reservation group
  that allocates the `#N`s (resolving derived-uid reuse through `UIDX`), the `aN`s, the new symbols and schema ids; it then
  streams the file with those ids; the phase-2 re-validation checks the reserved uids and symbols as read keys. Ids of a
  reservation whose commit never lands are skipped, never reused ([F11 §9.1] already treats them as holes). The alternative —
  provisional ids translated at append — needs a reserved id range in every `u32` reference and was rejected.
- **OP-09-18 (marker field set; gap of [PLAN §3.3]).** Closed by [F11 §7] (the 72-byte row with its holder list, the key
  and the active/old split); this chapter adds nothing.
- **OP-09-19 (the `index` values of `FPROMO`).** [50] F5 creates `FCOL` and `FIDX` "for fields with `index ≠ none`". This
  chapter follows [F08 §8.4.3]: `column` gives an `FCOL`; `bitmap` gives an `FIDX` and, unless the type is `set`, an `FCOL`.
  A `set` field with `index` = `column` uses `FCOL`'s set form.
- **OP-09-20 (derived-optional sections in v1).** `DIRMAP` ([80] X-F8) and the four caches [F11 §2.8] proposes (`FPRINT`,
  `ANCESTRY`, `GITRENAMES`, `ANCHORRES`). Other derived sections (`STATS`, `TERMS`/`POST`, `GLOBIDX`, the bitsets) are
  rebuildable too, but a reader must find them; making them optional would add fallbacks to every read path.
- **OP-09-21 (measurement 14 and the layouts).** The `NODE` layout (an array of 60-byte headers) follows [AR §4.4]'s
  `[NodeHdr; n]`, while [AR §3.1] speaks of structure-of-arrays columns. The hot header is one column group read per row; the
  cold columns are separate arrays. Measurement 14's column-scan probe runs on this layout; if it shows a per-field column
  split is needed, that is a finding against this chapter before the freeze.
- **OP-09-22 (the fold window of `ANCESTRY`).** [AR §2.14] calls the ancestry cache "lazy `ANCESTRY` facts" appended by
  `check`; [40 §2.6] and [F11 §12.12] put ancestry answers into `GITFACTS`, whose record kind `GitFacts` is in `K_RT`. This
  chapter folds `ANCESTRY` in the runtime window. WP-11's gap "record kinds for lazy `ANCESTRY`" must pick `GitFacts` or
  another kind of `K_RT` ([F17 §5.1]).
- **OP-09-23 (reconciliation with the sibling drafts).** [F04], [F06], [F08] and [F11] were drafted in parallel with this
  chapter. This chapter adopts their choices where they own the bytes: [F11]'s runtime section body, forms and proposed tags
  (`0x0201`–`0x021A`, so the `hist` and `blobs` tags sit at `0x0300`–`0x0311`), its `FileFamily` values for `seg_kind`
  ([F11] open point 11), its `CONFLICTS`, `GLOBIDX` and `FPRINT` rows and its `derived-optional` proposal; [F08]'s `NONE32`
  offsets, `kind` = 0 absent rows, edge ids and `index` values; [F04]'s `SegRef` digest rule; [F06]'s BK-5 list. OP-09-03,
  the one disagreement, closed in pass 1.
- **OP-09-24 (the reverse index of `parent`).** [F08 §10.1] holds `parent` in `NodeHdr.parent` and leaves its reverse
  direction to this chapter. The children of p are the in-list entries of kind `parent` in p's row, so `subtree`,
  `children_total` checks and I4's forest test read one CSR slice; the out-lists carry no `parent` entry, so the forward
  direction has one source of truth.
- **OP-09-25 (`FILES`, a registry [F04] requires).** [F04 §4.1] and its open point 4 name only `main`'s base, deltas and
  dictionary in `HEAD` and leave "`hist`, `blobs`, `gitmap`, `cs` and promoted-branch files … named by records and folded
  registries" to [F09] and [F10]. `FILES` is that registry, a snapshot in every main-set layer; with `HEAD.segments` and
  `PINS` it names every live sealed file. It also lists every live dictionary, because older ones stay live while a pinned
  set's `blobs` file names them. [F05] (WP-11, its open point 8) gives the records that add and remove entries in
  the tail.
- **OP-09-26 (`SCHEMAIDS`, a section [F08] requests).** [F08 §8.3] proposes the name and leaves the section to [F09] and
  [F11]. It is store-level and small (at most 191 kinds, 191 edge kinds and the project enumeration values), so this
  chapter defines it with [F11]'s body and an append-only layer form.
- **OP-09-27 (the value order of `FIDX`).** [50] F5 asks for `FIDX` "walkable in value order" without naming the order. This
  chapter uses [F08 §5.5]'s stored order (symbol ids and enumeration integers numerically), so `FCOL` sets, field-block
  sets and `FIDX` agree and a writer needs no string comparison. `ValueJoin` and postings-ordered grouping need only a
  consistent order; `ORDER BY` in `sort_rank` or string order re-sorts the directory. An earlier draft of this chapter used
  string and `sort_rank` order; [F08] (drafted in parallel) settled the stored order first.
- **OP-09-28 (pass 1 additions).** `EDGE_PROPS` widened to 40 bytes with `pflags` and the full pin (S1-2, A1-3, P1-1);
  `CKIMG` in changeset segments for a bulk import-checkpoint (S1-23, A1-10); the absent-to-deleted row of a bulk
  `CreateDeleted` (S1-6); schema-key conflict rows folding with `SCHEMA` (S1-7); the tags `0x020A`–`0x020C` of
  [F11 §13]'s session and backup tables (A1-23). The review should re-check §7.2, §8.2 and §16.4 against [F08 §10.2],
  [F11 §10] and [F06 §9] BK-5.
- **OP-09-29 (pass 1, round 2).** `FILES.flags` bit 0 `reserved` keeps the `cs` and `blobs` numbers of a folded
  `Reserve` record ([F05 §9.27]) named until its bulk `Commit` folds or `gc` releases them, which [F16] P-77 condition 2
  and P-84 need once the record leaves the tail (§14.4; P1-3); a promoted `commitref` set stores each `id16` once (§10.2);
  §16.4 cites [F05 §9.27] and [F16] P-84 for the ids of a changeset segment (OP-09-17 closed).
