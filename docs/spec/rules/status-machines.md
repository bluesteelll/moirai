# Status machines: states, transitions, guards, doors and derived effects

| Field | Value |
|---|---|
| Status | draft, pass 1 pending |
| Work package | WP-90 (R-MODEL); consumed by WP-93b (LQ `SET`, `REOPEN`, `tx.complete`), WP-94 (the status-machine suite) and M2's write path |
| Sources | [AR §3.1] the `status` column and the virtual `done`; [AR §3.2] status sets per kind; [AR §3.3] `gates`, `answers`, `supersedes`; [AR §3.4] I6, I8, I13, I14, I33′, I42′; [AR §3.5] derived state; [AR §3.6] guarded transitions; [AR §5a.1] branch kinds and write masks; [AR §5a.9] `lane close`, `lane freeze`; [AR §6.2] `claim`, `--start`, `complete`; [AR §6.6] quiet mode; [AR §7.1] verbs and exit codes; [AR §7.3] role write policy and its R4 rows; [AR §13] policy data; [40 §2.2] artifact status; [40 §2.9] effect on referrers; [40 §2.10] I-F7, I-F14; [40 §3.2], [40 §3.5], [40 §3.7] the doors that change artifact status; [40 §6.3] R4 role rows; [50 §3.8] `done`; [50 §3.9] item 6 read-only views; [50 §3.10] items 2, 5, 6; [50 §4.2] `tx.complete`; [50 §5.2] error codes; [50 §6.5] per-statement role policy; [90 §4.3] rights from presented leases; [90 §10.1] `LEASES.kind`, `role` |
| Format | [RULES/README] |
| Cited as | [RULES/status-machines]; a row as [RULES/status-machines TR-004] |

## 1. What this table decides

For every node kind, this file decides:

- which values the header column `status` may hold, which of them a new node may start in, and what the virtual
  field `done` reads (`statuses`);
- every transition a write may make, and through which **door** (the family of statements and verbs that performs
  it) (`transitions`, `doors`);
- the guards that refuse a transition, with their error code and exit code (`guards`, `transition-guards`);
- which rows of the role write policy decide who may use each door (`door-roles`); the roles themselves, their scopes
  and their rights are [RULES/role-write-policy]'s, the single source for them;
- on which views a status write is allowed at all (`branch-mask`);
- which derived predicates and runtime caches a status change touches (`derived-effects`);
- what `complete` writes for each outcome (`complete-outcomes`), and the rules that hold across all tables
  (`general-rules`).

It does not decide:

- the merge order of statuses: that is [RULES/merge-table] `status-lattice` (SL rows), which every `statuses` row
  cites;
- the integer codes of statuses and resolutions: [F08];
- existence changes (`Delete`, `Undelete`) and their effects on dependents: [RULES/delete-policy-matrix];
- cross-branch exclusion and the marker cache: [RULES/state-definition].

The status machine governs **direct writes** only. Status values produced by merge, `sync`, revert, cherry-pick,
`undo`, `op restore` and import follow [RULES/merge-table] and are checked against I8's state rule alone (GR-008).

## 2. How to read the tables

The format is [RULES/README]: a marker line announces each machine-readable table, the first column is the row id,
the last column is free prose, and every other cell holds one token (or a `, `-separated token list, or a citation).

- **Kinds and statuses** are the schema symbols of [AR §3.2] and [40 §2.2] (`in_progress`, `ready_to_merge`,
  `moved_declared`). `*` means "any" in the columns that say so.
- **Status sets in `derived-effects`.** `@finished` is the set of the kind's statuses whose `done` cell is `yes`
  (task: `done`, `cancelled`; verdict: `accepted`); `@unfinished` is every other status of the kind.
- **Doors** are defined in `doors`. A statement or verb belongs to exactly one door; the door's spellings are in its
  note.
- **Roles** are those of [RULES/role-write-policy] `role-rows`. The role of a caller is the role of the lease it
  presents ([90 §4.3]); an unleased caller has the role `general-purpose`.
