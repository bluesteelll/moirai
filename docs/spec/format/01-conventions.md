# 01 — Conventions

| | |
|---|---|
| Title | Conventions: how the specification is written, byte order and layout, integer and string encodings, the hash set, symbols, versions and reserved bytes |
| Chapter | [F01], `docs/spec/format/01-conventions.md` |
| Status | draft, pass 1 pending |
| Work package | WP-10 (R-SPEC-F), [60 §3.1] item 1 |
| Sources | [80 §1] principles X1–X9 (summarised in [AR §14]); [80 §2.7.1] (`ProcId.os`, `boot_id`); [80 §2.13] (byte order and word size); [80 §3.1] X-F1, X-F2, X-F3, X-F9; [80 §3.2] (HLC and timestamps, config line ends); [80 §4.2] T6, T7; [AR §2.12] (symbols never collected, enum integers never reused); [AR §3.1] (field block, value types, `CREATOR`); [AR §4.2] (`HEAD` slot checksum); [AR §4.3] (`RecHdr`, commit header encoding: presence bitmap, LEB128 varints, symbol fields); [AR §4.4] (`SegHdr`, `SYMTAB`, `TOMB`, `IDEM`, `LEASES`); [AR §4.6] (canonical form; what is not hashed); [AR §12] (no auto-migration); [40 §2.3] (`lp()`), [40 §2.5] (`oid`), [40 §2.11] R-1, R-3; [50 §8.1] F1, F3, F4, F10; [60 §2.5] (preamble: format version 1); [71 RAM-m6] (actor width); [90 §4.4], [90 §10.1]; [PLAN §3.2] WP-10, [PLAN §3.3] (the symbol-width gap) |
| Depends on | nothing; every other part depends on this chapter |

## 1. Scope

This chapter fixes the conventions that every other part of the specification uses. It defines no file and no record.
Where another chapter defines a structure, that chapter owns its bytes. This chapter owns the primitives those
structures are built from:

- how the specification itself is written: normative words, citation form, chapter structure, precedence, named holes,
  offset tables and coverage rows (§2);
- the cross-platform principles X1–X9 and what each fixes in the text (§3);
- byte order, word size, packing, bit numbering and magic numbers (§4);
- fixed-width integers, varints, booleans, enumerations, floating point, fixed-width byte strings and time values (§5);
- text, length-prefixed byte strings, `lp()`, hexadecimal and decimal text, ordering, and hand-editable text files (§6);
- the hash set and the git object-format registry (§7);
- symbols and the symbol-width rule (§8);
- version fields (§9);
- reserved and padding bytes (§10).

## 2. How the specification is written

### 2.1 Normative words

"Must", "must not", "never", "only" and "is" state requirements. "May" states a permission. A paragraph or table
marked *(informative)* states no requirement. Examples are informative unless a chapter says a fixture must reproduce
them.

### 2.2 Parts and citation form

| Form | Points to |
|---|---|
| `[FNN §x.y]` | format chapter `docs/spec/format/NN-name.md`, section x.y; `[FNN]` alone is the whole chapter |
| `[OS/<file> §x]` | a file of the OS-layer specification, `docs/spec/os/<file>` |
| `[LQ/<file> §x]` | a file of the query-language contract, `docs/spec/lq/<file>` |
| `[API §x]` | `docs/spec/store-api.md` |
| `[CFG §x]` | `docs/spec/config.md` |
| `[RULES/<file>]` | a rule table, `docs/spec/rules/<file>` |
| `[AR §x]` | `docs/ARCHITECTURE-RESEARCH.md` |
| `[40 §x]`, `[50 §x]`, `[60 §x]`, `[80 §x]`, `[90 §x]` | the normative designs in `docs/research/design/` |
| `[NN §x]` for other two-digit tags, `[A]`–`[D]`, `[X17]`–`[X20]`, `[H21]`–`[H23]` | the research reports, proposals and critiques, with the tags of [AR]'s source table |
| `[PLAN §x]` | `docs/m0/PLAN.md` |
| `[RFC 3629]`, `[RFC 5234]`, `[RFC 7405]`, `[FIPS 180-4]`, `[BLAKE3]`, `[XXH3]`, `[IEEE 754]`, `[git-objects]`, `[git-hash-transition]` | external specifications: UTF-8; ABNF; case-sensitive ABNF strings (`%s`); SHA-1 and SHA-256; the BLAKE3 specification; the xxHash XXH3 specification; binary floating point; git's object, pack and index formats; git's SHA-256 object format |

Rules:
- In `[OS/<file>]`, `[LQ/<file>]` and `[RULES/<file>]`, `<file>` is the file name without a `.md` extension
  (`[OS/fs §4.9]` is `docs/spec/os/fs.md`); another extension is kept (`[LQ/grammar-v1.ebnf]`).
- A section citation includes its subsections. Ranges are written `[AR §4.1–§4.4]`.
- Labelled items are cited with the document that owns their table: `[40] R-7`, `[50] F10`, `[80] X-F3`, `[80] P9`,
  `[80 §4.2] T5`, `[60 §3.13] GT13`, `[40 §2.10] I-F12`. `R-1`…`R-18`, `F1`–`F18` and `X-F1`–`X-F12` may be written bare
  once the table has been cited in the same chapter.
- A citation of a spec file that is not yet written is still made; the index (`docs/spec/README.md`) shows its status.

### 2.3 Chapter structure

Every chapter of every part begins with a header block, a two-column table with these rows in this order:

| Row | Content |
|---|---|
| Title | what the chapter specifies |
| Chapter | the citation tag and the path |
| Status | one of: `planned`, `draft, pass 1 pending`, `pass 1 closed`, `pass 2 closed`, `frozen (format-v1)` |
| Work package | the WP of [PLAN §3.2] and the author role |
| Sources | the exact design-document sections the chapter specifies, never headings alone |
| Depends on | the chapters whose definitions it uses |

Every chapter ends with, in this order, a **Coverage** section (§2.7), a **Holes** section (§2.5) and an **Open points
for the review** section. "Open points for the review" records every gap the chapter closed on its own authority, every
conflict between design documents it resolved, and every question it leaves to another chapter, each with the proposed
resolution.

### 2.4 Precedence

1. The design documents are normative ([AR] binding inputs, the owner review of 2026-09-27, A4). A chapter specifies
   them at byte level. A chapter text that contradicts a design document is a review finding, not a change of design.
