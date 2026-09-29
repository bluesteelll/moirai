# Policy keys: every operational-policy key and policy-data row, and the model function that implements it

| Field | Value |
|---|---|
| Status | draft, pass 1 pending; needs the owner's signature ([RULES/README] §6) |
| Work package | WP-90b (R-MODEL); the functions of rows whose `wp` is a later package are bound by it (WP-91, WP-92, M5); WP-94 reports the rows no case reached |
| Sources | [CFG §9.4] (visibility classes, the `read by` column), [CFG §9.5] (the sweep plan), [CFG §10] (the key registry), [CFG §10.13] (policy data), [CFG §10.14] (the 25 former owner questions); [AR §13] "Typed registry", "Sweep plan", "Policy data"; [60 §3.14]; [60 §4.2] "budgets not modelled"; [API §2.5] DT-1, DT-2; [API §16.7]; [F17 §1.5] SP-1, SP-2; [m0/PLAN §3.2] WP-90, WP-94; [m0/PLAN §7] E8 |
| Format | [RULES/README] |
| Cited as | [RULES/policy-keys]; a row as [RULES/policy-keys KY-001] |

## 1. What this table decides

[CFG §9.2] RG-6 requires every key of visibility class V to have a model function here, and [m0/PLAN §3.2] WP-90 asks
for a table that maps **every** operational-policy key and policy-data row of [CFG]'s registry to a model function, with
a test per allowed value of every key and row. This file is that table:

- `key-checkers` names the checker of each visibility class ([CFG §9.4]): who tests a key's effect where the model does
  not compute it.
- `key-functions` names every model function a row binds, with the work package that builds it.
- `policy-keys` has one row per key pattern of [CFG §10], in the registry's order: the instance the sweep uses, the
  class, the model function, the allowed-value set of [CFG §9.5], and the checker.
- `policy-rows` has one row per policy-data row of [CFG §10.13]: the instance, the model function and its values.

The model loads the registry of [CFG §10] as data ([`registry`]) and checks this table against it in both directions:
every key pattern has exactly one row, every row names a registered pattern, its class is the registry's, and its
`values` cell equals the sweep set the model computes from the key's type and range. It also parses [CFG §10] itself
at test time and requires the registry to equal it (every key once, its type, default and class; the policy-data names
of [CFG §10.13]), so a transcription error cannot hide in both copies. Then every value of every row is tested (§2).

The `function` cells name the model's functions as the model binds them, and this table is normative for those names.
[CFG §10]'s `read by` column names six of them differently (`lease::deadline` for `lease::ttl_for`, `idem::lookup` for
`idem::Table::lookup`, `tx::check_caps` for `budget::check_caps`, and `policy::model_profile`,
`policy::read_safelist` and `policy::model_write_rule` for the `profile::` functions), and [CFG §10.13]'s `model`
column names the policy-data functions by an earlier plan (`policy::may_self_claim`, `policy::role_rights`, …);
aligning them is R-SPEC-F's (review of WP-90b).

## 2. How a row is tested

For every row and every token of its `values` cell, the model sets the key instance to the value — the empty value is
written `(empty)` and an unset key `(unset)` — with `Init`'s `params` for a store key and `ConfigSet` for a user key
([API §8.1], §8.2). A value that breaks a constraint alone (C-1 to C-4 of [F17 §3], K-1 of [CFG §10.8]) is set with
the companion value [CFG §5.3]'s fallback gives the other key, so every allowed value is reachable. Then:

- **`invariance`**: the model's reference stream (a session lease, creates, a claim and a completion, a fork and a write
  on the fork) gives results, commit ids, the `State` snapshots and the `Runtime` snapshot equal to those of the default
  configuration. This holds for every key the model reads nowhere ([API §2.5] DT-2), class V keys of a later package
  included until that package binds their function; the key's own effect is its checker's.
- **any other function**: a targeted case shows the value's effect through that function, as the function's
  `key-functions` row states.

Policy data are schema rows versioned per branch ([CFG §2.4]); [F08] gives them no schema item class yet, so the model
takes them as an input beside its configuration snapshot (`PolicyData`, [OP-1]).

Column notes:

- `key`, `row_name`: the pattern as [CFG §10] writes it; `instance`: the instance the sweep sets (a parameter segment
  takes the representative of its vocabulary: role `developer`, client `codex`, family `claude-opus-5-5`, profile
  `unknown`, root `docs`, destination `default`, kind `task`).
- `vis`: the class of [CFG §9.4]; for a key [CFG §10] gives two classes (KY-011), the first.
- `values`: [CFG §9.5]'s allowed-value set of the instance, the default first, canonical forms of [CFG §4.1].
- `checker`: the checker of the class `vis` (`key-checkers`), then the checker of the key's second class, if any.
- A row whose function belongs to a later package (`key-functions` `wp` other than WP-90) is **pending**, not
  tested: until that package binds the function, the model reads the key nowhere, so its values are run through the
  invariance of the reference stream ([API §2.5] DT-2), which says nothing about the function's effect. The model's
  test lists these rows by id and fails when the list changes, and it refuses a test arm for a function of a package
  it does not list as landed (review of WP-90b).

## 3. Checkers

<!-- table: key-checkers -->
| row | checker | vis | basis | source | definition |
|---|---|---|---|---|---|
| KC-001 | GT2 | V | design | [60 §3.13] GT2; [API §16] | The model differential: the engine's results, commit ids and digests equal the model's under every allowed value. |
| KC-002 | GT3 | V | design | [60 §3.13] GT3; [60 §4.4] item 4 | Crash semantics: after an OS crash the engine equals one of the model's candidate states. |
| KC-003 | SP-1 | I | design | [F17 §1.5] SP-1; [API §16.7] | Class-I invariance: two runs that differ only in class-I values give the same compared data. |
| KC-004 | SP-2 | Rs | design | [F17 §1.5] SP-2; [API §16.4] | A resource-class refusal is not compared; the harness counts such commands. |
| KC-005 | GT9 | B | design | [60 §3.13] GT9; [CFG §9.4] | Budget replay: the cut, the cursor and the `unverified` states. |
| KC-006 | GT12 | O, X | design | [60 §3.13] GT12; [CFG §9.4] | The golden files and the reference renderer (O); hook fixtures, `doctor hooks` and `integrate --check` (X). |
| KC-007 | GT7 | X | design | [60 §3.13] GT7; [CFG §9.4] | Image transport tests. |
| KC-008 | GT17 | X | design | [60 §3.13] GT17; [CFG §10.14] | The git index is outside the model. |
| KC-009 | discovery | X | design | [F02 §3]; [CFG §9.4] | The discovery tests of [F02 §3]. |

## 4. Model functions

