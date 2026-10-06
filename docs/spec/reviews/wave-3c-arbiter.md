# Wave 3c: spec arbiter's check of the replay start (OQ-A-11 11.1 (B))

| Field | Value |
|---|---|
| Status | `open`: (B) is exact, so the fallback is not needed; W3C-ARB-1 to -3 go to the owner before V3 |
| Scope | RS-007's replay start R (11.1 (B)) and one-sided merge (11.1 (A)); open point 35 (iv), (v); consistency |
| Rules | `rules/merge-table.md` RS-007, CS-013, MR-039, MR-040, PR-016, VB-011, open points 15, 19 and 35 |
| Format | `format/12-vcs.md` §5.3 VM-7, §5.5, §7.1, the §7.4 row "Kleppmann steps", open point 32 |
| Model | `crates/moirai-model/src/` `vcs.rs`, `history.rs` (`plan_merge`), `merge.rs` (`Start`, `kleppmann`) |
| Commits | 1c5b464 (model) and a4ae6b5 (spec sync 3c), committed while this check ran; the check reads them |
| Decision | `reviews/owner-questions.md` OQ-A-11; [m0/PLAN §5] "Owner decisions of 2026-10-06" |
| Date | 2026-10-06 |
| Role | R-REV spec arbiter, wave 3c (rule S5: a separate session; it wrote none of the checked text) |

## Ruling

**(B) is stated exactly, so the fallback "(A) alone" is not needed, and applying (B) to `--base` and to the virtual base
(open point 35 (iv)) is within the decision.** R, the greatest commit of A(B) ∩ A(o) ∩ A(t) that every other commit of U
= A(o) ∪ A(t) descends from or is an ancestor of (ε when none is), exists and is unique for every pair of sides and
every base (single LCA, `--base`, virtual base, ε, and so for each inner virtual merge): a qualifying commit lies in U
and is comparable with every commit of U, so any two are comparable and they form a chain, whose greatest element is
unique; R reads only the DAG and A(B), which [F12 §5.2] defines for every base. Every commit of A(o) \ A(R) and of A(t)
\ A(R) is comparable with R and not its ancestor, so it descends from R; since A(R) ⊆ A(B), that includes every commit
of A(o) \ A(B) and A(t) \ A(B). For a single LCA L, R = L exactly when every commit of both sides since L descends from
L, since every candidate lies in A(L). The statement is faithful: the decided text, with "descends from" read as proper
and "since it" as U \ A(R), already puts R in A(B) ∩ A(o) ∩ A(t), so the restriction to A(B) is the decision's and not
an addition (it matters for `--base`, where a later common ancestor, such as the LCA, would otherwise start the replay
above the chosen base); its purpose clause ("so no commit is replayed on a state it was not made on") needs every
replayed commit to descend from R, which is the comparability condition, and which the first clause alone does not give
(W3C-ARB-8 has a base where it allows a commit that would replay a base-side commit on a state it was not made on). So
the criss-cross choice "as I31′ chooses a base" that the recommendation foresaw never arises: over a criss-cross the
LCAs are pairwise incomparable, so R lies below all of them, and a virtual state would not be "a commit". (iv) is within
the decision, not an extension: (B) is stated over A(B), not over a single LCA; the recommendation the owner took names
the criss-cross; `--base` naming tip(dst) is the decided (A) ("tip(dst) is B"); and VM-7 already gave each virtual merge
RS-007's merge rule with its inner base as B. (A)'s claims hold: a merge whose base is tip(dst) has b = o on every key,
so taking t is what I25′ and the three-way rules give; after step 0 LCA(src, main) = tip(main), so a merge of a branch
into `main` is one-sided, except under a `--base` that names another commit (W3C-ARB-5); a virtual merge's dst is no
commit, so it is never one-sided. The model computes R by its definition, E1 to E4 land as open point 35 (v) says, (B)
alone lands E1 to E3 and stages E4, (A) alone lands E1, E2 and E4 and stages E3, E5 stages as described, and the (0, 0)
step is taken against state(R) in RS-007, [F12 §7.4], VM-7, open point 15 and `kleppmann` alike (Checks run). Exactness
is the fallback's only condition, but three findings bear on whether the owner keeps (A) with (B) and on the V3
signature, and the orchestrator should put them to the owner before it: (B) is not monotone against the replay from B,
and on a reachable cross-lane history it stages a key that b, o and t all hold, which the replay from B, and so the
fallback, lands clean (W3C-ARB-1); [F12 §7.2] says such a key keeps its value, so RS-007 and §7.2 now give two results
(W3C-ARB-3); and E5 is wider than open point 35 (v) says, since it also covers "sync, resolve to `ours`, sync" when the
lane's own move is the later one, and then every later sync of that lane restages the key (W3C-ARB-2). The other
findings are wording, records and tests.

