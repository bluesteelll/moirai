# 19 — Errors and output

| | |
|---|---|
| Title | The verb-independent output contract: streams and bytes, the ASCII rule, byte units and ceilings, the header limits and the tree display label, the pack and brief lines, link markers, the both-ends rule, the `--ids` page, exit codes 0–10, the `--json v1` envelope, the error, warning and hint text forms, the error codes of the store, the file verbs and the VCS verbs, the refusals of [90 §10.1], and the violation-class enum with the named-query validator (F18) |
| Chapter | [F19], `docs/spec/format/19-errors-and-output.md` |
| Status | draft, pass 1 pending |
| Work package | WP-18b (R-SPEC-F): the `format/19` half of WP-18 ([PLAN §3.2] item 1); `config.md` is the other half |
| Sources | [AR §7.1] (the conventions paragraph: `--ids`, the `--json v1` envelope, the header and its rules as amended by the A1 re-review A-M3, stdout of a non-zero exit, the exit-7 sandbox texts, exit codes 0–10; the example I/O); [AR §7.2] (MCP ceilings, `isError`); [AR §7.3] (the unleased E406 text); [AR §7.4] step 4 (the pack line and footer) and its `brief` paragraph; [AR §8.3] TOKENS rows "Link marker; CLI result header", "Stdout of a non-zero exit", "LQ error text"; [AR §4.5] steps 4, 5, 6 and 10 (exit 7 for `seq` exhaustion, the writer wait, corruption, `pending`, outcome unknown); [AR §6.4] (exit 9; the "outcome unknown" text); [AR §6.6] (quiet-mode refusals); [AR §5a.7] steps 6 and 8, [AR §5a.8] (the conflict and violation taxonomy); [AR §4.6] ("`Violation` ops … exist only on staging refs"); [AR §3.4] I12, I34′, I37′; [AR §5b.8] (reftable destinations without git); [90 §2.1] (C0.1 output row), [90 §2.2] (results, `isError`, discovery by `tree`), [90 §4.1] (the two exit-5 refusals; detection), [90 §4.3] (E406), [90 §5.3] (exit-7 texts per harness), [90 §6.1]–§6.4, §6.7 (caps, the byte unit, both ends, profiles and ceiling keys, JSON on request), [90 §8.1] L2, L4, L5, L6, [90 §9.2], [90 §10.1] rows "Output contract" and "Error table and refusal texts"; [50 §4.4] (named-query merge and validation), [50 §5.2] (error format, code table), [50 §5.9] steps 2 and 5, [50 §5.10], [50 §6.4] (envelope, footers, both ends, `--ids`), [50 §6.6] (exit codes), [50 §8.1] F18; [40 §2.9] (header strings), [40 §3.7], [40 §3.8] (error examples, illustrative), [40 §5.1] (tree label, reader note), [40 §6.1], [40 §6.2] (markers); [80 §2.5] rules 4 and 6 (the size check, the fault line), [80 §2.6] (refusals, writes outside the store), [80 §3.1] X-F12, [80 §4.2] T6, T7, T9; [60 §2.5] rows "Harness-agnostic interface and pure Rust", R5 F18, R4 R-16, the preamble and the protocol-decision row (b), (f); the A1 re-review findings A-M3, A-m1, A-m3, A-m4, A-m7 (`docs/spec/reviews/a1-A.md`) and S-08 (`docs/spec/reviews/a1-S.md`); [PLAN §3.2] WP-18, [PLAN §3.3] row "The unknown-model write error code number; whether E406's new fix text is frozen; F18's violation classes" |
| Depends on | [F01] (text, hexadecimal, decimal, order, enumerations, versions); cites [F02], [F03], [F04], [F05], [F06], [F07], [F08], [F09], [F10], [F11], [F12], [F13], [F14], [F16], [F17], [F18], [F20], [LQ/envelope], [LQ/errors], [LQ/std], [LQ/lexical], [LQ/grammar-v1.ebnf], [LQ/canonical-ast], [OS/shell], [OS/fs], [OS/lock], [OS/map], [OS/env], [OS/path], [OS/project], [OS/proc], [CFG], [API], [RULES/pack-classes], [RULES/merge-table] |

## 1. Scope

### 1.1 What this chapter owns

- The streams, the encoding and the ASCII rule of every byte moirai writes to stdout and stderr (§2).
- The byte unit, the output ceilings and the non-zero-exit rule (§3).
- The header's part limits, the tree display label and the `files` part, the placement of the reader note, the pack and
  brief lines, and the link markers of packs and briefs (§4).
- The both-ends rule (§5) and the `--ids` page rule (§6).
- The exit codes 0–10, the rule that chooses one, and the exit code of every diagnostic (§7).
- The frame of the `--json v1` envelope, its value encoding, versioning and error form (§8).
- The text forms of errors, warnings and hints outside LQ (§9), and the error codes of the store, the file verbs, the
  VCS verbs and the refusals other chapters delegate here (§10).
- The refusal texts of [90 §10.1] (§11).
- The violation-class enum and the named-query validator of [50] F18 (§12).

### 1.2 What it cites

| What | Owner |
|---|---|
| Every LQ diagnostic (`E001`–`E505`, `W01`–`W09`, `N01`–`N12`): name, severity, text, JSON object | [LQ/errors] |
| LQ result shapes, the header fields of LQ results and their order, the reading echo, footers, cursors, `TX` results | [LQ/envelope] |
| The R-16 state, detail and header strings (`files: no tree bound`, the reader note and its slot values) | [F18 §4] |
| The value-conflict classes (codes 1–63, §12.1) and the text form of conflict and violation keys | [F12] |
| argv and stdin decoding, the console versus pipe distinction, the three golden substitutions, the OS-error unit | [OS/shell] |
| The registry rows of the ceiling keys (type, scope, reload) | [CFG] |
| The typed result data of each verb (the JSON `data`) | [API], [LQ/envelope] |
| Pack classes, drop lists, the digest token | [RULES/pack-classes] |

### 1.3 Precedence among the parts

- [LQ/envelope §1.2] gives this chapter precedence for exit codes, the byte unit, the both-ends rule, the ASCII rule, the
  `--ids` page rule and the header limits. [LQ/errors §1.2] leaves it the exit-code table, the non-LQ refusals and the
  violation classes.
- §7.3 lists the exit code of every LQ code. [LQ/errors §5.1] lists the same; the two tables must agree, and a
  disagreement is a review finding.
- §11 restates texts that [LQ/errors §5.5] owns byte for byte; [LQ/errors] is authoritative for those bytes.

### 1.4 Terms

- **Result**: everything one command writes to stdout (CLI), or one MCP tool result's `content[0].text`.
- **Template bytes**: the bytes moirai's own texts contribute. **Value bytes**: bytes that come from the store, the file
  system or the caller (titles, bodies, paths, ref names, query text, OS names).
- **Error**: a text that begins `error[` (§9.1). **Warning**, **notice**, **hint**: texts that never change the exit code.
  A **refusal** is an error raised before anything is written.
- **Profile**: the client profile of [90 §6.4] (`claude`, `codex`, `generic`).
- **Golden**: an expected output file of GT12 (M8), of the reference renderer (WP-71a), or of a fixture of `fixtures/lq`.

## 2. Streams and bytes

### 2.1 Streams

| What | Stream |
|---|---|
| a result: header, reader note, reading echo, body, footer | stdout |
| a `--json v1` or `--jsonl` envelope | stdout |
| an error, and the rows printed before it, when the command exits non-zero | stdout ([AR §7.1]) |
| under `--ids`: the ids | stdout, and nothing else |
| under `--ids`: warnings, notices, hints, errors and the `--ids` footer (§6.3) | stderr |
| the durability-failure line and the mapping-fault line (§10.2, rows `durability_failure`, `store_io_fault`) | stderr |
| anything else | nothing: moirai writes no banner, progress, log or debug text to either stream |

- `moirai mcp` writes JSON-RPC frames to stdout. A tool result's text is `content[0].text` (§8.7). The server writes to
  stderr only the two crash lines of row 6.
- A hook (`moirai hook <name>`) writes the output format of its harness's hook protocol ([AR §7.5]); the text moirai puts
  inside it (for example `additionalContext`) follows this chapter.

### 2.2 Encoding

[OS/shell §6] items 1–6 own the bytes: UTF-8, LF only, never CR, no byte-order mark, no byte 0x1B off a terminal,
byte-identical on every OS except the golden substitutions `<ROOT>`, `<OSERR>` and `<OS-DETAIL>`, and a broken stdout pipe
ends the process quietly with exit 0. This chapter adds:

1. Every line of a result, the last included, ends with LF. An empty result is zero bytes.
2. On a terminal moirai writes the same bytes as to a pipe: format v1 has no colour and no cursor control.
3. moirai never prompts. A verb that needs confirmation (`rm`, `file rm`, `links fix --drop`) without `--yes` prints its
   dry run and exits 0 ([AR §7.1] `rm ID --dry-run`, [40 §3.8]).

### 2.3 The ASCII rule

1. Every template byte is printable ASCII (0x20–0x7E) or LF (0x0A) ([90 §8.1] L5, [90 §10.1]). The rule covers headers,
   footers, markers, legend lines, continuation lines, the pack and brief lines, error, warning, notice and hint texts,
   `--ids` footers, JSON keys and punctuation, and every MCP result text.
2. The spellings: truncation `...` (three 0x2E); separator ` | ` (0x20 0x7C 0x20); arrow `->`; range `..`. No `…`, `·`,
   `→`, `≈`, `—`, `×`, `≤`, `≥`, typographic quote or other non-ASCII scalar value appears in a template.
3. Value bytes keep their UTF-8 (a Cyrillic title stays Cyrillic) and are rendered by §2.5.
4. The check: every golden, with its value bytes removed, contains no byte outside rule 1.

### 2.4 Values in texts

These spellings hold in every text; [LQ/envelope §5.2] extends them for LQ cells and agrees with them.

| Value | Text form |
|---|---|
| node id | `#` followed by N in decimal (`#812`) |
| anchor handle | `a` followed by the handle number in decimal (`a31`; [F04 §5.7]) |
| lease | `L-` followed by the `lease_id` in decimal (`L-18`; [F05 §9.4]) |
| file intent | `i-` followed by the intent id in decimal, the lsn of its `FsIntent` record ([F05]) |
| moirai commit | `c` and the first 8 lower-case hexadecimal digits of the commit id: 9 bytes ([AR §5a.1], [50 §2.4], review A-m7). The full id appears only in JSON (§8.3) |
| git commit of a tree's `HEAD` | its first 7 lower-case hexadecimal digits |
| sequence number | decimal; `rev <seq>` in headers |
| count (rows, ids, retries, lines, violations) | decimal, no grouping |
| byte quantity | decimal with `,` every three digits from the right, then ` B` (`15,200/16,000 B`, `8,000 B`, `600 B`) |
| budget quantity in a budget text | decimal with `,` grouping ([LQ/errors §2.2] `<N>`) |
| duration in an error text | `<n> ms` |
| age | `<n>` and the largest unit among `s`, `m`, `h`, `d` whose floor is non-zero (`0s` below one second) |
| timestamp | `YYYY-MM-DDTHH:MMZ`, UTC |
| store, tree and file-system path | the canonical absolute form with `/` ([80 §2.10] P12), bare or quoted by §2.5 rule 3; golden substitution `<ROOT>` |
| project path | root-relative with `/` ([OS/path §2.1]), bare or quoted by §2.5 rule 3 |
| ref name | bare |
| OS error | the unit `os <code> <SYMBOL>` of [OS/shell §6] item 6; an OS message text is never printed |
| score | two decimals ([F18 §5.3]) |
| any other value from the store or the caller | §2.5 |

Hexadecimal that moirai prints is lower-case ([F01 §6.4]). Of the hexadecimal a caller types, upper-case is accepted only
in git object ids (`--base`, the image verbs), as git accepts either case; commit prefixes, `#u:` literals and cursors follow
[LQ/lexical] and [LQ/envelope §8.3], and pointer files [F02 §3.3].

### 2.5 Untrusted text

1. **Quoted form.** `"`, the escaped value, `"`. Escapes: `"` → `\"`, `\` → `\\`, CR → `\r`, LF → `\n`, TAB → `\t`;
   every other scalar value in U+0000–U+001F and U+007F → `\u{`, its code point in lower-case hexadecimal without leading
   zeros, `}` (U+0001 → `\u{1}`, U+001B → `\u{1b}`, U+007F → `\u{7f}`), the escape LQ string literals accept
   ([LQ/lexical]); each byte of an ill-formed UTF-8 sequence → `\x` and two lower-case hexadecimal digits ([OS/path §9]).
   Every other scalar value is written as its UTF-8 bytes. This is [LQ/envelope §5.16]'s rule.
2. **Cut.** When the escaped value exceeds 120 bytes it is cut at the last scalar-value boundary at or before byte 117 that
   does not split an escape, and `...` is appended inside the quotes.
3. **Bare form.** A value of 1 to 64 bytes whose every byte is in `A-Z a-z 0-9 _ . / : @ # + * ? ~ ^ % ! -` is written bare;
   every other value is quoted (rule 1). Free-text fields are always quoted ([LQ/envelope §5.2]).
4. **Bodies** are fenced as [LQ/envelope §5.16] says.
5. **Values inside diagnostics** are capped as [LQ/errors §2.3] says (V = 64 bytes, fitted to 24).
6. **Data, never instructions.** A value appears only in a quoted, bare or fenced position, so it cannot forge a header, a
   row, a separator, a marker or an `error[` line.

## 3. Byte units and ceilings

### 3.1 The unit

Every budget, ceiling, limit and ledger row of the output contract counts the UTF-8 bytes of the emitted text, LF
included ([90 §6.2]). No output prints a token count or a token estimate; `--explain` may print per-family estimates
from the M0 conversion table ([90 §6.2], [90 §9.1]).

### 3.2 Ceilings

The keys are [CFG]'s; the defaults are [90 §6.4]'s and [AR §13]'s.

| Surface | What is bounded | Key | `claude` | `codex` | `generic` |
|---|---|---|---|---|---|
| MCP result | the whole `content[0].text` | `mcp.result-max-bytes`, `.<client>` | 25,000 B | HOLE(CFG-codex-mcp-result): the design value 16,000 B, decided by measurement 7's probes P3 and P4 ([CFG §10.8]; ≤ 36,000 B by key for a classic-mode model) | 25,000 B |
| MCP id-dense result | an MCP result whose body lines are ids only | `mcp.ids-page-bytes` | 8,000 B | 8,000 B | 8,000 B |
| MCP pack | the pack text | `pack.mcp.max-bytes`, capped by the MCP result ceiling | 25,000 B | 16,000 B | 25,000 B |
| CLI pack | stdout of `pack` without `-o` | `pack.cli.max-bytes` (≤ 28,000) | 24,000 B | 24,000 B | 24,000 B |
| CLI `--ids` page | §6 | `output.ids-max-bytes` (0 = unlimited) | 24,000 B | 24,000 B | 24,000 B |
| CLI stdout of a non-zero exit | §3.3 | `output.nonzero-exit-max-bytes`, `.<client>` | 8,000 B | 8,000 B | 8,000 B |
| LQ result page | [LQ/envelope §5.18] | `query.budget.default.bytes` (agent maximum 24,000) | 8,000 B | 8,000 B | 8,000 B |
| brief | the brief text | `brief.budget` | 8,000 B | 8,000 B | 8,000 B |
| hook context | any hook's injected text ([AR §7.5]) | the hook budget keys | ≤ 10,000 B | ≤ 10,000 B | ≤ 8,000 B ([90 §6.4]'s `generic` row; [RULES/pack-classes] PE-006 to PE-008 give the three profiles' hook ceilings, pass 1 A1-57) |
| one error, warning, notice or hint | its text, continuation lines included | fixed, not a key | 600 B | 600 B | 600 B |

- A result never exceeds the ceiling of its surface, except `pack -o FILE`, which writes a file.
- Where several ceilings apply, the least holds: an MCP pack is bounded by the minimum of its budget,
  `pack.mcp.max-bytes` and the MCP result ceiling.
- Which MCP results are id-dense is [LQ/envelope]'s and [API]'s (M10).

### 3.3 The non-zero-exit rule

