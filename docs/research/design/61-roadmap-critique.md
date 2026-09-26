# 61 — Adversarial review of the roadmap [60]

*moirai research/design, 2026-09-26. Status: research only; nothing is implemented. This file reviews `research/design/60-roadmap.md` [60] against the owner decisions of 2026-09-26, the design of record [AR], the adoption-driven draft [60d], the critiques [20] and [22], and the R4 design [40]. It edits no other file.*

**Owner decisions under test (binding).** (1) No SQLite or any other third-party embedded database anywhere: not as a backend, a stepping stone, a test oracle or a benchmark. The test oracle is a naive Rust reference model inside the project. (2) Early adoption is not a goal: no interim or throwaway stages, no temporary modes, no node caps, no reduced guarantees taken to ship sooner, no milestone ordered by time to adoption. Every component is built to its final specification and passes its full gates before anything builds on it. Standing preferences: maximum performance, minimal RAM, zero idle CPU, Windows 11 first, Opus for every agent.

**Sources.** [60] `research/design/60-roadmap.md` (the plan under review); [AR] `docs/ARCHITECTURE-RESEARCH.md`; [60d] `research/design/60-engine-first-roadmap-draft1-adoption-driven.md`; [20] `research/design/20-critique-perf-ram-windows.md`; [22] `research/design/22-critique-agent-fit-buildability.md`; [40] `research/design/40-file-links-design.md`; [05], [08], [15], [16] as cited by [60].

**Method.**
1. [60] was read in full (1,353 lines). [40] §0, §2, §5, §7, §8 and §9 were read, as were [60d] §2.3–§2.14 and §3–§4, [22] §2.7–§2.9 and §7, the graft list of [20] §5, and [AR] §4, §11 and §12.
2. **The edit list was applied mechanically.** A script parsed every block of [60 §9.1–§9.2] (120 edits, 134 operations) and applied them in order to a scratch copy of [AR]. Every anchor occurs exactly once, both in the original and at the moment it is applied. The result has 1,584 lines; [60 §9.3] says 1,583, a trailing-newline difference. The result was then searched for `SQLite`, the other database names, `oracle`, `S0`–`S6`, `v1.1`, `later`/`deferred`, `adopt*`, `M6`, `fast-import` and `node cap`.
3. [60 §2.5], the format-freeze table, was compared row by row with [40 §2.11], the R4 reservations.

Tags: **[M]** measured and quoted from the cited report; **[I]** inference; **est.** arithmetic with the inputs shown.

---

## 0. Verdict

**Revise before adopting [60] as the roadmap of record.** There are 2 blockers, 9 major issues and 10 minor ones.

[60] complies with both owner decisions to the letter:
- **No third-party database remains anywhere.** This was checked in [60] and in [AR] after its 134 edits. The remaining `SQLite` hits are design citations and the exclusion list.
- **No forbidden stage remains.** There is no serialized stage, no node cap, no swappable backend, no second git object backend and no "v1.1" deferral, and no milestone is ordered by time to adoption.
- **The edit list is mechanically sound.**

It does not yet meet "done properly" on three fronts.
1. **It is stale against [40], which already exists.** Its format-freeze table and its R4 milestone contradict the R4 design in hashed format content. Built as written, M8 would have to modify M1, M2, M3, M5 and M7 after they are certified (B1).
2. **No gate ever tests an OS crash or a power loss**, and the simulator's fault model is not specified. OS crashes are a loss mode the development machine has actually experienced. Two concrete paths to losing acknowledged commits are therefore untested (B2).
3. **Several dependency edges point the wrong way.**
   - Evidence that shapes the format arrives after the freeze.
   - Owner decisions that shape the format are scheduled after it.
   - M1 cannot be final while M2, M5 and R4 produce the contents of its segment sections.

The estimates are also materially low: about +75–100 units (est.), mostly R4.

None of the fixes below re-introduces an interim stage. Most of them move work earlier, or turn an extension point that is implicit into one that is explicit and certified.

| # | Severity | Issue (short) |
|---|---|---|
| B1 | blocker | The R4 reservations, placement, size and exit criteria contradict the existing design [40]; the canonical-form item 11 would be frozen wrong |
| B2 | blocker | No OS-crash or power-loss gate; the `Vfs` fault model is unspecified; adopting after a failed flush and an unflushed `HEAD` against GC can lose acknowledged commits |
| M-1 | major | M1 is not final: section contents, rollup, `TOPO`, FTS tier 2, `STATS` and R4 sections come from later milestones |
| M-2 | major | Evidence that shapes the format arrives after the M0 freeze (GT13, R4 replay corpora, the T1 trigger, image gate 0) |
| M-3 | major | Owner decisions #1, #5, #8 and #17 and the R4 calls are due after the milestones they shape |
| M-4 | major | Oracle independence and strength: a shared parser and binder in M5, commit ids undefined in the model, agent-written "human" fixtures, no mutation testing |
| M-5 | major | Code gated by thresholds far above the model's scale (FTS tier 2 at about 20k nodes, promotion, folds, retirement) is never compared with the model |
| M-6 | major | The measurement protocol is undefined: the "synthetic 16-agent load", n for p99, pairing with floors, "0 % CPU" |
| M-7 | major | Test infrastructure and machine time are not provisioned: no repository, no CI, no hosted Windows 11 x64 runner, everything on one laptop |
| M-8 | major | Estimates are low by about 75–100 units; the rate is borrowed from an estimate, not measured |
| M-9 | major | Format evolution after the release is untested; the "hardened" gate has no upgrade drill |
| m-1 … m-10 | minor | 7 edit-list misses plus wording fixes; the `across ≤ 8` cap; the virtual-base wording; the M0 "reference codec"; [05 §17]'s SQLite baseline; GT4 acknowledgement recording; the `ProjectFs` seam; the M3 driver; open vs tail; the `shared` reservation |

