# 07 — Canonical form

| | |
|---|---|
| Title | Canonical form of a commit: the byte encoding hashed into `commit_id` (items 1–9 and item 10 through `changeset_digest`); the canonical state of a view and the net changeset as a state diff; key classes and their order (the `.moi` line order inside a uid); canonical value encodings with names in place of every store-local number; the R-10 anchor selector block; schema items with named queries (F1–F3); the commit-kind names including import-checkpoint; message normalisation; commit-id derivation per commit kind; the gate-0 carrier table (stub for [F14]) |
| Chapter | [F07], `docs/spec/format/07-canonical-form.md` |
| Status | draft, pass 1 pending |
| Work package | WP-12b, the canonical-form part of WP-12 ([PLAN §3.2] item 1), author role R-SPEC-F |
| Sources | [AR §4.6] (items 1–10; "Not hashed"; "Net changeset = state diff"; the reservation tables for R4, R5, the audits, the ports and [90 §10.1]); [AR §4.3] (the `Commit` body fields `kind`, `import`, `foreign_git`, `changeset_digest`, `cs_ref`; per-key coalescing; bulk commits); [AR §4.5] step 4 (the canonical form and `changeset_digest` computed in phase 1), step 7 (re-parent over the unchanged `changeset_digest`), step 8 (`#N` allocation after the candidate); [AR §5a.1] (commit identity), [AR §5a.3] (the `sync` residue and "window ⊕ residue"), [AR §5a.5] (revert, cherry-pick), [AR §5a.7] steps 1 and 7, [AR §5a.8] (conflict classes); [AR §5b.1] (tree layout and fan-out), [AR §5b.2] rules 1–9 (line order, values, tombstones, R4 and R5 forms), [AR §5b.3] (the marker), [AR §5b.4] (commit mapping, the canonical field → carrier table, trailer order, foreign and import-checkpoint ids), [AR §5b.5] rules 7 and 8, [AR §5b.6] steps 2–5 (native, foreign and checkpoint import, demotion), [AR §5b.7] (gates 0–3); [AR §2.15] T15 (N4, N5, N6, N12, N13a, N13c, CB1, CM2); [40 §2.2] (the observation composite, `relink`), [40 §2.3], [40 §2.4] (`pathmove`), [40 §2.7] (the anchor record, `captured`, `pred`), [40 §2.11] R-1, R-2, R-4, R-5, R-10, R-11 (authoritative), [40 §5.5], [40 §5.7] (anchor lines, anchor text, hash-only destinations); [50 §4.4] (portable form; a definition is one atomic value), [50 §8.1] F1, F2, F3, F10, F14, F16, F17, F18; [60 §2.5] rows "Canonical form" and "Gate-0 carrier table", the audit row "Canonical form" and the R-10 row; [80 §3.1] X-F7, X-F9, [80 §3.2] row "Canonical commit form"; [90 §10.1] row "Commit header"; [PLAN §3.2] WP-12, WP-21; [PLAN §3.3] (the WP-12 gaps); reviews `docs/spec/reviews/a1-S.md` S-02, S-03, S-04, S-19, S-20 and `a1-A.md` A-M2 |
| Depends on | [F01], [F06], [F08]; cites [F02], [F05], [F09], [F10], [F11], [F12], [F13], [F14], [F16], [F18], [F19], [F20], [OS/path], [OS/clock], [API], [RULES/merge-table], [RULES/link-merge-rules] |

## 1. Scope

This chapter fixes the bytes that `commit_id` and `changeset_digest` hash. Nothing in it is stored: the canonical form is
an input to BLAKE3-256 and never a file, a record or a column. It owns:

- the commit-id input C: items 1–9 and `changeset_digest` (§3);
- the canonical names of the commit kinds, import-checkpoint included (§4);
- message normalisation, at write time and at import (§5);
- the canonical state of a view: its keys, key classes and values, the tombstone state and the treatment of conflict
  values (§6);
- the canonical encoding of every value of the closed type set and of every key class, with store-local numbers replaced
  by names, uids and full commit ids (§7);
- the edge value and the R-10 anchor selector block (§8);
- the canonical encoding of schema items, named queries included (§9);
- item 10: the entries, their order, and `changeset_digest` (§10);
- the byte-level list of what is not hashed (§11);
- the derivation of `commit_id` for every commit kind: ordinary, merge, sync, revert, cherry-pick, foreign and
  import-checkpoint (§12);
- the relation between the stored ops of [F06] and the entries of item 10 (§13);
- the gate-0 carrier table as a stub that [F14] completes (§14).

It does not own: the stored commit record ([F06]); the value, field, kind, edge and schema-item vocabularies and their
stored encodings ([F08]); the conflict- and violation-class enumerations ([F12], [F19 §12]); the `.moi` grammar, the
trailer grammar and the image rules ([F14]); the write protocol and the moment the canonical form is computed ([F16]); the
merge semantics ([RULES/merge-table], [RULES/link-merge-rules]).

## 2. Notation and building blocks

### 2.1 Types and framing

Every type is [F01]'s. The canonical form uses only these encodings:

| Encoding | Bytes | Use |
|---|---|---|
| `u8`, `u16`, `u32`, `u64`, `i64` | fixed width, little-endian ([F01 §5.1]) | counts (`u32`), the entry count (`u64`), `hlc`, integers of values |
| `bool8` | `00` false, `01` true ([F01 §5.4]) | flags of schema items |
| `lp(x)` | `u32` length, then the bytes of x ([F01 §6.3]) | every variable-length or optional component |
| `b16` | 16 bytes, no length | uids, BLAKE3-128 digests, `captured` |
| `b32` | 32 bytes, no length | full commit ids, `changeset_digest` |

Rules:

- **No varint.** The canonical form never uses [F01 §5.2]'s varints. Integers are fixed width; lengths are `lp()`'s
  `u32`; counts are `u32` except the entry count of §10.4, which is `u64`.
- **Optional components** are `lp(x)` with x empty when the component is absent. Where an absent component and a present
  empty one would both be the empty string, this chapter says which one is meant; for every such component below the
  present form is never empty.
- **Self-delimiting.** Every component is fixed width or `lp()`-framed, so every encoding below is prefix-free and the
  concatenation of encodings is decodable in one way. No component is framed twice unless a table says so.
- **Text** enters as its exact UTF-8 bytes ([F01 §6.1]). No normalisation is applied here except message normalisation
  (§5), which happens before the message is stored.

### 2.2 Names

Every enumeration value enters the canonical form as `lp(name)`, the frozen name of the value in its owning table, never
as its integer ([F01] open point 9). This covers:

| Enumeration | Names (owner) |
|---|---|
| commit kind | §4 ([F06 §3.1]) |
| kinds, statuses, resolutions, header and field enumerations | [F08 §9.1], [F08 §9.4], [F08 §9.5], and the names of project items ([F08 §8.5]) |
| edge kinds | [F08 §9.6] and project edge items |
| schema-row enumerations (merge class, storage, `index`, `coerce`, existence policy, edge class, `on_dst`, `on_src`, acyclicity, cardinality, `props`, `uid_derivation`, value types) | [F08 §5.1], [F08 §8.4] |
| `pathmove` class | `explicit`, `confirmed`, `committed`, `observed` ([40 §2.4]) |
| anchor `kind`, `mode`, `watch` | `file`, `heading`, `symbol`, `quote`, `range`, `lines`; `live`, `pinned`; `header`, `span` ([40 §2.7]) |
| git object format | `sha1`, `sha256` ([F01 §7.5]); the empty string for `none` |
| conflict class | `FieldEdit`, `StatusFork`, `TextHunk`, `DeleteVsModify`, `SupersedeFork`, `OwnerFieldEdited`, `PathClaim` ([F12 §6.1]; [AR §5a.8]'s `DATA` has no code and never enters, pass 1, P1-31, S1-32, A1-43) |
| provisional side of an existence conflict (`prov`) | `ours`, `theirs` ([F12 §6.3]) |

Names are case-sensitive bytes. A name that its owning table spells with a hyphen (`cherry-pick`, `delete-wins`,
`max-1-per-src`) enters with the hyphen.

### 2.3 Store-local numbers never enter

No `#N`, symbol id, lsn, `seq`, `ref_id`, anchor handle `aN`, project kind id, project edge id, project enumeration
integer, `path` root symbol, destination number or store id enters the canonical form ([AR §4.6] "Not hashed",
[AR §5b.5] rule 7, N4). Each is replaced:

| Store-local datum | Enters as |
|---|---|
| `#N` of a node (a `ref` value, `parent`, `replaced_by`, an edge endpoint, `origin_pred`) | the node's uid, `b16` ([F08 §2.1]: `ALLOC`/`UIDX` map every allocated `#N` to its uid) |
| a symbol id (actor, role, session, git branch and worktree, reason, field and kind names, interned text) | the symbol's string ([F01 §8]) |
| an enumeration integer, a kind id, an edge id | its name (§2.2) |
| the root symbol of a `path` | the root's name ([40] R-1 as revised by S-19) |
| an `id16` or a store-held commit prefix | the full 32-byte commit id |

A writer that cannot map a store-local number to its replacement (an `#N` without an `ALLOC` row, a symbol id without a
string) has a corrupt store: it refuses the write with exit 7 ([F19] `store_corrupt`) and hashes nothing.

### 2.4 Sorted lists inside values

Where a value holds a list whose order carries no meaning (the elements of a set, a list of names), the elements are
encoded one by one and written in the **bytewise order of their encodings** ([F01 §6.6]), with no two equal encodings.
This one rule applies to every such list in this chapter. (The order of entries in item 10 is a different, named order:
§10.3.)

### 2.5 Hash functions

BLAKE3-256 and BLAKE3-128 are [F01 §7.1]'s. The two digests of this chapter use fixed domain strings:

| Digest | Domain prefix |
|---|---|
| `commit_id` | `lp("moirai-commit-v1")` |
| `changeset_digest` | `lp("moirai-changeset-v1")` |

## 3. The commit id

### 3.1 Definition

```
commit_id = BLAKE3-256(C)
```

C is the concatenation, with no padding, of:

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `domain` | `lp("moirai-commit-v1")` | always | domain separation (§2.5) |
| 2 | `kind` | `lp(name)` | always | item 1: the commit kind (§4) |
| 3 | `n_parents` | `u32` | always | item 2: the number of parents, 0, 1 or 2 |
| 4 | `parents` | `n_parents` × `b32` | always | item 2: the parents' **stated** ids, in order (§3.3) |
| 5 | `hlc` | `u64` | always | item 3 |
| 6 | `actor` | `lp(text)` | always | item 4; empty allowed |
| 7 | `role` | `lp(text)` | always | item 4; empty allowed |
| 8 | `session` | `lp(text)` | always | item 4; empty allowed |
| 9 | `git_algo` | `lp(name)` | always | item 5: `sha1`, `sha256`, or empty (§3.6) |
| 10 | `git_head` | `lp(digest)` | always | item 5: 20 or 32 bytes, or empty |
| 11 | `git_branch` | `lp(text)` | always | item 5; empty allowed |
| 12 | `git_worktree` | `lp(text)` | always | item 5; empty allowed |
| 13 | `git_base` | `lp(digest)` | always | item 5: 20 or 32 bytes, or empty |
| 14 | `message` | `lp(text)` | always | item 6: the normalised message (§5); empty allowed |
| 15 | `schema_version` | `u32` | always | item 7 |
| 16 | `origin` | `lp(b32)` | always | item 8: the full id of the reverted or cherry-picked commit; empty otherwise |
| 17 | `foreign_algo` | `lp(name)` | always | item 9: `sha1` or `sha256`; empty when item 9 is empty |
| 18 | `foreign_oid` | `lp(digest)` | always | item 9: 20 or 32 bytes; empty when item 9 is empty |
| 19 | `changeset_digest` | `b32` | always | item 10, through its digest (§10.4) |

Every item of [AR §4.6] is in C, in its order, and nothing else is ([AR §4.6] "exactly these fields, in this order").

### 3.2 Item 1: kind

`lp` of the commit kind's canonical name (§4). The stored `kind` byte of [F06 §4.3] order 14 maps to it through §4's table.

### 3.3 Item 2: the parents' stated ids

- `n_parents` is 0 for a root commit (the store's first commit, a foreign root commit, the first import-checkpoint of a
  ref), 1 for `ordinary`, `revert`, `cherry-pick` and a checkpoint with a previous checkpoint, 2 for `merge` and `sync`
  ([AR §4.6] item 2; [F06 §3.3]).
- The order is [AR §5b.4]'s: first the dst tip (`merge`) or the lane tip (`sync`), second the src tip (`merge`) or
  `sync_base` (`sync`).
- Each id is the full 32-byte **stated** id: [F06 §4.4.1]'s `stated_ids` entry where the record has one, otherwise the
  `commit_id` of the actual parent record. A stated id differs from the actual parent's id only below a demoted parent
  (§12.5).

### 3.4 Item 3: `hlc`

The commit header's `hlc` ([F06 §4.4.4]), as a `u64` ([F01 §5.7]). A re-parent under the writer byte assigns a new
`hlc` ([AR §4.5] step 7), so it changes `commit_id` but not `changeset_digest` (§3.12). A local commit's value comes from
the store's one HLC sequence, which only semantic durable records advance ([F16] P-36, [API §6.2] CK-4; pass 1, P1-5,
S1-13, A1-17), so the engine and the model compute the same item 3 and maintenance never changes it.

