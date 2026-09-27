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
  WP-81a fills every hole before the freeze ([F01 §2.5]).
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
| format/03-lock.md | [F03] | `LOCK` layout v1 (X-F1), `LockHdr`, `SlotRec`, `ProcId`, `Anchor` (X-F2 and its [90 §10.1] amendment) | WP-11 (R-SPEC-P) | planned |
| format/04-head.md | [F04] | `HEAD` slots, R-6's `next_anchor`, the parameters fixed at `init` | WP-11 (R-SPEC-P) | planned |
| format/05-log.md | [F05] | Log extents, `RecHdr`, groups and the chain (X-F3), rotation padding, every record kind with its payload and durability class | WP-11 (R-SPEC-P) | planned |
| format/06-commit.md | [F06] | The commit body, ops with before-images, the closed value set, bulk commits, the unhashed header fields (F10, F14, F16, `actor_src`) | WP-12 (R-SPEC-F) | planned |
| format/07-canonical-form.md | [F07] | Canonical items 1–10, `changeset_digest`, the R-10 anchor selector block, commit kinds, message normalisation | WP-12 (R-SPEC-F) | planned |
| format/08-data-model.md | [F08] | `NodeHdr`, cold columns, the field block, the 13 kinds and 25 edge kinds, schema as data (F1–F3), R-2, R-3, R-5, uid derivations | WP-14 (R-SPEC-F) | planned |
| format/09-segments.md | [F09] | `SegHdr` and every segment section (R-8, F4–F7, F11–F13), frozen bitsets, delta and branch segments | WP-13 (R-SPEC-R) | planned |
| format/10-sealed-files.md | [F10] | `hist` (F8), `blobs` (R-9), `dict`, `gitmap`, `cs` frames; codec bytes and body-compression placement | WP-13 (R-SPEC-R) | planned |
| format/11-runtime-tables.md | [F11] | `REFS`, `PINS`, `HEADS`, `LEASES`, `MARKERS`, `IDEM`, `FILEOBS` (R-18), `OsFileId`, `JOURNALCUR`, `DIRMAP`, `TREES`/`VolumeCaps`, `ALLOC` (F17)/`UIDX`, `CONFLICTS` | WP-13 (R-SPEC-R) | planned |
| format/12-vcs.md | [F12] | The recursive-virtual-base addendum, ref names, revisions, the conflict-class enumeration | WP-12 (R-SPEC-F) | planned |
| [format/13-invariants.md](format/13-invariants.md) | [F13] | Every invariant with its enforcement point, model function and gate; I-F1…I-F14 cross-listed | WP-16 (R-SPEC-P), with WP-14 | draft, pass 1 pending |
| format/14-image.md | [F14] | The git image: tree, `.moirai-image`, the `.moi` v1 ABNF (R-11, F3), side ref, commit mapping, trailers, the gate-0 carrier table | WP-15 (R-SPEC-R) | planned |
| [format/15-fault-model.md](format/15-fault-model.md) | [F15] | The `Vfs` fault model, items 1–12, and the durability classes | WP-16 (R-SPEC-P) | draft, pass 1 pending |
| format/16-protocol.md | [F16] | Durability class per protocol point, protocol decisions (a)–(m), group commit, I-G1–I-G6, barrier, boot recovery, the three-phase write | WP-16 (R-SPEC-P) | planned |
| [format/17-store-parameters.md](format/17-store-parameters.md) | [F17] | Every threshold, fixed at `init` or tunable, with production values as holes and the test profile; the `InitParams` block | WP-16 (R-SPEC-P) | draft, pass 1 pending |
| format/18-file-links.md | [F18] | R-12 (I-F1…I-F14), R-15, R-16, R-17 | WP-14 (R-SPEC-F) | planned |
| format/19-errors-and-output.md | [F19] | Exit codes 0–10, the v1 envelope, byte-unit and output rules, error codes and refusal texts | WP-18 (R-SPEC-F) | planned |
| format/20-r4-resolver-constants.md | [F20] | R-14's appendix: `is_text`, EOL, `oid`, `fold_v1`, window and sketch hashes, winnowing, thresholds, the never-candidate list, the per-OS rules | WP-14b (R-SPEC-R) | planned |

