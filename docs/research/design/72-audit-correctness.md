# 72 — Audit of the integrated design: CORRECTNESS

*Date: 2026-09-26. Design stage, no code. Axis: **correctness**, one of the owner's four priorities (speed, minimal RAM, correctness, minimal agent tokens). Audited: [AR] `docs/ARCHITECTURE-RESEARCH.md` (read in full, 1,885 lines, including the Review log), [40] `40-file-links-design.md` rev 2, [50] `50-query-language-design.md` rev 2, [60] `60-roadmap.md` issue 2. Evidence: research reports [00]–[20] (in particular [17] durability, [18] locking and liveness, [20] crash rigs), the critiques [21], [41], [51], [61] and the review logs of [AR], [40], [50], [60]. One web check (Microsoft's `MoveFileExW` reference) is cited as [D, MS].*

*Tags follow [AR]: **[M]** measured on the owner's machine (quoted from the report that measured it), **[D]** vendor documentation, **[I]** inference by this audit. Locations are `[doc §section]`; quotes are verbatim from the named document.*

---

## 0. Verdict

**CHANGES REQUIRED before the M0 format freeze.** 2 blockers, 13 majors, 8 minors. None of them requires reversing an owner decision: every fix stays inside the owner's rules (own engine, no third-party database anywhere, every component built once to its final specification, operational policy in `config`). Nine of the fixes touch frozen M0 content (the `HEAD` slot, the lease row, `RecHdr` flags, the canonical form's anchor props, a store-wide uid index, the marker rule table, the resolver constants, the `Vfs`/`ProjectFs` trait surface, the GT1 enumerator), so they must land in the M0 specification review, not later.

The storage protocol's *process-crash* story is strong: readers bounded by `committed_lsn`, the ref move inside the commit record (N3), idempotency after republish (F-B7), the blocking lock wait, a frozen fault model, a model-based differential harness and mutation testing. The weak points are all on the **OS-crash** side and at the **seams the R4/R5 integration created**:

1. `HEAD` is never flushed, yet recovery and lock-free readers treat its `committed_lsn` as a trustworthy bound. After an OS crash it can lag the durable log (readers hide acknowledged commits) or lead it (lazy publishes; the recovery scan starts past a hole) — **B1**.
2. Lease liveness names no process. The natural reading, "the process that wrote the lease", records a CLI process that exits within ~0.2 s, which voids every CLI-taken lease; reboot and clock steps are also unspecified — **B2**.
3. The two OS-crash questions [61 B2] were answered by [60 §2.5] decisions (a) and (c), but (a) never reached [AR §4.5], covers only `Commit` records and ignores torn flushed groups (**M1**), and (c) flushes one `HEAD` slot while the other can still name the deleted file (**M2**); GT1's "per-file prefixes" enumeration cannot see either (**M3**).
4. The `settled`/`deleted` marker lifecycle breaks under `undo` of a reopen, `op restore`, forks and `branch -D`, and the reference model encodes the same event rule, so no differential gate can catch it (**M4**).
5. R4's one-sided glob composition contradicts G17's sync residue rule (**M5**); `image.anchor-text = hash-only`, a config key, changes hashed image content (**M6**); derived-uid `#N` reuse has no store-wide uid index to enforce it (**M7**).

---

## 1. Scope and method

- Every invariant of [AR §3.4] and [AR §5e.8] was traced to its enforcement point (write path [AR §4.5], merge [AR §5a.7], import [AR §5b.6], recovery [AR §4.5 step 2]) and to a gate of [60 §3.13] (table in §3).
- Every durable record kind was walked through the recovery scan under the frozen `Vfs` fault model [60 §2.5] rows (1)–(8).
- R4 and R5 were checked against the R1/R3 invariants: merge rules for link fields against G17 sync, I25′ and the GT6 properties; LQ `TX` against leases, guards and idempotency; named queries across branches and through the image; anchors and queries through the image.
- Multi-process races were checked against the lock protocol and the lock-free readers; Windows hazards against [05], [08 §4], [17], [18], [20].
- Confidently-wrong answers were checked for LQ semantics, automatic re-binding and stale packs.
- **Not re-raised** (resolved in a Review log): CB1–CB3, CM1–CM9, CL1–CL9 [AR]; B1–B4, M1–M12, m1–m18 [40]; B1–B3, M1–M10, m1–m11 [50]; B1–B2 and the M-/m-items of [61] as dispositioned in [60]. Where a finding below touches one of them, it states what is still open after the disposition.

---

## 2. Findings

Severity: **blocker** = must be fixed before the M0 freeze because it changes frozen bytes and otherwise silently violates a stated guarantee; **major** = a stated invariant or guarantee fails in a reachable scenario, or a gate cannot detect a reachable defect; **minor** = a narrower defect, a spec inconsistency or a missing test.

### B1 — `HEAD` is not a valid recovery or visibility bound after an OS crash (blocker)

**Where.** [AR §4.2]: "The slot is written after the commit flush and **not** flushed itself; a stale slot is rebuilt by the next writer from the log (1PC+C)". [AR §4.5 step 2]: "**Recovery scan**: from `committed_lsn` to the first bad checksum or epoch mismatch". [AR §6.5] `lazy`: "appended and published; flushed by the next durable commit". [40 §2.6]: "Hooks take the writer byte only to append a lazy record, with no flush". [AR §4.7]: readers "replay `(replayed_lsn, committed_lsn]` … never beyond `committed_lsn`". Fault model (1) [60 §2.5]: "any subset of the 4 KiB sectors written to a file since its last successful flush may be lost".

**Problem.** 1PC+C assumes that after a crash `HEAD` can only be *stale-older* and that only a *writer* needs fixing. Under the frozen fault model the on-disk `HEAD` can be older **or newer** than the durable log, and lock-free readers never recover.

**Scenario A — `HEAD` lags; readers hide acknowledged commits.**
1. The orchestrator writes critical rule `#212` and completes `#89`. Both commits are flushed and acknowledged. Their `HEAD` publishes sit in the cache.
2. An OS crash follows (the development machine has experienced OS crashes [02 §9]). The durable `HEAD` is an older slot.
3. After reboot the first process is the `SessionStart` hook, a reader. It renders the brief from the old `committed_lsn`: `#212` is absent, and `#89` is listed as ready.
4. Every read-only process — `pack`, `brief`, `ready`, MCP `query` — keeps serving the pre-crash view until some process happens to write.

This breaks L1 of "node 40" [AR §1 row 11] and the "never re-dispatched" promise of N2/D1 at exactly the moment the owner resumes work.

**Scenario B — `HEAD` leads; acknowledged commits land behind a hole.**
1. An evidence hook (0.9 % of shell calls [M, 13 §4.2]) appends a lazy `FileObs` record L and publishes `HEAD` past it, without a flush.
2. An OS crash follows. The write-back order is arbitrary: `HEAD`'s sector survives and L's sector reverts to zeros. Fault model (1) allows this, per file.
3. After reboot the next writer's recovery scan starts at the new `committed_lsn`, **after** the hole. It finds nothing to adopt, appends commit C, flushes and acknowledges.
4. Any later replay starts at or before the hole: a reader opening from `checkpoint_lsn`, `repair --rebuild-from-log`, the retirement of this extent into `hist`, or recovery from an older slot after another crash. That replay meets zeros at L and either "stop[s] at the first bad record" [AR §4.10], dropping the acknowledged C, or, under [60 §2.5] decision (b), "stops and reports `repair`".

R4 makes lazy-only windows routine: the `SessionStart` settle in a reader tree writes only lazy `PENDING` rows, and every evidence hook writes lazy rows.

**Why the gates miss it.**
- GT15's post-reboot oracle is "every recorded commit must be present and `doctor --verify` clean" [60 §3.13]. Nothing checks what a *reader* shows before the first writer runs.
- The model's crash semantics [60 §4.4 item 4] check the state after "recovery", not reader freshness.
- Scenario B *is* inside GT1's per-file-prefix enumeration (the `HEAD` prefix survives, the log prefix is empty), so GT1 will fail on the design as written. The fix changes the `HEAD` layout, so it must be made before the freeze, not discovered at M1.

**Fix (format, before M0).**
1. Add `durable_lsn u64` (the end of the last flushed group, advanced only by durable publishes) and `boot_id [16]` (Windows: the system boot time; the `HolderId` boot identity of [20 §0 item 10]) to the `HEAD` slot.
2. A process that reads a `HEAD` whose `boot_id` differs from the current boot takes the writer byte once (G1 wait), runs recovery, republishes with the current `boot_id`, and flushes `HEAD` once per boot, off the commit path. Only then does it read.
3. The recovery scan starts at `min(durable_lsn, checkpoint_lsn)`, not at `committed_lsn`.
   - An invalid record in `(durable_lsn, committed_lsn]` is the end of the log: a lost lazy tail. It is overwritten, and `committed_lsn` is republished at the last valid record.
   - An invalid record below `durable_lsn` is corruption: exit 7 and `repair`. This is decision (b), scoped so that it never fires on a benign lazy tail.