- **`basis`** is the shared enum of [RULES/README] §5: `design` (the cited text states the row), `derived` (it follows
  from the cited rules), `proposed` (R-MODEL's resolution, listed in the open points), `gap`, `withdrawn`.
- **Evaluation.** A status write is allowed if and only if: the view allows status writes (`branch-mask`); a
  `transitions` row matches (kind, from, to, door); every `transition-guards` row for (kind, from, to) holds on the
  candidate state; and the role write policy allows the caller's effective role to use that door for that transition
  (`door-roles` names the deciding rows). The general rules add the details.

## 3. Status-bearing fields

<!-- table: status-fields -->
| row | kind | field | stored | guarded | basis | source | note |
|---|---|---|---|---|---|---|---|
| SF-001 | * | `status` | yes | yes | design | [AR §3.1]; [AR §3.4] I8 | The header column `status` (u8). Its integer codes are [F08]'s; these tables use the schema symbols. `blocked` and container-`done` are never stored. |
| SF-002 | * | `done` | no | yes | design | [AR §3.1]; [50 §3.8] | Virtual. Reading it follows the `done` cell of `statuses`; writing it performs a transition (GR-009, GR-010). |
| SF-003 | task | `resolution` | yes | yes | design | [AR §3.1]; [AR §4.3] `SetStatus` | Travels with the status in `SetStatus{id, old, new, resolution}`; GR-014. |
| SF-004 | finding | `resolution` | yes | yes | design | [AR §3.1]; [AR §4.3] `SetStatus` | As SF-003. |
| SF-005 | task | `reopen_count` | yes | no | design | [AR §3.2]; [AR §3.6] | Counter; the `reopen` door adds 1 (GR-011). |
| SF-006 | task | `phase_state` | yes | no | proposed | [AR §3.2]; [AR §3.6]; [OP-19] | A workflow enum "advanced by verdicts"; the design states no guard, so no machine is imposed here. |

## 4. Statuses

`initial = yes` marks a status a `Create` may start in. The kind's **default** status — the one a `Create` without a
status starts in, which the canonical form holds as absent ([F07 §6.3]) — is [F08 §9.1]'s "initial status (default)"
column; it is the only `initial = yes` status of every kind but `artifact`, whose default is `present` (`planned` only
through `link --planned`). The model checks that each default is an `initial = yes` status. `done` gives what the
virtual field reads: `yes` or `no` for that status, `derived` for a kind whose `done` is a derived predicate independent
of the status, and `absent` for a kind that has no `done`. `lattice` names the [RULES/merge-table] row of the same kind
and status; the model checks at load that it exists and matches.

<!-- table: statuses -->
| row | kind | status | initial | done | lattice | basis | source | note |
|---|---|---|---|---|---|---|---|---|
| ST-001 | task | `open` | yes | no | SL-001 | design | [AR §3.2] | - |
| ST-002 | task | `in_progress` | no | no | SL-002 | design | [AR §3.2] | - |
| ST-003 | task | `done` | no | yes | SL-003 | design | [AR §3.1]; [AR §3.2] | - |
| ST-004 | task | `deferred` | no | no | SL-004 | design | [AR §3.2] | Side state. |
| ST-005 | task | `cancelled` | no | yes | SL-005 | design | [AR §3.1]; [50 §3.8] W03 | Side state; `done` reads true ("`status ∈ {done, cancelled}`"). |
| ST-006 | task | `frozen` | no | no | SL-006 | design | [AR §3.2] | Side state, "conflict against `done`". |
| ST-007 | doc | `draft` | yes | absent | SL-007 | design | [AR §3.2] | - |
| ST-008 | doc | `current` | no | absent | SL-008 | design | [AR §3.2] | - |
| ST-009 | doc | `superseded` | no | absent | SL-009 | design | [AR §3.2] | - |
| ST-010 | doc | `archived` | no | absent | SL-010 | design | [AR §3.2] | - |
| ST-011 | note | `active` | yes | absent | SL-011 | design | [AR §3.2] | - |
| ST-012 | note | `superseded` | no | absent | SL-012 | design | [AR §3.2] | - |
| ST-013 | note | `retracted` | no | absent | SL-013 | design | [AR §3.2] | - |
| ST-014 | note | `archived` | no | absent | SL-014 | design | [AR §3.2] | - |
| ST-015 | rule | `proposed` | yes | absent | SL-015 | design | [AR §3.2] | - |
| ST-016 | rule | `active` | no | absent | SL-016 | design | [AR §3.2] | - |
| ST-017 | rule | `superseded` | no | absent | SL-017 | design | [AR §3.2] | - |
| ST-018 | rule | `retracted` | no | absent | SL-018 | design | [AR §3.2] | - |
| ST-019 | rule | `archived` | no | absent | SL-019 | design | [AR §3.2] | - |
| ST-020 | decision | `proposed` | yes | absent | SL-020 | design | [AR §3.2] | - |
| ST-021 | decision | `accepted` | no | absent | SL-021 | design | [AR §3.2] | "never edited after acceptance, only superseded". |
| ST-022 | decision | `rejected` | no | absent | SL-022 | design | [AR §3.2] | - |
| ST-023 | decision | `superseded` | no | absent | SL-023 | design | [AR §3.2] | - |
| ST-024 | question | `open` | yes | derived | SL-024 | design | [AR §3.2]; [AR §3.5] `answered`; [OP-20] | `done` of a question is the derived predicate `answered` (a live `answers` edge visible on the reading branch), whatever the stored status. |
| ST-025 | question | `answered` | no | derived | SL-025 | design | [AR §3.2]; [AR §3.5]; [OP-20] | As ST-024. |
| ST-026 | question | `dropped` | no | derived | SL-026 | design | [AR §3.2]; [OP-20] | As ST-024: a dropped question that still blocks a task keeps it blocked until the edge is removed. |
| ST-027 | finding | `open` | yes | absent | SL-027 | design | [AR §3.2]; [OP-21] | - |
| ST-028 | finding | `confirmed` | no | absent | SL-028 | design | [AR §3.2] | - |
| ST-029 | finding | `refuted` | no | absent | SL-029 | design | [AR §3.2] | - |
| ST-030 | finding | `fixed` | no | absent | SL-030 | design | [AR §3.2] | - |
| ST-031 | finding | `deferred` | no | absent | SL-031 | design | [AR §3.2] | - |
| ST-032 | finding | `withdrawn` | no | absent | SL-032 | design | [AR §3.2] | - |
| ST-033 | verdict | `open` | yes | no | SL-033 | design | [AR §3.1]; [AR §3.2] | - |
| ST-034 | verdict | `accepted` | no | yes | SL-034 | design | [AR §3.1] | - |
| ST-035 | verdict | `superseded` | no | no | SL-035 | design | [AR §3.2] | - |
| ST-036 | measurement | `current` | yes | absent | SL-036 | design | [AR §3.2] | `stale` is derived, never a status. |
| ST-037 | measurement | `moved_declared` | no | absent | SL-037 | design | [AR §3.2] | - |
| ST-038 | measurement | `retracted` | no | absent | SL-038 | design | [AR §3.2] | - |
| ST-039 | artifact | `planned` | yes | absent | SL-039 | design | [40 §2.2]; [40 §3.2]; [F08 §9.1] | Created by `link --planned`; not the default status. |
| ST-040 | artifact | `present` | yes | absent | SL-040 | design | [40 §2.2]; [40 §3.2]; [40 §3.3]; [F08 §9.1] | The default status ([F08 §9.1]). Created by capture (`link --at`, `file add`) of a file that exists. Link states such as `missing` are tree-derived and never stored. |
| ST-041 | artifact | `removed` | no | absent | SL-041 | design | [40 §2.2]; [40 §3.5] | "deleted on purpose". |
| ST-042 | run | `running` | yes | absent | SL-042 | design | [AR §3.2] | - |
| ST-043 | run | `green` | no | absent | SL-043 | design | [AR §3.2] | - |
| ST-044 | run | `red` | no | absent | SL-044 | design | [AR §3.2] | - |
| ST-045 | run | `stopped` | no | absent | SL-045 | design | [AR §3.2] | - |
| ST-046 | run | `died` | no | absent | SL-046 | design | [AR §3.2] | - |
| ST-047 | lane | `active` | yes | absent | SL-047 | design | [AR §3.2] | - |
| ST-048 | lane | `ready_to_merge` | no | absent | SL-048 | design | [AR §3.2] | - |
| ST-049 | lane | `merge_pending` | no | absent | SL-049 | design | [AR §3.2] | - |
| ST-050 | lane | `merged` | no | absent | SL-050 | design | [AR §3.2] | - |
| ST-051 | lane | `frozen` | no | absent | SL-051 | design | [AR §3.2] | - |
| ST-052 | lane | `abandoned` | no | absent | SL-052 | design | [AR §3.2] | - |
| ST-053 | lane | `measuring` | no | absent | SL-053 | design | [AR §3.2]; [AR §6.6] | Implies quiet mode (DE-027). |
| ST-054 | area | `active` | yes | absent | SL-054 | design | [AR §3.2] | Also the status of R4 root nodes. |
| ST-055 | area | `archived` | no | absent | SL-055 | design | [AR §3.2] | - |

## 5. Doors

A door is a family of statements and verbs that performs a status change with one set of requirements. `requires`
lists what the door itself checks before any guard; its tokens are defined in the row's note.

<!-- table: doors -->
| row | door | requires | basis | source | note |
|---|---|---|---|---|---|
| DR-001 | set-status | target-live | design | [50 §3.10] item 6; [AR §7.1] | `SET x.status = 's'` and `SET x.done = true` in a `TX` (also inside `apply` and MCP `write`), and the verbs that expand to them: `set ID --status S`, `set ID --done`, `answer Q`, `run close`. `lane close` and `lane freeze` are DR-013's (spec sync 2b). |
| DR-002 | tx-complete | task-lease | design | [AR §6.2]; [50 §4.2] | `CALL tx.complete(...)`, `complete ID --lease L`, MCP `complete`, and the complete records of `apply`. `task-lease`: a presented task lease on the target with its current fencing token (I17′), on the lease's branch. Releases the lease into `settled` (CO rows). |
| DR-003 | claim-start | task-lease | design | [AR §6.2] | `claim ID --start` (and `tx.claim` with start): the new lease is the presented lease; the claim's own precondition, `ready` on the claimer's branch, is [RULES/state-definition]'s. |
| DR-004 | lease-first-write | task-lease | design | [AR §6.2] | The first `set` of the leased task that presents its lease while the task is `open` also writes `open → in_progress`, in the same commit ("otherwise the first `set`/`complete` under the lease performs it"). |
| DR-005 | reopen | reason | design | [AR §3.6]; [50 §3.10] item 6 | `REOPEN x REASON r`, `reopen ID --reason T`. `reason`: a non-empty reason in the statement. "The only way from `done` back to `open`". |
| DR-006 | supersede | supersedes-edge | design | [AR §3.4] I6; [AR §3.6]; [50 §3.10] item 6 | `CREATE (new)-[:SUPERSEDES]->(old)`, `supersede OLD --with NEW`. `supersedes-edge`: the edge is added by the same statement, which also writes `old`'s status; at most one active superseder per target (X12). |
| DR-007 | retract | reason | design | [AR §3.6]; [AR §7.1] | `retract ID --reason T`. Where the reason is stored is [OP-9]. |
| DR-008 | settle | writer-tree | design | [40 §3.2]; [40 §4.2]; [40 §5.3]; [72 M11] | R4 settle points (`links sync`, `SessionStart`, the separate settle commit after `complete`, the file verbs, the git `post-commit` block). `writer-tree`: the branch's designated tree, fresh for the node, after quiescence; each re-bind is CAS-guarded on `rev_seq`. |
| DR-009 | file-rm | writer-tree | design | [40 §3.5]; [AR §5e.5] | `file rm PATH --yes` (CLI only, through the `FsIntent` protocol); outside the writer tree exit 5. |
| DR-010 | links-fix | - | design | [40 §3.7]; [40 §6.3] | `links fix ID --drop`, `--same-as ID`, `--split` (to `removed`); `--restore` (to `present`); and the MCP `write` named mutation behind `links fix`. Records a decision, so it works from any eligible tree. |
| DR-011 | deletion-inference | writer-tree | design | [40 §2.10] I-F7; [AR §13] `files.deletion-inference` | Only when `files.deletion-inference = main-tree-commits`: a settle on the main tree that sees a deletion commit records `removed`. |
| DR-012 | delete-policy | - | design | [AR §3.3] `answers` row | A policy op that a node `Delete` writes in its own commit ([RULES/delete-policy-matrix] EG-022). |
| DR-013 | lane-close | lane-verb | proposed | [AR §5a.9] `lane close`, `lane freeze`; [AR §7.6] step 8; [API §11.5] `LaneClose`; [OP-8] | `lane close NAME` and `lane freeze NAME` (API `LaneClose`, `mode` `close` or `freeze`), from every non-side lane state, `active` included ([AR §7.6] step 8 closes a lane that is still `active`). `lane-verb`: the verb picks the target itself — for `close`, `merged` when tip(`main`) contains tip(`lane/<name>`) (`main` has absorbed the lane), `abandoned` otherwise; for `freeze`, `frozen` (spec sync 2b). |

## 6. Transitions

`move` says how the transition relates to the merge order: `up` (to a greater status), `down` (to a smaller one),
`to-side` (into a side status), `from-side` (out of one). Every transition that is not listed is refused (GR-001).

<!-- table: transitions -->
| row | kind | from | to | door | move | basis | source | note |
|---|---|---|---|---|---|---|---|---|
| TR-001 | task | `open` | `in_progress` | set-status | up | design | [AR §3.6] | - |
| TR-002 | task | `open` | `in_progress` | claim-start | up | design | [AR §6.2] | - |
| TR-003 | task | `open` | `in_progress` | lease-first-write | up | design | [AR §6.2] | - |
| TR-004 | task | `in_progress` | `done` | set-status | up | design | [AR §3.6]; [50 §3.10] item 6 | Guards TG-001, TG-002. |
| TR-005 | task | `in_progress` | `done` | tx-complete | up | design | [AR §6.2]; [50 §4.2] | Guards TG-001, TG-002. |
| TR-006 | task | `open` | `done` | set-status | up | derived | [AR §3.6]; [AR §6.2]; [AR §4.3] coalescing | The compound `open → in_progress → done` in one statement; the net op is `SetStatus{open → done}`. Guards TG-003, TG-004 (those of the second step). |
| TR-007 | task | `open` | `done` | tx-complete | up | design | [AR §6.2] "`complete` from `open` is the compound `open → in_progress → done` in one commit" | Guards TG-003, TG-004. |
| TR-008 | task | `open` | `cancelled` | set-status | to-side | design | [AR §3.6] | Not guarded ([OP-10]). |
| TR-009 | task | `open` | `deferred` | set-status | to-side | design | [AR §3.6] | - |
| TR-010 | task | `open` | `frozen` | set-status | to-side | design | [AR §3.6] | - |
| TR-011 | task | `in_progress` | `cancelled` | set-status | to-side | design | [AR §3.6] | Not guarded ([OP-10]). |
| TR-012 | task | `in_progress` | `deferred` | set-status | to-side | design | [AR §3.6] | - |
| TR-013 | task | `in_progress` | `frozen` | set-status | to-side | design | [AR §3.6] | - |
| TR-014 | task | `deferred` | `open` | set-status | from-side | design | [AR §3.6] | - |
| TR-015 | task | `done` | `open` | reopen | down | design | [AR §3.6]; [50 §3.10] item 6 | "only through `reopen --reason` … never a merge artefact"; GR-011 adds 1 to `reopen_count`. |
| TR-016 | task | `cancelled` | `open` | reopen | from-side | proposed | [AR §4.5] step 4 "`SetStatus{done or cancelled → open}`"; [OP-3] | [AR §3.6] names no exit from `cancelled`; [AR §4.5] writes a `cleared` marker for this very transition. |
| TR-017 | task | `frozen` | `open` | set-status | from-side | proposed | [AR §3.6]; [OP-4] | Unfreeze; without it `frozen` is terminal. |
| TR-018 | task | `in_progress` | `open` | set-status | down | proposed | [AR §6.2]; [OP-5] | Gives a started task back after its holder's lease ended (reclaim, expiry), so it can be `ready` again. |
| TR-019 | doc | `draft` | `current` | set-status | up | design | [AR §3.6] | - |
| TR-020 | doc | `current` | `superseded` | supersede | to-side | design | [AR §3.6]; [AR §3.4] I6 | "only together with a `supersedes` edge". |
| TR-021 | doc | `draft` | `archived` | set-status | to-side | derived | [AR §3.6] "archived" | [AR §3.6] lists `archived` without a source state; every non-side state is taken. |
| TR-022 | doc | `current` | `archived` | set-status | to-side | derived | [AR §3.6] "archived" | As TR-021. |
| TR-023 | note | `active` | `superseded` | supersede | to-side | design | [AR §3.6]; [AR §3.4] I6 | - |
| TR-024 | note | `active` | `retracted` | retract | to-side | design | [AR §3.6] "retracted with a reason" | - |
| TR-025 | note | `active` | `archived` | set-status | to-side | design | [AR §3.6] | - |
| TR-026 | rule | `proposed` | `active` | set-status | up | design | [AR §3.6] | - |
| TR-027 | rule | `active` | `superseded` | supersede | to-side | design | [AR §3.6]; [AR §3.4] I6 | - |
| TR-028 | rule | `proposed` | `retracted` | retract | to-side | derived | [AR §3.6] | As TR-021: every non-side state. |
| TR-029 | rule | `active` | `retracted` | retract | to-side | design | [AR §3.6] | - |
| TR-030 | rule | `proposed` | `archived` | set-status | to-side | derived | [AR §3.6] | As TR-021. |
| TR-031 | rule | `active` | `archived` | set-status | to-side | design | [AR §3.6] | - |
| TR-032 | decision | `proposed` | `accepted` | set-status | up | design | [AR §3.6] | - |
| TR-033 | decision | `proposed` | `rejected` | set-status | to-side | proposed | [AR §3.2]; [OP-6] | [AR §3.6] names no source for `rejected`. |
| TR-034 | decision | `accepted` | `superseded` | supersede | to-side | design | [AR §3.6]; [AR §3.2] "never edited after acceptance, only superseded" | - |
| TR-035 | question | `open` | `answered` | set-status | up | design | [AR §3.6] | Guards TG-005, TG-006. |
| TR-036 | question | `open` | `dropped` | set-status | to-side | design | [AR §3.6] | - |
| TR-037 | question | `answered` | `open` | reopen | down | design | [AR §3.6] "reopen explicit" | - |
| TR-038 | question | `dropped` | `open` | reopen | from-side | derived | [AR §3.6] "reopen explicit" | - |
| TR-039 | question | `answered` | `open` | delete-policy | down | design | [AR §3.3] `answers` "question reopens" | Written by the `Delete` of the last answering node ([RULES/delete-policy-matrix] EG-022). |
| TR-040 | finding | `open` | `confirmed` | set-status | up | design | [AR §3.6] | "a separate guarded write, never an automatic flip". |
| TR-041 | finding | `open` | `refuted` | set-status | up | design | [AR §3.6] | As TR-040. |
| TR-042 | finding | `open` | `deferred` | set-status | up | design | [AR §3.6] | - |
| TR-043 | finding | `open` | `withdrawn` | set-status | up | design | [AR §3.6] | - |
| TR-044 | finding | `confirmed` | `fixed` | set-status | up | design | [AR §3.6]; [AR §3.4] I13 | Guard TG-007. |
| TR-045 | verdict | `open` | `accepted` | set-status | up | design | [AR §3.6] | Verdict fields are immutable once written; only the status moves. |
| TR-046 | verdict | `open` | `superseded` | set-status | to-side | design | [AR §3.6]; [OP-23] | No `supersedes` edge is required for a verdict. |
| TR-047 | measurement | `current` | `moved_declared` | set-status | to-side | design | [AR §3.6] | - |
| TR-048 | measurement | `current` | `retracted` | retract | to-side | design | [AR §3.6] | - |
| TR-049 | artifact | `planned` | `present` | settle | up | design | [40 §3.2] | Guard TG-009. |
| TR-050 | artifact | `present` | `removed` | file-rm | to-side | design | [40 §3.5]; [40 §2.10] I-F7 | - |
| TR-051 | artifact | `present` | `removed` | links-fix | to-side | design | [40 §3.7]; [40 §2.10] I-F7 | `--drop`, `--same-as`, `--split`. |
| TR-052 | artifact | `planned` | `removed` | links-fix | to-side | proposed | [40 §3.7]; [OP-7] | `--drop` of a planned link. |
| TR-053 | artifact | `present` | `removed` | deletion-inference | to-side | design | [40 §2.10] I-F7 | - |
| TR-054 | artifact | `removed` | `present` | links-fix | from-side | design | [40 §3.7] `--restore`; [40 §2.10] I-F14 | One of the two explicit doors that may bring a removed file node back. |
| TR-081 | artifact | `present` | `removed` | settle | to-side | derived | [RULES/link-merge-rules] LV-005; [40 §2.3] "The residual case"; [40 §5.5] | A settle in a writer tree fresh for both claimants unifies a `PathClaim` by an exact rename inside one commit: the claimant whose alias is the rename's source becomes `removed{reason: same-as}` (the automatic form of `links fix --same-as`, which [F18 §2.7] I-F7 does not list; open point 26). |
| TR-055 | run | `running` | `green` | set-status | up | design | [AR §3.6]; [AR §3.4] I14 | Guard TG-008. |
| TR-056 | run | `running` | `red` | set-status | up | design | [AR §3.6] | - |
| TR-057 | run | `running` | `stopped` | set-status | up | design | [AR §3.6] | - |
| TR-058 | run | `running` | `died` | set-status | up | design | [AR §3.6] | - |
| TR-059 | lane | `active` | `ready_to_merge` | set-status | up | design | [AR §3.6] | - |
| TR-060 | lane | `ready_to_merge` | `merge_pending` | set-status | up | design | [AR §3.6] | - |
| TR-061 | lane | `merge_pending` | `merged` | set-status | up | design | [AR §3.6]; [AR §5a.9] `lane close` | A direct status write; `lane close` itself goes through the door `lane-close` (DR-013, TR-072 to TR-080; [OP-8]). |
| TR-062 | lane | `active` | `frozen` | set-status | to-side | derived | [AR §3.6]; [AR §5a.9] `lane freeze` | - |
| TR-063 | lane | `ready_to_merge` | `frozen` | set-status | to-side | proposed | [AR §3.6]; [OP-8] | - |
| TR-064 | lane | `merge_pending` | `frozen` | set-status | to-side | proposed | [AR §3.6]; [OP-8] | - |
| TR-065 | lane | `active` | `abandoned` | set-status | to-side | derived | [AR §3.6]; [AR §5a.9] `lane close` | - |
| TR-066 | lane | `ready_to_merge` | `abandoned` | set-status | to-side | proposed | [AR §3.6]; [OP-8] | - |
| TR-067 | lane | `merge_pending` | `abandoned` | set-status | to-side | proposed | [AR §3.6]; [OP-8] | - |
| TR-068 | lane | `active` | `measuring` | set-status | to-side | derived | [AR §3.6]; [AR §6.6] | - |
| TR-069 | lane | `measuring` | `active` | set-status | from-side | proposed | [AR §6.6]; [OP-8] | Ends the measuring window; without it quiet mode could never end for that lane. |
| TR-070 | lane | `frozen` | `active` | set-status | from-side | proposed | [OP-8] | Unfreeze. |
| TR-072 | lane | `active` | `merged` | lane-close | up | proposed | [AR §7.6] step 8; [API §11.5]; [OP-8] | `lane close` of a lane `main` has absorbed while its status is still `active` (DR-013). |
| TR-073 | lane | `ready_to_merge` | `merged` | lane-close | up | proposed | [AR §5a.9] `lane close`; [API §11.5]; [OP-8] | As TR-072. |
| TR-074 | lane | `merge_pending` | `merged` | lane-close | up | derived | [AR §5a.9] `lane close`; [API §11.5]; [OP-8] | TR-061's transition through the verb: `lane close` after the lane's merge into `main`. |
| TR-075 | lane | `active` | `abandoned` | lane-close | to-side | derived | [AR §5a.9] `lane close`; [API §11.5]; [OP-8] | `lane close` of a lane `main` has not absorbed; TR-065's transition through the verb. |
| TR-076 | lane | `ready_to_merge` | `abandoned` | lane-close | to-side | proposed | [AR §5a.9] `lane close`; [API §11.5]; [OP-8] | As TR-075. |
| TR-077 | lane | `merge_pending` | `abandoned` | lane-close | to-side | proposed | [AR §5a.9] `lane close`; [API §11.5]; [OP-8] | As TR-075. |
| TR-078 | lane | `active` | `frozen` | lane-close | to-side | derived | [AR §5a.9] `lane freeze`; [API §11.5]; [OP-8] | `lane freeze`; TR-062's transition through the verb. |
| TR-079 | lane | `ready_to_merge` | `frozen` | lane-close | to-side | proposed | [AR §5a.9] `lane freeze`; [API §11.5]; [OP-8] | As TR-078. |
| TR-080 | lane | `merge_pending` | `frozen` | lane-close | to-side | proposed | [AR §5a.9] `lane freeze`; [API §11.5]; [OP-8] | As TR-078. |
| TR-071 | area | `active` | `archived` | set-status | to-side | design | [AR §3.2] | Root nodes of R4 are never archived by a verb ([40 §2.4]); the row applies to scope areas. |

## 7. Guards

A guard is evaluated on the candidate state as left by the statements before it in the same `TX` (GR-007). A failed
guard refuses the whole block; nothing is written.

<!-- table: guards -->
| row | guard | refusal | exit | basis | source | definition |
|---|---|---|---|---|---|---|
| GD-001 | no-unfinished-child | E404 | 6 | design | [AR §3.6]; [50 §3.10] item 5; [50 §5.2] E404 | No live node whose `parent` is the target is a task with a status outside {`done`, `cancelled`}. [AR §3.6] says "while any child is open"; "open" is read as "unfinished" ([OP-10]). The refusal names the children. |
| GD-002 | not-gated | E404 | 6 | design | [AR §3.3] `gates`; [AR §3.6]; [AR §6.2]; [AR §3.3] X5 | No `gates` in-edge of the target has a live source verdict with status `open` and outcome `fail_fixable` or `fail_fundamental`, and no `gates` in-edge is flagged ([RULES/delete-policy-matrix] FL-002). The refusal names the gating verdict ("exit 6 with the gating verdict named"). `gates` never constrains `claim` or `ready` ([RULES/state-definition] BT-006). |
| GD-003 | answers-edge | E404 | 6 | design | [AR §3.6]; [AR §3.5] `answered`; [AR §2.17] S2-B | The candidate holds a live `answers` in-edge of the question from a live `decision` or `note`. |
| GD-004 | answer-text | E404 | 6 | design | [AR §3.6] "with an `answers` edge and an answer" | The question's `answer` field is non-empty in the candidate. |
| GD-005 | i13-review | E404 | 6 | design | [AR §3.4] I13; [OP-11] | Applies only when the finding's `f_kind` is `perf` or `complexity`. The finding has a live `verifies` in-edge from a live verdict whose `role` is `code-reviewer` or `architecture-critic`, and a live `addresses` in-edge created by an actor other than that verdict's creator. |
| GD-006 | i14-artifacts | E404 | 6 | design | [AR §3.4] I14; [OP-12] | Every symbol of the run's `expected_artifacts` names an artifact that the run has a live `produced` edge to and whose `oid` was read back after the run started. |
| GD-007 | planned-bind | skip | - | design | [40 §3.2] | The settle turns `planned` into `present` only in a writer tree whose HEAD descends from the planning commit (the `observed_git` of the planned composite), or when the file's creation time is later than that commit's time. Otherwise the settle writes nothing for the node; this is not an error. |

<!-- table: transition-guards -->
| row | kind | from | to | guard | basis | source | note |
|---|---|---|---|---|---|---|---|
| TG-001 | task | `in_progress` | `done` | no-unfinished-child | design | [AR §3.6] | Every door: `set --done`, `SET t.done = true`, `complete`. |
| TG-002 | task | `in_progress` | `done` | not-gated | design | [AR §3.6]; [AR §3.3] | As TG-001. |
| TG-003 | task | `open` | `done` | no-unfinished-child | derived | [AR §3.6]; [AR §6.2] | The compound transition carries the guards of its second step. |
| TG-004 | task | `open` | `done` | not-gated | derived | [AR §3.6]; [AR §6.2] | As TG-003. |
| TG-005 | question | `open` | `answered` | answers-edge | design | [AR §3.6] | - |
| TG-006 | question | `open` | `answered` | answer-text | design | [AR §3.6] | - |
| TG-007 | finding | `confirmed` | `fixed` | i13-review | design | [AR §3.4] I13 | - |
| TG-008 | run | `running` | `green` | i14-artifacts | design | [AR §3.4] I14 | - |
| TG-009 | artifact | `planned` | `present` | planned-bind | design | [40 §3.2] | - |

## 8. Who may use a door

Who may perform a transition is decided by [RULES/role-write-policy], the single source for roles, scopes (`own-role`,
`leased-task`, `any`) and rights; this file does not restate a grant. `door-roles` names, for each door, the rows that
decide it. The two files meet in one rule (GR-002): a role row allows a transition only through a door that
`transitions` lists for it, and a `transitions` row is usable only by a role the named rows allow. The model checks at
load that every row id cited here exists.

<!-- table: door-roles -->
| row | door | realized_by | basis | source | note |
|---|---|---|---|---|---|
| DG-001 | set-status | WS-001, WS-002, WS-007, WS-008, WS-009, WS-010, WV-018, WV-023 | design | [AR §7.3]; [50 §6.5]; [RULES/role-write-policy] `role-status` | Orchestrator and owner: every transition (WS-001, WS-002); critics and reviewers: `open → withdrawn` on findings of their own role; refuters: `open → confirmed` or `refuted`. `answer` is the owner's (WV-018); `run close` (WV-023) is the orchestrator's and owner's; `lane close` and `lane freeze` are DG-013's (spec sync 2b). The architect changes no status ([RULES/role-write-policy] OP-24). |
| DG-002 | tx-complete | WS-004, WS-006, WX-008, WM-009 | design | [AR §6.2]; [50 §6.5] | Developer and tester on their leased task, orchestrator and owner on any; the lease presented must be the task's live lease. |
| DG-003 | claim-start | WS-003, WS-005, WM-001, WM-002 | design | [AR §6.2]; [90 §4.3] | Self-claim roles on their own claim; the orchestrator's bulk claims. |
| DG-004 | lease-first-write | WS-003, WF-007 | design | [AR §6.2] | The first `set` of the leased task under its lease; WS-003's note covers the implicit start. |
| DG-005 | reopen | WX-006 | proposed | [AR §3.6]; [RULES/role-write-policy] OP-27 | Orchestrator and owner only: no role row grants `REOPEN`. |
| DG-006 | supersede | WE-001, WE-002, WS-001, WS-002 | derived | [AR §3.4] I6; [RULES/role-write-policy] `role-edges` | Creating the `SUPERSEDES` edge needs an edge grant; only the orchestrator and owner hold one. |
| DG-007 | retract | WS-001, WS-002, WV-019 | derived | [AR §7.1] `retract`; [RULES/role-write-policy] `role-status` | A status change no non-orchestrator row grants. |
| DG-008 | settle | WV-036, WR-013 | design | [AR §7.3] "`links sync` for every role"; [40 §6.3] | Every role, `general-purpose` included; settles also run in hooks without a lease. |
| DG-009 | file-rm | WV-031 | proposed | [AR §7.3] R4 rows; [RULES/role-write-policy] OP-9 | Orchestrator, owner, developer, tester, doc-writer; CLI only, in the writer tree. |
| DG-010 | links-fix | WV-037 | design | [AR §7.3] R4 rows; [40 §6.3] | Orchestrator, owner, developer, tester, and the architect on doc files only. |
| DG-011 | deletion-inference | WV-036, WR-013 | design | [40 §2.10] I-F7; [AR §13] `files.deletion-inference` | Part of a settle. |
| DG-012 | delete-policy | WV-016, WX-001 | design | [AR §7.3] "Node `DELETE` … orchestrator/owner only"; [RULES/delete-policy-matrix DP-001] | The policy op belongs to the node delete that writes it. |
| DG-013 | lane-close | WV-004, WS-001, WS-002 | design | [AR §7.3]; [RULES/role-write-policy] `role-verbs` | `lane close` and `lane freeze` are the orchestrator's and owner's (WV-004), whose `role-status` rows allow every transition through any door (spec sync 2b). |

## 9. Views that accept status writes

<!-- table: branch-mask -->
| row | view | status_writes | refusal | exit | basis | source | note |
|---|---|---|---|---|---|---|---|
| BM-001 | work | yes | - | - | design | [AR §5a.1] | `main` and `lane/*`. |
| BM-002 | plan | no | E305 | 6 | design | [AR §3.4] I33′; [AR §5a.1]; [50 §3.9] item 6; [50 §5.2] E305 | "`status`, `resolution`, `assignee`, leases/claims read-only"; so every door is refused, `claim-start`, `tx-complete`, `settle` and `file-rm` included. |
| BM-003 | merge | no | E305 | 6 | design | [50 §3.9] item 6; [AR §5a.7] step 8 | A staging ref accepts only `RESOLVE`; a `StatusFork` is settled there by `resolve`, not by a transition. |
| BM-004 | import | no | E305 | 6 | design | [50 §3.9] item 6 | - |
| BM-005 | tag | no | E305 | 6 | design | [AR §5a.1] "`tag` — immutable" | - |
| BM-006 | past-view | no | E305 | 6 | design | [50 §3.9] item 6 | A commit, `s…`, `~n`, `@n` or `@time`. |

## 10. Derived-state effects

What a status change touches besides the status itself. The model recomputes every derived predicate from scratch
([60 §4.2]); these rows name the subjects whose value can change, which is what `affected` must list (GR-015) and
what WP-94's suites check. Subjects: `self`; `blocks-dst` (the live targets of the node's `blocks` out-edges);
`blocks-dst-subtree` (those targets and their descendants); `blocks-src` (the sources of the node's `blocks`
in-edges); `gates-dst` (the targets of the verdict's `gates` out-edges); `parent`; `derivation-src` (the sources of the
node's live `derived_from`, `cites`, `implements` and `depends_on` in-edges); `at-src` (the sources of the file
node's `at` in-edges); `store`.

<!-- table: derived-effects -->
| row | kind | from | to | predicate | subject | basis | source | note |
|---|---|---|---|---|---|---|---|---|
| DE-001 | task | * | * | hold | self | design | [AR §3.4] I26′; [RULES/state-definition HV-001] | Whenever the status enters, leaves or moves within {`done`, `cancelled`}, the node's hold on the branch changes; the marker cache follows [RULES/state-definition] ME rows. |
| DE-002 | task | * | * | unblocked | self | design | [AR §3.5] | `status = open` is a clause. |
| DE-003 | task | * | * | blocked | self | design | [AR §3.5] | `unfinished` is a clause. |
| DE-004 | task | * | * | is_blocker | self | design | [AR §3.5] | "not done ∧ has an outgoing `blocks` edge to a not-done task". |
| DE-005 | task | @unfinished | @finished | open_blockers | blocks-dst | design | [AR §3.5] | One fewer open blocker on each target. |
| DE-006 | task | @finished | @unfinished | open_blockers | blocks-dst | design | [AR §3.5] | One more. |
| DE-007 | task | @unfinished | @finished | open_blockers_exo | blocks-dst | derived | [AR §3.5] | For each target whose subtree does not contain the task. |
| DE-008 | task | @finished | @unfinished | open_blockers_exo | blocks-dst | derived | [AR §3.5] | As DE-007. |
| DE-009 | task | * | * | unblocked | blocks-dst-subtree | derived | [AR §3.5]; [AR §3.4] I5′ | Descendants inherit exogenous blockers ("no ancestor with `open_blockers_exo > 0`"). |
| DE-010 | task | * | * | is_blocker | blocks-src | design | [AR §3.5] | A source stops being a blocker when its last unfinished target finishes. |
| DE-011 | task | @unfinished | @finished | children_done | parent | design | [AR §3.5] | - |
| DE-012 | task | @finished | @unfinished | children_done | parent | design | [AR §3.5] | - |
| DE-013 | task | * | * | ready_to_close | parent | design | [AR §3.5] | "container with all children done". |
| DE-014 | question | * | * | - | - | derived | [AR §3.5] `answered`; [RULES/merge-table] OP-4 | `done` of a question follows its `answers` edges, not its status; a status change alone changes no blocker count. |
| DE-015 | verdict | `open` | `accepted` | gated | gates-dst | derived | [AR §3.3] `gates`; [AR §3.6] | If the verdict's outcome is `fail_fixable` or `fail_fundamental`, each gated task loses one gating verdict (GD-002). |
| DE-016 | verdict | `open` | `superseded` | gated | gates-dst | derived | [AR §3.3] `gates` | As DE-015. |
| DE-017 | doc | * | `superseded` | suspect | derivation-src | design | [AR §3.5] `suspect` | - |
| DE-018 | note | * | `superseded` | suspect | derivation-src | design | [AR §3.5] | - |
| DE-019 | note | * | `retracted` | suspect | derivation-src | design | [AR §3.5] | - |
| DE-020 | rule | * | `superseded` | suspect | derivation-src | design | [AR §3.5] | - |
| DE-021 | rule | * | `retracted` | suspect | derivation-src | design | [AR §3.5] | - |
| DE-022 | decision | * | `superseded` | suspect | derivation-src | design | [AR §3.5]; [AR §3.3] `implements` "src `suspect` if the target is superseded" | - |
| DE-023 | verdict | * | `superseded` | suspect | derivation-src | design | [AR §3.5] | - |
| DE-024 | measurement | * | `retracted` | suspect | derivation-src | design | [AR §3.5] | - |
| DE-025 | artifact | * | `removed` | suspect | at-src | design | [40 §2.9]; [AR §3.5] | "a file node that is `removed` … makes its referrers `suspect`". |
| DE-026 | artifact | `removed` | * | suspect | at-src | design | [40 §2.9]; [AR §3.5] | `links fix --restore` clears it. |
| DE-027 | lane | * | `measuring` | quiet | store | design | [AR §6.6]; [F17 §5.3] | "any lane with status `measuring` implies it (the explicit flag wins)", while `quiet.from-lane-measuring` is true ([CFG]); [F17 §5.3] lists the other sources of quiet mode (the flag and a held quiet byte of `LOCK`). |
| DE-028 | lane | `measuring` | * | quiet | store | design | [AR §6.6] | - |
| DE-029 | finding | * | * | - | - | derived | [AR §3.5] review-loop termination, refuted share | These are queries over statuses; no maintained predicate changes. |

## 11. `complete` outcomes

<!-- table: complete-outcomes -->
| row | outcome | status | lease | hold | basis | source | note |
|---|---|---|---|---|---|---|---|
| CO-001 | done | `done` | released-settled | done | design | [AR §6.2]; [50 §4.2]; [API §10.5] | "writes `done` on the lease's branch, releases the lease into `settled`". Resolution `completed` ([API §10.5] step 1). |
| CO-002 | failed | `done` | released-settled | done | proposed | [AR §6.2]; [API §10.5]; [LQ/std §7.3]; [OP-16] | The literal reading: `complete` writes `done` for every outcome, here with resolution `rework`; the outcome travels in the `settled` marker ([F11 §7] `outcome` 2). Confirmed by WP-25 ([API §10.5] step 1) and [LQ/std §7.3] `tx.complete` (review pass 1, A1-36). |
| CO-003 | abandoned | `done` | released-settled | done | proposed | [AR §6.2]; [API §10.5]; [LQ/std §7.3]; [OP-16] | As CO-002, with resolution `wontdo` and `outcome` 3. |

## 12. General rules

<!-- table: general-rules -->
| row | rule | applies_to | refusal | exit | basis | source | note |
|---|---|---|---|---|---|---|---|
| GR-001 | no-transition | status-write | E404 | 6 | design | [AR §3.4] I8; [50 §3.10] item 5 | A status write on a core kind whose (kind, from, to, door) matches no `transitions` row is refused; the message names the door that exists, for example `REOPEN` for `done → open`. A project kind has no `transitions` rows; GR-018 decides its status writes (spec sync 2b). |
| GR-002 | role-policy | status-write | E406 | 6 | design | [AR §7.3]; [50 §6.5]; [RULES/role-write-policy WR-009] | Whether the caller's effective role may make the transition is decided by [RULES/role-write-policy] (`door-roles` names the rows). A role row is usable only through a door that `transitions` lists for the transition. |
| GR-005 | target-live | status-write | not_found | 3 | design | [AR §7.1] exit 3; [F19 §10.2] `not_found`; [API §9.1] | A status write on a deleted or absent node is "not found": [F19 §10.2] `not_found` with `what` = `node`, and the tombstone is printed ([RULES/delete-policy-matrix] DP-003). |
| GR-006 | create-status | create | E404 | 6 | proposed | [50 §3.10] item 6; [OP-17] | A `Create` takes an initial status. A `Create` that names another status is checked as a path of `transitions` rows from an initial status to it, every step with its guards and a role grant; `CREATE (x:artifact …)` stays E115 ([50 §3.10]). |
| GR-007 | per-statement | tx | - | - | design | [50 §3.10] items 2, 5; [AR §4.3] coalescing | Statements are checked in order on the candidate; a guard sees the effects of earlier statements. The stored op is the net `SetStatus` per node, so `TX { REOPEN t; SET t.done = true }` on a done task stores no status op; its net changeset holds only GR-011's `reopen_count` increment. |
| GR-008 | non-door | merge-history-import | - | - | design | [AR §5a.5]; [AR §5a.7]; [AR §5b.6]; [AR §3.4] I8 | Merge, `sync`, revert, cherry-pick, `undo`, `op restore` and import are not checked against `transitions` or the role write policy's status rows; I8 is checked on the resulting state: the status is one of the kind's `statuses` or a conflict value. |
| GR-009 | done-true | set-status | E115 | 2 | design | [AR §3.1]; [50 §3.8]; [50 §5.2] E115 | `SET x.done = true` is the transition to `done` (task), `answered` (question) or `accepted` (verdict); on a kind whose `done` is `absent` it is E115. |
| GR-010 | done-false | set-status | E404 | 6 | design | [AR §3.6]; [50 §3.10] item 6 | `SET x.done = false` is refused, naming `REOPEN`. |
| GR-011 | reopen-count | reopen | - | - | design | [AR §3.6]; [50 §3.10] item 6 | `REOPEN` on a task also writes `Incr(reopen_count, 1)` in the same statement. |
| GR-012 | artifact-set | set-status | E115 | 2 | design | [50 §3.10] item 6; [40 §2.2] | `SET` of an artifact's status is E115 naming `moirai file rm` or `links fix --drop`; an artifact's status changes only through the doors `settle`, `file-rm`, `links-fix` and `deletion-inference`. |
| GR-013 | derived-set | set-status | E115 | 2 | design | [AR §3.4] I8; [50 §5.2] E115 | `blocked`, `ready`, `unblocked`, `stale`, `claimed`, `container`, `answered`, `conflicted` and `suspect` are never written. |
| GR-014 | resolution | status-write | E404 | 6 | proposed | [AR §3.1]; [RULES/merge-table] OP-3; [OP-18] | `resolution` may be written only with a transition into a status whose `done` is `yes` (task) or out of `open` (finding); a transition to `open` clears it. A task transition into `done` that names no resolution writes `completed`, whatever the door: `set --done`, `SET t.done = true`, `SET t.status = 'done'` and `complete` for outcome `done` (CO-001) alike; any other transition that names none writes `none` (spec sync 2b). |
| GR-015 | affected | status-write | - | - | design | [AR §3.4] I42′; [50 §8.1] F15 | Every `derived-effects` subject whose predicate value changed is in the commit's `affected` list, or the commit carries `affected_complete = 0`. |
| GR-016 | phase-state | set-field | - | - | proposed | [AR §3.6]; [OP-19] | `phase_state` is an ordinary field under the role policy; no transition table applies. |
| GR-017 | lattice-check | load | - | - | derived | [RULES/merge-table] SL rows; [AR §3.2]; [AR §3.6] | At load the model checks that the `statuses` rows equal the SL rows kind by kind, and that the transitive closure of the `transitions` rows with `move = up` equals the SL order. |
| GR-018 | project-kind | status-write | - | - | proposed | [F08 §8.5.1]; [AR §3.6]; [OP-25] | A node of a project kind ([F08 §8.5.1]) may move from any of its kind's statuses to any other through the door `set-status`, unguarded: the schema gives a project kind statuses, not transitions, so no `transitions`, `transition-guards` or `derived-effects` row applies. The value must be one of the kind's non-retired statuses ([F08 §8.6] item 1); the role policy decides who (WS rows with kind `*`), and `branch-mask` where (spec sync 2b). |

## Coverage

These tables specify semantics, not bytes. The byte layouts they rely on are [F08]'s (`NodeHdr.status`, the status
and resolution codes, `CREATOR`), [F06]'s (`SetStatus`, `Incr`) and [F19]'s (E115, E305, E404, E406, `not_found`).

