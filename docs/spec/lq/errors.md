# LQ errors, warnings and notices

| | |
|---|---|
| Title | The LQ diagnostic table: every error, warning and notice code, its exit code, its frozen text and its JSON form |
| Chapter | [LQ/errors], `docs/spec/lq/errors.md` |
| Status | draft, pass 1 pending |
| Work package | WP-19b (R-SPEC-F), part of WP-19 of [PLAN §3.2] item 1 |
| Sources | [50 §5.2] (the code table and the error format), [50 §2.2] rules 1, 5, 7, 10, [50 §2.3] (the parser notes and the `(* E… *)` annotations), [50 §2.3.1], [50 §2.5], [50 §2.6], [50 §2.7], [50 §2.8] (the rejection table), [50 §2.9] (the error examples and the ten mistakes), [50 §3.2]–[50 §3.10], [50 §4.4], [50 §5.6], [50 §5.10], [50 §6.1]–[50 §6.6], [50 §7.4] items 4 and 9, [50 §12.5] (the 600-B bound); [AR §7.1] (exit codes, stdout on non-zero exit), [AR §7.3] (the unleased refusal text), [AR §7.7.3]; [90 §4.1] (the two exit-5 texts), [90 §4.3], [90 §8.1] L2, L4, L5, L6, [90 §10.1] row "Error table and refusal texts"; [80 §4] T6, T9; [73 F15] through [50 §12.5] |
| Depends on | [LQ/lexical], [LQ/grammar-v1.ebnf] (detection points), [LQ/envelope] (placement of texts), [LQ/std] (names quoted in texts), [LQ/gql-spelling] (display spelling), [F19] (exit codes; must agree) |

## 1. Scope

1.1. This chapter owns every LQ diagnostic code: `E001`–`E009`, `E101`–`E118`, `E201`–`E202`, `E301`–`E308`, `E401`–`E411`,
`E501`–`E505`, `W01`–`W10` and `N01`–`N12`. For each it fixes the name, the severity, the exit code, the component that raises it,
the frozen text and the JSON object. The texts freeze with the query surface at WP-72 (GT13); before that, WP-73 may change a text, a lint or a code's trigger,
and after it a changed text is a new grammar version ([50 §7.4] item 6).

1.2. It does not own the exit-code table itself, the non-LQ refusals of the store and the file verbs (`store_read_only`,
`fs_busy`, `not_writer_tree`, `ambiguous_path`, exit 1, 7 and 8), or the violation classes of the merge engine. Those are
[F19]'s ([F19] also records the exit code of every code below; the two tables must agree). `QueryInvalid` and `QueryCycle`
(F18) are violation classes of [F19]/[F12]; this chapter only fixes the E405 text a `TX` gets when its `DEFINE QUERY` would
create a named-query cycle (§5.5).

1.3. Every write verb and every `apply` batch compiles to one `TX` block ([50 §4.2], [AR §7.7.2]), so the E4xx texts below are
also the texts of the write verbs. [AR §7.1]'s example `error[guard_conflict]: …` for `set --if-rev` is superseded by
`error[E401 expect_mismatch]` (open point 3).

1.4. The result envelope, the header, the reading echo and the footers are [LQ/envelope]'s. The standard library's names and
signatures, which several texts below quote, are [LQ/std]'s.

## 2. Conventions shared by every text

2.1. **Character set.** Every byte a template contributes is printable ASCII (0x20–0x7E) or LF ([90 §8.1] L5). Values
interpolated from the caller's input or the store (identifiers, string literals, titles, excerpts of the query text) keep their
UTF-8 bytes; C0 controls and DEL inside them are rendered as `\u{h}`, h the code point in lower-case hex without leading zeros
(`\u{1b}`), the one spelling of [F19 §2.5] and [LQ/envelope §5.16] (pass 1, A1-48); TAB inside an excerpt as one space. No template contains a harness-specific tool name ([90 §8.1] L6): tools are named "the `query` tool",
"the `write` tool".

2.2. **Placeholders.** Templates below write placeholders as `<name>`. Their renderings:

| Placeholder | Rendering |
|---|---|
| `<id>` | `#N` (decimal, no padding); a uid the store knows renders as its `#N`, an unknown one as `#u:` + 32 lower-case hex |
| `<c8>` | `c` followed by the first 8 lower-case hex digits of the commit id (9 bytes; [AR §5a.1], [50 §2.4]) |
| `<rev>` | the sequence number in decimal |
| `<revspec>` | the revision as the caller wrote it ([LQ/lexical §7]) |
| `<ref>` | a ref name |
| `<lease>` | `L-` followed by the lease id in decimal ([F19 §2.4]) |
| `<holder>` | the holder's actor name as recorded (the `actor` symbol's text, [F11 §6] `holder`), as `dev#2` |
| `<kind>`, `<field>`, `<edge>`, `<fn>`, `<var>`, `<param>` | the name, in the spelling of the schema ([F08]) for schema names and of the query text otherwise, inside back-quotes |
| `<value>` | a value in the rendering of [LQ/envelope §5.2], inside back-quotes |
| `<n>`, `<m>` | a decimal integer, no grouping separator |
| `<N>` in budget texts | a decimal integer with `,` every three digits (`2,000,000`), as [50 §6.4]'s footers |
| `<list>` | up to 5 items joined by `, `; when more exist, `, ...` is appended |

2.2a. **Names inside LQ text** (spec sync 2b). Where a template embeds a placeholder in LQ text — a `write …` replacement, a
pattern such as N07's `(<b>)-[:<edge>]->(<a>)`, W01's `coalesce(<expr>, 0)`, E106's reversed pattern — the placeholder renders
as LQ source, not with §2.2's back-quotes: a name as the S-AST printer writes it (back-quoted only when LQ requires it,
[LQ/canonical-ast §3.4]), a value as an LQ literal. §2.2's back-quotes apply to names and values in prose.

2.3. **Value cap.** Each interpolated value is at most `V` bytes, where `V` starts at 64 and may drop to 24 during fitting
(§3.4). A longer value is cut at the last UTF-8 scalar boundary that leaves room for `...` and gets `...` appended; an escape
sequence is never split.

2.4. **One fix.** Every error names exactly one fix ([50 §5.2]). When the fix is mechanical, the text prints the replacement
text itself as `write <replacement>` ([90 §8.1] L4); the replacement is printed in the display spelling
(`HOLE(LQ-display-spelling)`, [LQ/gql-spelling §4]). The codes with a mechanical fix are marked "yes" in the "fix" column of §5.

2.5. **Bound.** Every rendered error, warning or notice text, including its continuation lines and their LF bytes, is at most
**600 bytes** ([50 §5.2], [AR §7.7.3], [AR §8.3] TOKENS row "LQ error text"). §3.4 gives the fitting procedure that guarantees
it.

## 3. Text form of an error

3.1. **Channel.** An error goes to stdout, like every result of a non-zero exit ([AR §7.1]: stdout carries at most
`output.nonzero-exit-max-bytes`, 8,000 B), with one exception: under `--ids` it goes to stderr, so a pipe never receives it as an
id. There is no header line before an error and no exit-code line after it ([50 §5.2]; the Bash tool reports the code). MCP
returns the same text in `content[0]` with `isError: true` ([50 §5.2], [90 §2.2]). At most three errors are printed per call
([50 §5.2]: panic-mode recovery reports up to three per pass), separated by one empty line; the exit code is the first error's.
The errors of one pass are printed in this order: E406 and E411 first (the policy refusals of a block, in code order); then
the located errors by span start, ties broken by the smaller code; then the unlocated errors, in code order. This is the
first-diagnostic rule of [F19 §12.5.3] with the policy refusals put first, so a caller who may not write learns that before
any detail of the text (spec sync 2a). A lexer or parser error ends the pass before binding, so binder errors never mix with
them. When the two alternatives of [LQ/grammar-v1.ebnf §P.5] fail at the same byte and one of the errors is P13's nesting
error, the nesting error wins (its text, `nesting deeper than 64 levels`, names the cause either way); every other tie keeps
P5's rule, the path alternative's error.

**Which error comes first** (spec sync 2b; normative for every implementation, whatever its lookahead). Read the text as the
token sequence of [LQ/lexical]. The first error of a pass is at the first token t such that the tokens before t form a
**viable prefix** — a prefix of some text grammar v1 accepts — and either t is not a valid token (a lexical error, at t) or
the tokens up to and including t form no viable prefix (a syntax error, at t, or the refusal of Annex R whose form t
completes). So a lexical error that lies after a syntax error is never reported first, and one inside a still-viable prefix
always is: `RETURN a NOT $` gives E001 `$ must be followed by a parameter name` at 1:14, because `RETURN a NOT` is viable
(`RETURN a NOT IN l`). An error whose detection point no chapter names is located at the first token or character of the
construct the rule refuses: a refused keyword form at its first word, an operator at the operator, a literal out of range at
its first character, E007 at the token where `EXPECT` was expected, E009 at the `}`, E114 at the quantifier's first
character, E118 at the comparison operator or the property-map key (`fixtures/lq/INDEX.md` §4 G-7, adopted). Binder errors
carry no position unless their row gives one.

3.2. **Located form.** An error is *located* when it concerns a span of the submitted LQ text (or of a stored definition) and
its code is not an E4xx or E5xx code. Let `L` be the 1-based line of the span start, `C` its 1-based column in Unicode scalar
values ([50 §2.2] rule 1), and `w` the number of decimal digits of `L`. The lines are, each ended by LF:

| # | Line | Present |
|---|---|---|
| 1 | `error[` code ` ` name `]: ` message | always |
| 2 | `w` spaces, `--> `, source, `:`, `L`, `:`, `C` | always |
| 3 | `w + 1` spaces, `\|` | when line 4 is present |
| 4 | `L`, ` \| `, excerpt | when the excerpt window is > 0 (§3.4) |
| 5 | `w + 1` spaces, `\| `, pad, carets, and ` ` + inline when an inline text exists | with line 4 |
| 6 | `w + 1` spaces, `= help: `, help | when the code has a help text |

- *source* is `<stdin>` for `moirai q -` and `moirai tx -`, `<argv>` for inline text, `<query>` for an MCP `q` or `tx` string,
  the file's base name for `-f FILE` (cut to 40 bytes by §2.3's rule), and `std.<name>` or the project query's name for an error
  inside a stored definition ([50 §4.4] "errors that point into its definition").
- *excerpt* is the part of line `L` from `max(line start, span start − W)` to `min(line end, span end + W)` scalars, `W` being
  the excerpt window (60 at first, §3.4). A cut on the left is marked by a leading `...`, a cut on the right by a trailing
  `...`. A span longer than 40 scalars shows its first 40 scalars and `...`, and the carets cover those 43 columns. A span that
  crosses line ends is shown up to the end of line `L`.
- *pad* is one space per scalar of the excerpt before the span start (the leading `...` counts 3). *carets* is one `^` per
  scalar of the span as shown (at least one; a zero-width span, as at end of input, has one caret on the column after the last
  scalar).
- *inline* is the code's inline text (§5), at most 80 bytes (cut by §2.3's rule).

Example (E101, [50 §2.9]):

```
error[E101 unknown_field]: kind `task` has no field `stauts`
 --> q.lq:2:9
  |
2 | WHERE t.stauts = 'open'
  |         ^^^^^^ did you mean `status`?
  = help: task fields: status, priority, labels, assignee, work_kind, ... (CALL schema(kind: 'task'))
```

3.3. **Unlocated form.** Every other error is line 1 above, then its detail lines (two spaces, then the detail text), then the
help line as `  = help: ` + help when the code has one. Each detail line is at most 120 bytes after interpolation: a longer one
is cut by §2.3's rule to that length (pass 1, A1-32). A detail line that §5 writes as a fixed closing line — `nothing was
written`, and `nothing was changed` — is the text's **closing line**: it is always last among the detail lines and fitting
never drops it (§3.4). Example (E401, [50 §2.9] Q18, in this chapter's form):

```
error[E401 expect_mismatch]: statement 1 matched 0 bindings, expected 1
  #89 now: status=done rev=4470 (changed at c3e9a1f00 by dev#2 on lane/l5np: "complete L-17")
  nothing was written
  = help: re-read with moirai q show ids=89
