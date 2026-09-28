# LQ spellings: Cypher, GQL, the strict-GQL spelling mode and the display printer

| | |
|---|---|
| Title | The spelling table of LQ v1 (every construct accepted in a Cypher and a GQL spelling, and the one form each maps to), the strict-GQL spelling mode used as an LQ-Bench alternative surface, the display spelling of quantifiers, and the display printer |
| Chapter | [LQ/gql-spelling], `docs/spec/lq/gql-spelling.md` |
| Status | draft, pass 1 pending |
| Work package | WP-19b (R-SPEC-F), part of WP-19 of [PLAN §3.2] item 1 |
| Sources | [50 §2.8] (the three compatibility tables), [50 §2.3] (grammar v1, quantifier and match-mode productions), [50 §2.3.1], [50 §2.6] (function spellings), [50 §3.4], [50 §3.7] (hop bounds), [50 §5.3] (canonical form versus display printer), [50 §7.4] items 3 and 7 (the alternative surfaces and the display-spelling ablation), [50 §8.2] (the LQ-Bench row: "LQ, LQ with strict GQL spellings, and the JSON IR as input"); [90 §8.1] L1, L4, L5; [AR §7.7.5]; [60 §3.1] item 10; PLAN WP-93a ("the strict-GQL spelling mode"); research [14 §3.2], [14 §5.1] (strict ISO GQL as a candidate) |
| Depends on | [LQ/grammar-v1.ebnf] (Annex G, the strict mode's detection), [LQ/canonical-ast] (canonical names), [LQ/lexical], [LQ/errors] (E004 texts) |

## 1. Scope

1.1. LQ v1 accepts the spellings models write — Cypher's and GQL's — and gives each accepted Cypher spelling Cypher's meaning
([50 §2.8]). This chapter fixes four things:
- §2: for every construct with more than one accepted spelling, the Cypher spelling, the GQL spelling, and the *canonical*
  construct both map to (the input of [LQ/canonical-ast]'s encoding, so both spellings hash alike, [50 §5.3]);
- §3: the **strict-GQL spelling mode**, the parser mode of the reference model's LQ-3 front end (PLAN WP-93a) behind LQ-Bench's
  alternative surface "LQ with strict GQL spellings" ([50 §8.2], [60 §3.1] item 10);
- §4: the **display spelling** of quantifiers, the one form moirai prints in the card, `--show-query`, `--show-tx`, error rewrites
  and the reading echo ([50 §2.8], [90 §8.1] L1);
- §5: the **display printer**, which prints LQ text back to agents ([50 §5.3]: "the display printer and the display spelling").

1.2. Forms that are not LQ in any spelling (`MERGE`, `SKIP`, `FOR`, path variables, …) are E004 or E113 in every mode;
[LQ/errors §6] owns their texts.

1.3. The chapter freezes at WP-72: the display spelling by its ablation, the strict-GQL mode's fate by the alternative-surface
ablation.

## 2. The spelling table

"Canonical" is the construct the binder produces; it is written here in LQ's own notation, and [LQ/canonical-ast] owns its bytes.
"Strict GQL" says what the strict-GQL spelling mode of §3 accepts. A row marked *shared* has one spelling in LQ v1, so every mode
accepts it.

2.1. **Edge quantifiers.** Cypher's variable-length spelling keeps Cypher's meaning: a missing lower bound is 1, and a bare `*`
is "one or more" ([50 §2.8]; Neo4j variable-length patterns). GQL's postfix `*` is "zero or more" ([50 §3.7] item 1: "`*` also
admits zero hops"), and `{,n}` has lower bound 0 ([50 §2.3]).

| Meaning (hops) | Cypher spelling | GQL spelling | Canonical | Strict GQL |
|---|---|---|---|---|
| 1 or more | `-[:T*]->`, `-[:T*1..]->` | `-[:T]->+`, `-[:T]->{1,}` | `{1,}` | GQL only |
| 0 or more | `-[:T*0..]->` | `-[:T]->*`, `-[:T]->{0,}` | `{0,}` | GQL only |
| m or more | `-[:T*m..]->` | `-[:T]->{m,}` | `{m,}` | GQL only |
| exactly m | `-[:T*m]->`, `-[:T*m..m]->` | `-[:T]->{m}`, `-[:T]->{m,m}` | `{m,m}` | GQL only |
| m to n | `-[:T*m..n]->` | `-[:T]->{m,n}` | `{m,n}` | GQL only |
| 1 to n | `-[:T*..n]->` | `-[:T]->{1,n}` | `{1,n}` | GQL only |
| 0 to n | `-[:T*0..n]->` | `-[:T]->{,n}`, `-[:T]->{0,n}` | `{0,n}` | GQL only |

The same holds for `<-[...]-` and `-[...]-`. Quantified groups `((a)-[:T]->(b) WHERE p)q` take only the GQL quantifiers `+`, `*`
and `{…}` in grammar v1 (`group_pat`, [50 §2.3]), in every mode. One edge takes one quantifier: `-[:T*2]->{1,3}` is E114
([LQ/errors §5.2]). How `{1,1}` is encoded is [LQ/canonical-ast]'s (its open point C-19 keeps it a quantifier); the display
printer prints it as a plain hop, and the reading echo adds no suffix for it ([LQ/envelope §4.4]).

2.2. **Pattern predicates and property tests.**

| Construct | Cypher spelling | GQL spelling | Canonical | Strict GQL |
|---|---|---|---|---|
| a pattern exists | `WHERE (t)<-[:BLOCKS]-()`, `exists((a)-->(b))` | `EXISTS { (t)<-[:BLOCKS]-() }` | `EXISTS {…}` | GQL only |
| a pattern does not exist | `WHERE NOT (t)<-[:BLOCKS]-()` | `NOT EXISTS { … }` | `NOT EXISTS {…}` | GQL only |
| a pattern's count | `size((t)<-[:BLOCKS]-())` | `COUNT { … }` | `COUNT {…}` | GQL only |
| a property is present | `exists(n.p)` | `n.p IS NOT NULL` | `IS NOT NULL` | GQL only |

2.3. **Edge abbreviations.**

| Construct | Cypher spelling | GQL spelling (in grammar v1) | Canonical | Strict GQL |
|---|---|---|---|---|
| any-kind edge, stored direction | `(a)-->(b)` | `(a)-[]->(b)` | edge of any kind, forward | both accepted |
| any-kind edge, reverse | `(a)<--(b)` | `(a)<-[]-(b)` | edge of any kind, reverse | both accepted |
| any-kind edge, either direction | `(a)--(b)` | `(a)-[]-(b)` | edge of any kind, undirected | both accepted |

GQL's own abbreviations (`->`, `<-`, `-` alone) are not productions of grammar v1 ([50 §2.3] `edge_pat`); the full forms with an
empty `[]` are. [LQ/grammar-v1.ebnf §G.3] keeps the Cypher abbreviations in the strict mode (open point 7).

2.4. **Match modes** ([50 §2.8]: fixed parts bind distinct edges and quantified parts bind endpoint pairs, so no mode changes a
result).

| Written | Spelling | Canonical | Strict GQL |
|---|---|---|---|
| `MATCH DIFFERENT RELATIONSHIPS` | Cypher | no mode (silent) | accepted (the default; [LQ/grammar-v1.ebnf §G] does not refuse it) |
| `MATCH DIFFERENT EDGES` | GQL | no mode (silent) | accepted |
| `MATCH WALK`, `TRAIL`, `ACYCLIC`, `SIMPLE` | GQL | no mode; W02 | accepted; W02 |
| `REPEATABLE ELEMENTS` | both | — | E004 in every mode |

2.5. **Operators, clauses and functions.**

| Construct | Cypher spelling | GQL spelling | Canonical | Strict GQL |
|---|---|---|---|---|
| inequality | `!=` | `<>` | `<>` | GQL only |
| create a node or an edge (in `TX`) | `CREATE` | `INSERT` | `CREATE` | GQL only |
| explicit grouping | implicit (the non-aggregate items) | `GROUP BY` listing exactly the non-aggregate items | implicit | both accepted |
| bag result | `RETURN` | `RETURN`, `RETURN ALL` | bag | accepted |
| list collection | `collect(x)` | `collect_list(x)` | `collect` | GQL only |
| list size | `size(l)` | `cardinality(l)` | `size` | GQL only |
| lower and upper case | `toLower(s)`, `toUpper(s)` | `lower(s)`, `upper(s)` | `lower`, `upper` | GQL only |
| current time | `datetime()` with no argument | — (`now()` is LQ's) | `now()` | both accepted |
| priority value | `'P1'`, bare `P1` | `1` | the integer | all accepted (a coercion, not a spelling, [50 §3.2]) |

Both spellings of a function bind to one implementation ([50 §2.6]); the canonical column is the canonical name of
[LQ/canonical-ast §5.3] Table 5.3, which the display printer also prints (§5.4). `length()` is a third accepted spelling of
`size` there.

2.6. **Shared constructs** (one spelling in grammar v1; every mode accepts them): `MATCH`, `OPTIONAL MATCH`, `WHERE`,
`RETURN [DISTINCT]`, `ORDER BY`, `LIMIT`, `WITH`, `UNWIND`, `CALL … YIELD`, `UNION [ALL]`, `EXCEPT`, `INTERSECT`, `CASE`,
`IN`, `IS [NOT] NULL`, `STARTS WITH`, `ENDS WITH`, `CONTAINS`, `any/all/none(x IN l WHERE p)`, `count(*)`, the aggregates,
label tests `x:a|b`, label disjunction `(:a|b)`, property maps, node literals `#N` and `#u:…`, parameters `$p`, quantified
groups, `USE` and the revision grammar, every `TX` statement other than `CREATE`/`INSERT`, and the functions `labels()`,
`type()`, `id()`, `length()`, `trim()`, `substring()`, `coalesce()`, `round()`, `abs()`, `toString()`, `toInteger()`,
`toFloat()`, `date()`, `datetime('…')`, `duration('…')`. GQL spells some of these differently (`FOR` for `UNWIND`, `NEXT` or
`LET` for `WITH`, `IS LABELED` for a label test, `CAST` for the conversions), but those spellings are not productions of v1
([50 §2.3.1]) and are E004 with the LQ form in every mode ([LQ/errors §6]).

2.7. **Departures from Cypher's meaning** ([50 §2.8]'s third table, with the two cases the A1 review's S-21 adds). These are
semantics, not spellings: every mode and both display spellings share them, and LQ-Bench's adversarial stratum tags each
([50 §7.4] item 2).

| Departure | LQ | Cypher | How an agent finds out |
|---|---|---|---|
| absent values | `x <> v` is true, `x = v` false; absent sorts last | three-valued: null | W01; the card |
| division | `/` on two integers gives a float | `3/2 = 1` | the card; `toInteger(a / b)` |
| quantified parts | bind endpoint pairs | one row per path | N08 on any aggregate over one |
| hop bounds on cyclic patterns | `{m,n}` admits walks (a superset of Cypher's trail endpoints); equal on acyclic kinds | trails | EXPLAIN shows the strategy |
| a quantified part and a fixed edge of one `MATCH` (S-21) | the walk may reuse the edge a fixed part of the same `MATCH` bound: distinct-edge binding ([50 §3.4] item 2) covers the fixed parts only, because a quantified part binds no edge (item 4) | forbidden (no relationship twice in one match) | the card's counting line; the adversarial stratum |
| undirected and mixed-kind patterns (S-21; [50 §2.8] as amended treats every undirected pattern and every mixed-kind alternation as a cyclic case) | an undirected pattern (`-[:T]-`) is cyclic, and so can be an alternation of kinds that are each acyclic but not jointly (`-[:DEPENDS_ON\|CHILD_OF]->` over docs: no invariant forbids a parent section depending on its child), so walks can differ from trails there too, not only on `RELATES`, `CITES`, `MENTIONS`; `BLOCKS\|GATES\|CHILD_OF` stays acyclic by I5′ | trails | EXPLAIN; the adversarial stratum |
| `ORDER BY` with absent values | absent last in both directions | nulls first or last by direction | the card |

## 3. The strict-GQL spelling mode

3.1. **What it is.** A parser flag over grammar v1 that refuses the Cypher-only spellings marked "GQL only" in §2 and accepts
everything else; [LQ/grammar-v1.ebnf §G] is normative for its detection, and the "Strict GQL" column of §2 is the same list
(§G.2): the Cypher quantifier spellings, `!=`, pattern predicates, `exists(<path>)`, `size(<path>)`, `exists(x.p)`, `CREATE`,
`toLower(`, `toUpper(`, `collect(` and `size(`. It adds no production, no function and no semantics: a query accepted in both
modes has the same canonical form, hash and result ([50 §5.3]). It is the input surface of LQ-Bench's alternative surface "LQ
with strict GQL spellings" ([50 §8.2]), run as an ablation ([50 §7.4] item 6, [AR §7.7.5]: the alternative surfaces are
ablations, with no "within 5 points" gate).

3.2. **Who implements it.** The reference model's LQ-3 front end (PLAN WP-93a: "grammar v1 including `TX` and the strict-GQL
spelling mode"), selected by LQ-Bench's harness for that arm; the product need not ([LQ/grammar-v1.ebnf §G.1]; open point 1).
The default surface accepts both spellings.

3.3. **Refusals.** Each refused spelling is E004 in this mode, with the message `<form> is Cypher spelling; this surface takes
the GQL spelling` and the inline `write <the GQL form of §2>`, a mechanical replacement ([90 §8.1] L4). Everything E004 refuses in
the default mode stays refused.

3.4. **The card for that arm** is, as proposed here, the §3 card of [LQ/card] with every Cypher-only spelling replaced by its GQL
form (lines 10 and 27, as [LQ/card §4] gives them); no other line of the card uses a Cypher-only spelling. Which arm uses the
mode and with which card is WP-70/WP-72's to fix ([LQ/grammar-v1.ebnf] O-9).

## 4. The display spelling of quantifiers

4.1. **What it governs.** Only edge quantifiers, in everything moirai prints: the card, `--show-query`, `--show-tx`, the
replacement texts of errors, and the reading echo's display pattern ([90 §8.1] L1, [LQ/envelope §4.3]). Group quantifiers print in
the GQL form in both candidates (§2.1). The canonical form, its encoding, every query hash and the `.moi` query files are
unaffected: the display printer is separate from the canonical encoder ([50 §5.3]).

4.2. **The two candidates** (`HOLE(LQ-display-spelling)`), per canonical quantifier:

| Canonical | Cypher candidate | GQL candidate |
|---|---|---|
| `{1,}` | `*1..` | `+` |
| `{0,}` | `*0..` | `*` |
| `{m,}` | `*m..` | `{m,}` |
| `{m,m}`, m ≥ 2 | `*m` | `{m}` |
| `{m,n}` | `*m..n` | `{m,n}` |

In a pattern, the Cypher candidate prints the quantifier inside the brackets (`-[:BLOCKS*2..]->`) and the GQL candidate after
the arrow (`-[:BLOCKS]->{2,}`). In the reading echo's display pattern the quantifier follows the edge name directly
(`x BLOCKS*2.. #93`, `x BLOCKS{2,} #93`).

4.3. **The rule of choice** ([90 §8.1] L1, owner decision #38 (a)): the display-spelling ablation runs on Opus 5.5 alone; the
Cypher spelling, the prior GPT models share, is kept unless GQL is better by more than the ablation's run-to-run spread. [50 §7.2]'s
draft card printed the provisional GQL `->+`; [LQ/card §3] prints the Cypher candidate as the default.

## 5. The display printer

5.1. **Use.** It prints LQ text from a bound AST: `--show-query` and `--show-tx` bodies ([LQ/envelope §10.3]), the replacement texts
of errors ([LQ/errors §2.4]), and the column names of the table shape ([LQ/envelope §5.5]). It keeps the names the text used
(variables, parameters, aliases); the canonical form's renaming of variables applies to hashing only ([50 §5.3]). Its output is
ASCII apart from string-literal contents ([90 §8.1] L5).

5.2. **Layout.** Each clause starts a line, in source order: `USE`, `MATCH`, `OPTIONAL MATCH`, `WHERE` (its own line after a
`MATCH`, `OPTIONAL MATCH`, `CALL … YIELD` or `WITH`), `CALL … YIELD`, `UNWIND`, `WITH`, and `RETURN` together with its
`GROUP BY`, `ORDER BY` and `LIMIT`. A `WHERE` whose top-level expression is an `AND` of two or more conjuncts prints the first
conjunct after `WHERE ` and each further one on its own line as two spaces, `AND `, the conjunct. `UNION`, `UNION ALL`, `EXCEPT` and
`INTERSECT` stand on their own lines. A `DEFINE QUERY` prints its header up to `AS {` on the first line, the query indented two
spaces, and `}` on its own line; a parameter list longer than 100 bytes breaks after a comma, continuation lines aligned under the
first parameter. A `TX` prints `TX` and its options, ` {`, each statement on its own line indented two spaces with `;` ending every
statement but the last, then `}` and ` DRY` when present. Inside a `CASE`, `EXISTS {}` or `COUNT {}` everything prints on one
line. Lines end with LF; no line has trailing whitespace. This layout reproduces [50 §2.9]'s `--show-query` example of
`std.ready`. A stored text keeps its author's layout ([50 §4.4] F3; [LQ/std]'s texts are [50 §4.1]'s), so `--show-query` of
another definition may break lines differently from its source (open point 6).

5.3. **Tokens.** Keywords upper case; core kind names lower case and edge names upper case, as the schema spells them ([F08]);
reverse aliases print as written (the canonical rewrite is for the echo and the hash, [50 §3.2]). One space around binary
operators and after commas and colons in maps and named arguments; none inside `()` and `[]`; `{ ` and ` }` around the body of
`EXISTS`, `COUNT`, `TX` and `DEFINE`, and `{k: v}` for maps. Strings single-quoted with `'` and `\` escaped; integers decimal;
floats in the shortest round-trip form with a `.`; durations with their unit; node literals `#N`, or `#u:` and 32 hex digits for a
uid the store does not know ([50 §4.4]); parameters `$name`; revisions as written. Parentheses only where precedence requires them
([50 §2.3] section 4), plus those the source had around an `OR` operand of an `AND`.

5.4. **Spellings it chooses.** Quantifiers by §4; otherwise one fixed spelling per pair of §2: `EXISTS { … }`, `NOT EXISTS { … }`,
`COUNT { … }`, `IS NOT NULL`, `-[]->`, `<-[]-`, `-[]-`, `DIFFERENT EDGES` (never printed, as the default), `<>`, `CREATE`, implicit
grouping, and for functions the canonical names of [LQ/canonical-ast §5.3] Table 5.3 (`collect`, `size`, `lower`, `upper`,
`now()`), so a printed text names each function as its hash does (open point 3).

## Coverage

| Item | Where (and who covers the rest) |
|---|---|
| [90 §10.1] "LQ card", part "display spelling chosen by the L1 ablation", for every printed text | §4, Holes; the card's text: [LQ/card] |
| [60 §2.5] "Harness-agnostic interface" row, part "the card's display spelling" | §4 |

## Holes

| Id | What | Decided by | Candidates | Constraint the value must meet |
|---|---|---|---|---|
| HOLE(LQ-display-spelling) | the quantifier forms of §4.2 that every printed text uses: the card ([LQ/card] lines 10 and 27), `--show-query` and `--show-tx`, error replacement texts ([LQ/errors §2.4]) and the reading echo ([LQ/envelope §4.3]) | LQ-Bench display-spelling ablation, WP-72 ([90 §8.1] L1) | the Cypher and the GQL columns of §4.2 | Cypher kept unless GQL wins beyond the ablation's run-to-run spread on Opus 5.5; the canonical form, every hash and the `.moi` query files unchanged ([50 §5.3]); every error text stays ≤ 600 B |

## Open points for the review

1. **Is the strict-GQL mode a product feature?** It exists for the ablation. If GQL spellings win nothing, the product needs no
   mode; if the ablation favoured it, the freeze would decide whether moirai offers a stricter surface. Nothing in grammar v1
   depends on the answer, because the mode only removes alternatives.
2. **ISO GQL delimits identifiers with double quotes.** Grammar v1 reads `"…"` as a string in every mode ([50 §2.2] rule 9,
   [LQ/lexical §5.7]), and the strict mode keeps that; an agent that meant a delimited identifier gets a string instead. Agents
   rarely delimit identifiers, and double-quoted strings are common in Cypher habits, so no refusal is proposed; the review may
   add one to [LQ/grammar-v1.ebnf §G].
3. **Function names in printed text.** §5.4 prints [LQ/canonical-ast §5.3]'s canonical names, a mix of Cypher (`collect`, `size`)
   and GQL (`lower`, `upper`) spellings. [90 §8.1] L1 makes only quantifiers subject to the ablation, so the function names are a
   spec choice, not a hole.
4. **Cypher's `*..n` means 1 to n, GQL's `{,n}` 0 to n.** §2.1 keeps each spelling's own meaning, as [50 §2.8]'s rule requires.
   The two look alike to an agent; LQ-Bench's adversarial stratum may want a case for it.
5. **Grouping in the strict mode.** GQL's grouping is SQL-like, so a stricter mode would require `GROUP BY` when a `RETURN`
   mixes aggregate and non-aggregate items. [LQ/grammar-v1.ebnf §G.3] keeps implicit grouping in both modes, and this chapter
   follows it.
6. **Source layout versus printed layout.** The standard library's texts keep [50 §4.1]'s layout, and `--show-query` prints the
   display printer's. Writing every `std` text in the printer's layout would make the two byte-identical and simplify WP-71a's
   goldens; it changes no parse, hash or result.
7. **Reconciled with [LQ/grammar-v1.ebnf §G].** A stricter mode could also refuse the Cypher edge abbreviations
   `-->`/`<--`/`--` (their GQL forms in v1 are `-[]->`, `<-[]-`, `-[]-`), `DIFFERENT RELATIONSHIPS`, a no-argument
   `datetime()` and implicit grouping. §G keeps all four accepted, and the table follows §G so the model's front end has one
   rule; a stricter surface would be a change to §G.2's list, decided with WP-70/WP-72 (O-9).
8. **The display spelling and the strict mode are independent.** The display spelling governs what moirai prints to every agent;
   the strict mode governs what one LQ-Bench arm accepts. A GQL display spelling does not imply the strict mode, and the strict
   arm's card uses GQL quantifiers whatever the display spelling is.
9. **Hole id.** [F01 §2.5] makes hole ids `<part>-<name>`; this chapter's hole is `HOLE(LQ-display-spelling)`, and
   [LQ/canonical-ast]'s mention now uses the same id (review pass 1 S1-40).
