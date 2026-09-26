# 21 — Critique of proposals A, B, C, D: semantic correctness of versioning, merge, integrity and concurrency (revised for the owner update of 2026-09-25)

*Date: 2026-09-25. Status: research critique, nothing implemented; this file is the only artifact written (it replaces the earlier three-proposal version of the same file). Lens: does each design give the right answer, keep every acknowledged fact, never deadlock the ready queue, and never let two readers, two branches or two representations (store vs git image) hold different truths without a conflict record.*

**Inputs.** The digest [00]; reports [04], [06], [08] in full; [03 §2, §8], [05 §4–§7, §10, §13, §16], [07 §4, §7–§8] in the parts this lens depends on; the four proposals [A] `10-proposal-A-lean-embedded.md`, [B] `11-proposal-B-git-faithful.md`, [C] `12-proposal-C-agent-workflow-first.md`, [D] `13-proposal-D-branches-git-image.md`, read in full; the sibling critiques [20] and [22] for the labels D imports (G1–G14, F-A1…F-C8, §2.1–§2.9). Three external quotes that D's git-image design rests on were re-fetched today and are verbatim: jj's git-compatibility page ("Commits with conflicts cannot be represented in Git"; change-id header "is not preserved by all `git` tooling … not preserved through a rebase operation"), git's hash-function-transition ("using SHA-256 based storage on public-facing Git servers is strongly discouraged"), and gitoxide's crate-status ("[ ] push"; "[ ] Git 3.0 compatibility (`SHA-256`, `reftable`)").

**Owner update as the bar.** R1 full branching for all versioned data, R2 git independence, R3 a deterministic git-compatible image with round-trip. A, B and C were written before the update and are judged against it anyway: a design without R1/R3 is deficient on that axis, and I say where its other layers are still the best available.

**Method.** Every flaw comes with a concrete operation sequence traced through the proposal's own text, with the failing section quoted. Severity tags:

| Tag | Meaning |
|---|---|
| **WRONG ANSWER** | a query returns a value that contradicts the intended semantics |
| **DATA LOSS** | an acknowledged write, or a fact needed to interpret one, disappears |
| **DEADLOCK** | tasks that should become ready never do without manual repair |
| **DIVERGENCE** | two readers, two branches, or the store and its git image hold different truths with no conflict record |
| **LIVENESS** | writes fail or stall under the design's own numbers |
| **UNDERSPECIFIED** | the text does not say; the natural reading is wrong, or two readings differ |

---

## 0. Verdict

| Proposal | Score (0–10, this lens, R1–R3 as hard requirements) | One-line verdict |
|---|---|---|
| **A — lean embedded** | **4.5** | Cleanest engine invariants and test gates, but no branching beyond `exp/*` rebase-on-read overlays (R1 fail), a binary `refs/moirai/data` backup with no readable image or import of git-side edits (R3 fail), and its trunk semantics still land cycles and dangling edges, unblock dependents whose prerequisite is open, and deadlock fix rounds (X4, X5, X10). |
| **B — git-faithful** | **5.5** | The most rigorous merge engine of the four before D: structural violations never reach trunk, incomparable status moves are conflicts, commits hash over `uid`. But coordination is *never* branched by design (R1 fail), `stale`/`check` shell out to `git merge-base` (R2 partial), and the image is `hist` frames as blobs (R3: transport only). Its merge rules and staging are what D correctly inherits. |
| **C — agent-workflow first** | **4.0** | Best coordination semantics (leases restrict deletes, artifact gate, verdict routing as a field), but the merge engine is deferred to M5 with A's "always land" defect, counter fields sit under a scalar merge rule, refutations auto-flip, and R1–R3 are absent. |
| **D — branches + git image** | **7.0** | The only design that meets R1–R3, and it adopts every X1–X13 fix as spec. It introduces five new reachable wrong answers in exactly the mechanics R1/R3 add: the merge base is taken from before-images instead of the LCA state, so every lane that ran `sync` gets spurious conflicts (N1); a task completed on a lane is re-dispatched from `main` before merge (N2); a torn `RefUpdate` after an adopted commit strands an acknowledged write (N3); the git image is deterministic per store, not per data (N4); and one canonicalisation slip demotes the rest of an imported history to foreign (N5). All five are fixable without changing the architecture. |

