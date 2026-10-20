# Wave 3e: the S lens arbiter's check of RS-007 `threeway` (OQ-A-13)

| Field | Value |
|---|---|
| Status | `open`: 13.6 (a) is met at the nightly and exit tiers and on four further seeds; every decided item meets; four text findings (one Important, three Minor) and four test or harness findings go to their owners (§3); the owner note is closed by e1178b5 |
| Scope | OQ-A-13's absolute acceptance (13.6 (a)) re-run at both tiers and on own seeds; the decision's items 13.1 to 13.5 with F1 to F5; RS-007's scope against the key-by-key merge of a conflicted-key-row node (the owner's ruling R-43 of 2026-10-13, OI-1g; W3D-SF-2); the rule and spec texts of wave 3e against the model's merge |
| Rules | `rules/merge-table.md` RS-007, RS-010, RS-015, MR-039, MR-040, CS-013, CM-011, AP-002, AP-003, PR-007, PR-008, PR-014, PR-016, PR-017, VA-001, VA-004, VA-005, VB-011, HT-004, open points 15 and 35; `rules/delete-policy-matrix.md` XB-004; `rules/link-merge-rules.md` RK-008 |
| Format | `format/12-vcs.md` §5.3 VM-7, §5.4, §5.5, §5.5a, §5.6, §5.7 VBC-10, §6.3, §6.5, §7.1, §7.2, §7.4, §7.9, §9.4, open point 33; `format/13-invariants.md` I4, I25′, V01, V05, MC-6, OP-13-17; `format/19-errors-and-output.md` §12.2 code 64, §12.3 code 132, open point 41; `format/07-canonical-form.md` §6.4; `format/05-log.md` §9.5; `store-api.md` §9.6 item 2 |
| Decision | `reviews/owner-questions.md` OQ-A-13 (13.6 (a); the decision of 2026-10-07; the recommendation's "a spec arbiter re-runs the widened harness and checks the statement") |
| Commit | Every run and every read used f71c6b193abff73286c4421f47012f2bc7d0e3f4 (master at the start); the run kit's hash is 62a979caa8b7b322d9692e591d321d0a170da7cd8bac9fb9c0a212d2a4117d00; the amendment read the same commit, except KF-8 and W3E-ARB-S-9 (at 72cc6f279067b3ca715ad17f0e3236188e016ded) |
| Date | 2026-10-20 |
| Amended | 2026-10-20, after the audit of the report's first version. W3E-ARB-S-1 is restated neutrally, with every text it concerns, the one-live case as decided by the owner's ruling R-109 of 2026-10-20, its observable and the owners of both sides, and is rated Important, CONFIRMED. TX-1, TX-2 (now also RS-015 and PR-008), TX-32, SC-1, RT-1 and DC-1 follow it, and TX-37 ([F12 §5.4]) and TX-38 ([F12 §6.3]) are added. W3E-ARB-S-2 is restated in the same form, with R-SPEC-P WP-16 among its owners. Every row whose text holds "repairable key" reads MISSES on W3E-ARB-S-4, TX-39 ([F19] open point 41) is added, and the finding's list adds [F13 §5] V01 and [F19] open point 41. W3E-ARB-S-6 and RT-4 rest on their observables, every MISSES row states its observable, and W3E-ARB-S-7 states its outcome only. Own seeds' per-generator rows are quoted, W3E-ARB-S-5 names its three tests, and AC-13 gives link_na. KF-8 and W3E-ARB-S-9 are closed by e1178b5. The runs of §1 are unchanged |
| Role | R-REV-S, the S lens arbiter (WP-80; rule S5: a separate session, which wrote none of the checked texts) |

## Ruling

**OQ-A-13 13.6 (a) is met.** At the nightly tier (300 histories per generator) and at the exit tier (3,000 per
generator), on all nine generators, every one of the acceptance's 11 asserted counts is 0: 0 avoidable `HierarchyCycle`
stagings, 0 wrong landings, 0 stuck stagings, and 0 for staged_other_wrong, reparent_lost, link_broken, one_sided_off_t,
pick_off_o, panics, not_forest and nondet. Every corpus case and link fixture of the acceptance passes, the CONTESTED
corpus included. On four seeds of its own (3 to 6; 5,400 histories) the evaluation report's eight computed fault counts
are 0; it does not compute link_broken, not_forest or nondet, so those three are INCONCLUSIVE on own seeds (W3E-ARB-S-8).
OQ-A-13's fallback K4 is not needed. None of the return conditions of the key-by-key merge of a conflicted-key-row node
against K2 arose. The rule and spec texts state what the model does, with one Important and three Minor exceptions
(W3E-ARB-S-1; W3E-ARB-S-2 to -4), and four test or harness gaps are recorded (W3E-ARB-S-5 to -8). No finding is
Critical.

## 1. The runs

Each run was one `cargo test` through the run kit, on the commit named above, its single expected test `ok`.

**Nightly** (the acceptance):
`env MOIRAI_TEST_TIER=nightly cargo test --locked --message-format=json-render-diagnostics -p moirai-model --lib --no-fail-fast -- suite::rs007eval::rs007_eval_absolute_acceptance --exact --nocapture`
→ `suite::rs007eval::rs007_eval_absolute_acceptance` ok. Its count lines equal, byte for byte, those of a fresh nightly
reference run at the same commit.

```text
RS007ACCEPT gen=narrow histories=300 merges=1437 landed=812 staged=59 genuine=59 requested=0 avoidable=0 wrong=0 stuck=0 staged_other_wrong=0 reparent_lost=0 hints=6 link_broken=0 link_na=0 one_sided_off_t=0 pick_off_o=0 panics=0 not_forest=0 nondet=0 ckr_one_sided=0
RS007ACCEPT gen=lanes histories=300 merges=1089 landed=528 staged=79 genuine=35 requested=0 avoidable=0 wrong=0 stuck=0 staged_other_wrong=0 reparent_lost=0 hints=14 link_broken=0 link_na=0 one_sided_off_t=0 pick_off_o=0 panics=0 not_forest=0 nondet=0 ckr_one_sided=0
RS007ACCEPT gen=wide histories=300 merges=2921 landed=1737 staged=233 genuine=99 requested=0 avoidable=0 wrong=0 stuck=0 staged_other_wrong=0 reparent_lost=0 hints=36 link_broken=0 link_na=0 one_sided_off_t=0 pick_off_o=0 panics=0 not_forest=0 nondet=0 ckr_one_sided=0
RS007ACCEPT gen=based histories=300 merges=1308 landed=625 staged=75 genuine=34 requested=0 avoidable=0 wrong=0 stuck=0 staged_other_wrong=0 reparent_lost=0 hints=7 link_broken=0 link_na=0 one_sided_off_t=0 pick_off_o=0 panics=0 not_forest=0 nondet=0 ckr_one_sided=0
RS007ACCEPT gen=multi histories=300 merges=2615 landed=1648 staged=171 genuine=102 requested=0 avoidable=0 wrong=0 stuck=0 staged_other_wrong=0 reparent_lost=0 hints=37 link_broken=0 link_na=0 one_sided_off_t=0 pick_off_o=0 panics=0 not_forest=0 nondet=0 ckr_one_sided=0
RS007ACCEPT gen=policy histories=300 merges=1638 landed=1021 staged=126 genuine=12 requested=0 avoidable=0 wrong=0 stuck=0 staged_other_wrong=0 reparent_lost=0 hints=3 link_broken=0 link_na=0 one_sided_off_t=0 pick_off_o=0 panics=0 not_forest=0 nondet=0 ckr_one_sided=14
RS007ACCEPT gen=strict histories=300 merges=2458 landed=1447 staged=246 genuine=70 requested=16 avoidable=0 wrong=0 stuck=0 staged_other_wrong=0 reparent_lost=0 hints=24 link_broken=0 link_na=2 one_sided_off_t=0 pick_off_o=0 panics=0 not_forest=0 nondet=0 ckr_one_sided=0
RS007ACCEPT gen=edges histories=300 merges=1054 landed=579 staged=81 genuine=13 requested=0 avoidable=0 wrong=0 stuck=0 staged_other_wrong=0 reparent_lost=0 hints=8 link_broken=0 link_na=0 one_sided_off_t=0 pick_off_o=0 panics=0 not_forest=0 nondet=0 ckr_one_sided=0
RS007ACCEPT gen=full histories=300 merges=1942 landed=1172 staged=252 genuine=20 requested=2 avoidable=0 wrong=0 stuck=0 staged_other_wrong=0 reparent_lost=0 hints=6 link_broken=0 link_na=0 one_sided_off_t=0 pick_off_o=0 panics=0 not_forest=0 nondet=0 ckr_one_sided=10
RS007ACCEPT tier=nightly seconds=118.3
```

**Exit** (the acceptance):
`env MOIRAI_TEST_TIER=exit cargo test --locked --message-format=json-render-diagnostics -p moirai-model --lib --no-fail-fast -- suite::rs007eval::rs007_eval_absolute_acceptance --exact --nocapture`
→ `suite::rs007eval::rs007_eval_absolute_acceptance` ok.

```text
RS007ACCEPT gen=narrow histories=3000 merges=13621 landed=7358 staged=745 genuine=745 requested=0 avoidable=0 wrong=0 stuck=0 staged_other_wrong=0 reparent_lost=0 hints=109 link_broken=0 link_na=0 one_sided_off_t=0 pick_off_o=0 panics=0 not_forest=0 nondet=0 ckr_one_sided=0
RS007ACCEPT gen=lanes histories=3000 merges=11451 landed=5820 staged=739 genuine=395 requested=0 avoidable=0 wrong=0 stuck=0 staged_other_wrong=0 reparent_lost=0 hints=87 link_broken=0 link_na=0 one_sided_off_t=0 pick_off_o=0 panics=0 not_forest=0 nondet=0 ckr_one_sided=0
RS007ACCEPT gen=wide histories=3000 merges=29503 landed=17722 staged=2183 genuine=780 requested=0 avoidable=0 wrong=0 stuck=0 staged_other_wrong=0 reparent_lost=0 hints=228 link_broken=0 link_na=0 one_sided_off_t=0 pick_off_o=0 panics=0 not_forest=0 nondet=0 ckr_one_sided=0
RS007ACCEPT gen=based histories=3000 merges=13859 landed=6769 staged=700 genuine=357 requested=0 avoidable=0 wrong=0 stuck=0 staged_other_wrong=0 reparent_lost=0 hints=91 link_broken=0 link_na=0 one_sided_off_t=0 pick_off_o=0 panics=0 not_forest=0 nondet=0 ckr_one_sided=0
RS007ACCEPT gen=multi histories=3000 merges=26316 landed=16591 staged=1854 genuine=764 requested=0 avoidable=0 wrong=0 stuck=0 staged_other_wrong=0 reparent_lost=0 hints=317 link_broken=0 link_na=0 one_sided_off_t=0 pick_off_o=0 panics=0 not_forest=0 nondet=0 ckr_one_sided=0
RS007ACCEPT gen=policy histories=3000 merges=16219 landed=10069 staged=1287 genuine=160 requested=0 avoidable=0 wrong=0 stuck=0 staged_other_wrong=0 reparent_lost=0 hints=51 link_broken=0 link_na=0 one_sided_off_t=0 pick_off_o=0 panics=0 not_forest=0 nondet=0 ckr_one_sided=82
RS007ACCEPT gen=strict histories=3000 merges=24424 landed=14427 staged=2425 genuine=732 requested=69 avoidable=0 wrong=0 stuck=0 staged_other_wrong=0 reparent_lost=0 hints=165 link_broken=0 link_na=7 one_sided_off_t=0 pick_off_o=0 panics=0 not_forest=0 nondet=0 ckr_one_sided=0
RS007ACCEPT gen=edges histories=3000 merges=11033 landed=6170 staged=995 genuine=172 requested=0 avoidable=0 wrong=0 stuck=0 staged_other_wrong=0 reparent_lost=0 hints=56 link_broken=0 link_na=0 one_sided_off_t=0 pick_off_o=0 panics=0 not_forest=0 nondet=0 ckr_one_sided=0
RS007ACCEPT gen=full histories=3000 merges=18805 landed=10945 staged=2564 genuine=261 requested=35 avoidable=0 wrong=0 stuck=0 staged_other_wrong=0 reparent_lost=0 hints=68 link_broken=0 link_na=7 one_sided_off_t=0 pick_off_o=0 panics=0 not_forest=0 nondet=0 ckr_one_sided=118
RS007ACCEPT tier=exit seconds=1037.0
```

**Own seeds** (the evaluation report, which asserts nothing; seeds 3 to 6 are streams the acceptance does not draw):
`env MOIRAI_RS007_EVAL_PARTS=search MOIRAI_RS007_EVAL_GENS=narrow,lanes,wide,based,multi,policy,strict,edges,full MOIRAI_RS007_EVAL_SEEDS=3,4,5,6 MOIRAI_RS007_EVAL_CASES=150 MOIRAI_RS007_EVAL_SHRINK=1 cargo test --locked --message-format=json-render-diagnostics -p moirai-model --lib --no-fail-fast -- suite::rs007eval::rs007_eval_report --exact --ignored --nocapture`
→ `suite::rs007eval::rs007_eval_report` ok. Its first line and the `all` rows, and below them the per-generator rows
this report relies on (the run prints those before the `all` rows):

```text
RS-007 evaluation (threeway): parts {"search"}; cases 150; seeds [3, 4, 5, 6]; generators ["narrow", "lanes", "wide", "based", "multi", "policy", "strict", "edges", "full"]; threads 2
RS007EVAL	all	histories	threeway	5400
RS007EVAL	all	panics	threeway	0
RS007EVAL	all	avoidable	threeway	0
RS007EVAL	all	wrong	threeway	0
RS007EVAL	all	staged_stuck	threeway	0
RS007EVAL	all	staged_other_wrong	threeway	0
RS007EVAL	all	reparent_lost	threeway	0
RS007EVAL	all	one_sided_not_t	threeway	0
RS007EVAL	all	pick_no_hier_not_o	threeway	0
RS007EVAL	all	requested	threeway	21
RS007EVAL	all	hints	threeway	224
```

Each generator's `histories` row and its eight counts' rows, in the run's order (the run prints its other rows between
them; they are not quoted). Every generator reads `histories` 600 and the eight counts 0:

```text
RS007EVAL	narrow	histories	threeway	600
RS007EVAL	narrow	panics	threeway	0
RS007EVAL	narrow	avoidable	threeway	0
RS007EVAL	narrow	staged_other_wrong	threeway	0
RS007EVAL	narrow	reparent_lost	threeway	0
RS007EVAL	narrow	staged_stuck	threeway	0
RS007EVAL	narrow	wrong	threeway	0
RS007EVAL	narrow	one_sided_not_t	threeway	0
RS007EVAL	narrow	pick_no_hier_not_o	threeway	0
RS007EVAL	lanes	histories	threeway	600
RS007EVAL	lanes	panics	threeway	0
RS007EVAL	lanes	avoidable	threeway	0
RS007EVAL	lanes	staged_other_wrong	threeway	0
RS007EVAL	lanes	reparent_lost	threeway	0
RS007EVAL	lanes	staged_stuck	threeway	0
RS007EVAL	lanes	wrong	threeway	0
RS007EVAL	lanes	one_sided_not_t	threeway	0
RS007EVAL	lanes	pick_no_hier_not_o	threeway	0
RS007EVAL	wide	histories	threeway	600
RS007EVAL	wide	panics	threeway	0
RS007EVAL	wide	avoidable	threeway	0
RS007EVAL	wide	staged_other_wrong	threeway	0
RS007EVAL	wide	reparent_lost	threeway	0
RS007EVAL	wide	staged_stuck	threeway	0
RS007EVAL	wide	wrong	threeway	0
RS007EVAL	wide	one_sided_not_t	threeway	0
RS007EVAL	wide	pick_no_hier_not_o	threeway	0
RS007EVAL	based	histories	threeway	600
RS007EVAL	based	panics	threeway	0
RS007EVAL	based	avoidable	threeway	0
RS007EVAL	based	staged_other_wrong	threeway	0
RS007EVAL	based	reparent_lost	threeway	0
RS007EVAL	based	staged_stuck	threeway	0
RS007EVAL	based	wrong	threeway	0
RS007EVAL	based	one_sided_not_t	threeway	0
RS007EVAL	based	pick_no_hier_not_o	threeway	0
RS007EVAL	multi	histories	threeway	600
RS007EVAL	multi	panics	threeway	0
RS007EVAL	multi	avoidable	threeway	0
RS007EVAL	multi	staged_other_wrong	threeway	0
RS007EVAL	multi	reparent_lost	threeway	0
RS007EVAL	multi	staged_stuck	threeway	0
RS007EVAL	multi	wrong	threeway	0
RS007EVAL	multi	one_sided_not_t	threeway	0
RS007EVAL	multi	pick_no_hier_not_o	threeway	0
RS007EVAL	policy	histories	threeway	600
RS007EVAL	policy	panics	threeway	0
RS007EVAL	policy	avoidable	threeway	0
RS007EVAL	policy	staged_other_wrong	threeway	0
RS007EVAL	policy	reparent_lost	threeway	0
RS007EVAL	policy	staged_stuck	threeway	0
RS007EVAL	policy	wrong	threeway	0
RS007EVAL	policy	one_sided_not_t	threeway	0
RS007EVAL	policy	pick_no_hier_not_o	threeway	0
RS007EVAL	strict	histories	threeway	600
RS007EVAL	strict	panics	threeway	0
RS007EVAL	strict	avoidable	threeway	0
RS007EVAL	strict	staged_other_wrong	threeway	0
RS007EVAL	strict	reparent_lost	threeway	0
RS007EVAL	strict	staged_stuck	threeway	0
RS007EVAL	strict	wrong	threeway	0
RS007EVAL	strict	one_sided_not_t	threeway	0
RS007EVAL	strict	pick_no_hier_not_o	threeway	0
RS007EVAL	edges	histories	threeway	600
RS007EVAL	edges	panics	threeway	0
RS007EVAL	edges	avoidable	threeway	0
RS007EVAL	edges	staged_other_wrong	threeway	0
RS007EVAL	edges	reparent_lost	threeway	0
RS007EVAL	edges	staged_stuck	threeway	0
RS007EVAL	edges	wrong	threeway	0
RS007EVAL	edges	one_sided_not_t	threeway	0
RS007EVAL	edges	pick_no_hier_not_o	threeway	0
RS007EVAL	full	histories	threeway	600
RS007EVAL	full	panics	threeway	0
RS007EVAL	full	avoidable	threeway	0
RS007EVAL	full	staged_other_wrong	threeway	0
RS007EVAL	full	reparent_lost	threeway	0
RS007EVAL	full	staged_stuck	threeway	0
RS007EVAL	full	wrong	threeway	0
RS007EVAL	full	one_sided_not_t	threeway	0
RS007EVAL	full	pick_no_hier_not_o	threeway	0
```

