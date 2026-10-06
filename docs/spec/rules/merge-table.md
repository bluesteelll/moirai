# Merge table: key class → merge rule → conflict class

| Field | Value |
|---|---|
| Status | draft, pass 1 pending |
| Work package | WP-90 (R-MODEL); consumed by WP-91 (merge, sync, revert, cherry-pick, the recursive virtual base) and WP-94 (suites) |
| Sources | [AR §2.5] T5; [AR §2.7] T7; [AR §2.12] T12; [AR §3.1]–[AR §3.6] data model; [AR §4.3] ops; [AR §4.5] step 4 markers; [AR §4.6] item 10 key classes; [AR §5a.1]–[AR §5a.8] VCS, merge algorithm and taxonomy; [AR §5b.6] steps 3–4 import merges; [AR §5d.1]–[AR §5d.3] runtime state and node 40; [AR §13] `merge.strict`, `merge.policy.<kind>`; [40 §2.2]–[40 §2.4], [40 §5.5] (link rules, in [RULES/link-merge-rules]); [50 §4.4] named queries; [50 §8.1] F3, F18; [60 §2.5] schema as data; [60 §3.4] the recursive virtual base and GT6; [60 §4.2], [60 §4.4] the model's algorithm and comparison; [72 M4], [72 M5] |
| Format | [RULES/README] |
| Cited as | [RULES/merge-table]; a row as [RULES/merge-table MR-024] |

## 1. What this table decides

For every key of a three-way merge — a merge, a `sync`, a revert, a cherry-pick, a merge inside the recursive virtual
base, or an import merge — this file decides:

- which **merge class** the key belongs to (`field-class`, `edge-class`, `merge-classes`);
- which **rule** gives the merged value (`merge-rules`, with `status-lattice`, `existence-policy` and `auto-policy`);
- which **conflict or violation class** the rule emits, and whether the merge lands or stages (`class-map`,
  `validators`, `land-or-stage`);
- how the **base** is chosen when there are several LCAs (`virtual-base`);
- what happens to **runtime state** that is never merged: leases, markers, absorbed vectors (`runtime-effects`).

The R4 link classes (`identity`, `observation`, `alias-set`, `glob-set`, `pathmove-set`, `derived-existence`, `anchor`)
have their rules in [RULES/link-merge-rules]; this file assigns fields to them and runs them inside the same procedure.

