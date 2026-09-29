# 21 — Scope scanners

| | |
|---|---|
| Title | The scope scanners of resolver version 1: the Rust, Markdown and TOML scanners as total algorithms over any anchor text (tokens and line classes, what an item is, names and qualifiers, line spans, nesting, malformed input, limits); name paths, the scope a capture records and how a scope resolves; the `symbol` and `heading` authoring forms; golden examples, one fixture per construct |
| Chapter | [F21], `docs/spec/format/21-scope-scanners.md` |
| Status | draft, pass 1 pending (written after pass 1 closed; its first review is pass 2, open point 1) |
| Work package | WP-14b (R-SPEC-R), the scanner appendix that owner question OQ-R-2 requires before the freeze and before WP-63 is accepted ([PLAN §3.2] items 1 and 6; `reviews/owner-questions.md`, decided 2026-09-28) |
| Sources | [40 §2.7] (the `scope` selector, the authoring forms `path::A/B` and `path#H`, capture steps 3 and 4, "numbering such as `3.2` or `§` is stripped into its own field"); [40 §2.7.1] (the three scanners: a Rust tokenizer that tracks comments, strings, raw strings, char literals versus lifetimes and brace nesting; a fence-aware ATX/setext Markdown line scanner; a TOML table and key line scanner; validated against tree-sitter as a test-only oracle); [40 §4.5] (scope narrowing, same-kind headers); [40 §8.3.4] row 8 (scanner agreement); [F20] Appendix A as it stood after spec sync 2a (the Rust part, moved here); the WP-74 oracle's rules 1–8 (`moirai-tsoracle` crate documentation, "Items", output format 2); owner question OQ-R-2; informative: [11 §2.5, §2.6, §4.1] (numbered headings, name-path uniqueness, the scope shapes), CommonMark 0.31.2 (ATX and setext headings, fenced code, HTML blocks, thematic breaks, list items), TOML 1.0.0 (keys, tables, arrays of tables, strings, arrays, inline tables) |
| Depends on | [F01] (notation, `uvar32`, `vstr`, order); [F08 §10.3.1] (the scope value and its `lang` and `skind` codes); [F14 §5.6] (the scope text); [F19 §10.2] (the `anchor_spec` refusal texts); [F20] (§1.2 notation, §1.3 resolver version, §2.5 `atext`, `lines`, `nl`, N and `start`/`end`, §2.8 header text, §6 the anchor constants and the interim scanner rule) |

## 1. Scope and conventions

### 1.1 What this chapter fixes

[40 §2.7.1] gives three hand-written scope scanners and [40 §2.7] the anchor fields they decide. Their output enters
hashed bytes: the `scope` value ([F08 §10.3.1]) enters `captured` ([F08 §11.4]) and the canonical selector block
([F07 §8.2] field 4), and for the `symbol` and `heading` forms the item's header line decides the quote, the hint and the
header span hash ([F20 §2.8, §6.1]). Engine, model and oracle must therefore agree on every byte a scanner produces, on
every input. This chapter is the specification they share. It fixes:

- the input of a scanner and which files each scanner reads (§1.3);
- the item model common to all three languages: item fields, name paths, the segment equality that decides whether a
  name path names one item, recordable name paths, the scope a capture records, how a scope resolves, and item headers
  of the same kind (§2);
- the Rust scanner: tokens, the canonical spelling, bracket groups and their classes, item start positions, the items,
  their extents, `impl` names and qualifiers, lines, parents and order, and conformance with the WP-74 oracle (§3);
- the Markdown scanner: line classes, ATX and setext headings, fences, comment blocks, front matter, container lines,
  heading names and numbering, levels and sections (§4);
- the TOML scanner: key paths, table headers, key/value lines and the value scan, parents and spans (§5);
- the `symbol` and `heading` authoring forms: how a spec is split, segment syntax, matching, and the outcomes (§6);
- the constants (§7) and the golden examples, one fixture per construct (§8).

Each scanner is a **total** function: it is defined for every byte string, it never reads past the text, and it never
depends on a buffer size, a configuration key or the OS. Its only failure is a stated limit (§2.7). The scanners are part
of resolver version 1 ([F20 §1.3]): version 1 is what this chapter says at the `format-v1` tag, and any change after it
is a new resolver version. No value here is a configuration key ([AR §13] "Never a key").

This chapter was [F20] Appendix A. Spec sync 2a moved the Rust part here unchanged in substance (A.1–A.9, with the
corrections of open points 3–6) and added the total Rust algorithm, the Markdown part and the TOML part. [F20] Appendix A
now maps its old sections to this chapter's.

### 1.2 Notation

- [F20 §1.2] applies: bytes `b[i]`, `b[i..j)`, `‖`, `WS`, `eqi`, `chars`, `cutp`, `cuts`.
- **SP** is the byte `20`, **HT** the byte `09`. A **line** is an element of `lines(t)` ([F20 §2.5]): the bytes between
  two `0A`, without them. Line numbers start at 1. `line(o)` of a byte offset o of t is 1 plus the number of `0A` bytes
  in `t[0..o)`.
- In examples, ⏎ is a line break of the source, ␊ the byte `0A` inside a result, and HT a horizontal tab.
- **U+FFFD replacement** of a byte string x: every maximal subpart of an ill-formed UTF-8 sequence in x is replaced by
  U+FFFD (`EF BF BD`), as Unicode §3.9 and Rust's `String::from_utf8_lossy` do; well-formed sequences are kept. It is
  written `lossy(x)`.
- An item is written `kind name [qual] start–end`, followed by `in` and its parent when it has one. `kind` is the
  `skind` name of [F14 §5.6] (`fn`, `h2`, `key`, …); `[qual]` is shown only when the qualifier is not empty.

### 1.3 Input, languages and results

- **Input.** A scanner reads the anchor text t = `atext(b)` of a text content b ([F20 §2.5]): `norm(b)` with one leading
  `EF BB BF` removed. t holds no `0D` and no `00` ([F20 §2.1, §2.2]) but may hold any other byte, invalid UTF-8
  included. Line numbers are those of `lines(t)`, so they are the lines of the hint, of [F20 §2.8] and of the raw file.
  No scanner runs on binary content.
- **Language.** The language of a file is decided by the last component of its path, compared with `eqi`: it ends in
  `.rs` → **Rust** (`lang` 1); in `.md` or `.markdown` → **Markdown** (`lang` 2); in `.toml` → **TOML** (`lang` 3).
  Every other file has no scanner: it has no items, and no anchor on it records a scope ([40 §2.7.1] "everything else").
  At capture the path is the captured path; at resolve it is the file's current path.
- **Result.** `scan(t)` is either a list of items (§2.1), possibly empty, or **failed** (§2.7). The scanner of a
  language is the function this chapter defines for it; nothing else about a file (its size, its git state, the time)
  changes the result.

### 1.4 Status and the interim rule

[F20 §6.1]'s interim scanner rule holds until this chapter has passed review (owner question OQ-R-2, decided
2026-09-28: the appendix is required before the freeze and before WP-63 is accepted; if it misses the freeze, format v1
keeps the interim rule). While it holds, no capture records a scope and the `symbol` and `heading` forms are refused, so
no hashed byte depends on this chapter. Whether this issue lifts the rule is not decided here: it is lifted when review
accepts this chapter (open point 1). Meanwhile the chapter is normative for WP-63's scanners, for the oracle
differential ([40 §8.3.4] row 8, WP-74, WP-76), for R-FIX's scanner fixtures (§8), and for the model.

## 2. Items, name paths and scopes

### 2.1 Items

An **item** of a scanned text has these fields:

| Field | Meaning |
|---|---|
| `lang` | the language of §1.3: 1 `rust`, 2 `markdown`, 3 `toml` ([F08 §10.3.1]) |
| `skind` | Rust: 1 `mod`, 2 `impl`, 3 `fn`, 4 `struct`, 5 `enum`, 6 `trait`, 7 `const`, 8 `static`, 9 `macro_rules`; Markdown: the heading level 1–6; TOML: 1 `table`, 2 `array_table`, 3 `key` ([F08 §10.3.1]) |
| `name` | a non-empty byte string of valid UTF-8 with no `00`, `0D` or `0A` (Rust §3.2, §3.7; Markdown §4.7; TOML §5.2) |
| `qual` | a byte string of valid UTF-8 with no `00`, `0D` or `0A`: a Rust trait impl's trait (§3.7), a Markdown heading's numbering (§4.7), else empty |
| `start` | the item's first line, its **header line**: the line whose header text [F20 §2.8] computes (`header(start, symbol)` for Rust and TOML, `header(start, heading)` for Markdown) |
| `end` | the item's last line, `end` ≥ `start` |
| `parent` | the item that encloses it, or none (Rust §3.8, Markdown §4.8, TOML §5.5) |

`name` and `qual` are text values of [F08 §5.3] with no U+0000, CR or LF, one line each, as [F08 §10.3.1] requires.
The **range** of an item is its lines [`start`, `end`]. The items of a text are listed in **pre-order**: by the offset of
their first byte, an enclosing item before the items it encloses. An item's **ancestors** are its parent, its parent's
parent, and so on; an item encloses its descendants' ranges, so the ranges of an item's ancestors contain its range.

### 2.2 Name paths and segment equality

- The **name path** of an item is the list of the (`skind`, `name`, `qual`) triples of its ancestors, outermost first,
  followed by its own: exactly the `segments` of [F08 §10.3.1]'s scope value, whose `lang` is the item's. Its scope text
  is [F14 §5.6]'s: `rust:mod a/impl S[Tr]/fn f`, `markdown:h1 Design[1]/h2 Storage[1.1]`, `toml:table package/key name`.
- **Segment equality.** Two segments of one language are **equal** iff their `skind` and `name` are equal byte for byte
  and, for Rust and TOML, their `qual` are equal byte for byte. For Markdown the `qual`, a heading's numbering, is not
  compared: renumbering a section changes no name path it resolves by ([11 §2.5]: 32 % of the owner's headings are
  numbered). Two name paths are equal iff they have the same language and length and their segments are pairwise equal.
- A name path P **names one item** of a text t iff exactly one item of `scan(t)` has a name path equal to P. It names no
  item of a text whose scan failed or whose language differs.
- A name path need not name one item: Rust `cfg` twins, two `impl S` blocks, two `const _`, two literals that spell
  alike (§3.2); Markdown headings with the same text under one parent; TOML arrays of tables (`[[bin]]`). Differentials
  compare the items of a file as multisets (§3.9).

### 2.3 Recordable name paths

A name path is **recordable** iff it has at most `SCOPE_MAX_SEGMENTS` = 64 segments ([F08 §10.3.1] `n`) and its scope
value — the encoding of [F08 §10.3.1]: `lang` (1 byte), `n` (1 byte), and per segment `skind` (1 byte), `name` and
`qual` as `vstr` — is at most `SCOPE_MAX_BYTES` = 4,096 bytes long. A scanner reports items at any depth and with names
of any length; only recordable name paths are ever stored. The bound keeps an anchor record small (its `scope` is
typically 40–60 bytes, [40 §2.7]) and lets an implementation compare long names by a marker: a name path with a name or
qualifier longer than 4,096 bytes is equal to no recordable name path.

