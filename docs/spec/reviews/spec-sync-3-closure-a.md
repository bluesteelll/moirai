# Spec sync 3: second closure check, lens A

| Field | Value |
|---|---|
| Title | Second closure check of spec sync 3 (`docs/spec/reviews/spec-sync-3.md`): the ten items its closure check applied without a second check, and pass 1's round-4 re-read of NC-11 and NC-12 (`docs/spec/reviews/pass1-closure.md` open point 1) |
| Status | review, pass 2; every finding awaits its disposition by the owning role named in its Fix |
| Work package | WP-80 ([PLAN §3.2] item 8), lens R-REV-A. The author of this file wrote none of the reviewed text (S5) and did not read the pass-2 findings of the other lenses |
| Lens | A, as an independent read-only check of applied fixes |
| Reviewed | the sixteen files the items and the re-read cite (§1) |
| Base | 022277db150d047e817652691e2fc19be1a37285 |
| Checked against | `docs/spec/reviews/spec-sync-3.md` (the R-MODEL, R-SPEC-F, R-SPEC-P, R-SPEC-R and R-FIX closure paragraphs and rows); `docs/spec/reviews/pass1-closure.md` §4, §5 and "Round 3 follow-up"; `docs/spec/reviews/owner-questions.md` (OQ-F-3) |

## 0. Verdict

**CLOSED, with three rows changed since: 0 findings.** Spec sync 3 has one commit, c6c2214, which holds the sync, its
independent check and the fixes its closure check applied. For each item the fix is in the text as the sync states it.
`git blame` at the base gives every cited line to c6c2214 or an older commit except one, PR-016
(`docs/spec/rules/merge-table.md:568`), which cf11a90 rewrote in place after the sync; the other later commits on each
file change other lines (§2, §6). Eight items are CLOSED. CC-2's PR-016 row was rewritten after the sync and keeps the
dropped-body clause, CC-10's fixture-index row was rewritten after the sync, when a later fixture covered bit 19, and
NC-11's texts were rewritten when the owner answered OQ-F-3; all three were judged again and are correct.
NC-12 is CLOSED. One defect met beside NC-11, in a text the re-read compares, is a finding of the lens's pass-2 report
(`docs/spec/reviews/A-pass2.md`, A2-20), not of this check.

## 1. Method

| File | Depth |
|---|---|
| `config.md` | delta |
| `rules/policy-keys.md` | full |
| `format/12-vcs.md` | delta |
| `rules/merge-table.md` | delta |
| `format/16-protocol.md` | delta |
| `format/05-log.md` | delta |
| `format/14-image.md` | delta |
| `format/09-segments.md` | delta |
| `format/10-sealed-files.md` | delta |
| `format/11-runtime-tables.md` | delta |
| `fixtures/hex/INDEX.md` | lens |
| `format/13-invariants.md` | delta |
| `format/18-file-links.md` | delta |
| `store-api.md` | full |
| `rules/pack-classes.md` | delta |
| `rules/state-definition.md` | delta |

For each item: the sync's text (`docs/spec/reviews/spec-sync-3.md`) against the cited sections at the base; `git log` of
the item's file since c6c2214; `git blame` of the cited lines at the base. Item 1's registry half is judged only by the
outcomes of the four tests the sync names, in the full gate's log at a base whose `docs/spec` tree outside
`docs/spec/reviews/` equals this base's: `policykeys::tests::the_table_equals_the_registry ... ok`,
`policykeys::tests::the_registry_equals_config_md ... ok`, `policykeys::tests::every_allowed_value_of_every_key ... ok`,
`suite::tags::every_tag_names_a_row_and_every_procedure_row_is_tagged ... ok`. Item 10 is read as text.

## 2. Closure table