2. Where design documents disagree with each other, a chapter follows [40], [50], [80] and [90] for their own
   reservations and mappings (R-1…R-18, F1–F18, X-F1–X-F12 and [80 §2]'s per-OS mappings, [90 §10.1]) and [AR]
   otherwise, and records the conflict in its open points ([PLAN §1]).
3. Where the design fixes nothing and no measurement decides, the owning chapter decides and records the decision as an
   open point. It never leaves such a byte unspecified.
4. Among the parts of this specification, the chapter that owns a structure (by the chapter map in
   `docs/spec/README.md`) is authoritative for its bytes. Another chapter cites it and does not restate the bytes.
5. A convention of this chapter applies to every structure unless the owning chapter states an exception explicitly and
   cites the section of this chapter it departs from.

### 2.5 Named holes

A value that an M0 measurement or benchmark decides ([60 §3.1] "Decisions fixed at M0 exit", [60 §5.2]) is a **named
hole**, written inline as `HOLE(<id>)`.

- **Id syntax.** `<part>-<name>`, where `<part>` is `F01`…`F20`, `OS`, `LQ`, `API` or `CFG`, and `<name>` is lower-case
  ASCII letters, digits and hyphens. Ids are unique across the specification. Example: `HOLE(F02-dict-file)`.
- **Holes table.** Every hole appears in its chapter's Holes section as one row with the columns
  `id | what | decided by (measurement n / WP) | candidates | constraint the value must meet`.
- **What may be a hole.** Only a value a measurement or benchmark decides. A value the design fixes is written. A value
  that is neither fixed nor measured is decided by the chapter (§2.4 rule 3).
- **What a hole may leave open.** A value. It never leaves open a width, an offset, or whether a later field exists,
  unless its candidates column lays out every alternative completely (for example, a section that is kept or dropped).
- **Filling.** WP-81a fills every hole. The value then replaces the inline `HOLE(...)` text, and the Holes row stays,
  with the value and a reference to the measurement record. Fixtures that depend on a holed value are written after the
  fill (WP-20b); before it, a fixture uses the test profile of [F17] or does not exist.

### 2.6 Offset tables, sequence tables, bit tables and enumeration tables

**Offset table** — for every fixed-size structure. Columns, in this order:

| offset | width | type | name | meaning |
|---|---|---|---|---|

- `offset` is the decimal byte offset from the first byte of the structure; `width` is the decimal byte count.
- `type` is a type of §5–§7, `[N]u8` for an opaque byte array, or the name of another structure that has its own table.
- `name` is the field name as code and fixtures spell it.
- `meaning` states the field's meaning, its valid values, and, for checksums, the covered byte range (§7.4).
- Rows cover every byte, in order, with no gap and no overlap. Bytes that carry no field are explicit rows named
  `_reserved` or `_pad` (§10). The table ends with a row `total | N`.
- Integers are little-endian (§4.1). A chapter that departs from this says so above the table.

**Sequence table** — for a variable-length encoding (a record body, a commit header, a text line):

| order | name | encoding | present when | meaning |
|---|---|---|---|---|

Fields follow each other in `order` with no padding. `encoding` is a type or named encoding of §5–§6. `present when`
names the presence bit or condition, or says `always`.

**Bit table** — for a flags field or a bitmap: `bit | name | meaning`. Bit 0 is the least significant bit (§4.4).
Unnamed bits are reserved-zero (§10).

**Enumeration table** — for an enumeration: `value | name | meaning`. Values not listed are reserved and invalid in
format v1 (§5.4).

*(Informative)* §7.5 has an example of an offset table.

### 2.7 Coverage rows

Each chapter's Coverage section lists every item it specifies, one row each: every row of [60 §2.5] (by its area and
content), every reservation R-1…R-18 ([40 §2.11], authoritative), F1–F18 ([50 §8.1]), X-F1–X-F12 ([80 §3]) and every item
of [90 §10.1]. A row that covers only part of an item says which part and names the chapter that covers the rest.
`docs/spec/COVERAGE.md` merges the rows of all chapters into one table with the columns `item | chapter § | fixture |
model function`; `xtask coverage` checks the fixture and model columns ([PLAN §3.2] item 1).

## 3. Principles X1–X9

### 3.1 The principles and what they fix in this specification

The principles are [80 §1]'s, adopted in [AR §14]. Each binds every chapter as follows.

| # | Principle ([80 §1]) | What it fixes in this specification |
|---|---|---|
| X1 | One on-disk format: every file, record, section, image object and runtime row has one layout, little-endian and 64-bit, independent of page size and path separator; OS-specific runtime data carries an OS kind tag, and a row whose tag this OS cannot interpret is ignored, as if absent | §4.1 byte order; §4.2 word and page size; stored paths use `/` only ([80 §2.10] P1, P12); the OS kind tag registry of §3.2. Big-endian and 32-bit targets are unsupported |
| X2 | One protocol semantics, written against the weakest OS | [F15] specifies the weakest fault model and [F16] one protocol; no chapter makes a byte or a rule depend on the OS a process runs on |
| X3 | OS differences live only behind the OS layer (`moirai-os`) | format chapters name `Vfs` and `ProjectFs` operations and durability classes ([80 §2.3]), never OS calls; OS calls appear only in `[OS/...]`, in its per-OS mapping appendices, and in notes marked informative |
| X4 | Implementations are selected at compile time, never swapped | no chapter specifies a runtime-selectable alternative encoding or protocol for one concern |
| X5 | Nothing is silently weakened: a guarantee an OS cannot give refuses the store or marks the answer (`unverified`, `Unknown`, Unknown-boot mode) | every chapter states, for each guarantee it relies on, whether its absence refuses (and with which exit code, [F19]) or marks; no silent fallback is specified anywhere |
| X6 | The weakest client (sandboxed, namespaced, another principal) defines the contract | no rule requires IPC, a readable foreign PID, a socket or a system identifier; an unreadable identity degrades to a defined mode ([80 §2.7]) |
| X7 | Windows is implemented and gated in M0–M11; Linux and macOS are specified now and ported later | every chapter specifies all three OSes now |
| X8 | Ports never change format or protocol | a byte or rule that only Linux or macOS uses is specified now (for example `DIRMAP`, the Linux file-handle digest, `OsFileId.docid`, the FSEvents kind of `JOURNALCUR`); a later need for a change is a specification defect that reopens M0 ([60] P4) |
| X9 | Public, documented interfaces only in product code | an undocumented source is read only where its absence selects a defined mode (macOS `kern.bootsessionuuid` → Unknown-boot mode, [80 §2.7.1]) |

