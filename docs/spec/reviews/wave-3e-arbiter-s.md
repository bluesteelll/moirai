# Wave 3e: the S lens arbiter's check of RS-007 `threeway` (OQ-A-13)

| Field | Value |
|---|---|
| Status | `open`: 13.6 (a) is met at the nightly and exit tiers and on four further seeds; every decided item meets; four text findings, four test or harness findings and one owner note go to their owners (§3) |
| Scope | OQ-A-13's absolute acceptance (13.6 (a)) re-run at both tiers and on own seeds; the decision's items 13.1 to 13.5 with F1 to F5; RS-007's scope against the key-by-key merge of a conflicted-key-row node (the owner's ruling R-43 of 2026-10-13, OI-1g; W3D-SF-2); the rule and spec texts of wave 3e against the model's merge |
| Rules | `rules/merge-table.md` RS-007, RS-010, MR-039, MR-040, CS-013, CM-011, PR-007, PR-014, PR-016, PR-017, VA-001, VA-004, VA-005, VB-011, HT-004, open points 15 and 35; `rules/delete-policy-matrix.md` XB-004; `rules/link-merge-rules.md` RK-008 |
| Format | `format/12-vcs.md` §5.3 VM-7, §5.5, §5.5a, §5.6, §5.7 VBC-10, §6.5, §7.1, §7.2, §7.4, §7.9, §9.4, open point 33; `format/13-invariants.md` I4, I25′, V01, V05, MC-6, OP-13-17; `format/19-errors-and-output.md` §12.2 code 64, §12.3 code 132; `format/05-log.md` §9.5; `store-api.md` §9.6 item 2 |
| Decision | `reviews/owner-questions.md` OQ-A-13 (13.6 (a); the decision of 2026-10-07; the recommendation's "a spec arbiter re-runs the widened harness and checks the statement") |
| Commit | Every run and every read used f71c6b193abff73286c4421f47012f2bc7d0e3f4 (master at the start); the run kit's hash is 62a979caa8b7b322d9692e591d321d0a170da7cd8bac9fb9c0a212d2a4117d00 |
| Date | 2026-10-20 |
| Role | R-REV-S, the S lens arbiter (WP-80; rule S5: a separate session, which wrote none of the checked texts) |

## Ruling

**OQ-A-13 13.6 (a) is met.** At the nightly tier (300 histories per generator) and at the exit tier (3,000 per
generator), on all nine generators, every one of the acceptance's 11 asserted counts is 0: 0 avoidable `HierarchyCycle`
stagings, 0 wrong landings, 0 stuck stagings, and 0 for staged_other_wrong, reparent_lost, link_broken, one_sided_off_t,
pick_off_o, panics, not_forest and nondet. Every corpus case and link fixture of the acceptance passes, the CONTESTED
corpus included. On four seeds of its own (3 to 6; 5,400 histories) the evaluation report's eight computed fault counts
are 0; it does not compute link_broken, not_forest or nondet, so those three are INCONCLUSIVE on own seeds (W3E-ARB-S-8).
OQ-A-13's fallback K4 is not needed. None of the return conditions of the key-by-key merge of a conflicted-key-row node
against K2 arose. The rule and spec texts state what the model does, with four Minor exceptions (W3E-ARB-S-1 to -4), and
four test or harness gaps are recorded (W3E-ARB-S-5 to -8). No finding is Critical or Important.

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
→ `suite::rs007eval::rs007_eval_report` ok. Its first line, and the `all` rows this report relies on (each of the nine
generators' rows reads `histories` 600 and the same eight counts 0):

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
| AC-13 | A staging `--strict` asked for is `Requested`, no fault (R-16 Q3) | MEETS | requested=16 (strict) and 2 (full) at nightly, 69 and 35 at exit, none counted as a fault; link_broken=0 on every one of them |
| AC-14 | Own seeds: the eight computed counts | MEETS | §1 own seeds: avoidable, wrong, staged_stuck, staged_other_wrong, reparent_lost, one_sided_not_t, pick_no_hier_not_o and panics are 0 on every generator, 600 histories each |
| AC-15 | Own seeds: link_broken, not_forest, nondet | INCONCLUSIVE | Not measured by the report; INCONCLUSIVE (W3E-ARB-S-8) |
| AC-16 | ckr_one_sided, a conflicted-key-row node RS-007 decides (informative) | recorded | Nightly: policy 14, full 10, the other seven 0. Exit: policy 82, full 118, the other seven 0. Non-zero, so the generators reach such nodes; with them, the corpus cases SF2B-TAKEN and SF2B-SHORTCUT carry the check (SC-2) |

### 2.2 The decision (OQ-A-13, 2026-10-07)

| Row | Item | Verdict | Evidence |
|---|---|---|---|
| DC-1 | 13.1 (a): K2 `threeway` is RS-007 | MEETS | RS-007's row states it (TX-1); AC-1 to AC-6 |
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
| TX-1 | MT RS-007 | MEETS | Every sentence checked: scope, **Values**, **Three-way**, 13.5 (b), MR-040's tie, **Origin** (the revert and cherry-pick case, the virtual case, F2), **Cycle repair** (the participants, step (1)'s tie-breaks in their order, step (2)'s order and the dead-parent preference), the hint and strict, **Guarantees**. Its scope has two exceptions it does not state: W3E-ARB-S-1, W3E-ARB-S-2 |
| TX-2 | MT RS-010 and PR-007 | MISSES | W3E-ARB-S-1, W3E-ARB-S-2 |
| TX-3 | MT MR-039 | MEETS | |
| TX-4 | MT MR-040 | MEETS | |
| TX-5 | MT CS-013 | MEETS | `merge::tests::a_cycle_repair_never_resets_an_order_only_key` ok |
| TX-6 | MT CM-011 | MEETS | Except its gloss "one with no repairable key" (RT-8; W3E-ARB-S-4) |
| TX-7 | MT PR-014 | MEETS | The dropped records on an overlaid key, the kept step-1 violations and the stale notice |
| TX-8 | MT PR-016 | MEETS | The origins it lists are the ones RS-007 reads. The dropped-set input: not checked here (R-MODEL WP-91's) |
| TX-9 | MT PR-017 | MEETS | Step (2): ascending origin, then least uid |
| TX-10 | MT VA-001 | MEETS | |
| TX-11 | MT VA-004 | MEETS | `merge::tests::a_dead_parent_key_whose_other_value_closes_a_cycle_stages_outside_strict` and `merge::tests::a_two_sided_key_with_both_values_under_deleted_parents_stages_outside_strict` ok |
| TX-12 | MT VA-005 | MEETS | Every cycle V05 finds is keyed by its least uid; except its gloss "one with no repairable key" (RT-8; W3E-ARB-S-4) |
| TX-13 | MT VB-011 | MEETS | `suite::vcs::vbc_10_a_cycle_in_the_virtual_base_is_skipped_silently` ok |
| TX-14 | MT HT-004 | MEETS | The line form `<key> took <side>; <other> skipped (<reason>)`, canonical key order, none inside a virtual merge |
| TX-15 | MT open point 15 | MEETS | A summary of RS-007 that agrees with it |
| TX-16 | MT open point 35 | MEETS | The 21 tests it names exist and are ok, and `suite::rs007eval::rs007_eval_absolute_acceptance` ok |
| TX-17 | DPM XB-004 | MEETS | As TX-11 |
| TX-18 | F12 §5.3 VM-7 | MEETS | dst's origin from the folded LCAs, src's from Lᵢ; strict off; silent |
| TX-19 | F12 §5.5 "Hierarchy cycles" | MEETS | No validator runs inside a virtual merge |
| TX-20 | F12 §5.5a | MEETS | The walks start at each side's tip or each folded LCA; C is read with no walk |
| TX-21 | F12 §5.6 | MEETS | Every input except the dropped set; the dropped-set input: not checked here (R-MODEL WP-91's) |
| TX-22 | F12 §5.7 VBC-10 | MEETS | As TX-13 |
| TX-23 | F12 §6.5 | MEETS | The restore of a `live` side's value keys, hierarchy key and out-edges (from the state of a parent of M, the merge that made the conflict value), the refusal of a restore under a parent no longer live, and a resolved value's origin by state |
| TX-24 | F12 §7.1 | MEETS | |
| TX-25 | F12 §7.2 | MEETS | A key equal in b, o and t is never reset |
| TX-26 | F12 §7.4 | MEETS | (hlc, commit id) order; the quoted sentences equal RS-007's |
| TX-27 | F12 §7.9 | MEETS | V01's key; V05's least uid for all three kinds of cycle |
| TX-28 | F12 §9.4 | MEETS | As TX-7 |
| TX-29 | F12 open point 33 | MEETS | A summary that agrees with the rows it names |
| TX-30 | F13 I4 | MEETS | |
| TX-31 | F13 I25′ | MEETS | avoidable=0 at both tiers |
| TX-32 | F13 V01 | MEETS | |
| TX-33 | F13 V05 | MEETS | Except the phrase "no repairable key" (RT-8; W3E-ARB-S-4) |
| TX-34 | F13 OP-13-17 | MEETS | |
| TX-35 | F19 §12.2 code 64 | MEETS | |
| TX-36 | F19 §12.3 code 132 | MEETS | The line form equals HT-004's |

### 2.4 RS-007's scope, the oracle's IF-1 handling and the K2 check

| Row | Check | Verdict | Evidence |
|---|---|---|---|
| SC-1 | Who decides the hierarchy key of a node whose existence key holds a conflict value a `conflicted-key` row gave: RS-007 when the provisional value is live (RS-007's "no existence policy"; [F12 §6.3] "The node's other keys"; PR-007; the owner's ruling R-43 of 2026-10-13, OI-1g), RS-008's provisional state for a new one-live `DeleteVsModify` | MEETS, with two cases outside the texts | The texts and the model agree for every case they name; the two they do not name are W3E-ARB-S-1 and W3E-ARB-S-2 |
| SC-2 | The oracle's IF-1 handling still judges such a node | MEETS | `suite::rs007eval::rs007_eval_judges_a_conflicted_key_row_task_and_fixes_a_kept_case_task` ok; the corpus cases SF2B-TAKEN, SF2B-SHORTCUT and FIXED-COPY pass inside the acceptance at both tiers; ckr_one_sided non-zero (AC-16) |
| SC-3 | Where the oracle's "fixed" and the model's fixed node differ: (i) an automatic side | finding | W3E-ARB-S-1 |
| SC-4 | (ii) a one-live `DeleteVsModify` equal to a side's value | finding | W3E-ARB-S-7 |
| SC-5 | (iii) an existence decision that raises a violation | finding | W3E-ARB-S-2 |
| SC-6 | The K2 return clause of the key-by-key merge (the owner's ruling R-43 of 2026-10-13, OI-1g; W3D-SF-2) | MEETS | None of the return conditions arose: every count is 0 at both tiers and no corpus case faults |

### 2.5 Items routed to this check

Each is another review's claim, checked as a claim.

| Row | Claim | Verdict | Evidence |
|---|---|---|---|
| RT-1 | PR-007 and RS-010 do not except the automatic sides AP-002 and AP-003, which the model fixes | MISSES | W3E-ARB-S-1 (as TX-2) |
| RT-2 | RK-008 does not state the refusal at the last `#N` for a preview or a dry run | MISSES | W3E-ARB-S-3. [API §9.6]'s same gap is already in R-SPEC-F's queue: recorded, not re-raised |
| RT-3 | The harness's fixed-node test can disagree with the model's, and "no verdict changes" | the claim MEETS; finding | No verdict changes: in a one-live value the dead side's key is `absent`, so the allowed values fall back to the live side's, which a live fixed result copied. The disagreement itself: W3E-ARB-S-7 |
| RT-4 | [API §9.6] item 2 for `merge --continue`: every `#N` the reply reports is the `#N` the node lands with | MEETS | The text states it for every merge-family reply, and the model renders the continue's reply so. No test fails if it did not: W3E-ARB-S-6 |
| RT-5 | [F05 §9.5] "Entry order" and [F13 §4] MC-6, re-checked independently | MEETS | Ascending `#N`, a leaving entry before a joining one; ME-005's and ME-011's entries by identity over both sections; `suite::gt18::a_branch_deletion_clears_in_identity_order_whichever_section_holds_them`, `suite::gt18::a_branch_deletion_clears_three_markers_in_identity_order_across_both_sections` and `suite::gt18::a_commit_flags_nonlinear_markers_in_identity_order_whichever_section_holds_them` ok |
| RT-6 | RS-007, VA-004, VA-005 and XB-004 on a two-sided key both of whose values have a deleted parent | MEETS | The dead-parent preference needs the other value's parent live or none, so the three-way value stays and VA-004 stages it; `merge::tests::a_two_sided_key_with_both_values_under_deleted_parents_stages_outside_strict` ok |
| RT-7 | The same rows on a two-sided key, not preferred, whose three-way value closes a cycle and whose other value is under a deleted parent | MEETS | Step (2) lands it at the other value with a `cycle` hint, and VA-004's first sentence stages its dangling parent; no sentence says otherwise |
| RT-8 | VA-005's "one with no repairable key" | MISSES | W3E-ARB-S-4 |
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
| KF-8 | MT RS-007, MT open point 15 and F12 §7.4 cite "the owner's ruling R-20 of 2026-10-12", which `owner-questions.md` does not hold, and OQ-A-13's 13.4 (a) there still words a resolution to base as producing a new origin | open: the owner's status line (W3E-ARB-S-9) |

### 2.7 Records only

- F12 §5.6's dropped set as an input of the virtual base a `Resolve` reads: R-MODEL WP-91, 21e250b; not checked here.
- F13 §6.2's `suspect` under the owner's ruling R-90 of 2026-10-19, `suite::n40`'s NG-005 pin, and DPM NN-007 and
  NG-005: R-MODEL's queue under that ruling; not checked here.
- `BodyDrop` and the store's dropped set: not checked here.
- F12 §6.5 with a merge M of one parent, and the alignment of F12 §7.9's `HierarchyCycle` key with F13 V05: left to the
  next S-lens pass.

## 3. Findings

### W3E-ARB-S-1 (Minor, PLAUSIBLE): PR-007, RS-010 and RS-007's scope do not except an automatic side

PR-007 says that an existence value a `conflicted-key` row gives, other than RS-008's cases and a new one-live
`DeleteVsModify`, "fixes none of the node's other keys, which merge key by key", and RS-010 says "otherwise they merge
key by key". Under `merge.policy.<kind>` = `ours` or `theirs` (AP-002, AP-003), MR-002's and MR-005's value rows on an
existence key take a side's value, and the model then fixes the node to that side whole, as RS-008's provisional state
does: RS-007 does not decide its hierarchy key, while RS-007's scope ("a node live in the result with no existence
policy") reads as if it did. Observable: no test exercises an existence key under an automatic side, and the acceptance's generators set
none (their policy rows are `delete-wins` and `resurrect`). PLAUSIBLE because AP-002's "the key takes o" can be read as
the automatic side, not the row, giving the value. Owner: R-MODEL WP-90 (name the automatic sides beside RS-008's
cases in PR-007 and RS-010, and in RS-007's scope).

### W3E-ARB-S-2 (Minor, PLAUSIBLE): an existence key that stages `IdCollision` fixes the node

MR-045 (`both-created`, `stage-take-o`, `IdCollision`) is the one existence row with a structural disposition. In the
model, the node then takes o's node whole, as an automatic side does, so RS-007 does not decide its hierarchy key and the
node's other keys do not merge key by key. RS-007's scope and PR-007 ("All other keys follow") do not say so.
Observable: no test exercises it, and the generators never reach it (a uid equal on two sides only by a fault).
PLAUSIBLE because `stage-take-o` can be read as taking o's node. Owner: R-MODEL WP-90.

### W3E-ARB-S-3 (Minor, PLAUSIBLE): RK-008 omits the refusal at the last `#N` for a preview or a dry run

RK-008 says that "a preview or a dry run allocates nothing and reports `next_id` + k − 1". At the bound, where
`next_id` + k − 1 exceeds 2^32 − 2, the model refuses the preview or dry run with `id_space_exhausted`, as the landing
would be refused; [F04 §5.7] states the refusal for "a write" only. Observable:
`suite::files::a_checked_or_dry_sync_at_the_last_node_id_is_refused` ok in the full gate at f71c6b1, which pins the
refusal. PLAUSIBLE because the bound can be read as implied. Owner: R-MODEL WP-92 (one clause in RK-008); [API §9.6]'s
same gap is R-SPEC-F's, already queued.

### W3E-ARB-S-4 (Minor, PLAUSIBLE): "a cycle with no repairable key" uses a term no text defines

VA-005 and CM-011 gloss the cycle RS-007 leaves as "one with no repairable key", and F12 §5.3 VM-7, §5.5 and open point
33, F13 I4, V05 and OP-13-17, and F19 code 64 repeat the phrase. No rule or spec text defines "repairable". RS-007's
step (1) lists what such a cycle may hold: "only fixed nodes, untouched keys, keys whose value and fallback have the same
parent, two-sided keys and keys already reset". A two-sided key whose two values differ in parent is in that list, yet
read plainly it is a key the repair could change. So the gloss is exact only when "repairable" means "a key step (1) may
reset". Observable: `merge::threeway::tests::f4_a_cycle_with_no_repairable_key_is_left_to_the_validators` ok (the cycle
is left as RS-007 states); no test shows a left cycle that holds a two-sided key. Owner: R-MODEL WP-90 (define the term
at RS-007's step (1), or replace the gloss by "a cycle step (1) cannot break"); the format chapters follow (R-SPEC-F
WP-12 and WP-18, R-SPEC-P WP-16).

### W3E-ARB-S-5 (Minor, CONFIRMED; a test gap): the dead-parent preference's "or none" is pinned by no test

RS-007, VA-004, XB-004 and open point 15 state that a two-sided key whose three-way value is under a deleted parent tries
the other side's value first when that value has a live parent **or none**. The texts are exact. The two tests of the
preference, `merge::tests::a_dead_parent_key_whose_other_value_closes_a_cycle_stages_outside_strict` and
`merge::tests::a_two_sided_key_with_both_values_under_deleted_parents_stages_outside_strict`, use an other value with a
live parent; no test was found whose other value is a root. Owner: R-MODEL WP-91 (a test of a key whose other value is a
root).

### W3E-ARB-S-6 (Minor, CONFIRMED; a test gap): no test fails if `merge --continue` reports placeholder `#N`s

[API §9.6] item 2 requires every `#N` a merge-family reply reports to be the `#N` the node lands with, and the model's
`merge --continue` reply does so (RT-4). No test pins it for the continue: in an earlier test round, a change to the
model that made the continue's reply report placeholder `#N`s passed the model's whole PR tier, while a test pins the
cherry-pick's reply. Owner: R-MODEL WP-91 (a test of a continue that lands a node new to the store).

### W3E-ARB-S-7 (Minor, PLAUSIBLE; the harness): the acceptance's fixed-node test can differ from the model's

RS-010's last sentences fix a node to RS-008's provisional state for every new one-live `DeleteVsModify` from RS-010
or RS-015, also one equal to a side's existence value (reachable when a side holds a `resurrect` value made over a
delete and the other side deleted the task alike from the same base), and the model does so. The acceptance's oracle
does not count that equal case as fixed: it judges such a node as one RS-007 decides and counts it in ckr_one_sided. No
verdict changes (RT-3), so no acceptance count can move; only the informative ckr_one_sided may exceed the nodes RS-007
decides. PLAUSIBLE: checked by reading, without a probe. Owner: R-MODEL WP-91.

### W3E-ARB-S-8 (Minor, CONFIRMED; the harness): own seeds cannot measure link_broken, not_forest or nondet

The acceptance's histories come from seeds 1 and 2 only, and the evaluation report, which takes other seeds, computes eight of the
acceptance's eleven counts: not link_broken, not_forest or nondet. AC-15 is therefore INCONCLUSIVE. Observable: the own
seeds run's output holds no row for those three. Owner: R-MODEL WP-91 (report the three counts in the evaluation
report, or give the acceptance a seed setting).

### W3E-ARB-S-9 (note): OQ-A-13's 13.4 (a) and R-20

`owner-questions.md` OQ-A-13 13.4 (a) still reads "a move, a revert, and a resolution to base or to a new value produce a
new one", while MR-040 and F12 §6.5 read origins by state (a resolution whose value equals a parent's passes that
parent's origin on, `base` included), and RS-007, open point 15 and F12 §7.4 cite for it "the owner's ruling R-20 of
2026-10-12", which `owner-questions.md` does not hold. The model follows the rules (DC-9). Owner: the owner's status
line for OQ-A-13.

## 4. Checks run

- The three runs of §1, each through the run kit on f71c6b1, with one expected test each, `ok`; each tier's count lines
  checked for presence, order, form, the tier's histories, the tier word and the 11 counts; the nightly's against a
  fresh nightly reference at the same commit, equal byte for byte; own seeds' first line, `histories` rows (600 per
  generator, 5,400 in all) and eight counts.
- Test outcomes: the full gate (`cargo xtask gate --branch m0/r-rev-s`) at f71c6b1, 2,811 test ids, 2,805 ok and 6
  ignored; each test this report names is `ok` there.
- Each text of §2.3 read against the model's merge, its validators, its origin walk, its virtual base, its continue
  and its markers at f71c6b1; each closing commit of §2.6 read with `git show --stat`.
- No probe, no mutation and no edit outside this file.
