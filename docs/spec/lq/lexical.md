# LQ lexical rules (grammar version 1)

| | |
|---|---|
| Title | LQ lexical rules: source decoding, whitespace and comments, the normal and revision lexer modes, every token with its limits and value, keyword sets, name matching, the transport and stored-text checks, and the token-stream fixture format |
| Chapter | [LQ/lexical], `docs/spec/lq/lexical.md` |
| Status | draft, pass 1 pending |
| Work package | WP-19a (R-SPEC-F), part of WP-19 ([PLAN §3.2] item 1) |
| Sources | [50 §2.2] rules 1–10; [50 §2.3] section 7 and the parser notes; [50 §2.4] revision forms; [50 §3.2] (coercion is the binder's, not the lexer's); [50 §4.4] portable text; [50 §5.2] E001–E003 and the positions of diagnostics; [50 §6.1] `-f`/stdin; [50 §6.2] rules 2, 4, 6; [80 §3] X-F9, X-F12; [80 §4] T1, T3, T5, T6; [AR §7.7.1]; [AR §7.7.3]; the A1 review's S-02 and S-11 (`docs/spec/reviews/a1-S.md`) |
| Depends on | [LQ/grammar-v1.ebnf] (productions; parser decisions, Annex P; refused forms, Annex R), [LQ/canonical-ast] (the tree built from these tokens; the portable form, §8), [LQ/errors] (texts of the codes named here), [LQ/std] (relation signatures, §2.9), [F01] (UTF-8, hex text, the hash set) |

This chapter fixes how LQ source text becomes tokens: the accepted bytes, the two lexer modes, every token kind with its
limits and value, the keyword sets, the name-matching rules, the transport and stored-text checks, and the token-stream
format that fixtures use. It is normative for the lexical productions that [LQ/grammar-v1.ebnf §7] summarises
([LQ/grammar-v1.ebnf §0.4]).

## 1. Scope and codes

The lexer raises three codes of [50 §5.2]; their texts are [LQ/errors]'s:

| Code | Name | Raised for |
|---|---|---|
| E001 | `syntax` | a character that starts no token (§5.2); `$` without a name; `!` without `=`; an empty back-quoted identifier; a malformed revision position (§7) |
| E002 | `unterminated` | a string, back-quoted identifier or block comment that is not closed (§3, §5.4, §5.7) |
| E003 | `bad_literal` | ill-formed UTF-8 (§2.1); a number, duration, node literal, uid, escape, count, commit prefix or datetime outside its form or range; a raw control character inside a string or back-quoted identifier |

Every lexical error ends the pass: the parser's recovery ([LQ/grammar-v1.ebnf §P.14]) does not resume inside a token.

## 2. Source text

### 2.1 Encoding

1. The source is a byte string. If it starts with the UTF-8 byte-order mark `EF BB BF`, those three bytes are removed,
   once; a second mark is the character U+FEFF, which §5.2 refuses outside strings ([50 §2.2] rule 1: PowerShell pipes
   add one mark).
2. The remaining bytes must be well-formed UTF-8 (RFC 3629): no overlong form, no encoded surrogate (U+D800–U+DFFF),
   nothing above U+10FFFF, no truncated sequence. The first ill-formed byte is E003 at that byte's offset. After this
   check the source is a sequence of Unicode scalar values.
3. An MCP string (`q`, `tx`) arrives as Unicode; it is encoded as UTF-8 and then treated exactly as bytes from stdin,
   including rule 1 for a leading U+FEFF.

### 2.2 Line ends

A line end is LF (0x0A) or the pair CR LF (0x0D 0x0A). A CR that is not followed by LF is whitespace and ends no line.

### 2.3 Positions

| Field | Meaning |
|---|---|
| `start`, `end` | 0-based byte offsets into the source after the mark of §2.1 rule 1 is removed; `end` is exclusive |
| `line` | 1-based: the number of line ends before `start`, plus 1 |
| `col` | 1-based: the number of Unicode scalar values between the start of the line and `start`, plus 1 |

The text form of a diagnostic prints `line:col`; the JSON form carries all four ([50 §5.2]). For E003 on ill-formed
UTF-8, `line` and `col` are computed over the well-formed prefix.

## 3. Whitespace and comments

- **Whitespace** is SP (U+0020), HT (U+0009), LF and CR. No other character is whitespace: U+00A0, U+3000, VT and FF
  are refused by §5.2.
- **Line comment**: `//` up to the next LF (not included) or the end of the input.
- **Block comment**: `/*` up to the first following `*/`. Comments do not nest: `/* a /* b */` ends at the first `*/`.
  A block comment not closed before the end of the input is E002 at its `/*`.
- Comments are whitespace. They may stand between any two tokens of normal mode and before a revision; they may not
  stand inside a revision (§7.6).
- `--` is not a comment: it is the undirected edge `(a)--(b)` ([50 §2.2] rule 2).
- `//` and `/*` inside a string or a back-quoted identifier are content, not comments.

## 4. Modes

### 4.1 Two modes

The lexer has a **normal mode** (§5) and a **revision mode** (§7). It starts in normal mode. The parser switches it to
revision mode at the positions of §4.2 and back to normal mode after one `revspec` or `rev_arg`
([LQ/grammar-v1.ebnf §P.2]). Outside those positions there are no revision literals: `s1`, `t.s2`, `cafebabe` and
`c9b2e6c1` are words ([50 §2.2] rules 4 and 6; fixtures `var-s1`, `prop-s2`, `var-hexlike`), and a revision-typed
property is compared with an integer, a bare word or a string that the binder coerces ([50 §3.2]).

