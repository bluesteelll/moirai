# 51 — Adversarial review of the query-language design (50, Lachesis / LQ)

*Review of `docs/research/design/50-query-language-design.md` ([50]) for owner requirement R5. Date: 2026-09-26. Research only: nothing in moirai is implemented, and this file is the only change made to the repository. Reviewer hats: language designer, engine, product. Inputs read in full: [50]; [AR] §3, §4, §5, §6, §7 and §8.1; [14] §1–§4 and §7–§8; [15] §0, §5, §9–§10; [16] §4.3–§4.5 and §6.2–§6.4; [07] §6.2; [40] §2.7–§2.9, §2.11, §6; [60] §2.5, §3.6 and §3.9.*

*Probes for this review (the probe scripts are not published):*
- *`mistakes.py` and `mistakes_results.json`: 23 agent-style LQ texts run through [50]'s own conformance checker `design-50/lqcheck.py`, which is imported read-only. The run instruments the revision lexer so that it records exactly which revspecs were read.*
- *`extra.py`: nine more grammar edge cases.*
- *`semantics_toy.py` and `semantics_toy.json`: three query shapes evaluated on a 5-task toy graph, once under [50]'s rules and once under Cypher/GQL rules. This demonstrates semantics only. It is not an implementation.*

| Tag | Meaning |
|---|---|
| **[M]** | Measured by the probes of this review (Python 3.14.5, 2026-09-26) |
| **[D]** | Documented: a specification or official documentation, re-checked today (URLs in §8), or a design document of this repository quoted by section |
| **[C]** | Third-party claim |
| **[I]** | Inference or judgement of this review |

---

## 0. Verdict

**CHANGES REQUIRED.** Three blockers, ten major issues and eleven minor issues (§7).

[50] is a serious, well-sourced design, and most of its architecture should stand:
- two entry points, with a read executor that holds only a `View`;
- derived state only through built-ins;
- counted budgets;
- `EXPECT`, `IF TIP`, `DRY` and idempotency on the canonical AST;
- measured shell transport;
- verbs implemented as named queries.

The defects sit in five places:

1. **Invisible semantic drift from the Cypher prior the design deliberately invites.** It has set-semantics counting, deduplicating `RETURN` and BFS-distance hop bounds. It also leaves gaps in "Cypher tolerance", and its direction typing cannot see reversals on the edges that matter most.
   - Of ten plausible agent mistakes tried here, **six parse and bind cleanly and return a wrong answer with no warning**.
   - The other four fail with an error that points the agent the wrong way (§3.1).
   - The retry channel that [50] relies on (LAST-CQ, CYGNET) only helps with errors the agent can see.
2. **One R3 break.** Named queries are hashed, exported schema items. Their text may contain the store-local `#N`, `s<seq>` and reflog forms that [AR §5b.5 rule 7] bans from hashed content.
3. **R4 is written against the superseded [13] model, not [40].** The two documents now contradict each other on the purity rule, built-in names, the link-state vocabulary, the anchor model and the edge-key discriminator.
4. **Several internal contradictions.**
   - Cursors are pinned to a past commit, but the `ready` those cursors page through works only at a branch tip.
   - `--ids` pagination against Unix pipes.
   - `USE` against `--branch` in the design's own pack queries.
   - The RSS claim against the design's own RSS formula.
5. **A normative grammar with a real ambiguity.** `main..lane/x` lexes as a single ref name. The design's own merge-preview example Q13 "parses" only because it is misread (§3.2 L1).

The v1 scope is also larger than any traced requirement. Because stored named queries pin the grammar version forever, every production shipped in v1 is permanent (§5.1).

## 1. What holds up (do not change)

- **The read/write split.** It is enforced by the executor's type, `&dyn View` with no path to the writer lock [50 §5.1]. This is the right defence. The claim that it is a property of the grammar is overstated (m3), but the structural guarantee is real.
- **Built-ins for derived state, sharing code with the verbs** (`ready`, `unblocked`, `blockers()`). This addresses the failure class with the highest cumulative risk in [14 §4.2].
- **Two-valued logic with an explicit absent value and W01** [50 §3.3]. It is internally consistent (`p` and `NOT p` partition the input), and it removes the classic `<>`-drops-NULL trap. Two gaps: `= NULL` (M4 below) and grouping equality (m7).
- **Counted, deterministic budgets.** They include pre-flight refusal, exit code 10 for every budget cut, and cancellation through slices on a `current_thread` runtime [50 §5.10]. The approach is sound; the issues are in how the budgets compose (M8).
- **Guarded writes**: `EXPECT` mandatory on `MATCH` targets, `IF TIP`, `DRY`, the idempotency key over the canonical bound AST, and a closed statement set that maps onto changeset ops.
- **Transport.** The measured argv forms, bare ids, and the removal of `@file` agree with [40]'s shell rules.
- **Factual citations.** The ones that could be checked from the reports are accurate (§6).

---

## 2. Method

1. Read [50] completely, then every cited section of [AR], [14], [15], [16], [07], [40] and [60] (header).
2. Ran agent-style texts through [50]'s own conformance checker `lqcheck.py`, which [50 §2.3] declares normative, to see what parses and what error text comes back. The binder and the executor do not exist, so their behaviour is read from the rules of [50]: E106 [50 §3.2], set semantics [50 §3.2 item 2, §3.4], hop bounds [50 §3.7 item 2], absent values [50 §3.3]. Such findings are tagged [I] unless the toy probe demonstrates them.
3. Checked R1–R4 against [AR §5], [AR §5b.5] and [40].
4. Checked the engine claims against [15] and against the budget arithmetic of [AR §8.1] and [50] itself.
5. Re-checked four external facts on the web (§6).

---

## 3. Language-designer hat

### 3.1 Ten plausible agent-written queries with mistakes

Each query is written the way a Claude agent with the [50 §7.2] card and a Cypher prior plausibly writes it. The "Parse" column is measured [M]. The "What LQ does" column follows [50]'s rules [I], and a toy computation [M] confirms it where noted.

