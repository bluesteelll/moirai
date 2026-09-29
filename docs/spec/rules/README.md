# Rule tables: format, parser contract and signing

| Field | Value |
|---|---|
| Status | draft, pass 1 pending |
| Work package | WP-90 (R-MODEL), with WP-91 and WP-92 as consumers ([m0/PLAN §3.2] item 9) |
| Sources | [60 §4.1] role of the model; [60 §4.2] rules as data, owner-signed; [60 §4.5] mitigations (signed rule tables); [60 §4.6] one function per rule, coverage by section tags; [AR §13] policy data and the sweep plan; [m0/PLAN §3.1] S2; [m0/PLAN §3.2] item 9 shared rules; [m0/PLAN §5] V3 |
| Cited as | [RULES/README]; a row as [RULES/README CT-001] |

## 1. What a rule table is

A rule table is the single source of truth for one family of rules of the reference model (`moirai-model`, [60 §4]).
The model does not restate these rules in code comments or constants. It includes each rule file with `include_str!`,
parses the machine-readable tables in it, and either evaluates the rows as data or checks that its code implements
exactly the rows listed. The owner reads and signs the same bytes the model parses (§6), so a signed table and the
tested behaviour cannot drift apart ([60 §4.5]: "owner-signed rule tables … they are data").

Each rule file has two layers:

1. **Prose for the owner.** A header block, then what the tables decide, the sources, and how to read a row. Prose
   is never parsed.
2. **Machine-readable tables.** Markdown pipe tables with a fixed, registered column set (§4, §7), one fact per row,
   no merged cells, and free prose only in the final column.

Every rule file ends with the sections "Coverage", "Holes" and "Open points for the review", like every chapter of
`docs/spec/`.

### 1.1 The rule files

| File | Decides | WP | State |
|---|---|---|---|
| `README.md` | this contract: column types and the registry of every table | WP-90 | this draft |
| `merge-table.md` | key class → typed merge rule → conflict class for every key class of the data model, the recursive virtual base, validators, land-or-stage | WP-90, WP-91 | this draft |
| `link-merge-rules.md` | the R4 link merge rules of [40 §5.5]: observation composite, derived identity and re-key, anchors, globs, `path_moves`, path claims | WP-92 | this draft |
| `status-machines.md` | guarded status transitions per kind, their doors and derived effects ([AR §3.6]) | WP-90 | this draft |
| `delete-policy-matrix.md` | the delete-policy matrix: edge kind × delete policy × effect on dependents ([AR §3.3], [AR §2.5]) | WP-90 | this draft |
| `pack-classes.md` | pack and brief candidate classes: membership, levels, order, byte budgets, the stale-pack notice ([AR §7.4], [50 §4.3]) | WP-90 | this draft |
| `state-definition.md` | the I26′ state definition with its marker-cache rules, `ready` and `unblocked`, absorbed vectors and leases ([AR §3.4], [72 M4]) | WP-90 | this draft |
| `role-write-policy.md` | the role write policy: roles × verbs, kinds, fields and edges → allowed or refused ([AR §7.3]) | WP-90 | this draft |
| `policy-keys.md` | every operational-policy key and policy-data row → model function, with the values each is tested at ([AR §13], [60 §3.14], [CFG §9.4], §9.5, §10) | WP-90b | this draft |
| `SIGNED.md` | the owner's BLAKE3 digests of the files above | owner | written by the owner (§6) |

Tables of a planned file are added to the registry (§7) when the file is written; the registry below lists every
table of the files written so far, plus `SIGNED.md`'s (RG-031 to RG-111 were added in review pass 1, S1-47, and
RG-112 to RG-115 with `policy-keys.md` in WP-90b).

## 2. Byte-level rules for a rule file

- UTF-8 without a byte-order mark. A carriage return (0x0D) anywhere in the file is a parse error: line ends are LF
  only. `.gitattributes` (`* text=auto eol=lf`, [m0/PLAN §2.5]) keeps them LF on every checkout, and the parser refuses
  a CRLF file rather than hashing bytes that differ from the signed ones.
- A table line contains no tab (0x09).
- Cells of every column type except `text` and `cite` hold printable ASCII only (0x20–0x7E); a space appears only
  as part of the `, ` separator of a `tokens` cell.

## 3. How a table is found

- A machine-readable table is announced by a marker line of exactly this form, starting in column 0 and alone on its
  line: `<!-- table: <table-id> -->`, where `<table-id>` matches `^[a-z][a-z0-9-]*$` and is unique across all rule
  files.
- The line immediately after the marker is the header row. No blank line may separate them.
- A pipe table with no marker immediately above it is prose (for example the header block of every file) and is
  ignored by the parser.
