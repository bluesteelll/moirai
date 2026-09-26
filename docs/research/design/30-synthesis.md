# 30 — Synthesis: the recommended moirai architecture

*Design synthesis for moirai. Date: 2026-09-26. Status: research/design only; nothing is implemented and this file is the only artifact written. It supersedes proposals A–D as the design of record; where it changes a proposal, it says so.*

**How to read this document.** Citations `[NN §name]` point into the eight research reports under `docs/research/` (01 roles, 02 orchestration, 03 landscape, 04 versioning, 05 storage/perf, 06 data model, 07 agent integration, 08 concurrency/git). `[A]`, `[B]`, `[C]`, `[D]` are proposals 10–13; `[20]`, `[21]`, `[22]` are the critiques (perf/RAM/Windows, semantics/correctness, agent-fit/buildability). Labels such as `G15`, `F-D1`, `N2`, `X4`, `D2` are the critiques' own finding labels and are used unchanged so the resolution ledger in §2.17 can be checked against them. Numbers carry the tag of their source: **[M]** measured on the owner's machine, **claimed** (vendor/third party), or **est.** (arithmetic with inputs shown). Everything not tagged is a design decision.

**Binding inputs.** The owner's brief (graph DB from scratch in Rust, typed fields, git-like versioning, subtasks, blockers, CLI/MCP/skills, synchronous references, maximum performance, minimal RAM, Windows 11) and the owner update of 2026-09-25: **R1** full branching for all versioned data with create/switch/list/delete/diff/log/merge (ideally tags, revert, cherry-pick, reflog/undo); **R2** moirai's own VCS with zero git dependency in the core; **R3** a deterministic git-compatible image that can be saved to and loaded from git. R1–R3 override every earlier position; in particular "single shared trunk + provenance scoping" [04 §9.3], [08 §7.2], [A T3], [C T3] and "coordination plane never branched" [B §3.1] are no longer acceptable branch models.

---

## 0. Executive summary

**What moirai is.** A single static Rust binary that opens a small memory-mapped store in about a millisecond and gives AI coding agents one typed graph for tasks, subtasks, blockers, plans, rules, decisions, findings, verdicts and measurements. The graph is versioned by moirai's own git-shaped version control: a BLAKE3 content-addressed commit DAG of typed changesets with before-images, first-class branches for every datum, a typed three-way merge engine with conflicts stored as data, tags, reflog, undo and revert. The store can be serialized into a deterministic, human-readable git image (one text file per node, one git commit per moirai commit or per checkpoint) and read back, so the data can live in any git repository and travel through any git remote, while git itself is never required to run the system. Agents reach it through a CLI (skill + hooks), a ten-tool MCP server for the roles that have no shell, and budgeted context packs that replace the owner's hand-written briefs.

**The architecture in ten bullets.**

1. **Engine (T1):** an append-only, checksummed operation log is the canonical history (~0.3–0.6 KB per commit, est.); current state is materialized into immutable, read-only-mapped, zero-copy columnar segments (node columns, forward and reverse CSR adjacency, frozen bitsets, symbol table, content-addressed bodies) plus a bounded in-process overlay built from the log tail. Open is O(1) plus a bounded tail; nothing is O(history) or O(nodes) on the hot path. Base: [B §4] as adopted by [D §4], with every graft of [20] (G1–G28) and [21] N3 written into the format.
2. **Process model (T2):** purely embedded multi-process; one writer at a time elected by a blocking `LockFileEx` wait on a dedicated `LOCK` file, lock-free readers, no daemon, no threads, no timers, zero idle CPU by construction. The session's MCP server is an ordinary long-lived client; an opportunistic leader is a later optimisation (M6) and never required for correctness.
3. **Branches (T3′, R1):** `main` is just a branch. A branch is a ref plus a *pinned checkpoint segment set* ⊕ trunk ops to the fork ⊕ the branch's own ops, found through a per-ref op index (G15). Fork costs one durable commit and no copying; ~50 live branches cost tens of MB of pinned disk and no RAM until read. HEAD is per client (directory binding, lease, or explicit `--branch`), so many agent processes read and write different branches of one store concurrently.
4. **Merge (T7):** typed per-field three-way merge with the base taken at the LCA state (N1), status lattices, `Incr` counters, add-wins sets, diff3 text, Kleppmann moves; value conflicts land as conflict values (jj), structural violations (dangling edge, cycle, hierarchy cycle, schema) never advance a ref and are staged on `merge/<src>` (TerminusDB/Dolt). Merges into `main` are sync-first, so the daily path has a unique LCA and criss-cross cannot arise (N7).
5. **Coordination under branching (T16):** everything in the graph is versioned per branch, including `status`/`done` and `blocks`. Only runtime coordination is store-level: leases with fencing tokens, **`settled` and `deleted` markers** (a task completed or deleted on any unmerged branch is never dispatched again from another branch — N2/D1), idempotency results, change-feed cursors, the `#N` allocator, HEAD bindings, pins and the git id map. Each is justified in §5d.
6. **Git independence (T14, R2):** the store is `.moirai/`, discovered by flag → env → walk-up (directory or pointer file, with store id) → an optional textual `.git`/`commondir` *hint*. The core never spawns `git` and never links a git library; provenance is read from two small text files; `moirai init` refuses to shadow a hinted store (D4).
7. **Git image (T15, R3):** a specified canonical `.moi` text file per node under a two-level `uid` fan-out, out-edges in the source file, tombstones as files, counters as append-only ledger lines, conflict values representable, commit metadata in git trailers with a verifiable `Moirai-Commit` id, no store-local datum in hashed content (N4). Export and import are hand-written at the semantic level; the git *object* writer/reader uses `git fast-import`/`git cat-file` when git is present, with a hand-written pack layer as a later "no git installed" mode. Default destination: a separate bare image repo, SHA-1, `main` + tags, one checkpoint per merge into `main`; 1:1 commits and lane branches on request.
8. **Deletion and "node 40" (T5):** hard delete in the current state, tombstone in history, per-edge-kind policies executed in one commit over the reverse index (O(degree)); a deleted blocker leaves a flagged edge that keeps dependents out of `ready` until resolved or re-pointed (X4); across branches the delete applies where it is received (commit, sync, merge, import) and is visible elsewhere on request and through the `deleted` runtime marker.
9. **Agent surface (T9):** CLI with a frozen output contract (ids first, one line per record, explicit drop footers, exit codes, bodies via stdin), three skills plus an orchestrator-only `moirai-branches` skill, fail-open hooks (SessionStart brief, UserPromptSubmit delta, SubagentStart role pack, SubagentStop lease safety net, PreToolUse stamp on write tools only), a ten-tool MCP server whose every tool takes an explicit `branch`, and context packs budgeted in characters with per-class quotas.
10. **Build order (T13):** on-disk format spec and measurements first; trunk graph + CLI adopted on `main` behind an engine trait (oracle backend), packs/briefs/hooks next; branches and merge for the next campaign's lanes; the git image at checkpoint granularity; MCP; then the from-scratch engine swapped in behind the same trait with a deterministic multi-process simulator and Windows kill loops as its exit gate.

**Why this design.** Every report agrees on the engine family: path-copy Merkle state costs 4 KiB × depth per tiny commit (~52 KB measured on DoltLite [04 §3.2]) while changesets cost bytes [04 §6]; Windows forbids resizing mapped files and mapped views are not coherent with `WriteFile` [05 §6.1–6.2], so only immutable files are mapped; a durable flush costs ~2 ms [M, 05 §2.2] and process spawn 15–73 ms [M, 05 §2.1], so the engine is a small share of an agent call and O(1) open matters more than micro-optimisation; the owner's machine has 1.8 GB free with 16 agent processes resident [M, 05 §2.4] and a zero-idle-CPU rule for benchmark windows [02 §9]. Proposal D is the only design that meets R1–R3 and it inherits the best layer of each earlier proposal (A's protocol and invariants, B's merge rigour and hygiene, C's agent surface); the critics scored it first on all three lenses (7.5 / 7.0 / 7.0) and found no flaw that requires abandoning its structure. This synthesis is D with every critique finding resolved (§2.17), the two daily-path wrong answers (N1, N2/D1) closed by rule, the two format-level defects (F-D1, F-D3) fixed before the first byte is written, and the build re-ordered so adoption precedes the engine.

**The biggest risks.** (1) The multi-process file protocol on Windows: SQLite's WAL-reset race hid for 16 years and fell to deterministic simulation in minutes [08 §3.1]; Beads lost 7 of 8 acknowledged closes under agent load [03 §2.7]. Mitigation: readers never read past `committed_lsn`, the ref move is inside the commit record, simulation with crash/fsync/lock-delay injection and 16-writer kill loops gate the engine milestone. (2) Branch isolation fighting the workflow: lanes miss rules and completions on `main` if nobody syncs. Mitigation: `~main` rules in every pack, `behind main` notices in every brief and hook, `settled`/`deleted` markers, `sync --check` gating, and an explicit fallback (`shared` field class) after one campaign. (3) Image determinism drift across formats and stores. Mitigation: no store-local data in hashed content, byte-identical round-trip property tests, retained encoders per format version, the exporter re-parsing what it writes. (4) Build size: two long poles (protocol and image determinism); the trait-and-oracle path keeps adoption independent of the engine. (5) Merge-semantics creep: the type set is closed and rules are schema data.

---

## 1. Requirements traceability

