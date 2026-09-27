# 13 — Invariants

| | |
|---|---|
| Title | Invariants: every invariant of the design of record with its enforcement point, check class, reference-model function and gates; the I26′ state definition and marker-cache rules; the I37′ validator order; derived-state semantics (F15) |
| Chapter | [F13], `docs/spec/format/13-invariants.md` |
| Status | draft, pass 1 pending |
| Work package | WP-16c, the invariants part of WP-16 ([PLAN §3.2] item 1), author role R-SPEC-P |
| Sources | [AR §3.4] (every invariant, including the table heading's re-check rule), [AR §3.5] (derived state), [AR §3.6] (status machines), [AR §4.5] steps 1–12 (the enforcement points of the write path), [AR §4.6], [AR §4.9], [AR §4.10], [AR §5a.1]–[AR §5a.9] (merge, validators, markers, `undo`, `-D`), [AR §5b.4]–[AR §5b.7] (image round trip and gates 0–3), [AR §5d.1], [AR §5e.8], [AR §6.2]–[AR §6.4], [AR §8.2] "Correctness gates", [AR §8.3] CORRECTNESS table (row "Invariant coverage"); [21 §8] (I35′, which [AR §3.4] I1 cites); [40 §2.10] (I-F1…I-F14, authoritative), [40 §8.3.2] (P1–P15), [40 §8.1] (FL placement); [80 §2.4.3] ("Invariants": I-G1–I-G6), [80 §2.4.4]; [50 §3.8], [50 §5.8], [50 §5.9], [50 §8.1] F15, F16, F18; [60 §2.5] rows "Derived-state semantics" and "Protocol decisions" (k); [60 §2.6] (validator table); [60 §3.13] (gate catalogue); [60 §4.2]–[60 §4.4]; [72] M4 (I26′ on states). |
| Depends on | [F01]; cites [F04], [F05], [F06], [F08], [F11], [F12], [F14], [F15], [F16], [F17], [F18], [F19], [F20], [API] |

This chapter lists every invariant of the design of record. For each one it gives the enforcement point, the check class,
the reference-model function that states it (to be written by R-MODEL in WP-90–WP-94) and the gates that test it. It also
fixes three things the invariants rest on:

- §4: the state definition of I26′ and the rules of its marker cache;
- §5: the validator order of I37′, which is the validator table that [60 §2.6] certifies as an extension point;
- §6: the derived-state semantics of [50] F15, the `affected` list and `affected_complete` (I42′).

## 1. Scope and conventions

### 1.1 What is listed

| Group | Invariants | Source |
|---|---|---|
| Design of record | I1–I14, I14′, I17′, I18′, I25′–I34′, I36′–I43′, I-P3 | [AR §3.4] |
| Cited by I1 | I35′ | [21 §8]; [AR §3.4] I1 and [AR §5b.6] cite it; it is added here so that the citation resolves |
| File links (R-12) | I-F1…I-F14 | [40 §2.10], authoritative; [AR §5e.8] summarises it. [F18] carries R-12 and cross-lists these rows; this chapter is where each row gets its enforcement point, model function and gates |
| Group commit (X-F3) | I-G1–I-G6 | [80 §2.4.3] |
| The I37′ order | validators V01–V13 | [AR §5a.7] step 6, [60 §2.6] (§5) |
| Derived-state semantics (F15) | the predicate set `P_F15`, `affected`, `affected_complete` | [60 §2.5], [50 §3.8], [50 §5.8], [50] F15 and F16 (§6) |

The numbers I15′, I16′ and I19′–I24′ belong to proposal D's numbering ([13 §3]) and were not carried into [AR]. They are not
invariants of the design of record, and no row is kept for them (OP-13-01).

### 1.2 Fields of an entry

- **Invariant.** The statement, restated normatively. On wording, the cited source wins.
- **Enforcement.** One or more enforcement points from §2, each tagged with a check class from §1.3.
- **Model function.** The function in crate `moirai-model` that states the invariant, as a Rust path under `moirai_model::`.
  Every such function carries the tag `// spec: [F13 §x] <ID>`, which `xtask coverage` checks ([PLAN §3.2] item 1).
- **Gates.** Gate ids of [60 §3.13] and the named rows of [AR §8.3], each with the milestone from which it is mandatory.
  Every invariant has at least one gate. The [AR §8.3] row "Invariant coverage" gates this chapter itself at the M0
  specification review and at every exit.

### 1.3 Check classes

| Class | Meaning |
|---|---|
| **P** (prevent) | The command is refused, or its candidate is staged, before any byte of the offending state lands on a ref |
| **M** (maintain) | A derived or cached structure is kept equal to its definition, incrementally |
| **C** (construct) | No code path can produce a violation, because the format has no field, op or carrier for it, or because the construction admits no other result |
| **V** (verify) | A violation is detected after the fact (recovery, `doctor --verify`, `doctor --fsck`) and reported with exit 7 and `moirai repair`, or as a `doctor` finding |

### 1.4 Model functions

- **Where they live.** Invariant predicates are in the modules `inv` (graph and state), `alloc`, `idem`, `lease`, `coord`,
  `merge`, `vcs`, `policy`, `canon`, `image`, `body`, `query`, `derived`, `crash` and `r4` of `moirai-model`. Each is a pure
  function over the model's materialised state (`BTreeMap<#N, Node>` per commit), its commit DAG and its runtime tables
  ([60 §4.2]).
- **Trace predicates.** Where an invariant concerns a process event rather than state (I-G4, I-G6, I-F5, I-F11), the
  function takes the simulator's replayable event trace ([F15 §6.4]) together with the protocol events of [F16]. That
  trace records the acknowledgements, flush and lock events, log appends and `ProjectFs` calls that the simulator or the
  harness emits. These functions are trace predicates in the modules `crash::trace` and `r4::trace`. They hold no lock
  and no timing logic of their own, which respects [60 §4.3] (OP-13-02).
- **How they are used.** The model's own suite (WP-94) evaluates every state predicate after every command. GT2 compares
  the engine with the model. GT1 and GT3 call the `crash` functions after every crash state.

### 1.5 Re-check rule

Every invariant over versioned state is:

- checked on every write;
- re-checked after every merge, sync, import, revert and cherry-pick;
- re-checked by `doctor --verify` ([AR §3.4] heading).

Runtime invariants (I14′, I17′, I26′, I32′, I36′) are checked where their tables are read or written. Group-commit
invariants are checked at every crash state of GT1 and GT3.

## 2. Enforcement points

| EP | Point | Source |
|---|---|---|
| EP-W1 | phase 1: open, tail replay into the compact overlay, branch-overlay build | [AR §4.5] step 1 |
| EP-W2 | phase 1: idempotency pre-check | [AR §4.5] step 2 |
| EP-W4 | phase 1: candidate validation by the **immediate validators**, per op or statement: schema, the `plan/*` write mask, CAS guards, lease token, role write policy, status machine, restrict policies, precedence acyclicity of each added edge, forest depth, `gates`, claim markers, the commit limit. Also markers and `affected` from the net ops | [AR §4.5] step 4, [50 §5.9] step 2 |
| EP-WD | the **deferred validators** in I37′ order (§5), over the whole candidate: `TX` blocks, merge, sync, import, revert, cherry-pick | [AR §5a.7] step 6, [50 §5.9] step 2 |
| EP-W6 | phase 2, under the writer byte: scan by the chain rule; CAS of each commit's implied ref move and orphan parking; idempotency evaluated after the scan | [AR §4.5] step 6 |
| EP-W7 | phase 2: re-validation by key; O(1) re-parent or recompute | [AR §4.5] step 7 |
| EP-W8 | phase 2: allocation of `#N` from `next_id`; the `UIDX` probe; the `ALLOC` entry | [AR §4.5] step 8 |
| EP-W9 | phase 2: append of one group (the commit with its `Lease` and `Marker` records; the `group_end` chain trailer); the `append_hlc` check | [AR §4.5] step 9 |
| EP-W10 | group commit: flush byte, scan and re-write, flush, publish, identity check | [AR §4.5] step 10, [80 §2.4.3] |
| EP-MG | merge engine: the sync-first precondition, LCA and the recursive virtual base, the segment-walk folds, the base per key, the typed rules | [AR §5a.7] steps 0–5 |
| EP-MS | advance or stage; staging refs per pair; `resolve`; `merge --continue` | [AR §5a.7] step 8 |
| EP-VC | `revert`, `cherry-pick`, `undo`, `op restore`, `branch -d` and `branch -D` | [AR §5a.5], [AR §5a.9] |
| EP-RC | recovery and boot-change recovery; readers' replay bounds | [AR §4.2], [AR §4.7], [80 §2.4.3] |
| EP-RD | read path: the runtime clauses of `ready` and `claim`, the marker probe, as-of views, the output writer, the LQ binder | [AR §3.5], [AR §4.7], [50 §3.8], [50 §5.8] |
| EP-CK | checkpoint fold and promotion: the `MARKERS` to `MARKERS_OLD` move and the `UIDX`, `ALLOC` and `IDEM` folds | [AR §4.9] |
| EP-GC | `moirai gc` | [AR §4.9], [F17 §11] |
| EP-DV | `doctor --verify` (full recomputation) and `doctor --fsck` | [AR §4.10] |
| EP-EX | image export encoder | [AR §5b.2]–[AR §5b.6] |
| EP-IM | image import: verification, demotion, recomputed foreign merges, the alias map | [AR §5b.6] |
| EP-FMT | the format itself: no field, op, canonical item or `.moi` line exists that could carry the forbidden datum | [AR §4.6], the format chapters |
| EP-FL | the FL-1 pure libraries: path rules P1–P12, anchor capture | [40 §2.4], [40 §2.7] |
| EP-PF | the `ProjectFs` reader and file operations | [40 §2.5], [80 §2.11] |
| EP-RS | the R4 resolver: a pure function of the link, the tree snapshot and the resolver version | [40 §4.3]–[40 §4.5] |
| EP-ST | the R4 settle write rule: writer tree, freshness, quiescence, the CAS on `rev_seq` | [AR §5e.3]–[AR §5e.4], [40 §4.2], [40 §5.3] |
| EP-BD | binding verbs: `worktree bind`, `lane open`, and `init`'s binding of `main` | [AR §5a.4], [40 §5.3] |

## 3. The invariants

### 3.1 Identity and allocation

| ID | Invariant | Enforcement | Model function | Gates |
|---|---|---|---|---|
| I1 | A `#N` is unique across all branches, is allocated under the writer byte from `next_id`, and is never reused. A `uid` is unique. A `Create` of a derived uid (R4 file and root nodes) that the store already knows, on any branch, reuses that uid's `#N`, which it finds through the store-wide `UIDX`. This holds for two unmerged lanes too ([72] M7) | EP-W8 (P: allocation and the `UIDX` probe under the writer byte); EP-IM (P: an `id:` hint is honoured only if `N ≥ next_id`); EP-CK (M: the `UIDX` and `ALLOC` folds); EP-DV (V) | `inv::i1_ids_unique`, with `alloc::allocate` | GT18 uid→`#N` uniqueness (M0 on the model; M2); GT2 (M1) |
| I35′ | A `#N` is bound to at most one `uid` over the store's life, across imports and GC ([21 §8]) | EP-W8 (C: `ALLOC` rows are never rewritten); EP-IM (P: on collision the alias map `(origin store, foreign #N) → local #N` records the remap); EP-GC (C: GC never frees a `#N`); EP-DV (V) | `inv::i35p_id_binds_one_uid` | GT18 uid→`#N` uniqueness (M0 on the model; M2); GT8 import cases (M5) |

### 3.2 Graph structure

| ID | Invariant | Enforcement | Model function | Gates |
|---|---|---|---|---|
| I2 | Every structural edge has live endpoints in the same visible version. The one exception is a flagged (`has_dangling`) `blocks` or `gates` edge left by a delete, and such an edge keeps its dependent out of `ready` | EP-W4 (P: the delete policies restrict, re-point or flag, [AR §3.3]); EP-WD V04 (P: `DanglingEdge`, staged); EP-DV (V) | `inv::i2_structural_edges_live` | GT10 node-40 table (M0 on the model; M2 on one branch; M3 across branches); GT6 I12 fuzz (M2) |
| I3 | Historical edges may reference dead ids, and they resolve through the tombstone view | EP-W4 (C: historical kinds keep the dead id; `TOMB` rows are kept); EP-RD (C: tombstone rendering) | `inv::i3_historical_edges_resolve` | GT10 node-40 table (M0 on the model); GT2 (M2) |
| I4 | `parent` is a forest of depth ≤ 12 | EP-W4 (P: forest depth on `Create` and `Move`); EP-MG (P: a Kleppmann move in HLC order skips a cycle-creating move, which becomes `HierarchyCycle`); EP-WD V01 and V05 (P); EP-DV (V) | `inv::i4_parent_forest` | GT6 (M2); GT2 (M2) |
| I5′ | The combined precedence graph `blocks ∪ gates ∪ child→parent ∪ {X→D : blocks(X,P), X ∉ subtree(P), D ∈ subtree(P)}` is acyclic. A node never blocks its own descendant, and blockers are inherited only from outside the subtree. `link X --blocks P` on a container checks every descendant. `move` re-derives the exogenous classification of every edge that touches the moved subtree | EP-W4 (P: Pearce–Kelly per added edge, implied edges included); EP-WD V03 (P: Pearce–Kelly, or full Kahn above `store.kahn-fallback-edges`, [F17 §8.1]); EP-DV (V: Kahn, [AR §4.9]) | `inv::i5p_precedence_acyclic` (DFS over the whole combined graph, implied edges re-derived by definition, [60 §4.2]) | GT6, including the X1 merge variant (M2, M3); GT2 (M2) |
| I6 | `supersedes(new, old)` implies `old.status = superseded` in the same commit, and a target has at most one active superseder | EP-W4 (P); EP-MG (P: `SupersedeFork` value conflict); EP-WD V06 | `inv::i6_supersedes` | GT2 (M2); GT6 (M3) |
| I7 | The target of `duplicate_of` is canonical: the chain has length 1 | EP-W4 (P: restrict, or re-point to the canonical node); EP-WD V07 | `inv::i7_duplicate_canonical` | GT2 (M2); GT6 (M3) |
| I-P3 | Reverse adjacency equals the inverse of forward adjacency in every visible version | EP-W4 (M: both directions in the same commit); EP-CK (M: the `IN_*` CSR); EP-DV (V) | `inv::ip3_reverse_is_inverse` | GT6 reverse CSR = inverse (M2); `doctor --verify` row of [AR §8.3] (M1) |

### 3.3 Status, schema, commits and provenance

| ID | Invariant | Enforcement | Model function | Gates |
|---|---|---|---|---|
| I8 | Status transitions follow the kind's machine ([AR §3.6]). `blocked`, `ready`, `stale`, `claimed` and container `done` are never stored as source truth | EP-W4 (P: the status machine; `reopen` is the only `done → open`); EP-MG (P: the status lattice; `StatusFork`); EP-FMT (C: no stored field holds a derived or runtime status) | `inv::i8_status_machine`, over the rule table `rules/status-machines` | GT2 (M2); GT6 (M3) |
| I9 | Every derived counter and bitset, `topo`, and the reverse CSR equal a full recomputation. For `topo`, "equal" means that it is a topological order of the combined precedence graph (OP-13-03) | EP-W4 (M: eager maintenance of touched nodes); EP-CK (M: sealed derived sections); EP-DV (V: per 65,536-id chunk) | `derived::recompute_all` (the model's from-scratch definitions are the oracle) and `inv::i9_derived_matches` | GT6 derived state = recomputation (M2); `doctor --verify` (M1); GT2 (M2) |
| I10 | Every mutation belongs to exactly one commit, and that commit carries provenance: actor, role, session, git head, branch and worktree, the branch ref, the idempotency-key hash and the message | EP-W9 (C: ops exist only inside a `Commit` record, one group per commit); EP-FMT (C) | `inv::i10_mutation_in_one_commit` | GT2 (M1); GT1 (M1: groups adopted all or nothing) |
| I11 | Fields conform to the schema version of their commit. Symbols are never garbage-collected, and enum integers are never reused | EP-W4 (P: schema per op); EP-WD V09 (P: `SchemaConflict`); EP-IM (P); EP-GC (C: `SYMTAB` entries are never dropped) | `inv::i11_schema_conformance` | GT2 (M2); GT6 (M3) |
| I12 | Every branch head satisfies I1–I11 at all times. A merge, import, revert or cherry-pick with a structural violation never advances a ref: it is staged on `merge/<dst>/from/<src>` or `import/<ref>` | EP-W4 (P: a write is refused); EP-WD and EP-MS (P: staged); EP-DV (V) | `inv::i12_heads_valid`, which applies the I1–I11 functions to every head | GT6 I12 fuzz (M2); GT2 (M3) |
| I13 | A `finding` with `f_kind ∈ {perf, complexity}` reaches `fixed` only with an `addresses` edge from a different actor **and** a `verifies` edge from a review verdict | EP-W4 (P: the guard on `→ fixed`) | `inv::i13_perf_fix_needs_review` | GT2 (M2) |
| I14 | A `run` closes `green` only when every `expected_artifacts` symbol has a `produced` artifact whose `oid` was read back | EP-W4 (P: the guard on `→ green`; "read back" means an `oid` computed from the file's bytes by the reader of [40 §2.5], compared with the artifact's stored `oid`) | `inv::i14_run_green_needs_artifacts` | GT2 (M2; M6 with real file reads) |

### 3.4 Runtime coordination

| ID | Invariant | Enforcement | Model function | Gates |
|---|---|---|---|---|
| I14′ | An idempotency key is bound to its payload hash and its branch. A hit with a different payload is exit 9. A hit on another branch is exit 9, unless the original branch was merged into the caller's branch or deleted after merge, in which case the original result is returned ([AR §6.4], N13e). Windows per [F17 §11.1] | EP-W2 (P); EP-W6 (P: evaluated after the scan, pending groups included; a hit on a pending group is returned only after that group's identity check); EP-CK (M: retention) | `idem::lookup` ([F17 §11.1]) and `inv::i14p_key_binding` | GT1 (M0 toy log; M1); GT2 (M1); GT4 retry streams (M1) |
| I17′ | A lease mutation must present the current fencing token. Expiry never bumps the token. The same holder may renew an expired, unreclaimed lease | EP-W4 (P: token check); EP-W7 (P: re-checked by key under the lock) | `lease::i17p_fencing` | GT18 lease liveness (M2 semantics; M8; M10); GT4 lease variants (M1); GT2 (M2) |
| I26′ | Defined on states (§4.1): `#N` is excluded from `ready` and `claim` on branch R, and is never listed there as a live blocker, if and only if some live ref X ≠ R of kind `work` holds `#N` done, cancelled or deleted at `tip(X)`, and the commit on X's history that last set that state is not an ancestor-or-self of `tip(R)`. The markers and absorbed vectors are a cache of this definition (§4.2) | EP-W4 (M: markers from the net ops); EP-VC (M: marker recomputation on `undo`, `op restore` and `branch -D`); EP-CK (M: `MARKERS_OLD`); EP-RD (P: the exclusion in `ready`, `claim`, `blocking`, `brief`); EP-DV (V: cache against definition) | `coord::i26p_excluded` (the definition, evaluated over all live refs, never from markers, [60 §4.2]) | GT18 I26′ state oracle (M0 on the model: ≥ 10^6 histories nightly, 10^4 in the PR tier; M3); GT10 node-40 table across branches (M0 on the model; M3) |
| I27′ | Every commit reachable in the log after recovery is reachable from a ref, the reflog or a pin, or is marked orphan and never satisfies an idempotency lookup | EP-W6 and EP-RC (P: a commit whose implied ref move fails its CAS is parked on `orphans/<ref>`); EP-W2 and EP-W6 (P: lookups ignore orphans) | `crash::i27p_orphans` | GT1 (M0 toy log: the torn ref move N3; M1); GT3 (M1); GT4 (M1) |
| I32′ | `rm` refuses while any live lease covers the `#N` on any branch, unless `--release` is given | EP-W4 (P: a `LEASES` probe by `#N`); EP-W7 (P) | `lease::i32p_rm_refused_under_lease` | GT10 node-40 table (M0 on the model); GT2 (M2) |
| I33′ | On `plan/*`, `status`, `resolution`, `assignee` and claims are read-only. `blocks`, `parent` and `gates` are writable and validated | EP-W4 (P: the branch-kind write mask); EP-WD V12 (P) | `policy::i33p_plan_mask` | GT2 (M3); GT6 CM9 shapes, a lane forked from `plan/*` (M3) |
| I36′ | `claimed`, the `settled`, `deleted` and `cleared` markers, and lease state are never versioned and never exported | EP-FMT (C: no op, canonical item, trailer or `.moi` line carries them, [AR §4.6] "Not hashed", [AR §5b.7]); EP-EX (C) | `canon::i36p_runtime_not_canonical` (the model's canonical diff has no key class for them) | GT8 gate 2 with the round-trip table of [AR §5b.7] (M5); GT2 runtime tables compared separately from `state(ref)` (M1) |

### 3.5 Version control and merge

| ID | Invariant | Enforcement | Model function | Gates |
|---|---|---|---|---|
| I25′ | For every key untouched on side S since the LCA, `merge` never emits a conflict on that key. The base of a key is its value at the LCA | EP-MG (P: the base per key is found by reverse-applying dst's per-node chain past the LCA) | `merge::i25p_untouched_no_conflict` | GT6 I25′ over random DAGs with interleaved `sync` and `merge` (M3); GT2 (M3) |
| I31′ | `merge` has exactly one base-selection rule for several LCAs: the recursive virtual base. The LCAs are merged pairwise in generation order, ties broken by the lowest commit id. A key whose virtual-base value is a conflict value is clean when both sides hold the same value, and conflicts whenever they differ | EP-MG (P) | `merge::i31p_virtual_base` (each LCA state materialised and merged recursively, [60 §4.2]) | GT6 I31′, including both sides equal → clean (M3); GT2 (M3) |
| I34′ | `revert` and `cherry-pick` stage on `NotFound`, and record a `DATA` mismatch as a `FieldEdit` conflict value | EP-VC (P) | `vcs::i34p_revert_cherry_pick` | GT2 (M3); GT6 (M3) |
| I37′ | At merge, `parent` moves are applied, and the implied I5′ edges re-derived, before any precedence edge is checked. The full order is §5 | EP-WD (P: V01 and V02 before V03, §5) | `merge::i37p_validator_order`, with `merge::validate_in_order` | GT6 I37′ and the X1 merge variant (M3); GT2 (M3) |
| I41′ | Staging refs exist per `(dst, src)` pair. A staged merge or sync of one pair never blocks another pair. A `sync` of lane L is refused only while its own staging ref `merge/<L>/from/main` exists | EP-MS (P) | `merge::i41p_staging_per_pair` | GT6 CM5 shapes: two staged syncs plus a third clean one (M3); GT2 (M3) |

### 3.6 Views and derived state

| ID | Invariant | Enforcement | Model function | Gates |
|---|---|---|---|---|
| I18′ | As-of output carries no derived fields unless `--recompute` is given or a query projects them. A derived predicate that a query uses at a past view is recomputed there and charged to the query's budget. Runtime and tree-derived state at a past view is an error (E302), never a value | EP-RD (P: the LQ binder raises E302; C: the output writer omits derived columns) | `query::i18p_asof_output` (LQ-3 evaluator) | GT2 LQ differential (M7); GT9 (M7) |
| I42′ | A commit's `affected` list names every node whose value of any predicate in `P_F15` changed (§6.3). Otherwise the commit carries `affected_complete = 0`, which happens only beyond the `suspect` budget ([F17 §8.2]). As-of derivation then recomputes instead of trusting the list ([50] F15, F16) | EP-W4 (M); EP-RD (P: a past view trusts stored cones only when every commit in the window is complete, [50 §5.8]) | `derived::affected_with_budget` ([F17 §8.2]) and `inv::i42p_affected` | GT6 derived state = recomputation (M2); GT2 write results (M2) and LQ past views (M7) |
| I43′ | A commit's unhashed `append_hlc` is monotonic (non-decreasing) in `seq` order, and this is checked on append ([50] F14) | EP-W9 (P: an append that would decrease it is refused, per [F16]'s HLC rule); EP-RC (V) | `canon::i43p_append_hlc_monotone`, on the injected clock | GT2 (M1); GT3 with clock-step injection (M1); GT4 clock-step variant (M1) |

### 3.7 Image round trip

| ID | Invariant | Enforcement | Model function | Gates |
|---|---|---|---|---|
| I28′ | The git object ids of a moirai commit are a function of moirai data, the object format and the destination's anchor-text mode (`full` or `hash-only`, recorded in the unhashed side ref). The moirai commit id is a function of moirai data only, because anchor text enters the canonical form only as BLAKE3-128 digests | EP-EX (C); EP-FMT (C: canonical item 10 carries digests, [AR §4.6]) | `canon::i28p_commit_id` (the model's own canonical encoder, [60 §4.2]) | GT2 commit ids byte for byte (M1); GT8 gate 3 in both anchor-text modes (M5) |
| I29′ | Importing an image of the same format version reproduces every native commit id. A demotion affects one commit only | EP-IM (P: per-commit verification and `verified` bit) | `canon::i29p_native_reimport` | GT8 gates 0 and 1 (M5) |
| I30′ | A foreign two-parent git commit is imported as a moirai merge computed by the typed rules. Counters are never taken from a text merge | EP-IM with EP-MG (P) | `merge::i30p_foreign_merge` | GT8 import onto a diverged ref (M5); GT10 git-side merge of counters (M5) |
| I38′ | Every field of a commit's canonical form has exactly one carrier in the image. The importer recomputes every native commit id from the trailers plus the tree diff against the first parent, for every commit kind including `sync` | EP-EX (C: the gate-0 carrier table of [F14]); EP-IM (P) | `canon::i38p_gate0` (the model supplies the canonical items and ids; the carrier check is the format oracle's) | GT8 gate 0 per commit kind (M5) |
| I39′ | A tombstone file carries every out-edge the store retains for the dead node: flagged structural edges marked `flagged`, historical edges as they are. After import, `has_dangling` and the reverse index therefore equal the exporting store's | EP-EX (C) | `image::i39p_tombstone_edges` (`state(ref)` includes a tombstone's retained out-edges) | GT8 gates 1 and 2 with the CM4 case (M5) |
| I40′ | Body bytes survive export and import exactly. The only normalisation a body ever receives is CRLF → LF, in the store at write time, never in the image codec | EP-W4 (P: normalised at write); EP-EX and EP-IM (C) | `body::i40p_normalise_at_write` | GT8 gate 1 bodies (M5); GT5 `.moi` fuzzing (M5) |

