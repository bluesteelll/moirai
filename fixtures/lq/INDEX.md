# fixtures/lq: LQ conformance fixtures and goldens

| | |
|---|---|
| Title | The golden fixtures of the query surface: token and syntax-tree streams, error codes and positions, canonical-AST encodings and hashes, JSON IR documents, error texts and result envelopes |
| Work package | WP-22, part `lq/` (R-FIX; [PLAN §3.2] item 1). The part `gt10/` is separate and follows WP-14 |
| Acceptance | GT10; E1 (the owner verifies the core set of §5); E5 |
| Separation | S1 and S3 ([PLAN §3.1]): written from the specification text only. The author read no line of `moirai-format-oracle`, `moirai-model`, `moirai-toylog` or any product crate |
| Sources | `docs/spec/lq/` ([LQ/lexical], [LQ/grammar-v1.ebnf], [LQ/canonical-ast], [LQ/json-ir], [LQ/errors], [LQ/envelope], [LQ/std], [LQ/card], [LQ/gql-spelling]); [F19]; the design [50] for its code blocks, its conformance-fixture names and its ten mistakes |
| Status | Written against the specification after review pass 1 (owner answers of 2026-09-28); updated for spec sync 2b (`docs/spec/reviews/spec-sync-2b.md`): `%% input-json` (S2B-F-33), `ir-nid-zero` (S2B-F-44), `pe-list-in-without-where` and `pe-list-in-pipe` (S2B-F-39), `e401-example` (S2B-F-40), `header-staged` (S2B-F-26), and §4's statuses; updated for spec sync 3 (`docs/spec/reviews/spec-sync-3.md`): `std/pack_target.lq` and its case (S3-F-17), and the five new `std/` files `proposed`, `pack_rules_proposed`, `pack_rules_unmerged_proposed`, `pack_hazards_proposed` and `brief_proposed` with their catalog rows and cases (S3-F-17, S3-F-18). The query surface freezes after WP-72; a WP-73 remedy that changes a rule updates the fixtures it touches (commit subject `WP-73:` or `WP-22:`) |

These files are data. The consumers are the reference model's front end and evaluator (WP-93a, WP-93b: the token,
syntax-tree, canonical-AST, error and mistake cases), LQ-Bench's converter and reference renderer (WP-71a/b: JSON IR,
error texts and envelopes), and later the product's LQ-1/LQ-2 (M7) and CLI goldens (M8), which must reproduce them.

## 1. Files

| Path | Cases | What it asserts |
|---|---|---|
| `std/<name>.lq` (45 files) | — | The 45 `lq-define` blocks of [LQ/std], copied byte for byte ([LQ/std §2.6]: LF line ends, no trailing whitespace, one final LF). Parsed with start symbol `define_stmt` ([LQ/grammar-v1.ebnf §P.1]) |
| `std/catalog.txt` | 45 rows | Shape, budget class and cursor class of each file ([LQ/std §2.3]–§2.5, §3), for the catalog test of [LQ/std §9] |
| `cases/conformance.cases` | 47 | The 47 conformance fixtures of [50 §0.4] and [50 §11], re-authored (§3.1): 37 accepted inputs with token stream and S-AST, 10 refused inputs with the intended code and position |
| `cases/design-blocks.cases` | 44 | The 44 LQ code blocks of [50] ([50 §11], `assemble2.py`): each parses; token stream; S-AST and JSON IR where [LQ/json-ir §7] gives the tree |
| `cases/mistakes.cases` | 10 | The ten mistakes of [50 §2.9] with the frozen outcomes of [LQ/errors §7]: parse, outcome, warnings, reading echo |
| `cases/std.cases` | 53 | Each `std/*.lq` parses (token stream); the 8 `lq-tx` blocks of [LQ/std §7] parse (token stream, S-AST for six) |
| `cases/card.cases` | 8 | The card's 7 examples ([LQ/card §3]) and the GQL variant of example 2 (§4): parse, token stream, S-AST |
| `cases/lexical.cases` | 134 | Every token kind and limit, keyword and plain-name positions, revision mode, and every E001/E002/E003 case of [LQ/lexical] |
| `cases/parser.cases` | 146 | The parser decisions of Annex P (S-AST) and every refused form of Annex R (code and position) |
| `cases/strict-gql.cases` | 38 | The strict-GQL spelling mode ([LQ/grammar-v1.ebnf §G]): each Cypher-only spelling refused, the GQL spellings and shared constructs accepted |
| `cases/cast.cases` | 33 | Canonical ASTs ([LQ/canonical-ast §5]) with their binding context, binary encoding (§6), query hash (§7); the portable form of definitions (§8); E117 |
| `cases/json-ir.cases` | 25 | IR documents of [LQ/json-ir] as input (`%% input-json`): the S-AST each denotes, the output form (§8), the refusals of §5 |
| `cases/errors-text.cases` | 37 | Located error texts and their JSON envelopes ([LQ/errors §3.2], §4.1); unlocated error texts, warning and notice texts for stated situations (§3.3, §4.2–§4.3, §5) |
| `cases/envelope.cases` | 18 | Text and JSON renderings of stated results ([LQ/envelope §3]–§7); header and footer lines; `TX` result lines (§9); cursor bytes and text (§8); the target-set digest (§9.4) |

