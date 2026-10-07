# Wave 3d: alternatives to RS-007's replay for hierarchy keys

| Field | Value |
|---|---|
| Status | `open`: the owner is asked OQ-A-13 (`owner-questions.md`) |
| Scope | RS-007's rule for the hierarchy key (parent, order) of every merged node, after the wave 3d verification found OQ-A-12's acceptance unmet (`wave-3d-verify.md`) |
| Method | One evaluation harness built first, with an oracle that no rule computes; three alternatives built on it, each in its own copy of the tree; two judges with distinct lenses (semantics, engineering), each re-running what it scored; then a completeness critic with its own widened runs. Compiled here by the orchestrator |
| Code | The harness and the recommended candidate K2 are in the reference model as the test-only rule `merge::Rule::Cand` (commit "WP-91: Add the RS-007 evaluation harness and the three-way candidate ..."); the model's default rule is unchanged. K3 and K4 are described below and not kept in the tree |
| Date | 2026-10-07 |

## Verdict

The replay of history is the common cause of the faults that OQ-A-6, OQ-A-11 and OQ-A-12 each patched. Every replay
variant measured leaves two classes of fault on the same order of magnitude:

- avoidable `HierarchyCycle` stagings, where a forest exists that keeps every value only one side changed: 21 % to 27 %
  of all such stagings;
- silent wrong landings, where a key only one side changed lands at another value: about 1,000 to 1,300 in 12,000
  histories.

A state-based three-way rule per hierarchy key with an exact cycle repair and no replay (K2, `threeway`) has 1 fault in
12,000 histories, a real cycle through a node the oracle does not judge. It has 0 wrong landings and 0 stuck stagings.
Its own work on a `sync` is flat (about 80 µs), and it needs no format addition. Both judges rank it first. The critic
confirms the result on a wider generator and finds that OQ-A-12's acceptance cannot be met as written: E6 has no
forest, and the relative criterion ("no history stages that the replay from B lands") counts the replay from B's own
wrong landings as the standard. The owner decides the rule, four semantic readings it rests on, and the acceptance
(OQ-A-13).

## The harness

`suite::rs007eval` (test builds). It runs each generated history on its own store under every rule asked for and judges
every merge-family command with a three-way oracle on states:

- **Judged keys.** The hierarchy key of each task live in all of b, o and t. b, o and t are the merge's own, as the
  store computes them. That includes the virtual base under the rule's own recursion, `--base C`, both judged merges of
  a `merge X --into main` (step 0 and the merge), and the DM rows of a revert and a cherry-pick.
- **Touched** (primary reading): a key is touched on side S when S's value differs from b's. A secondary reading also
  counts a key in the net changeset of a commit of the side since the base. It is reported so the numbers can be set
  against the wave 3d critic's audit (CRIT-2).
- **Classes.** A key untouched, touched on one side only, or touched alike on both sides is *determined*: it must land at
  that value. A key both sides changed to different values is *two-sided*: it may land at either.
- **Forest.** A forest exists when some choice for the two-sided keys, with every determined key at its value, leaves no
  cycle among the judged tasks. It is decided by brute force.
- **Verdicts.** A landing is WRONG when a determined key is off its value or a two-sided key is at neither side's value.
  A `HierarchyCycle` staging is AVOIDABLE when a forest exists, GENUINE otherwise. It is *stuck* when no choice of ours
  or theirs per staged key gives a forest. A fault is an AVOIDABLE staging or a WRONG landing.

The generators are narrow (`main` and two lanes), lanes, wide (three lanes, deletes, criss-cross merges, cherry-picks,
reverts) and based (`--base`). The search ran 2 seeds × 1,500 histories each, 12,000 in all. The harness also has a
41-case corpus with every history the wave 3c and wave 3d reviews pinned, each with its oracle verdict, and the long-lane
cost scenario of ADV-C-4. Its three PR-tier tests check the oracle's own cases. The report, trace and search tools are
ignored tests driven by `MOIRAI_RS007_EVAL_*` variables. `MOIRAI_RS007_RULE` sets the rule of every test thread
(`current`, `wave3c`, `fromb`, `cand`).

