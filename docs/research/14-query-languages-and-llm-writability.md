# 14 — Query languages for moirai, and how well LLM agents write them

*Research report for owner requirement **R5** (2026-09-26): "there must also be a query language for our graph DB". Lens: the query-language landscape and the evidence on how well LLM agents write each kind of language. Written 2026-09-26. Nothing in moirai was implemented. The only probes run were small ones (token counts, a shell-quoting check, a Stack Overflow tag count). The probe scripts and their output (`queries.py`, `measure.py`, `results.json`) are not published.*

---

## 0. Conventions

| Tag | Meaning |
|---|---|
| **[M]** | Measured in this session by the probes listed above. The method is described where the number appears. |
| **[D]** | Documented: a specification, official docs, source code, or a peer-reviewed or arXiv paper's own reported numbers. |
| **[C]** | A third-party claim: a secondary write-up, vendor marketing, or a paper I could only read through a summary. |
| **[I]** | My inference or recommendation. |

"EX" means *execution accuracy*: the query's result set equals the gold result set. Numbers in the text are exactly as the sources report them. The source URL is either inline or in §12.

---

## 1. Executive summary

1. **R5 reverses one earlier conclusion, and the evidence supports reversing it.** Reports 06 §12.2 and 30 §12 ruled out a query language for v1 on the grounds of "~50 % Text2Cypher execution accuracy for GPT-4". Two corrections to that argument:
   - **The citation was wrong.** The 49.07 % / 50.42 % figures come from SyntheT2C (arXiv 2406.10710: GPT-4 on a medical knowledge graph, precision-style metric), not from arXiv 2412.10064 [D].
   - **The figure measures the wrong task.** Benchmarks turn *ambiguous human questions* over *unfamiliar schemas with entity linking* into queries. A moirai agent writes a query for *its own precise intent*, over a *small, fixed, documented* schema, using ids it already holds (`#40`).
   
   The closer analogues are much better. On Text-to-JQL (Jira's issue-filter language), when the request spells out the query literally ("semantically exact"), EX is **0.915–0.993 for every frontier model, Claude included** [D Jackal]. On simple-retrieval Cypher, strong open models reach **86–93 % EX** zero-shot [D Mind the Query]. When a strong model fails, it is mostly *semantic*: reversed edge direction, schema linking, aggregation. It is rarely *syntactic* [D CypherBench, Text2GQL-Bench, Jackal] [I].
2. **Claude is reported separately in several recent benchmarks, and it is strong on graph query languages.**
   - CypherBench: Claude 3.5 Sonnet had the best EX, 61.58 % [D].
   - Text2GQL-Bench (Feb 2026), zero-shot: Claude Opus 4.5 reached 44.5 % EX in **ISO GQL** and 43.8 % in Cypher. GPT-5.2 reached 1.7 % in GQL and 44.0 % in Cypher. With 3 examples, all frontier models reached about 48–50 % [D].
   - Claude therefore transfers its Cypher knowledge to GQL without examples. Other models need a few examples first [D→I].
3. **Retry is what rescues a failed query, not clever feedback.** LAST-CQ (Sept 2026, Claude Sonnet 4 among six backbones) found three things [D]:
   - Detecting a failure and routing it to a retry recovers **91.7 %** of single-pass failures.
   - Raw database error strings work about as well as LLM-synthesised feedback (20.9 % vs 19.9 % exact match).
   - Spending the same call budget on parallel sampling makes results worse.
   
   A validation gate catches **100 %** of parse, constraint and schema-reference errors before execution. It catches **0 %** of "valid but wrong property" swaps [D CYGNET]. For moirai: a strict schema, loud and precise errors, and one retry are worth more than grammar sophistication [I].
4. **The language family that fits best is the GQL / Cypher-25 pattern family, as a documented subset with moirai sugar. It is not a new DSL, not Datalog, and not JSON.**
   - Cypher is by far the most represented graph query language in public training data: 9,920 SO questions tagged `cypher` and 23,006 tagged `neo4j`, against 3,635 `gremlin`, 436 `datomic`, 167 `datalog` and 1 `prql` [M].
   - GQL became an ISO standard in 2024. Neo4j (Cypher 25), Google Spanner Graph and Microsoft Fabric have adopted it [D].
   - Its ASCII-art patterns are readable to humans.
   - It is the only candidate family that handles all six moirai tasks without contortions, including multi-hop joins and aggregation (§4) [I].
5. **The syntaxes are not close on length** (proxy token counts, §4.3 [M]):
   - the purpose-built CLI verbs ≈ 13 tokens per query;
   - revset or pipeline DSLs ≈ 16;
   - the proposed GQL hybrid ≈ 36;
   - plain ISO GQL ≈ 52, Cypher ≈ 54, a JSON AST ≈ 48, Datalog ≈ 90.
   
   Tens of tokens per query are negligible next to tool output (the 25k-token cap) and next to the cost of one failed call. Correctness dominates, so token count is a secondary criterion [I].
6. **The shape of the recommendation [I]: "moirai QL" = GQL-core + moirai sugar + a git/jj revision grammar.**
   - A read-first subset of GQL that also accepts Cypher spellings: `MATCH` patterns, quantified paths, `FILTER`/`WHERE`, `RETURN … ORDER BY … LIMIT`, aggregation, `EXISTS {}`, `$params`, `CALL … YIELD`, `USE`.
   - moirai sugar:
     - `#N` node literals;
     - engine-derived state exposed as built-ins (`t.ready`, `t.open`, `blockers(#40)`, `r.applies('crates/ecs/**')`), so a query and a verb can never disagree;
     - edge names that read in their stored direction;
     - labels and types matched case-insensitively.
   - A revision grammar borrowed from git/jj for versioning: `USE lane/X`, `USE c812`, `changes(n, main..lane/X)`.
   - One guarded write form, `MATCH … FILTER … SET`, executed as a single commit under the writer byte.
   - The existing verbs and GitHub-style filters stay as the fast path. Each verb is documented as a named query, and the two are tested for equality.
7. **The standard has sharp edges, and LLMs hit them.**
   - In strict GQL, every unbounded quantifier must sit inside a restrictor (`TRAIL`/`ACYCLIC`/`SIMPLE`) or a selector (`ANY`/`SHORTEST`) [D GPML paper]. Cypher 25 needs neither.
   - While writing the queries for this report, I (a Claude model) left that restrictor out of 3 of the 7 GQL queries on the first draft [M, n = 1, anecdotal].
   - moirai's invariants already make `child_of` a forest and `blocks` a DAG (I4, I5′), so the engine can default the restrictor for acyclic edge kinds. It should demand one only for cyclic kinds (`relates`, `mentions`) [I].
   - The Text2GQL zero-shot failures were 85 % syntax errors [D]. A parser that accepts Cypher spellings (`WHERE`, `*1..`, `WITH`, `collect()`) removes most of that failure class [I].
8. **Side finding: the CLI examples in the synthesis are broken as written [M].** In both Git-Bash and Windows PowerShell 5.1, an unquoted `#40` starts a *comment*. So `moirai blockers #40 --transitive` runs as `moirai blockers`, and bash also drops everything after `#40` on that line. The CLI must accept bare `40`, and the skill must never show an unquoted `#N` in a shell example. The MCP path is unaffected.
9. **Must-have and later.**
   - **Must-have:** the core read subset; quantified paths over one edge kind, including the per-step `WHERE`; built-ins; `EXISTS`; aggregation; parameters; `USE`/`changes()`; guarded `SET`; `EXPLAIN`; schema introspection; strict schema errors; budgets; the same output contract as the verbs.
   - **Later:** `OPTIONAL MATCH`, `LET`/`NEXT`/`WITH` chaining, `CASE`/`WHEN`, `UNION`, `INSERT`/`DELETE`/link through the query language, shortest paths, saved queries and aliases, multi-branch (`--across`) queries, standing incremental queries.
   - **Never:** a procedure zoo like APOC, arbitrary user code in queries, a second full language (Datalog, Gremlin or SPARQL) (§7).

---

## 2. What R5 changes, and how it relates to earlier decisions

- **Earlier position.** Report 06 §12.2 recommended "commands plus the filter syntax" for v1 and "a read-only Datalog or GQL subset later as a power tool". Report 30 put "a query language (Cypher/GQL/Datalog) in v1" on its anti-requirement list (30 §12), citing ~50 % accuracy.
- **R5 overrides that position.** Nothing in the evidence below argues for dropping the verbs or the filter syntax. Agents should still reach for `ready`, `show`, `blockers`, `notes --path` and `brief` first: they are the cheapest calls and carry exactly the engine's semantics. The query language is the tool for *everything else*: ad-hoc multi-hop questions, aggregations for loop termination and refuted share, cross-cutting audits, and history questions.
- **Citation correction.** 06 §12.2 attributes "GPT-4 execution accuracy 49.07 % zero-shot and 50.42 % 2-shot" to arXiv 2412.10064 (Neo4j's Text2Cypher dataset paper). That paper contains neither number [D]. The figures are from **SyntheT2C**, arXiv 2406.10710, §4.3.1: "GPT-4 achieves the averaged Execution Result Accuracy of 49.07% (zero-shot) and 50.42% (2-shot)". They were measured on a medical knowledge-graph evaluation set, with accuracy defined as |gen ∩ gt| / |gen| [D].
- **Interfaces stay as they are.** The CLI → MCP → skills stack from 30 §7 is unchanged. R5 adds one CLI verb (`moirai q`), one deferred MCP tool (`query`) and one skill card (§6.5).

---

## 3. Landscape survey (state as of 2025–2026)

### 3.1 Comparison table

The SO column is the number of Stack Overflow questions carrying the tag, taken from the Stack Exchange API on 2026-09-26 [M]. It is a rough proxy for public training data. It lags reality, because Stack Overflow traffic has fallen since 2023.

| Language | Recursion / variable-length paths | Aggregation | Mutation | Parameters | Subqueries | Missing values | Versions / history | 2026 status | SO tag count [M] |
|---|---|---|---|---|---|---|---|---|---|
| **ISO GQL** (39075:2024) | Quantified path patterns `->{m,n}`, `+`, `*`. Path modes WALK / TRAIL / SIMPLE / ACYCLIC. Selectors ANY, ANY SHORTEST, ALL SHORTEST, (Spanner) ANY CHEAPEST. Unbounded quantifiers require a restrictor or selector [D] | `GROUP BY`, `count`, `collect_list`… plus horizontal aggregation over group variables | `INSERT`, `SET`, `REMOVE`, `DELETE` / `DETACH DELETE` | `$p` | `EXISTS {}`, `CALL {}` / procedure `CALL … YIELD`, linear composition via `NEXT`, plus `LET`, `FILTER`, `FOR` | SQL-style 3-valued logic (UNKNOWN); `IS NULL`, `coalesce` [D Fabric guide] | `USE <graph>` selects a graph from a catalog; no native history | Published April 2024. Neo4j, Spanner Graph and Microsoft Fabric implement it. There is no official conformance test: the independent gql-compat suite counts **814 grammar productions** and **228 optional features** [C] | `gql`: 542 (probably dominated by the unrelated Python GraphQL client `gql`) [I] |
| **SQL/PGQ** (SQL:2023 part 16) | `GRAPH_TABLE (g MATCH … COLUMNS …)`, with the same pattern language as GQL | the surrounding SQL | only through SQL on the underlying tables | SQL | SQL | SQL NULL | none | Oracle 23ai and DuckPGQ ship it. **The PostgreSQL 19 implementation was reverted on 2026-09-07** (commit b1f106c8…); it had covered only fixed-depth patterns [D] | — |
| **openCypher / Neo4j Cypher 25** | `-[:T*1..]->` (classic); quantified path patterns `((a)-[:T]->(b) WHERE …)+` (Cypher 5+); match modes `DIFFERENT RELATIONSHIPS` (the default, i.e. trail semantics), `REPEATABLE ELEMENTS`, `ACYCLIC` (2026.03); `SHORTEST k` [D] | implicit grouping, plus explicit `GROUP BY` since 2026.07 [D] | `CREATE`, `MERGE`, `SET`, `REMOVE`, `DELETE` | `$p` | `EXISTS {}`, `COUNT {}`, `COLLECT {}`, `CALL (x) {}`, plus `LET`, `FILTER`, `NEXT`, `WHEN … ELSE` since 2025.06 [D] | 3-valued logic; an unknown property silently reads as `null` | none (composite `USE db`) | Cypher 25 tracks GQL; "most mandatory GQL features" are supported as of Neo4j 2026.06 [D]. Kùzu was archived on 2025-10-10 and forked as LadybugDB [D/C] | `cypher`: 9,920 · `neo4j`: 23,006 |
| **Gremlin** (TinkerPop) | `repeat().until()/emit()/times()` | `group().by()`, `count()` | `addV`, `addE`, `property`, `drop`, `mergeV`/`mergeE` | bindings | nested traversals `__.` | nulls allowed in 3.5+ | none | 4.0.0-beta.3 (2026-07). 4.0 drops bytecode in favour of `gremlin-lang` scripts and HTTP [D] | 3,635 |
| **SPARQL 1.1 / 1.2** | property paths `+ * ? ^ / \|`. **A path cannot constrain its intermediate nodes** [D] | `GROUP BY`, `HAVING` | SPARQL Update: `INSERT` / `DELETE … WHERE` | `VALUES`, `BIND` | nested `SELECT`, `(NOT) EXISTS`, `MINUS` | unbound variables; an error inside `FILTER` makes it false | none (named graphs only) | 1.1 is a Recommendation (2013). 1.2 Query is a Working Draft (2026-09-21) that adds triple terms and `VERSION` [D] | 6,224 |
| **Datomic Datalog** | recursive rules | aggregates in `:find`, `:with` | transaction data, **`:db/cas`** built in | `:in $ ?x` | rules, `not-join`, `or-join` | no null (datom absent) | **`as-of`, `since`, `history` are native** [D] | proprietary, free binaries | 436 |
| **XTDB v2** | SQL recursive CTEs; XTQL | SQL | SQL (XTQL DML was removed for lack of uptake [C]) | SQL | SQL | SQL | **bitemporal by default** (`FOR VALID_TIME AS OF …`) [D] | v2.x maintained; SQL is primary [C] | — |
| **CozoScript** | recursive rules with stratified negation; fixed-rule algorithms (`<~ ShortestPathBFS`) | aggregation in rule heads | `:put`, `:rm`, `:update` | `$x` | rules | nulls allowed | time travel on relations (validity) | **dormant: last release v0.7.6, 2023-12-11** [D] | — |
| **Soufflé** | recursive, stratified | `count : {…}` | none (a batch analytics engine) | `.input` | components | no null | none | compiled to C++; used for static analysis | (in `datalog`) |
| **Logica** (Google) | recursion (`@Recursive`) | `+= 1`-style aggregation heads | none (compiles to SQL) | — | predicates | SQL | none | active; targets DuckDB, BigQuery, Postgres, SQLite. Google's *LogicLM* has the LLM emit a **JSON configuration** over Logica predicates rather than raw Logica [D] | — |
| **Datalevin** | Datomic-style rules | as Datomic | transactions | `:in` | rules | — | none | active; cost-based optimiser, claimed 2.4× faster than PostgreSQL on complex joins [C] | (in `datalog`) |
| **CodeQL (QL)** | `p+` / `p*` transitive-closure operators on predicates [D] | yes | none | — | predicates, classes | — | none | active | — |
| **EdgeQL** (Gel) | **none: no recursive traversal** (long-standing issue #4168) [D] | `group`, `count` | `insert`, `update … filter … set` | `<type>$x` | shapes, `with`, `for` | **no NULL: the empty set `{}`** [C] | none | **Gel Data Inc. shut down 2025-12-02; the team joined Vercel.** Code remains open source [D] | 35 |
| **TypeQL 3** (TypeDB 3.x) | recursive functions (these replaced rules) [D] | `reduce` | `insert`, `put`, `update`, `delete` | — | functions and pipelines (`match`, `fetch`, `reduce`, `sort`, `limit`) | no null (attribute absent) | none | active | 132 |
| **SurrealQL** | `->edge->table` arrows; recursion `.{1..3}(->e->t)` since 2.1; `+path`, `+collect`, `+shortest` since 2.2 [D] | SQL-like | `UPDATE … WHERE`, `RELATE` | `$x` | subqueries | `NONE` / `NULL` | none | active | 111 |
| **HelixQL** (HelixDB) | traversal steps | — | `AddN`, `AddE` | typed parameters | — | — | none | **HelixQL v1 is archived. v3 requests are operation trees (a JSON AST) built with Rust / TypeScript / Go / Python SDK DSLs and POSTed to `/v2/query`** [D] | — |
| **PRQL** / SQL pipe syntax | `loop` (experimental; compiles to `WITH RECURSIVE`) [D] | `group {…} (aggregate …)` | none | — | pipelines | SQL | none | PRQL active but niche. GoogleSQL pipe `\|>` is in BigQuery; its LLM-friendliness is claimed without numbers [C] | `prql`: **1** |
| **jq 1.8 / JMESPath** | jq: `recurse`, `..`, `paths`. JMESPath: no recursion | `group_by`, `length` | jq only (pure transforms) | `--arg` | pipes | `null` propagates | none | jq 1.8.2 (2026-06-20) [D]. JMESPath is used by the AWS and Azure CLIs' `--query` | jq: 6,922 · jmespath: 543 |
| **GraphQL** | **none**: depth is fixed by the selection shape | server-defined | named mutation fields | `$var` | fragments | nullable by default, null bubbling | none | spec edition September 2025 [D] | 20,774 |
| **git revisions** | `A..B`, `A...B`, `^`, `~n`, `^@`, `^!`, `:/text`, `@{u}`, `ref@{date}` [D] | — | — | — | — | — | *this is the versioning language* | ubiquitous | (in `git`) |
| **Jujutsu revsets and templates** | `::x`, `x::`, `x..y`, `ancestors(x, depth)`, `descendants()`, `heads()`, `roots()`, `connected()`, `reachable(srcs, domain)`, `latest()`; set operators `& \| ~`; string patterns `glob:` / `exact:` / `regex:` / `substring:`; `[revset-aliases]` with parameters [D]. Templates are a typed expression language (`x.method()`, `++`, `if()`, lambdas `map(\|c\| …)`, `json()`) [D] | via templates | none; verbs take revsets as arguments | aliases | functions | — | native | ≈ 56 pest rules in `lib/src/revset.pest` [D, counted from the listing] | `jujutsu`: 6 |
| **Mercurial revsets** | `x::y`, `::x`, `only(x, y)`, `ancestors(set, depth)`, `keyword()`, `file()`, `modifies()`, `sort()`, `limit()`; `revsetalias` with `$1` parameters [D] | — | none | aliases | functions | — | native | maintained | `mercurial`: 8,256 |
| **Fossil** | plain SQL over the repository's SQLite metadata tables (`fossil sql`), with ticket reports written in SQL [D] | SQL | SQL | SQL | SQL | SQL | history lives in tables | maintained | — |

### 3.2 Notes per family, and what each one teaches moirai

**GQL and SQL/PGQ (the ISO pattern language, "GPML").** The pattern core is shared: node and edge element patterns, label expressions, quantified path patterns, restrictors and selectors. GQL adds a whole-query language on top: linear composition (`NEXT`), `LET`, `FILTER`, `FOR`, `OPTIONAL MATCH`, sessions, catalog, graph types, `INSERT`/`SET`/`DELETE`, and `GQLSTATUS` codes [D].

Three points matter for moirai:
- **Finiteness.** "Every unbounded quantifier … must be contained in the scope of either a restrictor or a selector or both" (Deutsch et al., *Graph Pattern Matching in GQL and SQL/PGQ*, SIGMOD 2022) [D]. moirai can relax this for its acyclic structural edge kinds (§6.2) [I].
- **Size.** The full standard is large: the gql-compat project lists 814 grammar productions, 228 optional features and 317 normative subclauses [C]. A moirai *subset* has to be defined explicitly and documented as such. An embedded Rust implementation claiming full GQL exists (GraphLite, 2025–26, a "reference implementation") [C]. It is useful for study, but "written from scratch" rules it out as a dependency [I].
- **Null handling.** GQL uses SQL's three-valued logic: `FILTER p.x > 0` silently drops rows where `x` is null [D Fabric guide]. This is a classic source of empty results.

**Cypher 25.** Neo4j's current language is converging on GQL. From 2025.06 it has `LET`, `FILTER`, `NEXT`, `WHEN … ELSE`, and the match modes `REPEATABLE ELEMENTS` / `DIFFERENT RELATIONSHIPS`. Later releases added `ACYCLIC` (2026.03), `IS LABELED` (2026.04), `GROUP BY` and `cardinality()` (2026.07), string interpolation (2026.08), map comprehension, and `FULLTEXT` search (2026.09). GQL function aliases such as `collect_list`, `path_length` and `duration_between` are accepted [D]. Its default match mode (DIFFERENT RELATIONSHIPS, i.e. trail semantics) is why Cypher writers never type a restrictor.

Neo4j's official MCP server exposes three tools: `get_neo4j_schema` (schema sampled through APOC), `read_neo4j_cypher` and `write_neo4j_cypher`. It also has a `--read-only` switch and a response token limit [D]. That is the industry template for "an agent writes a query language over MCP".

**Gremlin** is an imperative traversal language. Its method-chain style is close to the SDK DSL that HelixDB moved to. It is verbose (task (a): ~45 proxy tokens, 14 quote characters, §4.4). Its prior in public data is moderate, and TinkerPop 4 is still in beta [D]. It offers nothing moirai needs that GQL lacks [I].

**SPARQL.** Property paths are elegant (`?x :blocks+ :n40`), but they cannot filter intermediate nodes. Task (a) ("through open tasks only") is therefore not expressible with a path alone [D]. SPARQL 1.2 is still a Working Draft [D]. Its RDF model is a poor fit for typed fields: report 06 §3 already rejected RDF as the primary model [I].

**Datalog family.** This family is the natural fit for recursion, derived predicates and incremental maintenance, and Datomic's model fits versioned data very well:
- `as-of`, `since` and `history` are first-class database values;
- `:db/cas` expresses task (f) in ~19 proxy tokens [D/M].

But the LLM prior is thin (436 `datomic` and 167 `datalog` SO questions) [M]. The only text-to-Datalog evaluation found is a preliminary study of open-source models on an e-commerce pilot set [C]. The best open Datalog engine in Rust (Cozo) has been dormant since 2023 [D]. The strongest data point is indirect: Google's LogicLM does *not* have the LLM write Logica. The LLM emits a JSON "query request" that references predicates predefined by engineers [D].

Lesson [I]: Datalog is a good *internal* model for moirai's derived predicates (`ready`, `blocked`, `suspect`). The user-visible move is to expose those predicates *by name*, not to make agents write rules.

**EdgeQL / Gel.** This is the best-designed missing-value semantics in the survey (empty sets instead of NULL) and a pleasant shape syntax. But it has no recursion, and the company is gone (2025-12) [D]. Lesson [I]: object "shapes" are the right mental model for *output* (moirai's one-line node rendering is a fixed shape). Recursion is non-negotiable for a task graph.

**TypeQL 3, SurrealQL, HelixQL.** These are three recent bespoke languages:
- TypeQL moved from rules to recursive functions [D].
- SurrealQL bolted recursion onto arrow syntax in 2.1–2.2 [D].
- HelixDB **abandoned its bespoke text language (HelixQL v1) for SDK-built operation trees** [D].

All three have small public footprints (132, 111 and 0 SO questions) [M]. Lesson [I]: a bespoke text DSL is expensive to keep alive, and nobody arrives already knowing it.

**PRQL and pipe SQL.** Linear left-to-right pipelines are *claimed* to suit LLMs, because each step mirrors the natural-language plan [C]. No quantitative comparison of pipe syntax against standard SQL was found. PRQL has 1 SO question [M]. In moirai's case a pipeline DSL has a further practical hazard: the pipe character `|` is the shell's pipe (§4.5).

**jq and JMESPath.** jq is the one "small expression language" with an LLM benchmark. In jqBench (ICLR 2026, Microsoft Research authors), Opus 4.1 solved 76 % of tasks in jq against 83 % in Python, and GPT-5 68 % against 82 %. When the models were given the jq manual, **Opus 4.1 fell to 31 %**, which the authors call the "documentation trap" [C, secondary summary of OpenReview VStKtgGXUc]. Lesson [I]: a compact card of examples beats a long reference manual in the prompt.

**GraphQL.** It has no recursion, and its mutations are server-defined named functions. Text-to-GraphQL benchmarks report 31–48 % for eight LLMs over 20 real schemas [C]. It is unsuitable as a graph query language for a task DAG [I].

**VCS languages.** git's revision syntax is the most widely known "graph query language" in the world, and every coding agent uses it daily [I]. jj and hg revsets show what a *set-algebra* language over a DAG looks like:
- functions plus `& | ~`;
- string-pattern prefixes (`glob:`);
- **user aliases with parameters** (`[revset-aliases]`, `revsetalias`);
- a small grammar (jj ≈ 56 pest rules) [D].

jj also separates *selection* (revsets) from *rendering* (templates), and its verbs take revsets as targets (`jj abandon 'x'`) [D]. Fossil takes the opposite route and exposes plain SQL over the repository's history tables [D].

Lessons for moirai [I]:
- borrow the *revision and range grammar* (`main..lane/X`, `c812`, `lane/X~3`);
- borrow *aliases*;
- borrow "verbs accept a selection";
- do not use set algebra as the general graph language, because multi-hop joins nest inside-out (§4, task (e)).

### 3.3 The ecosystem was volatile in 2025–2026 [D]

- Kùzu, the embedded Cypher database, was archived on 2025-10-10. The team was acqui-hired; the community fork is LadybugDB.
- Gel (EdgeDB) was shut down on 2025-12-02.
- CozoDB has had no release since 2023-12.
- HelixQL v1 has been archived.
- PostgreSQL 19 reverted SQL/PGQ on 2026-09-07.
- TinkerPop 4 is still in beta.
- SPARQL 1.2 is still a Working Draft.
- The stable pole is ISO GQL / Cypher 25, backed by Neo4j, Google (Spanner Graph) and Microsoft (Fabric Graph).

For a system that must last years, aligning the *surface syntax* with the ISO pattern language is the lowest-regret choice. The engine stays moirai's own [I].

---

## 4. Evidence: how well LLMs write query languages (2024–2026)

### 4.1 Benchmarks

| Benchmark (date) | Language | Setting | Result (Claude rows in **bold**) | Tag |
|---|---|---|---|---|
| CypherBench (Dec 2024; ACL 2025) | Cypher over 11 Wikidata-derived graphs (≈7.5 relation types and 18.7 properties per graph) | zero-shot, schema in prompt | **Claude 3.5 Sonnet 61.58 % EX (best)**, GPT-4o 60.18, Gemini 1.5 Pro 39.95, Qwen2.5-72B 41.87, models under 10B below 20 %. **Claude near 0 on SORT-template questions** | [D] |
| Text2GQL-Bench (Feb 2026, Ant Group et al.) | ISO GQL (executed on Spanner Graph) and Cypher | zero-shot; 3-shot; fine-tuned | GQL EX: **Claude Opus 4.5 44.5 % zero-shot / 50.1 % 3-shot** (grammar validity 0.618 / 0.728); GPT-5.2 1.7 % / 48.2 %; Qwen3-Max 3.2 % / 49.1 %; fine-tuned Qwen3-8B 45.1 % (grammar 0.908). Cypher EX zero-shot: **Claude 43.8 %**, GPT-5.2 44.0, Qwen3-Max 47.8. "Extremely hard" tier: **Claude 16.7 % / 26.5 %** | [D] |
| SyntheT2C (Jun 2024) | Cypher, medical KG | zero-shot / 2-shot | GPT-4 49.07 % / 50.42 % (precision-style metric) | [D] |
| Neo4j Text2Cypher-2024 benchmark (Nov 2024) | Cypher, 16 demo DBs | foundation vs fine-tuned models | GPT-4o about 30 % exact-match execution | [D] |
| Mind the Query (IBM, EMNLP 2025 Industry) | Cypher, 27,529 pairs, 11 graphs | zero-shot / 5-shot retrieval; closed models *excluded* to avoid leakage | Best zero-shot totals: DeepSeek-V3 76.75 %, Llama-4-Maverick 70.97 %. By category (DeepSeek-V3): simple retrieval 92.76, complex retrieval 69.04, simple aggregation 79.97, **complex aggregation 49.93** | [D] |
| LAST-CQ (Sep 2026) | Cypher, 2,471 executable Neo4j Text2Cypher pairs | single-pass vs agentic retry | Six backbones **including Claude Sonnet 4** (execution Google-BLEU 0.474 → 0.482). Retry recovers **91.7 %** of single-pass failures; raw DB errors ≈ synthesised feedback | [D] |
| CYGNET (Jun 2026) | Cypher, CypherBench schemas | validation gate + corrector | On a template-generated corpus, the gate catches 100 % of parse, constraint and schema-reference errors (0 false positives over 1,135 queries); corrector succeeds 81–95 %; property swaps that stay valid on the label: **0 %** caught | [D] |
| Jackal (Sep 2025) | **JQL** (a filter language for a task tracker), 5,000 pairs, live Jira with 200k issues | zero-shot | Gemini 2.5 Pro 0.603; **Claude Sonnet 4 0.587, Opus 4.1 0.583, Opus 4 0.576, Sonnet 3.7 0.572**. By request type: *semantically exact* **0.963 / 0.926 / 0.915 / 0.986** (Claude rows) vs *semantically similar* 0.19–0.22. Strong models "largely avoid" invalid JQL | [D] |
| Agentic Jackal (Apr 2026) | JQL | tool-grounded agent | Value grounding raises categorical-value accuracy from 48.7 % to 71.7 % and component-field accuracy from 16.9 % to 66.2 %. The dominant remaining failures are semantic ambiguity (issue type, choice of text field) | [D] |
| LLM-KG-Bench / *Assessing SPARQL capabilities* (2024; framework v3.0 2025) | SPARQL | syntax fixing; text-to-SPARQL | **Claude 3 Opus** and GPT-4: 99 % syntactically correct on the first try; F1 rises from 0.699 to 0.972 over feedback rounds (LC-QuAD). Syntax fixing is easy; semantics is hard; numeric IRIs hurt | [D] |
| Text2SPARQL'25 challenge (ESWC 2025) | SPARQL | open challenge | Models "partially learn SPARQL syntax" but struggle with semantics and ranking | [C] |
| NL2GraphQL (2026 workshop) | GraphQL, 20 schemas | 8 LLMs | 31–48 % | [C] |
| jqBench (ICLR 2026) | jq vs Python | agentic, with or without docs | **Opus 4.1: jq 76 %, Python 83 %, jq with manual 31 %** | [C] |
| SWE-AGI (Feb 2026) | MoonBit (a new language with little pretraining data) | long-horizon build tasks from a spec | GPT-5.3-Codex 19/22, **Claude Opus 4.6 15/22** | [C] |
| text2ql (Sep 2026) | IR → SQL / GraphQL | LLM → IR | Schema-aware prompting is "the dominant accuracy lever", worth +18.4 pp exact match | [C] |

### 4.2 Where strong models fail

What goes wrong depends on how familiar the syntax is to the model. The cumulative-risk column scores how much each error class threatens moirai if nothing is done about it; the last column says what reduces it. Pointers such as "§6.2 rule R3" refer to the design rules in §6.2.

| Error class | Evidence | Cumulative risk for moirai [I] | Mitigation in moirai [I] |
|---|---|---|---|
| **Syntax** (the model writes another dialect) | 85 % of GQL zero-shot failures; drops to 21.5 % with 3 examples [D Text2GQL]. Rare in strong models on JQL, SPARQL and Cypher [D] | high for any new DSL; low for a Cypher-compatible surface | Accept Cypher spellings. Put 5–8 examples in the skill card. Return caret-positioned errors with "did you mean" |
| **Reversed edge direction** | a top CypherBench category [D] | high: `parent` (child → parent) is a trap by its very name | Name edges so they read correctly in stored direction (`CHILD_OF`, `BLOCKS`, `REFUTES`). Give endpoint kinds per edge type, so an impossible direction is a type error |
| **Schema linking and property hallucination** | "Structural hallucination" (PLAYED_IN vs PLAYED_FOR) [D Mind the Query]; 26.3 % of few-shot GQL errors [D Text2GQL]; "Entity Linking" [D CypherBench] | medium (13 kinds, ~25 edge kinds) | **Unknown label, type or property is an error, not `null`.** `CALL schema()`. Case-insensitive labels and types. Ids instead of names (`#40`) remove entity linking |
| **Aggregation and SQL-isms** | "Aggregator hallucination" (INTERSECTION, SUM) [D Mind the Query]; 43.7 % of few-shot GQL errors [D Text2GQL]; complex aggregation is the lowest category [D] | medium: loop-termination and refuted-share queries aggregate | Implicit grouping (Cypher) *and* explicit `GROUP BY` (GQL). Built-ins for the recurring aggregates (`stats loop` stays a verb) |
| **Valid but semantically wrong** (property swaps, wrong derived-state definition) | "0 %" caught by structural validation [D CYGNET]; "Different results" is the main failure bucket for frontier models [D Jackal] | **highest**: a hand-written `ready` or `blockers` diverges silently from the engine's definition (inherited exogenous blockers, flagged dangling edges, settled-elsewhere markers, leases) | **Derived state only through built-ins**, with verbs and built-ins sharing one definition (06 §10, Beads #6105). `EXPLAIN` shows which built-ins ran. Empty results carry hints ("0 rows; 3 dropped by `x.prio > 1` because `prio` is null") |
| **Ordering and top-k** | Claude 3.5 Sonnet near 0 on CypherBench SORT questions [D] (a template-specific artefact) | low | Default order = id, or priority then id for task lists. `ORDER BY` + `LIMIT` documented with one example |
| **Null / three-valued logic** | documented footgun [D Fabric guide] | medium | Header fields have defaults (priority P2, criticality normal), so comparisons are total. Optional kind fields need `IS NULL`; `EXPLAIN` flags rows dropped as UNKNOWN (later) |

### 4.3 Schema in the prompt, few-shot, documentation

- **Schema in the prompt.** It is the strongest single lever when the schema is unfamiliar: +18.4 pp [C text2ql]. Filtering the schema helps small models and does little for large ones [C Ozsoy 2025]. moirai's schema is small and fixed; its full card fits in ~400–600 tokens [I].
- **Few-shot.** It matters a lot for *unfamiliar syntax*: GQL went from 1.7 % to 48.2 % for GPT-5.2 [D]. It matters little for *familiar syntax*: Cypher 49.07 % → 50.42 % [D SyntheT2C]; Claude's GQL gain was +5.6 pp [D].
- **Documentation.** Long reference text can *hurt*: jq 76 % → 31 % for Opus 4.1 [C].
- **Implication [I].** The skill carries a ≤ 800-token card containing the schema, 6–8 canonical examples (the six tasks of §5 are a good start) and a list of what is *not* supported. The long reference stays in a linked file that is loaded only on demand.

### 4.4 Retry beats sophistication

LAST-CQ found three things [D]:
- raw database errors are as good as LLM-synthesised feedback;
- parallel sampling is *worse* than one retry at the same budget;
- 91.7 % of single-pass failures are recovered.

CYGNET shows a deterministic gate catching every parse and schema-reference error [D]. In Claude Code the retry loop comes for free: the agent sees the error text and tries again.

Implication [I]: moirai's parser and binder must produce precise, short, stable errors. That means a code, a line and column, the expected tokens, a nearest-name suggestion, and "not supported in moirai QL v1: use …". Parse and schema errors get their own exit code (2), and a cheap check mode (`--check`) runs no I/O. Anthropic's tool-writing guide says error responses should "clearly communicate specific and actionable improvements, rather than opaque error codes or tracebacks" [D].

### 4.5 JSON arguments vs a text language; constrained decoding

| Consideration | Evidence | Implication [I] |
|---|---|---|
| Models write real code better than synthetic tool-call JSON | Cloudflare "Code Mode": "LLMs are better at writing code to call MCP, than at calling MCP directly" [D/C]. Anthropic's code-execution-with-MCP cut one task from 150,000 to 2,000 tokens [D] | Prefer one `query` string parameter holding a text language over a deep JSON AST. The JSON AST becomes the *internal* IR and an `--ast` output for programs |
| Constrained decoding on Claude | Structured outputs guarantee JSON-schema conformance, but **recursive schemas are not supported** and there are no custom grammars [D] | A JSON AST for nested boolean filters or subqueries cannot be *strictly* constrained on Claude anyway. It buys no guarantee a text language lacks |
| Constrained decoding elsewhere | GPT-5 custom tools accept Lark or regex grammars [D], but users report slowness and no hard guarantee [C] | Publish the moirai QL grammar (EBNF / Lark) so non-Claude clients can constrain. It is optional |
| Length | JSON AST ≈ 48 proxy tokens per query vs ≈ 36 for the hybrid; up to 70 quote characters on task (e) [M] | JSON costs more tokens and reads worse for humans |

### 4.6 What this means for moirai's expected accuracy [I]

The moirai setting differs from every benchmark above in four ways:
1. the agent knows its own intent, which is the "semantically exact" condition (JQL 0.915–0.993 [D]);
2. ids replace entity names;
3. the schema is small and fixed and is carried by the skill;
4. derived state is available by name.

The remaining risk sits in multi-hop joins and aggregations: in Mind the Query, strong open models score 54–69 % zero-shot on complex retrieval and 24–50 % on complex aggregation [D]. Those are the cases where retry helps most.

My estimate, which **must be measured** (§8): for typical agent queries in the hybrid syntax, Claude will get **≥ 85 % right on the first try and ≥ 95 % after one retry**. Plain ISO GQL without Cypher tolerance would be lower for non-Claude clients (1.7 % zero-shot for GPT-5.2 [D]).

---

## 5. The six moirai tasks in candidate syntaxes

### 5.1 Method

**Candidates.**
1. ISO GQL, written strictly: restrictor or selector on unbounded quantifiers, procedures for moirai functions.
2. Cypher in its "classic" form, the style models most often emit (variable-length `*1..`).
3. Datalog in the CozoScript dialect over stored relations `node`, `edge` and `change`.
4. Revset-style set algebra (jj-inspired, filter atoms as sets).
5. A pipeline DSL (jq / PRQL / Gremlin flavour, with named inverse edges).
6. A JSON AST as an MCP argument (HelixDB-v3 / LogicLM style).
7. **Hybrid**: the proposed GQL-core with moirai sugar (§6).
8. The v1 baseline: CLI verbs plus GitHub-style filters (30 §7.1).

The schema is that of 30 §3. "Open" means `status ∈ {open, in_progress}`. `ready`, `superseded` and `applies()` are engine built-ins (for the non-hybrid candidates, as properties or procedures). Branch selection uses each syntax's native form.

**Token proxy [M].** No Claude tokenizer is available offline. The probe:
- applies a cl100k-style pre-tokeniser regex (letter runs with one optional leading symbol; digit runs of up to 3; punctuation runs; whitespace);
- counts 1 token per letter run of ≤ 8 letters, otherwise ⌈len/6⌉;
- counts ⌈len/2⌉ per punctuation run and 1 per digit run.

Queries are normalised to single spaces. The proxy averages 2.49 characters per token, so absolute values are probably 10–25 % high for real BPE. Anthropic states the Opus 4.7-generation tokenizer yields about 1.0–1.35× more tokens than the previous one for the same text [C, via migration notes]. **Treat the numbers as ratios between syntaxes, not as billing figures.**

### 5.2 Queries

**(a) Ids of all open tasks that block #40, transitively (walking only through open tasks)**

```text
-- GQL
MATCH ANY (x:Task)((a:Task)-[:BLOCKS]->(b) WHERE a.status IN ['open','in_progress'])+(:Task {id: 40})
RETURN DISTINCT x.id
-- Cypher (classic)
MATCH p = (x:Task)-[:BLOCKS*1..]->(:Task {id: 40})
WHERE all(n IN nodes(p)[..-1] WHERE n.status IN ['open','in_progress'])
RETURN DISTINCT x.id
-- Datalog (CozoScript)
open[x] := *node{id: x, kind: 'task', status}, status in ['open', 'in_progress']
blk[x]  := *edge{src: x, kind: 'blocks', dst: 40}, open[x]
blk[x]  := *edge{src: x, kind: 'blocks', dst: y}, blk[y], open[x]
?[x] := blk[x]
-- Revset
reach(#40, blocked_by, open) & task
-- Pipeline
#40 | blocked_by+ via open | task | ids
-- JSON AST
{"start":[40],"walk":{"edge":"blocks","dir":"in","min":1,"via":{"status":["open","in_progress"]}},"where":{"kind":"task"},"out":"ids"}
-- Hybrid
MATCH (x:Task)((a WHERE a.open)-[:BLOCKS]->())+(#40) RETURN DISTINCT x.id
-- CLI (engine semantics, which ALSO include blockers inherited from ancestors, flagged dangling edges and markers)
moirai blockers 40 --transitive --ids
```

The last line shows the *semantic* trap. The engine's `blockers` (30 §3.5) is not the naive transitive closure. Every raw query above returns a *different* set whenever #40's ancestors carry exogenous blockers. The hybrid therefore also offers `FOR x IN blockers(#40, transitive) RETURN x.id`, which is the verb's definition exposed as a table function [I].

**(b) Ready tasks under epic #12, ordered by priority**

```text
-- GQL
MATCH ANY (t:Task)-[:PARENT]->+(:Task {id: 12})
FILTER t.ready
RETURN t.id, t.priority ORDER BY t.priority, t.id
-- Cypher
MATCH (t:Task)-[:PARENT*1..]->(:Task {id: 12}) WHERE t.ready RETURN t.id ORDER BY t.priority, t.id
-- Datalog
sub[x] := *edge{src: x, kind: 'parent', dst: 12}
sub[x] := *edge{src: x, kind: 'parent', dst: y}, sub[y]
?[x, p] := sub[x], *ready{id: x}, *node{id: x, priority: p}
:order p, x
-- Revset
sort(ready & subtree(#12), prio)
-- Pipeline
#12 | children+ | ready | sort prio | ids
-- JSON AST
{"start":[12],"walk":{"edge":"parent","dir":"in","min":1},"where":{"ready":true},"order":["priority","id"],"out":"ids"}
-- Hybrid
MATCH (t:Task)-[:CHILD_OF]->+(#12) FILTER t.ready RETURN t ORDER BY t.priority
-- CLI
moirai ready --scope 12 --ids
```

Without a `ready` built-in, GQL has to spell the predicate out: leaf, no open blockers, no exogenous blocker on any ancestor, not deferred. That costs **≈ 184 proxy tokens (473 characters)** [M], and it *still* cannot see leases or settled/deleted markers, because those are runtime state and are not in the versioned graph (30 §3.5, I36′). Built-ins are therefore not optional [I].

**(c) Rules applying to path `crates/ecs/**` that are critical and not superseded**

```text
-- GQL
MATCH (r:Rule)
FILTER r.criticality = 'critical' AND r.status <> 'superseded' AND applies_to(r, 'crates/ecs/**')
RETURN r.id, r.title
-- Cypher: same shape with WHERE and moirai.appliesTo(r, 'crates/ecs/**')
-- Datalog
?[r, t] := *node{id: r, kind: 'rule', criticality: 'critical', status, title: t, applies_to: g},
           status != 'superseded', glob_overlap(g, 'crates/ecs/**')
-- Revset
rule & critical & ~superseded & applies("crates/ecs/**")
-- Pipeline
rule critical -superseded applies:crates/ecs/** | ids
-- JSON AST
{"where":{"kind":"rule","criticality":"critical","status":{"ne":"superseded"},"applies_to":"crates/ecs/**"},"out":"lines"}
-- Hybrid
MATCH (r:Rule) FILTER r.criticality = 'critical' AND NOT r.superseded AND r.applies('crates/ecs/**') RETURN r
-- CLI
moirai find kind:rule crit:critical -status:superseded applies:crates/ecs/** --ids
```

No family has glob or path semantics built in. Every candidate needs a domain function: glob *overlap* between a rule's `applies_to` and the query glob, with an empty set meaning `*`. This is also where R4 file links will plug in: `(f)-[:ABOUT]->(:File {path: …})` [I].

**(d) What changed in the subtree of #12 between two commits, and on branch X vs main**

```text
-- GQL (no native history: a procedure)
CALL changes('c812..c900') YIELD node, op, field, old, new, commit
MATCH ANY (node)-[:PARENT]->*(:Task {id: 12})
RETURN node.id, op, field, old, new, commit
-- Datalog (over the change log, Datomic-history style)
sub[x] := x = 12
sub[x] := *edge{src: x, kind: 'parent', dst: y}, sub[y]
?[x, op, f, old, new, c] := *change{node: x, op, field: f, old, new, commit: c}, in_range(c, 'c812..c900'), sub[x]
-- Revset
changed(c812..c900) & subtree(#12)          changed(main..lane/X) & subtree(#12)
-- Pipeline
changes c812..c900 | under #12              changes main..lane/X | under #12
-- JSON AST
{"changes":{"range":"c812..c900"},"scope":{"subtree":12},"out":"diff"}
-- Hybrid
MATCH (n)-[:CHILD_OF]->*(#12) FOR ch IN changes(n, c812..c900) RETURN ch
USE lane/X MATCH (n)-[:CHILD_OF]->*(#12) FOR ch IN changes(n, main..lane/X) RETURN ch
-- CLI
moirai diff c812..c900 --scope 12          moirai diff main..lane/X --scope 12
```

**Revision grammar [I].** Range semantics must be chosen once and documented. In git, `A...B` means *symmetric difference* for `git log` but *merge-base..B* for `git diff`. jj's `x..y` is unambiguous (ancestors of y minus ancestors of x). Recommended:
- `a..b` = what `b` added since `a` (log semantics);
- `fork(a, b)` = the LCA;
- `a...b` = both sides since the LCA (the merge preview of 30 §5a.6).

Subtree membership is evaluated on the `USE` view (the head by default). Nodes deleted inside the range still appear, because `changes()` reads the op chains, not the current graph.

**(e) Findings refuted by critic runs in lane L**

```text
-- GQL (and Cypher, identical)
USE `lane/L`
MATCH (:Lane {name: 'L'})<-[:RUNS_IN]-(:Run {role: 'critic'})-[:PRODUCED]->(:Finding)-[:REFUTES]->(f:Finding {status: 'refuted'})
RETURN DISTINCT f.id
-- Datalog
?[f] := *node{id: l, kind: 'lane', name: 'L'}, *edge{src: r, kind: 'runs_in', dst: l}, *node{id: r, role: 'critic'},
        *edge{src: r, kind: 'produced', dst: x}, *node{id: x, kind: 'finding'},
        *edge{src: x, kind: 'refutes', dst: f}, *node{id: f, kind: 'finding', status: 'refuted'}
-- Revset (inside-out nesting)
at(lane/L, finding & refuted & out(out(in(lane:L, runs_in) & role:critic, produced) & finding, refutes))
-- Pipeline
on lane/L: lane:L | runs role:critic | produced finding | refutes refuted | ids
-- JSON AST: 280 characters, 70 quote characters (see results.json)
-- Hybrid: as GQL, with USE lane/L unquoted and RETURN DISTINCT f
-- CLI: not expressible; it would need a new qualifier such as refuted-by-role:critic
```

This is the task that separates the families:
- **Pattern languages** state the chain once, left to right, with the joins implicit.
- **Set algebra** has to nest it inside-out, like SQL subqueries. The revset version was the hardest of the 55 queries in this section to write correctly: the author had to reason about `in` versus `out` at every level [M, anecdotal].
- **Pipelines** handle *linear* chains well but have no variables for branching joins, e.g. "... and the same run also produced a verdict with outcome fail_*".

`run.role` is assumed to be a field. If role lives only in commit provenance (30 §3.1), a `produced_by_role()` built-in is needed [I].

**(f) Set done = true on #41 if its status is still in_progress (guarded mutation)**

```text
-- GQL
MATCH (t:Task {id: 41}) FILTER t.status = 'in_progress' SET t.done = TRUE RETURN t.id
-- Cypher
MATCH (t:Task {id: 41}) WHERE t.status = 'in_progress' SET t.done = true RETURN t.id
-- Datalog (Cozo)
?[id, status] := *node{id, status: s}, id = 41, s = 'in_progress', status = 'done'
:update node {id => status}
-- Datomic (native compare-and-swap)
[[:db/cas 41 :task/status :task.status/in_progress :task.status/done]]
-- Revset (jj style: verbs take sets)
set(#41 & in_progress, done=true)
-- Pipeline
#41 status:in_progress | set done=true
-- JSON AST
{"op":"set","id":41,"set":{"done":true},"if":{"status":"in_progress"}}
-- Hybrid
MATCH (t {id: 41}) FILTER t.status = 'in_progress' SET t.done = true RETURN t
-- CLI
moirai set 41 --done --if-status in_progress
```

Semantics required in every syntax [I]:
- the `MATCH`/`FILTER` is evaluated under the writer byte, so the check and the write are atomic;
- `done = true` goes through the guarded transition (30 §3.6), so it can fail on open children or a gating verdict with exit 6;
- it is one commit carrying provenance, and it honours the role policy and leases;
- `--expect 1` turns "0 rows matched" into exit 4 and prints the current value, which is the CAS failure;
- `--idempotency-key` works as for every other write.

### 5.3 Measured proxy metrics [M]

Estimated tokens per query (normalised characters in parentheses). "Quotes" counts `' " \`` characters, which is shell-quoting friction. "Depth" is the maximum bracket nesting.

| Syntax | a | b | c | d1 | d2 | e | f | **Σ tokens** | mean / query | Σ quotes | max depth |
|---|---|---|---|---|---|---|---|---|---|---|---|
| CLI verbs + filters | 10 (38) | 9 (30) | 21 (82) | 13 (34) | 12 (36) | n/a | 13 (45) | **78\*** | 13.0 | 0 | 0 |
| Pipeline | 17 (39) | 17 (41) | 15 (53) | 12 (30) | 11 (32) | 27 (79) | 12 (38) | **111** | 15.9 | 0 | 0 |
| Revset | 14 (35) | 11 (32) | 17 (56) | 13 (34) | 12 (36) | 36 (104) | 12 (33) | **115** | 16.4 | 2 | 4 |
| **Hybrid (GQL-core + sugar)** | 29 (73) | 29 (78) | 38 (109) | 33 (72) | 37 (85) | 55 (158) | 31 (77) | **252** | 36.0 | 12 | 2 |
| JSON AST | 54 (134) | 49 (119) | 42 (122) | 27 (70) | 26 (72) | 111 (280) | 29 (70) | **338** | 48.3 | 198 | 4 |
| ISO GQL (strict) | 47 (122) | 44 (112) | 47 (133) | 68 (156) | 67 (158) | 57 (163) | 33 (85) | **363** | 51.9 | 24 | 2 |
| Cypher (classic) | 60 (143) | 41 (98) | 48 (138) | 70 (162) | 69 (164) | 57 (163) | 33 (84) | **378** | 54.0 | 24 | 2 |
| Datalog (Cozo) | 99 (218) | 89 (176) | 61 (153) | 101 (187) | 100 (189) | 135 (294) | 46 (110) | **631** | 90.1 | 54 | 1 |

\* Six tasks only; (e) is not expressible.

Side examples for (a): SQL/PGQ 82 tokens (195 characters, depth 4); SPARQL 43 (cannot filter intermediate nodes); Gremlin 45 (14 quotes); Datomic rules 91; SurrealQL ≈ 32. Spelled-out GQL `ready` for (b): 184. Datomic `:db/cas` for (f): 19 [M].

### 5.4 Observations

1. **The hybrid saves ~31 % over strict GQL** (252 vs 363). The saving comes from `#N` literals (`(#40)` against `(:Task {id: 40})`), built-in booleans (`a.open` against `a.status IN ['open','in_progress']`), unquoted revisions, and no mandatory restrictor [M].
2. **Terse DSLs are ~2.2× shorter than the hybrid, but give up the prior and the joins.** On the one task that needs a real join (e) the gap shrinks to 27–36 vs 55 tokens. Absolute differences are ~20 tokens per query. That is under 0.1 % of a 25k tool-output cap, and far below the cost of one failed call (error text plus a re-issued query: ~100–300 tokens) [I].
3. **JSON is the worst of both.** It is almost as long as GQL, it is unreadable, and it carries 198 quote characters over the 7 tasks [M]. The Claude-side constraint guarantee cannot cover recursive filter trees anyway [D].
4. **Datalog is the longest**, because each hop needs an explicit join atom and each recursion two rules. It is the best of all only for (f), with Datomic's `:db/cas` [M].
5. **Verbs remain unbeatable when they exist** (≈ 13 tokens), and they carry the engine's exact semantics. The query language complements them; it does not replace them [I].

---

## 6. Recommendation: "moirai QL" = GQL-core + moirai sugar + git/jj revision grammar

### 6.1 Evaluation matrix [I, from §3–§5]

Scale: ++ best, + good, 0 neutral, − weak, −− poor.

| Criterion | GQL strict | Cypher | Datalog | Revset | Pipeline | JSON AST | **Hybrid** |
|---|---|---|---|---|---|---|---|
| LLM prior (Claude), first-try syntax | + (Claude 44.5 % zero-shot on a hard GQL set; others ~2 %) | ++ | − | 0 (git revisions ++, jj revsets −) | −− (new) | + (JSON known, schema new) | **++** (Cypher spellings accepted) |
| Semantic-error exposure (direction, derived state) | − | − | − | + | + | 0 | **+** (direction-readable edges, built-ins, strict schema) |
| Tokens per query | 52 | 54 | 90 | 16 | 16 | 48 | **36** |
| Human readability | + | + | − | 0 / − (nesting) | + | −− | **+** |
| Expressiveness (joins, recursion, aggregation, subqueries) | ++ | ++ | ++ | − | 0 (linear only) | 0 | **++** (subset grows) |
| Versioning (branch, as-of, diff) | − (procedures) | − | + (Datomic model) | ++ | + | + | **+** (`USE` + revision grammar + `changes()`) |
| Guarded mutation | + | + | + (Datomic ++) | − | 0 | + | **+** |
| Engine and implementation cost | −− (full standard) | − | − (semi-naive evaluation, stratification) | ++ (≈ 56 rules) | ++ | ++ | **0** (subset: parser + rule-based planner) |
| Shell friendliness | − | − | − | − (`& \| ~`) | −− (`\|` is the shell pipe) | −− | **−** (use stdin, heredoc or MCP) |
| Standard / longevity | ++ (ISO) | + | − (every dialect differs) | 0 | − | 0 | **+** (subset of the ISO pattern language) |

### 6.2 Design rules, each traced to evidence [I]

| # | Rule | Evidence |
|---|---|---|
| R1 | **Surface = a documented subset of ISO GQL patterns and clauses, plus Cypher-compatible spellings**: `WHERE` ≡ `FILTER`, `-[:T*1..]->` ≡ `-[:T]->+`, `WITH` ≡ `NEXT` (later), `collect()` ≡ `collect_list()`, `size()` ≡ `cardinality()`, `toLower()` ≡ `lower()` | Cypher prior [M SO counts]; GQL zero-shot syntax failures [D Text2GQL] |
| R2 | **`#N` is a node literal** everywhere a node pattern or a value can appear: `(#40)`, `x = #40`, `IN [#40, #41]`. `RETURN x.id` prints `#40` | ids are ~2 tokens and are copied verbatim from moirai output [06 §9] |
| R3 | **Edge types read correctly in their stored direction**: `CHILD_OF` (the stored `parent` edge), `BLOCKS`, `GATES`, `REFUTES`, `CONFIRMS`, `VERIFIES`, `ADDRESSES`, `SUPERSEDES`, `DERIVED_FROM`, `CITES`, `PRODUCED`, `RUNS_IN`, `ABOUT`, `SCOPED_TO`, `ANSWERS`, `MENTIONS`, `RELATES`. Each type declares its endpoint kinds, so an impossible direction is a *type error* | reversed direction is a top error [D CypherBench] |
| R4 | **Strict schema**: an unknown kind, edge type or property is an error with a nearest-name suggestion, never a silent `null`. Kinds and types match case-insensitively (`:task` ≡ `:Task`, `:blocks` ≡ `:BLOCKS`) | schema linking [D]; CYGNET gate [D] |
| R5 | **Derived state only through built-ins** that share code with the verbs. Virtual properties: `open`, `done`, `ready`, `blocked`, `container`, `suspect`, `superseded`, `conflicted`, `claimed` (runtime), `settled_elsewhere`, `rev`. Functions: `applies(glob)`, `text('…')`, `blockers(n [, transitive])`, `subtree(n)`, `ancestors(n)`, `changes(n, range)`, `fork(a, b)`, `now()` | one predicate definition [06 §10]; §5.2 (a)/(b) traps [M] |
| R6 | **Default restrictor**: quantified paths over acyclic kinds (`CHILD_OF`, `BLOCKS`, `GATES`, `SUPERSEDES`, `DERIVED_FROM`, `DEPENDS_ON`) need none, because the invariants (I4–I6) make every walk finite. Cyclic kinds (`RELATES`, `MENTIONS`, `CITES`) require `ANY`, `TRAIL`, `ACYCLIC` or `SHORTEST` | GPML finiteness rule [D]; author's own slip [M] |
| R7 | **Versioning via the `USE` clause and a git/jj revision grammar**: `USE main`, `USE lane/X`, `USE c812`, `USE main@2026-09-20`, `USE lane/X~3`. Ranges `a..b`, `a...b`, `fork(a,b)`. `changes(n, range)` and `CALL changes(range) YIELD …`. On as-of views, derived built-ins raise an error unless `RECOMPUTE` is given (I18′) | 30 §5a.6; git/jj semantics [D] |
| R8 | **One guarded write form in v1**: `MATCH … [FILTER …] SET x.field = v [, …] [RETURN …]`. Only fields and status (through transitions); one commit; `--expect N`; idempotency key; role policy and leases enforced. Creating, deleting and linking stay in the verbs and the `write` batch | §5.2 (f); 30 §7.2–7.3 |
| R9 | **Output contract = the verbs' contract**. The first line is `branch: … · rev N`. `RETURN node` prints the one-line rendering; `RETURN expr, …` prints TSV; `--ids`, `--json v1` and `--jsonl` apply. The default `LIMIT` is 50, with a `more: cursor` footer and never silent truncation | 07 §3; 30 §7.1 |
| R10 | **Errors are the retry channel**: a code, the position with a caret, expected tokens, "did you mean", "not in moirai QL v1: use …". Exit 2 for parse/bind errors, 4 for guard failure. `--check` validates without executing. `EXPLAIN` shows the plan, the built-ins used, the estimated rows and the budget | LAST-CQ [D]; Anthropic tool guide [D] |
| R11 | **Parameters** are `$name`, supplied as an MCP `params` object or with CLI `--param name=value`. Never splice strings | standard in GQL/Cypher [D]; injection hygiene |
| R12 | **Budgets**: a node-visit budget (default 100k, `--budget`), an intermediate-row memory cap (default 1 MiB), top-k for `ORDER BY` with `LIMIT`, and a timeout. When a budget is exceeded the query returns partial rows *plus* an explicit footer | RAM target (30 §8.1) |
| R13 | **Each verb has a canonical query definition**, printed by `moirai <verb> --as-query`. Property tests assert *verb output = query output* | Beads "three universes" lesson (06 §10) |

### 6.3 A grammar sketch (the v1 subset) [I]

```text
query      := [USE rev] clause+ | EXPLAIN query
clause     := MATCH [ANY|TRAIL|ACYCLIC] path (',' path)* [WHERE expr]
            | (FILTER|WHERE) expr
            | FOR var IN expr                    -- table functions: blockers(), subtree(), changes()
            | CALL fn '(' args ')' YIELD var (',' var)*
            | SET var '.' prop '=' expr (',' ...)*     -- guarded write (R8)
            | RETURN [DISTINCT] item (',' item)* [GROUP BY expr,..] [ORDER BY expr [ASC|DESC],..] [LIMIT n] [OFFSET n]
path       := node (edge node)* | '(' path [WHERE expr] ')' quant
node       := '(' [var] [':' kind ('|' kind)*] ['{' prop ':' expr,.. '}'] [WHERE expr] ')' | '(' ['#'N | var '#'N] ')'
edge       := '-[' [var] ':' TYPE ('|' TYPE)* ']->' [quant] | '<-[' ... ']-' [quant] | '-[' ... ']-' [quant]
quant      := '+' | '*' | '{' m ',' [n] '}' | '*' [m] '..' [n]          -- Cypher alias accepted
expr       := literal | '#'N | $param | var | var '.' prop | fn '(' args ')' | EXISTS '{' MATCH path.. [WHERE expr] '}'
            | expr op expr | NOT expr | expr IS [NOT] NULL | expr [NOT] IN list | expr (STARTS WITH|CONTAINS) str
agg        := count | min | max | sum | avg | collect_list(collect)
rev        := ref | 'c'HEX | tag | rev '~' N | rev '@' date ; range := rev '..' rev | rev '...' rev | fork '(' rev ',' rev ')'
```

This is roughly 40–60 productions, the same order as jj's revset grammar (≈ 56) and an order of magnitude below full GQL (814) [D/C]. A hand-written recursive-descent / Pratt parser in Rust of about 2–4k lines would do. It needs no parser-generator dependency and produces no per-query heap churn beyond one arena [I].

### 6.4 How it maps onto the engine (performance, RAM, versioning) [I]

- **Planning is rule-based**, with no cost-based optimiser in v1. Anchors in order of preference:
  1. `#N` or `$id`: O(1) row;
  2. a header-column equality or a built-in bitset (`kind`, `status`, `ready`, `is_blocker`, `suspect`);
  3. a label scan.
  
  The planner then expands edges from the anchor with the forward or reverse CSR. `EXPLAIN` prints this plan.
- **Quantified paths** run as BFS over a single edge kind (or an alternation of kinds), with a visited bitset: n/8 bytes, so 125 KB at 1e6 nodes. The per-step `WHERE` is a predicate on the frontier. At 1e5 nodes a walk costs the same as `blockers --transitive` (10–200 µs, 30 §8.1).
- **Execution is streaming.** Pull-based iterators over `u32` row ids. `DISTINCT` uses a bitset. `ORDER BY` + `LIMIT` uses a top-k heap. Aggregates use a small hash map bounded by the memory cap. The target is ≤ 1 MiB of extra private memory for typical queries, which keeps the CLI inside its ≤ 4 MB budget.
- **Versions.** `USE lane/X` is the existing branch overlay (+1–10 ms on first read). `USE c812` is the existing whole-graph as-of view: a reverse-apply of at most 50k ops from a pinned set, and derived fields are off unless `RECOMPUTE` is given. `changes(n, range)` walks node `n`'s op chain in O(edits). `CALL changes(range)` folds the range's changesets once. There are no new storage structures.
- **CPU at idle is zero.** There are no standing queries, caches or background threads. Parsing and planning take µs per call.
- **Writes** (R8) reuse the commit path. The query is re-bound and re-executed under the writer byte, the `SET` ops become a normal changeset, and validators, role policy and idempotency apply as for `moirai set`.

### 6.5 Surfaces: CLI, MCP, skill [I]

- **CLI.** `moirai q -` (stdin) or `moirai q @file.mql` is the recommended form, because queries contain `( ) > | ' # $`, which both bash and PowerShell interpret. `moirai q '…'` works in bash but is fragile in PowerShell 5.1 (30 §7.1). Flags: `--branch`, `--param k=v`, `--ids`, `--json`, `--limit`, `--cursor`, `--budget`, `--check`, `--explain`, `--expect N`, `--idempotency-key`.
- **MCP.** Add one **deferred** tool, `query`, with parameters `{q, params, branch, limit, cursor, format, dry_run}`. Its description is two lines: "moirai QL (GQL-style) read queries + guarded SET; see skill moirai-ql; call schema() first if unsure". Keep `find` as the filter-syntax tool. The upfront cost is roughly 20 characters of the names-only listing (07 §5.1). Reads are spawn-free. A `SET` passes through the existing `PreToolUse` stamp matcher, extended to `mcp__moirai__query` when `q` contains `SET`.
- **Skill.** A `moirai-ql` card of ≤ 800 tokens containing:
  - the schema card: 13 kinds, their key fields, the edge types with endpoint kinds and direction phrases;
  - 8 examples (the §5 hybrid set plus one aggregation and one `EXISTS`);
  - the built-in list;
  - "not supported in v1" with the alternatives;
  - the revision grammar;
  - the retry advice ("on error, fix the part named in the message; do not rewrite from scratch").
  
  The full reference goes in `reference-ql.md`, loaded on demand. This follows the jqBench documentation-trap result [C] and the few-shot result from Text2GQL-Bench [D].
- **Humans.** The same language, plus `EXPLAIN`, pretty TTY output and optional colour. A tree-sitter grammar for editors can come later.

### 6.6 Side finding: `#N` on a shell command line [M]

Measured in this session:
- in Git-Bash, `printf '%s|' show #40 --transitive` prints `show|`: the rest of the line is a comment;
- in Windows PowerShell 5.1, a function called with `show #40 --transitive` receives only `show`;
- `'#40'`, `40` and `x#41` pass through intact.

The synthesis examples (`moirai blockers #51 --explain`, `moirai set #12 --status done …`) would therefore silently drop the id and every flag after it. Required fixes [I]:
- the CLI accepts bare `40` (and `n40`) everywhere an id is expected;
- the skill's shell examples never show an unquoted `#N`;
- a verb that expects an id and receives none exits 2 with "id missing (note: unquoted `#` starts a shell comment)".

Output may keep `#40`. Inside a quoted or stdin query, `#40` is safe.

---

## 7. Must-have vs later vs never

| Feature | v1 of the query language (must) | v1.x (later) | Never / not planned | Why [I] |
|---|---|---|---|---|
| `MATCH` with node and edge patterns, multiple comma-separated patterns (joins), inline `WHERE` | ✔ | | | tasks (c), (e) |
| Quantified paths `+ * {m,n}` over one edge kind or an alternation; per-step `WHERE` (quantified path patterns); default restrictor for acyclic kinds | ✔ | shortest-path selectors (`ANY SHORTEST`, `ALL SHORTEST`), path variables and path functions | | task (a); acyclicity invariants |
| `FILTER` / `WHERE`, boolean / comparison / `IN` / `IS NULL` / `STARTS WITH` / `CONTAINS` | ✔ | regex `=~` | | |
| `#N` literals, `$params` | ✔ | | string-spliced parameters | R2, R11 |
| Built-in derived properties and functions (R5), incl. `applies()`, `text()`, `blockers()`, `subtree()` | ✔ | user-defined *aliases* (jj `[revset-aliases]` / hg `revsetalias` style), later *saved queries* as versioned nodes | user-defined recursive rules (Datalog) in the language | one definition of derived state |
| `RETURN [DISTINCT]`, `ORDER BY`, `LIMIT` / `OFFSET`, cursor | ✔ | | | output contract |
| Aggregation: `count`, `min`, `max`, `sum`, `avg`, `collect_list`; implicit grouping + `GROUP BY` | ✔ | `HAVING` equivalent via `NEXT` / `FILTER` | | loop-termination and refuted-share queries |
| `EXISTS {}` / `NOT EXISTS {}` | ✔ | `COUNT {}`, `CALL {}` subqueries | | "not superseded / no open blockers" patterns |
| `USE branch / commit / tag / time`; `changes()`; revision ranges | ✔ | multi-branch rows (`USE ALL` / `--across`), as-of with `RECOMPUTE` of derived fields | | task (d); R1–R3 |
| Guarded `SET` of fields and status in one commit (`--expect`, idempotency) | ✔ | `INSERT` / `DELETE` / link / unlink through the language, bulk writes with dry-run diff | `MERGE`-style upserts | task (f); writes stay mostly in verbs |
| `EXPLAIN`, `--check`, `CALL schema()`, strict schema errors, budgets | ✔ | `PROFILE` with timings; null-drop diagnostics | | the retry loop |
| `OPTIONAL MATCH`, `LET`, `NEXT` / `WITH`, `CASE` / `WHEN`, `UNION` | | ✔ | | frequent in LLM Cypher; accept them in the parser with a "v1.x" error before they are implemented |
| Full-text `SEARCH` / `text()` over bodies | titles + abstracts (tier-1 FTS) | bodies, tier-2 index | embeddings in the core | 30 §2.11 |
| Standing / incremental queries (DBSP-style) for hooks | | maybe | background evaluation | ~zero idle CPU |
| SQL surface | | optional `export sqlite` for offline ad-hoc SQL (Fossil-style) | an embedded SQL engine | RAM and "from scratch" |
| JSON AST | internal IR; `--ast` output | accepted as input for programs | as the primary agent surface | §4.5 |
| Gremlin, SPARQL, Datalog, pipeline or revset *as a second general language* | | a pipeline shorthand only if the §8 measurement shows a clear win | ✔ | one language, one card |
| Procedure zoo, user code in queries | | | ✔ | security, RAM, semantics drift |

---

## 8. Validation plan (before freezing the surface) [I]

A 1–2 day S0-style experiment. It is cheap, and it settles the one real uncertainty: first-try accuracy on moirai's own questions.

1. **Corpus.** 60 questions:
   - the 6 tasks of §5 × 3 phrasings (literal, short, paraphrased — the Jackal categories);
   - 30 questions mined from real BoykoEngine sessions (loop termination, refuted share, "what is blocking the merge of lane L", "rules about files owned by lane L", history questions);
   - 12 adversarial cases (direction traps, a hand-written `ready`, null fields, cyclic kinds).
2. **Candidates.** Hybrid, strict GQL, pipeline, JSON AST. Each gets the same ≤ 800-token card with 6 examples. For the hybrid, also run 0-shot and schema-only.
3. **Models.** The Claude models the owner actually runs as agents (Haiku, Sonnet, Opus), plus one non-Claude model if the owner uses one.
4. **Harness.** A mock engine: the hybrid parser, binder and planner prototype plus an in-memory fixture graph of ~2k nodes with branches. Error messages as specified in R10. One retry allowed.
5. **Metrics.** First-try EX; EX after one retry; parse / bind / semantic error split; tokens written; latency.
6. **Gates.** Adopt the hybrid if it reaches ≥ 85 % first-try EX and ≥ 95 % after one retry on Sonnet-class agents, and is not more than 5 pp below the best candidate. If the pipeline beats it by more than 10 pp on the literal and short phrasings, add a pipeline *shorthand* that desugars to the same IR (v1.x), but never as a second full language.

---

## 9. Risks [I]

| Risk | Impact | Mitigation |
|---|---|---|
| The subset trap: agents write Cypher features outside the subset (`OPTIONAL MATCH`, `WITH`, `UNWIND`, `apoc.*`) | failed calls | the parser recognises the whole Cypher / GQL keyword set and answers "not in v1; do X"; the card lists the unsupported features; the §8 measurement |
| Semantic divergence between queries and verbs | wrong dispatch decisions | R5 + R13 property tests; `EXPLAIN` shows the built-ins used |
| Scope creep toward "full GQL" | months of work; a large binary | freeze the subset per version; conformance is *not* a goal unless the owner decides otherwise (§10) |
| Heavy queries break the RAM or latency budget | CLI over 4 MB | budgets (R12), streaming, top-k, default `LIMIT` |
| Shell quoting (`#`, `'`, `\|`, `>`) | silently wrong commands | stdin / `@file` as the documented CLI form; MCP for agents; bare-integer ids (§6.6) |
| Writes through the query language bypass role or lease rules | integrity | the `SET` path reuses `set`'s validators, role policy and lease checks; only fields and status in v1 |
| A GQL standard revision diverges from the subset | churn | the subset uses only stable GPML core plus Cypher 25 spellings; keep a small compatibility table |

---

## 10. Open questions (owner decisions)

1. **May the query language write?** The options:
   - read-only, with all writes through verbs;
   - reads plus the guarded `SET` of §6.2 R8 (recommended);
   - full DML (`INSERT` / `DELETE` / link) in v1.
   
   This is a scope and safety call. With role policy enforced, all three are technically feasible.
2. **Is ISO GQL conformance a goal in itself** (e.g. queries portable to Neo4j or Spanner, a "GQL-compatible" claim), or is "GQL-shaped, Cypher-tolerant, moirai-extended" enough? The recommendation assumes the latter. Conformance adds large scope (§3.2).
3. **Where does R5 sit in the roadmap?** The M-milestones of 30 §9 have no slot for it. The v1 subset (parser, binder, planner, built-ins, `USE` / `changes`, errors, `EXPLAIN`) is an estimated 3–5k lines of Rust. Options:
   - ship it in v1 alongside the verbs;
   - ship it as v1.1 right after the storage milestones, with the §8 measurement first.
4. **Should saved queries and aliases be versioned objects in the graph** (shared across agents and branches, merged like other nodes), or local configuration like jj's `[revset-aliases]`?

---

## 11. Traceability to the task brief

| Brief item | Where |
|---|---|
| Syntax and semantics survey (patterns, variable-length, aggregation, mutation, parameters, subqueries, nulls) for GQL, SQL/PGQ, Cypher 25, Gremlin, SPARQL 1.1/1.2, the Datalog family, EdgeQL, TypeQL 3, SurrealQL, HelixQL, PRQL, jq / JMESPath, GraphQL, jj / git / hg / Fossil | §3.1–§3.3 |
| LLM writability evidence 2024–2026, error types, schema and few-shot effects, Claude reported separately, token length | §4, §5.3 |
| Six tasks in 4–6+ syntaxes with token counts and clarity | §5 |
| Conclusion: syntax family, must-have vs later | §1, §6, §7 |

---

## 12. Sources

**Standards and language documentation**
- ISO/IEC 39075:2024 GQL — https://www.iso.org/standard/76120.html
- Deutsch et al., *Graph Pattern Matching in GQL and SQL/PGQ* (SIGMOD 2022), finiteness rule for unbounded quantifiers — https://arxiv.org/abs/2112.06217
- GQL language guide, Microsoft Fabric (updated 2026-07) — https://learn.microsoft.com/en-us/fabric/graph/gql-language-guide
- GQL conformance, Microsoft Fabric — https://learn.microsoft.com/fr-fr/fabric/graph/gql-conformance
- Spanner Graph and ISO standards — https://docs.cloud.google.com/spanner/docs/graph/iso-standards
- Spanner Graph patterns — https://docs.cloud.google.com/spanner/docs/reference/standard-sql/graph-patterns
- gql-compat (independent conformance suite) — https://github.com/tamnd/gql-compat
- Neo4j Cypher Manual, GQL conformance (2026.06) — https://neo4j.com/docs/cypher-manual/current/appendix/gql-conformance/
- Neo4j Cypher Manual, additions and deprecations (Cypher 25) — https://neo4j.com/docs/cypher-manual/current/deprecations-additions-removals-compatibility/
- Neo4j MCP Cypher server — https://github.com/neo4j-contrib/mcp-neo4j/tree/main/servers/mcp-neo4j-cypher
- PostgreSQL 19 SQL/PGQ reverted (commit b1f106c8) — https://neon.com/postgresql/postgresql-19/sql-pgq-graph-queries ; https://git.postgresql.org/gitweb/?p=postgresql.git;a=commit;h=b1f106c80cbeb18d3a0219994d98a51a6eca8ede
- Kùzu archived / LadybugDB — https://thedataquarry.com/blog/from-kuzu-to-ladybug/ ; https://blog.ladybugdb.com/post/ladybug-spreading-its-wings/
- GraphLite (Rust GQL) — https://crates.io/crates/graphlite ; https://news.ycombinator.com/item?id=46121076
- TinkerPop 4.0 beta upgrade notes — https://tinkerpop.apache.org/docs/4.0.0-beta.2/upgrade/
- SPARQL 1.2 Query (WD 2026-09-21) — https://www.w3.org/TR/sparql12-query/ ; RDF 1.2 CR — https://www.w3.org/news/2026/w3c-invites-implementations-of-rdf-1-2-concepts-and-abstract-data-model-and-rdf-1-2-semantics
- Datomic transaction functions (`:db/cas`) — https://docs.datomic.com/transactions/transaction-functions.html
- XTDB — https://github.com/xtdb/xtdb
- CozoScript queries — https://docs.cozodb.org/en/latest/queries.html ; releases — https://github.com/cozodb/cozo/releases
- Logica — https://github.com/EvgSkv/logica ; LogicLM (EDBT 2025) — https://openproceedings.org/2025/conf/edbt/paper-314.pdf
- Datalevin — https://github.com/datalevin/datalevin
- CodeQL recursion / transitive closure — https://codeql.github.com/docs/ql-language-reference/recursion/
- Gel (EdgeQL) `for` reference — https://docs.geldata.com/reference/edgeql/for ; recursion issue — https://github.com/geldata/gel/issues/4168 ; Gel joins Vercel — https://www.geldata.com/blog/gel-joins-vercel
- TypeQL 3 functions — https://typedb.com/docs/typeql-reference/functions/
- SurrealQL idioms (recursive paths) — https://surrealdb.com/docs/surrealql/datamodel/idioms
- HelixDB legacy HQL notice — https://docs.helix-db.com/legacy/hql ; HelixDB README (v3 DSLs, JSON AST) — https://github.com/helixdb/helix-db
- PRQL `loop` — https://prql-lang.org/book/reference/stdlib/transforms/loop.html ; SQL pipe syntax (VLDB 2024) — https://vldb.org/pvldb/vol17/p4051-shute.pdf
- jq releases — https://github.com/jqlang/jq/releases ; JMESPath spec — https://jmespath.org/specification.html
- GraphQL specification editions — https://spec.graphql.org/
- Jujutsu revsets — https://docs.jj-vcs.dev/latest/revsets/ ; templates — https://docs.jj-vcs.dev/latest/templates/ ; grammar — https://github.com/jj-vcs/jj/blob/main/lib/src/revset.pest
- git revisions — https://git-scm.com/docs/gitrevisions
- Mercurial revsets — https://www.mercurial-scm.org/repo/hg/help/revsets
- Fossil technical overview — https://fossil-scm.org/home/doc/trunk/www/tech_overview.wiki

**LLM writability evidence**
- CypherBench — https://arxiv.org/abs/2412.18702
- Text2GQL-Bench — https://arxiv.org/abs/2602.11745
- SyntheT2C (the true source of 49.07 % / 50.42 %) — https://arxiv.org/abs/2406.10710
- Text2Cypher dataset paper (Ozsoy et al.) — https://arxiv.org/abs/2412.10064
- Neo4j Text2Cypher benchmarking (2024) — https://neo4j.com/blog/developer/benchmarking-neo4j-text2cypher-dataset/
- Mind the Query (EMNLP 2025 Industry) — https://aclanthology.org/2025.emnlp-industry.133.pdf
- LAST-CQ — https://arxiv.org/abs/2609.12746
- CYGNET — https://arxiv.org/abs/2606.04645
- Enhancing Text2Cypher with Schema Filtering — https://arxiv.org/abs/2505.05118
- Jackal (Text-to-JQL) — https://arxiv.org/abs/2509.23579 ; Agentic Jackal — https://arxiv.org/abs/2604.09470
- Assessing SPARQL capabilities of LLMs — https://arxiv.org/abs/2409.05925 ; LLM-KG-Bench 3.0 — https://arxiv.org/abs/2505.13098
- TEXT2SPARQL'25 — https://ceur-ws.org/Vol-4094/ ; https://text2sparql.aksw.org/2025/
- NL2GraphQL benchmark — https://dl.acm.org/doi/10.1145/3814574.3816745
- jqBench (ICLR 2026) — https://openreview.net/forum?id=VStKtgGXUc ; summary — https://en.papernotes.org/ICLR2026/llm_evaluation/jqbench_a_benchmark_for_reading_and_editing_json_from_natural_language_andor_exa/
- Text-to-Datalog preliminary evaluation — https://ceur-ws.org/Vol-4117/paper_aspocp_4.pdf
- SWE-AGI (MoonBit) — https://arxiv.org/abs/2602.09447
- text2ql — https://arxiv.org/abs/2609.02115
- Cloudflare Code Mode — https://blog.cloudflare.com/code-mode/
- Anthropic, Code execution with MCP — https://www.anthropic.com/engineering/code-execution-with-mcp
- Anthropic, Writing effective tools for agents (2025-09-11) — https://www.anthropic.com/engineering/writing-tools-for-agents
- Claude structured outputs — https://platform.claude.com/docs/en/build-with-claude/structured-outputs
- OpenAI GPT-5 CFG custom tools — https://cookbook.openai.com/examples/gpt-5/gpt-5_new_params_and_tools
- Opus 4.7 tokenizer change — https://www.anthropic.com/news/claude-opus-4-7

**Measurements in this session [M]**
- Stack Overflow tag counts via the Stack Exchange API (`/2.3/tags/{…}/info?site=stackoverflow`), 2026-09-26: neo4j 23,006; graphql 20,774; cypher 9,920; mercurial 8,256; jq 6,922; sparql 6,224; gremlin 3,635; jmespath 543; gql 542; jql 534; datomic 436; datalog 167; vaticle-typedb 132; surrealdb 111; edgedb 35; jujutsu 6; prql 1.
- Token-proxy probe: `queries.py`, `measure.py`, `results.json` (probe scripts are not published).
- Shell `#` comment check: Git-Bash `printf` and Windows PowerShell 5.1 function-argument test (§6.6).
