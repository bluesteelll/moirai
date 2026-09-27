# 60 — Engine-first roadmap: building moirai with no third-party database

> **SUPERSEDED — historical record.** This first draft is superseded by [60-roadmap.md](60-roadmap.md), the roadmap of record, and by [docs/ARCHITECTURE-RESEARCH.md](../../ARCHITECTURE-RESEARCH.md) §9; its claim to be "the roadmap of record for the build order" no longer holds. Its allowance of redb and heed/LMDB as optional out-of-tree benchmark reference points (binding consequence 3, §0, §4.4 and the edit-list rows of §7) is **withdrawn** by the owner's rule that no third-party embedded database is built, linked or run anywhere, not even as a benchmark ([AR §9], `AGENTS.md`; [60-roadmap.md](60-roadmap.md) deleted the same allowance). The text below is kept unchanged.

*moirai research/design, 2026-09-26. Status: research only; nothing is implemented. Trigger: the owner decision of 2026-09-26 (verbatim, translated): "No, we do not use SQLite — we build our own [engine] right away." It rejects the recommended default of owner decision #2 in [AR §11]. That default ran S1–S5 on a throw-away SQLite backend behind the engine trait and landed the from-scratch engine at S6. The decision therefore replaces the oracle-first plan of [AR §9] and [22 §7.3]. This file is the roadmap of record for the build order. [AR] is to be amended by the edit list in §7; this file does not edit it.*

**Binding consequences.** These come from the decision and constrain everything below.
1. No SQLite and no other third-party embedded database (redb, LMDB/heed, fjall, sled, RocksDB, Turso, …) appears anywhere in the product path at any milestone. That includes temporary backends and stepping stones.
2. The differential and property-test oracle is a naive in-memory **Rust reference model inside the project** (§3). It is never SQLite.
3. redb and heed/LMDB may appear only as optional, **out-of-tree** benchmark reference points (§4.4). They are never dependencies. SQLite is dropped from the benchmark plan.
4. Historical citations of SQLite's design stay as citations: WAL semantics, the WAL-reset race, the session extension and the rebaser idea. Only *usage* of SQLite is removed.

**Owner preferences respected throughout:** maximum performance, minimal RAM, zero idle CPU, Windows 11 first. Research only; no code.

**Sources.**

| Tag | Document |
|---|---|
| [AR] | `docs/ARCHITECTURE-RESEARCH.md`, the design of record |
| [22] | critique: agent fit and buildability (§2.7, §7.1–§7.3, §11 decision 7) |
| [20] | critique: performance, RAM and Windows (G1–G28, F-A1–F-D13, U-labels) |
| [05] | report: storage, performance and RAM (§2, §6, §7, §14, §16–§17) |
| [08] | report: concurrency, sync and git interop (§3, §4, §8–§10) |
| [A], [D] | the proposals' build plans (§9) |
| [10], [12] | R4 research |
| [14], [15], [16] | R5 research |
| [40], [50] | the R4 and R5 designs, written in parallel |

[40] and [50] are **not** relied on here; every R4/R5 item below is a marked placeholder (§2.14).

**Tags.** **[M]** measured on the owner's machine (quoted from [05]/[08]); **est.** arithmetic with its inputs shown. Everything else is a design decision.

---

## 0. Summary

- **The engine moves onto the critical path, built in layers.** E1 is a durable core that runs a deliberately **serialized-access protocol**: every moirai process, reader or writer, holds `LOCK` byte 0 for the length of one command. That protocol is safe to adopt. The design-of-record lock-free protocol ([AR §4.5, §6.1]) is switched on at **E4**, after its simulation and kill-loop gates pass. Both stages use **the same on-disk format**.
- **Milestones E0–E7 replace S0–S6:**

  | Milestone | Content | Adoption / requirement met |
  |---|---|---|
  | E0 | contract, measurements, harnesses, reference model | — |
  | E1 | durable core (serialized) | — |
  | E2 | graph core + CLI on `main` | **A1: first adoption, as the task tracker**; R2 met |
  | E3 | packs, brief, hooks, skill, import | **A2: the memory-system gate**, i.e. the old S2 gate |
  | E4 | lock-free protocol + full hardening | — |
  | E5 | branches + merge | **R1** |
  | E6 | git image | **R3** |
  | E7 | MCP | — |

- **Test workstream.** The simulator, crash enumeration, kill loops, fuzzers and the reference model run in parallel from E0. Each gate has a milestone from which it is mandatory (§2.13).
- **The oracle is a Rust reference model.** It is BTreeMap-based, recomputes all derived state from scratch, builds branch states by replay from genesis, and merges from materialised states. It deliberately uses *different algorithms* from the engine and shares no code with it (§3).
- **Benchmarks are judged against absolute budgets and the machine's physical floor**, not against SQLite. For example, a durable commit must cost at most the measured flush p50 plus 0.5 ms. redb and heed may be run once, out of tree, for information only (§4).
- **Calendar (sequential)** — 6.5–10 weeks to the first adoption, 8.5–13 to the memory-system gate, 14.5–22 to R1, 16.5–25 to R3 and 17.5–28 to MCP. The old plan reached the same points on SQLite at 5–7, 9–13, 11–16 and 12–19 weeks, and ran on its own engine only at 16–25. **If E4 overlaps E3** (two lanes of agents), R1 lands at 12.5–19 and the total drops to 15.5–25, the same as the old total (§5).
- **Risks.** Three grow: first adoption comes later, engine defects reach the owner's live store earlier, and schedule risk concentrates on the engine. Mitigations are the serialized stage, crash gates before any real data, frozen log and canonical formats with rebuildable segments, daily verified backups, a text export, a node cap for A1 and a pre-agreed cut list. Three shrink or disappear: the S6 swap risk, double implementations, and extrapolated performance numbers (§6).
- **§7 is an exact edit list for [AR].** It gives 42 numbered required edits with replacement text (every anchor verified to occur exactly once in the current file), the full §9 replacement block, recommended consistency edits for the legacy M-labels, and a list of matches that must *not* be touched.

---

## 1. What changes and why; the new critical path

**What changes and why.** The oracle-first plan ([22 §2.7, §7.3]; [AR §2.13, §9]) exists for one reason: to adopt packs and briefs before a from-scratch engine exists. It did so by putting a throw-away SQLite backend behind an engine trait. The owner has ruled out any third-party database at any stage, so that escape hatch is gone. Engine-first would normally mean "~60 % of the work precedes the first adoption gate" [22 §2.7]. This roadmap avoids that by **layering the engine**:

- The smallest durable core that is safe for real data comes first (E1). It uses serialized access, which removes the reader/writer/checkpoint race class where SQLite's WAL-reset bug hid for 16 years [08 §3.1].
- The graph semantics and the CLI go on top (E2). This is the first adoption.
- Packs, briefs and hooks follow (E3). This is the memory-system adoption.
- Only then does the store switch to lock-free readers, once the full deterministic simulation and 10,000-iteration kill loops pass (E4). Branches, the image and MCP follow.

Things that make this safe:

- **The format is frozen before the first byte is written.** The log record format, the commit body, the canonical commit form and the `HEAD` slot are fixed at E0. Segments, blobs and `hist` files are derived from the log and can be rebuilt. So no adopted store ever needs a history migration.
- **The reference model makes every layer testable in isolation** (§3).
- **The two old "engine trait" roles survive in new forms:**
  - The logical `Store` API is the typed commands-and-results interface. The engine implements it, and so does the reference model (test-only).
  - The `Vfs` trait (file, lock, flush, map, clock) is implemented by Windows, by an in-memory simulator and later by Unix.
  - There is **no storage-backend trait and no second backend**.
- **What does not change.** The architecture T1–T16, the data model, the format, the budgets, the agent surface, R1–R3 semantics and the owner decisions other than #2. The only [AR] positions that move are listed in §7: T13 (build order), T10's oracle clause, T2's staging, §8.2's baselines, §9, the §10 risk rows and §11 #2.

**New critical path.**

```
E0 contract · measurements · harnesses · reference model v0
 └─► E1 durable core, serialized access (log, HEAD, LOCK, segments, overlay, blobs, backup, repair)
      └─► E2 graph core + CLI on main ═══════════ A1: task tracker adopted (R2 met)
           ├─► E3 packs · brief · hooks · skill · import ═══ A2: memory-system gate
           └─► E4 lock-free protocol + full DST + 10k kill loops   (E3 ∥ E4 when two lanes are available)
                └─► E5 branches + typed merge ═══ R1   (needs E3 and E4)
                     └─► E6 git image, checkpoint granularity ═══ R3
                          └─► E7 MCP (needs E3, E4, E5)
test/simulation workstream (Vfs simulator, crash enumeration, reference model, fuzzers, kill loops) ═══ parallel from E0
```

---

## 2. Milestones E0–E7

### 2.1 Overview

Sizes use the old plan's scale: S ≈ 1 week, M ≈ 2–3 weeks, L ≈ 4–6 weeks of the owner plus agents (est.). The weekly rate is also the old plan's own ratio. S0–S6 without the deferred items is ≈ 127–137 units (A = 100): [AR §9]'s 150–160, minus the ≈ 23 deferred units of [22 §7.1]. Spread over 16–25 weeks, that is ≈ 5–8 units per week.

| Milestone | Replaces | Size | Weeks | Depends on | Delivers |
|---|---|---|---|---|---|
| **E0** Contract, measurements, harnesses | S0, minus the SQLite backend | M− | 1.5–2 | — | frozen format v1, `Store` API, `Vfs` + simulator, reference model v0, E0 measurements, hook experiment |
| **E1** Durable core, serialized access | the storage half of S6 | L | 3–5 | E0 | log/HEAD/LOCK, segments, overlay, blobs, checkpoints, `backup`/`restore`/`repair`/`doctor` |
| **E2** Graph core + CLI on `main` | S1 | M | 2–3 | E1 | **A1** task tracker; R2 |
| **E3** Packs, brief, hooks, skill, import | S2 | M | 2–3 | E2 | **A2** memory-system gate |
| **E4** Lock-free protocol + hardening | the protocol half of S6 | M | 2–3 | E2 (may overlap E3) | design-of-record protocol, certified |
| **E5** Branches + merge | S3 + the branch half of S6 | L | 4–6 | E3, E4 | **R1** |
| **E6** Git image, checkpoint granularity | S4 | M | 2–3 | E5 | **R3** |
| **E7** MCP | S5 | S–M | 1–3 | E3, E4, E5 | Bash-less roles write directly |

### 2.2 The enabling idea: two protocol stages on one format

**Stage S — serialized access (E1–E3).** Every moirai process takes `LOCK` byte 0 for one CLI command or one hook invocation. Before E7 there is no long-lived process. The lock is acquired with the G1 blocking wait: an overlapped `LockFileEx`, then `WaitForSingleObject(2 s)`, then `CancelIoEx` on timeout, followed by exit 7 naming the holder. While holding it, the process:

1. reads `HEAD` (by `pread` only);
2. runs the recovery scan in G2 order: from `committed_lsn` to the first bad record, adopt each complete record, re-flush, republish, and only then evaluate idempotency;
3. replays the tail;
4. executes the command;
5. commits with one `DATA_SYNC_ONLY` flush and publishes `HEAD`;
6. if the tail threshold is exceeded, writes the delta checkpoint **before releasing**.

Byte 2 (maintenance) is unused in this stage.

*Consequences:*
- Readers never observe an unflushed record, and never race a writer, a checkpoint or a GC. The whole reader/writer/checkpoint interleaving space that [AR §8.2]'s multi-process simulation exists to explore is empty.
- The cost is that reads queue behind writes. Per-command hold times are ≈ 0.1–1 ms for reads and 2–5 ms for writes at ≤ 5e4 nodes, plus a 5–50 ms checkpoint once per ~4,096 ops (est., from [AR §8.1]).
- A 16-process burst therefore finishes in ≈ 16 × 2.5–5 ms ≈ 40–80 ms, plus at most one checkpoint (est.). That is far inside the 2 s bound.
- Idle CPU is still zero, and private RSS is unchanged.

**Stage L — lock-free (E4 on).** This is exactly [AR §4.5/§6.1]:
- readers take no lock and stop at `committed_lsn`;
- the writer holds byte 0 only for its own commit;
- delta checkpoints run under byte 2 and take byte 0 for microseconds to publish (G9).

**One format, one switch.**
- `HEAD.flags` bit 3 (`lockfree`) records the stage. It is reserved in the E0 spec.
- Binaries built before E4 refuse a store with the bit set (exit 7, "upgrade moirai").
- Binaries from E4 on read the bit and run either stage.
- `moirai migrate --protocol lockfree` sets the bit while holding bytes 0 and 2. `--protocol serialized` is the rollback.
- The owner runs one binary from a stable install path, so an upgrade switches every process at once. No long-lived process exists before E7.

**Frozen versus rebuildable.**
- Frozen at E0: the log record header and kinds, the commit body ([AR §4.3]), the canonical form ([AR §4.6]), the `HEAD` slot ([AR §4.2]) and the `LOCK` byte map.
- Rebuildable: segments, blob files and `hist` frames are derived from the log. History is retained by default ([AR §4.9]), and bodies pass through the log tail (G7). A segment-format defect found in use before E4 therefore costs a `repair --rebuild-from-log`, not a migration of history.
- Branch-ready from the start: E1 already writes `prev_on_ref`, `ref_seq` and the per-ref lsn lists in `Checkpoint` records for `main`. History written before E5 is therefore complete for the per-ref index (G15).