---

## 1. What holds and should be kept

- **Decision compliance** (§2 below).
- **The physical half of branching sits inside the storage milestone** (refs, pins, `ClientHead`, the per-ref index, view construction, promotion). The protocol is certified once, with the branching surface inside the simulation and the kill loops. This is the right correction of [60d]'s E4/E5 split.
- **The in-process git object layer is its own leaf component (M4)**, replacing the "fast-import now, hand-written later" plan. That plan was a throwaway stage by construction [60 §2.4].
- **The recursive virtual merge base is in the release, with a correct counterexample** for the newest-LCA rule. Two LCAs disagree on a key, the two sides resolve it differently, and the rule silently takes one side [60 §3.4]. This was verified by hand.
- **The reference model's shape** [60 §4]: definitional algorithms, rule tables as data, a separate author, a coverage check by section tags, and crash semantics as "applied or not applied".
- **Floor-relative gates** [60 §5.3]; the release gate RG1–RG12 as a *definition* of "complete and hardened" [60 §6].
- **Harness validation by seeded bugs at M0** [60 §3.1]. Keep it, and extend it (M-4).
- **The edit list's discipline:** anchors unique, explicit keep-lists and application by script. All of this was verified; see §6.6.

## 2. Owner-decision compliance

| Check | Result | Evidence |
|---|---|---|
| SQLite or another third-party database as backend, stepping stone, oracle or benchmark | **none** | [60 §1.1, §5.5, §5.6]; [AR] after the edits: `SQLite` remains only in design citations (L30, L57, L599, L810, L1434, L1476), the exclusion list (L178) and the new texts that forbid it (L53, L212, L1429, L1445, L1502, L1530, L1581); redb/heed only as optional out-of-tree points (L178, L1429) |
| Interim, throwaway or temporary modes | **none found** | no serialized stage, no engine trait, no `ImageBackend` trait, no second git backend; the layout probes and the storage driver are declared permanent test infrastructure |
| Node caps | **none**; [AR] never had one (grep: 0 hits) | — |
| Reduced guarantees to ship sooner | **none found.** Two caps need wording: `across ≤ 8 refs` (m-2) and the as-of 16k-op CLI budget. Both are resource budgets from [16], not schedule cuts, but `across ≤ 8` contradicts M3's all-ref `--across` | [60 §3.6] vs [60 §3.4] |
| Adoption-driven ordering | **none.** The order is argued from dependencies (§2.4); adoption happens once, at RG1–RG12 | — |
| Remaining deferrals in [AR] after the edits | **3 residues** that still read as deferrals (m-1): `PostToolBatch … later` (L167), `--with-oplog`, deferred (L947), `op restore` later (L1484) | §6.6 |

---

## 3. Blockers

### B1 — The R4 content contradicts the R4 design that already exists

**Problem.** The source table of [60] (L25) says [40] is "not yet available", and every R4 item is a placeholder derived from [09]–[13]. But [40] exists (written at 04:36; [60] at 04:44), and it contradicts [60] in five places.

1. **Hashed canonical form, item 11.**
   - [60 §2.5] and E79 freeze item 11 as a generic *annotations* block carried by the trailers `Moirai-Relink`, `Moirai-Relink-Evidence` and `Moirai-Move-Prefix`.
   - [40 R-5, §5.7] freezes item 11 as the sorted `PathPrefix` moves, carried by one trailer, `Moirai-Path-Prefix`, and says "No other new trailer is needed", because relink provenance is the node's `relink` field.
   - Commit ids depend on item 11, so whichever text is frozen, the other design is a format defect. If E79 is applied as written, [AR] L666 will state the wrong item 11.
2. **Value types.**
   - [60]: `path` plus `oid`, which holds git *blob* ids only.
   - [40 R-1]: `path` plus `digest {algo u8, 20 | 32 B}`, which holds content ids *and* `observed_git` commit ids.
3. **Reservations missing from [60 §2.5].** [60]'s `FsIntentDone`/`FsIntentAborted` do not exist in [40]; its `FsIntent` record carries its own state.

   | [40] reservation | What it adds |
   |---|---|
   | R-2 | the `artifact` field set, the `planned`/`removed` statuses, the composite `observation` merge class |
   | R-3 | the `uid_derivation` schema column. Derived uids change I1 and the import rule `IdCollision` |
   | R-4 | a 128-bit discriminator in the edge key (it changes the edge key in the CSR and in canonical item 10), the anchor record layout, and the op `SetEdgeProps` |
   | R-5 | the op `PathPrefix` |
   | R-6 | `HEAD.next_anchor` |
   | R-7 | the record kinds `FPrint`, `UsnCursor`, `TreeReg` |
   | R-8 | the sections `ALIASIDX`, `ANCHORS`, `ANCHOR_UID`, `FILEOBS`, `PENDING`, `FSINTENT`, `FPRINT`, `USNCUR`, `TREES` |
   | R-9 | the "fingerprint" blob class |
   | R-10 | the anchor key class in canonical item 10 |
   | R-13 | the config keys |
   | R-14 | the resolver-version constant table |