Model functions are named as `COVERAGE.md` names them (`xtask coverage` resolves them so): the module of the source
file in `crates/moirai-model/src/`, then the function (spec sync 2b).

| Checklist row | Covered by | Fixture | Model function |
|---|---|---|---|
| [60 §2.5] "Schema as data": each kind's status set (the field's lattice values) and its initial values | `statuses`, `status-fields`, GR-017 | WP-94 suite `status-machines` | `status::statuses` |
| [40 §2.11] R-2: status values `planned`, `removed` | ST-039 to ST-041, TR-049 to TR-054, TR-081 | WP-94 suite `status-machines` | `status::transition` |
| [40 §2.11] R-12: I-F7 (`removed` never inferred) and I-F14 (only explicit doors bring a node back), status part | TR-050 to TR-054, TR-081, DR-009 to DR-011 | WP-94; FL-3 at M2 | `status::transition` |
| [90 §10.1] `LEASES.kind` and `role`: rights come from the presented lease | delegated: `door-roles`, GR-002, and [RULES/role-write-policy] | WP-94 | `policy::status` |
| [50 §8.1] F15: derived-predicate changes reach `affected` | `derived-effects`, GR-015 | WP-94 | `derived::affected`, `derived::affected_with_budget` |
| [AR §3.4] I8, I13, I14, I33′ | GR-001, GR-008, GD-005, GD-006, BM-002 | WP-94 | `status::transition`, `status::i8_status_machine`, `status::gd005_i13_review`, `status::gd006_i14_artifacts`, `status::branch_mask` |

