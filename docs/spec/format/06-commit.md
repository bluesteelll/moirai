# 06 — The commit record

| | |
|---|---|
| Title | The commit record: the payload of a `Commit` log record — the header with its presence bitmap and every field of [AR §4.3], the unhashed fields of [50] F10, F14 and F16 and [90 §10.1]'s `actor_src`; the commit-kind and provenance enumerations, import-checkpoint included; the closed value set with R-1's `path`, `oid` and `pathmove`; every op with its before-image and `prev` delta, R-4's edge property block and anchor record included; the bodies a commit carries; bulk commits through `cs.<n>` |
| Chapter | [F06], `docs/spec/format/06-commit.md` |
| Status | draft, pass 1 pending |
| Work package | WP-12a, the commit-record part of WP-12 ([PLAN §3.2] item 1), author role R-SPEC-F |
| Sources | [AR §4.3] (the `Commit` body, the op list and coalescing, "Encoding and size", bulk commits, bodies in the log tail); [AR §4.6] (items 1–10, "Not hashed", "Net changeset = state diff", the reservation tables for R4, R5, the audits and [90 §10.1]); [AR §4.5] steps 4, 7, 8 and 9 (candidate, re-parent, `#N` allocation, append); [AR §4.1] rows `cs.NNNN`, `blobs.NNNN` and rules; [AR §3.1] (field block and the closed type set; `CREATOR`); [AR §3.3] (edge properties, discriminator); [AR §5a.1] (commit content, schema version; ref fields); [AR §5a.2] (`ref_old`, `ref_seq`); [AR §5a.3] (sync residue); [AR §5a.5] (revert, cherry-pick, before-images); [AR §5a.7] steps 4, 7 and 8; [AR §5a.8] (conflict and violation classes); [AR §5b.4] (foreign and import-checkpoint ids, `hlc` rule, trailers, carrier table); [AR §5b.6] steps 2–5 (import flags, `verified`, stated parents); [AR §5b.7] (round-trip table: idempotency key hash); [AR §5d.1] (absorbed vectors); [AR §6.4] (idempotency, default keys); [40 §2.2] (artifact fields, `relink`), [40 §2.3], [40 §2.4] (`pathmove` and its `hlc`), [40 §2.7] (anchor record, `aN`, `captured`, `pred`), [40 §2.8], [40 §2.11] R-1, R-4, R-5, R-6, R-10; [50 §3.10] items 6, 8 and 11; [50 §4.4] (named-query definitions); [50 §8.1] F3, F10, F14, F15, F16, F18; [90 §4.1] (the actor row), [90 §4.2], [90 §10.1] (row "Commit header"); [60 §2.5] rows "Commit body" and "Ops and values", audit row "Commit body"; [PLAN §3.3] (the WP-12 gaps); reviews `a1-A.md` A-M2, `a1-S.md` S-03, S-04, S-09, S-19, S-20, `a1-P.md` A1P-12; [RULES/merge-table] open points 5, 9, 11, 18 and 20; [F17 §4.4] and OP-17-05; [OS/clock §7]; [LQ/canonical-ast §7.2] and its open points C-9, C-10 |
| Depends on | [F01], [F02]; cites [F04], [F05], [F07], [F08], [F09], [F10], [F11], [F12], [F13], [F14], [F16], [F17], [F18], [F19], [F20], [API], [LQ/canonical-ast], [LQ/envelope], [OS/clock], [RULES/merge-table], [RULES/link-merge-rules] |

## 1. Scope

This chapter specifies the **payload of a `Commit` log record**: every byte that follows the record's `RecHdr`. It owns:

- the commit-kind, import-provenance, statement-origin and actor-source enumerations (§3);
- the header part: the presence bitmap and every header field, hashed or not (§4);
- the rules for values inside a commit; their bytes are [F08 §5]'s, which this chapter cites and never restates (§5);
- the stored encoding of keys, key values, conflict values and node images (§6);
- every op of the changeset, its before-image, its `prev` delta, the net form and the op order (§7); the edge property
  block and the anchor record the edge ops carry are [F08 §10.2] and [F08 §10.3]'s;
- the bodies a commit record carries (§8);
- the `Commit` record of a bulk commit and what it requires of `cs.<n>` (§9).

It does not own: the `RecHdr`, record validity, groups, the chain trailer and the record-kind code of `Commit` ([F05]);
the canonical byte encoding, `commit_id` and `changeset_digest` ([F07]); the value type registry and every value's
stored bytes, the edge property block and the anchor record ([F08 §5], [F08 §10]); the kind, field, edge-kind, status,
resolution and schema-item enumerations and the schema-item layouts ([F08]); the layout of `cs.<n>` and of every segment section
([F09]); codec bytes and frames ([F10]); runtime tables ([F11]); the conflict- and violation-class enumeration and merge
semantics ([F12], [RULES/merge-table]); the write protocol, markers and recovery ([F16]); store parameters ([F17]).

## 2. Notation and shared rules

### 2.1 Types

Every type is [F01]'s: `u8`, `u16`, `u64` (§5.1), `uvar16`/`uvar32`/`uvar64` (§5.2), `svar64` (§5.3), `bool8` (§5.4),
`f64` (§5.5), `b16`, `b32` (§5.6), `vbytes`, `vstr` (§6.2); `tvalue` is §5.1's typed value in [F08 §5]'s encoding. A **`uvar32 ≤ 65,535`** is a `uvar32` whose value is at most
65,535; its bytes equal the `uvar16` encoding of the same value. **`oidv`** is [F01 §7.5]'s variable-width object id: an
`algo` byte (0 `none`, 1 `sha1`, 2 `sha256`) followed by exactly 0, 20 or 32 digest bytes. Every table below is a
sequence table ([F01 §2.6]): fields follow each other with no padding. The payload contains no fixed-size structure
larger than one field, so no offset table is needed.

### 2.2 Store-local numbers

- **`#N`** (a node id) is encoded `uvar32`, at least 1. The value 0 means "no node" only where a field says so
  ([F01 §5.8] rule as extended here: `replaced_by`, `parent`).
- **Symbol ids** follow [F01 §8]: each symbol field below names its class, and the class fixes the width.
- **`aN`** (an anchor handle, [40 §2.7], R-6) is encoded `uvar32`, at least 1.
- **`lsn`** is a log sequence number ([F05]). Stored lsns are absolute (`uvar64`), except `prev` (§7.3), which is a
  positive distance back from the record's own lsn.

None of these numbers is hashed or exported ([AR §4.6] "Not hashed").

### 2.3 Keys and owner nodes

A **key** is a key of canonical item 10 ([AR §4.6]) in store-local form (§6.1). The **owner** of a key is the node its
first component names: the node for existence, status, field, counter, observation, body and hierarchy keys; the source
node for an edge key; none for a schema key. The owner of an op is the owner of its key.

### 2.4 Two classes of rule

- **V-rules** (validity) are decidable from the record alone. A payload that breaks a V-rule is a malformed payload of a
  valid record, which [F05 §5.4] makes corrupt wherever it lies: exit 7 naming the extent and `moirai doctor --fsck`
  (pass 1, closure NC-6).