- Lines inside fenced code blocks (from a line whose first non-space characters are three backticks to the next such
  line) are never parsed, so examples of the format can be shown safely.
- A marker whose table id is not in the registry (§7), or a registered table that is missing from its registered file,
  is a parse error. The one exception is `SIGNED.md`: while the owner has not written it, it is absent and every rule
  file counts as unsigned (§6).

## 4. Table grammar

A table is three or more consecutive lines. The header row and every data row start with `| ` and end with ` |`; the
separator row starts and ends with `|` and is written without spaces (`|---|---|`):

1. **Header row.** Its cells are the column names, in the exact order the registry lists (§7). A missing, extra or
   reordered column is a parse error.
2. **Separator row.** Exactly as many cells as the header; each cell matches `^:?-{3,}:?$`.
3. **Data rows.** Every line up to the first line that does not start with `|` (a blank line ends the table). Each data
   row has exactly as many cells as the header.

**Cell splitting.** The line is split at every `|` that is not immediately preceded by a backslash. The empty strings
before the first and after the last `|` are dropped. Each cell is trimmed of leading and trailing spaces (0x20). The
two-byte sequence `\|` inside a cell stands for one `|`; it is the only escape, and a backslash followed by any other
byte is literal.

**Cell values.** In a column of any type other than `text`, one pair of enclosing backticks is stripped before the
value is checked (authors may write `` `in_progress` `` for readability). The value `-` means "empty" or "not
applicable" wherever the column type allows it (§5). No cell of a typed column may be empty after trimming.

**Row ids.** The first column of every table is `row`, of type `id`. A row id is the table's registered two-letter
prefix, a hyphen and three decimal digits (`MR-012`). Row ids are unique across all rule files, stable forever, and
never reused or renumbered: a withdrawn row keeps its id and gets basis `withdrawn` (in a table without a `basis`
column, its final cell starts with `withdrawn:`). A row added later takes the table's next free id and is placed
where it belongs, so ids need not ascend; in a `decision` table the order of the lines, not of the ids, is the
evaluation order (§8). Model functions cite rows in their tags (`rule: MR-012`) beside the section tags of
[60 §4.6].

**Final column.** Exactly one column of type `text` exists per table, and it is the last one (`note`, or
`definition` in vocabulary tables). It is the only place for free prose.

## 5. Column types

<!-- table: column-types -->
| row | type | note |
|---|---|---|
| CT-001 | `id` | Row id: `^[A-Z]{2}-[0-9]{3}$`, prefix as registered for the table. |
| CT-002 | `token` | One printable-ASCII word: `^[!-~]+$` with no space, no backtick and no unescaped pipe; `-` allowed. Tokens name vocabulary entries, row ids, kinds, fields, statuses, conflict classes and small procedure words; each table's prose defines its tokens. |
| CT-003 | `tokens` | One or more tokens separated by exactly `, ` (comma, space); `-` allowed. A token inside a list may contain a comma that is not followed by a space. |
| CT-004 | `enum` | A token from the fixed set written in the registry as `enum(a/b/c)`; `-` only if the set lists it. |
| CT-005 | `int` | Decimal digits without sign or leading zeros (except `0`). |
| CT-006 | `cite` | One or more citations separated by `; `. Each starts with a bracketed reference in the conventions of the specification (`[AR §5a.7]`, `[40 §5.5]`, `[50 §4.4]`, `[60 §3.4]`, `[F12 §x.y]`, `[RULES/<file> <row>]`) and may continue with a short qualifier (`[AR §5a.7] step 4`, `[AR §3.4] I25′`). `[OP-n]` cites open point n of the same file. UTF-8 allowed. |
| CT-007 | `text` | Free UTF-8 prose; final column only; `\|` escapes a pipe; `-` for none. |

**Shared enums.**

- `basis` = `enum(design/derived/proposed/gap/withdrawn)`:
  - `design`: the cited design section states the row;
  - `derived`: the row follows from the cited rules, and the note says how;
  - `proposed`: R-MODEL's resolution of a silence or a disagreement in the design, listed in "Open points for the
    review"; the owner accepts or overturns it at signing;
  - `gap`: no rule exists yet; the model raises `SpecGap(<row id>)` when a case reaches the row, and every tier treats
    that as a test failure, so a gap cannot be passed silently (§8);
  - `withdrawn`: kept only so its id is never reused.
- `disposition` = `enum(clean/value/structural/hint/gap/none)`, as defined in [RULES/merge-table] §2.

## 6. Signing

- **What is signed.** Every rule file listed in §1.1 except `SIGNED.md` itself, this README included, because it is the
  contract the model parses by. The signature covers the exact bytes of the file as committed.
- **Digest.** BLAKE3-256 over the file's bytes, written as 64 lower-case hex digits. The model already depends on
  `blake3` ([m0/PLAN §3.2] item 9); its test `rules_print_digests` prints the digest of every rule file, so the owner
  needs no extra tool.
