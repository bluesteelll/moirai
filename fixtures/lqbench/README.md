# LQ-Bench corpus: format and authoring rules

| | |
|---|---|
| **Title** | LQ-Bench corpus format, strata, construct tags, phrasing kinds and gold-result encoding |
| **Status** | draft, pass 1 pending |
| **Work package** | WP-70 (lane B, role R-BENCH; separation rule S6: gold queries and gold results are written from the task text by R-BENCH, never by R-MODEL) |
| **Sources** | [50 §7.4] items 1–9 (LQ-Bench); [50 §2.2–§2.9] (syntax, revision grammar, schema vocabulary, built-ins, worked examples); [50 §3.1–§3.10] (semantics: binding and counting, absent values, ordering, deleted nodes, recursion, derived and runtime state, versioned scope, mutations); [50 §4.1–§4.4] (standard library, named mutations); [50 §5.2] (error, warning and notice codes); [50 §6.1–§6.4] (CLI, transport, MCP, output envelope); [50 §7.1–§7.3] (card, few-shot); [AR §7.7.5] (normative GT13 gate list); [90 §8.1–§8.3] (display spelling, model profiles, the headless runner); [60 §3.1] item 10 and exit; [m0 PLAN §3.1] S6, §3.2 WP-70…WP-73, §3.3 (gap g-10) |
| **Companion files** | [store-design.md](store-design.md) (the seeded fixture store and every handle a task may cite); [plan.md](plan.md) (allocation of the 150 + 40 tasks to strata, constructs and five author batches; the stratified subsets of the shrink rule) |

---

## 1. What LQ-Bench is, and what this directory holds

LQ-Bench is the permanent accuracy benchmark of the query surface ([50 §7.4]). At M0 it runs as GT13 on LQ-3, the
reference model's own parser, binder and evaluator, through Claude Code in headless mode on Opus 5.5 ([90 §8.3],
[AR §7.7.5]). Its gates freeze grammar v1, the error texts, the lints and the card. This directory holds the committed,
synthetic part of the benchmark. Nothing in it is owner-derived.