### 4.2 Revision positions

| Position | Production read | Notes |
|---|---|---|
| after `USE` | `revspec` | a quote here is E001 (hint: write the revision unquoted) |
| after `ON` in a `tx_option` | `revspec` | as above |
| after `IF TIP` | `revspec` | as above |
| argument 0, or the named argument `range`, of `CALL diff(…)` and `CALL log(…)` | `rev_arg` | |
| the named arguments `since` and `ref` of `CALL changes(…)` | `rev_arg` | `ref` is Open point L-6 |
| the named argument `in` of `CALL history(…)` | `rev_arg` | `in` is a plain name there (§6.3) |
| the named argument `refs` of `CALL across(…)` | `rev_arg` | usually the list form `[main, lane/x]` |
| argument 0, or the named argument `ref`, of `CALL violations(…)` | `rev_arg` | Open point O-4 of [LQ/grammar-v1.ebnf] |

- The relation name is matched when `proc_name` is a single segment equal, ASCII-case-insensitively, to one of the six
  names. `std.diff` and project named queries are not revision positions: their revision-typed parameters take a
  `$param` or a quoted revspec that the binder coerces ([50 §4.4] Invocation).
- Named-argument names are matched exactly (lower case), as parameter names are case-sensitive
  ([LQ/grammar-v1.ebnf §P.10]).
- In an argument position, if the first byte after whitespace and comments is `'` or `"`, the parser reads an ordinary
  `expr` instead of a `rev_arg` (a string the binder coerces to a revision, [50 §3.2]); if it is `$`, the parameter is
  lexed as in §5.5.
- Both call forms count: `call_clause` and `standalone_call`. A revision relation written as a scalar function (not
  after `CALL`) is lexed in normal mode and refused by the binder (E109).

## 5. Normal mode

### 5.1 Token kinds

| Kind | Form | Value |
|---|---|---|
| word | `[A-Za-z_][A-Za-z0-9_]*` | the bytes; classified as a keyword or a name by the parser (§6) |
| back-quoted identifier | `` `…` `` | the decoded name (§5.4) |
| parameter | `$` + word | the word, without `$` (§5.5) |
| integer | digits | a value in 0 … 9223372036854775807 (§5.6) |
| float | §5.6 | the source text; the binary64 value is derived (§5.6) |
| duration | digits + one of `s m h d w` | the source text; the value in milliseconds is derived (§5.6) |
| string | `'…'` or `"…"` | the decoded value (§5.7) |
| node literal | `#` + digits | N in 1 … 4294967295 (§5.8) |
| uid literal | `#u:` + 32 hex | the 32 lower-case hex digits (§5.8) |
| punctuation | §5.9 | the bytes |
| end | end of input | — |

The lexer never turns a word into a number, a revision or a literal of another kind. `TRUE`, `FALSE` and `NULL` are
reserved words (§6.1), which the grammar's `literal` production admits.

### 5.2 Characters