- **`SIGNED.md`.** Written and committed only by the owner; agent briefs forbid writing it. Nothing mechanical enforces
  that yet: [m0/PLAN §3.1] gives R-MODEL all of `docs/spec/rules/**`, and every commit carries the owner's identity
  (AGENTS.md), so neither `xtask authors` nor CI's author check can tell an agent's edit from the owner's (Open point 5).
  It holds one machine-readable table:

  ```
  <!-- table: signatures -->
  | row | file | blake3 | signed_on | note |
  |---|---|---|---|---|
  | SG-001 | merge-table.md | <64 hex> | 2026-10-12 | - |
  ```

  `file` is a file name in `docs/spec/rules/`; `signed_on` is an ISO date token; one row per signed file.
- **Verification.** The model's test `rules_signed` reads `SIGNED.md` at run time (from `CARGO_MANIFEST_DIR`, not with
  `include_str!`, which cannot name a file that may not exist yet) and recomputes each listed file's digest from the
  bytes the model included: a mismatch fails in every tier and names the file and both digests. An absent `SIGNED.md`
  means every file is unsigned. A rule file with no row in `SIGNED.md` is *unsigned*; that fails only in tier `exit`
  (`MOIRAI_TEST_TIER=exit`, [m0/PLAN §2.1]), because E1 requires every table signed before the freeze ([m0/PLAN §7]).
- **Any edit breaks the signature visibly.** A change to a row, including its note, changes the digest, so the owner
  re-signs after every change (for example after a WP-73 remedy, [m0/PLAN §3.2] item 10). Row ids let the owner diff two
  versions row by row.

## 7. The registry

The registry is the list of every machine-readable table, its file, its kind, its row-id prefix and its exact columns
(`name:type`, in order). The model compiles in the column lists it implements and compares them with this table; any
difference in either direction is a parse error, so a table cannot change shape without a model change and a
re-signature.

Table kinds:

- `vocabulary`: defines tokens (cases, results, column types) that other tables use; the parser checks every use
  against the union of the vocabularies.
- `decision`: rows the model evaluates as data (§8).
- `procedure`: rows the model implements as tagged functions; the table fixes the order and the tokens, and a test
  checks that the implementation declares exactly these rows.
- `map`: cross-references from design rows to rule rows, checked for dangling references only.
- `meta`: this README's own tables and `SIGNED.md`.

