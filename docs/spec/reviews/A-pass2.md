# Review pass 2, lens A: agent fit, tokens and buildability

| Field | Value |
|---|---|
| Title | Independent review pass 2 of the integrated specification (`docs/spec/` outside `docs/spec/reviews/`), lens A (agent fit, tokens and buildability; the method of [22]) |
| Status | review, pass 2; every finding awaits its disposition by the owning role named in its Fix |
| Work package | WP-80 ([PLAN §3.2] item 8), lens R-REV-A. The author of this file wrote none of the reviewed text (S5) and did not read the pass-2 findings of the other lenses |
| Lens | A: what an agent reads (texts, refusals, results, notices), what it costs in bytes, and whether two implementers can build the same thing from the text |
| Reviewed | every file of `docs/spec/` outside `docs/spec/reviews/`, except `docs/spec/measurement-protocol.md`: 76 files, 52,374 lines (§1) |
| Base | 022277db150d047e817652691e2fc19be1a37285 |
| Checked against | [AR] §7.1–§7.5, §8.3 TOKENS rows, §11, §13; [90] §2, §4.1, §6, §8.1, §8.2, §10.1, §10.2, §14.6; [40] §2.7, §6.2; [50] §5.2, §6.4, §7; [60 §2.5]; [PLAN §3.2] item 8 and [PLAN §7] E1; `docs/m0/authors.md` §2; `docs/spec/reviews/a1-dispositions.md`, `pass1-closure.md`, `A-pass1.md` |

Severity, as WP-80 defines it for the passes:

- **blocker**: two independent implementers would produce different bytes, or a rule of the design of record is violated;
- **major**: an ambiguity, a missing case or an inconsistency that must be fixed before the freeze;
- **minor**: editorial, or local with no byte or rule consequence.

## 0. Verdict

**CHANGES REQUIRED: 0 blockers, 9 majors, 14 minors (23 findings).** No byte layout this lens recomputed disagrees with
itself or with its citers: every offset table sums to its total, every byte example and byte count re-derived here holds
(§6), every bracketed citation and open-point reference resolves, the rule tables parse, and the 45 standard-library
definitions equal their fixture files byte for byte. The texts an agent reads are in good shape and within their byte
bounds. The majors are elsewhere:

1. **`COVERAGE.md` is not yet E1-ready** (A2-1, A2-2, A2-3): 81 fixture cells and 87 model cells are blank under
   `coverage --strict`, and the model cells of the R5 rows name functions that belong to other items.
2. **Agent-facing texts that one chapter delegates and the owning chapter never wrote** (A2-5, A2-6, A2-7): the refusal of
   a `claim` of a task that is not ready, three notices, and the answer to a budget request above the role's ceiling.
3. **Two chapters that state one rule differently** (A2-4, A2-9): who E411 refuses, and which kinds the C7 hazard class of
   a pack holds.
4. **A missing case** (A2-8): the needs-triage note of a `SubagentStop` whose agent holds several task leases.

The minors are stale status and ownership records, stale cross-references, three templates with a non-ASCII ellipsis, an
inconsistent byte-quantity spelling, and a few informative examples and open points.

## 1. Method

Depth per file: **full** is every line; **delta** is every changed hunk of `git diff -U0 aa7c67b 022277d` over the file
and every cross-reference into it; **lens** is pass 1's "read for the lens" (headers, open points, Holes, Coverage, and the
rows and sections the findings cite). The texts an agent reads that changed since pass 1, and the two files new since
pass 1, are read in full. For the protocol, merge and byte chapters the delta was read for this lens's questions (agent
texts, byte budgets, delegated texts, names shared across chapters, examples); their semantics are the other lenses'
scope. An unchanged rule row read here rests on the later V3 review of the rule tables, not on pass 1's signature.

| File | Depth |
|---|---|
| `COVERAGE.md` | full |
| `HOLES.md` | delta |
| `README.md` | delta |
| `config.md` | full |
| `format/01-conventions.md` | delta |
| `format/02-store-layout.md` | delta |
| `format/03-lock.md` | delta |
| `format/04-head.md` | delta |
| `format/05-log.md` | delta |
| `format/06-commit.md` | delta |
| `format/07-canonical-form.md` | delta |
| `format/08-data-model.md` | delta |
| `format/09-segments.md` | delta |
| `format/10-sealed-files.md` | delta |
| `format/11-runtime-tables.md` | delta |
| `format/12-vcs.md` | delta |
| `format/13-invariants.md` | delta |
| `format/14-image.md` | delta |
| `format/15-fault-model.md` | delta |
| `format/16-protocol.md` | delta |
| `format/17-store-parameters.md` | delta |
| `format/18-file-links.md` | delta |
| `format/19-errors-and-output.md` | full |
| `format/20-r4-resolver-constants.md` | delta |
| `format/21-scope-scanners.md` | full |
| `lq/canonical-ast.md` | delta |
| `lq/card.md` | full |
| `lq/envelope.md` | full |
| `lq/errors.md` | full |
| `lq/gql-spelling.md` | delta |
| `lq/grammar-v1.ebnf` | delta |
| `lq/json-ir.md` | delta |
| `lq/lexical.md` | delta |
| `lq/std.md` | delta |
| `os/README.md` | delta |
| `os/clock.md` | delta |
| `os/env.md` | delta |
| `os/fs.md` | delta |
| `os/lock.md` | delta |
| `os/map.md` | delta |
| `os/mapping-appendix.md` | delta |
| `os/mem.md` | lens |
| `os/path.md` | delta |
| `os/proc.md` | delta |
| `os/project.md` | delta |
| `os/shell.md` | delta |
| `rules/README.md` | delta |
| `rules/delete-policy-matrix.md` | delta |
| `rules/link-merge-rules.md` | delta |
| `rules/merge-table.md` | delta |
| `rules/pack-classes.md` | delta |
| `rules/policy-keys.md` | full |
| `rules/role-write-policy.md` | delta |
| `rules/state-definition.md` | delta |
| `rules/status-machines.md` | delta |
| `store-api.md` | full |
| `store-api/examples/01-init-and-clock.json` | lens |
| `store-api/examples/02-tx-create.json` | full |
| `store-api/examples/03-set-guard.json` | full |
| `store-api/examples/04-claim-complete.json` | lens |
| `store-api/examples/05-idempotency.json` | lens |
| `store-api/examples/06-rm-policy.json` | lens |
| `store-api/examples/07-lane-merge.json` | full |
| `store-api/examples/08-merge-staged.json` | full |
| `store-api/examples/09-undo.json` | lens |
| `store-api/examples/10-caller-context.json` | lens |
| `store-api/examples/11-state-snapshot.json` | lens |
| `store-api/examples/12-runtime-snapshot.json` | full |
| `store-api/examples/13-file-mv.json` | lens |
| `store-api/examples/14-apply-batch.json` | lens |
| `store-api/examples/15-lease-clock.json` | lens |
| `store-api/examples/16-image.json` | lens |
| `store-api/examples/17-query.json` | lens |
| `store-api/examples/18-maintenance.json` | lens |
| `store-api/examples/19-schema-migrate.json` | lens |
| `store-api/examples/20-vcs-refs.json` | lens |