### 2.3 E0 — Contract, measurements, harnesses (M−, 1.5–2 weeks)

**Scope.**
- **On-disk format spec v1.** Frozen:
  - the 32-B record header (LSN, epoch, xxh3);
  - every record kind, including the branch-era ones: `RefUpdate`, `Pin`, `ClientHead`, `Marker` (settled/deleted/cleared), `GitMap`, and `Checkpoint` with per-ref lsn lists;
  - the commit body, including `ref_old`, `prev_on_ref`, `ref_seq` and absorbed vectors;
  - the canonical form;
  - the `HEAD` slot with the G18 table pointers and the `lockfree` bit;
  - the `LOCK` byte map, with diagnostics at offset 2048.

  Specified but rebuildable: the segment sections, including `REFS`/`PINS`/`HEADS`/`MARKERS`. Golden byte fixtures exist for every record kind.
- **R4-0 (placeholder):** a reserved value-type tag and edge-property slot for file references. Their final shape comes from [40].
- **The logical `Store` API:** typed commands (every CLI verb's semantic core), typed results in the `--json v1` data shape, `state(ref)` snapshots and a deterministic clock.
- **The `Vfs` trait and its in-memory implementation**, with injection points for crashes at every write, flush and publish, fsync errors, lock-release delays and sharing violations (errors 5/32). A Windows implementation skeleton.
- **Reference model v0** (§3): trunk semantics, meaning kinds, fields, edges, delete policies, derived state, status machines, leases, idempotency and the change feed.
- **Bench harness** recording p50/p90/p99, private bytes and peak working set via `GetProcessMemoryInfo`, flush counts and page faults; spawn-to-exit via `hyperfine -N --warmup 5`.
- **E0 measurements 1–13** (§4.1).
- **The 5-minute hook experiment** (W-all-2), exec-form PATH resolution (W-all-3), and English and Cyrillic token ratios (W-all-6).

**Exit criteria (owner's machine, Defender on, idle and under typical agent load).**
- All 13 measurements are recorded.
- The thresholds they decide are fixed: the lock bound, the checkpoint thresholds, the loose/pack threshold, the G15/G16 thresholds and the A1 node cap (a first estimate, re-measured at E1).
- The spec has been reviewed, with golden fixtures for every record kind.
- The reference model passes its fixture suite: the delete-policy matrix, the status machines and the single-branch rows of [AR §5d.3] (node 40).
- A toy log survives crash enumeration over the in-memory `Vfs`.

**Test gates.** Model fixtures; the crash enumerator runs on a toy log.

**Size basis.** The old S0 was S (≈ 1 week), including a throw-away SQLite backend. Dropping the backend saves ≈ 0.3–0.5 weeks. Adding the full frozen spec with fixtures, the `Vfs` trait with its simulator (≈ 1–2 units) and model v0 (≈ 2 units, ≈ 1–1.5k lines) costs more. Net: 1.5–2 weeks.

**Depends on:** nothing. **Adoptable:** no.

### 2.4 E1 — Durable core, serialized access (L, 3–5 weeks)

**Scope.**
- **Windows `Vfs`:**
  - `LockFileEx` on `LOCK` byte 0 with the G1 wait;
  - `NtFlushBuffersFileEx(FLUSH_FLAGS_FILE_DATA_SYNC_ONLY)`;
  - read-only `CreateFileMappingW`/`MapViewOfFile`;
  - `FILE_SHARE_READ|WRITE|DELETE`;
  - bounded retries on errors 5 and 32;
  - refusal of network and OneDrive paths.
- **Log:** 64 MiB extents, zero-filled with one full flush (G11); append plus one data-only flush per durable commit; the recovery scan (G2 order); the epoch, re-rolled on `init`/`restore`/`repair` (G25).
- **`HEAD`:** two slots, `pread` only (F-B1), published without a flush (1PC+C).
- **Segments:** base plus up to 3 delta segments with a tiered fold. The sections are:
  - node columns (60-B header, cold columns);
  - titles and field blocks;
  - forward and reverse CSR plus edge properties;
  - hand-written frozen bitsets;
  - `SYMTAB`, `TOMB`, `IDEM`, `LEASES`, `MARKERS`;
  - `REFS`/`PINS`/`HEADS` holding `main` only.
- **Overlay** built from the tail.
- **Bodies:** held in the log tail with their own 32 MiB threshold (G7) and sealed into `blobs.NNNN` at checkpoint. They are content-addressed (BLAKE3-128) and zstd-compressed, with the dictionary only if E0 measures a ratio ≥ 2×; the dictionary id is in the format either way. Blob files are mapped lazily.
- **Checkpoints and GC:** a delta checkpoint at 4,096 ops or 4 MiB, run inside byte 0 in stage S; `gc` rollup only on explicit request; delete-pending GC with a 60 s grace; an orphan sweep (G14); the quiet flag with the G10 cap.
- **Tools:**
  - `init --store` (full discovery arrives in E2);
  - `doctor --fsck|--verify` at storage level;
  - `backup` (holds byte 0 for the copy, ≤ 0.2 s at the A1 cap, est.) and `restore` (CM8);
  - `repair --rebuild-from-log`;
  - `gc`;
  - `quiet on|off`.

**Not in E1:** lock-free readers, the maintenance byte and `hist` retirement (all E4); pins beyond `backup`, `ClientHead` and branch overlays (E5); promotion (v1.1). The first 64 MiB extent lasts ≈ 100–200 days at 0.3–0.6 MB of log per day (est., [20 §0] inputs), so `hist` retirement is not needed before E4.

**Exit criteria (owner's machine, Defender on, idle and under load).**
- A durable commit costs p50 ≤ E0 flush floor p50 + 0.5 ms and p99 ≤ floor p99 + 1 ms, with **exactly one** flush per durable commit (counted).
- Open costs ≤ 1.5 ms at 1e5 and ≤ 3 ms at 1e6 with a full 4,096-op tail, warm. Bytes read on open are equal at 1e4 and 1e6, apart from the tail.
- CLI private RSS is ≤ 4 MB at 1e5.
- A delta checkpoint inside byte 0 costs ≤ 30 ms at 1e4 and ≤ 50 ms at 5e4. This fixes the A1 node cap.
- Recovery after a kill with a full tail takes ≤ 10 ms.
- `backup` → `restore` into an empty directory → `doctor --verify` is clean.
- `repair --rebuild-from-log` reproduces byte-identical segments, because the segment writer is deterministic.
- The stage-S wait with 16 processes (50/50 reads and writes) is recorded.

**Test gates (mandatory from here; §2.13):**
- GT1: crash-point enumeration at every write/flush/publish boundary, including between the two appends of one flushed group (N3), with fsync-error injection. Zero lost acknowledged commits and zero corrupt opens over ≥ 1e5 enumerated crash states.
- GT4: record, `HEAD` and segment parser fuzzers run clean for 24 h.
- A storage-level differential against the model's storage view (rows, edges, bodies, runtime tables) over 1e6 random storage operations.
- A kill-loop smoke test: 16 processes × 200 iterations.

**Size basis.** ≈ 17–20 of the 25 engine-core units of [22 §7.1]; the rest arrive in E4 and E5. At 5–8 units/week that is 2.1–4 weeks. Add 0.5–1 week for first-code overhead (workspace, Windows CI, bench integration) and for adoption-grade tooling (`backup`/`restore`/`repair`/`doctor`), which the old plan kept in S1. Total: 3–5 weeks.

**Depends on:** E0. **Adoptable:** no; there are no graph verbs yet.

### 2.5 E2 — Trunk graph + CLI on `main` (M, 2–3 weeks) — **A1: first adoption (task tracker); R2 met**

**Scope.** The old S1 minus `backup`/`restore`, which moved to E1.

- **Data model:** 13 kinds + `phase_state`/`return_to` + `gates`; schema as data; eager derived state for touched nodes; Pearce–Kelly over I5′ including implied exogenous edges; delete policies with X4 flagging and `--replaced-by` (I32′); the `suspect` closure budget; the `mentions` sigil rule; runtime leases with fencing tokens (I17′), run-scoped and store-wide and carrying their branch; idempotency bound to the payload and branch, kept 30 days (I14′); the change feed; `settled` markers written by ops (CB3), whose absorbed vectors are trivial on `main`.
- **Write verbs:** `add`, `set`, `link/unlink`, `move`, `doc patch`, `rm --dry-run/--yes`, and `apply` with `$refs`.
- **Read verbs:** `ready`, `blocking`, `blockers --explain`, `show`, `tree`, `find`, `changes`.
- **Coordination and analysis verbs:** `claim`, `complete`, `reopen`, `stats loop/refuted-share`, `lane conflicts`, `notes --path`.
- **`check`/`stale`:** ancestry from the commit-graph, else one cached `git merge-base` spawn per pair (CM7).
- **Store discovery and placement:** the full discovery chain with `<git-common-dir>/moirai/` placement, the `init` guard (D4), `--link`, `worktree bind`, and `doctor store|agents` (CM6).
- **Output contract** frozen: `--json v1`, exit codes 0–9.
- **`--branch`/`--lease` are accepted on every verb.** A ref other than `main` gets exit 2: "branches arrive with E5".
- **`export md --to DIR`:** a human-readable, engine-independent text copy, and a risk mitigation (§6).
- **R5-a slot** (placeholder, §2.14): the query core's minimal slice, with the read verbs above defined as named queries.

**Exit criteria (owner's machine).**
- `blocking --ids` ≤ 100 µs at 1e5 on the engine.
- Engine time ≤ 5 ms per CLI command at 1e5 on `main`; `get` ≤ 5 µs; a `ready` page ≤ 300 µs at 1e5 and ≤ 3 ms at 1e6.
- Incremental derived state equals the reference model under 1e6 random ops (GT2), and equals `doctor --verify` recomputation at 1e5 and 1e6.
- Every verb is documented with an example.
- `init` run in the main checkout and `ready` run from a `<lanes-dir>/*` worktree find the same store with no pointer file.
- `check` answers correctly for a commit made seconds ago.
- **Stage-S wait p99 ≤ 250 ms with 16 concurrent CLI processes.** If this fails, E4 moves ahead of E3 (§6.2).
- GT3 (serialized kill loop, 16 processes × 1,000 iterations) is clean.

**Test gates:** GT2 (differential against the model); golden outputs; PowerShell 5.1 argv tests; the delete-policy matrix; GT3; GT10 fixtures (the node-40 table on `main`).

**Size basis.** Graph semantics (15 units) plus the CLI share ≈ 2.5–3 weeks at 5–8 units/week. That is the same M as the old S1: S1 on SQLite had cheaper queries, but E2 no longer carries `backup`/`restore`.

**Depends on:** E1. **Adoption A1:** see §2.12.

### 2.6 E3 — Packs, brief, hooks, skill, import (M, 2–3 weeks) — **A2: memory-system gate (the old S2 gate)**

**Scope.** The old S2, verbatim:
- C's pack algorithm with the [AR §7.4] header, character budgets, the `*` default, the global critical-rule count, and the `~main` slot (empty until E5);
- N15 rendering;
- `brief`;
- the SessionStart, UserPromptSubmit, SubagentStart/Stop and agent-launched hooks (fail-open);
- `export memory-md`;
- the core skill, `moirai-report` and `moirai-orchestrate`;
- a one-off import of the dozen standing rules, current pins as measurements, live lanes and open owner questions.

Plus the **R4-a slot** (placeholder, §2.14): packs, `notes --path` and `files_owned` resolve file references, and explicit file verbs.

**Exit criteria.**
- The old S2 criteria:
  - one real campaign runs on `main` with no HDR and no hand-written resume block;
  - the dispatcher pattern is the only Workflow pattern;
  - the orchestrator judges an HDR-vs-pack diff complete;
  - hook output is ≤ 8,000 chars with exact drop footers.
- Hook wall time p99 ≤ 1 s, including spawn, under a 16-agent SubagentStart burst in stage S.
- **Fail-open verified:** a hook facing a held lock returns empty output with exit 0 inside its timeout.
- Pack/brief engine time ≤ 8 ms at 1e5.

**Test gates:** pack budget tests; hook payload fixtures against Claude Code 2.1.28x; pack candidate-class sets equal the model's (§3.2); GT10 pack fixtures.

**Size basis.** The old S2 (M), unchanged. It touches no engine code, which is why it can run in parallel with E4.

**Depends on:** E2. **Adoption A2:** see §2.12.

### 2.7 E4 — Lock-free protocol + hardening (M, 2–3 weeks; may overlap E3)

**Scope.**
- **Stage L** ([AR §4.5, §6.1]):
  - readers take no locks and stop at `committed_lsn`;
  - delta checkpoints run under the maintenance byte and re-take byte 0 only to publish (G9);
  - the G10 quiet cap works in this stage;
  - `hist` retirement uses zstd frames of 256 commits with a per-frame commit index (G3), and the active extent's `(id16 → lsn)` lives in the overlay (F-B2);
  - delete-pending GC is safe with long-lived mappings;
  - `backup` switches to a pinned segment set and no longer holds the writer byte.
- `moirai migrate --protocol lockfree|serialized`.
- Lock-hold instrumentation: histograms per byte.
- Integration of the full simulation and kill-loop suites, which the test workstream built from E0.

**Exit criteria (owner's machine).**
- **GT5:** multi-process simulation, 1e6 steps, with crash, fsync-error, lock-release-delay and AV sharing-violation injection, passes.
- **GT6:** 16 writer and reader processes, `TerminateProcess` at random points, 10,000 iterations / 1 h. Zero lost acknowledged commits, zero corrupt opens, and `doctor --verify` clean after every run.
- Writer-wait p99 ≤ 50 ms with 16 writers (G1).
- Reader latency p99 during a 16-writer burst ≤ 2× its idle value.
- Writer-byte hold p99 ≤ 5 ms, which asserts that no checkpoint runs inside byte 0.
- 0 % CPU over 10 minutes idle for every moirai process.
- A 24-h soak on a copy of the owner's live store.
- **Only then** is the owner's store switched with `migrate --protocol lockfree`.

**Test gates:** GT5 and GT6 (mandatory from here); an AV-interference test (a process holds files open without `FILE_SHARE_DELETE`); a lock-release-delay test using the E0-13 numbers.

**Size basis.** ≈ 5 engine-core units (≈ 1 week), plus 1–2 weeks of gate runs and fixes. "95 % of the effort is testing" [04 §3.14] is folklore, but it points the right way. The simulator, fault injection and kill-loop harness are built by the parallel workstream, so E4 integrates them rather than building them.

**Depends on:** E2. It may overlap E3 (disjoint code). It is **mandatory before E5 and E7**. **Adoptable:** yes, as an upgrade of A2 (§2.12).

### 2.8 E5 — Branches + merge for the next campaign's lanes (L, 4–6 weeks) — **R1 met**

**Scope.** The old S3:
- refs (log-folded tables), reflog and `ClientHead`;
- `branch`/`checkout`/`--list`/`-d`/`tag`/`undo --expect`/`revert`/`cherry-pick`/`log --graph`/`diff A...B`/`show@`/`blame`/`at`;
- `lane open/close`;
- the typed three-way merge with the base at the LCA, sync-first merges into `main` and the single-LCA rule;
- per-pair `merge/<dst>/from/<src>` staging (CM5), `resolve`, `merge --continue`, `sync --check`/`sync`, `merge-check`;
- `--across`;
- op-level `settled`/`deleted`/`cleared` markers and absorbed vectors (CB3, CM1);
- the segment-walk fold (CM9);
- `revert --mainline 1` (CL8);
- `apply` taking its branch from the run;
- `plan/*`;
- the `moirai-branches` skill;
- SubagentStart `sync --check` gating.

Plus **the engine's branch parts from the old S6**:
- pins refcounted per segment file (G18);
- the per-ref index used by overlay builds (G15);
- a streaming branch overlay builder with a fixed buffer;
- `ClientHead` with `session:<id>` expiry (G27).

Plus the **R4-b and R5-b slots** (placeholders).

**Exit criteria (owner's machine).**
- The old S3 criteria:
  - the owner's two register incidents replay correctly;
  - a lane completes a task through each of the ten doors, and `main` never re-dispatches it, while `reopen`/`undo` make it dispatchable again (I26′);
  - `ready` with 50 stale markers stays within budget;
  - two lanes with staged syncs do not block a third;
  - 10 synthetic lanes × 1k ops merge deterministically, including sync-then-merge, criss-cross, branch-of-branch and move+inheritance;
  - a merge costs ≤ 50 ms at 1e5;
  - a `plan/*` branch cannot mark work done.
- **Engine additions:**
  - the first read of a lane forked 1k/7k/14k/60k commits ago costs 1–3 / 5–10 / 10–20 ms (G28);
  - 50 branches × 2k ops, forked 1k–60k commits apart, are readable within budget (the old S6 criterion);
  - ≤ 10 ms on a lane including the first-read overlay build at a 14-day fork (CI);
  - GT6 passes with 16 directories bound to different branches;
  - `doctor --verify` passes on every branch head, meaning pin ⊕ ops equals the model's replay from genesis.

**Test gates:** GT7 (branch/merge properties and fuzzing against the model: I25′, I26′ through every door, I31′, I37′, CM1/CM5/CM9 shapes, the merge variant of X1); GT6 across branches; GT10 incident fixtures and the [AR §7.6] walk-through as a scripted fixture.

**Size basis.** The old S3 was L (VCS 14 + merge 14 units ≈ 4.5 weeks). The engine branch parts add ≈ 3 units. That is offset by dropping the SQL implementation of branch views (pin ⊕ ops expressed in SQL) that S3 needed on the oracle backend. Net: unchanged at 4–6 weeks.

**Depends on:** E4 (stage L is certified before branch records join the protocol) and E3 (the hooks that gate `sync --check`). **R1 guarantees:** see §2.12.

### 2.9 E6 — Git image at checkpoint granularity (M, 2–3 weeks) — **R3 met**

**Scope.** The old S4, unchanged:
- the `.moi` ABNF and golden fixtures (CL1);
- the encoder and decoder with self-check;
- the tree layout;
- the full trailer set (CB1);
- two-parent `sync` commits (CM2);
- byte-exact bodies (CM3);
- tombstones with their edges (CM4);
- `gitmap`;
- the unhashed side ref (CB2);
- `ImageBackend` over `git fast-import`/`cat-file`;
- export of `main` + `tags/*` + `lane/*` to a separate bare repo;
- the import path (native verification, foreign commits, `Undelete`, `incr` ledgers, two-parent foreign merges, `ImageParse` staging, import-checkpoint, markers written by imported ops);
- `image doctor --rebuild-map` and `image show`;
- the post-merge export hook.

Plus the **R4-c slot** (placeholder): `.moi` encoding of file references.

**Exit criteria.** The old S4 criteria:
- gate 0 (hash reconstruction for every commit kind) first;
- `commit` granularity byte-identical and `checkpoint` granularity state-identical for 1e5 nodes and 1e5 commits;
- `git fsck` clean;
- the failure-case list behaves as specified;
- a full export of 1e5 takes ≤ 3 s;
- export private RSS ≤ 15 MB at 1e5 (G24).

**Test gates:** GT8 (image gates 0–3, the `.moi` fuzzer, differential tests against `git fsck`/`log`/`cat-file`); the state-identical check compares against the model's `state(ref)`.

**Size basis.** The old S4 (M), unchanged. **Depends on:** E5.

### 2.10 E7 — MCP (S–M, 1–3 weeks)

**Scope.** The old S5:
- `moirai mcp` on rmcp, dual-era, on a `current_thread` runtime (G12);
- ten tools with an explicit `branch` parameter validated against the `lease`;
- the stamp hook on write tools only (G6);
- the role policy keyed on the dispatch label;
- plugin packaging.

**Plus from the old S6:**
- the LRU of ≤ 8 branch overlays (G19);
- rollup run by the MCP server only after serving a request, and only when deltas exceed 25 % of base.

Plus the **R5-c slot** (placeholder): query access through a read-only tool.

**Exit criteria.**
- Architect and critic complete a round on a lane branch without Bash.
- Schema ≤ 5k chars.
- MCP private RSS ≤ 10 MB + 1 MB × min(active branches, 8) at 1e5.
- 0 % CPU over 10 minutes idle.
- An MCP server held open through a GT6 kill loop never serves a record past `committed_lsn`.

**Test gates:** GT9 (conformance tests for both handshakes, a `structuredContent` regression test, RSS and idle checks).

**Size basis.** The old S5 (S–M); G19 and after-request rollup add ≈ 0.5 weeks, inside the range. **Depends on:** E3, E4 and E5.

### 2.11 Later (v1.1+)

Unchanged from [AR §9]:
- branch promotion (`seg.b*`, `TOUCH`);
- a hand-written loose/pack/idx/bundle layer (or `gix` if the owner allows it);
- `commit`-granularity export by default;
- SHA-256 images;
- `rebase --onto`, `op restore`, `--with-oplog`;
- tracked-directory and orphan-branch destinations;
- a recursive virtual merge base;
- the leader, `watch` and `PostToolBatch` push (M6);
- FTS tier 2 and schema strengthening;
- the `shared` field class;
- an HTTP MCP mode and a signed installer.

Also later: the R4-d and R5-d slots (§2.14).

### 2.12 What each adoptable stage guarantees, and what it must not be used for yet

| Stage | Guarantees | Must not be used for yet |
|---|---|---|
| **A1 — tracker on `main`** (E2 exit) | Every acknowledged write is durable: one data-only flush before the acknowledgement. A killed process never corrupts the store; the next lock holder adopts complete records and ignores a torn tail. Readers see only flushed, published commits, monotonic by `seq`. `ready`/`blocking`/rollups equal a full recomputation (reference model + `doctor --verify`). Retries with an idempotency key never duplicate. Waits are bounded at 2 s, then exit 7 naming the holder. `backup`/`restore` and `repair --rebuild-from-log` are verified. The log and canonical formats are frozen, so no history migration will ever be needed. Idle CPU is zero; CLI private RSS is ≤ 4 MB. | Branches (only `main`; other refs get exit 2). Long-lived processes (no MCP). Graphs above the A1 node cap (≈ 5e4 nodes, est., re-measured at E1, because checkpoints run inside byte 0). Stores on network or OneDrive paths (refused). Being the **only** copy of anything irreplaceable: run the daily `backup` and keep `export md`. Replacing the HDR/MEMORY.md resume block: that is A2's job, so keep both during the first campaign. Direct writes from Bash-less roles: the orchestrator's `apply` carries their output. Hook wiring (it arrives in E3). |
| **A2 — memory system on `main`** (E3 exit) | Everything in A1, plus: packs and briefs never truncate silently and keep owner rulings at ≥ L1; hook output is ≤ 8,000 chars; hooks fail open, so a held lock yields an empty brief rather than a blocked agent; hook p99 ≤ 1 s under a 16-agent burst. | Lanes as branches. Bash-less roles writing directly. The node cap still applies until E4. Sustained write bursts well beyond 16 agents: waits grow linearly in stage S (measured at E2/E3). |
| **H — hardened protocol** (E4 exit; A2 upgraded in place) | Everything in A2, plus: lock-free readers, so reads never wait for writers; writer hold ≤ 5 ms p99; checkpoints outside the writer byte; certified by 1e6-step simulation and 10,000-iteration kill loops; `hist` retirement; no node cap (the 1e6 budgets of [AR §8.1] apply). | Branches and MCP (not built yet). |
| **R1 — lanes as branches** (E5 exit) | Everything in H, plus R1 as specified in [AR §5a] and §5d: nothing completed or deleted on one live branch is re-dispatched from another; staged merges never advance a ref; kill loops pass across branches. | Lanes above ~8k ops without promotion (v1.1). `rebase`/`op restore` (deferred). Cross-machine writers. |
| **R3 — image** (E6 exit) | Deterministic export and import at checkpoint granularity through `git fast-import`/`cat-file`, as in [AR §5b.7]. | Machines without git (v1.1). SHA-256 images pushed to remotes. Remotes not listed in `config.image.allowed-remotes` (decision #16). |
| **MCP** (E7 exit) | The three Bash-less roles write directly, within the RSS gate. | — |

### 2.13 Test and simulation workstream: when each gate becomes mandatory

The workstream runs in parallel from E0, as before, at ≈ 20 % of the build ([22 §2.8]) plus the reference model (≈ 6 units, §3.6). A gate that is mandatory runs in CI on every PR from that milestone on, with short seeds, and nightly with long seeds. A gate failure blocks the milestone's exit and every later milestone.

| Gate | What | Built during | Mandatory from | Runs |
|---|---|---|---|---|
| **GT1** Crash-point enumeration | One process at a time over the in-memory `Vfs`. A crash at every write/flush/publish boundary, including between the two appends of a flushed group (N3); fsync-error injection; restart and recovery; the model checks acknowledged-commit semantics (§3.4). | E0–E1 | **E1 exit** | CI + nightly |
| **GT2** Differential vs the reference model | Seeded op sequences against engine and model; outputs and `state(ref)` compared (§3.4). Storage-level at E1, semantic from E2. | E0 on | **E2 exit** (storage level at E1) | CI + nightly |
| **GT3** Stage-S Windows kill loop | 16 processes, mixed reads and writes, `TerminateProcess` at random points, 1,000 iterations (~10 min); zero lost acknowledged commits, zero corrupt opens, `doctor --verify` clean. | E1 | **E2 exit** (before any real data) | nightly until E4 |
| **GT4** Parser fuzzers | Log records, `HEAD`, segments (E1); `.moi` generated from the ABNF (E6); the pack reader (v1.1). | E1 on | **E1 exit** (records), **E6** (`.moi`) | continuous |
| **GT5** Multi-process simulation | N simulated processes under seeded scheduling in stage L: lock-free readers, checkpoints under byte 2, GC with mappings; crash, fsync-error, lock-release-delay and AV sharing-violation injection; 1e6 steps. | E0–E4 | **E4 exit**, and before any store is switched to stage L | nightly; 24-h soak before each adoption upgrade |
| **GT6** Full Windows kill loop | 16 writer and reader processes in 16 directories, 10,000 iterations / 1 h; `doctor --verify` on every head; a `backup` taken mid-loop restores every commit acknowledged before its `committed_lsn` (CM8). | E4 | **E4 exit** (`main`); **E5 exit** (directories bound to different branches) | nightly |
| **GT7** Branch/merge properties + fuzz | I25′, I26′ via all ten doors, I31′, I37′; CM1/CM5/CM9 shapes; criss-cross; the merge variant of X1; `undo` versus absorbed vectors — all against the model. | E3–E5 (merge rules can start against the model during E3/E4) | **E5 exit** | CI + nightly |
| **GT8** Image gates | [AR §5b.7] gates 0–3; two stores export identical objects; git differential tests. | E6 | **E6 exit** | CI |
| **GT9** MCP conformance | Both handshakes; `structuredContent`; RSS; idle CPU. | E7 | **E7 exit** | CI |
| **GT10** Human-verified fixtures | The [AR §5d.3] node-40 table (E2 on `main`, E5 across branches); pack fixtures (E3); the owner's register incidents and the [AR §7.6] walk-through (E5); a git-side merge of counter increments (E6). Expected outputs are written by a person, not by the model. | E0 on | the milestone that builds each feature | CI |
| **GT11** Budget gates | §4.3. | E1 on | per row of §4.3 | CI (Windows, Defender on, idle and under a synthetic 16-agent load) |

### 2.14 R4 and R5 plug-in slots (placeholders)

These slots name *where* the file-link design [40] (R4) and the query-language design [50] (R5) attach. Their content, sizes and gates are **placeholders** until those designs exist. The sizes are rough and **not included** in §5's totals.

| Slot | Milestone | Placeholder content (to be replaced by [40]/[50]) | Rough size (est.) |
|---|---|---|---|
| **R4-0** | E0 | Reserve a value-type tag and an edge-property slot for file references in the frozen format, so stores adopted at A1 never migrate for R4. Keep machine-local fingerprint parts (NTFS file id, volume serial, mtime; [10 §0]) out of anything that is hashed or exported. | in E0 |
| **R4-a** | E3 | File references in packs, `notes --path`, `files_owned` and `artifact` nodes. Explicit `moirai file add\|mv\|rm` verbs that update every reference in one commit. Lazy re-binding on read (stat → file id → oid), with no watcher and no daemon ([10], [12]). | +1–2 weeks |
| **R4-b** | E5 | Per-worktree resolution for lanes: a reference resolves inside the lane's bound worktree. File-reference fields merge by the typed rules. | +0.5 weeks |
| **R4-c** | E6 | `.moi` encoding of file references. Only store-independent fields are hashed: repo-relative path, content oid and anchors. | +0.5 weeks |
| **R4-d** | later | Similarity-based re-binding at scale, and replaying git's per-commit renames. | later |
| **R5-a** | E2 | The query core's minimal slice: parser → AST → logical plan → interpreted executor over the engine's read API (columns, CSR, bitsets, overlay), with budgets and `EXPLAIN` ([15]). The CLI read verbs (`ready`, `blocking`, `blockers`, `find`, `tree`) become **named queries** over it, and their `--json v1` shapes are the result shapes. Gate: every named query's results equal the model's predicate on the GT2 corpus. | +1–2 weeks |
| **R5-b** | E5 | Versioned queries: branch-qualified, `AT <commit>`, `--across`, diff and history, and conflicts as queryable data ([16]). | +0.5–1 week |
| **R5-c** | E7 | Query access from MCP: a read-only tool with budgets. | +0.5 weeks |
| **R5-d** | later | Planner optimisations; in-language mutations, if [50] keeps them ([16]). | later |

---

## 3. The reference model (oracle)

### 3.1 Role

The model is a small, obviously-correct, **executable specification** of moirai's logical semantics:
- it lives in the `moirai-model` crate (`publish = false`);
- it is a dev-dependency of the test crates only;
- it is **never linked into the binary** and never a backend.

It implements the same logical `Store` API as the engine. It replaces SQLite in both roles the oracle-first plan gave SQLite: correctness reference and conservative semantics.

The model is not a performance reference (§4 is) and not a multi-process reference. Concurrency correctness comes from GT1/GT3/GT5/GT6, which check the engine against the model's *acknowledged-commit* semantics.

### 3.2 Scope: what it models, and the independent algorithm it uses

The core rule: wherever the engine uses an optimised or incremental mechanism, the model uses the **definitional** algorithm from [AR]. A disagreement is then either an engine bug or a spec ambiguity, and both are findings.

| Concern | Engine | Reference model |
|---|---|---|
| Current state | base + delta segments + overlay | `BTreeMap<#N, Node>` per state, built by folding net changesets from genesis |
| Derived state (`open_blockers`, `ready`, `is_blocker`, rollups, `suspect`, `answered`, `conflicted`) | maintained eagerly for touched nodes; frozen bitsets | **recomputed from scratch on every query**, each predicate a literal transcription of [AR §3.5] |
| Precedence acyclicity (I5′) | Pearce–Kelly, with full Kahn above 1,000 touched edges | DFS over the whole combined graph, with implied exogenous edges re-derived by definition |
| Branch view | `SEG(pin) ⊕ ops(main,(P,fork]) ⊕ own ops`, sync by reference (G17) | `state_at(tip)` by replaying the commit DAG from the root, memoised by commit id |
| Marker absorption (I26′) | absorbed vectors, O(1) (CM1) | exhaustive: the marker's commit is an ancestor-or-self of `tip(R)`, by DFS; `cleared` markers per [AR §5d.1] |
| ahead/behind `main` | `ref_seq` arithmetic | size of the ancestor-set difference |
| LCA | gen-pruned bidirectional walk | full ancestor sets; maximal common ancestors; newest-by-gen rule (I31′) |
| Merge base per key | reverse-apply of dst's per-node chain | read the key from the materialised LCA state |
| Merge / sync | segment-walk folds; typed rules; staged violations | three materialised states; per-key diff; the [AR §5a.7] rule table transcribed **as data**; validators by recomputation in I37′ order; `sync` is just a merge of `main` into the lane |
| revert / cherry-pick | stored before-images | diff of `state_at(parent)` and `state_at(c)` |
| undo | `RefUpdate` + absorbed-vector restore | move the ref pointer; emit `cleared` markers per the rules |
| Canonical op list ([AR §4.6]) | writer coalescing; full state diff for a `sync` | `diff(state_at(first parent), state_at(c))` as a sorted key→value list (catches the CM2 class of bug) |
| Leases, fencing, idempotency, change feed, `next_id` | `LEASES`/`IDEM` sections, `HEAD.fence`, seq ring | `BTreeMap`s, counters and a `Vec` of feed entries |
| Packs | class quotas, degradation, rendering | **candidate-class membership only** (C1–C8 sets for a target, role and branch); no rendering |
| Image (E6) | `.moi` codec, trees, trailers | provides `state(ref)` for the state-identical gate and the canonical diff for gate 0; no bytes |
| Queries (R5, placeholder) | planned, interpreted executor | a naive nested-loop evaluator over the materialised state |

### 3.3 Out of scope

The model does not cover:
- bytes, the on-disk format, checksums, epochs and LSNs;
- locks, timing and processes;
- segments, overlays, pins, checkpoints, GC, promotion and `hist`;
- performance and RAM;
- CLI text rendering (the harness compares `--json v1` data);
- `.moi` bytes;
- R4 file-system resolution (it stores file references as data only);
- the git subprocess.

Each of these is covered elsewhere: golden fixtures, fuzzers, GT1/GT5/GT6, GT8 and `doctor --verify`.

### 3.4 How differential tests run

1. **Generator.** A seeded, weighted mix of `Store` API commands for each milestone. It includes:
   - valid and invalid commands, so refusals and exit codes are tested;
   - several simulated clients with branch bindings (from E5);
   - retries with idempotency keys;
   - in simulated runs, crash and restart events.

   A deterministic clock injected through the `Vfs` makes HLCs, and therefore commit ids, deterministic.
2. **Lock-step execution.** Each command runs against the engine (in-memory `Vfs`, or real files on Windows in nightly runs) and against the model. The harness compares:
   - the exit-code class;
   - the result data (`--json v1` `data`, excluding engine-internal fields such as `lsn`);
   - after every command, a digest of `state(ref)` for every touched ref, with a full structural comparison on a mismatch and every k commands.
3. **Crash semantics.** For a commit that is in flight when a crash is injected, the model holds two candidate states: applied and not applied. After recovery the engine must equal one of them. A retry with the same idempotency key must converge (I14′, I27′). An acknowledged commit may never be missing.
4. **Engine self-check as a third opinion.** At the end of each case, `doctor --verify` recomputes every derived structure inside the engine. Engine, model and self-check must all agree.
5. **Shrinking and corpus.** Failing seeds shrink to minimal sequences and are stored as regression fixtures. The GT10 human-verified fixtures run in the same harness.
6. **Scale split.** The model runs cases of ≤ 1e4 commands and ≤ 2e3 nodes, which is fast. The 1e5–1e6 scales are checked by the engine's self-check and by the budget gates, never by the model.

### 3.5 Guarding against a shared misunderstanding

SQLite was an independent implementation. The model is written from the same document by the same team. The mitigations:
- **different algorithms** (§3.2);
- **no shared code** with the engine: only the schema-as-data rows are shared, as input;
- **human-verified fixtures** (GT10);
- every model–engine disagreement is triaged as a *spec* finding before either side is changed.

### 3.6 How it stays small

- **Budget:** ≤ 1.5k lines at E0, ≤ 3.5k at E5, ≤ 4.5k at E7 (est.). No `unsafe`; `std` only, plus `blake3` for body hashes.
- **Asymptotics are not a concern.** O(history) per query is acceptable.
- **One function per [AR] predicate, invariant or rule**, each citing its section in a comment. Rules are **data tables**: merge rules, status machines, delete policies.
- **Model first, per feature.** A feature enters the model before or with the engine, and a feature without a model counterpart cannot pass its gate. This keeps the model the executable spec.
- **Growth review at every milestone exit.** If the model exceeds its budget, the model is simplified (for example, memoisation is removed), not the spec.
- **Effort** ≈ 6 units over the whole build: v0 ≈ 2 (E0), + 1 (E2), + 2–3 (E5), + 0.5 (E6), + R5 predicates (placeholder).

---

## 4. Benchmark and measurement plan (replaces the SQLite rows)

### 4.1 E0 measurements on the owner's machine

Conditions: Defender on; idle and under typical agent load; p50/p90/p99, private bytes and peak working set, flush counts, page faults; `hyperfine -N --warmup 5`.

Items 1–10 are [AR §8.2] unchanged:
1. Open → append → data-only flush → close on a 64 MiB log file, n = 200 (G8).
2. Blocking `LockFileEx` contention with 16 writer processes (G1).
3. Overlay build versus branch age (G28). It is re-run at E5 on the real engine.
4. `sync` bytes per lane per day (G28).
5. Pinned file count with 50 branches (G28).
6. zstd dictionary ratio on real notes; English and Cyrillic token ratios.
7. `git` on PATH, exec-form hook PATH resolution, and the hook experiment.
8. Loose-object create + rename cost, and a full `git gc` on a 1e5-commit image repo.
9. `zlib-rs` versus `miniz_oxide`.
10. Tail replay of a full 4,096-op / 4 MiB tail.

Items 11–13 are new:

11. **Stage-S contention probe.** 16 processes each take byte 0 with the G1 wait and hold it for a synthetic 2 / 5 / 10 / 30 ms, including one data-only flush. Record wait p50/p99, last acknowledgement, and reader wait behind writers. This decides the provisional A1 guarantee and whether E4 must precede E3. It is re-measured with the real engine at E1 and E2.
12. **Physical floor.**
    - Data-only flush on a zero-filled extent (overwrite) versus append (G11).
    - Open + map of 8 sealed files.
    - `pread` of both `HEAD` slots.
    - Spawn-to-exit of an empty Rust executable from the stable install path, directly and through the agent's Git-Bash wrapper.

    These are the baselines the engine gates are stated against (§4.2).
13. **Lock-release delay after `TerminateProcess` of a holder:** p50/p99/max, idle and under load (W2). This checks the 2 s bound and parameterises the lock-delay injection in GT5 and the timing of the kill loops.

### 4.2 Floor-relative gates instead of "beat SQLite"

The old §8.2 said the engine "must beat [SQLite/redb/heed] on open time and private RSS and match them on commit latency". That is replaced by **absolute budgets** ([AR §8.1]) and **distance from the physical floor** measured in item 12:
- durable commit p50 ≤ floor flush p50 + 0.5 ms, and p99 ≤ floor p99 + 1 ms;
- exactly one flush per durable commit;
- open ≤ 1.5 ms at 1e5 and ≤ 3 ms at 1e6;
- CLI private RSS ≤ 4 MB at 1e5;
- engine time ≤ 5 ms per CLI command at 1e5.

These are stricter than "match SQLite". SQLite needs two syncs per commit in `synchronous=FULL` WAL mode, per the ≈ 4 ms two-fsync arithmetic of [08 §4], and caches ~2 MB per connection [05 §4.1].

### 4.3 Budget gates by milestone (each mandatory from the milestone named)

| Budget ([AR §8.1] unless marked new) | Target | Mandatory from |
|---|---|---|
| Durable commit vs floor; flushes per commit | ≤ floor p50 + 0.5 ms; exactly 1 | E1 |
| Open at 1e5 / 1e6; bytes read on open independent of N | ≤ 1.5 / ≤ 3 ms | E1 |
| Private RSS, CLI or hook process | ≤ 4 MB at 1e5 | E1 |
| Delta checkpoint inside byte 0, stage S (new) | ≤ 30 ms at 1e4, ≤ 50 ms at 5e4 (sets the A1 node cap) | E1 |
| Recovery after a kill with a full tail (new) | ≤ 10 ms | E1 |
| Engine per CLI command on `main`; `get`; `ready`/`blocking` page | ≤ 5 ms; ≤ 5 µs; ≤ 300 µs at 1e5 | E2 |
| Stage-S wait p99, 16 concurrent CLI processes (new) | ≤ 250 ms (if it fails, E4 moves ahead of E3) | E2 (retired at E4) |
| `pack`/`brief` engine time; hook output; hook wall p99 under a 16-agent burst (wall time new) | ≤ 8 ms at 1e5; ≤ 8,000 chars; ≤ 1 s | E3 |
| Writer-wait p99 with 16 writers; reader p99 during a writer burst (new); writer-byte hold p99 (new) | ≤ 50 ms; ≤ 2× idle; ≤ 5 ms | E4 |
| Idle CPU, every moirai process, 10 min | 0 % | E4 (E7 for the MCP server) |
| Lane read including the first-read overlay at a 14-day fork; merge of a 2k-op lane; `ready` with 50 stale markers; staged syncs | ≤ 10 ms; ≤ 50 ms at 1e5; same budget as with none; two staged syncs do not block a third | E5 |
| Full export of 1e5; round-trip gates 0–3; export RSS | ≤ 3 s; byte- and state-identical; ≤ 15 MB at 1e5 | E6 |
| MCP private RSS | ≤ 10 MB + 1 MB × min(active branches, 8) at 1e5 | E7 |

### 4.4 Optional out-of-tree reference points

redb 4.x and heed/LMDB may be run **once at E1 exit and once at E4 exit** on workloads (a), (e), (f, commit only) and (h) of [AR §8.2]:
- in a separate throw-away benchmark crate **outside the moirai workspace and its `Cargo.lock`**;
- with the same record layout, on the owner's machine;
- with the numbers recorded for information in the benchmark log.

They are never a gate, never a dependency, never in CI and never a fallback. **SQLite is not benchmarked.** If the owner prefers, this step can be skipped without affecting any gate.

### 4.5 Removed from the plan

- "SQLite (rusqlite, WAL, `synchronous=FULL`) as the conservative multi-process reference and the S0–S5 throw-away backend".
- "The from-scratch engine must beat them … if it cannot at S6, the trait keeps the product working".
- "`blocking --ids` ≤ 100 µs at 1e5 on the oracle": now measured on the engine at E2.
- "Swapped in when it beats the oracle on open time and private RSS": the S6 swap no longer exists.

---

## 5. Calendar

**Basis.**
- Sizes are summed sequentially, on the old plan's scale (S ≈ 1, M ≈ 2–3, L ≈ 4–6 weeks of the owner plus agents).
- The test workstream runs in parallel, as in the old plan.
- The conversion is ≈ 5–8 units/week of [22 §7.1] (§2.1).
- Per-milestone bases are in §2.3–§2.10.
- R4/R5 slots are excluded on both sides of the comparison (§2.14).

| Event | Old plan: date on SQLite | Old plan: date on its own engine | **New, sequential** | **New, E3 ∥ E4** |
|---|---|---|---|---|
| First adoption of anything real | 5–7 (S2, memory system) | 16–25 (after the S6 swap) | **6.5–10** (A1, tracker) | 6.5–10 |
| Memory-system gate (old S2 gate) | 5–7 | 16–25 | **8.5–13** (A2) | 8.5–13 |
| Design-of-record multi-process protocol certified | — | 16–25 (S6) | **10.5–16** (E4) | 8.5–13 |
| **R1** (branches + merge) | 9–13 (S3) | 16–25 | **14.5–22** (E5) | 12.5–19 |
| **R3** (git image) | 11–16 (S4) | 16–25 | **16.5–25** (E6) | 14.5–22 |
| **MCP** | 12–19 (S5) | 16–25 | **17.5–28** (E7) | 15.5–25 |
| **Total v1** | 16–25 | 16–25 | **17.5–28** | **15.5–25** |

Cumulative arithmetic (sequential):

| After | Weeks |
|---|---|
| E0 | 1.5–2 |
| E1 | 4.5–7 |
| E2 | 6.5–10 |
| E3 | 8.5–13 |
| E4 | 10.5–16 |
| E5 | 14.5–22 |
| E6 | 16.5–25 |
| E7 | 17.5–28 |

With the overlap, E3 and E4 both start at E2's end and both end at 8.5–13; every later date moves 2–3 weeks earlier.

**Effort.** ≈ 125–140 units (A = 100, est.) for E0–E7, the same as the old S0–S6 (≈ 127–137) within the estimate's error:
- the throw-away backend and the SQL implementation of trunk and branch views disappear (−6 to −10 units, est.);
- the reference model (+6) and layering (+2–3: stage S first, then stage L, on one format) are added.

Roughly 26–32k lines of Rust plus 12–16k of tests (est.), of which the model is ≈ 3–4.5k.

**Reading the comparison.**
- **The total is unchanged** with the overlap, and 1.5–3 weeks longer without it. The extra time comes from splitting the engine into E1 + E4 (5–8 weeks, against 4–6 for S6) and from a larger E0. The savings are real but smaller than one size step, so they do not show in S/M/L sums.
- **First adoption is 1.5–3 weeks later** (A1 against S2's date), and **the memory-system gate is 3.5–6 weeks later**.
- **R1, R3 and MCP are 3.5–9 weeks later** than the old plan's SQLite dates. Against the old plan's own-engine date (16–25), **R1 is 1.5–6 weeks earlier**, and R3 and MCP land at about the same time: within −3 to +3 weeks depending on the overlap. In the old plan, everything before 16–25 weeks ran on a backend that was deleted at S6.
- **Every milestone now runs on the final engine.** There is no swap event.

The dependency "E5 before the next multi-lane campaign" is more likely to be missed than S3 was. The fallback is unchanged: that campaign runs on `main` with `files_owned`, `lane conflicts` and store-wide leases, and the following campaign gets branches.

---

## 6. Risk deltas

### 6.1 Risks that grow, shrink or disappear

| Risk | Direction | Why | Mitigation | Signal |
|---|---|---|---|---|
| **First adoption later** | grows | A1 comes at 6.5–10 weeks (tracker) and A2 at 8.5–13, against 5–7 for the memory system on SQLite (§5). | A1 delivers the tracker and the owner's headline queries (`blocking --ids`, subtasks, blockers) before packs. E3 does not wait for E4. The E3 ∥ E4 overlap. The orchestrator prepares the E3 import content (standing rules, pins, lanes, open questions) as text files during E1. The HDR stays in use until A2. | A1/A2 dates against §5 |
| **Engine defects reach the owner's live store earlier** | grows | The from-scratch engine holds real data from E2, 8–15 weeks earlier than S6 would have. Segment writers, recovery and bitset maintenance are classic sources of early bugs [08 §8.1]. | Stage S removes the reader/writer/checkpoint race class. GT1 (crash enumeration), GT2 (differential) and GT3 (1,000-iteration kill loop) must pass before any real data. Log and canonical formats are frozen at E0, and segments/blobs/`hist` are derived, so `repair --rebuild-from-log` recovers from a segment-writer defect. Daily `backup` with a verified `restore`, plus the backup-age warning ([AR] risk 13). `export md` as an engine-independent text copy. The A1 node cap. The HDR/MEMORY.md resume block kept through the first campaign. `doctor --verify` run by the nightly window driver (never as a background job). | `doctor --verify` findings; any use of `repair`; kill-loop failures |
| **Schedule risk concentrated on the engine** ([AR] risk 5) | grows (high / high) | The engine is now on the critical path to every milestone. Nothing is swappable any more. | E1 is deliberately minimal (no lock-free readers, `hist` or promotion). The format is the contract. The pre-agreed cut list (§6.2). Typed merge rules are written as pure functions (states → merged state + conflicts) and tested against the model during E3/E4, so E5 starts with them done. They are never a backend and never shipped as one. | E1 exit after week 7; E4 exit after week 16 |
| **R1/R3/MCP later than the SQLite dates** | grows | 3.5–9 weeks later (§5); the next campaign is more likely to open before E5. | The overlap; the model-driven merge rules; the unchanged fallback (the campaign runs on `main` with `files_owned`, leases and `lane conflicts`). | E5 start date against the campaign plan |
| **Shared misunderstanding between model and engine** (new) | grows | The independent third-party oracle is gone. | §3.5: different algorithms, no shared code, GT10 human-verified fixtures, `doctor --verify` as a third opinion, disagreements triaged as spec findings. | model/engine disagreement rate |
| **Stage-S contention degrades the workflow before E4** (new) | new, bounded | Reads queue behind writes; hook latency grows with bursts. | The measured gates at E2/E3 (wait ≤ 250 ms, hook ≤ 1 s); fail-open hooks; a trigger that moves E4 ahead of E3 (§6.2). | E0-11 and E2/E3 numbers |
| **A format flaw found in use after A1** (new) | new, bounded | The format is frozen before real use. | Only the log records, commit body, canonical form and `HEAD` slot are frozen. Segments stay rebuildable until and after E4. Anything else goes through an explicit `migrate` ([AR §12]). The E0 spec already carries every branch-era record kind, so E5 needs no bump. | spec change requests |
| **Stage switch to lock-free is a discrete event** (new) | new, bounded | A bug that appears only in stage L would hit a store that ran safely in stage S. | GT5 and GT6 plus a 24-h soak on a copy of the live store before the switch. `migrate --protocol serialized` rolls back. A backup is taken immediately before the switch. | soak results |
| **Multi-process protocol loses an acknowledged write** ([AR] risk 1) | shrinks at adoption | Adopted stores run in stage S until the full simulation passes. | As above; the stage-L gates are unchanged from the old S6 gate. | GT5/GT6 |
| **Swap risk at S6** (behaviour of SQLite and the engine differing late; branch views implemented twice) | **disappears** | There is no second backend. | — | — |
| **Performance/RAM extrapolated from SQLite** | shrinks | Real engine numbers exist from E1, with no per-connection SQLite cache. | Floor-relative gates (§4.2). | GT11 |

### 6.2 Pre-agreed cuts and triggers

**If E1 exits after week 7**, apply these in order:
1. zstd dictionary → plain zstd (the format keeps dictionary id 0);
2. tiered delta fold → fold only in `gc`;
3. push `hist` retirement to right after E5. It must land before the first 64 MiB extent is 75 % full; `doctor` warns at that point. This buys ≈ 100–200 days (est.).

**If E4 exits after week 16:** keep A2 in stage S. Branches still wait for E4.

**Trigger for moving E4 ahead of E3:** the E2 stage-S wait p99 exceeds 250 ms with 16 CLI processes, or the E0-11 probe predicts a hook p99 above 1 s. Moving E4 first delays A2 by 2–3 weeks and does not change R1.

**Trigger for moving E7 ahead of E6:** the Bash-less roles become the owner's bottleneck. There is no dependency conflict: MCP needs E3, E4 and E5. R3 then moves 1–3 weeks later.

---

## 7. Edit list for `docs/ARCHITECTURE-RESEARCH.md`

Line numbers refer to the file as of 2026-09-26 (1,558 lines). Every anchor is an exact substring of the current text. "→" gives the replacement. The edits are ordered by position in the file. Apply §7.1 in full. §7.3 is recommended for consistency. §7.4 lists matches that must **not** be changed. Do not use blind find-and-replace: "S1", "S2", "S4", "S6" and "S12" also appear as [21] finding labels, [D §12] source labels and store variables.

### 7.1 Required edits

| # | Where (line) | Anchor (current text) | Replacement |
|---|---|---|---|
| 1 | header (3) | `It supersedes the synthesis and proposals A–D as the design of record; where it changes them, it says so.*` | `It supersedes the synthesis and proposals A–D as the design of record; where it changes them, it says so. Amended on 2026-09-26 by the owner's engine-first decision (no SQLite or other third-party database at any milestone): §9 now follows [60], and every passage that relied on an oracle backend was rewritten (Review log, last entry).*` |
| 2 | sources table (after the [30] row, 27) | *(new row)* | `\| [60] \| [research/design/60-engine-first-roadmap.md](research/design/60-engine-first-roadmap.md) \| engine-first roadmap (E0–E7), reference-model spec, benchmark plan and calendar adopted after the owner rejected the SQLite oracle backend (2026-09-26) \|` |
| 3 | §0 bullet 10 (52) | the whole bullet, from `10. **Build order (T13):** on-disk format spec and measurements first;` to `…Windows kill loops as its exit gate.` | `10. **Build order (T13, engine first — owner decision 2026-09-26):** no SQLite or other third-party database at any stage. Format spec v1, measurements, the in-memory \`Vfs\` and the Rust reference model first (E0); the from-scratch durable core in a serialized-access mode (E1); graph core + CLI adopted on \`main\` as the task tracker (E2) and packs/briefs/hooks as the memory system (E3); the lock-free protocol, certified by deterministic simulation and Windows kill loops (E4); branches and merge for the next campaign's lanes (E5, R1); the git image at checkpoint granularity (E6, R3); MCP (E7). The test workstream — simulator, kill loops, fuzzers, differential tests against the reference model — runs in parallel from E0 [60].` |
| 4 | §0 "Why this design" (54) | `and the build re-ordered so adoption precedes the engine.` | `and the build re-ordered so that the from-scratch engine is built first, in layers, the first of which is already adoptable on \`main\` (§9, [60]).` |
| 5 | §0 "The biggest risks" (56) | `16-writer kill loops gate the engine milestone; \`moirai backup\`/\`restore\` (S1) and the daily image export` | `16-writer kill loops gate the switch to lock-free readers (E4), while the first adopted layer runs a serialized-access protocol that has no reader/writer/checkpoint races; \`moirai backup\`/\`restore\` (E1) and the daily image export` |
| 6 | §0 "The biggest risks" (56) | `(4) Build size: two long poles (protocol and image determinism); the trait-and-oracle path keeps adoption independent of the engine.` | `(4) Build size and schedule: two long poles (protocol and image determinism), and the engine is on the critical path to every milestone. Mitigation: a layered engine whose first layer (E1, serialized access) is adoptable at E2; a reference model that makes each layer differentially testable; log and canonical formats frozen at E0 with segments rebuildable from the log; a pre-agreed cut list [60 §6].` |
| 7 | §1 row 1 (64) | `no \`petgraph\`, no async runtime in the core (§2 T10).` | `no \`petgraph\`, no async runtime in the core (§2 T10). No third-party database at any milestone, not even as a temporary backend or a test oracle; the differential oracle is the in-project Rust reference model (owner decision 2026-09-26, [60 §3]).` |
| 8 | §2.2 T2 Decision (97) | `…after which one bounded delta checkpoint runs anyway (G10).` | *(append)* `**Protocol stages (engine-first, [60 §2.2]).** The contract above is reached in two steps on one on-disk format. From E1 to E3 every process, reader or writer, holds byte 0 for one command (serialized access: readers never race a writer, a checkpoint or a GC; delta checkpoints run inside byte 0; byte 2 is unused). E4 switches the store to the lock-free protocol above after its simulation and kill-loop gates. The stage is recorded in \`HEAD.flags\` bit 3 (\`lockfree\`), which pre-E4 binaries refuse; \`moirai migrate --protocol serialized\` is the rollback.` |
| 9 | §2.10 T10 Decision (177) | `**Storage engines are allowed as test oracles and as the S0–S5 throw-away backend behind the engine trait** (owner decision #2).` | `**No third-party storage engine appears anywhere in the product path at any milestone — not as a temporary backend, not as a test oracle** (owner decision #2, 2026-09-26). The differential oracle is the in-project Rust reference model (§8.2, [60 §3]); redb 4.x and heed/LMDB may appear only as optional, out-of-tree benchmark reference points, never in the workspace or its lock file; SQLite is not benchmarked.` |
| 10 | §2.10 T10 Decision (177) | `(with \`zlib-rs\` or \`miniz_oxide\`, measured in M0)` | `(with \`zlib-rs\` or \`miniz_oxide\`, measured in E0)` |
| 11 | §2.10 T10 Revisit trigger (183) | `**Revisit trigger.** Owner allows a C engine (LMDB via \`heed\`) as the materialized state → M0 shrinks by a third and T1 becomes Option B. Owner forbids any C code → \`lz4_flex\` at a worse ratio.` | `**Revisit trigger.** None for third-party storage engines (owner decision 2026-09-26). Owner forbids any C code → \`lz4_flex\` at a worse ratio.` |
| 12 | §2.13 T13, whole subsection (207–213) | from `**Decision.** The smallest adoptable slice is S0–S2 of §9` to `…S6 becomes S1's prerequisite.` | The four paragraphs in §7.2 below. |
| 13 | §2.14 Placement (219) | `that is one reason \`backup\`/\`restore\` ship in S1` | `that is one reason \`backup\`/\`restore\` ship in E1` |
| 14 | §2.17 [22] row §2.3 (340) | `Adopted: 5-minute experiment in S0;` | `Adopted: 5-minute experiment in E0;` |
| 15 | §2.17 [22] row §2.5 (342) | `ratios measured in S0.` | `ratios measured in E0.` |
| 16 | §2.17 [22] row §2.7 / D8 (344) | `Adopted: S0–S5 on the oracle backend behind the trait; engine at S6; M1 split.` | `Superseded by owner decision #2 (2026-09-26): engine first, with no third-party backend. The concern is answered by layering: the durable core in serialized-access mode (E1) carries the trunk graph + CLI adoption (E2) and the packs/brief adoption (E3) before the lock-free protocol (E4) and branches (E5); D's M1 is split across E1/E4/E5 [60].` |
| 17 | §2.17 [22] row §2.9 (346) | `open owner questions in S2.` | `open owner questions in E3.` |
| 18 | §4.2 `HeadSlot` (540) | `flags u16 (bit0 quiet, bit1 fts_tier2, bit2 readonly),` | `flags u16 (bit0 quiet, bit1 fts_tier2, bit2 readonly, bit3 lockfree — set at E4; binaries built before E4 refuse the store),` |
| 19 | §4.5, after the paragraph ending `\`apply\` runs steps 5–8 once for a whole batch (one flush).` (644) | *(new paragraph)* | `**Protocol stages ([60 §2.2]).** Until E4 the store runs serialized access: every process, reader or writer, performs step 1 and holds byte 0 for its whole command; steps 2–3 therefore run for readers too, step 10's delta checkpoint runs before byte 0 is released, and byte 2 is unused. From E4 (\`HEAD.flags\` bit 3) the steps above apply as written. The on-disk format is identical in both stages; segments, blob files and \`hist\` frames are derived from the log and can be rebuilt with \`moirai repair --rebuild-from-log\`.` |
| 20 | §4.6 (648) | `(fixed in the S0 format spec)` | `(fixed in the E0 format spec and frozen with the log record format; segment formats remain rebuildable from the log, [60 §2.2])` |
| 21 | §4.7 Read (667) | `no node structs are materialised; readers take no locks.` | `no node structs are materialised; readers take no locks (from E4; before E4 they hold byte 0 for the command, §4.5 protocol stages).` |
| 22 | §4.10 (698) | `\`doctor --fsck\` verifies column checksums and BLAKE3 footers.` | `\`doctor --fsck\` verifies column checksums and BLAKE3 footers. \`repair --rebuild-from-log\` rebuilds every segment, blob and \`hist\` file from the log (all are derived), which is the recovery path for a segment-writer defect (E1, [60 §6]).` |
| 23 | §5b.2 Grammar (906) | `the S4 deliverable includes an ABNF` | `the E6 deliverable includes an ABNF` |
| 24 | §5b.5 rule 8 (958) | `the S4 "reconstruct and re-hash" unit test` | `the E6 "reconstruct and re-hash" unit test` |
| 25 | §5b.6 step 5 (979) | `of the S4 corpus.` | `of the E6 corpus.` |
| 26 | §5b.7 (1001) | `**Gates (S4 exit).**` | `**Gates (E6 exit).**` |
| 27 | §6.1 Writer protocol (1110) | `Readers take no locks; correctness comes from` | `Readers take no locks from E4 on (in the serialized stage E1–E3 every process holds byte 0 for one command, §4.5 protocol stages); correctness comes from` |
| 28 | §7.1 CLI block (1192–1193) | `moirai backup DIR [--force]      moirai restore DIR --into EMPTY_DIR      # transaction-consistent copy of the store; restore re-rolls the epoch (G25)` and `moirai gc [--prune] [--reflog-expire 90d] [--cruft-delay 14d]   moirai quiet on\|off   moirai migrate` | `moirai backup DIR [--force]      moirai restore DIR --into EMPTY_DIR      moirai repair --rebuild-from-log      # backup: transaction-consistent copy of the store; restore and repair re-roll the epoch (G25)` and `moirai gc [--prune] [--reflog-expire 90d] [--cruft-delay 14d]   moirai quiet on\|off   moirai migrate [--protocol lockfree\|serialized]` |
| 29 | §7.4 (1302) and §7.5 (1335) | `is measured in S0 and reported in the footer`; `the 5-minute experiment is the first S0 task` | `is measured in E0 and reported in the footer`; `the 5-minute experiment is the first E0 task` |
| 30 | §8.1 (1359, 1383, 1405) | `(**claimed** ratio, measured in S0)`; `+ Defender close cost **pending S0** (U10)`; end of the CI-gates paragraph `…\`doctor --verify\` clean after every kill-loop run.` | `(**claimed** ratio, measured in E0)`; `+ Defender close cost **pending E0** (U10)`; append to the paragraph: ` Each gate becomes mandatory at the milestone that builds what it measures ([60 §4.3]); until E4 the writer-wait gate is replaced by the serialized-access gate (wait p99 ≤ 250 ms with 16 concurrent CLI processes), and two gates are added: durable commit p50 ≤ the measured flush floor + 0.5 ms (E1), and writer-byte hold p99 ≤ 5 ms (E4).` |
| 31 | §8.2 heading and list (1409, after 1419) | `**S0 measurements on the owner's machine**` | `**E0 measurements on the owner's machine**`. After item 10 add: `11. Serialized-access contention: 16 processes taking byte 0 with the G1 wait and holding it 2/5/10/30 ms including one flush — wait p50/p99, last ack, reader wait behind writers (decides the A1 guarantee and whether E4 precedes E3; re-measured on the engine at E1/E2). 12. Physical floor: data-only flush on a zero-filled extent vs append (G11), open+map of 8 sealed files, \`HEAD\` pread, spawn of an empty Rust executable from the stable install path directly and through Git-Bash — the baselines the engine gates are stated against. 13. Lock-release delay after \`TerminateProcess\` of a holder, p50/p99/max, idle and loaded (W2) — checks the 2 s bound and parameterises lock-delay injection.` |
| 32 | §8.2 "Oracles and baselines" (1421) | the whole paragraph from `**Oracles and baselines.** Before and while the from-scratch engine is built` to `…the trait keeps the product working.` | `**Reference model and baselines.** The differential oracle is a naive in-memory Rust reference model inside the project — \`BTreeMap\` graph, derived state recomputed from scratch, branch states by replay from genesis, merges by the §5a.7 table over materialised LCA and side states, marker absorption by exhaustive ancestry — deliberately using different algorithms from the engine and sharing no code with it; it is test-only, never linked into the binary and never a backend ([60 §3]). No SQLite or other third-party database is used as an oracle or backend. Performance is judged against the absolute budgets of §8.1 and the physical floor measured in item 12 (durable commit p50 ≤ flush floor + 0.5 ms, exactly one flush per durable commit, open ≤ 3 ms at 1e6, private RSS ≤ 4 MB per CLI). redb 4.x and heed/LMDB may be run once at E1 exit and once at E4 exit as optional, out-of-tree reference points (a throw-away crate outside the moirai workspace and its lock file), for information only — never a gate, a dependency or a fallback; SQLite is not benchmarked.` |
| 33 | §8.2 Correctness gates (1426–1428) | `- **Deterministic multi-process simulation** (own workstream, ~20 % of the build):`; `- **Windows kill loops**: 16 writer + reader processes`; `- **Property tests**: incremental derived state == full recompute` | `- **Deterministic multi-process simulation** (own workstream, ~20 % of the build; single-process crash-point enumeration is mandatory from E1, the multi-process simulation from E4 — [60 §2.13]):`; `- **Windows kill loops** (a 1,000-iteration serialized-stage loop gates E2; the full loop gates E4 on \`main\` and E5 across branches): 16 writer + reader processes`; `- **Property tests** (differential against the reference model of [60 §3] unless stated): incremental derived state == full recompute` |
| 34 | §9 whole section (1435–1450) | from `## 9. Roadmap` to the end of the `Adoption path:` paragraph | The block in §7.2 below. |
| 35 | §10 rows 1, 5, 6, 8, 13 and new rows (1458–1470) | see the next table | see the next table |
| 36 | §11 row 1 (1480) | `the \`shared\` class moves into S3 and T3′ becomes` | `the \`shared\` class moves into E5 and T3′ becomes` |
| 37 | §11 row 2 (1481) | the whole row, from `\| 2 \| **Build order.** Run S1–S5 on a throw-away SQLite backend` to `the "from scratch" requirement is met either way \|` | `\| 2 \| **Build order.** *Decided by the owner on 2026-09-26:* engine first, with no SQLite or other third-party database at any milestone (not as a temporary backend, not as a test oracle); roadmap E0–E7 [60]. Remaining owner call: may E4 (lock-free protocol) overlap E3 (packs/hooks) when two lanes of agents are available? \| overlap when capacity allows; strictly sequential \| overlap when capacity allows; E4 always completes before E5 and E7 \| strictly sequential moves R1, R3 and MCP 2–3 weeks later; putting E4 before E3 delays the memory-system adoption by 2–3 weeks \|` |
| 38 | §11 row 15 (1494) | `with generated views from S2?` | `with generated views from E3?` |
| 39 | §12, after the row `A CoW B+tree / prolly-tree / Merkle-state engine for current state` (1508) | *(new row)* | `\| A third-party embedded database (SQLite, redb, LMDB/heed, fjall, sled, RocksDB) anywhere in the product path — including as a temporary backend, a stepping stone or a test oracle \| owner decision 2026-09-26; the engine is built from E1 and the Rust reference model is the oracle; redb/heed only as optional out-of-tree benchmark points \| [60] \|` |
| 40 | Review log "Where" column (1534–1554) | CB1 `§9 S4`; CB3 `§9 S3`; CM4 `§9 S4`; CM5 `two-lanes-staged test in S3 and CI` and `§9 S3`; CM6 `S1 exit criterion` and `§9 S1`; CM7 `§9 S1`; CM8 `in S1` and `§9 S1`; CM9 `in the S3 property tests` and `§9 S3`; CL1 `as an S4 deliverable` and `§9 S4`; CL7 `so S3 gains no merge rule from it` | CB1 `§9 E6`; CB3 `§9 E5`; CM4 `§9 E6`; CM5 `two-lanes-staged test in E5 and CI`, `§9 E5`; CM6 `E2 exit criterion`, `§9 E2`; CM7 `§9 E2`; CM8 `in E1`, `§9 E1`; CM9 `in the E5 property tests`, `§9 E5`; CL1 `as an E6 deliverable`, `§9 E6`; CL7 `so E5 gains no merge rule from it` |
| 41 | Review log row CL9 (1554) | `\| §9 \|` at the end of the CL9 row | `\| §9 (superseded 2026-09-26 by the engine-first calendar: 6.5–10 weeks to the tracker adoption, 8.5–13 to the memory-system gate, 14.5–22 to R1, 16.5–25 to R3, 17.5–28 to MCP; 15.5–25 in total with E3 ∥ E4 — [60 §5]) \|` |
| 42 | Review log, after the "Editorial corrections" paragraph (1556) | *(new paragraph)* | `**Owner decision 2026-09-26 — engine first, no SQLite.** The owner rejected decision #2's default (S1–S5 on a throw-away SQLite backend, engine at S6). Adopted as [60]: no third-party database at any milestone; roadmap E0–E7 with a serialized-access first stage (E1–E3) and the lock-free protocol at E4; the Rust reference model as the differential oracle; floor-relative benchmark gates with redb/heed only as optional out-of-tree reference points. Edits: header, sources, §0 (bullet 10, why, risks), §1 row 1, §2.2, §2.10, §2.13, §2.14, §2.17 (§2.3, §2.5, §2.7/D8, §2.9), §4.2, §4.5, §4.6, §4.7, §4.10, §5b.2, §5b.5, §5b.6, §5b.7, §6.1, §7.1, §7.4, §7.5, §8.1, §8.2, §9, §10 (rows 1, 5, 6, 8, 13–16), §11 (#1, #2, #15), §12, and this log's pointers. T1 and T3′–T16 other than T10 and T13 are unchanged.` |

*(Table row 35 in detail: §10 risk register.)*

| Row | Column | Current text | Replacement |
|---|---|---|---|
| 1 | Mitigation | `readers stop at \`committed_lsn\`; ref move inside the commit; fsync fatal; DST + kill loops as the S6 gate; \`doctor --verify\` in CI; the oracle backend until S6 passes; \`backup\`/\`restore\` bound the blast radius` | `readers stop at \`committed_lsn\`; ref move inside the commit; fsync fatal; serialized access until E4 (no lock-free readers before the full simulation passes); crash-point enumeration from E1, a serialized kill loop before the first real data (E2), full DST + 10,000-iteration kill loops as the E4 gate and again across branches at E5; \`doctor --verify\` in CI; \`backup\`/\`restore\` and \`repair --rebuild-from-log\` bound the blast radius` |
| 1 | Signal | `a kill-loop failure blocks the S6 swap` | `a DST or kill-loop failure blocks the E4 switch to lock-free mode (the store stays serialized)` |
| 5 | whole row | `\| 5 \| The from-scratch engine takes far longer than estimated \| high / medium \| … \| the trait + oracle backend keep adoption independent; the format spec, not the engine, is the contract; S6 is swappable \| S6 slipping does not block S1–S5 \|` | `\| 5 \| The from-scratch engine takes far longer than estimated — and it is now on the critical path to every milestone \| high / high \| "95 % of the effort is testing" (folklore but directionally right [04 §3.14]); redb needed years for multi-process; DoltLite ~2,000 PRs \| E1 is the smallest durable core (serialized access; no lock-free readers, \`hist\` or promotion) and is adoptable at E2; the format, not the engine, is the contract; typed merge rules developed against the reference model during E3/E4; E3 ∥ E4 overlap; pre-agreed cut list and triggers [60 §6.2] \| E1 exit after week 7; E4 exit after week 16 \|` |
| 6 | Signal | `the S0 experiment result` | `the E0 experiment result` |
| 8 | Mitigation / Signal | `S0 measurement; packs via fast-import; leader forwarding as the fallback \| S0 numbers` | `E0 measurement; packs via fast-import; leader forwarding as the fallback \| E0 numbers` |
| 13 | Mitigation | `\`backup\`/\`restore\` in S1 (transaction-consistent, verified)` | `\`backup\`/\`restore\` in E1 (transaction-consistent, verified)` |
| 14 (new) | — | — | `\| 14 \| Engine defects reach the owner's live store earlier: the from-scratch engine carries real data from E2, 8–15 weeks before the oracle plan's S6 \| medium / high \| new code; segment writers and recovery are the classic early failures [08 §8.1] \| serialized access until E4; crash enumeration (E1), reference-model differential tests and a 1,000-iteration kill loop (E2) before any real data; log and canonical formats frozen at E0 while segments, blobs and \`hist\` are rebuildable (\`repair --rebuild-from-log\`); daily \`backup\` with the backup-age warning; \`export md\` as an engine-independent copy; A1 node cap (≈ 5e4, est.); HDR/MEMORY.md kept until the E3 gate \| \`doctor --verify\` findings; any \`repair\` use \|` |
| 15 (new) | — | — | `\| 15 \| First adoption and R1/R3/MCP arrive later than under the oracle plan (tracker 6.5–10 weeks instead of 5–7; memory system 8.5–13; R1 14.5–22 instead of 9–13) \| high / medium \| [60 §5] \| A1 ships the tracker before packs; E3 does not wait for E4; E3 ∥ E4 with two lanes; the unchanged fallback of running a campaign on \`main\` when E5 is late \| milestone dates vs [60 §5] \|` |
| 16 (new) | — | — | `\| 16 \| The reference model and the engine share a misunderstanding of the spec (no independent third-party oracle any more) \| medium / medium \| both written from this document by one team \| different algorithms (replay from genesis, exhaustive ancestry, full DFS cycle checks, materialised LCA states), no shared code, human-verified fixtures (register incidents, the §5d.3 node-40 table, the §7.6 walk-through), \`doctor --verify\` as a third recompute \| model/engine disagreements triaged as spec findings \|` |

### 7.2 Replacement blocks

**§2.13 T13 (edit 12)** — replace the four paragraphs with:

> **Decision.** Engine first (owner decision 2026-09-26, replacing the oracle-first plan). The from-scratch engine is built in layers, each tested differentially against the in-project Rust reference model, and no third-party database is used at any stage. The first adoptable slice is E0–E2 of §9: format spec v1 frozen, measurements, `Vfs` and reference model; the durable core in serialized-access mode; trunk graph + CLI on `main`. It is adopted as the task tracker. E3 adds packs/brief/hooks/skill and the one-off import of standing rules, pins, lanes and open questions, and is the memory-system adoption gate (the former S2 gate). R2 is satisfied at E2; the design-of-record multi-process protocol (lock-free readers, maintenance byte) at E4; R1 at E5 (branches + merge for the next campaign's lanes); R3 at E6 (image at checkpoint granularity via fast-import). MCP is E7. v1 ships `tag`, `undo`, `revert` (incl. `--mainline 1` for merges), `reflog`, `cherry-pick`, `backup`/`restore` (E1, CM8), and `check`/`stale` that spawn `git merge-base` once per uncached pair when the commit-graph is stale (CM7). Later: `rebase --onto`, `op restore`, branch promotion, the hand-written pack layer and pack reader (which replaces the `check`/`stale` spawn), bundles, SHA-256 images, `--with-oplog`, tracked-directory and orphan-branch destinations, the leader and `watch`, FTS tier 2 and the `shared` field class.
>
> **Chosen from.** [60] over [22 §7.3]'s slice order; D's milestone content [D §9]; A's engine-first order [A §9], split into an adoptable serialized layer (E1–E3) and a hardened lock-free layer (E4).
>
> **Rejected.**
> - *Oracle first* (S0–S5 on a throw-away SQLite backend behind an engine trait, the engine at S6; [22 §7.3] and this section's previous text): rejected by the owner on 2026-09-26 — no third-party database in the product path at any milestone.
> - *D's M1 as written* (engine + refs + branches + overlays + promotion + checkout + undo + revert + tag + GC before any CLI) [D §9]: ~40 units before feedback (D8); split across E1, E4 and E5.
> - *Engine complete before any adoption* [A §9], [C §9]: 60 % of the work precedes the first adoption gate [22 §2.7]; layering brings the first adoption to E2 (6.5–10 weeks).
> - *Lanes before MCP* [B §9]: the three Bash-less roles wait behind the merge engine — kept, because E7 is small and E2–E3 already serve those roles through the orchestrator's `apply`.
>
> **Revisit trigger.**
> - The owner's first target is publishing to git → E6 before E5 (N4/N5 must land first).
> - Serialized-mode contention measured at E2 is above the thresholds of [60 §6.2] (wait p99 > 250 ms with 16 CLI processes, or hook p99 > 1 s) → E4 moves ahead of E3.
> - The Bash-less roles become the bottleneck → E7 ahead of E6.

**§9 Roadmap (edit 34)** — replace the whole section with:

> ## 9. Roadmap
>
> **Engine first (owner decision 2026-09-26).** No SQLite or other third-party database is used at any milestone, including as a temporary backend or a test oracle. The from-scratch engine is built in layers. The first layer runs a serialized-access protocol (every process holds `LOCK` byte 0 for one command, §4.5 protocol stages) that is already safe to adopt. The lock-free protocol of §4.5 and §6.1 is switched on at E4, after its simulation and kill-loop gates. Every layer is tested differentially against an in-project Rust reference model (§8.2, [60 §3]). The rationale, the per-milestone gates, the reference-model spec and the risk deltas are in [60].
>
> Sizes: S ≈ a week, M ≈ 2–3 weeks, L ≈ 4–6 weeks of the owner plus agents (est.). Relative effort for E0–E7 is ≈ 125–140 units (A = 100) [22 §7.1], the same as the oracle-first plan through S6 within the estimate's error: the throw-away backend and the second implementation of trunk and branch views disappear, and the reference model and the engine's layering are added. Roughly 26–32k lines of Rust plus 12–16k of tests (est.; the reference model is ≈ 3–4.5k of them).
>
> **Calendar (sizes summed; the test workstream in parallel):**
> - E0 + E1 + E2 = **6.5–10 weeks to the first adoption** (the task tracker on `main`);
> - + E3 = 8.5–13 to the memory-system gate;
> - + E4 = 10.5–16 to the design-of-record protocol;
> - + E5 = **14.5–22 to R1**;
> - + E6 = 16.5–25 to R3;
> - + E7 = 17.5–28 to MCP.
>
> When two lanes of agents are available, E4 overlaps E3 and every date from R1 on moves 2–3 weeks earlier (R1 12.5–19, R3 14.5–22, MCP 15.5–25).
>
> **Dependency.** E5 must be ready before the next multi-lane campaign opens. If it is not, that campaign runs on `main` with `files_owned`, `lane conflicts` and store-wide leases only (the E3 state), and the campaign after it gets branches. Nothing in the design is lost; that campaign simply does not exercise R1.
>
> | Milestone | Scope | Exit criteria (owner's machine, Defender on, idle and under typical agent load) | Tests / gates |
> |---|---|---|---|
> | **E0 — Contract, measurements, harnesses** (M−, 1.5–2 wk) | On-disk format spec v1: record header, every record kind including `RefUpdate`/`Pin`/`ClientHead`/`Marker`/`GitMap`/`Checkpoint` with per-ref lsn lists, the commit body, the canonical form (§4.6), the `HEAD` slot with the `lockfree` bit and the `LOCK` byte map are **frozen**; segment sections are specified but rebuildable from the log; a reserved value type for R4 file references (placeholder). The logical `Store` API. The `Vfs` trait with an in-memory implementation that injects crashes, fsync errors, lock delays and sharing violations. Reference model v0 (trunk semantics). Bench harness. The §8.2 measurements 1–13. The hook experiment. | Numbers recorded; lock bound, checkpoint, loose/pack, G15/G16 thresholds and the A1 node cap fixed; golden byte fixtures for every record kind; the model passes the delete-policy matrix and the single-branch node-40 fixtures; spec reviewed | bench harness; model fixtures; toy crash enumeration |
> | **E1 — Durable core, serialized access** (L, 3–5 wk) | Windows `Vfs` (G1 wait on byte 0 held per command, `DATA_SYNC_ONLY`, read-only maps, share flags, errors 5/32, refusal of network/OneDrive paths). 64 MiB zero-filled extents (G11). Recovery with adopt-republish-then-idempotency (G2). Epoch (G25). `HEAD` by `pread`. Base + delta segments (columns, CSR both directions, frozen bitsets, symbols, tombstones, runtime sections, `REFS`/`PINS`/`HEADS` holding `main`). Overlay. Bodies in the log tail sealed into `blobs` (G7). Delta checkpoint inside byte 0, tiered fold, explicit `gc` rollup, delete-pending GC, orphan sweep, quiet cap. `prev_on_ref`/`ref_seq` written for `main`. `init --store`, `doctor --fsck/--verify`, `backup`/`restore` (CM8), `repair --rebuild-from-log`. | Durable commit p50 ≤ flush floor + 0.5 ms with exactly one flush; open ≤ 1.5 ms at 1e5 and ≤ 3 ms at 1e6 with a full tail; bytes read on open independent of N; CLI private RSS ≤ 4 MB at 1e5; delta checkpoint ≤ 30 ms at 1e4 and ≤ 50 ms at 5e4; recovery ≤ 10 ms; `backup` → `restore` → `doctor --verify` clean; rebuild-from-log byte-identical | crash-point enumeration at every write/flush/publish boundary incl. N3, with fsync errors (mandatory from here); parser fuzzers 24 h; storage-level differential vs the model |
> | **E2 — Trunk graph + CLI on `main`** (M, 2–3 wk) — **first adoption (task tracker); R2 met** | 13 kinds + `phase_state`/`return_to` + `gates`; schema as data; derived state; I5′ with PK; delete policies with X4 flagging, `--replaced-by`, I32′; verbs `init` (D4 guard), `--link`, `worktree bind`, `add`, `set`, `link/unlink`, `move`, `doc patch`, `rm --dry-run/--yes`, `apply`, `ready`, `blocking`, `blockers --explain`, `show`, `tree`, `find`, `changes`, `claim`, `complete` (`settled` written by the op, CB3), `reopen`, `stats`, `lane conflicts`, `check`/`stale` (CM7), `notes --path`, `export md`, `doctor store\|agents` (CM6); discovery chain with `<git-common-dir>/moirai/` placement; output contract and exit codes frozen; `--branch`/`--lease` accepted (only `main` exists); R5-a slot (query core, CLI read verbs as named queries — placeholder) | `blocking --ids` ≤ 100 µs at 1e5; engine ≤ 5 ms per command at 1e5; derived state == reference model under 1e6 random ops and == `doctor --verify`; every verb documented; `init` in the main checkout and `ready` from a `<lanes-dir>/*` worktree find the same store; `check` of a seconds-old commit correct; serialized wait p99 ≤ 250 ms with 16 CLI processes (else E4 before E3) | differential tests vs the model; golden outputs; PowerShell 5.1 argv; delete-policy matrix; 16 × 1,000 serialized kill loop (mandatory before real data) |
> | **E3 — Packs, brief, hooks, skill, import** (M, 2–3 wk) — **memory-system adoption gate** | C's pack algorithm with the §7.4 header, chars budgets, `*` default, `~main` slot (empty until E5), N15 rendering; `brief`; SessionStart + UserPromptSubmit + SubagentStart/Stop + agent-launched hooks (fail-open); `export memory-md`; core skill + `moirai-report` + `moirai-orchestrate`; one-off import of the dozen standing rules, current pins, live lanes, open owner questions; R4-a slot (file references in packs, explicit file verbs — placeholder) | One real campaign on `main` with no HDR and no hand-written resume block; dispatcher pattern only; HDR-vs-pack diff judged complete; hook output ≤ 8,000 chars with exact drop footers; hook p99 ≤ 1 s under a 16-agent burst; a hook facing a held lock returns empty with exit 0 | pack budget tests; hook fixtures against Claude Code 2.1.28x; pack candidate sets == model |
> | **E4 — Lock-free protocol + hardening** (M, 2–3 wk; may overlap E3) | Readers without locks, stopping at `committed_lsn`; checkpoints under the maintenance byte (G9); quiet cap in this mode (G10); `hist` retirement with per-frame commit index (G3); long-lived mappings with delete-pending GC; `backup` via a pinned segment set; `migrate --protocol lockfree\|serialized`; lock-hold instrumentation | Multi-process DST with crash/fsync/lock-delay/AV injection passes 1e6 steps; 16-process Windows kill loop 10,000 iterations / 1 h with zero lost acknowledged commits and `doctor --verify` clean; writer-wait p99 ≤ 50 ms with 16 writers; reader p99 during a writer burst ≤ 2× idle; writer-byte hold p99 ≤ 5 ms; 0 % CPU over 10 min idle; 24-h soak on a copy of the owner's store; then the store is switched | the simulator, kill loops and fuzzers of §8.2 (mandatory from here) |
> | **E5 — Branches + merge for the next campaign's lanes** (L, 4–6 wk) — **R1 met** | Refs (log-folded tables), reflog, `ClientHead` with session expiry, pins refcounted per file, per-ref index (G15), streaming branch overlay (pin ⊕ ops, merge-by-reference sync, no promotion); `branch`/`checkout`/`--list`/`-d`/`tag`/`undo --expect`/`revert`/`cherry-pick`/`log --graph`/`diff A...B`/`show@`/`blame`/`at`; `lane open/close`; typed 3-way merge with base at the LCA, sync-first merges into `main`, single-LCA rule, per-pair `merge/<dst>/from/<src>` staging (CM5), `resolve`, `merge --continue`, `sync --check`/`sync`, `merge-check`, `--across`; op-level markers and absorbed vectors (CB3, CM1); segment-walk fold (CM9); `revert --mainline 1` (CL8); `apply` branch from the run; `plan/*`; `moirai-branches` skill; SubagentStart `sync --check` gating; R4-b and R5-b slots (placeholders) | The owner's two register incidents replay correctly; the ten-door I26′ test; `ready` with 50 stale markers within budget (CM1); two staged syncs do not block a third (CM5); 10 synthetic lanes × 1k ops with every conflict class incl. sync-then-merge, criss-cross, branch-of-branch (CM9) and move+inheritance merge deterministically; merge ≤ 50 ms at 1e5; a `plan/*` branch cannot mark work done; first read of lanes forked 1k/7k/14k/60k commits ago within 1–3/5–10/10–20 ms; 50 branches × 2k ops readable within budget; ≤ 10 ms on a lane including the overlay build at a 14-day fork; kill loop with 16 directories bound to different branches | property tests of §8.2 vs the model; I25′/I26′/I31′/I37′ fuzz; `doctor --verify` on every branch head |
> | **E6 — Git image, checkpoint granularity** (M, 2–3 wk) — **R3 met** | `.moi` ABNF + golden fixtures (CL1); encoder/decoder with self-check; tree layout; the full trailer set of §5b.4 (CB1); two-parent `sync` commits (CM2); byte-exact bodies (CM3); tombstones with edges (CM4); `gitmap`; the unhashed side ref (CB2); `ImageBackend` over `git fast-import`/`cat-file`; export of `main` + `tags/*` + `lane/*` at checkpoint granularity to a separate bare repo (SHA-1) (CM8), `commit` granularity behind a flag, `refs/moirai/*` as a second destination; import with native verification, foreign commits, `Undelete`, `incr` ledgers, typed two-parent foreign merges, `ImageParse` staging, `import-checkpoint`, markers from imported ops (CB3); `image doctor --rebuild-map`, `image show`; post-merge export hook; R4-c slot (placeholder) | Gate 0 first; byte-identical at `commit` granularity and state-identical at `checkpoint` granularity for 1e5 nodes and 1e5 commits (§5b.7 gates 1–2); `git fsck` clean; the specified failure cases behave as specified; full export of 1e5 ≤ 3 s | round-trip property tests; `.moi` fuzzer; differential tests against git; state-identical check vs the model |
> | **E7 — MCP** (S–M, 1–3 wk) | `moirai mcp` on rmcp dual-era (`current_thread`), ten tools with explicit `branch` validated against `lease`, stamp on write tools only, role policy on the dispatch label, plugin packaging; LRU of ≤ 8 branch overlays (G19); rollup only after serving a request; R5-c slot (placeholder) | Architect and critic complete a round on a lane branch without Bash; schema ≤ 5k chars; MCP private RSS ≤ 10 MB + 1 MB × min(active branches, 8) at 1e5; 0 % CPU over 10 min idle | conformance tests for both handshakes; `structuredContent` regression |
> | **Later (v1.1+)** | Branch promotion (`seg.b*`, `TOUCH`); hand-written loose/pack/idx/bundle writer and reader for "no git installed" image I/O (or `gix` if the owner allows it); `commit`-granularity and lane export by default; SHA-256 images; `rebase --onto`, `op restore`, `--with-oplog`; tracked-directory and orphan-branch destinations; recursive virtual merge base; the leader/`watch`/`PostToolBatch` push (M6); FTS tier 2; schema strengthening migrations; `resource` mutex ergonomics; the `shared` field class if one campaign shows chronic staleness; optional HTTP MCP mode; signed installer/winget; R4-d and R5-d (placeholders) | each gated by a measured need | |
>
> **Adoption path.**
> - **E2** puts task tracking on `main` on the from-scratch engine in serialized mode. `backup`/`restore` (E1) and `export md` are the safety nets, and the HDR and MEMORY.md resume block stay in use.
> - **E3** replaces the MEMORY.md resume block and the HDR on `main`.
> - **E4** switches the store to lock-free mode after its gates.
> - **E5** turns the next campaign's lanes into branches.
> - **E6** adds the browsable off-store image of `main` and every lane.
> - **E7** lets the Bash-less roles write directly.
>
> The 254 memory files stay a read-only archive linked by `artifact` nodes. `OPEN-QUESTIONS.md`, `BACKLOG.md` and `MEASUREMENT-QUEUE.md` become generated views (`export md`): regenerated, never merged.

### 7.3 Recommended consistency edits: legacy M-labels (not required by the decision)

[AR] still uses proposal D's measurement and milestone labels in places. Renaming them keeps the document on one milestone vocabulary. **M6 stays** as the name of the deferred leader milestone (lines 44, 97, 157, 267, 274, 275, 523, 1107, 1108, 1122, 1135, 1372, 1448).

| Line | Current | → |
|---|---|---|
| 93 | `**Revisit trigger.** M1 measurement at the owner's real update mix` | `**Revisit trigger.** E1/E2 measurement at the owner's real update mix` |
| 103 | `the M0 Defender close-cost measurement (G8)` | `the E0 Defender close-cost measurement (G8)` |
| 113 | `If M0 shows the interleaved-log scan` | `If E0 shows the interleaved-log scan` |
| 143 | `on real notes (M0)` | `on real notes (E0)` |
| 167 | `(Cyrillic ratio measured in M0, [22 §2.5])` | `(Cyrillic ratio measured in E0, [22 §2.5])` |
| 261 | `Adopted: M0 measurement; decides whether` | `Adopted: E0 measurement; decides whether` |
| 278 | `Format item before M1.` | `Format item of the E0 spec.` |
| 291 | `\| G28 \| M0 measurement additions \|` | `\| G28 \| E0 measurement additions \|` |
| 293 | `Carried as M0 measurements;` | `Carried as E0 measurements;` |
| 316 | `property test I25′ in M3 (§5a.7)` | `property test I25′ in E5 (§5a.7)` |
| 330 | `the merge variant of X1 added to M3 tests` | `the merge variant of X1 added to E5 tests` |
| 1225 | `"Storage engine M1"` (a task title in the example output) | optional: `"Storage engine E1"` |

Keep these citations of *other* documents' milestones unchanged: line 101 `[C M3]`, line 209 `[B §9 M0] oracle idea` (a citation of where the oracle idea came from), and line 211 `*D's M1 as written*` (already inside replacement edit 12).

### 7.4 Matches that must NOT be changed

| Line | Match | Why it stays |
|---|---|---|
| 29 | `the SQLite session extension` | a citation of a precedent |
| 56 | `SQLite's WAL-reset race hid for 16 years and fell to deterministic simulation in minutes [08 §3.1]` | a citation (the lesson behind the simulation gates) |
| 177 | `Excluded from the product: SQLite, redb, LMDB/heed, fjall, sled, …` | this *is* the exclusion; only the sentence after it changes (edit 9) |
| 598 | `(SQLite-changeset style [04 §3.13])` | a design citation |
| 807 | `(the SQLite rebaser idea [04 §3.13])` | a design citation |
| 1426 | `SQLite's WAL-reset race fell to this method in ~15 minutes [08 §3.1]` | a citation |
| 1458 | `SQLite WAL-reset race 16 years [08 §3.1]` (Evidence column) | a citation; only the Mitigation and Signal columns change |
| 68, 235, 954 / 1012 | `[D §12 S1…]` / `[D §12 S4]` | proposal D's source labels, not slice names |
| 151, 313, 314, 315, 403 | `(S12)`, `\| S1 \|`, `\| S2-B \|`, `\| S6 / N14 \|`, `(S2-B)` | [21]'s finding labels |
| 1001 | `import(export(S1)) ⊕ import(export(S2))` | store variables in gate 3. Optionally rename them to `St1`/`St2` to avoid a clash with the old slice names; not required. |
| 1352 | `` `.slice()` `` | JavaScript, from the owner's scripts |
| 181, 1430, 1482 | `gix` as an optional import/read **oracle in tests** / "`gix` only as a test oracle" | `gix` is a git library, not a database; the decision does not cover it. The owner may extend the ban; if so, the git CLI differential tests of §8.2 already cover the same ground. |
| 1554 | CL9 finding text `slice sizes never summed` | a historical finding; only its Where cell gets the superseded note (edit 41) |

**Out of scope for this edit list.** [AR §2.11] ("*Text2Cypher / a query language*" rejected) and the [AR §12] row "A query language (Cypher/GQL/Datalog) in v1" contradict R5. They belong to the R5 design [50], not to this decision. §2.14 only reserves R5's slots in the roadmap.