4. Readers that meet an invalid record in `(durable_lsn, committed_lsn]` treat it as the end of the visible log, never as an error.
5. Adoption re-writes every record in `(durable_lsn, end]` (M1), because a failed flush covers the lazy records before it as well.

**Gate.**
- Seeded bugs for M0 harness validation and the M1 engine: "writer trusts a lazily published `committed_lsn`" and "reader serves a pre-crash view".
- A new **post-crash freshness** assertion in GT1, GT3 and GT15: every read by any process started after a crash reflects every acknowledged durable record, before any writer has run.

### B2 — Lease liveness has no defined subject; CLI leases die on exit; reboot and clock semantics are unspecified (blocker)

**Where.** [AR §6.2]: "A lease records `(pid, process start time)`; at the next read that evaluates it, a lease whose PID is gone *or whose start time differs* (PID reuse) is treated as dead." [AR §4.4] `LEASES` row `{#N, holder, token, expires, run, pid, pid_start, branch}`, frozen at M0. [60 §2.5] decision (e): TTLs "specified against the monotonic clock". [AR §3.5]: `ready` excludes "a live lease by another holder". Research [18 §0 item 4, §8.3] and [20 §0 items 3c, 5].

**Problem.** The design never says *whose* pid is recorded. The only process that exists when `moirai claim` runs from an agent's Bash tool is the CLI itself: its whole call takes 115–190 ms [AR §8.1]. Walking up to the Claude process on Windows needs a process-snapshot walk whose parent pids are themselves reusable (PIDs are "multiples of 4, reused aggressively" [18 §8.1]).

**Scenarios.**
- **The CLI records its own pid.** `dev#1` runs `moirai claim 89 --ttl 15m`. The CLI exits about 0.2 s later. At the next `ready`, the lease is "dead". `#89` is dispatched to `dev#2` and both build it, which is the owner's stated fear [07 §7.4].
- **A dispatcher's run-scoped lease.** If the rule is applied to a lease taken by a Workflow dispatcher's `claim --ttl run`, every lease in the run dies at once.
- **A different principal.** [20 §0 item 5] reports Claude Code 2.1.281 embedding `srt-win`, which runs sandboxed commands as a separate `srt-sandbox` user. `OpenProcess` then returns `ERROR_ACCESS_DENIED`, which the rule does not classify.
- **A reboot.** Leases persist across an OS crash. A monotonic deadline from the previous boot (decision (e)) is meaningless after it, so a 15-minute lease can look alive for the whole previous uptime. Conversely, every holder is certainly dead after a reboot.

**Gates.** None catches this. The model excludes "locks, timing and processes" [60 §4.3]. GT4 checks acknowledged commits, not claim exclusivity.

**Fix (format, before M0).**
1. Replace `(pid, pid_start)` as the liveness key with a **holder anchor** `{kind, slot u16, nonce u64, boot_id}`, per [18 §8.3]'s lock-anchored slots. The session's MCP server holds a slot byte for its lifetime; a CLI or hook finds its session's slot by the `CLAUDE_CODE_SESSION_ID` hash. `(pid, start)` stay as diagnostics only.
2. A lease with no anchor is governed by TTL or run scope alone. The CLI **never** records its own pid as the holder.
3. Liveness is three-valued: Alive, Dead or Unknown. **Unknown never expires a lease.**
4. Store `expires` as a wall-clock deadline plus `(boot_id, monotonic deadline)`. On the same boot, use the monotonic deadline. On a different boot, non-run-scoped leases are Dead and are released at the first read with a triage line. Run-scoped leases keep their rule (`apply`, `run close`, `reclaim`).
5. The same anchor replaces the pid check for `FsIntent` recovery [40 §3.4 step 5].

TTL values stay `lease.*` config keys.

**Gate.** GT4 variants:
- a claim made through the CLI, after which 1,000 `ready`/`claim --next` calls from other processes within the TTL never return the task;
- a second, restricted principal: Unknown never expires;
- wall-clock steps of ±1 h change no expiry decision.

GT15 adds: after a reboot, non-run leases are released at the first read and run-scoped leases are kept.

### M1 — Adoption: [AR] still re-flushes, adopts only `Commit` records and has no group atomicity (major)

**Where.** [AR §4.5 step 2]: "every complete `Commit` found is adopted (re-flushed once …)". [AR §4.10]: "Flush failure aborts the process; the next writer recovers." [AR §4.5 step 7]: "append it and any `Lease`/`Marker` records in one write". [AR §4.3] `RecHdr`: "A record whose epoch or checksum does not match ends the log." [60 §2.5] decision (a): "re-writes its bytes (or verifies them through an unbuffered read) before flushing — re-flushing alone proves nothing". [17 §3.5]: "Choose re-write everywhere"; Windows' page state after a failed flush is "unknown".

**Problems.**
1. **[AR], the implementers' contract [AR "For implementers"], still specifies the seeded bug "adopt by re-flush only"** [60 §3.2]. An engine built from §4.5 fails M1.
2. **Only `Commit` records are adopted.** A flushed and acknowledged `RefUpdate` (`undo`, `branch`, `tag`), `Lease` (`claim` writes "a `Lease` record only" [AR §6.2]), `ClientHead`, `Pin`, `GitMap`, `Checkpoint` or `FsIntent*` record found past `committed_lsn` has no rule. It is either silently skipped — lost although acknowledged — or silently published by the next writer's republish without being applied to that writer's ref table, so its commit's `ref_old` CAS runs against a stale ref.
3. **No group framing.** A commit and its `Marker` records are separate records in one write. A crash can persist the `Commit` sector and tear the `Marker` sector.
   - Recovery adopts the completion without its `settled` marker, and I26′ fails exactly as N2/D1 described.
   - Likewise, `complete` can lose the record that "releases the lease into `settled`".
4. **Record validity omits the position.** A record whose `lsn` differs from the scan position is not rejected. If a recycled extent's zero-fill is ever incomplete, a same-epoch record from the extent's previous life validates.

**Fix (spec text and one format bit, before M0).**
1. [AR §4.5 step 2 and §4.10]: "every complete record in `(durable_lsn, end]` is re-written from the adopter's read buffer, then flushed, in log order, and applied by kind". Drop "or verifies them through an unbuffered read" from [60 §2.5 (a)], per [17 §3.5].
2. Reserve `RecHdr.flags` bit1 `group_end`. Recovery adopts a flushed group all-or-nothing and truncates at the first record of an incomplete group.
3. The `MARKERS` fold is defined from the net ops of adopted commits; `Marker` records are a cache that must agree.
4. Validity = length and kind sane ∧ epoch = `HEAD.epoch` ∧ `lsn` = scan position ∧ xxh3 matches.
5. Add an M0 measurement: flush-failure injection on a VHDX that goes offline mid-flush, to record what Windows returns and what later reads see. The re-write rule is safe either way; the measurement parameterises fault-model item (3).

**Gate.** New seeded bugs: "commit adopted without its group's `Marker`", "non-commit durable record skipped by recovery", "`lsn` not checked on a recycled extent". The model compares markers and leases after every GT1 recovery ([60 §4.4 item 5] compares markers for merges only).

### M2 — The durable `HEAD` barrier flushes one state, but a crash can revert to the other slot (major)

**Where.** [60 §2.5] decision (c): "every file deletion, extent retirement or recycling is preceded by a durable `HEAD` barrier (`FlushFileBuffers(HEAD)` naming a state that no longer needs the file)". [AR §4.2]: two 4 KiB slots; "readers take the valid one with the higher `slot_seq`". Fault model (1).

**Scenario.**
1. The barrier writes S10 (without file F) into slot A and flushes. The flush also makes slot B durable at S9, which still names F.
2. F is deleted.
3. Two publishes follow: S11 into slot B, then S12 into slot A. Neither is flushed.
4. A crash loses B's sector, which reverts to S9, and tears A's sector at 512 B, so A is invalid. Both outcomes are allowed by fault model (1).
5. The only valid slot is S9, which names the deleted segment or the retired log extent.
   - Every lock-free reader fails to open ("re-read `HEAD` and retry, bounded" cannot help).
   - The writer's recovery starts from a `committed_lsn` inside a deleted extent.

This is exactly [61 B2]'s second defect, still reachable.

