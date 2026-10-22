# Review pass 2, lens S: the specification

| Field | Value |
|---|---|
| Title | Independent review pass 2 of `docs/spec/` (format chapters 01–21, `os/`, `lq/`, `rules/`, `store-api.md` with its examples, `config.md`, `COVERAGE.md`, `HOLES.md`, `README.md`), lens S (semantics and correctness) |
| Status | review, pass 2; every finding awaits its disposition by the owning role named in its Fix |
| Work package | WP-80 ([PLAN §3.2] item 8), lens R-REV-S. The author of this file wrote none of the reviewed text (S5) and did not read the pass-2 findings of the other lenses |
| Lens | S: semantics and correctness, with [21 §0]'s defect classes WRONG ANSWER, DATA LOSS, DIVERGENCE and UNDERSPECIFIED |
| Reviewed | every file of `docs/spec/` outside `docs/spec/reviews/`, except `docs/spec/measurement-protocol.md`: 76 files at the base (§1). Pass 1's signature covers the text of aa7c67b, the rule tables excepted; this pass reads every change since then, the two files new since then, and the named checks of §2 |
| Base | 8bb4b6975fc808caf7fbe636556d25d4aa104c08 |
| Checked against | [AR §3.4], [AR §5a.5]–[AR §5a.8], [AR §13]; [40 §2.3], [40 §2.10]; [50 §5.5]; [60 §2.5], [60 §3.4], [60 §3.13]; [80 §2.4]; pass 1's reports and closure (`docs/spec/reviews/`); the owner's records in `docs/spec/reviews/owner-questions.md` |

Severity, as WP-80 defines it for the passes:

- **blocker**: two independent implementers would produce different bytes, or a rule of the design of record is
  violated;
- **major**: an ambiguity, a missing case or an inconsistency that must be fixed before the freeze;
- **minor**: editorial, or local with no byte or rule consequence.

Line numbers are at the base. Citations are `Fnn:<line>` for `docs/spec/format/nn-*.md` and the repository path
otherwise.

## 0. Verdict

**CHANGES REQUIRED: 0 blockers, 4 majors, 11 minors (15 findings).**

The changes since pass 1 are, again, careful within each chapter: every worked example this pass recomputed is right
(§6), every protocol rule of [F16] has its seeded bug, every invariant of [F13] has an enforcement point and a gate,
the item-1a texts agree with each other, and the owner's answer OQ-A-14 is stated as one outcome by every text that
carries it. The new scope-scanner chapter [F21] has no blocker or major finding of this lens. The four majors are
again between texts:

1. **`--policy` has two value sets** (S2-1): four texts give it every automatic-policy value, two give it
   `delete-wins` and `resurrect` only, and the staged commit's bytes hold only the second.
2. **Store A's staged commit** (S2-2): its ops chain into another ref, against [F06 §7.3].
3. **The commutation property** (S2-3) leaves out the automatic policies.
4. **I-F7 against the settle** (S2-4): a rule row lets a settle write `removed` through a door the invariant does not
   list, while [F18]'s own I-F1 text admits it.

Of the minors, S2-5 answers the one-parent question of [F12 §6.5]: the restore names "the merge that made the
conflict value" and its second parent, and no text says what a revert or cherry-pick that made one restores from.

## 1. Method

1. Depth (one row per file below): **full** is every line; **delta** is every changed hunk since aa7c67b and every
   cross-reference into it, rule rows row by row; **lens** is pass 1's "read for the lens" (these files did not change
   since aa7c67b). Item 1a's lines in [F06], [F07], [F16] and [F17] were read at full depth. [F12] and [F13] were read
   whole.
2. Integration: for every datum a change touched, the chapters that lay it out were compared field by field (the
   policy override, the `prev` chain, the scope value, the anchor derivations, the error codes, the record kinds).
3. Mechanical checks, each with its result line:
   - every `HOLE(<id>)` of the reviewed set against `HOLES.md`: 54 distinct ids, all indexed;
   - the 20 API examples parsed as JSON: 20 of 20 parse; every commit id is `c` + 64 or `c` + 8 hex digits;
   - the [F13 §3] entries: 58, every one with a gate; two enforcement points without a check class (S2-6);
   - the [F16 §17] catalogue: 102 rules, 102 rows of §17.3, no rule without a row and no row without a rule; 10 rows of
     §17.4; no empty detector or vehicle cell;
   - the worked examples that the Klotho renames changed, recomputed with `cargo xtask hex`: [F14 §17.2]'s file uid,
     root uid, both `captured` values, both anchor uids, `quote_h`, `prefix_h`, `suffix_h` and window; [F07 §16]'s
     BLAKE3-128 of the empty string (§6);
   - the old byte names: exactly the six known sites (the known state).
4. Every search excluded the other lenses' pass-2 files by file name.