The report names its counts as follows against the acceptance's: staged_stuck is stuck, one_sided_not_t is
one_sided_off_t, pick_no_hier_not_o is pick_off_o; avoidable, wrong, staged_other_wrong, reparent_lost and panics are
the same counts. Its `ch_` rows, a secondary reading of "touched" that the decision does not adopt (13.2 (a) reads it by
state), show `ch_avoidable` 140; they are informative only.

No run needed a repeat, a trace or a lone re-run.

## 2. Verdict table

### 2.1 13.6 (a), the absolute acceptance

| Row | Clause | Verdict | Evidence |
|---|---|---|---|
| AC-1 | 0 avoidable `HierarchyCycle` stagings, nightly | MEETS | §1 nightly: avoidable=0 on all nine generators (300 histories each) |
| AC-2 | 0 avoidable, exit | MEETS | §1 exit: avoidable=0 on all nine (3,000 each) |
| AC-3 | 0 wrong landings, nightly | MEETS | wrong=0 on all nine |
| AC-4 | 0 wrong landings, exit | MEETS | wrong=0 on all nine |
| AC-5 | 0 stuck stagings, nightly | MEETS | stuck=0 on all nine |
| AC-6 | 0 stuck stagings, exit | MEETS | stuck=0 on all nine |
| AC-7 | The widened oracle: every node live in the result judged, `absent` a value, existence taken from the result (R-15 Q5) | MEETS | `suite::rs007eval::rs007_eval_oracle_judges_every_node_live_in_the_result`, `suite::rs007eval::rs007_eval_oracle_takes_a_fixed_nodes_value_from_the_result` and `suite::rs007eval::rs007_eval_oracle_classifies_by_state` ok in the full gate at f71c6b1 |
| AC-8 | The generators' additions: existence policies, `--strict`, `blocks` and `gates` edges, more tasks (at least 6, R-15 Q6) | MEETS | The generators `policy` (the kind's `merge.policy.task` row and `--policy`, delete-wins and resurrect), `strict` (`merge.strict`), `edges` (`blocks` and `gates`), `multi` and `full` run 6 tasks; each has its count line at both tiers (§1) |
| AC-9 | Re-key in the generators | outside | Outside by the orchestrator's ruling R-16 Q1, the owner told; never a MISSES |
| AC-10 | The relative lockstep tests retired | MEETS | No gate id under `suite::kleppmann` in the full gate at f71c6b1, and 0 hits for `kleppmann` in the model's sources; the module was removed by 09d9f4d |
| AC-11 | The further asserted counts: staged_other_wrong, reparent_lost, link_broken, one_sided_off_t, pick_off_o, panics, not_forest | MEETS | Each 0 on all nine generators at both tiers (§1) |
| AC-12 | nondet, the determinism check | MEETS, on a subset | nondet=0 at both tiers. The acceptance runs the first 4 histories of each (generator, seed) twice: 72 histories per tier, the same 72 at both tiers, not every history |
| AC-13 | A staging `--strict` asked for is `Requested`, no fault (R-16 Q3) | MEETS | requested=16 (strict) and 2 (full) at nightly, 69 and 35 at exit, none counted as a fault; link_broken=0; link_na 2 at nightly and 14 at exit, where the link check did not apply |
| AC-14 | Own seeds: the eight computed counts | MEETS | §1 own seeds, the per-generator rows quoted there: avoidable, wrong, staged_stuck, staged_other_wrong, reparent_lost, one_sided_not_t, pick_no_hier_not_o and panics are 0 on every generator, `histories` 600 each |
| AC-15 | Own seeds: link_broken, not_forest, nondet | INCONCLUSIVE | Not measured by the report; INCONCLUSIVE (W3E-ARB-S-8) |
| AC-16 | ckr_one_sided, a conflicted-key-row node RS-007 decides (informative) | recorded | Nightly: policy 14, full 10, the other seven 0. Exit: policy 82, full 118, the other seven 0. Non-zero, so the generators reach such nodes; with them, the corpus cases SF2B-TAKEN and SF2B-SHORTCUT carry the check (SC-2) |

