# LQ-Bench corpus plan: allocation and author batches

| | |
|---|---|
| **Title** | Allocation of the 150 synthetic and 40 adversarial LQ-Bench tasks to strata, constructs and author batches |
| **Status** | draft, pass 1 pending |
| **Work package** | WP-70 (lane B, role R-BENCH; S6) |
| **Sources** | [50 §7.4] item 2 (strata and sizes, the three phrasings, the adversarial trap list), item 4 (metrics per construct tag), item 5 (the shrink rule), item 6 (gates), item 7 (ablations and their prompt sets); [90 §8.3] (the repeated 52-prompt sample, the transport arms of 20 literal prompts, the headless runner); [AR §7.7.5]; [60 §3.1] item 10; [m0 PLAN §3.2] WP-70…WP-73 |
| **Companion files** | [README.md](README.md) (format; cited README §n), [store-design.md](store-design.md) (cited SD §n) |

---

## 1. Totals

| Part | Tasks | Prompts | Ids | Where |
|---|---|---|---|---|
| Synthetic, ten strata | 150 | 450 | `T001`–`T150` | `tasks/B1.jsonl` … `tasks/B5.jsonl` |
| Adversarial | 40 | 40 | `A01`–`A40` | the same files |
| Real sessions | ≈ 30 | ≈ 30 | `R01`–`R30` | `/private/lqbench/` only (README §9); not in any batch |
| **Total** | 220 | **520** | | [50 §7.4] item 2 |

| Stratum | Tasks | Ids | Batch |
|---|---|---|---|
| lookup | 15 | T001–T015 | B1 |
| aggregation | 15 | T016–T030 | B1 |
| filter | 20 | T031–T050 | B2 |
| search | 10 | T051–T060 | B2 |
| traversal | 20 | T061–T080 | B3 |
| merge | 10 | T081–T090 | B3 |
| derived | 15 | T091–T105 | B4 |
| write | 15 | T106–T120 | B4 |
| history | 20 | T121–T140 | B5 |
| links | 10 | T141–T150 | B5 |

## 2. The batch list

Five disjoint batches of 30 synthetic and 8 adversarial tasks each (38 tasks, 98 prompts). A batch owns its id ranges,
its file `tasks/B<n>.jsonl` and nothing else. Batches pair strata whose questions use the same store clusters, so an
author learns a small part of the store design well.

| Batch | Task-id ranges | Strata | Adversarial ids and traps | Primary constructs (minimum tasks, §4) | Primary store clusters |
|---|---|---|---|---|---|
| **B1** | T001–T030, A01–A08 | lookup (T001–T015), aggregation (T016–T030) | A01–A03 `count-anonymous`; A04–A05 `count-quantified`; A06 `int-division`; A07–A08 `id-other-branch` | id-lookup 8, count-bag 5, count-entity 5, group-having 5, arith 4, count-quantified 3, exists-optional 3, edge-knowledge 3, tombstone-elsewhere 3, kind-label 3, named-query 8, non-ascii 3 | knowledge core SD §5.2; review loop SD §5.9–§5.10; deletion cluster SD §5.8; totals SD §13 |
| **B2** | T031–T060, A09–A16 | filter (T031–T050), search (T051–T060) | A09–A10 `eq-null`; A11–A12 `labels-fn`; A13–A14 `t-open`; A15–A16 `priority-desc` | search 10, field-filter 8, set-field 6, ordering 5, text-predicate 4, absent-value 4, kind-label 4, applies 3, time-provenance 3, named-query 8, non-ascii 3 | all task clusters SD §5.3–§5.8; rules and notes SD §5.2; signal terms SD §14; totals SD §13 |
| **B3** | T061–T090, A17–A24 | traversal (T061–T080), merge (T081–T090) | A17 `reversed-blocks`; A18 `reversed-blocks-alias`; A19 `reversed-child-of`; A20 `reversed-child-of-alias`; A21 `reversed-supersedes`; A22 `reversed-supersedes-alias`; A23 `blocked-by-retry`; A24 `indirect-hop-shortcut` | merge-state 10, edge-direction-blocks 5, reverse-alias 5, closure 5, hop-bound 5, edge-direction-child 4, multi-hop 4, quantified-group 4, edge-knowledge 4, named-query 8, non-ascii 3 | campaign DAGs and plan sections SD §5.3–§5.5, §6; refs, conflicts and the staged violation SD §3.1, §10, §11.6 |
| **B4** | T091–T120, A25–A32 | derived (T091–T105), write (T106–T120) | A25–A27 `hand-ready`; A28–A29 `done-cancelled`; A30–A31 `write-in-q`; A32 `hash-argv` (`surface: cli`) | tx-guarded 8, tx-structural 7, derived-ready 6, derived-structural 6, blockers-fn 5, runtime 5, derived-done 4, named-query 8, non-ascii 3 | derived state SD §11.1–§11.5; leases SD §8.1; campaigns SD §5.3–§5.8 |
| **B5** | T121–T150, A33–A40 | history (T121–T140), links (T141–T150) | A33 `runtime-past-view` (ready); A34 `runtime-past-view` (lease); A35 `link-past-view`; A36–A37 `ranges`; A38 `s1-variable`; A39 `indirect-hop-shortcut`; A40 `hash-argv` (`surface: cli`) | file-link 10, view-use 8, history-fn 6, diff-range 4, set-op 3, named-query 8, non-ascii 3 | history cluster SD §5.7; commit schedule and chain SD §7, §11.6; file links SD §9 |

