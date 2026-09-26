# 15 — Query execution for moirai's query language: low-RAM engines and what Rust implementations teach

*Research report for moirai. Date: 2026-09-26. Lens: **execution engines for graph queries with low RAM, and Rust implementations**. Status: research only. Nothing in moirai is implemented, and this file is the only change to the repository. Measurement probes lived outside the repository and are not published.*

**Trigger.** Owner requirement **R5** (2026-09-26): *"And there must also be a query language for our graph DB."* R5 overrides the position in [06 §12.2] that v1 would have only commands plus a filter syntax. This report does not design the language's syntax; that is another lens. It answers the question underneath it: **what engine executes that language inside moirai's constraints, and how should it be built?** The constraints are: single-digit-MB private RAM per process, O(1) open, no daemon, no threads, zero idle CPU, Windows 11 first, git-like branches, and agents as the main authors of queries.

**Tags.** **[M]** measured here (probe source and raw output are not published). **[D]** documented in a primary source (spec, paper, source code, official docs, crates.io/GitHub metadata checked on 2026-09-26). **[C]** a claim by a third party or a vendor. **[I]** my inference. Citations like `[30 §4.4]` point to other moirai research documents (`design/30-synthesis.md`, `06-…`, `07-…`).

---

## 0. Executive summary

1. **Build a small, hand-written, interpreted engine. Do not embed an existing query engine.** No 2026 Rust engine fits moirai's storage (mmap'd columnar segments + overlay + branch views), its RAM budget and its no-thread rule at the same time. CozoDB has been dormant since Dec 2024 [D]. Kùzu (C++) was archived on 2025-10-10 [D]. DataFusion, SurrealDB, Grafeo, Oxigraph and GlueSQL all assume their own storage and data model. What is worth borrowing is *ideas*: SurrealDB 3.0's AST → LogicalPlan → ExecutionPlan split [D]; Oxigraph's closure-compiled iterators with a cancellation token [D]; Kùzu's factorized, semijoin-filtered expansion [D]; Cozo's magic-set rewrite and stratification [D]; datafrog's tiny semi-naive core [D]; Feldera's split between "compiled standing views" and "interpreted ad-hoc queries over a snapshot" [D].

2. **Pipeline:** hand-written lexer → recursive-descent + Pratt parser → spanned AST → binder/type-checker against *schema-as-data* → logical algebra → rule-based rewrites → "cost-lite" physical planning → a pull-based, batch-at-a-time executor → streaming output writer. Parse + bind for a 144-byte query costs **~3.9 µs and 1.7 KiB of transient heap [M]**. There is no reason for plan caching, JIT or code generation.

3. **Execution model:** single-threaded, **pull-based vectorized** ("vector Volcano", batches of ~1024 ids), with **bitset set-algebra operators** used wherever a frozen bitset exists. Measured at 1e6 rows [M]:
   - per-row Volcano: 15–18 ms;
   - vectorized batches: 2.7–3.3 ms;
   - a tight hand loop: 4.8–5.5 ms;
   - AND of frozen bitsets: **12–14 µs**.

   Closure-compiling expressions (Oxigraph style) gains only 1.1–1.3× over a tree interpreter. Batching gains 5–7×. So the engine should interpret per batch, not per row, and never JIT. Pull fits `LIMIT`, cursors, per-batch budget checks and a no-thread process. DuckDB's reason for moving to push was parallel pipelines [D], which moirai does not have.

4. **Joins:** moirai's patterns are short, mostly acyclic and usually anchored (an id, a subtree, a status bitset). The right algorithm is **index-nested-loop expansion over the CSR from the most selective anchor, plus bitset semijoins**, a Yannakakis-style reduction that is nearly free over dense u32 ids. The anchor choice decides everything: on one 2-hop pattern the four plans differ **~100–150×** at 1e6 (0.23–0.30 ms vs 28–41 ms) [M]. For the rare cyclic pattern, add one **multiway sorted-intersection (Generic Join/LFTJ) operator** over the already-sorted CSR lists. It counted triangles **~9× faster than a pipelined binary hash join**, and with 0 extra heap vs 36 MiB [M]. A general WCOJ planner is not needed.

5. **Recursion:** built-in closure operators with set semantics and hard bounds. These are BFS/DFS over CSR with an edge-kind mask, a depth limit and a **reusable sparse-reset visited bitset** (N/8 bytes once per process: 122 KiB at 1e6).
   - A transitive-blockers query costs **~0.1–0.3 µs** [M].
   - A campaign subtree of 6.3k nodes costs **~0.05–0.15 ms** [M].
   - A worst-case whole-graph traversal at 1e6 costs **0.35–0.49 s and 4.1 MiB** [M], so it must hit a budget.

   Bottom-up (semi-naive) materialization of a recursive relation is the wrong default. The full `ancestor` closure at 1e6 costs **0.58–0.76 s and 37 MiB**, versus **11–16 µs for 1000 on-demand walks** [M]. User-defined recursive rules, if R5 needs them, must be demand-driven (magic sets / top-down seeding) and budgeted.

6. **Multi-source BFS (64 lanes, as DuckPGQ uses) is a RAM trap for moirai:** 24 B/node (**22.9 MiB at 1e6**) and slower than 64 sparse traversals when closures are small, as moirai's are: **~60× at 1e4 and ~300–800× at 1e5–1e6** [M]. Keep it out of v1.

7. **Aggregation and top-k:** array aggregation over small enum domains; popcount for counts; a bounded heap for `ORDER BY … LIMIT k`. A heap over the AND of two bitsets finds the top 20 of 1e6 in **1.1–1.6 ms with 205 B of heap**, versus 20–28 ms and 3 MiB to sort everything [M].

8. **Standing queries / IVM:** do not embed differential dataflow or DBSP. Both keep their state (arrangements, traces) resident in a long-lived process. Materialize cut its overhead from 96 B/update to 0–16 B/update [D], and DD users report ~100 B/record [C]. moirai has no daemon, and its short-lived CLI processes cannot rebuild that state per call. Use three tiers instead:
   - (a) fixed derived predicates (`ready`, `is_blocker`, counters, `suspect`) maintained in the write path and persisted. This is already the design [30 §3.5]. Measured at **~8–17 ns per status change** including dependents [M].
   - (b) filter subscriptions evaluated on the change-feed delta at read time, **~9 ns per (filter, changed id)** [M].
   - (c) recursive standing queries re-executed only when a delta hits their recorded read set (Salsa-style footprint invalidation).

   Feldera itself uses the same split: compiled incremental views, plus interpreted ad-hoc queries via DataFusion over a snapshot [D].

9. **Planning:** rule-based rewrites plus **exact cardinalities from the frozen bitsets**, whose popcounts cost µs [M]. This is a greedy anchor choice, not a DP enumerator. At 1e4–1e6 nodes with ≤ ~5 pattern variables, exact counts beat histograms, and there is no plan space worth searching.

10. **Safety is part of the engine, not the CLI.** Every query runs under a `Budget { steps, bytes, rows, depth, deadline }` and a cancel flag checked once per batch. It returns partial results with an explicit footer and a resumable cursor, and it can refuse up front when the exact-cardinality estimate exceeds the budget ("query too broad; add a filter"). Precedents include SQLite's per-N-instruction progress handler [D], Cozo's `:timeout` (default 300 s) [D], Neo4j's per-transaction memory limit [D], DataFusion's `MemoryPool` [D], Oxigraph's `CancellationToken` [D] and MCP's `notifications/cancelled` [D].

11. **Parser: hand-written.** The field has converged here: Ruff dropped LALRPOP for a hand-written RD parser (>2× faster, error recovery) [D]; SurrealDB 2.0 replaced nom with a hand-written RD parser "with better error messages" [D]; Grafeo's GQL parser is hand-written [D]. Chumsky is good at errors, but its 1.0 has been alpha since 2023 and the crate is labelled "minimal maintenance" [D]. The probe's ~500-line lexer+parser+binder produced rustc-style diagnostics, `did you mean` suggestions from the schema, and a JSON error object, at 2–18 µs per error [M].

12. **RAM and latency for typical queries** (engine only, warm, excluding the 15–190 ms process/shell overhead [30 §8.1]; see §13):
    - Most agent queries take **≤ 0.3 ms at 1e6** and **≤ 64 KiB of query heap** plus the one-time visited bitset.
    - Non-indexed scans take ~3 ms at 1e6.
    - Worst-case traversals and cyclic patterns take 0.15–1.4 s and 4–37 MiB unless budgeted.

    With default budgets of 4 MiB (CLI) / 8 MiB (MCP) and ~2e7 steps, a query process stays inside the private-RAM budget of [30 §8.1].

**What R5 changes in the synthesis.** The planned `find` filter syntax and the purpose-built verbs (`ready`, `blocking`, `blockers --transitive`, `tree`) become **front-ends to the same executor**. Each verb is a named, pre-planned query with a fixed output contract, so there is one engine and one set of indexes. The MCP `find` tool takes the query language. The derived-state machinery in [30 §3.5] is unchanged and becomes the "compiled view" tier.

---

## 1. What the engine must execute: moirai's query workload

The language lens will decide syntax. The engine lens needs the *shapes* of queries and their sizes. They come from the verbs and use cases in [30 §7.1], [06 §12.1] and [07], plus what a query language adds.

| Class | Example (informal) | Shape | Typical input → output size at 1e6 | Frequency (agents) |
|---|---|---|---|---|
| Q1 point / id list | `#40`, `#12 #17 #31` with fields | id lookup | 1–100 rows | very high |
| Q2 indexed filter | `task where status=open and prio<=1` | bitset AND/OR/ANDNOT | 1e6 → 1e3–1e5, paged to 20 | very high |
| Q3 filter on a non-indexed field | `where updated > c4400`, `where acceptance contains "WAL"` | column / field-block scan | 1e6 → few | medium |
| Q4 ordered page | `… order by prio, updated desc limit 20` | filter + top-k | 1e5 → 20 | high |
| Q5 derived-state reads | `ready`, `blocking`, `is_blocker`, `suspect` | maintained bitsets | → 20 / ids | very high |
| Q6 anchored traversal | transitive blockers of `#51`, subtree of `#88`, ancestors | bounded closure over 1–3 edge kinds | 1 → 1–1e4 | high |
| Q7 anchored pattern (acyclic) | open findings `about` tasks under `#88`; rules `applies_to` a role and `cites` a superseded decision | 2–4 hop join with filters on each variable | anchor 1–1e4 → 1–1e3 | medium, rising with R5 |
| Q8 aggregate | count by (kind, status); per-campaign rollups; refuted share per critic | group-by over small domains | 1e6 → ≤ 100 groups | medium |
| Q9 cyclic pattern | "decisions that supersede a decision cited by a rule that cites the new one" | triangles / 4-cycles | rare | rare |
| Q10 unanchored recursion | reachability between arbitrary sets; "everything connected to X" | whole-graph traversal | 1e6 | rare, dangerous |
| Q11 versioned variants | any of the above `--branch R`, `@commit`, `--across R1,R2`, `diff A..B` | same plan over a different view | same | medium |
| Q12 standing | "tell me when anything under `#88` becomes ready"; "new critical rules for role X" | incremental re-evaluation over deltas | delta 1–1e3 ids | per hook / per call |

