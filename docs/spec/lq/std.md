# LQ standard library: the `std` read catalog, pack and brief classes, and the `tx` named mutations

| | |
|---|---|
| Title | Signatures, shapes, budget classes and the LQ text of every standard named query and pack/brief class M0 needs, and the signatures and expansions of the `tx.*` named mutations |
| Chapter | [LQ/std], `docs/spec/lq/std.md` |
| Status | draft, pass 1 pending |
| Work package | WP-19b (R-SPEC-F), part of WP-19 of [PLAN §3.2] item 1 |
| Sources | [50 §4.1]–[50 §4.4] (catalog, definitions, write verbs, pack and brief inputs, project named queries), [50 §2.6] (built-ins and table functions), [50 §3.5], [50 §3.8]–[50 §3.10], [50 §5.10] (budget classes), [50 §6.1] (argv forms), [50 §6.3] (MCP `query` and `write`), [50 §7.4] item 3; [AR §2.11] (the `find` filters), [AR §3.2]–[AR §3.6] (fields, edges, derived state, status machines), [AR §6.2] (claims, `complete`), [AR §7.1] (verbs), [AR §7.2], [AR §7.3], [AR §7.4] (pack classes C1–C8, brief), [AR §7.7.2]; [40 §6.3] (the file-link named mutations), [40 §6.5]; [90 §4.3], [90 §6.6] (`"k=v"` parameters); the WP-80a re-review files `reviews/a1-A.md` (A-m2, A-m5, A-m6) and `reviews/a1-S.md` (S-05, S-07); [LQ/grammar-v1.ebnf §P.1] and O-4, O-5; [LQ/lexical §4.2], §9 |
| Depends on | [LQ/grammar-v1.ebnf], [LQ/lexical], [LQ/canonical-ast] (argument naming, lookup order), [LQ/envelope] (shapes), [LQ/errors], [F08] (fields, F1, F3), [F11] (`LEASES`, `MARKERS`), [F12] (conflict keys), [F18] (R-16, R-17) |

## 1. Scope

1.1. This chapter is the standard library as LQ text: the `std` read catalog behind every read verb ([50 §4.1]), the pack and
brief candidate classes ([50 §4.3], [AR §7.4]) and the `tx.*` named mutations behind every write verb ([50 §4.2]). Every
`lq-define` block parses with start symbol `define_stmt` and every `lq-tx` block with start symbol `tx` of grammar v1
([LQ/grammar-v1.ebnf §P.1]; each was checked against [50 §2.3]'s EBNF by a scratch parser that also accepts [50 §2.9]'s
examples).

1.2. The pack and brief *algorithms* (quotas, levels L0–L2, budgets, rendering, the relevance floors) stay in code
([50 §4.3]); the classes here only decide membership, so `pack --explain` can name the query behind each item and the reference
model can test class membership against the same definitions.

1.3. Project named queries (`DEFINE QUERY` by the orchestrator, [50 §4.4]) share the catalog model of §2 but are data of a store,
not of this chapter.

1.4. The library freezes at WP-72 with the query surface; the pack and brief classes are also a rule table the owner signs
(V3, [PLAN §3.2] WP-90).

## 2. The catalog model

2.1. **Namespaces and lookup.** The standard library lives in the namespace `std`; named mutations in `tx`. Both are reserved: a
project cannot define `std.*`, `tx.*`, or a name equal to an LQ keyword ([50 §4.4]). Lookup:
- `moirai q NAME`, MCP `query` `name`, and a verb alias ([AR §7.7.2]): `std`, then the project catalog;
- `CALL name(...)` inside a query: first the built-in relations of [50 §2.6], then `std`, then the project; `CALL std.name(...)`
  names the standard query explicitly. So `std.blockers`, `std.history`, `std.log`, `std.diff`, `std.across`, `std.conflicts`,
  `std.violations`, `std.changes` and `std.root_moves`, whose text calls the built-in relation of the same name, never call
  themselves (open point 1);
- `CALL tx.name(...)` inside a `TX` block: the `tx` catalog of §7.

2.2. **Signatures.** A signature is the `param_decl` list of a `DEFINE QUERY`: `$name: type [?] [= default]`, `?` marking an
optional parameter whose absence binds `NULL`. The v1 parameter types are `node`, `int`, `float`, `bool`, `text`, `rev`,
`timestamp`, `duration`, `range<int>`, `list<node>`, `list<int>`, `list<text>` and `list<rev>`; any other type in a definition is
E110 ([LQ/errors §5.3]). A `rev` argument value (an argv or MCP `k=v` value, a `$param` value, or a quoted string the binder
coerces) is read by the revision-mode rules of [LQ/lexical §7] and may be a revspec or a range (`main..lane/x`, `a...b`);
inside LQ text, a named query's `rev` parameter is never itself a revision position ([LQ/lexical §4.2], §2.8 item 1). Arguments
arrive as ([50 §6.1], [90 §6.6]):
- argv `k=v` or `k:v`, and MCP `params` as an array of `"k=v"` strings (one grammar for both);
- node values as bare integers (`scope=88`; `#88` and `#u:<32 hex>` are accepted, [80 §4] T1);
- lists comma-separated (`ids=40,41,52`); ranges `a..b`, `..b`, `a..`, or a single integer `a` meaning `a..a`;
- booleans `true`/`false`; timestamps ISO 8601; durations `<int><s|m|h|d|w>`;
- positional values: the first required parameter takes the positional arguments (`moirai q show 40 41` binds `ids`;
  `moirai q blockers 51` binds `id`); a `list` parameter collects every positional value. `find`, whose parameters are all
  optional, takes its positional argument as `text` (the [AR §7.1] form `moirai find [TEXT] [k:v ...]`).

A value that does not convert, an unknown parameter and a missing required one are E110 ([LQ/errors §5.3]).

2.3. **Shapes.** Each named query declares one output shape ([50 §4.1], [50 §6.4]); the renderings are [LQ/envelope §5]'s. The
closed v1 set: `node`, `table`, `tree`, `detail`, `diff`, `history`, `conflict`, `violation`, `across`, `loop`, `links`,
`changes`, `blockers`. In the `node` shape, columns after the first render as extras ([LQ/envelope §5.4]).

2.4. **Budget classes.** `BUDGET light`, `medium` or `heavy` sets the default `work` budget of a run of the query to a tenth of,
once, or ten times `query.budget.default.work` ([CFG §10.5]; pass 1, A1-37, [CFG] open point 18), which gives 200,000,
2,000,000 or 20,000,000 units at the key's default ([50 §4.1]: "light ≤ 2e5 units, medium ≤ 2e6, heavy ≤ 2e7"). The key is
runtime policy, so a store that changes it moves all three classes. Every other budget keeps its default ([50 §5.10]). A
caller raises a budget up to its role's cap as for any query. A definition without `BUDGET` is `medium`.

2.5. **Cursor class.** Each query is `pinned` or `live` by the binder's rule ([50 §3.5]: live when it reads runtime or
tree-derived state). The catalog's cursor column records the expected class; a definition whose bound class differs from its
column is a defect of this chapter, caught by the catalog test of §9.

2.6. **Parse units and fixture files.** Every `lq-define` block holds one `define_stmt` of grammar v1, parsed on its own (not as a
`write_input`: the library is compiled in, not written by a `TX`). R-FIX copies each block byte-exact into
`fixtures/lq/std/<name>.lq` (PLAN §2.5): LF line ends, no trailing whitespace, one final LF. The stored grammar version of every
definition is 1.

2.7. **Rendering parameters.** A parameter that the text does not reference is a rendering parameter of the shape: `show`'s
`$full`, and the `$role` of `pack_spec`. It still takes part in argument checking.

2.8. **The built-in relations and functions the library relies on** ([50 §2.6]); this chapter states four points [50] leaves
open:
1. **Revision-typed arguments** are the revision positions of [LQ/lexical §4.2]: argument 0 or `range` of `diff` and `log`;
   `since` and `ref` of `changes` (`ref: rev`, the ref whose feed is read; the answer to [LQ/grammar-v1.ebnf] O-5); `in` of
   `history`; `refs` of `across`; argument 0 or `ref` of `violations` (`ref: rev`, optional, the staging ref to read, equal to a
   `USE` of that ref; the answer to [LQ/grammar-v1.ebnf] O-4). A `NULL` value of a named argument means the argument is not
   given. A standard or project query's revision-typed parameter is passed as a `$param` or a quoted revspec, never in revision
   mode ([LQ/lexical §4.2]).
2. **`links(scope: n)`** yields `node, anchor, file, path, kind, scope, state, evidence, next`: the column [50 §2.6] calls
   `fix` is named `next` and holds the next command — the evidence command for a proposal, the settle command for an exact
   state — never a command that accepts a guess (the A1 review's A-m2, [41 M6], [40 §6.1]'s JSON `next`).