**Mechanical checks**, re-run at the base over the whole reviewed set (scripts in the reviewer's scratch space, not
committed; each takes its files from the reviewed set's list):

| Check | Result |
|---|---|
| bracketed citations (`[Fnn §…]`, `[OS/…]`, `[LQ/…]`, `[API]`, `[CFG]`, `[RULES/…]`, `[HOLES.md]`) against the headings and numbered paragraphs of their targets | 11,412 citations; every section reference resolves. Seven name no file: five generic `[OS/...]`/`[LQ/...]` examples in F01, `[RULES/SIGNED]` (a planned file, `docs/spec/README.md:132`) and `[RULES/delete-policy]` quoted as the old name it corrects (`docs/spec/config.md:1219`) |
| "[X] open point N" references | 267; all resolve |
| offset tables against their `total` rows | 102 tables, 100 fully numeric; contiguity and totals hold; the two variable totals (F10:499, `docs/spec/os/fs.md:579`) are correct |
| rule tables by [RULES/README] §2–§4 and §7 | 115 registry rows, 114 tables in 9 files, 2,240 row ids, all unique, no bad cell |
| the 20 Store API examples | all parse as JSON; 133 commit ids in results are `c` + 64 hex; the 5 shorter `c<8 hex>` values are revision arguments ([F12 §3.2]) and 2 are digest placeholders |
| `LQ/card`'s stated counts | all match: body 36 lines, 3,116 B, 457 words; GQL 3,110 B; 4-example 2,867/2,864 B; 0-example 2,394/2,391 B; frontmatter 206 B, description 168 characters; shrink steps 98, 84, 42, 67, 59, 79, 42, 96 B (291 B and 567 B cumulative); see A2-17 for one derived claim |
| `HOLE(...)` ids against [HOLES.md §2] | 53 = 53 |
| standard-library definitions against `fixtures/lq/std/` | 45 `lq-define` blocks, each byte-equal to its fixture file; no fixture file without a block |
| `config.md`'s model-function names against [RULES/policy-keys] | 36 keys compared, 1 differs (A2-16) |
| integration: owner layouts against their citers | `VolumeCaps` ([F11 §12.3] / [OS/project §4.2]) and `OsFileId` ([F11 §12.1] / [OS/project §3]) restate every name; `BindingExt` and the lease anchor embed the owner's bytes by citation |
| `cargo xtask coverage --strict` | `coverage: 152 rows, 168 findings; 81 blank fixture cells and 87 blank model cells (--strict: findings)`; no unmapped row (A2-1, A2-2) |

## 2. Named checks

| Id | Check | Verdict |
|---|---|---|
| N-A1 | [90 §14.6]'s model-profile item: the specification places the model-profile policy, L2's error, L4's replacement texts and L8's echo rule as [90 §10.2]'s M2–M7 row says, with no separate unit and no contradiction | MEETS: the spec freezes the keys (`docs/spec/config.md:939-941`, [RULES/policy-keys] KY-149 to KY-151), E411 ([F19 §11.1], [LQ/errors §5.5]), the replacement texts ([LQ/errors §2.4]) and the echo rule ([LQ/envelope §4.1]), and no text places their implementation in another milestone or a unit of its own |
| N-A2 | the status rows of the chapter headers against [F01 §2.3]'s values, and the records kept "until pass 2" | MISSES: A2-10 |
| N-A3 | the link marker's `was:` quote cap and the 50 B marker gate, deferred to this pass by the A1 dispositions | MEETS: [F18 §4.7] rule 5 caps part 66 at 31 B (F18:672-675) and [F19 §4.6] rule 5 holds every marker at 50 B (F19:353-356); recomputed, the longest marker form is 49 B |
| N-A4 | the A1 disposition that the renderer's goldens are reviewed in pass 2 before they bind M7 and M8 | no object at this pass: `git ls-files` at the base lists no golden of the reference renderer (its one path containing "golden" is a scanner test of a product crate); see §7 for the text goldens of `fixtures/lq/` |
| N-A5 | F21 and its interim scanner rule (F21:1193-1198): every blocker and major of this lens against F21 | MEETS: this lens raises no blocker or major against F21; its minors A2-18 and A2-19 concern F21's interfaces with F19 and F14 |
| N-A6 | the A1 dispositions' status cell ("the owner signs the dispositions in WP-80") | no object: a record no lens role may edit; an open point for the owner's records (§7) |
| N-A7 | the R4 reservations: the sections each of `COVERAGE.md` §8's rows R-1 to R-18 cites exist at the base and still specify the reservation as the row's frozen content states it | MEETS: every citation of the 18 rows resolves; every cited section names its part (spot reads: `next_anchor` at offset 80 of F04:55, the R-13 keys in `docs/spec/config.md` §10, `BindingExt` at row offset 105 of the 161-byte `HEADS` row, F11:501-504) |
| N-A8 | `COVERAGE.md`'s chapter-§ mapping with no unmapped row: no UNMAPPED and no CONFLICTING, every citation resolves to a section that specifies the row's part, and §2's summary matches the rows | MEETS: rows 1 + 22 + 27 + 40 + 7 + 18 + 18 + 12 + 7 = 152, as `docs/spec/COVERAGE.md:56` states; no row carries either mark; all 973 chapter citations of §3–§11 resolve and specify their parts. Minor A2-22 concerns §1's verification record |

## 3. Findings at a glance

| Id | Sev. | Where | Finding | Owner |
|---|---|---|---|---|
| A2-1 | major | `COVERAGE.md` §3–§11, fixture column | 81 blank fixture cells under `coverage --strict` | R-FIX, WP-20 |
| A2-2 | major | `COVERAGE.md` §3–§11, model column | 87 blank model cells under `coverage --strict` | R-MODEL, WP-90 to WP-94 |
| A2-3 | major | `COVERAGE.md` §9 | the R5 rows' model cells name functions of other items | R-SPEC-F, WP-10; R-MODEL, WP-90 |
| A2-4 | major | [F19 §11.1], [LQ/errors §5.5] against [API §9.1] | who E411 refuses | R-SPEC-F, WP-18 and WP-19 |
| A2-5 | major | [API §10.1], [LQ/errors §5.5] | no text for the refusal of a `claim` of a task that is not ready | R-SPEC-F, WP-19 |
| A2-6 | major | [F12 §9.4], [F18 §2.10], [CFG §7.6] | three notices with no code, text or JSON form | R-SPEC-F, WP-19 and WP-18; WP-25 |
| A2-7 | major | [CFG §10.5], [LQ/errors §5.4] | the answer to a budget request above the role's ceiling is stated nowhere | R-SPEC-F, WP-19 and WP-18 |
| A2-8 | major | [API §18] | the needs-triage note of an agent that holds several task leases | R-SPEC-F, WP-25 |
| A2-9 | major | [LQ/std §5] `pack_hazards` against [RULES/pack-classes] PM-023, PM-024, CL-007 | C7 holds decisions by the rules and not by the query | R-SPEC-F, WP-19; R-MODEL, WP-90 |
| A2-10 | minor | the header Status rows; `HOLES.md` §3, §4; `COVERAGE.md` open point 5; [F01] open point 15; [F21] open point 11 | status rows and "until pass 2" records | R-SPEC-F, WP-10; each chapter's role |
| A2-11 | minor | `docs/spec/README.md` index; [CFG] RG-6, open point 35 | `rules/policy-keys.md` still called planned | R-SPEC-F, WP-10 and WP-18 |
| A2-12 | minor | chapter headers; `docs/m0/authors.md` §2 | header work packages the ledger does not list | R-HARN-I, WP-01; the chapters' roles |
| A2-13 | minor | [LQ/errors §1.1], [F19 §1.2] | stale LQ code ranges | R-SPEC-F, WP-19 and WP-18 |
| A2-14 | minor | [F19 §10.2] `harvest_overlap`; [CFG §6.2] CFG06, CFG10 | a non-ASCII ellipsis inside three templates | R-SPEC-F, WP-18 |
| A2-15 | minor | [F19 §2.4] against [F19 §10.2], [CFG §6.2] | byte quantities spelled three ways | R-SPEC-F, WP-18 |
| A2-16 | minor | [CFG §10.4] `files.max-line-hashes` | a model-function name that [RULES/policy-keys] spells otherwise | R-SPEC-F, WP-18 |
| A2-17 | minor | [LQ/card §7.3] | "step 6 attains" the gate by a rounding | R-SPEC-F, WP-19 |
| A2-18 | minor | [F21 §6.5], [F21] open point 8 | stale pointer to [F19]'s texts | R-SPEC-R, WP-14b |
| A2-19 | minor | [F14 §5.6], [F14 §17.2] | a scope no scanner can record | R-SPEC-R, WP-15 |
| A2-20 | minor | [F13 §3.9] I-F5 | the read-verb list omits two verbs [F18 §2.5] lists | R-SPEC-P, WP-16 |
| A2-21 | minor | [F19 §12.4] | a `HierarchyCycle` on a write is reported as the depth rule | R-SPEC-F, WP-18 and WP-19 |
| A2-22 | minor | `COVERAGE.md` §1 | the verification record predates the rows it covers | R-SPEC-F, WP-10 |
| A2-23 | minor | [LQ/std] open point 3, [F19] open point 17, [RULES/role-write-policy] open point 17 | questions still left to "the review" | R-SPEC-F, WP-19 and WP-18; R-MODEL, WP-90 |

## 4. Findings

### Majors

**A2-1 — 81 blank fixture cells (major).** Where: `COVERAGE.md` §3–§11, column `fixture`
(`docs/spec/COVERAGE.md:22`, the column "blank until R-FIX and R-MODEL fill them"; E1, [PLAN §7]). Problem: under
`coverage --strict`, the mode for this pass and the freeze, every blank cell is a finding, and E1 requires no unmapped
row. Evidence: `coverage: 152 rows, 168 findings; 81 blank fixture cells and 87 blank model cells (--strict: findings)`.
The rows: `60-AR-Layout-dir`, `60-AR-Layout-config`, `60-AR-Layout-pointer`, `60-AR-Layout-names`,
`60-AR-Layout-userconf`, `60-I2-FM(2)`, `60-I2-FM(3)`, `60-I2-FM(4)`, `60-I2-FM(5)`, `60-I2-FM(6)`, `60-I2-FM(8)`,
`60-I2-FM(9)`, `60-I2-FM(10)`, `60-I2-FM(11)`, `60-I2-PD(a)`, `60-I2-PD(c)`, `60-I2-PD(e)`, `60-I2-PD(f)`, `60-I2-PD(j)`,
`60-I2-PD(k)`, `60-I2-PD(l)`, `60-I2-PD(m)`, `60-I2-PD-added`, `60-I2-StoreParams`, `60-I2-Derived`, `60-AU-HEAD-rmw`,
`60-AU-Commit-actor`, `60-AU-Commit-digest`, `60-AU-Commit-inline`, `60-AU-Seg-markers`, `60-AU-RefTable`,
`60-AU-Schema-lane`, `60-AU-Canon-digest`, `60-AU-Canon-lineorder`, `60-AU-Canon-anchordigest`, `60-AU-Vfs-classes`,
`60-AU-Vfs-renames`, `60-AU-Vfs-sims`, `60-AU-R14-E3d`, `60-AU-R14-reuse`, `60-AU-R14-E6`, `60-AU-R14-twin`,
`60-AU-R14-copy`, `60-AU-R14-denials`, `60-AU-Config`, `60-AU-CrossPlatform`, `60-AU-Harness`, `60-PA-(a)`,
`60-PA-(c)`, `60-PA-(g)`, `60-PA-(h)`, `60-PA-(i)`, `60-PA-NewParams`, `R-5`, `R-6`, `R-12`, `R-13`, `R-16`, `R-17`,
the R5 rows `F1`, `F2`, `F4`, `F9`, `F10`, `F11`, `F13`, `F14`, `F15`, `F16`, `F17` of §9, `X-F4`, `X-F5`, `X-F6`,
`X-F10`, `X-F11`, `X-F12`, `90-LEASES`, `90-CommitHeader`, `90-Output`, `90-Errors`, `90-Card`. Several rows need no byte
fixture in the sense of the column: the summary rows `60-AU-CrossPlatform` and `60-AU-Harness`, whose parts are the X-F
and [90 §10.1] rows (`docs/spec/COVERAGE.md:18-19`), the meta row `60-I2-PD-added`, and the fault-model, protocol and
`Vfs` rows (`60-I2-FM(…)`, `60-I2-PD(…)`, `60-PA-(…)`, `60-AU-Vfs-*`, `X-F4`, `X-F5`), whose check is the simulator and
the seeded bugs; the tool counts `n/a` as blank. Fix: R-FIX, WP-20 fills the cells with the fixtures that exist or that
WP-20b adds, and R-SPEC-F, WP-10 copies them at the next sync (R-FIX may not write `docs/spec/`). Which rows need no
fixture, and how such a cell is written so that `--strict` accepts it, is an owner call. Disposition: open.

**A2-2 — 87 blank model cells (major).** Where: `COVERAGE.md` §3–§11, column `model function`
(`docs/spec/COVERAGE.md:29-34`). Problem: as A2-1, for the model column. Evidence: the same count line. The rows:
`60-pre`, `60-AR-LOCK`, `60-AR-HEAD`, `60-AR-Log-RecHdr`, `60-AR-Log-chain`, `60-AR-Log-kinds`, `60-AR-Ops`,
`60-AR-Canonical`, `60-AR-Seg-hdr`, `60-AR-Seg-runtime`, `60-AR-Seg-branch`, `60-AR-Seg-sealed`, `60-AR-Image`,
`60-AR-Layout-dir`, `60-AR-Layout-config`, `60-AR-Layout-pointer`, `60-AR-Layout-names`, `60-AR-Layout-userconf`,
`60-I2-FM(1)` to `60-I2-FM(12)`, `60-I2-PD(a)`, `60-I2-PD(b)`, `60-I2-PD(c)`, `60-I2-PD(d)`, `60-I2-PD(f)`,
`60-I2-PD(j)`, `60-I2-PD(k)`, `60-I2-PD(l)`, `60-I2-PD(m)`, `60-I2-PD-added`, `60-I2-Gate0`, `60-AU-HEAD-fields`,
`60-AU-HEAD-bootrecovery`, `60-AU-HEAD-bootid`, `60-AU-HEAD-rmw`, `60-AU-LOCK`, `60-AU-Log-validity`, `60-AU-Log-kinds`,
`60-AU-Log-TreeReg`, `60-AU-Commit-actor`, `60-AU-Commit-csref`, `60-AU-Commit-inline`, `60-AU-Seg-cs`,
`60-AU-Seg-derivedopt`, `60-AU-Seg-leases`, `60-AU-Seg-alloc`, `60-AU-Seg-r4rt`, `60-AU-RefTable`, `60-AU-Schema-lane`,
`60-AU-Canon-digest`, `60-AU-Image`, `60-AU-Vfs-classes`, `60-AU-Vfs-renames`, `60-AU-Vfs-sims`, `60-AU-R14-denials`,
`60-AU-Config`, `60-AU-CrossPlatform`, `60-AU-Harness`, `60-PA-(a)`, `60-PA-(b)`, `60-PA-(c)`, `60-PA-(g)`, `60-PA-(h)`,
`60-PA-(i)`, `R-6`, `R-8`, `R-11`, the R5 row `F5`, `X-F1`, `X-F3`, `X-F4`, `X-F5`, `X-F6`, `X-F10`, `X-F12`, `90-Card`,
`90-Codec`. Many of them are byte layouts the model does not produce (`HEAD`, `LOCK`, segment, image and codec bytes;
[60 §4.3]: `HEAD` bytes and `.moi` bytes are outside the model's scope) or summary rows. Fix: R-MODEL, WP-90 to WP-94
tags the model functions that implement each row's sections, and R-SPEC-F, WP-10 copies the cells. Which rows need no
model function, and how such a cell is written, is an owner call. Disposition: open.

**A2-3 — The R5 rows' model cells name functions of other items (major).** Where: `COVERAGE.md` §9
(`docs/spec/COVERAGE.md:222-239`), filled by the rule of `docs/spec/COVERAGE.md:29-34` ("a `spec:` tag that cites one of
the row's citations ... or the row's item id"). Problem: the row ids `F1` to `F18` of §9 are also prefixes of the chapter
tags `[F10 …]` to `[F18 …]` and suffixes of the invariant labels I-F1 to I-F14, so a tag that names a chapter or an
invariant reads as the row's item id. The cells show it: row `F2` (schema field columns, `docs/spec/COVERAGE.md:223`)
names four anchor-matching functions; rows `F4`, `F6`, `F7`, `F8`, `F9` and `F10` name the checks of invariants I-F4,
I-F6, I-F7, I-F8, I-F9 and I-F10; row `F12` (full-text search, `docs/spec/COVERAGE.md:233`) names the ref-name checks of
[F12 §2]; row `F13` (`PATHIDX`, `ALIASIDX`, `ANCHORS`) names the derived-state functions of [F13]; row `F18`
(`docs/spec/COVERAGE.md:239`) names an anchor resolver beside the validator. A filled but wrong cell hides a blank from
`--strict` and from E1, so the 87 of A2-2 understate the gap. Evidence: the cells as cited; the model functions were not
read. Fix: R-SPEC-F, WP-10 and R-MODEL, WP-90, one of two remedies: (a) row ids that cannot collide (for example `R5-F1`)
with tags naming them, the cells re-filled; or (b) the cells re-filled from tags that cite one of the row's chapter
sections only, without the item-id route for these rows. The remedies change no byte. Disposition: open.

**A2-4 — Who E411 refuses (major).** Where: [F19 §11.1] "When" (F19:924-927) and [LQ/errors §5.5] E411
(`docs/spec/lq/errors.md:445`) against [API §9.1] (`docs/spec/store-api.md:998`) and [RULES/role-write-policy] WR-012
(`docs/spec/rules/role-write-policy.md:92`). Problem: [API] and WR-012 refuse a free-form `TX` under the `unknown` model
profile only for a caller with a session identity (CX-4) or with `door` = `mcp`, so that the CLI block of example 02
without a session is not refused. [F19 §11.1], which states the condition, and [LQ/errors], which owns the text, say
"the call is a free-form `TX`" with no such scope. An implementer of the CLI who follows [F19] refuses example 02's block
with exit 6 where the model writes it with exit 0. Evidence: the cited rows. Fix: R-SPEC-F, WP-18 states the scope in
[F19 §11.1] by citing [API §9.1] and WR-012, and R-SPEC-F, WP-19 does the same in E411's row of [LQ/errors §5.5].
Disposition: open.

**A2-5 — No text for a refused `claim` (major).** Where: [API §10.1] "Refusals" (`docs/spec/store-api.md:1289-1290`) and
its open point 12 (`docs/spec/store-api.md:2287`: "[LQ/errors] adds the case text"); [LQ/errors §5.5] E404
(`docs/spec/lq/errors.md:431`). Problem: a `claim` of a task that is not live or not ready is E404 "naming the failing
clause (live, `unblocked`, leased by `<holder>`, done or deleted on `<branch>`, deferred until)", but E404's row has only
the transition form `statement <i>: <id> <from> -> <to> refused: <why>` and the GR-019 and `PATCH` forms; a claim moves no
status, so no form fits, and the clauses have no wording. `claim` is the verb agents run most; its refusal is the text an
agent reads to decide what to do next, and texts freeze with the query surface. Evidence: `grep -n -i 'not ready\|leased
by'` over [LQ/errors] and [LQ/std] finds only E409's lease case. Fix: R-SPEC-F, WP-19 adds E404's claim case to
[LQ/errors §5.5]: a message per clause, the help (for example a later `claim --next`), and the JSON keys of §5.7.
Disposition: open.

**A2-6 — Three notices with no code, text or JSON form (major).** Where: [F12 §9.4] step 2 (F12:1072-1073: "the command
prints one notice per stale key"); [F18 §2.10] and its open point 25 (F18:245-247, F18:1010-1012: "a notice marks the
first result after a resolver-version change ([LQ/errors] numbers the notice; WP-19)"); [CFG §7.6] rule `main-tree`
(`docs/spec/config.md:571-574`: a failure to write the user entry "is a notice"). Problem: [LQ/errors §5.6] has notices
N01 to N13 and [F19 §10.4] its warnings, and none of them is any of these three; [API §11.8] gives `MergeContinue` the
result of `Merge`, which has no member for stale keys. A user whose staged resolution did not apply learns it only from
a notice that has no text; the resolver-version notice was delegated to [LQ/errors] and not written. Evidence: the
cited lines; searches over [LQ/errors] and [F19] for the three cases find none. Fix: R-SPEC-F, WP-19 numbers and words
the stale-resolution and resolver-version notices in [LQ/errors §5.6] (N14 is the next free code) or R-SPEC-F, WP-18
makes them warnings of [F19 §10.4]; R-SPEC-F, WP-18 gives `init`'s notice its text; R-SPEC-F, WP-25 adds the stale keys
to `MergeContinue`'s result in [API §11.8]. Disposition: open.

**A2-7 — A budget request above the role's ceiling (major).** Where: [CFG §10.5] (`docs/spec/config.md:810-813`: "A
requested value above the ceiling is handled as [LQ/errors] states (open point 13)"); [CFG] open point 13
(`docs/spec/config.md:1150-1155`: "proposal: clamp and show the applied value in the `budget` object, with a notice");
[RULES/policy-keys] KF-015 (`docs/spec/rules/policy-keys.md:100`: "clamped to the role's ceiling"); [LQ/errors §5.4]
(`docs/spec/lq/errors.md:402`). Problem: [LQ/errors] states no rule for a `--budget k=v` above `query.caps.<role>.k`:
it has no refusal and no notice for it, and only E304's help mentions a ceiling. The model's table clamps; an engine
could refuse with exit 2, clamp silently or clamp with a notice, and an agent cannot tell its raise was cut. Evidence:
`grep -n 'ceiling\|clamp'` over [LQ/errors] gives lines 402, 405 and 640, none of them the rule. Fix: R-SPEC-F, WP-19,
with WP-18 for [CFG]'s pointer, one of two remedies: (a) clamp to the ceiling and report it with a notice and the applied
value in `budget`, as [CFG] open point 13 proposes and KF-015 computes; or (b) refuse with exit 2 and a text naming the
ceiling. Disposition: open.

**A2-8 — The needs-triage note of an agent that holds several task leases (major).** Where: [API §18]
(`docs/spec/store-api.md:2128-2133`): the note's title `needs triage: <agent> stopped holding <lease>` and "an `ABOUT`
edge to the lease's task"; [RULES/policy-keys] open point 6 (`docs/spec/rules/policy-keys.md:391-393`: "[API §18] names
one `<lease>` in the title and one task for the `ABOUT` edge, while a stopping agent may hold several task leases ... and
[API §18] should say which it is"). Problem: the case is open: one note per released lease, or one note for all of them
with which title and which `ABOUT` edges. The hook writes through `Mutation` `tx.remember`, so the note is versioned
data and its commit id differs between the two readings. Evidence: the cited lines. Fix: R-SPEC-F, WP-25 states one of
two remedies in [API §18]: (a) one note per task lease the stop releases, each with its lease and task; or (b) one note
naming every lease, with an `ABOUT` edge to each task. R-MODEL, WP-90 follows ([RULES/policy-keys] open point 6).
Disposition: open.

**A2-9 — C7 holds decisions by the rules, not by the query (major).** Where: [LQ/std §5] `pack_hazards`
(`docs/spec/lq/std.md:880-892`: `MATCH (k:note|rule)` with `k.status = 'active'` in both parts) against
[RULES/pack-classes] PM-023, PM-024 (`docs/spec/rules/pack-classes.md:224-225`: kinds `note, rule, decision`) and CL-007
(`docs/spec/rules/pack-classes.md:164`: "Notes, rules and decisions scoped to or anchored in T's files");
`pack_hazards_proposed` takes decisions in its `AT` part. [LQ/std] open point 20 (e) records the difference
(`docs/spec/lq/std.md:1194-1196`) as a finding for R-MODEL and the review. Problem: an accepted decision anchored in a
task's files enters C7 by the rule table and not by the query, so the model and the engine render different packs, and
the decision's text is either spent or missing in every pack of that task. The design's C7 reads "scoped
hazards/notes" ([AR §7.4]). Evidence: the cited rows. Fix: one of two remedies: (a) R-SPEC-F, WP-19 adds `decision` to
`pack_hazards`' `AT` part with its authoritative status (`accepted`), and `fixtures/lq/std/pack_hazards.lq` follows
(R-FIX); or (b) R-MODEL, WP-90 drops `decision` from PM-023, PM-024 and CL-007, and `pack_hazards_proposed` follows.
Disposition: open.

### Minors

**A2-10 — Status rows and "until pass 2" records (minor).** Where: the Status rows of the chapter headers against
[F01 §2.3] (F01:70); `HOLES.md` §3 and §4 (`docs/spec/HOLES.md:127`, `docs/spec/HOLES.md:139`); `COVERAGE.md` open point
5 (`docs/spec/COVERAGE.md:298`); [F01] open point 15 (F01:719); [F21] open point 11 (F21:1243-1249). Problem: 53 of the
reviewed set's 55 `.md` files read "draft, pass 1 pending" though pass 1 is closed and signed; `rules/policy-keys.md`,
written after pass 1 closed, reads "draft, pass 1 pending; needs the owner's signature"
(`docs/spec/rules/policy-keys.md:5`) where [F01 §2.3] gives "draft, pass 2 pending" for that case, as F21 uses (F21:7);
the old and new hole ids are kept "until pass 2", which has now come; F21 open point 11 is a "Proposal for pass 2" (a
TOML anchor's header) that this pass cannot adopt, since it changes [F20 §2.8]'s hashed header text. Evidence: 53 files
with the plain row; the cited lines. Fix: R-SPEC-F, WP-10 for `README.md`, `HOLES.md`, `COVERAGE.md` and [F01]; each
chapter's role for its own Status row at the owner's disposition of pass 2 (R-SPEC-F WP-10, WP-12, WP-14, WP-18, WP-19,
WP-25; R-SPEC-P WP-11, WP-16, WP-17; R-SPEC-R WP-13, WP-14b, WP-15; R-MODEL WP-90 for the rule files). F21 open point 11
is an owner call (a hashed header text). Disposition: open.

**A2-11 — `rules/policy-keys.md` still called planned (minor).** Where: `docs/spec/README.md:131` (the index lists it
"planned", without a link); `docs/spec/config.md:641-643` (RG-6: "a rule file ... that the index lists as planned");
`docs/spec/config.md:1219-1220` ("`[RULES/policy-keys]` stays cited as a planned file"). Problem: the file exists (393
lines) and is reviewed here. Fix: R-SPEC-F, WP-10 links it in the index with its status, and R-SPEC-F, WP-18 drops
"planned" from RG-6 and open point 35. Disposition: open.

**A2-12 — Header work packages that the ledger does not list (minor).** Where: the Work package rows of the chapter
headers against `docs/m0/authors.md:26-101` (§2). Problem: the headers name WP-12a, WP-12b, WP-12c, WP-13a, WP-13b,
WP-14a, WP-16a, WP-16b, WP-16c, WP-17a, WP-17b, WP-18a (`docs/spec/config.md:8`), WP-18b (F19:8), WP-19a, WP-19b
(`docs/spec/lq/card.md:8`, `docs/spec/lq/errors.md:8`, `docs/spec/lq/envelope.md:8`) and WP-90b
(`docs/spec/rules/policy-keys.md:6`); §2 lists none of them, and WP-14b, which F21's header names (F21:8), carries the
path `format/20` only. A commit subject must name a ledger WP, so the headers do not say which WP a fix of the file
uses. Fix: one of two remedies: (a) R-HARN-I, WP-01 adds the sub-packages, and `format/21` to WP-14b's paths, to
`docs/m0/authors.md` §2; or (b) each chapter's role writes the ledger WP in its header with the part in words.
Disposition: open.

**A2-13 — Stale LQ code ranges (minor).** Where: [LQ/errors §1.1] (`docs/spec/lq/errors.md:14-15`: "`N01`–`N12`") and
[F19 §1.2] (F19:32: "`W01`–`W09`, `N01`–`N12`"). Problem: N13 is in [LQ/errors §5.1] (`docs/spec/lq/errors.md:304`) and
W10 since the A1 re-review; [F19 §7.3] already reads W01–W10, N01–N13 (F19:469), and [F19] open point 40 says the range
was corrected. Fix: R-SPEC-F, WP-19 and WP-18 correct the two ranges. Disposition: open.

**A2-14 — A non-ASCII ellipsis inside three templates (minor).** Where: [F19 §10.2] `harvest_overlap` (F19:725:
`…, also in this batch`); [CFG §6.2] CFG06 and CFG10 (`docs/spec/config.md:404`, `docs/spec/config.md:408`). Problem: the
ellipsis stands for the template's first part, but it is written inside the code span of the template, and [F19 §2.3]
rule 2 (F19:95-96) bars `…` from every template; [CFG §6.2] says each message is one ASCII line. An implementer who
copies the span prints a non-ASCII byte. Fix: R-SPEC-F, WP-18 writes each second form out in full. Disposition: open.

**A2-15 — Byte quantities spelled three ways (minor).** Where: [F19 §2.4] (F19:114: "decimal with `,` every three digits
from the right, then ` B`") against F19:668 (`(<n> bytes)`), F19:729 (`larger than 4,096 bytes`), F19:747 (`<n> bytes
but its header says <m>`), F19:751 (`needs <n> bytes`) and CFG01 (`docs/spec/config.md:399`: `larger than 262144
bytes`). Problem: the chapter that fixes the spelling does not use it in its own templates, so a golden cannot check the
rule. Fix: R-SPEC-F, WP-18, one of two remedies: (a) the templates use §2.4's form; or (b) §2.4 restricts its form to
headers, footers and budget lines and names the form of error texts. Disposition: open.

**A2-16 — A model-function name that the policy table spells otherwise (minor).** Where: [CFG §10.4]
`files.max-line-hashes` (`docs/spec/config.md:801`: model `anchors::window_available`) against [RULES/policy-keys]
KY-057 (`docs/spec/rules/policy-keys.md:198`: `links::window_available`). Problem: [CFG §9.4] makes the policy table
normative for these names and says [CFG] follows it (`docs/spec/config.md:687-690`); this is the one row of 36 that
does not. Fix: R-SPEC-F, WP-18 writes `links::window_available`. Disposition: open.

**A2-17 — "Step 6 attains" the token gate by a rounding (minor).** Where: [LQ/card §7.3]
(`docs/spec/lq/card.md:169-171`). Problem: at 1,160 tokens the gate needs 160/1,160 = 13.79 % of the body, 429.8 B;
step 6 saves 429 B (13.77 %), which leaves about 1,000.3 tokens at a constant ratio, so step 7 is the first that reaches
the gate. The hole's range still has a candidate inside the estimate. Evidence: the card's recomputed counts (§1). Fix:
R-SPEC-F, WP-19 names step 7, or writes the margin. Disposition: open.

**A2-18 — A stale pointer to [F19]'s texts (minor).** Where: [F21 §6.5] (F21:737-740: "([F19] text: open point 8)") and
[F21] open point 8 (F21:1229-1231: the refusals "that have none yet"). Problem: [F19 §10.2]'s `anchor_spec` rows now hold
all four texts (F19:682-685). Fix: R-SPEC-R, WP-14b cites [F19 §10.2] and closes open point 8. Disposition: open.

**A2-19 — A scope no scanner can record (minor).** Where: [F14 §5.6] (F14:408) and the worked example of [F14 §17.2]
(F14:1744, F14:1753, F14:1760): `rust:struct LockFile/impl LockFile/fn acquire`. Problem: [F21 §3.8] makes an `impl` a
sibling of the type it implements, never its child (F21:438-439), so no capture records this scope and it never
resolves; as an imported value it is legal bytes, but fixtures built from the worked example carry a scope that
narrows nothing. Fix: R-SPEC-R, WP-15 writes `rust:impl LockFile/fn acquire` and recomputes §17.2's values. Disposition:
open.

**A2-20 — I-F5's read verbs (minor).** Where: [F13 §3.9] I-F5 (F13:238: `show`, `pack`, `brief`, `get`, `find`, `q`,
`links check`, MCP reads) against [F18 §2.5] (F18:157), which also lists `links mentions` and `file where`. Problem: the
enforcement table's list is the one a gate author reads first. Fix: R-SPEC-P, WP-16 cites [F18 §2.5]'s list or adds the
two verbs. Disposition: open.

**A2-21 — A `HierarchyCycle` on a write is reported as the depth rule (minor).** Where: [F19 §12.4] (F19:1061:
`HierarchyCycle`, `DepthExceeded` -> E405 `forest depth 12 (I4)`); [LQ/errors §5.5] E405 (`docs/spec/lq/errors.md:432`).
Problem: a `MOVE` that closes a cycle in the `parent` forest is told it broke a depth rule; the detail line shows the
cycle, but the rule an agent reads first names the wrong cause. Fix: R-SPEC-F, WP-19 adds a rule text such as
`forest without cycles (I4)` to E405 and R-SPEC-F, WP-18 maps `HierarchyCycle` to it. Disposition: open.

**A2-22 — `COVERAGE.md`'s verification record predates its rows (minor).** Where: `COVERAGE.md` §1 "Verification"
(`docs/spec/COVERAGE.md:40`: "Every citation below was opened on 2026-09-28"). Problem: rows changed after that date
(for example R-4's and R-14's [F21] parts and the spec sync 3 parts of `60-AR-Log-kinds`); §2 records later rounds of
pass 1 only. The citations do resolve (N-A8). Fix: R-SPEC-F, WP-10 dates the record at its last full check.
Disposition: open.

**A2-23 — Questions still left to "the review" (minor).** Where: [LQ/std] open point 3
(`docs/spec/lq/std.md:1129-1131`: `area:net` is E110 "until the review decides whether areas are addressable by name");
[F19] open point 17 (F19:1240-1241: an imported named query whose `#u:` literal the store does not know stages
`QueryInvalid`; "the review decides whether that is intended"); [RULES/role-write-policy] open point 17
(`docs/spec/rules/role-write-policy.md:476-478`: any caller may write a `critical` note, which then reaches every brief).
Problem: each is a behaviour choice an agent meets (a refused argument form, a staged import, a brief's byte budget
spent by any writer); a review proposes and does not decide. Fix: owner call on each; R-SPEC-F, WP-19 and WP-18 and
R-MODEL, WP-90 write the outcome. Disposition: open.

## 5. Known open findings met

Cited, not re-raised:

- The R-SPEC-F item on [F19 §10.2]'s `body_dropped` row and DB-7 (c)'s `PATCH` case: the base holds the case and its
  text (F19:724).
- The R-SPEC-F item on whether [API §2.3]'s `dry` covers the admin verbs: [API §8.7] `BodyDrop` still says nothing of
  `ctx.dry`.
- The R-SPEC-F item on "They" (`docs/spec/store-api.md:1864`): unchanged.
- The status rows (53 files): rated once here, as A2-10.
- The R-MODEL item on an automatic side over a node with an absent flat side: met while reading [F12 §6.3]; not rated.

## 6. Checked and found correct

- The byte counts and recomputations of §1, and by hand: [F19 §4.2]'s examples (base parts of 75 B and 58 B, extras
  16 B, continuation 28 B, `DRY` extras 57 B) and its `files` part bound (80 B); every marker form of [F19 §4.6] (at most
  49 B); the E406 one-line text (152 B, F19:962); the proposed line (84 B; 145 B at its bound); [LQ/errors §3.4]'s bound
  check (40 + 77 + 17 + 22 B, a message of at least 443 B); `CursorHead` (36 B); F21's two scope encodings (25 B, 19 B),
  its `canon` vectors, its 51 keywords and its golden constructs 1–27, M1–M14 and T1–T12 as checked; [F06 §11]'s
  `SetStatus` frame with a known and an unknown locator (10 and 9 bytes); [F11 §14]'s `MARKERS` row (100 B body); [API
  §17.2]'s order keys; [LQ/lexical §6.1]'s 44 reserved words.
- [API §19]'s changed examples 02, 07 and 08 render a `create` with a position as two statements, as [API §9.3] states.
- The closure of spec sync 3 (`spec-sync-3-closure-a.md`).
- [API] open point 10's idempotency exemptions (`LinksSync`, `Sync`, `Merge`, `MergeContinue`, `Undo` explicit-key only):
  each retry without a key is safe by the command's own checks, as stated; confirmed.
- [LQ/errors] open point 3: [AR §7.1]'s `error[guard_conflict]` example is illustrative; confirmed.

## 7. Open points for the disposition

1. **The A1 dispositions' status cell** (`docs/spec/reviews/a1-dispositions.md:6`: "the owner signs the dispositions in
   WP-80"): the owner's record; no lens role may edit the file.
2. **Lifting F21's interim scanner rule** (F21:1193-1198): this lens raises no blocker or major against F21 (N-A5).
3. **The text goldens of `fixtures/lq/`** (`fixtures/lq/cases/envelope.cases`, `errors-text.cases`): `fixtures/lq/INDEX.md:14`
   says the product's M7 and M8 goldens must reproduce them, while the A1 disposition reviews only the renderer's goldens
   in pass 2. Whether that review also covers these R-FIX goldens is the owner's call.
4. **[F16] open point 12** (F16:1385-1387): "the review decides whether [F19] adds a notice" for a reader that cannot run
   boot-change recovery. This lens proposes a notice, as for A2-6.
5. **[F12] open point 29** (F12:1277-1283: an adversarial text key against the merge gate): to be decided before
   GT11's merge row is written.
