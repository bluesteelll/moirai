# Wave 3d: verification of the OQ-A-12 prototype

| Field | Value |
|---|---|
| Status | `open`: the acceptance of OQ-A-12 is not met; a new owner question follows the comparison of alternatives |
| Scope | The reference model's prototype of OQ-A-12 (b) and (c), commit 7131790, against the decision's acceptance |
| Decision | `owner-questions.md` OQ-A-12; [m0/PLAN §5] "Owner decisions of 2026-10-06" |
| Method | Four independent verifiers (lockstep search at scale, an adversary of (b), an adversary of (c), a code review), each in its own copy of the tree, then a completeness critic; compiled here by the orchestrator, ids as the verifiers gave them |
| Date | 2026-10-07 |

## Verdict

The acceptance of OQ-A-12 has four conditions. One holds: E5 in both shapes and E6 land. The other three fail.

1. **"No history stages that the replay from B lands"** fails in three independent classes:
   - W3D-REV-1 (confirmed by the critic): a kept key still moves during the replay, through commits of A(o) \ A(B), and
     its transient value can undo a move that only one side made. Wave 3c's rule stages the same history, so this is a
     residual of the W3C-ARB-1 class that (b) as built does not remove.
   - S-1 shape C and ADV-B-7 (two independent histories, one mechanism): a resolved key (c) re-asserts a move that a
     resolution kept at the resolving commit's (hlc, commit id); it then beats, or blocks, a later move made by a
     branch that already held the kept move. RS-007 stages where wave 3c and the replay from B both land.
   - S-4: under `merge --base C` with C outside A(o) ∩ A(t), the replay start R lies below C and the commits of
     A(C) \ A(o) are replayed as src steps although b already holds them. Inherited from wave 3c's (B); the wave 3c
     arbiter ruled (B) for `--base` within the decision but did not test it (W3C-ARB-9).
2. **"The backstop of (b) is never needed, or its rule is stated"** fails: all four verifiers met the backstop in the
   search at the nightly tier (640 histories) and reduced it to one 6-operation history; RS-007 does not state its rule,
   and the rule as built has defects (below).
3. **(c) is exact** is not established: (c) compares a commit's state with its recomputed candidate, and a
   pre-existing model bug makes a cached state differ from the state rebuilt from changesets (ADV-C-2), so two stores can
   derive different step keys for one commit.

Beyond the relative acceptance, the critic's absolute three-way audit (CRIT-2) finds that about 12 % of all stagings,
under wave 3c's rule, under the prototype and under the replay from B alike, are avoidable: a forest exists that keeps
every one-sided value. The smallest is a 3-move `sync` (lane/x puts #2 under #1, `main` puts #1 under #2, lane/x puts #2
back at the root, `sync lane/x` stages `#1.parent`). GT6's I25′ property (M3) would meet them under every variant
tried. The replay of history, which OQ-A-6, OQ-A-11 and OQ-A-12 each patched, is the common cause.

## Findings