No X-F row concerns status machines.

## Holes

None. No transition, guard or grant depends on a value an M0 measurement decides.

## Open points for the review

1. **File name and registry.** The task that commissioned this file names it `status-machines.md`, as [RULES/README]
   §1.1 does. Its tables are registered in [RULES/README] §7 as RG-085 to RG-095 (review pass 1 S1-47), with these
   rows:

   ```
   | RG-0xx | `status-fields` | status-machines.md | decision | SF | row:id, kind:token, field:token, stored:enum(yes/no), guarded:enum(yes/no), basis:enum, source:cite, note:text | - |
   | RG-0xx | `statuses` | status-machines.md | decision | ST | row:id, kind:token, status:token, initial:enum(yes/no), done:enum(yes/no/derived/absent), lattice:token, basis:enum, source:cite, note:text | - |
   | RG-0xx | `doors` | status-machines.md | vocabulary | DR | row:id, door:token, requires:tokens, basis:enum, source:cite, note:text | - |
   | RG-0xx | `transitions` | status-machines.md | decision | TR | row:id, kind:token, from:token, to:token, door:token, move:enum(up/down/to-side/from-side), basis:enum, source:cite, note:text | - |
   | RG-0xx | `guards` | status-machines.md | vocabulary | GD | row:id, guard:token, refusal:token, exit:token, basis:enum, source:cite, definition:text | - |
   | RG-0xx | `transition-guards` | status-machines.md | decision | TG | row:id, kind:token, from:token, to:token, guard:token, basis:enum, source:cite, note:text | - |
   | RG-0xx | `door-roles` | status-machines.md | map | DG | row:id, door:token, realized_by:tokens, basis:enum, source:cite, note:text | - |
   | RG-0xx | `branch-mask` | status-machines.md | decision | BM | row:id, view:token, status_writes:enum(yes/no), refusal:token, exit:token, basis:enum, source:cite, note:text | - |
   | RG-0xx | `derived-effects` | status-machines.md | procedure | DE | row:id, kind:token, from:token, to:token, predicate:token, subject:token, basis:enum, source:cite, note:text | - |
   | RG-0xx | `complete-outcomes` | status-machines.md | decision | CO | row:id, outcome:token, status:token, lease:token, hold:token, basis:enum, source:cite, note:text | - |
   | RG-0xx | `general-rules` | status-machines.md | procedure | GR | row:id, rule:token, applies_to:token, refusal:token, exit:token, basis:enum, source:cite, note:text | - |
   ```

