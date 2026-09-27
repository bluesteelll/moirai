# Delete-policy matrix: edge kind × delete policy × effect on dependents

| Field | Value |
|---|---|
| Status | draft, pass 1 pending |
| Work package | WP-90 (R-MODEL); consumed by WP-93b (LQ `DELETE`), WP-94 (the delete-policy and node-40 suites, GT10) and M2's write path |
| Sources | [AR §2.5] T5; [AR §3.3] edge table and its "On dst deleted" and "On src deleted" columns; [AR §3.4] I1, I2, I3, I4, I5′, I6, I7, I32′, I37′, I39′, I42′; [AR §3.5] `open_blockers`, `suspect`, `has_dangling`; [AR §5a.5] revert and `Undelete`; [AR §5a.7] existence and structural-edge rows; [AR §5a.9] `branch -D`; [AR §5b.2] rule 8 (tombstone files); [AR §5b.6] step 2 (import transitions); [AR §5d.3] "node 40"; [AR §7.1] `rm`, `resolve` and the `rm --dry-run` example; [AR §7.2] `write` row; [AR §7.3] role policy; [AR §13] policy data `edges.blocks.on-src-deleted`, `edges.gates.on-src-deleted`, `store.suspect-budget`; [40 §2.8] `at`; [40 §5.5] derived-uid existence; [50 §3.6] tombstones and flagged edges; [50 §3.10] `DELETE`, E409; [50 §6.5] role policy; [90 §4.3] |
| Format | [RULES/README] |
| Cited as | [RULES/delete-policy-matrix]; a row as [RULES/delete-policy-matrix EG-008] |

## 1. What this table decides

When a node is deleted (`rm`, `TX { DELETE x … }`, a foreign `Delete` from an image import), this file decides:

- whether the delete may run at all (`delete-preconditions`);
- for every edge kind, at each end, and for each delete option and policy value: what happens to the edge and to the
  node at its other end (`edge-policy`, with the vocabularies `delete-options`, `edge-conditions`, `edge-actions`,
  `edge-effects`);
- the order of the delete's steps inside its one commit (`delete-steps`);
- what a **flagged** edge is and how it keeps its dependent blocked until someone resolves it (`flagged-edges`);
- what a **tombstone** keeps (`tombstone`), and what `Undelete` restores (`undelete`);
- how a delete on one branch reaches the others (`cross-branch`, which points at [RULES/merge-table] and
  [RULES/state-definition]);
- the "node 40" table of [AR §5d.3] and the `rm` example of [AR §7.1] as worked rows (`n40-*` tables).

It does not decide the merge rules for existence keys ([RULES/merge-table] MR-041 to MR-046, EP rows), the
derived-uid re-key ([RULES/link-merge-rules] RK rows), or the exclusion of a node deleted on another branch
([RULES/state-definition]); it cites them.

## 2. How to read the tables

The format is [RULES/README]. In `edge-policy`:

- **`end`**: `dst-deleted` when the deleted node is the edge's destination (the edge is an in-edge of the deleted
  node), `src-deleted` when it is the source (an out-edge).
- **`option`**: the delete option that matters for this edge (`delete-options`); `none` means that option was not
  given; `*` means no option changes the row.
- **`policy`**: the value of the policy-data row `edges.<kind>.on-src-deleted` ([AR §13]) on the deleting branch, for the
  two edge kinds that have one; `-` otherwise; `*` any value.
- **`condition`**: a predicate on the edge's other endpoint, evaluated on the state before the delete
  (`edge-conditions`); `-` means always.
- **`action`** and **`effect`**: what happens to the edge, and to the node at its other end (`edge-actions`,
  `edge-effects`).

For one edge exactly one row matches (the model checks this at load for every combination of the vocabularies). The
deleted set is the target plus, under `--cascade`, all its descendants; an edge with both ends in the deleted set is
handled by DS-003.

## 3. Vocabularies

<!-- table: delete-options -->
| row | option | basis | source | definition |
|---|---|---|---|---|
| DO-001 | none | derived | [AR §3.3] | The option relevant to the row was not given. |
| DO-002 | replaced-by | design | [AR §3.3]; [AR §7.1] `rm --replaced-by ID`; [50 §3.10] `REPLACED BY y` | A live replacement Y; recorded as the tombstone's `replaced_by`. |
| DO-003 | cascade | design | [AR §3.3] `parent`; [50 §3.10] `POLICY CASCADE` | Deletes the whole subtree in the same commit. |
| DO-004 | reparent | design | [AR §3.3] `parent`; [50 §3.10] `POLICY REPARENT` | Moves the children up to the deleted node's parent. |
| DO-005 | reassign | design | [AR §3.3] `scoped_to` "`--reassign` to the parent area" | Moves `scoped_to` edges to the deleted area's parent area. |
| DO-006 | release | design | [AR §3.4] I32′; [50 §3.10] `RELEASE` | Releases every live lease on the deleted set, with a triage note on the tombstone. |
| DO-007 | * | derived | [RULES/README] §4 | Wildcard: no option changes the row. |

<!-- table: edge-conditions -->
| row | condition | basis | source | definition |
|---|---|---|---|---|
| CD-001 | - | derived | [RULES/README] §4 | Always true. |
| CD-002 | src-unfinished | design | [AR §3.5] `open_blockers`; [AR §2.5] X4 | The deleted source is a task with a status outside {`done`, `cancelled`}, or a question whose `answered` predicate is false: it was an open blocker. |
| CD-003 | src-finished | derived | [AR §3.5] | The negation of CD-002. |
| CD-004 | src-gating | design | [AR §3.3] `gates` | The deleted source verdict has status `open` and outcome `fail_fixable` or `fail_fundamental`: it was gating completion. |
| CD-005 | src-not-gating | derived | [AR §3.3] `gates` | The negation of CD-004. |
| CD-006 | last-answer | derived | [AR §3.3] `answers` "question reopens" | No other live `answers` in-edge of the question remains after the delete. |
| CD-007 | other-answer | derived | [AR §3.3] `answers` | Another live `answers` in-edge of the question remains. |
| CD-008 | has-parent-area | derived | [AR §3.3] `scoped_to` | The deleted area has a live parent area. |
| CD-009 | no-parent-area | derived | [AR §3.3] `scoped_to` | The deleted area has no parent. |

<!-- table: edge-actions -->
| row | action | basis | source | definition |
|---|---|---|---|---|
| EA-001 | refuse | design | [AR §3.3] "restrict"; [50 §3.10] E409 | The delete is refused: E409, exit 6, with the impact list; nothing is written. |
| EA-002 | drop | design | [AR §3.3] "drop" | The edge is removed in the delete's commit. |
| EA-003 | drop-notify | design | [AR §3.3] "drop + notify"; [AR §13] `drop-notify` | As `drop`, and the other endpoint is named in the commit's `affected` list and change-feed entry with the reason. |
| EA-004 | repoint | design | [AR §3.3] "re-point"; [AR §7.1] | The edge is replaced by one whose deleted end is the replacement (or, for `reassign`, the parent area); properties other than `flagged` are kept. |
| EA-005 | flag | design | [AR §3.3] "flag"; [AR §2.5] X4 | The edge is kept, with the deleted node as its source and the property `flagged = true`; it is retained as the tombstone's out-edge (FL rows). |
| EA-006 | tombstone-ref | design | [AR §3.3] "tombstone ref"; [AR §3.4] I3 | The edge is kept unchanged and now points at a dead id; readers render the tombstone. |
| EA-007 | retain | design | [AR §3.4] I39′; [AR §5b.2] rule 8 | The edge is kept as a retained out-edge of the tombstone. |
| EA-008 | delete-subtree | design | [AR §3.3] "`--cascade` deletes the subtree" | Every descendant joins the deleted set; its own edges follow this table. |
| EA-009 | move-up | design | [AR §3.3] "`--reparent` moves children up" | The child's parent becomes the deleted node's parent, or none if the deleted node was a root; order is kept. |