| File | Depth |
|---|---|
| `COVERAGE.md` | delta |
| `HOLES.md` | delta |
| `README.md` | delta |
| `config.md` | delta |
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
| `format/12-vcs.md` | full |
| `format/13-invariants.md` | full |
| `format/14-image.md` | delta |
| `format/15-fault-model.md` | delta |
| `format/16-protocol.md` | delta |
| `format/17-store-parameters.md` | delta |
| `format/18-file-links.md` | delta |
| `format/19-errors-and-output.md` | delta |
| `format/20-r4-resolver-constants.md` | delta |
| `format/21-scope-scanners.md` | full |
| `lq/canonical-ast.md` | delta |
| `lq/card.md` | delta |
| `lq/envelope.md` | delta |
| `lq/errors.md` | delta |
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
| `store-api.md` | delta |
| `store-api/examples/01-init-and-clock.json` | lens |
| `store-api/examples/02-tx-create.json` | delta |
| `store-api/examples/03-set-guard.json` | delta |
| `store-api/examples/04-claim-complete.json` | lens |
| `store-api/examples/05-idempotency.json` | lens |
| `store-api/examples/06-rm-policy.json` | lens |
| `store-api/examples/07-lane-merge.json` | delta |
| `store-api/examples/08-merge-staged.json` | delta |
| `store-api/examples/09-undo.json` | lens |
| `store-api/examples/10-caller-context.json` | lens |
| `store-api/examples/11-state-snapshot.json` | lens |
| `store-api/examples/12-runtime-snapshot.json` | delta |
| `store-api/examples/13-file-mv.json` | lens |
| `store-api/examples/14-apply-batch.json` | lens |
| `store-api/examples/15-lease-clock.json` | lens |
| `store-api/examples/16-image.json` | lens |
| `store-api/examples/17-query.json` | lens |
| `store-api/examples/18-maintenance.json` | lens |
| `store-api/examples/19-schema-migrate.json` | lens |
| `store-api/examples/20-vcs-refs.json` | lens |

## 2. Named checks

| Id | Check | Verdict |
|---|---|---|
| N-S1 | [F12 §6.5]: a conflict value whose maker M has one parent; is `theirs` defined for the restore of a live side? | MISSES: S2-5 |
| N-S2 | [F12 §7.9]'s `HierarchyCycle` key (F12:962) against [F13 §5] V05's outcome (F13:347) and [F19 §12.2] code 64 (F19:1022) | MEETS: all three key every cycle V05 finds by the hierarchy key of its least uid; V05 and code 64 list the same three sources of a cycle |
| N-S3 | [F05 §9.5]'s marker entry order (F05:658-662) with [F13 §4.2] MC-6 (F13:329): total, stable, stated once | MEETS: one order, stated once and cited by MC-6 and [F05 §10.1]; per `#N` a record has at most one leaving and one joining entry, so the order is total; folds write no entry, so it is stable |
| N-S4 | Store A's commit c11 (`fixtures/hex/store-a/log.1.hex`) against [F06 §7.3] (F06:724) and SL-2 (F06:777) | MISSES: S2-2 |
| N-S5 | [F21] and the interim scanner rule (F21:1193-1198; F20:1193-1197): every blocker and major against [F21] | MEETS: this lens has no blocker or major against [F21]; its two minors are S2-11 and S2-12 |
| N-S6 | Review a1-S's deferred minors S-16 and S-22 (`docs/spec/reviews/a1-dispositions.md:63`, `docs/spec/reviews/a1-dispositions.md:69`) | MISSES: S2-8. S-16 is closed in the text ([F20] open point 15, F20:1467-1480; pass 1's S1-31, closed by the owner). S-22 is deferred with its owner and milestone in [F09] OP-09-12 (F09:1390-1392), and [LQ/envelope] gives it to another owner (S2-8) |
| N-S7 | Pass 1's editorial residues (`docs/spec/reviews/pass1-closure.md:225-236`, `docs/spec/reviews/pass1-closure.md:322-326`) | MISSES: S2-9, S2-10. PX-011 and [RULES/pack-classes] open point 8 now state OQ-F-3's decision (closed); OQ-F-3's own text is the owner's record of the question as asked |
| N-S8 | The one-live case under an automatic side (OQ-A-14), every form, one outcome in every text | MEETS: RS-007's scope, PR-007, AP-002, AP-003 (by "As AP-002 with t"), [F12 §6.3] and [F13 §5] V01 each take the node whole from the automatic side whichever side holds a conflict value, and on MR-002 with two plain sides; each uses "counts as a node an existence policy fixed" (`docs/spec/rules/merge-table.md:147`, `docs/spec/rules/merge-table.md:325`, `docs/spec/rules/merge-table.md:559`, F12:633-634, F13:343); [RULES/merge-table] open point 26 (`docs/spec/rules/merge-table.md:910-912`) states neither: it says only that an automatic `ours` or `theirs` makes no conflict value and that a side's conflict value it takes is carried and reported as PR-011 states, which contradicts none of them; RS-007 keeps "a node live in the result with no existence policy" with the exception after it; [F07 §6.4]'s tombstone line agrees |
| N-S9 | [AR §11]'s OI-1g under automatic sides: the both-live and both-deleted forms merge key by key | MEETS: PR-007 and AP-002 state it; [F12 §6.3] states the one-live exception and §7.2's key-by-key rule otherwise; [F13] V01 counts such a node as merged. The tests the automatic-sides fix added are each `ok` in the full gate at the base (§6) |
| N-S10 | Every [F13] invariant has an enforcement point, a check class and a gate, and a `COVERAGE.md` row | MISSES: S2-6, S2-7 |
| N-S11 | Every [F16] protocol rule and decision has a seeded-bug row ([F16 §17.3], §17.4) | MEETS: 102 rules, 102 rows; L-1 to L-10 carry the other chapters' rules, item 1a's locator rule included (L-10) |
| N-S12 | The values `--policy` takes, in every text that states them | MISSES: S2-1 |