## 3. Adversarial allocation

The forty tasks cover every trap family of [50 §7.4] item 2; the family sizes follow how often the trap is expected and
how many distinct store shapes can carry it.

| Family ([50 §7.4] item 2) | Trap ids | Tasks | Batch | Store shapes to use (SD) |
|---|---|---|---|---|
| reversed `BLOCKS` / `CHILD_OF` / `SUPERSEDES`, with and without aliases | `reversed-blocks`, `reversed-blocks-alias`, `reversed-child-of`, `reversed-child-of-alias`, `reversed-supersedes`, `reversed-supersedes-alias` | 6 (A17–A22) | B3 | `BLOCKS` into #277, #280, #295 (§6); `CHILD_OF` under #248, #240, #273 (§5.3, §5.5); `SUPERSEDES` #213→#212, #220→#221, #237→#236 (§6) |
| `BLOCKED_BY`-style retries | `blocked-by-retry` | 1 (A23) | B3 | a task with blockers and no dependents (#277, #297) so the doubly reversed query is empty with N07 |
| counts over anonymous elements | `count-anonymous` | 3 (A01–A03) | B1 | blockers per task (#280 has 3), findings per section (#502, #803 about two sections), children per container |
| counts over quantified parts | `count-quantified` | 2 (A04–A05) | B1 | upstream of #280 (3 endpoints), descendants of #240 |
| "indirect" hop bounds on DAGs with shortcuts | `indirect-hop-shortcut` | 2 (A24, A39) | B3, B5 | #241→#243→#245 with #241→#245; #274→#275→#280 with #274→#280; `DEPENDS_ON` #259 (§6); A39 at a past view |
| `= NULL` | `eq-null` | 2 (A09–A10) | B2 | assignee absent (SD §13), `estimate` absent, `defer_until` absent |
| `labels()` | `labels-fn` | 2 (A11–A12) | B2 | labels `perf`, `test`, `hot` (SD §13) |
| `t.open` | `t-open` | 2 (A13–A14) | B2 | open against unfinished (in_progress #242, frozen #267) |
| `t.done` with cancelled tasks | `done-cancelled` | 2 (A28–A29) | B4 | campaign A (#253, #902 cancelled), tooling (#288 cancelled) |
| `priority DESC` | `priority-desc` | 2 (A15–A16) | B2 | "most important first" over campaign subtrees (P0 #261, #276, #277) |
| hand-written `ready` | `hand-ready` | 3 (A25–A27) | B4 | subtrees where the engine's `ready` differs from "no open blocker": #276 (leased), #274 (settled elsewhere), #249 (inherited), #297 (flagged), #265 (deferred) |
| ids created on another branch | `id-other-branch` | 2 (A07–A08) | B1 | #1402, #1304, #1400 asked from `main` (N06) |
| runtime and link states at past views | `runtime-past-view`, `link-past-view` | 3 (A33–A35) | B5 | `ready` at `tags/m1` (answer: `unblocked`), a lease at `main~30`, a link state at `main~20` (gold `error` E302, README §7.6) |
| `s1`-style variables | `s1-variable` | 1 (A38) | B5 | sections of #254 with `DEPENDS_ON`, as of a revision |
| ranges | `ranges` | 2 (A36–A37) | B5 | `log(main..lane/net)`, `diff(main...lane/audio, scope: #260)` (SD §11.6) |
| `#` in argv | `hash-argv` | 2 (A32, A40) | B4, B5 | `surface: cli` (README §3.1): a `show`, `blockers` or `history` call on a named id |
| write keywords in `q` | `write-in-q` | 2 (A30–A31) | B4 | a simple guarded change phrased as a question-like request; gold `kind: diff` |
| integer division | `int-division` | 1 (A06) | B1 | a share or average whose integer quotient differs from the float (refuted share 1/5) |
| **Total** | | **40** | | |

Gold results of `kind: error` are capped at four in the whole corpus: at most one in B3, one in B4 and two in B5 (A35
counts as one).

## 4. Construct coverage

[50 §7.4] item 6 gates the confident-wrong rate at ≤ 5 % **per construct tag**. With n prompts the gate tolerates
⌊0.05 n⌋ confident-wrong answers, so every tag is planned for at least 20 prompts (one tolerated error). A synthetic task
contributes 3 prompts to each of its tags, an adversarial task 1. The table gives the **minimum** number of synthetic
tasks per batch that carry each tag; authors may exceed it within the 1–4 tags per task of README §3.1.

| Tag | B1 | B2 | B3 | B4 | B5 | Adversarial | Minimum prompts |
|---|---|---|---|---|---|---|---|
| id-lookup | 8 | | | | | | 24 |
| kind-label | 3 | 4 | | | | A38 | 22 |
| field-filter | 2 | 8 | | | | A13, A14 | 32 |
| set-field | | 6 | | | | A11, A12 | 20 |
| text-predicate | 1 | 4 | | | 2 | | 21 |
| absent-value | 1 | 4 | | 1 | | A09, A10 | 20 |
| ordering | 2 | 5 | | | | A15, A16 | 23 |
| time-provenance | 2 | 3 | | | 2 | | 21 |
| edge-direction-blocks | | | 5 | 1 | | A17, A18, A23 | 21 |
| edge-direction-child | 2 | | 4 | | | A19, A20 | 20 |
| edge-direction-supersedes | | 2 | 3 | | 1 | A21, A22 | 20 |
| edge-knowledge | 3 | | 4 | | | | 21 |
| edge-other | 2 | | 3 | | 2 | | 21 |
| reverse-alias | | | 5 | 1 | | A18, A20, A22, A23 | 22 |
| multi-hop | 2 | 1 | 4 | | | | 21 |
| closure | | | 5 | 2 | | | 21 |
| hop-bound | | | 5 | | 1 | A24, A39 | 20 |
| quantified-group | | | 4 | 2 | 1 | | 21 |
| subtree | 2 | | 3 | 2 | | | 21 |
| exists-optional | 3 | 2 | 2 | | | | 21 |
| derived-ready | | | | 6 | | A25, A26, A27, A33 | 22 |
| derived-structural | 1 | | | 6 | | | 21 |
| derived-done | | 2 | | 4 | | A28, A29 | 20 |
| blockers-fn | | | 2 | 5 | | | 21 |
| runtime | | | | 5 | 2 | A34 | 22 |
| applies | 2 | 3 | | | 2 | | 21 |
| count-bag | 5 | | 1 | | | A01, A02, A03 | 21 |
| count-entity | 5 | 1 | 1 | | | | 21 |
| count-quantified | 3 | | 3 | | | A04, A05 | 20 |
| group-having | 5 | | | | 2 | | 21 |
| arith | 4 | 2 | | 1 | | A06 | 22 |
| set-op | 2 | 2 | | | 3 | | 21 |
| search | | 10 | | | | | 30 |
| non-ascii | 3 | 3 | 3 | 3 | 3 | | 45 |
| file-link | | | | | 10 | A35 | 31 |
| view-use | | | 1 | | 8 | A33, A34, A35 | 30 |
| history-fn | | | 1 | | 6 | | 21 |
| diff-range | | | 3 | | 4 | A36, A37 | 23 |
| tombstone-elsewhere | 3 | | 2 | | 1 | A07, A08 | 20 |
| merge-state | | | 10 | | | | 30 |
| tx-guarded | | | | 8 | | A30, A31 | 26 |
| tx-structural | | | | 7 | | | 21 |
| named-query | 8 | 8 | 8 | 8 | 8 | where `named` is set | 120 |
| surface | 2 | 2 | | 1 | 1 | A30, A31, A32, A40 | 22 |

Tag slots per batch at the minima: B1 76, B2 72, B3 82, B4 63, B5 59, that is 2.0–2.7 tags per task, inside the limit
of 4.

## 5. Other per-batch quotas

| Quota | B1 | B2 | B3 | B4 | B5 | Rule |
|---|---|---|---|---|---|---|
| tasks with a `named` query (README §3.1) | ≥ 8 | ≥ 8 | ≥ 8 | ≥ 8 | ≥ 8 | from [50 §4.1]: B1 `show`, `find`, `tree`, `loop`, `refuted_share`; B2 `find`, `notes`; B3 `blockers`, `tree`, `conflicts`, `violations`, `diff`, `across`, `log`; B4 `ready`, `blocking`, `blockers`; B5 `history`, `blame`, `log`, `diff`, `links_broken`, `links_pending`, `files_removed`, `files_replaced` |
| tasks whose answer is read on a lane, tag, plan or staging view | ≥ 2 | ≥ 3 | ≥ 6 | ≥ 4 | ≥ 6 | only the facts SD §11.2–§11.6 and §10 designs |
| `non-ascii` tasks | ≥ 3 | ≥ 3 | ≥ 3 | ≥ 3 | ≥ 3 | a Cyrillic literal (SD §14 terms, Cyrillic titles) or all three phrasings in Russian |
| `error` golds | 0 | 0 | ≤ 1 | ≤ 1 | ≤ 2 | README §7.6 |
| rows per gold | ≤ 20 | ≤ 20 | ≤ 20 | ≤ 20 | ≤ 20 | README §11 rule 6 |
| `surface: cli` | 0 | 0 | 0 | A32 only | A40 only | README §3.1 |

**Write stratum (B4, T106–T120), minimum mix.** Each item is at least one task; a task may cover several:
a compare-and-set `SET` with `EXPECT 1`; a bulk `MATCH … EXPECT n` with n > 1 plus an `ASSERT`; `IF TIP
{{commit:…}}`; `LEASE '{{lease:LS-276}}'` with `CALL tx.complete(#276, …)` on `lane/net`; a lease-guarded change on
`lane/audio` (`LS-264`); `CREATE` of a finding `ABOUT` a section with `UNLESS EXISTS`; `CREATE (x:task …) UNDER` a
container; an edge `CREATE`; an edge `DELETE` through an edge variable; `MOVE … UNDER`; `REOPEN … REASON` on a done task;
a node `DELETE … REPLACED BY`; `RESOLVE` of `#277.priority` on `lane/net` or of the staged `DanglingEdge`. At least three
write tasks commit on a lane (`TX ON lane/…`). Every write gold lists `targets` and primary `changes` (README §7.5).

## 6. Deterministic subsets

**Canonical prompt order.** (1) The synthetic prompts, stratum by stratum in the order lookup, filter, traversal,
derived, aggregation, search, links, history, merge, write; inside a stratum by task id; inside a task in the order
literal, short, paraphrased. (2) The adversarial prompts A01…A40. (3) The real-session prompts R01…R30 (as many as the
private stratum holds). Position 0 is the first prompt. The runner computes every subset below from this order; nobody
picks prompts by hand.

| Subset | Size | Rule | Used by |
|---|---|---|---|
| **Baseline** | 520 | every prompt | the baseline; D8 and D11 ablations; the two alternative surfaces and the display-spelling ablation unless the shrink rule moves them ([50 §7.4] items 5, 7) |
| **Stratified half** | 260 | all three phrasings of: the tasks at odd positions (1st, 3rd, …) of lookup, filter, traversal, aggregation, search, links, history and merge, and at even positions (2nd, 4th, …) of derived and write — 8 + 10 + 10 + 8 + 5 + 5 + 10 + 5 + 7 + 7 = 75 tasks, 225 prompts; the adversarial prompts with odd numbers (A01, A03, …, A39): 20; the real-session prompts with odd numbers: 15 | the other ablations of [50 §7.4] item 7, BM25 against the statistics-free scorer among them; shrink step (ii) for the alternative surfaces and the display spelling |
| **Quarter** | 130 | the prompts at even positions (0, 2, 4, …) of the half, in the half's canonical order: 113 synthetic, 10 adversarial, 7 real-session | shrink step (i): the non-gate ablations |
| **Without paraphrases** | 370 | the baseline minus the 150 paraphrased prompts | shrink step (iii): D8 and D11 |
| **Repeated sample** | 52 | positions 0, 10, 20, …, 510 of the baseline: 45 synthetic, 4 adversarial, 3 real-session | the second run of [90 §8.3] item (2) that measures the run-to-run spread |
| **Transport** | 20 | the literal phrasing of the task at position 1 and of the task at position ⌊n/2⌋ + 1 of each stratum (n its size) | each transport arm: Claude Code, and the scripted generic stdio client ([50 §7.4] item 3) |

When the real-session stratum holds fewer than 30 prompts, the half takes its odd-numbered prompts, the quarter and the
repeated sample follow the canonical order, and every report states the resulting sizes. Each gate is reported with its
sample size and 95 % interval ([50 §7.4] item 5).

## 7. Authoring workflow and acceptance

1. **Order of work.** The store design is fixed before any batch starts; batches are independent and run in parallel.
   A batch author who finds a gap in the store design files it with the batch lead (the R-BENCH session that owns
   store-design.md) and waits for the amended design; no batch edits store-design.md or another batch's file.
2. **Per-batch self-check** (README §11 rule 8): every gold recomputed a second time from the store design, section
   references in `notes`, the construct minima of §4 and the quotas of §5 met, the loader's lints clean.
3. **Cross-batch check** by the batch lead: no duplicate gold query, no two tasks whose phrasings differ only in ids, the
   construct totals of §4 reached over the whole corpus, every trap family of §3 present.
4. **Owner sample (V3).** The owner verifies a sample of gold results: five synthetic tasks and one adversarial task per
   batch, chosen by the owner, and ten real-session golds. A gold the owner rejects is corrected and its batch re-checked.
5. **Parse check.** Once WP-93a exists, every gold query is parsed and bound by LQ-3's front end; a failure is a corpus
   finding, or, when the gold follows [50 §2.3] exactly, a finding against the front end (README §6).

## Holes

| Id | What | Decided by | Candidates | Constraint the value must meet |
|---|---|---|---|---|
| HOLE(lqb-window-overhead) | which step of the shrink rule the GT13 runs use | Claude Code's per-call overhead measured from its reported usage in M0's first usage window ([90 §8.3]); the quota plan is re-issued on it (WP-71b, WP-72) | no shrink; step (i); steps (i)–(ii); steps (i)–(iii) | the baseline's 520 prompts and both transport arms are never cut; no gate is skipped; each gate carries its sample size and 95 % interval |

## Open points for the review

1. **Batches pair strata** (lookup with aggregation, filter with search, traversal with merge, derived with write,
   history with links) rather than mixing strata, so that each author works one region of the store design and the
   per-batch construct minima stay reachable.
2. **Split trap families.** `runtime-past-view` and `link-past-view` together are item 2's "runtime and link states at
   past views" (3 tasks); `indirect-hop-shortcut` (2) and `hash-argv` (2) are split over two batches each to vary their
   store shapes.
3. **Minimum 20 prompts per construct tag**, so that the ≤ 5 % gate tolerates exactly one confident-wrong answer per
   tag; a tag with fewer prompts would make the gate "zero confident-wrong" by arithmetic.
4. **Subsets are defined at prompt level in one canonical order**, which makes the half, the quarter, the 370-prompt set
   and the repeated sample reproducible and stratified; the quarter is 113 + 10 + 7 rather than exact proportions
   because 450 synthetic prompts do not divide by four.
5. **The transport arm uses two literal prompts per stratum** (20), writes included, so a quoting or encoding failure in
   any stratum's typical query shape shows up.
6. **The owner's V3 sample size** (25 synthetic and adversarial golds, 10 real-session golds) is a proposal inside
   [60 §3.15]'s 10–20 hours for GT10 and the gold sample.