| Id | Item | Verdict |
|---|---|---|
| CC-1 | [CFG §10.5] and [CFG §10.7] keys without a policy-keys row (`docs/spec/reviews/spec-sync-3.md:31-40`) | CLOSED: KY-168 after KY-167 (`docs/spec/rules/policy-keys.md:231`), KY-169 after KY-120 (`docs/spec/rules/policy-keys.md:264`) and open point 6 (`docs/spec/rules/policy-keys.md:381-393`) state what the sync says, against [CFG]'s rows (`docs/spec/config.md:852`, `docs/spec/config.md:898`); the only later change of the file adds KY-170 (b4bd12e); the four named tests pass |
| CC-2 | [F12 §7.5]'s "A dropped body" bullet, open point 31 and §6.2's `TextHunk` row (`docs/spec/reviews/spec-sync-3.md:141-146`) | CHANGED SINCE: the bullet cites CS-011, MR-032 and MR-038 (F12:892-895), open point 31 says how the table carries the case (F12:1322-1330), the `TextHunk` row names the dropped-body rule (F12:579); the merge table agrees (`docs/spec/rules/merge-table.md:128`, `docs/spec/rules/merge-table.md:195`, `docs/spec/rules/merge-table.md:201`, `docs/spec/rules/merge-table.md:734`). PR-016 (`docs/spec/rules/merge-table.md:568`) was rewritten in place by cf11a90 after the sync, when RS-007's origin of a landed resolution came to be stated by state; it still names the store's set of dropped bodies as an input that only the text rule reads (CS-011). Judged again: correct |
| CC-3 | P-101 step 7 and P-84 step 3: a bulk `Commit` after the publish naming a deleted `blobs` file (`docs/spec/reviews/spec-sync-3.md:224-231`) | CLOSED: P-84 step 3 also checks every `blobs` file its `cs.<n>`'s `BLOBTAB` names (F16:878-883), and step 7 hands a later bulk commit to it (F16:811-813) |
| CC-4 | P-78's second form claiming the purge's unnamed outputs; P-79, P-98, step 7 (`docs/spec/reviews/spec-sync-3.md:231-237`) | CLOSED: P-78 (F16:709-716), P-79 (F16:721-722), P-98 (F16:678-679) and step 7 (F16:805-807) state it |
| CC-5 | bytes held only by an unlanded reservation's `blobs.<n>` are not "held" (`docs/spec/reviews/spec-sync-3.md:237-246`) | CLOSED: [F05 §9.29] "Origin" and "The purge" (F05:1177-1188), P-84's last paragraph (F16:887-892), I-D1 (c) (F13:261) |
| CC-6 | the paragraph after step 8: yield checkpoints release no file the purge reads (`docs/spec/reviews/spec-sync-3.md:246-247`) | CLOSED: F16:838-842 |
| CC-7 | [F14 §11.2]'s `BodyDrop` on import (`docs/spec/reviews/spec-sync-3.md:285-293`) | CLOSED: F14:1339-1353 (origin `import`, an empty note, at most 2,048 hashes, held bytes kept, the carrying commit named) and open point 52 (c) and (d) (F14:2075-2093) |
| CC-8 | [F09 §16.4] and [F10 §8] mirror P-101 step 7 (`docs/spec/reviews/spec-sync-3.md:296-298`) | CLOSED: F09:1200-1205 and F10:532-543 |
| CC-9 | [F11] open point 44 (b), §1.3's `HARVEST` row and §13.5 cite [F05 §9.30] (`docs/spec/reviews/spec-sync-3.md:298-299`) | CLOSED: F11:76, F11:1281, F11:1564-1571 |
| CC-10 | `fixtures/hex/INDEX.md` §6, row "Commit kinds, import provenance, presence bits" (`docs/spec/reviews/spec-sync-3.md:344`) | CHANGED SINCE: the row (`fixtures/hex/INDEX.md:484`) was renumbered to order 46 by 747ae9b and rewritten by fe57b51 when store F's first log extent came to hold bit 19 and `actor_kind` 3; its "not covered" cell now names the other `actor_kind` values as [F06 §3.6] gives them (1–6 and 255 valid, 7–254 reserved and accepted, 0 invalid) and bit 18 with its order-46 group. Judged again: correct |
| NC-11 | [F05 §9.11] and its open point 14, [F11 §13.1] and its open point 42, against I-F5, [F18 §2.5], [API §14.1], [API] open point 48 and PX-011 (`docs/spec/reviews/pass1-closure.md:280-284`) | CHANGED SINCE: the owner answered OQ-F-3 (b) after the round-3 fix, and each text now states the decision instead of the interim: the layer that delivers a pack appends the `feed` 2 cursor, the `pack` verb appends nothing, no M0 command is such a layer (F05:1407-1418, F11:1203-1210, F11:1542-1548, `docs/spec/store-api.md:1708-1710`, `docs/spec/store-api.md:2426-2436`, `docs/spec/rules/pack-classes.md:315`, F18:157). Judged again: they agree |
| NC-12 | [F11 §14] and its open point 1 against [F05 §9.5] field 9, MF-009 and [API §15.7] (`docs/spec/reviews/pass1-closure.md:327-329`) | CLOSED: the worked row stores `actor` 0 (F11:1329-1350), open point 1 reads as the fix states (F11:1392-1397), and [F05 §9.5] field 9, MF-009 (`docs/spec/rules/state-definition.md:223`) and `markers`' `actor` agree; the example's bytes recompute (a 100-byte body) |

## 3. Findings at a glance

| Id | Sev. | Where | Finding | Owner |
|---|---|---|---|---|

None.

## 4. Findings

None. Beyond the items: [F13 §3.9]'s I-F5 statement lists fewer read verbs than [F18 §2.5]; that is the pass-2
report's A2-20.

## 5. Known open findings met

- [RULES/pack-classes] open point 8 still frames the question as PX-011's earlier wording before it states the decision
  (`docs/spec/rules/pack-classes.md:506-520`); pass 1's closure recorded it as an editorial residue for the rule-file
  re-signature, and the decided text follows it.

## 6. Checked and found correct

- History since c6c2214, per file: `docs/spec/rules/policy-keys.md` b4bd12e (KY-170 only); F12 fifteen later commits,
  none on the cited lines; F16 five (the locator, item 1a, names and owner-question citations); F05 one (the `Marker`
  entry order); F14 seven (names, re-key, §17.2 values); F09 five and F10 two (statistics, names, codecs); F11 two;
  `fixtures/hex/INDEX.md` two (747ae9b, fe57b51); `docs/spec/rules/merge-table.md` twelve, of which cf11a90 rewrote
  PR-016 (CC-2) and the others change other lines. `git blame` gives every cited line of CC-1 to CC-9 and NC-12 to
  c6c2214 or an earlier commit, except `docs/spec/rules/merge-table.md:568` (cf11a90, CC-2).
- The four named tests pass (§1).
- [F11 §14]'s `MARKERS` example: `RtHdr` 24 B, row 72 B, heap 4 B; `hlc`, `seq`, `emit_lsn`, `n` and `ref_seq` in
  little-endian as stated.

## 7. Open points for the disposition

1. Pass 1's open point 1 (`docs/spec/reviews/pass1-closure.md:280-284`) asked for a round-4 re-read before pass 1 closes;
   this file records it. Whether it closes that open point is the owner's record.