### OS layer — `os/`

Each file states its contract once and maps it to Windows, Linux and macOS in its own appendix. `[OS/README §1.3]` lists
the files of WP-17 parts 1 and 2.

| File | Cite | Content | WP (role) | Status |
|---|---|---|---|---|
| [os/README.md](os/README.md) | [OS/README] | Module map; the `Vfs`, `ProjectFs` and `Meter` seams; crate placement | WP-17a (R-SPEC-P) | draft, pass 1 pending |
| [os/fs.md](os/fs.md) | [OS/fs] | `os::fs`: store files, durability classes (X-F5), renames, `swap_dirs` and the swap intent | WP-17a (R-SPEC-P) | draft, pass 1 pending |
| [os/lock.md](os/lock.md) | [OS/lock] | `os::lock`: lock bytes, the X-F4 contract, the grant table, the lock order | WP-17a (R-SPEC-P) | draft, pass 1 pending |
| [os/map.md](os/map.md) | [OS/map] | `os::map`: sealed-file mappings, the `total_len` check, the fault handler (X-F6) | WP-17a (R-SPEC-P) | draft, pass 1 pending |
| os/env.md | [OS/env] | `os::env`: the environment guard, the `init` probe, the OS-version check (X-F6) | WP-17a (R-SPEC-P) | planned |
| [os/proc.md](os/proc.md) | [OS/proc] | `os::proc`: OS tags, `ProcId`, boot identity, Unknown-boot mode, liveness, parent watch (X-F2) | WP-17b (R-SPEC-P) | draft, pass 1 pending |
| [os/clock.md](os/clock.md) | [OS/clock] | The wall, monotonic and boot clocks; lease deadlines | WP-17b (R-SPEC-P) | draft, pass 1 pending |
| [os/mem.md](os/mem.md) | [OS/mem] | `os::mem`: private bytes, available memory, child peaks, `CountingAlloc`, the `Meter` implementation | WP-17b (R-SPEC-P) | draft, pass 1 pending |
| [os/path.md](os/path.md) | [OS/path] | `os::path`: P1–P12 (X-F7, X-F9), canonical roots, the CLI boundary, the user-scope configuration path | WP-17b (R-SPEC-P) | draft, pass 1 pending |
| os/project.md | [OS/project] | `os::project`: the complete `ProjectFs`, R4 file identity and change tracking (X-F8) | WP-17b (R-SPEC-P) | planned |
| os/ipc.md | [OS/ipc] | `os::ipc`: the leader's endpoint | WP-17b (R-SPEC-P) | planned |
| os/spawn.md | [OS/spawn] | `os::spawn`: the detached `moirai gc` child | WP-17b (R-SPEC-P) | planned |
| os/term.md | [OS/term] | `os::term`: console and pipe output | WP-17b (R-SPEC-P) | planned |
| os/test-host.md | [OS/test-host] | `os::test_host`: kill, suspend, resume, small volumes, clock offset | WP-17b (R-SPEC-P) | planned |
| os/shell.md | [OS/shell] | The shell transport rules T1–T10 (X-F12) | WP-17b (R-SPEC-P) | planned |

### Query language — `lq/`

The query surface is frozen only after GT13 (WP-72).

| File | Cite | Content | WP (role) | Status |
|---|---|---|---|---|
| lq/grammar-v1.ebnf | [LQ/grammar-v1.ebnf] | Grammar v1 with its requirement trace; lexical rules | WP-19 (R-SPEC-F) | planned |
| [lq/errors.md](lq/errors.md) | [LQ/errors] | The error, warning and notice table with texts (≤ 600 B, ASCII) | WP-19 (R-SPEC-F) | draft, pass 1 pending |
| [lq/envelope.md](lq/envelope.md) | [LQ/envelope] | The envelope and result shapes, reading echo and footers | WP-19 (R-SPEC-F) | draft, pass 1 pending |
| lq/std.md | [LQ/std] | Standard-library and `tx` signatures; the LQ text of the standard queries and pack and brief classes M0 needs | WP-19 (R-SPEC-F) | planned |
| lq/canonical-ast.md | [LQ/canonical-ast] | The canonical-AST encoding | WP-19 (R-SPEC-F) | planned |
| lq/json-ir.md | [LQ/json-ir] | The JSON IR schema | WP-19 (R-SPEC-F) | planned |
| lq/card.md | [LQ/card] | The card draft and the strict-GQL spelling table | WP-19 (R-SPEC-F) | planned |