| # | Agent intent | Query as written | Parse [M] | What LQ does | Steers to the fix? |
|---|---|---|---|---|---|
| 1 | open tasks with no blockers | `MATCH (t:task) WHERE t.status = 'open' AND NOT (t)<-[:BLOCKS]-() RETURN t` | E001 at 1:51 `expected RETURN (or another clause), found '<-'` | rejected. The path-pattern predicate in `WHERE` is valid, non-deprecated Cypher 25 [D, Neo4j "Path pattern expressions"]. [50 §2.8] neither accepts it nor lists it under E004 | **no**: nothing points to `NOT EXISTS {…}` or to `t.unblocked`. `exists((t)<-[:BLOCKS]-())` and `size((t)<-[:BLOCKS]-())` also give a bare E001 [M] |
| 2 | ready tasks | `MATCH (t:task {status: 'open'}) WHERE NOT EXISTS { MATCH (b:task)-[:BLOCKS]->(t) WHERE b.status <> 'done' } RETURN t` | ok | valid and **silently wrong**: it misses exogenous inheritance, flagged dangling edges, markers, leases, `defer_until` and containers, and it counts `cancelled` blockers as open | **silent**. The card's "never re-derive engine state" is the only defence |
| 3 | number of blockers per task | `MATCH (t:task)<-[:BLOCKS]-() WHERE t.open RETURN t.id, count(*) AS n ORDER BY n DESC LIMIT 10` | ok | bindings are distinct assignments of *named* variables [50 §3.2 item 2], so `n = 1` for every task. The toy graph gives LQ `{#3: 1, #2: 1}` against Cypher `{#3: 3, #2: 1}` [M] | **silent** |
| 4 | what #51 waits on | `MATCH (#51)-[:BLOCKS]->(b) RETURN b` | ok | returns what #51 blocks. Both ends are tasks, so E106 cannot fire | **silent** |
| 5 | children of #88 | `MATCH (p {id: #88})-[:CHILD_OF]->(c:task) RETURN c` | ok | returns #88's **parent**: one plausible-looking row | **silent** |
| 6 | what #93 depends on | `MATCH (t:task)-[:DEPENDS_ON]->(d:task) WHERE t = #93 RETURN d` | ok | E106 (`DEPENDS_ON` is doc → doc). By [50 §3.2] the suggestion is the reversed pattern, which is still doc → doc | **mis-steers**. The fix is `(d)-[:BLOCKS]->(#93)`. If the agent writes `BLOCKED_BY` instead, it gets E104 with no suggestion: Levenshtein(`BLOCKED_BY`, `BLOCKS`) = 5 > 2. The natural retry keeps the endpoint order and becomes the silent case 4 |
| 7 | section dependencies in plan #130 | `MATCH (s1:doc)-[:DEPENDS_ON]->(s2:doc) WHERE s1 IN descendants(#130) RETURN s1, s2` | E001 at 1:8 `expected ')', found 's1'` | `s1` lexes as a sequence-number revision literal [50 §2.2 rule 6]. `WITH s AS s1` fails too, and so does `t.s2`, although rule 3 says that any word after `.` is a name [M] | **no**: nothing mentions revision literals or back-quotes |
| 8 | unassigned open tasks | `MATCH (t:task) WHERE t.assignee = null AND t.open RETURN t` | ok | `x = v` against an absent value is false [50 §3.3], so the result is always empty | **silent** (also a Cypher/SQL trap, but LQ could reject it) |
| 9 | tasks labelled `l5` | `MATCH (t:task) WHERE 'l5' IN labels(t) RETURN t` | ok | `labels(n)` returns `[kind]` [50 §2.8], so the result is always empty. The task field is `t.labels`: same word, different thing | **silent** |
| 10 | commits on the lane not on `main` | `CALL log(main..lane/l5np) YIELD commit, actor, message` | ok, but the recorded revspec is **one ref named `main..lane/l5np`** [M] | binder: E301 unknown revision `main..lane/l5np` | **mis-steers**. The range syntax is correct, and the error says the revision does not exist. [50]'s own Q13 `diff(main...lane/l10)` misparses the same way [M] |

**Result.** Six silent wrong answers and four errors that point the wrong way.

Five cases steer well [M]:
- `shortestPath(...)` gives E113 with the `SHORTEST 1` hint;
- `SET` in `q` gives E006;
- `TX` without `EXPECT` gives E007;
- by [50]'s rules, `t.stauts` gives E101 with a did-you-mean;
- by [50]'s rules, `ready` at a past view gives E302 with `unblocked`.

The design's error channel is good where it fires. The problem is that most plausible mistakes never reach it.

One more silent case, measured on the toy graph: `MATCH (t:task) RETURN t.priority` returns 2 rows in LQ and 5 in Cypher [M], because `RETURN` deduplicates by default [50 §3.4]. The header's `n rows` then misleads any agent that counts rows.

### 3.2 Issues

**L1 — The revision lexing is ambiguous and steals identifiers.** Severity: major. Refs M1.
- `ref_seg` allows `.` [50 §2.3 lexical grammar]. `main..lane/x`, `main...lane/l10` and `v1.2..main` are therefore each lexed as one ref name [M].
- The conformance checker only reports accept/reject, so it counted Q13 as passing. The "36 blocks, all parse" claim [50 §0.4] hides a misparse.
- In expression positions, `s<digits>` and `c<7+ hex>` are literals. Common agent variable names (`s1`, `s2`) and field names after `.` (`t.s2`, contradicting rule 3) are rejected with an E001 that says nothing useful [M].
- git itself forbids `..` in ref names [D, git-check-ref-format].

**L2 — Counting and `RETURN` diverge from Cypher.** Severity: blocker. Refs B3.
- In [50 §3.2 item 2, §3.4], a `MATCH` binds distinct assignments of named variables only, and `RETURN` deduplicates by default. `count(*)` over a pattern with an anonymous element therefore changes meaning depending on whether the agent *named* an element it never uses.
- The rationale given ("set semantics is what makes closures linear" [15 §5.1]) applies only to quantified paths. [15 §5.1] itself says set and path semantics "agree on reachability, but not on counts".
- The language invites Cypher habits ("Cypher-tolerant", §2.8), so this is the "valid but semantically wrong" class that no validation gate catches (CYGNET: 0 % [D, 14 §4.1]).
- The gate-failure rule of [50 §7.4 item 6] ("changes the card, an error text, a built-in or the compatibility table — never the benchmark") offers no semantic remedy. The ablation list does not measure this choice.