3. **Total link states** ([50 §2.6] as amended for the A1 re-review's S-05). `link_state(n)` of a node with no `AT` edge is the
   frozen string `none`, and `a.state` of an anchor whose file did not resolve to `ok`/`moved-auto` is `unresolved`, never an
   absent value; a `<>` or `NOT IN` over the node form draws W10 ([LQ/errors §5.6]), because `'none' <> 'ok'` is true. Both
   strings are in R-16's closed set ([F18]).
4. **No resolvable tree** (the A1 review's S-07). A tree-derived built-in with no eligible tree is E302 with the `--tree` hint in
   every LQ read; `unverified (no tree)` is only a rendering of packs, briefs and the `links check` verb (§4.21).

2.9. **Signatures of the built-in relations.** [50 §2.6] gives each relation's arguments partly positionally; every parameter
is named here, the first positional one included, so that the binder can name and order every call's arguments by the callee's
signature ([LQ/canonical-ast] open point C-5). A call may pass the first parameter positionally or by name; `?` marks an
optional parameter; the yields are [50 §2.6]'s with the §2.8 corrections. Revision-typed parameters are the revision positions of
§2.8 item 1.

| Relation | Parameters | Yields |
|---|---|---|
| `blockers` | `n: node`, `transitive: bool = false` | `blocker, depth, via, reason, flagged, elsewhere` |
| `subtree` | `n: node`, `depth: int = 3` | `node, depth, parent, position` |
| `neighbors` | `n: node`, `depth: int = 1`, `types: list<text>?` | `node, edge, dir, depth` |
| `search` | `terms: text`, `kinds: list<text>?`, `fields: list<text> = ['title', 'abstract']` | `node, score, field, snippet` |
| `history` | `n: node`, `field: text?`, `in: rev?` | `seq, commit, ref, actor, role, at, op, aspect, name, before, after, message, via` |
| `blame` | `n: node` | `aspect, name, value, seq, commit, actor, at` |
| `log` | `range: rev?`, `actor: text?`, `touching: node?` | `commit, seq, ref, kind, actor, role, at, message, ops` |
| `diff` | `range: rev`, `scope: node?` | `change, node, kind, aspect, name, before, after, side, last_commit, actor` |
| `changes` | `since: rev`, `ref: rev?` | `seq, ref, commit, node, op, aspect, name, actor, affected` |
| `conflicts` | none | `key, node, class, base, ours, theirs, commit, hint` |
| `violations` | `ref: rev?` | `key, class, detail, suggested` |
| `across` | `refs: list<rev>`, `ids: list<node>`, `aspects: list<text>?` | `node, aspect, name, ref, value, diverged` |
| `refs` | none | `name, kind, tip, seq, ahead, behind, fork, staged` |
| `leases`, `markers` | none | the row fields of `LEASES` and `MARKERS` ([F11]) |
| `links` | `scope: node?` | `node, anchor, file, path, kind, scope, state, evidence, next` |
| `root_moves` | `root: text = 'project'` | `hlc, class, from, to, git` |
| `schema` | `kind: text?` | `kind, field, type, optional, index` ([50 §2.9] Q22) |
| `schema_edges` | none | `name, stored, src_kinds, dst_kinds, symmetric, reverse_names, reading, class, acyclic` (the F1 row, [F08]) |
| `queries` | none | `name, signature, shape, budget, lq, text` (the F3 item, [F08]) |

`search`'s `fields` default is `['title', 'abstract']`: bodies are opt-in ([50 §2.6], [AR §2.11]).

2.10. **Signatures of the scalar built-in functions** (pass 1, A1-54). The functions of [50 §2.6] with LQ-specific meaning; the
generic scalars (`size`, `lower`, `substring`, `coalesce`, `round` and the rest of [LQ/canonical-ast] Table 5.3) keep their
Cypher signatures. Function names and relation names are separate namespaces, so these defaults are not §2.9's: the scalar
`subtree` has **no depth bound** when `depth` is omitted, where the relation `subtree` defaults to 3.

| Function | Parameters | Returns |
|---|---|---|
| `subtree` | `n: node`, `depth: int?` (omitted: unbounded) | set of nodes: n and its `CHILD_OF` descendants within `depth` levels |
| `descendants` | `n: node`, `depth: int?` (omitted: unbounded) | set of nodes: as `subtree`, n excluded |
| `ancestors` | `n: node` | set of nodes: n's `parent` chain, n excluded |
| `children` | `n: node` | set of nodes: n's direct children |
| `applies` | `k: node`, `glob: text` | bool |
| `applies_role` | `k: node`, `role: text` | bool |
| `applies_phase` | `k: node`, `phase: text` | bool |
| `fits_role` | `t: node`, `role: text` | bool |
| `glob_match` | `path: text`, `glob: text` | bool |
| `text_match` | `n: node`, `terms: text` | bool |
| `file` | `path: text`, `root: text = 'project'` | node or absent |
| `link_state` | `x: node or edge` | text |
| `staleness` | `n: node` | text |
| `relevant_to` | `n: node`, `agent: text` | bool |
| `now`, `me`, `view_ref` | none | timestamp, text, text |
| `datetime` | none (then `now()`), or `s: text` | timestamp |
| `date` | `s: text` | timestamp |
| `duration` | `s: text` | duration |

So `t IN subtree(#88)`, as the card and `std.ready` write it, is the whole subtree of #88.

2.11. **Built-in node properties** (spec sync 2a; [50 §2.5]'s groups with their kinds, types and classes fixed). The binder
types a property read by this table; "every kind" includes project kinds. The class is what E115 names when a `SET` targets
the property ([LQ/errors §5.3]; `identity` renders `an identity field`), and runtime and tree-derived properties are E302 away
from a branch tip ([50 §3.8]). A property on a kind the table does not list reads as absent there ([LQ/canonical-ast §5.10]);
one that no kind of the variable's kind set has is E101.

| Property | Kinds | Type | Class | Absent |
|---|---|---|---|---|
| `id` | every kind | node | identity | never |
| `uid` | every kind | text: 32 lower-case hex digits | identity | never |
| `kind` | every kind | text: the kind's declared name ([F08 §8.2]); `labels(n)` is `[n.kind]` | identity | never |
| `created` | every kind | rev: the creating commit (`created_tx`, [F08 §3.1]) | identity | never |
| `created_at` | every kind | timestamp: when this store recorded the creating commit ([50] F14) | identity | never |
| `created_by`, `created_role` | every kind | text: the `CREATOR` actor and role ([F08 §4]) | identity | `created_role` when the creating commit had no role |
| `updated` | every kind | rev: `updated_tx` | derived | never |
| `rev` | every kind | rev: `rev_seq`, the `--if-rev` target | derived | never |
| `updated_at`, `updated_by` | every kind | timestamp; text | derived | never |
| `done`, `unfinished` | the kinds with `has_done` ([F08 §8.5.1]): task, question, verdict | bool; `unfinished` = `NOT done` | derived | on every other kind |
| `container`, `ready_to_close`, `is_blocker`, `suspect`, `conflicted`, `has_dangling` | every kind | bool (false where the definition's kind clause fails, [AR §3.5]) | derived | never |
| `unblocked`, `blocked` | every kind | bool (false on every kind but task, [AR §3.5]) | derived | never |
| `children_total`, `children_done`, `open_blockers` | every kind (header columns, [F08 §3.1]) | int (0 where nothing counts) | derived | never |
| `answered` | question | bool | derived | on every other kind |
| `depth` | every kind | int: the number of `parent` links up to a root (0 at a root) | derived | never |
| `topo` | every kind | int ([F08 §3.4]) | derived | never |
| `ready`, `claimed` | every kind | bool (false on every kind but task) | runtime | never |
| `lease` | every kind | map: `holder` text, `token` int, `expires` timestamp, `run` text, `branch` text | runtime | without a live lease |
| `settled_elsewhere`, `deleted_elsewhere` | every kind | bool | runtime | never |
| `state` (`f.state`) | artifact | text: [F18 §4]'s link state | tree-derived | never at a tip with a resolved tree |

A node bound through the pseudo-label `DELETED` ([50 §3.6]) has `id`, `uid`, `kind` (at deletion) and `title` (text), and
the tombstone properties `deleted_by` (text), `deleted_at` (timestamp), `deleted_reason` (text; absent without one) and
`replaced_by` (node; absent without one), all of class tombstone. Kind fields keep their [F08 §9] types, with the
`coerce = timestamp` rule of §2.13.

2.12. **Types of the yielded columns** (spec sync 2a). The relations of §2.9 yield these types; `any` is a column whose type
varies by row (the value of the key the row describes), which the binder does not type-check and the evaluator treats as the
value it holds. A named query's columns are those of its `RETURN`, typed by binding its definition.

| Relation | Columns and types |
|---|---|
| `blockers` | `blocker` node, `depth` int, `via` node (absent for a direct blocker), `reason` text (`direct`, `inherited`), `flagged` bool, `elsewhere` bool |
| `subtree` | `node` node, `depth` int, `parent` node (absent at the root), `position` int |
| `neighbors` | `node` node, `edge` text (the edge kind's LQ name), `dir` text, `depth` int |
| `search` | `node` node, `score` float, `field` text, `snippet` text |
| `history` | `seq` int, `commit` rev, `ref` text, `actor` text, `role` text, `at` timestamp, `op` text, `aspect` text, `name` text, `before` any, `after` any, `message` text, `via` text |
| `blame` | `aspect` text, `name` text, `value` any, `seq` int, `commit` rev, `actor` text, `at` timestamp |
| `log` | `commit` rev, `seq` int, `ref` text, `kind` text, `actor` text, `role` text, `at` timestamp, `message` text, `ops` int |
| `diff` | `change` text, `node` node, `kind` text, `aspect` text, `name` text, `before` any, `after` any, `side` text, `last_commit` rev, `actor` text |
| `changes` | `seq` int, `ref` text, `commit` rev, `node` node, `op` text, `aspect` text, `name` text, `actor` text, `affected` any |
| `conflicts` | `key` text, `node` node (absent for a schema key), `class` text, `base` any, `ours` any, `theirs` any, `commit` rev, `hint` text |
| `violations` | `key` text, `class` text, `detail` text, `suggested` text |
| `across` | `node` node, `aspect` text, `name` text, `ref` text, `value` any, `diverged` bool |
| `refs` | `name` text, `kind` text, `tip` rev, `seq` int, `ahead` int, `behind` int, `fork` rev (absent without a fork), `staged` bool |
| `leases`, `markers` | each field of the [F11] row: a `#N` as node, a commit as rev, a time as timestamp, a symbol id as its text, a list of symbols as `list<text>`, every other number as int |
| `links` | `node` node, `anchor` text, `file` node, `path` text, `kind` text, `scope` text, `state` text, `evidence` text, `next` text |
| `root_moves` | `hlc` int, `class` text, `from` text, `to` text, `git` text |
| `schema` | `kind` text, `field` text, `type` text, `optional` bool, `index` text |
| `schema_edges` | `name` text, `stored` text, `src_kinds` list<text>, `dst_kinds` list<text>, `symmetric` bool, `reverse_names` list<text>, `reading` text, `class` text, `acyclic` text |
| `queries` | `name` text, `signature` text, `shape` text, `budget` text, `lq` int, `text` text |

A `seq` column is an int, not a revision: compare it with integers, or pass it to a revision position as `s<seq>`.

2.13. **Fields with `coerce = timestamp`** ([F08 §8.4.4]; spec sync 2a). These fields (`defer_until`, `due` and the kind fields
[F08 §9.3] marks so) store Unix seconds, while an LQ `timestamp` is milliseconds ([LQ/canonical-ast §5.5]). The binder types
such a field as `timestamp`, and the evaluator scales it: a read yields the stored seconds × 1,000; a write (`SET`, a `CREATE`
property map, a named mutation's argument) stores ⌊milliseconds / 1,000⌋, and a value outside 1 … 2^32 − 1 seconds is E405
`schema conformance (I11)`. A literal compared with such a field is coerced to a `TIMESTAMP` in milliseconds, so the scale
never enters a C-AST or a hash, and renderings print the field as a timestamp ([LQ/envelope]).

2.14. **Edges with a `DELETED` endpoint** ([50 §3.6]; spec sync 2a). An edge to or from a tombstone exists only where
the delete policy retains it ([RULES/delete-policy-matrix] EG rows, [F08 §9.6]): as its destination, every historical
kind (ids 10–25: `supersedes`, `derived_from`, `cites`, `implements`, `refutes`, `confirms`, `verifies`, `addresses`,
`about`, `discovered_from`, `produced`, `consumed`, `contradicts`, `mentions`, `relates`, `at`) and every project edge
kind (always `tombstone`/`retain`, [F08 §8.5.4]); as its source, the same kinds and the structural `blocks` and `gates`,
whose edges a delete without replacement flags (EG-008, EG-014). The other structural kinds (`parent`, `merge_after`,
`runs_in`, `answers`, `scoped_to`, `duplicate_of`, `depends_on`) never have a tombstone endpoint, so for them the
pseudo-label `DELETED` is not among the endpoint kinds of [LQ/canonical-ast §5.10]'s kind sets, and a pattern that puts
it on such an end is E106.

## 3. The read catalog

| Name | Signature | Shape | Order | Class | Cursor | Verb alias ([AR §7.1]) |
|---|---|---|---|---|---|---|
| `ready` | `$scope: node?, $role: text?, $limit: int = 20` | node | priority, id | light | live | `ready [--scope ID] [--role R] [--limit N]` |
| `blocking` | `$scope: node?` | node | id | light | live | `blocking [--scope ID]` |
| `blockers` | `$id: node, $transitive: bool = false` | blockers | depth, blocker | light | live | `blockers ID [--transitive]` |
| `tree` | `$id: node, $depth: int = 3` | tree | position | medium | pinned | `tree ID [--depth N]` |
| `show` | `$ids: list<node>, $full: bool = false` | detail | id | light | pinned | `show ID.. [--full]` |
| `find` | `$kind, $status, $prio: range<int>, $label, $area: node, $text, $done, $suspect, $conflicted` (all optional), `$limit: int = 50` | node | id | medium | pinned | `find [TEXT] [k:v ...]` |
| `notes` | `$path: text, $role: text?` | node | criticality, authority, id | light | pinned | `notes --path P [--role R]` |
| `changes` | `$since: rev, $about: list<node>?, $for_agent: text?, $all: bool = false` | changes | seq | light | pinned | `changes --since SEQ [--all] [--about ID,..] [--for-agent A]` |
| `stale` | `$scope: node?` | node | id | medium | live | `stale [--scope ID]` |
| `conflicts` | `$scope: node?` | conflict | node, key | light | pinned | `conflicts [REF]` (REF is the view, `--branch`) |
| `violations` | `$ref: rev` | violation | key | light | pinned | — |
| `history` | `$id: node, $field: text?, $range: rev?` | history | seq desc | light | pinned | `log --node N` |
| `blame` | `$id: node, $field: text?` | table | aspect, name | light | pinned | `blame N [FIELD]` |
| `log` | `$range: rev?, $actor: text?, $touching: node?` | history | seq desc | light | pinned | `log [REF] [--actor A]` |
| `diff` | `$range: rev, $scope: node?, $aspect: text?, $side: text?` | diff | node, aspect, name | medium | pinned | `diff A..B` / `diff A...B` |
| `across` | `$refs: list<rev>, $ids: list<node>` | across | node, name, ref | medium | pinned | `show ID.. --across` |
| `loop` | `$plan: node, $round: int?` | loop | round | light | pinned | `stats loop P` |
| `refuted_share` | `$role: text, $round: int?` | table | round | light | pinned | `stats refuted-share --role R [--round k]` |
| `lane_conflicts` | `$a: node, $b: node` | table | path | medium | live | `lane conflicts L1 L2` |
| `delta` | `$since: rev, $agent: text` | changes | seq | light | pinned | the `UserPromptSubmit` hook |
| `links_broken` | `$scope: node?` | links | node, anchor | medium | live | `links check [--scope ID]` |
| `links_pending` | `$scope: node?` | links | node, anchor | medium | live | — |
| `links_proposals` | `$scope: node?` | links | node, anchor | medium | live | — |
| `links_guesses` | `$scope: node?` | links | node | medium | pinned | — |
| `files_removed` | `$scope: node?` | links | node | medium | pinned | — |
| `files_replaced` | `$scope: node?` | links | node | medium | live | — |
| `root_moves` | `$root: text = 'project'` | table | hlc, from, to | light | pinned | — |

`find`'s optional parameters are `$kind: text?`, `$status: text?`, `$label: text?`, `$text: text?`, `$done: bool?`,
`$suspect: bool?`, `$conflicted: bool?`. `$done` is added to [50 §4.1]'s signature so [AR §2.11]'s `done:false` filter has a
parameter (open point 3).

## 4. Definitions of the read catalog

4.1. **`ready`** ([50 §4.1], verbatim but for `BUDGET` and the order). The planner anchors it on `subtree($scope)` when a scope is
given and on the maintained `unblocked` bitset otherwise ([50 §4.1]). The order is `priority, id`: [50 §4.1]'s `priority, topo, id`
is not total, because no precedence path joins two ready tasks and `topo` among them is whatever valid order Pearce–Kelly kept
([F08 §3.4]), so the model and the engine would print different first pages (pass 1, A1-38; [API] open point 11). The owner
decided this order on 2026-09-28 (OQ-F-2 (a)); [50 §4.1] is corrected at WP-81a.

```lq-define
DEFINE QUERY ready($scope: node? = NULL, $role: text? = NULL, $limit: int = 20) SHAPE node BUDGET light AS {
  MATCH (t:task)
  WHERE t.ready
    AND ($scope IS NULL OR t IN subtree($scope))
    AND ($role IS NULL OR fits_role(t, $role))
  RETURN t ORDER BY t.priority, t.id LIMIT $limit
}
```

4.2. **`blocking`** ([50 §4.1]). "The ids of all blocking tasks" = the `is_blocker` bitset ∧ `kind:task`, markers honoured
([AR §3.5], [AR §7.1]).

```lq-define
DEFINE QUERY blocking($scope: node? = NULL) SHAPE node BUDGET light AS {
  MATCH (t:task)
  WHERE t.is_blocker AND NOT t.settled_elsewhere
    AND ($scope IS NULL OR t IN subtree($scope))
  RETURN t ORDER BY t.id
}
```

4.3. **`blockers`** — the engine's definition through the built-in relation ([50 §2.6]: inherited, flagged and settled-elsewhere
blockers).

```lq-define
DEFINE QUERY blockers($id: node, $transitive: bool = false) SHAPE blockers BUDGET light AS {
  CALL blockers($id, transitive: $transitive) YIELD blocker, depth, via, reason, flagged, elsewhere
  RETURN blocker, depth, via, reason, flagged, elsewhere
  ORDER BY depth, blocker
}
```

4.4. **`tree`**.

```lq-define
DEFINE QUERY tree($id: node, $depth: int = 3) SHAPE tree BUDGET medium AS {
  CALL subtree($id, depth: $depth) YIELD node, depth, parent, position
  RETURN node, depth, parent, position ORDER BY position
}
```

4.5. **`show`** ([50 §4.1]; `--at REV` supplies the view). Ids that yield no row are reported by N01, N06 or N12 and the result
exits 3 ([LQ/envelope §5.6]).

```lq-define
DEFINE QUERY show($ids: list<node>, $full: bool = false) SHAPE detail BUDGET light AS {
  MATCH (n) WHERE n IN $ids RETURN n ORDER BY n.id
}
```

4.6. **`find`** ([50 §4.1] with `$done`). `moirai q find kind:task status:open prio:..1 label:l5` passes intact in both shells
([50 §4.1]).

```lq-define
DEFINE QUERY find($kind: text? = NULL, $status: text? = NULL, $prio: range<int>? = NULL,
                  $label: text? = NULL, $area: node? = NULL, $text: text? = NULL,
                  $done: bool? = NULL, $suspect: bool? = NULL, $conflicted: bool? = NULL,
                  $limit: int = 50) SHAPE node BUDGET medium AS {
  MATCH (n)
  WHERE ($kind IS NULL OR n.kind = $kind)
    AND ($status IS NULL OR n.status = $status)
    AND ($prio IS NULL OR n.priority IN $prio)
    AND ($label IS NULL OR $label IN n.labels)
    AND ($area IS NULL OR EXISTS { (n)-[:SCOPED_TO]->(a) WHERE a IN subtree($area) })
    AND ($text IS NULL OR text_match(n, $text))
    AND ($done IS NULL OR n.done = $done)
    AND ($suspect IS NULL OR n.suspect = $suspect)
    AND ($conflicted IS NULL OR n.conflicted = $conflicted)
  RETURN n ORDER BY n.id LIMIT $limit
}
```

4.7. **`notes`** ([50 §4.1], verbatim but for `BUDGET`). The planner anchors it on `file($path)` through `PATHIDX` and the
reverse `AT` adjacency ([50 §4.1]).

```lq-define
DEFINE QUERY notes($path: text, $role: text? = NULL) SHAPE node BUDGET light AS {
  MATCH (k:note|rule|decision)
  WHERE k.status IN ['active', 'accepted']
    AND (applies(k, $path)
         OR EXISTS { (k)-[:SCOPED_TO]->(a:area) WHERE applies(a, $path) }
         OR EXISTS { (k)-[:AT]->(f:artifact) WHERE f = file($path) })
    AND ($role IS NULL OR applies_role(k, $role))
  RETURN k ORDER BY k.criticality, k.authority, k.id
}
```

4.8. **`changes`**. `$all` maps the verb's `--all` (every ref); without it the feed is the view's ref ([AR §6.3], open point 3).

```lq-define
DEFINE QUERY changes($since: rev, $about: list<node>? = NULL, $for_agent: text? = NULL, $all: bool = false) SHAPE changes BUDGET light AS {
  CALL changes(since: $since) YIELD seq, ref, commit, node, op, aspect, name, actor, affected
  WHERE ($all OR ref = view_ref())
    AND ($about IS NULL OR node IN $about)
    AND ($for_agent IS NULL OR relevant_to(node, $for_agent))
  RETURN seq, ref, node, op, aspect, name, actor ORDER BY seq
}
```

4.9. **`stale`** ([50 §4.1]: `staleness(n) <> 'fresh'`, pure; only `moirai check` records facts). Staleness exists for
measurements (`measured_on`) and notes (`observed_git_sha`) ([AR §3.5]); the scope is the subtree the node is about or scoped to
(open point 4).

```lq-define
DEFINE QUERY stale($scope: node? = NULL) SHAPE node BUDGET medium AS {
  MATCH (n:measurement|note)
  WHERE (n.measured_on IS NOT NULL OR n.observed_git_sha IS NOT NULL)
    AND staleness(n) <> 'fresh'
    AND ($scope IS NULL OR EXISTS { (n)-[:ABOUT|SCOPED_TO]->(x) WHERE x IN subtree($scope) })
  RETURN n ORDER BY n.id
}
```

4.10. **`conflicts`**.

```lq-define
DEFINE QUERY conflicts($scope: node? = NULL) SHAPE conflict BUDGET light AS {
  CALL conflicts() YIELD key, node, class, base, ours, theirs, commit, hint
  WHERE $scope IS NULL OR node IN subtree($scope)
  RETURN key, node, class, base, ours, theirs, commit, hint ORDER BY node, key
}
```

4.11. **`violations`** ([50 §4.1]: `USE $ref CALL violations()`).

```lq-define
DEFINE QUERY violations($ref: rev) SHAPE violation BUDGET light AS {
  USE $ref
  CALL violations() YIELD key, class, detail, suggested
  RETURN key, class, detail, suggested ORDER BY key
}
```

4.12. **`history`**. A `NULL` named argument of a built-in relation means the argument is not given.

```lq-define
DEFINE QUERY history($id: node, $field: text? = NULL, $range: rev? = NULL) SHAPE history BUDGET light AS {
  CALL history($id, field: $field, in: $range)
  YIELD seq, commit, ref, actor, role, at, op, aspect, name, before, after, message, via
  RETURN seq, commit, ref, actor, role, at, op, aspect, name, before, after, message, via
  ORDER BY seq DESC
}
```

4.13. **`blame`** ([50 §2.9] Q14 prints a table ordered by aspect and name; open point 5).

```lq-define
DEFINE QUERY blame($id: node, $field: text? = NULL) SHAPE table BUDGET light AS {
  CALL blame($id) YIELD aspect, name, value, seq, commit, actor, at
  WHERE $field IS NULL OR name = $field
  RETURN aspect, name, value, seq AS rev, commit, actor ORDER BY aspect, name
}
```

4.14. **`log`**. A `NULL` positional range means the history reachable from the view ([50 §3.9] item 4).

```lq-define
DEFINE QUERY log($range: rev? = NULL, $actor: text? = NULL, $touching: node? = NULL) SHAPE history BUDGET light AS {
  CALL log($range, actor: $actor, touching: $touching)
  YIELD commit, seq, ref, kind, actor, role, at, message, ops
  RETURN seq, commit, ref, actor, role, at, kind, message, ops ORDER BY seq DESC
}
```

4.15. **`diff`**.

```lq-define
DEFINE QUERY diff($range: rev, $scope: node? = NULL, $aspect: text? = NULL, $side: text? = NULL) SHAPE diff BUDGET medium AS {
  CALL diff($range, scope: $scope)
  YIELD change, node, kind, aspect, name, before, after, side, last_commit, actor
  WHERE ($aspect IS NULL OR aspect = $aspect) AND ($side IS NULL OR side = $side)
  RETURN change, node, kind, aspect, name, before, after, side, last_commit, actor
  ORDER BY node, aspect, name
}
```

4.16. **`across`**. `ref` is added to [50 §4.1]'s order (node, name) so the order is total.

```lq-define
DEFINE QUERY across($refs: list<rev>, $ids: list<node>) SHAPE across BUDGET medium AS {
  CALL across(refs: $refs, ids: $ids) YIELD node, aspect, name, ref, value, diverged
  RETURN node, aspect, name, ref, value, diverged ORDER BY node, name, ref
}
```

4.17. **`loop`** — [50 §2.9] Q6 with parameters ([50 §4.1]); the renderer adds the verdict line ([LQ/envelope §5.12]). It is the
review-loop termination of [AR §3.5]: the confirmed findings of severity ≥ important.

```lq-define
DEFINE QUERY loop($plan: node, $round: int? = NULL) SHAPE loop BUDGET light AS {
  MATCH (f:finding)
  WHERE EXISTS { (f)-[:ABOUT]->(s) WHERE s IN subtree($plan) }
    AND ($round IS NULL OR f.round = $round)
  RETURN f.round AS round,
         count(*) AS raised,
         count(CASE WHEN f.status = 'confirmed' THEN 1 END) AS confirmed,
         count(CASE WHEN f.status = 'refuted' THEN 1 END) AS refuted,
         count(CASE WHEN f.status = 'confirmed' AND f.severity IN ['blocker', 'important'] THEN 1 END) AS blocking
  ORDER BY round
}
```

4.18. **`refuted_share`** ([50 §4.1], verbatim but for `BUDGET`).

```lq-define
DEFINE QUERY refuted_share($role: text, $round: int? = NULL) SHAPE table BUDGET light AS {
  MATCH (f:finding) WHERE f.created_role = $role AND ($round IS NULL OR f.round = $round)
  WITH f.round AS round, count(*) AS raised, count(CASE WHEN f.status = 'refuted' THEN 1 END) AS refuted
  RETURN round, raised, refuted, round(100.0 * refuted / raised, 1) AS pct ORDER BY round
}
```

4.19. **`lane_conflicts`** — "overlapping `files_owned` globs of two lanes' tasks" ([50 §4.1]). A lane's tasks are the tasks
whose live lease is on the lane's branch (the lease rows capture `files_owned`, [AR §6.2], [AR §7.4] C1). LQ v1 has no glob-overlap
built-in for two globs, so overlap is approximated by either glob matching the other as text (open point 6).

```lq-define
DEFINE QUERY lane_conflicts($a: node, $b: node) SHAPE table BUDGET medium AS {
  MATCH (la:lane {id: $a}), (lb:lane {id: $b}), (ta:task), (tb:task)
  WHERE ta.claimed AND tb.claimed AND ta <> tb
    AND ta.lease.branch = la.moirai_branch AND tb.lease.branch = lb.moirai_branch
  UNWIND ta.files_owned AS ga
  UNWIND tb.files_owned AS gb
  WITH ta, tb, ga, gb WHERE glob_match(ga, gb) OR glob_match(gb, ga)
  RETURN ga AS path, ta, gb AS other, tb ORDER BY path, ta, other, tb
}
```

4.20. **`delta`** ([50 §4.1], verbatim but for `BUDGET`; the class C8 of the pack and the `UserPromptSubmit` delta).

```lq-define
DEFINE QUERY delta($since: rev, $agent: text) SHAPE changes BUDGET light AS {
  CALL changes(since: $since) YIELD seq, ref, node, op, aspect, name, actor
  WHERE relevant_to(node, $agent) AND actor <> $agent
  RETURN seq, ref, node, op, aspect, name, actor ORDER BY seq LIMIT 12
}
```

4.21. **`links_broken`** ([50 §4.1], [40 §6.5]; `moirai links check --scope 88` renders it and the two are tested for equality).

```lq-define
DEFINE QUERY links_broken($scope: node? = NULL) SHAPE links BUDGET medium AS {
  MATCH (n)-[a:AT]->(f)
  WHERE ($scope IS NULL OR n IN subtree($scope)) AND link_state(a) <> 'ok'
  RETURN f, a, link_state(a)
}
```

When the verb `moirai links check` runs this query, its `fs` limit is the orchestrator's ceiling `query.caps.orchestrator.fs`
whatever the caller's role (`--budget fs=N` lowers it), and `--budget-ms N` sets that run's wall-clock deadline to N ms, a
safety net (E503, the unreached links `unverified`, exit 10); `fs` units stay the primary, deterministic cut ([50 §4.1],
[50 §5.10] as amended for the A1 re-review's A-m5 and A1P-15; open point 7). Run by the verb with no eligible tree, the result is the header `files: no tree bound` and every link `unverified (no tree)`
([F18]), not E302; `moirai q links_broken` raises E302 (§2.8 item 4).

4.22. **`links_pending`**, **`links_proposals`**, **`links_guesses`**, **`files_removed`**, **`files_replaced`** — [40]'s former
`find` presets ([50 §4.1]). A file-level preset scopes by the files a node in the scope links to.

```lq-define
DEFINE QUERY links_pending($scope: node? = NULL) SHAPE links BUDGET medium AS {
  MATCH (n)-[a:AT]->(f)
  WHERE ($scope IS NULL OR n IN subtree($scope)) AND link_state(a) = 'pending'
  RETURN f, a, link_state(a)
}
```

```lq-define
DEFINE QUERY links_proposals($scope: node? = NULL) SHAPE links BUDGET medium AS {
  MATCH (n)-[a:AT]->(f)
  WHERE ($scope IS NULL OR n IN subtree($scope))
    AND link_state(a) IN ['moved-needs-confirm', 'ambiguous']
  RETURN f, a, link_state(a)
}
```

```lq-define
DEFINE QUERY links_guesses($scope: node? = NULL) SHAPE links BUDGET medium AS {
  MATCH (f:artifact)
  WHERE (f.relink STARTS WITH 'agent/' OR f.relink STARTS WITH 'policy/')
    AND ($scope IS NULL OR EXISTS { (n)-[:AT]->(f) WHERE n IN subtree($scope) })
  RETURN f ORDER BY f.id
}
```

```lq-define
DEFINE QUERY files_removed($scope: node? = NULL) SHAPE links BUDGET medium AS {
  MATCH (f:artifact)
  WHERE f.status = 'removed'
    AND ($scope IS NULL OR EXISTS { (n)-[:AT]->(f) WHERE n IN subtree($scope) })
  RETURN f ORDER BY f.id
}
```

```lq-define
DEFINE QUERY files_replaced($scope: node? = NULL) SHAPE links BUDGET medium AS {
  MATCH (f:artifact)
  WHERE link_state(f) = 'replaced'
    AND ($scope IS NULL OR EXISTS { (n)-[:AT]->(f) WHERE n IN subtree($scope) })
  RETURN f ORDER BY f.id
}
```

`links_guesses` depends on R-17's closed `relink` vocabulary: it lists every unconfirmed guess, the values whose `how` is `agent`
(an agent's acceptance) or `policy` (an automatic strong re-bind under policy B), as [50 §4.1] states after the A1 re-review's
A-M1 ([F18], [40 §2.2]).

4.23. **`root_moves`** ([50 §2.6], [40 §6.5]).

```lq-define
DEFINE QUERY root_moves($root: text = 'project') SHAPE table BUDGET light AS {
  CALL root_moves($root) YIELD hlc, class, from, to, git
  RETURN hlc, class, from, to, git ORDER BY hlc, from, to
}
```

## 5. Pack classes (C1–C8)

5.1. **Model.** Each class is a named query whose rows are its candidates ([50 §4.3], [AR §7.4] step 2). A class returns the
candidate in its first column and, where the pack algorithm needs it, a reason column; set operations combine the sub-classes
[AR §7.4] lists. The pack algorithm applies the per-class quotas, the relevance floors, the levels and the "each node renders once"
rule ([AR §7.4] step 3); none of that is in the text below. A `UNION` result is ordered by its columns left to right
([50 §3.5]); the algorithm re-orders.

| Class ([AR §7.4]) | Named query | Parameters |
|---|---|---|
| C1 header | `pack_header` | `$target: node` |
| C2 rules | `pack_rules` ∪ `pack_rules_unmerged` | `$role: text, $phase: text?`; `$role: text` |
| C3 target | `pack_target` | `$target: node` |
| C4 effective spec | `pack_spec` | `$target: node, $role: text, $round: int?` |
| C5 findings | `pack_findings` | `$target: node, $role: text, $round: int?` |
| C6 measurements | `pack_measurements` | `$lane: node` |
| C7 hazards | `pack_hazards` | `$target: node` |
| C8 delta | `delta` (§4.20) | `$since: rev, $agent: text` |

5.2. **C1 `pack_header`** — the branch's ref row (ahead, behind, staged), the staging refs that involve it, and [40 §6.2]'s
link-state counts. The worktree, the other lanes' `files_owned` from the lease rows, the quiet state, the dirty row and the global
critical-rule count are runtime and tree facts the algorithm reads directly ([AR §7.4] C1); they are not candidates.

```lq-define
DEFINE QUERY pack_header($target: node) SHAPE table BUDGET light AS {
  CALL refs() YIELD name, kind, tip, seq, ahead, behind, fork, staged
  WHERE name = view_ref() OR (kind = 'merge' AND name CONTAINS view_ref())
  RETURN 'ref' AS item, name AS key, ahead, behind, staged, NULL AS n
  UNION ALL
  CALL links(scope: $target) YIELD state
  RETURN 'links' AS item, state AS key, NULL AS ahead, NULL AS behind, NULL AS staged, count(*) AS n
}
```

5.3. **C2 `pack_rules`** — rules where `applies_to` meets `{R, P, *}` and that are authoritative (`active`) on the branch; order
criticality, authority, id ([AR §7.4] C2). The lane component of `{R, P, lane, *}` has no built-in in LQ v1 (open point 8).

```lq-define
DEFINE QUERY pack_rules($role: text, $phase: text? = NULL) SHAPE node BUDGET light AS {
  MATCH (r:rule)
  WHERE r.status = 'active'
    AND (applies_role(r, $role) OR ($phase IS NOT NULL AND applies_phase(r, $phase)))
  RETURN r ORDER BY r.criticality, r.authority, r.id
}
```

5.4. **C2 `pack_rules_unmerged`** — the `~main` class ([50 §4.3], verbatim but for `BUDGET`): critical rules on `main` the
caller's branch has not merged, including a rule changed on `main`.

```lq-define
DEFINE QUERY pack_rules_unmerged($role: text) SHAPE node BUDGET light AS {
  USE main
  CALL diff(HEAD...main) YIELD node, side
  WHERE side IN ['theirs', 'both']
  MATCH (r:rule)
  WHERE r = node AND r.status = 'active' AND r.criticality = 'critical' AND applies_role(r, $role)
  RETURN DISTINCT r ORDER BY r.criticality, r.authority, r.id
}
```

5.5. **C3 `pack_target`** — the target, its ancestors, the open questions that block it, owner rulings about its subtree, and
the files it links ([50 §4.3], [AR §7.4] C3). "Owner rulings" are the nodes with `authority = 'owner'` that are `ABOUT` a node of
the subtree (open point 9).

```lq-define
DEFINE QUERY pack_target($target: node) SHAPE node BUDGET medium AS {
  MATCH (t) WHERE t = $target
  RETURN t AS node, 'target' AS why
  UNION
  MATCH (a) WHERE a IN ancestors($target)
  RETURN a AS node, 'ancestor' AS why
  UNION
  MATCH (q:question)-[:BLOCKS]->(t) WHERE t = $target AND q.unfinished
  RETURN q AS node, 'question' AS why
  UNION
  MATCH (k)-[:ABOUT]->(x) WHERE x IN subtree($target) AND k.authority = 'owner'
  RETURN k AS node, 'ruling' AS why
  UNION
  MATCH (t)-[:AT]->(f:artifact) WHERE t = $target
  RETURN f AS node, 'link' AS why
}
```

5.6. **C4 `pack_spec`** — sections reachable through `IMPLEMENTS` or `ABOUT` from the target (or the target doc's own sections),
with `changed` true for sections changed after round `$round` and for their `DEPENDS_ON` dependents ([50 §4.3], [AR §7.4] C4).
`$role` is a rendering parameter: the role decides levels, not membership.

```lq-define
DEFINE QUERY pack_spec($target: node, $role: text, $round: int? = NULL) SHAPE node BUDGET medium AS {
  MATCH (d:doc)
  WHERE d = $target OR EXISTS { MATCH (t)-[:IMPLEMENTS|ABOUT]->(d) WHERE t = $target }
  MATCH (s:doc) WHERE s IN subtree(d)
  RETURN DISTINCT s AS node, $round IS NOT NULL AND coalesce(s.changed_in_round, 0) > $round AS changed
  UNION
  MATCH (d:doc)
  WHERE d = $target OR EXISTS { MATCH (t)-[:IMPLEMENTS|ABOUT]->(d) WHERE t = $target }
  MATCH (s:doc) WHERE s IN subtree(d) AND $round IS NOT NULL AND coalesce(s.changed_in_round, 0) > $round
  MATCH (dep:doc)-[:DEPENDS_ON]->(s)
  RETURN DISTINCT dep AS node, true AS changed
}
```

5.7. **C5 `pack_findings`** — findings about the target's subtree: confirmed ones for developers, the critic's own previous ones
for critics, open ones for reviewers ([50 §4.3], [AR §7.4] C5; open point 10).

```lq-define
DEFINE QUERY pack_findings($target: node, $role: text, $round: int? = NULL) SHAPE node BUDGET light AS {
  MATCH (f:finding)
  WHERE EXISTS { MATCH (f)-[:ABOUT]->(x) WHERE x IN subtree($target) }
    AND CASE $role
          WHEN 'developer' THEN f.status = 'confirmed'
          WHEN 'architecture-critic' THEN f.created_role = $role
                                          AND ($round IS NULL OR coalesce(f.round, 0) <= $round)
          WHEN 'code-reviewer' THEN f.created_role = $role OR f.status = 'open'
          ELSE f.status IN ['open', 'confirmed']
        END
  RETURN f ORDER BY f.severity, f.id
}
```

5.8. **C6 `pack_measurements`** — current measurements visible on the branch, except those produced by a run of another lane;
`staleness()` is read from the cache only on the pack path ([AR §7.4] C6, [70 S6]). The "known reds" classification is the
algorithm's (open point 11).

```lq-define
DEFINE QUERY pack_measurements($lane: node) SHAPE node BUDGET light AS {
  MATCH (m:measurement)
  WHERE m.status = 'current'
    AND NOT EXISTS { MATCH (r:run)-[:PRODUCED]->(m), (r)-[:RUNS_IN]->(l:lane) WHERE l <> $lane }
  RETURN m, staleness(m) AS staleness ORDER BY m.id
}
```

5.9. **C7 `pack_hazards`** — active notes and rules scoped by path to the target's `files_owned` (a `GLOBIDX` range probe by
literal prefix, [70 S17]), and those anchored in files under those globs (the reverse index file → anchors → referrers,
[40 §6.2]) ([AR §7.4] C7). An empty `applies_to` means `*` and belongs to C2, so the first part requires a non-empty one.

```lq-define
DEFINE QUERY pack_hazards($target: node) SHAPE node BUDGET medium AS {
  MATCH (t:task) WHERE t = $target
  UNWIND t.files_owned AS g
  MATCH (k:note|rule)
  WHERE k.status = 'active' AND coalesce(size(k.applies_to), 0) > 0 AND applies(k, g)
  RETURN DISTINCT k AS node
  UNION
  MATCH (t:task) WHERE t = $target
  UNWIND t.files_owned AS g
  MATCH (k:note|rule)-[:AT]->(f:artifact)
  WHERE k.status = 'active' AND glob_match(f.path, g)
  RETURN DISTINCT k AS node
}
```

C8 is `std.delta` (§4.20).

## 6. Brief classes

6.1. `brief` is the pack machine with fixed classes ([AR §7.4]); its classes are `brief_lanes`, `brief_triage`,
`brief_questions`, `brief_critical` and `brief_verdicts` ([50 §4.3]). The brief's other lines (the checkpoint note of each open
campaign, stale summaries, at most three non-`ok` link lines) come from `stale`, `links_broken` and the checkpoint notes, and are
listed in open point 12.

```lq-define
DEFINE QUERY brief_lanes() SHAPE node BUDGET light AS {
  MATCH (l:lane) WHERE l.status IN ['active', 'ready_to_merge', 'merge_pending', 'measuring', 'frozen']
  RETURN l AS node, 'lane' AS why
  UNION
  MATCH (r:run) WHERE r.status = 'running'
  RETURN r AS node, 'run' AS why
  UNION
  MATCH (a:lane)-[:MERGE_AFTER]->(b:lane) WHERE a.status = 'merge_pending'
  RETURN a AS node, 'merge queue' AS why
}
```

`brief_triage` is [50 §4.3]'s text, anchored on `RuntimeScan(markers) ∪ BitmapScan(has_dangling)` (≤ 50 µs, [AR §8.3]).

```lq-define
DEFINE QUERY brief_triage() SHAPE node BUDGET light AS {
  MATCH (t:task) WHERE t.settled_elsewhere OR t.deleted_elsewhere OR t.has_dangling RETURN t
}
```

```lq-define
DEFINE QUERY brief_questions() SHAPE node BUDGET light AS {
  MATCH (q:question) WHERE q.status = 'open' AND q.asked_of = 'owner'
  RETURN q ORDER BY q.criticality, q.id
}
```

```lq-define
DEFINE QUERY brief_critical() SHAPE node BUDGET light AS {
  MATCH (k:rule|note) WHERE k.status = 'active' AND k.criticality = 'critical'
  RETURN k ORDER BY k.authority, k.id
}
```

```lq-define
DEFINE QUERY brief_verdicts($since: rev) SHAPE node BUDGET light AS {
  MATCH (v:verdict) WHERE v.created > $since
  RETURN v ORDER BY v.created, v.id
}
```

## 7. Named mutations (`tx.*`)

7.1. **Expansion templates.** Write verbs expand to `TX` blocks with fixed guards; `--show-tx` prints the expansion
([50 §4.2]). In the templates below, `<id>` is replaced by the verb's id argument as a node literal (`#12`), `<kind>` and `<T>` by
a kind or an edge's LQ name, `[...]` marks a part present only when its argument is given, and `...` a part repeated per list
element. Every free-text value travels as a `$parameter`, never spliced into text ([50 §2.2] rule 7). The canonical bound AST of
the expansion, with the parameter values, is the idempotency payload ([50 §3.10] item 8). The verb's `--idempotency-key`,
`--lease`, `--branch`, `--if-tip` and `--dry-run` become `KEY`, `LEASE`, `ON`, `IF TIP` and `DRY`.

7.2. **Verb mutations.**

| Named mutation | Verb ([AR §7.1]) | Parameters | Expansion |
|---|---|---|---|
| `tx.add` | `add KIND 'title' [--parent ID] [--blocked-by ID,..] [--blocks ID,..] [--field k=v]..` | `$kind: text, $title: text, $parent: node?, $blocked_by: list<node>?, $blocks: list<node>?, $fields: list<text>?, $body: text?` | `TX { CREATE (n:<kind> {title: $title[, <f>: $<f>]...})[ UNDER <parent>]; [CREATE (<b>)-[:BLOCKS]->(n); ...][CREATE (n)-[:BLOCKS]->(<b>); ...][SET n.body = $body] }` |
| `tx.set` | `set ID [k=v].. [--status S] [--done] [--resolution R] [--if-rev N] [--if-status S] [--if-holder H]` | `$id: node, $fields: list<text>?, $status: text?, $done: bool?, $resolution: text?, $if_rev: int?, $if_status: text?, $if_holder: text?` | without guards: `TX { SET <id>.<f> = $<f>[, <id>.<f> = $<f>]... }`; with guards: `TX { MATCH (n {id: <id>}) WHERE <guards> EXPECT 1 SET n.<f> = $<f>[, ...] }`, guards `n.rev = $if_rev`, `n.status = $if_status`, `n.lease.holder = $if_holder` joined by `AND`; `--status S` is `SET ... .status = $status`, `--done` is `.done = true`, `--resolution R` is `.resolution = $resolution` |
| `tx.link` | `link A --<kind> B` | `$a: node, $kind: text, $b: node, $pinned: text?` | `TX { CREATE (<a>)-[:<T>[ {pinned: $pinned}]]->(<b>) }`; `--parent` is `TX { MOVE <a> UNDER <b> }`; `--cites@COMMIT` sets `$pinned` |
| `tx.unlink` | `unlink A --<kind> B` | `$a: node, $kind: text, $b: node` | `TX { MATCH (<a>)-[e:<T>]->(<b>) EXPECT 1 DELETE e }` |
| `tx.move` | `move ID --parent P` | `$id: node, $parent: node` | `TX { MOVE <id> UNDER <parent> }` |
| `tx.reopen` | `reopen ID --reason T` | `$id: node, $reason: text` | `TX { REOPEN <id> REASON $reason }` |
| `tx.supersede` | `supersede OLD --with NEW` | `$old: node, $new: node` | `TX { CREATE (<new>)-[:SUPERSEDES]->(<old>) }` |
| `tx.doc_patch` | `doc patch SECTION --remove FILE --add FILE [--depends-on S,..]` | `$section: node, $old: text, $new: text, $depends_on: list<node>?` | `TX { PATCH <section>.body REMOVE $old ADD $new[; CREATE (<section>)-[:DEPENDS_ON]->(<s>)]... }` |
| `tx.rm` | `rm ID [--reason T] [--replaced-by ID] [--cascade\|--reparent] [--release] [--dry-run] [--yes]` | `$id: node, $reason: text?, $replaced_by: node?, $policy: text?, $release: bool = false` | `TX { DELETE <id>[ POLICY CASCADE\|REPARENT][ REPLACED BY <replaced_by>][ RELEASE][ REASON $reason] }`; without `--yes` the verb runs it with `DRY` |
| `tx.resolve` | `resolve KEY --take ours\|theirs\|base\|repoint:ID \| --value V \| --all --policy P` | `$key: text?, $take: text, $value: text?, $all: bool = false` | `TX { RESOLVE '<key>' TAKE OURS\|THEIRS\|BASE\|VALUE $value\|REPOINT <id> }`; `--all --policy P`: `TX { RESOLVE (CALL conflicts() YIELD key RETURN key) EXPECT >= 1 TAKE <P> }` with `<P>` = `OURS`, `THEIRS` or `BASE` |
| `tx.remember` | MCP `remember`; the CLI verbs `rule\|note\|decision\|finding\|verdict\|measurement --stdin` | `$kind: text, $title: text, $text: text, $fields: list<text>?, $about: list<node>?, $applies_to: list<text>?` | `TX { CREATE (n:<kind> {title: $title[, <f>: $<f>]...}); SET n.<textfield> = $text[; CREATE (n)-[:ABOUT]->(<x>)]... }` where `<textfield>` is `text` for a rule and `body` otherwise; the kind's mandatory fields are checked (`failure_scenario` for findings) and a verdict's `DERIVED_FROM` edges are written ([50 §4.2]) |

`tx.resolve`'s key is a string literal in the grammar (`RESOLVE string`, [50 §2.3]), so the expansion inserts the key, after it
passed the conflict-key syntax check of [F12], as a single-quoted literal with `'` and `\` escaped; it is the one value a template
inserts as text, and the syntax check leaves it nothing but key characters. The CLI `--take repoint:ID` spells the node as a bare
id. Instances, which parse:

```lq-tx
TX { CREATE (n:task {title: $title}) UNDER #88; CREATE (#12)-[:BLOCKS]->(n) }
```

```lq-tx
TX { MATCH (n {id: #12}) WHERE n.rev = $if_rev AND n.status = $if_status EXPECT 1 SET n.priority = $priority }
```

```lq-tx
TX { MATCH (#12)-[e:BLOCKS]->(#51) EXPECT 1 DELETE e }
```

```lq-tx
TX { PATCH #133.body REMOVE $old ADD $new; CREATE (#133)-[:DEPENDS_ON]->(#131) }
```

```lq-tx
TX ON lane/l10 KEY 'orch:rm-40' { DELETE #40 REPLACED BY #52 RELEASE REASON $reason } DRY
```

```lq-tx
TX { RESOLVE (CALL conflicts() YIELD key RETURN key) EXPECT >= 1 TAKE THEIRS }
```

```lq-tx
TX { CREATE (n:finding {title: $title, failure_scenario: $failure_scenario, severity: $severity, f_kind: $f_kind}); SET n.body = $text; CREATE (n)-[:ABOUT]->(#130) }
```

7.3. **Procedures.** These are callable as `CALL tx.<name>(...)` inside a `TX` block but are not expressible with `SET`
([50 §4.2]); their effect is the engine's.

| Procedure | Parameters | Yields | Effect |
|---|---|---|---|
| `tx.complete` | `$id: node, $outcome: text, $summary: text, $evidence: list<text>? = NULL, $digest: text? = NULL` | `task, status, ready` | for every outcome (`done`, `failed`, `abandoned`): match the leased task (`EXPECT 1`), perform `open → in_progress → done` in one commit with `resolution` `completed`, `rework` or `wontdo` for `done`, `failed` or `abandoned`, release the lease into a `settled` marker whose outcome records `$outcome`, and yield the newly ready ids ([AR §6.2], [50 §4.2], [RULES/status-machines] CO-002, CO-003, [API §10.5]; pass 1, A1-36); refused with E404 while a child is open or a `fail_*` verdict gates the task; the link settle is a separate CAS-guarded commit after it ([72 M11]); `$digest` is the pack digest ([AR §7.4] step 4) |
| `tx.claim` | `$ids: list<node>? = NULL, $next: bool = false, $scope: node? = NULL, $role: text? = NULL, $agent: text? = NULL, $ttl: text? = NULL, $start: bool = false, $run: text? = NULL, $session: bool = false` | `lease, token, branch, expires` | a `Lease` record only; `$start` also performs `open → in_progress`; `$ttl` is a duration or `run`, and `NULL` means the store's `lease.ttl-default` ([CFG]; pass 1, A1-37), so the named mutation and the verb honour the key alike; `$run` and `$session` mint role leases under [AR §7.3]'s minting policy ([AR §6.2], [90 §4.3]) |
| `tx.heartbeat` | `$lease: text` | `lease, expires` | a lazy renewal record |
| `tx.release` | `$lease: text` | `lease` | releases the lease |
| `tx.reclaim` | `$older_than: duration? = NULL, $run: text? = NULL` | `lease` | releases the matching leases ([AR §6.2]) |

```lq-tx
TX LEASE 'L-18' { CALL tx.complete(#89, outcome: 'done', summary: $summary) YIELD ready }
```

7.4. **File named mutations** ([40 §6.3], [90 §6.6], [AR §7.2]). They read or move project files (capture, resolution, the
`FsIntent` protocol), so they are not `TX` statements: they run alone, as the `write` tool's `name` + `params[]` or as their CLI
verb, and a `CALL` of one inside a `TX` block is E115 naming this rule ([LQ/errors §5.3]). Their names are [40 §6.3]'s JSON op
names, so the MCP name, the `apply` op and `--show-tx` use one vocabulary.

| Named mutation | Verb | Parameters |
|---|---|---|
| `tx.link_file` | `link ID --at SPEC [--watch header\|span] [--planned] [--quote-file FILE]` | `$node: node, $spec: text, $watch: text? = NULL, $planned: bool = false, $quote: text? = NULL, $end: text? = NULL` |
| `tx.unlink_file` | `unlink ID --at aN\|PATH` | `$node: node, $anchor: text? = NULL, $path: text? = NULL` (exactly one of the two) |
| `tx.record_move` | `file relink PATH\|ID --to PATH --after` | `$from: text, $to: text` |
| `tx.links_fix` | `links fix ID\|aN --accept --expect PATH \| --to PATH \| --confirm \| ...` | `$target: text, $action: text, $expect: text? = NULL, $to: text? = NULL, $at: text? = NULL, $same_as: node? = NULL` (`accept` requires `expect`; `confirm` must come from another actor, [40 §6.3]) |
| `tx.links_sync` | `links sync [--scope ID] [--budget-ms N]` | `$scope: node? = NULL, $budget_ms: int? = NULL` |

`$quote` and `$end` are added to `tx.link_file` so roles without a shell can create the quoted-text anchor forms the skill
teaches through `--quote-file` (the A1 review's A-m6); the U+FFFD refusal of [40 §2.7] applies to them.

7.5. **Verbs outside this table.** `retract`, `answer`, `apply`, and the VCS, image, store and integration verbs of [AR §7.1]
are not named mutations of this chapter: `retract` and `answer` expand like `tx.set` on the status and text fields their kinds
define ([F08]); `apply` batches are the JSON form of the same IR ([50 §4.2]); the VCS and maintenance verbs act on refs, not on
the graph ([50 §1.3]). Open point 13.

## 8. Verb aliases

A read verb is its named query with its flags as parameters ([50 §4.1], [AR §7.7.2]); `--show-query` prints the definition with
the bound values ([LQ/envelope §10.3]). The flags every read verb takes (`--at`, `--tree`, `--budget`, `--json`, `--ids`,
`--cursor`, `--branch`) are the envelope's, not parameters. The alias column of §3 is normative; where a verb flag and a parameter
differ in spelling, the flag's `-` becomes `_` (`--for-agent A` → `for_agent=A`). `conflicts [REF]` takes REF as the view
(`--branch REF`). `show ID.. --across` runs `across` with `refs` = the caller's branch and `main`.

## 9. Tests this chapter implies

Each named query parses (`lq-define` blocks, grammar v1), binds against the core schema, classifies as its cursor column says
(§2.5), and for every verb alias the verb's output equals the named query's for every argv form ([50 §8.3]). Each pack and brief
class returns, on the model, exactly the members the owner-signed table lists (V3).

## Coverage

| Item | Where (and who covers the rest) |
|---|---|
| [50] F3: the vocabulary of a named-query item (parameter types, the shape set, budget classes, grammar version 1) for the standard library | §2.2–§2.6; the `QUERIES` item bytes: [F08]; the `.moi` query file: [F14] |
| [50] F18: the standard library has no `QueryCycle` (no `std` text calls another `std` query) | §2.1, §4; the validator: [F19], [F12] |
| [60 §2.5] "Derived-state semantics" row as the library uses it (`ready` tip-only and live, `unblocked` at any view) | §4.1, §4.2, §6.1; the semantics: [F13], [F16] |
| [40] R-16 and R-17 as the presets use them (`link_state` strings with `none` and `unresolved`; `relink` `agent/*` and `policy/*`) | §2.8, §4.21–§4.22; the strings and the vocabulary: [F18] |

## Holes

None. The library's text is frozen at WP-72; it holds no measured value. The budget-class limits are [50 §4.1]'s.

## Open points for the review

1. **`CALL` name resolution.** A built-in relation is found before `std` inside `CALL`, while `moirai q NAME` finds `std` first.
   Without this, nine `std` queries would call themselves. The alternative, renaming the std queries (`std.blockers_list`), breaks
   the verb = named-query identity ([50 §1.4]). [LQ/canonical-ast] open point C-6 takes the same rule.
2. **`BUDGET` in the texts.** [50 §4.1] shows the definitions without a `BUDGET` clause and gives the class in the catalog table;
   this chapter writes the class into each definition, so `--show-query` prints it and F3 stores it as data.
3. **Additions to [50 §4.1]'s signatures:** `find`'s `$done` ([AR §2.11]'s `done:false`), `changes`'s `$all` ([AR §7.1]'s
   `--all`), and `find`'s positional `text`. [AR §2.11]'s `area:net` names an area by title; `$area` is a `node` as [50 §4.1]
   types it, so `area:net` is E110 until the review decides whether areas are addressable by name.
4. **`stale`'s scope.** [50 §4.1] gives only the core predicate. Measurements and notes are rarely in a task subtree, so the
   scope follows `ABOUT` and `SCOPED_TO` into it.
5. **`blame`** prints a table ordered by aspect and name, as Q14 shows, not [50 §4.1]'s "history · rev desc" row, which it shares
   with `history`.
6. **`lane_conflicts` overlap.** LQ v1 has `glob_match(text, glob)` and `applies()` (overlap against `applies_to` only), but no
   overlap test of two globs; "either matches the other as text" misses crossing wildcards (`a/*/x` against `a/y/*`). A built-in
   `glob_overlap(g, h)` would be an addition to the relation and built-in registry before the freeze.
7. **The `links check` budget (the A1 re-review's A-m5, shared with WP-18).** §4.21 follows [50]'s amended text: the verb runs at
   the orchestrator's `fs` ceiling and `--budget-ms` is a deadline, keeping `fs` as the deterministic cut (A1P-15). The
   deadline's error is E503; [F19]/[CFG] own `files.read-budget-ms` as the safety net of the other read paths.
8. **C2's lane scoping.** [AR §7.4] C2 intersects `applies_to` with `{R, P, lane, *}`; LQ v1 has `applies_role` and
   `applies_phase` but no `applies_lane`. Until one exists, lane-scoped rules reach the pack only through `*`.
9. **"Owner rulings ABOUT the subtree"** is read as `authority = 'owner'` on an `ABOUT` source (findings, verdicts, measurements,
   questions). Owner rules are `rule{authority = owner}` and reach the pack through C2.
10. **C5's role rules** are approximations of [AR §7.4] C5: "reviewer → open findings on the same files" is taken as open findings
    about the subtree or the reviewer's own, because file-level matching needs the target's `files_owned` against the findings'
    anchors, which C7 already covers.
11. **C6.** "Measurements for the lane and `main`" is read as "current measurements not produced by a run of another lane"; the
    red classification needs a target direction [AR §3.2] does not store, so it stays in the algorithm.
12. **Brief lines outside the five classes.** The checkpoint note per open campaign, stale summaries and the three link lines are
    rendered from `stale`, `links_broken` and checkpoint notes; the review may prefer named classes for them.
13. **Write verbs without an expansion here** (`retract`, `answer`, the knowledge verbs' per-kind text fields): their fields are
    [F08]'s. The remember mapping of MCP `text` to `text` for rules and to `body` otherwise is this chapter's choice.
14. **The file named mutations' names** follow [40 §6.3]'s JSON op names; [40] and [AR §7.2] name the verbs but not the mutations.
15. **Total link states (§2.8 item 3)** are [50 §2.6]'s rule since the A1 dispositions (S-05). `std.links_broken` and the
    presets compare an `AT` edge variable's or an artifact's state, which is never `none`, so none of them draws W10.
16. **Reconciled with [LQ/grammar-v1.ebnf], [LQ/lexical] and [LQ/canonical-ast].** Every `define_stmt` here is a parse unit of
    §P.1; the revision positions of §2.8 item 1 are [LQ/lexical §4.2]'s, including `violations(ref:)` and `changes(ref:)`, which
    O-4 and O-5 asked this chapter to confirm; named-query names after the namespace match exactly and shape, budget and type
    names case-insensitively ([LQ/lexical §9]); §2.9 names every relation parameter as C-5 needs. The relation yields for
    `schema_edges()` and `queries()` are this chapter's choice from F1 and F3's fields; `leases()` and `markers()` yield their
    tables' row fields.
17. **Pass 1 changes** (A1-36, A1-37, A1-38, A1-54, A1-58). `tx.complete` writes `done` for every outcome, as [AR §6.2], the
    rule table and [API §10.5] do (§7.3); `tx.claim`'s `$ttl` defaults to `NULL`, meaning `lease.ttl-default`; the `BUDGET`
    classes derive from `query.budget.default.work` (§2.4); `std.ready` orders by `priority, id` (§4.1; [50 §4.1] is corrected
    at WP-81a); §2.10 gives the scalar built-ins' signatures, with `subtree(n [, depth])` unbounded by default; §1's paragraphs
    are in order.
18. **Spec sync 2a** (WP-93a review and author). §2.11 fixes the kinds, types and classes of the built-in node properties:
    the header-backed derived properties exist on every kind and are never absent ([50 §2.5]: header columns are never
    absent), while `done`, `unfinished`, `answered` and `state` exist only on the kinds that define them; `lease` is absent
    without a live lease. §2.12 types every yielded column (`seq` is int, commits are rev, a key's value is `any`). §2.13 has
    the evaluator scale `coerce = timestamp` fields between Unix seconds and LQ milliseconds, so no C-AST or hash sees the
    scale ([F08] open point 11). §2.14 lists the edge kinds that admit a `DELETED` endpoint, from the delete-policy rows.
    OQ-F-2 was decided on 2026-09-28: `std.ready` orders by `priority, id` (§4.1).
