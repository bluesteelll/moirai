# 08 — Data model

| | |
|---|---|
| Title | Data model: the two node identities, the 60-byte node header, the cold columns, the value types and their stored encodings, the field block, titles and bodies, schema as data (kinds, fields, enumerations, edge kinds, named queries; [50] F1–F3), the core schema of version 1 (13 kinds, 25 edge kinds), edge keys and property blocks with the anchor record, and the derived identities of R4 (file, root and anchor uids) |
| Chapter | [F08], `docs/spec/format/08-data-model.md` |
| Status | draft, pass 1 pending |
| Work package | WP-14a (R-SPEC-F): the data-model part of WP-14 ([PLAN §3.2] item 1); chapter 18 is the other part |
| Sources | [AR §2.4] T4 (the two ids); [AR §2.5] T5 (delete policies, tombstones); [AR §2.6] T6 (title ≤ 200 B, abstract, bodies ≤ 64 KiB); [AR §2.12] T12 (schema as data, weakening and strengthening, R4 and R5 additions); [AR §3.1] (node header, cold columns, field block, closed type set, virtual `done`); [AR §3.2] (13 kinds, fields, status sets); [AR §3.3] (25 edge kinds, classes, policies, acyclicity, cardinality, discriminator, edge properties, sigil rule); [AR §3.4] I1–I7, I11, I-F1…I-F14 row; [AR §3.5] (derived columns and flags); [AR §3.6] (status machines); [AR §4.3] (`Create` and the ops that carry values; commit size bound); [AR §4.4] (`NODE`, `UID`, `TOPO`, `DEFER`, `DUE`, `TITLE_*`, `FIELDS_*`, `BODY_REF`, `EDGE_PROPS`, `CREATOR`); [AR §4.6] items 7 and 10 and "Not hashed"; [AR §5a.7] step 4 (merge classes); [AR §5b.2] rules 2–4, 8, 9 (field names, value forms, source-truth flags, tombstones); [AR §5e.2]; [AR §7.1] (`--about`, `--applies-to`, `retract --reason`); [AR §7.3] (deviation findings, `artifact{…}` kinds, doc-writer `derived_from`); [AR §7.4] step 5 (`consumed` with `pinned_commit`); [40 §2.1]–[40 §2.9]; [40 §2.10] I-F1, I-F2, I-F3, I-F8, I-F9, I-F14; [40 §2.11] R-1–R-5, R-17 (authoritative); [40 §5.5] (re-key row); [40 §5.7] (field names, `origin_pred` presence); [50 §2.5] (schema vocabulary, edge table, readings, properties); [50 §3.2] (coercion); [50 §3.3] (absent values, defaults); [50 §3.5] (enum ranks); [50 §4.4] (named queries); [50 §8.1] F1–F5, F13; [60 §2.5] rows "Ops and values", "Schema as data", audit rows "Schema" and "Commit body", R-1–R-5, F1–F5; [80 §2.10] P1, P3, P4, P7, P12 through [OS/path §2–§3]; reviews `docs/spec/reviews/a1-S.md` S-01, S-03, S-19, S-20; open points of [RULES/merge-table], [RULES/link-merge-rules], [RULES/status-machines], [RULES/delete-policy-matrix], [RULES/pack-classes] and [RULES/role-write-policy] addressed to WP-14 |
| Depends on | [F01]; cites [F02], [F04], [F06], [F07], [F09], [F10], [F11], [F12], [F13], [F14], [F17], [F18], [F19], [F20], [OS/path], [LQ/lexical], [LQ/grammar-v1.ebnf], [LQ/canonical-ast], [LQ/envelope], [LQ/std], [API], [CFG], [RULES/merge-table], [RULES/link-merge-rules], [RULES/status-machines], [RULES/delete-policy-matrix], [RULES/pack-classes], [RULES/role-write-policy] |

## 1. Scope

This chapter fixes what a node, a value, an edge and a schema item are, and their stored bytes. It owns:

- the two identities of a node, `#N` and `uid` (§2);
- `NodeHdr`, the 60-byte fixed row of every node (§3), and the cold columns (§4);
- the value types of the closed type set and their stored encodings, R-1's `path`, `oid` and `pathmove` included (§5);
- the field block (§6), and the rules for titles and bodies (§7);
- schema as data: the item records of kinds, fields, enumeration values, edge kinds and named queries, with [50]'s F1,
  F2 and F3 columns, and the store-local ids they use (§8);
- the core schema of schema version 1: the 13 node kinds with their fields and statuses, and the 25 edge kinds with
  their classes, delete policies, acyclicity, cardinality and F1 values (§9);
- edge keys, edge property blocks and the anchor record (§10);
- the derived identities of R4: file uids with their predecessor and dead-uid rules, root nodes (R-5) and anchor uids
  (§11), and the data-model constants (§12).

Other chapters own the containers these structures live in: the segment sections that store rows, columns, field
blocks, titles, schema items and anchors ([F09]); the ops that carry values and schema items ([F06]); the canonical
encoding hashed into commit ids ([F07]); the image text ([F14]); the runtime tables `ALLOC`, `UIDX`, `LEASES` and R4's
evidence tables ([F11]); `HEAD.next_id` and `HEAD.next_anchor` ([F04]). The invariants are stated with their
enforcement points in [F13]; R4's invariant texts, strings and `relink` vocabulary in [F18]; the merge rules in
[RULES/merge-table] and [RULES/link-merge-rules]; the status machines in [RULES/status-machines].

**Terms.** A *view* is the versioned state of one ref at one commit ([AR §5a]). A node is *live* on a view when it
exists there and is not deleted; a *tombstone* is a deleted node's remaining row ([AR §2.5]). *Store-local* means
meaningful only inside one store: never hashed, never exported ([AR §4.6] "Not hashed"). A *symbol* and its classes
are [F01 §8]'s.

## 2. Identities

### 2.1 `#N`

- `#N` is a `u32` from 1 to 2^32 − 1; 0 means "no node". It is allocated store-wide under the writer byte from
  `HEAD.next_id` ([F04]), in increasing order, and never reused, not even after the node is deleted, on any branch,
  across imports and GC ([AR §2.4], I1). A write that would need `#N` = 2^32 is refused ([F19]).
- `#N` is the display identity (`#812`) and the key of every runtime table ([AR §5d.1]). It is store-local: it never
  appears in hashed content or the image (N4, [AR §4.6]).
- Row r of every dense per-node column (§3, §4) belongs to `#N` = r + 1 ([AR §3.1]).
- A `#N` is bound to exactly one uid for the store's life (I1, I35′). The binding is kept in the store-wide `ALLOC` and
  `UIDX` indexes ([F11]). A `Create` of a uid that `UIDX` already knows, on any branch, reuses that uid's `#N` ([40 §2.3],
  I-F2); a uid that a value or an edge references but that no view holds (a tombstone reference from an import,
  [40 §5.7]) is also given a `#N` through `UIDX`, with no node behind it on any view ([F11] states the row).

### 2.2 `uid`

- A uid is a `b16` ([F01 §5.6]), compared bytewise and written as 32 lower-case hexadecimal digits ([F01 §6.4]); in
  portable LQ text it is `#u:<32 hex>` ([50 §4.4]). It is the identity in canonical hashes and in the image, and it
  never changes ([AR §3.1]).
- Every kind's schema row gives its `uid_derivation` (R-3, §8.4.7):
  - `random`: 16 bytes from the operating system's cryptographically secure random source (`Entropy::fill_random`,
    [OS/README §4.6]; open point 41); a value
    that is all zero, or that `UIDX` already holds, is drawn again;
  - `file-key`: the file-node derivation of §11.2;
  - `root-key`: the root-node derivation of §11.3;
  - `anchor-key`: the anchor derivation of §11.4 (the discriminator of an `at` edge, not a node).
- The all-zero uid is never a node's uid. A derivation whose result is all zero is refused as a collision ([F19]).

## 3. The node header: `NodeHdr`

### 3.1 Layout

Every row of a view is one `NodeHdr` of exactly 60 bytes ([AR §3.1]). It is byte-packed ([F01 §4.3]), little-endian,
and in the field order of [AR §3.1]; no field is aligned (open point 2). [F09] places the rows (the `NODE` section:
`[NodeHdr; n]`, [AR §4.4]; open point 1).

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 1 | `u8` | `kind` | the node's kind id (§3.3): 0 = no node on this view, 1–13 core kinds, 64–254 project kinds |
| 1 | 1 | `u8` | `status` | the kind's status value (§9.5); `blocked` and a container's `done` are never stored (I8) |
| 2 | 1 | `u8` | `resolution` | §9.4; 0 = none. Non-zero only on a task whose status is `done` or `cancelled` and on a finding whose status is not `open` ([RULES/status-machines] GR-014) |
| 3 | 1 | `u8` | `priority` | 0–4 = P0–P4 (§9.4); default 2 |
| 4 | 1 | `u8` | `criticality` | §9.4; default 2 (`normal`) |
| 5 | 1 | `u8` | `confidence` | §9.4; default 0 (`unset`) |
| 6 | 1 | `u8` | `authority` | §9.4; default 4 (`agent`) |
| 7 | 2 | `u16` | `flags` | bit table §3.2 |
| 9 | 8 | `u64` | `rev_seq` | store `seq` of the commit that last touched this node on this view; after a merge, the merge commit's `seq`; the `--if-rev` CAS target |
| 17 | 4 | `u32` | `parent` | `#N` of the parent, or 0 |
| 21 | 4 | `u32` | `created_tx` | store `seq` of the creating commit on this view |
| 25 | 4 | `u32` | `updated_tx` | store `seq` of the last commit that changed a primary value of the node on this view |
| 29 | 8 | `u64` | `last_op_lsn` | lsn of the newest op on this node in this view's history, the head of its op chain ([F06]); 0 = none |
| 37 | 2 | `u16` | `open_blockers` | derived (§3.4) |
| 39 | 2 | `u16` | `open_blockers_exo` | derived (§3.4) |
| 41 | 2 | `u16` | `children_total` | derived: live direct children on this view (§3.4) |
| 43 | 2 | `u16` | `children_done` | derived: live direct children whose virtual `done` is true (§3.4) |
| 45 | 4 | `u32` | `title_off` | byte offset of the node's title in its segment's `TITLE_BLOB` ([F09]); `NONE32` (`0xFFFFFFFF`) when no title is stored (§7.1) |
| 49 | 4 | `u32` | `fields_off` | byte offset of the node's field block (§6) in its segment's `FIELDS_BLOB`; `NONE32` when the node has no stored field |
| 53 | 4 | `u32` | `body_ref` | 1-based index of the node's entry in its segment's `BLOBTAB` ([F09], [F10]); 0 = no body |
| 57 | 3 | `[3]u8` | `_reserved` | zero ([F01 §10]) |
| total | 60 | | | |

`seq` values are `u64` in the log and the commit header; `created_tx` and `updated_tx` hold them as `u32`, because a
store holds at most 2^32 − 1 commits and a commit beyond that is refused ([AR §3.1], [F17]).

### 3.2 `flags`

| bit | name | kind of fact | meaning |
|---|---|---|---|
| 0 | `deleted` | existence | the row is a tombstone (§3.5) |
| 1 | `suspect` | derived | [AR §3.5] `suspect`; its definition is [F13]'s (open point 30) |
| 2 | `has_dangling` | derived | the node is the destination of a `flagged` `blocks` or `gates` edge (§10.2) |
| 3 | `pinned` | source truth | set and cleared explicitly; versioned and exported ([AR §5b.2] rule 2) |
| 4 | `container` | derived | `children_total > 0` |
| 5 | `conflicted` | derived | the node holds at least one unresolved conflict value on this view ([AR §3.5]) |
| 6 | `archived` | source truth | as `pinned` |
| 7 | `frozen` | source truth | as `pinned` |

Bits 8–15 are reserved-zero. There is no `claimed`, `proposed`, `stale`, `ready` or `blocked` bit: those are derived
at read time or runtime ([AR §3.1], I8, I36′). The three source-truth flags are independent facts: they neither mirror
nor imply the statuses `archived` and `frozen` that some kinds have (open point 21). What reads do with them is the
query library's and the pack classes' ([LQ/std], [RULES/pack-classes]).

### 3.3 Column values

- **`kind`.** 0 marks a row with no node on this view: a `#N` allocated on another branch, or never created here.
  1–13 are the core kinds of §9.1. 14–63 are reserved for core kinds of later schema versions and invalid in version 1.
  64–254 are project kinds, whose ids are store-local (§8.3). 255 is invalid.
- **Header enumerations** (`status`, `resolution`, `priority`, `criticality`, `confidence`, `authority`) hold the
  integer of an enumeration value of §9.4–§9.5, or a store-local integer of a project value (§8.3). A header value always
  exists: a node never has an absent header column ([50 §3.3]); where a column does not apply, it holds the value named
  as its default.
- **Offsets.** `title_off` and `fields_off` are offsets into blobs of the segment that holds the row ([F09]); `NONE32`
  is never a valid offset. A delta segment's row points into that delta segment's blobs.
- **Row validity** (tested by the format oracle and `doctor --fsck`, not on every read: [F01 §10], [F01] open point 6):
  `kind` is valid; `status` is a value of the kind; `resolution` obeys the column rule; `priority` ≤ 4; the other
  enumerations hold known values; the reserved bits and bytes are zero; `parent` is 0 or an allocated `#N` other than
  the row's own; a row with `kind` = 0 has all 60 bytes zero.

### 3.4 Derived columns

`open_blockers`, `open_blockers_exo`, `children_total`, `children_done`, and the flags `suspect`, `has_dangling`,
`container` and `conflicted` are derived state: their definitions and maintenance are [AR §3.5]'s and [F13]'s (I9, F15).
`open_blockers` and `open_blockers_exo` count `blocks` in-edges only, flagged ones included; `gates` in-edges never count
in them and constrain only `complete` ([F13 §6.2]; [RULES/state-definition] BT rows; review pass 1 S1-30). This chapter
fixes only their storage:

- The four counters are `u16`. They **saturate** at 65,535: a writer that would raise a counter above 65,535 stores
  65,535, and a stored 65,535 means "65,535 or more". A reader that needs the exact value of a saturated counter, and a
  writer that would decrement one, recounts it from the adjacency ([F09]). Zero tests (`open_blockers = 0`, the
  container test) never need a recount. So every stored counter equals `min(true count, 65,535)`, which is what I9's
  recomputation compares (open point 6).
- `topo` (§4) is derived by Pearce–Kelly. It is valid when every precedence edge u → v of I5′ has `topo(u) < topo(v)`
  and no two live nodes share a value; I9 checks this validity, not equality with one recomputed order (open point 29).

### 3.5 Tombstone rows