| # | Owner requirement | How it is met | Evidence |
|---|---|---|---|
| 1 | **Graph database written from scratch in Rust** | Hand-written on-disk format, log, segments, columnar graph (CSR both directions), frozen bitsets, lock/HEAD protocol, overlay, Pearce–Kelly, merge engine, diff3, `.moi` codec, CLI. Leaf crates only: `zerocopy`, `blake3`, `xxhash-rust`, `zstd`, `windows-sys`/`libc`, `serde_json`; `rmcp`+`tokio` only in the MCP front-end; no storage engine, no `gix`/`git2`, no `petgraph`, no async runtime in the core (§2 T10). | Validated zero-copy costs as much as deserialising, `zerocopy` avoids it [05 §9.1]; pure-Rust `roaring` copies on deserialize [05 §10.3]; Rust versioned-store building blocks are experimental or beta in 2026 [04 §11]; "keep the core small and owned" [03 §8.2]. |
| 2 | **Every node stores typed information; typed fields incl. boolean `done`** | Fixed 60-byte columnar header (kind, status, resolution, priority, criticality, confidence, authority, flags, counters) plus a tagged-varint field block per kind (bool, int, f64, enum-with-lattice, text, set, ref); schema as data with weakening/strengthening rules. `done` is exposed as a typed virtual field derived from `status` (setting `done=true` performs the guarded transition), never stored twice (§3). | Beads #6105: three commands counted ready/blocked differently because state was stored twice [06 §2.2]; TerminusDB weakening/strengthening [06 §4]; the role templates are a ready-made schema [01 §6.1]. |
| 3 | **Versioned like git: branches that can be merged, etc. (R1)** | Own commit DAG with `main`, `lane/*`, `plan/*`, `merge/*`, `tags/*`; `branch`/`checkout`/`--list`/`-d`/`diff`/`log`/`merge`/`tag`/`revert`/`cherry-pick`/`reflog`/`undo`/`op log`; per-client HEAD; every versioned datum (tasks, status, blockers, rules, notes, decisions, findings, verdicts, measurements, schema) branches; typed merge with conflicts as data (§5a). | R1 (owner update); jj conflicts-as-values and TerminusDB validate-then-advance [04 §3.8, §3.3]; the owner's register incidents (rulings lost across branches, RESOLVED resurrected as OPEN, 188 vs 191) demand typed merge, not text merge [02 §7.3], [01 §7]. |
| 4 | **Own VCS, independent of git (R2)** | Own object model (commits, changeset ops, blobs, refs, pins, reflog), own merge engine, own discovery (`--store`, `MOIRAI_DIR`, `.moirai` dir/pointer file); `.git`/`commondir` read textually as an optional hint; no `git` process or library in the core; moirai branches have their own lifecycle, optional explicit binding to worktrees (§5c). | Beads' phantom store in worktrees [03 §2.7]; custom refs are not fetched by default and `push --mirror` deletes them [08 §7.1]; git spawn costs 74 ms [M, 08 §2]. |
| 5 | **Git-compatible image: save to / load from git (R3)** | Deterministic mapping of moirai commits/branches/tags to git blobs/trees/commits/refs: `.moi` canonical text per node, two-level `uid` fan-out, trailers with a verifiable `Moirai-Commit`, refs under `refs/moirai/*` or a separate repo; incremental export with an in-store id map (`gitmap`) that is reconstructible from trailers; import of native and foreign (hand-edited, git-merged) commits validated by the same merge engine; explicit lossless/lossy table (§5b). | R3; jj cannot represent conflicts in git and its change-id header is not preserved by all tooling [D §12 S1, verified in 21 §5]; Dolt's `refs/dolt/data` and `--force-with-lease` CAS loop [08 §7.1]; hg-git/cinnabar keep explicit maps [D §12 S8–S9]. |
| 6 | **Subtasks (recursive decomposition)** | Subtasks are tasks with a `parent` edge (forest, depth ≤ 12), rollups `children_total`/`children_done`, containers never "ready to work", `ready_to_close` when all children are done; hierarchy shown as a computed path, never baked into ids (§3). | Hierarchical ids mix identity with position (Beads PR #5131, Task Master #795) [06 §9.1]; GitHub's 8-level sub-issue cap as a precedent [06 §7.2]. |
| 7 | **Blockers (B cannot start before A is done)** | `blocks` structural edges; acyclicity maintained with Pearce–Kelly over the combined precedence graph `blocks ∪ child→parent ∪ implied exogenous edges` (I5′); `open_blockers` counters and a `ready` bitset maintained eagerly for affected nodes; `blocking --ids` is one bitmap scan; `gates` (verdict → task) constrains completion, not start (X5). | Beads deadlock through parent/child + blocks (#6506, PR #5131) [06 §2.2, §7.2]; PK best all-round [06 §7.1]; recursive SQL cycle check at 120 GB [03 §2.10]. |
| 8 | **CLI** | Line-oriented, ids first, deterministic order, `--ids`/`--json v1`, explicit drop footers, empty results exit 0, distinct exit codes 0–9, bodies via stdin/`@file`, no ANSI/prompts off-TTY; every verb takes `--branch`/`--lease`; versioning and image verbs are CLI-only orchestrator rituals (§7.1). | Claude Code treats exit 1 as failure and shows ~10k chars [07 §6.2]; PowerShell 5.1 strips quotes from argv [07 §5.2]; Beads recommends CLI + hooks over MCP for token cost [03 §8.1]. |
| 9 | **MCP server** | `moirai mcp` on rmcp (dual-era, `current_thread`), ten tools (`brief`, `pack`, `get`, `find`, `claim`, `complete`, `remember`, `write`, `changes`, `branch`), compact text output, every tool takes `branch` and validates it against `lease`, stamp hook on write tools only, role write policy keyed on the dispatch label (§7.2). | Three of nine roles have no Bash [07 §5.4]; tool search loads only names up front (≈216 chars) [07 §5.1]; `structuredContent` quirk [07 §2.6]; `mcp_tool` hooks cannot rewrite input, so the stamp is a command hook and must be narrowed to writes [22 §10 W-all-1], [20 §1.4]. |
| 10 | **Claude Code skills / harness integration** | Skills `moirai` (core), `moirai-orchestrate`, `moirai-report`, `moirai-branches`; hooks SessionStart/UserPromptSubmit/SubagentStart/PostToolUse(Agent)/SubagentStop/PreToolUse(stamp); plugin packaging with the binary installed separately; the dispatcher pattern (`claim` in bulk → `args` → `apply` with idempotency) as the Workflow default (§7.3–7.5). | SessionStart cannot call MCP tools [07 §4.1]; hook strings capped at 10,000 chars [07 §4.1]; Workflow scripts have no filesystem and resume re-runs completed agents [07 §4.1], [02 §12.7]. |
| 11 | **Synchronous references / "node 40 deleted, everyone knows"** | L0: one commit walks the reverse CSR, applies per-edge policies, writes tombstone, `affected` list and change-feed entry atomically; L1: every process reads `HEAD` before every operation and replays the tail (monotonic reads keyed by `committed_lsn`); L2: `changes --since` feed, hook deltas; L3: tombstone rendering in every tool result, CAS guards refuse stale writes with the current value. Across branches: applied where received, visible on request, `deleted` marker prevents re-dispatch (§5d.3, §6). | Datomic VAET reverse index [04 §8], [06 §8.1]; LLM context cannot be invalidated, so L3 is bounded staleness + preconditions [08 §6.1]; file watching is lossy on Windows [08 §4 W10]. |
| 12 | **Maximum performance** | Engine ≤ 5 ms per command at 1e5 on `main` (CI gate); open 0.3–3 ms; `get` 1–5 µs; `blocking --ids` 10 µs–3 ms; durable commit ~2 ms p50 (one data-only flush); first read of a 14-day lane ~5–10 ms with G15; merge of a 2k-op lane ≤ 50 ms at 1e5 (§8). | Measured primitives: open+map 0.22 ms, cached page touch ~1 µs, flush 1.73–2.0 ms p50 [M, 05 §2.2–2.3]; spawn dominates the CLI path, so O(1) open is the lever [05 §14.1]. |
| 13 | **Minimal RAM** | Private RSS ≤ 4 MB per CLI/hook process and ≤ 10 MB + 1 MB × min(active branches, 8) for the MCP server at 1e5 (CI gates); all data lives in read-only mapped pages shared by every moirai process; hot index ≈ 99 B/node (9.9 MB at 1e5, 99 MB at 1e6, evictable); no per-process buffer pools, no daemon holding the graph (§8). | 1.8 GB free with 16 agent processes at 3.5 GB private [M, 05 §2.4]; mmap shares pages and costs ~1 µs per cached touch [05 §5.3]; a RAM-resident daemon would need 100–300 MB at 1e5 [05 §16 C]; recomputed per-node bytes [20 §1.1]. |
| 14 | **Windows 11 first, portable later** | Byte-range locks only on `LOCK` (lock bytes past EOF), no data-file locks, `NtFlushBuffersFileEx(DATA_SYNC_ONLY)`, mapped files immutable and never truncated, ≤ ~12 stable files plus pinned sets, no rename on the commit path, bounded retries on errors 5/32, refuse network/OneDrive paths, quiet mode, signed binary at a stable path; Unix builds swap `fdatasync`/`fcntl`/`mmap` only (§4.5). | [05 §6.1–6.4], [08 §4 W1–W12]; Defender charges per file opened [05 §6.4]. |
| 15 | **Serve the owner's multi-agent workflow** | Node kinds mirror the role templates; findings are objects across rounds; loop termination is a query (`stats loop`); packs replace HDR blocks; leases are run-scoped for dispatcher claims; verdict routing as `phase_state`/`return_to`; a live cross-lane view (`--across`, `lane conflicts`, `settled` markers); `merge-check` before the code merge (§7.6). | [01 §7], [01 §8.3], [02 §11–§12], [07 §8]. |

---
## 2. Decision record (T1–T16; T3′ replaces T3)

Each row: the decision, where it comes from, what was rejected and why, and the concrete observation that would reopen it.

### 2.1 T1 — Materialized-state structure

**Decision.** Immutable, sealed, read-only-mapped columnar segments (one base + tiered delta segments) plus a bounded in-process overlay replayed from the log tail, with the op log as canonical history. The [20]/[21] rules are part of the format, not options: 32-byte record header with LSN, store epoch and xxh3 (X2); readers never read past `HEAD.committed_lsn`, only a writer scans forward, adopts, re-flushes and republishes before evaluating idempotency (G2/F-A1/F-B7); the ref move is inside the commit record (N3); commit index per sealed `hist` file (G3); bodies in the unmapped log tail with their own byte threshold (G7); delta checkpoints under the maintenance byte, writer byte held only for publish (G9/X9); rollup only on explicit `gc` or in the resident MCP server after a request (F-A3/F-B4); refs, pins and client heads out of the 4 KiB `HEAD` slot into log-folded tables (G18/F-D3); per-ref op index in every commit header and every `Checkpoint` record (G15/F-D1); zero-filled 64 MiB log extents (G11); epoch re-rolled on `init`/`restore`/`repair` (G25).

**Chosen from.** [B §4.1–4.8] as adopted by [D §4]; A's frozen bitsets and delete-pending GC [A §4.4, §4.10]; C's body-in-tail rule [C §4.2].

**Rejected.** *One preallocated file with copy-on-write B+tree pages + two meta slots* [04 §9 A], [08 §8.2], [05 §16 B]: the fastest proven point read (LMDB 0.64 µs, MEASURED-EXT [05 §4.2]) but "the hardest to build from scratch" (allocator, free lists, split/merge, multi-process page reclamation, reader registry) and 12–16 KiB per retained version [05 §16 B], [04 §3.14]; mapped growth needs remapping on Windows; no advantage for a read-mostly store of 10 MB–1 GB whose working set fits RAM [05 §5]. *C's full base rewrite at every checkpoint* [C §4.1]: 0.1–0.3 s inside the writer lock at 1e5, deterministic writer failures with a 250 ms timeout (F-C1/F-C2). *A RAM-resident daemon* [05 §16 C]: idle RAM = dataset (100–300 MB at 1e5), breaks the RAM requirement.

**Revisit trigger.** M1 measurement at the owner's real update mix showing a point read > 50 µs after a full 4,096-op tail, or a delta checkpoint > 100 ms at 0.5 M nodes; or a hard requirement for uniform O(depth) whole-graph time travel. Then Option B of [05 §16] behind the same lock protocol, with Sanakirja-style refcounted forks for branches [04 §3.10].

### 2.2 T2 — Process model

**Decision.** Purely embedded multi-process is the contract. Writers take an exclusive byte-range lock (`LOCK` byte 0) with a **blocking `LockFileEx` wait on an overlapped handle + `WaitForSingleObject(2 s)` + `CancelIoEx`** (G1), no sleep-backoff; readers take no locks; maintenance holds byte 2; nothing has threads or timers, so idle CPU is zero without a flag. The session's stdio MCP server is an ordinary long-lived client with warm maps and an LRU of ≤ 8 branch overlays (G19). An opportunistic leader (leader byte 1, named pipe with DACL, pipelined group commit, broadcast, auto-keyed forwarding, G13) is milestone M6 and may never be built. Quiet mode: no automatic checkpoints; hard cap at 8× the tail threshold after which one bounded delta checkpoint runs anyway (G10).

**Chosen from.** [A §2 T2, §6.1–6.2]; [08 §9 C] staged so that stage 1 (A's protocol) is the whole of v1; [D §2 T2].

**Rejected.** *Auto-started per-repo daemon* [07 §9.1]: daemons are the recurring failure in prior art (Beads deleted ~24k LOC of daemon, then proposed one again for per-call cost; Claude Code's own Windows daemon has a pipe bug; job-object kills) [08 §5], [03 §8.2]; a daemon still pays the CLI spawn and saves only ~1 ms of open [05 §14.2]. *Leader in v1* [C M3]: bundles pipe security, forwarding, failover into the first adoption milestone (F-C5, [22 §3.3 issue 4]). *Sleep-backoff writer wait* [A §4.6], [B §4.5], [C §4.4]: convoy under a 16-agent burst, p99 100–300 ms est. (F-A2, [20 §1.5]).

**Revisit trigger.** Writer-wait p99 > 50 ms with 16 writers *after* G1; MCP tool p50 > 5 ms from overlay catch-up; the M0 Defender close-cost measurement (G8) showing CLI commits > 20 ms (then bring the leader forward and make the CLI forward to it); or a harness push channel that reaches the model [07 §2.3].

### 2.3 T3′ — Branch model under R1

**Decision.** Full branching. `main` is a branch; `lane/<name>` (work), `plan/<name>` (coordination read-only, see N9), `merge/<name>` (staging) and `tags/<name>` are ref kinds; every node, field, edge, body and schema item branches. A branch view is `SEG(pin) ⊕ ops(main, (P, fork]) ⊕ ops(X since fork)` where a `sync` commit expands by reference to `ops(main, (M_{k−1}, M_k]) ⊕ resolutions_k` (G17). Branch ops are found through the per-ref index (G15), so the first read of a branch is O(its own commits), not O(store traffic since the fork). Promotion of a branch to its own delta segment is triggered by op count **or age** (G16) and is deferred to v1.1 because the owner's lanes are ~1–3k ops [D §5a.3]. HEAD is per client (`--branch` → `MOIRAI_BRANCH` → `--lease` (the lease's branch) → dispatch marker → `--client`/directory binding → git-worktree hint binding → `config.default-branch`); the lease resolves the branch before the directory does (D3). One moirai branch per *lane* opened by `lane open`, never automatically per git worktree or git branch; scratch and `wf_*` worktrees write to `main`. Store-level `settled`/`deleted` markers make cross-branch double dispatch impossible (N2/D1). Merges into `main` are sync-first (§5a.7).

**Chosen from.** [D §5a] with G15–G20 [20 §5], N1–N3/N7–N9 [21 §2], D1–D6 [22 §3.4].

**Rejected.** *Shared live trunk + provenance/ancestry scoping* [04 §9.3], [08 §7.2], [A T3], [C T3]: overruled by R1, and A/C's `exp/*` overlays "rebase on read" with no fork-point snapshot, so isolation costs O(trunk ops since fork) per read (`--pinned`) and a delete on trunk has no defined semantics for a branch op whose target vanished [21 §4.1]. *Two planes, coordination never branched* [B §3.1]: overruled by R1; B's `exp/` snapshot per branch costs 12–55 MB at 1e5 [20 §3]. *Automatic moirai branch per git worktree/branch*: 44 worktrees and 107 branches, many detached or harness-created, would pin segment sets and create merge rituals nobody asked for [08 §2, §7.2], [22 §11 decision 2]. *Copying `main`'s folded churn into every lane on `sync`* [D §5a.3]: 3–10× history growth at 50 lanes (F-D2).

**Revisit trigger.** One campaign in which staged merges and `sync` conflicts dominate the orchestrator's time even after N1/N7, or rules written on `main` chronically fail to reach lanes: then the opt-in `shared` field class (status and `blocks` live on `main` for all branches) [D §2 T3′] — never a return to planes. If M0 shows the interleaved-log scan is < 3 ms at the owner's real lane ages, G15 may be simplified (kept in the format anyway; it costs 8 B per commit).

### 2.4 T4 — IDs

**Decision.** Both. A store-global, never-reused, non-versioned `#N` (u32, allocated under the writer byte, dense row index) is the display identity and the key of every runtime table (leases, `settled`, `deleted`, G27); a 128-bit random `uid` (cold column, 16 B) is the identity in canonical commit hashes and in the git image. `#N` never appears in hashed image content (N4); an imported `id:` hint is honoured only when `N ≥ next_id` at import time, otherwise the alias map (N11, I35′). Mentions in text are parsed only for `#N < next_id` with the sigil rule (X6, [22 §2.4]). No kind prefixes, no dotted hierarchy.

**Chosen from.** [D §2 T4], [06 §9.2], [B T4].

**Rejected.** *128-bit ids only with short hash aliases* [03 §8.1], [04 §0.5]: ~24 tokens per UUID and 5–10× more Haiku errors [06 §9.1]; dense arrays and bitsets need dense ids [05 §10.4]. *`#N` only* [06 §9.2] with a 64-bit uid [A T4]: R3's round trip needs a store-independent identity; 64 bits is too small for that role (birthday risk across stores). *Hierarchical dotted ids*: position ≠ identity bugs (Beads PR #5131, Task Master #795) [06 §9.1].

**Revisit trigger.** Cross-machine writers in v1 → `uid` primary, `#N` a per-store alias from day one (all four proposals agree).

### 2.5 T5 — Deletion and "node 40"

**Decision.** Hard delete in the current state (row keeps only `deleted`), tombstone in history with the full before-image, one commit over the reverse index (O(degree)). Per-edge-kind policies: `parent` restrict (or `--cascade`/`--reparent`); `blocks`/`gates` out of the deleted node are **re-pointed** with `--replaced-by`, otherwise left as a **flagged** structural edge that keeps the dependent out of `ready` until `resolve` (X4 — never silent unblocking); `answers`/`scoped_to`/`duplicate_of`/`runs_in` restrict or reassign; historical kinds keep the dead id and render the tombstone; sources of `derived_from`/`cites` become `suspect`. `rm` refuses while **any** live lease covers the node on any branch unless `--release` (N8, I32′); `rm --dry-run` prints the impact report [A §7.1]. A transitive `suspect` closure has an op budget (10k) beyond which a violation record is written [20 §7 T5]. Across branches: the delete applies on every branch that receives it (commit, `sync`, `merge`, `import`) atomically; other branches see it with `--across` and through the `deleted` runtime marker, which stops them dispatching or listing the node as a live blocker (§5d.3). At merge: read-only elsewhere → delete wins; modified elsewhere → `DeleteVsModify` conflict value (`delete-wins`/`resurrect` policies per kind); new structural edge elsewhere → `DanglingEdge` violation, staged. Tombstones are exported as files, so "deleted ≠ absent" survives git merges.

**Chosen from.** [D §2 T5, §5d.3], [B T5], [C §3.3 `holds`], [06 §8.2–8.3].

**Rejected.** *Drop-and-notify for `blocks`* [A §3.3], [B §3.4]: hands a task to a developer whose prerequisite is open (X4, A's own walkthrough step 13). *Soft delete as a status*: leaks into every filter and weakens references; unnecessary in a versioned store [06 §8.1]. *Refuse any delete while referenced* ("strike, never delete" taken literally): friction for agents; the same effect is available as `restrict` on the structural kinds that matter.

**Revisit trigger.** Owner rules that a deleted blocker always means "no longer required" → drop-and-notify becomes the default policy for `blocks` (one schema row).

### 2.6 T6 — Bodies

**Decision.** Tiered: title (≤ 200 B) and a one-line abstract always in the store; bodies ≤ 64 KiB inline as content-addressed (BLAKE3-128), zstd-dictionary-compressed blobs deduplicated across revisions and branches, written first into the unmapped log tail and copied to sealed `blobs.NNNN` at checkpoint (G7); larger artifacts as `artifact` nodes (path + sha256 + bytes + kind + excerpt). In the image the body is the tail of the node's `.moi` file (text, diffable).

**Chosen from.** [B T6], [D §2 T6], [C §4.2].

**Rejected.** *Pointers only into repo/scratchpads*: diff3 and the verbatim-removed-text guard need the text in-store [01 §2.1]; scratchpads are session-scoped and reach 7.8 GB, so pointers rot [02 §8.3]. *16 KiB cap* [C T6]: too small for 24 KB plan sections [01 §7 L12]. *Store everything*: 1–3 GB/year on a disk-starved machine [02 §12.6].

**Revisit trigger.** Measured dictionary ratio < 2× on real notes (M0) and disk pressure → lower the cap to 16 KiB; owner wants prose only in the repo → pointer-only and text merge rules dropped.

### 2.7 T7 — Merge semantics

**Decision.** Typed per-key three-way merge. Keys: `(uid, field)`, `(uid, kind, uid)` edges, `(uid, existence)`, `(uid, parent)`, `(uid, body)`, schema items. **The base of a key is its value at the LCA state** (N1, I25′), never "the earliest before-image". Rules: status = lattice join for forward moves, incomparable or side-state vs forward → `StatusFork`; scalars = one side equals base → take the other, else `FieldEdit`; counters = `Incr` ops summed, never conflict (X11); sets = add-wins with removals relative to base; text = line diff3 (`TextHunk` on overlap; the removed-text guard on sections); `parent`/`order` = Kleppmann move in HLC order, cycle-creating move skipped and logged; existence = `DeleteVsModify`; `supersedes` ≤ 1 active (`SupersedeFork`); owner-authority fields = `main` wins when merging into `main`, else `OwnerFieldEdited`; coordination fields from `plan/*` dropped as `PlanStatusIgnored`. **Value conflicts land** as conflict values (jj) unless `--strict`; nodes leave `ready`. **Structural violations never land**: `DanglingEdge`, `Cycle`, `HierarchyCycle`, `IdCollision`, `SchemaConflict`, `RemovedTextNotInBase`, `ImageParse`, `NotFound` (N10) are staged on `merge/<src>`. Post-merge validation order: apply `parent` moves → re-derive implied exogenous edges → check every added `blocks`/`gates` and implied edge with PK, full Kahn above 1,000 touched precedence edges (N16, I37′). Multiple LCAs: exactly one rule — the newest by generation number (ties by commit id) (N7, I31′); merges into `main` are sync-first so the daily path has a unique LCA. `revert`/`cherry-pick` stage on `NotFound` and record `DATA` mismatches as `FieldEdit` conflict values (N10, I34′).

**Chosen from.** [B §5.4], [D §5a.7–5a.8], [04 §5], [21 §2, §6 T7].

**Rejected.** *Always land, including structural classes* [A §5.4], [C §5.5]: admits a cycle onto a ref and breaks Pearce–Kelly's precondition (X10); dangling structural edges at a head violate I2. *Blocking merges by default* (Dolt-strict): autonomous agents would stall on every text hunk; `--strict` remains a flag. *Recursive virtual base for criss-cross in v1*: unnecessary on the daily path once merges into `main` are sync-first; scheduled for v1.1 if cross-lane merges show spurious conflicts. *Auto-flip of finding status from a refutation* [C I6]: parallel refuters disagreeing must be a `StatusFork`, not a last-writer race (S12).

**Revisit trigger.** Owner prefers "always land": acceptable only with topological-order-free fallbacks for flagged components; owner prefers strict: flip the default flag.

### 2.8 T8 — Durability

**Decision.** Durability classes. `durable` (every graph mutation, claim/complete/release, delete, ref move, client-HEAD move, `settled`/`deleted` markers, `apply` batches): append → one `NtFlushBuffersFileEx(FLUSH_FLAGS_FILE_DATA_SYNC_ONLY)` → publish `HEAD` without a flush (1PC+C; the log is truth, `HEAD` a cache). `lazy` (heartbeats, cursors, session marks): appended, flushed by the next durable commit, documented as lost after power loss (F-C8 wording). fsync failure is fatal for the process; the next writer recovers. Log extents are zero-filled once at creation with one full `FlushFileBuffers` so later commits are true overwrites (G11). No group commit in v1; pipelined group commit only in the M6 leader (G13).

**Chosen from.** [A T8], [B T8], [C T8], [D §2 T8], [20 §7 T8].

**Rejected.** *Per-commit `FlushFileBuffers`* (metadata flush): slower for no durability gain [05 §2.2]. *Group commit across processes in v1*: needs a leader; the workload is bursty and low (~19 git commits/day, 16-agent bursts) [08 §1]; the blocking lock wait removes the convoy [20 §1.5]. *`FILE_FLAG_WRITE_THROUGH`*: 0.14 ms but not trusted on consumer drives [05 §6.2].

**Revisit trigger.** Sustained flush p99 > 10 ms under build load (measured max already 13.7 ms) → pipelined group commit in the leader; owner accepting a 100 ms loss window for all writes → every commit lazy with a deadline enforced by the committing process.

### 2.9 T9 — Agent surface

**Decision.** One core, two front ends. CLI + skill + hooks are primary; a ten-tool MCP server serves the three Bash-less roles and typed writes. Every CLI verb takes `--branch`/`--lease`; every MCP tool takes `branch` (model-typed from the dispatch marker `moirai:task=#89 lease=L-18 branch=lane/l5np role=developer`) validated against the lease (D2/D3); the resolved branch is printed on the first line of every result. The PreToolUse stamp hook (a `command` hook, +15–73 ms spawn) matches **write tools only** (G6); reads are unstamped. Context packs are budgeted in characters (Cyrillic ratio measured in M0, [22 §2.5]), per-class quotas, degrade before drop, empty `applies_to` = `*`, global critical-rule count in the header, `~main` markers for unmerged critical rules, conflicted knowledge rendered as one line with the base text (N15), `pack` is a pure read. Hooks: SessionStart (command, brief ≤ 8,000 chars), UserPromptSubmit (delta), SubagentStart (role pack), PostToolUse(Agent, async: agentId → task/lease), SubagentStop (lease safety net + I12 artifact check), PreToolUse stamp; optional `mcp_tool` PostToolBatch delta later. Dispatcher pattern for Workflows: run-scoped leases (`--ttl run`), `apply` derives its branch from the run (D6). Role write policy keyed on the dispatch label with `agent_type` as fallback ([22 §2.1]).

**Chosen from.** [C §7.2–7.5], [B §7.1], [D §7], [22 §9].

**Rejected.** *MCP-only*: SessionStart cannot call MCP; hooks and bootstrapping need the CLI [07 §4.1]. *CLI-only*: three roles have no Bash [07 §5.4]. *Stamp every MCP call*: +15–73 ms per read [20 §1.4]. *`pack` as a write* (C's `consumed` edges): orphans for Workflow agents without a `run` node [22 §3.3 issue 2]; kept as opt-in `--record-run`. *`lease` as a node kind* [C]: consumes `#N` for ~40k leases/year [22 §3.3 issue 6]. *28 node kinds* [C §3.2]: Beads' accretion; a 60-line skill cannot teach them [22 §3.3 issue 1].

**Revisit trigger.** A harness release letting `mcp_tool` hooks return `updatedInput` → stamp everything for free; hooks proven not to fire for Workflow `agent()` calls → dispatcher-only (the SubagentStart pack is dropped, nothing else changes).

### 2.10 T10 — "From scratch" boundary

**Decision.** Hand-written: on-disk formats, log and segment codecs, HEAD/LOCK protocol, overlay, columnar graph and CSR, frozen bitsets, term dictionary (tier 2), Pearce–Kelly, derived state, delete policies, lease/idempotency tables, commit DAG, refs/pins/reflog, branch views, merge engine and validators, diff3, `.moi` encoder/decoder, tree/commit mapping, `gitmap`, export/import semantics, CLI, pack/brief. Allowed leaf crates: `zerocopy` (validation-free views), `blake3`, `xxhash-rust`, `zstd` (codec only, statically linked; `lz4_flex` if C is refused), `windows-sys`/`libc`, `serde_json`; `sha1`/`sha2` in the image module; `rmcp` + `tokio` (`current_thread`) only in `moirai mcp`; dev-only: a property-testing crate, `hyperfine`. Excluded from the product: SQLite, redb, LMDB/heed, fjall, sled, `gix`/`git2`, `petgraph`, `roaring`, `fst`, `tantivy`, any async runtime in the core. **Storage engines are allowed as test oracles and as the S0–S5 throw-away backend behind the engine trait** (owner decision #2). **Git object I/O**: v1 uses `git fast-import` (writer) and `git cat-file --batch`/`rev-list`/`diff-tree` (reader) behind an `ImageBackend` trait when git is present; a hand-written loose/pack/idx/bundle layer (with `zlib-rs` or `miniz_oxide`, measured in M0) is v1.1 for "no git installed" export/import (D10; owner decision #3).

**Chosen from.** [D §2 T10] minus the hand-written git object layer in v1; [22 §5.3, §8 T10].

**Rejected.** *`gix` in the product*: large dependency; push is unimplemented and SHA-256/reftable parity is open work (verified [21 §5 item 12]); acceptable only as an optional import oracle in tests. *Hand-written pack layer in v1* [D §5b.8]: 4–6k lines plus fuzzing for a capability R2 does not require of the image and the owner's machine (Git for Windows present) does not need yet [22 §3.4 D10]. *`roaring` crate*: pure-Rust version copies on deserialize [05 §10.3]. *`fst`*: not needed below ~20k nodes; last released 2021 [05 §13].

**Revisit trigger.** Owner allows a C engine (LMDB via `heed`) as the materialized state → M0 shrinks by a third and T1 becomes Option B. Owner forbids any C code → `lz4_flex` at a worse ratio.

### 2.11 T11 — Search

**Decision.** Exact ids, typed GitHub-style filters (`kind:task status:open prio:<=1 area:net done:false suspect:true`), graph expansion verbs (`tree`, `blockers --transitive`, `neighbors`, `notes --path`) with node budgets; FTS tier 1 = brute-force scan of titles and abstracts (opt-in bodies) at any scale; tier 2 = hand-written front-coded term dictionary + delta-varint postings per delta segment above ~20k nodes (v1.1); results carry `seq` and `branch`. Embeddings out of the core; an optional external embedder process later.

**Chosen from.** [A T11], [B T11], [D §2 T11].

**Rejected.** *Text2Cypher / a query language*: ~50 % execution accuracy for GPT-4 [06 §12.2]. *tantivy*: ≥ 15 MB per indexing thread and many files [05 §13]. *Vector-first retrieval*: lossy on agent trajectories; a filesystem-plus-grep agent beat mem0-graph on LoCoMo [03 §6.3].

**Revisit trigger.** Owner-accepted RAM budget for a local model as a separate process, and recurring "what did we decide about X" misses under lexical recall.

### 2.12 T12 — Schema

**Decision.** Fixed core: 13 node kinds (`task`, `doc`, `note`, `rule`, `decision`, `question`, `finding`, `verdict`, `measurement`, `artifact`, `run`, `lane`, `area`) with C's `task.phase_state` (14 states of [01 §5.1]) and `verdict.return_to`, plus the `gates` edge (verdict → task). Schema as data on every branch (`kinds`, `fields` with type and lattice order, `edges` with class, delete policy, acyclicity and cardinality) exported as `schema/*.moi`; weakening changes (add kind/field/enum value) apply instantly and merge freely; strengthening changes are a `Schema{strengthen}` op that needs `moirai migrate` on the branch and re-validation at merge (`SchemaConflict`); enum integers never reused; symbols never GC'd. Campaign/phase/rung/candidate/checkpoint of [02 §12.1] are `task` (labels), `task`, `task`, `decision{rejected, revive_condition}`, `note{checkpoint}`. `verdict derived_from finding` edges are written by `remember{verdict}` from its inputs (N14).

**Chosen from.** [A §3.2], [D §3 D5/D13], [C §3.2] fields, [06 §4].

**Rejected.** *28 kinds / 35 edge kinds in v1* [C §3.2–3.3]. *Fully schemaless*: typos and no invariants [06 §3.1]. *Kinds hard-coded only*: project extensions would need a release.

**Revisit trigger.** The owner wants kinds added weekly without a release — extensions are already data; only fast columns need a release.

### 2.13 T13 — Scope of v1

**Decision.** The smallest adoptable slice is S0–S2 of §9 (format spec + oracle + measurements; trunk graph + CLI on `main`; packs/brief/hooks/skill + one-off import of standing rules, pins, lanes, open questions), adopted on one real campaign. R2 is satisfied at S1, R1 at S3 (branches + merge for the next campaign's lanes), R3 at S4 (image at checkpoint granularity via fast-import). MCP is S5; the from-scratch engine is S6 behind the trait with the simulator and kill loops as its exit gate. v1 ships `tag`, `undo`, `revert`, `reflog`, `cherry-pick`; `rebase --onto`, `op restore`, branch promotion, hand-written pack layer, bundles, SHA-256 images, `--with-oplog`, tracked-directory and orphan-branch destinations, the leader and `watch`, FTS tier 2 and the `shared` field class are later.

**Chosen from.** [22 §7.3] with D's milestone content [D §9]; [B §9 M0] oracle idea.

**Rejected.** *D's M1 as written* (engine + refs + branches + overlays + promotion + checkout + undo + revert + tag + GC before any CLI) [D §9]: ~40 units before feedback (D8). *Lanes before MCP* [B §9]: the three Bash-less roles wait behind the merge engine — kept, but S5 is small and S1–S2 already serve them through the orchestrator's `apply`. *Engine first, adoption last* [A §9], [C §9]: 60 % of the work precedes the first adoption gate [22 §2.7].

**Revisit trigger.** Owner's first target is publishing to git → S4 before S3 (N4/N5 must land first). Owner rules out the oracle backend → S6 becomes S1's prerequisite.

### 2.14 T14 — Git independence under R2

**Decision.** Discovery, first hit wins, a miss never creates a store: (1) `--store <dir>`; (2) `MOIRAI_DIR`; (3) walk up for `.moirai` — a directory containing `HEAD`, or a pointer file whose first line is `moiraidir: <path>` and second line `store-id: <128-bit hex>` (a stale pointer whose store id does not match is reported, not followed); (4) hint only, if `config.discovery.git-hint` (default true): a `.git` file/dir read textually (`gitdir:` then `<gitdir>/commondir`) and `<git-common-dir>/moirai/` tried; (5) exit 7 naming the expected paths, the hinted store if any, and `moirai init --link <store>`. `moirai init` refuses to create a store where step 4 resolves to an existing one unless `--force --shadow`; `doctor store` lists shadowing directories (D4). `moirai init --link` drops pointer files; `moirai worktree bind <dir> <ref>` records directory → branch bindings in the runtime `HEADS` table; `lane open` bundles `branch` + `bind` + the lane node. Provenance (`git.head`, `git.branch`, `git.worktree`) is read from `HEAD`/`refs/heads/*`/`packed-refs` textually (~60 lines); ancestry for `stale` reads `objects/info/commit-graph` generation numbers when present, else the pack reader of the image module, cached as lazy facts in the log (C's cache, G27); if no repo is discoverable `stale` reports "unknown". Optional conveniences: `hook git-post-merge` (runs `merge-check`), `post-checkout` binding refresh. Network transport of the image spawns `git` if present, else prints the command.

**Chosen from.** [D §5c], [B §4.9] init/no-auto-create, [C §4.7] cache, [22 §9 graft 3].

**Rejected.** *Store only in `<git-common-dir>/moirai/`* [A §4.1], [C §4.1]: requires a git layout (R2). *`git merge-base --is-ancestor` on the query path* [B §3.6]: 74 ms per call, 2.2 s inside a hook for 30 pins (F-B6). *Auto-following git branches*: [08 §7.2] arguments 1–5.

**Revisit trigger.** None on this fork.

### 2.15 T15 — Git image under R3

**Decision.** D's format with the corrections of [21] and the hygiene of [20]: `.moi` canonical text per node (§5b.2) under `nodes/<h1>/<h2>/<uid>.moi` (2×2-hex fan-out on `uid`, fixed), out-edges in the source file, tombstones as files, **no `id:` line and no `Moirai-Seq` trailer** (N4); counters as append-only `incr` ledger lines (N6); the ordinary `field` line omitted while a `conflict` line exists for that key (N12); transitions absent→live = `Create`, live→tombstone = `Delete`, tombstone→live = `Undelete`, live→absent = foreign `Delete{image:file-removed}`, tombstone→absent = `TombstoneRemoved` hint (N5); canonical parent references by the parents' **stated** ids, a per-commit `verified` bit, a demotion affects one commit only (N5, I29′); foreign two-parent commits are imported as moirai merges recomputed by the typed engine over the two parents' states, with the git tree used only to resolve the text conflicts a human chose (N6, I30′); `import-checkpoint` kind for checkpoint-granularity commits (N13a); the destination ref's oid recorded as "last seen" on import (N13b); foreign `hlc = max(committer_time, max(parent.hlc)+1)` (N13c); every past `.moi` encoder retained and the format version each destination uses recorded (O5). Object format follows the destination; SHA-1 for anything that leaves the machine, SHA-256 allowed locally. Writer/reader: `git fast-import` / `git cat-file --batch` in v1 behind a trait; hand-written loose/pack layer later (D10). Hygiene when a hand-written writer exists: temp-then-rename for every object/pack/idx, git's `<ref>.lock` protocol plus read-after-write verification for refs in a live `.git`, reftable detection and delegation, packs above ~8 objects, image-repo config written by moirai (`gc.auto = 0`, `gc.autoPackLimit = 0`, `pack.threads = 2`, `pack.windowMemory = 64m`), `git gc` only through `moirai image gc` outside quiet windows, lazy parent-tree reads, explicit export/import RAM budgets, exports do not hold the maintenance byte (G21–G24, G26). **Defaults**: destination = separate bare repo `<parent of the main worktree>/<project>-moirai.git`; refs = `main` + `tags/*`; granularity = one checkpoint per merge into `main` plus one per day and on explicit export; `--granularity commit` and `--refs lane/*` opt-in per destination; `refs/moirai/*` in the project repo as an optional second destination at checkpoint granularity only (D9).

**Chosen from.** [D §5b] + [21 N4–N6, N12, N13, O5] + [20 G21–G26] + [22 D9/D10, §5.3].

**Rejected.** *Binary `hist`/segment blobs under `refs/moirai/data`* [A §5.7], [B §5.6], [C §5.6]: a backup, not a readable, diff-friendly image with import of git-side edits (fails R3). *Tracked directory as the default*: git-side text merges of `.moi` files are expected there; only safe at checkpoint granularity and after N6's ledger form. *1:1 commits for every lane into the project repo*: ~4 GB/year undeltified before `git gc` on a disk-starved machine (D9, F-D7). *jj-style non-standard commit headers*: not preserved by all git tooling [D §12 S1].

**Revisit trigger.** Owner wants PR review of the image above all → tracked-directory destination at checkpoint granularity becomes the default and N6's ledger form is mandatory; owner wants per-commit archaeology → `commit` granularity on the separate repo.

### 2.16 T16 — Coordination under branching

**Decision.** Versioned per branch: nodes, all typed fields including `status`/`done`/`resolution`/`assignee`/`phase_state`, all edges including `blocks`/`gates`/`parent`, bodies, schema, conflict values. Store-level runtime, each justified as "who is doing / has done what now", never history: **leases** `{#N, holder, token, expires, run, pid, branch}` keyed by `#N` (G27), visible from every branch, `claim` requires live + ready on the claimer's branch and records that branch, `complete`/`set --lease` write on the lease's branch (`--move-lease` needs an explicit branch and warns); **`settled`** `{#N, branch, commit, holder, outcome, hlc}` written by `complete` in the same commit, honoured by `ready`/`claim`/`blocking`/`brief` on every branch as "done on `<branch>` (unmerged)" until the commit is an ancestor of the reader's tip, turned into a triage line on branch delete (I26′); **`deleted`** `{#N, branch, commit}` likewise for `rm`; **fencing tokens** monotonic store-wide, expiry never bumps the token (I17′); **idempotency results** keyed by (key16, payload hash, branch), replay on another branch is exit 9 *unless* the original branch has been merged into the caller's branch or deleted after merge, in which case the original result is returned (N13e); **change feed** `seq` store-wide with `ref` on every entry, `changes --since` defaulting to the caller's branch ∪ `main`; **`next_id`**, **client HEAD bindings** (`session:<id>` keys expire with the idempotency window), **quiet flag**, **pins**, **`gitmap`**, **alias map**, **ancestry cache**. `claimed` is derived from the lease table, never a versioned flag, never exported (N12, I36′). Leases on a branch that is deleted are released with a triage note (N13d). `undo` takes `--expect <tip>` by default (N13f). `plan/*` branches: `status`/`resolution`/`assignee`/claims read-only; `blocks`/`parent`/`gates` writable and validated (N9, I33′).

**Chosen from.** [D §5d.1] + [22 §9 graft 1, §8 T16] + [21 §6 T16, §8].

**Rejected.** *Versioned leases* (merged from a lane = a claim by a possibly dead process; meaningless on `plan/*`) [D §5d.1]. *Per-branch leases* by default: parallel branches would both build #12 [07 §7.4]. *Coordination on a shared plane*: R1.

**Revisit trigger.** Owner wants parallel what-ifs claiming the same task → key leases by `(#N, ref_sym)` with a cross-branch warning (no cost change).

### 2.17 Critique resolution ledger

Every fatal or serious finding of [20], [21] and [22], with its disposition. "Adopted" means the fix is part of the specification in §3–§7; "by construction" means the engine choice removes the finding; "rejected" gives the reason.

**[20] perf / RAM / Windows.**

| Label | Finding | Disposition |
|---|---|---|
| F-A1, F-B7 | readers adopt unflushed commits; republish order vs idempotency | Adopted: readers stop at `committed_lsn`; the next writer scans, re-flushes, republishes, then evaluates idempotency (§4.3). |
| F-A2 | writer convoy under sleep-backoff | Adopted G1: blocking overlapped `LockFileEx` wait (§4.1, §6). |
| F-A3, F-B4 | rollups inside a CLI/hook writer lock | Adopted G9: delta checkpoints under the maintenance byte; rollup only on `gc` or MCP-after-request (§4.3). |
| F-A4 | preallocation does not give overwrite-in-place | Adopted G11: zero-filled extents + one full flush per extent (§4.1). |
| F-A5 / G8 / U10 | Defender close cost on the log unmeasured | Adopted: M0 measurement; decides whether the CLI must forward to a leader (§8.2). |
| F-A6 | 7-day idempotency retention | Adopted: 30 days (§6.4). |
| F-A7 | `LOCK` diagnostics inside a lockable range | Adopted: diagnostics at offset 2048, never inside a locked range (§4.1). |
| F-B1 | mapped `HEAD` page read | Adopted: `HEAD` read only by `pread` (§4.3). |
| F-B2 / U15 | active-log commit-id side index unspecified | Adopted: the commit index is built when a log extent is sealed into `hist`; for the active extent the per-process overlay holds `(id16 → lsn)` for records it replayed (bounded by the tail) (§4.4). |
| F-B3 | body location before checkpoint | Adopted G7 (§4.1). |
| F-B5 | `watch` polls | Adopted: `watch` leader-only (M6); no polling process ever (§6). |
| F-B6 | `git merge-base` on the query path | Adopted: own commit-graph/pack reader + lazy-fact cache; never on the hot path (§5c). |
| F-B8 | per-process lane overlays | Accepted cost, bounded by G15 and the G19 LRU (§8). |
| F-B9 / U14 | `show @commit` in `hist` optimistic | Adopted: table states ≤ 0.1–0.5 ms per cold frame (§8). |
| F-C1, F-C2 | full base rewrite in the writer lock; 250 ms timeout | By construction: delta segments, maintenance byte, 2 s blocking wait. |
| F-C3, F-C9 | CSR `prop_idx`; 170 bitsets | By construction: sparse edge-prop side table; 13 kinds. |
| F-C4 / U7 | pure-Rust `roaring` copies | By construction: hand-written frozen containers (§4.4). |
| F-C5 | forwarded writes without keys | Adopted G13 for M6 (§6). |
| F-C6 | group-commit timer | Adopted G13: pipelined, no accumulation timer (M6). |
| F-C7 | reader registry aliases | By construction: delete-pending GC, no registry (§4.3). |
| F-C8 / U17 | "flushed at process exit" | Adopted wording: flushed by the next durable commit; lost after power loss (§6.4). |
| F-D1 / U21 / G15 / G16 | first read of a branch scans the whole log since the fork | Adopted: `prev_on_ref` in every commit header + per-ref lsn lists in every `Checkpoint` record; promotion by age as well as size (§4.2, §5a.3). Format item before M1. |
| F-D2 / U22 / G17 | `sync` copies `main`'s churn into every lane | Adopted: merge-by-reference `sync` (§5a.3, §5a.7). |
| F-D3 / U23 / U24 / U25 / G18 | `HEAD` slot cannot hold refs/pins; file count | Adopted: `HEAD` keeps scalars + `refs_lsn`/`pins_lsn`/`heads_lsn`; tables are log records folded into segment sections; pins refcount segment files, unbounded (§4.1–4.2). |
| F-D4 / U26 / G19 | MCP RSS per active lane | Adopted: LRU of K = 8 overlays; gate restated as 10 MB + 1 MB × min(active, 8); `--across` without a ref list uses promoted branches' `TOUCH` bitmaps only (§8). |
| F-D5 / U34 / G20 | ref CAS on `old` fails concurrent same-branch writers | Adopted: commit parent = branch tip after tail replay under the writer byte; `--if-tip` opt-in (§5a.2). |
| F-D6 / U28–U30 / G21, G22 | image writer hygiene; live-repo refs; reftable | Adopted for the hand-written writer (v1.1); in v1 `git fast-import`/`update-ref` implement git's own protocol (§5b.6). |
| F-D7 / U36 / G23 | loose objects per commit; `git gc` | Adopted: packs by default (fast-import produces packs), batched runs, config written by moirai, explicit `image gc` (§5b.8). |
| F-D8 / U27 / G24 | export/import RSS unstated | Adopted: lazy parent-tree reads; explicit-command budgets in the table (§8). |
| F-D9 / U31 / G25 | epoch protects nothing | Adopted: re-rolled on `init`/`restore`/`repair`; recycled extents zero-filled (§4.5). |
| F-D10 / G26 | exports hold the maintenance byte | Adopted: exports rely on pins + delete-pending grace; only the `gitmap` append takes the writer byte (§5b.6). |
| F-D11 / U35 | merge folds are O(store commits since LCA) | Adopted via G15: each side's fold is O(its own commits) (§5a.7). |
| F-D12 / G27 | leases keyed by `uid`; `HEADS` without expiry | Adopted: runtime tables keyed by `#N`; `session:<id>` heads expire (§5d.1). |
| F-D13 / G27 | ancestry through the pack reader | Adopted: `commit-graph` generation numbers when present (§5c). |
| G28 | M0 measurement additions | Adopted (§8.2). |
| U9 | MCP calls "0.1–5 ms" as configured | Adopted G6: stamp on write tools only; reads spawn-free (§7.2). |
| U11, U12, U13, U20, U32, U33 | claimed/unmeasured numbers | Carried as M0 measurements; the budget table marks them (§8). |
| U18, U19 | wording | Corrected in this document. |

**[21] semantics / correctness.**

| Label | Finding | Disposition |
|---|---|---|
| X1, X2b | exogenous-inheritance deadlock; `Move` invalidates classification | Adopted I5′: implied edges `{X→D : blocks(X,P) exogenous, D ∈ subtree(P)}` checked at write, `move` re-derives (§3.4). |
| X2 | flushed-but-unpublished commit overwritten | Adopted (as F-A1). |
| X3 | idempotency keys not payload-bound | Adopted I14′: key + payload hash + branch; exit 9 (§6.4). |
| X4 | delete-with-replacement unblocks open prerequisites | Adopted: re-point or flag (§3.3, §5d.3). |
| X5 | verdict-as-`blocks` deadlocks fix rounds | Adopted: `gates` constrains completion only (§3.3). |
| X6 | `mentions` to unallocated ids | Adopted: `#N < next_id` + sigil rule (§3.3). |
| X7 | expired-but-unreclaimed lease on resume | Adopted I17′: same-holder renewal; expiry never bumps the token (§6.2). |
| X8, S10 | `suspect` both derived and clearable; as-of mixing derived fields | Adopted: `suspect` purely derived from `(target state, edge.pinned_commit)`; as-of output carries no derived fields unless `--recompute` (I18′) (§3.5, §5a.6). |
| X9 | maintenance inside the writer byte | Adopted (as F-A3). |
| X10 | `Cycle` admitted onto a ref | Adopted: structural violations staged, never land (§5a.8). |
| X11 | `rev`/counters under scalar merge | Adopted: `rev_seq` u64 above both merge inputs; `Incr` ops; citations pin commit ids (§3.1, §5a.7). |
| X12 | two lanes supersede one rule | Adopted: ≤ 1 active superseder; `SupersedeFork` (§3.3). |
| X13 | what-if branches mark work done | Adopted: `plan/*` kind with a field write mask (§5a.1). |
| S1 | leases on a deleted task | Adopted with N8's resolution (refuse unless `--release`). |
| S2-B | question `answered` with no visible answer | Adopted: derived from a visible `answers` edge (§3.5). |
| S6 / N14 | verdict without derivation edges | Adopted: `verdict derived_from finding`; all-refuted → `suspect`, `gates` listed by `stale` (§3.3). |
| **N1** | merge base from before-images → spurious conflicts after `sync` | **Adopted**: base = value at the LCA state via as-of on the dst side's per-node chain; G17 additionally keeps `main`'s ops out of lane folds; property test I25′ in M3 (§5a.7). |
| **N2 / D1** | task completed on a lane re-dispatched from `main` | **Adopted**: `settled` and `deleted` runtime markers (I26′), written in the completing commit, honoured everywhere (§5d.1–5d.2). |
| **N3** | torn `RefUpdate` strands an adopted commit | **Adopted**: `ref_old` inside the commit record; adoption implies the ref move with CAS; `RefUpdate` only for non-commit moves; crash point between appends added to DST (I27′) (§4.2, §5a.2). |
| N4 | image deterministic per store | Adopted: no `id:` line, no `Moirai-Seq`; aliases in an unhashed side ref (I28′) (§5b.2, §5b.4). |
| N5 | canonicalisation slip cascades demotions; tombstone→live undefined | Adopted: `Undelete` transition; parent refs by stated id; `verified` bit; fuzz over revert/cherry-pick/undo (I29′) (§5b.6). |
| N6 | git text merge loses `Incr` | Adopted: `incr` ledger lines; foreign merges recomputed by the typed engine (I30′) (§5b.2, §5b.6). |
| N7 | criss-cross stated two ways | Adopted: one rule (newest-by-gen LCA, I31′) plus sync-first merges into `main` (§5a.7). Recursive virtual base deferred to v1.1. |
| N8 | `rm` vs live lease contradiction | Adopted: refuse unless `--release` (I32′). |
| N9 | `plan/*` cannot add `blocks` | Adopted: read-only = status/resolution/assignee/claims (I33′). |
| N10 | `NotFound`/`DATA` classes missing | Adopted (I34′); "depends structurally" defined in §5a.5. |
| N11 | `#N` hints re-bind numbers after GC | Adopted: honoured only if `N ≥ next_id` (I35′). |
| N12 | versioned `claimed`; conflict vs field line | Adopted (I36′; §5b.2 rule). |
| N13 a–f | checkpoint verification, two-store CAS, foreign `hlc`, branch delete with leases, idempotency across merged branches, `undo` CAS | All adopted (§5b.6, §5d.1, §5a.5). |
| N15 | conflict markers in packs | Adopted: one-line `~conflicted` rendering with the base text (§7.4). |
| N16 | moves vs implied edges vs PK order at merge | Adopted (I37′); the merge variant of X1 added to M3 tests (§5a.7). |
| Minors | epoch wording; "sandboxed agents may not run git"; branch-of-branch view | Corrected: epoch re-rolled (G25); the git-sandbox claim is dropped; `view(Y) = view(X)@fork_Y ⊕ ops(Y)` recursive (§5a.3). |
| [21 §5 item 17] | merge cost estimate assumed the wrong base rule | Adopted: the budget adds one as-of read per touched key on the dst side (§8). |

**[22] agent fit / buildability.**

| Label | Finding | Disposition |
|---|---|---|
| §2.1 | role policy keyed on `agent_type` misses Workflow agents | Adopted: dispatch label first, `agent_type` fallback, documented default; `doctor agents` reports unparseable role files. |
| §2.2 | 60-minute dispatcher leases vs multi-hour runs | Adopted: `--ttl run`, released by `apply`/`run close`/dead `bg_task_id`. |
| §2.3 | agentId → task mapping unverified for Workflow `agent()` | Adopted: 5-minute experiment in S0; dispatcher pattern is the only supported Workflow pattern until it passes; safety net is run-scoped. |
| §2.4 | `#N` mention parsing collides with issue numbers | Adopted (X6). |
| §2.5 | `chars/3.5` wrong for Cyrillic | Adopted: budgets in characters; ratios measured in S0. |
| §2.6 | rules with empty `applies_to` vanish | Adopted: empty = `*`; global critical-rule count in the header. |
| §2.7 / D8 | engine before the value of packs is validated; M1 overloaded | Adopted: S0–S5 on the oracle backend behind the trait; engine at S6; M1 split. |
| §2.8 | test infrastructure under-budgeted | Adopted: the simulator + kill loops + fuzzers are their own workstream at ~20 % plus the image's round-trip corpus. |
| §2.9 | the graph starts empty | Adopted: one-off import of the dozen standing rules, current pins, live lanes, open owner questions in S2. |
| §2.10 / D2 / D3 | branch identity from `cwd` alone | Adopted: explicit `branch` tool parameter; lease-resolved branch first; marker second; directory third; `--move-lease` explicit. |
| D4 | `init` shadows the shared store | Adopted: `init` guard, store-id pointer files, `doctor store` shadow report. |
| D5 | hooked `sync` mutates lane state | Adopted: hooks run `sync --check`; auto-apply only when the preview has zero conflicts and zero violations. |
| D6 | `apply` defaults to the wrong branch | Adopted: branch derived from the run (`run:<id>` → lane → `moirai_branch`); batch refused before any write if a lease's branch differs. |
| D7 | over-engineered verbs for v1 | Adopted in part: `rebase --onto`, `op restore`, promotion, bundles, SHA-256, `--with-oplog`, extra destinations deferred; `cherry-pick` kept (shares the revert/merge code path); `plan/*` kept as one read-only kind (the owner asked for re-planning branches). |
| D9 | 1:1 image for every lane is the wrong default | Adopted (§2.15 defaults). |
| D10 | hand-written git object layer not required | Adopted: fast-import/cat-file first behind a trait (owner decision #3). |
| D11 | `--across` over all refs | Adopted: `--across` defaults to the caller's branch + `main`; all-refs form uses promoted `TOUCH` bitmaps only. |
| D12 | merge-queue order | Adopted: `merge-check` → `moirai merge` (staging on violation) → `git merge` → `merge --continue` if staged. |
| D13 | spec hygiene | Corrected. |
| §9 graft 8 | clean `sync` re-forks the pin instead of appending | **Rejected** as the primary mechanism: it rewrites lane commit ids in a 1:1 image (the graft says so) and G17 already keeps the lane overlay small without rewriting history. Kept as an optional `sync --refork` for unexported lanes. |
| §9 grafts 10–12 | `notes --path`, `check #ID`, `export md`, developer-never-verdicts-own-task, `doc patch` write-time guard, `resource` mutex nodes | Adopted (§7.1). |
| §11 decision 3 (stamp every call) | — | Rejected as default (+15–73 ms per read); explicit `branch` parameter instead. |

---
## 3. Data model (final)

### 3.1 Node header (hot, columnar, one row per `#N`)

Every node has a fixed **60-byte** header stored as structure-of-arrays columns in segments (little-endian, `zerocopy` views, u32 row = `#N − 1`). Rows are never reused; a deleted node keeps its row with `deleted` set. Column choice follows [B §3.2] with the changes of [D §3 D3] (no `plane`, no `proposed`, `rev_seq` u64) and [21] N12 (no versioned `claimed`).

| Column | Type | Bytes | Meaning |
|---|---|---|---|
| `kind` | u8 | 1 | one of the 13 core kinds (< 64) or a project kind (≥ 64) |
| `status` | u8 | 1 | kind-specific enum (§3.6); `blocked` and container `done` are never stored |
| `resolution` | u8 | 1 | closed tasks and findings: completed, wontdo, duplicate, superseded, obsolete, rework |
| `priority` | u8 | 1 | P0–P4, scheduling |
| `criticality` | u8 | 1 | critical / high / normal / low, surfacing order |
| `confidence` | u8 | 1 | verified / observed / inferred / speculative; findings: confirmed / plausible |
| `authority` | u8 | 1 | owner / orchestrator / measured / research / agent |
| `flags` | u16 | 2 | bit0 `deleted`, bit1 `suspect`, bit2 `has_dangling`, bit3 `pinned`, bit4 `container`, bit5 `conflicted`, bit6 `archived`, bit7 `frozen`; **no `claimed`, no `proposed`, no `stale`** (all derived or runtime) |
| `rev_seq` | u64 | 8 | store `seq` of the commit that last touched this node **on the reading branch's view**; after a merge it is the merge commit's seq, above both inputs; CAS target (`--if-rev`) |
| `parent` | u32 | 4 | `#N` of the parent or 0 |
| `created_tx`, `updated_tx` | u32 × 2 | 8 | commit seq numbers (provenance lives on the commit) |
| `last_op_lsn` | u64 | 8 | head of the node's op chain on this view (blame, as-of) |
| `open_blockers`, `open_blockers_exo` | u16 × 2 | 4 | derived (§3.5) |
| `children_total`, `children_done` | u16 × 2 | 4 | derived |
| `title_off`, `fields_off`, `body_ref` | u32 × 3 | 12 | offsets into the title blob, the tagged-varint field block, the blob table |
| pad | | 3 | reserved |
| **total** | | **60** | |

Cold columns (separate arrays, touched only by the queries that need them): `uid: u128` (16 B, random at create, never changed; identity in canonical hashes and the image), `topo: u32` (Pearce–Kelly position in the precedence graph), `defer_until: u32`, `due: u32`.

Kind-specific fields live in the field block as `(field_sym varint, type u8, value)`: `bool` carries no value bytes, integers are zigzag varints, `f64` 8 bytes, strings are interned symbols or length-prefixed bytes, node refs are u32, sets are sorted u32 lists, `counter` is an i64 whose merge rule is `Incr`. The closed type set is {bool, int, counter, f64, enum-with-lattice, text, set, ref, commit-ref}. **`done`** is a typed *virtual* field on every kind that has a status: reading it returns `status ∈ {done, cancelled}` for tasks, `answered` for questions, `accepted` for verdicts; writing `done=true` performs the guarded transition (§3.6). It is never stored twice (Beads #6105 [06 §2.2]).

### 3.2 Node kinds (13) and typed fields

| Kind | Purpose | Kind-specific fields | Status set (lattice order for merge) |
|---|---|---|---|
| `task` | unit of work; subtasks are tasks with `parent`; campaigns/phases/rungs are tasks with labels | `work_kind` {design, impl, fix, test, measure, merge, doc, research, review, debt}, `phase_state` (the 14 states of [01 §5.1]: proposed … documented, incl. blocked/frozen/deferred side states), `assignee` sym, `acceptance` text, `files_owned` set<glob>, `estimate` u16, `reopen_count` counter, `reopen_if` text, `pre_registered` bool, `labels` set | `open < in_progress < done`; side states `deferred`, `cancelled`, `frozen` (conflict against `done`) |
| `doc` | plan, plan section, report body, patch; sections are docs with `parent` | `doc_kind` {plan, section, report, patch}, `heading` text, `order` fractional index, `revision` u16, `changed_in_round` u16, `targets` list<{metric, value, unit, op}>, `readiness` list<{item, state, reason}> | `draft < current`; side `superseded`, `archived` |
| `note` | descriptive knowledge, hazard, lesson, checkpoint, summary | `note_kind` {note, hazard, lesson, checkpoint, summary}, `symptom`/`mechanism`/`defence` text, `incidents` counter, `applies_to` {roles, phases, lanes, globs}, `observed_git_sha`, `review_after` u32 | `active`; side `superseded`, `retracted`, `archived` |
| `rule` | normative ("never X") | `text`, `enforcement` {must, should}, `applies_to` (as note; empty = `*`), `since` u32, `rationale`, `owner_quote` text (required when `authority = owner`) | `proposed < active`; side `superseded`, `retracted`, `archived` |
| `decision` | ADR-like; never edited after acceptance, only superseded | `context`, `what`, `why`, `tradeoff` text, `alternatives` list<{text, rejected_why, measurement_ref, git_tag, revive_condition}>, `owner_quote` | `proposed < accepted`; side `rejected`, `superseded` |
| `question` | owner / orchestrator question | `q_kind` {values, scope, unclear}, `asked_of` {owner, orchestrator, architect}, `options` list<text>, `answer` text (verbatim) | `open < answered`; side `dropped`; `answered` is derived from a visible `answers` edge (S2-B) |
| `finding` | review/critique remark, root cause, measurement observation | `local_id` sym (C1, W2, F3), `severity` {blocker, important, optional}, `f_kind` {correctness, perf, complexity, security, plan, style, debt, test}, `failure_scenario` text (mandatory), `what_needed` text, `where` {section ref \| file:symbol@sha}, `round` u16, `evidence` text | `open < (confirmed \| refuted) < (fixed \| deferred \| withdrawn)`; `confirmed` vs `refuted` are incomparable → `StatusFork` (S12) |
| `verdict` | gate result by a role | `role` sym, `round` u16, `raw_label` text, `outcome` {pass, pass_with_conditions, fail_fixable, fail_fundamental, unknown, na}, `return_to` {architect, developer, tester, none}, `criteria`/`conditions` text | immutable once written; `open < accepted`; side `superseded` |
| `measurement` | a number with its environment | `metric` sym, `value` f64, `unit` sym, `target` f64, `command` text, `measured_on` sha+algo, `env` {host sym, profile sym, load {quiet, loaded}, scale sym}, `baseline` ref | `current`; side `moved_declared`, `retracted`; `stale` is derived |
| `artifact` | pointer to a file outside the store | `path`, `sha256` [32]B, `bytes` u64, `artifact_kind` enum, `excerpt` text | `present`; `missing` derived by `doctor` |
| `run` | a Workflow run or agent call | `wf_id`, `bg_task_id`, `session_id`, `script_path`, `args_hash`, `journal_path`, `started`, `ended`, `expected_artifacts` set | `running < (green \| red \| stopped \| died)` |
| `lane` | a worktree + git branch + base, bound to a moirai branch | `worktree_path`, `git_branch`, `base_sha`, `tip_sha`, `target_dir`, `moirai_branch` ref-sym, `dirty_files` u16 | `active < ready_to_merge < merge_pending < merged`; side `frozen`, `abandoned`, `measuring` (implies quiet) |
| `area` | scope node (subsystem, directory, topic) | `path_globs` set | `active`; side `archived` |

Mutexes for non-task resources (benchmark slot, merge queue) are `task{work_kind = mutex}` with lease semantics ([C §3.2] `resource`, [07 §7.4] "merge slots"). A "critical note about the project" is a `note` or `rule` with `criticality = critical`; it sorts first in every brief and pack.

### 3.3 Edge kinds and policies

Edges are binary, typed, keyed by `(src, kind, dst)` with set semantics, stored in both directions in the same commit (I-P3). A sparse side table holds the only edge property, `pinned_commit` (16 B commit-id prefix) for `cites`/`implements`/`derived_from`.

| Edge (src → dst) | Class | Acyclicity / cardinality | On dst deleted | On src deleted |
|---|---|---|---|---|
| `parent` (child → parent) | structural | forest, depth ≤ 12 | **restrict** (`--cascade` deletes the subtree; `--reparent` moves children up) | drop; rollups updated |
| `blocks` (A → B): A done before B starts | structural | part of the precedence DAG (I5′), PK | **re-point** with `--replaced-by`, else **flag** (`has_dangling` on B; B stays out of `ready` until `resolve`) (X4) | drop; `B.open_blockers--` and B may enter `ready` |
| `gates` (verdict → task): completion blocked while the verdict is `open` with `outcome ∈ fail_*` | structural | checked for acyclicity with `blocks` | flag as above | drop |
| `merge_after` (lane → lane) | structural | DAG | drop + notify | drop |
| `runs_in` (run → lane) | structural | ≤ 1 | restrict | drop |
| `answers` (decision/note → question) | structural | ≤ 1 active | restrict | drop; question reopens |
| `scoped_to` (knowledge → area) | structural | many | restrict or `--reassign` to the parent area | drop |
| `duplicate_of` (dup → canonical) | structural | chain length 1 | restrict or re-point to canonical | drop |
| `depends_on` (section → section) | structural | DAG | drop + src `suspect` | drop |
| `supersedes` (new → old) | historical | acyclic; **≤ 1 active superseder per target** (X12) | tombstone ref | old stays superseded; `doctor` warns |
| `derived_from` (note/summary/verdict → source) | historical | acyclic by construction | tombstone ref + **src `suspect`** | — |
| `cites` (any → knowledge, `pinned_commit`) | historical | — | tombstone ref + **src `suspect`** | — |
| `implements` (task/artifact → decision/section, `pinned_commit`) | historical | — | tombstone ref; src `suspect` if the target is superseded | — |
| `refutes` / `confirms` (finding/measurement → finding/decision/rule) | historical | — | tombstone ref | — |
| `verifies` (measurement/verdict → finding/task/decision) | historical | — | tombstone ref | — |
| `addresses` (task/artifact → finding) | historical | — | tombstone ref | — |
| `about` (finding/verdict/measurement/question → target) | historical | ≤ 1 typical | tombstone ref | — |
| `discovered_from` (new → task) | historical | acyclic by construction | tombstone ref | — |
| `produced` / `consumed` (run → node/artifact) | historical | — | tombstone ref | — |
| `contradicts` (rule ↔ rule, one direction stored) | historical | — | tombstone ref | — |
| `mentions` (any → any; parsed from `#N` in title/abstract/body at write time, only for `N < next_id`, sigil rule: `#N` not preceded by an alphanumeric and not followed by `/` or `.digit`) | historical | — | tombstone ref, rendered `#40 (deleted c812 by dev#2: "dup of #52" → #52)` | recomputed from text |
| `relates` | historical | — | tombstone ref | — |

A verdict never carries `blocks` (X5): `gates` constrains `complete` (exit 6 with the gating verdict named), never `claim`, so a failed review does not deadlock the fix round when a lease expires or a task is reopened.

### 3.4 Invariants (checked on every write; re-checked after any merge, import, revert and by `doctor --verify`)

| ID | Invariant |
|---|---|
| I1 | `#N` is unique across all branches, allocated under the writer byte, never reused (including across imports and GC: a `#N` binds to at most one `uid` over the store's life, I35′); `uid` is unique. |
| I2 | Every structural edge has live endpoints in the same visible version; a flagged (`has_dangling`) blocker edge is the only exception and it excludes its dependent from `ready`. |
| I3 | Historical edges may reference dead ids; they resolve through the tombstone view. |
| I4 | `parent` is a forest with depth ≤ 12. |
| I5′ | The combined precedence graph `blocks ∪ gates ∪ child→parent ∪ {X→D : blocks(X,P), X ∉ subtree(P), D ∈ subtree(P)}` is acyclic; a node never blocks its own descendant; blockers are inherited from outside the subtree only; `link X --blocks P` on a container checks every descendant; `move` re-derives the exogenous classification for edges touching the moved subtree. |
| I6 | `supersedes(new, old)` implies `old.status = superseded` in the same commit; ≤ 1 active superseder per target. |
| I7 | The target of `duplicate_of` is canonical. |
| I8 | Status transitions follow the kind's machine; `blocked`, `ready`, `stale`, `claimed`, container-`done` are never stored as source truth. |
| I9 | Every derived counter, bitset, `topo` and the reverse CSR equal a full recomputation (property tests, `doctor --verify`). |
| I10 | Every mutation belongs to exactly one commit carrying provenance (actor, role, session, git head/branch/worktree, branch ref, idempotency key hash, message). |
| I11 | Fields conform to the schema version of their commit; symbols are never GC'd; enum integers are never reused. |
| I12 | Every branch head satisfies I1–I11 at all times; a merge, import, revert or cherry-pick with a structural violation never advances a ref (staged on `merge/*`). |
| I13 | A `finding` with `f_kind ∈ {perf, complexity}` reaches `fixed` only with an `addresses` edge from a different actor **and** a `verifies` edge from a review verdict, not a test run alone ("retest is not re-review" [01 §3]). |
| I14 | A `run` closes `green` only when every `expected_artifacts` symbol has a `produced` artifact whose `sha256` was read back ("0 errors but no plan file" [01 §7 L2]). |
| I14′ | An idempotency key is bound to its payload hash and branch; a hit with a different payload is exit 9. |
| I17′ | A lease mutation must present the current fencing token; expiry never bumps the token; the same holder may renew an expired, unreclaimed lease. |
| I18′ | As-of output carries no derived fields unless `--recompute`. |
| I25′ | For every key untouched on side S since the LCA, `merge` never emits a conflict on that key; the base of a key is its value at the LCA. |
| I26′ | A `#N` completed or deleted on any branch and not yet merged into branch R is excluded from `ready`/`claim` on R and never listed there as a live blocker. |
| I27′ | Every commit reachable in the log after recovery is reachable from a ref, the reflog or a pin, or is marked orphan and never satisfies an idempotency lookup. |
| I28′ | The git object ids of a moirai commit are a function of moirai data and the object format only. |
| I29′ | Importing an image of the same format version reproduces every native commit id; a demotion affects one commit only. |
| I30′ | A foreign two-parent git commit is imported as a moirai merge computed by the typed rules; counters are never taken from a text merge. |
| I31′ | `merge` has exactly one base-selection rule for multiple LCAs. |
| I32′ | `rm` refuses while any live lease covers the `#N` on any branch unless `--release`. |
| I33′ | On `plan/*`, `status`/`resolution`/`assignee`/claims are read-only; `blocks`, `parent`, `gates` are writable and validated. |
| I34′ | `revert`/`cherry-pick` stage on `NotFound` and record `DATA` mismatches as conflict values. |
| I36′ | `claimed`, `settled`, `deleted` markers and lease state are never versioned and never exported. |
| I37′ | At merge, `parent` moves are applied and the implied I5′ edges re-derived before any precedence edge is checked. |
| I-P3 | Reverse adjacency equals the inverse of forward adjacency in every visible version. |

### 3.5 Derived state (each predicate defined once in the engine, maintained eagerly for affected nodes only)

| Derived | Definition | Maintenance / cost |
|---|---|---|
| `open_blockers[n]` | count of `blocks`/`gates` in-edges whose source is not done/accepted, plus flagged dangling blocker edges | ±1 on source status change or edge add/remove/flag, O(out-degree) |
| `open_blockers_exo[n]` | as above, excluding sources inside n's subtree | decided at edge-add time by walking the source's ancestors, O(depth) |
| **`ready`** bitset | `kind = task ∧ status = open ∧ ¬deleted ∧ ¬conflicted ∧ ¬container ∧ open_blockers = 0 ∧ no ancestor with open_blockers_exo > 0 ∧ no live lease by another holder ∧ no `settled`/`deleted` marker from an unmerged branch ∧ defer_until ≤ now` | membership recomputed for touched nodes; the ancestor clause walks ≤ 12 parents per candidate at query time; the marker clause is one runtime-table probe per candidate |
| `is_blocker` bitset | not done ∧ has an outgoing `blocks` edge to a not-done task; "ids of all blocking tasks" = this bitset ∧ `kind:task` | O(out-degree) |
| rollups `children_total`, `children_done`, `ready_to_close` | direct children; container with all children done | O(depth) on child status change or reparent |
| `suspect` | a `derived_from`/`cites`/`implements`/`depends_on` target was retracted, superseded, deleted, or its current commit differs from the edge's `pinned_commit`; purely derived from `(target state, pinned_commit)`; "re-confirm" re-pins the edge | O(closure) along reverse derivation edges, op-budgeted at 10k |
| `stale` (measurement, note, artifact) | `measured_on`/`observed_git_sha` is not an ancestor of the bound worktree's git tip, or files under `applies_to` changed since; `artifact.sha256 ≠ file` | on demand only, never on the hot path; ancestry from the commit-graph/pack reader, cached as lazy facts in the log |
| `answered` (question) | a live `answers` edge is visible on the reading branch | O(1) |
| `conflicted` | an unresolved conflict value exists on this node on this branch | O(1) flag set by merge, cleared by `resolve` |
| `diverged` (read-time, `--across` only) | another listed branch holds a different value for the same key | computed while reading; never stored |
| `settled_elsewhere`, `deleted_elsewhere` (read-time) | a runtime marker for this `#N` names a branch whose marked commit is not an ancestor of the reader's tip | one probe + one gen-pruned ancestry check (µs) |
| critical path | longest open path in the precedence DAG under a subtree | on demand, DP over `topo` order |
| review-loop termination | `count(finding{about ⊂ target, status = confirmed, severity ≥ important, ¬fixed}) = 0` | bitset intersection (`stats loop`) |
| refuted share | per critic, per round: refuted / raised | bitset counts |

### 3.6 Status machines (guarded transitions)

- **task**: `open → in_progress → done`; `open|in_progress → cancelled|deferred|frozen`; `deferred → open`; `done → open` only through `reopen --reason` (an explicit op incrementing `reopen_count`; never a merge artefact). `phase_state` is advanced by verdicts (`return_to` moves it back explicitly). `complete` refuses (exit 6) while any child is open or an open `fail_*` verdict `gates` the task.
- **finding**: `open → confirmed|refuted|deferred|withdrawn`; `confirmed → fixed` (I13). A refutation is a finding with a `refutes` edge; the status move is a separate guarded write, never an automatic flip.
- **question**: `open → answered` (with an `answers` edge and an answer) or `dropped`; reopen explicit.
- **verdict**: `open → accepted|superseded`.
- **knowledge** (note, rule, decision, doc): `proposed|draft → active|accepted|current → superseded` only together with a `supersedes` edge; `retracted` with a reason; `archived`.
- **lane**: `active → ready_to_merge → merge_pending → merged`; side `frozen`, `abandoned`, `measuring`.
- **run**: `running → green|red|stopped|died` (I14).
- **measurement**: `current → moved_declared|retracted`.

---

## 4. Storage engine (final)

### 4.1 Files (all under `<store>/`, which is `.moirai/`; ≤ ~12 fixed files plus pinned checkpoint sets)

| File | Size / growth | Mapped? | Role |
|---|---|---|---|
| `HEAD` | 2 × 4 KiB checksummed slots | no (`pread` only) | cache of the current sequence, segment set, pointers to the ref/pin/head tables, counters (§4.2) |
| `LOCK` | 4 KiB | no | lock bytes 0 writer, 1 leader (M6), 2 maintenance, 3 quiet-advisory; holder diagnostics `{pid, start_ms, command}` at offset 2048, never inside a locked range |
| `log.NNNN` | 64 MiB extents, zero-filled at creation with one full `FlushFileBuffers`, then `DATA_SYNC_ONLY` per commit; rotated when full; ≤ 4 kept as active history before retirement | no (explicit I/O; never mapped) | canonical history: commits, ref moves, client heads, leases, markers, idempotency results, checkpoints, lazy records |
| `hist.NNNN` | sealed zstd frames of 256 commits + a per-frame commit index (`id16 → lsn`) | yes, read-only | retired log extents; the commit index lives here, never in the base segment |
| `seg.base.G` | ≈ 99 B/node hot + titles/fields + blob table | yes, read-only | materialized `main` state at generation G |
| `seg.dK` | small, tiered (≤ 3 live before a fold) | yes, read-only | delta segments: touched rows, list replacements, bitset ± lists, new tombstones/symbols |
| `seg.b<refsym>.K` | v1.1 | yes, read-only | promoted branch delta segment + `TOUCH` bitmap |
| `blobs.NNNN` | bodies only | yes, read-only | content-addressed (BLAKE3-128) zstd-dictionary frames; sealed, never extended while mapped |
| `dict.D` | 32–110 KiB | yes | zstd dictionary D (retrained at rollup when bodies grew > 25 %) |
| `gitmap.NNNN` | 41 B per commit per algorithm per destination | yes | sealed pages of `(commit_id16, dest u8, algo u8, git_oid[32])`; tail entries are `GitMap` log records |
| `config` | text | no | `default-branch`, `image.*`, `discovery.git-hint`, `lease.*`, `gc.*`, `quiet` |

Rules: no file is ever truncated or renamed over while it can be mapped; growth is by new files; reclamation writes a new file and switches `HEAD`; old files are deleted after `HEAD` has pointed elsewhere for 60 s and no pin references them, tolerating delete-pending (files are opened with `FILE_SHARE_READ|WRITE|DELETE`); segments under construction use a temp name and an orphan sweep runs in the next maintenance holder and in `doctor` (G14). The store is refused on network and OneDrive paths. Pinned checkpoint sets (branch bases, tags) keep their base and delta files alive across rollups; `doctor` reports pinned files and their holders.

### 4.2 `HEAD` slot (4 KiB, `zerocopy`, G18 layout)

```
HeadSlot {
  magic "MOIR", format u16, flags u16 (bit0 quiet, bit1 fts_tier2, bit2 readonly),
  slot_seq u64,            // increments per publish; readers take the valid slot with the higher value
  epoch u64,               // store epoch: random at init; re-rolled on restore/repair (G25)
  committed_lsn u64,       // visibility bound: readers never read past it
  checkpoint_lsn u64,      // first log record not folded into a segment
  commit_seq u64,          // last commit seq (change-feed cursor)
  next_id u32, _ u32,      // #N allocator (non-versioned)
  fence u64,               // lease fencing-token allocator (non-versioned)
  active_log u32, n_segments u8, _ [3]u8,
  segments [SegRef; 8],    // {file_no u32, kind u8, upto_lsn u64, blake3_16} = 29 B each
  refs_lsn u64,            // lsn of the newest RefTable record (folded into the REFS section at checkpoint)
  pins_lsn u64,            // newest Pin record  → PINS section
  heads_lsn u64,           // newest ClientHead record → HEADS section
  markers_lsn u64,         // newest settled/deleted record → MARKERS section
  image_cursor [4]{dest u8, algo u8, seq u64},
  seq_ring [32]{seq u64, lsn u64},   // recent commits for cheap `changes --since`
  reserved …,
  xxh3_128 [16]
}
```

The slot holds no variable-size table. The ref table (name → `{kind, commit_id, lsn, base_pin, fork_commit, fork_seq, ops_since_fork, promoted_seg, gen}` ≈ 57 B per ref), the pin table (segment file → refcount + holders), the client-head table and the marker table live in log records (`RefTable`, `Pin`, `ClientHead`, `Marker`) whose newest lsn the slot names; a checkpoint folds them into the `REFS`/`PINS`/`HEADS`/`MARKERS` segment sections, so a fresh process reads the section plus the records since. Cost: one extra `pread` per open (10–170 µs). The slot is written after the commit flush and **not** flushed itself; a stale slot is rebuilt by the next writer from the log (1PC+C).

### 4.3 Log records and the commit body

```
RecHdr (32 B): len u32 | kind u8 | flags u8 (bit0 lazy) | _ u16 | lsn u64 | epoch u64 | xxh3_64 u64
```

Kinds: `Commit`, `RefUpdate` (non-commit ref moves: `branch`, `tag`, `undo`, `op restore`, `branch -d`), `ClientHead`, `Lease`, `Marker` (settled/deleted), `Idem`, `GitMap`, `Pin`, `Checkpoint` (segment set change + per-ref lsn lists for the window, G15), `RefTable`, `Lazy`, `Noop`. A record whose epoch or checksum does not match ends the log for recovery purposes.

`Commit` body:

```
commit_id      [32]  BLAKE3-256 over the canonical form (§4.6)
n_parents u8, parents [{id16, lsn}; n]      // 24 B each; 2 for merges, 0 for root
gen u32                                      // 1 + max(parent.gen)
seq u64                                      // store-wide monotonic
ref u16                                      // branch this commit lands on
ref_old id16                                 // the branch tip this commit replaces (N3): adoption of a complete record
                                             // implies the ref move ref_old → commit_id, CAS-checked against the ref table
prev_on_ref u64                              // lsn of the previous commit on the same ref (G15)
kind u8                                      // ordinary | merge | sync | revert | cherry-pick | import-native | import-foreign | import-checkpoint
hlc u64                                      // hybrid logical clock (ms << 16 | counter)
actor u16, role u8, session u32              // symbols
git {algo u8, head [32], branch u16, worktree u16, base [32]}
idem_key [16], idem_payload [16]             // BLAKE3-128 of the key and of the payload (I14′)
sync_base id16                               // sync/merge only: the src commit absorbed (G17 merge-by-reference)
verified u8                                  // import-native: canonical hash matched the trailer (N5)
msg_len u16, msg
affected_len u16, affected [u32]             // ids whose derived state changed (change feed)
n_ops u16, ops[]
```

Ops are tagged varints; every op that changes a value carries its before-image (SQLite-changeset style [04 §3.13]) and a `prev` delta to the node's previous op lsn on this ref, so per-node history is a linked chain: `Create{id, uid, kind, fields}`, `Delete{id, reason, replaced_by, before-image}`, `Undelete{id, before-image}`, `SetField{id, field, old, new}`, `SetStatus{id, old, new, resolution}`, `Incr{id, field, delta}`, `SetBody{id, old_hash, new_hash}`, `AddEdge`/`RemoveEdge{src, kind, dst, props}`, `Move{id, old_parent, new_parent, old_order, new_order}`, `Schema{weaken|strengthen, payload}`, `Conflict{key, class, base, ours, theirs}`, `Violation{class, description, suggested}`, `Resolve{key, choice}`. A small commit is ≈ 230–330 B before compression (est.: 32 header + ~200 commit header + ~40 ops) — 100× below the ~52 KB measured per DoltLite single-row commit [04 §3.2].

### 4.4 Segment layout

```
SegHdr { magic "MSEG", format u16, seg_kind u8 (base|delta|branch|hist|blobs), n_rows u32, base_seq u64, upto_lsn u64,
         n_sections u16, sections: [{tag u16, off u64, len u64, xxh3 u64}], blake3 [32] }
```

Sections of `seg.base` (rows dense by `#N`; dead rows keep their tombstone header):

| Section | Layout | Bytes / row (est.) |
|---|---|---|
| `NODE` | `[NodeHdr; n]` 60 B fixed | 60 |
| `UID` | sorted `[(u128, u32)]` (cold; touched by import/export/`--uid`) | 20 |
| `TOPO`, `DEFER`, `DUE` | u32 each | 12 (cold) |
| `TITLE_OFF` + `TITLE_BLOB` | u32 + varint-len UTF-8 (≤ 200 B) | 4 + ~60 |
| `FIELDS_OFF` + `FIELDS_BLOB` | u32 + tagged-varint blocks | 4 + ~24 |
| `BODY_REF` → `BLOBTAB` | u32 → `(blake3_16, file u32, off u64, len u32, raw_len u32)` | 4 + per unique body |
| `OUT_OFF`, `OUT_DST`, `OUT_KIND` | CSR `[u32; n+1]`, `[u32; e]`, `[u8; e]` sorted by (kind, dst) | 4 + 5/edge |
| `IN_OFF`, `IN_SRC`, `IN_KIND` | reverse CSR | 4 + 5/edge |
| `EDGE_PROPS` | sparse sorted `(edge idx u32, pinned_commit id16)` | rare |
| `BM_*` | frozen bitsets: per kind, per (kind, status), `ready`, `is_blocker`, `deleted`, `suspect`, `conflicted`, `container`, `has_dangling` (~80 sets) | ≤ 128 KiB per set at 1e6 |
| `SYMTAB` | sorted string table | small |
| `TOMB` | sorted `{id, tx, reason_sym, replaced_by}` 16 B | per deleted node |
| `IDEM` | open-addressing `(key16, payload16, branch_sym, result blob ref)`; 30-day retention | 40 B per key |
| `LEASES`, `MARKERS` | sorted `{#N, holder, token, expires, run, pid, branch}` / `{#N, branch, commit id16, kind, hlc}` | 32 / 40 B per live row |
| `REFS`, `PINS`, `HEADS` | folded tables (§4.2) | small |
| `ANCESTRY` | `(sha32, sha32) → bool` lazy facts | rare |
| `TERMS`, `POST` | tier-2 FTS (v1.1, ≥ 20k nodes; built per delta segment, merged at rollup) | ~135 B/node |

A **frozen bitset** is hand-written: per 65,536-id chunk either a sorted u16 array (≤ 4,096 members) or an 8 KiB bitmap, with a 16-byte chunk index, queried in place from the mapping. Delta segments hold only touched rows (sorted `IDS` array), the complete forward/reverse lists of touched nodes (list-replacement semantics), bitset ± lists, new bodies/symbols/tombstones/leases/markers. A reader resolves a node by probing overlay → deltas newest-first → base. Hot bytes per node: 60 + 8 + 30 (3 edges × 2 directions × 5 B) + ~1 ≈ **99 B** [20 §1.1].

### 4.5 Write path (one command, one commit)

1. Acquire `LOCK` byte 0: `LockFileEx` on an overlapped handle, `WaitForSingleObject(2 s)`, `CancelIoEx` on timeout → exit 7 naming the holder from offset 2048 and whether its PID is alive (G1).
2. `pread` both `HEAD` slots; take the valid one with the higher `slot_seq`. **Recovery scan**: from `committed_lsn` to the first bad checksum or epoch mismatch; every complete `Commit` found is adopted (re-flushed once, its implied ref move applied with a CAS against the ref table — on CAS failure the commit is parked on `merge/<ref>` as an orphan for review, I27′), `HEAD` republished; **only then** the idempotency key is evaluated (X2/F-B7 order).
3. Replay `(replayed_lsn, committed_lsn]` into the process overlay.
4. Idempotency: hit with the same payload and a compatible branch → return the stored result (exit 0, `replayed: true`); same key, different payload → exit 9 with the original result; branch mismatch → exit 9 unless the original branch was merged into the caller's (N13e).
5. Resolve the client's branch (§5a.4) and validate: schema, branch kind write mask (`plan/*`), CAS guards (`--if-rev` against `rev_seq`, `--if-status`, `--if-holder`, `--if-tip` if given), lease token (I17′), role write policy, status machine, restrict policies, PK for every added precedence edge (I5′ including implied edges), forest depth, `gates`, `settled`/`deleted` markers for `claim`.
6. Apply ops to a scratch copy of the branch overlay: allocate `#N` from `next_id`, update forward and reverse adjacency, counters, bitsets, `suspect` closure (budgeted), `mentions` re-parse, compute `affected`. `complete` also produces a `Marker{settled}`; `rm` a `Marker{deleted}`; `claim`/`release` a `Lease` record.
7. Serialize the commit (`commit_id` over the canonical form; `ref`, `ref_old` = the branch tip **after** step 3, `prev_on_ref`), append it and any `Lease`/`Marker` records in one write; `NtFlushBuffersFileEx(FLUSH_FLAGS_FILE_DATA_SYNC_ONLY)` (skipped for `lazy`); flush failure aborts the process.
8. Publish the alternate `HEAD` slot (no flush) with `committed_lsn`, `commit_seq`, `next_id`, `fence`, the ref table lsn.
9. Release byte 0. Print `{commit_id, seq, rev_seq per touched node, affected, newly_ready}`.
10. **After** release: if the tail exceeds 4,096 ops / 4 MiB of records (bodies on a separate 32 MiB threshold) and quiet mode is off, take byte 2, write a delta segment to a temp name, flush it, re-take byte 0 for microseconds to append the `Checkpoint` record (with per-ref lsn lists) and publish; readers keep the old set. In quiet mode nothing runs until 8× the threshold, then one bounded delta runs anyway (G10).

Engine cost excluding the flush: tens of microseconds for a small commit (est.). `apply` runs steps 5–8 once for a whole batch (one flush).

### 4.6 Canonical form of a commit

Hashed into `commit_id`: parents' **stated** ids, `kind`, `hlc`, actor/role/session as strings, git provenance, message, schema version, `sync_base`, and the op list with nodes named by `uid`, edges by `(uid, kind name, uid)`, fields by name, ops sorted by `(uid, field name, kind name, dst uid)` — never `#N`, `lsn`, `seq`, `ref` symbol numbers, offsets or the store id. Two stores that import the same image compute the same ids (I28′); this is what makes the git trailer `Moirai-Commit` verifiable.

### 4.7 Read path and open path

Read: `pread` `HEAD` (10–170 µs) → map the listed segments if not already mapped (0.22 ms each [M], cached per process while the set is unchanged; a delete-pending miss means "re-read `HEAD` and retry", bounded) → replay `(replayed_lsn, committed_lsn]` into the overlay (µs per op; never beyond `committed_lsn`) → resolve the branch and, on first use in this process, build its overlay (§5a.3) → serve from mapped columns + overlay; strings are borrowed from the map, bodies decompress into a per-request bump arena; no node structs are materialised; readers take no locks.

Open: 2 × 4 KiB `HEAD` reads + ≤ 8 maps + one `pread` of the folded tables + tail replay ≤ 4,096 ops = **0.3–3 ms** warm (est.; the worst case is the checkpoint threshold itself, ~3–5 ms for a full 4 MiB tail [20 U13]). No step is O(history) or O(nodes).

### 4.8 Indexes

| Index | Structure | Where |
|---|---|---|
| id → row | direct (dense `#N`) in base; sorted `IDS` in deltas; hash map in overlay | segments / process |
| uid → id | sorted `UID` column | segments (import/export only) |
| adjacency fwd/rev | CSR + overlay ± vectors | segments + process |
| kind/status/ready/blocker/deleted/suspect/conflicted | frozen bitsets + overlay ± lists | segments + process |
| precedence order | `TOPO` | segments / overlay |
| commit id → lsn | per-frame fan-out index in `hist`; for the active extent, the overlay's `(id16 → lsn)` of replayed records | history / process |
| per-ref commit chain | `prev_on_ref` links + per-ref lsn lists in `Checkpoint` records | log / hist |
| per-node op chain | `NODE.last_op_lsn` → `prev` links | log / hist |
| idempotency, leases, markers, refs, pins, heads | folded sections + tail records | segments + process |
| text | tier 1 scan; tier 2 `TERMS`/`POST` per delta segment (v1.1) | segments |
| git ancestry cache | `ANCESTRY` lazy facts | segments |
| git id map | `gitmap.NNNN` + `GitMap` records | own files |

### 4.9 Checkpoint, rollup, GC

- **Delta checkpoint**: automatic (outside the writer byte, §4.5 step 10): fold the overlay into `seg.dK`; tiered: when d3 exists, fold d1..d3 into a new d1. 5–50 ms at any N (∝ tail).
- **Rollup** (new base, rebuilt bitsets, `TOPO` by one Kahn pass, dictionary retrain, tier-2 terms): **only** on explicit `moirai gc` or in the resident MCP server after serving a request when deltas exceed 25 % of base; never in a transient CLI/hook process; refused in quiet mode. 10–30 ms at 1e4, 0.1–0.3 s at 1e5, 1–3 s at 1e6 [05 §7].
- **History retirement**: a full `log.NNNN` is compressed into `hist.NNNN` (zstd frames of 256 commits + commit index); nothing is dropped by default.
- **Pins**: every merge into `main` and every `tag --pin` pins the newest sealed checkpoint set (refcount per file); a branch fork pins the newest set with `upto_seq ≤ fork_seq`.
- **GC** (`moirai gc`, explicit): reachability = refs ∪ reflog entries younger than `gc.reflog-expire` (90 d) ∪ pins; drop segment files with refcount 0 after 60 s delete-pending grace; rewrite `hist` frames dropping unreachable commits older than `gc.cruft-delay` (14 d), keeping commit headers unless `--prune-headers`; blob GC for bodies unreferenced by any live row, retained history or ref; `gitmap` compaction; orphan temp files swept.

### 4.10 Crash safety and Windows

- Every record has length, LSN, epoch and xxh3; recovery is "scan forward, stop at the first bad record". A commit is acknowledged only after its flush. `HEAD` is a cache; two checksummed slots make a torn write harmless. Segments are written to a temp name, flushed, then referenced by a flushed `Checkpoint` before `HEAD` lists them. Flush failure aborts the process; the next writer recovers (ATC'20 lesson [08 §8.1]). The epoch rejects records from a *different* init (a restored older copy of the same store is handled by the adopt-and-republish scan; `restore`/`repair` re-roll the epoch and zero-fill recycled extents, G25). `doctor --verify` recomputes every derived structure and every branch head (pin ⊕ ops vs promoted segments) and compares; `doctor --fsck` verifies column checksums and BLAKE3 footers.
- Windows: `LockFileEx` only on `LOCK` (Rust's `File::lock` locks the whole range [05 §6.3]); lock release after a crash may lag, so the waiter is bounded and names the holder; `NtFlushBuffersFileEx(DATA_SYNC_ONLY)` through `windows-sys`, never `FILE_FLAG_WRITE_THROUGH` as durability; mappings read-only via `CreateFileMappingW`/`MapViewOfFile`, never resized; ≤ ~12 stable files + pinned sets, no per-node or per-commit files, no create/delete churn on the commit path; bounded retry on errors 5/32 for deletes and export renames; commit path never renames; signed binary at a stable install path; bodies never on argv; UTF-8 output regardless of code page. Unix builds swap `fdatasync`, `fcntl` byte locks and `mmap`.

---
## 5. Version control (final)

### 5a. moirai's own VCS and branching

#### 5a.1 Object model

| Object | Identity | Content | Where |
|---|---|---|---|
| **commit** | BLAKE3-256 of the canonical form (§4.6); `id16` prefix in indexes; displayed `c<8 hex>` | header (parents, kind, gen, seq, ref, ref_old, prev_on_ref, hlc, actor, role, session, git provenance, idem, sync_base, message, schema version) + changeset (typed ops with before-images) | `log` / `hist` |
| **changeset op** | position in its commit | `Create`, `Delete`, `Undelete`, `SetField`, `SetStatus`, `Incr`, `SetBody`, `AddEdge`, `RemoveEdge`, `Move`, `Schema`, `Conflict`, `Violation`, `Resolve` | inside the commit |
| **blob** | BLAKE3-128 of raw bytes | zstd-dictionary frame of a body | `blobs.NNNN` / log tail |
| **ref** | name: `main`, `lane/<n>`, `plan/<n>`, `merge/<n>`, `import/<n>`, `tags/<n>` | `{kind: work \| plan \| merge \| import \| tag, commit_id, lsn, base_pin, fork_commit, fork_seq, ops_since_fork, promoted_seg, gen}` | `RefTable` records → `REFS` section |
| **reflog entry** | `(ref, seq)` | commits carry their own ref move (`ref`, `ref_old`); `RefUpdate {ref, old, new, reason, actor, hlc}` records non-commit moves | log |
| **client head** | key: `blake3_16(absolute directory)`, `--client <name>`, or `session:<id>` (expires with the idempotency window) | `ClientHead {key, ref \| detached commit, hlc}` | log → `HEADS` |
| **tag** | name under `tags/` | a ref of kind `tag` with an optional message; `--pin` pins the checkpoint set | as ref |
| **pin** | segment file | refcount + holders | `Pin` records → `PINS` |
| **runtime records** | `#N` | lease, settled, deleted (§5d) | log → `LEASES`/`MARKERS` |

A commit's parents may be on any refs; the DAG is store-wide, so a branch can fork from a branch, `main` can be merged into a lane repeatedly and the lane back into `main`. `gen = 1 + max(parent.gen)` prunes ancestor walks: `gen(A) ≥ gen(B)` ⇒ A is not an ancestor of B (git commit-graph generation numbers [04 §3.1]).

Branch kinds and write masks: `work` (default; `main`, `lane/*`) — everything writable; `plan` (`plan/*`) — `status`, `resolution`, `assignee`, leases/claims read-only, everything else including `blocks`/`parent`/`gates` writable and validated (I33′); `merge`/`import` — staging refs written only by the merge/import machinery; `tag` — immutable.

#### 5a.2 Refs, the reflog, and how a commit lands

- Every ordinary commit carries `ref` and `ref_old`; landing it is a compare-and-swap on the ref table entry (`old == ref_old`) performed under the writer byte in the same publish. **The parent of a plain write is the branch tip after tail replay under the writer byte** (G20), so two agents committing on one lane serialize on the writer byte exactly as on a shared trunk and never fail for "someone committed first"; only an explicit `--if-tip <commit>` guard can fail with exit 4 and the current tip.
- `RefUpdate` records exist only for non-commit moves (`branch` create/delete, `tag`, `undo`, `op restore`); they are written in the same flushed group as any commit they accompany.
- `moirai reflog <ref>` is a walk of the ref's chain (`prev_on_ref`) plus its `RefUpdate` records; the last 32 moves per ref are cached in `REFS` for O(1) `undo`.

#### 5a.3 How a branch lives on the engine

```
view(X) = SEG(pin_X)                          // sealed checkpoint set of main at seq P ≤ fork_seq(X)
        ⊕ ops(main, (P, fork_seq(X)])         // trunk ops from that checkpoint to the fork commit
        ⊕ fold over X's own commits in order:
              ordinary commit c  → ops(c)
              sync commit s_k    → ops(main, (M_{k-1}, M_k]) ⊕ resolutions(s_k)     // merge-by-reference (G17)
        ⊕ tail(X)                             // this process's overlay for X since its last replay
view(Y forked from X) = view(X)@fork_Y ⊕ ops(Y)   // recursive; promotion of X makes it cheap
view(main) = SEG(current) ⊕ tail(main)
```

- **Fork** (`moirai branch lane/l5np [--from main|<commit>]`): one durable `RefUpdate` (create) + one `Pin` increment on the newest sealed set with `upto_seq ≤ fork_seq`. Cost ~2 ms, zero copying. The pin keeps that set alive across later rollups of `main`.
- **Overlay build** (first read of X in a process): read the `Checkpoint` records since the fork (a few KB each; ~1–2 per day) for X's lsn lists, `pread` X's own commits and, for sync windows, `main`'s commits `(M_{k−1}, M_k]` through `main`'s per-ref index; decode into the overlay structure of §4.5. Cost O(X's commits + main's commits in synced windows): a 14-day lane with ~700 own commits ≈ 5–10 ms worst case (est., G15), independent of the other 49 branches' traffic; a fresh lane 1–3 ms. Private memory ≤ ~1 MB per branch read (≈ 150 B per op at 6k ops, est.); the builder streams through a fixed buffer, never reads extents whole.
- **Promotion** (v1.1): when `ops_since_fork > 8,192`, the overlay exceeds 8 MiB, **or** the branch's `promoted_seq`/`fork_seq` is more than 16 checkpoints behind the head (G16), the next maintenance holder writes `seg.b<X>.K` (overridden rows, lists, bitsets, `TOUCH` bitmap) and moves `base_pin` forward. 10–100 ms, never in the writer byte. Until promotion exists, the G15 index alone bounds cost for the owner's 1–3k-op lanes.
- **`sync`** (`merge main --into <lane>`): appends a `sync` commit on the lane with `sync_base = tip(main)` and **only the resolution ops** for keys both sides touched since the last sync (typically dozens); `main`'s ops are never copied (G17). A lane's history therefore grows by its own work plus resolutions, not by `main`'s churn (F-D2: ~0.1–0.2 GB/year at 50 lanes instead of 0.2–1 GB). `sync --refork` (optional, unexported lanes only) instead re-forks the pin at `main`'s tip when the preview is clean and disjoint ([22 §9 graft 8]).
- **`merge lane → main`** copies the lane's folded ops into the merge commit on `main` (that is the data landing on trunk; O(lane ops) once per lane); cross-lane merges likewise.
- **Cost with ~50 live branches** (est.): 50 refs × 57 B in `REFS`; pins retain the base files of the checkpoint sets the branches forked from (2–4 bases at ~12 MB each at 1e5, 115 MB at 1e6) plus their delta files (30–120 files, ~10–30 MB at 1e5) — 25–60 MB of disk at 1e5, 0.25–0.6 GB at 1e6, zero RAM until a branch is read; `doctor` lists pins held by abandoned branches; `branch -d` releases them.

#### 5a.4 HEAD per client and checkout semantics

Client key resolution, first hit wins: `--branch <ref>` → `MOIRAI_BRANCH` → the branch recorded on `--lease L` (a lease always carries its branch; a mismatch between `--branch` and the lease is exit 5 unless `--move-lease <ref>`, which warns) → the dispatch marker (`moirai:… branch=lane/l5np`, stamped into `ctx` at SubagentStart or typed into the MCP `branch` parameter) → `--client <name>`/`MOIRAI_CLIENT` → the registered binding for the current directory (`moirai worktree bind`, longest bound prefix) → the binding of the git worktree if a `.git` hint exists → `config.default-branch` (`main`). The MCP server resolves per call from the tool's `branch` parameter or, for stamped writes, `ctx.cwd`; unresolved reads use the session's checkout (`session:<id>` key). Every result prints `branch: lane/l5np · rev 4471` on its first line.

`moirai checkout <ref|commit>` writes a durable `ClientHead` for the resolved key. Detached reads are allowed; writes on a detached head are refused unless `--branch-new <name>`. Many processes on different branches: each builds only its own overlays; writes serialize on the writer byte with `ref` = the client's branch; nothing about one client's head affects another. `moirai branch --list` prints tip, kind, fork point, ops-since-fork, ahead/behind `main` (gen-pruned walk, µs), last actor, bound directories, live leases.

#### 5a.5 Op log, reflog, undo, revert, cherry-pick

The commit log **is** the operation log (every agent write is one commit, jj's two levels collapse [04 §3.8]); `RefUpdate` + `ClientHead` records make it a complete jj-style op log, so any past *view* (set of ref tips) can be restored.

| Verb | Effect | Cost |
|---|---|---|
| `undo [--ref R] [N] [--expect <tip>]` | moves R back to the value N moves ago (default: the client's branch, N = 1); CAS on `--expect`, which defaults to the tip the client last read (N13f); appends `RefUpdate{reason: undo}`; nothing is rewritten; undone commits stay until GC | 1 durable commit |
| `op log` / `op restore <seq>` (v1.1) | lists ref-set changes store-wide; restores all refs to their values at `seq` | scan; 1 commit |
| `revert <commit> [--onto R]` | appends the inverse changeset (before-images make inversion exact) as a new commit on R after running the validators; a `RemoveEdge`/`SetField` whose target or before-image no longer matches is a `NotFound`/`DATA` case: `NotFound` stages the revert on `merge/<R>`, `DATA` lands a `FieldEdit` conflict value (I34′); refused with the dependent set when a later commit on R *depends structurally* on the reverted one, defined as: it added a structural edge to, or a child of, a node the reverted commit created; or it completed a task the reverted commit reopened; or it resolved a conflict the reverted commit introduced. Reverting a merge/sync commit is refused in v1 (use `undo` at the ref level) | 2 ms + O(ops) |
| `cherry-pick <commit> [--onto R]` | 3-way apply of the commit's changeset with base = its first parent onto R's view; same typed rules, validators and `NotFound`/`DATA` handling; result is a new commit (`kind: cherry-pick`, origin in the trailer) | as revert |
| `reflog R` | the ref's move history with actor, reason, hlc | O(moves) |
| `rebase --onto` | **deferred** (history rewriting; the only verb that would change lane commit ids in the image) | — |

#### 5a.6 log, diff, show, blame, as-of

- `log [R] [--graph] [--node #N] [--actor] [--since seq] [--all-branches]`: walk from the ref tip through `prev_on_ref`/parents (gen-ordered priority queue); `--node` walks the node's per-view op chain (`last_op_lsn` → `prev`), switching into `main`'s chain segment for a sync window when the chain crosses a sync commit; per-node O(edits to the node); a cold `hist` frame decodes in ≤ 0.1–0.5 ms.
- `diff A..B` (A ancestor of B): fold of changesets along the path, grouped per key, rendered as typed hunks. `diff A...B` (symmetric, lane vs `main`): LCA by gen-pruned walk, then both sides' folds side by side with a `both` column for keys touched on both sides — the merge preview. Example rendering as [B §5.5].
- `show #N@<commit|seq|time> [--branch R]`: reverse-apply the node's chain from R's view until it passes the target; O(edits after the target). Membership of a chain op in the target's ancestry is gen-pruned.
- `blame #N [field]`: last op per field/edge with commit metadata.
- Whole-graph as-of (`moirai at <commit> -- <query>`): reverse-apply from the nearest later pinned set within 50k ops, else replay forward from the nearest earlier pin (every merge into `main` and every `--pin` tag pins a set, so the distance is bounded by pin spacing). Output carries no derived fields unless `--recompute` (I18′).

#### 5a.7 Merge algorithm

`moirai merge <src> [--into <dst>] [--policy P] [--strict] [--base <commit>] [--message M]`; `moirai sync` = `merge main --into <current lane>`.

0. **Sync-first precondition for merges into `main`**: if `tip(main) ∉ ancestors(tip(src))`, the merge first performs `sync` (merge `main` into `src`) as step 0 in the same writer transaction; if that stages, the whole merge stages and the message says which. After a successful sync, LCA(src, main) = `tip(main)` and the merge into `main` cannot conflict (every key: dst equals base → take theirs); it can still be refused for unresolved `conflicted` nodes on `src` (`merge-check` lists them). This makes the daily path a single-LCA path and criss-cross impossible there; a concurrent hooked `sync` cannot race because hooks only run `sync --check` (D5) and `sync` is refused while a staging ref `merge/<src>` exists.
1. **LCA** of `tip(src)` and `tip(dst)` by gen-pruned bidirectional walk. Multiple LCAs → **the newest by generation number, ties by lowest commit id** (I31′; git's `resolve` strategy, deterministic, may produce conflicts on keys changed between the LCAs — property-tested, recursive virtual base scheduled for v1.1 if cross-lane merges show it). `--base` overrides.
2. **Changesets**: fold each side's commits since the LCA through its per-ref index (O(own commits), G15); sync commits contribute only their resolutions (main's ops in synced windows are dst's own history when dst = main). Each fold is `key → final value`.
3. **Base per key**: the value at the LCA state, computed by reverse-applying the dst side's per-node chain for that key from `tip(dst)` back past the LCA (O(edits since LCA) per touched key; the LCA is a pin or within 4k ops of one on the daily path) (I25′, N1).
4. **Partition by key**; disjoint keys commute and apply directly; same-key changes go to the typed rule:

| Field type | Rule | On disagreement |
|---|---|---|
| status (per-kind lattice from the schema) | join if both are forward moves and comparable | `StatusFork` conflict value (side state vs forward; incomparable, e.g. `confirmed` vs `refuted`; `reopen` vs `done`) |
| enum / number scalar | equal → take; one side = base → take the other | `FieldEdit{base, ours, theirs}` |
| counter (`Incr`) | sum of both deltas over base | never conflicts |
| set | add-wins union with removals relative to base | never conflicts |
| text | line diff3 against the base blob; `doc.section` bodies also run the removed-text guard | `TextHunk` conflict value; `RemovedTextNotInBase` violation |
| `parent` / `order` | Kleppmann move in HLC order; a cycle-creating move is skipped and logged | `HierarchyCycle` violation |
| existence | delete vs modify → `DeleteVsModify`; `--policy delete-wins\|resurrect` per kind (default: tasks delete-wins, knowledge resurrect) | conflict value on the (resurrected) node |
| structural edge to a node deleted on the other side | `DanglingEdge` violation; suggested resolution = the edge's delete policy or `--replaced-by` | structural, staged |
| `supersedes` | second active superseder → `SupersedeFork` | conflict value |
| owner-authority fields (`authority = owner`, `owner_quote`) | `main` wins when `dst = main`; otherwise conflict | `OwnerFieldEdited` |
| coordination fields when `src` is `plan/*` | dropped, logged `PlanStatusIgnored` | hint |
| schema | weakening changes union; strengthening on both sides → `SchemaConflict` | structural |
| `pinned_commit` on citations | edge props: theirs if ours = base, else conflict | `FieldEdit` |

5. **Apply** to a candidate overlay on dst's view (TerminusDB "commit without advancing the label" [04 §3.3]).
6. **Validate** in this order (I37′): apply all `parent` moves → re-derive the implied exogenous edge set → check every added `blocks`/`gates` edge and every implied edge whose endpoints moved with incremental PK (full Kahn over the combined precedence graph when the merge touches > 1,000 precedence edges, 10–50 ms at 1e6 est.) → I2 dangling structural edges (reverse index ∩ deletions on either side) → I4 forest → I6/I7 cardinality → schema → `plan/*` read-only → `Duplicate`/`Contradiction` hints.
7. **Emit** one merge commit with two parents, the merged ops, one `Conflict` op per value conflict and one `Violation` op per structural problem, `affected` for the change feed; `rev_seq` of every touched node becomes the merge commit's seq (above both inputs).
8. **Advance or stage**: zero violations and (`--strict` ⇒ zero conflicts) → the merge commit lands on dst (CAS on the tip; nodes with conflict values get `conflicted` and leave `ready`). Otherwise it lands on `merge/<src>` (kind `merge`), dst is untouched, the CLI prints the violations with suggested resolutions and exits 6. `resolve <key> --take ours|theirs|base|--value V` and `resolve --all --policy P` append `Resolve` commits on `merge/<src>`; `merge --continue` re-runs steps 5–8 **against the current dst tip** with the staged resolutions as an overlay (the SQLite rebaser idea [04 §3.13]); `merge --abort` deletes the staging ref. Pins: a merge into `main` pins the resulting checkpoint set.

Merge cost (est.): a 2k-op lane vs 5k trunk ops since the LCA: two folds 1–3 ms + base as-of reads for ~200 touched keys 1–5 ms + typed merge + validators = 5–20 ms at 1e4, 10–40 ms at 1e5, 20–80 ms at 1e6 (+5–50 ms if full Kahn). CI gate: ≤ 50 ms at 1e5.

#### 5a.8 Conflict and violation taxonomy

| Class | Kind | Lands on the ref? | Default resolution |
|---|---|---|---|
| `FieldEdit`, `StatusFork`, `TextHunk`, `DeleteVsModify`, `SupersedeFork`, `OwnerFieldEdited` (dst ≠ main), `DATA` (revert/cherry-pick before-image mismatch) | value conflict | yes unless `--strict`; node `conflicted`, out of `ready` | agent `resolve`; per-kind auto-policies as schema data (`theirs` for findings, `ours` for rules are opt-in) |
| `DanglingEdge`, `Cycle`, `HierarchyCycle`, `IdCollision` (import), `SchemaConflict`, `RemovedTextNotInBase`, `ImageParse` (import), `NotFound` (revert/cherry-pick), `TombstoneRemoved` (import hint escalated when the node is referenced) | structural violation | **never**; staged on `merge/<src>` or `import/<ref>` | suggested fix printed; `resolve --policy` |
| `Duplicate`, `Contradiction`, `PlanStatusIgnored`, `ForeignMerge` | hint | yes, as a log line | none |

Conflict values render in `show` as `~conflict: base=… ours=… theirs=… (moirai resolve '#91.body' …)`; packs render a conflicted knowledge node as one line with the **base** text and the resolve hint, never with `<<<<<<<` markers (N15). `git`-style markers appear only in `show --full --markers`.

#### 5a.9 Tags, branch delete, GC

- `tag <name> [commit] [-m] [--pin]`: a ref of kind `tag`; `--pin` pins the checkpoint set for fast as-of.
- `branch -d <name>`: refused if not merged into `main` unless `-D`; releases the pin refcount; the reflog keeps the tip for `gc.reflog-expire`; **live leases on that branch are released with a triage note and `settled`/`deleted` markers for it become triage lines in `doctor`/`brief`** (N13d, I26′). `-D` warns how many commits, completions and deletions are dropped.
- `lane close|freeze <name>`: sets the lane node status and removes the directory binding; `apply` for a run bound to a closed lane resolves to the branch the lane merged into (N13e).
- GC: §4.9. Exported commits never disappear from the image; reflog expiry and cruft delay are the only places a moirai commit can vanish.

#### 5a.10 Per-branch cost summary (est.; inputs [05 §2], [20 §1.6])

| Operation | 1e4 | 1e5 | 1e6 |
|---|---|---|---|
| `branch` (fork), `checkout`, `undo`, `tag` | 2–3 ms (one durable commit) | same | same |
| first read of a fresh lane / 14-day lane / 60-day `plan/*` branch (G15) | 1–3 / 5–10 / 10–20 ms | same | same |
| subsequent reads (long-lived process) | as `main` + overlay probe ~0.2 µs; catch-up scans new records and keeps its own | | |
| `sync` (merge-by-reference, dozens of resolution ops) | 5–20 ms | 10–40 ms | 20–80 ms |
| `merge lane → main` (2k-op lane) | 5–20 ms | 10–40 ms | 20–80 ms (+ Kahn) |
| promotion (v1.1, 8k ops) | 10–40 ms | 20–60 ms | 30–100 ms |
| `log --node` | 10–50 µs per shown edit (+ ≤ 0.5 ms per cold `hist` frame) | | |
| as-of at a pinned set | 5–50 ms | | |
| 50 live branches: disk | ~5 MB | 25–60 MB | 0.25–0.6 GB |
| 50 live branches: RAM | 0 until read; ≤ 1 MB per branch read per process; MCP LRU ≤ 8 | | |

---
### 5b. The git-compatible image (R3)

Goals in order: (1) every moirai commit, branch and tag maps to git objects by a pure function of moirai data and the object format, so two stores holding the same commit produce byte-identical objects (I28′); (2) a human or an agent can read and diff the image in any git tool; (3) an edit or merge made on the git side is importable as a first-class moirai commit validated by the same engine; (4) the store stays canonical — the image is derived, never the working copy (Beads' dual-source lesson [03 §8.2]). Git is a storage and transport target; it is never consulted to answer a query.

#### 5b.1 Tree layout

```
.moirai-image                        format marker (§5b.3)
schema/kinds.moi  fields.moi  edges.moi     schema-as-data, sorted rows, one file per table
nodes/<h1>/<h2>/<uid>.moi            h1 = uid hex[0..2], h2 = uid hex[2..4]; uid = 32 lowercase hex
refs/heads.moi  refs/tags.moi        checkpoint-granularity images only: the moirai ref table rows
```

Two levels of 8-bit fan-out over `uid` (65,536 leaf trees): at 1e4 nodes most leaves hold 0–1 entries, at 1e5 ≈ 1.5, at 1e6 ≈ 15; one commit touching k nodes rewrites root + ≤ k mid trees + ≤ k leaves (root ≈ 7.4 KB SHA-1 / 10.5 KB SHA-256 raw at ≥ 1e5 nodes) — the path-copy tax bounded by the fan-out, not by N. The fan-out is part of the format version. **Out-edges only** are serialized, in the source node's file: adding `A blocks B` rewrites `A.moi` only; in-edges are rebuilt on import (a rule cited by 10k nodes would otherwise change on every citation). **Tombstones are files** and stay forever; a file that *disappears* is a foreign hard delete. Bodies are the tail of the node file (one blob per node keeps object count ≈ N; `git diff` shows prose). No `#N`-named files, no kind-named directories (kinds change; `#N` is store-local).

#### 5b.2 The `.moi` node file format (canonical text, version 1)

The exporter is the only writer of canonical bytes; the importer accepts a superset (CRLF, trailing whitespace, reordered lines) and normalises.

1. UTF-8, LF, no BOM, exactly one trailing LF, no trailing whitespace, bytes preserved (no Unicode normalisation).
2. Line 1: `moirai-node 1`. Then header lines `key: value` in this fixed order, absent/empty values omitted: `uid`, `kind`, `title`, `status`, `resolution`, `priority`, `criticality`, `confidence`, `authority`, `parent`, `order`, `created`, `updated`, `deleted`, `flags`. **There is no `id:` line** (N4): `#N` is store-local and would make identical data hash differently across stores. An optional, unhashed side ref `refs/moirai/aliases/<store-id>` (tree: `aliases.moi`, rows `uid #N`) carries the exporting store's numbers for humans; importers may read it as a hint (§5b.6 step 5).
3. Then, each group sorted bytewise: `field <name>: <value>` lines; `label <value>` lines; **`incr <field> <+k|-k> c<commit>` ledger lines** for counter-typed fields (one per `Incr` op, sorted by commit id; the current value is the sum and is never written as a `field` line) (N6); `edge <kind> -> <uid> [pin=c<commit>]` lines sorted by (kind name, dst uid, props); `conflict <key>: base=<v> ours=<v> theirs=<v> class=<class>` lines sorted by key (**the ordinary `field`/`edge` line for a key is omitted while a `conflict` line exists for it**, N12); `violation <class>: <description>` lines.
4. Values: integers decimal; floats shortest round-trip (Ryu) with a mandatory `.` or exponent; booleans `true`/`false`; timestamps RFC 3339 UTC `Z` with millisecond precision; node references as 32-hex `uid`; commit references as `c<64 hex>`; sets `[a, b, c]` sorted bytewise; strings bare when they contain no control characters, no leading/trailing spaces, do not start with `"`, `[` or `{` and are ≤ 4,096 bytes, otherwise JSON-string-escaped; multi-line text fields use block form `field why: <<` … two-space-indented raw lines … `>>`, chosen iff the value contains a newline.
5. `created`/`updated`/`deleted` are `c<commit-id>` followed by the commit's HLC timestamp for readability; importers use only the commit id.
6. Derived state (`open_blockers`, `ready`, `suspect`, `is_blocker`, rollups, `conflicted`, `rev_seq`, `lsn`, `claimed`, `settled`, `stale`) is never written (I36′).
7. The body, if any, follows a line consisting of `---` and runs to end of file, raw. A node without a body has no `---` line.
8. A tombstone file contains only `moirai-node 1`, `uid`, `kind` (at deletion), `title`, `deleted: c<commit> <time>`, `field reason: …`, `field replaced_by: <uid>`.

Example — task `#12` of the walk-through after `#40` was deleted with `--replaced-by #52`, at `nodes/01/8f/018f3c2e7a117b3c9d5e4c2f1a0b9e77.moi`:

```
moirai-node 1
uid: 018f3c2e7a117b3c9d5e4c2f1a0b9e77
kind: task
title: Wire lease reclaim
status: in_progress
priority: 1
criticality: normal
parent: 018f3c2e7a117b3c9d5e4c2f1a0b9e07
order: a0V
created: c9b2e6c1d4f0a7e3b5c8d1f2a9e4b7c6d3f0a1e2b5c8d7f4a3e6b9c2d5f8a1b4 2026-09-21T14:02:11.483Z
updated: c812e0f3a6b9c2d5e8f1a4b7c0d3e6f9a2b5c8d1e4f7a0b3c6d9e2f5a8b1c4d7 2026-09-25T14:02:40.011Z
field acceptance: reclaim sweeps expired leases; fencing token checked in complete/release
field assignee: dev#1
field estimate: 3
field files_owned: [src/lock.rs, src/reclaim.rs]
field phase_state: implementing
field work_kind: impl
label: l5np
label: physics
incr reopen_count +1 c4470a11e2f3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d
edge blocks -> 018f3c2e7a117b3c9d5e4c2f1a0b9e91
edge cites -> 018f3c2e7a117b3c9d5e4c2f1a0b9e12 pin=c4410f0e2d3c4b5a69788796a5b4c3d2e1f0a9b8c7d6e5f4a3b2c1d0e9f8a7b6
edge implements -> 018f3c2e7a117b3c9d5e4c2f1a0b9e40
edge mentions -> 018f3c2e7a117b3c9d5e4c2f1a0b9e52
---
Reclaim must run under the maintenance byte, never inside a request.
See #52 for the lock protocol this depends on.
```

`git diff` of the commit that deleted `#40` shows, in `…9e40.moi`, the header collapsing to a tombstone, and in `…9e77.moi` one line changing from `edge blocks -> …9e40` to `edge blocks -> …9e52`. Nothing else changes: in-edges are derived.

#### 5b.3 Format marker `.moirai-image`

```
moirai-image 1
object-format: sha1
store-id: 6f2a…          (the exporting store; an importing store records it as `origin`)
schema-version: 3
granularity: commit      (or checkpoint)
```

#### 5b.4 Commit mapping (moirai commit → git commit)

| git field | value |
|---|---|
| `tree` | the image tree at that commit; unchanged subtrees keep their ids |
| `parent` × n | git ids of the moirai parents through `gitmap`; merges have two parents; revert/cherry-pick results are ordinary commits with trailers naming the origin |
| `author` | name `moirai/<actor>`, email `<role>@moirai.invalid`, time = `hlc >> 16` ms → seconds, tz `+0000` |
| `committer` | identical to `author` (deterministic; the exporter's identity or wall clock would change the hash on re-export) |
| message | the moirai message, blank line, trailers in fixed order: `Moirai-Commit: <64 hex>`, `Moirai-Kind: ordinary\|merge\|sync\|revert\|cherry-pick\|import-native\|import-foreign\|checkpoint`, `Moirai-Ref: lane/l5np`, `Moirai-Actor`, `Moirai-Role`, `Moirai-Session`, `Moirai-Git-Head: sha1:7c1e0a…`, `Moirai-Git-Branch`, `Moirai-Worktree`, `Moirai-Origin: c…` (revert/cherry-pick source), `Moirai-Sync-Base: c…`, `Moirai-Schema: 3`, `Moirai-Ops: 3`, `Moirai-Idem: <hex>`; **no `Moirai-Seq`** (N4). Checkpoint commits carry `Moirai-Kind: checkpoint`, `Moirai-Commit: <head commit id>` and `Moirai-Folded: <n> commits from c… to c…` |

Refs: `main` → `refs/moirai/heads/main`, `lane/x` → `refs/moirai/heads/lane/x`, `tags/v` → `refs/moirai/tags/v` (lightweight; `--annotated` uses the author rule for the tagger). In a **separate image repo** the prefixes are `refs/heads/` and `refs/tags/`, so an ordinary `git clone` fetches everything. In a **project repo** the `refs/moirai/` namespace keeps the image out of `git branch` and requires the fetch refspec `+refs/moirai/*:refs/moirai/*` (documented; `doctor image` checks every clone config it can read and warns about `push --mirror` from clones that lack it — Dolt's `refs/dolt/data` caveat [08 §7.1]). Optional `refs/moirai/ops/<store-id>` (`--with-oplog`, deferred): `RefUpdate`/`ClientHead` events for reproducing reflog history elsewhere.

#### 5b.5 Determinism rules

1. Tree entries sorted by git's rule (bytewise, directories compared as `name/`); modes `100644`/`040000`; no executable bits, no symlinks.
2. Object format = the destination repo's `extensions.objectFormat`; never mixed; `gitmap` records the algorithm and destination.
3. Every blob byte is produced by §5b.2; the exporter re-parses each file it writes and asserts equality.
4. Commit objects follow §5b.4; no `gpgsig`, no `encoding`, no non-standard headers (jj's `jj:trees` header is exactly what tooling drops [D §12 S1]).
5. `gitmap` is derivable: walking the image and reading `Moirai-Commit` trailers rebuilds it (`image doctor --rebuild-map`), reporting commits whose recomputed canonical hash ≠ trailer.
6. **Every past `.moi` encoder is retained**; an image records the format version each destination uses, and re-exporting an old commit uses that version's encoder (O5). A format bump is a new destination, never a rewrite of an existing one.
7. Hashed content contains no store-local datum: no `#N`, no `seq`, no lsn, no store id (I28′). Property test: two stores that imported the same bundle export byte-identical objects.

#### 5b.6 Export and import

**Object I/O backends** (`ImageBackend` trait). v1: **`git fast-import`** as the writer (a text stream of `commit`/`M 100644 inline`/`D`/`reset`/`tag` commands with `mark`s = moirai commit ids, `--export-marks` giving the oids for `gitmap`; deterministic because object ids are a function of content; fast-import writes a pack, so per-object loose-file costs and `gc.auto` storms are avoided) and **`git cat-file --batch` / `git rev-list` / `git diff-tree`** as the reader, when `git` is on PATH (Git for Windows 2.54 is present [07 §1]). v1.1: a hand-written loose/pack/idx/bundle writer and reader for "no git installed" export/import, with the G21–G24 hygiene. The semantic layers (canonical `.moi` codec, tree diffing at the path level, trailers, verification, validation, `gitmap`, staging) are hand-written in v1 and shared by both backends.

**Export** (`moirai image export [--to <gitdir|bundle>] [--refs main,tags/*] [--granularity checkpoint|commit] [--since-cursor] [--object-format sha1|sha256]`):

1. Open the destination (recognised by `HEAD` + `objects/` + `refs/`; `--create` writes a bare skeleton with `objectFormat` and `refStorage = files` explicit, plus `gc.auto = 0`, `gc.autoPackLimit = 0`, `pack.threads = 2`, `pack.windowMemory = 64m`, and `.gitattributes` `*.moi text eol=lf`).
2. Frontier: for each selected ref, walk moirai commits from the tip down to one already in `gitmap` for this (destination, algorithm) — `image_cursor` gives the starting seq; topologically order the new commits.
3. Per commit (`commit` granularity) or per export run (`checkpoint`): build the tree by applying the changeset to the exported tree of the first parent, reading only root + touched mid + touched leaf trees from the destination (lazy, G24); encode touched `.moi` files from the branch view at that commit (as-of, O(edits) per touched node); stream to the backend; record `(commit_id, dest, algo, oid)`.
4. Update refs through the backend (fast-import's own ref update; or `git update-ref --stdin`; in a hand-written backend git's `<ref>.lock` protocol with read-after-write verification, G21) with a CAS against the last-seen oid for that ref on that destination; mismatch → exit 6 with `moirai image import` as the remedy.
5. Durability: objects and refs are written by the backend before `gitmap` records are appended under the writer byte in one durable commit; a crash in between leaves the destination one step ahead of `gitmap`, which the next export detects and re-maps (idempotent: object ids are content-addressed).
6. Exports do not hold the maintenance byte; they read sealed segments and the log and rely on pins plus the 60 s delete-pending grace (G26). Refused in quiet mode unless `--force`.

**Import** (`moirai image import [--from <gitdir|bundle>] [--refs …]`):

1. Read refs and objects through the backend; walk commits from each ref tip until an oid in `gitmap`; order topologically; record each destination ref's oid as "last seen" (N13b).
2. **Native** commit (`Moirai-Commit` trailer present, unknown to the store): diff the tree against the tree of its first parent → parse touched `.moi` files → typed ops (absent→live `Create` + fields + edges; changed lines `SetField`/`SetStatus`/`AddEdge`/`RemoveEdge`/`Move`/`SetBody`; `incr` lines added since the parent → `Incr`; live→tombstone `Delete`; **tombstone→live `Undelete`**; live→absent foreign `Delete{image:file-removed}`; tombstone→absent `TombstoneRemoved` hint) → reconstruct the canonical form using the **parents' stated ids** from their trailers → verify BLAKE3 = `Moirai-Commit`. Match → append with the same id, `kind: import-native`, `verified = 1`. Mismatch → this commit alone is demoted to foreign with a new id and `verified = 0`; its children still verify against their stated parent ids and record `parent_actual` (N5, I29′). Checkpoint commits (`Moirai-Kind: checkpoint`) are imported as `import-checkpoint` with a new id and `Moirai-Folded` origin, never reported as tampered (N13a).
3. **Foreign** commit (no trailer, unknown trailer, hash mismatch): same tree diff → ops; new id, `kind: import-foreign`, `actor: git:<author email>`, `hlc = max(committer_time, max(parent.hlc) + 1)` (N13c), `foreign_git: <oid>`, message = the git message. **A two-parent foreign commit is imported as a moirai merge computed by the typed 3-way over the two parents' imported states** (base at their LCA); the foreign tree is consulted only to resolve `TextHunk` conflicts the human resolved on the git side (a line present in the foreign file and in exactly one side is taken as that side's resolution); counters come from the ledger union, never from a text merge; any remaining disagreement is a conflict value (I30′, N6).
4. **Validate** every imported commit with the merge validators against its parent view: `ImageParse` (a `.moi` that does not parse, e.g. leftover `<<<<<<<` markers), `SchemaConflict`, `DanglingEdge`, `Cycle`, `HierarchyCycle`, `IdCollision` (a `uid` live with a different `created` commit). Clean → append and move the local ref. Violations → append on `import/<ref>` (staging) and stop the local ref there; `resolve` + `merge --continue` finish it like a local merge.
5. **Ids**: `uid` is identity; an alias hint from `refs/moirai/aliases/*` is honoured only if `N ≥ next_id` (then `next_id = N + 1`), otherwise the alias map `(origin store-id, foreign #N) → local #N` records the remap and `show` prints `#40 (was #17 in store 6f2a…)` (N11, I35′).
6. `gitmap` gets `(commit_id, dest, algo, oid)` for native and foreign commits alike, so the next export of a foreign commit is a no-op and the round trip closes.

Both directions take the writer byte only to append commits, so agents keep working during a 1e6-node export.

#### 5b.7 Round-trip guarantees

| Data | Lossless? | Mechanism |
|---|---|---|
| nodes, fields, sets, labels, counters (as ledgers), bodies, edges with props, tombstones (reason, replaced-by), conflict values, violations, schema | **yes** | `.moi` canonical text; tombstones are files |
| commit DAG (parents, merges, sync bases, revert/cherry-pick origins) and commit metadata (actor, role, session, hlc, message, project-git provenance, idempotency key hash, ops count) | **yes** at `commit` granularity | trailers + parents; `Moirai-Commit` verifies the canonical hash |
| moirai commit ids | **yes** (`verified`) | recomputed from the canonical form |
| branch/tag names and kinds | **yes** | ref names; kind in `refs/heads.moi` (checkpoint) or the `Moirai-Ref` trailer |
| `#N` numbers | **best effort** | unhashed alias ref; alias map on collision |
| reflog / op log / client HEADs | only with `--with-oplog` (deferred) | `refs/moirai/ops/<store-id>` |
| leases, `settled`/`deleted` markers, idempotency results, cursors, `next_id`, fencing tokens, pins, segments, lsn/seq | **no, by design** | runtime state (I36′); re-established on the importing store |
| derived state | recomputed | never serialized |
| intermediate commits between checkpoints | **no** at `checkpoint` granularity | folded; the head state is exact, the path to it is not |
| git object ids across object formats | different by construction | `gitmap` keeps both |
| store file bytes | no | the image is logical |

Property tests (S4 exit): `export(store) → fresh import → export` byte-identical for 1e5 nodes and 1e5 commits (SHA-1; SHA-256 when supported); `import(export(S1)) ⊕ import(export(S2))` on a third store equals `merge` of the two; two stores importing the same bundle export identical objects; the encoder/decoder fuzzed over random op sequences including `revert`, `cherry-pick`, `undo`, block strings, `---` inside bodies and CRLF.

#### 5b.8 Destinations, granularity, object format, failure modes

| Destination | Refs | Cloned by default? | Working tree touched? | PR-reviewable? | Use |
|---|---|---|---|---|---|
| **separate bare repo** `<parent of the main worktree>/<project>-moirai.git` (default) | `refs/heads/*`, `refs/tags/*` | yes | no | yes (branch view) | isolates history size from the project repo; pushed to a private remote by the owner |
| project repo, `refs/moirai/*` | `refs/moirai/heads/*` | no (refspec) | no | via `git log refs/moirai/heads/main` | backup + cross-machine sync riding the project remote; checkpoint granularity only |
| project repo, orphan branch `moirai/image` (deferred) | `refs/heads/moirai/image` | yes | no | yes | when the image must be visible in every clone |
| tracked directory `docs/moirai/` (deferred) | none | yes | yes | **yes** | checkpoint granularity only; git-side merges expected and imported as foreign merges |

Defaults: refs `main` + `tags/*`; granularity `checkpoint` (one per merge into `main`, one per day, and on explicit export); `commit` granularity and lane refs opt-in per destination. SHA-1 for anything that leaves the machine (the hash-function transition document still discourages SHA-256 on public servers [D §12 S4]); SHA-256 allowed for local-only repos; `doctor image` warns when a SHA-256 image is pushed to a remote that has not advertised support.

| Failure | Answer |
|---|---|
| hand edit of a `.moi` on the git side | foreign commit, validated; a parse error stages `ImageParse` with the path |
| `git merge` of two exported branches on the git side | imported as a typed moirai merge over the parents' states (I30′); leftover markers stage `ImageParse` |
| file deleted on the git side | foreign `Delete{image:file-removed}` with a tombstone; a real moirai delete always writes a tombstone file, so an absent file is always a foreign act |
| history rewritten on the git side (rebase/squash/filter-repo) | trailers survive (message trailers, unlike jj headers); commits whose tree equals the recomputed tree are re-mapped in `gitmap`; others are foreign; the exporter's CAS refuses to move a ref it does not recognise until an import has run |
| `push --mirror` from a clone without `refs/moirai/*` | `doctor image` warns; the store is canonical, a fresh export restores the refs; the separate-repo default avoids it |
| image repo `gc` prunes unreferenced objects after a partial export | the exporter re-emits them (idempotent) |
| two stores export to one destination | the second's ref CAS fails; it imports first (native ids verified), then exports (Dolt's fetch-then-`--force-with-lease` loop) |
| object-format mismatch | each format is a distinct destination with its own `gitmap` column; never mixed |
| CRLF / autocrlf on a tracked directory | `.gitattributes` `*.moi text eol=lf`; the importer normalises; the exporter always writes LF |
| reftable repository | v1 delegates ref updates to `git update-ref`/fast-import (reftable-aware); a hand-written writer detects `extensions.refStorage = reftable` and refuses without git (G22) |
| AV filter driver rolls back a loose-ref write (git-for-windows #6396 class) | read-after-write verification with one retry; `image doctor` compares `gitmap` against refs (G21) |
| `git gc` during a quiet window | never automatic (config written by moirai); `moirai image gc` refuses in quiet mode (G23) |

#### 5b.9 Cost (est.; SHA-256 1.94 GB/s [M], fast-import zlib on one core; inputs [20 §1.7])

| Quantity | 1e4 | 1e5 | 1e6 |
|---|---|---|---|
| full export objects | ~1e4 blobs + ~9k trees | ~1e5 + ~66k | ~1e6 + ~66k |
| full export raw / packed | 5–6 / ~2 MB | ~58 / 20–25 MB | ~565 / 200–250 MB |
| full export time (fast-import, pack) | 0.1–0.3 s | 1–2.5 s | 10–25 s |
| full export private RSS (explicit command) | ~2 MB | 12–15 MB | 110–150 MB |
| incremental, one commit, 3 nodes | ~32 KB raw / ~17 KB packed; one fast-import run ≈ 40–80 ms incl. the git spawn (74 ms measured for `git --version`) | same | same |
| checkpoint per merge + daily, 1e5 nodes | ~4 MB/day changed files → ~1.5 GB/year undeltified, ~0.2 GB after repack | | |
| `commit` granularity, 365k commits/year | ~6 GB undeltified (SHA-1), ~0.3–0.6 GB after repack | | |
| full import | 0.3–1 s | 2–7 s | 25–70 s (+ a final rollup) |
| incremental import, one commit | 5–20 ms + the git spawn | same | same |
| `gitmap` | 41 B per commit per (destination, algorithm) | | |

### 5c. Git independence (R2)

Discovery, location, binding and provenance are fixed in §2.14; this section states what runs without git and what never does.

**Runs with no `git` binary, no git library and outside any repository**: `init`, every read and write verb, branches, merge, sync, tags, revert, cherry-pick, undo, reflog, GC, doctor, hooks, the MCP server, packs and briefs, `image export --to <bundle|dir>` and `image import` once the hand-written backend exists (v1.1); in v1 the image verbs need `git` on PATH and say so (exit 7 with the reason) — the core never does. Provenance fields are empty when no `.git` is discoverable; `stale` degrades to "unknown" and says so.

**Reads git *files* textually, never spawns**: the discovery hint (`.git` file/dir → `commondir`), provenance (`HEAD`, `refs/heads/*`, `packed-refs`, ~60 lines), ancestry for `stale` (`objects/info/commit-graph` generation numbers when present [04 §3.1], else the image module's pack reader), cached as lazy facts in the log so every process shares the answers (C's cache).

**Spawns `git` only when present and only for transport of the image** (`image push/pull`), printing the exact command otherwise; and, in v1, for the image object backend (§5b.6). Both live in the `image` module, outside the core crate.

**Branch independence**: moirai refs have their own namespace and lifecycle; nothing is created, switched or deleted because git did something. Conveniences, all explicit: `lane open <name> --worktree <dir> [--git-branch <b>] [--base <sha>]` (= `branch lane/<name>` + `worktree bind` + a `lane` node recording the git facts), `worktree bind|unbind|--list`, `hook git-post-merge` (runs `merge-check` for the bound lane and offers `moirai merge`), `hook git-post-checkout` (refreshes the binding's provenance), `doctor lanes` (bindings whose directories are gone; worktrees with no lane). One store serves the owner's 44 worktrees through the hint with no pointer files; directories outside any repository use pointer files.

### 5d. Coordination under branching and the "node 40" story

#### 5d.1 Versioned vs runtime state

| State | Versioned per branch | Store-level runtime | Rule |
|---|---|---|---|
| nodes; all typed fields incl. `status`/`done`/`resolution`/`assignee`/`phase_state`; all edges incl. `blocks`/`gates`/`parent`; bodies; schema; conflict values; `rev_seq` | ✔ | | merge rules §5a.7; `reopen` vs `done` at merge = `StatusFork`, never silent |
| refs, tags, reflog, client heads | (version pointers) | ✔ | exported with the image (`--with-oplog` for heads/reflog) |
| **lease** `{#N, holder, token, expires, run, pid, branch}` | | ✔ | "an agent is working on this node now": keyed by `#N`, visible from every branch; `claim` requires live + ready **on the claimer's branch** and records it; `complete`/`set --lease` write on the lease's branch (`--move-lease <ref>` explicit, warns); `release`/`reclaim`/expiry store-level; a lease on a branch that is deleted is released with a triage note; `rm` refuses under any live lease unless `--release` |
| **`settled`** `{#N, branch, commit, holder, outcome, hlc}` | | ✔ | written by `complete` in the completing commit; on every other branch R, while `commit ∉ ancestors(tip(R))`, `ready`/`claim`/`blocking`/`brief` treat `#N` as `done on <branch> (c…, unmerged)`: not claimable, not listed as a live blocker, rendered with the branch; cleared by ancestry (µs, gen-pruned) when R receives the completion; becomes a triage line if the branch is deleted (I26′) |
| **`deleted`** `{#N, branch, commit}` | | ✔ | written by `rm`; same treatment: no branch dispatches or lists as a live blocker a node another live branch deleted before it has received the delete |
| fencing tokens `HEAD.fence` | | ✔ | monotonic store-wide; expiry never bumps the token; same-holder renewal of an expired unreclaimed lease (I17′) |
| idempotency results | | ✔ | key + payload hash + **branch**; replay on another branch is exit 9 with the original result, except when the original branch was merged into the caller's or deleted after merge, which returns the original result as success (N13e) |
| change-feed `seq`, per-session cursors | | ✔ | `seq` store-wide; every entry carries `ref`; `changes --since S [--branch R\|--all]` defaults to the caller's branch ∪ `main`; ref moves (merges, undo, branch delete) are entries |
| `next_id`, `uid` generation | | ✔ | one allocator; `#N` unique across branches |
| client HEAD bindings, quiet flag, pins, `gitmap`, alias map, ancestry cache | | ✔ | not versioned; `session:<id>` heads expire with the idempotency window |

Why leases and markers are runtime, not history (R1's "justify" clause): a lease merged from a lane would be a claim by a possibly dead process; a lease on `plan/*` is meaningless; and the owner's stated fear is two lanes building the same task [07 §7.4] — a store-wide lease prevents it *while the lease lives*, and the `settled` marker prevents it *after* `complete` releases the lease and before the merge, which is where D's "double work is impossible" claim failed (N2/D1). Both are "who is doing / has done what now", exactly like leases, and are never exported (I36′). Cost: ~30k markers/year at 40 B.

#### 5d.2 Task status and `done` across branches and at merge

- `status` is versioned; `done` is `status == done`. A task completed on `lane/x` is `done` on `lane/x`, `settled` everywhere. `main` learns it at `merge lane/x --into main` (lattice `done > in_progress > open`; `cancelled`/`deferred`/`frozen` vs `done` → `StatusFork`). A lane learns `main`'s completions at `sync`.
- `blocks` across branches: `ready(B)` on branch R uses R's view of A's status **and** the markers. If A is done on `lane/x` and B lives on `lane/y`, B is ready on `lane/y` after `lane/x → main → lane/y` (or a direct cross-merge); meanwhile `blockers #B --across` prints `A: done on lane/x (c4470, not merged into your branch)`, and `ready`/`blocking` on `main` never list A as a live blocker or re-dispatch it (I26′). Hooks add a one-line `behind main by N commits (k completions, m rules, j deletions touching your nodes)` notice for the bound branch.
- `reopen` is an explicit op; merging a reopen against a `done` on the other side is a `StatusFork`.
- Verdict gating (`gates`) is versioned like `blocks`: a critic's `fail_fixable` verdict written on `lane/x` gates completion on `lane/x`; dispatch happens on the lane branch (`ready --branch lane/x`), so the verdict is visible where the work is; the orchestrator sees it from `main` through `--across`.
- Rules and owner rulings written on `main` reach lanes at `sync`; until then every pack shows unmerged critical rules with a `~main` marker (owner rulings are never hidden by branching, §7.4).

#### 5d.3 "Node 40 deleted": within a branch and across branches

Within one branch, one commit (§4.5): the writer walks #40's reverse list (O(degree)); restrict kinds refuse unless `--cascade`/`--reparent`; `blocks`/`gates` out of #40 are re-pointed with `--replaced-by` or flagged (X4); historical in-edges stay and now point at a dead id; `cites`/`derived_from` sources become `suspect`; the row keeps `deleted`; the tombstone `{40, tx, reason, replaced_by}`, the `deleted` marker and the change-feed entry with `affected = [12, 77, 203, …]` are in the same flushed group. L1: every other process reads `HEAD` before its next operation and replays the record (µs) — any read of #12 afterwards shows the flagged blocker, any reference to #40 renders `#40 (deleted c812 by dev#2: "dup of #52" → #52)`. L2: `changes --since` and the hook deltas (`UserPromptSubmit`, optional `PostToolBatch`) carry the line, relevance-filtered to nodes the agent claimed, is blocked on or cited, capped at 600 characters. L3: any write that assumed #40 (`set #40`, `link --blocks #40`, `--if-rev` on #12 with an old rev) fails with exit 3/4 and the current value; a `SubagentStart` pack never includes #40.

| Case | On the deleting branch (`lane/x`) | On other branches before any merge | At merge |
|---|---|---|---|
| `rm #40` on `lane/x`; `main` and `lane/y` reference it | as above | **unaffected** (git-like isolation): #40 is live there, but the `deleted` marker keeps it out of `ready`/`blocking`/`claim` on every branch; `show #40 --across` prints `deleted on lane/x c812 by dev#2 (not merged)`; packs add an advisory line when the caller's branch is behind | `merge lane/x --into main`: `main` only read #40 → delete wins, `main`'s referrers get the same atomic treatment; `main` modified #40 → `DeleteVsModify` conflict value; `main` added a structural edge to #40 → `DanglingEdge` violation staged on `merge/lane/x` with the suggested `resolve … --take repoint:#52`; historical edges become tombstone refs, their sources `suspect` |
| `rm #40` on `main`; lanes reference it | as above on `main`; every process reading `main` sees the tombstone at its next read | lanes unaffected until `sync`; the hook notice says `behind main: 1 deletion touching your #12`; `blockers #12` on the lane prints `#40 (deleted on main c812; sync to apply)`; the marker keeps #40 out of lane dispatch | `sync` applies the same rules with the lane as dst; a flagged blocker keeps #12 out of `ready` until `resolve` |
| #40 leased by `dev#2` on `lane/y`, deleted on `lane/x` | **refused**: `rm` prints `leased by dev#2 on lane/y (L-19)`; `--release` releases the lease with a triage note attached to #40's tombstone and proceeds (I32′) | — | `DeleteVsModify` if `lane/y` modified #40 |
| #40 deleted on `lane/x`, then `lane/x` deleted unmerged | the deletion never reaches `main` | the `deleted` marker becomes a triage line in `doctor`/`brief`; `branch -D` warns | — |
| #40's file removed by hand in the git image | — | — | import creates a foreign `Delete{image:file-removed}` on the imported ref; the merge rules above apply when that ref is merged |
| `revert` of the deleting commit | `Undelete #40` with the before-image; re-pointed edges restored if their targets still exist, else `NotFound` staged | — | the image sees a tombstone→live transition (`Undelete`) |

Engine-level guarantee on every branch head at every commit: no structural edge points at a dead node except a flagged blocker that keeps its dependent out of `ready`; every referrer of a tombstone renders it; every derived counter equals a recompute (I2, I12, I9). What "maximally synchronous" means under R1: a delete on one branch never *mutates* another branch, but every branch that receives it applies it atomically, every other branch can see it on request, and no branch can dispatch or list as a live blocker a node another live branch has deleted or completed.

---
## 6. Concurrency, leases, change feed, durability classes

### 6.1 Processes and locks

| Process | Lifetime | Store access | Locks |
|---|---|---|---|
| `moirai` CLI (agent Bash calls, hooks, orchestrator scripts) | ms | direct (§4.5, §4.7) | writer byte during a commit; maintenance byte during a delta checkpoint it triggered |
| `moirai mcp` (one per Claude session; serves all its subagents across worktrees [08 §2]) | session | direct; keeps maps, the `main` overlay and an LRU of ≤ 8 branch overlays warm (G19); may run a rollup after serving a request when deltas exceed 25 % of base; 0 % CPU when idle (no threads, no timers) | same as CLI; leader byte only in M6 |
| leader (M6, optional) | session | pipe with per-user DACL, `FILE_FLAG_FIRST_PIPE_INSTANCE`, `PIPE_REJECT_REMOTE_CLIENTS`; pipelined group commit; broadcast; every forwarded write carries a client-generated 128-bit key (G13) | leader byte + writer byte per commit |

Writer protocol: exclusive `LockFileEx` on `LOCK` byte 0 with the G1 blocking wait (2 s bound; timeout prints the holder's PID, command and liveness). Readers take no locks; correctness comes from immutable segments, `committed_lsn` and the monotonic `seq`. Maintenance (delta checkpoint, promotion, rollup, GC) holds byte 2 and re-takes byte 0 only to publish. Under a 16-agent burst the ideal serial last-ack is ~32–40 ms (16 × 2–2.5 ms), which the blocking wait achieves without convoy (F-A2). Per-process caches: mapped segments (shared pages), the overlay since the segment set's `upto_lsn`, symbol table, the branch overlays the process read; invalidation is by `committed_lsn` ("replay the tail"), never "drop everything", because sealed segments never change.

### 6.2 Leases and claims

- `claim #12 [--branch R] --agent A [--ttl 15m|run]` is one durable commit: the task must be live and `ready` on R (no live lease by another holder, no `settled`/`deleted` marker from an unmerged branch, `gates` do not constrain `claim`). Result `{lease: L-<n>, token: <fence>, branch: R, expires}`. Claiming again as the same holder is idempotent.
- Fencing: `complete`, `set --lease`, `release` must present the lease; a stale token fails with exit 5 and names the current holder (Kleppmann's argument [07 §7.4]). Every mutation takes CAS guards (`--if-rev` against `rev_seq`, `--if-status`, `--if-holder`, `--if-tip`); a failed guard returns the current value (exit 4) with what changed and who changed it.
- Liveness: TTL (15 min self-claims; **run-scoped** for dispatcher claims: `--ttl run`, released by `apply`, `run close` or a dead `bg_task_id` — 60 minutes is wrong for the owner's multi-hour runs [22 §2.2]); `heartbeat` (lazy record); the holder's PID recorded so a dead Claude process expires the lease at the next read; `SubagentStop` releases or flags leases held by the stopping `agent_id` (only for hooks that fire; the run-scoped rule covers Workflow agents); `reclaim --older-than 30m`.
- `complete #12 --lease L --outcome done|failed|abandoned --summary - [--evidence …]` writes `done` on the lease's branch, releases the lease **into `settled`**, returns newly ready ids on that branch; refused (exit 6) while a child is open or an open `fail_*` verdict `gates` the task.
- Mutexes (benchmark slot, merge queue): `task{work_kind = mutex}` with the same lease semantics.

### 6.3 Change feed

Every commit has `seq`; each record lists ops, `affected` ids (dependents unblocked, referrers of a tombstone, sources marked suspect, newly ready) and lease/marker events; ref moves are entries. `changes --since <seq> [--branch R|--all] [--about #..] [--for-agent A]` reads records from `HEAD.seq_ring` (last 32) or the `hist` commit index (older), filters by relevance (nodes the agent claimed, is blocked on, authored or cited) and prints one line per change; every command header prints the current `seq`. No file watcher (lossy on Windows [08 §4 W10]), no polling process; the feed is pulled at hook boundaries (`UserPromptSubmit`, `SubagentStart`, optional `mcp_tool` `PostToolBatch` with `${cwd}`/`${session_id}` substitution [08 §6.2]) and, in M6, pushed by the leader to followers and to `watch` (leader-only, never a polling process — F-B5).

### 6.4 Idempotency and Workflow resume

Every write verb and `apply` take a key (MCP: `idempotency_key`); the writer hashes it and the payload (BLAKE3-128 each) and binds the record to the branch. Hit with equal payload → the original result, `replayed: true`; different payload → exit 9 with the original result; different branch → exit 9 unless the original branch was merged into the caller's or deleted after merge (N13e). Entries live 30 days (a Workflow resumed after > 7 days re-runs every completed agent [07 §4.1]) and ride the segment pipeline (40 B each). `apply` batches with local `$refs` are all-or-nothing, keyed once per run (`run:<id>`), and land on the branch derived from the run (`run --runs_in--> lane --moirai_branch`) unless `--branch` overrides; a batch containing a lease whose branch differs from the batch branch is refused before any write (D6). Workflow agents that write directly use content-hash keys, so a re-run critic that emits different findings does not get the old results back (X3).

### 6.5 Durability classes

| Class | Members | Guarantee |
|---|---|---|
| `durable` | every graph mutation, `claim`/`complete`/`release`, `rm`, ref moves, `ClientHead`, `settled`/`deleted` markers, `Idem`, `Pin`, `GitMap`, `Checkpoint`, `apply` batches (one flush per batch) | acknowledged only after `NtFlushBuffersFileEx(DATA_SYNC_ONLY)` returns; survives power loss |
| `lazy` | heartbeats, read cursors, session marks | appended and published; flushed by the next durable commit; may be lost after power loss (documented) |

fsync failure aborts the process (never retried on the same handle); the next writer recovers. Zero-filled extents make every commit a true overwrite (G11). Group commit only in the M6 leader, pipelined, no accumulation timer.

### 6.6 Quiet mode

`moirai quiet on|off` sets `HEAD.flags.quiet`; any lane with status `measuring` implies it (the explicit flag wins). In quiet mode: no automatic delta checkpoints until the tail exceeds 8× the threshold, then exactly one bounded delta runs (G10); no rollup, promotion, GC, export or import (unless `--force`); the MCP server stays resident at 0 % CPU (it has no threads or timers to stop). Because no moirai process has background work in any mode, the idle rule of [02 §9] is satisfied by construction; the flag only suppresses the foreground maintenance a busy writer would otherwise trigger.

---

## 7. Agent interface (final)

### 7.1 CLI surface

Conventions: compact line-oriented text by default (ids first, deterministic order), `--ids` (never prints a header), `--json v1`/`--jsonl` with a versioned envelope `{"v":1,"branch":…,"rev":…,"data":…,"next":…,"dropped":…}`, empty results exit 0, bodies via `--stdin`/`@file` (never argv: PowerShell 5.1 strips quotes [07 §5.2]), `--agent` defaulting to `$MOIRAI_AGENT` → the hook-injected label → `session:<id>`, no ANSI or prompts off-TTY, every verb accepts `--branch R`, `--lease L`, `--client NAME`; every result's first line is `branch: <ref> · rev <seq>`. Exit codes: 0 ok (incl. empty) · 1 internal · 2 usage · 3 not found (tombstone printed) · 4 guard conflict (current value printed) · 5 lease lost/stale token/branch mismatch · 6 precondition (blocked, gating verdict, staged merge, I13/I14) · 7 store unavailable/locked/no git for an image verb · 8 partial batch · 9 idempotency payload/branch mismatch.

```
# context
moirai brief   [--scope #id] [--role R] [--budget-chars 8000] [--more] [--across]
moirai pack    #id --role R [--phase P] [--budget-chars 40000] [--since-round k] [--more] [--record-run]
# read
moirai ready   [--scope #id] [--role R] [--limit 20] [--cursor c] [--ids] [--explain #id] [--across [REFS]]
moirai blocking [--scope #id] [--ids]            # ids of all blocking tasks (is_blocker ∧ kind:task), markers honoured
moirai blockers #id [--transitive] [--explain] [--across]
moirai show    #id.. [@COMMIT] [--full] [--neighbors N] [--across] [--uid] [--markers]
moirai find    [TEXT] [kind:task status:open prio:<=1 area:net done:false suspect:true conflicted:true] [--bodies] [--ids]
moirai tree    #id [--depth N]      moirai notes --path src/net/x.rs      moirai stale [--scope #id]     moirai check #id
moirai changes --since SEQ [--branch R|--all] [--about #..] [--for-agent A]
moirai stats   loop #P | refuted-share --role R [--round k]      moirai lane conflicts #L1 #L2      moirai conflicts [REF]
# write (all: --idempotency-key K, --agent A, --if-rev N, --if-status S, --if-holder H)
moirai add     task|doc|note|rule|decision|question|finding|verdict|measurement|artifact|run|lane|area "title" [--parent #] [--blocked-by #,..] [--blocks #,..] [--field k=v].. [--body -|@file]
moirai rule|note|decision|finding|verdict|measurement --stdin|@file [--critical] [--about #..] [--applies-to role:tester,path:crates/phys/**] [--authority owner --owner-quote @file]
moirai set     #id [k=v].. [--status S] [--done] [--resolution R] [--lease L]
moirai link    #a --blocks|--gates|--parent|--cites[@COMMIT]|--supersedes|--refutes|--confirms|--verifies|--addresses|--implements|--derived-from|--answers|--scoped-to|--depends-on #b
moirai unlink  #a --<kind> #b        moirai move #id --parent #p        moirai reopen #id --reason T
moirai doc patch #section --remove @old --add @new [--depends-on #s1,#s2]     # refuses (exit 4) if @old is not a substring
moirai supersede #old --with #new    moirai retract #id --reason T     moirai answer #q --by owner --verbatim @file
moirai rm      #id [--reason T] [--replaced-by #id] [--cascade|--reparent] [--release] [--dry-run] [--yes]
moirai resolve KEY --take ours|theirs|base|repoint:#id|--value V | --all --policy P
moirai apply   FILE|- [--idempotency-key run:ID] [--branch R] [--dry-run]
# coordination
moirai claim   #id.. | --next [--scope #] [--role R] --agent A [--ttl 15m|run]      moirai heartbeat L     moirai release L
moirai complete #id --lease L --outcome done|failed|abandoned --summary - [--evidence commit:sha|#id..]
moirai reclaim --older-than 30m      moirai run open|close ID [--lane L]
# branches, refs, history (orchestrator rituals; CLI only)
moirai branch [NAME [--from REF|COMMIT] [--kind work|plan]] | --list | -d|-D NAME
moirai checkout REF|COMMIT [--branch-new NAME]      moirai worktree bind DIR REF | unbind DIR | --list
moirai lane open NAME --worktree DIR [--git-branch B] [--base SHA]      moirai lane close|freeze NAME
moirai sync [--check] [--refork]                    # merge main into the current branch
moirai merge SRC [--into DST] [--policy P] [--strict] [--base COMMIT]   moirai merge --continue|--abort   moirai merge-check SRC [--into DST]
moirai cherry-pick COMMIT [--onto REF]   moirai revert COMMIT [--onto REF]   moirai undo [--ref REF] [N] [--expect COMMIT]
moirai tag NAME [COMMIT] [-m MSG] [--pin]   moirai reflog REF   moirai op log
moirai log [REF] [--graph] [--node #N] [--actor A] [--since SEQ] [--all-branches]
moirai diff A..B | A...B [--node #N] [--stat]      moirai blame #N [FIELD]      moirai at COMMIT -- <read verb ...>
# git image
moirai image export [--to DEST] [--refs ...] [--granularity checkpoint|commit] [--object-format sha1|sha256] [--create]
moirai image import [--from DEST|FILE] [--refs ...]      moirai image push|pull [REMOTE]     moirai image doctor [--rebuild-map]     moirai image show COMMIT     moirai image gc
# store, integration, maintenance
moirai init [--link STORE] [--default-branch main] [--force --shadow]      moirai doctor [store|lanes|image|agents|hooks|--verify|--fsck]
moirai gc [--prune] [--reflog-expire 90d] [--cruft-delay 14d]   moirai quiet on|off   moirai migrate
moirai export md --to DIR | memory-md | rules --to .claude/rules/moirai/
moirai hook session-start|prompt|subagent-start|subagent-stop|agent-launched|stamp|git-post-merge|git-post-checkout      moirai mcp [--legacy|--modern|--auto]
```

Example I/O (the owner's two headline requests first):

```
$ moirai blocking --ids
#12
#17
#31

$ moirai blocking --scope #88
branch: main · rev 4471 · 3 blocking tasks (1 settled elsewhere hidden: #89 done on lane/l5np c4470, unmerged)
#12  task in_progress P1 "Byte-range lock protocol"   blocks #51 #52   lease dev#1 L-9 (run r7)
#17  task open        P2 "HEAD slot format"           blocks #51
#31  task open        P1 "Delta segment writer"       blocks #33 #34

$ moirai rule --critical --applies-to role:developer,role:tester --authority owner --owner-quote @ruling.txt --stdin <<'EOF'
Never kill processes by image name; only the PID tree you started.
EOF
branch: main · rev 4472
#212 rule active critical  applies_to={developer,tester}  authority=owner  c4472 by orchestrator  hash 9f3c…e1
(will appear first in every brief and pack; lanes see it as ~main until they sync)

$ moirai blockers #51 --explain --across --branch lane/l5np
branch: lane/l5np · rev 4471 · behind main 3
#51 task open P1 "Wire lease reclaim"  BLOCKED
  #12 task in_progress P1 "Byte-range lock protocol"   (direct; lease dev#1 L-9, 11m left)
  #17 task open P2 "HEAD slot format"                   (direct)
     on main: done c9b1 (not merged into lane/l5np; run `moirai sync`)
  #7  task open P0 "Storage engine M1"                  (inherited from ancestor #9, exogenous)

$ moirai set #12 --status done --if-rev 4460 --lease L-9
error[guard_conflict]: #12 rev_seq is 4468, you passed 4460 (changed at c4468 by dev#2 on lane/l5np: blocker #40 deleted, edge flagged)
current: #12 task in_progress P1 "Byte-range lock protocol"  rev 4468  blockers: #40 (deleted c4468 → flagged; moirai resolve 'edge:#12:blocks:#40')
hint: re-read with `moirai show #12`, or pass --if-rev 4468
(exit 4)

$ moirai rm #40 --reason "dup of #52" --replaced-by #52 --dry-run
branch: lane/l10 · rev 4455
#40 task open P2 "Reader registry"   would be deleted; leases: none; markers: none
  structural: #203 --blocks-> #40   re-point to #52
              #40 --blocks-> #12    re-point: #52 blocks #12
  historical: #17 cites #40 (pinned c4410) → suspect;  #77 mentions #40 (text mention)
run again with --yes

$ moirai merge lane/l10 --into main
merge lane/l10 (c4455f02) into main (c9b2e6c1)
  step 0: lane/l10 behind main by 31 commits → sync first: 212 keys, 3 typed (status join ×2, add-wins ×1), 0 conflicts, 0 violations → c4456 on lane/l10
  step 1: base c4456 (unique); applied 188 keys disjoint
  1 violation -> staged on merge/l10 (exit 6):
  DanglingEdge: main added `#203 blocks #40`; lane/l10 deleted #40 ("dup of #52", replaced_by #52)
    suggested: moirai resolve 'edge:#203:blocks:#40' --take repoint:#52 && moirai merge --continue

$ moirai image export
export main tags/* (checkpoint, sha1) -> <workspace>/BoykoEngine-moirai.git
  1 checkpoint commit (Moirai-Folded: 53 commits c9b2e6c1..c9c0aa17), 171 blobs, 402 trees via git fast-import (0.41 s)
  refs updated: refs/heads/main 3e1f… ; gitmap +53 · cursor seq 4471

$ moirai image import
import refs/heads/main: 2 new git commits
  a91c… native   Moirai-Commit c9c0… verified -> applied on main
  b402… foreign  author git:<owner> "fix typo in rule #212" -> 1 op SetField(#212.text) -> applied on main as c9c1 (import-foreign)
import refs/heads/lane/l10: 1 new git commit
  c77d… foreign merge (2 parents) -> typed 3-way over parents: 4 keys, 1 TextHunk resolved from the git tree, incidents ledger union 3+1+1=5
  nodes/01/8f/…9e40.moi: parse error line 9 (git conflict marker) -> staged on import/lane/l10 (ImageParse)
```

### 7.2 MCP tools (ten; compact text results, no `structuredContent` by default)

| Tool | Purpose | Key params | Load |
|---|---|---|---|
| `brief` | session/role digest within a budget | `role`, `branch`, `budget_chars`, `across` | alwaysLoad |
| `pack` | per-task, per-role context pack with drop footer | `id`, `role`, `phase`, `branch`, `lease`, `budget_chars`, `since_round` | alwaysLoad |
| `get` | nodes by id, optionally at a commit, with neighbours | `ids[]`, `branch`, `at`, `detail`, `neighbors`, `across` | deferred |
| `find` | filter syntax + text + presets (`ready`, `blocking`, `stale`, `conflicts`) | `query`, `text`, `branch`, `preset`, `limit`, `cursor` | deferred |
| `claim` | claim / next / heartbeat / release | `action`, `id`, `scope`, `role`, `agent`, `branch`, `lease`, `ttl` | alwaysLoad |
| `complete` | finish a claimed task; returns newly ready ids | `id`, `lease`, `outcome`, `summary`, `evidence[]`, `idempotency_key` | alwaysLoad |
| `remember` | one knowledge node (rule, note, decision, finding, question, verdict, measurement) with kind-specific validation (`failure_scenario` mandatory for findings; `verdict` writes its `derived_from` edges) | `kind`, `title`, `text`, `fields{}`, `about[]`, `applies_to{}`, `branch`, `lease`, `idempotency_key` | alwaysLoad |
| `write` | atomic batch: create/set/link/unlink/move/doc_patch/transition with `$refs` and guards | `ops[]`, `branch`, `lease`, `idempotency_key`, `dry_run` | deferred |
| `changes` | delta since a seq, or one node's history | `since_seq`, `id`, `branch`, `for_agent`, `limit` | deferred |
| `branch` | read-only: `list`, `status` of a branch, `across` for a set of ids | `action`, `branch`, `ids[]` | deferred |

Branch resolution per call: the `branch` parameter (model-typed from the dispatch marker; ~6 tokens) validated against `lease` when both are given (mismatch → exit 5 text) → for stamped writes the `ctx.cwd` binding → the session's checkout (`session:<id>`). The server prints the resolved branch first in every result. The `PreToolUse` stamp hook (`command` type, `updatedInput.ctx = {agent_id, agent_type, cwd, session_id, role_label}`, `permissionDecision` per owner choice) matches `mcp__moirai__(claim|complete|remember|write)` only (G6); reads are spawn-free. The engine enforces the role write policy on the dispatch label (`ctx.role_label` from the marker, `agent_type` fallback, unknown → the `general-purpose` row: `remember` findings/notes/questions only); client `tools:` allowlists are convenience. Server `instructions` (≤ 2,048 chars) front-load: call `brief` first, `pack` before working a task, `claim` before editing, `complete` when done, `remember` for findings/rules/decisions; ids are `#N`; `(deleted …)` marks a tombstone; never grep the git image, use `get`. Schema ≈ 5k chars (≈ 1.2–1.5k tokens, est.), names-only ≈ 220 chars up front under tool search [07 §5.1].

### 7.3 Role write policy (server-side; label from the dispatch marker, `agent_type` fallback)

| Role | May create / write | May not |
|---|---|---|
| orchestrator | everything on any branch; branch/merge/image verbs; `authority = owner` only with `--owner-quote` | — |
| owner (main session, `--by owner`) | rulings, `question.answered`, `rule{authority=owner}` | — |
| architect | `doc` (plan, section), `decision`, `question`, `deviation` findings | verdicts, `finding.fixed`, task status |
| researcher | `note`, `artifact{research}`, `question`, findings with `confidence` | anything else |
| architecture-critic, code-reviewer | `finding` (with `failure_scenario`), `verdict{role}` (+ `derived_from` edges), own findings → withdrawn | plan text, `finding.fixed`, verdicts on own task |
| refuter (`general-purpose` with `role=refuter`) | `refutes`/`confirms` edges, finding status confirmed/refuted | new findings of other kinds |
| developer | `claim`/`complete` with lease, `files_owned`, `deviation`, `question`, `note`, `artifact{impl}` | verdicts, `finding.fixed` on own task |
| tester | `measurement` (env + `measured_on` mandatory), `artifact{test}`, findings `f_kind=test` | verdicts, task status except `complete` of own claim |
| results-analyst | `verdict{role=analyst, return_to}`, `task{work_kind=debt}` | fixes |
| project-analyst | findings with global `local_id`, notes | verdicts |
| doc-writer | `artifact{page}` + `derived_from` | — |

### 7.4 Context-pack algorithm

`pack #T --role R --phase P --branch B --budget-chars N` (budgets in **characters**; the per-script token ratio for English and Cyrillic is measured in S0 and reported in the footer):

1. **Resolve**: T's ancestor chain; the branch (§5a.4); round `k` from the latest verdict about T; `ahead/behind main`.
2. **Candidate classes** (three renderings each: L0 one line ≈ 80 chars, L1 abstract + key fields ≈ 300 chars, L2 full body):
   - **C1 header** (always, L1): `branch`, `ahead/behind main`, staged merges, worktree/git branch/base/tip/target dir, dirty count, other active lanes' `files_owned` (do-not-touch), quiet state, global critical-rule count.
   - **C2 rules** where `applies_to ∩ {R, P, lane, *} ≠ ∅ ∧ authoritative` on B, **plus** critical rules on `main` not yet merged into B marked `~main` (owner rulings are never hidden by branching); order criticality ↓, authority (owner > orchestrator > measured > research > agent), id ↑; `critical` → L2, `high` → L1, else L0; `contradicts` pairs shown together. Empty `applies_to` = `*`.
   - **C3 target**: T at L2 (body, acceptance, `files_owned`, status, lease, `settled`/`deleted` notices), ancestors L0, open questions blocking T at L1 with options, owner rulings about the subtree at L1 (verbatim, never truncated).
   - **C4 effective spec**: sections reachable via `implements`/`about`; developer → implementation sections L2; tester → metrics-and-validation sections + baselines with env L2/L1; critic → sections with `changed_in_round > k` plus `depends_on` dependents L2, unchanged ones L0 (`unchanged since r<k>`); artifacts L0.
   - **C5 findings** about T: developer → `confirmed` only (L1 with `failure_scenario`); critic round k+1 → own previous findings L0, refuted ones as ids with `do not re-raise`; reviewer → open findings on the same files. Branch-local unless `--across`.
   - **C6 measurements/pins** for the lane and `main`: current pins L1, `stale` marked, known reds L0.
   - **C7 scoped hazards/notes** whose `applies_to` paths intersect T's `files_owned`: criticality-ordered L1/L0.
   - **C8 delta** since this (agent, T) cursor: L0, max 10.
3. **Fill**: per-class minimum quotas (C1 fixed; C2 ≥ 15 %; C3 ≥ 20 %; C4 ≥ 30 % developer/tester, ≥ 40 % critic; C5 ≥ 10 %), then remaining budget by `(class rank, criticality, recency, id)`; degrade L2 → L1 → L0 before dropping; never cut mid-text; owner rulings and critical rules never below L1; a **conflicted** knowledge node renders as one L1 line with the base text and `~conflicted (moirai resolve '#212.text')`, never with markers (N15); `suspect`/`stale` items keep their marker rather than being dropped.
4. **Emit**: deterministic order (prompt-cache friendly); header `moirai pack #51 developer · branch lane/l5np · rev 4471 · 38,900/40,000 chars (~11.1k tokens)`; footer `dropped: 4 findings(optional) #88 #91 #93 #95, 7 notes → moirai pack #51 --more`. Never silent truncation [01 §7 L1].
5. **Record** only with `--record-run`: `consumed` edges with `pinned_commit` from the run to every L1/L2 item, so `moirai check #51` can verify nothing consumed has moved (PLANFENCE-style [06 §11.2]). `pack` is otherwise a pure read.

`brief` is the same machine with fixed classes: checkpoint per open campaign, live lanes (branch, ahead/behind, staged merges, live leases, dirty count), runs in flight, merge queue (`merge_after`), settled-elsewhere and deleted-elsewhere triage lines, open owner questions, top critical rules/hazards, stale summaries, verdicts since last session; default 8,000 chars (the hook cap is 10,000 [07 §4.1]); `export memory-md` renders it into the top of `MEMORY.md`.

### 7.5 Skills and hooks

Skills: `moirai` (core, ≤ 1.5k tokens: verbs, output conventions, `--agent`/`--branch`/`--lease`, stdin bodies, exit codes, one example per verb, "never grep the image; use `moirai show`"; links `reference.md`); `moirai-orchestrate` (preloaded into the orchestrator: campaign/lane setup, dispatcher pattern, `apply` format, verdict routing, loop-termination query, merge-queue ritual, quiet-window checklist); `moirai-report` (preloaded into developer/tester/reviewer/critic: how to `complete`, `remember --kind finding` with `failure_scenario`, `measurement --env --measured-on`); `moirai-branches` (orchestrator only, ≈ 1k tokens: `lane open → sync --check/sync → merge-check → merge → resolve → merge --continue → branch -d → image export`). Shipped as a plugin (`skills/`, `hooks/hooks.json`, `.mcp.json`); the binary installed separately (plugin `bin/` blocks claude.ai/Cowork installs [07 §9.6]).

| Hook (exec form, fail-open, timeouts) | Command | Effect |
|---|---|---|
| `SessionStart` (startup/resume/clear/compact) | `moirai hook session-start` (10 s) | `additionalContext` = brief ≤ 8,000 chars; replaces the hand-written MEMORY.md resume block; prints the bound branch |
| `UserPromptSubmit` | `moirai hook prompt` (5 s) | relevance-filtered delta since the session cursor incl. cross-branch notices (≤ 600 chars); prints nothing when empty |
| `SubagentStart` | `moirai hook subagent-start` (10 s) | role pack: critical rules for the role label, protocol line, `pack` reference for the task in the dispatch table; runs `sync --check` for the bound lane and **auto-applies only a preview with zero conflicts and zero violations**, otherwise prints `behind main: N commits, 1 conflict on #91 → moirai sync` (D5) |
| `PostToolUse` matcher `Agent` (async) | `moirai hook agent-launched` | maps `agentId → {task, lease, branch}` from the `moirai:task=#51 lease=L-9 branch=lane/l5np role=developer` marker |
| `SubagentStop` | `moirai hook subagent-stop` (10 s) | releases or flags leases held by `agent_id`; stores `last_assistant_message` as a `needs-triage` note if a lease is still open (linked by `#N`, rendered through the tombstone if the task is gone); checks I14 expected artifacts; blocks at most once |
| `PreToolUse` matcher `mcp__moirai__(claim\|complete\|remember\|write)` | `moirai hook stamp` (5 s) | `updatedInput.ctx`; `permissionDecision: allow` (owner may choose `ask`) |
| `PostToolBatch` (`mcp_tool`, optional, v1.1) | `changes` with `${cwd}`, `${session_id}` | ≤ 600-char delta for agents holding leases; off by default |
| git `post-merge` / `post-checkout` (optional) | `moirai hook git-post-merge` / `git-post-checkout` | `merge-check` for the bound lane; binding refresh |

`WorktreeCreate` is not used (it replaces worktree creation [07 §4.2]); no `PreCompact` hook (`SessionStart(compact)` re-injects). Whether `SubagentStart`/`SubagentStop` fire for Workflow `agent()` calls is unverified [07 §8.6]; the 5-minute experiment is the first S0 task, and until it passes the dispatcher pattern is the only supported Workflow pattern (run-scoped leases make the safety net unnecessary).

### 7.6 Walk-through: one BoykoEngine-style campaign under full branching

Roles: orchestrator (main session, directory bound to `main`), architect and architecture-critic (no Bash; MCP with `branch=lane/l5np` in their markers), developer and tester (Bash, CLI, in `<lanes-dir>/l5np`).

1. **Session start.** `SessionStart` → brief on `main`: checkpoint, 3 live lanes with ahead/behind, `merge/l10` staged with one `DanglingEdge`, one settled-elsewhere line (`#89 done on lane/l5np, unmerged`), open owner question `#9`, 4 critical rules, `dropped: 7 ready tasks → moirai brief --more`.
2. **Decompose on `main`.** `add task "Narrowphase batching (L5)" --parent #7` → `#88`; subtasks `#89..#93` with `--blocked-by`; `question add --kind scope --blocks #93 …` → `#9`. One commit each.
3. **Open the lane.** `moirai lane open l5np --worktree <lanes-dir>/l5np --git-branch u/l5np --base 7c1e0a` → `branch lane/l5np --from main` (fork c4410, pin), `worktree bind`, lane node `#94` on `main`. Three durable commits, ~6–9 ms.
4. **Design on the lane.** The architect's `write{branch: "lane/l5np", ops: [doc plan #130, section ×6 with depends_on, decision ×3 with alternatives]}` (idempotency key `run:r7/architect/rev1`) lands on `lane/l5np`. The critic's `pack{id: #130, role: architecture-critic, branch: "lane/l5np"}` reads the lane (D2 fixed: the plan exists where the critic looks); findings `#160..#163` and verdict `#164 fail_fixable --gates #89` (with `derived_from` edges to its findings) are lane commits. Refuters write `refutes` edges and status moves; parallel disagreement on one finding is a `StatusFork`, not a race. Termination: `stats loop #130` → `round 1: raised 4, confirmed 2, refuted 2 (50 %); confirmed blockers: 1 → continue`; round 2 after `doc patch #133 --remove @old --add @new` (engine refuses if `@old` is not a substring) → `confirmed blockers: 0 → DESIGN APPROVED`; `#164` accepted, `#89` completable.
5. **A rule lands on `main` meanwhile.** The orchestrator writes rule `#212 --critical` on `main`. The tester's next pack on the lane shows `#212 ~main (not merged; moirai sync)`; the `SubagentStart` hook runs `sync --check`: clean → auto-applied as a `sync` commit on the lane with `sync_base = c4472` and zero resolution ops.
6. **Dispatch on the lane.** `moirai ready --branch lane/l5np --ids` → `#89 #90`; `claim #89 #90 --agent wf:r7/dev#{1,2} --ttl run --branch lane/l5np` (store-wide leases carrying the branch, released by `apply`); Workflow `args` carry ids, leases and `branch`.
7. **Implement, test, verdict on the lane.** dev#1: `pack #89 --lease L-18` (the lease fixes the branch even after the Bash tool reset `cwd` — D3), edits, `add finding "…" --about #89 --confidence observed` for a deviation, returns schema output. Tester writes `measurement --metric narrowphase_ms --value 3.9 --target 4.5 --measured-on 9ab1… --env host=ryzen9,profile=release,load=quiet --stdin`; `check #89` confirms `9ab1…` is on the lane's git tip (commit-graph, no spawn). Reviewer verdict `#180` with `return_to=none`. The orchestrator's `apply results.json --idempotency-key run:r7` (branch derived from run r7 → lane `#94` → `lane/l5np`) completes `#89` with `evidence commit:abc123`: `done` on the lane, lease released into `settled`, `#93` becomes ready **on the lane**; on `main`, `#89` is listed as `done on lane/l5np (unmerged)` and is never re-dispatched (N2/D1 fixed). A resume that re-runs dev#1 hits the idempotency key and changes nothing.
8. **Merge queue.** The merge script runs `moirai merge-check lane/l5np --into main` (gates green, 0 open confirmed findings, pins moved and declared, rulings on `main` the lane predates: none after the sync, `merge_after` prerequisites: `lane/l10` first, conflicted nodes on the lane: 0), then `moirai merge lane/l5np --into main` (sync-first: `main` moved by 2 commits since step 5 → sync with 0 conflicts → merge commit on `main`: 9 findings, 2 measurements, 6 sections, 5 status moves, `#89`'s `settled` marker cleared by ancestry), **then** `git merge u/l5np` in the code repo (D12). One `TextHunk` on section `#91` (both lanes edited it) landed as a conflict value on `lane/l5np` during the sync; `resolve '#91.body' --take theirs` had already cleared it before the merge. `lane close l5np`; `branch -d lane/l5np` releases the pin.
9. **Image.** The post-merge hook runs `moirai image export` (checkpoint of `main` + tags into `<workspace>/BoykoEngine-moirai.git` via fast-import, 0.4 s); `git push` to the private remote is the owner's call. A colleague's hand edit on the image comes back through `image import` as a foreign commit the next morning; the brief lists it.
10. **Next session.** `brief` regenerates from `main`; the old lane is gone from the list; `reflog lane/l5np` still shows its life; nobody edits a resume block by hand.

What changed versus today: no HDR strings, no `.slice()`, findings have ids across rounds, the loop terminates by a query, rulings reach every lane at `sync` and are marked `~main` until then, a task finished on a lane cannot be dispatched twice, the resume block is generated, and the whole graph is browsable in a git tool.

---
## 8. Performance and RAM budget (final) and the benchmark plan

### 8.1 Budget table

Machine: Ryzen 9 5900HS, 16 GB (1.8 GB free with 16 agent processes at 3.5 GB private [M]), consumer NVMe, NTFS, Defender real-time on, CPU load 30–100 % during the measurements [05 §2], [08 §2]. Workload: 3 edges/node stored twice, 60 B titles, 24 B fields, bodies 1 KiB raw → ~340 B zstd-dict (**claimed** ratio, measured in S0), ~10 commits per node lifetime, ~1k commits/day store-wide (est.; the dispatcher pattern makes it lower), 3–5 live lanes, ~50 live refs worst case. Process spawn (15–73 ms native, +~109 ms under the agent's Git-Bash wrapper [M]) is excluded; it dominates every CLI call and is outside the engine.

| Quantity | 1e4 | 1e5 | 1e6 | Derivation / tag |
|---|---|---|---|---|
| Hot index bytes (60 B header + 8 B CSR offsets + 30 B edges + ~1 B bitsets), shared page cache | 1.0 MB | 9.9 MB | 99 MB | 99 B/node [20 §1.1]; evictable; shared by all moirai processes |
| Whole store touched (+ titles, fields, blob table, bodies) | ~5.5 MB | ~55 MB | ~0.55 GB | ~540 B/node; only pages actually read are resident |
| History, `commit` granularity, per 1e5 commits | ~30 MB raw / ~12 MB `hist` | same | same | 0.3 KB/commit raw, zstd 2–3× (est.) |
| History growth with 50 lanes syncing daily (G17) | | ~0.1–0.2 GB/year | | resolutions only [20 F-D2 recomputed] |
| Pinned checkpoint sets, ~50 live branches (disk) | ~5 MB | 25–60 MB | 0.25–0.6 GB | 2–4 bases + retained deltas |
| `gitmap` per 1e5 commits per (dest, algo) | 4 MB | 4 MB | 4 MB | 41 B/entry |
| **Private RSS, CLI or hook process on `main`** | 1.5–3 MB | 2–4 MB | 3–6 MB | native console baseline 0.69 MB [M]; overlay ≤ 4,096 ops × 80–200 B; arena ≤ 256 KiB |
| **Private RSS, CLI reading a lane** | +0.2–1 MB | same | same | branch overlay ≤ 6k ops × ~150 B; streaming builder |
| **Private RSS, MCP server** (`current_thread`, G12) | 3–6 MB + 1 MB × min(active branches, 8) | 4–8 MB + same | 6–12 MB + same | rmcp RSS is **claimed** (U11); LRU K = 8 (G19) |
| Private RSS, leader (M6) | +2–8 MB | | | broadcast ring + group-commit buffers (est., unmeasured) |
| Private RSS, `image export` full (explicit command) | ~2 MB | 12–15 MB | 110–150 MB | lazy parent trees; pack index entries are the O(objects) structure (G24) |
| Open store (`main`) | 0.3–1 ms | 0.5–1.5 ms | 0.5–3 ms | `HEAD` pread + folded-tables pread + ≤ 8 maps × 0.22 ms [M] + tail replay; worst case ~3–5 ms at a full 4 MiB tail (U13) |
| First read on a lane (G15) | +1–3 ms fresh; +5–10 ms at 14 days; +10–20 ms at 60 days | same | same | O(own commits); streaming |
| `get #N` | 1–5 µs | 1–5 µs | 1–5 µs | row + overlay + 3–6 page touches at ~1 µs [M] |
| `ready` (page of 20) | 10–50 µs | 50–300 µs | 0.3–3 ms | bitset AND + ancestor walk ≤ 12 + marker probe per candidate |
| `blocking --ids` | 10–50 µs | 50–300 µs | 0.3–3 ms | `is_blocker ∧ kind:task ∧ ¬settled` scan; printing ≈ 0.1 ms per 1,000 ids |
| `ready --across` (3 listed refs) | +3× branch first-read cost | | | promoted branches use `TOUCH` bitmaps (v1.1) |
| `blockers #N --transitive` | 5–50 µs | 10–200 µs | 20 µs–2 ms | reverse CSR walk, visited bitset, budget 10k |
| `show #N@commit` | 10–50 µs warm; ≤ 0.1–0.5 ms per cold `hist` frame | same | same | chain walk |
| `pack` / `brief` | 1–5 ms | 2–8 ms | 3–12 ms | a few hundred node reads + ~40 body decompressions + rendering; + one lane overlay if on a lane |
| **Durable commit (engine)** | ~2.0 ms p50, ~6 ms p99, 13.7 ms max | same | ~2.2 ms | one `DATA_SYNC_ONLY` flush 1.73–2.0 ms p50 [M]; + Defender close cost **pending S0** (U10) |
| `apply` of N ops | ~2 ms + µs per op | | | one flush per batch |
| `branch`, `checkout`, `undo`, `tag` | 2–3 ms | same | same | one durable commit |
| `sync` (merge-by-reference) | 5–20 ms | 10–40 ms | 20–80 ms | two folds + base as-of per touched key + validators |
| `merge` of a 2k-op lane vs 5k trunk ops | 5–20 ms | 10–40 ms | 20–80 ms (+5–50 ms full Kahn) | §5a.7; CI gate ≤ 50 ms at 1e5 |
| Delta checkpoint (automatic, outside the writer byte) | 5–30 ms | 10–50 ms | 20–100 ms | O(tail) |
| Rollup (`gc` or MCP-after-request only) | 10–30 ms | 0.1–0.3 s | 1–3 s | sequential rewrite at 1–2 GB/s [05 §7] |
| Promotion (v1.1) | 10–40 ms | 20–60 ms | 30–100 ms | delta segment write |
| `image export`, one checkpoint (3 nodes changed) | 40–80 ms incl. one `git` spawn | same | same | fast-import; the spawn is ~74 ms measured for `git --version` |
| `image export`, full | 0.1–0.3 s | 1–2.5 s | 10–25 s | §5b.9 |
| `image import`, full | 0.3–1 s | 2–7 s | 25–70 s | §5b.9 |
| Writer wait, 16 concurrent writers (G1) | last ack ~32–40 ms ideal; p99 ≤ 50 ms gate | | | serial 2–2.5 ms commits; no convoy |
| Whole CLI call from the agent Bash tool | 115–190 ms | 115–190 ms | 120–200 ms | Git-Bash ~109 + spawn 15–73 + engine ≤ 5 ms [05 §14.1] |
| MCP tool call (in-session, unstamped read) | 0.1–5 ms | 0.1–5 ms | 0.2–10 ms | no spawn; stamped writes +15–73 ms |
| FTS tier 1 (titles + abstracts) | 0.2–1 ms | 2–10 ms | 20–80 ms | 260 B/node scan; tier 2 above 20k nodes: 0.1–5 ms per term |
| Cold cache after reboot | +0.3–0.6 ms per point query | | +1–5 ms for `ready` | ~50–100 µs per 4 KiB page, extrapolated (U20) |
| Idle CPU, all processes, all modes | 0 | 0 | 0 | no threads, timers, watchers, polling |

The owner's realistic scale (0.3–0.5 M nodes after three years [02 §12.6]) sits between the 1e5 and 1e6 columns; the RAM target holds across all three because data is shared through read-only mappings.

**CI budget gates** (run on every PR on Windows with Defender on and under a synthetic 16-agent load, and separately on an idle machine; regressions block): engine ≤ 5 ms per CLI command at 1e5 on `main`; ≤ 10 ms on a lane **including** the first-read overlay build at a 14-day fork distance; private RSS ≤ 4 MB CLI / ≤ 10 MB + 1 MB × min(active branches, 8) MCP at 1e5; exactly one flush per durable commit; no O(history) or O(N) work on open (asserted by counting bytes read on open at 1e4 vs 1e6); writer-wait p99 ≤ 50 ms with 16 writers; merge of a 2k-op lane ≤ 50 ms at 1e5; `export → fresh import → export` byte-identical; full export of 1e5 ≤ 3 s; 0 % CPU over 10 min idle for the MCP server; `doctor --verify` clean after every kill-loop run.

### 8.2 Benchmark and verification plan

**S0 measurements on the owner's machine** (Defender on; idle and under typical agent load; p50/p90/p99, private bytes and peak working set via `GetProcessMemoryInfo`, flush counts, page faults; `hyperfine -N --warmup 5` for spawn-to-exit):
1. open → append → `DATA_SYNC_ONLY` flush → close on a 64 MiB log file, n = 200 (the Defender close cost, G8) — decides whether the CLI must forward to a leader early.
2. Blocking overlapped `LockFileEx` contention: 16 writer processes, p50/p99 wait, fairness (G1).
3. Overlay build vs branch age: branches forked 1k / 7k / 14k / 60k commits ago with 50 concurrent writers, with and without the per-ref index (G28) — fixes G15/G16 thresholds.
4. `sync` bytes per lane per day under merge-by-reference vs copying (G28).
5. Pinned file count and disk with 50 branches forked over 40 checkpoints (G28).
6. zstd dictionary ratio on the owner's real notes and plan sections (U12); token/char ratio of English and Cyrillic text with Anthropic's token-counting endpoint (W-all-6).
7. `git` presence on PATH, exec-form PATH resolution of `moirai.exe` for hooks (W-all-3), the 5-minute experiment on whether `SubagentStart`/`SubagentStop`/`PostToolUse(Agent)` fire for Workflow `agent()` calls (W-all-2).
8. Loose-object create + rename cost under Defender, n = 1,000, and full `git gc` time/peak RSS on a 1e5-commit image repo (decides the hand-written writer's loose/pack threshold and the `image gc` guidance, G28).
9. deflate throughput of `zlib-rs` vs `miniz_oxide` on this CPU (only matters for the hand-written writer, U33).
10. Tail replay at a full 4,096-op / 4 MiB tail (U13).

**Oracles and baselines.** Before and while the from-scratch engine is built, the moirai workload (§8.3 workloads a–i) runs against **SQLite (rusqlite, WAL, `synchronous=FULL`)** as the conservative multi-process reference and the S0–S5 throw-away backend, and against **redb 4.x** and **heed/LMDB** as performance baselines [05 §16.1]. The from-scratch engine must beat them on open time and private RSS and match them on commit latency to justify itself; if it cannot at S6, the trait keeps the product working.

**Workloads** (1e3 / 1e4 / 1e5 / 1e6 nodes, synthetic corpora with realistic body sizes): (a) point get; (b) list by status; (c) blocking task ids; (d) transitive subtasks and blockers; (e) create/update/delete with referential cascade; (f) commit, branch, diff, sync, merge; (g) FTS; (h) 8–16 processes mixing reads and writes across branches; (i) crash tests.

**Correctness gates.**
- **Deterministic multi-process simulation** (own workstream, ~20 % of the build): a `Vfs` trait with an in-memory implementation, seeded scheduling of N simulated processes, crash-point enumeration at every write/flush/publish boundary — including between the two appends of a flushed group (N3) — fsync-error injection, lock-release-delay injection, AV-style sharing violations. SQLite's WAL-reset race fell to this method in ~15 minutes [08 §3.1].
- **Windows kill loops**: 16 writer + reader processes in 16 directories bound to different branches, `TerminateProcess` at random points, 10,000 iterations / 1 h; zero lost acknowledged commits, zero corrupt opens, `doctor --verify` clean on every branch head.
- **Property tests**: incremental derived state == full recompute after 1e5 random ops incl. deletes, reparents, merges; reverse CSR == inverse(forward); `invert(changeset)` restores state; pin ⊕ ops == replay-from-genesis per branch; merge is deterministic; disjoint keys commute; a clean merge equals sequential application; no structural violation ever reaches a ref (I12 fuzz); **I25′** (no conflict on keys untouched by one side) over random DAGs with interleaved `sync`/`merge`; **I26′** (no branch can claim or list as a blocker a node another live branch completed or deleted); the merge variant of X1 (one side adds `X blocks P`, the other moves `C1` under `P` and adds `C1 blocks X`); criss-cross shapes; two stores importing one bundle export identical objects; `export → import → export` byte-identical incl. `revert`/`cherry-pick`/`undo`/`Undelete`.
- **Fuzzers**: record and segment parsers, `.moi` parser (block strings, `---` in bodies, CRLF, pathological escapes), pack reader (v1.1).
- **Differential tests** against `git fsck`/`git log`/`git cat-file` on every exported image; `gix` may be used as a read oracle in tests only.
- **Replay of the owner's recorded incidents** as fixtures: the union merge that resurrected RESOLVED as OPEN in 10 places and the 185→188→191 counter [02 §7.3] must yield the correct result through `sync` + `merge` and through a git-side merge of the image.

---

## 9. Roadmap

Sizes: S ≈ a week, M ≈ 2–3 weeks, L ≈ 4–6 weeks of the owner plus agents (est.). Relative effort of the full build (A = 100): ≈ 118–128 units through S5 on the oracle, ≈ 150–160 with S6 and the deferred items [22 §7.1]. Roughly 26–32k lines of Rust plus 12–16k of tests for the v1 slice (est.); the two long poles are the multi-process protocol and image determinism.

| Slice | Scope | Exit criteria | Tests / gates |
|---|---|---|---|
| **S0 — Contract, oracle, measurements** (S) | On-disk format spec v1 (records with LSN + epoch, `HEAD` with table pointers, per-ref index, pins, markers, `gitmap`); the engine trait (open/commit/replay/bitset/CSR/overlay accessors, **branch view = pin ⊕ ops in the interface**); a throw-away SQLite backend; the §8.2 measurements; the hook experiment | numbers recorded; loose/pack threshold, lock bound and G15/G16 thresholds fixed; spec reviewed | bench harness |
| **S1 — Trunk graph + CLI on `main`** (M) — **R2 met** | 13 kinds + `phase_state`/`return_to` + `gates`; schema-as-data; verbs `init` (D4 guard), `--link`, `worktree bind`, `add`, `set`, `link/unlink`, `move`, `doc patch`, `rm --dry-run/--yes` (X4 flagging, `--replaced-by`, I32′), `apply` with `$refs` and payload-bound keys, `ready`, `blocking`, `blockers --explain`, `show`, `tree`, `find`, `changes`, `claim` (run-scoped, store-wide, branch-carrying), `complete` (writing `settled`), `reopen`, `stats loop/refuted-share`, `lane conflicts`, `check`, `notes --path`, `doctor store\|agents`; discovery chain; output contract and exit codes frozen (`--json v1`); every verb already accepts `--branch`/`--lease` though only `main` exists | `blocking --ids` ≤ 100 µs at 1e5 on the oracle; incremental derived state == recompute under 1e6 random ops; every verb documented with an example | property tests; golden outputs; PowerShell 5.1 argv tests; delete-policy matrix |
| **S2 — Packs, brief, hooks, skill, import** (M) — **adoption gate** | C's pack algorithm with the §7.4 header, chars budgets, `*` default, `~main` slot (empty until S3), N15 rendering; `brief`; SessionStart + UserPromptSubmit + SubagentStart/Stop + agent-launched hooks; `export memory-md`; core skill + `moirai-report` + `moirai-orchestrate`; one-off import of the dozen standing rules, current pins as measurements, live lanes, open owner questions | one real campaign on `main` with no HDR and no hand-written resume block; dispatcher pattern only; the orchestrator judges an HDR-vs-pack diff complete (owner-judged, replaces the unmeasurable precision@k); hook output ≤ 8,000 chars with exact drop footers | pack budget tests; hook payload fixtures against Claude Code 2.1.28x |
| **S3 — Branches + merge for the next campaign's lanes** (L) — **R1 met** | refs (log-folded tables), reflog, `ClientHead`, pins, per-ref index, branch overlay (pin ⊕ ops, merge-by-reference sync, no promotion), `branch`/`checkout`/`--list`/`-d`/`tag`/`undo --expect`/`revert`/`cherry-pick`/`log --graph`/`diff A...B`/`show@`/`blame`/`at`; `lane open/close`; typed 3-way merge with base at the LCA, sync-first merges into `main`, single LCA rule, `merge/*` staging, `resolve`, `merge --continue`, `sync --check`/`sync`, `merge-check`, `--across` on `show`/`blockers`/`ready`; `settled`/`deleted` markers honoured everywhere; `apply` branch from the run; `plan/*` kind; `moirai-branches` skill; SubagentStart `sync --check` gating | the owner's two register incidents replay correctly; a lane completes a task and `main`'s `ready`/`blocking` never re-dispatch it (I26′ test); 10 synthetic lanes × 1k ops with every conflict class incl. sync-then-merge, criss-cross and move+inheritance merge deterministically; merge ≤ 50 ms at 1e5; a `plan/*` branch cannot mark work done | property tests of §8.2; I25′/I26′/I31′/I37′ fuzz |
| **S4 — Git image, checkpoint granularity** (M) — **R3 met** | `.moi` encoder/decoder with self-check, tree layout, commit mapping and trailers, `gitmap`, alias side ref, `ImageBackend` over `git fast-import`/`cat-file`; export of `main` + `tags/*` at checkpoint granularity into a separate bare repo (SHA-1), `refs/moirai/*` as a second destination; import with native verification (stated parent ids, `verified` bit), foreign commits, `Undelete`, `incr` ledgers, two-parent foreign merges through the typed 3-way, `ImageParse` staging, `import-checkpoint`; `image doctor --rebuild-map`, `image show`; post-merge export hook | `export → fresh import → export` byte-identical for 1e5 nodes and 1e5 commits; `git fsck` clean; a hand edit, a git-side merge with markers, a git-side merge of two counter increments, a file deletion, a revert-of-delete and a squash each produce the specified result; full export of 1e5 ≤ 3 s | round-trip property tests; `.moi` fuzzer; differential tests against git |
| **S5 — MCP** (S–M) | `moirai mcp` on rmcp dual-era, ten tools with explicit `branch` validated against `lease`, stamp on write tools only, role policy on the dispatch label, plugin packaging | architect and critic complete a round on a lane branch without Bash; schema ≤ 5k chars; MCP private RSS ≤ 10 MB + 1 MB × active branches at 1e5 | conformance tests for both handshakes; `structuredContent` regression |
| **S6 — The from-scratch engine behind the trait** (L) | log/HEAD/LOCK protocol (G1, G2, G11, G18, G25), segments, overlay, frozen bitsets, CSR, delta checkpoints, `hist`, blobs with dictionary, pins, per-ref index, branch overlays, GC; swapped in when it beats the oracle on open time and private RSS | open ≤ 3 ms at 1e6; 16-process Windows kill loop 1 h with zero lost acknowledged commits; DST with crash/fsync/lock-delay injection at every I/O boundary passes 1e6 steps; `doctor --verify` clean on every branch; 50 branches × 2k ops forked 1k–60k commits apart readable within budget | the simulator, kill loops and fuzzers of §8.2 (own workstream, started in S0) |
| **Later (v1.1+)** | branch promotion (`seg.b*`, `TOUCH`), hand-written loose/pack/idx/bundle writer and reader for "no git installed" image I/O (or `gix` if the owner allows it), `commit`-granularity and lane export, SHA-256 images, `rebase --onto`, `op restore`, `--with-oplog`, tracked-directory and orphan-branch destinations, recursive virtual merge base, the leader/`watch`/`PostToolBatch` push (M6), FTS tier 2, schema strengthening migrations, `resource` mutex ergonomics, the `shared` field class if one campaign shows chronic staleness, optional HTTP MCP mode, signed installer/winget | each gated by a measured need | |

Adoption path: S2 replaces the MEMORY.md resume block and the HDR on `main`; S3 turns the next campaign's lanes into branches; S4 gives the owner the backup and the browsable image before MCP because an OS-crash-induced loss of `.moirai/` would end adoption on this host [02 §9]; the 254 memory files stay a read-only archive linked by `artifact` nodes; `OPEN-QUESTIONS.md`/`BACKLOG.md`/`MEASUREMENT-QUEUE.md` become generated views (`export md`), regenerated, never merged.

---

## 10. Risk register

| # | Risk | Likelihood / impact | Evidence | Mitigation | Owner-visible signal |
|---|---|---|---|---|---|
| 1 | The multi-process file protocol silently loses or corrupts an acknowledged write on Windows (delayed lock release, torn tail, AV holding files, fsync errors, restore confusion) | medium / severe | SQLite WAL-reset race 16 years [08 §3.1]; ALICE 60 crash bugs in 11 systems; Beads lost 7 of 8 closes [03 §2.7]; branches add pins and markers to the surface | readers stop at `committed_lsn`; ref move inside the commit; fsync fatal; DST + kill loops as the S6 gate; `doctor --verify` in CI; the oracle backend until S6 passes | a kill-loop failure blocks the S6 swap |
| 2 | Branch isolation fights the workflow: lanes miss rules/completions/deletions on `main`, staged merges pile up | medium / high | every report recommended a shared trunk for this reason [08 §7.2], [02 §7.3]; lanes live for days | `~main` rules in packs; `behind main` notices; `settled`/`deleted` markers; `sync --check` gating; sync-first merges; `resolve --all --policy`; the `shared` field class as an explicit fallback after one campaign | staged-merge count and `sync` conflict rate in `brief`; owner decision #1 |
| 3 | Image determinism drift (float formatting, escaping, sort order, CRLF, format bumps, per-store data) or misclassified foreign commits | medium / medium | export formats classically diverge; hg-git/cinnabar keep explicit maps [D §12 S8–S9] | no store-local data in hashed content; retained encoders per version; byte-identical round-trip tests in CI; the exporter re-parses what it writes; `image doctor` | `image doctor` reports "tampered" commits |
| 4 | Typed merge rules produce plausible but wrong results (lattice hides a disagreement, add-wins resurrects a removed label, diff3 merges semantically conflicting hunks, wrong base) | medium / high | merge-semantics creep is the recurring risk [04 §11]; the owner's incidents were silent merges; N1 was exactly such a wrong rule | conflicts as data by default; base at the LCA; only sets, counters and hierarchy auto-resolve; owner-authority fields never auto-merge; incidents as fixtures; per-kind policies opt-in | `conflicted` counts; fixture failures |
| 5 | The from-scratch engine takes far longer than estimated | high / medium | "95 % of the effort is testing" (folklore but directionally right [04 §3.14]); redb needed years for multi-process; DoltLite ~2,000 PRs | the trait + oracle backend keep adoption independent; the format spec, not the engine, is the contract; S6 is swappable | S6 slipping does not block S1–S5 |
| 6 | Hooks do not fire for Workflow `agent()` calls or hook limits change | medium / medium | unverified [07 §8.6]; hook strings capped at 10,000 chars | dispatcher pattern needs no hooks; run-scoped leases; briefs self-budget | the S0 experiment result |
| 7 | Agents bypass the protocol (forget `complete`, invent ids, write stale values, run `init` in a worktree) | high / medium | LLM compliance is probabilistic; Beads needed `doctor` for exactly this; Beads phantom stores | dispatcher `apply`; CAS guards returning current values; `mentions` parsing; `init` guard and `doctor store`; idempotency keys | `doctor` reports; guard-conflict rate |
| 8 | Defender/NTFS costs on the log close path or on image writes exceed estimates | medium / medium | per-file scan cost [05 §6.4]; U10 unmeasured | S0 measurement; packs via fast-import; leader forwarding as the fallback | S0 numbers |
| 9 | Windows lock-release delay after a killed writer stalls all writers | low / medium | "depends upon available system resources" [05 §6.3]; the owner kills wholesale | 2 s bounded wait naming the holder and its liveness; liveness only, never correctness | exit-7 frequency |
| 10 | Disk growth on a disk-starved machine (history, pins, image repo) | medium / low | [02 §9] | checkpoint-granularity image default; merge-by-reference sync; `gc` with reflog/cruft expiry; `doctor` pin report; `image gc` | `doctor` disk report |
| 11 | Two roles' worth of vocabulary (branches, resolve keys) overloads the orchestrator | low / medium | [22 §4.3] | CLI-only branch verbs; `moirai-branches` skill; resolve commands printed verbatim in merge output | orchestrator token use per lane |
| 12 | Claude Code / MCP behaviour changes per release (`structuredContent`, handshake, hook fields) | medium / low | [07 §2.6], [07 §4] | text-first output; dual-era server; per-version hook fixtures | conformance tests |

---

## 11. Owner decisions

Deduplicated from the eight reports, the four proposals and the three critiques; questions the owner update already answered (branch model, git independence, git image existence) are dropped. Ordered by how much the answer changes the design.

| # | Decision | Options | Recommended default | Consequence of the alternative |
|---|---|---|---|---|
| 1 | **Coordination liveness under full branching.** Accept git-like isolation (lanes see `main`'s completions/rules/deletions at `sync`) with `settled`/`deleted` markers, `~main` rules and `--across` views, or add an opt-in `shared` field class (status and `blocks` live on `main` for all branches)? | isolation + markers; `shared` class | isolation + markers for one campaign, then revisit | the `shared` class moves into S3 and T3′ becomes "full branching for knowledge, shared status" — closer to B; less isolation, more liveness |
| 2 | **Build order.** Run S1–S5 on a throw-away SQLite backend behind the engine trait (branch views in the interface) and land the from-scratch engine at S6, or engine first? | oracle first; engine first | oracle first (never released outside the owner's machine; deleted at S6) | engine first → ~60 % of the work precedes the first adoption gate [22 §2.7]; the "from scratch" requirement is met either way |
| 3 | **Git object I/O for the image.** `git fast-import`/`cat-file` when git is present (v1), hand-written loose/pack layer (v1.1), or `gix`? May `image push/pull` spawn `git`? | fast-import first; hand-written; `gix` | fast-import first, hand-written later, `gix` only as a test oracle; spawning `git` allowed with a printed fallback | hand-written first adds 4–6k lines + fuzzing before R3 is usable; `gix` adds a large dependency; forbidding the spawn makes the image unusable until v1.1 |
| 4 | **Image destination, refs, granularity, object format.** | separate bare repo / `refs/moirai/*` / orphan branch / tracked dir; `main`+tags / all refs; checkpoint / 1:1; SHA-1 / SHA-256 | separate bare repo, `main` + tags, checkpoint per merge + daily, SHA-1; `refs/moirai/*` optional second destination | 1:1 for all lanes ≈ 0.5–4 GB/year and a `git gc` ritual; tracked directory requires the ledger form and PR-review discipline; SHA-256 cannot be pushed to public servers yet |
| 5 | **Branch-per-lane ritual.** One moirai branch per lane opened by `lane open` (scratch and `wf_*` worktrees write to `main`), or automatic branches per git worktree/branch? Which R1 "ideally" verbs are v1? | per lane; per worktree | per lane; v1 = `tag`, `undo`, `revert`, `cherry-pick`, `reflog`; `rebase`, `op restore` later | per worktree pins ~44 sets and creates merges nobody asked for |
| 6 | **MCP branch identity.** Explicit `branch` parameter typed from the dispatch marker and validated against the lease, or stamp every MCP call (identity + branch for reads, +15–73 ms each)? Permission posture of the stamp: `allow` or `ask`? | parameter; stamp all; allow / ask | parameter; `allow` for reads and lease-scoped writes, `ask` for `rm`, `merge`, owner-authority fields | stamping all costs a spawn per read; `ask` everywhere interrupts every agent write |
| 7 | **Read-only roles writing through MCP** (architect, critic, researcher, project-analyst) while keeping no file Write/Edit? | yes under the role policy; no (orchestrator persists everything) | yes | no → "the plan does not exist" persists for Bash-less roles unless the orchestrator's `apply` carries their output |
| 8 | **Bodies.** Inline ≤ 64 KiB with dedup and dictionary compression, or pointers only? | inline 64 KiB; 16 KiB; pointers | inline 64 KiB; artifacts as pointers | pointers only → no diff3, no removed-text guard, no `git diff` over prose |
| 9 | **Deletion semantics.** A deleted blocker leaves a flagged edge (dependents stay blocked until `resolve`/`--replaced-by`), or drop-and-notify? Refuse `rm` under a live lease unless `--release`? | flag; drop-and-notify; restrict everything | flag; refuse under lease | drop-and-notify can hand a task to an agent whose prerequisite is open (X4) |
| 10 | **Merge default.** Value conflicts land as conflict values (jj) or `--strict` (Dolt) by default? Structural violations always staged. | land; strict | land | strict stalls autonomous agents on every text hunk |
| 11 | **Cross-machine sync / cloud agents writing the same graph in v1?** | no; yes | no (`uid` is stored from day one so v1.1 sync needs no migration) | yes → `uid` primary, `#N` a per-store alias, image import on the daily path |
| 12 | **Durability and quiet windows.** Lazy class for heartbeats/cursors (lost on power loss); one bounded delta checkpoint allowed at 8× the tail threshold during a quiet window; the MCP server may stay resident at 0 % CPU; `moirai quiet on` in the window driver (explicit flag wins over the lane `measuring` status)? | as stated; flush everything; unbounded tail | as stated | flushing everything doubles heartbeat cost; an unbounded tail makes open cost grow during long windows |
| 13 | **Language and authority.** English for node text, owner quotes stored verbatim as given; `authority = owner` writable only from the main session with `--owner-quote`; Russian allowed in `brief --lang ru`? Commit-policy rule: which of the two contradictory rules does moirai encode? | as stated; Russian nodes | as stated; the owner picks the commit rule and moirai marks the other superseded | Russian node text halves pack capacity per character budget |
| 14 | **Retention.** History forever (cold-packed), reflog 90 d, cruft 14 d, idempotency 30 d; `gc --squash-before` as an owner command? | as stated; prune by age | as stated | pruning by age loses as-of/blame for old nodes |
| 15 | **Migration and generated views.** Import the ~50 standing feedback rules, dated lessons, current pins, live lanes and open owner questions as typed nodes (owner-reviewed), archive the 254 memory files as read-only artifacts, and replace `OPEN-QUESTIONS.md`/`BACKLOG.md`/`MEASUREMENT-QUEUE.md` and the MEMORY.md resume block with generated views from S2? Dev Drive / Defender exclusion as an optional speed-up only? | as stated; bulk import; keep hand-maintained | as stated | bulk import spends weeks on 182k words; keeping hand-maintained registers keeps the dual source of truth |

---

## 12. Appendix: what we deliberately do not build (anti-requirements)

| Not built | Why | Evidence |
|---|---|---|
| A separately managed daemon or auto-started server | recurring failure in prior art; still pays the CLI spawn; lifecycle, version skew, job-object kills | [08 §5], [03 §8.2], [05 §14.2] |
| Any background activity: polling, file watchers, timers, auto-compaction, telemetry, network calls | the owner's quiet-window idle rule; `ReadDirectoryChangesW` is lossy | [02 §9], [08 §4 W10] |
| A CoW B+tree / prolly-tree / Merkle-state engine for current state | 4 KiB × depth per tiny commit; hardest to build; RAM tracks history size | [04 §3.2, §6], [05 §16 B] |
| Git as the engine (objects in the project ODB as the live store, or working-tree JSONL as the source of truth) | worktrees diverge; merge drivers are not cloned and GitHub ignores them; loose-object churn under Defender; Beads deleted this path | [08 §7.1], [04 §9 C] |
| Automatic moirai branches following git branches/worktrees | unstable branch identity (44 worktrees, detached HEADs, `wf_*`); silent context switches; git operations do not map to DB merges | [08 §7.2] |
| A shared "coordination plane" that never branches, or provenance-scoped knowledge instead of branches | overruled by R1 | owner update |
| A CRDT core | never-failing merges hide conflicts; whole document resident; no DAG-cycle prevention | [04 §3.11, §9 D] |
| Soft delete as a status | leaks into every filter; unnecessary in a versioned store | [06 §8.1] |
| Hierarchical or kind-prefixed ids; UUIDs shown to agents | position ≠ identity bugs; 24 tokens per UUID | [06 §9.1] |
| A query language (Cypher/GQL/Datalog) in v1 | ~50 % execution accuracy; commands + filters cover the hot queries | [06 §12.2] |
| Embeddings or a vector index in the core; `tantivy` | model RAM dominates; lossy for agent trajectories; many files | [05 §13], [03 §6.3] |
| Storing transcripts, journals, scratchpad bulk, build artifacts, code-symbol indexes, gate runners | GBs per session; owned by the harness/LSP/graphify | [02 §13] |
| LLM-driven rewriting of stored facts (summaries that replace sources) | 7 of 13 doc-rot repairs wrote new falsehoods; summaries outlive retractions | [02 §13] |
| Textual union merges of authority-bearing records; numbers without provenance; one growing record per campaign | the owner's incidents | [02 §7.3], [02 §13] |
| Policy enforcement by blocking tool calls | the existing hooks deliberately never block; moirai answers queries, hooks decide | [02 §13] |
| A `lease` node kind; `pack` as a write; 28 node kinds; `chars/3.5` token budgets; `#N` parsing without the sigil rule | see [22 §2, §3.3] | [22] |
| Auto-migration of the on-disk format on open | stranded users in prior art; readers refuse newer formats; `migrate` is explicit | [03 §8.2] |
| Beads-compatible import, GitHub/Linear sync, HTTP MCP, a human UI | not needed by the owner's workflow in v1; MCP HTTP only if non-Claude agents appear | [03 §10], [07 §11] |
| Storing secrets or owner chat | none needed; credential-shaped content is rejected on write | [02 §13] |

---

## 13. Sources

Research reports: `docs/research/01-boyko-workflow-roles.md`, `02-boyko-workflow-orchestration.md`, `03-landscape-agent-memory-and-trackers.md`, `04-versioned-storage-designs.md`, `05-rust-storage-perf-ram.md`, `06-graph-data-model-integrity.md`, `07-agent-integration-cli-mcp-skills.md`, `08-concurrency-sync-git-interop.md`, and the digest `00-phase1-digest.md`. Proposals: `design/10-proposal-A-lean-embedded.md`, `11-proposal-B-git-faithful.md`, `12-proposal-C-agent-workflow-first.md`, `13-proposal-D-branches-git-image.md`. Critiques: `design/20-critique-perf-ram-windows.md`, `21-critique-semantics-correctness.md`, `22-critique-agent-fit-buildability.md`. External precedents are cited through those documents (jj git-compatibility and conflicts, git hash-function transition, gitoxide crate status, Dolt git remotes, git-bug, hg-git/cinnabar, Fossil, SQLite session extension, Kleppmann move operation, Pearce–Kelly, Beads issues and releases, Claude Code hooks/MCP/workflow docs, Microsoft `LockFileEx`/`NtFlushBuffersFileEx`/`CreateFileMapping` documentation), each verified on 2026-09-25 by the report or critique that quotes it. No web request was made while writing this synthesis.

*End of synthesis 30.*
