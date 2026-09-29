# fixtures/canonical: canonical-form fixtures

| | |
|---|---|
| Title | The golden fixtures of the canonical form: the byte stream of item 10 (`changeset_digest`) and of the commit-id input C for every commit kind, the full and hash-only anchor forms, message normalisation, defaults and key order |
| Work package | WP-21, first part: `canonical/` (R-FIX; [PLAN §3.2] item 1). `r4/` is the other half of this part; `carrier/` and `moi/` are the second part (`fixtures/carrier/INDEX.md`, `fixtures/moi/INDEX.md`) |
| Acceptance | E3: WP-91 (the reference model's independent encoder) reproduces every `changeset_digest` and `commit_id` here |
| Separation | S1 ([PLAN §3.1]): written from the specification text only. The author read no line of `moirai-format-oracle`, `moirai-model`, `moirai-toylog` or any product crate, and ran no project code. Every hash was computed by throw-away scripts that implement only what the chapters say; the scripts' BLAKE3 was checked against the BLAKE3 reference implementation (the C code published with the `blake3` crate) over the standard test-vector inputs, and against [F07 §16]'s empty-input digest. The scripts also reproduce every informative byte example of [F07 §16], [F08 §5.6] and [F08 §11.2] |
| Sources | [F07] (normative for every byte here); [F01 §5]–§7 (integers, `lp()`, hashes); [F06 §3], §4.4.4, §6–§7 (kinds, the foreign `hlc`, key values, ops); [F08 §5], §6, §9–§11 (values, defaults, the core schema, edges, anchor records, derivations); [F12 §6]–§7 (conflict classes, provisional values, the merge contract); [RULES/merge-table] (MR, DM, RS rows) |
| Status | Written against the specification after review pass 1 (owner answers of 2026-09-28). A specification change that moves a byte updates the cases it touches (commit subject `WP-21:` or `WP-73:`) |

The consumers are the reference model's encoder and commit ids (WP-91), later the engine's canonical encoder (M1–M3), and
WP-21's second part, which re-expresses these commits as `.moi` trees and trailers (`carrier/`, [F07 §14]). Every value
is synthetic: no owner data, no real paths, users or hosts.

## 1. Files

| Path | Cases | What it asserts |
|---|---|---|
| `cases/commits.cases` | 11 | One synthetic history (§5): the store's root commit, an ordinary commit with git provenance, a revert, a cherry-pick, a merge with value conflicts, a sync whose item 10 is larger than its residue, and the lane commits they build on |
| `cases/tombstones.cases` | 2 | A delete with a flagged and a historical retained out-edge; the undelete of the same node, written as the revert of the delete |
| `cases/anchors.cases` | 3 | A file node, a root node and two anchors (a `quote` anchor with an empty prefix, a `range` anchor), held with their texts and as digests only: same entries and same id; texts added to hash-only anchors make no entry |
| `cases/foreign.cases` | 4 | Foreign commits: a root, a child with an ill-formed message byte, a child whose committer clock is behind its parent's, and a two-parent commit whose item 10 comes from the typed merge |
| `cases/checkpoint.cases` | 3 | Import-checkpoint commits: a first checkpoint (no parent) that is also the key-order case, the same checkpoint with a symmetric edge written from the other end, and a checkpoint with a previous checkpoint as parent |
| `cases/normalisation.cases` | 27 | Explicit defaults against omitted ones (same id); 25 cases of `N` and `N_imp` ([F07 §5]) |
| `cases/values.cases` | 60 | Unit encodings: every `cv` tag, the per-class state values, counter deltas, `cstate`, node images, selector blocks and schema items |

`.gitattributes` gives `fixtures/** -text`: every byte of these files is kept as written.

## 2. The case format

### 2.1 Framing

The framing is that of `fixtures/lq/INDEX.md` §2.1: a `.cases` file is UTF-8 text with LF line ends, a sequence of cases
`%% case <id>` … `%% end`; lines outside a case are comments (`#`). A **line directive** is `%% <name> <value>`; a **block
directive** is `%% <name>` alone on its line, and its block is every following line up to the next line that starts with
`%% `. Line directives marked repeatable may occur several times, in order. Only `source` and `note` lines and comments
hold bytes outside ASCII; every text inside a block is ASCII, with JSON `\u` escapes.

### 2.2 Directives

| Directive | Kind | Meaning |
|---|---|---|
| `source` | line, repeatable | The specification sections the case rests on |
| `note` | line, repeatable | Commentary |
| `p-state` | block | The state at the commit's **first parent** (§3); empty for a commit without a parent. For a merge, sync, revert or cherry-pick it is also **o**, the destination's state ([RULES/merge-table] §2) |
| `q-state` | block | The state at the commit (§3) |
| `base-state`, `theirs-state` | block | Merge-family cases only: **b** and **t** of the three-way merge that produced the q-state (the DM row in the notes says which states they are) |
| `merge-rows` | block | Informative: for each key the merge decided, the [RULES/merge-table] row or case that decided it (`ours-only`, `theirs-only`, `MR-009`, …); keys equal on all three sides are not listed |
| `residue` | block | Informative, `sync` only: the keys of the stored residue (the keys whose merged value differs from what `main`'s window alone yields, [F07 §10.5], [AR §4.6]), rendered as entries |
| `entries` | block | Informative: the entries of item 10 in their order ([F07 §10.3]), one per line, rendered as §4.1 says |
| `digest-input` | block, hex | Normative: `lp("moirai-changeset-v1") ‖ E_1 ‖ … ‖ E_n ‖ u64(n)` ([F07 §10.4]), one comment line before each part |
| `entry-count` | line | n |
| `changeset-digest` | line | BLAKE3-256 of the `digest-input` bytes, 64 hex digits |
| `git-commit` | block | Foreign and checkpoint cases: the git commit the items are derived from (§4.3) |
| `message-input-hex` | block, hex | `defaults-*` cases: the message as the caller gave it; `N` of it ([F07 §5.1]) is the stored message |
| `commit` | block | Items 1–9 as values (§4.2) |
| `c` | block, hex | Normative: the commit-id input C ([F07 §3.1]), one comment line naming each item |
| `commit-id` | line | BLAKE3-256 of the `c` bytes, 64 hex digits |
| `function` | line | `normalisation.cases`: `N` ([F07 §5.1]–§5.2) or `N_imp` ([F07 §5.3]) |
| `input-hex`, `output-hex` | block, hex | The message bytes before and after the function |
| `input-text`, `output-text` | line | Informative: the same bytes as a JSON string, when they are UTF-8 and short |
| `output-length` | line | The length of the output in bytes |
| `refused` | line | `N` refuses the message with this case of [F19] `bad_value` ([F07 §5.2], open point 15): `message-utf8`, `message-length` or `message-trailer` |
| `encoding` | line | `values.cases`: which encoding the input is given in (§4.4) |
| `input` | block | `values.cases`: the value in the notation of §3 |
| `hex` | block, hex | `values.cases`: the expected bytes |
| `length` | line | The length of `hex` in bytes |

### 2.3 Hex blocks

Hexadecimal digits in byte order, lower case; whitespace between digits is ignored; `;` starts a comment that runs to the
end of the line. A line of the form `<hex> * <n>` stands for n copies of the bytes `<hex>` (used for messages of 64 KiB).

## 3. The state notation

A state block describes the versioned state of one view as a set of nodes and schema items. It is this directory's
notation, not the image's: [F14]'s `.moi` form of the same states is WP-21's second part.

### 3.1 Tokens

A line is a sequence of tokens separated by one space. A token that contains a JSON string (RFC 8259, ASCII with `\u`
escapes) runs to the string's closing quote, so a string may contain spaces (`quote="a b"` is one token). `<uid>` is 32
lower-case hex digits ([F08 §2.2]); `<hex64>` is a full commit id; `-` is "none". Names (kinds, fields, enumeration values,
edge kinds, statuses, resolutions) are bare.

### 3.2 Nodes and keys

```
node <uid> <kind>                                   a live node
node <uid> <kind> deleted <reason-json> <uid|->     a tombstone: its reason and replaced_by (uid or -)
  status <status> <resolution>
  parent <uid|-> <order-json|->                     the hierarchy key: parent uid and order key
  field <name> <value>                              §3.3
  counter <name> <int>                              the counter's total
  body <json>                                       the body text (the key is BLAKE3-128 of its UTF-8 bytes)
  edge <edge-kind> <dst-uid> [pin <hex64>] [flagged]
  at <dst-uid> <anchor-uid> <anchor attributes>     an at edge key; the anchor uid is the discriminator (§3.4)
  conflict <key> <class> [prov ours|theirs]         §3.5
```

Key lines are indented by two spaces and belong to the node line above them. A tombstone has only `field title` lines
(the title kept at deletion; for an `artifact`, the last path text) and `edge` lines (its retained out-edges, [F07 §6.4]).

### 3.3 Values

```
absent
bool true | bool false
int <decimal i64>
f64 <16 hex digits>                  the IEEE 754 binary64 bit pattern, most significant digit first
enum <name>
text <json>                          the interned and inline forms of text are one value ([F07 §7.1])
set <elem> <count> <payload>...      elem: int enum text ref commit path oid pathmove; unordered here
ref <uid>
commit <hex64>
path <root> <json>
oid <algo> <hex>                     algo sha1 (40 hex digits) or sha256 (64)
pathmove <hlc: 16 hex> <class> <root> <from-json> <root> <to-json> <git>      git: - or <algo>:<hex>
```

A set payload is the value without its type word (two tokens for `path` and `oid`, seven for `pathmove`); the canonical
order ([F07 §2.4]) is the encoder's job. Fields typed `sym` ([F08 §5.1]) are written with `text` values.

### 3.4 Anchors

The attributes of an `at` line (and of an edge side `present anchor …`) are `key=value` tokens, the anchor record of
[F08 §10.3]:

| Attribute | Value |
|---|---|
| `kind`, `mode`, `watch` | names; `watch` is always written |
| `resolver` | decimal |
| `captured` | 32 hex digits ([F08 §11.4]); `pred=<32 hex>` when present |
| `scope` | the scope value's bytes in hex ([F08 §10.3.1]) |
| `quote`, `prefix`, `suffix`, `end` | the texts as JSON strings (a store that holds the texts) |
| `quote_h`, `prefix_h`, `suffix_h`, `end_h` | the BLAKE3-128 digests in hex (a hash-only store, `text_unavailable`) |
| `occurrence` | decimal |
| `hint` | `<first>-<last>` |
| `window` | the window value W in hex ([F20 §2.7.3]) |
| `span_hash` | 16 hex digits, the `u64` value most significant digit first |
| `blob`, `git` | `<algo>:<hex>` |
| `marker` | a JSON string |

An absent attribute is absent in the record. Where a case needs `window` and `span_hash`, they are stated values of the
record, not derived from any file content here: [F07] hashes them as bytes, and their derivation ([F20 §2.7]–§2.8) is
outside this directory.

### 3.5 Conflict values

```
  conflict <key> <class> [prov ours|theirs]
    base <side>
    ours <side>
    theirs <side>
```

`<key>` is `existence`, `status`, `hierarchy`, `field <name>`, `observation`, `body` or `edge <edge-kind> <dst-uid>
[<disc-uid>]`; `<class>` is a conflict class name of [F12 §6.1]; `prov` is given exactly on an existence key. A side is
written in the form of its key class:

| Key class | Side |
|---|---|
| existence | `absent`, `live <kind>` followed by its node image as lines indented by six spaces (`status`, `field`, `counter`, `body` or `body-hash <32 hex>`), or `deleted <kind> <reason-json> <uid|->` |
| status | `absent` or `<status> <resolution>` |
| hierarchy | `absent` or `<uid|-> <order-json|->` |
| field | a value (§3.3) |
| observation | `absent` or six values separated by ` | `: `path`, `oid`, `bytes`, `observed_git`, `observed_blob`, `relink` |
| body | `absent`, a JSON text, or `hash <32 hex>` |
| edge | `absent` or `present [pin <hex64>] [flagged] [anchor <attributes>]` |

A conflict line replaces the key's value: the node carries no plain line for that key. A node whose existence key holds a
conflict is written with its **provisional** existence ([F12 §6.3]): a tombstone line under `delete-wins`, and its other
keys as that side left them.

### 3.6 Schema items

```
schema field <kind|*> <name> type=<t> elem=<t|-> class=<merge class> storage=<s> decl=<n> optional=<bool>
       index=<i> coerce=<c> one_line=<bool> ascii=<bool> retired=<bool> [range=<min>..<max>] [default <value>]
schema query <name> lq=<n> params=<json> shape=<json> budget=<json> text=<json>
schema kind <name> retired=… uid_derivation=… root_variant=… existence_policy=… title_derived=… immutable_fields=… has_done=… done_derived=…
schema enum <kind|*> <field> <value> retired=… sort_rank=<n> side=… done=… covers=<name>,…|-
schema edge <name> retired=… eclass=… on_dst=… on_src=… acyclic=… card=… max_depth=<n> uid_derivation=… props=…
       symmetric=… same_kind=… lq_name=… src_kinds=any|<kind>,… dst_kinds=… reverse_names=<name>,…|- reading=<json>
```

(Each is one line.) Every item is a project item of [F08 §8.5] with its store-local parts omitted; `kind`, `enum` and
`edge` items occur only in `values.cases`.

### 3.7 From a state to its canonical state

A harness computes CS(V) ([F07 §6]) from a state block with the core schema of [F08 §9] and the block's schema items:

1. Each node line gives the existence key: `live(kind)`, or `deleted(kind, reason, replaced_by)`.
2. Each key line of a live node gives the key [F07 §6.2] assigns: `status` the status key, `parent` the hierarchy key,
   `field` a field key, `counter` a counter key, `body` the body key, `edge` and `at` edge keys owned by the source.
3. A value is **absent**, and makes no key, when it is empty (text `""`, a set of 0 elements, a body `""`, a counter of
   0), when it equals its field's default in the effective schema ([F07 §6.3]), and for the status line when it is the
   kind's initial status with resolution `none`. `f64` `8000000000000000` (−0.0) enters as +0.0.
4. A symmetric edge (`contradicts`, `relates`) is a key of the endpoint whose uid is bytewise smaller, whichever node line
   it is written under ([F07 §6.6]).
5. A conflict line gives the key's value; while an observation key holds one, the six member field keys are absent.
6. A tombstone's keys are its existence key, its title and its retained edges ([F07 §6.4]).

## 4. Entries and commits

### 4.1 The `entries` block (informative)

One line per entry, `<key> = <value>`, where `<key>` is `<uid> existence|status|hierarchy|observation|body`,
`<uid> field <name>`, `<uid> counter <name>`, `<uid> edge <edge-kind> <dst-uid> <disc|->` or `schema <item class> <key
components>`. A counter's value is its signed delta; a conflict value is `conflict <class> base (<side>) ours (<side>)
theirs (<side>) [prov <side>]`, a node image in brackets; a schema value is `present` or `absent`. The `digest-input`
block is normative: each entry's bytes follow the comment line that names its key.

### 4.2 The `commit` block

| Line | Item ([F07 §3.1]) |
|---|---|
| `kind <name>` | 1 |
| `parent <hex64>` | 2: one line per parent, in order (none for a root commit) |
| `hlc <16 hex>` | 3: the `u64` value, most significant digit first |
| `actor <json>`, `role <json>`, `session <json>` | 4 |
| `git-head <algo> <hex>` or `-`, `git-branch <json>`, `git-worktree <json>`, `git-base <algo> <hex>` or `-` | 5; `git_algo` is derived by [F07 §3.6] |
| `message <json>` | 6: the stored message, the output of `N` or `N_imp` |
| `schema-version <n>` | 7 |
| `origin <hex64>` or `-` | 8 |
| `foreign <algo> <hex>` or `-` | 9 |

Item 10 is `changeset-digest`. The `c` block is these items encoded by [F07 §3.1], in order.

### 4.3 The `git-commit` block

For a foreign or import-checkpoint commit, the git data its items come from ([F07 §12.3]–§12.4):

| Line | Meaning |
|---|---|
| `object-format <algo>` | the destination's object format |
| `oid <hex>` | the git commit's own object id (item 9) |
| `parent <hex64> hlc <16 hex>` | per git parent, in order: the moirai id this store holds for it and that commit's `hlc` |
| `committer-time <T>` | the committer timestamp in seconds ([F06 §4.4.4]) |
| `author-email-hex <hex>` | foreign commits: the bytes between `<` and `>` of the author line |
| `message-hex <hex>` | a foreign commit's whole message; a checkpoint commit's message part before its trailer block (the part [F14] separates) |
| `marker-schema-version <n>` | the `schema-version:` of the tree's `.moirai-image` marker (item 7) |

### 4.4 `values.cases`

`encoding` names the input's form and the bytes expected:

| `encoding` | Input | Bytes |
|---|---|---|
| `cv` | a value (§3.3) | [F07 §7.1] |
| `existence`, `status`, `hierarchy`, `observation`, `body`, `edge` | a side of that key class (§3.5) | the class's value encoding, [F07 §7.2], §8.1 |
| `delta` | a signed decimal | [F07 §7.5] |
| `cstate` | a conflict group without its node (§3.5) | [F07 §7.3] |
| `selector` | `anchor <attributes>` (§3.4) | the selector block, [F07 §8.2] |
| `schema-item` | a schema line (§3.6) | `sf` = 1 and the item value, [F07 §9.2]–§9.6 |

## 5. The history

The commit cases form one history per repository; each case's `p-state` equals its first parent's `q-state`.

| Case | File | Kind | Parents | Entries | `commit_id` (first 16 digits) |
|---|---|---|---|---|---|
| `root` | `commits.cases` | ordinary | — | 23 | `64930f4cffe947e9` |
| `ordinary` | `commits.cases` | ordinary | `root` | 6 | `163e88e25904ad16` |
| `revert` | `commits.cases` | revert | `ordinary` | 6 | `c425a75cab057fa2` |
| `main-c3` | `commits.cases` | ordinary | `revert` | 3 | `e9e5a7d9c6589968` |
| `lane-x-1` | `commits.cases` | ordinary | `ordinary` | 7 | `0c356fdc2d0fce91` |
| `lane-x-2` | `commits.cases` | ordinary | `lane-x-1` | 2 | `1604505e547e1e81` |
| `lane-z-1` | `commits.cases` | ordinary | `ordinary` | 4 | `cc912a175b764273` |
| `cherry-pick` | `commits.cases` | cherry-pick | `main-c3` | 4 | `352fde46a717001f` |
| `merge` | `commits.cases` | merge | `cherry-pick`, `lane-x-2` | 7 | `7a68640429984a83` |
| `lane-y-1` | `commits.cases` | ordinary | `ordinary` | 3 | `97e3cd7aad4af5cb` |
| `sync` | `commits.cases` | sync | `lane-y-1`, `main-c3` | 8 | `9fbedf9a38e44590` |
| `delete` | `tombstones.cases` | ordinary | `ordinary` | 9 | `aaa574fb5321518f` |
| `undelete` | `tombstones.cases` | revert | `delete` | 9 | `7ee5079d1979589c` |
| `anchor-full` | `anchors.cases` | ordinary | `root` | 13 | `c2645145d5e8c99c` |
| `anchor-hash-only` | `anchors.cases` | ordinary | `root` | 13 | `c2645145d5e8c99c` |
| `anchor-texts-added` | `anchors.cases` | ordinary | `anchor-full` (= `anchor-hash-only`) | 1 | `7d9dab189d18bd33` |
| `defaults-explicit` | `normalisation.cases` | ordinary | `root` | 4 | `6087a9137cdacdab` |
| `defaults-omitted` | `normalisation.cases` | ordinary | `root` | 4 | `6087a9137cdacdab` |
| `foreign-root` | `foreign.cases` | ordinary | — | 6 | `3bc4c8c528527f1a` |
| `foreign-child` | `foreign.cases` | ordinary | `foreign-root` | 1 | `2f47a25478af482b` |
| `foreign-clock-behind` | `foreign.cases` | ordinary | `foreign-root` | 1 | `b78e2e4b45ee8dd9` |
| `foreign-merge` | `foreign.cases` | merge | `foreign-child`, `foreign-clock-behind` | 1 | `1fe97b54e2d3fe24` |
| `checkpoint-first` | `checkpoint.cases` | checkpoint | — | 24 | `acc5b76f635def90` |
| `checkpoint-first-sym` | `checkpoint.cases` | checkpoint | — | 24 | `acc5b76f635def90` |
| `checkpoint-next` | `checkpoint.cases` | checkpoint | `checkpoint-first` (= `checkpoint-first-sym`) | 10 | `263c02ad1f7a2466` |

Three pairs share an id by design: `anchor-full`/`anchor-hash-only`, `defaults-explicit`/`defaults-omitted` and
`checkpoint-first`/`checkpoint-first-sym`. The local history is main = `root` → `ordinary` → `revert` → `main-c3` →
`cherry-pick` → `merge`, with lanes x (`lane-x-1`, `lane-x-2`), y (`lane-y-1`, `sync`) and z (`lane-z-1`) forked at
`ordinary`, and side branches for `delete`/`undelete`, the anchor cases and the defaults pair. The foreign and checkpoint
cases are separate repositories.

### 5.1 How a harness uses the cases

1. Hash `digest-input` and `c` with BLAKE3-256 and compare with `changeset-digest` and `commit-id` (any BLAKE3
   implementation will do; this checks the fixture, not the encoder).
2. **Encoder.** Build CS(P) and CS(Q) from `p-state` and `q-state` (§3.7), compute the entries of [F07 §10.1] and encode
   them: the bytes must equal `digest-input`. Encode C from the `commit` block and `changeset-digest`: the bytes must equal
   `c`.
3. **Merge-family cases** (WP-91's merge, E5): the typed three-way merge of `base-state`, `p-state` (o) and `theirs-state`
   must give CS(`q-state`); `merge-rows` names the rows expected to decide each key.
4. **Derived items.** For `git-commit` cases, derive items 1, 3, 4, 6, 7 and 9 from the block ([F07 §12.3]–§12.4) and
   compare with `commit`. For `message-input-hex`, `N` of it must equal `message`.
5. **Pairs.** The cases of each same-id pair must give identical `digest-input` and `c`.
6. `values.cases` and the message cases of `normalisation.cases` are unit tests of single functions.

## 6. Coverage

### 6.1 The gate-0 fixture rows ([F07 §14.4])

| Row | Cases |
|---|---|
| `ordinary` | `commits.cases` `ordinary` |
| `merge` | `commits.cases` `merge` (FieldEdit on a field; theirs' clean key T2.status; ours' clean keys N1.symptom, D1.tradeoff, D2) |
| `sync` | `commits.cases` `sync` (8 entries, a residue of 1 key) |
| `revert` | `commits.cases` `revert` (the inverse of `ordinary`); `tombstones.cases` `undelete` (the inverse of a delete) |
| `cherry-pick` | `commits.cases` `cherry-pick` |
| foreign | `foreign.cases` (`foreign-merge`: item 10 from the typed merge) |
| import-checkpoint | `checkpoint.cases` `checkpoint-first` (no parent), `checkpoint-next` (a previous checkpoint as parent) |
| anchors | `anchors.cases` `anchor-full`, `anchor-hash-only` (a `range` anchor and a `quote` anchor with an empty prefix); `anchor-texts-added` |
| tombstones | `tombstones.cases` `delete` (flagged `blocks`, historical `cites`), `undelete`; `commits.cases` `merge` node X1 (a tombstone landed from the absent state) |
| normalisation | `normalisation.cases` (the §5.4 messages are `n-crlf-trailing` … `n-trailer-not-first-line`; `defaults-explicit` = `defaults-omitted`) |
| order | `checkpoint.cases` `checkpoint-first` (three nodes; all eight classes on the file node; two `at` keys ordered by disc; schema entries after node entries, field before query) and `checkpoint-first-sym` (the symmetric edge written from the other end) |

### 6.2 Specification sections

| Section | Cases |
|---|---|
| [F07 §2.1]–§2.5 (framing, names, store-local numbers, set order, domains) | every `digest-input` and `c`; `values.cases` `cv-set-*`, `cv-ref`; `commits.cases` `merge` (prov and class by name) |
| [F07 §3] (items 1–10) | every `c` block; item 5 in `ordinary` and `anchor-full`; item 8 in `revert`, `cherry-pick` and `undelete`; item 9 in `foreign.cases` and `checkpoint.cases` |
| [F07 §4] (kinds) | `ordinary`, `merge`, `sync`, `revert`, `cherry-pick`, `checkpoint` across the files |
| [F07 §5] (messages) | `normalisation.cases`; `foreign-root`, `foreign-child` (N_imp); `defaults-explicit` (N) |
| [F07 §6.1]–§6.2 (keys and classes) | `checkpoint-first` |
| [F07 §6.3] (defaults, empty values) | `root`, `defaults-explicit`, `defaults-omitted` |
| [F07 §6.4] (tombstones) | `delete`, `undelete`, `lane-x-1`, `lane-x-2`, `merge` |
| [F07 §6.5] (conflict values) | `merge` (FieldEdit, DeleteVsModify), `sync`, `checkpoint-first` (observation), `checkpoint-next` (its resolution); `values.cases` `cstate-*` |
| [F07 §6.6] (edges, symmetric kinds) | `ordinary`, `delete`, `checkpoint-first`, `checkpoint-first-sym` |
| [F07 §7.1] (`cv`) | `values.cases` `cv-*` |
| [F07 §7.2]–§7.5 (class values, `cstate`, images, deltas) | `values.cases`; `merge` (images with `snap` = 1) |
| [F07 §8.1] (edge values) | `values.cases` `edge-*`; `ordinary` (pin), `delete` (flagged) |
| [F07 §8.2]–§8.3 (selector block; full and hash-only) | `anchors.cases`; `checkpoint-first` (occurrence, pred, marker); `values.cases` `selector-*` |
| [F07 §9] (schema items) | `checkpoint-first` (a field and a named query); `values.cases` `schema-*` |
| [F07 §10] (entries, order, digest, sync) | every commit case; `sync` for §10.5 |
| [F07 §12.1] (local commits) | `commits.cases`, `tombstones.cases`, `anchors.cases` |
| [F07 §12.3] (foreign) | `foreign.cases` |
| [F07 §12.4] (import-checkpoint) | `checkpoint.cases` |
| [F07 §13] (ops to entries: Create, Delete, CreateDeleted, Undelete, Conflict, Resolve) | `root`, `delete`, `merge` (X1), `undelete`, `merge`, `checkpoint-next` |
| [F06 §4.4.4] (foreign `hlc`) | `foreign-root`, `foreign-clock-behind` (parent term), `checkpoint-next` |
| [F08 §11.2]–§11.4 (derived uids in use) | `anchors.cases`, `checkpoint.cases` (the derivations themselves are `fixtures/r4/`) |

### 6.3 Rows of `docs/spec/COVERAGE.md`

The fixture column of these rows can cite (R-SPEC fills the column):

| Row | Fixture |
|---|---|
| 60-AR-Canonical | `fixtures/canonical/cases/commits.cases`, `fixtures/canonical/cases/values.cases` |
| 60-I2-Gate0 | `fixtures/canonical/cases/*.cases` (§6.1); the carriers are `fixtures/carrier/` |
| 60-AU-Canon-digest | `fixtures/canonical/cases/commits.cases` (every `digest-input`) |
| 60-AU-Canon-lineorder | `fixtures/canonical/cases/checkpoint.cases` (`checkpoint-first`) |
| 60-AU-Canon-anchordigest | `fixtures/canonical/cases/anchors.cases` |
| R-1 | `fixtures/canonical/cases/values.cases` (`cv-path`, `cv-oid-*`, `cv-pathmove-*`, `cv-set-pathmove`) |
| R-2 | `fixtures/canonical/cases/checkpoint.cases` (the observation composite) |
| R-4, R-10 | `fixtures/canonical/cases/anchors.cases`, `fixtures/canonical/cases/values.cases` (`selector-*`) |
| R-5 | `fixtures/canonical/cases/checkpoint.cases` (the root node and `path_moves`) |
| F3 | `fixtures/canonical/cases/checkpoint.cases` (`checkpoint-first`: the named query item), `fixtures/canonical/cases/values.cases` (`schema-query`) |

## 7. Findings and gaps

Found while authoring; to be filed with the review. The cases follow the reading stated here until it is resolved.

| # | Where | Finding or gap | Cases |
|---|---|---|---|
| G-1 | [F07 §7.1] tag 4 vs §9.3 `default` | Tag 4 (`counter`) occurs "only inside a node image", but a field item's `default` is a `cv` of the field's declared type, so a project `counter` field with a default has no stated tag (4, or 3 as an `int`) | the project counter field `artifact.hits` has no default |
| G-2 | [F07 §10.5] | "What `main`'s window alone yields at the lane" is not defined in state terms. The informative `residue` block reads it as the lane state with each key that `main` changed in its window set to `main`'s value (a counter: the lane's total plus `main`'s delta) | `commits.cases` `sync` |
| G-3 | [F07 §14.4] row normalisation | "A default written explicitly in the image" is an image-level case; this directory states it at the state level (explicit default values in a state block). Closed by WP-21's second part: `fixtures/carrier/defaults-explicit` is the image-level twin (the same id as `defaults-omitted`) | `normalisation.cases` `defaults-*` |
| G-4 | [F08 §10.3.1] interim rule | Until [F20] Appendix A is complete no writer records a scope and no `symbol` or `heading` anchor is captured. Commit cases use `quote` and `range` anchors without a scope; the one scope value (`values.cases` `selector-quote-full-options`) encodes a scope as an imported anchor keeps it | — |
| G-5 | [F07 §8.2] `window`, `span_hash` | The anchors' `window` and `span_hash` are stated values; this part of WP-21 does not derive them from file content ([F20 §2.7]–§2.8) | `anchors.cases`, `checkpoint.cases` |
| G-6 | [F14 §7.2.2], [LQ/lexical §10.2], [LQ/canonical-ast §8.1] | Correction (WP-21, second part). The named query `stale_blockers` was stored as a bare query (`MATCH … RETURN t, u`); the stored text of a named query is the whole `define_stmt` in portable form, which the image checks against `name:`, `params:`, `shape:` and `budget:`. The text is now `DEFINE QUERY stale_blockers() SHAPE table BUDGET light AS { … }`: the `changeset_digest` and `commit_id` of `checkpoint-first` and `checkpoint-first-sym` changed (`acc5b76f…`), `checkpoint-next` changed its id through its parent (`263c02ad…`; its item 10 did not change), and `values.cases` `schema-query` changed its bytes. Every other case is byte-identical | `checkpoint.cases`, `values.cases` `schema-query` |
