# 03 — Landscape: agent-memory systems and agent-oriented task trackers

*Research lens: what already exists, how it is built, what broke, and what gap is left for moirai.*
*Date: 2026-09-25. Status: research only, nothing implemented. Nothing in this report was installed or run.*

---

## How to read the evidence tags

Every external claim carries a tag and a source (URL list in §11).

| Tag | Meaning |
|---|---|
| **[M]** | **Measured by this research** on 2026-09-25, e.g. GitHub API star counts, release dates, asset sizes, PyPI/npm/crates.io versions. |
| **[R]** | **Reported measurement** by a third party (bug report, maintainer post, blog) with a concrete setup. Not reproduced here. |
| **[C]** | **Claimed** by the project's authors or vendor (README performance tables, self-reported benchmark scores, marketing). |
| **[D]** | **Documented behaviour**: read in official docs or source code. Not exercised. |
| **[S]** | **Secondary source** (news article, third-party review, search-engine summary). Lowest confidence. |

Snapshot of maturity signals (all [M], GitHub API / package registries, 2026-09-25):

| Project | Stars | Last push | Lang | Latest release |
|---|---|---|---|---|
| gastownhall/beads (was steveyegge/beads) | 27,422 | 2026-09-25 | Go | v1.3.0, 2026-09-15 (v1.3.1-rc.1 2026-09-21) |
| Dicklesworthstone/beads_rust (`br`) | 1,107 | 2026-09-25 | Rust | crate 0.7.0, 2026-09-25 |
| Dicklesworthstone/mcp_agent_mail | 2,164 | 2026-09-22 | Python | — |
| modelcontextprotocol/servers (memory server inside) | 90,592 | 2026-09-22 | TS | `@modelcontextprotocol/server-memory` 2026.8.31 |
| basicmachines-co/basic-memory | 4,039 | 2026-09-24 | Python | 0.23.2, 2026-08-25 |
| letta-ai/letta / letta-code | 24,884 / 3,424 | 2026-09-10 / 2026-09-25 | Py / TS | letta 0.33.2, 2026-09-25 |
| mem0ai/mem0 | 66,005 | 2026-09-25 | Python | mem0ai 2.2.1, 2026-09-25 |
| getzep/graphiti | 31,160 | 2026-09-24 | Python | graphiti-core 0.30.2, 2026-09-08 |
| topoteretes/cognee | 30,980 | 2026-09-25 | Python | 1.6.1, 2026-09-24 |
| langchain-ai/langmem | 1,684 | 2026-09-09 | Python | 0.0.30, 2025-10-27 |
| agiresearch/A-mem | 1,185 | 2025-12-12 | Python | — |
| MemTensor/MemOS | 11,584 | 2026-09-23 | TS/Py | MemoryOS 2.0.33, 2026-09-03 |
| oraios/serena | 29,801 | 2026-09-24 | Python | — |
| eyaltoledano/claude-task-master | 28,088 | 2026-04-28 | JS | task-master-ai 0.43.1, 2026-03-31 |
| MrLesk/Backlog.md | 6,845 | 2026-09-24 | TS | 1.53.0, 2026-09-24 |
| cjo4m06/mcp-shrimp-task-manager | 2,148 | 2025-08-21 | JS | 1.0.21, 2025-07-06 |
| github/github-mcp-server | 33,192 | 2026-09-25 | Go | — |
| BloopAI/vibe-kanban (sunsetting) | 28,190 | 2026-09-19 | Rust | — |
| automazeio/ccpm | 8,384 | 2026-03-18 | Shell | — |
| git-bug/git-bug | 10,337 | 2026-09-25 | Go | — |
| memvid/memvid | 16,556 | 2026-07-14 | Rust | memvid-core 2.0.140 |
| tursodatabase/agentfs | 3,425 | 2026-06-03 | Rust | — |

---

## 0. TL;DR

1. **Beads is the closest prior art and the richest source of lessons, most of them negative.** It became
   a de-facto standard for agent task graphs (27k stars [M]). Its **data model** is good and worth copying:
   typed dependency edges split into blocking and non-blocking, a `ready` queue, `discovered-from`,
   hash IDs, atomic claim, `prime` context injection. Its **storage history** is a cautionary tale:
   SQLite + JSONL-in-git + a daemon (Oct 2025 – Jan 2026), then Dolt-only with the daemon, SQLite and JSONL sync
   deleted, **"~70,000+ lines total"** [S], then embedded Dolt restored for solo users, then an HTTP server
   (`bd serve`) re-added in v1.3.0 (Sep 2026) to amortise per-process startup cost. Open issues report lost writes under
   concurrent agents (**7 of 8 closes lost** [R]), phantom databases in git worktrees [R], orphaned
   dependency rows [R], stale denormalised `is_blocked` flags [R], a **120 GB** memory blow-up from a
   recursive SQL cycle check [R], idle Dolt servers at **~2 GB RSS and 8–38% CPU each** [R], and
   **5–10 s per command** in remote mode [R].
2. **Knowledge-memory systems (mem0, Zep/Graphiti, Cognee, Letta, MemOS, Basic Memory, the MCP memory server)
   do not model tasks, readiness or blockers.** Most of them need a server stack: Neo4j, FalkorDB, Postgres or Qdrant.
   None of them guarantees engine-level referential integrity across versions. The MCP memory server rewrites its whole
   JSONL file on every mutation and serialises writes only inside one process [D].
3. **Versioning is converging on "git for agent state"**: Beads on Dolt (cell-level merge), Letta Code's
   *Context Repositories* (memory as git-backed files, subagents in worktrees, Feb 2026 [D]), and small Rust
   projects (grite, bones, braid, indra_db, prollytree) built on event logs, CRDTs or content addressing. **No
   project yet combines a native graph engine, engine-enforced reference integrity, git-like
   commits/branches/merge, typed task-and-knowledge nodes, a small embedded footprint, and Windows-first
   multi-process safety.** That combination is moirai's gap.
4. **Agent interface economics favour CLI + skill + hooks over MCP.** Beads' own docs claim ~1–2k tokens
   for CLI+hooks against 10–50k for MCP schemas [C]. Task Master ships 7/15/36-tool tiers at ~5k/10k/21k tokens [C].
   Claude Code itself now **omits its Task tools on newer models by default** because "the tools'
   definitions and reminders take up context" [D].
5. **On memory benchmarks, simple, well-driven retrieval is competitive.** A filesystem-plus-grep agent scored 74.0% on
   LoCoMo against 68.5% for mem0-graph [C, Letta]. Controlled studies show a single variable such as the embedding
   model can flip conclusions [C, MemDelta]. For *agentic* trajectories, similarity-only retrieval is "lossy" and
   causal/graph structure helps [C, AMA-Bench]. Headline scores are often inflated: MemPalace's 100% claims were
   retracted [S]. None of the popular benchmarks measures task-graph correctness, the core of moirai.
6. **The owner's harness (Claude Code) already ships partial substitutes.** It has shared Task lists
   (`CLAUDE_CODE_TASK_LIST_ID`), agent-team claiming with file locks, auto memory (200 lines / 25 KB of
   `MEMORY.md` injected at start), and per-subagent memory directories [D]. Main-conversation auto memory is **not**
   loaded into subagents [D]. moirai must add value over these: cross-worktree, cross-agent, versioned,
   integrity-checked shared state with budgeted context packs.

---

## 1. Method

- Primary sources first: repository READMEs, docs trees, Go and TS source (`types.go`, `index.ts`), release notes,
  GitHub issues, papers (arXiv), official vendor docs. Secondary sources only when primaries were blocked.
  Medium blocked direct fetches of Steve Yegge's posts, so those claims are marked [S].
- Currency was checked on 2026-09-25 through the GitHub REST API, PyPI JSON, the npm registry and the crates.io API ([M] rows above).
- The Beads deep dive read the **current** tree (`gastownhall/beads@main`) and the **legacy** tree at tag
  `v0.49.6` (the last SQLite-era line) to reconstruct the architectural shift.
- The web-search budget ran out late in the session. The Rust long-tail survey (§5) therefore used the GitHub
  search API and README reads. Those projects are small and their numbers are almost all [C].

---

## 2. Beads (`bd`): deep dive

### 2.1 Identity and trajectory

- "Beads — a memory upgrade for your coding agent", by Steve Yegge. Go. Introduced in the Medium post
  *Introducing Beads: A coding agent memory system* (2025-10-13 [S]). The repository now lives at
  **`github.com/gastownhall/beads`**; `steveyegge/beads` redirects there [M]. The Go module path is still
  `github.com/steveyegge/beads`, published 2026-09-15 [D, pkg.go.dev]. The docs site is `beads.gascity.com`.
- The README pitch: "persistent, structured memory for coding agents" that "replaces messy markdown plans with a
  dependency-aware graph" [D].
- The author says it was built entirely by agents: "100% vibe coded … 130k lines of Go … roughly half tests", later
  "never looked at Beads either, and it's 225k lines of Go code" [S, Medium posts via search snippets]. The v1.3.0
  release alone "includes 1,342 commits" since the v1.1 line [D, release notes].
- Beads is the data plane of Yegge's multi-agent orchestrator **Gas Town** and its successors (Gas City, and "Wasteland",
  launched March 2026) [D, DoltHub blog].

**Timeline of storage and architecture:**

| When | Version | Architecture | Source |
|---|---|---|---|
| Oct 2025 | ≤ v0.20.0 | SQLite (`.beads/beads.db`, gitignored) plus `.beads/issues.jsonl` committed to git as the "git-backed source of truth"; **sequential IDs** `bd-1, bd-2` | [D] legacy ARCHITECTURE.md; [S] ascii.co.uk |
| late 2025 | v0.20.1 | Switch to **hash IDs** (`bd-a1b2`) with adaptive length | [S] ascii.co.uk; [D] hash-ids.md |
| Nov 2025 – Jan 2026 | ≤ v0.49.x | Per-workspace **daemon** (RPC over a Unix socket `.beads/bd.sock`), 5-second debounced incremental JSONL export, auto-import when JSONL is newer, content-hash merge, 3-way merge engine, tombstones, git hooks, "sync branch" | [D] v0.49.6 ARCHITECTURE.md; [S] tiby.fr |
| early Feb 2026 | v0.50 / v0.51 | **Dolt-only.** Daemon/RPC removed ("~24,000 lines deleted"); SQLite, the JSONL sync layer, the 3-way merge engine and tombstones deleted ("~70,000+ lines total"); `bd sync` made a no-op and later removed | [S] jdillon/vscode-beads#65 (2026-02-16); [D] DoltHub blog |
| Feb 2026 | v0.56 | Server mode needed a running `dolt sql-server`. A user objected that it was "still single-user local, but with a MySQL server in the middle", that file-watch change detection was gone, and that ports clashed across projects | [R] beads#2050 (2026-02-23) |
| Mar–Apr 2026 | ~v0.58 → 1.0 | **Embedded Dolt** restored as the default ("Beads Classic"): about 120 commits, 86+ storage methods, a shared `issueops` library, multi-process concurrency tests, 2 Dolt bugs found | [D] DoltHub blog 2026-04-02 |
| May–Jul 2026 | 1.0.x → 1.1.0 (2026-07-04) | Explicit consent for schema migrations, `bd metrics`, idempotent init | [D] releases |
| 2026-08-11 | 1.2.0 / 1.2.1 | **Untested release that auto-migrated schemas v53 → v65**. Users then saw "schema version mismatch … 12 migrations ahead"; v1.2.2 re-shipped the v1.1 code | [D] v1.2.2 notes |
| 2026-09-15 | **1.3.0** | `bd serve` **HTTP API (41 OpenAPI operations)** "instead of a `bd` subprocess forked per call"; **work leases with heartbeat/reclaim**; **compare-and-set** guards (`--if-assignee`, `--if-status`, exit code 13); `bd sync` federation verb; durable append-only **events journal**; `--brief` output 93.4% smaller | [D] v1.3.0 notes |