```

3.4. **Fitting.** The text is rendered with `W = 60`, at most `A = 5` help alternatives ([50 §5.2]: "at most 5 valid
alternatives … nearest first"), `V = 64`, at most 10 detail lines. While the rendering exceeds 600 bytes, the next step of this
list is applied and the text re-rendered; the list ends in a form that always fits:

1. drop the last help alternative, down to one (the `(CALL schema(...))` tail stays);
2. drop the last detail line other than the closing line, down to two such lines (for E401: two target lines), and add a
   detail line `... <n> more` naming how many were dropped, placed before the closing line;
3. `W` = 30, then `W` = 10;
4. `V` = 24;
5. drop the help line;
6. `W` = 0 (lines 3–5 of §3.2 disappear; line 2 stays);
7. drop every remaining detail line other than the closing line, counting them in the `... <n> more` line;
8. cut the message by §2.3's rule to the length that fits.

Bound check (pass 1, A1-32, redone). Before step 7 the fixed-size lines are: line 1's prefix, at most 6 + 4 + 1 + 26 (the
longest name, `step_variable_out_of_scope`) + 3 = 40 bytes before the message; line 2, at most 10 + 4 + 40 + 1 + 10 + 1 + 10 +
1 = 77 bytes; at most two detail lines of 2 + 120 + 1 = 123 bytes each; the `... <n> more` line, at most 2 + 14 + 1 = 17 bytes;
the closing line, at most 2 + 19 + 1 = 22 bytes. After step 7 only line 1, line 2, the `...` line and the closing line remain:
40 + 77 + 17 + 22 = 156 bytes plus the message and its LF, so step 8 leaves the message at least 443 bytes and always ends
within 600 bytes. Detail lines interpolate several values (E401's fields, rev, commit, actor, ref and message), so a
detail line can reach its 120-byte cut on ordinary input; the fitting no longer assumes 120 bytes of fixed text.

3.5. **Several errors in one pass.** Each is fitted on its own. They share nothing; a second error that repeats the first's code
and span is not printed.

## 4. JSON form and the text of warnings and notices

4.1. **Error envelope** (`--json v1`, MCP `format: "json"`): one line of compact JSON (no insignificant whitespace, keys in the
order shown, LF after the closing brace):

```
{"v":1,"branch":<string|null>,"rev":<int|null>,"errors":[<error>...],"exit":<int>}
```

`branch` and `rev` are the caller's resolved branch and its tip sequence number, or `null` when the error came before they were
resolved. Each `<error>` is:

| Key | Type | Content |
|---|---|---|
| `code` | string | `E001` … |
| `name` | string | the snake_case name of §5 |
| `severity` | string | `"error"` |
| `span` | object or null | `{"start":<byte offset>,"end":<byte offset>,"line":<L>,"col":<C>}`, offsets 0-based into the submitted text's bytes after BOM removal, `end` exclusive; `null` for an unlocated error |
| `message` | string | the message of line 1, rendered with `V` = 256 and not fitted |
| `suggest` | array of string | the did-you-mean candidates, nearest first, at most 5 |
| `expected` | array of string | the parser's expected token spellings (E001 only), at most 10; `[]` otherwise |
| `help` | string or null | the help text, not fitted |
| `detail` | array of string | the detail lines of §3.3 without their indentation; `[]` when none |

and then the code-specific keys of §5.7 (additive; absent when the code defines none). [50 §2.9] Q18's `"error":{…}` object with
`expected` meaning the `EXPECT` bound and a `hint` key is replaced by this form: `expected` keeps the parser meaning of [50 §5.2]
and the bound moves to `expect` (open point 4).

4.2. **Warnings and notices, text.** A warning or notice renders as one line `Wnn: ` or `Nnn: ` followed by its message, then
zero or more continuation lines indented by five spaces (the width of `Wnn: `), each ended by LF. A configuration diagnostic of
[CFG §6.2] renders `CFGnn: ` and its message, with continuation lines indented seven spaces (the width of `CFGnn: `; pass 1,
A1-62). They are placed in the footer block of [LQ/envelope §6] in this order: `W` warnings in ascending code order, then `N`
notices in ascending code order, then the `CFG` diagnostics in ascending code order ([F19 §10.6]), except under `--ids`, where
they go to stderr before the `--ids` footer. A code that fires several times in one result prints once, with the counts summed
where its text carries a count, and with the first occurrence's values otherwise. Each is at most 600 bytes, fitted by steps 2,
4 and 7 of §3.4.

4.3. **Warnings and notices, JSON.** The envelope's `warnings` and `notices` arrays hold objects
`{"code":<string>,"name":<string>,"message":<string>,"detail":[<string>...]}` plus `"count":<int>` for the codes whose text
carries a count (W01, W04, W05, W10, N09, N10), in the order of §4.2. `message` is the first line without the `Wnn: ` prefix.

4.4. **Warnings do not change the exit code.** A result with warnings or notices exits as it would without them ([50 §5.2]: the
W and N rows carry no exit code).

## 5. The code table

5.1. **Summary.** "Raised by" names the component of [50 §5.1]'s pipeline. "M0" says whether the reference model's LQ-3
(WP-93a/b) or the LQ-Bench reference renderer (WP-71a) must produce the code at M0 (`model`, `renderer`), or whether it
exists only in the product (`product`: it depends on a planner, a memory account, a clock or a shell the model does not have).
"Fix" says whether the text carries a mechanical replacement (§2.4).

| Code | Name | Sev. | Exit | Raised by | M0 | Fix |
|---|---|---|---|---|---|---|
| E001 | `syntax` | error | 2 | lexer, parser; binder for an unaliased `WITH` item | model | when a rewrite is known |
| E002 | `unterminated` | error | 2 | lexer | model | no |
| E003 | `bad_literal` | error | 2 | lexer | model | no |
| E004 | `not_in_lq` | error | 2 | parser | model | per §6 |
| E005 | `one_statement` | error | 2 | parser | model | no |
| E006 | `read_only` | error | 2 | parser | model | no |
| E007 | `expect_required` | error | 2 | parser | model | no |
| E008 | — | — | — | not assigned in grammar v1; never assigned later (§8) | — | — |
| E009 | `empty_tx` | error | 2 | parser ([LQ/grammar-v1.ebnf §P.12]) | model | no |
| E101 | `unknown_field` | error | 2 | binder | model | yes (fixed hints) |
| E102 | `unknown_value` | error | 2 | binder | model | yes (`labels()`) |
| E103 | `type_mismatch` | error | 2 | binder, executor | model | yes (counters) |
| E104 | `unknown_edge_type` | error | 2 | binder | model | no |
| E105 | `unknown_kind` | error | 2 | binder | model | no |
| E106 | `edge_direction` | error | 2 | binder | model | yes (reversed pattern) |
| E107 | `ambiguous_edge_name` | error | 2 | binder | model | no |
| E108 | `unknown_enum_word` | error | 2 | binder | model | no |
| E109 | `unknown_function` | error | 2 | parser ([LQ/grammar-v1.ebnf §P.12]: a `CALL` inside `TX` that names no named mutation), binder | model | no |
| E110 | `bad_parameter` | error | 2 | binder, CLI argv | model | no |
| E111 | `no_such_node` | error | 2 | binder | model | no |
| E112 | `aggregate_misuse` | error | 2 | binder | model | no |
| E113 | `path_variable` | error | 2 | parser, binder | model | yes |
| E114 | `bad_quantifier` | error | 2 | parser | model | no |
| E115 | `not_writable` | error | 2 | binder | model | no (names the verb) |
| E116 | `step_variable_out_of_scope` | error | 2 | binder | model | no |
| E117 | `store_local_in_definition` | error | 2 | binder | model | no |
| E118 | `null_comparison` | error | 2 | parser ([LQ/grammar-v1.ebnf §P.18]) | model | yes |
| E201 | `too_broad` | error | 10 | planner | product | no |
| E202 | `unbounded_sort` | error | 10 | planner | product | no |
| E301 | `unknown_revision` | error | 3 | view resolution | model | no |
| E302 | `not_at_this_view` | error | 2 | binder | model | yes (`unblocked`) |
| E303 | `as_of_too_far` | error | 10 | planner | product | no |
| E304 | `too_many_refs` | error | 10 | planner | model | no |
| E305 | `read_only_view` | error | 6 | tx binder | model | no |
| E306 | `cursor_mismatch` | error | 2 | executor | renderer | no |
| E307 | — | — | — | retired ([50 §3.9] item 1); never reused | — | — |
| E308 | `use_in_subquery` | error | 2 | parser | model | no |
| E401 | `expect_mismatch` | error | 4 | tx | model | no |
| E402 | `tip_moved` | error | 4 | tx | model | no |
| E403 | `assert_failed` | error | 6 | tx | model | no |
| E404 | `transition_refused` | error | 6 | tx | model | yes (`REOPEN`) |
| E405 | `invariant` | error | 6 | tx | model | no |
| E406 | `role_policy` | error | 6 | binder (a read under a named-only safelist), tx binder, tx | model | no |
| E407 | `lease` | error | 5 | tx | model | no |
| E408 | `idempotency_mismatch` | error | 9 | tx | model | no |
| E409 | `restricted_delete` | error | 6 | tx | model | no |
| E410 | `ambiguous_bind` | error | 6 | tx | model | no |
| E411 | `unknown_model_write` | error | 6 | tx binder | model | no (names the mutation) |
| E501 | `work_budget` | error | 10 | executor | product | no |
| E502 | `memory_budget` | error | 10 | executor | product | no |
| E503 | `deadline` | error | 10 | executor | product | no |
| E504 | `cancelled` | error | 10 | executor | product | no |
| E505 | `fs_budget` | error | 10 | executor | model | no |
| W01 | `absent_decided` | warning | — | binder, executor | model | — |
| W02 | `match_mode_ignored` | warning | — | binder | model | — |
| W03 | `done_includes_cancelled` | warning | — | executor | model | — |
| W04 | `body_scan` | warning | — | planner | product | — |
| W05 | `derived_recomputed` | warning | — | executor | product | — |
| W06 | `live_page` | warning | — | executor | renderer | — |
| W07 | `hand_derived_readiness` | warning | — | binder | model | — |
| W08 | `query_file_in_worktree` | warning | — | CLI | product | — |
| W09 | `stdin_question_marks` | warning | — | CLI, lexer | product | — |
| W10 | `link_state_none_admitted` | warning | — | binder, executor | model | — |
| N01 | `deleted_id` | notice | — | executor | model | — |
| N02 | `staged_view` | notice | — | executor | model | — |
| N03 | `composite_views` | notice | — | executor | model | — |
| N04 | `reflog_time_resolved` | notice | — | executor | model | — |
| N05 | `diff_base_lca` | notice | — | executor | model | — |
| N06 | `id_on_other_branch` | notice | — | executor | model | — |
| N07 | `reverse_has_edges` | notice | — | executor | model | — |
| N08 | `endpoint_pair_count` | notice | — | binder | model | — |
| N09 | `duplicate_rows` | notice | — | executor | renderer | — |
| N10 | `division_by_zero` | notice | — | executor | model | — |
| N11 | `file_alias` | notice | — | executor | model | — |
| N12 | `never_allocated_id` | notice | — | executor | model | — |

W09 and N12 are added by this chapter (open points 5 and 6); W09's name is the one [LQ/lexical] open point L-7 proposed. W10 is
[50 §5.2]'s, added by the A1 re-review's S-05 disposition after this chapter's W09. The W
and N names are added by this chapter for the JSON form; [50 §5.2] gives only codes and descriptions (open point 7). The
detection points of the lexer and parser codes are [LQ/lexical §1] and [LQ/grammar-v1.ebnf §P, §R]; this chapter owns their
texts.

5.2. **Lexer and parser errors (E0xx, E113, E114, E308).** One row per detection case of [LQ/lexical §1], [LQ/lexical §5]–§7 and
[LQ/grammar-v1.ebnf §P, §R]; the case names the rule it implements.

| Code (case) | Message (line 1) | Inline | Help |
|---|---|---|---|
| E001 (generic) | `expected <list>, found <token>` — `<list>` the expected token spellings in back-quotes (at most 5 in text, 10 in JSON `expected`), `<token>` the found token in back-quotes or `end of input` | none | none |
| E001 (a character that starts no token, lexical §5.2) | `<char> cannot start a token here` | none | for `^`, `~`, `@`: `revisions follow USE, TX ON, IF TIP or a revision argument of diff, log, changes, history, across or violations` |
| E001 (`$` without a name, lexical §5.5) | `$ must be followed by a parameter name` | none | `parameter names start with a letter or _` |
| E001 (`!` without `=`) | `! is not an operator` | `write NOT <expr>` or `write <>` | none |
| E001 (empty back-quoted name, lexical §5.4) | `an empty back-quoted name` | none | none |
| E001 (revision expected, lexical §7.2) | `expected a revision: a ref, c<hex>, s<seq>, HEAD or $param` | none | none |
| E001 (quote after `USE`, `ON`, `IF TIP`, P2) | `a revision here is written unquoted` | `write <revspec without quotes>` | none |
| E001 (suffix after a parameter, lexical §7.2) | `a $param revision takes no suffix` | none | `pass the full revision in the parameter` |
| E001 (bare `@`, lexical §7.3) | `a bare @ is not a revision` | `write HEAD, <ref>@<n> or <ref>@<datetime>` | none |
| E001 (four or more dots; empty revision list; a range inside a list, lexical §7.4) | `a range is a..b or a...b` or `a revision list holds one or more revisions and no ranges` | none | none |
| E001 (comment inside a revision, lexical §7.6) | `a revision ends at whitespace or a comment` | none | none |
| E001 (`WHERE` after `RETURN`, [50 §2.7], P17) | `WHERE after RETURN filters nothing` | `write WITH <items> WHERE <expr> RETURN <items>` with the query's own items and predicate | `filter rows or groups with WITH ... WHERE before RETURN` |
| E001 (Cypher `WITH` order, P16) | `WITH takes WHERE before ORDER BY and LIMIT` | `write WITH <items> WHERE <expr> ORDER BY <keys> LIMIT <n>` | none |
| E001 (`USE` after a clause, P19) | `USE must start its query part` | `write USE <revspec>` at the start of the part | none |
| E001 (group without a quantifier, P6) | `a parenthesised path group needs a quantifier` | `add +, * or {m,n} after the group` | none |
| E001 (a `{` after an edge or a group that is not a quantifier and holds no integer, P6) | the generic form with `<list>` = `` `{m,n}`, `{m,}`, `{m}`, `{,n}` `` in this order and `<token>` = `` `{` `` (spec sync 2b) | none | none |
| E001 (`EXPECT` with min > max, [LQ/canonical-ast] N5) | `EXPECT <m>..<n> has m > n` (spec sync 2b) | none | none |
| E001 (empty subquery, P8) | `EXISTS { } needs a pattern or clauses` | none | none |
| E001 (`CALL` without `YIELD` before another clause, P7) | `a CALL followed by more clauses needs YIELD with named columns` | none | none |
| E001 (a repeated `TX` option, P12) | `TX takes <option> once` | none | none |
| E001 (`moirai tx` input not starting with `TX`, P1) | `moirai tx and the write tool take one TX { ... } block` | none | `reads go through moirai q or the query tool` |
| E001 (`CREATE` of a node inside a `MATCH ... EXPECT` list, P11) | `node creation is a statement of its own` | `end the MATCH statement with ; before CREATE` | none |
| E001 (`:name` as a parameter, [50 §2.2] rule 7, §R) | `:<name> is not a parameter` | `write $<name>` | none |
| E001 (nesting, [50 §5.2], P13) | `nesting deeper than 64 levels` | none | `split the query or flatten the expression` |
| E001 (binder: an unaliased `WITH` item that is not a bare variable, [LQ/canonical-ast] C-21) | `WITH <expr> needs a name` | `write WITH <expr> AS <name>` | none |
| E001 (binder: parts of a set operation with different column counts) | `<op> parts return <n> and <m> columns` — `<op>` the operator as written; parts are matched by position, their names may differ, and the result takes the first part's column names (spec sync 2b) | none | none |
| E002 | `unterminated <what>` — `<what>` is `string`, `block comment` or `back-quoted name`; the span is the opening delimiter ([LQ/lexical §3], §5.4, §5.7: strings and names end on their line) | none | `close it with <delimiter> on the same line; write a line break as \n` for strings |
| E003 (UTF-8, lexical §2.1) | `invalid UTF-8 at byte <n>` | none | `send UTF-8; from PowerShell pass the query with -f FILE or through the query tool` |
| E003 (node or uid literal, lexical §5.8) | `<token> is not a node literal` or `#<digits> is out of range (1 to 4294967295)` | none | `write #<digits> or #u:<32 lower-case hex>` |
| E003 (number, duration, range, lexical §5.6) | `<token> is out of range` or `<token> runs into <char>` or `an exponent needs digits` | none | `separate the token from the next one` for the run-in form |
| E003 (escape, lexical §5.7) | `unknown escape \<c>` or `\u{<hex>} is not a Unicode scalar value` | none | `write \u{<hex>}` |
| E003 (control character, lexical §5.2) | `control character U+<hex> inside a string or name` | none | `write \u{<hex>}` |
| E003 (revision, lexical §7.2–§7.6) | `a revision has an upper-case letter` or `<revspec> is malformed` or `<field> of <datetime> is out of range` or `a suffix count is out of range` | none | `ref names are lower case; HEAD is upper case` for the first form; `write YYYY-MM-DD, YYYY-MM-DDTHH:MM or YYYY-MM-DDTHH:MM:SSZ` for a datetime |
| E004 | per §6 | per §6 | per §6 |
| E005 | `one statement per call; a second statement starts here` | none | `send each statement in its own call, or put writes in one TX { ... } block` |
| E006 | `<what> writes; q and the query tool only read` — `<what>` is the keyword (`SET`, `CREATE`, `INSERT`, `DELETE`, `REMOVE`, `MOVE`, `REOPEN`, `PATCH`, `RESOLVE`, `DEFINE`, `DROP QUERY`, `ASSERT`, `TX`) or `CALL <fn>` for a `tx.*` name | none | `send writes with moirai tx or the write tool` |
| E007 | `MATCH in TX needs EXPECT` | `add EXPECT <n>, a range a..b, <= n or >= n` | `EXPECT turns "matched nothing" into a guard failure` |
| E009 | `TX block has no write statement`; the span is the `}` that closes the block ([LQ/grammar-v1.ebnf §P.12]) | none | `read with moirai q or the query tool; TX is for writes` |
| E113 (parser: a path variable, `p = <pattern>`) | `path variables are not in LQ v1` | `write CALL blockers(<id>, transitive: true) YIELD blocker, depth, via` with the pattern's anchor id, else `#N` | `sets of endpoints need no path variable` |
| E113 (binder: an edge variable on a quantified edge, [LQ/grammar-v1.ebnf] O-8) | `<var> names a quantified edge; LQ has no path or list values` | `write a quantified group: (x)((a)-[<var>:<T>]->(b) WHERE ...){m,n}(y)` | none |
| E114 | one of: `quantifier {<m>,<n>} has m > n`; `quantifier bound <n> is above 4294967295`; `one edge takes one quantifier` (e.g. `-[:T*2]->{1,3}`, [LQ/gql-spelling §2.1]); `{<text>} after an edge is not a quantifier` ([LQ/grammar-v1.ebnf §P.6]) | none | none |
| E308 | `USE inside EXISTS {} or COUNT {} is not allowed` | none | `compare versions with diff(), across() or a composite query` |

E114's bound is [LQ/lexical §8]'s (0 … 2^32 − 1).

5.3. **Binder errors (E1xx; E113's binder case is in §5.2).**

| Code | Message (line 1) | Inline | Help |
|---|---|---|---|
| E101 | `kind <kind> has no field <field>`; for a kind set, `kinds <k1>\|<k2> have no field <field>` | `did you mean <s>?` when a candidate within Levenshtein distance 2 exists in the current schema | `<kind> fields: <list> (CALL schema(kind: '<kind>'))` |
| E101 (`open`, fixed hint, [50 §3.2]) | `kind <kind> has no field <field>` with `<field>` = `open` | `write <v>.status = 'open', or <v>.unfinished for any unfinished status` | none |
| E102 | `<value> is not a value of <field> (<enum>)` | `did you mean <s>?` when one exists | `<field> values: <list>` |
| E102 (`labels()`, [50 §2.5]) | `<value> is not a kind; task labels are the field` | `write <value> IN <v>.labels` | none |
| E103 | one of: `<left type> <op> <right type>: the types do not match`; `counter <field> changes only by an increment`; `integer overflow in <expr>`; `<var> is a bound variable, not a revision`; `<expr> is <type>, not a node or edge; it has no property <prop>` (a property read on a value that is not a node or an edge, e.g. a yielded `int` column; spec sync 2a) | for the counter: `write SET <v>.<field> = <v>.<field> + <k>` | none |
| E104 | `unknown edge type <edge>` | `did you mean <s>?` when one exists (reverse aliases and snake_case stored names are candidates) | `edges from <kind> to <kind>: <list>` |
| E105 | `unknown kind <kind>` | `did you mean <s>?` when one exists | `kinds: task doc note rule decision question finding verdict measurement artifact run lane area` plus the branch's project kinds |
| E106 | `<edge> links <src kinds> -> <dst kinds>; <a> and <b> are <kinds>` | `write <reversed pattern>` when the reversed pattern types, else none | `edges from <kind> to <kind>: <list>` |
| E106 (`DEPENDS_ON` between tasks, [50 §2.5]) | `DEPENDS_ON links doc sections (doc -> doc); <a> and <b> are tasks` | `between tasks write (d)-[:BLOCKS]->(t) (d finishes before t) or (t)-[:BLOCKED_BY]->(d)` | none |
| E107 | `parent is ambiguous in a pattern` | none | `write (child)-[:CHILD_OF]->(parent), (parent)-[:PARENT_OF]->(child) or the property n.parent` |
| E108 | `bare word <word> is not a value of <field>` | `did you mean <s>?` | `<field> values: <list>` |
| E108 (a bare word nothing coerces, [LQ/canonical-ast §5.5]; spec sync 2a) | `bare word <word> is not a bound variable or a value` | none | `write '<word>' for a text value, or bind <word> in a pattern first` |
| E109 | one of: `unknown function <fn>`; `<fn> takes <n> arguments, got <m>`; `<fn> has no argument <name>`; `<name> is a named mutation; call it inside TX`; `inside TX, CALL names a named mutation` ([LQ/grammar-v1.ebnf §P.12]); `<name> is a relation; call it with CALL <name>(...) YIELD ...` (a revision relation written as a scalar function, [LQ/lexical §4.2]) | `did you mean <s>?` when one exists | `CALL queries() lists the named queries; the built-ins are in reference-ql.md` |
| E109 (a `YIELD` of a column the callee does not yield; spec sync 2a) | `<fn> yields no column <name>` | `did you mean <s>?` when one exists | `<fn> yields: <columns>` ([LQ/std] signatures) |
| E109 (a call of a named query that does not bind; spec sync 2a) | `<name> does not bind: <code> <message>` — `<code>` and `<message>` are the callee's first diagnostic ([F19 §12.5.3]) | none | `moirai q --show-query <name> prints its text` |
| E109 (a call cycle among named queries, [F19 §12.5.4]; spec sync 2a) | `<name> is on a call cycle: <a> -> <b> -> <a>` — the cycle as [F19 §12.5.4] finds it, starting and ending at its least name | none | `remove one of the calls` |
| E109 (a reserved name in `DEFINE QUERY` or `DROP QUERY`, [LQ/std §2.1]; spec sync 2a) | `<name> is reserved: std.* and tx.* belong to moirai, and a query name is not an LQ keyword` | none | `choose another name; CALL queries() lists the names in use` |
| E110 | one of: `<param> must be <type>; got <value>`; `<query> has no parameter <param>`; `<query> needs <param>` | none | the signature: `<query>(<param>: <type>, ...)`; when a required node parameter is missing from argv, `'#' starts a shell comment; write 40` ([80 §4] T9) |
| E110 (an unknown `param_decl` type, [LQ/std §2.2]; spec sync 2a) | `<type> is not a parameter type` | `did you mean <s>?` when one exists | `parameter types: node, int, float, bool, text, rev, timestamp, duration, range<int>, list<node>, list<int>, list<text>, list<rev>` |
| E110 (a `$param` with no value in an ad-hoc query, which has no query name; spec sync 2a) | `$<param> has no value` | none | `pass <param>=<value> after the query (the query tool: params)` |
| E111 | `<id> was never allocated in this store`; `uid <id> is not in this store` | none | `next id is <id>` for the first form |
| E112 | one of: `aggregate <fn>() inside WHERE`; `aggregate <fn>() inside another aggregate`; `aggregate <fn>() outside RETURN and WITH` (any other position: a pattern, `UNWIND`, `SET`, a `CALL` argument, a `TX` statement; spec sync 2a); `GROUP BY must list exactly the non-aggregate items: <list>` | none | `filter aggregates with WITH ... WHERE` for the first form; `aggregate in RETURN or WITH, then use the alias` for the third |
| E115 | `<target> is <class> and cannot be written here` — `<class>` is `derived`, `runtime`, `tree-derived`, `an observation field`, `an identity field`, `a root-node field`, `created by capture`, `an edge property` or `a file operation` | none | the verb that writes it, by class: `derived` and `runtime`: `it follows from the graph; write the fields it is derived from`; observation, identity and root-node fields, `removed` status: `moirai file mv, moirai file rm or moirai links fix`; `AT` creation: `moirai link ID --at SPEC (the write tool: tx.link_file)`; `CREATE (x:artifact ...)`: `moirai file add PATH`; edge properties: `moirai links fix ID --repin or --pin`; the five file named mutations inside a `TX` block ([LQ/std §7.4]): `send it alone: the write tool with name and params, or its CLI verb` |
| E116 | `<var> is a step variable of a quantified group and is not visible outside it` | none | `bind the endpoints outside the group: (x)((a)-[...]->(b)){m,n}(y)` |
| E117 | `a reflog position or anchor handle differs per store` (a reflog revision `REF@n`/`REF@<datetime>`, or an anchor handle `aN` compared in a definition; [50 §4.4] as amended for the A1 re-review's S-02) | none | `use a $param, a c<hex> commit id, a ref name or the anchor's fields` ([50 §4.4], verbatim) |
| E118 | `a comparison with NULL is never true`; located at the comparison operator for `= NULL`, `<> NULL` and `!= NULL`, and at the property-map key for `{<p>: null}` (spec sync 2b) | `write <x> IS NULL` for `=`, `write <x> IS NOT NULL` for `<>` and `!=`, `write WHERE <v>.<p> IS NULL` for `{<p>: null}` | none |

5.4. **Planner, view and executor errors (E2xx, E3xx, E5xx).**

| Code | Message (line 1) | Detail lines | Help |
|---|---|---|---|
| E201 | `<what> visits at least <N> <unit> (budget <k>=<N>; est. ~<N> reachable)` — [50 §5.6]'s text | none | `add an anchor, a hop bound, or --budget <k>=<N>` |
| E202 | `ORDER BY without LIMIT over at least <N> rows needs more than mem=<N> B` | none | `add LIMIT n or an anchor` |
| E301 | `unknown revision <revspec>`; `<revspec> matches <n> commits` | the candidates, one per line: `<c8> rev <rev> <ref>` (at most 5) | `write more hex digits` for the prefix form |
| E302 | `<prop> uses <what>, which exists only at a branch tip; the view is <revspec> (as-of)` — `<what>` is `leases and markers`, `the file tree` or `git ancestry`; with no resolvable tree: `<prop> needs a resolved tree`; for `violations()` on a view that is not a staging ref ([50 §3.9] item 9; spec sync 2a): `violations() reads a staging ref; the view is <revspec>` | none | inline for `ready`: `use <v>.unblocked (structural, valid at any version)`; for no tree: `pass --tree DIR (the query tool: tree)`; for `violations()`: `read it on the staging ref: USE <staging ref> CALL violations() YIELD key, class, detail, suggested` |
| E303 | `as-of <revspec> needs <N> ops replayed; the cap is <N>` | none | `moirai tag <name> <revspec> --pin, then query the tag` |
| E304 | `<n> views requested; the budget is refs=<n>` — `<n>` counts the distinct views the query opens, compared by their C-AST revision nodes ([LQ/canonical-ast §5.6]), so a revspec written twice counts once: each part's view (its `USE`, else the default view, counted once), each element of `across(refs:)`, both ends of a `diff()` range and, for a three-dot range, its base, and the `ref` of `violations()`; `log`, `changes` and `history` walk commits and open no further view (spec sync 2a) | none | `--budget refs=<n> (at most <ceiling>)`, `<ceiling>` being the caller's role cap `query.caps.<role>.refs` ([CFG §10.5]; 8 for an agent role, 80 for the orchestrator and the owner; pass 1, A1-53) |
| E305 | `<revspec> is read-only: <why>` — `<why>` is `a commit`, `a tag`, `an orphans ref` (a parked ref, [F12 §2.2]), `a past view`, `field <field> is masked on plan branches`, `a staging ref accepts only RESOLVE` or `an import ref accepts only RESOLVE` (a plain write on `import/*`; `RESOLVE` lands there, [F12 §3.7], §9.6 and its open point 21; spec sync 2a) | none | `write on a branch tip: TX ON <ref> { ... }` |
| E306 | `cursor <cursor> belongs to another query or is damaged` | none | `run the query again without --cursor` |
| E501 | `budget: work <N> exhausted after <id>`; for a write whose phase-1 working set exceeds `wmem` ([F17 §4.4] W2, [CFG §10.5]): `budget: wmem <N> B exceeded by statement <i>`; for an agent write whose changeset exceeds the inline bound ([F17 §4.4] W1, W4): `budget: the changeset needs <N> B; an agent write holds at most <N> B (store.commit.inline-max-bytes)`; for the deterministic caps of a `TX` block ([CFG §10.5]; spec sync 2b): `budget: the block has <n> statements; the cap is <N> (tx.max-statements)` and `budget: the block writes more than <N> ops (tx.max-ops)` | for `wmem`, the inline bound and the caps: `nothing was written` | `add an anchor, a LIMIT, or --budget work=<N>`; for `wmem`: `--budget wmem=<N> (at most <ceiling>), or split the block` (pass 1, P1-11); for the inline bound: `split the block into smaller TX blocks; a wmem raise does not help` (pass 1, round 1, P1-11); for the caps: `split the block into smaller TX blocks` |
| E502 | `budget: mem <N> B exhausted` | none | `add an anchor or a LIMIT, or --budget mem=<N>` |
| E503 | `deadline of <n> ms reached` | none | `add an anchor or a LIMIT` |
| E504 | `cancelled` | none | none |
| E505 | `fs: <N> units used; <n> links unverified` | none | `--budget fs=<N>` |

A resumable plan cut by E501–E505 prints its rows and the continuation footer of [LQ/envelope §6.4] instead of this error text:
the footer *is* the error's text ([50 §5.10] "rows so far + footer + cursor, exit 10"), and the JSON envelope carries both
`data` and an `errors` entry. A blocking plan prints this error text with the EXPLAIN excerpt of [LQ/envelope §10] as detail
lines (at most 6), no rows and no cursor ([50 §5.6]).

5.5. **Transaction errors (E401–E411).** Every E4xx text is raised before any append ([50 §3.10] item 5). The texts of a
refusal of one statement — E401, E403, E404, E405, E409, E410 and E406's statement case — name it by its 1-based index in
the block (for a deferred validator, E405, the block's last statement; for the `Schema` command, [API §9.8], the 1-based
item index) and report `nothing was written` as their closing detail line; so does E402. The refusals of the block or the
call as a whole — E406's unleased, MCP and safelist cases, E407, E408 and E411 — name no statement and have no closing line,
as their rows show (spec sync 2b). E401 and E402 are decided against
the tip the block commits on: `MATCH` targets are re-evaluated under the writer byte whenever a commit, a marker or a lease
touched what they read ([50 §3.10] item 3 and [50 §5.9] step 4 as amended for the A1 re-review's S-06, [AR §4.5] step 7), so a
runtime predicate such as `t.ready` or `t.claimed` in a target is checked against the current runtime tables.

| Code | Message (line 1) | Detail lines | Help |
|---|---|---|---|
| E401 | `statement <i> matched <n> bindings, expected <expect>` | per literal target and per matched target (at most 10): `<id> now: ` + each field the statement's `WHERE` reads as `<field>=<value>` joined by one space + ` rev=<rev> (changed at <c8> by <actor> on <ref>: "<message>")`; then `nothing was written` | `re-read with moirai q show ids=<ids>` |
| E402 | `<ref> moved: IF TIP <c8>, the tip is <c8> (rev <rev>)`; `IF TARGETS <digest>: the targets now digest to <digest>` | `nothing was written` | `run the block with DRY again and apply its digest` |
| E403 | `statement <i>: ASSERT is false` | the assertion's `ELSE` text as `"<text>"` when present; `nothing was written` | none |
| E404 | `statement <i>: <id> <from> -> <to> refused: <why>` — `<why>` is `<n> children are open`, `verdict <id> <outcome> gates it`, `the removed text is not in <id>.body`, `done -> open needs REOPEN` or the machine's rule name | the open children or gating verdicts (at most 10); `nothing was written` | for `done -> open`: `write REOPEN <id> REASON '<reason>'`; else none |
| E405 | `statement <i>: <rule> would be violated` — `<rule>` is `acyclic precedence (I5')`, `forest depth 12 (I4)`, `live endpoints (I2)`, `one active superseder (I6)`, `canonical duplicate target (I7)`, `named-query cycle (QueryCycle)`, `at most one <edge> (cardinality)` (`runs_in`, `answers`), `named queries bind (QueryInvalid)`, `schema conformance (I11)` ([F19 §12.4], its open point 16) | the cycle or the offending edge, one line; `nothing was written` | none |
| E406 | `statement <i>: role <role> may not <action>` | `nothing was written` | the refusing rule as [RULES/role-write-policy] WZ-001 renders it: `no <table> row lets <role> <op> <kind>` followed by `.<field>` when the op sets a field, `<table>` being the `role-write-policy` table whose check refused (`role-statements`, `role-create`, `role-values`, `role-fields`, `role-status` or `role-edges`, WR-009), e.g. `no role-fields row lets developer set decision.authority` (spec sync 2a) |
| E406 (unleased, frozen) | `this write needs a lease` | none | `an orchestrator presents its session lease with --lease (mint it once per session: moirai claim --role orchestrator --session)`. The design writes the text as one line; this row renders it split at its semicolon into message and help, with no other change ([F19 §11.3]; pass 1, A1-51) |
| E406 (MCP, CLI-only statements) | `<stmt> runs only through moirai tx, for the orchestrator and the owner` — `<stmt>` is `node DELETE`, `RESOLVE`, `DEFINE QUERY` or `DROP QUERY` | none | none |
| E406 (safelist) | `role <role> may run only named queries (query.safelist.<role> = named-only)` | none | `moirai q --list lists them` |
| E407 (missing) | `this write needs lease <lease>` | none | `pass --lease <lease>` |
| E407 (stale) | `lease <lease> is stale: <id> is held by <holder>` | none | `claim the task again or ask the orchestrator` |
| E407 (branch) | `--branch <ref> differs from lease <lease>'s branch <ref>` | none | `drop --branch; the lease fixes the branch` |
| E407 (declared agent, [90 §4.1]) | `declared agent <agent> differs from lease <lease>'s holder <holder>` | none | `drop --agent, or pass your own lease` |
| E407 (bound lease, [90 §4.1]) | `<lease> is bound to <identity>; pass your own lease` | none | none |
| E408 | `key <key> was used for a different payload` or `key <key> was used on <ref>`; for a default key ([AR §6.4]): `this write's default key was used for a different payload` or `this write's default key was used on <ref>` | `original: rev <rev> <c8> on <ref>` when the entry records a commit; `original: recorded without a commit` otherwise ([API §7.4]; pass 1, A1-52) | `use a new key for a new write`; for a default key: `pass an explicit key, or --no-dedupe` |
| E409 | `statement <i>: DELETE <id> refused: restricted references`; for a root node in the deleted set ([F08 §11.3], [RULES/delete-policy-matrix] DP-010): `statement <i>: DELETE <id> refused: <id> is a root node`, naming the root node of the deleted set with the smallest `#N`; for a live lease without `RELEASE` (I32′, [RULES/delete-policy-matrix] DP-005, [API §9.1]): `statement <i>: DELETE <id> refused: leased by <holder> on <ref> (<lease>)`, naming the first live task lease on a node of the deleted set in `LEASES` order ([F11 §6]: `#N`, then lease id), with `<ref>` its branch, and preceded by that node's `<id> ` when it is not the target (`DELETE #40 refused: #41 leased by dev#2 on lane/y (L-19)`); for an invalid replacement ([RULES/delete-policy-matrix] DP-007): `statement <i>: DELETE <id> refused: replacement <id> <why>`, `<why>` being `is not live`, `is in the deleted set` or `does not fit <id> -[:<edge>]-> <id>` (the first re-pointed edge, drawn as re-pointed, whose end does not accept the replacement's kind) | for restricted references, the references (at most 10): `<id> -[:<edge>]-> <id>`; for a lease, every other live task lease on the deleted set in the same order (at most 10): `<id> leased by <holder> on <ref> (<lease>)`; for a replacement that does not fit, every other such edge (at most 10): `<id> -[:<edge>]-> <id>`; edges are listed, and the first one chosen, in (source `#N`, edge name bytewise, destination `#N`) order; then, in every case, `nothing was written` | `add POLICY CASCADE or POLICY REPARENT, or REPLACED BY <id>`; for a root node: none; for a lease: `add RELEASE to release the lease; a triage note goes to the tombstone (moirai rm --release)`; for a replacement: `name a live replacement outside the deleted set that every re-pointed edge accepts` |
| E410 | `statement <i>: UNLESS EXISTS matched <n> nodes; create-or-bind needs at most 1` | the matches (at most 10); `nothing was written` | `narrow the UNLESS EXISTS pattern` |
| E411 | `free-form TX is refused for a model with the <profile> profile` — `<profile>` the caller's profile whose `query.safelist.model.<profile>` is `named-only` or `dry-targets` (`unknown` by default; spec sync 2b) | `use the named mutation <name> (the write tool: name and params)` when one matches the block's statements, else `no named mutation matches; ask the orchestrator`; with `query.safelist.model.unknown = dry-targets`: `or run the block with DRY and apply it with IF TARGETS` | none |

E411 is the one new code of [90 §10.1] (the gap of PLAN §3.3, row "unknown-model write error code number"): E4 because it is a
write refusal decided before execution, exit 6 because it is a policy refusal like E406 ([AR §7.1]: 6 covers role policy).
Order (spec sync 2b): a message refusal (`bad_value`, [API §9.1]) and E402 are checked before E411; E406, static or on the
candidate, wins over E411 (§3.1's code order); E411 comes before every statement's own refusal.
The two exit-5 texts of [90 §10.1] are the E407 rows "declared agent" and "bound lease"; the E406 unleased text of [AR §7.3] and
[90 §4.3] is frozen, rendered as a message and a help line split at its semicolon (open point 2; [F19 §11.3]).

E409 has four cases (pass 1, round 3, closure NC-8). Of the root node, the lease, the restricted references and the
replacement, the one whose precondition comes first in [RULES/delete-policy-matrix §4] is printed: the root node (DP-010)
before the lease (DP-005), the references (DP-006) and the replacement (DP-007). The lease case prints [AR §5d.3]'s text (`leased by dev#2 on lane/y (L-19)`) after
the statement prefix, and its help names the one fix I32′ allows, `RELEASE` (`tx.rm`'s `--release`, [LQ/std §7.2]).

5.6. **Warnings and notices.** `<v>` is the query's variable name; texts with a count sum it over the result (§4.2).

| Code | Text (first line; continuation lines indented five spaces) |
|---|---|
| W01 | `W01: <n> rows excluded because <field> is absent; use coalesce(<expr>, 0) or <expr> IS NULL` ([50 §3.3]) |
| W02 | `W02: <mode> changes nothing: fixed parts bind distinct edges and quantified parts bind endpoint pairs` |
| W03 | `W03: t.done includes cancelled; write t.status = 'done' for completed only` ([50 §3.8], verbatim; `t` is replaced by `<v>`) |
| W04 | `W04: filtering on body scans <n> bodies (charged to the work budget)` |
| W05 | `W05: derived state recomputed for <n> nodes at <revspec>` |
| W06 | `W06: live result: rows may have entered or left since page 1` ([50 §3.5], verbatim) |
| W07 | `W07: hand-derived readiness misses inherited and flagged blockers, markers, leases, defer_until and containers; use <v>.unblocked (structural) or <v>.ready (dispatchable now)` ([50 §3.2]) |
| W08 | `W08: <file> lies inside a working tree and will show as an untracked file; pass the query on stdin with a quoted heredoc, or put the file outside the tree` |
| W09 | `W09: this text came through a PowerShell pipe and contains '?' where other characters were expected; pass it with -f FILE or through the query tool` — fires when [LQ/lexical §10.1]'s predicate P? holds and the parent process is PowerShell |
| W10 | `W10: <n> rows passed <op> because link_state(<v>) is 'none' (no AT edge); to keep linked nodes only, add EXISTS { (<v>)-[:AT]->() }` — fires on `<>` or `NOT IN` over `link_state()` of a node variable that is neither an `AT` edge variable nor an artifact ([50 §2.6], [50 §5.2]); `<op>` is `<>` or `NOT IN` |
| N01 | `N01: <id> is deleted in this view (rev <rev> <c8> by <actor> "<reason>" -> <id>)` (`-> <id>` only with a replacement; [50 §3.6]) |
| N02 | `N02: <ref> is a staging ref: read-only except RESOLVE`, continuation `resolve: TX ON <ref> { RESOLVE '<key>' TAKE ... }, then moirai merge --continue <src> --into <dst>` |
| N03 | `N03: <n> parts read <n> views; each row comes from the view of the part that produced it` |
| N04 | `N04: <ref>@<datetime> resolved to rev <rev> <c8>` |
| N05 | `N05: <revspec> is not an ancestor of <revspec>; the diff starts at their LCA rev <rev> <c8>` |
| N06 | `N06: <id> is not in this view: created on <ref> at rev <rev> (<c8>), not merged into <ref>`, continuation `USE <ref>, or CALL across(refs: [<ref>, <ref>], ids: [<id>])`; when the creating branch is deleted: `created on deleted branch <ref> at rev <rev> (<c8>)` ([50 §3.6]) |
| N07 | `N07: nothing matched, but <n> <edge> edges point the other way: (<b>)-[:<edge>]->(<a>), also written (<a>)-[:<alias>]->(<b>)` ([50 §2.9]; the alias clause only when the kind has a reverse alias) |
| N08 | `N08: count(*) over a quantified pattern counts (<x>, <y>) endpoint pairs, not paths` ([50 §2.9]; `count(*)` is replaced by the aggregate as written) |
| N09 | `N09: <n> duplicate rows on this page; RETURN DISTINCT removes them` |
| N10 | `N10: division by zero gave absent in <n> rows` |
| N11 | `N11: '<path>' is an old path of <id>; it is now '<path>'` |
| N12 | `N12: <id> was never allocated in this store` |

5.7. **Code-specific JSON keys** (after `detail`, in this order when present):

| Code | Keys |
|---|---|
| E401 | `"statement":<int>`, `"expect":<string>` (the bound as written, e.g. `"1"`, `"2..5"`, `">= 3"`), `"matched":<int>`, `"current":[{"id":"#N","kind":<string>,"status":<string or null>,"title":<string or null>,"rev":<int>,"changed_by":{"commit":<commit>,"actor":<string>,"ref":<string>,"message":<string>}}...]` (one object per detail line, in its order; not a full node object; spec sync 2b), `"written":false` |
| E402 | `"statement":null`, `"tip":<commit>`, `"expected_tip":<commit or null>`, `"targets":<32 hex or null>`, `"written":false` |
| E403, E404, E405, E406, E409 (other cases), E410 | `"statement":<int>`, `"written":false`; for E405 of a deferred validator the block's last statement, for the `Schema` command the item index (§5.5); for E406's verb-level cases (unleased, MCP, safelist) `"statement":null` (spec sync 2b) |
| E409, lease case | `"statement":<int>`, `"leases":[{"node":"#N","id":<string>,"holder":<string>,"branch":<string>}...]` (the line-1 lease, then the leases of the detail lines, at most 10 more, in the text's order; spec sync 2b; `id`, `holder` and `branch` as the `lease` object of [LQ/envelope §7.4]), `"written":false` (pass 1, round 3, closure NC-8) |
| E407 | `"lease":<string>`, `"holder":<string or null>`, `"written":false` |
| E408 | `"key":<string or null>` (`null` for a default key), `"original":{"rev":<int>,"commit":<commit>,"ref":<string>}` or `null` when the entry records no commit ([API §7.4]; pass 1, A1-52) |
| E411 | `"mutation":<string or null>`, `"written":false` |
| E201, E202, E303, E304, E501–E505 | `"budget":{<key>:<int>...}` as [LQ/envelope §7.5]; for E501 of `tx.max-statements` `{"statements":<n>,"statements_limit":<cap>}` and of `tx.max-ops` `{"ops":<n>,"ops_limit":<cap>}`, `<n>` the count when the cap was passed (spec sync 2b) |
| E301 | `"candidates":[<commit>...]` |

A `<commit>` in JSON is `c` followed by the 64 lower-case hex digits of the commit id ([LQ/envelope §7.3]; the A1 dispositions of
A-m7 and S-24).

## 6. E004: forms that are not LQ, and their rewrites

Every row is refused with E004 in every grammar mode; the detection point of each is [LQ/grammar-v1.ebnf §R], and
[LQ/grammar-v1.ebnf §G] (with [LQ/gql-spelling §3]) adds the Cypher-only spellings the strict-GQL spelling mode refuses.
Message: `<form> is not in LQ`. Inline: the rewrite, as `write …` when it is mechanical and `use …` otherwise. Help: none unless
shown. The replacement column follows §R's.

| Written | Inline | Mechanical |
|---|---|---|
| `MERGE` | `write CREATE (...) UNLESS EXISTS { ... }` | no |
| `DETACH DELETE`, `NODETACH DELETE` | `write DELETE <x>; the schema's per-edge policies run (add POLICY or REPLACED BY)` | no |
| `shortestPath(`, `allShortestPaths(`, `nodes(`, `relationships(` | `write CALL blockers(<id>, transitive: true) YIELD blocker, depth, via` | no |
| `SHORTEST`, `ANY SHORTEST`, `ALL SHORTEST`, `ANY`/`ALL` path selectors after `MATCH` | `remove the selector: quantified parts bind endpoint pairs` | yes (deletion) |
| `REPEATABLE ELEMENTS` | `remove it: patterns bind distinct edges and quantified parts bind endpoint pairs` | yes (deletion) |
| `SKIP n`, `OFFSET n` | `use the next cursor: --cursor K (the query tool: cursor)` | no |
| `CALL { ... }` subquery | `use EXISTS { }, COUNT { } or WITH` | no |
| `NEXT` | `use WITH` | no |
| `FOREACH` | `use one SET per MATCH target` | no |
| `FOR x IN list` | `write UNWIND <list> AS <x>` | yes |
| `LET x = e` | `write WITH *, <e> AS <x>` | yes |
| `FILTER e` | `write WITH * WHERE <e>` | yes |
| list comprehension `[x IN l WHERE p \| e]`, `[... \| ...]` | `use any(), all(), none(), IN, COUNT { } or UNWIND` | no |
| index `x[i]`, slice `x[a..b]` | `use IN or any(), all(), none()` | no |
| `single(x IN l WHERE p)` | `write COUNT { ... } = 1` | no |
| `a XOR b` | `write (<a> OR <b>) AND NOT (<a> AND <b>)` | yes |
| `a % b` | `write <a> - <b> * toInteger(<a> / <b>)` | yes |
| `:a:b` (a second label) | `write :<a>\|<b> (a node has one kind)` | yes |
| `x.f(args)` (a method call) | `write <f>(<x>, <args>)` (e.g. `applies(r, 'crates/**')`) | yes |
| `=~` | `use CONTAINS, STARTS WITH, glob_match() or search()` | no |
| `timestamp()` | `write now()` | yes |
| `apoc.*`, `gds.*`, `db.*`, `dbms.*` procedures | `use the built-in table functions; CALL schema() lists kinds, fields and edges` | no |
| `LOAD CSV`, `SHOW ...`, `CREATE INDEX`, `CREATE CONSTRAINT`, `DROP INDEX`, `DROP CONSTRAINT` | `use CALL schema(); write data with moirai apply` | no |
| `<-[...]->` (two arrowheads) | `write -[...]- (either direction)` | yes |
| `x IS LABELED k` (a GQL label test) | `write <x>:<k>` | yes |
| `CAST(x AS t)` (a GQL conversion) | `write toInteger(<x>), toFloat(<x>) or toString(<x>)` | yes for those three types |

Path variables (`p = …`) are E113, not E004 ([50 §2.8]). Write keywords in a read are E006. `:name` as a parameter and a bare `@`
in a revision are E001 (§5.2), as §R detects them. The last two rows are §R's O-13 additions.

## 7. The ten mistakes of [50 §2.9]: frozen outcomes

WP-22's `lq/` fixtures assert these outcomes; the reading echo's text is [LQ/envelope §4]'s.

| # | Query as written | Outcome |
|---|---|---|
| 1 | `WHERE t.status = 'open' AND NOT (t)<-[:BLOCKS]-()` | parses as `NOT EXISTS {…}`; runs; W07 |
| 2 | hand-written `NOT EXISTS { … b.status <> 'done' }` readiness | runs; W07 |
| 3 | `(t:task)<-[:BLOCKS]-() … count(*)` | runs; correct bag count; no diagnostic |
| 4 | `(#51)-[:BLOCKS]->(b)` meant as "what #51 waits on" | runs; reading echo `#51 BLOCKS b \| #51 must finish before b starts`; N07 when empty |
| 5 | `(p {id: #88})-[:CHILD_OF]->(c:task)` meant as children | runs; reading echo `#88 CHILD_OF c \| #88 is a child of c` (an anchored endpoint prints as its id, [LQ/envelope §4.3]) |
| 6 | `DEPENDS_ON` between tasks | E106, the `DEPENDS_ON` row of §5.3 |
| 7 | `MATCH (s1:doc) …` | runs; `s1` is an identifier; no diagnostic |
| 8 | `t.assignee = null` | E118, inline `write t.assignee IS NULL` |
| 9 | `'l5' IN labels(t)` | E102, the `labels()` row of §5.3 |
| 10 | `CALL log(main..lane/l5np)` | runs; no diagnostic |

## 8. Numbering rules

8.1. A code is never reused: E307 stays retired, E008 stays unassigned. A new code is appended after the last code of its range:
the next free codes are `E010`, `E119`, `E203`, `E309`, `E412`, `E506`, `W11` and `N13`. A new code is a change to this chapter
before the freeze, and a new grammar version after it ([50 §7.4] item 6).

8.2. Identifiers of the form `N13e`, `N14`, `N15` in [AR] and [50] (for example [50 §3.10] item 8's `(N13e)`, [50 §4.2]'s
`(N14)`, [AR §7.4]'s `(N15)`) are entries of [AR §2.17]'s critique ledger, not LQ notices. `N13` is the next free LQ notice
number (open point 8).

## Coverage

| Item | Where (and who covers the rest) |
|---|---|
| [90 §10.1] "Error table and refusal texts": the one new code for an `unknown`-profile free-form write, the two exit-5 refusal texts, mechanical fixes printed as replacement text (L4) | §2.4, §5.5 (E411; E407 "declared agent" and "bound lease"), §6; [F19] lists the same codes with their exit codes |
| [60 §2.5] "Harness-agnostic interface" row, part "one error code and two exit-5 refusal texts" | §5.5; the rest of the row: [LQ/envelope] (output contract), [LQ/gql-spelling] and [LQ/card] (display spelling), [F11] (`LEASES` fields), [F03] (X-F2 amendment), [F06] (`actor_src`), [F10] (codec) |
| [60 §2.5] "Harness-agnostic interface" row, part "ASCII" for diagnostic texts | §2.1, §3, §4.2; for results: [LQ/envelope §2.2] and [F19] |
| [50] F18: `QueryCycle` as the E405 refusal of a `TX` | §5.5; the violation classes and the merge validator: [F19], [F12] |
| [50] F17: the `ALLOC` facts N06 prints | §5.6; the `ALLOC` layout: [F11] |
| [80] X-F12 T6 (invalid UTF-8 exits 2) and T9 (error texts name the shell problem) | §5.2 (E003), §5.3 (E110), §5.6 (W09); T1–T10 as a whole: [OS/shell], [F19] |

## Holes

None of its own. The replacement texts of §2.4 and §6 print quantifiers in the display spelling, `HOLE(LQ-display-spelling)`,
which [LQ/gql-spelling] defines and WP-72 fills; each text stays ≤ 600 B in either spelling. The texts themselves are frozen
at WP-72; a WP-73 remedy that changes one is a specification edit, not a hole fill.

## Open points for the review

1. **E411 (PLAN §3.3 gap, shared with WP-18).** The unknown-profile refusal is `E411 unknown_model_write`, exit 6, raised by the
   tx binder before any execution. [F19] must list the same number and exit; if WP-18 chose another number, the review keeps
   one and the other chapter follows.
2. **E406's fix text is frozen (PLAN §3.3 gap).** The unleased text of [AR §7.3]/[90 §4.3] is split into the message
   `this write needs a lease` and the help line, byte-for-byte the design's wording. [90 §2.3] relies on it teaching the mint.
3. **`error[guard_conflict]` of [AR §7.1] is superseded.** Every write verb compiles to a `TX` ([AR §7.7.2]), so a failed
   `--if-rev`/`--if-status` is E401 with the current values. The review confirms that [AR §7.1]'s example is illustrative.
4. **One JSON error shape.** [50 §5.2] gives `errors: [...]` with `expected` as expected tokens; [50 §2.9] Q18 gives a single
   `error` object with `expected` as the `EXPECT` bound and a `hint` key. This chapter keeps [50 §5.2]'s array, adds `detail`,
   moves the bound to `expect` and folds `hint` into `help`. The unlocated text form likewise prints `= help:` instead of
   Q18's `| hint:`.
5. **W09 is new.** [50 §6.2] rule 6 and [80 §4] T6 keep a PowerShell `?` warning that the table of [50 §5.2] never numbered.
   It fires on [LQ/lexical §10.1]'s predicate P? with a PowerShell parent, under the name [LQ/lexical] open point L-7 proposed.
   Its wording is OS-neutral apart from naming PowerShell.
6. **N12 is new, and the `show` family never raises E111 for its ids.** [50 §3.6] keeps [AR §7.1]'s exit 3 for missing ids in
   the `detail` shape, while E111 exits 2. In the `detail` shape each requested id that yields no row is reported by N01
   (deleted), N06 (other branch) or N12 (never allocated), the other rows print, and the result exits 3 ([LQ/envelope §5.6]).
   Everywhere else a never-allocated literal id stays E111.
7. **W and N names are new.** They exist only for the JSON form; the text form prints codes only, as [50 §2.9] does.
8. **Ledger identifiers that look like notices.** §8.2. The review may prefer to rename the ledger entries; nothing here
   depends on it.
9. **Choices this chapter makes where [50] gives a behaviour but no text or number:** the located/unlocated split (§3.2–§3.3),
   the fitting order and caps (§3.4: `W` 60/30/10/0, `V` 64/24, 80-byte inline, 40-byte source), the stdout/stderr rule under
   `--ids` (§3.1; research [07 §6.2] had stderr for every error, [AR §7.1] stdout for every non-zero exit, and [AR] wins),
   E202's threshold expressed through `mem` instead of an unnamed "sort cap" ([50 §5.2]), and the JSON key order. Each is a
   rendering choice with no measured input.
10. **User-derived bytes in "ASCII" texts.** [90 §8.1] L5 and [73 F15] say error texts are ASCII. This chapter reads that as
    "every byte moirai's templates contribute is ASCII": identifiers, literals and excerpts from the caller's own text keep their
    UTF-8 bytes (controls escaped), because escaping a Cyrillic literal as `\u{…}` would multiply its bytes and make the excerpt
    unreadable. The alternative, escaping every non-ASCII scalar, is one line in §2.1.
11. **[50 §2.9] examples that differ from these templates** are illustrative: Q18's error text and JSON (point 4), the commit
    display `c<7 hex>` in several examples against the normative `c<8 hex>` of [AR §5a.1] and [50 §2.4] ([LQ/envelope] open
    point 3; S-24: goldens are authored from this text, never copied from examples).
12. **Reconciled with [LQ/grammar-v1.ebnf] and [LQ/lexical].** The detection points (Annex P, Annex R, lexical §1) are theirs;
    this chapter aligned its codes with them: E009 is the parser's; `:name` and a bare `@` are E001, not E004; E114's bound is
    2^32 − 1; `nodes(`, `relationships(` and the `CONSTRAINT` statements are E004; SHORTEST-style selectors are refused with
    "remove the selector"; the `%` rewrite is §R's; an edge variable on a quantified edge is E113 at bind time (O-8); `IS LABELED`
    and `CAST` are E004 with their LQ forms, which §R added at this chapter's request (O-13).
13. **The A1 re-review's S-02 (portable form).** The rewrite of node- and revision-typed constants on the bound AST is
    [LQ/canonical-ast]'s; E117's text is [50 §4.4]'s amended one, which covers anchor handles. The re-review's S-06 (the `TX`
    re-validation trigger, markers and leases included) changes when E401 and E402 fire, not their texts; its rule belongs to
    [F16] and [AR §4.5] step 7.
14. **The A1 re-review's S-05 (absent link states)**, as [50] now settles it: `link_state(n)` is `none` for a node without `AT`
    edges and `a.state` is `unresolved` for an unresolved file ([LQ/std §2.8]), and W10 makes a `<>`/`NOT IN` over the node form
    hedged rather than silent. W10's name and text are this chapter's; its trigger and number are [50 §5.2]'s.
15. **Pass 1 changes** (A1-32, A1-48, A1-51, A1-52, A1-53, A1-62, P1-11). The fitting of §3.4 never drops the closing line
    (`nothing was written`), cuts every detail line to 120 bytes, keeps at most two other detail lines (two E401 target
    lines) before dropping them all in a new step 7, and its bound is recomputed for interpolated detail lines. Controls
    escape as `\u{h}` (§2.1). E406's unleased text is the design's one line rendered as message and help. E408 takes a
    default key (`key` null, its own wording and help) and an entry without a commit (`original` null). E304's help renders
    the caller's `refs` ceiling. `CFGnn` diagnostics indent continuations by seven spaces and follow the `W` and `N` codes in
    the footer. E501 covers a write whose working set exceeds `wmem` and names the `--budget wmem=` raise.
16. **Pass 1, round 1** (P1-11, P1-21; [F19] open point 16). E501 gains its third case, an agent write whose changeset
    exceeds `store.commit.inline-max-bytes` ([F17 §4.4] W1, W4): the text names the split, since a `wmem` raise cannot
    help. E405 gains the rule texts `at most one <edge> (cardinality)`, `named queries bind (QueryInvalid)` and `schema
    conformance (I11)`, and E409 the root-node case of [F08 §11.3]; [F19 §10.5] and §12.4 cite them.
17. **Pass 1, round 3** (closure NC-8, A1-39's residue). [API §9.1] and [RULES/delete-policy-matrix] DP-005 give the
    live-lease refusal of I32′ the code E409, which had no text for it; §5.5 adds the lease case (message after
    [AR §5d.3]'s `leased by dev#2 on lane/y (L-19)`, the other leases as detail lines, the help naming `RELEASE`) and
    §5.7 its `leases` key. DP-007's invalid replacement, also mapped to E409, gets its case in the same row, and the order
    of the reference and edge lists is fixed. The root-node refusal ([F08 §11.3]) had no row among
    [RULES/delete-policy-matrix §4]'s preconditions; DP-010 (R-MODEL, round 3) now checks it after the options and
    before the lease, so the root-node case is printed first and names the root node of the deleted set with the
    smallest `#N`.
18. **Spec sync 2a** (WP-93a review and author). §3.1 orders the errors of a pass (E406 and E411 first, then located
    errors by span start with ties to the smaller code, then unlocated errors), as the model does and as [F19 §12.5.3]
    picks a first diagnostic, and settles a tie of [LQ/grammar-v1.ebnf §P.5]'s alternatives at one byte in favour of
    P13's nesting error. E009's span is the block's closing `}`. New texts: E108 for a bare word nothing coerces; E112 for
    an aggregate outside `RETURN` and `WITH`; E109 for a `YIELD` of a column the callee lacks, a named query that does not
    bind, a call cycle, and a reserved name in `DEFINE QUERY` or `DROP QUERY` (E109 because the refusal concerns the name
    of a callable); E110 for an unknown `param_decl` type and for a `$param` with no value in an ad-hoc query; E103 for a
    property read on a value that is neither a node nor an edge; E302 for `violations()` off a staging ref. E406's help
    renders [RULES/role-write-policy] WZ-001 (the refusing table and the (role, op, kind, field) tuple). E304 counts
    distinct views by their C-AST revision nodes. E305 gains `an orphans ref` and reads `an import ref accepts only
    RESOLVE`, matching [F12 §3.7] and §9.6, which let `import/*` take `RESOLVE`; [50 §3.9] item 6 is corrected at WP-81a.
19. **Spec sync 2b.** §3.1 makes the first-error rule normative through viable prefixes (a lexical error inside a viable
    prefix comes first; `RETURN a NOT $` is E001 at `$`) and adopts `fixtures/lq/INDEX.md` §4 G-7's positions. §2.2a
    renders names inside embedded LQ text as LQ source. §5.5's preamble is scoped (statement refusals name the statement
    and close with `nothing was written`, E405 and E410 included; block and call refusals do neither), E406's verb-level
    `statement` is `null`, E405's `statement` is the last statement or the `Schema` item index. New texts: P6's
    non-quantifier `{`, `EXPECT` with m > n, set-operation column counts, E501 for `tx.max-statements` and `tx.max-ops`
    with a `budget` object. E401's `current` objects, E409's `leases`, E411's profile and order, E118's position and
    E109's parser phase are stated. E401's help is not back-quoted (the table row; `fixtures/lq` follows).