## Findings

| id | severity | where | finding | proposed fix |
|---|---|---|---|---|
| W3C-ARB-1 | major | RS-007; OP-35 (v) | (B) stages a key b, o, t agree on; replay from B lands | record E6; owner |
| W3C-ARB-2 | major | OP-35 (v) E5; F12 §7.4 | E5 also covers "sync, resolve, sync", on every sync | widen E5; owner |
| W3C-ARB-3 | major | F12 §7.2; RS-007 | "equal in all three keeps its value" against RS-007 | say which rule wins |
| W3C-ARB-4 | minor | RS-007, F12 §7.4 "Replay start" | "otherwise R precedes ..." is false as worded | restate |
| W3C-ARB-5 | minor | RS-007; F12 §7.4; OP-35 | "every merge into `main` is one-sided" fails on `--base` | exception |
| W3C-ARB-6 | minor | RS-007; F12 §7.1; PR-016 | "base is state(tip(dst))" reads as equal states | say identity |
| W3C-ARB-7 | minor | RS-007; F06 §4.4.4 | ancestors-first needs hlc order; native hlc unchecked | cite, enforce |
| W3C-ARB-8 | minor | OP-35 (v) record | how R meets "chosen over a criss-cross" is unrecorded | add reasoning |
| W3C-ARB-9 | note | `vcs.rs` tests | the definition property draws A(B) from LCAs only | add `--base`, ε |
| W3C-ARB-10 | note | F12 §5.5a; OP-35 (v) "Cost" | the replay since R is not named in the work budget | name it |

OP-35 is [RULES/merge-table] open point 35. Every history below is synthetic: tasks created on `main` in one commit F,
all at the root, lanes forked at F, and the commits made in the order listed, so their (hlc, id) order is that order.

### W3C-ARB-1 (major): (B) stages a key that b, o and t all hold, where the replay from B lands