1. When a CLI command exits with a code other than 0, its stdout holds at most `output.nonzero-exit-max-bytes` bytes of the
   active profile ([AR §7.1]; Claude Code's Bash tool shows about 10,000 characters of a failed call, [90 §6.1]).
2. Fitting, in this order:
   1. each error, warning, notice and hint text is fitted to 600 B (§9.1, [LQ/errors §3.4]); at most three errors are
      printed;
   2. the rows printed before an exit-10 cut or before an error are cut at a row boundary until the whole stdout fits; the
      cursor then continues after the last printed row, and the header and the footer say so (§5);
   3. under `--ids`, §6.2 applies.
3. An error-only result is at most 3 × 600 B plus two separating empty lines, so it always fits.
4. The rule is a CLI rule. An MCP result, `isError` or not, is bounded by the MCP result ceiling (§3.2).

## 4. The first lines of a result

### 4.1 Which results carry the header

1. The first line of every result of a verb that reads or writes a branch view is the **header** ([AR §7.1]): every read
   verb (a named query), every write verb (a named mutation, `tx`, `apply`), the file verbs, and the verbs that move a ref
   (`merge`, `sync`, `revert`, `cherry-pick`, `undo`, `checkout`, `branch`, `tag`), whose header names the ref that moved.
2. The fields of the header and their order are [LQ/envelope §3.1] and §9.1–§9.3 for every verb, since every verb with a
   view is a named query or a named mutation ([AR §7.7.2]).
3. No header is printed:
   - on an error-only result ([LQ/errors §3.1]);
   - on `--ids` stdout (§6);
   - on `pack` and `brief`, whose first line is the pack or brief line (§4.5);
   - on `explain`, `profile` and `--check` output, which has its own first line ([LQ/envelope §10]);
   - by the verbs that work on no view: `init`, `config`, `doctor`, `backup`, `restore`, `repair`, `gc`, `quiet`,
     `migrate`, `integrate`, `hooks install`, `hook`, `mcp`, `schema`, `export` and `image`. Their first line begins with
     the verb's own word.

### 4.2 Header parts and their limits

The header's fields fall into four **parts**; the reader note is a fifth, on line 2 (§4.4). The limits are [AR §7.1]'s and
[AR §8.3]'s as amended by the A1 re-review A-M3 (open point 1).

| Part | Fields ([LQ/envelope §3.1], §9.1–§9.3) | Limit |
|---|---|---|
| base | `branch: <ref>`; `rev <seq>` or `rev <old> -> <new>`; one view flag (`as-of (USE <revspec>)`, `staged (read-only)`, `live`); `<n> row`, `<n> rows`, `<n>+ rows`, `check` or `tx (dry)` (a `DRY` result has no rows field; its `tx (dry)` takes that place); `committed <c8>`; `replayed`; `staged` on a write to a staging ref | ≤ 60 B; ≤ 80 B when the view or the written ref is a staging ref, whose name `merge/<dst>/from/<src>` holds two ref names (spec sync 2b) |
| files | the `files @` field or `files: no tree bound` (§4.3) | ≤ 80 B |
| extras | every other field: a composite rev part, the `across` field, the `diff` field, the three search fields, the `derived recomputed` field, `schema v<n>`, `behind main <n>`, `view moved +<k> commits since page 1`, `key <key>`, `lease <lease>`, `IF TIP ok`, `IF TARGETS ok`, `would commit <n> changes` | ≤ 60 B, all extras of the line together |
| continuation | `dropped <n>`; `more: cursor <cursor>` with the cursor text itself not counted | ≤ 30 B |
| reader note | line 2 (§4.4) | ≤ 80 B |

**Counting.**
1. A part counts the bytes of its fields, each field with the ` | ` that precedes it. `branch: <ref>` has none. The LF is
   not counted.
2. The limits are checked on goldens: GT12 (M8), the WP-71a goldens, and the examples of [AR §7.1], [40 §3.8] and
   [50 §2.9] as re-counted there (review A-M3). A golden whose part exceeds its limit fails.
3. At run time moirai cuts only the tree label and the git branch (§4.3) and the reader note's slots ([F18 §4.8]). It never
   cuts a ref, a revision, a revspec, an id, a count, a key, a lease id or a cursor; a header whose values are longer than
   the goldens' is printed whole.
4. The cursor text is not counted because its length follows its content ([LQ/envelope §8]), not a template. It is ASCII
   and shell-safe ([LQ/envelope §8.3]).

*(Informative)* `branch: merge/main/from/lane/l5np | rev 4471 | staged (read-only) | 12 rows` has a base part of 75 B,
within a staging view's 80 B, where even `merge/a/from/b` would pass 60 B.
`branch: lane/l5np | rev 4471 -> 4472 | committed c4472a0f1` has a base part of 58 B;
` | behind main 3` is an extras part of 16 B; ` | dropped 4 | more: cursor ` is a continuation part of 28 B. A `DRY`
header's extras (pass 1, A1-30; open point 1 adopted) are at most ` | IF TIP ok | IF TARGETS ok | would commit 99999 changes`,
57 B: `nothing written` is not printed (a dry run writes nothing by definition) and `tx (dry)` is counted in the base part.
A composite rev part prints each part without its `<c8>` (JSON `parts` carries the commits, [LQ/envelope §3.2]).

### 4.3 The `files` part and the tree display label

([RFC 5234] ABNF with [RFC 7405]'s `%s`.)

```abnf
files-part   = files-at / %s"files: no tree bound"
files-at     = %s"files @ " label " (" git-part [ %s", dirty " count SP age %s" ago" ] ")"
git-part     = gitbranch SP head7 / %s"detached " head7 / gitbranch %s" unborn" / %s"no git"
head7        = 7LHEX
age          = count ( %s"s" / %s"m" / %s"h" / %s"d" )
count        = "0" / NZDIGIT *DIGIT
label        = <the tree display label of rule 1>
gitbranch    = <the tree's branch of rule 2>
LHEX         = DIGIT / %x61-66
NZDIGIT      = %x31-39
SP           = %x20
```

1. **The tree display label** ([AR §7.1] as amended by A-M3; [40 §5.1]). Let T be the tree's canonical absolute path
   ([80 §2.10] P9, P12), M the canonical absolute path of the repository's main worktree, and P the parent directory of M.
   - If M exists and T begins with P followed by `/`, the label is T without that prefix. The main worktree is labelled by
     its own directory name.
   - Otherwise (a tree outside P, or a store with no git repository), the label is T's last two path segments joined by
     `/`, or its one segment when T has only one below its root prefix (`X:/`, `//server/share/`, `/`).
   - A label longer than 20 bytes becomes `...` followed by its longest suffix of at most 17 bytes that begins at a
     scalar-value boundary.
   - The label's bytes are value bytes, written bare with control characters and ill-formed bytes escaped as §2.5 rule 1
     says; it is never quoted.
2. **`gitbranch`**: the tree's checked-out branch without `refs/heads/` ([F18 §3.2] rule 1), cut as the label is, to at
   most 16 bytes (`...` and at most 13). `detached` when `HEAD` names no branch; `unborn` when the branch has no commit;
   `no git` for a tree outside every git repository.
3. **`head7`**: the first 7 lower-case hexadecimal digits of the tree's `HEAD` commit.
4. **Dirty**: printed when the tree's dirty row (`TREES`, [F11]) records n > 0 changed paths; `age` is the time since the
   row was written (§2.4).
5. The canonical path goes only to JSON `tree` and to `file where` ([AR §7.1]).
6. `files: no tree bound` is [F18 §4.8]'s string. No command follows it in the header (review A-m4); `doctor lanes` names the
   binding command.
7. **Bound.** With a dirty count of at most 5 digits and an age of at most 3 digits, the files part is at most
   3 + 8 + 20 + 2 + 16 + 1 + 7 + 8 + 5 + 1 + 4 + 4 + 1 = 80 B.

### 4.4 Line 2: the reader note

- A file-bearing result read from a reader tree prints, on line 2, [F18 §4.8]'s note
  `reading only: tree on <here>, branch expects <there>` with [F18 §4.8]'s slot values and slot cuts, which keep it within
  79 B ([AR §7.1], review A-M3).
- The note is not part of the header's `files` part, which still prints its `files @` field for the tree.
- Line 2 holds the note; the reading-echo lines of [LQ/envelope §4] follow it; the body follows them.

### 4.5 The pack line and the brief line

```abnf
pack-line    = %s"moirai pack #" count SP role SEP %s"branch " ref SEP %s"rev " count SEP used "/" budget %s" B"
               [ SEP %s"dropped " count SEP %s"more: " continuation ] SEP %s"digest " token
brief-line   = %s"moirai brief" SEP %s"branch " ref SEP %s"rev " count SEP used "/" budget %s" B"
               [ SEP %s"dropped " count SEP %s"more: " continuation ]
SEP          = %s" | "
used         = grouped                            ; §2.4 byte quantity without " B"
budget       = grouped
grouped      = 1*3DIGIT *( "," 3DIGIT )
token        = count "-" 8LHEX                    ; [RULES/pack-classes] NR-002
role         = <a role name ([AR §7.3])>
ref          = <a ref name ([F12])>
continuation = <a continuation of rule 2>
```

`count`, `LHEX` and `SP` are §4.3's.

1. `used` is the byte count of the whole emitted text ([RULES/pack-classes] PY-002, PY-003); `budget` is the effective
   budget E after the caps of §3.2.
2. **Continuations.**
   - CLI `pack`: `moirai pack <T> --role <role>`, then `--phase <P>`, `--budget <n>` and `--since-round <k>` when the call
     had them, then `--more`.
   - MCP `pack`: `call pack again with more=true`.
   - `brief`: `moirai brief`, then `--scope <id>`, `--role <role>` and `--budget <n>` when the call had them, then `--more`.
     The MCP `brief` tool has no continuation parameter ([AR §7.2]); its result prints the CLI form (open point 22).
3. The last line of a pack or brief that dropped anything is `dropped: <what> | more: <continuation>` ([90 §6.3]); `<what>`
   is [RULES/pack-classes]'s drop list.
4. **Limit**: the pack line and the brief line are each at most 200 B in every golden (open point 22).

### 4.6 Link markers in packs and briefs

[RULES/pack-classes] RN-008 renders one marker on every link that is not `ok` ([40 §6.2]). Its spelling:

```abnf
marker       = "[" mstate [ SP mextra ] [ SEP maction SP handle ] "]"
maction      = %s"verify" / %s"confirm"
handle       = "#" count / "a" count
mstate       = <the state, or the qualified state of [F18 §4.7] rule 3, as the table below gives it>
mextra       = <the extra the table below gives for the state>
```

`SEP` is §4.5's; `count` and `SP` are §4.3's.

| Link state ([F18 §4.4]) | Marker |
|---|---|
| `moved-auto` | `[moved-auto from <old path>]` |
| `moved-auto` whose `relink` is a guess ([F18 §5.6]) | `[accepted guess <c8> \| confirm <handle>]` |
| `moved-needs-confirm` | `[moved-needs-confirm <score> \| verify <handle>]`; without a score `[moved-needs-confirm \| verify <handle>]`. The candidate path is on the `verify` output, not in the marker |
| `ambiguous` | `[<qualified state> \| verify <handle>]`; when that exceeds 50 B, `[ambiguous \| verify <handle>]` |
| `deleted` | `[deleted <c8>]`, or `[deleted <c8> -> <replacement path>]` |
| `replaced` | `[replaced since <c8> \| verify <handle>]` |
| `stale-anchor` | `[<qualified state> <score> \| verify <handle>]`; without a score `[<qualified state> \| verify <handle>]` |
| `missing` | `[missing since <c8> \| verify <handle>]` |
| `absent-in-tree` with detail `diverged` | `[absent-in-tree (diverged) \| verify <handle>]`; `behind` is never marked: it is folded into the header count ([40 §6.2]) |
| `pending` | `[pending \| verify <handle>]` |
| `planned` | `[planned]` |
| `unverified` | `[<qualified state>]` |

1. `<qualified state>` is [F18 §4.7] rule 3's `<state> (<label>)`.
2. A path in a marker is cut as the tree label is (§4.3 rule 1), to at most 20 bytes.
3. `<handle>` is the file node's `#N` for a file-level state and the anchor's `aN` for an anchor-level one.
4. **Legend lines**, one each per result, after the class lines: `verify #N: moirai file where N --evidence` when any marker
   carries `verify`; `confirm #N: moirai links fix N --confirm (orchestrator or owner)` when any carries `confirm`. No
   marker and no legend prints a command that accepts a guess ([41 M6], [40 §6.2]).
5. **Limit**: every marker is at most 50 B ([AR §8.3] TOKENS row "Link marker"; pass 1, A1-29). With a path cut to 20 B,
   a score of 4 B, a `<c8>` of 9 B and a handle of at most 11 B (`#4294967295`), the forms above are at most 49 B, except
   the qualified `ambiguous (normalization collision)`, which rule 1's fallback replaces by the bare state. The goldens
   (GT12, WP-71a) check every marker against 50 B (open point 23).

## 5. The both-ends rule

1. A result that dropped anything, or that continues on a next page, carries the drop count and the continuation on its
   first line and again on its last line ([90 §6.3]):

| Result | First line | Last line |
|---|---|---|
| a result with a header | `dropped <n>` and `more: cursor <cursor>` in the header's continuation part | the footer's `dropped:` line and continuation line, the continuation last ([LQ/envelope §6]) |
| `pack`, `brief` | the pack or brief line (§4.5) | `dropped: <what> \| more: <continuation>` |
| hook context | its first line (the brief line, or the header of a delta) | its last line |
| `--ids` | none: stdout holds only ids | the stderr footer (§6.3); exit 10 marks the cut |

2. A head cut, a tail cut and a middle cut of one result each leave at least one of the two lines ([90 §6.3]).
3. A continuation can be run as printed: it obeys the argv rules T1–T3 ([OS/shell §3]). For free-form LQ text it is an
   instruction ([LQ/envelope §6.4]).
4. Every result is one text; through MCP it is one text item ([90 §6.3]).

## 6. `--ids`

### 6.1 Stdout

```abnf
ids-out      = *id-line
id-line      = "#" count LF                       ; count and LF as in §4.3 and [RFC 5234]
```

Only ids, one per line, in the result's order, with no header, no footer and no row cap ([AR §7.1], [90 §2.1]).

### 6.2 The page and the exit code

Let L be `output.ids-max-bytes` and N be `output.nonzero-exit-max-bytes`.

1. **L = 0** (scripts): no byte limit applies to `--ids` stdout; every id is printed. A budget cut still exits 10 (rule 4).
2. **Whole**: when every id fits in L bytes and no budget cut the result, every id is printed and the exit code is 0.
3. **Page**: otherwise the command prints the longest prefix of whole id lines of at most min(L, N) bytes and exits 10.
   (N applies because exit 10 is a failed call to a harness, §3.3; open point 6.)
4. **Budget cut**: a budget that ends the query early prints the ids produced so far, within rule 3's bound, and exits 10.
5. **Empty**: an empty result prints nothing and exits 0.
6. `--ids` together with `--json`, `--jsonl`, `--count` or `--format table` is a usage error (exit 2).

### 6.3 Stderr

In this order: the warnings, notices and hints of the result ([LQ/errors §4.2], §9.2, §9.3); then, when the result was cut,
the footer line of [LQ/envelope §6.5] (`ids: <n> printed | more | cursor <cursor> | <rerun>`). A command that fails writes
its error texts to stderr instead of the footer ([LQ/errors §3.1]). Each text is at most 600 B.

## 7. Exit codes 0–10

### 7.1 The codes

| Code | Name | Meaning |
|---|---|---|
| 0 | ok | The command did what it was asked. This includes an empty result, a paginated result whose continuation is in its footer, a dry run, a replayed write, a result with warnings, notices or hints, and a broken stdout pipe |
| 1 | internal | A defect in moirai: a panic, a failed internal check, a value moirai built that the format or the OS refuses |
| 2 | usage | The input does not parse or bind: argv, MCP parameters, stdin that is not UTF-8, LQ lexer, parser and binder errors, paths, anchor specs, values, configuration keys and values |
| 3 | not found | An id, a revision or another object the command names does not exist; the tombstone or the notice that explains it is printed |
| 4 | guard conflict | A guard the caller set failed (`EXPECT`, `IF TIP`, `IF TARGETS`, `--if-rev`, `--if-status`, `--if-holder`); the current values are printed; nothing was written |
| 5 | lease or view mismatch | The presented lease is missing, lost, stale, held by another holder or bound to another thread; an explicit branch or tree disagrees with the lease; a file verb runs outside the writer tree; a binding conflicts with another |
| 6 | precondition | The store refuses the operation in its present state: a status transition, an invariant, an assertion, the role policy, a restricted delete, an ambiguous bind, a read-only view, an unknown-profile free-form write, a staged merge, sync, import, revert or cherry-pick, `--strict` with conflicts or non-`ok` links, an open staging pair, a placement refusal, quiet mode |
| 7 | store unavailable | The store cannot be found, opened, locked, written or trusted, or a write's outcome is not known: discovery misses, bad pointers, a swap, a newer format, a refused location, a read-only volume, flag or sandbox, lock waits, a pending or unknown outcome, a durability failure, a mapping fault, a size mismatch, corruption, an exhausted id space, a busy file, a cross-volume move, a full disk, no git for an image verb |
| 8 | partial batch | A command made of several independent commits committed some and not others (`apply` over several runs; `image import` or `image pull` over several refs), or a file verb whose commit recorded failed items (`file mv`, `file rm` or `file revert` over several items, some done and some failed: [F05 §9.16], [API §12.4] step 5). A `TX` is all or nothing and never exits 8 |
| 9 | idempotency mismatch | The key was used for another payload or on another branch; the original result is printed |
| 10 | incomplete result | A budget ran out, a pre-flight check refused on a lower bound, the query was cancelled, `fs` units ran out, or an `--ids` result was cut. Rows printed before the cut are correct, and the continuation says how to go on |

### 7.2 Choosing the code

1. A command ends with exactly one code: the code of the first error it prints ([LQ/errors §3.1]).
2. A resumable plan cut by a budget prints its rows and exits 10; a blocking plan exits 10 with no rows ([50 §5.6]).
3. Rows or bytes per page are not a cut: a paginated result exits 0 ([50 §5.10]). The `--ids` cut exits 10 (§6.2).
4. A command of several commits (§7.1 code 8) exits 8 when at least one committed and one failed, and with the first failed
   unit's code when none committed.
5. Warnings, notices and hints never change the code ([LQ/errors §4.4]).
6. A cancelled query (Ctrl-C in the CLI, `notifications/cancelled` through MCP) ends with E504, exit 10.
7. A broken stdout pipe ends the process with 0 and no further output ([OS/shell §6] item 4).
8. A panic prints the `internal` text (§10.2) through a panic hook, to stdout (stderr under `--ids`), and then ends the
   process with exit code 1 without unwinding, by the same OS call that `fail_stop` uses with 7 ([OS/fs §4.4.5]).
9. moirai produces no code outside 0–10. A caller that sees another code (a process killed from outside, an OS abort)
   treats the outcome as unknown and re-runs with the same idempotency key ([AR §6.4]).

### 7.3 The code of every diagnostic

| Exit | LQ codes ([LQ/errors §5.1]) | Codes of this chapter (§10) |
|---|---|---|
| 0 | none; `W01`–`W09`, `N01`–`N12` and hints never set a code | — |
| 1 | none | `internal` |
| 2 | E001–E007, E009, E101–E118, E302, E306, E308 | `usage`, `bad_path`, `nonportable_name`, `ambiguous_path`, `anchor_spec`, `bad_value`, `config_key`, `config_value`, `bad_ref_name`, `ref_exists`, `ref_prefix` |
| 3 | E301; the `detail` shape's missing ids (N01, N06, N12, [LQ/envelope §5.6]) | `not_found` (a write to a node that is not live included), `commit_pruned` |
| 4 | E401, E402 | — |
| 5 | E407 | `not_writer_tree`, `tree_mismatch`, `binding_conflict` |
| 6 | E305, E403, E404, E405, E406, E409, E410, E411 | `placement_refused`, `staged`, `staging_exists`, `conflicted_src`, `links_not_ok`, `repin_needs_at`, `confirm_refused`, `path_claimed`, `quiet_mode`, `name_taken`, `not_merged`, `revert_refused`, `not_fresh` |
| 7 | none | `no_store`, `not_a_store`, `bad_pointer`, `swap_in_progress`, `store_retired`, `no_canonical_path`, `other_store`, `format_version`, `refused_location`, `read_only_volume`, `readonly_flag`, `store_read_only`, `sandbox_write`, `store_locked`, `maintenance_busy`, `no_slot`, `outcome_pending`, `outcome_unknown`, `durability_failure`, `store_io_fault`, `sealed_size`, `store_corrupt`, `id_space_exhausted`, `commit_too_large`, `uid_collision`, `fs_busy`, `cross_volume`, `no_dir_flush`, `disk_full`, `no_git` |
| 8 | none | `partial_batch` |
| 9 | E408 | — |
| 10 | E201, E202, E303, E304, E501–E505 | none: the `--ids` cut has no error text (§6.2) |

E008 is unassigned and E307 retired ([LQ/errors §8.1]). The `PATCH` substring case of E404 is listed at exit 6 as
[LQ/errors] has it; [AR §7.1] gives `doc patch` exit 4 (open point 7).

### 7.4 Hooks

`moirai hook <name>` exits 0 on every path and reports a decision only through its harness's hook JSON. A hook that fails
prints nothing and exits 0 (hooks fail open, [AR §7.5]). It never exits 2, which Claude Code reads as "block".

### 7.5 MCP

- `isError` is `true` exactly when the CLI would exit 1–9, or exit 10 with no row printed (a blocking plan, E201, E202,
  E303, E304, or a cut before the first row). A result with rows that ends in a budget cut has `isError: false`; its footer
  says `exit 10` ([LQ/envelope §6.4]).
- The text form carries no exit-code line ([50 §5.2]). The JSON form carries the code as `exit` (§8.6).

## 8. The `--json v1` envelope

### 8.1 Form

- `--json v1` and `--json` are the same flag in format v1 ([AR §7.1]); `--jsonl` is §8.5.
- The envelope is one JSON object in compact form (no whitespace outside strings), UTF-8 without a byte-order mark,
  followed by one LF, on stdout.

### 8.2 Keys

The keys appear in this order; a key marked "when" is omitted otherwise.

| # | Key | Type | Present | Content |
|---|---|---|---|---|
| 1 | `v` | int | always | `1` |
| 2 | `branch` | string or null | always | the view's ref; `null` for a verb with no view (§4.1 rule 3) and before a ref was resolved |
| 3 | `rev` | int or null | always | the view commit's sequence number; `null` as for `branch` |
| 4 | the family's leading keys | — | by family | LQ reads and writes: [LQ/envelope §7.1] (`commit` … `cols`) and §7.7; every other verb: [API] |
| 5 | `data` | array or object | always in a success envelope | the rows, or the verb's result object ([API]) |
| 6 | `next` | string or null | always in a success envelope | the continuation cursor or token |
| 7 | `dropped` | object or null | always in a success envelope | `{"count":<int>,"what":[<string>...],"more":<string or null>}` ([LQ/envelope §7.1]) |
| 8 | the family's trailing keys | — | by family | LQ: `notices`, `warnings`, `budget` ([LQ/envelope §7.1]) |
| 9 | `hints` | array | when non-empty | §9.3 objects |
| 10 | `errors` | array | when the command exits non-zero | §8.6 |
| 11 | `exit` | int | when the command exits non-zero | the exit code, 1–10 |

### 8.3 Values

- Node ids are strings `"#N"`; commit ids are `c` followed by 64 lower-case hexadecimal digits ([50 §2.9] Q18, the A1
  disposition A-m7; [LQ/envelope §7.3]; pass 1, S1-17, A1-16), the form a revspec takes back;
  sequence numbers are integers; timestamps are RFC 3339 UTC strings with milliseconds (`2026-09-25T12:03:00.000Z`);
  durations and byte quantities are integers (milliseconds, bytes); enumerations are their names; an absent value is
  `null` ([LQ/envelope §7.3]).
- **Integers** are decimal, `-` only for a negative value, no leading zero, no fraction and no exponent. Every integer a
  format-v1 envelope carries is below 2^53; a value that can exceed it (an `hlc`, an lsn, a hash) is carried as a string.
- **f64** values are the shortest round-trip form with a `.` or an exponent; NaN and infinities are never written.
- **Strings** escape `"` as `\"`, `\` as `\\`, LF as `\n`, CR as `\r`, TAB as `\t` and every other scalar value in
  U+0000–U+001F as `\u00` and two lower-case hexadecimal digits; nothing else is escaped (not `/`, not DEL, not non-ASCII).
  A byte of an ill-formed UTF-8 sequence in a value is written as the four characters `\x` and two lower-case hexadecimal
  digits, which JSON spells `\\x..`. Strings are never cut.

### 8.4 Versions and additivity

1. `v` is 1 for every envelope of format v1.
2. **Additive** changes keep `v`: a key appended at its position of §8.2 (after the last key of its family and before
   `hints`, `errors` and `exit`), and a new value of an enumeration. A consumer ignores unknown keys and accepts unknown
   enumeration values.
3. Every other change (a removed, renamed, retyped or reordered key, a changed meaning) is a new envelope version `v` = 2,
   produced only under `--json v2`; `--json` stays `--json v1` ([AR §7.1] "one frozen, versioned envelope").

### 8.5 `--jsonl`

Line 1 is an object with every key that precedes `data`; each element of `data` follows as its own line; the last line is
an object with every key that follows `data` ([LQ/envelope §7.6]). Every line is compact JSON ended by LF. When `data` is an
object rather than an array, `--jsonl` prints the whole envelope as one line. An error envelope (§8.6) is one line.

### 8.6 Errors in JSON

- A command that exits non-zero and printed no rows writes the **error envelope**
  `{"v":1,"branch":<string or null>,"rev":<int or null>,"errors":[<error>...],"exit":<int>}` ([LQ/errors §4.1]).
- A result with rows that ends with an error keeps its success keys and adds `errors` and `exit` (§8.2 rows 10–11).
- An `<error>` of this chapter's table has [LQ/errors §4.1]'s keys in its order — `code`, `name`, `severity`, `span`,
  `message`, `suggest`, `expected`, `help`, `detail` — then the code-specific keys of §10.3:
  - `code` and `name` are both the code's name (`"fs_busy"`);
  - `severity` is `"error"`; `span` is `null`; `expected` is `[]`;
  - `suggest` holds the did-you-mean candidates, nearest first, at most 5;
  - `message` is line 1's message rendered with V = 256 and not fitted; `help` is the help text or `null`; `detail` holds
    the detail lines without their indentation.
- The two crash lines (`durability_failure`, `store_io_fault`) have no JSON form: the process ends after writing them.

### 8.7 MCP

- A tool result's `content[0].text` is byte-identical to what the CLI writes on stdout for the same call and profile,
  its final LF included.
- `format: "json"` returns the §8 envelope as that text ([90 §6.7]); no `structuredContent` is emitted ([90 §2.2]).

### 8.8 Members named for other chapters

A link object in JSON ([40 §6.1], [LQ/envelope], [API]) carries, among its members:

| Member | Type | Content |
|---|---|---|
| `state` | string | the link state string of [F18 §4.4] |
| `detail` | string or null | the token of its principal part ([F18 §4.6], §4.7 rule 2), or `null` when it has none |
| `parts` | array of string | the tokens of every part of [F18 §4.7] rule 1, in that order |
| `score` | number or null | the score, two decimals, when a part carries one |
| `resolver` | int | the resolver version under which the link was resolved ([F18 §2.10], [F20 §1.3]) |

The other members of a link object are [API]'s.

## 9. Error, warning and hint texts

### 9.1 Errors

1. **LQ codes** use [LQ/errors §3]'s located and unlocated forms (`error[<code> <name>]: <message>`).
2. **This chapter's codes** use the unlocated form without a numeric code:

```
error[<name>]: <message>
  <detail line>
  = help: <help>
```

   - Line 1 is always present. `<message>` is at most 120 bytes of template plus its values (V = 64, §2.5 rule 5).
   - Detail lines are indented two spaces, at most 10, each at most 120 bytes of template plus values.
   - The help line is present when the code has a help text for the case.
3. **Exceptions**: the `store_read_only` text of §11.4 (four unindented lines, [90 §5.3]); the one-line crash texts
   `durability_failure` and `store_io_fault` (§10.2).
4. **Bound.** Each error text, LF bytes included, is at most 600 B ([50 §5.2], [90 §4.3]). The fitting steps 2, 4, 5 and 7
   of [LQ/errors §3.4] apply to this chapter's texts in that order.
5. **Channel and count.** Stdout, stderr under `--ids` (§2.1); at most three errors per command, separated by one empty
   line ([LQ/errors §3.1]). An error that follows rows is placed in the footer block after the budget-use line and before
   the continuation line ([LQ/envelope §6.1]).

### 9.2 Warnings

1. **LQ warnings** are `Wnn: <message>` ([LQ/errors §4.2]).
2. **This chapter's warnings**: line 1 `warning[<name>]: <message>`, continuation lines indented two spaces, at most 600 B.
3. **Place**: in the footer block after the LQ warnings, in the order raised; under `--ids`, on stderr (§6.3). `doctor`
   prints them as its body lines.
4. JSON: in the envelope's `warnings` array, as `{"code":<name>,"name":<name>,"message":<string>,"detail":[<string>...]}`
   ([LQ/errors §4.3]).

### 9.3 Hints

1. A **hint** reports a hint class of §12.3: line 1 `hint[<class name>]: <message>`, at most 600 B.
2. **Place**: in the footer block after the notices and before the `dropped:` line; under `--ids`, on stderr.
3. JSON: the envelope's `hints` array (§8.2 row 9), `{"class":<name>,"code":<int>,"message":<string>}`.

### 9.4 One fix, printed as text

Every error names exactly one fix ([50 §5.2]). When the fix is a command or a text to write, the help line prints it
([90 §8.1] L4; [LQ/errors §2.4] for LQ). No text of this chapter names a harness-specific tool name ([90 §8.1] L6): tools
are "the `query` tool", "the `write` tool".

## 10. The error table of this chapter

### 10.1 Placeholders

[LQ/errors §2.2]'s placeholders hold; this chapter adds:

| Placeholder | Rendering |
|---|---|
| `<store>` | the store directory's canonical absolute path (§2.4) |
| `<path>`, `<dir>`, `<file>` | a project path, or a file-system path, bare or quoted (§2.5 rule 3) |
| `<spec>` | a path or anchor spec as the caller wrote it (an `--at` value, a file-verb operand), bare or quoted (§2.5 rule 3) |
| `<tree>` | a tree's display label (§4.3) |
| `<treepath>` | a tree's canonical absolute path |
| `<oserr>` | the OS-error unit (§2.4) |
| `<t>` | a duration in milliseconds, decimal |
| `<key>` | an idempotency key, bare or quoted |
| `<lease>`, `<handle>`, `<intent>` | §2.4 |
| `<skey>` | a conflict or violation key in [F12]'s text form, single-quoted in a command (T3) |
| `<staging>` | a staging ref (`merge/<dst>/from/<src>`, `import/<ref>`, [AR §5a.7]) |
| `<verb>` | the verb path as typed (`file mv`) |
| `<fs>` | a file-system name: [OS/env]'s `FsName`; on Linux the `statfs` magic (`0x` and 8 lower-case hexadecimal digits) followed by the `mountinfo` type in parentheses when it was read ([OS/env] open point 9) |

### 10.2 Texts

Messages are line 1 after `error[<name>]: `; "detail" lists the detail lines; "help" the `= help:` text.

| Code | Exit | Case | Message | Detail | Help |
|---|---|---|---|---|---|
| `internal` | 1 | a panic | `internal error: panic at <file>:<line>: <panic message>` | `moirai <version> <target triple>` | `re-run with the same idempotency key (a committed write replays); report this text if it repeats` |
| | | a failed internal check; a `relink` value outside [F18 §5.1]'s grammar at write time ([F18 §5.7]) | `internal error: <check> failed` | as above | as above |
| | | a name moirai built that the OS refuses ([OS/fs §6.2] `InvalidName`) | `internal error: the OS refused a name moirai built: <oserr>` | as above | as above |
| `usage` | 2 | unknown verb or flag | `unknown verb <word>`; `unknown flag <flag> for moirai <verb>` | — | `did you mean <s>?` when a candidate lies within Levenshtein distance 2; else `moirai <verb> --help` |
| | | flag value | `<flag> needs a value`; `<flag> takes <type>; got <value>` | — | `moirai <verb> --help` |
| | | a missing operand | `moirai <verb> needs <operand>` | — | for an id: `'#' starts a shell comment; write 40` ([80 §4.2] T9); else `moirai <verb> --help` |
| | | extra or conflicting arguments | `unexpected argument <arg>`; `<a> and <b> cannot be combined` | — | `moirai <verb> --help` |
| | | argv not UTF-8 ([OS/shell §4] item 1) | `argument <n> is not valid UTF-8` | — | `pass non-ASCII text on stdin or with -f FILE` |
| | | an input file | `cannot read <file>: <oserr>` | — | — |
| | | stdin or `-f FILE` longer than `input.max-bytes` ([CFG §10.5], [LQ/lexical §8]; pass 1, P1-39) | `<source> is longer than input.max-bytes (<n> bytes)` — `<source>`: `stdin` or the file | — | `split the input, or raise input.max-bytes` |
| | | MCP parameters | `<tool>: missing <param>`; `<tool>: unknown parameter <param>`; `<tool>: <param> must be <type>; got <value>`; `<tool>: <param> takes one of <list>` | — | — |
| `bad_path` | 2 | P1, P4 ([OS/path]) | `<path> is refused: <reason> (<rule>)` — `<reason>`: `a segment contains "\"`, `a segment contains a control character`, `a name is not valid UTF-8`, `a name has an unpaired surrogate`, `it has an empty, . or .. segment`; `<rule>`: `P1` or `P4` | — | P4: `rename the file; such a name cannot be linked`; P1: `write a root-relative path with / separators` |
| | | a root that is not configured | `root <name> is not configured` | — | `moirai config set --user roots.<name> PATH` |
| | | a drive-relative `X:rel` or a device form `\\.\…`, `\\?\…` path argument on Windows ([OS/path §7] step 3; pass 1, round 1, P1-37) | `<path> is refused: <reason>` — `<reason>`: `a drive-relative path depends on that drive's current directory`, `a device path bypasses Windows name checks` | — | `write the full path X:/..., or a path relative to the current directory` |
| `nonportable_name` | 2 | `file mv` to a name with a P5 issue under `files.portable-names = refuse` ([OS/path §8.2]) | `<path> is not portable: <issue>` — `device-name`: `<stem> is a Windows device name`; `trailing-dot-or-space`: `it ends in a dot or a space`; `reserved-char`: `it contains <char>`; `too-long`: `a segment is longer than 255 bytes`; `fold-sibling`: `it differs from <sibling> only in case or normalization` | — | `choose another name, or pass --allow-nonportable` |
| `ambiguous_path` | 2 | a relative spec matches several files ([40 §3.8]) | `<spec> matches <n> files in <tree>` | the matches, root-relative, ascending bytewise, at most 10 | `write the root-relative path` |
| `anchor_spec` | 2 | span out of range ([F20 §6.1] step 1) | `<path>:<L>-<M> is outside the file (<n> lines)` | — | — |
| | | span on content that is not text ([F20 §6.1], its open point 31) | `<path> is not text; a span anchor needs text` | — | `link the whole file: moirai link <id> --at <path>` |
| | | quote input with U+FFFD ([F20 §6.1] step 3) | `the quote text contains U+FFFD (a replacement character)` | — | `pass the original bytes with --quote-file FILE` |
| | | quote input empty after trimming | `the quote text is empty` | — | — |
| | | quote not in the file | `the quote does not occur in <path>` | — | `copy the quote from the file as it is now` |
| | | a `path::A/B` or `path#H` form while [F20 §6.1]'s interim scanner rule holds ([F08 §10.3.1]; pass 1, round 1, A1-14, S1-4, P1-20) | `<spec>: symbol and heading anchors are not available yet` | — | `anchor the lines instead: --at <path>:L-M, with the item's first and last line` |
| | | symbol or heading not found ([F21 §6.5], once its interim rule is lifted) | `<path> has no <kind> <scope>` | — | `candidates: <list>` when scope names lie within Levenshtein distance 2 |
| | | several items match, or the one match's name path names several items ([F21 §6.5]; spec sync 2b) | `<spec> matches <n> items in <path>` | their scope texts ([F14 §5.6]), one per line, in pre-order, at most 10 | `anchor the lines instead: --at <path>:L-M` |
| | | the matching item's name path is not recordable ([F21 §2.3], §6.5) | `<spec> names an item whose name path cannot be stored (over 64 segments or 4,096 bytes)` | — | `anchor its lines: --at <path>:L-M records its nearest recordable ancestor` |
| | | the file's scan failed ([F21 §2.7], §6.5) | `<path> could not be scanned as <lang>` | — | `anchor the lines instead: --at <path>:L-M` |
| | | a malformed selector: an empty segment, a malformed `X[Y]`, a segment over 4,096 bytes ([F21 §6.1], §6.5) | `<spec> is not a valid symbol or heading selector: <why>` — `<why>`: `an empty segment`, `a malformed X[Y]`, `a segment longer than 4,096 bytes` | — | — |
| | | a span of trivial lines with an empty window (I-F9, [F18] open point 23) | `<path>:<L>-<M> holds only blank or brace lines and no line around it to anchor on` | — | `anchor a line with text, or link the whole file` |
| `bad_value` | 2 | a value that does not fit its type or shape outside LQ binding ([F08 §5.4.5], [F06 §5.2]) | `<field> expects <shape>; got <value>`; `NaN is not a value of <field>` | — | — |
| | | a path value whose root differs from its node's `root` ([F18 §2.8]) | `<path>'s root <r> differs from the node's root <s>` | — | — |
| | | a git branch name that is not UTF-8 ([F18 §3.2] rule 1) | `the git branch of <tree> is not valid UTF-8` | — | `rename the branch` |
| | | `--base` ([F18 §3.5]) | `--base <sha> does not name exactly one commit of <tree>'s repository` | — | `write more hex digits` |
| | | a commit message that [F07 §5.2] refuses (spec sync 2b) | `the commit message <why>` — `<why>`: `is not valid UTF-8 or contains U+0000`, `is longer than 65,535 bytes`, `ends in a paragraph that begins with Moirai-` | — | for the last: `reword the last paragraph; Moirai- lines are the image's trailers` |
| `bad_ref_name` | 2 | a ref name that breaks [F12 §2.1] or [F12 §2.4] (RN-1–RN-6, IN-3); pass 1, A1-39 | `<name> is not a valid ref name: <reason>` — `<reason>`: `use lower-case a-z, 0-9, _ and - in segments joined by . and /` (RN-1), `branches start with lane/ or plan/, tags with tags/` (RN-1, IN-3), `--kind <k> does not match <prefix>` (IN-3), `the segment <s> is reserved` (RN-2), `the segment <s> reads as a commit or sequence number` (RN-3), `the segment <s> is a Windows device name` (RN-4), `the segment <s> ends in .lock` (RN-5), `it is longer than 128 bytes` (RN-6) | — | — |
| `ref_exists` | 2 | a new ref whose name a live ref holds (RN-7) | `<name> already exists` | — | — |
| `ref_prefix` | 2 | a new ref whose name is a prefix of a live ref's, or the reverse (RN-8) | `<name> and <other> cannot both exist: one is a prefix of the other` | — | — |
| `config_key` | 2 | `config get\|set\|unset` of an unregistered key ([CFG]) | `unknown configuration key <key>` | — | `did you mean <s>?`, else `moirai config list --defaults lists the keys` |
| `config_value` | 2 | `config set` with a value outside the key's type or range ([CFG]) | `<key> takes <type>; got <value>` | — | `<key>: <allowed values or range>` |
| `not_found` | 3 | a named object other than a revision or an LQ node id | `<what> <value> does not exist` — `<what>`: `lease`, `anchor`, `run`, `lane`, `intent`, `conflict key`, `staging ref`, `backup`, `path`, `file node`, `directory`; optionally ` on <ref>` | — | staging ref: `moirai conflicts lists the open staging refs`; directory (a `file mv` destination parent, [F18] open point 28): `create the directory, then run the command again`; else — |
| | | a write outside LQ that names a node that is not live on its view ([AR §5d.3] L3, [RULES/delete-policy-matrix] DP-003; [API] open point 5; pass 1, A1-39) | `node <id> is not live on <ref>` | the tombstone line or the not-found line of [LQ/errors] N01, N06 | — |
| `commit_pruned` | 3 | `revert`, `cherry-pick`, `diff` or `history --patch` of a commit `gc` pruned to its header ([F06 §4.4.15]) | `commit <c8> was pruned by gc; its changes are gone` | — | — |
| `not_writer_tree` | 5 | a file verb or R4 write outside the writer tree ([40 §3.4], [40 §5.3]) | `<tree> is not the writer tree of <ref> (writer tree: <tree>)`; `<ref> has no writer tree` | every role: `use a raw mv or rm instead; links follow by evidence once the code reaches <tree>, or ask the orchestrator` (with no writer tree: `ask the orchestrator to bind a tree for <ref>`). Orchestrator and owner roles only (review A-m4): `bind: moirai worktree bind <treepath> <ref> --replace` (without `--replace` when <ref> has no writer tree) | — |
| `tree_mismatch` | 5 | an explicit tree outside the presented lease's lane, for a tree-derived write ([90 §4.1] Tree row) | `tree <tree> is outside lease <lease>'s lane <lane>` | `lane tree: <tree>` | `drop --tree (the tool: tree); the lease fixes the tree` |
| `binding_conflict` | 5 | the checks of `lane open` and `worktree bind` (I-F12, [F18 §3.5]) | `<ref> already has the designated tree <tree>`; `<tree> is already the designated tree of <ref>` | every role: `ask the orchestrator`. Orchestrator and owner roles only: `move the designation: add --replace` | — |
| `placement_refused` | 6 | `init` would shadow a store ([F02 §2.2]) | `<path> would shadow the store <store>` | — | `pass --force --shadow to create it anyway` |
| | | `init --here` in a linked worktree | `a store per worktree is refused; this repository's store is <path>` | — | `run moirai init without --here` |
| | | `init --here` below the main worktree's top level | `--here works only at the top level of the main worktree <path>` | — | `run it in <path>` |
| | | a `.git` entry that does not resolve | `<path> is a .git entry whose common directory does not resolve` | — | `repair the repository, then run moirai init again` |
| | | a non-store entry or a bad pointer found by the guard | `<path> is not a store or a valid pointer file; --force --shadow does not override this` | — | `moirai doctor store` |
| | | the target exists (`init`, `init --link`, `backup DIR` without `--force`) | `<path> already exists` | — | `backup`: `pass --force`; else — |
| | | `init --link` to a non-store | `<path> is not a store` | — | — |
| | | `restore --into` a directory that is not empty | `<dir> is not empty` | — | `restore into an empty directory` |
| `staged` | 6 | a merge, sync, import, revert or cherry-pick that staged ([AR §5a.7] step 8) | `<op> staged on <staging>: <v> violations, <c> conflicts; <ref> did not move` — `<op>`: `merge of <src> into <dst>`, `sync of <lane>`, `import of <ref>`, `revert of <c8> onto <ref>`, `cherry-pick of <c8> onto <ref>` | one line per violation, `<skey> <class> <description>` (§12.6); under `--strict`, one line per conflict, `<skey> <class>`; at most 10, then `... <n> more` | `moirai conflicts <staging> lists each one with its fix` |
| `staging_exists` | 6 | a second merge of one pair; a sync of L while `merge/<L>/from/main` exists (I41′) | `<staging> is open` | — | `finish it with moirai merge --continue <src> --into <dst>, or drop it with moirai merge --abort <src> --into <dst>` |
| `conflicted_src` | 6 | a merge into `main` while src holds unresolved conflicts, those its own step-0 sync just landed included ([AR §5a.7] step 0, [RULES/merge-table] PR-003, [API §11.7]) | `<src> holds <n> unresolved conflicts; a merge into main needs none` | the keys, at most 10 | `resolve them on <src> (moirai conflicts <src>), then merge again` |
| `links_not_ok` | 6 | `links check --strict`, `merge-check --strict-links` ([AR §7.1]) | `<n> of <m> links are not ok (--strict)` | — | — |
| `repin_needs_at` | 6 | `links fix --repin` after a match that is not exact ([40 §3.7]) | `<handle> matched as <qualified state>, not exactly; --repin recaptures only an exact match` | — | `read moirai file where <handle> --evidence, then pass --at SPEC naming the intended place` |
| `confirm_refused` | 6 | `links fix --confirm` ([F18 §5.5]) | `<id>'s relink is <how>/...; nothing to confirm`; `<actor> set this guess; another actor confirms it`; `role <role> may not confirm (files.confirm-roles = <list>)` | — | — |
| `path_claimed` | 6 | a write that would give an I-F1 key a second live holder ([F18 §2.1]) | `<root>:<path> is already held by <id>` | — | `link to <id>, or resolve the PathClaim first` |
| `quiet_mode` | 6 | a verb or flag quiet mode refuses ([AR §6.6]) | `<verb> is refused in quiet mode` | — | `pass --force, or run moirai quiet off` |
| `name_taken` | 6 | `run open` with a run name the view already holds ([API §10.7]; pass 1, A1-39) | `run <name> already exists on <ref>` | — | `choose another run name` |
| `not_merged` | 6 | `branch -d` of a branch whose tip `main` has not absorbed ([AR §5a.9], [API §11.2]) | `<ref> is not merged into main` | — | `merge it first, or delete it with moirai branch -D <ref>` |
| `revert_refused` | 6 | `revert` of a `sync` commit, of a merge without `--mainline 1`, or of a commit with a dependent set ([AR §5a.5], [API §11.10]) | `<c8> is a sync commit; a sync is never reverted`; `<c8> is a merge; revert it with --mainline 1`; `<c8> has dependent commits` | for a dependent set: the dependents as `<c8> <message>`, at most 10, then `... <n> more` | for a dependent set: `revert the dependents first, newest first` |
| `not_fresh` | 6 | `file mv` of an alias source whose node this tree is not fresh for ([40 §3.4], [API §12.4] step 1) | `<path> is an old path of <id>, and this tree has not seen its latest move` | — | `moirai links sync, then run the command again` |
| `no_store` | 7 | discovery found nothing ([F02 §3.1] step 5, [F02 §3.5] step 5) | `no moirai store found for <dir>` | `walk-up: <n> directories from <dir> to <top>, no .moirai entry`; `git hint: <path> and <path>: nothing there`, `git hint: no .git entry above <dir>` or `git hint: off (discovery.git-hint = false)`; `store reachable by the git hint: <store>` when the hint is off and would have found one; `denied: <path> (<oserr>)`, at most 3 | CLI: `moirai init --link <store> links this directory to a store; moirai init creates one`; MCP without `tree`: `pass tree = your working directory` ([90 §2.2]) |
| `not_a_store` | 7 | an entry that is not a store ([F02 §3.2]); `--store` or `MOIRAI_DIR` naming one | `<path> is not a store (initialisation in progress, or damaged)`; `<path> (from --store) is not a store directory`; `<path> (from MOIRAI_DIR) is not a store directory` | — | `moirai doctor store` |
| `bad_pointer` | 7 | a malformed or stale pointer file ([F02 §3.3] rule 5) | `pointer file <path> is malformed: <reason>` — `line <n> does not match the grammar`, `it is larger than 4,096 bytes`, `it has lines after store-id`; `pointer file <path> is stale: <reason>` — `its target is not a store`, `its target is a pointer file`, `the store ids differ` | `target: <path>` when it parsed; `store id: recorded <32 hex>, found <32 hex>` when both exist | `remove <path>, then run moirai init --link <store>` |
| `swap_in_progress` | 7 | a `restore` swap intent ([F02 §3.2]) | `a restore swap of <store> is running or was interrupted (<file>)` | — | `retry; if it persists, run moirai doctor, which completes or rolls back the swap` |
| `store_retired` | 7 | discovery yields again, after [F16] P-86's probe delays, a store whose `HEAD.flags.retired` is set with no swap intent beside it: a `restore` ended without clearing the flag ([F02 §3.6], [F16] P-85, its open point 10; pass 1, round 2) | `<store> is marked retired by a restore that did not finish, and no swap is pending` | `nothing was read or changed` | `run moirai doctor, which clears the flag` |
| `no_canonical_path` | 7 | a directory whose canonical path the OS cannot give: `GetFinalPathNameByHandleW` fails on a volume mounted only in a folder or on a virtual provider ([OS/path §4.1] step 3; pass 1, round 1, P1-37) | `<dir> has no canonical path on this volume: <oserr>` | `nothing was changed` | `reach it through a drive letter, or move the repository to a volume that has one` |
| `other_store` | 7 | an MCP call whose `tree` belongs to another store ([F02 §3.5], [90 §2.2]) | `<tree> belongs to the store <store>; this server serves <store>` | — | `use the moirai server of that repository, or the CLI there` |
| `format_version` | 7 | a structure with a newer format version ([F01 §9.1], [F03 §4.2] LH-2) | `<file> has format version <n>; this moirai reads version 1` | `nothing was read or changed` | `use a moirai release that reads format <n>` |
| `refused_location` | 7 | the environment guard at `init`, `restore` and every open ([OS/env §3], [80 §2.6]) | `<store> is refused: <reason>` — by reason id: `fs-type` `the file system <fs> is not supported here (supported: <list>)`; `network` `it is on a network volume (<fs>)`; `unc` `it is on a UNC path`; `cross-kernel` `it is on another kernel's file system (<fs>)`; `cloud` `it is in a cloud-managed folder (<cloud kind>)`; `fuse` `it is on a FUSE mount (<fs>)`; `overlay` `it is on an overlay file system`; `volatile` `it is on a volatile file system (<fs>)`; `no-durable-flush` `the file system refused <call> (<oserr>)`; `no-byte-locks` `the file system refused byte-range locks (<oserr>)`; `no-noreplace-rename` `the file system refused a no-replace rename (<oserr>)`; `os-too-old` `<os> <found> is older than the minimum <minimum>` | at `init` and `restore`: `nothing was created` | `os-too-old`: `update to <os> <minimum> or later`; every other reason: `keep the repository and its store on a local <list> volume` |
| `read_only_volume` | 7 | a write on a read-only volume ([OS/env §7]) | `<store> is on a read-only volume: reads work, writes do not` | — | — |
| `readonly_flag` | 7 | a write, maintenance or GC while `HEAD.flags.readonly` is set ([F04 §5.2]) | `the store is read-only (HEAD flag readonly): reads work, writes do not` | — | — |
| `store_read_only` | 7 | a writer that cannot open `LOCK` or the log for writing ([90 §5.3], [OS/env §7]) | §11.4 | §11.4 | §11.4 |
| `sandbox_write` | 7 | a sandboxed CLI that cannot write outside the store: an image destination, `backup DIR`, the `restore` swap ([80 §2.6]) | `this sandbox cannot write <path> (<reason>)` | `owner fix: add "/<parent>" to sandbox.filesystem.allowWrite`, where `/<parent>` is `/` followed by the canonical absolute path of the destination's parent | — |
| `store_locked` | 7 | a store lock's bounded wait timed out before anything was written: the writer byte's ([OS/lock §4], [AR §4.5] step 5), or the flush byte's while a rotation held it ([F16] P-72; pass 1, P1-31), or the retries of a quiet-mode requester that found all nine quiet bytes busy ([F03 §3.1] rule 4; pass 1, P1-10) | `the <lock> lock of <store> was held for <t> ms (<key>)` — `<lock>` `<key>`: `writer` `lock.writer-wait-ms`, `flush` `lock.flush-wait-ms`, `quiet` `lock.writer-wait-ms` (every quiet byte busy, [F03 §3.1] rule 4; pass 1, P1-10) | writer lock: `holder: <cmd> (pid <pid>, <activity>, since <timestamp>, session <liveness>)` from `WriterDiag` ([F03 §6.3] WD-4): `<cmd>` quoted and cut to 80 bytes, `<activity>` the enumeration name, `<liveness>` `alive`, `dead`, `unknown` or `none` for a zero session hash; `holder not recorded` when the record fails its check; then, for both locks, `nothing was written` | `retry; a dead holder's lock is released by the OS` |
| `maintenance_busy` | 7 | an explicit maintenance verb (`gc`, `backup`, `repair`, `maintain`) that finds the maintenance byte held; the byte is only tried, never waited for ([F16] P-1, P-76; [OS/lock §6]; pass 1, P1-31) | `maintenance of <store> is running in another process` | `nothing was changed` | `retry when it ends; moirai doctor agents names the holder` |
| `no_slot` | 7 | `file mv` or `file rm` with no liveness slot ([F03 §8.4] SR-4, §8.7) | `no liveness slot of <store> is free` | `nothing was changed` | `retry when fewer moirai processes run; moirai doctor agents lists slot use` |
| `outcome_pending` | 7 | the flush byte's bounded wait timed out after the append ([AR §4.5] step 10.2, [OS/lock §4]) | `the commit is appended but not yet durable after <t> ms (lock.flush-wait-ms); outcome pending` | `key: <key>` | `re-run the same command with the same key: it completes the commit, or replays it once durable` |
| `outcome_unknown` | 7 | a group lost before its flush twice ([AR §4.5] step 10.5, [50 §5.9] step 5) | `outcome unknown: re-run with the same key or check moirai changes` ([AR §6.4]) | `key: <key>` | — |
| `durability_failure` | 7 | `fail_stop` ([OS/fs §4.4.5]), stderr, one line; a `FlushFailed` from `create_root`, `swap_dirs` or `swap_recover` ([OS/fs §6.2]) | the whole line: `error[durability_failure]: <call> (<class>) failed: <oserr>; outcome unknown: re-run with the same key or check moirai changes` — `<call>` the failing OS call's name (`DurabilityFailure.call`); `<class>` the durability class's name as [80 §2.3.1] and [F15 §4.1] spell it: `durable`, `durable+meta`, `durable-name` or `sync_group` (the `DurabilityClass` variants `Durable`, `DurableMeta`, `DurableName`, `SyncGroup`; a `FlushFailed` is `durable-name`; `lazy` never fails this way) | — | — |
| `store_io_fault` | 7 | the mapping-fault handler ([OS/map §8], [80 §2.5] rule 6); a writer's scan that fails to read the log at or above `durable_lsn` ([F16] P-92; pass 1, S1-25); stderr, one line | the whole line, frozen by [80 §2.5] rule 6 without an `error[` prefix: `store I/O fault in <file> at <offset>: run moirai doctor --fsck` | — | — |
| `sealed_size` | 7 | a sealed file whose size differs from its `total_len` after one re-read of `HEAD` ([80 §2.5] rule 4, [F10 §2.4], [OS/map §4]) | `<file> is <n> bytes but its header says <m>` | — | `run moirai doctor --fsck` |
| `store_corrupt` | 7 | `LOCK` of another size or with a bad header ([F03 §2.1], §4.2); `HEAD` of another size or a fatal slot ([F04 §2], §7); a segment that fails an open check ([F09 §17.2]); an invalid group below `durable_lsn` ([F05 §5.3], decision (b)); a valid log record whose payload is malformed, wherever it lies ([F05 §5.4], [F06 §2.4]; pass 1, closure NC-6) | `<file> is damaged: <what>` — `<what>`: `its size is <n> bytes, not <m>`, `its header fails its check`, `a slot passes its checksum but not its validity rules`, `its <section> fails its check`, `an invalid group at lsn <L> lies below the durable end of the log`, `the record at lsn <L> has a malformed payload` | — | an invalid group of the log: `run moirai repair`; a malformed payload and every other file: `run moirai doctor --fsck` |
| | | no valid `HEAD` slot after three reads ([F04 §8.1]) | `HEAD has no valid slot` | — | `run moirai repair` |
| `id_space_exhausted` | 7 | a write that needs an id beyond its space ([AR §4.5] step 4, [F01 §8.1] S4, [F02 §6.2], [F04 §5.7], [F05 §2.3], [F06 §4.4.3], [F08 §2.1], [F08 §8.3]) | `the <space> space of this store is full at <max>` — `<space>`: `node id`, `anchor handle`, `commit sequence`, `log extent`, `<family> file number`, `symbol class <class>`, `schema <space> id` | `nothing was written` | — |
| `commit_too_large` | 7 | a group that no extent can hold, even as a bulk commit ([F06 §4.6]) | `the commit needs <n> bytes; a log extent holds <m>` | `nothing was written` | `split the write into smaller commits` |
| `uid_collision` | 7 | a derived uid that is all zero ([F08 §2.2]) | `the derived uid of <what> is all zero and cannot be stored` | `nothing was written` | — |
| `fs_busy` | 7 | errors 5 and 32 after the bounded retry, `Busy`, and the Unix busy states of `file mv` and `file rm` ([OS/fs §6.2], §6.3, [F20 §5.19]) | `<op> of <path> failed after <t> ms and <n> retries: <oserr>` — `<op>`: `rename`, `delete`, `open` | a directory: `a process has an open handle or its current directory inside <path>` and `common causes: a shell cd'ed inside, an editor, a watcher`; a file whose holders are known: `held by: <image> (pid <pid>)`, at most 3; a file verb: `intent <intent> aborted; nothing changed` | `close what holds it, then run the command again` |
| `cross_volume` | 7 | `file mv` across volumes ([OS/project §6.4], [F20 §5.19]) | `<path> and <path> are on different volumes; moirai file mv never copies` | `intent <intent> aborted; nothing changed` when an intent was opened | `move it with a raw mv; links re-bind by evidence or show a proposal to confirm` (pass 1, A1-59) |
| `no_dir_flush` | 7 | `file mv`, `file rm` or `file revert` whose plan step finds that a parent directory on the project volume cannot be flushed (`Unsupported` or `AccessDenied` from `sync_dir`: SMB, FUSE, `\\wsl$`; [OS/project §6.2], [API §12.4] step 1; pass 1, P1-16); `doctor`, for an intent whose recovery re-barrier met such a volume and left it open ([F16] P-71; round 1) | `<dir> is on a volume where moirai cannot flush a directory: <oserr>` | `nothing was changed` | `move it with a raw mv; links re-bind by evidence or show a proposal to confirm` |
| | | `file rm --trash` with the store on another volume ([40 §3.5]) | `<path> is on another volume than the store; --trash moves within one volume` | — | `run moirai file rm without --trash` |
| `disk_full` | 7 | a write, flush or create that failed with disk full, or the sparse-extent early warning (decision (f), [OS/fs §6.2]) | `no space left on the volume of <path>: <oserr>` | `the command was not acknowledged` | `free space, then run the command again with the same key` |
| `no_git` | 7 | an image verb that needs the git command ([AR §7.1], [AR §5b.8]) | `image <verb> needs the git command for <what>, and git is not on PATH` — `<what>`: `a reftable destination`, `the remote <remote>` | — | `install git, or export to a destination with refStorage = files (moirai image export --to DEST --create)` |
| `partial_batch` | 8 | `apply` over several runs; `image import` or `image pull` over several refs | `<k> of <n> <units> committed; <f> failed` — `<units>`: `runs`, `refs`; `<f>` is n minus k | one line per failed unit, at most 10: `<unit>: <code or name> <message>` | `re-run with the same idempotency keys: committed units replay, failed ones run again` |

### 10.3 Code-specific JSON keys

After [LQ/errors §4.1]'s common keys (§8.6), in this order:

| Code | Keys |
|---|---|
| `internal` | `"component":<string>`, `"version":<string>` |
| `usage` | `"argument":<string or null>` |
| `bad_path`, `nonportable_name` | `"path":<string>`, `"rule":<string>` (the rule id, the issue name, or `drive-relative` or `device` for [OS/path §7] step 3) |
| `ambiguous_path` | `"matches":[<string>...]` |
| `anchor_spec`, `bad_value`, `placement_refused`, `confirm_refused` | `"case":<string>`, naming the case in the order of §10.2's rows for the code: `range`, `binary`, `fffd`, `empty`, `not-found`, `no-scanner`, `no-scope`, `several`, `not-recordable`, `scan-failed`, `syntax`, `no-window`; `shape`, `nan`, `root`, `branch-utf8`, `base`, `message`; `shadow`, `linked-worktree`, `subdirectory`, `broken-git`, `non-store`, `exists`, `link-target`, `not-empty`; `not-a-guess`, `same-actor`, `role`) |
| `bad_ref_name` | `"name":<string>`, `"rule":<string>` (`RN-1` … `RN-6`, `IN-3`) |
| `ref_exists` | `"name":<string>` |
| `ref_prefix` | `"name":<string>`, `"other":<string>` |
| `config_key`, `config_value` | `"key":<string>` |
| `not_found` | `"what":<string>` (`node` for a node that is not live), `"value":<string>` |
| `commit_pruned` | `"commit":<string>` |
| `name_taken` | `"name":<string>` |
| `not_merged` | `"ref":<string>` |
| `revert_refused` | `"commit":<string>`, `"case":<string>` (`sync`, `mainline`, `dependents`), `"dependents":[<string>...]` |
| `not_fresh` | `"path":<string>`, `"node":"#N"` |
| `no_dir_flush` | `"dir":<string>`, `"os":{"code":<int>,"symbol":<string>}` |
| `not_writer_tree`, `binding_conflict` | `"tree":<string>` (canonical path), `"writer_tree":<string or null>`, `"ref":<string>` |
| `tree_mismatch` | `"tree":<string>`, `"lease":<string>`, `"lane_tree":<string>` |
| `staged` | `"staging_ref":<string>`, `"violations":[{"key":<string>,"class":<string>,"code":<int>,"description":<string>,"suggested":<string>}...]`, `"conflicts":<int>` |
| `staging_exists` | `"staging_ref":<string>` |
| `conflicted_src` | `"keys":[<string>...]`, `"sync":{"commit":<string>,"outcome":"landed","conflicts":[{"key":<string>,"class":<string>}...],"violations":[]}` or `null` (the step-0 sync this command appended on src before it refused, [API §11.7] "Sync first with conflict values"; `null` when src held the conflicts before the command) |
| `links_not_ok` | `"not_ok":<int>`, `"total":<int>` |
| `repin_needs_at` | `"anchor":<string>`, `"state":<string>` |
| `path_claimed` | `"root":<string>`, `"path":<string>`, `"holder":"#N"` |
| `no_store` | `"dir":<string>`, `"examined":[<string>...]`, `"hinted_store":<string or null>` |
| `not_a_store`, `swap_in_progress`, `store_retired` | `"path":<string>` |
| `no_canonical_path` | `"dir":<string>`, `"os":{"code":<int>,"symbol":<string>}` |
| `bad_pointer` | `"pointer":<string>`, `"state":"malformed"` or `"stale"`, `"target":<string or null>`, `"recorded_id":<32 hex or null>`, `"found_id":<32 hex or null>` |
| `other_store` | `"tree":<string>`, `"store":<string>`, `"served_store":<string>` |
| `format_version` | `"file":<string>`, `"version":<int>` |
| `refused_location` | `"reason":<string>` (the reason id), `"fs":<string or null>` |
| `store_read_only` | `"store":<string>`, `"profile":<string>`, `"mcp_call":<string or null>` |
| `sandbox_write` | `"path":<string>`, `"allow_write":<string>` |
| `store_locked` | `"lock":"writer"`, `"flush"` or `"quiet"`, `"waited_ms":<int>`, `"holder":{"cmd":<string>,"pid":<int>,"activity":<string>,"since":<timestamp>,"session":<string>}` or `null` (always `null` for the flush and quiet locks) |
| `no_slot` | none |
| `outcome_pending`, `outcome_unknown` | `"key":<string>` |
| `sealed_size` | `"file":<string>`, `"size":<int>`, `"total_len":<int>` |
| `store_corrupt` | `"file":<string>`, `"what":<string>` |
| `id_space_exhausted` | `"space":<string>`, `"max":<int>` |
| `commit_too_large` | `"bytes":<int>`, `"extent_bytes":<int>` |
| `fs_busy` | `"path":<string>`, `"op":<string>`, `"os":{"code":<int>,"symbol":<string>}`, `"retries":<int>`, `"waited_ms":<int>` |
| `cross_volume` | `"path":<string>`, `"intent":<string or null>` |
| `disk_full` | `"path":<string>`, `"os":{"code":<int>,"symbol":<string>}` |
| `no_git` | `"verb":<string>` |
| `partial_batch` | `"committed":<int>`, `"failed":[{"unit":<string>,"error":<error object>}...]` |
| every other code | none |

### 10.4 Warnings

| Name | Raised by | Text |
|---|---|---|
| `nonportable_name` | `link`, `file add`, `file mv` under `files.portable-names = warn` ([OS/path §8.2]) | `warning[nonportable_name]: <path> is not portable: <issue>`, the issue texts of §10.2 |
| `not_a_tree` | `worktree bind DIR REF` on a directory that is not a tree ([F18 §3.5]) | `warning[not_a_tree]: <dir> is not a git tree: binding only, not a designated tree` |
| `hook_label_narrowed` | a hook label that disagrees with the presented lease's role ([90 §4.3]) | `warning[hook_label_narrowed]: the hook label <label> differs from lease <lease>'s role <role>; the rights are their intersection` |
| `lease_moved` | a write with `--move-lease <ref>` that moves the presented lease's branch ([AR §5a.4], [API §4.3]; spec sync 2b) | `warning[lease_moved]: lease <lease> moved from <ref> to <ref>` |
| `two_harnesses` | detection with two harnesses' variables ([90 §4.1]); `doctor agents` | `warning[two_harnesses]: variables of <harness> and <harness> are both set: profile generic, no session identity` |
| `foreign_lock` | `doctor` ([OS/lock §11]) | `warning[foreign_lock]: another program holds a lock on <store>/LOCK; moirai's waits may stall behind it` |
| `flushing_disabled` | `doctor` ([OS/env §8]) | `warning[flushing_disabled]: write-cache buffer flushing is turned off for the disk of <store>; a flush may not reach the disk` |
| `no_barrier` | `doctor` | `warning[no_barrier]: <store> is on ext4 mounted with barrier=0 or nobarrier` |
| `write_cache_forced` | `doctor` | `warning[write_cache_forced]: the disk of <store> is set to write-through over a volatile cache` |
| `removable_drive` | `doctor` | `warning[removable_drive]: <store> is on a removable or external drive; some USB bridges ignore flushes` |
| `untested_os` | `doctor` | `warning[untested_os]: <os> <found> is allowed but outside the tested set` |
| `user_config_unresolved` | `doctor` ([F02 §7.3] rule 2, [OS/path §10]) | `warning[user_config_unresolved]: the user configuration file has no location (<variable> is unset or not absolute)` |
| `graph_only_revert` | `revert` or `cherry-pick` of a commit whose group carried an `FsIntentDone` ([40 §3.6], [API §11.10]; pass 1, A1-39) | `warning[graph_only_revert]: <c8> moved files on disk; moirai file revert <c8> moves them back` |

### 10.5 Refusals named by other chapters

Each refusal another chapter delegates here, with its code. A chapter that states an exit code agrees with this table.

| Refusal | Source | Code | Exit |
|---|---|---|---|
| a structure with a newer format version | [F01 §9.1], [F03 §4.2] LH-2 | `format_version` | 7 |
| an exhausted symbol class | [F01 §8.1] S4 | `id_space_exhausted` | 7 |
| a store file number beyond 2^32 − 1 | [F02 §6.2] | `id_space_exhausted` | 7 |
| `init` placement and the shadow guard | [F02 §2.1]–§2.2, its open point 13 | `placement_refused` | 6 |
| a refused location at `init` | [F02 §2.4], [OS/env] | `refused_location` | 7 |
| discovery: nothing found; not a store; a bad pointer; a swap | [F02 §3.1]–§3.3 | `no_store`, `not_a_store`, `bad_pointer`, `swap_in_progress` | 7 |
| discovery: a retired store with no swap intent | [F02 §3.6], [F16] open point 10 | `store_retired` | 7 |
| `LOCK` of another size or with a bad header | [F03 §2.1], §4.2 | `store_corrupt` | 7 |
| no liveness slot free | [F03 §8.4], §8.7 | `no_slot` | 7 |
| the writer wait timed out | [F03 §6.3] WD-4, [OS/lock §4] | `store_locked` | 7 |
| the flush wait at a rotation timed out; every quiet byte stayed busy | [F16] P-72, [F03 §3.1] rule 4 | `store_locked` | 7 |
| `HEAD` of another size; a fatal slot; no valid slot | [F04 §2], §7, §8.1 | `store_corrupt` | 7 |
| `next_id` or `next_anchor` beyond 2^32 − 1 | [F04 §5.7] | `id_space_exhausted` | 7 |
| `HEAD.flags.readonly` | [F04 §5.2] | `readonly_flag` | 7 |
| an extent beyond `log.4294967295` | [F05 §2.3] | `id_space_exhausted` | 7 |
| an invalid group below `durable_lsn` | [F05 §5.3] | `store_corrupt` | 7 |
| a valid log record with a malformed payload, wherever it lies | [F05 §5.4], [F06 §2.4] | `store_corrupt` | 7 |
| a commit `seq` beyond 2^32 − 1 | [F06 §4.4.3], [AR §4.5] step 4 | `id_space_exhausted` | 7 |
| an over-long commit | [F06 §4.6], its open point 17 | `commit_too_large` | 7 |
| NaN at write; a value that does not match its shape | [F06 §5.2], [F08 §5.4.5] | `bad_value` | 2 |
| `#N` = 2^32 | [F08 §2.1] | `id_space_exhausted` | 7 |
| an all-zero derived uid | [F08 §2.2] | `uid_collision` | 7 |
| exhausted schema id spaces | [F08 §8.3] | `id_space_exhausted` | 7 |
| a result that does not conform to the schema (I11) | [F08 §8.6] | the binder's E1xx code, else E405 with the rule `schema conformance (I11)` ([LQ/errors §5.5]) | 2 or 6 |
| `rm` of a root node | [F08 §11.3], [RULES/delete-policy-matrix] DP-010 | E409, its root-node case ([LQ/errors §5.5]) | 6 |
| a node delete under a live lease without `RELEASE` (I32′) | [API §9.1], [RULES/delete-policy-matrix] DP-005 | E409, its lease case ([LQ/errors §5.5]) | 6 |
| a delete whose replacement is not live, lies in the deleted set or does not fit a re-pointed edge | [RULES/delete-policy-matrix] DP-007 | E409, its replacement case ([LQ/errors §5.5]) | 6 |
| a segment or sealed file that fails an open check | [F09 §17.2] | `store_corrupt` | 7 |
| a size mismatch before mapping | [F10 §2.4], [OS/map §4] | `sealed_size` | 7 |
| a structural violation on a write | [F13 §5] VO-3 | §12.4 | 6 |
| the `suspect` budget exceeded | [F17 §8.2] | hint `SuspectBudget` (§12.3) | 0 |
| a second live holder of an I-F1 key | [F18 §2.1] | `path_claimed` | 6 |
| a path value whose root differs from its node's `root` | [F18 §2.8] | `bad_value` | 2 |
| a non-UTF-8 git branch; an invalid `--base` | [F18 §3.2], §3.5 | `bad_value` | 2 |
| I-F12 binding checks | [F18 §3.5] | `binding_conflict` | 5 |
| `--confirm` with nothing to confirm, by the acceptor, or by a role outside `files.confirm-roles` | [F18 §5.5] | `confirm_refused` | 6 |
| a `relink` value outside the grammar at write time | [F18 §5.7] | `internal` | 1 |
| a missing destination parent of `file mv` | [F18] open point 28 | `not_found` (`directory`) | 3 |
| a span of trivial lines with an empty window | [F18] open point 23 | `anchor_spec` | 2 |
| anchor capture refusals | [F20 §6.1], its open point 31 | `anchor_spec` | 2 |
| Unix busy states of `file mv` and `file rm` | [F20 §5.19] | `fs_busy` | 7 |
| cross-volume `file mv` | [F20 §5.19], [OS/project §6.4] | `cross_volume` | 7 |
| a project volume that cannot flush a directory | [OS/project §6.2], [API §12.4] | `no_dir_flush` | 7 |
| a pruned commit named by `revert`, `cherry-pick` or `diff` | [F06 §4.4.15] | `commit_pruned` | 3 |
| a busy maintenance byte seen by an explicit maintenance verb | [F16] P-76, [OS/lock §6] | `maintenance_busy` | 7 |
| ref-name grammar, an existing name, a prefix clash | [F12 §2.1], §2.4 | `bad_ref_name`, `ref_exists`, `ref_prefix` | 2 |
| a run name in use; an unmerged branch; a refused revert; a stale alias source | [API §10.7], §11.2, §11.10, §12.4 | `name_taken`, `not_merged`, `revert_refused`, `not_fresh` | 6 |
| a write to a node that is not live | [API §9.1], [RULES/delete-policy-matrix] DP-003 | `not_found` (`node`) | 3 |
| argv that is not UTF-8 | [OS/shell §4] item 1 | `usage` | 2 |
| a drive-relative or device-form path argument | [OS/path §7] step 3 | `bad_path` | 2 |
| a directory whose canonical path the OS cannot give | [OS/path §4.1] step 3 | `no_canonical_path` | 7 |
| stdin that is not UTF-8 | [OS/shell §5.2] step 4, [80 §4.2] T6 | E003 | 2 |
| a durability failure | [OS/fs §4.4.5] | `durability_failure` | 7 |
| a mapping fault | [OS/map §8] | `store_io_fault` | 7 |
| a failed read of the log at or above `durable_lsn` in a writer's scan | [F16] P-92, [F05 §5.3] | `store_io_fault` | 7 |
| the environment guard's refusals | [OS/env §3], §6, §7 | `refused_location`, `read_only_volume`, `store_read_only`, `sandbox_write` | 7 |

### 10.6 Configuration diagnostics

The configuration diagnostics `CFG01`–`CFG17` ([CFG §6.2]) are numbered by this chapter, which owns code numbering across
the specification (pass 1, A1-39): the numbers and names of [CFG §6.2]'s table are frozen as they stand, and [CFG §6.2]
owns their texts and severities. They are warnings or notices and never change an exit code ([LQ/errors §4.4]). In text a
diagnostic renders `CFGnn: <message>`; its continuation lines are indented seven spaces, the width of `CFGnn: `; in the
footer block they follow the LQ warnings and notices, in ascending code order ([LQ/errors §4.2]; pass 1, A1-62). In JSON
they are objects of the `warnings` array ([LQ/errors §4.3]).

## 11. The refusals of [90 §10.1]

[90 §10.1] row "Error table and refusal texts" freezes one new code, two exit-5 texts and mechanical fixes printed as
replacement text. The codes are LQ codes; [LQ/errors §5.5] owns their bytes, restated here.

### 11.1 The unknown-model write: E411

- **Code**: `E411 unknown_model_write`, exit 6, raised by the `tx` binder before any execution ([LQ/errors §5.5]; the
  [PLAN §3.3] gap "unknown-model write error code number").
- **When**: the caller's model profile, resolved by [90 §4.1]'s Model row and [90 §8.2], is `unknown`,
  `query.safelist.model.unknown` is `named-only` (the default) or `dry-targets`, and the call is a free-form `TX` — not a
  named mutation and not a verb. Under `dry-targets` a `TX` that carries `IF TARGETS <digest>` from a `DRY` of the same
  block is admitted. Under `off` nothing is refused ([90 §8.1] L2, [90 §10.8]).
- **Text** ([LQ/errors §5.5]):

```
error[E411 unknown_model_write]: free-form TX is refused for a model with the unknown profile
  use the named mutation <name> (the write tool: name and params)
```

  The detail is `no named mutation matches; ask the orchestrator` when none matches; under `dry-targets` a second detail
  line reads `or run the block with DRY and apply it with IF TARGETS`.

### 11.2 The two exit-5 texts

Both are E407 rows ([LQ/errors §5.5]), exit 5, raised before any write ([90 §4.1]):

1. **A declared agent that differs from the presented lease's holder** — outside `claim`, where `--agent` names the
   holder of the new lease:

```
error[E407 lease]: declared agent <agent> differs from lease <lease>'s holder <holder>
  = help: drop --agent, or pass your own lease
```

2. **An environment lease bound to another thread** — a use of `MOIRAI_LEASE` through the environment from an attested
   thread other than the one it bound to at first use; an explicit `--lease` or `lease` is never refused on this ground:

```
error[E407 lease]: <lease> is bound to <identity>; pass your own lease
```

`<identity>` is the namespaced identity of the bound thread (`codex:<thread>`, [90 §4.1]).

### 11.3 E406's unleased text

The unleased refusal of [AR §7.3] and [90 §4.3] is frozen (the [PLAN §3.3] gap "whether E406's new fix text is frozen").
The design writes it as one line (152 B); this chapter **renders** it as a message line and a help line, split at the
semicolon: the words are the design's, the `; ` becomes an LF, two spaces and `= help: `, and nothing else changes
([LQ/errors §5.5] renders it the same way; pass 1, A1-51). [RULES/role-write-policy] WZ-001 quotes the design's one-line
form, which is the same text:

```
error[E406 role_policy]: this write needs a lease
  = help: an orchestrator presents its session lease with --lease (mint it once per session: moirai claim --role orchestrator --session)
```

### 11.4 The exit-7 sandbox texts: `store_read_only`

A write verb that cannot open `LOCK` or a log extent for writing (`ERROR_ACCESS_DENIED`, `EROFS`, `EPERM`, `EACCES`;
[OS/env §7]) prints four lines, each ended by LF, within 600 B ([90 §5.3]):

```
error[store_read_only]: this sandbox cannot write the moirai store <store> (<reason>)
do now: <do-now>
<fallback>
owner fix: <owner-fix>
```

| Profile | Line 1 | `<do-now>` | `<fallback>` | `<owner-fix>` |
|---|---|---|---|---|
| `codex` | as above | `repeat this write with the moirai MCP tool: <call>; do not request escalated permissions for it` | `if the moirai tools are unavailable: put this write in your final result.v1 (or tell the user) and continue` | by `integrate.codex.store-writes`: `writable-root`: `moirai integrate codex --store-writes writable-root   (prints the sandbox_workspace_write.writable_roots line to add)`; `execpolicy-store`: `moirai integrate codex --store-writes execpolicy-store`; `execpolicy`: `moirai integrate codex --store-writes execpolicy`; `mcp`: `none needed; integrate.codex.store-writes = mcp routes writes through the MCP tools` |
| `claude` | as above | as `codex` | as `codex` | `add "/<store>" to sandbox.filesystem.allowWrite (moirai integrate claude --sandbox-allow)` |
| `generic` | `error[store_read_only]: this process cannot write the moirai store <store> (<reason>)` | `use the moirai MCP tools if this session has them` | `otherwise ask the user to run this command outside the sandbox` | `moirai doctor sandbox` |

1. `<reason>`: `access denied` for `AccessDenied`, `read-only file system` for `EROFS` ([OS/fs §6.2]).
2. `<call>`: the equivalent MCP call by [AR §7.2]'s verb-to-tool mapping, written `<tool>{<k>: <v>, ...}` with the
   parameters in the tool's declaration order, integers bare, strings in double quotes, and free text elided as `"..."`
   ([90 §5.3]: `complete{id: 89, lease: "L-18", outcome: "done", summary: "..."}`).
3. A verb with no MCP equivalent (the CLI-only verbs of [AR §7.2]) prints `do now: <verb> has no moirai MCP tool; do not
   request escalated permissions for it`.
4. `/<store>` is `/` followed by the store's canonical absolute path (`//home/u/repo/.git/moirai`, [80 §2.6]).
5. These texts are ASCII and golden in GT12 (M8). Exit 7 keeps its meaning; only the text is per profile ([90 §5.3]).

## 12. Violation classes and the named-query validator (F18)

### 12.1 The class code space

A class is stored as one `u8` code wherever a stored structure carries one: the `class` of a `Conflict` op and of a
`Violation` op ([F06 §7.7]) and of a `CONFLICTS` row ([F11], [50] F11). One code space holds every class of [AR §5a.8]:

| Codes | Classes | Owner |
|---|---|---|
| 0 | none; invalid in every stored structure | — |
| 1–63 | value-conflict classes (`FieldEdit`, `StatusFork`, `TextHunk`, `DeleteVsModify`, `SupersedeFork`, `OwnerFieldEdited`, `PathClaim`; [AR §5a.8]'s `DATA` has no code, [F12 §6.1]; pass 1, P1-31) | [F12] |
| 64–127 | structural-violation classes: may be stored as `Violation` ops | this chapter, §12.2 |
| 128–191 | hint classes: never stored | this chapter, §12.3 |
| 192–255 | reserved, invalid in format v1 ([F01 §5.4]) | — |

- A code is never reused for another class ([AR §2.12]). A code not listed is invalid ([F01 §9.3]).
- Texts, JSON and the image name a class by its name, never by its code. Whether a canonical form carries a conflict value's
  class by name is [F07]'s ([F01] open point 9).

### 12.2 Structural classes: the violation-class enum

| Code | Name | Raised by | Rule |
|---|---|---|---|
| 64 | `HierarchyCycle` | V01 (a Kleppmann move skipped because it would close a cycle) and V05 ([F13 §5]) | the `parent` forest has no cycle (I4) |
| 65 | `Cycle` | V03 ([F13 §5]); [RULES/merge-table] VA-016 | the combined precedence graph is acyclic (I5′); the declared-acyclic kinds |
| 66 | `DanglingEdge` | V04 | structural edges have live endpoints (I2) |
| 67 | `DepthExceeded` | V05 | the `parent` forest has depth ≤ 12 (I4); [F13] OP-13-06, confirmed at pass 1 (P1-21, S1-33) |
| 68 | `Cardinality` | V07 | `duplicate_of` chains of length 1 (I7), `runs_in` ≤ 1, `answers` ≤ 1 active ([AR §3.3]); confirmed at pass 1 |
| 69 | `SchemaConflict` | V09 | schema conformance, strengthening included (I11) |
| 70 | `QueryInvalid` | V10 (§12.5) | every named query in scope parses and binds ([50] F18) |
| 71 | `QueryCycle` | V11 (§12.5) | the named-query call graph is acyclic ([50] F18) |
| 72 | `PlanMask` | V12 | `plan/*` masked fields are not written (I33′); confirmed at pass 1 |
| 73 | `RemovedTextNotInBase` | the text merge rule ([RULES/merge-table] MR-036) | a `doc.section` body's removed text exists in the base |
| 74 | `IdCollision` | import ([AR §5b.6] step 4); merge ([RULES/merge-table] MR-045) | two creations of one random uid |
| 75 | `ImageParse` | import ([AR §5b.6] step 4, [F14]) | an image file that does not parse or re-bind |
| 76 | `NotFound` | revert and cherry-pick (I34′) | the node the reverted op touched exists |
| 77 | `TombstoneRemoved` | import ([AR §5b.6] step 2) | a referenced node's tombstone is still in the image; unreferenced, the same class is a hint line (§12.3) |

The codes follow [F13 §5]'s validator order for the validator classes, then the classes of the typed rules, import and
revert.

### 12.3 Hint classes

A hint lands as a hint line (§9.3) and is never stored ([AR §5a.8] "yes, as a log line").

| Code | Name | Trigger | Text after `hint[<name>]: ` |
|---|---|---|---|
| 128 | `Duplicate` | V13 ([RULES/merge-table] HT-001) | `<id> and <id> are <kind> nodes with the same title under <id>` |
| 129 | `Contradiction` | V13 (HT-002) | `rules <id> and <id> apply to the same scope and contradict each other`, or `rules <id> and <id> apply to the same scope and have the same heading` |
| 130 | `ForeignMerge` | import of a foreign git merge (HT-003) | `git commit <7 hex> is a foreign merge; imported as a typed merge of its <n> parents` |
| 131 | `SuspectBudget` | a commit whose `suspect` changes exceed `store.suspect-budget` ([F17 §8.2]) | `suspect changed on <n> nodes, above store.suspect-budget (<P23>); affected is incomplete and past views recompute derived state` |
| — | `TombstoneRemoved` (code 77) | import, the node unreferenced | `the image no longer holds the tombstone of <id>; nothing references it` |

### 12.4 Outcome by command

On a write (a verb, `tx`, `apply`, MCP `write`) a structural violation refuses the whole command with nothing written
([F13 §5] VO-3); on a merge, sync, import, revert or cherry-pick it becomes one `Violation` op and the command stages
([AR §5a.7] step 8, I12), ending with `staged` (§10.2), exit 6.

| Class | On a write | On the merge family |
|---|---|---|
| `HierarchyCycle`, `DepthExceeded` | E405, rule `forest depth 12 (I4)` | staged |
| `Cycle` | E405, rule `acyclic precedence (I5')` | staged |
| `DanglingEdge` | E405, rule `live endpoints (I2)` | staged |
| `Cardinality` | E405, rule `canonical duplicate target (I7)` for `duplicate_of`; for `runs_in` and `answers` the rule `at most one <edge> (cardinality)` ([LQ/errors §5.5]) | staged |
| `SchemaConflict` | the binder's E1xx code, else E405 with the rule `schema conformance (I11)` ([LQ/errors §5.5]) | staged |
| `QueryInvalid` | E405 with the rule `named queries bind (QueryInvalid)` ([LQ/errors §5.5]) | staged |
| `QueryCycle` | E405, rule `named-query cycle (QueryCycle)` | staged |
| `PlanMask` | E305 `field <field> is masked on plan branches` | staged |
| `RemovedTextNotInBase`, `IdCollision`, `ImageParse`, `NotFound`, `TombstoneRemoved` | cannot arise | staged |
| a value-conflict class ([F12]) | cannot arise ([F13 §5] VO-3) | lands, unless `--strict` stages it |

### 12.5 The named-query validator (V10, V11)

**12.5.1 When it runs.** V10 and V11 of [F13 §5] run on every candidate C — a write's ([50 §5.9] step 2) and a merge's,
sync's, import's, revert's or cherry-pick's ([AR §5a.7] step 6, [50 §4.4]) — whose net changeset against its first parent D
changes a schema item: a kind, a field, an edge kind, an enumeration value or a `QUERIES` item ([50] F1–F3). On any other
candidate they report nothing.

**12.5.2 Scope.** S is the set of named queries of C for which at least one holds:
1. Q's `QUERIES` item differs between D and C (Q was defined or redefined);
2. Q's stored text, parsed, contains a `CALL P(<args>)` whose name resolves to a project query P whose item differs
   between D and C, or that C no longer holds (P was redefined or dropped);
3. C changes a schema item other than a `QUERIES` item; then every named query of C is in S.

*(Informative)* Rule 3 over-approximates "whose referenced schema it touched" ([50 §4.4]). Every branch head holds only
queries that parse and bind and has an acyclic call graph (I12), so a query outside [50]'s set passes V10 and lies on no
cycle: S yields exactly [50]'s violations, and an engine may restrict the work to [50]'s set.

**12.5.3 V10: `QueryInvalid`.** For each Q in S, in ascending bytewise order of its name: parse Q's stored text with the
grammar of its recorded LQ grammar version, start symbol `define_stmt` ([LQ/grammar-v1.ebnf §P.1]), then bind it against
C's schema and C's other named queries as [50 §4.4] binds a definition, with the canonical-AST algorithm of its grammar
version ([LQ/canonical-ast]). A parse or bind error makes one `QueryInvalid` violation keyed by Q's `QUERIES` item. Its
**first diagnostic** is the reported error with the least span start, ties broken by the smaller code.

**12.5.4 V11: `QueryCycle`.** G is the directed graph whose vertices are the named queries of C and which has an edge
Q → P for every `CALL P(<args>)` in Q's parsed text whose name resolves to the project query P (name lookup is `std`
first, then the project, [50 §4.4] "Invocation"). A Q that does not parse contributes no edge. `std.*` and `tx.*`
definitions are not vertices: they are fixed, call no project query and have no call cycle among themselves ([LQ/std] F18
row). Every strongly connected component of G that has two or more vertices, or one vertex with an edge to itself, and that
contains a query of S, makes one `QueryCycle` violation:
- its **witness** w is the component's least query name (bytewise), and its key is w's `QUERIES` item;
- its **cycle** is the first path back to w that a depth-first search from w finds when it visits the successors inside
  the component in ascending name order.

**12.5.5 Order.** V10's violations precede V11's ([F13 §5] VO-2); within each, ascending bytewise by the key's query name,
which is the canonical key order of `QUERIES` items ([F07]; open point 17).

**12.5.6 Outcome.** §12.4. On a write the first violation refuses the block with E405: for `QueryCycle` the rule
`named-query cycle (QueryCycle)` with the cycle as its detail line; for `QueryInvalid` the rule `named queries bind
(QueryInvalid)` ([LQ/errors §5.5]) with the detail line `<name>: <code> <message>` from the first diagnostic.

### 12.6 `Violation` op contents for F18's classes

The `Violation{class, description, suggested}` op ([AR §4.3], [F06 §7.7]) carries, for the two classes of F18:

| Field | `QueryInvalid` (70) | `QueryCycle` (71) |
|---|---|---|
| `class` | 70 | 71 |
| `description` | `named query <name> no longer parses: <code> <message>` or `named query <name> no longer binds: <code> <message>`, from the first diagnostic ([LQ/errors] texts), the message cut to 160 bytes | `named queries call each other in a cycle: <w> -> <q2> -> ... -> <w>`, at most 8 names, then `-> ...` |
| `suggested` | the first of: `moirai resolve <skey> --take <s>` for the first s of (`ours`, `theirs`, `base`) whose definition of Q, put in C in place of Q's, parses and binds; otherwise `moirai resolve <skey> --value - with a definition of <name> that binds against this schema` | the first of: `moirai resolve <skey> --take <s>` for the first s of (`ours`, `theirs`, `base`) whose definition of w, or its absence, leaves w on no cycle of C's graph; otherwise `moirai resolve <skey> --value - with a definition of <w> that does not call <q2>` |

- `<skey>` is the `QUERIES` item's key in [F12]'s text form, single-quoted (proposed form `query:<name>`, open point 17);
  the sides are those of the operation ([RULES/merge-table]: ours = dst, theirs = src, base = the base state).
- Names are value bytes (§2.5 rule 3). Each field is at most 400 bytes of UTF-8, values cut by §2.5 rule 2.
- GT2 compares class, key and order exactly ([F13 §5] VO-2); `description` and `suggested` follow [LQ/errors]' texts and
  are compared by the M7 goldens.

### 12.7 Rendering

A violation renders as [LQ/envelope §5.10] says: `<skey> <class> <description>`, then
`  suggested: <suggested>, then moirai merge --continue <src> --into <dst>`. In a `staged` error text (§10.2) each is one
detail line `<skey> <class> <description>`. A conflict renders as [LQ/envelope §5.10] says.

## Coverage

| Item | Part covered here | Section |
|---|---|---|
| [60 §2.5] "Harness-agnostic interface and pure Rust": the output contract's byte units, both-ends, ASCII and `--ids` page rules | complete (the LQ shapes that apply them are [LQ/envelope]'s) | §2.3, §3, §5, §6 |
| [60 §2.5] "Harness-agnostic interface and pure Rust": one error code and two exit-5 refusal texts | complete; the bytes are shared with [LQ/errors §5.5] | §11.1, §11.2 |
| [60 §2.5] R5 row F18: violation classes `QueryInvalid`, `QueryCycle` and the named-query merge validator | complete | §12 |
| [50] F18 | complete: class codes 70 and 71 in the violation-class enum, the validator's scope, order, witness and outcome, and the `Violation` op contents | §12.1–§12.7 |
| [60 §2.5] R4 row R-16 | part: the placement and limits of `files: no tree bound` and the reader note, the tree display label, the link markers and the JSON members `state`, `detail`, `parts`, `resolver`; the strings are [F18 §4]'s | §4.3, §4.4, §4.6, §8.8 |
| [60 §2.5] preamble: "readers refuse a newer version" | part: the exit code and text; the rule is [F01 §9.1]'s | §10.2 `format_version` |
| [60 §2.5] protocol decisions (b) and (f) | part: the refusals' codes and texts (`store_corrupt`, `disk_full`); the protocol is [F16]'s | §10.2 |
| [80] X-F12 | part: T6's exit 2 (E003), T7's output bytes by citation of [OS/shell §6], T9's hint text; argv and stdin are [OS/shell]'s | §2.2, §10.2 `usage`, §10.5 |
| [80] X-F6 | part: the mapping-fault line and the size-mismatch refusal, exit 7, and the environment guard's refusal texts; the mechanisms are [OS/map] and [OS/env]'s | §10.2 `store_io_fault`, `sealed_size`, `refused_location` |
| [80] X-F5 | part: `fail_stop`'s line and exit 7; the mechanism is [OS/fs]'s | §10.2 `durability_failure` |
| [80] X-F4 | part: the exit-7 texts of a timed-out writer wait and of a missing liveness slot; the contract is [OS/lock]'s | §10.2 `store_locked`, `no_slot` |
| [80] X-F3 | part: the exit-7 texts of a pending and of an unknown outcome; the protocol is [F16]'s | §10.2 `outcome_pending`, `outcome_unknown` |
| [90 §10.1] "Output contract": byte units in headers and footers, the both-ends rule, ASCII in every rendered string, header limits with `dropped`/`more`, the `--ids` page rule | complete | §2.3, §3.1, §4.2, §5, §6 |
| [90 §10.1] "Error table and refusal texts": the unknown-profile code, the two exit-5 texts, mechanical fixes as replacement text | complete; texts shared with [LQ/errors] | §9.4, §11 |

## Holes

None. No value of this chapter waits on an M0 measurement. The ceilings of §3.2 are [CFG] keys with the design's
defaults; WP-81a records any change that measurement 7 (the Bash tool's caps) or probe P3 ([90 §10.5]) motivates in [CFG]
and [AR §13], not here. The display spelling inside LQ replacement texts is `HOLE(LQ-display-spelling)`, owned by
[LQ/gql-spelling] (the id formerly written `display-spelling`, [HOLES.md §4]). The default of `mcp.result-max-bytes.codex`
(§3.2) is `HOLE(CFG-codex-mcp-result)`, owned by [CFG].

## Open points for the review

1. **Header limits: per part, not totals (review A-M3; conflict).** [90 §6.3], [90 §9.2], [90 §10.1] and [50 §6.4] state
   totals (≤ 60 B, ≤ 100 B with `files @`, ≤ 90 B and ≤ 130 B with `dropped`/`more`); [AR §7.1] and [AR §8.3], amended
   after the A1 re-review, count per part and say the totals "no real worktree path could meet". [F01 §2.4] rule 2 gives
   [90] precedence for its own reservations, but [AR]'s text is the later, reviewed amendment that answers A-M3, so §4.2
   follows [AR]. For a header without `files` and extras the two agree (60 + 30 = 90 B). The review confirms. Consequences
   for WP-19: [LQ/envelope §3.3] restates the totals and must cite §4.2; "extras" is read as one budget for all extras of a
   line, as A-M3's fix asks, so [LQ/envelope §9.3]'s `DRY` header (65 B of extras in [50 §2.9] Q19) and the composite
   header of Q15 (64 B) exceed it. Proposals: drop `nothing written` from the `DRY` header (a dry run writes nothing by
   definition) and drop each part's `<c8>` from the composite rev part (JSON `parts` carries the commits). **Pass 1
   (A1-30): adopted**, with one more step the arithmetic needs: `tx (dry)` counts in the base part, in the place of the
   rows field a `DRY` result lacks (§4.2). [LQ/envelope §3.2] and §9.3 follow.
2. **Limits are golden checks.** §4.2 rule 3: moirai cuts only labels, git branches and the reader note's slots. Cutting a
   ref would weaken the D2/D3 branch signal the header exists for.
3. **The cursor text is not counted** in the continuation part (§4.2 rule 4). [LQ/envelope §8]'s cursors are at least ≈ 100
   characters, so ≤ 30 B could never hold them; [90 §6.3]'s `cursor k7f3q2` is illustrative. A cursor whose sort key holds a
   long text value grows with it; WP-19 may bound the sort-key text a cursor carries.
4. **Tree display label** (§4.3). [AR §7.1]'s rule (relative to the main worktree's parent, else the last two segments) is
   adopted. [LQ/envelope §3.2]'s "or its registered short name" comes from the review's proposal, which [AR] did not adopt;
   WP-19 removes it. `detached`, `unborn`, `no git` and the age rule are this chapter's decisions.
5. **The reader note is on line 2** ([AR §7.1], A-M3), not inside the header's `files` field, where [LQ/envelope §3.1]'s
   grammar puts `readingonly`. WP-19 moves it. Line 2 precedes the reading echo (§4.4).
6. **`--ids` and the non-zero-exit cap (conflict inside [AR §7.1]).** [AR §7.1] pages `--ids` at 24,000 B with exit 10, and
   also caps the stdout of every non-zero exit at 8,000 B because a harness shows little of a failed call ([90 §6.1]: Claude
   Code ≈ 10,000 characters). A 24,000-B page with exit 10 would be cut by Claude Code, which is what the page exists to
   prevent ([90 §2.1]). §6.2 resolves: a result that fits `output.ids-max-bytes` prints whole with exit 0; a cut prints at
   most min(`output.ids-max-bytes`, `output.nonzero-exit-max-bytes`) with exit 10; `output.ids-max-bytes` = 0 disables
   both limits for scripts. Measurement 7 re-checks the Bash tool's caps. **Pass 1 (A1-31):** [LQ/envelope §6.5] now cites
   §6.2. **Decided 2026-09-28** (owner question OQ-F-1, option (a); `reviews/owner-questions.md`): a cut `--ids` page
   holds min(`output.ids-max-bytes`, `output.nonzero-exit-max-bytes`) = 8,000 B by default and exits 10; a whole result
   still prints up to 24,000 B with exit 0. WP-81a still edits [AR §7.1]'s "pages at `output.ids-max-bytes`
   (24,000 B)" to match.
7. **`doc patch` exit (conflict).** [AR §7.1] gives exit 4 for a removed text that is not a substring; [50 §5.2] and
   [LQ/errors] put it under E404, exit 6. The error table is not a reservation of [F01 §2.4] rule 2's list, so [AR] would
   win; a code has one exit code, so following [AR] needs a new LQ code (proposal: `E412 patch_mismatch`, exit 4, the
   current body excerpt printed like E401's values). Until the review decides, §7.3 mirrors [LQ/errors].
8. **Non-LQ errors carry no numeric code.** [AR §7.1], [40 §3.8] and [90 §5.3] spell `error[store_read_only]`,
   `error[fs_busy]`; §9.1 keeps that form and puts the name in JSON `code` and `name`. The alternative, a numbered range
   printed as `error[S701 store_read_only]`, would change [90 §5.3]'s golden text.
9. **`store_read_only` per profile** (§11.4). [90 §5.3] gives the lines; this chapter adds: the `generic` first line says
   "this process" (no sandbox is known to exist), the `read-only file system` reason, the owner-fix line for each value of
   `integrate.codex.store-writes`, the `<call>` notation, and the do-now line of CLI-only verbs.
10. **The two crash lines.** The mapping-fault line keeps [80 §2.5] rule 6's text without an `error[` prefix (X-F6 is
    [80]'s reservation). The durability-failure line takes the prefix and [AR §6.4]'s "outcome unknown" clause, because the
    next flush holder may make the group durable. [AR §4.5] step 10.5 writes a shorter "outcome unknown: retry with the
    same key"; §10.2 uses [AR §6.4]'s text in both places.
11. **Panics and hooks** (§7.2 rule 8, §7.4). A Rust abort does not exit 1 on either OS, so the panic hook ends the process
    with code 1 itself. Hooks always exit 0 because Claude Code reads exit 2 as a block; [AR §7.5] says hooks fail open.
12. **`isError` for exit 10 with rows** (§7.5). A partial result that carries its continuation is not marked as an error,
    so a harness shows it as a result; its footer still says `exit 10`.
13. **Hints in text and JSON** (§9.3, §8.2 row 9). [AR §5a.8] lands hints "as a log line" without a form. The `hint[...]`
    line and the `hints` key are new; WP-19 adds `hints` to [LQ/envelope §7.1] (after `budget`) and to the footer order of
    [LQ/envelope §6.1] (after the notices).
14. **Ownership of class codes (conflict between spec files).** [RULES/merge-table] §9 and [F13] OP-13-06 put the numeric
    class codes in [F12]; [PLAN §3.2] WP-18 and this work package put F18's classes and the violation-class enum here.
    Resolution: one `u8` space (§12.1), [F12] owning codes 1–63 (value conflicts) and this chapter 64–191. [F12] cites §12.1;
    R-MODEL updates [RULES/merge-table] §9's sentence and fills VA-005, VA-007, VA-008 and VA-013 with §12.2's classes; the
    owner re-signs that table (V3). **Done** (pass 1, round 1, P1-21): §9's sentence gives §12.1's one code space, and
    VA-005, VA-007, VA-008 and VA-013 carry codes 67, 68 and 72. **Decided 2026-09-28** (owner question OQ-M-1, option
    (a)): the owner accepted the changed rows, the VA-005, VA-007, VA-008 and VA-013 codes among them; the re-signature in
    `rules/SIGNED.md` follows under V3.
15. **Three new structural classes** (§12.2, closing [F13] OP-13-06 as proposed there): `DepthExceeded` (67),
    `Cardinality` (68, `duplicate_of`, `runs_in`, `answers`) and `PlanMask` (72). [AR §5a.8] names none; classes are frozen
    format, so pass 1 confirms them.
16. **E405 rule texts and E409 cases for WP-19.** [LQ/errors §5.5]'s E405 `<rule>` list lacks `QueryInvalid`, the
    `runs_in` and `answers` cardinalities and schema conformance. Proposals: `named queries bind (QueryInvalid)`,
    `at most one <edge> (cardinality)`, `schema conformance (I11)`. [F08 §11.3]'s refusal of `rm` on a root node needs an E409
    case (`<id> is a root node`). [F17] OP-17-11 asks [LQ/errors] to list both causes (`wmem`, the inline bound) under E501.
    **Closed** (pass 1, round 1, P1-21, P1-11): [LQ/errors §5.5] takes the three rule texts and the root-node case of
    E409, and [LQ/errors §5.4]'s E501 names the inline bound beside `wmem`; §10.5 and §12.4 cite them.
17. **The named-query validator** (§12.5–§12.6).
    - The scope rule 3 over-approximates [50 §4.4]'s set; I12 makes the results equal (§12.5.2).
    - The key text form `query:<name>` is a proposal for [F12]; [F07] must order `QUERIES` items by name bytes for §12.5.5.
    - `resolve <key> --take` and `--value -` on a violation's key (not a conflict value) must be accepted on a staging ref;
      [F12] and M3's goldens confirm, or the suggested text changes.
    - A stored definition whose `#u:` literal names a uid the importing store does not know fails to bind (E111) and so
      stages `QueryInvalid` on import. The review decides whether that is intended.
18. **`TombstoneRemoved` has one code** (77) for its structural and its hint case (§12.3); the code range says only whether a
    class can be stored.
19. **`SuspectBudget`** (§12.3) answers [F17] OP-17-15 with a hint class; it never changes the exit code.
20. **Exit codes decided here.** `placement_refused` exit 6 ([F02] open point 13); `format_version` exit 7 confirmed
    ([F01] open point 14); every exhausted id space exit 7, following [AR §4.5] step 4 and [F04 §5.7] (exit 6 was
    considered: the store stays readable, but [AR] fixes 7 for `seq`, and one code for every space is simpler);
    `commit_too_large` and `uid_collision` exit 7 ([F06], [F08]); `bad_value` exit 2 and `path_claimed`, `confirm_refused`
    exit 6, `binding_conflict` exit 5 ([F18] open point 31); `quiet_mode` exit 6; the missing `file mv` destination parent
    `not_found` exit 3 ([F18] open point 28).
21. **Verbs without a header** (§4.1 rule 3). [AR §7.1] says "every result's first line is `branch: <ref>`", while its own
    examples of `merge`, `image export` and `backup` begin otherwise. This chapter keeps the header for every verb with a
    view (a staged merge prints the `staged` error) and none for store-level verbs.
22. **The pack and brief lines** (§4.5, answering [RULES/pack-classes] open point 12). A separate limit of 200 B; the pack
    continuation carries `--role` (and the other arguments given), because `moirai pack 51 --more` alone ([AR §7.4]) cannot
    reproduce the pack; the digest is `<rev>-<8 hex>` (NR-002), where [AR §7.4] shows `digest 7f3a`; the last line uses
    [90 §6.3]'s `dropped: <what> | more: <continuation>` rather than [AR §7.4]'s `-> moirai pack 51 --more`. The MCP `brief` tool has no
    continuation parameter, so a brief delivered through MCP prints the CLI continuation; [AR §7.2] may add one at M10.
    The names of [RULES/pack-classes]' digest parameter (NR-001) are a naming decision of WP-25, not a measurement; this
    chapter agrees with them.
23. **Link markers** (§4.6, answering [F18] open point 17 and review A-m1). Markers use [F18 §4.7]'s qualified forms.
    The `was:` quote stays in link lines ([F18 §4.7] rule 5) and leaves markers; `pending` prints the plain state rather
    than [F18]'s proposed `pending: not in this tree yet`, to keep the qualified-form rule. **Pass 1 (A1-29):** the draft's
    restated gate (every marker ≤ 90 B, golden median ≤ 50 B) changed [AR §8.3] without an owner decision and is
    withdrawn. The shapes are shortened instead, so that every marker meets [AR §8.3]'s ≤ 50 B at the widest handle: the
    `moved-needs-confirm` marker drops the candidate path (which the `verify` command prints; [40 §6.2]'s `moved? ->
    storage-v2.md` example is informative and has a 3-digit id), and an `ambiguous` marker whose qualified form would
    exceed 50 B prints the bare state (only `ambiguous (normalization collision)` with a handle of 9 or more digits). No
    owner decision is needed; if the owner prefers the richer shapes, [AR §8.3] must change at WP-81a.
24. **Examples are illustrative** (reviews A-m3, S-08). The layouts of [40 §3.8], [AR §7.1]'s example I/O and [50 §2.9]
    are re-counted and re-spelled in the goldens from §4 and [LQ/envelope]; text commits are `c<8 hex>` and JSON commits `c`
    + 64 lower-case hex digits (A-m7, [50 §2.9] Q18, §8.3; pass 1, S1-17, A1-16); `error[guard_conflict]` is E401
    ([LQ/errors] open point 3).
25. **Escapes** (§2.5, §8.3). `\u{h}` is written without leading zeros, matching the LQ literal escape; [LQ/envelope
    §5.16]'s `\u{XX}` is read so. JSON `\u00xx` uses lower-case digits. Proposal for pass 1: also escape C1 controls
    (U+0080–U+009F) and U+2028, U+2029 in the quoted form of both chapters, since some renderers treat them as line breaks.
    **Pass 1 (A1-48):** one spelling, `\u{h}` without leading zeros; [LQ/envelope §5.16] and [LQ/errors §2.1] now say so.
26. **Shell hints** (T9). moirai prints the hint as its `= help:` line (`'#' starts a shell comment; write 40`), the form
    [LQ/errors] uses, rather than [80 §4.2]'s `hint:` prefix. The zsh hint, which only skills and documentation print,
    reads `zsh: no matches found -> quote the value or use stdin` ([OS/shell] open point 4).
27. **No ANSI on a terminal either** (§2.2 rule 2). [AR §7.1] forbids ANSI off a TTY only; format v1 has none at all, so
    goldens never depend on the stream.
28. **Integers in JSON below 2^53** (§8.3), so JavaScript consumers (Codex code mode) read them exactly; wider values are
    strings. `--json` stays `--json v1` in every later release (§8.4).
29. **`partial_batch`** (exit 8) covers `apply` over several runs and `image import`/`image pull` over several refs. [AR §6.4]
    makes one run's batch all or nothing, so no single-run `apply` exits 8.
30. **Run display names.** [F11] open point 4 leaves the displayed run name to this chapter and [API]; texts show a run by
    the name given to `run open` (`run r7`, [AR §7.1]), which [API] stores.
31. **Image-verb refusals** other than `no_git` (a ref CAS that fails, a ref the exporter does not recognise, [AR §5b.8]) are
    [F14]'s and M5's goldens; they take codes of §7.1 without changing this table's form.
32. **Upper-case hexadecimal input** (§2.4, answering [F01 §6.4]'s delegation). Only git object ids typed by the caller
    accept upper case, because git does; every moirai-defined hexadecimal input stays lower-case only.
33. **Index and coverage files.** `docs/spec/README.md` still lists this chapter as `planned`, and `docs/spec/COVERAGE.md`
    needs the rows of the Coverage section above. Both files belong to other work packages (WP-10); their owners update
    them. (Pass 1: both are current.)
34. **Pass 1 changes** (S1-17, A1-16, S1-44, P1-31, S1-32, A1-43, P1-16, A1-39, A1-57, A1-59, A1-62, S1-21). JSON commit
    ids are `c` + 64 hex (§8.3). Exit 8 covers a file verb with failed items (§7.1). `store_locked` names the writer or the
    flush lock; `maintenance_busy` (exit 7) is new; `DATA` is gone from §12.1. New codes: `bad_ref_name`, `ref_exists`,
    `ref_prefix` (exit 2, [F12] open point 16), `commit_pruned` (exit 3, [F06 §4.4.15]), a `not_found` case for a node
    that is not live (exit 3, [API] open point 5), `name_taken`, `not_merged`, `revert_refused`, `not_fresh` (exit 6,
    [API] open point 20), `no_dir_flush` (exit 7, P1-16), the warning `graph_only_revert` ([API] open point 38), and
    §10.6's numbering of `CFG01`–`CFG17`. The Codex MCP ceiling cites its hole; the `generic` hook ceiling stays 8,000 B
    by [90 §6.4]; the `cross_volume` help names the proposal outcome.
35. **Pass 1, round 1** (P1-37, P1-31, P1-10, P1-16, S1-25, A1-14, S1-4, P1-20, P1-21, P1-11). New code
    `no_canonical_path` (exit 7) for a directory whose canonical path the OS cannot give ([OS/path §4.1] step 3), and a
    `bad_path` case for a drive-relative or device-form argument ([OS/path §7] step 3). `maintenance_busy` no longer
    claims a wait: the maintenance byte is only tried ([F16] P-76), so its text and JSON carry no duration. `store_locked`
    names the quiet bytes' retries (R-SPEC-P's edit, kept), and `store_io_fault` a writer's failed read above
    `durable_lsn` (R-SPEC-P's edit, kept). `no_dir_flush` also covers `doctor`'s report of an intent that recovery left
    open. `anchor_spec` refuses the `path::A/B` and `path#H` forms while [F20 §6.1]'s interim scanner rule holds (case
    `no-scanner`). The E405 rule texts and the E409 root-node case of open point 16 are in [LQ/errors §5.5], and E501's
    inline-bound case in [LQ/errors §5.4].
36. **Pass 1, round 2** (closure NC-6; [F16] open point 10). `store_corrupt` also names a valid log record whose payload
    is malformed, which [F05 §5.4] and [F06 §2.4] make corrupt wherever it lies, with `moirai doctor --fsck` as its fix
    (the invalid-group case keeps `moirai repair`); §10.5 lists it. New code `store_retired` (exit 7): discovery that
    finds a store still marked `retired` with no swap intent after [F16] P-86's probe delays, a `restore` that ended
    without clearing the flag ([F02 §3.6]); only `doctor` clears it, as [F16] open point 10 says.
37. **Pass 1, round 3** (closure NC-8, A1-39's residue). §10.5 lists the two E409 refusals that [API §9.1] and
    [RULES/delete-policy-matrix] DP-005 and DP-007 delegate: a node delete under a live lease without `RELEASE` (I32′)
    and an invalid replacement. Their texts and JSON keys are [LQ/errors §5.5] and §5.7's new cases.
38. **Spec sync 2b.** §4.2: a staging view's base part may take 80 B, since its ref name holds two ref names and no
    staged-view header fitted 60 B. §10.2: `anchor_spec` gains [F21 §6.5]'s refusals (cases `several`, `not-recordable`,
    `scan-failed`, `syntax`); `bad_value` gains the commit-message case (`message`) of [F07 §5.2]. §10.4: `lease_moved`.
    §10.3: `conflicted_src` carries `sync`, the step-0 sync a refused sync-first merge appended ([API §11.7]), because
    the error envelope of §8.6 has no `data` (independent check of the sync).
