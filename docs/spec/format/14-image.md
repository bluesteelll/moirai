# 14 — The git image

| | |
|---|---|
| Title | The git image (R3): the image tree and its fan-out, the `.moirai-image` marker, the complete `.moi` format version 1 (node files of every kind, tombstones with flagged and historical edges, scalar, body, existence and observation conflicts, ledger lines, block strings, dropped bodies, file and root nodes, R-11 `anchor` lines with digests, hash-only anchors and the `text-unavailable` sub-state, schema tables, F3 named-query files), the git commit objects with their trailers and trailer order, ref mapping and destinations, the unhashed side refs, the reconstruction of every canonical item from an image commit, and the complete gate-0 carrier table |
| Chapter | [F14], `docs/spec/format/14-image.md` |
| Status | draft, pass 1 pending |
| Work package | WP-15 (R-SPEC-R), [PLAN §3.2] item 1 |
| Sources | [AR §5b.1]–[AR §5b.8] (tree layout and fan-out; the `.moi` rules 1–9 and the example; the marker; the commit mapping, the canonical field → carrier table, trailer order, sync, foreign and import-checkpoint rules, ref mapping; determinism rules 1–8; export steps 1–6 and import steps 1–6; round-trip table and gates 0–3; destinations, defaults and failure table); [AR §4.6] (items 1–10, "Not hashed", "Net changeset = state diff", the reservation-table rows "Canonical form" and "Image" for R4 and R5); [AR §2.15] T15 (N4, N5, N6, N12, N13a–c, CB1, CB2, CM2, CM3, CM4, CL1, O5); [AR §3.1]–[AR §3.3] (header columns, kinds and fields, edge kinds and properties); [AR §5a.1] (ref names and kinds, tags), [AR §5a.5] (revert, cherry-pick); [40 §2.2] (artifact fields, `relink`), [40 §2.3] (derivations, the `created` rule for dual creation, foreign uids), [40 §2.4] (root node, `path_moves`, `pathmove` classes), [40 §2.7] (anchor fields, `captured`, `pred`, the uniqueness ladder), [40 §2.11] R-1–R-5, R-10, R-11, R-12, R-13, R-17 (authoritative), [40 §5.5] (the observation composite as one merge key), [40 §5.7] (file and root node files, anchor lines, anchor text, hash-only destinations, `text-unavailable`, import validation, "never exported"); [50 §4.4] (portable form, storage, merge, the image file and its ABNF sketch), [50 §8.1] F3, F10, F14, F16; [60 §2.5] rows "Image format v1" ([AR] row and audit row), "Gate-0 carrier table", audit row "Canonical form", R4 row R-11, R5 row F3; [60 §3.6] (M5 exit criteria, gates 0–3); [80 §2.10] P1, P11, P12, [80 §3.1] X-F9, [80 §3.2] (image names, modes); [90 §10.1] row "Commit header"; reviews `docs/spec/reviews/a1-S.md` S-02, S-03, S-04 and `a1-dispositions.md` FB-4, FB-5, FS-2; [F07 §14] (the carrier-table stub this chapter completes) and [F07] open points 5, 6, 8, 9, 17, 18, 21, 22, 23, 26; [F06] open points 2 and 13; [RULES/merge-table] open points 5 (d) and 11; [F10] OP-10-09; [F04] open point 7; [CFG] open point 8; [F20 §2.7.3] and open point 8; [LQ/lexical §10.2]; [LQ/canonical-ast §8]; [PLAN §3.2] WP-15, WP-21; [PLAN §3.3] (the WP-15 gaps). Spec sync 3: [AR §11] #33 and OQ-A-7 with [F06 §8.1] DB-6, DB-10, DB-11 (dropped bodies); [AR §11] OQ-A-9 with [F06 §4.4.17] (the provenance group); [AR §11] OQ-A-10 with [API §8.8] (the harvest cursor) |
| Depends on | [F01], [F06], [F07], [F08]; cites [F02], [F04], [F05], [F10], [F11], [F12], [F13], [F16], [F17], [F18], [F19], [F20], [API], [CFG], [LQ/grammar-v1.ebnf], [LQ/lexical], [LQ/canonical-ast], [LQ/std], [RULES/merge-table], [RULES/link-merge-rules]; external [RFC 5234], [RFC 7405], [RFC 3629], [RFC 8259], [RFC 4648], [RFC 3339], [ECMA-262], [git-objects], [git-hash-transition] |

## 1. Scope

The image is the git-compatible export of a store ([AR §5b]). Every moirai commit, branch and tag maps to git objects by a
pure function of moirai data and the destination's object format (I28′); a git tool can read and diff it; an edit made on
the git side imports as a validated moirai commit. The store stays canonical: git is never consulted to answer a query.

This chapter owns:

- the image tree, its fan-out and the git tree objects that hold it (§3);
- the `.moirai-image` marker (§4);
- the text forms of every value of the closed type set in the image (§5);
- the `.moi` format version 1: node files (§6), schema tables and named-query files (§7), checkpoint ref rows (§8), and what
  an importer accepts and refuses (§9);
- the git commit objects of the image: author and committer, the message, the trailers and their order, checkpoint
  commits and tags (§10);
- the reconstruction of every canonical item from an image commit, and the image-only data a store keeps for it (§11);
- the complete gate-0 carrier table, which replaces the stub of [F07 §14] and keeps every row of it (§12);
- ref mapping and destination naming (§13); the unhashed side refs (§14); the determinism rules (§15); the catalogue of the
  `moi/` fixtures (§16).

It does not own: the canonical bytes, `commit_id` and `changeset_digest` ([F07]); the stored commit record ([F06]); the
data model, the schema items and the derivations ([F08]); `gitmap` pages ([F10 §7]); the `GitMap` record ([F05 §9.7]) and
`HEAD.image_cursor` ([F04 §5.11]); merge semantics ([F12], [RULES/merge-table], [RULES/link-merge-rules]); the export
and import protocol with its durability order ([F16]; [AR §5b.6] step 5); the `image.*` configuration keys ([CFG §10.10]);
LQ parsing, binding and the portable form ([LQ/lexical], [LQ/canonical-ast]); git's own object, pack and ref formats
([git-objects], [git-hash-transition]), which the in-process git object layer of M4 reads and writes.

**Terms.**
- **Destination**: a git repository the image is written to ([AR §5b.8]); in format v1 the separate bare repository.
- **d**: a destination's object format (`sha1` or `sha256`) together with its anchor-text mode (`full` or `hash-only`,
  [40] R-13).
- **T(C, d)**: the image tree of moirai commit C for d (§3.3).
- **Node file**: `nodes/<h1>/<h2>/<uid>.moi`. **Live file**: the node file of a live node. **Tombstone file**: the node file
  of a deleted node.
- **Native commit**: a git commit written by an exporter for one moirai commit, with a `Moirai-Commit` trailer.
  **Checkpoint commit**: a git commit written at checkpoint granularity, with `Moirai-Kind: checkpoint` and no
  `Moirai-Commit`. **Foreign commit**: every other git commit, and a native commit whose hash does not verify.
- **Import-checkpoint commit**: the moirai commit an importer creates from a checkpoint commit (kind `checkpoint`, [F06 §3.1]).
- **CS(K)**: the canonical state of moirai commit K ([F07 §6]). **P1(K)**, **P2(K)**: its first and second parent; the
  first parent of a root commit is the empty state and the empty tree.
- **H(K)**: K and every ancestor of K through every parent.

## 2. Notation and text rules

### 2.1 Grammar notation

Grammars are ABNF ([RFC 5234]) with [RFC 7405]'s `%s"…"` for case-sensitive strings. A quoted string without `%s` is used
only where it holds no letter. The core rules `DIGIT`, `HEXDIG` and `ALPHA` are [RFC 5234]'s. A rule written
`<text in angle brackets>` is prose and is defined in the paragraph that follows it. Every file this chapter defines is
UTF-8 without a byte-order mark, with LF line ends, and ends with LF; the grammars state the exact bytes an exporter
writes. §9.1 states the wider set an importer accepts.

### 2.2 Characters

```abnf
LF          = %x0A
SP          = %x20
DQUOTE      = %x22
NZDIGIT     = %x31-39
LHEX        = DIGIT / %x61-66                        ; lower-case hexadecimal digit
LALPHA      = %x61-7A
UALPHA      = %x41-5A
tail        = %x80-BF
u2          = %xC2-DF tail                           ; U+0080 .. U+07FF
u2vis       = %xC2 %xA0-BF / %xC3-DF tail            ; U+00A0 .. U+07FF
u3          = %xE0 %xA0-BF tail / %xE1-EC 2tail / %xED %x80-9F tail / %xEE-EF 2tail
u4          = %xF0 %x90-BF 2tail / %xF1-F3 3tail / %xF4 %x80-8F 2tail
nonascii    = u2 / u3 / u4                           ; every scalar value above U+007F ([RFC 3629])
vis         = %x21-7E / u2vis / u3 / u4              ; a visible character: not SP, not a control
tch         = SP / vis                               ; a text character: no control character
lch         = %x01-09 / %x0B-0C / %x0E-7F / nonascii ; any scalar value but U+0000, LF and CR
bch         = %x00-09 / %x0B-0C / %x0E-7F / nonascii ; any scalar value but LF and CR
```

A **control character** is U+0000–U+001F, U+007F or U+0080–U+009F.

### 2.3 Common tokens

```abnf
hex32       = 32LHEX                                 ; a uid or a BLAKE3-128 digest ([F01 §6.4])
hex40       = 40LHEX
hex64       = 64LHEX
uid         = hex32
dg16        = hex32
commit-id   = %s"c" hex64                            ; a full moirai commit id
oid-text    = %s"sha1:" hex40 / %s"sha256:" hex64    ; an oid ([F01 §7.5]): algorithm name and digest
dec         = "0" / NZDIGIT *DIGIT                   ; unsigned decimal ([F01 §6.5]); the bound is stated per use
sdec        = "0" / [ "-" ] NZDIGIT *DIGIT           ; signed decimal; "-0" is not written
iname       = LALPHA *( LALPHA / DIGIT / "_" )       ; kind, field and stored edge-kind names ([F08 §8.2])
vname       = ( ALPHA / DIGIT / "_" ) *( ALPHA / DIGIT / "_" / "-" )   ; enumeration-value names ([F08 §8.2])
lqname      = UALPHA *( UALPHA / DIGIT / "_" )       ; `lq_name` and reverse names ([F08 §8.2])
rootname    = LALPHA *( LALPHA / DIGIT / "_" / "-" ) ; `project`, `abs` or a named root ([F08 §5.4.1])
refname     = <a moirai ref name>
b64url      = *( ALPHA / DIGIT / "-" / "_" )
```

`refname` is a ref name of [F12] (`main`, `lane/<n>`, `plan/<n>`, `tags/<n>`, …), after [80] X-F9's NFC normalisation of
ref-name input. Name lengths are [F08 §8.2]'s (1–64 bytes); a longer name does not match.

### 2.4 JSON strings

A JSON string is [RFC 8259]'s:

```abnf
jstring     = DQUOTE *( jplain / jesc ) DQUOTE
jplain      = %x20-21 / %x23-5B / %x5D-7F / nonascii
jesc        = %x5C ( DQUOTE / %x5C / %x2F / %x62 / %x66 / %x6E / %x72 / %x74 / %x75 4HEXDIG )
```

- **Writing** (the canonical escape set). An exporter escapes exactly `"` as `\"`, `\` as `\\`, U+0008 as `\b`, U+000C as
  `\f`, LF as `\n`, CR as `\r`, U+0009 as `\t`, and every other scalar value below U+0020 as `\u00` followed by two
  lower-case hexadecimal digits. Every other scalar value, U+007F and non-ASCII characters included, is written as its
  UTF-8 bytes. So one string has one written form.
- **Reading.** An importer accepts every escape of [RFC 8259], `\/` and upper-case hexadecimal digits included, and decodes
  a `\u` surrogate pair to one scalar value. A lone surrogate, or a decoded value that is not valid UTF-8, is `ImageParse`
  (§9.2).

### 2.5 Bare values and tokens

A **line value** is the value that follows `: ` in a `key: value` line. A text is written as a line value in one of two
forms:

```abnf
sval        = bare / jstring
bare        = bfirst [ *tch vis ]                    ; no leading or trailing SP
bfirst      = %x21 / %x23-5A / %x5C-7A / %x7C-7E / u2vis / u3 / u4   ; not DQUOTE, "[" or "{"
```

A text is written **bare** exactly when it is non-empty, at most 4,096 bytes long, contains no control character, has no
leading or trailing SP, does not begin with `"`, `[` or `{`, and is not the two bytes `<<`; otherwise it is written as
its JSON string ([AR §5b.2] rule 4). A bare text may contain SP inside.

A **token** is a value inside a space-separated property list (§6.7, §6.8, §7.1, §8, §14):

```abnf
token       = jstring / tbare
tbare       = tfirst *vis                            ; no SP anywhere
tfirst      = %x21 / %x23-24 / %x26-5A / %x5C-7A / %x7C-7E / u2vis / u3 / u4   ; not DQUOTE, "%", "[" or "{"
```

A text is written as a bare token exactly when it is non-empty, contains no SP and no control character, and does not begin
with `"`, `%`, `[` or `{`; otherwise as its JSON string. An importer that reads a token decodes a JSON string and takes a
bare token as its bytes.

### 2.6 base64url

base64url is [RFC 4648] §5 without padding. An exporter writes the unique encoding: the unused low bits of the last
character are zero. An importer refuses (§9.2) a character outside the alphabet, a length whose remainder modulo 4 is 1,
padding, and non-zero unused bits.

### 2.7 Time text

```abnf
rfc3339ms   = 4DIGIT "-" 2DIGIT "-" 2DIGIT %s"T" 2DIGIT ":" 2DIGIT ":" 2DIGIT "." 3DIGIT %s"Z"
```

The time text of an `hlc` ([F01 §5.7]) is the UTC instant `hlc >> 16` milliseconds after 1970-01-01T00:00:00Z, proleptic
Gregorian, with no leap second ([RFC 3339]). It exists only for instants before year 10000; a line that would carry the time
text of a later instant carries none (§6.3).

### 2.8 Floating point

An `f64` value x ([F01 §5.5]; never NaN or an infinity, and −0.0 is stored as +0.0, [F08 §5.3]) is written by
[ECMA-262]'s `Number::toString(x)` with radix 10: the shortest digit string that rounds to x (ties to even), in positional
notation for 10^−6 ≤ |x| < 10^21 and for 0, otherwise as `d[.ddd]e+n` or `d[.ddd]e-n`; then `.0` is appended when the
result contains neither `.` nor `e`. This is [LQ/envelope §5.2]'s rule, so the image and the output envelope print one
value alike.

```abnf
v-f64       = [ "-" ] ( 1*DIGIT "." 1*DIGIT / DIGIT [ "." 1*DIGIT ] %s"e" ( "+" / "-" ) NZDIGIT *DIGIT )
```

Examples: `28.6`, `0.0`, `0.001`, `-3.0`, `1e+21`, `1.5e-7`. An importer accepts any string of this grammar whose value is a
finite binary64 and re-derives the canonical text; NaN, an infinity and a value outside the binary64 range are `ImageParse`.

## 3. The image tree

### 3.1 Layout

```
.moirai-image                                  the marker (§4)
nodes/<h1>/<h2>/<uid>.moi                      one node file per live or deleted node (§6)
schema/kinds.moi                               project kinds (§7.1)
schema/fields.moi                              project fields and enumeration values (§7.1)
schema/edges.moi                               project edge kinds (§7.1)
schema/policy.moi                              policy-data rows ([F08 §8.5.6]; §7.1)
schema/queries/<q>.moi                         one file per project named query (§7.2)
refs/heads.moi  or  refs/tags.moi              checkpoint commits only: the checkpointed ref (§8)
```

- `<uid>` is the node's uid as 32 lower-case hexadecimal digits; `<h1>` is its first two digits and `<h2>` the next two
  ([AR §5b.1]): two levels of 8-bit fan-out, part of image format 1. File nodes, root nodes and every other kind use the
  same path. There is no file named by `#N` and no directory named by a kind ([AR §5b.1], N4).
- `<q>` is 32 lower-case hexadecimal digits: the first 16 bytes of BLAKE3-256 of the query name's bytes (§7.2.1;
  [50] F3, [80] X-F9, [F01 §6.4]).
- Every name in the tree is ASCII hexadecimal text or a fixed ASCII word, so every tree checks out on Windows, Linux and
  macOS, on case-insensitive and normalization-insensitive volumes included ([80 §2.10] P11 (a)).
- A `schema/*.moi` table file exists exactly when it has at least one row. A directory exists exactly when it holds an entry.
- **Out-edges only.** An edge is written in its source node's file; in-edges are rebuilt by the importer ([AR §5b.1]).
- **Tombstones are files** and stay for the life of the image ([AR §5b.1], I39′). A node file that disappears is a foreign
  hard delete (§11.2).

### 3.2 Git tree objects

- Every blob has mode `100644` and every tree mode `40000`: these are the bytes git writes into a tree object for a regular
  file and a directory ([git-objects]). The zero-padded form `040000`, which [AR §5b.5] rule 1 and [80 §3.2] quote, is the
  conventional display form; written into a tree object it is a `zeroPaddedFilemode` finding of `git fsck` (open point 38).
- Tree entries are sorted by git's rule: bytewise by name, a tree's name compared as if it ended in `/`
  ([AR §5b.5] rule 1). No executable bit, symbolic link, submodule or empty tree is ever written.
- The object format is the destination's `extensions.objectFormat`; objects of the two formats are never mixed in one
  destination ([AR §5b.5] rule 2).

### 3.3 The image tree as a function

For a moirai commit C and a destination d, **T(C, d)** is the tree whose entries are:

1. `.moirai-image` with d's object format and C's schema version (§4);
2. for every node u that exists in CS(C) (its existence key is present, [F07 §6.1]), the node file of §6 encoding u's state
   at C, with the anchor texts d's mode allows (§6.7), and every body the exporting store has dropped written as its hash
   and reason (§6.8, §6.9);
3. for every schema item of CS(C), its row or query file (§7);

and nothing else. A checkpoint commit's tree adds the row file of §8. T(C, d) is a pure function of moirai data, d and
the exporting store's dropped set ([AR §5b] goal 1, I28′; [F06 §8.1] DB-11): two stores that hold C and have dropped the
same bodies for the same reasons produce byte-identical trees for one d. The dropped set changes no canonical item, so
the commit ids and item 10 of every commit are the same whichever bodies are dropped (§11.1, §12.2).

**Reuse.** An exporter builds T(C, d) from the destination tree of C's first parent, rewriting root, touched mid and touched
leaf trees only (G24). It may reuse a subtree only when the subtree equals the corresponding subtree of T(C, d). A subtree
an exporter wrote for d is equal, with three exceptions, and in each the exporter re-encodes:

- **Non-canonical imported trees.** A foreign commit's tree is the git tree it was imported from, and a native commit
  written by hand can verify with non-canonical bytes (reordered lines, CR LF). The importer re-encodes every file it
  parses and records, store-locally, each imported commit with a file that did not re-encode to its own bytes. When C's
  first-parent chain passes through such commits, the exporter re-encodes from CS(C) every path whose entry differs between
  the first parent's tree and the tree of the nearest first-parent ancestor that is not such a commit (the empty tree when
  there is none), and drops every entry outside the layout.
- **Anchor-text mode.** When the destination's mode changed since the first parent was written ([CFG §10.10]), every node
  file that holds an `anchor` line is re-encoded.
- **Image format version.** A tree written by an encoder of another format version is never reused (a new destination by
  rule, §15 rule 6; it does not occur in format v1).