*(Informative)* The scope `markdown:h1 Design[1]/h2 Storage[1.1]` is the 25 bytes
`02 02` `01 06 44 65 73 69 67 6E 01 31` `02 07 53 74 6F 72 61 67 65 03 31 2E 31`; the scope `toml:table package/key name`
is the 19 bytes `03 02` `01 07 70 61 63 6B 61 67 65 00` `03 04 6E 61 6D 65 00`.

### 2.4 The scope of a capture

[F20 §6.1] records a scope with every capture of a scanned file whose scope item is found here ([40 §2.7] authoring
table: "an enclosing scope is added when a scanner finds one"). Let the **span** be the lines [qs, qe] of the capture:
the quote span of [F20 §6.1] step 4 for a `quote`, `range`, `symbol` or `heading` anchor, and the hint [L, M] for a
`lines` anchor.

1. **Scope item.** For a `symbol` or `heading` form, the scope item is the item §6 finds. For every other form, let C be
   the items whose range contains [qs, qe] (C holds the ancestors of each of its items), and D the items of C that are
   ancestors of no other item of C (the innermost ones). The scope item is the deepest item that is an ancestor of, or
   equal to, every item of D. There is none when C is empty, or when the items of D have no common ancestor (two
   top-level items on one line).
2. **The scope recorded** is the name path of the first item x, on the walk from the scope item through its ancestors,
   whose name path is recordable (§2.3) and names one item of t (§2.2). When the walk finds none, or there is no scope
   item, or the scan failed, no scope is recorded: `has_scope` is clear and `captured` takes `lp("")` ([F08 §11.1]).

So a span inside the second of two `cfg` twins `fn g` of `mod a` records `rust:mod a`; a span on a line holding two
top-level items records none; a span inside a `### Example` heading that occurs twice under one `## Usage` records the
`## Usage` path; an item nested deeper than 64 items records its nearest ancestor with a recordable name path that names
one item. For a `symbol` or `heading` form the scope item itself must qualify (§6.5), so its own name path is recorded.

### 2.5 Resolving a scope

A scope S of language λ **resolves uniquely** in the current anchor text t′ iff the file's current language is λ and S
names one item y of t′ (§2.2). Its **byte range** in N′ = N(t′) is `[start(s) .. end(e))` of [F20 §2.5], with s and e
y's `start` and `end`. [F20 §6.2] step 3 (search region), [F20 §6.4] region R2 and [F20 §6.5] ("scope only") use this
range. A scope that does not resolve uniquely narrows nothing: the whole of N′ is searched ([40 §2.7.1]).

### 2.6 Item headers of the same kind

[F20 §6.4] restricts the fuzzy candidates of a `symbol` or `heading` anchor whose scope did not resolve uniquely to those
inside "the header text of an item header of the same kind" ([40 §4.5]). Here:

- the anchor's **item kind** is the `skind` of the last segment of its scope: the same keyword for Rust, the same `skind`
  for TOML, the same level for Markdown;
- the **header text** of an item y of t′ is `header(start(y), k)` of [F20 §2.8], with k = `heading` for Markdown and
  `symbol` for Rust and TOML, and its byte range in N′ is `[start(start(y)) .. start(start(y)) + len(header))`;
- a candidate [s, e) of [F20 §6.4] **lies inside** a header text when both s and e are within that byte range.

A `symbol` or `heading` anchor without a scope (an import, [F08 §10.3.1]) has no item kind: the restriction does not
apply to it, as under [F20 §6.1]'s interim rule. When `scan(t′)` failed, t′ has no item headers, so no candidate counts.

### 2.7 A failed scan

A scan **fails** when the Rust scanner would hold more than `RUST_MAX_DEPTH` = 1,024 open groups (§3.3) or the TOML
scanner more than `TOML_MAX_DEPTH` = 1,024 open brackets of one value (§5.4). The Markdown scanner never fails. A failed
scan has no items, which decides every consequence:

- capture records no scope (§2.4); a `symbol` or `heading` form on the file is refused (§6.5);
- no scope resolves in the text (§2.5), and the same-kind restriction admits no candidate (§2.6);
- the oracle differential counts the file apart, with its own count (§3.9).

The limits bound memory and are far above the nesting of hand-written code; a file near them is generated or
adversarial. They are resolver constants, not keys.

### 2.8 Memory and streaming (informative)

Each scanner reads t once, from its start, and keeps a bounded state: Rust a stack of at most 1,024 groups and one open
item per group level; Markdown a few flags and the lines of the current paragraph; TOML a stack of at most 1,024
brackets. None needs the whole item list at once: a capture can find the scope item and its ancestors in one pass and
count the items that share each of their at most 64 name paths in a second pass, and a name longer than
`SCOPE_MAX_BYTES` can be kept as a marker (§2.3). The definitions below are written with trees and lists for clarity; an
implementation must not recurse once per nesting level (GT5 fuzzes the scanners, [PLAN §3.2] WP-67).

## 3. Rust

### 3.1 Tokens

A text x is read from its start as a sequence of whitespace, comments and tokens. The rules are byte-level, so they are
total over invalid UTF-8; for valid UTF-8 they are the character-level rules of the WP-74 oracle (rules 1–8) and of
[F20] Appendix A.2 before this chapter.

1. **Whitespace** is Rust's `Pattern_White_Space`: the bytes `09`–`0D` and `20`, and the byte sequences `C2 85`
   (U+0085), `E2 80 8E`, `E2 80 8F`, `E2 80 A8` and `E2 80 A9` (U+200E, U+200F, U+2028, U+2029), each matched where it
   starts.
2. **Comments** are `//` up to, not including, the next `0A` (or the end), and `/*` … `*/`, nested, up to the `*/` that
   closes the first `/*` (or the end). Doc comments (`///`, `//!`, `/**`, `/*!`) are comments. Whitespace and comments
   separate tokens and are not tokens.
3. **Word bytes** are `30`–`39`, `41`–`5A`, `61`–`7A`, `5F`, and every byte ≥ `80` at which no whitespace sequence of
   rule 1 starts. A **character** at offset i is the well-formed UTF-8 sequence that starts at i (Unicode Table 3-7), or
   the single byte at i when none does.
4. A **token** is, at the current position:
   - a **literal**, tried first:
     - at `"`: a string, in which `\` takes the next byte with it, closed by the next `"` that no `\` takes;
     - at a maximal run of word bytes that is exactly `b` or `c` and is directly followed by `"`: a byte or C string,
       the same;
     - at a run that is exactly `b` directly followed by `'`: a byte character, closed by the next `'` that no `\` takes;
     - at a run that is exactly `r`, `br` or `cr`, directly followed by n ≥ 0 `#` bytes and then `"`: a raw string,
       closed by the first `"` followed by n `#` bytes (no escapes);
     - at `'` directly followed by `\`: a character literal, closed by the next `'` that no `\` takes;
     - at `'` directly followed by a character c other than `'` and `\`, which is directly followed by `'`: the
       character literal `'c'`.

     Each literal takes a directly following maximal run of word bytes as its suffix (`"x"suffix`, `1u8` is a word). An
     unterminated literal runs to the end of the text;
   - a **word**: a maximal run of word bytes (identifiers, keywords, and numbers such as `1u8` or `0x1F`); the run `r`
     directly followed by `#` and a word byte is one word with the `#` and the following run, a **raw identifier**
     (`r#type`);
   - a **lifetime**: at a `'` that starts no literal and is directly followed by a word byte, the `'` and the maximal run
     after it (`'a`, `'static`, `'_`);
   - otherwise **punctuation**: the one byte at the position (`<`, `:`, `&`, `#`, `{`, a lone `'`, a control byte, …).
5. **Shebang.** When the text starts with `#!` and, after whitespace and comments, the next byte is not `[`, the bytes
   up to the first `0A` (or the end) are a comment. `#![attr]` is an inner attribute, not a shebang.

**Keywords.** K is the set of these 51 words: the strict keywords `as`, `async`, `await`, `break`, `const`, `continue`,
`crate`, `dyn`, `else`, `enum`, `extern`, `false`, `fn`, `for`, `if`, `impl`, `in`, `let`, `loop`, `match`, `mod`,
`move`, `mut`, `pub`, `ref`, `return`, `self`, `Self`, `static`, `struct`, `super`, `trait`, `true`, `type`, `unsafe`,
`use`, `where`, `while`, and the reserved words `abstract`, `become`, `box`, `do`, `final`, `macro`, `override`, `priv`,
`try`, `typeof`, `unsized`, `virtual`, `yield`. The weak keywords (`macro_rules`, `raw`, `safe`, `union`), the words
`default` and `auto`, and the edition-2024 reservation `gen` are not in K: they name macros, functions and variables in
real code (`fn default() -> Self`). A **name** is a word token that is not in K and whose first byte is not an ASCII
digit (`r#match`, `Größe`, `default` and `_` are names).

### 3.2 The canonical spelling

`canon(x)` of a text x writes the tokens of x (§3.1, without rule 5) in order, each with its bytes changed as follows,
and nothing else:

1. U+FFFD replacement (`lossy`, §1.2) of the token's bytes;
2. each CR LF pair and each LF is written as the two bytes `\n` (`5C 6E`), each CR not followed by LF as `\r` (`5C 72`),
   and each NUL as `\0` (`5C 30`).

One SP is written between two adjacent tokens exactly when:
- both are words, literals or lifetimes;
- the first is the punctuation `/` and the second is `/` or `*`, so that no comment opener forms;
- the first is the word `r`, `br` or `cr` and the second is the punctuation `#`, so that no raw string or raw identifier
  forms.

Properties:
- `canon(x)` holds no `00`, `0A` or `0D`, so it is one line and a text value of [F08 §5.3] with no U+0000, CR or LF, and
  it does not depend on the line endings of x. In t (§1.3) a token can hold only one of these bytes, an LF inside a
  literal, so only `\n` is written there; the other escapes make the function total for the oracle, which reads raw
  files.
- Replacing per token equals replacing over the whole of x first: a maximal ill-formed subpart consists of bytes ≥ `80`
  and never contains the lead byte of a whitespace sequence, so it never spans a token boundary.
- Outside literals the result holds no comment and no two adjacent spaces. For a text of valid Rust tokens it splits into
  the same tokens as x, so `canon(canon(x)) = canon(x)`. An escape reads like the escape it spells (`"a⏎b"` and
  `"a\nb"` have one spelling), so two items can share a name (§2.2). A name (§3.1) is its own spelling unless it holds
  invalid UTF-8.

*(Informative)* Vectors (␍ is a lone CR, ␀ a NUL byte):