- **C-rules** (consistency) relate a record to the state it applies to (for example "`old` equals the key's value in the
  base state"). Writers must satisfy them. Replay applies each op's new value and does not test C-rules. `doctor --verify`,
  the format oracle's state checks, the reference model and the GT2/GT6 comparisons test them; a violation is a
  corruption finding (exit 7 from `doctor`), never a reason to stop replay.

Every rule below is tagged V or C.

## 3. Enumerations

All four are `u8` ([F01 §5.4]). Values not listed are reserved and invalid (V).

### 3.1 Commit kind (hashed: canonical item 1)

| value | name | meaning |
|---|---|---|
| 0 | `ordinary` | every local write that is not one of the kinds below; a foreign one-parent or root commit ([AR §5b.4]) |
| 1 | `merge` | a merge of src into dst ([AR §5a.7]); a foreign two-parent commit ([AR §5b.6] step 3) |
| 2 | `sync` | `merge main --into <lane>`: stores the residue only ([AR §5a.3]) |
| 3 | `revert` | the inverse of an origin commit ([AR §5a.5]) |
| 4 | `cherry-pick` | the three-way application of an origin commit ([AR §5a.5]) |
| 5 | `checkpoint` | a commit created by checkpoint-granularity import (`Moirai-Kind: checkpoint`, [AR §5b.4], N13a): the **import-checkpoint** kind |

The names are the frozen spellings of `Moirai-Kind` ([F14]) and of every text rendering. How the kind enters the
canonical bytes (number or name) is [F07]'s.

### 3.2 Import provenance (not hashed)

| value | name | meaning ([AR §4.3] `import`) |
|---|---|---|
| 0 | `local` | written by this store's own write path |
| 1 | `native` | imported with a `Moirai-Commit` trailer whose canonical hash verified (N5) |
| 2 | `foreign` | imported without a trailer, with an unknown trailer, or demoted after a hash mismatch |
| 3 | `checkpoint` | imported from a checkpoint commit (`Moirai-Kind: checkpoint`, no `Moirai-Commit`) |

### 3.3 Valid combinations (V)

| `import` | `kind` | `n_parents` | `foreign_git` (item 9) | `origin` (item 8) |
|---|---|---|---|---|
| `local` | `ordinary` | 0 (the store's root commit only, C) or 1 | absent | absent |
| `local` | `merge`, `sync` | 2 | absent | absent |
| `local` | `revert`, `cherry-pick` | 1 | absent | present |
| `native` | any of 0–5 | as for the same kind under `local`; `checkpoint` 0 or 1 | present iff the source commit carried `Moirai-Foreign-Git` | present iff kind is `revert` or `cherry-pick` |
| `foreign` | `ordinary` (0 or 1 parent), `merge` (2) | 0, 1 or 2 as the kind says | present | absent |
| `checkpoint` | `checkpoint` | 0 or 1 (the previous checkpoint of the ref) | present | absent |

A `native` import keeps the original kind, so a checkpoint commit that another store re-exported natively ([AR §5b.4]
row 9) arrives as `native` with kind `checkpoint`.

### 3.4 Statement origin: `stmt_origin` ([50] F10; not hashed)

| value | name | the commit was produced by | `stmt_sym` (class `stmt`) | `stmt_hash` |
|---|---|---|---|---|
| 0 | `verb` | a CLI write verb | the named mutation the verb compiles to (`tx.claim`); for a verb that compiles to no `TX` block (`revert`, `cherry-pick`, `migrate`), the verb's name | present iff the verb compiled to a `TX` block |
| 1 | `named-mutation` | a named mutation invoked by name: a `TX` block that is one `CALL tx.<name>(…)`, or MCP `write` with `name` | the mutation's name | present |
| 2 | `tx` | a free `TX` block through `moirai tx` or `apply` | 0, or the symbol `apply` for an `apply` batch | present |
| 3 | `mcp-write` | MCP `write` with free `TX` text | 0 | present |
| 4 | `merge` | `merge`, `sync`, `merge --continue` (the landing or staged commit) | 0 | absent |
| 5 | `import` | `image import` (every `import ≠ local` commit, and only those, V) | 0 | absent |
| 6 | `file-verb` | a file verb that moves project files: `file mv`, `file rm`, `file add` | the verb word (`mv`, `rm`, `add`) | absent |

`stmt_hash` is [LQ/canonical-ast §7.2]'s `H` of the `TX` root the commit came from, parameter values substituted. The
rendering (`via tx.complete`, `via mcp.write`, `via file mv`) is [LQ/envelope §5.9]'s.

### 3.5 Actor source: `actor_src` ([90 §4.1], [90 §4.2], [90 §10.1]; not hashed)

| value | name | the actor came from |
|---|---|---|
| 0 | `none` | nothing named the agent (the actor is `session:<harness>:<id>`, or empty); also every `import ≠ local` commit, whose actor comes from the imported commit, not from a caller context (C) |
| 1 | `lease` | the presented lease's holder |
| 2 | `meta` | Codex `_meta.threadId` (`codex:<threadId>`) |
| 3 | `stamp` | the Claude stamp's `agent_id` |
| 4 | `declared` | a declared `--agent`/`agent` |
| 5 | `env` | the environment: `MOIRAI_AGENT`, then `CODEX_THREAD_ID`, `CLAUDE_CODE_SESSION_ID` |
| 6 | `client` | MCP `clientInfo` |

The value is the row of [90 §4.1]'s actor order that supplied the actor. `show --provenance` prints it; no other output
does ([90 §4.2]).

## 4. The `Commit` payload

### 4.1 Parts

The payload is the byte string P that [F05] assigns to the record (its `RecHdr.len` minus the header and any padding
[F05] defines). It consists of the **header part** (§4.3, orders 1–39) followed by the **changeset part** (§4.3, orders
40–44) and, on a staged commit only, the **staging arguments** (order 45, §4.4.16). The fields fill P exactly (V): a decoder that ends before the end of P, or needs a byte beyond it, finds the
record invalid (a V-rule break, with §2.4's consequence).

The changeset part is the quantity `cs_bytes` of [F17 §4.4]: its byte length, from the first byte of `n_ops` to the end of
order 44 (the end of P when bit `stage` is clear).

### 4.2 Presence bitmap

`presence` is a `u32` (little-endian, fixed width: the design's "u32 presence bitmap"). A clear bit means the group is
absent and takes no bytes.

| bit | name | group present when (C unless marked V) |
|---|---|---|
| 0 | `ref_old` | the ref had a tip before this commit (every commit except the first commit of a ref that had no tip) |
| 1 | `prev_on_ref` | the ref already holds a commit before this one |
| 2 | `stated` | some parent's stated id differs from the id of the actual parent record (a demoted parent, N5) |
| 3 | `git` | the command ran inside a git repository, i.e. some part of canonical item 5 is non-empty |
| 4 | `foreign_git` | canonical item 9 is non-empty (§3.3, V) |
| 5 | `origin` | kind is `revert` or `cherry-pick` (V) |
| 6 | `idem` | `import = local` and the write carried an idempotency key, explicit or default ([AR §6.4]) |
| 7 | `sync_base` | kind is `merge` or `sync` (V) |
| 8 | `absorbed` | kind is `merge` or `sync` and the absorbed vector, without the commit's own ref, is non-empty; only for those kinds (V) |
| 9 | `verified` | `import = native`, or `import = foreign` after a demotion (§4.4.10); never for other imports (V) |
| 10 | `ckpt` | kind is `checkpoint` (V) |
| 11 | `xtr` | `import` is `native` or `checkpoint` and the source commit carried `Moirai-Ref` or `Moirai-Idem`; never for `local` or `foreign` (V) |
| 12 | `stmt_hash` | as §3.4's last column |
| 13 | `msg` | the message is non-empty |
| 14 | `affected` | `affected` is non-empty or `affected_complete = 0` |
| 15 | `cs_ref` | the commit is a bulk commit (§9) |
| 16 | `pruned` | the record is the header-only form of a commit that `gc` dropped (§4.4.15); only in a rewritten `hist` file (C); then `n_ops` = `n_bodies` = 0 and bits 15 and 17 are clear (V) |
| 17 | `ckimg` | kind is `checkpoint`, the commit is inline, and its checkpoint tree holds at least one node file that differs from its parent checkpoint's tree: the image-only data of those node files follows (§4.4.14) (C); only with kind `checkpoint` and never with bit 15 (V). An inline checkpoint whose tree differs in no node file leaves the bit clear. A bulk checkpoint keeps the same data in its `cs.<n>` under the same condition (§9 BK-5, [F09 §16.4] `CKIMG`) |
| 18 | `stage` | the commit is a staged `merge` or `sync` on a `merge/*` staging ref ([F12 §9.2], §9.4) and its command had a `--base`, a `--policy` or an effective `strict` of true (§4.4.16) (C); only with kind `merge` or `sync` and never with bit 16 (V); only on a ref of kind `merge` ([F05 §9.10] `rkind` 3) (C) |
| 19–31 | — | reserved-zero ([F01 §10]) (V) |

### 4.3 Sequence

The "hashed" note names the canonical item ([AR §4.6]) a field feeds; every other field is not hashed.

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `presence` | `u32` | always | §4.2 |
| 2 | `commit_id` | `b32` | always | BLAKE3-256 of the canonical form ([F07]) (C: it relates the record to the symbols and states it names, so a decoder never recomputes it; `doctor --verify`, the model and the oracle over fixtures that carry real ids do; spec sync 2b) |
| 3 | `n_parents` | `u8` | always | 0, 1 or 2 (§3.3, V) |
| 4 | `parents` | `n_parents` × (`id16` `b16`, `lsn` `uvar64`) | always | the **actual** parents, first = dst or lane tip, second = src tip or `sync_base` ([AR §5b.4]): the first 16 bytes of each parent's `commit_id` and the lsn of its `Commit` record |
| 5 | `stated_mask` | `u8` | bit `stated` | bit i set ⇔ parent i has a stated id below; bits ≥ `n_parents` reserved-zero; not 0 (V) |
| 6 | `stated_ids` | one `b32` per set bit of `stated_mask`, in bit order | bit `stated` | the parent's **stated** id (hashed: item 2); a parent without a set bit has as stated id the `commit_id` of its actual record |
| 7 | `gen` | `uvar32` | always | `1 + max(parent.gen)`, 1 for a root commit ([AR §5a.1]), over the actual parents |
| 8 | `seq` | `uvar64` | always | store-wide monotonic commit sequence number, at most 2^32 − 1 (§4.4.3) |
| 9 | `ref` | `uvar32` | always | symbol, class `ref`: the name of the ref this commit lands on |
| 10 | `ref_id` | `uvar32` | always | the never-reused id of that ref ([F11]) |
| 11 | `ref_old` | `b16` | bit `ref_old` | `id16` of the tip this commit replaces (N3): adopting the record implies the ref move `ref_old → commit_id`, CAS-checked against the ref table ([F16]) |
| 12 | `prev_on_ref` | `uvar64` | bit `prev_on_ref` | lsn of the previous `Commit` record with the same `ref_id` (G15) |
| 13 | `ref_seq` | `uvar32` | always | position on the ref's chain, at least 1, never reused after `undo` (CM1) |
| 14 | `kind` | `u8` | always | §3.1 (hashed: item 1) |
| 15 | `import` | `u8` | always | §3.2 |
| 16 | `hlc` | `u64` | always | hybrid logical clock value ([F01 §5.7]) (hashed: item 3); §4.4.4 |
| 17 | `actor` | `uvar32` | always | symbol, class `actor` (hashed as its string: item 4); id 0 = empty |
| 18 | `role` | `uvar32 ≤ 65,535` | always | symbol, class `role` (item 4) |
| 19 | `session` | `uvar32` | always | symbol, class `session` (item 4) |
| 20 | `schema_version` | `uvar32` | always | the schema version of the ref's state at this commit, after its own `Schema` ops ([F08]) (hashed: item 7) |
| 21 | `git` | §4.4.6 | bit `git` | git provenance (hashed: item 5) |
| 22 | `foreign_git` | `oidv`, `algo` 1 or 2 | bit `foreign_git` | the git object the commit came from (hashed: item 9) |
| 23 | `origin` | `b32` | bit `origin` | the full id of the commit reverted or cherry-picked (hashed: item 8) |
| 24 | `idem_key` | `b16` | bit `idem` | §4.4.7 |
| 25 | `idem_payload` | `b16` | bit `idem` | §4.4.7 |
| 26 | `sync_base` | `b16` | bit `sync_base` | `id16` of the src commit absorbed (G17); equals `parents[1].id16` (V) |
| 27 | `absorbed` | §4.4.9 | bit `absorbed` | dst's absorbed vector after this commit (CM1) |
| 28 | `verified` | `bool8` | bit `verified` | §4.4.10 |
| 29 | `ckpt` | §4.4.11 | bit `ckpt` | the checkpoint's origin trailers |
| 30 | `xtr` | §4.4.12 | bit `xtr` | the imported commit's informational trailers |
| 31 | `stmt_origin` | `u8` | always | §3.4 |
| 32 | `actor_src` | `u8` | always | §3.5 |
| 33 | `stmt_sym` | `uvar32` | always | symbol, class `stmt` (§3.4); 0 = none |
| 34 | `stmt_hash` | `b16` | bit `stmt_hash` | §3.4 |
| 35 | `append_delta` | `svar64` | always | `append_hlc − hlc` (§4.4.5) |
| 36 | `msg` | `vstr`, length 1 to 65,535 | bit `msg` | the message, normalised at write time ([F07]; hashed: item 6) |
| 37 | `affected` | §4.4.13 | bit `affected` | the change-feed set and its completeness flag ([50] F15, F16) |
| 38 | `changeset_digest` | `b32` | always | BLAKE3-256 of canonical item 10 ([F07]) (item 10 enters `commit_id` through it) (C, as order 2) |
| 39 | `cs_ref` | (`file` `uvar32`, `len` `uvar64`, `b3` `b16`) | bit `cs_ref` | the sealed changeset file of a bulk commit (§9) |
| 40 | `n_ops` | `uvar32` | always | number of ops; 0 when bit `cs_ref` or `pruned` is set (V) |
| 41 | `ops` | `n_ops` × op (§7) | always | the stored changeset in net form (§7.8) and op order (§7.9) |
| 42 | `n_bodies` | `uvar32` | always | number of carried bodies; 0 when bit `cs_ref` or `pruned` is set (V) |
| 43 | `bodies` | `n_bodies` × body entry (§8) | always | the bodies this commit carries |
| 44 | `ckimg` | §4.4.14 | bit `ckimg` | an import-checkpoint's image-only data (not hashed) |
| 45 | `stage` | §4.4.16 | bit `stage` | the staged merge's own arguments, which `merge --continue` re-uses (not hashed) |

### 4.4 Field rules

#### 4.4.1 Parents and stated parents

- The actual parents are records this store holds: each `parents[i].lsn` is the lsn of a valid `Commit` record whose
  `commit_id` begins with `parents[i].id16` (C). Parents always precede the child in the log (C).
- The canonical form hashes the **stated** ids ([AR §4.6] item 2). A stated id differs from the actual parent's id only
  when the parent was demoted at import: the child verifies against the id its trailer names while its actual parent is
  the demoted commit (`parent_actual`, [AR §5b.4], N5, I29′). Only `native` imports use the `stated` group (C).

#### 4.4.2 Ref fields

- `ref`, `ref_id`, `ref_old`, `prev_on_ref` and `ref_seq` are store-local and not hashed. A re-parent under the writer
  byte ([AR §4.5] step 7) rewrites `ref_old`, `prev_on_ref`, `ref_seq`, `seq`, `hlc`, `commit_id`, `append_delta` and, when
  the parent moves, `parents` and `gen`; it never changes `changeset_digest` (§5.5). The changeset part's bytes are
  serialised under the writer byte once the record's lsn and the allocated `#N`s are known (§7.3, open point 16).
- `ref_seq` is allocated from the ref's `ref_seq_next` ([F11]); the first commit of a ref has `ref_seq` ≥ 1, so the
  absorbed value 0 means "nothing absorbed" (C).
- A commit whose implied ref move fails its CAS is parked on `orphans/<ref>` ([AR §3.4] I27′); its record is unchanged.

#### 4.4.3 `seq`

`seq` is `uvar64` as [AR §4.3] types it, but a writer never writes a value above 2^32 − 1: the commit that would need one
is refused with exit 7 and nothing is written ([AR §4.5] step 4; the error code is [F19]'s). A decoder treats a larger
value as invalid (V).

#### 4.4.4 `hlc`, and the foreign-commit `hlc` unit

- A **local** commit's `hlc` is assigned under the writer byte at the step [F16] P-36 names: the next value of the store's
  one HLC sequence ([OS/clock §7] `hlc_next`), which only the semantic durable records advance and which is never below
  the `hlc` of any commit the store holds, an imported one included ([API §6.2] CK-4; pass 1, P1-5, S1-13, A1-17). A
  `Checkpoint`, `Reserve`, `Lazy`, `SessionMark` or runtime record never advances it, so maintenance timing never
  changes a commit id ([F17 §1.5] SP-1). The value is also the commit's `append_hlc`. The engine keeps the two maxima
  the rule needs, `hlc_seq` and `hlc_commit`, in `HEAD` ([F04 §5.15]) and carries them across an epoch re-roll in the
  extent head ([F05 §9.28]).
- A **native** import keeps the `hlc` of its `Moirai-Hlc` trailer.
- A **foreign** or **import-checkpoint** commit gets the deterministic value ([AR §5b.4], N13c), with the unit gap of
  [PLAN §3.3] closed as follows:

  ```
  hlc = max( (T × 1000) << 16 ,  max over its parents p of (p.hlc + 1) )
  ```

  where T is the git commit's **committer** timestamp in **seconds** since the Unix epoch (the zone offset is ignored),
  so `T × 1000` is milliseconds, the unit of `hlc`'s bits 16–63 ([F01 §5.7]); the second term is omitted for a commit with
  no parents; `p.hlc` is the `hlc` of each actual parent as imported. [AR §5b.4]'s `committer_time << 16` omitted the
  factor 1000.
- An imported commit whose T is negative, or whose `T × 1000` is 2^48 or more, or whose second term would exceed
  2^64 − 1, is an `ImageParse` violation ([AR §5a.8]), staged like every import violation ([AR §5b.6] step 4).
- *(Informative)* Export writes the author and committer time `floor((hlc >> 16) / 1000)` seconds ([AR §5b.4]), so a
  foreign commit whose first term dominates re-exports with its original T.

#### 4.4.5 `append_hlc` ([50] F14)

`append_hlc` = `hlc + append_delta`, computed in wrapping-free integer arithmetic; a result outside [0, 2^64 − 1] is
invalid (V). It is this store's HLC when the record was appended: the next value of the sequence of §4.4.4 ([F16] P-36,
[API §6.2] CK-4), so it is strictly increasing in `seq` order (I43′, checked on append by [F16]).

- For `import = local`, `append_delta` = 0: one byte (V).
- For an import it is usually positive; it is signed because an imported `hlc` from a clock ahead of this store's may
  exceed this store's HLC ([AR §4.3]: "a varint delta from `hlc`"; zigzag keeps 0 at one byte).

#### 4.4.6 `git`: provenance group (canonical item 5)

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `algo` | `u8` | always | the repository's object format ([F01 §7.5]): 1 `sha1`, 2 `sha256`; 0 only when neither `head` nor `base` is present (V) |
| 2 | `gflags` | `u8` | always | bit 0 `head` present, bit 1 `base` present; bits 2–7 reserved-zero (V) |
| 3 | `head` | digest, 20 or 32 bytes by `algo` | `gflags` bit 0 | the git `HEAD` commit of the caller's tree |
| 4 | `branch` | `uvar32` | always | symbol, class `git-branch`; 0 = empty |
| 5 | `worktree` | `uvar32` | always | symbol, class `git-worktree`; 0 = empty |
| 6 | `base` | digest, 20 or 32 bytes by `algo` | `gflags` bit 1 | the lane's base commit (`Moirai-Git-Base`) |

The group is present iff at least one of `algo`, `head`, `branch`, `worktree`, `base` is non-empty (C). An unborn
repository gives `algo` set with neither digest.

#### 4.4.7 The idempotency pair

- `idem_key` = BLAKE3-128 of the key, framed for domain separation ([F01 §7.3]):
  - an explicit key k (its UTF-8 bytes): `BLAKE3-128( lp("moirai-idem-key-v1") ‖ lp(k) )`;
  - a default key ([AR §6.4]): `BLAKE3-128( lp("moirai-idem-default-v1") ‖ lp(s) ‖ lp(a) ‖ lp(b) ‖ lp(H) )`, where s is
    the namespaced session `<harness>:<id>` (empty when none), a is the attested thread or agent (`codex:<threadId>`,
    `claude:<agent_id>`) where one exists and otherwise the resolved actor's string, b is the name of the branch the
    command writes ([API §7.2], §7.4; spec sync 2b), and H is [LQ/canonical-ast §7.2]'s 16-byte hash of the `TX` root
    (or [API §7.3]'s payload).
- `idem_payload` = H, the canonical bound AST hash of the payload ([LQ/canonical-ast §7.2]); for a verb that compiles to
  no `TX` block, the payload hash [API] defines for that verb.
- Only `import = local` commits carry the group (§4.2), and only they satisfy idempotency lookups ([F16]); an imported
  commit's `Moirai-Idem` goes to `xtr` (§4.4.12).

#### 4.4.8 `sync_base`

For `merge` and `sync` commits it repeats the second parent's `id16` ([AR §4.3] "sync/merge only: the src commit
absorbed"); a sync expands `main`'s window by reference up to it ([AR §5a.3]).

#### 4.4.9 `absorbed`: the absorbed vector

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `n_absorbed` | `uvar32 ≤ 65,535` | always | number of entries, at least 1 (V) |
| 2 | entries | `n_absorbed` × (`ref_id` `uvar32`, `ref_seq` `uvar32`) | always | sorted by `ref_id` ascending, no duplicate (V); every `ref_seq` ≥ 1 (V) |

The vector is dst's after this commit ([AR §5d.1]): `absorbed_dst[src] = ref_seq(tip src)`, every other entry the maximum
of both sides. It excludes the entry of the commit's own `ref_id`, which is the record's `ref_seq` (V: no entry equals
`ref_id`), and entries of deleted refs (C).

#### 4.4.10 `verified`

`1` for `import = native` (the canonical hash matched `Moirai-Commit`); `0` for a `foreign` commit that carried a
`Moirai-Commit` trailer which did not verify (a demotion, N5); absent otherwise (V: value 1 only with `native`, 0 only
with `foreign`).

#### 4.4.11 `ckpt`: checkpoint origin

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `head` | `b32` | always | `Moirai-Head`: the head moirai commit whose state the checkpoint carries |
| 2 | `n_folded` | `uvar32` | always | the `<n>` of `Moirai-Folded` |
| 3 | `first` | `b32` | always | the first folded commit (`from c…`); all zero exactly when `n_folded` = 0 (V) |
| 4 | `last` | `b32` | always | the last folded commit (`to c…`); all zero exactly when `n_folded` = 0 (V) |

Not hashed. Kept so that `show` and a re-export can reproduce the checkpoint's trailers ([AR §5b.4]). `n_folded` = 0 is a
checkpoint that folds nothing (after an `undo`, `Moirai-Folded: 0` with no `from … to …`, [F14 §10.7]; pass 1, S1-23).

#### 4.4.12 `xtr`: informational trailers of an imported commit

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `xflags` | `u8` | always | bit 0 `Moirai-Ref` present, bit 1 `Moirai-Idem` present; bits 2–7 reserved-zero; not 0 (V) |
| 2 | `x_ref` | `uvar32` | `xflags` bit 0 | symbol, class `ref`: the `Moirai-Ref` value as imported |
| 3 | `x_idem` | `b16` | `xflags` bit 1 | the `Moirai-Idem` value as imported |

Not hashed. A re-export of the imported commit writes these values, so gate 1's byte-identical round trip holds for
commits whose landing ref differs from the exporter's ([AR §5b.7], [F14]).

#### 4.4.13 `affected` ([50] F15, F16; I42′)

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `affected_len` | `uvar32` | always | number of ids ([50] F16: u32) |
| 2 | `affected_complete` | `bool8` | always | 1: the list is [F13 §6.3]'s `D(c)`; 0: the list is incomplete ([F13 §6.3], [F17 §8.2]) |
| 3 | `first` | `uvar32` | `affected_len` ≥ 1 | the least id |
| 4 | `gaps` | (`affected_len` − 1) × `uvar32` | `affected_len` ≥ 2 | each id minus the previous one, at least 1 |

The set is stored ascending by `#N` with no duplicate (V: every gap ≥ 1). An absent group means the empty set, complete.
The semantics of the set are [F13 §6.3]'s.

#### 4.4.14 `ckimg`: image-only data of an import-checkpoint ([F14 §11.3]; pass 1, S1-23, A1-10)

A checkpoint tree carries, per node file, provenance lines and ledger lines that no canonical item holds ([F14 §6.3],
[F14 §6.5]). A re-export must reproduce them byte for byte (gate 2, [AR §5b.7]), so the importing commit keeps them, not
hashed:

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `n_files` | `uvar32` | always | number of entries, at least 1 (V) |
| 2 | entries | `n_files` × entry | always | sorted by `id` ascending, no duplicate (V) |

An entry:

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `id` | `uvar32` | always | the `#N` of the node the file describes |
| 2 | `iflags` | `u8` | always | bit 0 `created`, bit 1 `updated`, bit 2 `deleted` present; bits 3–7 reserved-zero; not 0 (V) |
| 3 | `created` | prov | `iflags` bit 0 | the file's `created:` value |
| 4 | `updated` | prov | `iflags` bit 1 | the file's `updated:` value |
| 5 | `deleted` | prov | `iflags` bit 2 | the file's `deleted:` value; only without bits 0 and 1 (V: a tombstone file has neither, [F14 §6.10]) |
| 6 | `n_ledger` | `uvar32` | always | number of ledger lines of the file |
| 7 | ledger | `n_ledger` × (`field` `uvar32`, `delta` `svar64`, `token` `vstr`) | always | the file's `incr` lines in file order ([F14 §6.5]): `field` a symbol of class `name`, `delta` ≠ 0 (V), `token` the `ledger-token` as written, at least 1 byte (V) |

`prov` is (`commit` `b32`, `time` `vstr`): the commit id of the `prov` rule of [F14 §6.3] and its `rfc3339ms` text exactly
as written, empty when the line had none. The group lists exactly the node files the checkpoint tree holds that differ
from its parent checkpoint's tree (C). An exporter that re-exports this commit's tree writes these values instead of
deriving them from its own history ([F14 §11.3]).

#### 4.4.15 `pruned`: the header-only form (pass 1, S1-21, A1-5)

`gc` drops unreachable commits older than `gc.cruft-delay` and, unless `--prune-headers` is given, keeps each one's header
in its rewritten `hist` file ([AR §4.9], [F10 §4.6]). The kept record is the **header-only form** of the dropped `Commit`
payload:

- presence bit `pruned` is set; bits `cs_ref`, `ckimg` and `stage` are clear; `n_ops` = 0, `n_bodies` = 0, and nothing follows
  `bodies` (V);
- every other field is the dropped record's, byte for byte: `commit_id`, `changeset_digest`, the parents, `hlc`, the
  message, `affected` and the rest. So `commit_id` still verifies against [F07 §3.1], and a pruned commit stays
  distinguishable from an empty commit (whose `changeset_digest` is the digest of zero entries, [F07 §10.4]);
- the form occurs only in `hist` files, never in the log (C); [F10 §4.6] recomputes `RecHdr.len` and the checksum.

A pruned commit has no changeset. `show` prints its header with the line `changes pruned by gc`; `revert`, `cherry-pick`,
`diff` against it and `history --patch` of it are refused with `commit_pruned` (exit 3, [F19 §10.2]). Because only
unreachable commits are pruned, no ref's history walk and no per-node `prev` chain of a live ref reaches one.

#### 4.4.16 `stage`: a staged merge's arguments (spec sync 2b)

A staged commit ([F12 §9.2]) records the arguments of the command that computed it, so that `merge --continue` recomputes
the same operation ([F12 §9.4] step 1). The group is:

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `sgflags` | `u8` | always | bit 0 `strict` (the command's effective `strict`: `--strict`, else `merge.strict`, [CFG]); bits 1–2 the policy override (0 none, 1 `delete-wins`, 2 `resurrect`; 3 invalid, V); bit 3 `base` present; bits 4–7 reserved-zero; not 0 (V) |
| 2 | `base` | `b32` | `sgflags` bit 3 | the full id of the `--base` commit ([F12 §4.3] rule 1) |

A staged `merge` or `sync` without the group was computed with no `--base`, no policy override and `strict` false. A
staged step-0 sync ([F12 §9.6]) records the arguments that applied to the sync: the merge's policy override and effective
`strict`, which step 0 applies as the merge does, never the merge's `--base`, which step 0 does not apply (it merges over
[F12 §4.3]'s base, [API §11.7]). A re-staged commit ([F12 §9.4] step 4) copies the group of the commit it replaces,
byte for byte. A staged `revert` or `cherry-pick` never carries it: those commands take no `--base`, `--policy` or
`--strict` ([API §11.10]). The group is store-local: staged commits are never exported ([AR §5b.4]) and the group is
not hashed (§4.5).

### 4.5 Hashed and unhashed fields

| Canonical item ([AR §4.6]) | Field(s) of this record |
|---|---|
| 1 kind | `kind` |
| 2 parents' stated ids | `stated_ids`, else the actual parents' `commit_id` (§4.4.1) |
| 3 `hlc` | `hlc` |
| 4 actor, role, session | the strings of `actor`, `role`, `session` |
| 5 git provenance | `git` |
| 6 message | `msg` |
| 7 schema version | `schema_version` |
| 8 origin | `origin` |
| 9 `foreign_git` | `foreign_git` |
| 10 net changeset | `changeset_digest` (over the ops, or over `cs.<n>` for a bulk commit) |

Every other field is **not hashed**, exactly [AR §4.6]'s list: `lsn` and `parents[].lsn`, `seq`, `gen`, `ref`, `ref_id`,
`ref_old`, `prev_on_ref`, `ref_seq`, symbol numbers, `import`, `verified`, the idempotency pair, `absorbed`, `affected`
and `affected_complete`, `stmt_origin`, `stmt_sym`, `stmt_hash`, `append_hlc`, `actor_src`, `ckpt`, `xtr`, `cs_ref`,
`ckimg`, `stage`, the `pruned` bit, and,
inside the changeset part, every `#N`, `aN`, `prev`, before-image, `Violation` op, `creator` of a `Create`, and anchor
quote, prefix, suffix and `end` text (their digests are hashed, R-10).

### 4.6 Size and the one-extent rule

- **Header part bound.** With every group present, the header part is at most
  `659 + 10·A + M + 5·F` bytes, where A is `n_absorbed`, M the message length and F `affected_len` (each varint at its
  bound's maximum length, [F01 §5.2]; groups that cannot co-occur are counted anyway). It is the largest non-changeset
  part of a `Commit` record ([F17] OP-17-05). The changeset part is `cs_bytes`. The staging arguments (order 45) add at
  most 33 bytes.
- *(Informative)* A typical local agent commit — one parent, git provenance with SHA-1, a key, a named mutation, 2–4
  affected ids, a 20-byte message and one small op — has a header part of ≈ 230–250 B; §11 shows one without git
  (194 B).
- **W3's writer check.** Before it appends, the writer computes the length G of the whole group (every record with its
  `RecHdr` and [F05]'s padding, the chain trailer included). If G exceeds the usable length of an extent (the extent
  length `store.log-extent-bytes` minus [F05]'s rotation reserve), then:
  1. a bulk-class verb ([F17 §4.4] W1) whose commit is inline switches to a bulk commit (§9), which moves the changeset
     part into `cs.<n>`;
  2. otherwise, or if G still exceeds the usable length, the command is refused with exit 7 and nothing is written
     (error code: [F19]; open point 17).

  The recovery scan treats a record longer than an extent as invalid ([AR §4.3], [F05]).

## 5. Values

### 5.1 Typed values

A **typed value** (`tvalue`) is a value in the stored encoding of [F08 §5.1]–§5.2: [F08]'s type byte followed by its
type's value bytes. [F08 §5] is the only definition of value bytes in the store (pass 1, P1-1, S1-1, A1-1): this chapter
defines no tag, no set layout and no value order of its own, and every value an op, a key value, a conflict side or a node
image holds is those bytes. In the value positions this chapter names (an op's `old` and `new`, a `kval`, the sides of a
conflict value), the type byte `00` (`absent`, [F08 §5.1]) stands for no value; a node-image entry never holds it (§6.3).

*(Informative)* `false` is `01`, `true` `81`, the int 3 `02 06`, the text `ab` `06 02 61 62`, a `ref` to #12
`09 0C 00 00 00`, and absent `00`.

### 5.2 Value rules

- **One type per field** (C). The schema gives each field one type ([F08]); a value of another type is refused at write.
  `text` and `sym` are two storage forms of one type: equal strings are equal values ([F07] compares strings). Which
  fields a writer interns is [F08]'s.
- **Empty is absent** ([F08 §5.3]; V where decidable). No value position holds the empty text, a `sym` 0, an empty set or
  an `oid` of algorithm `none`: it holds `absent`. A `SetField` that empties a field has `new` = `absent`; a `SetBody`
  that empties a body has `bflags` bit 1 clear ([F08 §7.2]).
- **`f64`** (V). [F08 §5.3]'s invalid patterns (NaN, ±infinity, −0.0) make the record invalid. Writers refuse NaN and
  infinities at write (exit 2, [F19] `bad_value`) and store −0.0 as +0.0, which settles [F01] open point 8 for stored
  values; [F07] applies the same rule to canonical values.
- **Counters** (C). A counter-typed field changes only through `Incr` and appears as `counter` values only in node images
  (§6.3) and conflict sides; `SetField` never carries type `counter`.
- **Ref** (V). A `ref` is at least 1. A reference to a uid the store has seen but that names no live node is still a
  `#N` (`UIDX`, [F11]); [F08] maps uid-valued fields such as `origin_pred` to `ref` (open point 11).
- **No list, struct or uid type.** The closed set has none; [F08] maps every [AR §3.2] list or struct field into it
  ([RULES/merge-table] open point 11; open point 11 here).

### 5.3 Sets

A set value is [F08 §5.2]'s `set` (element type byte, `n` ≥ 1, the elements strictly ascending in [F08 §5.5]'s stored
order). The empty set is `absent` (§5.2). [F07] sorts set elements canonically.

### 5.4 Reading a value

A decoder reads the type byte, then exactly the value bytes [F08 §5.2] gives it. A varint, UTF-8, length or pattern
violation inside a value is a V-rule failure of the whole record ([F01 §5.2], [F01 §6.1], [F08 §5.3]).

### 5.5 R-1: `path`, `oid`, `pathmove`

The bytes of `path`, `oid` and `pathmove` are [F08 §5.2]'s (`pathmove` classes 1 `explicit` to 4 `observed`, 0
invalid). This chapter fixes two rules of their use in commits:

- **Roots.** The `path` root id is store-local. [F07] and [F14] encode the root **by name** (R-1 as revised by S-19); an
  artifact's `path` and `origin_path` roots equal its `root` field (I-F8, C).

- **`pathmove.hlc`** ([40 §2.4] as revised for `a1-A.md` A-M2 and `a1-S.md` S-20). The value is fixed when the candidate
  is computed in phase 1 ([AR §4.5] step 4), from the writer's HLC under [OS/clock §7]'s rule. A re-parent under the writer
  byte never changes it, so `changeset_digest` stays valid and the re-parent's re-hash stays O(1). Nothing compares it
  with the adding commit's header `hlc`: not the importer, `doctor --verify` or the reference model. It only orders
  entries, identically in every store. Under the Store API's injected clock the model derives the same value
  ([API §6.2] CK-5).

## 6. Keys, key values and node images

### 6.1 Key classes and `ckey`

A `ckey` is a key in store-local form: a class byte and the key's components.

| value | class | components after the class byte | canonical key ([AR §4.6] item 10) |
|---|---|---|---|
| 1 | `existence` | `node` `uvar32` | `(uid)` → created / deleted / undeleted |
| 2 | `status` | `node` | `(uid, status)` with its resolution |
| 3 | `hierarchy` | `node` | `(uid)` → (parent uid, order) |
| 4 | `field` | `node`, `name` `uvar32` (symbol, class `name`) | `(uid, field)` |
| 5 | `observation` | `node` | the six observation fields of an `artifact` as one merge key ([40 §2.2]); a conflict key only |
| 6 | `counter` | `node`, `name` | `(uid, field)` of a counter field |
| 7 | `edge` | `src` `uvar32`, `ekind` `u8`, `dst` `uvar32`, `dflag` `u8`, `disc` `b16` if `dflag` bit 0 | `(uid, kind, dst uid, disc)` |
| 8 | `body` | `node` | `(uid, body)` |
| 9 | `schema` | `item_class` `u8`, `item_key` `vbytes` | a schema item; `item_class` is [F08 §8.5]'s class (1–6) and `item_key` its stored key form ([F08 §8.5]: the component names joined by `00`) |

0 and 10–255 are invalid (V). `ekind` is [F08]'s edge-kind code. `dflag` bit 0 marks a discriminator; bits 1–7 are
reserved-zero (V). The discriminator is present exactly on `at` edges ([AR §3.3], [40] R-4; C), and is the anchor uid.
The class value is also the op's rank in the op order (§7.9). Values 1–8 equal the class codes of the canonical form
([F07 §6.1]), so one numbering serves both chapters; the schema class, which [F07] keys apart, is 9 (pass 1, A1-46).

### 6.2 Key values (`kval`) and conflict states (`cstate`)

A `kval` is a value of a key. Its shape follows from the key's class, so it carries no class byte.

| key class | `kval` encoding |
|---|---|
| `existence` | `ex` `u8`: 0 `absent` (no node on that side); 1 `live`, then `kind` `u8` ([F08]) and `snap` `u8` (0 or 1, V), then a node image (§6.3) if `snap` = 1; 2 `deleted`, then `kind` `u8`, `reason` `uvar32` (class `reason`), `replaced_by` `uvar32` (`#N`, 0 = none). 3–255 invalid (V) |
| `status` | `st` `u8`: 0 absent; 1 present, then `status` `u8` and `resolution` `u8` ([F08]) |
| `field`, `counter` | `tvalue` (§5.1), `absent` allowed |
| `observation` | six `tvalue`s in the order `path`, `oid`, `bytes`, `observed_git`, `observed_blob`, `relink`, each may be `absent` |
| `body` | `bf` `u8`: 0 absent; 1 present, then the body hash `b16` |
| `hierarchy` | `parent` `uvar32` (`#N`, 0 = none), `order` `vstr` (the fractional index; empty = none) |
| `edge` | `ef` `u8`: 0 absent; 1 present, then an edge property block ([F08 §10.2]) |
| `schema` | `sf` `u8`: 0 absent; 1 present, then the item `vbytes` ([F08]) |

Wherever a `u8` flag above takes values 0 and 1 only, 2–255 are invalid (V). A `deleted` value carries its node's kind, as
the canonical value does ([F07 §7.2]), so that every `kval` maps to one canonical value without reading another key.

- **Snapshots.** A `live` existence value in a conflict side carries `snap` = 1 and that side's node image, so that a
  `DeleteVsModify` resolved `--take` towards a live side can restore every value key of it ([RULES/merge-table] open
  point 5 (c); open point 13). A `Resolve.new` that makes the node live carries the restored image the same way. Elsewhere
  `snap` is 0 (C).

A `cstate` is the value a key holds, which may be a conflict value:

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `cs` | `u8` | always | 0 plain, 1 conflict; 2–255 invalid (V) |
| 2 | `value` | `kval` | `cs` = 0 | the plain value |
| 3 | `class` | `u8` | `cs` = 1 | the conflict class ([F12 §6.1]) |
| 4 | `base`, `ours`, `theirs` | 3 × `kval` | `cs` = 1 | the three sides |
| 5 | `prov` | `u8` | `cs` = 1 and the key's class is `existence` | the provisional side ([F12 §6.3]): 0 `ours`, 1 `theirs`; 2–255 invalid (V). Absent for every other key class, whose provisional value is derived from the sides (pass 1, S1-5, A1-9) |

A conflict value's sides are plain values, never conflict values (C): a merge over a key whose base holds a conflict
value takes that conflict's base ([RULES/merge-table] open point 18).

### 6.3 Node images

A node image lists the **value keys** of one node: its status, its body and its fields (kind fields and the header
scalars the schema names as fields, [F08]). It never holds the existence key, the hierarchy key or edges, which have
their own ops.

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `n_entries` | `uvar32` | always | number of entries |
| 2 | entries | `n_entries` × entry | always | sorted: the status entry first, the body entry next, then field entries by `name` ascending; at most one of each key (V) |

An entry is `ie` `u8` then:
- 0 `status`: `status` `u8`, `resolution` `u8`;
- 1 `body`: the body hash `b16`;
- 2 `field`: `name` `uvar32` (class `name`), then a `tvalue` that is not `absent` (V).

3–255 are invalid (V). A key not listed is absent. A counter's entry holds its current total with type `counter` ([F08 §5.1]).

## 7. Ops

### 7.1 Frame

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `op` | `u8` | always | the op tag (§7.2) |
| 2 | `len` | `uvar32` | always | byte length of the body |
| 3 | body | `len` bytes | always | the op's fields (§7.4–§7.7), which fill `len` exactly (V) |

The length lets a reader skip ops without decoding them: the compact overlay decodes values on probe ([AR §4.5] step 1).

### 7.2 Op tags

| value | op | key (§6.1) | carries `prev` |
|---|---|---|---|
| 1 | `Create` | existence | yes |
| 2 | `Delete` | existence | yes |
| 3 | `Undelete` | existence | yes |
| 4 | `SetField` | field | yes |
| 5 | `SetStatus` | status | yes |
| 6 | `Incr` | counter | yes |
| 7 | `SetBody` | body | yes |
| 8 | `AddEdge` | edge | yes |
| 9 | `RemoveEdge` | edge | yes |
| 10 | `SetEdgeProps` | edge | yes |
| 11 | `Move` | hierarchy | yes |
| 12 | `Schema` | schema | no |
| 13 | `Conflict` | its `key` | iff the key has an owner |
| 14 | `Violation` | none (it may name a key) | no |
| 15 | `Resolve` | its `key` | iff the key has an owner |
| 16 | `CreateDeleted` | existence | yes |

0 and 17–255 are invalid (V): a zeroed op area never decodes. There is no op for directory moves: `path_moves` is an
ordinary set field ([40] R-5).

### 7.3 `prev`

`prev` is a `uvar64`: 0 when the owner has no earlier op on this ref; otherwise the record's own lsn minus the lsn of
the newest earlier `Commit` record with the same `ref_id` whose changeset touches the owner — holds an op of §7.2 that
carries a `prev` for that owner, or, for a bulk commit, a row of that node in its `cs.<n>`. So every op keeps [AR §4.3]'s "`prev` delta to the node's previous op lsn on this ref", and per-node
history is a chain within a ref; walkers hop to the parent ref at a fork and into `main`'s chain at a sync window
([AR §5a.6]).

- All ops of one owner in one record carry the same `prev` (V).
- **Inline commits.** The writer serialises the ops after it knows the record's lsn, under the writer byte, as it must
  anyway to fill in the `#N` placeholders of [AR §4.5] step 8, and takes each owner's `prev` from the owner's chain head at
  that moment (open point 16). So `prev` is right whatever an intervening commit touched, and a re-parent that
  re-validates by key ([AR §4.5] step 7) needs nothing more.
- **Bulk commits.** The rows of `cs.<n>` carry an absolute `prev` sealed before the writer byte ([F09 §16.4]). A re-parent
  can keep it only if no intervening commit touched the owner: [F16] P-34 therefore re-validates a bulk commit **by
  node** — every node that owns a row of its changeset counts as read and written, and any intervening commit that
  touches one forces a phase-1 re-run and a re-stream (pass 1, S1-20).
- A record keeps its lsn for life (adoption re-writes it in place; `hist` keeps it, [F10]), so `prev` never goes stale.

### 7.4 Node ops

Every node op begins with `id` (`uvar32`, the owner `#N`) and `prev` (§7.3).

**Base state.** An op's "before" values are the key's values in the commit's **base state**: the state at the first
parent; for a `sync` commit, the lane parent's state with `main`'s window `(M_{k−1}, M_k]` applied by reference, so
that the residue ops turn "what the window alone yields" into the merged value ([AR §5a.3]; C).

**`Create`** — a node comes into existence (canonical `created(kind)`).

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `id` | `uvar32` | always | the `#N`: newly allocated, or the `#N` the store already binds to `uid` (I1, `UIDX`) |
| 2 | `prev` | `uvar64` | always | §7.3 |
| 3 | `uid` | `b16` | always | random, or derived per the kind's `uid_derivation` ([F08]) |
| 4 | `kind` | `u8` | always | the node kind ([F08]) |
| 5 | `c_actor` | `uvar32` | always | symbol, class `actor`: the `CREATOR` actor ([50] F4) |
| 6 | `c_role` | `uvar32 ≤ 65,535` | always | symbol, class `role`: the `CREATOR` role |
| 7 | `image` | node image | always | every value key of the node at the end of the commit |

`c_actor` and `c_role` are the creating commit's `actor` and `role` for a local create; a merge, sync, cherry-pick or
native import that lands a `Create` made elsewhere keeps the op's original creator; a foreign or checkpoint import uses
the importing commit's actor and role (C; open point 12). They are not hashed.

**`Delete`** — the node is deleted (canonical `deleted(reason, replaced_by)`).

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `id` | `uvar32` | always | the node |
| 2 | `prev` | `uvar64` | always | §7.3 |
| 3 | `reason` | `uvar32` | always | symbol, class `reason` (`TOMB.reason_sym`); 0 = empty; a foreign file removal uses `image:file-removed` |
| 4 | `replaced_by` | `uvar32` | always | `#N` of the replacement, 0 = none |
| 5 | `before` | node image | always | the full before-image: every value key of the node in the base state ([AR §2.5] "tombstone in history with the full before-image") |

Which value keys the tombstone state keeps (title, reason, replaced_by; [AR §5b.2] rule 8) is [F08]'s and [F07]'s. The
node's hierarchy key and edges change by their own ops in the same commit (the delete policies, [AR §3.3]).

**`Undelete`** — a deleted node lives again (canonical `undeleted`).

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `id` | `uvar32` | always | the node |
| 2 | `prev` | `uvar64` | always | §7.3 |
| 3 | `reason` | `uvar32` | always | before-image: the tombstone's reason (class `reason`) |
| 4 | `replaced_by` | `uvar32` | always | before-image: the tombstone's replacement, 0 = none |
| 5 | `image` | node image | always | every value key of the node at the end of the commit |

**`CreateDeleted`** — a node that is absent in the base state is deleted at the commit (canonical absent →
`deleted(kind, reason, replaced_by)`, [F07 §7.2]; pass 1, S1-6, A1-4). It arises when a merge or sync lands a lane that
created and later deleted the node, and when an import-checkpoint brings in a tombstone file its parent lacks
([F07 §10.1], [F12 §7.8], [F14 §11.2]).

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `id` | `uvar32` | always | the `#N` the store binds to `uid` (I1, `UIDX`), newly allocated if the store has never seen the uid |
| 2 | `prev` | `uvar64` | always | §7.3 |
| 3 | `uid` | `b16` | always | the node's uid |
| 4 | `kind` | `u8` | always | the node kind ([F08]) |
| 5 | `c_actor` | `uvar32` | always | symbol, class `actor`: the `CREATOR` actor, by `Create`'s rule: the original creator for a merge, sync or native import, the importing commit's actor for a foreign or checkpoint import (C) |
| 6 | `c_role` | `uvar32 ≤ 65,535` | always | symbol, class `role`: the `CREATOR` role |
| 7 | `reason` | `uvar32` | always | symbol, class `reason`; 0 = empty |
| 8 | `replaced_by` | `uvar32` | always | `#N` of the replacement, 0 = none |
| 9 | `image` | node image | always | the tombstone's retained value keys: at most the `title` field entry ([F07 §6.4], [F08 §3.5]) (C) |

`c_actor`, `c_role`, `id` and `prev` are not hashed. The tombstone's retained out-edges ([F07 §6.4], I39′) are `AddEdge`
ops of the same record.

**`SetField`**

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `id` | `uvar32` | always | the node |
| 2 | `prev` | `uvar64` | always | §7.3 |
| 3 | `name` | `uvar32` | always | the field, symbol class `name` |
| 4 | `old` | `tvalue` | always | before-image |
| 5 | `new` | `tvalue` | always | the new value; `old` ≠ `new` (V, bytewise) |

**`SetStatus`** — status and resolution are one key ([AR §4.6] item 10).

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `id` | `uvar32` | always | the node |
| 2 | `prev` | `uvar64` | always | §7.3 |
| 3 | `old_status` | `u8` | always | before-image ([F08]'s status code of the node's kind) |
| 4 | `old_resolution` | `u8` | always | before-image ([F08]'s resolution code, whose "none" value [F08] fixes) |
| 5 | `new_status` | `u8` | always | new status |
| 6 | `new_resolution` | `u8` | always | new resolution; the pair differs from the old pair (V) |

**`Incr`** — no before-image: its inverse is the negated delta.

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `id` | `uvar32` | always | the node |
| 2 | `prev` | `uvar64` | always | §7.3 |
| 3 | `name` | `uvar32` | always | the counter field, class `name` |
| 4 | `delta` | `svar64` | always | the net delta, not 0 (V) |

**`SetBody`**

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `id` | `uvar32` | always | the node |
| 2 | `prev` | `uvar64` | always | §7.3 |
| 3 | `bflags` | `u8` | always | bit 0 `old` present, bit 1 `new` present; bits 2–7 reserved-zero; not 0 (V) |
| 4 | `old` | `b16` | `bflags` bit 0 | before-image: BLAKE3-128 of the old body |
| 5 | `new` | `b16` | `bflags` bit 1 | BLAKE3-128 of the new stored body ([AR §5b.2] rule 7); differs from `old` (V). Clear when the body becomes empty: an empty body is no body ([F08 §7.2]) |

**`Move`** — the hierarchy key.

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `id` | `uvar32` | always | the node |
| 2 | `prev` | `uvar64` | always | §7.3 |
| 3 | `old_parent` | `uvar32` | always | before-image, `#N`, 0 = none |
| 4 | `new_parent` | `uvar32` | always | `#N`, 0 = none |
| 5 | `old_order` | `vstr` | always | before-image, the fractional index, empty = none |
| 6 | `new_order` | `vstr` | always | the (parent, order) pair differs from the old pair (V) |

### 7.5 Edge ops, the edge property block and the anchor record

#### 7.5.1 `AddEdge`, `RemoveEdge`, `SetEdgeProps`

`AddEdge` and `RemoveEdge` share one layout ([AR §4.3] `{src, kind, dst, disc, props}`); `RemoveEdge`'s `props` is its
before-image.

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `src` | `uvar32` | always | the source node (the owner) |
| 2 | `prev` | `uvar64` | always | §7.3 |
| 3 | `ekind` | `u8` | always | the edge kind ([F08]) |
| 4 | `dst` | `uvar32` | always | the destination node |
| 5 | `dflag` | `u8` | always | bit 0: `disc` follows; bits 1–7 reserved-zero (V) |
| 6 | `disc` | `b16` | `dflag` bit 0 | R-4's discriminator: the anchor uid on an `at` edge |
| 7 | `props` | edge property block ([F08 §10.2]) | always | `AddEdge`: the new props; `RemoveEdge`: the props the edge had |
| 8 | `anchor_no` | `uvar32` | `props.pflags` bit 2 (`anchor`) | the anchor's store-local handle `aN` ([40 §2.7], R-6), not hashed |

- **V.** `props.pflags` bit 2 is set exactly when `dflag` bit 0 is set, and then the anchor record's `uid` equals `disc`
  ([F08 §10.3]).
- `AddEdge`'s `anchor_no` is newly allocated from `next_anchor`, or, when the store already knows the anchor uid (a merge,
  sync or import landing it), the `aN` bound to it ([40 §2.7], A1P-12; C). Recovery derives `next_anchor` as one more
  than the greatest `anchor_no` of any `AddEdge` in the scanned log, as it derives `next_id` ([F05], [F16]).
- `RemoveEdge`'s `anchor_no` names the removed anchor's handle (C).

**`SetEdgeProps`** ([40] R-4: `{src, kind, dst, disc, old, new}`) changes the props of an edge that exists before and
after: an anchor repin or pin, a `pinned_commit` re-pin, a flag set by a delete policy (open point 14).

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `src` | `uvar32` | always | the source node |
| 2 | `prev` | `uvar64` | always | §7.3 |
| 3 | `ekind` | `u8` | always | the edge kind |
| 4 | `dst` | `uvar32` | always | the destination node |
| 5 | `dflag` | `u8` | always | as above |
| 6 | `disc` | `b16` | `dflag` bit 0 | as above |
| 7 | `old` | edge property block ([F08 §10.2]) | always | before-image |
| 8 | `new` | edge property block ([F08 §10.2]) | always | the new props; differs from `old` (V) |

For an `at` edge, `old` and `new` both carry an anchor record whose `uid`, `captured` and `pred` equal each other and
`disc` (a repin changes selectors, never `captured` or the uid, [40 §2.7]; V). The edge keeps its `aN`, so the op carries
none.

#### 7.5.2 The edge property block

The block is [F08 §10.2]'s (`pflags` with bit 0 `has_pin`, bit 1 `flagged`, bit 2 `anchor`; the full 32-byte
`pinned_commit`; the anchor record), byte for byte; this chapter does not restate it (pass 1, S1-2, A1-3, P1-1). Which
bits each edge kind admits is [F08 §10.2]'s rule over the kind's `props` (C).

#### 7.5.3 The anchor record ([40 §2.7], R-4)

The anchor record an `at` edge's property block carries is [F08 §10.3]'s, byte for byte, with its scope value of
[F08 §10.3.1] (pass 1, P1-1, S1-3, A1-2). The field semantics and capture rules are [40 §2.7] and [F20 §6.1]'s; the uid
derivation is [F08 §11.4]'s; which fields enter canonical item 10, and in which form, is [F07 §8.2]'s (R-10). The ops add
only the unhashed `anchor_no` (§7.5.1).

- **Text-unavailable.** An anchor whose record has `text_unavailable` set arrived through a `hash-only` destination and
  resolves by hint, window and scope only ([40 §5.7], [F20 §6]).
- **I-F9** (C, [F18]): a `quote`, `range`, `symbol` or `heading` anchor carries a quote or its digests; a `lines` anchor
  a window ([F08 §10.3]).

### 7.6 `Schema`

[AR §4.3]'s `Schema{weaken|strengthen|query, payload}` and [50] F3's `Schema{weaken, query}`: the mode is weaken or
strengthen, and a project named query is a schema item of class `query` whose key is its name ([50 §4.4]).

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `mode` | `u8` | always | 0 `weaken` (applies at once; merges freely, except a `policy` item, whose divergence is `SchemaConflict`, [F08 §8.5.6]), 1 `strengthen` (needs `moirai migrate`, [AR §2.12]); 2–255 invalid (V) |
| 2 | `item_class` | `u8` | always | 1 kind, 2 field, 3 enum value, 4 edge kind, 5 `query`, 6 `policy` ([F08 §8.5]) |
| 3 | `item_key` | `vbytes` | always | the item's stored key form ([F08 §8.5]: the component names joined by one `00` byte, `*` = `2A`); for `query` and `policy`, the name's UTF-8 bytes |
| 4 | `sflags` | `u8` | always | bit 0 `old` present, bit 1 `new` present; bits 2–7 reserved-zero; not 0 (V) |
| 5 | `old` | `vbytes` | `sflags` bit 0 | before-image: the item in [F08]'s encoding |
| 6 | `new` | `vbytes` | `sflags` bit 1 | the new item; absent for `DROP QUERY` ([50 §3.10] item 6) |

A `DEFINE QUERY` or `DROP QUERY` is mode 0 with `item_class` `query` (C); so is every `Schema` op of class `policy`
([F08 §8.5.6]; C). The `QUERIES` item's layout — grammar version,
parameter signature, shape, budget class, the portable text and the unhashed canonical-AST hash — is [F08]'s (F3).

The store-local id of a kind, edge kind or enumeration value ([F08 §8.3]) is part of the item record ([F08 §8.5]: the
store-local fields `kind_id`, `edge_id`, `value`), so the `Schema` op that first lands an item carries its id inside
`new`, unhashed ([F07 §9]); the op has no id field of its own (pass 1, A1-20).

### 7.7 `Conflict`, `Violation`, `Resolve`

**`Conflict`** sets a key to a conflict value ([AR §5a.7] step 7).

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `key` | `ckey` | always | the conflicted key |
| 2 | `prev` | `uvar64` | the key has an owner | §7.3 |
| 3 | `old` | `cstate` | always | before-image: the key's value in the base state (dst's), possibly an earlier conflict value |
| 4 | `class` | `u8` | always | the value-conflict class ([F12]): `FieldEdit`, `StatusFork`, `TextHunk`, `DeleteVsModify`, `SupersedeFork`, `OwnerFieldEdited`, `PathClaim` |
| 5 | `base` | `kval` | always | base side |
| 6 | `ours` | `kval` | always | dst side |
| 7 | `theirs` | `kval` | always | src side |
| 8 | `prov` | `u8` | the key's class is `existence` | the provisional side ([F12 §6.3]): 0 `ours`, 1 `theirs`; 2–255 invalid (V) |

Orders 4–8 are exactly the conflict part of a `cstate` with `cs` = 1 (§6.2), so the key's new value is that `cstate`.
[F11 §10] `CONFLICTS` stores these bytes per side. A `PathClaim` sits on each claiming node's `observation` key
([RULES/link-merge-rules] PC-002); where a `SupersedeFork` sits is [F12]'s ([RULES/merge-table] open point 9).

**`Violation`** records a structural violation; it exists only on staging refs (`merge/*`, `import/*`), is never hashed
and never exported ([AR §4.6], [AR §5a.8]; C).

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `class` | `u8` | always | the structural class ([F12]), `QueryInvalid` and `QueryCycle` ([50] F18) included |
| 2 | `vflags` | `u8` | always | bit 0 `key` present; bits 1–7 reserved-zero (V) |
| 3 | `key` | `ckey` | `vflags` bit 0 | the key the violation is about |
| 4 | `description` | `vstr` | always | the detail text ([LQ/envelope §5.10]) |
| 5 | `suggested` | `vstr` | always | the suggested resolution statement; may be empty |

**`Resolve`** ([AR §5a.8], [50 §3.10] item 6 `RESOLVE`) replaces a conflict value, or a violation's subject, by a chosen
value. The canonical form records the resulting value, not the choice ([AR §5b.4] row 10), so the op stores both.

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `key` | `ckey` | always | the resolved key |
| 2 | `prev` | `uvar64` | the key has an owner | §7.3 |
| 3 | `choice` | `u8` | always | 0 `ours`, 1 `theirs`, 2 `base`, 3 `value`, 4 `repoint`, 5 `drop`; 6–255 invalid (V) |
| 4 | `target` | `uvar32` | `choice` = 4 | the `#N` the edge is re-pointed to |
| 5 | `old` | `cstate` | always | before-image |
| 6 | `new` | `kval` | always | the resulting plain value of `key` |

A `repoint` changes the violating or flagged edge's key to `absent` in `new`; the re-pointed edge is an `AddEdge` in the
same commit (C). `drop` applies only to a flagged edge on a work or plan branch, and its `new` is `absent` (C). On a
violation's key that holds no conflict value, `ours`, `theirs` and `base` name the staged operation's sides ([F12 §6.5]).
A write to a key that holds a conflict value is always a `Resolve` (`choice` 3 for a plain `SET`; C).

### 7.8 Net form

The stored op list is the commit's **net** changeset ([AR §4.3]): what remains after the writer coalesces the commit's
changes per key. For every kind except `sync` it equals the typed diff between the base state and the state at the
commit; for `sync` it is the residue ([AR §4.6] "Net changeset = state diff").

- **NF-1 One op per key** (V). No two ops of a record have the same key (§7.2's key column; `Conflict` and `Resolve` take
  the key they name). `Violation` ops are not keyed.
- **NF-2 First before-image, last value** (C). When a key changed several times in the commit, `old` is its value in the
  base state and `new` its last value; two `Incr` on one counter sum ([AR §4.3]).
- **NF-3 No no-op** (V where the op shows both values, C otherwise). An op whose new value equals its old value is not
  stored; an `Incr` that sums to 0 is not stored; a key changed and changed back is not stored. One exception: a
  `Resolve` on a staging ref is stored even when `new` equals `old`'s plain value, because it records that the key's
  staged violation was resolved ([F12 §9.4] step 2); without it a resolution that keeps the staged value (a `--take ours`
  on a V01 skip) would be lost (spec sync 2b). Whether a ref is a staging ref is not in the record, so for `Resolve`
  ops NF-3 is a C-rule.
- **NF-4 Image folding** (V). A record with a `Create` or `Undelete` of node n holds no `SetField`, `SetStatus`, `Incr` or
  `SetBody` of n: those changes are in the image, which holds n's value keys at the end of the commit ([AR §4.3]: "a
  `Create` followed by `SetField` folds into the `Create`").
- **NF-5 Delete folding** (V). A record with a `Delete` of node n holds no `SetField`, `SetStatus`, `Incr` or `SetBody`
  of n; the `Delete`'s before-image holds n's value keys in the base state.
- **NF-6 Existence nets** (C). A node created and deleted in one commit leaves no op (and its placeholder `#N` is never
  allocated, [AR §4.5] step 8); a node deleted and undeleted in one commit leaves only the net value changes.
- **NF-7 Edges** (C). Added then removed: no op. Removed then added with other props: one `SetEdgeProps`. Added, then
  props changed: one `AddEdge` with the last props.
- **NF-8 Hierarchy** (C). One `Move` with the first before-image and the last value, and none if they are equal.
- **NF-9 Conflicts** (C). A key that becomes a conflict value has a `Conflict` op and no other op; a key whose conflict
  value is replaced has a `Resolve` op and no other op.
- **NF-10 Markers** follow from the net ops ([AR §4.5] step 4, [F16]), so `TX { REOPEN t; SET t.done = true }` on a done
  task stores no `SetStatus` and emits no marker; its `Incr` of `reopen_count` remains.
- **NF-11 Absent to deleted** (C; pass 1, S1-6). A record holds a `CreateDeleted` of node n exactly when n's existence
  key is `absent` in the base state and `deleted` at the commit. A local write never produces that transition (NF-6), so
  only `merge`, `sync` and `cherry-pick` commits and imported commits hold the op. The record holds no `SetField`,
  `SetStatus`, `Incr`, `SetBody` or `Move` of n (the op's image holds the retained title, as NF-5 folds a `Delete`); n's
  retained out-edges are `AddEdge` ops.

### 7.9 Op order (V)

Ops are sorted ascending by (owner, rank, detail), where:
- **owner** is the key's owner `#N`, or 0 for a `Schema` op, a `Conflict` or `Resolve` on a schema key, and a `Violation`
  without an owner (a `Violation` with a node key uses that node);
- **rank** is the key's class value (§6.1: existence 1 … body 8, schema 9); a `Violation` has rank 10;
- **detail** is: for field and counter keys, `name`; for edge keys, (`ekind`, `dst`, `dflag`, `disc` bytewise); for
  schema keys, (`item_class`, `item_key` bytewise); for `Violation` ops, the op body bytewise; none otherwise.

The order is store-local and only the stored form's. It lets a reader stop scanning a record once it passes the owner it
looks for; [F07] sorts canonically by uid and names.

### 7.10 Inverses and the canonical relation

- **Inverses** ([AR §5a.5]). `revert` appends the inverse of an origin's net ops, computed from the stored before-images:
  `Create` → `Delete` (the image becomes the before-image; as for every `Delete`, `before` holds the node's value keys
  in the revert's base state, §7.4, which differ from the image only where a later commit changed the node),
  `Delete` → `Undelete` and `Undelete` → `Delete` (each takes
  the other's `reason`, `replaced_by` and image; [AR §5d.3]: "`Undelete #40` with the before-image"), `SetField`,
  `SetStatus`, `SetBody`, `Move`, `SetEdgeProps` and `Schema` swap `old` and `new`, `AddEdge` ↔ `RemoveEdge`, `Incr`
  negates `delta`, `Conflict` and `Resolve` restore their `old`. **The inverse `Delete` of a `Create`** has `reason` = 0
  (the empty reason) and `replaced_by` = 0 (none), so a reverted creation leaves the tombstone `deleted(kind, "", none)`
  ([F07 §7.2], [F07 §13]), which [F14 §6.10] writes with no `field reason:` line. No revert takes a reason or a replacement ([AR §7.1]), and the
  commit's `origin` (canonical item 8) already names the reverted commit, so the tombstone carries none (pass 1, round 3,
  closure NC-9). `CreateDeleted` has no inverse: a revert leaves the tombstone, as the inverse of a `Create` does, and
  emits no existence op. A before-image that no longer matches is `NotFound` or a conflict value of its key's own class
  ([AR §5a.5]; I34′; there is no `DATA` class, [F12 §6.1]). A `sync` is never reverted; a bulk commit's before-images come
  from its base state (§9). A pruned commit (§4.4.15) is never reverted.
- **Canonical relation.** Every stored op maps to item-10 entries keyed by uid and names ([AR §4.6]); the mapping, and the
  full state diff of a `sync`, are [F07]'s. No stored-only datum enters it: `#N`, `aN`, `prev`, symbol numbers,
  before-images, `creator`, `Violation` ops and anchor texts stay out (§4.5).

## 8. Bodies carried by a commit

A body travels in the log tail with a codec byte and is sealed into `blobs.<n>` at a checkpoint ([AR §4.3], [AR §4.9]).
The `bodies` of a record (§4.3 order 43) are:

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `hash` | `b16` | always | BLAKE3-128 of the raw stored bytes |
| 2 | `codec` | `u8` | always | [F10]'s codec byte |
| 3 | `raw_len` | `uvar32` | always | length of the raw bytes, at most [F08]'s body cap (64 KiB, [AR §2.6]) |
| 4 | `data` | `vbytes` | always | the raw bytes when `codec` is [F10]'s `none`; otherwise a payload of that codec that decodes to exactly `raw_len` bytes |

- **BD-1** (V) Entries are sorted by `hash` ascending, with no duplicate.
- **BD-2** For codec `none`, `len(data) = raw_len` (V). `hash` = BLAKE3-128 of the raw bytes (C: checked by
  `doctor --fsck`, the format oracle and the reader that seals the body, so a replay never hashes bodies). The hash
  covers the stored bytes, which the store normalised at write (CRLF → LF, [AR §5b.2] rule 7; I40′).
- **BD-3** Whether a writer may put a codec other than `none` in a commit record is [F10]'s body-placement hole
  (measurement 6); the entry's bytes are the same in every outcome.
- **BD-4 Carriage** (C). A record carries every body whose hash one of its ops introduces as a **new** value — a
  `SetBody.new`, the body entry of a `Create` or `Undelete` image, a body side of a `Conflict`, a body `Resolve.new` —
  unless the body is **available**: listed in the `BLOBTAB` of the segment set published in the `HEAD` slot the writer read
  under the writer byte before appending, or carried by an earlier `Commit` record after that set's `upto_lsn`.
  Before-images are never carried.
- **BD-5** (C) A record carries no body that none of its ops references.
- **BD-6 Obligation on [F16] and [F09]** (C). A checkpoint or GC that publishes a segment set S with bound u keeps in S's
  `BLOBTAB` every body that a `Commit` record after u references without carrying it; a reader resolves a body hash first
  through the tail records after its segment set's bound, then through `BLOBTAB`.

## 9. Bulk commits

A **bulk commit** ([AR §4.3], [71 RAM-B1]) keeps its changeset in a sealed changeset segment `cs.<n>` ([F02 §5.1]) instead
of the record.

- **BK-1** (V) Bit `cs_ref` is set, and `n_ops` = `n_bodies` = 0.
- **BK-2** (C) `cs_ref.file` is the file number n of `cs.<n>` (a `u32` from 1, [F02 §6.2]); `cs_ref.len` is its length,
  which equals the file's size and its `SegHdr.total_len` ([F09]); `cs_ref.b3` is the first 16 bytes of the file's
  `SegHdr.seg_digest` ([F09 §2.1]), the value `FILES.digest16` also holds ([F09 §14.4]; pass 1, P1-6, S1-19, A1-21). A
  reader checks the length before mapping ([80 §2.5]), and the open check V-8 compares `b3` with the header's
  `seg_digest[0..16]` without reading the file ([F09 §17.1]); `doctor --fsck` recomputes `seg_digest` over the file, and
  [F16] states whether recovery checks it on adoption. A missing or
  mismatching file named by a valid record is corruption (exit 7, `moirai repair`).
- **BK-3** (C) The file is built as `tmp/cs.<nonce>`, flushed (`durable+meta`), moved to `cs.<n>` by `rename_noreplace`,
  and both directories are flushed (`durable-name`), all before the group holding the record is appended ([F02 §5.2]
  rule 2; [F16]). Its bodies go to a `blobs.<n>` file that is durable by the same rule ([AR §4.3]: "bodies straight to
  `blobs`"; [F09], [F10]).
- **BK-4** Which verbs may produce one, and the refusal for agent verbs, are [F17 §4.4] W1–W4. A bulk and an inline commit
  of the same changeset have the same `commit_id`: `changeset_digest` is computed over canonical item 10 while the rows
  stream ([F07]).
- **BK-5 Requirements on the `seg_kind = changeset` layout** ([F09]). The segment, in delta-segment layout (rows sorted
  by `#N`, list replacements, bitset ± lists), must hold every effect the inline ops of §7 can express, so that a bulk
  commit loses nothing an inline one keeps:
  - created nodes with uid, kind and creator (`c_actor`, `c_role`); deleted nodes with reason and replacement;
    undeleted nodes;
  - per touched row, its `prev` (§7.3), so the per-node chain passes through bulk commits; its correctness rests on
    [F16] P-34's node-granular re-validation (§7.3);
  - nodes that go from absent to deleted (`CreateDeleted`, §7.4) with kind, reason, replacement and creator ([F09 §16.4]'s
    absent-to-deleted row);
  - conflict values (a long merge can be bulk), violations (a bulk merge that stages), resolutions, schema items;
  - anchors with their `aN` and edge props;
  - for a bulk import-checkpoint, the image-only data of §4.4.14, entry for entry ([F09 §16.4] `CKIMG`; pass 1, S1-23);
  - no before-images: a revert, cherry-pick, `blame` or `show` of a bulk commit derives them from its base state
    ([AR §4.6] "before-images (derivable from the parent state)").
- **BK-6** Readers map `cs.<n>` as one more delta layer; the next checkpoint folds it; `hist` keeps the record and its
  `cs_ref` ([AR §4.1]). The crash state "file durable, record not" leaves an unreferenced file for the orphan sweep
  ([F02] open point 5, [F16]).

## 10. Validation summary

A decoder of a `Commit` payload (the product codec, the format oracle, `doctor --fsck`) checks every V-rule: §3.1–§3.3,
§4.1, §4.2 (reserved bits, presence against kind and import), §4.3 (orders 3, 5, 26, 40, 42), §4.4.3, §4.4.5, §4.4.6,
§4.4.9–§4.4.16, §5.1–§5.5 with [F08 §5]'s value rules, §6.1–§6.3, §7.1–§7.9 with [F08 §10.2]–§10.3's block and record
rules, §8 BD-1 and BD-2's length rule, §9 BK-1, and [F01]'s encoding rules (canonical varints, UTF-8, `bool8`). The
C-rules are checked by `doctor --verify`, the model and the gates (§2.4).

## 11. Example (informative)

A local `ordinary` commit on `main` produced by `moirai claim 12 --start` through a lease: one parent, no git provenance,
an idempotency key, the named mutation `tx.claim`, message `claim --start`, `affected` = {#12}, one `SetStatus` on #12.
Symbol ids, status codes, digests and lsns are illustrative; `C…`, `P…`, `K…`, `Q…`, `S…` and `D…` stand for 32-, 16- or
32-byte digests. Fixtures are written from the rules above, never from this example ([F01 §2.1]).

```
43 70 00 00                  presence: ref_old, prev_on_ref, idem, stmt_hash, msg, affected (0x7043)
C… (32)                      commit_id
01                           n_parents
P… (16)  B4 24               parent id16, lsn 4,660
07                           gen 7
F7 22                        seq 4,471
01 00                        ref (symbol 1 = "main"), ref_id 0
P… (16)                      ref_old = the parent
B4 24                        prev_on_ref 4,660
07                           ref_seq 7
00 00                        kind ordinary, import local
03 00 00 6C 50 C4 A0 01      hlc 0x01A0C4506C000003 ([F01 §5.7])
05 02 03                     actor 5, role 2, session 3
01                           schema_version 1
K… (16)  Q… (16)             idem_key, idem_payload
01 01 04                     stmt_origin named-mutation, actor_src lease, stmt_sym 4 = "tx.claim"
S… (16)                      stmt_hash
00                           append_delta 0
0D 63 6C 61 69 6D 20 2D 2D 73 74 61 72 74    msg "claim --start"
01 01 0C                     affected: 1 id, complete, #12
D… (32)                      changeset_digest
01                           n_ops
05 07 0C C0 25 00 00 01 00   SetStatus, len 7: #12, prev 4,800 back, open/none -> in_progress/none
00                           n_bodies
```

The payload is 205 bytes, 237 with the 32-byte `RecHdr` ([AR §4.3]'s estimate: ≈ 0.35–0.5 KB with git provenance).

## Coverage

| Item | Part covered here | Section |
|---|---|---|
| [60 §2.5] row "Commit body" ([AR §4.3] incl. `ref_old`, `prev_on_ref`, `ref_seq`, `sync_base`, absorbed vector, `foreign_git`, `verified`, `import`) | complete: every field, its encoding, presence and rules | §4 |
| [60 §2.5] row "Ops and values" (every op with before-images and `prev` deltas; the closed type set {bool, int, counter, f64, enum-with-lattice, text, set, ref, commit-ref}) | every op with its before-image and `prev`, key values and conflict states; the value bytes are [F08 §5]'s, which the ops carry | §5, §6, §7 |
| [60 §2.5] audit row "Commit body" (`actor u32`, `changeset_digest`, `cs_ref`, the inline bound) | the fields and the bulk-commit record; the bound's value and the write-size switch are [F17 §4.4]'s | §4.3, §4.6, §9 |
| [60 §2.5] row "Canonical form" | only the stored inputs of items 1–10 and the hashed/unhashed split; the encoding is [F07]'s | §4.5, §7.10 |
| [60 §2.5] row "Gate-0 carrier table" | the commit-kind enumeration with import-checkpoint, which the table's per-kind fixtures use; the table is [F14]'s | §3.1–§3.3 |
| [60 §2.5] row "Log" | the payload of record kind `Commit` only; `RecHdr`, groups and the other kinds are [F05]'s | §4.1 |
| [60 §2.5] audit row "Segments" (`seg_kind = changeset`) | the `Commit` side of a bulk commit and the requirements on the segment; its layout is [F09]'s | §9 |
| [40] R-1 (`path`, `oid`, `pathmove`; `hlc` as [40 §2.4] defines it) | `pathmove.hlc` and the root rule in commits; the stored encodings are [F08 §5.2]'s, the by-name canonical encoding of the root [F07]'s | §5.5 |
| [40] R-4 (`at`, discriminator, anchor record with `captured` and `pred`, `SetEdgeProps`, anchor props of `AddEdge`/`RemoveEdge`, unhashed `aN`) | the ops, the discriminator in the edge key and the unhashed `aN`; the edge property block and the anchor record are [F08 §10.2]–§10.3's, the `at` kind code [F08]'s, the `ANCHORS` section [F09]'s | §6.1, §7.5 |
| [40] R-5 (no op for directory moves) | complete for the op list | §7.2 |
| [40] R-6 (`next_anchor`) | the log side: `AddEdge` carries `aN`, from which recovery derives the counter; the `HEAD` field is [F04]'s | §7.5.1 |
| [40] R-10 (digests `quote_h`, `prefix_h`, `suffix_h`, `end_h`) | only that the ops carry the anchor record; its digests and texts are [F08 §10.3]'s, the canonical selector block [F07]'s | §7.5.3 |
| [40] R-3 (derivation inputs) | only that the ops carry `captured` and `pred` inside [F08 §10.3]'s record; the derivations are [F08]'s | §7.5.3 |
| [40] R-17 (`relink`) | only its storage as a text value; the vocabulary is [F18]'s | §5.1 |
| [50] F3 (`Schema{weaken, query}`) | the op; the `QUERIES` item layout is [F08]'s | §7.6 |
| [50] F10 (`stmt_origin`, `stmt_sym`, `stmt_hash`) | complete | §3.4, §4.3 |
| [50] F14 (`append_hlc`) | the encoding and value rules; the HLC rule is [OS/clock §7]'s and the append check [F16]'s | §4.4.5 |
| [50] F15 | the stored order and encoding of `affected`; the semantics are [F13 §6.3]'s | §4.4.13 |
| [50] F16 (`affected_len` u32, `affected_complete`) | complete | §4.4.13 |
| [50] F17 (`ALLOC`) | only the `Create` inputs (`uid`, `#N`); the index is [F11]'s | §7.4 |
| [50] F18 (`QueryInvalid`, `QueryCycle`) | the `Violation` op that carries a class; the enumeration is [F12]'s | §7.7 |
| [90 §10.1] row "Commit header" (`actor_src`) | complete | §3.5, §4.3 |

No X-F item of [80 §3] is specified here.

## Holes

None. This chapter fixes no value that an M0 measurement decides. Two holes of other chapters touch its bytes without
changing any width or presence rule: [F10]'s codec-byte values (the `codec` byte of §8) and [F10]'s body-placement hole
(whether a commit record may carry a compressed body, BD-3), both decided by measurement 6 (WP-54).

## Open points for the review

1. **[PLAN §3.3] gaps of WP-12 closed here.**
   - *Import-checkpoint kind value*: `kind` 5 `checkpoint` (§3.1) with `import` 3 (§3.2); a checkpoint re-exported
     natively arrives as `native` with kind 5 (§3.3).
   - *Foreign-commit `hlc` unit*: seconds × 1000, then `<< 16` (§4.4.4). [AR §5b.4] and [AR §2.15] write
     `committer_time << 16`, which puts seconds into the millisecond bits; the review should correct both texts at WP-81a.
   - *F10, F14, F16 and `actor_src` in the header*: placed at orders 31–37 (§4.3).
   - *The length-prefix scheme and the value encodings of canonical items* belong to [F07] (WP-12's other chapter). This
     chapter fixes the **stored** value encodings (§5) that [F07] maps.
2. **Header fields missing from [AR §4.3].** [AR §5a.1] lists the schema version in the commit header and [AR §4.6] hashes
   it (item 7) and the revert/cherry-pick origin (item 8), but [AR §4.3]'s layout has neither. Added as
   `schema_version` (always) and `origin` (`b32`, full, because the origin may not be local after an import). Also
   added: `stated` (the demoted-parent case of N5 needs the stated id, which is hashed and not recoverable from the actual
   parent), `ckpt` (`Moirai-Head` and `Moirai-Folded`, which [AR §5b.6] step 2 says an import records) and `xtr`
   (`Moirai-Ref` and `Moirai-Idem` as imported, which gate 1's byte-identical round trip needs because both are in the git
   commit object, [AR §5b.7]). [F14] should confirm that the carrier table needs nothing else unhashed.
3. **`ref_old` and `prev_on_ref` are optional.** [AR §4.3] lists them as always present. The first commit of a store (and
   of a ref that had no tip) has neither, and an all-zero sentinel would depend on [F05]'s lsn origin. Two presence bits
   make "none" explicit.
4. **Parent lsns are varints.** [AR §4.3] sizes parents at "24 B each" (fixed `lsn u64`), but its encoding rule makes every
   integer except `hlc` a varint. This chapter follows the encoding rule; the 24 B figure is the fixed-width bound.
5. **`sync_base` is redundant.** It always equals `parents[1].id16` (V). It is kept because the design lists it; the
   review may drop the group, freeing bit 7, before the freeze.
6. **`append_delta` is signed** (`svar64`). An imported `hlc` from a clock ahead of this store's can exceed
   `append_hlc`; zigzag keeps a local commit at one byte, as [AR §4.3] intends.
7. **`affected_complete` is a byte** ([50] F16 as aligned by S-09) inside the `affected` group, and the ids are stored
   ascending as gaps, which [F13 §6.3] left to this chapter.
8. **`stmt_origin` values and `stmt_hash` presence.** [50] F10 lists the seven origins without numbers or meanings. §3.4
   reads `verb` as a CLI write verb and gives `stmt_hash` to every commit that came from a `TX` block, so a CLI verb that
   compiles to no `TX` block (`revert`, `cherry-pick`, `migrate`) has no `stmt_hash`. This refines [LQ/canonical-ast]
   C-9, which gives it to every `verb`; the rest of C-9 (absent for merge, import and file verb) is confirmed. An MCP
   `write` that names a named mutation is recorded as `named-mutation`, so it renders like the CLI verb ([AR §6.2]: the
   two produce the same commit).
9. **`actor_src` numbering and imports.** [90 §4.2]'s set is closed. §3.5 numbers it with `none` = 0 and uses `none` for
   imported commits, whose actor comes from the imported record; the `import` byte tells them apart. The alternative, a
   new value `import`, would change [90]'s reservation.
10. **Idempotency key framing** ([LQ/canonical-ast] C-10). §4.4.7 fixes the domain-separated framing of explicit and
    default keys. [AR §6.4] writes the default key as "BLAKE3(…)" and the header as BLAKE3-128 of the key; this chapter
    derives `idem_key` from the inputs in one step instead of hashing a hash. [API] and [F11] use the same derivation; the
    default-key bit of the `IDEM` row ([F17] OP-17-16) stays [F11]'s.
11. **The closed type set has no list, struct or uid type.** [AR §3.2] declares `list<…>` and struct fields and [40 §2.2]
    types `origin_pred` as "u128 or absent". §5 adds no tag. [F08] maps each such field into the closed set (for
    example a list as `text` with a frozen grammar, a struct as component fields, `origin_pred` as `ref`), or asks the
    review for a tag before the freeze ([RULES/merge-table] open point 11).
12. **`Create` carries its creator.** `CREATOR` ([50] F4) is "set at `Create`, never changed", but a merge commit that
    lands a lane's `Create` on `main` has the orchestrator as its actor. The op therefore stores `c_actor` and `c_role`
    (store-local, unhashed), which merges, syncs, cherry-picks and native imports keep. [F09] (the `CREATOR` column) and
    [F08] should confirm.
13. **`Undelete` carries the restored image; conflict sides may carry a snapshot.** [AR §4.3] gives `Undelete` only a
    before-image; the restored values need a home, and NF-4 folds them into the op as it folds a `Create`'s. For
    `DeleteVsModify` under `delete-wins`, a `live` side carries its node image (§6.2), which settles [RULES/merge-table]
    open point 5 (c) inside the closed set; how the image shows it (point 5 (d)) is [F14]'s.
14. **`SetEdgeProps` is general.** [AR §4.3] introduces it for R4's repins and pins. The same op records a `pinned_commit`
    re-pin ("re-confirm re-pins the edge", [AR §3.5]) and the `flagged` bit a delete policy sets, so every props change
    of an existing edge has one form.
15. **Owner of the anchor record's bytes.** The chapter map gives the anchor record (R-4) to no chapter; [F20] assumes
    [F08], `a1-S.md` S-03 assumes [F18]. This chapter defines the stored bytes because the ops carry them (§7.5.3); [F08]
    (derivations), [F18] (semantics, scope grammar), [F09] (`ANCHORS` section, which should reuse the encoding) and [F07]
    (selector block) cite it. The review should confirm one owner. `scope` is one string, as the image writes it
    ([40 §5.7]); [40 §2.7]'s separate numbering field for Markdown headings is inside that string's grammar ([F18]).
    **Pass 1 (P1-1, S1-3, A1-2): closed.** [F08 §10.3] is the one owner and §7.5.3 cites it byte for byte; this chapter's
    former layout is withdrawn, the scope is [F08 §10.3.1]'s binary value, and the ops add only `anchor_no`.
16. **`prev` is measured from the record's own lsn.** The distance is positive and small for recently touched nodes, and
    needs the record's lsn, which the writer knows only under the writer byte. The writer re-serialises the ops there in
    any case to fill in the `#N` placeholders ([AR §4.5] step 8), so the O(1) re-parent keeps its meaning for the
    header and `changeset_digest` only. Consequence for [F17 §4.4]: W1 is decided on the phase-1 encoding, and the final
    encoding differs by at most 4 bytes per placeholder plus the `prev` widths; [F16] states that W1 and W3 are re-checked
    on the final encoding. **Pass 1 (P1-26):** the re-serialisation (the ops with `prev` and `#N`, the record's XXH3 and
    the chain) is O(`cs_bytes`) under the writer byte, up to ≈ 1 ms at the inline bound, against a hold budget of tens of
    µs. Decision: keep the encoding and **measure** it: measurement 2 sweeps the inline size up to P05
    (`store.commit.inline-max-bytes`) and reports the writer hold per size ([F17 §3], [60 §5.2]). If the hold exceeds the
    M1 gate, the fallback is to encode `prev` against a base lsn known in phase 1 (the tip's lsn), which moves the
    re-serialisation out of the writer byte and changes only this field's meaning, before the freeze.
17. **An over-long commit.** §4.6 refuses a commit whose group cannot fit one extent even as a bulk commit (exit 7). Only
    `affected` (≤ 5 B per node), the message (≤ 64 KiB) and the absorbed vector (≤ 10 B per ref) can grow the header
    part; at the production extent (64 MiB) the refusal needs ≈ 12 M affected ids. [F19] assigns the code; [F17]
    OP-17-05 is answered by the bound of §4.6.
18. **Body carriage and availability** (§8). The design says bodies travel in the tail but not how a record references a
    body another record carried. BD-4 lets a merge reference a lane's bodies without copying them; BD-6 is an obligation
    on [F16] and [F09] for checkpoints and GC. Proposal for [F10]: fix the codec byte value of `none` (for example 0)
    independently of measurement 6, since every outcome needs it and the M0 fixtures use it ([PLAN §3.2] WP-20).
19. **`commit-ref` values are 32 bytes.** [AR §3.3] and [AR §4.4] describe `pinned_commit` as a 16-byte prefix in
    `EDGE_PROPS`, but the image writes the full id and a pinned commit need not be local. The op stores 32 bytes; [F09]
    may keep a 16-byte index column, and [F07] hashes the full id. **Pass 1 (P1-1, S1-1, S1-2, A1-3): closed.**
    [F08 §5.1] (`commitref`) and [F08 §10.2] (`pinned_commit`) are 32 bytes, and every stored form keeps the full id: a
    state rebuilt from segments must reproduce [F07]'s hashed value, so [F09]'s `EDGE_PROPS` row stores 32 bytes too.
20. **`Violation` names a key; `Resolve` stores the result.** [AR §4.3]'s `Violation{class, description, suggested}` has no
    key, but [LQ/envelope §5.10] renders one. `Resolve{key, choice}` gains its before-image and resulting value because
    the canonical form records the value, not the choice. Whether `DATA` keeps its own class code ([RULES/merge-table]
    open point 20) is [F12]'s; §7.7 lists the classes the merge table emits. **Pass 1 (P1-31, S1-32, A1-43):** `DATA` has
    no code ([F12 §6.1]) and is no longer named here (§7.10).
21. **Conflict sides are flat.** A conflict value's sides are never conflict values ([RULES/merge-table] open point 18);
    `Conflict.old` and `Resolve.old` may be. If the review rejects the flattening, a nested side needs a `cstate` in place
    of the `kval` in §6.2 before the freeze.
22. **Net form and op order are V-rules where decidable.** One op per key, image and delete folding, and the op order are
    checked by every decoder, so a decode followed by a re-encode is byte-identical (E3) and one net changeset has one
    stored encoding per store.
23. **`seq` above 2^32 − 1 is invalid on read** (§4.4.3), matching the write refusal of [AR §4.5] step 4.
24. **`ref_seq` starts at 1** and the absorbed vector omits the commit's own ref (§4.4.2, §4.4.9); [F11]'s `ref_seq_next`
    must start at 1.
25. **The `stmt_origin`/`import` link.** `stmt_origin = import` exactly when `import ≠ local` (V). Other origin/kind pairs
    are C-rules, so that a later verb class needs no format change.
26. **One value encoding** (pass 1, P1-1, S1-1, A1-1). §5's tag table (tags 0–14, `absent`/`false`/`true` as tags, `ref`
    as `uvar32`, `commit-ref` 32 bytes, empty text and empty set as values, `pathmove` classes 0–3) is withdrawn. Values
    in ops are [F08 §5]'s bytes: `absent` is type byte 0, `bool` is carried in the type byte, `ref` is `u32`, `commitref`
    is 32 bytes in [F08] too, empty is absent everywhere, NaN, ±infinity and −0.0 are invalid, `pathmove` classes are 1–4,
    and sets use [F08 §5.5]'s order. [F07 §7.1]'s canonical tags are [F07]'s own and are not affected.
27. **`prov` in conflict states** (pass 1, S1-5, A1-9): §6.2 and §7.7 carry [F12 §6.3]'s provisional-side byte after
    `theirs`, for existence keys only; [F07 §7.3] hashes it and [F11 §10] stores it.
28. **`deleted` existence values carry the kind** (§6.2), matching [F07 §7.2]'s `deleted(kind, reason, replaced_by)`, so a
    conflict side or a `CONFLICTS` row decodes to its canonical value alone (S1-7).
29. **`CreateDeleted`** (op 16, §7.4, NF-11; pass 1, S1-6, A1-4). The transition absent → deleted, which a merge or sync
    of a lane that created and deleted a node and a checkpoint import produce, had no op. A new op tag was chosen over a
    flag on `Delete`, so `Delete`'s layout and its NF-5 folding stay as they were. Its inverse is none (§7.10).
30. **The header-only form** (§4.4.15, presence bit 16; pass 1, S1-21, A1-5) and **the image-only data of an
    import-checkpoint** (§4.4.14, presence bit 17, order 44; pass 1, S1-23, A1-10). The latter lets gate 2 reproduce a
    checkpoint tree's provenance and ledger lines; [F09 §16.4] carries the same entries for a bulk checkpoint (BK-5).
31. **Key classes share [F07]'s numbering** (§6.1; pass 1, A1-46). The `ckey` classes were 0–8 in another order; they are
    now [F07 §6.1]'s codes 1–8 with `schema` 9, and the op order's ranks follow. [F12 §6.2] cites the new values; [F11 §10]
    stores `ckey` bytes and needs no change of text.
32. **The HLC of local commits** (§4.4.4, §4.4.5; pass 1, P1-5, S1-13, A1-17) follows [API §6.2] CK-4, which [F16] P-36
    and [OS/clock §7] adopt: only semantic durable records advance the sequence, and a local commit's `hlc` is above every
    commit the store holds. **Closed** (pass 1, round 1): P-36 states the rule over the maxima `hlc_seq` and `hlc_commit`
    ([F04 §5.15]).
33. **`cs_ref.b3`** is `seg_digest[0..16]` (BK-2; pass 1, P1-6, S1-19, A1-21), so the open check compares it without
    reading the file.
34. **Pass 1, round 2** (closure NC-6, NC-7). §2.4: a V-rule break is a malformed payload of a valid record, corrupt
    wherever it lies ([F05 §5.4]; R-SPEC-P's edit, kept), and §4.1 reads its decoder rule through it. §4.2 bit 17: an
    inline checkpoint sets `ckimg` exactly when its tree holds a node file that differs from the parent checkpoint's
    tree, the condition of §4.4.14's `n_files` ≥ 1 and of [F09 §16.4] `CKIMG`, so every inline checkpoint has one
    encoding.
35. **The inverse `Delete` of a reverted `Create`** (§7.10; pass 1, round 3, closure NC-9). §7.10 said `Create` ↔ `Delete`
    without the `Delete`'s `reason` and `replaced_by`, hashed bytes of canonical item 10 ([F07 §13]), so an engine and the
    reference model could write different tombstones for the revert of any creating commit and their commit ids would
    differ (GT2). Decided here: `reason` 0 (empty), `replaced_by` 0 (none), the values [RULES/merge-table] open point 33
    proposes and DM-017 reads. §7.10 also states that a `Delete`'s inverse is an `Undelete` ([AR §5d.3],
    [RULES/delete-policy-matrix §9]); "`Create` ↔ `Delete` … and back" read as if a revert of a delete re-created the
    node.
36. **Spec sync 2b.** Presence bit 18 and order 45, `stage` (§4.4.16): a staged `merge` or `sync` records its
    command's `--base`, policy override and effective `strict`, which [F12 §9.4] step 1 re-uses (the contested row of
    [F12] open point 30 (a)); absent means none, none and false, so the staged commit of `fixtures/hex/` store A stays
    valid. NF-3 stores a `Resolve` on a staging ref even when `new` equals `old`. `Resolve.choice` 5 `drop` (a flagged
    edge, [F12 §6.5]). `item_key` is [F08 §8.5]'s stored key form (names joined by `00`), and `item_class` 6 is a policy
    row. `commit_id` and `changeset_digest` are tagged C (their correctness needs the symbols and states the record
    names; [F07 §12]). The default idempotency key hashes the command's branch too ([API §7.2]).