### 3.2 OS kind tags

Every field named `os` in a runtime row, a `LOCK` record, an anchor or a `ProcId` ([80 §2.7.1], [80] X-F1, X-F2, X-F8)
holds one value of this registry (type `u8`):

| value | name | meaning |
|---|---|---|
| 0 | `unspecified` | never written by a process as its own tag; a value that carries it is uninterpretable and treated as absent |
| 1 | `windows` | written by a Windows process; OS-specific content in Windows form |
| 2 | `linux` | written by a Linux process; OS-specific content in Linux form |
| 3 | `macos` | written by a macOS process; OS-specific content in macOS form |

Values 4–255 are reserved and uninterpretable. These are the values `[OS/proc §2]` fixes; this section restates them as
the convention every chapter uses. A reader interprets the OS-specific content of a row only when the row's tag equals
its own OS; otherwise it treats the row as absent (X1), unless the owning chapter names fields that remain readable for
diagnostics (for example a foreign `ProcId` that `doctor` prints). Whether a structure with no OS-specific content (for
example a free slot record) carries tag 0 is the owning chapter's rule. Finer kinds inside a structure (for example the
kind of an `OsFileId`) are also the owning chapter's ([F11]).

## 4. Byte order, word size and layout

### 4.1 Byte order

- Every multi-byte integer in a structure this specification defines is **little-endian**; signed integers are two's
  complement.
- Byte strings (digests, identifiers, text, magic numbers) have no byte order: they are stored first byte first.
- Formats moirai reads or writes but does not define follow their own specifications, byte order included: git's
  object, loose-object, pack, pack-index, commit-graph and ref formats ([git-objects], [git-hash-transition]; network
  byte order where git uses it); LZ4 block and zstd frame formats inside compressed payloads ([F10]); the Unicode
  Character Database files ([F20]). The chapter that uses such a format cites its specification.

*(Informative)* The u32 value `0x0A0B0C0D` is stored as `0D 0C 0B 0A`; the u64 value `0x0102030405060708` as
`08 07 06 05 04 03 02 01`.

### 4.2 Word size and page size

- File offsets, file lengths and log sequence numbers that can exceed 2^32 − 1 are `u64`. No field's width depends on
  the platform's pointer size.
- No field, alignment or rule depends on the page size (4 KiB on Windows and x86-64 Linux, 16 KiB on macOS arm64,
  [80 §2.5]). Mapped files are mapped whole, from offset 0.
- No stored byte depends on the OS path separator: every stored path uses `/` ([80 §2.10] P1, P12).

### 4.3 Packing and alignment

- Structures are **byte-packed**: a field starts at the byte after the previous field ends. There is no implicit
  padding. Padding exists only as an explicit `_pad` or `_reserved` row of the offset table (§2.6, §10).
- Readers never assume that a field is aligned. *(Informative: the product codec's `zerocopy` little-endian types have
  alignment 1; the format oracle parses bytes.)*
- A chapter may require a structure or a section to begin at an offset that is a multiple of N (for example 8 for
  segment sections, [80 §3.2]). It then states N and that the gap before the structure is zero bytes, which are
  reserved-zero (§10).

### 4.4 Bits, flags and bitmaps

- Bit *i* of an integer field is `(value >> i) & 1`; bit 0 is the least significant bit.
- In a bitmap stored as a byte array, bit *i* is bit `i mod 8` of byte `i div 8` (least significant bit first). This
  equals the numbering of the same bitmap read as little-endian `u64` words.
- A flags field lists its bits in a bit table (§2.6). Unnamed bits are reserved-zero (§10).

### 4.5 Magic numbers

A magic number is a fixed ASCII byte string at a fixed offset, stored first byte first and compared as bytes, never as
an integer: `"MOIR"` is the bytes `4D 4F 49 52`. A new magic is four printable ASCII bytes beginning with `M`.

*(Informative)* Magics named in the design: `MOIR` (`HEAD` slot, [F04]), `MLCK` (`LOCK` header, [F03]), `MSEG` (segment
header, [F09]), `MDIC` (dictionary header, [F10]); and `MSWP`, the `restore` swap intent that `[OS/fs §4.9.3]` defines.
The owning chapter is authoritative.

## 5. Integers and scalar values

### 5.1 Fixed-width types

| Type | Width | Encoding |
|---|---|---|
| `u8` | 1 | unsigned |
| `u16` | 2 | unsigned, little-endian |
| `u32` | 4 | unsigned, little-endian |
| `u64` | 8 | unsigned, little-endian |
| `i32` | 4 | two's complement, little-endian |
| `i64` | 8 | two's complement, little-endian |
| `f64` | 8 | §5.5 |
| `bool8` | 1 | §5.4 |
| `[N]u8` | N | opaque bytes, first byte first |
| `b16`, `b20`, `b32` | 16, 20, 32 | fixed-width byte strings, §5.6 |

### 5.2 Unsigned varints (`uvar`)

The design's "LEB128 varint" ([AR §4.3]) is **unsigned LEB128**: the value is split into 7-bit groups, least
significant group first; every byte except the last has bit 7 set; the last byte has bit 7 clear.

- **Bound.** Every varint field has a declared bound, written `uvar16`, `uvar32` or `uvar64`, from the width the design
  gives the field (a `u32` field encoded as a varint is `uvar32`). The maximum lengths are 3, 5 and 10 bytes.
- **Canonical form.** An encoding is valid only if it is the shortest one for its value: its last byte is not `00`,
  unless the whole encoding is the single byte `00`.
- **Decoding refuses** a non-canonical encoding, an encoding longer than the bound's maximum, a value above the bound
  (for `uvar64`, a tenth byte other than `00` or `01`), and an encoding cut off by the end of its container. A refused
  varint makes its containing structure invalid, with the consequence the owning chapter states (for a log record:
  the record is invalid, [F05]).

*(Informative)* Examples:

| Value | Bytes |
|---|---|
| 0 | `00` |
| 1 | `01` |
| 127 | `7F` |
| 128 | `80 01` |
| 300 | `AC 02` |
| 16,383 | `FF 7F` |
| 16,384 | `80 80 01` |
| 65,535 | `FF FF 03` |
| 2^32 − 1 | `FF FF FF FF 0F` |
| 2^64 − 1 | `FF FF FF FF FF FF FF FF FF 01` |

`80 00` (0 in two bytes) and `81 00` are refused as non-canonical.

### 5.3 Signed varints (`svar`)

The design's "zigzag varint" ([AR §3.1]) is `svar64`: an `i64` value *i* is mapped to the `u64` value
`z = (i << 1) XOR (i >> 63)` (arithmetic shift), which is then encoded as `uvar64`. Decoding computes
`i = (z >> 1) XOR −(z AND 1)`. The canonical-form and refusal rules of §5.2 apply to the `uvar64`.

*(Informative)* Examples:

| i | z | Bytes |
|---|---|---|
| 0 | 0 | `00` |
| −1 | 1 | `01` |
| 1 | 2 | `02` |
| −64 | 127 | `7F` |
| 64 | 128 | `80 01` |
| −65 | 129 | `81 01` |
| 2^63 − 1 | 2^64 − 2 | `FE FF FF FF FF FF FF FF FF 01` |
| −2^63 | 2^64 − 1 | `FF FF FF FF FF FF FF FF FF 01` |

### 5.4 Booleans and enumerations

- `bool8`: `00` is false, `01` is true; any other value makes the structure invalid. (The field block's `bool` type
  carries no value bytes at all, [AR §3.1]; its encoding is [F08]'s.)
- An enumeration is a `u8` unless its owning chapter states another width. Its values are listed in an enumeration
  table (§2.6). A value not in the table is invalid in format v1. Enumeration values are never reused for another
  meaning ([AR §2.12]).

### 5.5 Floating point

`f64` is an IEEE 754 binary64 value; its 64-bit pattern is stored as a `u64` in little-endian order. The rules that make
an `f64` canonical where it enters a canonical form, a sort key or a merge comparison (NaN, the sign of zero) are the
owning chapter's ([F07], [F08]; open point 8).

### 5.6 Fixed-width byte strings: identifiers and digests

- `b16`, `b20` and `b32` are byte strings of 16, 20 and 32 bytes. They have no byte order and no arithmetic. They are
  compared bytewise (§6.6) and written as hexadecimal text in byte order (§6.4).
- Where [AR] writes `u128` for a node's `uid` ([AR §3.1], [AR §4.4] `UID`), the type is `b16`. Sorting by `uid` is
  bytewise, so the order of the binary keys equals the order of their hexadecimal text.
- `id16` in [AR §4.3] is the first 16 bytes of a 32-byte commit id (§7.1). `blake3_16` fields are BLAKE3-128 digests.
- A store id ([F02 §4]), a derived or random `uid`, and every BLAKE3-128 digest are `b16`.

### 5.7 Time values

| Name | Type | Meaning |
|---|---|---|
| `hlc` | `u64` | hybrid logical clock value `(unix_ms << 16) OR counter`: bits 16–63 hold milliseconds since 1970-01-01T00:00:00Z (leap seconds not counted), bits 0–15 a counter ([AR §4.3]). The 48-bit millisecond part lasts until the year 10889. Values compare as `u64` |
| `unix_ms` | `u64` | milliseconds since 1970-01-01T00:00:00Z, UTC ([80 §3.2]) |
| `unix_ns` | `i64` | nanoseconds since 1970-01-01T00:00:00Z, UTC; negative before 1970. File timestamps of R4's runtime rows use it, each with a granularity field ([40] R-18, [F11]) |
| `boot_ns` | `u64` | nanoseconds of the boot clock, monotonic and including suspend ([80 §2.7.1]); meaningful only together with the boot identity under which it was read |

The rule by which a process advances its `hlc` is [F16]'s. The `hlc` of a commit created by foreign or checkpoint
import is [F06]'s (the seconds-versus-milliseconds gap of [PLAN §3.3] is WP-12's). Conversion from an OS time format
(Windows `FILETIME`) happens in `os::proc` ([80 §3.2]).

*(Informative)* `unix_ms` = 1,790,000,000,000 with counter 3 gives `hlc` = `0x01A0C4506C000003`, stored as
`03 00 00 6C 50 C4 A0 01`.

### 5.8 Zero as "none"

Zero means "none" or "empty" only where a chapter says so. This chapter fixes four such uses: store file number 0
names no file ([F02 §6.2]); symbol id 0 is the empty string in every symbol class (§8.1); OS tag 0 is `unspecified`
(§3.2); git object-format value 0 is `none` (§7.5).

## 6. Strings

### 6.1 Text

- Text is UTF-8 ([RFC 3629]): Unicode scalar values only, so no encoded surrogate code points (U+D800–U+DFFF), no
  overlong forms and nothing above U+10FFFF. A field typed as text holds valid UTF-8; invalid UTF-8 in such a field makes
  the structure invalid.
- Text is stored as **exact bytes**. No Unicode normalisation, case folding, trimming or line-end conversion is applied,
  except where a rule of the owning chapter says so. The rules the design names: NFC for untracked names on a
  normalization-insensitive volume ([80 §2.10] P3); NFC normalisation of a new ref name on input ([80] X-F9); message
  normalisation at write time ([AR §4.6] item 6, [F07]); `fold_v1` only for index order and collision and twin checks,
  never for identity ([80 §2.10] P6, [F20]).
- Every text file moirai writes into a store, a pointer file, a configuration file or an image destination is UTF-8
  without a byte-order mark, with LF line ends. The bytes moirai writes to stdout and stderr are [F19]'s
  ([80 §4.2] T7).

### 6.2 Length-prefixed byte strings

| Name | Encoding | Use |
|---|---|---|
| `vbytes` | a `uvar32` length L, then L bytes | variable-length byte strings inside records and sections — the design's "length-prefixed bytes" |
| `vstr` | a `vbytes` whose bytes are valid UTF-8 (§6.1) | text inside records and sections — the design's "varint-length UTF-8", for example the text part of a `path` value ([40] R-1) and `TITLE_BLOB` entries ([AR §4.4]) |
| `fstr<N>` | exactly N bytes: a `u16` length L with L ≤ N − 2, then L bytes of UTF-8, then N − 2 − L zero bytes | bounded text inside a fixed-size structure. A longer text is cut at the last scalar-value boundary at or before N − 2 bytes. The zero bytes are reserved-zero (§10) |