<!-- table: edge-effects -->
| row | effect | basis | source | definition |
|---|---|---|---|---|
| EF-001 | none | derived | [AR §3.3] | No derived state of the other endpoint changes. |
| EF-002 | subtree-deleted | design | [AR §3.3] | The descendants are deleted with the target. |
| EF-003 | children-moved | design | [AR §3.3]; [AR §3.4] I5′ | The children and their subtrees move up; I4 holds (depth shrinks) and the implied exogenous edges of the moved subtrees are re-derived. |
| EF-004 | rollups-updated | design | [AR §3.3] "rollups updated" | The parent's `children_total`, `children_done`, `ready_to_close` and `container` are recomputed. |
| EF-005 | blocker-recomputed | design | [AR §3.3] "A's `is_blocker` recomputed" | The surviving source's `is_blocker` is recomputed. |
| EF-006 | blocker-moved | proposed | [AR §7.1]; [OP-2] | The surviving source now blocks the replacement: the replacement's `open_blockers` counts it if it is unfinished. |
| EF-007 | dependent-reblocked | design | [AR §3.3] "the edge becomes `replacement → B`" | B's `open_blockers` now reflects the replacement's state instead of the deleted node's. |
| EF-008 | dependent-held | design | [AR §3.3] `blocks` "B stays out of `ready` until `resolve`"; [AR §2.5] X4 | B gets `has_dangling`; the flagged edge counts in B's `open_blockers` and `open_blockers_exo`, so B and its descendants are not `unblocked` until it is resolved (FL-001). |
| EF-009 | dependent-released | design | [AR §13] `drop-notify` | B loses one open blocker and may become `unblocked`; it is named in `affected`. |
| EF-010 | dependent-notified | design | [AR §3.3] `merge_after` "drop + notify" | The other endpoint is named in `affected` and the change feed; no predicate of it changes. |
| EF-011 | gate-moved | proposed | [AR §2.5]; [OP-2] | The gate now runs from the replacement verdict, or to the replacement task. |
| EF-012 | completion-held | design | [AR §3.3] `gates` "a deleted failing verdict never silently ungates" | T's `complete` is refused while the flagged edge exists ([RULES/status-machines GD-002]); T's readiness is unchanged (X5). |
| EF-013 | completion-released | design | [AR §13] `drop-notify` | T may complete; it is named in `affected`. |
| EF-014 | question-reopened | design | [AR §3.3] `answers` "question reopens" | The question's `answered` predicate becomes false; its status `answered` moves to `open` by the door `delete-policy` ([RULES/status-machines TR-039]); tasks it blocks gain an open blocker. |
| EF-015 | scope-moved | design | [AR §3.3] `scoped_to` | The knowledge node is scoped to the parent area. |
| EF-016 | canonical-moved | proposed | [AR §3.3] `duplicate_of`; [OP-4] | The duplicate now points at the replacement, which must be canonical (I7). |
| EF-017 | src-affected | proposed | [AR §3.3] `depends_on` "drop + src `suspect`"; [OP-5] | The dependent section is named in `affected` with the reason; since the edge is gone, no lasting `suspect` can be derived. |
| EF-018 | src-suspect | design | [AR §3.5] `suspect`; [AR §3.3] | The surviving source is `suspect` (derived from the dead target). |
| EF-019 | rendered | design | [AR §3.3] `mentions` | The source keeps its text; readers render the target as a tombstone, for example `#40 (deleted c812 by dev#2: "dup of #52" -> #52)`. |
| EF-020 | anchors-kept | design | [AR §3.3] `at`; [AR §3.4] I39′ | The `at` edge and its anchors stay with the tombstone's retained out-edges. |

## 4. Preconditions of a delete

Checked in this order before anything is written; the first failure refuses the whole delete.

<!-- table: delete-preconditions -->
| row | check | refusal | exit | basis | source | note |
|---|---|---|---|---|---|---|
| DP-001 | role | E406 | 6 | design | [AR §7.3]; [50 §6.5]; [AR §7.2] `write` row; [90 §4.3] | Node `DELETE` is orchestrator or owner only, through the CLI; MCP `write` refuses a node `DELETE` for every role. Edge deletes follow [RULES/status-machines]'s roles and [50 §6.5]. |
| DP-002 | view | E305 | 6 | derived | [50 §3.9] item 6; [AR §5a.1] | A `work` or `plan` tip. The `plan/*` mask covers status, resolution, assignee and claims ([AR §3.4] I33′), not existence. |
| DP-003 | target-live | - | 3 | design | [AR §7.1] exit 3 | The target must be live in the view; the tombstone is printed otherwise. |
| DP-004 | options | - | 2 | derived | [AR §3.3]; [50 §3.10] | `--cascade` together with `--reparent` is a usage error. |
| DP-005 | lease | F19 | 6 | design | [AR §3.4] I32′; [AR §5d.3] row 3; [50 §3.10] | A live lease ([RULES/state-definition] LL rows) on any node of the deleted set, on any branch, refuses the delete unless `--release`; the message names the holder, the branch and the lease (`leased by dev#2 on lane/y (L-19)`). The error code is [F19]'s ([OP-9]). |
| DP-006 | restrict | E409 | 6 | design | [50 §3.10]; [AR §3.3] | Any edge whose `edge-policy` action is `refuse`; the impact list is printed. |
| DP-007 | replacement | E409 | 6 | proposed | [AR §3.3]; [AR §7.1]; [OP-3] | `--replaced-by Y`: Y is live, outside the deleted set, and valid at the re-pointed end of every edge that `repoint`s to it (endpoint kinds of [50 §2.5]). |
| DP-008 | acyclic | E405 | 6 | design | [AR §3.4] I5′, I37′; [50 §3.10] item 5 | Every re-pointed `blocks` or `gates` edge and every `--reparent` move is checked at the end of the block, in I37′ order. |
| DP-009 | cardinality | E405 | 6 | design | [AR §3.4] I6, I7 | A re-pointed `duplicate_of` must end at a canonical node. |

## 5. The matrix