<!-- table: key-functions -->
| row | function | wp | basis | source | definition |
|---|---|---|---|---|---|
| KF-001 | `invariance` | WP-90 | design | [API §2.5] DT-2; [API §16.7]; [F17 §1.5] SP-1; [CFG §9.4] | The model reads no such key: the reference stream of every allowed value gives the same results, commit ids, state and runtime snapshots as the default ([RULES/policy-keys] §2). The key's own checker (column `checker`) tests its effect. |
| KF-002 | `context::resolve_branch` | WP-90 | design | [API §4.2] CX-2; [CFG §10.1] | CX-2 in its order of record, `default-branch` last. |
| KF-003 | `derived::affected_with_budget` | WP-90 | design | [F17 §8.2]; [CFG §10.2] P23 | The commit's `affected` set, complete iff the suspects fit the budget. |
| KF-004 | `idem::Table::lookup` | WP-90 | design | [API §7.4]; [F17 §11.1]; [CFG §10.2] P28, P29 | The idempotency lookup with its two windows (CK-6). |
| KF-005 | `gc::reachable_after_gc` | WP-90 | design | [API §8.5]; [F17 §11.2]; [CFG §10.2] P30, P31 | Reachability at a `Gc` run; the dropped commits stop resolving (E301). |
| KF-006 | `crash::survives` | WP-90 | design | [CFG §10.3]; [F05 §6.1]; [API §10.2] | Whether an acknowledged record of a kind survives an OS crash; the deadlines a lease may have after one. |
| KF-007 | `quiet::in_quiet_mode` | WP-90 | design | [AR §6.6]; [API §8.3]; [CFG §10.3] | The flag, or a `measuring` lane while the key is true; quiet mode refuses `Gc` without `force`. |
| KF-008 | `lease::ttl_for` | WP-90 | design | [API §10.1]; [CFG §10.3] | The TTL of a new lease by its claim shape. |
| KF-009 | `lease::reclaim` | WP-90 | design | [API §10.4]; [CFG §10.3] | The leases `reclaim` releases. |
| KF-010 | `profile::read_safelist` | WP-90 | design | [CFG §10.5]; [RULES/role-write-policy WQ-003] | Whether a role may run only named queries; the binder's `named_only`. |
| KF-011 | `budget::check_caps` | WP-90 | design | [CFG §10.5]; [50 §3.10] item 10 | The deterministic caps of a `TX` block: statements at binding, net ops after the candidate. |
| KF-012 | `profile::model_profile` | WP-90 | design | [90 §8.2]; [CFG §4.3]; [CFG §10.9] | A session's profile from its declared model, else its client's default family. |
| KF-013 | `profile::model_write_rule` | WP-90 | design | [90 §8.1] L2; [CFG §10.9]; [RULES/role-write-policy WR-012] | A profile's write rule; E411 for a refused free-form `TX`. |
| KF-014 | `runs::open_policy` | WP-90 | design | [CFG §10.11]; [90 §7.1] | The runs the dispatcher opens for a Workflow run. |
| KF-015 | `budget::effective` | WP-90 | design | [CFG §10.5]; [CFG] open point 12 | A budget's effective value: the request or the default, clamped to the role's ceiling, `mem` and `wmem` against the headroom. The cuts are GT9's. |
| KF-016 | `pack::budget` | WP-90 | design | [RULES/pack-classes PK-001]; [RULES/pack-classes PB-001]; [CFG §10.8] | N of a pack kind for a role (PK and PB rows). |
| KF-017 | `pack::ceiling` | WP-90 | design | [RULES/pack-classes PE-001]; [CFG §10.8] | The ceiling of a surface under a client profile (PE rows). |
| KF-018 | `pack::quota` | WP-90 | design | [RULES/pack-classes PQ-001]; [CFG §10.8] | A class's minimum share of E for a role (PQ rows). |
| KF-019 | `pack::notice_mode` | WP-90 | design | [RULES/pack-classes NM-001]; [CFG §10.8] | The stale-pack notice mode (NM rows). |
| KF-020 | `hooks::installed` | WP-90 | design | [CFG §10.6]; [CFG §10.7]; [AR §13] "Sweep plan" hooks | The hooks `hooks install` registers for a harness. |
| KF-021 | `hooks::session_start` | WP-90 | design | [AR §7.5]; [RULES/role-write-policy WH-001] | The writes and the read of `SessionStart`. |
| KF-022 | `hooks::subagent_start` | WP-90 | design | [AR §7.5]; [RULES/role-write-policy WH-002] | The writes and the read of `SubagentStart`, the clean auto-sync included. |
| KF-023 | `hooks::stamp` | WP-90 | design | [AR §7.5]; [RULES/role-write-policy WH-006] | The stamp's `permissionDecision` for a write class. |
| KF-024 | `policy::Rights::mint` | WP-90 | design | [RULES/role-write-policy WM-001]; [CFG §10.13] | The `role-mint` rows with the self-claim roles and the minting roles. |
| KF-025 | `policy::narrowing_label` | WP-90 | design | [RULES/role-write-policy WR-007]; [CFG §10.13] | A hook label narrows a lease's rights, never widens them. |
| KF-026 | `policy::Rights::verb` | WP-90 | design | [RULES/role-write-policy WV-043]; [CFG §10.13] | The `role-verbs` rows, `mcp-write` by the roles whose `mcp-write` is yes. |
| KF-027 | `policy::Rights::statement` | WP-90 | design | [RULES/role-write-policy WX-001]; [CFG §10.13] | The `role-statements` rows, the restricted classes by the policy-data roles. |
| KF-028 | `policy::Rights::field` | WP-90 | design | [RULES/role-write-policy WF-007]; [CFG §10.13] | The `role-fields` rows, the developer's list by `policy.role.developer.fields`. |
| KF-029 | `policy::Rights::value` | WP-90 | design | [RULES/role-write-policy WA-001]; [CFG §10.13] | The `role-values` rows, `authority = owner` by the roles whose `authority-owner` is yes. |
| KF-030 | `delete::edge_policy` | WP-90 | design | [RULES/delete-policy-matrix EG-008]; [CFG §10.13] | The `edge-policy` row of an edge by the branch's `on-src-deleted` value. |
| KF-031 | `merge::land_or_stage` | WP-91 | design | [RULES/merge-table LS-001]; [CFG §10.11] | Step 8 of a merge (column `strict`). |
| KF-032 | `merge::auto_policy` | WP-91 | design | [RULES/merge-table AP-001]; [CFG §10.13] | A kind's automatic merge policy. |
| KF-033 | `links::root_dir` | WP-92 | design | [40 §2.4]; [CFG §10.1] | - |
| KF-034 | `links::designated_tree` | WP-92 | design | [40 §5.3]; [CFG §10.1] | - |
| KF-035 | `links::tree_gate` | WP-92 | design | [40 §5.3]; [CFG §10.1] | - |
| KF-036 | `links::cloud_policy` | WP-92 | design | [40 §4.6]; [CFG §10.1] | - |
| KF-037 | `links::content_available` | WP-92 | design | [F20]; [CFG §10.4] | - |
| KF-038 | `anchors::window_available` | WP-92 | design | [F20]; [CFG §10.4] | - |
| KF-039 | `links::auto_policy` | WP-92 | design | [40 §9.2]; [CFG §10.6] | - |
| KF-040 | `links::scratchpad_policy` | WP-92 | design | [40 §9.2]; [CFG §10.6] | - |
| KF-041 | `links::ignored` | WP-92 | design | [F20 §4.4]; [CFG §10.6] | - |
| KF-042 | `links::deletion_inference` | WP-92 | design | [40 §9.2]; [CFG §10.6] | - |
| KF-043 | `links::confirm_rights` | WP-92 | design | [40 §9.2]; [CFG §10.6] | - |
| KF-044 | `links::portable_name_policy` | WP-92 | design | [80 §2.10] P5; [CFG §10.6] | - |
| KF-045 | `image::export_set` | M5 | design | [AR §5b.8]; [CFG §10.10] | Group I is M5's ([API §2.2]); the model supplies states, changesets and commit ids at M0. |
| KF-046 | `image::anchor_text_on_import` | M5 | design | [40 §5.7]; [CFG §10.10] | As above. |
| KF-047 | `image::import_merge` | M5 | design | [AR §5b.6]; [CFG §10.10] | As above. |