*(Informative)* The text `docs/a.md` as `vstr` is `09 64 6F 63 73 2F 61 2E 6D 64`. The text `gc --rollup` as
`fstr<16>` is `0B 00 67 63 20 2D 2D 72 6F 6C 6C 75 70 00 00 00`.

A chapter that uses `fstr<N>` names N. Whether a chapter uses `vbytes`, `vstr` or `fstr<N>` for a given field is that
chapter's decision.

### 6.3 `lp()`: the length prefix of hash inputs

```
lp(x) = u32-le(len(x)) ‖ x
```

exactly as [40 §2.3] defines it: the byte length of `x` as a little-endian `u32`, followed by the bytes of `x`.

- `lp()` builds inputs of the hash functions of §7. It is never a stored encoding.
- `len(x)` is below 2^32. An argument that would be longer is refused by the operation that builds the input.
- An argument that is a byte string or text enters as its bytes (text as its UTF-8 bytes). An argument that is absent,
  or "empty" in the design's formula (for example "`lp(origin_pred or empty)`", [40 §2.3]), is the empty byte string, so
  `lp` contributes the four bytes `00 00 00 00`.
- An integer argument enters as its little-endian bytes of its declared width (§7.3), then `lp` applies. The conversion
  of any other typed argument (an enumeration, a structured value) is stated by the chapter that defines the
  derivation ([F08], [F20]; open point 9).
- The length-prefix scheme of the canonical commit encoding ([AR §4.6]) is [F07]'s (a WP-12 gap of [PLAN §3.3]). If
  [F07] uses `lp()`, it cites this section.

*(Informative)* `lp("moirai-file-v1")` is `0E 00 00 00 6D 6F 69 72 61 69 2D 66 69 6C 65 2D 76 31`.

### 6.4 Hexadecimal text

- Hexadecimal text has two digits per byte, in byte order (first byte first), lower-case `0`–`9` and `a`–`f`, with no
  separator and no prefix. A 16-byte value is 32 digits, a 20-byte value 40, a 32-byte value 64.
- A truncated form "`hex(x)[0..n]`" ([80] X-F9, [50] F3) is the first n digits, which are the first n/2 bytes of `x`.
- moirai writes only lower-case. A parser of text that moirai writes (pointer files, the image, trailers) accepts only
  lower-case. A parser of user input accepts upper-case only where [F19] or `[LQ/...]` says so.
- In prose, `0x` introduces a number in hexadecimal with the most significant digit first (`0x2A` = 42). That notation
  describes values, not stored bytes.

### 6.5 Decimal text

A number written as text — in a store file name ([F02 §6]), a text format or a trailer — is ASCII decimal digits with
no sign (for an unsigned value), no leading zeros (zero is `0`), no separators and no fractional part. A negative value
is written with a leading `-`. A chapter that needs another spelling defines it.

### 6.6 Order

- Byte strings, and text, are ordered by **bytewise lexicographic comparison of unsigned bytes** (`memcmp`); a proper
  prefix sorts before the longer string. For UTF-8 text this equals code-point order.
- Integers are ordered numerically; `f64` values by the owning chapter's rule (§5.5).
- A tuple is ordered field by field, left to right.
- "Sorted" in any chapter means ascending in this order, with no two equal keys unless the owning chapter allows
  duplicates and states their order.

### 6.7 Hand-editable text files

Rules for the text files a person may edit by hand: pointer files ([F02 §3.3]) and, where [CFG] cites this section, the
store and user configuration files.

- **Writing.** moirai writes UTF-8 without a byte-order mark, ends every line, the last included, with LF, and writes
  nothing else than the file's grammar allows.
- **Reading.** A reader skips one leading byte-order mark (`EF BB BF`), accepts LF or CR LF as a line end, and accepts a
  missing line end after the last line. A CR not followed by LF, a NUL byte, or invalid UTF-8 makes the file malformed.
  This matches the stdin rule T6 of [80 §4.2] and the configuration rule of [80 §3.2].

## 7. The hash set

### 7.1 Functions

| Name | Definition | Output | Uses in format v1 (owning chapter) |
|---|---|---|---|
| BLAKE3-256 | the BLAKE3 hash function ([BLAKE3]) in its default, unkeyed mode; the first 32 bytes of its output | `b32` | commit ids and `changeset_digest` ([F07]); segment footers ([F09]); the named-query file name `q` ([80] X-F9, [50] F3, [F14]) |
| BLAKE3-128 | the first 16 bytes of the same output, so BLAKE3-128(x) = BLAKE3-256(x)[0..16] | `b16` | derived uids ([40 §2.3, §2.7], [F08]); every `blake3_16` field (`SegRef`, `BLOBTAB`, `cs_ref`, the dictionary header); body hashes; anchor digests `quote_h`, `prefix_h`, `suffix_h` ([40] R-10, [F07]); session hashes of anchors and slot records ([80] X-F2, [90 §4.4], [F03]); idempotency key and payload hashes ([F06]); `TREES` and `HEADS` keys ([80] X-F7, [F11]); the canonical-AST hash ([50] F3) |
| XXH3-64 | XXH3's 64-bit variant ([XXH3]) with seed 0 and the default secret. The seeded form XXH3-64(x; s) is the same variant with seed s | `u64` | `RecHdr.xxh3_64` and the group `chain` trailer ([80] X-F3, [F05]); section checksums ([F09]); `LOCK` record checksums ([80] X-F1, [F03]) |
| XXH3-128 | XXH3's 128-bit variant with seed 0 and the default secret | a 128-bit value (§7.2) | the `HEAD` slot checksum `xxh3_128` ([AR §4.2], [F04]) |
| SHA-1 | [FIPS 180-4] | `b20` | git object ids in a repository whose object format is `sha1`; `oid` values with algorithm `sha1` ([40 §2.5]) |
| SHA-256 | [FIPS 180-4] | `b32` | git object ids in a repository whose object format is `sha256`; `oid` values with algorithm `sha256`; the SHA-256 pins of the Unicode data files ([PLAN §2.4]) |

- BLAKE3's keyed mode and key-derivation mode are not used in format v1. A chapter that needs domain separation puts a
  fixed `lp("moirai-…-v1")` prefix in the input, as [40 §2.3] does.
