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
| Dropped bodies | I-D1 | [AR §11] #33 and OQ-A-7 (decided 2026-10-06); [F06 §8.1]; spec sync 3 (§3.10) |
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
- **The toy vehicle (M0).** PLAN §2.2 forbids `moirai-toylog` → `moirai-model`, and PLAN §3.1 S4 makes the seeded-bug
  author (R-TOY) someone other than the enumerator's author (R-HARN-S), so that the harness cannot be tuned to its own
  bugs. For the toy log the I-G4 and I-G6 predicates are therefore written in `moirai-vfs-sim`, the enumerator's crate:
  generic predicates over the simulator's lock, flush and namespace events and over the decoded `HEAD` slot writes
  ([F04 §3]), with the namespace check of [F16 §17.2], which judges any subject against the simulator's own namespace
  model. I-G1 to I-G3 (ack, fresh, chain) and avail are the enumerator's own verdicts there. I-G5 has no predicate of its
  own on the toy: its adoption-or-loss and identity-check clauses are judged by ack and chain, and its exact retry by the
  `model` family, which for the toy is the toy's own `doctor --verify` (I14′), written in the toy and reviewed by
  R-HARN-S ([F16 §17.2]). S4 names only the enumerator's author, so a check in the toy does not break it; the review is
  what keeps such a check from being fitted to the toy's bugs. The `crash` functions of `moirai-model` above serve the
  engine's gates from M1 ([F16 §17.2]; OWNER O-2 confirms the authorship).
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
| I35′ | A `#N` is bound to at most one `uid` over the store's life, across imports and GC ([21 §8]) | EP-W8 (C: an `ALLOC` row that binds a uid is never rewritten; only the hole of an id reserved by a `Reserve` record is filled, once, by the fold of the bulk commit that uses it, [F11 §9.1]); EP-IM (P: on collision the alias map `(origin store, foreign #N) → local #N` records the remap); EP-GC (C: GC never frees a `#N`); EP-DV (V) | `inv::i35p_id_binds_one_uid` | GT18 uid→`#N` uniqueness (M0 on the model; M2); GT8 import cases (M5) |

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
| I13 | A `finding` with `f_kind ∈ {perf, complexity}` reaches `fixed` only with an `addresses` edge from a different actor **and** a `verifies` edge from a review verdict; the actor of the `addresses` edge differs from the creator of that verdict ([RULES/status-machines] GD-005) | EP-W4 (P: the guard on `→ fixed`) | `inv::i13_perf_fix_needs_review` | GT2 (M2) |
| I14 | A `run` closes `green` only when every `expected_artifacts` symbol has a `produced` artifact whose `oid` was read back | EP-W4 (P: the guard on `→ green`; "read back" means an `oid` computed from the file's bytes by the reader of [40 §2.5], compared with the artifact's stored `oid`) | `inv::i14_run_green_needs_artifacts` | GT2 (M2; M6 with real file reads) |

### 3.4 Runtime coordination