### 3.5 Item 4: actor, role, session

The strings of the header's `actor`, `role` and `session` symbols ([F06 §4.3] orders 17–19; classes `actor`, `role`,
`session`, [F01 §8.2]). Symbol id 0 is the empty string ([F01 §8.1] S2), so an unset field enters as `lp("")` =
`00 00 00 00`. Symbols are one line ([F08 §5.3]), so none of these strings holds an LF.

### 3.6 Item 5: git provenance

From [F06 §4.4.6]'s `git` group; every component is empty when the group is absent.

| Component | Value |
|---|---|
| `git_algo` | the name of `git.algo` when `git.head` or `git.base` is present; **empty otherwise**, even when the stored `algo` is non-zero (an unborn repository, open point 11) |
| `git_head` | the digest bytes of `git.head` (20 for `sha1`, 32 for `sha256`), or empty |
| `git_branch` | the string of the `git.branch` symbol: the short form of [F18 §3.2] rule 1, the bytes after `refs/heads/` of the ref the tree's `HEAD` names symbolically; empty when `HEAD` is detached, names a ref outside `refs/heads/`, or names a branch that is not valid UTF-8 (open point 11) |
| `git_worktree` | the string of the `git.worktree` symbol: the canonical root text `c.text` of the caller's tree ([OS/path §4.5], [80] X-F7) |
| `git_base` | the digest bytes of `git.base`, or empty |

Digests enter as raw bytes, never as hexadecimal text.

### 3.7 Item 6: message

`lp` of the stored message ([F06 §4.3] order 36), which is the output of §5's normalisation. An absent `msg` group is the
empty message, `lp("")`.

### 3.8 Item 7: schema version

The header's `schema_version` ([F06 §4.3] order 20) as a `u32`. In format v1 it is 1 for every commit ([F08 §8.1]):
the core schema of version 1 is fixed, and project schema items enter item 10 (§9).

### 3.9 Item 8: origin

`lp` of the 32-byte `origin` ([F06 §4.3] order 23) for `revert` and `cherry-pick` (including a native import of those
kinds), `lp("")` for every other kind.

### 3.10 Item 9: `foreign_git`

For a commit whose `foreign_git` group is present ([F06 §4.3] order 22): `lp` of the algorithm's name, then `lp` of the
digest bytes. Otherwise both are `lp("")`. Item 9 is present exactly for commits created by foreign or checkpoint import
and for their native re-imports ([F06 §3.3]).

### 3.11 Item 10: `changeset_digest`

The 32 bytes of `changeset_digest` ([F06 §4.3] order 38), which §10.4 defines. Item 10 enters C only through this digest
([AR §4.6], [60 §2.5] audit row "Canonical form").

### 3.12 Re-parent and bulk commits

- **Re-parent in O(1)** ([AR §4.5] step 7, [70 S2]). Item 10 depends on no `#N`, lsn, `seq`, `aN` or header field (§11),
  so the candidate's `changeset_digest` is final in phase 1. A re-parent under the writer byte recomputes C over the new
  parents (item 2), the new `hlc` (item 3) and the unchanged `changeset_digest`: a few hundred bytes (196 in §16's
  example), one BLAKE3 call. A `pathmove` value's `hlc` is part of item 10 and does not change on a re-parent (S-20, [F06 §5.5]).
- **Bulk commits** ([AR §4.3], [F06 §9]). `changeset_digest` is computed over the entries as they stream (§10.6), so a
  bulk commit and an inline commit of the same changeset have the same `commit_id` ([F06] BK-4).

## 4. Commit kinds

| Stored code ([F06 §3.1]) | Canonical name | Item 2 parents | Item 8 | Produced by |
|---|---|---|---|---|
| 0 | `ordinary` | 0 or 1 | empty | every local write that is not one of the kinds below; a foreign commit with 0 or 1 parents; `resolve` commits on a staging ref |
| 1 | `merge` | 2 | empty | `merge`, `merge --continue` ([AR §5a.7]); a foreign two-parent commit |
| 2 | `sync` | 2 | empty | `merge main --into <lane>` ([AR §5a.3]) |
| 3 | `revert` | 1 | the reverted commit | `revert` ([AR §5a.5]) |
| 4 | `cherry-pick` | 1 | the picked commit | `cherry-pick` ([AR §5a.5]) |
| 5 | `checkpoint` | 0 or 1 | empty | checkpoint-granularity import: the **import-checkpoint** kind (N13a, [AR §5b.4]) |

- The names are the values of the `Moirai-Kind` trailer ([AR §5b.4]; [F14]).
- The provenance byte `import` ([F06 §3.2]: `local`, `native`, `foreign`, `checkpoint`) is not hashed. A verified native
  import keeps the original kind and id ([AR §4.6] item 1).
- [AR §4.6] item 1 lists five kinds. The sixth, `checkpoint`, is the kind [AR §5b.4] gives an import-checkpoint commit "in
  the local record" and [F06 §3.1] stores as 5; it enters item 1 by that name (open point 16).

## 5. Message normalisation

### 5.1 At write time

A local commit's message m, as given by the caller (a `--message`, the MCP `message`, or a text [API] generates for a verb),
is normalised to N(m) before it is stored and hashed ([AR §4.6] item 6):

1. **UTF-8.** m must be valid UTF-8 ([F01 §6.1]) and must not contain U+0000 ([F08 §5.3]).
2. **Line ends.** Every CR LF pair becomes LF; then every remaining CR becomes LF.
3. **Trailing whitespace.** From the end of every line (a line is the text between two LFs, or before the first, or after
   the last), every trailing byte of the set {`09` HT, `0B` VT, `0C` FF, `20` SP} is removed. No other character counts as
   whitespace; in particular no non-ASCII space is removed.
4. **Trailing newlines.** Every trailing LF is removed.

Leading empty lines and runs of empty lines inside the message are kept. N is idempotent: N(N(m)) = N(m). The result is
`msg` ([F06 §4.3] order 36); an empty result is the absent `msg` group.

### 5.2 Refusals

The write is refused with exit 2 ([F19] `bad_value`, open point 15) and nothing is written when:

- m breaks step 1;
- N(m) is longer than 65,535 bytes (the bound of `msg`, [F06 §4.3] order 36);
- the **last paragraph** of N(m) begins with the seven bytes `Moirai-` (`4D 6F 69 72 61 69 2D`), compared case-sensitively
  ([AR §4.6] item 6). A paragraph is a maximal run of non-empty lines; the last paragraph is the one after the last empty
  line, or the whole of N(m) when it has no empty line; it "begins with" those bytes when its first line does.

The third rule keeps the image's trailer block separable ([AR §5b.4]); it applies only to local writes (§5.3).

### 5.3 At import

The message of a foreign commit, of an import-checkpoint commit, and of a demoted commit (§12) comes from a git commit
object and is normalised by N_imp, which never refuses:

1. **Decoding.** The bytes are decoded as UTF-8, and every ill-formed subsequence is replaced by U+FFFD by the "maximal
   subpart" practice of Unicode §3.9 (the practice of Rust's `String::from_utf8_lossy`). Every U+0000 then becomes U+FFFD.
   A git `encoding` header is ignored.
2. Steps 2–4 of §5.1.
3. **Length.** If the result is longer than 65,535 bytes, it is cut at the last scalar-value boundary at or before 65,535
   bytes, and steps 3 and 4 of §5.1 are applied once more.

