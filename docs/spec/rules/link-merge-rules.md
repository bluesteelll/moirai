# Link merge rules (R4)

| Field | Value |
|---|---|
| Status | draft, pass 1 pending |
| Work package | WP-92 (R-MODEL, R4 part of the model), run inside WP-91's merge; format per [RULES/README] |
| Sources | [40 §2.2] merge classes of the file node; [40 §2.3] derived identity, predecessor order, dead-uid rule; [40 §2.4] roots, `path_moves` classes, globs; [40 §2.7] anchor uids; [40 §2.10] I-F1, I-F14; [40 §2.11] R-2, R-3, R-4, R-5, R-10, R-12, R-17; [40 §5.3] conflict resolution by observation; [40 §5.4] merge ritual; [40 §5.5] merge rules for link fields (authoritative); [40 §5.6] history operations; [AR §5a.7] step 4 R4 rows; [AR §5e.6]; [72 M5] sync residue; [60 §3.4] FL-7; [60 §4.2] "the merge rules incl. the re-key rule and the prefix history as data, owner-signed" |
| Cited as | [RULES/link-merge-rules]; a row as [RULES/link-merge-rules LM-005] |

## 1. What this table decides

The merge rules of R4's link intent ([40 §2.1]): file nodes (the widened `artifact` kind), root nodes with their
`path_moves`, `at` edges with their anchors, and path globs. They extend [RULES/merge-table] and run inside its
procedure (PR rows): the same inputs b, o, t, the same first-match evaluation, the same dispositions. Where [40]
reserves a rule, [40] wins over [AR] ([40 §5.5] is "authoritative" for these rows, [40 §2.11]).

The merge engine never reads the file system or git, so these rules are a pure function of the base, dst and src
states ([40 §5.5], [AR §3.4] I28′, I30′). Conflicts that only the file system can settle land as conflict values and are
resolved later by a settle in a writer tree (`link-resolution`), which is not part of the merge.

Which fields belong to which link class is decided in [RULES/merge-table] `field-class` (FC-106 to FC-120, FC-136 to
FC-138) and `edge-class` (EC-025). The artifact status merges by the generic `status` rows with the artifact lattice
(SL-039 to SL-041).

## 2. How to read a row

As [RULES/merge-table] §2. In addition:

- A file node's **observation composite** is one key whose value is the tuple (`path`, `oid`, `bytes`,
  `observed_git`, `observed_blob`, `relink`); it is equal on two sides only if all six fields are equal.
- An anchor's value is its selector block as the canonical form encodes it ([40 §2.11] R-10; [F07 §8.2], the block
  of [F08 §10.3]'s record): quote, prefix, suffix and a `range` anchor's end enter only as their BLAKE3-128 digests. Two anchors are therefore equal whether or not a store holds their text (a `hash-only` import,
  whose record has `text_unavailable` set, compares equal to a `full` one, [F07 §8.3]).
- "Gained since the base" for a set-valued field means: in the side's final value and not in b's value.
- S′ and S name the two sides of a composition: S′ is the side whose root node gained the directory move, S the side
  whose path is composed.

## 3. Cases and results

<!-- table: link-cases -->
| row | case | classes | definition |
|---|---|---|---|
| LC-001 | `both-same-path` | observation | `both`, and o's `path` equals t's `path` (exact bytes; the same root). |
| LC-002 | `both-compose` | observation | `both`; b is present; o's `path` ≠ t's `path`; and exactly one side S′ gained since the base, on the root node of the file node's `root`, at least one `path_moves` entry of class `explicit`, `confirmed` or `committed` whose `from` is a byte prefix of both b's `path` and the other side S's `path`. |
| LC-003 | `created-vs-dead` | derived-existence | The uid is `file-key`; b is `absent`; one side S holds the uid live with a status other than `removed`; the other side's final state for the uid is deleted, or live with status `removed`. |
| LC-004 | `created-both-live` | derived-existence | b is `absent`, and o and t both hold the uid live with a status other than `removed`. |
| LC-005 | `root-created-vs-dead` | derived-existence | As LC-003 for a `root-key` uid (a root node). No store's history reaches it: a root node is never engine-deleted and an `area` has no `removed` status ([F08 §11.3]; LM-013). |