2. **Status codes.** The integer code of every status and resolution is [F08]'s (WP-14); enum integers are never reused
   (I11). This file names statuses only by symbol, so WP-14's codes cannot conflict with it.
3. **`cancelled → open`** (TR-016). [AR §3.6] names no exit from `cancelled`, yet [AR §4.5] step 4 writes `cleared` for
   `SetStatus{done or cancelled → open}`, and the virtual `done` covers both. Proposed: `REOPEN` also reopens a
   cancelled task.
4. **`frozen → open`** (TR-017). Without it a frozen task could never be worked again; proposed as the unfreeze.
5. **`in_progress → open`** (TR-018). A started task whose holder died stays `in_progress` after `reclaim` or expiry,
   and `ready` requires `status = open` ([AR §3.5]), so it could never be dispatched again: the only route back would be
   `deferred` and then `open`. Proposed: a direct `set-status` back to `open`, which [RULES/role-write-policy] WS-001 grants the orchestrator.
6. **`decision proposed → rejected`** (TR-033). The status set has `rejected` but [AR §3.6] names no source; `proposed`
   is the only non-final state.
7. **`artifact planned → removed`** (TR-052). `links fix --drop` of a planned link is not described in [40 §3.7];
   proposed so that a planned file that will never exist can be dropped.
