# State definition: I26′, `ready` and `unblocked`, markers, absorbed vectors and leases

| Field | Value |
|---|---|
| Status | draft, pass 1 pending |
| Work package | WP-90 (R-MODEL); consumed by WP-93b (the runtime properties of [50 §3.8]), WP-94 (GT18's I26′ state oracle, lease liveness) and M1–M3 (the engine's `MARKERS`, `LEASES` and absorbed vectors) |
| Sources | [AR §3.4] I17′, I26′, I32′, I36′, I42′; [AR §3.5] `unblocked`, `ready`, `blocked`, `is_blocker`, `settled_elsewhere`, `deleted_elsewhere`; [AR §4.3] `Marker` records and flushed groups; [AR §4.4] `LEASES`, `MARKERS`, `MARKERS_OLD`; [AR §4.5] step 4 (markers from net ops), step 6 (the `MARKERS` fold); [AR §5a.1] ref kinds; [AR §5a.2] `ref_seq`; [AR §5a.3] forks; [AR §5a.5] `undo`, `op restore`; [AR §5a.7] step 7; [AR §5a.9] `branch -d`, `-D`; [AR §5d.1] runtime state; [AR §5d.2]; [AR §5d.3]; [AR §6.2] leases and liveness; [AR §7.6] steps 1, 6–8; [50 §3.6] "Deleted elsewhere"; [50 §3.8]; [50 §3.9] items 4, 6, 7; [50 §4.1] `std.ready`, `std.blocking`; [60 §2.5] "Derived-state semantics", `MARKERS` key, `LEASES`; [60 §3.13] GT18; [60 §4.2] "Cross-branch exclusion (I26′)"; [72 M4]; [72 B2]; [80 §2.7.1], [80 §2.7.2], [80 §3] X-F2; [90 §4.1], [90 §4.3], [90 §4.4], [90 §10.1] |
| Format | [RULES/README] |
| Cited as | [RULES/state-definition]; a row as [RULES/state-definition PD-012] |

## 1. What this table decides

- **I26′ as a function of states** ([AR §3.4], [72 M4]): when a task is kept out of `ready` and `claim` on a branch,
  and never listed there as a live blocker, because another live branch holds it done, cancelled or deleted
  (`view-kinds`, `hold-values`, `origin-rules`, `predicates` PD-012 to PD-014).
- **`unblocked` and `ready`**: the structural predicate valid at any view, and the dispatch predicate valid only at a
  tip, clause by clause (`predicates`, `validity`, `blocker-terms`).
- **Leases**: when a lease is live, what ends it, and what a live lease does to `ready`, `claim` and `rm`
  (`lease-live`, `lease-ends`, `lease-effects`).
- **The marker cache**: which `settled`, `deleted` and `cleared` records the engine keeps, when they are written,
  when a marker counts, and how a reader tests it in O(1) (`marker-fields`, `marker-events`, `absorption`,
  `vector-rules`), with the argument that the cache equals the definition.
- **Coverage of GT18's doors and ref moves** (`door-coverage`) and worked cases (`scenarios`, `scenario-expect`).

The reference model evaluates the definition itself, over all live refs, never from markers ([60 §4.2]). It also
maintains the marker cache by these rules and compares the two after every command ([m0/PLAN §6.2] R12), so a flaw in
a cache rule shows up as a disagreement inside the model before any engine exists.

## 2. How to read the tables

The format is [RULES/README]. Notation used in the notes:

- A **ref** has a kind (`work`, `plan`, `merge`, `import`, `tag`, [AR §5a.1]) and a tip commit. A ref is **live**
  while it exists in the current ref set; `branch -d`, `branch -D`, `merge --abort` and the removal of a landed staging
  ref end it.
- A commit c **lands** on exactly one ref, `ref(c)`, at position `ref_seq(c)` on that ref's chain; positions are never
  reused ([AR §5a.2]). `anc*(c)` is the set of ancestors of c, c included. `state_at(c)` is the materialised state of c
  ([60 §4.2]).
- `p1(c)` and `p2(c)` are a commit's parents in stated order: for a merge, the destination's tip and the source's tip;
  for a `sync`, the lane's tip and `sync_base` ([AR §4.6] item 2).
- **S** is the set of closed hold values {`done`, `cancelled`, `deleted`}.
- `#N` is the node in question; "the caller" is the actor resolved by [90 §4.1]'s Actor row.

## 3. Views and holds

<!-- table: view-kinds -->
| row | ref_kind | holds_count | tip_reads | basis | source | note |
|---|---|---|---|---|---|---|
| VK-001 | work | yes | yes | design | [AR §3.4] I26′ "some live ref X ≠ R of kind `work`"; [AR §5a.1] | `main` and `lane/*`. |
| VK-002 | plan | no | yes | design | [AR §3.4] I26′; [AR §2.17] X13 | A what-if branch never blocks dispatch elsewhere; `ready` may be read at its tip. |
| VK-003 | merge | no | yes | design | [AR §3.4] I26′ "staging refs produce none"; [72 M4] fix 2 | - |
| VK-004 | import | no | yes | derived | [AR §5a.1]; [72 M4] fix 2 | A staging ref of the importer, like VK-003. |
| VK-005 | tag | no | no | proposed | [AR §5a.1]; [50 §3.9] items 4, 6; [OP-11] | A tag is immutable and not a branch tip: `ready` there is E302. |
| VK-006 | past-view | no | no | design | [50 §3.8]; [50 §3.9] item 4; [AR §3.5] `ready` | A commit, `s…`, `~n`, `@n` or `@time`: `ready` is E302 with `unblocked` as the fix. |
| VK-007 | orphans | no | yes | derived | [AR §3.4] I26′ "some live ref X ≠ R of kind `work`"; [F12 §2]; [F13 §4.1]; [F05 §9.2] reason 5 `park` | An `orphans/<ref>` ref (kind 6) holds a commit whose ref CAS failed at replay, parked there by [F16] P-70; it is not of kind `work`, so it never holds, and its tip is read like VK-004's (review pass 1 round 2, residue of P1-3, S1-11, A1-11). |

The **hold** of `#N` at a commit c is the closed value its state has there; `none` otherwise. A deleted node's hold is
`deleted` whatever its status was.

<!-- table: hold-values -->
| row | kind | state | hold | basis | source | note |
|---|---|---|---|---|---|---|
| HV-001 | task | `done` | done | design | [AR §3.4] I26′ "holds `#N` done"; [AR §4.5] step 4 | Live, with status `done`. |
| HV-002 | task | `cancelled` | cancelled | design | [AR §3.4] I26′ "cancelled"; [AR §4.5] step 4 | Live, with status `cancelled`. |
| HV-003 | * | deleted | deleted | design | [AR §3.4] I26′ "or deleted"; [AR §4.5] step 4 "every `Delete` a `Marker{deleted}`" | A tombstone in the state, any kind. |
| HV-004 | * | other | none | derived | [AR §3.4] I26′ | Live with any other status, any other kind, or absent from the state. |

The **origin** `org(c, #N)` of a closed hold is "the commit on X's history that last set that state" ([AR §3.4] I26′):
the commit where that exact hold value first appears on the way from c back through its parents. Rows are evaluated in
order; `hold(x)` is `#N`'s hold at `state_at(x)`.