## 3. Findings at a glance

| Id | Sev. | Where | Finding | Owner |
|---|---|---|---|---|
| S2-1 | major | [RULES/merge-table] `auto-policy`, open point 26; PV-011; RG-010; [F12 §6.5]; [API §11.7]; [F06 §4.4.16] | `--policy` takes two value sets; the staged bytes hold one | R-MODEL WP-90; R-SPEC-F WP-12, WP-25 |
| S2-2 | major | `fixtures/hex/store-a/` c11; [F06 §7.3] | A staged commit's `prev`s name another ref's records | R-FIX WP-20 with R-ORA WP-95, or R-SPEC-F WP-12 |
| S2-3 | major | [F12 §7.7] | The directional rows omit the automatic policies | R-SPEC-F WP-12 |
| S2-4 | major | [F18 §2.7], [F13 §3.9] I-F7; TR-081, LV-005 | A settle writes `removed` through a door I-F7 does not list | R-SPEC-F WP-14, R-SPEC-P WP-16, or R-MODEL WP-92 |
| S2-5 | minor | [F12 §6.5]; RS-008; DM-012 | The restore of a side names a merge's second parent; a revert or cherry-pick that made the value has none | R-SPEC-F WP-12; R-MODEL WP-90 |
| S2-6 | minor | [F13 §3.2] I6, I7 | Two enforcement points without a check class | R-SPEC-P WP-16 |
| S2-7 | minor | `COVERAGE.md` | No row maps the design-of-record invariants | R-SPEC-F WP-10 |
| S2-8 | minor | [LQ/envelope] open point 8; [F09] OP-09-12 | Two texts give the BM25 arithmetic to each other | R-SPEC-F WP-19 |
| S2-9 | minor | [API §9.1] | E409's row omits DP-010's root-node case | R-SPEC-F WP-25 |
| S2-10 | minor | [RULES/delete-policy-matrix] DS-001, DS-002 | The step order runs DP-005 and DP-010 before their set exists | R-MODEL WP-90 |
| S2-11 | minor | [F21 §4.6], §4.9, open point 9 | A heading on a list item's next line is not among the stated differences | R-SPEC-R WP-14b |
| S2-12 | minor | [F21 §6.5], open point 8 | [F21] calls texts missing that [F19 §10.2] has | R-SPEC-R WP-14b |
| S2-13 | minor | [API §18] | The needs-triage note of an agent with several task leases | R-SPEC-F WP-25; R-MODEL WP-90 |
| S2-14 | minor | [RULES/state-definition] open point 4 | Asks [F13] MC-1 for a sentence it has | R-MODEL WP-90 |
| S2-15 | minor | [RULES/merge-table] VA-005 | Its `emits` cell names only `DepthExceeded` | R-MODEL WP-90 |

## 4. Findings

### Majors