### Store API, configuration and measurement protocol

| File | Cite | Content | WP (role) | Status |
|---|---|---|---|---|
| store-api.md | [API] | The logical `Store` API: typed commands and results, `state(ref)` and its digest, the deterministic clock, fields excluded from comparison | WP-25 (R-SPEC-F) | planned |
| store-api/examples/*.json | [API] | Example commands and results in the `--json v1` data shape | WP-25 (R-SPEC-F) | planned |
| config.md | [CFG] | Configuration syntax, precedence, the unknown-key rule, the registry format, `HEAD.config_gen`, the key registry | WP-18 (R-SPEC-F) | planned |
| measurement-protocol.md | — | The measurement protocol that freezes [60 §5.1] | WP-50 (R-HARN-I) | planned |

### Rules — `rules/`

The reference model's rule tables, published as documents for the owner's signature (V3). `[RULES/README]` is the
contract for every rule file.

| File | Cite | Content | WP (role) | Status |
|---|---|---|---|---|
| [rules/README.md](rules/README.md) | [RULES/README] | The rule-file format, the parser contract, signing, the table registry | WP-90 (R-MODEL) | draft, pass 1 pending |
| [rules/merge-table.md](rules/merge-table.md) | [RULES/merge-table] | The typed merge table | WP-90, WP-91 (R-MODEL) | draft, pass 1 pending |
| rules/link-merge-rules.md | [RULES/link-merge-rules] | R4's link merge rules | WP-92 (R-MODEL) | planned |
| rules/status-machines.md | [RULES/status-machines] | The status machines | WP-90 (R-MODEL) | planned |
| rules/delete-policy.md | [RULES/delete-policy] | The delete-policy matrix | WP-90 (R-MODEL) | planned |
| rules/pack-classes.md | [RULES/pack-classes] | The context-pack classes | WP-90 (R-MODEL) | planned |
| rules/i26-state.md | [RULES/i26-state] | The I26′ state definition with its marker-cache rules | WP-90 (R-MODEL) | planned |
| rules/policy-keys.md | [RULES/policy-keys] | Every operational-policy key and policy-data row mapped to a model function | WP-90 (R-MODEL) | planned |
| rules/SIGNED.md | [RULES/SIGNED] | The owner-committed BLAKE3 digest of each table | owner (V3) | planned |

### Coverage and reviews

| File | Cite | Content | WP (role) | Status |
|---|---|---|---|---|
| COVERAGE.md | — | One row per [60 §2.5] row, R-1…R-18, F1–F18, X-F1–X-F12 and [90 §10.1] item: chapter section, fixture, model function | WP-10 (R-SPEC-F) skeleton; rows from every chapter's WP | planned |
| [reviews/a1-P.md](reviews/a1-P.md) | — | Phase-0 re-review of [40] and [50] revision 2 and the [PLAN §6.2] confirmations, lens P (performance, RAM, Windows) | WP-80a (R-REV-P) | review |
| [reviews/a1-S.md](reviews/a1-S.md) | — | The same, lens S (semantics and correctness) | WP-80a (R-REV-S) | review |
| [reviews/a1-A.md](reviews/a1-A.md) | — | The same, lens A (agent fit, tokens and buildability) | WP-80a (R-REV-A) | review |
| reviews/&lt;lens&gt;-&lt;pass&gt;.md | — | Review passes 1 and 2, findings with severity and disposition | WP-80 (R-REV-P, -S, -A) | planned |