A `Delete` turns the row into a tombstone ([AR §2.5]; the op and its before-image are [F06]'s):

- `kind`, `status`, `resolution`, `priority`, `criticality`, `confidence`, `authority`, `created_tx` and the title are
  kept, so a tombstone renders with its kind and title at deletion ([50 §3.6]); an artifact, whose title is not stored
  while it lives (§7.1), is given its last `path` text as its stored title;
- `flags` = `deleted` only; `parent` = 0; the four counters = 0; `fields_off` = `NONE32`; `body_ref` = 0;
- `updated_tx` and `rev_seq` name the deleting commit; `last_op_lsn` names the `Delete`;
- the reason and replacement live in the `TOMB` section ([AR §4.4], [F09]); the retained out-edges (flagged structural
  edges and historical edges, I39′) stay in the adjacency;
- the cold columns keep `uid` and `CREATOR`; `topo`, `defer_until` and `due` become 0.

## 4. Cold columns

Cold columns are one value per row, in separate arrays that only the queries needing them touch ([AR §3.1]); [F09]
owns their sections. The logical columns:

| column | type | meaning |
|---|---|---|
| `uid` | `b16` | §2.2; set at `Create`, never changed; all zero on a `kind` = 0 row. [AR §4.4] stores it as the sorted section `UID` of (uid, `#N`) pairs; [F09] fixes how a row finds its uid |
| `topo` | `u32` | Pearce–Kelly position in the precedence graph (§3.4); derived |
| `defer_until` | `u32` | Unix seconds (UTC, [F01 §5.7] without the millisecond scale): the node is not dispatchable before this time ([AR §3.5]); 0 = none. The largest value, 2^32 − 1, is 2106-02-07T06:28:15Z |
| `due` | `u32` | Unix seconds, as `defer_until`; 0 = none |
| `CREATOR` | `Creator` (below) | the actor and role of the creating commit, set at `Create` and never changed ([50] F4) |
| `DOCLEN` | [F09] | [50] F12's per-field token lengths; kept or dropped by the LQ-Bench decision of WP-72 ([F09]'s hole) |

`Creator` (6 bytes, [F01 §8.3]):

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 4 | `u32` | `actor` | symbol id of class `actor` ([F01 §8.2]): the commit header's `actor` at `Create` |
| 4 | 2 | `u16` | `role` | symbol id of class `role`: the commit header's `role` at `Create`; 0 = no role |
| total | 6 | | | |

LQ reads `CREATOR` as `created_by` and `created_role` ([50 §2.5]); the rule tables use `CREATOR.role` as the owner of a
node ([RULES/role-write-policy] WT-004, which [RULES/status-machines] GR-002 applies to status writes).

## 5. Values

### 5.1 The type byte and the type registry

The closed type set of [AR §3.1] with R-1's three types is {bool, int, counter, f64, enum-with-lattice, text, set, ref,
commit-ref, path, oid, pathmove}. A stored value is preceded, wherever its type is not fixed by position, by a **type
byte**:

| bit | name | meaning |
|---|---|---|
| 0–5 | `type_id` | the type id of the registry below |
| 6 | — | reserved-zero |
| 7 | `vbit` | the value of a `bool`; zero for every other type |

| value | name | logical type | stored encoding (§5.2) |
|---|---|---|---|
| 0 | `absent` | no value | no value bytes; `vbit` 0. Valid only in a value position that admits absence: the old and new value of an op and the sides of a key value or conflict value ([F06 §5.1], [F06 §6.2]). Invalid in a field block, a set element, a default, a promoted column and every other position |
| 1 | `bool` | bool | no value bytes: the value is `vbit` |
| 2 | `int` | int | `svar64` ([F01 §5.3]) |
| 3 | `counter` | counter | `svar64`: the current value, the sum of every `Incr` ([AR §3.1]); merge rule `Incr` |
| 4 | `f64` | f64 | `f64` ([F01 §5.5]), rules of §5.3 |
| 5 | `enum` | enum-with-lattice | `uvar16`: the enumeration value's integer (§8.3, §9.4) |
| 6 | `text` | text | `vstr` ([F01 §6.2]), inline |
| 7 | `sym` | text | `uvar32`: a symbol id of class `text` ([F01 §8.2]), ≠ 0; the interned form of a text value |
| 8 | `set` | set | §5.2 |
| 9 | `ref` | ref | `u32`: a `#N` ≠ 0; it denotes the node's uid (§2.1) |
| 10 | `commitref` | commit-ref | `b32`: the full 32-byte id of a moirai commit ([F07 §3.1]), ≠ all zero. The full id, because the canonical form hashes it and the image writes it ([F07 §7.1], [F14 §5.1]) and a cited commit need not be one this store holds |
| 11 | `path` | path (R-1) | §5.2 |
| 12 | `oid` | oid (R-1) | §5.2 |
| 13 | `pathmove` | pathmove (R-1) | §5.2 |

Values 14–63 are reserved and invalid in format v1. `text` and `sym` are two stored forms of the one logical type
`text`; a field's schema row says which form it uses (§8.5.2), and a value is always stored in its field's form
("strings are interned symbols or length-prefixed bytes", [AR §3.1]). Store-local parts of values — symbol ids, `#N`,
enumeration integers, the `path` root id — are rendered by name, uid or full commit id in canonical forms ([F07]) and in
the image ([F14]).

**One registry, one encoding** (pass 1, P1-1, S1-1, A1-1). This section and §5.2–§5.5 are the only definition of a
stored value's bytes. Every structure that stores a value uses them byte for byte: the field block (§6), the ops,
key values, conflict values and node images of a commit ([F06 §5]–§7, which cite this section and define no tag of their
own), the sections and columns of segments ([F09]) and the rows of runtime tables ([F11]). [F07] maps these bytes to its
canonical encoding, which is its own and never stored. The one derived form is an index key, not a stored value: the
promoted element of a `commitref` field in [F09 §10.1] (`FCOL`, `FIDX`) is the id's first 16 bytes (`id16`), an index
column as review pass 1 (S1-1) allows. A promoted field keeps its storage `field` (§8.4.2), so the full 32-byte value
stays in the row's field block: a reader takes the value from there, and confirms against it every row an `id16` key
selects (pass 1, round 1, P1-1).

### 5.2 Stored encodings

**`path`** (R-1: "root sym u16 + varint-length UTF-8, exact bytes"):

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `root` | `u16` | always | symbol id of class `root` ([F01 §8.2]) naming the root; ≠ 0 |
| 2 | `text` | `vstr` | always | the path text, exact bytes (§5.4.1) |

**`oid`** (R-1: "algo u8 + 20 or 32 B"): the variable-width form of [F01 §7.5] — an `algo` byte of the git
object-format registry, then exactly its digest bytes. As a field value `algo` is `sha1` or `sha256`; `none` (algo 0,
no digest bytes) is valid only where this chapter says "or empty" (the `git` member of `pathmove`). An empty `oid` field
is an absent field (§6.2). Two `oid` values are equal only when their `algo` and digest bytes are equal ([F20 §2.3]).

**`pathmove`** (R-1: `{hlc u64, class u8, from path, to path, git oid-or-empty}`):

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `hlc` | `u64` | always | the writer's HLC when the candidate of the commit that adds the entry was computed ([40 §2.4], review S-20); orders entries only and is never compared with any commit's `hlc` |
| 2 | `class` | `u8` | always | 1 `explicit`, 2 `confirmed`, 3 `committed`, 4 `observed` ([40 §2.4]); other values invalid |
| 3 | `from` | `path` | always | a directory prefix (§5.4.2) |
| 4 | `to` | `path` | always | a directory prefix of the same root as `from`, different from `from` |
| 5 | `git` | `oid` | always | the git commit in which the move was observed or committed; `none` without git |

**`set`**:

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `elem` | `u8` | always | the element type: a type byte (`vbit` 0) of `int`, `enum`, `text`, `sym`, `ref`, `commitref`, `path`, `oid` or `pathmove`; it equals the field's declared element type |
| 2 | `n` | `uvar32` | always | the number of elements, ≥ 1 (an empty set is an absent field, §6.2) |
| 3 | `elements` | `n` × the element encoding | always | strictly ascending in the order of §5.5: no duplicates |

### 5.3 Value rules

- **Text** (`text`, `sym`, the text of `path`, titles and bodies) is valid UTF-8 ([F01 §6.1]). Every CR LF pair and every
  lone CR in a text value or body is replaced by LF at write time — the store rule [AR §5b.2] rule 7 states for bodies,
  extended to every text value so that the image's block form can carry it (open point 14). U+0000 is refused in
  every text value except bodies.
- **One-line text** (every `sym` value, and every field whose schema row sets `one_line`): contains no LF.
- **Lengths.** A `text` value is at most 65,536 bytes; a `sym` value is 1–4,096 bytes; a title is 1–200 bytes; a body is
  at most 65,536 bytes ([AR §2.6]); larger content is an `artifact` ([F17] OP-17-19). The empty text is never stored: an
  empty value is an absent field (§6.2).
- **Empty is absent, everywhere.** The empty text (a `text` of length 0; `sym` id 0 is never a value), the empty set and
  an `oid` of algorithm `none` are never stored as values in any structure: a field block, an op's old or new value, a
  key value, a conflict side or a node image stores `absent` instead (§6.2; [F06 §5.2]). An empty body is no body (§7.2).
- **`int`** values stay within their field's range when its schema row gives one (§8.5.2); `int` arithmetic never wraps
  ([50 §3.3]).
- **`f64`** ([F01] open point 8): NaN and ±infinity are refused at write time (exit 2, [F19] `bad_value`); −0.0 is stored
  as +0.0. A stored NaN pattern, either infinity (`0x7FF0000000000000`, `0xFFF0000000000000`) or −0.0
  (`0x8000000000000000`) makes the structure that holds it invalid: a decoder refuses it wherever it finds it.
- **`ref`**: an allocated `#N` (1 ≤ N < `next_id`); the node need not be live on the view.
- **`enum`**: a non-retired value of the field for the node's kind (§8.5.3).

### 5.4 Sub-formats of text values

#### 5.4.1 Path text

The text of a `path` value is, by root ([40 §2.4], [OS/path §2]):