- **Outside** strings, back-quoted identifiers and comments, only these characters may occur: ASCII letters, digits,
  `_`, the punctuation bytes of §5.9, `$`, `#`, `` ` ``, `'`, `"`, and whitespace. Every other scalar — any non-ASCII
  character, a C0 control other than HT, LF and CR, DEL (U+007F), and the ASCII characters `!` (unless in `!=`), `&`,
  `\`, `^`, `~` (unless in `=~`), `@` — is E001 at its position. `^`, `~` and `@` are revision-mode characters (§7.3); in normal mode the
  E001 text names the revision positions.
- **Inside** strings and back-quoted identifiers, every scalar value is allowed except: the closing delimiter and `\`
  (which §5.4 and §5.7 give their meaning); LF and CR, which are E002 (the literal is unterminated on its line); and the
  controls U+0000–U+0008, U+000B, U+000C, U+000E–U+001F and U+007F, which are E003 at their position (write a string's
  control characters with `\u{…}`). HT is allowed.

### 5.3 Words

A word is the longest run `[A-Za-z_][A-Za-z0-9_]*`. It is ASCII only; a name with any other character must be
back-quoted. A word is a reserved word (§6.1), a contextual keyword where the grammar admits that keyword at that point
(§6.2), or a name. A digit never starts a word (§5.6).

### 5.4 Back-quoted identifiers

`` ` `` starts a back-quoted identifier, which ends at the next `` ` `` that is not followed by another `` ` ``. Inside,
the pair ` `` ` stands for one `` ` ``. The value is the content with each pair reduced; it may be any text allowed by
§5.2 and may equal a keyword (`` `match` `` is a name). An empty back-quoted identifier (` `` ` followed by a
non-back-quote) is E001. A back-quoted identifier that is not closed before a line end or the end of the input is E002
at its opening back-quote. A back-quoted identifier is never a keyword.

### 5.5 Parameters

`$` followed by a word is a parameter; its name is the word, case-sensitive ([50 §2.2] rule 7). `$` followed by
anything else (a digit, a back-quote, whitespace, the end) is E001 (hint: parameter names start with a letter or `_`).
`:name` is not a parameter ([50 §2.2] rule 7); the parser refuses it where an expression is expected
([LQ/grammar-v1.ebnf §R]).

### 5.6 Numbers and durations

At a digit the lexer reads, in this order:

1. the digits `D1`;
2. if the next byte is `.` and the byte after it is a digit: `.`, the digits `D2`, then an optional exponent (step 3) —
   a **float**;
3. else, if the next byte is `e` or `E` followed by a digit, or by `+` or `-` and a digit: the exponent
   `(e|E)[+|-]digits` — a **float**;
4. else, if the next byte is one of `s`, `m`, `h`, `d`, `w` (lower case) and the byte after it is not a letter, digit or
   `_`: the unit — a **duration**;
5. else an **integer**.

After the token, a letter, digit or `_` directly following is E003 at the token's start (`12abc`, `3days`, `15M`,
`1.5x`, `0x10`, `2e`). An exponent marker without digits (`1e`, `1e+`) is E003.

Consequences: `1..3` is integer `..` integer; `1.` is integer `.`; `.5` is `.` integer (E001 later); `-1` is the
operator `-` and the integer 1 (there is no negative literal; `unary_expr` applies the sign).

| Kind | Value | Range (E003 outside) |
|---|---|---|
| integer | decimal; leading zeros are allowed and ignored | 0 … 9223372036854775807 |
| float | the IEEE 754 binary64 value nearest to the decimal value, ties to even (the correctly rounded conversion) | finite; a value that rounds to an infinity is E003; a value that rounds to zero is 0.0 |
| duration | the integer times the unit in milliseconds: `s` 1,000; `m` 60,000; `h` 3,600,000; `d` 86,400,000; `w` 604,800,000 | 0 … 9223372036854775807 ms |

`m` is minutes. Floats and durations keep their source text as the token value, so that the syntax tree reproduces
them ([LQ/canonical-ast §3.2]); their numeric values enter only the canonical form.

The configuration files use another duration grammar ([CFG §4.1]: `ms s m h d`, no `w`), on purpose: configuration needs
millisecond waits and no weeks, query text the reverse. The two never meet as text; the Store API takes durations as integer
milliseconds or this section's form ([API §5.1]; pass 1, A1-55).

### 5.7 Strings

A string starts with `'` or `"` and ends at the next unescaped occurrence of the same quote. Inside it:

| Escape | Value |
|---|---|
| `\\` | `\` |
| `\'` | `'` |
| `\"` | `"` |
| `\n` | U+000A |
| `\r` | U+000D |
| `\t` | U+0009 |
| `\u{h}` … `\u{hhhhhh}` | the scalar value given by 1 to 6 hex digits (either case); it must be at most U+10FFFF and outside U+D800–U+DFFF |

Any other `\` sequence is E003 at the `\` — including JSON's `A`, whose fix is `\u{0041}`, and `\x41`, `\0`,
`\b`, `\/`. A raw LF or CR, or the end of the input, before the closing quote is E002 at the opening quote: strings are
single-line (Open point L-1). The token's value is the decoded text; both quote forms give the same kind of token, and
two adjacent strings are two tokens. The single-quote form is preferred in examples because it needs no escaping inside
MCP JSON and survives PowerShell 5.1 ([50 §2.2] rule 9).

### 5.8 Node and uid literals

- `#` followed by digits is a **node literal**; its value is the decimal number with leading zeros ignored and must be in
  1 … 4294967295 (E003 otherwise, `#0` included).
- `#u:` followed by exactly 32 lower-case hex digits is a **uid literal** ([AR §3.1] uid).
- Anything else after `#` is E003 (hint: `#<digits>` or `#u:<32 hex>`): `#`, `#x`, `#u`, `#u:` with fewer than 32 hex
  digits or with upper-case hex.
- A letter, digit or `_` directly after either literal is E003 (`#40abc`; `#u:` followed by 33 hex digits).
- `#133.body` is the node literal `#133`, `.` and the word `body` (the `PATCH` target of [50 §4.2]).

In LQ text `#N` is an ordinary token. The rule that ids are bare in argv ([80 §4] T1) concerns the shell command line,
not LQ text ([50 §0.1] shell rule).

### 5.9 Punctuation

Punctuation is lexed by longest match:

| Bytes | Token | Notes |
|---|---|---|
| `..` | `..` | ranges in `edge_body` (`*1..3`) and `expect` (`2..5`); three dots are `..` then `.` |
| `->` | `->` | always one token |
| `<-` | `<-` | **only when the byte after `<-` is `[` or `-`** ([50 §2.3] parser notes); otherwise `<` then `-`, so `a<-1` is `a < -1` |
| `<>` `<=` `>=` `!=` | themselves | |
| `=~` | `=~` | lexed only so that the parser can refuse it with E004 ([LQ/grammar-v1.ebnf §R]) |
| `(` `)` `[` `]` `{` `}` `,` `;` `.` `:` `\|` `+` `-` `*` `/` `=` `<` `>` `?` `%` | themselves | `?` is valid only after a `param_decl` type; `%` only so that the parser can refuse it with E004 |

`--` is two `-` tokens; `-->` is `-` `->`; `<--` is `<-` `-`; `<-->` is `<-` `->` by longest match (after `<-` the
bytes `->` are one token), not `<-` `-` `->`; the grammar still refuses it (no `edge_pat` alternative continues `<-`
with `->`; [LQ/grammar-v1.ebnf §R]).