<!-- table: registry -->
| row | table | file | kind | row_prefix | columns | note |
|---|---|---|---|---|---|---|
| RG-001 | `column-types` | README.md | meta | CT | row:id, type:token, note:text | §5 |
| RG-002 | `registry` | README.md | meta | RG | row:id, table:token, file:token, kind:enum(vocabulary/decision/procedure/map/meta), row_prefix:token, columns:tokens, note:text | this table |
| RG-003 | `signatures` | SIGNED.md | meta | SG | row:id, file:token, blake3:token, signed_on:token, note:text | written by the owner (§6) |
| RG-004 | `merge-classes` | merge-table.md | vocabulary | MC | row:id, class:token, key_class:tokens, rules_file:enum(merge-table/link-merge-rules/none), basis:enum, source:cite, note:text | the closed set of merge classes |
| RG-005 | `cases` | merge-table.md | vocabulary | CS | row:id, case:token, classes:tokens, definition:text | case predicates over (b, o, t) |
| RG-006 | `results` | merge-table.md | vocabulary | RS | row:id, result:token, definition:text | result functions |
| RG-007 | `merge-rules` | merge-table.md | decision | MR | row:id, class:token, case:token, result:token, conflict:token, disposition:enum, basis:enum, source:cite, note:text | first match per class |
| RG-008 | `status-lattice` | merge-table.md | decision | SL | row:id, kind:token, status:token, side:enum(yes/no), covers:tokens, basis:enum, source:cite, note:text | the merge order of every kind's statuses |
| RG-009 | `existence-policy` | merge-table.md | decision | EP | row:id, kind:token, uid_derivation:enum(random/file-key/root-key/anchor-key), policy:enum(delete-wins/resurrect/none), basis:enum, source:cite, note:text | provisional state under `DeleteVsModify` |
| RG-010 | `auto-policy` | merge-table.md | decision | AP | row:id, value:token, applies_to:token, effect:token, basis:enum, source:cite, note:text | values of `merge.policy.<kind>` and `--policy` |
| RG-011 | `field-class` | merge-table.md | decision | FC | row:id, kind:token, field:token, type:token, class:token, basis:enum, source:cite, note:text | merge class of every core field |
| RG-012 | `edge-class` | merge-table.md | decision | EC | row:id, edge:token, edge_class:enum(structural/historical), class:token, props:token, constraint:tokens, basis:enum, source:cite, note:text | merge class of every core edge kind |
| RG-013 | `class-map` | merge-table.md | map | CM | row:id, conflict:token, kind:enum(value/structural/hint), lands:enum(yes/no/log), emitted_by:tokens, basis:enum, source:cite, note:text | the [AR §5a.8] taxonomy |
| RG-014 | `validators` | merge-table.md | procedure | VA | row:id, order:int, check:token, emits:token, disposition:enum, basis:enum, source:cite, note:text | I37′ order |
| RG-015 | `land-or-stage` | merge-table.md | decision | LS | row:id, violations:token, conflicts:token, strict:enum(yes/no/any), outcome:token, basis:enum, source:cite, note:text | step 8 |
| RG-016 | `procedure` | merge-table.md | procedure | PR | row:id, step:token, action:token, basis:enum, source:cite, note:text | the merge algorithm |
| RG-017 | `virtual-base` | merge-table.md | procedure | VB | row:id, condition:token, outcome:token, basis:enum, source:cite, note:text | the recursive virtual base |
| RG-018 | `derived-merges` | merge-table.md | procedure | DM | row:id, operation:token, dst:token, src:token, base:token, special:tokens, basis:enum, source:cite, note:text | sync, revert, cherry-pick, virtual and import merges |
| RG-019 | `runtime-effects` | merge-table.md | procedure | RE | row:id, class:token, effect:token, basis:enum, source:cite, note:text | leases, markers, absorbed vectors, derived state |
| RG-020 | `hints` | merge-table.md | procedure | HT | row:id, hint:token, trigger:token, basis:enum, source:cite, note:text | hint classes |
| RG-021 | `source-map` | merge-table.md | map | SM | row:id, source_row:token, realized_by:tokens, note:text | design rows → rule rows |
| RG-022 | `link-cases` | link-merge-rules.md | vocabulary | LC | row:id, case:token, classes:tokens, definition:text | R4 case predicates |
| RG-023 | `link-results` | link-merge-rules.md | vocabulary | LR | row:id, result:token, definition:text | R4 result functions |
| RG-024 | `link-merge-rules` | link-merge-rules.md | decision | LM | row:id, class:token, case:token, result:token, conflict:token, disposition:enum, basis:enum, source:cite, note:text | first match per class; same columns as `merge-rules` |
| RG-025 | `path-claim` | link-merge-rules.md | procedure | PC | row:id, aspect:token, rule:token, basis:enum, source:cite, note:text | I-F1 at merge |
| RG-026 | `compose-steps` | link-merge-rules.md | procedure | CP | row:id, step:int, action:token, basis:enum, source:cite, note:text | directory-move composition |
| RG-027 | `rekey-steps` | link-merge-rules.md | procedure | RK | row:id, step:int, action:token, basis:enum, source:cite, note:text | re-key, never resurrect |
| RG-028 | `link-resolution` | link-merge-rules.md | procedure | LV | row:id, conflict:token, resolved_by:token, condition:token, provenance:token, basis:enum, source:cite, note:text | how link conflicts are settled after a merge |
| RG-029 | `link-history` | link-merge-rules.md | procedure | LH | row:id, operation:tokens, effect:token, basis:enum, source:cite, note:text | history verbs and files |
| RG-030 | `link-source-map` | link-merge-rules.md | map | LX | row:id, source_row:token, realized_by:tokens, note:text | [40 §5.5] rows → rule rows |
| RG-031 | `pack-terms` | pack-classes.md | vocabulary | PT | row:id, term:token, sort:enum(input/view/set/pred/fn/order), basis:enum, source:cite, definition:text | §3 |
| RG-032 | `pack-kinds` | pack-classes.md | decision | PK | row:id, pack:token, trigger:tokens, classes:tokens, budget_key:token, default_bytes:token, basis:enum, source:cite, note:text | §4 |
| RG-033 | `pack-budgets` | pack-classes.md | decision | PB | row:id, role:token, key:token, default_bytes:int, final_at:token, basis:enum, source:cite, note:text | §4 |
| RG-034 | `pack-ceilings` | pack-classes.md | decision | PE | row:id, surface:enum(cli/mcp/file/hook), client:token, key:token, default_bytes:token, max_bytes:token, basis:enum, source:cite, note:text | §4 |
| RG-035 | `pack-bytes` | pack-classes.md | procedure | PY | row:id, rule:token, basis:enum, source:cite, definition:text | §4 |
| RG-036 | `pack-classes` | pack-classes.md | vocabulary | CL | row:id, class:token, rank:int, name:token, named_query:token, basis:enum, source:cite, definition:text | §5.1 |
| RG-037 | `pack-quotas` | pack-classes.md | decision | PQ | row:id, class:token, roles:tokens, key:token, default_pct:int, basis:enum, source:cite, note:text | §5.1 |
| RG-038 | `pack-floors` | pack-classes.md | procedure | PF | row:id, class:token, basis:enum, source:cite, definition:text | §5.1 |
| RG-039 | `pack-members` | pack-classes.md | procedure | PM | row:id, class:token, part:token, roles:tokens, view:enum(B/M/U/feed), kinds:tokens, level:token, basis:enum, source:cite, definition:text | §5.2 |
| RG-040 | `pack-levels` | pack-classes.md | procedure | PL | row:id, kind:token, level:enum(ID/L0/L1/L2), content:tokens, basis:enum, source:cite, note:text | §5.3 |
| RG-041 | `pack-order` | pack-classes.md | procedure | PO | row:id, class:token, keys:tokens, basis:enum, source:cite, note:text | §5.4 |
| RG-042 | `pack-header` | pack-classes.md | procedure | PH | row:id, position:int, item:token, when:token, basis:enum, source:cite, definition:text | §5.4 |
| RG-043 | `pack-render` | pack-classes.md | procedure | RN | row:id, rule:token, basis:enum, source:cite, definition:text | §5.4 |
| RG-044 | `pack-fill` | pack-classes.md | procedure | PX | row:id, step:int, action:token, basis:enum, source:cite, definition:text | §5.4 |
| RG-045 | `hook-pack` | pack-classes.md | procedure | HP | row:id, position:int, item:token, level:token, basis:enum, source:cite, definition:text | §6 |
| RG-046 | `brief-classes` | pack-classes.md | procedure | BR | row:id, class:token, rank:int, named_query:token, kinds:tokens, level:token, basis:enum, source:cite, definition:text | §6 |
| RG-047 | `delta-rules` | pack-classes.md | procedure | DL | row:id, pack:token, rule:token, basis:enum, source:cite, definition:text | §6 |
| RG-048 | `notice-sets` | pack-classes.md | procedure | NS | row:id, set:token, code:int, view:enum(B/M), kinds:tokens, basis:enum, source:cite, definition:text | §7 |
| RG-049 | `notice-rules` | pack-classes.md | procedure | NR | row:id, step:int, rule:token, basis:enum, source:cite, definition:text | §7 |
| RG-050 | `notice-digest` | pack-classes.md | procedure | ND | row:id, offset:token, width:token, type:token, name:token, basis:enum, source:cite, meaning:text | §7 |
| RG-051 | `notice-entry` | pack-classes.md | procedure | NE | row:id, offset:token, width:token, type:token, name:token, basis:enum, source:cite, meaning:text | §7 |
| RG-052 | `notice-modes` | pack-classes.md | decision | NM | row:id, mode:token, output:token, cap_bytes:int, basis:enum, source:cite, note:text | §7 |
| RG-053 | `pack-source-map` | pack-classes.md | map | PS | row:id, source_row:token, realized_by:tokens, note:text | §8 |
| RG-054 | `role-terms` | role-write-policy.md | vocabulary | WT | row:id, term:token, sort:enum(input/scope/set/pred), basis:enum, source:cite, definition:text | §3 |
| RG-055 | `role-rights` | role-write-policy.md | procedure | WR | row:id, step:int, rule:token, basis:enum, source:cite, definition:text | §4 |
| RG-056 | `role-rows` | role-write-policy.md | vocabulary | WO | row:id, role:token, carried_by:tokens, self_claim:enum(yes/no), mcp_write:enum(yes/no), basis:enum, source:cite, note:text | §5 |
| RG-057 | `role-mint` | role-write-policy.md | decision | WM | row:id, form:token, commands:tokens, allowed:tokens, key:token, refusal:token, exit:token, basis:enum, source:cite, note:text | §6 |
| RG-058 | `role-verbs` | role-write-policy.md | decision | WV | row:id, verb:token, class:enum(ref/graph/runtime/file-fs/file-link/admin/read/surface), surface:enum(cli/mcp/both), roles:tokens, key:token, refusal:token, exit:token, basis:enum, source:cite, note:text | §7 |
| RG-059 | `role-statements` | role-write-policy.md | decision | WX | row:id, statement:token, roles:tokens, surface:enum(cli/mcp/both), key:token, refusal:token, exit:token, basis:enum, source:cite, note:text | §8 |
| RG-060 | `role-create` | role-write-policy.md | decision | WC | row:id, role:token, kind:token, constraint:tokens, required:tokens, basis:enum, source:cite, note:text | §9 |
| RG-061 | `role-values` | role-write-policy.md | decision | WA | row:id, field:token, value:tokens, roles:tokens, requires:tokens, basis:enum, source:cite, note:text | §9 |
| RG-062 | `role-fields` | role-write-policy.md | decision | WF | row:id, role:token, kind:token, scope:token, fields:tokens, key:token, basis:enum, source:cite, note:text | §9 |
| RG-063 | `role-status` | role-write-policy.md | decision | WS | row:id, role:token, kind:token, scope:token, from:tokens, to:token, via:token, basis:enum, source:cite, note:text | §9 |
| RG-064 | `role-edges` | role-write-policy.md | decision | WE | row:id, role:token, edge:token, ops:tokens, src_scope:token, dst_scope:token, basis:enum, source:cite, note:text | §9 |
| RG-065 | `role-reads` | role-write-policy.md | decision | WQ | row:id, subject:token, what:token, allowed:enum(yes/no), key:token, refusal:token, exit:token, basis:enum, source:cite, note:text | §10 |
| RG-066 | `role-hooks` | role-write-policy.md | procedure | WH | row:id, hook:token, writes:tokens, basis:enum, source:cite, note:text | §10 |
| RG-067 | `role-refusals` | role-write-policy.md | vocabulary | WZ | row:id, situation:token, code:token, name:token, exit:token, text_owner:token, basis:enum, source:cite, note:text | §10 |
| RG-068 | `role-source-map` | role-write-policy.md | map | WY | row:id, source_row:token, realized_by:tokens, note:text | §11 |
| RG-069 | `view-kinds` | state-definition.md | decision | VK | row:id, ref_kind:token, holds_count:enum(yes/no), tip_reads:enum(yes/no), basis:enum, source:cite, note:text | - |
| RG-070 | `hold-values` | state-definition.md | decision | HV | row:id, kind:token, state:token, hold:token, basis:enum, source:cite, note:text | - |
| RG-071 | `origin-rules` | state-definition.md | decision | OR | row:id, parents:int, condition:token, origin:token, basis:enum, source:cite, note:text | first match in order |
| RG-072 | `predicates` | state-definition.md | procedure | PD | row:id, predicate:token, clause:token, basis:enum, source:cite, note:text | - |
| RG-073 | `validity` | state-definition.md | decision | VD | row:id, predicate:token, valid_at:enum(any-view/tip-only), past_view:token, basis:enum, source:cite, note:text | - |
| RG-074 | `blocker-terms` | state-definition.md | decision | BT | row:id, edge:token, source_state:token, counts_in:token, weight:int, basis:enum, source:cite, note:text | - |
| RG-075 | `lease-live` | state-definition.md | decision | LL | row:id, scope:enum(run/ttl), anchor:token, boot:token, slot:token, deadline:token, live:token, basis:enum, source:cite, note:text | first match in order |
| RG-076 | `lease-ends` | state-definition.md | procedure | LE | row:id, event:token, effect:token, basis:enum, source:cite, note:text | - |
| RG-077 | `lease-effects` | state-definition.md | procedure | LF | row:id, consumer:token, rule:token, basis:enum, source:cite, note:text | - |
| RG-078 | `marker-fields` | state-definition.md | vocabulary | MF | row:id, field:tokens, basis:enum, source:cite, note:text | `tokens`: MF-008 and MF-009 name two fields each (review pass 1 round 1, A1-35) |
| RG-079 | `marker-events` | state-definition.md | procedure | ME | row:id, event:token, condition:token, record:token, basis:enum, source:cite, note:text | - |
| RG-080 | `absorption` | state-definition.md | decision | AB | row:id, marker_state:token, test:token, basis:enum, source:cite, note:text | - |
| RG-081 | `vector-rules` | state-definition.md | procedure | VR | row:id, event:token, rule:token, basis:enum, source:cite, note:text | - |
| RG-082 | `door-coverage` | state-definition.md | map | DC | row:id, door:token, events:tokens, basis:enum, source:cite, note:text | - |
| RG-083 | `scenarios` | state-definition.md | decision | SN | row:id, scenario:token, step:int, ref:token, action:token, basis:enum, source:cite, note:text | fixture data |
| RG-084 | `scenario-expect` | state-definition.md | decision | SX | row:id, scenario:token, step:int, check_ref:token, excluded:enum(yes/no), basis:enum, source:cite, note:text | fixture data |
| RG-085 | `status-fields` | status-machines.md | decision | SF | row:id, kind:token, field:token, stored:enum(yes/no), guarded:enum(yes/no), basis:enum, source:cite, note:text | - |
| RG-086 | `statuses` | status-machines.md | decision | ST | row:id, kind:token, status:token, initial:enum(yes/no), done:enum(yes/no/derived/absent), lattice:token, basis:enum, source:cite, note:text | - |
| RG-087 | `doors` | status-machines.md | vocabulary | DR | row:id, door:token, requires:tokens, basis:enum, source:cite, note:text | - |
| RG-088 | `transitions` | status-machines.md | decision | TR | row:id, kind:token, from:token, to:token, door:token, move:enum(up/down/to-side/from-side), basis:enum, source:cite, note:text | - |
| RG-089 | `guards` | status-machines.md | vocabulary | GD | row:id, guard:token, refusal:token, exit:token, basis:enum, source:cite, definition:text | - |
| RG-090 | `transition-guards` | status-machines.md | decision | TG | row:id, kind:token, from:token, to:token, guard:token, basis:enum, source:cite, note:text | - |
| RG-091 | `door-roles` | status-machines.md | map | DG | row:id, door:token, realized_by:tokens, basis:enum, source:cite, note:text | - |
| RG-092 | `branch-mask` | status-machines.md | decision | BM | row:id, view:token, status_writes:enum(yes/no), refusal:token, exit:token, basis:enum, source:cite, note:text | - |
| RG-093 | `derived-effects` | status-machines.md | procedure | DE | row:id, kind:token, from:token, to:token, predicate:token, subject:token, basis:enum, source:cite, note:text | - |
| RG-094 | `complete-outcomes` | status-machines.md | decision | CO | row:id, outcome:token, status:token, lease:token, hold:token, basis:enum, source:cite, note:text | - |
| RG-095 | `general-rules` | status-machines.md | procedure | GR | row:id, rule:token, applies_to:token, refusal:token, exit:token, basis:enum, source:cite, note:text | - |
| RG-096 | `delete-options` | delete-policy-matrix.md | vocabulary | DO | row:id, option:token, basis:enum, source:cite, definition:text | - |
| RG-097 | `edge-conditions` | delete-policy-matrix.md | vocabulary | CD | row:id, condition:token, basis:enum, source:cite, definition:text | - |
| RG-098 | `edge-actions` | delete-policy-matrix.md | vocabulary | EA | row:id, action:token, basis:enum, source:cite, definition:text | - |
| RG-099 | `edge-effects` | delete-policy-matrix.md | vocabulary | EF | row:id, effect:token, basis:enum, source:cite, definition:text | - |
| RG-100 | `delete-preconditions` | delete-policy-matrix.md | procedure | DP | row:id, check:token, refusal:token, exit:token, basis:enum, source:cite, note:text | - |
| RG-101 | `edge-policy` | delete-policy-matrix.md | decision | EG | row:id, edge:token, end:enum(dst-deleted/src-deleted), option:token, policy:token, condition:token, action:token, effect:token, basis:enum, source:cite, note:text | - |
| RG-102 | `delete-steps` | delete-policy-matrix.md | procedure | DS | row:id, step:token, action:token, basis:enum, source:cite, note:text | - |
| RG-103 | `flagged-edges` | delete-policy-matrix.md | procedure | FL | row:id, rule:token, basis:enum, source:cite, note:text | - |
| RG-104 | `tombstone` | delete-policy-matrix.md | decision | TB | row:id, item:token, kept:enum(yes/no), basis:enum, source:cite, note:text | - |
| RG-105 | `undelete` | delete-policy-matrix.md | procedure | UD | row:id, item:token, effect:token, basis:enum, source:cite, note:text | - |
| RG-106 | `cross-branch` | delete-policy-matrix.md | map | XB | row:id, case:token, realized_by:tokens, basis:enum, source:cite, note:text | - |
| RG-107 | `n40-nodes` | delete-policy-matrix.md | decision | NN | row:id, node:token, kind:token, status:token, parent:token, note:text | fixture data |
| RG-108 | `n40-edges` | delete-policy-matrix.md | decision | NG | row:id, src:token, kind:token, dst:token, props:token, note:text | fixture data |
| RG-109 | `n40-cases` | delete-policy-matrix.md | decision | NC | row:id, case:token, ref:token, after:token, action:token, basis:enum, source:cite, note:text | fixture data |
| RG-110 | `n40-properties` | delete-policy-matrix.md | vocabulary | NP | row:id, property:token, definition:text | - |
| RG-111 | `n40-expect` | delete-policy-matrix.md | decision | NX | row:id, case:token, ref:token, subject:token, property:token, value:token, basis:enum, source:cite, note:text | fixture data |
| RG-112 | `key-checkers` | policy-keys.md | vocabulary | KC | row:id, checker:token, vis:tokens, basis:enum, source:cite, definition:text | §3 |
| RG-113 | `key-functions` | policy-keys.md | vocabulary | KF | row:id, function:token, wp:token, basis:enum, source:cite, definition:text | §4 |
| RG-114 | `policy-keys` | policy-keys.md | decision | KY | row:id, key:token, instance:token, vis:enum(V/I/Rs/B/O/X), function:token, values:tokens, checker:tokens, basis:enum, source:cite, note:text | §5; one row per key pattern of [CFG §10] |
| RG-115 | `policy-rows` | policy-keys.md | decision | PV | row:id, row_name:token, instance:token, function:token, values:tokens, basis:enum, source:cite, note:text | §6; one row per policy-data row of [CFG §10.13] |