- SHA-1 and SHA-256 compute git object ids and file pins only, never a moirai commit id or uid.
- `id16` is the first 16 bytes of a commit id; for any input it equals BLAKE3-128 of that input.
- *(Informative)* The implementations are the pure-Rust crates of [PLAN §2.4] (`blake3` with `pure`, `xxhash-rust`,
  `sha1`, `sha2`). Test vectors are those of each function's specification; this chapter reproduces none.

### 7.2 How outputs are stored

- A BLAKE3 or SHA digest is a byte string stored in output order (`b16`, `b20`, `b32`).
- An XXH3-64 result is a `u64` **value** stored little-endian (§4.1). It is **not** stored in xxHash's "canonical
  representation", which is big-endian.
- An XXH3-128 result is the pair (low 64 bits, high 64 bits), stored as the 128-bit value in little-endian order:

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 8 | `u64` | `low64` | the low 64 bits of the XXH3-128 value |
| 8 | 8 | `u64` | `high64` | the high 64 bits of the XXH3-128 value |
| total | 16 | | | |

*(Informative)* The XXH3-64 value `0x0102030405060708` is stored as `08 07 06 05 04 03 02 01`. An XXH3-128 value with
`low64 = 0x1112131415161718` and `high64 = 0x2122232425262728` is stored as
`18 17 16 15 14 13 12 11 28 27 26 25 24 23 22 21`.

- When a chapter takes an integer from a digest (for example a `u64` hash field), the integer is the first k bytes of
  the digest read as a little-endian unsigned integer of k bytes, unless the owning chapter says otherwise.

### 7.3 How inputs are formed

- An input is exactly the bytes the owning chapter specifies.
- An integer operand contributes its little-endian bytes of its declared width. So XXH3-64(epoch), the chain seed after
  `init` or an epoch re-roll ([AR §4.3], [80] X-F3), hashes the 8 little-endian bytes of the `u64` epoch.
- A text operand contributes its UTF-8 bytes, a byte-string operand its bytes.
- An input made of several operands frames each operand with `lp()` (§6.3), unless the owning chapter gives another
  framing and says why it is unambiguous.

### 7.4 Checksum ranges

Every checksum or digest field's owning chapter states the byte range it covers (by offsets, or by the fields it
spans), its seed where it has one, and what a mismatch means for that structure. There is no implied default range. A
checksum field is never inside its own range.

### 7.5 Git object-format registry

Every field named `algo` that says which hash function made a git object id or an `oid` ([AR §4.2] `image_cursor`,
[AR §4.3] `git` and `foreign_git`, [AR §4.1] `gitmap`, [40] R-1 `oid`) holds one value of this registry (type `u8`):

| value | name | digest | digest bytes |
|---|---|---|---|
| 0 | `none` | no object id | 0 |
| 1 | `sha1` | SHA-1 | 20 |
| 2 | `sha256` | SHA-256 | 32 |

- Values 3–255 are reserved and invalid in format v1.
- The names are git's `extensions.objectFormat` values ([git-hash-transition]). A text form (a canonical string, a
  trailer, the image) uses the name, never the number.
- **Variable-width form.** Where an id is encoded with its own length, it is the `algo` byte followed by exactly the
  digest bytes of that algorithm. The design's "oid-or-empty" ([AR §4.3], [40 §2.2]) is `algo` = 0 with no digest
  bytes.