## 6. Keywords

### 6.1 Reserved words

These 44 words are keywords everywhere except in the plain-name positions of §6.3; as a variable they must be
back-quoted ([50 §2.2] rule 3):

`MATCH OPTIONAL WHERE WITH RETURN CALL YIELD UNWIND USE UNION EXCEPT INTERSECT ORDER BY LIMIT GROUP AND OR NOT IN IS
NULL TRUE FALSE EXISTS CASE WHEN THEN ELSE END AS DISTINCT ASC DESC ASCENDING DESCENDING TX SET REMOVE DELETE CREATE
INSERT EXPECT ASSERT`

### 6.2 Contextual keywords

Every other keyword of [LQ/grammar-v1.ebnf] is contextual: a word is that keyword exactly where the grammar admits the
keyword at that point, and a name everywhere else ([LQ/grammar-v1.ebnf §P.3]). The contextual keywords of grammar v1
are:

`EXPLAIN PROFILE ALL WALK TRAIL ACYCLIC SIMPLE DIFFERENT RELATIONSHIPS EDGES STARTS ENDS CONTAINS COUNT ANY NONE SIZE
ON IF TIP TARGETS KEY LEASE MESSAGE DRY MOVE UNDER BEFORE AFTER FIRST LAST REOPEN REASON PATCH ADD POLICY RESTRICT
CASCADE REPARENT REPLACED RELEASE UNLESS RESOLVE TAKE OURS THEIRS BASE VALUE REPOINT DEFINE QUERY SHAPE BUDGET DROP`

`YIELD key, node, class` and `r.order` work because `KEY` is contextual and a word after `.` is a name
([50 §2.2] rule 3). `HEAD` is not a normal-mode keyword; it exists only in revision mode (§7.2).

### 6.3 Plain-name positions

In these positions any word, reserved or not, is a name:

| Position | Example |
|---|---|
| after `.` (property access, `proc_name`, `tx_name`, `qname` segments) | `t.order`, `std.ready`, `tx.complete` |
| a `prop_map` or `map_lit` key | `{limit: 5}` |
| a named-argument name (a word followed by `:` at the start of an argument) | `history(#12, in: main..lane/x)` |
| a label or type name after `:` or `\|` | `(n:task)`, `-[:BLOCKS\|GATES]->` |
| a yield field name (the name before an optional `AS`) | `YIELD key, node` |
| the first segment of `proc_name` and `qname`, and the `tx` of `tx_name` | `CALL std.ready()`; `CALL tx.complete(…)` is then refused in a read (E006) and accepted in a `TX` block; either way `tx` is a name here, matched ASCII-case-insensitively (L-3), and a token stream records it as `NAME` (§11) |
| the word after `SHAPE`, `BUDGET`, and the words of a `param_decl` `type` | `SHAPE node`, `$ids: list<node>` |

A variable, a `YIELD … AS` alias, an `AS` alias, an `UNWIND … AS` name, a list-predicate variable, a `CREATE` variable
and a `target` are not plain-name positions: a reserved word there must be back-quoted.

### 6.4 Refused words and names

These words are not keywords of grammar v1. The parser recognises them only at the positions of
[LQ/grammar-v1.ebnf §R], where they raise the code shown there; elsewhere they are names:

`MERGE DETACH NODETACH SKIP OFFSET FOREACH FOR LET FILTER NEXT XOR REPEATABLE ELEMENTS SHORTEST LOAD SHOW INDEX
CONSTRAINT LABELED`

These function names are refused at a function-call position (name matched ASCII-case-insensitively):
`shortestPath allShortestPaths nodes relationships single timestamp CAST`, and the procedure namespaces `apoc`, `gds`,
`db`, `dbms` as the first `proc_name` segment.

### 6.5 Case

Keywords are matched ASCII-case-insensitively (`match` = `MATCH`); there is no Unicode case folding. The token stream
of §11 prints a keyword in upper case.

## 7. Revision mode

### 7.1 What is read

At a revision position the lexer skips whitespace and comments and then reads one `revspec` (after `USE`, `ON`,
`IF TIP`) or one `rev_arg` (argument positions): a base (§7.2), zero or more suffixes (§7.3), and in argument positions
an optional range operator with a second revspec, or a list (§7.4).

### 7.2 Bases

| First bytes | Base | Rule |
|---|---|---|
| `$` + word | parameter | `revspec = param`: a parameter takes no suffix (`$r~1` is E001) |
| `HEAD` | `HEAD` | the four bytes `48 45 41 44`, not followed by a letter, digit, `_`, `-` or `/`, nor by a `.` that would continue a ref word (a `.` whose next byte starts a `ref_word`, as in the ref-name rule below); a `.` that starts `..` or `...` ends the base, so `HEAD..main` and `HEAD...lane/x` are ranges (case-sensitive; Open point O-3 of [LQ/grammar-v1.ebnf]) |
| a lower-case letter, digit or `_` | a `ref_name`, then classified | below |
| an upper-case letter (other than `HEAD`) | — | E003 (hint: ref names are lower case; `HEAD` is upper case) |
| `[` (argument positions only) | a list | §7.4 |
| `'` or `"` (argument positions only) | — | revision mode ends; an `expr` follows (§4.2) |
| anything else | — | E001 (expected a revision: a ref, `c<hex>`, `s<seq>`, `HEAD` or `$param`) |