## 5. Keys

<!-- table: policy-keys -->
| row | key | instance | vis | function | values | checker | basis | source | note |
|---|---|---|---|---|---|---|---|---|---|
| KY-001 | `discovery.git-hint` | `discovery.git-hint` | X | `invariance` | true, false | discovery | design | [CFG §10.1] | - |
| KY-002 | `default-branch` | `default-branch` | V | `context::resolve_branch` | main, lane/x | GT2 | design | [CFG §10.1] | The last step of CX-2 ([API §4.2]): a command with no other branch source resolves to it. |
| KY-003 | `roots.<name>` | `roots.docs` | V | `links::root_dir` | (unset), /work/x | GT2 | design | [CFG §10.1] | Maps a named root to a directory; unset renders `unmapped root`. |
| KY-004 | `files.main-tree` | `files.main-tree` | V | `links::designated_tree` | (unset), /work/x | GT2 | design | [CFG §10.1] | The designated tree of `main` at every re-bind and settle. |
| KY-005 | `files.main-ref` | `files.main-ref` | V | `links::tree_gate` | (unset), main | GT2 | design | [CFG §10.1] | The expected git ref of the main tree (I-F12). |
| KY-006 | `files.cloud` | `files.cloud` | V | `links::cloud_policy` | metadata-only, refuse | GT2 | design | [CFG §10.1] | Link creation and automatic resolution under a cloud root (I-F11). |
| KY-007 | `store.log-extent-bytes` | `store.log-extent-bytes` | I | `invariance` | 64MiB, 64KiB, 1GiB | SP-1 | design | [CFG §10.2] | - |
| KY-008 | `store.log-active-extents` | `store.log-active-extents` | I | `invariance` | 4, 1, 64, 2 | SP-1 | design | [CFG §10.2] | - |
| KY-009 | `store.hist-frame-commits` | `store.hist-frame-commits` | I | `invariance` | 256, 1, 65536, 4 | SP-1 | design | [CFG §10.2] | - |
| KY-010 | `store.hist-frame-bytes` | `store.hist-frame-bytes` | I | `invariance` | 1MiB, 4KiB | SP-1 | design | [CFG §10.2] | - |
| KY-011 | `store.commit.inline-max-bytes` | `store.commit.inline-max-bytes` | I | `invariance` | 1MiB, 4KiB, 128MiB | SP-1, SP-2 | design | [CFG §10.2]; [F17 §4.4] | [CFG §10.2] classes it "I; Rs for agent verbs": the write-size switch is class I (SP-1), and the bound it sets on an agent verb's inline payload is a resource-class refusal (E501) that SP-2 counts. `vis` holds the registry's first class; the second checker covers the Rs part (review of WP-90b). |
| KY-012 | `store.checkpoint.ops` | `store.checkpoint.ops` | I | `invariance` | 4096, 1, 1048576, 8 | SP-1 | design | [CFG §10.2] | - |
| KY-013 | `store.checkpoint.bytes` | `store.checkpoint.bytes` | I | `invariance` | 4MiB, 1KiB, 1GiB, 2KiB | SP-1 | design | [CFG §10.2] | - |
| KY-014 | `store.checkpoint.body-bytes` | `store.checkpoint.body-bytes` | I | `invariance` | 32MiB, 1KiB, 4GiB, 8KiB | SP-1 | design | [CFG §10.2] | - |
| KY-015 | `store.tail.max-overlay-bytes` | `store.tail.max-overlay-bytes` | I | `invariance` | 1MiB, 512, 64MiB, 1KiB | SP-1 | design | [CFG §10.2] | - |
| KY-016 | `store.tail.max-overlay-bytes.quiet` | `store.tail.max-overlay-bytes.quiet` | I | `invariance` | 2MiB, 512, 64MiB, 2KiB | SP-1 | design | [CFG §10.2] | - |
| KY-017 | `quiet.tail-cap-multiplier` | `quiet.tail-cap-multiplier` | I | `invariance` | 8, 1, 64, 2 | SP-1 | design | [CFG §10.2] | - |
| KY-018 | `store.tail.runtime-bytes` | `store.tail.runtime-bytes` | I | `invariance` | 2MiB, 512, 64MiB, 1KiB | SP-1 | design | [CFG §10.2] | - |
| KY-019 | `maintenance.cli-threshold-multiplier` | `maintenance.cli-threshold-multiplier` | I | `invariance` | 2, 1, 16 | SP-1 | design | [CFG §10.2] | - |
| KY-020 | `store.fold-width` | `store.fold-width` | I | `invariance` | 3, 1, 5, 2 | SP-1 | design | [CFG §10.2] | - |
| KY-021 | `maintenance.rollup-threshold` | `maintenance.rollup-threshold` | I | `invariance` | 25, 1, 1000 | SP-1 | design | [CFG §10.2] | - |
| KY-022 | `store.dict.train-sample-bytes` | `store.dict.train-sample-bytes` | I | `invariance` | 4MiB, 4KiB, 64MiB | SP-1 | design | [CFG §10.2] | - |
| KY-023 | `store.dict.retrain-growth` | `store.dict.retrain-growth` | I | `invariance` | 25, 1, 1000 | SP-1 | design | [CFG §10.2] | - |
| KY-024 | `store.fts.tier2-nodes` | `store.fts.tier2-nodes` | I | `invariance` | 20000, 1, 4294967295, 16 | SP-1 | design | [CFG §10.2] | - |
| KY-025 | `store.promotion.overlay-ops` | `store.promotion.overlay-ops` | I | `invariance` | 4000, 1, 4294967294, 16 | SP-1 | design | [CFG §10.2] | - |
| KY-026 | `store.promotion.overlay-bytes` | `store.promotion.overlay-bytes` | I | `invariance` | 1MiB, 1KiB, 4294967294 | SP-1 | design | [CFG §10.2] | - |
| KY-027 | `store.promotion.age-checkpoints` | `store.promotion.age-checkpoints` | I | `invariance` | 16, 1, 65536, 2 | SP-1 | design | [CFG §10.2] | - |
| KY-028 | `store.kahn-fallback-edges` | `store.kahn-fallback-edges` | I | `invariance` | 1000, 0, 4294967295, 4 | SP-1 | design | [CFG §10.2] | - |
| KY-029 | `store.suspect-budget` | `store.suspect-budget` | V | `derived::affected_with_budget` | 10000, 1, 4294967295, 4 | GT2 | design | [CFG §10.2] | Phase 1 of a write: more suspects than the budget leave `affected` incomplete ([F17 §8.2]). |
| KY-030 | `store.image.loose-pack-threshold` | `store.image.loose-pack-threshold` | I | `invariance` | 8, 0, 65536, 2 | SP-1 | design | [CFG §10.2] | - |
| KY-031 | `store.pack-objects-max` | `store.pack-objects-max` | I | `invariance` | 65536, 16, 16777216 | SP-1 | design | [CFG §10.2] | - |
| KY-032 | `lock.writer-wait-ms` | `lock.writer-wait-ms` | Rs | `invariance` | 2000, 100, 60000 | SP-2 | design | [CFG §10.2] | - |
| KY-033 | `lock.flush-wait-ms` | `lock.flush-wait-ms` | Rs | `invariance` | 2000, 100, 60000 | SP-2 | design | [CFG §10.2] | - |
| KY-034 | `idempotency.retention` | `idempotency.retention` | V | `idem::Table::lookup` | 30d, 1s, 3650d, 1h | GT2 | design | [CFG §10.2] | An explicit key older than this is ignored by the lookup ([API §7.4]; CK-6). |
| KY-035 | `idempotency.default-window` | `idempotency.default-window` | V | `idem::Table::lookup` | 10m, 1s, 3650d, 1m | GT2 | design | [CFG §10.2] | A default key older than this is ignored by the lookup; C-3 keeps it within the retention. |
| KY-036 | `gc.reflog-expire` | `gc.reflog-expire` | V | `gc::reachable_after_gc` | 90d, 1h, 3650d, 2h | GT2 | design | [CFG §10.2] | Reflog entries younger than this keep their commits reachable at a `Gc` run ([F17 §11.2]). |
| KY-037 | `gc.cruft-delay` | `gc.cruft-delay` | V | `gc::reachable_after_gc` | 14d, 0d, 3650d, 30m | GT2 | design | [CFG §10.2] | An unreachable commit younger than this survives a `Gc` run. |
| KY-038 | `gc.trash-expire` | `gc.trash-expire` | I | `invariance` | 14d, 0d, 3650d, 30m | SP-1 | design | [CFG §10.2] | - |
| KY-039 | `gc.fileobs-idle-expire` | `gc.fileobs-idle-expire` | I | `invariance` | 30d, 1h, 3650d | SP-1 | design | [CFG §10.2] | - |
| KY-040 | `gc.delete-grace` | `gc.delete-grace` | I | `invariance` | 1m, 0d, 1h, 1s | SP-1 | design | [CFG §10.2] | - |
| KY-041 | `durability.lazy-kinds` | `durability.lazy-kinds` | V | `crash::survives` | heartbeat,cursor,session-mark, (empty), heartbeat, cursor, session-mark | GT3 | design | [CFG §10.3]; [API §6.7] | The kinds whose records a crash may lose; a lazy heartbeat renewal may revert its deadline. No Store API command shows the effect: `EnvCrash` `between` is a process crash that keeps every lazy record, and no candidate of `in-next` loses one, so GT2 cannot observe the key and GT3 alone consumes `crash::survives`. Open for R-SPEC-F: an OS-crash form of [API §6.7] whose candidates include lost lazy records, or a class other than V in [CFG §10.3] (review of WP-90b). |
| KY-042 | `quiet.from-lane-measuring` | `quiet.from-lane-measuring` | V | `quiet::in_quiet_mode` | true, false | GT2 | design | [CFG §10.3] | A lane with status `measuring` implies quiet mode, which refuses `Gc` without `force`. |
| KY-043 | `maintenance.rollup` | `maintenance.rollup` | I | `invariance` | auto, explicit | SP-1 | design | [CFG §10.3] | - |
| KY-044 | `lease.ttl-default` | `lease.ttl-default` | V | `lease::ttl_for` | 15m, 1m, 30d | GT2 | design | [CFG §10.3] | A task claim without `ttl` takes this TTL; half of it is the renewal threshold. |
| KY-045 | `lease.reclaim-older-than` | `lease.reclaim-older-than` | V | `lease::reclaim` | 30m, 1m, 3650d | GT2 | design | [CFG §10.3] | A `Reclaim` with neither `older_than` nor `run` releases task leases claimed longer ago. |
| KY-046 | `lease.orchestrator-ttl` | `lease.orchestrator-ttl` | V | `lease::ttl_for` | 12h, 1m, 30d | GT2 | design | [CFG §10.3] | The session role lease's TTL where no slot anchors it. |
| KY-047 | `backup.max-age` | `backup.max-age` | O | `invariance` | 1d, 1h, 3650d | GT12 | design | [CFG §10.3] | - |
| KY-048 | `mcp.overlay-bytes` | `mcp.overlay-bytes` | I | `invariance` | 4MiB, 0, 256MiB | SP-1 | design | [CFG §10.4] | - |
| KY-049 | `mcp.overlay-bytes.<client>` | `mcp.overlay-bytes.codex` | I | `invariance` | 0, 256MiB | SP-1 | design | [CFG §10.4] | - |
| KY-050 | `mcp.overlay-lru` | `mcp.overlay-lru` | I | `invariance` | 8, 1 | SP-1 | design | [CFG §10.4] | - |
| KY-051 | `git.delta-cache-bytes.cli` | `git.delta-cache-bytes.cli` | I | `invariance` | 256KiB, 0, 64MiB | SP-1 | design | [CFG §10.4] | - |
| KY-052 | `git.delta-cache-bytes.mcp` | `git.delta-cache-bytes.mcp` | I | `invariance` | 1MiB, 0, 64MiB | SP-1 | design | [CFG §10.4] | - |
| KY-053 | `mem.rss-gate.cli` | `mem.rss-gate.cli` | Rs | `invariance` | 4000000, 1MiB, 1GiB | SP-2 | design | [CFG §10.4] | - |
| KY-054 | `mem.rss-gate.cli-per-view` | `mem.rss-gate.cli-per-view` | Rs | `invariance` | 1MiB, 0, 64MiB | SP-2 | design | [CFG §10.4] | - |
| KY-055 | `mem.rss-gate.mcp` | `mem.rss-gate.mcp` | Rs | `invariance` | 15625KiB, 1MiB, 1GiB | SP-2 | design | [CFG §10.4] | - |
| KY-056 | `files.max-read-bytes` | `files.max-read-bytes` | V | `links::content_available` | 16MiB, 128KiB, 4GiB | GT2 | design | [CFG §10.4] | A larger project file is `Unavailable(size)`. |
| KY-057 | `files.max-line-hashes` | `files.max-line-hashes` | V | `anchors::window_available` | 65536, 1024, 16777216 | GT2 | design | [CFG §10.4] | Beyond it, window-only anchors are `unverified (size)`. |
| KY-058 | `files.deep.threads` | `files.deep.threads` | I | `invariance` | 8, 1, 64 | SP-1 | design | [CFG §10.4] | - |
| KY-059 | `files.deep.content-readers` | `files.deep.content-readers` | I | `invariance` | 2, 1, 8 | SP-1 | design | [CFG §10.4] | - |
| KY-060 | `query.budget.default.work` | `query.budget.default.work` | B | `budget::effective` | 2000000, 1, 10000000000 | GT9 | design | [CFG §10.5] | - |
| KY-061 | `query.budget.default.mem` | `query.budget.default.mem` | B | `budget::effective` | 1MiB, 256KiB, 1GiB | GT9 | design | [CFG §10.5] | - |
| KY-062 | `query.budget.default.wmem` | `query.budget.default.wmem` | B | `budget::effective` | 1MiB, 256KiB, 1GiB | GT9 | design | [CFG §10.5] | - |
| KY-063 | `query.budget.default.rows` | `query.budget.default.rows` | B | `budget::effective` | 50, 1, 1000000 | GT9 | design | [CFG §10.5] | - |
| KY-064 | `query.budget.default.bytes` | `query.budget.default.bytes` | B | `budget::effective` | 8000, 1000, 10000000 | GT9 | design | [CFG §10.5] | - |
| KY-065 | `query.budget.default.visited` | `query.budget.default.visited` | B | `budget::effective` | 100000, 1, 1000000000 | GT9 | design | [CFG §10.5] | - |
| KY-066 | `query.budget.default.refs` | `query.budget.default.refs` | B | `budget::effective` | 4, 1, 80 | GT9 | design | [CFG §10.5] | - |
| KY-067 | `query.budget.default.fs` | `query.budget.default.fs` | B | `budget::effective` | 400, 1, 1000000000 | GT9 | design | [CFG §10.5] | - |
| KY-068 | `query.budget.default.deadline-cli` | `query.budget.default.deadline-cli` | B | `budget::effective` | 2s, 100ms, 1h | GT9 | design | [CFG §10.5] | - |
| KY-069 | `query.budget.default.deadline-mcp` | `query.budget.default.deadline-mcp` | B | `budget::effective` | 5s, 100ms, 1h | GT9 | design | [CFG §10.5] | - |
| KY-070 | `query.caps.<role>.work` | `query.caps.developer.work` | B | `budget::effective` | 20000000, 1, 10000000000 | GT9 | design | [CFG §10.5] | - |
| KY-071 | `query.caps.<role>.mem` | `query.caps.developer.mem` | B | `budget::effective` | 2MiB, 256KiB, 1GiB | GT9 | design | [CFG §10.5] | - |
| KY-072 | `query.caps.<role>.wmem` | `query.caps.developer.wmem` | B | `budget::effective` | 4MiB, 256KiB, 1GiB | GT9 | design | [CFG §10.5] | - |
| KY-073 | `query.caps.<role>.rows` | `query.caps.developer.rows` | B | `budget::effective` | 500, 1, 1000000 | GT9 | design | [CFG §10.5] | - |
| KY-074 | `query.caps.<role>.bytes` | `query.caps.developer.bytes` | B | `budget::effective` | 24000, 1000, 10000000 | GT9 | design | [CFG §10.5] | - |
| KY-075 | `query.caps.<role>.visited` | `query.caps.developer.visited` | B | `budget::effective` | 1000000, 1, 1000000000 | GT9 | design | [CFG §10.5] | - |
| KY-076 | `query.caps.<role>.refs` | `query.caps.developer.refs` | B | `budget::effective` | 8, 1, 80 | GT9 | design | [CFG §10.5] | - |
| KY-077 | `query.caps.<role>.fs` | `query.caps.developer.fs` | B | `budget::effective` | 10000, 1, 1000000000 | GT9 | design | [CFG §10.5] | - |
| KY-078 | `query.caps.<role>.deadline-cli` | `query.caps.developer.deadline-cli` | B | `budget::effective` | 2s, 100ms, 1h | GT9 | design | [CFG §10.5] | - |
| KY-079 | `query.caps.<role>.deadline-mcp` | `query.caps.developer.deadline-mcp` | B | `budget::effective` | 5s, 100ms, 1h | GT9 | design | [CFG §10.5] | - |
| KY-080 | `query.safelist.<role>` | `query.safelist.developer` | V | `profile::read_safelist` | off, named-only | GT2 | design | [CFG §10.5] | `named-only`: free-form LQ reads by the role are E406 (WQ-003). |
| KY-081 | `query.asof.max-ops.cli` | `query.asof.max-ops.cli` | B | `invariance` | 16000, 0, 10000000 | GT9 | design | [CFG §10.5] | - |
| KY-082 | `query.asof.max-ops.mcp` | `query.asof.max-ops.mcp` | B | `invariance` | 100000, 0, 10000000 | GT9 | design | [CFG §10.5] | - |
| KY-083 | `files.read.max-uncached-ancestry` | `files.read.max-uncached-ancestry` | B | `invariance` | 1, 0, 64 | GT9 | design | [CFG §10.5] | - |
| KY-084 | `files.read.max-e6-commits` | `files.read.max-e6-commits` | B | `invariance` | 32, 0, 4096 | GT9 | design | [CFG §10.5] | - |
| KY-085 | `input.max-bytes` | `input.max-bytes` | O | `invariance` | 16MiB, 64KiB, 1GiB | GT12 | design | [CFG §10.5] | - |
| KY-086 | `tx.max-statements` | `tx.max-statements` | V | `budget::check_caps` | 1000, 1, 1000000 | GT2 | design | [CFG §10.5] | A larger `TX` block is E501 naming the split, at binding. |
| KY-087 | `tx.max-ops` | `tx.max-ops` | V | `budget::check_caps` | 10000, 1, 1000000 | GT2 | design | [CFG §10.5] | A `TX` whose net changeset has more ops is E501 naming the split. |
| KY-088 | `tx.max-work-in-lock` | `tx.max-work-in-lock` | I | `invariance` | 500000, 0, 1000000000 | SP-1 | design | [CFG §10.5] | - |
| KY-089 | `files.policy.auto` | `files.policy.auto` | V | `links::auto_policy` | exact, strong | GT2 | design | [CFG §10.6] | `strong` also applies unique strong re-bind candidates, marked as guesses. |
| KY-090 | `files.scratchpads` | `files.scratchpads` | V | `links::scratchpad_policy` | refuse, allow | GT2 | design | [CFG §10.6] | `link --at` and `file add` of a scratchpad path. |
| KY-091 | `files.ignore` | `files.ignore` | V | `links::ignored` | target/,node_modules/,build/, (empty), *.tmp | GT2 | design | [CFG §10.6] | The ignore matcher of a tree with no git and no ignore file. |
| KY-092 | `files.read-budget-ms` | `files.read-budget-ms` | B | `invariance` | 20, 1, 60000 | GT9 | design | [CFG §10.6] | - |
| KY-093 | `files.session-start-cap-ms` | `files.session-start-cap-ms` | B | `invariance` | 150, 1, 10000 | GT9 | design | [CFG §10.6] | - |
| KY-094 | `files.links-sync-ms` | `files.links-sync-ms` | B | `invariance` | 2000, 1, 3600000 | GT9 | design | [CFG §10.6] | - |
| KY-095 | `files.deep.budget-ms` | `files.deep.budget-ms` | B | `invariance` | 10000, 1, 3600000 | GT9 | design | [CFG §10.6] | - |
| KY-096 | `mcp.links-sync-slice-ms` | `mcp.links-sync-slice-ms` | I | `invariance` | 200, 1, 1000 | SP-1 | design | [CFG §10.6] | - |
| KY-097 | `files.settle.others-after` | `files.settle.others-after` | X | `invariance` | 1d, 0d, 3650d | GT12 | design | [CFG §10.6] | - |
| KY-098 | `files.pending-escalate` | `files.pending-escalate` | O | `invariance` | 14d, 1h, 3650d | GT12 | design | [CFG §10.6] | - |
| KY-099 | `files.deletion-inference` | `files.deletion-inference` | V | `links::deletion_inference` | explicit, main-tree-commits | GT2 | design | [CFG §10.6] | `main-tree-commits` records git deletions as `removed` (I-F7). |
| KY-100 | `files.mv-git` | `files.mv-git` | X | `invariance` | false, true | GT17 | design | [CFG §10.6] | - |
| KY-101 | `files.confirm-roles` | `files.confirm-roles` | V | `links::confirm_rights` | orchestrator,owner, (empty), developer | GT2 | design | [CFG §10.6] | Who may confirm an agent's guess with `links fix --confirm`. |
| KY-102 | `files.portable-names` | `files.portable-names` | V | `links::portable_name_policy` | refuse, warn | GT2 | design | [CFG §10.6] | Names some OS cannot hold: refused or warned. |
| KY-103 | `files.hooks.evidence` | `files.hooks.evidence` | X | `hooks::installed` | true, false | GT12 | design | [CFG §10.6] | The `mv`/`rm` evidence hook is registered only when true. |
| KY-104 | `files.hooks.edit-evidence` | `files.hooks.edit-evidence` | X | `hooks::installed` | auto, on, off | GT12 | design | [CFG §10.6] | The `Write\|Edit` evidence hook: `on`, `off`, or with the `mcp_tool` transport. |
| KY-105 | `hooks.transport` | `hooks.transport` | X | `hooks::installed` | auto, mcp, command | GT12 | design | [CFG §10.7] | The handler kind; `auto` is `mcp_tool` in Claude Code and Codex, which `files.hooks.edit-evidence = auto` follows. |
| KY-106 | `hooks.session-start.enabled` | `hooks.session-start.enabled` | X | `hooks::installed` | true, false | GT12 | design | [CFG §10.7] | `hooks install` registers the hook only when true. |
| KY-107 | `hooks.user-prompt-submit.enabled` | `hooks.user-prompt-submit.enabled` | X | `hooks::installed` | true, false | GT12 | design | [CFG §10.7] | As above. |
| KY-108 | `hooks.subagent-start.enabled` | `hooks.subagent-start.enabled` | X | `hooks::installed` | true, false | GT12 | design | [CFG §10.7] | As above. |
| KY-109 | `hooks.agent-launched.enabled` | `hooks.agent-launched.enabled` | X | `hooks::installed` | true, false | GT12 | design | [CFG §10.7] | As above; Claude Code only. |
| KY-110 | `hooks.subagent-stop.enabled` | `hooks.subagent-stop.enabled` | X | `hooks::installed` | true, false | GT12 | design | [CFG §10.7] | As above. |
| KY-111 | `hooks.stamp.enabled` | `hooks.stamp.enabled` | X | `hooks::installed` | true, false | GT12 | design | [CFG §10.7] | As above; Claude Code only. |
| KY-112 | `hooks.session-start.settle` | `hooks.session-start.settle` | X | `hooks::session_start` | true, false | GT12 | design | [CFG §10.7] | `SessionStart` settles links (WH-001; the settle is WP-92's). |
| KY-113 | `hooks.session-start.path-export` | `hooks.session-start.path-export` | X | `invariance` | true, false | GT12 | design | [CFG §10.7] | - |
| KY-114 | `hooks.session-start.worker-pack` | `hooks.session-start.worker-pack` | X | `hooks::session_start` | true, false | GT12 | design | [CFG §10.7] | A dispatched worker's `SessionStart` renders the role pack, not the brief. |
| KY-115 | `hooks.session-start.orchestrator-lease` | `hooks.session-start.orchestrator-lease` | X | `hooks::session_start` | true, false | GT12 | design | [CFG §10.7] | A main session's `SessionStart` mints the session role lease (WH-001). |
| KY-116 | `hooks.subagent-start.auto-sync` | `hooks.subagent-start.auto-sync` | X | `hooks::subagent_start` | true, false | GT12 | design | [CFG §10.7] | `SubagentStart` applies a clean `sync` preview (WH-002). |
| KY-117 | `hooks.sync-auto-keys` | `hooks.sync-auto-keys` | X | `hooks::subagent_start` | 2000, 0, 1000000 | GT12 | design | [CFG §10.7] | The largest preview `SubagentStart` applies; 0 never applies. |
| KY-118 | `hooks.stamp.permission` | `hooks.stamp.permission` | X | `hooks::stamp` | allow, ask | GT12 | design | [CFG §10.7] | The stamp's `permissionDecision`. |
| KY-119 | `hooks.stamp.ask-for` | `hooks.stamp.ask-for` | X | `hooks::stamp` | owner-authority, (empty), edge-delete, links-confirm, owner-authority,edge-delete,links-confirm | GT12 | design | [CFG §10.7] | The write classes for which the stamp answers `ask`. |
| KY-120 | `hooks.delta.max-commits` | `hooks.delta.max-commits` | O | `invariance` | 2000, 1, 1000000 | GT12 | design | [CFG §10.7] | - |
| KY-121 | `pack.budget.<role>` | `pack.budget.developer` | O | `pack::budget` | 16000, 1000, 1000000 | GT12 | design | [CFG §10.8] | N of a role pack without `--budget` ([RULES/pack-classes] PB rows). |
| KY-122 | `pack.cli.max-bytes` | `pack.cli.max-bytes` | O | `pack::ceiling` | 24000, 1000, 28000 | GT12 | design | [CFG §10.8] | The CLI ceiling (PE-001); a user file can only lower it. |
| KY-123 | `pack.mcp.max-bytes` | `pack.mcp.max-bytes` | O | `pack::ceiling` | 25000, 1000, 48000 | GT12 | design | [CFG §10.8] | The MCP ceiling (PE-003 to PE-005), capped by the profile's MCP result ceiling. |
| KY-124 | `pack.quota.c2` | `pack.quota.c2` | O | `pack::quota` | 15, 0, 100 | GT12 | design | [CFG §10.8] | C2's minimum share of E (PQ-001); K-1. |
| KY-125 | `pack.quota.c3` | `pack.quota.c3` | O | `pack::quota` | 20, 0, 100 | GT12 | design | [CFG §10.8] | C3's minimum share (PQ-002); K-1. |
| KY-126 | `pack.quota.c4-dev` | `pack.quota.c4-dev` | O | `pack::quota` | 30, 0, 100 | GT12 | design | [CFG §10.8] | C4's share for developer and tester (PQ-003); K-1. |
| KY-127 | `pack.quota.c4-critic` | `pack.quota.c4-critic` | O | `pack::quota` | 40, 0, 100 | GT12 | design | [CFG §10.8] | C4's share for the architecture-critic (PQ-004); K-1. |
| KY-128 | `pack.quota.c5` | `pack.quota.c5` | O | `pack::quota` | 10, 0, 100 | GT12 | design | [CFG §10.8] | C5's minimum share (PQ-006); K-1. |
| KY-129 | `pack.staleness-notice` | `pack.staleness-notice` | O | `pack::notice_mode` | lines, off, ids | GT12 | design | [CFG §10.8] | The notice mode of `complete` and `apply` given a pack digest (NM rows). |
| KY-130 | `brief.budget` | `brief.budget` | O | `pack::budget` | 8000, 1000, 9500 | GT12 | design | [CFG §10.8] | N of `brief` and the `SessionStart` brief (PK-005). |
| KY-131 | `brief.lang` | `brief.lang` | O | `invariance` | en, ru | GT12 | design | [CFG §10.8] | - |
| KY-132 | `hooks.subagent-start.budget` | `hooks.subagent-start.budget` | O | `pack::budget` | 3000, 1000, 10000 | GT12 | design | [CFG §10.8] | N of the hook role pack (PK-004). |
| KY-133 | `hooks.delta.budget` | `hooks.delta.budget` | O | `pack::budget` | 600, 100, 10000 | GT12 | design | [CFG §10.8] | N of the prompt and resume deltas (PK-007, PK-008). |
| KY-134 | `mcp.always-load` | `mcp.always-load` | X | `invariance` | (empty), brief, pack, get, query, changes, branch, claim, complete, remember, write, brief,pack,get,query,changes,branch,claim,complete,remember,write | GT12 | design | [CFG §10.8] | - |
| KY-135 | `mcp.result-max-bytes` | `mcp.result-max-bytes` | O | `pack::ceiling` | 25000, 1000, 48000 | GT12 | design | [CFG §10.8] | Every MCP result's ceiling, and the default of `mcp.result-max-bytes.<client>`. |
| KY-136 | `mcp.result-max-bytes.<client>` | `mcp.result-max-bytes.codex` | O | `pack::ceiling` | 16000, 1000, 36000 | GT12 | design | [CFG §10.8] | The MCP result ceiling of a client profile; `codex` at most 36,000. |
| KY-137 | `output.nonzero-exit-max-bytes` | `output.nonzero-exit-max-bytes` | O | `invariance` | 8000, 1000, 10000 | GT12 | design | [CFG §10.8] | - |
| KY-138 | `output.nonzero-exit-max-bytes.<client>` | `output.nonzero-exit-max-bytes.codex` | O | `invariance` | 8000, 1000, 10000 | GT12 | design | [CFG §10.8] | - |
| KY-139 | `mcp.ids-page-bytes` | `mcp.ids-page-bytes` | O | `invariance` | 8000, 1000, 25000 | GT12 | design | [CFG §10.8] | - |
| KY-140 | `output.ids-max-bytes` | `output.ids-max-bytes` | O | `invariance` | 24000, 0, 1000000000 | GT12 | design | [CFG §10.8] | - |
| KY-141 | `export.memory-md` | `export.memory-md` | X | `invariance` | auto, full, pointer | GT12 | design | [CFG §10.8] | - |
| KY-142 | `client.profile` | `client.profile` | O | `invariance` | auto, claude, codex, generic | GT12 | design | [CFG §10.9] | - |
| KY-143 | `mcp.tools` | `mcp.tools` | X | `invariance` | all, read, core | GT12 | design | [CFG §10.9] | - |
| KY-144 | `integrate.instructions-scope` | `integrate.instructions-scope` | X | `invariance` | project, user | GT12 | design | [CFG §10.9] | - |
| KY-145 | `integrate.claude-md` | `integrate.claude-md` | X | `invariance` | import, copy | GT12 | design | [CFG §10.9] | - |
| KY-146 | `integrate.codex.store-writes` | `integrate.codex.store-writes` | X | `invariance` | writable-root, execpolicy-store, execpolicy, mcp | GT12 | design | [CFG §10.9] | - |
| KY-147 | `integrate.codex.approval` | `integrate.codex.approval` | X | `invariance` | split, prompt, writes, approve | GT12 | design | [CFG §10.9] | - |
| KY-148 | `integrate.hooks` | `integrate.hooks` | X | `invariance` | full, none, min | GT12 | design | [CFG §10.9] | - |
| KY-149 | `lq.model-profile.<family>` | `lq.model-profile.claude-opus-5-5` | V | `profile::model_profile` | gated, compatible, unknown | GT2 | design | [CFG §10.9] | The profile of a family: `unknown` writes named mutations only under the default write rule. |
| KY-150 | `lq.model-profile.default.<client>` | `lq.model-profile.default.codex` | V | `profile::model_profile` | unknown, gpt-5-6-luna | GT2 | design | [CFG §10.9] | The family of a session that declares no model (CX-6). |
| KY-151 | `query.safelist.model.<profile>` | `query.safelist.model.unknown` | V | `profile::model_write_rule` | named-only, off, dry-targets | GT2 | design | [CFG §10.9] | The write rule of a profile: `named-only` refuses free-form `TX` with E411 (WR-012). |
| KY-152 | `image.dest.<name>.path` | `image.dest.default.path` | X | `invariance` | (unset), /work/x | GT7 | design | [CFG §10.10] | - |
| KY-153 | `image.dest.<name>.refs` | `image.dest.default.refs` | V | `image::export_set` | main,tags/*,lane/*, (empty), *.tmp | GT2 | design | [CFG §10.10] | Which refs an export copies. |
| KY-154 | `image.dest.<name>.granularity` | `image.dest.default.granularity` | V | `image::export_set` | checkpoint, commit | GT2 | design | [CFG §10.10] | Which commits an export writes. |
| KY-155 | `image.dest.<name>.object-format` | `image.dest.default.object-format` | X | `invariance` | sha1, sha256 | GT7 | design | [CFG §10.10] | - |
| KY-156 | `image.dest.<name>.kind` | `image.dest.default.kind` | X | `invariance` | bare-repo | GT7 | design | [CFG §10.10] | - |
| KY-157 | `image.dest.<name>.anchor-text` | `image.dest.default.anchor-text` | V | `image::anchor_text_on_import` | full, hash-only | GT2 | design | [CFG §10.10] | An import of a `hash-only` anchor gives `text-unavailable`. |
| KY-158 | `image.dest.<name>.git.pack-threads` | `image.dest.default.git.pack-threads` | I | `invariance` | 2, 1, 64 | SP-1 | design | [CFG §10.10] | - |
| KY-159 | `image.dest.<name>.git.pack-window-memory` | `image.dest.default.git.pack-window-memory` | I | `invariance` | 64MiB, 1MiB, 4GiB | SP-1 | design | [CFG §10.10] | - |
| KY-160 | `image.export.on-merge-to-main` | `image.export.on-merge-to-main` | X | `invariance` | true, false | GT7 | design | [CFG §10.10] | - |
| KY-161 | `image.export.max-age` | `image.export.max-age` | X | `invariance` | 1d, 1h, 3650d | GT7 | design | [CFG §10.10] | - |
| KY-162 | `image.import-merge` | `image.import-merge` | V | `image::import_merge` | auto, stage | GT2 | design | [CFG §10.10] | A divergent import merges by the typed rules or always stages. |
| KY-163 | `image.allowed-remotes` | `image.allowed-remotes` | X | `invariance` | (empty), https://example.invalid/image.git | GT7 | design | [CFG §10.10] | - |
| KY-164 | `image.transport.spawn-git` | `image.transport.spawn-git` | X | `invariance` | true, false | GT7 | design | [CFG §10.10] | - |
| KY-165 | `merge.strict` | `merge.strict` | V | `merge::land_or_stage` | false, true | GT2 | design | [CFG §10.11] | Value conflicts land (`false`) or stage (`true`): [RULES/merge-table] `land-or-stage` column `strict`. |
| KY-166 | `runs.granularity` | `runs.granularity` | V | `runs::open_policy` | workflow, agent-call | GT2 | design | [CFG §10.11] | The dispatcher opens one run per Workflow run, or one per agent call. |

## 6. Policy data

<!-- table: policy-rows -->
| row | row_name | instance | function | values | basis | source | note |
|---|---|---|---|---|---|---|---|
| PV-001 | `policy.self-claim-roles` | `policy.self-claim-roles` | `policy::Rights::mint` | developer,tester, (empty), developer | design | [CFG §10.13]; [RULES/role-write-policy WM-001]; [RULES/role-write-policy WM-003] | A self-claim for a role outside the set needs the orchestrator or the owner; a role-less self-claim is `developer`. |
| PV-002 | `policy.mint.role-lease` | `policy.mint.role-lease` | `policy::Rights::mint` | orchestrator,owner, (empty), owner | design | [CFG §10.13]; [RULES/role-write-policy WM-004] | Who may mint run-scoped role leases and bulk claims. |
| PV-003 | `policy.hook-label` | `policy.hook-label` | `policy::narrowing_label` | narrow | design | [CFG §10.13]; [90 §10.8] | A design rule listed for completeness: its only value. |
| PV-004 | `policy.role.<role>.mcp-write` | `policy.role.developer.mcp-write` | `policy::Rights::verb` | no, yes | design | [CFG §10.13]; [RULES/role-write-policy WV-043] | MCP writes by the role. |
| PV-005 | `policy.role.<role>.tx` | `policy.role.owner.tx` | `policy::Rights::statement` | per-statement, none | proposed | [CFG §10.13]; [RULES/role-write-policy WX-001]; [OP-2] | `per-statement`: the `role-statements` rows whose key it is; `none`: none of those classes ([OP-2]). |
| PV-006 | `policy.role.developer.fields` | `policy.role.developer.fields` | `policy::Rights::field` | files_owned, (empty), files_owned,title | design | [CFG §10.13]; [RULES/role-write-policy WF-007] | The fields a developer may set on its leased task. |
| PV-007 | `policy.role.<role>.define-query` | `policy.role.orchestrator.define-query` | `policy::Rights::statement` | yes, no | design | [CFG §10.13]; [RULES/role-write-policy WX-003] | `DEFINE QUERY` and `DROP QUERY`. |
| PV-008 | `policy.role.<role>.authority-owner` | `policy.role.orchestrator.authority-owner` | `policy::Rights::value` | yes, no | design | [CFG §10.13]; [RULES/role-write-policy WA-001] | Writes with `authority = owner`, with WA-001's attestation and quote. |
| PV-009 | `edges.blocks.on-src-deleted` | `edges.blocks.on-src-deleted` | `delete::edge_policy` | flag, drop-notify | design | [CFG §10.13]; [RULES/delete-policy-matrix EG-008] | A deleted `blocks` source: a flagged retained edge, or removed with a notice. |
| PV-010 | `edges.gates.on-src-deleted` | `edges.gates.on-src-deleted` | `delete::edge_policy` | flag, drop-notify | design | [CFG §10.13]; [RULES/delete-policy-matrix EG-014] | As above for `gates`. |
| PV-011 | `merge.policy.<kind>` | `merge.policy.task` | `merge::auto_policy` | none, ours, theirs, delete-wins, resurrect | design | [CFG §10.13]; [RULES/merge-table AP-001] | Opt-in per kind; `merge --policy` overrides it. |

## Coverage

| Item | Part covered here | Section |
|---|---|---|
| [CFG §9.2] RG-6: every class-V key has a model function | complete: one `policy-keys` row per key pattern, the V rows bound to the functions of `key-functions` | §4, §5 |
| [CFG §9.5] the sweep plan's allowed-value sets, one at a time | complete for every key pattern and policy-data row; the pairs and the hook sweep are the checkers' (GT2, GT12) | §2, §5, §6 |
| [CFG §10.13] policy data | complete | §6 |
| [CFG §10.14] / [60 §3.14] the 25 former owner questions | every key or row they name has its row here, with its function or checker | §5, §6 |
| [m0/PLAN §3.2] WP-90 `rules/policy-keys`; [m0/PLAN §7] E8 "every allowed value of each policy key" | complete for WP-90's functions; the rows of WP-91, WP-92 and M5 are tested by `invariance` until their packages bind them | §2 |

No R-row, F-row or X-F row concerns this file.

## Holes

None of its own. Defaults written `HOLE(…)` in [CFG] and [F17] enter the sweep with the design value the hole names (the
first candidate where [CFG] names no design value: `gated` for `CFG-model-profile-opus`); WP-81a's value replaces it.

## Open points for the review

1. **Policy data as model input.** [CFG §2.4] and [AR §13] make policy data schema rows versioned per branch, but
   [F08 §8.5] has no item class for them and [API] no command that writes them. The model takes them as an input
   (`PolicyData`) beside the configuration snapshot, and each role-policy row whose `key` names one takes its role cell
   from it. Proposed for WP-14: a schema item class for policy-data rows, so a schema write changes them per branch.
2. **`policy.role.<role>.tx` has no value set.** [CFG §10.13] gives "the per-statement policy" as its value. This table
   sweeps two values: `per-statement` (the `role-statements` rows whose key it is) and `none` (the role may use none of
   those classes). [CFG §10.13]'s default also says "bulk targets for `orchestrator` only", while
   [RULES/role-write-policy] WX-005 admits `orchestrator, owner`; the model follows WX-005.
3. **Class-O and class-X keys with a model function.** The pack budgets, ceilings, quotas and notice mode, and the hook
   keys, change output or harness behaviour only ([CFG §9.4] O, X), but the model computes them as data from
   [RULES/pack-classes] and [RULES/role-write-policy] `role-hooks`, so each has a per-value test here beside its checker.
4. **`client.profile` is class O but selects a write rule.** Its value is the client of CX-7, whose default family
   (`lq.model-profile.default.<client>`) decides the model profile and so E411 (class V). The model resolves CX-7 from
   `ctx.client` and `MOIRAI_CLIENT` only ([API §4.2]), which are the key's flag and variable ([CFG §10.12]), never from
   the file value. The review decides whether the key is class V or CX-7 ignores the file.