**L3 — Hop bounds are BFS distances.** Severity: major. Refs M2.
- [50 §3.7 item 2] admits `b` only when the *shortest* qualifying path has length in `[m, n]`.
- On the precedence DAG, which has shortcut edges, `(x)-[:BLOCKS]->{2,}(#3)` ("indirect blockers") returns ∅ in LQ and {#1} under GQL/Cypher on the toy graph [M].
- Neo4j defines quantifiers by the number of traversals in each matched path [D].
- The cost argument ("O(nodes × n)") is weak. "There is a walk of length k with m ≤ k ≤ n" can be evaluated as a layered frontier with per-level dedup in O(min(n, m + d) × scanned edges). After level m it continues as the plain closure. On the DAG and forest kinds, walks are paths, so this reproduces the GQL/Cypher endpoint sets [I].

**L4 — Direction typing cannot see reversals on the edges that matter.** Severity: major. Refs M3.
- E106 fires only when the endpoint kind sets are asymmetric. `CHILD_OF` (task → task), `BLOCKS`, `SUPERSEDES`, `DEPENDS_ON`, `DUPLICATE_OF`, `MERGE_AFTER` and `CONTRADICTS` all have the same kind at both ends. Reversal is undetectable on exactly the task-graph edges.
- [50 §2.5]'s sentence "a reversed pattern is a type error (E106) instead of an empty result" overclaims.
- Synonyms an agent will reach for get no suggestion from Levenshtein ≤ 2: `BLOCKED_BY`, `PARENT_OF`, `HAS_CHILD`, `SUBTASK_OF`, `SUPERSEDED_BY`, and `DEPENDS_ON` used on tasks.
- The retry path goes from a visible error to a silent reversal (case 6 → case 4).
- `CONTRADICTS` is stored one way but means a symmetric relation [AR §3.3]. A directed LQ pattern misses half of the pairs (m9).

**L5 — Gaps in Cypher tolerance, and missing lints.** Severity: major. Refs M4.
- Path-pattern predicates in `WHERE`, `exists(pattern)` and `size(pattern)`: case 1.
- Multi-label `:note:rule`: E001 [M]. In moirai a node has exactly one kind, so a conjunction is always empty and should be E004 with `:note|rule`.
- `labels(t)` against a non-kind literal: case 9.
- `= NULL` / `<> NULL`: case 8.
- The rendered `P1` is not accepted back as `t.priority = 'P1'`. `t.rev = 4466`, AR's bare-integer habit, is not accepted either (not specified).
- `datetime()` with no argument (the Cypher "now") is not specified.
- There is no lint for a hand-derived ready/blocked predicate: case 2.

**L6 — Names that read as values.** Severity: minor. Refs m1.
- `t.open` means "non-terminal status", so it is true for `in_progress`. `t.done` is true for `cancelled` [AR §3.1]. In the same language, `t.status = open` (bare enum) means literally `open`. An agent reading `WHERE t.open` as "status = open" gets in-progress, leased tasks.
- `ORDER BY t.priority DESC` for "most important first" puts P4 first. The card should say so.

**L7 — Contradictions in versioned scope.** Severity: minor. Refs m2.
- `USE` against `--branch` is E307 [50 §3.9 item 1]. The design's own pack class C2 `pack_rules_unmerged` (`USE main … EXCEPT MATCH …` [50 §4.3]) runs with `--branch lane/…` and would therefore raise E307.
- The EXCEPT-by-id definition misses rules that exist on both sides but changed on `main`, which [AR §7.4] C2 counts as "not yet merged".
- The EBNF has no `USE` inside `EXISTS {}`, but the checker accepts it [M]. Correlated cross-view subqueries have no semantics, and they break "each part is evaluated to an id set" [50 §3.9 item 2].
- Nothing says which definition `USE main~5 CALL stale_blockers(…)` uses: the one at the view or the one at the caller's tip.

**L8 — Unspecified semantics.** Severity: minor. Refs m7.
- integer `/` (in Cypher, `3/2 = 1`), and division by zero;
- whether absent values group together under `DISTINCT` and `GROUP BY` (grouping equality differs from `=`);
- `EXPECT >= 0`, which is unbounded but not counted as "bulk above 10";
- whether `collect()` over large groups sorts in the arena.

---

## 4. Engine hat

### 4.1 Queries that cannot use the indexes or that blow the budget at 1e6

| # | Query shape | Plan under [50] | Consequence | Fix |
|---|---|---|---|---|
| E1 | runtime predicates as filters, e.g. [50 §4.3] `brief_triage`: `MATCH (t:task) WHERE t.settled_elsewhere OR t.deleted_elsewhere OR t.has_dangling` | no runtime-table operator in §5.4–§5.5, so the plan scans every task with one `MARKERS` probe each (0.2–1 µs [AR §3.5]) | 1e5 tasks cost 20–100 ms against the CI gate "engine ≤ 5 ms per CLI command at 1e5" and `brief` at 2–8 ms [AR §8.1] [I]. It runs on every `SessionStart` | `MarkerScan`/`LeaseScan` operators and a rewrite rule: a runtime predicate that can only be true for ids in a runtime table is anchored on that table. The result is then ∪ with the `has_dangling` bitset |
| E2 | duplicate audits (value joins): `MATCH (a:finding),(b:finding) WHERE a.local_id = b.local_id AND a.round = b.round AND a <> b` | no hash-join operator, so a cartesian product; pre-flight E201 | the correct behaviour is refusal. The only rewrite, `WITH f.local_id, f.round, collect(f)`, needs a hash aggregate of about 140k groups, which gives E502 even at the 4 MiB MCP cap [I]. The question cannot be answered at 1e6 | group by FIDX-promoted fields (`local_id` and `round` are default promotions, F5) by walking the value postings in order, with no hash map. E201 and E502 texts name that form |
| E3 | two live closures in one query: `MATCH (a:task)-[:BLOCKS]->+(b) WHERE b IN descendants(#88)`, or a closure inside `EXISTS {}` under a closure anchor | "one reusable sparse-reset visited bitset per process", excluded from the arena [50 §5.5, §5.10] | a second concurrent closure either corrupts the shared bitset or allocates memory that is not charged | extra visited sets come from the arena (N/8 = 122 KiB each at 1e6), and pre-flight counts them |
| E4 | any query whose first projected column is a scalar: `MATCH (t:task) WHERE t.open RETURN t.title` | the implicit total order [50 §3.5] turns every such query into a blocking TopK | on a work-budget cut the result is exit 10 with **no rows and no cursor** [50 §5.10]. "Partial results with a resumable cursor" holds only for id-ordered streaming plans | say so, and list which plans are resumable. The property test "resuming from the cursor yields the full result" [50 §8.3] must be limited to them |
| E5 | undirected any-kind closure from an anchor: `MATCH (#88)-[*]-(x) RETURN count(x)` | pre-flight cannot bound it; the walk runs to the visited budget | exit 10 after about 40 ms. Correct, but the E201 example "~1.0e6 reachable" [50 §5.6] is not a lower bound, as that section claims; it is an estimate | label it as an estimate. Refuse only on true lower bounds (the sum of start-set degrees from `OUT_OFF`) |
| E6 | `affected`-cone recompute at a past view [50 §5.8] | relies on F15, "affected names every node whose derived value changed" | in [AR], `suspect` propagation is op-budgeted at 10k [AR §3.5], and `affected_len` is u16 [AR §4.3]. A large merge or a heavily cited rule can exceed either, so the as-of `unblocked`/`suspect` result becomes silently wrong. `defer_until ≤ now()` changes with no commit at all | reserve an `affected_complete` bit (or a u32 length). An incomplete commit forces a full recompute. Time-dependent clauses are always evaluated at the view's `now()` for every candidate |

### 4.2 Budget and RSS composition

[50 §5.10] says: "Any query within the default budgets keeps a CLI process within the ≤ 4 MB private-RSS gate at 1e5". [50 §5.12]'s own formula is baseline 1.5–3 MB + overlay ≤ 0.6 MB + lane overlay ≤ 1 MB + visited + arena ≤ 1 MiB. That is **up to about 5.7 MB** before any as-of view. The 8,000-op reverse overlay adds about 1.2 MB [50 §5.8], which gives about 6.9 MB [I]. [AR §8.1]'s 2–4 MB estimate assumed an arena of ≤ 256 KiB.

The budgets are separate counters (arena bytes, as-of ops, refs), so no single cap bounds their sum. Refs M8.

**Fix:**
- Define one per-query *private bytes* budget covering the arena, the as-of reverse overlay, extra visited sets and aggregation state.
- Set its default to gate − measured baseline − branch overlay.
- Add a composed worst-case query (a lane view + `USE ~n` at the as-of cap + a full arena) to the GT11 RSS gate.

### 4.3 Cursors against runtime state

A cursor is evaluated "at the cursor's commit — a cheap as-of of a recent commit" [50 §3.5]. `ready`, leases, the `*_elsewhere` markers and link states are "tip only", and at a past view they raise E302 [50 §3.8].

`std.ready` pages (default limit 20). After any commit on the branch, page 2 of `moirai ready` is an as-of evaluation of a runtime predicate, which is an error. Leases also change *without* a commit, so "byte-identical output for the same query on the same view" [50 §3.5] is false for every runtime-dependent query. [16 §6.4] carried the same contradiction. Refs M5.

**Fix:** runtime-dependent queries get *live* cursors. Page N+1 runs at the current tip with a keyset on the sort key; the header says `live · pages may shift` (W06); the cursor carries a `live` flag. Pinned cursors and the determinism claim are limited to runtime-free queries.

---

## 5. Product hat

### 5.1 Is v1 too big?

The size is [50 §8.2]'s own: M5 at 42–60 units against [60 §3.6]'s placeholder of 23–39, and about 16–24k product lines. [14 §7] classed the following as "later": shortest-path selectors and path variables, `OPTIONAL MATCH`/`LET`/`NEXT`/`UNION`, `COUNT {}`, and multi-branch rows. [50] puts all of them into v1 on the reading that "no interim stages" means "everything now".

That reading is wrong for one specific reason. A stored named query records its grammar version, and "a later grammar version can still parse old definitions" [50 §2.3, §4.4]. So:
- **every v1 production is a permanent obligation**, carried by every future parser, the differential generator, LQ-Bench and the card;
- a production *added* in a later grammar version is additive, not throwaway.

The owner's rule ("built once, to its final specification") therefore favours a smaller, requirement-traced v1 grammar. This review does not argue for less quality; it argues against permanent surface that no requirement asks for.

Features with no EX requirement and no `std` query that needs them:
- path variables with `SHORTEST 1`/`ANY`, and `nodes()`/`edges()`/`length(p)`. Only Q26 uses them, and `blockers()` already returns `via`;
- `SKIP`/`OFFSET`, which also fights the keyset cursors: offsets over a moving view skip or repeat rows;
- list comprehensions and slices;
- `XOR`, `%`, `single()`;
- `RETURN ALL`;
- `FOR`, which duplicates `UNWIND`.

Spelling synonyms (`INSERT`, `GROUP BY`, no-op match modes) cost little and help tolerance, so keep them. Refs M6.

### 5.2 Compatibility with R1–R4

**R3, blocker.** Named queries are schema items in the hashed canonical changeset [AR §4.6 item 10], exported as `schema/queries/<name>.moi` [50 §4.4, F3]. Nothing forbids `#N`, `s<seq>`, `REF@n` or `REF@time` in their text. All four are store-local:
- [AR §5b.5 rule 7]: "Hashed content contains no store-local datum: no `#N`, no `seq`";
- reflog positions are not exported [AR §5b.7].

A project query `… WHERE t IN descendants(#88)` therefore:
- makes the commit id depend on store-local numbering;
- binds to a *different node* in a store that imports the image (alias remap [AR §5b.6 step 5]);
- breaks I28′/I29′ for any cross-store import.

Refs B1.

**R4, blocker.** [50] follows [13 §2] and defers to [40] "being written in parallel". [40] now exists, and it contradicts [50] on five points:

| Topic | [50] | [40] |
|---|---|---|
| purity | a read query never calls the filesystem, and `link_status()` reads the runtime cache only (§2.6, §3.1 item 4) | reads compute link states live, and "each link costs a stat" (§6.5, decision 7). The owner guarantee "a moved or vanished file is visible to every referrer at its next read" depends on it. Under [50]'s rule, the Q9 output (`link=modified/ok/moved`) cannot be produced without fresh cache entries |
| names | `link_status(f)`, `applies(r, path)`, `descendants()` | `link_state(n)`, `f.state`, `a.state`, `r.applies(glob)`, `subtree(#88)` used as a set in `WHERE`, `file('path')`, `links_broken()` |
| state vocabulary | `modified`, `moved`, `diverged`, `deleted` | `ok`, `moved-auto`, `moved-needs-confirm`, `stale-anchor`, `planned`, `pending`… "frozen in the output contract" |
| anchor model | `AT` edge props `symbol`, `line_hint`, `excerpt`, `context_hash` | an `at` edge carries 1..n anchors; the edge key gains a 128-bit **discriminator** (R-4); anchor fields `kind`, `scope`, `quote`, `watch` |
| ops and tools | a closed statement set maps 1:1 onto ops; MCP `find` is replaced by `query` | new ops `SetEdgeProps` and `PathPrefix`, which have no LQ statement; MCP `find` gains `links:*` presets, although [50] removes `find` |

The discriminator changes what an LQ edge variable binds: one binding per (src, `at`, dst, anchor), not per (src, kind, dst). That is a format-level fact that must be settled before the M0 freeze. Refs B2.

**R1, major.** An id created on another branch and not merged is `< next_id` and not deleted, so a free-form query returns **zero rows with no notice** [50 §3.6 covers only deleted ids and ids ≥ `next_id`]. The orchestrator on `main` asking about finding `#165` that a critic wrote on `lane/l5np` concludes it does not exist. [AR §5d.3] requires that "every other branch can see it on request", but the agent does not know to ask.

The notice needs a store-wide `#N → (creating ref_id, seq)` map (8–12 B per id), and nothing reserves it. Refs M7.

**R2.** Compatible. `staleness()` uses the in-process git reader of [60] M4 and never spawns.

### 5.3 The CLI/MCP token-economy contract

- **`--ids` against pipes.** Severity: major. Refs M9. `--ids` is paged at 500 rows, exits 0 [50 §3.5, §5.10] and prints no header [50 §6.4]. Footers are "never silent" and go to stdout. The owner's headline command `moirai blocking --ids | xargs moirai show` [07 §6.2, AR §7.1] either passes the footer into `xargs`, or truncates silently if the footer is suppressed. **Fix:** in `--ids` mode, no row cap (the work budget still applies); a budget cut writes its footer to **stderr** and exits 10.
- **Envelope and header drift.** Severity: minor. Refs m4.
  - [AR §7.1] and [40 §6.1] print `branch: … · rev …` and `{"v":1,"branch","rev","data"}`.
  - [50] prints `view: … @ … · s… · tip` and `{"v":1,"view":{…},"cols","data":[[…]]}`, which is not an additive change although [50 §6.4] invokes "additive changes do not bump v" [07 §6.2].
  - The claim "hooks and skills written against [AR §7.1] keep working" [50 §6.1] is therefore not true.
  - Unify one v1 envelope across [AR], [40] and [50] before M0.
- **MCP `write` refuses `DELETE`** [50 §6.3]. Severity: minor. Refs m5. `DELETE e` of an edge variable is how LQ spells `unlink` [50 §4.2]. [AR §7.2]'s `write` and [40 §6.3]'s `unlink_file` need it over MCP. Distinguish `RemoveEdge` from node `Delete`.
- **Card size.** Severity: minor. Refs m6.
  - The card measures 3,041 characters and 1,205 proxy tokens [50 §7.1], against a target of ≤ 1,000 and [14 §4.3]'s ≤ 800, "measured at build time".
  - It teaches `t.open` without the in-progress caveat, and it never says that counts are over distinct bindings.
  - `-f q.lq` gives no location. In the owner's 44 worktrees, agents will write `q.lq` into the tree: a dirty file, which feeds `dirty_files` in packs. Name a scratch location (for example `%TEMP%/moirai/`), or prefer the heredoc.

### 5.4 Does it avoid the Text2Cypher failure modes it cites?

| Failure class [14 §4.2] | [50]'s answer | Assessment |
|---|---|---|
| Syntax, from writing another dialect | Cypher spellings, E004 table | mostly handled. Gaps: case 1, multi-label, `labels()` [M] |
| Reversed direction | direction names plus E106 | handled for asymmetric kinds only. Blind on `BLOCKS`/`CHILD_OF`/`SUPERSEDES` (L4) |
| Schema linking | E101/E104 with Levenshtein ≤ 2 | good for typos, blind to synonyms (L4) |
| Aggregation | implicit and explicit grouping | **new failure mode introduced** by set-semantics counting (L2) |
| Valid but semantically wrong | built-ins, W01 | built-ins good. [50] adds new silent traps: L2, L3, L6, cases 8–9, R1 lane-only ids |
| NULL / three-valued logic | two-valued with W01 | good, except `= NULL` |
| SORT | total default order | good. The `priority DESC` trap is on the card |

### 5.5 LQ-Bench as the protecting gate

LQ-Bench [50 §7.4] is well designed on metrics (the confident-wrong rate is the right headline). It has four gaps. Refs M10.
1. **No ablation for the semantic choices most likely to be silently wrong.** It ablates the absent-value rule, but not set against bag counting, not BFS against walk bounds, and not reverse edge aliases.
2. **The remedy clause forbids semantic changes** (item 6).
3. **It drops [14 §8]'s ~30 questions mined from real sessions.** The 150 tasks are all synthetic.
4. **"Literal" phrasings written by the LQ designers leak LQ vocabulary** (`unblocked`, `descendants`). Literal should mean the owner's domain wording, not LQ's.

---

## 6. Factual checks

| Claim in [50] | Check | Result |
|---|---|---|
| ~50 % Text2Cypher figure mis-cited (SyntheT2C, not arXiv 2412.10064) | [14 §2] | consistent [D via 14] |
| Jackal 0.915–0.993 on semantically exact requests, Claude included; CYGNET 100 %/0 %; LAST-CQ 91.7 % | [14 §1, §4.1] | consistent |
| GQL zero-shot failures 85 % syntax; few-shot 1.7 → 48.2 % (GPT-5.2), Claude +5.6 pp | [14 §4.2–4.3] | consistent |
| 814 productions and 228 optional features in full GQL | [14 §3.2] | consistent, correctly tagged [C] |
| jq 76 → 31 % with the manual | [14 §3.2] | consistent, tagged [C] (secondary summary) |
| 3.9 µs / 1.7 KiB parse + bind; MS-BFS 23 MiB at 1e6 and 60–800× slower; TopK 1.1–1.6 ms | [15 §0, §5.2] | consistent |
| "the author of [14] left the restrictor out of 3 of 7 GQL queries" [M, 14] | [14 §1 item 7] | [14] labels it "n = 1, anecdotal"; [50 §3.7] drops that caveat. Minor |
| "LLMs write Cypher far more often than GQL (9,920 SO … )" | [14 §3.1] | the counts are Stack Overflow tags, a proxy for training data, not for what LLMs write. Wording |
| Claude Code skips `mcp__` permission rules with parentheses | re-checked today | **confirmed** [D]: "When Claude Code loads a settings file, it skips any `mcp__` rule that has parentheses." The same page adds that a parameter-level *deny* is possible through `--disallowedTools`; this does not change [50]'s conclusion for allow rules |
| Quantifiers `{m,n}` (§3.7 departs deliberately) | Neo4j variable-length patterns page, re-checked | Neo4j counts traversals per matched path and returns one row per path [D]. [50]'s BFS-distance rule is a real departure; see L3 |
| Pattern predicates in `WHERE` (not listed in §2.8) | Neo4j "Path pattern expressions", Cypher 25 | valid and not deprecated [D]. [50] rejects them with a non-steering E001 [M] |
| ref grammar | git-check-ref-format | git forbids `..` in ref names [D]. [50]'s `ref_seg` allows it, which causes L1 |
| "every LQ text in this document parses" (§0.4) | probe | true only as accept/reject. Q13 and every `a..b`/`a...b` range misparse as one ref [M] |

---

## 7. Issue register

### Blockers

**B1 — Named queries carry store-local data into hashed, exported content (R3).**
- *Scenario:* the orchestrator runs `DEFINE QUERY q() AS { MATCH (t) WHERE t IN descendants(#88) … }`. The `Schema{weaken, query}` op, with its text and canonical-AST hash, enters `commit_id` and `schema/queries/q.moi`.
- A second store importing the image remaps `#88` through the alias map to another node, or recomputes a different id. The query now silently queries the wrong subtree, and the native-commit verification demotes (I28′/I29′).
- The same applies to `s<seq>`, `REF@n` and `REF@time`.
- *Fix:*
  - In `DEFINE QUERY`, reject `#N`, `s<seq>` and reflog revspecs (new E1xx, "use a `$param`, a `c<hex>` commit id or a ref name").
  - If node constants are required, store them in the canonical text as uids (`#u:<32hex>`) and render them as local `#N` for display.
  - Compute the canonical-AST hash over the uid form. Add this to F3 and to the `.moi` ABNF before M0.
  - Add a property test: two stores that import the same bundle bind every named query to the same uids and hash it identically.

**B2 — The R4 parts contradict [40] (purity, names, states, anchors, discriminator, ops, MCP `find`).**
- *Scenario:* Q9 and `notes`/`pack` C7 run as LQ named queries. Under [50 §3.1 item 4], `link_status()` may not stat files, so links read `unverified` or report stale cached states. This breaks [40]'s guarantee that a moved or vanished file is visible at the next read.
- `(k)-[a:AT]->(f)` binds per edge, whereas [40]'s key is per anchor. `a.symbol` does not exist in [40].
- The [40 §6.5] named query `… n IN subtree(#88) AND link_state(a) <> 'ok'` fails to bind in LQ (E109 on `subtree`/`link_state`).
- *Fix:*
  - Adopt [40 §6.5] as the R4 part of LQ, or agree one vocabulary with [40] and edit both. Settle function versus method style (`applies(r, p)` vs `r.applies(p)`), and `descendants()` vs `subtree()`.
  - Replace the purity rule. Tree-derived built-ins may stat files in the caller's resolved tree and never write (settle points excluded). Each stat is charged to a separate `fs` budget counter. They are marked in the header (`files @ <tree>`) and excluded from the determinism claim and from pinned cursors.
  - Define edge-variable semantics under the discriminator, with one binding per anchor.
  - Either add statements for `SetEdgeProps`/`PathPrefix`, or declare them verb-only.
  - Reconcile the MCP tool list (`find` presets against `query`).
  - Do all of this before M0 freezes the edge-key format.

**B3 — Set-semantics bindings and deduplicating `RETURN` silently change counts and row sets relative to the Cypher habits the language invites.**
- *Scenario:* `MATCH (t:task)<-[:BLOCKS]-() RETURN t.id, count(*)` returns 1 for every task (Cypher: its in-degree). `MATCH (t:task) RETURN t.priority` returns 2 rows where Cypher returns 5 (toy graph [M]).
- Loop-termination and refuted-share queries written by agents in a Cypher style can under-count without warning.
- The gate-remedy clause forbids fixing semantics, and no ablation measures this choice.
- *Fix:*
  - Use Cypher/GQL binding semantics for fixed-length pattern parts: one binding per matched assignment of all elements, anonymous ones included.
  - Keep reachability (one binding per endpoint pair) only inside quantified parts, which is where [15 §5.1]'s linearity argument applies.
  - Make `RETURN` a bag and `RETURN DISTINCT` deduplicate.
  - Emit a notice when an aggregate runs over a quantified part ("counts endpoint pairs, not paths").
  - If the owner prefers set semantics, it must be loud instead: `count(*)` over a pattern with anonymous elements is an error naming `count(DISTINCT …)` or asking the agent to name the element, and the header prints `n rows (deduplicated from m)`.
  - In either case, add set-vs-bag to the LQ-Bench ablations and allow semantic changes as a gate remedy.

### Major

**M1 — The revision lexing is ambiguous (`a..b` lexes as one ref) and steals identifiers (`s1`, `t.s2`).**
- *Scenario:* `CALL log(main..lane/l5np)` and [50]'s own Q13 `diff(main...lane/l10)` are read as single ref names [M]. The agent gets E301 "unknown revision" for a correct range. `MATCH (s1:doc)` gives E001 `expected ')', found 's1'` [M].
- *Fix:*
  - Forbid `..` in ref names, as git does [D], and lex `..`/`...` before ref characters.
  - Recognise revision literals only in revision positions. Elsewhere, coerce quoted strings by type (`t.rev = 's4466'`), as is already done for enums.
  - Make the conformance fixture assert the token and AST stream, not only accept/reject.