### 2.2 The decision (OQ-A-13, 2026-10-07)

| Row | Item | Verdict | Evidence |
|---|---|---|---|
| DC-1 | 13.1 (a): K2 `threeway` is RS-007 | MEETS | RS-007's row states it (the sentences of TX-1 that meet); AC-1 to AC-6 |
| DC-2 | OQ-A-13 F1: the reset key lies on a cycle of the option graph restricted to U | MEETS | `merge::threeway::tests::f1_the_reset_key_lies_on_a_cycle_of_the_option_graph` ok |
| DC-3 | OQ-A-13 F2: a both-same key's origin is the later of its sides' | MEETS | `merge::threeway::tests::f2_a_both_same_keys_origin_is_the_later_of_its_sides` ok |
| DC-4 | OQ-A-13 F3: `absent` compared canonically in the origin walk | MEETS | `suite::vcs::the_origin_walk_compares_absent_canonically` ok |
| DC-5 | OQ-A-13 F4: a cycle with no repairable key goes to V05 | MEETS | `merge::threeway::tests::f4_a_cycle_with_no_repairable_key_is_left_to_the_validators` ok; the term's wording: W3E-ARB-S-4 |
| DC-6 | OQ-A-13 F5: the forest guarantee covers `HierarchyCycle` only | MEETS | `suite::k2probe::k2_a_precedence_cycle_a_two_sided_choice_closes_stages_as_today` ok |
| DC-7 | 13.2 (a): touched by state | MEETS | `suite::rs007eval::rs007_eval_oracle_classifies_by_state` and `suite::k2probe::k2_a_move_away_and_back_is_no_touch` ok; avoidable=0 (AC-1, AC-2) |
| DC-8 | 13.3 (b): the hint by default, the staging under `--strict` | MEETS | `suite::k2probe::k2_a_later_two_sided_move_that_closes_a_cycle_is_skipped_and_lands`, `suite::k2probe::k2_under_strict_a_skipped_two_sided_move_stages_a_hierarchy_cycle`, `suite::k2probe::k2_a_later_move_under_a_node_the_result_deletes_lands_with_a_hint`, `suite::k2probe::k2_under_strict_a_move_under_a_node_the_result_deletes_stages_a_dangling_edge` and `suite::rs007eval::rs007_eval_oracle_calls_a_strict_staging_of_a_two_sided_key_requested` ok; hints counted at both tiers (§1) |
| DC-9 | 13.4 (a), read by state (the owner's ruling R-20 of 2026-10-12), with the CONTESTED corpus ADV-C-6-yx and W3D-REV-5 pinned to it | MEETS | The acceptance asserts that each focus merge of a CONTESTED case lands at the reading "by the original moves' times", and it passed at both tiers; `suite::vcs::a_resolutions_origin_follows_the_state_it_lands` ok |
| DC-10 | 13.5 (b): a reparent beats a concurrent order-only move | MEETS | `suite::k2probe::k2_a_reparent_beats_a_concurrent_order_only_move` ok; reparent_lost=0 at both tiers |
| DC-11 | K4 | not needed | Every count 0 at both tiers, every corpus case and fixture passed |

### 2.3 Texts against the model

MEETS means that the text states what the model does; each row's evidence is its locator and, where one exists, a
gate result (all at f71c6b1).

| Row | Text | Verdict | Evidence and notes |
|---|---|---|---|
| TX-1 | MT RS-007 | MISSES | Its scope, "a node live in the result with no existence policy": the text and the model disagree for W3E-ARB-S-1's case and for W3E-ARB-S-2's (observable: no test pins either case). Every other sentence meets: **Values**, **Three-way**, 13.5 (b), MR-040's tie, **Origin** (the revert and cherry-pick case, the virtual case, F2), **Cycle repair** (the participants, step (1)'s tie-breaks in their order, step (2)'s order and the dead-parent preference), the hint and strict, **Guarantees** |
| TX-2 | MT RS-010, RS-015, PR-007 and PR-008 | MISSES | RS-010's last sentences and RS-015's, which follow them ("otherwise they merge key by key"), PR-007's "fixes none of the node's other keys, which merge key by key" and PR-008's per-key rows: the text and the model disagree for W3E-ARB-S-1's case; PR-007's "All other keys follow" also for W3E-ARB-S-2's (observable: no test pins either case). The sentences that meet: RS-010's and RS-015's base, flat sides and class, their one-live `DeleteVsModify` that takes RS-008's provisional state, and the provisional side; PR-007's re-keys first, existence keys first, the `policy` row that fixes the node by RS-008, and a deleted uid's retained out-edges; PR-008's `conflicted-key` rows first, then the class's rows in table order |
| TX-3 | MT MR-039 | MEETS | |
| TX-4 | MT MR-040 | MEETS | |
| TX-5 | MT CS-013 | MEETS | `merge::tests::a_cycle_repair_never_resets_an_order_only_key` ok |
| TX-6 | MT CM-011 | MISSES | Its gloss "one with no repairable key": W3E-ARB-S-4 (as RT-8; observable: no test shows a left cycle that holds a two-sided key). Every other sentence meets: VA-001 reports the keys RS-007's cycle repair reset or, under strict, skipped; VA-005 reports a cycle RS-007 left |
| TX-7 | MT PR-014 | MEETS | The dropped records on an overlaid key, the kept step-1 violations and the stale notice |
| TX-8 | MT PR-016 | MEETS | The origins it lists are the ones RS-007 reads. The dropped-set input: not checked here (R-MODEL WP-91's) |
| TX-9 | MT PR-017 | MEETS | Step (2): ascending origin, then least uid |
| TX-10 | MT VA-001 | MEETS | |
| TX-11 | MT VA-004 | MEETS | `merge::tests::a_dead_parent_key_whose_other_value_closes_a_cycle_stages_outside_strict` and `merge::tests::a_two_sided_key_with_both_values_under_deleted_parents_stages_outside_strict` ok |
| TX-12 | MT VA-005 | MISSES | Its gloss "one with no repairable key": W3E-ARB-S-4 (as RT-8; observable: no test shows a left cycle that holds a two-sided key). Every other sentence meets: every cycle V05 finds is keyed by its least uid |
| TX-13 | MT VB-011 | MEETS | `suite::vcs::vbc_10_a_cycle_in_the_virtual_base_is_skipped_silently` ok |
| TX-14 | MT HT-004 | MEETS | The line form `<key> took <side>; <other> skipped (<reason>)`, canonical key order, none inside a virtual merge |
| TX-15 | MT open point 15 | MEETS | A summary of RS-007 that agrees with it |
| TX-16 | MT open point 35 | MEETS | The 21 tests it names exist and are ok, and `suite::rs007eval::rs007_eval_absolute_acceptance` ok |
| TX-17 | DPM XB-004 | MEETS | As TX-11 |
| TX-18 | F12 §5.3 VM-7 | MISSES | Its "A cycle with no repairable key": W3E-ARB-S-4 (observable: no test shows a left cycle that holds a two-sided key). Every other sentence meets: dst's origin from the folded LCAs, src's from Lᵢ; strict off; silent |
| TX-19 | F12 §5.5 "Hierarchy cycles" | MISSES | Its "A cycle with no repairable key": W3E-ARB-S-4 (observable: no test shows a left cycle that holds a two-sided key). Every other sentence meets: no validator runs inside a virtual merge |
| TX-20 | F12 §5.5a | MEETS | The walks start at each side's tip or each folded LCA; C is read with no walk |
| TX-21 | F12 §5.6 | MEETS | Every input except the dropped set; the dropped-set input: not checked here (R-MODEL WP-91's) |
| TX-22 | F12 §5.7 VBC-10 | MEETS | As TX-13 |
| TX-23 | F12 §6.5 | MEETS | The restore of a `live` side's value keys, hierarchy key and out-edges (from the state of a parent of M, the merge that made the conflict value), the refusal of a restore under a parent no longer live, and a resolved value's origin by state |
| TX-24 | F12 §7.1 | MEETS | |
| TX-25 | F12 §7.2 | MEETS | A key equal in b, o and t is never reset |
| TX-26 | F12 §7.4 | MEETS | (hlc, commit id) order; the quoted sentences equal RS-007's |
| TX-27 | F12 §7.9 | MEETS | V01's key; V05's least uid for all three kinds of cycle |
| TX-28 | F12 §9.4 | MEETS | As TX-7 |
| TX-29 | F12 open point 33 | MISSES | Its "a cycle with no repairable key": W3E-ARB-S-4 (observable: no test shows a left cycle that holds a two-sided key). Every other sentence meets: a summary that agrees with the rows it names |
| TX-30 | F13 I4 | MISSES | Its "a cycle with no repairable key": W3E-ARB-S-4 (observable: no test shows a left cycle that holds a two-sided key). Every other sentence meets |
| TX-31 | F13 I25′ | MEETS | avoidable=0 at both tiers |
| TX-32 | F13 V01 | MISSES | Its scope, "a node live in the result with no existence policy", as RS-007's: the text and the model disagree for W3E-ARB-S-1's case and for W3E-ARB-S-2's (observable: no test pins either case); its "A cycle with no repairable key is V05's": W3E-ARB-S-4 (observable: no test shows a left cycle that holds a two-sided key). Every other sentence meets: the moves applied; RS-007 applied before anything else; at a `merge --continue`, a key an applying resolution set holds its resolved value; the reset and skipped keys it reports (MR-039) and the `cycle` hint outside strict; the dead-parent sentence and V04's `DanglingEdge` under strict (VA-004); at a `merge --continue`, the keys and hints an applying resolution set dropped (PR-014); its key and hint line |
| TX-33 | F13 V05 | MISSES | Its "because it has no repairable key": W3E-ARB-S-4 (as RT-8; observable: no test shows a left cycle that holds a two-sided key). Every other sentence meets |
| TX-34 | F13 OP-13-17 | MISSES | Its "one with no repairable key": W3E-ARB-S-4 (observable: no test shows a left cycle that holds a two-sided key). Every other sentence meets |
| TX-35 | F19 §12.2 code 64 | MISSES | Its "with no repairable key": W3E-ARB-S-4 (observable: no test shows a left cycle that holds a two-sided key). Every other sentence meets |
| TX-36 | F19 §12.3 code 132 | MEETS | The line form equals HT-004's |
| TX-37 | F12 §5.4, the last bullet of RVB-4 | MISSES | Its "otherwise they merge key by key as §6.3 states": the text and the model disagree for W3E-ARB-S-1's case (observable: no test pins the case). Every other sentence meets: the one-live `DeleteVsModify` takes RS-008's provisional state; the provisional side is the side RS-008's policy names when exactly one flat side is live, and `ours` otherwise |
| TX-38 | F12 §6.3 "The node's other keys" | MISSES | Its "the node's other keys still merge key by key ..., and a change made by one side only is kept" and "The value fixes none of those keys to the side it came from": the text and the model disagree for W3E-ARB-S-1's case, where the value the automatic side takes may be a side's conflict value, taken unchanged (observable: no test pins the case). Every other sentence meets: outside that case, the conflict value attached when its provisional value is live and a tombstone merge when it is deleted; a new one-live `DeleteVsModify` takes RS-008's provisional state |
| TX-39 | F19 open point 41 | MISSES | Its "a cycle RS-007 left, with no repairable key": W3E-ARB-S-4 (observable: no test shows a left cycle that holds a two-sided key). Every other sentence meets: code 64's sources as CM-011 states them (TX-35), `MoveSkipped` as code 132 (TX-36), `<skey>` where HT-004 writes `<key>`. Its last sentence records [F13 §5] V01 as it stood when the point was written; V01 now states RS-007's rule (TX-32) |

### 2.4 RS-007's scope, the oracle's IF-1 handling and the K2 check

| Row | Check | Verdict | Evidence |
|---|---|---|---|
| SC-1 | Who decides the hierarchy key of a node whose existence key holds a conflict value a `conflicted-key` row gave: RS-007 when the provisional value is live (RS-007's "no existence policy"; [F12 §6.3] "The node's other keys"; PR-007; the owner's ruling R-43 of 2026-10-13, OI-1g), RS-008's provisional state for a new one-live `DeleteVsModify` | MISSES | Outside W3E-ARB-S-1's case the texts and the model agree. In that case, an automatic side on an existence key that MR-002 or MR-005 decides, the texts and the model disagree: W3E-ARB-S-1 (as TX-1, TX-2, TX-37, TX-38 and TX-32; observable: no test pins the case) |
| SC-2 | The oracle's IF-1 handling still judges such a node | MEETS | `suite::rs007eval::rs007_eval_judges_a_conflicted_key_row_task_and_fixes_a_kept_case_task` ok; the corpus cases SF2B-TAKEN, SF2B-SHORTCUT and FIXED-COPY pass inside the acceptance at both tiers; ckr_one_sided non-zero (AC-16) |
| SC-3 | Where the oracle's "fixed" and the model's fixed node differ: (i) an automatic side | finding | W3E-ARB-S-1 |
| SC-4 | (ii) a one-live `DeleteVsModify` equal to a side's value | finding | W3E-ARB-S-7 |
| SC-5 | (iii) an existence decision that raises a violation | finding | W3E-ARB-S-2 |
| SC-6 | The K2 return clause of the key-by-key merge (the owner's ruling R-43 of 2026-10-13, OI-1g; W3D-SF-2) | MEETS | None of the return conditions arose: every count is 0 at both tiers and no corpus case faults |

### 2.5 Items routed to this check

Each is another review's claim, checked as a claim.

| Row | Claim | Verdict | Evidence |
|---|---|---|---|
| RT-1 | PR-007 and RS-010 do not except the automatic sides AP-002 and AP-003, which the model fixes | MISSES | The disagreement the claim names holds, but not its framing, that the texts are at fault: W3E-ARB-S-1 states it neutrally (as TX-2; observable: no test pins the case) |
| RT-2 | RK-008 does not state the refusal at the last `#N` for a preview or a dry run | MISSES | W3E-ARB-S-3 (observable: `suite::files::a_checked_or_dry_sync_at_the_last_node_id_is_refused` ok in the full gate at f71c6b1). [API §9.6]'s same gap is already in R-SPEC-F's queue: recorded, not re-raised |
| RT-3 | The harness's fixed-node test can disagree with the model's, and "no verdict changes" | the claim MEETS; finding | No verdict changes: checked by reading, without a probe (PLAUSIBLE, as W3E-ARB-S-7). The disagreement itself: W3E-ARB-S-7 |
| RT-4 | [API §9.6] item 2 for `merge --continue`: every `#N` the reply reports is the `#N` the node lands with | MEETS | The text states it for every merge-family reply. No test pins the continue's reply: W3E-ARB-S-6 |
| RT-5 | [F05 §9.5] "Entry order" and [F13 §4] MC-6, re-checked independently | MEETS | Ascending `#N`, a leaving entry before a joining one; ME-005's and ME-011's entries by identity over both sections; `suite::gt18::a_branch_deletion_clears_in_identity_order_whichever_section_holds_them`, `suite::gt18::a_branch_deletion_clears_three_markers_in_identity_order_across_both_sections` and `suite::gt18::a_commit_flags_nonlinear_markers_in_identity_order_whichever_section_holds_them` ok |
| RT-6 | RS-007, VA-004, VA-005 and XB-004 on a two-sided key both of whose values have a deleted parent | MEETS | The dead-parent preference needs the other value's parent live or none, so the three-way value stays and VA-004 stages it; `merge::tests::a_two_sided_key_with_both_values_under_deleted_parents_stages_outside_strict` ok |
| RT-7 | The same rows on a two-sided key, not preferred, whose three-way value closes a cycle and whose other value is under a deleted parent | MEETS | Step (2) lands it at the other value with a `cycle` hint, and VA-004's first sentence stages its dangling parent; no sentence says otherwise |
| RT-8 | VA-005's "one with no repairable key" | MISSES | W3E-ARB-S-4 (`merge::threeway::tests::f4_a_cycle_with_no_repairable_key_is_left_to_the_validators` ok; observable: no test shows a left cycle that holds a two-sided key) |
| RT-9 | The "or none" half of the dead-parent preference is pinned by no test | confirmed | W3E-ARB-S-5. The texts state it exactly |
| RT-10 | The determinism check covers a subset | confirmed | AC-12 states the subset and its count |

One further routed item concerns planning notes outside the repository; it has no repository text and is recorded
outside this report.

### 2.6 Known findings: closed or open

| Row | Finding (by its text) | State |
|---|---|---|
| KF-1 | F12 §5.5a described every origin as a walk from a side's tip | closed by b05385e |
| KF-2 | F12 §5.6 listed only the `hlc` of the commits the origin walks reach | closed by b05385e |
| KF-3 | F19 §10.1's `<skey>` did not cover a hint key | closed by 632cdb1 |
| KF-4 | PR-014 named only "VA-001's reset key" | closed by 5c96db7 |
| KF-5 | F13 V01 did not say what an applying resolution's key holds at `merge --continue` | closed by 14c7cc3 |
| KF-6 | F12 §7.9 and VA-005 keyed only a cycle RS-007 left | closed by b05385e and 5c96db7 |
| KF-7 | F19 code 64 named a narrower set of V05's cycles than F13 | closed by 632cdb1 |
| KF-8 | MT RS-007, MT open point 15 and F12 §7.4 cite "the owner's ruling R-20 of 2026-10-12", which `owner-questions.md` does not hold, and OQ-A-13's 13.4 (a) there still words a resolution to base as producing a new origin | closed by e1178b5 |

### 2.7 Records only

- F12 §5.6's dropped set as an input of the virtual base a `Resolve` reads: R-MODEL WP-91, 21e250b; not checked here.
- F13 §6.2's `suspect` under the owner's ruling R-90 of 2026-10-19, `suite::n40`'s NG-005 pin, and DPM NN-007 and
  NG-005: R-MODEL's queue under that ruling; not checked here.
- `BodyDrop` and the store's dropped set: not checked here.
- F12 §6.5 with a merge M of one parent, and the alignment of F12 §7.9's `HierarchyCycle` key with F13 V05: left to the
  next S-lens pass.

## 3. Findings

### W3E-ARB-S-1 (Important, CONFIRMED): under an automatic side, the texts merge a node's other keys key by key and the model does not

**The texts.** PR-007 says that an existence value a `conflicted-key` row gives (MR-001 to MR-005), outside RS-008's
cases and a new `DeleteVsModify` whose flat sides are one live and one deleted, "fixes none of the node's other keys,
which merge key by key", and PR-008 then decides each remaining key by its own rows. RS-010's last sentences, which
RS-015 follows, say that otherwise the node's other keys "merge key by key as [F12 §6.3] states", and [F12 §5.4]'s last
bullet of RVB-4 says the same. [F12 §6.3] "The node's other keys" says that when a node's existence key holds a
conflict value a `conflicted-key` row gives ("a side's, taken unchanged, or a new one made over a conflicted base or
side"), the node's other keys still merge key by key, "a change made by one side only is kept", and the value fixes
none of them to the side it came from. RS-007's scope, "a node live in the result with no existence policy", which
[F13 §5] V01 repeats, gives such a live node's hierarchy key to RS-007. These texts carry the owner's ruling R-43 of
2026-10-13, OI-1g: "the other keys still merge key by key, and a change made by one side only is kept".

**The case.** `merge.policy.<kind>` = `ours` or `theirs` (AP-002, AP-003; never inside a virtual merge), and an
existence key that MR-002 or MR-005 decides, which takes the automatic side's value. That value may be a conflict
value, since MR-005 needs one on at least one side; it is then taken unchanged and carried. This merge makes no
conflict value of its own, and a carried value is reported as PR-011 states.

**The outcome** in the merge result, when the node's flat sides are both live or both deleted: the node's other keys
are the automatic side's, not merged key by key; a change only the other side made is not kept; and RS-007 does not
decide a live node's hierarchy key. (When both sides hold a plain tombstone, the two merge key by key, as the texts
state.)

**Why CONFIRMED.** AP-002's "the key takes o" can be read two ways, and neither leaves a text that fixes the node. If
the row gives the value, PR-007 (which names MR-002 and MR-005) and [F12 §6.3] ("a side's, taken unchanged") cover it
and say key by key. If the automatic side gives it, no text speaks of the node's other keys, so PR-008 decides them key
by key, and RS-007's scope takes a live node, because an automatic side is no existence policy (AP-002 and AP-003 apply
to value conflicts; AP-004 and AP-005 are the existence policies). So the disagreement holds under both readings. The
case was traced on the model at f71c6b1, for `ours` and `theirs` and for MR-002 and MR-005.

**The one-live case, decided by the owner's ruling R-109 of 2026-10-20.** Under `merge.policy.<kind>` = `ours` or
`theirs` (AP-002, AP-003; never inside a virtual merge), the automatic side decides the node's existence key in place of
the conflict value MR-002 or MR-005 would make, and the node's flat sides are one live and one deleted, whichever side
holds a conflict value. Key by key has no stated meaning there (a tombstone's other keys are `absent`, [F07 §6.4]), and
no text at f71c6b1 states what the node takes. The owner's ruling R-109 of 2026-10-20 decides it: the node is taken
whole from the automatic side, live with that side's keys when that side is live and deleted when it is deleted; the
merge makes no conflict value, and a conflict value that side holds is carried and reported as PR-011 states; the node
counts as a node an existence policy fixed, so RS-007's hierarchy rule does not decide it. The model's outcome at
f71c6b1 is the ruled one in every form of the case (the conflict value on the live side or on the deleted side; the
automatic side live or deleted), so this finding's disagreement is the cases where the flat sides are both live or both
deleted. The texts that are to state the ruled outcome are R-MODEL WP-90's (RS-007's scope words; PR-007's, AP-002's
and AP-003's notes), R-SPEC-F WP-12's ([F12 §6.3]) and R-SPEC-P WP-16's ([F13 §5] V01); the model, R-MODEL WP-91,
already matches it.

**Observable.** No test sets up MR-002 or MR-005 under an automatic side, and none asserts such a node's other keys.
The two tests that set an automatic side on an existence key,
`merge::tests::delete_versus_modify_follows_the_existence_policy` and
`policykeys::tests::every_value_of_every_policy_row`, are ok in the full gate at f71c6b1; on a `DeleteVsModify` over
two plain sides (MR-042) they assert the existence key only.
`suite::vcs::every_cached_state_equals_the_fold_of_its_changesets`, ok there too, writes `ours` and `theirs` among its
random steps and asserts only that each cached state equals the fold of its changesets. The acceptance's generators set no automatic side (their policy rows are `delete-wins` and
`resurrect`).

**Severity.** Important: a change only one side made is lost without a conflict, which OI-1g rules out.

**Owners, both sides.** This report chooses neither. If the model changes: R-MODEL WP-91. If the texts change instead:
R-MODEL WP-90 (PR-007, PR-008, RS-010, RS-015, RS-007's scope, AP-002, AP-003), R-SPEC-F WP-12 ([F12 §5.4], §6.3) and
R-SPEC-P WP-16 ([F13 §5] V01); that would narrow the owner's ruling OI-1g, so it is the owner's. The one-live case's
four owners are named above.

### W3E-ARB-S-2 (Minor, PLAUSIBLE): an existence key that stages `IdCollision`: the texts and the model disagree on the node's other keys

MR-045 (`both-created`, `stage-take-o`, `IdCollision`) is the one existence row with a structural disposition. RS-007's
scope and PR-007 ("All other keys follow"), and [F13 §5] V01, which repeats the scope, give no outcome for a node whose
existence key stages `IdCollision`, and the merge result does not merge its other keys key by key. Observable: no test
exercises it, and the generators never reach it (a uid equal on two sides only by a fault). PLAUSIBLE because
`stage-take-o` can be read as taking o's node. Owners, both sides: R-MODEL WP-91 for the model, or R-MODEL WP-90
(RS-007's scope, PR-007) and R-SPEC-P WP-16 (V01) for the texts.

### W3E-ARB-S-3 (Minor, PLAUSIBLE): RK-008 omits the refusal at the last `#N` for a preview or a dry run

RK-008 says that "a preview or a dry run allocates nothing and reports `next_id` + k − 1". At the bound, where
`next_id` + k − 1 exceeds 2^32 − 2, the model refuses the preview or dry run with `id_space_exhausted`, as the landing
would be refused; [F04 §5.7] states the refusal for "a write" only. Observable:
`suite::files::a_checked_or_dry_sync_at_the_last_node_id_is_refused` ok in the full gate at f71c6b1, which pins the
refusal. PLAUSIBLE because the bound can be read as implied. Owner: R-MODEL WP-92 (one clause in RK-008); [API §9.6]'s
same gap is R-SPEC-F's, already queued.

### W3E-ARB-S-4 (Minor, PLAUSIBLE): "a cycle with no repairable key" uses a term no text defines

VA-005 and CM-011 gloss the cycle RS-007 leaves as "one with no repairable key", and F12 §5.3 VM-7, §5.5 and open point
33, F13 I4, V01, V05 and OP-13-17, and F19 code 64 and open point 41 repeat the phrase. No rule or spec text defines
"repairable". RS-007's step (1) lists what such a cycle may hold: "only fixed nodes, untouched keys, keys whose value
and fallback have the same parent, two-sided keys and keys already reset". A two-sided key whose two values differ in
parent is in that list, yet read plainly it is a key the repair could change. So the gloss is exact only when "repairable" means "a key step (1) may
reset". Observable: `merge::threeway::tests::f4_a_cycle_with_no_repairable_key_is_left_to_the_validators` ok (the cycle
is left as RS-007 states); no test shows a left cycle that holds a two-sided key. Owner: R-MODEL WP-90 (define the term
at RS-007's step (1), or replace the gloss by "a cycle step (1) cannot break"); the format chapters follow (R-SPEC-F
WP-12 and WP-18, R-SPEC-P WP-16).

### W3E-ARB-S-5 (Minor, CONFIRMED; a test gap): the dead-parent preference's "or none" is pinned by no test

RS-007, VA-004, XB-004 and open point 15 state that a two-sided key whose three-way value is under a deleted parent tries
the other side's value first when that value has a live parent **or none**. The texts are exact. The three tests of
the preference, `merge::tests::a_dead_parent_key_whose_other_value_closes_a_cycle_stages_outside_strict`,
`merge::tests::a_two_sided_key_with_both_values_under_deleted_parents_stages_outside_strict` and
`suite::k2probe::k2_a_later_move_under_a_node_the_result_deletes_lands_with_a_hint`, each ok in the full gate at
f71c6b1, use an other value with a live parent; no test was found whose other value is a root. Owner: R-MODEL WP-91
(a test of a key whose other value is a root).

### W3E-ARB-S-6 (Minor, CONFIRMED; a test gap): no test fails if `merge --continue` reports placeholder `#N`s

[API §9.6] item 2 requires every `#N` a merge-family reply reports to be the `#N` the node lands with (RT-4). A test
pins it for the cherry-pick's reply: `suite::files::a_dry_cherry_pick_reports_next_id_for_a_new_uid` ok in the full
gate at f71c6b1. No test of `merge --continue`'s reply `#N` was found among the model's tests, by name or by doc
comment, so no test fails if the continue's reply reports placeholder `#N`s. Owner: R-MODEL WP-91 (a test of a
continue that lands a node new to the store).

### W3E-ARB-S-7 (Minor, PLAUSIBLE; the harness): ckr_one_sided may count a node RS-007 does not decide

RS-010's last sentences fix a node to RS-008's provisional state for every new one-live `DeleteVsModify` from RS-010
or RS-015, also one equal to a side's existence value (reachable when a side holds a `resurrect` value made over a
delete and the other side deleted the task alike from the same base), and the model does so. Such a node is counted in
ckr_one_sided, which may exceed the nodes RS-007 decides; no asserted count moves (RT-3). PLAUSIBLE: checked by reading,
without a probe. Owner: R-MODEL WP-91.

### W3E-ARB-S-8 (Minor, CONFIRMED; the harness): own seeds cannot measure link_broken, not_forest or nondet

The acceptance's histories come from seeds 1 and 2 only, and the evaluation report, which takes other seeds, computes eight of the
acceptance's eleven counts: not link_broken, not_forest or nondet. AC-15 is therefore INCONCLUSIVE. Observable: the own
seeds run's output holds no row for those three. Owner: R-MODEL WP-91 (report the three counts in the evaluation
report, or give the acceptance a seed setting).

### W3E-ARB-S-9 (note; closed by e1178b5): OQ-A-13's 13.4 (a) and R-20

`owner-questions.md` OQ-A-13 13.4 (a) still reads "a move, a revert, and a resolution to base or to a new value produce a
new one", while MR-040 and F12 §6.5 read origins by state (a resolution whose value equals a parent's passes that
parent's origin on, `base` included), and RS-007, open point 15 and F12 §7.4 cite for it "the owner's ruling R-20 of
2026-10-12", which `owner-questions.md` does not hold. The model follows the rules (DC-9). Owner: the owner's status
line for OQ-A-13.

Closed by e1178b5: `owner-questions.md` now holds, under OQ-A-13, the ruling "13.4 (a) is read by state" of 2026-10-12,
which it says the rules cite as R-20, and the outcome of 2026-10-20. The text above is kept as the record.

## 4. Checks run

- The three runs of §1, each through the run kit on f71c6b1, with one expected test each, `ok`; each tier's count lines
  checked for presence, order, form, the tier's histories, the tier word and the 11 counts; the nightly's against a
  fresh nightly reference at the same commit, equal byte for byte; own seeds' first line, `histories` rows (600 per
  generator, 5,400 in all) and eight counts.
- Test outcomes: the full gate (`cargo xtask gate --branch m0/r-rev-s`) at f71c6b1, 2,812 tests, 2,806 ok and 6
  ignored (the sum of its `test result:` lines); each test this report names is `ok` there, except
  `suite::rs007eval::rs007_eval_report`, which the gate ignores (`generator`) and the own-seeds run of §1 ran `ok`.
- Each text of §2.3 read against the model's merge, its validators, its origin walk, its virtual base, its continue
  and its markers at f71c6b1; each closing commit of §2.6 read with `git show --stat`.
- No probe, no mutation and no edit outside this file.
- The amendment (2026-10-20): each claim of the audit of the report's first version re-read against the texts and the
  model at f71c6b1, and each held; the per-generator rows of own seeds checked against the run, line for line and in
  the run's order; the model's tests at f71c6b1 searched by name and doc comment for a test of `merge --continue`'s
  reply `#N` (none found); `owner-questions.md` read at the commit named in the Commit row. No probe, no mutation and
  no run.