### 3.8 Group commit (X-F3, [80 §2.4.3])

| ID | Invariant | Enforcement | Model function | Gates |
|---|---|---|---|---|
| I-G1 | An acknowledgement implies all three of these: a successful flush that began after the group's bytes were last written, a publish that covers the group, and a passed identity check. A replayed idempotent result is acknowledged the same way | EP-W10 (P: the identity check before acknowledging); EP-W6 (P: a replay of a pending group waits for its identity check) | `crash::ig1_ack_implies_durable`: every acknowledged durable effect is present in every recovered state ([60 §4.4] item 4) | GT1 (M0 toy log; M1); seeded bugs (1), (6) and (7) of [80 §2.4.4] (E4, M0 toy log); "Durable commit semantics" (M1) |
| I-G2 | Readers never see a durable-class group before a flush covers it. `committed_lsn` never passes a pending durable group, and no process's overlay holds a group beyond the published `committed_lsn` | EP-W6 (P: pending groups go only to a scratch layer); EP-W10 (P: the publish stops before the first uncovered pending durable group); EP-RC (P) | `crash::ig2_read_freshness`: no read reflects a durable-class group that no flush has covered, and after a crash every read reflects every acknowledged record before any writer runs | GT1 post-crash read freshness (M0 toy log; M1); seeded bugs (2) and (10); GT3 (M1) |
| I-G3 | The log is a chain: every group is valid only behind the exact predecessor it was validated against. After any crash or failed flush, the valid log is a prefix of that chain, and nothing acknowledged depends on a lost group | EP-W9 (C: the `group_end` chain trailer, [F05]); EP-W6 and EP-RC (P: a scan stops at the first invalid group) | `crash::ig3_chain_prefix` | GT1, including a failed flush with reverted, invalidated or evicted pages and ≥ 3 live pending writers (M0 toy log; M1); seeded bug (9) |
| I-G4 | At most one log flush is in flight per store. The flush holder scans and re-writes the pending range under the writer byte, and never flushes or waits for a lock while it holds that byte | EP-W10 (P: the lock order slot < leader < maintenance < flush < writer, [80 §2.2.3]) | `crash::trace::ig4_flush_discipline`, a predicate over the simulator's lock and flush events | GT1 with lock-state assertions (M0 toy log; M1); seeded bugs (4) and (8) |
| I-G5 | An appended group whose writer dies is either adopted by the next flush holder (re-write, flush, publish) or lost together with everything after it. It is acknowledged only by a process that passes its identity check, and its idempotency key makes a retry exact | EP-W10 (P); EP-RC (P) | `crash::ig5_orphan_group` | GT1 (M0 toy log; M1); seeded bugs (3) and (13); GT4 (M1) |
| I-G6 | Every publish is a read-modify-write of the newest valid slot, under the writer byte, that folds the `HEAD` effects of every newly covered group in log order. `durable_lsn`, the counters and the `lsn` pointers never decrease. `committed_lsn` decreases only to the valid end after a lost lazy tail | EP-W10 (P); EP-RC (P) | `crash::trace::ig6_publish_monotone`, a predicate over the sequence of published slots | GT1 and the "`HEAD` barrier states" row (M0 toy log; M1); seeded bugs (5), (11) and (12) |