A column written `basis:enum` or `disposition:enum` without a set uses the shared enum of §5.

## 8. How the model loads and uses the tables

- **Inclusion.** `crates/moirai-model/src/rules.rs` includes every rule file except `SIGNED.md` (§6) with a path
  relative to that source file, for example `include_str!("../../../docs/spec/rules/merge-table.md")`. The files in
  `docs/spec/rules/` are the only copy: nothing is duplicated under `crates/moirai-model/` (see Open point 1).
  `include_str!` makes rustc record each file as a build dependency, so an edit rebuilds the model.
- **Parsing.** All files are parsed once per test process into typed Rust tables behind a `OnceLock`, before any
  model function runs. Any violation of §2–§7 panics with the file, the line, the table id and the column; a rule
  file never parses partially and is never skipped.
- **Referential checks at load.** Row ids are unique across files; every token a decision table uses is defined by a
  vocabulary table; every merge class used by `field-class` or `edge-class` exists in `merge-classes`, and its rules
  live in exactly the file `merge-classes` names; every conflict class used by a rule row appears in `class-map`; every
  row id cited in a `realized_by` or `emitted_by` cell exists; every `[OP-n]` cited exists in that file's open points.
- **Decision tables.** For a key of merge class C, the model evaluates the rows of C in table order, and the first
  row whose case holds decides the result, the conflict class and the disposition. For every class the rows must be
  exhaustive; a property test draws random (b, o, t) triples per class and fails if no row matches (a gap in the
  table) or if a `gap` row is reached (`SpecGap`).
