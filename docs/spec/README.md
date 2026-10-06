# moirai specification

This directory holds moirai's specification: the on-disk format v1, the OS layer, the query-language contract, the
logical `Store` API, the configuration system and the measurement protocol. Milestone M0 writes it, reviews it, fills
its named holes from the M0 measurements, and freezes it with the tag `format-v1` ([60 §3.1], [PLAN §3.2] item 1 and
item 8).

The design documents are normative: [AR] `docs/ARCHITECTURE-RESEARCH.md` with [40], [50], [60], [80] and [90] in
`docs/research/design/`. The specification states them at byte level and never changes them; a disagreement is a review
finding ([F01 §2.4]). The execution plan is `docs/m0/PLAN.md`.

## Conventions in brief

[F01] holds the full rules. In short:
- **Citations.** Format chapters are cited `[FNN §x.y]`. The other parts are cited `[OS/<file> §x]`, `[LQ/<file> §x]`,
  `[API §x]`, `[CFG §x]` and `[RULES/<file>]`. Design documents are cited `[AR §x]`, `[40 §x]`, `[50 §x]`, `[60 §x]`,
  `[80 §x]` and `[90 §x]` ([F01 §2.2]).
- **Chapter structure.** Every chapter opens with a header block (title, chapter, status, work package, sources,
  dependencies) and ends with Coverage, Holes, and Open points for the review ([F01 §2.3]).
- **Named holes.** A value an M0 measurement decides is written `HOLE(<id>)` and listed in its chapter's Holes table;
  WP-81a fills every hole before the freeze ([F01 §2.5]). [HOLES.md](HOLES.md) indexes every hole of every part.
- **Coverage.** [COVERAGE.md](COVERAGE.md) maps every row of [60 §2.5], R-1…R-18, F1–F18, X-F1–X-F12 and every
  [90 §10.1] item to the sections that specify it, with the fixture and model-function columns that `xtask coverage`
  checks ([F01 §2.7]).
- **Layouts.** Every fixed-size structure has an offset table `offset | width | type | name | meaning`, little-endian,
  with every padding and reserved byte explicit ([F01 §2.6], [F01 §10]).

## Status legend

The header block of each file is authoritative for its status; this index records the status at its last edit.

| Status | Meaning |
|---|---|
| planned | not yet written |
| draft, pass 1 pending | written; waiting for review pass 1 (WP-80) |
| pass 1 closed | review pass 1 left no open blocker or major finding |
| pass 2 closed | the integrated review pass 2 left no open blocker or major finding |
| frozen (format-v1) | part of the tag `format-v1` (WP-81b) |
| review | a review file of WP-80a or WP-80; its own header names the pass |

## Index

### Format — `format/`