<!-- table: edge-policy -->
| row | edge | end | option | policy | condition | action | effect | basis | source | note |
|---|---|---|---|---|---|---|---|---|---|---|
| EG-001 | `parent` | dst-deleted | none | - | - | refuse | none | design | [AR §3.3] `parent` "restrict" | A live child refuses the delete. |
| EG-002 | `parent` | dst-deleted | cascade | - | - | delete-subtree | subtree-deleted | design | [AR §3.3] `parent` | - |
| EG-003 | `parent` | dst-deleted | reparent | - | - | move-up | children-moved | design | [AR §3.3] `parent` | - |
| EG-004 | `parent` | src-deleted | * | - | - | drop | rollups-updated | design | [AR §3.3] `parent` "rollups updated" | - |
| EG-005 | `blocks` | dst-deleted | none | - | - | drop | blocker-recomputed | design | [AR §3.3] `blocks` "drop (the dependent B is gone …)" | - |
| EG-006 | `blocks` | dst-deleted | replaced-by | - | - | repoint | blocker-moved | proposed | [AR §7.1] `rm 40 … --replaced-by 52` example "`#203 --blocks-> #40` re-point to #52"; [OP-2] | The edge becomes `A → Y`. |
| EG-007 | `blocks` | src-deleted | replaced-by | * | - | repoint | dependent-reblocked | design | [AR §3.3] `blocks` "re-point with `--replaced-by`"; [AR §2.5] | The edge becomes `Y → B`. |
| EG-008 | `blocks` | src-deleted | none | flag | src-unfinished | flag | dependent-held | design | [AR §3.3] `blocks`; [AR §2.5] X4 "a deleted blocker never silently unblocks" | The default policy. |
| EG-009 | `blocks` | src-deleted | none | flag | src-finished | drop | none | proposed | [AR §2.5] X4; [OP-1] | A finished blocker's deletion unblocks nothing, so flagging would block B anew. |
| EG-010 | `blocks` | src-deleted | none | drop-notify | - | drop-notify | dependent-released | design | [AR §13] `edges.blocks.on-src-deleted`; [AR §2.5] revisit trigger | - |
| EG-011 | `gates` | dst-deleted | none | - | - | drop | none | design | [AR §3.3] `gates` "drop (the gated task is gone)" | - |
| EG-012 | `gates` | dst-deleted | replaced-by | - | - | repoint | gate-moved | proposed | [AR §2.5]; [OP-2] | By analogy with EG-006. |
| EG-013 | `gates` | src-deleted | replaced-by | * | - | repoint | gate-moved | design | [AR §2.5] "`blocks`/`gates` out of the deleted node are re-pointed with `--replaced-by`" | Y must be a verdict (DP-007). |
| EG-014 | `gates` | src-deleted | none | flag | src-gating | flag | completion-held | design | [AR §3.3] `gates` "flag as above (a deleted failing verdict never silently ungates)" | - |
| EG-015 | `gates` | src-deleted | none | flag | src-not-gating | drop | none | proposed | [AR §3.3] `gates`; [OP-1] | A verdict that gated nothing leaves nothing to protect. |
| EG-016 | `gates` | src-deleted | none | drop-notify | - | drop-notify | completion-released | design | [AR §13] `edges.gates.on-src-deleted` | - |
| EG-017 | `merge_after` | dst-deleted | * | - | - | drop-notify | dependent-notified | design | [AR §3.3] `merge_after` "drop + notify" | - |
| EG-018 | `merge_after` | src-deleted | * | - | - | drop | none | design | [AR §3.3] `merge_after` | - |
| EG-019 | `runs_in` | dst-deleted | * | - | - | refuse | none | design | [AR §3.3] `runs_in` "restrict" | - |
| EG-020 | `runs_in` | src-deleted | * | - | - | drop | none | design | [AR §3.3] `runs_in` | - |
| EG-021 | `answers` | dst-deleted | * | - | - | refuse | none | design | [AR §3.3] `answers` "restrict" | - |
| EG-022 | `answers` | src-deleted | * | - | last-answer | drop | question-reopened | design | [AR §3.3] `answers` "question reopens" | - |
| EG-023 | `answers` | src-deleted | * | - | other-answer | drop | none | derived | [AR §3.3] `answers` | Another answer remains (possible only after a merge; "≤ 1 active"). |
| EG-024 | `scoped_to` | dst-deleted | none | - | - | refuse | none | design | [AR §3.3] `scoped_to` "restrict" | - |
| EG-025 | `scoped_to` | dst-deleted | reassign | - | has-parent-area | repoint | scope-moved | design | [AR §3.3] `scoped_to` "`--reassign` to the parent area" | - |
| EG-026 | `scoped_to` | dst-deleted | reassign | - | no-parent-area | refuse | none | proposed | [AR §3.3]; [OP-6] | There is no parent area to reassign to. |
| EG-027 | `scoped_to` | src-deleted | * | - | - | drop | none | design | [AR §3.3] `scoped_to` | - |
| EG-028 | `duplicate_of` | dst-deleted | none | - | - | refuse | none | design | [AR §3.3] `duplicate_of` "restrict" | - |
| EG-029 | `duplicate_of` | dst-deleted | replaced-by | - | - | repoint | canonical-moved | proposed | [AR §3.3] `duplicate_of` "re-point to canonical"; [OP-4] | - |
| EG-030 | `duplicate_of` | src-deleted | * | - | - | drop | none | design | [AR §3.3] `duplicate_of` | - |
| EG-031 | `depends_on` | dst-deleted | * | - | - | drop | src-affected | proposed | [AR §3.3] `depends_on` "drop + src `suspect`"; [AR §3.4] I2; [OP-5] | - |
| EG-032 | `depends_on` | src-deleted | * | - | - | drop | none | design | [AR §3.3] `depends_on` | - |
| EG-033 | `supersedes` | dst-deleted | * | - | - | tombstone-ref | none | design | [AR §3.3] `supersedes` | - |
| EG-034 | `supersedes` | src-deleted | * | - | - | retain | none | design | [AR §3.3] `supersedes` "old stays superseded" | - |
| EG-035 | `derived_from` | dst-deleted | * | - | - | tombstone-ref | src-suspect | design | [AR §3.3] `derived_from` | - |
| EG-036 | `derived_from` | src-deleted | * | - | - | retain | none | design | [AR §3.4] I39′ | - |
| EG-037 | `cites` | dst-deleted | * | - | - | tombstone-ref | src-suspect | design | [AR §3.3] `cites` | - |
| EG-038 | `cites` | src-deleted | * | - | - | retain | none | design | [AR §3.4] I39′ | - |
| EG-039 | `implements` | dst-deleted | * | - | - | tombstone-ref | src-suspect | derived | [AR §3.5] `suspect`; [AR §3.3] `implements`; [OP-7] | - |
| EG-040 | `implements` | src-deleted | * | - | - | retain | none | design | [AR §3.4] I39′ | - |
| EG-041 | `refutes` | dst-deleted | * | - | - | tombstone-ref | none | design | [AR §3.3] | - |
| EG-042 | `refutes` | src-deleted | * | - | - | retain | none | design | [AR §3.4] I39′ | - |
| EG-043 | `confirms` | dst-deleted | * | - | - | tombstone-ref | none | design | [AR §3.3] | - |
| EG-044 | `confirms` | src-deleted | * | - | - | retain | none | design | [AR §3.4] I39′ | - |
| EG-045 | `verifies` | dst-deleted | * | - | - | tombstone-ref | none | design | [AR §3.3] | - |
| EG-046 | `verifies` | src-deleted | * | - | - | retain | none | design | [AR §3.4] I39′ | - |
| EG-047 | `addresses` | dst-deleted | * | - | - | tombstone-ref | none | design | [AR §3.3] | - |
| EG-048 | `addresses` | src-deleted | * | - | - | retain | none | design | [AR §3.4] I39′ | - |
| EG-049 | `about` | dst-deleted | * | - | - | tombstone-ref | none | design | [AR §3.3] | - |
| EG-050 | `about` | src-deleted | * | - | - | retain | none | design | [AR §3.4] I39′ | - |
| EG-051 | `discovered_from` | dst-deleted | * | - | - | tombstone-ref | none | design | [AR §3.3] | - |
| EG-052 | `discovered_from` | src-deleted | * | - | - | retain | none | design | [AR §3.4] I39′ | - |
| EG-053 | `produced` | dst-deleted | * | - | - | tombstone-ref | none | design | [AR §3.3] | - |
| EG-054 | `produced` | src-deleted | * | - | - | retain | none | design | [AR §3.4] I39′ | - |
| EG-055 | `consumed` | dst-deleted | * | - | - | tombstone-ref | none | design | [AR §3.3] | - |
| EG-056 | `consumed` | src-deleted | * | - | - | retain | none | design | [AR §3.4] I39′ | - |
| EG-057 | `contradicts` | dst-deleted | * | - | - | tombstone-ref | none | design | [AR §3.3] | - |
| EG-058 | `contradicts` | src-deleted | * | - | - | retain | none | design | [AR §3.4] I39′ | - |
| EG-059 | `mentions` | dst-deleted | * | - | - | tombstone-ref | rendered | design | [AR §3.3] `mentions` | - |
| EG-060 | `mentions` | src-deleted | * | - | - | retain | none | design | [AR §3.4] I39′ | [AR §3.3]'s "recomputed from text" applies when a live source's text changes. |
| EG-061 | `relates` | dst-deleted | * | - | - | tombstone-ref | none | design | [AR §3.3] | - |
| EG-062 | `relates` | src-deleted | * | - | - | retain | none | design | [AR §3.4] I39′ | - |
| EG-063 | `at` | dst-deleted | * | - | - | tombstone-ref | src-suspect | design | [AR §3.3] `at`; [40 §2.8] | Engine-deleted file node (`moirai rm 812`, rare). A file node that is `removed` (a status, not a delete) makes its referrers `suspect` too ([RULES/status-machines DE-025]). |
| EG-064 | `at` | src-deleted | * | - | - | retain | anchors-kept | design | [AR §3.3] `at`; [AR §3.4] I39′ | - |

## 6. The steps of one delete