**M2 — Hop bounds `{m,n}` are BFS distances, which silently differ from GQL/Cypher on the precedence DAG.**
- *Scenario:* "indirect blockers of #3" `(x)-[:BLOCKS]->{2,}(#3)` returns ∅ in LQ and {#1} in GQL/Cypher when #1 blocks #3 both directly and through #2 [M].
- *Fix:* define `{m,n}` as "a walk of length k with m ≤ k ≤ n exists". Evaluate it as a layered frontier with per-level dedup up to level m, then as the plain closure, in O(min(n, m + d) × edges). Show the strategy in EXPLAIN.

**M3 — Direction typing is blind on same-kind edges, and synonyms get no suggestion.**
- *Scenario:* `(#51)-[:BLOCKS]->(b)` for "what #51 waits on" and `(#88)-[:CHILD_OF]->(c)` for "children of #88" both bind and return wrong rows. `BLOCKED_BY` gives E104 without a suggestion, and the retry keeps the endpoint order.
- *Fix:*
  - Add a reverse-alias table: `BLOCKED_BY` → `BLOCKS`, `PARENT_OF`/`HAS_CHILD` → `CHILD_OF`, `SUPERSEDED_BY` → `SUPERSEDES`, each with swapped endpoints. Aliases are accepted and canonicalised, or rejected with the fully rewritten pattern as the suggestion.
  - Give a targeted hint for `DEPENDS_ON` on tasks.
  - Add notice N07 for anchored patterns on same-kind edges that return 0 rows while the reverse direction has k > 0; it costs one CSR slice length.
  - Optionally add a one-line reading echo for anchored single-hop patterns (`c = parent of #88`).
  - Match symmetric kinds (`CONTRADICTS`, `RELATES`) in both directions.
  - Correct the E106 overclaim in [50 §2.5].

**M4 — Gaps in Cypher tolerance, and errors that point the wrong way.**
- *Scenario:* cases 1, 8 and 9 of §3.1, plus multi-label (E001) and `'P1'`/bare-integer `rev`.
- *Fix:*
  - Accept `WHERE [NOT] (a)-[…]-(b)` and `exists(pattern)` as `[NOT] EXISTS {…}`, and `size(pattern)` as `COUNT {…}`.
  - Multi-label: E004 with "`:note|rule`; a node has one kind".
  - `labels(x)` compared with a non-kind literal: E102 with `'l5' IN x.labels`.
  - `= NULL`/`<> NULL`: error with "use IS NULL".
  - Coerce `'P1'` to 1, and a bare integer to a revision for `rev`/`created`/`updated`.
  - `datetime()` with no argument means `now()`.
  - Lint W07 for hand-derived blocked/ready predicates (`NOT EXISTS` over `BLOCKS` in-edges, or a status comparison on blockers), suggesting `t.unblocked`/`t.ready`.

**M5 — Cursors pinned to a past commit contradict the tip-only runtime state.**
- *Scenario:* page 2 of `moirai ready` after any commit is an as-of evaluation of `t.ready`, which is E302 by [50 §3.8]. Leases change without commits, so the "byte-identical output on the same view" claim is false for these queries.
- *Fix:* runtime-dependent queries get live keyset cursors at the current tip (flagged `live`, W06, with "pages may shift" in the header). Pinned cursors and the determinism claim are limited to runtime-free queries.

**M6 — The v1 grammar is larger than any traced requirement, and every shipped production is permanent.**
- *Scenario:* M5 grows to 42–60 units (against 23–39). Path variables, `SKIP`/`OFFSET`, list comprehensions and slices, `XOR`, `%`, `single`, `RETURN ALL` and `FOR` all enter the differential generator, LQ-Bench, the card and every future parser. Stored named queries pin the grammar version forever.
- *Fix:*
  - Add a requirement-trace column to the EBNF (production → EX requirement, `std` query, or LQ-Bench evidence that agents write it).
  - Drop the untraced semantic features from grammar v1 and keep the cheap spelling synonyms.
  - Re-estimate M5.
  - Later additions are new grammar versions, which are additive and not throwaway.

**M7 — Ids created on another branch return empty results silently (R1), and several needed format fields are not reserved.**
- *Scenario:* on `main`, `MATCH (f {id: #165}) RETURN f` gives 0 rows and no notice. #165 exists on `lane/l5np`.
- F15 (`affected` complete) cannot hold under [AR]'s 10k-op `suspect` budget and the u16 `affected_len`, so as-of derived values go silently wrong.
- *Fix:* reserve at M0:
  - a store-wide `ALLOC` section `#N → (ref_id, create seq)`, which enables notice N06 "#165 is not in this view; created on lane/l5np s4473";
  - an `affected_complete` bit or a u32 length in the commit header, with a full-recompute fallback;
  - violation class codes for `QueryInvalid`/`QueryCycle`.
- Time-dependent clauses are always re-evaluated at the view's `now()`.

**M8 — Engine: runtime predicates have no anchor, and the RSS budgets do not compose.**
- *Scenario:* `brief_triage` scans every task with one marker probe each, 20–100 ms at 1e5, against the 5 ms CI gate. The composed worst case (a lane plus as-of at 8k ops plus a full arena) reaches about 5.7–6.9 MB against the claimed ≤ 4 MB at 1e5.
- Two live closures in one query share the one per-process visited bitset.
- Duplicate audits cannot be answered at 1e6.
- *Fix:*
  - Add `MarkerScan`/`LeaseScan` operators with an anchor rewrite.
  - Use one per-query private-bytes budget covering the arena, the reverse overlay, extra visited sets and aggregation state, with its default derived from the gate. Add a composed worst case to GT11.
  - Charge extra visited sets to the arena.
  - Group by FIDX-promoted fields by walking the postings.
  - Label pre-flight estimates as estimates, and list which plans can be resumed.

**M9 — `--ids` pagination breaks pipes, or truncates silently.**
- *Scenario:* `moirai blocking --ids | xargs moirai show` with more than 500 blocking tasks. The stdout footer becomes an `xargs` argument, or, if the footer is suppressed, the list is silently cut.
- *Fix:* `--ids` has no row cap and is bounded by the work budget only. A cut writes its footer and cursor to stderr and exits 10.

**M10 — LQ-Bench cannot correct the semantic choices most likely to be silently wrong.**
- *Scenario:* L2, L3 and L4 fail the confident-wrong gate. The remedy clause allows only card, error-text, built-in and compatibility-table changes, so the benchmark would be "passed" by card wording.
- *Fix:*
  - Add ablations: set vs bag, BFS vs walk bounds, reverse aliases on and off.
  - Allow semantic changes as a remedy.
  - Add [14 §8]'s ~30 real-session questions as a stratum.
  - Write the "literal" phrasings in the owner's domain words, not in LQ vocabulary.
  - Report confident-wrong per construct.

### Minor

**m1 — `t.open` / `t.done` read like status values.**
- *Scenario:* `WHERE t.open` returns `in_progress` tasks; `t.done` includes `cancelled`.
- *Fix:* rename `open` (for example `unfinished`), keep AR's `done` but state `done ∪ cancelled` on the card, and add the `priority DESC` note.

**m2 — `USE` vs `--branch` E307 contradicts the pack composites, and the C2 `~main` definition diverges from [AR §7.4].**
- *Scenario:* `pack_rules_unmerged` called with `--branch lane/x` hits E307. A rule changed on `main` but present on the lane is missed.
- *Fix:*
  - `--branch` sets the default view only for parts without `USE`.
  - Define C2 with `diff(B...main)`, side `theirs`.
  - Specify that a named query called under `USE` resolves its definition at the caller's tip.
  - Allow or forbid `USE` in subqueries, consistently in the EBNF and the checker.

**m3 — "Read-only is a property of the grammar" is not literally true.**
- *Scenario:* `CALL std.complete(#89, outcome: 'done')` parses under the read grammar [M]. Only a catalog check refuses it, and the verb `stale` "records the facts it learned" although it is the named query `std.stale`.
- *Fix:* separate namespaces (`std.q.*` / `std.tx.*`) with a binder check. Restate the guarantee as "grammar plus executor type". Make `moirai q stale` pure and keep fact recording in `check`.

**m4 — The v1 output envelope and header differ across [AR], [40] and [50].**
- *Scenario:* hooks parse `branch: … · rev …` while `q` prints `view: …`. JSON has `branch`/`rev` in one place and `view`/`cols` in another.
- *Fix:* one frozen v1 envelope for every verb, into the M0 contract. Drop the "keeps working" claim.

**m5 — The MCP `write` refuses every `DELETE`, which removes `unlink`.**
- *Scenario:* an architect over MCP cannot remove a `DEPENDS_ON` edge, and [40]'s `unlink_file` has no LQ spelling.
- *Fix:* `DELETE <edge var>` is `RemoveEdge` under the role policy; node `DELETE` stays CLI-only.

**m6 — The card size is unverified, and the card omits counting semantics and a scratch location.**
- *Scenario:* 1,205 proxy tokens against targets of ≤ 1,000 ([50]) and ≤ 800 ([14]). Agents write `q.lq` into the worktrees.
- *Fix:* measure with the tokenizer before the freeze; add a line on counting; name a scratch path outside the tree.

**m7 — Arithmetic and grouping semantics are unspecified.**
- *Scenario:* `refuted / raised` with integers; division by zero; whether absent values group together.
- *Fix:* specify them: `/` on integers gives a float (or errors, as the owner prefers); division by zero gives absent with a notice; absent values form one group.

**m8 — Named queries merge by line-level diff3 and are not re-validated after a merge.**
- *Scenario:* two non-overlapping edits merge into a text that nobody wrote or bound. It lands on `main` and fails only at use.
- *Fix:* merge a query definition as an atomic value (`FieldEdit`). Parse and bind merged queries as a merge validator, staging `QueryInvalid` like `QueryCycle`.

**m9 — Symmetric edge kinds are typed as directed.**
- *Scenario:* `(#212)-[:CONTRADICTS]->(r)` misses contradictions stored from the other rule.
- *Fix:* mark `CONTRADICTS` (and `RELATES` if symmetric) as symmetric in F1 and match both directions.

**m10 — Role policy is widened without an owner decision.**
- *Scenario:* a developer may `SET` any field on the leased task, including `parent` (a `Move`), `status` and `priority`. [AR §7.3] lists `files_owned` and similar.
- *Fix:* give a per-role field allowlist in the role table, and put the widening in front of the owner as a decision.

**m11 — The requirement that search tiers return identical rankings is stronger than specified.**
- *Scenario:* BM25 statistics (N, df, average length) differ between segment postings, the overlay and branch views, so tier 1 and tier 2 disagree on a lane.
- *Fix:* define the statistics as computed over the view's live documents (re-tokenising overlay documents) and state the cost, or relax the requirement to "equal result sets, and equal order when statistics are equal".

## 8. Required before approval, in order

1. **B1**: the canonical form of stored named queries, into F3 and the `.moi` ABNF (M0).
2. **B2**: reconcile R4 with [40] and edit both documents (M0, because of the edge-key discriminator).
3. **B3, M2, M3, M4**: fix the semantic surface, then **M10** to extend LQ-Bench before the freeze.
4. **M1**: fix the grammar and make the conformance fixture assert token streams.
5. **M5, M9, m4**: fix the output contract (cursors, `--ids`, the envelope).
6. **M7, M8**: format reservations and engine operators and budgets (M0 for the reservations).
7. **M6**: requirement-trace pass and re-estimate of M5.
8. The minor issues, alongside the above.

**Sources re-checked today [D]:**
- Neo4j Cypher Manual, path pattern expressions (Cypher 25): https://neo4j.com/docs/cypher-manual/current/expressions/predicates/path-pattern-expressions/
- Neo4j Cypher Manual, variable-length patterns: https://neo4j.com/docs/cypher-manual/current/patterns/variable-length-patterns/
- git-check-ref-format: https://git-scm.com/docs/git-check-ref-format
- Claude Code permissions: https://code.claude.com/docs/en/permissions