- **Procedure tables.** Each row is implemented by a function tagged `rule: <row id>`. A test compares the set of tags
  with the table's rows in both directions, and the implementation declares the tokens it implements for each row;
  a mismatch fails.
- **Row coverage.** WP-94's section-tag coverage report ([m0/PLAN §3.2] item 9) also lists every row id no test case
  reached. A decision row that no case reaches needs a fixture before E5.
- **No JSON, no generated code.** The parser is hand-written in the model ([m0/PLAN §3.2] item 9: `std` plus the hash
  crates, no JSON code). The rule files are never regenerated from code; code follows the tables.

## Coverage

This file specifies no [60 §2.5] row, R-row, F-row, X-F row or [90 §10.1] item; it is the loading contract for the rule
tables that cover them (see each rule file's Coverage section).

## Holes

None. The contract contains no value that an M0 measurement decides.

## Open points for the review

1. **One copy of the tables.** [m0/PLAN §3.2] item 9 says "Rule tables are data under `crates/moirai-model/rules/`,
   published as `docs/spec/rules/*.md`". This contract keeps a single copy in `docs/spec/rules/`, included by relative
   path, because two copies could diverge and the signed bytes must be the parsed bytes. Proposed: PLAN.md's wording is
   corrected to "the model includes `docs/spec/rules/*.md`".
2. **Unsigned tables before the freeze.** Unsigned files fail only in tier `exit` (§6), so pass-1 work is not blocked
   while the owner signs table by table (V3, weeks 3–4). The review confirms that this matches E1's intent. The owner
   accepted the rows review pass 1 changed on 2026-09-28 (owner question OQ-M-1, option (a)); their signatures in
   `SIGNED.md` follow under V3.
3. **`gap` rows fail every tier.** A row with basis `gap` makes any case that reaches it fail. This forces each gap to
   be closed by the review before E5; until then, randomized suites that hit a gap report it as a specification
   finding rather than a model bug. The review confirms this is the wanted pressure.
4. **Registry ownership of planned tables.** A planned rule file (§1.1) adds its tables to §7 when written; each
   addition changes this README and needs its re-signature. The five files written in wave 1 proposed their rows in
   their own open points; review pass 1 (S1-47) found §1.1 stale and the rows unregistered, and they are now RG-031 to
   RG-111, with the columns and prefixes each file proposed (checked against every table's header row and against every
   other prefix: no two tables share one). `policy-keys.md` was written in WP-90b and registered as RG-112 to RG-115;
   no rule file remains planned.
5. **Protecting `SIGNED.md` mechanically.** Proposed for WP-01/WP-02 (`docs/m0/authors.md` path precedence): make
   `docs/spec/rules/SIGNED.md` an owner-only path that no WP's role may write, so `xtask authors` refuses any `WP-xx:`
   commit touching it; the owner's signing commits carry no `WP-` subject. Until then the rule is procedural.
