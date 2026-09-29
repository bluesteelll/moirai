# LQ result envelope, shapes, reading echo and footers

| | |
|---|---|
| Title | The frozen v1 result envelope as LQ prints it: header, reading echo, row shapes, footers, the JSON envelope, cursors and `TX` results |
| Chapter | [LQ/envelope], `docs/spec/lq/envelope.md` |
| Status | draft, pass 1 pending |
| Work package | WP-19b (R-SPEC-F), part of WP-19 of [PLAN §3.2] item 1 |
| Sources | [50 §6.4] (the envelope, shapes, node lines, untrusted text, JSON, footers), [50 §2.4], [50 §2.5] (edge readings, reverse aliases), [50 §2.8] (display spelling), [50 §2.9] (every example output), [50 §3.5] (total order, pages, pinned and live cursors, byte identity), [50 §3.6], [50 §3.9], [50 §3.10] items 8–9, [50 §4.4] (the `DEFINE` result), [50 §5.3] (canonical hash), [50 §5.10] (budgets), [50 §5.11], [50 §6.1], [50 §6.3], [50 §7.4] items 2 and 4; [AR §5a.1] (display ids), [AR §7.1] (the output contract), [AR §7.2], [AR §8.3] TOKENS rows; [90 §2.1], [90 §2.2], [90 §6.1]–[90 §6.3], [90 §6.7], [90 §8.1] L1, L3, L5, L8, [90 §10.1] row "Output contract"; [80 §4] T7; [40 §2.9], [40 §6.1]–[40 §6.2]; research [16 §6.4] (cursor contents) |
| Depends on | [F19] (exit codes, byte unit, both-ends, ASCII, `--ids` page, header limits), [F18] (R-16 strings), [F06] (F10 origins), [F08] (F1 readings, field order), [F12] (conflict keys), [LQ/canonical-ast] (query hash), [LQ/errors], [LQ/std], [LQ/gql-spelling] |

## 1. Scope and precedence

1.1. This chapter fixes every byte an LQ read or write prints: the text header, the reading echo, the row shapes, the footers,
the JSON envelope, the cursor and the `TX` result texts. Every read verb is a named query and every write verb a named mutation
([AR §7.7.2]), so this is the envelope of every verb of [AR §7.1] that runs through LQ. It freezes at WP-72 with the query surface; the reference
renderer (WP-71a) implements it, and its goldens bind M7/M8 ([PLAN §6.2] R13).

1.2. [F19] owns the verb-independent output contract: exit codes 0–10, the byte unit, the both-ends rule, the ASCII rule, the
`--ids` page rule and the header byte limits ([90 §10.1] row "Output contract"). Where this chapter restates one of them it cites
it; where the two disagree, [F19] wins for those five rules and this chapter wins for LQ's shapes, echo, cursors and `TX`
results. Diagnostic texts are [LQ/errors]'s. The link-state strings, their details and the header strings `files: no tree bound`
and `reading only: …` are R-16's ([F18]).

1.3. Terms. *Base fields* are the header fields `branch`, `rev` (or a write's `rev <old> -> <new>`), the view flag, `rows` and a
write's `committed <commit>` ([AR §7.1]). *Template bytes* are the bytes moirai's own texts contribute; *value bytes* come from
the store or the caller.

## 2. Bytes, channels and layout

2.1. **Encoding.** Output is UTF-8 with LF line ends, never ANSI escapes off a TTY, byte-identical across operating systems apart
from [80 §4] T7's three golden substitutions (`<ROOT>`, `<OSERR>`, `<OS-DETAIL>`). A broken pipe on stdout exits quietly with
0 (T7).

2.2. **ASCII.** Every template byte is printable ASCII or LF ([90 §8.1] L5; [90 §10.1]: `...` for truncation, `|` for
separators, `->` for arrows). Value bytes keep their UTF-8, with the escaping of §5.2. Budgets count UTF-8 bytes ([90 §6.2]).

2.3. **Channels.** Results go to stdout. Under `--ids` only ids go to stdout; the footer, warnings, notices and errors go to
stderr (§6.5, [LQ/errors §3.1]).

2.4. **Layout of a text result.** In this order, each line ended by LF:
1. the header line (§3), omitted under `--ids`;
2. for a result read from a reader tree, R-16's note `reading only: tree on <ref>, branch expects <ref>` ([F18]; ≤ 80 B,
   [AR §7.1]);
3. the reading-echo lines (§4), zero or more;
4. the body: the rows in the result's shape (§5), or the `TX` statement lines (§9);
5. the footer block (§6).

2.5. **MCP.** The `query` and `write` tools return the same text in `content[0]` (one text block, [90 §6.3]); `format: "json"`
returns the JSON envelope of §7 as text ([90 §6.7]); errors set `isError: true`. No `structuredContent` ([90 §2.2]).

## 3. The header line

3.1. **Grammar of a read's header** (ABNF, RFC 5234 notation; `SEP` is the three bytes ` | `; the header of a `TX`, of `DRY`,
of a replay, of EXPLAIN and of `--check` are §9 and §10):

```
header      = "branch: " ref SEP revpart [SEP viewflag] *(SEP descriptor) SEP rows
              [SEP dropped] [SEP more] [SEP files] *(SEP extra)
revpart     = "rev " seq / composite
viewflag    = "as-of (USE " revspec ")" / "staged (read-only)" / "live"
descriptor  = across / diff
rows        = count " row" / count " rows"          ; "row" iff count = 1
dropped     = "dropped " count
more        = "more: cursor " cursor                ; cursor: section 8.3
files       = "files @ " treelabel " (" gitbranch " " head7 [", dirty " count [" " age]] ")"
            / "files: no tree bound"                ; the reader note is line 2 (section 2.4)
extra       = search / recompute / schema / behind / moved
seq, count  = 1*DIGIT                              ; no leading zeros, no grouping
```

The remaining names (`ref`, `revspec`, `composite`, `across`, `diff`, `treelabel`, `gitbranch`, `head7`, `age`, `search`,
`recompute`, `schema`, `behind`, `moved`) are the fields of §3.2.

3.2. **Fields.**