| ID | Invariant | Enforcement | Model function | Gates |
|---|---|---|---|---|
| I14′ | An idempotency key is bound to its payload hash and its branch. A hit with a different payload is exit 9. A hit on another branch is exit 9, unless the original branch was merged into the caller's branch or deleted after merge, in which case the original result is returned ([AR §6.4], N13e). A default key hashes the name of the command's branch ([API §7.2]), so it hits only an entry made on a branch of that name: the cross-branch rule, N13e included, applies to explicit keys, and to a default key only when that name now names another ref; a default-key command on a branch of another name executes as new ([API §7.4]). Windows per [F17 §11.1] | EP-W2 (P); EP-W6 (P: evaluated after the scan, pending groups included; a hit on a pending group is returned only after that group's identity check); EP-CK (M: retention) | `idem::lookup` ([F17 §11.1]) and `inv::i14p_key_binding` | GT1 (M0 toy log; M1); GT2 (M1); GT4 retry streams (M1) |
| I17′ | A lease mutation must present the current fencing token. Expiry never bumps the token. The same holder may renew an expired, unreclaimed lease | EP-W4 (P: token check); EP-W7 (P: re-checked by key under the lock) | `lease::i17p_fencing` | GT18 lease liveness (M2 semantics; M8; M10); GT4 lease variants (M1); GT2 (M2) |
| I26′ | Defined on states (§4.1): `#N` is excluded from `ready` and `claim` on branch R, and is never listed there as a live blocker, if and only if some live ref X ≠ R of kind `work` holds `#N` done, cancelled or deleted at `tip(X)`, and the commit on X's history that last set that state (its origin, §4.1) is not an ancestor-or-self of `tip(R)`. The markers and absorbed vectors are a cache of this definition (§4.2) | EP-W4 (M: markers from the hold changes of each landing commit); EP-VC (M: marker recomputation on `undo`, `op restore`, forks and `branch -d`/`-D`); EP-CK (M: `MARKERS_OLD`); EP-RD (P: the exclusion in `ready`, `claim`, `blocking`, `brief`); EP-DV (V: cache against definition) | `coord::i26p_excluded` (the definition, evaluated over all live refs, never from markers, [60 §4.2]) | GT18 I26′ state oracle (M0 on the model: ≥ 10^6 histories nightly, 10^4 in the PR tier; M3); GT10 node-40 table across branches (M0 on the model; M3) |
| I27′ | Every commit reachable in the log after recovery is reachable from a ref, the reflog or a pin, or is marked orphan and never satisfies an idempotency lookup | EP-W6 and EP-RC (P: a commit whose implied ref move fails its CAS is parked on `orphans/<ref>`); EP-W2 and EP-W6 (P: lookups ignore orphans) | `crash::i27p_orphans` | GT1 (M0 toy log: the torn ref move N3; M1); GT3 (M1); GT4 (M1) |
| I32′ | `rm` refuses while any live lease covers the `#N` on any branch, unless `--release` is given | EP-W4 (P: a `LEASES` probe by `#N`); EP-W7 (P) | `lease::i32p_rm_refused_under_lease` | GT10 node-40 table (M0 on the model); GT2 (M2) |
| I33′ | On `plan/*`, `status`, `resolution`, `assignee` and claims are read-only. `blocks`, `parent` and `gates` are writable and validated | EP-W4 (P: the branch-kind write mask); EP-WD V12 (P) | `policy::i33p_plan_mask` | GT2 (M3); GT6 CM9 shapes, a lane forked from `plan/*` (M3) |
| I36′ | `claimed`, the `settled`, `deleted` and `cleared` markers, and lease state are never versioned and never exported | EP-FMT (C: no op, canonical item, trailer or `.moi` line carries them, [AR §4.6] "Not hashed", [AR §5b.7]); EP-EX (C) | `canon::i36p_runtime_not_canonical` (the model's canonical diff has no key class for them) | GT8 gate 2 with the round-trip table of [AR §5b.7] (M5); GT2 runtime tables compared separately from `state(ref)` (M1) |

### 3.5 Version control and merge

| ID | Invariant | Enforcement | Model function | Gates |
|---|---|---|---|---|
| I25′ | For every key untouched on side S since the LCA, `merge` never emits a conflict on that key. The base of a key is its value at the LCA | EP-MG (P: the base per key is found by reverse-applying dst's per-node chain past the LCA) | `merge::i25p_untouched_no_conflict` | GT6 I25′ over random DAGs with interleaved `sync` and `merge` (M3); GT2 (M3) |
| I31′ | `merge` has exactly one base-selection rule for several LCAs: the recursive virtual base. The LCAs are merged pairwise in generation order, ties broken by the lowest commit id. A key whose virtual-base value is a conflict value is clean when both sides hold the same value, takes the other side's value when one side still holds the base's conflict value (that side did not touch the key since the base, I25′), and conflicts when both sides changed it and they differ ([F12 §5.4] RVB-1 to RVB-4; [RULES/merge-table] MR-001 to MR-004, VB-018). [AR §3.4] and [60 §3.4] say "conflicts whenever they differ"; review pass 1 (S1-15) reads that as "whenever both sides changed it and they differ", and WP-81a edits both texts | EP-MG (P) | `merge::i31p_virtual_base` (each LCA state materialised and merged recursively, [60 §4.2]) | GT6 I31′, including both sides equal → clean and one side untouched → the other side's value ([F12 §5.7] VBC-1, VBC-3) (M3); GT2 (M3) |
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
| I28′ | The git object ids of a moirai commit are a function of moirai data, the object format, the destination's anchor-text mode (`full` or `hash-only`, recorded in the unhashed side ref) and the exporting store's dropped set (a dropped body is written as its hash and reason, [F06 §8.1] DB-11, [F14 §3.3]). The moirai commit id is a function of moirai data only, because anchor text enters the canonical form only as BLAKE3-128 digests | EP-EX (C); EP-FMT (C: canonical item 10 carries digests, [AR §4.6]) | `canon::i28p_commit_id` (the model's own canonical encoder, [60 §4.2]) | GT2 commit ids byte for byte (M1); GT8 gate 3 in both anchor-text modes (M5) |
| I29′ | Importing an image of the same format version reproduces every native commit id. A demotion affects one commit only | EP-IM (P: per-commit verification and `verified` bit) | `canon::i29p_native_reimport` | GT8 gates 0 and 1 (M5) |
| I30′ | A foreign two-parent git commit is imported as a moirai merge computed by the typed rules. Counters are never taken from a text merge | EP-IM with EP-MG (P) | `merge::i30p_foreign_merge` | GT8 import onto a diverged ref (M5); GT10 git-side merge of counters (M5) |
| I38′ | Every field of a commit's canonical form has exactly one carrier in the image. The importer recomputes every native commit id from the trailers plus the tree diff against the first parent, for every commit kind including `sync` | EP-EX (C: the gate-0 carrier table of [F14]); EP-IM (P) | `canon::i38p_gate0` (the model supplies the canonical items and ids; the carrier check is the format oracle's) | GT8 gate 0 per commit kind (M5) |
| I39′ | A tombstone file carries every out-edge the store retains for the dead node: flagged structural edges marked `flagged`, historical edges as they are. After import, `has_dangling` and the reverse index therefore equal the exporting store's | EP-EX (C) | `image::i39p_tombstone_edges` (`state(ref)` includes a tombstone's retained out-edges) | GT8 gates 1 and 2 with the CM4 case (M5) |
| I40′ | Body bytes survive export and import exactly. The only normalisation a body ever receives is CRLF → LF, in the store at write time, never in the image codec. A dropped body travels as its hash and reason, and the importer stores no bytes for it ([F06 §8.1] DB-11) | EP-W4 (P: normalised at write); EP-EX and EP-IM (C) | `body::i40p_normalise_at_write` | GT8 gate 1 bodies (M5); GT5 `.moi` fuzzing (M5) |

### 3.8 Group commit (X-F3, [80 §2.4.3])

| ID | Invariant | Enforcement | Model function | Gates |
|---|---|---|---|---|
| I-G1 | An acknowledgement implies all three of these: a successful flush that began after the group's bytes were last written, a publish that covers the group, and a passed identity check. A replayed idempotent result is acknowledged the same way | EP-W10 (P: the identity check before acknowledging); EP-W6 (P: a replay of a pending group waits for its identity check) | `crash::ig1_ack_implies_durable`: every acknowledged durable effect is present in every recovered state ([60 §4.4] item 4) | GT1 (M0 toy log; M1); seeded bugs (1), (6) and (7) of [80 §2.4.4] (E4, M0 toy log); "Durable commit semantics" (M1) |
| I-G2 | Readers never see a durable-class group before a flush covers it. `committed_lsn` never passes a pending durable group, and no process's overlay holds a group beyond the published `committed_lsn` | EP-W6 (P: pending groups go only to a scratch layer); EP-W10 (P: the publish stops before the first uncovered pending durable group); EP-RC (P) | `crash::ig2_read_freshness`: no read reflects a durable-class group that no flush has covered, and after a crash every read reflects every acknowledged record before any writer runs, as read precisely below the table | GT1 post-crash read freshness (M0 toy log; M1); seeded bugs (2) and (10); GT3 (M1) |
| I-G3 | The log is a chain: every group is valid only behind the exact predecessor it was validated against. After any crash or failed flush, the valid log is a prefix of that chain, and nothing acknowledged depends on a lost group | EP-W9 (C: the `group_end` chain trailer, [F05]); EP-W6 and EP-RC (P: a scan stops at the first invalid group) | `crash::ig3_chain_prefix` | GT1, including a failed flush with reverted, invalidated or evicted pages and ≥ 3 live pending writers (M0 toy log; M1); seeded bug (9) |
| I-G4 | At most one log flush is in flight per store. The flush holder scans and re-writes the pending range under the writer byte, and never flushes or waits for a lock while it holds that byte | EP-W10 (P: the lock order slot < leader < maintenance < flush < writer, [80 §2.2.3]) | `crash::trace::ig4_flush_discipline`, a predicate over the simulator's lock and flush events | GT1 with lock-state assertions (M0 toy log; M1); seeded bugs (4) and (8) |
| I-G5 | An appended group whose writer dies is either adopted by the next flush holder (re-write, flush, publish) or lost together with everything after it. It is acknowledged only by a process that passes its identity check, and its idempotency key makes a retry exact | EP-W10 (P); EP-RC (P) | `crash::ig5_orphan_group` | GT1 (M0 toy log; M1); seeded bugs (3) and (13); GT4 (M1) |
| I-G6 | Every publish is a read-modify-write of the newest valid slot, under the writer byte, that folds, in log order, the `HEAD` effects of every group above the slot's `durable_lsn` that it covers ([F16] P-50). `durable_lsn`, the counters and the `lsn` pointers never decrease. `committed_lsn` decreases only to the valid end after a lost lazy tail | EP-W10 (P); EP-RC (P) | `crash::trace::ig6_publish_monotone`, a predicate over the sequence of published slots | GT1 and the "`HEAD` barrier states" row (M0 toy log; M1); seeded bugs (5), (11) and (12) |

**I-G2's post-crash clause, read precisely** (spec sync 2b, WP-32 and WP-40):
- **Boot mode.** The clause holds for Known-boot readers. An Unknown-boot reader reflects every acknowledged record only
  after the next writer's flush ([OS/proc §5] U5–U6, [F16] P-67): a crash that tears one slot and reverts the other can
  lose a publish that only that flush republishes.
- **"Before any writer runs".** The judged read is a reader's first read after its own boot-change recovery ([F16] P-60,
  P-66).
- **Process deaths without a crash.** The first read after deaths alone is judged for consistency only: no phantom,
  nothing half-applied, and freshness against the writers' published state, not completeness. A failed `HEAD` flush,
  or a publish write that applied nothing, followed by a death that holds the writer byte forever ([F15] FM-8.1 class
  (c)), can leave every reader's view behind the last publish with no writer to repair it. [60 §4.4] item 3 requires
  only that `rev` be monotonic per process.
- **Lazy values.** Read freshness covers durable-class records. A lazy value that a reader in the same boot has already
  served may be absent from a later read after a failed flush of its file ([F15] FM-3.6); a harness may widen this
  store-wide (FM-3.6 "Harness allowance"), which is a gate's allowance, not part of the invariant. A field kept only in
  `HEAD` whose durable publish was not acknowledged may likewise be read again with its old value (FM-3.6 "Fields kept
  only in `HEAD`").

### 3.9 File links (R-12, [40 §2.10])

The statements of record are [F18 §2.1]–[F18 §2.14], one section per invariant I-F1 … I-F14 in order ([F18 §2.15];
review pass 1, A1-44). The "Invariant" column below is a summary for this table's enforcement points, model functions and
gates; where it and [F18 §2] differ — [F18] adds the anchor's `pred` input to I-F2, `links fix --split` to I-F7, the root
clause of review S-19 to I-F8 and the history-verb doors of review S-14 to I-F14 — [F18 §2] wins.

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

### 3.10 Dropped bodies ([AR §11] #33, OQ-A-7; spec sync 3)

The **dropped set** of a view is the hashes of the `DROPPED` table of its segment set ([F11 §13.4]) and of the `BodyDrop`
records ([F05 §9.29]) after that set's bound ([F06 §8.1] DB-1). A purge is **pending** while an active extent holds a
`BodyDrop` record of origin `command` ([F16] P-73, P-101); a record of origin `import` names only hashes whose bytes the
store does not hold ([F05 §9.29] "Origin") and leaves nothing to purge. For (b) and (c), the `cs.<n>` and `blobs.<n>` of
a reservation whose bulk `Commit` has not landed ([F16] P-84) are not live: that `Commit` cannot land carrying a dropped
body ((a), [F16] P-34), and once the `blobs.<n>` holds one, both files are a deletion still due, which `gc` or a purge
makes without waiting for `gc.cruft-delay` ([F16] P-84, P-101 step 7).

| ID | Invariant | Enforcement | Model function | Gates |
|---|---|---|---|---|
| I-D1 | Dropped bodies are not kept. (a) No `Commit` record appended after a `BodyDrop` record carries a body entry for a hash that record drops, in the log or in a `hist` file. (b) In every live graph segment and `cs.<n>`, a `BLOBTAB` entry has `file` = 0 only when its hash is in the dropped set, and, when no purge is pending, exactly when it is. (c) When no purge is pending and its deletions are done, no store file holds a dropped body's bytes: no live `blobs` file holds a blob of class `body` with a dropped hash, no live `hist` file holds a body entry for one, no full-text section holds a posting of one, and every dictionary that was live when the purge began has been replaced ([F16] P-101 step 3); the files of a reservation whose bulk `Commit` has not landed are not live, and such a `blobs.<n>` that holds a dropped body is a deletion still due ([F16] P-84) | EP-W4 (P: a write that supplies a dropped body's bytes is refused, [F06 §8.1] DB-7); EP-W7 (P: a `BodyDrop` in the window forces a recomputation, [F16] P-34); EP-W9 (C: a carried entry is only a body that is not dropped, [F06 §8] BD-4); EP-IM (P: an import stores no bytes of a dropped hash, and its `BodyDrop` names no hash the store holds, DB-11, [F05 §9.29]); EP-CK (M: no fold seals, indexes or trains on a dropped body, [F16] P-80); EP-GC (M: the purge, run by `BodyDrop`, `gc` and `backup`, [F16] P-101; the release of a reservation whose `blobs.<n>` holds a dropped body without `gc.cruft-delay`, [F16] P-84); EP-DV (V: `doctor --fsck`, below) | `body::id1_dropped_bodies`, over the store's decoded records, `BLOBTAB` entries, `BLOBIDX` rows and full-text postings that the harness supplies (the model's own suite checks (a)'s refusals and carriage over its command stream) | GT1 and GT3 with `BodyDrop` and its purge in the streams, `doctor --fsck` after every crash state and at the end of every run (M2, the re-certification of [60 §3.3]); GT2: the dropped set and the `body_dropped` refusals (M2) |

**I-D1's `doctor --fsck` check.** `doctor --fsck` computes the dropped set of the newest slot's view and whether a purge
is pending (a pending purge is itself reported, by plain `doctor` too, as the finding `purge_pending` of [F19] and
[API §8.6], with `moirai gc` as its remedy; no automatic maintenance runs a purge, [F16] P-76), then reports a V-class
violation of I-D1 (§1.3) for:
- (a) a body entry, in a `Commit` record of the log or of a live `hist` file, whose hash is dropped at a lower lsn: the
  record's `RecHdr.lsn`, which a `hist` file keeps ([F10 §4.1]), above the `BodyDrop` record's lsn or the `DROPPED` row's
  `drop_lsn`;
- (b) a `BLOBTAB` entry with `file` = 0 whose hash is not dropped, and, when no purge is pending, one with `file` ≠ 0
  whose hash is dropped;
- (c) when no purge is pending: a `BLOBIDX` row (h, 1) of a live `blobs` file with h dropped; a `hist` body entry with a
  dropped hash; a full-text posting of a node whose `body_ref` names a dropped `BlobRef` ([F09 §12.1]). A store file
  that no slot, record or pin names is reported as a deletion still due ([F16] P-77, P-79), not as a violation; so are
  the `cs.<n>` and `blobs.<n>` of a reservation whose bulk `Commit` has not landed when that `blobs.<n>` holds a
  dropped body ([F16] P-84), files that (b) and (c) do not check.
  Dictionaries are not checked: no record says which bodies a training read, which is why a purge
  replaces every live one ([F16] P-101 step 3).

The check reads every live `hist` file, so its cost is that of a full history scan, as the rest of `doctor --fsck`.
What a drop does not reach — backups, exported images, other stores, freed sectors — is outside the store and outside
the check ([F06 §8.1] DB-10).

## 4. I26′: the state definition and its marker cache

### 4.1 The definition

The owner signs this definition as the rule table "the I26′ state definition with its marker-cache rules" ([PLAN §7] E1):
[RULES/state-definition] `view-kinds` (VK), `hold-values` (HV), `origin-rules` (OR) and PD-012 to PD-018. This section
restates it; where the two differ, the rule table wins. The reference model evaluates it directly and never uses markers
([60 §4.2]).

- **Live work refs.** Let `W` be the set of live refs of kind `work` (`main` and `lane/*`, VK-001). Refs of kinds `plan`,
  `merge` (staging), `import`, `tag` and `orphans` are never in `W` (VK-002 to VK-005, VK-007).
- **Hold.** For a commit `c` and a node `#N`, the hold `h(c, #N)` is `deleted` when `#N` is a tombstone in `state_at(c)`,
  `done` or `cancelled` when `#N` is a live task with that status there, and `none` otherwise (HV-001 to HV-004). Let
  `S` = {`done`, `cancelled`, `deleted`}.
- **Origin.** For `h(c, #N) ∈ S`, the origin `org(c, #N)` is "the commit on X's history that last set that state" ([AR §3.4]
  I26′): the commit where that exact hold value first appears on the way back from `c` through its parents, first parent
  first (OR-001 to OR-006). A root commit is its own origin. A commit whose first parent holds the same value inherits that
  parent's origin; a two-parent commit (merge or sync) whose second parent holds the same value inherits that parent's
  origin; any other commit produced the value itself (an ordinary commit, a revert, a cherry-pick, an imported commit, a
  `Delete`, an `Undelete` that restores a closed status, a merge's own resolution) and is the origin. A merge that takes a
  lane's completion therefore does not originate it again (OP-13-04, closed).
- **Exclusion.** For every live ref `R` (of any kind) and every node `#N`:
  `excluded(R, #N)` ⇔ ∃ `X ∈ W`, `X ≠ R`: `h(tip(X), #N) ∈ S` ∧ `org(tip(X), #N) ∉ ancestors-or-self(tip(R))` (PD-012).
  `deleted_elsewhere` and `settled_elsewhere` ([AR §3.5]) are its restrictions to the hold `deleted` and to the holds
  `done` and `cancelled` (PD-013, PD-014).
- **Effect.** An excluded `#N` is not in `ready` on R (PD-015), `claim` of it on R is refused (PD-017), and `blocking` on R
  never lists it as a live blocker (PD-018). It is rendered as "done on `<branch>` (unmerged)" or "deleted on `<branch>`"
  ([AR §2.16]).

### 4.2 The marker cache (engine)

The engine answers `excluded` in O(1) from markers and absorbed vectors ([AR §2.16], [AR §4.4], [AR §5a.7] step 7). The
cache's rules are [RULES/state-definition] §6: the marker fields MF-001 to MF-009, the events ME-001 to ME-013, the
reader's test AB-001 to AB-004 and the absorbed-vector rules VR-001 to VR-006, with the argument there that the cache
equals §4.1. [F11 §7] gives the byte layout of `MARKERS` and `MARKERS_OLD`, and [F05 §9.5] the `Marker` record that
carries every change of a marker's holder set or flag (the entries of ME-001 to ME-011; the storage moves of ME-012 and
ME-013 write none). The labels below are kept for citation; each points at the rule rows, which win.

| # | Rule |
|---|---|
| MC-1 | **Emission follows holds.** A commit that lands on a ref X of kind `work` changes the cache only where X's hold of a node changes between its first parent and the commit: a hold the commit produced itself originates a marker with holders {X} (ME-001); a hold taken from the other parent adds X to the holder set of that hold's origin, or re-emits that marker if it is inactive (ME-002, ME-003); a hold that ends removes X from its origin's holder set, and `cleared` is written only when no holder remains (ME-004). This holds whatever produced the commit: verb, batch, `TX`, merge, sync, cherry-pick, revert or import. Commits on other ref kinds, staging refs included, change no holder set (ME-008), but they can still flag markers nonlinear (ME-011, which applies to every ref kind); a commit that leaves every hold as it was changes no holder set either (ME-010): `TX { REOPEN t; SET t.done = true }` on a done task emits nothing ([AR §4.3]) |
| MC-2 | **Key and state.** A marker is keyed (`#N`, origin ref, origin commit) and carries the origin's `ref_seq`, `hlc` and `seq`, the set of live work refs that hold `#N` with that origin, and a `nonlinear` flag (MF-001 to MF-009). It is **active** while its holder set is non-empty (MF-006) |
| MC-3 | **Absorbed vectors.** Every ref carries `absorbed[(ref_id, ref_seq)]`: `absorbed_R[Y]` is the greatest `ref_seq` of a Y-landed commit in ancestors-or-self(tip(R)), maintained by commit, fork, sync, merge (`absorbed_dst[src] = ref_seq(tip src)`, every other entry the maximum of both sides) and `undo` (VR-001 to VR-006; [AR §5a.2], [AR §5a.5], [AR §5a.7] step 7) |
| MC-4 | **Excluded on R.** `#N` is excluded on R iff some active marker of `#N` is not absorbed by R: a linear marker when `absorbed_R[origin ref] < ref_seq`, a nonlinear one when its origin commit ∉ ancestors-or-self(tip(R)) (AB-001 to AB-004) |
| MC-5 | **Ref moves and the ref set.** A ref deletion (`branch -d`, `-D`) removes the ref from every holder set and clears only the markers left with no holder, which is the design's "re-attribution to a live ref that contains the marker's commit" (ME-005). `undo` and `op restore` apply MC-1's changes for every node whose hold differs between the old and the new tip, in both directions (ME-006). A fork joins the holder sets of every closed hold at its fork commit (ME-007). The first commit to land on a ref that `undo` or `op restore` moved off a marker's origin flags that marker nonlinear for good (ME-011). Each `settled`, `deleted` or `cleared` record prints a triage line ([AR §5a.5], [AR §5a.9]) |
| MC-6 | **Inertness.** At each checkpoint fold, cleared markers and markers absorbed by every live ref of every kind move from `MARKERS` to `MARKERS_OLD`, which no scan reads, with their holder sets and flags (ME-012, [AR §4.4]). ME-002 to ME-007 and ME-011 go on writing for a row in `MARKERS_OLD` the entries they would write in `MARKERS`, and such an entry returns the row to `MARKERS` with the entry applied; after `undo` or `op restore` moves a ref, or a fork creates one, an active row that some live ref has not absorbed returns as well (ME-013). The move and the return are storage moves: they are derived from the rows and the absorbed vectors, write no record and change no holder set or flag, so the records are the same whether or not a fold ran ([F11 §7] "Records"). A `gc` run's checkpoint fold, after its inertness move, drops the `MARKERS_OLD` rows whose `hlc` is older than the run's reflog window (`gc.reflog-expire`, or the run's `reflog_expire`; [F11 §7] "Retention", [F17 §11.2]) |
| MC-7 | **Equivalence obligation.** For every live ref R and every task `#N`, the cache answer (MC-4) equals `excluded(R, #N)` of §4.1. GT18's I26′ oracle checks this equivalence through every door: `complete`, `set --done`, `set --status`, MCP `write`, `apply`, `cherry-pick`, `revert`, `merge`, `sync`, image import. It also checks every ref move: `reopen`, `Undelete`, `undo` of either, `op restore` both ways, forks with `branch -D` or `undo` on the parent, staging and `merge --abort`, and `TX` coalescing ([AR §8.2]; [RULES/state-definition] `door-coverage` and the scenarios S1 to S13). `doctor --verify` re-checks it ([AR §4.10]) |

The first draft's own rules MC-1 to MC-6 disagreed with §4.1 when a parent ref reopened a completion that a fork still held
(OP-13-05). The holder set settles that case: the marker stays active while the fork holds its origin
([RULES/state-definition] scenario S7, there reached by a sync instead of a fork).

## 5. I37′: the validator order

The validator table is the extension point that [60 §2.6] certifies in M2 and M3, with M7 registering V10 and V11. Each
row gives the validator id, its position, the rule, the candidates it runs for and its outcome. The order is normative.

| V | Pos. | Rule | Runs for | Outcome on a merge, sync, import, revert or cherry-pick | Registered |
|---|---|---|---|---|---|
| V01 | 1 | Apply every `parent` move of the candidate. At a merge, the moves are the Kleppmann moves of the typed rule, in HLC order; a cycle-creating move is skipped and logged | candidates with `Move` ops | `HierarchyCycle` (structural, staged) | M2, M3 |
| V02 | 2 | Re-derive the implied exogenous edge set from the parent CSR (derived on the fly, never materialised, [71] RAM-m4) | all | none (a preparation step) | M2 |
| V03 | 3 | Precedence acyclicity (I5′) of every added `blocks` or `gates` edge and every implied edge whose endpoints moved: incremental Pearce–Kelly, or full Kahn when their count exceeds `store.kahn-fallback-edges` ([F17 §8.1]). The reported **witness** is canonical: the least edge, in canonical edge-key order (src uid, kind, dst uid, [AR §4.6] item 10), among the added or moved precedence edges that lie on a cycle | candidates that add or move precedence edges | `Cycle` (structural, staged) | M2 |
| V04 | 4 | Dangling structural edges (I2): the reverse index intersected with deletions on either side | all | `DanglingEdge` (structural, staged) | M3 |
| V05 | 5 | The `parent` forest (I4): no cycle, depth ≤ 12 | candidates with `Move` or `Create` | `HierarchyCycle` for a cycle; `DepthExceeded` (67) for depth > 12 (structural, staged; [F19 §12.2], key [F12 §7.9]) | M2 |
| V06 | 6 | `supersedes` cardinality (I6) | all | `SupersedeFork` (value conflict, lands unless `--strict`) | M3 |
| V07 | 7 | Other cardinalities: `duplicate_of` chain length 1 (I7); `runs_in` ≤ 1; `answers` ≤ 1 active ([AR §3.3]) | all | `Cardinality` (68; structural, staged; [F19 §12.2], key [F12 §7.9]) | M2 |
| V08 | 8 | Path claims (I-F1) | candidates that touch file nodes | `PathClaim` (value conflict) | M3 (FL-7) |
| V09 | 9 | Schema conformance (I11), strengthening included | all | `SchemaConflict` (structural, staged) | M2, M3 |
| V10 | 10 | Every named query the candidate touched, or whose referenced schema it touched, parses and binds | all | `QueryInvalid` (structural, staged; [50] F18) | M7 |
| V11 | 11 | The named-query call graph is acyclic | all | `QueryCycle` (structural, staged; [50] F18) | M7 |
| V12 | 12 | `plan/*` read-only fields (I33′) | candidates landing on `plan/*` | a refusal on a write; `PlanMask` (72; structural, staged) at a merge into `plan/*` ([F19 §12.2], key [F12 §7.9]) | M3 |
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
  - On a staging ref, a `RESOLVE` block ([F12 §6.5]) is refused only for a structural violation of a check that the view
    before it passed, and always when it makes a node its own ancestor. Violations the staged view already has do not
    refuse it; `merge --continue` re-checks them ([F12 §9.4]). Otherwise single-key resolution would be impossible
    whenever the staged view breaks I2, I4, I5′, I6, I7 or I11.
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
| `open_blockers`, `open_blockers_exo` | counts of `blocks` in-edges whose source is unfinished (a task not `done` or `cancelled`, a question not `answered`), flagged `blocks` edges included; the exogenous count excludes sources inside the subtree ([RULES/state-definition] BT-001 to BT-004). `gates` in-edges never count here (BT-006) | `NodeHdr` u16 columns | yes |
| `gated` | `kind = task ∧` some `gates` in-edge has a live source verdict with status `open` and outcome `fail_fixable` or `fail_fundamental`, or is flagged ([RULES/state-definition] PD-024, BT-005, BT-007, BT-008). It drives only the `complete` guard ([RULES/status-machines] GD-002), never `unblocked`, `ready` or `claim` | — | yes |
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

`gates` constrains `complete` and never `claim` ([AR §3.3] X5, [AR §6.2]): counting its in-edges in `open_blockers`, as
[AR §3.5] does, would keep a task gated by an `open` `fail_fixable` verdict out of `ready`, so nobody could claim the fix
that lets the verdict be accepted. Review pass 1 (S1-30) adopted the rule table's reading above; [AR §3.5] is edited at
WP-81a.

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
| [AR §11] #33 and OQ-A-7 (bodies droppable by hash without changing commit ids) | the invariant I-D1 with its enforcement points, model function, gates and `doctor --fsck` check; I28′ and I40′ name the dropped set. What a drop is, is [F06 §8.1]'s; the purge [F16] P-101's | §3.7, §3.10 |
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
  - **Spec sync 2b (WP-40 review and closure):** for the toy vehicle, R-HARN-S writes the I-G4 and I-G6 predicates and the
    namespace check in `moirai-vfs-sim` (§1.4), because the toy may not depend on `moirai-model` (PLAN §2.2) and these
    generic checks are the enumerator's, which S4 keeps from the seeded-bug author; ack, fresh, chain and avail are the
    enumerator's verdicts; checks that need the toy's own state
    (`doctor --verify`, which carries the `model` family, and read visibility against its replayed view) stay the toy's
    and are reviewed by R-HARN-S as a WP-40 acceptance step. R-MODEL keeps the `crash` functions for the
    engine's gates from M1. The authorship is PLAN's (OWNER O-2).
- **OP-13-03 (`topo`).** I9 lists `topo` among the structures that "equal a full recomputation". A Pearce–Kelly position is
  not a function of the graph. The resolution:
  - I9 is read for `topo` as "is a topological order of the combined precedence graph";
  - `topo` is excluded from `P_F15` and from GT2 data.

  Otherwise every edge insertion would list renumbered nodes in `affected`, and the model, which uses DFS, could not
  reproduce the values.
- **OP-13-04 (the setter in I26′) — closed in review pass 1 (S1-16).** The first draft formalised "the commit on X's history
  that last set that state" as the oldest commit of the run on X's first-parent chain, which made a merge into `main` the
  setter. The review adopted [RULES/state-definition]'s origin reading (OR rows, its open point 1): the origin follows the
  parents, first parent first, so the lane commit stays the origin, and a ref that merged the lane directly and reopened
  the task on purpose is not overridden by `main`'s merge commit (its scenario S8). §4.1 states that reading.
- **OP-13-05 (`reopen` on a parent ref) — closed in review pass 1 (S1-16).** The first draft's cache scoped `cleared` to
  (`#N`, ref) and lost a completion that a fork still held, so it disagreed with its own definition. The review adopted the
  holder-set cache of [RULES/state-definition] §6 (MF-006, ME-004, ME-005; scenario S7): a marker stays active while any
  live work ref holds its origin. §4.2 cites those rows; [F11 §7] stores the holder set and the `nonlinear` flag, and
  [F05 §9.5] records every holder change.
- **OP-13-06 (unnamed violation classes).** [AR §5a.8] names no class for:
  - depth > 12 at a merge (V05);
  - `duplicate_of`, `runs_in` and `answers` cardinality at a merge (V07);
  - a merge that would write masked fields on `plan/*` (V12).

  The proposal is one structural class per case, such as `DepthExceeded`, `Cardinality` and `PlanMask`, added to the
  violation-class enum in [F12] and to [F19]'s code table. Classes are part of the frozen format, so the decision belongs to
  pass 1. **Closed in review pass 1 (P1-21, S1-33):** [F19 §12.2] assigns `DepthExceeded` 67, `Cardinality` 68 and
  `PlanMask` 72, [F12 §7.9] gives their keys, and §5 cites them. The merge-table rows that stage them are
  [RULES/merge-table]'s.
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
- **OP-13-09 (I13 "different actor"). Closed: decided 2026-09-28 (OQ-M-2 (a)).** The rule did not say whom the actor of
  the `addresses` edge must differ from. The first draft proposed the actor of the commit that moves the finding to
  `fixed`; [RULES/status-machines] GD-005 reads it as the creator of the reviewing verdict. The owner chose GD-005's
  reading: the actor of the `addresses` edge differs from the creator of the verdict whose `verifies` edge the guard
  reads, so the reviewer did not review their own fix. The I13 row cites GD-005.
- **OP-13-10 (I43′ strictness).** The invariant is stated as non-decreasing, which is what "monotonic" guarantees. Whether
  [F16]'s HLC rule makes `append_hlc` strictly increasing per commit is [F16]'s decision (WP-16b), and a strict rule would
  tighten the check at EP-W9. Review pass 1 (S1-13, P1-5): [F16] P-36 draws every commit's `append_hlc` from one
  sequence over the semantic durable records ([API §6.2] CK-4), strictly increasing in `seq` order, so the check at EP-W9
  may be strict.
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
- **OP-13-15 (spec sync 2b).** Three readings met by WP-32, WP-90b and WP-91: I-G2's post-crash clause is stated
  precisely after the §3.8 table (boot mode, the judged first read, deaths without a crash, lazy values); MC-1 says that
  a commit on another ref kind changes no holder set but can still flag markers nonlinear (ME-008, ME-011); VO-3 lets a
  `RESOLVE` block on a staging ref pass over the violations its view already has, re-checked by `merge --continue`
  ([F12 §6.5], §9.4). The independent check of the sync added three: MC-6 and §4.2's lead-in follow
  [RULES/state-definition] open point 17 (a) (ME-012's move and ME-013's return are storage moves that keep the holder
  set and flag and write no record; `gc` drops `MARKERS_OLD` rows after its inertness move), where MC-6 still had ME-013
  flag a returned row nonlinear; I14′ says that a default key, which hashes the branch name ([API §7.2]), meets the
  cross-branch rule only when that name names another ref; §1.4 states PLAN S4 as PLAN does (it names only the
  enumerator's author).
- **OP-13-16 (spec sync 3: dropped bodies).** [AR §11] #33 and OQ-A-7 (a), decided 2026-10-06, add the invariant I-D1
  (§3.10) over [F06 §8.1]'s dropped bodies. Its three clauses come from R-SPEC-F's design: (a) no `Commit` appended after
  a `BodyDrop` carries a body it drops; (b) a dropped `BlobRef` (`file` = 0) appears exactly for dropped hashes; (c) after
  a completed purge no store file holds a dropped body's bytes. Two readings are added here:
  - (b) and (c) hold fully only once no purge is pending ([F16] P-101: an active extent holds a `BodyDrop` record of
    origin `command`), because a file written before the drop keeps its bytes until the purge rewrites it; (b)'s "only
    when" direction holds always. `doctor --fsck` checks each clause in that scope and reports a pending purge as a
    finding of its own.
  - (c) is checked structurally (`BLOBIDX`, `hist` body entries, full-text postings), not by searching bytes, which
    compressed files would hide; dictionaries cannot be checked, so [F16] P-101 replaces every live one.

  The model function is in `body`, beside I40′. Its gates start at M2, where bodies and `BodyDrop` are built
  ([API §8.7]); no M0 gate covers I-D1, which OP-13-13's list therefore does not name. I28′ and I40′ now say that an
  exported dropped body is its hash and reason ([F06 §8.1] DB-11, [F14 §6.9]).
  **Independent check of spec sync 3.** "Pending" counts only `BodyDrop` records of origin `command`: an import's record
  now names only hashes its store does not hold ([F05 §9.29]), so it leaves no bytes for (b) or (c) to find. A pending
  purge is the finding `purge_pending` of [F19] and [API §8.6] (named by R-SPEC-F), shown by plain `doctor` as well as by
  `--fsck`, since no automatic maintenance runs a purge ([F16] P-76). EP-IM also states that an import drops no held
  bytes.
  **Closure check of spec sync 3.** Bytes that only the `blobs.<n>` of a reservation whose bulk `Commit` has not landed
  holds are not held ([F05 §9.29] "Origin"), so an import may drop such a hash and no purge follows; the old file kept
  the bytes until `gc.cruft-delay`, and (c)'s check reported a violation for a state the protocol allows. Such a
  reservation's `Commit` cannot land carrying a dropped body ((a), [F16] P-34), so its files are now not live for (b)
  and (c), are released without `gc.cruft-delay` by the next `gc` or by a purge's step 7 once the `blobs.<n>` holds a
  dropped body ([F16] P-84, P-101), and are reported as a deletion still due until then. The other reading, counting
  those bytes as held, is not taken: the reservation's `Commit` may never land, which would leave a body key with no
  bytes that is not dropped.