<!-- table: link-results -->
| row | result | definition |
|---|---|---|
| LR-001 | `compose` | The composition of CP rows: S's composite with its `path` rewritten through S′'s gained entries and `relink = merge-compose/prefix`; the pre-composition path joins `aliases` (LR-004). |
| LR-002 | `equal-existence` | One live node. Its `created` commit is, of the two creating commits, the one with the least (generation, commit id), commit ids compared bytewise ([40 §2.3]); every other key of the uid merges by its own class with base `absent`. |
| LR-003 | `rekey` | The re-key of RK rows, performed before per-key evaluation (PR-007). |
| LR-004 | `union3-aliases` | `union3` of [RULES/merge-table] RS-005, plus every pre-composition path LR-001 produced for this node in this merge. |
| LR-005 | `union3-compose` | `union3`, then every element that side S added (in S's value, not in b's) is rewritten through the other side's gained directory moves (CP-009 to CP-011). |
| LR-006 | `refuse-internal` | No key takes a value: the merge is refused with [F19 §10.2]'s `internal` code (`internal error: <check> failed`, exit 1), as a failed internal check, the way [F08 §11.2] step 4 refuses an over-long derivation loop. Used only for a case that the chapters that own the data make unreachable; the model reports reaching it as a test failure naming the row, never as `SpecGap`. |

## 4. The link merge rules