| x | `canon(x)` |
|---|---|
| `Foo < T , U >` | `Foo<T,U>` |
| `Vec<⏎    Vec<u8>,⏎>` | `Vec<Vec<u8>,>` |
| `[u8; 4]` | `[u8;4]` |
| `crate :: a :: Foo` | `crate::a::Foo` |
| `<T as Iterator>::Item` | `<T as Iterator>::Item` |
| `&'a   mut` HT `Foo` | `&'a mut Foo` |
| `& 'static str` | `&'static str` |
| `dyn Fn(u8) -> u8 + Send + 'a` | `dyn Fn(u8)->u8+Send+'a` |
| `unsafe  extern "C" fn ( )` | `unsafe extern "C" fn()` |
| `extern"C" fn()` | `extern "C" fn()` |
| `Foo /* a /* nested */ b */ < T >` | `Foo<T>` |
| `mut/**/Foo` | `mut Foo` |
| `Foo<{ A / *B }>` | `Foo<{A/ *B}>` |
| `A / /B` | `A/ /B` |
| `Foo<{ "a  b" }>` | `Foo<{"a  b"}>` |
| `Foo<{ br#"a " b"# }>` | `Foo<{br#"a " b"#}>` |
| `Foo<{ "x"suffix }>` | `Foo<{"x"suffix}>` |
| `Foo<{ 1.0e-3 }>` | `Foo<{1.0e-3}>` |
| `mut r#type`; `r # type` | `mut r#type`; `r #type` |
| `Foo<{ "a⏎b".len() }>`, LF or CR LF | `Foo<{"a\nb".len()}>` |
| `Foo<{ "a␍b" }>`; `Foo<{ "a␀b" }>` | `Foo<{"a\rb"}>`; `Foo<{"a\0b"}>` |
| `dyn` U+2028 `Trait` | `dyn Trait` |
| `S<` `FF` `>`; `S<` `E2 80 FF` `>` | `S<` U+FFFD `>`; `S<` U+FFFD U+FFFD `>` |

### 3.3 Groups and their classes

**Groups.** The tokens of t are read in order with a stack of open groups (the whole text, the **root**, is not on it):

1. An opener token `(`, `[` or `{` opens a **group**, which is pushed. If the stack then holds more than
   `RUST_MAX_DEPTH` = 1,024 groups, the scan fails (§2.7).
2. A closer token `)`, `]` or `}` whose opener kind is on the stack closes the topmost group of that kind: the token is
   that group's **closer**, and every group above it on the stack is **closed implicitly** and has no closer. A closer
   whose opener kind is not on the stack is a **stray closer**: an ordinary token.
3. At the end of the text every group still open is closed implicitly.

So the text is a tree. A **sequence** is the root's contents or a group's contents: a list of **elements**, each a token
or a group. A group is one element of the sequence its opener stands in; its opener and closer are not elements of its
own contents. The **last token** of an element is the token itself, or for a group its closer, or, for a group closed
implicitly, the last token of its last element (its opener when it has none).

**Classes.** The root has class `code`. A group has class:
- `tree` when it lies inside a `tree` group, or when, in its sequence,
  - its opener is `[` and the elements before it are `#` or `#` `!`: an **attribute** (`#[…]`, `#![…]`);
  - the element before it is `!` and the one before that is a word token not in K: a **macro invocation** (`foo!(…)`,
    `a::b! { … }`, `r#try!(…)`); the words of K exclude `if !(x)`, `while !(x)`, `match !(x)`, `for ! {` and the like;
  - the elements before it are the word `macro_rules`, `!` and a name: a `macro_rules!` body;
- otherwise `code` when its opener is `{`, and `plain` when it is `(` or `[`.

A `tree` group is a token tree: nothing inside it is an item ([40 §2.7.1]; oracle rule 2). A `plain` group holds no item
start position, but the `code` groups inside it are scanned (a closure body in an argument list, a block in an array
length).

### 3.4 Item start positions

Items are recognised only in `code` sequences. An element e of a code sequence S is at an **item start position** iff:

1. e is a token;
2. no item recognised in S is open at e (§3.6); and
3. the element before e in S, after every attribute before e is skipped (its `#`, its optional `!` and its `[` group),
   is absent, a `;` token, a `}` token (a stray closer), or a `{` group of any class.

So the first token of a block, and the token after a `;` or after a block, start items; an attribute or a doc comment
before an item does not move its start position. A stray `}` is counted as the end of a block so that one extra brace
does not hide the items after it.

### 3.5 Recognising an item

At an item start position e, the elements u0 = e, u1, u2, … of S are matched against this pattern; when it does not
match, e starts no item and the scan goes on with e as an ordinary token.

1. **Visibility.** If u(i) is the word `pub`, i advances; if u(i) is then a `(` group, i advances past it (`pub(crate)`,
   `pub(in path)`).
2. **Qualifiers**, repeated:
   - the words `async`, `unsafe`, `safe` and `default`: i advances;
   - the word `extern`: if u(i+1) is the word `crate`, no item (`extern crate` is a declaration); else i advances, and
     advances once more if u(i) is a string or raw string literal (the ABI, `extern "C"`);
   - the word `const` when u(i+1) is one of the words `fn`, `async`, `unsafe`, `extern` (keywords, so `const safe:
     bool` and `const default: u8` stay `const` items): i advances.
3. **Keyword** u(i), then:

| u(i) | the rest of the pattern | `skind` | `name` |
|---|---|---|---|
| `fn`, `mod`, `struct`, `enum`, `trait` | u(i+1) is a name | 3, 1, 4, 5, 6 | `canon(u(i+1))` |
| `const` | u(i+1) is a name (`_` is one) | 7 | `canon(u(i+1))` |
| `static` | u(i+1) is a name, or u(i+1) is the word `mut` and u(i+2) is a name | 8 | `canon` of that name |
| `impl` | — | 2 | §3.7 |
| `macro_rules` | u(i+1) is `!`, u(i+2) is a name, u(i+3) is a group | 9 | `canon(u(i+2))` |

   Any other u(i), or a keyword whose pattern does not match, is no item. The item's **first token** is u0: its
   visibility, a qualifier or its keyword ([F20] Appendix A.5 before this chapter).

Consequences on valid Rust: `const fn`, `async fn`, `unsafe fn`, `extern "C" fn` and trait method declarations are `fn`
([F08 §10.3.1]); `const _` is a `const` named `_`; `static mut` is a `static`; `unsafe impl` and `unsafe trait` are
items; `union`, `type`, `use`, `extern crate`, `extern` blocks, `const` blocks, `unsafe`/`async` blocks, enum variants,
fields, closures, `let` statements and macro invocations are not items (they never match). Outside the contract:
`default fn` is a `fn`; `auto trait A` is no item (`auto` is no qualifier, and `trait` then stands after a word); lazy
`static ref X` is no item (`ref` is in K); `static ||` is no item.

### 3.6 The extent of an item

An item x recognised at u0 in S ends at its **final element** f, a later element of S, found as follows; x is **open**
from u0 until f has been scanned, with the groups among its elements (from u0 to f) scanned as sequences of their own in
the meantime.

- **`const` and `static`**: f is the first `;` token of S after the name.
- **`macro_rules`**: if u(i+3) is a `{` group, f is that group; else f is the `;` token directly after the group when
  there is one, or the group.
- **`mod`, `fn`, `struct`, `enum`, `trait`, `impl`**: the **header scan** reads the elements of S after the name (after
  `impl` for an `impl`) with an angle count a = 0 and the previous element p (at first the name, or `impl`):
  1. a `;` token: f is that token;
  2. a `{` group of class `code`: if a > 0 and p is one of the tokens `<`, `,` or `=`, it is a const-generic block
     (`Foo<{ N }>`, `N: usize = { 3 }`) and the scan goes on; else f is that group, the **body** (and a is dropped);
  3. a `<` token: a increases by 1;
  4. a `>` token, when a > 0 and it is not the second byte of an arrow (p is the token `-` or `=` and ends at the byte
     just before it, `->` or `=>`): a decreases by 1;
  5. any other element, a `tree` group included (`fn f() -> ty!{…} {` takes the second group as its body): the scan goes
     on.

  Each element read becomes p for the next.

When S ends before f is found (the text ends, or S is a group's contents and the group closes), x ends with the last
element of S. The **last token** of x is the last token (§3.3) of f, or of the last element of S; x's `end` is the line
of its last byte. While x is open, no element of S is at an item start position (§3.4 rule 2); items recognised inside
the groups among x's elements are x's descendants.

The angle count needs no match of `<` with `>`: it only decides whether a `{` belongs to a generic argument. A `<` that is
a comparison cannot stand at S's level of a header in valid Rust (const-generic expressions are braced), and in malformed
input the rule of step 2 ("else f is that group") ends a header whose `<` never closes at its first block (`struct S<T {`
still has a body).

### 3.7 `impl` names and qualifiers

For an `impl` x, let h be the index in S of its final element f, or the length of S when it has none. Over the
elements u(i+1) … u(h−1) after the keyword:

1. **Generic parameters.** If u(i+1) is a `<` token, the parameters run from it to the `>` token at which the angle
   count of §3.6 steps 3 and 4 (arrows excluded) first returns to 0; the **header** starts after that `>`, and is empty
   when the count does not return to 0 before u(h). Otherwise the header starts at u(i+1).
2. **Separator and `where`.** The header is read with a fresh angle count b = 0. The first `where` word at b = 0 ends
   the header's type part. Before it, the first `for` word at b = 0 is the **separator**, unless it is a higher-ranked
   binder: `for` directly followed by a `<` token and then a lifetime or `>` (`for<'a>`, `for<>`); the qualified path
   `for <T as Iterator>::Item` is a separator.
3. **Name and qualifier.** With a separator, the **trait** is the elements from the header's start to the separator,
   and the **self type** the elements after it up to `where` or h; without one, the trait is empty and the self type is
   the elements from the header's start up to `where` or h. `name` = `canon` of the text from the first byte of the self
   type's first token to the last byte of its last token (its path and generic arguments kept, comments dropped);
   `qual` = `canon` of the trait's text, empty when the trait is empty. A negative impl's `!` is the trait's first
   token, so `qual` starts with it (`impl ! /* c */ Sync for R` has the qualifier `!Sync`).
4. An `impl` whose `name` is empty (`impl<T> { … }`, `impl Tr for {}`) is **not reported**: it is no item, it is never a
   parent, and the items inside it have the next reported enclosing item as their parent (oracle rule 3).

The `impl`'s generic parameters, `where` clause, `unsafe` and `default` are not part of the name. **Decision** (kept from
[F20] Appendix A.4): `qual` keeps a negative impl's `!` and the trait's generic arguments, since `impl From<A> for E` and
`impl From<B> for E` are two impls whose methods would otherwise share one name path, and a negative impl is another item
than a positive one. The `symbol` form still names a trait by its bare name (§6.2).

### 3.8 Lines, parents and order

- **`start`** is `line` of the first byte of the item's first token (§3.5). Outer attributes and comments before the
  item, doc comments included, are not part of it. When the visibility or a qualifier stands on a line of its own
  (`pub(crate)⏎fn f()`), `start` is that line and the header of [F20 §2.8] continues past it (`pub(crate)␊fn f()`).
- **`end`** is `line` of the last byte of the item's last token (§3.6): its closing `}` or `;` in valid Rust (`mod m;`,
  `struct S;`, `macro_rules! m ( … );`). Several items may share a line.
- **Parent**: the innermost reported item that is open (§3.6) when the item is recognised. It is the nearest reported
  item whose source range, from its first token to its last byte, contains the item's. `extern` blocks, `const` blocks
  and every other construct that is not an item are transparent: `fn ext();` in an `extern "C" { … }` block inside
  `mod a` has the parent `mod a`. Items nest only by containment in the source: an `impl` is a sibling of the `struct`,
  `enum` or `trait` it implements, never its child.
- **Order**: pre-order (§2.1), which is the order in which the items are recognised.

### 3.9 The contract, the oracle and input outside the contract

- **Normative for every input.** §3.1–§3.8 define `scan(t)` for every byte string. Engine and model implement them
  independently ([PLAN §3.1] S2) and agree byte for byte, because the scope, the header line and the hint enter
  `captured`.