Three facts drive the design:

- **Almost everything is anchored or served by an index that already exists.** Frozen bitsets per kind, per (kind, status), `ready`, `is_blocker` and the flags [30 §4.4] turn Q2/Q5 into word-wise bit operations. Forward and reverse CSR sorted by (kind, dst) turn Q6/Q7 into slice walks [I].
- **Output is small because agents read it.** Pages of ~20, and results capped at ≈ 8k tokens [07]. This favours streaming, early termination, top-k and cursors over full materialization [I].
- **Queries are written by LLMs.** Some will be wrong, some will be broad, and some will be unbounded. Safety (§9) and diagnostics (§2.4) are first-class requirements, not polish. Text2Cypher execution accuracy was about 50% for GPT-4 [D, from 06]. Even if 2026 models do much better, the engine must assume a steady stream of malformed and over-broad queries [I].

---

## 2. Compilation pipeline for a small embedded engine

### 2.1 Stages and what each costs

| Stage | Output | Why it exists in moirai | Cost |
|---|---|---|---|
| Lex | tokens with byte spans | spans feed every diagnostic; hand-written, byte-level | part of 3.9 µs [M] |
| Parse (RD + Pratt for expressions) | AST with spans | recursive descent handles statements, Pratt handles operator precedence; error recovery at stage boundaries (`\|`, `,`, `)`) | part of 3.9 µs [M] |
| Bind / type-check | typed IR: kinds → u8, fields → column or field-block ids, enum literals → integers, `#N` → row ids, edge names → edge-kind masks | schema is data [30 §2.12], so binding is a lookup in the store's schema version; every unknown name gets `did you mean` | part of 3.9 µs [M] |
| Logical plan | small algebra: `Scan(kind)`, `Filter`, `Expand(edge, dir, min..max)`, `Pattern(vars, edges)`, `Aggregate`, `TopK/Sort`, `Limit`, `Union/Except`, `AsOf/Across` | lets rewrites and verbs share one IR | µs [I] |
| Rewrite (rule-based) | normalized plan: NNF/DNF, filter pushdown, `ORDER BY+LIMIT → TopK`, `COUNT(*) → popcount`, predicate → bitset access path, derived predicate → maintained bitset (`ready`), constant folding | deterministic and explainable | µs [I] |
| Physical plan ("cost-lite") | operator tree with access paths, anchor choice, semijoin placement, budgets pre-checked | uses exact counts from bitsets (§8) | ≤ tens of µs even at 1e6 (popcount of one set ≈ 12–14 µs [M]) |
| Execute | stream of batches → output writer | pull, batch-at-a-time, budget/cancel check per batch | §3–§7 |

**Measured [M]:** the probe's lexer + parser + binder for
`task where status = open and prio <= 1 and not done | follow <- blocks *1..5 | where status != done | order by prio asc, updated desc | limit 20`
(144 bytes) takes **3,869 ns per query with 1,720 B peak transient heap**, on a machine at 97–100 % CPU load from other agents. The error path takes 2–18 µs; the Levenshtein suggestion dominates. At these costs, **plan caching, prepared statements and code generation have no payoff** in a CLI whose process spawn costs 15–73 ms [30 §8.1] [I]. One consequence: the MCP server can accept query text on every call without keeping per-query state.

### 2.2 What the Rust systems do