**Why the gates miss it.** GT1 enumerates "per-file prefixes plus one torn sector". The fatal state — the older write lost, the newer write torn — is not a prefix (**M3**).

**Fix (spec text, before M0).**
1. The barrier makes **both** slots name states that do not need the file: write S to slot X, flush, write S (`slot_seq + 1`) to slot Y, flush. That is two ~2 ms flushes per maintenance batch, off the commit path.
2. Defence in depth: recovery that finds a `HEAD` naming a missing file rebuilds the segment set from the durable `Checkpoint` records instead of failing.

**Gate.** GT1 enumerates all {old, new, torn} × {old, new, torn} slot states at every barrier point (9 per point). Seeded bug: "single-slot barrier".

### M3 — GT1 enumerates less than the frozen fault model allows (major)

**Where.** [60 §3.13] GT1 and [60 §3.1 item 3]: "bounded subsets of the unflushed writes — per-file prefixes plus one torn sector, as in CrashMonkey". [60 §2.5] fault model (1): "**any subset** of the 4 KiB sectors … may be lost".

**Problem.** Per-file prefixes cannot produce a surviving later write together with a lost earlier write in the same file. That gap is exactly:
- the state decision (b) exists for ("valid records of the same epoch beyond a bad one");
- M2's slot state;
- B1's intra-log variants: a lazy record lost while a later lazy record survives.

A gate that cannot generate the states a protocol rule exists for cannot certify that rule. The **RG3** claim "GT1 exhaustive over every record kind under the fault model" [60 §6] is therefore not true as specified.

**Fix.**
- Enumerate every subset while a file has ≤ 12 unflushed sectors (4,096 states), and at least 10⁴ random subsets beyond.
- Enumerate `HEAD`'s two slots exhaustively (9 states).
- Combine files as a bounded product.
- Keep the prefix enumeration as the fast PR-level tier; the full enumeration runs nightly.

**Gate.** Harness validation at M0 must catch M2's single-slot barrier and B1's lazy hole on the toy log.

### M4 — The `settled`/`deleted` marker lifecycle is event-based and incomplete, and the model shares the flaw (major)

**Where.**
- [AR §5a.5] `undo`: "writes `Marker{cleared}` … for every `settled`/`deleted` marker whose commit it removed from R".
- [AR §5a.5] `op restore`: "restores all refs to their values at `seq`" — no marker rule at all.
- [AR §5a.9] `branch -D`: "writes `Marker{cleared}` for every … marker the branch still held unabsorbed".
- [AR §4.4] `MARKERS` rows `{#N, kind settled|deleted|cleared, ref_id, …}`: the scope of `cleared` (per `#N` or per `(#N, ref)`) is unstated.
- [60 §4.2] model: "exhaustive: the marker's commit is an ancestor-or-self of `tip(R)`, by DFS; `cleared` markers per [AR §5d.1]".
- [AR §8.2] ten-door test.

**Problem.** Markers are created and cleared by *events* (ops and verbs), while I26′'s intent is a property of *state*: "a node another live branch completed or deleted" [AR §5d.3]. Ref moves that re-expose or hide states are not ops, so the events drift from the states.

**Scenarios.**
1. **Undo of a reopen.** `#89` is done on `lane/l5np` (marker at `ref_seq` 41). `reopen` at 42 writes `cleared`. `undo` moves the lane back to 41: `#89` is done on the lane again, but no marker exists. `main` lists `#89` in `ready`, and `claim --next` hands it to `dev#3`. This is N2/D1's double dispatch. The same happens for `undo` of an `Undelete` and for `op restore` forward.
2. **`op restore` backward.** It removes the completing commit from every ref, but no `cleared` is written. `#89` stays "done on `lane/l5np` (unmerged)" on every branch forever, although no branch holds it done. `ready` silently omits a dispatchable task.
3. **Forks.** `lane/y` is forked from `lane/x` after `#89` was completed on `x`. `x` is then deleted with `-D`, or undoes the completion. The marker (with `ref_id = x`) is cleared, yet `y` still holds `#89` done. `main` re-dispatches it.
4. **Unscoped `cleared`.** `#N` is done on lane A. `set --done` on lane B is allowed, because only `claim` checks markers [AR §4.5 step 5]. `reopen` on B then writes `cleared` for `#N`. If `cleared` is per `#N`, A's completion vanishes from every other branch.
5. **Staging refs.** A merge that stages writes markers with the staging ref's `ref_id` [AR §5a.7 step 7]. `merge --abort` deletes that ref, and the effect on markers is unspecified.

**Why the gates miss it.** The reference model encodes the same event rule ("`cleared` markers per [AR §5d.1]"), so GT2 and GT6 compare two implementations of one flawed rule. This is the "shared misunderstanding" risk [60 §4.5] made concrete. The ten-door test enumerates the doors that *create* markers and four that clear them, but none of scenarios 1–5.

**Fix (the owner-signed rule table, before M0).**
1. **Define I26′ by state.** `#N` is excluded on R iff some live ref X of kind `work`, other than R, has `#N` done, cancelled or deleted at `tip(X)`, and the commit on X's history that last set that state is not an ancestor-or-self of `tip(R)`. The **model computes this definition exhaustively from states**, not from markers.
2. **Engine markers become a cache of that definition.**
   - Key them by `(#N, ref_id, commit)`; `cleared` is scoped to `(#N, ref_id)`.
   - `undo` and `op restore` recompute markers for every ref they move: `cleared` for completion and deletion commits that leave the ref's history; `settled`/`deleted` re-emitted for commits that re-enter it while the state still holds at the new tip. This is O(commits in the moved range), acceptable for rare explicit verbs.
   - `branch -D X` and `undo` on X re-attribute X's markers to live refs that still contain the marker's commit (`absorbed_Y[X] ≥ ref_seq`) instead of clearing them.
   - Staging refs produce no markers; the landing commit does.
   - Markers are derived from the **net** (post-coalescing) op list, so `TX { REOPEN t; SET t.done = true }` on a done task emits nothing.

**Gate.** An I26′ state-oracle generator (≥ 10⁶ histories, ≥ 5 refs) covering scenarios 1–5, required at the M3 exit, with every read and claim compared against the state definition.

### M5 — R4's one-sided glob composition contradicts G17's sync residue: lane views, canonical diffs and the model disagree (major)

**Where.**
- [AR §5a.3] `sync`: "appends a `sync` commit … and **only the resolution ops for keys both sides touched** since the last sync".
- [AR §4.6]: the sync's canonical op list is "`main`'s window materialised plus the resolutions".
- [AR §5a.7] table: "globs add-wins and composed through the other side's recorded moves".
- [40 §5.5]: "entries added on one side are composed through the other side's `explicit`/`confirmed`/`committed` `path_moves` entries added since the LCA".

**Problem.** Composition changes a key that **only one side touched**. The glob key is untouched on the lane, so G17 stores no resolution for it.