**The two findings that matter most:** N1 (D's `sync` ritual produces wrong merges by D's own step 2) and N2 (D's headline claim that store-wide leases make double work impossible is false once `complete` releases the lease). Both are on the daily path of the owner's merge-queue workflow.

---

## 1. Findings shared by A, B and C (carried forward), and what D did with them

The earlier version of this critique traced thirteen shared defects on trunk semantics. They still stand for A, B and C; the table records D's answer so the rest of this document can concentrate on what is new.

| # | Defect (A/B/C) | Class | D's answer | Verified against D's text? |
|---|---|---|---|---|
| X1 | Exogenous-inheritance deadlock passes I5: `link X --blocks P` then `link C1 --blocks X` (C1 ∈ subtree(P)) is acyclic on `blocks ∪ child→parent` but nothing is ever ready | DEADLOCK | D6: I5′ adds the implied edges `{X→D : blocks(X,P), X ∉ subtree(P), D ∈ subtree(P)}`; `link X --blocks P` checks every descendant | yes [D §3 D6]; see N16 for the merge-time ordering hazard |
| X2 | Flushed-but-unpublished commit overwritten by the next writer; readers adopt records past `committed_lsn` | DATA LOSS | write path scans to the first bad checksum or epoch mismatch, adopts and re-flushes, republishes, *then* evaluates idempotency; readers stop at `committed_lsn` | yes [D §4.3]; but see N3 (the ref move is a separate record) |
| X2b | `Move` invalidates the stored exogenous classification | DEADLOCK | D6: `move` re-derives | yes |
| X3 | Idempotency keys not bound to the payload | WRONG ANSWER | I14′ payload-bound, `IDEM (key16, payload_hash16, branch_sym, result blob ref)`, exit 9 | yes [D §4.4, §7.1]; see N13(e) for the branch binding |
| X4 | Delete-with-replacement unblocks dependents whose prerequisite is open (A's own walkthrough step 13) | WRONG ANSWER | T5: `--replaced-by` re-points structural edges, otherwise a flagged edge keeps the dependent out of `ready` until `resolve` | yes [D §2 T5] |
| X5 | Verdict-as-`blocks` deadlocks the fix round after lease expiry or reopen | DEADLOCK | D5: `gates` edge constrains completion, `blocks` constrains start | yes |
| X6 | `mentions` to not-yet-allocated ids | DIVERGENCE | D8: only `#N < next_id` plus the sigil rule | yes |
| X7 | Expired-but-unreclaimed lease presented by its holder on resume | UNDERSPECIFIED | I17′: same-holder renewal; expiry never bumps the token | yes [D §6] |
| X8 | `suspect` both derived and user-clearable | UNDERSPECIFIED | D9: purely derived from `(target state, pinned_commit)`; re-confirm re-pins | yes |
| X9 | Maintenance inside the writer byte vs the writer timeout | LIVENESS | checkpoints/promotions/exports under the maintenance byte; writer byte for publish only | yes [D §4.3] |
| X10 | Admitting a `Cycle` violation onto trunk breaks Pearce–Kelly's precondition | WRONG ANSWER | structural violations are staged on `merge/<src>`, never advance a ref | yes [D §5a.8] |
| X11 | Per-node `rev` and counter-typed fields under scalar merge (the owner's 188-vs-191 bug) | WRONG ANSWER | D3 `rev_seq` = store seq of the last touching commit on this branch view; D4 counters are `Incr` ops; citations pin commit ids | yes; but see N6 for the git-image path |
| X12 | Two lanes supersede the same rule | DIVERGENCE | D11: ≤ 1 active superseder; `SupersedeFork` at merge | yes |
| X13 | What-if branches can mark tasks `done` and the lattice merges it forward | WRONG ANSWER | D2: `plan/*` branch kind with coordination fields read-only | yes, but over-applied to `blocks` (N9) |
| S1 | Leases on a deleted task | DATA LOSS (B) | D7: runtime lease records; `rm` refuses while a live lease covers a node in scope unless `--release` | contradicted by D §5d.3 row 3 (N8) |
| S2-B | Question `answered` on trunk with no visible answer | WRONG ANSWER | D10: `answered` derived from an `answers` edge visible on the reading branch | yes |
| S6 | Verdict has no derivation edge to the findings it counted | UNDERSPECIFIED | **not adopted** (D lists [21 §7] invariants, not the §6 graft) | N14 |
| S10 | As-of results mixing reverse-applied headers with current bitsets | WRONG ANSWER | I18′: no derived fields unless `--recompute` | yes |

A, B and C remain as scored in §4: the defects above are reachable on day one of their trunk-only v1.

---

## 2. New findings on proposal D

### N1. The merge base is "the earliest before-image", not the state at the LCA — WRONG ANSWER (spurious conflicts) on D's core `sync` ritual

[D §5a.7 step 2]: "**Changesets**: fold `ops(dst, LCA..tip)` and `ops(src, LCA..tip)` by walking the commits on each side sequentially … each fold is a map key → (**base value from the earliest before-image**, final value)." [D §5a.3]: "**Merging `main` into X** (`moirai sync`) … appends a merge commit on X whose changeset is `main`'s ops since the LCA, so the overlay grows by those ops." [D §10 risk 1]: "`sync` is one command and can be hooked to `SubagentStart`".

**Trace.** `main`: M0 (fork point) → M1 sets `#91.priority = 2` (was 1) → M2. Lane `lane/x` forked at M0: L1, L2 touch other nodes. `moirai sync` on the lane appends S with parents (L2, M2); S's changeset for `#91.priority` is `SetField{old: 1, new: 2}` — the before-image is the *lane's* value before the sync, because that is what `revert S` needs. `main` continues: M3 sets `#91.priority = 3`. Now `moirai merge lane/x --into main`. LCA(S, M3) = M2 (S reaches M2 through its second parent). `ops(src, M2..S)` = {L1, L2, S}: the earliest before-image for `#91.priority` is **1** (from S), final = 2. `ops(dst, M2..M3)` = {M3}: base 2, final 3. Typed merge for a number: "equal → take; one side = base → take the other; else `FieldEdit{base, ours, theirs}`" [D §5a.7 table]. With base = 1, ours = 3, theirs = 2: neither side equals base → **`FieldEdit` conflict** and `#91` becomes `conflicted`. The truth: the lane never touched `#91`; the correct answer is 3, clean. The same happens for every scalar `main` changed twice around a sync; for `status` the lattice hides it (both forward from `open`), for text it is worse: `body` diff3 against the *lane's pre-sync blob* with ours = v3, theirs = v2 — v3 edits v2, so the hunks overlap and every section `main` edited twice yields a `TextHunk`. Because D hooks `sync` to `SubagentStart`, a lane syncs many times per campaign, and the merge queue then meets a wall of conflicts that are not conflicts. Each one lands as a conflict value (no `--strict`), the node leaves `ready`, and an agent "resolves" a disagreement that never existed — exactly the silent-merge class the owner's register incidents belong to [02 §7.3], now produced by the engine.

**Why B did not hit this.** B's lanes are sparse and forked once; B's step 2 has the same wording ("the before-image recorded in the earliest S op") but B's `lane sync` "advances `base_lsn`" and the lane overlay is folded per key relative to that base, so the sync's ops carry base-relative before-images. D moved to a real DAG with merge commits and kept B's fold rule.

**Required fix.** The base of a key is its value **at the LCA commit**, computed by the as-of machinery [D §5a.6] (D pins a checkpoint set at every merge into `main`, so the LCA is usually a pin or within 4k ops of one), or equivalently: when folding a side, skip ops that a merge commit *imported* from the other side (mark ops in a merge commit with their origin side). Property test to add to M3: for every key untouched on side S, `merge` never produces a conflict on that key, for random DAGs with interleaved `sync`/`merge`. D's "the owner's two register incidents replay correctly" gate does not exercise this shape.

### N2. A task completed on a lane is re-dispatched from `main` before the merge — WRONG ANSWER on `ready`; D's "double work is impossible" claim is false

[D §5d.1 leases]: "a lease means 'an agent is working on this node now'; **visible from every branch**; `ready` on any branch excludes nodes leased by another holder … `release`/`reclaim`/expiry are store-level". [D §5d.2]: "A task completed on `lane/x` is `done` on `lane/x` only. `main` learns it at `merge lane/x --into main`". [D §10 risk 1 mitigation]: "leases are store-wide so double work is impossible even when views diverge". [D §7.5 step 7]: "`complete #89 --lease L-18` writes `done` on `lane/l5np`; `#93` becomes ready on the lane; on `main` it is still blocked (`ready --across` explains)".

**Trace.** `#89` exists on `main` and `lane/l5np`. dev#1 claims `#89` on the lane (lease L-18, branch `lane/l5np`). While the lease is live, `ready --branch main` excludes `#89` — correct. dev#1 finishes: `complete #89 --lease L-18` sets `done` on the lane and **releases the lease** (a `complete` ends a claim in every proposal; D's [B §6.2] base: "`complete`, `set`, `release` must carry the lease"). The lane is not yet merged (the merge queue runs `lane/l10` first, [D §7.5 step 8]). The orchestrator, bound to `main`, runs `ready --branch main --ids` → `#89` is `open`, unleased, unblocked → listed → `claim #89 --agent wf:r8/dev#2` succeeds (a lease may be taken on any branch where the node is live and ready [D §5d.1]) → dev#2 implements `#89` a second time in another worktree. Nothing warns unless someone typed `--across`. When both merge, `status` joins to `done` on the lattice and the duplicate work is invisible in the history. This is the owner's "two lanes building the same task" fear [07 §7.4] that D says its leases prevent.

**Required fix.** `complete` on a branch other than `main` must leave a **store-level runtime marker** `settled {uid, branch, commit, outcome}` that lives until the completing branch is merged into (or deleted from) every branch that still shows the task open; `ready` and `claim` on every branch treat a settled uid like a foreign lease and print `done on lane/x (c4470, unmerged)`. Equivalently: a lease is released by `complete` only into a "settled" state, not into "free". This is a small addition to the runtime table in [D §5d.1] and belongs in M1, not M3.

### N3. A torn `RefUpdate` after an adopted commit strands an acknowledged write — DATA LOSS / DIVERGENCE

[D §5a.2]: "Every ref move is a `RefUpdate` record written in the same flushed group as the commit that caused it". [D §4.2] lists `Commit` and `RefUpdate` as separate record kinds. [D §4.3 write path]: "(2) scan from `HEAD.committed_lsn` to the first bad checksum or epoch mismatch, adopt and re-flush complete unpublished commits, republish `HEAD`, *then* evaluate the idempotency key". [D §5a.10]: "Reachability = refs ∪ reflog entries younger than … ∪ pins".

**Trace.** P1 appends `Commit N` (ref = `lane/x`), then is killed by `taskkill` before appending `RefUpdate{lane/x: old → N}` [08 §2 "kills processes wholesale"]. The OS write-back leaves `Commit N` complete on disk. P2 takes the writer byte, scans forward, finds a complete `Commit N` with a valid checksum and epoch, adopts it and republishes `HEAD` — but no `RefUpdate` exists, so `lane/x` still points at `old`. `Commit N` is now in the log, carries the `Idem` record of P1's key, and is reachable from **no ref, no reflog entry and no pin**. P1's caller never got an ack (the crash preceded the flush) and retries with the same key and payload: "(2) … then evaluate the idempotency key" → hit → "already done", original result returned. The caller believes the write is on `lane/x`; `moirai show` on `lane/x` does not have it; after `gc.cruft-delay` (14 days) the commit is pruned. An acknowledged-by-retry write is lost, and between the retry and the pruning the store's idempotency table and its refs disagree with no conflict record.

**Required fix.** The ref move must be part of the commit record (the `Commit` body already carries `ref u16`; add `ref_old` and let adoption of a complete commit *imply* the ref move `ref_old → commit_id` — with a CAS check that the ref still equals `ref_old`, else stage it on `merge/<ref>` as an orphan for review). `RefUpdate` records then exist only for non-commit moves (`undo`, `branch`, `tag`, `op restore`). Add to the DST crash-point enumeration: kill between the two appends of a "flushed group".

### N4. The git image is deterministic per exporting store, not per moirai data — DIVERGENCE across stores; `gitmap` per algorithm cannot hold two oids for one commit

[D §5b goals]: "(1) every moirai commit, branch and tag maps to git objects by a **pure function of moirai data**, so two exporters produce byte-identical objects". [D §5b.4 trailers]: "`Moirai-Seq: 4410` (exporting store's seq; informational)" is in the commit message, which is hashed. [D §5b.2 rule 6]: "`id: <N>` is the exporting store's `#N`" is a header line of every `.moi` blob. [D §4.1]: "`gitmap.NNNN` sorted `(commit_id16, algo u8, git_oid[32])`". [D §5b.6 export step 2]: "walk the moirai commits from the tip down until a commit already in `gitmap` for this destination's algorithm".

**Trace.** Store S1 exports commit c (seq 4410 in S1) to `image.git`; `gitmap(S1)` records oid g1. S2 imports the image: c verifies natively, S2 assigns seq 9,102 and, for a node whose `#N` was taken, a new `#N` via the alias map. S2 records `gitmap(S2): c → g1` from the import. S2 now exports to a *second* SHA-1 destination (the owner's separate image repo plus a checkpoint mirror in the project repo is D's own recommended hybrid, [D §5b.8]). Step 2 stops at c because `gitmap` has an entry for the SHA-1 algorithm — but the destination has never seen g1, and re-encoding c from S2's data produces `Moirai-Seq: 9102` and different `id:` lines, i.e. an object with oid g2 ≠ g1. Either the exporter writes g2 while `gitmap` says g1 (the map lies; the next `image doctor --rebuild-map` reports every such commit as tampered), or it trusts `gitmap` and the destination's ref points at an object that does not exist there. Determinism also fails in the simplest case: two fresh stores that imported the same bundle and both export `main` produce different trees for identical moirai state, so a colleague's `git diff` between the two images shows `id:` and seq noise on every node.

**Required fix.** Remove every store-local datum from hashed content: no `Moirai-Seq` trailer (keep seq in the store-side `gitmap` row), and no `id:` line in `.moi` (put the `#N` alias hint in a separate, unhashed side file such as `refs/moirai/aliases/<store-id>` or a `.moirai-aliases` blob that the importer reads but the canonical tree excludes). Then goal (1) holds and `gitmap` per algorithm is sufficient. Property test: two stores importing the same bundle export byte-identical objects.

### N5. One canonicalisation slip demotes the rest of an imported history to foreign — DATA LOSS of native identity; the revert-of-delete transition is undefined in the image

[D §4.2]: "**Canonical form** hashed into `commit_id` (BLAKE3-256): **parent ids**, `kind`, `hlc`, actor/role/session as strings, git provenance, message, schema version, and the op list". [D §5b.6 import step 3]: "reconstruct the canonical form with the trailer metadata and **verify** BLAKE3 = `Moirai-Commit`; append as a moirai commit with the same id (kind `import-native`), parents mapped through `gitmap`. **A verification failure demotes the commit to foreign** … the moirai commit gets a **new** id". [D §5b.6 import step 3, ops from file transitions]: "a file added = `Create` + fields + edges; … a file turned tombstone = `Delete`; a file removed = `Delete{reason: image:file-removed}`". Nothing maps the transition **tombstone → live file**.

**Trace.** On S1: commit c1 deletes `#40` (`.moi` becomes a tombstone), c2 = `moirai revert c1` (the inverse changeset re-creates the row: the file returns to a live node, same `uid`). Export; import into fresh S2. c2's tree diff shows a tombstone turning into a live file. The importer's rules produce `Create` for "a file added" — but the uid exists, tombstoned; [D §5b.6 step 4] lists `IdCollision` for "a `uid` already live with different `created` commit" — the uid is not live, so the importer either emits a `Create` on a tombstoned uid (refused by I1: never reuse) or an undefined op. Whatever it emits, the reconstructed op list is not c2's (`revert` produced an undelete with before-images), so BLAKE3 ≠ trailer → c2 is **demoted to foreign** with a new id f2. c3 (child of c2) reconstructs its canonical form with "parent ids" — if it uses the id of the parent *as imported* (f2), the hash mismatches the trailer → c3 demoted → and so on for the whole remaining history: every later native commit on S2 is foreign, the `Moirai-Commit` identity that R3 promises is gone, and `image doctor` reports thousands of "tampered" commits. If instead the importer uses the *trailer* id of the parent for hashing, c3 verifies but is stored with a parent id (f2) that its own hash does not cover — an id that `doctor --verify` can never recompute. Neither branch of the "if" is specified.

**Required fix.** (a) Define the image transitions completely: tombstone → live = `Undelete{uid, before-image}` (the inverse of `Delete`, which D's `revert` already produces); live → tombstone = `Delete`; absent → live = `Create`; live → absent = `Delete{image:file-removed}`; tombstone → absent = no-op with a `Violation{TombstoneRemoved}` hint. (b) Make the canonical form's parent references the parents' **stated** ids (trailer values), store a per-commit `verified` bit, and let a demotion affect only the demoted commit: its children keep their native ids and point at the foreign commit through an explicit `parent_actual` field. (c) Fuzz the encoder/decoder pair on random op sequences including `revert`, `cherry-pick` and `undo` before M4 exits.

### N6. A git-side text merge silently loses `Incr` and D only *hints* — WRONG ANSWER (the owner's 188-vs-191 incident through the image path)

[D §5b.9 row 2]: "`git merge` of two exported branches on the git side … clean merges of disjoint lines are imported as a foreign merge commit and re-validated (typed invariants, not text, decide) … **the store never adopts git's text merge as truth without validation**". [D §5b.6 step 3]: "a two-parent foreign commit becomes a moirai merge commit whose changeset is the diff against its first parent plus a `Violation{ForeignMerge}` **hint** naming the second parent". [D §5a.8]: hints "land on the ref" as a log line. [D §5b.8]: the tracked-directory destination "git-side merges of `.moi` files are expected".

**Trace.** Base: `field incidents: 3` on hazard note `#77`. Branch `lane/a` records an incident (`Incr +1` → file line `field incidents: 4`); `lane/b` records another (`field incidents: 4`). Both exported; a colleague runs `git merge` on the image (tracked directory, PR review): both sides changed the line to the identical text `field incidents: 4` → git merges cleanly to 4. Import: the foreign merge's changeset is "the diff against its first parent" = nothing for `incidents` (4 → 4); the second parent's increment is gone. Truth: 5. D's typed validators (I2, I4, I5′, cardinality, schema) have nothing to say about a counter; the `ForeignMerge` hint lands and nobody reads hints. The same text merge turns a `status: in_progress` (ours) vs `status: done` (theirs) into conflict markers (good, staged as `ImageParse`), but `status: open → done` on one side and unchanged on the other merges clean and is correct; the counter case is the one that lies, and it is precisely the incident class [02 §7.3] the typed engine was built to stop.

**Required fix.** Counters must not be exported as a current value: export `Incr` as an append-only ledger (`incr incidents +1 c<commit>` lines, one per op, sorted by commit id) so a text merge unions the lines and the importer sums them; or, if a `counter`-typed field's line changed on both sides of a foreign merge, stage the merge with `Violation{ForeignMerge}` instead of a hint. More generally: a foreign two-parent commit should be imported as a moirai **merge** computed by moirai's own typed 3-way over the two parents' imported states, with the git-side tree used only to resolve the text conflicts the human chose — then the typed rules (lattice, `Incr`, add-wins) decide, and "the store never adopts git's text merge as truth" becomes true.

### N7. Criss-cross merges: two contradictory rules, and the shape D's own rituals produce — UNDERSPECIFIED, then LIVENESS

[D §5a.7 step 1]: "LCA of `tip(src)` and `tip(dst)` by gen-pruned bidirectional parent walk … (**multiple LCAs → the newest by gen**; recursive-merge of criss-cross bases is not implemented in v1: **a `CrissCross` violation is emitted and `--base <commit>` must be given**)". These are two different behaviours for the same input.

**Trace.** The orchestrator's merge queue runs `merge lane/x --into main` (commit Mm with parents (M5, L7)) while the lane's `SubagentStart` hook runs `sync` for a new subagent (commit S with parents (L7, M5)) — D recommends both [D §7.3, §10 risk 1]. Next `sync` or next `merge lane/x`: `tip(main) = Mm`, `tip(lane/x) = S`; the LCAs are {M5, L7}, neither an ancestor of the other. Under the second reading the merge is staged with `CrissCross` and cannot proceed without a hand-picked `--base`; under the first it proceeds with the newer-by-gen base and possibly spurious conflicts (git's `resolve` strategy). With three lanes and hooked syncs this shape is routine, and every occurrence is a staged merge the brief lists first [D §10 risk 5] — the pile-up D itself names as a risk.

**Required fix.** Pick one: implement the recursive base (merge the LCAs into a virtual base, as git's `ort` does; the typed merge already exists, and the virtual base needs no validators) or state that the newest-by-gen LCA is used and accept spurious conflicts (N1's fix makes those rarer). Delete the `CrissCross` violation or reserve it for > 2 LCAs.

### N8. `rm` with a live lease: D7 says refuse, §5d.3 says allow — contradiction on a store-wide record

[D §3 D7]: "`rm` and `--cascade` **refuse** while a live lease covers a node in scope unless `--release`". [D §5d.3 row 3]: "#40 leased by `dev#2` on `lane/y`, deleted on `lane/x` … `rm` prints `leased by dev#2 on lane/y (L-19)`; **allowed** (the lease is not on this branch's view) unless `--strict-leases`". But [D §5d.1]: leases are "keyed by `uid`, never by branch … **visible from every branch**". A lease cannot be both visible from every branch and "not on this branch's view". Implementers will read one of the two. The safe rule, consistent with D7 and with C's `holds` restrict that D cites as its source: `rm` refuses while any live lease covers the uid, on any branch, unless `--release` (which writes the triage note D7 already promises).

### N9. `plan/*` branches cannot add `blocks` edges — a what-if re-plan cannot express precedence

[D §3 D2]: branch kind `plan` = "coordination fields `status`, `resolution`, `assignee`, **and `blocks` edges are read-only**; claims refused", citing [21 X13]. X13's finding was that a what-if branch could mark work `done`; its fix was "coordination kinds and their **status** fields are read-only". Blockers are the substance of a re-plan ([04 §9.3] `exp/<name>`: "an orchestrator restructuring the task tree"). Under D2 a re-plan may create tasks and move them but may not say which must precede which; merging it into `main` drops nothing (there is nothing to drop) — the plan branch is useless for the one thing it exists for. Fix: read-only = `status`, `resolution`, `assignee`, leases; `blocks` allowed and validated by I5′ at write and at merge; `PlanStatusIgnored` stays for status.

### N10. `revert` and `cherry-pick` of ops that no longer apply — UNDERSPECIFIED (the SQLite `NOTFOUND` class is missing)

[D §5a.5]: "`moirai revert <commit>` appends the inverse changeset … **after running the validators**; refused with the dependent set if a later commit on R **depends structurally** on the reverted one". [D §5a.9]: "`cherry-pick <commit>`: 3-way apply of that commit's changeset with base = its first parent onto R's view".

**Trace.** c812 deletes `#40 --replaced-by #52` (ops: `Delete #40`, `RemoveEdge(#40 blocks #12)`, `AddEdge(#52 blocks #12)`). Later c900 deletes `#52` (its edge to `#12` is re-pointed again or flagged). `revert c812` inverts: `Undelete #40`, `AddEdge(#40 blocks #12)`, `RemoveEdge(#52 blocks #12)` — the last op targets an edge that no longer exists (its source is dead). D's validators check I2 (endpoints live) for *added* structural edges; for a `RemoveEdge` of a missing edge, and for a `SetField` whose before-image no longer matches (the `DATA` class), nothing is said. "Depends structurally" is not defined either: does c900 "depend" on c812? The SQLite session taxonomy D cites [04 §3.13] answers this: `NOTFOUND` (target row/edge missing) and `DATA` (before-image mismatch) are conflict classes with OMIT/REPLACE/ABORT handlers. D should stage a revert/cherry-pick on any `NOTFOUND` and land `DATA` as a `FieldEdit` conflict value, and define "depends structurally" as "a later commit added a structural edge to, or a child of, a node the reverted commit created, or completed a task the reverted commit reopened".

### N11. Import honours `#N` hints "if free", which re-binds a number to a different `uid` — I1's "never reused" weakened

[D §5b.6 step 5]: "`uid` is identity; `id:` hints are honoured **when free**, else the alias map". [D §5a.10]: unreachable commits are pruned after reflog expiry and cruft delay. [D §2 T4] / [B §3.5 I1]: "`#N` … never reused".

**Trace.** Branch `lane/z` creates `#7001` (uid u1), is exported, then deleted with `-D`; after 90 + 14 days its commits are pruned and the row for `#7001` is gone from every segment. `#7001` is now "free" below `next_id`. Importing a colleague's image whose node u2 carries `id: 7001` honours the hint. Old exported bundles, old `mentions` text ("see #7001") and the pruned reflog all meant u1; the store now renders `#7001` as u2 with no tombstone in between — the one aliasing that never-reused ids were supposed to make impossible [06 §8.2 item 4]. Fix: define "free" as "never allocated in this store" (a hint is honoured only if `N ≥ next_id` at import time, after which `next_id = N + 1`), otherwise remap through the alias map. The cost is that most hints from a sibling store are remapped — which is the honest outcome, since `#N` is store-local.

### N12. Versioned `flags` vs runtime leases; conflict values in the image — UNDERSPECIFIED

[D §3 D3] keeps [B §3.2]'s header `flags` including `claimed`, removing only `plane` and `proposed`; [D §3 D7] makes leases runtime records; [D §5b.2 rule 2] exports a `flags` header line and rule 7 excludes derived state "(`open_blockers`, `ready`, `suspect`, `is_blocker`, rollups, `conflicted`, `rev_seq`, `lsn`)" — `claimed` is not in that list. If `claimed` is a versioned bit set by `claim`, then `claim` is a commit on a branch (contradicting "leases are runtime, keyed by uid, visible from every branch") and the image exports who holds a lease; if it is derived from the lease table it must be listed as derived. State it. Second gap: a node carrying a conflict value has no single value for the conflicted field; [D §5b.2 rule 3] writes `conflict <key>: base= ours= theirs=` lines, but whether the ordinary `field <name>:` line is omitted, carries `ours`, or carries the markers is not defined — and a git-side reader that edits it produces an ambiguous import. Define: the field line is omitted while a conflict line exists for that key.

### N13. Smaller round-trip and coordination gaps (each with a concrete case)

(a) **Checkpoint-granularity commits cannot verify.** [D §5b.5 rule 6]: a checkpoint commit's `Moirai-Commit` is "the head commit id" while its tree diff is the fold of many commits; [D §5b.6 step 3] verifies BLAKE3 of the reconstructed ops against the trailer → always fails → every checkpoint import is "demoted to foreign" and gets a new id. Not wrong, but unstated: define `import-checkpoint` (new id, `Moirai-Folded` origin recorded) so `image doctor` does not report every checkpoint as tampered.

(b) **Two stores, one destination: the CAS after import.** [D §5b.9 row 7]: the second store "must `import` first … then export again". Export step 5 verifies the destination ref "equals the last exported oid for that ref"; after an import the last *exported* oid is still the second store's pre-import tip, not the imported tip → CAS fails again. Import must record the destination ref's oid as "last seen" for that destination.

(c) **Foreign `hlc` from committer time.** [D §5b.6 step 3]: foreign commits take "`hlc` from the committer time". A hand edit made days ago and imported today has an `hlc` below its imported parent's; Kleppmann moves are "applied in HLC order" [D §5a.7], so a foreign `parent:` change can be ordered *before* local moves it was made after, and a later `sync` can skip the wrong move. Clamp: `hlc = max(committer_time, max(parent.hlc) + 1)`.

(d) **Branch delete with live leases.** [D §5d.1]: `complete`/`set --lease` "write on the claimer's current branch (must equal the lease's branch unless `--move-lease`)". `branch -D lane/x` while L-19 carries `lane/x` → the holder's `complete` fails with no path but `--move-lease`. Release or re-bind leases on branch delete, with the triage note.

(e) **Idempotency bound to branch on resume.** [D §5d.1]: "replaying a key on another branch is an error (exit 9)". A Workflow paused for hours, resumed after the lane was merged and closed (`lane close` removes the binding, so the orchestrator's client key now resolves to `main`): the resumed `apply` replays `run:r7` on `main` → exit 9 → the resume fails at the one step designed to make it safe. Rule: a key hit with the same payload whose original branch has been merged into the caller's branch (or deleted after merge) returns the original result as success.

(f) **`undo` without a tip CAS.** [D §5a.5]: `undo` moves the client's branch back "N `RefUpdate`s ago". If another process committed on the same branch after the client last read it, the undo removes *that* commit. jj accepts this (repo-wide operations); an agent-driven store should CAS on the expected tip (`undo --expect c…`) by default.

(g) **Idempotency key hashes and `Moirai-Idem` in trailers** are fine; the `Moirai-Worktree` and `Moirai-Session` trailers are moirai data (from the commit) and hash stably — no issue.

### N14. Verdicts still have no derivation edge to the findings they counted (carried gap)

[D §3] adopts C's `verdict.counts`-as-query ("`stats loop`") and B's `about` edge, but no `verdict derived_from finding`. A `fail_fixable` verdict whose only finding was refuted (`refutes` edge, status `refuted`) keeps its `gates` edge on the task; `suspect` propagates along `derived_from`/`cites` only [B §3.6]. The task cannot complete until someone re-issues the verdict, and nothing marks it. Fix: `verdict derived_from finding` edges written by `remember{kind: verdict}` from its `about`/`counts` inputs; a verdict whose findings are all refuted/withdrawn becomes `suspect` and its `gates` edge is listed by `stale`.

### N15. Conflicted rule bodies are rendered with `<<<<<<<` markers into packs

[D §5a.7 table]: `TextHunk` "rendered with `<<<<<<<` markers on read"; [D §7.4]: C2 rules are taken from the caller's branch plus `~main` critical rules. A critical rule whose body conflicted at `sync` is injected into every developer's pack as a three-way text with markers, verbatim, at P0/L2 (never degraded). The agent either follows one side at random or stops. Packs should render a conflicted knowledge node as a one-line `~conflicted (moirai resolve '#212.body')` with the *base* text, never the markers.

### N16. I5′ at merge: moves must be applied and the implied edges re-derived before the cycle check (implementation hazard, not a design flaw)

[D §5a.7 step 5]: "I5′ (full Kahn … when the merge touches > 1,000 precedence edges, incremental PK otherwise)". The implied edges `{X→D : blocks(X,P) exogenous, D ∈ subtree(P)}` depend on subtree membership, which the same merge may change through Kleppmann moves (step 3). If PK is run edge by edge in changeset order before the moves settle, an edge can be accepted that becomes cyclic after a later move in the same merge. State the order: apply all `parent` moves, derive the implied edge set, then check every added `blocks`/`gates` edge and every implied edge whose endpoints moved. Add the merge variant of X1 (branch 1 adds `X blocks P`, branch 2 moves `C1` under `P` and adds `C1 blocks X`) to the M3 property tests; D's list ("every conflict class") does not name it.

### Minor and wording

- [D §4.5]: "the epoch in headers rejects records from a restored older copy of the store" — a restored copy carries the same epoch; the epoch rejects records from a *different* `init`. The restored-copy case is handled by the adopt-and-republish scan, which is the right mechanism; the sentence just misattributes it.
- [D §2 T14]: "sandboxed agents may not run git [08 §2]" — [08 §2] says the Bash sandbox is unsupported on native Windows and, elsewhere, allows writes to the shared `.git`; it does not say agents cannot run `git`. R2 needs no such claim.
- [D §5a.3]: the branch-view formula names only forks from `main`; a branch forked from a branch (allowed by [D §5a.1]: "a branch can fork from a branch") needs the recursive form `view(Y) = view(X)@fork ⊕ ops(Y)`. Promotion makes it cheap; the spec should say it.

---

## 3. Scenario traces

Legend: **ok** correct by the text; **wrong** wrong answer / data loss / deadlock; **under** underspecified, natural reading fails; **n/a** not in the design (A/B/C lack R1/R3 features).

| # | Scenario | A | B | C | D |
|---|---|---|---|---|---|
| S1 | `#40` deleted while another process holds a lease on `#40`, or on a task blocked by `#40` | under / **wrong** (X4) | under / **wrong** (X4) | **ok** / **wrong** (X4) | **ok** on `#12` (X4 fix, `gates` vs `blocks`); **under** on `#40` (N8 contradiction) |
| S2 | delete on branch A vs new edge to it on branch B, then merge | under, **wrong** at merge (X10) | **wrong** (question plane hole), merge ok | n/a until M5; then **wrong** | **ok** (`DanglingEdge` staged; `--replaced-by` suggested) |
| S3 | two acyclic blocker graphs whose merge deadlocks through inheritance | **wrong** (X1) | **wrong** (X1) | **wrong** (X1) | **ok** by I5′; **under** ordering with moves (N16) |
| S3b | explicit cycle (`A blocks B` + `move B under A`) | **wrong** (lands) | **ok** | **wrong** (lands) | **ok** (staged) |
| S4 | concurrent claims, two subagents, two worktrees (now also: two branches) | **ok** | **ok** | **ok** | **ok** while the lease lives; **wrong** after `complete` before merge (N2) |
| S5 | Workflow resume replays writes | **wrong** (X3), under (X7) | partial, under (X7) | **wrong** (X3), under (X7) | **ok** (I14′, I17′); **wrong** when the lane was closed meanwhile (N13e) |
| S6 | retraction of a finding cited by a summary and a decision | ok; under (X8) | ok; under (X8) | ok; **wrong** (`counts`, `summary.status`) | **ok** (D9); verdict gap remains (N14) |
| S7 | id collisions across branches / clones | ok in-store; 64-bit uid | ok | ok | **ok** (uid); `#N` re-binding on import (N11) |
| S8 | crash between op-log append and index/`HEAD` update | under (X2) | under (X2) | under (X2), no LSN | **ok** for the commit; **wrong** for the ref move (N3) |
| S9 | reader snapshot during compaction | ok | ok | contradictory (registry) | **ok** (pins + delete-pending) |
| S10 | as-of query across a schema change | ok primary; under derived | ok (I11) | ok; under derived | **ok** (I18′, symbols never GC'd, schema per branch) |
| S11 | fix round after a fail verdict (ready-queue liveness) | **wrong** (X5) | **wrong** (X5) | ok | **ok** (`gates`) |
| S12 | parallel refuters disagree on one finding | ok | ok (`StatusFork`) | **wrong** (auto-flip) | **ok** (incomparable lattice → conflict) |
| S13 | pinned citation after a two-sided section edit | under (X11) | **wrong** (X11) | under (X11) | **ok** (pin = commit id; `rev_seq` > both) |
| S14 | checkpoint in the writer lock under 16 writers | ok ≤1e5; liveness at 1e6 | same | liveness at 1e5 | **ok** (maintenance byte) |
| O1 | merge of two branches that both changed status / done / blockers / the same rule text | n/a (trunk) | lattice + `StatusFork` + diff3 (knowledge only) | n/a (M5) | **ok** in isolation; **wrong** after any `sync` (N1); conflicted rules leak markers into packs (N15) |
| O2 | claim/lease on one branch while the task is edited or deleted on another | n/a | n/a (claims trunk-only) | n/a | edit: **ok** (per-branch status, `DeleteVsModify` at merge); delete: **under** (N8); completion: **wrong** (N2) |
| O3 | revert / cherry-pick of a commit that deleted a node | `undo` without validators (item 14) | n/a | `undo` refused on dependents | validators run; **under** for `NOTFOUND`/`DATA` (N10) |
| O4 | export → git → hand edit / git merge of two exported branches / deleted files → import | n/a (binary backup) | n/a (`hist` frames; `export md` never re-imported) | n/a | hand edit **ok**; deleted file **ok**; git merge **wrong** for counters (N6); revert-of-delete **wrong** cascade (N5); two-store CAS **under** (N13b) |
| O5 | git-image hash stability across moirai versions / stores | n/a | n/a | n/a | per-store only (N4); format version pinned but no old-format emitter (below) |
| O6 | full operation with no git installed | store needs a `.git` common dir or `MOIRAI_DIR`; no `init` | `init`; `stale`/`check` spawn `git merge-base` | store needs `.git`; no `init` | **ok** (discovery chain, own reader, spawn only for optional push) |

**O5 addendum.** [D §5b.3] versions the image (`moirai-image 1`, `schema-version`) and [D §5b.7] promises `export → import → export` byte-identical — within one moirai version. Nothing says a newer moirai re-exports an old commit with the *old* rules; if the `.moi` encoder changes (a new header field, float formatting), a fresh store importing an old image and exporting to a new destination produces different objects for the same commits, and N4's `gitmap`-per-algorithm assumption breaks the same way. Rule to add: the exporter keeps every past `.moi` format version as a selectable encoder and an image records the version each destination uses.

---

## 4. Per-proposal critique against R1–R3

### 4.1 Proposal A — "Lean embedded" (4.5)

**R1.** "Shared live trunk for everything coordination-like … explicit `exp/*` what-if branches only" [A §2 T3]; branches are "a delta overlay that rebases on read: the branch view is the current trunk state with the branch's ops applied on top" [A §5.2]. Not first-class branches for all data; and the rebase-on-read view has no defined state for an op whose target vanished on trunk (S2), so even `exp/*` is semantically unsound until merge. **R2.** Store in `<git-common-dir>/moirai/`, `.git` read without spawning git, `MOIRAI_DIR` override [A §4.1] — the core runs without a git *binary* but requires a git *layout* or an env override; no `init`, no pointer file, no "miss never creates" rule. **R3.** "publish log and base files as git blobs under `refs/moirai/data` for backup" [A §5.7] — opaque binary segments, no per-node layout, no import of git-side edits, no id mapping. Fails R3.

**Best-available layers.** The engine invariants I1–I11 and the M0 exit gates (DST with crash-point enumeration, 16-writer kill loops) [A §3.4, §9]; coordination ops refused off-trunk [A §5.4]; `rm --dry-run` impact report and `--replaced-by` [A §7.1]; delete-pending GC without a reader registry [A §4.10]. D reuses all of it.

**Still wrong on trunk day one:** X1, X2, X2b, X3 (`result_hash` only), X4 (its own walkthrough), X5, X6, X7, X8, X9 at 1e6, X10 (default merge lands `Cycle`/`DanglingEdge` and `is_blocker` can list a tombstone), X11 (`rev`, `cites.pinned_rev`), X12, X13, S1 (leases on a deleted task), `undo` without validators, `HEAD.refs[64]` overflow location.

### 4.2 Proposal B — "git-faithful" (5.5)

**R1.** "the coordination plane (task status, claims/leases, `blocks`, lanes, runs) is a single linearizable trunk that is never branched" [B §1 item 2]; lanes are "sparse and live-rebased" knowledge-only branches; `exp/` full branches "in v2" with claims refused but status editable (X13). This is a deliberate, argued violation of R1 — the reports' recommendation [04 §9.3], [08 §7.2] — and the owner has overruled it. **R2.** `init`, "a miss inside a worktree never auto-creates a store", discovery reads `.git`/`commondir` textually [B §4.9]; but `stale`, `check #ID` and `reconcile` "shell out to `git merge-base --is-ancestor`" [B §5.6] — a query path that fails without git (and costs 74 ms per miss, [20 F-B6]). **R3.** `push` writes "retired `hist.NNNN` frames and a `head.json` as git blobs" [B §5.6]; `pull` imports with `uid → #N` mapping and re-verifies canonical hashes — a real transport with id mapping and hash verification (the seed of D's `gitmap`), but not readable, not diff-friendly, and git-side edits are impossible by construction; `export md` is "never re-imported". Transport yes, image no.

**Best-available layers (inherited by D).** I12 with `refs/merge/<lane>` staging; the incomparable finding lattice and `StatusFork`; `OwnerFieldEdited`; canonical hashing over `uid`; full idempotent results; the verbatim-removed-text guard as a merge violation; `WriterInfo` outside locked bytes; M3's replay of the owner's two register incidents.

**Still wrong:** X1, X4, X5 (verdict blocks + immutable verdicts), X11 (`section.rev`), the question-plane hole (S2-B), lane structural edges bypassing trunk restrict until merge, `merge --continue` fast-forward after trunk moved, X2, X7, leader forwarding without keys.

### 4.3 Proposal C — "Agent-workflow first" (4.0)

**R1.** "One live trunk for everything coordination-shaped … knowledge is scoped, not branched … explicit moirai branches (`exp/<name>`) only for what-if re-plans, M5" [C §2 T3]; `exp/*` "the same overlay structure as the tail" [C §5.2] — A's rebase-on-read with the same hole — while the merge "finds an LCA by `parent_seq`" [C §5.5], i.e. the two halves assume different branch models. **R2.** `<git-common-dir>/moirai/` only [C §4.1]; no `MOIRAI_DIR`, no `init`; ancestry "via `git merge-base --is-ancestor`" cached in the log [C §4.7] (a spawn, but cached and shared — the better placement). **R3.** "Optional M7: publish history as git objects under `refs/moirai/data`" [C §5.6]. Fails R3.

**Best-available layers (D adopts most).** Leases restrict deletes (`holds`, [C §3.3]); I11/I12 (retest ≠ re-review; expected artifacts); verdict routing as `phase_state`/`return_to`; `undo` refused on structural dependents; `stats loop`/refuted share; `resource` mutex; the pack algorithm.

**Still wrong:** X1, X11 (`incidents`, `reopen_count`, `verdict.counts`, `round` under a scalar rule), S12 (I6/I7 auto-flip), X9 (250 ms vs 0.1–0.3 s checkpoint), `done: bool` stored vs derived, `summary` status `suspect` next to the flag, the `seq % 1024` reader registry, no LSN/epoch in `RecHdr`, X10 at M5, X3, X2, X7.

### 4.4 Proposal D — "Branches + git image" (7.0)

**R1.** Met: `main` is a branch; `lane/*`, `plan/*`, `merge/*`, `tags/*` kinds; per-client HEAD; create/switch/list/delete/diff/log/merge/tag/revert/cherry-pick/rebase/reflog/undo/`op restore` all specified [D §5a.1–5a.10]; the runtime-vs-versioned split is justified per item [D §5d.1]. **R2.** Met: discovery chain with `--store`, `MOIRAI_DIR`, `.moirai` directory or pointer file, and a `.git` textual *hint* [D §5c]; no `git` spawn in the core; own pack/loose reader for provenance; push/pull spawn `git` only when present and otherwise print the command. **R3.** Met in shape: `.moi` canonical text per node, tombstones as files, two-level fan-out, trailers, `refs/moirai/*` or a separate repo, own writer/reader, `gitmap`, native/foreign import, an explicit lossless/lossy table [D §5b.1–5b.10]. The defects are in the details, and they are the ones a round-trip test would find first (N4, N5, N6, N13a–b).

**Strengths on this lens beyond A/B/C.** Every X1–X13 fix is in the spec (§1 table). `rev_seq` above both merge inputs and citations pinned to commit ids close X11 properly. `merge --continue` re-runs against the current `dst` tip with staged resolutions as an overlay — the rebaser idea done right. Structural violations never land; value conflicts do; `plan/*` cannot mark work done. Leases carry the branch they were taken on; `claim` requires readiness on the claimer's branch. The branch view as pinned checkpoint ⊕ trunk ops to fork ⊕ own ops is the correct git-like isolation (a trunk delete does *not* mutate a lane), and the cross-branch story is explicit: "every branch that *receives* the delete … applies it atomically, and every other branch can *see* it on request" [D §5d.3]. The image keeps tombstones as files so "deleted ≠ absent" survives git merges — the Beads JSONL lesson [08 §6.3] applied. Conflict values are representable in the image, which jj cannot do (verified quote). The cost table for 50 branches is honest. The three external precedents I re-fetched are quoted accurately.

**Fatal (for the default configuration).**
1. N1 — spurious merge conflicts on every key `main` changed before and after a lane's `sync`; `sync` is the design's primary liveness mechanism.
2. N2 — a task completed on a lane is dispatched again from `main`; the "double work is impossible" claim is false.
3. N3 — an adopted commit without its `RefUpdate` is acknowledged through the idempotency table and then pruned.

**Serious.**
4. N4 — `Moirai-Seq` and `id:` in hashed content make the image deterministic per store; `gitmap` per algorithm then lies across destinations.
5. N5 — canonical parent ids plus "demote on mismatch" cascade; tombstone → live transition undefined.
6. N6 — git-side text merge loses `Incr`; `ForeignMerge` is only a hint.
7. N7 — criss-cross rule stated two ways; the shape is routine with hooked `sync` plus a merge queue.
8. N8 — `rm` with a live lease: D7 vs §5d.3.
9. N9 — `plan/*` cannot add `blocks`.
10. N10 — `NOTFOUND`/`DATA` classes missing from `revert`/`cherry-pick`; "depends structurally" undefined.
11. N11 — `#N` hints honoured "if free" re-bind numbers after GC.
12. N13(a–f), N14, N15, N16 as listed.

**Minor.** Epoch wording; "sandboxed agents may not run git"; branch-of-branch view formula; the `.moi` block-string rules must be fuzzed (D says so).

---

## 5. Claims that are wrong or unverified

| # | Where | Claim | Status |
|---|---|---|---|
| 1 | D §5b goal (1), §5b.5 | "two exporters produce byte-identical objects" / "a pure function of moirai data" | **Wrong** while `Moirai-Seq` (trailer) and `id:` (`.moi` line 3) are hashed (N4). |
| 2 | D §10 risk 1, §5d.1 | "leases are store-wide so double work is impossible even when views diverge" | **Wrong** after `complete` releases the lease and before merge (N2). |
| 3 | D §5a.7 step 1 | multiple LCAs → "the newest by gen" *and* "a `CrissCross` violation is emitted and `--base` must be given" | **Contradictory** (N7). |
| 4 | D §3 D7 vs §5d.3 row 3 | `rm` refuses / is allowed while a live lease exists on another branch | **Contradictory**; also inconsistent with "visible from every branch" (N8). |
| 5 | D §5b.9 row 2 | "the store never adopts git's text merge as truth without validation" | **Wrong** for counters and any typed field whose text merge is clean but semantically lossy; validation covers structure only (N6). |
| 6 | D §5b.6 step 3 | "A verification failure demotes the commit to foreign" as a contained event | **Wrong/underspecified**: with parent ids in the canonical form the demotion cascades or leaves unverifiable ids (N5). |
| 7 | D §5a.7 step 2 | the merge base is "the earliest before-image" on each side | **Wrong** whenever a side contains a merge commit from the other side (N1). |
| 8 | D §4.5 | "the epoch in headers rejects records from a restored older copy of the store" | **Mis-stated**: a restored copy has the same epoch; the adopt-and-republish scan is what handles it. |
| 9 | D §2 T14 | "sandboxed agents may not run git [08 §2]" | **Not in [08 §2]**; the sandbox note there is about Windows support and `.git` write permissions. |
| 10 | D §2 T15, §12 [S1] | jj: "Commits with conflicts cannot be represented in Git"; change-id header "not preserved by all `git` tooling" | **Verified verbatim** today (docs.jj-vcs.dev/latest/git-compatibility). |
| 11 | D §5b.8, §12 [S4] | "using SHA-256 based storage on public-facing Git servers is strongly discouraged"; `extensions.objectFormat`/`compatObjectFormat`; idx v3 compat map | **Verified verbatim** today (git-scm.com/docs/hash-function-transition). |
| 12 | D §2 T10, §12 [S5] | gix has "no push" and lists "Git 3.0 compatibility (SHA-256, reftable)" as open work | **Verified** today (crate-status.md: "[ ] push"; "[ ] Git 3.0 compatibility (`SHA-256`, `reftable`)"). Note gix *does* list loose-object writing complete and pack writing partial ("[ ] write index along with the new pack"), so D's "gix has pack writing" is generous. |
| 13 | D §5a.10 | reflog expiry 90 days and cruft delay 14 days are "git's defaults" | **Correct** (`gc.reflogExpire`, `gc.pruneExpire`; [04 §3.1]). |
| 14 | D §5b.5 rule 1 | tree entries sorted "bytewise on name, directories compared as `name/`" | **Correct** (git's tree sort rule). |
| 15 | D §5b.6 step 4 | bundle "v3 for SHA-256" | **Correct** (bundle v3 carries `@object-format`). |
| 16 | D §5b.10 | full export 0.5–2 s at 1e5, incremental 5–35 ms | **Est.**; the per-touched-node as-of read in step 3 is O(edits after the target) per (commit, node) pair; fine at 10 edits/node, unpriced. |
| 17 | D §5a.11 | merge of a 2k-op lane 10–40 ms at 1e5 "sequential folds, not chain reads" | **Est.**; valid only if the base comes from the fold (N1's *wrong* rule); computing the base at the LCA adds an as-of read per touched key. |
| 18 | D §9 M3 gate | "10 synthetic lanes × 1k ops with every conflict class merge deterministically" | **Insufficient**: no `sync`-then-merge shape (N1), no criss-cross (N7), no move-plus-inheritance merge (N16). |
| 19 | A §5.4, C §5.5 | "conflicts as data, the merge always lands, so agents are never blocked" | **Wrong** for structural violations (X10); B and D are right. |
| 20 | A, B, C, D | SubagentStart/Stop/PostToolUse(Agent) fire for Workflow `agent()` calls | **Unverified** [07 §8.6]; D schedules the experiment in M0. |
| 21 | B §2 T4 | Beads switched to hash ids because sequential ids collided across *clones* | [03 §2.4] says agents and branches; clones are Dolt's argument [06 §9.2]. |
| 22 | C §4.1 | reader registry "so a compactor can find the oldest live snapshot" | **Wrong**: `seq % 1024` aliases; not redb's protocol. |
| 23 | A/C | "a retry returns the original result" | Only `result_hash` stored (X3); D stores the result blob. |

---

## 6. Positions on the forks (this lens; T3′ replaces T3)

| Fork | Best position | Evidence | What would change my mind |
|---|---|---|---|
| **T1** | **D**: B's segments + overlay with the [20]/[21] rules as spec (LSN + epoch in headers, readers stop at `committed_lsn`, writer adopts and republishes, maintenance byte, per-log commit index, pins refcounted in `HEAD`), **plus N3's fix**: the ref move is inside the commit record. | 1PC+C needs the log as truth and `HEAD` as a hint [05 §7]; immutable segments remove reader registration; pins make branch bases free; N3 shows the last separate record. | A measured overlay cost forcing a CoW B+tree; then reader pinning and 2PC return. |
| **T2** | **D/A**: purely embedded, blocking `LockFileEx` wait, no leader before M6, forwarded writes always keyed. | Leader death is the only new failure mode [08 §9.2]; keys make it benign. | Writer p99 > 50 ms with 16 writers after G1. |
| **T3′** | **D's full branching**, with: base at the LCA (N1), a `settled` runtime marker so completed-unmerged tasks are excluded everywhere (N2), one criss-cross rule (N7), `blocks` writable on `plan/*` (N9), atomic commit+ref (N3). Keep D's lease rules (store-wide, branch-carrying). Reject A/C's shared trunk and B's planes as the *default* — the owner ruled — but keep B's staging and violation taxonomy, which D does. | R1; D's isolation story is the only one that makes "delete on one branch" well-defined (S2 is **ok** only for D); the four new wrong answers are all in the merge/lease mechanics and each has a local fix. | A campaign in which staged merges and `sync` conflicts dominate the orchestrator's time even after N1/N7 — then D's own fallback (an opt-in `shared` field class for status and `blocks`, [D §2 T3′]) is the next step, not a return to planes. |
| **T4** | **D**: store-global `#N` under the writer byte, 128-bit `uid` in canonical hashes and the image, **plus N11**: an import honours an `id:` hint only if `N ≥ next_id`. | [06 §9.2]; N11 shows "if free" re-binds numbers after GC. | Cross-machine writes in v1 → `uid` primary, `#N` alias from day one (all four agree). |
| **T5** | **D**: hard delete + tombstone + before-image; `--replaced-by` re-points; flagged dangling blocker keeps the dependent out of `ready`; leases are runtime records; tombstones exported as files. **Resolve N8** as "refuse while any live lease covers the uid unless `--release`". | X4 fixed; C's `holds` restrict was right; [08 §6.3] tombstones. | Owner rules a deleted blocker always means "no longer required" → drop-and-notify. |
| **T6** | **B/D**: ≤ 64 KiB inline, content-addressed, dictionary-compressed; body as the tail of the `.moi` file. | diff3 and the removed-text guard need text in-store; dedup matters more with branches. | Owner wants prose only in the repo. |
| **T7** | **D**: typed 3-way; structural violations staged, value conflicts land unless `--strict`; incomparable lattice moves are conflicts; `Incr` for counters; `supersedes` ≤ 1; `OwnerFieldEdited`. **Plus**: base at the LCA (N1); `NOTFOUND`/`DATA` classes for revert/cherry-pick (N10); a recursive or fixed LCA rule (N7); foreign git merges recomputed by the typed engine, not adopted from text (N6); conflicted knowledge rendered without markers (N15); the I5′ ordering (N16). | [04 §5], Dolt/TerminusDB/jj precedents; X10; the owner's incidents. | Owner prefers "always land" for structural classes — acceptable only with `topo`-free fallbacks for flagged components. |
| **T8** | **D** (= A/B/C + zero-filled extents): one data-only flush per durable commit, `HEAD` unflushed, lazy class for heartbeats/cursors, fsync failure fatal; ref moves and client-HEAD moves durable. | [05 §2.2]; [08 §8.2]. | A 100 ms loss window for everything changes nothing here as long as acks follow flushes. |
| **T9** | **D** (C's packs + B's CLI + [22]'s fixes) with `--branch` on every verb and `branch: … · rev …` on every result line; **plus** N15 and the `settled` notice in `ready`/`brief`. | [07 §5.4]; [22 §2]. | Hooks proven not to fire for Workflow `agent()` → dispatcher-only. |
| **T10** | **D's list**; on this lens the hand-written pack/loose reader and `.moi` codec are the new correctness surface — keep D's differential tests against `git cat-file`/`fsck`/`log` when git is present, and add a fuzzed round-trip corpus; `gix` may serve as an *optional import oracle* (it reads packs/refs fine; it lacks push and SHA-256, which is why it cannot be the writer). | verified crate-status; N5. | Owner allows `gix` in the product → use it for import only. |
| **T11** | Any; results must carry `seq` and branch (D does). | — | — |
| **T12** | **D**: schema as data per branch, weakening merges, strengthening staged as `SchemaConflict`; symbols never GC'd; status lattices versioned with the schema. | S10. | — |
| **T13** | **D's order** (branches core in M1; merge M3; image M4; MCP M5) with [22]'s S0 oracle backend, and with N1, N2, N3 fixed **before M3's exit** and N4, N5, N6 before M4's, each with the property tests named in §2. Add the missing M3 shapes (sync-then-merge, criss-cross, move+inheritance merge). | The new wrong answers are reachable in the first campaign that opens a lane. | Owner's first target is publishing → M4 before M3 is fine *if* N4/N5 land first. |
| **T14** | **D**: `--store`, `MOIRAI_DIR`, `.moirai` dir or pointer file, `.git` as a textual hint only; `init` explicit; never auto-create on a miss; textual `.git` reader for provenance; no spawn in the core. | R2; Beads phantom DB [03 §2.7]. | — |
| **T15** | **D's `.moi` layout and object mapping** with: no store-local data in hashed content (N4); canonical parent refs by stated id and a `verified` bit (N5); `Undelete` transition (N5); counters as append-only `incr` lines or staged foreign merges (N6); conflict-line vs field-line rule (N12); `import-checkpoint` kind (N13a); destination ref recorded on import (N13b); `hlc` clamp (N13c); old-format encoders kept (O5). SHA-1 default for anything pushed (verified [S4]); separate image repo at `commit` granularity as the default destination; tracked directory only at checkpoint granularity and only after N6. | R3; verified precedents; jj's inability to represent conflicts is the strongest argument for D's text format. | If the owner wants PR review of the image above all, N6's ledger form becomes mandatory, not optional. |
| **T16** | **D's split** (all graph data versioned; leases, fencing, idempotency, cursors, `next_id`, HEAD bindings, pins, `gitmap`, alias map runtime) **plus**: `settled` markers (N2); `claimed` derived from the lease table, never a versioned flag (N12); leases released/re-bound on `branch -D` (N13d); idempotency replay tolerant of merged-then-closed branches (N13e); `undo` with a tip CAS (N13f). "Node 40 deleted on one branch" = D §5d.3 as written. | [07 §7.4]; D §5d.1's justification is sound; the gaps are the seams between runtime and versioned state. | Owner wants leases per branch (parallel what-ifs claiming the same task) → key by `(uid, branch)` with a cross-branch warning, as D offers. |

---

## 7. Grafts

Winner on this lens: **D**, on condition that N1–N3 are fixed before its M3 exit and N4–N6 before M4's. Adopt:

From **A**: `rm --dry-run` impact report (D has `rm` policies but not the dry-run verb in §7.1); the exit-code table with the current value on 4 and the tombstone on 3 (D keeps it); delete-pending GC (D keeps it).

From **B**: everything D already took; additionally B's statement that trunk-sourced structural edges may not target proposed nodes has a D analogue worth keeping explicit: a structural edge from `main` to a node that exists only on a lane is impossible by isolation — say so, so `link` errors are explained.

From **C**: `holds`-style refusal of `rm` while leased (resolve N8 in C's direction); I12 expected-artifacts gate (D adopts I11′/I12 via [22]); `stats loop`/refuted share (D has); the `consumed`-edge pinning as opt-in `--record-run` (D has).

From the earlier version of this critique, **not yet in D**: `verdict derived_from finding` (N14); as-of results without derived fields (D has I18′).

**New, from this critique:** base at the LCA with a property test on untouched keys (N1); `settled` runtime marker and cross-branch exclusion in `ready`/`claim` (N2); ref move inside the commit record, `RefUpdate` only for non-commit moves, crash-point between the two appends in DST (N3); no `Moirai-Seq`/`id:` in hashed content; alias hints in an unhashed side blob (N4); `Undelete` transition; canonical parent refs by stated id; per-commit `verified` bit; encoder/decoder fuzz over `revert`/`cherry-pick`/`undo` (N5); counters as `incr` ledger lines or staged `ForeignMerge`; foreign merges recomputed by the typed engine (N6); one LCA rule, recursive base preferred (N7); `rm` refuses across branches unless `--release` (N8); `blocks` writable on `plan/*` (N9); `NOTFOUND`/`DATA` conflict classes and a definition of "depends structurally" (N10); `#N` hints honoured only when `N ≥ next_id` (N11); `claimed` derived; conflict-line/field-line rule (N12); `import-checkpoint`, destination-ref-on-import, `hlc` clamp, lease re-binding on `branch -D`, idempotency replay across merged branches, `undo --expect` (N13); conflicted knowledge rendered without markers (N15); moves-then-implied-edges-then-PK at merge, with the merge variant of X1 in M3's tests (N16); past `.moi` encoders retained (O5).

---

## 8. Invariants to add to D

| ID | Invariant | Closes |
|---|---|---|
| I25′ | For every key untouched on side S since the LCA, `merge` never emits a conflict on that key; the base of a key is its value at the LCA | N1 |
| I26′ | A `uid` completed on any branch and not yet merged into branch R is excluded from `ready` and `claim` on R, and rendered as `done on <branch> (unmerged)` | N2 |
| I27′ | Every commit reachable in the log after recovery is reachable from a ref, the reflog or a pin, or is marked orphan and never satisfies an idempotency lookup | N3 |
| I28′ | The git object ids of a moirai commit are a function of moirai data and the object format only; two stores holding the same commit produce identical objects | N4 |
| I29′ | Importing an image exported by the same format version reproduces every native commit id; a demotion affects one commit only | N5 |
| I30′ | A foreign two-parent git commit is imported as a moirai merge computed by the typed rules over the two parents' states; counters are never taken from a text merge | N6 |
| I31′ | `merge` has exactly one base-selection rule for multiple LCAs | N7 |
| I32′ | `rm` refuses while any live lease covers the uid on any branch unless `--release` | N8 |
| I33′ | On a `plan/*` branch, `status`/`resolution`/`assignee`/claims are read-only; `blocks`, `parent`, `gates` are writable and validated | N9 |
| I34′ | `revert` and `cherry-pick` stage on `NOTFOUND` and record `DATA` mismatches as conflict values | N10 |
| I35′ | A `#N` is bound to at most one `uid` over the store's lifetime, including across imports and GC | N11 |
| I36′ | `claimed`, `settled` and lease state are never versioned and never exported | N2, N12 |
| I37′ | At merge, `parent` moves are applied and implied I5′ edges re-derived before any precedence edge is checked | N16 |

---

## 9. Score rationale

- **A 4.5.** Unchanged engine quality; under R1–R3 it offers no full branching, no readable image and no round-trip, and its trunk still carries four reachable wrong answers (X4 in its own walkthrough, X10, X5, the rebase-on-read hole) plus the `result_hash` idempotency gap.
- **B 5.5.** The best merge rigour before D and the only pre-update design with `init`, uid-hashed commits and a verified import — but it deliberately refuses R1 (coordination never branched), its `stale`/`check` path spawns `git`, its image is opaque frames, and it keeps X1, X4, X5, X11 and the question-plane hole.
- **C 4.0.** Strong coordination semantics and the best agent surface, undercut by two wrong answers on the v1 daily path (counter fields; refutation auto-flip), a liveness bug from its own numbers, a merge engine deferred with A's defect, and nothing toward R1–R3.
- **D 7.0.** Meets R1–R3 and fixes every shared defect of the other three in the spec rather than as options; loses three points for five new reachable wrong answers introduced by the branch mechanics and the image (N1–N5, of which N1 and N2 are on the daily path), one contradiction pair (N7, N8), a lost-counter path through git merges (N6), and a handful of seams between runtime and versioned state (N10–N16). Every one has a local fix; none requires abandoning the architecture.

*End of critique 21 (revised).*
