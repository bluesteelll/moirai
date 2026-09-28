# Unicode Character Database 17.0.0: the pinned inputs of `fold_v1`

- **What.** The Unicode 17.0.0 data files from which `cargo xtask ucd` generates the `fold_v1` tables of `moirai-files`
  (`crates/moirai-files/src/fold/tables.rs`) and against which those tables are tested ([F20 §3.1, §3.2]; PLAN §2.4,
  §6.2 R6; WP-61). The reference model derives `fold_v1` from the same two normative files by its own algorithm
  (PLAN §3.2 item 9, WP-92).
- **Source.** `https://www.unicode.org/Public/17.0.0/ucd/<file>`, downloaded on 2026-09-28. The files are unmodified: byte
  for byte as published, LF line endings, stored with `-text` (`.gitattributes`: `fixtures/** -text`).
- **Licence.** Copyright © Unicode, Inc. The files are distributed under the Unicode License v3 (SPDX `Unicode-3.0`),
  whose text is in [`LICENSES/Unicode-3.0.txt`](../../../LICENSES/Unicode-3.0.txt); NOTICE carries the attribution.
- **Pins.** Each file is pinned by its size and SHA-256. `crates/moirai-files/tests/fold_ucd.rs` reads the table below and
  refuses a file whose size or digest differs, before it checks the tables against the files. `cargo xtask ucd` reads
  the same table and refuses an input whose size differs, or a `CaseFolding.txt` whose first line names another
  version, before it generates anything. Replacing a file is a new Unicode version: a new directory, a new fold function
  and a new resolver version ([F20 §3.1]), never an edit here.

| File | Bytes | SHA-256 | Role |
|---|---|---|---|
| `UnicodeData.txt` | 2198209 | `2e1efc1dcb59c575eedf5ccae60f95229f706ee6d031835247d843c11d96470c` | normative: field 3 (`Canonical_Combining_Class`) and field 5 (canonical decomposition mappings; `<tag>` mappings are compatibility mappings and are never applied) |
| `CaseFolding.txt` | 87539 | `ff8d8fefbf123574205085d6714c36149eb946d717a0c585c27f0f4ef58c4183` | normative: the lines with status `C` or `F` (full case folding); `S` and `T` lines are ignored |
| `NormalizationTest.txt` | 2827429 | `5019ffd530751a741900c849c0e010332f142a3612234639bd200b82138a87db` | conformance: the NFD column of every line, and NFD(X) = X for every scalar value not in part 1 |

The file headers give the publication dates: `CaseFolding-17.0.0.txt` 2025-07-30, `NormalizationTest-17.0.0.txt`
2025-06-30; the UCD `ReadMe.txt` of the directory is dated 2025-08-15 ("final data files for the Unicode Character
Database, for Version 17.0.0").