4. **Placement.**
   - [40 §8.1] attaches the subsystem layer by layer to its host components:
     - FL-3 goes to the graph core: derived uids and `#N` reuse, `at` edges with the discriminator, the path, alias and anchor indexes, the `suspect` extension, and I-F1–I-F3, I-F8, I-F9.
     - FL-7 goes to version control: the merge of the observation composite, `PathClaim`, `StatusFork` resolved by observation, `PathPrefix` union and composition, and anchor add-wins.
     - FL-8 goes to the image: anchor lines, derived uids on import, the `IdCollision` exemption.
     - FL-6 goes to the query language.
   - [60] puts all of R4 into M8, after M7. It states that "Nothing in C7 depends on C8" (§2.3, L188), and its M3 carries only "prefix composition when a commit carries a `Moirai-Move-Prefix` annotation".
   - Built that way, M8 must change the op applier, derived state and invariants of M2, the merge engine of M3, the import rules of M7, and the fold and seal path of M1 — all certified milestones. P3 and P4 forbid exactly that.
   - [40]'s FL-1 histogram diff is shared with M3's diff3, an edge [60] does not have. [40]'s FL-2 `ProjectFs` seam, which has its own simulator, is missing from P1's list of seams.
5. **Size, gates and defaults.**
   - **Size.** [40] is 21–29k lines of product code plus 10–14k of tests. At [60]'s own ratio (≈ 3 units per 1k lines including tests, §7.1) that is **63–87 units**, against [60]'s 20–31 (§3.9).
   - **Exit criteria.** [60] asks for quote anchors "≥ 90 %"; [40 §8.3.4] asks for ≥ 96 % on the 1,180-citation sample and ≥ 99 % agreement of the window tie-break.
   - **Model duties.** [60 §4.2] says anchor quality is checked "not by the model"; [40] P11 says every anchor state agrees with the model's brute-force search.
   - **Owner-call defaults.** For the USN journal, [60 §3.14] says "walk-based"; [40 §9.2 #5] says "use a journal wherever one exists".
   - **Import tooling.** [40]'s `links import` of 34,195 citations (decision 12) is absent from M9.

**Fix.**
- **Re-issue [60] against [40]:**
  - replace the R4 rows of §2.5 with [40 §2.11] R-1…R-14 verbatim;
  - replace E79 with [40]'s text: item 11 = `PathPrefix` moves, the `Moirai-Path-Prefix` trailer, the `digest` type, the discriminator, the sections, the record kinds and `next_anchor`;
  - dissolve M8 into [40]'s layers attached to their hosts (§7 gives a dependency-true order);
  - size R4 from [40];
  - adopt [40]'s exit criteria, model duties and defaults;
  - add `ProjectFs` to P1 and the FL-1 → M3 edge;
  - move the R4 owner decisions to before M2 (M-3);
  - add `links import` to M9.
- **Record the rule** that [60] carries no placeholder that contradicts [40] or [50] once they exist.

### B2 — No gate tests an OS crash or a power loss, and the fault model is unspecified

**Problem.** RG3 promises "zero lost acknowledged commits", but no gate exercises the loss mode this host actually produces.
- **GT4 kills processes with `TerminateProcess`.** The OS page cache survives a process kill, so GT4 can never observe unflushed data being lost. [20 §2] describes the F-A1 class with exactly this scenario: an OS crash follows within the write-back window and the dirty page is lost. The development machine has also experienced disk-full events [AR §10 row 13].
- **GT1 and GT3 are underspecified.** They inject "a crash at every write/flush/publish boundary" into an in-memory `Vfs`, but [60] never says what survives a crash:
  - whether unflushed writes are dropped (all of them, any subset, or torn at sector granularity);
  - whether `DATA_SYNC_ONLY` persists size, create, rename and delete metadata;
  - what state a failed flush leaves.

  A simulator that keeps unflushed bytes, which is the easy default, proves nothing about power loss.

**Two candidate defects in [AR]'s protocol that only a specified fault model can find [I]:**
1. **Adopting after a failed flush.** [AR §4.5]: step 7 aborts the process on a flush failure, and step 2 has the next writer adopt every complete record, "re-flushed once". After a failed flush, the cached pages may be clean but not durable. ATC'20 [08 §8.1] reports this; NTFS behaviour is undocumented. A later re-flush then succeeds and proves nothing. The sequence:
   1. Writer Q adopts commit C.
   2. Later writers append C+1…C+n and acknowledge them.
   3. A bugcheck leaves zeros at C's offset (zero-filled extent, G11).
   4. Recovery "stops at the first bad record" ([AR §4.3, §4.10]) and silently discards C+1…C+n, all of which were acknowledged.
2. **An unflushed `HEAD` against destructive maintenance.** `HEAD` is never flushed [AR §4.2]. Segment files are deleted "after `HEAD` has pointed elsewhere for 60 s" [AR §4.1], and log extents are retired or recycled (G25). After a bugcheck, the durable `HEAD` can be older than the one GC relied on. It can then name deleted segments, or a `committed_lsn` inside a retired or zero-filled extent. Whether this happens depends on how NTFS orders a data page against a journaled delete. That ordering must be enumerated, not assumed.

**Also absent from every gate:**
- disk-full injection in GT1 and GT3 (there is only the one-off drill in RG7);
- **process suspension**: a holder stopped by a debugger, or by a console QuickEdit selection, while it holds byte 0 or byte 2; a long-lived reader stopped across the 60 s GC grace;
- **wall-clock steps**, which affect lease TTLs, the HLC and the GC grace;
- **torn concurrent reads**: a lock-free reader that `pread`s a `HEAD` slot while it is being written.

**Fix.**
- **M0: specify the `Vfs` fault model inside the format specification,** as the persistence properties of ALICE did:
  - unflushed data is lost in any subset of 4 KiB sectors, and a sector may be torn at 512 B;
  - state which metadata operations are durable after `DATA_SYNC_ONLY`, after a full flush, and in journal order;
  - a failed flush leaves its range indeterminate forever: a later successful flush proves nothing;
  - reads concurrent with writes may return mixed sectors;
  - `ERROR_DISK_FULL` can occur on any allocating write;
  - a process can pause for any length of time;
  - the wall clock can step backward or forward.

  The seeded-bug validation of M0 must include both scenarios above.
- **GT1:** at every crash point, enumerate bounded subsets of unflushed writes (per-file prefixes plus one torn sector, bounded as in CrashMonkey), plus sequences of "flush error, then more commits, then crash". Assert that no acknowledged commit is lost after any later crash.
- **Protocol questions for the M0 specification review** (examples, not decisions):
  - adoption *re-writes* the adopted bytes before flushing, or verifies them through an unbuffered read, instead of only re-flushing;
  - recovery that finds valid records of the same epoch beyond a bad record stops and diagnoses `repair` instead of silently truncating;
  - every deletion, retirement or recycling is preceded by `FlushFileBuffers(HEAD)` naming a state that no longer needs the file. This is off the commit path, so its cost does not matter.
- **GT3 and GT4 gain three variants:**
  - suspend and resume random processes for 1–120 s (`NtSuspendProcess`), including the byte-0 holder, the byte-2 holder in the middle of a checkpoint, and readers across the GC grace;
  - disk-full on a small VHDX (diskpart can create one on Windows 11 Home);
  - wall-clock steps of ±1 h.
- **New gate GT15, an OS-crash loop,** mandatory from M1 and part of RG3:
  - **Setup.** Run a Windows 11 guest in VirtualBox or VMware Workstation; the host reports Windows 11 Home, which has no Hyper-V. Turn the host I/O cache off and make the hypervisor honour guest flushes (VirtualBox: `IgnoreFlush 0`).
  - **Workload.** The guest runs the GT4 workload. Every writer streams each acknowledged commit id to the host over a socket.
  - **Crash.** At random times the host triggers a bugcheck in the guest (Sysinternals NotMyFault) or a hard power-off.
  - **Check.** After reboot, every commit id the host recorded must be present, and `doctor --verify` must be clean.
  - **Volume.** ≥ 1,000 cycles at M1 exit, nightly afterwards, and ≥ 5,000 cumulative for RG3.
- **State the remaining assumption in RG3.** Loss of the drive's own cache (a consumer NVMe without power-loss protection) is covered only by the simulator, and the design assumes the drive honours FLUSH.

---

## 4. Major issues

### M-1 — M1 cannot be "final" while later milestones produce its section contents

**Problem.** M1 promises the following [60 §3.2]:
- "base and tiered delta segments with every §2.5 section";
- "`repair --rebuild-from-log` reproduces byte-identical segments";
- "rollup ≤ 0.3 s at 1e5";
- promotion and checkpoint budgets.

But the contents of several sections come from later semantics:
- the `BM_*` sets (`ready`, `suspect`, `is_blocker`, …);
- `TOPO`, a Kahn pass over the *precedence* edge kinds, which is a schema notion;
- `TERMS`/`POST` (tokenisation; FTS tier 2 is "merged at rollup");
- `STATS` (for the planner of R5);
- R4's `FILEOBS`, `PENDING`, `FSINTENT`, `FPRINT` and the others.

These come from M2, M5 and R4. "None of these is stubbed in product code" [60 §3.2] is therefore impossible to satisfy. Either those sections are empty at M1 exit, which is a temporary state, or the byte-identical rebuild and the rollup and checkpoint budgets are certified on a workload that is not the final one. Either way, M2, M5 and R4 later change M1's seal, rollup and rebuild paths. The same is true of GT2 at storage level: the model's "storage view" cannot check derived sections.

**Fix.**
- M1 defines a permanent **section-producer and record-fold registry**. Each entry names a tag, a layout class, a fold function over op windows and a rebuild function from the log.
- M1 certifies the registry with **every section and record kind of the frozen format exercised** by the storage driver through generic physical producers: ± lists, sorted tables, and bitsets unioned from ± lists.
- M2, the query milestone and R4 register producers without touching M1 code.
- P1 lists the registry as a seam.
- For each later milestone, state as its own exit criteria which M1 gates and budgets re-certify with the real producers: the byte-identical rebuild, the checkpoint, rollup and promotion budgets, and GT1/GT3/GT4 with the real sections.

### M-2 — Evidence that shapes the format arrives after the M0 freeze

**Problem.** Four pieces of evidence that can change frozen bytes come after the freeze.
- **(a) The R5 accuracy experiment.** GT13 runs at M5, and if it fails "[50] is revised first" [60 §3.6]. But the R5 reservations froze at M0: the named-query item kind, `STATS`, the `ready`/`dispatchable` split, cursors pinned to commits, idempotency over canonical ASTs. A revised surface can need different ones, and M0 re-opens after M1–M4 exist.
- **(b) R4's anchor layout.** [40] freezes the ±16-line window of u16 hashes, `occurrence`, `span_hash` and the resolver constants (R-14) at M0. The evidence that validates them, the replay corpora of [40 §8.3.4], runs in M8.
- **(c) The T1 trigger.** [AR §2.1]'s T1 revisit trigger ("M1 measurement … point read > 50 µs after a full tail, or a delta checkpoint > 100 ms at 0.5 M nodes → Option B") can replace the structure of the materialised state after M1. M0's probes (item 14) do not measure those two quantities.
- **(d) Image gate 0.** Gate 0 (every hashed canonical item has a carrier in the image) and image determinism, the second long pole of [22 §7.1], run only in M7, after M5 and M6 are built on the canonical form. The image needs nothing from M5 or M6: the C6 → C7 edge is packaging only, the image verbs.

**Fix.**
- **(a)** Run GT13 **in M0**, on the reference model with its own naive, test-only query parser (which M-4 needs anyway), and freeze the surface together with the R5 reservations.
- **(b)** Build [40]'s FL-1 libraries in a second M0 lane (they are pure and have no engine dependency) and run the replay corpora before the freeze.
- **(c)** Add the T1 and T2 trigger quantities to M0 item 14, and re-word the T1 trigger as an M0 decision (edit E127, §6.6).
- **(d)** Add to M0 a **gate-0 carrier table** (canonical item → tree path or trailer) with a fixture for every commit kind. Move the image core directly after M3 and M4, ahead of the query language; the image CLI verbs land with the CLI (§7).

### M-3 — Owner decisions are due after the milestones they shape

[60 §3.14] schedules several decisions too late. P2's own rule is that a decision is due before the first milestone whose format, protocol or code it can change.

| Decision | [60] due | What it can change | Due |
|---|---|---|---|
| #1 `shared` field class | before M3 | view construction (status and `blocks` read from `main` on every branch, an M1 mechanism) and the schema's field-class enum (M0) | **before M0** |
| #5 branch per lane or per worktree | before M3 | `ClientHead`/`HEADS` semantics and pin volume (~44 pinned sets), both M1 | **before M0** |
| #8 prose in the repository | before M2 | "store + tracked Markdown" makes bodies file-backed: body storage (M0/M1) and R4 file nodes | **before M0** |
| #17 one store per repository or a global store | before M6 | store layout and pointer files (frozen at M0 per §2.5), the `#N` space, the scope of the writer lock, a project column | **before M0** |
| [40 §9.2] R4 calls (policy, deletion inference, unbound trees, accelerators) | before M8 | FL-3 and FL-7 in M2 and M3 (B1) | **before M2** |

**Fix.** Move these as shown and keep the recommended defaults. Add the rule to P2 in so many words.

### M-4 — Oracle independence and oracle strength

**Problem.**
1. **Shared parser and binder.** M5 builds "the front end (parser, binder, diagnostics) and the reference model's nested-loop evaluator" first [60 §3.6]. The model evidently evaluates the product's parse and bind output. That is shared code exactly where R5 bugs will live — name resolution, view scoping, absent-value semantics — and it contradicts §4.5 "no shared code".
2. **Commit ids.** GT2 compares result data, and every read names `commit`. LCA order and virtual-base order break ties "by commit id". Yet the model excludes bytes (§4.3), so it can neither compute commit ids nor break those ties identically, unless it shares the engine's canonical encoder, which is the most critical encoding in the system. [60] states neither choice.
3. **Correlated authors.** GT10 expected outputs may be written by "a person or an agent" (L450); [60d] required a person. The model's author and the engine's author are both Opus 5.5 instances. Correlated misreadings of ambiguous text are the main residual risk, and nothing measures them.
4. **One harness validation.** The harness is validated with seeded bugs once, on a toy log, at M0. Nothing shows that GT2, GT3 or GT4 detect a defect in the *real* engine.

**Fix.**
1. The model gets its own naive query parser and binder (≈ 1–1.5k lines, test-only). The generator emits ASTs; the model evaluates the AST; a printer feeds text to the product. `parse(print(ast)) == ast` becomes a property.
2. The model implements the canonical form independently (≈ 300 lines), and GT2 compares commit ids byte for byte from M1 on. This is an independent check of I28′ and I38′.
3. At every exit the owner reviews and signs the model's rule tables: the merge table, the status machines, the delete-policy matrix, the link merge rules and the pack classes. They are data, so this takes hours, not days. GT10 expected outputs for the register incidents, the node-40 table and the §7.6 walk-through are owner-verified. The owner's hours are budgeted (M-8).
4. **Mutation testing at every exit.** Mutants are applied in CI only, never compiled into a product build. GT2 and GT6 must kill a stated share (for example ≥ 90 %) of the compiling mutants in the semantic crates. At M1 exit, protocol bugs seeded into the real engine must each be caught by GT1, GT3 or GT4: skip the flush, publish before the flush, adopt without CAS, delete before the `HEAD` barrier.

### M-5 — Threshold-gated code is never compared with the model

**Problem.** Model cases are capped at ≤ 1e4 commands and ≤ 2e3 nodes (§4.4 item 8); the 1e5–1e6 scales are covered only by `doctor --verify`, which compares the engine with itself, and by budgets. Many code paths switch on thresholds above that scale, so they are never differentially tested:
- FTS tier 2 above ~20k nodes: tier-2 results are never compared with the model;
- promotion by size or age;
- the tiered fold of d1..d3;
- `hist` retirement at a 64 MiB extent;
- checkpoints at 4,096 ops / 4 MiB, and bodies at 32 MiB;
- the Kahn fallback above 1,000 touched precedence edges;
- the 10k-op budget of the `suspect` closure;
- the loose/pack threshold.

A self-check cannot find a misreading of the specification.

**Fix.**
- Every threshold becomes a store parameter, recorded in `config` or `HEAD`, visible in the format and not a compile-time flag. The extent size becomes a format parameter, so a store with small extents is a real store.
- A test profile sets the thresholds tiny: tier 2 at 16 nodes, a checkpoint at 8 ops, 64 KiB extents, promotion at 16 ops, Kahn at 4 edges.
- GT2 and GT3 sweep thresholds, including the production values.

### M-6 — The measurement protocol is undefined

**Problem.**
- Every time and RAM gate is measured "idle and under a synthetic 16-agent load" (P6), but that load is never defined.
- The p99 gates state no n. M0 item 1 uses n = 200, where the p99 is the second-largest sample.
- The floors are measured once, at M0. §5.3 claims that "a slow disk day cannot hide an engine regression", which is true only if the floor is re-measured in the same session, interleaved with the gated measurement.
- "0 % CPU over 10 minutes" is not defined as a measurement.
- The point at which private bytes are read (peak or at exit) is not defined.

**Fix.** Define and freeze a measurement protocol at M0:
- **Load.** A load fixture: a recorded CPU, disk and memory profile of a real 16-agent campaign, replayed by a load generator, with free RAM held at ≈ 1.8 GB [M, 05 §2].
- **Sample size.** n ≥ 1,000 for every p99, and ≥ 10,000 for operations under a millisecond. Run 5 repetitions and gate on the median p99.
- **Floors.** Re-measure floors interleaved with the gated measurement in the same run.
- **Idle CPU.** Zero CPU-time delta (`GetProcessTimes`) and zero context switches (ETW) over 10 minutes.
- **RSS.** Peak private bytes from `GetProcessMemoryInfo` at exit.
- **CI.** A stated noise band for CI regression detection.

### M-7 — Test infrastructure and machine time are not provisioned

**Problem.**
- **No repository and no CI.** The moirai project directory is not a git repository and has no CI. [60 §3.13, §5.1] run every mandatory gate in CI "on a Windows 11 runner with Defender on". To my knowledge, hosted x64 CI images are Windows Server; the only hosted Windows 11 image is ARM64. The only Windows 11 x64 machine the plan names is the owner's laptop.
- **Machine time** (est.):
  - RG5 alone is about 16 fuzzers × 7 CPU-days ≈ 112 CPU-days;
  - GT4 runs nightly in four variants, ≈ 4 h of the whole machine;
  - GT3 runs ≥ 1e7 steps a night;
  - a 24 h fuzzer run at each exit;
  - the 72 h soak and GT15;
  - exit campaigns at four scales, idle and loaded.

  All of it would run on a 16 GB laptop with ≈ 1.8 GB free under agent load, shared with the owner's benchmark windows and workflow.

**Fix.** Put an infrastructure deliverable into M0:
- the repository and CI;
- a **dedicated Windows 11 x64 test host** (same build, NVMe, Defender on, mini-PC class) for the nightly GT3/GT4/GT15 runs, fuzzing and the soak;
- hosted Windows Server runners for short seeds at PR level, as a secondary signal only;
- the owner's laptop only for exit measurements, in agreed windows;
- a compute budget per milestone, in CPU-hours.

### M-8 — The estimates are low, and the rate is not measured

| Item | [60] | Recomputed (est.) | Basis |
|---|---|---|---|
| R4 (M8) | 20–31 | 63–87 | [40 §8.1]: 21–29k lines + 10–14k of tests at [60]'s ≈ 3 units per 1k |
| Query language (M5) | 23–39 | ≈ 33–49 | the scope [60] lists (executor, subscriptions, `tx`, standard library) is the 10–15k-line figure of [15 §14.3], i.e. 30–45 build units, not 20–35 |
| Reference model | 7–9 (its own parts sum to 7.5–9.5) | ≈ 18–20 | ≤ 5.5k lines at the same ratio ≈ 16, plus the independent parser and canonical form of M-4 |
| M4 | 11–15 | ≈ 15–21 | SHA-256, split commit-graph, reftable read and bundles on top of [22]'s 9-unit base; 4–6k lines per [AR T10] |
| M0 | 10–14 | ≈ 16–22 | + [40]'s and [50]'s reservations, the fault model, the gate-0 table, the trigger probes, infrastructure |
| M11 | 6–10, 2-week floor | 10–18, 4–6-week floor | any engine fix after the review must reset the 14-night count (RG3 must hold on the release-candidate commit); review (1–2 weeks) + fixes + 14 nights + 72 h |
| GT15 and fault-model simulator | — | 3–5 | B2 |

Net: about **+75–100 units (est.)**, i.e. roughly 270–360 units. On one lane at [60]'s own rate that is ≈ 36–75 weeks.

**The rate itself is not measured.** "5–8 units per week" is the previous plan's estimate ([22 §7.3]: "S ≈ a week, M ≈ 2–3 weeks"). The early slices of that plan were SQLite-backed, which is easier than engine work. The two-lane figures also assume that the owner's supervision doubles.

**Fix.**
- Recompute §7 from [40] and from the scope listed.
- Publish P50 and P90.
- Measure velocity (units delivered per week against the estimate) at the M0 and M1 exits, and re-issue the calendar then.
- Budget the owner's hours: about 25 decisions, the specification reviews, the rule-table sign-offs of M-4, the GT10 fixtures, and the M9/RG8 judgements.

### M-9 — Format evolution after the release is untested

**Problem.** Readers refuse a newer format version, nothing migrates automatically, `migrate` covers only schema strengthening, and "pre-release stores are regenerated, never migrated" (P4, [AR §12]). After the release, the owner's store holds years of live data. RG10 asks only that "upgrade and rollback" are *documented* and that the version refusal is tested. The first format fix after the release would therefore run an untested procedure on live data.

**Fix.** Add an **upgrade drill** to RG10, using a synthetic format v2:
- **(i) A derived-file layout change** (a new section), applied by `repair --rebuild-from-log` to a copy of a v1 store.
- **(ii) A log or canonical-form change**, applied by `image export --with-oplog` → `image import` into a v2 store. Commit ids are preserved where the canonical form is unchanged; otherwise the id map is documented.
- **Verification.** Both paths are checked state-identical against the model.
- **Rollback.** Rollback means restoring the pre-upgrade backup, and it is drilled too.

---

## 5. Minor issues

- **m-1 — Edit-list misses and wording.**
  - Seven anchors, each unique in [AR] and unchanged by E1–E120, are listed in §6.6: the `PostToolBatch … later` residue, the `--with-oplog`, deferred residue, "`op restore` later", two sentences saying "`shared` class after one campaign", the "bring the leader forward" trigger and the T1 trigger's "M1 measurement".
  - The heading of §2.13 still reads "T13 — Scope of v1".
  - §9.5 says "the release gate (RG1) requires [50]'s edit list to be applied before M0 exits". A release gate cannot gate M0; make it an M0 entry criterion.
  - E79 must be replaced from [40] (B1).
- **m-2 — `across ≤ 8 refs`** in the M5 views [60 §3.6], from [16], against `--across` over all refs through `TOUCH` bitmaps in M3. The CLI verb becomes a named query, so one of the two must change. Either state it as a budget with a resumable cursor, or drop it for promoted refs.
- **m-3 — Recursive-virtual-base wording.** "A key that holds a conflict value in the virtual base conflicts on every side that differs from it" would turn "both sides resolved to the same value" into a spurious conflict. State that the both-sides-equal case is clean, and add it to the I31′ properties.
- **m-4 — The M0 "reference codec"** (golden fixtures "re-encode byte-identically with the reference codec", L250) sits beside "no engine or product code" at M0. Declare it one of two things:
  - either the permanent pure `moirai-format` crate, built to its final specification at M0 and checked against fixtures hand-written in hex;
  - or a test-only independent decoder, kept as a permanent differential oracle for the format.

  It must not quietly become a second product codec.
- **m-5 — [05 §17] item 5** still lists "redb 4.x, heed, and SQLite (rusqlite, WAL)" as baselines, and [60]'s source table points to [05 §17] as the measurement plan. Add a line to §5.6 saying that item is superseded.
- **m-6 — GT4 and RG3 mechanics.**
  - State how acknowledgements are recorded outside the store (each process reports every acknowledgement to the harness over a pipe).
  - State the store scale of GT4 and the model's sampling rate for "re-evaluate each read at that seq".
  - State that RG3's "last 14 nights clean" holds on the release-candidate commit, and that any change to engine code resets the count.
- **m-7 — Missing seams and edges.** `ProjectFs` ([40] FL-2) is missing from P1, and the FL-1 → M3 diff edge is missing (see B1).
- **m-8 — The M3 kill loop "in 16 directories bound to different branches"** runs before discovery and the CLI exist. Specify the M3 test driver (permanent test infrastructure) and the resolution-chain inputs it simulates: environment, lease, dispatch marker, binding, worktree hint.
- **m-9 — Open budget against [AR]'s own estimate.** M1 gates "open ≤ 3 ms at 1e6 with a full 4,096-op tail", while [AR §4.7] estimates 3–5 ms for a full 4 MiB tail. Reconcile the two through M0 item 10 before the checkpoint threshold is frozen.
- **m-10 — Choosing `shared` after the release.** If the `shared` class may ever be chosen after the release (E41 keeps "revisited after the release"), reserve its field-class value and view flag at M0, or state that choosing it is a format-v2 event (M-9).

---

## 6. Supporting audits

### 6.1 Dependency edges

| Edge in [60 §2.3] | Real? | Note |
|---|---|---|
| C0 → all | yes | — |
| C1 → C2 | yes, but not clean | the section contents flow C2 → C1 (M-1) |
| C1 → C3 | yes | view construction and promotion are physical; a semantics-free op application is enough to certify them (after M-1) |
| C2 → C3 | yes | — |
| C1 → C4 | weak | only the Windows `Vfs` primitives; C4 could start as soon as the `Vfs` is certified inside M1 |
| C2, C3 → C5 | yes | — |
| C4 → C5 | weak | only the ancestry built-ins |
| C5 → C6 | yes | verbs are named queries |
| C3, C4 → C7 | yes | — |
| C6 → C7 | **packaging only** | the image core needs nothing from C5/C6; the verbs can land with the CLI (M-2d) |
| C7 → C8 ("nothing in C7 depends on C8") | **wrong** | [40] FL-8 puts anchor lines, derived uids on import and the `IdCollision` exemption into the image (B1) |
| C8 → (C2, C3, C1, C5) | **missing** | FL-3 into the graph core, FL-7 into VCS, the R4 runtime sections into M1, FL-6 into the query language (B1, M-1) |
| FL-1 → C3 | **missing** | the histogram diff shared by diff3 and anchors |
| owner decisions → C0/C1 | **mis-dated** | #1, #5, #8, #17 (M-3) |

### 6.2 Reservations: [60 §2.5] against [40 §2.11]

| [40] | In [60 §2.5]? |
|---|---|
| R-1 `path`, `digest` | `path` yes; `digest` **renamed and narrowed** (`oid`, blob ids only) |
| R-2 artifact fields, `planned`/`removed`, `observation` merge class | **no** |
| R-3 `uid_derivation` | **no** |
| R-4 edge-key discriminator, anchor layout, `SetEdgeProps` | partly ("typed property blocks"); **no discriminator, no op** |
| R-5 `PathPrefix` op; item 11; `Moirai-Path-Prefix` | **contradicted** (item 11 = annotations; three other trailers) |
| R-6 `HEAD.next_anchor` | **no** |
| R-7 `FsIntent`, `FileObs`, `Pending`, `FPrint`, `UsnCursor`, `TreeReg` | partly; **three missing, two extra** |
| R-8 ten sections | `PATHIDX` only |
| R-9 fingerprint blob class | **no** |
| R-10 anchor key class in item 10 | **no** |
| R-11 `.moi` anchor lines, artifact fields | "file nodes and `at` edges with anchor properties" in the golden set; **no `anchor` line form** |
| R-12 I-F1…I-F11 | **no** |
| R-13 config keys | **no** |
| R-14 resolver constant table | **no** |

### 6.3 Windows hazards the owner named, against the gates

| Hazard | Gate in [60] | Gap | Fix |
|---|---|---|---|
| Lost acknowledged writes (process kill) | GT1, GT3, GT4, RG3 | none for process kills | — |
| Lost acknowledged writes (OS crash, power loss; OS crashes have occurred on the development machine) | none | **total** | fault model, GT1 subsets, GT15 (B2) |
| Torn `HEAD` on a crash | GT1 (implicit) | the fault model does not state torn sectors | B2 |
| Torn `HEAD` on a concurrent read | none | a reader `pread`s during the slot write | mixed-sector reads in GT3 (B2) |
| Stale durable `HEAD` against GC and retirement | none | [AR] never flushes `HEAD` | barrier plus GT1 (B2) |
| Delayed lock release | M0 item 12; injection in GT3; GT4 | none | — |
| A holder paused (debugger, QuickEdit) | none | liveness, GC grace | suspend injection (B2) |
| Defender or another process holding files | the AV-interference test at M1; Defender on | none material | — |
| fsync (flush) errors | "fsync-error injection" in GT1/GT3 | the state after a failure is unspecified; adopt-by-re-flush | B2 |
| Disk full (has occurred on the development machine) | RG7 drill only | not injected | GT1/GT3 injection; a VHDX variant of GT4 (B2) |
| Clock steps | none (the deterministic clock is for reproducibility) | leases, HLC, GC grace | clock-step injection (B2) |

### 6.4 The reference model

- **Trustworthy?** Its algorithmic independence is good (§4.2). Its *specification* independence is weak: the same document, the same model family, "agent" fixtures, and a parser and binder shared with the product in M5. Commit ids are undefined in its scope. Fixes: M-4.
- **Small?** Its budget, ≤ 5.5k lines, is plausible for its current scope. [40]'s brute-force resolver, the independent parser and the canonical form take it to ≈ 7k. That is still small for an executable specification, but it is ≈ 18–20 units, not 7–9 (M-8).
- **Covers the scale that matters?** No, for threshold-gated paths (M-5).

### 6.5 Estimates

See M-8. The range most likely to move after [50] exists is the query language; R4's is now known from [40].

### 6.6 The edit list: mechanical check and misses

**Mechanical check.** All 134 operations apply, every anchor is unique, and the order is respected. After application:
- `v1\.1` remains only in the historical Review-log rows (CM7, CL2, CL7) and in the new anti-requirement in §12, as [60 §9.3] claims;
- `M6` refers only to the CLI;
- `fast-import`, `ImageBackend` and `throw-away` appear only in rejected or superseded positions;
- no database is used anywhere.

**Proposed additions.** Every anchor below was checked to occur exactly once in [AR] and to be untouched by E1–E120. The line numbers refer to the current [AR].

```text
E121 · §2.9 T9 Decision · L167
- optional `mcp_tool` PostToolBatch delta later.
+ optional `mcp_tool` PostToolBatch delta (in the release, off by default).

E122 · §5b.4 side refs · L947
- (`--with-oplog`, deferred)
+ (`--with-oplog`, in the release)

E123 · §11 row 5 · L1484
- `rebase`, `op restore` later
+ `op log`/`op restore` also in the release; `rebase --onto` not built ([60 §1.3])

E124 · §0 risks (2) · L56
- and an explicit fallback (`shared` field class) after one campaign
+ and the `shared` field class as an owner decision taken before M0, because it changes view construction and the schema's field classes

E125 · §10 row 2 · L1459
- the `shared` field class as an explicit fallback after one campaign
+ the `shared` field class, decided by the owner before M0

E126 · §2.2 T2 Revisit trigger · L103
- (then bring the leader forward and make the CLI forward to it)
+ (decided before M1; after M1 a change re-opens M1 and re-runs its gates)

E127 · §2.1 T1 Revisit trigger · L93
- **Revisit trigger.** M1 measurement at the owner
+ **Revisit trigger.** M0 layout-probe measurement (item 14, confirmed at M1 exit) at the owner

E128 · §2.13 heading · L205
¶ ### 2.13 T13 — Scope of v1
+ ### 2.13 T13 — Build order and scope
```

E124/E125 assume the timing of M-3; E79 is replaced as B1 describes.

---

## 7. A dependency-true order

This is one order that satisfies B1, M-1 and M-2 without any interim stage. The one hard requirement, whatever the numbering, is that **no milestone modifies a certified one except through an extension point certified in the earlier milestone** (the M-1 registry, the named-query registry, the built-in registry).

| New | Content | Depends on | Was |
|---|---|---|---|
| M0 | contract and evidence: format v1 with [40] R-1…R-14 and [50]'s reservations; the `Vfs` fault model; the gate-0 carrier table; the model with its full logical semantics, its own query parser and its own canonical form; GT13 on the model; FL-1 libraries and replay corpora (second lane); layout and T1/T2 trigger probes; measurement protocol; repository, CI and test host | — | M0 |
| M1 | storage engine plus the section/record registry, every reserved kind exercised; GT15 | M0 | M1 |
| M2 | graph core plus FL-3 (file nodes, derived uids, `at` + anchors, `suspect` extension, I-F invariants) | M1 | M2 |
| M3 | version control plus FL-7 merge rules, the recursive base, the shared FL-1 diff | M2 | M3 |
| M4 | git object layer (second lane from the certified `Vfs`) | M1 | M4 |
| M5 | git image core plus FL-8, gated through the `Store` API and a driver | M3, M4 | M7 |
| M6 | file-link runtime: FL-2 `ProjectFs`, FL-4 resolver, settle and intent protocol, FL-10 accelerators | M1–M4 | M8 (part) |
| M7 | query language plus FL-6 link built-ins | M2, M3, M4, M6 | M5 |
| M8 | CLI plus FL-5, image and file verbs | M5–M7 | M6 |
| M9 | agent interface plus FL-9 and `links import` | M8 | M9 |
| M10 | MCP | M7, M9 | M10 |
| M11 | release hardening (M-8 floor; the M-9 drill; GT15 in RG3) | all | M11 |

If this order is taken, the edit list's milestone labels (E3, E11, E27, E31, E32, E34, E35, E45 and others) must be regenerated.
