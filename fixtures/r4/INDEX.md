# fixtures/r4: derived identities, predecessor order, path rules and `fold_v1`

| | |
|---|---|
| Title | The golden fixtures of R4's pure functions: the uid derivations of file nodes, root nodes and anchors with their hashed bytes, predecessor order and the dead-uid and anchor predecessor loops, the path rules P1–P12 as functions over data, and `fold_v1` |
| Work package | WP-21, first part: `r4/` (R-FIX; [PLAN §3.2] item 1). `canonical/` is the other half of this part. The scanner-construct fixtures of [F20] Appendix A.9, also for `r4/`, are a later part (§4 G-6) |
| Acceptance | E3 (derivations, used by WP-91's ids); WP-61 and WP-62 ("the `r4/` fixtures pass"; for WP-61, except `paths.cases` `p3-01` … `p3-06`, which are the port phase's, §3.2); the model's `r4::` functions (WP-92) |
| Separation | S1 ([PLAN §3.1]): written from the specification text only. The author read no line of `moirai-format-oracle`, `moirai-model`, `moirai-toylog` or any product crate (`moirai-files` included), and ran no project code. Hashes come from throw-away scripts that implement only what the chapters say (BLAKE3 checked against the BLAKE3 reference implementation over the standard test-vector inputs; SHA-1 and SHA-256 from the platform's library) |
| Sources | [F08 §2.2], §5.4.1, §10.3.1, §11 (derivations, root names, the scope value); [F20 §3] (`fold_v1`, `ceq`); [OS/path] §2–§8 (P1–P12, the CLI boundary, representability and portability); [F12 §2] (ref names, P11 (b)); [F18 §2.8] (I-F8); [F01 §6.3] (`lp()`), §7 (hashes) |
| Status | Written against the specification after review pass 1 (owner answers of 2026-09-28); spec sync 3 (`docs/spec/reviews/spec-sync-3.md` S3-X-1): §3.2 marks P3's six cases port phase, `p3-06` provisional, with no case byte changed |

`fold.cases` was computed from `fixtures/ucd/17.0.0/UnicodeData.txt` and `CaseFolding.txt` (their SHA-256 pins checked
first) by an implementation of [F20 §3.1] that reproduces the NFD column of every line of `NormalizationTest.txt` and
NFD(X) = X for every other scalar value. Every value is synthetic: no owner data, no real paths, users or hosts.

## 1. Files

| Path | Cases | What it asserts |
|---|---|---|
| `cases/derivations.cases` | 21 | `uid_file` (with and without a predecessor, named and `abs` roots, NFC, NFD and case spellings), `uid_root`, `captured` (every anchor kind, empty context, occurrence, window, scope) and `uid_anchor`, each with its hashed bytes |
| `cases/predecessor.cases` | 15 | Registration of a file node over a stated view ([F08 §11.2] steps 1–5): existing nodes, candidates, the greatest-uid order, tombstones, the dead-uid loop, roots and exact bytes. Capture of an anchor ([F08 §11.4] steps 1–4): reuse, a repinned collision, the predecessor chain |
| `cases/paths.cases` | 137 | P1–P12 of [OS/path §3] over data: `RelPath` and `AbsPath` grammars with their `PathError`, P12's lexical form, the CLI boundary, P3's stored spelling, P7, P5's portability issues, `representable` per OS, P8's symlink `oid`, P9's keys and lookup order, P11 (a) query file names and P11 (b) ref-name rules |
| `cases/fold.cases` | 37 | `fold_v1` over the examples of [F20 §3.1] and the Unicode edge cases (folding that changes a combining class, canonical ordering, singletons, Hangul, supplementary planes, unassigned code points); `ceq` |

`.gitattributes` gives `fixtures/** -text`: every byte of these files is kept as written.

## 2. The case format

### 2.1 Framing

As `fixtures/lq/INDEX.md` §2.1: UTF-8 text with LF line ends, cases `%% case <id>` … `%% end`, line directives
`%% <name> <value>` and block directives `%% <name>` followed by their lines; lines outside a case are comments. Values
are written as JSON strings (RFC 8259, `\u` escapes, so every argument and expectation is ASCII); only `source` and `note`
lines hold bytes outside ASCII.

### 2.2 Directives common to the files

| Directive | Kind | Meaning |
|---|---|---|
| `source`, `note` | line, repeatable | The specification sections the case rests on; commentary |
| `function` | line | The function under test (§2.3–§2.6) |
| `rule` | line | `paths.cases`: the rule of [OS/path §3] (`P1` … `P12`, or `CLI` for [OS/path §7]) |
| `arg` | line, repeatable | `arg <name> <value>`: one argument, in the function's order |
| `expect` | line | The expected result (per function below) |

### 2.3 `derivations.cases`

| Directive | Kind | Meaning |
|---|---|---|
| `input-hex` | block | The exact bytes hashed: each `lp()`-framed argument ([F01 §6.3]) after a comment line naming it; `;` starts a comment |
| `input-length` | line | Their length in bytes |
| `output` | line | BLAKE3-128 of the bytes ([F01 §7.1]): the uid or the `captured` digest, 32 hex digits |

| `function` | `arg`s ([F08 §11.1]) |
|---|---|
| `uid_file` ([F08 §11.2]) | `root <json>`, `path <json>` (the path text without its root), `pred <uid>` or `-` |
| `uid_root` ([F08 §11.3]) | `root <json>` |
| `captured` ([F08 §11.4]) | `file_uid <uid>`, `kind <name>`, `scope <hex>` or `-`, `quote`, `prefix`, `suffix`, `end` (each `<json>` or `-`), `occurrence <n>` or `-`, `window <hex>` or `-` |
| `uid_anchor` ([F08 §11.4]) | `src <uid>`, `captured <32 hex>`, `pred <uid>` or `-` |

An argument written `-` is absent and enters as `lp("")`; a `-` quote, prefix, suffix or end of a kind that has none, and
a window of a kind other than `lines`, enter as `lp("")` whatever the argument says ([F08 §11.1], §11.4).

### 2.4 `predecessor.cases`

| Directive | Kind | Meaning |
|---|---|---|
| `operation` | line | `register` ([F08 §11.2]) or `capture` ([F08 §11.4]) |
| `view` | block | The view's nodes or anchors, one per line (below) |
| `steps` | block | Informative: the steps of the section as they run on the view, with each value derived |
| `result` | line | `existing <uid>` (step 1 found the file node), `new <uid> pred <uid|->` (the node is created with that uid and `origin_pred`), `reuse <uid>` (capture step 1), or `new <uid> captured <32 hex> pred <uid|->` (a new anchor) |

`register` views and arguments:

```
file <uid> root=<name> path=<json> status=<present|planned|removed> [aliases=<json>,<json>...]    a live artifact
tombstone <uid> kind=<kind> [root=<name> path=<json>]                                             a deleted node
node <uid> kind=<kind>                                                                              any other live node
arg root <json>
arg path <json>
```

`capture` views list the anchors of the view with their **current** selectors (a repinned anchor's current selectors
differ from the inputs its `captured` was derived from); the arguments are the referrer, the file node and the new
capture's selectors:

```
anchor <uid> src=<uid> dst=<uid> captured=<32 hex> pred=<uid|-> kind=<k> quote=<json> prefix=<json> suffix=<json> [end=<json>] [occurrence=<n>] [window=<hex>]
arg src <uid>
arg dst <uid>
arg selectors kind=<k> quote=<json> prefix=<json> suffix=<json> [occurrence=<n>]
```

### 2.5 `paths.cases`

| `function` | `arg`s | `expect` |
|---|---|---|
| `relpath` ([OS/path §2.1], P1, P4) | `hex <bytes>` (omitted for the empty input), and `text <json>` when the bytes are UTF-8 | `ok` or `error <PathError>` ([OS/path §11]) |
| `abspath` ([OS/path §2.2], P12) | `text <json>` | `ok` or `error <PathError>` |
| `canonical_abs_lexical` ([OS/path §5] step 2) | `os windows|unix`, `cwd <json>` (Unix, a relative argument), `text <json>` | the `AbsPath` as `<json>` |
| `cli_path` ([OS/path §7]) | `os`, `tree <json>` (the tree's canonical text), `cwd <json>` (the canonical current directory), `text <json>` (the argument) | the `RelPath` as `<json>`, or `error <PathError>` |
| `stored_untracked_name` (P3) | `text <json>`, `norm_insensitive_always`, `git`, `precompose_unicode` (each `true`/`false`) | the stored spelling as `<json>`; `expect-cps` lists its code points |
| `origin_path` (P7) | `tracked`, `git_spelling <json>` (tracked only), `enumerated <json>`, `norm_insensitive_always` | the spelling as `<json>` |
| `portable_issues` ([OS/path §8.2], P5) | `segment <json>`, `sibling <json>` (repeatable: the siblings after the operation) | `none`, or the issues in the table order of §8.2, separated by ` , `: `device-name`, `trailing-dot-or-space`, `reserved-char <json>` (the first such character), `too-long`, `fold-sibling <json>` (the smallest such sibling in byte order) |
| `representable` ([OS/path §8.1]) | `os windows|linux|macos`; a `rows` block, one `<json segment> true|false` per line | per row |
| `symlink_oid` (P8, [F20 §2.3]) | `target <json>`, `algo sha1|sha256` | the digest in hex |
| `trees_key` ([OS/path §4.5], P9) | `text <json>` | BLAKE3-128 of the canonical text, 32 hex digits |
| `lookup_tree` ([OS/path §4.4], P9) | a `view` block of rows `row <name> text=<json> root_id=<token> os=<os>`; `text`, `root_id`, `os` of the canonical root looked up | the row's name, or `new` |
| `query_file_name` (P11 (a)) | `name <json>` | the file's path as `<json>` |
| `ref_name_check` ([F12 §2.4]–§2.5, P11 (b)) | `verb branch|tag`, `name <json>` (a complete name), `live <json>` (repeatable: the live refs) | `ok` or `refused RN-<n>` (the first rule that fails, in [F12 §2.4]'s order) |

Root ids in `lookup_tree` are opaque tokens: equal tokens are the same object ([OS/project §3.2]); `0` is an id of kind 0
(no trusted id).

### 2.6 `fold.cases`

| `function` | Directives |
|---|---|
| `fold_v1` ([F20 §3.1]) | `input-text <json>`, `input-cps` (code points), `input-utf8 <hex>`; the result as `output-cps` and `output-utf8 <hex>` |
| `ceq` ([F20 §3.4]) | `input-text <json>`, `input2-text <json>`; `output true|false` |

## 3. Coverage

### 3.1 The WP-21 `r4/` row

| Item ([PLAN §3.2] WP-21) | Where |
|---|---|
| derivations | `derivations.cases`; derived uids in use: `fixtures/canonical/cases/anchors.cases`, `checkpoint.cases` |
| predecessor order | `predecessor.cases` `reg-*` (file nodes, greatest uid, dead-uid loop) and `cap-*` (anchor predecessor term) |
| P1–P12 | `paths.cases` (§3.2); P6 is `fold.cases` |
| `fold_v1` | `fold.cases`; its use in P5 is `paths.cases` `p5-18` … `p5-21` |

### 3.2 The rules P1–P12 ([OS/path §3])

| Rule | Cases |
|---|---|
| P1 (root-relative, no empty, `.` or `..` segment) | `relpath-01` … `relpath-13` |
| P2 (tracked: git's spelling) | through P7: `p7-01` |
| P3 (NFC for untracked names on normalization-insensitive volumes) | `p3-01` … `p3-06`, **port phase** (owner decision OQ-A-5 (a), 2026-10-06; [OS/path §1] and open point 14): no M0–M11 crate runs them, the macOS port does ([PLAN §3.2] WP-61's acceptance). `p3-06` is **provisional**: its expected value (U+212B → U+00C5, as Unicode 17.0.0 NFC gives) waits for the port's test against git on APFS, because P3's function is git's precomposition, which may leave the singleton unchanged ([OS/path] open point 2); `p3-01` … `p3-05` stand under either reading. No byte of the six cases changed (spec sync 3) |
| P4 (`\`, C0 controls, not UTF-8) | `relpath-14` … `relpath-19`; `cli-15` |
| P5 (portability) | `p5-01` … `p5-22`; `representable-windows`, `representable-linux`, `representable-macos` |
| P6 (`fold_v1`) | `fold.cases` |
| P7 (`origin_path`) | `p7-01` … `p7-03`; `derivations.cases` `file-nfc-spelling`, `file-nfd-spelling`, `file-case-spelling` |
| P8 (symlinks) | `p8-01` … `p8-03` |
| P9 (canonical root; by id first) | `p9-key-01` … `p9-key-03`, `p9-lookup-01` … `p9-lookup-05` |
| P10 (relative walks) | none: an OS behaviour with no data function; `moirai-os` tests it (WP-33) |
| P11 (a) query file names, (b) ref names | `p11a-01` … `p11a-03`; `p11b-01` … `p11b-27`. P11 (c), store file names, is [F02]'s and has no case here |
| P12 (`abs`) | `abspath-01` … `abspath-19`, `lexical-01` … `lexical-07`; `derivations.cases` `file-abs-root` |
| the CLI boundary ([OS/path §7]) | `cli-01` … `cli-17` |

### 3.3 Specification sections

| Section | Cases |
|---|---|
| [F08 §11.1]–§11.2 (inputs, file uids, registration) | `derivations.cases` `file-*`; `predecessor.cases` `reg-*` |
| [F08 §11.3] (root nodes) | `derivations.cases` `root-*` |
| [F08 §11.4] (`captured`, anchor uids, capture) | `derivations.cases` `captured-*`, `anchor-*`; `predecessor.cases` `cap-*` |
| [F08 §10.3.1] (the scope value's bytes) | `derivations.cases` `captured-scope` |
| [F18 §2.8] I-F8 (exact bytes, root equality) | `predecessor.cases` `reg-exact-bytes`, `reg-other-root-ignored`; `paths.cases` `relpath-*` |
| [F20 §3.1], §3.3–§3.4 | `fold.cases` |
| [OS/path §2]–§8, [F12 §2.4]–§2.5 | `paths.cases` |

### 3.4 Rows of `docs/spec/COVERAGE.md`

The fixture column of these rows can cite (R-SPEC fills the column):

| Row | Fixture |
|---|---|
| R-3 | `fixtures/r4/cases/derivations.cases`, `fixtures/r4/cases/predecessor.cases` |
| R-4 | `fixtures/r4/cases/derivations.cases` (`captured-*`, `anchor-*`) |
| R-5 | `fixtures/r4/cases/derivations.cases` (`root-*`) |
| X-F7 | `fixtures/r4/cases/paths.cases`, `fixtures/r4/cases/fold.cases` |
| X-F9 | `fixtures/r4/cases/paths.cases` (`p11a-*`, `p11b-*`) |
| R-12 (I-F2, I-F8) | `fixtures/r4/cases/predecessor.cases`, `fixtures/r4/cases/paths.cases` (`relpath-*`) |

## 4. Findings and gaps

Found while authoring; to be filed with the review. The cases follow the reading stated here until it is resolved.

| # | Where | Finding or gap | Cases |
|---|---|---|---|
| F-1 | [OS/path §7] step 5 | The tree prefix is compared as bytes after lexical normalisation, so on Windows (a case-insensitive volume) an argument that spells the tree root with another case (`C:/Work/repo/x.rs` for the tree `C:/work/repo`) is outside the tree. Proposed: state it as intended (the user's spelling is kept) or compare the prefix through the canonical root of step 4's directory | `cli-07` asserts the text as written |
| G-1 | [OS/path §3] P11 (a) | "The query name's UTF-8 bytes as stored on the name line": for a name that needs back-quotes ([F12 §6.6] `` `Foo bar` ``), the cases hash the name without its back-quotes | `p11a-03` |
| G-2 | [F12 §2.4]–§2.5 IN-3 | The cases check complete names; IN-3's completion (`branch NAME` → `lane/NAME`) is not applied, which is how [F12 §2.4]'s informative list refuses `feature-x` | `p11b-07` |
| G-3 | [OS/path §5] step 2 | A relative Windows argument is made absolute by `GetFullPathNameW`, whose own rewriting (trailing dots and spaces) is not specified here; the lexical cases use absolute Windows arguments only | `lexical-*` |
| G-4 | [F08 §11.2] step 2 | Two candidates rarely share a view with arbitrary uids; `reg-greatest-uid` states its candidates' uids (foreign nodes, whose uids need not equal their derivation, [F08 §11.2]) to make the bytewise order visible | `reg-greatest-uid` |
| G-5 | [F08 §10.3.1] interim rule | No writer records a scope until [F20] Appendix A is complete; `captured-scope` exercises the function over a scope value as an imported anchor keeps it | `captured-scope` |
| G-6 | [F20] Appendix A.9 | The appendix asks R-FIX for one fixture per scanner construct in `fixtures/r4/`; they are not part of this first part of WP-21 and follow separately | — |