<!-- table: origin-rules -->
| row | parents | condition | origin | basis | source | note |
|---|---|---|---|---|---|---|
| OR-001 | 0 | always | c | derived | [AR §3.4] I26′ | A root commit that holds `#N` in S is its origin. |
| OR-002 | 1 | p1-same | org-p1 | derived | [AR §3.4] I26′; [OP-1] | The value was inherited: follow the parent. |
| OR-003 | 1 | p1-differs | c | derived | [AR §3.4] I26′; [OP-1] | This commit set the value: an ordinary commit, a revert, a cherry-pick, an imported commit, a `Delete`, an `Undelete` that restores a closed status. |
| OR-004 | 2 | p1-same | org-p1 | proposed | [AR §3.4] I26′; [OP-1] | A merge or sync whose destination side already held the value keeps that origin. |
| OR-005 | 2 | p2-same | org-p2 | proposed | [AR §3.4] I26′; [OP-1] | The value came from the source side: the origin is on that side ("merge `lane/x` into `main`" does not originate `lane/x`'s completion again). |
| OR-006 | 2 | both-differ | c | proposed | [AR §3.4] I26′; [OP-1] | The merge produced the value itself (a resolution, `--policy`), so it is the origin. |

`hold(X)` of a live ref X is the pair (v, o): the hold v of `#N` at tip(X), and, when v is in S, its origin
o = org(tip(X), #N). **X holds `#N`** when X is of a kind whose `holds_count` is `yes` and v is in S.

## 4. The definition, and the derived and runtime predicates

`predicates` lists each predicate as the conjunction (or, for PD-012 to PD-014, the existential) of its clauses. The
model implements one function per clause and tags it with the row id.

<!-- table: predicates -->
| row | predicate | clause | basis | source | note |
|---|---|---|---|---|---|
| PD-001 | unblocked | kind-task | design | [AR §3.5]; [50 §3.8] | - |
| PD-002 | unblocked | status-open | design | [AR §3.5]; [50 §3.8] | - |
| PD-003 | unblocked | not-deleted | design | [AR §3.5] | [50 §3.8] omits it because patterns never bind deleted nodes ([50 §3.6]); the same set. |
| PD-004 | unblocked | not-conflicted | design | [AR §3.5]; [50 §3.8] | `conflicted`: an unresolved conflict value on the node on this branch. |
| PD-005 | unblocked | not-container | design | [AR §3.5]; [50 §3.8] | PD-022. |
| PD-006 | unblocked | open_blockers-zero | design | [AR §3.5]; [50 §3.8] "flagged edges count" | The count of `blocker-terms` rows with `counts_in = open_blockers`. |
| PD-007 | unblocked | no-ancestor-exo | design | [AR §3.5] "no ancestor with `open_blockers_exo > 0`" | Walks at most 12 parents ([AR §3.4] I4). |
| PD-008 | unblocked | defer-until-le-view-now | design | [AR §3.5]; [50 §3.8]; [50 §3.9] item 7 | `defer_until ≤ now()`: the wall clock read once at query start at a tip, the view commit's HLC at a past view. |
| PD-009 | ready | unblocked | design | [AR §3.5] `ready`; [50 §3.8]; [60 §2.5] "Derived-state semantics" | PD-001 to PD-008 at the tip. |
| PD-010 | ready | tip-view | design | [AR §3.5] "read-time, tip only"; [50 §3.8] | At any other view, `ready` is E302 with `unblocked` as the suggested fix (`validity`). |
| PD-011 | ready | no-other-live-lease | design | [AR §3.5] "no live lease by another holder"; [AR §6.2] | No task lease on `#N` that `lease-live` says is live, held by a holder other than the caller (LF-001). |
| PD-012 | excluded | held-elsewhere-unabsorbed | design | [AR §3.4] I26′; [72 M4] fix 1; [60 §4.2] | On view R: some live ref X ≠ R holds `#N` (HV, `view-kinds`) with origin o, and o ∉ anc*(tip(R)). |
| PD-013 | deleted_elsewhere | held-elsewhere-unabsorbed-deleted | design | [AR §3.5] `deleted_elsewhere`; [50 §3.6] | PD-012 restricted to holds whose value is `deleted`. |
| PD-014 | settled_elsewhere | held-elsewhere-unabsorbed-closed | design | [AR §3.5] `settled_elsewhere`; [AR §5d.2] | PD-012 restricted to holds whose value is `done` or `cancelled` (tasks only, HV-001, HV-002). |
| PD-015 | ready | not-excluded | design | [AR §3.4] I26′ "excluded from `ready`/`claim`"; [AR §3.5] "no active settled/deleted marker … not absorbed" | ¬PD-012 on the reading branch. |
| PD-016 | ready | defer-until-le-wall-now | design | [AR §3.5] "`defer_until ≤` wall-clock now" | At a tip the view's `now()` is the wall clock, so PD-008 and PD-016 coincide. |
| PD-017 | claim | ready-on-claimer-branch | design | [AR §6.2] "the task must be live and `ready` on R"; [AR §5d.1] | With the claimer as the caller, so its own live lease does not exclude it (claiming again is idempotent). `gates` never constrains `claim` (X5). |
| PD-018 | blocking-listed | is-blocker-task-not-excluded | design | [AR §3.4] I26′ "never listed there as a live blocker"; [AR §7.1] `blocking`; [OP-8] | `is_blocker ∧ kind = task ∧ ¬PD-012`. |
| PD-019 | blocked | kind-task | design | [AR §3.5] `blocked` | - |
| PD-020 | blocked | unfinished | design | [AR §3.5] | PD-025. |
| PD-021 | blocked | open-blocker-or-exo-ancestor | design | [AR §3.5] "`open_blockers > 0 ∨` an ancestor has an open exogenous blocker" | Flagged edges count. |
| PD-022 | container | has-live-child | proposed | [50 §2.5] `container` among the derived properties; [AR §3.1] flag bit 4; [OP-14] | At least one live node has this node as its `parent`. |
| PD-023 | is_blocker | unfinished-with-unfinished-target | design | [AR §3.5] "not done ∧ has an outgoing `blocks` edge to a not-done task" | - |
| PD-024 | gated | gating-in-edge | proposed | [AR §3.3] `gates`; [AR §3.6]; [RULES/status-machines GD-002]; [OP-7] | At least one `blocker-terms` row with `counts_in = gated` and weight 1 applies. Drives the `complete` guard only. |
| PD-025 | unfinished | not-done | design | [AR §3.5]; [50 §3.8] | `done` per [RULES/status-machines] `statuses` (for a question, PD-026). |
| PD-026 | answered | live-answers-in-edge | design | [AR §3.5] `answered`; [AR §2.17] S2-B | A live `answers` in-edge from a live `decision` or `note` is visible on the reading branch. |

<!-- table: validity -->
| row | predicate | valid_at | past_view | basis | source | note |
|---|---|---|---|---|---|---|
| VD-001 | unblocked | any-view | value | design | [50 §3.8]; [60 §2.5] "Derived-state semantics" | The persisted bitset holds PD-001 to PD-007; PD-008 is evaluated per candidate at the view's `now()`. |
| VD-002 | blocked | any-view | value | design | [50 §3.8] | - |
| VD-003 | container | any-view | value | derived | [50 §2.5] | - |
| VD-004 | gated | any-view | value | derived | [AR §3.3] `gates` | - |
| VD-005 | ready | tip-only | E302 | design | [AR §3.5]; [50 §3.8] | The fix names `unblocked`. |
| VD-006 | excluded | tip-only | E302 | design | [50 §3.8] runtime row; [AR §3.4] I36′ | Depends on the live ref set. |
| VD-007 | settled_elsewhere | tip-only | E302 | design | [50 §3.8] | - |
| VD-008 | deleted_elsewhere | tip-only | E302 | design | [50 §3.8] | - |
| VD-009 | claimed | tip-only | E302 | design | [50 §3.8]; [AR §3.4] I36′ | Derived from the lease table. |

The terms that count toward `open_blockers` and the completion guard. `open_blockers_exo(n)` sums the same terms
restricted to in-edges whose source lies outside subtree(n); the dead source of a flagged edge lies outside every
subtree.

<!-- table: blocker-terms -->
| row | edge | source_state | counts_in | weight | basis | source | note |
|---|---|---|---|---|---|---|---|
| BT-001 | blocks | live-task-unfinished | open_blockers | 1 | design | [AR §3.5] "`blocks`/`gates` in-edges whose source is not done" | - |
| BT-002 | blocks | live-question-unanswered | open_blockers | 1 | design | [AR §3.5]; [50 §2.5] `BLOCKS` "task, question → task" | A question is done when `answered` (PD-026). |
| BT-003 | blocks | live-finished | open_blockers | 0 | design | [AR §3.5] | - |
| BT-004 | blocks | flagged | open_blockers | 1 | design | [AR §3.5] "plus flagged dangling blocker edges"; [AR §3.4] I2; [RULES/delete-policy-matrix FL-001] | - |
| BT-005 | gates | live-verdict-gating | gated | 1 | design | [AR §3.3] `gates` "completion blocked while the verdict is `open` with outcome ∈ `fail_*`" | - |
| BT-006 | gates | * | open_blockers | 0 | proposed | [AR §3.3] X5 "`gates` constrains `complete` … never `claim`"; [AR §6.2]; [OP-7] | [AR §3.5] counts `gates` in-edges in `open_blockers`; X5 forbids that for readiness. |
| BT-007 | gates | flagged | gated | 1 | proposed | [AR §3.3] `gates` "a deleted failing verdict never silently ungates"; [RULES/delete-policy-matrix FL-002] | - |
| BT-008 | gates | live-verdict-not-gating | gated | 0 | design | [AR §3.3] `gates` | Accepted, superseded, or not failing. |

## 5. Leases

A lease row that `lease-ends` has ended is not live and is ignored. For every other task lease, `lease-live` decides,
first matching row wins. Columns: `scope` (`run` for run-scoped, `ttl` otherwise); `anchor` (the anchor kind of
[80 §3] X-F2 as amended by [90 §10.1]); `boot` (the checker's boot identity against the lease's `boot_hash`: `same`,
`different`, or `unknown` for a checker in Unknown-boot mode); `slot` (`named` when some held liveness slot of `LOCK`
has a valid record whose primary hash equals the anchor's, `not-named` otherwise, `unreadable` when `LOCK` cannot be
read or probed); `deadline` (`passed` when the boot-clock deadline `mono` has passed on the same boot, or the wall
deadline in Unknown-boot mode).

<!-- table: lease-live -->
| row | scope | anchor | boot | slot | deadline | live | basis | source | note |
|---|---|---|---|---|---|---|---|---|---|
| LL-001 | run | * | * | * | * | yes | design | [AR §6.2] "released **only** by `apply` … `run close`, or an explicit `reclaim`"; [72 B2] fix 4 | Anchor, boot and clocks never end a run-scoped lease. |
| LL-002 | ttl | * | different | * | * | no | design | [AR §6.2] "after a reboot every non-run-scoped lease is Dead"; [80 §2.7.2] | Released at the first read with a triage line (LE-009). |
| LL-003 | ttl | none | * | * | passed | no | design | [AR §6.2]; [80 §2.7.1] "Lease deadlines" | Anchor `none` lives by its deadline alone. |
| LL-004 | ttl | none | * | * | not-passed | yes | design | [AR §6.2]; [90 §4.4] | - |
| LL-005 | ttl | session | * | named | * | yes | design | [80 §2.7.2] `session` "Alive when … some held slot has a valid record … whose primary hash equals the anchor's"; [90 §4.4] | Alive keeps the lease whatever its deadline ([OP-12]). |
| LL-006 | ttl | session | * | not-named | * | no | design | [80 §2.7.2] "Dead when … no held slot's record names the session" | - |
| LL-007 | ttl | session | * | unreadable | not-passed | yes | design | [AR §6.2] "Unknown never ends a lease early"; [80 §2.7.2] | - |
| LL-008 | ttl | session | * | unreadable | passed | no | design | [AR §6.2] "only its deadline, run scope or an explicit `reclaim` does" | - |
| LL-009 | ttl | session-ttl | * | named | not-passed | yes | design | [80 §2.7.2] `session-ttl`; [90 §4.4] | - |
| LL-010 | ttl | session-ttl | * | named | passed | no | design | [80 §2.7.2] "(`session-ttl`) the deadline has passed" | - |
| LL-011 | ttl | session-ttl | * | not-named | * | no | design | [80 §2.7.2] | - |
| LL-012 | ttl | session-ttl | * | unreadable | not-passed | yes | design | [AR §6.2]; [80 §2.7.2] | - |
| LL-013 | ttl | session-ttl | * | unreadable | passed | no | design | [AR §6.2] | - |
| LL-014 | ttl | leader | * | * | * | - | derived | [AR §4.4] `LEASES` anchor kinds; [80 §2.7.2] `leader`; [F03 §10.3]; [F11 §6]; [OP-12] | Unreachable: a lease carries anchor kinds 0 `none`, 1 `session` and 4 `session-ttl` only ([F03 §10.3]), and a `LEASES` row with kind 2 `intent` or 3 `leader` is invalid ([F11 §6]), so no lease reaches this row in format v1. A case that reaches this row is a failed internal check, which the model reports as a test failure naming the row, never as `SpecGap` (as [RULES/link-merge-rules] LR-006 does; review pass 1 round 2, P1-21). The optional leader, if built, adds its rule here. |

<!-- table: lease-ends -->
| row | event | effect | basis | source | note |
|---|---|---|---|---|---|
| LE-001 | release | ended | design | [AR §6.2]; [AR §7.1] `release L` | Presents the lease's current token (I17′). |
| LE-002 | complete | ended-into-settled | design | [AR §6.2] "releases the lease **into `settled`**" | The completing commit is the origin of the task's `done` hold ([RULES/status-machines CO-001]). |
| LE-003 | apply | ended | design | [AR §6.2] "`apply` (for the leases its batch names)" | Run-scoped leases. |
| LE-004 | run-close | ended | design | [AR §6.2] | The run's leases. |
| LE-005 | reclaim-run | ended | design | [AR §6.2] `reclaim --run <id>` | - |
| LE-006 | reclaim-older-than | ended | design | [AR §6.2] `reclaim --older-than` | - |
| LE-007 | rm-release | ended | design | [AR §3.4] I32′; [RULES/delete-policy-matrix DS-007] | With a triage note on the tombstone. |
| LE-008 | branch-deleted | ended | design | [AR §5a.9] "live leases on that branch are released with a triage note"; [AR §5d.1] | - |
| LE-009 | dead-at-read | ended | design | [AR §6.2] "released at the first read with a triage line" | A lease whose anchor is Dead (LL-002, LL-006, LL-011) is ended by the first read that evaluates it. A lease that is not live only because its deadline passed stays until reclaimed or renewed (LE-011). |
| LE-010 | subagent-stop | ended | design | [AR §6.2]; [AR §7.5] `SubagentStop` | Only where the hook runs; it "releases or flags". |
| LE-011 | renew | not-ended | design | [AR §3.4] I17′ "the same holder may renew an expired, unreclaimed lease"; [90 §4.4] | A write presenting a TTL lease, or `heartbeat`, moves its deadline when more than half the TTL has elapsed; the token is unchanged. |

<!-- table: lease-effects -->
| row | consumer | rule | basis | source | note |
|---|---|---|---|---|---|
| LF-001 | ready | other-holder-excludes | design | [AR §3.5]; [AR §6.2] | A live task lease on `#N` whose holder is not the caller excludes `#N` from the caller's `ready` on every branch; a caller with no resolved actor is excluded by every live lease ([OP-13]). Role leases (`#N` = 0) never count. |
| LF-002 | ready | store-wide | design | [AR §5d.1] "visible from every branch" | A lease taken on `lane/x` excludes `#N` on `main` too. |
| LF-003 | claim | requires-ready | design | [AR §6.2]; [RULES/state-definition PD-017] | A new lease gets token `HEAD.fence + 1`; expiry never bumps the token (I17′). |
| LF-004 | claim | gates-free | design | [AR §3.3] X5; [AR §6.2] | - |
| LF-005 | rm | lease-refuses | design | [AR §3.4] I32′; [RULES/delete-policy-matrix DP-005] | Any live lease on any branch, unless `--release`. |
| LF-006 | all | unknown-never-ends | design | [AR §6.2]; [80 §2.7.2] | - |
| LF-007 | merge | never-merged | design | [AR §3.4] I36′; [RULES/merge-table RE-001] | - |

## 6. The marker cache

The engine does not evaluate PD-012 by walking the DAG: it keeps one marker per closed hold origin and answers PD-012
with one probe and one vector lookup (CM1). This section is the cache's exact specification; the byte layout of
`MARKERS` and `MARKERS_OLD` is [F11 §7]'s (the key of MF-001, MF-003 and MF-004, the holder set of MF-006 and the flag of
MF-007 included), and the log record that carries every change is [F05 §9.5]'s. [F13 §4] cites these rows as I26′'s
definition and marker-cache rules.

<!-- table: marker-fields -->
| row | field | basis | source | note |
|---|---|---|---|---|
| MF-001 | n | design | [AR §4.4] `MARKERS` `{#N, …}` | The node. |
| MF-002 | kind | design | [AR §4.4] "kind settled\|deleted\|cleared" | `settled` for a `done` or `cancelled` hold, `deleted` for a `deleted` hold; `cleared` ends a marker (ME-004, ME-005). |
| MF-003 | origin-commit | design | [AR §4.4] "commit id16"; [60 §2.5] "the marker key `(#N, ref_id, commit)`" | The origin o of the hold (OR rows). |
| MF-004 | origin-ref | design | [AR §4.4] `ref_id`; [AR §5d.1] | ref(o): the ref o landed on, which may since have been deleted or be a staging ref. |
| MF-005 | origin-ref-seq | design | [AR §4.4] `ref_seq` | ref_seq(o). |
| MF-006 | holders | proposed | [AR §5a.9] "re-attributes … to a live ref that contains the marker's commit"; [72 M4] fix 2; [OP-3] | The set of live refs that hold `#N` with this origin (`view-kinds` `holds_count = yes`). The marker is **active** while it is non-empty. Stored as a list of `ref_id`s in the row ([F11 §7]) and in every record that changes it ([F05 §9.5]). |
| MF-007 | nonlinear | proposed | [72 M4]; [OP-4] | Set once, never cleared (ME-011, ME-013); selects the exact DAG test (AB-002). A flag bit of the row ([F11 §7]). |
| MF-008 | hlc, seq | design | [AR §4.4] | Bookkeeping: when the record was written. |
| MF-009 | holder-actor, outcome | design | [AR §5d.1] `settled` `{…, holder, outcome, …}`; [API §10.5]; [F05 §9.5] fields 9, 11; [F11 §7] `actor`, `outcome` | For `settled` records written by `complete`: the lease holder and the outcome, for triage lines only. Exactly one entry carries them: the `settled` entry (ME-001) that the commit of a `complete` writes for the task it settles, with the holder of the task lease that `complete` presented and released and its `--outcome` (1 `done`, 2 `failed`, 3 `abandoned`; CO-001 to CO-003). Every other entry carries 0 for both: the other doors of ME-001, `cancelled` holds, and every re-emit (ME-003, ME-006, ME-007), even of a marker a `complete` once wrote, so a re-emit resets the row's `actor` and `outcome` to 0. Entries that keep the marker (`holders`, `flag-nonlinear`, a revival by ME-013) carry neither field and leave the row's values unchanged (review pass 1 round 3). |

Each row of `marker-events` fires for every node whose hold changes at a live ref. "(v0, o0)" is the ref's hold before
the event and "(v1, o1)" after it. `record` names the `Marker` entries appended to the log ([F05 §9.5]), in the same
flushed group as the commit or `RefUpdate` that caused them ([AR §4.3]): `settled-or-deleted` writes or re-emits the
marker with its complete holder set; `holders` replaces the holder set of an existing marker; `cleared` ends a marker
whose holder set emptied; `flag-nonlinear` sets MF-007. Every change of a marker's holder set or flag is recorded, so
replay applies the records in log order and never derives a holder change; the model maintains the cache by these rules
itself and GT2 compares the two ([60 §4.4] item 5). `none` means that no marker changes (ME-008 to ME-010);
`move-cold` is the checkpoint fold's own change and writes no record (ME-012).

<!-- table: marker-events -->
| row | event | condition | record | basis | source | note |
|---|---|---|---|---|---|---|
| ME-001 | commit-lands | new-origin-here | settled-or-deleted | design | [AR §4.5] step 4; [72 M4] fix 2 | v1 in S and o1 = this commit: a new marker with holders {X}. Covers every door (`door-coverage`). |
| ME-002 | commit-lands | new-origin-active | holders | proposed | [AR §5a.7] step 7; [OP-2] | v1 in S, o1 older and its marker active: X joins its holders. The typical merge into `main` or `sync`. |
| ME-003 | commit-lands | new-origin-inactive | settled-or-deleted | proposed | [72 M4] fix 2 "the landing commit does"; [OP-2] | v1 in S, o1 older and its marker inactive or never written (for example an origin on a staging or import ref): the marker is written, or re-emitted, with holders {X}. |
| ME-004 | commit-lands | old-hold-ends | holders-or-cleared | proposed | [AR §4.5] step 4 "`cleared` scoped to `(#N, ref_id)`"; [72 M4] fix 2; [OP-3]; [OP-5] | v0 in S and (v1, o1) ≠ (v0, o0): X leaves o0's holders (`holders`); if none remain, `cleared` is written for the marker key (`#N`, ref(o0), o0) instead. Any exit from S counts, not only `→ open` and `Undelete`. |
| ME-005 | ref-deleted | work-ref | holders-or-cleared | design | [AR §5a.9] `-D` "clears only the markers no live ref still holds"; [72 M4] fix 2 | The ref leaves every holder set (`holders`); each marker left with no holder is cleared instead, with a triage line. A marker that keeps a holder stays active at its origin position: the design's "re-attribution" ([OP-3]). Applies to `-d` as well. |
| ME-006 | ref-moved | work-ref | per-hold-change | design | [AR §5a.5] `undo`, `op restore` "recomputes the markers … in both directions"; [72 M4] fix 2 | For every node whose hold differs between the old and the new tip: ME-004 for the old hold, ME-002 or ME-003 for the new one, each with its record; one triage line per `settled`, `deleted` or `cleared` record. |
| ME-007 | ref-created | work-ref | holders-or-re-emit | derived | [AR §5a.3] fork; [72 M4] scenario 3 | A fork Y at commit f joins the holders of every closed hold at f: `holders` for an active marker (ME-002), a re-emitted marker for an inactive one (ME-003). |
| ME-008 | commit-lands | non-work-ref | none | design | [72 M4] fix 2 "Staging refs produce no markers"; [AR §3.4] I26′; [AR §2.17] X13 | Commits on `merge/*`, `import/*` and `plan/*` refs. |
| ME-009 | ref-deleted | non-work-ref | none | design | [AR §5a.7] step 8 `merge --abort`; [RULES/merge-table RE-004] | - |
| ME-010 | commit-lands | hold-unchanged | none | design | [72 M4] fix 2 "on a done task emits nothing"; [AR §4.3] | Markers follow net state, never statements. |
| ME-011 | commit-lands | origin-ref-diverged | flag-nonlinear | proposed | [OP-4] | A commit d lands on ref Y while some Y-landed commit o that is the origin of a marker in `MARKERS` is not in anc*(d), because `undo` or `op restore` moved Y off o. o's marker becomes nonlinear for good. A marker in `MARKERS_OLD` is not flagged here: ME-013 flags every marker it revives. |
| ME-012 | checkpoint-fold | inert | move-cold | design | [AR §4.4] "`MARKERS_OLD` … globally inert … move there"; [70 S4] | A cleared marker, or one that every live ref (of every kind) has absorbed (`absorption`), moves to `MARKERS_OLD` with an empty holder set; its holder set is no longer maintained. The fold decides this from the rows and the absorbed vectors alone, so it writes no record. |
| ME-013 | ref-moved-or-created | revive | holders-and-nonlinear | proposed | [AR §4.4]; [OP-10] | After `undo`, `op restore` or a fork from a commit other than a tip, a `MARKERS_OLD` row whose origin some live ref no longer contains, and which some live work ref still holds, returns to `MARKERS` with its holders recomputed (`holders`) and flagged nonlinear (`flag-nonlinear`): a revival follows exactly such a ref move, and AB-002 is exact, so the flag can cost a walk but never an answer. |

A reader on view R tests a marker for `#N` like this; `absorbed_R` is the vector of `vector-rules`.

<!-- table: absorption -->
| row | marker_state | test | basis | source | note |
|---|---|---|---|---|---|
| AB-001 | linear | vector | design | [AR §5d.1] "`absorbed_R[ref_id] < ref_seq`"; [AR §3.5] `settled_elsewhere` | Absorbed iff `absorbed_R[origin-ref] ≥ origin-ref-seq`: one lookup, O(1) (CM1). |
| AB-002 | nonlinear | dag | proposed | [OP-4] | Absorbed iff origin-commit ∈ anc*(tip(R)), by a generation-pruned walk ([AR §5a.1] `gen`). |
| AB-003 | inactive | ignored | proposed | [OP-3] | A marker with no holder never excludes. |
| AB-004 | any | excluded-iff-unabsorbed | design | [AR §3.5] `settled_elsewhere`, `deleted_elsewhere`; [AR §3.4] I26′ | `#N` is excluded on R iff some active marker of `#N` is not absorbed by R; `settled_elsewhere` and `deleted_elsewhere` split by the marker's kind. |

<!-- table: vector-rules -->
| row | event | rule | basis | source | note |
|---|---|---|---|---|---|
| VR-001 | definition | max-ref-seq | design | [AR §5d.1] "the highest `ref_seq` of that ref reachable from tip(R)" | V(c)[Y] = max{ref_seq(d) : d ∈ anc*(c), ref(d) = Y}, or 0; `absorbed_R = V(tip(R))`. |
| VR-002 | commit-one-parent | copy-and-own | design | [AR §5d.1] "a commit on R sets `absorbed_R[R]` to its `ref_seq`" | V(c) = V(p1) with V(c)[ref(c)] = ref_seq(c). |
| VR-003 | commit-two-parents | pointwise-max-and-own | design | [AR §5d.1] "a merge or sync … every other entry to the max of both sides"; [AR §5a.7] step 7 | V(c) = max(V(p1), V(p2)) entry by entry, with V(c)[ref(c)] = ref_seq(c). This equals the design's `absorbed_dst[src] = ref_seq(tip src)` whenever tip(src) landed on src ([OP-9]). |
| VR-004 | fork | vector-at-fork-commit | derived | [AR §5d.1] "a fork of Y from X at `ref_seq f` copies X's vector"; [OP-9] | absorbed_Y = V(f), which is X's vector only when f is tip(X). |
| VR-005 | undo-or-restore | vector-at-new-tip | design | [AR §5d.1] "`undo` restores the vector recorded on the newest merge/sync/fork record at or below the restored tip" | absorbed_R = V(new tip). |
| VR-006 | ref-deleted | keep-entries | proposed | [AR §4.4] `REFS`; [OP-10] | Vector entries for a deleted ref stay while any marker names it as origin-ref; ref ids are never reused. |

**Why the cache equals the definition.** For a view R and a node `#N`:

1. By the OR rules, o = org(tip(X), #N) is always in anc*(tip(X)). So if R itself holds `#N` with origin o, then
   o ∈ anc*(tip(R)); the clause "X ≠ R" of PD-012 is implied by "o ∉ anc*(tip(R))" and can be dropped.
2. ME-001 to ME-007 change a marker's holders exactly when some live ref's hold of `#N` changes, and every change of a
   ref's tip or of the ref set is one of their events. So a marker is active iff some live ref holds `#N` with its
   origin (MF-006), and PD-012 becomes: some active marker's origin is not in anc*(tip(R)).
3. For a linear marker with origin o on ref Y, every Y-landed commit with `ref_seq` ≥ `ref_seq(o)` descends from o
   (`ref_seq` grows along Y, and ME-011 flags the first commit that does not). Hence V(tip(R))[Y] ≥ `ref_seq(o)` iff
   o ∈ anc*(tip(R)), which is AB-001. A nonlinear marker is tested by AB-002, which is the definition itself.

The model runs PD-012 by DFS over all live refs and the cache by these rules after every command, and GT18 compares
them over ≥ 10⁶ histories with ≥ 5 refs ([60 §3.13]).

## 7. Doors and ref moves

GT18's "ten doors" and ref moves ([AR §8.2], [60 §3.13]) with the marker events each one exercises.

<!-- table: door-coverage -->
| row | door | events | basis | source | note |
|---|---|---|---|---|---|
| DC-001 | complete | ME-001 | design | [AR §8.2]; [AR §6.2] | - |
| DC-002 | set-done | ME-001 | design | [AR §8.2] | `set --done`. |
| DC-003 | set-status | ME-001, ME-004 | design | [AR §8.2] | `set --status cancelled`, and backward moves. |
| DC-004 | mcp-write-named | ME-001 | design | [AR §8.2] "`name: tx.complete`" | - |
| DC-005 | mcp-write-tx | ME-001, ME-010 | design | [AR §8.2] "a `TX` with `SET t.done = true`" | - |
| DC-006 | apply | ME-001 | design | [AR §8.2]; [AR §7.6] step 7 | - |
| DC-007 | cherry-pick | ME-001 | design | [AR §8.2] | The cherry-pick commit on R is the origin (OR-003). |
| DC-008 | revert | ME-001, ME-004 | design | [AR §8.2]; [AR §5a.5] | A revert of a completion ends the hold; a revert of a delete restores it (`Undelete`). |
| DC-009 | merge | ME-002, ME-003, ME-004, ME-001 | design | [AR §8.2]; [AR §5a.7] step 7 | ME-001 only when the merge itself produced the value (OR-006). |
| DC-010 | sync | ME-002, ME-003, ME-004 | design | [AR §8.2]; [AR §5a.3] | - |
| DC-011 | image-import | ME-001, ME-002, ME-003 | design | [AR §8.2]; [AR §5b.6] steps 2, 4 "the imported ops write their markers" | Fast-forward appends originate (ME-001); an import merge propagates (ME-002, ME-003). |
| DC-012 | reopen | ME-004 | design | [AR §8.2] "`reopen`" | - |
| DC-013 | undelete | ME-004, ME-001 | design | [AR §8.2] "`Undelete`" | ME-001 when the restored status is `done` or `cancelled` ([OP-6]). |
| DC-014 | undo | ME-006, ME-011 | design | [AR §8.2] "`undo` of a reopen or an undelete" | ME-011 fires at the next commit on the ref. |
| DC-015 | op-restore | ME-006, ME-011, ME-013 | design | [AR §8.2] "`op restore` in both directions" | - |
| DC-016 | fork | ME-007, ME-013 | design | [AR §8.2] "forks with `branch -D` or `undo` on the parent" | - |
| DC-017 | branch-delete | ME-005 | design | [AR §8.2]; [AR §5a.9] | `-d` and `-D`. |
| DC-018 | staging | ME-008, ME-009, ME-003 | design | [AR §8.2] "staging and `merge --abort`" | The landing commit of `merge --continue` propagates (ME-003). |
| DC-019 | tx-coalescing | ME-010 | design | [AR §8.2] "`TX` coalescing" | - |
| DC-020 | rm | ME-001 | design | [AR §4.5] step 4; [RULES/delete-policy-matrix DS-008] | The `deleted` hold. |

## 8. Worked scenarios

One task `#89`, open at commit `c0` on `main`; `lane/a`, `lane/b` and `lane/c` are work refs forked from `main` at
`c0`. Each scenario starts from that state. Actions are written `verb:arg:arg`: `complete:89`, `reopen:89`,
`set-done:89`, `set:<id>:<field>=<value>`, `rm:89`, `undo` (one step on the step's ref), `op-restore:<scenario>-<step>`
(all refs back to before that step), `branch:<name>:from=<ref>` (with `:kind=plan` for a plan branch),
`branch-D:<name>`, `sync`, `merge:<src>` into the step's ref (`:stages` when a violation on another key stages it),
`merge-abort:<src>`, `revert:<scenario>-<step>` (the commit that step wrote), and
`tx:reopen-then-set-done:89`. `scenario-expect` gives PD-012 for `#89` on `check_ref` after the step; the cache must
give the same answer.

<!-- table: scenarios -->
| row | scenario | step | ref | action | basis | source | note |
|---|---|---|---|---|---|---|---|
| SN-001 | S1 | 1 | lane/a | complete:89 | design | [72 M4] scenario 1 | Origin c1 on `lane/a`. |
| SN-002 | S1 | 2 | lane/a | reopen:89 | design | [72 M4] scenario 1 | - |
| SN-003 | S1 | 3 | lane/a | undo | design | [72 M4] scenario 1 | `lane/a` holds (done, c1) again; the marker is re-emitted (ME-006). |
| SN-004 | S2 | 1 | lane/a | complete:89 | design | [72 M4] scenario 2 | - |
| SN-005 | S2 | 2 | lane/a | op-restore:S2-1 | design | [72 M4] scenario 2 | The completion leaves every ref. |
| SN-006 | S3 | 1 | lane/a | complete:89 | design | [72 M4] scenario 3 | - |
| SN-007 | S3 | 2 | lane/b | branch:lane/b:from=lane/a | design | [72 M4] scenario 3 | `lane/b` is re-forked from `lane/a` after c1. |
| SN-008 | S3 | 3 | lane/a | branch-D:lane/a | design | [72 M4] scenario 3; [AR §5a.9] | `lane/b` still holds (done, c1). |
| SN-009 | S4 | 1 | lane/a | complete:89 | design | [72 M4] scenario 3 "or undoes the completion" | - |
| SN-010 | S4 | 2 | lane/b | branch:lane/b:from=lane/a | design | [72 M4] scenario 3 | - |
| SN-011 | S4 | 3 | lane/a | undo | design | [72 M4] scenario 3 | - |
| SN-012 | S5 | 1 | lane/a | complete:89 | design | [72 M4] scenario 4 | - |
| SN-013 | S5 | 2 | lane/b | set-done:89 | design | [72 M4] scenario 4 "only `claim` checks markers" | - |
| SN-014 | S5 | 3 | lane/b | reopen:89 | design | [72 M4] scenario 4 | - |
| SN-015 | S6 | 1 | lane/a | complete:89 | design | [72 M4] scenario 5 | - |
| SN-016 | S6 | 2 | main | merge:lane/a:stages | design | [72 M4] scenario 5 | The staging commit writes no marker (ME-008). |
| SN-017 | S6 | 3 | main | merge-abort:lane/a | design | [72 M4] scenario 5 | - |
| SN-018 | S7 | 1 | main | complete:89 | proposed | [OP-3] | Origin c1 on `main`. |
| SN-019 | S7 | 2 | lane/a | sync | proposed | [OP-3] | `lane/a` holds (done, c1) through the sync (OR-005). |
| SN-020 | S7 | 3 | main | reopen:89 | proposed | [OP-3] | `main` stops holding; `lane/a` still does. |
| SN-021 | S7 | 4 | lane/b | sync | proposed | [OP-3] | `lane/b` now contains c1. |
| SN-022 | S8 | 1 | lane/a | complete:89 | proposed | [OP-1] | - |
| SN-023 | S8 | 2 | main | merge:lane/a | proposed | [OP-1] | `main` holds with origin c1 (OR-005); no new marker (ME-002). |
| SN-024 | S8 | 3 | lane/b | merge:lane/a | proposed | [OP-1] | A cross-lane merge. |
| SN-025 | S8 | 4 | lane/b | reopen:89 | proposed | [OP-1] | A deliberate reopen after seeing c1. |
| SN-026 | S9 | 1 | lane/a | complete:89 | design | [72 M4] fix 2 | - |
| SN-027 | S9 | 2 | lane/a | tx:reopen-then-set-done:89 | design | [72 M4] fix 2 | Net state unchanged: no record (ME-010). |
| SN-028 | S10 | 1 | lane/a | complete:89 | proposed | [OP-5] | - |
| SN-029 | S10 | 2 | lane/a | revert:S10-1 | proposed | [OP-5] | `#89` back to `in_progress`: the hold ends (ME-004). |
| SN-030 | S11 | 1 | lane/a | complete:89 | proposed | [OP-6] | - |
| SN-031 | S11 | 2 | lane/a | rm:89 | proposed | [OP-6] | The hold becomes (deleted, c2). |
| SN-032 | S11 | 3 | lane/a | revert:S11-2 | proposed | [OP-6] | `Undelete` restores `done`: the hold is (done, c3), origin the revert commit. |
| SN-033 | S12 | 1 | lane/a | complete:89 | proposed | [OP-4] | Origin c1 on `lane/a`. |
| SN-034 | S12 | 2 | lane/b | branch:lane/b:from=lane/a | proposed | [OP-4] | - |
| SN-035 | S12 | 3 | lane/a | undo | proposed | [OP-4] | c1 leaves `lane/a`'s chain; `lane/b` still holds (done, c1). |
| SN-036 | S12 | 4 | lane/a | set:90:priority=1 | proposed | [OP-4] | A new `lane/a` commit c2 with a higher `ref_seq` that does not descend from c1: c1's marker becomes nonlinear (ME-011). |
| SN-037 | S12 | 5 | main | merge:lane/a | proposed | [OP-4] | `absorbed_main[lane/a]` ≥ `ref_seq(c1)`, yet c1 is not in `main`'s history. |
| SN-038 | S13 | 1 | lane/a | complete:89 | design | [AR §2.17] X13 | - |
| SN-039 | S13 | 2 | plan/p | branch:plan/p:from=lane/a:kind=plan | design | [AR §5a.1] | - |
| SN-040 | S13 | 3 | lane/a | branch-D:lane/a | design | [AR §3.4] I26′ "of kind `work`" | Only a plan branch still holds c1. |

<!-- table: scenario-expect -->
| row | scenario | step | check_ref | excluded | basis | source | note |
|---|---|---|---|---|---|---|---|
| SX-001 | S1 | 1 | main | yes | design | [AR §5d.2] | - |
| SX-002 | S1 | 2 | main | no | design | [AR §5d.1] "superseded by a `Marker{cleared}` … written by `reopen`" | - |
| SX-003 | S1 | 3 | main | yes | design | [72 M4] scenario 1 "`main` lists `#89` in `ready`" is the bug fixed | - |
| SX-004 | S2 | 1 | main | yes | design | [AR §5d.2] | - |
| SX-005 | S2 | 2 | main | no | design | [72 M4] scenario 2 | - |
| SX-006 | S3 | 3 | main | yes | design | [72 M4] scenario 3; [AR §5a.9] | - |
| SX-007 | S3 | 3 | lane/c | yes | design | [72 M4] scenario 3 | - |
| SX-008 | S4 | 3 | main | yes | design | [72 M4] scenario 3 | - |
| SX-009 | S4 | 3 | lane/a | yes | derived | [AR §3.4] I26′ | `#89` is open again on `lane/a`, and `lane/b` holds c1, which `lane/a` no longer contains. |
| SX-010 | S5 | 3 | main | yes | design | [72 M4] scenario 4 "A's completion vanishes" is the bug fixed | `lane/a` holds c1. |
| SX-011 | S5 | 3 | lane/b | yes | derived | [AR §3.4] I26′ | `lane/b` reopened, but `lane/a`'s c1 is not in its history. |
| SX-012 | S6 | 2 | lane/c | yes | design | [72 M4] scenario 5 | `lane/a` holds c1; the staging commit changes nothing. |
| SX-013 | S6 | 3 | lane/c | yes | design | [72 M4] scenario 5 | - |
| SX-014 | S6 | 3 | main | yes | design | [72 M4] scenario 5 | - |
| SX-015 | S7 | 3 | lane/b | yes | proposed | [AR §3.4] I26′; [OP-3] | `lane/a` holds c1 and `lane/b` lacks it. A `cleared` scoped to (`#89`, `main`) would answer `no`. |
| SX-016 | S7 | 4 | lane/b | no | proposed | [OP-3] | - |
| SX-017 | S8 | 4 | lane/b | no | proposed | [OP-1] | c1 is in `lane/b`'s history; `main`'s hold has origin c1, not the merge commit. |
| SX-018 | S8 | 4 | lane/c | yes | proposed | [OP-1] | - |
| SX-019 | S9 | 2 | main | yes | design | [72 M4] fix 2 | Unchanged from step 1. |
| SX-020 | S10 | 2 | main | no | proposed | [OP-5] | - |
| SX-021 | S11 | 2 | main | yes | design | [AR §5d.3] | `deleted_elsewhere`. |
| SX-022 | S11 | 3 | main | yes | proposed | [OP-6] | `settled_elsewhere` again, from the `Undelete`. |
| SX-023 | S12 | 5 | main | yes | proposed | [OP-4] | Only the exact test (AB-002) sees it. |
| SX-024 | S13 | 3 | main | no | design | [AR §3.4] I26′ "of kind `work`"; [AR §2.17] X13 | - |

## Coverage

These tables specify semantics. The byte layouts are [F11]'s (`MARKERS`, `MARKERS_OLD`, `LEASES`, `REFS` and its
absorbed vectors), [F03]'s (`LOCK` slots, `Anchor`), [F05]'s (`Marker`, `Lease`, `RefUpdate` records) and [F06]'s (the
commit's `absorbed` vector, `ref_seq`).

| Checklist row | Covered by | Fixture | Model function |
|---|---|---|---|
| [60 §2.5] "Derived-state semantics": the bitset holds only the structural predicate `unblocked`; `ready` adds leases, markers and `defer_until` at read time, at a tip only | PD-001 to PD-016, `validity` | WP-94 suite `state`; GT18 I26′ oracle | `model::derived::unblocked`, `model::runtime::ready` |
| [60 §2.5] "Segments": `MARKERS`/`MARKERS_OLD` with the marker key `(#N, ref_id, commit)` (its semantics) | `marker-fields`, `marker-events`, `absorption` | GT18 (model, M0; engine, M3) | `model::markers::apply_event`, `model::markers::excluded` |
| [60 §2.5] "Segments": `LEASES` with the holder anchor and the deadline form `{wall, boot_hash, mono}` (its semantics) | `lease-live`, `lease-ends`, `lease-effects` | GT18 lease liveness | `model::lease::is_live` |
| [80 §3] X-F2: liveness rules, Unknown never ends a lease, the boot-clock deadline, Unknown-boot mode (semantic part) | LL-001 to LL-013, LE-009, LF-006 | GT18 lease liveness; GT4 lease variants (M1) | `model::lease::is_live` |
| [90 §10.1] `LEASES` `kind`, `anchor` (`session-ttl`), `bound` (effect on `ready`) | LL-009 to LL-013, LF-001 | GT18 | `model::lease::is_live` |
| [50 §8.1] F15: `affected` names every node whose derived predicate changed, `unblocked` included | PD-001 to PD-008 with [RULES/status-machines GR-015] | WP-94 | `model::derived::affected` |
| [AR §3.4] I17′, I26′, I32′, I36′ | LE-011, LF-003, PD-012, LF-005, LF-007 | GT18 | `model::i26::excluded` |

No R-row concerns this file.

## Holes

None. The TTLs are configuration keys (`lease.ttl-default`, `lease.reclaim-older-than`, `lease.orchestrator-ttl`,
[AR §13]); no rule here depends on a value an M0 measurement decides.

## Open points for the review

1. **"The commit on X's history that last set that state"** (OR rules, SN-022 to SN-025, SX-017, SX-018). Read as the
   **origin**: follow the parents, first parent first, while the hold value is unchanged; the commit where it first
   appears is the origin. Two other readings were rejected. (a) *The newest commit on X's first-parent chain whose
   diff changed the value* makes every merge and sync commit a new origin; the design's cache writes no marker for a
   sync's non-residue keys, so cache and definition would disagree. (b) *The newest commit whose stored ops set the
   value* depends on the sync residue, a storage detail, and over-excludes: after `lane/b` merges `lane/a` directly and
   reopens `#89` on purpose, `main`'s merge commit would keep `#89` excluded on `lane/b` until `lane/b` syncs `main`
   (S8). The origin reading is a pure function of states, as [72 M4] fix 1 requires. Review pass 1 (S1-16) adopted it:
   [F13 §4.1] now cites the OR rows and PD-012, and its OP-13-04 is closed.
2. **Merges and syncs propagate holds; they do not originate them** (ME-002, ME-003, DC-009, DC-010). [AR §4.5] step 4
   writes a marker for "every net `SetStatus{→ done|cancelled}` … whatever … merge, sync … produced the op". Under the
   origin reading a merge that takes a side's value adds that side's origin to the destination's holds instead.
   [RULES/merge-table RE-003] now cites ME-001 to ME-004; [AR §4.5] step 4 should say so at WP-81a.
3. **Holders, and the scope of `cleared`** (MF-006, ME-004, ME-005, AB-003, S7). [AR §5d.1] scopes `cleared` to
   (`#N`, `ref_id`). That is exact only when no other live ref holds the same origin. Counter-example (S7): `main`
   completes `#89`, `lane/a` syncs, `main` reopens; `lane/a` still holds `done` from c1, and `lane/b`, which has
   neither commit, would be told `#89` is dispatchable although the definition excludes it. Proposed: a marker carries
   the set of live refs that hold its origin, `cleared` is written only when that set empties, and it is keyed by the
   marker key (`#N`, origin-ref, origin-commit). The design's `-D` "re-attribution to a live fork" becomes the special
   case of ME-005 (the position stays the origin's, so a ref that absorbed the origin through another route is not
   excluded). Review pass 1 (S1-16) adopted the holder-set cache: [F13 §4.2] cites these rows (its OP-13-05, the
   `reopen` on a parent ref, is the case S7 settles), [F11 §7] keys the row by (`#N`, origin ref, origin commit) and
   stores the holder set as a heap list of `ref_id`s with the `nonlinear` flag, and [F05 §9.5] records every holder
   change (the `record` column), because replay cannot recompute a holder set from net ops: a `sync` stores only its
   residue, and a fork or a ref move changes holds with no op at all.
4. **Nonlinear markers** (MF-007, ME-011, ME-013, AB-002, S12). `undo` or `op restore` followed by a new commit on the
   same ref gives that ref two histories. A ref that absorbed the new one then has `absorbed[ref] ≥ ref_seq(o)` without
   o in its history, so the O(1) test (CM1) answers wrongly for an origin still held elsewhere. Proposed: flag such
   markers and test them by a DAG walk. The cost falls only on explicit, rare verbs. A marker in `MARKERS_OLD` is not
   tracked by ME-011; ME-013 flags every marker it revives, which is safe because AB-002 is the definition itself.
5. **Every exit from S ends a hold** (ME-004, S10). [AR §4.5] step 4 clears only on `SetStatus{done|cancelled → open}`
   and `Undelete`; a revert of a completion (`done → in_progress`) would leave a stale marker.
6. **`Undelete` of a closed node** (DC-013, S11). The design writes only `cleared` for an `Undelete`. When the restored
   status is `done` or `cancelled`, the branch still holds the task closed, so the `Undelete` commit becomes a new
   origin; otherwise another branch could dispatch a task this one holds done.
7. **`gates` and readiness** (BT-006, BT-007, PD-024). [AR §3.5] counts `gates` in-edges in `open_blockers`, which
   would take a task gated by a `fail_fixable` verdict out of `ready` and `claim` — the deadlock X5 was adopted to
   prevent ("`gates` constrains `complete` … never `claim`", [AR §3.3], [AR §6.2]). Proposed: `gates` counts only in
   the completion guard (`gated`), including a flagged `gates` edge. The name `gated` is new; WP-19 may rename it.
   Review pass 1 (S1-30) adopted this reading: [F13 §6.2] and [F08 §3.4] count `blocks` in-edges only, and [AR §3.5]
   should be edited at WP-81a.
8. **`blocking` and deleted nodes** (PD-018). I26′ says a node completed *or deleted* elsewhere is never listed as a
   live blocker; [50 §4.1]'s `std.blocking` filters only `NOT t.settled_elsewhere`. Proposed: WP-19's `LQ/std` text
   adds `AND NOT t.deleted_elsewhere` (or uses `excluded`).
9. **Absorbed vectors** (VR-003, VR-004). Defined as the maximum over ancestors (VR-001); the incremental rules follow.
   The design's "`absorbed_dst[src] = ref_seq(tip src)`" and "a fork … copies X's vector" agree with it only when
   tip(src) landed on src and when the fork is at tip(X); [RULES/merge-table RE-005] now cites VR-003.
10. **Retention and revival** (VR-006, ME-013). Markers may name deleted refs (MF-004), so their vector entries stay
    while any marker names them. A backward ref move or a fork from an old commit can make an inert marker count
    again; its `MARKERS_OLD` row then returns to `MARKERS`. Both are bounded by explicit verbs.
11. **Which refs hold** (VK rows). Only `work` refs hold, as I26′ says; `plan/*` what-ifs never block dispatch (X13),
    staging refs produce nothing ([72 M4]) and imports stage like merges. Reading `ready` at a tag is proposed as E302,
    since a tag is not a branch tip.
12. **Leases: `session` anchors and the deadline; the leader anchor** (LL-005, LL-014). [90 §4.4] keeps a Claude Code
    session-anchored lease Alive while its server holds the slot, whatever the TTL; [AR §6.2] lists "TTL (15 min
    self-claims)" as the liveness rule. Proposed: the TTL decides only when the liveness is Unknown, for `none` anchors
    and for `session-ttl`. A `leader` anchor ([AR §4.4] lists it) has no rule until the optional leader is built;
    [F03 §10.3] and [F11 §6] keep it off every lease (a lease carries anchor kinds 0, 1 and 4 only), so LL-014 is an
    unreachable row, no longer a `gap` (review pass 1 round 2: it was the last `gap` row of the rule files, P1-21).
13. **"Another holder"** (PD-011, LF-001). The holder compared is the caller's resolved actor ([90 §4.1] Actor row); a
    caller with no actor is excluded by every live lease. The design does not say how an anonymous `ready` treats
    leases.
14. **`container`** (PD-022). [50 §2.5] lists `container` as a derived property, and [RULES/merge-table FC-018] as a
    derived flag. Proposed: a node with at least one live child. The header's flag bit 4 is its cache.
15. **File name and registry.** This file is named as its commissioning task names it, `state-definition.md`.
    [RULES/README] §1.1 now lists it under that name, and its tables are registered in [RULES/README] §7 (RG-069 to
    RG-084, review pass 1 S1-47) with the columns below:

    ```
    | RG-0xx | `view-kinds` | state-definition.md | decision | VK | row:id, ref_kind:token, holds_count:enum(yes/no), tip_reads:enum(yes/no), basis:enum, source:cite, note:text | - |
    | RG-0xx | `hold-values` | state-definition.md | decision | HV | row:id, kind:token, state:token, hold:token, basis:enum, source:cite, note:text | - |
    | RG-0xx | `origin-rules` | state-definition.md | decision | OR | row:id, parents:int, condition:token, origin:token, basis:enum, source:cite, note:text | first match in order |
    | RG-0xx | `predicates` | state-definition.md | procedure | PD | row:id, predicate:token, clause:token, basis:enum, source:cite, note:text | - |
    | RG-0xx | `validity` | state-definition.md | decision | VD | row:id, predicate:token, valid_at:enum(any-view/tip-only), past_view:token, basis:enum, source:cite, note:text | - |
    | RG-0xx | `blocker-terms` | state-definition.md | decision | BT | row:id, edge:token, source_state:token, counts_in:token, weight:int, basis:enum, source:cite, note:text | - |
    | RG-0xx | `lease-live` | state-definition.md | decision | LL | row:id, scope:enum(run/ttl), anchor:token, boot:token, slot:token, deadline:token, live:token, basis:enum, source:cite, note:text | first match in order |
    | RG-0xx | `lease-ends` | state-definition.md | procedure | LE | row:id, event:token, effect:token, basis:enum, source:cite, note:text | - |
    | RG-0xx | `lease-effects` | state-definition.md | procedure | LF | row:id, consumer:token, rule:token, basis:enum, source:cite, note:text | - |
    | RG-0xx | `marker-fields` | state-definition.md | vocabulary | MF | row:id, field:token, basis:enum, source:cite, note:text | - |
    | RG-0xx | `marker-events` | state-definition.md | procedure | ME | row:id, event:token, condition:token, record:token, basis:enum, source:cite, note:text | - |
    | RG-0xx | `absorption` | state-definition.md | decision | AB | row:id, marker_state:token, test:token, basis:enum, source:cite, note:text | - |
    | RG-0xx | `vector-rules` | state-definition.md | procedure | VR | row:id, event:token, rule:token, basis:enum, source:cite, note:text | - |
    | RG-0xx | `door-coverage` | state-definition.md | map | DC | row:id, door:token, events:tokens, basis:enum, source:cite, note:text | - |
    | RG-0xx | `scenarios` | state-definition.md | decision | SN | row:id, scenario:token, step:int, ref:token, action:token, basis:enum, source:cite, note:text | fixture data |
    | RG-0xx | `scenario-expect` | state-definition.md | decision | SX | row:id, scenario:token, step:int, check_ref:token, excluded:enum(yes/no), basis:enum, source:cite, note:text | fixture data |
    ```

16. **Model cost.** PD-012 evaluated literally is O(live refs × history) per candidate. At the model's scale (≤ 10⁴
    commands, [60 §4.4] item 8) that is acceptable; ancestor sets are memoised per commit.