**S2-1 — `--policy` takes two value sets, and the staged bytes hold only one (major, DIVERGENCE).**
**Where:** [RULES/merge-table] `auto-policy` (`docs/spec/rules/merge-table.md:317-318`) and open point 26
(`docs/spec/rules/merge-table.md:910-912`); PV-011 (`docs/spec/rules/policy-keys.md:327`); RG-010
(`docs/spec/rules/README.md:184`); [F12 §6.5] (F12:692-693); [API §11.7] (`docs/spec/store-api.md:1485`);
[F06 §4.4.16] (F06:463); [AR §5a.7] step 4 (`docs/ARCHITECTURE-RESEARCH.md:925`).
**Problem.** Four texts read as the wide set: `auto-policy` gives "the allowed values of … `merge.policy.<kind>` … and
of `--policy` on `merge` and `resolve --all`" and adds that "the model implements every value"; PV-011 lists `none`,
`ours`, `theirs`, `delete-wins` and `resurrect` with "`merge --policy` overrides it"; RG-010 calls the table the "values
of `merge.policy.<kind>` and `--policy`"; [F12 §6.5] resolves with "the side P names ([RULES/merge-table]
`auto-policy`)". Two read as the narrow set: open point 26 ("`--policy` takes the last two") and [API §11.7], whose
`policy` argument is "`delete-wins` or `resurrect`", as [AR §5a.7] writes it. So an engine built from [API §11.7]
refuses `merge --policy ours` as usage, while the model accepts it (GT2 diverges). A staged merge cannot record
`--policy ours` at all: the `stage` group's override is two bits, 0 none, 1 `delete-wins`, 2 `resurrect` (F06:463), so
`merge --continue` could not recompute such a staged merge ([F12 §9.4] step 1). Under the narrow set, `resolve --all
--policy delete-wins` names no side for a conflict value other than `DeleteVsModify` (a `FieldEdit`, a `TextHunk`), and
[F12 §6.5] does not say what happens to it. [API §11.7]'s default cell names the kinds' existence policies only, not
`merge.policy.<kind>`, which the `auto-policy` rows let `--policy` override. `docs/spec/config.md:997` names the two
overrides and no value set.
**Evidence:** a search of `docs/spec/` and [AR] for `--policy`: the lines above are every statement of its values; no
other text states a set.
**Fix** (two remedies; the owner's disposition chooses):
(a) `--policy` takes `delete-wins` and `resurrect` only: R-MODEL, WP-90 narrows the `auto-policy` preamble, PV-011's
note and RG-010's description; R-SPEC-F, WP-12 narrows F12:692-693 and states what `resolve --all --policy` does to a
conflict value that is not `DeleteVsModify` (refused, or left as it is); [API §11.7] already states the set and
[F06 §4.4.16] already holds it; R-SPEC-F, WP-25 names `merge.policy.<kind>` in the default cell.
(b) `--policy` takes every `auto-policy` value: R-MODEL, WP-90 rewrites open point 26; R-SPEC-F, WP-25 widens
[API §11.7]; R-SPEC-F, WP-12 widens the `stage` group's override field (F06:463), a byte choice the tree does not imply;
WP-81a edits [AR §5a.7]'s row. **Owner call** for (b): a byte choice, and a departure from the design of record.
**Disposition:** open.

**S2-2 — Store A's staged commit c11 carries `prev`s that name another ref's records (major, DIVERGENCE).**
**Where:** [F06 §7.3] (F06:726-727) and SL-2 (F06:777-780); `fixtures/hex/store-a/log.1.hex:2830-2832` (c11's ref id 2
and `ref_seq` 1), `fixtures/hex/store-a/log.1.hex:2865` and `fixtures/hex/store-a/log.1.hex:2899` (its two `prev`s);
`fixtures/hex/INDEX.md:97-98`; [F12 §9.2] item 3 (F12:1039-1041).
**Problem.** c11 is the first commit on the staging ref `merge/main/from/lane/l5np` (ref id 2, `ref_seq` 1, as
[F12 §9.2] item 3 requires). Its `Conflict` on #1 carries `prev` 3810, which names c3 on `main` (lsn 9555 − 5745), and
its existence `Conflict` on #11 carries `prev` 1478, which names c7 on `main` (lsn 9555 − 8077). [F06 §7.3] makes `prev`
0 "when the owner has no earlier op on this ref" and otherwise the distance to the newest earlier record "with the same
`ref_id`"; SL-2 states the same per-ref rule for lanes ("never another ref's record"). The staging ref has no earlier
record, so by the rule both are 0. A writer built from [F06 §7.3] writes 0; the fixture, and the format oracle that
decodes it, hold `main`'s distances. Once item 1a's fields enter the fixture, c11's two frames carry locators that a
writer following the rule never writes.
**Fix** (two remedies; the owner's disposition chooses):
(fixture) R-FIX, WP-20 sets c11's two `prev`s to 0, so their locators vanish, and store A ripples again from g18, with
the format oracle's matching change by R-ORA, WP-95, in their own landing;
(rule) R-SPEC-F, WP-12 adds a staging-ref exception to [F06 §7.3] and SL-2: a staged commit's ops chain into dst's
chain. **Owner call** for (rule): it makes a format rule.
**Disposition:** open.

**S2-3 — The directional rows of [F12 §7.7] omit the automatic policies (major, WRONG ANSWER).**
**Where:** [F12 §7.7] (F12:933-938) and open point 12 (F12:1210-1212); AP-002 and AP-003
(`docs/spec/rules/merge-table.md:325-326`).
**Problem.** [F12 §7.7] states that merge(A into B) and merge(B into A) give equal states, sides exchanged, "except at
the keys decided by these directional rows", and calls this the commutativity of [40 §8.3.2] P8 and GT6 "stated
exactly". Under `merge.policy.<kind>` = `ours` or `theirs` (AP-002, AP-003) a value conflict of that kind resolves to
dst's or src's value, and on MR-002 or MR-005 with one live and one deleted flat side the whole node comes from dst or
src (OQ-A-14): exchanging dst and src changes the result at each such key and node, and neither row is listed. The
`auto-policy` values are swept pairwise with `merge.strict` (`docs/spec/rules/merge-table.md:318`), so a GT6 property
written from §7.7 reports a false failure under them, or must exclude them without a text that says so.
**Fix:** R-SPEC-F, WP-12: [F12 §7.7] and its open point 12 list AP-002 and AP-003 among the directional rows: the key an
automatic side resolves, and the node it takes whole.
**Disposition:** open.

**S2-4 — A settle writes `removed` through a door that I-F7 does not list (major, DIVERGENCE).**
**Where:** [F18 §2.7] (`docs/spec/format/18-file-links.md:193-196`) against [F18 §2.1]
(`docs/spec/format/18-file-links.md:99-101`); [F13 §3.9] I-F7 (F13:240); TR-081
(`docs/spec/rules/status-machines.md:226`); LV-005 (`docs/spec/rules/link-merge-rules.md:152`);
[RULES/status-machines] open point 26 (`docs/spec/rules/status-machines.md:524-528`).
**Problem.** I-F7, as [F18 §2.7] states it from [40 §2.10], lists the only doors through which a file node becomes
`removed` (`file rm`, `links fix --drop`, `--same-as`, `--split`, and deletion inference on `main`), and [F13 §3.9]
enforces it at EP-W4 ("only these doors emit `→ removed`"). TR-081 lets the door `settle` move an artifact from
`present` to `removed{reason: same-as}` when it unifies a `PathClaim` by an exact rename in a fresh writer tree (LV-005,
[40 §2.3] "settle performs the unification itself"), a door I-F7 does not list. An engine that enforces I-F7 as [F18]
and [F13] state never writes the unification, while the model writes it by TR-081: after such a settle the two hold
different statuses for the claimants. [F18] disagrees with itself: its I-F1 text resolves a `PathClaim` by
"`links fix --same-as`, or a settle that unifies the two by an E6 rename in a writer tree fresh for both", which
"leaves at most one". Open point 26 records the conflict and asks [F18 §2.7] to list the door; neither [F18 §2.7] nor
[F13] does.
**Fix** (two remedies; the owner's disposition chooses):
(a) I-F7 lists the settle's `PathClaim` unification as a door: R-SPEC-F, WP-14 ([F18 §2.7]), R-SPEC-P, WP-16 ([F13 §3.9]
I-F7's row and its EP-W4 clause), and [40 §2.10] at WP-81a;
(b) a settle never writes `removed`: R-MODEL, WP-92 withdraws TR-081 and LV-005's unification, and the `PathClaim` value
stands until `links fix --same-as`; [40 §2.3]'s sentence is edited at WP-81a.
**Owner call:** each remedy changes what an invariant of the design of record admits, or a rule it states.
**Disposition:** open.

### Minors

**S2-5 — The restore of a taken side names a merge's second parent; a revert or cherry-pick that made the conflict
value has none (minor, UNDERSPECIFIED).**
**Where:** [F12 §6.5] (F12:656-660); RS-008 (`docs/spec/rules/merge-table.md:148`); [RULES/merge-table] open point 5 (c)
(`docs/spec/rules/merge-table.md:789-792`); DM-012 (`docs/spec/rules/merge-table.md:656`); CS-014
(`docs/spec/rules/merge-table.md:131`).
**Problem.** A `live` existence side that a resolution takes has its hierarchy key and out-edges restored "from that
side's state at M, the merge that made the conflict value: `state(first parent of M)` for `ours`, `state(second parent
of M)` for `theirs`"; RS-008 repeats it, and open point 5 (c) speaks of "the merge that made the conflict value". A
revert or a cherry-pick commit has one parent, tip R, and its origin (F12:988-989), and it lands conflict values
(F12:1111), so it can be M. Its `ours` restores from tip R; its `theirs` has no second parent. On plain sides the case
does not arise: a `DeleteVsModify` needs a live base (CS-014), and a revert or cherry-pick whose src changed a node that
is live in the base and deleted on dst stages `NotFound` instead (DM-012), so `theirs` is never the live side there. No
text says so, and DM-012 does not say whether a dst side whose conflict value has a deleted provisional value (MR-005)
counts as "deleted on dst". [F12 §6.5]'s bullet on a violation key names the source state of a revert or a cherry-pick
("the state [RULES/merge-table]'s DM row reads as src", F12:677-678); the bullet on a conflict value does not.
**Fix:** R-SPEC-F, WP-12: [F12 §6.5] calls M the commit that made the conflict value and, for an M of kind `revert` or
`cherry-pick`, takes `theirs`' state from the src state of M's DM row (DM-003 to DM-005), as F12:677-678 does;
R-MODEL, WP-90: RS-008 and open point 5 (c) follow, and DM-012 states whether a provisional value counts as deleted.
**Disposition:** open.

**S2-6 — Two invariants name an enforcement point without a check class (minor, UNDERSPECIFIED).**
**Where:** [F13 §3.2] I6 and I7 (F13:140-141).
**Problem.** [F13 §1.2] tags every enforcement point with a class of §1.3. I6's `EP-WD V06` and I7's `EP-WD V07` carry
none; every other point of the 58 entries carries one.
**Fix:** R-SPEC-P, WP-16 tags both.
**Disposition:** open.

**S2-7 — `COVERAGE.md` has no row that maps the design-of-record invariants (minor, UNDERSPECIFIED).**
**Where:** `docs/spec/COVERAGE.md:5` (its charter), `docs/spec/COVERAGE.md:56` (152 rows); [F13] Coverage (F13:463-465);
`docs/m0/PLAN.md:351` ("100 % invariant coverage").
**Problem.** `COVERAGE.md` maps I-F1–I-F14 (row R-12), I-G1–I-G6 (rows 60-I2-PD(k) and X-F3) and [50] F15, but no row
maps I1–I14, I14′–I43′, I35′ and I-P3 ([F13 §3.1]–§3.7), I-D1 (§3.10) or the I37′ validator order (§5): its charter has
no invariant row, and [F13]'s own Coverage maps them to [AR §8.3] "Invariant coverage", [60 §2.6] and [AR §11] #33,
which are not rows of `COVERAGE.md`.
**Fix** (two remedies; the owner's disposition chooses): R-SPEC-F, WP-10 adds rows for [AR §8.3] "Invariant coverage"
([F13 §3]), [60 §2.6] ([F13 §5]) and [AR §11] #33 ([F13 §3.10]); or `COVERAGE.md` §1 states that [F13]'s Coverage
table is the index of record for the invariants outside its charter.
**Disposition:** open.

**S2-8 — Two texts give the BM25 arithmetic to each other (minor, DIVERGENCE).**
**Where:** [LQ/envelope] open point 8 (`docs/spec/lq/envelope.md:589-590`); [F09 §12.4] (F09:906-907) and OP-09-12
(F09:1390-1392).
**Problem.** Review a1-S's S-22 is deferred by [F09] with its owner and milestone: if GT13 keeps BM25, "[LQ/std] fixes
the formula, summation order and rounding before M2", and the hole `F09-doclen` says the same. [LQ/envelope]'s open
point 8 says the arithmetic "belongs with F12's statistics ([F09])". [LQ/std] states nothing yet, so a reader who
starts at the envelope is sent to [F09], which sends them back.
**Fix:** R-SPEC-F, WP-19: [LQ/envelope] open point 8 names [LQ/std] (WP-19) as the owner, as [F09] OP-09-12 does.
**Disposition:** open.

**S2-9 — E409's row in [API §9.1] omits DP-010's root-node case (minor, UNDERSPECIFIED).**
**Where:** `docs/spec/store-api.md:996`; DP-010 (`docs/spec/rules/delete-policy-matrix.md:123`);
`docs/spec/reviews/pass1-closure.md:227`.
**Problem.** Pass 1's closure left this residue: the row names DP-005 to DP-007, while [LQ/errors §5.5] and
[F19 §10.5] carry the root-node case. It is unchanged at the base.
**Fix:** R-SPEC-F, WP-25 adds DP-010 to E409's row.
**Disposition:** open.

**S2-10 — The delete steps run DP-005 and DP-010 before the set they test exists (minor, UNDERSPECIFIED).**
**Where:** DS-001 and DS-002 (`docs/spec/rules/delete-policy-matrix.md:205-206`);
`docs/spec/reviews/pass1-closure.md:229`.
**Problem.** Pass 1's closure left this residue: DS-001 (step 1) applies the DP rows, among them DP-005 and DP-010,
which test the deleted set that DS-002 (step 2) defines. It is unchanged at the base.
**Fix:** R-MODEL, WP-90: the step table says that step 2's set is computed inside step 1 after DP-004.
**Disposition:** open.

**S2-11 — [F21]'s stated differences from CommonMark omit a heading on a list item's next line (minor, DIVERGENCE).**
**Where:** [F21 §4.6] (F21:543-544, F21:555), §4.9 (F21:594-596), open point 9 (F21:1232-1235); §4.2's rules 5 and 7.
**Problem.** The container rule recognises only a list item's first line "so that a heading inside one is not taken for
a document heading", and §4.9 and open point 9 name one exception, a heading "after a blank line". By §4.2, a heading
on the line right after a list item's marker line, indented 0 to 3 columns (`- item⏎  # Sub`), is a document heading
too: rule 5 classifies it before rule 7 makes it a container line, while CommonMark puts it inside the list item. The
result is deterministic, but no text states it and no construct of §8.2 pins it.
**Fix** (two remedies; the owner's disposition chooses): R-SPEC-R, WP-14b (a) names the case in §4.9 and open point 9
and adds a construct to §8.2; or (b) makes a shallow line inside a container run, indented to the marker's content, a
container line, with a construct (which changes which headings are items).
**Disposition:** open.

**S2-12 — [F21] calls missing the refusal texts that [F19 §10.2] now has (minor, DIVERGENCE).**
**Where:** [F21 §6.5] (F21:737-740) and open point 8 (F21:1229-1231); [F19 §10.2] `anchor_spec` (F19:681-685).
**Problem.** [F21 §6.5]'s rows "several", "not recordable", "failed scan" and "syntax" point at "[F19] text: open point
8", and open point 8 asks R-SPEC-F for those texts; [F19 §10.2] has them, with the cases `several`, `not-recordable`,
`scan-failed` and `syntax`.
**Fix:** R-SPEC-R, WP-14b cites [F19 §10.2] in §6.5 and closes open point 8.
**Disposition:** open.

**S2-13 — The needs-triage note of an agent that held several task leases is not stated (minor, UNDERSPECIFIED).**
**Where:** [API §18] (`docs/spec/store-api.md:2128-2133`); [RULES/policy-keys] open point 6
(`docs/spec/rules/policy-keys.md:391-393`); WH-004.
**Problem.** The note has the title `needs triage: <agent> stopped holding <lease>` and "an `ABOUT` edge to the lease's
task"; WH-004 releases every lease the stopping agent holds. Whether such an agent gets one note per task lease or one
note for all, with which title and edges, is not stated; the open point records the question.
**Fix** (two remedies; the owner's disposition chooses): R-SPEC-F, WP-25 states one note per task lease, each with its
task's `ABOUT` edge, or one note that names every lease, with one `ABOUT` edge per task; R-MODEL, WP-90 follows.
**Disposition:** open.

**S2-14 — [RULES/state-definition] open point 4 asks [F13] MC-1 for a sentence it already has (minor, DIVERGENCE).**
**Where:** `docs/spec/rules/state-definition.md:469-471`; [F13 §4.2] MC-1 (F13:324).
**Problem.** The open point says MC-1 reads that commits on other ref kinds "change nothing" and should say they still
flag markers nonlinear; MC-1 says so ("they can still flag markers nonlinear (ME-011, which applies to every ref
kind)").
**Fix:** R-MODEL, WP-90 marks the request done.
**Disposition:** open.

**S2-15 — VA-005's `emits` cell names only `DepthExceeded` (minor, DIVERGENCE).**
**Where:** VA-005 (`docs/spec/rules/merge-table.md:583`); CM-011 (`docs/spec/rules/merge-table.md:531`); RG-014
(`docs/spec/rules/README.md:188`); [F13 §5] V05 (F13:347).
**Problem.** VA-005's note and CM-011 make it the emitter of `HierarchyCycle` for every cycle V05 finds, and V05's
outcome is both classes; the row's `emits` cell, one token by RG-014, names `DepthExceeded` alone.
**Fix** (two remedies; the owner's disposition chooses): R-MODEL, WP-90 gives the `validators` table's `emits` column a
token list and names both classes, or splits VA-005 into two rows at order 5.
**Disposition:** open.

## 5. Known open findings met (cited, not re-raised)

- **R-SPEC-F:** F07:1229's "the conflict's introducing commit" against [F12 §6.5], RS-008 and open point 5 (c) (beside
  S2-5, which concerns the second parent, not the word); "They" at `docs/spec/store-api.md:1864`; the `COVERAGE.md`
  rows 60-AR-HEAD, 60-AR-Log-extents, 60-AR-Log-RecHdr and 60-AR-Log-chain name no store file of their own;
  [F19 §10.2]'s `body_dropped` row, `docs/spec/store-api.md:860` against `docs/spec/store-api.md:296` (the E406 and
  E407 order), and whether [API §2.3]'s `dry` covers the admin verbs; [F08] open point 30's "content"; the sentence for
  [F06 §7.7] or [API §9.6] on the bound; [API §9.6]'s bound gap (`docs/spec/reviews/wave-3e-arbiter-s.md:285`, RT-2).
- **R-SPEC-R:** [F14] names only merge re-keys where [API] says merges or syncs; F10:148's "admitted non-dictionary
  codecs" includes codec 0.
- **R-SPEC-P:** F17:809-810's "(§4.5)" pointer; F17:14 ("and what the value" breaks the line); F17:556-557's test
  value 4.
- **R-MODEL:** the findings of `docs/spec/reviews/wave-3e-arbiter-s.md`'s table that remain open (W3E-ARB-S-2 to -8,
  queued for the next R-MODEL unit); an automatic side over a node with an absent flat side, for which PR-007's and
  AP-002's notes (`docs/spec/rules/merge-table.md:559`, `docs/spec/rules/merge-table.md:325`) state no outcome (its
  reachability rated plausible only); "a cycle with no repairable key", a term no text defines
  (`docs/spec/reviews/wave-3e-arbiter-s.md:400`, `docs/spec/reviews/wave-3e-arbiter-s.md:259`). N-S9 does not rate
  the absent flat side.
- **R-REV-S, on `docs/spec/reviews/wave-3e-arbiter-s.md`:** this lens's role wrote that report, so these items are
  cited and never rated (S5); they are queued for the role's next text-fix batch after pass 2: the tombstone wording at
  `docs/spec/reviews/wave-3e-arbiter-s.md:355` with `docs/spec/reviews/wave-3e-arbiter-s.md:340-343` (F07:392-395,
  F07:397; corrected in OQ-A-14, `docs/spec/reviews/owner-questions.md:864-865`); "asserts only" at
  `docs/spec/reviews/wave-3e-arbiter-s.md:371-372`; TX-2's unjudged clauses at
  `docs/spec/reviews/wave-3e-arbiter-s.md:228` (`docs/spec/rules/merge-table.md:559`,
  `docs/spec/rules/merge-table.md:151`, `docs/spec/rules/merge-table.md:560`); the quoted wording of OI-1g at
  `docs/spec/reviews/wave-3e-arbiter-s.md:332-333` (the text that carries it is F12:622-625); TX-39's verdict at
  `docs/spec/reviews/wave-3e-arbiter-s.md:265` (F19:1349-1350); W3E-ARB-S-6's evidence at
  `docs/spec/reviews/wave-3e-arbiter-s.md:422` and `docs/spec/reviews/wave-3e-arbiter-s.md:426-427`; two lines over
  120 columns at `docs/spec/reviews/wave-3e-arbiter-s.md:372` and `docs/spec/reviews/wave-3e-arbiter-s.md:406`.
- **WP-81a's [AR] list:** `docs/ARCHITECTURE-RESEARCH.md:538`, `docs/ARCHITECTURE-RESEARCH.md:344`,
  `docs/ARCHITECTURE-RESEARCH.md:1578`, F06:1430-1431, `docs/research/design/30-synthesis.md:457`; [F13] departs from
  [AR §3.5] until then.
- **Queued and recorded:** the owner's ruling R-90 is in the text ([F13 §6.2], [F17 §8.2], NN-007, NG-005, NP-010), and
  its read-time `pin_drift` check is specified later; [API §8.7]'s attestation member, "held" after a `Gc`, and
  [F08 §10.4]'s dropped body are queued for R-SPEC-F.
- **Status rows:** most `.md` files of `docs/spec/` still read "draft, pass 1 pending", `rules/policy-keys.md` among
  them though it is new since pass 1; another lens rates the status rows.
- **The known state:** the old byte names at their six sites and the accepted windows (F08:1068 against
  `docs/spec/lq/std.md:568`; byte 3 and `PRIO` against the fixtures with a `NODE` section; the fixtures and the oracle
  without item 1a's fields) are expected, not findings.

## 6. Checked and found correct

Recomputed or cross-checked, needing no change; a later pass need not repeat them unless the text changes.

- [F14 §17.2], recomputed with `cargo xtask hex` over [F08 §11.1]–§11.4's inputs: the file uid
  `19143eabafc5ec1c2f8ddb102a731ae3`, the root uid `721354ad0f367138746bf0429056d772`, the `file` anchor's `captured`
  `c81b5793ae8794402388225c48c0048f` and uid `de8edb59215a406d849289841739d59b`, the `symbol` anchor's `captured`
  `8856fc02927f89608d0a57eb57ed2337` and uid `6a49c1797315fca432f54040700ab381`, `quote_h`, `prefix_h` and `suffix_h`
  (both texts 32 bytes), and the window's base64url `AwACAB86Apx9DrRRyNI`.
- [F07 §16]: the 43-byte status entry, the 74-byte digest input, the 196-byte commit input with the 16-byte domain
  string, and BLAKE3-128 of the empty string (`af1349b9f5f9a1a6a0404dea36dcc949`, recomputed); [F08 §11.2]'s 46-byte
  input.
- [F06 §11]: the 208-byte payload, the `SetStatus` frame (`prev` 4,800 as `C0 25`, the locator (227, 11), the named run
  at payload offset 195 after the 32-byte header) and the 207-byte form with the unknown locator.
- [F21 §2.3]'s two scope values (25 and 19 bytes); the 51 keywords of §3.1; every `canon` vector of §3.2; the item lists
  of constructs 1–27, M1–M14 and T1–T12 against §3–§5's rules; the outcomes of constructs 14 and M14 against §6; the
  headers of construct 11 and T1 and T4 against [F20 §2.8].
- [F12 §2.3]'s longest ref name, 284 bytes; [F12 §7.4]'s quotation of RS-007's **Values** and **Origin** sentences,
  equal to RS-007's.
- Item 1a: [F06 §7.1.1], §7.3.1 (LV-1, LC-1, SL-1 to SL-4, LR-1 to LR-3), [F16] P-35 to P-37, L-10 and [F17]'s
  locator test row agree; P-37's fallback to unknown locators keeps [F17 §4.4]'s inline-size and group-size checks
  decided as before.
- The `BodyDrop` bound of 2,048 hashes ([API §8.7]) fits one group at a 64 KiB extent ([F05 §9.29]); the record kinds
  29 and 30 agree across [F05], [F11], [F16] and [API]; every error code [API] and [F21] use is in [F19 §10];
  every LQ code is in [F19 §7.3]'s exit map or is unassigned or retired.
- The automatic-sides tests, each `ok` in the full gate at the base:
  `merge::tests::an_automatic_ours_keeps_its_deleted_conflicted_node_whole_against_a_live_side`,
  `merge::tests::an_automatic_ours_keeps_its_live_conflicted_node_whole_against_a_deleted_side`,
  `merge::tests::an_automatic_ours_keeps_its_node_whole_when_its_values_prov_names_theirs`,
  `merge::tests::an_automatic_ours_keeps_its_plain_live_node_whole_against_a_deleted_conflicted_side`,
  `merge::tests::an_automatic_ours_plain_live_value_over_a_live_conflicted_side_merges_key_by_key`,
  `merge::tests::an_automatic_ours_plain_tombstone_over_a_deleted_conflicted_side_merges_the_tombstone`,
  `merge::tests::an_automatic_side_keeps_the_modified_node_whole_over_a_delete_versus_modify`,
  `merge::tests::an_automatic_theirs_keeps_its_tombstone_whole_against_a_live_conflicted_side`,
  `suite::vcs::an_automatic_ours_over_two_deleted_flat_sides_merges_the_tombstone_key_by_key`,
  `suite::vcs::an_automatic_ours_over_two_live_existence_values_lands_a_change_only_src_made`,
  `suite::vcs::an_automatic_theirs_over_two_live_existence_values_keeps_a_move_only_dst_made`.
- The named checks marked MEETS in §2.

## 7. Open points for the disposition

1. **Owner calls:** S2-1 remedy (b), S2-2 remedy (rule), S2-4 (either remedy).
2. **Two remedies, neither chosen here:** S2-1, S2-2, S2-4, S2-7, S2-11, S2-13, S2-15.
3. **Severity.** S2-2 and S2-4 are rated major: in each, a reader following the text writes one thing and the fixture or
   the model another, but the rule's intent is not broken (`prev` is unhashed and per ref; I-F7's subject is absence).
   The disposition may rate either a blocker under WP-80's first clause.
4. **[F21] and its interim rule.** This lens has no blocker or major against [F21]; whether the interim scanner rule
   lifts is pass 2's closure's, after every lens has reported.