| Field | Content |
|---|---|
| `ref` | the ref of the view the rows were read from ([50 §6.4]): the ref a `USE` names (after its suffixes); for a commit, sequence-number or reflog view, the ref recorded in the view commit's header; without `USE`, the caller's resolved branch ([90 §4.1]) |
| `revpart` | `rev ` and the sequence number of the view's commit (the tip for a tip view) |
| `composite` | for a query of several parts ([50 §3.9] item 2): the parts joined by ` UNION `, ` UNION ALL `, ` EXCEPT ` or ` INTERSECT `; each part is `[<revspec> = ]rev <seq>[ (as-of)]`, the `<revspec> = ` prefix present iff the part has a `USE` ([50 §2.9] Q15). A part prints no `<c8>`: JSON `parts` carries each part's commit (§7.1), and the extras part stays within 60 B ([F19 §4.2]; pass 1, A1-30) |
| `viewflag` | at most one: `as-of (USE <revspec>)` for a past view of a single-part query (the revspec as written, or `--at`'s value), `staged (read-only)` for a staging ref, `live` when the result reads runtime or tree-derived state ([50 §3.5]) |
| `across` | `across <ref> <c8>, <ref> <c8>...` for `across()` ([50 §2.9] Q12) |
| `diff` | `diff <range>` for `diff()`, plus ` (LCA rev <seq> <c8>)` when the base is an LCA ([50 §2.9] Q13, N05) |
| `rows` | the number of rows on this page |
| `dropped` | the number of rows rendered at L0 instead of L1 on this page (§5.18), when > 0 |
| `more` | the continuation cursor, when the result was paginated ([90 §6.3]) |
| `files` | for file-bearing results ([AR §7.1]): the tree label, the tree's git branch, the first 7 hex digits of its git `HEAD`, and its dirty row count with the row's age (`<n>s`, `<n>m`, `<n>h` or `<n>d` then ` ago`, the largest unit with a non-zero floor); `files: no tree bound` when no tree is bound |
| `treelabel`, `gitbranch` | the tree's display label: its path relative to the parent directory of the main worktree, or, for a tree outside it, its last two path components, cut from the left with `...` to ≤ 20 B; the git branch's name cut from the left with `...` to ≤ 16 B ([AR §7.1] header rules, the A1 re-review's A-M3). The canonical path goes only to JSON `tree` and `file where` |
| `search` | three fields: `search: <n> terms`, `titles+abstracts` or `titles+abstracts+bodies`, and `index` (tier 2) or `scan` (tier 1) ([50 §2.9] Q8) |
| `recompute` | `derived recomputed for <n> nodes (<m> ops reverse-applied)` or `(<m> ops replayed)` ([50 §2.9] Q11) |
| `schema` | `schema v<n>` on catalog results (`schema()`, `schema_edges()`, `queries()`) |
| `behind` | `behind main <n>` when the view is the tip of a work branch other than `main` and `main` has `<n>` > 0 commits it has not absorbed ([AR §7.1]) |
| `moved` | `view moved +<k> commits since page 1` on page 2 and later of a pinned cursor when the branch tip moved ([50 §3.5]) |

3.3. **Byte limits**, counted per part ([AR §7.1] header rules, [AR §8.3] TOKENS row; [F19] freezes them): the base fields
(§1.3) with their separators ≤ 60 B, `tx (dry)` of a `DRY` header included in the place of the rows field; the `files` segment
with its separator ≤ 80 B; the view descriptors and extras (`across`, `diff`, composite parts, `search`, `recompute`, `schema`,
`behind`, `moved`, and a write's `key`, `lease`, `IF TIP ok`, `IF TARGETS ok` and `would commit <n> changes`, §9) ≤ 60 B;
`dropped` and `more` ≤ 30 B; the reader note of line 2 ≤ 80 B. [F19 §4.2] owns these numbers, which this section restates;
[90 §6.3]'s combined figures are superseded ([50 §6.4] as amended).

3.4. **Headers of writes, explain and check** are §9.1–§9.4 and §10.

3.5. **`--count`** prints the header only; its `rows` field is the result's total row count instead of the page's, evaluated
under the same budgets. A budget cut prints `<n>+ rows` and exits 10.

## 4. The reading echo

4.1. **When.** A reading-echo line is printed for ([50 §6.4], [90 §8.1] L8):
- under the `gated` model profile ([90 §8.2]): every *anchored hop* on a *same-kind* edge kind, and every edge pattern written
  with a reverse alias;
- under the `compatible` and `unknown` profiles: every edge pattern and every quantified group of every `MATCH` and
  `OPTIONAL MATCH` clause, anchored or not, in reads and in `TX` results.

An edge pattern with no type (`-->`, `<--`, `--`, `-[e]->`, `-[*1..3]->`) prints no line under any profile, and neither does a
quantified group that holds one: it names no kind, so it has no reading to confirm, and its direction is the arrow as written
(spec sync 2a; open point 11).

An edge pattern is *anchored* when one of its endpoint node patterns is a node literal, or carries an `id` or `uid` property whose
value is a literal or a parameter. A kind is *same-kind* when its declared source and destination kind sets intersect
([50 §2.5] item 2; F1). Symmetric kinds (`CONTRADICTS`, `RELATES`) never echo under `gated`. Patterns inside `EXISTS {}` and
`COUNT {}` echo like top-level patterns. Each line is printed once per distinct text, in the order of first appearance in the
query.

4.2. **Line.** `reads: `, the display (§4.3), ` | `, the reading (§4.4), LF. When the pattern was written with a reverse alias,
the display is followed by ` (written `, the written pattern in the same display form with the alias's name, and `)`:
`reads: b BLOCKS #51 (written #51 BLOCKED_BY b) | b must finish before #51 starts` ([50 §2.9] Q4).

4.3. **Display.** `<a> <T><q> <b>` in the stored direction (after canonicalising a reverse alias, [50 §3.2]):
- `<a>`, `<b>`: an anchored endpoint prints as its id (`#88`; a `#u:` literal as the local `#N` when the store knows it); a named
  endpoint as its variable name; an anonymous one as `()`, or `(:<label>)` when it has a label; a bound parameter as its bound
  id;
- `<T>`: the LQ name of the stored kind; an alternation prints its names joined by `|` (`BLOCKS|GATES`), in the written order
  that the C-AST keeps ([LQ/canonical-ast §5.4]);
- `<q>`: the quantifier in the display spelling (`HOLE(LQ-display-spelling)`, [LQ/gql-spelling §4]): `{1,2}`, `+`, `{2,}` under
  GQL, `*1..2`, `*1..`, `*2..` under Cypher; empty for a single hop and for `{1,1}`, both on a single edge and on a quantified
  group of several edges (the display printer's `*1`/`{1}` is not used here);
- an undirected pattern (`-[:T]-`) prints `<a> <T><q> <b> (either direction)`;
- **an alternation that mixes directions** (its entries have different effective directions, [LQ/canonical-ast §5.4], as
  `-[:BLOCKS|PARENT_OF]->`) prints one part per effective direction, in the order `right`, `left`, `both`, joined by ` or `:
  the `right` entries as `<a> <T…><q> <b>`, the `left` entries with the endpoints swapped as `<b> <T…><q> <a>`, the `both`
  entries as `<a> <T…><q> <b> (either direction)`; each part joins its names with `|` in written order
  (`reads: a BLOCKS b or b CHILD_OF a (written a BLOCKS|PARENT_OF b) | a must finish before b starts, or b is a child of a`);
- a quantified group of one edge prints as that edge between the group's outer endpoints ([50 §2.9] Q5:
  `x BLOCKS{1,5} #93`); a group of several edges prints `<a> (<T1> <T2> ...)<q> <b>`.

4.4. **Reading.** The kind's `reading` template (F1, [F08]) with `{a}` and `{b}` replaced by the displayed endpoints;
for an alternation, the readings joined by `, or `, in the order the display prints the names (§4.3); for an undirected
pattern, `<reading(a, b)>, or <reading(b, a)>`; for a group of several edges, `<a> reaches <b> through <T1> then <T2>`.
Then the quantifier suffix, after one space:

| Quantifier (canonical) | Suffix |
|---|---|
| none, `{1,1}` | none |
| `{m,n}`, m < n | `(through m to n steps)` |
| `{m,}` (with `+` = `{1,}` and `*` = `{0,}`) | `(through m or more steps)` |
| `{m,m}`, m ≠ 1 | `(through exactly m steps)` |

4.5. **The core readings** (the F1 `reading` value of every core edge kind; [F08] stores them, and this table is what the echo
prints; [50 §2.5]'s "Reading" column, in F1's `{a}`/`{b}` form):

| LQ name | Reading | LQ name | Reading |
|---|---|---|---|
| `CHILD_OF` | `{a} is a child of {b}` | `IMPLEMENTS` | `{a} implements {b}` |
| `BLOCKS` | `{a} must finish before {b} starts` | `REFUTES` | `{a} refutes {b}` |
| `GATES` | `verdict {a} gates the completion of {b}` | `CONFIRMS` | `{a} confirms {b}` |
| `MERGE_AFTER` | `lane {a} merges after lane {b}` | `VERIFIES` | `{a} verifies {b}` |
| `RUNS_IN` | `run {a} runs in lane {b}` | `ADDRESSES` | `{a} addresses finding {b}` |
| `ANSWERS` | `{a} answers question {b}` | `ABOUT` | `{a} is about {b}` |
| `SCOPED_TO` | `{a} is scoped to area {b}` | `DISCOVERED_FROM` | `{a} was discovered from task {b}` |
| `DUPLICATE_OF` | `{a} duplicates canonical {b}` | `PRODUCED` | `run {a} produced {b}` |
| `DEPENDS_ON` | `section {a} depends on section {b}` | `CONSUMED` | `run {a} consumed {b}` |
| `SUPERSEDES` | `{a} supersedes {b}` | `CONTRADICTS` | `{a} and {b} contradict` |
| `DERIVED_FROM` | `{a} is derived from {b}` | `MENTIONS` | `{a} mentions {b}` |
| `CITES` | `{a} cites {b}` | `RELATES` | `{a} and {b} relate` |
| `AT` | `{a} is anchored in file {b}` | | |

A project edge kind prints the reading its schema row carries.

4.6. **JSON.** The envelope's `reads` key is an array of the line texts without the `reads: ` prefix, in the order of §4.1.

## 5. Row shapes

5.1. **Choosing the shape.** A named query declares its shape ([LQ/std §2.3]). A free-form read takes one from its projection:
- one column holding a node variable → `node`;
- a first column holding a node variable, then one or more further columns → `node with extras`;
- every other projection, and every result under `--format table` → `table`;
- a standalone `CALL` of `history`, `log`, `diff`, `conflicts`, `violations` or `links` → that relation's shape; any other
  standalone `CALL` → `table`.

A column "holds a node variable" when its expression is a bare variable (or yielded column) of node type; `t.id` is a node
value, not a node variable, and renders as `#N` inside a table.

5.2. **Values in text.**

| Type | Rendering |
|---|---|
| node | `#N`; a tombstone referenced from a live row `#N(deleted)` ([50 §3.6]) |
| edge | `<src> <T> <dst>`; an `AT` edge `<src> AT <aN> <anchor kind>` ([50 §6.4]); a flagged blocker edge adds ` flagged` |
| int, counter | decimal, no grouping |
| f64 | ECMAScript's `Number::toString` (the shortest round-trip digits; positional notation for 1e-6 ≤ \|x\| < 1e21 and for 0, else `<d>[.<ddd>]e+<n>` or `e-<n>`), with `.0` appended when the result has neither `.` nor `e` (`28.6`, `0.0`, `0.001`, `1e+21`); `search()`'s `score` is rounded half-even to 3 decimals by the relation, so text and JSON agree ([50 §2.9] Q8) |
| bool | `true`, `false` |
| absent | `-` ([50 §6.4]) |
| enum | its name (`in_progress`); `priority` as `P0`–`P4` in node lines, as the integer in tables and JSON |
| revision | `rev <seq>` in the header, `<seq>` in cells |
| commit | `<c8>`: `c` and the first 8 lower-case hex digits ([AR §5a.1], [50 §2.4]) |
| timestamp | `YYYY-MM-DDTHH:MMZ` (UTC, minutes; [50 §2.9] Q14) |
| duration | an integer and the largest exact unit among `w d h m s`, else milliseconds as `<n>ms` |
| text, sym, path | bare when every byte is in `A-Z a-z 0-9 _ . / : @ # + * ? ~ ^ % ! -` and the value is 1–64 bytes; otherwise quoted as §5.16 says. Free-text fields (`title`, `abstract`, `body`, `text`, `message`, `summary`, `reason`, `failure_scenario`, `answer`, the `*_quote` fields) are always quoted |
| list, set | `[e1,e2]` without spaces, elements rendered by this table; inside node-line keys without brackets (`blockers:#12,#17`) |
| map | `{k1:v1,k2:v2}` in the map's declared key order |

5.3. **Node line** ([50 §6.4]). Fields separated by one space, in this order:
1. `#N`;
2. the kind;
3. the status, or `deleted` for a tombstone row;
4. kind tokens before the title — task: `P<priority>`; finding: `severity`, `f_kind`, `r<round>` (when present), `local_id` (when
   present);
5. `criticality` when not `normal`, then `authority` when not `agent` ([AR §3.1] defaults);
6. the title, quoted (§5.16); an artifact's title is its `path`;
7. kind keys, each `key:value`, omitted when absent or empty — task: `parent:`, `blockers:` (the open direct blockers, a flagged
   dangling one as `#N(flagged)`), `inherited:` (the open exogenous blockers of its ancestors), `labels:`, `lease:<holder>(<lease>)`
   (runtime, tip only), `children:<done>/<total>` (when total > 0); finding: `about:` (the `ABOUT` targets); rule: `applies_to:`
   (`*` when empty); artifact: `state:<link state>` (live results only);
8. extras (§5.4);
9. flags, upper case, in this order: `BLOCKED`, `SUSPECT`, `CONFLICTED`, `DELETED-ELSEWHERE`, `SETTLED-ELSEWHERE` (runtime);
10. for a tombstone row: `(rev <seq> <c8> by <actor> "<reason>" -> <id>)`, the replacement clause only with a replacement.

Examples ([50 §2.9]): `#51 task open P1 "Wire lease reclaim" parent:#9 blockers:#12,#17 inherited:#7 BLOCKED`;
`#162 finding confirmed important perf r2 C2 "Pair cache rebuilt every frame" about:#133`;
`#412 rule active critical owner "Never hold a World borrow across a system boundary" applies_to:crates/ecs/**`.

The runtime parts (`lease:`, `DELETED-ELSEWHERE`, `SETTLED-ELSEWHERE`) are *decorations*: read from the tip's runtime tables when
the line is rendered, excluded from the byte-identity guarantee, and in JSON under the node's `runtime` key ([50 §3.5]).

5.4. **Node with extras.** The node line with each further column as `name=value` (§5.2 rendering) inserted before the flags, in
column order, absent values omitted ([50 §2.9] Q8 `score=7.412`, Q26 `depth=2 via=#90`). `name` is the column name of §5.5.

5.5. **Table.** A line of column names, then one line per row. Columns are separated by two spaces; every column but the last is
padded with spaces to the width, in Unicode scalar values, of its widest cell on the page (name line included); no line has
trailing spaces. A column's name is its `AS` alias, else the display printer's text of its expression ([LQ/gql-spelling §5]:
`s.id`, `count(*)`, `link_state(a)`). Absent is `-`.

5.6. **Detail** (`show`). The node line, then one line per present field: two spaces, the field name, `: `, the value; fields in
the schema's declaration order ([F08]), header columns first (`uid` only with `--uid`); then, with `--full`, the body fenced as
§5.16. Rows of ids that did not resolve are omitted, each reported by N01, N06 or N12, and the result exits 3 when any requested
id is missing ([LQ/errors] open point 6).

5.7. **Tree.** Each row is a node line indented by two spaces per `depth` (the root at depth 0), in `position` order.

5.8. **Diff.** One line per change ([50 §6.4], Q13, Q19):
`<change> <id> <kind> <aspect>[ <name>] <what>[ [<side>]][ rev <seq> <actor>]` where `<change>` is `+` added, `-` removed, `~`
changed; `<name>` is omitted when it equals the aspect (`status`); `<what>` is `<before>-><after>` for fields and statuses,
`"<title>" created` or `"<title>" deleted ("<reason>", replaced_by <id>)` for existence, `<n> lines +<a> -<r>` for bodies, and
`<T> <id> added|removed` for edges; `[<side>]` (`ours`, `theirs`, `both`) only for three-dot ranges; `rev <seq> <actor>` is the
last commit's, absent in `DRY`.

5.9. **History** (`history`, `log`). `history`: `rev <seq> <c8> <ref> <actor> <role> <at> <op> <aspect>[ <name>] <what>
"<message>" via <origin>` ([50 §2.9] Q14), where `<op>` is `+`, `-`, `~`, `<what>` as §5.8 (for a create, the kind), and
`<origin>` renders F10's `stmt_origin`/`stmt_sym` ([F06]): a named mutation as its name (`tx.complete`), a free `TX` as `tx`, an
MCP `write` as `mcp.write`, a merge as `merge`, an import as `import`, a file verb as `file <verb>`. `log`:
`rev <seq> <c8> <ref> <actor> <role> <at> <commit kind> "<message>" ops:<n>`.

5.10. **Conflict and violation.** Conflict: `<key> <class> base=<value> ours=<value> theirs=<value> (<commit kind> rev <seq>)`,
then `  resolve: TX ON <ref> { RESOLVE '<key>' TAKE OURS }   (or THEIRS, BASE, VALUE $text)` ([50 §2.9] Q16). Violation:
`<key> <class> <detail>`, then `  suggested: <statement>, then moirai merge --continue <src> --into <dst>`. The key's text form
is [F12]'s conflict key (`#91.body`, `edge:#203:blocks:#40`).

5.11. **Across.** As a table for free-form `across()`; the named query `across` prints one line per diverged `(node, name)`:
`<id>.<name> <ref>=<value> <ref>=<value> !=` ([50 §6.4]), `!=` only when the values differ.

5.12. **Loop.** The table of `std.loop`, then one line: `confirmed blockers in round <r>: <n> -> continue` when `<n>` > 0, else
`confirmed blockers in round <r>: 0 -> DESIGN APPROVED` ([50 §6.4], [AR §7.6]); `<r>` is the last round listed.

5.13. **Links** ([40 §3.8] as corrected by [LQ/std §2.8] item 2 and §4.21). One line per link: `<handle>  <state>  <path>[ -> <now>]  <detail>
[ | verify <handle>]` where `<handle>` is the file node `#N` for file rows and the anchor handle `aN` for anchor rows, `<state>`
the link state padded to 20 bytes, `<detail>` R-16's detail text ([F18]); the `verify` part repeats the line's own handle
(`verify #815`, `verify a31`; pass 1, A1-48). Then one legend line `verify #N: moirai file where N --evidence` when any line
carries `verify` ([F19 §4.6] rule 4). The line never prints a command that accepts a guess: its last part is the next command,
evidence or settle ([41 M6]; `links()`'s `next` column; the A1 re-review's A-m2).

5.14. **Changes** (`changes`, `delta`). `rev <seq> <ref> <id> <op> <aspect>[ <name>] <actor>`.

5.15. **Blockers.** The node-with-extras shape over `blocker`, with `depth=`, `via=` (absent for direct), `reason=`, `FLAGGED` as a
flag when `flagged`, and `elsewhere=<ref>` when set.

5.16. **Untrusted text** ([50 §6.4], [16 §6.9]). A quoted value is `"` + the value with `"` → `\"`, `\` → `\\`, CR → `\r`, LF →
`\n`, TAB → `\t`, other C0 controls and DEL → `\u{h}` + `"`, where h is the code point in lower-case hexadecimal without leading
zeros (`\u{1}`, `\u{1b}`, `\u{7f}`), the one spelling of [F19 §2.5] rule 1 and of LQ's string escapes ([LQ/lexical]; pass 1,
A1-48). When the escaped value exceeds 120 bytes it is cut at the last
scalar boundary at or before byte 117 that does not split an escape, and `...` is appended inside the quotes. Bodies appear only
when projected or with `--full`, fenced:
`--- body <id> | <n> B | by <actor> rev <seq> | untrusted text ---`, the body bytes, then `--- end body <id> ---`. If the body
contains a line equal to the closing fence, the fence gains `-` characters on both sides until no body line equals it.
`authority` and `owner_quote` render so an owner ruling is distinguishable from an agent note.

5.17. **Row order.** The rows print in the result's total order ([50 §3.5]).

5.18. **Degradation under the byte budget.** A page is rendered at L1 (the full node line). If the rendered result exceeds the
output byte budget (`query.budget.default.bytes`, 8,000 B, agent maximum 24,000; [50 §5.10]), every node line of the page is
re-rendered at L0 (`#N kind status "<title cut at 40 B>"`, then the flags), the header gains `dropped <n>`, and the footer a
`dropped:` line (§6.2). If it still exceeds the budget, the page is cut to its longest prefix that fits and a cursor is emitted
([50 §5.10]: "degrade the row format (L1 → L0), then paginate, exit 0").

## 6. Footers

6.1. **Order** of the footer block, each part optional: (1) warnings, ascending code; (2) notices, ascending code
([LQ/errors §4.2]); (3) the `dropped:` line; (4) the budget-use line; (5) the continuation line, always last ([90 §6.3]).

6.2. **Dropped.** `dropped: node keys on <n> rows (use --json or --limit <m>)`; a projected body replaced by its length:
`dropped: bodies (add --full)` ([50 §6.4]).

6.3. **Budget use** is printed only when a budget was more than 50 % used and not exhausted ([50 §6.4]):
`budget: <k> <N>/<N>` for each such budget, joined by `, ` (`budget: work 1,203,441/2,000,000`).

6.4. **Continuation.** A paginated result: `<n> more | cursor <cursor> | <rerun>` when the remaining row count is known (blocking
plans and exact-count anchors), else `more | cursor <cursor> | <rerun>`. A budget cut of a resumable plan: `budget: <k> <N>
exhausted after <id> | continue: <rerun> | exit 10`; an `fs` cut: `fs: <N> units used; <n> links unverified | --budget fs=<N> |
exit 10` ([50 §6.4]). `<rerun>` is, for a named query invoked by argv, `moirai q <name> <k=v ...> --cursor <cursor>` in canonical
argument order; for free-form text, `run the same query with --cursor <cursor>`; for the `query` tool, `call query again with
cursor=<cursor>` (open point 2).

6.5. **`--ids`.** Stdout carries `#N` lines only, no header and no row cap. The page and the exit code are [F19 §6.2]'s, which this
section cites (pass 1, A1-31): a result that fits `output.ids-max-bytes` (24,000 B; `0` = unlimited, [90 §2.1]) prints whole and
exits 0; a cut page prints at most min(`output.ids-max-bytes`, `output.nonzero-exit-max-bytes`) bytes, 8,000 B by default, and
exits 10. A cut by that page or by a budget writes to stderr the line `ids: <n> printed | more | cursor <cursor> | <rerun>`,
after any warnings and notices ([50 §3.5]).

6.6. **Both ends.** The header carries `dropped` and `more` whenever the footer does, so a head, middle or tail cut of one result
keeps one of them ([90 §6.3]).

## 7. The JSON envelope (`--json v1`)

7.1. **Form.** One line of compact JSON (no insignificant whitespace), LF after it. Keys in this order:

| Key | Type | Present | Content |
|---|---|---|---|
| `v` | int | always | `1` |
| `branch` | string | always | §3.2 `ref` |
| `rev` | int | always | the view commit's sequence number |
| `commit` | string | always | the view commit's id as `c` + 64 lower-case hex |
| `view` | string | always | `tip`, `as-of` or `staged` |
| `use` | string | when a `USE` or `--at` applied | the revspec as written |
| `live` | bool | when `true` | |
| `tree` | string | file-bearing results | the tree's canonical path ([F19]) |
| `parts` | array | composite queries | `[{"op":<string or null>,"use":<string or null>,"rev":<int>,"commit":<string>,"view":<string>}...]`, `op` null for the first part |
| `reads` | array of string | when echo lines exist | §4.6 |
| `cols` | array of string | always | the column names of §5.5 |
| `data` | array | always | the rows (§7.4) |
| `next` | string or null | always | the cursor |
| `dropped` | object or null | always | `{"count":<int>,"what":[<string>...],"more":<string or null>}`; `what` holds `"node keys"` or `"bodies"` |
| `notices`, `warnings` | array | always | [LQ/errors §4.3] |
| `budget` | object | always | §7.5 |

A result that ends with an error adds `"errors":[…]` and `"exit":<int>` after `budget` ([LQ/errors §4.1]); a failed call with no
rows prints [LQ/errors §4.1]'s error envelope instead.

7.2. **Additivity.** New keys are appended after the keys above; `v` stays 1 ([07 §6.2], [AR §7.1]).

7.3. **Values in JSON.** Nodes are strings `"#N"`; commits `c` followed by 64 lower-case hex digits (the A1 dispositions of A-m7
and S-24: as Q1, Q6 and the `.moi` files carry them, and a revspec takes them back);
revisions integers; timestamps RFC 3339 UTC with milliseconds (`2026-09-25T12:03:00.000Z`); durations integers of
milliseconds; enums their name; `priority` an integer; f64 as in §5.2; absent `null`;
text as JSON strings with `"`, `\` and C0 controls escaped (`\n`, `\r`, `\t`, else `\u00XX`) and every other scalar as raw
UTF-8, never truncated; an edge `{"src":"#N","type":<LQ name>,"dst":"#N"}` plus `"anchor":"aN","kind":<anchor kind>` for `AT`
and `"flagged":true` for a flagged edge.

7.4. **Rows.** In the `node` shape, each row is a node object; otherwise an object keyed by column name in column order. A node
object has, in order: `id`, `uid` (with `--uid`), `kind`, `status`, `priority` (tasks), `criticality` and `authority` (when not
default), `title`, then the kind keys of §5.3 as `parent`, `blockers`, `inherited_blockers`, `labels`, `children_done`,
`children_total`, `severity`, `f_kind`, `round`, `local_id`, `about`, `applies_to`, `path`, `state`, then the flags `blocked`,
`suspect`, `conflicted` as `true` (omitted when false), `rev` (the node's `rev_seq`), and `runtime`
(`{"lease":{"id":<string>,"holder":<string>,"branch":<string>,"expires":<timestamp>},"settled_elsewhere":[<ref>...],
"deleted_elsewhere":[<ref>...]}`, only the parts that exist, the key absent when none) ([50 §2.9] Q1, §3.5).

7.5. **Budget object.** Always, in this order: `"work"`, `"work_limit"`, `"rows"`, `"bytes"` ([50 §2.9] Q1) and `"mem"` (the
bytes charged to `mem`); then, only when the query used them, `"visited"`, `"visited_limit"`, `"refs"`, `"refs_limit"`, `"fs"`,
`"fs_limit"`, `"asof_ops"`, `"asof_limit"`. All values are integers. `mem`'s limit is omitted because it depends on the process's
headroom and would break byte identity ([50 §5.10]); E502's text carries it.

7.6. **`--jsonl`.** Line 1 is the envelope's keys `v` to `cols`; then one row object per line; the last line is
`{"next":…,"dropped":…,"notices":[…],"warnings":[…],"budget":{…}}` ([50 §6.4]: "one row per line after a header object").

7.7. **`TX` results** (§9) use the envelope with `data` = the `diff` rows of the commit (or of the `DRY` candidate) and these keys
after `commit`: `"rev_new":<int or null>`, `"key":<string or null>`, `"lease":<string or null>`, `"replayed":<bool>`,
`"dry":<bool>`, `"targets":<32 hex or null>` (the target-set digest, `DRY` only), `"statements":[{"index":<int>,"text":<string>,
"matched":<int or null>,"targets":["#N"...]}...]`, `"affected":{"ready":["#N"...],"other":["#N"...]}`,
`"markers":[{"kind":<string>,"id":"#N","ref":<string>}...]`.

## 8. Cursors

8.1. **Contents** ([50 §3.5], [16 §6.4]): the query hash, the view commit per part, page 1's `now()`, the last sort key, the page
size and the remaining work budget, plus the `live` flag. The fixed head is an offset table; the whole cursor is a sequence
table ([F01 §2.6]; pass 1, A1-48), little-endian:

`CursorHead`:

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 1 | `u8` | `version` | `1` |
| 1 | 1 | `u8` | `flags` | bit 0 `live`; bit 1 `ids` (an `--ids` byte page); bits 2–7 reserved, zero |
| 2 | 2 | `u16` | `page_rows` | rows per page; 0 under `--ids` |
| 4 | 4 | `u32` | `pages` | pages already served (≥ 1) |
| 8 | 8 | `u64` | `query_hash` | the first 8 bytes of the query hash `H` read as little-endian u64 ([LQ/canonical-ast §1.1], [50 §5.3]) |
| 16 | 8 | `u64` | `now_ms` | page 1's `now()`, milliseconds since the Unix epoch, UTC |
| 24 | 8 | `u64` | `work_left` | work units left of the query's budget |
| 32 | 1 | `u8` | `n_parts` | number of view commits that follow; 0 for a live cursor |
| 33 | 1 | `u8` | `n_keys` | number of sort-key values that follow |
| 34 | 2 | `u16` | `_reserved` | zero |
| total | 36 | | | |

The cursor:

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `head` | `CursorHead` | always | above |
| 2 | `part_commit` | `n_parts` × `b16` | always | each part's view commit, its `id16` (first 16 bytes of the commit id) |
| 3 | `key` | `n_keys` sort-key values (§8.2) | always | the last emitted row's full sort key: the `ORDER BY` values, then the binding identity |
| 4 | `check` | `u64` | always | XXH3-64 (seed 0) over every preceding byte |

8.2. **Sort-key values.** Each is a tag byte and a payload:

| Tag | Type | Payload |
|---|---|---|
| 0 | absent | none |
| 1 | false | none |
| 2 | true | none |
| 3 | int, counter | i64 |
| 4 | f64 | u64, the IEEE-754 bits |
| 5 | text, sym, path | u32 length, the UTF-8 bytes |
| 6 | node | u32 `#N` |
| 7 | edge | u32 src, u32 dst, u8 name length, the LQ name's bytes, u8 `has_disc` (0 or 1), then 16 bytes of discriminator iff `has_disc` = 1 |
| 8 | revision | u64 seq |
| 9 | timestamp | i64 milliseconds since the Unix epoch |
| 10 | enum | u16 declared rank ([50 §3.5]) |
| 11 | duration | i64 milliseconds |
| 12 | commit | 16 bytes, the `id16` |
| 13 | list | u32 count, then the elements |

8.3. **Text form.** `k` followed by the Crockford base-32 encoding of the bytes: alphabet `0123456789abcdefghjkmnpqrstvwxyz`,
five bits per character taken most-significant bit first across the byte string, the last group padded with zero bits, no
padding characters ([16 §6.4]: `[a-z0-9]`, no `+`, `/` or `=`). The text never starts with `#`, `~`, `=`, `@`, `!`, `-` or `/`
([80 §4] T2).

8.4. **Validation.** A cursor is refused with E306 when its text has a character outside the alphabet, its `check` does not
match, its `version` is not 1, a reserved bit or field is non-zero, or its `query_hash` differs from the hash of the query it is
passed with. A pinned cursor evaluates page N+1 at its `part_commit`s with the keyset `key` and `now_ms`; a live cursor at the
current tip with the keyset and W06 ([50 §3.5]).

## 9. `TX` results

9.1. **Committed.** Header: `branch: <ref> | rev <old> -> <new> | committed <c8>[ | key <key>][ | lease <lease>][ | staged]`
([50 §2.9] Q18, Q24, [AR §7.1]); `staged` marks a commit on a staging ref. Then one line per statement:
`<i> <summary>[: <effect>]` or `<i> <summary> (<note>)`, with the mutations a `MATCH` target carries as lines indented two spaces
and a `DELETE`'s per-edge policy results indented four ([50 §2.9] Q24). Summaries:

| Statement | Summary | Effect or note |
|---|---|---|
| `MATCH … EXPECT` | `MATCH ... EXPECT <expect>` | `matched <n> (<ids, at most 10>[, ...])` |
| `SET` | `SET <id>.<field> = <value>` | `<field> <before>-><after>`; for `status`/`done`: `status <from>-><to> (resolution <r>)`, and `; lease <lease> released into settled` when it settled a leased task |
| `REMOVE` | `REMOVE <id>.<field>` | `<field> <before>->-` |
| `CREATE` node | `CREATE <kind> <id> "<title>"[ -[:<T>]-> <id>]...[ UNDER <id>]` | note `UNLESS EXISTS matched 0`, or `bound to <id>, UNLESS EXISTS matched 1` |
| `CREATE` edge | `CREATE <id> -[:<T>]-> <id>` | none |
| `DELETE` node | `DELETE <id> <kind> "<title>"[ REPLACED BY <id>]` | policy lines: `<id> -[:<T>]-> <id>` padded, two spaces, the action (`re-pointed: <edge>`, `dropped (<why>)`, `kept as a tombstone reference[; <id> suspect]`, `flagged`) |
| `DELETE` edge | `DELETE <id> -[:<T>]-> <id>[ <aN>]` | none |
| `MOVE` | `MOVE <id> UNDER <id>` | `parent <old>-><new>` |
| `REOPEN` | `REOPEN <id>` | `status done->open; reopen_count <n>` |
| `PATCH` | `PATCH <id>.body` | `<n> lines +<a> -<r>` |
| `RESOLVE` | `RESOLVE <key> TAKE <choice>` | the resulting value or edge (`the edge becomes #203 -[:BLOCKS]-> #52`) |
| `CALL tx.*` | `CALL <name>(<first argument>)` | the mutation's effect line ([LQ/std §7]) |
| `ASSERT` | `ASSERT <expression, display text cut to 40 B>` | `-> true` |
| `DEFINE QUERY` | `DEFINE QUERY <signature> SHAPE <shape>` | ` \| LQ <v> \| bound against schema v<n> \| ok`, then `  stored portable: <id> -> <#u:...>, ... \| exported as schema/queries/<q>.moi` with `<q>` the 32 hex digits ([50 §2.9] Q21) |
| `DROP QUERY` | `DROP QUERY <name>` | none |

Then one line `affected: ` followed by `none`, `newly ready <ids>`, `<ids>`, or `newly ready <ids>; <ids>` (ids joined by one
space; the second list is the other affected ids), then ` | markers: ` and the markers as `<kind> <id> (<ref>)` joined by `, `
when the commit produced markers ([50 §3.10] item 7): Q18 becomes `affected: newly ready #93 | markers: settled #89 (lane/l5np)`
and Q24 `affected: #12 #17 #77 | markers: deleted #40 (lane/l10)`. Then, on a staging ref, `next: moirai merge --continue <src>
--into <dst>` ([50 §2.9] Q25).

9.2. **Replayed.** `branch: <ref> | rev <tip> | replayed`, then `replayed: rev <seq> <c8> (key <key>)`, exit 0 ([AR §6.4];
[50 §2.9] Q20's re-run sentence and [50 §3.10] item 8). Q20's block itself does not bind as written: its second statement
makes a `finding` the source of `DERIVED_FROM`, whose sources are {note, doc, verdict, artifact} ([F08 §9.6]), so a
golden built on Q20 writes that statement as `CREATE (f)-[:CITES]->(#133)` (`cites`: any → knowledge); [50 §2.9] Q20 is
corrected so at WP-81a (spec sync 2a).

9.3. **`DRY`.** Header: `branch: <ref> | rev <tip> | tx (dry)[ | IF TIP ok][ | IF TARGETS ok] | would commit <n> changes`
([50 §2.9] Q19 without its `nothing written`, which a dry run implies; `tx (dry)` counts in the base part and the rest in the
extras part, at most 57 B: [F19 §4.2], pass 1, A1-30). Body: the statement lines of §9.1, where each `MATCH … EXPECT` lists its targets under it as
`  target <id> <kind> "<title>"` ([90 §8.1] L3; at most 50 per statement, then `  ... <n> more targets (the digest covers all)`);
the `diff` rows of §5.8 without `rev`/actor; `<i> ASSERT <expr> -> true` lines; `affected: <...> | invariants ok`;
`targets: <digest>`; and `to apply: send the same TX without DRY, with IF TARGETS '<digest>'`, followed by
`; IF TIP <c8> refuses it if <ref> moved in between` when the block has `IF TIP`.

9.4. **The target-set digest** ([50 §3.10] items 4 and 9, [72 m1]) is BLAKE3-128 over the byte sequence below, printed as 32
lower-case hex digits in the `targets:` line, in JSON `targets` and in `IF TARGETS '<hex>'`. This is the layout [LQ/canonical-ast]
open point C-11 proposes for this chapter to adopt; `lp()`, `u16`, `u32` and the uid are [F01]'s encodings.

| # | Field | Encoding | Meaning |
|---|---|---|---|
| 1 | domain | `lp("moirai-lq-targets-v1")` | domain separation |
| 2 | `lq` | u16 | the grammar version, 1 |
| 3 | per `MATCH … EXPECT` statement, in block order | u32 index (1-based), u32 binding count, then the bindings sorted bytewise | only `MATCH … EXPECT` statements bind target sets; literal and parameter targets cannot be swapped |
| 3a | a binding | the concatenation, in variable-index order ([LQ/canonical-ast §5.7]), of the values of the variables the statement's mutations use | a node: its 16-byte uid; an edge: source uid (16), `lp(<LQ name of the stored kind>)`, destination uid (16), and the 16-byte discriminator or 16 zero bytes |

`IF TARGETS` recomputes this digest from the targets bound under the writer byte and refuses the block with E402 when it
differs ([LQ/errors §5.5]).

## 10. EXPLAIN, PROFILE, `--check` and `--show-query`

10.1. **`--check`** (the `query` tool's `mode: check`): the header with `check` in place of the rows field, the reading-echo lines,
one line `ok` followed by the output columns as `<name>: <type>` joined by `, `, then every warning and notice the binder raises
([50 §5.11]).

10.2. **EXPLAIN and PROFILE.** The header `explain | branch <ref> | rev <seq> | <c8> | tip|as-of|staged | pinned|live | query
q:<8 hex> | parse+bind <n> us` ([50 §2.9] Q23; `q:` shows the first 8 hex digits of the query hash), the `reads:` lines, and the
`budget` line ending in `resumable: yes` or `resumable: no`. The plan lines between them are informative and not frozen at M0:
the operators belong to M7's planner ([50 §8.2] LQ-6), and LQ-Bench's server refuses `explain` and `profile` (PLAN WP-71b).

10.3. **`--show-query` and `--show-tx`.** Line 1: the qualified name and the signature as a `DEFINE` writes it
(`std.ready($scope: node? = NULL, $role: text? = NULL, $limit: int = 20) SHAPE node`), three spaces, and `(built-in, LQ <v>)` or
`(project, LQ <v>)`; then the body by the display printer ([LQ/gql-spelling §5]); then `// bound: $<p> = <value>, ...` with every
parameter in declaration order ([50 §2.9], "Named queries from the shell"). `--show-tx` prints the verb's `TX` expansion
([LQ/std §7]) the same way.

## 11. The comparison form of a result (for LQ-Bench gold results)

PLAN §3.3 assigns the mapping of gold answers ([50] g-10) to WP-19 and WP-70. This chapter fixes what a result *is* for
comparison; WP-70's scorer fixes how two are scored ([50 §7.4] item 4):
- a read's result is its JSON `data` array over every page (a paginated result is concatenated in page order), with `cols`;
- a write's result is its `DRY` JSON `data` (the would-be `diff` rows) and its `statements[].targets`;
- node references compare by uid, so a gold result stated against the seeded fixture store survives a regeneration that
  renumbers `#N`; the real-session stratum's private referent mapping ([50 §7.4] item 2) maps owner-side referents to fixture
  uids;
- runtime decorations (`runtime`) and the `budget`, `notices`, `warnings` and `reads` keys are not part of the result; the
  scorer reads warnings, notices and echoes separately to classify an answer as hedged ([50 §7.4] item 4).

## Coverage

| Item | Where (and who covers the rest) |
|---|---|
| [90 §10.1] "Output contract": byte units in headers and footers, the both-ends rule, ASCII only, header limits with `dropped`/`more`, the `--ids` page rule — as LQ results print them | §2.2, §3.1–§3.3, §6.4–§6.6; the rules themselves: [F19] |
| [60 §2.5] "Harness-agnostic interface" row, part "the output contract's byte units, both-ends, ASCII and `--ids` page rules" | §2, §3.3, §6; the row's other parts: see [LQ/errors] Coverage |
| [50] F1: the `reading` templates and `reverse_names` as the reading echo prints them | §4.3–§4.5; the F1 row bytes: [F08] |
| [50] F10: `stmt_origin`/`stmt_sym` printed as `via` | §5.9; the header bytes: [F06] |
| [50] F12: the search header fields and `score` printing | §3.2, §5.2; `DOCLEN`, `TERMS`/`POST` and the tokenizer byte: [F09] |
| [50] F16: `affected_complete` = 0 printed as the `recompute` extra and W05 | §3.2; the header field: [F06] |
| [50] F17: the `detail` shape's missing ids (N06, N12) | §5.6; the `ALLOC` layout: [F11] |
| [80] X-F12 T2 (the cursor alphabet) and T7 (byte-identical output, golden substitutions) | §2.1, §8.3; T1–T10 as a whole: [OS/shell] |
| [40] R-16 as printed (link states, `files: no tree bound`, `reading only`) | §3.2, §5.13; the strings: [F18] |

## Holes

None of its own. The reading echo's display pattern (§4.3) and the `--show-query`/`--show-tx` bodies (§10.3) print quantifiers
in the display spelling, `HOLE(LQ-display-spelling)`, which [LQ/gql-spelling] defines and WP-72 fills. The cursor layout, the
target-set digest and every text of this chapter are decided here, with no measured input.

## Open points for the review

1. **Header limits (the A1 re-review's A-M3).** §3.3 follows [AR §7.1]'s per-part rules as the dispositions amended them; [F19]
   freezes the numbers and the goldens (WP-22, WP-71a) re-count every example per part. `schema` (catalog results) is counted
   with the extras, which [AR §7.1]'s list does not name.
2. **The continuation must name the query.** [50 §6.4]'s footer `moirai q --cursor k7f3q2` cannot resume a free-form query: the
   cursor carries a hash of the query, not its text. §6.4 prints the full argv for named queries and a rerun instruction for
   free-form text and the `query` tool.
3. **Commit ids.** Text `<c8>` is `c` plus 8 hex digits (9 bytes) as [AR §5a.1] and [50 §2.4] state; [50 §2.9]'s examples show
   7 digits (`c9c0aa17`) and are illustrative. JSON carries `c` + 64 hex, as the A1 dispositions decided (A-m7, S-24) and [F19]
   freezes.
4. **Choices [50] leaves open**, each fixed here without a measured input: the shape-selection rule (§5.1; it renders [50 §2.9]
   Q4's `CALL … RETURN blocker, depth, via, reason` and Q12's `CALL across(…) … RETURN node, name, ref, value` as node-with-extras,
   where those examples show tables; [50 §2.9]'s examples are not consistent among themselves, since Q8, Q9, Q26 and Q27 print the
   same projection form as node-with-extras), the bare-value
   rule (§5.2; it prints `applies_to:*` with `:` like the other kind keys, where Q10 shows `applies_to=*`), the node-line fields per
   kind (§5.3; the normative list of [50 §6.4] is followed, so Q9's `hazard` note subtype is not printed), the L0 row form (§5.18),
   the footer order (§6.1), `--count` (§3.5), the JSON key order and presence rules (§7), the cursor layout (§8), the `TX` line
   forms (§9; [50 §2.9] Q18 and Q24 print `affected`/`markers` in two orders, unified here, and Q19's `assert 2:` becomes
   `2 ASSERT`). The target-set digest's input bytes (§9.4) are [LQ/canonical-ast] C-11's proposal, adopted here (a gap PLAN §3.3
   assigns to no WP).
5. **`DRY` lists every target by title ([90 §8.1] L3)**, which [50 §2.9] Q19 does not show. §9.3 adds `target` lines, capped at 50
   per statement because a bulk target set would otherwise exceed the page; the digest still covers all.
6. **[AR §7.1]'s `blocking` header annotation** `(1 settled elsewhere hidden: …)` is not reproduced: `std.blocking` filters those
   rows in its text ([LQ/std §4.2]) and the triage line belongs to `brief_triage`. The review may add a counted extra.
7. **The display printer** used by §4, §10.3 and the replacement texts is specified in [LQ/gql-spelling §5] rather than in
   [LQ/canonical-ast], because it depends on the display spelling and not on the canonical encoding.
8. **Not closed here.** The BM25 arithmetic that makes the two search tiers rank identically (the A1 review's S-22) is needed only
   if the search-stratum ablation keeps BM25; it belongs with F12's statistics ([F09]), and this chapter fixes only how `score` is
   printed. Goldens (WP-71a) are authored from this text, never copied from [50 §2.9]'s examples (S-24).
9. **The comparison form** (§11) is offered to WP-70; if WP-70's scorer defines another, §11 follows it.
10. **Pass 1 changes** (A1-30, A1-31, A1-48). The `DRY` header drops `nothing written` and counts `tx (dry)` in the base part,
    and composite parts drop their `<c8>` (§3.2, §3.3, §9.3), as [F19] open point 1 proposed, so every golden header meets the
    60 B extras limit. `--ids` pages cite [F19 §6.2] (§6.5): a cut page holds at most min(`output.ids-max-bytes`,
    `output.nonzero-exit-max-bytes`) = 8,000 B by default and exits 10, as the owner decided on 2026-09-28 (OQ-F-1 (a);
    WP-81a edits [AR §7.1]'s 24,000 B). Link lines print `verify <handle>` with the line's own handle (§5.13);
    control characters escape as `\u{h}` without leading zeros (§5.16); the cursor layout is an offset table for its fixed
    head plus a sequence table (§8.1).
11. **Spec sync 2a** (WP-93a review and author). The reading echo: `<q>` is empty for `{1,1}` on a single edge and on a
    quantified group of several edges (§4.3); an edge pattern with no type prints no echo under any profile, nor does a
    group that holds one (§4.1), because it names no kind whose reading could be confirmed, and a generic reading would cost
    a line per pattern without telling the agent anything its arrow does not; an alternation that mixes directions prints
    one part per effective direction, `right` then `left` (endpoints swapped) then `both`, and its readings follow that
    order (§4.3, §4.4). §9.2 cites the replay rule's sources ([AR §6.4], [50 §3.10] item 8) and records that [50 §2.9] Q20's
    `DERIVED_FROM` from a finding does not bind ([F08 §9.6]); the example becomes `CITES` at WP-81a.