The **ref name** is read greedily: a `ref_word` is `[a-z0-9_][a-z0-9_-]*`; a `.` continues the segment only when the
byte after it starts a `ref_word`; a `/` starts a new segment only when the byte after it starts a `ref_word`. So `..`
and `...` are never part of a ref name, and neither is a leading or trailing `.` or `/` ([50 §2.2] rule 6, [50 §12.2]
M1; git-check-ref-format). `ref_word` stays as [50] defines it ([80 §3] X-F9); NFC normalisation of ref-name input is
moot because the form is ASCII (Open point L-10).

The ref name is then **classified**, in this order:

1. a single `ref_word` (no `/`, no `.`) matching `c[0-9a-f]{7,64}` is a **commit literal** (a commit id or prefix);
2. a single `ref_word` matching `s[0-9]+` is a **sequence literal**; its value must be at most 18446744073709551615
   (E003 otherwise);
3. anything else is a **ref name**.

No ref can have the shape of rule 1 or 2: [50 §2.2] rule 6, as amended after the A1 re-review (S-11), makes LQ's
`ref_name` the ref-name grammar of the whole store and refuses to create a ref with a segment matching
`c[0-9a-f]{7,64}` or `s[0-9]+` ([F12]). So a token of those shapes in revision mode is always a literal. `c123456` (six
hex digits) and `c` followed by 65 hex digits are ref names, which the binder reports as unknown revisions (E301) if no
such ref exists (Open point L-8).

### 7.3 Suffixes

Suffixes follow the base with no whitespace or comment before or between them:

| Form | Suffix | Count |
|---|---|---|
| `~` [digits] | first-parent ancestor | omitted = 1; 0 … 4294967295 |
| `^` [digits] | n-th parent | omitted = 1; 0 … 4294967295 |
| `@` digits | reflog position | 0 … 4294967295 |
| `@{` digits `}` | the same as `@` digits (git's spelling, accepted in files, [50 §2.4]) | as above |
| `@` datetime | the ref at that wall time (§7.5) | — |

A count outside its range is E003. `@` followed by none of these forms is E001 ("a bare @ is not a revision",
[50 §2.4]). Reflog suffixes are store-local: in a stored definition they are E117 ([50 §4.4]; the binder's check).

### 7.4 Ranges and lists (argument positions only)

- After the first revspec, optional whitespace (not comments), then `...` or `..` by longest match (four or more dots
  are E001), then optional whitespace, then a second revspec. The second revspec has no range and no list. Whitespace
  around the operator is a leniency; the canonical spelling has none (Open point L-5).
- A list is `[`, one or more revspecs separated by `,`, and `]`, with whitespace (and comments) allowed around elements
  and commas. An empty list, and a range inside a list, are E001.

### 7.5 Datetimes

After `@`, the bytes `DDDD-DD-DD` (D an ASCII digit) start a datetime: `YYYY-MM-DD`, then optionally `T` and `HH:MM`,
then optionally `:SS`, then optionally `Z`.

| Field | Range |
|---|---|
| `YYYY` | 0000 … 9999 |
| `MM` (month) | 01 … 12 |
| `DD` | 01 … the last day of that month in the proleptic Gregorian calendar (29 February only in leap years) |
| `HH` | 00 … 23 |
| `MM` (minutes) | 00 … 59 |
| `SS` | 00 … 59 (no leap second) |

A field outside its range is E003. The time zone is always UTC; `Z` is optional and changes nothing; a missing time is
`00:00:00` and missing seconds are `00`. The normalised form is `YYYY-MM-DDTHH:MM:SSZ`, and the value is milliseconds
since 1970-01-01T00:00:00Z (negative before it). An offset such as `+02:00` is not part of a datetime: the revision ends
before `+`.

### 7.6 End of a revision

A revision ends at the first byte that cannot continue it. If that byte is an ASCII letter, digit or `_` the revision
is malformed (E003: `main~5x`, `c9b2e6c1Z`). Otherwise revision mode ends and the next token is lexed in normal mode.
A comment or whitespace ends a revision, so `main /* x */ ~2` is the ref `main` followed by a normal-mode `~` (E001).

## 8. Limits

| Item | Limit | Code |
|---|---|---|
| integer | 0 … 2^63 − 1 | E003 |
| node literal | 1 … 2^32 − 1 | E003 |
| uid literal | exactly 32 lower-case hex digits | E003 |
| float | finite binary64 | E003 |
| duration | 0 … 2^63 − 1 ms | E003 |
| commit literal | 7 … 64 lower-case hex digits after `c` | (a longer or shorter word is a ref name, §7.2) |
| sequence literal | 0 … 2^64 − 1 | E003 |
| suffix count | 0 … 2^32 − 1 | E003 |
| quantifier bound, `EXPECT` count | 0 … 2^32 − 1 (quantifier); 0 … 2^63 − 1 (`EXPECT`, an integer) | E114 (parser, [LQ/grammar-v1.ebnf §R]); E003 |
| nesting depth | 64 | E001 (parser, [LQ/grammar-v1.ebnf §P.13]) |
| input text on stdin or from `-f FILE` | `input.max-bytes` ([CFG §10.5], 16 MiB by default), counted while reading | exit 2 ([F19 §10.2] `usage`), before lexing |

There is no lexical limit on the length of a query, a string or a name; the budgets of [50 §5.10] bound the work. The input
bound of the last row is not lexical: stdin and `-f FILE` are read incrementally and refused as soon as they pass
`input.max-bytes`, so no budget has to apply to a text that was never bounded (pass 1, P1-39; [OS/shell §5.2] reads).

## 9. Name matching

| Namespace | Matching | Source |
|---|---|---|
| keywords | ASCII case-insensitive | [50 §2.2] rule 3 |
| variables, parameters, property names, map keys, yield field names, named-argument names, `AS` aliases | exact (case-sensitive) | [50 §2.2] rule 4 |
| kind names (labels), the pseudo-label `DELETED`, edge type names and their aliases | ASCII case-insensitive (`:Task` = `:task`, `:blocks` = `:BLOCKS`) | [50 §2.2] rule 4 |
| built-in scalar, aggregate and table-function names; the `std`, `tx`, `apoc`, `gds`, `db`, `dbms` prefixes | ASCII case-insensitive | Open point L-3 |
| named-query and named-mutation names after the namespace | exact | Open point L-3 |
| shape, budget and parameter-type names | ASCII case-insensitive | Open point L-3 |
| ref names | exact; lower case by construction; `HEAD` exact | §7.2 |

Enum values, their coercion from strings and bare words, and the suggestions for a near miss are the binder's
([50 §3.2]; E102, E108 in [LQ/errors]).

## 10. Transport and stored-text checks

### 10.1 Stdin and file decoding ([80 §4] T6, X-F12)

Stdin (`moirai q -`, `moirai tx -`) and `-f FILE` go through §2.1 unchanged on every OS: one leading mark removed, and
ill-formed UTF-8 is E003, exit 2 — which turns dash's heredoc corruption into a loud error ([80 §4] T5, T6). Stdin is
read only with `-` or `--stdin`.

**The `?` predicate.** A source satisfies P? when either holds:

- (a) outside strings, back-quoted identifiers and comments, a `?` occurs that the parser did not accept as the `?` of
  a `param_decl`;
- (b) inside a string or back-quoted identifier, two or more consecutive `?` occur.

When P? holds and the text arrived on stdin from a parent process that is PowerShell, the CLI adds the transport
warning that suggests `-f` ([50 §6.2] rule 6; [80 §4] T6). It never changes the text (no silent fix). A default
PowerShell 5.1 pipe turns non-ASCII into `?` ([50 §6.2] rule 2); case (a) usually also ends in E001 (a `?` where a name
was meant). The warning's code and text are [LQ/errors]'s (Open point L-7); detecting the parent shell is the CLI's
(M8) through the OS layer's process module ([OS/proc]).

### 10.2 The portable-text check ([50 §4.4], F3)

A stored named-query text (the `QUERIES` text blob and the body of `schema/queries/<q>.moi`, [50 §4.4]) is
**portable** when both hold:

1. **Stored normal form.** The text is valid UTF-8 with no byte-order mark, no CR, and no SP or HT immediately before an
   LF or at the end ([50 §4.4] Storage; [LQ/canonical-ast §8.1] steps 1–3).
2. **No store-local constant after binding.** Parsed with start symbol `define_stmt` ([LQ/grammar-v1.ebnf §P.1]) and
   bound against the schema of the ref that carries it, the definition holds no node-typed constant other than a `#u:`
   literal, no revision-typed constant whose base is a sequence number or a commit prefix, no reflog revision and no
   anchor handle ([LQ/canonical-ast §8.2]).

The exporter asserts both and the importer checks both, staging `ImageParse` if one fails ([50 §4.4]; the `.moi` rules
are [F14]'s). A full commit id (`c` and 64 hex digits) is portable and binds as itself even in a store that does not
hold the commit ([LQ/canonical-ast §5.6]); only a query that opens that view fails, with E301 at view resolution (spec
sync 2a). Condition 2 is decided by re-binding, never by a character pattern ([50 §4.4] as amended after the A1
re-review, S-02): a back-quoted name or a string containing `#1` is portable, and `{id: 40}` is not. A byte scan for `#`
followed by a digit outside strings, comments and back-quoted names (with §3, §5.4 and §5.7 recognising them) is implied
by condition 2 and may serve as a fast pre-check, but never decides alone. The definition-time rewrite that makes a text
portable is [LQ/canonical-ast §8.1].

## 11. Token-stream fixture format

Conformance fixtures ([50 §8.3]; WP-22's `fixtures/lq/`) assert the **classified token stream of a successful parse**:
the tokens in order, each as the parser used it. Format, one token per line, LF line ends, UTF-8:

```
line  = kind SP value LF
kind  = "KW" / "NAME" / "QNAME" / "PARAM" / "INT" / "FLOAT" / "DUR" / "STR" / "NODE" / "UID" / "P"
      / "HEAD" / "REF" / "COMMIT" / "SEQ" / "SUF" / "RANGE" / "EOF"
```

| Kind | When | Value printed |
|---|---|---|
| `KW` | a word the parse used as a keyword (reserved, or contextual at a keyword position, or `count`/`all`/`any`/`none`/`exists`/`size` in their keyword forms of [LQ/grammar-v1.ebnf §P.9]) | the keyword in upper case |
| `NAME` | a word used as a name (variable, property, label, type, function, procedure, key) | the word as written |
| `QNAME` | a back-quoted identifier | the decoded name as a JSON string (§11.1) |
| `PARAM` | a parameter, in either mode | the name without `$` |
| `INT` | an integer | the decimal value without leading zeros |
| `FLOAT` | a float | the source text |
| `DUR` | a duration | the source text |
| `STR` | a string | the decoded value as a JSON string (§11.1) |
| `NODE` | a node literal | N in decimal, without leading zeros |
| `UID` | a uid literal | the 32 hex digits |
| `P` | punctuation, including a revision list's `[`, `,` and `]` | the bytes |
| `HEAD` | the `HEAD` base | `HEAD` |
| `REF` | a ref name | the name |
| `COMMIT` | a commit literal | the hex digits without `c` |
| `SEQ` | a sequence literal | the decimal value without the `s` |
| `SUF` | one suffix | `~n`, `^n`, `@n` with n explicit (an omitted count printed as 1; `@{n}` printed as `@n`), or `@YYYY-MM-DDTHH:MM:SSZ` |
| `RANGE` | a range operator | `..` or `...` |
| `EOF` | the end | `-` |

A generic function call is `NAME` (`count(x)`); the keyword forms are `KW` (`count( * )`, `COUNT {`, `exists(` with a
path, `size(` with a path, `all(x IN …)`). A word in a plain-name position (§6.3) is `NAME`, printed as written, even
when it is a reserved word: `CALL tx.claim(ids: [#15])` in a `TX` block records `KW CALL`, `NAME tx`, `P .`,
`NAME claim`, and `std.ready` records `NAME std`, `P .`, `NAME ready` (Open point L-14). A failing input is not a
token-stream fixture: its fixture asserts the first error's code, `line` and `col` ([LQ/grammar-v1.ebnf §P.14]).

### 11.1 JSON strings in fixtures

A JSON string value is `"` + the text + `"`, where `"` is written `\"`, `\` is `\\`, LF `\n`, CR `\r`, HT `\t`, every
other scalar below U+0020 and U+007F as `\u00xx` with lower-case hex, and every other scalar as its raw UTF-8 bytes.
[LQ/canonical-ast §4] uses the same rule.

### 11.2 Examples

`CALL log(main..lane/l5np) YIELD commit, actor, message` (fixture `range-2dot`):

```
KW CALL
NAME log
P (
REF main
RANGE ..
REF lane/l5np
P )
KW YIELD
NAME commit
P ,
NAME actor
P ,
NAME message
EOF -
```

`USE main@2026-09-25T10:00Z MATCH (s1:doc) WHERE s1 IN subtree(#130) RETURN s1`:

```
KW USE
REF main
SUF @2026-09-25T10:00:00Z
KW MATCH
P (
NAME s1
P :
NAME doc
P )
KW WHERE
NAME s1
KW IN
NAME subtree
P (
NODE 130
P )
KW RETURN
NAME s1
EOF -
```

`TX ON lane/l5np LEASE 'L-18' { MATCH (t {id: #89}) WHERE t.status = 'in_progress' EXPECT 1 SET t.done = true }`
(the card's example 7, [50 §7.2]):

```
KW TX
KW ON
REF lane/l5np
KW LEASE
STR "L-18"
P {
KW MATCH
P (
NAME t
P {
NAME id
P :
NODE 89
P }
P )
KW WHERE
NAME t
P .
NAME status
P =
STR "in_progress"
KW EXPECT
INT 1
KW SET
NAME t
P .
NAME done
P =
KW TRUE
P }
EOF -
```

## Coverage

| Item | Part covered here | Section |
|---|---|---|
| [80] X-F12 (T6: stdin decoding) | the LQ side of T6: one byte-order mark removed, ill-formed UTF-8 refused with E003 (exit 2), the PowerShell `?` predicate P?. Reading stdin, the parent-shell test and the rest of T1–T10 are [OS/shell]'s and [F19]'s | §2.1, §10.1 |
| [80] X-F12 (T1: ids bare in argv) | that `#N` in LQ text is an ordinary token; the argv rule itself is [OS/shell]'s | §5.8 |
| [80] X-F9 | "LQ `ref_word` stays as is": the ref-name form read in revision mode, and why NFC and the fold rule do not arise in LQ text. The ref-name rules of the ref verbs are [F12]'s | §7.2, L-10 |
| [50] F3 | the portable-text check of stored definitions (exporter and importer): the stored normal form and the re-binding condition. The rewrite is [LQ/canonical-ast §8]'s, the item [F08]'s, the query file's ABNF [F14]'s | §10.2 |

No other [60 §2.5] row, R-1…R-18 item, F-item, X-F item or [90 §10.1] item is specified here.

## Holes

None. No lexical byte, limit or code of this chapter waits on an M0 measurement. The chapter freezes with the query
surface after WP-72 ([LQ/grammar-v1.ebnf] O-11).

## Open points for the review

| # | Point | Resolution here |
|---|---|---|
| L-1 | [50 §2.2] rule 9 does not say whether a string may span lines. | Strings are single-line: a raw LF or CR before the closing quote is E002. A stored definition's text is normalised to LF line ends ([50 §4.4]); a raw CR LF inside a string would change the string's value under that normalisation, and E002 then points at the line where the quote is missing. Line breaks inside a value are written `\n`. |
| L-2 | Raw control characters are not addressed by [50]. | Outside strings and back-quoted identifiers they are E001; inside, the C0 controls other than HT, and DEL, are E003 (write `\u{…}`). This keeps a query text printable in diagnostics and `.moi` files. |
| L-3 | [50 §2.2] rule 4 fixes case rules for keywords, variables, parameters, properties, kinds, labels and edge types, but not for function, relation, named-query, shape or type names. | Built-in function and relation names, and the `std`/`tx` prefixes, are ASCII-case-insensitive (the Cypher habit: `COUNT(*)`, `toLower`); named-query and named-mutation names after the namespace are exact, because project names may differ only in case ([50 §4.4] names the `Foo`/`foo` case); shape, budget and type names are ASCII-case-insensitive. [LQ/canonical-ast §5.3] fixes the canonical spelling of each. |
| L-4 | Leading zeros are not addressed. | Accepted and ignored in integers, node literals, sequence literals and counts. Cypher 4 read a leading `0` as octal; LQ integers are decimal only ([50 §2.2] rule 8), so `010` is 10. |
| L-5 | Whitespace around `..`/`...` in a `rev_arg`. | Allowed (a leniency git does not have); not inside a revspec. The display printer writes none. |
| L-6 | `changes(ref: …)`. | A revision position, so `ref: lane/x` needs no quotes ([LQ/grammar-v1.ebnf] O-5). |
| L-7 | [50 §6.2] rule 6 asks for a warning on `?` but [50 §5.2]'s table allocates no code. | P? is defined here (§10.1); [LQ/errors] allocates W09 `stdin_question_marks` for it (its §5.1 and §5.6). |
| L-8 | Commit literals of 65 or more hex digits, or of fewer than 7; ref segments of literal shape (the A1 review's S-11). | A revision token is classified as a commit or sequence literal first (§7.2 rules 1–2), as S-11 asks of WP-19; [50 §2.2] rule 6, as amended (S-11 fixed in `docs/spec/reviews/a1-dispositions.md`), refuses such ref names at creation, which is WP-12's ([F12]), so no ref is shadowed. Words of `c` and 65 or more hex digits, or fewer than 7, are ref names by the grammar ([50 §2.3] `ref_word`), refused by the binder as unknown revisions (E301), not by the lexer; E003 "commit prefix" covers an upper-case or otherwise malformed `c…` word (§7.6). |
| L-9 | `HEAD`'s case. | Exact upper case; any other upper-case letter in a revision is E003 ([LQ/grammar-v1.ebnf] O-3). |
| L-10 | [80 §3] X-F9 asks for NFC normalisation of ref-name input and the refusal of fold-equal ref names. | In LQ the question does not arise: a ref name in LQ text is lower-case ASCII by `ref_word`, and amended [50 §2.2] rule 6 makes that the store's ref-name grammar, so the fold-equality refusal holds trivially. The ref-creating verbs and the image apply it ([F12], [F14]); X-F9 itself says "LQ `ref_word` stays as is". |
| L-11 | Revision 2 of [50 §4.4] stated the exporter's assertion as a `#`-digit scan; the A1 review's S-02 (major) showed that store-local constants need not be spelled with `#` or `s` (`{id: 40}`, `t.rev = 4466`, `a.anchor = 'a17'`), and amended [50 §4.4] now validates "by re-binding … never by a character pattern". | §10.2 follows the amended text: the stored normal form plus the re-binding condition; the byte scan is only an implied pre-check. The importer binds every imported definition anyway (F18 `QueryInvalid`), so the check costs one bind per definition. The definition-time rewrite that removes such constants is [LQ/canonical-ast §8.1]. |
| L-12 | A leading U+FEFF in an MCP string. | Removed like the stdin mark (§2.1 rule 3), so the three transports behave identically. |
| L-13 | The design's probe parser (`lqcheck2.py`, [50 §11]) lexes `<-` only before `[` or `-`, treats `..`/`...` by longest match, and refuses `%` and `=~` at operator positions. | This chapter follows it in each case; where it differs from the probe (strings on one line, named-argument case, `changes(ref:)`), the difference is listed above. |
| L-14 | How `CALL tx.x` is recorded in token streams (spec sync 2a, WP-93a). §6.3 makes the first segment of `proc_name` and `qname` a plain name, while an implementation recorded `KW TX` for the `tx` of a `TX` block's call. | `NAME`, printed as written: `tx` of `tx_name` is a plain-name position like the first segment of `proc_name` and `qname` ([LQ/grammar-v1.ebnf §P.3]), so every word in such a position is `NAME` (§11), and WP-22's fixtures carry `NAME tx`. The prefix is matched ASCII-case-insensitively (L-3). |
| L-15 | Spec sync 2a corrections (WP-93a review; WP-22's findings F-1, F-2). | `<-->` lexes as `<-` `->` (§5.9); §6.1 lists 44 reserved words, as [50 §2.2] rule 3 does; `HEAD` is refused as a base only before a `.` that continues a ref word, so `HEAD..main` and `HEAD...main` are ranges (§7.2); a full commit id binds as itself in the portable-text check (§10.2). |