## The candidates

- **K2 `threeway`** (no replay):
  - *Three-way.* o = t gives that value; o = b gives t; t = b gives o.
  - *Two-sided keys.* A key both sides changed to different values takes the value with the later *origin*. The origin is
    the (hlc, commit id) of the commit that produced the value. A walk along first parents passes over commits that leave
    the key unchanged, and over a two-parent commit that holds its second parent's value. So a `sync`, a merge into
    `main` and a resolution to ours or theirs pass the origin on, and none of them re-times a value.
  - *Cycle repair.* While no choice of the two-sided keys gives a forest, one determined key on the unavoidable cycle is
    reset to b and staged (MR-039). Then each two-sided key keeps its later value unless no forest remains. In that case
    it lands at the other side's value, skipped and logged, not staged.
- **K3 `replayfix`** (the replay kept): RS-007's replay from the replay start R, with seven repairs in the replay's own
  terms and one non-replay forest step after them:
  - os2: a one-sided merge on states, in both directions;
  - trans: a side's own moves of a key it leaves at b's value are dropped;
  - dedup: a step that repeats its side's value is no move;
  - redo: undone moves are retried;
  - admit: a two-sided key left at either side's value lands;
  - pick: a pick or revert leaves a key out only where dst's value differs from b's;
  - baseb: a `--base` outside A(o) ∩ A(t) replays from B.

  The replay layer alone (K3a) still stages 116 avoidable cycles in 12,000 histories. That class (R1) cannot be
  removed by any time-ordered replay. Only the forest step removes it, and it overrides MR-040 for 127 keys.
- **K4 `fromb`** (the replay from B): the replay from state(B) with one-sided shortcuts on states and a net-change filter
  (a side's moves of a key it holds at b's value are no moves). Three bounded repairs follow: retry an undone move; let a
  stuck one-sided key flip up to three two-sided keys on its cycle; land a stuck two-sided key at the first side's value
  that closes no cycle. It reverses OQ-A-11 11.1 (B).

## Numbers

12,000 histories (narrow, lanes, wide, based × seeds 1 and 2 × 1,500), all six rules on every history:

| | K2 | K3 | K4 | Current | Wave3c | FromB |
|---|---|---|---|---|---|---|
| Oracle faults | 1 | 2 | 2 | 1,844 | 1,846 | 1,785 |
| Avoidable `HierarchyCycle` stagings | 1 | 2 | 2 | 577 | 581 | 800 |
| Avoidable share of `HierarchyCycle` stagings | 0.0 % | 0.1 % | 0.1 % | 20.9 % | 21.0 % | 27.0 % |
| Wrong landings (all one-sided keys) | 0 | 0 | 0 | 1,267 | 1,265 | 985 |
| Stuck stagings | 0 | 0 | 43 | 42 | 42 | 41 |
| Stagings | 4,370 | 4,362 | 4,507 | 4,924 | 4,929 | 5,117 |
| One-sided merges staging or landing off t | 0 | 0 | 0 | 0 | 0 | 170 |
| Faults under the secondary (changeset) reading | 271 | 273 | 269 | 1,080 | 1,082 | 973 |
| Later two-sided move dropped where it fits (MR-040, K2's origin) | 0 | 73 | 364 | 322 | 321 | 324 |
| Sync-transparency probes passed | 4 of 4 | 3 of 4 | 0 of 4 | 0 of 4 | 0 of 4 | 0 of 4 |
| Committed relative lockstep at 1,500 cases | fails | fails | fails | | | |

- "Current" is the OQ-A-12 prototype (b)+(c) the model runs by default; "Wave3c" is the rule RS-007's text states;
  "FromB" is the replay from B (the rule before wave 3c).
- Two mechanisms explain most of the replays' wrong landings, and every replay variant keeps both:
  - *RETIME-MAIN*: a merge into `main` carries the lane's moves in its first-parent changeset, so they replay at the
    merge's time and beat a later move another lane made.
  - *REVERT-AFTER-MERGE*: the merge that brought commit C in counts as a later move of C's key, so `revert C` silently
    does nothing.
- The MR-040 row uses K2's own definition of origin, so K2's 0 holds by construction. The other rules' counts measure
  their distance from the original-times reading, not oracle faults.
- In a sync-transparency probe, a clean `sync` must not change which of two concurrent moves a later merge keeps.
- The relative lockstep is `suite::kleppmann`'s acceptance of OQ-A-12. K2 is worse in 43 histories against Wave3c and 46
  against FromB, K3 in 46 and 43, and K4 fails after about 155 histories. Every flagged history but one is a reference
  landing that the oracle calls WRONG; the exception is a DanglingEdge case.

Cost on a long-lived lane (the harness's ADV-C-4 scenario, debug build). Mean of the last 10 syncs, in ms:

| N | K2 | K4 | K3 | FromB | Wave3c | Current |
|---|---|---|---|---|---|---|
| 150 | 4.2 | 6.7 | 44.6 | 9.4 | 75.1 | 68.5 |
| 300 | 3.9 | 9.9 | 151.3 | 9.4 | 138 | 248 |
| 600 | 6.5 | 18.8 | 547.2 | 23.1 | | |
| 1,000 | 9.7 | 32.6 | | 37.3 | | |

K2's own hierarchy decision, timed inside the rule, is 72 µs per sync at N=300 and 84 µs at N=1,000. The growth of the
whole sync is merge work every rule shares. K3's growth is super-linear, so W3C-ARB-10's daily-path bound is not met.

The critic's wider generator ("multi"): 3,000 histories with 6 tasks, three lanes, 2- and 3-statement transactions, and
ours, theirs, base and abort resolutions:

| | K2 | K3 | K4 | Current | Wave3c | FromB |
|---|---|---|---|---|---|---|
| Oracle faults | 1 | 3 | 3 | 1,276 | 1,275 | 1,107 |
| Avoidable share | 0.2 % | 0.5 % | 0.5 % | 33.3 % | 34.0 % | 37.0 % |
| Wrong landings | 0 | 0 | 0 | 980 | 970 | 770 |
| Stuck stagings | 0 | 3 | 26 | 22 | 22 | 23 |

- K2's one fault is a real cycle through a node a revert brought back. On the same revert, Current lands the node at a
  value none of b, o and t holds, and the oracle counts it as correct.
- K3 and K4 each stage avoidably twice in one class: a `--base` whose base holds the tasks deleted while both sides hold
  them live. They read absent as a root; K2 lands both.
- Direction symmetry: `a --into b` and `b --into a` gave identical hierarchy results over 8,253 cross merges, under
  every rule.
- Reorder against reparent: when one side only reorders a node and the other moves it to a new parent, the later of the
  two wins as one value. The reparent was lost with no staging and no hint in about 59 % of such keys, under every
  rule: K2 193 of 325, K3 190 of 324, K4 180 of 306, Current 144 of 244.

## The judges

| Lens | Ranking | Recommendation |
|---|---|---|
| Semantics (correctness against the oracle, I25′, MR-039, MR-040, sync transparency, staged-key count) | K2, K3, K4, then the baselines | K2. It is the only rule that keeps MR-040's own sentence ("a side's own earlier move keeps its own (hlc, commit id) against a third branch") everywhere. The CONTESTED cases of wave 3d (W3D-REV-5, ADV-C-6) are artifacts of the baselines' own avoidable or over-wide stagings: under K2 the resolution that the other reading would time-stamp never happens |
| Engineering (text exactness, simplicity, I28′/I30′ determinism, daily and long-lane cost, indexes, format, redo) | K2 64 of 70, K4 45, K3 35 (baselines FromB 47, Wave3c 41, Current 17, which fail the oracle) | K2. No replay start, no step keys, no derived index, no format addition; the result depends only on b, o, t and hashed commit data, with every tie broken by uid. Three one-sentence fixes to the text, one of which matters in practice (below) |

The judges disagree on second place. The fallback, if the owner keeps a literal replay, is K4: its cost grows linearly,
K3's super-linearly.

## Findings

| id | severity | finding |
|---|---|---|
| ALT-1 | blocker | OQ-A-12's acceptance is contradicted, not only unmet. E6 has no forest: #2 under #3 is touched only by src, #4 under #2 only by dst, and #3 under #4 by neither. Every value E6 could land drops a move only one side made. "No history stages that the replay from B lands" fails for every candidate, because those histories are the reference's own wrong landings. The decision must withdraw both clauses, not only pick a rule |
| ALT-2 | major | I25′ has no exact statement for hierarchy keys. "Touched" is undefined (state or changeset). Read literally ("never emits a conflict on a key untouched on side S"), no rule meets it in a genuine cycle. GT6's property (M3) needs the text: "a merge stages a `HierarchyCycle` on a key untouched on one side only when no forest keeps every such key at the other side's value" |
| ALT-3 | major | The oracle judges only tasks live in b, o and t. 14,311 of about 68,400 judged merges hold an unjudged task, and every remaining fault of every candidate lies there. Before the oracle becomes the acceptance, it must judge every node live in the result, with absent as a value |
| ALT-4 | major | Generator coverage: every random history has 4 tasks (6 in the critic's run). None has an existence policy or `--strict`, `blocks` or `gates` edges, file nodes or re-key, hlc skew, a node created after the start, or an import between stores. Existence-fixed nodes on a cycle (ADV-B-4, S-3) remain untested |
| ALT-5 | major | VA-003 (precedence cycles over blocks ∪ gates ∪ child→parent) and VA-005 (I4, depth ≤ 12) also read the hierarchy result. A two-sided choice can cause a `Cycle` or `DepthExceeded` staging that the other choice avoids. The rule must say that its forest guarantee covers `HierarchyCycle` only, or take these validators into account |
| ALT-6 | major | K2 leaves a cycle with no repairable key to "the validators' cycle check (V05, I37′)". But [RULES/merge-table] VA-005 says "cycles are VA-001's", [F13 §5] V05 says "no cycle", and VA-001/V01 report "the moves MR-039 skipped", while K2 has no moves. These rows must agree, or such a cycle could go unstaged |
| ALT-7 | major | K2's text: "the determined keys that lie on a cycle within U" is load-bearing. Two plausible readings diverge from the model in 472 and 88 of about 49,000 decisions; it must be defined as the cycle of the option graph restricted to U. A both-same key's origin and its side on a tie are unstated (no measured effect). `Dag::origin` compares flat values, so in the model absent equals a root, against the text (no fault traced to it) |
| ALT-8 | major | A two-sided key's later value that would close a cycle no other choice avoids, or whose parent is not live in the result, lands at the other side's value: 442 keys in 12,000 histories under K2 (301 cycles, 141 dead parents). This narrows MR-039. Staging them instead costs about 364 more avoidable stagings (K2-a). Landing them needs a hint row ([AR §5a.8]) so they are not silent |
| ALT-9 | major | MR-040's "later" needs a stated reading. Under K2 a value keeps the origin of the commit that produced it, and a `sync`, a merge into `main` or a resolution to ours or theirs passes it on. A move, a revert, or a resolution to base or to a new value produces a new origin; K2's text does not say this, and base resolutions were never generated in the 12,000 histories. Part of the MR-040 evidence uses K2's own definition and so is circular |
| ALT-10 | major | A whole-value MR-040 silently drops reparents: a later order-only move beats an earlier change of parent, in about 59 % of such keys under every rule. The oracle allows either value, so no fault count shows it |
| ALT-11 | major | Companion edits missing from every candidate's list: VA-001 and [F13 §5] V01, CM-011, VA-005/V05, PR-014 (`merge --continue` drops "VA-001's skipped move"), PR-017, a new HT row, XB-004/VA-004 (K2's dead-parent preference lands 141 two-sided keys where XB-004 calls for `DanglingEdge`), [AR §5a.7] steps 3 and 4, [AR §0] item 4, [AR §2.7] T7, [F12 §5.3] VM-7, §7.4, §9.4, VB-011, open points 15 and 35, [60] GT6, and the [AR §11] rows of OQ-A-6, OQ-A-11 and OQ-A-12 |
| ALT-12 | minor | The engine cost claims are model-level. K2's origin walk is O(first-parent commits since the value was produced). The engine needs each side's per-node first-parent chain and an as-of read at every second parent the walk crosses; [AR §5a.7] step 3 names only dst's chain. The repair's cost on large hierarchies is unmeasured; I4's depth bound of 12 bounds each walk |
| ALT-13 | minor | HLC robustness: K2's determined keys never read hlc, so a skewed clock or an unchecked native import (W3C-ARB-7) can change only which of two allowed values a two-sided key takes. K3's and K4's correctness depends on the (hlc, id) order putting ancestors first |
| ALT-14 | minor | Resolution for agents: under K2 the staged key's natural `ours` re-closes the cycle in E6, so a blanket `--take ours` is refused. For a one-sided key reset to b, exactly one of ours and theirs equals b and resolves it, and the reply must name that side |
| ALT-15 | minor | A virtual-base value that only an inner virtual merge produced gets origin (0, 0). It is deterministic but arbitrary; 0 faults among 1,587 virtual-base merges |
| ALT-16 | note | Under K2, a determined key moved under a node the other side deleted stages `DanglingEdge` (V04), as XB-004 asks, where the replays landed the node at a value none of b, o and t holds (`based:1:1286`). It is the one class where K2 stages and Current lands with neither faulting |

## K2 as measured

The text the model implements (`merge/cand.rs`, variant `exact`) is in OQ-A-13 13.1 (a). The owner is asked to take it
with the exactness fixes of ALT-6 and ALT-7 and nothing grafted that has not been run. The judges proposed grafts: the
hint row for skipped two-sided moves; preferring a determined key whose single reset clears the cycle; GT6 properties
for sync transparency and "no unforced earlier landing"; and an informative note that the forest work may be restricted
to the changed keys' parent chains. These go through the re-run of the widened harness before the merge table's V3
signature.

## Reproduction

In the model, at the commit named above:

- `MOIRAI_RS007_RULE=cand cargo test -p moirai-model --locked`: the whole suite under K2. Seven pinned `suite::vcs`
  tests change, each judged right by the oracle. Under the default rule the suite passes.
- `MOIRAI_RS007_EVAL_RULE=cand MOIRAI_RS007_EVAL_VS=current,wave3c,fromb MOIRAI_RS007_EVAL_CASES=1500
  MOIRAI_RS007_EVAL_SEEDS=1,2 cargo test -p moirai-model --locked --lib rs007_eval_report -- --ignored --nocapture`:
  the corpus, the search and the cost scenario. It takes about 15 minutes.
- `MOIRAI_RS007_EVAL_CASE=E6 MOIRAI_RS007_EVAL_RULE=cand,current,wave3c,fromb cargo test -p moirai-model --locked --lib
  rs007_eval_trace -- --ignored --nocapture`: one case, traced under each rule.
- `suite::k2probe`: four pinned K2 probes, among them E6's staging and a later two-sided move that is skipped and lands.