- **Fixed 32-byte slot.** Where a structure reserves 32 bytes for an id whose algorithm is given by an `algo` field
  (the design's `[32]` digests):

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 20 | `b20` | `digest_lo` | `sha1`: the SHA-1 digest. `sha256`: the first 20 bytes of the SHA-256 digest. `none`: zero |
| 20 | 12 | `[12]u8` | `digest_hi` | `sha256`: the last 12 bytes of the SHA-256 digest. `sha1` and `none`: zero, reserved-zero (§10) |
| total | 32 | | | |

- A hexadecimal rendering of an id covers its digest bytes only: 40 digits for `sha1`, 64 for `sha256`.

## 8. Symbols and the symbol-width rule

### 8.1 The rule

A **symbol** is an interned string held in the store-wide symbol table (`SYMTAB`, [AR §4.4], [F09]; new symbols of the
log tail, [F05]). A record refers to a symbol by a numeric **symbol id**. Symbols are store-local: symbol ids are never
hashed and never exported ([AR §4.6] "symbol numbers"); canonical forms and the image carry the string.

- **S1. Classes.** Every field that refers to a symbol belongs to exactly one **symbol class** (§8.2). Each class has
  its own id space.
- **S2. Ids.** In every class, id 0 is the empty string and is never stored in `SYMTAB`. Ids from 1 upward are
  allocated densely in first-use order and are never reused, renumbered or collected ([AR §2.12]).
- **S3. One width per class.** Each class has one width W, `u16` or `u32`. Every reference to the class, in every
  structure, uses W: W bytes little-endian in a fixed-width position, and a varint bounded by W (`uvar16` or `uvar32`,
  §5.2) in a varint position. No structure refers to a class at another width.
- **S4. Exhaustion.** A write that would need an id above W's maximum is refused, with the error code of [F19]. An id is
  never wrapped or truncated.
- **S5. Widths.** W is `u32` for every class except the classes §8.2 marks `u16`: those whose fixed-width fields the
  design sizes at two bytes and whose vocabularies are small and closed in practice.

### 8.2 Classes

| Class | W | Fields of the design that refer to it |
|---|---|---|
| `actor` | `u32` | commit header `actor` ([AR §4.3]); `CREATOR.actor` ([AR §3.1], [50] F4) |
| `role` | `u16` | commit header `role`; `CREATOR.role`; `LEASES.role` ([AR §4.4], [90 §10.1]) |
| `session` | `u32` | commit header `session` |
| `ref` | `u32` | commit header `ref` (the branch the commit lands on); `IDEM.branch_sym` ([AR §4.4]) |
| `git-branch` | `u32` | commit header `git.branch` |
| `git-worktree` | `u32` | commit header `git.worktree` |
| `stmt` | `u32` | commit header `stmt_sym` ([50] F10) |
| `root` | `u16` | the root of a `path` value ([40] R-1) |
| `reason` | `u32` | `TOMB.reason_sym` ([AR §4.4]) |
| `name` | `u32` | schema names: field names (`field_sym` of the field block, [AR §3.1]), kind and edge-kind names, `lq_name` and `reverse_names` ([50] F1); [F08] binds the fields |
| `text` | `u32` | interned text values ("strings are interned symbols", [AR §3.1]); [F08] says which strings are interned |

The owning chapters ([F06], [F08], [F09], [F11]) bind each of their symbol fields to one class of this table. A chapter
may add a class before the freeze; its width is `u32` unless S5's condition holds and the review agrees.

### 8.3 Relation to the widths written in the design

- [AR §4.3] lists the commit header's `ref`, `git.branch` and `git.worktree` as `u16`. Under S3 and S5 they are `u32`
  class references. The commit header encodes every integer except `hlc` as a varint ([AR §4.3]), so the change moves
  no byte of any layout; it raises the bound from 65,535 to 2^32 − 1. Ref names, git branch names and worktree paths
  grow with every lane (open point 1).
- Every fixed-width symbol field keeps the width the design gives it: `CREATOR` = (`actor` u32, `role` u16), 6 bytes per
  node ([50] F4); `LEASES.role` u16; the `path` root u16 ([40] R-1). The row sizes [AR §4.4] states for `TOMB`
  (16 bytes: `id`, `tx`, `reason_sym`, `replaced_by`) and `IDEM` (40 bytes: two 16-byte hashes, `branch_sym`, a result
  reference) leave 4 bytes for each symbol id, which is the `u32` width of `reason` and `ref`.
- *(Informative)* Why classes. With one shared id space, ≈ 16,000 new actor symbols a year ([71 RAM-m6]) would push
  every later role, root or ref name past 65,535, where a two-byte field could no longer refer to it. Separate id
  spaces keep each class within its own width.

## 9. Versions

### 9.1 Format version fields

- Every structure that begins a file or a `HEAD` slot and carries a field named `format` holds a `u16` **format version**
  there: the `HEAD` slot, the `LOCK` header and the segment header ([AR §4.2], [80] X-F1, [AR §4.4]), and any other
  file header whose chapter gives it one. A structure without its own version field (a log record, a section, a table
  row) has the version of the store, which is `HEAD.format`.
- Format version 1 is the format frozen at M0 exit and tagged `format-v1` ([60 §2.5], WP-81b). The value 0 is never
  valid.
- All files of one store carry the same format version. A store whose files carry different versions is invalid.
- A reader of format 1 that finds:
  - version 1: reads the structure;
  - a version above 1: refuses the store with exit 7 ([F19]), naming the file and both versions. It never reads, never
    writes and never migrates the store: nothing auto-migrates on open ([60 §2.5], [AR §12]). A later version is
    adopted only through an explicit upgrade procedure ([60 §6] RG10);
  - version 0: the structure is invalid, as on a checksum mismatch (§7.4).

### 9.2 Other version numbers

These are not format versions; each has its owning chapter and its own comparison rule.

| Version | Where | Owner |
|---|---|---|
| schema version | canonical-form item 7 ([AR §4.6]) | [F07], [F08] |
| resolver version | R-14's constant table; `ANCHORRES` keys; `FILEOBS.resolver_version` ([40 §2.6]) | [F20], [F11] |
| tokenizer version byte | segment header ([50] F12) | [F09] |
| LQ grammar version (`u16`) | `QUERIES` items ([50] F3) | [F08], `[LQ/...]` |
| image format version | the `.moirai-image` marker and the `.moi` codec ([AR §5b.3]) | [F14] |
| output envelope version | `"v":1` of `--json v1` ([AR §7.1]) | [F19] |
| record versions inside `LOCK` | `SlotRec.ver` ([80] X-F1) | [F03] |

### 9.3 Unknown values in summary

- An enumeration value, OS tag, object-format value or record kind not listed for format v1 is invalid.
- A segment section whose tag a reader does not know is ignored only if its entry carries the `derived-optional` flag
  ([AR §4.4], [F09]); otherwise the segment is invalid.
- A reserved byte or bit that is not zero makes its structure invalid (§10).

## 10. Reserved and padding bytes

1. Every byte of a fixed-size structure is named in its offset table. Bytes that carry no field are `_reserved` or
   `_pad` rows (§2.6).
2. Writers write zero into every reserved or padding byte, every unnamed bit of a flags field, and every unused entry
   of a fixed-length array (for example `SegRef` entries beyond `n_segments`, [F04]), unless the owning chapter defines
   another fill.
3. A non-zero reserved byte or bit is a format violation: the structure is invalid, with the consequence the owning
   chapter states for an invalid structure. The owning chapter names the runtime validations that test reserved bytes
   (open point 6). The format oracle ([PLAN §3.2] WP-95) and `doctor --fsck` test every reserved byte of every
   structure they decode.
4. A chapter may mark a field "reserved, ignored on read" explicitly. Only such a field may be non-zero without making
   its structure invalid, and format v1 writers still write zero there.
5. Reserved space is for later format versions. A format-v1 writer never uses it.

## Coverage

| Item | Part covered here | Section |
|---|---|---|
| [60 §2.5] preamble: "format version 1; readers refuse a newer version; nothing auto-migrates on open" | complete | §9.1 |
| [60 §2.5] Cross-platform row (X-F1–X-F12, owner decision #32) | the X1 basis only: little-endian, 64-bit, page-size-free, OS kind tags; the X-F items are specified in their chapters | §3, §4.1–§4.2 |
| X1–X9 ([80 §1]) | what each principle fixes in the specification; the OS kind tag registry | §3.1, §3.2 |
| [40] R-1 | the varint-length string encoding of `path` values (`vstr`); the `oid` form `algo u8` + 20 or 32 bytes and the `algo` values; the `root` symbol class at `u16`. The value layouts are [F08]'s | §6.2, §7.5, §8.2 |
| [40] R-3 | `lp()` of the length-prefixed derivations and the BLAKE3-128 they use. The derivations are [F08]'s | §6.3, §7.1 |
| [50] F3 | the hash and hex rendering of `q` = hex(BLAKE3-256(name))[0..32]; the canonical-AST hash is BLAKE3-128. The file name and item layout are [F14]'s and [F08]'s | §6.4, §7.1 |
| [50] F4 | the symbol widths of `CREATOR` (`actor` u32, `role` u16). The column is [F09]'s | §8.2, §8.3 |
| [50] F10 | the `stmt` symbol class of `stmt_sym` (u32). The header is [F06]'s | §8.2 |
| [80] X-F9 | the hex and BLAKE3-256 primitives of hashed named-query file names. The name rule is [F14]'s | §6.4, §7.1 |
| [80] X-F2 | the BLAKE3-128 primitive of session hashes; the OS kind tag of `ProcId` and anchors. The layouts are [F03]'s | §3.2, §7.1 |
| [80] X-F3 | the XXH3-64 definition, the seeded form and the epoch-seed input bytes of the chain. The protocol and trailer are [F05]'s and [F16]'s | §7.1, §7.3 |
| [90 §10.1] holder anchor (X-F2 amendment) | the BLAKE3-128 of the namespaced identity: function and storage. The anchor layout is [F03]'s | §7.1, §7.2 |
| [90 §10.1] `LEASES` runtime rows | the `role` symbol class (u16). The row is [F11]'s | §8.2 |

## Holes

None. This chapter fixes no value that an M0 measurement or benchmark decides.

## Open points for the review

1. **The symbol-width rule** (gap of [PLAN §3.3], WP-10). Resolution: symbol classes with separate id spaces, one width
   per class, `u32` except `role` and `root` (§8). Conflict recorded: [AR §4.3] writes `ref u16`, `git.branch u16` and
   `git.worktree u16`; this chapter makes them `u32` class references. Because the commit header is varint-encoded, no
   layout changes. Consequences for other chapters: `SYMTAB` must carry the class of each entry and support lookups by
   (class, id) and by (class, string) ([F09], WP-13); [F06], [F08] and [F11] bind their fields to classes; [F19] needs
   one error code for an exhausted class. Alternative considered and rejected: one shared `u32` id space with every
   symbol field widened to `u32`. It would contradict [40] R-1's `u16` root, which [40] owns, and cost 2 bytes per node
   in `CREATOR`.
2. **128-bit identifiers are byte strings** (§5.6). [AR §3.1] and [AR §4.4] write `uid: u128`. This chapter makes every
   uid, store id and 16-byte digest a `b16`, compared bytewise, so that binary sort order equals hex sort order.
   This affects the `UID` section order ([F09]), the uid order of canonical item 10 ([F07]) and the `.moi` fan-out
   ([F14]).
3. **The git object-format registry** (§7.5). The design names an `algo u8` in several structures but no values. This
   chapter fixes 0 `none`, 1 `sha1`, 2 `sha256`, the variable-width form (algo 0 = "oid-or-empty") and the fixed 32-byte
   slot with SHA-1 zero-padded at the end. It affects [F04] (`image_cursor`), [F06] (`git`, `foreign_git`), [F08]
   (`oid`, `pathmove`), [F10] (`gitmap`) and [F14].
4. **The OS kind tag registry** (§3.2). X1 requires tags; the design gives no values. `[OS/proc §2]` (WP-17) fixed
   0 `unspecified` (uninterpretable), 1 `windows`, 2 `linux`, 3 `macos`; this chapter restates exactly those values and
   meanings as the convention for every chapter. It affects [F03] (`SlotRec.os`, `Anchor.os`, the embedded `ProcId`) and
   [F11] (`OsFileId`, `TREES`, `JOURNALCUR`). The review should keep one owner for the table; this chapter proposes
   [F01], with `[OS/proc]` citing it.
5. **XXH3 storage order** (§7.2). XXH3-64 and XXH3-128 values are stored little-endian as integers, not in xxHash's
   big-endian canonical representation. It affects [F03], [F04], [F05] and [F09].
6. **Reserved bytes are checked** (§10). A non-zero reserved byte makes the structure invalid; a field may be declared
   "reserved, ignored on read" explicitly. Proposal for the owning chapters: `HEAD`-slot validation ([F04]) and
   log-record validation ([F05]) test the reserved bytes and bits of their own headers, because that costs nothing on
   those paths. Mapped column rows are tested by the oracle and `doctor --fsck` only, since testing them on every read
   would cost scan time.
7. **Canonical varints** (§5.2). Non-minimal encodings are refused, so that decoding and re-encoding are byte-identical
   (E3) and a canonical input has one encoding.
8. **`f64` canonical rules** are left to [F07] and [F08] (§5.5). Proposal: refuse NaN at write time (exit 2), and store
   −0.0 as +0.0 in every value that enters a canonical form, so that equal values hash equally.
9. **`lp()` over typed arguments** (§6.3). [40 §2.7] computes `captured` over `lp(kind)` and `lp(occurrence)`. This
   chapter fixes integers as little-endian bytes of their declared width and leaves the other types to [F08] and [F20].
   Proposal: an enumeration enters as its frozen name string, not its number, so the derivation does not depend on
   enumeration numbering.
10. **Hand-editable text files** (§6.7). The BOM and CR LF tolerance on read follows [80 §4.2] T6 and [80 §3.2]. [CFG]
    (WP-18) is asked to cite §6.7 for the store and user configuration files, so that both files and pointer files
    share one reading rule.
11. **`fstr<N>`** (§6.2) is offered for bounded text in fixed structures, such as `WriterDiag.cmd` (≤ 400 B, [80] X-F1)
    in [F03] (WP-11). WP-11 decides whether to use it.
12. **The HLC layout** (§5.7): the 48 + 16 bit split follows [AR §4.3]'s "ms << 16 | counter". The unit of a foreign
    commit's `hlc` (seconds versus milliseconds, [AR §5b.4] "`committer_time << 16`") stays WP-12's gap, and [F06]
    resolves it.
13. **Source of X1–X9.** The work package cites "X1–X9 of [AR §4]". The principles are [80 §1]'s and are summarised in
    [AR §14]; this chapter cites [80 §1] as their source.
14. **Format-version refusal** (§9.1): a newer version is exit 7, "store unavailable". Pass 1 should confirm that exit 7
    is the intended code; [F19] owns the text.
15. **Hole ids across parts** (§2.5). The format chapters written so far use `F<NN>-<name>` ([F15], [F17], [F02]). Some
    `[OS/...]` and `[LQ/...]` files use `os-<name>` or an unprefixed name (`HOLE(os-win-boot-source)`,
    `HOLE(share-retry-ms)`, `HOLE(display-spelling)`). Proposal for the review: those files rename their ids to
    `OS-<name>` and `LQ-<name>` before WP-81a, so that one `xtask` check can find every hole by its prefix and prove the
    ids unique.