| File | Cite | Content | WP (role) | Status |
|---|---|---|---|---|
| [format/01-conventions.md](format/01-conventions.md) | [F01] | Conventions: citation form, chapter structure, holes, offset tables; X1–X9; byte order; integer, varint, string and `lp()` encodings; the hash set; the symbol-width rule; version fields; reserved bytes | WP-10 (R-SPEC-F) | draft, pass 1 pending |
| [format/02-store-layout.md](format/02-store-layout.md) | [F02] | Store layout: placement, discovery, the pointer file, the store id, directory contents, store file names (X-F10), user-scope configuration locations (X-F11) | WP-10 (R-SPEC-F) | draft, pass 1 pending |
| [format/03-lock.md](format/03-lock.md) | [F03] | `LOCK` layout v1 (X-F1), `LockHdr`, `WriterDiag`, `LeaderRec`, `SlotRec`, `ProcId`, `Anchor` (X-F2 and its [90 §10.1] amendment) | WP-11 (R-SPEC-P) | draft, pass 1 pending |
| [format/04-head.md](format/04-head.md) | [F04] | `HEAD` slots, R-6's `next_anchor`, the audit fields, the `InitParams` block's place, `project_oid_algo`, the HLC maxima `hlc_seq` and `hlc_commit`, slot choice, the publish and boot-change recovery (slot side) | WP-11 (R-SPEC-P) | draft, pass 1 pending |
| [format/05-log.md](format/05-log.md) | [F05] | Log extents, `RecHdr`, groups and the chain (X-F3), rotation padding, the scan, every record kind (1–30, with 27 `Reserve`, the extent head 28 `ExtentHead`, 29 `BodyDrop` and the reserved 30 `Harvest`) with its payload and durability class, replay | WP-11 (R-SPEC-P) | draft, pass 1 pending |
| [format/06-commit.md](format/06-commit.md) | [F06] | The commit body with its presence bitmap, ops with before-images and `prev`, the values inside commits (their bytes are [F08]'s), bulk commits, the header-only form, an import-checkpoint's image-only data, the unhashed header fields (F10, F14, F16, `actor_src`) | WP-12a (R-SPEC-F) | draft, pass 1 pending |
| [format/07-canonical-form.md](format/07-canonical-form.md) | [F07] | Canonical items 1–10, `changeset_digest`, the R-10 anchor selector block, commit kinds, message normalisation, the gate-0 carrier stub | WP-12b (R-SPEC-F) | draft, pass 1 pending |
| [format/08-data-model.md](format/08-data-model.md) | [F08] | `NodeHdr`, cold columns, the one value registry and every value's bytes, the field block, schema as data (F1–F3), the core schema, edges, the edge property block and the anchor record, R-2, R-3, R-5, uid derivations | WP-14a (R-SPEC-F) | draft, pass 1 pending |
| [format/09-segments.md](format/09-segments.md) | [F09] | `SegHdr`, the tag registry and every segment section (R-8, F4–F7, F11–F13), frozen bitsets, base, delta, branch and changeset segments | WP-13a (R-SPEC-R) | draft, pass 1 pending |
| [format/10-sealed-files.md](format/10-sealed-files.md) | [F10] | `hist` (F8), `blobs` (R-9), `dict`, `gitmap`; the lifetime of `cs` files (their layout is [F09 §16.4]'s); the codec byte and frame formats (holes of measurement 6) | WP-13a (R-SPEC-R) | draft, pass 1 pending |
| [format/11-runtime-tables.md](format/11-runtime-tables.md) | [F11] | `REFS`, `PINS`, `HEADS`, `LEASES`, `MARKERS`, `IDEM`, `ALLOC` (F17)/`UIDX`, `CONFLICTS`, `GLOBIDX`, the R4 runtime tables (`FILEOBS` (R-18), `OsFileId`, `VolumeCaps`, `TREES`, `JOURNALCUR`, `DIRMAP`, …), the session tables `CURSORS` (change-feed and pack cursors) and `SESSMARKS`, the `BACKUPS` registry, and the row images that log records carry | WP-13b (R-SPEC-R) | draft, pass 1 pending |
| [format/12-vcs.md](format/12-vcs.md) | [F12] | Ref names (X-F9 P11 (b)), revisions, the recursive-virtual-base addendum, the conflict-class enumeration, the typed three-way merge contract, staging refs | WP-12c (R-SPEC-F) | draft, pass 1 pending |
| [format/13-invariants.md](format/13-invariants.md) | [F13] | Every invariant with its enforcement point, model function and gate; I-F1…I-F14 cross-listed; I26′, the I37′ validator order, derived-state semantics (F15) | WP-16c (R-SPEC-P), with WP-14 | draft, pass 1 pending |
| [format/14-image.md](format/14-image.md) | [F14] | The git image: tree, `.moirai-image`, the `.moi` v1 ABNF (R-11, F3), commit mapping, trailers, the gate-0 carrier table, refs, destinations, side refs | WP-15 (R-SPEC-R) | draft, pass 1 pending |
| [format/15-fault-model.md](format/15-fault-model.md) | [F15] | The `Vfs` fault model, items 1–12, the durability classes and namespace operations, the crash-gate contract | WP-16a (R-SPEC-P) | draft, pass 1 pending |
| [format/16-protocol.md](format/16-protocol.md) | [F16] | Durability class per protocol point, protocol decisions (a)–(m), group commit, I-G1–I-G6, the chain rule, barrier, recovery, the three-phase write, the rules P-1–P-100, the seeded-bug catalogue | WP-16b (R-SPEC-P) | draft, pass 1 pending |
| [format/17-store-parameters.md](format/17-store-parameters.md) | [F17] | Every threshold, fixed at `init` or tunable, with production values as holes and the test profile; the `InitParams` block | WP-16c (R-SPEC-P) | draft, pass 1 pending |
| [format/18-file-links.md](format/18-file-links.md) | [F18] | R-12 (I-F1…I-F14), R-15, R-16, R-17 | WP-14 (R-SPEC-F) | draft, pass 1 pending |
| [format/19-errors-and-output.md](format/19-errors-and-output.md) | [F19] | Exit codes 0–10, the v1 envelope, byte-unit and output rules, error codes and refusal texts, the F18 violation classes and validator | WP-18b (R-SPEC-F) | draft, pass 1 pending |
| [format/20-r4-resolver-constants.md](format/20-r4-resolver-constants.md) | [F20] | R-14's appendix: `is_text`, EOL, `oid`, `fold_v1`, window and sketch hashes, winnowing, the git pair score, thresholds, the never-candidate list, the per-OS rules; the interim scanner rule (the scanners are [F21]'s) | WP-14b (R-SPEC-R) | draft, pass 1 pending |
| [format/21-scope-scanners.md](format/21-scope-scanners.md) | [F21] | the scope scanners of resolver version 1: the Rust, Markdown and TOML scanners as total algorithms, name paths, recordable scopes, the scope a capture records and how it resolves, the `symbol` and `heading` authoring forms and their outcomes, the scanner constants | WP-14b (R-SPEC-R) | draft, pass 2 pending |

### OS layer — `os/`

Each file states its contract once and maps it to Windows, Linux and macOS in its own appendix. `[OS/README §1.3]` lists
the files of WP-17 parts 1 and 2. `os::spawn`, `os::ipc` and `os::test_host` are specified in [OS/proc §11]–[OS/proc §13]
and `os::term` in [OS/shell §10]; they have no files of their own.

| File | Cite | Content | WP (role) | Status |
|---|---|---|---|---|
| [os/README.md](os/README.md) | [OS/README] | Module map; the `Vfs`, `ProjectFs` and `Meter` seams; the `Entropy` random source; crate placement | WP-17a (R-SPEC-P) | draft, pass 1 pending |
| [os/fs.md](os/fs.md) | [OS/fs] | `os::fs`: store files, durability classes (X-F5), renames, `swap_dirs` and the swap intent | WP-17a (R-SPEC-P) | draft, pass 1 pending |
| [os/lock.md](os/lock.md) | [OS/lock] | `os::lock`: lock bytes, the X-F4 contract, the grant table, the lock order | WP-17a (R-SPEC-P) | draft, pass 1 pending |
| [os/map.md](os/map.md) | [OS/map] | `os::map`: sealed-file mappings, the `total_len` check, the fault handler (X-F6) | WP-17a (R-SPEC-P) | draft, pass 1 pending |
| [os/env.md](os/env.md) | [OS/env] | `os::env`: the per-OS allow-lists, the classification at every open, the full probe at `init`/`restore`, the OS-version check (X-F6) | WP-17a (R-SPEC-P) | draft, pass 1 pending |
| [os/proc.md](os/proc.md) | [OS/proc] | `os::proc`: OS tags, `ProcId`, boot identity, Unknown-boot mode, liveness, parent watch (X-F2); `os::spawn`, `os::ipc`, `os::test_host` | WP-17b (R-SPEC-P) | draft, pass 1 pending |
| [os/clock.md](os/clock.md) | [OS/clock] | The wall, monotonic and boot clocks; stamps and lease deadlines; which rule uses which clock; the HLC | WP-17b (R-SPEC-P) | draft, pass 1 pending |
| [os/mem.md](os/mem.md) | [OS/mem] | `os::mem`: private bytes, available memory, child peaks, `CountingAlloc`, the `Meter` implementation | WP-17b (R-SPEC-P) | draft, pass 1 pending |
| [os/path.md](os/path.md) | [OS/path] | `os::path`: P1–P12 (X-F7, X-F9), canonical roots, the CLI boundary, the user-scope configuration path | WP-17b (R-SPEC-P) | draft, pass 1 pending |
| [os/project.md](os/project.md) | [OS/project] | `os::project`: the complete `ProjectFs`, R4 file identity, volume capabilities, timestamps and change tracking (X-F8) | WP-17b (R-SPEC-P) | draft, pass 1 pending |
| [os/shell.md](os/shell.md) | [OS/shell] | The shell transport rules T1–T10 (X-F12); the `os::term` contract | WP-17b (R-SPEC-P) | draft, pass 1 pending |
| [os/mapping-appendix.md](os/mapping-appendix.md) | [OS/mapping-appendix] | Part 2's per-OS mapping of every seam method to its calls, and an index of part 1's mappings | WP-17b (R-SPEC-P) | draft, pass 1 pending |

### Query language — `lq/`

The query surface is frozen only after GT13 (WP-72).

| File | Cite | Content | WP (role) | Status |
|---|---|---|---|---|
| [lq/grammar-v1.ebnf](lq/grammar-v1.ebnf) | [LQ/grammar-v1.ebnf] | Grammar v1 (54 + 15 + 13 productions) with its parser decisions, refused forms, the strict-GQL spelling mode and the requirement trace | WP-19a (R-SPEC-F) | draft, pass 1 pending |
| [lq/lexical.md](lq/lexical.md) | [LQ/lexical] | Lexical rules: source decoding, lexer modes, tokens and their limits, keyword sets, name matching, transport and stored-text checks | WP-19a (R-SPEC-F) | draft, pass 1 pending |
| [lq/canonical-ast.md](lq/canonical-ast.md) | [LQ/canonical-ast] | The S-AST and its fixture text form; the canonical AST, its encoding and the query hash; the values derived from the hash | WP-19a (R-SPEC-F) | draft, pass 1 pending |
| [lq/json-ir.md](lq/json-ir.md) | [LQ/json-ir] | The JSON IR: mapping rules, validation, diagnostics, its JSON Schema and output form | WP-19a (R-SPEC-F) | draft, pass 1 pending |
| [lq/errors.md](lq/errors.md) | [LQ/errors] | The error, warning and notice table with texts (≤ 600 B, ASCII) | WP-19b (R-SPEC-F) | draft, pass 1 pending |
| [lq/envelope.md](lq/envelope.md) | [LQ/envelope] | The envelope and result shapes, reading echo, footers, cursors and `TX` results | WP-19b (R-SPEC-F) | draft, pass 1 pending |
| [lq/std.md](lq/std.md) | [LQ/std] | Standard-library and `tx` signatures; the LQ text of the standard queries and pack and brief classes M0 needs | WP-19b (R-SPEC-F) | draft, pass 1 pending |
| [lq/card.md](lq/card.md) | [LQ/card] | The card draft: its text, size limits, display-spelling variants and measurement | WP-19b (R-SPEC-F) | draft, pass 1 pending |
| [lq/gql-spelling.md](lq/gql-spelling.md) | [LQ/gql-spelling] | The spelling table (Cypher and GQL forms), the strict-GQL spelling mode, the display spelling of quantifiers | WP-19b (R-SPEC-F) | draft, pass 1 pending |

### Store API, configuration and measurement protocol

| File | Cite | Content | WP (role) | Status |
|---|---|---|---|---|
| [store-api.md](store-api.md) | [API] | The logical `Store` API: typed commands and results, the caller context, the injected deterministic environment, idempotency, `state(ref)` and its digests, the model-engine comparison and its exclusions | WP-25 (R-SPEC-F) | draft, pass 1 pending |
| [store-api/examples/](store-api/examples/) (20 files) | [API §19] | Example commands and results in the `--json v1` data shape, at least one pair per command family | WP-25 (R-SPEC-F) | draft, pass 1 pending |
| [config.md](config.md) | [CFG] | Configuration syntax, precedence, the unknown-key rule, the registry format, `HEAD.config_gen`, the key registry | WP-18a (R-SPEC-F) | draft, pass 1 pending |
| measurement-protocol.md | — | The measurement protocol that freezes [60 §5.1] | WP-50 (R-HARN-I) | planned |

### Rules — `rules/`

The reference model's rule tables, published as documents for the owner's signature (V3). `[RULES/README]` is the
contract for every rule file.

| File | Cite | Content | WP (role) | Status |
|---|---|---|---|---|
| [rules/README.md](rules/README.md) | [RULES/README] | The rule-file format, the parser contract, signing, the table registry | WP-90 (R-MODEL) | draft, pass 1 pending |
| [rules/merge-table.md](rules/merge-table.md) | [RULES/merge-table] | The typed merge table: key class → merge rule → conflict class | WP-90, WP-91 (R-MODEL) | draft, pass 1 pending |
| [rules/link-merge-rules.md](rules/link-merge-rules.md) | [RULES/link-merge-rules] | R4's link merge rules | WP-92 (R-MODEL) | draft, pass 1 pending |
| [rules/status-machines.md](rules/status-machines.md) | [RULES/status-machines] | The status machines: states, transitions, guards, doors, derived effects | WP-90 (R-MODEL) | draft, pass 1 pending |
| [rules/delete-policy-matrix.md](rules/delete-policy-matrix.md) | [RULES/delete-policy-matrix] | The delete-policy matrix: edge kind × delete policy × effect on dependents | WP-90 (R-MODEL) | draft, pass 1 pending |
| [rules/pack-classes.md](rules/pack-classes.md) | [RULES/pack-classes] | The context-pack and brief classes, levels, budgets and the stale-pack notice | WP-90 (R-MODEL) | draft, pass 1 pending |
| [rules/state-definition.md](rules/state-definition.md) | [RULES/state-definition] | The I26′ state definition with its marker-cache rules; `ready`, `unblocked`, markers, absorbed vectors, leases | WP-90 (R-MODEL) | draft, pass 1 pending |
| [rules/role-write-policy.md](rules/role-write-policy.md) | [RULES/role-write-policy] | The role write policy: roles × verbs, kinds, fields and edges → allowed or refused | WP-90 (R-MODEL) | draft, pass 1 pending |
| rules/policy-keys.md | [RULES/policy-keys] | Every operational-policy key and policy-data row mapped to a model function | WP-90 (R-MODEL) | planned |
| rules/SIGNED.md | [RULES/SIGNED] | The owner-committed BLAKE3 digest of each table | owner (V3) | planned |

### Coverage, holes and reviews

| File | Cite | Content | WP (role) | Status |
|---|---|---|---|---|
| [COVERAGE.md](COVERAGE.md) | — | One row per [60 §2.5] row (multi-part rows split), R-1…R-18, F1–F18, X-F1–X-F12 and [90 §10.1] item: chapter sections, fixture, model function; 152 rows, none unmapped | WP-10 (R-SPEC-F) skeleton; rows from every chapter's WP, assembled by R-SPEC-F | draft, pass 1 pending |
| [HOLES.md](HOLES.md) | — | Every named hole of every part: where it sits, what decides it, its candidates; aliases; decided rule-file holes; measured decisions that are not holes | R-SPEC-F; filled by WP-81a | draft, pass 1 pending |
| [reviews/a1-P.md](reviews/a1-P.md) | — | Phase-0 re-review of [40] and [50] revision 2 and the [PLAN §6.2] confirmations, lens P (performance, RAM, Windows) | WP-80a (R-REV-P) | review |
| [reviews/a1-S.md](reviews/a1-S.md) | — | The same, lens S (semantics and correctness) | WP-80a (R-REV-S) | review |
| [reviews/a1-A.md](reviews/a1-A.md) | — | The same, lens A (agent fit, tokens and buildability) | WP-80a (R-REV-A) | review |
| [reviews/a1-dispositions.md](reviews/a1-dispositions.md) | — | Dispositions of the A1 re-review and the outcome of [PLAN §6.2]'s resolutions R1–R19; the owner signs them in WP-80 (V2) | WP-80a (R-SPEC) | review |
| reviews/&lt;lens&gt;-&lt;pass&gt;.md | — | Review passes 1 and 2, findings with severity and disposition | WP-80 (R-REV-P, -S, -A) | planned |