| Path | Content | Written by |
|---|---|---|
| `README.md` | this file: the corpus format and the authoring rules | R-BENCH |
| `store-design.md` | the declarative design of the seeded fixture store: refs, trees, named referents with stable handles, filler, derived state at the fixture clock, totals | R-BENCH |
| `plan.md` | the allocation of tasks to strata, constructs and author batches; the stratified subsets | R-BENCH |
| `tasks/B1.jsonl` … `tasks/B5.jsonl` | the corpus, one file per author batch of [plan.md §2] | R-BENCH batch authors |
| `/private/lqbench/real.jsonl`, `/private/lqbench/referents.json` | the ≈ 30 real-session questions and their referent mapping (§9). **Gitignored; never committed, never on a hosted runner** ([AR §11] #36, #37) | R-BENCH with the owner |

The generator, the loader, the scorer and the harness live in the crate `moirai-lqbench` ([m0 PLAN §2.2], WP-70, WP-71a,
WP-71b). They read the files above; they never write into `fixtures/lqbench/`.

## 2. The prompt set

| Part | Tasks | Prompts | Source |
|---|---|---|---|
| Synthetic | 150 | 450 (3 phrasings each) | `tasks/B*.jsonl`, strata below |
| Real sessions | ≈ 30 | ≈ 30 (1 each, verbatim) | `/private/lqbench/` (§9) |
| Adversarial | 40 | 40 (1 each) | `tasks/B*.jsonl`, stratum `adversarial` |
| **Total** | 220 | **520** | [50 §7.4] item 2 |

The ten synthetic strata and their sizes are fixed by [50 §7.4] item 2:

| Stratum id | Name in [50 §7.4] | Tasks | Scored as |
|---|---|---|---|
| `lookup` | lookups | 15 | rows |
| `filter` | filters and ordering | 20 | rows |
| `traversal` | traversal and closure | 20 | rows |
| `derived` | derived state and blockers | 15 | rows |
| `aggregation` | aggregation and counting | 15 | rows |
| `search` | text search | 10 | rows |
| `links` | file links | 10 | rows |
| `history` | versions and history | 20 | rows |
| `merge` | conflicts and merge | 10 | rows |
| `write` | guarded writes | 15 | `DRY` diffs |

Real-session prompts form the stratum `real`; adversarial prompts form the stratum `adversarial`. The gate "no stratum
below 75 % after one retry" ([50 §7.4] item 6) reads the ten synthetic strata and `real`; the adversarial prompts are
reported per trap and enter the gates through the confident-wrong limits of their construct tags (open point 10).

## 3. Corpus file format

Each `tasks/B<n>.jsonl` file is UTF-8 without a BOM, LF line ends, one JSON object per line, no blank lines, ordered by
`id`. The files live under `fixtures/`, so `.gitattributes` treats them as binary (`fixtures/** -text`) and no tool
rewrites their line ends ([m0 PLAN §2.5]).

### 3.1 Fields

| Field | Type | Required | Meaning |
|---|---|---|---|
| `id` | string | yes | `T001`…`T150` for synthetic tasks, `A01`…`A40` for adversarial tasks. The ranges per batch are fixed in [plan.md §2]. |
| `stratum` | string | yes | one of the ten ids of §2, or `adversarial` |
| `base_stratum` | string | adversarial only | the synthetic stratum whose kind of question the adversarial task asks (for per-stratum analysis only; not a gate) |
| `constructs` | array of string | yes | 1–4 construct tags from §5.1, the primary construct first |
| `phrasings` | object | yes | synthetic: exactly the keys `literal`, `short`, `paraphrased` (§4); adversarial: exactly the key `literal` |
| `gold_query` | string | yes | the reference LQ text (§6): a read query, or for `write: true` one `TX { … }` block |
| `gold_result` | object | yes | the scored answer (§7) |
| `write` | bool | yes | `true` when the task asks for a change; its answer is scored as a `DRY` diff (§7.5) |
| `named` | object or null | yes | the standard named query that answers the task, when one exists: `{"name": "ready", "params": ["scope=240"]}` in the argv grammar of [50 §6.1]; `null` otherwise. Feeds the metric "named-query use where one exists" ([50 §7.4] items 4 and 6). |
| `surface` | string | no | `mcp` (default): the runner's two benchmark tools; `cli`: the runner offers Bash restricted to the `lqb q` / `lqb tx` shims ([50 §7.4] item 3 "or Bash with `moirai q`"). Only the argv traps use `cli` ([plan.md §3]). |
| `trap` | object | adversarial only | §5.2 |
| `notes` | string | yes | the author's reasoning for the gold result in one to five sentences, citing store-design sections (`SD §11.2`); may be empty only for lookups |

No other key is allowed. The loader rejects unknown keys, a missing required key, a handle not in the store design, a
construct tag not in §5.1, a trap id not in §5.2, a duplicate `id`, and a duplicate `gold_query` inside the corpus.

### 3.2 Placeholders

Some store values are not fixed by this design but by format chapters still being written or by the generator's seed:
commit ids (BLAKE3 over the canonical form, [AR §4.6]), store sequence numbers (the genesis offset, [store-design.md
§2.6]), lease ids (a store counter) and uids (random at create, [AR §3.1]). Phrasings and gold queries write them as
placeholders, and the harness substitutes them from the generator's manifest before a prompt is sent:

| Placeholder | Renders as | Example rendering |
|---|---|---|
| `{{commit:<commit handle>}}` | `c` + the first 8 lower-case hex digits of the commit id | `c41d7e0b` |
| `{{seq:<commit handle>}}` | the decimal store sequence number (`s` is written by the author: `USE s{{seq:c.sync-net}}`) | `92` |
| `{{lease:<lease handle>}}` | the lease id | `L-9` |
| `{{uid:<node handle>}}` | the node's uid as 32 lower-case hex digits (random for most kinds, derived for artifacts and the root node) | `018f3c2e7a117b3c9d5e4c2f1a0b9e77` |

Node ids (`#N`), anchor handles (`a1`…`a16`), ref names, paths, titles and field values are fixed by the store design and
are written literally; the generator asserts them (store-design §2.7). A placeholder never appears in `gold_result`, which
uses symbolic handles instead (§7.2).

### 3.3 Example lines

A synthetic read task (pretty-printed here; one line in the file):

```json
{"id":"T062","stratum":"traversal","constructs":["hop-bound","edge-direction-blocks","closure"],
 "phrasings":{"literal":"Which tasks must be finished at least two steps before #280 can start, counting only chains of direct prerequisites?",
              "short":"indirect prerequisites of #280",
              "paraphrased":"Before snapshot interpolation (#280) can begin, which work sits two or more dependency links upstream of it?"},
 "gold_query":"MATCH (x:task)-[:BLOCKS]->{2,}(#280) RETURN x",
 "gold_result":{"kind":"rows","columns":[{"name":"x","type":"node"}],"rows":[["@TC-1"]],"order":"set","view":"main",
                "notices":[]},
 "write":false,"named":null,
 "notes":"SD §6: TC-1 -> TC-2 -> TC-7 is a walk of length 2; TC-1 also blocks TC-7 directly (the shortcut), which does not remove it (walk semantics, [50 §3.7] rule 2)."}
```

An adversarial task (the literal phrasing only):

```json
{"id":"A17","stratum":"adversarial","base_stratum":"traversal","constructs":["edge-direction-blocks"],
 "phrasings":{"literal":"What does #277 have to wait for before work on it can start?"},
 "gold_query":"MATCH (b)-[:BLOCKS]->(#277) RETURN b",
 "gold_result":{"kind":"rows","columns":[{"name":"b","type":"node"}],"rows":[["@TC-3"]],"order":"set","view":"main"},
 "write":false,"named":{"name":"blockers","params":["id=277"]},
 "trap":{"id":"reversed-blocks","lure":"writes (#277)-[:BLOCKS]->(b), which lists what #277 blocks (nothing)",
         "trap_result":{"kind":"rows","columns":[{"name":"b","type":"node"}],"rows":[],"order":"set","view":"main"},
         "exposed_by":["reads-echo","N07"]},
 "notes":"SD §6: TC-3 BLOCKS TC-4 is the only BLOCKS in-edge of #277; #277 has no BLOCKS out-edge, so the reversed pattern returns 0 rows with N07."}
```

A write task:

```json
{"id":"T107","stratum":"write","constructs":["tx-guarded","field-filter"],
 "phrasings":{"literal":"Raise #279 to priority P1, but only if it is still open and still P3; change nothing else.",
              "short":"bump #279 to P1 if open and P3",
              "paraphrased":"If the bandwidth budget task #279 is untouched (open, P3), make it P1."},
 "gold_query":"TX { MATCH (t {id: #279}) WHERE t.status = 'open' AND t.priority = 3 EXPECT 1 SET t.priority = 1 }",
 "gold_result":{"kind":"diff","on":"main","targets":["@TC-6"],
                "changes":[{"change":"~","node":"@TC-6","aspect":"field","name":"priority","before":3,"after":1}]},
 "write":true,"named":null,
 "notes":"SD §5.5 and §11.1: TC-6 is open, P3 at NOW on main."}
```

## 4. Phrasing kinds

The three kinds are [50 §7.4] item 2's, which are Jackal's categories [14 §4.1]. The accuracy gates apply to `literal` and
`short`; `paraphrased` is scored and reported, and counts in the confident-wrong rates, but no accuracy gate reads it
([50 §7.4] item 6).

| Kind | Rule | Example (for the same task) |
|---|---|---|
| `literal` | One or two complete sentences in the owner's **domain words**, never LQ's ([50 §7.4] item 2): every referent is named (ids as `#N`, lanes as "the net lane" or `lane/net`, paths, titles in quotes when used), every condition is spelled out, and the expected ordering is stated when order matters ("most important first"). No LQ keyword, function, edge-type or property spelling in code form: not `BLOCKS`, `CHILD_OF`, `subtree(`, `t.unblocked`, `USE`, `->`, `#u:`. Domain verbs are fine ("blocks", "is a subtask of", "is waiting on"). | "List the tasks under #273 that nothing unfinished is waiting on, most important first." |
| `short` | At most 12 words, telegraphic, the same referents and conditions; may drop articles and verbs. | "unblocked tasks under #273 by priority" |
| `paraphrased` | A different sentence structure and different words for the same question, the same referents (ids stay; a title may replace an id only when the title is unique in the store design) and the same conditions. It must not be answerable differently from the literal phrasing. | "Within the netcode snapshots campaign (#273), which pieces of work are free of unfinished prerequisites? Put the most urgent first." |
| adversarial (`literal` key) | One sentence that asks the question the trap lures, in domain words, written the way an agent's orchestrator would ask it; it follows the `literal` rules but does not warn about the trap. | "What does #277 have to wait for before work on it can start?" |

All three phrasings of one task have the same gold result. A phrasing never contains the answer, a hint about the LQ
form, or text from the card's examples ([50 §7.2]). A phrasing may contain Cyrillic when its task carries the
`non-ascii` construct (§5.1); it is then written as a native Russian speaker would ask it, with the ids unchanged.

## 5. Construct tags and trap ids

### 5.1 Construct tags (closed vocabulary)

A construct tag names an LQ construct the task's correct answer depends on. [50 §7.4] items 4 and 6 report and gate the
confident-wrong rate **per construct tag** (≤ 5 %), so tags are coarse enough to collect samples ([plan.md §4] sets the
minimum count per tag). A task carries 1–4 tags, primary first; an adversarial task carries the tag of the construct its
trap targets first.

| Tag | Covers |
|---|---|
| `id-lookup` | `#N`, `{id: #N}`, lists of ids, `#u:` literals, `show` |
| `kind-label` | kind labels, label tests `x:a\|b`, alternation of kinds |
| `field-filter` | header and kind fields compared with `=`, `<>`, `IN`, enum coercion (`P1`, `'open'`), priority ranges |
| `set-field` | set-valued fields (`labels`, `files_owned`, `applies_to` globs): `'x' IN t.labels`, `any/all/none` |
| `text-predicate` | `CONTAINS`, `STARTS WITH`, `ENDS WITH`, `glob_match()` |
| `absent-value` | absent optional fields: `IS NULL`, `IS NOT NULL`, absent-value logic, `coalesce`, W01 |
| `ordering` | `ORDER BY` on enums by declared rank, priority (P0 first), multi-key orders, `LIMIT`, the total order |
| `time-provenance` | `created_at`, `updated_at`, `now()` with durations, `created_by`, `created_role` |
| `edge-direction-blocks` | the direction of `BLOCKS` / `BLOCKED_BY` |
| `edge-direction-child` | the direction of `CHILD_OF` / `PARENT_OF` / `HAS_SUBTASK` |
| `edge-direction-supersedes` | the direction of `SUPERSEDES` / `SUPERSEDED_BY` |
| `edge-knowledge` | `ABOUT`, `CITES`, `DERIVED_FROM`, `IMPLEMENTS`, `ANSWERS`, `REFUTES`, `CONFIRMS`, `VERIFIES`, `ADDRESSES`, `SCOPED_TO`, `DEPENDS_ON`, `DUPLICATE_OF`, `DISCOVERED_FROM`, `GATES` |
| `edge-other` | `MERGE_AFTER`, `RUNS_IN`, `PRODUCED`, `CONSUMED`, `MENTIONS`, and the symmetric `RELATES`, `CONTRADICTS` |
| `reverse-alias` | the answer is most naturally written with a reverse alias (`BLOCKED_BY`, `PARENT_OF`, `SUPERSEDED_BY`, `ANSWERED_BY`, …) |
| `multi-hop` | fixed patterns of two or more hops, branching patterns |
| `closure` | unbounded quantifiers `+`, `*`, `*1..` |
| `hop-bound` | hop bounds `{m,n}`, `{m,}`, `*m..n`, on DAGs with shortcut edges |
| `quantified-group` | quantified groups with a per-step `WHERE` |
| `subtree` | `subtree()`, `descendants()`, `ancestors()`, `children()` |
| `exists-optional` | `EXISTS {}`, `NOT EXISTS`, pattern predicates, `COUNT {}`, `OPTIONAL MATCH` |
| `derived-ready` | `t.ready`, `std.ready` (dispatchable now, tip only) |
| `derived-structural` | `unblocked`, `blocked`, `open_blockers`, `is_blocker`, rollups (`children_total`, `children_done`, `ready_to_close`), `suspect`, `conflicted`, `has_dangling`, `answered` |
| `derived-done` | the virtual `done` (done or cancelled) and `unfinished`; `status = 'done'` against `t.done` |
| `blockers-fn` | `CALL blockers(…)` with inherited, flagged and elsewhere rows |
| `runtime` | leases, `claimed`, `settled_elsewhere`, `deleted_elsewhere`, `leases()`, `markers()` |
| `applies` | `applies()`, `applies_role()`, `fits_role()`, `std.notes` |
| `count-bag` | `count(*)` over fixed patterns: one binding per match, anonymous elements included |
| `count-entity` | counting entities: `count(DISTINCT …)`, `RETURN DISTINCT`, existence tests that make the entity the unit (Q6) |
| `count-quantified` | aggregates over quantified parts: endpoint pairs, N08 |
| `group-having` | implicit grouping, `WITH … WHERE`, conditional aggregates with `CASE`, `collect()`, `GROUP BY` |
| `arith` | `/` (always a float), `round()`, `toInteger()`, division by zero |
| `set-op` | `UNION`, `UNION ALL`, `EXCEPT`, `INTERSECT` |
| `search` | `CALL search(…)`, `text_match()`, `find text=` |
| `non-ascii` | a Cyrillic literal in the gold query or a Cyrillic phrasing |
| `file-link` | `file()`, `AT` edge variables and anchor fields, `link_state()`, `f.state`, `a.state`, `links()`, `std.links_*` |
| `view-use` | `USE` with a ref, `~n`, `^n`, `s<seq>`, a commit, `@n`, `@datetime`; derived state at a past view |
| `history-fn` | `history()`, `blame()`, `log()`, `changes()` |
| `diff-range` | `diff()`, ranges `a..b` and `a...b`, `across()`, `refs()` |
| `tombstone-elsewhere` | `:DELETED` patterns, N01, ids created on another branch (N06) |
| `merge-state` | `conflicts()`, `violations()`, staging refs |
| `tx-guarded` | `MATCH … EXPECT n` targets, compare-and-set predicates, `ASSERT`, `IF TIP`, `IF TARGETS`, `LEASE`, `CALL tx.*` |
| `tx-structural` | `CREATE` (nodes, edges, `UNDER`, `UNLESS EXISTS`), edge `DELETE`, `MOVE`, `REOPEN`, node `DELETE … REPLACED BY`, `RESOLVE` |
| `named-query` | the task is answered by a standard named query (`named` is not null) |
| `surface` | the tool surface itself: argv forms (`k=v`, ranges `..1`, id lists, `#` in argv), `$parameters`, write keywords refused in `q` (E006) |

### 5.2 Trap ids (adversarial tasks)

The trap families are [50 §7.4] item 2's list. `trap` is an object:

| Key | Type | Meaning |
|---|---|---|
| `id` | string | one id from the table below |
| `lure` | string | one sentence: the wrong query or reading the phrasing invites |
| `trap_result` | object or null | the result the lured query returns, in the §7 encoding, or `null` when the lured query fails with an error |
| `exposed_by` | array of string | what the lured run prints that exposes the mistake: codes (`E118`, `W07`, `N06`, …), `reads-echo`, or `[]` when the fall is silent (a confident-wrong candidate) |

| Trap id | [50 §7.4] item 2 family | Primary construct |
|---|---|---|
| `reversed-blocks` | reversed `BLOCKS`, without an alias | `edge-direction-blocks` |
| `reversed-blocks-alias` | reversed `BLOCKS`, the phrasing invites `BLOCKED_BY` | `edge-direction-blocks` |
| `reversed-child-of` | reversed `CHILD_OF`, without an alias | `edge-direction-child` |
| `reversed-child-of-alias` | reversed `CHILD_OF`, the phrasing invites `PARENT_OF` | `edge-direction-child` |
| `reversed-supersedes` | reversed `SUPERSEDES`, without an alias | `edge-direction-supersedes` |
| `reversed-supersedes-alias` | reversed `SUPERSEDES`, the phrasing invites `SUPERSEDED_BY` | `edge-direction-supersedes` |
| `blocked-by-retry` | `BLOCKED_BY`-style retries (an agent that corrects a reversed hop by flipping the alias as well, landing on the same wrong direction) | `reverse-alias` |
| `count-anonymous` | counts over anonymous elements | `count-bag` |
| `count-quantified` | counts over quantified parts | `count-quantified` |
| `indirect-hop-shortcut` | "indirect" hop bounds on DAGs with shortcuts | `hop-bound` |
| `eq-null` | `= NULL` | `absent-value` |
| `labels-fn` | `labels()` used for task labels | `set-field` |
| `t-open` | `t.open` | `field-filter` |
| `done-cancelled` | `t.done` where cancelled tasks exist | `derived-done` |
| `priority-desc` | `priority DESC` for "most important first" | `ordering` |
| `hand-ready` | hand-written readiness | `derived-ready` |
| `id-other-branch` | ids created on another branch | `tombstone-elsewhere` |
| `runtime-past-view` | runtime state (`ready`, leases) at a past view | `view-use` |
| `link-past-view` | link states at a past view | `file-link` |
| `s1-variable` | `s1`-style variable names | `kind-label` |
| `ranges` | revision ranges `a..b`, `a...b` | `diff-range` |
| `hash-argv` | `#` in argv | `surface` |
| `write-in-q` | write keywords in `q` | `surface` |
| `int-division` | integer division | `arith` |

## 6. Gold queries

- A gold query is the reference answer the author wrote from the task text. It is **kept for reference; results are what
  is scored** ([50 §7.4] item 2). It must parse under grammar v1 ([50 §2.3]) and bind against the fixture schema; the
  WP-70 lint parses every gold query with LQ-3's parser once WP-93a exists and reports failures as findings against the
  corpus (never by editing the model).
- It is written in the display spelling the card uses today (GQL quantifiers, `->+`, `{2,}`), which the display-spelling
  ablation may change ([90 §8.1] L1); the result does not depend on the spelling, so a later change re-renders the gold
  queries mechanically and changes no gold result (HOLE(lqb-display-spelling)).
- It uses the fewest constructs that answer the task, the standard named query's text when `named` is set, parameters
  only when the task gives values that way, and placeholders (§3.2) for commit ids, sequence numbers and lease ids.
- It carries no `USE` when the task asks about the default view (`main`, §8). A task that names a lane, a tag, a past
  point or a range puts that in the query (`USE lane/net`, `USE main~5`, `CALL log(main..lane/net)`).
- For a write, it is one `TX { … }` block without `DRY` (the harness applies `DRY`, §7.5), with the `ON` branch when the
  task names one, and with `LEASE '{{lease:…}}'` when the task presents a lease.

## 7. Gold results

### 7.1 Object shape

| Key | Used by | Meaning |
|---|---|---|
| `kind` | all | `rows` (a read result), `diff` (a write, scored as its `DRY` diff) or `error` (the correct outcome is a refusal; §7.6) |
| `columns` | `rows` | array of `{"name", "type"}` in row order. Names are informative only; the scorer matches columns by value (§7.4). Types: `node`, `edge`, `anchor`, `int`, `float`, `text`, `enum`, `priority`, `bool`, `rev`, `commit`, `ref`, `timestamp`, `duration`, `list` |
| `rows` | `rows` | array of arrays of values (§7.2), in the order `order` requires |
| `order` | `rows` | `"set"`, `"bag"`, `"list"`, or `{"by": [column names], "dir": ["asc"\|"desc", …]}` (§7.3) |
| `view` | `rows`, `error` | the view the gold was read from (`main`, `lane/net`, `main~5`, `tags/m1`, `main...lane/audio`, …); documentation for reviewers, not scored |
| `notices` | `rows` | codes the correct query's output carries (`N06`, `N08`, `W03`, `W06`, …); informative, not scored |
| `tolerance` | `rows` | absolute tolerance for `float` columns; default `1e-9`; a task that asks for a rounded value states the rounding and uses `0` |
| `on`, `targets`, `changes`, `created` | `diff` | §7.5 |
| `code` | `error` | §7.6 |

### 7.2 Values

| Type | Encoding | Example |
|---|---|---|
| `node` | `"@<handle>"` for a named referent of the store design; `"@F<k>"` for filler node with filler ordinal `k` (store-design §12); `"@new<n>"` for the n-th node a write creates (`@new1` is `#2001`, store-design §2.5) | `"@TC-3"`, `"@F417"` |
| `edge` | `{"src": node, "type": "<stored LQ type>", "dst": node}`, always in the stored direction whatever alias the query used; `AT` edges add `"anchor": "@a<n>"` | `{"src":"@TC-3","type":"BLOCKS","dst":"@TC-4"}` |
| `anchor` | `"@a<n>"` (store-design §9.4) | `"@a7"` |
| `int`, `float`, `bool`, `text` | JSON number, boolean, string; text exactly as stored (Cyrillic as UTF-8) | `3`, `0.25`, `true` |
| `enum` | the value's name | `"in_progress"`, `"critical"`, `"stale-anchor"` |
| `priority` | the integer 0–4 (`P0` is `0`) | `1` |
| `rev`, `commit` | `"@c.<commit handle>"` (store-design §7) | `"@c.sync-net"` |
| `ref` | the ref name | `"lane/net"` |
| `timestamp` | ISO 8601 UTC with seconds and `Z` | `"2026-06-12T10:00:00Z"` |
| `duration` | seconds as an integer | `86400` |
| `list` | a JSON array of values, in the natural order of [50 §3.5] | `["@TA-1","@TA-3"]` |
| absent | `null` | `null` |

### 7.3 Order and multiplicity

| `order` | The agent's rows match the gold rows when … |
|---|---|
| `"set"` | the distinct row sets are equal; duplicates in the agent's result are ignored |
| `"bag"` | the multisets are equal: every row occurs as often as in the gold (bag semantics, [50 §3.4]; used by counting tasks) |
| `"list"` | the sequences are equal, row by row |
| `{"by": [...], "dir": [...]}` | the multisets are equal and the agent's rows are non-decreasing (or non-increasing for `desc`) by the named columns; ties may come in any order. Used whenever the question fixes an order that leaves ties, such as "most important first", because the engine's total-order tie-breaks (`topo`, binding identity) are not part of what the task asks. |

A task that asks for "the first n" rows must have no tie across the cut at position n under its `by` keys; the author
checks this against the store design.

### 7.4 How the scorer compares a read

The scorer (WP-70) compares the result of the agent's **scored call** with the gold:

1. The scored call for first-try accuracy is the agent's first query call; for accuracy after one retry, the first call
   after the first call that failed (non-zero exit) or that the agent itself followed with another query; for the final
   answer, the last query call that exited 0 ([50 §7.4] item 4).
2. Every gold column must be matched by a distinct column of the agent's result whose values, after the row matching of
   §7.3, are equal. Extra agent columns are ignored. A node-shaped result (a node object or a node line) supplies the
   node's id and every projected property as candidate columns, so `RETURN t` matches a gold `node` column and a gold
   `priority` column at once.
3. Values compare by type: nodes by id after mapping handles through the generator's manifest; floats within
   `tolerance`; enums by name; priorities as integers; timestamps as instants; absent equals only absent.
4. A result that exited 10 (a budget or cursor cut) is incomplete and never matches unless the gold has at most the rows
   the agent received and the task asked only for a prefix.
5. **Confident-wrong** is a scored call that exited 0, printed no warning, notice or reading echo that contradicts the
   question, and does not match; **hedged-wrong** is a non-matching call whose output carries such a signal ([50 §7.4]
   item 4). The scorer decides contradiction mechanically: any of W01, W03, W06, W07, N01, N06, N07, N08, N09, N10 or a
   `reads:` line whose reading differs from the gold query's reading counts as a signal.

### 7.5 Writes

A write task is scored on the `DRY` diff of the agent's final `TX` block, evaluated on a **pristine copy of the fixture
store** (every prompt starts from the same generated store, so write tasks are independent of each other and of their
order). The harness runs the agent's block with `DRY` whatever the agent wrote; nothing is committed.

| Key | Meaning |
|---|---|
| `on` | the branch the block must commit on (`main` unless the task names another) |
| `targets` | the set of existing nodes the correct write touches directly (its `MATCH … EXPECT` bindings and literal targets) |
| `created` | optional: `{"@new1": {"kind": "finding", "title": "…"}, …}` for nodes the write creates, in creation order |
| `changes` | the set of **primary** diff rows of the correct write: `{"change": "+"\|"-"\|"~", "node": node, "aspect": "exists"\|"field"\|"status"\|"body"\|"parent"\|"edge", "name": field name or edge `"<TYPE>-><node>"`, "before": value, "after": value}` |

The scorer compares the agent's `DRY` diff restricted to primary rows with `changes` as a set. **Implied rows** — those
the engine adds for a primary change: `resolution` with a status change, rollup and derived-counter changes, `affected`,
markers, `reopen_count` with `REOPEN`, the re-pointed or flagged edges of a node `DELETE` when the gold lists the `DELETE`
itself — are ignored on both sides; the list of implied aspects is the scorer's, one table, reviewed with the corpus. A
diff that touches a node outside `targets ∪ created` counts as "a write outside the gold target set" ([50 §7.4] item 4)
even when it also contains every primary row. A refused block (exit 4, 5, 6) matches only a gold of `kind: "error"`.

The gold diff is written by hand from the task text and the store design (S6). When LQ-3's `DRY` of the gold query
disagrees with the hand-written diff, the disagreement is a review finding against the corpus or against the model; it is
never resolved by copying the model's output into the gold.

### 7.6 Errors as the correct outcome

`{"kind": "error", "code": "E302", "view": "main~20"}` is used only when the task asks for something the language
correctly refuses and no rewrite answers the question as asked — for example a link state at a past view, which [50 §3.8]
makes E302 rather than a value. A run matches when its final call fails with that code. Adversarial tasks prefer a
question that has an answer (for "was #278 ready at `tags/m1`?" the answer is the structural `unblocked` value, and E302
on `ready` is the loud retry signal), so `error` golds stay rare: at most four in the corpus ([plan.md §3]).

## 8. The caller context of every prompt

The harness gives every prompt the same context, so that a gold result read from the store design is the answer:

| Item | Value |
|---|---|
| Store | a fresh copy of the store the generator builds from store-design.md (seed and schedule fixed) |
| Clock | `now()` is `2026-06-15T09:00:00Z` for every evaluation at a tip (store-design §2.1); the harness pins the model's wall clock |
| Branch | the caller's branch is `main`; no `--branch` is passed; a `USE` or `TX ON` in the agent's query selects any other view |
| Caller | agent label `bench`, role `orchestrator` for writes ([50 §6.5]), no lease unless the task presents one (`LEASE '{{lease:…}}'`) |
| Write path | `moirai tx` semantics: every `TX` statement the orchestrator row of [50 §6.5] allows, node `DELETE` and `RESOLVE` included; the MCP `write` tool's extra refusals ([50 §6.3]) belong to the transport arms, not to the capability measure |
| Trees | each work branch resolves its designated tree by its binding (store-design §3.4): `main` → `T-main`, `lane/audio` → `T-audio`, and so on |
| Budgets | the defaults of [50 §5.10]; tasks are designed to fit them |
| Tools | the card as system text; `moirai_q` (text + params) and `moirai_named` (name + params); `surface: cli` tasks add Bash restricted to `lqb q` and `lqb tx` ([90 §8.3], WP-71b) |

## 9. The real-session stratum (private)

The ≈ 30 real-session questions are the owner's words ([50 §7.4] item 2; [AR §11] #36–#38). They are kept **verbatim**
in `/private/lqbench/real.jsonl`, which the `.gitignore` excludes and `xtask private index` manifests. Only their
aggregate scores are committed.

- **Record.** The same fields as §3.1 with `id` `R01`…`R30`, `stratum` `real`, and `phrasings` with the single key
  `verbatim`. The text keeps every word the owner wrote. Its **referent spans** — ids, lane and branch names, paths, round
  numbers, role names — are marked as `[[r:<n>|<original text>]]` in a parallel field `marked`, which only the loader
  reads.
- **Referent mapping (the resolution of gap g-10, [m0 PLAN §3.3]).** `/private/lqbench/referents.json` maps every marked
  span to a store-design referent: `{"R07": {"1": {"handle": "@LN-net", "surface": "lane/net"}, "2": {"handle": "@DA-plan",
  "surface": "#254"}}}`. At run time the loader substitutes each marked span with its `surface` form and sends the result;
  every other character is the owner's. The substitution is recorded with the result. A question whose referent has no
  fitting counterpart in the store design is either mapped to one of the real-session support clusters of store-design
  §5.10, or dropped from the stratum with a reason the owner confirms (V6); the store design is never extended with
  owner-derived content.
- **Gold.** R-BENCH writes the gold query and gold result from the substituted text and the store design (S6), in the §7
  encoding. The owner verifies a sample of the real-session golds with the synthetic sample (V3).
- **Where they go.** Only to Anthropic, through Claude Code in headless mode ([90 §8.3]); never in the public repository,
  a hosted runner or a transcript that leaves `/private/`.

## 10. Subsets and the shrink rule

Every run uses the full 520 prompts for the baseline and the two transport arms. The ablations, the repeated sample and
the transport arm take the deterministic subsets of [plan.md §6]: the stratified 260-prompt half, the 130-prompt quarter,
the 370-prompt set without paraphrases, the repeated 52-prompt sample and the 20 transport prompts. If the subscription
quota cannot carry a run, the prompt set shrinks by [50 §7.4] item 5's documented rule, step by step, stopping at the first
step that fits:

1. the non-gate ablations of [50 §7.4] item 7 (every one but D8, D11 and the display spelling) run on the quarter instead
   of the half;
2. the two alternative surfaces and the display-spelling ablation move to the half;
3. the gate-deciding ablations D8 and D11 drop the paraphrased phrasing (the 370-prompt set).

The baseline's 520 prompts and the two transport arms are never cut, and no gate is skipped: each gate is reported with
its sample size and the 95 % interval of its estimate.

## 11. Authoring rules

1. **Separation (S6).** Batch authors are R-BENCH sessions. They read the design documents, this README, the store
   design and the plan. They do not read `moirai-model` or any product crate, and they never run the model to obtain a
   gold result.
2. **Synthetic only.** No owner-derived text, path, name or prompt. Every referent is a store-design handle.
3. **Card-disjoint.** The ids of the card's examples (`#51`, `#88`, `#89`, `#93`, `#130`) are filler in the store design,
   and `lane/l5np` does not exist; no task cites them, so no example of [50 §7.2] answers a task by copying.
4. **Only designed facts.** A gold result may depend only on facts store-design.md states. Store-design §16 lists the facts
   that depend on engine choices not yet frozen; no task may depend on them.
5. **Unambiguous questions.** Each phrasing has exactly one correct answer under [50 §3]. When two readings of the domain
   words are plausible, the literal phrasing spells out the intended one.
6. **Small answers.** A gold result has at most 20 rows; larger sets are asked as counts. Global counts over filler use
   store-design §13.
7. **One task, one question.** No task asks two questions; composite answers (`UNION`) are one question about two views.
8. **Self-check.** Before a batch is handed over, its author recomputes every gold result from the store design a second
   time and records the section references in `notes`.

## Holes

| Id | What | Decided by | Candidates | Constraint the value must meet |
|---|---|---|---|---|
| HOLE(lqb-display-spelling) | the quantifier spelling of every gold query and of every phrasing-independent example in this file | the display-spelling ablation of [50 §7.4] item 7 in GT13 (WP-72; [90 §8.1] L1) | Cypher (`-[:T*1..]->`, `*2..`) or GQL (`-[:T]->+`, `{2,}`) | a change re-renders gold queries mechanically and changes no gold result; the canonical form and hashes are unaffected ([50 §5.3]) |
| HOLE(lqb-search-scorer) | the ranking function behind `search()` | the BM25 against statistics-free ablation (WP-72, [74 A15]) | BM25 with `DOCLEN`; a statistics-free scorer | search golds are sets or single dominant matches (store-design §14), so neither candidate changes a gold result |

## Open points for the review

1. **Phrasing key `paraphrased`.** The work-package brief named the third phrasing `natural`; [50 §7.4] item 2 calls it
   "paraphrased", and the brief asked for [50]'s kinds, so the key is `paraphrased`.
2. **Adversarial prompts have one phrasing** under the key `literal`, stratum `adversarial` and an added `base_stratum`,
   because [50 §7.4] item 2 counts them as 40 prompts (450 + 30 + 40 = 520).
3. **Keys added to the brief's format:** `named` (the named-query metric needs to know where one exists), `surface`
   (the `#`-in-argv trap cannot be exercised through MCP strings, [50 §6.1]), `base_stratum`, and the structured `trap`
   object (`id`, `lure`, `trap_result`, `exposed_by`) so that the scorer can separate a fall into the trap from other
   errors.
4. **Placeholders** for commit ids, sequence numbers, lease ids and uids (§3.2), because those values are fixed by format
   chapters (06, 11, 12), the store counter or the generator's seed, not by this design. Node ids and anchor numbers are
   fixed here and asserted by the generator.
5. **Caller context (§8)** is a requirement on WP-71b: a pinned clock, the `main` default, the orchestrator role for
   writes, per-branch tree bindings and a pristine store per prompt. Without the pinned clock `defer_until` and every
   `now()`-relative gold would drift.
6. **Write scoring (§7.5)** ignores implied rows and compares primary rows as a set; the implied-aspect list is the
   scorer's single table, reviewed with the corpus.
7. **Real-session referents (§9, gap g-10):** verbatim words with marked referent spans substituted at run time; unmapped
   questions are mapped to a support cluster or dropped with the owner's confirmation, never answered with owner data in
   the store.
8. **Named queries available at M0** are WP-19's (gap g-9). A `named` entry names only a query of [50 §4.1]; if WP-19
   leaves one out of M0, the loader clears the entry and the task counts as "no named query exists".
9. **Scored call selection (§7.4 rule 1)** is this file's reading of "first-try" and "after one retry" for a 3-turn agent
   run; the review confirms it or replaces it before WP-72.
10. **Which strata the 75 % floor reads.** [50 §7.4] item 2 defines the ten synthetic strata and lists the real sessions
    and the adversarial tasks beside them; this file applies the floor to the ten and `real`, because the adversarial
    set is built to lure mistakes and has its own per-construct gate. If the review reads item 6 as covering the
    adversarial set too, the floor is applied to it unchanged; the corpus does not change.
11. **Write path (§8).** The capability runner applies `TX` blocks with `moirai tx` semantics and the orchestrator role,
    so node `DELETE`, `RESOLVE` and bulk targets are measurable; the MCP `write` tool refuses those ([50 §6.3]) and is
    exercised only by the transport arms.