**Reading:** in 11 months Beads changed its storage substrate twice and its process model three times
(daemon → no daemon → optional server). Each change was a response to the previous one's failure modes.

### 2.2 Data model (current, from `internal/types/types.go`) [D]

**Issue** is one wide record. The fields fall into these groups:

- *Content:* `Title`, `Description`, `Design`, `AcceptanceCriteria`, `Notes`, `SpecID`, `Labels[]`, `Comments[]`,
  `Metadata` (free JSON), `ExternalRef`, `SourceSystem`.
- *Workflow:* `Status`, `Priority` (0–4), `IssueType`, `IsBlocked` (**a denormalised cache**), `Assignee`, `Owner`,
  `EstimatedMinutes`, `DueAt`, `DeferUntil`, `StartedAt`, `ClosedAt`, `CloseReason`, `ClosedBySession`.
- *Concurrency (added in 1.3):* `LeaseExpiresAt`, `HeartbeatAt`, `LeaseGrantedNode`, `RowVersion` (for CAS).
- *Provenance:* `CreatedBy`, `CreatedAt`, `UpdatedAt`, `ContentHash`, `SourceRepo`, `IDPrefix`, `Actor`.
- *Compaction:* `CompactionLevel`, `CompactedAt`, `CompactedAtCommit`, `OriginalSize`.
- *Orchestration extensions (Gas Town):* `Ephemeral`, `NoHistory`, `WispType`, `StorageClass`, `Pinned`,
  `IsTemplate`, `BondedFrom`, `AwaitType/AwaitID/Timeout/Waiters` (gates), `SourceFormula`, `MolType`,
  `WorkType`, `EventKind/Target/Payload` (message-like beads), `Sender`.

**Statuses:** `open, in_progress, blocked, deferred, closed, pinned, hooked`.
**Types:** `bug, feature, task, epic, chore, decision, message, molecule, gate, spike, story, milestone, event`.
The original model had five types; the list grew as Gas Town pushed orchestration concepts into the tracker.

**Dependency (edge) types:** 19 constants, grouped in the source as:

| Group | Types | Affects `ready`? |
|---|---|---|
| Workflow | `blocks`, `parent-child`, `conditional-blocks` (B runs only if A **fails**), `waits-for` (B waits for all of A's children) | **yes** (`AffectsReadyWork()`) |
| Association | `related`, `discovered-from` | no |
| Graph links | `replies-to`, `relates-to`, `duplicates`, `supersedes` | no |
| Entity | `authored-by`, `assigned-to`, `approved-by`, `attests` | no |
| Convoy / reference | `tracks`, `until`, `caused-by`, `validates` | no |
| Delegation | `delegated-from` | no |

Details worth noting:
- Beads distinguishes `IsBlockingEdge()` (blocks, conditional-blocks, waits-for) from `IsSchedulingEdge()`
  (blocks, conditional-blocks, parent-child). parent-child *propagates* blocked state downward ("children blocked when parent
  blocked") but does not make an epic wait on its children [D, dependencies.md]. **The docs contradict each other:**
  `issues.md` says parent-child has "No impact" on the ready queue while `dependencies.md` says it blocks
  [D]. A typed, single-source edge taxonomy would avoid that.
- `conditional-blocks` decides "failure" with `IsFailureClose()`, which **scans the close reason for failure
  keywords** [D]. Deriving a typed outcome from free text is fragile; moirai should store the outcome as a typed field.
- `discovered-from` is the distinctive agent-era edge. It records that work was found while doing other work, so agents
  file side-quests instead of dropping them [D].
- Cycles are **rejected at write time** (`bd dep add` checks before committing), and `bd dep cycles` lists existing
  ones [D].

Legacy SQLite schema at v0.49.6 [D]: tables `issues`, `dependencies (issue_id, depends_on_id, type)`, `labels`,
`comments`, `events`, `config`, `metadata`, **`dirty_issues`** (export tracking), **`export_hashes`**,
**`child_counters`** (hierarchical IDs), `issue_snapshots`, `compaction_snapshots`, `repo_mtimes`. It also had views
`ready_issues` (a **recursive CTE**) and `blocked_issues`. The dirty and export-hash tables are the plumbing that the
dual source of truth (SQLite ⇄ JSONL) required.

### 2.3 `bd ready` semantics [D]

- An issue is ready when "ALL of its blocking dependencies are closed". The queue excludes `in_progress, blocked,
  deferred, hooked`. `--include-deferred` shows future `defer_until` items. Sort orders are `priority` (default),
  `hybrid` and `oldest`. `--claim` atomically claims the first match; `--explain` says why an item is or is not ready;
  `--mol` filters to molecule steps.
- v1.3.0 fixed dated defers that never woke up: "One deployment measured **241 beads, including P1s, silently dark**" [R, release notes]. The fix is a lazy "wake sweep" on reads of the ready front.
- Readiness depends on the `IsBlocked` cache. #6608 (2026-09-18) shows `bd doctor --fix` removing dependency rows
  with raw SQL, bypassing `RecomputeBlocked()`, so **`bd ready` hides work that should be ready** [R].

### 2.4 IDs: from sequential to hash, and why [D]

- **Why:** sequential IDs "break when multiple agents create issues simultaneously" and when "different branches
  have independent numbering" [D, hash-ids.md]. Collisions under concurrent multi-branch creation were the
  motivating bug [S, v0.20.1 coverage].
- **How:** a hash of content + timestamp + random salt, shown as `prefix-<base36>`. The legacy architecture doc
  says "derived from random UUIDs" [D]. **Adaptive length** follows the birthday bound
  `P ≈ 1 − e^(−n²/2N)`, N = 36^len, with a 25% maximum collision probability by default. That gives 4 characters for 0–500 issues,
  5 for 501–1,500, and 6+ above, capped by `max_hash_length` = 8. On collision it tries len, len+1 and len+2 with 10 nonces each
  [D, adaptive-ids.md].
- **Hierarchical children:** `bd-a3f8.1`, `bd-a3f8.1.1`, up to 3 levels, backed by a `child_counters` table [D]. This is
  a sequential counter under a hash root. It is safe only while one writer creates children of a given parent.
- Resolution accepts partial prefixes (`bd show a1b2`) and fuzzy titles. On import, a colliding ID gets a
  disambiguator [D].
- **Lesson:** coordination-free identity is non-negotiable in multi-branch, multi-agent settings. Short display
  IDs must be *views* over a longer internal identity.

### 2.5 Storage backends over time

**(a) Classic (≤ v0.49): SQLite + JSONL + git + daemon** [D]
- Write path: CLI → SQLite, mark dirty, 5 s debounce, incremental export to JSONL, optional commit via git hook.
- Read path after `git pull`: JSONL newer than DB triggers auto-import, which merges by content hash (same ID + same hash =
  skip, different hash = update, new ID = insert).
- Deletions travelled as **tombstones** (`status=tombstone`), because an import "cannot infer that records absent
  from an export were deleted" [D, sync-concepts.md].
- Failure modes recorded by users: the DB and JSONL drift apart ("Database out of sync with JSONL. Run 'bd import'
  first… bd hangs" [R, Discussion #380]); **`bd update` rewrote `issues.jsonl` from local SQLite, "silently dropping
  issues only present in jsonl from other sessions"** [R, #3931]; daemon repo-ID mismatches ("Database repo ID:
  d1f9ca0c vs Current repo ID: 01eac8ea") and wrong-remote sync that made issues "silently vanish" [R, frr.dev];
  "the sync model is deceptively manual" [R, tiby.fr].

**(b) Dolt server mode** [D, docs/architecture/dolt.md]
- A `dolt sql-server` runs on port 3307, or 3308 for a shared server in `~/.beads/shared-server/`, and serves multiple concurrent writers.
  Auto-commit defaults to **off** in server mode "to prevent 'database is read only' errors under concurrent load".
- Operational failures [R]:
  - #4282: 7 orphaned `dolt sql-server` daemons, each ~2 GB RSS, 8–38% CPU while idle, spiking to 300–600% combined, costing ~67 W of battery.
  - #3760: 4 idle agents opened **41 new DB connections/s**, costing **393–557% CPU**. An opt-in `bd serve` prototype cut that to 0.4 conn/s and 30–80% CPU.
  - #4102: remote mode `bd ready` 5.1 s, `bd stats` 10.3 s, against a 0.22 s network RTT. Causes: a cold pool per process and 10–15 serial queries per command.
  - #6064: an old server with a new client corrupted a journal.

**(c) Embedded Dolt (current default)** [D]
- Dolt runs in process with data in `.beads/embeddeddolt/`, a **single writer enforced by a file lock** ("database is locked"
  on contention), and **one Dolt commit per write** by default.
- Beads pins **Dolt v2.2.0** because v2.3.0+ has a `DOLT_RESET('--hard')` regression that hit 3 of 60 and 3 of 100 test databases [D].
- Dolt GC is generational; `dolt gc --full` is sometimes needed [D].
- Storage footprint: one repository with **2,682 issues had a ~282 MB `.beads/dolt`** directory [R, #4475].
- Binary size: `beads_1.3.0_windows_amd64.zip` = **53,984,039 bytes** compressed [M]; the Linux amd64 tar.gz is 53.2 MB [M].

**Current source of truth:** "the local Dolt database is the source of truth for `bd list`, `bd show`, `bd ready`,
and every write command"; `.beads/issues.jsonl` "is an export … for viewers, interchange, migration, and backup",
"not the source of truth or a backup" [D].

### 2.6 Git sync and merge behaviour [D]

- Dolt data rides on the **same git remote** under **`refs/dolt/data`**, separate from `refs/heads/*`. `bd bootstrap`
  probes `origin` for that ref and clones the issue DB automatically. `bd dolt push` / `bd dolt pull` sync it. v1.3's `bd sync`
  does pull + conflict detection + push, and "conflicts are detected positively, from the merge's own captured
  conflict rows … never inferred from the pull's exit status" [D].
- Dolt merges are **cell-level** (row × column), not line-based. **Dolt branches are independent of git
  branches**. `bd vc status/commit/merge`, `bd history <id>`, `bd diff`, and `bd branch` expose the history
  [D].
- Consequence: a feature branch in git does *not* carry its own issue state unless the user also branches Dolt.
  Worktrees of one repo share (or fail to share, see below) one DB.

### 2.7 Multi-agent and multi-worktree story

Mechanisms [D]:
- **Atomic claim** (`bd update <id> --claim` / `bd ready --claim`: "the first claim wins").
- **Merge slots**, an exclusive-access primitive for conflict-prone operations.
- **Assignee routing**; **leases, heartbeat, reclaim and CAS** since v1.3.
- **Contributor mode**, which routes planning issues to a separate repo, and **stealth mode**, which keeps `.beads` out of the main repo.
- `BEADS_DIR` for git-free use.
- Multi-repo "federation" and routing docs.

Reported failures under the exact workflow the owner runs (orchestrator + N agents in worktrees) [R]:
- **#4767** (bd 1.1.0, embedded, direct mode): coordinator + 8 headless agents, each in a nested worktree. **7 of 8 `bd close`
  calls reported success but did not persist.** Synthetic stress (300+ ops) did not reproduce it; the loss correlated with
  "full agent tool-loops" load. The reporter works around it with read-back retries. Open, untriaged.
- **#4135** (1.0.3–1.0.4, Apr–May 2026): six failure modes, including daemon/worktree **split-brain** after a branch
  switch, auto-flush on read wiping JSONL, config-precedence bugs, bootstrap writing to the wrong directory,
  `project_id` UUID drift blocking writes, and **only the first `bd close` in a chained bash command persisting**. Open.
- **#6551 / #6552** (v1.2.2, Sep 2026): `bd prime` resolves the DB through a different code path. In a worktree
  it silently created an empty "phantom" embedded DB and returned **0 of 96 persistent memories**. "Every agent starting in a
  worktree ran without the project's accumulated knowledge, for weeks, with nothing indicating a problem." The
  phantom store then "permanently shadows the main repo's database". Open.

**Lesson:** DB discovery across worktrees, and durability-on-ack under real agent load, are where agent trackers
actually fail. Neither is visible in a single-process demo.

### 2.8 MCP server and agent interface [D]

- `beads-mcp` (PyPI, 1.3.0, 2026-09-15 [M]) shells out to the `bd` CLI. Its tools are `init, create, list, ready, show, update,
  close, dep, comment, blocked, stats`.
- The maintainers **recommend against MCP when a shell exists**: "the CLI + hooks approach is recommended over MCP. It
  uses ~1-2k tokens vs 10-50k for MCP schemas" [C].
- `bd prime` emits "AI-optimized workflow context" and can inject memory bodies at session start. Hooks call it.
- `--json` everywhere. `--brief` (−93.4% payload) and `--brief-deps` (−89.3%) arrived in v1.3 [C/R, release notes].
- `bd query` offers a small filter language (`status=open AND priority<2`, with parentheses) [D].
- v1.3 `bd serve`: HTTP + OpenAPI with bearer tokens [D].
- An npm wrapper `@beads/bd` (1.3.0 [M]) and Homebrew packaging exist.

### 2.9 Compaction and memory decay [D/S]

- The README advertises "Semantic 'memory decay' summarizes old closed tasks to save context window" [D].
- Mechanism: `bd compact --analyze --json` lists candidates (closed ≥ 30 days, "Tier 1"; Tier 2 at 90 days was planned [S]).
  The **agent** writes the summary and `bd compact --apply --id … --summary file` stores it, so the LLM work runs on the agent's own budget.
  Fields `CompactionLevel/CompactedAt/CompactedAtCommit/OriginalSize` record it [D].
- v1.1.2: "Compaction now archives before discarding" [D]. `bd compact` / `bd flatten` also squash **Dolt history**
  and then run GC [D].
- **Wisps** are ephemeral beads with TTL-based compaction. They leak: 804,727 orphaned wisp rows, 68% of all wisp rows, on
  one production store [R, #6487]; ~40–50 dangling dependency rows per hour [R, #4673].
- **Key-value memories:** `bd remember "insight"`, `bd recall <key>`, `bd memories [search]`, `bd forget`. `bd prime`
  tells agents *not* to write `MEMORY.md` files [D]. #6626 notes that `prime` never mentions `recall`, so agents mis-verify memories
  [R]. This is a string key/value store bolted onto a task tracker, not a knowledge graph.

### 2.10 Referential integrity [D + R]

- `bd delete` [D]: by default it **refuses** if dependents outside the deletion set exist. `--cascade` deletes dependents
  recursively; `--force` deletes and **orphans** dependents. It removes dependency links in both directions and **rewrites
  text references in directly connected issues to `[deleted:ID]`**. `--dry-run` previews.
- Integrity is enforced **in application code, not by the engine**. The results [R]:
  - #4673: wisp deletion cleaned `dependencies` but not `wisp_dependencies`, leaving 2,902 orphaned rows on one DB.
  - #6487: 804,727 orphaned wisp rows.
  - #6608: `bd doctor --fix` deletes rows with raw SQL, bypasses the storage layer and leaves `is_blocked` stale.
  - #4475: `bd doctor`'s cycle check, a recursive CTE up to depth 100 that concatenates paths as strings, drove Dolt to a
    **120.6 GB peak footprint** on 2,682 issues. The suggested fix is the Go-side DFS already used by `bd dep cycles`.
- There is a whole `bd doctor` subsystem to *detect and repair* integrity drift. #3369 proposes decomposing it.

**Lesson for moirai:** "if node 40 is deleted, every referrer knows immediately" has to be an *engine
invariant*: the reverse index is updated in the same atomic commit as the forward edge, and derived state
(readiness) is maintained on the one mutation path. Otherwise the project ends up with a `doctor`.

### 2.11 Performance and RAM summary

| Observation | Value | Tag |
|---|---|---|
| Windows amd64 release archive | 54.0 MB (zip) | [M] |
| Per-call CLI startup | ~230 ms cold, ~151 ms warm | [R] #3760 |
| Remote-mode latency | `ready` 5.1 s, `stats` 10.3 s | [R] #4102 |
| Idle `dolt sql-server` | ~2 GB RSS, 8–38% CPU each | [R] #4282 |
| Connection storm with 4 idle agents | 41 conn/s, 393–557% CPU | [R] #3760 |
| Pathological cycle check | 120.6 GB peak | [R] #4475 |
| On-disk | ~282 MB for 2,682 issues | [R] #4475 |
| Claim of original design | "Local queries complete in milliseconds" | [C] legacy docs |

### 2.12 Beads-derived and competing projects

| Project | What it is | Notes |
|---|---|---|
| **beads_rust (`br`)**, Jeffrey Emanuel | Rust port that **freezes "classic beads"**: SQLite (via **FrankenSQLite**, pure-Rust SQLite) + JSONL; no daemon, no hooks, no auto-commit; explicit `br sync --flush-only / --import-only / --merge / --reconcile` (bare `br sync` is rejected); `BEGIN IMMEDIATE`; `--json/--robot`; optional MCP (`br serve`); "cooperative admission control" per actor/harness/session | 1,107 stars [M]. Binary 27.7 MB (glibc) / 26.6 MB (musl) [C]. The rationale is the upstream's divergence toward Gas Town; "Steve has given his full endorsement" [C] |
| **beads_viewer (`bv`) / beads_viewer_rust (`bvr`)** | Graph-aware TUI with a `--robot-*` JSON triage API (PageRank / critical path) | Referenced by bones and Agent Mail |
| **mcp_agent_mail** | Async "mail" between agents: identities (adjectives + nouns), inbox/outbox, **advisory file reservations with TTL**, storage in git (Markdown audit) + SQLite FTS5. Beads owns task state; Mail owns conversation and leases. The installer now provisions `br` rather than Go `bd` | 2,164 stars [M] |
| Community ecosystem | 25+ tools: TUIs (Mardi Gras, perles with its "BQL"), web UIs, VS Code / JetBrains / Neovim plugins, native apps, a TS SDK, orchestration kits | [D] community-tools.md. The **vscode-beads** extension broke completely at v0.50 (socket RPC removed) [S] |
| **bones** (Rust) | "CRDT-native issue tracker": append-only `.bones/events/*.events`, ITC clocks, deterministic merge, rebuildable SQLite projection, **duplicate detection on create** (FTS5 + vectors + structural, RRF) | Inspired by beads/bv [C]. Small |
| **braid** (Rust) | One **Automerge** document (a "skein") per project, synced through an Automerge sync server; no git, no daemon; `braid ready` | The doc ID is a bearer secret; the default public relay stores data unencrypted [D] |
| **grite** (Rust) | Append-only event log in **git refs** (`refs/grite/wal`), CRDT merge, **sled** materialised view, optional daemon, distributed locks with TTL | Claims create ~5 ms, list of 1k issues ~10 ms, CLI ~15 MB RSS, daemon ~30 MB [C] |
| **chainlink** (Rust) | Lean SQLite tracker: sessions with hand-off notes, "breadcrumbs" that survive compaction, sub-issues, deps, Claude Code hooks | 359 stars [S, GitHub search]. "Code-AI_Generated" badge |
| **PlanDB** (Rust) | "Compound graph": containment to any depth plus dependency edges that cross containment boundaries; `split/insert/pivot`; atomic `go`; critical path; BM25 over tasks and "context" entries that **surface automatically on claim**; CLI/MCP/HTTP | 105 stars [S]. Used by SWE-AF [C] |
| **bead-rs**, **pearls**, **firetrail**, **task-graph-mcp** (Rust) | Small SQLite or git-native agent task graphs; task-graph-mcp (0.5.0, 2026-03-04 [M]) has 6 configurable edge types with "blocking behaviour none/start/completion", phases, atomic claim, advisory file marks | Very young, single-digit stars |

---

## 3. Knowledge-memory systems

### 3.1 Official MCP "memory" server (`modelcontextprotocol/servers/src/memory`)

- **Model:** entities (unique `name`, `entityType`, `observations: string[]`), directed relations
  `(from, to, relationType)` in active voice [D].
- **Storage:** one **JSONL** file (`MEMORY_FILE_PATH`). **Every mutation reads the whole file and rewrites it**:
  `loadGraph()` then `saveGraph()` [D, index.ts].
- **Concurrency:** a `mutationQueue` Promise chain "serializes all read-modify-write graph mutations behind a single
  queue". This is **in-process only**, with no OS file lock, so two Claude Code sessions each spawning the server can race [D].
- **Integrity:** `create_relations` **validates that both endpoints exist**; `delete_entities` **cascades** relation
  removal (`relations.filter(r => !names.includes(r.from) && !names.includes(r.to))`) [D]. References inside observation
  text are not tracked. Identity is the name, and there is no rename tool.
- **Query:** `search_nodes` is a case-insensitive **substring** match over name, type and observations; `open_nodes` / `read_graph`
  [D]. No versioning.
- **Lesson:** it is the minimal viable knowledge graph, and it is what many users start with. It proves the
  entity–relation–observation triple is enough for agents. Whole-file rewrites and process-local locking are
  exactly what moirai must not do.

### 3.2 Basic Memory (basicmachines-co)

- **Model:** Markdown files are the source of truth. Frontmatter has `title/type/tags/permalink`. **Observations** are
  `- [category] text #tag`; **relations** are `- relation_type [[Target]]` wikilinks [D].
- **Storage:** a SQLite (or Postgres) index with FTS and optional vectors (FastEmbed; Milvus/pgvector in v0.23) plus reranking;
  bidirectional file ⇄ DB sync. Cloud offers snapshots and per-note history [D].
- **Integrity:** **forward references allowed**: "Relations can link to notes that don't exist yet. When those notes
  are created later, the connections are already in place." **Permalinks stay stable on rename or move** [D]. Link resolution
  tries exact identity, then a unique case-insensitive alias, and otherwise leaves the link **unresolved** [D].
- **Interface:** MCP (`write_note, read_note, edit_note, search_notes, build_context` over `memory://` URLs), CLI, and
  skills [D].
- **Lesson:** *unresolved-but-typed* references are a useful state (agents often write about things before they
  exist), and **stable identity separate from human names** is essential. Worth copying for knowledge nodes.

### 3.3 Letta / MemGPT, and Letta Code's MemFS

- **Classic Letta:** in-context **memory blocks** (a labelled string section with a character limit, editable by agents
  through memory tools, **shareable across agents**); **archival** (vector) and **recall** (conversation history) memory
  out of context. All state lives in a DB (Postgres) behind the Letta server [D].
- **Sleep-time agents** manage memory asynchronously ("processes, summarizes, and rewrites memory blocks while the user is
  idle") [D].
- **Context Repositories / MemFS (2026-02-12):** Letta Code projects the agent's memory into a **git-backed file
  tree**.
  - `system/` is always loaded; the file tree is always in the prompt; frontmatter descriptions act as signposts.
  - "Every memory edit is committed".
  - **Memory subagents work in git worktrees** and "merge their changes back through git-based conflict resolution".
  - Built-in skills: init, reflection (sleep-time), and "defragmentation" into "15–25 focused files" [D].
  - The post reports **no benchmarks** [D].
- **Lesson:** a respected memory vendor moved *from* DB-resident memory tools *to* git-versioned files that agents
  handle with ordinary tools, citing concurrency across subagents. That validates moirai's "git-like versioning"
  requirement. It also shows the competitor to beat is "files + git", which has zero query semantics and no integrity.

### 3.4 mem0

- **Architecture (2026):** the April-2026 algorithm replaced the two-pass `ADD/UPDATE/DELETE` extraction with
  **single-pass ADD-only extraction** ("never overwrites a fact"). It adds **entity linking** and **multi-signal retrieval** that fuses
  semantic, BM25 keyword and entity scores, plus temporal reranking. Graph memory, previously Neo4j-backed, is not mentioned in the
  current algorithm docs [D, mem0 blog; README].
- **Claims:** LoCoMo **92.5**, LongMemEval **94.4**, BEAM-1M **64.1**, BEAM-10M **48.6** at ~6.7–7.0k tokens/query. The post
  does not state the judge or answer model [C].
- **Storage:** pluggable vector stores (Qdrant, pgvector, and others), LLM calls on write; library, self-hosted server or cloud.
  "OpenMemory" is an MCP front end [D].
- **Lesson:** *append-only facts + multi-signal fusion* beat *mutable facts + vector only*, even by the vendor's own
  account. Every write costs an LLM call, a latency and cost profile moirai should avoid on its hot path.

### 3.5 Zep / Graphiti

- **Model:** a **bi-temporal** knowledge graph. Facts (edges) carry `valid_at/invalid_at` (world time) and
  `created_at/expired_at` (system time). **Episodes** are the provenance of every derived fact. Contradicting facts are
  **invalidated, not deleted** [D].
- **Storage:** an external graph DB: Neo4j, FalkorDB or Amazon Neptune. Kuzu support is **deprecated** after Kuzu was
  archived (Oct 2025). Hybrid search (BM25 + embeddings + graph traversal), MCP server, REST [D].
- **Claims:** the Zep paper (arXiv 2501.13956) reports DMR 94.8% vs MemGPT 93.4%, LongMemEval accuracy +18.5% and latency
  −90% vs baseline [C]. In the public dispute with mem0, Zep said its LoCoMo score is 75.14% ± 0.17 when
  run correctly, and it criticises LoCoMo itself [C].
- **Lesson:** bi-temporal validity and "invalidate, don't delete" are exactly right for *rules, decisions and
  findings* that get superseded. The server-DB dependency is exactly wrong for moirai's footprint goals.

### 3.6 Cognee

- An ECL pipeline (`add → cognify → memify → search`) that builds a knowledge graph plus vectors from documents and code, with custom
  ontologies. It can run on Postgres + pgvector alone, or on Kuzu / LanceDB / SQLite / Neo4j. MCP server. 1.6.1 (2026-09-24) [M/D].
  There is a separate "Cognee-RS" Rust repository [D].
- **Lesson:** it is a document-to-graph ingestion engine, not a task or decision store. It is useful as a mental model for an
  optional "ingest code/docs into the knowledge graph" plugin.

### 3.7 LangMem (LangChain)

- Semantic, episodic and procedural memory, managed in the **hot path** (agent tools `manage_memory`, `search_memory`) or by a
  **background manager** that extracts and consolidates. Storage is LangGraph `BaseStore` (in-memory, or Postgres in production) [D].
- **Activity:** last PyPI release 0.0.30 on **2025-10-27** [M]; 1.7k stars. It looks like maintenance mode.
- **Lesson:** the hot-path vs background split matters. moirai should give agents cheap explicit writes and leave
  consolidation to an optional, separately scheduled job.

### 3.8 A-MEM (paper, arXiv 2502.12110)

- Zettelkasten-style notes with LLM-generated keywords, tags and context descriptions. It **links automatically** to related
  notes, and **memory evolution** means new notes can rewrite attributes of old ones. It improves over baselines on
  LoCoMo across six models [C]. Repository last pushed 2025-12-12 [M].
- **Lesson:** automatic linking is useful. Automatic *rewriting* of old memories without versioning destroys
  provenance. moirai should make every such change a commit.

### 3.9 MemOS (MemTensor)

- "Memory OS" with **MemCubes** (composable, isolated memory units per user/project/agent). Plaintext, tool traces and personas;
  the research framing also covers activation (KV cache) and parametric memory. Self-hosted on **Neo4j + Qdrant**; a local plugin variant on
  **SQLite FTS5 + vectors**. 2.0 "Stardust"; last update 2026-08-17 [D].
- **Claims:** LoCoMo 88.83, LongMemEval 89.20, "35.24% token savings" [C].
- **Lesson:** even an "OS" framing falls back to SQLite FTS + vectors for the local, embeddable variant.

### 3.10 Claude Code native memory and task facilities (the owner's harness) [D]

- **CLAUDE.md hierarchy:** managed, user (`~/.claude/CLAUDE.md`), project, and local (`CLAUDE.local.md`, per worktree only). `@path`
  imports (still loaded at launch), `.claude/rules/*.md` with `paths:` frontmatter for on-demand scoped rules, and
  `AGENTS.md` interop. "Target under 200 lines per CLAUDE.md file."
- **Auto memory:** `~/.claude/projects/<project>/memory/` holds a `MEMORY.md` index plus topic files. "All worktrees and
  subdirectories within the same git repository share one auto memory directory." It is **machine-local**. Only "the first 200
  lines of `MEMORY.md`, or the first 25KB" load at start; topic files are read on demand.
- **Subagents:** "The main conversation's auto memory isn't loaded into subagents." A subagent's `memory: user|project|local`
  field gives it its own directory (`~/.claude/agent-memory/<name>/` or `.claude/agent-memory/<name>/`) with the same 200-line /
  25 KB preload.
- **Memory tool (API):** `{"type":"memory_20250818","name":"memory"}`, a **client-side** file API (`view, create, str_replace,
  insert, delete, rename`) rooted at `/memories`. The API injects "ALWAYS VIEW YOUR MEMORY DIRECTORY BEFORE DOING ANYTHING
  ELSE" and "ASSUME INTERRUPTION" into the system prompt. It is designed to pair with context editing and compaction.
- **Tasks:**
  - `TaskCreate/TaskGet/TaskUpdate/TaskList` with dependencies (blocked-by / blocks); completed tasks automatically unblock
    dependents; persisted under `~/.claude/tasks/`.
  - `CLAUDE_CODE_TASK_LIST_ID` shares a task list across sessions.
  - **Agent teams** share a task list and **"Task claiming uses file locking to prevent race conditions"**. Mailboxes are JSON
    files in `~/.claude/teams/{team}/inboxes/`. Known limitation: "Task status can lag: teammates sometimes fail to mark tasks as completed,
    which blocks dependent tasks."
  - **By default the Task tools are provided only on Claude 3.x, Opus 4–4.7, Sonnet 4–4.6 and Haiku 4.5.** Other models omit them,
    because "Claude keeps track of multi-step work without a written checklist, and the tools' definitions and
    reminders take up context". Opt in with `CLAUDE_CODE_ENABLE_TODO_TOOLS=1`.
- **Lesson:** the harness provides *session-scoped* coordination primitives (shared lists, file-locked claims,
  mailboxes) but **no versioning, no knowledge graph, no integrity, no cross-machine story, and no typed queries**.
  moirai should complement them: feed context through hooks, and possibly back the task list. It should not add another
  large always-on tool surface.

### 3.11 Serena memories [D]

- Plain Markdown files in `.serena/memories/`, plus global ones in `~/.serena/memories/global/`. Tools `write_memory`,
  `read_memory`, `list_memories` (plus edit/delete). An **onboarding** pass writes the initial memories and users are told to
  review them. There is an open request for semantic memory search (#994).
- **Lesson:** "named Markdown files + list + read" is the baseline every coding agent already understands.
  moirai's CLI should be *at least* this easy.

### 3.12 Other notable 2026 entrants (brief)

| Project | Idea | Tag |
|---|---|---|
| **OpenViking** (ByteDance, 38.7k stars, partly Rust) | Context as a virtual filesystem (`viking://`) with **L0 abstract / L1 overview / L2 full** tiers per directory (`.abstract.md`, `.overview.md`); directory-scoped retrieval | [D]; claims 80–83% accuracy vs 24–57% native, −34 to −91% input tokens [C] |
| **Mastra Observational Memory** | Observer + Reflector agents maintain an **append-only observation prefix**, with no per-turn retrieval, so prompt-cache hit rates stay high | LongMemEval 94.87% (gpt-5-mini), 84.23% (gpt-4o) [C] |
| **memvid v2** (Rust) | Single-file `.mv2` memory: embedded WAL, BM25 + HNSW, append-only "Smart Frames", optional AES-GCM | "10–100× faster" than v1 [C] |
| **AgentFS** (Turso, Rust core) | All agent state (POSIX-like FS, KV, tool-call audit log) in **one SQLite file**; snapshot by copying the file | [D], beta |
| **prollytree** (Rust) | Prolly-tree (Merkle B-tree) KV store with **git-backed commits, branches, 3-way merge**, GlueSQL layer, optional vector index; pitched for agent memory | [D], 34 stars [M] |
| **indra_db** (Rust) | "Git for knowledge graphs": content-addressed entries, branch/checkout/diff/log, semantic search, cloud remote | [C], tiny |
| **minigraf** (Rust) | "SQLite of bi-temporal graph databases": single `.graph` file, Datalog, time travel, WASM | [C], 36 stars [S] |
| **mnestic** (Rust) | Maintained fork of **CozoDB** (Datalog relational-graph-vector) "as a substrate for agentic memory": cached graph projections, **budgeted weighted traversal** to fill a fixed context window, bitemporality | [C]; upstream Cozo's last commit was 2024-12-04 [D] |
| **Engram** (Go) | Single binary, SQLite FTS5 memory for coding agents | [S] OSS Insight |
| **MemPalace** | Viral April 2026; 100% LongMemEval/LoCoMo claims **walked back** after an audit (the questions it failed were tuned on; top_k=50 on LoCoMo) | [S] Vectorize, OSS Insight |

---

## 4. Agent-oriented task trackers

### 4.1 Task Master AI (`claude-task-master`)

- `tasks.json` holds `id, title, description, status, dependencies[], priority, details, testStrategy, subtasks[]`. IDs
  are **dotted sequential** (`1`, `1.2`). Tags provide workstreams. PRD parsing and complexity analysis use LLM providers [D].
- MCP tool tiers: **core 7 tools ≈ 5k tokens, standard 15 ≈ 10k, all 36 ≈ 21k (default)**, set with `TASK_MASTER_TOOLS` [C].
- Last npm release 0.43.1 on **2026-03-31**; last push 2026-04-28 [M]. Momentum has shifted to the company's
  hosted product ("Hamster") [D].
- **Lessons:** a single JSON file plus sequential IDs is a merge-conflict magnet across worktrees. Publishing the tool-tier
  token costs is a good practice to copy.

### 4.2 Backlog.md

- One Markdown file per task in `backlog/` with frontmatter; IDs `TASK-N` (configurable prefix); dependencies,
  milestones, acceptance-criteria checklists; MCP server (`backlog mcp start`) plus web Kanban; 1.53.0 (2026-09-24) [M/D].
- **Cross-branch ID allocation:** by default (`checkActiveBranches=true, remoteOperations=true, activeBranchDays=30`)
  it **reads task files on local and remote branches** to avoid handing out duplicate sequential IDs. The docs warn this
  "may impact performance on large repositories" [D].
- There is also cross-branch *status* resolution. Its tie-break has had bugs (#1024: "most_progressed status resolution never
  runs") [R].
- **Lesson:** sequential IDs plus git branches force expensive global scans and heuristic status merges.
  Coordination-free IDs and a **monotone status lattice** avoid both (see 01-boyko report §0.5 on union-merges
  resurrecting `OPEN`).

### 4.3 Shrimp Task Manager

- MCP-first: `plan_task / split_tasks / execute_task / verify_task / reflect_task / research`, "init project rules", JSON
  files in `DATA_DIR`, task-history backups [D].
- Last release **2025-07-06**, last push 2025-08-21 [M]: **stale**.
- **Lesson:** prompt-heavy workflow tools age quickly as models improve. Durable value sits in the *data layer*.

### 4.4 GitHub Issues and Linear through MCP

- **GitHub:** sub-issues GA (Apr 2025 [S]); **issue dependencies ("blocked by" / "blocking") GA 2025-08-21** with REST,
  webhooks and search filters, up to 50 links per relationship type; `gh issue` gained `--blocked-by/--blocking` and
  JSON fields on 2026-06-10 [D]. The official `github-mcp-server` (Go, 33k stars [M]) exposes issues.
- **Linear:** a hosted remote MCP (`mcp.linear.app/mcp`, OAuth), launched 2025-05-01 and extended in Feb 2026 to
  initiatives and milestones; blocking relations can be set at creation [D/S].
- One practitioner abandoned Beads for "Linear CLI + Claude Code Tasks" and reports **"I created 49 issues in under a minute"**
  with the CLI, while the MCP needed escaping workarounds [R, frr.dev].
- **Lesson:** network trackers are fine for *strategy-level* items and humans. At agent speed (hundreds of calls per
  session), offline worktrees and sub-10 ms reads, they are the wrong substrate. moirai should *bridge* to them optionally
  and not compete with them.

### 4.5 CCPM and Vibe Kanban

- **CCPM:** PRD → epic → tasks as `.claude/epics/<feature>/NNN.md` (renamed to the GitHub issue ID after sync), **one git worktree
  per epic**, `depends_on / parallel / conflicts_with` metadata, GitHub Issues as the source of truth [D]. Last push
  2026-03-18 [M].
- **Vibe Kanban** (Rust backend, SQLite, a worktree per task, MCP): **its company Bloop shut down on 2026-04-10**. The project
  continues as community open source, and server features were removed [D/S].
- **Lesson:** "worktree per task" is now a standard pattern, and trackers must treat worktrees as first-class. Depending on
  a VC-backed hosted component is a real risk: Vibe Kanban's server features, Kuzu's archival.

### 4.6 Non-agent prior art that solved the same distribution problems

| System | Relevant design | Tag |
|---|---|---|
| **git-bug** (Go, 10.3k stars [M]) | Issues as **operation packs** stored as git objects under custom refs; **Lamport clocks** order operations; concurrent edits produce a DAG joined by an empty merge commit; append-only, so "merges nearly always succeed" | [S/D] |
| **Radicle COBs** (Rust) | "Collaborative objects": each change is a signed git commit in a per-object **DAG**; state is materialised by a **topological, causally consistent reduce**; a generic COB layer for issues and patches | [D] docs.rs radicle::cob, LWN |
| **TaskChampion** (Rust; Taskwarrior 3 storage) | Replicas with local SQLite; sync by exchanging **operations**; published sync spec | [D] |
| **Dolt** (Go) | Prolly-tree storage, cell-level diff/merge, branches, and **data refs pushed to git remotes** (`refs/dolt/data`) | [D] |
| **Automerge** (Rust core) | JSON CRDT with sync protocol (used by braid) | [D] |

---

## 5. Rust projects in this exact space (2025–2026)

Collected with the GitHub search API (`language:Rust`, pushed ≥ 2026) and README reads. Stars as of 2026-09-25 [S from search
listing] except where marked [M].

| Project | Category | Storage / versioning | Maturity |
|---|---|---|---|
| beads_rust (`br`) | Beads-classic tracker | FrankenSQLite + JSONL; explicit sync | 1,107 [M]; most mature Rust tracker |
| vibe-kanban | Worktree/agent Kanban | SQLite (sqlx) | 28k [M]; company shut down |
| OpenViking | Context DB | FS paradigm, tiered L0–L2 | 38.7k [S]; partly Rust |
| memvid v2 | Single-file memory | `.mv2`, WAL, BM25 + HNSW | 16.6k [M] |
| AgentFS | Agent FS + KV + audit | SQLite (Turso) single file | 3.4k [M] |
| RuVector | Vector/GNN DB "agent memory" | own engine | 4.5k [S] |
| iwe | Markdown knowledge graph, LSP + CLI + MCP | files | 1.7k [S] |
| chainlink | Agent issue tracker | SQLite | 359 [S] |
| PlanDB | Compound task graph + context | SQLite | 105 [S] |
| mentedb, VelesDB, CortexaDB, yantrikdb, brain-db, NodeDB | "Memory DBs for agents" (WAL + HNSW + graph, decay, contradiction detection) | custom engines | 50–200 [S] each; claims only |
| grite | Git-refs event log tracker | git refs + CRDT + sled | 18 [S] |
| bones | CRDT event-log tracker | `.events` log + SQLite projection | 6 [S] |
| braid | Automerge tracker | Automerge doc + sync server | 15 [S] |
| minigraf | Bi-temporal embedded graph DB (Datalog) | single file | 36 [S] |
| mnestic | CozoDB fork for agent memory | RocksDB/SQLite backends (Cozo) | 45 [S] |
| prollytree | Versioned KV, git-backed | prolly tree + git objects | 34 [M] |
| indra_db | Git-like graph memory | content-addressed | 2 [S] |
| task-graph-mcp, bead-rs, pearls, firetrail | Agent task graphs | SQLite / git | < 10 each |

**Observations.**

1. Rust agent-memory and tracker projects are **numerous but young**. Apart from ports and hosted products, nothing
   has meaningful adoption.
2. The **tracker** projects (grite, bones, braid, chainlink, PlanDB) have a task model but a thin knowledge side and no graph
   engine. The **memory-DB** projects (mentedb, VelesDB, minigraf, mnestic) have engines but **no task/readiness semantics
   and no git-like branch/merge**. The exceptions, prollytree and indra_db, are KV or entry stores, not typed graphs.
3. Several projects converge independently on **append-only event log → rebuildable materialised
   projection**: grite, bones, git-bug, Radicle, and mem0's ADD-only extraction. That is strong signal for moirai's commit model.

---

## 6. Benchmarks and evaluations: what retrieval approach wins

### 6.1 The benchmarks

| Benchmark | What it measures | Scale | Notes |
|---|---|---|---|
| **LoCoMo** (Maharana et al., ACL 2024, arXiv 2402.17753) | QA and summarisation over very long multi-session chats | ~300 turns, up to 35 sessions; Zep: 16–26k tokens per conversation | Criticised as fitting in modern context windows. The GPT-4o-mini judge accepted 62.81% of deliberately wrong-but-topical answers [S, Zep/field guide]. Gemini 2.5 Flash with no memory reportedly scores 72.8% [S] |
| **LongMemEval** (Wu et al., ICLR 2025, arXiv 2410.10813) | Extraction, multi-session reasoning, temporal reasoning, **knowledge updates**, abstention | 500 questions; S ≈ 115k tokens | Paper shows a **30% accuracy drop** for commercial assistants over sustained interactions. Recommends session decomposition, fact-augmented keys and time-aware query expansion [D] |
| **BEAM** (ICLR 2026, arXiv 2510.27246) | 10 abilities incl. contradiction resolution, event ordering | 128k → **10M** tokens, 100 conversations, 2,000 questions | Even 1M-context LLMs degrade [D] |
| **AMA-Bench** (arXiv 2602.22769) | Memory over **agent trajectories** (states, actions, tool outputs), not dialogue | — | Existing memory "rel[ies] heavily on lossy similarity-based retrieval". The causality-graph + tool-augmented retrieval agent reaches 57.22% (+11.16 pp) [C] |
| **Context-Bench** (Letta) | Chained file operations, entity tracing | — | Best model ~74%; relevant skills add +14.1% for Claude [C] |
| **MemDelta** (arXiv 2606.29914) | *Controlled* re-evaluation | — | Verbatim RAG ≈ full context for gpt-4o-mini (47.2 vs 49.8, p = 0.34), but results flip by model family. **Changing the embedding model alone moves accuracy by 6.2 pp and flips mem0-vs-RAG**. Agent self-memory 42% vs basic retrieval 47%. mem0 matches cloud RAG on 2 of 6 types at ~50× cost [C] |

### 6.2 Self-reported leaderboard (all [C], not comparable across rows)

| System | LoCoMo | LongMemEval | Setup disclosed? |
|---|---|---|---|
| mem0 (Apr 2026 algorithm) | 92.5 | 94.4 | Token budget yes; judge/model not in the post |
| Zep | 75.14 ± 0.17 (own re-run, 2025); 94.7 cited by mem0's 2026 guide | 71.2 (gpt-4o) | Partly |
| MemOS 2.0 | 88.83 | 89.20 | Vendor harness |
| Mastra OM | — | 94.87 (gpt-5-mini) / 84.23 (gpt-4o) | Yes |
| Letta filesystem agent | 74.0 (gpt-4o-mini) | — | Yes; grep + semantic file search |
| mem0 graph (2025 paper, as cited by Letta) | 68.5 | — | — |
| MemPalace | "100" → retracted; 96.6 R@5 is ChromaDB defaults | — | Retracted [S] |

### 6.3 What actually wins (synthesis)

1. **Retrieval that the agent drives iteratively with familiar tools** (grep, file reads, search then open) is
   competitive with bespoke memory libraries on conversational benchmarks [C, Letta; D, MemDelta]. Models are
   post-trained on filesystem and search tool use.
2. **Hybrid lexical + semantic + structural (entity/graph) fusion** beats vector-only. mem0 abandoned
   graph-only and vector-only in favour of fusion [C]. bones and Basic Memory also fuse FTS5 with vectors and RRF [D].
3. **Temporal and "knowledge update" handling** is where systems differ most (LongMemEval, BEAM). Append-only
   storage with validity intervals (Graphiti) or ADD-only facts (mem0) is the pattern that works [C/D].
4. **Stable, append-only context prefixes** (Mastra OM, Claude Code's 200-line `MEMORY.md`, Letta's `system/`) exploit prompt
   caching. For a coding harness this matters as much as recall accuracy [C/D].
5. **None of these benchmarks tests moirai's core job**: *correct* task state (ready/blocked), rule and decision
   precedence under supersession, referential integrity under deletes, or concurrency. moirai must bring its **own
   eval harness**: correctness under concurrent multi-worktree writes, precision@k for rules/decisions given a
   file/task context, and tokens per context pack.

---

## 7. Comparison matrix

Legend: **RI** = referential integrity. "App" = enforced in application code. "Engine" = enforced by the storage
engine in the same transaction. "—" = not provided or not found. RAM/perf entries carry their tags.

### 7.1 Task-oriented systems

| System | Data model | Storage | Versioning | RI of references | Concurrency | Query interface | RAM / perf | Agent interface | Maturity |
|---|---|---|---|---|---|---|---|---|---|
| **Beads (current, 1.3)** | Wide issue record; 19 typed edge kinds (4 affect ready); labels, comments, events, KV memories | Dolt (embedded default, or `sql-server`); JSONL export only | Dolt commits per write, branches, cell-level diff/merge, `history`, push/pull via `refs/dolt/data` | App: delete refuses/cascades/forces, `[deleted:ID]` text rewrite; **orphans and stale `is_blocked` reported**; `bd doctor` repairs | Embedded: single writer (file lock). Server: multi-writer. Atomic claim, leases, heartbeat, CAS (1.3). **Lost writes under agent load reported** | CLI (`ready/blocked/dep tree/query` mini-language), `--json/--brief`, HTTP API (41 ops), SQL via Dolt | Binary 54 MB zip [M]; CLI ~150–230 ms/call [R]; server ~2 GB RSS idle [R]; 120 GB pathological [R]; ~282 MB disk for 2.7k issues [R] | CLI + hooks + `prime` (preferred), `beads-mcp`, skill, HTTP | 27k stars; very high churn; untested release incident Aug 2026 |
| **Beads classic (≤ 0.49)** | Same core, fewer types | SQLite + JSONL in git + daemon | Via git history of JSONL; content-hash import; 3-way merge engine; tombstones | App | Daemon RPC; debounce; hooks | CLI | "ms" local queries [C] | CLI, MCP | Superseded; frozen in `br` |
| **beads_rust (`br`)** | Beads-classic | FrankenSQLite + JSONL | Explicit JSONL sync modes (flush/import/merge/reconcile) | App | `BEGIN IMMEDIATE`; cooperative admission | CLI `--json/--robot`, schema/capabilities introspection | 27.7 MB binary [C] | CLI, optional MCP; Agent Mail | 1.1k stars; active |
| **Claude Code Tasks / agent teams** | Task (pending/in-progress/completed), blocked-by/blocks | JSON files under `~/.claude/tasks/<id>/` | — | Auto-unblock on complete | File-locked claiming; shared list via env var | Tool calls | n/a | Built-in tools (off by default on newer models) | Built-in |
| **Task Master AI** | Task + subtasks (dotted IDs), deps, priority, testStrategy | `tasks.json` (tags) | git on the file | App | None beyond the file | CLI / MCP (7/15/36 tools) | 5k/10k/21k tokens of tool schemas [C] | MCP-first | 28k stars; slowing (last release Mar 2026) |
| **Backlog.md** | Markdown file per task, deps, milestones, AC | Files in repo | git; cross-branch scans | App | Cross-branch ID and status heuristics | CLI, web, MCP | Branch scans slow on large repos [D] | MCP + CLI | 6.8k stars; active |
| **Shrimp** | Tasks with deps, verify/reflect | JSON files | — | App | — | MCP | — | MCP | Stale since mid-2025 |
| **GitHub Issues (+MCP)** | Issue, sub-issues, blocked-by/blocking (≤ 50) | Hosted | Audit/timeline | Server-side | Server | REST / GraphQL / `gh` / MCP | Network latency | MCP (official), `gh` CLI | GA |
| **Linear (+MCP)** | Issues, projects, relations, initiatives | Hosted | Audit | Server-side | Server | GraphQL / MCP / CLI | Network | Remote MCP (OAuth) | GA |
| **PlanDB** | Compound graph (containment + cross deps), context entries | SQLite | — | App | Atomic claim | CLI / MCP / HTTP, BM25, critical path | — | Rules + skill installer | 105 stars |
| **grite** | Issues as events | git refs WAL + sled view | Event log, CRDT merge | App | Daemon, TTL locks | CLI | ~15 MB RSS CLI, ~5 ms create [C] | CLI / AGENTS.md | Tiny |
| **bones** | Issues as events | `.events` + SQLite projection | CRDT, ITC clocks | App | Merge by union | CLI / TUI `--format json` | — | CLI | Tiny |
| **braid** | Strands in one Automerge doc | Automerge + sync server | CRDT history | App | CRDT | CLI / MCP | — | CLI / MCP | Tiny |
| **git-bug** | Bug entity = op log | git objects in refs | Op DAG + Lamport | App | Merge by op-log union | CLI / TUI / web / GraphQL | — | — | Mature (non-agent) |

### 7.2 Knowledge-oriented systems

| System | Data model | Storage | Versioning | RI of references | Concurrency | Query interface | RAM / perf | Agent interface | Maturity |
|---|---|---|---|---|---|---|---|---|---|
| **MCP memory server** | Entity (name, type, observations[]), relation (from, type, to) | One JSONL file, **whole-file rewrite per mutation** | — | Validates endpoints on create; cascades relation delete; no rename | In-process promise queue only | Substring search, open nodes, read whole graph | O(file) per write [D] | MCP (9 tools) | Reference impl.; 2026.8.31 |
| **Basic Memory** | Markdown entity + observations + typed wikilinks | Files + SQLite/Postgres index (FTS, vectors) | Cloud per-note history and snapshots; local = your git | Forward refs allowed; stable permalinks; unresolved links tolerated | Deadlock-free concurrent indexing (v0.23) [D] | `search_notes`, `build_context` via `memory://` | — | MCP, CLI, skills | 4k stars; active |
| **Letta (server)** | Memory blocks (label, value, limit), archival passages, recall | Postgres (+ vectors) | DB state | — | Server | Agent tools | Server stack | Letta agents / SDK | 25k stars |
| **Letta Code MemFS** | Markdown files (`system/` always loaded) + frontmatter | Git repo (local or hosted) | **Git: every edit a commit; worktrees for subagents; merge** | None (files) | Git merges | Bash / grep / read | — | Coding-agent harness | Feb 2026; active |
| **mem0** | Atomic facts + entities | Vector store (+ LLM on write) | ADD-only history (2026) | — | Server/library | Semantic + BM25 + entity fusion | ~0.9–1.1 s p50, ~7k tokens/query [C] | SDK, MCP (OpenMemory), skills | 66k stars |
| **Zep / Graphiti** | Bi-temporal KG, episodes | Neo4j / FalkorDB / Neptune | Temporal validity, invalidation | Graph DB | DB server | Hybrid BM25 + vector + traversal | Server DB | MCP, REST, SDK | 31k stars |
| **Cognee** | Doc/code → KG + vectors, ontologies | Postgres or Kuzu / Lance / SQLite / Neo4j | — | Graph DB | Pipelines | search types | — | MCP, SDK | 31k stars |
| **LangMem** | Semantic / episodic / procedural items | LangGraph BaseStore | — | — | Store | Tools + background manager | — | Tools | Stalled (Oct 2025) |
| **A-MEM** | Zettelkasten notes + links, evolution | Vector DB (research) | — (rewrites notes) | — | — | Similarity + links | — | Library | Research |
| **MemOS** | MemCubes (text, traces, personas) | Neo4j + Qdrant; local SQLite FTS5 + vec | — | — | Server | Hybrid | "35% token savings" [C] | Plugins / MCP | 11.6k stars |
| **Claude Code memory** | CLAUDE.md + rules + `MEMORY.md` + topic files | Files | Your git (project) / none (auto memory) | None | None (files) | Loaded by harness / Read tool | 200 lines / 25 KB preload | Built-in | Built-in |
| **Serena memories** | Named Markdown files | `.serena/memories/` | Your git | None | None | list / read | — | MCP | 30k stars |
| **OpenViking** | FS tree with L0/L1/L2 tiers | Own store (Rust + Py) | — | — | Server | `ls/tree/find/search` | Token cut claims [C] | SDK / HTTP | 38.7k stars |

---

## 8. Lessons

### 8.1 What to steal

1. **Beads' edge taxonomy split into scheduling vs informational edges** (`blocks, parent-child, waits-for,
   conditional-blocks` vs `related, discovered-from, supersedes, duplicates, caused-by, validates, tracks`), and
   **`ready` as a first-class query** with explainability (`--explain`). Also `discovered-from`, `defer_until`, gates, and
   `--claim` as an atomic find-and-take.
2. **Hash-based, coordination-free IDs with adaptive-length display** (Beads birthday-bound scheme), plus prefix
   resolution. Keep a longer internal ID so short aliases never become identity.
3. **Write-time cycle rejection** on blocking edges.
4. **Delete semantics with three explicit modes** (refuse / cascade / orphan-with-marker) **and rewriting of textual
   mentions** (`[deleted:ID]`). moirai goes further by making these engine invariants (§8.3).
5. **Dolt's model:** cell-level (node-field-level) diff and merge, per-entity history (`bd history <id>`), and **publishing
   the DB's history on the project's existing git remote under a private ref namespace** (`refs/dolt/data`). This
   gives zero new infrastructure for sync.
6. **Event-log + deterministic-merge designs** (git-bug, Radicle COBs, grite, bones): an append-only log of typed
   operations, causal ordering (Lamport/ITC), materialised views rebuilt from the log. Merge = union + replay.
7. **Graphiti's bi-temporal validity** and "invalidate, don't delete" for knowledge. **mem0's ADD-only** facts.
   These let a superseded rule stay queryable as "was valid until commit X".
8. **Basic Memory's forward references and stable permalinks**: a typed, *unresolved* reference state.
9. **Budgeted, tiered context:** Claude Code's 200-line / 25 KB `MEMORY.md`, Letta's `system/` plus file-tree signposts,
   OpenViking's L0/L1/L2 abstracts, mnestic's budgeted traversal "to fill a fixed context window". moirai's `prime`
   or context-pack query should take a **token budget** and fill it with the cheapest relevant graph neighbourhood.
10. **Interface discipline:** CLI-first with `--json`, compact defaults (`--brief`), and a small MCP surface (Task Master's
    published tool-tier costs). Claude Code dropping always-on task tools shows how scarce context is.
11. **Multi-agent coordination primitives:** atomic claim, leases + heartbeat + reclaim, compare-and-set guards
    with a distinct exit code (Beads 1.3), advisory file reservations (Agent Mail, task-graph-mcp), and file-locked claims
    (Claude Code agent teams).
12. **Agent-driven compaction:** the tool identifies candidates and the *agent* writes the summary on its own budget. Archive before
    discarding.
13. **PlanDB's compound graph:** containment hierarchy **orthogonal** to cross-cutting dependencies, with
    `split / insert / pivot` restructuring and context surfacing on claim. **bones' duplicate detection on create.**

### 8.2 What failed, and why

| Failure | Where | Root cause | Evidence |
|---|---|---|---|
| Silent data loss and drift between two stores | Beads classic (SQLite ⇄ JSONL) | **Dual source of truth** plus heuristic import/export and debounce | #3931, Discussion #380, #4135 [R] |
| Daemon complexity, split-brain, repo-ID mismatch | Beads daemon era; still present in 1.0.x | A background process owning state that was discovered by path heuristics | frr.dev, #4135 [R] |
| Server RAM/CPU leaks, ports, lost file-watch notifications | Beads + `dolt sql-server` | Adopting a general-purpose SQL server for a single-user local tool | #4282, #2050, #3760 [R] |
| Process-per-call and connection-per-call overhead | Beads 1.0–1.2 | CLI forks plus DB connect and schema introspection on every call; agents call hundreds of times | #3760, #4102 [R]; fixed by `bd serve` in 1.3 |
| Lost acknowledged writes under agent load | Beads embedded Dolt | Unclear. Single-writer lock plus multiple processes in nested worktrees; success reported before durability confirmed | #4767 [R] |
| Worktree phantom DBs | Beads 1.2 | Several code paths resolve the DB location differently; auto-creating an empty store on a miss | #6551, #6552 [R] |
| Orphan rows and stale caches | Beads | Integrity and derived state maintained by application code with bypass paths (raw SQL in `doctor`) | #4673, #6487, #6608 [R] |
| Memory blow-up in a graph algorithm | Beads `doctor` | Graph algorithm written as a recursive SQL CTE with string paths | #4475 [R] |
| Upgrade breakage | Beads 1.2.1; v0.50 for extensions | Untested release auto-migrating schema; big-bang backend swap | v1.2.2 notes [D]; vscode-beads#65 [S] |
| ID collisions across branches | Beads < 0.20.1; Task Master; Backlog.md | Sequential IDs | [D] |
| Tool-schema context bloat | MCP-heavy trackers | Every tool definition is paid for every turn | Beads docs, Task Master tiers [C]; Claude Code tools-reference [D] |
| Benchmark theatre | MemPalace; mem0/Zep dispute | Tuning on the test set, top_k = everything, weak LLM judges | [S], [C] |
| Upstream abandonment | Kuzu (archived 2025-10-10); Vibe Kanban (company closed 2026-04-10); LangMem stall; Shrimp stall | Dependency on single-vendor OSS | [D/S/M] |
| Vector-only memory in agentic settings | AMA-Bench findings | Similarity retrieval loses causal and objective structure | [C] |

### 8.3 The gap moirai fills

No surveyed system provides all of the following together, and each item is independently justified by a failure above:

1. **A native, embedded graph engine** (no SQL emulation, no external server), with **reverse-edge indexes maintained in
   the same atomic commit** as forward edges. "Node 40 deleted ⇒ every referrer knows immediately" becomes an engine
   invariant, not a `doctor` job. That covers structural edges *and* typed mentions in text fields.
2. **A git-like commit DAG at node/field granularity:** history, branches, diff, merge with deterministic,
   type-aware merge rules (monotone status lattice, LWW for scalar fields, union for sets, tombstone-wins or
   explicit conflict for deletes vs concurrent new references).
3. **One schema for tasks *and* knowledge** (rule, note, decision, finding) with typed fields (`done: bool`, etc.),
   bi-temporal validity or supersession for knowledge, and readiness for tasks. Beads' `remember` KV and PlanDB's "context" are
   the closest attempts, and both are untyped.
4. **Readiness and blocker queries as incrementally maintained indexes**, not recomputed recursive SQL and not a
   denormalised flag with bypass paths.
5. **A small, fast, portable footprint:** a single static binary, low-millisecond cold CLI, no required daemon, Windows-first
   file locking and paths, bounded RAM. Every incumbent is either a large Go binary on Dolt, a Python stack, or a
   server DB.
6. **Worktree- and multi-process-correct by construction:** one deterministic store location per repo, shared across
   worktrees (the way Claude Code auto memory keys on the git repository), durable-on-ack writes, and loud failures.
7. **An agent-economical interface:** CLI + skill + hooks (SessionStart / SubagentStart context packs within a token
   budget), a thin MCP, and a change feed for "maximally synchronous" consumers.

---

## 9. Implications for moirai

Format: *Finding → Evidence → Implication (confidence)*.

1. **Coordination-free identity.** Sequential IDs collided across agents and branches (Beads < 0.20.1, Task Master, Backlog.md
   needs cross-branch scans) → use random or hashed internal IDs (e.g. 128-bit) with adaptive-length short display
   aliases and prefix resolution; never reuse an ID, even after delete. (High)
2. **One source of truth.** Beads' SQLite ⇄ JSONL dual store caused the most damaging bugs → one canonical store;
   every export (JSONL, Markdown, CLAUDE.md snippets) is a derived, one-way projection. (High)
3. **Deletes are data.** "Imports cannot infer deletions"; tombstones existed for merges; `[deleted:ID]` rewriting →
   delete = a tombstone node version in a commit. Referrers resolve to "deleted (at commit X)" at read time, and merges
   see the delete explicitly. (High)
4. **Integrity in the engine, on one mutation path.** Orphans and a stale `is_blocked` came from app-level
   maintenance with bypasses (#4673, #6487, #6608) → forward and reverse adjacency plus derived readiness updated atomically
   inside the storage engine; repair tools may only *verify*. Property-test "reverse index == inverse(forward)". (High)
5. **Native graph algorithms with budgets.** A recursive SQL CTE hit 120 GB (#4475) → Tarjan/DFS in the engine with
   bounded memory; reject cycles at write time on blocking edge kinds; every traversal takes a node, edge or token budget.
   (High)
6. **Per-call cost dominates agent workloads.** 41 conn/s and 5 cores idle (#3760); 5–10 s per command (#4102); Beads re-added
   a server in 1.3 → design for a **cold-start CLI in low milliseconds** (mmap'd store, no handshake). An optional
   long-lived process (for MCP) must be an accelerator, never required state. (High)
7. **No mandatory background server.** Idle Dolt servers at ~2 GB RSS and 38% CPU (#4282), lost file-watch (#2050) →
   embedded by default. Any helper process auto-exits when idle and has a RAM ceiling. (High)
8. **Multi-process correctness under real agent load is the acceptance test.** #4767 lost 7 of 8 closes; #6551/#6552
   phantom stores → deterministic store discovery keyed on the git *common dir* (all worktrees resolve to one store),
   never auto-create on a miss inside a worktree, fsync before reporting success, and read-your-writes across processes.
   The CI stress test should simulate an orchestrator plus N agent processes in N worktrees on Windows. (High)
9. **Decide the git-branch ↔ moirai-branch relationship explicitly.** Dolt branches are independent of git branches;
   Letta MemFS uses git worktrees for memory subagents; the 01 report documents rulings lost across branches →
   an owner decision (see open questions). A likely default: one shared "main" graph across worktrees, with optional per-task
   branches merged by the orchestrator. (Medium)
10. **Sync via the existing git remote.** `refs/dolt/data` (Beads), `refs/grite/wal`, and git-bug refs show that a private
    ref namespace on the project remote gives cross-machine sync with no new infrastructure → reserve a
    `refs/moirai/*` export and import path, even if v1 is single-machine. (Medium)
11. **Token economy decides adoption.** CLI+hooks ~1–2k tokens vs MCP 10–50k [C]; Task Master 5k/10k/21k tiers [C];
    Claude Code omits Task tools on newer models [D] → CLI + skill as the primary interface; MCP with ≤ ~7 tools; compact
    default output; explicit `--budget` on context queries. (High)
12. **Retrieval: start lexical and structural, keep embeddings optional.** Filesystem + grep scored 74% LoCoMo [C];
    embedding choice flips results [C]; similarity-only is lossy for agent trajectories [C]; winners fuse
    BM25 + semantic + entity [C] → v1: exact lookup, typed filters, FTS/BM25, and graph-neighbourhood expansion. Vectors come
    later as a pluggable index (RAM-guarded). (Medium-High)
13. **Append-only knowledge with supersession.** mem0 moved to ADD-only; Graphiti invalidates; the 01 report shows summaries
    outliving their retractions → knowledge nodes are never silently overwritten. `supersedes / refutes /
    invalidated_at` edges plus a query default of "currently valid only". (High)
14. **Budgeted, tiered context packs.** Claude Code preloads ≤ 200 lines / 25 KB; Letta `system/`; OpenViking L0/L1/L2 →
    each node carries a one-line abstract (L0). `moirai prime --budget N` returns critical rules, ready tasks and relevant
    decisions within N tokens, deterministically ordered so prompt caches hit. (Medium-High)
15. **Serve subagents explicitly.** Main-session auto memory is not loaded into subagents [D]; agent teams use file-locked
    claims and JSON mailboxes [D] → provide SubagentStart / SessionStart hook recipes and a per-role context pack. Claims need
    lease, heartbeat and CAS semantics (Beads 1.3 converged on these). (Medium)
16. **Change feed for "maximally synchronous".** Beads 1.3 added a durable events journal. #2050 complained that
    notification disappeared with the server → expose a monotonic commit sequence plus a cheap "watch" (file-change marker or
    named pipe on Windows) so other processes see deletes and updates immediately. (Medium)
17. **Release and migration discipline is part of the product.** v1.2.1 auto-migrated schemas and stranded users; the v0.50
    backend swap broke the ecosystem → a versioned on-disk format spec, explicit-consent migrations, backward-compatible readers,
    fuzzing and crash-recovery tests. Avoid a sprawling 225k-LOC surface. (High)
18. **Footprint targets are achievable and differentiating.** bd ships a 54 MB zip [M]; br is ~27 MB [C]; grite claims
    ~15 MB RSS [C] → set explicit budgets (e.g. binary < 10–15 MB, CLI op RSS < 20 MB, O(changed) writes) and track them in CI.
    (Medium; targets are suggestions)
19. **Build your own evaluation.** No public benchmark measures task-state correctness, precedence of superseded rules, or
    integrity under concurrency; vendor scores are unreliable → a moirai eval suite from the owner's real workflow traces
    (the 01 report's role pipeline): correctness, precision@k of context packs, tokens, latency. (High)
20. **Dependency risk argues for a small, owned core.** Kuzu archived; Vibe Kanban's company closed; Dolt regressions forced a
    version pin in Beads → building the graph store from scratch in Rust is defensible *if* the core stays small. Borrow
    ideas (prolly trees, op logs) rather than dependencies. (Medium)

---

## 10. Open questions only the owner can answer

1. **Git coupling.** Should moirai data travel *with* the git repo (a private ref namespace on the remote, per-branch state)
   or live *beside* it (one per-machine store shared by all worktrees of the repo)? Should moirai branches map 1:1 to git
   branches or worktrees, or be independent (as in Dolt)?
2. **Multi-machine.** Is cross-machine sync (laptop ⇄ desktop, cloud agents) needed in v1, or is single-machine enough?
3. **Concurrency envelope.** What is the maximum number of concurrent writer processes (orchestrator + subagents + Workflow
   scripts across worktrees), and what write/read latency is acceptable per call?
4. **Delete semantics.** When node 40 is deleted and others reference it, should the default be *refuse*, *cascade*,
   or *tombstone and mark referrers*? Should mentions inside free text (e.g. "see #40") count as references?
5. **Merge policy.** On a branch merge where both sides changed the same field, or one side deleted a node the other side linked
   to: resolve automatically by type-specific rules, or surface a conflict for the orchestrator or human?
6. **Knowledge semantics.** Are rules, decisions and findings plain notes, or typed facts with validity intervals and
   supersession, answering "what was the rule at commit X"?
7. **Semantic search.** Is exact + full-text + graph search enough for v1, or are embeddings required? If required,
   is a local embedding model (RAM cost) acceptable?
8. **Interop.** Import from or export to Beads JSONL, GitHub Issues, Claude Code Tasks (`~/.claude/tasks`), or
   CLAUDE.md / `.claude/rules` projections?
9. **Retention.** Keep full history forever, or allow compaction and summarisation of old closed tasks (agent-written
   summaries, archive before discard)?
10. **Context budget.** How many tokens may a SessionStart or SubagentStart context pack consume by default?
11. **Relationship to Claude Code built-ins.** Replace native Tasks and auto memory in the owner's workflow, or complement them?
12. **Human surface.** Is a TUI or web viewer needed, or are agents and the CLI enough?
13. **Distribution and licensing.** Private tool, or open source? This affects whether Beads-compatible import is worth doing.

---

## 11. Sources

All accessed 2026-09-25 unless noted.

**Beads: code, docs, releases**
- https://github.com/gastownhall/beads (README; repository; stars and push via https://api.github.com/repos/gastownhall/beads)
- https://raw.githubusercontent.com/gastownhall/beads/main/internal/types/types.go
- https://raw.githubusercontent.com/gastownhall/beads/main/docs/core-concepts/dependencies.md
- https://raw.githubusercontent.com/gastownhall/beads/main/docs/core-concepts/issues.md
- https://raw.githubusercontent.com/gastownhall/beads/main/docs/core-concepts/hash-ids.md
- https://raw.githubusercontent.com/gastownhall/beads/main/docs/core-concepts/adaptive-ids.md
- https://raw.githubusercontent.com/gastownhall/beads/main/docs/core-concepts/sync-concepts.md
- https://raw.githubusercontent.com/gastownhall/beads/main/docs/architecture/dolt.md
- https://raw.githubusercontent.com/gastownhall/beads/main/docs/CLI_REFERENCE.md
- https://raw.githubusercontent.com/gastownhall/beads/main/docs/multi-agent/coordination.md
- https://raw.githubusercontent.com/gastownhall/beads/main/docs/community-tools.md
- https://github.com/gastownhall/beads/tree/main/integrations/beads-mcp
- https://beads.gascity.com/cli-reference/ready ; https://beads.gascity.com/cli-reference/delete
- https://github.com/gastownhall/beads/releases ; https://github.com/gastownhall/beads/releases/tag/v1.3.0 ; https://github.com/gastownhall/beads/releases/tag/v1.2.2
- https://api.github.com/repos/gastownhall/beads/releases/latest (asset sizes)
- Legacy: https://raw.githubusercontent.com/steveyegge/beads/v0.49.6/docs/ARCHITECTURE.md ; https://raw.githubusercontent.com/steveyegge/beads/v0.49.6/internal/storage/sqlite/schema.go
- https://pkg.go.dev/github.com/steveyegge/beads

**Beads: issues and discussions (reported measurements)**
- https://github.com/gastownhall/beads/issues/4135 (six embedded-mode failure modes)
- https://github.com/gastownhall/beads/issues/4767 (7/8 closes lost)
- https://github.com/gastownhall/beads/issues/4475 (120.6 GB recursive CTE)
- https://github.com/gastownhall/beads/issues/4282 (orphaned dolt servers, 2 GB RSS)
- https://github.com/gastownhall/beads/issues/4102 (5–10 s remote mode)
- https://github.com/gastownhall/beads/issues/3760 (bd serve proposal, 41 conn/s)
- https://github.com/gastownhall/beads/issues/2050 (server mode regression for standalone users)
- https://github.com/gastownhall/beads/issues/6551 ; https://github.com/gastownhall/beads/issues/6552 (phantom DB in worktrees)
- https://github.com/gastownhall/beads/issues/4673 ; https://github.com/gastownhall/beads/issues/6487 (orphaned rows)
- https://github.com/gastownhall/beads/issues/6608 (stale is_blocked)
- https://github.com/gastownhall/beads/issues/3931 (JSONL rewrite drops issues) — title-level evidence
- https://github.com/gastownhall/beads/discussions/380 (DB out of sync with JSONL)
- https://github.com/gastownhall/beads/issues/6626 (prime/recall docs gap)
- https://github.com/gastownhall/beads/issues/6064 (old server + new client corrupted journal) — title-level evidence

**Beads: context, history, derived projects**
- https://www.dolthub.com/blog/2026-04-02-restoring-beads-classic/
- https://github.com/jdillon/vscode-beads/issues/65 (v0.50–0.51 changes; 2026-02-16)
- https://ascii.co.uk/news/article/news-20251215-9239b867/beads-v0201-adds-hash-based-ids-for-multi-agent-workflows [S]
- https://steve-yegge.medium.com/introducing-beads-a-coding-agent-memory-system-637d7d92514a ; https://steve-yegge.medium.com/beads-blows-up-a0a61bb889b4 ; https://steve-yegge.medium.com/beads-best-practices-2db636b9760c (Medium returned 403; claims via search snippets [S])
- https://www.frr.dev/posts/beads-is-dead/ ; https://tiby.fr/articles/i-built-a-distributed-issue-tracker-i-didnt-need
- https://github.com/Dicklesworthstone/beads_rust ; https://github.com/Dicklesworthstone/beads_viewer_rust ; https://github.com/Dicklesworthstone/mcp_agent_mail
- https://github.com/bobisme/bones ; https://github.com/cscheid/braid ; https://github.com/neul-labs/grite ; https://github.com/dollspace-gay/chainlink ; https://github.com/Agent-Field/plandb ; https://lib.rs/crates/task-graph-mcp ; https://github.com/jedarden/bead-rs

**Knowledge-memory systems**
- https://github.com/modelcontextprotocol/servers/tree/main/src/memory ; https://raw.githubusercontent.com/modelcontextprotocol/servers/main/src/memory/index.ts
- https://github.com/basicmachines-co/basic-memory ; https://docs.basicmemory.com/raw/concepts/knowledge-format.md ; https://docs.basicmemory.com/llms.txt
- https://docs.letta.com/guides/agents/memory ; https://www.letta.com/blog/memory-blocks/ ; https://docs.letta.com/guides/agents/architectures/sleeptime/ ; https://www.letta.com/blog/context-repositories/ ; https://docs.letta.com/concepts/memfs ; https://github.com/letta-ai/letta-code
- https://github.com/mem0ai/mem0 ; https://mem0.ai/blog/mem0-the-token-efficient-memory-algorithm ; https://mem0.ai/blog/ai-memory-benchmarks-in-2026
- https://github.com/getzep/graphiti ; https://arxiv.org/abs/2501.13956 ; https://blog.getzep.com/lies-damn-lies-statistics-is-mem0-really-sota-in-agent-memory/
- https://github.com/topoteretes/cognee
- https://github.com/langchain-ai/langmem ; https://pypi.org/pypi/langmem/json
- https://arxiv.org/abs/2502.12110 ; https://github.com/agiresearch/A-mem
- https://github.com/MemTensor/MemOS
- https://oraios.github.io/serena/02-usage/045_memories.html ; https://github.com/oraios/serena ; https://github.com/oraios/serena/issues/994
- https://github.com/volcengine/OpenViking ; https://mastra.ai/research/observational-memory ; https://github.com/memvid/memvid ; https://github.com/tursodatabase/agentfs ; https://turso.tech/blog/agentfs
- https://github.com/zhangfengcdt/prollytree ; https://github.com/moonstripe/indra_db ; https://github.com/project-minigraf/minigraf ; https://github.com/shuruheel/mnestic
- https://ossinsight.io/blog/agent-memory-race-2026 [S] ; https://vectorize.io/articles/mempalace-benchmarks [S]
- Kuzu: https://www.theregister.com/software/2025/10/14/kuzudb-graph-database-abandoned-community-mulls-options/1142229 ; https://gdotv.com/blog/kuzu-legacy-embedded-graph-database-landscape/

**Claude Code / Anthropic**
- https://code.claude.com/docs/en/memory ; https://code.claude.com/docs/en/sub-agents ; https://code.claude.com/docs/en/env-vars.md ; https://code.claude.com/docs/en/tools-reference.md ; https://code.claude.com/docs/en/interactive-mode.md ; https://code.claude.com/docs/en/agent-teams.md
- https://platform.claude.com/docs/en/agents-and-tools/tool-use/memory-tool

**Task trackers**
- https://github.com/eyaltoledano/claude-task-master ; https://registry.npmjs.org/task-master-ai
- https://github.com/MrLesk/Backlog.md ; https://github.com/MrLesk/Backlog.md/blob/main/ADVANCED-CONFIG.md ; https://github.com/MrLesk/Backlog.md/issues/1024
- https://github.com/cjo4m06/mcp-shrimp-task-manager
- https://github.blog/changelog/2025-08-21-dependencies-on-issues/ ; https://github.blog/changelog/2026-06-10-manage-sub-issues-types-and-dependencies-from-github-cli/ ; https://github.com/github/github-mcp-server
- https://linear.app/docs/mcp ; https://linear.app/changelog/2026-02-05-linear-mcp-for-product-management
- https://github.com/automazeio/ccpm ; https://github.com/BloopAI/vibe-kanban ; https://www.vibekanban.com/blog/shutdown
- https://github.com/git-bug/git-bug ; https://dev.to/eme_gug_0821b41b948be6516/git-as-a-database-what-git-bug-teaches-us-about-storing-data-in-refs-302n [S]
- https://docs.rs/radicle/latest/radicle/cob/index.html ; https://lwn.net/Articles/966869/
- https://github.com/GothenburgBitFactory/taskchampion

**Benchmarks and evaluations**
- LoCoMo: https://arxiv.org/abs/2402.17753 (not re-read in this session; cited for provenance)
- LongMemEval: https://arxiv.org/abs/2410.10813
- BEAM: https://arxiv.org/abs/2510.27246 ; https://github.com/mohammadtavakoli78/BEAM
- AMA-Bench: https://arxiv.org/abs/2602.22769
- MemDelta: https://arxiv.org/abs/2606.29914
- Letta: https://www.letta.com/blog/benchmarking-ai-agent-memory/ (2025-08-12) ; https://www.letta.com/blog/context-bench/

**Cross-reference inside this project**
- [01-boyko-workflow-roles.md](01-boyko-workflow-roles.md) (workflow-role lens; §0 items 3 and 5 cited for union-merge and retraction failures)