The reference model evaluates the decision tables as data ([60 §4.2]: "the merge table transcribed as data,
owner-signed") over three materialised states, never over op folds; the engine must produce the same keys, values,
conflicts, violations and land-or-stage decision ([60 §4.4] item 5). The typed merge rules are never a configuration key
([AR §13] "Never a key"); only `merge.strict` and the policy data `merge.policy.<kind>` vary them, and the model
implements every allowed value (AP rows).

## 2. How to read a row

**Inputs.** A merge has a destination *dst* (the ref being merged into, or the left operand of a virtual merge) and a
source *src*. For one key k:

- **o** ("ours") is k's value in the state at tip(dst);
- **t** ("theirs") is k's value in the state at tip(src);
- **b** is k's value in the base state: the single LCA, the recursive virtual base (§11) or `--base`.

A key is a key of the canonical net changeset ([AR §4.6] item 10): a field `(uid, field)`, the status `(uid, status)`
with its resolution, a counter `(uid, field)`, the body `(uid, body)`, an edge `(src uid, kind, dst uid, disc)`, the
existence `(uid)`, the hierarchy `(uid) → (parent uid, order)`, a schema item, or a conflict value. Two exceptions,
both from [40 §2.2] and [AR §3.1]: the six observation fields of an `artifact` are **one** merge key, and a struct
field whose components have their own FC rows (`applies_to`) is merged per component.

**Values.** `absent` is a value. Two values are equal if and only if their canonical encodings ([F07]) are
byte-equal ([F12 §7.3]). A conflict value `{class, base, ours, theirs}` is a value like any other. On an existence key it
also carries its provisional side `prov` (`ours` or `theirs`, set by RS-008, RS-010 or RS-015 from the policy in force),
and a `live` side carries its node image ([F06 §6.2]); both are part of the value and of its equality ([F12 §6.3],
[F12 §7.3]). Where a plain value of a conflicted key is needed (derived state, validators, RE-009), the rules use its
provisional value: the side `prov` names on an existence key; on every other key `ours`, or `theirs` when `ours` is
`absent` ([F12 §6.3]). When b is a conflict value, the `conflicted-key` rows compare b, o and t by [F12 §5.4]'s ≈
instead: two conflict values with equal class and `base` whose `ours` and `theirs` are exchanged (on an existence key,
with opposite provisional sides) are also equal, so a side that left its own merge's conflict unresolved matches the
virtual base's conflict value in either orientation (MC-001; spec sync 2b).

**Evaluation.** PR rows give the order: re-keys, then existence keys, then every other key. For a key of class C the
model evaluates the `conflicted-key` rows first if b, o or t is a conflict value, and otherwise the rows of C in table
order; the first row whose `case` holds decides `result`, `conflict` and `disposition`. Every class's rows are
exhaustive (the last row's case is `both` or `any`).

**Dispositions.**

- `clean`: the key takes `result` and nothing is recorded.
- `value`: a value conflict; the key holds a conflict value and the node gets `conflicted`; the merge lands unless
  strict (LS rows) ([AR §5a.8] "value conflict").
- `structural`: a structural violation; the merge never advances dst and stages on `merge/<dst>/from/<src>`
  ([AR §5a.8] "structural violation").
- `hint`: a log line; the merge lands.
- `gap`: the design has no rule; the model raises `SpecGap(<row>)` ([RULES/README] §5).
- `none`: the row emits nothing (procedure rows).

**Kinds in FC and EP rows.** `*` means every kind; `doc/section` is a `doc` node with `doc_kind = section`;
`area/root` is the root node of R4 (an `area` whose schema row says `uid_derivation = root-key`, [40 §2.4]). The most
specific row wins: `area/root` over `area` over `*`.

**Basis.** `design`, `derived`, `proposed`, `gap` as defined in [RULES/README] §5. Every `proposed` and `gap` row
cites the open point that explains it.

## 3. Merge classes

<!-- table: merge-classes -->
| row | class | key_class | rules_file | basis | source | note |
|---|---|---|---|---|---|---|
| MC-001 | `conflicted-key` | conflict | merge-table | derived | [AR §4.6] item 10; [AR §5a.7] step 1; [60 §3.4] | Pre-class: its rows run before the key's own class whenever b, o or t is a conflict value, so conflict values merge like other values and the recursive-virtual-base rule of [60 §3.4] has one home. When b is a conflict value, its cases compare by [F12 §5.4]'s ≈ (§2; spec sync 2b). |
| MC-002 | `scalar` | field | merge-table | design | [AR §2.7]; [AR §5a.7] step 4 row "enum / number scalar" | Whole-value three-way merge: enums, numbers, booleans, refs, commit refs, paths, oids, symbols, single-line strings, and the list and struct values FC rows mark atomic. |
| MC-003 | `owner` | field | merge-table | design | [AR §2.7]; [AR §5a.7] step 4 row "owner-authority fields" | `owner_quote`: `main` wins when dst is `main`, otherwise `OwnerFieldEdited`. The scope of "owner-authority fields" is [OP-1]. |
| MC-004 | `authority` | field | merge-table | design | [AR §5a.7] step 4 row "owner-authority fields"; [AR §3.1] | The `authority` column: the owner rule when the value `owner` is involved, the scalar rule otherwise. |
| MC-005 | `status` | status | merge-table | design | [AR §2.7]; [AR §5a.7] step 4 row "status"; [AR §4.6] item 10 | Status and resolution are one key; the per-kind merge order is in `status-lattice`. |
| MC-006 | `counter` | counter | merge-table | design | [AR §2.7]; [AR §5a.7] step 4 row "counter"; [AR §3.1] | Fields whose merge rule is `Incr`; never conflicts. |
| MC-007 | `set` | field | merge-table | design | [AR §2.7]; [AR §5a.7] step 4 row "set" | Add-wins with removals relative to the base; never conflicts. |
| MC-008 | `text` | field, body | merge-table | design | [AR §2.7]; [AR §5a.7] step 4 row "text" | Line diff3 against the base: multi-line text fields and every body except doc sections. |
| MC-009 | `section-text` | body | merge-table | design | [AR §5a.7] step 4 row "text" | Bodies of doc sections: text plus the removed-text guard. |
| MC-010 | `hierarchy` | hierarchy | merge-table | design | [AR §2.7]; [AR §5a.7] step 4 row "parent / order"; [AR §4.6] item 10 | The key (uid) → (parent uid, order): Kleppmann moves in HLC order. |
| MC-011 | `existence` | existence | merge-table | design | [AR §2.5]; [AR §5a.7] step 4 row "existence"; [AR §5d.3] | Nodes of kinds whose uid derivation is `random`. |
| MC-012 | `edge` | edge | merge-table | design | [AR §3.3]; [AR §5a.7] step 4 rows "structural edge …", "supersedes", "pinned_commit" | Every edge kind except `parent` (hierarchy) and `at` (anchor). Value: present with its properties, or absent. |
| MC-013 | `schema-item` | schema | merge-table | design | [AR §2.12]; [AR §5a.7] step 4 row "schema"; [60 §2.5] "Schema as data" | One row of a schema table: a kind, a field of a kind, an enum value of a field, an edge kind, or a policy row ([F08 §8.5.6]; spec sync 2b). |
| MC-014 | `query` | schema | merge-table | design | [50 §4.4]; [50 §8.1] F3; [AR §5a.7] step 4 row "R5 named-query definition" | One project named query, an atomic value (signature, shape, budget, text). |
| MC-015 | `identity` | field | link-merge-rules | design | [40 §2.2]; [40 §2.11] R-2 | Immutable identity inputs of derived-uid nodes. |
| MC-016 | `observation` | field | link-merge-rules | design | [40 §2.2]; [40 §5.5]; [40 §2.11] R-2 | The artifact observation composite: one merge key over `path`, `oid`, `bytes`, `observed_git`, `observed_blob`, `relink`. |
| MC-017 | `alias-set` | field | link-merge-rules | design | [40 §2.2]; [40 §5.5] | `aliases`: add-wins, plus the pre-composition paths of a composition. |
| MC-018 | `glob-set` | field | link-merge-rules | design | [40 §2.4]; [40 §5.5]; [72 M5] | Path globs: add-wins, entries composed through the other side's directory moves. |
| MC-019 | `pathmove-set` | field | link-merge-rules | design | [40 §2.4]; [40 §5.5]; [40 §2.11] R-5 | The root node's `path_moves`: add-wins union. |
| MC-020 | `derived-existence` | existence | link-merge-rules | design | [40 §2.3]; [40 §5.5]; [40 §2.11] R-3 | Nodes of kinds whose uid derivation is `file-key` or `root-key`. |
| MC-021 | `anchor` | edge | link-merge-rules | design | [40 §2.7]; [40 §5.5]; [40 §2.11] R-4, R-10 | `at` edges: the anchor uid is the discriminator and the value is the anchor selector block. |
| MC-022 | `derived` | none | none | design | [AR §3.5]; [AR §3.4] I9; [AR §5a.7] step 5 | Derived columns and flags are never merged; they are recomputed from the merged primary data (RE-009). |
| MC-023 | `runtime` | none | merge-table | design | [AR §5d.1]; [AR §3.4] I36′ | Store-level runtime state is never merged; its merge effects are the RE rows. |
| MC-024 | `none` | none | none | derived | [AR §3.1]; [AR §4.6] "Not hashed" | Bookkeeping columns and virtual fields that are not keys of the canonical changeset. |

## 4. Cases and results

<!-- table: cases -->
| row | case | classes | definition |
|---|---|---|---|
| CS-001 | `same` | * | o = t. Two `absent` values are equal. An empty value is `absent` in every state the rules read or produce: the empty text, the empty set and a counter of 0 are never values ([F08 §5.3], [F08 §6.2]; an empty body is no body, [F08 §7.2]). |
| CS-002 | `ours-only` | * | o ≠ b and t = b. |
| CS-003 | `theirs-only` | * | t ≠ b and o = b. |
| CS-004 | `both` | * | o ≠ b, t ≠ b and o ≠ t. After `same`, `ours-only` and `theirs-only` it is the only case left, so a class whose last row is `both` or `any` is exhaustive. |
| CS-005 | `any` | counter, set, hierarchy, alias-set, glob-set, pathmove-set | Always true. |
| CS-006 | `base-conflicted` | conflicted-key | b is a conflict value, o ≉ b, t ≉ b and o ≉ t (≈ of [F12 §5.4], §2): both sides changed the key since a conflict-valued base, to different values ([F12 §5.4] RVB-4). In practice the recursive virtual base of a criss-cross holds a conflict value on this key ([60 §3.4]). |
| CS-007 | `both-forward-comparable` | status | `both`, and: neither o nor t is a side state (SL `side = yes`); b is `absent` or a non-side state; b < o and b < t in the kind's merge order (the transitive closure of the SL `covers` relation, with `absent` below every non-side state); o and t have different statuses; and those two statuses are comparable. Resolutions travel with their status and play no part in the order. |
| CS-008 | `dst-main-changed` | owner | dst is the ref `main` (never true inside a virtual merge, VB-007) and o ≠ t. |
| CS-009 | `dst-main-owner-involved` | authority | dst is the ref `main` (never inside a virtual merge), o ≠ t, and at least one of b, o, t is `owner`. |
| CS-010 | `both-owner-involved` | authority | `both`, and at least one of b, o, t is `owner`. |
| CS-011 | `both-diff3-clean` | text, section-text | `both`, and diff3 of (b, o, t) over the line diff of [F12] has no conflicting hunk and its result is at most 65,536 bytes, the bound of a text value and of a body ([F12 §7.5] "Length"; [F08 §5.3], [F08 §7.2]); `absent` is read as the empty text. A clean diff3 whose result is longer falls to the class's `both` row (`TextHunk`). On a body key, it also needs that none of b, o and t is a dropped body, a hash in the store's dropped set (PR-016; [F06 §8.1] DB-1): a dropped body has no readable lines, so diff3 is not run and the key falls to the class's `both` row, a `TextHunk` whose sides are the three hashes, as a result over the length bound does ([F12 §7.5] "A dropped body"; spec sync 3). Every other case of a body key compares hashes and reads no dropped set. |
| CS-012 | `both-guard-fail` | section-text | `both-diff3-clean`, and the removed-text guard fails for the diff3 result R: for S = o or S = t, the multiset of S's lines minus the multiset of R's lines is not contained in the multiset of b's lines (text a side had, the merge dropped, and the base never held). |
| CS-013 | `kleppmann-skipped` | hierarchy | The `kleppmann` result (RS-007) undid this key's last move, because the step that made it left a node its own ancestor. A move that set its node's current value is never undone, so it never makes its key `kleppmann-skipped` (RS-007; [OP-35] case (iii), spec sync 3). |
| CS-014 | `deleted-vs-modified` | existence, derived-existence | b is live; one side's value is deleted; the other side's value is live, and that side changed at least one other key of the same uid since the base: a field, status, counter, body or hierarchy key of the uid, or an out-edge key whose source is the uid. In-edges belong to their sources and do not count; they meet VA-004 or become tombstone references ([AR §5d.3]). |
| CS-015 | `both-created` | existence | b is `absent`, and o and t are both live: one uid created on both sides. |
| CS-016 | `both-present` | edge, anchor | `both`, and o and t are both present: they differ only in their properties or anchor selectors. |
| CS-017 | `both-strengthen` | schema-item | `both`, and at least one of the changes b → o and b → t is a strengthening: any change other than adding a kind, a field or an enum value ([AR §2.12]). |
| CS-018 | `dropped-vs-modified` | query | `both`, and exactly one of o and t is `absent`: a `DROP QUERY` on one side and a change on the other. |
| CS-019 | `both-same-ast` | query | `both`, o and t are both definitions, and their canonical-AST hashes over the portable form are equal ([50 §4.4]). |

<!-- table: results -->
| row | result | definition |
|---|---|---|
| RS-001 | `take-o` | The key's value is o. |
| RS-002 | `take-t` | The key's value is t. |
| RS-003 | `join` | The greater of o and t in the kind's merge order; the resolution travels with the status that wins. |
| RS-004 | `sum` | o + t − b, each `absent` read as 0, in checked i64 arithmetic; a sum of 0 is `absent` ([F07 §6.3], [F08 §6.2]). An overflow is a specification finding: the model panics. |
| RS-005 | `union3` | (b ∩ o ∩ t) ∪ (o − b) ∪ (t − b), each `absent` read as the empty set: an element survives unless a side removed it, and an element either side added is kept. Sorted as the set type's canonical encoding requires; an empty result is `absent` ([F08 §5.3]). |
| RS-006 | `diff3` | The merged text of the clean diff3 of CS-011; an empty result is `absent`, for a text field and for a body alike ([F12 §7.5] "Empty result"). |
| RS-007 | `kleppmann` | Computed once per merge for all hierarchy keys, in steps ([F12 §5.3] VM-7, [F12 §7.4]). A commit's step keys are the hierarchy keys of its canonical net changeset against its first parent (the full state diff for a `sync`, [AR §4.6]); a two-parent commit M (a merge or a `sync`) with parents p₁ and p₂ also has each hierarchy key whose value in state(M) differs from its value in state(p₂) and that is a step key of some commit of A(p₂) \ A(p₁), so M's step re-asserts what M kept against the merged branch's moves, and nothing else ([AR §11] OQ-A-6 (a): case (i) of [OP-35] in its narrow form). A commit's step keys depend only on the commit graph, so an engine may compute them once per commit and keep them (PR-016 lists the commits whose step keys a merge reads). For a merge or a `sync` (and a virtual merge, VB-011): start from b's (parent, order) for every node. Each commit of A(o) \ A(B) and of A(t) \ A(B) (the ancestor sets of [F12 §5.2]) that has step keys is one step, keyed by its (hlc, commit id); the step sets each of its step keys to its value in that commit's state. For each side, a hierarchy key whose value on that side differs from b while no step of that side sets it (the base of `--base` or a virtual base is not where that side's commits start) is set to that side's value in a step keyed (0, 0), before every commit. For a revert or a cherry-pick of a commit C (DM-003 to DM-005; [OP-35] case (ii)): start from o's (parent, order) for every node; dst has no step and neither side has a (0, 0) step; src has one step, keyed by C's (hlc, commit id), which sets each hierarchy key whose value in src's state differs from b (the hierarchy keys of C's net changeset against its first parent) to src's value, leaving out each key that is a step key of a commit of A(o) ordered after C by (hlc, commit id): that later move stands (MR-040). Apply the steps in ascending (hlc, commit id) order, commit ids compared bytewise, all moves of a step at once; where two steps share a key (a commit that is a step of both sides, or the two sides' (0, 0) steps), dst's moves apply first, then src's. A move that sets its node's current value (its value just before the step) changes nothing: it is never undone and never makes its key `kleppmann-skipped` ([OP-35] case (iii)). After a step in which a node is its own ancestor, undo the step's moves that changed their node's value one at a time, the least uid among those moves' nodes that lie on a cycle first, until none is (MR-039); every such cycle holds one of them, because the state before each step is a forest. A key whose last move was undone is `kleppmann-skipped` (CS-013); a later move of it that applied decides its value (MR-040). Each key's value is its node's final (parent, order). Step keys follow the re-key (RK-005, RK-006). Keys of a uid fixed by an existence policy (PR-007) take no part. When tip(dst) is B (dst made no commit since B) and src's commits since B (A(t) \ A(B)) are a linear chain of single-parent commits whose first has B as its parent, the steps replay src's own states in order, each a forest, so the merge meets no cycle and every hierarchy key takes src's value. A revert or a cherry-pick of a commit whose net changeset against its first parent has no hierarchy entry leaves every hierarchy key at o's value. Outside these cases the steps can still stage a `HierarchyCycle` that the sides' histories do not call for; [OP-35] lists the known cases, those the owner decided and those still open (spec sync 3). |
| RS-008 | `policy` | `DeleteVsModify` on the existence key. The key holds the conflict value {DeleteVsModify, base b, ours o, theirs t}. The node's other keys take, provisionally, the values of the side the policy names: `delete-wins`, the deleting side (the node is deleted and keeps only its retained out-edges, [AR §3.4] I39′); `resurrect`, the modifying side (the node is live with that side's keys); `none`, dst's side. The policy is the kind's EP row unless `--policy` or `merge.policy.<kind>` overrides it (AP rows). No other conflict is emitted for the node's keys. A `live` side of the conflict value carries that side's node image, its value keys only ([F06 §6.2] `snap` = 1). A `resolve --take` towards a live side restores the value keys from that image, and the node's hierarchy key and out-edges from that side's state at the conflict's introducing commit M (the state of M's first parent for `ours`, of its second parent for `theirs`), as ordinary `Move`, `AddEdge` and `SetEdgeProps` ops of the `Resolve` commit, under the write path's checks ([F12 §6.5]; [OP-5]). |
| RS-009 | `conflict-value` | The key holds the conflict value {class, base b, ours o, theirs t}, the class taken from the row's `conflict` cell; the node gets `conflicted` (RE-008). |
| RS-010 | `conflict-as-class` | As `conflict-value`, with base b′, ours flat(o) and theirs flat(t) ([F12 §5.4] RVB-4): b′ is the `base` field of the conflict value b, and flat(v) is v when v is plain and v's provisional value ([F12 §6.3], §6.4) when v is itself a conflict value, so a conflict value never nests. Its class is found by evaluating the key's own class rows on (b′, flat(o), flat(t)), skipping every row whose disposition is not `value`: the first matching row's `conflict` cell is the class, or the class of b when no such row matches ([OP-18]). When the class is `DeleteVsModify` on an existence key, the node's other keys take RS-008's provisional state if exactly one of flat(o), flat(t) is live, and o's values otherwise; the conflict value's provisional side follows ([F12 §5.4], [F12 §6.3]). |
| RS-015 | `conflict-plain-base` | As `conflict-value`, with base b, ours flat(o) and theirs flat(t) (flat as in RS-010): b is plain and at least one of o, t is a conflict value ([F12 §5.4] "A plain base with a conflict-valued side"). Its class is found by evaluating the key's own class rows on (b, flat(o), flat(t)), skipping every row whose disposition is not `value`: the first matching row's `conflict` cell is the class, or, when no such row matches, the class of the side that holds a conflict value (o's when both do). An existence key's provisional state follows RS-010's last sentence. The conflicted side's own conflict value stays readable as the `old` of the new `Conflict` op ([F12 §6.4]). |
| RS-011 | `keep-defined` | `DeleteVsModify` on a named query: the modified definition stays provisionally, and the key's conflict value records {base b, ours o, theirs t}. |
| RS-012 | `stage-take-o` | A structural violation: the staged candidate carries o for this key, and the merge commit on the staging ref holds one `Violation` op naming the key and b, o, t. |
| RS-013 | `stage-diff3` | A structural violation after a clean diff3: the staged candidate carries the diff3 result (`absent` when empty, as RS-006), and a `Violation` op names the key and b, o, t. |
| RS-014 | `gap` | No rule is specified: the model raises `SpecGap(<row id>)`. |

## 5. The merge rules

Rows are grouped by class; within a class the first matching row wins.

<!-- table: merge-rules -->
| row | class | case | result | conflict | disposition | basis | source | note |
|---|---|---|---|---|---|---|---|---|
| MR-001 | `conflicted-key` | `same` | `take-o` | - | clean | design | [60 §3.4]; [AR §5a.7] step 1; [F12 §5.4] RVB-1 | Two sides holding the same value never conflict, whatever the base holds: "resolved identically never conflict". A conflict value held by both sides stays, and no new `Conflict` op is emitted. Over a conflict-valued base the sides compare by ≈ (§2), so two exchanged orientations of one conflict are the same (spec sync 2b). |
| MR-003 | `conflicted-key` | `ours-only` | `take-o` | - | clean | derived | [AR §5a.7] step 4; [AR §3.4] I25′; [F12 §5.4] RVB-3 | src did not touch the key since the base: dst's value stays. At a real LCA: a conflict value landed on dst stays, and a resolution made on dst stays. Over a conflict-valued (virtual) base: dst resolved it and src still holds the base's conflict value, in either orientation (≈, §2; spec sync 2b), so dst's resolution stands ([OP-18]). |
| MR-004 | `conflicted-key` | `theirs-only` | `take-t` | - | clean | derived | [AR §5a.7] step 4; [AR §3.4] I25′; [F12 §5.4] RVB-2 | dst did not touch the key since the base: src's value lands. At a real LCA: a conflict value landed on src is carried to dst (into `main` only as PR-003 allows), and a resolution made on src lands. Over a conflict-valued (virtual) base: src resolved it and dst still holds the base's conflict value, in either orientation (≈, §2; spec sync 2b), so src's resolution lands ([OP-18]). |
| MR-002 | `conflicted-key` | `base-conflicted` | `conflict-as-class` | - | value | design | [60 §3.4]; [AR §2.7]; [AR §5a.7] step 1; [F12 §5.4] RVB-4; [OP-18] | The base holds a conflict value and both sides changed the key since it, to different values: "two sides that resolved a criss-cross differently always conflict". A side that still holds the base's conflict value did not touch the key, so MR-003 or MR-004 decides it first (I25′). The class, the sides and the base come from RS-010. |
| MR-005 | `conflicted-key` | `both` | `conflict-plain-base` | - | value | derived | [F12 §5.4] "A plain base with a conflict-valued side"; [AR §5a.7]; [OP-18] | A plain base, both sides changed the key, and at least one side holds a conflict value. The design has no rule; [F12 §5.4] states RVB-4's flat form over the plain base (review pass 1, P1-21), so no merge input reaches `SpecGap`. The class, the sides and the base come from RS-015. |
| MR-006 | `scalar` | `same` | `take-o` | - | clean | design | [AR §5a.7] step 4 row "enum / number scalar" | "equal → take". |
| MR-007 | `scalar` | `ours-only` | `take-o` | - | clean | design | [AR §5a.7] step 4 row "enum / number scalar"; [AR §3.4] I25′ | "one side = base → take the other". |
| MR-008 | `scalar` | `theirs-only` | `take-t` | - | clean | design | [AR §5a.7] step 4 row "enum / number scalar"; [AR §3.4] I25′ | As MR-007. |
| MR-009 | `scalar` | `both` | `conflict-value` | FieldEdit | value | design | [AR §5a.7] step 4 row "enum / number scalar"; [AR §5a.8] | `FieldEdit{base, ours, theirs}`. |
| MR-010 | `owner` | `same` | `take-o` | - | clean | design | [AR §5a.7] step 4 row "owner-authority fields" | - |
| MR-011 | `owner` | `dst-main-changed` | `take-o` | - | clean | design | [AR §5a.7] step 4 row "owner-authority fields"; [AR §5d.2] | "`main` wins when dst = main": an edit of an owner field never reaches `main` through a merge. Nothing records the dropped change ([OP-1]). |
| MR-012 | `owner` | `ours-only` | `take-o` | - | clean | derived | [AR §3.4] I25′; [OP-1] | dst is not `main`: a one-sided change lands, because I25′ forbids a conflict on a key one side did not touch; "otherwise conflict" is read as "both changed". |
| MR-013 | `owner` | `theirs-only` | `take-t` | - | clean | derived | [AR §3.4] I25′; [AR §5d.2]; [OP-1] | As MR-012; this is how an owner ruling written on `main` reaches a lane at `sync`. |
| MR-014 | `owner` | `both` | `conflict-value` | OwnerFieldEdited | value | design | [AR §5a.7] step 4 row "owner-authority fields"; [AR §5a.8] | dst is not `main`. |
| MR-015 | `authority` | `same` | `take-o` | - | clean | design | [AR §5a.7] step 4 row "owner-authority fields" | - |
| MR-016 | `authority` | `dst-main-owner-involved` | `take-o` | - | clean | design | [AR §5a.7] step 4 row "owner-authority fields" | "`authority = owner`": `main` wins. |
| MR-017 | `authority` | `ours-only` | `take-o` | - | clean | derived | [AR §3.4] I25′ | - |
| MR-018 | `authority` | `theirs-only` | `take-t` | - | clean | derived | [AR §3.4] I25′ | - |
| MR-019 | `authority` | `both-owner-involved` | `conflict-value` | OwnerFieldEdited | value | design | [AR §5a.7] step 4 row "owner-authority fields"; [AR §5a.8] | dst is not `main`. |
| MR-020 | `authority` | `both` | `conflict-value` | FieldEdit | value | derived | [AR §5a.7] step 4 row "enum / number scalar" | Neither side nor the base is `owner`: the scalar rule. |
| MR-021 | `status` | `same` | `take-o` | - | clean | design | [AR §5a.7] step 4 row "status" | - |
| MR-022 | `status` | `ours-only` | `take-o` | - | clean | derived | [AR §3.4] I25′; [AR §5d.2] | A one-sided change lands, backward moves included: a `reopen` is an explicit op on its own side and never a merge artefact. |
| MR-023 | `status` | `theirs-only` | `take-t` | - | clean | derived | [AR §3.4] I25′ | As MR-022. |
| MR-024 | `status` | `both-forward-comparable` | `join` | - | clean | design | [AR §5a.7] step 4 row "status"; [AR §5d.2] | "join if both are forward moves and comparable": `in_progress` vs `done` gives `done`; `planned` vs `present` gives `present`. |
| MR-025 | `status` | `both` | `conflict-value` | StatusFork | value | design | [AR §5a.7] step 4 row "status"; [AR §5a.8]; [AR §3.2]; [OP-3] | Side state vs forward move, incomparable states (`confirmed` vs `refuted`), `reopen` vs `done`, `present` vs `removed`; also one status carried with two different resolutions ([OP-3]). |
| MR-026 | `counter` | `any` | `sum` | - | clean | design | [AR §5a.7] step 4 row "counter"; [AR §2.7] | "sum of both deltas over base"; never conflicts (X11). |
| MR-027 | `set` | `any` | `union3` | - | clean | design | [AR §5a.7] step 4 row "set" | "add-wins union with removals relative to base"; never conflicts. |
| MR-028 | `text` | `same` | `take-o` | - | clean | design | [AR §5a.7] step 4 row "text" | - |
| MR-029 | `text` | `ours-only` | `take-o` | - | clean | derived | [AR §3.4] I25′ | - |
| MR-030 | `text` | `theirs-only` | `take-t` | - | clean | derived | [AR §3.4] I25′ | - |
| MR-031 | `text` | `both-diff3-clean` | `diff3` | - | clean | design | [AR §5a.7] step 4 row "text"; [OP-13] | "line diff3 against the base blob", over [F12]'s line diff. |
| MR-032 | `text` | `both` | `conflict-value` | TextHunk | value | design | [AR §5a.7] step 4 row "text"; [AR §5a.8]; [F12 §7.5] "Length", "A dropped body"; [F06 §8.1] DB-7 | The conflict value holds the three whole texts; packs render the base text (N15). Also reached by a clean diff3 whose result exceeds 65,536 bytes (CS-011; [F12 §7.5] "Length"), and on a body key whenever b, o or t is a dropped body, where diff3 is not run: the sides are the three hashes, and a dropped side renders as [F06 §8.1] DB-6 says (CS-011; spec sync 3). |
| MR-033 | `section-text` | `same` | `take-o` | - | clean | design | [AR §5a.7] step 4 row "text" | - |
| MR-034 | `section-text` | `ours-only` | `take-o` | - | clean | derived | [AR §3.4] I25′ | - |
| MR-035 | `section-text` | `theirs-only` | `take-t` | - | clean | derived | [AR §3.4] I25′ | - |
| MR-036 | `section-text` | `both-guard-fail` | `stage-diff3` | RemovedTextNotInBase | structural | design | [AR §5a.7] step 4 row "text"; [AR §5a.8]; [OP-14] | "`doc.section` bodies also run the removed-text guard"; the guard's formula (CS-012) is [OP-14]. |
| MR-037 | `section-text` | `both-diff3-clean` | `diff3` | - | clean | design | [AR §5a.7] step 4 row "text" | - |
| MR-038 | `section-text` | `both` | `conflict-value` | TextHunk | value | design | [AR §5a.7] step 4 row "text"; [F12 §7.5] "A dropped body"; [F06 §8.1] DB-7 | The guard runs only on a clean diff3; a `TextHunk` value keeps all three texts and drops nothing. Also reached by a clean diff3 whose result exceeds 65,536 bytes (CS-011), and whenever b, o or t is a dropped body: diff3 is not run, so neither MR-036's guard nor MR-037 applies, and the sides are the three hashes (CS-011; spec sync 3). |
| MR-039 | `hierarchy` | `kleppmann-skipped` | `kleppmann` | HierarchyCycle | structural | design | [AR §5a.7] step 4 row "parent / order"; [AR §5a.8]; [AR §11] OQ-A-6; [OP-15]; [OP-35] | "a cycle-creating move is skipped and logged": RS-007 undoes it; `HierarchyCycle` is structural, so the merge stages. A move that sets its node's current value closes no cycle and is never undone (RS-007; [OP-35] case (iii), spec sync 3). |
| MR-040 | `hierarchy` | `any` | `kleppmann` | - | clean | design | [AR §2.7]; [AR §5a.7] step 4 row "parent / order"; [AR §11] OQ-A-6; [OP-35] | Two different moves of one node do not conflict: the later one in (hlc, commit id) order wins. A two-parent commit's step re-asserts only the keys it kept against its second parent's branch, so a side's own earlier move keeps its own (hlc, commit id) against a third branch; a revert or a cherry-pick leaves out C's move of a key that a dst commit after C moved (RS-007; [OP-35] cases (i) and (ii), spec sync 3). |
| MR-041 | `existence` | `same` | `take-o` | - | clean | design | [AR §5a.7] step 4 row "existence" | Both live, or both deleted with the same reason and replacement. |
| MR-042 | `existence` | `deleted-vs-modified` | `policy` | DeleteVsModify | value | design | [AR §2.5]; [AR §5a.7] step 4 row "existence"; [AR §5d.3] | "`main` modified #40 → `DeleteVsModify` conflict value"; the provisional state follows EP and AP rows. |
| MR-043 | `existence` | `ours-only` | `take-o` | - | clean | design | [AR §2.5]; [AR §5d.3] | Deleted on dst while src only read the node: the delete wins ("`main` only read #40 → delete wins"). Also a creation or an `Undelete` on one side. |
| MR-044 | `existence` | `theirs-only` | `take-t` | - | clean | design | [AR §2.5]; [AR §5d.3] | As MR-043, from src. |
| MR-045 | `existence` | `both-created` | `stage-take-o` | IdCollision | structural | proposed | [AR §2.4]; [AR §5a.8]; [OP-27] | Two random 128-bit uids created on two sides are equal only by a fault; the design names `IdCollision` for import only. |
| MR-046 | `existence` | `both` | `take-o` | - | clean | proposed | [AR §4.6] item 10; [OP-7] | Both sides deleted the node, with different reasons or replacements: it is deleted with dst's `reason` and `replaced_by`. |
| MR-047 | `edge` | `same` | `take-o` | - | clean | design | [AR §3.3] | Set semantics per key. |
| MR-048 | `edge` | `ours-only` | `take-o` | - | clean | design | [AR §3.3]; [AR §3.4] I25′ | An edge added, removed or re-pinned on one side lands. A structural edge to a node the other side deleted is caught by VA-004; a historical one becomes a tombstone reference ([AR §3.4] I3). |
| MR-049 | `edge` | `theirs-only` | `take-t` | - | clean | design | [AR §3.3]; [AR §3.4] I25′ | As MR-048. |
| MR-050 | `edge` | `both-present` | `conflict-value` | FieldEdit | value | design | [AR §5a.7] step 4 row "pinned_commit on citations" | "theirs if ours = base, else conflict": both sides re-pinned one citation differently, or added one edge with different properties. |
| MR-051 | `edge` | `both` | `conflict-value` | FieldEdit | value | proposed | [AR §3.3]; [OP-8] | One side removed the edge while the other changed its properties. |
| MR-052 | `schema-item` | `same` | `take-o` | - | clean | design | [AR §2.12] | - |
| MR-053 | `schema-item` | `ours-only` | `take-o` | - | clean | design | [AR §2.12]; [AR §5a.7] step 4 row "schema" | A weakening lands; a strengthening lands and VA-010 re-validates the other side's data against it. |
| MR-054 | `schema-item` | `theirs-only` | `take-t` | - | clean | design | [AR §2.12]; [AR §5a.7] step 4 row "schema" | As MR-053. |
| MR-055 | `schema-item` | `both-strengthen` | `stage-take-o` | SchemaConflict | structural | design | [AR §2.12]; [AR §5a.7] step 4 row "schema" | "strengthening on both sides → `SchemaConflict`". Every change of a policy row ([F08 §8.5.6]) is a strengthening in CS-017's sense, so two sides that changed one row differently, a removal included, stage here and an effective policy is always plain (spec sync 2b). |
| MR-056 | `schema-item` | `both` | `stage-take-o` | SchemaConflict | structural | proposed | [AR §2.12]; [OP-16] | Both sides added one schema item (same name) with different definitions: "weakening changes union" cannot unite two definitions of one item. |
| MR-057 | `query` | `same` | `take-o` | - | clean | design | [50 §4.4] | - |
| MR-058 | `query` | `ours-only` | `take-o` | - | clean | design | [50 §4.4] | "a query defined or changed on one side lands"; so does a `DROP` on one side. |
| MR-059 | `query` | `theirs-only` | `take-t` | - | clean | design | [50 §4.4] | As MR-058. |
| MR-060 | `query` | `dropped-vs-modified` | `keep-defined` | DeleteVsModify | value | proposed | [50 §4.4]; [AR §5a.7] step 4 row "R5 named-query definition"; [OP-17] | The class is the design's ("`DROP` versus modify is `DeleteVsModify`"); the provisional state is proposed. |
| MR-061 | `query` | `both-same-ast` | `take-o` | - | clean | proposed | [50 §4.4]; [OP-17] | "changed on both sides to the same canonical-AST hash is not a conflict" is the design's; that dst's text lands is proposed. |
| MR-062 | `query` | `both` | `conflict-value` | FieldEdit | value | design | [50 §4.4]; [AR §5a.7] step 4 row "R5 named-query definition" | "No line-level merge ever produces a text that nobody wrote." |

## 6. Status lattices

The merge order of each kind's statuses, from the "Status set (lattice order for merge)" column of [AR §3.2]. A row's
`covers` lists the statuses it lies immediately above; `side = yes` marks a side state, which is below or above
nothing and joins with nothing (CS-007). The status *machine* (which transitions a write may make, [AR §3.6]) is a
separate table (`status-machines.md`, planned); the two must agree where [AR §3.2] and [AR §3.6] agree ([OP-2]).

<!-- table: status-lattice -->
| row | kind | status | side | covers | basis | source | note |
|---|---|---|---|---|---|---|---|
| SL-001 | task | `open` | no | - | design | [AR §3.2] | - |
| SL-002 | task | `in_progress` | no | open | design | [AR §3.2]; [AR §5d.2] | - |
| SL-003 | task | `done` | no | in_progress | design | [AR §3.2]; [AR §5d.2] | "lattice `done > in_progress > open`". |
| SL-004 | task | `deferred` | yes | - | design | [AR §3.2] | `deferred → open` is a transition ([AR §3.6]), not an order. |
| SL-005 | task | `cancelled` | yes | - | design | [AR §3.2]; [AR §5d.2] | Counts as done for the virtual `done` field; `cancelled` vs `done` is a `StatusFork`. |
| SL-006 | task | `frozen` | yes | - | design | [AR §3.2] | "conflict against `done`". |
| SL-007 | doc | `draft` | no | - | design | [AR §3.2] | - |
| SL-008 | doc | `current` | no | draft | design | [AR §3.2] | - |
| SL-009 | doc | `superseded` | yes | - | design | [AR §3.2]; [AR §3.4] I6 | Set together with a `supersedes` edge. |
| SL-010 | doc | `archived` | yes | - | design | [AR §3.2] | - |
| SL-011 | note | `active` | no | - | design | [AR §3.2] | - |
| SL-012 | note | `superseded` | yes | - | design | [AR §3.2] | - |
| SL-013 | note | `retracted` | yes | - | design | [AR §3.2] | - |
| SL-014 | note | `archived` | yes | - | design | [AR §3.2] | - |
| SL-015 | rule | `proposed` | no | - | design | [AR §3.2] | - |
| SL-016 | rule | `active` | no | proposed | design | [AR §3.2] | - |
| SL-017 | rule | `superseded` | yes | - | design | [AR §3.2] | - |
| SL-018 | rule | `retracted` | yes | - | design | [AR §3.2] | - |
| SL-019 | rule | `archived` | yes | - | design | [AR §3.2] | - |
| SL-020 | decision | `proposed` | no | - | design | [AR §3.2] | - |
| SL-021 | decision | `accepted` | no | proposed | design | [AR §3.2] | - |
| SL-022 | decision | `rejected` | yes | - | design | [AR §3.2] | - |
| SL-023 | decision | `superseded` | yes | - | design | [AR §3.2] | - |
| SL-024 | question | `open` | no | - | design | [AR §3.2]; [OP-4] | - |
| SL-025 | question | `answered` | no | open | design | [AR §3.2]; [AR §3.5]; [OP-4] | "`answered` is derived from a visible `answers` edge". |
| SL-026 | question | `dropped` | yes | - | design | [AR §3.2] | - |
| SL-027 | finding | `open` | no | - | design | [AR §3.2] | - |
| SL-028 | finding | `confirmed` | no | open | design | [AR §3.2]; [AR §3.6] | - |
| SL-029 | finding | `refuted` | no | open | design | [AR §3.2]; [AR §2.7] | Incomparable with `confirmed`, so parallel refuters that disagree give a `StatusFork` (S12). |
| SL-030 | finding | `fixed` | no | confirmed | proposed | [AR §3.2]; [AR §3.6] I13; [OP-2] | Only `confirmed → fixed` is a transition. |
| SL-031 | finding | `deferred` | no | open | proposed | [AR §3.2]; [AR §3.6]; [OP-2] | - |
| SL-032 | finding | `withdrawn` | no | open | proposed | [AR §3.2]; [AR §3.6]; [OP-2] | - |
| SL-033 | verdict | `open` | no | - | design | [AR §3.2] | - |
| SL-034 | verdict | `accepted` | no | open | design | [AR §3.2] | - |
| SL-035 | verdict | `superseded` | yes | - | design | [AR §3.2] | - |
| SL-036 | measurement | `current` | no | - | design | [AR §3.2] | - |
| SL-037 | measurement | `moved_declared` | yes | - | design | [AR §3.2] | - |
| SL-038 | measurement | `retracted` | yes | - | design | [AR §3.2] | - |
| SL-039 | artifact | `planned` | no | - | design | [AR §3.2]; [40 §2.2] | - |
| SL-040 | artifact | `present` | no | planned | design | [40 §5.5] | "`planned` vs `present` → lattice join → `present`". |
| SL-041 | artifact | `removed` | yes | - | design | [40 §5.5] | vs `present`: `StatusFork`, never resolved by path presence. |
| SL-042 | run | `running` | no | - | design | [AR §3.2] | - |
| SL-043 | run | `green` | no | running | design | [AR §3.2] | The four end states are pairwise incomparable. |
| SL-044 | run | `red` | no | running | design | [AR §3.2] | - |
| SL-045 | run | `stopped` | no | running | design | [AR §3.2] | - |
| SL-046 | run | `died` | no | running | design | [AR §3.2] | - |
| SL-047 | lane | `active` | no | - | design | [AR §3.2] | - |
| SL-048 | lane | `ready_to_merge` | no | active | design | [AR §3.2] | - |
| SL-049 | lane | `merge_pending` | no | ready_to_merge | design | [AR §3.2] | - |
| SL-050 | lane | `merged` | no | merge_pending | design | [AR §3.2] | - |
| SL-051 | lane | `frozen` | yes | - | design | [AR §3.2] | - |
| SL-052 | lane | `abandoned` | yes | - | design | [AR §3.2] | - |
| SL-053 | lane | `measuring` | yes | - | design | [AR §3.2]; [AR §6.6] | Implies quiet mode. |
| SL-054 | area | `active` | no | - | design | [AR §3.2] | - |
| SL-055 | area | `archived` | yes | - | design | [AR §3.2] | - |

## 7. Existence policies and automatic resolution

`existence-policy` names, per kind, the provisional state a `DeleteVsModify` leaves (RS-008): `delete-wins` ("record
the lost edit", [04 §5] C2), `resurrect` ("modify resurrects"), or `none` (dst's state stands). The conflict value lands
in every case; the policy only decides what the node looks like until someone resolves it.

<!-- table: existence-policy -->
| row | kind | uid_derivation | policy | basis | source | note |
|---|---|---|---|---|---|---|
| EP-001 | task | random | delete-wins | design | [AR §5a.7] step 4 row "existence"; [AR §2.5] | "default: tasks delete-wins"; a deleted task stays out of `ready` everywhere. |
| EP-002 | doc | random | resurrect | design | [AR §5a.7] step 4 row "existence"; [AR §3.6] | "knowledge resurrect"; knowledge = note, rule, decision, doc ([AR §3.6]). |
| EP-003 | note | random | resurrect | design | [AR §5a.7] step 4 row "existence"; [AR §3.6] | As EP-002. |
| EP-004 | rule | random | resurrect | design | [AR §5a.7] step 4 row "existence"; [AR §3.6] | As EP-002. |
| EP-005 | decision | random | resurrect | design | [AR §5a.7] step 4 row "existence"; [AR §3.6] | As EP-002. |
| EP-006 | question | random | resurrect | proposed | [AR §5a.7]; [OP-5] | Neither a task nor knowledge in [AR §3.6]; resurrect loses nothing while the conflict stands. |
| EP-007 | finding | random | resurrect | proposed | [AR §5a.7]; [OP-5] | As EP-006; review evidence. |
| EP-008 | verdict | random | resurrect | proposed | [AR §5a.7]; [AR §3.3] "gates"; [OP-5] | A resurrected failing verdict keeps gating, in the spirit of X4 (a deleted gate never silently ungates). |
| EP-009 | measurement | random | resurrect | proposed | [AR §5a.7]; [OP-5] | As EP-006. |
| EP-010 | run | random | resurrect | proposed | [AR §5a.7]; [OP-5] | As EP-006; `runs_in`, `produced`, `consumed` stay valid. |
| EP-011 | lane | random | resurrect | proposed | [AR §5a.7]; [OP-5] | As EP-006; `merge_after` and `runs_in` stay valid. |
| EP-012 | area | random | resurrect | proposed | [AR §5a.7]; [OP-5] | As EP-006; `scoped_to` (restrict) stays valid. |
| EP-013 | area/root | root-key | none | proposed | [40 §2.4]; [40 §5.5]; [OP-5] | Derived-uid kind: existence rules in [RULES/link-merge-rules]; no automatic policy, as for `artifact`. |
| EP-014 | artifact | file-key | none | design | [40 §5.5] "existence" | "no automatic policy for `artifact`"; the provisional state (dst's) is [OP-5]. |

`auto-policy` gives the allowed values of the per-kind policy data `merge.policy.<kind>` ([AR §13], opt-in, default
`none`) and of `--policy` on `merge` and `resolve --all`. The model implements every value, swept pairwise with
`merge.strict` ([AR §13] "Sweep plan").

<!-- table: auto-policy -->
| row | value | applies_to | effect | basis | source | note |
|---|---|---|---|---|---|---|
| AP-001 | `none` | value-conflicts | no-auto-resolution | design | [AR §13] policy data; [AR §5a.8] | The default for every kind: value conflicts land or stage by LS rows. |
| AP-002 | `ours` | value-conflicts | resolve-to-o | proposed | [AR §5a.8]; [AR §13]; [OP-26] | Opt-in per kind (the design's example: `ours` for rules): a value conflict on a key of that kind is not emitted and the key takes o. Structural violations are never auto-resolved. |
| AP-003 | `theirs` | value-conflicts | resolve-to-t | proposed | [AR §5a.8]; [AR §13]; [OP-26] | As AP-002 with t (the design's example: `theirs` for findings). |
| AP-004 | `delete-wins` | existence | provisional-deleting-side | design | [AR §5a.7] step 4 row "existence" | `--policy delete-wins` overrides the EP row for this merge; the `DeleteVsModify` value still lands. |
| AP-005 | `resurrect` | existence | provisional-modifying-side | design | [AR §5a.7] step 4 row "existence" | As AP-004. |

## 8. Field and edge classes

Every stored field of the 13 core kinds ([AR §3.1], [AR §3.2], [40 §2.2]) with its merge class. The schema's field rows
carry the same class as data ([60 §2.5] "fields (type, lattice, class)"); the model builds its genesis schema from
[F08] and a model test checks that every core field's class there equals its row here. `type` is written as the
design writes it; its encoding in the closed type set is [F08]'s ([OP-11]).

<!-- table: field-class -->
| row | kind | field | type | class | basis | source | note |
|---|---|---|---|---|---|---|---|
| FC-001 | * | `uid` | u128 | identity | design | [AR §3.1] | "never changed"; the identity of every key. |
| FC-002 | * | `kind` | u8 | identity | derived | [AR §3.1]; [AR §4.6] item 10 | Fixed by `Create` (`created(kind)`); no op changes it. |
| FC-003 | * | `CREATOR` | struct | identity | design | [AR §3.1]; [50 §8.1] F4 | "set at `Create`, never changed". |
| FC-004 | * | `title` | text | scalar | proposed | [AR §2.6]; [AR §5b.2] rule 2; [OP-11] | One line of at most 200 B: a whole-value `FieldEdit`, not diff3. |
| FC-005 | * | `abstract` | text | scalar | proposed | [AR §2.6]; [OP-11] | One line; where it is stored is [F08]'s. |
| FC-006 | * | `status` | u8 | status | design | [AR §3.1]; [AR §5a.7] step 4 row "status" | - |
| FC-007 | * | `resolution` | u8 | status | design | [AR §4.6] item 10; [AR §4.3] `SetStatus` | Part of the status key ([OP-3]). |
| FC-008 | * | `priority` | u8 | scalar | design | [AR §5a.7] step 4 row "enum / number scalar" | - |
| FC-009 | * | `criticality` | u8 | scalar | design | [AR §5a.7] step 4 row "enum / number scalar" | - |
| FC-010 | * | `confidence` | u8 | scalar | design | [AR §5a.7] step 4 row "enum / number scalar" | - |
| FC-011 | * | `authority` | u8 | authority | design | [AR §5a.7] step 4 row "owner-authority fields" | - |
| FC-012 | * | `flags.pinned` | bool | scalar | derived | [AR §3.1]; [AR §5b.2] rule 2 | A source-truth flag. |
| FC-013 | * | `flags.archived` | bool | scalar | derived | [AR §3.1]; [AR §5b.2] rule 2 | A source-truth flag. |
| FC-014 | * | `flags.frozen` | bool | scalar | derived | [AR §3.1]; [AR §5b.2] rule 2 | A source-truth flag. |
| FC-015 | * | `flags.deleted` | bool | existence | design | [AR §3.1]; [AR §4.6] item 10 | The existence key. |
| FC-016 | * | `flags.suspect` | bool | derived | design | [AR §3.5] | - |
| FC-017 | * | `flags.has_dangling` | bool | derived | design | [AR §3.5]; [AR §3.4] I39′ | - |
| FC-018 | * | `flags.container` | bool | derived | derived | [AR §3.5]; [AR §5b.2] rule 2 | Not a source-truth flag. |
| FC-019 | * | `flags.conflicted` | bool | derived | design | [AR §3.5] | Set when the node holds a conflict value (RE-008). |
| FC-020 | * | `parent` | ref | hierarchy | design | [AR §5a.7] step 4 row "parent / order"; [AR §4.6] item 10 | One key with `order`. |
| FC-021 | * | `order` | text | hierarchy | design | [AR §4.6] item 10; [AR §5b.2] rule 4 | Fractional index. |
| FC-022 | * | `defer_until` | u32 | scalar | derived | [AR §3.1] | - |
| FC-023 | * | `due` | u32 | scalar | derived | [AR §3.1] | - |
| FC-024 | * | `body` | blob | text | design | [AR §5a.7] step 4 row "text"; [AR §4.6] item 10 | - |
| FC-025 | * | `topo` | u32 | derived | design | [AR §3.1]; [AR §3.4] I9 | - |
| FC-026 | * | `rev_seq` | u64 | none | design | [AR §5a.7] step 7 | Set to the merge commit's seq for every touched node (RE-007). |
| FC-027 | * | `created_tx` | u32 | none | derived | [AR §3.1] | - |
| FC-028 | * | `updated_tx` | u32 | none | derived | [AR §3.1] | - |
| FC-029 | * | `last_op_lsn` | u64 | none | derived | [AR §3.1] | - |
| FC-030 | * | `open_blockers` | u16 | derived | design | [AR §3.5] | - |
| FC-031 | * | `open_blockers_exo` | u16 | derived | design | [AR §3.5] | - |
| FC-032 | * | `children_total` | u16 | derived | design | [AR §3.5] | - |
| FC-033 | * | `children_done` | u16 | derived | design | [AR §3.5] | - |
| FC-034 | * | `done` | bool | none | design | [AR §3.1] | Virtual: reads and writes map to the status key; never stored. |
| FC-035 | task | `work_kind` | enum | scalar | design | [AR §3.2] | - |
| FC-036 | task | `phase_state` | enum | scalar | proposed | [AR §3.2]; [OP-28] | No merge lattice is declared for it. |
| FC-037 | task | `assignee` | sym | scalar | design | [AR §3.2] | Read-only on `plan/*` (VA-013). |
| FC-038 | task | `acceptance` | text | text | design | [AR §3.2] | - |
| FC-039 | task | `files_owned` | set<glob> | glob-set | design | [40 §2.4]; [40 §5.5] | - |
| FC-040 | task | `estimate` | u16 | scalar | design | [AR §3.2] | - |
| FC-041 | task | `reopen_count` | counter | counter | design | [AR §3.2]; [AR §3.6] | - |
| FC-042 | task | `reopen_if` | text | text | design | [AR §3.2] | - |
| FC-043 | task | `pre_registered` | bool | scalar | design | [AR §3.2] | - |
| FC-044 | task | `labels` | set | set | design | [AR §3.2] | - |
| FC-045 | doc | `doc_kind` | enum | scalar | design | [AR §3.2] | - |
| FC-046 | doc | `heading` | text | text | design | [AR §3.2] | - |
| FC-047 | doc | `order` | text | hierarchy | derived | [AR §3.2]; [AR §4.6] item 10 | The same key as FC-021. |
| FC-048 | doc | `revision` | u16 | scalar | design | [AR §3.2] | Increments on both sides conflict loudly as `FieldEdit`, never silently (X11). |
| FC-049 | doc | `changed_in_round` | u16 | scalar | design | [AR §3.2] | - |
| FC-050 | doc | `targets` | list | scalar | proposed | [AR §3.2]; [OP-11] | Atomic list value. |
| FC-051 | doc | `readiness` | list | scalar | proposed | [AR §3.2]; [OP-11] | Atomic list value. |
| FC-052 | doc/section | `body` | blob | section-text | design | [AR §5a.7] step 4 row "text" | - |
| FC-053 | note | `note_kind` | enum | scalar | design | [AR §3.2] | - |
| FC-054 | note | `symptom` | text | text | design | [AR §3.2] | - |
| FC-055 | note | `mechanism` | text | text | design | [AR §3.2] | - |
| FC-056 | note | `defence` | text | text | design | [AR §3.2] | - |
| FC-057 | note | `incidents` | counter | counter | design | [AR §3.2] | - |
| FC-058 | note | `applies_to.roles` | set | set | proposed | [AR §3.2]; [OP-12] | Per component. |
| FC-059 | note | `applies_to.phases` | set | set | proposed | [AR §3.2]; [OP-12] | Per component. |
| FC-060 | note | `applies_to.lanes` | set | set | proposed | [AR §3.2]; [OP-12] | Per component. |
| FC-061 | note | `applies_to.globs` | set<glob> | glob-set | design | [40 §2.4]; [72 M5]; [OP-12] | - |
| FC-062 | note | `observed_git_sha` | oid | scalar | derived | [AR §3.2] | - |
| FC-063 | note | `review_after` | u32 | scalar | design | [AR §3.2] | - |
| FC-064 | rule | `text` | text | text | design | [AR §3.2]; [OP-1] | - |
| FC-065 | rule | `enforcement` | enum | scalar | design | [AR §3.2] | - |
| FC-066 | rule | `applies_to.roles` | set | set | proposed | [AR §3.2]; [OP-12] | - |
| FC-067 | rule | `applies_to.phases` | set | set | proposed | [AR §3.2]; [OP-12] | - |
| FC-068 | rule | `applies_to.lanes` | set | set | proposed | [AR §3.2]; [OP-12] | - |
| FC-069 | rule | `applies_to.globs` | set<glob> | glob-set | design | [40 §2.4]; [72 M5]; [OP-12] | - |
| FC-070 | rule | `since` | u32 | scalar | design | [AR §3.2] | - |
| FC-071 | rule | `rationale` | text | text | design | [AR §3.2] | - |
| FC-072 | rule | `owner_quote` | text | owner | design | [AR §5a.7] step 4 row "owner-authority fields" | - |
| FC-073 | decision | `context` | text | text | design | [AR §3.2] | - |
| FC-074 | decision | `what` | text | text | design | [AR §3.2] | - |
| FC-075 | decision | `why` | text | text | design | [AR §3.2] | - |
| FC-076 | decision | `tradeoff` | text | text | design | [AR §3.2] | - |
| FC-077 | decision | `alternatives` | list | scalar | proposed | [AR §3.2]; [OP-11] | Atomic list value. |
| FC-078 | decision | `owner_quote` | text | owner | design | [AR §5a.7] step 4 row "owner-authority fields" | - |
| FC-079 | question | `q_kind` | enum | scalar | design | [AR §3.2] | - |
| FC-080 | question | `asked_of` | enum | scalar | design | [AR §3.2] | - |
| FC-081 | question | `options` | list | scalar | proposed | [AR §3.2]; [OP-11] | Atomic list value; order matters. |
| FC-082 | question | `answer` | text | text | design | [AR §3.2] | Verbatim text. |
| FC-083 | finding | `local_id` | sym | scalar | design | [AR §3.2] | - |
| FC-084 | finding | `severity` | enum | scalar | design | [AR §3.2] | - |
| FC-085 | finding | `f_kind` | enum | scalar | design | [AR §3.2] | - |
| FC-086 | finding | `failure_scenario` | text | text | design | [AR §3.2] | - |
| FC-087 | finding | `what_needed` | text | text | design | [AR §3.2] | - |
| FC-088 | finding | `where` | struct | none | proposed | [AR §3.3]; [OP-31] | Not a stored field: `file:symbol@sha` is an `at` edge with a symbol anchor. |
| FC-089 | finding | `round` | u16 | scalar | design | [AR §3.2] | - |
| FC-090 | finding | `evidence` | text | text | design | [AR §3.2] | - |
| FC-091 | verdict | `role` | sym | scalar | design | [AR §3.2]; [OP-29] | Verdicts are immutable once written. |
| FC-092 | verdict | `round` | u16 | scalar | design | [AR §3.2]; [OP-29] | - |
| FC-093 | verdict | `raw_label` | text | text | design | [AR §3.2]; [OP-29] | - |
| FC-094 | verdict | `outcome` | enum | scalar | design | [AR §3.2]; [OP-29] | - |
| FC-095 | verdict | `return_to` | enum | scalar | design | [AR §3.2]; [OP-29] | - |
| FC-096 | verdict | `criteria` | text | text | design | [AR §3.2]; [OP-29] | - |
| FC-097 | verdict | `conditions` | text | text | design | [AR §3.2]; [OP-29] | - |
| FC-098 | measurement | `metric` | sym | scalar | design | [AR §3.2] | - |
| FC-099 | measurement | `value` | f64 | scalar | design | [AR §3.2] | - |
| FC-100 | measurement | `unit` | sym | scalar | design | [AR §3.2] | - |
| FC-101 | measurement | `target` | f64 | scalar | design | [AR §3.2] | - |
| FC-102 | measurement | `command` | text | text | design | [AR §3.2] | - |
| FC-103 | measurement | `measured_on` | oid | scalar | derived | [AR §3.2] | sha + algo. |
| FC-104 | measurement | `env` | struct | scalar | proposed | [AR §3.2]; [OP-11] | Atomic struct {host, profile, load, scale}. |
| FC-105 | measurement | `baseline` | ref | scalar | design | [AR §3.2] | - |
| FC-106 | artifact | `origin_path` | path | identity | design | [40 §2.2] | - |
| FC-107 | artifact | `origin_pred` | u128 | identity | design | [40 §2.2] | - |
| FC-108 | artifact | `root` | sym | scalar | design | [40 §2.2]; [OP-30] | [40]'s class, although `root` is a derivation input. |
| FC-109 | artifact | `path` | path | observation | design | [40 §2.2] | Member of the one composite key. |
| FC-110 | artifact | `oid` | oid | observation | design | [40 §2.2] | Member of the composite. |
| FC-111 | artifact | `bytes` | u64 | observation | design | [40 §2.2] | Member of the composite. |
| FC-112 | artifact | `observed_git` | oid | observation | design | [40 §2.2] | Member of the composite. |
| FC-113 | artifact | `observed_blob` | oid | observation | design | [40 §2.2] | Member of the composite. |
| FC-114 | artifact | `relink` | text | observation | design | [40 §2.2]; [40 §2.11] R-17 | Member of the composite. |
| FC-115 | artifact | `aliases` | set<path> | alias-set | design | [40 §2.2] | - |
| FC-116 | artifact | `artifact_kind` | enum | scalar | design | [40 §2.2] | - |
| FC-117 | artifact | `reason` | text | scalar | design | [40 §2.2] | [40]'s class `scalar`, not diff3. |
| FC-118 | artifact | `replaced_by` | ref | scalar | design | [40 §2.2] | - |
| FC-119 | artifact | `excerpt` | text | scalar | design | [40 §2.2] | [40]'s class `scalar`, not diff3. |
| FC-120 | artifact | `title` | text | none | design | [40 §2.2]; [AR §3.2] | Derived from `path`, not stored. |
| FC-121 | run | `wf_id` | sym | scalar | design | [AR §3.2] | - |
| FC-122 | run | `bg_task_id` | sym | scalar | design | [AR §3.2] | - |
| FC-123 | run | `session_id` | sym | scalar | design | [AR §3.2] | - |
| FC-124 | run | `script_path` | path | scalar | derived | [AR §3.2]; [AR §3.3]; [OP-32] | An `abs` artifact reference. |
| FC-125 | run | `args_hash` | sym | scalar | design | [AR §3.2] | - |
| FC-126 | run | `journal_path` | path | scalar | derived | [AR §3.2]; [AR §3.3]; [OP-32] | An `abs` artifact reference. |
| FC-127 | run | `started` | u64 | scalar | design | [AR §3.2] | - |
| FC-128 | run | `ended` | u64 | scalar | design | [AR §3.2] | - |
| FC-129 | run | `expected_artifacts` | set | set | design | [AR §3.2] | - |
| FC-130 | lane | `worktree_path` | path | scalar | derived | [AR §3.2]; [AR §3.3]; [OP-32] | An `abs` artifact reference. |
| FC-131 | lane | `git_branch` | sym | scalar | design | [AR §3.2] | - |
| FC-132 | lane | `base_sha` | oid | scalar | design | [AR §3.2] | - |
| FC-133 | lane | `tip_sha` | oid | scalar | design | [AR §3.2] | - |
| FC-134 | lane | `target_dir` | path | scalar | design | [AR §3.2] | - |
| FC-135 | lane | `moirai_branch` | sym | scalar | design | [AR §3.2] | - |
| FC-136 | area | `path_globs` | set<glob> | glob-set | design | [40 §2.4]; [40 §5.5] | - |
| FC-137 | area/root | `root` | sym | identity | proposed | [40 §2.3]; [40 §2.4]; [OP-30] | The derivation input of a root-key uid. |
| FC-138 | area/root | `path_moves` | set<pathmove> | pathmove-set | design | [40 §2.4]; [40 §5.5] | - |

Every edge kind of [AR §3.3] (25 kinds) with its class. `props` names the property block compared by CS-016;
`constraint` lists what VA rows check after the merge.

<!-- table: edge-class -->
| row | edge | edge_class | class | props | constraint | basis | source | note |
|---|---|---|---|---|---|---|---|---|
| EC-001 | `parent` | structural | hierarchy | - | forest, depth<=12 | design | [AR §3.3]; [AR §4.6] item 10 | Merged as the hierarchy key, never as an edge key. |
| EC-002 | `blocks` | structural | edge | flagged | precedence-acyclic | design | [AR §3.3]; [AR §3.4] I5′ | `flagged` marks a dangling blocker kept by `rm` ([AR §5b.2] rule 8). |
| EC-003 | `gates` | structural | edge | flagged | precedence-acyclic | design | [AR §3.3]; [AR §3.4] I5′ | - |
| EC-004 | `merge_after` | structural | edge | - | acyclic | design | [AR §3.3]; [OP-10] | - |
| EC-005 | `runs_in` | structural | edge | - | max-1 | design | [AR §3.3]; [OP-10] | - |
| EC-006 | `answers` | structural | edge | - | max-1-active | design | [AR §3.3]; [OP-10] | - |
| EC-007 | `scoped_to` | structural | edge | - | - | design | [AR §3.3] | - |
| EC-008 | `duplicate_of` | structural | edge | - | chain-1 | design | [AR §3.3]; [AR §3.4] I7; [OP-10] | - |
| EC-009 | `depends_on` | structural | edge | - | acyclic | design | [AR §3.3]; [OP-10] | - |
| EC-010 | `supersedes` | historical | edge | - | acyclic, max-1-active-superseder | design | [AR §3.3]; [AR §3.4] I6 | - |
| EC-011 | `derived_from` | historical | edge | pinned_commit | acyclic | design | [AR §3.3]; [OP-10] | "acyclic by construction" in one branch; two branches can close a cycle. |
| EC-012 | `cites` | historical | edge | pinned_commit | - | design | [AR §3.3] | - |
| EC-013 | `implements` | historical | edge | pinned_commit | - | design | [AR §3.3] | - |
| EC-014 | `refutes` | historical | edge | - | - | design | [AR §3.3] | - |
| EC-015 | `confirms` | historical | edge | - | - | design | [AR §3.3] | - |
| EC-016 | `verifies` | historical | edge | - | - | design | [AR §3.3] | - |
| EC-017 | `addresses` | historical | edge | - | - | design | [AR §3.3] | - |
| EC-018 | `about` | historical | edge | - | - | design | [AR §3.3] | "≤ 1 typical": not an invariant, not checked. |
| EC-019 | `discovered_from` | historical | edge | - | acyclic | design | [AR §3.3]; [OP-10] | As EC-011. |
| EC-020 | `produced` | historical | edge | - | - | design | [AR §3.3] | - |
| EC-021 | `consumed` | historical | edge | - | - | design | [AR §3.3] | - |
| EC-022 | `contradicts` | historical | edge | - | - | design | [AR §3.3]; [OP-25] | Symmetric for reading, one direction stored. |
| EC-023 | `mentions` | historical | edge | - | - | proposed | [AR §3.3]; [AR §5b.6] step 5; [OP-24] | Merged as ordinary keys; the write-time `#N` re-parse does not run at merge. |
| EC-024 | `relates` | historical | edge | - | - | design | [AR §3.3] | - |
| EC-025 | `at` | historical | anchor | anchor | at-least-1-anchor | design | [40 §2.8]; [40 §2.11] R-4 | Rules in [RULES/link-merge-rules]. |

## 9. Conflict and violation classes

The taxonomy of [AR §5a.8] with the rows that emit each class, and the three structural classes review pass 1 added
for validators the taxonomy left without one (CM-023 to CM-025; P1-21). Numeric codes are one `u8` space
([F19 §12.1]): [F12 §6.1] owns the value-conflict codes 1–63, [F19 §12.2] the structural codes 64–127 and [F19 §12.3]
the hint codes 128–191.

<!-- table: class-map -->
| row | conflict | kind | lands | emitted_by | basis | source | note |
|---|---|---|---|---|---|---|---|
| CM-001 | `FieldEdit` | value | yes | MR-009, MR-020, MR-050, MR-051, MR-062, DM-013, LM-006, LM-021, LM-022 | design | [AR §5a.8] | - |
| CM-002 | `StatusFork` | value | yes | MR-025 | design | [AR §5a.8] | - |
| CM-003 | `TextHunk` | value | yes | MR-032, MR-038 | design | [AR §5a.8] | - |
| CM-004 | `DeleteVsModify` | value | yes | MR-042, MR-060, LM-010 | design | [AR §5a.8] | - |
| CM-005 | `SupersedeFork` | value | yes | VA-006 | design | [AR §5a.8] | Where the value sits is [OP-9]. |
| CM-006 | `OwnerFieldEdited` | value | yes | MR-014, MR-019 | design | [AR §5a.8] | Only when dst is not `main`. |
| CM-007 | `DATA` | value | yes | - | proposed | [AR §5a.8]; [AR §5a.5]; [AR §2.7]; [F12 §6.1]; [OP-20] | The revert and cherry-pick case "before-image no longer matches"; emitted with the class of its key's `both` row (DM-013), `FieldEdit` for a scalar. `DATA` has no code and is never stored ([F12 §6.1]). |
| CM-008 | `PathClaim` | value | yes | VA-009, PC-002 | design | [AR §5a.8]; [40 §5.5] | - |
| CM-009 | `DanglingEdge` | structural | no | VA-004 | design | [AR §5a.8] | - |
| CM-010 | `Cycle` | structural | no | VA-003, VA-016 | design | [AR §5a.8] | - |
| CM-011 | `HierarchyCycle` | structural | no | MR-039, VA-001 | design | [AR §5a.8] | VA-001 applies the moves and reports the ones MR-039 skipped ([F13 §5] V01). |
| CM-012 | `IdCollision` | structural | no | MR-045, LM-026 | design | [AR §5a.8]; [AR §5b.6] step 4 | Named for import; the merge rows are proposed ([OP-27]). |
| CM-013 | `SchemaConflict` | structural | no | MR-055, MR-056, VA-010 | design | [AR §5a.8] | - |
| CM-014 | `RemovedTextNotInBase` | structural | no | MR-036 | design | [AR §5a.8] | - |
| CM-015 | `ImageParse` | structural | no | - | design | [AR §5a.8]; [AR §5b.6] step 4 | Import only; never emitted by a merge. |
| CM-016 | `NotFound` | structural | no | DM-012 | design | [AR §5a.8]; [AR §5a.5] | Revert and cherry-pick only. |
| CM-017 | `TombstoneRemoved` | structural | no | - | design | [AR §5a.8]; [AR §5b.6] step 2 | Import only: a hint, escalated when the node is referenced. |
| CM-018 | `QueryInvalid` | structural | no | VA-011 | design | [AR §5a.8]; [50 §8.1] F18 | - |
| CM-019 | `QueryCycle` | structural | no | VA-012 | design | [AR §5a.8]; [50 §8.1] F18 | - |
| CM-020 | `Duplicate` | hint | log | VA-014 | design | [AR §5a.8] | - |
| CM-021 | `Contradiction` | hint | log | VA-015 | design | [AR §5a.8] | - |
| CM-022 | `ForeignMerge` | hint | log | HT-003 | design | [AR §5a.8] | - |
| CM-023 | `DepthExceeded` | structural | no | VA-005 | derived | [AR §3.4] I4; [F19 §12.2] code 67; [F13 §5] V05 | Not in [AR §5a.8]'s taxonomy: I4 holds at every head (I12), so a candidate deeper than 12 stages (I37′). |
| CM-024 | `Cardinality` | structural | no | VA-007, VA-008 | derived | [AR §3.4] I7; [AR §3.3]; [F19 §12.2] code 68; [F13 §5] V07 | Not in [AR §5a.8]'s taxonomy: one class for the `duplicate_of` chain, `runs_in` and `answers` bounds. |
| CM-025 | `PlanMask` | structural | no | VA-013 | derived | [AR §3.4] I33′; [F19 §12.2] code 72; [F13 §5] V12 | Not in [AR §5a.8]'s taxonomy: a merge into `plan/*` that would write a masked field stages. |

## 10. The merge procedure

<!-- table: procedure -->
| row | step | action | basis | source | note |
|---|---|---|---|---|---|
| PR-001 | 0 | sync-first | design | [AR §5a.7] step 0 | A merge into `main` whose src does not contain tip(main) first merges `main` into src (a `sync`, DM-002). Step 0 applies only when src is a branch (`work` or `plan`); for any other src (a tag, `import/*`, `orphans/*`) the merge runs without step 0, over PR-004's base, and nothing is written on src ([F12 §8.1], [F12 §9.6]; spec sync 2b). Both are computed before the writer byte and committed as one flushed group; if the sync stages, the whole merge stages; if it lands conflict values, its group alone is appended on src and PR-003 refuses the merge ([API §11.7] "Sync first with conflict values"). Afterwards LCA(src, main) = tip(main). |
| PR-002 | 0 | refuse-pair-staged | design | [AR §5a.7] step 8; [AR §3.4] I41′ | A second merge of one (dst, src) pair is refused while `merge/<dst>/from/<src>` exists; a sync of lane L is refused while `merge/<L>/from/main` exists; other pairs are never blocked. The refusal is [F19 §10.2] `staging_exists`, exit 6. |
| PR-003 | 0 | refuse-src-conflicted | proposed | [AR §5a.7] step 0; [OP-22] | A merge into `main` is refused while tip(src) holds an unresolved conflict value, with [F19 §10.2] `conflicted_src`, exit 6; `merge-check` lists them. |
| PR-004 | 1 | base | design | [AR §5a.7] step 1 | The base state: VB rows, or `--base <commit>`. |
| PR-005 | 2 | sides | derived | [AR §5a.7] step 2; [AR §4.6]; [60 §4.2] | The engine folds each side since the base (segment walk); the model materialises the states at tip(dst) and tip(src) and compares them key by key with the base state. Both give the same (b, o, t) because a net changeset is a state diff ([AR §4.6]). |
| PR-006 | 3 | base-per-key | design | [AR §5a.7] step 3; [AR §3.4] I25′ | b is the key's value in the base state, never an earliest before-image (N1). |
| PR-007 | 4 | existence-first | derived | [AR §5a.7] step 4; [AR §2.5]; [40 §5.5] | Re-keys (RK rows) run first, on side S's state. Then every existence key is decided (MR-041 to MR-046, LM-007 to LM-014). A uid whose existence row is `policy` fixes all its other keys by RS-008; a uid deleted in the result keeps only its retained out-edges ([AR §3.4] I39′). All other keys follow. |
| PR-008 | 4 | per-key-rules | design | [AR §5a.7] step 4 | For each remaining key: its class from FC or EC rows (the observation composite as one key); the `conflicted-key` rows first when b, o or t is a conflict value; then the class's rows in table order. Disjoint keys commute. |
| PR-009 | 5 | apply | design | [AR §5a.7] step 5 | The results form a candidate state on dst's view; dst's ref does not move. |
| PR-010 | 6 | validate | design | [AR §5a.7] step 6; [AR §3.4] I37′ | VA rows by their `order`; derived state is recomputed where a check needs it (RE-009). |
| PR-011 | 7 | emit | design | [AR §5a.7] step 7 | One merge commit with parents (tip(dst), tip(src)); its net ops are the state diff against tip(dst); one `Conflict` op per value conflict; one `Violation` op per structural problem; `affected`; markers (RE-003); dst's new absorbed vector (RE-006). |
| PR-012 | 8 | land-or-stage | design | [AR §5a.7] step 8 | LS rows. |
| PR-013 | 8 | resolve | design | [AR §5a.7] step 8; [F12 §6.5] | `resolve <key>` with `--take` ours, theirs or base, or `--value V`, and `resolve --all --policy P`, append `Resolve` commits on the staging ref. A take towards a live side of a `DeleteVsModify` also restores the node's hierarchy key and out-edges (RS-008). |
| PR-014 | 8 | continue | derived | [AR §5a.7] step 8; [F12 §9.4] | `merge --continue` recomputes the operation against the current tip(dst) with the staged commit's own arguments, its `--base`, policy override and effective `strict` ([F06 §4.4.16]; never the continue's configuration), and re-runs PR-009 to PR-012 with the staged resolutions as an overlay ([F12 §9.4] steps 1 and 2: the resolved keys include a `Resolve` whose `new` equals `old` and the keys of its companion ops). A resolution whose key's value on the current tip(dst) differs from its value at the dst tip it was made against is stale: the key keeps the recomputed candidate's value and the command prints a notice naming it, so no resolution silently overwrites a later dst change ([F12 §9.4] step 2). When dst did not move, every resolution applies. The validators run over the merge's own base, state(D₁) and state(P₂) (for a revert or cherry-pick, the DM row's base and source); on a key the overlay set, step 1's conflict value, the typed rules' violations and VA-001's skipped move are dropped, and every other step-1 structural violation is kept (MR-036, MR-039, MR-045, MR-055, MR-056, DM-012; [F12 §9.4] step 3; spec sync 2b). |
| PR-015 | 8 | abort | design | [AR §5a.7] step 8 | `merge --abort` deletes the staging ref; no marker was written for it (RE-004). |
| PR-016 | * | pure | design | [AR §5a.7] step 6; [40 §5.5]; [AR §3.4] I28′, I30′; [F12 §7.1] | The merge reads neither the file system nor git. Its result is a function of the base, dst and src states, whether dst is `main`, the (hlc, commit id), net changesets and states of the commits whose step keys RS-007 reads (for a merge, a `sync` or a virtual merge, each side's commits since the base; for a revert or a cherry-pick of C, C and the commits of A(o) ordered after C; and for each two-parent commit among them, recursively, every commit of A(p₂) \ A(p₁)), with the second parents' states of the two-parent commits among them, the store's set of dropped bodies ([F06 §8.1] DB-1), which only the text rule reads (CS-011; [F12 §7.5] "A dropped body"; spec sync 3), and these tables; conflicts only the file system can settle land as values ([RULES/link-merge-rules] LV rows). |
| PR-017 | * | order-independent | derived | [60 §3.4] GT6 | The result does not depend on the order in which keys are visited; RS-007 fixes the only order that matters. |

Validators run on the candidate in this order ([AR §3.4] I37′, [AR §5a.7] step 6). `order` is the position of the
validator V01–V13 of [F13 §5] that the row realises, whose order is normative: violations and conflicts are emitted in
it, and within one position in canonical key order ([F13 §5] VO-2; review pass 1, P1-21). The model checks each by
recomputation over the whole candidate ([60 §4.2]: DFS over the combined graph), never incrementally.

<!-- table: validators -->
| row | order | check | emits | disposition | basis | source | note |
|---|---|---|---|---|---|---|---|
| VA-001 | 1 | apply-parent-moves | HierarchyCycle | structural | design | [AR §5a.7] step 6; [AR §3.4] I37′; [F13 §5] V01 | The `kleppmann` result is applied before anything else; its skipped moves are MR-039's. |
| VA-002 | 2 | derive-implied-exogenous-edges | - | none | design | [AR §5a.7] step 6; [AR §3.4] I5′; [F13 §5] V02 | Implied edges {X→D : blocks(X,P), X ∉ subtree(P), D ∈ subtree(P)} over the merged hierarchy. |
| VA-003 | 3 | precedence-acyclic | Cycle | structural | design | [AR §5a.7] step 6; [AR §3.4] I5′; [F13 §5] V03 | blocks ∪ gates ∪ child→parent ∪ implied edges must be acyclic. |
| VA-004 | 4 | structural-endpoints-live | DanglingEdge | structural | design | [AR §5a.7] step 6; [AR §3.4] I2; [AR §5d.3]; [F13 §5] V04 | A live, unflagged structural edge with a deleted endpoint. Historical edges are exempt (tombstone references, I3). Suggested resolution: the edge kind's delete policy, or `--replaced-by`. |
| VA-005 | 5 | forest-depth | DepthExceeded | structural | derived | [AR §3.4] I4; [F13 §5] V05; [F19 §12.2] code 67; [F12 §7.9]; [OP-10] | `parent` must stay a forest of depth ≤ 12; cycles are VA-001's. I4 holds at every head (I12), so a deeper candidate stages; the `Violation` op's key is the hierarchy key of the least uid deeper than the bound ([F12 §7.9]). |
| VA-006 | 6 | one-active-superseder | SupersedeFork | value | design | [AR §5a.7] step 4 row "supersedes"; [AR §3.4] I6; [F13 §5] V06 | "second active superseder → `SupersedeFork`"; where the value sits is [OP-9]. |
| VA-007 | 7 | duplicate-of-canonical | Cardinality | structural | derived | [AR §3.4] I7; [F13 §5] V07; [F19 §12.2] code 68; [F12 §7.9]; [OP-10] | The target of `duplicate_of` must be canonical; two sides can build a chain of two, which stages. Key: the greatest offending edge key in canonical order ([F12 §7.9]). |
| VA-008 | 7 | structural-cardinality | Cardinality | structural | derived | [AR §3.3]; [F13 §5] V07; [F19 §12.2] code 68; [F12 §7.9]; [OP-10] | `runs_in` ≤ 1 per run, `answers` ≤ 1 active per question; a candidate above either bound stages. Key: as VA-007. |
| VA-009 | 8 | path-claims | PathClaim | value | design | [AR §5a.7] step 6; [AR §3.4] I-F1; [40 §5.5]; [F13 §5] V08 | Details in [RULES/link-merge-rules] PC rows. |
| VA-010 | 9 | schema-conformance | SchemaConflict | structural | design | [AR §5a.7] step 6; [AR §2.12]; [F13 §5] V09 | Every node of the candidate conforms to the merged schema. |
| VA-011 | 10 | named-queries-bind | QueryInvalid | structural | design | [AR §5a.7] step 6; [50 §4.4]; [50 §8.1] F18; [F13 §5] V10 | Every named query the merge touched, or whose referenced schema it touched, parses and binds. |
| VA-012 | 11 | named-query-call-graph | QueryCycle | structural | design | [AR §5a.7] step 6; [50 §4.4]; [50 §8.1] F18; [F13 §5] V11 | The named-query call graph stays acyclic. |
| VA-013 | 12 | plan-read-only | PlanMask | structural | derived | [AR §5a.7] step 6; [AR §3.4] I33′; [F13 §5] V12; [F19 §12.2] code 72; [F12 §7.9]; [OP-10] | On `plan/*`, `status`, `resolution`, `assignee` and claims are read-only; a merge into `plan/*` that would write one of them stages. Key: the masked key ([F12 §7.9]). |
| VA-014 | 13 | duplicate-hint | Duplicate | hint | design | [AR §5a.7] step 6; [AR §5a.8]; [F13 §5] V13 | Trigger: HT-001. |
| VA-015 | 13 | contradiction-hint | Contradiction | hint | design | [AR §5a.7] step 6; [AR §5a.8]; [F13 §5] V13 | Trigger: HT-002. |
| VA-016 | 3 | other-kinds-acyclic | Cycle | structural | proposed | [AR §3.3]; [F13 §5] V03; [F19 §12.2] code 65; [OP-10] | `merge_after`, `depends_on`, `supersedes`, `derived_from` and `discovered_from` are declared acyclic but are not in I37′'s list; checked at V03's position, with V03's class. |

The land-or-stage decision. *strict* is `--strict` or the store key `merge.strict` (default `false`, [AR §13]); a
command-line flag outranks the key. `merge --continue` takes the effective `strict` its staged commit recorded
([F06 §4.4.16]; PR-014), not the key's value at the continue.

<!-- table: land-or-stage -->
| row | violations | conflicts | strict | outcome | basis | source | note |
|---|---|---|---|---|---|---|---|
| LS-001 | >=1 | any | any | stage | design | [AR §5a.7] step 8; [AR §2.7] | The merge commit lands on `merge/<dst>/from/<src>` (a sync of lane L on `merge/<L>/from/main`); dst is untouched; exit 6 with the violations and suggested resolutions. |
| LS-002 | 0 | >=1 | yes | stage | design | [AR §5a.7] step 8; [AR §13] `merge.strict` | - |
| LS-003 | 0 | >=1 | no | land-conflicted | design | [AR §5a.7] step 8; [AR §5a.8] | The merge commit lands on dst; nodes with conflict values get `conflicted` and leave `ready`. |
| LS-004 | 0 | 0 | any | land | design | [AR §5a.7] step 8 | - |

## 11. The recursive virtual base

The rule of [60 §3.4] and [AR §5a.7] step 1 (I31′ as amended). The model computes it by definition: full ancestor
sets, maximal common ancestors, and each LCA's state materialised and merged by these same tables, recursively
([60 §4.2]).

<!-- table: virtual-base -->
| row | condition | outcome | basis | source | note |
|---|---|---|---|---|---|
| VB-001 | always | lcas=maximal(ancestors(dst)&ancestors(src)) | design | [AR §5a.7] step 1; [60 §4.2] | Ancestors-or-self of tip(dst) and tip(src); the LCAs are the maximal elements of their intersection. |
| VB-002 | base-flag | base=state(flag) | design | [AR §5a.7] step 1 | `--base <commit>` overrides; no virtual base is computed. |
| VB-003 | lca-count=0 | base=empty-state | proposed | [OP-19] | Unrelated histories (possible after importing a foreign root): the base holds no node, schema item or query. |
| VB-004 | lca-count=1 | base=state(lca) | design | [AR §5a.7] step 1; [AR §3.4] I25′ | The daily case. |
| VB-005 | lca-count>=2 | order=gen-asc,id-asc | design | [AR §5a.7] step 1; [60 §3.4] | "in generation order (ties by lowest commit id)"; commit ids compared bytewise. |
| VB-006 | lca-count>=2 | base=fold(V1=state(L1),Vk=merge(V(k-1),Lk)) | design | [AR §5a.7] step 1; [60 §3.4] | "merged pairwise … by these same typed rules and the result is the base". Each inner merge has dst = V(k−1) and src = Lk, and its own base is the virtual base of that pair, computed recursively over the DAG extended by the virtual commits (a virtual commit's parents are V(k−1) and Lk). |
| VB-007 | virtual-merge | dst-main=false | proposed | [OP-19] | Inside a virtual merge no dst is `main`: MR-011 and MR-016 never apply there. |
| VB-008 | virtual-merge | dst=V(k-1),src=L(k) | proposed | [OP-19] | Direction matters for the rows that prefer dst: MR-046, MR-061, LM-004, LM-014. |
| VB-009 | virtual-merge | validators=not-run | proposed | [OP-19] | A virtual base need not satisfy invariants; only the final candidate is validated, and a virtual merge never stages. |
| VB-010 | virtual-merge | conflicts=kept-as-values | design | [AR §5a.7] step 1; [60 §3.4] | A key the inner merge leaves in conflict holds that conflict value in the base. |
| VB-011 | virtual-merge | move-steps=real-commits | proposed | [F12 §5.3] VM-7; [OP-19] | For RS-007 on a virtual side, the steps are the real commits of that side's ancestry since the inner base (A(side) \ A(inner base), [F12 §5.2]), each keyed by its (hlc, commit id) and valued in that commit's state (spec sync 2b). |
| VB-012 | virtual-merge | link-rules=apply | derived | [40 §5.5]; [60 §3.4] | Composition and re-key ([RULES/link-merge-rules]) run inside virtual merges like every other rule. |
| VB-019 | virtual-merge | auto-policy=ignored | proposed | [F12 §5.3] VM-2; [OP-19] | No automatic policy applies inside a virtual merge: `merge.policy.<kind>`, `--policy` and `--strict` of the outer command are ignored, and a `DeleteVsModify` takes the kind's EP row from the schema of the virtual merge's dst state. An automatic `ours`/`theirs` there would pick one LCA's value by generation order, the silent choice the virtual base exists to prevent, and the base would depend on the command line. |
| VB-013 | vb-conflict | same=clean | design | [60 §3.4] | MR-001: "two sides that resolved it identically never do". |
| VB-014 | vb-conflict | differ=conflict | design | [60 §3.4]; [F12 §5.4] RVB-4 | MR-002: "two sides that resolved a criss-cross differently always conflict", read as "both sides changed the key since the virtual base and differ" ([OP-18]). |
| VB-018 | vb-conflict | one-side-untouched=other-side | derived | [AR §3.4] I25′; [F12 §5.4] RVB-2, RVB-3; [F12 §5.7] VBC-3; [OP-18] | MR-003 and MR-004: a side that still holds the virtual base's conflict value, in either orientation ([F12 §5.4]'s ≈), did not touch the key since the base, so the other side's value lands, clean, as at a real LCA, for both lanes of VBC-3 (spec sync 2b). |
| VB-015 | property | independent-of-lca-enumeration-order | design | [60 §3.4] GT6 | Follows from VB-005's total order. |
| VB-016 | property | agreed-untouched-key-never-conflicts | design | [60 §3.4] GT6 | "a key both LCAs agree on and neither side touched never conflicts". |
| VB-017 | daily-path | sync-first=single-lca | design | [AR §5a.7] step 0 | After PR-001 the LCA of a merge of a branch into `main` is tip(main); the virtual base arises only in cross-lane and branch-of-branch merges (CM9), and in a merge into `main` of a tag or an `import/*` or `orphans/*` ref, which runs without step 0. |

## 12. Derived merges

Every three-way operation is one of these rows: the same rules with different inputs. `vbase` is the base of §11
computed for the row's dst and src.

<!-- table: derived-merges -->
| row | operation | dst | src | base | special | basis | source | note |
|---|---|---|---|---|---|---|---|---|
| DM-001 | merge | tip(dst) | tip(src) | vbase | - | design | [AR §5a.7] | - |
| DM-002 | sync | tip(lane) | tip(main) | vbase | - | design | [AR §5a.3]; [AR §5a.7] step 0 | `sync` = `merge main --into <lane>`; dst is the lane, never `main`. The engine stores only the residue; the canonical op list is the full state diff ([AR §4.6], [72 M5]). |
| DM-003 | cherry-pick | tip(R) | state(c) | state(p1(c)) | DM-012, DM-013, DM-016 | design | [AR §5a.5] | "3-way apply of the commit's changeset with base = its first parent onto R's view". |
| DM-004 | revert | tip(R) | state(p1(c)) | state(c) | DM-012, DM-013, DM-016, DM-017 | design | [AR §5a.5]; [60 §4.2]; [F06 §7.10] | The inverse changeset as a three-way apply; before-images make it exact. A node c created is never made absent (DM-017). |
| DM-005 | revert-merge-mainline-1 | tip(R) | state(p1(c)) | state(c) | DM-012, DM-013, DM-016, DM-017 | design | [AR §5a.5] CL8; [F06 §7.10] | The merge record stores the merged ops with before-images. |
| DM-006 | revert-merge-mainline-2 | - | - | - | refuse | design | [AR §5a.5]; [F19 §10.2] `revert_refused` | "`--mainline 2` is refused": [F19 §10.2] `revert_refused`, exit 6. |
| DM-007 | revert-sync | - | - | - | refuse | design | [AR §5a.5]; [F19 §10.2] `revert_refused` | A sync stores only the residue; use `undo` on the lane. Refused with `revert_refused`, exit 6. |
| DM-008 | revert-with-dependents | - | - | - | refuse | design | [AR §5a.5]; [F19 §10.2] `revert_refused` | Refused with the dependent set when a later commit on R depends structurally on the reverted one: it added a structural edge to, or a child of, a node the reverted commit created; it completed a task the reverted commit reopened; or it resolved a conflict the reverted commit introduced. The refusal is [F19 §10.2] `revert_refused`, exit 6. |
| DM-009 | virtual | V(k-1) | L(k) | vbase | VB-007, VB-009 | design | [60 §3.4] | §11. |
| DM-010 | foreign-merge-import | state(p1) | state(p2) | vbase | DM-014, DM-015 | design | [AR §5b.6] step 3; [AR §3.4] I30′ | A two-parent foreign git commit is imported as a moirai merge computed by these rules. |
| DM-011 | import-merge | tip(ref) | tip(import/ref) | vbase | - | design | [AR §5b.6] step 4 | A clean imported chain behind a moved local ref: `merge import/<ref> --into <ref>`, its LCA at the last common exported commit; `image.import-merge = stage` stages instead. |
| DM-012 | revert-or-cherry-pick | - | - | - | notfound | proposed | [AR §5a.5]; [AR §3.4] I34′; [OP-21] | `NotFound` (structural; staged on `merge/<R>/from/<commit>`): src changes a key whose uid, or for an edge whose source or destination uid, is live in the base but deleted or absent on dst. |
| DM-013 | revert-or-cherry-pick | - | - | - | data | proposed | [AR §5a.5]; [AR §2.7]; [OP-20] | The `DATA` case is the `both` case of the key's class; for `scalar` keys it lands as `FieldEdit` (MR-009). |
| DM-014 | foreign-merge-import | - | - | - | texthunk-from-foreign-tree | design | [AR §5b.6] step 3 | For a `TextHunk`, a line present in the foreign file and in exactly one side is taken as that side's resolution; any remaining disagreement is a conflict value. |
| DM-015 | foreign-merge-import | - | - | - | counters-from-typed-merge | design | [AR §5b.6] step 3; [AR §3.4] I30′; [F14 §11.2] | Counters come from the typed merge (MR-026) of the parents' imported states over their LCA, never from a text merge or from the foreign file's ledger lines: when both parents merged one shared increment, the union of their ledger lines counts it twice (spec sync 2b; [AR §5b.6] step 3's "union of the `incr` ledger lines" is edited at WP-81a). |
| DM-016 | revert-or-cherry-pick-pruned | - | - | - | refuse | derived | [F06 §4.4.15]; [F06 §7.10]; [F19 §10.2] `commit_pruned` | A commit `gc` pruned to its header has no ops or before-images, so it is never reverted or picked: [F19 §10.2] `commit_pruned`, exit 3, before any state is read (review pass 1 round 2, S1-21 and A1-5). |
| DM-017 | revert-creation | - | - | - | src-absent-reads-inverse | proposed | [F06 §7.10] "`Create` → `Delete`", "`CreateDeleted` has no inverse"; [F12 §7.8]; [OP-33] | On an existence key whose base value (state(c)) is not `absent` and whose src value (state(p1(c))) is `absent`, c brought the node into being, and a revert never makes a node `absent`: when c created it live, src reads as the tombstone that [F06 §7.10]'s inverse `Delete` writes, `deleted(kind, "", none)` (`reason` 0, `replaced_by` 0), so the node is deleted (CS-014 and DM-012 apply as to any delete); when c took it from `absent` to `deleted` (`CreateDeleted`), src reads as b, so dst's value stays and no existence op is emitted. The node's other keys follow their own rows. |

## 13. Runtime and derived state

Leases, markers and the rest of the store-level runtime state are never merged ([AR §5d.1], I36′). A merge affects
them only through the commit it lands.

<!-- table: runtime-effects -->
| row | class | effect | basis | source | note |
|---|---|---|---|---|---|
| RE-001 | lease | not-merged | design | [AR §5d.1]; [AR §3.4] I36′ | Leases are store-wide and keyed by (#N, lease id); a merge never creates, moves or releases one. |
| RE-002 | marker | not-merged | design | [AR §5d.1]; [AR §3.4] I36′ | `settled`, `deleted` and `cleared` markers are never versioned or exported. |
| RE-003 | marker | follow-hold-changes | derived | [AR §4.5] step 4; [AR §5a.7] step 7; [72 M4]; [RULES/state-definition ME-001]; [RULES/state-definition ME-002]; [RULES/state-definition ME-003]; [RULES/state-definition ME-004] | The landing commit changes the marker cache exactly where dst's hold of a node changes between tip(dst) and the merge commit (its net ops against tip(dst)): a hold the merge itself produced originates a marker (ME-001); a hold taken from src joins the holder set of src's origin (ME-002, ME-003), so a merge propagates a completion and does not originate it again; a hold that ends leaves its holder set, and `cleared` is written for the marker key (#N, origin ref, origin commit) only when no holder remains (ME-004). Review pass 1 (S1-16) adopted this origin reading. |
| RE-004 | marker | none-on-staging | design | [AR §5a.7] step 7; [72 M4] | A commit on a staging ref emits no marker; the landing commit of `merge --continue` does. |
| RE-005 | absorbed | src-seq-and-max | design | [AR §5a.7] step 7; [AR §5d.1]; [RULES/state-definition VR-003] | dst's new vector: `absorbed_dst[src] = ref_seq(tip src)`, every other entry the maximum of both sides' entries. This equals VR-003's pointwise maximum whenever tip(src) landed on src. |
| RE-006 | absorbed | recorded-in-commit | design | [AR §4.3] `n_absorbed`; [AR §5a.7] step 7 | The merge and sync records carry dst's vector after the commit. |
| RE-007 | rev_seq | merge-seq | design | [AR §5a.7] step 7 | `rev_seq` of every touched node becomes the merge commit's seq, above both inputs. |
| RE-008 | conflicted | set-on-conflict-value | design | [AR §3.5]; [AR §5a.7] step 8 | A node that holds a conflict value is `conflicted` and out of `ready`; `resolve` clears it. |
| RE-009 | derived | recompute | design | [AR §3.5]; [AR §3.4] I9; [60 §4.2] | `open_blockers`, rollups, `suspect`, `has_dangling`, `unblocked`, `is_blocker` and `topo` are recomputed from the merged primary data; the model recomputes them from scratch. |
| RE-010 | affected | complete-or-flagged | design | [AR §3.4] I42′; [50 §8.1] F15, F16; [F13 §6.3]; [F17 §8.2] | `affected` names every node whose derived predicates changed, or the commit has `affected_complete = 0`, which happens only when the number of nodes whose `suspect` value changed (S(c)) exceeds `store.suspect-budget` ([F13 §6.3], [F17 §8.2]; [RULES/delete-policy-matrix] DS-010). |
| RE-011 | idempotency | not-merged | design | [AR §5d.1]; [AR §6.4] | Keyed by branch; a replay on a branch the original was merged into returns the original result (N13e). |
| RE-012 | alloc-uidx | not-merged | design | [AR §5d.1]; [40 §2.3] | `#N` allocation is store-wide; a re-keyed node gets a new `#N` (RK-008). |
| RE-013 | refs-heads-pins | not-merged | design | [AR §5d.1] | Refs, client heads, pins, `gitmap`, the alias map and caches are runtime. |
| RE-014 | r4-runtime | not-merged | design | [AR §5d.1]; [AR §3.4] I-F4 | `TREES`, `FILEOBS`, `PENDING`, `FSINTENT`, `FPRINT`, `JOURNALCUR`, `DIRMAP`, `PREFIXEV`, `GITFACTS`, `ANCHORRES`. |
| RE-015 | i26 | model-evaluates-definition | design | [AR §3.4] I26′; [60 §4.2]; [72 M4] | The model decides `ready` and `claim` exclusion from I26′'s state definition over all live refs; RE-003's markers are only compared with the engine's ([60 §4.4] item 5). |

## 14. Hints

<!-- table: hints -->
| row | hint | trigger | basis | source | note |
|---|---|---|---|---|---|
| HT-001 | `Duplicate` | same-kind-same-title-same-parent | proposed | [AR §5a.8]; [11 §5.4] step 5; [OP-23] | Two live nodes of one kind with equal titles under one parent after the merge, at least one added by the merge. |
| HT-002 | `Contradiction` | active-rules-same-applies_to-contradicts-or-same-heading | proposed | [AR §5a.8]; [11 §5.4] step 5; [OP-23] | Two active rules with equal `applies_to` joined by a `contradicts` edge, or with identical headings. |
| HT-003 | `ForeignMerge` | foreign-two-parent-import | proposed | [AR §5a.8]; [AR §5b.6] step 3; [OP-23] | Logged when DM-010 imports a foreign merge. |

## 15. Source map

Every row of the design's merge tables, with the rule rows that realise it. The model checks only that the cited rows
exist; the owner uses this table to check that nothing was dropped.

<!-- table: source-map -->
| row | source_row | realized_by | note |
|---|---|---|---|
| SM-001 | AR-5a.7-r01-status | MR-021, MR-022, MR-023, MR-024, MR-025, SL-001 | [AR §5a.7] step 4 "status (per-kind lattice from the schema)"; with every SL row. |
| SM-002 | AR-5a.7-r02-scalar | MR-006, MR-007, MR-008, MR-009 | "enum / number scalar". |
| SM-003 | AR-5a.7-r03-counter | MR-026 | "counter (`Incr`)". |
| SM-004 | AR-5a.7-r04-set | MR-027 | "set". |
| SM-005 | AR-5a.7-r05-text | MR-028, MR-031, MR-032, MR-036, MR-038 | "text", with the removed-text guard. |
| SM-006 | AR-5a.7-r06-parent-order | MR-039, MR-040, VA-001 | "`parent` / `order`". |
| SM-007 | AR-5a.7-r07-existence | MR-041, MR-042, MR-043, MR-044, EP-001, AP-004, AP-005 | "existence"; with every EP row. |
| SM-008 | AR-5a.7-r08-structural-edge-to-deleted | VA-004 | "structural edge to a node deleted on the other side". |
| SM-009 | AR-5a.7-r09-supersedes | VA-006 | "`supersedes`". |
| SM-010 | AR-5a.7-r10-owner-authority | MR-010, MR-011, MR-014, MR-016, MR-019 | "owner-authority fields". |
| SM-011 | AR-5a.7-r11-schema | MR-052, MR-053, MR-055, VA-010 | "schema". |
| SM-012 | AR-5a.7-r12-pinned-commit | MR-048, MR-050 | "`pinned_commit` on citations". |
| SM-013 | AR-5a.7-r13-r4-composite | LM-001, LM-004, LM-005, LM-006 | R4 observation composite. |
| SM-014 | AR-5a.7-r14-r4-existence | LM-007, LM-008, LM-010 | R4 existence of derived-uid kinds. |
| SM-015 | AR-5a.7-r15-r4-status | MR-024, MR-025, SL-040, SL-041 | R4 artifact status. |
| SM-016 | AR-5a.7-r16-r4-root-path | VA-009, PC-001, PC-002 | R4 (root, exact path). |
| SM-017 | AR-5a.7-r17-r4-anchors-globs-pathmoves | LM-016, LM-017, LM-018, LM-019, LM-021 | R4 anchors, globs, `path_moves`. |
| SM-018 | AR-5a.7-r18-r5-named-query | MR-057, MR-058, MR-060, MR-061, MR-062, VA-011, VA-012 | R5 named-query definition. |
| SM-019 | AR-5a.7-step0-sync-first | PR-001, VB-017 | - |
| SM-020 | AR-5a.7-step1-lca-rvb | VB-001, VB-004, VB-005, VB-006, VB-013, VB-014, VB-018 | - |
| SM-021 | AR-5a.7-step6-validators | VA-001, VA-002, VA-003, VA-004, VA-009, VA-010, VA-011, VA-012, VA-013, VA-014 | I37′ order. |
| SM-022 | AR-5a.7-step7-emit | PR-011, RE-003, RE-004, RE-005, RE-007 | - |
| SM-023 | AR-5a.7-step8-land-or-stage | LS-001, LS-002, LS-003, LS-004, PR-013, PR-014, PR-015 | - |
| SM-024 | AR-5a.8-taxonomy | CM-001, CM-009, CM-020 | With every CM row. |
| SM-025 | AR-5d.3-node-40-at-merge | MR-042, MR-043, VA-004, EC-012 | "main only read #40 → delete wins"; "modified → `DeleteVsModify`"; "structural edge → `DanglingEdge`"; historical edges become tombstone references. |
| SM-026 | 50-4.4-versioning-and-merge | MR-058, MR-060, MR-061, MR-062, VA-011, VA-012 | [50 §4.4] "Versioning and merge". |
| SM-027 | 60-3.4-rvb | VB-005, VB-006, VB-013, VB-014, VB-015, VB-016, VB-018 | [60 §3.4] "The recursive virtual base" and its GT6 properties; VB-018 reads its "conflicts whenever they differ" with I25′ ([OP-18]). |
| SM-028 | AR-5a.5-revert-cherry-pick | DM-003, DM-004, DM-005, DM-006, DM-007, DM-008, DM-012, DM-013, DM-016, DM-017 | - |
| SM-029 | AR-5b.6-import-merges | DM-010, DM-011, DM-014, DM-015 | I30′. |
| SM-030 | AR-11-33-dropped-body-merge | CS-011, MR-032, MR-038, PR-016 | Owner decision #33 (OQ-A-7): a body key whose case needs diff3 while b, o or t is a dropped body takes a `TextHunk` whose sides are the hashes ([F12 §7.5] "A dropped body"; spec sync 3). |

## Coverage

Rule tables specify semantics, not bytes; the byte layouts of the values and codes named here are in [F06]
(`Conflict`, `Violation`, `Resolve` ops), [F07] (the canonical key classes), [F08] (the schema's class and
`uid_derivation` columns) and [F12] (the conflict-class enum). Rows of the M0 checklists this file covers:

| Checklist row | Covered by |
|---|---|
| [60 §2.5] "Schema as data": fields (type, lattice, class); edges (class) | `field-class`, `edge-class`, `status-lattice`, `merge-classes` |
| [60 §2.5] "Canonical form" item 10 key classes (as merge keys) | §2, `merge-classes` |
| [50 §8.1] F3: a named query is one atomic value for merge | MR-057 to MR-062 |
| [50 §8.1] F18: `QueryInvalid`, `QueryCycle` and the named-query merge validator | VA-011, VA-012, CM-018, CM-019 |
| [40 §2.11] R-2: merge classes `observation` and `identity` in the schema's merge-class enum | MC-015, MC-016, FC-106 to FC-114 |
| [40 §2.11] R-3: the merge re-key rule | MC-020; rules in [RULES/link-merge-rules] |
| [40 §2.11] R-12: I-F1 at merge | VA-009 |
| [60 §3.4]: the recursive virtual base | `virtual-base`, MR-001 to MR-004 |
| [AR §3.4] I25′, I31′, I34′, I36′, I37′, I41′ | MR rows (I25′), VB-005/VB-006 with MR-001 to MR-004 and VB-018 (I31′), DM-012/DM-013 (I34′), RE-001/RE-002 (I36′), `validators` (I37′), PR-002 (I41′) |
| [AR §11] #33, OQ-A-7: a dropped body in the text rule ([F12 §7.5] "A dropped body", [F06 §8.1] DB-7; spec sync 3) | CS-011, MR-032, MR-038, PR-016, SM-030; GT2 compares the model's result with the engine's from M2, when the engine has bodies ([API §2.2]) |

No X-F row and no [90 §10.1] item concerns merge rules.

## Holes

None. No merge rule depends on a value an M0 measurement decides: `store.kahn-fallback-edges` changes only how the
engine checks VA-003, not the result, and the model always checks by full recomputation.

## Open points for the review

Each point names the rows it affects and the chapter or WP that owns the final text.

1. **Scope of "owner-authority fields"** (MC-003, MC-004, MR-010 to MR-020, FC-064, FC-072, FC-078). [AR §5a.7] lists
   "owner-authority fields (`authority = owner`, `owner_quote`)". This table takes the narrow reading: the
   `owner_quote` field, and the `authority` field whenever `owner` is involved. The broad reading — every field, the
   status and the body of a node whose `authority` is `owner` — would also protect a rule's `text` from a lane's edit,
   which fits "a lane may never override an owner ruling" ([11 §5.4]). Two further choices: "otherwise conflict" is
   read as "both sides changed" (I25′ forbids a conflict on a one-sided change), and when dst is `main` a lane's change
   is dropped with no record, because the taxonomy has no hint for it. The owner chooses the reading; a hint class for
   the drop would be [F12]'s.
2. **Finding lattice** (SL-027 to SL-032). [AR §3.2] writes `open < (confirmed | refuted) < (fixed | deferred |
   withdrawn)`, which makes `refuted < fixed`: a side that refuted and a side that confirmed and fixed would join
   silently to `fixed`, the S12 hazard. [AR §3.6] allows `open → deferred | withdrawn` and only `confirmed → fixed`.
   This table uses the order of [AR §3.6]'s transitions (fixed above confirmed only; deferred and withdrawn above open
   only), so every disagreement is a `StatusFork`. [AR §3.2]'s notation is corrected by WP-14.
3. **Status and resolution** (MR-025, FC-007). The status key carries the resolution ([AR §4.6] item 10). Equal
   statuses with different resolutions (for example `done/completed` and `done/duplicate`) are treated as a
   `StatusFork`; the design is silent.
4. **Question `answered`** (SL-024, SL-025). [AR §3.2] and [AR §3.5] derive `answered` from a visible `answers` edge,
   while [AR §3.6] writes it as a transition. If the stored status never holds `answered`, SL-025 is unreachable and
   `answered` is derived only. WP-14 decides whether `answered` is stored.
5. **`DeleteVsModify` details** (RS-008, EP-006 to EP-014, MR-042). (a) The design names defaults only for tasks
   (`delete-wins`) and knowledge (`resurrect`); every other random-uid kind is proposed `resurrect` because the conflict
   value always lands, so resurrecting loses nothing and keeps structural referrers valid. (b) For `artifact` ("no
   automatic policy") and root nodes, dst's state stands provisionally. (c) Under `delete-wins` the conflict value must
   keep the modifying side's other keys, or `resolve --take` cannot restore them. **Settled** (review pass 1, S1-22):
   a `live` side carries its node image of value keys inside the closed set ([F06 §6.2] `snap`, [F06 §6.3]); the
   hierarchy key and the out-edges, which the image does not hold, are restored from that side's state at the
   conflict's introducing commit by ordinary ops of the `Resolve` commit ([F12 §6.5]; RS-008). No hashed byte changes, and the model
   computes the same side state from history. (d) A tombstone `.moi` file keeps no fields ([AR §5b.2] rule 8), so the
   `conflict existence` line of a provisionally deleted node needs a place in the image. **Settled** by [F14 §6.8.1]: a
   provisionally deleted node is a tombstone file carrying its `conflict existence` line.
6. **"Modified" for `DeleteVsModify`** (CS-014). Defined as any change to a key whose first component is the uid,
   out-edges included and in-edges excluded. This matches [AR §5d.3]'s three cases (read, modified, structural edge
   added) and is proposed as their exact boundary.
7. **Both sides deleted, differently** (MR-046). The node is deleted with dst's `reason` and `replaced_by`. The
   alternative is a `FieldEdit` on the existence key, which would need a conflict on a tombstone (point 5 d).
8. **Edge removed on one side, re-pinned on the other** (MR-051; also LM-022 for anchors). Proposed `FieldEdit` on the
   edge key, so neither the removal nor the re-pin is lost silently. Add-wins would keep the edge; remove-wins would
   drop the re-pin.
9. **Where `SupersedeFork` sits** (VA-006, CM-005). Two superseders are two different edge keys, while a conflict value
   needs one key. Proposed: the value sits on the src-side superseder's edge key (base `absent`, ours `absent`, theirs
   present). WP-12 fixed that key form ([F12 §6.2]). This file first also proposed that both superseders be
   `conflicted`; it now follows [F12 §6.3], where `conflicted` follows key ownership, so only the src-side superseder
   (the owner of the edge key) is flagged, and flagging the other would need a derived rule beyond ownership ([F12] open
   point 7).
10. **Validator classes the taxonomy lacks** (VA-005, VA-007, VA-008, VA-013, VA-016). [AR §5a.8] names no class for a
    forest deeper than 12 (I4), a `duplicate_of` chain of two (I7), `runs_in` or `answers` cardinality, or a merge that
    would write read-only fields on `plan/*` (I33′). Proposed: structural classes, since each breaks an invariant that
    I12 requires at every head. **Settled** in review pass 1 (P1-21): [F19 §12.2] assigns `DepthExceeded` (67),
    `Cardinality` (68) and `PlanMask` (72), [F12 §7.9] gives their `Violation` keys and [F13 §5] V05, V07 and V12 emit
    them; VA-005, VA-007, VA-008 and VA-013 now emit them (basis `derived`), CM-023 to CM-025 list them, and the `order`
    column takes [F13 §5]'s positions. Separately, VA-016 proposes that every
    edge kind declared acyclic be checked with `Cycle`, because two branches can close a cycle that each branch's
    "acyclic by construction" cannot see.
11. **Field types outside the closed type set** (FC-004, FC-005, FC-050, FC-051, FC-077, FC-081, FC-104). The closed
    type set of [AR §3.1] has no list, struct or symbol type, yet [AR §3.2] declares `list<…>` and struct fields and
    `sym` fields. Proposed: lists and structs merge as atomic scalars; `sym` fields are scalars. `title` and
    `abstract` are single-line and merge as scalars (`FieldEdit`, not `TextHunk`); the `.moi` conflict key for `title`
    is WP-15's. WP-14 maps each field to the closed set.
12. **`applies_to` per component** (FC-058 to FC-069). Proposed: each of roles, phases, lanes and globs merges as its
    own set, globs with composition. Because an empty `applies_to` means `*`, a union can narrow a rule that one side
    widened (base {X}; one side removes X, meaning "all"; the other adds Y; the result is {Y}). The review decides
    whether a side that empties a component should conflict instead.
13. **diff3 must be one specified function** (MR-031, CS-011). Whether diff3 finds a conflict depends on the line
    diff. The model and the engine agree only if [F12] specifies the diff (FL-1's histogram diff) and the hunk rule
    exactly; the model then implements that text independently. Answered by [F12 §7.5] (the line diff HD and diff3).
14. **Removed-text guard** (CS-012, MR-036). The design names the guard without a formula. Proposed: for each side S,
    the lines S has and the merged text lacks (as multisets) must all be in the base. A correct diff3 always passes, so
    the guard catches hunk misalignment and repeated lines. [F12 §7.5] states the guard over its lines.
15. **Kleppmann details** (RS-007, MR-039, MR-040, CS-013, VB-011). **Decided in spec sync 2b** (WP-91 review, fix and
    closure; [F12 §5.3] VM-7, [F12 §7.4]): per-commit steps. Each commit a side made since the base that has hierarchy
    entries is one step keyed by its (hlc, commit id) and valued in that commit's state; after a step that leaves a
    cycle, the step's moves are undone least uid first, and a key whose last move was undone is `kleppmann-skipped`. The
    first draft gave each key one move, its side's final value keyed by the newest commit that changed it; moves of one
    commit then shared a key and the uid tie-break ordered them, so a parent/child swap made in one `TX` was skipped,
    and a side that restructured over several commits met cycles its own history never had (a three-commit restructure
    staged `#1.parent HierarchyCycle` on `sync` and on the merge into `main`). A one-step-per-order-key reading (moves
    with one (hlc, id) applied together) fixed only the first; per-commit steps contain it within a commit and replay a
    linear chain of single-parent commits from the base through that side's own forests, so, merged into a dst that made
    no commit since the base, it never skips (RS-007; the known cases outside it are open point 35). The (0, 0) step
    covers a side whose value differs from a base its commits do not start at (`--base`, a virtual base); a revert or a
    cherry-pick starts from o instead (open point 35 (ii), spec sync 3). "Skipped and logged" is read together with
    `HierarchyCycle` being structural: the move is undone in the staged candidate and the merge stages.
16. **Schema merge details** (MR-055, MR-056, CS-017). Two different definitions of one new item are proposed as
    `SchemaConflict`; identical strengthenings on both sides are `same` and clean. Enum integers are "never reused"
    ([AR §3.4] I11), but two lanes that each add an enum value may pick the same integer; whether enum integers are
    allocated store-wide, like `#N`, is WP-14's.
17. **Named queries** (MR-060, MR-061). Proposed: under `DROP` versus modify, the modified definition stays until
    resolved; when both sides reach the same canonical-AST hash with different texts, dst's text lands.
18. **Merging over conflict values** (MR-001 to MR-005, RS-010, VB-014, VB-018). Settled in review pass 1 (S1-15) with
    [F12 §5.4] RVB-1 to RVB-4. [60 §3.4] and [AR §5a.7] step 1 say a key whose virtual-base value is a conflict value
    "conflicts whenever they differ". Read literally, a side that still holds the base's conflict value (it never
    touched the key) would conflict with a side that resolved it, which I25′ forbids and which MR-004 does not do at a
    real LCA. The rows now run in the order MR-001 (same: clean), MR-003 and MR-004 (one side untouched: the other
    side's value lands, clean), MR-002 (both changed and differ: conflict), so the sentence reads "conflicts whenever
    both sides changed it and they differ" (VB-018). MR-002's conflict value takes the inner base b′ (b's
    `base` field), the flat sides and the class of the key's value rows on them, else b's class (RS-010), so it never
    nests. [60 §3.4], [AR §5a.7] step 1 and [AR §3.4] I31′ should be edited to that wording at WP-81a, and [60 §3.13] GT6
    gains the property "one side untouched → the other side's value" ([F12 §5.7] VBC-3). The rows changed, so the owner
    re-signs this file (V3). MR-005 (a plain base, both sides changed, one holding a conflict value) was a gap; review
    pass 1 (P1-21) adopted [F12] open point 3 as [F12 §5.4]'s normative rule, and MR-005 now takes it through RS-015:
    the plain base b, the flat sides, the class of the key's value rows on them, else the conflicted side's class (o's
    when both are conflicted). RS-010 and RS-015 give an existence key's provisional state as [F12 §5.4] does. The
    owner re-signs the rows (V3). Spec sync 2b (WP-91 review): over a conflict-valued base the rows compare by
    [F12 §5.4]'s ≈, which also equates a conflict value with its exchanged orientation; with byte equality a lane that
    left its own merge's conflict unresolved would be clean in one LCA order and a spurious MR-002 conflict in the
    other, so VBC-3 held for one lane only (§2, MC-001, CS-006).
19. **Recursive virtual base details** (VB-003, VB-007 to VB-011, VB-019). Proposed: no common ancestor means an empty
    base; inside a virtual merge dst is never `main`, validators do not run, dst is the earlier fold, the Kleppmann
    steps are the real commits since the inner base (VB-011, spec sync 2b), and no automatic policy or outer `--strict`
    applies (VB-019, added for [F12 §5.3] VM-2 and
    [F12] open point 8). [F12 §5.3] VM-1 to VM-8 state the same rules.
20. **`DATA` versus `FieldEdit`** (CM-007, DM-013). [AR §2.7] and [AR §5a.5] record a `DATA` mismatch "as a
    `FieldEdit` conflict value", while [AR §5a.8] lists `DATA` as a class. This table emits `FieldEdit`; WP-12 decides
    whether `DATA` keeps an enum code. **Decided** by [F12 §6.1] (open point 5 there): `DATA` has no code; the case is
    stored with its key's `both` class and told apart by the commit's kind (CM-007).
21. **`NotFound` in state terms** (DM-012). The design states it on ops ("a `RemoveEdge`/`SetField` whose target …
    no longer matches"); the model works on states. The state-based definition is proposed.
22. **Conflicted nodes and merges into `main`** (PR-003, MR-004). [AR §5a.7] step 0 says the merge "can still be
    refused for unresolved `conflicted` nodes on src". Proposed: always refused, so conflicts are resolved on the lane
    and never land on `main` from a lane.
23. **Hint triggers** (HT-001 to HT-003). [AR §5a.8] names the hints; their triggers come from proposal B
    ([11 §5.4] step 5), which the design of record did not adopt word for word.
24. **`mentions` at merge** (EC-023). Proposed: `mentions` edges merge as ordinary keys, and the write-time `#N`
    re-parse does not run on merged bodies, as on import ([AR §5b.6] step 5, CL2).
25. **`contradicts` stored in one direction** (EC-022). Two lanes may add A→B and B→A, which are two keys. Proposed:
    the write path stores a symmetric kind with the lower uid as source, so both sides produce one key (WP-14).
26. **Values of `merge.policy.<kind>`** (AP-001 to AP-005). The design gives `ours` and `theirs` as opt-in examples and
    `delete-wins`/`resurrect` for `--policy`. Proposed: the key takes `none`, `ours`, `theirs`; `--policy` also takes
    `delete-wins` and `resurrect`; an automatic `ours`/`theirs` emits no conflict value. WP-18 registers the key.
27. **`IdCollision` in a merge** (MR-045). Proposed as a structural violation for two creations of one random uid,
    which only a fault can produce.
28. **`phase_state` has no lattice** (FC-036). It merges as a scalar; the "14 states" of [AR §3.2] do not state a
    merge order.
29. **Verdicts are immutable** (FC-091 to FC-097). Writes are refused after creation, so a merge sees two-sided
    changes only on `status`; the text rows are listed for completeness.
30. **`root` fields** (FC-108, FC-137). [40 §2.2] classes an artifact's `root` as `scalar` although it is a uid
    derivation input; no verb changes it. The root node's `root` is proposed `identity`. WP-14 aligns the two.
31. **`finding.where` with a section reference** (FC-088). `file:symbol@sha` is an `at` edge ([AR §3.3]); the edge kind
    for a section reference is not named. WP-14.
32. **`abs` artifact fields** (FC-124, FC-126, FC-130). [AR §3.3] says `lane.worktree_path` and `run.script_path`/
    `journal_path` "are `abs` artifacts"; whether the field holds a path or a ref to an artifact node is WP-14's. Either
    way the class is `scalar`.
33. **Reverting a creation** (DM-017; review pass 1 round 2, residue of S1-6 and A1-4). A revert is a three-way apply
    with base state(c) and src state(p1(c)), so for a node that c created, src holds `absent` and `theirs-only` would
    make the node absent, a state no op of [F06 §7.4] can write ([F12 §7.8]). [F06 §7.10] inverts a `Create` to a
    `Delete` and gives `CreateDeleted` no inverse ("a revert leaves the tombstone"); DM-017 states that in state terms.
    The tombstone's `reason` and `replaced_by` are those of [F06 §7.10]'s inverse `Delete`: the empty reason and no
    replacement, as R-MODEL proposed (review pass 1 round 3, closure NC-9; R-SPEC-F's alignment edit).
34. **Empty merge results** (CS-001, CS-011, RS-004 to RS-006, RS-013; review pass 1 round 2, residue of S1-1 and
    S1-34). The rules produce `absent`, never an empty value: a sum of 0, an empty `union3` and an empty diff3 result
    are `absent` ([F08 §5.3], [F08 §6.2], [F12 §7.5]), so the model's states compare with the engine's canonical states
    byte for byte. [F12 §7.5]'s length bound is part of the case `both-diff3-clean`: a clean diff3 longer than 65,536
    bytes is a `TextHunk`, as [F12 §7.5] says.
35. **Spurious Kleppmann results** (RS-007, MR-039, MR-040, CS-013; spec sync 2b: WP-91 fix 2, the WP-91/92 sync closure
    and its arbiter ruling; **decided in part** by the owner on 2026-10-06, OQ-A-6 (a), and applied in spec sync 3). As
    spec sync 2b left it, RS-007 replayed every side from B, made a two-parent commit a step only for its first-parent
    keys and undid every move of a cycle-closing step, and three known cases gave a hierarchy result that the sides'
    histories do not call for. The owner took Option A, with case (i) in its narrow form; RS-007 now states the three
    rules, and the reference model is changed to follow them in wave 3b ([m0/PLAN §5]).
    (i) **A two-parent commit inside one side.** A merge or `sync` that kept its first parent's value of a key the
    merged branch had changed (for example after resolving a skip to `ours`) was no step for that key, so the merged
    branch's move could be that key's last step and win. Example: lane/x puts #2 under #1; main then puts #1 under #2;
    `sync lane/x` stages `#1.parent HierarchyCycle`, which is resolved to `ours` (#1 at the root, #2 under #1). lane/y,
    forked before main's move, puts #2 under #4 after lane/x's move and before main's. `merge lane/x --into lane/y`
    replayed lane/x's move, lane/y's move and main's move; main's move, #1 under #2, then closed no cycle and applied,
    so the merge landed clean with #1 under #2, which neither lane holds. **Decided:** a two-parent commit is also a
    step for the hierarchy keys where its state differs from its second parent's, limited to keys that a commit of
    A(p₂) \ A(p₁) moved. "Moved" is read as "is a step key of": a merge inside the merged branch counts with its own
    second-parent keys, since its step can be the key's last one too. In the example the sync's step sets #1 back to the
    root after main's move, and the merge lands #1 at the root and #2 under #4 (lane/y's later move, MR-040). The broad
    form (every key where the state differs from the second parent's) was not taken: it re-keys a side's own earlier
    moves to the merge commit's (hlc, commit id), so a routine `sync` could make that older move beat a third branch's
    later one, against MR-040.
    **The reading of "moved" is an interpretation**, not the owner's literal text, and awaits confirmation (owner
    question OQ-A-11, below; independent check of spec sync 3). The literal narrow form counts only a commit's
    first-parent hierarchy keys as its moves; the recursive reading also counts an inner merge's second-parent keys,
    which that merge kept rather than moved. Example where the two differ, in (hlc, commit id) order Y, X, L, N, M, with
    #1 under #3 and #2, #3 at the root in the base: lane/b puts #2 under #1 (Y); main puts #1 under #2 (X); `sync lane/b`
    (N) replays Y and then X, whose move closes #1 → #2 → #1 and is undone, and `#1.parent` is resolved to `ours`, so N
    holds #1 under #3 and #2 under #1, and N's step keys are {#1} under either reading (X moved #1). lane/a, forked at X,
    puts #3 under #1 (L). `merge lane/b --into lane/a` (M) replays from X: Y's move closes #2 → #1 → #2 and is undone, L
    applies, N's #1 under #3 closes #1 → #3 → #1 and is undone; both keys stage and are resolved to `ours` (#1 under #2,
    #2 at the root, #3 under #1). M's state differs from N's at #1, #2 and #3; of the commits of A(N) \ A(L), Y has the
    step key #2 and N the step key #1 but moved nothing against its first parent, so M's second-parent keys are {#2}
    under the literal reading and {#1, #2} under the recursive one. main then makes a commit with no hierarchy entry and
    lane/a syncs: the sync replays Y, L, N and M from X, and Y's and N's moves are undone again. Under the recursive
    reading M's step sets #1 under #2 and #2 at the root, both no-ops that apply, so the sync lands lane/a's own
    hierarchy. Under the literal reading M's step sets only #2, #1's last move is N's undone one, and the sync stages
    `#1.parent HierarchyCycle` on a key that main has not moved since X and that lane/a already holds. The recursive
    reading keeps the boundary that MR-040's argument against the broad form rests on, read as "a key some step of the
    merged branch sets": a side's own move that no step of the merged branch sets keeps its (hlc, commit id). RS-007,
    [F12 §7.4] and the model follow the recursive reading until OQ-A-11 is decided.
    (ii) **A revert or a cherry-pick.** RS-007 replayed every step from B, and a pick's B = state(p₁(C)) is not where
    dst's commits start (nor a revert's B = state(C) when dst's commits since C do not all descend from C), so dst's own
    commits replayed from B could close a cycle they never met, even when C has no hierarchy entry. Example: lane/a puts
    #1 under #2, then sets #4.priority (C); main puts #2 under #1, then #1 under #3; picking C onto main staged
    `#2.parent HierarchyCycle` with #2 at the root. **Decided:** a revert or a cherry-pick starts from o's (parent,
    order), dst has no step, and src's one step (C's) leaves out each key that a dst commit after C, by (hlc, commit
    id), set; neither side has a (0, 0) step. The pick in the example has no step and lands main's hierarchy with C's
    priority.
    (iii) **A no-op move in a step that closes a cycle.** The undo exempted no move, so a move that sets the value its
    node already holds could be the least uid on the cycle: undoing it changed nothing, but its key became
    `kleppmann-skipped`. Example, with #1's uid below #2's: ours, in one commit, puts #3 under #2 and #1 under #3;
    theirs, in a later commit, puts #1 under #3 and #2 under #1. Both `#1.parent` and `#2.parent` staged, although both
    sides hold #1 under #3. **Decided:** a move that sets its node's current value is never undone and does not make its
    key `kleppmann-skipped`. In the example only `#2.parent` stages, the sides' real disagreement; if ours then puts #2
    under #4 in a still later commit, nothing stages.
    **Not decided** (spec sync 3). The owner or an arbiter decides these before the merge table's V3 signature and
    before the engine implements RS-007; until then RS-007 stands as written, and the reference model follows it, with
    the tests that pin its current results (E1, E2 and (vi) below). Owner question OQ-A-11, which the independent check of
    spec sync 3 asked for before the merge table's V3 signature and before WP-91 closes, puts (v) with E1 to E4, (vi)
    and the reading of "moved" in (i) to the owner; (iv) has no proposal yet and stays here. GT6's I25′ property over
    random DAGs with interleaved sync and merge (M3) would meet E1 to E3, each a structural staging on a key that only one
    side changed.
    (iv) **`--base` and a virtual base.** Their steps still start from B, with the (0, 0) steps. A side's commits since
    the chosen commit, or since an inner base, that do not all descend from it can close a cycle they never met. There
    is no proposal yet.
    (v) **The limit of the narrow form.** A two-parent commit re-asserts only keys that its second parent's branch
    moved. A key it kept from its first parent is not re-asserted, so when the replay undoes the first-parent side's
    move of that key, the key ends `kleppmann-skipped`. That happens when the move was made on a state that B does not
    hold (a commit of A(side) \ A(B) that does not descend from B, which every `sync` leaves behind it, because its
    second parent becomes the next base), or when the move closed a cycle in that commit's own merge and the resolution
    kept it. OQ-A-6 (a) expected three model tests to flip; under the narrow form two do (the pick of (ii) and the no-op
    move of (iii)), and the third, E2 below, still stages. The owner question's recommendation counted on Option A
    fixing the daily sync-then-merge path; E1 and E2 show that it does not, with or without a resolution (review
    RS-007-A).
    - **E1**, with no resolution: lane/x puts #2 under #3, then #1 under #2, then #2 back at the root; main puts #3
      under #1. `merge lane/x --into main`: step 0's sync lands clean (#1 under #2, #3 under #1), and its step holds
      only #3. The merge then replays lane/x's moves from main's state, where #1 under #2 closes the cycle
      #1 → #2 → #3 → #1 and is undone. Nothing re-asserts #1, so the merge stages `#1.parent HierarchyCycle`, although dst made no commit
      since the base and main never moved #1. This is the default `merge --into main` on the most common workflow: a
      structural staging on a key that one side never touched, against the intent of I25′.
    - **E2**: (i)'s example up to the sync, then lane/x puts #1 under #3: `merge lane/x --into main`, with main
      unchanged since the sync, replays lane/x's first move from B (main's move, #1 under #2), where it closes a cycle
      and is undone. The sync's step re-asserts only #1, which main moved, so the merge still stages `#2.parent
      HierarchyCycle` with #2 at the root, although lane/x holds #2 under #1 and main never moved #2.
    - **E3**: the same history, but main next makes a commit with no hierarchy entry and lane/x syncs again: that sync's
      B is main's first move as well (the first sync's second parent), and it stages the same key. This is the daily
      path "sync, resolve, keep working, sync", and the merge is two-sided.
    - **E4**, a history whose commits all descend from B: lane/b puts #1 under #2; lane/a, forked at the same commit,
      then puts #2 under #1 and later #1 under #3; `merge lane/b --into lane/a` stages `#2.parent` (lane/a's move closed
      the cycle), which is resolved to `ours`. Merging lane/a into an unmoved `main` replays lane/a's first move after
      lane/b's, undoes it again, and the merge commit's step re-asserts only #1.

    The broad form of (i) re-asserts these keys, at the cost to MR-040 stated under (i). Proposed, not decided: (A) when
    tip(dst) is B, every hierarchy key takes src's value without a replay. Such a merge is one-sided: dst touched no key
    since the base, so I25′ forbids a conflict on any key, and (A) is what I25′ asks of a merge whose dst tip is B for
    the hierarchy keys. It covers every merge into `main` after step 0's sync (VB-017), so E1, E2 and E4, but not a
    `sync` or a merge between lanes (E3). (B) Start the replay from a commit that every commit of A(o) \ A(B) and of
    A(t) \ A(B) descends from, and replay the commits of both sides since it, so that no commit is replayed on a state
    it was not made on; this covers E1, E2 and E3, not E4. (C) The broad form of (i), which covers all four. (D) A
    two-parent commit is also a step for each hierarchy key that its merge's resolutions set (the resolved keys of
    [F12 §9.4] step 2), which needs those keys recorded with the commit; with (B) it covers all four without re-keying
    the side's other moves.
    (vi) **A move that changes only its node's order.** A hierarchy value is (parent, order), so a move that reorders a
    node under the same parent changes its node's value and can be undone, although a move that keeps its node's parent
    cannot close a cycle: the forest argument needs only the parent changes. Example, with #1's uid below #2's: the base
    has #1 under #3; ours (commit 10) puts #3 under #2; theirs (commit 20), in one transaction, gives #1 a new order
    under #3 and puts #2 under #1. Theirs' step closes the cycle #2 → #1 → #3 → #2. #1's reorder, the least uid on the
    cycle, is undone first, which leaves the cycle in place, and then #2's move: both `#1.parent` and `#2.parent` stage,
    although both sides keep #1 under #3. Proposed (review RS-007-A): the undo takes the step's moves that changed their
    node's parent; a move that keeps its node's parent and changes only its order closes no cycle and is never undone
    (it never makes its key `kleppmann-skipped`). The example then stages only `#2.parent`, and #1 takes theirs' order.
    This changes results beyond OQ-A-6 (a), so it needs an arbiter ruling or the owner's call (OQ-A-11); RS-007, MR-039,
    CS-013 and [F12 §7.4]'s row would change together.
    **The recommendation R-MODEL gives in OQ-A-11**: for (v), (A) together with (B), which cover E1 to E4 between them without re-keying a side's moves and without a
    format addition, (B) being stated exactly by R-MODEL (the commit the replay starts from, chosen over a criss-cross as
    I31′ chooses a base) and checked by the arbiter before the V3 signature; (A) alone if (B) cannot be stated before
    then, leaving E3 recorded here; for (vi), the undo of only the moves that changed their node's parent; for (i), the
    recursive reading.