- **Dropped bodies** (spec sync 3). A node file written before the store dropped a body it holds (its body section, a
  body conflict side or an existence snapshot's `body` line) is re-encoded, so its bytes become the dropped form of §6.8
  and §6.9. Such a commit's tree differs from its first parent's in that file while no canonical key changes; the file
  contributes no entry to item 10 (§11.1). Trees exported before the drop are not rewritten ([F06 §8.1] DB-10).

### 3.4 Entries outside the layout

- An entry at the root of a tree whose name is not `.moirai-image`, `nodes`, `schema` or `refs` is ignored by the importer
  and reported by `image doctor`; an exporter never writes one (§3.3).
- Under `nodes/`, `schema/` and `refs/`, every entry must match the layout of §3.1 with the right object type: a directory
  `<h1>` of two lower-case hexadecimal digits, a directory `<h2>` likewise, a blob `<uid>.moi` whose first four digits equal
  `<h1><h2>`; the three table files; `queries/<q>.moi`; the two row files. Any other entry there is `ImageParse` (§9.2),
  because it looks like data the importer would otherwise drop silently (X5).

## 4. The `.moirai-image` marker

```abnf
marker-file = %s"moirai-image 1" LF
              %s"object-format: " ( %s"sha1" / %s"sha256" ) LF
              %s"schema-version: " dec LF
```

- `1` is the image format version ([F01 §9.2]); `object-format` is the destination's object format; `schema-version` is the
  commit's schema version, 1 in format v1 ([F08 §8.1], canonical item 7). Nothing store-local or destination-local is in the
  marker: the store id, the granularity and the anchor-text mode are in the side ref (§14.1) ([AR §5b.3], CB2).
- *(Informative)* [AR §5b.3] shows `schema-version: 3` as an illustration; format v1 writes `1`.
- Import: a tree without the marker, with a version other than 1, with an object format other than the destination's, or
  with a schema version other than 1 is `ImageParse` ([F07 §15]); for a native commit the marker's schema version must also
  equal the `Moirai-Schema` trailer ([F07] open point 18).

## 5. Value text forms

### 5.1 Single-line forms by type

Every value of the closed type set ([F08 §5.1], [F07 §7.1]) has one **single-line form**:

| Type | Single-line form | Notes |
|---|---|---|
| `bool` | `true` or `false` | |
| `int` | `sdec` | the value; never a counter, which is a ledger (§6.5) |
| `f64` | `v-f64` (§2.8) | |
| `enum` | the value's name (`vname`) | never its integer, which is store-local for project values ([F08 §8.3]); `priority` is `P0`…`P4` |
| `text`, `sym` | `sval` (§2.5) | the two stored forms of one text ([F07 §7.1]); a text that contains LF is written in a block (§5.4) in a `field` line and as its JSON string elsewhere |
| `ref` | the referenced node's `uid` | never `#N` (N4) |
| `commitref` | `commit-id` | the full id ([F07 §8.1]; open point 13) |
| `path` | `sval` of the path's text form (§5.2) | |
| `oid` | `oid-text` | `sha1:<40 hex>` or `sha256:<64 hex>` |
| `pathmove` | the JSON array of §5.2 | |
| `set` | §5.3 | |

The empty text, the empty set and an `oid` of algorithm `none` are absent values ([F08 §5.3], [F08 §6.2], [F07 §7.1]) and
are never written as values; a line that would carry one is omitted. No store holds an explicit empty set (pass 1, A1-1:
[F06 §5] cites [F08]'s one encoding), so a writer never writes `[]`; an importer still reads `[]`, like every empty value,
as absent, which is the canonical value of an empty set ([F07 §7.1]) (open point 14).

### 5.2 Paths and the root by name

[40] R-1 as revised by review S-19 requires the root of every `path` and `pathmove` value to be carried by name.

- **Implied root.** An artifact's `path`, `origin_path` and every element of its `aliases`, and the `from` and `to` of every
  entry of a root node's `path_moves`, have the node's `root` field as their root (I-F8, [F08 §8.6] rule 3). Their text form
  is the path text alone ([40 §5.7]): `crates/engine/src/lock.rs`, `docs/plan/`.
- **Explicit root.** Every other `path` value — `run.script_path`, `run.journal_path`, `lane.worktree_path`,
  `lane.target_dir`, and every project field of type `path` or `set` of `path` — is written `<rootname>:<path text>`
  (`abs:D:/work/l5np`, `project:docs/a.md`). The first `:` ends the root name, since a root name holds no `:`
  ([F08 §5.4.1]).
- The path text is the stored exact bytes ([F08 §5.4.1]), never folded; the importer checks the path rules of I-F8
  ([F18 §2.8]) and refuses a violation (§9.2).

A `pathmove` value ([F08 §5.2]) is a JSON array of five JSON strings with no white space:

```abnf
v-pathmove  = "[" jstring "," jstring "," jstring "," jstring "," jstring "]"
```

The strings are, in order: the `hlc` as exactly 20 decimal digits, zero-padded, so that bytewise order is numeric order
([40 §5.7]); the class (`explicit`, `confirmed`, `committed` or `observed`); `from` and `to` in the text form above (implied
root inside a root node's `path_moves`, explicit root elsewhere), each ending in `/`; the `git` commit as `oid-text`, or the
empty string when it is empty.

### 5.3 Sets

```abnf
v-set       = "[" [ elem *( "," SP elem ) ] "]"
elem        = ebare / jstring
ebare       = efirst [ *ech evis ]
evis        = %x21-2B / %x2D-5A / %x5C / %x5E-7E / u2vis / u3 / u4       ; vis but not "," "[" "]"
efirst      = %x21 / %x23-2B / %x2D-5A / %x5C / %x5E-7E / u2vis / u3 / u4   ; evis but not DQUOTE
ech         = SP / evis
```

- An element is its single-line form (§5.1). A text or path element is written bare when that form contains no `,`, `[`, `]`
  or control character, has no leading or trailing SP and does not begin with `"`; otherwise as the JSON string of the text.
  Elements of every other type (`int`, `enum`, `ref`, `commitref`, `oid`) never need escaping.
- Elements are sorted bytewise by their written form, with no two equal ([AR §5b.2] rule 4). The canonical form orders set
  elements by their canonical encodings instead ([F07 §2.4]); the importer re-sorts ([F07] open point 25).
- A set of `pathmove` is always written in the block form (§5.4) and never as `v-set`.

### 5.4 Block form

A `field` line whose value is a text containing LF, or a set of `pathmove`, is written as a block ([AR §5b.2] rule 4,
[40 §5.7]):

```abnf
block-open  = %s": <<" LF
text-block  = 2*( SP SP *lch LF ) %s">>" LF          ; a text: one line per line of the value
pm-block    = 1*( SP SP v-pathmove LF ) %s">>" LF    ; a set of pathmove: one entry per line
```

- **Text.** The value V (which contains at least one LF) is split at every LF into lines l1 … lm (m ≥ 2); each is written as
  two SP, its bytes and LF; then `>>` and LF. A value ending in LF gives a last line of two SP only. The decoder collects the
  lines up to the first line that is exactly `>>`, removes exactly two leading SP from each and joins them with LF. No line
  of a block equals `>>`, since every inner line begins with two SP. Lines inside a block are byte-exact and are exempt
  from the trailing-whitespace rule ([AR §5b.2] rule 1). A text holds no CR ([F08 §5.3]) and no U+0000.
- **`pathmove` sets.** One entry per line, the lines sorted bytewise, as §5.3 sorts every set by its written form, with
  no two equal. The `hlc` string has a fixed width, so the entries are in `hlc` order first ([40 §5.7]: sorted by
  (hlc, from, to)); entries of one `hlc` follow the bytes of the rest of the array, which puts the class before `from`.
  This is the image's order only: the stored order is [F08 §5.5]'s and the canonical order [F07 §2.4]'s, and the importer
  re-sorts (§5.3) (spec sync 2b).

### 5.5 Tokens

Inside a property list (`k=v` separated by SP) and in a conflict side (§6.8), a value is a **token** (§2.5): a text-typed
value (text, `sym`, `path`) is its bare token or the JSON string of its text; a value of another type is its single-line
form, written as a token when it has no SP and does not begin with `"`, `%`, `[` or `{` (every `bool`, `int`, `f64`,
`enum`, `ref`, `commitref` and `oid`), and as the JSON string of its single-line form otherwise (a `set`, a `pathmove`). The
importer decodes the token and parses the result by the key's type.

### 5.6 The scope text

The `scope` selector of an anchor is the structured value of [F08 §10.3.1] (a language and one to 64 segments of
item kind, name and qualifier), whose bytes enter `captured` and the selector block ([F07 §8.2]); the anchor record that
[F06 §7.5.3] carries and [F09 §13.3] stores holds those bytes. Its text form, which [40 §5.7] writes, is the bijective
image of those bytes (pass 1, A1-2):

```abnf
scope-text  = lang ":" seg *( "/" seg )
lang        = %s"rust" / %s"markdown" / %s"toml"
seg         = skind SP sname [ "[" squal "]" ]
skind       = %s"mod" / %s"impl" / %s"fn" / %s"struct" / %s"enum" / %s"trait" / %s"const" / %s"static"
            / %s"macro_rules"                                    ; rust
            / %s"h1" / %s"h2" / %s"h3" / %s"h4" / %s"h5" / %s"h6" ; markdown: the heading level
            / %s"table" / %s"array_table" / %s"key"               ; toml
sname       = 1*( schar / pct )
squal       = 1*( schar / pct )
schar       = SP / %x21-24 / %x26-2E / %x30-5A / %x5C / %x5E-7E / u2vis / u3 / u4   ; not "%", "/", "[", "]", control
pct         = "%" 2LHEX
```

- `skind` is [F08 §10.3.1]'s `skind` of the segment's language by name; `sname` is its `name`; `squal` its `qual`, written
  exactly when `qual` is non-empty (Rust `impl Trait for T`: the trait; Markdown: the stripped numbering).
- In `sname` and `squal`, each byte of `%`, `/`, `[`, `]` and of a control character is written as `%` and two lower-case
  hexadecimal digits of the byte; every other byte is written as itself. The mapping between the text and [F08]'s bytes is a
  bijection, so the importer rebuilds the exact bytes that enter `captured`.
- In a property list the scope is a token (§5.5); since every segment holds SP, it is always a JSON string:
  `scope="rust:struct LockFile/impl LockFile/fn acquire"`.
- While [F20 §6.1]'s interim scanner rule holds, no capture records a scope, so a store writes this text only for an
  imported anchor that carries one ([F08 §10.3.1]); the grammar and the bijection are unchanged by the rule.

### 5.7 Anchor texts

`quote`, `prefix`, `suffix` and `end` are the exact bytes of the normalised anchor text N ([F20 §2.5], [F08 §10.3]).
N is a transform of text content, and text content ([F20 §2.1] `is_text`) need not be valid UTF-8. An anchor text is
written:

```abnf
atext       = token / "%" b64url
```

- as a token (§2.5) when its bytes are valid UTF-8; the empty text is `""`;
- otherwise as `%` followed by the base64url (§2.6) of its bytes. A bare token never begins with `%`, so the two forms
  cannot be confused (open point 11).

## 6. Node files

### 6.1 Grammar

```abnf
node-file   = live-file / tomb-file

live-file   = m-node h-uid h-kind [ h-title ] [ h-status ] [ h-resolution ] [ h-priority ] [ h-criticality ]
              [ h-confidence ] [ h-authority ] [ h-parent ] [ h-order ] h-created h-updated [ h-flags ]
              *field-line *label-line *incr-line *edge-line *anchor-line *conflict-line [ body / dropped-body ]

tomb-file   = m-node h-uid h-kind h-title h-deleted [ t-reason ] [ t-replaced ]
              *edge-line *anchor-line *conflict-line

m-node      = %s"moirai-node 1" LF
h-uid       = %s"uid: " uid LF
h-kind      = %s"kind: " iname LF
h-title     = %s"title: " sval LF
h-status    = %s"status: " vname LF
h-resolution = %s"resolution: " vname LF
h-priority  = %s"priority: " vname LF
h-criticality = %s"criticality: " vname LF
h-confidence = %s"confidence: " vname LF
h-authority = %s"authority: " vname LF
h-parent    = %s"parent: " uid LF
h-order     = %s"order: " 1*( DIGIT / ALPHA ) LF
h-created   = %s"created: " prov LF
h-updated   = %s"updated: " prov LF
h-deleted   = %s"deleted: " prov LF
prov        = commit-id [ SP rfc3339ms ]
h-flags     = %s"flags: [" flag *( "," SP flag ) "]" LF
flag        = %s"archived" / %s"frozen" / %s"pinned"
t-reason    = %s"field reason: " sval LF
t-replaced  = %s"field replaced_by: " uid LF

field-line  = %s"field " iname ( ": " fval LF / block-open ( text-block / pm-block ) )
fval        = sval / v-f64 / v-set / v-pathmove     ; parsed by the field's type (§5.1)
label-line  = %s"label " sval LF
incr-line   = %s"incr " iname SP ( "+" / "-" ) NZDIGIT *DIGIT SP ledger-token LF
ledger-token = 1*vis
edge-line   = %s"edge " iname " -> " uid [ %s" pin=" commit-id ] [ %s" flagged" ] LF
anchor-line = %s"anchor " uid " -> " uid a-props LF
conflict-line = %s"conflict " c-key %s" class=" 1*ALPHA %s" base=" [ token ] %s" ours=" [ token ] %s" theirs=" [ token ] LF
body        = %s"---" LF *( bch / LF ) LF
dropped-body = %s"--- dropped " dg16 SP drop-reason LF   ; a dropped body (§6.9; spec sync 3)
drop-reason = %s"secret" / %s"private" / %s"other"       ; [F06 §8.1] DB-2

hkey-line   = hkey ": " *lch LF                     ; the shape of every header line; §9.2 "unknown header key"
hkey        = LALPHA *( LALPHA / DIGIT / "_" )
```

`a-props` is §6.7's, `c-key` §6.8's. A line of the `hkey-line` shape whose `hkey` is none of the header keys (the keys
of the `h-…` productions above, §6.2) and that no other production of the file takes is an **unknown header key**; every
other line that no production takes is a line that matches no production (§9.2; spec sync 2b). The grammar admits every
line an exporter writes, in the exporter's order; the rules of §6.2–§6.11 say which lines a node file holds and which
values they carry. A value that `fval` admits but the field's
type does not is `ImageParse` (§9.2): the parse is by type, with the schema of the commit's own tree ([F08 §8.1]).

### 6.2 Header lines

| Line | Written when | Value | Canonical key ([F07 §6.1]) |
|---|---|---|---|
| `moirai-node 1` | always, first | the node-file format version | — |
| `uid:` | always | the node's uid | the owner of every key of the file |
| `kind:` | always | the kind's name ([F08 §9.1], or a project kind) | existence: `live(kind)` or `deleted(kind, …)` |
| `title:` | a live node whose kind is not `title_derived`; every tombstone | the title ([F08 §7.1]); in an artifact's tombstone its last `path` text ([F08 §3.5]) | field `title` |
| `status:` | every live node, unless a `conflict status` line exists | the status name | status, with `resolution:` |
| `resolution:` | a live node whose resolution is not `none`, unless a `conflict status` line exists | the resolution name | status |
| `priority:`, `criticality:`, `confidence:`, `authority:` | a live node whose value differs from the field's default (`P2`, `normal`, `unset`, `agent`, [F08 §9.2]), unless a `conflict field.<name>` line exists | the value's name | field `priority`, … |
| `parent:` | a live node with a parent, unless a `conflict parent` line exists | the parent's uid | hierarchy |
| `order:` | a live node with an `order` key, unless a `conflict parent` line exists | the fractional index ([F08 §5.4.4]) | hierarchy |
| `created:` | every live node | §6.3 | none (provenance) |
| `updated:` | every live node | §6.3 | none (provenance) |
| `deleted:` | every tombstone | §6.3 | none; the line marks the tombstone form |
| `flags:` | a live node with at least one source-truth flag set ([F08 §3.2]) | the set names, sorted bytewise | fields `archived`, `frozen`, `pinned`; a flag with a `conflict field.<flag>` line is left out of the list |

- `status:` is written at every status, the kind's initial status included, as [40 §5.7]'s examples show (`status: present`,
  `status: active`). The canonical form takes the initial status with resolution `none` as absent ([F07 §6.3]); the
  importer maps it so.
- A default value is omitted and read back as the default ([F07 §6.3]: "a line whose value is the field's default is
  absent"). *(Informative)* [AR §5b.2]'s example writes `criticality: normal`, the default; the corrected example (§17.1)
  omits it.
- Derived state is never written (I36′, [AR §5b.2] rule 6): the derived columns and flags of [F08 §3.2]–§3.4
  (`open_blockers`, `open_blockers_exo`, the rollups `children_total` and `children_done`, `topo`, `suspect`,
  `has_dangling`, `container`, `conflicted`) and the read-time states `ready`, `is_blocker`, `rev_seq`, `claimed`,
  `settled` and `stale`. These fifteen are the **derived-state names**. A `field` line whose name the node's kind does not
  have is a derived field (a line not allowed, §9.2) when the name is a derived-state name, and an unknown field otherwise
  (spec sync 2b). There is no `id:` line (N4).

### 6.3 Provenance lines: `created`, `updated`, `deleted`

Each is a commit id, then SP and the time text of that commit's `hlc` (§2.7) when the instant precedes year 10000
([AR §5b.2] rule 5). They carry no canonical item; the importer uses only `created`'s commit id, for `IdCollision`
([AR §5b.6] step 4). Their values are functions of the commit graph, so a node file is a pure function of moirai data
(§3.3). For node u and moirai commit C:

- **created(u, C)** is the commit K of H(C) with the least (`gen`, commit id), compared as (`u32`, bytes), such that u exists
  in CS(K) and not in CS(P1(K)). For a node created once, K holds its `Create`; for a derived uid created on two lines, K is
  [40 §2.3]'s "creating commit with the least (generation, commit id) among the creating commits the view's history holds".
- **updated(u, C)** is C when u has no node file at P1(C), or when u's node files at C and at P1(C), each read without its
  `updated:` line, differ; otherwise updated(u, P1(C)). The `updated:` line therefore changes exactly when the rest of the
  file does.
- **deleted(u, C)**, for u deleted in CS(C): deleted(u, P1(C)) when u is deleted in CS(P1(C)); otherwise deleted(u, P2(C))
  when C has a second parent and u is deleted in CS(P2(C)); otherwise C.
- **Import-checkpoint override.** When the recursion reaches an import-checkpoint commit K at which u's node file came in,
  the value is the one K's import recorded for u (§11.3), with its time text as written. This keeps a store that imported a
  checkpoint image able to reproduce its trees byte for byte (gate 2).

### 6.4 Field and label lines

Every field key of the live node ([F07 §6.2]) that holds a non-absent plain value, and that no header line of §6.2
carries, is one `field` line, except `labels`:

| Field ([F08 §9.2]–§9.3) | Line |
|---|---|
| `labels` (set of `sym`) | one `label <text>` line per element, sorted bytewise by the written text |
| a counter field (`reopen_count`, `incidents`, every project field of type `counter`) | ledger lines (§6.5), never a `field` line |
| `body` | the body section (§6.9) |
| every other field whose storage is `field` or `cold`, except `order` (a header line): `abstract`, `defer_until`, `due`, `reason`, every kind field, every project field | `field <name>: <value>` |

- `field` lines are sorted bytewise by field name ([F07] open point 6), not by whole-line bytes, so `field a:` precedes
  `field a0:`.
- `defer_until`, `due` and the other `int` fields with `coerce = timestamp` are written as decimal Unix seconds, their stored
  type ([F08 §4], [F08 §8.4.4]); the RFC 3339 form of [AR §5b.2] rule 4 is used only in the provenance lines (open point 6).
- Record lists (`doc.targets`, `doc.readiness`, `decision.alternatives`, `question.options`, [F08 §5.4.5]) are texts: a list
  of one record is a single-line value, a JSON string when the record holds HT between its members; a list of several
  records holds LF and is a block, whose lines keep their HT bytes.
- A field of class `observation` is omitted while a `conflict observation` line exists ([F07 §6.5]).
- A field with a `conflict field.<name>` line is omitted (N12).

### 6.5 Ledger lines

A counter field is written as **ledger lines** `incr <field> <+k|-k> <token>` ([AR §5b.2] rule 3, N6): the counter's value
is the sum of the signed deltas of its lines, and no `field` line carries it.

- **The ledger.** Let L(u, f, C) be the list of lines of counter f of node u at C:
  - if C is an import-checkpoint commit at which u's node file came in: the lines of that file as imported, verbatim (§11.3);
  - otherwise L(u, f, P1(C)) (empty at a root commit), followed, when item 10 of C has a counter entry for (u, f) with
    delta δ ([F07 §7.5]), by the line (f, `c<id of C>`, δ).

  Every commit on the view's first-parent chain whose item 10 changes the counter adds exactly one line, with the net delta
  against its first parent — for a merge, revert or cherry-pick the delta of its stored `Incr` ([F06 §7.8]), for a `sync`
  the delta of its full diff ([F07 §10.5]) — so the sum of the lines always equals the counter's total and the ledger is
  append-only along a ref (open point 24).
- **Written** for a live node, sorted by (field name bytes, token bytes). No two lines of one field carry the same token.
  A tombstone carries no ledger ([F07 §6.4]); the ledger continues when the node is undeleted.
- **Tokens.** An exporter writes `c<64 hex>`. A foreign edit may write any `ledger-token` in place of it ([AR §5b.2]
  rule 3); the importer only sums the deltas, and the foreign commit's own line in the store's ledger carries the foreign
  commit's id (§11.2).
- **The delta an importer reads.** For each counter (u, f): Σ(deltas of the lines at C) − Σ(deltas of the lines at P1(C)),
  a missing file counting 0 ([F07 §14.2] row 6). A delta of 0 is no entry.
- A delta is written without leading zeros and is never 0. Its magnitude is at most 2^64 − 1; the sum of a file's lines must
  lie in the `i64` range ([F07 §7.5]), else `ImageParse`.

### 6.6 Edge lines

`edge <kind> -> <dst uid> [pin=c<64 hex>] [flagged]` is one out-edge of the file's node that is not `at` and not `parent`
([F07 §6.6]): `parent` is the hierarchy key (`parent:` line), and `at` edges are `anchor` lines (§6.7).

- `<kind>` is the stored edge-kind name ([F08 §9.6]), never the LQ name. A symmetric edge (`contradicts`, `relates`) is
  written in the file of the endpoint whose uid is bytewise smaller ([F08 §10.1]); an importer also reads it from the
  other endpoint's file (§9.1 rule 10).
- `pin=` carries `pinned_commit` for a kind whose `props` is `pinned`, when the edge has a pin; `flagged` is set on a
  retained `blocks` or `gates` out-edge of a tombstone that was neither re-pointed nor resolved (X4; [AR §5b.2] rule 8).
- Lines are sorted by (kind name, dst uid, the rest of the line) bytewise ([AR §5b.2] rule 3). An edge with a
  `conflict edge.<kind>.<dst>` line is omitted.
- `mentions` edges are written like every other edge; an importer takes them from these lines only and never re-parses
  imported text ([AR §5b.6] step 5, CL2).

### 6.7 Anchor lines (R-11)

An `at` edge with its anchors is written as one `anchor` line per anchor, in the referrer's file ([40 §5.7], R-11); no
`edge at` line exists, since the adjacency is implied.

```abnf
a-props     = %s" kind=" akind %s" mode=" ( %s"live" / %s"pinned" ) %s" watch=" ( %s"header" / %s"span" )
              [ %s" scope=" token ]
              [ %s" quote_h=" dg16 %s" prefix_h=" dg16 %s" suffix_h=" dg16 ]
              [ %s" end_h=" dg16 ]
              [ %s" quote=" atext %s" prefix=" atext %s" suffix=" atext ]
              [ %s" end=" atext ]
              [ %s" occurrence=" dec ]
              [ %s" hint=" dec "-" dec ]
              [ %s" window=" b64url ]
              [ %s" span=xxh3:" 16LHEX ]
              [ %s" blob=" oid-text ]
              [ %s" git=" oid-text ]
              %s" captured=" dg16
              [ %s" pred=" dg16 ]
              [ %s" marker=" token ]
              %s" v=" dec
akind       = %s"file" / %s"heading" / %s"symbol" / %s"quote" / %s"range" / %s"lines"
```

`anchor <anchor uid> -> <dst uid>`: the anchor uid is the edge key's discriminator ([40] R-4, [F07 §6.6]); the dst uid is
the file node's. Lines are sorted by (dst uid, anchor uid) ([40 §5.7]). The properties are written in the order above, which
is the order of the selector block ([F07 §8.2]) with the four texts after the digests:

| Property | Written when | Value | Selector field ([F07 §8.2]) |
|---|---|---|---|
| `kind`, `mode`, `watch` | always | names ([40 §2.7]) | 1–3 |
| `scope` | the anchor has a scope | the scope text (§5.6) as a token | 4 |
| `quote_h`, `prefix_h`, `suffix_h` | `kind` is `heading`, `symbol`, `quote` or `range` — on **every** such line, in both modes | BLAKE3-128 of `quote.exact`, `prefix.exact` (as widened) and `suffix.exact` (as widened); an empty prefix or suffix has the digest of the empty string | 5–7 |
| `end_h` | `kind` is `range` — on every such line | BLAKE3-128 of `end.exact` (S-04) | 8 |
| `quote`, `prefix`, `suffix` | `quote_h` is written, the destination's mode is `full`, and the store holds the texts | the texts (§5.7) | not hashed |
| `end` | `end_h` is written, and as for `quote` | the text | not hashed |
| `occurrence` | the anchor has an occurrence | 1–65,535 | 9 |
| `hint` | the anchor has a hint | `first-last`, 1-based, inclusive, first ≤ last; a one-line hint is `n-n` | 10 |
| `window` | the anchor has a window | base64url of the window value W ([F20 §2.7.3]) | 11 |
| `span` | the anchor has a span hash (every kind but `file`) | `xxh3:` and the 16 lower-case hexadecimal digits of the XXH3-64 value, most significant digit first (the value, not its stored bytes, [F01 §7.2]) | 12 |
| `blob` | the anchor's `blob` is not empty | `oid-text` | 13–14 |
| `git` | the anchor has an observed git commit | `oid-text` | 15–16 |
| `captured` | always | the capture digest | 17 |
| `pred` | the anchor has a predecessor term (S-03) | the 16-byte term | 18 |
| `marker` | the anchor has an in-file marker | the marker id as a token | 19 |
| `v` | always | the anchor's `resolver`: the resolver version at capture, decimal, ≥ 1 (1 in format v1, [F20]) | 20 |

- **Digests on every line** (R-11, [72 M6]). The digests are the carriers of the selector block ([F07 §8.2]); the texts are
  a verified cache of their preimages. Both modes therefore give one canonical form and one commit id ([F07 §8.3]).
- **`full` mode** (the default, [CFG §10.10] `image.dest.<name>.anchor-text`) writes the texts where the store holds them.
  The importer verifies each text against its digest; a mismatch is `ImageParse`. The texts are all present or all absent
  on a line: `quote`, `prefix` and `suffix` come together, and `end` comes with them on a `range` line.
- **`hash-only` mode** writes no text. An anchor that arrives without its texts — from a `hash-only` destination, or from
  a store that itself holds the anchor without its texts — is imported in the **`text-unavailable`** sub-state ([40 §5.7];
  [F08 §10.3] `aflags` bit 5 `text_unavailable`; the detail string is [F18 §4.6]'s): it resolves by hint,
  window and scope only, never `fresh` by quote, until `links fix --repin --at …` recaptures it. The sub-state covers `end`.
- **`v=`** is the anchor's resolver version, a selector field hashed with the block; it is not the version of the line format,
  which is the file's `moirai-node 1` (open point 1).
- Presence follows the stored record, which [F08 §10.3] alone lays out ([F06 §7.5.3] cites it): `hint`, `window` and
  `span` are present exactly when `kind ≠ file`, and `blob` may be `none` only for a planned `file` anchor; the line
  writes whatever the record holds, and the importer rebuilds the same record (open point 12). I-F9 requires
  a quote on `heading`, `symbol`, `quote` and `range` anchors and a window on `lines` anchors ([F18 §2.9]); a line that breaks
  it is `ImageParse`.
- An anchor with a `conflict edge.at.<dst>.<anchor>` line is omitted. An anchor line whose dst uid names no node of the
  importing view is a tombstone reference: `at` is historical ([40 §5.7], [AR §5b.6] step 4).
- The anchor uid, `captured` and `pred` let the importer check the derivation of [F08 §11.4] (§9.3); `captured` itself is
  trusted as stored ([F08 §11.5]).

### 6.8 Conflict lines

`conflict <key> class=<class> base=<side> ours=<side> theirs=<side>` holds one conflict value `{class, base, ours,
theirs}` ([F07 §6.5], [F06 §6.2]). While it exists, the ordinary line or lines of the key are omitted (N12).

```abnf
c-key       = %s"field." iname / %s"status" / %s"body" / %s"parent" / %s"existence" / %s"observation"
            / %s"edge." iname "." uid [ "." uid ]
dropped-tok = %s"dropped:" dg16 ":" drop-reason      ; a dropped body as a side (§6.1; spec sync 3)
```

| Key | Canonical key ([F07 §6.1]) | A side is | Side token |
|---|---|---|---|
| `field.<name>` | field (uid, name), every storage: header (`field.priority`), flag (`field.pinned`), cold, field, and title (`field.title`) | the field's value | its token (§5.5) |
| `status` | status | (status, resolution) | `<status>` or `<status>/<resolution>` when the resolution is not `none` |
| `parent` | hierarchy | (parent uid, order) | `<uid or ->,<order or ->`, for example `018f…9e09,a0V` or `-,a1` |
| `body` | body | the body | the JSON string of the body's bytes, always ([AR §5b.2] rule 3); a body the exporting store has dropped is the bare token `dropped-tok`, `dropped:<hash>:<reason>` (spec sync 3) |
| `edge.<kind>.<dst>` | edge (uid, kind, dst, empty disc) | `present(props)` | `present`, `pin=c<64 hex>` or `flagged` |
| `edge.at.<dst>.<anchor>` | edge (uid, `at`, dst, anchor uid) | an anchor | the JSON string of the anchor line's properties, `kind=…` through `v=…`, as §6.7 writes them on the line: the texts included exactly where §6.7 writes them (the destination's mode is `full` and the store holds them); the importer verifies them against the digests as on a line and hashes only the selector block |
| `observation` | observation (class 5) | the six observation fields ([40 §2.2]) | the JSON string of the side's `field` lines for `path`, `oid`, `bytes`, `observed_git`, `observed_blob` and `relink`, in that order, absent members left out, joined by LF; the empty side is the absent composite, since a present one always holds `path` (a live artifact's `path` is required, [F08 §8.6] rule 1) |
| `existence` | existence | an existence value | the JSON string of §6.8.1's text |

- **Absent** is the empty side: nothing between `=` and the SP that follows, or the end of the line
  (`base= ours=5 theirs=7`). No non-absent side is empty: the empty text and the empty set are absent values (§5.1). A
  side that holds a field's default is absent and written empty ([F07 §6.3]); a `status` side is written by name even at
  the kind's initial status with resolution `none`, as the `status:` line is (§6.2), and is read as absent. A writer
  therefore never writes an empty `status` side; an importer reads one as absent (§9.1 rule 7; spec sync 2b).
- `<class>` is a value-conflict class name of [F12 §6.1] ([F19 §12.1], [F07 §2.2]): `FieldEdit`, `StatusFork`, `TextHunk`,
  `DeleteVsModify`, `SupersedeFork`, `OwnerFieldEdited`, `PathClaim`. Another name, `DATA` included (no store holds that
  class, [F12 §6.1]; pass 1, S1-32, A1-43), is `ImageParse`.
- Lines are sorted bytewise by key. Sides are plain values ([F07 §6.5]).
- A body conflict omits the `---` section ([AR §5b.2] rule 3). Its sides are the body texts; the canonical sides are their
  BLAKE3-128 hashes, which the importer computes, and it stores the texts as bodies ([F06 §8] BD-4).
- **A dropped side** (spec sync 3; [F06 §8.1] DB-11). A body side whose body the exporting store has dropped is written
  `dropped:<hash>:<reason>`: the body key's BLAKE3-128 in lower-case hexadecimal and the reason's name ([F06 §8.1] DB-2),
  for example `base=dropped:5f0c…e1a2:secret`. It cannot be confused with a body text, which is always a JSON string
  and so begins with `"`. Its canonical side is that hash, and the importer stores no bytes for it (§11.2). The other
  sides keep their texts; a `TextHunk` with a dropped side is what a merge stages when its text rule needs a dropped
  body's lines ([F06 §8.1] DB-7 (d), [F12 §7.5]). In a body side a bare token that is not a `dropped-tok` does not parse
  by the key's type (§9.2).
- `observation` is the key text for the observation composite that [AR §5b.2] rule 3 lacks ([F07] open point 8); it carries
  `FieldEdit` and `PathClaim` ([RULES/link-merge-rules] PC-002). While it exists the six member `field` lines are omitted
  ([F07 §6.5]).
- `field.title` is the key of a title conflict ([RULES/merge-table] open point 11); the `title:` line is then omitted.
- `edge.at.<dst>.<anchor>` is the key of an anchor-selector conflict ([40 §5.5] "both repinned differently"); a
  `SupersedeFork` sits on an `edge.supersedes.<dst>` key ([RULES/merge-table] open point 9).

#### 6.8.1 Existence sides

An existence side is one of the values of [F07 §7.2] class 1. Its text, lines joined by LF with no final LF, is:

```abnf
ex-text     = ex-live / ex-deleted
ex-live     = %s"live " iname *( LF snap-line )
ex-deleted  = %s"deleted " iname [ LF %s"reason: " sval ] [ LF %s"replaced_by: " uid ]
snap-line   = %s"status: " vname [ "/" vname ]
            / %s"title: " sval
            / %s"field " iname ": " fval-1
            / %s"label " sval
            / %s"total " iname SP sdec
            / %s"body " ( jstring / dropped-tok )
fval-1      = sval / v-f64 / v-set / v-pathmove     ; single-line forms only: a multi-line text is its JSON string
```

- `live <kind>` carries the side's node image ([F07 §7.4], [F06 §6.3]): its status line (always, with `/<resolution>` when
  not `none`; the initial status with `none` is read as absent, as in §6.2), then its title, header, flag and field keys as `title:` and `field <name>:` lines (the header enumerations and
  flags by their field names: `field priority: P1`, `field pinned: true`), sorted by field name, the `title:` line under
  the name `title` (`field priority: P1`, `title: …`, `field work_kind: impl`; spec sync 2b), then `label` lines, then one
  `total <field> <value>` line per non-zero counter (the node image holds totals, not ledgers), then the body as
  `body <JSON string of its bytes>`, or, for a body the exporting store has dropped, `body dropped:<hash>:<reason>`
  (§6.8 "A dropped side"; spec sync 3). The image holds no hierarchy or edge key ([F06 §6.3]): a `--take` towards the live
  side restores them from that side's state with ordinary ops ([F12 §6.5]).
- `deleted <kind>` carries the tombstone's reason and replacement.
- This settles [RULES/merge-table] open point 5 (d) and [F06] open point 13: a provisionally deleted node is a tombstone
  file carrying its `conflict existence` line, and a provisionally live node is a live file carrying it.
- **`prov`** ([F07 §7.3], [F12 §6.3]) has no text of its own: the file's form carries it. It names the side, `ours` or
  `theirs`, whose value has the file's form (`deleted` in a tombstone file, `live` in a live file), and `ours` when both
  sides or neither have it ([RULES/merge-table] RS-010: "o's values otherwise"). The exporter writes the form of the
  provisional value (a tombstone file when it is `deleted`, [F12 §6.3]), so the two agree; §12.2 lists this carrier
  (spec sync 2b).

### 6.9 The body

A body is written after a line `---` as **its bytes followed by exactly one LF**; the decoder takes every byte after the
`---` line and strips exactly one final LF ([AR §5b.2] rule 7, CM3). A body that ends in LF gives a file ending in two LF;
the file never ends without LF. Bodies are byte-exact: hard breaks, inner `---` lines and several final LF survive, and
BLAKE3-128 of the decoded bytes equals the body key ([F07 §6.3], I40′). A body is valid UTF-8 without CR ([F08 §7.2]); it may
contain U+0000.

- A node without a body has no `---` line. An empty body is no body ([F08 §7.2]; pass 1, S1-34), so a native file never
  ends in the `---` line and the one LF an empty body would give; an importer reads that form as no body.
- The only normalisation a body receives is the store's CR LF → LF at write time; an importer applies the same rule to a
  git-side file that has CR LF ([AR §5b.2] rule 7) and never touches the bytes otherwise.
- **A dropped body** ([F06 §8.1] DB-6, DB-11; [AR §11] #33, OQ-A-7; spec sync 3). When the exporting store has dropped
  the node's body, the file ends with the one line `--- dropped <hash> <reason>` (`dropped-body`, §6.1) in place of the
  `---` line and the bytes: `<hash>` is the body key, the BLAKE3-128 of the dropped bytes in lower-case hexadecimal, and
  `<reason>` the drop's reason (`secret`, `private` or `other`, [F06 §8.1] DB-2), for example
  `--- dropped 5f0c…e1a2 secret`. The line carries the body key exactly as a body section does: the importer takes the
  hash as the body key without hashing anything (§11.1), so a native commit's `Moirai-Commit` still verifies. The file
  ends in that line's LF. A body section begins with a line that is exactly `---`, so the two forms cannot be confused,
  and a node has at most one of them: a `--- dropped` line followed by a body section matches no production (§9.2),
  while the same text after a body's `---` line is part of the body. Only a live file can hold the line, since a
  tombstone has no body (§6.10).

### 6.10 Tombstone files

A tombstone file carries the deleted node's canonical state ([F07 §6.4], [AR §5b.2] rule 8, I39′) and nothing else:

| Line | Value | Canonical key |
|---|---|---|
| `uid:`, `kind:` | the node and its kind at deletion | existence `deleted(kind, reason, replaced_by)` |
| `title:` | the title kept at deletion; for an artifact its last `path` text | field `title` |
| `deleted:` | §6.3 | none; marks the tombstone form |
| `field reason:` | the deletion's reason text, `image:file-removed` for a foreign removal; omitted when empty | existence (not the field `reason`) |
| `field replaced_by:` | the replacement's uid; omitted when none | existence (not the field `replaced_by`) |
| `edge` lines | the retained out-edges: flagged `blocks` and `gates` edges with `flagged`, historical edges with their props | edge |
| `anchor` lines | the retained `at` edges with their anchors | edge |
| `conflict` lines | an `existence` conflict (§6.8.1), a conflict on a retained edge | as §6.8 |

A tombstone file has no `created:`, `updated:`, status, header-enumeration, flag, field, label or ledger line and no body.
In a tombstone file the `field reason:` and `field replaced_by:` lines carry the existence value; in a live file the fields
`reason` and `replaced_by` are ordinary field keys ([F07] open point 23). The `deleted:` line tells the forms apart.

### 6.11 File nodes and root nodes (R4)

A **file node** is a live or deleted `artifact` ([40 §5.7], R-2, [F08 §9.3] artifact):

- no `title:` line while live: the title is derived from `path` ([40 §2.2]);
- `status:` `planned`, `present` or `removed` ([F08 §9.5]);
- `field` lines, sorted by name: `aliases` (a set of paths, implied root), `artifact_kind`, `bytes`, `excerpt`,
  `observed_blob`, `observed_git`, `oid`, `origin_path` (implied root), `origin_pred` (a uid, only when the derivation had
  a predecessor, [40 §5.7]), `path` (implied root), `reason`, `relink` (the closed grammar of R-17, [F18 §5.1]),
  `replaced_by`, `root` (the root name);
- no line carries machine-local evidence ([40 §5.7] "Never exported", I-F4).

A **root node** is an `area` whose `root` field is present ([F08 §11.3]): `kind: area`, `title: root:<name>`,
`status: active`, `field path_moves: <<` with one `pathmove` entry per line (§5.2, §5.4), and `field root: <name>`. Its
`path_moves` is an ordinary set field in the tree diff: R4 adds no trailer and no commit annotation ([40] R-5, [41 B4]).

### 6.12 Line groups and canonical key classes

The line order of a node file, with the canonical key class each line carries ([F07 §6.1]); [F07 §10.3] orders item-10
entries inside one uid by class code, and an importer that reads a checkpoint in tree order buffers one node file at a time
([F07 §6.1]):

| Order | Lines | Key class (code) | Hashed |
|---|---|---|---|
| 1 | `moirai-node 1`, `uid:` | — (the uid is the owner) | — |
| 2 | `kind:` | existence (1) | yes |
| 3 | `title:` | field (4) | yes |
| 4 | `status:`, `resolution:` | status (2) | yes |
| 5 | `priority:`, `criticality:`, `confidence:`, `authority:` | field (4) | yes |
| 6 | `parent:`, `order:` | hierarchy (3) | yes |
| 7 | `created:`, `updated:`, `deleted:` | none (`deleted:` marks the tombstone form) | no |
| 8 | `flags:` | field (4) | yes |
| 9 | `field` lines; a tombstone's `field reason:` and `field replaced_by:` | field (4); existence (1) in a tombstone | yes |
| 10 | `label` lines | field `labels` (4) | yes |
| 11 | `incr` lines | counter (6), as a delta | the delta only; the tokens are not |
| 12 | `edge` lines | edge (7) | yes |
| 13 | `anchor` lines | edge (7), kind `at` | yes, except the four texts |
| 14 | `conflict` lines | the named key, including observation (5) | yes |
| 15 | `---` and the body, or the `--- dropped` line (§6.9) | body (8) | its BLAKE3-128; for the dropped line its hash, not the reason |

## 7. Schema files

### 7.1 Tables

The view's schema items ([F08 §8.1]; [F07 §9]) are written; the core schema of schema version 1 is not, since item 7
identifies it ([F07 §9]). Each item class has its file ([AR §5b.1], [AR §2.12]): kinds in `schema/kinds.moi`, fields and
enumeration values in `schema/fields.moi` (the "fields with type and lattice order" of [AR §2.12]), edge kinds in
`schema/edges.moi`, policy rows in `schema/policy.moi` ([F08 §8.5.6]; spec sync 2b). Every value that [F08 §8.5]
marks store-local (kind ids, edge ids, enumeration integers) is left out; every reference is by name.

```abnf
schema-file = %s"moirai-schema 1" LF *schema-row
schema-row  = kind-row / field-row / value-row / edge-row / policy-row

policy-row  = %s"policy " pname SP token LF   ; token: the row's canonical value ([CFG §4.1]) as a §5.5 token
pname       = pseg *( "." pseg )             ; a [CFG §10.13] row instance in [CFG §3.3]'s canonical (lower-case)
                                             ; key-name form: at most 16 segments and 255 bytes
pseg        = ( LALPHA / DIGIT ) *63( LALPHA / DIGIT / "-" / "_" )

kind-row    = %s"kind " iname
              %s" derivation=" ( %s"random" / %s"file-key" / %s"root-key" )
              %s" root-variant=" ( %s"none" / %s"root-key" )
              %s" existence=" ( %s"delete-wins" / %s"resurrect" / %s"none" )
              [ %s" flags=" names ] [ %s" retired" ] LF

field-row   = %s"field " iname SP iname
              %s" type=" tname [ %s" elem=" tname ]
              %s" class=" mclass %s" storage=" storage %s" decl=" dec %s" optional=" ( %s"true" / %s"false" )
              %s" index=" ( %s"none" / %s"column" / %s"bitmap" )
              %s" coerce=" ( %s"none" / %s"priority" / %s"revision-integer" / %s"timestamp" )
              [ %s" flags=" names ] [ %s" default=" token ] [ %s" min=" sdec %s" max=" sdec ] [ %s" retired" ] LF

value-row   = %s"value " ( iname / "*" ) SP iname SP vname %s" rank=" dec
              [ %s" flags=" names ] [ %s" covers=" vnames ] [ %s" retired" ] LF

edge-row    = %s"edge " iname
              %s" class=" ( %s"structural" / %s"historical" )
              %s" on-dst=" ondst %s" on-src=" onsrc %s" acyclic=" acyc %s" card=" card %s" max-depth=" dec
              %s" derivation=" ( %s"none" / %s"anchor-key" ) %s" props=" ( %s"none" / %s"pinned" / %s"flagged" / %s"anchor" )
              [ %s" flags=" names ] %s" lq=" lqname %s" src=" kset %s" dst=" kset [ %s" reverse=" lqnames ]
              %s" reading=" token [ %s" retired" ] LF

tname       = %s"bool" / %s"int" / %s"counter" / %s"f64" / %s"enum" / %s"text" / %s"sym" / %s"set" / %s"ref"
            / %s"commitref" / %s"path" / %s"oid" / %s"pathmove"
mclass      = %s"none" / %s"scalar" / %s"owner" / %s"authority" / %s"status" / %s"counter" / %s"set" / %s"text"
            / %s"section-text" / %s"hierarchy" / %s"identity" / %s"observation" / %s"alias-set" / %s"glob-set"
            / %s"pathmove-set" / %s"derived"
storage     = %s"header" / %s"flag" / %s"cold" / %s"field" / %s"title" / %s"body"
ondst       = %s"restrict" / %s"restrict-cascade-reparent" / %s"restrict-reassign" / %s"restrict-repoint" / %s"drop"
            / %s"drop-notify" / %s"drop-src-suspect" / %s"tombstone" / %s"tombstone-src-suspect"
onsrc       = %s"drop" / %s"drop-rollups" / %s"repoint-or-flag" / %s"drop-reopen" / %s"retain-warn" / %s"retain"
            / %s"recompute" / %s"retain-anchors"
acyc        = %s"none" / %s"forest" / %s"precedence" / %s"dag" / %s"by-construction"
card        = %s"many" / %s"max-1-per-src" / %s"max-1-active-per-dst" / %s"chain-1" / %s"typical-1" / %s"anchors-min-1"
names       = iname *( "," iname )
vnames      = vname *( "," vname )
lqnames     = lqname *( "," lqname )
kset        = "*" / iname *( "," iname )
```

| Row | Item ([F08 §8.5]; canonical [F07 §9]) | Rules |
|---|---|---|
| `kind` | kind item | `flags` lists the set `kflags` of `title_derived`, `immutable_fields`, `has_done`, `done_derived` |
| `field <kind> <field>` | field item | `type` and `elem` by [F08 §5.1]'s names, `text` and `sym` distinct ([F07 §9.3]); `elem` only for `set`; `flags` lists `one_line`, `ascii`; `default=` exactly when `has_default`, as a token of the field's type (§5.5); `min=` and `max=` exactly when `has_range`. A project field always names its kind ([F08 §8.5.2]) |
| `value <kind or *> <field> <value>` | enumeration-value item | `rank` is `sort_rank`; `flags` lists `side`, `done`; `covers` lists the covered values by name |
| `edge` | edge-kind item with [50] F1 | `flags` lists `symmetric`, `same_kind`; `src`/`dst` are the `KindSet` (`*` for `any`, else kind names); `reverse` the reverse names; `reading` the reading template as a token |
| `policy <name> <value>` | policy row ([F08 §8.5.6]; spec sync 2b) | in `schema/policy.moi`; a row equal to its default is never written |

- Every list (`flags`, `covers`, `src`, `dst`, `reverse`) is sorted bytewise and omitted when empty, except `src` and `dst`,
  which are always written (`*` or at least one name).
- **Order.** `kinds.moi` rows by kind name; `fields.moi` rows by (kind name with `*` first, field name, value name), a field
  row before the value rows of its field (its value name counts as empty); `edges.moi` rows by edge name; `policy.moi`
  rows by row name.
- `retired` marks `iflags.retired` ([F08 §8.1]): a retired item stays in the file.
- A schema item never holds a conflict value in an exported image: a conflicting change to a kind, field, value, edge
  kind or policy row is the structural `SchemaConflict`, which exists only on staging refs ([RULES/merge-table] MR-055,
  MR-056). Only named queries carry value conflicts (§7.2.4).

### 7.2 Named-query files (F3)

#### 7.2.1 The name and the file name

- **The name's bytes** are the query name's **canonical spelling**: its `qname` segments ([LQ/grammar-v1.ebnf]) joined by
  `.`, each segment written as itself when it matches `[A-Za-z_][A-Za-z0-9_]*` and otherwise back-quoted, with every `` ` ``
  inside doubled ([LQ/lexical §5.4]). Names are exact, case-sensitive bytes ([LQ/lexical §9]); `Foo` and `foo` are two
  queries in two files.
- **The file name** is `schema/queries/<q>.moi`, q = the first 32 lower-case hexadecimal digits of BLAKE3-256 of the name's
  bytes ([50 §4.4], [50] F3, [80] X-F9, [F01 §7.1]). A definition whose q equals the q of another query of the view is
  refused at `DEFINE` (exit 2, [F19]); for 128-bit prefixes this is a hash collision, and the refusal keeps one file per
  name (X5; open point 28).

#### 7.2.2 Grammar

```abnf
query-file  = %s"moirai-query 1" LF
              %s"name: " sval LF
              ( query-def / query-conflict )

query-def   = %s"lq: " dec LF
              [ %s"params: " sval LF ]
              %s"shape: " vname LF
              %s"budget: " vname LF
              %s"---" LF
              query-text LF

query-text  = 1*( lch / LF )                          ; the portable text, byte-exact

query-conflict = %s"conflict definition class=" 1*ALPHA
                 %s" base=" [ jstring ] %s" ours=" [ jstring ] %s" theirs=" [ jstring ] LF
```

| Line | Value | Canonical ([F07 §9.6]) |
|---|---|---|
| `name:` | the name's canonical spelling (§7.2.1) | the schema key |
| `lq:` | the LQ grammar version of the text, ≥ 1 ([F08 §8.5.5] `lq_version`) | `lq_version` |
| `params:` | the parameter signature (§7.2.3); the line is absent when the query has no parameter | `params`; empty when absent |
| `shape:` | the shape word ([LQ/std §2.3]) | `shape` |
| `budget:` | the budget-class word ([LQ/std §2.4]) | `budget` |
| after `---` | the stored text: the whole `define_stmt` in portable form ([LQ/canonical-ast §8.1], [LQ/lexical §10.2]) | `text` |

- The text follows the body rule (§6.9): its bytes and one LF, the decoder stripping one LF ([50 §4.4]: "LF only,
  byte-exact"). The stored normal form has no CR, no BOM and no SP or HT before an LF or at the end ([LQ/lexical §10.2]
  condition 1).
- **Consistency.** The text parses with start symbol `define_stmt` ([LQ/grammar-v1.ebnf]); its `qname` has the file's
  name, its `param_decl` list renders to the `params:` value by §7.2.3, its `SHAPE` word equals `shape:` and its `BUDGET`
  word equals `budget:` (compared ASCII-case-insensitively, [LQ/lexical §9]; written as the item stores them). A mismatch is
  `ImageParse`. When the text has no `SHAPE` or `BUDGET`, the item's stored word is written: `table` for the shape
  ([LQ/std §2.3]) and `medium` for the budget ([LQ/std §2.4]); a `shape:` or `budget:` line with another word is then a
  mismatch (spec sync 2b).
- **Portability** ([50 §4.4], S-02). The exporter asserts, and the importer checks, [LQ/lexical §10.2]: the stored normal form,
  and — by **re-binding** the definition against the schema of the commit that carries it, never by a character pattern —
  no node-typed constant other than a `#u:` literal, no revision-typed constant whose base is a sequence number or a commit
  prefix, no reflog revision and no anchor handle. A failure is `ImageParse`. The canonical-AST hash is recomputed on import
  and never exported ([50] F3).
- The display of `#u:<uid>` as a local `#N` is a rendering ([50 §4.4]); the file holds `#u:` literals.

#### 7.2.3 The parameter signature

The `params` value — [F08 §8.5.5]'s hashed `params` and the `params:` line — is the rendering of the `define_stmt`'s
`param_decl` list:

```abnf
sig         = decl *( "," SP decl )
decl        = param ":" SP ptype [ "?" ] [ SP "=" SP default ]
ptype       = ident-word [ "<" ident-word ">" ]
ident-word  = ( ALPHA / "_" ) *( ALPHA / DIGIT / "_" )
param       = "$" ident-word
default     = <the default's token, byte-exact from the portable text>
```

- Parameters are in declaration order. The type is written as in the text with no space inside (`list<node>`); the default
  is the exact bytes of its `literal` or `node_lit` token in the portable text (`20`, `NULL`, `false`, `'c<64 hex>'`,
  `#u:<32 hex>`).
- On the `params:` line the signature is a line value (§2.5): bare in the usual case; a JSON string when it contains a
  control character (a string default holding a raw HT).

#### 7.2.4 Conflicted definitions

A named query holds a conflict value when it was changed on both sides to different definitions (`FieldEdit`) or dropped on
one side and changed on the other (`DeleteVsModify`) ([50 §4.4], [F07 §6.5]). Its file is `moirai-query 1`, the `name:`
line and one `conflict definition` line; it has no `lq:`, `params:`, `shape:`, `budget:` or text. Each side is the JSON
string of the complete `query-def` file of that side (from `moirai-query 1` through the text's final LF), or empty when the
query is absent on that side. The file exists while the conflict exists, whatever the provisional state ([RULES/merge-table]
RS-011).

## 8. Checkpoint ref rows

A checkpoint commit (§10.7) of ref R carries one row file, `refs/heads.moi` for a branch and `refs/tags.moi` for a tag
([AR §5b.1]):

```abnf
refs-file   = %s"moirai-refs 1" LF %s"ref " refname %s" kind=" ( %s"work" / %s"plan" / %s"tag" ) LF
```

The one row is R and its ref kind ([AR §5a.1]: `work` for `main` and `lane/*`, `plan` for `plan/*`, `tag` for `tags/*`;
staging refs are never exported). The tree of a checkpoint of R is thus a function of R and its state alone: creating a lane
changes no other ref's next checkpoint (open point 21). The row carries no canonical item ([F07 §14.3]); the importer takes
the ref's name and kind from the git ref and `Moirai-Ref`, and `image doctor` reports a row that disagrees with them.

## 9. Reading an image file

### 9.1 The superset an importer accepts

The exporter is the only writer of canonical bytes; the importer accepts a superset and normalises it ([AR §5b.2]):

1. one leading UTF-8 byte-order mark is skipped;
2. every CR LF and every lone CR becomes LF, in every line, block and body ([AR §5b.2] rule 7, [AR §5b.8] "CRLF /
   autocrlf");
3. SP and HT at the end of a line are removed, except inside a block (§5.4) and the body;
4. a missing final LF is accepted;
5. the lines of a node file may come in any order, except that `moirai-node 1` is first, a block stays attached to its
   `field` line, and the body or the `--- dropped` line (§6.9) is last; the importer re-sorts them;
6. every [RFC 8259] escape and upper-case `\u` digits in JSON strings (§2.4);
7. a written default (the initial status, `priority: P2`, a `field` equal to its default) is read as absent
   ([F07 §6.3], [F07 §14.4] "normalisation"), and so is an empty `status` side of a `conflict` line (§6.8);
8. a live file without `created:` or `updated:` lines (a node file written by hand); the store fills them from history;
9. the elements of a `v-set` and the entries of a `pathmove` block may come in any order, and the importer re-sorts them
   (§5.3, §5.4); an element or entry written twice stages `ImageParse` (§9.2);
10. a symmetric edge line written in the file of its larger endpoint is read as the key of the smaller endpoint
    ([F07 §6.6], §6.6); written in both endpoints' files it is that one key, and the two lines must carry the same
    properties (otherwise a line repeated, §9.2). Its state depends on both files, so an importer that parses only the
    files a commit changes (§11.1) also reads the other endpoint's file for such a line (spec sync 2b).

Nothing else is normalised: identifiers, digests and oids are lower-case only ([F01 §6.4]); a value keeps its bytes.

### 9.2 `ImageParse`

An import that meets any of these stages `ImageParse` (code 75, [F19 §12.2]) with the path, on `import/<ref>`
([AR §5b.6] step 4); nothing in this list demotes a commit:

- the marker is missing or breaks §4; an entry under `nodes/`, `schema/` or `refs/` breaks §3.4; a node file's `uid:`
  differs from its file name; a query file's name does not hash to its file name;
- a file that is not valid UTF-8 after §9.1, holds U+0000 outside a body or a block, or begins with an unknown magic line or
  version;
- a line that matches no production of its file; an unknown header key (§6.1); a required line missing (`uid:`, `kind:`;
  `title:` of a kind that requires one, [F08 §7.1], and of every tombstone); a line repeated (two lines of one header key,
  two `field` lines of one field, two `label` lines of one text, two `edge` lines of one (kind, dst), two `anchor` lines of
  one anchor uid, two `conflict` lines of one key, two ledger lines of one (field, token), two `policy` rows of one name
  (§7.1), one entry twice in a `pathmove`
  block, a symmetric edge in both endpoints' files with different properties, §9.1 rule 10); a line not allowed in its
  form (§6.10, §6.11: a `title:` in a live artifact, a counter as a `field` line, a derived field (a derived-state name,
  §6.2), `flagged` on a live node's edge, an `edge at` line); an ordinary line and a
  `conflict` line for one key; a body, or a `--- dropped` line (§6.9), with a body conflict;
- an unknown kind, field, enumeration value, edge kind or conflict class for the commit's schema ([F08 §8.1]); a value that
  does not parse by its field's type or breaks the field's constraints ([F08 §5.3], [F08 §8.5.2]: range, one line, the
  record-list shape of [F08 §5.4.5], the glob grammar of [F08 §5.4.3]; a `v-set` that holds one element twice, §9.1
  rule 9); a `path` that breaks I-F8 ([F18 §2.8]); a `relink`
  outside R-17's grammar ([F18 §5.7]); NaN, an infinity, an out-of-range number (§2.8), a ledger whose sum leaves `i64`;
- an anchor line whose texts do not match their digests ([40 §5.7]); whose texts are partly present; that lacks the digests
  its kind requires or breaks I-F9; with `end_h` on a kind other than `range`; with non-canonical base64url (§2.6); with a
  hint whose first line is greater than its last; with `v=0`;
- a named-query file that breaks §7.2.2's consistency or portability rules;
- leftover merge markers (`<<<<<<<`, `=======`, `>>>>>>>` lines), which match no production ([AR §5b.6] step 4).

A git commit whose trailer paragraph holds a malformed value of a known trailer, a repeated trailer, `Moirai-Git-Head` and
`Moirai-Git-Base` of different algorithms, a `Moirai-Sync-Base` that differs from its second parent's stated id, or a
schema version other than 1 also stages `ImageParse` ([F07 §15]); an unknown trailer makes it foreign (§10.9).

### 9.3 Derived uids

The importer recomputes every file uid from (`root`, `origin_path`, `origin_pred`), every root-node uid from `root` and every
anchor uid from (source uid, `captured`, `pred`) ([F08 §11.5], I-F2). A node or anchor whose uid does not match is accepted
as **foreign**, flagged by `image doctor` and treated as random from then on ([40 §2.3], [40 §5.7]). The image carries no
mark for it: the mismatch is a property of the stored inputs, which every importer recomputes ([F18 §2.2] "[F08] and [F14]
encode it"; open point 36). `IdCollision` ([AR §5b.6] step 4) applies to random-uid kinds only.

## 10. Commit objects

### 10.1 Bytes

A native or checkpoint commit is a git commit object ([git-objects]) whose content is exactly:

```abnf
commit-body = %s"tree " oidhex LF
              *2( %s"parent " oidhex LF )
              %s"author " ident LF
              %s"committer " ident LF
              LF
              message
oidhex      = hex40 / hex64                          ; by the destination's object format
ident       = %s"moirai/" *ich SP "<" *ich %s"@moirai.invalid" ">" SP dec %s" +0000"
ich         = %x01-09 / %x0B-3B / %x3D / %x3F-7F / nonascii   ; not NUL, LF, "<", ">"
message     = [ msg-part LF LF ] 1*( trailer LF )
msg-part    = <the stored message, §10.3>
```

- No `encoding`, `gpgsig`, `mergetag` or other header is written ([AR §5b.5] rule 4).
- `tree` is T(C, d)'s id (§3.3), or the checkpoint tree (§10.7).
- `parent` lines are the git ids, through `gitmap` for (d's destination, d's object format), of C's **actual** parents
  ([F06 §4.4.1]), in order: the first is the dst tip (`merge`) or the lane tip (`sync`), the second the src tip or
  `sync_base` ([AR §5b.4]). A root commit has none.

### 10.2 Author and committer

Both lines are identical ([AR §5b.4]): the exporter's identity and wall clock never enter, so a re-export writes the same
bytes.

- **Name** `moirai/<actor>`, **email** `<role>@moirai.invalid`, with actor and role the strings of canonical item 4
  ([F07 §3.5]); an empty actor gives `moirai/`, an empty role `@moirai.invalid`. Each byte `00`, `0A`, `3C` (`<`) or
  `3E` (`>`) of the two strings is written as `5F` (`_`). The lines are never read back ([F07 §14.3]).
- **Time**: `floor((hlc >> 16) / 1000)` seconds in decimal ([F06 §4.4.4]), zone `+0000`.
- A checkpoint commit uses `moirai/checkpoint <checkpoint@moirai.invalid>` with the time of its head commit's `hlc`.

### 10.3 The message and the trailer block

The message is the moirai message, an empty line, and the **trailer block**; with an empty moirai message it is the trailer
block alone ([AR §5b.4]).

- `msg-part` is the stored message: N(m) of [F07 §5.1] for a local commit, or the stored imported message ([F07 §5.3]) for
  a foreign or import-checkpoint commit, byte for byte. It holds no CR and no final LF.
- The trailer block is one line per trailer, each ending in LF, with no empty line inside.
- **Separation.** Let the lines of the git message be its bytes split at every LF, a final empty piece dropped. The
  **final paragraph** is the run of non-empty lines after the last empty line, or every line when there is no empty line.
  The **message part** is the lines before the empty line that precedes the final paragraph, joined by LF, or empty. The
  trailer block is always the final paragraph, and a message part never ends in an empty line, so this split recovers what
  the exporter joined ([F07 §5.3]: the final paragraph only; an imported message may end in a paragraph that begins with
  `Moirai-`).

### 10.4 Trailers

A trailer line is `Moirai-<Name>: <value>`: the key, `:`, one SP, and the value to the end of the line. The value is
extracted byte-exactly — no byte of it is trimmed ([F07] open point 22) — and a string value that is not written bare
(§2.5) is a JSON string.

```abnf
trailer     = t-commit / t-kind / t-parent / t-head / t-ref / t-folded / t-hlc / t-actor / t-role / t-session
            / t-git-head / t-git-branch / t-worktree / t-git-base / t-origin / t-sync-base / t-foreign / t-schema
            / t-ops / t-idem
t-commit    = %s"Moirai-Commit: " commit-id
t-kind      = %s"Moirai-Kind: " ( %s"ordinary" / %s"merge" / %s"sync" / %s"revert" / %s"cherry-pick" / %s"checkpoint" )
t-parent    = %s"Moirai-Parent: " ( "1" / "2" ) SP commit-id
t-head      = %s"Moirai-Head: " commit-id
t-ref       = %s"Moirai-Ref: " refname
t-folded    = %s"Moirai-Folded: " dec %s" commits" [ %s" from " commit-id %s" to " commit-id ]
t-hlc       = %s"Moirai-Hlc: " dec
t-actor     = %s"Moirai-Actor: " sval
t-role      = %s"Moirai-Role: " sval
t-session   = %s"Moirai-Session: " sval
t-git-head  = %s"Moirai-Git-Head: " oid-text
t-git-branch = %s"Moirai-Git-Branch: " sval
t-worktree  = %s"Moirai-Worktree: " sval
t-git-base  = %s"Moirai-Git-Base: " oid-text
t-origin    = %s"Moirai-Origin: " commit-id
t-sync-base = %s"Moirai-Sync-Base: " commit-id
t-foreign   = %s"Moirai-Foreign-Git: " oid-text
t-schema    = %s"Moirai-Schema: " dec
t-ops       = %s"Moirai-Ops: " dec
t-idem      = %s"Moirai-Idem: " hex32
```

| Trailer | Written when | Value | Carries ([F06] field; [F07] item) |
|---|---|---|---|
| `Moirai-Commit` | every native commit | the commit id | nothing: the id being verified |
| `Moirai-Kind` | every native and checkpoint commit | the kind's name ([F07 §4]) | item 1 (`kind`) |
| `Moirai-Parent` | a native commit whose parent i has a **stated** id ([F06 §4.4.1] `stated_ids`) that differs from the id §12.1 row 2 derives from git parent i without this trailer (the `Moirai-Commit` git parent i carries in this destination, or the id an importer gives it) | `i` and the stated id | item 2 for parent i (open point 15) |
| `Moirai-Head` | kind `checkpoint` | the head moirai commit of the checkpoint | nothing ([F06 §4.4.11] `ckpt.head`) |
| `Moirai-Ref` | §10.4.1 | a ref name | nothing (`ref` or `xtr.x_ref`) |
| `Moirai-Folded` | kind `checkpoint` | §10.7 | nothing (`ckpt`) |
| `Moirai-Hlc` | every native commit | the `hlc` as a `u64` decimal | item 3 (authoritative: the author time loses the millisecond and counter bits) |
| `Moirai-Actor`, `Moirai-Role`, `Moirai-Session` | a native commit whose string is non-empty | the string | item 4 |
| `Moirai-Git-Head` | a native commit whose `git.head` is present | `<algo>:<hex>` | item 5 `git_algo`, `git_head` |
| `Moirai-Git-Branch` | `git.branch` non-empty | the string | item 5 |
| `Moirai-Worktree` | `git.worktree` non-empty | the string ([OS/path §4.5] `c.text`) | item 5 |
| `Moirai-Git-Base` | `git.base` present | `<algo>:<hex>` | item 5 `git_algo`, `git_base` |
| `Moirai-Origin` | kind `revert` or `cherry-pick` | the origin's full id | item 8 |
| `Moirai-Sync-Base` | kind `sync` | the second parent's stated id | nothing (a copy of item 2's second id, [F07 §14.1]) |
| `Moirai-Foreign-Git` | `foreign_git` present (a natively re-exported foreign or import-checkpoint commit) | `<algo>:<hex>` | item 9 |
| `Moirai-Schema` | every native commit | the schema version | item 7 |
| `Moirai-Ops` | every native commit | the number of item-10 entries ([F07 §10.4]) | nothing: a pre-check |
| `Moirai-Idem` | §10.4.1 | 32 hex digits | nothing (`idem_key` or `xtr.x_idem`) |

- `git_algo` is the algorithm of `Moirai-Git-Head`, else of `Moirai-Git-Base`, else empty ([F07 §3.6]: an unborn
  repository's algorithm is not hashed and needs no carrier). When both are present their algorithms agree.
- A trailer whose value would be empty is not written; its absence is the empty value ([F07 §12.2]).

#### 10.4.1 `Moirai-Ref` and `Moirai-Idem`

- `Moirai-Ref` is the `xtr.x_ref` value of [F06 §4.4.12] when the record has one; otherwise, for a commit whose `import` is
  `local`, `foreign` or `checkpoint`, the name of the ref it landed on (`ref`, [F06 §4.3] order 9); otherwise (a native
  import whose source carried no `Moirai-Ref`) it is not written. A checkpoint commit writes the checkpointed ref.
- `Moirai-Idem` is `xtr.x_idem` when the record has one; otherwise `idem_key` ([F06 §4.4.7]) of a local commit that has
  the idempotency group; otherwise it is not written.

So an imported native commit re-exports with the values it arrived with, and gate 1's byte-identical round trip holds for
commits whose landing ref or key differs between stores ([F06 §4.4.12], [AR §5b.7]).

### 10.5 Trailer order

Trailers are written in this order, each at most once, `Moirai-Parent` once per parent index in index order:

`Moirai-Commit`, `Moirai-Kind`, `Moirai-Parent`, `Moirai-Head`, `Moirai-Ref`, `Moirai-Folded`, `Moirai-Hlc`,
`Moirai-Actor`, `Moirai-Role`, `Moirai-Session`, `Moirai-Git-Head`, `Moirai-Git-Branch`, `Moirai-Worktree`,
`Moirai-Git-Base`, `Moirai-Origin`, `Moirai-Sync-Base`, `Moirai-Foreign-Git`, `Moirai-Schema`, `Moirai-Ops`,
`Moirai-Idem`.

This merges [AR §5b.4]'s two lists into one: the native list (`Moirai-Commit` … `Moirai-Idem`) and the checkpoint list
(`Moirai-Kind`, `Moirai-Head`, `Moirai-Ref`, `Moirai-Folded`) are both sub-sequences of it; `Moirai-Parent` is new (open
point 15). An importer accepts the known trailers of a paragraph in any order (§10.9).

### 10.6 Native commits by kind

| Kind | Git parents | Trailers besides `Moirai-Commit`, `Moirai-Kind`, `Moirai-Hlc`, `Moirai-Schema`, `Moirai-Ops` and the item-4/5 and `Moirai-Ref`/`Moirai-Idem` trailers | Item 10 ([F07 §12]) |
|---|---|---|---|
| `ordinary` | 0 or 1 | — | the diff of T(C) against T(P1) |
| `merge` | 2: dst tip, src tip | — | the diff against dst's tree |
| `sync` | 2: lane tip, `sync_base` | `Moirai-Sync-Base` | the **full** diff against the lane tip's tree, not the residue ([F07 §10.5], CM2) |
| `revert` | 1 | `Moirai-Origin` | the diff against the parent's tree |
| `cherry-pick` | 1 | `Moirai-Origin` | the diff against the parent's tree |
| foreign, re-exported | 0, 1 or 2 | `Moirai-Foreign-Git` | the diff against the first parent's tree |
| `checkpoint` (an import-checkpoint commit), re-exported | 0 or 1 | `Moirai-Head`, `Moirai-Folded`, `Moirai-Foreign-Git` | the diff against the parent's tree |

- At `commit` granularity every commit reachable from the selected refs through both parents is exported ([AR §5b.4],
  CM2), so every parent has a git id.
- A foreign or import-checkpoint commit re-exported to a **different** destination is written natively with
  `Moirai-Foreign-Git` ([AR §5b.4] row 9); in the destination it came from, `gitmap` already maps it and nothing is
  written.
- `Violation` ops, before-images and every unhashed field stay out ([AR §5b.4] "never exported"; §12.5). Staging refs are
  never selected for export ([AR §5b.6] step 2).

### 10.7 Checkpoint commits

At `checkpoint` granularity ([AR §5b.6] step 3, [AR §5b.8]) an export writes, for each selected ref R whose head H (R's tip)
differs from the head of R's previous checkpoint in this destination, one checkpoint commit:

- **tree**: T(H, d) plus the row file of R (§8);
- **parent**: the git commit the destination's ref for R points to, when it exists (the previous checkpoint, or a native
  commit written before the destination switched granularity); none otherwise;
- **author and committer**: §10.2;
- **message**: `checkpoint <R>`, an empty line, and the trailers `Moirai-Kind: checkpoint`, `Moirai-Head: <H>`,
  `Moirai-Ref: <R>`, `Moirai-Folded`.

`Moirai-Folded: <n> commits from <first> to <last>`: let G be the moirai commit the parent stands for — the parent's
`Moirai-Head` when the parent is a checkpoint commit, the moirai commit `gitmap` maps it to when it is a native commit — and
F = H(H) minus H(G), or H(H) when there is no parent. n is the number of commits in F; `first` is the commit of F with the
least (`gen`, commit id) and `last` the one with the greatest, which is H. When n is 0 (R moved back by `undo` or
`op restore`), the trailer is `Moirai-Folded: 0 commits`, and the importer stores zero ids in `ckpt.first` and `ckpt.last`
([F06 §4.4.11]; open point 19).

**Tags at checkpoint granularity.** A selected tag `tags/v` whose commit H is the `Moirai-Head` of a checkpoint commit
already in the destination points at that commit. Otherwise the export writes a checkpoint commit for the tag as above,
with R = `tags/v`, whose parent is the destination's `refs/tags/v` if it exists and none otherwise (open point 20).

The importer creates an import-checkpoint commit from each checkpoint commit ([F07 §12.4]); a checkpoint that carries a
whole store is a bulk commit ([AR §5b.6] step 4, [F06 §9]).

### 10.8 Tags

A tag is lightweight by default: the git ref names the git commit of the tag's moirai commit ([AR §5b.4]). With
`--annotated` it names a git tag object ([git-objects]) whose content is `object <oidhex>` LF `type commit` LF
`tag <v>` LF `tagger <ident>` LF LF and the message: the tag's message ([AR §5a.1]) followed by LF, or `tag <v>` LF when it
has none. The tagger follows §10.2 with the actor and `hlc` of the tag's creating `RefUpdate` ([F05 §9.2]) and an empty role.
A tag object is not a moirai object and carries no canonical item.

### 10.9 Classifying an imported git commit

For a git commit that `gitmap` does not map for this destination, the importer takes the final paragraph of its message
(§10.3):

1. **Native candidate**: the paragraph's first line begins with `Moirai-Commit: `.
2. **Checkpoint candidate**: the paragraph's first line is `Moirai-Kind: checkpoint` and no line of the paragraph is a
   `Moirai-Commit` line.
3. Otherwise the commit is **foreign** ([F07 §12.3]).

In a candidate's paragraph every line must be a `Key: value` line of §10.4's set; a line of another key, or a line that is
not a trailer, makes the commit foreign ("unknown trailer", [AR §5b.6] step 3). A native candidate needs `Moirai-Kind`,
`Moirai-Hlc`, `Moirai-Schema` and `Moirai-Ops`, and for kind `checkpoint` also `Moirai-Head`, `Moirai-Folded` and
`Moirai-Foreign-Git`; a checkpoint candidate holds only `Moirai-Kind`, `Moirai-Head`, `Moirai-Ref` and `Moirai-Folded`, all
but `Moirai-Ref` required; a missing required trailer makes the commit foreign. A malformed value, a repeat or an inconsistency
listed in §9.2 stages `ImageParse`. The trailers may come in any order.

A native candidate whose `Moirai-Ops` differs from the number of item-10 entries the importer counts ([F07 §10.4]; it is
demoted without hashing, §12.1), whose recomputed id (§11) differs from `Moirai-Commit`, or whose message part fails
[F07 §5.1] step 1, is **demoted**: it alone becomes a foreign commit with [F07 §12.3]'s id, `verified = 0`, and the whole
git message as its message ([F07 §12.5]); its children still verify against the ids their trailers state. A foreign commit
with more than two parents stages `ImageParse` ([F07 §12.3]).

A verified native candidate whose `Moirai-Commit` names a commit the store already holds is a **twin**: nothing is
appended ([F07 §12.2] imports only a commit the store does not hold), and its children's stated ids name the held commit
as usual. `gitmap` holds one git id per commit and destination ([F10 §7.3]): when the held commit has none in this
destination, the twin's git id is mapped to it; when it already has one (a hand-written twin of a commit exported or
imported there), the first mapping stays, the twin stays unmapped, and `image doctor` reports it. An unmapped twin is
classified again by each import that meets it, with the same outcome (spec sync 2b).

## 11. Reconstruction

### 11.1 States from trees

Every image commit's item 10 is the diff of two canonical states ([F07 §10.1]); an importer builds each state from a tree:

1. It parses the tree's `.moirai-image` marker (§4) and schema files (§7.1), which with the core schema of the marker's
   schema version give the effective schema ([F08 §8.1]).
2. For every node file it parses the lines (§9.1) and maps them to keys ([F07 §6.1]) by §6.2–§6.11: `kind:` and the form
   (live or tombstone) to the existence value; `status:`/`resolution:` to the status key, the initial status with `none`
   to absent; `parent:`/`order:` to the hierarchy key; header lines, `flags:`, `field` and `label` lines to field keys, a
   default to absent; the ledger to each counter's total; `edge` lines to edge keys with `present(props)`; `anchor` lines to
   `at` edge keys with the selector block of [F07 §8.2] built from their properties (digests, never texts; the scope text
   mapped to [F08 §10.3.1]'s bytes); the body to its BLAKE3-128, and a `--- dropped` line to its hash (§6.9);
   `conflict` lines to conflict values on their keys, with §6.8's sides (a dropped side to its hash).
3. For every query file it builds the named-query item or its conflict value ([F07 §9.6]).

Only the files a commit changes need parsing: item 10 is the diff of T(C) against T(P1(C)) at the path level, and a file
whose blob id is unchanged contributes no entry. The importer keeps the texts of anchors, the bodies, the provenance and
ledger lines it needs (§11.3), outside the canonical state.

### 11.2 File transitions

For each node path whose blob differs between the first parent's tree and the commit's tree:

| First parent | Commit | Existence entry ([F07 §10.1]) | Other entries | Stored ops ([F06 §7]) |
|---|---|---|---|---|
| absent | live file | `live(kind)` (created) | one per non-absent value key and out-edge | `Create` with its image; edge ops |
| live | live | none, unless a `conflict existence` line appears or changes | one per changed key | `SetField`, `SetStatus`, `Incr`, `SetBody`, `Move`, edge ops, `Conflict`, `Resolve` |
| live | tombstone | `deleted(kind, reason, replaced_by)` | every value key except `title` → absent; `title` as kept; out-edges not retained → absent | `Delete`; edge ops ([F07 §13]) |
| tombstone | live | `live(kind)` (undeleted) | each restored value key | `Undelete` with its image |
| tombstone | tombstone | changed when `kind:`, the reason or the replacement differ | changed retained edges, a changed `conflict` line | edge ops, `Conflict`, `Resolve` |
| absent | tombstone | `deleted(kind, reason, replaced_by)` from the absent state | `title`; the retained out-edges | `CreateDeleted` ([F06 §7.4], NF-11) with the kind, reason, replacement, the creator by [F06 §7.4]'s rule, and the retained title as its image; the retained out-edges as `AddEdge` ops (pass 1, S1-6, A1-4) |
| live | absent | foreign only: `deleted(kind, "image:file-removed", none)` | as live → tombstone; the title and the retained out-edges come from the first parent's file by each edge kind's `on_src` policy under its default policy ([F08 §8.4.6]; never the store's `edges.<kind>.on-src-deleted`, so every importer computes the same entries): a `repoint-or-flag` edge (there is no replacement) stays with `flagged` when the source was an open blocker in the first parent's state (an unfinished `blocks` source, a gating `gates` verdict) and becomes absent otherwise; `retain`, `retain-warn`, `retain-anchors` and `recompute` edges stay; the others become absent — so a hand-deleted blocker never unblocks silently (X4; spec sync 2b) | foreign `Delete{image:file-removed}`; a re-export writes the tombstone file ([AR §5b.6] step 2) |
| tombstone | absent | foreign only: none | none | `TombstoneRemoved`: a hint when nothing references the node, a violation when something does ([F19 §12.2]) |

- An exporter never removes a node file, so the last two rows arise only from foreign commits; a native commit whose tree
  shows them fails to verify and is demoted.
- **Counters.** A counter entry carries Σ(lines at C) − Σ(lines at P1(C)) (§6.5); a live → tombstone transition gives minus
  the total, an absent → live transition the total.
- **Bodies.** The importer stores every body whose hash an entry introduces ([F06 §8] BD-4): the body section, a body
  conflict side, the `body` line of an existence snapshot.
- **Dropped bodies** ([F06 §8.1] DB-11; spec sync 3). For a `--- dropped` line, a dropped side or a snapshot's
  `body dropped:…` line, the importer stores no bytes and takes the hash as the body key. For each such hash that its
  store neither holds as a body ([F06 §8.1] DB-4) nor has dropped, and whose bytes the import supplies nowhere, it
  appends a `BodyDrop` record of origin `import` ([F05 §9.29]) with the line's reason, an empty note and the importing
  command's actor, before the first imported `Commit` record that names the hash (in that record's group or an earlier
  one, [F05 §4.7]), at most 2,048 hashes per record in ascending order so every group fits [F17 §4.4] W3: a record
  that would make that commit's group exceed W3 stands in a group of its own before it. One record names hashes of one
  reason. The import's first line for a hash decides its reason; a later line for the same hash, whatever its reason,
  finds it dropped. For a hash its store holds, it keeps the bytes; only the owner's body drop drops held bytes (DB-3),
  so an import's record leaves nothing to purge ([F16] P-101). When an imported commit supplies a hash's bytes (a body
  section, a body side or a snapshot `body` line) that the store does not hold, the first imported `Commit` that names
  the hash carries them, whether its own file holds the bytes or a dropped line (BD-4). An importer that meets bytes
  whose hash its store has dropped (a body section, a body side or a snapshot `body` line of an image older than the
  drop) stores none of them and does not refuse the import: the entry takes the hash, as for a `--- dropped` line, and
  BD-4 does not apply to it.
- **Foreign commits.** A foreign commit with 0 or 1 parents takes this table's entries. A two-parent foreign commit is
  imported as a typed merge of its parents' imported states over their merge base; its tree is consulted only to resolve
  `TextHunk` conflicts a human resolved on the git side, counters come from the typed merge, and every remaining
  disagreement is a conflict value ([AR §5b.6] step 3, I30′, N6). A foreign commit's own ledger line carries its id (§6.5):
  the tokens a human wrote are summed, not kept.

### 11.3 Image-only data a store keeps

Some bytes of a tree are carried by no canonical item but must come back on re-export, or gate 2 ("a re-export of the fresh
store to a new destination reproduces every head tree byte-identically", [AR §5b.7]) fails. A store keeps them:

| Data | Kept for | Where ([F06]) |
|---|---|---|
| anchor texts (`quote`, `prefix`, `suffix`, `end`) | every anchor imported with its texts | the anchor record ([F08 §10.3], carried by the op of [F06 §7.5]) |
| `Moirai-Ref`, `Moirai-Idem` as imported | native and checkpoint imports | `xtr` ([F06 §4.4.12]) |
| `Moirai-Head`, `Moirai-Folded` | checkpoint imports | `ckpt` ([F06 §4.4.11]) |
| the stated parent ids | native imports below a demoted parent | `stated` ([F06 §4.4.1]) |
| per node file brought in by an import-checkpoint commit: its `created:`, `updated:` and `deleted:` values (commit token and time text as written) and its ledger lines (field, token, delta) | import-checkpoint commits | `ckimg` ([F06 §4.4.14]) of an inline commit; the `CKIMG` section of the `cs.<n>` of a bulk one ([F09 §16.4], [F06 §9] BK-5); pass 1, S1-23, A1-10 |

Everything else in a tree is a function of the canonical state and the commit graph (§3.3, §6.3, §6.5), except the
dropped-body lines and sides, which are a function of the store's dropped set: an importer keeps their reasons in its own
`DROPPED` table through the `BodyDrop` records of §11.2, not as image-only data of a commit (§6.9; spec sync 3).

## 12. The gate-0 carrier table

Every field hashed into `commit_id` has exactly one carrier in the git commit, and the importer rebuilds the canonical form
from these carriers and nothing else (CB1, I38′, [AR §5b.4]). This section completes [F07 §14] and keeps each of its rows;
[F07] owns the bytes each item hashes.

### 12.1 Items 1–9

| Item ([F07 §3]) | Native commit, every kind (§10.6) | Foreign commit ([F07 §12.3]) | Import-checkpoint commit ([F07 §12.4]) |
|---|---|---|---|
| 1 kind | `Moirai-Kind: <name>` | derived: `ordinary` for 0 or 1 git parents, `merge` for 2 | `Moirai-Kind: checkpoint` of the checkpoint paragraph |
| 2 parents' stated ids | the git parents in order ([AR §5b.4]: dst or lane tip first); parent i's stated id is `Moirai-Parent: i` when present, else the `Moirai-Commit` of git parent i, else (a foreign or checkpoint parent) the id this store holds for git parent i through `gitmap` ([F07 §12.2]); a `sync`'s second parent is repeated in `Moirai-Sync-Base`, which must equal it | the ids this store holds for the git parents through `gitmap` (a demoted parent's foreign id) | the id this store holds for the git parent (the previous checkpoint), or none |
| 3 `hlc` | `Moirai-Hlc` | [F06 §4.4.4] from the committer timestamp and the parents' `hlc` | as foreign |
| 4 actor, role, session | `Moirai-Actor`, `Moirai-Role`, `Moirai-Session`; a missing one is empty | actor `git:` and the author's email ([F07 §12.3]); role and session empty | actor `image:checkpoint`; role and session empty |
| 5 git provenance | `Moirai-Git-Head`, `Moirai-Git-Branch`, `Moirai-Worktree`, `Moirai-Git-Base`; `git_algo` from the head or base value (§10.4) | empty | empty |
| 6 message | N ([F07 §5.1]) of the message part (§10.3) | N_imp ([F07 §5.3]) of the whole git message | N_imp of the message part |
| 7 schema version | `Moirai-Schema`, equal to the tree's marker (§4) | the marker's `schema-version:` | the marker's `schema-version:` |
| 8 origin | `Moirai-Origin` | empty | empty |
| 9 `foreign_git` | `Moirai-Foreign-Git`; absent is empty | the destination's object format and the commit's own id | the destination's object format and the checkpoint commit's id |
| 10 `changeset_digest` | §12.2 over the diff of the commit's tree against its first git parent's tree (the empty tree for none) | 0–1 parents: as native; 2 parents: the typed merge (§11.2) | §12.2 over the diff against the git parent's tree |

`Moirai-Ops` carries [F07 §10.4]'s entry count as a pre-check: an importer that counts another number demotes without
hashing (§10.9).

### 12.2 Item 10 by key class

| Key class ([F07 §6.1]) | Tree path | Carrier ([F14] section) | Value the importer reads |
|---|---|---|---|
| 1 existence | `nodes/<h1>/<h2>/<uid>.moi` | the file's presence and form; `kind:`; in a tombstone `field reason:` and `field replaced_by:` (§6.10); with a `conflict existence` line, the form also carries `prov` (§6.8.1) | absent; `live(kind)`; `deleted(kind, reason, replaced_by)`; transitions §11.2; an existence conflict's `prov`: the side whose value has the file's form, else `ours` |
| 2 status | same | `status:`, `resolution:` (§6.2) | (status, resolution); the initial status with `none` is absent |
| 3 hierarchy | same | `parent:`, `order:` | (parent uid, order); both missing is absent |
| 4 field: title | same | `title:` (live, not `title_derived`; every tombstone) | the text |
| 4 field: header enumerations | same | `priority:`, `criticality:`, `confidence:`, `authority:` | the name; a missing line or a default is absent |
| 4 field: flags | same | `flags:` | `true` for each listed flag; an unlisted flag is absent (`false` is the default) |
| 4 field: field-block and cold fields | same | `field <name>:` lines, blocks (§6.4, §5.4) | the typed value (§5.1); a default is absent |
| 4 field: `labels` | same | `label` lines | the set of their texts |
| 5 observation | same | `conflict observation` (§6.8) | the conflict value; the six member fields are then absent |
| 6 counter | same | `incr` lines (§6.5) | Σ(lines) at C − Σ(lines) at P1, as the delta of [F07 §7.5] |
| 7 edge (not `at`) | same, the source's file | `edge` lines: `pin=`, `flagged` (§6.6) | `present(props)` |
| 7 edge `at` | same, the referrer's file | `anchor` lines (§6.7): `kind`, `mode`, `watch`, `scope`, the four digests, `occurrence`, `hint`, `window`, `span`, `blob`, `git`, `captured`, `pred`, `marker`, `v` | the key's disc is the anchor uid; `present` with the selector block of [F07 §8.2]; identical in `full` and `hash-only` mode |
| 8 body | same | the `---` section, or the `--- dropped` line (§6.9) | BLAKE3-128 of the decoded bytes, or the dropped-body line's hash; no section and no line is absent |
| conflict values | same | `conflict` lines (§6.8), sides by key | `{class, base, ours, theirs}`; a body side is hashed from its text, and a dropped side is its hash; a `live` existence side carries its node image |
| schema: kind, field, enumeration value, edge kind | `schema/kinds.moi`, `schema/fields.moi`, `schema/edges.moi` | one row per item (§7.1) | the item of [F07 §9.2]–§9.5, references by name |
| schema: policy row ([F08 §8.5.6]; spec sync 2b) | `schema/policy.moi` | one `policy` row per item (§7.1) | the item of [F07 §9.7] |
| schema: named query | `schema/queries/<q>.moi` | `name:` (the key), `lq:`, `params:`, `shape:`, `budget:`, the text (§7.2) | the item of [F07 §9.6]; a `conflict definition` line gives its conflict value |

### 12.3 R4 and R5 items

| Reservation | Hashed data | Carrier |
|---|---|---|
| [40] R-1 `path`, `oid`, `pathmove` | root by name, exact text; algorithm and digest; hlc, class, from, to, git | §5.2 text forms (implied or explicit root), `oid-text`, the `pathmove` JSON array |
| [40] R-2 artifact field set, `planned`/`removed` | field keys; status | `field` lines of §6.11; `status:` |
| [40] R-2 the observation composite | one conflict key | `conflict observation` |
| [40] R-3 derivation inputs | `origin_path`, `origin_pred`, `root`; `captured`, `pred` | `field` lines; anchor `captured=`, `pred=` |
| [40] R-4 `at`, discriminator, anchor record | edge key with the anchor uid; the selector block | `anchor <uid> -> <dst>` lines |
| [40] R-5 root node and `path_moves` | field keys `root`, `path_moves`; no op, item, trailer or annotation | the root node's `field root:` and `field path_moves: <<` block |
| [40] R-10 anchor digests | `quote_h`, `prefix_h`, `suffix_h`, `end_h` | the digest properties, on every line in both modes |
| [40] R-11 | — | this chapter: §5.6, §5.7, §6.7, §6.11 |
| [40] R-17 `relink` | a field key of the composite | `field relink:` |
| [50] F3 named queries | the `QUERIES` item in portable form | `schema/queries/<q>.moi` |

R4 and R5 add **no trailer and no commit annotation** ([AR §5b.2] rule 9, [40 §5.7], [41 B4]): link provenance, directory
moves and named queries are part of the tree diff, so the one reconstruction rule covers every commit kind, checkpoint
commits included.

### 12.4 Carried, not hashed

| Data | Carrier |
|---|---|
| the commit id being verified | `Moirai-Commit` |
| the landing ref; the idempotency key hash | `Moirai-Ref`, `Moirai-Idem` (§10.4.1) |
| the checkpoint origin | `Moirai-Head`, `Moirai-Folded` |
| a copy of a `sync`'s second parent | `Moirai-Sync-Base` |
| the entry count | `Moirai-Ops` |
| author and committer | derived from items 3 and 4; never read back |
| node provenance | `created:`, `updated:`, `deleted:` (§6.3) |
| ledger tokens | the last field of each `incr` line |
| anchor texts | `quote=`, `prefix=`, `suffix=`, `end=` in `full` mode |
| a dropped body's reason | the reason of the `--- dropped` line and of a `dropped:` side (§6.8, §6.9; spec sync 3) |
| the ref's name and kind | the git ref name; `refs/heads.moi`, `refs/tags.moi` (§8) |
| store id, granularity per ref, image format, anchor-text mode, export cursor; `#N` | the side ref `refs/moirai/meta/<store-id>`: `meta.moi`, `aliases/<h1>.moi` (§14.1) |
| reflog and client-head events | `refs/moirai/ops/<store-id>` with `--with-oplog` (§14.2) |

### 12.5 Never exported

`Violation` ops (they exist only on staging refs, which are never exported); before-images; `#N`, `aN`, `lsn`, `seq`,
`gen`, `ref_id`, `ref_old`, `prev_on_ref`, `ref_seq`; symbol ids and every store-local schema id ([F08 §8.3]); the store id
outside the side ref; `import`, `verified`, `idem_payload`, absorbed vectors, `affected` and `affected_complete`
([50] F16); `stmt_origin`, `stmt_sym`, `stmt_hash` ([50] F10); `append_hlc` ([50] F14); `actor_src` ([90 §10.1]); the
provenance group `prov` (`actor_kind` and `model`, [F06 §4.4.17]; [AR §11] OQ-A-9, spec sync 3), declared and
store-local, which is not this chapter's `prov` production (§6.1) nor an existence conflict's `prov` (§6.8.1); `cs_ref`;
`CREATOR` (an importer derives it, [F06 §7.4]); markers, leases, idempotency results, cursors, pins, `next_id`, fencing
tokens, `ALLOC` and `UIDX` ([50] F17); derived state (I36′) and a named query's canonical-AST hash; every R4 resolution
and evidence table — `FILEOBS`, `PENDING`, `FSINTENT`, `FPRINT`, `JOURNALCUR`, `DIRMAP`, `TREES`, `PREFIXEV`, `GITFACTS`,
`ANCHORRES`, the binding rows, file ids, volume keys, creation times and mtimes (I-F4, [40 §5.7], [F18 §2.4]); the
`DROPPED` table's `drop_lsn` and `hlc` and a `BodyDrop` record's note and actor ([F11 §13.4], [F05 §9.29]; a dropped body
travels only as its hash and reason, §6.9); the harvest cursor `HARVEST` ([F11 §13.5], [API §8.8]); segments, log and
store files ([AR §5b.4] "never exported", [AR §5b.7]).

### 12.6 Gate 0 and its fixtures

**Gate 0** ([AR §5b.7], [AR §5b.5] rule 8, M5): for every commit kind, export one commit, rebuild its canonical form from the
carriers of §12.1–§12.2 alone, and assert that the rebuilt `commit_id` equals the exported one, before any round-trip
corpus runs; in both anchor-text modes ([60 §3.13] GT8). [PLAN §3.2] WP-21 writes the fixtures (`fixtures/carrier/`) from
this chapter and [F07]; they are frozen for M5 ([PLAN §3.2] WP-21). Each fixture holds the git commit object, the first
parent's tree and the commit's tree (listings and every touched blob), the parents' ids, and the expected [F07] byte
stream, `changeset_digest` and `commit_id`. The set keeps every row of [F07 §14.4] and adds the rows marked +:

| Case | Required content |
|---|---|
| `ordinary` | one parent; git provenance with both digests; a field, a status, a counter delta (ledger), a body, an edge with a pin |
| `merge` | two parents; at least one conflict value (a field `FieldEdit`) and one clean key from each side |
| `sync` | two parents; a residue smaller than the full diff, so that item 10 differs from the stored ops; `Moirai-Sync-Base` |
| `revert` | origin set; the inverse of an `ordinary` fixture |
| `cherry-pick` | origin set |
| foreign | no trailer; a one-parent hand edit (a CR LF file, reordered lines, a ledger line with a non-commit token, a removed node file); a two-parent case whose item 10 comes from the typed merge |
| import-checkpoint | kind `checkpoint`, a previous checkpoint as parent, and a first checkpoint with no parent; `Moirai-Folded` with n ≥ 1 and with n = 0 (+) |
| + re-exports | a foreign commit and an import-checkpoint commit re-exported natively with `Moirai-Foreign-Git`; a child of a demoted parent re-exported with `Moirai-Parent` |
| + demotion | a native candidate whose rebuilt id differs from `Moirai-Commit`; a native candidate whose `Moirai-Ops` differs from the counted entries while its rebuilt id equals `Moirai-Commit`, demoted without hashing (§10.9, §12.1; spec sync 2b) |
| anchors | an `at` edge with a `range` anchor (S-04) and a `quote` anchor with an empty prefix, exported in `full` and in `hash-only` mode: the same `commit_id`; + a `symbol` anchor with a scope and an `occurrence`, a `lines` anchor with a window, an anchor with `pred` and `marker`, an anchor text that is not UTF-8 |
| tombstones | a delete with flagged and historical retained edges; an undelete; a tombstone landed from the absent state; + a tombstone carrying a `conflict existence` line |
| normalisation | messages of [F07 §5.4]; a default written explicitly in the image (the default priority, the initial status) giving the same id as the line left out |
| order | two nodes, all eight classes on one uid, a symmetric edge written in both directions, a schema entry and a named query |
| + R4 | a file node registered with `origin_pred`, `aliases` and `observed_blob`; a root node gaining a `path_moves` entry; a `relink` change; an `observation` conflict (`PathClaim`) |
| + R5 | a named query defined, changed, and dropped, and a `FieldEdit` on a definition |
| + trailers | JSON-escaped `Moirai-Actor` and `Moirai-Worktree` values; a commit without git provenance; a commit in a SHA-256 destination |
| + dropped bodies | an `ordinary` commit that sets a body the exporting store has dropped, written as the `--- dropped` line, with the same `commit_id` as with the body section; a body `TextHunk` with a dropped side; a commit whose only tree change is a node file re-encoded after a drop (§3.3), with no item-10 entry for it (spec sync 3) |

## 13. Refs and destinations

### 13.1 Ref mapping

| moirai ref | Separate image repository (the default destination) | Project repository (not built, [74 A17]; naming reserved) |
|---|---|---|
| `main`, `lane/<n>`, `plan/<n>` | `refs/heads/main`, `refs/heads/lane/<n>`, `refs/heads/plan/<n>` | `refs/moirai/heads/…` |
| `tags/<n>` | `refs/tags/<n>` | `refs/moirai/tags/<n>` |
| staging refs (`merge/*`, `import/*`, `orphans/*`) | never exported | never exported |

- In the separate repository an ordinary `git clone` fetches every exported ref ([AR §5b.4]). The side refs are
  `refs/moirai/meta/<store-id>` and `refs/moirai/ops/<store-id>` in every destination (§14).
- **Import** maps `refs/heads/<n>` to the moirai ref `<n>` and `refs/tags/<n>` to `tags/<n>`; a git ref whose name is not a
  moirai ref name of [F12], or that names a staging ref, is not imported and is reported.
- Ref names follow [F12] and [80] X-F9 (P11 (b)): no segment is a Windows device name or ends in `.lock`, input is
  NFC-normalised, and a name fold-equal to a live ref is refused; `doctor image` reports fold-equal refs in a destination.
- An export never deletes a destination ref; the ref of a deleted moirai ref stays until the owner removes it (open point 47).
- Updates follow [AR §5b.6] step 4: every ref of a run, the side ref included, in one `packed-refs` transaction in a moirai
  bare repository, each with a CAS against the last-seen oid ([F05 §9.7] `seen`); a mismatch is exit 6 ([F19]).

### 13.2 Destinations

- A destination is named by its configuration name `<name>` ([CFG §10.10] `image.dest.<name>.*`; `default` when `--to` is
  not given). Its number `dest` (1–255), which `gitmap` pages, `GitMap` records and `HEAD.image_cursor` use ([F10 §7],
  [F05 §9.7], [F04 §5.11]), is declared by the first `GitMap` record for the name (`gflags` bit 0, `dest_name`): the
  smallest number no earlier `GitMap` record declared. A number is never reused for another name, and a name keeps its
  number for the store's life. A 256th name is refused (exit 7, [F19]). This settles [F10] OP-10-09, [F04] open point 7 and
  [CFG] open point 8.
- **What a destination records.** Its object format is the destination repository's own `extensions.objectFormat`
  (written explicitly at `--create`, [AR §5b.6] step 1), and its kind is the bare-repository layout itself; [CFG]'s `init`
  keys `image.dest.<name>.object-format` and `.kind` are read from there once the destination exists. The image format
  version and the anchor-text mode are in `meta.moi` (§14.1). The `.moirai-image` marker of each tree also carries the object
  format.
- **Retained encoders** (O5, [AR §5b.5] rule 6): a destination records its image format version (`moi-format`), a
  re-export of an old commit uses that version's encoder, and a format bump is a new destination, never a rewrite. Format
  v1 has one encoder.

## 14. Side refs

The side refs are unhashed, store-local and never part of any commit's tree ([AR §5b.1], CB2). An importer reads them only
as hints.

### 14.1 `refs/moirai/meta/<store-id>`

`<store-id>` is the exporting store's id as 32 lower-case hexadecimal digits ([F02 §4]). The ref names a commit with no
parent, rewritten by every export run of that store to that destination:

- **tree**: the blob `meta.moi` (mode `100644`) and the subtree `aliases/` (mode `40000`), which holds one blob
  `<h1>.moi` (mode `100644`) per first uid byte h1 that some row has, h1 written as two lower-case hexadecimal digits
  (`00.moi` … `ff.moi`); a prefix without rows has no file (pass 1, P1-17; the fan-out of open point 32, pending the
  owner's sign-off of the [AR §5b.1] change, `reviews/owner-questions.md`);
- **author and committer**: `moirai/image <image@moirai.invalid>`, with the time of the `hlc` of the newest commit the run
  exported (§10.2);
- **message**: `moirai image metadata` and LF.

```abnf
meta-file   = %s"moirai-meta 1" LF
              %s"store-id: " hex32 LF
              %s"moi-format: " dec LF
              %s"anchor-text: " ( %s"full" / %s"hash-only" ) LF
              %s"last-export-seq: " dec LF
              *( %s"ref " refname SP ( %s"checkpoint" / %s"commit" ) LF )

aliases-file = %s"moirai-aliases 1" LF
               1*( uid %s" #" NZDIGIT *DIGIT LF )    ; every uid of one file begins with the file's byte h1
```

- `meta.moi`: the store id; the image format version (1); the destination's anchor-text mode ([40 §5.7]: "the destination's
  mode is recorded in the unhashed side ref"); `last-export-seq`, the greatest store `seq` this store has exported to the
  destination ([F04 §5.11]); one `ref` row per ref this store has exported there, with its granularity, sorted by name.
- `aliases/<h1>.moi`: one row `uid #N` per node file in the trees of the refs this store has exported to the destination
  whose uid begins with the byte h1, as they stand after the run, with this store's `#N` for the uid, sorted by uid
  ([AR §5b.1]); each file follows `aliases-file`. The union of the files is [AR §5b.1]'s alias table. An export run
  writes the blob of every prefix whose rows it changed and takes every other entry of `aliases/` unchanged from the side
  ref's previous tree, so a run that touches a few nodes writes a few small blobs, not the whole table (≈ 4.2 MB at 1e5
  nodes, [AR §5b.9]'s ≤ 50 ms incremental export). The tree is a function of the rows alone, so the rewrite is
  deterministic whichever blobs are reused. An importer honours `#N` only when N ≥ its `next_id` and otherwise records the
  alias `(origin store id, #N) → local #N` ([AR §5b.6] step 5, N11, I35′).
- Gate 1's "byte-identical objects and refs" ([AR §5b.7]) covers every hashed object and the image refs of §13.1; the side
  refs differ by construction, since their names carry the store id (open point 31).

### 14.2 `refs/moirai/ops/<store-id>`

Written only by `--with-oplog` ([AR §5b.4]; in the release): a parentless commit like §14.1's whose tree holds one blob,
`ops.moi`, listing the store's `RefUpdate` and `ClientHead` events ([F05 §9.2], [F05 §9.3]) for the exported refs in log
order, so that reflog history can be reproduced elsewhere:

```abnf
ops-file    = %s"moirai-ops 1" LF *( ref-event / head-event )
ref-event   = %s"ref " refname SP ( %s"create" / %s"delete" / %s"undo" / %s"op-restore" )
              SP ( commit-id / "-" ) SP ( commit-id / "-" ) SP token SP dec LF      ; old, new, actor, hlc
head-event  = %s"head " ( %s"client:" token / %s"session:" token )
              SP ( %s"set " ( refname / commit-id ) / %s"remove" ) SP dec LF         ; key, target, hlc
```

Only client heads keyed by a client name or a session are written. A head keyed by a directory, and every binding field
(directory path, file id, designation, expected ref, base commit), is machine-local and is not written (I-F4, [F18 §2.4]).

### 14.3 Attributes

`*.moi -text diff` is written into the destination's `info/attributes`, never into a tree ([AR §5b.1], CL1). A clone does
not receive `info/attributes`; the importer's CR LF rule (§9.1) covers a clone that converts line ends.

## 15. Determinism rules

These restate [AR §5b.5] for the bytes of this chapter; each is a condition of I28′.

1. Tree entries are sorted by git's rule; modes are `100644` and `40000` only; no executable bit, symbolic link or empty tree
   (§3.2).
2. The object format is the destination's; objects of two formats are never mixed; `gitmap` records the format and the
   destination (§13.2).
3. Every blob byte is produced by §4–§8 and §14; the exporter re-parses each file it writes and asserts that parsing and
   re-encoding reproduce it ([AR §5b.5] rule 3).
4. Commit objects follow §10.1; no `gpgsig`, no `encoding`, no other header.
5. `gitmap` is derivable: walking the image and reading `Moirai-Commit` trailers rebuilds it (`image doctor --rebuild-map`),
   reporting commits whose recomputed id differs from the trailer ([AR §5b.5] rule 5).
6. Every past image format's encoder is retained; a destination keeps its format version (§13.2).
7. Hashed content holds no store-local datum: no `#N`, `seq`, lsn, store id or granularity (§4, §12.5); named-query texts are
   portable (§7.2.2); derived uids read no machine-local input ([F08 §11]).
8. Every commit kind is reconstructable from its trailers and the diff against its first parent's tree (§11, §12).
9. T(C, d) is a function of moirai data, d and the exporting store's dropped set (§3.3): values are written in one form
   (§2.4, §2.5, §5), lines in one order (§6.2, §6.4–§6.8, §7.1), provenance and ledgers by functions of the commit graph
   (§6.3, §6.5), a dropped body as its hash and reason (§6.8, §6.9), and subtrees are reused only when they equal T(C, d).

## 16. The `moi/` fixture catalogue

[PLAN §3.2] WP-21 writes `fixtures/moi/` from this chapter. Each positive fixture is a file with its expected parse (the keys
and values of §11.1); each negative fixture is named for the `ImageParse` rule it breaks. The set must contain at least:

| Group | Fixtures |
|---|---|
| node kinds | one live node file of each of the 13 core kinds, and one of a project kind, each with every header line its kind can carry |
| values | every type of §5.1: bare and JSON texts (an SP inside, a leading SP, a leading `"`, `[`, `{`, the value `<<`, 4,096 and 4,097 bytes, a control character), `f64` edge values (`0.0`, `1e+21`, `1.5e-7`, `-3.0`), sets with escaped elements, explicit-root and implied-root paths (an `abs` path on each OS form), `oid` of both algorithms |
| blocks | a two-line text, a text ending in LF, an empty inner line, an inner line `>>` after its two SP, trailing SP inside a block, a `path_moves` block with several entries |
| ledgers | several counters and lines; a negative delta; a foreign token |
| edges | pinned, flagged (tombstone), symmetric, `mentions` |
| anchors | every anchor kind; `full` with texts, `hash-only`, and the `text-unavailable` import of a full-mode line without texts; `occurrence`, `marker`, `pred`, a one-line hint, a scope with each language and an escaped name, a non-UTF-8 text in `%` form, an empty prefix |
| conflicts | a scalar `FieldEdit` with an absent side, a `StatusFork` with resolutions, a body `TextHunk` (bodies with LF and `"`), a `parent` conflict, an `edge` and an `edge.at` conflict, an `observation` `PathClaim`, `existence` `DeleteVsModify` on a live file and on a tombstone with a snapshot carrying status, fields, labels, a counter total and a body; a body `TextHunk` with a dropped side, and an `existence` snapshot whose `body` line is dropped (§6.8, §6.8.1; spec sync 3) |
| bodies | no body; a body without final LF; with one final LF; with three; with an inner `---` line; with trailing double SP; with U+0000; a dropped body, one per reason (§6.9; spec sync 3) |
| tombstones | with flagged and historical edges and anchors; an artifact tombstone with its last path as title; with `replaced_by`; without reason |
| R4 | a file node with every field; a `removed` file node; a `planned` file node; a root node with `path_moves` |
| schema | `kinds.moi`, `fields.moi` with field rows (defaults and ranges) and value rows (covers, `*` kind), `edges.moi` with a reading that needs JSON, `policy.moi` with a row of a parameterised name (`policy.role.<role>.mcp-write` for one role) and a row with a list value (`policy.role.developer.fields`) |
| queries | a query file with parameters, one without, one with a string default holding HT (JSON `params:`), a conflicted definition with an absent side |
| side refs and rows | `.moirai-image` in both object formats, `meta.moi`, `aliases/<h1>.moi`, `ops.moi`, `refs/heads.moi`, `refs/tags.moi` |
| superset inputs | a BOM, CR LF, trailing SP, reordered lines, `\u` and `\/` escapes, a default written explicitly, a live file without provenance lines |
| `ImageParse` | one negative per rule of §9.2; for dropped bodies (spec sync 3): a `--- dropped` line with an unknown reason, with an upper-case hash, in a tombstone, followed by a body section, or with a body conflict; a body side that is a bare token other than `dropped:<hash>:<reason>` |

## 17. Examples (informative)

Uids, commit ids and git ids below are synthetic. Digests, derived uids and query hashes marked "computed" were computed
with BLAKE3 from the bytes shown; the `span` values are illustrative. Fixtures are written from the rules above, never from
these examples ([F01 §2.1]).

### 17.1 The task node of [AR §5b.2], corrected

[AR §5b.2]'s example writes its commit ids with 63 hexadecimal digits after `c`; format v1 requires 64 ([F01 §6.4]). It
also writes `priority: 1` (enumerations are written by name, §5.1) and `criticality: normal` (a default, omitted, §6.2).
Corrected, at `nodes/01/8f/018f3c2e7a117b3c9d5e4c2f1a0b9e77.moi`:

```
moirai-node 1
uid: 018f3c2e7a117b3c9d5e4c2f1a0b9e77
kind: task
title: Byte-range lock protocol
status: in_progress
priority: P1
parent: 018f3c2e7a117b3c9d5e4c2f1a0b9e09
order: a0V
created: c9b2e6c1d4f0a7e3b5c8d1f2a9e4b7c6d3f0a1e2b5c8d7f4a3e6b9c2d5f8a1b47 2026-09-21T14:02:11.483Z
updated: c4470a11e2f3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4de 2026-09-25T14:02:40.011Z
field acceptance: writer and flush bytes locked per range; a dead holder's range freed by the OS
field assignee: dev#1
field estimate: 3
field files_owned: [src/lock.rs, src/vfs/lock_bytes.rs]
field phase_state: implementing
field work_kind: impl
label l5np
label storage
incr reopen_count +1 c4470a11e2f3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4de
edge blocks -> 018f3c2e7a117b3c9d5e4c2f1a0b9e51
edge cites -> 018f3c2e7a117b3c9d5e4c2f1a0bd3a5 pin=c4410f0e2d3c4b5a69788796a5b4c3d2e1f0a9b8c7d6e5f4a3b2c1d0e9f8a7b65
edge implements -> 018f3c2e7a117b3c9d5e4c2f1a0b9e40
edge mentions -> 018f3c2e7a117b3c9d5e4c2f1a0b9e52
---
Writers lock byte ranges of LOCK, never the whole file.
See #52 for the reader registry this depends on.

```

The file ends with the body's own LF and the encoding LF (§6.9). The `updated:` commit is the one that last changed this
file; it also carries the ledger line of the reopen (§6.3, §6.5).

### 17.2 A file node, the root node and a referrer's anchors

The file `crates/engine/src/lock.rs` registered in root `project` with no predecessor has the uid
`d706fcde60f0b6cbf56c23297162b652` (computed, [F08 §11.2]); after a move its file
`nodes/d7/06/d706fcde60f0b6cbf56c23297162b652.moi` reads:

```
moirai-node 1
uid: d706fcde60f0b6cbf56c23297162b652
kind: artifact
status: present
created: cc4e084fb20d5ba50ba22ca5874d846c8ccda0ca92161fac5ae87e00eb07440df 2026-09-21T14:02:11.483Z
updated: ca0f95ceb140682ca1c5c708146ed5080c9dbee17d53c7bd9eb509b1207f4f7ad 2026-09-26T09:12:40.118Z
field aliases: [crates/engine/src/lock.rs]
field artifact_kind: source
field bytes: 18231
field observed_blob: sha1:de177738b58e970465382658e69b18745029e248
field observed_git: sha1:75bea42e34a5942eb6593b2867480e8fbc95eed1
field oid: sha1:de177738b58e970465382658e69b18745029e248
field origin_path: crates/engine/src/lock.rs
field path: crates/engine/src/sync/lock.rs
field relink: lazy/file-id
field root: project
```

The root node of `project` has the uid `5bd0e29e6afc4a73557e4cbdf7d34c33` (computed, [F08 §11.3]):

```
moirai-node 1
uid: 5bd0e29e6afc4a73557e4cbdf7d34c33
kind: area
title: root:project
status: active
created: cc4e084fb20d5ba50ba22ca5874d846c8ccda0ca92161fac5ae87e00eb07440df 2026-09-21T14:02:11.483Z
updated: ca0f95ceb140682ca1c5c708146ed5080c9dbee17d53c7bd9eb509b1207f4f7ad 2026-09-26T09:12:40.118Z
field path_moves: <<
  ["00117336598351118336","explicit","docs/plan/","docs/archive/plan/","sha1:80bb9dad56a25b8ec857f344172c71c9d8fe4ca6"]
>>
field root: project
```

Task `018f3c2e7a117b3c9d5e4c2f1a0b9e51` cites the file twice: a `file` anchor and a `symbol` anchor with scope
`rust:struct LockFile/impl LockFile/fn acquire`, quote `pub fn acquire(&self, timeout: Duration) -> Result<Guard>`, the
32-byte prefix `" Blocks until the byte is ours.\n"` and the 32-byte suffix `" {\nlet mut spins = 0u32;\nloop {\n"`, and a
window of three hashes before and two after. The digests, `captured` and both anchor uids are computed ([F08 §11.4]; the
symbol anchor's `captured` over the file uid above, the kind name, the scope bytes of [F08 §10.3.1], the three texts and
empty `end`, `occurrence` and window). While [F20 §6.1]'s interim scanner rule holds, a store holds such a `symbol`
anchor only by import (a capture refuses the `path::A/B` form); its bytes and lines are the same either way. The two lines
of the referrer's file, sorted by (dst uid, anchor uid), in `full` mode:

```
anchor 6b8c72caccb5d1cb7d3c9fc37f1ac98e -> d706fcde60f0b6cbf56c23297162b652 kind=file mode=live watch=header blob=sha1:de177738b58e970465382658e69b18745029e248 captured=bcf306557fb04ecf570128703b7ac5fe v=1
anchor a11bbeaab98f3fa9a695506f8ded56e0 -> d706fcde60f0b6cbf56c23297162b652 kind=symbol mode=live watch=header scope="rust:struct LockFile/impl LockFile/fn acquire" quote_h=3e747c4056197b57203a29bb675bdccf prefix_h=2622bf4975bc646f601cc36a812296a0 suffix_h=1144314642f739afb24bcd5aeaec0b44 quote="pub fn acquire(&self, timeout: Duration) -> Result<Guard>" prefix=" Blocks until the byte is ours.\n" suffix=" {\nlet mut spins = 0u32;\nloop {\n" hint=88-131 window=AwACAB86Apx9DrRRyNI span=xxh3:9c1f0a2b3c4d5e6f blob=sha1:de177738b58e970465382658e69b18745029e248 git=sha1:75bea42e34a5942eb6593b2867480e8fbc95eed1 captured=f6ba05cb0b8d5577c95cd8082b4367c4 v=1
```

The same symbol anchor in `hash-only` mode, which yields the same selector block and the same commit id ([F07 §8.3]):

```
anchor a11bbeaab98f3fa9a695506f8ded56e0 -> d706fcde60f0b6cbf56c23297162b652 kind=symbol mode=live watch=header scope="rust:struct LockFile/impl LockFile/fn acquire" quote_h=3e747c4056197b57203a29bb675bdccf prefix_h=2622bf4975bc646f601cc36a812296a0 suffix_h=1144314642f739afb24bcd5aeaec0b44 hint=88-131 window=AwACAB86Apx9DrRRyNI span=xxh3:9c1f0a2b3c4d5e6f blob=sha1:de177738b58e970465382658e69b18745029e248 git=sha1:75bea42e34a5942eb6593b2867480e8fbc95eed1 captured=f6ba05cb0b8d5577c95cd8082b4367c4 v=1
```

The window value is `03 00 02 00 1F 3A 02 9C 7D 0E B4 51 C8 D2` ([F20 §2.7.3]: `n_before` 3, `n_after` 2, then the five
hashes as little-endian `u16`), whose base64url is `AwACAB86Apx9DrRRyNI`.

### 17.3 A tombstone

Task `#40` deleted without `--replaced-by` while it blocked `#12`, at `nodes/01/8f/018f3c2e7a117b3c9d5e4c2f1a0b9e40.moi`:

```
moirai-node 1
uid: 018f3c2e7a117b3c9d5e4c2f1a0b9e40
kind: task
title: Reader registry
deleted: ca0f95ceb140682ca1c5c708146ed5080c9dbee17d53c7bd9eb509b1207f4f7ad 2026-09-26T09:12:40.118Z
field reason: dup of the lock protocol task
edge blocks -> 018f3c2e7a117b3c9d5e4c2f1a0b9e77 flagged
edge cites -> 018f3c2e7a117b3c9d5e4c2f1a0bd3a5
edge mentions -> 018f3c2e7a117b3c9d5e4c2f1a0b9e52
```

After an import, `#12` has `has_dangling` and stays out of `ready` until the flagged edge is resolved (I39′, CM4).

### 17.4 Conflict lines

A merge left a scalar conflict, a status fork and a body conflict on one task (sorted by key):

```
conflict body class=TextHunk base="Pairs are batched per frame.\n" ours="Pairs are batched per archetype.\n" theirs="Pairs are batched per grid cell.\n"
conflict field.estimate class=FieldEdit base=3 ours=5 theirs=
conflict status class=StatusFork base=in_progress ours=done/completed theirs=cancelled/obsolete
```

The file has no `---` section, no `field estimate:` line and no `status:` line while these exist. `theirs=` is absent: the
src side removed the estimate.

### 17.5 A named-query file

The query `lane_ready` is stored at `schema/queries/40cad467926a6dce0d6b5b93a4b8d71c.moi` (computed: the first 16 bytes of
BLAKE3-256 of `lane_ready`):

```
moirai-query 1
name: lane_ready
lq: 1
params: $scope: node = #u:018f3c2e7a117b3c9d5e4c2f1a0b9e09, $limit: int = 20
shape: node
budget: light
---
DEFINE QUERY lane_ready($scope: node = #u:018f3c2e7a117b3c9d5e4c2f1a0b9e09, $limit: int = 20) SHAPE node BUDGET light AS {
  MATCH (t:task)
  WHERE t.ready AND t IN subtree($scope)
  RETURN t ORDER BY t.priority, t.id LIMIT $limit
}
```

### 17.6 Commit messages

A local `ordinary` commit on `lane/l5np` made by `claim` through a lease, inside a git worktree, with an idempotency key:

```
claim --start

Moirai-Commit: cc4e084fb20d5ba50ba22ca5874d846c8ccda0ca92161fac5ae87e00eb07440df
Moirai-Kind: ordinary
Moirai-Ref: lane/l5np
Moirai-Hlc: 117336598351118336
Moirai-Actor: dev#1
Moirai-Role: developer
Moirai-Session: claude:s1
Moirai-Git-Head: sha1:75bea42e34a5942eb6593b2867480e8fbc95eed1
Moirai-Git-Branch: lane-l5np
Moirai-Worktree: D:/work/demo-lanes/l5np
Moirai-Git-Base: sha1:355b6ed5b782f6c37af0af19ff53ee150bdc8736
Moirai-Schema: 1
Moirai-Ops: 1
Moirai-Idem: 6653a45d135b95e5b4266fe14c279747
```

The author and committer lines of that commit are `moirai/dev#1 <developer@moirai.invalid> 1790414403 +0000`
(`floor((hlc >> 16) / 1000)`, §10.2).

A checkpoint commit of `main`:

```
checkpoint main

Moirai-Kind: checkpoint
Moirai-Head: ca0f95ceb140682ca1c5c708146ed5080c9dbee17d53c7bd9eb509b1207f4f7ad
Moirai-Ref: main
Moirai-Folded: 12 commits from cc4e084fb20d5ba50ba22ca5874d846c8ccda0ca92161fac5ae87e00eb07440df to ca0f95ceb140682ca1c5c708146ed5080c9dbee17d53c7bd9eb509b1207f4f7ad
```

### 17.7 `meta.moi`

```
moirai-meta 1
store-id: 0123456789abcdef0123456789abcdef
moi-format: 1
anchor-text: full
last-export-seq: 4471
ref lane/l5np checkpoint
ref main checkpoint
ref tags/v1 checkpoint
```

## Coverage

| Item | Part covered here | Section |
|---|---|---|
| [60 §2.5] [AR] row "Image format v1" (`.moi` ABNF, `.moirai-image` marker, trailer set, side-ref layout) | complete | §3–§10, §14 |
| [60 §2.5] issue-2 row "Gate-0 carrier table" (canonical item → carrier for every hashed item, a fixture per commit kind, R4 and R5 items) | complete: the table of [F07 §14] completed with every row kept; the byte encodings of the items are [F07]'s | §12 |
| [60 §2.5] audit row "Image format v1" (digests on every `anchor` line, text in `full` mode verified on import, `text-unavailable`) | complete | §6.7, §9.2 |
| [60 §2.5] audit row "Canonical form" ("inside a uid, key classes in `.moi` line order") | the `.moi` line order and its map to key classes; the entry order is [F07 §10.3]'s | §6.12 |
| [60 §2.5] [AR] rows "`HEAD`" (`image_cursor`) and "Log" (`GitMap`) | only the naming and numbering of destinations; the fields are [F04]'s and [F05]'s | §13.2 |
| [40] R-1 (`path`, `oid`, `pathmove`; the root by name) | the image text forms; stored encodings [F06], [F08]; canonical [F07] | §5.1, §5.2 |
| [40] R-2 (artifact field set, `planned`/`removed`, `observation`, `identity`, `area` fields) | the image field names, statuses and the observation conflict key; the data model is [F08]'s | §6.8, §6.11 |
| [40] R-3 (derivations, predecessor, dead uids) | import-side recomputation and the foreign rule; the derivations are [F08 §11]'s | §9.3 |
| [40] R-4 (`at`, discriminator, anchor record) | the image form of `at` edges: `anchor` lines keyed by the anchor uid; the record is [F06]'s and [F08]'s | §6.7 |
| [40] R-5 (root node; no op, item, trailer or annotation) | the root-node file; no trailer or annotation | §6.11, §12.3 |
| [40] R-10 (digests `quote_h`, `prefix_h`, `suffix_h`, `end_h`) | their carriers on every anchor line; the selector block is [F07 §8.2]'s | §6.7, §12.2 |
| [40] R-11 (`anchor` lines with digests, `end_h`, texts in `full` mode, `pred=`, artifact field names, `planned`/`removed`, `pathmove` blocks, `text-unavailable` covering `end`) | complete, with `occurrence=`, `marker=` and the meaning of `v=` | §5.4, §5.6, §5.7, §6.7, §6.11 |
| [40] R-12 (I-F1…I-F14) | I-F2 on import (recomputation, foreign flag), I-F4 (never exported), I-F8 and I-F9 checks on import; the statements are [F18]'s | §9.2, §9.3, §12.5 |
| [40] R-13 (`image.dest.<name>.anchor-text`) | the meaning of the two modes in the image and the mode's record in `meta.moi`; the key is [CFG]'s | §6.7, §14.1 |
| [40] R-16 (`text-unavailable`) | the sub-state on import; the string is [F18 §4.6]'s | §6.7 |
| [40] R-17 (`relink`) | the `field relink:` line and `ImageParse` on a value outside the grammar; the grammar is [F18 §5]'s | §6.11, §9.2 |
| [50] F3 (`schema/queries/<q>.moi`, portable form) | complete for the image: the file name, the ABNF, the parameter signature, the re-binding check, the conflict form; the item is [F08]'s, its hash [F07]'s | §7.2 |
| [50] F10, F14, F16 | not exported | §12.5 |
| [80] X-F9 (P11 (a) hashed query file names; P11 (b) ref names) | (a) complete; (b) only the image-side mapping and `doctor image`'s report; the rule is [F12]'s | §7.2.1, §13.1 |
| [80] X-F7 (P1, P12 path text) | path values in the image are exact `/`-separated bytes, `abs` in the P12 form; the rules are [OS/path]'s | §5.2 |
| [90 §10.1] row "Commit header" (`actor_src`) | not exported | §12.5 |
| [AR §11] #33 and OQ-A-7 (bodies droppable by hash without changing commit ids; spec sync 3) | the image side of [F06 §8.1] DB-11: the `--- dropped` line, dropped conflict sides and snapshot lines, T(C, d) as a function of the dropped set, reuse after a drop, reconstruction and the import rule, the carrier, gate-0 and `moi/` rows | §3.3, §6.1, §6.8, §6.8.1, §6.9, §6.12, §9.1, §9.2, §11.1, §11.2, §12.2, §12.4–§12.6, §15, §16 |
| [AR §11] OQ-A-9 (the provenance field; spec sync 3) | not exported | §12.5 |
| [AR §11] OQ-A-10 (the harvest cursor; spec sync 3) | not exported | §12.5 |
| [60 §3.1] item 1 (the `.moi` v1 ABNF with golden files for every node kind, a tombstone with flagged and historical edges, scalar and body conflicts, ledger lines, block strings, file nodes, anchor lines, named-query items) | the grammar and the catalogue WP-21 writes the `moi/` and `carrier/` fixtures from | §5–§8, §12.6, §16 |

No other X-F item and no other [90 §10.1] item is specified here.

## Holes

None. No value of this chapter is decided by an M0 measurement or benchmark. Values that other chapters leave to
measurements change the length of some image values but no rule here: the window value's length (HOLE(F20-window-lines)),
the quote and context lengths ([F20 §6.1]); the codec holes of [F10] never reach the image, which carries raw bodies.

## Open points for the review

1. **[PLAN §3.3] gap "`.moi` anchor-line grammar" (WP-15), closed** (§6.7): digests on every line whose kind carries a
   quote (`quote_h`, `prefix_h`, `suffix_h`) or an end (`end_h`), in both modes; the texts only in `full` mode, all or none;
   `occurrence=` after the texts and `marker=` after `pred=`, so that the hashed properties are in [F07 §8.2]'s block order;
   `v=` is the anchor's `resolver` version at capture (a hashed selector field), not a line-format version; `blob=` is
   optional (see 12).
2. **[PLAN §3.3] gap "fixing the [AR §5b.2] example ids to 64 hex", closed** (§17.1). The example's commit ids have 63
   hexadecimal digits after `c`; §17.1 gives the corrected example, which also writes `priority: P1` and drops
   `criticality: normal`. WP-81a should replace [AR §5b.2]'s example with §17.1's. [AR §5b.3]'s `schema-version: 3` is an
   illustration; format v1 writes 1 (§4).
3. **Stale [60 §2.5] copy of R-11** ([PLAN §3.3] last row): [60 §2.5]'s R-11 row lacks `end_h`, `pred=` and the
   `text-unavailable` sub-state covering `end`; this chapter follows [40 §2.11] R-11 as revised by S-03 and S-04.
4. **`status:` is always written, other defaults never** (§6.2). [40 §5.7]'s file-node and root-node examples write the
   initial status (`present`, `active`), so every live file shows its status; the canonical form treats the initial status
   with `none` as absent ([F07 §6.3]) and the importer maps it. Every other header enumeration and every field is omitted at
   its default, which [AR §5b.2]'s example breaks once (`criticality: normal`).
5. **Enumerations by name** (§5.1): `priority: P1`, not [AR §5b.2]'s `priority: 1`. Project enumeration integers are
   store-local ([F08 §8.3]), and one rule for every enumeration keeps the codec generic.
6. **Domain times as decimal seconds** (§6.4). [F08] types `defer_until`, `due`, `since`, `review_after`, `started` and
   `ended` as `int` Unix seconds; the image writes them as integers and uses [AR §5b.2] rule 4's RFC 3339 form only for the
   provenance lines. Alternative considered: RFC 3339 for `coerce = timestamp` fields, rejected because the parse would then
   depend on a schema column that a strengthening can change.
7. **Conflict keys and sides** (§6.8). Added to [AR §5b.2] rule 3's key list: `observation` ([F07] open point 8),
   `edge.at.<dst>.<anchor>` (anchor selectors), and `field.<name>` for every field storage, `field.title` included
   ([RULES/merge-table] open point 11). An absent side is empty; structured sides (anchor, observation, existence) are JSON
   strings of the lines that would carry them, so the codec has one line grammar.
8. **Existence conflicts and snapshots** (§6.8.1): a provisionally deleted node is a tombstone file with a `conflict
   existence` line, and a live side carries its node image, the body as text ([RULES/merge-table] open point 5 (d), [F06]
   open point 13). The snapshot carries body bytes, not only the hash, so that the importer can store the body a `resolve
   --take` restores ([F06 §8] BD-4).
9. **Tombstones may carry `conflict` lines** (§6.10): the existence conflict and conflicts on retained edges. [AR §5b.2]
   rule 8 lists no such line.
10. **The scope text** (§5.6) is a bijection with [F08 §10.3.1]'s structured value, which the one anchor record of
    [F08 §10.3] stores and [F06 §7.5.3] cites — closed in pass 1 (A1-2, S1-3).
11. **Anchor texts that are not UTF-8** (§5.7) are written as `%` and base64url. Text content (`is_text`, [F20 §2.1]) may
    be Latin-1 or another encoding, and a `.moi` file is UTF-8; without this form a `full` export would have to drop such
    texts. Closed in pass 1: [F08 §10.3] stores the texts as `vbytes` (S1-3).
12. **[F06]/[F08] divergences the image absorbed** — closed in pass 1 (A1-2, S1-3): [F08 §10.3] is the one record,
    `blob` is `none` only for a planned `file` anchor, `hint`, `window` and `span` presence is kind-driven, and the image
    writes kinds and `pathmove` classes by name.
13. **`commitref` and `pinned_commit` need 32 bytes** (§5.1, §6.6) — closed in pass 1 (A1-1, A1-3, S1-2): [F08 §5.1]
    and §10.2 store the full id, and [F09 §7.2]'s `EDGE_PROPS` row keeps all 32 bytes, so a store rebuilt from segments
    writes `c<64 hex>` ([AR §5b.2] rule 4) and hashes the full id ([F07 §8.1]).
14. **Empty sets** (§5.1): the image writes none, following [F08 §5.3], §6.2 and [F07 §7.1]; `[]` stays admitted on import
    and reads as absent. No store holds an explicit empty set since pass 1 (A1-1).
15. **New trailer `Moirai-Parent`** (§10.4). [AR §5b.4] carries item 2 by "the git parents, each mapped to its own
    `Moirai-Commit` trailer". That fails for a child of a demoted parent re-exported to another destination: the parent is
    written there natively with its foreign id, which differs from the child's stated id, and the child would be demoted in
    every further store. `Moirai-Parent: <i> c<64 hex>`, written only when the stated id differs from the git parent's
    `Moirai-Commit`, gives item 2 a carrier in every case (I38′). [F07 §12.2] and §14.1 row 2 should cite it. [F06] open
    point 2 ("confirm that the carrier table needs nothing else unhashed") is confirmed: `ckpt` and `xtr` suffice for the
    unhashed trailers.
16. **The unborn-repository algorithm** needs no carrier: [F07 §3.6] does not hash it, so the image writes no trailer for it.
17. **One trailer order** (§10.5) merges [AR §5b.4]'s native and checkpoint lists; a native re-export of an import-checkpoint
    commit carries `Moirai-Head` and `Moirai-Folded` from `ckpt`; `Moirai-Sync-Base` is written for `sync` only (not for
    `merge`, whose [F06] `sync_base` is not carried) and must equal the second parent's stated id; `Moirai-Ops` is
    [F07 §10.4]'s entry count. The importer accepts known trailers in any order; an unknown one makes the commit foreign, a
    malformed or repeated known one stages `ImageParse` ([F07 §15]).
18. **Trailer values** (§10.4) are extracted byte-exactly after `: ` ([F07] open point 22); a value that is not bare-safe
    (a leading or trailing SP, a leading `"`, a control character) is a JSON string, so no trailing space ever sits at the
    end of a git message line, where git tools trim it.
19. **Checkpoint commits** (§10.7): message `checkpoint <R>`, author `moirai/checkpoint` with the head's time, parent the
    destination ref's current commit, and `Moirai-Folded` over H(H) minus H(G). For n = 0 (after `undo`), the trailer has no
    `from … to …`, and [F06 §4.4.11] allows all-zero `ckpt.first` and `ckpt.last` exactly when `n_folded` = 0 (done in
    pass 1, S1-23). Gate 2's "commit objects differ
    only in the import-checkpoint ids and parents" also covers `Moirai-Folded`'s count, which a re-exporting store computes
    over its own history.
20. **Tags at checkpoint granularity** (§10.7): [AR] does not say what a tag points to when its commit was folded. A tag
    whose commit is some checkpoint's head points at that checkpoint; otherwise it gets its own checkpoint commit. The
    importer lands a tag without a parent as a root import-checkpoint (a bulk commit); a review may prefer a parent search.
21. **`refs/heads.moi` and `refs/tags.moi` hold one row**, the checkpointed ref (§8), so that a checkpoint tree is a
    function of its ref alone: a row per exported ref would change every ref's next checkpoint whenever a lane is created.
    The row is informational; the ref name and kind come from the git ref ([F07 §14.3]).
22. **Image-only data after a checkpoint import** (§11.3) — closed in pass 1 (S1-23, A1-10): an import-checkpoint
    commit records, per node file it brings in, the file's `created:`, `updated:` and `deleted:` values and its ledger
    lines (tokens as written), unhashed: in `ckimg` ([F06 §4.4.14]) when inline, in its `cs.<n>`'s `CKIMG` section
    ([F09 §16.4]) when bulk. An exporter that re-exports the commit's tree writes these values instead of deriving them
    from its own history, so gate 2's byte-identical head trees hold.
23. **Provenance lines are functions of the commit graph** (§6.3): `created` by [40 §2.3]'s least (generation, commit id)
    rule, `updated` as the last first-parent commit that changed the file, `deleted` following the side that deleted the
    node. [F08]'s `created_tx`/`updated_tx` are store-local `seq`s and are not what the image writes.
24. **The ledger is one line per stored `Incr` on the first-parent chain** (§6.5). [AR §5b.2] rule 3's "one per `Incr` op"
    is read over the view's first-parent chain: a merge's or sync's net delta is one line with the merge's id, so the lines
    always sum to the counter, stay append-only along a ref, and do not depend on a merge base. A foreign commit's
    hand-written tokens are summed and replaced by its own id in the store; an import-checkpoint keeps its imported lines
    (point 22).
25. **A tombstone from the absent state** (§11.2) is stored as [F06 §7.4]'s `CreateDeleted` (NF-11) — closed in pass 1
    (S1-6, A1-4).
26. **T(C, d) and subtree reuse** (§3.3): a subtree is reused only when it equals T(C, d); after a foreign parent the files
    it changed are re-encoded and entries outside the layout dropped; after an anchor-text mode change every file with an
    anchor line is re-encoded. This keeps [CFG]'s `hot` reload class for `anchor-text` compatible with I28′.
27. **Entries outside the layout** (§3.4): ignored at the root, `ImageParse` under `nodes/`, `schema/` and `refs/`.
28. **Named-query files** (§7.2): the stored text is the whole `define_stmt` ([LQ/lexical §10.2], [LQ/canonical-ast §8.1]),
    so `name:`, `params:`, `shape:` and `budget:` are hashed copies the importer checks against it; `params:` is omitted for
    a query without parameters (refining [50 §4.4]'s sketch, which would leave a trailing SP); the name's bytes are its
    canonical `qname` spelling, which [LQ] and [F08] should confirm; a `q` collision is refused at `DEFINE`. A definition
    without `SHAPE` stores `table` ([LQ/std §2.3]; spec sync 2b), the one shape that renders every projection
    ([LQ/envelope §5.1]), and one without `BUDGET` stores `medium`.
29. **Schema tables** (§7.1): enumeration values live in `fields.moi` ("fields with type and lattice order", [AR §2.12]); only
    the view's project items are exported; list-valued properties are comma-separated without SP so that rows stay one
    token per property.
30. **Destinations** (§13.2): numbers declared by `GitMap` records, never reused; the object format and kind recorded by the
    destination repository itself; the anchor-text mode and format version in `meta.moi`.
31. **The side ref is a parentless commit** rewritten per run (§14.1), so it adds no history; its name carries the store id,
    so gate 1's byte identity covers the hashed objects and the image refs, not the side refs.
32. **The alias table fans out** (§14.1; pass 1, P1-17). A single `aliases.moi` ([AR §5b.1] names one file) would be
    rewritten and hashed whole at every run: ≈ 4.2 MB raw at 1e5 nodes (42 MB at 1e6), which the ≤ 50 ms incremental
    checkpoint export of [AR §5b.9] cannot absorb, and the side-ref layout freezes at M0, before M5 could measure it. The
    side ref therefore holds `aliases/<h1>.moi`, 256 blobs keyed by the uid's first byte, and a run rewrites only the
    prefixes it touched. This changes [AR §5b.1]'s file name: recorded for the owner in `reviews/owner-questions.md`
    (sign-off, then the [AR §5b.1] and §5b.6 step 4 texts at WP-81a); if the owner keeps one file, §14.1 reverts to
    `aliases.moi` with the same row grammar. P1-17 also asks that measurement 8 time the side-ref write; the measurement
    list is [60 §5.2]'s, so WP-81a adds it there.
33. **`refs/moirai/ops/<store-id>`** (§14.2) is given a layout here because the side-ref layout is frozen at M0; it carries
    no directory path, file id or binding field.
34. **Foreign messages** use [F07 §5.3]'s N_imp, including the U+FFFD substitution and the 65,535-byte cut; a demoted
    commit's message keeps its trailer block ([F07 §12.5]).
35. **More than two git parents** stage `ImageParse` ([F07] open point 17, confirmed).
36. **The foreign-uid mark is not encoded** (§9.3): the mismatch is recomputed from stored inputs on every import, so no
    line is needed. Closed in pass 1 (A1-44): [F18 §2.2] now says no chapter encodes the mark.
37. **The `f64` text form** (§2.8) is [ECMA-262]'s `Number::toString` with `.0` appended, the rule [LQ/envelope §5.2]
    already uses; [AR §5b.2] rule 4's "Ryu" names the digit algorithm, which gives the same digits.
38. **Tree mode bytes** (§3.2): git writes directories as `40000` in tree objects; [AR §5b.5] rule 1 and [80 §3.2] quote
    `040000`, which as stored bytes is `git fsck`'s `zeroPaddedFilemode`. WP-81a should correct both texts.
39. **Author identities** (§10.2): the bytes `NUL`, LF, `<` and `>` are replaced by `_`; an empty role gives
    `@moirai.invalid`, which `git fsck` accepts. The lines are never read back.
40. **External references**: [RFC 8259] (JSON), [RFC 4648] (base64url), [RFC 3339] (timestamps) and [ECMA-262]
    (`Number::toString`) are cited here and should join [F01 §2.2]'s list of external specifications.
41. **Paths carry their root by name** (§5.2): implied for the artifact and root-node fields whose root equals the node's
    `root` (I-F8), explicit `<root>:` elsewhere, as [F07 §16]'s `project:docs/a.md`.
42. **`span=`** writes the XXH3-64 value most significant digit first, as a number, following [40 §5.7]'s `xxh3:` example;
    the stored and hashed forms are the little-endian value ([F01 §7.2], [F07 §8.2]).
43. **`hint=`** is always `first-last`, a one-line hint `n-n`, so each hint has one form.
44. **Foreign ledger lines, provenance lines and hand-made files** are accepted (§9.1 items 7–8, §6.5) and normalised on
    re-export; foreign commits are the only way non-canonical bytes enter a destination, and §3.3 keeps them out of every
    later native tree.
45. **`IdCollision`** reads the `created:` line's commit id; since the line is informational for derived-uid kinds
    ([40 §2.3]), `IdCollision` applies to random-uid kinds only (§9.3).
46. **`Moirai-Sync-Base` and the marker** must agree with the second parent and `Moirai-Schema`; a disagreement stages
    `ImageParse` rather than demoting, because neither is hashed ([F07] open point 18).
47. **Deleted refs**: an export never deletes a destination ref (§13.1); `image doctor` lists refs whose moirai ref is gone.
    The design says nothing; deleting would lose the off-store copy CM8 keeps.
48. **Model functions** proposed for `COVERAGE.md` (R-MODEL decides): the model has no `.moi` bytes ([60 §4.2]); its image
    functions are `image::export_set`, `image::anchor_text_on_import`, `image::import_merge` ([CFG §10.10]) and [F07]'s
    `canon::i38p_gate0`; the ABNF conformance check of this chapter is the format oracle's (WP-95).
49. **The length of `q`** (§3.1, §7.2.1). [60 §2.5]'s F3 row writes "q = hex BLAKE3-256 of the name" without a length;
    [50] F3, [50 §4.4], [80] X-F9 and [AR §5b.1] fix the first 32 hexadecimal digits. [50] and [80] own this reservation,
    so the file name has 32 digits; [60 §2.5]'s row should say so at WP-81a.
50. **The foreign file removal keeps X4** (§11.2): a git-side deletion of a live node file becomes a tombstone whose
    retained out-edges follow the edge kinds' `on_src` policies, a blocker's `blocks` edges flagged. [AR §5b.6] step 2 names
    the op but not its retained edges; without this rule a hand deletion of a blocker would unblock its dependents silently.
51. **Spec sync 2b** (WP-21 carrier and `moi/` findings M-1 to M-9 and C-2 to C-4; WP-95 decoders and conformance).
    The `pathmove` block is sorted by its lines' bytes, as every set is (§5.4). A header-key production separates an
    unknown header key from a line with no production (§6.1). The fifteen derived-state names are listed, so a derived
    field (a line not allowed) and an unknown field differ by name (§6.2); the alternative, the core fields of merge class
    `derived`, names no field and would leave `neg/line-not-allowed--derived-field` without a rule. An empty `status` side
    is read as absent (§6.8, §9.1 rule 7). `edge.at` sides carry the texts as the line does; an empty observation side
    is the absent composite (§6.8). `title:` sorts among the snapshot's field lines; the file's form carries an existence
    conflict's `prov` (§6.8.1, §12.2). A definition without `SHAPE` stores `table` (§7.2.2, [LQ/std §2.3]). Set and
    `pathmove` elements are re-sorted and a repeat stages `ImageParse`; a symmetric edge is read from either endpoint's
    file (§9.1 rules 9–10, §9.2). The `Moirai-Ops` demotion is in §10.9 with a carrier case (§12.6), and a native twin of
    a held commit appends nothing (§10.9). A foreign file removal applies `repoint-or-flag`'s default policy, never the
    store's configured one (§11.2). After the independent check of the sync: `pname` is [CFG §3.3]'s key name in its
    canonical form (lower-case segments that start with a letter or digit, at most 16 segments and 255 bytes), the form
    [F12 §6.6]'s `policy-name` and [F08 §8.2] use, instead of a production that allowed a trailing or repeated dot; the
    `policy.moi` rows join §7.1's file list, its conflict bullet, §9.2's repeated lines and §16's catalogue.
52. **Spec sync 3** ([AR §11] #33, OQ-A-7 (a), OQ-A-9 (a) and OQ-A-10 (a), decided 2026-10-06; [F06 §8.1] DB-11).
    (a) **The dropped-body line.** A node whose body the exporting store has dropped ends with
    `--- dropped <hash> <reason>` in place of the body section (§6.9): the hash is the body key, so item 10 and every
    commit id are unchanged and a native commit still verifies; the reason is carried, not hashed (§12.4). The line
    shares the body's `---` prefix so that it sorts and parses where the body does, and it cannot be mistaken for a
    body, which always starts with a line that is exactly `---`. (b) **Dropped sides.** A body conflict side and an
    existence snapshot's `body` line are written `dropped:<hash>:<reason>`. R-SPEC-F's design wrote `dropped:<hash>`
    without the reason; the reason is added because an importer that meets only a dropped side must still append a
    `BodyDrop`, whose `reason` must be 1–3 ([F05 §9.29]), and a fixed fallback would lose the exporting store's
    reason on a round trip (gate 2). A bare token cannot be a body text, which is always a JSON string, so the forms do
    not collide. (c) **T(C, d)** depends on the exporting store's dropped set (§3.3, §15 rule 9; [F13] I28′); a subtree
    written before a drop is re-encoded, and the resulting tree change carries no item-10 entry. Gate 2 still holds
    for an image that one export wrote to a new destination: its trees all carry the dropped form, so it supplies no
    dropped hash's bytes, and a fresh store that imports it records every dropped hash with the image's reason and
    re-exports the same lines; an image that gives one hash two reasons (only a hand edit can) re-exports the first
    one. An image that a destination accumulated across a drop — trees exported before it, which [F06 §8.1] DB-10
    leaves in place, beside trees exported after it — does supply the bytes: a fresh store that imports it keeps them,
    records no drop, and writes the body section on re-export, until its owner drops the body there. (d) **Import.**
    No bytes are stored for a dropped line or side. A hash the store neither holds nor has dropped, and whose bytes the
    import supplies nowhere, gets a `BodyDrop` of origin `import` (the line's reason, an empty note, the importing
    command's actor; at most 2,048 hashes a record so that each group fits [F17 §4.4] W3) before the first imported
    commit that names it. A hash the store holds keeps its bytes, since only the owner's drop drops held bytes
    ([F06 §8.1] DB-3, [F05 §9.29] "Origin"); bytes the import supplies in some commit are carried by the first imported
    commit that names the hash (BD-4); bytes whose hash the store has dropped are discarded without refusing the import
    (§11.2). The first text of §11.2 recorded every dropped-line hash the store had not dropped, held ones included,
    which let any import drop bytes its store holds, against [F05 §9.29], [F06 §8.1] DB-3 and DB-11 and [F13] EP-IM;
    the closure check of spec sync 3 aligned it.
    (e) **Never exported** (§12.5): the provenance group of [F06 §4.4.17] (named so, since this chapter already uses
    `prov` for a production and for an existence conflict's side), the `DROPPED` table's lsn and HLC and the drop
    record's note and actor, and the harvest cursor. (f) New fixture rows for `moi/` and `carrier/` (§12.6, §16); no
    existing fixture byte changes, since none holds a dropped body.