<!-- table: delete-steps -->
| row | step | action | basis | source | note |
|---|---|---|---|---|---|
| DS-001 | 1 | preconditions | design | [AR §4.5] step 4; [50 §3.10] item 5 | The DP rows, in order, on the candidate. |
| DS-002 | 2 | deleted-set | design | [AR §3.3] `parent`; [AR §5d.3] | The target, plus every descendant under `--cascade`. |
| DS-003 | 3 | internal-edges | proposed | [AR §3.4] I39′; [OP-8] | An edge with both ends in the deleted set: a historical edge is retained as its source's tombstone out-edge; a structural edge is dropped (no live dependent remains to protect). |
| DS-004 | 4 | edge-policy | design | [AR §5d.3] "the writer walks #40's reverse list (O(degree))"; [AR §3.3] | Every other edge of every node in the deleted set takes its `edge-policy` row. |
| DS-005 | 5 | delete-ops | design | [AR §4.3] `Delete{id, reason, replaced_by, before-image}` | One `Delete` per deleted node, with its before-image, the reason and `replaced_by`. |
| DS-006 | 6 | policy-ops | design | [AR §2.5]; [AR §5d.3] | The ops the actions imply, in the same commit: `RemoveEdge` and `AddEdge` for `drop` and `repoint`, `Move` for `move-up`, the `flagged` property for `flag` (its op form is [F06]'s), `SetStatus` for `question-reopened`. |
| DS-007 | 7 | release | design | [AR §3.4] I32′; [AR §5d.3] row 3 | Under `--release`, every live lease on the deleted set is released, with a triage note attached to the tombstone. |
| DS-008 | 8 | hold | design | [AR §4.5] step 4; [RULES/state-definition OR-003] | The commit is the origin of the `deleted` hold of every deleted node on its branch; the marker cache writes `deleted` records ([RULES/state-definition ME-001]). |
| DS-009 | 9 | affected | design | [AR §5d.3]; [AR §3.4] I42′ | `affected` lists every other endpoint whose derived state changed (EF rows), every source made `suspect`, and every notified endpoint; the tombstone, the markers and the change-feed entry are in the same flushed group. |
| DS-010 | 10 | suspect-budget | proposed | [AR §2.5]; [AR §13] `store.suspect-budget`; [OP-10] | If the transitive `suspect` closure exceeds `store.suspect-budget`, the commit carries `affected_complete = 0` (I42′) and readers recompute `suspect`. |
| DS-011 | * | dry-run | design | [AR §7.1] `rm … --dry-run` | Steps 1 to 4 run and print the impact (structural edges with their actions, historical edges with their effects, leases, markers); nothing is written. |

## 7. Flagged edges

<!-- table: flagged-edges -->
| row | rule | basis | source | note |
|---|---|---|---|---|
| FL-001 | blocks-count | design | [AR §3.5] "plus flagged dangling blocker edges"; [50 §3.6] | A flagged `blocks` in-edge counts 1 in its target's `open_blockers` and `open_blockers_exo` (the dead source lies outside every subtree), so the target and its descendants are neither `unblocked` nor `ready` ([RULES/state-definition BT-004]). |
| FL-002 | gates-complete | proposed | [AR §3.3] `gates`; [AR §3.3] X5; [OP-1] | A flagged `gates` in-edge refuses its target's `complete` ([RULES/status-machines GD-002]) and does not affect readiness. |
| FL-003 | has-dangling | design | [AR §3.5]; [AR §3.4] I39′ | `has_dangling(n)` holds when n has at least one flagged in-edge. |
| FL-004 | only-exception | design | [AR §3.4] I2; [AR §5d.3] "Engine-level guarantee" | At every branch head a flagged `blocks` or `gates` edge is the only structural edge with a dead endpoint. |
| FL-005 | resolve-repoint | design | [AR §7.1] `resolve KEY --take repoint:ID`; [AR §5d.3] | `resolve 'edge:#A:blocks:#B' --take repoint:Y` replaces the flagged edge by `Y → B` without the flag; I5′ is checked. |
| FL-006 | resolve-drop | proposed | [AR §7.1] `resolve`; [AR §13] `drop-notify`; [OP-11] | Resolving the flagged edge without a replacement removes it, and B loses one open blocker. `unlink` or `DELETE e` of the flagged edge has the same effect. |
| FL-007 | who | design | [50 §6.5] "`RESOLVE` … orchestrator/owner only"; [AR §7.3] | Resolving or removing a flagged edge is the orchestrator's or the owner's. |
| FL-008 | image | design | [AR §5b.2] rule 8; [AR §3.4] I39′ | The tombstone file keeps `edge blocks -> <uid> flagged` or `edge gates -> <uid> flagged`; the importer re-creates the edge and sets `has_dangling` on its target. |
| FL-009 | undelete | derived | [AR §5a.5]; [AR §5d.3] `revert` row | `Undelete` of the dead source clears the flag: the edge is an ordinary edge again. |
| FL-010 | merge | design | [RULES/merge-table EC-002]; [RULES/merge-table VA-004] | A flagged edge merges as an edge key whose properties include `flagged`; VA-004 exempts it from `DanglingEdge`. |
| FL-011 | render | design | [50 §3.6]; [AR §7.1] | `blockers()` lists a flagged edge with `flagged = true`; `show` and `blockers` render `#40 (deleted c4468 -> flagged; moirai resolve 'edge:#40:blocks:#12')`. |

## 8. Tombstones

A tombstone is what stays of a deleted node in the current state. History keeps the full before-image in the `Delete`
op; the tombstone keeps only the rows marked `yes`.

<!-- table: tombstone -->
| row | item | kept | basis | source | note |
|---|---|---|---|---|---|
| TB-001 | row | yes | design | [AR §3.1] "a deleted node keeps its row with `deleted` set"; [AR §3.4] I1 | The `#N` row stays with the `deleted` flag; `#N` is never reused. |
| TB-002 | uid | yes | design | [AR §5b.2] rule 8 | - |
| TB-003 | kind | yes | design | [AR §5b.2] rule 8; [50 §3.6] | The kind at deletion. |
| TB-004 | title | yes | design | [AR §5b.2] rule 8; [50 §3.6] | - |
| TB-005 | deleted-commit | yes | design | [AR §4.4] `TOMB {id, tx, reason_sym, replaced_by}`; [AR §5b.2] rule 8 `deleted: c<commit> <time>` | - |
| TB-006 | deleted-by | yes | design | [50 §3.6] `deleted_by`, `deleted_at` | Read from the deleting commit. |
| TB-007 | reason | yes | design | [AR §5b.2] rule 8; [AR §4.4] `TOMB` | - |
| TB-008 | replaced-by | yes | design | [AR §5b.2] rule 8; [AR §4.4] `TOMB` | - |
| TB-009 | flagged-out-edges | yes | design | [AR §3.4] I39′; [AR §5b.2] rule 8 | - |
| TB-010 | historical-out-edges | yes | design | [AR §3.4] I39′; [AR §5b.2] rule 8 | `at` edges with their anchors included. |
| TB-011 | other-structural-out-edges | no | design | [AR §3.3]; [AR §3.4] I2 | Dropped or re-pointed by the matrix. |
| TB-012 | fields | no | design | [AR §5b.2] rule 8 "Nothing else — no other fields, no body" | The status and every kind field included. |
| TB-013 | body | no | design | [AR §5b.2] rule 8 | - |
| TB-014 | lifetime | yes | design | [AR §5b.1] "Tombstones are files and stay forever"; [AR §5b.6] `TombstoneRemoved` | A tombstone is never removed by the store; an image that loses one reports `TombstoneRemoved` on import. |
| TB-015 | rendering | yes | design | [AR §3.3] `mentions`; [50 §3.6] N01; [90 §8.1] L5 | Every reference renders `#40 (deleted c812 by dev#2: "dup of #52" -> #52)`, in ASCII. |

## 9. `Undelete`

`Undelete` is the inverse of `Delete`: the op written by a `revert` of the deleting commit ([RULES/merge-table DM-004]),
by a cherry-pick of such a revert, or by an import that sees a tombstone file become a live node ([AR §5b.6] step 2).

<!-- table: undelete -->
| row | item | effect | basis | source | note |
|---|---|---|---|---|---|
| UD-001 | node | restored | design | [AR §4.3] `Undelete{id, before-image}`; [AR §5d.3] `revert` row | Fields, status, body and parent come back from the before-image; the `deleted` flag is cleared. |
| UD-002 | flagged-edges | unflagged | derived | [AR §5a.5]; [RULES/delete-policy-matrix FL-009] | - |
| UD-003 | repointed-edges | restored-if-live | design | [AR §5d.3] "re-pointed edges restored if their targets still exist, else `NotFound` staged" | The re-pointed edge is removed and the original restored; if an endpoint is no longer live, the revert stages with `NotFound` ([RULES/merge-table DM-012]). |
| UD-004 | dropped-edges | restored-if-live | derived | [AR §5a.5] "before-images make inversion exact"; [RULES/merge-table DM-012] | As UD-003. |
| UD-005 | policy-ops | inverted | derived | [AR §5a.5] | A `question-reopened` status change is inverted with the rest. |
| UD-006 | hold | ends | design | [AR §4.5] step 4; [RULES/state-definition OR-003] | The `deleted` hold ends on the branch. If the restored status is `done` or `cancelled`, the `Undelete` commit is the origin of a new hold ([RULES/state-definition HV-001]). |
| UD-007 | derived-uid | allowed | design | [40 §2.10] I-F14; [AR §5e.8] | `Undelete` is one of the two explicit doors that may bring a dead derived uid back. |
| UD-008 | image | tombstone-to-live | design | [AR §5b.6] step 2; [AR §5d.3] `revert` row | The image sees a tombstone file become a live node. |

## 10. Across branches

A delete never changes another branch ([AR §5d.3]: "a delete on one branch never *mutates* another branch"). Each row
names the rule rows that realise the case.

<!-- table: cross-branch -->
| row | case | realized_by | basis | source | note |
|---|---|---|---|---|---|
| XB-001 | other-branch-before-merge | PD-012, HV-003, OR-003 | design | [AR §5d.3] row 1; [AR §3.4] I26′ | The node stays live on the other branches, but the deleted hold keeps it out of `ready`, `claim` and `blocking` there until they absorb the deleting commit ([RULES/state-definition]). |
| XB-002 | merge-read-only | MR-043, MR-044 | design | [AR §5d.3] row 1 "`main` only read #40 → delete wins" | dst takes the delete; the merged ops carry the policy ops for the referrers the deleting side saw. |
| XB-003 | merge-modified | MR-042, EP-001, RS-008 | design | [AR §5d.3] row 1 "`main` modified #40 → `DeleteVsModify` conflict value" | The provisional state follows the kind's existence policy (tasks: delete-wins). |
| XB-004 | merge-new-structural-edge | VA-004 | design | [AR §5d.3] row 1 "`main` added a structural edge to #40 → `DanglingEdge`" | Staged; the suggested resolution is the edge's `edge-policy` action or `repoint:<replaced_by>`. |
| XB-005 | merge-new-historical-edge | MR-048, MR-049, EG-035 | design | [AR §5d.3] row 1 "historical edges become tombstone refs, their sources `suspect`" | The new edge lands as a tombstone reference; its source is `suspect` where its `edge-policy` row says so. |
| XB-006 | both-deleted | MR-041, MR-046 | design | [RULES/merge-table] | - |
| XB-007 | derived-uid-recreated | LM-007, RK-003, RK-010 | design | [40 §5.5]; [40 §2.10] I-F14 | A file node created on one side while the other removed or deleted that uid is re-keyed, never resurrected. |
| XB-008 | branch-deleted-unmerged | ME-005, SN-008 | design | [AR §5a.9]; [AR §5d.3] row 4 | `branch -D` keeps the hold active while a live ref still holds the deletion, and clears it otherwise, with a triage line. |
| XB-009 | image-file-removed | VA-004 | design | [AR §5d.3] row 5; [AR §5b.6] step 2 | Import writes a foreign `Delete{image:file-removed}` on the imported ref; a structural edge left dangling stages the import on `import/<ref>`. |
| XB-010 | leased-elsewhere | DP-005 | design | [AR §5d.3] row 3; [AR §3.4] I32′ | A lease on another branch refuses the delete unless `--release`. |

## 11. Node 40: worked rows

The design's "node 40" table ([AR §5d.3]) and the `rm` example of [AR §7.1], on one synthetic store. Every expected
value follows from the rows above; WP-94 runs these as fixtures on the model, and WP-22 writes the GT10 files from them
([m0/PLAN §3.2] items 1 and 9).

**Starting state `c0`.** On `main` at commit `c0`, with `lane/x` and `lane/y` forked from `main` at `c0`. The policy
rows have their defaults (`flag`) unless a case says otherwise. No lease exists unless a case says so.

<!-- table: n40-nodes -->
| row | node | kind | status | parent | note |
|---|---|---|---|---|---|
| NN-001 | #9 | task | open | - | Parent of #40, so a container. |
| NN-002 | #12 | task | open | - | The dependent: #40 blocks #12. |
| NN-003 | #40 | task | open | #9 | The node deleted ("Reader registry"). |
| NN-004 | #41 | task | open | #40 | A child of #40, so #40 is a container. |
| NN-005 | #52 | task | open | - | The replacement. |
| NN-006 | #203 | task | open | - | Blocks #40. |
| NN-007 | #17 | note | active | - | Cites #40, pinned at `c0`. |
| NN-008 | #77 | note | active | - | Mentions #40 in its body. |

<!-- table: n40-edges -->
| row | src | kind | dst | props | note |
|---|---|---|---|---|---|
| NG-001 | #40 | blocks | #12 | - | - |
| NG-002 | #203 | blocks | #40 | - | - |
| NG-003 | #41 | parent | #40 | - | - |
| NG-004 | #40 | parent | #9 | - | - |
| NG-005 | #17 | cites | #40 | pinned_commit=c0 | - |
| NG-006 | #77 | mentions | #40 | - | - |

**Cases.** `ref` is where the action runs; `after` is the state it starts from (`c0`, or the state a named case left;
`c0/drop-notify` is `c0` with `edges.blocks.on-src-deleted = drop-notify` on `main`; `c0/lease-L-19` is `c0` with a live
lease `L-19` on #40 held by `dev#2` on `lane/y`). Actions are written `verb:arg:arg`: `rm:<id>` with options
`replaced-by=<id>`, `reparent`, `cascade`, `release`; `resolve:<key>:drop` or `resolve:<key>:repoint=<id>`;
`merge:<src>` into the case's ref; `set:<id>:<field>=<value>`; `add:<id>:blocks=<id>` (a new task that blocks);
`branch:<name>:from=<ref>`; `branch-D:<name>`; `revert:<case>` (the commit that case wrote); `import:file-removed=<id>`
(an image import whose tree lost that node's file).

<!-- table: n40-cases -->
| row | case | ref | after | action | basis | source | note |
|---|---|---|---|---|---|---|---|
| NC-001 | C0 | main | c0 | none | derived | [AR §3.5] | The starting state, checked before any delete. |
| NC-002 | C1 | main | c0 | rm:40:replaced-by=52 | design | [AR §3.3] `parent`; [50 §3.10] E409 | Refused: #41 is a live child. |
| NC-003 | C2 | main | c0 | rm:40:replaced-by=52:reparent | design | [AR §7.1] `rm 40 --reason "dup of #52" --replaced-by 52`; [AR §5d.3] | Reason "dup of #52". |
| NC-004 | C3 | main | c0 | rm:40:reparent | design | [AR §3.3] `blocks` "else flag"; [AR §2.5] X4 | Reason "obsolete"; no replacement. |
| NC-005 | C3r | main | C3 | resolve:edge:#40:blocks:#12:drop | proposed | [AR §7.1] `resolve`; [OP-11] | - |
| NC-006 | C3p | main | C3 | resolve:edge:#40:blocks:#12:repoint=52 | design | [AR §7.1] `--take repoint:ID` | - |
| NC-007 | C3n | main | c0/drop-notify | rm:40:reparent | design | [AR §13] `edges.blocks.on-src-deleted` | - |
| NC-008 | C4 | main | c0 | rm:40:replaced-by=52:cascade | design | [AR §3.3] `parent` "`--cascade` deletes the subtree" | - |
| NC-009 | C5 | lane/x | c0 | rm:40:replaced-by=52:reparent | design | [AR §5d.3] row 1 | `main` and `lane/y` still reference #40. |
| NC-010 | C6 | main | C5 | merge:lane/x | design | [AR §5d.3] row 1 "`main` only read #40 → delete wins" | `main` has not moved, so no sync is needed. |
| NC-011 | C7a | main | C5 | set:40:priority=0 | design | [AR §5d.3] row 1 | `main` modifies #40. |
| NC-012 | C7b | main | C7a | merge:lane/x | design | [AR §5d.3] row 1 "`main` modified #40 → `DeleteVsModify`"; [AR §5a.7] step 0 | The sync of step 0 (`main` into `lane/x`) lands the conflict on `lane/x`; the merge into `main` is refused ([RULES/merge-table PR-003]). |
| NC-013 | C8a | main | C5 | add:205:blocks=40 | design | [AR §5d.3] row 1; [AR §7.1] merge example | `main` adds a structural edge to #40. |
| NC-014 | C8b | main | C8a | merge:lane/x | design | [AR §5d.3] row 1 "`DanglingEdge` violation, raised by the sync of step 0 … so the whole merge stages"; [AR §7.1] | - |
| NC-015 | C9 | lane/x | c0/lease-L-19 | rm:40:replaced-by=52:reparent | design | [AR §5d.3] row 3 | Refused under the lease. |
| NC-016 | C9r | lane/x | c0/lease-L-19 | rm:40:replaced-by=52:reparent:release | design | [AR §5d.3] row 3 "`--release` releases the lease with a triage note … and proceeds" | - |
| NC-017 | C10 | lane/x | C5 | branch-D:lane/x | design | [AR §5d.3] row 4; [AR §5a.9] | No live ref holds the deletion afterwards. |
| NC-018 | C10f-a | lane/z | C5 | branch:lane/z:from=lane/x | design | [AR §5d.3] row 4 "a live branch forked from it after the deleting commit still carries it" | - |
| NC-019 | C10f-b | lane/x | C10f-a | branch-D:lane/x | design | [AR §5d.3] row 4; [72 M4] scenario 3 | `lane/z` still holds the deletion. |
| NC-020 | C11 | main | C2 | revert:C2 | design | [AR §5d.3] row 6 | - |
| NC-021 | C12 | main | c0 | import:file-removed=40 | design | [AR §5d.3] row 5; [AR §5b.6] step 2 | - |

<!-- table: n40-properties -->
| row | property | definition |
|---|---|---|
| NP-001 | exists | The node, or the edge written `edge:#A:kind:#B`, is live in the ref's state (`yes` or `no`). |
| NP-002 | deleted | The node is a tombstone in the ref's state. |
| NP-003 | flagged | The edge carries `flagged = true`. |
| NP-004 | parent | The node's parent, or `-`. |
| NP-005 | status | The node's status. |
| NP-006 | open_blockers | [RULES/state-definition] BT rows. |
| NP-007 | unblocked | [RULES/state-definition] PD rows. |
| NP-008 | has_dangling | FL-003. |
| NP-009 | is_blocker | [AR §3.5]. |
| NP-010 | suspect | [AR §3.5]. |
| NP-011 | replaced_by | The tombstone's `replaced_by`. |
| NP-012 | hold | The node's hold on the ref ([RULES/state-definition] HV rows): `done`, `cancelled`, `deleted` or `none`. |
| NP-013 | deleted_elsewhere | [RULES/state-definition] PD-013. |
| NP-014 | excluded | I26′'s exclusion on the ref ([RULES/state-definition] PD-012). |
| NP-015 | blocking-listed | The node is listed by `blocking` on the ref. |
| NP-016 | conflict | The class of the node's unresolved conflict value, or `none`. |
| NP-017 | refusal | The error code of a refused action. |
| NP-018 | exit | The exit code of the action. |
| NP-019 | refusal-names | The lease id the refusal message names. |
| NP-020 | staged | The staging ref the action landed on. |
| NP-021 | violation | The class of the staged violation. |
| NP-022 | live | The lease is live ([RULES/state-definition] LL rows). |

<!-- table: n40-expect -->
| row | case | ref | subject | property | value | basis | source | note |
|---|---|---|---|---|---|---|---|---|
| NX-001 | C0 | main | #12 | open_blockers | 1 | derived | [AR §3.5] | #40 is open. |
| NX-002 | C0 | main | #12 | unblocked | no | derived | [AR §3.5] | - |
| NX-003 | C0 | main | #40 | is_blocker | yes | derived | [AR §3.5] | - |
| NX-004 | C1 | main | rm | refusal | E409 | design | [50 §3.10] | Impact list names `#41 parent #40`. |
| NX-005 | C1 | main | rm | exit | 6 | design | [50 §5.2] E409 | - |
| NX-006 | C1 | main | #40 | deleted | no | design | [50 §3.10] item 5 | Nothing is written. |
| NX-007 | C2 | main | #40 | deleted | yes | design | [AR §5d.3] | - |
| NX-008 | C2 | main | #40 | replaced_by | #52 | design | [AR §7.1] | - |
| NX-009 | C2 | main | edge:#40:blocks:#12 | exists | no | design | [RULES/delete-policy-matrix EG-007] | - |
| NX-010 | C2 | main | edge:#52:blocks:#12 | exists | yes | design | [RULES/delete-policy-matrix EG-007]; [AR §7.1] "re-point: #52 blocks #12" | - |
| NX-011 | C2 | main | edge:#203:blocks:#40 | exists | no | proposed | [RULES/delete-policy-matrix EG-006]; [OP-2] | - |
| NX-012 | C2 | main | edge:#203:blocks:#52 | exists | yes | proposed | [RULES/delete-policy-matrix EG-006]; [AR §7.1] "`#203 --blocks-> #40` re-point to #52" | - |
| NX-013 | C2 | main | #41 | parent | #9 | design | [RULES/delete-policy-matrix EG-003] | - |
| NX-014 | C2 | main | #12 | open_blockers | 1 | design | [RULES/delete-policy-matrix EF-007] | #52 is open. |
| NX-015 | C2 | main | #12 | unblocked | no | design | [RULES/delete-policy-matrix EF-007] | - |
| NX-016 | C2 | main | #52 | open_blockers | 1 | proposed | [RULES/delete-policy-matrix EF-006] | #203 is open. |
| NX-017 | C2 | main | #17 | suspect | yes | design | [RULES/delete-policy-matrix EG-037]; [AR §7.1] "`#17 cites #40 (pinned c4410) -> suspect`" | - |
| NX-018 | C2 | main | edge:#17:cites:#40 | exists | yes | design | [RULES/delete-policy-matrix EG-037] | A tombstone reference. |
| NX-019 | C2 | main | #77 | suspect | no | design | [RULES/delete-policy-matrix EG-059]; [AR §7.1] "`#77 mentions #40 (text mention)`" | - |
| NX-020 | C2 | main | edge:#77:mentions:#40 | exists | yes | design | [RULES/delete-policy-matrix EG-059] | - |
| NX-021 | C2 | main | #40 | hold | deleted | design | [RULES/delete-policy-matrix DS-008] | - |
| NX-022 | C3 | main | edge:#40:blocks:#12 | flagged | yes | design | [RULES/delete-policy-matrix EG-008] | #40 was unfinished. |
| NX-023 | C3 | main | #12 | has_dangling | yes | design | [RULES/delete-policy-matrix EF-008] | - |
| NX-024 | C3 | main | #12 | open_blockers | 1 | design | [RULES/delete-policy-matrix FL-001] | - |
| NX-025 | C3 | main | #12 | unblocked | no | design | [RULES/delete-policy-matrix FL-001]; [AR §2.5] X4 | - |
| NX-026 | C3 | main | edge:#203:blocks:#40 | exists | no | design | [RULES/delete-policy-matrix EG-005] | - |
| NX-027 | C3 | main | #203 | is_blocker | no | design | [RULES/delete-policy-matrix EF-005] | #203 blocked nothing else. |
| NX-028 | C3 | main | #41 | parent | #9 | design | [RULES/delete-policy-matrix EG-003] | - |
| NX-029 | C3r | main | edge:#40:blocks:#12 | exists | no | proposed | [RULES/delete-policy-matrix FL-006] | - |
| NX-030 | C3r | main | #12 | open_blockers | 0 | proposed | [RULES/delete-policy-matrix FL-006] | - |
| NX-031 | C3r | main | #12 | has_dangling | no | proposed | [RULES/delete-policy-matrix FL-003] | - |
| NX-032 | C3r | main | #12 | unblocked | yes | proposed | [RULES/delete-policy-matrix FL-006] | #12: an open task, no children, no parent, no deferral. |
| NX-033 | C3p | main | edge:#52:blocks:#12 | exists | yes | design | [RULES/delete-policy-matrix FL-005] | - |
| NX-034 | C3p | main | edge:#40:blocks:#12 | exists | no | design | [RULES/delete-policy-matrix FL-005] | - |
| NX-035 | C3p | main | #12 | open_blockers | 1 | design | [RULES/delete-policy-matrix FL-005] | #52 is open. |
| NX-036 | C3p | main | #12 | has_dangling | no | design | [RULES/delete-policy-matrix FL-003] | - |
| NX-037 | C3n | main | edge:#40:blocks:#12 | exists | no | design | [RULES/delete-policy-matrix EG-010] | - |
| NX-038 | C3n | main | #12 | unblocked | yes | design | [RULES/delete-policy-matrix EF-009] | - |
| NX-039 | C4 | main | #41 | deleted | yes | design | [RULES/delete-policy-matrix EG-002] | - |
| NX-040 | C4 | main | #40 | deleted | yes | design | [RULES/delete-policy-matrix EG-002] | - |
| NX-041 | C4 | main | edge:#52:blocks:#12 | exists | yes | design | [RULES/delete-policy-matrix EG-007] | - |
| NX-042 | C5 | lane/x | #40 | deleted | yes | design | [AR §5d.3] row 1 | As C2 on `lane/x`. |
| NX-043 | C5 | main | #40 | deleted | no | design | [AR §5d.3] row 1 "unaffected (git-like isolation)" | - |
| NX-044 | C5 | main | #40 | deleted_elsewhere | yes | design | [AR §5d.3] row 1; [RULES/state-definition PD-013] | - |
| NX-045 | C5 | main | #40 | excluded | yes | design | [AR §3.4] I26′ | - |
| NX-046 | C5 | main | #40 | blocking-listed | no | design | [AR §5d.3] row 1 "keeps it out of `ready`/`blocking`/`claim` on every branch" | #40 is still `is_blocker` on `main`. |
| NX-047 | C5 | main | #12 | open_blockers | 1 | design | [AR §5d.2] "`ready(B)` on branch R uses R's view of A's status" | The delete does not reach `main`'s state. |
| NX-048 | C5 | lane/y | #40 | excluded | yes | design | [AR §3.4] I26′ | - |
| NX-049 | C6 | main | #40 | deleted | yes | design | [RULES/delete-policy-matrix XB-002] | - |
| NX-050 | C6 | main | edge:#52:blocks:#12 | exists | yes | design | [RULES/delete-policy-matrix XB-002] | - |
| NX-051 | C6 | main | #41 | parent | #9 | design | [RULES/delete-policy-matrix XB-002] | - |
| NX-052 | C6 | main | #17 | suspect | yes | design | [RULES/delete-policy-matrix XB-002] | - |
| NX-053 | C6 | main | #40 | deleted_elsewhere | no | design | [RULES/state-definition PD-013] | `main` holds the deletion itself. |
| NX-054 | C6 | lane/y | #40 | excluded | yes | design | [AR §5d.2] | Until `lane/y` syncs. |
| NX-055 | C7b | lane/x | #40 | conflict | DeleteVsModify | design | [RULES/delete-policy-matrix XB-003] | Landed by the sync of step 0. |
| NX-056 | C7b | lane/x | #40 | deleted | yes | design | [RULES/merge-table EP-001] | Tasks: provisional `delete-wins`. |
| NX-057 | C7b | main | merge | exit | 6 | proposed | [RULES/merge-table PR-003] | Refused while `lane/x` holds an unresolved conflict value. |
| NX-058 | C7b | main | #40 | deleted | no | design | [AR §5a.7] step 8 | - |
| NX-059 | C8b | main | merge | staged | merge/lane/x/from/main | design | [AR §5d.3] row 1; [AR §5a.7] step 8 | The whole merge stages. |
| NX-060 | C8b | main | merge | violation | DanglingEdge | design | [RULES/delete-policy-matrix XB-004] | Suggested `resolve 'edge:#205:blocks:#40' --take repoint:52`. |
| NX-061 | C8b | main | merge | exit | 6 | design | [AR §5a.7] step 8 | - |
| NX-062 | C8b | main | edge:#205:blocks:#40 | exists | yes | design | [AR §5a.7] step 8 "dst is untouched" | - |
| NX-063 | C8b | main | #40 | deleted | no | design | [AR §5a.7] step 8 | - |
| NX-064 | C9 | lane/x | rm | exit | 6 | design | [RULES/delete-policy-matrix DP-005] | - |
| NX-065 | C9 | lane/x | rm | refusal-names | L-19 | design | [AR §5d.3] row 3 "`leased by dev#2 on lane/y (L-19)`" | - |
| NX-066 | C9 | lane/x | #40 | deleted | no | design | [RULES/delete-policy-matrix DP-005] | - |
| NX-067 | C9r | lane/x | #40 | deleted | yes | design | [RULES/delete-policy-matrix DS-007] | - |
| NX-068 | C9r | lane/x | lease:L-19 | live | no | design | [RULES/delete-policy-matrix DS-007] | Released with a triage note on the tombstone. |
| NX-069 | C10 | main | #40 | excluded | no | design | [RULES/delete-policy-matrix XB-008] | No live ref holds the deletion. |
| NX-070 | C10 | lane/y | #40 | excluded | no | design | [RULES/delete-policy-matrix XB-008] | - |
| NX-071 | C10f-b | main | #40 | excluded | yes | design | [RULES/delete-policy-matrix XB-008]; [72 M4] scenario 3 | `lane/z` holds the deletion. |
| NX-072 | C10f-b | lane/z | #40 | deleted | yes | design | [AR §5a.3] fork | - |
| NX-073 | C11 | main | #40 | deleted | no | design | [RULES/delete-policy-matrix UD-001] | - |
| NX-074 | C11 | main | #40 | status | open | design | [RULES/delete-policy-matrix UD-001] | - |
| NX-075 | C11 | main | edge:#40:blocks:#12 | exists | yes | design | [RULES/delete-policy-matrix UD-003] | - |
| NX-076 | C11 | main | edge:#52:blocks:#12 | exists | no | design | [RULES/delete-policy-matrix UD-003] | - |
| NX-077 | C11 | main | edge:#203:blocks:#40 | exists | yes | proposed | [RULES/delete-policy-matrix UD-003]; [RULES/delete-policy-matrix EG-006] | - |
| NX-078 | C11 | main | edge:#203:blocks:#52 | exists | no | proposed | [RULES/delete-policy-matrix UD-003]; [RULES/delete-policy-matrix EG-006] | - |
| NX-079 | C11 | main | #41 | parent | #40 | design | [RULES/delete-policy-matrix UD-004] | - |
| NX-080 | C11 | main | #17 | suspect | no | design | [RULES/delete-policy-matrix UD-001] | - |
| NX-081 | C11 | main | #40 | hold | none | design | [RULES/delete-policy-matrix UD-006] | - |
| NX-082 | C12 | main | import | staged | import/main | design | [RULES/delete-policy-matrix XB-009] | `#41 parent #40` and `#203 blocks #40` dangle. |
| NX-083 | C12 | main | import | violation | DanglingEdge | design | [RULES/delete-policy-matrix XB-009] | - |
| NX-084 | C12 | main | #40 | deleted | no | design | [AR §5b.6] step 4 | The local ref stays where it was. |

## Coverage

These tables specify semantics, not bytes. The byte layouts are [F06]'s (`Delete`, `Undelete`, `RemoveEdge`, `AddEdge`,
`Move`, the `flagged` edge property), [F08]'s (the schema's per-edge delete policy, [60 §2.5] "Schema as data"), [F09]'s
(`TOMB`), [F14]'s (tombstone `.moi` lines) and [F19]'s (E305, E405, E406, E409 and the lease refusal).

| Checklist row | Covered by | Fixture | Model function |
|---|---|---|---|
| [60 §2.5] "Schema as data": edges (class, **policy**, acyclicity, cardinality), the policy part | `edge-policy`, the vocabularies, DP-008, DP-009 | WP-94 suite `delete-policy`; GT10 node-40 (WP-22) | `model::delete::edge_policy` |
| [AR §3.4] I2, I3, I32′, I39′ | FL-004, EA-006, DP-005, TB-009, TB-010 | WP-94; GT10 node-40 | `model::delete::apply` |
| GT10 node-40 table on one branch and across branches ([60 §3.13]) | `n40-nodes`, `n40-edges`, `n40-cases`, `n40-expect` | WP-22 `fixtures/gt10/` (owner-verified core set) | `model::delete::apply`, `model::i26::excluded` |
| [40 §2.11] R-12: I-F14 (only `Undelete` and `links fix --restore` bring a dead derived uid back), delete part | UD-007, XB-007 | WP-94 | `model::delete::undelete` |

No F-row, X-F row or [90 §10.1] item concerns delete policies.

## Holes

None. No delete rule depends on a value an M0 measurement decides; `store.suspect-budget` is a configuration key whose
value changes only whether `affected` is complete (DS-010).

## Open points for the review

1. **Deleting a blocker that was already finished** (EG-009, EG-015, FL-002). [AR §3.3] says "else flag" with no
   condition. Taken literally, deleting a `done` blocker would block its dependent anew, which X4 ("a deleted blocker
   never silently unblocks") does not ask for. Proposed: flag only a source that was an open blocker (`blocks`) or a
   gating verdict (`gates`); drop otherwise. A flagged `gates` edge refuses completion only and leaves readiness alone,
   consistent with X5 ([RULES/status-machines] OP-22).
2. **Re-pointing in-edges to the replacement** (EG-006, EG-012, EF-006, EF-011). [AR §3.3] says "drop" for a `blocks` or
   `gates` edge into the deleted node, but [AR §7.1]'s dry run re-points `#203 --blocks-> #40` to #52 under
   `--replaced-by`. Proposed: re-point when a replacement is given, drop otherwise; the example is the more specific
   statement.
3. **Replacement validity** (DP-007). Proposed: the replacement must be live, outside the deleted set, and of a kind the
   re-pointed edge accepts at that end ([50 §2.5] endpoint kinds); otherwise E409 with the edge named. [F19] may give it
   its own code.
4. **`duplicate_of` "re-point to canonical"** (EG-029, EF-016). Read as: with `--replaced-by Y`, the duplicate points at
   Y, which must itself be canonical (I7); without it, restrict.
5. **`depends_on` "drop + src `suspect`"** (EG-031, EF-017). A dropped edge leaves nothing from which `suspect` can be
   derived, and `suspect` is purely derived (X8), while I2 allows no dangling structural edge except a flagged blocker.
   Proposed: drop the edge and name the dependent section in `affected` with the reason, so the change feed and hook
   deltas carry it; no lasting `suspect`. The alternative, keeping a flagged `depends_on` edge, would widen I2, I39′
   and the tombstone format ([F14]).
6. **`--reassign` with no parent area** (EG-026). Proposed: refused like the plain restrict.
7. **`implements` on a deleted target** (EG-039). [AR §3.3]'s `implements` row says "src `suspect` if the target is
   superseded"; [AR §3.5]'s `suspect` includes a deleted target for `implements`. This table follows [AR §3.5], the
   single definition of the derived predicate.
8. **Edges inside the deleted set** (DS-003). Under `--cascade` an edge can have both ends deleted. Proposed: historical
   edges are retained as the source tombstone's out-edges; structural ones are dropped, since no live dependent is left
   to protect and I39′ retains only flagged edges.
9. **The lease refusal's code** (DP-005). [50 §3.10] says "a live lease refuses unless `RELEASE` (I32′)" without a code;
   E407 is for a missing or stale presented lease. [F19] assigns one (WP-18).
10. **The `suspect` budget** (DS-010). [AR §2.5] says "a violation record is written" beyond the budget, but `Violation`
    ops exist only on staging refs ([AR §5b.4]). Proposed: `affected_complete = 0` (I42′), and readers recompute
    `suspect`. [AR §3.5] defines `suspect` by one hop while its maintenance cost says "O(closure)"; the model uses the
    one-hop definition, and WP-14 states which is meant.
11. **Resolving a flagged edge without a replacement** (FL-006, NC-005). [AR §7.1] shows `resolve 'edge:#40:blocks:#12'`
    and the option `repoint:ID`, but no spelling for "drop the flagged edge". Proposed: the drop outcome exists and is
    also reached by `unlink`/`DELETE e`; WP-18 and WP-19 fix the `resolve` spelling.
12. **File name and registry.** This file is named as its commissioning task names it, `delete-policy-matrix.md`;
    [RULES/README] §1.1 and the specification index list it as `delete-policy.md`. One of them is renamed at
    integration, and every `[RULES/delete-policy-matrix]` citation follows. Its tables must be added to
    [RULES/README] §7:

    ```
    | RG-0xx | `delete-options` | delete-policy-matrix.md | vocabulary | DO | row:id, option:token, basis:enum, source:cite, definition:text | - |
    | RG-0xx | `edge-conditions` | delete-policy-matrix.md | vocabulary | CD | row:id, condition:token, basis:enum, source:cite, definition:text | - |
    | RG-0xx | `edge-actions` | delete-policy-matrix.md | vocabulary | EA | row:id, action:token, basis:enum, source:cite, definition:text | - |
    | RG-0xx | `edge-effects` | delete-policy-matrix.md | vocabulary | EF | row:id, effect:token, basis:enum, source:cite, definition:text | - |
    | RG-0xx | `delete-preconditions` | delete-policy-matrix.md | procedure | DP | row:id, check:token, refusal:token, exit:token, basis:enum, source:cite, note:text | - |
    | RG-0xx | `edge-policy` | delete-policy-matrix.md | decision | EG | row:id, edge:token, end:enum(dst-deleted/src-deleted), option:token, policy:token, condition:token, action:token, effect:token, basis:enum, source:cite, note:text | - |
    | RG-0xx | `delete-steps` | delete-policy-matrix.md | procedure | DS | row:id, step:token, action:token, basis:enum, source:cite, note:text | - |
    | RG-0xx | `flagged-edges` | delete-policy-matrix.md | procedure | FL | row:id, rule:token, basis:enum, source:cite, note:text | - |
    | RG-0xx | `tombstone` | delete-policy-matrix.md | decision | TB | row:id, item:token, kept:enum(yes/no), basis:enum, source:cite, note:text | - |
    | RG-0xx | `undelete` | delete-policy-matrix.md | procedure | UD | row:id, item:token, effect:token, basis:enum, source:cite, note:text | - |
    | RG-0xx | `cross-branch` | delete-policy-matrix.md | map | XB | row:id, case:token, realized_by:tokens, basis:enum, source:cite, note:text | - |
    | RG-0xx | `n40-nodes` | delete-policy-matrix.md | decision | NN | row:id, node:token, kind:token, status:token, parent:token, note:text | fixture data |
    | RG-0xx | `n40-edges` | delete-policy-matrix.md | decision | NG | row:id, src:token, kind:token, dst:token, props:token, note:text | fixture data |
    | RG-0xx | `n40-cases` | delete-policy-matrix.md | decision | NC | row:id, case:token, ref:token, after:token, action:token, basis:enum, source:cite, note:text | fixture data |
    | RG-0xx | `n40-properties` | delete-policy-matrix.md | vocabulary | NP | row:id, property:token, definition:text | - |
    | RG-0xx | `n40-expect` | delete-policy-matrix.md | decision | NX | row:id, case:token, ref:token, subject:token, property:token, value:token, basis:enum, source:cite, note:text | fixture data |
    ```

13. **Worked rows versus GT10 fixtures.** The `n40-*` tables are rule data the owner signs; WP-22's `fixtures/gt10/`
    files are written from the specification by another author (S3). A disagreement between the two is a
    specification finding, not a fixture fix.