8. **Lane side states** (TR-061 to TR-080, DR-013). [AR §3.6] lists `frozen`, `abandoned` and `measuring` without
   sources or exits. Proposed: `frozen` and `abandoned` from each forward state except `merged`; `measuring` only from
   `active` and back to `active`; `frozen` back to `active`. `lane close` is read as `merged` after the lane's merge
   into `main` and `abandoned` otherwise; WP-25 (Store API) fixes the verb's expansion. **Spec sync 2b** (GT10 finding
   F-7): `LaneClose` ([API §11.5]) closes a lane that is still `active` ([AR §7.6] step 8), while only
   `merge_pending → merged` was a transition, so `lane close` and `lane freeze` now have their own door `lane-close`
   (DR-013, DG-013) with transitions from every non-side state to `merged` (when `main` has absorbed the lane's tip),
   `abandoned` and `frozen` (TR-072 to TR-080). A `frozen` or `measuring` lane is first returned to `active` (TR-069,
   TR-070). The `set-status` rows TR-061 to TR-067 stay for direct status writes.
9. **Where a retraction's reason lives** (DR-007). `retract ID --reason T` needs a carrier: a field, or the commit
   message. [F08] and WP-19 decide; the door only requires that it is non-empty.
10. **"While any child is open"** (GD-001, TR-008, TR-011). Read as "unfinished", so an `in_progress`, `deferred` or
    `frozen` child also blocks completion. Cancelling or deferring a task with unfinished children is not guarded; the
    design is silent.
11. **I13's "different actor" and "review verdict"** (GD-005). Read as: the verdict of the `verifies` edge has role
    `code-reviewer` or `architecture-critic`, and the `addresses` edge was created by an actor other than that
    verdict's creator. **Decided** 2026-09-28 (owner question OQ-M-2, option (a)): this reading, not the actor of the
    commit that moves the finding to `fixed` ([F13] OP-13-09, now closed citing GD-005) and not "different from the
    finding's author".
12. **I14's "read back"** (GD-006). Read as: the artifact's `oid` was observed after the run started. How the engine
    records that observation is [40]'s runtime; the model checks it against the simulated tree.
13. **Role rights live in one table** (`door-roles`, GR-002). [RULES/role-write-policy] was written beside this file and
    decides, for every write, which role may make it; its `role-status` rows are the only grants of status transitions.
    This file therefore names doors and cites those rows instead of restating grants, so the two cannot drift. Three
    points of that file shape the status machine and are confirmed there, not here: "own" is `CREATOR.role` equal to
    the lease's role (its WT-004 and Open point 2); the architect changes no status, reading [50 §6.5]'s "fields of own
    docs, decisions and questions" without `status` (its Open point 24), so accepting a decision or making a plan
    `current` stays with the orchestrator and owner; and `file rm` belongs to the orchestrator, owner, developer,
    tester and doc-writer, `links fix` to the architect only on doc files (its WV-031, WV-037 and Open point 9).
14. **`lease-first-write` as a door** (DR-004, DG-004). [AR §6.2]'s "the first `set`/`complete` under the lease performs
    it" is a separate door because it changes the status without a status statement; [RULES/role-write-policy] WS-003
    covers it in a note. The review confirms that the implicit start needs no row of its own there.
15. **Door-to-verb mapping.** Each door's spellings are in its `doors` note; [RULES/role-write-policy] `role-verbs` and
    `role-statements` classify the same verbs. A verb that appears under two doors (none does today) would be a
    specification finding.
16. **`complete --outcome failed` and `abandoned`** (CO-002, CO-003). [AR §6.2] and [50 §4.2] write `done` for every
    outcome, and the `settled` marker records the outcome. Consequence: a failed task is excluded from dispatch on
    every branch until someone reopens it, which is safe against double dispatch. The alternative (failed and
    abandoned return the task to `open`) would need a transition the design does not state. [RULES/role-write-policy]
    WS-004 leaves the outcomes to [API] (WP-25); CO-002 and CO-003 are the rows WP-25 confirms or overturns. **Confirmed**
    (review pass 1, A1-36): [API §10.5] step 1 and [LQ/std §7.3] write `done` for every outcome, with resolution
    `completed`, `rework` or `wontdo` for `done`, `failed` or `abandoned`; the CO notes name them.
17. **Creating a node in a non-initial status** (GR-006). Proposed: allowed as a checked path of transitions, so
    `remember` can create an accepted decision only for a role that may accept it.
18. **Resolution** (GR-014). The design lists the resolution values but not which statuses carry them. Proposed as in
    GR-014; together with [RULES/merge-table] OP-3. Spec sync 2b (GT10 gap G-4): a task that reaches `done` by
    `set --done` or `SET`, not `complete`, gets resolution `completed` unless the write names one, as `complete` does for
    outcome `done` ([API §10.5] step 1), so the resolution does not depend on the door.
19. **`phase_state`** (SF-006, GR-016). "The 14 states of [01 §5.1]" do not match that report's diagram, which draws
    more nodes; the value set is [F08]'s. The design says it is "advanced by verdicts" without saying which write does
    it; no machine is imposed until the owner states one.
20. **Question `answered`** (ST-024 to ST-026, TR-035, TR-039). The status is stored and moved by guarded writes (GD-003
    keeps it aligned with the edge at write time), but `done` follows the edge-derived predicate, which S2-B requires.
    After a merge the stored status and the edge can disagree; `done` then follows the edge. Consistent with
    [RULES/merge-table] OP-4, which leaves to WP-14 whether `answered` is stored at all.
21. **`done` on the other kinds.** [AR §3.1] gives `done` for tasks, questions and verdicts only. For the other ten
    kinds `done` is `absent` (an LQ absent value), and writing it is E115.
22. **`gates` and readiness.** [AR §3.5] counts `gates` in-edges in `open_blockers`, but X5 and [AR §6.2] say `gates`
    constrains completion only, never `claim`. This file keeps `gates` in the completion guard (GD-002) and
    [RULES/state-definition] BT-006 keeps it out of `open_blockers`. Review pass 1 (S1-30) adopted this reading for
    [F13 §6.2] and [F08 §3.4]; [AR §3.5] should be edited at WP-81a.
23. **Verdict `superseded`** (TR-046). [AR §3.6] ties the `supersedes` edge to knowledge (note, rule, decision, doc)
    only, and [50 §2.5] gives `SUPERSEDES` the endpoints "knowledge → same kind"; so a verdict moves to `superseded` by
    `set-status` with no edge.
24. **Header flags `frozen` and `archived`.** [AR §3.1] has flag bits of those names beside the statuses `frozen` and
    `archived`, and [RULES/merge-table] FC-013 and FC-014 treat them as source-truth flags. This file governs the
    `status` column only; whether the flags mirror the statuses or are separate facts is [F08]'s.
25. **Status writes on project kinds** (GR-001, GR-018; spec sync 2b, WP-90a). A project kind ([F08 §8.5.1]) has
    statuses in the schema but no `transitions` rows, so GR-001 read alone refused every status change on it. Proposed
    (GR-018): any status of the kind to any other through `set-status`, with no guard, the role policy and
    `branch-mask` applying as to a core kind. The alternative, transitions as schema data, needs a schema item class
    [F08] does not have; the review decides whether a project needs it. A project kind's initial status is
    [F08 §8.5.1]'s: its non-retired status with the least `sort_rank`.
26. **The settle's path-claim unification** (TR-081; WP-92). [RULES/link-merge-rules] LV-005 and [40 §2.3] ("settle
    performs the unification itself", evidence `git/r100`) make a settle write `removed{reason: same-as}`, but
    [F18 §2.7] I-F7 lists only `file rm`, `links fix --drop`, `--same-as`, `--split` and deletion inference, and no
    `transitions` row let the door `settle` reach `removed`. TR-081 adds that transition as the automatic form of
    `--same-as`; [F18 §2.7] should list it (a spec finding of WP-92).