R precedes B whenever a side holds a commit that does not descend from B. The commits of A(B) \ A(R), which the replay
from B took as settled in b, then become steps of both sides, interleaved by (hlc, id) with commits of other branches
they were never combined with. A base commit's move can then close a cycle and be undone, and no later step re-asserts
it in a state where it applies. History, tasks #1 to #4, lanes lane/x and lane/y: lane/y puts #2 under #3 (Y1); lane/x
puts #1 under #3 (X1); `main` puts #4 under #2 (M1); lane/x puts #3 under #4 (X2); `main` puts #2 under #1 (M2);
`merge lane/x --into lane/y` lands (MY); `sync lane/x` stages `#2.parent`, which is resolved to `ours` (N). Then
`merge lane/y --into lane/x`: the LCA is X2, and b, o and t all hold #3 under #4. Y1 does not descend from X2, so R = F.
The replay: Y1, X1 and M1 apply; X2's #3 under #4 closes #3 → #4 → #2 → #3 and is undone; M2 applies; MY's #3 under #4
closes #3 → #4 → #2 → #1 → #3 and is undone; N puts #2 at the root. #3 ends `kleppmann-skipped` at the root, a value
none of b, o and t holds, and the merge stages `#3.parent HierarchyCycle`. Replayed from B (the rule before wave 3c,
and the fallback (A) alone, since this merge is two-sided) it lands clean with lane/x's hierarchy. That is against I25′
(#3 is untouched on both sides since the LCA) and [F12 §7.2] (W3C-ARB-3). Open point 35 (v) says that (B) changes no
merge whose sides start at its base, which is true, but no text says that (B) can stage where the replay from B lands.
A lockstep random search (Checks run) found three such histories in 1,950, against nine where (B) lands what the replay
from B stages; none in 3,000 histories whose stagings were aborted rather than resolved, so the class appears to need
a resolution earlier in the history (here N's, on another key). Of the options OQ-A-11 listed, (C) covers it (N's step
would re-set #3 under #4 after MY, when it applies; by hand) and (D) does not (no resolution set #3).
**Fix.** Record the case in open point 35 (v) as E6, with a model test that pins it as E5's does; state in RS-007's
guarantees that outside the one-sided merge the replay from R can stage where a replay from B lands; and report it to
the owner before the V3 signature, as it bears on keeping (A) with (B) or taking the fallback (A) alone. W3C-ARB-3's
option (b) would also cover it.

### W3C-ARB-2 (major): E5 also covers "sync, resolve to `ours`, sync", and then repeats on every sync

History, tasks #1 to #3, lane lane/x: `main` puts #2 under #1 (X); lane/x puts #1 under #2 (Y), then #2 under #3 (Y2);
`sync lane/x` replays X, Y (it closes #1 → #2 → #1 and is undone) and Y2, and stages `#1.parent`; resolved to `ours`
(#1 under #2, #2 under #3), the sync N lands, and its step re-asserts only #2, the key `main` moved. `main` then makes
a commit with no hierarchy entry and lane/x syncs again: R is the fork, since Y does not descend from X; the replay
undoes Y again; nothing re-asserts #1; and the sync stages `#1.parent HierarchyCycle`, although `main` never moved #1.
Resolved to `ours` each time, every later sync of lane/x stages it again (checked over four syncs: the LCA moves on, R
stays at the fork), until the lane merges into an unmoved `main`, which (A) makes one-sided. The rule before wave 3c and
(A) alone give the same stagings, so the case is not new; but open point 35 (v) presents E5 only as "E4's history on a
two-sided merge" on the path "merge another lane, resolve to `ours`, sync", and says (B) covers E3, the path "sync,
resolve, keep working, sync". That holds only in E3's order, where `main`'s move is the later one; in the other order
the same daily path is E5's. [F12 §7.4]'s closing description ("a merge's resolution kept a side's cycle-closing move")
does cover it. (C) and (D) cover it (under (D) N's step re-asserts the key its resolution set).
**Fix.** Add this history and its repetition to E5 in open point 35 (v), pin it with a model test, and say so in the
owner question on E5 (S3C-M-5, raised in chat), since it changes how often E5 is met.

### W3C-ARB-3 (major): [F12 §7.2] and RS-007 give two results for a key equal in b, o and t

[F12 §7.2]: "The keys a merge decides are every key whose value is not the same in B, O and T [...]. A key equal in all
three keeps its value." RS-007: "Computed once per merge for all hierarchy keys [...] Each key's value is its node's
final (parent, order)", and the model replays every merged node. In W3C-ARB-1's history an engine that follows §7.2
lands the merge clean and one that follows RS-007 stages it, so I28′ and I30′ (two stores emit the same merge commits)
need one reading. The conflict was latent under the replay from B (a key moved away and back on a side could be
undone); (B) makes it reachable, since the commits of A(B) \ A(R) now move keys that b, o and t agree on.
**Fix, before the V3 signature.** Either (a) RS-007 decides every hierarchy key and §7.2 names the exception, which
also means I25′ does not hold for hierarchy keys outside the one-sided merge and needs the owner, since I25′ is an
invariant; or (b) a hierarchy key equal in b, o and t keeps its value and is never `kleppmann-skipped`, the replay
deciding the others, with the validators' cycle check (I37′) as the backstop; (b) changes RS-007's results (W3C-ARB-1
then lands), so it needs the owner or an arbiter ruling.

### W3C-ARB-4 (minor): "otherwise R precedes the base and every commit [...] that does not descend from it"

RS-007 and [F12 §7.4] say: R is the base's commit when every commit of both sides since a single-LCA base descends from
it; "otherwise R precedes the base and every commit of A(o) ∪ A(t) that does not descend from it". Read literally, R
itself and its proper ancestors lie in A(o) ∪ A(t) and do not descend from the base, and R precedes none of them; "it"
can be the base or R; and a virtual base or ε is no commit to precede. The first clause also holds for `--base C` when C
is a commit of A(o) ∩ A(t) that every commit of both sides since C descends from.
**Fix.** "R is the base's commit when the base is a commit of A(o) ∩ A(t) that every commit of both sides since it
descends from (a single LCA, or `--base` naming such a commit); otherwise R is a proper ancestor of that commit, or of
every LCA, or ε. In every case R is an ancestor of every commit of A(o) \ A(B) and of A(t) \ A(B)."

### W3C-ARB-5 (minor): "every merge of a branch into `main` is one-sided" fails under `--base`

RS-007 ("Every merge of a branch into `main` is one-sided, since its step 0 leaves LCA(src, main) = tip(main)"), [F12
§7.4] and open point 35 (v) state it without exception. Step 0 never takes the merge's `--base` ([F06 §4.4.16]; spec
sync 2b S2B-F-1), so `merge --base C --into main` with C other than tip(main) merges over state(C), is not one-sided and
replays from R. The model follows the condition, not the sentence (`plan_merge`: one-sided under `--base` only when C
is tip(dst)).
**Fix.** "[...] is one-sided after its step 0, unless `--base` names a commit other than tip(main)", in all three.

### W3C-ARB-6 (minor): "the base is state(tip(dst))" can be read as equality of states

RS-007 ("When the base of a merge or a `sync` is state(tip(dst)) (the single LCA is tip(dst), `--base` names tip(dst),
or tip(dst) and the base are both ε)"), [F12 §7.1] ("whether B is state(tip dst)"), [F12 §7.4] and PR-016 can be read
as B ≈ O, with the parenthetical as examples. That reading also holds when dst's commits since the LCA have a null net
effect, or when `--base` names another commit with the same state, and it gives a different result: such a merge would
take t with no replay, while under commit identity it replays and can stage. The decision says "when tip(dst) is B",
and the model tests identity (`plan_merge`).
**Fix.** "When the base's commit is tip(dst), exactly when the single LCA is tip(dst), `--base` names tip(dst), or
tip(dst) and the base are both ε (equal states of different commits do not count)"; in [F12 §7.1] and PR-016,
"whether the base's commit is tip(dst)".

### W3C-ARB-7 (minor): "ancestors since R are steps too" needs the step order to put ancestors first

RS-007 and [F12 §7.4]: every replayed commit descends from R "and its own ancestors since R are steps too, so no
commit is replayed on a state that lacks the history it was made on". That follows only if the (hlc, commit id) order
puts every commit after its ancestors. [F06 §4.4.4] gives this for local commits (an `hlc` never below any commit the
store holds) and for foreign and import-checkpoint commits (at least each parent's `hlc` + 1), but a native import
keeps its `Moirai-Hlc` trailer, and this check found no rule that compares it with the parents' `hlc`. The order was
already RS-007's before wave 3c; the sentence now states the property.
**Fix.** Cite [F06 §4.4.4] for the order, and either make a native commit whose `hlc` does not exceed each parent's
an `ImageParse` violation ([F14]'s import checks) or qualify the sentence.

### W3C-ARB-8 (minor): the record does not say how R meets "chosen over a criss-cross as I31′ chooses a base"

Open point 35 (v)'s "Decided" paragraph and its decision record say (B) "is stated exactly in RS-007". They do not say
how the statement meets the decided words "(the commit the replay starts from, chosen over a criss-cross as I31′
chooses a base)", or why R must be comparable with every commit of A(o) ∪ A(t) rather than only be a commit that
every commit since B descends from. Example: x and y off F; `main`'s C merges x and y; the lane forks at x (Lx) and
syncs at C (N); `main` makes C2 off C and the lane Z off N. At the next sync the LCA is C, and the commits that every
commit of both sides since C descends from are F and x; starting at x would replay y, a commit of `main`'s, on
state(x), which y was not made on, while R = F. Over a criss-cross (L1 and L2 off C2, merged into each other both ways)
that condition leaves L1 and L2 to choose between, and R = C2, below both, needs no choice.
**Fix.** Add the reasoning of this ruling and the two examples to open point 35 (v)'s "Decided" paragraph, so that the
owner's record shows the parenthetical was resolved and not dropped.

### W3C-ARB-9 (note): the property for R draws A(B) from the LCAs only

`vcs::tests::the_replay_start_meets_its_definition_on_random_dags` takes A(B) as the union of the LCAs' ancestor sets,
so single and virtual bases only; the `--base` bases (any commit: below the LCA, on one side only, a descendant of a
tip, on another root) and the empty base rest on one unit case and the implementation's generality.
**Fix.** Draw A(B) also as anc*(C) for a random commit C, and as ∅.

### W3C-ARB-10 (note): the replay since R is not named in the merge's work budget

Open point 35 (v)'s "Cost" sentence says the replay of a lane that has synced reaches back to its fork when `main`
holds none of its commits. A long-lived lane that syncs daily then replays its whole history since the fork, and
`main`'s commits since then, on every sync; [AR §5a.7] step 3's daily-path bound (the LCA within 4k ops of a pin) no
longer bounds the replay. [F12 §5.5a] charges the virtual base to the merge but does not name the replay.
**Fix.** Name the replay since R (with the step keys it reads) as part of the merge's work under [F17 §4.4] W1 and W2
in [F12 §5.5a] or RS-007, and measure it on a long-lived lane when the engine implements RS-007.

## Checks run

All runs used `CARGO_TARGET_DIR` = the session scratchpad's `target`, except where noted.

1. `cargo test -p moirai-model --locked --lib -- suite::vcs` in the repository: 55 passed, 0 failed.
2. `cargo test -p moirai-model --locked --lib -- vcs::tests merge::tests` in the repository: 24 passed, 0 failed.
3. `cargo test -p moirai-model --locked --lib` in the repository: 614 passed, 0 failed.
4. `cargo test -p moirai-model --locked` in the repository, after the rebuild of item 7: 614 passed, 0 failed; no doc
   test.
5. A scratch copy of the workspace in the scratchpad (not part of the repository), with a thread-local switch in
   `plan_merge` and `Bases::vbase` between four rules: wave 3c; before wave 3c (replay from B, no one-sided merge,
   the parent-only undo kept); (B) alone; (A) alone. Results:
   - E1 to E5 (`suite::vcs::a_clean_sync_first_merge_into_main_takes_the_lanes_hierarchy`,
     `a_sync_resolved_to_ours_then_merged_into_an_unmoved_main_lands_the_lane`,
     `a_second_sync_after_a_sync_resolved_to_ours_lands_clean`,
     `a_merge_resolved_to_ours_then_merged_into_an_unmoved_main_lands_the_lane`,
     `a_sync_after_a_merge_resolved_to_ours_stages_the_kept_move`) and the two properties
     (`a_lane_merged_into_an_unmoved_main_takes_its_hierarchy`,
     `a_lane_that_kept_its_own_moves_against_a_merged_lane_lands_on_an_unmoved_main`): wave 3c 7 of 7 pass; before
     wave 3c 2 pass (E5's pin and the first property, in its 48 cases), E1 to E4 and the second property fail; (B)
     alone 4 pass (E1 to E3, E5), E4 and both properties fail; (A) alone 6 pass, E3 fails.
   - W3C-ARB-2's history: the second sync stages `#1.parent HierarchyCycle` under all four rules, with R = the fork
     under wave 3c; repeated, it stages on each of four syncs.
   - Lockstep search, wave 3c against (A) alone: random histories over `main`, lane/x and lane/y (moves of four
     nodes, priority edits, syncs, cross-lane merges, merges into `main`; stagings resolved to `ours` or `theirs`,
     aborted if they stage again). 1,950 histories in three runs: 3 where wave 3c stages and (A) alone lands, 9 the
     reverse, 8 where both stage on different keys, none where both land with different hierarchies; proptest shrank
     one of the first kind to W3C-ARB-1's history, and a deterministic run of that history confirmed it (wave 3c
     stages `#3.parent`, R = F, LCA = X2; (A) alone lands).
     With every staging aborted instead (3,000 histories): 0 of the first kind, 4 of the second, 1 of the third.
   - Stagings of a `HierarchyCycle` on a key equal in b, o and t (single-LCA merges and syncs, 1,500 random histories
     per rule, different histories per rule): wave 3c in one history (two stagings); the other three rules in none.
     The counts show that the class exists, not its rate.
   - Non-empty (0, 0) steps over 600 random histories, wave 3c and before wave 3c: none, as open point 35 (iv) says.
   - `Dag::replay_start` on W3C-ARB-8's two DAGs: R = F with LCA C, where F and x satisfy the first clause alone; R = C2
     over the criss-cross, where the first clause alone leaves the two LCAs.
   - `--base` probes: `--base` naming tip(dst) on two diverged lanes is one-sided and lands src's hierarchy and src's
     other values (b = o on every key); `--base` naming the fork on E3's history lands, with R = the fork.
6. Reading: `owner-questions.md` OQ-A-11; [m0/PLAN §5]; `git show` of 1c5b464 and a4ae6b5; [F12] §3.4 to §5.8, §7.1 to
   §7.4 and open point 32; [RULES/merge-table] RS-007, CS-013, MR-039, MR-040, PR-001, PR-016, VB-011, VB-017, open
   points 15, 19 and 35; [F06 §4.4.4]; [AR §3.4] I25′; `spec-sync-3c.md`.
7. The scratch copy first shared the target directory with the repository, and cargo then reused its test binary for one
   repository run (623 tests, the scratch tests among them; items 1 to 3 ran before it and are unaffected). `cargo clean
   -p moirai-model` and a rebuild from the repository sources restored the directory (item 4; no scratch test is in the
   binary). Later scratch runs used a separate target directory, `target-arb` in the scratchpad.