<!-- table: link-merge-rules -->
| row | class | case | result | conflict | disposition | basis | source | note |
|---|---|---|---|---|---|---|---|---|
| LM-001 | `observation` | `same` | `take-o` | - | clean | design | [40 §5.5] | Neither side changed the composite, or both changed it identically. |
| LM-002 | `observation` | `ours-only` | `take-o` | - | clean | design | [40 §5.5] "one side changed" | - |
| LM-003 | `observation` | `theirs-only` | `take-t` | - | clean | design | [40 §5.5] "one side changed" | - |
| LM-004 | `observation` | `both-same-path` | `take-o` | - | clean | design | [40 §5.5] "both changed, same path" | "take dst's composite (content drift is re-observed at the next settle)". |
| LM-005 | `observation` | `both-compose` | `compose` | - | clean | design | [40 §5.5] "both changed, different paths, and one side's root node gained … a `path_moves` entry"; [OP-1] | "verified at the next settle" (LV-008). |
| LM-006 | `observation` | `both` | `conflict-value` | FieldEdit | value | design | [40 §5.5] "both changed, different paths otherwise" | The node is `conflicted`; resolved by observation in a writer tree fresh for every side (LV-001), otherwise by `links fix`. |
| LM-007 | `derived-existence` | `created-vs-dead` | `rekey` | - | clean | design | [40 §5.5] "created on side S … re-key, never resurrect"; [40 §2.10] I-F14 | [41 B1]: a removed identity is never brought back by a merge. |
| LM-008 | `derived-existence` | `created-both-live` | `equal-existence` | - | clean | design | [40 §5.5] "the same uid created on both sides"; [40 §2.3] | Two lanes that link one file create one node. |
| LM-009 | `derived-existence` | `same` | `take-o` | - | clean | derived | [RULES/merge-table MR-041] | - |
| LM-010 | `derived-existence` | `deleted-vs-modified` | `policy` | DeleteVsModify | value | design | [40 §5.5] "existed at the LCA", "engine-deleted on one side, modified on the other"; [OP-5] | "no automatic policy for `artifact`": EP-014 and EP-013 give `none`, so dst's state stands provisionally. |
| LM-011 | `derived-existence` | `ours-only` | `take-o` | - | clean | derived | [RULES/merge-table MR-043] | - |
| LM-012 | `derived-existence` | `theirs-only` | `take-t` | - | clean | derived | [RULES/merge-table MR-044] | - |
| LM-013 | `derived-existence` | `root-created-vs-dead` | `refuse-internal` | - | none | derived | [F08 §11.3]; [F19 §10.2] `internal`; [LQ/errors §5.5] E409; [40 §2.3]; [OP-4] | A root-node uid derives from the root name alone, so it cannot be re-keyed. [F08 §11.3] makes the case unreachable: `rm` of a root node is refused (E409's root-node case) and no other path deletes one, so a dead root node on one side is a failed internal check (review pass 1, P1-21). |
| LM-014 | `derived-existence` | `both` | `take-o` | - | clean | proposed | [RULES/merge-table MR-046]; [RULES/merge-table OP-7] | Both sides ended the node differently (for example deleted with different reasons): dst's state. |
| LM-015 | `alias-set` | `any` | `union3-aliases` | - | clean | design | [40 §2.2] "add-wins set"; [40 §5.5] | An alias never captures a new file. |
| LM-016 | `glob-set` | `any` | `union3-compose` | - | clean | design | [40 §5.5] "globs"; [40 §2.4]; [72 M5] | A composition changes a key only one side touched (CP-012). |
| LM-017 | `pathmove-set` | `any` | `union3` | - | clean | design | [40 §5.5] "`path_moves`"; [40 §2.11] R-5 | "add-wins union"; entries are values with their hlc, so every store orders them alike. |
| LM-018 | `anchor` | `same` | `take-o` | - | clean | design | [40 §5.5] "anchor existence"; [40 §2.7] | Identical captures on two lanes share one anchor uid and merge into one anchor. |
| LM-019 | `anchor` | `ours-only` | `take-o` | - | clean | design | [40 §5.5] "anchor existence", "one side repinned" | An anchor added, removed or re-pinned on one side lands. |
| LM-020 | `anchor` | `theirs-only` | `take-t` | - | clean | design | [40 §5.5] "anchor existence", "one side repinned" | - |
| LM-021 | `anchor` | `both-present` | `conflict-value` | FieldEdit | value | design | [40 §5.5] "both repinned differently" | A settle keeps the selectors that resolve `fresh` in the merged tree (LV-004). |
| LM-022 | `anchor` | `both` | `conflict-value` | FieldEdit | value | proposed | [40 §5.5]; [OP-7] | One side removed the anchor, the other re-pinned it. |
| LM-023 | `identity` | `same` | `take-o` | - | clean | design | [40 §2.2] "identity (immutable)" | - |
| LM-024 | `identity` | `ours-only` | `take-o` | - | clean | derived | [40 §2.2]; [40 §2.3] | Set once, by the creating side. |
| LM-025 | `identity` | `theirs-only` | `take-t` | - | clean | derived | [40 §2.2]; [40 §2.3] | As LM-024. |
| LM-026 | `identity` | `both` | `stage-take-o` | IdCollision | structural | proposed | [40 §2.3] "Verification"; [AR §5b.6] step 4; [OP-8] | One uid with different identity inputs on two sides: at least one fails its derivation. |

## 5. Path claims

I-F1 after a merge: at most one live `present` or `planned` file node per (root, exact path) on every view
([40 §2.10]). The check is validator VA-009 of [RULES/merge-table]; these rows fix it.

<!-- table: path-claim -->
| row | aspect | rule | basis | source | note |
|---|---|---|---|---|---|
| PC-001 | group | by-root-and-exact-path-bytes | design | [40 §2.10] I-F1; [40 §2.4] | Group the candidate's live file nodes by (root, exact `path` bytes). Paths equal only under case or normalization folding are different paths here: collisions of that kind are resolve-time states. |
| PC-002 | emit | pathclaim-on-each-composite | design | [40 §5.5] "(root, exact path)"; [AR §5a.8] | A group with two or more distinct uids: each node in it holds a `PathClaim` conflict value on its observation composite key ({base b, ours o, theirs t} of that node's composite) and is `conflicted`. Where the value sits is [OP-9]. |
| PC-003 | status-scope | present-or-planned | proposed | [40 §2.10] I-F1; [40 §5.5]; [OP-2] | I-F1 counts `present` and `planned` nodes; [40 §5.5]'s row says "two live present nodes". |
| PC-004 | disposition | value-lands-unless-strict | design | [AR §5a.8]; [RULES/merge-table LS-003] | `PathClaim` is a value conflict. |
| PC-005 | removed-excluded | removed-never-claims | derived | [40 §2.10] I-F1 | A `removed` node never claims its path, so a re-keyed node (RK) and its removed predecessor may share one exact path. |

## 6. Composition through directory moves

<!-- table: compose-steps -->
| row | step | action | basis | source | note |
|---|---|---|---|---|---|
| CP-001 | 1 | select-root-node | design | [40 §2.4]; [40 §2.3] | The root node of the file node's `root`: the `area` whose uid is the root-key derivation of that root name. |
| CP-002 | 2 | select-gained-entries | design | [40 §5.5]; [40 §2.4] | Entries of that root node's `path_moves` in S′'s final value and not in b's, with class `explicit`, `confirmed` or `committed`. `observed` entries never compose. |
| CP-003 | 3 | order-entries | design | [40 §5.5] "`path_moves`"; [F08 §5.5] | Ascending (hlc, from, to): hlc numerically, `from` and `to` bytewise. Entries equal in (hlc, from, to) differ only in `class` or `git` and rewrite alike; [F08 §5.5]'s stored order is this order extended to a total one. |
| CP-004 | 4 | rewrite-path | design | [40 §5.5] "compose: apply P→P′ to the other side's new path" | p := S's `path`; for each entry in order, if its `from` is a byte prefix of p, then p := its `to` followed by the rest of p after `from`. |
| CP-005 | 5 | result-composite | derived | [40 §5.5]; [OP-1] | The result is S's composite with `path` := p; `oid`, `bytes`, `observed_git` and `observed_blob` stay S's. |
| CP-006 | 6 | set-relink | design | [40 §5.5]; [40 §2.2]; [40 §2.11] R-17 | `relink` := `merge-compose/prefix`. |
| CP-007 | 7 | alias-pre-composition | design | [40 §5.5] "add the pre-composition path to `aliases`" | S's `path` before step 4 joins the node's `aliases` (LR-004). |
| CP-008 | 8 | verify-at-next-settle | design | [40 §5.5] | The composed path is an expectation: git's `merge.directoryRenames` defaults to `conflict`, so where git did not compose, the next settle re-binds on exact evidence (LV-008). |
| CP-009 | 9 | glob-literal-prefix | design | [40 §2.4] "Globs"; [OP-6] | A glob's literal prefix runs up to the last `/` before its first wildcard character. |
| CP-010 | 10 | glob-rewrite | design | [40 §5.5] "globs"; [40 §2.4]; [72 M5] | A glob element added on side S whose literal prefix starts with an entry's `from` has that leading `from` replaced by the entry's `to`, for each of the other side's gained entries (CP-002) in CP-003 order. Base elements are not rewritten here: the side that moved the directory rewrote its own globs in the move's commit. |
| CP-011 | 11 | glob-root | design | [40 §2.4] "Globs … rooted at `project`" | The entries used for globs are those of the `project` root node. |
| CP-012 | 12 | sync-residue | design | [AR §5a.3]; [AR §4.6]; [72 M5] | A composition or a re-key changes a key only one side touched; a `sync` records the composed value in its residue (a re-key's keys: RK-011). The model's canonical op list is the full state diff and needs no residue. |

## 7. Re-key, never resurrect

<!-- table: rekey-steps -->
| row | step | action | basis | source | note |
|---|---|---|---|---|---|
| RK-001 | 1 | when | design | [40 §5.5]; [RULES/merge-table PR-007] | For every uid matching LC-003, before any per-key rule, in real and virtual merges alike (VB-012). |
| RK-002 | 2 | name-predecessor | design | [40 §5.5] | U := the uid; S := the side holding it live and not `removed`. |
| RK-003 | 3 | derive | design | [40 §2.3]; [40 §5.5] "uid′ = uid(root, `origin_path`, U)"; [F08 §11.2] | q := U; uid′ := BLAKE3-128(lp("moirai-file-v1") ‖ lp(root) ‖ lp(origin_path) ‖ lp(q)), with S's `root` and `origin_path` and lp(x) = u32-le(len x) ‖ x. |
| RK-004 | 4 | skip-dead | derived | [40 §2.3] "Dead uids are never re-created"; [F08 §11.2] step 4; [F12 §7.6] step 2; [OP-3] | While uid′ is the uid of any node of the base, o or t — live at any path, live with status `removed`, or a tombstone: q := uid′ and uid′ := the derivation of RK-003 over q. This is [F08 §11.2] step 4's test with the view replaced by the three merge inputs. A loop that ran more times than the three states hold artifact nodes together is refused as an internal error, as in [F08 §11.2] step 4. |
| RK-005 | 5 | move-keys | design | [40 §5.5] "S's node becomes uid′ with S's fields and `origin_pred = U`"; [F12 §7.6] step 3 | On S's state every key owned by U ([F06 §2.3]: value keys, status, body, existence, hierarchy, and out-edges, `at` edges with their anchors included) moves to uid′, and `origin_pred` := q, the predecessor of RK-004's last derivation (U when RK-004 did not re-derive), so that uid′ verifies over its stored inputs ([40 §2.10] I-F2; [F08 §11.2] step 5). Derived detail: `created` is S's creating commit, since the node is S's. |
| RK-006 | 6 | repoint-added-referrers | design | [40 §5.5] "every edge incident to U that side S added since the LCA … is re-pointed to uid′ in the merge commit"; [F12 §7.6] step 3; [F12] open point 25; [40 §2.7]; [OP-10] | On S's state: every edge key present in S and absent in the base whose destination is U — every edge kind: `at` edges with their anchors, `produced`, `consumed`, `mentions`, `implements`, `cites` and the rest — is replaced by the same edge with destination uid′, its properties and anchors unchanged; every existence value `deleted(reason, replaced_by = U)` and every `ref`-typed field value equal to U (`replaced_by`, `parent` and the rest) that S set since the base is set to uid′. Anchor uids do not change: they derive from the source uid and `captured`. U's own out-edges move with RK-005. |
| RK-007 | 7 | predecessor-keeps | design | [40 §5.5] "U keeps the other side's state and referrers" | U keeps the other side's keys, anchors and referrers. |
| RK-008 | 8 | new-number | design | [40 §2.3] "A re-keyed node gets a new `#N`" | uid′ gets a new `#N` (store runtime `ALLOC`/`UIDX`, RE-012); U keeps its own. |
| RK-009 | 9 | pure | design | [40 §5.5] | A pure function of the two histories: every store computes the same uid′, equal to the uid a registration after the removal derives. |
| RK-010 | 10 | no-resurrection | design | [40 §2.10] I-F14; [40 §8.3.2] P13 | U is never live again through the merge; only `links fix --restore` or `Undelete` bring it back. P13's cases, which the model's suite runs against these rows: a file removed on one branch and an unrelated file re-created at the same path on another, merged in either order; a store that never held the removing branch; a registration after a checkpoint-granularity import at a path with two or more predecessors; and a re-keyed side that added non-anchor edges to U (`produced`, `mentions`, `cites`) and U's own out-edges, all of which must follow the node (RK-005, RK-006). |
| RK-011 | 11 | sync-residue | design | [40 §5.5] "a sync records the re-key with its re-pointings in its residue"; [F12 §7.6] step 5; [AR §5a.3]; [72 M5]; [OP-10] | A `sync` whose merge re-keys records in its residue every key RK-005 to RK-007 changed, because those keys differ from what `main`'s window alone yields. The model's canonical op list is the full state diff and needs no residue (CP-012). |

## 8. Settling link conflicts after a merge

These rows are not merge rules: the merge only lands the conflict values. They say which later, separate commit
resolves each, so the model's R4 part (WP-92) and the replay rows can check the whole path.

<!-- table: link-resolution -->
| row | conflict | resolved_by | condition | provenance | basis | source | note |
|---|---|---|---|---|---|---|---|
| LV-001 | FieldEdit-observation | settle-observation | fresh-for-every-side,exact-evidence | merge-observation/<evidence> | design | [40 §5.3] "Conflict values"; [40 §5.5]; [OP-11] | A settle in a writer tree resolves only when the tree is fresh for every side's value and holds exact evidence for the chosen path; "the newest side" is rejected. |
| LV-002 | FieldEdit-observation | render | until-resolved | - | design | [40 §5.3] | The link renders `ambiguous (merge conflict: b.rs \| c.rs)`. |
| LV-003 | FieldEdit-observation | links-fix | explicit | links-fix | design | [40 §5.5]; [40 §3.7] | - |
| LV-004 | FieldEdit-anchor | settle-fresh-candidate | exactly-one-fresh | - | design | [40 §5.5] "both repinned differently" | The settle keeps the candidate that resolves `fresh` in the merged tree; if both or neither do, the conflict stays. |
| LV-005 | PathClaim | settle-unify | exact-rename-in-fresh-writer-tree | git/r100 | design | [40 §5.5] "(root, exact path)"; [40 §2.3] "The residual case"; [OP-12] | E6 in a fresh writer tree shows an exact rename from one node's path to the other's. |
| LV-006 | PathClaim | links-fix-same-as | explicit | links-fix | design | [40 §5.5] | - |
| LV-007 | StatusFork-present-removed | links-fix-drop-or-restore | explicit | links-fix | design | [40 §5.5] | Never by path presence in the merged tree: the file there may be unrelated. |
| LV-008 | composed-path | settle-verify | next-settle | - | design | [40 §5.5] | The alias is checked by E4 and E6; the link re-binds on exact evidence. |
| LV-009 | DeleteVsModify-artifact | resolve | explicit | - | design | [40 §5.5]; [AR §5a.7] step 8 | No automatic policy. |
| LV-010 | merge-check-strict-links | refuse | missing,ambiguous,replaced,stale-anchor | - | design | [40 §5.4] | Part of the merge ritual, which reads the tree; never part of the merge engine. |
| LV-011 | merge-check-uncommitted | list | observed_blob-empty | - | design | [40 §5.4] | "commit the move first". |

## 9. History verbs and the working tree

<!-- table: link-history -->
| row | operation | effect | basis | source | note |
|---|---|---|---|---|---|
| LH-001 | undo, revert, cherry-pick | no-working-tree-change | design | [40 §5.6]; [AR §5e.6] | No moirai history operation touches the working tree. |
| LH-002 | undo, revert, cherry-pick | next-settle-rebinds | design | [40 §5.6] | After a history operation that changed a path, the next settle re-binds to where the file is if exact evidence exists; otherwise the link is `missing` (P5). |
| LH-003 | revert | path_moves-by-set-rules | design | [40 §5.6] | Reverting the commit that added a `path_moves` entry removes it through the ordinary set rules. |
| LH-004 | cherry-pick | path_moves-by-set-rules | design | [40 §5.6] | A cherry-pick adds the entry through the ordinary set rules. |
| LH-005 | revert | warn-file-revert | design | [40 §5.6]; [AR §5e.6] | A plain `revert` of a file-moving commit warns and names `file revert`. |
| LH-006 | file-revert | fs-aware-inverse | design | [AR §5e.6] | `file revert COMMIT` is the file-system-aware inverse. |
| LH-007 | image-import | foreign-setfield | design | [40 §5.6] | A hand-edited `field path:` in the image is a foreign `SetField`, validated like any op; it moves nothing on disk. |

## 10. Source map

<!-- table: link-source-map -->
| row | source_row | realized_by | note |
|---|---|---|---|
| LX-001 | 40-5.5-r01-composite-one-side | LM-002, LM-003 | - |
| LX-002 | 40-5.5-r02-composite-same-path | LM-004 | - |
| LX-003 | 40-5.5-r03-composite-compose | LM-005, CP-001, CP-002, CP-003, CP-004, CP-005, CP-006, CP-007, CP-008 | - |
| LX-004 | 40-5.5-r04-composite-otherwise | LM-006, LV-001, LV-002, LV-003 | - |
| LX-005 | 40-5.5-r05-created-both | LM-008 | - |
| LX-006 | 40-5.5-r06-rekey | LM-007, RK-001, RK-002, RK-003, RK-004, RK-005, RK-006, RK-007, RK-008, RK-009, RK-010, RK-011 | - |
| LX-007 | 40-5.5-r07-deleted-vs-modified | LM-010, EP-014, LV-009 | - |
| LX-008 | 40-5.5-r08-status-one-side | MR-022, MR-023 | "the other side's composite change is kept as data on the removed node": the composite key merges separately (LM-002, LM-003). |
| LX-009 | 40-5.5-r09-planned-vs-present | MR-024, SL-040 | - |
| LX-010 | 40-5.5-r10-present-vs-removed | MR-025, SL-041, LV-007 | - |
| LX-011 | 40-5.5-r11-path-claim | PC-001, PC-002, PC-003, PC-004, LV-005, LV-006 | - |
| LX-012 | 40-5.5-r12-anchor-existence | LM-018, LM-019, LM-020 | - |
| LX-013 | 40-5.5-r13-anchor-one-side-repinned | LM-019, LM-020 | - |
| LX-014 | 40-5.5-r14-anchor-both-repinned | LM-021, LV-004 | - |
| LX-015 | 40-5.5-r15-globs | LM-016, CP-009, CP-010, CP-011, CP-012 | - |
| LX-016 | 40-5.5-r16-path-moves | LM-017, CP-003 | - |
| LX-017 | 40-5.5-r17-at-to-deleted-node | EC-025, MR-048, VA-004 | Historical: a tombstone reference and a `suspect` source (RE-009), never `DanglingEdge` (VA-004 exempts historical edges). |
| LX-018 | 40-2.2-merge-class-column | FC-106, FC-107, FC-108, FC-109, FC-110, FC-111, FC-112, FC-113, FC-114, FC-115, FC-116, FC-117, FC-118, FC-119 | [40 §2.2]'s "Merge class" column. |
| LX-019 | 40-2.3-created-least | LR-002 | "`created` of a node created on both sides". |
| LX-020 | 40-5.6-history | LH-001, LH-002, LH-003, LH-004, LH-005, LH-007 | - |
| LX-021 | AR-5e.6-merge | LM-005, LM-006, LM-007, LM-008, LM-010, MR-025, PC-002, LM-015, LM-016, LM-017, LM-018 | [AR §5e.6] "Merge" summary. |

## Coverage

Rule tables specify semantics, not bytes; the layouts of the values named here are in [F06] (`SetEdgeProps`), [F07]
(the anchor selector block, R-10), [F08] (file and root nodes, R-2, R-3, R-5) and [F18] (R-12, R-17).

| Checklist row | Covered by |
|---|---|
| [40 §2.11] R-2: merge classes `observation` (composite) and `identity` (immutable) | LM-001 to LM-006, LM-023 to LM-026; FC rows in [RULES/merge-table] |
| [40 §2.11] R-3: the merge re-key rule with its edge-complete re-pointing and its record in a sync's residue; the `created` rule of a dual creation by (generation, commit id) | RK-001 to RK-011, LR-002 |
| [40 §2.11] R-4: anchors merge add-wins by anchor uid; `SetEdgeProps` repins | LM-018 to LM-022 |
| [40 §2.11] R-5: `path_moves` is an ordinary add-wins set, with no op, canonical item or trailer | LM-017, CP-002, CP-003, LH-003, LH-004 |
| [40 §2.11] R-10: anchor selectors compared through their digests | §2 (equality of anchor values) |
| [40 §2.11] R-12: I-F1 and I-F14 at merge | PC-001 to PC-005 (I-F1), LM-007, RK-010 (I-F14) |
| [40 §2.11] R-17: `relink` values `merge-compose/prefix` and `merge-observation/<evidence>` | CP-006, LV-001 |
| [60 §3.4] FL-7: the merge rules of the observation composite, path claims, the prefix history, anchor add-wins, the re-key rule | this file |

No X-F row and no [90 §10.1] item concerns link merge rules.

## Holes

None. The link merge rules use no resolver constant (R-14) and no measured value; the settle rows of §8 depend on
resolver constants only through the settle itself, which [F20] specifies.

## Open points for the review

1. **Composition details** (LC-002, CP-005, LM-005). (a) "One side's root node gained … an entry" is read as
   *exactly one* side: if both sides gained covering entries, the rule falls to `FieldEdit` (LM-006). (b) Only `path`
   is rewritten; `oid`, `bytes`, `observed_git` and `observed_blob` stay those of side S, although they were observed at
   the pre-composition path — the next settle verifies (CP-008). (c) Several gained entries apply in (hlc, from, to)
   order, each to the result of the previous one.
2. **Path-claim scope** (PC-003). [40 §2.10] I-F1 says "present or planned"; [40 §5.5] says "two live present nodes".
   Both are [40]; the invariant's wording is used, because a `planned` node and a `present` node at one path break
   I-F1 just as two `present` nodes do.
3. **Re-key iteration** (RK-003 to RK-005). Settled in review pass 1 (S1-48): the dead test is [F08 §11.2] step 4's —
   the uid names **any** node of the state, live at another path, `removed` or a tombstone — applied to the base, dst
   and src states, as [F12 §7.6] step 2 states. It reads the three states only, so the merge stays a pure function of
   the two histories (RK-009), as [40 §2.3]'s view-scoped rule (A1 re-review S-01, FB-2) intends. The loop keeps U and
   the running predecessor q apart: the keys that move are U's, and `origin_pred` is the last q, so that a re-derived
   uid′ verifies over its stored inputs (I-F2).
4. **Root nodes cannot be re-keyed** (LC-005, LM-013). A root-node uid is BLAKE3-128(lp("moirai-root-v1") ‖ lp(root
   name)) with no predecessor input, so "re-key, never resurrect" has no root-node form. It arises only if a root node
   is engine-deleted on one side and created on the other. **Decided** in review pass 1 (P1-21) by [F08 §11.3] (its
   open point 25): a root node is never engine-deleted — `rm` of one is refused with E409's root-node case
   ([LQ/errors §5.5]) — and an `area` has no `removed` status, so no store's history reaches LC-005. LM-013 is no longer
   a gap: reaching it is a failed internal check (LR-006, [F19 §10.2] `internal`, exit 1). The model's generators draw
   root nodes live only, as [F08 §11.3] guarantees; a hand-built triple that reaches the row fails the test that built
   it.
5. **`DeleteVsModify` on file nodes** (LM-010). "No automatic policy" is read as `none`: dst's state stands
   provisionally until `resolve` ([RULES/merge-table] Open point 5).
6. **Glob wildcards** (CP-009). The literal-prefix rule needs the set of wildcard characters (`*`, `?`, `[` are
   assumed). The glob syntax belongs to [F08] with `files_owned`, `applies_to` and `path_globs`.
7. **Anchor removed on one side, re-pinned on the other** (LM-022). Proposed `FieldEdit`, as for other edges
   ([RULES/merge-table] Open point 8); "add-wins" would keep the anchor and undo the `unlink` silently.
8. **Identity mismatch** (LM-026). One uid with different `origin_path`, `origin_pred` or `root` on two sides can only
   come from a node that fails its derivation (a foreign node treated as random, [40 §5.7]). Proposed: a structural
   `IdCollision`.
9. **Where `PathClaim` sits** (PC-002). The value is placed on each claiming node's observation composite key; the
   `.moi` conflict key for the composite (one line for six fields) is WP-15's.
10. **Re-key scope** (RK-006, RK-011; review pass 1 S1-14). The first draft re-pointed only S's `at` edges, as [40]
    revision 2 did. [40 §5.5] as revised by the A1 re-review (S-01, disposition FB-3) and [F12 §7.6] re-point every edge
    S added since the base whose destination is U, and the `replaced_by` references; [F12] open point 25 adds every
    other `ref`-typed value equal to U that S set, because leaving it on U is the same wrong answer. RK-006 now states
    that scope and RK-011 the sync residue; [40 §8.3.2] P13's non-anchor case is listed in RK-010. The rows changed
    after the first draft, so the owner re-signs this file (V3).
11. **The evidence of a settle by observation** (LV-001; WP-92). [40 §5.3] requires exact evidence "for the chosen path",
    and [F18 §5.1] admits only a `lazy`, `git` or `hook` token after `merge-observation/`. When one side's path is
    present in the writer tree and the other side's path reaches it by exact evidence (the common case: git applied one
    lane's rename), the model records the token of that exact evidence (`merge-observation/r100` for an E6 rename). It
    resolves nothing when both sides' paths are present (two files), when the sides reach different paths, or when the
    only exact evidence is an intent record (`explicit/…`, which the grammar does not admit after
    `merge-observation/`). Proposed: [40 §5.3] states this choice.
12. **Which node a `PathClaim` unification keeps** (LV-005; WP-92). [40 §2.3] "The residual case" says the settle
    "performs the unification itself" when git shows an exact rename from A's path to B's, and `links fix A --same-as
    B` makes A `removed{reason: same-as, replaced_by: B}`; the direction of the automatic unification is not stated.
    The model keeps the node registered at the rename's destination (B, which holds the claimed path by registration)
    and removes the node whose alias is the rename's source (A), the shape of `--same-as`. Proposed: [40 §2.3] states it.