- **The contract** is stable Rust, with negative impls, which std and nightly crates use. On a source that is valid
  UTF-8 and valid Rust of the contract, the items are exactly those that the WP-74 oracle's rules 1–7 describe: the kinds
  of §3.5's table at any depth (modules, `impl` and `trait` bodies, `extern` blocks, function bodies and every block,
  closure body and initialiser inside them); nothing inside a macro invocation or `macro_rules!` body; attributes not
  evaluated, so `cfg` twins are two items; names and qualifiers as §3.5 and §3.7 spell them; lines, parents and order as
  §3.8 gives them. A disagreement between the oracle and this chapter on such a source is a specification finding for
  this chapter, never a reason to tune the scanner to the oracle; a change here changes the oracle and raises its
  format number (oracle rule 8, output format 2).
- **The oracle's claim.** `moirai-tsoracle` (WP-74; host-only, tree-sitter-rust) marks the items it vouches for: the
  source is valid UTF-8, and no syntax error of tree-sitter's tree lies inside the item, on the path from it to the root,
  or in the name or qualifier of an enclosing item. Syntax the pinned grammar does not parse (the crate's "Known grammar
  gaps") falls outside the claim. The oracle reads raw files: it skips one BOM, counts lines at `0A`, and `canon` spells a
  CR LF inside a literal as it spells an LF, so its lines and names are those this chapter gives for t.
- **The differential** ([40 §8.3.4] row 8; WP-74, WP-76) compares a file's claimed items with the scanner's as
  multisets, and reports name-path, header-line (`start`) and span (`start`–`end`) agreement separately, with the count
  of unclaimed items and of failed scans (§2.7) beside the agreement rate.
- **Outside the contract** the rules still decide, and §8.1 constructs 15–27 pin them: an unterminated literal or block
  comment runs to the end of the text and hides everything after it; an unbalanced closer closes implicitly or is
  stray (§3.3); an item without a name is no item, and an `impl` without a self type is not reported (its inner items go
  to the next enclosing item, as the oracle proposed); names are spelled with U+FFFD replacement; `auto trait`, trait
  aliases (`trait A = B;` is a `trait`), `macro` 2.0 definitions (no item; their body is a block), `impl const`
  (qualifier `const Tr`), `default fn` (a `fn`), `static ||` and lazy_static's `static ref` outside its macro (no item),
  and default field values follow the same rules.

## 4. Markdown

### 4.1 Lines, indentation and blank lines

The Markdown scanner reads the lines of t in order. For a line l:
- **Indentation** `ind(l)` is the column reached by its leading SP and HT bytes, a SP advancing one column and an HT to
  the next multiple of `MD_TAB_STOP` = 4; `rest(l)` is l after those bytes.
- l is **blank** iff it holds only SP and HT bytes (or none).
- **Shallow** means `ind(l)` ≤ `MD_MAX_INDENT` = 3. Every block construct below except paragraph continuation needs a
  shallow line, as in CommonMark; a line indented 4 or more columns is code or continuation text, never a heading.

### 4.2 The scan

The state is: `fence`, an open fence (its byte c and length n) or none; `comment`, a flag; `para`, the list of lines of
the open paragraph or none; `cont`, a flag for an open container run. At the start everything is none or clear. If
line 1 opens front matter (§4.5), the scan starts after its closing line. Then each line l, in order, is classified by
the first rule that applies:

1. `fence` is open: if l is shallow and a closing fence for it (§4.5), `fence` is cleared. The line is code.
2. `comment` is set: if l contains the bytes `-->`, `comment` is cleared. The line is comment.
3. l is blank: `para` becomes none and `cont` is cleared.
4. `para` is open, l is shallow and `rest(l)` is a **setext underline** (§4.4): the lines of `para` form a setext
   heading; `para` becomes none.
5. l is shallow and `rest(l)` is:
   1. an **ATX heading** (§4.3): a heading on l; `para` becomes none and `cont` is cleared;
   2. a **fence opener** (§4.5): `fence` is set; `para` becomes none and `cont` is cleared;
   3. an **HTML comment opener**, starting with `<!--`: `comment` is set unless l contains `-->`; `para` becomes none
      and `cont` is cleared;
   4. a **thematic break**: three or more of one of the bytes `-`, `*`, `_`, with any SP and HT between and after them
      and nothing else; `para` becomes none and `cont` is cleared;
   5. a **container marker** (§4.6): if `para` is open and the marker cannot interrupt a paragraph, l is appended to
      `para`; otherwise `para` becomes none and `cont` is set.
6. `para` is open: l is appended to `para` (paragraph continuation, at any indentation).
7. `cont` is set: l is a container line.
8. l is shallow: `para` becomes the list holding l (a paragraph starts).
9. Otherwise l is indented code.

A heading is the only item. Lines of code, comments, front matter and containers are never headings; nothing else
about them matters.

### 4.3 ATX headings

`rest(l)` is an ATX heading iff it starts with 1 to `MD_MAX_LEVEL` = 6 bytes `#`, directly followed by SP, HT or the end
of the line. Its **level** is the number of `#`. Its **content** is the bytes after the `#` run with leading and
trailing SP and HT removed, and then an optional closing sequence removed: when the content ends in a run of `#` that is
the whole content or is preceded by SP or HT, that run and the SP and HT before it are removed (`## Title ##` → `Title`,
`# foo#` → `foo#`, `# foo \#` → `foo \#`, `## ##` → empty). The heading's `start` is l.

### 4.4 Setext headings

`rest(l)` is a setext underline iff it is a run of one or more `=` or of one or more `-`, followed only by SP and HT;
`=` gives level 1 and `-` level 2. It makes a heading only in rule 4, when a paragraph is open, so `===` elsewhere is
text and `---` elsewhere is a thematic break; `***` and `- - -` are never underlines. The heading's lines are the
paragraph's; its `start` is the paragraph's first line (its header line, [F20 §2.8] `header(start, heading)` = that
line); its **content** is the paragraph's lines, each with leading and trailing SP and HT removed, joined by one SP. The
underline line belongs to the heading's section and to no header.

### 4.5 Fences, comments and front matter

- **Fence opener**: `rest(l)` starts with a run of n ≥ `MD_FENCE_MIN` = 3 bytes `` ` `` or n ≥ 3 bytes `~`; for a
  backtick run, the rest of the line holds no `` ` ``. A **closing fence** for it: a shallow line whose `rest` is a run
  of at least n of the same byte followed only by SP and HT. An unclosed fence runs to the end of the text. Fenced lines
  (`# comment` in a shell block, `## Build` in a Markdown example) are never headings.
- **HTML comment block** (CommonMark's HTML block type 2): from a shallow line whose `rest` starts with `<!--` to the
  first line, that one included, that contains `-->`. A commented-out heading is not a heading. Other HTML blocks are
  not recognised (open point 9).
- **Front matter**: when line 1 is `---` followed only by SP and HT, and a later line is `---` or `...` followed only by
  SP and HT, lines 1 up to the first such later line are front matter and are skipped. Without the closing line, line 1
  is an ordinary line (a thematic break).

### 4.6 Container lines

The scanner does not track block quotes and list items; it recognises only their first lines, so that a heading inside
one is not taken for a document heading and a list item does not make the paragraph above it a setext heading:

- a **container marker** is a shallow line whose `rest` starts with `>`; or with `-`, `+` or `*` followed by SP, HT or
  the end of the line; or with 1 to 9 ASCII digits and then `.` or `)`, followed by SP, HT or the end of the line;
- it **can interrupt a paragraph** iff it is a `>`; or a bullet or ordered marker whose line holds a byte other than SP
  and HT after the marker and, for an ordered marker, whose digits have the value 1 (CommonMark);
- a container marker, and every later non-blank line up to the next blank line that no rule of §4.2 before rule 7
  classifies, are **container lines**: never headings, never paragraph lines.

So `> # Quote`, `- # Item` and `1. # Step` hold no heading; `Para⏎- item⏎---` has no heading (the `---` is a thematic
break); `Para⏎2. two⏎---` is one setext heading `Para 2. two` (a list starting at 2 cannot interrupt a paragraph). A
heading indented up to 3 columns inside a list item, after a blank line, is read as a heading (open point 9).

### 4.7 Heading text: name and numbering

A heading's content c (§4.3, §4.4) becomes its fields:

1. **Collapse.** Every maximal run of SP and HT in c is replaced by one SP.
2. **Numbering.** A numbering prefix is matched at the start of the collapsed c, byte by byte:
   1. optionally `§` (`C2 A7`) followed by at most one SP;
   2. a first component: 1 to `NUM_MAX_DIGITS` = 9 ASCII digits, or one ASCII capital letter `A`–`Z` directly followed
      by `.` or `)`;
   3. further components, each a `.` followed by 1 to 9 ASCII digits, while they occur;
   4. optionally one `.` or `)`;
   5. then a SP is required. A run of more than 9 digits, or no SP, means no numbering.

   The **numbering** is the matched text before the SP. After that SP, a dash separator is skipped too: `-`, `–`
   (U+2013) or `—` (U+2014) followed by a SP. If nothing is left after them, there is no numbering.
3. **Fields.** With a numbering, `qual` = the numbering and `name` = the rest; without one, `qual` is empty and `name` =
   the collapsed c. Both then undergo U+FFFD replacement (`lossy`). The **text** of the heading is the collapsed c,
   replaced likewise; the `heading` form matches it (§6.4).
4. A heading whose `name` is empty (`#` alone, `## ##`) is no item: it neither nests nor ends a section.

Examples (§8.2 construct M10): `3.2 Recovery` → name `Recovery`, qual `3.2`; `§3 Storage` → `Storage`, `§3`;
`A.1 Parts` → `Parts`, `A.1`; `1. Scope` → `Scope`, `1.`; `20 — R4 constants` → `R4 constants`, `20`; `2026-09-28
update`, `3D graphics`, `1.2.3`, `A Tale` and `A.I. and ML` have no numbering. Letters count only as the first
component, so appendix numbering (`A.1`, `B.`) is stripped and words are not.

### 4.8 Levels, parents and sections

The headings of t, in line order, are the Markdown items; `skind` is the level.
- **Parent**: the nearest earlier heading with a lower level (levels may be skipped: an `h3` under an `h1` has the `h1`
  as its parent).
- **Section**: `end` is the line before the `start` of the next heading whose level is at most the heading's own, or the
  last line of t when there is none. A section contains the sections of its descendants, and blank lines, code and text
  before the next heading belong to it.

### 4.9 Differences from CommonMark (informative)

The scanner is a line scanner of 0.17 ms per file ([40 §2.7.1]), not a Markdown parser. On the headings of ordinary
documents it agrees with CommonMark 0.31.2. It differs where CommonMark needs container structure: headings inside block
quotes and list items are not items unless they stand after a blank line with at most 3 columns of indentation (then they
are read as document headings); a fence opened inside a list item ends only at its closing fence, not with the item;
HTML blocks other than comments are not recognised; an empty ATX heading is no item; setext content is joined with SP
where CommonMark keeps a soft line break; heading text is the source text, not rendered inline content (`**x**` stays
`**x**`). Each difference yields a deterministic result that the fixtures of §8.2 pin.

## 5. TOML

### 5.1 Lines and the scan state

The TOML scanner reads the lines of t in order with this state: the current **table** (the last table or array-of-tables
item, or none), and an open **value**, the key item whose value continues on the next line, with its bracket stack and
open multi-line string. TOML whitespace is SP and HT. A line read while a value is open continues that value (§5.4) and
is nothing else. Every other line, after its leading SP and HT, is:

1. empty, or starting with `#`: nothing;
2. starting with `[[`: an array-of-tables header if §5.3 matches, else nothing;
3. starting with `[`: a table header if §5.3 matches, else nothing;
4. otherwise: a key/value line if a key path (§5.2) is followed, after any SP and HT, by `=`; else nothing.

A line that is nothing (a malformed header, a line without `=`) changes no state: keys after it belong to the table
before it.

### 5.2 Keys and key paths

- A **key** is a **bare key**, a maximal run of the bytes `A`–`Z`, `a`–`z`, `0`–`9`, `-` and `_`; a **basic quoted
  key**, `"` … `"` on one line, in which `\` takes the next byte with it; or a **literal quoted key**, `'` … `'` on one
  line. A quoted key that does not close on its line is no key.
- A **key path** is a key followed by any number of (SP/HT, `.`, SP/HT, key).
- **Spelling.** The name of a key path is its keys as written — bare keys and quoted keys with their quotes and their
  exact inner bytes — joined by `.`, with the SP and HT around the dots removed, and then U+FFFD replacement:
  `[ a . "b c" ]` names `a."b c"`. Equal TOML keys written differently (`a` and `"a"`) have different names (open point
  10). Names hold no `0A` (keys are on one line) and no `00` or `0D` (t has none).

### 5.3 Table headers

After leading SP and HT, a **table header** is `[`, any SP and HT, a key path, any SP and HT, `]`; an
**array-of-tables header** is the same between `[[` and `]]`. After the closing bracket only SP and HT may follow, then
the end of the line or a comment starting with `#`. The line is an item — `skind` 1 `table` or 2 `array_table`, `name`
the key path's spelling, `qual` empty, no parent, `start` its line — and becomes the current table. `[ [a] ]` is no
header; `[a]]` and `[b] junk` are nothing.

### 5.4 Key/value lines and the value scan

A key/value line is an item — `skind` 3 `key`, `name` the key path's spelling (a dotted key `a.b = 1` names `a.b`),
`qual` empty, `start` its line, parent the current table — whose `end` is the line where its **value** ends. The value
scan reads from the byte after `=`, skipping SP and HT, and takes the first value token:

- a `#` or the end of the line: the value is empty and ends on this line;
- `"""` or `'''`: a **multi-line string**, which ends after the first maximal run of 3 or more `"` (respectively `'`)
  after the opener; in a `"""` string a `\` takes the next byte with it, the line's `0A` included, and so no run starts
  at a byte it takes. A run of 4 or 5 closes too (`"""a""""` is one string);
- `"` or `'`: a single-line string; the value ends on this line;
- `[` or `{`: a bracket, pushed on the value's stack; the scan goes on (below);
- anything else: a scalar; the value ends on this line.

Inside brackets, the scan reads byte by byte, across lines: SP and HT are skipped; `#` ends the reading of the line (a
comment); `"""` and `'''` open a multi-line string as above; `"` opens a string that ends at the next `"` that no `\`
takes, or at the end of the line; `'` opens one that ends at the next `'`, or at the end of the line; `[` and `{` are
pushed, and if the stack then holds more than `TOML_MAX_DEPTH` = 1,024 brackets the scan fails (§2.7); `]` and `}`
close the topmost bracket of their kind, with every bracket above it, or are ignored when their kind is not on the
stack; every other byte is skipped. The value ends on the line where the stack becomes empty, or where a multi-line
string at the top level closes; the rest of that line is ignored. A value still open at the end of t ends on the last
line.

So a multi-line array or string makes its key span several lines, and no line inside it is a header or a key
(`[not.a.table]` inside `"""` is text); an unclosed `[` makes the rest of the file part of one value.

### 5.5 Items, parents and spans

The items are the table, array-of-tables and key items in line order. A key's parent is the table current at its line,
or none for a key before the first header. A table's `end` is the largest `end` of its keys, or its own line when it has
none: comment and blank lines after its last key belong to no item. Tables do not nest: `[a.b]` is a top-level item named
`a.b`, not a child of `[a]` ([40 §2.7] "TOML `table.path / key`"), so every TOML name path has one or two segments. Keys
inside inline tables and arrays are not items. Each `[[bin]]` is its own item with the same name path, so the keys of an
array of tables never name one item (§2.2).

### 5.6 Differences from TOML 1.0 (informative)

The scanner does not validate TOML. It accepts a value after a key without checking it, multi-line inline tables
(TOML 1.1), and duplicate keys and tables; it reads keys only at the start of a line; non-ASCII bare keys (TOML 1.1) are
no keys, so their lines are nothing. 96.4 % of the owner's TOML citations never moved ([40 §2.7.1]): the scanner exists
to name `[table]` and `key`, which it does exactly on valid TOML.

## 6. Authoring forms

### 6.1 Splitting a spec, segments and escapes

- **`symbol` form** `path::S1/…/Sk` ([40 §2.7]): the spec splits at the first `::` whose prefix ends in `.rs` or `.toml`
  (`eqi`); the prefix is the path, resolved as [40 §2.7] capture step 1 says, and the rest is the **selector**. A spec
  with no such `::` is not this form.
- **`heading` form** `path#H1/…/Hk`: the spec splits at the first `#` whose prefix ends in `.md` or `.markdown` (`eqi`),
  so `docs/C#/intro.md#Setup` has the path `docs/C#/intro.md`.
- The selector splits at every `/` into k ≥ 1 **segments**; an empty segment is a syntax error ([F19 §10.2]
  `anchor_spec`). A `symbol` segment is `X`, or `X[Y]` when it ends in `]` (its first `[` then starts Y); X and Y are
  non-empty and hold none of `/`, `[` and `]`. A `heading` segment is any non-empty text, brackets included
  (`## [Unreleased]`).
- **Escapes.** In X, Y and heading segments, after the split, `%2F`, `%5B`, `%5D` and `%25` (hexadecimal digits in either
  case) stand for `/`, `[`, `]` and `%`, as in the scope text ([F14 §5.6]); every other `%` is itself. So
  `docs/io.md#Input%2Foutput` names the heading `Input/output`.
- A segment longer than `SCOPE_MAX_BYTES` bytes after the escapes is refused (such a name is never recordable, §2.3).

### 6.2 The `symbol` form on a Rust file

The form names one Rust item by the last k segments of its name path, typed with bare names:

- **Bare name.** A `name` or `qual` v has a bare name when it has the form `[!] w1::…::wm[<…]`: an optional `!`, then
  m ≥ 1 words (§3.1; a raw identifier is one word) joined by `::`, then either nothing or a `<` and anything after it.
  Its bare name is the `!`, if present, followed by wm: `Wrap<T>` → `Wrap`, `crate::a::Tr<u8>` → `Tr`, `!Send` →
  `!Send`, `std::fmt::Display` → `Display`. `&'a mut[u8]`, `(A,B)`, `dyn Fn(u8)->u8+Send` and `<T as Iterator>::Item`
  have none.
- **Segment match.** X matches a `name` n iff X = n or X is n's bare name; Y matches a `qual` q the same way. `X[Y]`
  matches an `impl` whose `qual` is not empty, X matching its `name` and Y its `qual`. `X` matches an item whose `qual`
  is empty and whose `name` X matches; as the last segment (i = k) it matches no `impl`. So `path::LockFile` names the
  type, `path::LockFile/acquire` a method of an inherent `impl LockFile`, and `path::LockFile[Drop]/drop` the method of
  the trait impl.
- **Path match.** S1/…/Sk matches an item whose name path has at least k segments and whose last k segments are matched
  by S1 … Sk in order: a Serena-style relative name path ([40 §2.7]).
- **Preference.** When some items match with every segment matched by equality (X = n, Y = q), only they count;
  otherwise every matching item counts.

### 6.3 The `symbol` form on a TOML file

A segment matches a TOML item iff it is `X` (no brackets) and X equals the item's `name`; the path match is §6.2's
suffix match. `Cargo.toml::dependencies/serde` names the key `serde` of `[dependencies]`; `Cargo.toml::serde` names it
too when no other table has a key `serde`; `Cargo.toml::workspace.dependencies` names that table; a quoted key is typed
with its quotes (`Cargo.toml::target."cfg(windows)".dependencies/winapi`).

### 6.4 The `heading` form

A segment H matches a heading iff H equals its `name`, or its text (§4.7, the numbering included), or its `qual` when
that is not empty: `#Recovery`, `#3.2 Recovery` and `#3.2` all match `## 3.2 Recovery`. The path match is §6.2's suffix
match: `#Storage/Recovery` names a `Recovery` heading whose parent is a `Storage` heading. Levels are never typed.

### 6.5 Outcomes

The capture of a `symbol` or `heading` form ([F20 §6.1]) scans the file and ends in exactly one of:

| Case | Condition | Result |
|---|---|---|
| found | exactly one item matches (after §6.2's preference), and its name path is recordable and names one item of t | the anchor's item: its header line gives the quote ([F20 §6.1] step 3), its range the hint (step 7), its name path the scope (§2.4) |
| not found | no item matches | refused, [F19 §10.2] `anchor_spec` "symbol or heading not found" |
| several | two or more items match; or one matches but its name path names several items (a Markdown heading picked by its numbering whose name path, which ignores numbering, is shared) | refused, exit 2, the items listed by their scope texts ([F19] text: open point 8) |
| not recordable | one item matches and its name path is not recordable (§2.3) | refused, exit 2 ([F19] text: open point 8); the `path:L-M` form of its lines records its nearest recordable ancestor (§2.4) |
| failed scan | the scan failed (§2.7) | refused, exit 2 ([F19] text: open point 8) |
| syntax | an empty segment, a malformed `X[Y]`, a segment over `SCOPE_MAX_BYTES` | refused, exit 2 ([F19] text: open point 8) |

The conditions are tested in this order, the first that holds deciding: syntax; failed scan; not found; several (two or
more items match); not recordable; several (the one item's name path names several items); otherwise found. While
[F20 §6.1]'s interim rule holds, every `symbol` and `heading` form is refused before any of this (`anchor_spec`, case
`no-scanner`).

## 7. Constants

Every constant of this chapter, part of resolver version 1 ([F20 §7] cites this table):

| Name | Value | Unit | § | Source |
|---|---|---|---|---|
| `SCOPE_MAX_SEGMENTS` | 64 | segments | 2.3 | [F08 §10.3.1] |
| `SCOPE_MAX_BYTES` | 4,096 | bytes of a scope value | 2.3, 6.1 | this chapter (open point 7) |
| `RUST_MAX_DEPTH` | 1,024 | open groups | 3.3 | this chapter (open point 7) |
| `TOML_MAX_DEPTH` | 1,024 | open brackets of one value | 5.4 | this chapter (open point 7) |
| `MD_MAX_LEVEL` | 6 | `#` bytes | 4.3 | CommonMark |
| `MD_MAX_INDENT` | 3 | columns | 4.1 | CommonMark |
| `MD_TAB_STOP` | 4 | columns | 4.1 | CommonMark |
| `MD_FENCE_MIN` | 3 | fence bytes | 4.5 | CommonMark |
| `NUM_MAX_DIGITS` | 9 | digits per numbering component and ordered-list marker | 4.6, 4.7 | CommonMark (list markers) |
| language suffixes | `.rs`; `.md`, `.markdown`; `.toml` (`eqi`) | — | 1.3 | [40 §2.7.1] |
| Rust keywords K | the 51 words of §3.1 | — | 3.1 | Rust reference (strict and reserved keywords) |
| Rust qualifier words | `async`, `unsafe`, `safe`, `default`, `extern` (with an optional ABI string), and `const` before `fn`, `async`, `unsafe` or `extern` | — | 3.5 | this chapter |
| numbering separators | `-`, `–`, `—`, each followed by SP | — | 4.7 | this chapter |

## 8. Golden examples

R-FIX writes one fixture per construct below (WP-21, `fixtures/r4/`; the part that `fixtures/r4/INDEX.md` G-6 names),
from this chapter and never from a scanner's or the oracle's output. The oracle's unit tests cover the Rust constructs of
the contract. Each construct gives a source, the items, and where stated header texts ([F20 §2.8]), recorded scopes
(§2.4) and form results (§6). Line numbers are those of the source as shown. A source written on one line with ⏎ has no
trailing line break unless it ends in ⏎.

### 8.1 Rust

Constructs 1–14 are [F20] Appendix A.9's; construct 10 gains its last two rows.

| # | Construct | Source | Items |
|---|---|---|---|
| 1 | every kind | `mod m {}⏎impl S {}⏎fn f() {}⏎struct S;⏎enum E { A }⏎trait T {}⏎const C: u8 = 1;⏎static G: u8 = 2;⏎macro_rules! mac { () => {} }` | `mod m` 1–1; `impl S` 2–2; `fn f` 3–3; `struct S` 4–4; `enum E` 5–5; `trait T` 6–6; `const C` 7–7; `static G` 8–8; `macro_rules mac` 9–9 |
| 2 | declarations that are not items | `use std::fmt;⏎extern crate alloc;⏎type A<T> = Vec<T>;⏎union U { a: u8, b: u16 }⏎extern "C" {}⏎fn f<const N: usize>() -> impl Sized { let c = const { 1 }; let k = \|\| 2; }⏎trait T { type Assoc; }⏎enum E { Variant { field: u8 } }` | `fn f` 6–6; `trait T` 7–7; `enum E` 8–8 |
| 3 | visibility and qualifiers | `pub const fn a() {}⏎pub(crate) async unsafe fn b() {}⏎extern "C" fn c() {}⏎pub(in crate::x) static mut D: u8 = 0;⏎unsafe trait E {}⏎const _: () = ();` | `fn a` 1–1; `fn b` 2–2; `fn c` 3–3; `static D` 4–4; `trait E` 5–5; `const _` 6–6 |
| 4 | raw and non-ASCII identifiers | `fn r#match() {}⏎struct Größe;⏎mod 名前 {}⏎fn _private() {}` | `fn r#match` 1–1; `struct Größe` 2–2; `mod 名前` 3–3; `fn _private` 4–4 |
| 5 | siblings that touch | `mod a{fn f(){}}fn g(){}struct S;` | `mod a` 1–1; `fn f` 1–1 in `mod a`; `fn g` 1–1; `struct S` 1–1 |
| 6 | parents follow containment | `fn outer() {⏎    let _ = { fn deep() {} };⏎    fn shallow() {}⏎}` | `fn outer` 1–4; `fn deep` 2–2 in `fn outer`; `fn shallow` 3–3 in `fn outer` |
| 7 | macro bodies are token trees | `macro_rules! mk {⏎    ($n:ident) => { fn $n() {} struct Inside; };⏎}⏎thread_local! { static TL: u8 = 0; }⏎lazy_static::lazy_static! { static ref X: u8 = 0; }⏎mk!(made);⏎fn after() {}` | `macro_rules mk` 1–3; `fn after` 7–7 |
| 8 | LF, CR LF, and a BOM with CR LF (one `atext`) | `fn a() {}⏎⏎impl S {⏎    fn b() {}⏎}⏎` | `fn a` 1–1; `impl S` 3–5; `fn b` 4–4 in `impl S` |
| 9 | a last line without a terminator | `fn a() {⏎}` | `fn a` 1–2 |

**10. `impl` names and qualifiers** (one source per row, each item on line 1 unless stated):

| Source | `name` | `qual` |
|---|---|---|
| `impl<T> Pool<T> {}` | `Pool<T>` | (empty) |
| `impl<T: Clone> From<T> for Wrap<T> {}` | `Wrap<T>` | `From<T>` |
| `impl<T> !Send for Raw<T> {}` | `Raw<T>` | `!Send` |
| `impl ! /* comment */ Sync for Raw2 {}` | `Raw2` | `!Sync` |
| `unsafe impl<'a> Sync for &'a   mut [u8] {}` | `&'a mut[u8]` | `Sync` |
| `impl Tr for dyn Fn(u8) -> u8 + Send {}` | `dyn Fn(u8)->u8+Send` | `Tr` |
| `impl Tr for extern "C" fn() {}` | `extern "C" fn()` | `Tr` |
| `impl Tr for (A, B) {}` | `(A,B)` | `Tr` |
| `impl<T: Iterator> Tr for <T as Iterator>::Item {}` | `<T as Iterator>::Item` | `Tr` |
| `impl crate::a::Tr<u8> for super::b::C<{ 4 }> {}` | `super::b::C<{4}>` | `crate::a::Tr<u8>` |
| `impl<T> Display⏎    for Multi<⏎        T, // why⏎        u8,⏎    >⏎where⏎    T: Clone,⏎{⏎}` (lines 1–9) | `Multi<T,u8,>` | `Display` |
| `impl Tr for Foo<{ "a⏎b".len() }> {}`, LF or CR LF (lines 1–2) | `Foo<{"a\nb".len()}>` | `Tr` |
| `impl Tr for Bar<{ r"x⏎y" }> {}` (lines 1–2) | `Bar<{r"x\ny"}>` | `Tr` |
| `impl Tr for ! {}` | `!` | `Tr` |
| `impl Tr for ty!{ x } {⏎    fn m() {}⏎}` (lines 1–3; `fn m` 2–2 in it) | `ty!{x}` | `Tr` |

**11. Lines and header texts** ([F20 §2.8]). Source:

```
/// Doc comment.
#[derive(Debug)]
#[cfg(test)]
pub struct S {
    x: u8,
}

// A plain comment.
#[inline]
pub(crate)
unsafe fn f(
    a: u8,
) -> u8 {
    a
}

/** Block doc. */
mod m;

#[macro_export]
macro_rules! mac (
    () => {}
);
```

Items: `struct S` 4–6; `fn f` 10–15; `mod m` 18–18; `macro_rules mac` 21–23. Headers (␊ is the byte `0A`):
`header(4, symbol)` = `pub struct S`; `header(10, symbol)` = `pub(crate)␊unsafe fn f(␊a: u8,␊) -> u8`;
`header(18, symbol)` = `mod m`; `header(21, symbol)` = `macro_rules! mac (␊() => {}␊)`.

**12. Nesting at any depth.** Source:

```
mod a {
    impl Tr for S {
        fn f() {
            struct Local;
            impl Local { fn g(&self) {} }
            let _x = { enum E { A } 1 };
        }
    }
    extern "C" {
        fn ext();
        static EXT: u8;
    }
    trait T {
        const K: u8;
        fn decl(&self);
        fn def(&self) { fn inner() {} }
    }
    const C: () = { fn in_const() {} };
}
fn top() {}
```

Items: `mod a` 1–19; `impl S [Tr]` 2–8 in `mod a`; `fn f` 3–7 in `impl S`; `struct Local` 4–4 in `fn f`; `impl Local`
5–5 in `fn f`; `fn g` 5–5 in `impl Local`; `enum E` 6–6 in `fn f`; `fn ext` 10–10 in `mod a`; `static EXT` 11–11 in
`mod a`; `trait T` 13–17 in `mod a`; `const K` 14–14, `fn decl` 15–15 and `fn def` 16–16 in `trait T`; `fn inner` 16–16
in `fn def`; `const C` 18–18 in `mod a`; `fn in_const` 18–18 in `const C`; `fn top` 20–20. Recorded scopes (§2.4): a
span on line 5 records `rust:mod a/impl S[Tr]/fn f/impl Local/fn g`; lines 9–12 record `rust:mod a`; line 16 records
`rust:mod a/trait T/fn def/fn inner`.

**13. Scopes that repeat or have no common item.** Source
`mod a {⏎    #[cfg(unix)]⏎    fn g() { unix() }⏎    #[cfg(windows)]⏎    fn g() { windows() }⏎}⏎fn h() {} fn k() {}`:
items `mod a` 1–6; `fn g` 3–3 and `fn g` 5–5 in `mod a`; `fn h` 7–7; `fn k` 7–7. A span on line 5 records `rust:mod a`
(`mod a/fn g` names two items); a span on line 7 records no scope.

**14. The `symbol` form** (§6.2). Source
`struct Wrap<T>(T);⏎impl<T> Wrap<T> { fn get(&self) {} }⏎impl<T: Clone> From<T> for Wrap<T> { fn from(t: T) -> Self { Wrap(t) } }⏎impl From<u8> for Wrap<u16> { fn from(v: u8) -> Self { Wrap(v.into()) } }`:

| Form after `path::` | Result |
|---|---|
| `Wrap` | `struct Wrap` (the last segment matches no `impl`) |
| `Wrap/get` | `impl Wrap<T>/fn get` (bare name `Wrap`) |
| `get` | `impl Wrap<T>/fn get` |
| `Wrap[From]/from` | several: `impl Wrap<T>[From<T>]/fn from` and `impl Wrap<u16>[From<u8>]/fn from` |
| `Wrap[From<u8>]/from` | `impl Wrap<u16>[From<u8>]/fn from` |
| `Wrap<u16>[From<u8>]/from` | the same, by equality |
| `from` | several: two items |
| `Wrap/from` | not found (`Wrap` without brackets matches no trait impl) |

Constructs 15–27 pin input outside the contract (§3.9) and the rules that §3.3–§3.7 add:

| # | Construct | Source | Items |
|---|---|---|---|
| 15 | unterminated literals and comments | (a) `fn a() { let s = "abc; }⏎fn b() {}`; (b) `fn a() {} /* fn b() {}`; (c) `fn a() {}⏎const S: &str = r#"abc;⏎fn b() {}` | (a) `fn a` 1–2; (b) `fn a` 1–1; (c) `fn a` 1–1; `const S` 2–3 |
| 16 | unbalanced groups | (a) `mod m {⏎    fn a() {}⏎fn b() {}`; (b) `fn a() {} }⏎fn b() {}`; (c) `fn a() { foo(; }⏎fn b() {}` | (a) `mod m` 1–3; `fn a` 2–2 in `mod m`; `fn b` 3–3 in `mod m`; (b) `fn a` 1–1; `fn b` 2–2 (a stray `}` ends a block); (c) `fn a` 1–1; `fn b` 2–2 (the `}` closes the `(` implicitly) |
| 17 | a `<` that never closes | (a) `fn f(x: Vec<u8) {⏎}⏎fn g() {}`; (b) `struct S<T {⏎}⏎fn g() {}` | (a) `fn f` 1–2; `fn g` 3–3; (b) `struct S` 1–2; `fn g` 3–3 |
| 18 | const-generic blocks in headers | `fn f<const N: usize = { 3 }>() -> Foo<{ N }> {⏎}⏎fn g() {}` | `fn f` 1–2; `fn g` 3–3 |
| 19 | items without a name | (a) `impl<T> {⏎    fn f() {}⏎}`; (b) `impl Tr for {}⏎fn g() {}`; (c) `fn () { fn inner() {} }`; (d) `fn match() { fn x() {} }` | (a) `fn f` 2–2, no parent; (b) `fn g` 2–2; (c) `fn inner` 1–1; (d) `fn x` 1–1 |
| 20 | words that are not qualifiers | (a) `auto trait A {}⏎fn g() {}`; (b) `static ref X: u8 = 0;⏎fn g() {}`; (c) `impl S { default fn f() {} }`; (d) `trait A = B;⏎fn g() {}` | (a) `fn g` 2–2; (b) `fn g` 2–2; (c) `impl S` 1–1; `fn f` 1–1 in `impl S`; (d) `trait A` 1–1; `fn g` 2–2 |
| 21 | what is a token tree | (a) `fn a() { if !(x) { fn b() {} } }`; (b) `gen! { fn x() {} }⏎fn y() {}`; (c) `#[doc = { fn x() {} }] fn y() {}`; (d) `macro_rules! { fn x() {} }⏎fn y() {}` | (a) `fn a` 1–1; `fn b` 1–1 in `fn a`; (b) `fn y` 2–2; (c) `fn y` 1–1; (d) `fn y` 2–2 |
| 22 | shebang and inner attribute | (a) `#!/usr/bin/env rust-script⏎fn main() {}`; (b) `#![allow(x)]⏎fn main() {}` | (a) `fn main` 2–2; (b) `fn main` 2–2 |
| 23 | extents without a terminator | (a) `mod m { const X: u8 = 1 }⏎fn g() {}`; (b) `fn f() -> X⏎fn g() {}` | (a) `mod m` 1–1; `const X` 1–1 in `mod m`; `fn g` 2–2; (b) `fn f` 1–2 (`fn g` is part of `f`'s header) |
| 24 | the depth limit | `fn a() ` followed by 1,024 `(`, 1,024 `)` and ` {}`; the same with 1,025 of each | `fn a` 1–1; failed scan |
| 25 | a name that is not UTF-8 | `fn caf` `E9` `() {}` | `fn caf`U+FFFD 1–1 |
| 26 | `unsafe extern` blocks (Rust 2024) | `unsafe extern "C" {⏎    safe fn f();⏎    pub safe static S: u8;⏎}` | `fn f` 2–2; `static S` 3–3 |
| 27 | weak keywords as names | `const safe: bool = true;⏎const default: u8 = 0;⏎impl Default for X { fn default() -> Self { X } }` | `const safe` 1–1; `const default` 2–2; `impl X [Default]` 3–3; `fn default` 3–3 in `impl X` |

### 8.2 Markdown

Sources are shown between `~~~~` lines; line numbers count from the first line inside.

**M1. ATX headings, numbering, closing sequence.**

~~~~
# Title

## 1. Scope

text

### 1.1 What   it  fixes ###

## 2 Layout
~~~~

Items: `h1 Title` 1–9; `h2 Scope [1.]` 3–8 in `h1 Title`; `h3 What it fixes [1.1]` 7–8 in `h2 Scope`; `h2 Layout [2]`
9–9 in `h1 Title`. Headers: `header(3, heading)` = `## 1. Scope`; `header(7, heading)` = `### 1.1 What   it  fixes ###`
(the line as trimmed by `nl`, never collapsed).

**M2. What is not an ATX heading.** Source (line 9 holds an HT after `#`):

~~~~
#Not a heading
####### seven
    # indented code
   ### three spaces
#
## ##
# foo#
# foo \#
#	Tab
~~~~

Items: `h3 three spaces` 4–6; `h1 foo#` 7–7; `h1 foo \#` 8–8; `h1 Tab` 9–9. Lines 5 and 6 are empty headings: no
items, so `h3 three spaces` runs to line 6. An HT before the `#` counts 4 columns, so HT `# x` is never a heading.

**M3. Setext headings.**

~~~~
Title
=====

Sub
title
---

    code
---
~~~~

Items: `h1 Title` 1–9; `h2 Sub title` 4–9 in `h1 Title`. The heading on lines 4–6 is one paragraph of two lines; its
header is line 4 (`Sub`). Line 8 is indented code, so line 9 is a thematic break.

**M4. Fences** (shown between lines of six `~`).

~~~~~~
# A
```rust
# not a heading
```
~~~~~
```
# still code
~~~~~
## B
``` x ` y
# after
~~~~~~

Items: `h1 A` 1–10; `h2 B` 9–10 in `h1 A`; `h1 after` 11–11. Line 5 opens a tilde fence of 5 that only line 8 closes;
line 10 is no fence (a backtick in a backtick fence's info), so it is a paragraph that line 11 interrupts.

**M5. An unclosed fence.** `# A⏎```⏎# hidden⏎## hidden too⏎`: `h1 A` 1–4.

**M6. HTML comments.** `# A⏎<!--⏎# commented out⏎-->⏎## B⏎<!-- one line -->⏎## C⏎`: `h1 A` 1–7; `h2 B` 5–6 in
`h1 A`; `h2 C` 7–7 in `h1 A`.

**M7. Front matter.** `---⏎title: X⏎---⏎# Real⏎`: `h1 Real` 4–4 (without the front-matter rule, CommonMark would read
`title: X` as a setext `h2`). `---⏎title: X⏎` has no closing line: no front matter, no item.

**M8. Container lines.**

~~~~
# A
> # quoted
- # listed
1. # ordered
text
- item
continued
---

Para
- item
---

Para
2. two
---
~~~~

Items: `h1 A` 1–16; `h2 Para 2. two` 14–16 in `h1 A`. Lines 2–7 are container lines; line 8 and line 12 are thematic
breaks; `2.` cannot interrupt the paragraph of line 14, so lines 14–15 are one paragraph and line 16 its underline.

**M9. Levels and sections.** `## Two⏎#### Four⏎### Three⏎# One⏎###### Six⏎`: `h2 Two` 1–3; `h4 Four` 2–2 in `h2 Two`;
`h3 Three` 3–3 in `h2 Two`; `h1 One` 4–5; `h6 Six` 5–5 in `h1 One`.

**M10. Numbering** (§4.7; each source is `# ` followed by the text, HT written as HT):

| Heading text | `name` | `qual` |
|---|---|---|
| `3.2 Recovery` | `Recovery` | `3.2` |
| `§3 Storage` | `Storage` | `§3` |
| `§ 3.1 Storage` | `Storage` | `§ 3.1` |
| `A.1 Parts` | `Parts` | `A.1` |
| `A. Intro` | `Intro` | `A.` |
| `1. Scope` | `Scope` | `1.` |
| `1) First` | `First` | `1)` |
| `20 — R4 constants` | `R4 constants` | `20` |
| `2.7 - Anchors` | `Anchors` | `2.7` |
| `3.2.` SP SP `Tabs` HT `and` SP SP `spaces` | `Tabs and spaces` | `3.2.` |
| `2026-09-28 update` | `2026-09-28 update` | (empty) |
| `3D graphics` | `3D graphics` | (empty) |
| `1.2.3` | `1.2.3` | (empty) |
| `A Tale` | `A Tale` | (empty) |
| `A.I. and ML` | `A.I. and ML` | (empty) |
| `1234567890 big` | `1234567890 big` | (empty) |
| `10x faster` | `10x faster` | (empty) |
| `3.2 —` | `—` | `3.2` |
| `3.2 Восстановление` | `Восстановление` | `3.2` |

**M11. Paragraph continuation.** `Para⏎    indented continuation⏎===⏎⏎- item⏎next⏎===⏎`: `h1 Para indented continuation`
1–7. Line 2 continues the paragraph although it is indented; lines 5–7 are container lines, so line 7 is no underline.

**M12. No setext heading.** `Text⏎***⏎⏎***⏎Text2⏎- - -⏎`: no item (`***` and `- - -` are thematic breaks).

**M13. Invalid UTF-8.** `# caf` `E9` `⏎`: `h1 caf`U+FFFD 1–1.

**M14. Scopes and the `heading` form.** Source:

~~~~
# 1 Design

## 1.1 Storage

### Recovery

## 1.2 Input/output

### Recovery

# 2 Plan

## Storage
~~~~

Items: `h1 Design [1]` 1–10; `h2 Storage [1.1]` 3–6 in `h1 Design`; `h3 Recovery` 5–6 in `h2 Storage`;
`h2 Input/output [1.2]` 7–10 in `h1 Design`; `h3 Recovery` 9–10 in `h2 Input/output`; `h1 Plan [2]` 11–13;
`h2 Storage` 13–13 in `h1 Plan`.

Recorded scopes (§2.4): a span on line 5 records `markdown:h1 Design[1]/h2 Storage[1.1]/h3 Recovery`; line 7 records
`markdown:h1 Design[1]/h2 Input%2foutput[1.2]` ([F14 §5.6] escapes the `/`); lines 3–9 record `markdown:h1 Design[1]`;
line 13 records `markdown:h1 Plan[2]/h2 Storage`. After line 3 is renumbered to `## 1.4 Storage`, the recorded
`markdown:h1 Design[1]/h2 Storage[1.1]/h3 Recovery` still resolves uniquely (numbering is not compared, §2.2).

| Form after `path#` | Result |
|---|---|
| `Recovery` | several: the `Recovery` headings under `Storage` and under `Input/output` |
| `Storage/Recovery` | `h3 Recovery` 5–6 |
| `Design/Storage` | `h2 Storage [1.1]` 3–6 |
| `1.1 Storage` | `h2 Storage [1.1]` 3–6 (by its text) |
| `1.1` | `h2 Storage [1.1]` 3–6 (by its numbering) |
| `Storage` | several: `h2 Storage [1.1]` and `h1 Plan/h2 Storage` |
| `Input%2Foutput/Recovery` | `h3 Recovery` 9–10 |
| `Plan/Storage` | `h2 Storage` 13–13 |
| `Nothing` | not found |

### 8.3 TOML

**T1. Tables and keys.**

~~~~
name = "top"
[package]
name = "x"
version = "0.1.0"

# comment
[dependencies]
serde = { version = "1", features = ["derive"] }
~~~~

Items: `key name` 1–1; `table package` 2–4; `key name` 3–3 in `table package`; `key version` 4–4 in `table package`;
`table dependencies` 7–8; `key serde` 8–8 in `table dependencies`. Headers ([F20 §2.8]): `header(2, symbol)` =
`[package]`; `header(3, symbol)` = `name = "x"`; `header(8, symbol)` = `serde =` (the Rust cut stops at `{`, open point
11). Recorded scopes: line 3 records `toml:table package/key name`; line 8 `toml:table dependencies/key serde`; line 1
`toml:key name`; lines 5–6 none. Forms after `path::`: `package/name` → `key name` 3–3; `name` → several (`key name` and
`table package/key name`); `dependencies/serde` and `serde` → `key serde` 8–8; `dependencies` → `table dependencies` 7–8;
`package[x]/name` → not found.

**T2. Arrays of tables.** `[[bin]]⏎name = "a"⏎⏎[[bin]]⏎name = "b"⏎`: `array_table bin` 1–2; `key name` 2–2 in
`array_table bin`; `array_table bin` 4–5; `key name` 5–5 in `array_table bin`. A span on line 2 records no scope; the
form `bin/name` is refused as several.

**T3. Dotted and quoted keys.**

~~~~
[ workspace . dependencies ]
a.b = 1
"quoted key" = 2
'lit' = 3
"a.b" = 4
[target."cfg(windows)".dependencies]
winapi = "0.3"
~~~~

Items: `table workspace.dependencies` 1–5; `key a.b` 2–2, `key "quoted key"` 3–3, `key 'lit'` 4–4 and `key "a.b"` 5–5
in `table workspace.dependencies`; `table target."cfg(windows)".dependencies` 6–7; `key winapi` 7–7 in it. Forms:
`workspace.dependencies/a.b` → `key a.b` 2–2; `"quoted key"` → `key "quoted key"` 3–3;
`target."cfg(windows)".dependencies/winapi` → `key winapi` 7–7.

**T4. A multi-line array.**

~~~~
features = [
  "a",
  [1],
  "b", # comment ]
]
next = 1
~~~~

Items: `key features` 1–5; `key next` 6–6. Line 3 is no header and the `]` in the comment of line 4 closes nothing.
`header(1, symbol)` = `features = [␊"a",␊[1],␊"b", # comment ]` (the Rust tokenisation of [F20 §2.8] does not know TOML
comments, open point 11).

**T5. Multi-line strings.** `text = """⏎[not.a.table]⏎key = 1⏎"""⏎lit = '''⏎[also.not]⏎'''⏎after = 1⏎`: `key text` 1–4;
`key lit` 5–7; `key after` 8–8.

**T6. Lines that are nothing.** `[a]⏎x = 1⏎[b] junk⏎y = 2⏎not a key⏎[[c] ]⏎z = 3⏎`: `table a` 1–7; `key x` 2–2,
`key y` 4–4 and `key z` 7–7 in `table a`.

**T7. An unclosed array.** `a = [1, 2⏎[b]⏎c = 1⏎`: `key a` 1–3.

**T8. Value forms.** `a = "x" # c⏎b = 'y'⏎c = 1979-05-27T07:32:00Z⏎d = [ { x = 1 }, { y = [2, 3] } ]⏎e =⏎f = """one-line"""⏎g = "unterminated⏎h = 1⏎`:
`key a` 1–1 … `key h` 8–8, one item per line (`e` has an empty value; `g`'s string ends with its line).

**T9. Quote runs.** `s = """a""""⏎t = """a\"""⏎b"""⏎u = 1⏎`: `key s` 1–1 (a run of 4 closes); `key t` 2–3 (the `\`
takes the first `"`, so the run on line 2 is 2 long); `key u` 4–4.

**T10. A table without keys.** `[a]⏎[b]⏎x = 1⏎`: `table a` 1–1; `table b` 2–3; `key x` 3–3 in `table b`.

**T11. Keys outside TOML 1.0.** `ключ = 1⏎[таблица]⏎ok = 1⏎`: `key ok` 3–3, no parent (lines 1 and 2 are nothing).

**T12. The depth limit.** `a = ` followed by 1,024 `[` and 1,024 `]`: `key a` 1–1; with 1,025 of each: failed scan.

### 8.4 Files without a scanner

A `.py`, `.hlsl`, `.json` or `.txt` file has no items: every capture on it records no scope, and every `path::…` or
`path#…` spec on it does not split (§6.1), so it is read as a path. A file renamed from `x.md` to `x.txt` keeps its
anchors' Markdown scopes, which no longer resolve (§2.5).

## Coverage

| Item | Part covered here | Section |
|---|---|---|
| [40 §2.7.1] scope scanners (OQ-R-2; [F20] open point 30) | complete: the Rust, Markdown and TOML scanners as total algorithms (tokens and line classes, items, names and qualifiers, line spans, nesting, malformed input, limits), name paths, the scope a capture records, how a scope resolves, same-kind headers, the authoring forms, one fixture per construct. Lifting [F20 §6.1]'s interim rule waits for review (open point 1) | §1–§8 |
| [40] R-4 and R-10 (anchor record, selector block) | the bytes of the `scope` selector's segments (`skind`, `name`, `qual`) and which scope a capture records; the record and block layouts are [F08 §10.3]'s and [F07 §8.2]'s | §2.1–§2.4, §3.7, §4.7, §5.2 |
| [40] R-14 (resolver constants) | the scanner part of resolver version 1: §7's constants and the scanner rules; the rest is [F20]'s | §1.1, §7 |
| [60 §2.5] R4 reservations, row R-14 | the scanner part, as R-14 | §7 |

## Holes

None. Every value of this chapter is fixed. The limits of §7 are not measurement-decided: no M0 measurement exercises a
file near them, and a change after the tag would be a new resolver version.

## Open points for the review

1. **Lifting the interim scanner rule (for pass 2).** OQ-R-2 (decided 2026-09-28) requires this appendix before the
   freeze and before WP-63 is accepted, and keeps [F20 §6.1]'s interim rule until it exists; the owner's text does not
   say who declares it complete. This chapter does not decide it. Proposal: the interim rule is lifted when review
   pass 2 accepts this chapter with no open blocker or major finding; [F20 §6.1], [F08 §10.3.1], [F14 §5.6] and
   [F19 §10.2] then drop their interim texts in one sync. If pass 2 does not accept it before the freeze, format v1 keeps
   the interim rule (OQ-R-2 option (b)).
2. **Status row.** [F01 §2.3] has no status for a chapter written after pass 1 closed. This chapter uses `draft, pass 1
   pending`, as the closest listed value, and is reviewed first in pass 2. [F01] (R-SPEC-F) may add a status
   `draft, pass 2 pending`.
3. **Correction: the scope item of a capture** (§2.4, formerly [F20] Appendix A.6). A.6 read "the deepest item of C that
   is an ancestor of, or equal to, every item of C"; since C holds every ancestor of its items, that is always C's
   outermost item, while A.9 construct 12 expects the innermost (`rust:mod a/impl S[Tr]/fn f/impl Local/fn g` for line
   5). §2.4 now takes the deepest common ancestor-or-self of C's innermost items, which gives every expectation of
   A.9 unchanged and `mod a` for two sibling items on one line inside `mod a`.
4. **Correction: tokens are byte-level** (§3.1, formerly A.2). A.2 defined words, whitespace and character literals over
   "characters", which is not total over invalid UTF-8. §3.1 matches whitespace as byte sequences, makes every other
   byte ≥ `80` a word byte, defines a character as a well-formed sequence or one byte, admits any character but `'` and
   `\` in `'c'` (as tree-sitter-rust's `[^\\']`), and adds Rust's shebang rule. On valid UTF-8 nothing changes, and
   `canon` is unchanged: per-token replacement equals whole-text replacement (§3.2). The WP-74 oracle's `canon` reads
   `'''` as one character literal, where §3.1 reads three `'` tokens; rustc rejects `'''`, so the difference lies outside
   the oracle's claim, and R-REPLAY may align it.
5. **The total Rust algorithm** (§3.3–§3.8) replaces A.8's "outside the contract (not yet specified)". Decisions: a
   closer closes the topmost group of its kind and every group above it, a closer of a kind not open is stray (a stray
   `}` still ends a block for item start positions, so one extra brace hides nothing); keywords K exclude the weak
   keywords and `gen` (reserved only from edition 2024, so older code may name a macro or function `gen`); `auto` is not
   a qualifier; a `{` token tree is never a body; the angle count decides only const-generic blocks and resets at the
   body. The oracle's proposals are adopted: a nameless `impl` is not reported and its items go to the next enclosing
   item; names take U+FFFD replacement.
6. **The `symbol` form's split and escapes** (§6.1, formerly A.7's "the path ends at its first `::`"): the split is now
   at the first `::` after a `.rs` or `.toml` prefix, and the heading form's at the first `#` after a `.md` or
   `.markdown` prefix, so paths holding `::` or `#` in a directory still parse; `%2F`, `%5B`, `%5D` and `%25` let a
   form name an item or heading whose name holds `/`, `[`, `]` or `%`. [F20 §6.1] refers here.
7. **The limits** `SCOPE_MAX_BYTES`, `RUST_MAX_DEPTH` and `TOML_MAX_DEPTH` are this chapter's (§2.3, §2.7). [F08 §10.3.1]
   bounds a scope by its 64 segments only; 4,096 bytes bounds a stored scope and lets implementations compare long names
   by a marker. Whether [F08 §10.3.1] adds it as a validity rule for decoders (an imported longer scope would then be
   invalid) is R-SPEC-F's call; this chapter binds only capture.
8. **For R-SPEC-F: [F19 §10.2] `anchor_spec` texts** for the refusals of §6.5 that have none yet: several matches (with
   the list), a name path that is not recordable (deeper than 64 segments or over 4,096 bytes), a failed scan, and a
   malformed selector. The "not found" row's condition "(once the scanner appendix of [F20] exists)" becomes "[F21 §6.5]".
9. **Markdown containers** (§4.6, §4.9). The scanner recognises only the first lines of block quotes and list items and
   keeps no container stack; CommonMark's container rules need a stack of open blocks with lazy continuation.
   Consequences: a heading inside a list item after a blank line is read as a document heading, a fence opened inside a
   list item ends only at its closing fence, and HTML blocks other than comments are not recognised. Front matter is
   skipped though CommonMark has none, because a YAML block would otherwise yield a bogus setext heading. Alternative:
   CommonMark 0.31.2 exactly, with a container stack. No M0 oracle checks the Markdown scanner ([40 §8.3.4] row 8 is the
   Rust scanner's), so the fixtures of §8.2 are its only check; a host-only CommonMark differential, like WP-74's for
   Rust, is possible later.
10. **TOML key spelling** (§5.2). Keys are spelled as written, so `a` and `"a"` differ and `[ a . b ]` equals `[a.b]`
    only through the whitespace rule. Decoding quoted keys would make equal TOML keys equal names, but a decoded key
    holding `.` would then collide with a dotted path. Kept as written; TOML files are rarely re-quoted.
11. **TOML header texts** (§8.3 T1, T4). [F20 §2.8]'s header uses the Rust tokenisation for every `symbol` anchor, so
    a TOML key with an inline table has the header `serde =` (as [40 §2.7] step 3's "first line up to `{`" also gives),
    and a TOML comment inside a multi-line array is not a comment there. Both are deterministic. Proposal for pass 2:
    for an anchor whose scope has `lang` 3, `header(l, symbol)` is `nl(l)`, the key or header line itself, which covers
    the whole inline table and stops at a multi-line array's first line; it is a function of the anchor's stored scope,
    so the resolver still needs no scanner at the hint. Not adopted here because it changes [F20 §2.8], which spec sync
    2a revised after the WP-74 review, and [40 §2.7] step 3, which WP-81a aligns.
12. **Same-kind headers** (§2.6) take the item kind from the last segment of the anchor's scope, the only place an
    anchor stores it. A captured `symbol` or `heading` anchor always has a scope (§6.5); an imported one without a scope
    gets no restriction, as under the interim rule.
13. **`lines` anchors record a scope** (§2.4): their span is the hint [L, M], since [F20 §6.1] step 4 defines no quote
    span for them; [40 §2.7]'s authoring table adds a scope to every `path:L-M` capture.