**Scenario.**
1. On `lane/l5np`: `file mv docs/plan docs/archive/plan`, the [AR §7.1] example. The root node's `path_moves` gains the entry `docs/plan/ → docs/archive/plan/`, and the lane's own globs are rewritten.
2. Meanwhile on `main`: rule `#230` is written with `applies_to path:docs/plan/**`.
3. `sync` runs. The merge rule composes `#230.applies_to` to `docs/archive/plan/**`, but the lane never touched that key, so no resolution is stored.
4. The lane view (`main`'s window by reference plus the residue) and the canonical op list built from the same parts both show the uncomposed `docs/plan/**`.
5. On the lane, `#230` renders `[glob matches nothing]`, and pack class C2/C7 scoping drops it for agents editing `docs/archive/plan/` — a critical rule silently missing from their packs.
6. The sync-first merge into `main` then carries the uncomposed value to `main`.

The model merges materialised states [60 §4.2], so it *does* compose. GT2 and GT6 would disagree at the M3 exit — if the generator happens to produce this pattern.

**Fix (spec text, before M0).**
- Redefine the residue as **every key whose merged value differs from what `main`'s window alone yields at the lane**: the keys both sides touched, plus keys changed by a one-sided composition or a re-key.
- Keep the canonical op list as the true state diff, and assert at sync time that the window plus the residue equals it; `doctor --verify` re-checks.
- Restate GT6's "a clean merge equals sequential application" modulo composition (m4).

**Gate.** A GT6 generator that combines `path_moves` with globs on the other side, in both directions. Required: `view(lane)` after sync = the model's `state_at(sync)` = the exported `.moi` tree; then `merge lane → main` is composed.

### M6 — `image.anchor-text = hash-only`, a `config` key, changes hashed image content (major)

**Where.**
- [AR §4.6 item 10]: "an `at` edge's `props` is its anchor selector block".
- [40 §5.7]: "`image.anchor-text = hash-only` writes span hashes and windows without the text for a chosen destination".
- [AR §11] config table: `image.anchor-text` (per destination).
- I28′: "The git object ids of a moirai commit are a function of moirai data and the object format only"; I29′; I38′.
- [AR §5b.7] gates 1–3 (no hash-only corpus).

**Scenario.**
1. The owner sets `hash-only` on the separate bare repository, whose private remote has different access rights — the case the key exists for [41 m17].
2. After losing the store, the owner imports that image into a fresh store.
3. Every commit whose tree diff touches an `anchor` line is rebuilt without `quote`/`prefix`/`suffix`, so its canonical hash differs from `Moirai-Commit`. It is demoted to foreign with a new id (I29′ fails), and `gitmap` diverges.
4. At the owner's scale of 10–35k citations [40 §7.3], most link commits demote.
5. The recovered anchors lose the selector that resolves 96.3 % of citations, against 27.2 % for line numbers [M, 11 §2.3].

A configuration value thereby changes hashed content and identity, which the owner's rule reserves for owner decisions.

**Fix (format, before M0).**
1. The canonical form hashes anchor text only through digests (`quote_h`, `prefix_h`, `suffix_h` = BLAKE3-128 of the text). Every `anchor` line carries the digests; `full` mode also carries the text, which the importer verifies against its digest.
2. Restate I28′ as a function of moirai data, the object format and the destination's declared anchor-text mode, recorded in the unhashed side ref `refs/moirai/meta/<store-id>`.
3. An import from a hash-only image yields anchors in an explicit `text-unavailable` sub-state: they resolve by hint, window and scope only, never as `fresh` by quote, until `links fix --repin --at …`.

**Gate.** GT8 gates 0–3 run in both modes; a hash-only import verifies 100 % of native commits.

### M7 — Derived-uid `#N` reuse has no enforcement point: there is no store-wide uid → `#N` index (major)

**Where.**
- [AR §3.4] I1: "Creating a derived-uid node … whose uid the store already knows, **on any branch**, reuses that uid's `#N`".
- [AR §4.8]: "uid → id | sorted `UID` column | segments (import/export only)".
- F17 `ALLOC` is `#N → (ref_id, create_seq)`, with no uid.
- [40 §2.11] R-8 lists no uid index.

**Problem.** The `UID` section belongs to a view's segments. A uid created on an unmerged lane lives only in that lane's log and overlay.

**Scenario.**
1. `dev#1` on `lane/l5np` links `crates/engine/src/lock.rs`, which derives uid U; `#812` is allocated.
2. Before any merge, `tester` on `lane/l10` links the same file. The writer finds no U in `main`'s segments or in `l10`'s view, and allocates `#901`.
3. I1's "`#812` means the same file in every lane" fails.
4. At the merge, "created on both sides → one node" must collapse two `#N`. That means rewriting CSR entries, `ANCHORS` keys `(src#, dst#, anchor#)` and the `FILEOBS` rows keyed by `(#N, tree)`, and orphaning runtime rows. None of this is specified.

Import's `IdCollision` check ("a `uid` live with a different `created` commit") needs the same store-wide lookup.

**Fix (format, before M0).** Widen `ALLOC` to `#N → (uid, ref_id, create_seq)` (+16 B per id; 1.6 MB at 10⁵ ids), plus a sorted uid → `#N` section folded at checkpoints. The write path probes it under the writer byte, import uses it for `IdCollision`, and `repair` rebuilds it from the log.

**Gate.** A GT2 (M2) generator that links the same file on two lanes before any merge, then merges and syncs. Required: exactly one `#N` per uid over the store's life.

### M8 — File verbs and store maintenance lack namespace-durability barriers (major)

**Where.**
- [40 §3.4]: "`MoveFileExW(src, dst, 0)`: never `MOVEFILE_REPLACE_EXISTING`, never `MOVEFILE_COPY_ALLOWED`", then "One durable commit that carries … `FsIntentDone`".
- [40 §3.5] `DeleteFileW`, then the same commit shape.
- [AR §4.10]: "Segments are written to a temp name, flushed, then referenced by a flushed `Checkpoint`".
- Fault model (2) [60 §2.5]: metadata operations "survive a crash as a *prefix of their issue order* on the volume, independently of unflushed data".
- [40 §8.3.5]: crash enumeration of the intent protocol "combined with sharing-violation and delay injection" — no OS-crash semantics.

**Problem.** `DATA_SYNC_ONLY` on the log does not persist the rename or delete that the commit records. Under the design's own fault model the commit can survive while the namespace operation does not.

**Scenario 1 — `file rm`.**
1. `file rm docs/old.md --yes`. The intent is flushed, `DeleteFileW` runs, and the commit `{removed, FsIntentDone}` is flushed and acknowledged. Every referrer is `suspect`.
2. An OS crash follows.
3. After reboot `docs/old.md` exists again, while the graph says `removed` and the intent is closed, so recovery never looks at it again.

**Scenario 2 — directory `file mv`.**
1. `file mv docs/plan docs/archive/plan` commits the `explicit` `path_moves` entry and the rewritten globs.
2. After an OS crash, the rename is undone.
3. Settles re-bind the observations back through aliases, but the `explicit` entry and the glob rewrites stay. The globs match nothing, and E5 and merge composition keep applying a move that never happened.

**Scenario 3 — store files.** The same class hits segment, `hist` and `blobs` temp → final renames: a durable `Checkpoint` can reference a name whose rename was lost, and the orphan sweep deletes the temp file (G14).

**Fix (spec plus the `Vfs`/`ProjectFs` trait surface, before M0).**
1. `file mv` passes `MOVEFILE_WRITE_THROUGH`: "The function does not return until the file is actually moved on the disk" [D, MS].
2. Deletes, `--trash` and every store temp → final rename are followed by `FlushFileBuffers` on the parent directory handle, opened with `FILE_FLAG_BACKUP_SEMANTICS` and write access (0.07 ms p50 [M, 17 §0 item 4]), before the commit or `Checkpoint` that depends on the name.
3. Add `sync_dir` to both traits, as [17 §0 item 4] proposes for Unix.
4. The M0 rig calibration (item 17) is extended to namespace operations. The calibration decides whether the flag, the directory flush or both are required.
5. After a boot change, `doctor` compares recent `FsIntentDone` outcomes with the file system.

**Gate.** The `ProjectFs` simulator implements fault-model (2) for crashes. GT15 streams `FsIntent` outcomes and compares them with file-system ground truth after reboot.

### M9 — Image export and `backup` are acknowledged before their bytes and names are durable (major)

**Where.**
- [AR §5b.6 step 5]: "objects and refs are written by the git object layer before `gitmap` records are appended under the writer byte in one durable commit". No flush of packs, refs or names is stated.
- [AR §7.1] `backup`: "copied 61.3 MB … xxh3 verified on every file" (a read check, not durability).
- [AR §7.6 step 9]: export and `backup` run "before the nightly quiet window".
- CM8 makes both the off-store copy.

**Scenario.**
1. The nightly export writes a ~20–25 MB pack at 10⁵ nodes [AR §5b.9] and renames it into `objects/pack/`.
2. The `gitmap` commit is flushed.
3. An OS crash follows within the write-back window. The pack reverts to zeros, or the rename is lost.
4. After reboot the image ref names a missing object, but `gitmap` says it was exported. The next export's frontier stops at the `gitmap` entry ("walk moirai commits from the tip down to one already in `gitmap`") and never re-emits.
5. The off-store copy the owner relies on after the next store loss is silently broken. `backup` has the same shape.

**Fix (spec text).**
1. Export order: pack and idx data flushed → rename (write-through) → directory barrier → `<ref>.lock` written and flushed → rename → directory barrier → side ref → then the durable `gitmap` commit.
2. Every export begins by checking that the newest `gitmap` entry per (destination, algorithm, ref) names an object present in the destination, and re-emits otherwise.
3. `backup` flushes every copied file and the directory before reporting success, and appends a durable `Backup{dir, committed_lsn, digest}` record so the backup-age check in `doctor` and `brief` is truthful.

**Gate.** After every reboot, GT15 runs `git fsck --strict` on the image, checks `gitmap` ⊆ the destination, and restores the last acknowledged backup.

### M10 — Importing onto a locally diverged ref is unspecified (major)

**Where.** [AR §5b.6 step 4]: "Clean → append and move the local ref." [AR §7.6 step 9]: "A colleague's hand edit on the image comes back through `image import` as a foreign commit the next morning." [AR §7.1] example: "applied on main as c9c1 (import-foreign)".

**Scenario.**
1. The export at night is checkpoint X.
2. Overnight, `main` gains local commits (about 1k commits a day [AR §8.1]).
3. The colleague's foreign commit F has parent X. On import, "move the local ref" means either moving `main` to F, dropping every overnight commit from `main`, or re-parenting F onto the tip.
4. Re-parenting changes F's deterministic id, which is "a pure function of the git commit and the parents' ids", so two stores with different local tips compute different ids for the same foreign commit (I28′).

**Fix (spec text).**
1. Imported commits always keep their stated parents.
2. When the local tip is not an ancestor-or-self of the imported chain, the chain lands on `import/<ref>`, and the importer runs the typed `merge import/<ref> --into <ref>`, with its LCA at the last common exported commit.
   - A clean merge lands a local merge commit, with its markers and absorbed vector.
   - Violations stage as today.
3. A `config` key (`image.import-merge = auto|stage`, default `auto`) selects between merging and always staging.

**Gate.** A GT8 fixture: export → 5 local commits → a foreign edit on the image → import. All 5 local commits stay reachable; F's id is identical in a second store with a different local tip; the result equals the model's merge.

### M11 — The link settle inside `complete` contradicts `TX` purity and the writer-byte budget; settles have no CAS (major)

**Where.**
- [40 §4.2] table: "`complete ID` | **settle** | links of the completed task's subtree | ≤ 20 ms plus the quiescence wait if it writes | yes, within the same commit".
- [50 §3.10]: "a `TX` statement is a pure function of graph state and parameters [51 B2]".
- [50 §4.2]: `complete` = `TX LEASE 'L-18' { CALL tx.complete(…) }`.
- [AR §6.4]: "every write verb … compiles to one LQ `TX` block".
- [AR §8.1]: "writer-byte hold p99 ≤ 5 ms".
- [40 §3.4]: CAS on `rev_seq` only in `file mv` step 4.

**Problem.** Either:
- **the settle runs under the writer byte.** The ≥ 50 ms quiescence wait then breaks the 5 ms hold and 50 ms wait gates. At 16 writers, one `complete` with a moved link holds every other agent for more than 50 ms.
- **it runs outside the byte.** It then has no CAS, and a stale observation can overwrite a newer one. Example: `links sync` in the same writer tree records `p → q2` after a second move; `complete`'s earlier resolution then commits `p → q1`.

In both cases CLI `complete` (with the settle) and MCP `write{tx: "CALL tx.complete(…)"}` (pure) produce different commits for the "same" mutation. That breaks [AR §7.7.2]'s "a query and a verb can never disagree" and the verb = named-mutation test.

**Fix (spec text).**
1. `tx.complete` stays pure: one commit.
2. The settle that `complete` triggers is a **separate commit**, resolved outside the writer byte (the quiescence wait included). Each re-bind carries a CAS on the file node's `rev_seq` as read at the start of resolution; on a mismatch the op is dropped and left to the next settle.
3. Apply the same CAS rule to every settle: `SessionStart`, `links sync`, `post-commit`.

**Gate.** A GT3 scenario with two settles in one writer tree and an interleaved move: no observation regresses. The writer-byte hold p99 ≤ 5 ms gate is measured on settle-bearing verbs. A verb = mutation equality test for `complete`.

### M12 — GT4 and GT15 check commits only; their oracles miss the other durable effects (major)

**Where.** [60 §3.13] GT4: "Every process reports each acknowledged commit". GT15: "every writer streams each acknowledged commit id … after reboot every recorded commit must be present and `doctor --verify` clean"; mandatory from M1 with "the GT4 workload". [20 §0 item 8]: a VM rig "cannot catch missing device flushes or reordering between flushes".

**Uncovered.** These are acknowledged durable effects that are not commits:
- `Lease` records (`claim` is lease-only, [AR §6.2]);
- `RefUpdate` (`branch`, `tag`, `undo`, `op restore`);
- `Marker` effects;
- `FsIntent` outcomes against the file system;
- `gitmap` against the destination (M9);
- backups;
- post-crash reader freshness (B1);
- lazy-heavy windows before the crash (B1, scenario B).

GT15's workload is fixed at the M1 level, while GT4 grows at M3, M5, M6, M8 and M10.

**Fix.**
1. The harness records every acknowledged durable effect by kind (commit id, lease token, ref move, marker effect, intent id and outcome, `gitmap` entry, backup) and checks each after recovery.
2. GT15 adopts GT4's per-milestone workload growth, including evidence-hook traffic immediately before the power-off.
3. After reboot, add the freshness assertion (B1), `git fsck` on the image (M9) and `FsIntent` against the file system (M8).
4. State explicitly that device-cache loss (W3) is covered by the simulator alone.

### M13 — R4 accepts whatever occupies a path, or a moved directory's slot, as the linked file (major)

**Where.**
- [40 §4.3] step 1: "present → … ok".
- E3d: "q = D′/basename(p) present → exact, **whatever its content** (the same reasoning as 'path present → ok')".
- [40 §4.4]: `replaced` only "when p is present", and only if containment < 0.29 in both directions.
- [40 §8.3.1] matrix rows 1–31: row 16 covers a two-file swap and rename-over of *linked* nodes only.
- R4's guarantee [AR §5e.1]: "no link ever points silently at the wrong file".

**Scenario 1 — promote-replace.**
1. An agent runs `mv crates/engine/src/storage storage_old; mv storage_v2 storage`. This "promote v2" refactor is plausible in the owner's workflow [I].
2. A finding anchored at `storage/log.rs::Log/append` now sees the v2 file at that path: new file id and oid, similar content (containment ≥ 0.29 is likely between two implementations of the same module [I]).
3. The file is `ok`, and the symbol anchor resolves `fresh` in v2's `append`.
4. File-id evidence (E3) places the original at `storage_old/log.rs`, but E3 is never consulted while the path is present.

**Scenario 2 — delete, re-create, then move the directory.**
1. `rm src/net/x.rs`, a `Write` of an unrelated new `src/net/x.rs`, then `mv src/net src/transport`, all before a settle.
2. E3d re-binds F to the unrelated `src/transport/x.rs` as `moved-auto`, an exact and automatic re-bind. That is a P1 violation that the `replaced` rule would have caught had the path not moved.

**Fix (resolver v1 constants, R-14, before M0).**
1. E3d is exact only if `D′/basename(p)` has `FILEOBS`'s file id, or equal size and mtime, or `oid ∈ {o, last_oid}`. Otherwise the §4.4 `replaced` test runs at q, yielding `replaced` or `moved-needs-confirm (directory moved, file replaced)`.
2. At settle, a present p whose file id differs from `FILEOBS` triggers one `OpenFileById(FILEOBS.file_id)` (0.24–0.57 ms [M, 09 §8]). If the original is alive at q with a size, mtime or oid match, the state is `ambiguous (path reused; original at q)`, recorded in the `FILEOBS` state so that reads with an unchanged stat quadruple render it too.
3. New matrix rows 32 (directory promote-replace) and 33 (delete + unrelated re-create + parent move before a settle), and P1 generators for both.

### Minor findings

**m1 — `EXPECT n` admits a count-preserving target swap; `IF TIP` is impractical on busy branches.**
- **Where:** [50 §3.10] items 3, 4 and 9.
- **Scenario:**
  1. An agent runs `DRY` on `MATCH (t:task) WHERE 'l5' IN t.labels AND t.status = 'open' EXPECT 3 SET t.priority = 0` and sees `#89 #90 #91`.
  2. `#91` completes and `#95` is created with label `l5`.
  3. The apply still matches 3 rows and re-prioritises `#95`, which the agent never saw.
  4. `IF TIP` on `main` fails under 16-agent bursts (~2 ms apart), so agents will drop it.
- **Fix:** `DRY` returns a target-set digest, and `TX … IF TARGETS <digest>` guards the bound set instead of the whole tip. GT3 then checks that no apply touches a target outside the `DRY` set.

**m2 — Stale packs have no signal at completion.**
- **Where:** [AR §7.4], [AR §7.5] (`SubagentStart` fires once; `UserPromptSubmit` never fires inside a subagent).
- **Scenario:** A multi-hour Workflow agent works from a pack built at rev 4471. Owner ruling `#212 --critical` lands on its branch at rev 4490. `complete` reports only newly ready ids, so work contradicting the ruling completes silently.
- **Fix:** The pack header carries a digest (the rev plus the C2/C3 member uids and their `rev_seq`). `complete`, `apply` and MCP `complete` accept it and print a notice listing the critical rules, owner rulings and blockers changed since. The notice's verbosity is a `config` key (`pack.staleness-notice`). Gated by a GT10 fixture.

**m3 — The named-query merge decision depends on a derived, version-dependent hash.**
- **Where:** [50 §4.4]: the canonical-AST hash is "derived, stored unhashed as a cache, and recomputed on import", and "changed on both sides to the same canonical-AST hash is not a conflict".
- **Problem:** A binder normalisation change in a later moirai release changes clean-versus-`FieldEdit` for a foreign merge recomputed on import, so I30′ determinism fails across binary versions.
- **Fix:** Freeze the canonical-AST algorithm under the grammar version stored with each query (`lq:`), and version it like the `.moi` encoders (O5), or decide on the portable stored text and signature (byte equality) alone.

**m4 — The GT6 property "a clean merge equals sequential application" contradicts R4's composition rules.**
- **Where:** [AR §8.2]; [40 §5.5] (compose, re-key).
- **Fix:** Restate the property as sequential application through the composition and re-key functions. Otherwise M3's gate fails, or the property gets weakened ad hoc.

**m5 — A retry without a key after a crash-before-acknowledgement duplicates creates.**
- **Where:** [AR §6.4] (keys are optional options); [AR §4.5] (an aborted writer's commit is adopted by the next writer).
- **Scenario:** `add task "X"` is adopted after its writer died with exit 1. The agent retries and gets two tasks.
- **Fix:** The CLI and MCP derive a default key = BLAKE3(session, agent, canonical AST) with a 10-minute window (`--no-dedupe` to opt out). An exit-1 death prints "outcome unknown: re-run with the same key or check `moirai changes`".

**m6 — `restore` swap under long-lived processes.**
- **Where:** [AR §7.1] `restore DIR --into EMPTY_DIR`. The swap into place is manual.
- **Problem:** An MCP server that keeps handles continues to append to the retired directory, because files are opened with `FILE_SHARE_DELETE`, so the directory rename succeeds.
- **Fix:** `restore` performs the swap under the writer and maintenance bytes and sets a reserved `HEAD.flags` bit `retired` in the old store. Every process checks it at each operation and re-discovers the store.

**m7 — The git-oracle corpus misses shallow clones, partial clones, replace refs and grafts.**
- **Where:** [60] M4 GT7 corpus.
- **Problem:** Ancestry across a shallow boundary must answer `unknown`, never `false`, and `git merge-base` honours `refs/replace/`.
- **Fix:** Add these repository shapes to GT7 and define the "unknown" answers.

**m8 — `check` is listed as a read verb but must append facts.**
- **Where:** [AR §7.1] `# read … moirai check ID`; CM7 (cached as a lazy fact); [50 m3] ("`check` records facts"); CI gate "zero log bytes appended by any read verb".
- **Fix:** Classify `check` as a write verb (a named mutation that appends only lazy `ANCESTRY` facts), and exclude it from the I-F5 read set.

---

## 3. Invariants: enforcement point and gate

Status: **ok** = an enforcement point and a gate exist; **partial** = one exists but is incomplete; **gap** = missing. Gates are [60 §3.13].

| Invariant | Enforcement point | Gate | Status |
|---|---|---|---|
| I1 `#N` unique, never reused; derived-uid `#N` reuse | write path step 6 under the writer byte | GT2 (M2), FL-3 properties | **gap**: no store-wide uid index (M7) |
| I2 live endpoints; a flagged edge excludes its dependent from `ready` | step 5 restrict policies; merge step 6; import step 4 | GT2, GT6 I12 fuzz, GT10 node-40 | ok |
| I3 historical edges to dead ids | by construction (tombstones) | GT2 | ok |
| I4 forest, depth ≤ 12 | step 5; merge step 6 | GT6 | ok |
| I5′ combined precedence DAG | step 5 Pearce–Kelly; merge step 6 (full Kahn above the parameter) | GT6 incl. the X1 merge variant | ok |
| I6, I7 cardinalities | step 5; merge step 6 | GT2 | ok |
| I8 status machines; derived state never stored | step 5 | GT2 | ok |
| I9 derived state = recompute | eager maintenance; `doctor --verify` | GT6, `doctor` after every case | ok |
| I10 one commit per mutation with provenance | write path | GT2 | ok |
| I11 schema conformance; symbols never GC'd; enum ints never reused | step 5; GC rules | GT2 covers conformance only | **partial**: no gate asserts symbols survive `gc` or that enum ints are never reused across a strengthen/migrate |
| I12 heads satisfy I1–I11; violations never advance | merge and import step 8 | GT6 I12 fuzz | ok |
| I13, I14 | step 5 | GT2, GT10 | ok |
| I14′ idempotency bound to payload and branch | step 4 after republish | GT2 retries, GT4 | ok (m5 on key-less retries) |
| I17′ fencing; no token bump on expiry | step 5 | GT2 | **partial**: liveness and clock semantics missing (B2) |
| I18′ as-of without derived fields; E302 | read path, LQ binder | GT2 (M7), GT9 | ok |
| I25′ untouched keys never conflict | merge step 3 (base at the LCA) | GT6 | ok |
| I26′ completed or deleted elsewhere excluded | markers from ops; absorbed vectors | GT6 ten doors | **gap**: event rule incomplete and shared with the model (M4); torn groups (M1) |
| I27′ adopted commits reachable, or orphans | recovery step 2 | GT1, GT3 | **partial**: non-commit records unadopted (M1) |
| I28′ git ids a function of data and format | exporter | GT8 gate 3 | **gap** for `hash-only` (M6) |
| I29′ native commits re-verify | importer | GT8 gates 0–1 | **gap** for `hash-only` (M6) |
| I30′ foreign merges by the typed engine | importer | GT8, incidents fixture | partial (m3 across versions) |
| I31′ one base rule for multiple LCAs | merge step 1 | GT6 criss-cross | ok |
| I32′ `rm` refuses under a live lease | step 5 | GT2 | **partial**: depends on lease liveness (B2) |
| I33′ `plan/*` mask | step 5 | GT2 | ok |
| I34′ revert/cherry-pick classes | revert path | GT6 | ok |
| I36′ runtime never versioned or exported | exporter | GT8 (never-exported carriers) | ok |
| I37′ merge validation order | merge step 6 | GT6 X1 variant | ok |
| I38′ one carrier per canonical field | exporter and importer | GT8 gate 0 | ok (plus M6) |
| I39′ tombstones keep out-edges | exporter | GT8 corpus (CM4) | ok |
| I40′ bodies byte-exact | codec | GT8, GT5 | ok |
| I41′ staging per pair | merge step 8 | GT6 CM5 shape | ok |
| I42′ `affected` complete or flagged | write path step 6 | GT2 via [50 §8.3] as-of cone | **partial**: add a direct check: `affected` ⊇ the model's per-commit derived-predicate diff |
| I43′ `append_hlc` monotonic | append | GT3 clock steps | ok, once the B1 recovery precedes the append |
| I-P3 reverse = inverse of forward | write path | GT6 | ok |
| I-F1 one live file node per (root, path) | write path (`PATHIDX`); merge `PathClaim` | FL-3 properties, P8 | ok |
| I-F2 derivations; `#N` reuse | write path | FL-3, P10 | **gap** (M7) |
| I-F3 ≥ 1 anchor per `at` | write path | FL-3 | ok |
| I-F4 no machine-local datum exported | exporter | P10, GT8 | ok |
| I-F5 reads append nothing | executor holds only a read `View` | CI log-byte counter, P4 | ok (m8 on `check`) |
| I-F6 re-binds exact, writer tree, fresh, quiescent | settle write rule | P1, P7 | **partial**: no CAS on settle writes (M11) |
| I-F7 no inferred `removed` | settle write rule | P12 | ok |
| I-F8, I-F9 path rules; no bare line number | write path | FL-3, GT5 | ok |
| I-F10 `resolve` pure | resolver-version constants | P3 | ok |
| I-F11 hands off project files | `ProjectFs` | handle-count probe, cloud tests | ok |
| I-F12 one designated tree per branch | `worktree bind` | P7, matrix row 27 | ok |
| I-F13 no content-only re-bind | copy rule | P1 | **partial**: E3d accepts any content (M13) |
| I-F14 no resurrection | merge re-key rule | P13 | ok |

---

## 4. Do the test instruments cover the risks?

Legend: **C** = covered, **P** = partial, **—** = not covered.

| Risk | Model / GT2 | GT1 | GT3 | GT4 | GT15 | Fuzz | Differential (GT7/GT8/LQ) | R4 GT17 / P | Net |
|---|---|---|---|---|---|---|---|---|---|
| Lost acknowledged commit after a process kill | C | C | C | C | — | — | — | — | covered |
| Lost acknowledged commit after an OS crash: adoption after a failed flush | C (state) | P (prefixes) | P | — | P | — | — | — | **partial** (M1, M3) |
| `HEAD` older or newer than the log after an OS crash | — | P | — | — | P (writers only) | — | — | — | **uncovered for readers** (B1) |
| Deletion before the durable `HEAD` barrier | — | P (misses the slot-B state) | P | — | P | — | — | — | **partial** (M2) |
| Torn flushed group (commit without markers) | C if compared | C (enumerated) | — | — | — | — | — | — | **partial**: the comparison must include markers (M1) |
| Non-commit durable records | — | P | P | — | — | — | — | — | **uncovered** (M1, M12) |
| Namespace operations lost (file verbs, store renames) | — | — (`ProjectFs` lacks it) | — | — | — (no oracle) | — | — | P (kill only) | **uncovered** (M8) |
| Image and backup durability | — | — | — | — | — | — | GT7 (no crash) | — | **uncovered** (M9) |
| Double dispatch through lease liveness | — | — | — | — | — | — | — | — | **uncovered** (B2) |
| Double dispatch or stuck tasks through the marker lifecycle | P (shared rule) | — | — | — | — | — | — | — | **uncovered** (M4) |
| Sync residue vs composition | C, if the generator finds it | — | — | — | — | — | GT8 would diverge | P14 (image only) | **partial** (M5) |
| `hash-only` image identity | — | — | — | — | — | — | not in the GT8 corpus | — | **uncovered** (M6) |
| Two `#N` for one derived uid | P | — | — | — | — | — | — | P10 (uid only) | **uncovered** (M7) |
| Import onto a diverged ref | — | — | — | — | — | — | not in the GT8 fixtures | — | **uncovered** (M10) |
| Settle overwrite race | — | — | P (no settle scenario) | — | — | — | — | P5 (single process) | **uncovered** (M11) |
| Wrong automatic re-bind or state (promote-replace, E3d) | P (brute force; generators lack it) | — | — | — | — | P | — | P1 (rows missing) | **partial** (M13) |
| LQ semantic confident-wrong | C | — | — | — | — | C (grammar) | C (≥ 10⁶ queries) | — | covered (LQ-Bench gate) |
| LQ `TX` lost update or target swap | C | — | C (`EXPECT`/`IF TIP`) | — | — | — | C | — | **partial** (m1) |
| Stale packs | — | — | — | — | — | — | — | — | **uncovered** (m2) |
| Cross-version merge determinism | — | — | — | — | — | — | — | — | **uncovered** (m3) |
| Drive volatile-cache loss (W3) | — | C (simulated) | C | — | — (VM rig cannot see it, [20 §0 item 8]) | — | — | — | simulator only; accepted, state it |

The shared-misunderstanding risk [60 §4.5] is real in exactly one place found here: M4. The rest of the model design (different algorithms, no shared code, owner-signed tables) holds.

---

## 5. The two open OS-crash questions, answered

**(1) Adopting a record after a failed flush.**
- [60 §2.5 (a)] (re-write, then flush) is the right rule and portable [17 §3.5]; drop the unbuffered-read alternative.
- Remaining defects:
  - [AR §4.5] and [AR §4.10] still say "re-flushed once" (M1);
  - the adopter cannot know *which* flush failed, so it must re-write everything from `durable_lsn`, including the lazy records below `committed_lsn`, which requires B1's `durable_lsn`;
  - adoption must cover every durable record kind and whole flushed groups (M1).
- Windows' failed-flush page state is unknown [17 §3.5]. Add the VHDX flush-failure measurement to M0.

**(2) Deleting files before a durable `HEAD` write.**
- [60 §2.5 (c)] is necessary but not sufficient:
  - with two slots, the barrier must make both slots name post-deletion states (M2);
  - the namespace operations that *create* files referenced by durable records need their own barrier (M8);
  - GT1 must enumerate the non-prefix states that expose both (M3).
- Recovery should rebuild the segment set from durable `Checkpoint` records rather than fail.

---

## 6. Windows hazards

| # | Hazard | Status |
|---|---|---|
| W-a | `HEAD` never flushed; write-back order arbitrary after an OS crash | **finding B1** |
| W-b | NTFS page state after a failed `NtFlushBuffersFileEx` undocumented | M1: re-write rule plus the measurement |
| W-c | Rename and delete metadata not persisted by a data-only flush of another file | **finding M8** (`MOVEFILE_WRITE_THROUGH` or a directory flush) |
| W-d | PIDs reused aggressively (multiples of 4); transient CLI processes; `srt-win` runs sandboxed commands as another user | **finding B2** |
| W-e | Lock release lag after `TerminateProcess` (≤ 32 ms p99 [M, 18]) | covered: 2 s bounded wait, delay injection in GT3/GT4 |
| W-f | Defender and indexer sharing violations (errors 5/32), delete-pending names | covered: bounded retries, never re-using a mapped file's name, GT3 injection. Wording: [AR §4.9] "fold d1..d3 into a new d1" must mean a new file number, never the old name, because a delete-pending name cannot be re-created |
| W-g | Hosted Windows runners run with Defender off [20 §0 item 7] | covered: GT4 and GT15 nightly on the dedicated Defender-on test host |
| W-h | Wall-clock steps and reboots versus TTLs and HLCs | B2 for leases; I43′ fine after B1 |
| W-i | NTFS tunneling restores creation times | covered in the copy rule [40 §4.3] |
| W-j | Case-only renames under `core.ignorecase` | covered [40 M1 disposition] |
| W-k | D: has no USN journal | covered: E2 is an accelerator, never needed for correctness |
| W-l | A VirtualBox power-off cannot lose writes already issued to the virtual disk (W3) | accepted; simulator only; state it in GT15 |
| W-m | Cross-kernel access through `\\wsl$` would break byte-range exclusion [18 §9] | minor gap: add `\\wsl$`/`\\wsl.localhost` to [AR §4.1]'s refused paths (WSL is not installed today) |

---

## 7. Confidently-wrong answers

- **LQ semantics.** Sound after [51]:
  - Cypher/GQL counting, walk bounds, two-valued logic with W01, E118, E302;
  - derived state only through built-ins, W07;
  - reverse aliases with the reading echo;
  - LQ-Bench's per-construct confident-wrong gate, 0 on writes;
  - `TX` targets bound inside the lock.
  - Open: m1 (a target swap under `EXPECT n`) and m3 (cross-version merge of named queries).
- **Automatic re-binding.** Exact-only, the copy rule, quiescence and freshness are sound. Open: M13 (a present path or a moved directory's slot accepted regardless of file identity) and M11 (a stale settle overwriting a newer one).
- **Stale packs.** m2.
- **Stale dispatch lists.** B1 (readers after a reboot), B2 (dead-looking leases), M4 (marker drift) — all three produce a wrong `ready` answer.

---

## 8. Correctness gates

Each row: gate name; pass criterion; scale; mandatory from. "New" marks a gate or a criterion this audit adds; the others restate [60 §3.13] with sharpened criteria.

| Gate | Pass criterion | Scale | Mandatory from |
|---|---|---|---|
| GT1 crash enumeration (full subsets) — new criterion | 0 lost acknowledged durable records of any kind; every in-flight group applied or absent as a whole | all subsets for ≤ 12 unflushed sectors per file, ≥ 10⁴ random beyond, cross-file products; ≥ 10⁵ crash states | M0 (toy log), M1 exit; nightly |
| GT1 seeded protocol bugs | 100 % caught: the 12 of [60] plus trust a lazily published `committed_lsn`; a reader serving a pre-crash view; a single-slot `HEAD` barrier; a commit adopted without its group's markers; a skipped non-commit record; a rename without a namespace barrier | toy log (M0), real engine (M1) | M0 exit, M1 exit |
| Post-crash read freshness — new | 100 % of reads by processes started after a crash reflect every acknowledged durable record before any writer runs | every GT1/GT3 crash state; every GT15 cycle | M1 |
| `HEAD` barrier states — new | 0 opens of a deleted file across all 9 slot states at every GC, retirement and recycling point | every barrier point | M1 |
| GT3 multi-process simulation | 0 lost acknowledged records; reads monotonic per process; model agreement at each read's `seq` | ≥ 10⁶ steps at the M1 exit; ≥ 10⁷ per night for RG3 | M1 |
| GT4 kill loop (all durable effects) | 0 lost acknowledged commits, leases, ref moves, marker effects, intent outcomes; 0 corrupt opens; `doctor --verify` clean | 16 processes, 10⁴ nodes, 10,000 iterations per hour × 4 variants | M1, extended at M3/M5/M6/M8/M10 |
| GT15 OS-crash loop (extended oracle) | 0 lost acknowledged durable effects; `FsIntent` = file-system truth; image `git fsck --strict` clean and `gitmap` ⊆ destination; last backup restores; lazy-heavy phase before each crash | ≥ 1,000 cycles at the M1 exit; ≥ 5,000 cumulative for RG3 | M1; oracle extended at M5/M6 |
| Rig calibration (data and namespace) | an unflushed write, rename or delete is lost ≥ 1 in 100 power-offs; a barrier-protected one never | ≥ 100 hard power-offs | M0 |
| GT2 differential incl. runtime tables | 0 disagreements in exit class, data, commit ids, `state(ref)`, markers, leases, absorbed vectors | ≥ 10⁶ commands per milestone; ≥ 10⁷ for RG4 | M1 onward |
| I26′ state oracle — new | engine `ready`/`claim` = exclusion defined from branch states across the 10 doors plus undo and `op restore` in both directions, fork/`-D`, staging abort, `TX` coalescing | ≥ 10⁶ histories, ≥ 5 refs | M0 (model), M3 exit |
| Sync residue equivalence — new | `view(lane)` after sync = model `state_at(sync)` = image tree, incl. one-sided composition and re-keys | ≥ 10⁵ generated cases with `path_moves` and globs | M3, M5 |
| uid → `#N` uniqueness — new | every uid binds to exactly one `#N` over the store's life; derived uids on two lanes share it | GT2 FL-3 generator | M2 |
| GT8 image gates 0–3, both anchor-text modes | gate 0 for every kind; gate 1 byte-identical; gate 3 two-store identical; 100 % native re-verification in `hash-only` | 10⁵ nodes / 10⁵ commits, SHA-1 and SHA-256 | M5 |
| Import onto a diverged ref — new | local commits never lost or re-parented; foreign ids identical across stores; result = model merge | GT8 fixtures plus a generator | M5 |
| Settle concurrency — new | 0 observation regressions; writer-byte hold p99 ≤ 5 ms incl. settle-bearing verbs | GT3 with the `ProjectFs` simulator, 16 processes | M6 |
| Lease liveness — new | CLI-taken leases survive CLI exit until TTL or run end; Unknown never expires; reboot releases non-run leases; ±1 h clock steps change nothing | GT4 variants plus a second restricted principal | M2 (semantics), M8, M10 |
| R4 P1 no wrong automatic re-bind (extended generators) | precision 1.0 in the exact class, incl. promote-replace and delete + re-create + directory move | ≥ 10⁶ simulated operations; matrix rows 1–33 on NTFS | M0 (FL-1 corpora), M6, M8 |
| R4 link-state truth — new | 0 `ok`/`moved-auto` where ground truth places the file elsewhere or deleted | GT17 plus the simulator | M6 |
| P11 anchors vs brute force | 100 % equal or more conservative | fixtures plus fuzzing | M0, M6 |
| LQ differential | 0 disagreements in rows, order, multiplicity; `TX` state and diff equal | ≥ 10⁶ queries and `TX` blocks, all view kinds | M7 |
| LQ `TX` target integrity — new | 0 lost updates; 0 applies outside the `DRY` target set under `IF TARGETS` | GT3 with `TX` writers | M7 |
| LQ-Bench (GT13) | confident-wrong ≤ 2 % on reads, ≤ 5 % per construct, 0 on writes; ≥ 85 % first try, ≥ 95 % after one retry | 520 prompts | M0, M7, M8, M10, per change |
| Verb = named mutation | every CLI write verb's commit = its `TX` expansion's canonical changeset (incl. `complete`) | every verb × every argv form | M7, M8 |
| GT7 git oracle (extended corpus) | 100 % agreement; `unknown` (never `false`) across shallow boundaries | owner repositories, 46 HEADs, shallow/partial clones, replace refs, split commit-graph | M4 |
| GT16 mutation testing | ≥ 90 % killed in the semantic crates; 100 % of seeded protocol bugs | per milestone exit | M0 onward |
| GT5 fuzzing | 0 crashes or hangs; ≥ 7 CPU-days per fuzzer | continuous | per component; RG5 |
| `doctor --verify` | clean after every GT2 case, every kill-loop run and every OS-crash cycle | all | M1 onward |
| Invariant coverage — new | every invariant of [AR §3.4] and [AR §5e.8] has an enforcement point, a model function and at least one gate (§3 of this audit) | 100 % | M0 specification review; every exit |
| Stale-pack notice — new | `complete`/`apply` list every critical rule, owner ruling and blocker changed since the pack digest | GT10 fixtures | M9 |
| Crash-before-ack retry — new | a retry without a key after an adopted write never duplicates within the default key window | GT4 | M8 |

---

## 9. Consequential edits

| Document | Section | Edit (finding) |
|---|---|---|
| [AR] | §4.2 `HeadSlot` | `durable_lsn`, `boot_id`, flag bit `retired` (B1, m6) |
| [AR] | §4.3 `RecHdr` | flags bit1 `group_end`; validity includes `lsn` = scan position (M1) |
| [AR] | §4.5 step 2, §4.10 | re-write from `durable_lsn`; adopt every record kind and whole groups; boot-change recovery (B1, M1) |
| [AR] | §4.1, §4.9 | two-slot barrier; directory flush after renames and deletes; refuse `\\wsl$` (M2, M8, W-m) |
| [AR] | §4.4, §4.8 | `ALLOC` widened with uid; uid → `#N` section (M7); lease row: holder anchor and `boot_id` (B2) |
| [AR] | §5a.3, §4.6 | sync residue = keys whose merged value differs from the window (M5) |
| [AR] | §5a.5, §5a.9, §5d.1, §3.4 I26′ | state-defined I26′; marker recompute on `undo`/`op restore`; re-attribution on `-D`; scoped `cleared`; no staging markers (M4) |
| [AR] | §5b.6 | export durability order; divergent import through `import/<ref>` plus a typed merge (M9, M10) |
| [AR] | §6.2 | lease liveness by anchor; three-valued liveness; reboot rule (B2) |
| [AR] | §7.1 | `check` as a write verb; default idempotency keys; outcome-unknown message (m8, m5) |
| [AR] | §7.4 | pack digest (m2) |
| [AR] | §8.2, §9 | GT rows of §8 of this audit |
| [40] | §3.4, §3.5 | `MOVEFILE_WRITE_THROUGH`; directory flush; anchor-based intent liveness (M8, B2) |
| [40] | §4.2 | `complete`'s settle as a separate CAS-guarded commit; CAS on every settle (M11) |
| [40] | §4.3, §4.4 | E3d file-identity check; present-path file-id check at settle (M13) |
| [40] | §5.7, §2.11 R-10 | anchor-text digests in the canonical props; `text-unavailable` sub-state (M6) |
| [40] | §8.3 | rows 32–33; OS-crash semantics in the `ProjectFs` simulator (M13, M8) |
| [50] | §3.10 | `IF TARGETS`; markers from net ops; `complete` purity note (m1, M4, M11) |
| [50] | §4.4 | canonical-AST algorithm frozen per grammar version (m3) |
| [60] | §2.5 | decisions (a) and (c) restated; fault-model item (3) wording per [17]; new reservations (B1, B2, M1, M2, M6, M7) |
| [60] | §3.13 | GT1 enumeration; GT4 and GT15 oracles; new gates (M3, M12) |
| [60] | §4.2 | model: I26′ by state; markers and leases compared after recovery (M4, M1) |

---

## 10. Checked and found sound (not findings)

- Readers bounded by `committed_lsn` during normal operation.
- The ref move inside the commit record (N3).
- Idempotency evaluated after republish (F-B7).
- The blocking overlapped lock wait (G1).
- The recursive virtual base and sync-first merges into `main`.
- CM2's canonical sync diff (apart from M5), CB1's carrier table, CB2's unhashed side ref, CM4's tombstone out-edges, CM3's body encoding.
- Per-pair staging (CM5).
- `TX` targets and guards evaluated inside the lock; `DRY` outside it; the 5·10⁵-unit cap.
- "Reads never write" enforced by executor type.
- E302 at past views; portable named-query text.
- Merges never read the file system or git.
- R4's re-key-never-resurrect rule, the freshness rule, the designated writer tree, the quiescence re-check, the copy rule.
- LQ-Bench's confident-wrong gate on writes.
- The reference model's separation rules (except M4).

---

## Sources

- [AR], [40], [50], [60] and the critiques [21], [41], [51], [61], with their Review logs.
- Research [02 §9], [05], [08], [09], [10], [11], [13], [17 §0, §3.5, §3.6], [18 §0, §8], [20 §0, §7].
- [D, MS] Microsoft Learn, `MoveFileExW` (`MOVEFILE_WRITE_THROUGH`: "The function does not return until the file is actually moved on the disk"): https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-movefileexw

*End of 72-audit-correctness.md.*