### 3.9 File links (R-12, [40 §2.10])

| ID | Invariant | Enforcement | Model function | Gates |
|---|---|---|---|---|
| I-F1 | On every branch view, at most one live file node with status `present` or `planned` exists per (root, exact path). This is enforced at write time. After a merge, a duplicate is a `PathClaim` conflict value. Case-insensitive collisions are a resolve-time state | EP-W4 (P: the `PATHIDX` probe); EP-WD V08 (P); EP-DV (V) | `r4::if1_one_live_file_per_path` | GT2 FL-3 (M2); GT6 FL-7 and P8 (M3) |
| I-F2 | A file node's uid equals the derivation of [40 §2.3] over its stored `origin_path` and `origin_pred`. A root node's uid equals its derivation, and an anchor uid equals the derivation of [40 §2.7] over its stored `captured`. A `Create` of a uid the store already knows reuses its `#N` | EP-W4 (P); EP-W8 (P: `UIDX`); EP-IM (P: derived uids verified on import); EP-DV (V) | `r4::if2_uid_derivations`, with `alloc::allocate` | GT18 uid→`#N` uniqueness (M0 on the model; M2); P10 (M5) |
| I-F3 | Every `at` edge carries at least one anchor. Every anchor belongs to exactly one `at` edge. Anchor uids are unique per (src, dst) | EP-W4 (P); EP-FMT (C: the discriminator in the edge key, [40] R-4) | `r4::if3_at_edge_anchors` | GT2 FL-3 (M2) |
| I-F4 | OS file ids, volume serials, mtimes, creation times, stat caches, resolution states, proposals, `PENDING`, `FSINTENT`, `FPRINT`, `PREFIXEV`, `GITFACTS`, `TREES` and journal cursors never appear in versioned, hashed or exported data | EP-FMT (C: [AR §4.6] "Not hashed"; [F11] runtime-only sections); EP-EX (C) | `r4::if4_no_machine_local_in_canonical` | GT8 with P10: two stores derive the same uids and objects (M5); GT2 FL-4 (M6) |
| I-F5 | Read verbs (`show`, `pack`, `brief`, `get`, `find`, `q`, `links check`, MCP reads) append nothing to the log. `check` is a write verb | EP-RD (C: a read path takes no writer role and appends no record) | `r4::trace::if5_reads_append_nothing`, a predicate over logged appends | "Reads append nothing", the `Vfs` counter (M6, M8); P4 (M6) |
| I-F6 | An automatic re-bind is written only when all of these hold: exact evidence (or `files.policy.auto = strong`); written by the branch's writer tree; that tree is fresh for the node; after the quiescence re-check; and, on `main`, only for an observation committed in the writer tree's HEAD | EP-ST (P) | `r4::if6_rebind_rule` | P1 and P7 (M6); GT17 (M6); GT18 settle concurrency (M6); GT10 R4 corpora, row 1 through the model's exact-evidence resolution (M0) |
| I-F7 | `removed` is never inferred from absence. It is written only by `file rm`, `links fix --drop`, `links fix --same-as`, or, with `files.deletion-inference = main-tree-commits`, by a deletion commit seen on the main tree | EP-ST (P); EP-W4 (P: only these doors emit `→ removed`) | `r4::if7_removed_explicit` | P12 (M6); GT17 (M6) |
| I-F8 | Stored paths are root-relative, `/`-separated, with no empty, `.` or `..` segment and no leading `/`, valid UTF-8, exact bytes. There is no Unicode normalisation, except [80 §2.10] P3 (NFC for untracked names on normalisation-insensitive volumes). The one exception to the form is root `abs`, which is the machine-local form of [80 §2.10] P12 and is checked only for existence and `oid` | EP-FL (P: P1–P12 at capture and in the `file` verbs); EP-W4 (P: the `path` value type); EP-IM (P) | `r4::if8_path_rules` (the model's own P1–P12 over data) | GT5 path-spec fuzzing (M0); GT2 FL-3 (M2) |
| I-F9 | No live span anchor has a bare line number as its only selector. Every `quote`, `range`, `symbol` and `heading` anchor carries a quote, and every `lines` anchor carries a window | EP-FL (P: capture); EP-W4 (P) | `r4::if9_no_bare_line_anchor` | GT5 anchor-selector fuzzing (M0); GT2 FL-3 (M2) |
| I-F10 | `resolve(link, tree snapshot, resolver_version)` is a pure function. Thresholds and pattern lists are constants of the resolver version ([F20]), and a version bump is a visible event | EP-RS (C) | `r4::if10_resolve_pure` (the brute-force resolver, a pure function) | the P11 differential (M0, WP-77); P3 (M6); GT17 (M6) |
| I-F11 | moirai opens project files only with full sharing (`FILE_SHARE_READ\|WRITE\|DELETE` on Windows; read-only, no-follow, no access-time update on Unix). It never maps or locks them, closes each handle before touching the next file, and never opens the content of a cloud-only entry on an automatic path | EP-PF (P) | `r4::trace::if11_hands_off`, a predicate over the simulator's `ProjectFs` calls | "I-F11 handle probe; no hydration" (M6, M8) |
| I-F12 | Binding uniqueness: each moirai branch has at most one designated tree, and each tree is designated for at most one branch. Trees are identified by their exact git top-level, and a binding of a directory never covers a nested worktree inside it | EP-BD (P); EP-ST (P: a non-designated tree only reads) | `r4::if12_binding_unique` | GT2 FL-4 (M6); P7 (M6) |
| I-F13 | No content-only re-bind. An automatic re-bind never rests on equal content alone: an equal-`oid` candidate needs corroboration by creation time (where the per-OS rule allows it), by a git rename inside one commit, or by captured intent. A candidate that coexisted with the original is never a target | EP-RS and EP-ST (P: the copy rule, [F20]) | `r4::if13_copy_rule` (the copy rule by definition over the simulated creation times) | P1 with backup, mirror and vendored-copy generators (M6); GT17 (M6); GT10 R4 corpora, row 1 (M0) |
| I-F14 | No resurrection. A derived uid that is removed or deleted on a branch view never becomes live again on that view through registration or merge. Only `links fix --restore` or `Undelete`, both explicit and recorded, bring it back | EP-W4 (P: a registration after removal derives a new uid); EP-MG (P: the re-key rule, [AR §5a.7]); EP-DV (V) | `r4::if14_no_resurrection` | P13 (M2 FL-3; M3 FL-7); GT6 FL-7 (M3) |

## 4. I26′: the state definition and its marker cache

### 4.1 The definition

The owner signs this definition as the rule table "the I26′ state definition with its marker-cache rules" ([PLAN §7] E1).
The reference model evaluates it directly and never uses markers ([60 §4.2]).

- **Live work refs.** Let `W` be the set of live refs of kind `work` (`main` and `lane/*`). Refs of kinds `plan`, `merge`
  (staging), `import`, `tag` and `orphans` are never in `W`.
- **Terminal state.** For a commit `c` and a task `#N`, `term(c, #N)` holds when `#N` is deleted in `state_at(c)`, or when it
  is a task with status `done` or `cancelled` there. Call that state `s(c, #N)`.
- **Setter.** For `X` in `W` with `term(tip(X), #N)`, let `c0 = tip(X), c1, c2, …` be the first-parent chain of `tip(X)`. At
  a fork, that chain continues on the parent ref through `fork_commit`. The **setter** `set(X, #N)` is the oldest `c_k` such
  that `s(c_j, #N) = s(tip(X), #N)` for every `j ≤ k`. In words: it is the commit at which `#N`'s current terminal state
  began on X's first-parent history. A merge or sync commit is the setter when its net changeset against its first parent
  produced that state (OP-13-04).
- **Exclusion.** For every live ref `R` (of any kind) and every task `#N`:
  `excluded(R, #N)` ⇔ ∃ `X ∈ W`, `X ≠ R`: `term(tip(X), #N)` ∧ `set(X, #N) ∉ ancestors-or-self(tip(R))`.
- **Effect.** An excluded `#N` is not in `ready` on R, `claim` of it on R is refused, and `blocking` on R never lists it as a
  live blocker. It is rendered as "done on `<branch>` (unmerged)" or "deleted on `<branch>`" ([AR §2.16]).
  `settled_elsewhere` and `deleted_elsewhere` ([AR §3.5]) are the read-time names of the same predicate.

### 4.2 The marker cache (engine)

The engine answers `excluded` in O(1) from markers and absorbed vectors ([AR §2.16], [AR §4.4], [AR §5a.7] step 7). The
rules below restate [AR §2.16], [AR §4.5] step 4, [AR §5a.5] and [AR §5a.9]. [F11] gives the byte layout of `MARKERS` and
`MARKERS_OLD`.

| # | Rule |
|---|---|
| MC-1 | **Emission from net ops.** Every commit that lands on a ref of kind `work` emits markers from its net changeset against its first parent: `settled` for each task whose net op sets `done` or `cancelled`; `deleted` for each `Delete`; `cleared`, scoped to `(#N, ref_id)`, for each net `SetStatus` from `done` or `cancelled` to another status and for each `Undelete`. This holds whatever produced the commit: verb, batch, `TX`, merge, sync, cherry-pick, revert or import. Commits on other ref kinds, staging refs included, emit none ([72] M4). `TX { REOPEN t; SET t.done = true }` on a done task emits none, because its net ops are empty for `t` ([AR §4.3]) |
| MC-2 | **Key.** `(#N, ref_id, commit)` with `ref_seq`, `hlc` and `seq` ([AR §4.4]) |
| MC-3 | **Absorbed vectors.** Every ref carries `absorbed[(ref_id, ref_seq)]`, maintained by commit, fork, sync, merge (`absorbed_dst[src] = ref_seq(tip src)`, every other entry the maximum of both sides) and `undo` ([AR §5a.2], [AR §5a.5], [AR §5a.7] step 7) |
| MC-4 | **Active on R.** A `settled` or `deleted` marker `(#N, X, c)` with `X ≠ R` excludes `#N` on R while no later `cleared (#N, X)` exists and `absorbed_R[X] < ref_seq(c)` |
| MC-5 | **Ref moves.** `undo` and `op restore` recompute the markers of every ref they move, in both directions. They emit `cleared` for each completion or deletion that leaves the ref's history, and re-emit `settled` or `deleted` for each one that re-enters it while the state still holds at the new tip. `branch -D` re-attributes every marker the branch still held unabsorbed to a live ref that contains the marker's commit (`absorbed_Y[X] ≥ ref_seq`), and clears only the markers that no live ref holds. Each case prints a triage line ([AR §5a.5], [AR §5a.9]) |
| MC-6 | **Inertness.** At each checkpoint fold, markers that are globally inert (cleared, or absorbed by every live ref) move from `MARKERS` to `MARKERS_OLD`, which no scan reads ([AR §4.4]). `gc` drops `MARKERS_OLD` rows older than `gc.reflog-expire` ([F17 §11.2]) |
| MC-7 | **Equivalence obligation.** For every live ref R and every task `#N`, the cache answer (an MC-4-active marker exists) equals `excluded(R, #N)` of §4.1. GT18's I26′ oracle checks this equivalence through every door: `complete`, `set --done`, `set --status`, MCP `write`, `apply`, `cherry-pick`, `revert`, `merge`, `sync`, image import. It also checks every ref move: `reopen`, `Undelete`, `undo` of either, `op restore` both ways, forks with `branch -D` or `undo` on the parent, staging and `merge --abort`, and `TX` coalescing ([AR §8.2]). `doctor --verify` re-checks it ([AR §4.10]) |

A case in which MC-1–MC-6 as written and §4.1 disagree is recorded as OP-13-05. The review decides which side changes, and
the owner signs the result.

## 5. I37′: the validator order

The validator table is the extension point that [60 §2.6] certifies in M2 and M3, with M7 registering V10 and V11. Each
row gives the validator id, its position, the rule, the candidates it runs for and its outcome. The order is normative.

| V | Pos. | Rule | Runs for | Outcome on a merge, sync, import, revert or cherry-pick | Registered |
|---|---|---|---|---|---|
| V01 | 1 | Apply every `parent` move of the candidate. At a merge, the moves are the Kleppmann moves of the typed rule, in HLC order; a cycle-creating move is skipped and logged | candidates with `Move` ops | `HierarchyCycle` (structural, staged) | M2, M3 |
| V02 | 2 | Re-derive the implied exogenous edge set from the parent CSR (derived on the fly, never materialised, [71] RAM-m4) | all | none (a preparation step) | M2 |
| V03 | 3 | Precedence acyclicity (I5′) of every added `blocks` or `gates` edge and every implied edge whose endpoints moved: incremental Pearce–Kelly, or full Kahn when their count exceeds `store.kahn-fallback-edges` ([F17 §8.1]). The reported **witness** is canonical: the least edge, in canonical edge-key order (src uid, kind, dst uid, [AR §4.6] item 10), among the added or moved precedence edges that lie on a cycle | candidates that add or move precedence edges | `Cycle` (structural, staged) | M2 |
| V04 | 4 | Dangling structural edges (I2): the reverse index intersected with deletions on either side | all | `DanglingEdge` (structural, staged) | M3 |
| V05 | 5 | The `parent` forest (I4): no cycle, depth ≤ 12 | candidates with `Move` or `Create` | `HierarchyCycle` for a cycle; the class for depth > 12 is open (OP-13-06) | M2 |
| V06 | 6 | `supersedes` cardinality (I6) | all | `SupersedeFork` (value conflict, lands unless `--strict`) | M3 |
| V07 | 7 | Other cardinalities: `duplicate_of` chain length 1 (I7); `runs_in` ≤ 1; `answers` ≤ 1 active ([AR §3.3]) | all | class open (OP-13-06) | M2 |
| V08 | 8 | Path claims (I-F1) | candidates that touch file nodes | `PathClaim` (value conflict) | M3 (FL-7) |
| V09 | 9 | Schema conformance (I11), strengthening included | all | `SchemaConflict` (structural, staged) | M2, M3 |
| V10 | 10 | Every named query the candidate touched, or whose referenced schema it touched, parses and binds | all | `QueryInvalid` (structural, staged; [50] F18) | M7 |
| V11 | 11 | The named-query call graph is acyclic | all | `QueryCycle` (structural, staged; [50] F18) | M7 |
| V12 | 12 | `plan/*` read-only fields (I33′) | candidates landing on `plan/*` | a refusal; the class at a merge into `plan/*` is open (OP-13-06) | M3 |
| V13 | 13 | `Duplicate` and `Contradiction` hints | merge, sync, import | hint (lands as a log line) | M3 |

Rules:

- **VO-1 (the order).** V03 never runs before V01 and V02 have been applied (I37′). The other positions fix the emission
  order.
- **VO-2 (every validator runs).** Every validator runs, even after an earlier one has reported. Violations and conflicts are
  emitted in validator order, and within one validator in canonical key order (§4.6 item 10 of [AR]). GT2 therefore compares
  the lists exactly ([60 §4.4] item 5).
- **VO-3 (outcome by kind of command).**
  - On a write (a plain verb, `TX`, `apply`, MCP `write`), any structural violation refuses the whole command with nothing
    written. [F19] gives the exit code per class. A value-conflict class cannot arise on a write.
  - On a merge, sync, import, revert or cherry-pick, each structural violation becomes one `Violation` op, and the commit is
    staged on `merge/<dst>/from/<src>` or `import/<ref>` (I12, I41′). Value conflicts land as `Conflict` ops unless
    `--strict` is given ([AR §5a.7] step 8).
- **VO-4 (immediate and deferred).** The immediate validators of EP-W4 (schema per op, write mask, CAS guards, lease token,
  role policy, status machine, restrict policies) run per statement and before the deferred ones. They are not part of the
  I37′ order. A `TX` runs the deferred validators once, over the whole block ([50 §5.9] step 2).
- **VO-5 (the witness makes the fallback invisible).** V03's canonical witness makes the choice between Pearce–Kelly and Kahn
  invisible ([F17 §8.1], OP-13-07).

## 6. Derived-state semantics (F15)

### 6.1 State classes

| Class | Valid at | Stored | Examples |
|---|---|---|---|
| versioned | any view | in segments and the log | fields, edges, bodies, schema, conflict values |
| derived, versioned | any view | persisted, or recomputed on read | the set `P_F15` of §6.2 |
| runtime | branch tip only | runtime tables ([F11]) | leases, markers, `claimed`, `settled_elsewhere`, `deleted_elsewhere`, `ready`'s runtime clauses |
| tree-derived | branch tip with a resolved tree | never | link and anchor states, `staleness` ([40 §2.9]) |

### 6.2 The derived predicates

`P_F15` is the set of predicates whose changes a commit's `affected` list must name (§6.3). Every predicate is defined
once, is shared by the write path, the verbs and LQ ([50 §3.8]), and is recomputed from scratch by the model ([60 §4.2]).

| Predicate | Definition | Persisted as | In `P_F15` |
|---|---|---|---|
| `done` (virtual) | `status ∈ {done, cancelled}` for tasks, `answered` for questions, `accepted` for verdicts ([AR §3.1]) | from `status` | yes |
| `unfinished` | `NOT done`, for kinds with a status machine | — | yes |
| `unblocked` (structural) | `kind = task ∧ status = open ∧ ¬deleted ∧ ¬conflicted ∧ ¬container ∧ open_blockers = 0 ∧` no ancestor has `open_blockers_exo > 0`, with flagged dangling edges counted | `BM_unblocked` | yes; the time clause `defer_until ≤ now()` is outside `P_F15` |
| `blocked` | `kind = task ∧ unfinished ∧ (open_blockers > 0 ∨` an ancestor has an open exogenous blocker`)` | — | yes |
| `open_blockers`, `open_blockers_exo` | counts of `blocks` and `gates` in-edges whose source is not done or accepted, flagged edges included; the exogenous count excludes sources inside the subtree | `NodeHdr` u16 columns | yes |
| `is_blocker` | not done ∧ has an outgoing `blocks` edge to a task that is not done | `BM_is_blocker` | yes |
| `children_total`, `children_done`, `ready_to_close` | direct children; a container whose children are all done | `NodeHdr` u16 columns | yes |
| `container` | has children ([AR §3.1] flags bit 4) | flags, `BM_container` | yes |
| `suspect` | a `derived_from`, `cites`, `implements` or `depends_on` target is retracted, superseded or deleted, or its current commit differs from the edge's `pinned_commit`; or an `at` target file node is `removed` or deleted. Single-hop ([AR §3.5]) | `BM_suspect` | yes; the `suspect` budget applies ([F17 §8.2]) |
| `answered` | a live `answers` edge is visible | — | yes |
| `conflicted` | an unresolved conflict value exists on the node on this view | flags, `BM_conflicted` | yes |
| `has_dangling` | the node has a flagged dangling structural in-edge | flags, `BM_has_dangling` | yes |
| `depth` | depth in the `parent` forest | — | yes |
| `topo` | a Pearce–Kelly position | cold column | **no**: only its validity is semantic, and its values are engine-internal (OP-13-03) |
| critical path; review-loop termination; refuted share | on demand ([AR §3.5]) | — | no |
| `ready` | `unblocked` ∧ no live lease by another holder ∧ not `excluded` (§4.1) ∧ `defer_until ≤` the wall clock now | — | no (tip-only runtime) |
| `stale`; `diverged`; link and anchor states | [AR §3.5], [40 §2.9] | — | no (on demand, read-time or tree-derived) |

### 6.3 `affected` and `affected_complete` (F15, F16, I42′)

For a commit `c` that lands on ref R, with first parent `p` (the empty state for a root commit):

- `D(c)` = { n : ∃ π ∈ `P_F15` with π(`state_at(p)`, n) ≠ π(`state_at(c)`, n) }, evaluated on R's view. The same definition
  applies to every commit kind. For `sync`, `p` is the lane parent and `state_at(c)` is the full merged state ([AR §4.6]
  "Net changeset = state diff"). For merge, `p` is the destination tip.
- `affected(c) = D(c)` and `affected_complete = 1`, except when `|S(c)|` exceeds `store.suspect-budget`. In that case
  `affected(c) = D(c) \ (S(c) \ A(c))` and `affected_complete = 0`, with `S` and `A` as defined in [F17 §8.2].
- `affected` is a set. [F06] fixes its stored order and its encoding (`affected_len u32`, [50] F16).
- `newly_ready` in a write's result is separate from `affected`. It is a tip-only runtime set and is never stored
  (OP-13-08).
- A past view recomputes derived state in full for the queried subgraph whenever any commit in its window has
  `affected_complete = 0`. Otherwise it recomputes only the union of the stored `affected` lists (the cone), [50 §5.8].
- The model computes `D(c)` by comparing two from-scratch recomputations. The engine computes it incrementally. GT6 and
  GT2 compare the two.

### 6.4 Validity at views

- **Past views.** Predicates in `P_F15` are valid at any view. At a past view they are recomputed as §6.3 states and charged
  to the query's budget. Plain as-of output omits them unless `--recompute` is given or a query projects them (I18′).
- **Tip-only state.** `ready`, the runtime clauses and every tree-derived state exist only at a branch tip. At a past view
  they are E302, with `unblocked` as the suggested fix for `ready` ([50 §3.8]).

## Coverage

| Item | Part covered here | Section |
|---|---|---|
| [60 §2.5] issue-2 row "Derived-state semantics" | complete: the persisted bitset holds only the structural predicate `unblocked`; `ready` is `unblocked` plus the runtime clauses, at a tip only; the exact `affected` rule | §6 |
| [60 §2.5] "Protocol decisions" (k): invariants I-G1–I-G6 | the invariants, with enforcement points, model functions and gates. The protocol is [F16]'s | §3.8 |
| [80] X-F3 | the invariants I-G1–I-G6 only. The chain trailer is [F05]'s and the protocol [F16]'s | §3.8 |
| [40] R-12 (I-F1…I-F14) | enforcement points, model functions and gates. [F18] carries the reservation and cross-lists the statements | §3.9 |
| [50] F15 | complete: the predicate set `P_F15` and the exact `affected(c)` | §6.2, §6.3 |
| [50] F16 | the semantics of `affected_complete`. The header layout is [F06]'s | §6.3 |
| [50] F18 | the positions of `QueryInvalid` and `QueryCycle` in the validator order (V10, V11). The class enum is [F12]'s | §5 |
| [60 §2.6] the validator table (I37′ order) | the ordered table with outcomes and registration milestones | §5 |
| [AR §8.3] CORRECTNESS row "Invariant coverage" | every invariant of [AR §3.4] and [AR §5e.8], with I35′ and I-G1–I-G6, has an enforcement point, a model function and at least one gate | §3 |
| [90 §10.1] | none | — |

## Holes

None. The values that this chapter cites (`store.kahn-fallback-edges`, `store.suspect-budget` and the retention windows)
are specified in [F17], where the production values that measurements decide are holes.

## Open points for the review

- **OP-13-01 (numbering gaps).** I15′, I16′ and I19′–I24′ appear only in proposal D's list ([13 §3]). [AR §3.4] does not
  carry them, so no row exists. I35′ is added from [21 §8], because [AR §3.4] I1 and [AR §5b.6] cite it and would otherwise
  point at nothing.
- **OP-13-02 (trace predicates in the model).** [60 §4.3] puts locks, timing and processes outside the model.
  - I-G4, I-G6, I-F5 and I-F11 are nevertheless given model functions, as pure predicates over an event trace that the
    simulator and harness emit. The trace is [F15 §6.4]'s replayable trace plus the protocol events of [F16]. The
    functions contain no lock logic.
  - The alternative is to leave these four to the enumerator's assertion hooks (WP-32, R-HARN-S) with no model function.
    That would break the "model function for every invariant" rule of [AR §8.3].
  - The review decides whether R-MODEL or R-HARN-S writes them. S4 forbids the seeded-bug author from being the enumerator's
    author, not the model's.
- **OP-13-03 (`topo`).** I9 lists `topo` among the structures that "equal a full recomputation". A Pearce–Kelly position is
  not a function of the graph. The resolution:
  - I9 is read for `topo` as "is a topological order of the combined precedence graph";
  - `topo` is excluded from `P_F15` and from GT2 data.

  Otherwise every edge insertion would list renumbered nodes in `affected`, and the model, which uses DFS, could not
  reproduce the values.
- **OP-13-04 (the setter in I26′).** "The commit on X's history that last set that state" is formalised as the oldest commit
  of the contiguous run on X's first-parent chain, counted from the tip, in which `#N` has its current terminal state. This
  makes the setter the commit whose net changeset against its first parent produced the state, which is the commit MC-1 keys
  its marker with. For a merge into `main`, the setter is the merge commit, not the lane commit it absorbed. An alternative
  reading (the lane commit) gives the same exclusions everywhere except for refs that merged the lane directly without
  `main`. The first-parent reading is the one the marker cache can mirror. The owner signs it with the rule table.
- **OP-13-05 (major: `reopen` on a parent ref).**
  - **The case.** Let P be a work ref, `c` a commit on P that completes `#N`, X a lane forked from P after `c`, and R a lane
    forked from P before `c`. P then `reopen`s `#N`, which emits `cleared (#N, P)`.
  - **§4.1.** X still holds `#N` done by inheritance, its setter is `c`, and `c` is not an ancestor of R, so `#N` stays
    excluded on R.
  - **MC-1–MC-6.** The only marker for `c` is keyed `(#N, P, c)`, and it is now cleared, so the cache answers "not
    excluded". X's fork emitted no marker.
  - **Consequence.** GT18 would report the disagreement.
  - **Proposed fix.** Extend MC-5's `-D` re-attribution to `cleared`: a `cleared (#N, P)` re-attributes marker `(#N, P, c)`
    to every live work ref Y with `absorbed_Y[P] ≥ ref_seq(c)` that still holds the terminal state at `tip(Y)`. This is a
    change to the signed marker-cache rules, so R-MODEL publishes it for the owner's signature, or the owner prefers to
    change §4.1 instead. Until then the model implements §4.1 as written.
- **OP-13-06 (unnamed violation classes).** [AR §5a.8] names no class for:
  - depth > 12 at a merge (V05);
  - `duplicate_of`, `runs_in` and `answers` cardinality at a merge (V07);
  - a merge that would write masked fields on `plan/*` (V12).

  The proposal is one structural class per case, such as `DepthExceeded`, `Cardinality` and `PlanMask`, added to the
  violation-class enum in [F12] and to [F19]'s code table. Classes are part of the frozen format, so the decision belongs to
  pass 1.
- **OP-13-07 (the canonical cycle witness).** V03 reports the least added or moved precedence edge on a cycle, in canonical
  edge-key order. Without a canonical witness, Pearce–Kelly and Kahn would name different cycles, `store.kahn-fallback-edges`
  would become visible, and GT2's exact comparison of violation lists would fail.
- **OP-13-08 (what `affected` holds).** [AR §6.3] describes `affected` as "dependents unblocked, referrers of a tombstone,
  sources marked suspect, newly ready". [50] F15 defines it by derived predicates. Following [50]'s precedence for its own
  reservation, §6.3 defines `affected` exactly as `D(c)`:
  - dependents and derivation referrers enter through `P_F15`;
  - referrers through other historical kinds, such as `relates` and `mentions`, do not;
  - `newly_ready` is a separate, unstored result field.

  An exact definition is needed because the write result is compared by GT2. [AR §6.3] should be edited at WP-81a.
- **OP-13-09 (I13 "different actor").** The rule does not say whom the actor of the `addresses` edge must differ from. The
  proposed reading is: different from the actor of the commit that moves the finding to `fixed`. R-MODEL's status-machine
  table carries the reading, and the owner signs it.
- **OP-13-10 (I43′ strictness).** The invariant is stated as non-decreasing, which is what "monotonic" guarantees. Whether
  [F16]'s HLC rule makes `append_hlc` strictly increasing per commit is [F16]'s decision (WP-16b), and a strict rule would
  tighten the check at EP-W9.
- **OP-13-11 ([PLAN §3.3] gaps).** [PLAN §3.3] assigns no gap to this chapter. The two WP-16 gaps (`MOVEFILE_WRITE_THROUGH`
  without measurement 17, and the widest reading of fault-model item (3)) belong to [F16] and [F15]. This chapter depends on
  them only through the I-G gates.
- **OP-13-12 (readings in `P_F15`).** Two readings in §6.2 need confirmation:
  - `container` is read as "has at least one child". [AR §3.1] names flags bit 4 without defining it.
  - `done` and `unfinished` are in `P_F15`, so a completion lists the completed node itself in `affected`.

  The reference model's derived-state table (WP-90) should carry both readings for the owner to sign.
- **OP-13-13 (gates before a milestone).** Several invariants have no gate before M2, M3, M5 or M6, because the component
  that enforces them is built there, for example the image invariants in M5. The invariant-coverage gate of [AR §8.3]
  requires only that each has at least one gate. At M0, the model-side gates (GT10 core fixtures, GT18 on the model, GT1 and
  E4 on the toy log, GT5 and the P11 differential on FL-1) cover I1, I35′, I2, I3, I14′, I26′, I27′, I32′, I-G1–I-G6,
  I-F2, I-F6, I-F8, I-F9, I-F10 and I-F13.
- **OP-13-14 (`suspect` is single-hop: a conflict between documents).**
  - [AR §2.5] speaks of "a transitive `suspect` closure" with an op budget. [AR §3.5] defines `suspect` purely from
    `(target state, pinned_commit)`, which is a single-hop predicate: a node that derives from a node that is itself only
    `suspect` is not `suspect` by that definition.
  - §6.2 follows the definition row of [AR §3.5], which is the one the model transcribes ([60 §4.2]). It reads "closure"
    as the set of referrers whose single-hop value a commit changes; [F17 §8.2] budgets that set.
  - If the review wants transitive propagation, `P_F15`, the model's `derived` module and [F17 §8.2]'s `S(c)` change
    together.