| System | Pipeline | Notes for moirai |
|---|---|---|
| **SurrealDB 3.0** (2026) | "AST → LogicalPlan → ExecutionPlan", "fully streaming internally"; at 3.0 the new executor covers **read-only statements only** [D](https://surrealdb.com/blog/surrealdb-3-0-benchmarks-a-new-foundation-for-performance); later releases added a planner-strategy option and closed remaining gaps [D](https://github.com/surrealdb/surrealdb/pull/6997) | A mature Rust DB rebuilt its executor in the textbook three-stage shape and started with reads. moirai can do the same: the QL is read-only first; writes stay verbs (open question Q1). |
| **GlueSQL** 0.20 (Aug 2026) | sqlparser-rs AST → new `StatementPlan` "execution-facing representation"; aggregates "receive their execution slots during planning" [D](https://github.com/gluesql/gluesql) | Resolve slots and columns at bind time, not per row. |
| **Oxigraph** (spargebra → sparopt → spareval) | spargebra now parses with **chumsky + logos** [D](https://github.com/oxigraph/oxigraph/blob/main/lib/spargebra/Cargo.toml); sparopt is a "work in progress" optimizer [D](https://docs.rs/sparopt/latest/sparopt/); spareval compiles the plan into a tree of closures `Rc<dyn Fn(Tuple) -> Box<dyn Iterator>>`, uses hash joins (`HashBuildLeftProbeRight`) and a `CancellationToken(Arc<AtomicBool>)` checked on every scanned quad [D](https://github.com/oxigraph/oxigraph/blob/main/lib/spareval/src/eval.rs) | Closure compilation is simple and works, but it is row-at-a-time: the probe measured only 1.1–1.3× over plain interpretation (§3.2). The cancellation-token pattern is exactly right. |
| **CozoDB** 0.7.6 | CozoScript (pest grammar + miette diagnostics [D](https://github.com/cozodb/cozo/blob/main/cozo-core/Cargo.toml)) → DNF → stratification by SCC → **magic-set rewrite** → **semi-naive** bottom-up; atoms reordered so filters run as early as their bindings allow; "rows are generated in a streaming fashion"; `:limit` stops early only without `:order` [D](https://docs.cozodb.org/en/latest/execution.html) | The Datalog playbook in ~one page. Dormant: last commit 2024-12-04, last release Dec 2023 [D](https://github.com/cozodb/cozo); dbdb.io calls it abandoned [C](https://dbdb.io/db/cozodb). |
| **Grafeo** 0.5.42 (2026) | six front-ends (GQL, Cypher, Gremlin, GraphQL, SPARQL, SQL/PGQ) → "unified logical plan"; "cost-based optimizer with DPccp join ordering and histograms"; "push-based vectorized execution", morsel parallelism, factorized processing, Ring-index WCOJ for SPARQL [D/C](https://github.com/GrafeoDB/grafeo); **hand-written GQL lexer and parser** (`parser.rs` 406 KB) [D](https://github.com/GrafeoDB/grafeo/tree/main/crates/grafeo-adapters/src/query/gql) | The most complete 2026 Rust graph engine. It is built for throughput with threads, which moirai forbids. Its SF0.1 memory claims (43–136 MB [C]) are far above moirai's budget. |
| **HelixDB** v3 | HelixQL, a query language compiled to Rust handlers at deploy time, was **deprecated in v2** ("Queries are now written with the Rust DSL" [D](https://github.com/HelixDB/hql-v1-docs)); v3 sends DSL queries to a running instance "no build or deploy step" [D](https://github.com/HelixDB/helix-db) | A compile-at-deploy query language failed product-wise because users needed ad-hoc queries. Agents need ad-hoc queries even more. |
| **GraphLite** 0.0.1 | ISO GQL "based on grammar optimized from OpenGQL", sled storage, "cost-based query optimization" [C](https://github.com/GraphLite-AI/GraphLite); 232 stars, last push Feb 2026 [D] | Shows how large a full GQL front end is. Not a dependency candidate. |
| **IndraDB** 5.0 (Aug 2025) | no text language; a `Query` enum tree (`Pipe`, `PipeProperty`, `Include`, `Count` …) with a per-pipe `limit` [D](https://docs.rs/indradb-lib/latest/indradb/struct.PipeQuery.html) | A builder-API query tree is a reasonable *internal* IR, and its per-step limit is a good safety primitive. |

### 2.3 Parser approach and error quality

Agents correct a query from the error message alone. They cannot read a grammar file or step through a parser. The error must name the exact span, what was expected, the nearest valid name, and one concrete fix [I].

| Approach | Status (crates.io/GitHub, 2026-09-26) | Error quality | Speed / size | Evidence |
|---|---|---|---|---|
| **Hand-written lexer + recursive descent + Pratt** | — | Best: every error site is code, so it can name the construct being parsed, suggest fixes and add schema-aware hints; recovery at chosen sync tokens | Fastest; no dependency; small code | Ruff replaced its LALRPOP parser with a hand-written RD parser: ">2x faster", error recovery "especially important for building editor-friendly tools" [D](https://astral.sh/blog/ruff-v0.4.0). SurrealDB 2.0: "optimised recursive descent parser with a separate lexing step", "better error messages" than its nom-based predecessor [D](https://surrealdb.com/blog/challenge-accepted-announcing-surrealdb-2-0). rustc, rust-analyzer and Grafeo's GQL parser are hand-written [D]. |
| **chumsky** (PEG combinators with recovery) | stable line 0.13.0 (May 2026); `1.0.0-alpha.8` (Jan 2025) unreleased as 1.0 since 2023; repo moved to Codeberg; lib.rs label "minimal maintenance" [D](https://lib.rs/crates/chumsky) | Very good, with built-in recovery strategies; "no silver bullet" [D] | "a little slower than a well-optimised hand-written parser" [D]; heavy generics mean slower compiles [I] | Adopted by Oxigraph's spargebra (with logos) [D] |
| **pest** (PEG grammar file → parse tree) | 2.9.2 (Sep 2026) [D] | Generic "expected X" positives; needs a separate AST-building pass | fine | CozoScript [D] |
| **LALRPOP** (LR(1) generator) | 0.23.1 (Mar 2026) [D] | Poor recovery (Ruff's reason to leave) | good | Ruff (until v0.4) [D] |
| **winnow** (nom successor) | 1.0.4 (Jul 2026) [D] | "context" labels; recovery is manual | fast | — |
| **logos** (lexer generator) | 0.16.1 [D] | n/a (lexer) | very fast | Oxigraph [D] |
| Diagnostic renderers: **annotate-snippets** (rustc's default emitter on nightly since 2025-11-07 [D](https://github.com/rust-lang/rust/pull/148188)), ariadne 0.6, miette 7.6, codespan-reporting 0.13 [D] | — | rustc-style snippets | dependency | — |

**Probe [M]** (hand-written, ~500 lines including the binder; output copied verbatim from `ql_demo.txt`):

```
error[E101]: unknown field `stauts` on kind `task`
 --> 1:12
  |
1 | task where stauts = open
  |            ^^^^^^ did you mean `status`?
  = help: fields of task: status, prio, updated, done, title
  json: {"code":"E101","span":[11,17],"msg":"unknown field `stauts` on kind `task`","suggest":"status"}

error[E001]: expected `|`, `and`, `or` or end of query, found `limit`
  |
1 | task where status = open limit 5
  |                          ^^^^^ expected `|`, `and`, `or` or end of query
  = help: pipeline stages are separated by `|`: write `| limit`

error[E102]: `opne` is not a value of `status`      -> did you mean `open`?  (status is one of: open | in_progress | done | cancelled)
error[E103]: type mismatch: `prio` expects an integer 0..=4, found a string
error[E104]: unknown edge kind `blokcs`              -> did you mean `blocks`?
error[E106]: 9 is out of range for `prio` (0..=4)
```

**Recommendation [I]:**
- Use a hand-written byte lexer and a recursive-descent parser with Pratt expressions and panic-mode recovery at stage separators, so several errors can be reported in one pass. Estimated at 1.5–3k lines for a moderate language.
- Render diagnostics in-house for plain text, since the CLI prints no ANSI off-TTY [30 §7.1]. Add annotate-snippets only if colour is ever wanted.
- **Freeze error codes as part of the output contract.** Every error has a stable code, a byte span, a message, an optional `suggest`, and a `help` that lists the valid alternatives from the *current schema version*.
- Emit the same object as JSON in MCP results.
- Keep the grammar in an EBNF file that tests check against the parser, because no generator enforces it.

### 2.4 Diagnostics are partly a binder job

Most agent mistakes the probe reproduced are *semantic*, not syntactic: unknown field, wrong enum value, wrong kind, out of range, type mismatch. The binder is the only stage that knows the schema, so it must produce the actionable message. Three rules [I]:

1. **Suggest from the schema, never from the grammar.** Suggestions cover fields of the bound kind, the enum values of that field, edge kinds, and kind names, with Levenshtein ≤ 2 as in the probe.
2. **Errors from the planner are also diagnostics.** Examples: "unbounded traversal over `relates` would visit ~1.0e6 nodes (budget 1e5); add a depth bound or an anchor", or "ORDER BY without LIMIT over 55k rows; add `limit`". Exact cardinalities make these cheap and honest (§8, §9).
3. **Warnings do not block.** For example, "`done` is virtual; using `status in (done, cancelled)`". They appear in the footer, the same place as drop footers [30 §7.1].

---

## 3. Execution models and their RAM behaviour

### 3.1 The literature in one table

| Model | Idea | RAM behaviour | Who uses it |
|---|---|---|---|
| **Volcano / iterator (pull, tuple-at-a-time)** | every operator has `next()` [D](https://doi.org/10.1109/69.273032) | minimal: one tuple in flight per operator; blocking operators (sort, hash build) materialize | SQLite VM (bytecode, row-at-a-time), Oxigraph spareval |
| **Vectorized (pull or push, batch-at-a-time)** | `next()` returns ~1–2k values + a selection vector; per-batch interpretation amortizes dispatch [D](https://www.cidrdb.org/cidr2005/papers/P19.pdf) | one batch per operator (KiBs) | DuckDB, DataFusion, Kùzu, Grafeo |
| **Push-based pipelines** | source pushes batches through operators to a sink; easier parallel scheduling; DuckDB switched from pull to push in 2021 for parallelism [D](https://github.com/duckdb/duckdb/pull/2393) | same as vectorized | DuckDB, Grafeo |
| **Data-centric compiled (LLVM / codegen)** | fuse a pipeline into one tight loop [D](https://www.vldb.org/pvldb/vol4/p539-neumann.pdf) | minimal, but compile latency is ms–s | HyPer, Umbra; Feldera compiles SQL → Rust ahead of time [D](https://github.com/feldera/feldera/tree/main/sql-to-dbsp-compiler) |
| **Comparison** | compiled and vectorized are both efficient; neither dominates [D](https://www.vldb.org/pvldb/vol11/p2209-kersten.pdf) | — | — |

The RAM difference between these models is small; what they share matters more [I]. **Pipelined operators hold a batch. Pipeline breakers (sort, hash-join build, group-by by id, distinct) hold their whole input.** In moirai, RAM control therefore means (a) avoiding pipeline breakers whose input is O(N) or O(E), and (b) capping the rest with a budget. The CSR already is a join index, so most "hash builds" disappear (§4). The exceptions are top-k instead of sort (§6), fixed arrays instead of hash group-by for enum domains (§6), and bitsets instead of hash sets for id sets (§5).

### 3.2 Measured: one predicate, five executors

Setup [M]: synthetic typed graph with dense u32 ids and columnar `kind/status/prio/updated/parent`. Forward and reverse CSR are sorted by (kind, dst). There are **2.01 distinct edges/node**, stored twice, with hub in-degree up to 6,052 at 1e6. The predicate is `kind = task ∧ status = open ∧ prio ≤ 1` (4.8 % selectivity). Numbers are medians from high-priority runs, with the minimum over all runs in brackets. The machine (Ryzen 9 5900HS, Windows 11) ran at **97–100 % CPU load** from other agent processes throughout, so absolute times are pessimistic and variance is large (see §12).

| Executor | 1e4 | 1e5 | 1e6 | ns/row @1e6 | heap |
|---|---:|---:|---:|---:|---:|
| F1 hand-written tight loop over 3 columns | 12.6 µs (10.7) | 523 µs (471) | 5.5 ms (4.8) | ~5 | 0 |
| F2 Volcano, `Box<dyn Op>`, interpreted expression tree per row | 186 µs (134) | 1.9 ms (1.6) | 18.2 ms (17.4) | ~18 | 0 |
| F3 Volcano, closure-compiled expression (Oxigraph style) | 152 µs (96) | 1.6 ms (1.3) | 15.5 ms (14.8) | ~15 | 205 B |
| F4 **vectorized**: 1024-id batch + selection vector, one primitive loop per predicate per batch | 26.7 µs (22.4) | 274 µs (214) | **3.3 ms (2.3)** | ~3 | 0 |
| F5 **AND of three frozen bitsets + popcount** | 0.2 µs (0.1) | 1.4 µs (1.2) | **14.2 µs (12.3)** | ~0.014 | 0 |

**Reading [I]:**
- Per-row dynamic dispatch costs ~15–20 ns/row. Batching removes 80 % of that. Closure compilation removes almost none of it.
- The vectorized loop beat the naive tight loop at 1e5/1e6. It is branch-free (`k += (c[id] == v)`), while the short-circuit `&&` loop mispredicts.
- The bitset path is **~200–400× faster** than any scan. Every predicate that has a maintained bitset should compile to bitset algebra, and the planner should prefer maintaining a bitset over optimizing a scan.

### 3.3 Choice for moirai

- **Single-threaded, pull-based, batch-at-a-time** [I]. Pull gives early termination for `LIMIT`/pages (`ready` page: **~1 µs at every size** because it stops after 20 hits [M]), natural cursors (ids are ordered, so "resume after `#N`" is exact), and a natural point to check budget and cancel flags. Push's advantage is parallel scheduling [D](https://github.com/duckdb/duckdb/pull/2393), and moirai's process model has no threads [30 §2.2].
- **Three data representations flow between operators:**
  1. `IdSet`: a dense bitset, sparse sorted u32 list, or roaring-like chunked container matching the segment format [30 §4.4]; used for set algebra and semijoins.
  2. `Batch`: up to 1024 row ids + selection vector + lazily fetched columns.
  3. `Factorized`: (parent row, CSR slice borrowed from the mapping) for 1-to-many expansion, so a node with 6,000 children is one entry, not 6,000 rows. This is Kùzu's "unflat" vector group [D](https://www.cidrdb.org/cidr2023/papers/p48-jin.pdf).
- **Late materialization:** titles, field blocks and bodies are fetched only by the final projection for rows that survive `LIMIT`. Strings are borrowed from the map; bodies decompress into the per-request bump arena [30 §4.7].
- **Merge-on-read under branches:** a scan over a branch view iterates base columns and applies the overlay's sorted patch lists per batch, as `(base ∧ ¬removed) ∨ added` for bitsets and row replacement for columns. The cost is proportional to the overlay (≤ 4,096 ops [30 §4.5]), not to N [I].
- **No JIT, no codegen, no SIMD intrinsics in v1.** The branch-free per-batch loops auto-vectorize [I].

---

## 4. Pattern matching: binary joins, worst-case optimal joins, factorization

### 4.1 State of the art (2025–2026)

- **Binary join plans** can produce intermediates asymptotically larger than the output on cyclic queries. **WCOJ algorithms** bound work by the AGM bound. Leapfrog Triejoin does so by intersecting sorted tries one variable at a time [D](https://arxiv.org/abs/1210.0481), and Generic Join is the general form [D](https://arxiv.org/abs/1310.3314). On acyclic queries WCOJ was historically slower than good binary plans [D](https://arxiv.org/abs/2301.10841).
- **Unification.** Free Join (SIGMOD 2023) unifies both. Its implementation is a Rust library that takes DuckDB's binary plan and "matches or outperforms both" [D](https://arxiv.org/abs/2301.10841). A 2025 unified architecture reports up to 3.1× over Generic Join and up to 4.8× over Free Join [C](https://arxiv.org/abs/2505.19918). **Umbra** builds hash tries at runtime and switches to WCOJ only where the optimizer predicts growing intermediates [D](https://www.vldb.org/pvldb/vol13/p1891-freitag.pdf). **GraphflowDB → Kùzu** mixed binary and WCOJ plans [D](https://arxiv.org/abs/1903.02076).
- **Kùzu** made **factorization** central. m-n join intermediates are kept as Cartesian products of vector groups ("flat" vs "unflat"). ASP-Join (accumulate → semijoin filter → probe) passes semijoin filters sideways so scans read only the needed rows, and ASP-Join is also the core of its multiway WCOJ [D](https://www.cidrdb.org/cidr2023/papers/p48-jin.pdf), with the reasoning in its blog posts [D](https://blog.kuzudb.com/post/factorization/), [D](https://blog.kuzudb.com/post/wcoj/). Kùzu was archived on 2025-10-10 (v0.11.3) after the team joined Apple [D](https://github.com/kuzudb/kuzu)/[C](https://thedataquarry.com/blog/from-kuzu-to-ladybug/). The **LadybugDB** fork is active (crate `lbug` 0.20.4, 2026-09-10) [D](https://github.com/LadybugDB/ladybug).
- **Acyclic queries: semijoin reduction is back.** Yannakakis runs in O(N + OUT). Yannakakis+ (SIGMOD 2025) makes it practical (average speed-up 2.41×) [C](https://arxiv.org/abs/2504.03279). Robust Predicate Transfer in DuckDB makes acyclic queries almost insensitive to join order (max/min time over random orders 1.6×) [C](https://arxiv.org/abs/2502.15181).
- **Graph extensions of relational engines.** DuckPGQ translates SQL/PGQ to relational plans and builds in-memory CSR for path operators, using vectorized **multi-source BFS** with one bit per concurrent search [D](https://www.vldb.org/pvldb/vol16/p4034-wolde.pdf), [D](https://duckpgq.org/). It is still labelled a research project [D](https://duckdb.org/community_extensions/extensions/duckpgq).
- **Newest.** Filters folded natively into WCOJ over a compact Ring index (Aug 2026) [C](https://arxiv.org/abs/2608.03840).

### 4.2 moirai's patterns: anchored, short, acyclic → INL + bitset semijoins

In moirai, every node id is a dense u32 and every edge kind has a sorted CSR slice in both directions [30 §4.4]. Under these conditions:
- A **semijoin** is a bitset probe (1 load + 1 AND).
- An **index nested loop** is a slice walk with no build side.
- **Exact cardinalities** of every indexed predicate come from a popcount.

**Measured [M]** on "open findings `about` tasks inside the subtree of a campaign hub". Subtree size: 840 / 734 / 6,330; open findings: ~1.5k / 14k / 140k.

| Plan | 1e4 | 1e5 | 1e6 | heap @1e6 |
|---|---:|---:|---:|---:|
| P4 **INL from the selective side**: subtree(hub) → reverse `about` → filter `status=open` | **10.4 µs** | **10.2 µs** | **0.30 ms (0.23)** | 0 |
| P1 semijoin: subtree → bitset; scan all findings, probe the bitset | 24.6 µs | 424 µs | 12.7 ms (5.7) | 32 KiB |
| P3 hash join: `HashSet` of subtree ids; scan findings, probe | 105 µs | 825 µs | 15.6 ms (9.8) | 40 KiB |
| P2 naive nested loop: per open finding, walk its task's parents to see if the hub is an ancestor | 23.5 µs | 1.9 ms | 40.7 ms (28.3) | 0 |

The wrong plan costs **~20–150×** at 1e6. The right plan is obvious *if the planner knows the sizes*: subtree(hub) ≈ 6.3k vs open findings ≈ 140k. Exact counts (popcounts) and cheap bounded probes (walk the subtree up to a cap) give the planner those sizes (§8). The design is:

- **Anchor** on the variable with the smallest exact candidate set: literal ids < subtree/closure of a literal < indexed bitset < scan.
- **Expand** along pattern edges using CSR slices (factorized).
- **Semijoin-filter** far variables with a bitset when their candidate set is already materialized and small relative to the expansion. This is the Kùzu ASP-Join idea [D] and the Yannakakis idea [D].
- **Check** remaining predicates per batch.

This is a greedy Yannakakis-lite. For ≤ 5 variables, an exhaustive order check (≤ 120 orders × µs) is also affordable if greedy proves brittle [I].

### 4.3 Cyclic patterns: one multiway-intersection operator

**Measured [M]:** triangle counting over the undirected simple graph of all edges, degree-oriented. 976 triangles at 1e6. Pipelined binary plan with 2.7M oriented 2-path intermediates; an unoriented plan would have 106M.

| Plan | 1e4 | 1e5 | 1e6 | heap @1e6 |
|---|---:|---:|---:|---:|
| C1 sorted-list intersection (Generic Join / LFTJ on 2 iterators) | 0.42 ms | 10.5 ms | **165 ms (142)** | **0** |
| C2 pipelined binary hash join (build `HashSet<edge>`, enumerate 2-paths, probe) | 1.6 ms | 59 ms | 1,439 ms (1,284) | **36 MiB** |

Moirai's CSR lists are already sorted by (kind, dst), so the intersection operator needs **no build side** and allocates nothing. It is ~9× faster and avoids an O(E) hash table that would break the RAM budget on its own [M]. **Recommendation [I]:** implement one `Intersect(k lists)` operator (leapfrog over k sorted slices) and use it whenever a pattern variable is constrained by ≥ 2 already-bound neighbours, i.e. whenever the pattern closes a cycle. That is the whole WCOJ story moirai needs; a general attribute-order optimizer (Umbra, Free Join, ADOPT) is out of scope.

### 4.4 Factorization for tokens, too

Factorized results are also the token-efficient output format: `#88 children(6329): #901 #902 …` instead of 6,329 rows repeating `#88`. The output writer should keep the (parent, slice) shape when the query projects a parent with its neighbours. The line-oriented contract [30 §7.1] already expects one line per record, with neighbour lists inline [I].

---

## 5. Recursion: closure, bounded traversal, semi-naive, magic sets

### 5.1 Semantics decide termination

- In GQL and SQL/PGQ, **WALK** is the default path mode. Patterns whose results could be infinite (unbounded quantifiers under WALK) must use a **restrictive** mode (TRAIL, ACYCLIC, SIMPLE) or a **selective** prefix (ANY, ANY SHORTEST, ALL SHORTEST) [D](https://arxiv.org/abs/2112.06217).
- Kùzu's default for variable-length relationships was WALK, capped by `VAR_LENGTH_EXTEND_MAX_DEPTH` = 30. Its community documents row and memory explosions: "10 outgoing edges per node … depth 5 yields 100,000" paths [C](https://oneuptime.com/blog/post/2026-08-12-kuzu-variable-length-traversal-explosion/view).
- DuckDB added `USING KEY` recursive CTEs (v1.3, 2025). They treat the recursive state as a keyed dictionary, so reachability and shortest paths terminate on cycles without path tracking [D](https://duckdb.org/2025/05/23/using-key). The v2.0 rework keeps epoch-invariant state and probes frozen keyed state directly: 42.6× on a reachability query, LDBC path-finding peak memory 3.9 → 2.7 GB [D/C](https://duckdb.org/2026/08/25/how-duckdb-runs-recursive-ctes-faster).

**For moirai [I]:** the default is **set semantics (reachability)**: "the set of nodes reachable via `blocks` within ≤ d hops". This terminates on any graph and is linear in the visited subgraph. Path *enumeration* is allowed only with an explicit selector (`shortest`, `any`) or a small explicit bound and a row budget. The DAG edges (`blocks`, `parent`, `supersedes`, `derived_from`) are acyclic by invariant [30 §3.4], which makes set and path semantics agree on reachability, but not on counts.

### 5.2 Algorithms and measurements

| Operator | Algorithm | Measured [M] (1e4 / 1e5 / 1e6) | Query heap |
|---|---|---|---|
| Transitive blockers of one task (reverse `blocks`, mean closure 2.7–3.7, max 54) | DFS + **reusable sparse-reset bitset** (dense N/8 bits, reset only the touched words) | 1000 queries: 106 µs / 187 µs / 235 µs → **~0.1–0.3 µs per query** | 0 per query; N/8 once per process (1.2 / 12 / 122 KiB) |
| same | fresh dense bitset per query | 1000 queries: 0.3 / 0.7 / 5.0 ms (zeroing N/8 dominates at 1e6) | 1.3 / 12 / 122 KiB per query |
| same | `HashSet<u32>` per query | 1000 queries: 0.6 / 0.7 / 2.6 ms | ~0.5 KiB |
| 64 such closures at once | MS-BFS, 64 lanes, `seen/frontier/next: [u64; N]` [D](https://www.vldb.org/pvldb/vol8/p449-then.pdf) | 352 µs / 3.5 ms / 3.0 ms | **235 KiB / 2.3 MiB / 22.9 MiB** |
| same 64 | 64 × sparse-reset DFS | **5.9 / 4.9 / 3.9 µs** | 0 |
| Subtree of a campaign hub (840 / 734 / 6,330 nodes) | level BFS over reverse `parent` | 5.4 µs / 4.9 µs / 118 µs | 4 / 4 / 32 KiB (result list) |
| Subtree, depth ≤ 2 | same, bounded | 1.0 / 1.5 / 13 µs | 0 |
| Ancestors (≤ 12 hops), 1000 queries | parent-column walk | 4.4 / 9.4 / 16 µs | 0 |
| **Worst case:** undirected reachability from `#0` over all edges | BFS, dense bitset + queue | 0.4 ms / 37.5 ms / **486 ms (350)** | 65 KiB / 0.5 MiB / **4.1 MiB** |
| **Bottom-up** semi-naive `anc(x,y) :- parent(x,y). anc(x,z) :- anc(x,y), parent(y,z).` (datafrog-style sorted `Vec<u64>` relations) | full materialization | 2.0 ms / 70 ms / **761 ms (584)**; 22.6k / 243k / **2.41M tuples** | 354 KiB / 3.7 MiB / **37 MiB** |

**Reading [I]:**
1. The **reusable sparse-reset bitset** is the right visited-set primitive. It beats a hash set by 5–10× and a fresh bitset by 3–20×. It costs N/8 bytes once, allocated lazily only by the first traversal in a process. The mmap'd segment format is also dense by id, so the bitset maps 1:1 onto rows.
2. **MS-BFS is wrong for moirai's closures.** They are tiny (mean < 4) and rarely overlap. MS-BFS pays O(N) state per query: 24 B/node, about a quarter of the entire hot index (~99 B/node [30 §4.4]). Keep it for analytics, which is out of scope.
3. **Whole-graph traversals cost 0.35–0.5 s and 4 MiB at 1e6** on a random-locality graph. Real moirai ids are allocated in creation order and edges mostly point to recent nodes, so locality will be better, but this is the query class that budgets exist for (§9).
4. **Bottom-up recursion materializes what nobody asked for.** 2.41M `anc` tuples and 37 MiB, versus µs for the on-demand walks agents actually run. This is exactly what magic sets fix [D](https://docs.cozodb.org/en/latest/execution.html): seed the recursion with the query's bound constants, so it only derives facts relevant to `#N`. Direction-optimizing BFS [D](https://doi.org/10.1109/SC.2012.50) is irrelevant at these frontier sizes.

### 5.3 Recommendations

- **v1:** built-in closure operators: `Expand*` (edge-kind mask, direction, min..max depth, set semantics), `Ancestors`, `Subtree`, `ShortestPath` (bounded BFS with parent pointers, returns one path), `Reachable(a, b)` (bidirectional bounded BFS).
  - Default depth 12 (the hierarchy limit [30 §3.3]), maximum 64.
  - Visited-node budget 1e5 by default; the overflow returns partial results with a footer.
- **If R5 wants user-defined recursive rules or views** (Datalog-like `rule blocked_by_transitively(x, y) := …`): use semi-naive evaluation over sorted u32/u64 relations (datafrog's ~1k lines are a proven template [D](https://github.com/rust-lang/datafrog)), always after a **magic-set / demand transformation**, with stratified negation and aggregation (Cozo's pipeline [D]), and always under the same budget. Compile-time Datalog macros (ascent [D](https://github.com/s-arash/ascent), crepe [D](https://github.com/ekzhang/crepe)) cannot run *runtime* queries. They could express the engine's *fixed* derived rules, but those are simpler written by hand next to the write path (§7) [I].

---

## 6. Aggregation, ordering, top-k, pagination

**Measured [M]:**

| Query | 1e4 | 1e5 | 1e6 | heap @1e6 |
|---|---:|---:|---:|---:|
| T1 top-20 open tasks by (prio, −updated), heap over a full scan | 24.6 µs | 668 µs | 7.5 ms | 205 B |
| T2 same, materialize all matches + sort | 76 µs | 2.1 ms | 28 ms | **3.0 MiB** |
| T3 same, **iterate AND(task, open) bitset** + heap | **9.0 µs** | **85 µs** | **1.6 ms (1.1)** | 205 B |
| A1 count by (kind, status) into a fixed 8×4 array | 16.7 µs | 135 µs | 1.5 ms | 0 |
| R2 count of `ready` via maintained counters (scan) | 40 µs | 1.2 ms | 14.9 ms | 0 |
| R3 same, recomputed from edges (no maintained state) | 215 µs | 5.7 ms | 101 ms | 0 |
| R1 first page of 20 `ready` tasks (early termination) | 2.0 µs | 1.2 µs | 1.1 µs | 0 |

**Rules [I]:**
- `ORDER BY … LIMIT k` becomes **TopK(k)**, a bounded heap. Sort-without-limit is allowed only when the exact input count is ≤ a budgeted cap.
- `COUNT(*)` over indexed predicates is a **popcount**. Group-by over enum columns or kinds is a **fixed array**. Group-by over node ids (per-campaign rollups) is a hash map bounded by the byte budget, or better a maintained rollup (`children_total/done` [30 §3.5]).
- Pagination is **keyset**: the cursor is the last emitted (sort key, id). Because ids are dense and ordered, a resumed scan starts at the right word of the bitset. This makes budget overflow resumable (§9).
- R3 vs R1/R2 shows why the derived predicates stay maintained (§7): recomputing `ready` on every read costs 100 ms at 1e6. Read from the maintained bitset, it is a popcount (≈ 14 µs, F5).

---

## 7. Standing queries, incremental view maintenance and subscriptions

### 7.1 Landscape (2025–2026)

| System | Model | Memory model | Status |
|---|---|---|---|
| **Differential dataflow** (Rust) | incremental dataflow over timestamped collections; state in *arrangements* (sorted batches + traces) | resident per worker; Materialize's arrangement overhead went from **96 B/update to 0–16 B/update** [D](https://materialize.com/blog/materialize-and-memory/); a user measured ~100 B/record in `group_arranged` and 4× over raw data [C](https://github.com/TimelyDataflow/differential-dataflow/issues/151) | 0.25.1 (Jul 2026) [D] |
| **DBSP / Feldera** (Rust) | Z-sets + four operators; any query → incremental circuit; VLDB 2023 best paper [D](https://docs.feldera.com/vldb23.pdf) | state (traces) per operator; Feldera spills to NVMe [D](https://www.feldera.com/blog/feldera-storage); SQL is compiled by a **Java/Calcite** compiler into **Rust source, then rustc** [D](https://github.com/feldera/feldera/tree/main/sql-to-dbsp-compiler); **ad-hoc queries use DataFusion over a snapshot** [D](https://docs.feldera.com/sql/ad-hoc/) | `dbsp` 0.354 (Sep 2026) [D] |
| **DDlog** (VMware) | Datalog → Rust over DD | same as DD | **archived** [D](https://github.com/vmware-archive/differential-datalog) |
| **Datalog maintenance** | DRed (over-delete / re-derive), Counting, Backward/Forward [D](https://arxiv.org/abs/1711.03987) | counts or re-derivation per fact | theory, mature |
| **Salsa** (rust-analyzer) | memoized queries with dependency tracking; recompute on input change ("red-green") [D](https://github.com/salsa-rs/salsa) | memo table per process | active |

### 7.2 Fit to moirai's process model

moirai has **no daemon**. Most reads are **short-lived CLI or hook processes**, and the MCP server lives for a session only [30 §2.2]. A DD/DBSP circuit keeps its state (arrangements, traces) in the running process. Rebuilding it on every CLI call costs O(data), which defeats the purpose. Keeping it in the MCP server means O(data) private RAM per session and no sharing across the 16 agent processes [I]. Feldera's own architecture points the same way: incremental views are compiled ahead of time and run continuously, and ad-hoc queries are interpreted over a snapshot. moirai's "compiled views" are the handful of derived predicates the engine already maintains in the write path [30 §3.5], persisted in the segments and shared through the page cache.

**Measured [M]:**
- **I1** (~1000 task completions then reverts, each updating `open_blockers` of `blocks` dependents and the `ready` bitset): **13 / 22 / 33 µs** at 1e4 / 1e5 / 1e6, i.e. **~7–17 ns per status change**.
- **I2** (20 standing filter predicates, interpreted, evaluated only against the ~1000 changed ids of a delta): **147 / 180 / 183 µs**, i.e. **~9 ns per (filter, changed id)**, independent of N.

### 7.3 Recommended three-tier design [I]

1. **Tier A: engine-maintained derived state ("compiled views").** Includes `ready`, `is_blocker`, `open_blockers(_exo)`, rollups, `suspect`, `conflicted` [30 §3.5]. Hand-written, updated in the write path (O(degree) per change), persisted, and verified against full recomputation (I9 property tests [30 §3.4]). The query planner rewrites predicates like `ready` or `blocked` to these bitsets and columns. **New derived predicates are added by code, not by users**, so each one gets the same scrutiny as the rest of the engine.
2. **Tier B: filter subscriptions (non-recursive).** A subscription is a stored, bound query plus a change-feed cursor. On the subscriber's next call (hook, `brief`, MCP request), the engine reads the delta since the cursor [30 §6.3] and evaluates the subscription's predicate on the changed ids only, O(|delta|) at ~9 ns each. For joins, it evaluates the delta rule (ΔR ⋈ S ∪ R ⋈ ΔS) via INL from the changed ids. The writer never pays, and idle CPU stays zero (no polling, no push).
3. **Tier C: recursive or complex standing queries.** Record the query's **read footprint** on each evaluation: the ids and edge kinds it touched, as a small id list or bitset within the budget. On the next call, re-run only if the delta intersects the footprint (Salsa-style). If the footprint exceeds its budget, fall back to re-running at most once per call. This avoids a DRed/Counting implementation for a feature whose inputs are small.

Out of scope for v1: arbitrary user-defined incremental views with DD/DBSP semantics. The inputs are tiny (deltas of ≤ ~1k ids per call) and the machinery is large [I].

---

## 8. Planning: rule-based vs cost-based at 1e4–1e6

- Cost-based optimizers mostly fail through **cardinality estimation errors** [D](https://www.vldb.org/pvldb/vol9/p204-leis.pdf). Robust approaches (Robust Predicate Transfer, Yannakakis+) make acyclic plans insensitive to join order [C].
- **moirai has exact statistics for free.** Every indexed predicate is a frozen bitset plus overlay ± lists. |kind ∧ status| is a popcount (12–14 µs at 1e6 [M]), or a per-segment maintained count in the header (O(1)). Degree statistics per edge kind (sum, max, histogram buckets) can be computed at checkpoint time and stored in the segment header. A bounded probe (e.g. "walk the subtree of `#88` up to 10k nodes") *is* the estimate for recursive anchors, and costs ≤ 0.1 ms [M].

**Recommendation [I]:** rule-based rewrites (§2.1) + **greedy anchor-first join ordering driven by exact counts and bounded probes** + semijoin placement when the far side is ≤ 1/64 of the expansion + `Intersect` for cyclic closures. There is no DP enumerator (Grafeo's DPccp [D] targets multi-way joins over large tables) and no histograms beyond degree buckets. Every plan is printable (`--explain`), and `--profile` prints actual rows and steps per operator. Agents learn from EXPLAIN far better than from silent slowness.

Named queries matter more than plan caching. The purpose-built verbs (`ready`, `blocking`, `blockers --transitive`, `tree`, `stale`) become **pre-bound plans with fixed output contracts** [30 §7.1], and a project can store its own named queries as schema data. HelixDB's experience argues for keeping them interpreted, not compiled: the compile-at-deploy query language was deprecated [D].

---

## 9. Query safety: budgets, cancellation, partial results

### 9.1 What others do

| System | Mechanism | Granularity |
|---|---|---|
| SQLite | `sqlite3_progress_handler(db, N, cb)`: callback every ~N VM instructions; non-zero return interrupts; `sqlite3_interrupt()` [D](https://sqlite.org/c3ref/progress_handler.html) | deterministic step budget |
| CozoDB | `:timeout N` (default **300 s**, 0 disables), `::running`, `::kill <id>`, `:limit` early stop [D](https://docs.cozodb.org/en/latest/queries.html) | wall clock |
| Neo4j | `db.memory.transaction.max` terminates the transaction "without affecting the overall health" [D](https://neo4j.com/docs/operations-manual/current/performance/memory-configuration/) | memory, estimated |
| DataFusion | `MemoryPool` (`GreedyMemoryPool`, `FairSpillPool`, `TrackConsumersPool` naming the largest consumers) [D](https://docs.rs/datafusion/latest/datafusion/execution/memory_pool/index.html) | reservations per operator |
| Oxigraph | `CancellationToken(Arc<AtomicBool>)` checked per scanned quad [D](https://github.com/oxigraph/oxigraph/blob/main/lib/spareval/src/eval.rs) | cooperative |
| Kùzu | `VAR_LENGTH_EXTEND_MAX_DEPTH` default 30 [C](https://oneuptime.com/blog/post/2026-08-12-kuzu-variable-length-traversal-explosion/view) | recursion depth |
| IndraDB | `limit` on every pipe step [D](https://docs.rs/indradb-lib/latest/indradb/struct.PipeQuery.html) | per step |
| MCP | `notifications/cancelled {requestId}`; the receiver SHOULD NOT respond [D](https://modelcontextprotocol.io/specification/2025-11-25/basic/utilities/cancellation) | protocol |

### 9.2 Design for moirai [I]

```
struct Budget { steps: u64, bytes: usize, rows_out: u32, visited: u32, depth: u8, deadline: Option<Instant> }
struct Ctx<'v> { view: &'v dyn View, budget: Budget, used: Usage, cancel: &'v AtomicBool, arena: Bump, profile: Option<Profile> }
```

- **Deterministic first.** The primary limit is **steps**: rows touched plus edges walked plus bitset words, counted per batch. Given the same data, the same query stops at the same place. This matters because agents retry, and a flaky timeout sends them in circles. A wall-clock deadline is a secondary safety net, checked per batch via `QueryPerformanceCounter` (~20 ns).
- **Memory by construction.** All query allocations come from a per-query bump arena with a hard cap. Operators that could grow (hash group-by by id, distinct, sort-without-limit, path enumeration, result buffering for `--json`) reserve from the arena and fail with a budget error, never a process OOM. The visited bitset is per process, N/8 bytes, and not charged per query.
- **Defaults** (tunable per call with `--budget`, and per role in the role policy [30 §7.3]):

  | Limit | CLI | MCP | Note |
  |---|---|---|---|
  | steps | 2e7 (~≤ 0.1–0.4 s) | 2e7 | measured throughput 3–20 ns per row/edge (§3, §5) |
  | arena bytes | 4 MiB | 8 MiB | [30 §8.1] private-RSS gates |
  | rows out | 200 (max 1000, cursor beyond) | 100 | token economy [07] |
  | visited (traversals) | 1e5 | 1e5 | whole-graph BFS at 1e6 needs 1e6 |
  | depth | 12 (max 64) | 12 | hierarchy depth limit |
  | deadline | 2 s | 5 s | safety net |

- **Pre-flight refusal.** When the planner's *exact* lower bound on work exceeds the budget (e.g. an unanchored traversal from a 55k-node set, or a sort of 1e5 rows without `limit`), return a diagnostic before executing: `E201 query too broad: ~1.0e6 nodes reachable (budget 1e5); add an anchor, a depth bound, or --budget visited=2e6`.
- **Partial results are explicit.** When a budget trips mid-way, stream what was produced and end with a footer such as `budget: steps 2e7 exhausted after #48211; continue: moirai q --cursor c_9f…`, with exit code 8 ("partial") from the existing table [30 §7.1]. The MCP result carries the same text plus a cursor. This never cuts silently [01 §7 L1].
- **Cancellation.** The CLI installs a Ctrl-C handler that sets the flag. The MCP server maps `notifications/cancelled` to the request's flag. Both are checked per batch.
- **Read-only by default.** The query language is side-effect free; writes stay verbs or an explicit, idempotent `apply` batch (open question Q1). A read query can never take the writer lock.

---

## 10. Versioning interplay (R1–R3)

- **One `View` trait under every scan:** `main` tip, branch tip (pinned checkpoint ⊕ trunk ops ⊕ branch ops [30 §5a.3]), as-of commit (history-backed), or the merge staging view. Operators are view-agnostic, so R1 costs the executor nothing beyond merge-on-read (§3.3) [I].
- **Snapshot isolation for free.** `HEAD` is read once at query start and the plan runs against that `committed_lsn` [30 §4.7]. Readers take no locks. The result's first line states `branch · rev` [30 §7.1].
- **`@commit` / as-of:** derived fields are absent unless `--recompute` [30 I18′]. A predicate on `ready` therefore needs recomputation for as-of views: R3-style, ~0.1 s at 1e6 [M], or restricted to the anchor's closure. The planner must say so in EXPLAIN.
- **`--across R1,R2`:** run the same physical plan per view and merge the streams by id, marking `diverged` rows [30 §3.5]. The cost is the branch first-read cost times k [30 §8.1].
- **`diff A..B` as a query source:** the changed-key set from the per-ref op index is a small id set. Queries over "what changed" (e.g. "tasks whose status changed on lane/x since c4400") anchor on it with INL, the same path as Tier-B subscriptions.

---

## 11. Rust implementations survey: what to borrow, what to avoid

Status is from crates.io and GitHub metadata checked on 2026-09-26 [D] unless marked.

| Project | Status 2025–26 | Language / model | Execution | Borrow | Avoid |
|---|---|---|---|---|---|
| **CozoDB** 0.7.6 | last release Dec 2023; last commit 2024-12-04; "abandoned" per dbdb [C] | CozoScript (Datalog), pest + miette | DNF → strata → magic sets → semi-naive, streaming rows, prefix scans; `:timeout` 300 s default | stratification + magic-set pipeline; `:limit` early-stop rule; `::running/::kill` | dependency on a dormant project; RocksDB/sled/SQLite storage layers |
| **Oxigraph** 0.5.11 / spareval 0.2.7 | active (push 2026-09-24) | SPARQL 1.1/1.2; chumsky + logos parser | closure-compiled pull iterators, hash joins, `CancellationToken` | cancellation token; clean crate split (parser / optimizer / evaluator) | row-at-a-time closures as the main execution path (only 1.1–1.3× over interpreting [M]) |
| **SurrealDB** 3.x | active | SurrealQL; hand-written RD parser since 2.0 | AST → LogicalPlan → ExecutionPlan, streaming, reads first | staged rollout: new executor for reads only, old path kept as fallback ([D](https://github.com/surrealdb/surrealdb/issues/7091)) | its data model and KV layers |
| **GlueSQL** 0.20 | active (push 2026-09-23) | SQL via sqlparser-rs | `StatementPlan` with slots resolved at plan time | plan-time slot resolution | async storage trait overhead |
| **Grafeo** 0.5.42 | active (push 2026-08-10) | 6 languages; hand-written GQL parser | push vectorized, morsel-parallel, factorized, DPccp, Ring WCOJ | factorization; one logical plan behind many surfaces | threads, histograms/DPccp, memory profile (43–136 MB at SF0.1 [C]) |
| **HelixDB** v3 | active; HelixQL deprecated [D] | Rust/TS/Go/Python DSL | runtime orchestrator | lesson: ad-hoc queries beat compile-at-deploy | compiled-query model |
| **GraphLite** 0.0.1 | early (232 stars) | ISO GQL (OpenGQL grammar) | sled; "cost-based optimizer" [C] | shows how big full-GQL scope gets | full GQL as v1 scope |
| **IndraDB** 5.0.0 | Aug 2025; quiet since | builder query enum | materializing pipes | per-step `limit` | — |
| **Kùzu** (C++) | archived 2025-10-10 [D]; LadybugDB fork active [D] | Cypher | vectorized, factorized, ASP-Join, WCOJ, recursive-join frontiers | factorized vectors, sideways semijoin filters, bounded var-length | WALK default for var-length paths; C++ dependency |
| **datafrog** 2.0.1 | crate from 2019, repo maintained (push 2026-08) | none (you write joins by hand) | semi-naive over sorted `Vec` relations, leapjoin | the ~1k-line semi-naive core if user rules arrive | — |
| **ascent** 0.8.1 / **crepe** 0.2.0 | active | compile-time Datalog macros; lattices, BYODS [D](https://kmicinski.com/assets/byods.pdf) | generated Rust | ideas for internal rule code | cannot run runtime queries |
| **egglog** 3.0 | active (PLDI 2025 tutorial) | Datalog + equality saturation | relational e-matching = Generic Join [D](https://arxiv.org/abs/2108.02290) | proof that GJ is simple enough to embed | scope |
| **Nemo** | active (push 2026-09-21) | rule engine, LFTJ over sorted columnar tables [D](https://arxiv.org/abs/2308.15897) | in-memory tries | sorted-column + leapfrog as the recursion core | in-memory whole-DB model |
| **differential-dataflow** 0.25 / **dbsp** 0.354 | active | incremental dataflow / Z-sets | resident arrangements / traces, threads | theory of delta rules (§7) | a long-lived stateful runtime in moirai |
| **DataFusion** | active | SQL/DataFrame, Arrow | vectorized, MemoryPool, spilling | memory-pool design with named consumers for error messages | Arrow/async/thread stack in a 4 MB process |
| **Free Join** (research, Rust) | paper code | — | unified binary/WCOJ over DuckDB plans | intersection-as-a-join-operator idea | as a dependency |

**Conclusion [I]:** nothing is a drop-in. The reusable parts are small: ~1k lines of semi-naive core, a leapfrog intersection, a cancellation token and a memory-pool pattern. They are cheaper to write against moirai's own segment and view types than to adapt. This matches T10 "from scratch" [30 §2.10].

---

## 12. Measurements: method, full results, caveats

**Probe** (not published; Rust 1.98.1, no dependencies, `opt-level=3`, fat LTO, `panic=abort`; binary 323 KB including the parser). Commands: `qlprobe.exe 10000|100000|1000000` and `qlprobe.exe ql`. Raw outputs: `run_*.txt`, `run2_*.txt`, `run3_*.txt` (High priority class), `run4_*.txt` (High), `ql_demo.txt`, consolidated in `ALL_RESULTS.txt`.

**Synthetic graph:**
- **Nodes:** 55 % tasks (35 % open, 5 % in progress, 55 % done, 5 % cancelled), 20 % findings, 10 % notes, 3 % rules, 3 % decisions, 5 % docs, 4 % measurements; priorities P0–P4 at 5/20/50/20/5 %.
- **`parent`:** forest over tasks, depth ≤ 12, with 3 % roots and 10 % of children attached to ≤ 50 campaign hubs.
- **`blocks`:** 0.8 per task from a recent task (a DAG).
- **`about`:** one per finding.
- **`cites`:** 2 per knowledge node, half of them Zipf-skewed to old "hot" nodes.
- **`relates`:** 0.3 per node, local.
- **Totals:** 2.01 distinct edges per node, both directions stored. Graph heap 41.5 B/node (0.40 / 3.96 / 39.6 MB). In moirai the equivalent data is read-only mapped and shared, not private [30 §8.1].

**Timing and memory:** median of 3–300 repetitions (auto-calibrated to ~0.4 s), plus the minimum over all runs. Query heap is the per-query peak from a counting global allocator. Process private bytes and working set come from `K32GetProcessMemoryInfo`. Baseline private bytes of the probe process: 0.62–0.67 MB.

**Consolidated results [M]** (final run's median, with the minimum over all runs in brackets):

| id | query | 1e4 median (min) | 1e5 median (min) | 1e6 median (min) | peak heap 1e4 / 1e5 / 1e6 | result @1e6 |
|---|---|---:|---:|---:|---|---:|
| F1 | filter: columnar tight loop | 12.6 µs (10.7 µs) | 523 µs (471 µs) | 5.5 ms (4.8 ms) | 0 / 0 / 0 | 47857 |
| F2 | filter: Volcano, interpreted expr tree | 186 µs (134 µs) | 1.9 ms (1.6 ms) | 18.2 ms (17.4 ms) | 0 / 0 / 0 | 47857 |
| F3 | filter: Volcano, closure-compiled expr | 152 µs (96.2 µs) | 1.6 ms (1.3 ms) | 15.5 ms (14.8 ms) | 205 B / 205 B / 205 B | 47857 |
| F4 | filter: vectorized 1024-id chunks + selvec | 26.7 µs (22.4 µs) | 274 µs (214 µs) | 3.3 ms (2.3 ms) | 0 / 0 / 0 | 47857 |
| F5 | filter: AND of 3 frozen bitsets + popcount | 0.2 µs (0.1 µs) | 1.4 µs (1.2 µs) | 14.2 µs (12.3 µs) | 0 / 0 / 0 | 47857 |
| T1 | top-20 open tasks by (prio, -updated): heap | 24.6 µs (20.5 µs) | 668 µs (569 µs) | 7.5 ms (6.9 ms) | 205 B / 205 B / 205 B | id |
| T2 | top-20 same: materialize all + sort | 76.2 µs (59.4 µs) | 2.1 ms (1.5 ms) | 28.2 ms (20.1 ms) | 24.0 KiB / 384.0 KiB / 3.0 MiB | id |
| T3 | top-20 same: iterate AND(task,open) bitset + heap | 9.0 µs (7.8 µs) | 84.9 µs (70.0 µs) | 1.6 ms (1.1 ms) | 205 B / 205 B / 205 B | id |
| A1 | count group by (kind,status): array agg | 16.7 µs (11.8 µs) | 135 µs (116 µs) | 1.5 ms (944 µs) | 0 / 0 / 0 | 192695 |
| R1 | ready page (limit 20, maintained counters) | 2.0 µs (1.6 µs) | 1.2 µs (0.9 µs) | 1.1 µs (0.9 µs) | 0 / 0 / 0 | 289 ids scanned |
| R2 | ready count, all (maintained counters) | 40.2 µs (37.0 µs) | 1.2 ms (973 µs) | 14.9 ms (10.7 ms) | 0 / 0 / 0 | 55231 |
| R3 | ready count, recomputed from edges | 215 µs (125 µs) | 5.7 ms (3.7 ms) | 101 ms (94.7 ms) | 0 / 0 / 0 | 55231 |
| B1 | 1000x blockers*: reusable sparse-reset bitset | 106 µs (78.5 µs) | 187 µs (131 µs) | 235 µs (129 µs) | 0 / 0 / 0 | 3724 |
| B2 | 1000x blockers*: fresh dense bitset each | 317 µs (214 µs) | 662 µs (428 µs) | 5.0 ms (3.0 ms) | 1.3 KiB / 12.2 KiB / 122.1 KiB | 3724 |
| B3 | 1000x blockers*: HashSet<u32> each | 558 µs (406 µs) | 690 µs (530 µs) | 2.6 ms (973 µs) | 512 B / 512 B / 512 B | 3724 |
| B4 | 64 sources blockers*: MS-BFS 64 lanes | 352 µs (172 µs) | 3.5 ms (1.3 ms) | 3.0 ms (1.2 ms) | 234.9 KiB / 2.3 MiB / 22.9 MiB | 219 |
| B5 | 64 sources blockers*: 64x sparse-reset | 5.9 µs (4.9 µs) | 4.9 µs (4.3 µs) | 3.9 µs (3.5 µs) | 0 / 0 / 0 | 219 |
| S1 | subtree(hub) all depths (reverse parent) | 5.4 µs (4.8 µs) | 4.9 µs (4.4 µs) | 118 µs (51.0 µs) | 4.0 KiB / 4.0 KiB / 32.0 KiB | 6329 |
| S2 | subtree(hub) depth<=2 | 1.0 µs (0.7 µs) | 1.5 µs (1.3 µs) | 13.2 µs (9.0 µs) | 0 / 0 / 0 | 1917 |
| S3 | 1000x subtree(random task) | 71.2 µs (57.3 µs) | 92.3 µs (72.1 µs) | 112 µs (93.8 µs) | 0 / 0 / 0 | 4141 |
| S4 | 1000x ancestors(random task) walk<=12 | 4.4 µs (3.9 µs) | 9.4 µs (7.2 µs) | 16.0 µs (11.1 µs) | 0 / 0 / 0 | 4417 |
| W1 | undirected reach from #0 (BFS, bitset) | 396 µs (278 µs) | 37.5 ms (20.5 ms) | 486 ms (350 ms) | 65.2 KiB / 524.2 KiB / 4.1 MiB | 999259 |
| P1 | findings∧open -about-> subtree(hub): semijoin bitset | 24.6 µs (21.0 µs) | 424 µs (337 µs) | 12.7 ms (5.7 ms) | 4.0 KiB / 4.0 KiB / 32.0 KiB | 1580 |
| P2 | same: nested loop, walk parents up per finding | 23.5 µs (20.0 µs) | 1.9 ms (700 µs) | 40.7 ms (28.3 ms) | 0 / 0 / 0 | 1580 |
| P3 | same: hash join (HashSet of subtree ids) | 105 µs (85.2 µs) | 825 µs (613 µs) | 15.6 ms (9.8 ms) | 5.0 KiB / 5.0 KiB / 40.0 KiB | 1580 |
| P4 | same: index NL from subtree side (reverse about) | 10.4 µs (9.2 µs) | 10.2 µs (9.3 µs) | 299 µs (225 µs) | 0 / 0 / 0 | 1580 |
| C1 | triangles: sorted-intersection (GJ/LFTJ) | 424 µs (323 µs) | 10.5 ms (6.8 ms) | 165 ms (142 ms) | 0 / 0 / 0 | 976 |
| C2 | triangles: binary hash join (pipelined) | 1.6 ms (1.0 ms) | 59.3 ms (37.6 ms) | 1439 ms (1284 ms) | 288.0 KiB / 2.2 MiB / 36.0 MiB | 976 |
| D1 | semi-naive anc(x,y) full materialization | 2.0 ms (1.5 ms) | 69.8 ms (38.1 ms) | 761 ms (584 ms) | 354.0 KiB / 3.7 MiB / 37.0 MiB | 2409919 |
| I1 | ~1000 completions: incremental ready (+revert) | 13.1 µs (11.1 µs) | 22.0 µs (17.9 µs) | 33.0 µs (26.5 µs) | 0 / 0 / 0 | 243 newly ready |
| I2 | 20 standing filters × ~1000-id delta | 147 µs (133 µs) | 180 µs (149 µs) | 183 µs (155 µs) | 0 / 0 / 0 | 49 matches |
| QL | parse + bind 144-byte query (`qlprobe ql`) | 3.9 µs | — | — | 1.7 KiB transient | — |

Triangle context: 2.7M oriented 2-paths (binary-join intermediate) and 106M unoriented wedges at 1e6; 2.0M oriented edges.

**Caveats:**
1. **Heavy background load.** CPU was at 97–100 % from other agent processes (desktop applications and several `claude` processes). A normal-priority rerun at 1e6 was up to ~2× slower than the High-priority runs. Treat absolute times as pessimistic; ratios between plans were stable across runs.
2. **Data is heap-resident**, not mmap'd. Cold-page effects (~50–100 µs per 4 KiB page after reboot [30 §8.1]) are not included.
3. **Poor locality.** The synthetic graph has worse locality than real moirai data (50 % uniform-random `cites`) and fewer edges (2.0 vs the 3 assumed in [30]). Traversal costs scale roughly with edges touched.
4. **Operators are simplified.** They omit overlay merge-on-read, the tombstone check and field-block decoding. Field-block predicates (tagged varints) will cost more per row than the u8 columns measured, est. 5–10× [I]. Fields that are queried often should be promoted to columns or bitsets.

---

## 13. Estimates: RAM per query and latency for typical moirai queries

These are engine-only numbers: warm cache, on `main`, excluding process spawn and shell (15–190 ms [30 §8.1]). They are derived from §12 [M] with the adjustments noted [I]. "Heap" is query-private working memory. The one-time visited bitset (N/8: 1.2 / 12 / 122 KiB) and the shared mapped pages are excluded.

| Typical query | 1e4 | 1e5 | 1e6 | Heap (1e6) | Basis |
|---|---:|---:|---:|---:|---|
| Parse + bind + plan | ~5–10 µs | same | same (+ ≤ 15 µs per popcount estimate) | < 4 KiB | QL [M] + [I] |
| `#40` / ids with fields | 1–5 µs | 1–5 µs | 1–5 µs | < 4 KiB | [30 §8.1] |
| Indexed filter, first page of 20 | < 5 µs | < 5 µs | < 10 µs | < 8 KiB | F5 + early stop [I] |
| Indexed filter, exact count | 0.2 µs | 1.4 µs | 14 µs | 0 | F5 [M] |
| Indexed filter + `order by … limit 20` | ~10 µs | ~85 µs | ~1–2 ms | < 1 KiB | T3 [M] |
| Non-indexed column predicate (u8/u32 column), full scan | ~27 µs | ~0.3 ms | ~3 ms | 0 | F4 [M] |
| Field-block (varint) predicate, full scan | ~0.15–0.3 ms | ~1.5–3 ms | ~15–30 ms | < 16 KiB | F4 × 5–10 [I] |
| Text contains (tier-1 FTS) | 0.2–1 ms | 2–10 ms | 20–80 ms | < 64 KiB | [30 §8.1] |
| `ready` page / `ready` count | ~1–2 µs / ≤ 1 µs | same / ~1.4 µs | same / ~14 µs | 0 | R1, F5 [M] |
| Transitive blockers of `#N` (closure ≈ 4, max ≈ 50) | 0.1–0.3 µs (≤ 5 µs worst) | same | same | 0 | B1 [M] |
| Subtree of a campaign (≈ 6k nodes at 1e6) | ~5 µs | ~5 µs | ~0.05–0.15 ms | 32 KiB | S1 [M] |
| Anchored 2–3 hop pattern with filters (good plan) | ~10 µs | ~10 µs | ~0.2–0.3 ms | < 64 KiB | P4 [M] |
| Same pattern, badly planned (what the planner must avoid) | ~25–100 µs | ~0.4–2 ms | ~6–40 ms | ≤ 40 KiB | P1–P3 [M] |
| Group-by (kind, status) | ~17 µs | ~0.14 ms | ~1–1.5 ms (or µs via bitset counts) | 0 | A1 [M] |
| Cyclic pattern (triangle-like) over all edges | ~0.4 ms | ~7–10 ms | ~0.15 s | 0 | C1 [M] |
| Unanchored whole-graph traversal (**budget trips at 1e6**) | ~0.3–0.4 ms | ~20–40 ms | 0.35–0.5 s | 4.1 MiB | W1 [M] |
| Bottom-up recursive rule, full materialization (**forbidden by default**) | ~2 ms | ~40–70 ms | ~0.6–0.8 s | 37 MiB | D1 [M] |
| Subscription check per call (20 filters × 1k-id delta) | ~0.15 ms | ~0.18 ms | ~0.18 ms | 0 | I2 [M] |

**Private RSS of a query process** = baseline (~0.6–0.7 MB for a no-dependency Rust binary [M]; the full CLI est. 1.5–4 MB [30 §8.1]) + the overlay + the visited bitset (≤ 122 KiB at 1e6) + query heap (typically < 64 KiB; capped at 4 MiB CLI / 8 MiB MCP). This fits the ≤ 4 MB CLI gate at 1e5 and the 3–6 MB estimate at 1e6 in [30 §8.1], **provided** the budget blocks the four RAM-heavy shapes the probe found: O(E) hash builds (36 MiB), MS-BFS (23 MiB), bottom-up closure materialization (37 MiB) and full-graph BFS (4 MiB).

---

## 14. Recommended execution architecture for a hand-written moirai engine

### 14.1 Shape

```
query text / verb / MCP `find`
  │  lex (hand-written, byte spans) ─ parse (RD + Pratt, recovery at | , ) ) ─ AST{spans}
  ▼
bind + type-check against schema-as-data (kinds, fields, enums, edge kinds; did-you-mean; E1xx codes)
  ▼
logical algebra: Scan · Filter · Expand(edge-mask, dir, lo..hi) · Pattern · Intersect · Aggregate · TopK · Sort · Limit · Union/Except · AsOf · Across · Diff
  ▼
rewrites (rule-based): NNF/DNF · pushdown · ORDER+LIMIT→TopK · COUNT→popcount · derived predicates→maintained bitsets · verbs→named plans
  ▼
physical planning (cost-lite): exact counts from bitsets + bounded probes → anchor, INL order, semijoin placement, Intersect for cycles,
                               budget pre-flight (E2xx "too broad"), EXPLAIN
  ▼
executor (single thread, pull, batch = 1024 ids + selvec):
  sources:  IdList · BitmapScan(AND/OR/ANDNOT over frozen bitsets ⊕ overlay ±) · ColumnScan(vectorized, merge-on-read) · DeltaScan(change feed)
  graph:    Expand (CSR slice, factorized) · SemiJoin(bitset probe) · Intersect(leapfrog k sorted slices) · Closure/Subtree/Ancestors/ShortestPath
  relational: Filter(per-batch interpreted expr) · Project(late materialization) · TopK(heap) · Sort(capped) · Aggregate(array | bounded hash) · Distinct(bitset)
  control:  Limit/Cursor(keyset) · Budget+Cancel check per batch · Profile counters
  ▼
output writer (streaming; CLI line contract / JSON v1 envelope; drop + budget footers; cursor)
```

### 14.2 Invariants of the engine [I]

1. **No per-row heap allocation.** All working memory comes from the per-query arena (capped). Strings are borrowed from mapped segments. Bodies decompress into the arena only in the final projection.
2. **No O(N) or O(E) pipeline breaker without a budget reservation.** Id sets are bitsets. The CSR is the join index, so there is no hash-join build side over edges.
3. **Every operator is view-agnostic** (`&dyn View`: main, branch, as-of, merge-staging). Merge-on-read happens inside the scans.
4. **Deterministic results and deterministic budget stops.** Ordering is by (sort key, id). The steps budget is primary.
5. **Every query can be explained** (`--explain`), profiled (`--profile`) and cancelled (flag checked per batch).
6. **Derived predicates are never recomputed by queries on the live view.** They are read from Tier-A state. As-of views recompute on request, within budget.

### 14.3 Build order and size (est.) [I]

| Step | Content | Est. size | Gate |
|---|---|---|---|
| E1 | lexer, parser, AST, diagnostics renderer, error codes, EBNF + golden error tests | 2–3k lines | fuzzing (cargo-fuzz) finds no panics; golden diagnostics |
| E2 | binder over schema-as-data; logical IR; verbs re-expressed as named plans | 1.5–2k | the existing verb outputs are byte-identical through the new path |
| E3 | executor core: BitmapScan, ColumnScan, Filter, Project, TopK, Limit/Cursor, Aggregate, Budget/Cancel, output writer | 2.5–3.5k | [30 §8.1] CI gates; budget-stop determinism tests |
| E4 | graph operators: Expand (factorized), SemiJoin, Closure/Subtree/Ancestors/ShortestPath, Intersect | 1.5–2.5k | property tests against a brute-force evaluator |
| E5 | planner: exact-count anchor choice, semijoin placement, pre-flight refusal, EXPLAIN/PROFILE | 1–1.5k | plan-quality benchmark (P1–P4-style pairs must pick the fast plan) |
| E6 | views: branch, as-of, across, diff sources | 0.5–1k (the views exist already) | same results as replay-from-genesis |
| E7 | subscriptions: Tier B (delta filters), Tier C (footprint re-run) | 0.8–1.2k | no idle CPU; delta result = full re-evaluation result |
| (later) | user rules: semi-naive + magic sets + stratification | 2–3k | only if owner asks (Q2) |

Total for v1 (E1–E7) ≈ **10–15k lines** of Rust, with no new dependencies [I].

**Testing.**
- **Differential testing:** every query runs against a naive reference evaluator (nested loops over the same view) and, for the relational subset, against the SQLite oracle backend already planned [30 §8.2].
- **Metamorphic logic-bug testing** in the style of SQLancer's TLP [D](https://github.com/sqlancer/sqlancer): a query's result equals the union of its partitions under `p`, `¬p` and `p IS NULL`.
- **Grammar-based query fuzzing**, and **budget-stop replay tests** (same query and data → same stop point and cursor).

---

## 15. Risks and what would change this recommendation

| Risk / assumption | Evidence | What would change the call |
|---|---|---|
| Agents write mostly anchored, small queries | [07], verbs in [30 §7.1]; LLM query accuracy unknown for a new language | If profiling shows many unanchored analytics queries, add MS-BFS and more vectorized scans; still no threads |
| Field-block predicates are slow (est. 5–10× F4) | [I] | If common, promote hot fields (e.g. `assignee`, `work_kind`, `severity`) to columns or bitsets at checkpoint time |
| Locality of real data is better than the probe's | ids allocated in creation order [30 §3.1] | If real traversals are slower, reorder CSR within segments at rollup |
| Recursive user rules will be needed | not stated in R5 | Adds a semi-naive + magic-set module (~2–3k lines); budgets make it safe |
| Standing queries need push delivery | zero-idle-CPU rule [30 §6.6] | Requires the optional leader (M6) to evaluate subscriptions on commit and signal waiters |
| Load-affected measurements | 97–100 % CPU during the probe | Re-run S0-style on a quiet machine; the plan ratios, not the absolute numbers, drive the design |

---

## 16. Sources

**moirai documents:** `design/30-synthesis.md` (§2.2, §3.1–3.5, §4.4–4.8, §6.3, §7.1–7.3, §8.1–8.2); `06-graph-data-model-integrity.md` §12; `07-agent-integration-cli-mcp-skills.md`; `00-phase1-digest.md`.

**Execution models:**
- Graefe, Volcano (TKDE 1994) https://doi.org/10.1109/69.273032
- Boncz et al., MonetDB/X100 (CIDR 2005) https://www.cidrdb.org/cidr2005/papers/P19.pdf
- Neumann, compiled query plans (VLDB 2011) https://www.vldb.org/pvldb/vol4/p539-neumann.pdf
- Kersten et al. (VLDB 2018) https://www.vldb.org/pvldb/vol11/p2209-kersten.pdf
- DuckDB push-based switch https://github.com/duckdb/duckdb/pull/2393

**Joins:**
- LFTJ https://arxiv.org/abs/1210.0481
- Generic Join ("Skew strikes back") https://arxiv.org/abs/1310.3314
- Free Join https://arxiv.org/abs/2301.10841
- Unified binary/WCOJ (2025) https://arxiv.org/abs/2505.19918
- Umbra WCOJ https://www.vldb.org/pvldb/vol13/p1891-freitag.pdf
- GraphflowDB https://arxiv.org/abs/1903.02076
- Kùzu CIDR 2023 https://www.cidrdb.org/cidr2023/papers/p48-jin.pdf
- Kùzu blogs https://blog.kuzudb.com/post/factorization/ , https://blog.kuzudb.com/post/wcoj/
- Yannakakis+ https://arxiv.org/abs/2504.03279
- Robust Predicate Transfer https://arxiv.org/abs/2502.15181
- Ring WCOJ with filters (2026) https://arxiv.org/abs/2608.03840
- Leis et al., optimizers (VLDB 2015) https://www.vldb.org/pvldb/vol9/p204-leis.pdf

**Graph and recursion:**
- DuckPGQ (VLDB 2023 demo) https://www.vldb.org/pvldb/vol16/p4034-wolde.pdf ; https://duckpgq.org/ ; https://duckdb.org/community_extensions/extensions/duckpgq
- MS-BFS https://www.vldb.org/pvldb/vol8/p449-then.pdf
- Direction-optimizing BFS https://doi.org/10.1109/SC.2012.50
- GQL/SQL-PGQ pattern matching https://arxiv.org/abs/2112.06217
- DuckDB `USING KEY` https://duckdb.org/2025/05/23/using-key and v2.0 recursion https://duckdb.org/2026/08/25/how-duckdb-runs-recursive-ctes-faster
- Kùzu var-length explosion [C] https://oneuptime.com/blog/post/2026-08-12-kuzu-variable-length-traversal-explosion/view
- Kùzu archive https://github.com/kuzudb/kuzu ; LadybugDB https://github.com/LadybugDB/ladybug ; https://thedataquarry.com/blog/from-kuzu-to-ladybug/

**IVM:**
- DBSP https://docs.feldera.com/vldb23.pdf
- Feldera ad-hoc queries https://docs.feldera.com/sql/ad-hoc/ ; SQL compiler https://github.com/feldera/feldera/tree/main/sql-to-dbsp-compiler ; storage https://www.feldera.com/blog/feldera-storage
- Differential dataflow https://github.com/TimelyDataflow/differential-dataflow and issue 151 https://github.com/TimelyDataflow/differential-dataflow/issues/151
- Materialize memory https://materialize.com/blog/materialize-and-memory/
- DDlog (archived) https://github.com/vmware-archive/differential-datalog
- Datalog maintenance https://arxiv.org/abs/1711.03987
- Salsa https://github.com/salsa-rs/salsa

**Rust systems:**
- CozoDB https://github.com/cozodb/cozo , https://docs.cozodb.org/en/latest/execution.html , https://docs.cozodb.org/en/latest/queries.html , https://dbdb.io/db/cozodb
- Oxigraph https://github.com/oxigraph/oxigraph (spareval `eval.rs`, spargebra `Cargo.toml`)
- SurrealDB 3.0 https://surrealdb.com/blog/surrealdb-3-0-benchmarks-a-new-foundation-for-performance and 2.0 parser https://surrealdb.com/blog/challenge-accepted-announcing-surrealdb-2-0
- GlueSQL https://github.com/gluesql/gluesql
- Grafeo https://github.com/GrafeoDB/grafeo
- HelixDB https://github.com/HelixDB/helix-db , https://github.com/HelixDB/hql-v1-docs
- GraphLite https://github.com/GraphLite-AI/GraphLite
- IndraDB https://github.com/indradb/indradb , https://docs.rs/indradb-lib/latest/indradb/struct.PipeQuery.html
- datafrog https://github.com/rust-lang/datafrog
- ascent https://github.com/s-arash/ascent , BYODS https://kmicinski.com/assets/byods.pdf
- crepe https://github.com/ekzhang/crepe
- egglog https://github.com/egraphs-good/egglog , https://arxiv.org/abs/2304.04332 , relational e-matching https://arxiv.org/abs/2108.02290
- Nemo https://arxiv.org/abs/2308.15897 , https://github.com/knowsys/nemo
- DataFusion memory pool https://docs.rs/datafusion/latest/datafusion/execution/memory_pool/index.html

**Parsers and diagnostics:**
- Ruff v0.4.0 https://astral.sh/blog/ruff-v0.4.0
- chumsky https://lib.rs/crates/chumsky , https://codeberg.org/zesterer/chumsky
- winnow https://crates.io/crates/winnow ; pest https://pest.rs ; LALRPOP https://github.com/lalrpop/lalrpop ; logos https://github.com/maciejhirsz/logos
- annotate-snippets as rustc default (nightly) https://github.com/rust-lang/rust/pull/148188
- ariadne https://github.com/zesterer/ariadne ; miette https://github.com/zkat/miette

**Safety:**
- SQLite https://sqlite.org/c3ref/progress_handler.html , https://sqlite.org/c3ref/interrupt.html
- Neo4j memory https://neo4j.com/docs/operations-manual/current/performance/memory-configuration/
- MCP cancellation https://modelcontextprotocol.io/specification/2025-11-25/basic/utilities/cancellation
- SQLancer https://github.com/sqlancer/sqlancer

**Text-to-query accuracy:** Text2Cypher https://arxiv.org/html/2412.10064v1 (via [06 §12.2]).