- root `abs`: an `AbsPath` ([OS/path §2.2], P12): machine-local, never re-bound, never compared across machines or OSes;
- every other root: a non-empty `RelPath` ([OS/path §2.1]; P1, P4): `/` separators, no empty, `.` or `..` segment, no
  leading or trailing `/`, no `\`, no C0 control. Its spelling follows P2, P3 and P7 ([OS/path §3]): git's HEAD spelling
  for tracked files, else the enumerated spelling, with P3's NFC rule; no other normalisation (I-F8).

**Root names** are `project`, `abs`, or a named root: 1–64 bytes of `[a-z0-9_-]`, starting with a letter, and neither
`project` nor `abs` (the names of `roots.<name>`, [CFG]; open point 38). A root name is interned in class `root`.

#### 5.4.2 Directory prefix

A `from` or `to` of a `pathmove` is the text of a non-empty `RelPath` followed by one `/` ([40 §2.4]: "root-relative
directory prefixes ending in `/`"). Its root is never `abs`.

#### 5.4.3 Path globs

The elements of `files_owned` and `path_globs`, and the `path:` elements of `applies_to` (§9.3), are globs rooted at
`project` ([40 §2.4]) with this grammar ([RFC 5234]):

```abnf
glob      = gseg *( "/" gseg )
gseg      = "**" / 1*gpart                 ; "**" only as a whole segment
gpart     = gchar / "*" / "?" / gclass
gclass    = "[" [ "!" ] 1*gitem "]"
gitem     = cchar [ "-" cchar ]            ; a range by code point, first ≤ second
gchar     = <a Unicode scalar value other than C0, "/", "\", "*", "?", "[">
cchar     = <a Unicode scalar value other than C0, "/", "\", "]">
```

Matching a `RelPath` p: the glob's segments match p's segments in order; `**` matches zero or more whole segments; `*`
matches any run of scalar values inside one segment, the empty run included; `?` matches one scalar value; a class
matches one scalar value in (with `!`, not in) its items. The **literal prefix** of a glob is its bytes up to and
including the last `/` before the first `*`, `?` or `[`, or the whole glob when it has none ([40 §2.4];
[RULES/link-merge-rules] CP-009, its open point 6). This is the glob syntax of [50 §2.6] `glob_match`.

#### 5.4.4 Order keys

The `order` field (§9.2) is a base-62 fractional index ([AR §5b.2] rule 4): one or more bytes of `[0-9A-Za-z]`,
compared bytewise. moirai never writes a key ending in `0`, so a key between any two keys always exists; the generation
rule is [API]'s. Keys are never renormalised.

#### 5.4.5 Record lists

Four core fields hold a list of small records ([AR §3.2]: `list<…>`), which the closed type set has no type for. They
are `text` values in this form (open point 10):

```abnf
record-list = record *( LF record )        ; no trailing LF
record      = member *( HTAB member )      ; exactly the shape's member count
member      = *mchar                       ; may be empty
mchar       = <a Unicode scalar value other than C0>   ; so no HTAB, LF or CR
```

| field | members, in order (R = must be non-empty) | member rules |
|---|---|---|
| `doc.targets` | `metric` (R), `value` (R), `unit`, `op` | `value`: an `f64` in the image's decimal form ([AR §5b.2] rule 4); `op`: one of `<`, `<=`, `=`, `>=`, `>` or empty |
| `doc.readiness` | `item` (R), `state`, `reason` | — |
| `decision.alternatives` | `text` (R), `rejected_why`, `measurement_ref`, `git_tag`, `revive_condition` | `measurement_ref`: empty or `#u:` + 32 lower-case hex (a uid, portable, [50 §4.4]) |
| `question.options` | `option` (R) | — |

A record list has at least one record. A writer refuses a value that does not match its shape (exit 2, [F19]); an
image import that does not match is `ImageParse`.

#### 5.4.6 Tagged scope elements of `applies_to`

The elements of `applies_to` (§9.3) are `sym` values of one of the forms `role:<r>`, `phase:<p>`, `lane:<l>` or
`path:<glob>`, where `<r>`, `<p>`, `<l>` are 1–64 bytes of `[a-z0-9_./-]` and `<glob>` follows §5.4.3 ([AR §7.1]
`--applies-to 'role:tester,path:crates/phys/**'`). An empty `applies_to` means `*` ([AR §3.2]).

### 5.5 Order of values

The stored order of set elements (strictly ascending) and of the field block (§6):

| type | order |
|---|---|
| `int`, `counter` | numeric, signed |
| `enum`, `ref`, `sym` | numeric by the stored integer (`sym` by symbol id: store-local) |
| `text`, `commitref` | bytewise ([F01 §6.6]) |
| `path` | (`root` id numeric, text bytewise) |
| `oid` | (`algo`, digest bytewise) |
| `pathmove` | (`hlc`, `from` text, `to` text, `class`, `git`) — [40 §2.4]'s (hlc, from, to) extended to a total order |

The stored order is store-local where it uses ids; the canonical order of a set is [F07]'s.

### 5.6 One value of every type in each form

*(Informative; pass 1, round 1, P1-1.)* One value per type as this section stores it and as [F07 §7.1] hashes it. The
stored form is the same in every structure that stores a value: a field-block entry (§6) writes its `field_sym`
before it, and an op, key value or conflict side ([F06 §5.1]) writes it as it stands; a set element is the value bytes
without the type byte. The store-local numbers assumed: root `project` = root symbol 1; the text `ok` interned as
symbol 7; a field whose enumeration numbers `high` as 2; #40 with uid U (16 bytes); a commit id C (32 bytes); a SHA-1
digest D (20 bytes). These are the values of the cross-chapter byte fixture of `COVERAGE.md` row 60-AR-Values, which
R-FIX (WP-20) builds as files, together with the uid and digest bytes it chooses.

| type | value | type byte | value bytes (§5.2) | canonical `cv` ([F07 §7.1]) |
|---|---|---|---|---|
| 0 `absent` | none (op and conflict positions only) | `00` | none | `00` |
| 1 `bool` | false; true | `01`; `81` | none | `01`; `02` |
| 2 `int` | −3 | `02` | `05` | `03 FD FF FF FF FF FF FF FF` |
| 3 `counter` | 300 (a node image only, [F06 §6.3]) | `03` | `D8 04` | `04 2C 01 00 00 00 00 00 00` |
| 4 `f64` | 1.5 | `04` | `00 00 00 00 00 00 F8 3F` | `05 00 00 00 00 00 00 F8 3F` |
| 5 `enum` | `high` | `05` | `02` | `06 04 00 00 00 68 69 67 68` |
| 6 `text` | `ok` | `06` | `02 6F 6B` | `07 02 00 00 00 6F 6B` |
| 7 `sym` | `ok` | `07` | `07` | `07 02 00 00 00 6F 6B` (one value with `text`) |
| 8 `set` | {1, 5} of `int` | `08` | `02 02 02 0A` | `09 03 02 00 00 00 01 00 00 00 00 00 00 00 05 00 00 00 00 00 00 00` |
| 9 `ref` | #40 | `09` | `28 00 00 00` | `0A` ‖ U |
| 10 `commitref` | C | `0A` | C | `0B` ‖ C |
| 11 `path` | `project`, `docs/a.md` | `0B` | `01 00 09 64 6F 63 73 2F 61 2E 6D 64` | `0C 07 00 00 00 70 72 6F 6A 65 63 74 09 00 00 00 64 6F 63 73 2F 61 2E 6D 64` |
| 12 `oid` | SHA-1 D | `0C` | `01` ‖ D | `0D 04 00 00 00 73 68 61 31 14 00 00 00` ‖ D |
| 13 `pathmove` | `hlc` `0x01A0C4506C000003`, `explicit`, `project:a/` → `project:b/` (directory prefixes, §5.4.2), no git | `0D` | `03 00 00 6C 50 C4 A0 01 01 01 00 02 61 2F 01 00 02 62 2F 00` | `0E 03 00 00 6C 50 C4 A0 01 08 00 00 00 65 78 70 6C 69 63 69 74 07 00 00 00 70 72 6F 6A 65 63 74 02 00 00 00 61 2F 07 00 00 00 70 72 6F 6A 65 63 74 02 00 00 00 62 2F 00 00 00 00 00 00 00 00` |

The `int` −3 is zigzag 5; the counter 300 is zigzag 600, `uvar64` `D8 04` ([F01 §5.3]); the set's `02 02 02 0A` is the
element type byte `02`, `n` = 2 and the zigzag elements 2 and 10; its `cv` has element tag 3, count 2 and the two `i64`
payloads in bytewise order ([F07 §2.4]).

## 6. The field block

### 6.1 Layout

Every kind-specific field, and every common field whose storage is `field` (§9.2), lives in the node's field block
([AR §3.1]): a sequence of `(field_sym varint, type u8, value)` entries in the segment's `FIELDS_BLOB`, found through
`NodeHdr.fields_off`.

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `n` | `uvar32` | always | number of entries, ≥ 1 |
| 2 | `entries` | `n` × `FieldEntry` | always | strictly ascending by `field_sym` |

`FieldEntry`:

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `field_sym` | `uvar32` | always | symbol id of class `name` ([F01 §8.2]) naming the field; ≠ 0 |
| 2 | `type` | `u8` | always | the type byte (§5.1); its `type_id` equals the field's declared type |
| 3 | `value` | by `type` | always | §5.2 |

A decoder can skip an entry of any type without the schema: every encoding is self-delimiting.

### 6.2 Rules

- A field is **absent** when it has no entry. Absence is the one representation of an empty value: an empty text, an
  empty set, an empty `oid`, a counter of 0 and a value equal to the field's default are never stored. So every value
  has exactly one stored form (open point 12). The empty-value part of this rule (not the default part, which is a
  property of a view's field block) holds in every value position of [F06] too (§5.3).
- An entry's field must have a non-retired field item for the node's kind, or for every kind (`*`), whose storage is
  `field` (§8.5.2); header columns, flags, cold columns, the title and the body are never field entries.
- Field values obey §5.3 and their schema row's constraints (§8.5.2). A node with no entry has `fields_off` = `NONE32`.
- The field block of a tombstone is empty (§3.5).

*(Informative)* A task whose field symbols are `labels` = 5, `estimate` = 21 and `pre_registered` = 30, with labels
{sym 7, sym 12}, estimate 3 and `pre_registered` true, has the 12-byte block
`03 05 08 07 02 07 0C 15 02 06 1E 81`: `n` = 3; `labels` (`05`), type `set` (`08`), elements `sym` (`07`), two
(`02`), ids 7 and 12; `estimate` (`15`), type `int` (`02`), zigzag 6 (`06`); `pre_registered` (`1E`), type byte `81`
(`bool` with `vbit` set).

## 7. Titles and bodies

### 7.1 Titles

- A title is one line of 1–200 bytes of text (§5.3) ([AR §2.6]), stored in the segment's `TITLE_BLOB` ([F09]) and found
  through `title_off`. It is required at `Create` for every kind except `artifact`.
- An `artifact`'s title is derived from its `path` and not stored while the node lives ([40 §2.2]); its `title_off` is
  `NONE32` and writes of its title are refused. Its tombstone stores its last path text (§3.5).
- A root node's title is `root:` followed by its root name, set at `Create` and never changed ([40 §2.4]).
- `#N` mentions in a title create `mentions` edges (§10.4).

### 7.2 Bodies

- A body is at most 65,536 bytes of valid UTF-8 after the CR normalisation of §5.3 ([AR §2.6], [AR §5b.2] rule 7). Its
  content address is BLAKE3-128 of the stored bytes ([F01 §7.1]); `SetBody` names it ([F06]); the bytes live in the log
  tail and in `blobs` files ([F10]), found through `body_ref` and `BLOBTAB` ([F09]).
- Every kind may carry a body. Bodies merge as text, except the bodies of `doc` nodes whose `doc_kind` is `section`,
  which also run the removed-text guard ([AR §5a.7]; [RULES/merge-table] MC-009).
- **An empty body is no body** (pass 1, S1-34; [F07] open point 26). A body of zero bytes is never stored: a write that
  sets the body to the empty text stores `new` absent ([F06 §7.4] `SetBody`, `bflags` bit 1 clear), `body_ref` becomes 0,
  and the body key is absent in the canonical state ([F07 §6.3]). A diff3 result of zero bytes is absent in the same way
  ([F12 §7.5]).

## 8. Schema as data

### 8.1 What the schema of a view is

The **effective schema** of a view is the core schema of its schema version (§9) together with the view's **schema
items**, which are versioned per branch, merged, hashed in canonical item 10 and exported as `schema/*.moi`
([AR §2.12], [AR §4.6], [F14]). Items describe project kinds, project fields (also on core kinds), project enumeration
values (also of core enumerations), project edge kinds, and project named queries ([50] F3).

- **Schema version.** Format v1 defines schema version **1**: the core schema of §9. It is the value of canonical item 7
  ([AR §4.6]) for every commit of a format-v1 store and the `schema-version` line of `.moirai-image` ([AR §5b.3]). A
  commit or image of another schema version is refused as a newer format is ([F01 §9.1]; open point 8).
- **Core items are fixed.** No schema item may change, retire or shadow a core kind, field, enumeration value or edge
  kind; items may add fields to core kinds and values to core enumerations.
- **Weakening and strengthening** ([AR §2.12], [RULES/merge-table] CS-017). Adding an item whose key the view does not
  hold is a weakening change (`Schema{weaken}`, or `Schema{query}` for a named query, [F06]); it applies at once and
  merges freely. Every other change — retiring an item, changing a field's type, class, `optional`, default, range or
  constraints, changing an enumeration value's rank, side flag or covers, changing an edge kind's policies, endpoints or
  F1 columns — is a strengthening change (`Schema{strengthen}`), which needs `moirai migrate` on the branch and is
  re-validated at merge (`SchemaConflict`). Defining, changing and dropping a named query is `Schema{query}` ([50 §4.4]).
- **Retired items.** A kind, field, enumeration value or edge kind is removed by retiring it (`iflags.retired`, §8.5):
  its store-local id is never given to another item (I11), and no live node or edge may use it after the migration.
  A dropped named query is removed (its item is absent).
- **Where items live.** [F06] carries items in `Schema` ops; [F09] stores the view's items in a segment section (open
  point 44).

### 8.2 Names

| name of | grammar | uniqueness | symbol class |
|---|---|---|---|
| kind | `[a-z][a-z0-9_]*`, 1–64 bytes | among kinds, ASCII case-insensitive ([LQ/lexical]: labels are case-insensitive); not `deleted` | `name` |
| field | `[a-z][a-z0-9_]*`, 1–64 bytes | per kind, and against the common fields (§9.2) and LQ's built-in node properties ([50 §2.5]: `id`, `uid`, `kind`, `rev`, `created`, `updated`, `created_at`, `updated_at`, `created_by`, `created_role`, `updated_by`, `done`, `unfinished`, `container`, `unblocked`, `blocked`, `open_blockers`, `is_blocker`, `children_total`, `children_done`, `ready_to_close`, `suspect`, `conflicted`, `answered`, `has_dangling`, `depth`, `topo`, `ready`, `claimed`, `lease`, `settled_elsewhere`, `deleted_elsewhere`, `state`) | `name` |
| enumeration value | `[A-Za-z0-9_][A-Za-z0-9_-]*`, 1–64 bytes | per (kind, field) | `name` |
| edge kind (stored) | `[a-z][a-z0-9_]*`, 1–64 bytes | among edge kinds | `name` |
| `lq_name`, `reverse_names` | `[A-Z][A-Z0-9_]*`, 1–64 bytes | across every `lq_name`, reverse name and upper-cased stored edge name, ASCII case-insensitive | `name` |
| named query | [LQ/grammar-v1.ebnf] `qname` | per view; never shadowing `std`, `tx` or a keyword ([50 §4.4]) | `name` |

A project item may not use a core name. Names are never hashed as symbol ids: canonical forms carry the strings.

### 8.3 Store-local schema ids

Three kinds of schema item have a small integer that stored rows use: a kind's id (`NodeHdr.kind`), an edge kind's id
(the kind byte of the adjacency, [F09]), and an enumeration value's integer (header columns and `enum` values). For core
items the integers are the fixed values of §9. For project items they are **store-local**, like symbol ids ([F01 §8];
open point 7):

- **Id spaces and ranges.** Kind ids 64–254. Edge kind ids 64–254. Enumeration values per (kind, field): 64–254 for a
  header column that has core values, 0–254 for the `status` of a project kind, 64–65,535 for a field-block enumeration
  that has core values, 0–65,535 for a project enumeration field.
- **Allocation.** The writer allocates, under the writer byte, the smallest id of the range that the store has never
  given to any item of that space (for enumeration values: of that (kind, field)); an item key that the store already
  knows, on any branch, keeps its id (as `UIDX` does for uids, I1). Ids are never reused, retired items included.
  Exhaustion refuses the write ([F19]).
- **Record.** The item record holds its id in its *store-local* field (§8.5.1 `kind_id`, §8.5.3 `value`, §8.5.4
  `edge_id`), so the `Schema` op that first lands an item carries the id inside its `new` bytes ([F06 §7.6]), unhashed
  ([F07 §9]); no separate op field exists (pass 1, A1-20). Recovery rebuilds the store-wide map
  (space, key) → id from the log, as it rebuilds `UIDX`; checkpoints fold the map into a section ([F09], [F11]; the
  proposed name is `SCHEMAIDS`); `repair --rebuild-from-log` rebuilds it.
- **Never hashed, never exported.** Canonical forms and the image name kinds, edge kinds and values by name ([F07],
  [F14]). An importing store allocates its own ids.

### 8.4 Enumerations of the schema rows

#### 8.4.1 Merge class of a field ([AR §5a.7] step 4; R-2; [RULES/merge-table] §3)

| value | name | meaning |
|---|---|---|
| 0 | `none` | not a key of the canonical changeset (bookkeeping, virtual) |
| 1 | `scalar` | whole-value three-way merge |
| 2 | `owner` | owner-authority field |
| 3 | `authority` | the `authority` column |
| 4 | `status` | the status with its resolution, merged by the kind's lattice |
| 5 | `counter` | `Incr` deltas summed |
| 6 | `set` | add-wins relative to the base |
| 7 | `text` | line diff3 |
| 8 | `section-text` | line diff3 plus the removed-text guard (the body of a doc section) |
| 9 | `hierarchy` | (`parent`, `order`) moved by Kleppmann's rule |
| 10 | `identity` | immutable identity input (R-2) |
| 11 | `observation` | a member of the artifact's observation composite, one merge key (R-2) |
| 12 | `alias-set` | `aliases` |
| 13 | `glob-set` | glob sets, composed through directory moves |
| 14 | `pathmove-set` | `path_moves` |
| 15 | `derived` | derived state, recomputed, never merged |

#### 8.4.2 Storage of a field

| value | name | where the value lives |
|---|---|---|
| 1 | `header` | a `NodeHdr` column |
| 2 | `flag` | a source-truth bit of `NodeHdr.flags` |
| 3 | `cold` | a cold column (§4) |
| 4 | `field` | the field block (§6) |
| 5 | `title` | `TITLE_BLOB` (§7.1) |
| 6 | `body` | the body (§7.2) |

Project fields always have storage `field`.

#### 8.4.3 `index` ([50] F2, F5)

| value | name | meaning |
|---|---|---|
| 0 | `none` | no promoted structure |
| 1 | `column` | an `FCOL.<field>` section: a dense typed column with an absent bitmap ([F09]) |
| 2 | `bitmap` | an `FIDX.<field>` section: value → frozen bitset; for a field whose type is not `set`, also an `FCOL.<field>` section |

#### 8.4.4 `coerce` ([50] F2, [50 §3.2])

| value | name | meaning |
|---|---|---|
| 0 | `none` | no coercion |
| 1 | `priority` | `'P1'`, bare `P1` and `1` denote the same value |
| 2 | `revision-integer` | a bare integer is a store sequence number; a revision-shaped word or string is a revspec |
| 3 | `timestamp` | the field holds Unix seconds; an ISO 8601 string compares as a timestamp (open point 11) |

#### 8.4.5 Existence policy of a kind ([AR §5a.7] step 4; [RULES/merge-table] EP rows)

| value | name | provisional state of a `DeleteVsModify` |
|---|---|---|
| 1 | `delete-wins` | the deleting side's |
| 2 | `resurrect` | the modifying side's |
| 3 | `none` | the destination's; no automatic policy |

#### 8.4.6 Edge enumerations ([AR §3.3])

Edge class: 1 `structural`, 2 `historical`.

On destination deleted (`on_dst`):

| value | name | effect ([AR §3.3], [RULES/delete-policy-matrix]) |
|---|---|---|
| 1 | `restrict` | the delete is refused while the edge exists |
| 2 | `restrict-cascade-reparent` | refused, unless `--cascade` (the subtree is deleted) or `--reparent` (children move up) |
| 3 | `restrict-reassign` | refused, unless `--reassign` to the parent area |
| 4 | `restrict-repoint` | refused, unless re-pointed to the canonical node |
| 5 | `drop` | the edge is removed; for `blocks` and `gates`, re-pointed to the replacement under `--replaced-by` instead ([RULES/delete-policy-matrix] EG-006, EG-012) |
| 6 | `drop-notify` | removed, with a notice |
| 7 | `drop-src-suspect` | removed; the source is reported `suspect` ([RULES/delete-policy-matrix] open point 5) |
| 8 | `tombstone` | kept as a tombstone reference |
| 9 | `tombstone-src-suspect` | kept as a tombstone reference; the source becomes `suspect` |

On source deleted (`on_src`):

| value | name | effect |
|---|---|---|
| 1 | `drop` | removed |
| 2 | `drop-rollups` | removed; the parent's rollups are updated |
| 3 | `repoint-or-flag` | re-pointed to `--replaced-by`; else, under the default policy, kept as a `flagged` retained out-edge when the source was an open blocker (an unfinished `blocks` source, a gating `gates` verdict) and removed otherwise (X4; [RULES/delete-policy-matrix] EG-008, EG-009, EG-014, EG-015); under the policy `drop-notify` of `edges.<kind>.on-src-deleted`, removed with a notice (EG-010, EG-016) |
| 4 | `drop-reopen` | removed; the question reopens |
| 5 | `retain-warn` | kept as a retained out-edge; the target stays as it is and `doctor` warns |
| 6 | `retain` | kept as a retained out-edge (I39′) |
| 7 | `recompute` | recomputed from the text (`mentions`, §10.4) |
| 8 | `retain-anchors` | kept with its anchors as retained out-edges (I39′) |

Acyclicity (`acyclic`): 0 `none`; 1 `forest` (with `max_depth`); 2 `precedence` (part of the combined precedence graph
of I5′); 3 `dag`; 4 `by-construction` (acyclic by construction on one branch; checked at merge,
[RULES/merge-table] VA-016).

Cardinality (`card`):

| value | name | rule |
|---|---|---|
| 0 | `many` | none |
| 1 | `max-1-per-src` | a source has at most one such out-edge |
| 2 | `max-1-active-per-dst` | a destination has at most one such in-edge whose source is **active**: live, with a status that is not a side state |
| 3 | `chain-1` | a source has at most one such out-edge, and its destination has none (I7) |
| 4 | `typical-1` | informative "≤ 1 typical"; not enforced |
| 5 | `anchors-min-1` | every edge carries at least one anchor (I-F3) |

Properties (`props`): 0 `none`; 1 `pinned` (an optional `pinned_commit`); 2 `flagged`; 3 `anchor` (§10.2).

#### 8.4.7 `uid_derivation` (R-3)

| value | name | on | derivation |
|---|---|---|---|
| 0 | `none` | edge kinds without a discriminator | — |
| 1 | `random` | kinds | §2.2 |
| 2 | `file-key` | kinds | §11.2 |
| 3 | `root-key` | the root nodes of a kind (`root_variant`) | §11.3 |
| 4 | `anchor-key` | the `at` edge kind: its discriminator | §11.4 |

#### 8.4.8 `KindSet` (F1 `src_kinds`, `dst_kinds`)

[50] F1: "u64 masks; project kinds ≥ 64 use an extension mask". 33 bytes:

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 1 | `u8` | `ks_flags` | bit 0 `any`: every kind, including project kinds added later; bits 1–7 reserved-zero |
| 1 | 8 | `u64` | `core` | bit k set ⇔ kind id k (1–63) is in the set; bit 0 is zero; zero when `any` |
| 9 | 24 | `[24]u8` | `ext` | bit i ([F01 §4.4]) set ⇔ project kind id 64 + i is in the set; zero when `any` |
| total | 33 | | | |

`ext` uses store-local ids; the canonical form lists project kinds by name ([F07]).

### 8.5 Item records

Every item starts with:

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `class` | `u8` | always | 1 kind, 2 field, 3 enumeration value, 4 edge kind, 5 named query |
| 2 | `iflags` | `u8` | always | bit 0 `retired` (not for class 5); bits 1–7 reserved-zero |
| 3 | … | the class's body below | always | |

The item **key** (what the canonical form sorts and merges by) is: kind name; (kind name or `*`, field name); (kind name
or `*`, field name, value name); edge kind name; query name. Every other part of the body is the item's value, except
the parts marked *store-local*, which are not hashed; symbol ids are hashed as their strings ([F07]).

**Item key order** (pass 1, A1-41). Wherever a stored structure sorts items by key (the view's schema section,
[F09 §8.3]), items are ordered by (`class`, then the key's components in the order above), each component compared as its
**name string** bytewise ([F01 §6.6]), never as a symbol id; `*` is the one-byte string `2A`, which sorts before every
name of §8.2. Two items of one view never have equal keys. The canonical order of schema entries is [F07 §10.3]'s.

#### 8.5.1 Kind

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `name` | `uvar32` | always | kind name (class `name`) |
| 2 | `kind_id` | `u8` | always | *store-local*: §8.3 |
| 3 | `uid_derivation` | `u8` | always | §8.4.7: `random` for every project kind |
| 4 | `root_variant` | `u8` | always | 0, or `root-key` when an instance whose `root` field is present is a root node (`area` only, §11.3) |
| 5 | `existence_policy` | `u8` | always | §8.4.5 |
| 6 | `kflags` | `u8` | always | bit 0 `title_derived` (the title is derived from `path`); bit 1 `immutable_fields` (fields are read-only after `Create`; statuses still move); bit 2 `has_done` (the kind has the virtual `done` field); bit 3 `done_derived` (`done` follows the derived `answered` predicate); bits 4–7 reserved-zero |

A project kind's statuses are enumeration items of its field `status`; it declares at least one, and the default of its
`status` field is its initial status.

#### 8.5.2 Field

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `kind` | `uvar32` | always | kind name symbol, or 0 for `*` (every kind; core items only) |
| 2 | `name` | `uvar32` | always | field name (class `name`) |
| 3 | `type` | `u8` | always | a type byte with `vbit` 0 (§5.1) |
| 4 | `elem` | `u8` | always | the element type byte for `set`; 0 otherwise |
| 5 | `class` | `u8` | always | merge class, §8.4.1 |
| 6 | `storage` | `u8` | always | §8.4.2 |
| 7 | `decl` | `u16` | always | declaration order: the order of `show` ([LQ/envelope §5.6]); a new project field takes the view's largest `decl` + 1; ties are ordered by field name |
| 8 | `optional` | `bool8` | always | F2: the field may be absent ([50 §3.3]) |
| 9 | `index` | `u8` | always | F2/F5, §8.4.3 |
| 10 | `coerce` | `u8` | always | F2, §8.4.4 |
| 11 | `cflags` | `u8` | always | bit 0 `has_default`; bit 1 `has_range`; bit 2 `one_line`; bit 3 `ascii` (every byte 20–7E); bits 4–7 reserved-zero |
| 12 | `default` | type byte + value | `has_default` | F2: the value a non-optional field has when not written; never stored in a field block (§6.2) |
| 13 | `range_min` | `svar64` | `has_range` | least `int` value |
| 14 | `range_max` | `svar64` | `has_range` | greatest `int` value, ≥ `range_min` |

A non-optional field has a default, except a required field (one that must be written at `Create`): `optional` =
false and no default.

#### 8.5.3 Enumeration value

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `kind` | `uvar32` | always | kind name symbol, or 0 for `*` |
| 2 | `field` | `uvar32` | always | field name symbol |
| 3 | `name` | `uvar32` | always | value name (class `name`) |
| 4 | `value` | `uvar16` | always | *store-local* for project values (§8.3): the stored integer |
| 5 | `sort_rank` | `u16` | always | F2: the value's rank in `ORDER BY` and comparisons ([50 §3.5]) |
| 6 | `eflags` | `u8` | always | bit 0 `side` (a side state, [RULES/merge-table] §6); bit 1 `done` (makes the virtual `done` true); bits 2–7 reserved-zero |
| 7 | `n_covers` | `u8` | always | number of `covers` |
| 8 | `covers` | `n_covers` × `uvar16` | always | the values this value lies immediately above in the merge lattice ([RULES/merge-table] §6); empty for a side state; *store-local* integers, named in canonical forms |

The **lattice** of an enumeration field is the transitive closure of `covers`. It is used when the field's class is
`status`; a `scalar` enumeration has no covers.

#### 8.5.4 Edge kind (with [50] F1)

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `name` | `uvar32` | always | stored edge kind name (class `name`) |
| 2 | `edge_id` | `u8` | always | *store-local* for project edge kinds (§8.3) |
| 3 | `eclass` | `u8` | always | §8.4.6 |
| 4 | `on_dst` | `u8` | always | §8.4.6 |
| 5 | `on_src` | `u8` | always | §8.4.6 |
| 6 | `acyclic` | `u8` | always | §8.4.6 |
| 7 | `card` | `u8` | always | §8.4.6 |
| 8 | `max_depth` | `u8` | always | largest number of ancestors under a `forest` kind; 0 otherwise |
| 9 | `uid_derivation` | `u8` | always | `none`, or `anchor-key` (§8.4.7) |
| 10 | `props` | `u8` | always | §8.4.6 |
| 11 | `eflags` | `u8` | always | bit 0 `symmetric` (F1); bit 1 `same_kind` (source and destination must have the same kind); bits 2–7 reserved-zero |
| 12 | `lq_name` | `uvar32` | always | F1: the LQ name (class `name`) |
| 13 | `src_kinds` | `KindSet` | always | F1 |
| 14 | `dst_kinds` | `KindSet` | always | F1 |
| 15 | `n_reverse` | `u8` | always | number of reverse names |
| 16 | `reverse_names` | `n_reverse` × `uvar32` | always | F1: reverse aliases (class `name`) |
| 17 | `reading` | `vstr` | always | F1: the reading template: ASCII, one line, 1–200 bytes, containing `{a}` and `{b}` exactly once each ([LQ/envelope §4.4]) |

Project edge kinds are `historical` with `on_dst` = `tombstone`, `on_src` = `retain`, `props` = `none` and
`uid_derivation` = `none`; their `acyclic` and `card` are free.

#### 8.5.5 Named query (F3)

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `name` | `uvar32` | always | the query name (class `name`) |
| 2 | `lq_version` | `u16` | always | the LQ grammar version the text is written in; ≥ 1 |
| 3 | `params` | `vstr` | always | the parameter signature, in the portable text form of the image's `params:` line ([50 §4.4], [F14]); empty for none |
| 4 | `shape` | `vstr` | always | the shape word ([LQ/std]) |
| 5 | `budget` | `vstr` | always | the budget-class word ([LQ/std]) |
| 6 | `text` | `vstr` | always | the query text in portable form: LF only, no trailing whitespace, comments kept, every store-local constant rewritten ([50 §4.4]) |
| 7 | `ast_hash` | `b16` | always | *derived, unhashed*: BLAKE3-128 of the canonical bound AST over the portable form ([LQ/canonical-ast]); all zero until computed; recomputed on import and by `doctor --verify` |

Fields 1–6 are hashed ([50] F3). The item is one atomic merge value ([RULES/merge-table] MC-014).

### 8.6 Schema conformance at write time (I11)

A write is refused ([F19]) unless its result conforms to the effective schema of the written view:

1. the node's kind is a non-retired kind; its header enumerations hold non-retired values of its kind; every field
   entry satisfies §6.2; no required field is absent (a finding's `failure_scenario`, an artifact's `root`,
   `origin_path` and `path`, a root node's `root`);
2. identity values never change after `Create`: `uid`, `kind`, `CREATOR`, the fields of class `identity`, an artifact's
   `root` (a derivation input, §11.2), a root node's title; the fields of a kind with `immutable_fields` (`verdict`)
   never change after `Create` ([AR §3.2]); a `decision` whose status is `accepted` takes no field change (it is only
   superseded, [AR §3.2]);
3. an artifact's `path`, `origin_path` and `aliases` have its `root` as their root (I-F8, review S-19); a root node's
   `path_moves` entries have its `root` as their root;
4. a rule whose `authority` is `owner` has an `owner_quote` ([AR §3.2]);
5. an edge satisfies its kind's endpoint kinds, `same_kind`, `props` and cardinality at write time (§10);
6. schema items obey §8.2–§8.5; an item that retires or changes a core item is refused.

[F13] lists these with their enforcement points (EP-W4 and the merge validator V09) and gates.

## 9. The core schema, version 1

These tables are the genesis schema that the reference model transcribes ([RULES/merge-table] §8). Every enumeration's
`sort_rank` equals its integer.

### 9.1 Kinds

| id | kind | `uid_derivation` | `root_variant` | existence policy | `kflags` | initial status (default) |
|---|---|---|---|---|---|---|
| 1 | `task` | random | — | delete-wins | `has_done` | `open` |
| 2 | `doc` | random | — | resurrect | — | `draft` |
| 3 | `note` | random | — | resurrect | — | `active` |
| 4 | `rule` | random | — | resurrect | — | `proposed` |
| 5 | `decision` | random | — | resurrect | — | `proposed` |
| 6 | `question` | random | — | resurrect | `has_done`, `done_derived` | `open` |
| 7 | `finding` | random | — | resurrect | — | `open` |
| 8 | `verdict` | random | — | resurrect | `has_done`, `immutable_fields` | `open` |
| 9 | `measurement` | random | — | resurrect | — | `current` |
| 10 | `artifact` | file-key | — | none | `title_derived` | `present` (`planned` through `--planned`) |
| 11 | `run` | random | — | resurrect | — | `running` |
| 12 | `lane` | random | — | resurrect | — | `active` |
| 13 | `area` | random | root-key | resurrect (root nodes: none) | — | `active` |

The existence policies are [AR §5a.7]'s ("tasks delete-wins, knowledge resurrect", "no automatic policy for
`artifact`") completed by [RULES/merge-table] EP-006–EP-013. The virtual `done` exists only for task, question and
verdict ([AR §3.1]; [RULES/status-machines] open point 21); a question's `done` is the derived `answered` predicate, a
visible live `answers` edge ([AR §3.5]; [RULES/status-machines] ST-024).

### 9.2 Common fields (kind `*`)

Every kind has these rows, in this declaration order. "req." = required; "def." = default.

| decl | field | type | class | storage | optional / default | index | coerce | constraints |
|---|---|---|---|---|---|---|---|---|
| 1 | `title` | `text` | scalar | title | req. (artifact: derived, not stored) | — | — | one line, 1–200 B (§7.1) |
| 2 | `abstract` | `text` | scalar | field | optional | — | — | one line |
| 3 | `status` | `enum` | status | header | def. the kind's initial status (§9.1) | — | — | the kind's values (§9.5) |
| 4 | `resolution` | `enum` | status | header | def. `none` | — | — | §3.1 column rule |
| 5 | `priority` | `enum` | scalar | header | def. `P2` | — | priority | — |
| 6 | `criticality` | `enum` | scalar | header | def. `normal` | — | — | — |
| 7 | `confidence` | `enum` | scalar | header | def. `unset` | — | — | `confirmed` and `plausible` only on findings |
| 8 | `authority` | `enum` | authority | header | def. `agent` | — | — | — |
| 9 | `parent` | `ref` | hierarchy | header | optional | — | — | §10.1 |
| 10 | `order` | `text` | hierarchy | field | optional | — | — | §5.4.4; one line, ASCII |
| 11 | `labels` | `set` of `sym` | set | field | optional | bitmap | — | one line each |
| 12 | `pinned` | `bool` | scalar | flag | def. false | — | — | — |
| 13 | `archived` | `bool` | scalar | flag | def. false | — | — | — |
| 14 | `frozen` | `bool` | scalar | flag | def. false | — | — | — |
| 15 | `defer_until` | `int` | scalar | cold | optional | — | timestamp | 1 … 2^32 − 1 |
| 16 | `due` | `int` | scalar | cold | optional | — | timestamp | 1 … 2^32 − 1 |
| 17 | `reason` | `text` | scalar | field | optional | — | — | the reason of a side-state transition: `retract`, `file rm`, `links fix --drop` (open point 16) |
| 18 | `body` | body | text (doc sections: section-text) | body | optional | — | — | §7.2 |

`uid`, `kind`, `CREATOR`, `topo`, `rev_seq`, `created_tx`, `updated_tx`, `last_op_lsn` and the derived columns and flags
are not field items: LQ exposes them as built-in properties ([50 §2.5]).

### 9.3 Kind fields

Kind fields follow the common ones; `decl` starts at 20 in each kind. Every kind field is optional unless marked "req."
or given a default. `sym` fields store interned text; `text` fields inline text.

**task**

| decl | field | type | class | optional / default | index | constraints |
|---|---|---|---|---|---|---|
| 20 | `work_kind` | `enum` | scalar | optional | bitmap | §9.4 |
| 21 | `phase_state` | `enum` | scalar | optional | bitmap | §9.4; no merge lattice ([RULES/merge-table] OP-28) |
| 22 | `assignee` | `sym` | scalar | optional | bitmap | — |
| 23 | `acceptance` | `text` | text | optional | — | — |
| 24 | `files_owned` | `set` of `sym` | glob-set | optional | — | globs, §5.4.3 |
| 25 | `estimate` | `int` | scalar | optional | — | 0 … 65,535 |
| 26 | `reopen_count` | `counter` | counter | def. 0 | — | ≥ 0 |
| 27 | `reopen_if` | `text` | text | optional | — | — |
| 28 | `pre_registered` | `bool` | scalar | def. false | — | — |

**doc**

| decl | field | type | class | optional / default | index | constraints |
|---|---|---|---|---|---|---|
| 20 | `doc_kind` | `enum` | scalar | optional | — | §9.4 |
| 21 | `heading` | `text` | text | optional | — | one line |
| 22 | `revision` | `int` | scalar | optional | — | 0 … 65,535 |
| 23 | `changed_in_round` | `int` | scalar | optional | — | 0 … 65,535 |
| 24 | `targets` | `text` | scalar | optional | — | record list, §5.4.5 |
| 25 | `readiness` | `text` | scalar | optional | — | record list, §5.4.5 |

**note**

| decl | field | type | class | optional / default | index | constraints |
|---|---|---|---|---|---|---|
| 20 | `note_kind` | `enum` | scalar | optional | — | §9.4 |
| 21 | `symptom` | `text` | text | optional | — | — |
| 22 | `mechanism` | `text` | text | optional | — | — |
| 23 | `defence` | `text` | text | optional | — | — |
| 24 | `incidents` | `counter` | counter | def. 0 | — | ≥ 0 |
| 25 | `applies_to` | `set` of `sym` | glob-set | optional | — | tagged elements, §5.4.6; empty means `*` |
| 26 | `observed_git_sha` | `oid` | scalar | optional | — | a git commit id |
| 27 | `review_after` | `int` | scalar | optional | — | Unix seconds ≥ 0; coerce `timestamp` |

**rule**

| decl | field | type | class | optional / default | index | constraints |
|---|---|---|---|---|---|---|
| 20 | `text` | `text` | text | optional | — | — |
| 21 | `enforcement` | `enum` | scalar | optional | — | §9.4 |
| 22 | `applies_to` | `set` of `sym` | glob-set | optional | — | as note |
| 23 | `since` | `int` | scalar | optional | — | Unix seconds ≥ 0; coerce `timestamp` |
| 24 | `rationale` | `text` | text | optional | — | — |
| 25 | `owner_quote` | `text` | owner | optional; req. when `authority` = `owner` | — | verbatim |

**decision**

| decl | field | type | class | optional / default | index | constraints |
|---|---|---|---|---|---|---|
| 20 | `context` | `text` | text | optional | — | — |
| 21 | `what` | `text` | text | optional | — | — |
| 22 | `why` | `text` | text | optional | — | — |
| 23 | `tradeoff` | `text` | text | optional | — | — |
| 24 | `alternatives` | `text` | scalar | optional | — | record list, §5.4.5 |
| 25 | `revive_condition` | `text` | text | optional | — | the revive condition of a rejected candidate ([AR §2.12]) |
| 26 | `owner_quote` | `text` | owner | optional | — | verbatim |

**question**

| decl | field | type | class | optional / default | index | constraints |
|---|---|---|---|---|---|---|
| 20 | `q_kind` | `enum` | scalar | optional | — | §9.4 |
| 21 | `asked_of` | `enum` | scalar | optional | — | §9.4 |
| 22 | `options` | `text` | scalar | optional | — | record list, §5.4.5 |
| 23 | `answer` | `text` | text | optional | — | verbatim |

**finding**

| decl | field | type | class | optional / default | index | constraints |
|---|---|---|---|---|---|---|
| 20 | `local_id` | `sym` | scalar | optional | bitmap | the round-local id (`C1`, `W2`) |
| 21 | `severity` | `enum` | scalar | optional | bitmap | §9.4 |
| 22 | `f_kind` | `enum` | scalar | optional | bitmap | §9.4 |
| 23 | `failure_scenario` | `text` | text | req. | — | "mandatory" ([AR §3.2]) |
| 24 | `what_needed` | `text` | text | optional | — | — |
| 25 | `round` | `int` | scalar | optional | column | 0 … 65,535 |
| 26 | `evidence` | `text` | text | optional | — | — |

`finding.where` of [AR §3.2] is not a field: a `file:symbol@sha` place is an `at` edge with a `symbol` anchor, and a
section reference is an `about` edge to the section ([AR §3.3], [40 §2.8]; open point 23).

**verdict** (fields immutable after `Create`)

| decl | field | type | class | optional / default | index | constraints |
|---|---|---|---|---|---|---|
| 20 | `role` | `sym` | scalar | optional | — | a role name |
| 21 | `round` | `int` | scalar | optional | column | 0 … 65,535 |
| 22 | `raw_label` | `text` | text | optional | — | — |
| 23 | `outcome` | `enum` | scalar | optional | bitmap | §9.4 |
| 24 | `return_to` | `enum` | scalar | optional | — | §9.4 |
| 25 | `criteria` | `text` | text | optional | — | — |
| 26 | `conditions` | `text` | text | optional | — | — |

**measurement**

| decl | field | type | class | optional / default | index | constraints |
|---|---|---|---|---|---|---|
| 20 | `metric` | `sym` | scalar | optional | bitmap | — |
| 21 | `value` | `f64` | scalar | optional | — | §5.3 |
| 22 | `unit` | `sym` | scalar | optional | — | — |
| 23 | `target` | `f64` | scalar | optional | — | §5.3 |
| 24 | `command` | `text` | text | optional | — | — |
| 25 | `measured_on` | `oid` | scalar | optional | — | a git commit id ("sha + algo") |
| 26 | `env_host` | `sym` | scalar | optional | — | [AR §3.2] `env.host` |
| 27 | `env_profile` | `sym` | scalar | optional | — | `env.profile` |
| 28 | `env_load` | `enum` | scalar | optional | — | `env.load`, §9.4 |
| 29 | `env_scale` | `sym` | scalar | optional | — | `env.scale` |
| 30 | `baseline` | `ref` | scalar | optional | — | — |

The tester's "env + `measured_on` mandatory" is a role-policy rule ([AR §7.3], [RULES/role-write-policy]), not a schema
requirement.

**artifact** (the file node, R-2; [40 §2.2])

| decl | field | type | class | optional / default | index | constraints |
|---|---|---|---|---|---|---|
| 20 | `root` | `sym` | scalar | req.; immutable | — | a root name (§5.4.1) |
| 21 | `origin_path` | `path` | identity | req. | — | the registration path (§11.2) |
| 22 | `origin_pred` | `ref` | identity | optional | — | the derivation's predecessor uid; absent when there was none |
| 23 | `path` | `path` | observation | req. | — | the current path |
| 24 | `oid` | `oid` | observation | optional | — | content id of [40 §2.5] with the root's algorithm ([F20 §2.3]) |
| 25 | `bytes` | `int` | observation | optional | — | raw size, ≥ 0 |
| 26 | `observed_git` | `oid` | observation | optional | — | HEAD of the observing tree; absent without git |
| 27 | `observed_blob` | `oid` | observation | optional | — | git's blob id at `path` in τ(`observed_git`); absent for an uncommitted observation |
| 28 | `relink` | `sym` | observation | optional | — | ASCII; the closed R-17 grammar of [40 §2.2] ([F18]) |
| 29 | `aliases` | `set` of `path` | alias-set | optional | — | former paths |
| 30 | `artifact_kind` | `enum` | scalar | optional | — | §9.4 |
| 31 | `replaced_by` | `ref` | scalar | optional | — | set by `file rm` and `links fix --drop` |
| 32 | `excerpt` | `text` | scalar | optional | — | — |

The six fields of class `observation` form one merge key ([40 §2.2]). `reason` (common) carries the removal reason.
The title is derived from `path`.

**run**

| decl | field | type | class | optional / default | index | constraints |
|---|---|---|---|---|---|---|
| 20 | `wf_id` | `text` | scalar | optional | — | one line |
| 21 | `bg_task_id` | `text` | scalar | optional | — | one line |
| 22 | `session_id` | `text` | scalar | optional | — | one line |
| 23 | `script_path` | `path` | scalar | optional | — | root `abs` ([40 §2.8]; open point 22) |
| 24 | `args_hash` | `text` | scalar | optional | — | one line |
| 25 | `journal_path` | `path` | scalar | optional | — | root `abs` |
| 26 | `started` | `int` | scalar | optional | — | Unix seconds ≥ 0; coerce `timestamp` |
| 27 | `ended` | `int` | scalar | optional | — | Unix seconds ≥ 0; coerce `timestamp` |
| 28 | `expected_artifacts` | `set` of `sym` | set | optional | — | — |

**lane** (no versioned dirty count: a worktree's dirty count is the runtime `TREES.dirty` row, [F11], [70 S5])

| decl | field | type | class | optional / default | index | constraints |
|---|---|---|---|---|---|---|
| 20 | `worktree_path` | `path` | scalar | optional | — | root `abs` |
| 21 | `git_branch` | `sym` | scalar | optional | — | — |
| 22 | `base_sha` | `oid` | scalar | optional | — | a git commit id |
| 23 | `tip_sha` | `oid` | scalar | optional | — | a git commit id |
| 24 | `target_dir` | `path` | scalar | optional | — | root `abs` |
| 25 | `moirai_branch` | `sym` | scalar | optional | — | a ref name ([F12]) |

**area**

| decl | field | type | class | optional / default | index | constraints |
|---|---|---|---|---|---|---|
| 20 | `path_globs` | `set` of `sym` | glob-set | optional | — | globs, §5.4.3 |
| 21 | `root` | `sym` | identity | optional; req. on a root node | — | a root name; present exactly on root nodes (§11.3) |
| 22 | `path_moves` | `set` of `pathmove` | pathmove-set | optional | — | root nodes only (R-5) |

### 9.4 Enumerations

Header enumerations (kind `*`):

| field | values (integer name) |
|---|---|
| `resolution` | 0 `none`, 1 `completed`, 2 `wontdo`, 3 `duplicate`, 4 `superseded`, 5 `obsolete`, 6 `rework` |
| `priority` | 0 `P0`, 1 `P1`, 2 `P2`, 3 `P3`, 4 `P4` |
| `criticality` | 0 `critical`, 1 `high`, 2 `normal`, 3 `low` |
| `confidence` | 0 `unset`, 1 `verified`, 2 `observed`, 3 `inferred`, 4 `speculative`, 5 `confirmed`, 6 `plausible` |
| `authority` | 0 `owner`, 1 `orchestrator`, 2 `measured`, 3 `research`, 4 `agent` |

Field enumerations:

| kind.field | values (integer name) |
|---|---|
| `task.work_kind` | 0 `design`, 1 `impl`, 2 `fix`, 3 `test`, 4 `measure`, 5 `merge`, 6 `doc`, 7 `research`, 8 `review`, 9 `debt`, 10 `mutex` |
| `task.phase_state` | 0 `proposed`, 1 `researching`, 2 `designing`, 3 `design_review`, 4 `refuting`, 5 `design_approved`, 6 `implementing`, 7 `code_review`, 8 `testing`, 9 `analysis`, 10 `accepted`, 11 `committed`, 12 `merged`, 13 `documented`; side values 14 `blocked`, 15 `frozen`, 16 `deferred` |
| `doc.doc_kind` | 0 `plan`, 1 `section`, 2 `report`, 3 `patch` |
| `note.note_kind` | 0 `note`, 1 `hazard`, 2 `lesson`, 3 `checkpoint`, 4 `summary` |
| `rule.enforcement` | 0 `must`, 1 `should` |
| `question.q_kind` | 0 `values`, 1 `scope`, 2 `unclear` |
| `question.asked_of` | 0 `owner`, 1 `orchestrator`, 2 `architect` |
| `finding.severity` | 0 `blocker`, 1 `important`, 2 `optional` |
| `finding.f_kind` | 0 `correctness`, 1 `perf`, 2 `complexity`, 3 `security`, 4 `plan`, 5 `style`, 6 `debt`, 7 `test`, 8 `deviation` |
| `verdict.outcome` | 0 `pass`, 1 `pass_with_conditions`, 2 `fail_fixable`, 3 `fail_fundamental`, 4 `unknown`, 5 `na` |
| `verdict.return_to` | 0 `architect`, 1 `developer`, 2 `tester`, 3 `none` |
| `measurement.env_load` | 0 `quiet`, 1 `loaded` |
| `artifact.artifact_kind` | run-output kinds 0 `design`, 1 `critique`, 2 `research`, 3 `impl`, 4 `test`, 5 `review`, 6 `triage`, 7 `fix`, 8 `merge`, 9 `verify`, 10 `patch`, 11 `manifest`, 12 `message`, 13 `page`, 14 `plan_file`; file kinds 15 `source`, 16 `doc`, 17 `asset`, 18 `generated`, 19 `dir` |

The ranks give the orders of [50 §3.5]: P0 first; `critical` < `high` < `normal` < `low`; `blocker` < `important` <
`optional`; `owner` < `orchestrator` < `measured` < `research` < `agent`.

### 9.5 Statuses and lattices

The `status` values of each kind ([AR §3.2]), with the merge lattice of [RULES/merge-table] SL-001–SL-055: "covers"
names the values immediately below; a side state covers nothing; "done" marks the values that make the virtual `done`
true.

| kind | value: name (covers) — side states | done |
|---|---|---|
| task | 0 `open`, 1 `in_progress` (open), 2 `done` (in_progress) — side 3 `deferred`, 4 `cancelled`, 5 `frozen` | `done`, `cancelled` |
| doc | 0 `draft`, 1 `current` (draft) — side 2 `superseded`, 3 `archived` | — |
| note | 0 `active` — side 1 `superseded`, 2 `retracted`, 3 `archived` | — |
| rule | 0 `proposed`, 1 `active` (proposed) — side 2 `superseded`, 3 `retracted`, 4 `archived` | — |
| decision | 0 `proposed`, 1 `accepted` (proposed) — side 2 `rejected`, 3 `superseded` | — |
| question | 0 `open`, 1 `answered` (open) — side 2 `dropped` | derived (§9.1) |
| finding | 0 `open`, 1 `confirmed` (open), 2 `refuted` (open), 3 `fixed` (confirmed), 4 `deferred` (open), 5 `withdrawn` (open) | — |
| verdict | 0 `open`, 1 `accepted` (open) — side 2 `superseded` | `accepted` |
| measurement | 0 `current` — side 1 `moved_declared`, 2 `retracted` | — |
| artifact | 0 `planned`, 1 `present` (planned) — side 2 `removed` | — |
| run | 0 `running`, 1 `green` (running), 2 `red` (running), 3 `stopped` (running), 4 `died` (running) | — |
| lane | 0 `active`, 1 `ready_to_merge` (active), 2 `merge_pending` (ready_to_merge), 3 `merged` (merge_pending) — side 4 `frozen`, 5 `abandoned`, 6 `measuring` | — |
| area | 0 `active` — side 1 `archived` | — |

The status machines — which transitions a write may make — are [RULES/status-machines]'s. `answered` is a stored value,
moved by guarded writes; `done` of a question follows the edge (open point 17). The finding lattice follows [AR §3.6]'s
transitions (open point 19).

### 9.6 Edge kinds

The 25 core edge kinds of [AR §3.3] with [50 §2.5]'s F1 values. "any" is `KindSet.any`. "Knowledge" is {`doc`,
`note`, `rule`, `decision`} ([AR §3.6]).

| id | edge | class | src → dst (F1) | flags | acyclic | card | on_dst | on_src | props |
|---|---|---|---|---|---|---|---|---|---|
| 1 | `parent` | structural | {task, doc, area} → {task, doc, area} | same_kind | forest, `max_depth` 12 | max-1-per-src | restrict-cascade-reparent | drop-rollups | none |
| 2 | `blocks` | structural | {task, question} → {task} | — | precedence | many | drop | repoint-or-flag | flagged |
| 3 | `gates` | structural | {verdict} → {task} | — | precedence | many | drop | repoint-or-flag | flagged |
| 4 | `merge_after` | structural | {lane} → {lane} | — | dag | many | drop-notify | drop | none |
| 5 | `runs_in` | structural | {run} → {lane} | — | none | max-1-per-src | restrict | drop | none |
| 6 | `answers` | structural | {decision, note} → {question} | — | none | max-1-active-per-dst | restrict | drop-reopen | none |
| 7 | `scoped_to` | structural | {note, rule, decision, finding, measurement} → {area} | — | none | many | restrict-reassign | drop | none |
| 8 | `duplicate_of` | structural | any → any | same_kind | none | chain-1 | restrict-repoint | drop | none |
| 9 | `depends_on` | structural | {doc} → {doc} | — | dag | many | drop-src-suspect | drop | none |
| 10 | `supersedes` | historical | knowledge → knowledge | same_kind | dag | max-1-active-per-dst | tombstone | retain-warn | none |
| 11 | `derived_from` | historical | {note, doc, verdict, artifact} → any | — | by-construction | many | tombstone-src-suspect | retain | pinned |
| 12 | `cites` | historical | any → knowledge | — | none | many | tombstone-src-suspect | retain | pinned |
| 13 | `implements` | historical | {task, artifact} → {decision, doc} | — | none | many | tombstone-src-suspect | retain | pinned |
| 14 | `refutes` | historical | {finding, measurement} → {finding, decision, rule} | — | none | many | tombstone | retain | none |
| 15 | `confirms` | historical | {finding, measurement} → {finding, decision, rule} | — | none | many | tombstone | retain | none |
| 16 | `verifies` | historical | {measurement, verdict} → {finding, task, decision} | — | none | many | tombstone | retain | none |
| 17 | `addresses` | historical | {task, artifact} → {finding} | — | none | many | tombstone | retain | none |
| 18 | `about` | historical | {finding, verdict, measurement, question, rule, decision, note} → any | — | none | typical-1 | tombstone | retain | none |
| 19 | `discovered_from` | historical | any → {task} | — | by-construction | many | tombstone | retain | none |
| 20 | `produced` | historical | {run} → any | — | none | many | tombstone | retain | none |
| 21 | `consumed` | historical | {run} → any | — | none | many | tombstone | retain | pinned |
| 22 | `contradicts` | historical | {rule} → {rule} | symmetric | none | many | tombstone | retain | none |
| 23 | `mentions` | historical | any → any | — | none | many | tombstone | recompute | none |
| 24 | `relates` | historical | any → any | symmetric | none | many | tombstone | retain | none |
| 25 | `at` | historical | any → {artifact} | — | none | anchors-min-1 | tombstone-src-suspect | retain-anchors | anchor |

`uid_derivation` is `anchor-key` for `at` and `none` for the others. Ids 26–63 are reserved for core edge kinds of later
schema versions; 0 and 255 are invalid.

The `on_dst` and `on_src` values name each core kind's default action on a delete. [RULES/delete-policy-matrix]
`edge-policy` refines them per delete option (`--replaced-by`, `--cascade`, `--reparent`, `--reassign`), per value of the
policy data `edges.<kind>.on-src-deleted` and per condition on the other endpoint, and is authoritative for the action
and its effect (review pass 1 S1-46); the value stored in the schema row does not change. A project edge kind is always
`tombstone`/`retain` (§8.5.4), so no refinement applies to it.

F1's LQ columns:

| edge | `lq_name` | `reverse_names` | `reading` ([LQ/envelope §4.5]) |
|---|---|---|---|
| `parent` | `CHILD_OF` | `PARENT_OF`, `HAS_CHILD`, `HAS_SUBTASK` | `{a} is a child of {b}` |
| `blocks` | `BLOCKS` | `BLOCKED_BY` | `{a} must finish before {b} starts` |
| `gates` | `GATES` | `GATED_BY` | `verdict {a} gates the completion of {b}` |
| `merge_after` | `MERGE_AFTER` | — | `lane {a} merges after lane {b}` |
| `runs_in` | `RUNS_IN` | — | `run {a} runs in lane {b}` |
| `answers` | `ANSWERS` | `ANSWERED_BY` | `{a} answers question {b}` |
| `scoped_to` | `SCOPED_TO` | — | `{a} is scoped to area {b}` |
| `duplicate_of` | `DUPLICATE_OF` | — | `{a} duplicates canonical {b}` |
| `depends_on` | `DEPENDS_ON` | — | `section {a} depends on section {b}` |
| `supersedes` | `SUPERSEDES` | `SUPERSEDED_BY` | `{a} supersedes {b}` |
| `derived_from` | `DERIVED_FROM` | — | `{a} is derived from {b}` |
| `cites` | `CITES` | `CITED_BY` | `{a} cites {b}` |
| `implements` | `IMPLEMENTS` | `IMPLEMENTED_BY` | `{a} implements {b}` |
| `refutes` | `REFUTES` | `REFUTED_BY` | `{a} refutes {b}` |
| `confirms` | `CONFIRMS` | `CONFIRMED_BY` | `{a} confirms {b}` |
| `verifies` | `VERIFIES` | `VERIFIED_BY` | `{a} verifies {b}` |
| `addresses` | `ADDRESSES` | `ADDRESSED_BY` | `{a} addresses finding {b}` |
| `about` | `ABOUT` | — | `{a} is about {b}` |
| `discovered_from` | `DISCOVERED_FROM` | — | `{a} was discovered from task {b}` |
| `produced` | `PRODUCED` | — | `run {a} produced {b}` |
| `consumed` | `CONSUMED` | — | `run {a} consumed {b}` |
| `contradicts` | `CONTRADICTS` | — | `{a} and {b} contradict` |
| `mentions` | `MENTIONS` | `MENTIONED_BY` | `{a} mentions {b}` |
| `relates` | `RELATES` | — | `{a} and {b} relate` |
| `at` | `AT` | — | `{a} is anchored in file {b}` |

[50 §2.5]'s forward synonym `SUBTASK_OF` of `CHILD_OF` is not a reverse name; LQ's alias table carries it (open point
43).

## 10. Edges

### 10.1 Edge keys

- An edge is keyed by (source, kind, destination, discriminator) with set semantics, and is stored in both directions in
  the same commit (I-P3, [AR §3.3]; the adjacency is [F09]'s). In the store the endpoints are `#N` and the kind is its
  edge id; canonical forms use uids and names ([F07]).
- The **discriminator** is a `b16`: the anchor uid on an `at` edge (R-4), empty on every other kind. An `at` edge key is
  (src, `at`, dst, anchor uid); the adjacency holds one entry per (src, dst) and the anchors per (src, dst, anchor)
  ([40 §2.8], R-8, [F09]).
- **Symmetric kinds** (`contradicts`, `relates`) are stored with the endpoint whose uid is bytewise smaller as the source,
  so a pair written in either direction is one key ([RULES/merge-table] open point 25). A pair of equal endpoints is
  refused.
- **`parent`** is held in `NodeHdr.parent`; [F09] states how its reverse direction is indexed. Its merge key is the
  hierarchy key (uid) → (parent uid, `order`) ([AR §4.6]).
- **Self-edges** are refused for every kind.

### 10.2 Property blocks

Every edge value carries one **edge property block** ([AR §3.3], [60 §2.5] typed property blocks). This section is the
only definition of its bytes (pass 1, S1-2, A1-3, P1-1): the ops of [F06 §7.5] carry it byte for byte, and [F09]'s
`EDGE_PROPS` row and `ANCHORS` record store its parts.

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `pflags` | `u8` | always | bit table below |
| 2 | `pinned_commit` | `b32` | `has_pin` | the full id of the pinned commit ([AR §3.3], [AR §7.4] step 5); ≠ all zero; the full id for the reasons of `commitref` (§5.1) |
| 3 | `anchor` | the anchor record (§10.3) | `anchor` | the anchor of an `at` edge |

`pflags`:

| bit | name | meaning |
|---|---|---|
| 0 | `has_pin` | `pinned_commit` follows |
| 1 | `flagged` | a retained out-edge of a deleted source, neither re-pointed nor resolved (X4, [AR §5b.2] rule 8) |
| 2 | `anchor` | the anchor record follows |

Bits 3–7 are reserved-zero. The bits an edge admits follow its kind's `props` (§8.4.6; checked at write, §8.6 rule 5, and
by the format oracle): `none` — `pflags` = 0; `pinned` — only `has_pin`, which is optional; `flagged` — only `flagged`;
`anchor` — exactly `anchor`, which is always set. So a block is one byte (`00`) on most edges, 33 bytes on a pinned edge
and 1 byte plus the anchor record on an `at` edge. A `flagged` edge exists only with a tombstone source; its destination has
`has_dangling` (§3.2).

### 10.3 The anchor record

An anchor is the property value of one `at` edge ([40 §2.7], R-4). Its store-local handle `aN` is allocated from
`HEAD.next_anchor` ([F04], R-6), carried unhashed by the op that creates the anchor ([F06 §7.5.1] `anchor_no`), and is
part of the `ANCHORS` key (src `#N`, dst `#N`, `aN`) ([F09]); it is not in the record.

**Owner** (pass 1, P1-1, S1-3, A1-2). This section is the only layout of the anchor record. The ops of [F06 §7.5] carry
it byte for byte inside the edge property block (§10.2), [F09]'s `ANCHORS` section stores it, [F07 §8.2] maps it to the
canonical selector block and [F14 §6.7] to the bijective `anchor` line. The record:

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `aflags` | `u16` | always | bit table below |
| 2 | `uid` | `b16` | always | the anchor uid, the edge's discriminator (§11.4) |
| 3 | `kind` | `u8` | always | 1 `file`, 2 `heading`, 3 `symbol`, 4 `quote`, 5 `range`, 6 `lines` |
| 4 | `mode` | `u8` | always | 1 `live`, 2 `pinned` |
| 5 | `watch` | `u8` | always | 1 `header`, 2 `span`; default `header` for `file`, `heading` and `symbol`, `span` for the others ([40 §2.7]) |
| 6 | `resolver` | `u16` | always | resolver version at capture ([F20 §1.3]); ≥ 1 |
| 7 | `captured` | `b16` | always | the capture digest (§11.4); never changes |
| 8 | `pred` | `b16` | `has_pred` | the predecessor term of the anchor uid; never changes |
| 9 | `hint_first` | `u32` | `kind` ≠ `file` | first line of the hint, 1-based ([F20 §6.1] step 7) |
| 10 | `hint_last` | `u32` | `kind` ≠ `file` | last line, ≥ `hint_first` |
| 11 | `scope` | `vbytes` | `has_scope` | the scope value (§10.3.1), non-empty |
| 12 | `quote` | `vbytes` | `kind` ∈ {heading, symbol, quote, range} and not `text_unavailable` | the quote's exact bytes of the normalised anchor text N ([F20 §2.5, §6.1]); for `range`, the start quote |
| 13 | `quote_h` | `b16` | as `quote`, but `text_unavailable` | BLAKE3-128 of those bytes |
| 14 | `prefix` | `vbytes` | as `quote` | the prefix as widened at capture; may be empty |
| 15 | `prefix_h` | `b16` | as `quote_h` | its digest |
| 16 | `suffix` | `vbytes` | as `quote` | the suffix; may be empty |
| 17 | `suffix_h` | `b16` | as `quote_h` | its digest |
| 18 | `end` | `vbytes` | `kind` = `range` and not `text_unavailable` | the end quote's exact bytes |
| 19 | `end_h` | `b16` | `kind` = `range` and `text_unavailable` | its digest ([40] R-10, review S-04) |
| 20 | `occurrence` | `u16` | `has_occurrence` | 1-based occurrence index ([F20 §6.1] step 8) |
| 21 | `window` | `vbytes` | `kind` ≠ `file` | the window value W of [F20 §2.7.3] |
| 22 | `span_hash` | `u64` | `kind` ≠ `file` | [F20 §2.8]; a `file` anchor has none: the field is omitted |
| 23 | `blob` | `oid`, or empty | always | the file's `oid` at capture ([F20 §2.3]); `algo` `none` (the single byte `00`) when the target had no content at capture: a planned target (`link --planned`), whose anchor is a `file` anchor |
| 24 | `git` | `oid` | `has_git` | the observed git commit at capture; `algo` ≠ `none` |
| 25 | `marker` | `vstr` | `has_marker` | the opt-in in-file marker id ([40 §9.2] decision 3); one line, 1–64 bytes |

`aflags`:

| bit | name | meaning |
|---|---|---|
| 0 | `has_pred` | `pred` is present |
| 1 | `has_scope` | `scope` is present |
| 2 | `has_occurrence` | `occurrence` is present |
| 3 | `has_git` | `git` is present |
| 4 | `has_marker` | `marker` is present |
| 5 | `text_unavailable` | the quote, prefix, suffix and end texts are not held; their digests are ([40 §5.7]: an anchor imported without its text) |

Bits 6–15 are reserved-zero. `quote`, `prefix`, `suffix` and `end` are the exact bytes of N ([F20 §2.5]); `is_text`
does not imply valid UTF-8 ([F20 §2.1]), so they are `vbytes` and no normalisation is ever applied to them. I-F9 holds by
construction: `heading`, `symbol`, `quote` and `range` carry a quote, `lines` a window. The selector fields (orders 3–6
and 9–25) form one merge key that changes only by a repin (`SetEdgeProps`, [F06]); `uid`, `captured` and `pred` never
change ([40 §2.7]). The canonical selector block, with quote, prefix, suffix and end entering only as digests, is [F07]'s (R-10);
the image line is [F14]'s (R-11).

**Validity** (V for every decoder: [F06], [F09], the format oracle). Every enumeration holds a listed value; `resolver` ≥ 1;
`hint_last` ≥ `hint_first` ≥ 1; `text_unavailable` only when `kind` carries a quote (`heading`, `symbol`, `quote`,
`range`); `scope`, `window` and `marker` are non-empty when present; `window` is a valid window value of [F20 §2.7.3];
`occurrence` ≥ 1; `blob` has `algo` `none` only when `kind` = `file`; `git` has `algo` ≠ `none`. **Consistency** (C):
when the texts are held, each digest [F07 §8.2] computes from them is the digest of the exact bytes; an import whose
`anchor` line carries both a text and a digest that disagree is `ImageParse` ([40 §5.7], [F14]).

#### 10.3.1 The scope value

The `scope` selector names the enclosing item a scope scanner found ([40 §2.7], [40 §2.7.1]). Its bytes, which also
enter `captured` (§11.4):

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `lang` | `u8` | always | 1 `rust`, 2 `markdown`, 3 `toml` |
| 2 | `n` | `u8` | always | number of segments, 1–64, outermost first |
| 3 | `segments` | `n` × `ScopeSeg` | always | |

`ScopeSeg`:

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `skind` | `u8` | always | Rust: 1 `mod`, 2 `impl`, 3 `fn`, 4 `struct`, 5 `enum`, 6 `trait`, 7 `const`, 8 `static`, 9 `macro_rules`. Markdown: the heading level 1–6. TOML: 1 `table`, 2 `array_table`, 3 `key` |
| 2 | `name` | `vstr` | always | the item's name as the scanner reports it (Rust: the type of an `impl`; Markdown: the heading text without its numbering; TOML: the table path or key); non-empty, one line |
| 3 | `qual` | `vstr` | always | Rust `impl Trait for T`: `Trait`; Markdown: the stripped numbering (`3.2`, `§3`); empty otherwise |

What the scanners report — item boundaries, names, numbering, TOML paths — is the scanner grammar of resolver version 1.
Pass 1 (P1-20, S1-4, A1-14) requires it as a normative appendix of [F20] with a fixture per construct, before the freeze
and before WP-63 is accepted; this section fixes only the bytes. **Until that appendix exists, no writer records a
scope**: `has_scope` is clear on every anchor a store captures, capture's uniqueness ladder skips its scope rung
([F20 §6.1] step 8.2), and `captured` takes `lp("")` for scope (§11.1). The scanners also decide the header line, and
so the quote, hint and header span hash, of a `symbol` or `heading` anchor, which enter `captured` and the hashed
selector block ([F07 §8.2]); so, under the same interim rule, no writer captures an anchor of kind 2 `heading` or 3
`symbol`: the `path#H` and `path::A/B` authoring forms are refused (exit 2, [F19 §10.2] `anchor_spec`), as
[F20 §6.1]'s interim scanner rule states. An imported anchor that carries a scope, or is of kind 2 or 3, keeps its
bytes as stored (its `captured` is trusted, §11.5). If the appendix is not written by the freeze, the owner decides
between keeping this interim rule in format v1 and removing the scanner-derived bytes from the hashed inputs (a change of
[PLAN] FB-4; `reviews/owner-questions.md` OQ-R-2).

### 10.4 `mentions`

`mentions` edges are maintained by the write path, never written directly ([AR §3.3]): at every write of a node's title,
abstract or body, its `mentions` out-edges become the set of nodes named by a **sigil** in its title, abstract and body
together. A sigil is `#`
followed by a maximal run of ASCII digits without a leading `0`, whose value N satisfies 1 ≤ N < `next_id`, where the `#`
is not preceded by an ASCII letter or digit and the run is not followed by `/` or by `.` and a digit ([AR §3.3], X6).
The edge's destination is the node `#N` store-wide (its uid through `ALLOC`), whether or not it exists on the view: a
historical edge may reference a node the view does not hold (I3). A sigil naming the node itself creates no edge
(§10.1). Merges and imports do not re-parse text ([RULES/merge-table] EC-023).

## 11. Derived identities (R-3, R-5)

### 11.1 Inputs of the derivations

Every derivation is BLAKE3-128 ([F01 §7.1]) over `lp()`-framed arguments ([F01 §6.3]). The arguments enter as:

| argument | bytes |
|---|---|
| a domain string (`"moirai-file-v1"`, `"moirai-root-v1"`, `"moirai-anchor-v1"`) | its ASCII bytes |
| a root name | its UTF-8 bytes (never its symbol id) |
| a path (`origin_path`) | the path text's exact bytes, without its root |
| a uid (`origin_pred`, file uid, src uid, `pred`) | its 16 bytes |
| "empty" (an absent predecessor, an absent occurrence, a window of a non-`lines` anchor) | the empty string: `lp` gives `00 00 00 00` |
| an anchor `kind` | its name (`file`, `heading`, `symbol`, `quote`, `range`, `lines`) as ASCII bytes ([F01] open point 9) |
| `scope` | the scope value's bytes (§10.3.1); empty when the anchor has none |
| `quote.exact`, `prefix.exact`, `suffix.exact`, `end.exact` | the exact bytes of N ([F20 §6.1]); empty when the kind has none |
| `occurrence` | its `u16`, 2 bytes little-endian; empty when absent |
| `window` | the window value W's bytes ([F20 §2.7.3]) when `kind` = `lines`; empty otherwise |
| `captured` | its 16 bytes |

### 11.2 File uids

```
uid_file(r, p, q) = BLAKE3-128( lp("moirai-file-v1") ‖ lp(r) ‖ lp(p) ‖ lp(q or empty) )
```

with r the root name, p the registration path, q the predecessor uid ([40 §2.3]).

**Registration** of the file at (root r, path p) on a view V — by capture, `file add`, `link --at` or `file mv`'s
destination — is:

1. If V holds a live artifact of root r with `path` = p (exact bytes) and status `present` or `planned`, that node is the
   file node; nothing is created (I-F1).
2. **Predecessor.** The candidates are the artifacts of V with root r that once held p and do not hold it now: live
   nodes with status `removed` whose `path` is p, and live nodes whose `aliases` contain p. Tombstones are not
   candidates ([AR §5b.2] rule 8). q is the candidate with the bytewise greatest uid, or empty when there is none
   ([40 §2.3], review S-01).
3. u = `uid_file(r, p, q)`.
4. **Dead uids are never re-created.** While u is the uid of any node of V — live at another path, `removed`, or a
   tombstone — set q = u and u = `uid_file(r, p, q)`. The loop reads V alone, so every store derives the same u for the
   same file, path and view ([40 §2.3]). It ends when u names no node of V; a loop that ran more times than V has
   artifact nodes is refused as an internal error (exit 1; only a BLAKE3 collision can cause it).
5. The node is created with uid u, `root` = r, `origin_path` = p, `origin_pred` = q (absent when empty), `path` = p,
   and the `#N` that `UIDX` gives u (§2.1).

`origin_path` is spelled as P7 says ([OS/path §3]): git's HEAD spelling for a tracked file, else the enumerated
spelling; never case-folded ([40 §2.3]). A uid created on one line while another line removed it is re-keyed at merge to
`uid_file(r, origin_path, U)` with U the removed uid — the rule of [40 §5.5], whose procedure is
[RULES/link-merge-rules] RK-001–RK-011 and [F12 §7.6] (open point 35). A foreign node whose uid does not equal its derivation
over its stored inputs is accepted as foreign, flagged by `image doctor`, and treated as random ([40 §2.3]).

*(Informative)* For root `project`, path `docs/a.md` and no predecessor, the 46 hashed bytes are
`0E 00 00 00` `6D 6F 69 72 61 69 2D 66 69 6C 65 2D 76 31` `07 00 00 00` `70 72 6F 6A 65 63 74` `09 00 00 00`
`64 6F 63 73 2F 61 2E 6D 64` `00 00 00 00`.

### 11.3 Root nodes (R-5)

```
uid_root(r) = BLAKE3-128( lp("moirai-root-v1") ‖ lp(r) )
```

- Each root that holds at least one file node on a view has one **root node** there: kind `area`, uid `uid_root(r)`,
  title `root:` + r, field `root` = r, field `path_moves` ([40 §2.4]). It is created in the commit that registers the
  root's first file node on that view, unless the view already holds it. Two lanes create the same uid, so a merge sees
  equal existence; a known uid reuses its `#N` (I-F2).
- An `area` is a root node exactly when its `root` field is present; its uid must equal `uid_root(root)` (I-F2). Only
  the write path that registers file nodes creates root nodes.
- A root node is never engine-deleted: `rm` of a root node is refused ([F19]), so the re-key case that a root-key uid
  cannot express never arises ([RULES/link-merge-rules] LM-013, its open point 4). Its title and `root` never change.
- **`path_moves`** is an ordinary set field of `pathmove` values (§5.2). There is no op, no canonical-form item, no image
  trailer and no commit annotation for directory moves ([40] R-5, [41 B4]); the entries' classes and what they drive are
  [40 §2.4]'s.
- A root node for root `abs` has no `path_moves`: `abs` files are never re-bound ([40 §2.4]).

### 11.4 Anchor uids

```
captured      = BLAKE3-128( lp(file uid at capture) ‖ lp(kind) ‖ lp(scope) ‖ lp(quote.exact) ‖ lp(prefix.exact)
                            ‖ lp(suffix.exact) ‖ lp(end.exact) ‖ lp(occurrence) ‖ lp(window if kind = lines, else empty) )
uid_anchor(s, c, p) = BLAKE3-128( lp("moirai-anchor-v1") ‖ lp(s) ‖ lp(c) ‖ lp(p or empty) )
```

with the argument bytes of §11.1, s the referrer's (source's) uid, c = `captured` and p the predecessor term
([40 §2.7], review S-03). `prefix` and `suffix` enter as widened by the capture's uniqueness ladder, and a `lines`
anchor's window as stored ([F20 §6.1]).

A capture of anchor A on the edge (s, `at`, f) of a view V is:

1. **De-duplication.** If an anchor on (s, f) has current selectors equal to A's — equal `kind`, `scope`, `quote`,
   `prefix`, `suffix`, `end`, `occurrence` and, for `lines`, `window` — that anchor is reused and nothing is created
   ([40 §2.7]; open point 36).
2. u = `uid_anchor(s, captured, empty)`.
3. While u is the uid of an anchor on (s, f) in V, set p = u and u = `uid_anchor(s, captured, p)`; the bound of §11.2
   step 4 applies with V's anchors on (s, f).
4. The anchor is created with uid u, `captured`, and `pred` = p when step 3 ran.

`captured` and `pred` are stored and never change, so a repin keeps the uid and the derivation stays verifiable after a
repin and after a file-node re-key. Anchor uids are therefore unique per (s, f) (I-F3), and identical captures on two
lanes derive one uid and merge add-wins ([40 §5.5]).

### 11.5 Verification

`doctor --verify` and image import recompute every file uid from (`root`, `origin_path`, `origin_pred`), every root-node
uid from `root`, and every anchor uid from (source uid, `captured`, `pred`) (I-F2). They cannot recompute `captured`
without the captured text; `captured` is trusted as stored ([40 §2.7]).

## 12. Constants

| name | value | § | source |
|---|---|---|---|
| `NONE32` | `0xFFFFFFFF` | 3.1 | this chapter |
| row size of `NodeHdr` | 60 bytes | 3.1 | [AR §3.1] |
| counter saturation | 65,535 | 3.4 | this chapter |
| title | 1–200 bytes, one line | 7.1 | [AR §2.6] |
| body | ≤ 65,536 bytes | 7.2 | [AR §2.6], [F17] OP-17-19 |
| `text` value | ≤ 65,536 bytes | 5.3 | this chapter |
| `sym` value | 1–4,096 bytes | 5.3 | this chapter |
| names (kind, field, value, edge, root) | 1–64 bytes | 8.2, 5.4.1 | this chapter |
| reading template | 1–200 bytes, ASCII | 8.5.4 | this chapter |
| scope segments | 1–64 | 10.3.1 | this chapter |
| `parent` depth (`max_depth`) | 12 | 9.6 | I4 |
| core kind ids / project kind ids | 1–13 / 64–254 | 3.3 | [AR §3.1] |
| core edge ids / project edge ids | 1–25 / 64–254 | 9.6, 8.3 | this chapter |
| schema version | 1 | 8.1 | this chapter |

None of these is a store parameter or a configuration key.

## Coverage

| Item | Part covered here | Section |
|---|---|---|
| [60 §2.5] "Schema as data": kinds, fields (type, lattice, class), edges (class, policy, acyclicity, cardinality) | complete: item records, enumerations and the core schema of version 1 | §8, §9 |
| [60 §2.5] "Ops and values": value encodings for the closed type set | the stored encoding of every value type, R-1's included; the ops that carry them are [F06]'s, the canonical encodings [F07]'s | §5 |
| [60 §2.5] "Segments": `NODE` `[NodeHdr; n]` | the `NodeHdr` row layout; the section is [F09]'s | §3 |
| [60 §2.5] audit row "Schema": the `lane` kind without a versioned dirty count | complete | §9.3 (lane) |
| [60 §2.5] audit row "Commit body": `actor u32` (also `CREATOR`) | the `CREATOR` value (`actor` u32, `role` u16); the column's section is [F09]'s, the commit header [F06]'s | §4 |
| [40] R-1 | the stored layouts of `path`, `oid`, `pathmove` and their rules; canonical encoding by root name is [F07]'s, the image [F14]'s | §5.2, §5.4.1–§5.4.2 |
| [40] R-2 | complete: the `artifact` field set with `origin_path`, `origin_pred`, `observed_blob`; statuses `planned`, `removed`; merge classes `observation` and `identity`; `area` fields `root` and `path_moves` | §8.4.1, §9.3, §9.5 |
| [40] R-3 | the `uid_derivation` column; the length-prefixed derivations of [40 §2.3] and [40 §2.7] with the widened `captured` and the predecessor term; the predecessor order by greatest uid on the view; the view-scoped dead-uid rule. The merge re-key procedure is [RULES/link-merge-rules]' and [F12]'s | §8.4.7, §11 |
| [40] R-4 | edge kind `at`, the 128-bit discriminator in the edge key, the anchor record layout with `captured` and `pred`; `SetEdgeProps` and the `aN` in `AddEdge` are [F06]'s; `ANCHORS` is [F09]'s | §9.6, §10.1, §10.3 |
| [40] R-5 | complete: the root node's derivation, fields, creation; no op, item, trailer or annotation | §11.3 |
| [40] R-12 | the data-model side of I-F2 (derivations, verification), I-F3 (uniqueness by construction), I-F8 (path values, root equality) and I-F9 (selector presence); the invariant texts are [F18]'s, enforcement [F13]'s | §5.4.1, §8.6, §10.3, §11 |
| [40] R-17 | the storage of `relink` (a `sym` observation member); the vocabulary is [F18]'s | §9.3 (artifact) |
| [50] F1 | complete: edge rows `lq_name`, `src_kinds`, `dst_kinds`, `symmetric`, `reverse_names`, `reading`, with the core values | §8.4.8, §8.5.4, §9.6 |
| [50] F2 | complete: field rows `optional`, `default`, `index`, `sort_rank` (enumeration values), `coerce` | §8.4.3, §8.4.4, §8.5.2, §8.5.3 |
| [50] F3 | the `QUERIES` item record; the op is [F06]'s, the canonical form [F07]'s, the image file [F14]'s | §8.5.5 |
| [50] F4 | the `CREATOR` value; the column is [F09]'s | §4 |
| [50] F5 | the `index` column and the default promotions (`labels`, `assignee`, `work_kind`, `phase_state`, `severity`, `f_kind`, `round`, `local_id`, `outcome`, `metric`); the `FCOL`/`FIDX` sections are [F09]'s | §8.4.3, §9.2, §9.3 |
| [80] X-F7 | the stored `path` values follow P1, P3, P4, P7 and P12 through [OS/path]; the rules themselves are [OS/path]'s, `fold_v1` [F20]'s | §5.4.1, §11.2 |
| [90 §10.1] | none of its items is a data-model structure | — |

## Holes

None. No value of this chapter is decided by an M0 measurement or benchmark. Values this chapter stores whose size
depends on a hole are owned elsewhere: the window value's length (HOLE(F20-window-lines)) and the quote and context
lengths ([F20 §7]); whether `DOCLEN` exists ([F09]).

## Open points for the review

1. **`NodeHdr` as rows** (conflict inside [AR]). [AR §3.1] says the header is "stored as structure-of-arrays columns",
   [AR §4.4] says `NODE` is `[NodeHdr; n]`, 60 B fixed. This chapter specifies the 60-byte row and reads §3.1 as the
   hot/cold split (the header's columns are one fixed row; cold columns are separate arrays). [F09] (WP-13) confirms
   the section form; the row's bytes do not change either way.
2. **Field order.** The offset table keeps [AR §3.1]'s field order, which leaves `rev_seq` and `last_op_lsn` unaligned.
   With a 60-byte stride no ordering keeps 8-byte fields aligned in every row, readers never assume alignment
   ([F01 §4.3]), and the design's order is the least surprising.
3. **Integer codes** of kinds, statuses, resolutions and every core enumeration (§9) are assigned here
   ([RULES/status-machines] open point 2): core kinds 1–13 in [AR §3.2]'s order, statuses in lattice order with side
   states after, other values in the design's listing order. `kind` = 0 marks an unused row, so an all-zero row is valid.
4. **Tombstone rows** (§3.5): which columns a `Delete` keeps. An artifact's tombstone stores its last path as title, so
   `MATCH (x:DELETED)` can render it.
5. **Offsets and "none"**: `NONE32` for `title_off` and `fields_off`, and a 1-based `body_ref`, so no valid offset or
   index doubles as "none".
6. **Saturating counters** (§3.4). [AR §3.1] sizes the derived counters at u16 without saying what happens past 65,535.
   Saturation with a recount keeps every zero test exact, refuses no write, and needs no violation class a merge would
   lack. Alternative: refuse such writes, which would also need a merge-time class.
7. **Store-local schema ids** ([RULES/merge-table] open point 16). Project kind ids, edge ids and enumeration integers
   are allocated store-wide like symbols, never hashed or exported, and kept in a store-wide map rebuilt from the log.
   Consequences: [F06] carries the id in the op that first lands an item (unhashed, like `aN`); [F09]/[F11] (WP-13) add
   the map's section (proposed `SCHEMAIDS`); [F07] names every such id by its name. Alternative considered: ids in the
   hashed item, which would turn two lanes' independent additions into a false `SchemaConflict` and make ids differ
   between stores.
8. **Schema version.** Canonical item 7 hashes "the schema version" ([AR §4.6]) but the commit header of [AR §4.3] has
   no such field. This chapter defines schema version 1 as the core schema of §9, constant for every format-v1 commit;
   project extensions are schema items, which item 10 hashes anyway. A later core schema would need a header field or a
   format version (WP-12 records this in [F06]/[F07]).
9. **Core items are fixed** (§8.1): items may add fields and enumeration values to core kinds, never change or retire
   core items; strengthening applies to project items only.
10. **Mapping [AR §3.2]'s types to the closed type set** ([RULES/merge-table] open point 11). `sym` is interned `text`;
    `u16`, `u32`, `u64` fields are `int` with a range; `sha + algo` is `oid`; `list<text>` and `list<{…}>` are record
    lists (§5.4.5), merged as scalars; the structs `applies_to` and `env` become a tagged set (§5.4.6) and four
    `env_*` fields. The record list is a TAB/LF form rather than JSON so that the reference model, which has no JSON
    code ([PLAN §3.2] item 9), can validate it. Consequences for [RULES/merge-table]: FC-058–FC-061 and FC-066–FC-069
    become one `applies_to` row of class `glob-set` (composition only for `path:` elements); FC-104 becomes four scalar
    rows.
11. **Time fields and `coerce = timestamp`.** Domain times (`defer_until`, `due`, `since`, `review_after`, `started`,
    `ended`) are Unix seconds; the cold columns are u32 ([AR §3.1]), so they end in 2106. [50] F2 lists three `coerce`
    values; the fourth, `timestamp`, lets the binder apply [50 §3.2]'s ISO 8601 coercion to these fields. It extends a
    [50] reservation additively; WP-19 confirms.
12. **One stored form per value** (§6.2): empty values and defaults are absent; `bool` carries its value in the type
    byte's bit 7, which is how "bool carries no value bytes" ([AR §3.1]) still distinguishes true from false.
    **Pass 1 (P1-1, S1-1, A1-1): closed.** §5.1 is the one registry for every stored structure; [F06]'s tag table is
    removed. The decisions: type id 0 is `absent` where an op or a conflict side admits it; `ref` is `u32` ([AR §3.1]
    "node refs are u32"); `commitref` and `pinned_commit` are 32 bytes; empty text, empty set and empty `oid` are absent in
    every position; NaN, ±infinity and −0.0 are invalid patterns; `pathmove` classes are 1–4 with 0 invalid; the set order
    is §5.5's. The edge property block (§10.2) is one layout with `pflags` bit 0 `has_pin`, bit 1 `flagged`, bit 2
    `anchor` (S1-2; A1-3's alternative, [F06]'s old bit order, is not taken because this chapter owns the block).
13. **`f64`** ([F01] open point 8): NaN and infinities refused, −0.0 stored as +0.0.
14. **Text normalisation and limits** (§5.3). The CR → LF store rule of bodies is extended to every text value; U+0000
    is refused outside bodies; `text` values ≤ 64 KiB and `sym` values ≤ 4,096 B are this chapter's caps (the design
    fixes only the title and body caps).
15. **`order`** is a common field (the `.moi` header has `order` for every node, [AR §5b.2] rule 2) merged with `parent`
    as the hierarchy key; keys never end in `0`; the generation algorithm is [API]'s.
16. **Common fields `abstract`, `labels`, `order`, `reason`.** [AR §2.6] and [50 §2.5] give every node an abstract and
    labels; `reason` carries the reason of `retract`, `file rm` and `links fix --drop` ([AR §3.6] "retracted with a
    reason"; [RULES/status-machines] open point 9) and replaces the artifact-only `reason` of [40 §2.2] (same class,
    scalar). `reopen --reason` stays in the commit message.
17. **Question `answered`** ([RULES/merge-table] open point 4; [RULES/status-machines] open point 20): stored and moved
    by guarded writes, while `done` follows the derived edge predicate, as [RULES/status-machines] ST-024 proposes.
18. **`done` on other kinds** ([RULES/status-machines] open point 21): only task, question and verdict have it (`has_done`).
19. **Finding lattice** (conflict inside [AR]; [RULES/merge-table] open point 2). [AR §3.2] writes
    `open < (confirmed | refuted) < (fixed | deferred | withdrawn)`; [AR §3.6] allows only `confirmed → fixed`. §9.5
    follows §3.6 and [RULES/merge-table] SL-027–SL-032, so a refuted-versus-fixed pair is a `StatusFork`. [AR §3.2]'s
    notation should be corrected at WP-81a.
20. **`phase_state` values** ([RULES/status-machines] open point 19; [RULES/merge-table] open point 28): the 14 states of
    [C §3.2] (proposed … documented) plus the side values `blocked`, `frozen`, `deferred` that [AR §3.2] names; [01]'s
    `NeedsOwner` and `MeasurementQueued` are not included (a project may add them as a weakening). Merged as a scalar.
21. **Flags `archived`, `frozen`, `pinned`** ([RULES/status-machines] open point 24): source-truth facts independent of
    the statuses of the same names; their read-side meaning is [LQ/std]'s and [RULES/pack-classes]'. The design gives no
    definition; the review may instead make each flag mirror its status where the kind has one.
22. **`abs` fields** ([RULES/merge-table] open point 32): `run.script_path`, `run.journal_path`, `lane.worktree_path` and
    `lane.target_dir` hold `path` values of root `abs`, not refs to artifact nodes; [40 §2.8]'s "become `abs`
    artifacts" is read as "use root `abs`". An `abs` file node remains available for run outputs.
23. **`finding.where`** ([RULES/merge-table] open point 31): not a field; `file:symbol@sha` is an `at` edge with a
    `symbol` anchor, and a section reference is an `about` edge.
24. **`root` fields** ([RULES/merge-table] open point 30): an artifact's `root` keeps [40 §2.2]'s class `scalar` but is
    immutable after `Create` (§8.6); a root node's `root` is `identity`.
25. **Root nodes are never deleted** ([RULES/link-merge-rules] open point 4): refusing `rm` closes LM-013's gap, since a
    root-key uid has no predecessor to re-key with.
26. **Glob syntax** ([RULES/link-merge-rules] open point 6) is §5.4.3, with the literal-prefix rule; `\` is not an escape
    because paths cannot contain it (P4).
27. **Symmetric kinds** ([RULES/merge-table] open point 25): stored with the smaller uid as source.
28. **Endpoint kinds widened** (conflict: [50 §2.5] and [AR §3.3] against [AR §7.1], [AR §7.3], [AR §7.4] and
    [50 §4.3]). `ABOUT` sources gain `rule`, `decision`, `note` ([AR §7.1] `rule|note|decision --about`;
    [RULES/pack-classes] open point 2); `DERIVED_FROM` sources gain `artifact` ([AR §7.3] doc-writer;
    [RULES/role-write-policy] open point 12). The precedence rule would give [50]'s F1 values, but [50 §4.3] itself
    needs the wider `ABOUT`, and both widenings only admit writes the design describes; the review confirms or reverts.
29. **`SUPERSEDES` endpoints** are knowledge only ([50 §2.5], [AR §3.6]); a verdict becomes `superseded` by a status
    write ([RULES/status-machines] open point 23). **`topo`** validity replaces equality in I9 (§3.4), because
    Pearce–Kelly's order is one valid order among many.
30. **`suspect`** ([RULES/delete-policy-matrix] open point 10): [AR §2.5] speaks of a transitive closure, [AR §3.5]
    defines a one-hop predicate. This chapter stores the flag; the definition is [F13]'s (F15), and the proposal is
    [AR §3.5]'s one-hop definition, reading "closure" as the set of direct referrers of every changed target.
31. **`consumed` carries `pinned_commit`** ([AR §7.4] step 5, `--record-run`), although [AR §3.3] and [50 §2.5] list
    the property only for `cites`, `implements` and `derived_from`.
32. **Added enumeration values**: `f_kind = deviation` ([AR §7.3] "deviation findings"; [RULES/role-write-policy] open
    point 3); `artifact_kind`'s run-output kinds, which [AR §3.2] leaves unlisted, are [C §3.2]'s plus `research` and
    `impl` ([AR §7.3]; [RULES/role-write-policy] open point 4).
33. **`implements` on a deleted target** ([RULES/delete-policy-matrix] open point 7): `tombstone-src-suspect`, following
    [AR §3.5]'s `suspect` definition rather than [AR §3.3]'s "if the target is superseded".
34. **Predecessor order** (conflict: [40] against [AR] and [60]). [40 §2.3] and [40] R-3 (revised by the A1
    dispositions, review S-01) order predecessors by the greatest candidate uid on the registering view; [AR §5e.2],
    [AR §5b.5] rule 7, [AR §4.6]'s reservation table and [60 §2.5]'s R-3 row still say "(generation, commit id)". [40] is
    authoritative for R-3; §11.2 follows it, and the [AR]/[60] texts are stale (WP-81a). The dual-creation `created`
    rule ("least (generation, commit id)") is provenance only and is not specified here.
35. **Re-key edge scope** (conflict inside [40]). [40 §0.1] item 4 and R-3 say the re-key moves "every edge that side
    added"; [40 §5.5]'s row and [RULES/link-merge-rules] RK-006 re-point only anchors. §11.2 gives only the derivation;
    WP-12 ([F12]) and R-MODEL align the procedure with R-3's wording. **Closed** (pass 1, S1-14): [F12 §7.6] re-points
    every edge the re-keyed side added, every kind, and [RULES/link-merge-rules] RK-006 (`repoint-added-referrers`) states
    the same scope, with RK-011 for the residue ([F12] open point 25).
36. **Anchor de-duplication and loops** (§11.4): "equal current selectors" is read as equality of the `captured`
    inputs other than the file uid; the predecessor loops are bounded; an all-zero derived uid is refused.
37. **Anchor record ownership.** [F20] cites [F08] for the anchor record; review a1-S (S-03) names chapter 18. This
    chapter holds the stored record and the derivation; chapter 18 (R-12, R-16, R-17) and [F07] (R-10) cite it.
    **Pass 1 (P1-1, S1-3, A1-2): closed.** §10.3 is the only layout; [F06 §7.5.3]'s second layout is removed and the ops
    carry this record. From [F06]'s former layout this record takes one change: `blob` may be empty for a planned target
    (§10.3 order 23). Codes stay 1-based, the uid stays in the record (the op's discriminator must equal it, [F06 §7.5.1]),
    presence follows `kind` and `aflags`, texts stay `vbytes`, and `text_unavailable` marks held digests.
38. **Name grammars** (§8.2, §5.4.1), including root names, are this chapter's; [CFG] must accept exactly the root-name
    grammar for `roots.<name>`.
39. **The scope value** (§10.3.1) fixes bytes that enter `captured`; what the scanners report is [F20] open point 30.
    **Pass 1 (P1-20, S1-4, A1-14):** the scanner grammar becomes a normative [F20] appendix before the freeze; until it
    exists no scope is recorded (§10.3.1), so an engine and the model cannot derive different uids from an unspecified
    scanner. **Round 1** (closure open point 2): the interim rule also refuses the `symbol` and `heading` forms, whose
    header line, quote, hint and span hash a scanner decides ([F20 §6.1]); the owner question is OQ-R-2.
40. **`text-unavailable` anchors** store the four digests in place of the texts (§10.3), so a hash-only import keeps every
    canonical input.
41. **Tombstone-reference `#N`s** (§2.1): an unknown uid referenced by an import gets a `#N` with no node; [F11] states
    the `ALLOC`/`UIDX` row for it. **The random source** of random uids (§2.2) — also needed by the store id and the
    `tmp/` nonces of [F02 §4, §5.3] — has no call in the OS-layer files written so far; WP-17 names one. Resolved
    (pass-1 finding S1-27): `Entropy::fill_random` ([OS/README §4.6]).
    **Pass 1 (P1-14):** the same seam; its simulator derives the draws from the stream seed ([OS/README §4.6],
    [API §6.4], [API §17.3], [API §17.4]).
42. **Resolution column rule** (§3.1) follows [RULES/status-machines] GR-014; **owner quote** is required only on rules,
    as [AR §3.2] states; **measurement mandatory fields** stay a role-policy rule.
43. **`SUBTASK_OF`** is a forward synonym, which F1's `reverse_names` cannot hold; WP-19 puts it in LQ's alias table.
    **Pass 1 (A1-56): closed.** [LQ/canonical-ast §5.4] fixes the synonym table, which holds it.
44. **Where schema items are stored.** [AR §4.4] lists no schema section; [F09] (WP-13) adds one for the view's items and
    the `SCHEMAIDS` map (open point 7).
45. **For WP-12.** [F06] should encode op values with §5's encodings and carry the store-local ids of §8.3; [F07] should
    render symbol ids, `#N`, enumeration integers, kind and edge ids, `path` roots and `commitref` values by name, uid or
    full commit id, and fix the canonical order of sets.
46. **The `mentions` sigil** (§10.4) uses ASCII letters and digits for "alphanumeric" and resolves `#N` store-wide.
47. **[PLAN §3.3]** assigns WP-14 the R-12, R-15, R-16 and R-17 gap; chapter 18 closes it. This chapter closes the
    review items S-01 and S-03 (derivations), S-19 (the root invariant, §8.6) and S-20 (`pathmove.hlc`, §5.2) on its
    side.
48. **Pass 1, round 1** (P1-1; A1-14, S1-4, P1-20). §5.1 names the one derived form of a value that is not a stored
    value: [F09 §10.1]'s promoted `commitref` element is an `id16` index key, and the 32-byte value stays in the field
    block, which a reader reads and confirms against (the 16-byte index column review pass 1, S1-1, allows). §5.6 gives
    one value of every type in its stored and canonical forms, the values of the cross-chapter fixture of
    `COVERAGE.md` row 60-AR-Values. §10.3.1's interim rule also covers the `symbol` and `heading` forms, as
    [F20 §6.1] states it.
49. **Pass 1, round 2** (closure NC-5). §5.6's `pathmove` value now holds directory prefixes (`a/`, `b/`), as §5.2 and
    §5.4.2 require of `from` and `to`; its stored bytes and its `cv` were re-derived from §5.2 and [F07 §7.1], and every
    other row of §5.6 was re-derived and is unchanged.