`.gitattributes` gives `fixtures/** -text`: every byte of these files is kept as written.

## 2. The case format

### 2.1 Files

A `.cases` file is UTF-8 text with LF line ends. It is a sequence of cases; every line outside a case is a comment (the
files start their comment lines with `#`). A case is:

```
%% case <id>
<directive lines and blocks>
%% end
```

`<id>` is unique within its file and matches `[a-z0-9][a-z0-9.-]*`. Every directive line starts with `%% `. A **line
directive** is `%% <name>` followed by one space and its value on the same line (or nothing). A **block directive** is
`%% <name>` alone on its line; the block is every following line up to the next line that starts with `%% `. An empty
block has no lines. The table of §2.2 fixes which names are blocks; every other directive is a line directive, and one
without a value (`accept`) stands alone on its line too. Line directives marked repeatable may occur several times, in
order; every other directive occurs at most once per case.

### 2.2 Directives

| Directive | Kind | Meaning |
|---|---|---|
| `source` | line, repeatable | The specification sections the case rests on |
| `note` | line, repeatable | Commentary. In the cases of `errors-text.cases` part 2 and `envelope.cases` the notes state the situation the golden renders |
| `entry` | line | The start symbol: `read` = `read_input` (`moirai q`, MCP `query`), `write` = `write_input` (`moirai tx`, MCP `write`), `define` = `define_stmt` (a standard-library or definition source, [LQ/grammar-v1.ebnf §P.1]). For a JSON IR document, the root tag |
| `mode` | line | `strict-gql` for the strict-GQL spelling mode ([LQ/grammar-v1.ebnf §G]); absent means the default mode |
| `display` | line | `cypher` or `gql`: the candidate of `HOLE(LQ-display-spelling)` ([LQ/gql-spelling §4]) that the expected text uses; absent where no printed text depends on it |
| `profile` | line | The model profile ([90 §8.2]) the reading echo is computed for: `gated`, `compatible` or `unknown` |
| `transport` | line | How the text arrived, which names the source of a located error ([LQ/errors §3.2]): `argv` (`<argv>`), `stdin` (`<stdin>`), `query` (an MCP string, `<query>`), `file <name>` (the file's base name) |
| `context` | line, repeatable | The binding and call context, one item per line: `schema core` (the core schema of [F08]); `node <N> <uid>`; `commit <seq> <64 hex>`; `param <name> <type> <value>` ([LQ/canonical-ast §9]); `branch <ref>` and `rev <seq>` (the caller's resolved branch and tip, for JSON error envelopes) |
| `input` | block | The LQ query text: the block's lines, each followed by LF, without the last LF. It holds no CR and no line starting with `%% ` |
| `input-json` | block | A JSON IR document ([LQ/json-ir]) in the same line form as `input`; a runner tells it from LQ text by this directive, not by the file name or the `entry` value ([LQ/canonical-ast §9]; spec sync 2b, S2B-F-33). Every case of `json-ir.cases` uses it |
| `input-hex` | block | The query bytes in hexadecimal. Whitespace between digits is ignored; `;` starts a comment that runs to the end of the line. Used for byte-exact inputs (byte-order marks, CR, controls, ill-formed UTF-8) |
| `input-file` | line | The query is the whole file at this path, relative to `fixtures/lq/` |
| `tokens` | block | The classified token stream of the successful parse ([LQ/lexical §11]), one token per line, ending with `EOF -`. Implies that the input parses |
| `sast` | block | The S-AST in the S-expression form of [LQ/canonical-ast §4.1]–§4.2. Implies that the input parses |
| `sast-same-as` | line | The S-AST equals the S-AST of the named case: `<id>` in the same file, or `<file>:<id>` |
| `cast` | block | The canonical AST in the text form of [LQ/canonical-ast §4.3], bound in the `context` given |
| `cast-same-as` | line | The C-AST (hence its encoding and hash) equals that of the named case of the same file |
| `encoding` | block | The C-AST's binary encoding ([LQ/canonical-ast §6]) in hexadecimal, as for `input-hex`. A case holds at most one of `encoding` and `hex` ([LQ/canonical-ast §9]) |
| `hash` | line | `<algorithm> <a>..<b> [first <n>] = <hex>`: the named hash over bytes `a` to `b` (exclusive) of the case's one byte block (`encoding` or `hex`), truncated to its first `n` bytes; a query hash only over `encoding`. `blake3_256 0..174 first 16` is the query hash H of [LQ/canonical-ast §7.1], the value an `xtask hex` directive `{blake3_256 0..174}` gives before truncation. `xxh3_64 a..b = 0x<16 hex>` is the XXH3-64 value (seed 0) as a number |
| `explain-id` | line | The EXPLAIN id `q:` + the first 8 hex digits of H ([LQ/canonical-ast §7.2]) |
| `cursor-query-hash` | line | H[0..8] read as a little-endian u64, written `0x` + 16 hex digits ([LQ/canonical-ast §7.2]) |
| `portable` | block | The stored portable text of a definition ([LQ/canonical-ast §8.1]), each line followed by LF except the last |
| `json-ir` | block, one line | The JSON IR output form of the S-AST ([LQ/json-ir §8]), byte for byte |
| `accept` | line, no value | The input parses; nothing else is asserted about the tree |
| `error` | line | The first error of the pass ([LQ/grammar-v1.ebnf §P.14]): `<code> <line>:<col> <basis>`, `<code> ptr <JSON pointer> <basis>` for a JSON IR document ([LQ/json-ir §5.3]), or `<code> *` when only the code is asserted. `line` and `col` are 1-based, `col` in Unicode scalar values ([LQ/lexical §2.3]); `basis` is `spec` when the specification fixes the position and `conv` when it does not and §4 G-7's convention gives it |
| `outcome` | line | `runs`: the input parses, binds and evaluates without error on a store as the header of the file describes; `error`: see `error` |
| `warnings`, `notices` | line | The exact set of warning (notice) codes the result carries, space-separated, or `none` |
| `reads` | block | The exact reading-echo lines ([LQ/envelope §4]); an empty block means none |
| `nodes` | block | Store facts for a rendering golden: one node object of [LQ/envelope §7.4] per line |
| `text` | block | The expected text: the block's lines, each followed by LF |
| `json` | block, one line | The expected JSON (an envelope, or a warning or notice object), byte for byte, followed by LF where it is printed |
| `hex` | block | Expected bytes, as for `input-hex`; never with `encoding` in one case |

### 2.3 How a harness uses the cases

- **Token streams** are the classified stream of a successful parse: a harness needs the parser to report, for each
  word, whether it used it as a keyword or as a name ([LQ/lexical §11]). A case with `tokens` also asserts that the
  parse succeeds.
- **S-expressions** compare by their token sequences, whitespace ignored ([LQ/canonical-ast §4.1]).
- **Errors.** A harness asserts the code of the first error and, for a `spec` position, its line and column. A mismatch
  on a `conv` position is triaged against §4 G-7 before it is counted as a defect; [LQ/errors §3.1] adopts G-7's
  positions (spec sync 2b, S2B-F-31), so a `conv` position of a construct that §3.1 names is normative, and such marks
  may be re-marked `spec`.
- **Outcomes** of `mistakes.cases` need a store with the core schema in which the named nodes exist (tasks `#51`, `#88`,
  `#93` and a doc `#130`); the `gt10/` walk-through store has nodes of these kinds under other numbers, so a harness
  creates the four ids itself. The warnings and reading-echo lines do not depend on the store's contents; notices that
  do (N07 on an empty result) are stated as notes.
- **Error texts** (`errors-text.cases`): part 1 cases are driven end to end (input, transport, context → text and JSON);
  part 2 cases are renderer tests: the test builds the stated situation and compares the text.
- **Envelopes** (`envelope.cases`): the renderer builds the stated result (its notes, its `nodes` block and the JSON's
  `data`) and compares both renderings. The values inside the JSON `budget` object are inputs, not derived; a harness
  compares every other byte and checks the `budget` keys and their order (§4 G-12).
- **Hashes** can be checked with any BLAKE3 or XXH3-64 implementation over the `encoding` or `hex` block.

## 3. Coverage

### 3.1 The WP-22 `lq/` row

| Item ([PLAN §3.2] WP-22) | Where |
|---|---|
| The 47 conformance fixtures, re-authored as token/AST streams or error codes | `conformance.cases`. [50]'s probe fixtures (`fixtures2.py`) are unpublished ([LQ/grammar-v1.ebnf] O-14), so the 47 are re-authored from what [50 §0.4] and §11 say they cover (ranges, `s1`, pattern predicates, `#u:`, aliases, the intended error codes), keeping every name [50] and [LQ/lexical] cite: `var-s1`, `prop-s2`, `var-hexlike`, `range-2dot`, `range-3dot`, `paren-arith`, `undirected-minus`, `pattern-predicate`, `exists-fn-pattern`, `size-fn-pattern`, `use-in-exists` |
| The 44 [50] code blocks | `design-blocks.cases`: the block of [50 §0.2], the 35 blocks of Q1–Q29 ([50 §2.9]; Q17 is a JSON string, not an LQ block) and the 8 `DEFINE` statements of [50 §4.1] (7) and §4.3 (1), each parsed on its own as `assemble2.py` did |
| The ten §2.9 mistakes | `mistakes.cases` |
| Error goldens | `errors-text.cases`; codes and positions in `lexical.cases`, `parser.cases`, `strict-gql.cases`, `json-ir.cases`, `cast.cases` (E117) |
| Envelope goldens | `envelope.cases` |

### 3.2 Specification sections

| Section | Cases |
|---|---|
| [LQ/lexical] §2 (encoding, BOM, positions) | `lexical.cases` `lx-bom`, `lx-crlf-and-comments`, `lx-e003-utf8-*`, `lx-e001-second-bom` |
| [LQ/lexical] §3 (whitespace, comments) | `lexical.cases` `lx-comments-*`, `lx-comment-markers-in-text`, `lx-e002-block-comment` |
| [LQ/lexical] §4, §7 (revision positions and mode) | `lexical.cases` `lx-rev-*`, `lx-e001-*` revision cases, `lx-e003-rev-*` and date cases; `conformance.cases` `range-*`, `use-*`, `commit-literal`, `short-hex-is-ref`, `head-suffix`, `rev-*`, `tx-options` |
| [LQ/lexical] §5 (tokens, limits) | `lexical.cases`; `conformance.cases` `numbers`, `strings`, `lt-minus`, `uid-literal`, `parameters`, `backquoted-names` |
| [LQ/lexical] §6, §9 (keywords, plain names, case) | `lexical.cases` `lx-keyword-case`, `lx-contextual-keywords-as-names`, `lx-plain-name-positions`, `lx-refused-words-as-names`, `lx-named-argument-reserved-word`; `conformance.cases` `keyword-case` |
| [LQ/lexical] §11 (token-stream format) | every `tokens` block; the three worked examples of §11.2 are `conformance.cases` `range-2dot` and `use-datetime` and `card.cases` `card-7`, byte for byte |
| [LQ/grammar-v1.ebnf] Annex P | `parser.cases` `pp-*`; `conformance.cases` |
| [LQ/grammar-v1.ebnf] Annex R | `parser.cases` `pe-*`; `conformance.cases` refused cases |
| [LQ/grammar-v1.ebnf] Annex G, [LQ/gql-spelling] §2–§3 | `strict-gql.cases`; `card.cases` |
| [LQ/canonical-ast] §3–§4 (S-AST) | every `sast` block |
| [LQ/canonical-ast] §5–§8 (C-AST, encoding, hashes, portable form) | `cast.cases` |
| [LQ/json-ir] | `json-ir.cases`; `json-ir` blocks in `conformance.cases` and `design-blocks.cases` |
| [LQ/errors] §3–§5 | `errors-text.cases` |
| [LQ/errors] §6 (E004 rows) | `parser.cases` `pe-*` (one case per row), `strict-gql.cases` |
| [LQ/errors] §7 (the ten mistakes) | `mistakes.cases` |
| [LQ/envelope] §3–§9, [F19] §2–§4 | `envelope.cases` |
| [LQ/std] | `std/*.lq`, `std/catalog.txt`, `std.cases` |
| [LQ/card] §3–§6 | `card.cases`; the body's sizes (3,116 / 3,110 / 2,867 / 2,394 bytes, 36 lines) were checked against §3, §4 and §6.2 and agree |

### 3.3 Rows of `docs/spec/COVERAGE.md`

The fixture column of these rows can cite (R-SPEC fills the column):

| Row | Fixture |
|---|---|
| F1 | `fixtures/lq/cases/cast.cases` (`reverse-alias`, `symmetric-kind`, `alternation-keeps-both`); `fixtures/lq/cases/envelope.cases` (`result-table-echo-*`) |
| F2 | `fixtures/lq/cases/cast.cases` (`s44-bare-words`, `define-coerced-constants`) |
| F3 | `fixtures/lq/cases/cast.cases` (`define-portable`, `define-coerced-constants`, `define-*-e117`); `fixtures/lq/std/*.lq` |
| F10, 60-AR-CommitBody (`idem_payload`) | `fixtures/lq/cases/cast.cases` (`tx-options-dropped`, `tx-bare`, `tx-options-kept`) |
| X-F9 (LQ `ref_word`) | `fixtures/lq/cases/lexical.cases` (`lx-rev-*`) |
| X-F12 (T6: BOM, invalid UTF-8, W09) | `fixtures/lq/cases/lexical.cases` (`lx-bom`, `lx-e003-utf8-*`); `fixtures/lq/cases/errors-text.cases` (`w09`) |
| R-16 (LQ's `none`, `files: no tree bound`) | `fixtures/lq/cases/errors-text.cases` (`w10`); `fixtures/lq/cases/envelope.cases` (`header-lines`) |
| 90-Errors, 60-AU-Harness (error code, exit-5 texts, L4 rewrites) | `fixtures/lq/cases/errors-text.cases` (`e411-no-match`, `e407-declared-agent`, `e406-unleased`, the inline rewrites of `e113-path-variable`, `e118-eq-null`, `e001-where-after-return`) |
| 90-Output (header limits, ASCII) | `fixtures/lq/cases/envelope.cases` (`header-lines`, `header-staged`) |
| 90-Card | `fixtures/lq/cases/card.cases` |

## 4. Specification findings and gaps

Found while authoring and filed with the review. A **finding** (F) is a contradiction or an error in the
specification; the fixtures follow the reading stated here until it is resolved. A **gap** (G) is something the
specification does not decide; the fixtures either leave it unasserted or use the convention stated. The last column
gives each one's status after spec sync 2b (`docs/spec/reviews/spec-sync-2b.md`; F-1 and F-2 by spec sync 2a).

| # | Where | Finding | Fixtures | Status |
|---|---|---|---|---|
| F-1 | [LQ/lexical §7.2] `HEAD` row | `HEAD` must not be followed by `.`, which refuses `HEAD..x` and `HEAD...main`: [LQ/std §5.4] `pack_rules_unmerged` and [50 §4.3] use `diff(HEAD...main)`, and [LQ/std §1.1] and [50 §11] say every block parses. Proposed: `.` stops `HEAD` only when a `ref_word` follows it, as for ref names | assert the parse: `design-blocks.cases` `s4.3-pack-rules-unmerged`, `std.cases` `std-pack-rules-unmerged` | settled by spec sync 2a ([LQ/lexical] L-15: `HEAD..main` and `HEAD...main` are ranges) |
| F-2 | [LQ/lexical §6.1] | "These 43 words" lists 44 reserved words | the 44 are reserved | settled by spec sync 2a ([LQ/lexical] L-15: 44 reserved words) |
| F-3 | [LQ/lexical §11] `KW` row and the paragraph after it | `exists` is listed among the keyword forms of §P.9, as if `exists(e)` without a path were a `NAME`, but `EXISTS` is a reserved word | `exists(` is `KW EXISTS` in every form (`conformance.cases` `exists-fn-property`) | settled by S2B-F-38 (`exists` is always `KW`) |
| F-4 | [LQ/grammar-v1.ebnf §R] list comprehensions | "`'[' word IN`" also refuses the list literal `[x IN l]` (a one-element list of a membership test) that `list_lit` derives; the rewrite `[(x IN l)]` is not stated | `parser.cases` `pe-list-in-without-where` now parses (tokens, S-AST); `pe-list-comprehension` (`WHERE`) and `pe-list-in-pipe` (`\|`) expect E004 | settled by S2B-F-39 (the refusal needs `WHERE` or `\|` at the list's top level; `[x IN l]` is a list literal) |
| F-5 | [LQ/errors §3.3] vs §5.5 E401 | The example's help back-quotes the command (``re-read with `moirai q show ids=89` ``); the row's template does not | `errors-text.cases` `e401-example`: no back-quotes | settled by S2B-F-40 (the example's help has no back-quotes) |
| F-6 | [LQ/errors §5.5] preamble vs rows | The preamble puts `nothing was written` in every E4xx text and names the statement index; the rows of E406 (unleased, safelist, MCP), E407, E408 and E411 give other or no detail lines and no index | `e406-unleased`, `e407-declared-agent`, `e411-no-match` follow the rows | settled by S2B-F-41 (statement refusals name the statement and close with `nothing was written`; block and call refusals do neither); the three texts follow it |
| F-7 | [LQ/errors §2.2] | `<kind>`, `<field>`, `<edge>`, `<fn>`, `<var>`, `<param>` render inside back-quotes, which conflicts with templates that embed them in LQ text (N07's `(<b>)-[:<edge>]->(<a>)`, W01's `coalesce(<expr>, 0)`, E106's inline) and with §3.2's E101 example (`task fields: ...`) | those texts are not goldened | settled by S2B-F-42 ([LQ/errors §2.2a]: a placeholder inside embedded LQ text renders as LQ source); those texts may now be goldened |
| F-8 | [LQ/grammar-v1.ebnf §P.12] vs [LQ/errors §5.1] | P12 raises E109 at parse time for a `CALL` of a non-`tx` name inside `TX`; the table lists E109 as the binder's only | `parser.cases` `pe-tx-call-non-tx` expects E109 | settled by S2B-F-43 (E109 is raised by the parser and the binder) |
| F-9 | [F19 §4.2] base part ≤ 60 B | A staged header cannot fit: `branch: merge/a/from/b \| rev 4481 \| staged (read-only) \| 1 row` is 62 B, [50 §2.9] Q16's is 72 B | `envelope.cases` `header-staged` (72 B) is limit-checked | closed by S2B-F-26 (a staging view's base part may take 80 B) |
| F-10 | [LQ/json-ir §5.1] steps 2 and 3 | The schema of §6 carries the `nid` and `int` ranges, so step 2 refuses them with E001 before step 3's E003 is reached; E003 remains for `float` and `dur` | `json-ir.cases` `ir-nid-zero`, `ir-float-infinite`, `ir-duration-overflow` expect E003 at the value's pointer | settled by S2B-F-44 (the schema states no numeric range of `nid` and `int`; step 3 refuses with E003) |

| # | Gap | Fixtures | Status |
|---|---|---|---|
| G-1 | The order of the expected-token list of a generic E001 (`expected <list>, found <token>`) | no generic E001 text is goldened | open |
| G-2 | Whether the JSON error envelope of a lexer or parser error carries the caller's branch and tip or `null` ([F19 §8.2]: `null` "before a ref was resolved") | `errors-text.cases` assumes they are resolved first and gives them as `context` | open |
| G-3 | E401's JSON `current` objects: which node-object keys, and where `changed_by` goes | E401 has a text golden only | settled by S2B-F-47 ([LQ/errors §5.7]: `current` holds `{id, kind, status, title, rev, changed_by}` per detail line); a JSON golden may be added |
| G-4 | `TX` results in JSON: where "after `commit`" puts the keys of [LQ/envelope §7.7] relative to `view`; the `diff` rows of `data` | `TX` results have text goldens only | open |
| G-5 | The layout of a `DRY` body: how target lines, mutation lines and `diff` rows interleave ([LQ/envelope §9.3]) | only the `DRY` header and closing lines are goldened | open |
| G-6 | Renderings of W02's `<mode>`, N04's `<datetime>`, N02's `<key>`, E003's `<token>`, `<char>` and `U+<hex>` | not goldened | open |
| G-7 | Positions of errors whose detection point the specification does not name, and the span of binder errors. **Convention (`conv`)**: the first token or character of the construct the rule refuses — for a refused keyword form its first word; for an operator the operator; for a literal out of range its first character; in revision mode the revision's base for an upper-case letter or a run-in, the suffix for a count or a datetime out of range; for E118 the comparison operator or the property-map key; for E007 the token where `EXPECT` was expected; for E009 the `}`; for E114 the quantifier's first character; for a binder error, no position (`*`); for a JSON IR error, the offending node | every `conv` position | adopted by [LQ/errors §3.1] (S2B-F-31): the positions it names there are normative (§2.3) |
| G-8 | Placeholders not in [LQ/errors §2.2]'s table: `<agent>`, `<identity>`, `<actor>`, `<src>`, `<dst>`, `<key>` | `e407-declared-agent` renders `<agent>` like `<holder>`; the others are not goldened | open |
| G-9 | How a value is printed inside a `write …` rewrite (E102's `write <value> IN <v>.labels`): as LQ text or back-quoted | E102 is asserted by code only | open |
| G-10 | The `expected` array of a special-case E001 (and of a lexer E001) | E001 cases have text goldens without JSON | open |
| G-11 | The JSON rows of the node-with-extras shape: a node column is `"#N"` by [LQ/envelope §7.3]–§7.4, so the facts its node lines print are not in the envelope | `envelope.cases` `result-node-with-extras` gives them in a `nodes` block | open |
| G-12 | What `budget.bytes` and `budget.mem` count | budget values are not asserted | open |
| G-13 | The text of a zero duration ("the largest exact unit") | not goldened | open |
| G-14 | Whether the `--ids` continuation (`ids: … \| <rerun>`) repeats `--ids` | not goldened | open |

## 5. The core set for owner verification (E1)

GT10 has the owner verify a core set; the rest was written by an author who saw neither engine nor model code (S3). The
proposed core set:

1. `cases/conformance.cases` (47 cases): what LQ text means, token by token and tree by tree.
2. `cases/mistakes.cases` (10 cases): the outcomes [LQ/errors §7] freezes.
3. `cases/cast.cases` cases `s44-quoted`, `s44-bare-words` and `s44-parameters`: the canonical form and hash of
   [LQ/canonical-ast §4.4] and §6.5 (the 174 bytes are reproduced byte for byte).
4. `cases/errors-text.cases` cases `e406-unleased`, `e407-declared-agent`, `e411-no-match`: the refusal texts of
   [90 §10.1].

## 6. How these files were made and are maintained

- Every expectation was written from the specification text. Token streams, line and column positions and hex
  encodings were cross-checked with private scratch tools written from [LQ/lexical] and [LQ/canonical-ast §6]
  (not committed, sharing no code with any crate); the scratch encoder reproduces [LQ/canonical-ast §6.5]'s 174 bytes.
  Hash values were computed with the `blake3` and `xxhash-rust` crates of `Cargo.lock`.
- Every S-AST was checked against the node catalogue of [LQ/canonical-ast §3.2]–§3.3 (field count, field types, union
  membership) before it was written here.
- A change to a rule these files assert is a specification change first (R-SPEC), then an edit here by R-FIX (WP-73).
  A disagreement between an implementation and a case is triaged against the specification: a `spec` expectation that
  the specification supports is a defect of the implementation; anything else is a finding for the review.
- The files never contain owner-derived data: every id, title, ref and commit is synthetic or taken from the design's
  fictional campaign ([AR §7.6], [50 §2.9]).
