---
title: Lachesis, the query language
nav: Lachesis
description: "Moirai's own query language: Cypher-shaped patterns over the graph, the engine's state as built-ins, versions inside queries, guarded writes, and a benchmark that decides when the language is frozen."
---

# Lachesis, the query language

Lachesis (LQ, files `*.lq`) is how agents and people ask Moirai anything its commands do not cover. It looks like Cypher, accepts common GQL spellings, knows Moirai's own notions of readiness, versions and file links, and is designed and measured so that language models write it correctly. It is named after the Moira who measures the thread, which is what a query does.

## Why a language of its own {#why-a-language}

Moirai has many commands: `ready`, `blockers`, `show`, `log`, `links check` and more. Each one is a query written in LQ. `moirai ready --scope 200` is the same thing as `moirai q ready scope=200`, which runs the standard [named query](glossary.html#term-named-query) `std.ready`. Every write command is a named mutation that expands to an LQ write block. One parser, binder, planner and executor serve every read, and one statement compiler serves every write. A query and a command can therefore never disagree about what "ready" or "blocked" means.

<figure class="diagram">
<svg viewBox="0 0 720 320" role="img" aria-labelledby="lachesis-d1-t">
  <title id="lachesis-d1-t">Commands and free-form queries share one read path and one write path</title>
  <defs><marker id="lachesis-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path class="d-arrow" d="M0,0 L10,5 L0,10 z"/></marker></defs>
  <path class="d-box" d="M32,20 h184 a8,8 0 0 1 8,8 v20 a8,8 0 0 1 -8,8 h-184 a8,8 0 0 1 -8,-8 v-20 a8,8 0 0 1 8,-8 z"/>
  <text class="d-text-small" x="124" y="38" text-anchor="middle" dominant-baseline="middle">moirai ready --scope 200</text>
  <path class="d-box" d="M32,80 h184 a8,8 0 0 1 8,8 v20 a8,8 0 0 1 -8,8 h-184 a8,8 0 0 1 -8,-8 v-20 a8,8 0 0 1 8,-8 z"/>
  <text class="d-text-small" x="124" y="98" text-anchor="middle" dominant-baseline="middle">moirai q ready scope=200</text>
  <path class="d-box" d="M32,140 h184 a8,8 0 0 1 8,8 v20 a8,8 0 0 1 -8,8 h-184 a8,8 0 0 1 -8,-8 v-20 a8,8 0 0 1 8,-8 z"/>
  <text class="d-text-small" x="124" y="158" text-anchor="middle" dominant-baseline="middle">moirai q - / MCP query tool</text>
  <path class="d-box" d="M272,50 h152 a8,8 0 0 1 8,8 v20 a8,8 0 0 1 -8,8 h-152 a8,8 0 0 1 -8,-8 v-20 a8,8 0 0 1 8,-8 z"/>
  <text class="d-text-small" x="348" y="68" text-anchor="middle" dominant-baseline="middle">named query std.ready</text>
  <path class="d-box-accent" d="M480,70 h208 a8,8 0 0 1 8,8 v60 a8,8 0 0 1 -8,8 h-208 a8,8 0 0 1 -8,-8 v-60 a8,8 0 0 1 8,-8 z"/>
  <text class="d-text-strong" x="584" y="90" text-anchor="middle" dominant-baseline="middle">Read path</text>
  <text class="d-text-small" x="584" y="112" text-anchor="middle" dominant-baseline="middle">one parser, binder, planner</text>
  <text class="d-text-small" x="584" y="132" text-anchor="middle" dominant-baseline="middle">executor on a read-only view</text>
  <line class="d-edge" x1="224" y1="38" x2="264" y2="62" marker-end="url(#lachesis-arrow)"/>
  <line class="d-edge" x1="224" y1="98" x2="264" y2="76" marker-end="url(#lachesis-arrow)"/>
  <line class="d-edge" x1="432" y1="68" x2="472" y2="92" marker-end="url(#lachesis-arrow)"/>
  <line class="d-edge" x1="224" y1="158" x2="472" y2="128" marker-end="url(#lachesis-arrow)"/>
  <path class="d-box" d="M32,200 h184 a8,8 0 0 1 8,8 v20 a8,8 0 0 1 -8,8 h-184 a8,8 0 0 1 -8,-8 v-20 a8,8 0 0 1 8,-8 z"/>
  <text class="d-text-small" x="124" y="218" text-anchor="middle" dominant-baseline="middle">moirai set / moirai tx</text>
  <path class="d-box" d="M32,260 h184 a8,8 0 0 1 8,8 v20 a8,8 0 0 1 -8,8 h-184 a8,8 0 0 1 -8,-8 v-20 a8,8 0 0 1 8,-8 z"/>
  <text class="d-text-small" x="124" y="278" text-anchor="middle" dominant-baseline="middle">MCP write tool</text>
  <path class="d-box" d="M272,230 h152 a8,8 0 0 1 8,8 v20 a8,8 0 0 1 -8,8 h-152 a8,8 0 0 1 -8,-8 v-20 a8,8 0 0 1 8,-8 z"/>
  <text class="d-text-small" x="348" y="248" text-anchor="middle" dominant-baseline="middle">one TX block</text>
  <path class="d-box-accent" d="M480,222 h208 a8,8 0 0 1 8,8 v36 a8,8 0 0 1 -8,8 h-208 a8,8 0 0 1 -8,-8 v-36 a8,8 0 0 1 8,-8 z"/>
  <text class="d-text-strong" x="584" y="240" text-anchor="middle" dominant-baseline="middle">Write path</text>
  <text class="d-text-small" x="584" y="260" text-anchor="middle" dominant-baseline="middle">one commit, all or nothing</text>
  <line class="d-edge" x1="224" y1="218" x2="264" y2="242" marker-end="url(#lachesis-arrow)"/>
  <line class="d-edge" x1="224" y1="278" x2="264" y2="256" marker-end="url(#lachesis-arrow)"/>
  <line class="d-edge" x1="432" y1="248" x2="472" y2="248" marker-end="url(#lachesis-arrow)"/>
</svg>
<figcaption>A read command is a named query; a write command expands to a <code>TX</code> block. Free-form LQ enters the same two paths.</figcaption>
</figure>

The shape is Cypher's because language models have seen far more Cypher than GQL. LQ keeps Cypher's meaning wherever it accepts Cypher's spelling, accepts GQL spellings where they mean the same thing, and turns every form it does not support into an error that names the LQ way to write it. The grammar contains only forms that a stated requirement, a standard named query or benchmark evidence needs, because every production of grammar version 1 is permanent.

The language is also measured. Before it is frozen, a benchmark called [LQ-Bench](#lq-bench) checks that a model, given only a short skill card, writes correct queries, and that its mistakes are loud rather than silent.

## A first query {#first-query}

The examples on this page use a small, fictional HTTP server, `tinyhttp`. This chapter uses its own snapshot of that project's store, so its ids and revisions differ from those in other chapters. Its campaign task `#200`, "HTTP/2 support", has subtasks, and the work happens on the branch `lane/http2`. Which subtasks can an agent start now?

```console
$ moirai q - <<'EOF'
MATCH (t:task) WHERE t.ready AND t IN subtree(#200) RETURN t ORDER BY t.priority LIMIT 5
EOF
branch: lane/http2 | rev 4520 | live | 2 rows
#204 task open P1 "Decode HTTP/2 frames" parent:#200
#216 task open P3 "Request logging" parent:#200

```

*Designed output.*

The first line of every result is a header: the branch and revision the rows were read from, `live` because readiness depends on current leases, and the row count. Each row is a node line: id, kind, status, priority, title and key fields. `P0` is the most important priority, so `ORDER BY t.priority` lists the most important first. Node lines print a priority as `P0` to `P4`, but in LQ a priority is its number: `P1` is 1, so `t.priority <= 1` selects `P0` and `P1`.

The same question has a named query, which is shorter and needs no quoting in any shell. `--show-query` prints the LQ behind it, which is how agents learn the language from the commands they already use:

```console
$ moirai q ready scope=200 --show-query
std.ready($scope: node? = NULL, $role: text? = NULL, $limit: int = 20) SHAPE node   (built-in, LQ 1)
MATCH (t:task)
WHERE t.ready
  AND ($scope IS NULL OR t IN subtree($scope))
  AND ($role IS NULL OR fits_role(t, $role))
RETURN t ORDER BY t.priority, t.id LIMIT $limit
// bound: $scope = #200, $role = NULL, $limit = 20

```

*Designed output.*

> [!TIP]
> **Ids on the command line**
>
> Inside a query, `#200` is a node. On a shell command line an unquoted `#` starts a comment, so arguments carry bare numbers: `scope=200`. Free-form text travels through a quoted heredoc in Bash, a piped here-string in PowerShell, or the MCP `query` tool.

## Patterns: kinds, edges and directions {#patterns}

A pattern describes a piece of the graph. Node kinds are the labels: `task`, `doc`, `note`, `rule`, `decision`, `question`, `finding`, `verdict`, `measurement`, `artifact`, `run`, `lane` and `area`. A node has exactly one kind, so `(x:note|rule)` means "a note or a rule". `#N` is a node literal; `#u:` followed by 32 hex digits is the portable form of the same id. `CALL schema(kind: 'task')` lists a kind's fields.

In LQ an edge type is the schema's [edge kind](concepts.html#edges) in capitals: `blocks` is `BLOCKS`, `about` is `ABOUT`. The one exception is `parent`, which LQ calls `CHILD_OF` so that `(a)-[:CHILD_OF]->(b)` reads correctly. Edge types are named so that they read correctly left to right in the direction they are stored. Most have reverse aliases for asking the question the other way round:

| Edge | Reverse alias | Reading of `(a)-[:T]->(b)` |
|---|---|---|
| `CHILD_OF` | `PARENT_OF` | a is a child of b |
| `BLOCKS` | `BLOCKED_BY` | a must finish before b starts |
| `GATES` | `GATED_BY` | verdict a gates the completion of b |
| `SUPERSEDES` | `SUPERSEDED_BY` | a supersedes b |
| `CITES` | `CITED_BY` | a cites b |
| `ABOUT` | — | a is about b |
| `AT` | — | a is anchored in file b (a [file link](file-links.html)) |

Direction mistakes are the classic silent error in graph queries: "what does #211 wait for?" written backwards returns what #211 blocks. LQ makes the direction visible. When a pattern starts from a fixed node on an edge whose two ends can be the same kind, such as `BLOCKS` between tasks, or when it uses a reverse alias, the result carries, right under its header, a [reading echo](glossary.html#term-reading-echo) that says in words what the pattern means:

```console
$ moirai q - <<'EOF'
MATCH (#211)-[:BLOCKED_BY]->(b) RETURN b
EOF
branch: lane/http2 | rev 4520 | 2 rows
reads: b BLOCKS #211 (written #211 BLOCKED_BY b) | b must finish before #211 starts
#207 task in_progress P1 "Connection pool" parent:#200 lease:dev#2(L-18)
#209 task open P2 "Limit header size" parent:#200 blockers:#204 BLOCKED

```

*Designed output.*

Two more checks back this up. The binder checks the kinds at both ends of each edge as types, so `DEPENDS_ON` between two tasks is an error that names `BLOCKS` and `BLOCKED_BY`. And when an anchored pattern matches nothing while edges of that kind point the other way, notice `N07` says so and shows the reversed pattern. Models that have not passed the benchmark's gates see the reading echo on every edge pattern.

## WHERE, RETURN, ORDER BY and LIMIT {#clauses}

The shape of a query is `[USE rev] MATCH pattern [WHERE ...] RETURN ... [ORDER BY ...] [LIMIT n]`. `OPTIONAL MATCH`, `WITH`, `UNWIND`, aggregates, `EXISTS { }`, `COUNT { }`, `CASE`, parameters (`$name`, never spliced into the text) and `UNION`, `EXCEPT` and `INTERSECT` work as in Cypher.

```lq
MATCH (t:task)
WHERE t.unfinished AND t.priority <= 1 AND 'h2' IN t.labels
RETURN t.id, t.status, t.priority
ORDER BY t.priority, t.id

```

```text
branch: lane/http2 | rev 4520 | 2 rows
t.id  t.status     t.priority
#204  open         1
#207  in_progress  1

```

*Designed output.*

Every result has a total, deterministic order: explicit keys first, then the ids of the bound elements. A page holds 50 rows by default. A longer result ends with a cursor that fetches the next page, and a query that reads no live state returns the same bytes every time on the same version. `SKIP` and `OFFSET` are not in the language, because offsets over a changing graph skip or repeat rows.

## Counting {#counting}

Counting follows Cypher and GQL. A pattern binds once per match of all its elements, anonymous ones included, so this counts the blocking edges of each task:

```lq
MATCH (t:task)<-[:BLOCKS]-()
WHERE t IN subtree(#200)
RETURN t.id, count(*) AS blockers
ORDER BY blockers DESC, t.id

```

```text
branch: lane/http2 | rev 4520 | 2 rows
t.id  blockers
#211  2
#209  1

```

*Designed output.*

`RETURN` keeps duplicate rows; `RETURN DISTINCT` removes them, and notice `N09` points out duplicates on a page. A variable-length part is different: it binds once per pair of start and end nodes, not once per path, because path counts explode on graphs with shortcuts. A count over such a part therefore counts endpoint pairs, and a notice says so:

```console
$ moirai q - <<'EOF'
MATCH (x:task)-[:BLOCKS*1..]->(#211) RETURN count(*) AS upstream
EOF
branch: lane/http2 | rev 4520 | 1 row
reads: x BLOCKS*1.. #211 | x must finish before #211 starts (through 1 or more steps)
upstream
3
N08: count(*) over a quantified pattern counts (x, #211) endpoint pairs, not paths

```

*Designed output.*

## The absent value and two-valued logic {#absent-values}

Header fields such as status and priority always have a value. Optional fields, such as a task's `assignee`, can be absent. LQ uses two-valued logic with an explicit absent value, where SQL and GQL use three-valued logic:

| Expression, with `x` absent | Result |
|---|---|
| `x = v`, `x IN list`, `x CONTAINS s` | false |
| `x <> v`, `x NOT IN list` | true |
| `x < v` and the other ordered comparisons | false |
| `x IS NULL` | true |
| grouping, `DISTINCT` | all absent values form one group |
| `ORDER BY` | absent values sort last, in both directions |

So `t.assignee <> 'dev#1'` includes unassigned tasks, which is what the writer almost always means; under three-valued logic those rows would silently vanish. Warning `W01` counts the rows that an absent value excluded from an ordered comparison. Comparing with `NULL` is an error with the fix:

```text
error[E118 null_comparison]: a comparison with NULL is never true
 --> <stdin>:1:33
  |
1 | MATCH (t:task) WHERE t.assignee = NULL RETURN t
  |                                 ^ write t.assignee IS NULL

```

*Designed output.*

Division is another deliberate departure from Cypher: `/` always returns a number with a fraction, so `refuted / raised` is a ratio, not 0. `toInteger(a / b)` gives the other meaning.

## The engine's state as built-ins {#built-ins}

An agent could try to work out "ready" by hand: an open task with no unfinished blocker. That misses blockers inherited from a parent, blockers left behind by a deletion, deferred tasks, containers, leases and work finished on other branches. LQ exposes the engine's own definitions instead. They share code with the write path, and a hand-written readiness test draws warning `W07`:

```text
W07: hand-derived readiness misses inherited and flagged blockers, markers, leases, defer_until and containers; use t.unblocked (structural) or t.ready (dispatchable now)

```

*Designed output.*

| Built-in | Meaning | Valid at |
|---|---|---|
| `t.ready` | dispatchable now, exactly what `moirai ready` lists | a branch tip |
| `t.unblocked`, `t.blocked` | the structural part: no open blockers, or some | any version |
| `t.unfinished`, `t.done` | not done; done or cancelled (warning `W03` when cancelled rows appear) | any version |
| `t.open_blockers`, `t.is_blocker`, `t.suspect`, `t.conflicted` | derived counters and flags | any version |
| `t.claimed`, `t.lease` | who holds the task now | a branch tip |
| `subtree(n)`, `ancestors(n)`, `children(n)` | sets of nodes, used with `IN` | any version |
| `CALL blockers(n, transitive: true)` | the engine's full list of blockers, including inherited ones, ones left by a deletion and work settled on other branches, with depth and the node they come through | any version; its `elsewhere` column only at a branch tip |
| `applies(r, 'src/http/parse.rs')` | whether a rule or note applies to a path | any version |
| `file('src/http/parse.rs')` | the file node at a path, or at an old path | any version |
| `link_state(x)`, `CALL links(scope: n)` | the [link state](file-links.html#link-states) computed in your working tree | a branch tip with a resolved tree |

Other built-ins include `search()` for ranked text search over titles and abstracts, `staleness()`, `relevant_to()`, `fits_role()`, `neighbors()`, `history()`, `changes()` and others. Asking why a task is blocked uses the relation, which returns the chain without path variables:

```console
$ moirai q - <<'EOF'
CALL blockers(#211, transitive: true) YIELD blocker, depth, via, reason
RETURN blocker, depth, via, reason
EOF
branch: lane/http2 | rev 4520 | 3 rows
#204 task open P1 "Decode HTTP/2 frames" parent:#200 depth=2 via=#209 reason=direct
#207 task in_progress P1 "Connection pool" parent:#200 lease:dev#2(L-18) depth=1 reason=direct
#209 task open P2 "Limit header size" parent:#200 blockers:#204 depth=1 reason=direct BLOCKED

```

*Designed output.*

Link states work the same way. This lists every link under the campaign that needs attention, and it reads the files in your working tree without writing anything:

```lq
MATCH (n)-[a:AT]->(f:artifact)
WHERE n IN subtree(#200) AND link_state(a) <> 'ok'
RETURN f.path AS path, a.kind AS anchor, link_state(a) AS state
ORDER BY path

```

State that exists only now, such as readiness, leases and link states, exists only at a branch tip. At a past version it is error `E302`, which suggests `unblocked` in place of `ready`. It is an error rather than an "unknown" value because, under two-valued logic, "unknown" would satisfy `<> 'ok'` and turn every link of an old version into a broken one.

## Quantified paths {#quantified-paths}

A quantifier repeats an edge. Both spellings work and mean the same thing: Cypher's `-[:CHILD_OF*1..2]->` and GQL's `-[:CHILD_OF]->{1,2}`; `+` means one or more steps. A parenthesised group with its own `WHERE` filters every step. This finds the tasks upstream of `#211` through unfinished tasks only, at most five steps away:

```lq
MATCH (x:task)((a:task)-[:BLOCKS]->(b) WHERE a.unfinished){1,5}(#211)
RETURN x

```

A bound `{m,n}` admits an end node reachable by a walk of `m` to `n` steps. On Moirai's acyclic edge kinds, such as `CHILD_OF` and `BLOCKS`, that equals Cypher's answer. Evaluation goes level by level with a visited set, so every quantified pattern terminates on any graph, and no restricting keyword is ever needed. Path variables and shortest-path searches are not in version 1; `blockers()` already returns the chain.

## Versions inside a query {#versions}

Moirai keeps history like git (see [how versions work](versions.html)), and a query can read any version of it. `USE` chooses the version for a query part. Revisions are recognised only in revision positions (after `USE`, after `ON` and `IF TIP` in a write block, and in the version arguments of the history relations), so a variable named `s1` is never mistaken for one. The forms are git's: a branch (`lane/http2`), an ancestor (`main~5`, `main^2`), a commit prefix (`c4f1a9e20`), a store sequence number (`s4400`), a reflog position or time (`main@3`, `main@2026-11-01T10:00Z`), and ranges (`main..lane/http2`, `main...lane/http2`).

| Query | Question it answers |
|---|---|
| `USE main~5 MATCH (t {id: #211}) RETURN t.status, t.unblocked` | What was this task's state five commits ago on `main`? |
| `CALL log(main..lane/http2)` | Which commits on the lane are not on `main` yet? |
| `CALL diff(main...lane/http2) YIELD change, node, aspect, side WHERE side = 'both'` | Merge preview: what did both sides change? |
| `CALL history(#204, field: 'status')` | Every change to one field, with actor and commit. |
| `CALL blame(#204)` | Who last set each field and edge? |
| `USE lane/http2 CALL conflicts() YIELD key, node, class, base, ours, theirs` | Which conflict values does the lane hold after a merge? |
| `CALL across(refs: [main, lane/http2], ids: [#204, #207]) YIELD node, name, ref, value, diverged WHERE diverged RETURN node, name, ref, value` | Where do these nodes differ between two branches? |

Derived properties such as `unblocked` are recomputed at a past version, and the header says so. Parts of a composite query can read different versions: `MATCH (t:task) WHERE t.unblocked RETURN t EXCEPT USE main~5 MATCH (t:task) WHERE t.unblocked RETURN t` lists the tasks that became unblocked in the last five commits.

## Writes: one TX block, one commit {#writes}

Reads go through `moirai q` and the MCP `query` tool, whose grammar has no write statements; a read executor holds only a read-only view of the store. Writes go through `moirai tx` and the MCP `write` tool. One `TX` block is one commit on one branch, all or nothing:

```console
$ moirai tx - <<'EOF'
TX ON lane/http2 LEASE 'L-18' {
  MATCH (t {id: #207}) WHERE t.status = 'in_progress' EXPECT 1
  SET t.done = true
}
EOF
branch: lane/http2 | rev 4520 -> 4521 | committed c7d02b9e1 | lease L-18
1 MATCH ... EXPECT 1: matched 1 (#207)
  SET #207.done = true: status in_progress->done (resolution completed); lease L-18 released into settled
affected: none | markers: settled #207 (lane/http2)

```

*Designed output.*

The guards are the point of the design:

- **`EXPECT n`** is required on every `MATCH` target: an exact count, a range, `<= n` or `>= n`. Targets are checked again under the writer lock against the tip the block lands on. A mismatch writes nothing, exits with code 4 and prints the current values.
- **`IF TIP c...`** refuses the block if the branch moved. **`IF TARGETS '...'`** refuses it if the matched set differs from the one a dry run showed.
- **`LEASE 'L-n'`** presents the [lease](glossary.html#term-lease) that fences the task against a second worker.
- **`ASSERT expr ELSE 'message'`** checks a condition on the result before anything is written.
- **`DRY`** runs every check without the writer lock and prints the would-be changes and a digest of the target set. Applying the same block with `IF TARGETS` and that digest is plan and apply.
- **`KEY 'k'`** makes the block idempotent. The key is bound to the canonical form of the block with its parameter values and to the branch: a replay with the same payload returns the original result with exit code 0; the same key with a different payload exits with code 9.

If another agent completed `#207` first, the same block fails loudly:

```text
error[E401 expect_mismatch]: statement 1 matched 0 bindings, expected 1
  #207 now: status=done rev=4521 (changed at c3e9a1f00 by dev#3 on lane/http2: "complete #207")
  nothing was written
  = help: re-read with moirai q show ids=207

```

*Designed output.*

A bulk change uses the same tools:

```lq
TX ON lane/http2 KEY 'reprio-h2' IF TIP c4f1a9e20 {
  MATCH (t:task)-[:CHILD_OF]->+(#200) WHERE t.status = 'open' AND 'h2' IN t.labels EXPECT 3
  SET t.priority = 1;
  ASSERT COUNT { MATCH (x:task)-[:CHILD_OF]->+(#200) WHERE x.priority = 0 } = 0
    ELSE 'triage P0 work under #200 first'
} DRY

```

<details>
<summary>The full statement set</summary>

The statements map one to one onto Moirai's change operations, so every write can be recorded, reverted and merged: `CREATE ... [UNDER p] [UNLESS EXISTS { ... }]` (create, or bind to the one existing match; there is no `MERGE`), `SET` and `REMOVE`, `SET t.done = true` (the guarded transition), `SET c = c + k` (a counter increment that never conflicts at merge), `PATCH x.body REMOVE $old ADD $new`, edge `CREATE` and `DELETE`, `MOVE ... UNDER`, `REOPEN ... REASON`, `DELETE ... POLICY ... REPLACED BY ... RELEASE` (the schema's per-edge delete policies; there is no `DETACH`), `RESOLVE` for conflict values, `CALL tx.*` for named mutations, and `DEFINE QUERY` and `DROP QUERY`.

Operations that read or move project files, such as creating a file link or moving a file, are commands, not statements: they are not pure functions of the graph, and a `TX` statement that tries one is refused with the name of the command to use.

</details>

## Named queries and mutations {#named-queries}

Every read command is a named query of the built-in `std` library, and every write command is a `tx.*` named mutation. `moirai q --list` lists them, `moirai q --describe NAME` shows one signature, `--show-query` prints a query's LQ, and `--show-tx` prints a write command's expansion. Arguments are `k=v` pairs typed by the signature: `moirai q blockers id=211 transitive=true`, `moirai q diff range=main...lane/http2`.

A project can add its own. By default only the `orchestrator` and `owner` roles may define one, through the command line:

```lq
TX KEY 'define-stale-in-progress' {
  DEFINE QUERY stale_in_progress($scope: node = #200, $days: int = 3) SHAPE node AS {
    MATCH (t:task)
    WHERE t IN subtree($scope) AND t.status = 'in_progress'
      AND t.updated_at < now() - $days * 1d
    RETURN t ORDER BY t.updated_at
  }
}

```

After that, `moirai q stale_in_progress days=2` runs it. A project query is versioned schema data: it branches and merges with the data it reads, and it is exported with the [git image](git-image.html). It is stored in a portable form, with every node id rewritten to its portable uid and every sequence number or commit prefix to a full commit id (ref names stay as written; reflog positions are refused), so another store that imports the image binds it to the same nodes. A definition is merged as one value, and two definitions that differ only in formatting never conflict. After every merge, the definitions it touched are bound again, and one that no longer binds stops the merge like any other violation.

## Budgets and exit code 10 {#budgets}

Every query runs under counted budgets, not timers, so the same query on the same version always stops at the same row:

| Budget | Default |
|---|---|
| work units (rows examined, edges expanded, and so on) | 2,000,000, about 10 to 40 ms |
| private memory | at most 1 MiB, and never more than the process's headroom under its memory gate; never below 256 KiB |
| rows and bytes per page | 50 rows, 8,000 bytes |
| nodes visited by a closure | 100,000 |
| branches read in one query | 4 |
| file-system units for link states | 400 |
| wall-clock deadline, a safety net only | 2 s from the command line, 5 s over MCP |

A caller can raise a budget up to a per-role ceiling with `--budget`. A long result that merely fills its page is not a failure: it exits with code 0 and a cursor. When a budget is exhausted, the result is incomplete, whatever rows it printed, and the exit code is 10. A refusal before execution, because a query provably exceeds its budget, and a cancellation also exit with code 10. The footer names the budget and how to continue:

```text
fs: 400 units used; 7 links unverified | --budget fs=800 | exit 10

```

*Designed output.*

## Error messages written for models {#errors}

One retry on a precise error recovers most failed queries, so LQ moves every mistake it can detect from "valid but wrong" into an error, a warning or a notice. Each error names exactly one fix, prints the replacement text when the fix is mechanical, and draws its suggestions from the current schema, nearest first, at most five:

```text
error[E101 unknown_field]: kind `task` has no field `stauts`
 --> <stdin>:2:9
  |
2 | WHERE t.stauts = 'open'
  |         ^^^^^^ did you mean `status`?
  = help: task fields: status, priority, labels, assignee, work_kind, ... (CALL schema(kind: 'task'))

```

*Designed output.*

An error text is at most 600 bytes. Its fixed wording is ASCII (names and values taken from your query or the store keep their own characters), and it carries no exit-code line, because the agent's shell tool already reports the code. Warnings and notices do not change the exit code; they sit in the footer and point at the likely mistake.

<details>
<summary>Forms that are not in LQ, and what to write instead</summary>

| Written | LQ form |
|---|---|
| `MERGE` | `CREATE ... UNLESS EXISTS { ... }` |
| `DETACH DELETE` | `DELETE` with the schema's per-edge policies |
| path variables, `shortestPath()` | `CALL blockers(#N, transitive: true) YIELD blocker, depth, via` |
| `SKIP`, `OFFSET` | the cursor of the previous page |
| list comprehensions, `x[i]` | `any()`, `all()`, `none()`, `IN`, `UNWIND` |
| `=~` (regular expressions) | `CONTAINS`, `STARTS WITH`, `glob_match()`, `search()` |
| `= NULL` | `IS NULL` |
| `:note:rule` | `:note\|rule` (a node has one kind) |
| a write statement in `moirai q` | `moirai tx` or the MCP `write` tool |

</details>

## LQ-Bench: the gate that freezes the language {#lq-bench}

Agents learn LQ from a skill card of at most 1,000 tokens and 3,500 bytes. The draft card has seven examples, chosen for the parts no other language has: `#N`, built-ins, `USE`, ranges and `TX`; the benchmark decides how many the final card keeps. A longer reference is loaded only on demand. `CALL schema()` lists the branch's kinds and fields, and `CALL schema_edges()` its edge kinds with their reverse aliases and readings.

LQ-Bench is a permanent test that measures whether this is enough. It runs a model with the card and the real tool surface against a synthetic store of about 2,000 nodes, shaped like real project work: lanes, merges with conflict values, deleted nodes, flagged blockers and file links in every state. The tasks include 150 synthetic questions, each in three phrasings that use the domain's words rather than LQ's, and 40 adversarial tasks aimed at known traps: reversed edges, counts over anonymous elements, `= NULL`, hand-written readiness, states at past versions, and others.

The surface freezes only when every gate holds:

- first-try accuracy of at least 85 %, and at least 95 % after one retry, on the literal and short phrasings;
- confidently wrong answers (exit code 0, nothing flagged, wrong result) at most 2 % of reads overall and at most 5 % for any single construct, and none at all for writes;
- a named query used in at least 80 % of the cases where one exists;
- no group of tasks below 75 % after one retry;
- the card within 1,000 tokens.

The gates will first be measured on one model, Claude Opus 5.5. The benchmark itself is not built yet; see [the crates](building.html#status-crates). Each run sets a model profile for the model family it measured: `gated` for a model that passed the write gates, `compatible` for one that reaches 75 % after one retry on every group of tasks, and `unknown` otherwise. A model with the `unknown` profile writes only through named mutations, and the reading echo is always on for `compatible` and `unknown` models. A failed gate may change the card, an error text, a warning, a built-in or even the semantics, but never the benchmark. The benchmark also decides open choices by comparison, such as whether quantifiers are displayed in Cypher's or GQL's spelling. It runs first against the project's reference model, before any engine code exists, and again on every change to the card, the grammar, the error texts or the warnings. After the freeze, a change of meaning needs a new grammar version, and every stored named query keeps the meaning of the version it was written in.