The `Moirai-` rule of §5.2 does not apply: an imported message may end in a paragraph that begins with `Moirai-` (a demoted
commit's message keeps its old trailer block, §12.5). [F14] therefore separates the trailer block by the **final**
paragraph only ([AR §5b.4]).

A **native** import (§12.2) applies N — steps 1–4 of §5.1 — (not N_imp) to the message part [F14] extracts; only step 1
can fail, and a message part that breaks it cannot verify and is demoted (§12.5). None of §5.2's refusals applies there: a
message part whose last paragraph begins with `Moirai-` verifies as it stands, since a demoted commit that is re-exported
natively keeps its old trailer paragraph in its message (§12.5), and refusing it would leave that commit unverifiable for
good (spec sync 2b).

### 5.4 Examples (informative)

Bytes are written as C-style escapes.

| Input m | N(m) | Note |
|---|---|---|
| `fix lock\r\n\r\nsee #12  \r\n\r\n\r\n` | `fix lock\n\nsee #12` | CR LF, trailing spaces and trailing newlines removed |
| `a\rb\t\n` | `a\nb` | lone CR becomes LF; trailing HT removed |
| `\n\nsubject` | `\n\nsubject` | leading empty lines are kept |
| ` \t\n` | (empty) | the `msg` group is absent |
| `done\n\nMoirai-Ref: main` | refused | last paragraph begins with `Moirai-` |
| `done\n\nnote\nMoirai-Ref: main` | `done\n\nnote\nMoirai-Ref: main` | the last paragraph begins with `note` |

## 6. The canonical state of a view

Item 10 is the net changeset, which equals the typed diff between the state at the first parent and the state at the
commit, for every commit kind ([AR §4.6] "Net changeset = state diff"). This section defines that state. §10 turns two
states into entries.

### 6.1 Keys and key classes

The **canonical state** CS(V) of a view V is a finite map from keys to non-absent values. A key that the map does not
hold has the value `absent`. Keys belong to eight **node key classes**, owned by a uid, and to the schema:

| Code | Class | Key | Holds ([AR §4.6] item 10) | `.moi` lines ([AR §5b.2]) |
|---|---|---|---|---|
| 1 | existence | (uid) | `live(kind)` or `deleted(kind, reason, replaced_by)` | `kind:` (and the tombstone form: `deleted:`, `field reason:`, `field replaced_by:`) |
| 2 | status | (uid) | status and resolution, one key | `status:`, `resolution:` |
| 3 | hierarchy | (uid) | (parent uid, order) | `parent:`, `order:` |
| 4 | field | (uid, field name) | the field's value | header `title:`, `priority:`, `criticality:`, `confidence:`, `authority:`, `flags:`; `field` lines; `label` lines |
| 5 | observation | (uid) | a conflict value over an artifact's six observation fields; never a plain value (§6.5) | its `conflict` line |
| 6 | counter | (uid, field name) | the counter's total | `incr` ledger lines |
| 7 | edge | (src uid, edge-kind name, dst uid, disc) | `present(props)` | `edge` lines; `anchor` lines for `at` edges |
| 8 | body | (uid) | the BLAKE3-128 of the body | the `---` section |
| — | schema item | (item class, item key) | the item (§9) | `schema/*.moi`, `schema/queries/<q>.moi` |

**Order of classes.** The class code is the class's rank inside one uid (§10.3). The store-local `ckey` classes of
[F06 §6.1] use the same codes 1–8 (and 9 for a schema key), so one numbering serves both chapters (pass 1, A1-46). It
follows the `.moi` line order of
[AR §5b.2] ([AR §4.6], [60 §2.5] audit row "Canonical form"): the header's `kind`, `status` and `parent`/`order` lines,
then the `field` lines, the `incr` lines, the `edge` and `anchor` lines, and the body. Three kinds of line sit elsewhere in
a `.moi` file than their class's rank: the header lines of class-4 keys (`title`, the four header enumerations, `flags`),
the `label` lines, and the `conflict` lines (N12 groups them after the edge lines). An importer that reads a checkpoint in
tree order (uid order, [AR §5b.1]) therefore buffers **one node's lines** and emits that node's entries in rank order; it
never buffers across nodes ([AR §4.6]: "hashes a checkpoint node by node"). Open point 6.

### 6.2 Which fields are which keys

Every field item of the effective schema ([F08 §8.5.2], the core rows of [F08 §9.2]–§9.3 included) maps to at most one
key, by its merge class ([F08 §8.4.1]) and storage ([F08 §8.4.2]):

| Field | Key |
|---|---|
| merge class `status` (`status`, `resolution`) | the status key |
| merge class `hierarchy` (`parent`, `order`) | the hierarchy key |
| merge class `counter` | a counter key (uid, name) |
| storage `body` (`body`, whose class is `text` or `section-text`) | the body key |
| merge class `none` or `derived` | no key: not hashed ([F08 §8.4.1]) |
| every other field (merge classes `scalar`, `owner`, `authority`, `set`, `text`, `identity`, `observation`, `alias-set`, `glob-set`, `pathmove-set`) | a field key (uid, name), whatever its storage: `header` (`priority`, `criticality`, `confidence`, `authority`), `flag` (`pinned`, `archived`, `frozen`), `cold` (`defer_until`, `due`), `field`, `title` |

The six observation fields of an `artifact` (`path`, `oid`, `bytes`, `observed_git`, `observed_blob`, `relink`) are six
field keys while they hold plain values; their joint merge key is the observation key of §6.5 ([40 §2.2],
[RULES/merge-table] §2). `uid`, `kind`, `CREATOR`, `topo`, `rev_seq`, `created_tx`, `updated_tx`, `last_op_lsn`, the
derived columns and flags ([F08 §3.4]) and virtual `done` are not field items and have no key.

### 6.3 Values of a live node

For a node n that is live on V:

- **existence**: `live(k)`, k the name of n's kind.
- **status**: absent when n's status is its kind's initial status (the default of its `status` field, [F08 §9.1]) and its
  resolution is `none`; otherwise (status, resolution).
- **hierarchy**: absent when n has no parent and no `order`; otherwise (parent's uid or none, `order` or none).
- **field** f: **absent** when n has no value for f, when the value is empty (the empty text, the empty set, an `oid` with
  algorithm `none`), or when the value equals the default of f in V's effective schema ([F08 §6.2]: "a value equal to the
  field's default is never stored", extended here to every storage); otherwise the value. This holds for header,
  flag and cold storage too: `priority` = `P2`, `criticality` = `normal`, `confidence` = `unset`, `authority` = `agent`,
  `pinned` = false are absent. The `title` of a kind with `title_derived` (`artifact`) is absent: it is derived and not
  stored ([40 §2.2]).
- **counter** f: the total, the sum of every `Incr` of f ([AR §3.1]); absent when 0 (the counter default).
- **body**: BLAKE3-128 of the stored body bytes when n has a body ([F08 §7.2]); absent otherwise (open point 26).
- **edges**: one key per edge whose source is n (§6.6), holding `present(props)`.

Because a default is absent, adding a field with a default to the schema (a weakening change) changes no node's canonical
state; a strengthening change of a default changes the state of the nodes that relied on it, and `moirai migrate` writes
explicit values for them ([F08 §8.1]). The default used is the one of the schema of the state being described: the
first parent's schema for the parent's state, the commit's schema for the commit's state.

### 6.4 Tombstones

A deleted node's canonical state is exactly what its image tombstone carries ([AR §5b.2] rule 8, I39′):

- **existence**: `deleted(k, reason, replaced_by)`: k its kind, `reason` the text of the tombstone's reason (class `reason`,
  [F06 §7.4] `Delete`; `image:file-removed` for a foreign removal, [AR §5b.6] step 2), `replaced_by` the uid of the
  replacement or none;
- **field** `title`: the title kept at deletion; for an `artifact`, whose live title is derived, the last `path` text
  ([F08 §3.5]);
- **edges**: its retained out-edges: flagged `blocks` and `gates` edges with `flagged` set, historical edges with their
  props, `at` edges with their anchors (I39′, [40 §5.7]).

Every other key of a tombstone is absent: status, hierarchy, every field except `title`, every counter, the body. The
header columns that [F08 §3.5] keeps in a tombstone's `NodeHdr` (status, the header enumerations) are store-local
rendering aids and are not part of the canonical state (open point 10).

A node that is neither live nor deleted on V — never created there, or a tombstone reference with a `#N` but no node
([F08 §2.1]) — has no key in CS(V).

### 6.5 Conflict values

A key holds either a plain value or a **conflict value** `{class, base, ours, theirs}` ([AR §5a.8], [F06 §6.2]). A key
that holds a conflict value holds nothing else: its plain value is not part of the canonical state (N12: "the ordinary
line for a key is omitted while a `conflict` line exists for it").

- **Sides are plain.** base, ours and theirs are plain values of the key's class, never conflict values
  ([RULES/merge-table] open point 18, [F06] open point 21).
- **Observation composite.** A conflict on an artifact's observation composite (`FieldEdit`, or `PathClaim`,
  [RULES/link-merge-rules] PC-002) sits on the **observation key** (class 5). While it does, the six member field keys
  are absent. When the conflict is resolved, the observation key becomes absent and the member keys hold the resolved
  values. The observation key never holds a plain value.
- **Existence conflicts** (`DeleteVsModify`) sit on the existence key. Each side is an existence value; a `live` side
  carries that side's node image (§7.4; [F06 §6.2] "Snapshots"). The node's other keys hold the provisional state the
  merge produced ([AR §5a.7] step 4: a tombstone under `delete-wins`, the modifying side's values under `resurrect`).
- **Counters never conflict** ([AR §5a.7] step 4).
- **Schema items** may hold conflict values: a named query changed on both sides (`FieldEdit`) or dropped on one and
  changed on the other (`DeleteVsModify`) ([50 §4.4], [RULES/merge-table] MC-014).

### 6.6 Edges

- **Key.** An edge key is (src uid, edge-kind name, dst uid, disc), where disc is the 16-byte anchor uid on an `at` edge
  and empty on every other kind ([AR §4.6] item 10, [40] R-4). An edge is a key of its **source** node, as the image
  writes out-edges only in the source's file ([AR §5b.1]).
- **Symmetric kinds** (`contradicts`, `relates`): the source is the endpoint whose uid is bytewise smaller
  ([F08 §10.1]), so a pair written in either direction is one key.
- **`parent`** is never an edge key: parenthood is the hierarchy key ([F08 §10.1]).
- **`at`** edges are edge keys like any other and sort among edge kinds by the name `at` (R-10: "no new key class"). Their
  value is the edge value of §8 with the selector block.
- **In-edges** are derived and have no key.

## 7. Canonical value encodings

### 7.1 Typed values: `cv`

A typed value (a field value, a set element's payload, a default) is a tag byte and a payload. These **canonical tags**
are this chapter's own and are never stored: the stored type bytes are [F08 §5.1]'s, which a writer maps to the tags
below (pass 1, P1-1: the stored registry has one owner, [F08]). Tag 8 is unused, because the interned and inline forms of
a text (`sym` and `text`, [F08 §5.1]) are one value.

| Tag | Name | Payload | Rule |
|---|---|---|---|
| 0 | `absent` | none | the value of an absent key; never inside a set, a side of a node image, or a default |
| 1 | `false` | none | bool |
| 2 | `true` | none | bool |
| 3 | `int` | `i64` | two's complement, little-endian |
| 4 | `counter` | `i64` | a counter's total; only inside a node image (§7.4) |
| 5 | `f64` | 8 bytes | the IEEE 754 binary64 bit pattern as a little-endian `u64` ([F01 §5.5]); −0.0 enters as +0.0 (`00 × 8`); NaN and ±∞ never occur ([F08 §5.3]) |
| 6 | `enum` | `lp(name)` | the value's name in its field's enumeration (§2.2) |
| 7 | `text` | `lp(bytes)` | UTF-8, non-empty; both stored forms (`text`, `sym`) |
| 8 | — | — | unused |
| 9 | `set` | `elem` `u8` ‖ `count` `u32` ‖ `count` element payloads | `elem` is the element's tag (one of 3, 6, 7, 10, 11, 12, 13, 14); `count` ≥ 1; the payloads (without tags) in the order of §2.4 |
| 10 | `ref` | `b16` | the uid of the referenced node |
| 11 | `commit-ref` | `b32` | a full commit id |
| 12 | `path` | `lp(root name)` ‖ `lp(path text)` | the root by name (R-1, S-19); the text's exact bytes |
| 13 | `oid` | `lp(algo name)` ‖ `lp(digest)` | algorithm `sha1` (20 bytes) or `sha256` (32 bytes) |
| 14 | `pathmove` | `u64` hlc ‖ `lp(class name)` ‖ `path` from ‖ `path` to ‖ `lp(git algo name)` ‖ `lp(git digest)` | the two `path`s as tag 12's payload; the `git` oid with both parts empty when it is empty (R-1's "oid-or-empty") |

- **Empty is absent.** The empty text, the empty set and an `oid` of algorithm `none` are `absent` (tag 0), never tag 7
  with length 0, tag 9 with count 0 or tag 13 with empty parts ([F08 §6.2]; open point 9).
- A field's value always has its field's declared type ([F08 §8.5.2]); a writer that finds another type has a defect.
- `pathmove.hlc` is the value [40 §2.4] defines (S-20); it is hashed as data and compared with nothing.

### 7.2 State values per key class

| Class | Value encoding |
|---|---|
| 1 existence | `ex` `u8`, then: 0 absent; 1 `live`: `lp(kind name)` ‖ `snap` `u8` (0 or 1) ‖ node image (§7.4) when `snap` = 1; 2 `deleted`: `lp(kind name)` ‖ `lp(reason text)` ‖ `lp(replaced_by uid)` (empty when none) |
| 2 status | `st` `u8`, then: 0 absent; 1: `lp(status name)` ‖ `lp(resolution name)` |
| 3 hierarchy | `hf` `u8`, then: 0 absent; 1: `lp(parent uid)` (empty when none) ‖ `lp(order)` (empty when none), not both empty |
| 4 field | `cv` (§7.1) |
| 5 observation | `of` `u8`, then: 0 absent; 1: six `cv`s in the order `path`, `oid`, `bytes`, `observed_git`, `observed_blob`, `relink` (each may be tag 0) |
| 6 counter | `cv` tag 4 (the total) inside node images; entries carry a delta instead (§7.5) |
| 7 edge | §8.1 |
| 8 body | `bf` `u8`, then: 0 absent; 1: `b16` |

- `snap` is 1 exactly for a `live` value that is a side of an existence conflict, and 0 everywhere else ([F06 §6.2]).
- `deleted` carries the kind so that a merge or an import that lands a tombstone its first parent lacked has a complete
  value (open point 4).
- A reason is text of any length, the empty text included; `lp("")` is the empty reason.

### 7.3 Conflict states: `cstate`

The value of every key of classes 1–5, 7 and 8, and of every schema key, in an entry and as a state value:

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `cs` | `u8` | always | 0 plain, 1 conflict |
| 2 | `value` | the class's value (§7.2, §8.1, §9) | `cs` = 0 | the plain value |
| 3 | `class` | `lp(name)` | `cs` = 1 | the conflict class by name (§2.2) |
| 4 | `base` | the class's value | `cs` = 1 | base side, plain |
| 5 | `ours` | the class's value | `cs` = 1 | dst side, plain |
| 6 | `theirs` | the class's value | `cs` = 1 | src side, plain |
| 7 | `prov` | `lp(name)` | `cs` = 1 and the key's class is existence (1) | the provisional side, `ours` or `theirs` ([F12 §6.3], [F06 §6.2]; pass 1, S1-5, A1-9) |

Two values of one key are **equal** when their `cstate` encodings are byte-equal. This is the equality of
[RULES/merge-table] §2 ("two values are equal if and only if their canonical encodings are byte-equal"), and the one rule
of value equality: [F12 §7.3] cites it (pass 1, A1-19). A plain `live` value has `snap` = 0 and no image (§7.2), so plain
existence values compare by kind alone; a node image takes part only inside a conflict value's side, where it is part of
the value, and `prov` takes part in an existence conflict.

### 7.4 Node images

A node image is the list of a node's value keys on one side of an existence conflict ([F06 §6.3]): its status, fields,
counters and body. It holds no existence, hierarchy or edge key (open point 12).

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `n` | `u32` | always | number of entries |
| 2 | entries | `n` × entry | always | sorted by (class code, name bytes); one entry per key |

An entry is its class code `u8`, then:
- 2 status: `lp(status name)` ‖ `lp(resolution name)`;
- 4 field: `lp(field name)` ‖ `cv`, not `absent`;
- 6 counter: `lp(field name)` ‖ `cv` tag 4, not 0;
- 8 body: `b16`.

Only non-absent values of §6.3 appear; a side whose every value key is absent has `n` = 0.

### 7.5 Counter deltas

An entry of class 6 carries the counter's **net delta** ([AR §4.6] item 10: "counters (uid, field) → net delta"): the total
in the commit's state minus the total in the first parent's state, where an absent total is 0. A delta is never 0 (a key
whose total did not change has no entry). The difference of two `i64` totals can exceed the `i64` range, so the delta is
a sign and a magnitude, 9 bytes:

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 1 | `u8` | `sign` | 0 positive, 1 negative |
| 1 | 8 | `u64` | `magnitude` | the absolute value, 1 to 2^64 − 1 |
| total | 9 | | | |

## 8. Edge values and the anchor selector block (R-10)

### 8.1 The edge value

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `ef` | `u8` | always | 0 absent, 1 present |
| 2 | `pflags` | `u8` | `ef` = 1 | [F08 §10.2]'s `pflags`: bit 0 `has_pin`, bit 1 `flagged`, bit 2 `anchor`; bits 3–7 zero |
| 3 | `pinned` | `b32` | `pflags` bit 0 | the full id of the pinned commit (`pinned_commit`, [AR §3.3]) |
| 4 | `selector` | the selector block (§8.2) | `pflags` bit 2 | the anchor of an `at` edge |

- The bits are the stored block's ([F08 §10.2]; pass 1, S1-2, A1-3): `flagged` only on `blocks` and `gates`, set on a
  retained out-edge that was neither re-pointed nor resolved (X4, [AR §5b.2] rule 8); `has_pin` only on kinds with
  `props = pinned`, set when the edge has a pin; `anchor` exactly on `at` edges.
- `pinned` is the full 32-byte id, which every stored form keeps ([F08 §10.2], [F09] `EDGE_PROPS`).

### 8.2 The selector block

The anchor record of one `at` edge ([40 §2.7], R-4; stored in [F08 §10.3]'s one layout, which the ops of [F06 §7.5] carry)
enters as this block. Its
field order is the `anchor` line's order of [AR §5b.2] rule 9, with `occurrence` after `end_h` and `marker` after `pred`
(open point 21). Every field is written; an absent one is `lp("")`.

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `kind` | `lp(name)` | always | `file`, `heading`, `symbol`, `quote`, `range` or `lines` |
| 2 | `mode` | `lp(name)` | always | `live` or `pinned` |
| 3 | `watch` | `lp(name)` | always | `header` or `span` |
| 4 | `scope` | `lp(bytes)` | always | the scope value's bytes of [F08 §10.3.1] (the bytes that enter `captured`, [F08 §11.1]); empty when the anchor has no scope |
| 5 | `quote_h` | `lp(b16)` | always | BLAKE3-128 of `quote.exact`; empty unless `kind` is `heading`, `symbol`, `quote` or `range` |
| 6 | `prefix_h` | `lp(b16)` | always | BLAKE3-128 of `prefix.exact` as widened at capture; empty exactly when `quote_h` is |
| 7 | `suffix_h` | `lp(b16)` | always | BLAKE3-128 of `suffix.exact` as widened; empty exactly when `quote_h` is |
| 8 | `end_h` | `lp(b16)` | always | BLAKE3-128 of `end.exact`; empty unless `kind` is `range` (S-04) |
| 9 | `occurrence` | `lp(u16)` | always | the 1-based occurrence index; empty when none |
| 10 | `hint` | `lp(u32 first ‖ u32 last)` | always | the hint's first and last line (1-based, inclusive); empty when none |
| 11 | `window` | `lp(W)` | always | the window value W of [F20 §2.7.3], as one byte string; empty when none |
| 12 | `span_hash` | `lp(u64)` | always | XXH3-64 of the span or header ([F20 §2.8]) as a little-endian value ([F01 §7.2]); empty when none (a `file` anchor) |
| 13 | `blob_algo` | `lp(name)` | always | the algorithm of `blob` ([F20 §2.3]); empty when `blob` is empty |
| 14 | `blob` | `lp(digest)` | always | the file's `oid` at capture; empty when there was no content |
| 15 | `git_algo` | `lp(name)` | always | the algorithm of `git`; empty when `git` is empty |
| 16 | `git` | `lp(digest)` | always | the observed git commit at capture; empty when none |
| 17 | `captured` | `b16` | always | the capture digest ([40 §2.7], [F08 §11.4]) |
| 18 | `pred` | `lp(b16)` | always | the predecessor term of the anchor uid (S-03); empty when none |
| 19 | `marker` | `lp(text)` | always | the in-file marker id; empty when none |
| 20 | `resolver` | `u16` | always | the resolver version at capture (1 in format v1, [F20]) |

- **Texts never enter.** `quote`, `prefix`, `suffix` and `end` enter **only** as their digests ([40] R-10, [72 M6],
  S-04). A digest is taken over the exact bytes of the normalised anchor text N ([F20 §2.5], [F20 §6.1]); an empty prefix
  or suffix has the digest of the empty string, which is present, not empty.
- **The uid is the key.** The anchor uid is the edge key's disc (§6.6), not a field of the block.
- **Not in the block**: the store-local handle `aN` ([40] R-6), the `text_unavailable` state and the texts themselves.
- Every present field is non-empty: `scope` has at least 2 bytes, `occurrence` 2, `hint` 8, `window` at least 4
  ([F20 §2.7.3]), `span_hash` 8, `pred` 16, `marker` 1–64 ([F08 §10.3]), so `lp("")` always means absent.

### 8.3 Full and hash-only anchors hash alike

A store that holds an anchor's texts computes each digest from its text; a store that imported the anchor from a
`hash-only` destination holds the digests alone (the `text-unavailable` sub-state, [40 §5.7]). Both produce the same
selector block, so `image.dest.<name>.anchor-text` changes no `changeset_digest` and no commit id ([72 M6], I28′). A
change that only adds or drops the texts of an anchor whose digests are unchanged is no change of the canonical state
and yields no entry.

## 9. Schema items

Item 10 hashes the view's **schema items**: the project kinds, fields, enumeration values, edge kinds, named queries
and policy rows of [F08 §8.5]. The core schema of schema version 1 ([F08 §9]) is not in item 10; item 7 identifies it. Every part of an
item that [F08 §8.5] marks store-local (`kind_id`, `edge_id`, an enumeration value's integer, the `covers` integers, the
`ext` bits of a `KindSet`) and the derived `ast_hash` are replaced by names or left out, as below.

### 9.1 Keys

| Item class | Code | Key components, each `lp(bytes)` |
|---|---|---|
| kind | 1 | kind name |
| field | 2 | kind name, or `*` for every kind; field name |
| enumeration value | 3 | kind name or `*`; field name; value name |
| edge kind | 4 | edge-kind name |
| named query | 5 | query name |
| policy row | 6 | policy row name ([F08 §8.5.6]; spec sync 2b) |

A schema key's value is `cstate` (§7.3) over `sf` `u8` (0 absent, 1 present) followed, when present, by the item value
of §9.2–§9.7. A dropped named query and a removed policy row are absent ([F08 §8.1]); a retired item is present with
`retired` set.

### 9.2 Kind

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `retired` | `bool8` | always | [F08 §8.5] `iflags.retired` |
| 2 | `uid_derivation` | `lp(name)` | always | [F08 §8.4.7] |
| 3 | `root_variant` | `lp(name)` | always | `none` or `root-key` |
| 4 | `existence_policy` | `lp(name)` | always | [F08 §8.4.5] |
| 5 | `title_derived` | `bool8` | always | `kflags` bit 0 |
| 6 | `immutable_fields` | `bool8` | always | `kflags` bit 1 |
| 7 | `has_done` | `bool8` | always | `kflags` bit 2 |
| 8 | `done_derived` | `bool8` | always | `kflags` bit 3 |

### 9.3 Field ([50] F2)

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `retired` | `bool8` | always | |
| 2 | `type` | `lp(name)` | always | [F08 §5.1]'s type name (`text` and `sym` are distinct here: the storage form is part of the item) |
| 3 | `elem` | `lp(name)` | always | the element type's name for `set`; empty otherwise |
| 4 | `class` | `lp(name)` | always | merge class, [F08 §8.4.1] |
| 5 | `storage` | `lp(name)` | always | [F08 §8.4.2] |
| 6 | `decl` | `u16` | always | declaration order |
| 7 | `optional` | `bool8` | always | F2 |
| 8 | `index` | `lp(name)` | always | F2, [F08 §8.4.3] |
| 9 | `coerce` | `lp(name)` | always | F2, [F08 §8.4.4] |
| 10 | `one_line` | `bool8` | always | `cflags` bit 2 |
| 11 | `ascii` | `bool8` | always | `cflags` bit 3 |
| 12 | `has_default` | `bool8` | always | `cflags` bit 0 |
| 13 | `default` | `cv` | `has_default` | F2: the default value, never tag 0 |
| 14 | `has_range` | `bool8` | always | `cflags` bit 1 |
| 15 | `range_min` | `i64` | `has_range` | |
| 16 | `range_max` | `i64` | `has_range` | |

### 9.4 Enumeration value ([50] F2 `sort_rank`)

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `retired` | `bool8` | always | |
| 2 | `sort_rank` | `u16` | always | F2 |
| 3 | `side` | `bool8` | always | `eflags` bit 0 |
| 4 | `done` | `bool8` | always | `eflags` bit 1 |
| 5 | `n_covers` | `u32` | always | |
| 6 | `covers` | `n_covers` × `lp(value name)` | always | the values this value covers, by name, in the order of §2.4 |

### 9.5 Edge kind ([50] F1)

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `retired` | `bool8` | always | |
| 2 | `eclass` | `lp(name)` | always | `structural` or `historical` |
| 3 | `on_dst` | `lp(name)` | always | [F08 §8.4.6] |
| 4 | `on_src` | `lp(name)` | always | [F08 §8.4.6] |
| 5 | `acyclic` | `lp(name)` | always | [F08 §8.4.6] |
| 6 | `card` | `lp(name)` | always | [F08 §8.4.6] |
| 7 | `max_depth` | `u8` | always | |
| 8 | `uid_derivation` | `lp(name)` | always | `none` or `anchor-key` |
| 9 | `props` | `lp(name)` | always | [F08 §8.4.6] |
| 10 | `symmetric` | `bool8` | always | F1; `eflags` bit 0 |
| 11 | `same_kind` | `bool8` | always | `eflags` bit 1 |
| 12 | `lq_name` | `lp(name)` | always | F1 |
| 13 | `src_kinds` | `KindSet` | always | F1 |
| 14 | `dst_kinds` | `KindSet` | always | F1 |
| 15 | `n_reverse` | `u32` | always | |
| 16 | `reverse_names` | `n_reverse` × `lp(name)` | always | F1, in the order of §2.4 |
| 17 | `reading` | `lp(text)` | always | F1: the reading template |

`KindSet` ([F08 §8.4.8]) in canonical form:

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `any` | `bool8` | always | every kind |
| 2 | `n` | `u32` | always | 0 when `any` |
| 3 | `kinds` | `n` × `lp(kind name)` | always | core and project kinds by name, in the order of §2.4 |

### 9.6 Named query ([50] F3)

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `lq_version` | `u16` | always | the LQ grammar version of the text |
| 2 | `params` | `lp(text)` | always | the parameter signature in portable text form; empty for none |
| 3 | `shape` | `lp(text)` | always | the shape word |
| 4 | `budget` | `lp(text)` | always | the budget-class word |
| 5 | `text` | `lp(text)` | always | the query text in **portable form** ([50 §4.4], S-02): every node-typed constant as `#u:<uid>`, every revision-typed constant as a full commit id |

These are [F08 §8.5.5]'s hashed fields 2–6 (the name is the key). The canonical-AST hash `ast_hash` is derived and not
hashed ([50] F3). A definition is one atomic value ([50 §4.4]); its entry changes when any of the five fields changes.

### 9.7 Policy row (spec sync 2b)

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `value` | `lp(text)` | always | [F08 §8.5.6] `value`, in its canonical form (the name is the key) |

A row equal to its default is never an item ([F08 §8.5.6]), so the default has one canonical form, absence.

## 10. Item 10: the net changeset and `changeset_digest`

### 10.1 Entries

Let P be the canonical state at the commit's first parent (the empty state for a commit with no parent) and Q the
canonical state at the commit. Item 10 is the set of **entries** (k, Q(k)) for every key k with P(k) ≠ Q(k), where a key
missing from a state is `absent`. Values are compared as their `cstate` encodings (§7.3); a counter key's values are its
totals, compared as integers, and its entry carries the delta of §7.5 instead of Q(k). One entry per key ([AR §4.6]
item 10: "one entry per key").

This definition is the same for every commit kind ([AR §4.6] "Net changeset = state diff"): what differs between kinds is
only which state Q is (§12). In particular it holds for a `sync`, whose stored op list is the residue (§10.5), and for
imports, which compute it from the tree (§12.2–§12.4).

The existence transitions of [AR §4.6] item 10 and [AR §2.15] are renderings of existence entries, not separate values:

| P(existence) | Q(existence) | Rendering |
|---|---|---|
| absent | `live(k)` | `created(k)` |
| `live(k)` | `deleted(k, r, b)` | `deleted(r, b)` |
| `deleted(…)` | `live(k)` | `undeleted` |
| absent | `deleted(k, r, b)` | a tombstone landed by a merge, sync or import: stored as [F06 §7.4]'s `CreateDeleted` (open point 5) |
| `deleted(k, r, b)` | `deleted(k, r′, b′)` | a changed tombstone |

A node that is created and deleted inside one commit, or deleted and undeleted to the same values, has no existence entry
([F06] NF-6).

### 10.2 Entry encoding

A **node entry**:

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `section` | `u8` | always | 1 |
| 2 | `uid` | `b16` | always | the owner of the key |
| 3 | `class` | `u8` | always | the class code of §6.1, 1 to 8 |
| 4 | `name` | `lp(field name)` | `class` = 4 or 6 | the field |
| 5 | `edge_kind` | `lp(edge-kind name)` | `class` = 7 | |
| 6 | `dst` | `b16` | `class` = 7 | the destination's uid |
| 7 | `disc` | `lp(b16)` | `class` = 7 | the anchor uid on an `at` edge; empty on every other kind |
| 8 | `value` | the delta of §7.5 | `class` = 6 | |
| 9 | `value` | `cstate` (§7.3) of the class's value | `class` ≠ 6 | Q(k) |

A **schema entry**:

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `section` | `u8` | always | 2 |
| 2 | `item_class` | `u8` | always | 1 to 6 (§9.1) |
| 3 | `key` | the key components of §9.1 | always | |
| 4 | `value` | `cstate` over `sf` and the item value | always | Q(k) (§9.1) |

### 10.3 Order

Entries are written in this order, with no two entries of one key:

1. every node entry, then every schema entry (the git tree order of `nodes/` before `schema/`, [AR §5b.1]; open point 7);
2. node entries by (uid bytewise, class code, name bytes, dst uid bytewise, disc), where the name is the field name for
   classes 4 and 6 and the edge-kind name for class 7, and an empty disc precedes a non-empty one ([AR §4.6]: "sorted by
   (uid, key class, field or edge-kind name, dst uid, discriminator)"); uids compare bytewise, which is also the order
   of their hexadecimal text ([F01 §5.6]);
3. schema entries by (item class, first key component bytes, second, third), each component compared bytewise; named
   queries are therefore in the byte order of their names ([F19 §12.5.5]).

"Bytes" and "bytewise" mean [F01 §6.6]'s comparison of the raw bytes, never of their `lp()` encoding.

### 10.4 The digest

```
changeset_digest = BLAKE3-256( lp("moirai-changeset-v1") ‖ E_1 ‖ … ‖ E_n ‖ u64(n) )
```

where E_1 … E_n are the entries in the order of §10.3 and n is their number, as a `u64`. An empty changeset (n = 0) has
the digest of `lp("moirai-changeset-v1") ‖ 00 00 00 00 00 00 00 00`. The same n is the `Moirai-Ops` trailer's entry
count ([AR §5b.4] row 10; §14.2).

### 10.5 `sync`

A `sync` commit stores only the residue of [AR §5a.3] ([F06 §7.8]), but its item 10 is the full state diff against the
lane parent ([AR §4.6]): Q is the lane parent's state with `main`'s window (M_{k−1}, M_k] applied and then the residue
([F06 §7.4] "Base state"). The writer computes the entries from the same fold the sync performs — for every key the
window or the residue touches, P(k) is the lane parent's value and Q(k) the value after window ⊕ residue — and asserts
that the result equals the diff; `doctor --verify` re-checks it ([AR §4.10]). The cost is O(window ops) of hashing, with
no extra I/O or storage.

### 10.6 Producing the entries

- **Local writes** compute the entries in phase 1 from the candidate ([AR §4.5] step 4), before `#N`s are allocated:
  entries name uids, never `#N`s, and new nodes have their uids at candidate time (random or derived, [F08 §2.2]).
- **Bulk producers** (a bulk commit, [AR §4.3]; `migrate`; a directory `file mv` with thousands of links) stream entries
  into BLAKE3 in the order of §10.3. A producer whose input is not in that order sorts externally, in spill runs of at most
  1 MiB named `tmp/sort.<nonce>` ([F02 §5.3], [AR §4.6]).
- **Every producer whose entries exceed its write budget spills** (pass 1, P1-19). The rule of the previous item is not
  limited to verbs of the bulk class: a merge, a `sync` (item 10 of a sync is the full diff over `main`'s window, §10.5,
  which for a lane 14 days behind can be large) and a revert or cherry-pick whose entries, held in memory for sorting,
  would exceed `wmem` ([F17 §4.4] W2) spill to `tmp/sort.<nonce>` runs by the same rule and merge them in §10.3's order.
  [F17 §4.4] W1's bulk class lists `sync` with the long merges, and GT11's RAM gate covers a sync 14 days behind
  ([AR §8.3] RAM row "sync"; the case is added to [60 §3.13] GT11 at WP-81a).
- **Checkpoint imports** read the tree in git order, which is uid order under `nodes/` ([AR §5b.1]), buffer one node at a
  time (§6.1), then read `schema/`, and so hash a whole checkpoint with memory bounded by the largest node.

## 11. What is not hashed

Every field of the stored record that §3 does not name, and every stored datum §10 does not reach, is not hashed
([AR §4.6] "Not hashed"). By field of [F06]:

| Not hashed | Where it is stored |
|---|---|
| `commit_id` itself, `parents[].id16` and `parents[].lsn` (the actual parents: item 2 hashes the stated ids), `gen`, `seq` | [F06 §4.3] orders 2, 4, 7, 8 |
| `ref`, `ref_id`, `ref_old`, `prev_on_ref`, `ref_seq` | orders 9–13 |
| `import`, `verified`, `ckpt` (`Moirai-Head`, `Moirai-Folded`), `xtr` (`Moirai-Ref`, `Moirai-Idem` as imported) | orders 15, 28–30 |
| `idem_key`, `idem_payload` | orders 24–25 |
| `sync_base` (item 2 hashes the same id as the second parent) and the absorbed vector | orders 26–27 |
| `stmt_origin`, `stmt_sym`, `stmt_hash` ([50] F10), `actor_src` ([90 §10.1]), `append_hlc` ([50] F14) | orders 31–35 |
| `affected`, `affected_len`, `affected_complete` ([50] F15, F16) | order 37 |
| `cs_ref` (a bulk commit hashes its entries, not its file) | order 39 |
| inside ops: every `#N`, `aN`, `prev`, symbol id, before-image, `Create.c_actor`/`c_role` (`CREATOR`), `Violation` ops | [F06 §7] |
| anchor `quote`, `prefix`, `suffix` and `end` texts (their digests are hashed) and the `text-unavailable` state | [F08 §10.3] |
| an import-checkpoint's image-only data (`ckimg`) and the header-only form's `pruned` bit | [F06 §4.4.14], [F06 §4.4.15] |
| a staged commit's staging arguments (`stage`) | [F06 §4.4.16] |
| bodies' bytes, codec and compressed form (the body key hashes BLAKE3-128 of the raw bytes) | [F06 §8], [F10] |
| every runtime table: markers, leases, `ALLOC` and `UIDX` ([50] F17), `IDEM`, `REFS`, `PINS`, `HEADS`, R4's resolution and evidence tables (file ids, volume serials, stat caches, fingerprints, proposals, intents, `ANCHORRES`; I-F4) | [F05], [F11] |
| derived state: every derived column and flag, `rev_seq`, virtual `done`, `ready`, the named query's `ast_hash` | [F08 §3.4], [F08 §8.5.5] |
| the store-local schema ids of [F08 §8.3] | [F09] |

A `Violation` op exists only on staging refs ([AR §5a.8]) and contributes nothing. A `Resolve` op contributes the key's
resulting value, not the choice ([AR §5b.4] row 10; §13).

## 12. Commit-id derivation per commit kind

Every commit's id is §3's C over its items. This section says where each item comes from, for each way a commit enters a
store. The stored record ([F06]) holds the values, so recomputing an id from a stored record needs no other input
(`doctor --verify`, the reference model, and the format oracle over fixtures that carry real commit ids: `canonical/`,
`carrier/`). Commit-id correctness is a C-rule ([F06 §4.3] order 2, [F06 §2.4]): a decoder never recomputes an id, so the
hex fixtures' synthetic ids (`fixtures/hex/INDEX.md` §2) are valid records and the oracle's decoders accept them (spec
sync 2b).

### 12.1 Local commits

| Item | `ordinary` | `merge` | `sync` | `revert` | `cherry-pick` |
|---|---|---|---|---|---|
| 1 kind | `ordinary` | `merge` | `sync` | `revert` | `cherry-pick` |
| 2 parents | the ref's tip (none for the store's root commit) | dst tip, src tip | lane tip, `main`'s tip (`sync_base`) | R's tip | R's tip |
| 3 `hlc` | [OS/clock §7] at the step [F16] names; new on re-parent | same | same | same | same |
| 4 | the resolved actor, role and session ([AR §5a.4], [90 §4.1]) | same | same | same | same |
| 5 | the caller's tree provenance (§3.6) | same | same | same | same |
| 6 | N(message) (§5.1) | same | same | same | same |
| 7 | 1 | 1 | 1 | 1 | 1 |
| 8 | empty | empty | empty | the reverted commit's full id | the picked commit's full id |
| 9 | empty | empty | empty | empty | empty |
| 10 | diff against the tip's state | diff against dst's state of the merged state, conflict values included ([AR §5a.7] step 7) | diff against the lane tip's state of window ⊕ residue (§10.5) | diff against R's tip of the state with the inverse applied ([AR §5a.5]) | diff against R's tip of the three-way result |

A merge or sync that stages lands on `merge/<dst>/from/<src>` with the same canonical form; its `Violation` ops are not
hashed. A `merge --continue` computes a new merge commit against the current dst tip ([AR §5a.7] step 8).

### 12.2 Native import

A git commit with a `Moirai-Commit` trailer that the store does not hold ([AR §5b.6] step 2). The importer rebuilds C from
the carriers of §14 and verifies BLAKE3-256(C) = `Moirai-Commit`:

- items 1, 3, 4, 5, 7, 8, 9 from their trailers; a missing trailer is the empty value (item 4, item 5 components,
  items 8 and 9);
- item 2: for each git parent in order, the id its `Moirai-Commit` trailer names (the **stated** id, even when that parent
  was demoted, §12.5); for a parent without a `Moirai-Commit` trailer, the id this store holds for that git object
  through `gitmap`;
- item 6: N (§5.1) of the message part that [F14] separates from the trailer block;
- item 10: the entries of the tree diff against the **first** parent's tree, for every kind, `sync` included (CM2): P is
  the canonical state of the first parent's tree, Q that of this commit's tree.

On a match the commit is appended with the same id, its kind and `import = native`, `verified = 1` ([F06 §3.3]). A `sync`
keeps its residue in the stored record; its item 10 is the full diff.

### 12.3 Foreign commits

A git commit with no `Moirai-Commit` trailer that is not a checkpoint commit, or with an unknown trailer ([AR §5b.4],
[AR §5b.6] step 3). Its id is a pure function of the git commit and its parents' ids, so two stores that import it agree
(I28′):

| Item | Value |
|---|---|
| 1 | `ordinary` for 0 or 1 git parents, `merge` for 2 |
| 2 | for each git parent in order, the id this store holds for it (through `gitmap`; for a demoted parent its foreign id) |
| 3 | [F06 §4.4.4]: `max((T × 1000) << 16, max over parents of (p.hlc + 1))`, T the committer timestamp in seconds |
| 4 | actor `git:` followed by the author's email (the bytes between `<` and `>` of the author line, decoded as in §5.3 step 1); role and session empty |
| 5 | all empty |
| 6 | N_imp (§5.3) of the whole git message |
| 7 | the `schema-version:` of the `.moirai-image` marker in the commit's tree ([AR §5b.3]) |
| 8 | empty |
| 9 | the destination's object format and the git commit's own object id |
| 10 | 0 or 1 parent: the tree diff against the first parent's tree (the empty tree for a root commit). 2 parents: the diff against the first parent's state of the state that the typed three-way merge over the two parents' imported states produces (I30′, [AR §5b.6] step 3) |

A git commit with more than two parents has no moirai kind: the import stages `ImageParse` ([F06 §3.3] allows at most two
parents; open point 17). A tree without a readable `.moirai-image` marker stages `ImageParse`.

### 12.4 Import-checkpoint commits

A checkpoint commit (`Moirai-Kind: checkpoint` and no `Moirai-Commit`, [AR §5b.4], N13a) gets a deterministic id:

| Item | Value |
|---|---|
| 1 | `checkpoint` |
| 2 | the id this store holds for the git parent — the ref's previous checkpoint — or none |
| 3 | as for a foreign commit (§12.3) |
| 4 | actor `image:checkpoint`; role and session empty |
| 5 | all empty |
| 6 | N_imp of the message part before the trailer block ([F14] strips the final paragraph at its first `Moirai-Kind:` line, [AR §5b.4]) |
| 7 | the marker's `schema-version:` |
| 8 | empty |
| 9 | the destination's object format and the checkpoint commit's object id |
| 10 | the tree diff against the git parent's tree, or against the empty tree |

`Moirai-Head` and `Moirai-Folded` are stored unhashed ([F06 §4.4.11]). A checkpoint that carries a whole store is a bulk
commit whose entries stream in tree order (§10.6). A re-export of an import-checkpoint commit to another destination is
written natively with `Moirai-Commit` and `Moirai-Foreign-Git` ([AR §5b.4] row 9), and imports there as §12.2 with kind
`checkpoint`.

### 12.5 Demotion

A commit whose recomputed hash differs from its `Moirai-Commit` trailer is demoted: it alone becomes a foreign commit with
§12.3's id and `verified = 0` ([F06 §4.4.10], N5, I29′). Its message is the whole git message, trailers included (§5.3).
Its children still verify against the ids their trailers state: a child's item 2 is the stated id, and its record keeps
the demoted commit as its actual parent ([F06 §4.4.1] `stated` group).

## 13. From stored ops to entries

The entries of item 10 are defined by §10.1 over states. A writer usually derives them from the net ops it stores
([F06 §7]); this table is that derivation. It is informative, and its result must equal §10.1's: `doctor --verify` and the
reference model recompute the digest from states, and a difference is a corruption finding ([F06 §2.4] C-rules).

| Stored op ([F06 §7.2]) | Entries |
|---|---|
| `Create{uid, kind, image}` | existence → `live(kind)`; one entry per non-absent value key of the image (§6.3): status, fields, counters (delta = the total), body |
| `Delete{reason, replaced_by, before}` | existence → `deleted(kind, reason, uid of replaced_by)`; every value key of `before` except `title` → absent (a counter's delta is minus its total); for a kind with a derived title, `title` → the last `path` text (§6.4). Hierarchy and edges change through their own ops |
| `CreateDeleted{uid, kind, reason, replaced_by, image}` | existence: absent → `deleted(kind, reason, uid of replaced_by)`; the `title` entry of the image, if any: absent → that title. Retained out-edges come from the record's `AddEdge` ops ([F06] NF-11) |
| `Undelete{image}` | existence → `live(kind)`; one entry per value key of the image that differs from the tombstone state (§6.4) |
| `SetField{name, new}` | field (uid, name) → `new` as a canonical value (§6.3); none when the canonical values are equal (a default written explicitly, `text` versus `sym` forms) |
| `SetStatus` | status → (new status, new resolution), or absent when they are the initial status and `none` |
| `Incr{name, delta}` | counter (uid, name) → delta |
| `SetBody` | body → the new hash, or absent |
| `AddEdge`, `RemoveEdge`, `SetEdgeProps` | edge → `present(props)` or absent; none when the canonical props are equal (§8.3) |
| `Move` | hierarchy → (new parent's uid, new order), or absent |
| `Schema` | schema key → the new item (§9), or absent |
| `Conflict{key, class, base, ours, theirs, prov}` | key → the conflict value (with `prov` on an existence key); for the observation key, each member field key that held a value → absent (§6.5) |
| `Resolve{key, new}` | key → `new` as a plain value; for the observation key, the observation key → absent and each member field whose resolved value is not absent → that value |
| `Violation` | none |

A merge's, sync's, revert's and cherry-pick's stored ops are net against the first parent's state ([F06 §7.8]), so the
table applies to them as to an ordinary commit; a `sync` adds `main`'s window (§10.5). A bulk commit applies the table to
the rows of its `cs.<n>` ([F06] BK-5).

## 14. The gate-0 carrier table (stub)

Every hashed item has exactly one carrier in the git commit, and the importer rebuilds C from these carriers and nothing
else (CB1, I38′, [AR §5b.4]). This section is a **stub**: it fixes which item each carrier holds and the per-kind fixtures
gate 0 needs ([60 §2.5] row "Gate-0 carrier table"). [F14] (WP-15) completes it with the trailer grammar, the `.moi`
ABNF and the verification steps, and keeps every row below.

### 14.1 Items 1–9

| Item | Native commit | Foreign commit (§12.3) | Import-checkpoint commit (§12.4) |
|---|---|---|---|
| 1 kind | trailer `Moirai-Kind: <name>` (§4) | derived: git parent count | `Moirai-Kind: checkpoint` |
| 2 parents' stated ids | the git parents, each mapped to its own `Moirai-Commit` trailer, order preserved; the second parent of a `sync` also as `Moirai-Sync-Base: c<64 hex>` (unhashed, must equal) | the git parents through `gitmap` | the git parent through `gitmap` |
| 3 `hlc` | trailer `Moirai-Hlc: <u64 decimal>` (authoritative; the author and committer times are `hlc >> 16` rounded to seconds and never read back) | derived: committer time and parents | derived, as foreign |
| 4 actor, role, session | trailers `Moirai-Actor`, `Moirai-Role`, `Moirai-Session`, each omitted when empty | derived: author email | derived: `image:checkpoint` |
| 5 git provenance | trailers `Moirai-Git-Head: <algo>:<hex>`, `Moirai-Git-Branch`, `Moirai-Worktree`, `Moirai-Git-Base: <algo>:<hex>`, each omitted when empty; `git_algo` is the `<algo>` of head or base (both must agree) and empty when neither is present (§3.6) | empty | empty |
| 6 message | the git message before the trailer block | the whole git message | the git message before the trailer block |
| 7 schema version | trailer `Moirai-Schema: <n>` | the `.moirai-image` marker's `schema-version:` line | the marker's `schema-version:` line |
| 8 origin | trailer `Moirai-Origin: c<64 hex>` | empty | empty |
| 9 `foreign_git` | trailer `Moirai-Foreign-Git: <algo>:<hex>` | the destination's object format and the commit's oid | the destination's object format and the commit's oid |

### 14.2 Item 10 by key class

Item 10 is carried by the tree diff against the first parent's tree ([AR §5b.4] row 10, [AR §5b.6] step 2), node files
parsed through the `.moi` codec; `Moirai-Ops: <n>` carries §10.4's entry count as a pre-check.

| Key class | Tree path | Carrier lines ([AR §5b.2]; [F14] fixes the grammar) |
|---|---|---|
| 1 existence | `nodes/<h1>/<h2>/<uid>.moi` | the file's presence and `kind:` line; the tombstone form (`deleted:` line with `field reason` and `field replaced_by` lines, which in a tombstone carry the existence value, not field keys) |
| 2 status | same | `status:` and `resolution:` lines; the kind's initial status with resolution `none` is `absent` (§6.3) |
| 3 hierarchy | same | `parent:` and `order:` lines |
| 4 field | same | header lines `title:`, `priority:`, `criticality:`, `confidence:`, `authority:`, the `flags:` line (`pinned`, `archived`, `frozen`), `field <name>:` lines, `label` lines (the `labels` field); a line whose value is the field's default is `absent` (§6.3) |
| 5 observation | same | the `conflict` line of the observation composite |
| 6 counter | same | `incr <field> <±k> c<commit>` ledger lines: the delta is the sum of the lines the new file has minus the sum of those the parent file had |
| 7 edge | same, the source's file | `edge <kind> -> <uid> [pin=c<commit>] [flagged]` lines; `anchor <anchor uid> -> <dst uid> …` lines for `at` edges, whose digests, `captured`, `pred`, `occurrence`, `marker` and `v=` fields carry the selector block of §8.2 in both anchor-text modes |
| 8 body | same | the section after the `---` line (BLAKE3-128 of the decoded bytes) |
| conflict values | same | `conflict <key> class=<class> base=… ours=… theirs=…` lines; a body conflict's sides are texts the importer hashes |
| schema items | `schema/kinds.moi`, `schema/fields.moi`, `schema/edges.moi` (rows), `schema/queries/<q>.moi` (q = 32 hex digits of BLAKE3-256 of the name, [50] F3, [80] X-F9) | the rows of §9; a query file's `name:`, `lq:`, `params:`, `shape:`, `budget:` lines and its text |

### 14.3 Carrying no item

These are in the git commit or the tree and carry no hashed item; an importer never builds C from them:

- trailers `Moirai-Commit` (the id being verified), `Moirai-Ref` and `Moirai-Idem` (informational), `Moirai-Ops` (a
  pre-check), `Moirai-Sync-Base` (a copy of parent 2), and a checkpoint's `Moirai-Head` and `Moirai-Folded`;
- the author and committer lines ([AR §5b.4]);
- the `.moi` lines `moirai-node 1`, `uid:` (the key itself), `created:`, `updated:` and `deleted:` (commit references
  derived from history; `deleted:` marks the tombstone form);
- the `.moirai-image` lines `moirai-image 1` and `object-format:`;
- `refs/heads.moi` and `refs/tags.moi` of a checkpoint image, and every object of the unhashed side ref
  `refs/moirai/meta/<store-id>` ([AR §5b.1]).

### 14.4 Gate-0 fixtures

Gate 0 exports one commit of each kind, rebuilds C from its carriers and asserts `commit_id` equality before any
round-trip corpus runs ([AR §5b.7], [AR §5b.5] rule 8). [PLAN §3.2] WP-21 writes the fixtures (`fixtures/canonical/` and
`fixtures/carrier/`) from this chapter and [F14]; each needs:

| Case | Required content |
|---|---|
| `ordinary` | one parent; git provenance with both digests; a field, a status, a counter delta, a body, an edge with a pin |
| `merge` | two parents; at least one conflict value (a field `FieldEdit`) and one clean key from each side |
| `sync` | two parents; a residue smaller than the full diff, so that item 10 differs from the stored ops |
| `revert` | origin set; the inverse of an `ordinary` fixture |
| `cherry-pick` | origin set |
| foreign | no trailer; a two-parent case whose item 10 comes from the typed merge |
| import-checkpoint | kind `checkpoint`, a previous checkpoint as parent, and a first checkpoint with no parent |
| anchors | an `at` edge with a `range` anchor (S-04) and a `quote` anchor with an empty prefix, exported in `full` and in `hash-only` mode: the same `commit_id` |
| tombstones | a delete with flagged and historical retained edges; an undelete; a tombstone landed from the absent state |
| normalisation | messages of §5.4; a default written explicitly in the image (the default priority, the initial status) giving the same id as the line left out |
| order | two nodes, all eight classes on one uid, a symmetric edge written in both directions, a schema entry and a named query |

### 14.5 What [F14] completes

- The trailer grammar: exact byte extraction of each value (no trimming of the value's own bytes), the order of
  [AR §5b.4], and the rule that the trailer block is the **final** paragraph (§5.3).
- The `.moi` ABNF for every line above, including the `anchor` line fields `occurrence=` and `marker=`, which §8.2 hashes
  and [AR §5b.2] rule 9 does not list (open point 21), and a key text for the observation composite in `conflict` lines
  (open point 8).
- An import rule for a tombstone file whose uid the parent tree lacks (open point 5).
- Sorting `field` lines by field name bytes rather than by whole-line bytes (open point 6).
- The mapping of `.moi` values to `cv` values: the `path` root, `oid` and `pathmove` text forms, the `scope` text to the
  bytes of [F08 §10.3.1] (open point 9), JSON-string escapes, and values an importer refuses as `ImageParse` (NaN, an
  empty set written as `[]` if [F14] does not accept it as absent).

## 15. Checks and refusals

- **At write** ([F19]): §5.2's message refusals (exit 2, `bad_value`); a store-local number without its replacement
  (§2.3) is `store_corrupt` (exit 7). An `lp()` argument of 2^32 bytes or more ([F01 §6.3]) cannot occur: every hashed
  component is a part of one stored record or `cs.<n>` row, which fits one log extent or one sealed file ([F06 §4.6]).
- **At import** ([AR §5b.6] step 4): a carrier that does not parse, a `Moirai-Git-Head` and `Moirai-Git-Base` whose
  algorithms differ, a schema version other than 1, a git commit with more than two parents, or a tree without a readable
  marker stages `ImageParse`; a hash mismatch demotes (§12.5).
- **`doctor --verify`** recomputes, for every commit it reaches, `changeset_digest` from the states (§10.1) and
  `commit_id` from the record (§12), and re-checks the `sync` identity (§10.5); a mismatch is a corruption finding (exit 7
  from `doctor`).
- **The reference model** ([60 §4], [PLAN §3.2] WP-91) has its own encoder for this chapter (≈ 300 lines) and reproduces
  every commit-id fixture (E3).

## 16. Examples (informative)

Byte strings are hexadecimal. `P…` is a 32-byte parent id and `D…` a 32-byte digest; their values are not computed here.
Fixtures are written from the rules above, never from these examples.

**A status entry.** Task uid `00112233445566778899aabbccddeeff` moves from `open` to `in_progress` (43 bytes):

```
01                                             section: node
00 11 22 33 44 55 66 77 88 99 AA BB CC DD EE FF   uid
02                                             class: status
00                                             cs: plain
01                                             st: present
0B 00 00 00 69 6E 5F 70 72 6F 67 72 65 73 73   lp("in_progress")
04 00 00 00 6E 6F 6E 65                        lp("none")
```

The digest input of a commit with only this entry is 74 bytes: `13 00 00 00 6D 6F 69 72 61 69 2D 63 68 61 6E 67 65 73 65
74 2D 76 31` (`lp("moirai-changeset-v1")`), the entry, and `01 00 00 00 00 00 00 00` (n = 1).

**The commit input C** of that commit: kind `ordinary`, one parent, `hlc` `0x01A0C4506C000003` ([F01 §5.7]), actor
`dev#1`, role `developer`, session `claude:s1`, no git provenance, message `claim --start` (196 bytes):

```
10 00 00 00 6D 6F 69 72 61 69 2D 63 6F 6D 6D 69 74 2D 76 31   lp("moirai-commit-v1")
08 00 00 00 6F 72 64 69 6E 61 72 79                          lp("ordinary")
01 00 00 00  P… (32)                                         one parent
03 00 00 6C 50 C4 A0 01                                      hlc
05 00 00 00 64 65 76 23 31                                   lp("dev#1")
09 00 00 00 64 65 76 65 6C 6F 70 65 72                       lp("developer")
09 00 00 00 63 6C 61 75 64 65 3A 73 31                       lp("claude:s1")
00 00 00 00 × 5                                              item 5: all empty
0D 00 00 00 63 6C 61 69 6D 20 2D 2D 73 74 61 72 74           lp("claim --start")
01 00 00 00                                                  schema version 1
00 00 00 00                                                  item 8 empty
00 00 00 00 00 00 00 00                                      item 9 empty
D… (32)                                                      changeset_digest
```

**Values.** A `path` value `project:docs/a.md`: `0C 07 00 00 00 70 72 6F 6A 65 63 74 09 00 00 00 64 6F 63 73 2F 61 2E 6D
64`. A `labels` set {`storage`, `l5`, `perf`} in the order of §2.4 (`l5`, `perf`, `storage`, because each encoding begins
with its little-endian length): `09 07 03 00 00 00 02 00 00 00 6C 35 04 00 00 00 70 65 72 66 07 00 00 00 73 74 6F 72 61 67 65`.
A counter delta of −3: `01 03 00 00 00 00 00 00 00`. A `pathmove` value with `hlc` `0x01A0C4506C000003`, class
`explicit`, `docs/plan/` to `docs/archive/plan/` in root `project`, no git commit:

```
0E 03 00 00 6C 50 C4 A0 01 08 00 00 00 65 78 70 6C 69 63 69 74
07 00 00 00 70 72 6F 6A 65 63 74 0A 00 00 00 64 6F 63 73 2F 70 6C 61 6E 2F
07 00 00 00 70 72 6F 6A 65 63 74 12 00 00 00 64 6F 63 73 2F 61 72 63 68 69 76 65 2F 70 6C 61 6E 2F
00 00 00 00 00 00 00 00
```

**An anchor in two modes.** A `quote` anchor with an empty prefix has `prefix_h` = BLAKE3-128 of the empty string, so
its field 6 is `10 00 00 00` followed by `AF 13 49 B9 F5 F9 A1 A6 A0 40 4D EA 36 DC C9 49` (the first 16 bytes of the
digest that the BLAKE3 test vectors give for the empty input; the fixtures take the value from the implementation, not
from here). A store holding the texts and a store that imported the anchor `hash-only` write the same 20 fields of §8.2,
so the edge entry, `changeset_digest` and `commit_id` are equal.

## Coverage

| Item | Part covered here | Section |
|---|---|---|
| [60 §2.5] row "Canonical form" ([AR §4.6] items 1–10) | complete: the byte encoding of every item, `commit_id`, the domain prefixes | §3, §10 |
| [60 §2.5] audit row "Canonical form" (item 10 through `changeset_digest`; inside a uid the key classes in `.moi` line order; anchor quote, prefix and suffix only as BLAKE3-128 digests) | complete, with `end_h` (S-04) | §3.11, §6.1, §8.2, §10 |
| [60 §2.5] row "Gate-0 carrier table" | the stub: item → carrier (tree path or trailer) for items 1–10 and every key class, per commit kind, and the fixture list; the grammar is [F14]'s | §12, §14 |
| [60 §2.5] row "Ops and values" | the canonical encodings of the closed type set {bool, int, counter, f64, enum-with-lattice, text, set, ref, commit-ref} and R-1's types; the stored encodings are [F06]'s and [F08]'s | §7.1 |
| [60 §2.5] row "Schema as data" | the canonical encoding of schema items (kinds, fields with type, lattice and class, edges with class, policy, acyclicity and cardinality); the item records are [F08]'s | §9 |
| [60 §2.5] row "Image format v1" | only the hashing requirements on `anchor` lines, the tombstone form and the carriers; the ABNF is [F14]'s | §14.2, §14.5 |
| [60 §2.5] R4 row R-10 and [40] R-10 | complete: the selector block as the edge-property value of an item-10 edge entry, the discriminator in the key, `quote_h`, `prefix_h`, `suffix_h`, `end_h`, no new key class | §6.6, §8 |
| [40] R-1 | the canonical encodings of `path`, `oid` and `pathmove`, the root by name (S-19); the stored layouts are [F06]'s and [F08]'s | §7.1 |
| [40] R-2 | the observation composite as one conflict key with member field keys; `identity` and `observation` fields as field keys; `planned`/`removed` statuses by name | §6.2, §6.5, §7.2 |
| [40] R-3 | the `uid_derivation` column in kind and edge items by name; derived uids enter as uids. The derivations are [F08]'s | §9.2, §9.5 |
| [40] R-4 | the `at` edge key with its 128-bit discriminator; `SetEdgeProps` as an edge entry; `aN` not hashed | §6.6, §8, §11, §13 |
| [40] R-5 | no canonical item for directory moves: `path_moves` is a field key holding a set of `pathmove` values | §6.2, §7.1 |
| [40] R-11 | the requirements the canonical form puts on `anchor` lines (digests on every line, `occurrence`, `marker`, `v=`; hash-only changes no id); the grammar is [F14]'s | §8.3, §14.2, §14.5 |
| [40] R-12 | I-F4 on the canonical side (no resolution or evidence datum is hashed); the invariant texts are [F18]'s | §11 |
| [40] R-17 | `relink` is hashed as its text bytes; the vocabulary is [F18]'s | §6.2, §7.1 |
| [50] F1 | the canonical edge-kind item (`lq_name`, `src_kinds`, `dst_kinds`, `symmetric`, `reverse_names`, `reading`) | §9.5 |
| [50] F2 | the canonical field item (`optional`, `default`, `index`, `coerce`) and enumeration item (`sort_rank`) | §9.3, §9.4 |
| [50] F3 | complete for hashing: the named query as a schema item in portable form, ordered by name, `ast_hash` unhashed; its image file is [F14]'s | §9.6, §10.3 |
| [50] F10, F14, F16 | not hashed: `stmt_origin`, `stmt_sym`, `stmt_hash`, `append_hlc`, `affected_len`, `affected_complete`; the fields are [F06]'s | §11 |
| [50] F17 | `ALLOC` not hashed; `#N` replaced by uids through it | §2.3, §11 |
| [50] F18 | `QueryInvalid` and `QueryCycle` are violations and never hashed | §11 |
| [80] X-F7 | the `git.worktree` provenance string enters item 5 as [OS/path §4.5]'s `c.text`; `path` values enter as exact bytes, never folded | §3.6, §7.1 |
| [80] X-F9 | only the carrier `schema/queries/<q>.moi` of named queries; the name rule is [F14]'s and [OS/path]'s | §14.2 |
| [80 §3.2] "Canonical commit form" (X1) | the encoding is defined without reference to the OS | §2.1 |
| [90 §10.1] row "Commit header" (`actor_src`) | not hashed | §11 |

No other X-F item and no other [90 §10.1] item is specified here.

## Holes

None. No value of this chapter is decided by an M0 measurement or benchmark. The length of the window value W depends on
HOLE(F20-window-lines) ([F20 §2.7.2]); W enters the selector block `lp()`-framed (§8.2), so no canonical byte layout
depends on the fill. The codec holes of [F10] do not reach the canonical form, which hashes raw body bytes.

## Open points for the review

1. **[PLAN §3.3] gap "the length-prefix scheme and the value encodings of canonical items" (WP-12), closed here.** Every
   variable-length component is `lp()` ([F01 §6.3]); integers are fixed-width little-endian; no varint enters; counts are
   `u32` and the entry count `u64`; each digest has a domain prefix (§2.1, §2.5). Values are §7's `cv` and per-class
   encodings. The other WP-12 gaps (import-checkpoint kind value, foreign `hlc` unit, the header fields F10, F14, F16 and
   `actor_src`) are closed in [F06].
2. **Item 10 is the full state diff** (§10.1). Deleting a node yields an existence entry plus an `absent` entry for each of
   its value keys (a counter's delta is minus its total); undeleting yields each restored value. The alternative — an
   existence entry that implies its value keys — needs a special rule per class and differs from what an importer's line
   diff sees; this choice makes the writer's and the importer's computation the same function.
3. **Defaults are absent** (§6.3), for every field whatever its storage, and for status (the initial status with
   resolution `none`). [F08 §6.2] already stores no default in a field block; applying the rule to header, flag and cold
   storage makes the canonical state independent of storage, and adding a field with a default (a weakening) changes no
   node's state. Alternative rejected: hashing effective values, under which every weakening that adds a defaulted field
   would change every node of its kind. [F14] maps a written default to `absent` (§14.2).
4. **Existence values are states** (`live(kind)`, `deleted(kind, reason, replaced_by)`), the form [F06 §6.2] stores, so
   conflict sides and entries share one encoding; [AR §4.6]'s `created`, `deleted`, `undeleted` are renderings (§10.1).
   `deleted` carries the kind, which [AR]'s `deleted(reason, replaced_by uid)` omits, because a tombstone can arrive from
   the absent state.
5. **A tombstone arriving from the absent state** — a merge or sync of a branch that created and deleted a node the first
   parent never held, or a checkpoint import where a node was created and deleted between two checkpoints — is a valid
   diff (§10.1). [F06] has no stored op for it (`Delete` needs an existing node, and NF-1 forbids a `Create` and a
   `Delete` of one key), and [AR §5b.6] step 2 lists no import rule for an absent → tombstone file. Proposal: [F06] allows
   a `Delete` whose before-image is empty with a new flag, or a merge stores the tombstone as `Create` with a deleted
   image; [F14] adds the import rule; [F12] confirms the merge produces the tombstone. **Pass 1 (S1-6, A1-4): closed.**
   [F06 §7.4] adds op 16 `CreateDeleted` with NF-11; §10.1 and §13 map it; [F12 §7.8] states that merges emit it; the
   import rule is [F14 §11.2]'s.
6. **Key-class order** (§6.1). Class order follows the `.moi` groups (header `kind`, `status`, `parent`/`order`, then
   `field`, `incr`, `edge`/`anchor`, body). Header-carried fields, `label` lines and `conflict` lines sit elsewhere in a
   file, so an importer buffers one node, which [AR §4.6]'s "node by node" allows. `at` edges sort among edge kinds by
   the name `at` rather than after them (R-10: no new key class); [F14] may put `anchor` lines anywhere. Requests to
   [F14]: sort `field` lines by name bytes, because whole-line byte order differs when one name is a prefix of another
   followed by a digit (`field a0:` sorts before `field a:`).
7. **Schema entries after node entries** (§10.3), in the git tree order of `nodes/` before `schema/`. [AR §4.6]'s sort
   key starts with the uid and says nothing about uid-less schema items.
8. **The observation key** (class 5) holds only conflict values; while it does, the six member field keys are absent
   (§6.5), which is what N12 makes the image show. [F06 §6.1] has the class ("a conflict key only"); [F12] and [F14] need
   a key text for it (`observation` is proposed) — [AR §5b.2] rule 3's key list lacks one.
9. **Conflicts between [F06] and [F08] that the canonical form had to resolve** (both chapters should align at pass 1):
   - empty text and empty set: [F06 §5.2]–§5.3 make them values distinct from `absent`, [F08 §6.2] makes them absent; the
     canonical form follows [F08] (§7.1), the data-model owner;
   - `commit-ref`: [F06] stores 32 bytes, [F08 §5.1] 16; the canonical form hashes the full id ([F06] open point 19), which
     a 16-byte store cannot supply for a commit it does not hold;
   - `pathmove` class codes (0–3 in [F06 §5.5], 1–4 in [F08 §5.2]) and the anchor `kind`, `mode` and `watch` codes (0-based
     in [F06 §7.5.3], 1-based in [F08 §10.3]) differ; the canonical form uses names, so it is independent of both;
   - anchor `scope`: a string in [F06 §7.5.3], a binary value in [F08 §10.3.1]; the canonical form takes [F08]'s bytes,
     which are also `captured`'s input, and [F14] maps the image's text form to them;
   - presence of `hint` and `window`: flag-driven in [F06], kind-driven in [F08]; the block encodes presence, so either
     rule gives one encoding, but the two chapters must agree on which one the record follows;
   - `blob` with no content: allowed in [F06] (`algo` 0), refused in [F08] (`algo` ≠ `none`); the block encodes both.

   **Pass 1 (P1-1, S1-1, S1-3, A1-1, A1-2): closed.** [F08 §5] and [F08 §10] are the only stored forms and [F06] cites
   them: empty text and set are absent, `commitref` and `pinned_commit` are 32 bytes, the anchor record is [F08]'s with the
   binary scope and kind-driven presence, and `blob` may be empty for a planned target. The canonical form is unchanged.
10. **The tombstone's canonical state is the image's** (§6.4): title, reason, replaced_by and retained out-edges.
    [F08 §3.5] keeps status and the header enumerations in a tombstone's `NodeHdr`; they are store-local and unhashed, since
    no carrier holds them.
11. **Item 5 details** (§3.6). The algorithm enters only with a digest: an unborn repository's algorithm has no trailer
    carrier, so hashing it would demote every such commit on import. `git.branch` uses [F18 §3.2] rule 1's short form,
    and is empty when `HEAD` is detached, outside `refs/heads/` or not UTF-8. [F06 §4.4.6] should state the same when it
    interns the symbol. `git.worktree` is [OS/path §4.5]'s `c.text`.
12. **Node images hold no hierarchy or edge** (§7.4), mirroring [F06 §6.3]. A `DeleteVsModify` resolved towards the live
    side under `delete-wins` therefore cannot restore the node's parent or its non-retained out-edges. For [F06],
    [F12] and [RULES/merge-table] open point 5: add them to the snapshot, or record that `resolve` restores value keys only.
    **Pass 1 (S1-22): closed without a change here.** [F12 §6.5] states that a take towards a live side restores value
    keys from the image and the hierarchy key and out-edges from that side's state at the conflict's introducing commit,
    with ordinary `Move` and `AddEdge` ops in the `Resolve` commit. Images stay as they are, so no hashed byte changes.
13. **Counter deltas are sign and magnitude** (§7.5), because a delta between two `i64` totals (a deletion of a node whose
    counter is −2^63, for example) can exceed `i64`.
14. **Enumerations enter by name** (§2.2), settling [F01] open point 9 for the canonical form and [F19 §12.1]'s question:
    the conflict class enters by name. The names are frozen with their tables; a rename would be a new value.
15. **Message refusal codes.** §5.2 uses [F19]'s `bad_value` (exit 2); [F19] should add the cases `message-utf8`,
    `message-length` and `message-trailer` with texts. The whitespace set of §5.1 step 3 is ASCII HT, VT, FF and SP only;
    leading empty lines are kept; the `Moirai-` test is case-sensitive on the last paragraph's first line.
16. **`checkpoint` in item 1** (§4). [AR §4.6] item 1 and [AR §4.3]'s `kind` comment list five kinds; [AR §5b.4] gives
    import-checkpoint commits kind `checkpoint` "in the local record" and [F06 §3.1] stores it as 5. The canonical name is
    `checkpoint`, matching `Moirai-Kind`. [AR §4.6] item 1 should list it at WP-81a. §4 also reads the `Resolve` commits
    that `resolve` appends on a staging ref ([AR §5a.7] step 8) as kind `ordinary`, since [AR] names no kind for them;
    [F12] confirms.
17. **Foreign-commit details** (§12.3): a git commit with more than two parents stages `ImageParse` ([AR §5b.4] names
    kinds for one and two parents only); item 7 comes from the tree's `.moirai-image` marker; the author email is decoded
    as the message is. [F14] confirms.
18. **Item 7 carriers differ by import class** (§14.1): `Moirai-Schema` for native commits, the marker for foreign and
    checkpoint commits. [F14] checks that a native commit's trailer equals its tree's marker.
19. **`pathmove.hlc`** is hashed as data and compared with nothing (S-20, A-M2); a re-parent leaves it and
    `changeset_digest` unchanged (§3.12).
20. **Stale [60 §2.5] R-10 row** ([PLAN §3.3] last row): [60 §2.5]'s R-10 omits `end_h`; this chapter follows [40 §2.11]
    R-10 as revised by S-04.
21. **Anchor fields in the selector block** (§8.2): every field of the anchor record except the texts, the
    `text-unavailable` state, `aN` and the uid (which is the key). `occurrence` and `marker` are versioned selector data
    and so are hashed; [AR §5b.2] rule 9's `anchor` line lacks them, so [F14] (WP-15, whose [PLAN §3.3] gap names
    `occurrence`, `marker` and `v=`) must carry them, or the importer cannot rebuild item 10.
22. **Trailer and symbol strings** are single-line (symbols contain no LF, [F08 §5.3]); a value may still begin or end
    with a space, so [F14] must extract trailer values byte-exactly after `: `.
23. **Tombstone `field reason` and `field replaced_by` lines** carry the existence value (§14.2), while a live artifact
    has fields named `reason` and `replaced_by` ([F08 §9.2], §9.3); [F14] distinguishes them by the tombstone form
    (`deleted:` line).
24. **The entry count ends the digest input** (§10.4) and equals `Moirai-Ops`; streaming producers know it only at the
    end, which is where it is.
25. **One ordering rule inside values** (§2.4): the bytewise order of the encodings, so a text set's order compares the
    little-endian length bytes of `lp()` before the text, and an `int` set's order is not numeric. The order only has to
    be unique, and one rule for every element type is the simplest to implement identically in the engine and the model.
    The `.moi` text sorts sets by their text; the importer re-encodes and re-sorts.
26. **An empty body** (§6.3): the canonical body key follows the store's notion of "has a body". Proposal for [F08] and
    [F06]: writing an empty body removes the body, so that one empty value has one form, as for fields; otherwise [F14]
    must carry an empty body (`---` followed by one LF) distinctly from no body. **Pass 1 (S1-34): closed** by
    [F08 §7.2]: an empty body is no body, and `SetBody` to empty stores `new` absent.
27. **Proposed model functions** for `COVERAGE.md` (R-MODEL decides): `canon::commit_id`, `canon::changeset_digest`,
    `canon::state_diff`, `canon::normalise_message`, `canon::normalise_imported_message`, `canon::selector_block`,
    `canon::foreign_id`, `canon::checkpoint_id`; [F13]'s `canon::i28p_commit_id` and `canon::i38p_gate0` use them.
28. **Pass 1 changes** (P1-19, S1-5, A1-9, A1-19, A1-46, P1-5). §10.6 extends external sorting to every producer whose
    entries exceed `wmem`, merges and syncs included; §7.3 adds `prov` to existence conflicts and is the one equality rule
    [F12 §7.3] cites; §6.1's codes are also [F06 §6.1]'s `ckey` classes; §8.1's `pflags` takes [F08 §10.2]'s bit order;
    §3.4 cites the corrected HLC rule; `DATA` is removed from §2.2.
29. **Spec sync 2b.** (a) **Who recomputes commit ids** (§12): contested between dropping the oracle from the list,
    making commit-id correctness a C-rule, and requiring real ids in every fixture. The C-rule adopted ([F06 §4.3] order
    2), with the oracle recomputing ids over `canonical/` and `carrier/`, which carry real ones: a decoder cannot know a
    record's symbols and states, the hex fixtures' synthetic ids then are valid records, and nothing that checks real
    ids is lost. (b) A native import applies none of §5.2's refusals (§5.3), so a demoted commit re-exported natively
    verifies. (c) §9.1 and §9.7: the policy row class 6. (d) §11: the staged commit's `stage` group is not hashed.