| id | severity | finding |
|---|---|---|
| S-1 | blocker | (c) re-asserts a resolution-kept older move at the resolving commit's (hlc, id): silent wrong values against I25′ (shapes A, B) and a staging where wave 3c and the replay from B land (shape C); 7- and 8-operation histories in the search's own alphabet |
| ADV-B-7, ADV-B-8 | blocker | the same mechanism found independently: a staging where both baselines land, and a landing that drops a lane's resolution |
| W3D-REV-1 | blocker | a kept key's transient value during the replay undoes a one-sided move; 8 operations; wave 3c stages it too |
| S-2, ADV-B-1, ADV-C-1, W3D-REV-2 | blocker | the backstop fires at the nightly tier; the committed acceptance test fails there |
| ADV-B-2 | major | the backstop resets a key both sides moved alike; only `--take base` then lands, with a value neither side holds |
| ADV-B-3 | major | backstop cascade: a reset closes a new cycle and a key one side never touched stages |
| ADV-B-4 | major | the backstop leaves a cycle when a node fixed by an existence policy lies on it |
| W3D-REV-3 | major | the backstop chooses by least uid, not by the replay's order; the staged key's `ours` resolution can re-stage |
| S-3 | major | the backstop loop runs under every rule and breaks cycles through existence-fixed nodes |
| ADV-B-6, CRIT-3 | major | after E6 lands, the next `sync` of the lane stages the kept key once (a sync whose src made no hierarchy step since B is not one-sided: the mirror of (A) is missing) |
| W3D-REV-5, ADV-C-6, CRIT-4 | major | (c) gives a lane's older move the sync's time against a third branch's later move; defensible only when the third branch's move is concurrent, and MR-040's text must change either way |
| ADV-C-2, CRIT-5 | major | pre-existing: a carried `DeleteVsModify` makes a cached state differ from its changesets, so resolved keys depend on the cache |
| ADV-C-3 | major | a revert of a commit whose move a later sync kept by resolution lands an empty commit, since `moved_after` reads resolved keys |
| ADV-C-4 | major | (c) doubles a sync's cost on a long lane; recomputing all derived keys from scratch is super-quadratic (13.8 s against 0.12 s at 999 commits) |
| ADV-C-5 | major | pre-existing: every merge that re-keys a file node panics in debug builds (an overflow on a provisional #N) |
| CRIT-6 | major | under (c) a landed commit's step keys follow whatever merge rule is in force, so every later rule fix re-keys landed history; (d) would not |
| S-4 | major | `--base` outside A(o) ∩ A(t) (inherited from wave 3c) |
| W3D-REV-4 | minor | the backstop resets a node whose parent equals b's (order only) |
| ADV-B-5 | minor | the backstop resets a key to a b value whose parent is deleted in the result |
| ADV-C-7 | minor | `--policy` and `--base` results also count as resolved keys |
| W3D-REV-6, W3D-REV-7, S-7, CRIT-7 | minor | harness blind spots: a staged step-0 sync's violations, Refused against Landed, landed values not asserted, no order-only moves, no deletes, histories end at the first divergence, 64 PR-tier cases lack power, the backstop count is not per variant |
| W3D-REV-8, W3D-REV-9 | minor | the doc comment of `Engine::kleppmann` does not state (b); test-switch hygiene |
| ADV-C-9 | note | pre-existing: `suite::gt18::the_marker_cache_equals_the_definition` is flaky (random seed; marker entry order) |
| ADV-C-8, W3D-REV-10, W3D-REV-11 | note | confirmed: on the histories checked (c) is independent of #N numbering and replicates `plan_merge` for the default arguments; the step-key memo's borrows are sound |
| S-6 | note | (b) also fixes a silent wave 3c wrong value (a step-0 sync landing a key that b, o and t agree on at a value none holds) |

## Numbers

- Committed generator (main and two lanes; moves, priority edits, syncs, cross merges, merges into `main`), 1,500
  histories per seed: the prototype against the replay from B, worse 0 at seeds 1, 4, 5 and 6, backstop 1 at seed 1;
  against wave 3c, worse 0 at seeds 2, 4, 5 and 6. Seed 23 at 3,000 histories: worse 1 against the replay from B
  (W3D-REV-1).
- Widened generator (three lanes, deletes, criss-cross, cherry-picks, reverts, up to 40 operations), 1,500 per seed,
  without `--base`: worse 0, 0 and 1 against the replay from B; with `--base`: 13, 9 and 19, nearly all from S-4.
- Absolute audit, 1,500 histories per seed: avoidable stagings 78 of 347 and 103 of 370 under the prototype, about the
  same under wave 3c's rule, slightly more under the replay from B.
- Cost on a 999-commit lane (debug build): the last syncs take about 250 ms under the prototype against 124 ms under
  wave 3c's rule.

## What follows

The three pre-existing model bugs (ADV-C-2, ADV-C-5, ADV-C-9) are fixed on their own. The alternatives to RS-007's
replay, among them a per-key three-way rule for hierarchy keys with a cycle repair in place of the replay, are
compared on one evaluation harness that closes the blind spots above, and the owner is asked a new question with that
evidence before the merge table's V3 signature. Until then RS-007's text stands as wave 3c left it, and the model's
prototype of (b) and (c) is not accepted.

## The pre-existing model bugs, fixed, and the specification findings they raised

Fixed in the model, each with regression tests (commits "WP-91: Accept a provisional #N ...", "WP-90: Scan the marker
rows in identity order ...", "WP-91: Copy a carried existence conflict's node ..."); the whole model suite passes
(630 passed). The fixes raised these points for the specification roles; each needs a rule or a text before the
format freeze:

| id | where | finding | proposed |
|---|---|---|---|
| W3D-SF-1 | [F12 §6.3]; [RULES/merge-table] §2, MR-001, MR-003, MR-004, PR-007 | No text says which side a node's other keys come from when a merge takes a side's conflict value unchanged; its `prov` names a side of the merge that made it. The model now copies the node from the side whose value is taken (ADV-C-2's fix) | "A conflict value taken from a side keeps its `prov`, which names a side of the merge that made it, never of this merge; the uid's node is that side's, which holds it in the value's provisional state" |
| W3D-SF-2 | PR-007; I25′; [AR §3.4] I39′ | PR-007 says that only an existence row of `policy` fixes a node's other keys, but the model copies the whole node for every conflict-valued existence result, so a change only src made to a node whose existence both sides hold in conflict is lost (a `sync` lands an empty changeset and drops `main`'s retitle) | owner or spec decision: (a) a conflict-valued existence result fixes the node to the holding side, as the model does; (b) the other keys merge key by key, with the value attached, when its provisional state is live, and as a tombstone merge (I39′) when it is deleted |
| W3D-SF-3 | [F12 §6.5] "A live existence side"; RS-008 | The restore of a `resolve --take` towards a live side reads the parents of the commit whose `Conflict` op set the value; a value carried by MR-004 gets a new `Conflict` op in the carrying merge, so the restore reads the wrong parent (the same resolution moves #1 differently on two lanes) | M is the merge that made the value, or the `Conflict` op records its originating merge |
| W3D-SF-4 | [F05 §9.5], §10.1; ME-012; [F13 §4.2] MC-6; [LQ/std §2.15] "Ties" | The order of a `Marker` record's entries is observable (the change feed) and must not depend on folds, but no text states it | "written in this order, which no fold changes: for a commit landing, a ref move or a fork, by `#N` ascending, a node's leaving entry (ME-004) before its joining entry; for a ref deletion (ME-005), ascending by identity over both sections; then the `nonlinear` entries of ME-011, ascending by identity over both sections" (the model's order); or one total order by (identity, `mkind`) |
| W3D-SF-5 | [API §9.6] item 2; RK-008 | A merge reply reports conflict and violation keys, and validator texts, with the provisional #Ns of uids new to the store, before the landing renumbers them (`#4294967295.observation` where the node lands as #7) | give the landing #Ns before the validators run; state which #N a preview or dry merge reports for a uid not yet allocated |
| W3D-SF-6 | [F08 §2.1]; [F04 §5.7] | Off by one at the top of the id space: whether the last allocatable #N is 2^32 − 1 or 2^32 − 2; the model's landing renumber has no id-space check | state the last #N and the refusal |
| W3D-SF-7 | RK-005; [API §15.4]; [F14] | RK-005's "created is S's creating commit" conflicts with [API §15.4] `created` (the merge commit for a uid′) and with [F14]'s least (generation, id) | pick one notion |
