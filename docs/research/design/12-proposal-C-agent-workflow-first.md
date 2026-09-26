# 12 — Proposal C: "Agent-workflow first"

*Architecture proposal for moirai. Date: 2026-09-25. Status: design only — no code, no files other than this one.*
*Evidence base: reports [01]–[08] under `docs/research/`, cited as `[NN §section]`. Numbers marked **(est.)** are my estimates with the inputs shown; everything else is a measured or documented figure from the cited report.*

---

## 1. Thesis and the angle optimised for

1. moirai is first a **typed, shared, live record of the owner's multi-agent workflow** (tasks, lanes, runs, plans-as-sections, decisions, findings with a refutation lifecycle, verdicts, measurements with environment, rules with `applies_to` and authority) and only second a general graph database; every storage decision is chosen as the *simplest engine* that makes the workflow record fast, small and crash-safe on Windows.
2. The thing agents actually consume is the **context pack**: a budgeted, deterministic, drop-accounted selection of nodes that replaces the hand-written `HDR` blocks, `.slice()` truncation and the Russian resume block in `MEMORY.md` [01 §7 L1, L13; 02 §10.2].
3. **Coordination state lives on one live trunk** shared by every worktree (found via the git common dir); knowledge is scoped by provenance (lane, git SHA) and promoted at merge time; explicit moirai branches exist only for what-if planning. "Versioned like git" is delivered as an immutable commit DAG with full history, diff, blame, as-of and undo — not as a branch per lane [02 §7; 04 §9.3; 08 §7.2].
4. "Maximally synchronous" is an **engine invariant** (reverse adjacency updated in the same commit, tombstone view, `suspect` propagation along `derived_from`/`cites`, change log with affected-node lists) plus **bounded staleness at the agent** (every write carries preconditions; every hook turn pulls a relevance-filtered delta) [06 §8; 07 §2.3; 08 §6].
5. Storage: an **append-only op log** (history, ~0.3–0.6 KB per commit) materialised into **immutable, read-only-mapped columnar snapshots** plus a small tail overlay; one writer at a time via a lock byte on a separate `LOCK` file; one flush per durable commit; an *opportunistic leader* inside the long-lived MCP server for group commit and push, never required for correctness [05 §16 Option A; 08 §9 Architecture C].

The angle I optimised for: **what the owner's harness needs day to day** (reports 01/02/07) — token economy, typed hand-offs, per-role write policy, review-loop termination as a query, leases for parallel lanes, retraction propagation, session-start briefs, and an adoption path from today's `MEMORY.md`/registers — subject to the hard requirements (max performance, minimal RAM, Windows 11, zero idle CPU).

---

## 2. Positions on the known forks T1–T13

| Fork | Position | Evidence | What would change my mind |
|---|---|---|---|
| **T1 Materialised state** | **Log + immutable mmap'd columnar snapshots + tail overlay** (05 Option A). No CoW B+tree in v1. Snapshots are zerocopy SoA columns, forward/reverse CSR, roaring bitmaps; the tail (≤ 4 MiB / 4,096 ops) is replayed into an in-memory overlay at open. | Open+map 0.22 ms, soft fault ~1 µs/page, flush ~1.8–2 ms [05 §2]; CoW B+tree is "hardest to build from scratch", "95 % is testing" [05 §16 B; 04 §3.14]; the owner's scale is 1e4–1e5 nodes, 0.3–0.5 M over 3 years [01 §8.3; 02 §12.6]; mapped views must be immutable and never truncated on Windows [05 §6.1]. | A measured overlay-read cost > 5 ms or checkpoint > 300 ms at the owner's real 1e5 with random-update bursts; or a hard requirement for uniform O(depth) access to *any* historical full-graph version (then 04 Architecture B/CoW roots). |
| **T2 Process model** | **Embedded multi-process is the contract; the MCP server is an opportunistic leader** (08 C), staged: M1 embedded only, M3 leader (group commit, warm cache, broadcast). No auto-started daemon. Zero timers anywhere; `moirai quiet on` suppresses checkpoints. | Spawn 20–74 ms vs pipe 11–60 µs [05 §2.1; 07 §5.2; 08 §2]; Beads removed its daemon (~24k LOC) then re-proposed a server [03 §2.1; 08 §5]; Windows job-object kill of spawned daemons and Claude Code's own pipe bug [08 W9, W12]; the stdio MCP server already lives as long as the session and serves all subagents [08 §5.2]. | If the redb-style wholesale cache invalidation makes follower reads > 5 ms at 1e5 under 16 writers (bring the leader forward to M2); if the leader pipe is unreliable on the owner's box (stay embedded, accept 2 ms/commit). |
| **T3 Branch model** | **One live trunk for everything coordination-shaped** (tasks, lanes, runs, leases, blockers, questions, rulings, rules); **knowledge is scoped, not branched**: nodes written from a lane carry `scope=lane:<id>` + `git_sha`, are `proposed` until `moirai reconcile` at merge time. **Explicit moirai branches** (`exp/<name>`) only for what-if re-plans, M5. | 12 rulings lost across branches; union merge resurrected OPEN ×10 [01 §7 L5–L6]; 44 worktrees, 107 branches, detached HEADs, harness-created `wf_*` worktrees [08 §2]; "no shared live view of who owns what" [01 §7 L15]; Beads and Taskmaster refused branch-following [08 §7.2]. | If the owner wants a critic's lane findings *invisible* to other lanes even on request (scope filters already hide them by default); if what-if re-plans become weekly (promote branches to M3). |
| **T4 IDs** | **Both**: internal dense `u32` **is** the display number `#N` (store-global, allocated under the writer lock, never reused), plus a 128-bit UUIDv7 `uid` column from day one for future cross-store import. Only `#N` is ever printed. | ~2 tokens vs ~24 for UUID; Haiku 5–7 vs 29–68 errors [06 §9.1]; collisions only across clones [04 §0.5; 06 §9.2]; one store per repo in the common dir makes the counter safe [08 §7.3]. | Cloud/other-machine agents writing to the same graph in v1 → `uid` becomes the primary key and `#N` a per-store alias with an alias map. |
| **T5 Deletion** | **Hard delete in current state + tombstone in history**, resolved through a tombstone view on every read; per-edge-kind policies (structural: restrict / cascade / drop-and-notify; historical: keep + tombstone + `suspect` on the source). `rm` is rare and gated (`--yes`, impact report); the everyday verbs are `supersede`, `retract`, `resolve`. Cross-branch: delete on trunk is immediate for all; on `exp/*` merge it is a conflict/violation record. | "Mark RESOLVED rather than delete", "strike, never delete" [01 §3; 02 §12.3]; soft delete leaks [06 §8.1]; Beads orphans/`[deleted:ID]` [03 §2.10]; Datomic VAET, Gel `on target delete` [06 §6]. | If the owner wants a *visible* tombstone node in the current state (e.g. to attach a reason later) — then a `deleted` status on a kept row instead of a removed row; cost is one flag check in every filter. |
| **T6 Bodies** | **Tiered**: `title` (≤ 120 B) and `abstract` (≤ 280 B, mandatory for knowledge kinds) always in moirai; `body` in moirai when ≤ 16 KiB (zstd + per-store dictionary) — covers findings, rules, decisions, verdicts, measurements, plan sections, most results; larger reports/patches/manifests are `artifact` nodes (path + sha256 + bytes). | Result payload p50 8.8 KB, p90 38 KB, max 153 KB; storing all bodies = 1–3 GB/year [02 §12.6]; scratchpads are session-scoped and reach 7.8 GB [02 §8.3]; packs must inline rule/finding text for roles without Bash [07 §5.4]. | Disk becomes a non-issue and the owner wants FTS over every report → raise the cap; or the owner wants the repo to stay the only home of prose → pointer-only + rendered Markdown views. |
| **T7 Merge semantics** | **Typed per-field 3-way with conflicts as data** (jj/Dolt): status on a monotone lattice (reopen only as an explicit event), add-wins sets, diff3 text, counters and derived values recomputed; post-merge O(V+E) validation (Kahn over `blocks ∪ child→parent`, forest, dangling structural edges); the merge commit always lands, conflicted nodes are excluded from `ready`. Used only by `exp/*` branches (M5) and by nothing in v1's daily path. | Identical +3 edits merged to 188 not 191 [01 §7 L6]; jj conflicts as values, Dolt violation tables [04 §3.8, §3.2]; no CRDT prevents DAG cycles [04 §3.11]. | If the owner prefers `--strict` merges that refuse to advance on any conflict (offered as a flag; default stays non-blocking). |
| **T8 Durability** | **Durability classes with one flush per durable commit**; group commit only in the leader and only when traffic exists (no timers). `durable`: claims, completes, deletes, rulings, decisions, verdicts, findings, `apply` batches. `lazy`: heartbeats, cursors, run-progress pings — appended, flushed by the next durable commit or process exit. | 1.73–1.97 ms per flush, 3.05 ms for 64 pages + 1 flush, write-through not trusted [05 §2.2]; ≤ 16 concurrent agents, tens of writes/s peak [02 §12.6; 08 §1]. | p99 flush > 10 ms under the owner's load (group commit moves to M2); the owner accepting a 100 ms loss window for *all* writes (then `lazy` becomes the default). |
| **T9 Agent surface** | **CLI + skill + hooks first; thin MCP (9 tools) second**, one core. CLI for hooks, bootstrap, Bash roles, scripts; MCP for architect/critic/researcher (no Bash), per-role allowlists and identity stamping. Context packs with token budgets and drop footers. Hooks: SessionStart, SubagentStart, PostToolUse(Agent), SubagentStop, PreToolUse(mcp__moirai__*), UserPromptSubmit; PostToolBatch delta optional (M4). Leases with fencing tokens; idempotency keys on every write. | 3 of 9 roles have no Bash [07 §5.4]; SessionStart cannot call MCP [07 §4.1]; PowerShell 5.1 strips quotes [07 §5.2]; tool search makes an 11-tool surface ~1.2–1.5k tokens [07 §5.1]; Workflow resume re-runs completed agents [07 §4.1]. | If Claude Code ships channels GA and pushes MCP notifications into the model (then `watch` becomes primary push); if non-Claude agents appear (HTTP MCP mode earlier). |
| **T10 From-scratch boundary** | Hand-written: on-disk format, op log, commit DAG, snapshots, CSR/overlay, cycle detection (Pearce–Kelly), derived state, merge engine, pack/brief algorithms, lock protocol, pipe protocol. Allowed crates: `zerocopy`, `blake3`, `xxhash-rust`, `zstd`, `roaring`, `fst` (M6), `memmap2`, `windows-sys`, `bumpalo`, `smallvec`, `serde`/`serde_json` (I/O only), `rmcp` (only in `moirai mcp`). Forbidden in the product: any storage engine (redb/LMDB/SQLite/sled) — allowed as test oracles. | zerocopy safe views cost no validation pass; rkyv validated ≈ deserialize [05 §9.1]; BLAKE3/xxh3 by role [05 §11]; rmcp is the Tier-1 SDK, keep tokio out of the core [07 §2.7]. | If "from scratch" means zero non-std crates: hand-write xxh3/varint LZ-style compression (+2–3 weeks) and use SHA-256 via the OS CNG through `windows-sys`. |
| **T11 Search** | v1: exact id, typed GitHub-style filters, graph expansion (`blockers`, `tree`, `derived`, `about`) with `--depth` and budgets; text: brute-force over mapped bodies (memchr) up to ~5e4 nodes; FST + postings segment above that (M6). Embeddings out of scope; optional external plugin later. | grep-agent 74.0 vs mem0-graph 68.5 on LoCoMo; embedding model alone flips conclusions [03 §6]; LLMs detect stale memory at 55.2 % [06 §11]; the hot queries are structural [01 §8.3]. | The owner asks "what did we decide about X" often enough that lexical recall fails → add a local static-embedding model (~30 MB) as a separate process. |
| **T12 Schema** | **Fixed core** (header + the v1 kinds/edges below, compiled in) **+ schema-as-data extensions** (extra fields, enum values, kinds) that are *weakening* only in v1; strengthening changes need an explicit migration commit (M6). Enum integers never reused. | Role templates are a ready-made schema [01 §2, §6]; TerminusDB weakening/strengthening [06 §4]; Beads' 13 issue types grew by accretion [03 §2.2]. | If the owner wants to add kinds weekly without a moirai release (extensions are already data; only the *fast columns* need a release). |
| **T13 Scope of v1** | **M0–M3** (below): tasks/subtasks/blocks/ready/claims; rules, decisions, findings, refutations, verdicts, questions, rulings, measurements with env; supersede/retract/suspect; `brief`, `pack`, `apply`; CLI + core skill + SessionStart/SubagentStart/SubagentStop hooks; history (log/diff/blame/undo/as-of); thin MCP with role policy. Adopted first on **one new campaign**, with `MEMORY.md`'s resume block rendered from `moirai brief`. Not v1: explicit branches/merge, FTS index, `refs/moirai` export, PostToolBatch push, plan-patch verbs, embeddings. | The top losses are truncation, non-persisting roles, register merges, retraction leaks, finding identity, copy-pasted rules, hand-written checkpoints [01 §7; 02 §11]. | If the owner's first target is re-planning a large existing plan (then plan-section patch verbs move into M2). |

---

## 3. Data model

### 3.1 Node header (fixed columns, 56 B + 16 B uid)

Every node has a dense row index equal to its id (`#N`). Rows are never reused; a deleted node keeps its row with `flags.deleted` in the *history* view only (the current-state snapshot omits it from bitmaps and CSR).

| Column | Type | Bytes | Notes |
|---|---|---|---|
| `kind` | u8 | 1 | code table §3.2; never reused |
| `status` | u8 | 1 | per-kind enum, §3.5 |
| `resolution` | u8 | 1 | tasks/findings/leases |
| `priority` | u8 | 1 | P0..P4 (scheduling) |
| `criticality` | u8 | 1 | critical/high/normal/low (surfacing) |
| `confidence` | u8 | 1 | verified/observed/inferred/speculative; findings: confirmed/plausible |
| `authority` | u8 | 1 | owner/orchestrator/measured/research/agent |
| `flags` | u16 | 2 | done, deleted, suspect, stale, conflicted, claimed, pinned, proposed, has_dangling, container |
| `rev` | u32 | 4 | increments on every change to this node |
| `parent` | u32 | 4 | `parent` edge cached (0 = none) |
| `scope` | u32 | 4 | id of `lane` or `area` node, 0 = global |
| `created_tx`, `updated_tx` | u32 ×2 | 8 | commit seq |
| `title_off`, `abstract_off`, `body_off`, `fields_off` | u32 ×4 | 16 | offsets into blobs (body: zstd frame + u32 raw len) |
| `open_blockers` | u16 | 2 | derived |
| `children_total`, `children_done` | u16 ×2 | 4 | derived |
| `last_op` | u32 | 4 | log position of this node's latest op (per-node history chain) |
| **total** | | **56** | |
| `uid` | u128 (separate column) | 16 | UUIDv7; never printed unless `--uid` |

Kind-specific typed fields live in a tagged-varint block (`field_sym:varint, type:u8, value`): bool takes 0 value bytes, ints are zigzag varints, strings are interned symbols or length-prefixed bytes, node refs are u32, sets are sorted u32 lists. Schema evolution is trivial (unknown symbols are kept, not dropped).

### 3.2 Node kinds (v1) and their typed fields

Kinds mirror the role templates [01 §2, §6.1] and the orchestration vocabulary [02 §6, §12.1]. `done: bool` is a first-class *typed field* on every kind that has a status; it is a projection of `status` (setting `done=true` performs the guarded transition).

| Code | Kind | Typed fields beyond the header | Status set |
|---|---|---|---|
| 1 | `campaign` | `owner_order` (verbatim text), `opened_at`, `closed_at` | open / closed |
| 2 | `phase` | `exit_criteria`, `closing_commit` | open / closed |
| 3 | `lane` | `worktree_path`, `branch`, `base_sha`, `tip_sha`, `target_dir`, `dirty_files:int`, `owns:set<path>` | active / ready_to_merge / merged / frozen / abandoned |
| 4 | `task` | `task_kind` {design, impl, fix, test, measure, merge, doc, research, review, debt}, `phase_state` {proposed, researching, designing, design_review, refuting, design_approved, implementing, code_review, testing, analysis, accepted, committed, merged, documented}, `done:bool`, `owns:set<path>`, `acceptance:text`, `defer_until`, `reopen_if:text`, `pre_registered:bool`, `estimate` | open / in_progress / done / cancelled / deferred / frozen + `resolution` {completed, wontdo, duplicate, superseded, obsolete, rework} |
| 5 | `run` | `wf_id`, `bg_task_id`, `session_id`, `script_path`, `args_hash`, `journal_path`, `started_at`, `ended_at`, `expected_artifacts:set<sym>` | running / green / red / stopped / died |
| 6 | `session` | `harness_session_id`, `started_at`, `stop_point`, `stopped_by` {owner, crash, api_error} | open / stopped |
| 7 | `checkpoint` | `summary`, `next_actions`, `merge_queue:list<lane>` | current / superseded |
| 8 | `lease` | `holder` (agent label), `token:u64` (fencing), `expires_at`, `heartbeat_at`, `holder_pid`, `ttl_s` | active / released / expired / reclaimed / completed |
| 9 | `plan` | `goal`, `targets:list<{metric, value, unit}>`, `revision:int`, `readiness:list<{item, state, reason}>` | draft / in_review / approved / superseded |
| 10 | `plan_section` | `heading`, `ordinal`, `revision:int`, `changed_in_round:int` | current / superseded |
| 11 | `decision` | `what`, `why`, `trade_off`, `decided_at` | proposed / accepted / rejected / superseded |
| 12 | `alternative` | `losing_number`, `measurement_ref`, `git_tag`, `build_flag`, `reopen_if:text`, `stale_after` | proposed / chosen / rejected / frozen / revived |
| 13 | `question` | `question_kind` {values, scope, unclear}, `options:list<text>`, `asked_of` {owner, orchestrator, architect} | open / answered / dropped |
| 14 | `ruling` | `verbatim:text` (owner words), `binding:bool` | active / superseded |
| 15 | `finding` | `local_id` (C1/W2/F3/V-012), `severity` {blocker, important, optional}, `finding_kind` {correctness, perf, complexity, safety, plan_gap, style, security, debt}, `where:text`, `failure:text` (mandatory), `what_needed:text`, `round:int`, `observed_git_sha` | open / confirmed / refuted / deferred / withdrawn / fixed |
| 16 | `refutation` | `outcome` {confirmed, refuted, partial, could_not_check}, `evidence:text` (cmd/grep/file) | recorded |
| 17 | `verdict` | `role`, `round:int`, `raw_label` (APPROVED / CHANGES_REQUESTED / ACCEPTED / REWORK / …), `outcome` {pass, pass_with_conditions, fail_fixable, fail_fundamental, unknown, n_a}, `return_to` {architect, developer, tester, none}, `conditions:text`, `counts:{critical, important, optional}` | recorded / superseded |
| 18 | `test` | `name`, `file`, `red_when:text`, `mutation_proved:bool`, `expected_count:int` | active / retired |
| 19 | `testrun` | `command`, `ran`, `passed`, `failed`, `skipped`, `profile`, `git_sha`, `failures:list<{id, expected, received, cause}>` | green / red / vacuous |
| 20 | `measurement` | `metric`, `value:f64`, `unit`, `target:f64?`, `delta:f64?` (derived), `command`, `git_sha`, `env:{host_triple, profile, load, scale, quiet:bool}`, `baseline_ref` | current / moved_declared / superseded (+ derived `stale`) |
| 21 | `rule` | `text`, `enforcement` {must, should}, `applies_to:{roles:set, phases:set, lanes:set, paths:set<glob>}`, `since`, `rationale` | proposed / active / superseded / retracted / archived |
| 22 | `note` | `note_type` {note, hazard, lesson, critical}, `symptom`, `mechanism`, `defence`, `incidents:int`, `applies_to` (as rule) | active / superseded / retracted / archived |
| 23 | `summary` | (body) — must have ≥ 1 `derived_from` | active / suspect / archived |
| 24 | `area` | `path_globs:set` | active / archived |
| 25 | `artifact` | `path`, `sha256:[u8;32]`, `bytes`, `artifact_kind` {design, critique, test, review, triage, fix, merge, verify, patch, manifest, message, page, plan_file}, `first_line` | present / missing (derived by `doctor`) / superseded |
| 26 | `deviation` | `what`, `why` | open / confirmed / rejected |
| 27 | `role` | `name`, `tools:set`, `can_write:bool`, `model`, `defined_in:path`, `def_hash` | active / stale |
| 28 | `resource` | (a mutex, e.g. "benchmark machine", "merge queue") | free / held |

Subtasks are `task` with a `parent`; rungs/steps are tasks with `task_kind`; backlog/tech-debt items are `task{task_kind=debt, status=deferred, reopen_if}`; a "critical note about the project" is `note{note_type=critical, criticality=critical}`.

### 3.3 Edge kinds

Edges are `(src, kind, dst)` with set semantics and an optional 12-byte property record `{created_tx:u32, pinned_rev:u32, reason_sym:u32}`. Stored in both directions in the same commit.

| Code | Edge (src → dst) | Class | Acyclic? | Cardinality | On **dst** deleted | On **src** deleted |
|---|---|---|---|---|---|---|
| 1 | `parent` (child → parent) | structural | forest, depth ≤ 12 | ≤ 1 out | **restrict** (default); `--cascade` deletes subtree; `--reparent` moves children up | drop, fix rollups |
| 2 | `blocks` (A → B) | structural | DAG on `blocks ∪ child→parent` (Pearce–Kelly) | many | **drop + notify**: `open_blockers[B]--`, event "blocker #A deleted" on B, B flagged `suspect` | drop + notify |
| 3 | `merge_after` (lane → lane) | structural | DAG | many | drop + notify | drop |
| 4 | `runs_in` (run → lane) | structural | – | ≤ 1 | restrict | drop |
| 5 | `holds` (lease → task/resource) | structural | – | 1 | restrict while lease active | drop |
| 6 | `answers` (ruling/decision → question) | structural | – | ≤ 1 active | restrict | drop; question reopens |
| 7 | `scoped_to` (knowledge → area/lane) | structural | – | many | restrict or `--reassign` to parent area | drop |
| 8 | `depends_on` (plan_section → plan_section) | structural | DAG | many | drop + mark src `suspect` | drop |
| 20 | `supersedes` (new → old) | historical | yes | many | keep; tombstone view | keep; `doctor` warns |
| 21 | `derived_from` (summary/checkpoint/finding → source) | historical | yes | many | keep; **src → `suspect`** | – |
| 22 | `cites` (any → knowledge, `pinned_rev`) | historical | – | many | keep; src → `suspect` | – |
| 23 | `refutes` / 24 `confirms` (refutation/measurement → finding/claim) | historical | – | many | keep | – |
| 25 | `verifies` (testrun/measurement/verdict → task/finding/decision) | historical | – | many | keep | – |
| 26 | `addresses` (artifact/commit-ref/task → finding) | historical | – | many | keep | – |
| 27 | `implements` (task/artifact → decision/plan_section) | historical | – | many | keep; src → `suspect` if target superseded | – |
| 28 | `about` (finding/verdict/measurement/question → target) | historical | – | ≤ 1 typical | keep; tombstone | – |
| 29 | `rejected_for` (alternative → decision) | historical | – | ≤ 1 | keep | – |
| 30 | `contradicts` (rule ↔ rule, stored one direction) | historical | – | many | keep | – |
| 31 | `mentions` (any → any; parsed from `#N` in title/abstract/body) | historical | – | many | keep; renders `#40 †c812` | recomputed from text |
| 32 | `relates` | historical | – | many | keep | – |
| 33 | `discovered_from` (new node → task being worked) | historical | – | ≤ 1 | keep | – |
| 34 | `produced` / 35 `consumed` (run → node/artifact) | historical | – | many | keep | – |
| 36 | `returns_to`-like pointers are **fields**, as are `owns`, `measured_on` (git_sha), `raised_by`/`decided_by` (commit provenance + `authority`). | | | | | |

### 3.4 Invariants (checked on every write; re-checked after any merge and by `doctor --verify`)

| ID | Invariant |
|---|---|
| I1 | `#N` unique across the store, never reused; `uid` unique. |
| I2 | Every structural edge has live endpoints at the head. |
| I3 | Historical edges may point to dead ids; those resolve through the tombstone view (`who, when, why, replaced_by`). |
| I4 | `parent` is a forest; depth ≤ 12. |
| I5 | `blocks ∪ {child→parent}` is acyclic; a node never blocks its own descendant; blockers inherit from ancestors only when the blocker is outside the ancestor's subtree ("exogenous only") [06 §7.2]. |
| I6 | `supersedes(new, old)` implies `old.status ∈ {superseded}` in the same commit; `refutes` from a refutation with `outcome=refuted` implies `finding.status=refuted` in the same commit. |
| I7 | Status transitions follow the kind's machine (§3.5); `blocked`, `ready`, `stale`, `done`-for-containers are never stored as source truth. |
| I8 | Derived counters/bitmaps equal a full recompute (debug asserts, property tests, `doctor --verify`). |
| I9 | Every mutation belongs to exactly one commit; the commit carries `{actor, agent_type, session, lane, worktree, git_sha, message, idempotency_key?}`. |
| I10 | Role write policy (§7.6) holds for the stamped `agent_type`; owner `ruling`s exist only with `authority=owner` and non-empty `verbatim`. |
| I11 | A `finding` with `finding_kind ∈ {perf, complexity}` may reach `fixed` only with an `addresses` edge **and** a `verifies` edge from a `verdict{role=code-reviewer\|architecture-critic}` (a `testrun` alone is insufficient — "retest is not re-review" [01 §3]). |
| I12 | A `run` may close `green` only when every `expected_artifacts` symbol has a `produced` artifact whose `sha256` was read back ("0 errors but no plan file" [01 §7 L2]). |
| I13 | A `lease` mutation must present the current fencing `token`; a stale token is rejected (exit 5). |
| I14 | Reverse CSR == inverse(forward CSR) after every commit (property test). |

### 3.5 Status machines and merge lattices

- **task**: `open → in_progress → done`; side exits `cancelled`, `deferred`, `frozen`. Lattice for merge: `done > in_progress > open`; `cancelled`/`frozen` are terminal and beat `open`. **Reopen is an explicit op** (`reopen --reason`, increments `reopen_count`) and never a merge artefact. `phase_state` is a separate enum advanced by verdicts (`return_to` moves it back explicitly).
- **finding**: `open → confirmed | refuted | deferred | withdrawn`; `confirmed → fixed` (I11). Lattice: `refuted/withdrawn/fixed > confirmed/deferred > open`.
- **question**: `open → answered | dropped` (answered via `answers` edge). Lattice: `answered > open` — this is the direct fix for "RESOLVED printed OPEN again in 10 places" [01 §7 L5].
- **rule/note/decision**: `proposed → active → superseded | retracted | archived`. Written from a lane: `proposed` until `reconcile`.
- **lease**: `active → released | completed | expired | reclaimed`.
- **lane**: `active → ready_to_merge → merged | frozen | abandoned`.
- **run**: `running → green | red | stopped | died`.
- **measurement**: `current → moved_declared | superseded`; `stale` is derived (lane tip moved past `git_sha` without a re-measure, or `env.host_triple` differs from the current host).

### 3.6 Derived state (maintained eagerly, only for affected nodes, on the single write path)

| Derived value | Definition | Update cost |
|---|---|---|
| `open_blockers[n]` | count of `blocks` in-edges whose source is not `done/cancelled` | O(out-degree) on status change |
| `ready` bitmap | `kind=task ∧ status=open ∧ ¬container ∧ open_blockers=0 ∧ no exogenous open blocker on any ancestor ∧ no active lease ∧ defer_until ≤ now ∧ ¬conflicted` | O(out-degree + depth) |
| `blocked` (query-only) | `open ∧ ¬ready` with an explanation list | O(depth + in-degree) |
| `is_blocker` bitmap | `¬done ∧ has outgoing blocks to an open task` | O(1) per edge change; "ids of all blocking tasks" = one bitmap scan |
| rollups | `children_total/done`; container `ready_to_close` when all children done | O(depth) |
| `suspect` | set when a `derived_from`/`cites`/`depends_on`/`implements` target is retracted, superseded, deleted, or moved past `pinned_rev`; propagates transitively along reverse derivation edges | O(affected closure) |
| `stale` (measurement/artifact/knowledge) | `measurement.git_sha` behind the lane tip; `artifact.sha256` ≠ file; `rule/note.observed_git_sha` with changed `applies_to.paths` (ancestry checked lazily via git, cached) | on demand + cached per (sha, tip) |
| `authoritative` | no live `supersedes`/`retracts` in-edge | O(1) flag |
| `conflicted` | node has an unresolved conflict/violation record (merge only) | O(1) |
| `dangling_count` | historical edges to dead ids | O(1) on delete |
| review-loop termination | `count(finding{about ⊂ target, status=confirmed, severity ≥ important, ¬fixed}) = 0` | bitmap intersection |
| refuted share | per critic, per round: `refuted / raised` | bitmap counts |

Each predicate is defined once in the engine and used by every command and tool (Beads' "three universes" [06 §2.2 #6105]).

---

## 4. Storage engine

### 4.1 On-disk layout — `<git-common-dir>/moirai/` (≤ 6 files)

| File | Size | Role |
|---|---|---|
| `HEAD` | 8 KiB (2 × 4 KiB slots) | slot: `magic u32, format_ver u16, flags u16 (quiet, readonly), seq u64, commit_id [u8;32], log_seg u32, log_off u32, next_id u32, snapshots [u32;4], refs_off u32, dict_id u32, xxh3_128 [u8;16]`. Readers take the valid slot with the highest `seq`. Refs (`trunk`, `exp/*`, `leader`, per-session cursors) are a small table inside the slot (≤ 3 KiB). |
| `LOCK` | 0 B | byte 0: writer (exclusive); byte 1: leader (exclusive); bytes 2..1025: reader registry (shared lock on byte `2 + (seq % 1024)`, so a compactor can find the oldest live snapshot); byte 4096+: quiet-mode marker. All locks via `LockFileEx` on this file only; data files are never locked [05 §6.3; 08 W1]. |
| `log.NNNN` | preallocated 16 MiB segments | append-only, checksummed records (§4.2). Never truncated; sealed when full. |
| `snap.NNNN` | immutable | columnar snapshot (base) — mapped read-only. |
| `snap.NNNN.d1`, `.d2` | immutable | up to two delta snapshots since the base (M6; v1 rewrites the base at every checkpoint because it is ≤ 30 MB at 1e5). |
| `idem` | inside `snap` | idempotency table `(key_hash u128 → commit seq, result hash)`; the tail part is in the log. |

The zstd dictionary lives in the base snapshot footer (`dict_id` in HEAD). The `next_id` counter is in HEAD and in every commit record; on crash recovery it is `max(id in log tail) + 1`.

### 4.2 Log records

Record header (16 B): `len u32 | xxh3_64 u64 | type u8 | flags u8 | pad u16`. Types: `Commit`, `Lazy` (unflushed class), `Checkpoint` (snapshot published), `Ref`, `Quiet`.

Commit body:

```
commit_id      [u8;32]  BLAKE3(header_without_id ‖ ops)
parent_seq     u64      (0 for root); merge: second parent u64 (flags.merge)
seq            u64      monotonic
hlc            u64      hybrid logical clock (48-bit ms ‖ 16-bit counter)
actor_sym      u32      "orchestrator", "developer#2", "session:<id>"
agent_type_sym u32
session_sym    u32
lane           u32      node id or 0
git_sha        [u8;32] + alg u8 (SHA-1 padded / SHA-256)
git_branch_sym u32      ("detached" allowed)
worktree_sym   u32
idem_key       u128     (0 if none)
durability     u8       durable | lazy
next_id_after  u32
msg_len u16 + msg
n_ops   u32 + ops[]
```

Ops (tagged varints, each with a before-image for undo/as-of/blame):

`Create{id,kind,uid}`, `Tombstone{id, reason_sym}`, `SetField{id, field_sym, old, new}`, `SetStatus{id, old, new, resolution, reason_sym}`, `AddEdge{src,kind,dst,props}`, `RemoveEdge{src,kind,dst}`, `Move{id, old_parent, new_parent}`, `SetText{id, which(title|abstract|body), old_hash, new_blob_ref}`, `Lease{op, lease_id, token, expires}`, `Derived{...}` is **never** logged (recomputed).

Typical commit: 200 B header + 2–6 ops × 20–80 B ≈ **0.3–0.6 KB** (matches [04 §6]). Bodies are written into the log inline (zstd frame) and copied into the snapshot blob at checkpoint.

### 4.3 Snapshot segment (`snap.NNNN`, immutable, mapped read-only)

Structure of arrays, little-endian, each column `#[repr(C)]` read through `zerocopy::Ref` with bounds checks; footer with per-column offsets, per-column xxh3-64, `format_ver`, `seq_at_checkpoint`, `blake3` of the logical content.

| Section | Layout | Size at 1e5 (est.) |
|---|---|---|
| header columns (§3.1) | 56 B × N | 5.6 MB |
| `uid` | 16 B × N | 1.6 MB |
| forward CSR | `off:[u32;N+1]`, `dst:[u32;E]`, `kind:[u8;E]`, `prop_idx:[u32;E]` (0 = none) | 0.4 + 3.5 × 9 B ≈ 3.6 MB at E = 3.5N |
| reverse CSR | same | 3.6 MB |
| edge props | 12 B × (edges with props) | ≤ 1 MB |
| bitmaps | roaring, frozen (queryable in place): per status, per kind, `ready`, `is_blocker`, `suspect`, `stale`, `proposed`, `claimed`, `container` | ≤ 50 × 20 KiB ≈ 1 MB |
| symbol table | sorted `(len u16, bytes)` + `off:[u32]` | ≤ 0.2 MB |
| title blob | length-prefixed UTF-8 | 6 MB (60 B avg) |
| abstract blob | UTF-8 | 7.5 MB (150 B avg on ~50 % of nodes) |
| body blob | zstd frames with dictionary, `raw_len u32` prefix | ~27 MB (40 % of nodes × 2 KiB raw ÷ 3) |
| field blocks | tagged varints | 2.4 MB (24 B avg) |
| `last_op` | u32 × N | 0.4 MB |
| idempotency table | sorted `(u128, u64, u128)` | ≤ 1 MB |
| zstd dictionary | 64–110 KiB | 0.1 MB |

"Hot set" touched by `ready`/`blockers`/`brief`: headers + CSR + bitmaps ≈ **100 B/node** → 1 / 10 / 100 MB at 1e4 / 1e5 / 1e6 (consistent with [05 §3, §10.4]).

### 4.4 Write path (embedded; the leader path forwards the same request over the pipe)

1. `LockFileEx(LOCK, byte 0, EXCLUSIVE)` with bounded retry: 0.5 ms × 2^k jitter up to 250 ms total; on timeout exit 7 with the holder (from the lease/heartbeat table) named.
2. Read `HEAD` (4 KiB explicit read, ~0.17 ms), pick the valid slot; if the process's overlay is behind `seq`, replay the missing log records (checksummed) into the overlay.
3. **Validate** the requested ops against head state: schema (I11-style field types), write policy for the stamped `agent_type` (I10), status transition (I7), structural endpoints (I2), forest (I4), acyclicity via Pearce–Kelly on `blocks ∪ child→parent` for each added edge (affected-region search only), lease token (I13), guards (`--if-rev`, `--if-status`, `--if-holder`), idempotency key lookup (if present, return the stored result and stop).
4. Compute derived deltas (`open_blockers`, ready/is_blocker bitmap deltas, rollups, `suspect` closure, `mentions` re-parse of changed text) and the change-log affected set.
5. Serialize the commit record (§4.2) including `next_id_after`, `seq+1`; append to `log.NNNN` (preallocated, so overwrite-in-place cost); if the segment is full, seal it and open the next.
6. If `durability=durable`: `NtFlushBuffersFileEx(FLUSH_FLAGS_FILE_DATA_SYNC_ONLY)` on the log segment (1.73 ms measured [05 §2.2]). **fsync failure is fatal** (abort, recover on next open).
7. Write the other `HEAD` slot (no flush; recoverable from the log: 1PC+C).
8. Unlock byte 0. Return `{seq, commit_id, per-node rev, content hashes of written text}`.
9. If `tail_bytes > 4 MiB` or `tail_ops > 4,096` and quiet-mode is off: run a **checkpoint** synchronously before unlocking (≤ 30 ms at 1e4, 0.1–0.3 s at 1e5, est. [05 §7]); at 1e6 (M6) write a delta snapshot instead (10–100 ms est.).

Group commit (M3, leader only): while ≥ 1 write is pending, the leader accumulates arrivals for ≤ 5 ms or until 64 records, appends them all, flushes once (3.05 ms for 64 pages measured), then acknowledges each caller. No timer runs when no write is pending.

### 4.5 Read path

1. Read `HEAD` → `seq`, snapshot list. Long-lived processes cache the mapping keyed by snapshot id and compare `seq` per request (one 4 KiB read; or an 8-byte `pread` of the slot's `seq`).
2. Map `snap.NNNN` read-only (`memmap2`, `PROT_READ` / `PAGE_READONLY`), validate the footer (`format_ver`, `seq_at_checkpoint`; column checksums were validated at seal time and are re-validated only by `doctor`).
3. Replay log records `(seq_at_checkpoint, head.seq]` into the overlay: sorted `Vec<(src,kind,dst,±)>` for edges, `SmallVec` per touched node for fields, a small `HashMap<u32, NodeDelta>` for headers, bitmap deltas. Bounded by the checkpoint policy: ≤ 4,096 ops → ≤ 1–3 ms (est.).
4. Answer the query from columns + overlay through one accessor layer (`node(id)`, `out(id, kind)`, `in(id, kind)`, `bitmap(name)`), decompressing bodies into a per-request bump arena. The CLI never materialises node structs.

### 4.6 Open path

`HEAD` read (0.17 ms) + 1–3 mappings (0.22 ms each) + tail replay (≤ 3 ms) = **0.5–3 ms** regardless of history length. No index rebuild, no O(N) validation. Repeated by every CLI invocation; kept warm by MCP servers.

### 4.7 Indexes

- **Adjacency**: forward + reverse CSR (immutable) + overlay. `blockers T --transitive` is a DFS over reverse `blocks` with a visited bitmap and a node budget (default 10k).
- **Bitmaps**: status × kind, `ready`, `is_blocker`, `suspect`, `stale`, `proposed`, `claimed`, `container`, `authoritative`; `find kind:task status:open prio:<=1` = bitmap AND then filter.
- **Id → row**: identity. `uid → id`: sorted `uid` column, binary search (needed only for import).
- **Symbols**: sorted table; role/phase/label/path symbols enable `applies_to` matching by set intersection over u32s.
- **Text**: v1 brute force over mapped titles/abstracts/bodies with `memchr`/Aho–Corasick, capped at 50 ms; M6 adds an FST term dictionary + delta-varint postings as an extra snapshot section (324 KB for 119k terms measured elsewhere [05 §13]).
- **Ancestry cache** (M4): `(git_sha, trunk_tip) → bool` from `git merge-base --is-ancestor`, stored as lazy facts in the log so it is shared by all processes.

### 4.8 Compaction and GC

- **Checkpoint**: fold the overlay into a new base snapshot (`snap.NNNN+1`), publish via `HEAD`, keep the old base until no reader registry byte for a `seq` older than the new checkpoint is held **and** a 60 s grace has passed; delete with `FILE_SHARE_DELETE` semantics, tolerate `ERROR_ACCESS_DENIED`/`SHARING_VIOLATION` and retry on the next checkpoint [08 W3].
- **Log**: never truncated (it is the history). `moirai gc --cold-pack` rewrites sealed segments older than the retention window with zstd (batch frames), keeping commit headers intact; segments referenced by a pinned snapshot's `seq_at_checkpoint` are kept as-is so as-of queries stay bounded.
- **History anchors**: every 64th checkpoint's base snapshot is retained (logarithmically thinned after a year) for as-of queries.
- **Nothing runs in the background**: checkpoints happen inside a write (or `moirai checkpoint`), gc only on `moirai gc`. `moirai quiet on` sets `HEAD.flags.quiet`: no auto-checkpoint, no leader broadcast batching, no gc.

### 4.9 Crash safety

- Torn-tail rule: on open, scan forward from `seq_at_checkpoint`; the first record with a bad length or checksum is the end of the log. A commit whose record is complete but whose `HEAD` slot write was lost is still recovered (1PC+C).
- Two `HEAD` slots with `xxh3_128`; the writer alternates slots.
- Lazy-class records are recovered if their bytes made it; they may be lost after power loss (documented).
- Leases, idempotency keys and cursors are log records, so they survive any process death.
- `doctor --verify` recomputes all derived state, checks I1–I14, and reports (never repairs silently; `--repair` rebuilds the snapshot from the log, which is the only legal repair).
- Windows: byte-range locks vanish with the process (with possible delay [08 W2]); no rename on the commit path; no shared memory; no mmap writes; refuse network/OneDrive paths.

### 4.10 Windows specifics (summary of the rules obeyed)

`LockFileEx` only on `LOCK` (data files never locked) [05 §6.3]; lock bytes past EOF [08 W1]; `NtFlushBuffersFileEx` via `windows-sys` [05 §6.2]; mapped files are immutable and never truncated [05 §6.1]; ≤ 6 files, preallocated segments, stable exe path [05 §6.4]; named pipe with per-user DACL, `FILE_FLAG_FIRST_PIPE_INSTANCE`, `PIPE_REJECT_REMOTE_CLIENTS` (M3) [08 W8]; bodies via stdin/`@file`/JSON (PowerShell 5.1 strips quotes) [07 §5.2]; exec-form hooks with absolute path fallback [07 §9.5].

---

## 5. Versioning

### 5.1 Commits and the op log

Every write is a moirai **commit** (jj-style auto-commit): bursty, hundreds per day [02 §12.6] × 0.3–0.6 KB ≈ **50–100 MB/year** of history uncompressed, 2–4× less cold-packed (est.). Commits form a DAG through `parent_seq` (+ second parent for merges) and are content-addressed by BLAKE3 for tamper-evidence and cross-machine export. `apply` batches are one commit. A "session unit" is not needed as a separate level: `changes --since <seq>` and `log --session S` group commits by provenance.

### 5.2 Refs and branches

`trunk` is the live timeline. `exp/<name>` (M5) is a ref plus a **delta overlay** (the same overlay structure as the tail) forked at a commit; reads on `exp/x` consult the overlay first. Ids on `exp/*` come from the same global counter. Long-lived `exp/*` branches are rebuilt by replay, never promoted to their own snapshot in v1.

### 5.3 Diff

`diff A..B` = the ops in `(A, B]` grouped by `(node, field|edge)` and rendered as the *final* change per key (a field set three times shows once, with `A`'s and `B`'s values); `diff --lane L` is `diff` filtered by `lane`; `diff P --since-round k` (plan sections) lists sections whose `changed_in_round > k` plus their `depends_on` dependents — the critic's "round scope = delta" [01 §2.3].

### 5.4 History, blame, as-of, undo

- `log #40` walks the per-node op chain via `last_op` (O(edits to the node)) and prints commit provenance per change.
- `blame #40 --field failure` gives the commit that set the current value.
- `show #40 --as-of <seq|commit|time>`: reverse-apply this node's ops newer than the point (cheap); whole-graph as-of (`--as-of` on `find`) reverse-applies from the current snapshot for recent points, otherwise forward-replays from the nearest retained anchor snapshot (bounded by anchor spacing).
- `undo <commit>` appends the inverse changeset as a new commit (never rewrites history); refused if a later commit depends on it structurally (then it reports the conflict set).

### 5.5 Merge algorithm (M5; `exp/*` → `trunk`, or `trunk` → `exp/*` to refresh)

1. LCA by `parent_seq` walk (generation numbers cached per commit).
2. Collect both sides' changesets since the LCA (they carry before-images).
3. Partition by key `(node, field)` / `(src,kind,dst)`. Disjoint keys commute → apply directly.
4. Overlapping keys → typed 3-way:
   - status enums: lattice (§3.5); `reopen` events are ops and replay as explicit reopens;
   - scalar fields: if one side equals base, take the other; else **conflict value** `{base, ours, theirs}`;
   - text: line diff3; overlapping hunks → conflict value;
   - sets (`labels`, `owns`, `applies_to.*`): add-wins union with removals relative to base;
   - `parent`: Kleppmann move semantics (apply in HLC order, skip a move that would create a cycle, record it);
   - derived values: never merged, recomputed after step 5.
5. Apply to an overlay on the target; run validators: Kahn over `blocks ∪ child→parent` (O(V+E)), forest, structural endpoints live (reverse index), schema.
6. Write the merge commit containing conflict values and **violation records** (`DanglingEdge`, `Cycle`, `DeleteVsModify`, `IdCollision` (impossible by construction), `Constraint`); nodes involved get `conflicted` and leave `ready`.
7. Advance the ref unless `--strict`. `moirai conflicts` / `moirai resolve #40 --take ours|theirs|value` clear them.

Conflict taxonomy follows the SQLite-session classes plus `CYCLE` [04 §5].

### 5.6 Git linkage

Every commit records `git_sha`, `git_branch`, `worktree`, `lane` (read from the `.git` file / `commondir` without spawning git; supplied by the stamp hook for MCP). SHA links are provenance, not foreign keys (rebase/squash rewrite them). Knowledge visibility for `scope=lane:L` nodes is "applies" in a worktree whose HEAD contains the node's `git_sha` (ancestry cache, M4), "pending on lane L" otherwise. `moirai reconcile --lane L --into trunk` (an explicit orchestrator step or a git `post-merge` hook) promotes `proposed` knowledge, marks tasks `done-on-trunk` when their evidence commits are ancestors, flags abandoned lanes, and writes a `merge` event. Optional M7: publish history as git objects under `refs/moirai/data` with a fetch refspec and a `doctor` check against `push --mirror` deletion [08 §7.3].

---

## 6. Concurrency and synchronisation

### 6.1 Processes and locks

| Process | Lifetime | Store access | Locks held |
|---|---|---|---|
| `moirai` CLI (hooks, Bash agents, orchestrator scripts) | ms | direct (or forwards to leader if `HEAD.refs.leader` names a live pipe) | writer byte during a commit only |
| `moirai mcp` (one per Claude session; shared by its subagents) | session | direct; becomes **leader** if it wins byte 1 (M3) | leader byte for life; writer byte per commit |
| `moirai watch` (plugin monitor, optional M4) | session | read-only | none (reader registry byte while iterating) |

Readers never block writers; writers never block readers (readers read the last valid `HEAD` slot and immutable snapshots). One writer at a time (writer byte); the leader serialises forwarded writes in-process and still takes the writer byte so a direct CLI writer can never interleave.

### 6.2 Leases and claims

`claim #51 --agent wf:r7/dev#1 --ttl 60m` is one durable commit: task must be `open`, `ready`, unclaimed (or same holder → idempotent). Returns `lease #L, token 1043, expires 12:41`. Every mutation on the task by an agent presents `--lease L` and the token; a stale token fails with exit 5 and the current holder. Liveness: TTL; `heartbeat` (explicit, or the `async` PostToolUse hook); `holder_pid` liveness check when a hook runs; `SubagentStop` releases or flags; `reclaim --older-than 30m`. `resource` nodes give the same semantics for "one benchmark at a time" and the merge queue.

### 6.3 Change feed

The commit record's affected set `{ids touched, ids whose derived state changed, kinds}` is the feed; `seq` is the cursor. `changes --since <seq> [--for-agent A | --lane L | --scope #T]` filters to: nodes the agent holds leases on, is blocked by, cited in its last pack (`consumed` edges from its `run`), or rules matching its role. Long-lived processes learn of foreign commits by comparing `HEAD.seq` on their next request; the leader (M3) additionally pushes `{seq, ids}` over the pipe to followers and `watch`. File watchers are at most a doorbell [08 W10] and are not used in v1.

### 6.4 "Node 40 deleted" end to end

1. **Store (L0, same commit)**: `rm #40 --yes` → policies over #40's reverse list: `blocks(40→12)` dropped, `open_blockers[12]--`, #12 gets `suspect` + event; `parent(40→7)` restricted unless `--cascade`; `cites(88→40)` kept, #88 → `suspect`; `mentions(17→40)` kept; tombstone `{40, c812, developer#2, "dup of #52", replaced_by #52}`; affected = {12, 88, 17, 7}; ready bitmap updated; commit flushed.
2. **Other processes (L1)**: the orchestrator's MCP server sees `seq=812` on its next request and replays the record (µs); a CLI call opens fresh. Any read of #12 now prints `blocked_by: (none) · suspect: blocker #40 deleted c812`; any reference renders `#40 †c812 by developer#2: "dup of #52" → #52`.
3. **Observers (L2, M3)**: leader broadcasts `{812, [40,12,88,17,7]}`; `moirai watch --for-session S` prints `#12 READY (blocker #40 deleted by developer#2)`.
4. **Agents (L3)**: the next `UserPromptSubmit` (or PostToolBatch, M4) delta for the session holding a lease on #12 injects `since rev 800: #40 deleted (was blocking your #12 → READY); #88 suspect`. A stale agent that writes `set #40 …` gets exit 3 with the tombstone; `set #12 --if-rev 5` fails with exit 4 and the current value. Nothing based on a stale read can succeed silently.

### 6.5 Idempotency

Every write accepts `--idempotency-key K` (mandatory for `apply` from Workflow scripts: `run:<wf_id>/agent:<label>`). The store keeps `(blake3(K) → seq, result_hash)` for 30 days; a retry returns the original result and `replayed: true`. Required by Workflow resume semantics, MCP client restarts and model retries [07 §4.1, §7.4].

---

## 7. Agent interface

### 7.1 CLI surface (v1 verbs; every write takes `--agent`, `--idempotency-key`, `--stdin`/`@file` for text; empty results exit 0)

```
moirai brief   [--lane L] [--budget 8k] [--more]                # session-start digest
moirai pack    #T --role R [--phase P] [--lane L] [--budget 12k] [--since-round k]
moirai ready   [--scope #E] [--role R] [--limit 20] [--ids]
moirai blocking [--ids]            moirai blockers #T [--transitive] [--explain]
moirai show    #ID.. [--full] [--as-of S]   moirai tree #T [--depth 3]
moirai find    [TEXT] kind:task status:open prio:<=1 lane:l5np severity:>=important [--ids]
moirai log #ID | --since S | --lane L     moirai diff A..B | #P --since-round k     moirai blame #ID --field f
moirai add     task|question|lane|run|artifact|test|measurement "title" [--parent #P] [--blocks #X] [--field k=v]..
moirai rule|note|decision|finding|verdict|refutation "text"|--stdin --about #T [--severity ..] [--applies-to role:tester,phase:verify,path:crates/phys/**] [--authority owner --verbatim @file]
moirai set     #ID k=v.. [--status S] [--done] [--if-rev N] [--if-status S] [--lease L]
moirai link    #A --blocks|--parent|--cites|--supersedes|--implements|--addresses|--verifies #B [--pin]
moirai supersede #OLD --with #NEW        moirai retract #ID --reason ..        moirai resolve #Q --by owner --verbatim @file
moirai rm      #ID [--cascade|--reparent] [--dry-run] --yes                         # prints referrer impact first
moirai apply   FILE|- [--idempotency-key K] [--dry-run]                              # atomic batch with $refs
moirai claim   #T.. | --next --scope #E --role developer --agent A [--ttl 15m]
moirai heartbeat L   moirai release L   moirai reclaim --older-than 30m
moirai complete #T --lease L --outcome done|failed|abandoned --summary - [--evidence #M,#R,sha]
moirai reopen  #T --reason ..            moirai stale [--lane L]            moirai conflicts
moirai changes --since S [--for-agent A]  moirai watch [--for-session S] [--jsonl]
moirai stats refuted-share --role architecture-critic [--round k] ; moirai stats loop #P
moirai lane open --worktree <lanes-dir>/l5np --branch u/l5np --base <sha> ; moirai lane conflicts #L1 #L2 ; moirai lane merge-check #L ; moirai reconcile --lane L
moirai checkpoint | gc [--cold-pack] | quiet on|off | doctor [--verify|--repair|agents|hooks]
moirai hook session-start|subagent-start|subagent-stop|agent-launched|prompt|stamp
moirai mcp [--legacy|--modern|--auto]   moirai export memory-md | rules --to .claude/rules/moirai/
```

Example outputs (compact text; `--json` gives `{"v":1,"rev":812,"data":…,"next":…,"dropped":…}`):

```
$ moirai blocking --ids
#12
#31
#77

$ moirai blockers #51 --transitive --explain
rev 812 · #51 task open P1 "Wire lease reclaim" · BLOCKED
  #12 task in_progress P1 "Lock protocol"            blocks #51   holder developer#1 lease 11m
  #40 †c812 by developer#2 "dup of #52" → #52        (dropped from #51's blockers at c812)
  #9  question open scope "TTL default 15m or 60m?"  blocks #51 via ancestor #7 (exogenous)

$ moirai note "Never force-push a shared branch" --critical --applies-to role:* --authority owner --verbatim @ruling.txt
#913 note critical active  authority=owner  rev 813  sha=3f9a…  (will appear first in every brief and pack)

$ moirai set #12 --status done --lease L-9
error[guard_conflict]: #12 rev 7 (you sent --if-rev 5): status=in_progress changed by developer#3 at c811 "took over after lease expiry"
hint: re-read with `moirai show #12`, or pass --if-rev 7
exit 4
```

Exit codes: 0 ok (incl. empty), 1 internal, 2 usage, 3 not found (tombstone printed), 4 guard conflict (current value printed), 5 lease lost/stale token, 6 precondition (blocked / open blocking verdict / I11), 7 store locked or unavailable, 8 partial batch.

### 7.2 MCP tools (9; compact text results, no `structuredContent` by default; `ctx` stamped by the hook and HMAC-verified)

| Tool | Purpose | Key params | alwaysLoad |
|---|---|---|---|
| `brief` | session digest (same as CLI) | `lane`, `budget` | yes |
| `pack` | budgeted context pack for a target and role | `target`, `role`, `phase`, `budget`, `since_round` | yes |
| `get` | nodes by id (concise/full), neighbours, `as_of` | `ids[]`, `detail`, `neighbors`, `as_of` | yes |
| `find` | filters + text + `ready`/`blocking`/`stale` presets | `q`, `filters`, `preset`, `limit`, `cursor` | no |
| `claim` | claim / next / heartbeat / release | `action`, `id`, `scope`, `role`, `ttl_s`, `lease` | yes |
| `complete` | finish a claimed task; returns newly ready ids | `id`, `lease`, `outcome`, `summary`, `evidence[]`, `idempotency_key` | yes |
| `remember` | rule / note / decision / finding / refutation / verdict / measurement / question with kind-specific validation | `kind`, `text`, `about[]`, `fields{}`, `applies_to{}`, `idempotency_key` | yes |
| `write` | typed batch: create / set / link / transition with guards and `$refs` | `ops[]`, `idempotency_key`, `dry_run` | no |
| `changes` | since a rev, or history of one node | `since_rev`, `id`, `for_agent`, `limit` | no |

Server `instructions` (front-loaded, ≤ 2,048 chars): "moirai = this repo's live task graph + rules/decisions/findings memory shared by all worktrees. Call `brief` first, `pack` before working a task, `claim` before editing, `complete` when done, `remember` for findings/rules/decisions. Ids are `#N`; a `†` marks a deleted node." Schema size (est.) ≈ 4k chars ≈ 1.2k tokens, of which ~200 chars load up front under tool search [07 §5.1].

### 7.3 Skills

- `moirai` (core, ~60 lines): verbs, output conventions, `--agent`, bodies via stdin, exit codes, one example per verb; links `reference.md`. Dynamic context: `` !`moirai brief --budget 3k` ``.
- `moirai-orchestrate` (user-invocable; preloaded into the orchestrator `--agent`): campaign/lane/rung setup, dispatch patterns (§7.7), verdict routing, loop-termination query, reconcile ritual, `MEMORY.md` render.
- `moirai-report` (preloaded into developer/tester/reviewer/critic via `skills:`): 5 lines on how to finish — `complete` fields, `remember --kind finding` with mandatory `failure`, `measurement --env --git-sha`.

### 7.4 Hooks (exec form, fail-open, explicit timeouts)

| Event | Command | Effect |
|---|---|---|
| `SessionStart` (startup/resume/compact/clear) | `moirai hook session-start` (10 s) | injects `brief` ≤ 8,000 chars; renders the same text into the top of `MEMORY.md` if `export memory-md` is enabled |
| `SubagentStart` | `moirai hook subagent-start` (10 s) | injects role rules (critical first) + protocol line + `pack` reference for the task named in the run's dispatch table |
| `PostToolUse` matcher `Agent` (`async`) | `moirai hook agent-launched` | maps `agentId → {task, lease}` from the `moirai:task=#51 lease=L-9` marker in the prompt; creates the `run`/agent record |
| `SubagentStop` | `moirai hook subagent-stop` (10 s) | if a lease is still active: store `last_assistant_message` as a `needs-triage` note linked to the task, release or block once; checks I12 expected artifacts |
| `PreToolUse` matcher `mcp__moirai__.*` | `moirai hook stamp` (5 s) | `updatedInput.ctx = {agent_id, agent_type, cwd, session_id, hmac}`; `permissionDecision: allow` (owner may choose `ask`) |
| `UserPromptSubmit` | `moirai hook prompt` (5 s) | relevance-filtered delta since the session cursor; prints nothing when nothing changed |
| `PostToolBatch` (M4, optional, `mcp_tool` type) | `changes` | ≤ 600-char delta for agents holding leases; off by default |

`WorktreeCreate` is **not** used (it replaces git worktree creation [07 §4.2]); lanes are opened explicitly by `moirai lane open` or lazily from provenance.

### 7.5 Context-pack algorithm

`pack #T --role R --phase P --lane L --budget B` (B in tokens; chars = 3.5 × B with a 10 % margin; both reported).

1. **Resolve**: T's ancestor chain to the campaign; lane from `--lane` or cwd; round `k` from the latest `verdict{about ⊂ T}`.
2. **Candidate classes** (each item has three renderings: L0 one line ≈ 80 chars; L1 abstract + key fields ≈ 300 chars; L2 full body):
   - **C1 lane header** (always, L1): worktree, branch, base/tip sha, target dir, dirty count, *other active lanes' `owns` sets* (do-not-touch), quiet-mode state.
   - **C2 rules** where `applies_to ∩ {R, P, L, *} ≠ ∅ ∧ authoritative`: order = criticality ↓, authority (owner > orchestrator > measured > research > agent), id ↑; `critical` → L2, `high` → L1, else L0; `contradicts` pairs are shown together with a `⚠ contradicts #x` marker.
   - **C3 target**: T at L2 (body, acceptance, owns, status, lease), ancestors at L0, `question{open, blocks ⊂ subtree}` at L1 with options, `ruling{about ⊂ subtree}` at L1 (owner verbatim never truncated).
   - **C4 effective spec**: `plan_section` reachable via `implements`/`about` from T; role filter: developer → "Implementation plan" sections (L2); tester → "Metrics and validation" sections + `test.expected_count` + `measurement` baselines with env (L2/L1); critic → only sections with `changed_in_round > k` plus `depends_on` dependents (L2), unchanged sections listed at L0 with `unchanged since r<k>`; `artifact` pointers (path + sha256) at L0.
   - **C5 findings** `about ⊂ target`: developer/fixer → `confirmed` only (L1, `failure` included); critic round k+1 → its own previous findings with status (L0), `refuted` ones as ids only with `do not re-raise`; reviewer → open findings on the same files.
   - **C6 measurements/pins** for the lane and trunk: current pins (L1), `stale` ones marked, known reds (L0).
   - **C7 scoped hazards/notes** whose `applies_to.paths` intersect T.`owns` or ancestors' `owns`: criticality-ordered, L1/L0.
   - **C8 delta** since this (agent, T) cursor: L0, max 10.
3. **Fill**: per-class minimum quotas (C1 fixed; C2 ≥ 15 %; C3 ≥ 20 %; C4 ≥ 30 % for developer/tester, ≥ 40 % for critic; C5 ≥ 10 %), then remaining budget by global priority `(class rank, criticality, recency, id)`. Degrade an item L2 → L1 → L0 before dropping it; never cut an item mid-text; owner rulings and `critical` rules are never degraded below L1.
4. **Emit**: deterministic order (prompt-cache friendly); header `moirai pack #51 developer rev 812 · 11,420/12,000 tok`; footer `dropped: 4 findings(optional) #88 #91 #93 #95, 7 notes → moirai pack #51 --more` (ids always listed so the agent can fetch).
5. **Record**: a `consumed` edge from the current `run` to every L1/L2 item with `pinned_rev` (`#40@r7`), so "which runs used the retracted spec?" and PLANFENCE-style checks (`moirai check #51`: nothing consumed has moved) are queries [06 §11.2].

`brief` is the same machine with fixed classes: current checkpoint per open campaign, live lanes (path/branch/tip/dirty/status), runs in flight, merge queue, open owner questions, top critical rules/hazards, stale summaries, verdicts since last session; default budget 2.3k tokens (8,000 chars) [07 §4.1].

### 7.6 Role write policy (enforced on the stamped `agent_type`; CLI `--agent` is trusted for the orchestrator's shell and the SubagentStart-injected label)

| Role | May create | May transition | May not |
|---|---|---|---|
| orchestrator | everything except `ruling{authority=owner}` without `--verbatim` | all | – |
| owner (main session, `--by owner`) | `ruling`, `question.answered`, `rule{authority=owner}` | all | – |
| architect | `plan`, `plan_section`, `decision`, `alternative`, `question`, `deviation.confirmed` | plan revisions | `verdict`, `finding.fixed`, task status |
| researcher | `note`, `artifact{research}`, `question` | – | anything else |
| architecture-critic | `finding{about: plan_section}`, `verdict{role=critic}` | own findings → withdrawn | editing plan text, `finding.fixed` |
| refuter | `refutation` | finding → confirmed/refuted (via I6) | – |
| developer | `claim`, `deviation`, `question`, `artifact{impl}`, `complete`, `note` | own task status (with lease) | `verdict`, `finding.fixed` on its own task |
| code-reviewer | `finding{about: artifact/file}`, `verdict{role=reviewer}` | `finding.fixed` (I11) | code edits (not moirai's concern), `verdict{role=analyst}` |
| tester | `test`, `testrun`, `measurement`, `finding{kind=test}` | – | `verdict` |
| results-analyst | `verdict{role=analyst, return_to}`, `task{kind=debt}` | task `phase_state` back-moves | – |
| project-analyst | `finding{global local_id}`, `note` | – | verdicts |
| doc-writer | `artifact{page}` + `derived_from` | – | – |

### 7.7 Walk-through: one BoykoEngine-style campaign

Setup (orchestrator, main session, interactive):

```
SessionStart → moirai brief (rev 800): CRITICAL RULES(4) · OWNER QUESTIONS(1: #9) · IN FLIGHT(2) · READY(5/12) · merge queue l10 → l5np
moirai add campaign "Lease reclaim" --field owner_order=@order.txt          → #700
moirai add task "Design lease reclaim" --parent #700 --field task_kind=design → #701
moirai lane open --worktree <lanes-dir>/lease --branch u/lease --base 49f2fcfb      → lane #702
```

Research → design → critique loop (Workflow script; architect and critic have no Bash, so they use MCP; the orchestrator claims and persists):

```
researcher (MCP): remember{kind:note, about:[#701], text:…, fields:{sources:[…]}}     → #703..#706
architect  (MCP): pack{target:#701, role:architect}  → research notes, prior decisions, frozen alternatives with reopen_if
           write{ops:[create plan #P, create plan_section ×9 (heading, ordinal), create decision ×3 with alternatives rejected_for, create task ×5 (impl steps, parent #700, blocks chain)]}
critic r1  (MCP): pack{target:#P, role:architecture-critic}  → all sections (round 0)
           remember{kind:finding, about:[#P/§4], fields:{local_id:"C1", severity:blocker, failure:"…", confidence:plausible}} ×3
           remember{kind:verdict, about:[#P], fields:{role:critic, round:1, raw_label:"CHANGES REQUESTED", outcome:fail_fixable, counts:{critical:1,important:2}}}
refuters   (parallel, MCP): remember{kind:refutation, about:[#C1], fields:{outcome:refuted, evidence:"grep … shows guard exists"}}  → C1.status=refuted (I6)
orchestrator: moirai stats loop #P   → "round 1: raised 3, confirmed 2, refuted 1 (33%); confirmed blockers: 1 → continue"
architect r2 (MCP): pack{target:#P, role:architect, since_round:1} → confirmed findings only + the sections they are about
           write{ops:[set plan_section #s4 body=… (rev++ , changed_in_round=2), link #s4 --depends-on #s7]}
critic r2  (MCP): pack{…, since_round:1} → §4 (changed) + §7 (dependent) at L2, others "unchanged since r1"
           …  verdict{round:2, outcome:pass}  → moirai stats loop #P → "confirmed blockers: 0 → DESIGN APPROVED"; #701 phase_state=design_approved
```

Implementation in worktrees (Workflow: dispatcher claims, agents read, orchestrator persists):

```
orchestrator: moirai ready --scope #700 --role developer --ids            → #710 #711 #712
              moirai lane conflicts #710 #711 #712                          → "owns overlap: none"
              moirai claim #710 #711 #712 --agent wf:r7/dev#{1..3} --ttl 60m --json  → leases L-20..22
              Workflow(args={run:r7, tasks:[{id:#710, lease:L-20}, …]})
dev#1 (SubagentStart injects role rules; Bash): moirai pack #710 --role developer --budget 12k
              … edits …  moirai add deviation "used SmallVec instead of Vec" --about #710 --field why=…
              returns schema {files, conformance, deviations, unsafe}
tester ∥ reviewer: moirai pack #710 --role tester → Metrics & validation, expected counts, baselines(env)
              tester: moirai add testrun --field ran=51,passed=51,failed=0 --field git_sha=… ; moirai add measurement --field metric=claim_p50_us,value=41,unit=us,target=50 --env host=…,profile=release,load=quiet,scale=1e5 --field git_sha=…
              reviewer: moirai finding "…" --about #710 --severity important --field finding_kind=perf,failure=…; moirai verdict --about #710 --field role=reviewer,round=1,raw_label=CHANGES_REQUESTED
orchestrator: moirai apply results.json --idempotency-key run:r7      (one atomic, replay-safe batch)
              moirai find kind:finding about:#710 status:confirmed severity:>=important  → 1 → fix round; retest ≠ re-review (I11 blocks `fixed` until the reviewer verifies)
analyst:      moirai find kind:measurement about:#700 → targets vs actual computed; moirai verdict --field role=analyst,raw_label=ACCEPTED,outcome=pass
```

Merge and freeze:

```
orchestrator: moirai lane merge-check #702 → gates green, pins moved (declared), 0 open confirmed findings, rulings postdating base: #9 answered
              git merge … ; moirai reconcile --lane #702 → 4 proposed notes promoted, #710..#712 done-on-trunk, merge event c9xx
              moirai set #alt-3 --status frozen --field git_tag=…,reopen_if="if L1 cache > 32 MB"
              SessionStart(compact) → brief again; MEMORY.md top block regenerated by `moirai export memory-md`
```

What changed versus today: no `HDR` strings, no `.slice()`, findings have ids across rounds, the loop terminates by a query, rulings are typed and visible to every lane at once, the resume block is generated.

---

## 8. Performance and RAM budget (est., warm page cache, Ryzen 9 5900HS / consumer NVMe / NTFS / Defender on; process spawn 20–74 ms excluded)

Derivation inputs: per-node hot set ≈ 100 B (56 B header + 8 B CSR offsets + ~35 B CSR entries at 3.5 edges/node + bitmaps) [05 §10.4 gives 80–90 B at 3 edges]; titles 60 B; abstracts 150 B on 50 % of nodes; bodies on 40 % of nodes, 2 KiB raw, ÷ 3 with dictionary; history 10 ops/node × 60 B; flush 1.73–1.97 ms [05 §2.2]; open+map 0.22 ms [05 §2.3]; overlay ≤ 4,096 ops.

| Quantity | 1e4 nodes | 1e5 nodes | 1e6 nodes | How derived |
|---|---|---|---|---|
| Hot set (headers+CSR+bitmaps), shared page cache | 1 MB | 10 MB | 100 MB | 100 B/node |
| Full current snapshot on disk | 3 MB | 30 MB | 300 MB | + titles/abstracts/fields/uid ≈ 200 B/node + bodies 0.27 KB/node |
| Bodies (inside the above) | 1.1 MB | 11 MB | 110 MB | 40 % × 2 KiB ÷ 3 |
| History (log, uncompressed / cold-packed) | 6 / 2 MB | 60 / 20 MB | 600 / 200 MB | 10 ops × 60 B; ×3 zstd |
| Private RSS: CLI | 1.5–3 MB | 2–4 MB | 3–6 MB | arena + overlay (≤ 4,096 ops ≈ 0.5 MB) + std; [05 §16.1] |
| Private RSS: MCP server (follower) | 3–6 MB | 4–8 MB | 6–12 MB | + cached mappings, per-request arenas, rmcp/tokio ≈ 2–3 MB |
| Private RSS: leader (M3) | 4–8 MB | 6–12 MB | 10–20 MB | + group-commit buffers (≤ 1 MB) + broadcast queue |
| Open store | 0.5–1 ms | 0.5–1.5 ms | 0.5–3 ms | HEAD read 0.17 + 1–3 maps × 0.22 + tail replay ≤ 3 ms |
| `get #N` | 1–5 µs | 1–5 µs | 1–5 µs | column reads + overlay lookup |
| `ready` (list 20) | 5–20 µs | 20–100 µs | 0.1–1 ms | bitmap AND + per-candidate ancestor check O(depth) |
| `blocking --ids` | 5–20 µs | 20–100 µs | 0.2–2 ms | `is_blocker` bitmap scan; printing dominates |
| `blockers #T --transitive` | 5–50 µs | 10–100 µs | 10 µs–1 ms | reverse DFS, visited bitmap, budgeted |
| `pack` (12k tokens) | 0.5–2 ms | 1–5 ms | 2–10 ms | ~50–200 candidate nodes, ~40 bodies decompressed (0.2 µs each) + `applies_to` set intersections |
| `brief` (8 KB) | 0.5–2 ms | 1–3 ms | 2–8 ms | fixed classes, bitmaps |
| Durable commit (embedded) | ~2 ms | ~2 ms | ~2 ms | one data-sync flush; validation µs |
| Group commit, 16 concurrent writers (M3) | 3–4 ms for the batch | same | same | 64 pages + 1 flush = 3.05 ms |
| Checkpoint (synchronous, in the writer) | 10–30 ms | 0.1–0.3 s | delta 10–100 ms; full rollup 1–3 s (M6) | sequential write of the snapshot at 1–2 GB/s + serialization [05 §7] |
| Whole-CLI call from the agent Bash tool | 115–190 ms | 115–190 ms | 120–200 ms | Git-Bash ~109 + spawn 20–73 + engine ≤ 5 ms [05 §14.1] |
| MCP tool call (in-session) | 0.1–5 ms | 0.1–5 ms | 0.2–10 ms | no spawn; pipe 11–60 µs if forwarded |
| Cold cache (after reboot) | +0.3–0.6 ms per point query | same | +1–5 ms for `ready` | ~50–100 µs per 4 KiB page touched (extrapolated, not measured) |

The owner's realistic scale (0.3–0.5 M nodes after three years [02 §12.6]) sits between the 1e5 and 1e6 columns; the RAM target "single-digit MB private per process" holds across all three because data is shared through read-only mappings.

CI budget gates (M1 onward): engine time ≤ 5 ms per CLI command at 1e5; private bytes ≤ 4 MB CLI, ≤ 10 MB MCP at 1e5; exactly 1 flush per durable commit; zero O(N) work on open; zero CPU when idle (verified by a 60 s idle sample of the MCP server).

---

## 9. Build plan

Relative sizes: S ≈ 1 week, M ≈ 2–3 weeks, L ≈ 4–6 weeks of one focused developer with agent assistance (est.). Total to v1 (M0–M3) ≈ 3–4 months; M4–M7 another 3–4 months.

| Milestone | Content | Exit criteria | Tests | Size |
|---|---|---|---|---|
| **M0 Bench & oracles** | workload generator (1e3–1e6, real body sizes from the owner's notes), `hyperfine` harness, private-bytes/working-set/flush counters; the moirai workload on redb 4.x, heed and SQLite as oracles | numbers for flush, open, get, ready on the owner's box, Defender on, idle and under load; zstd-dictionary ratio on real notes measured | – | S |
| **M1 Core store** | HEAD/LOCK/log/snapshot format; writer/reader protocol; commit records with before-images; snapshot builder; overlay; header columns; CSR; bitmaps; symbol table; body blobs; tombstone view; `next_id`; idempotency table; `doctor --verify/--repair` | all budget gates pass at 1e5; 16-process Windows kill-loop (TerminateProcess at random points, 10k iterations) never corrupts and never loses an acknowledged durable commit; deterministic multi-process simulation (in-process scheduler over a fake FS with crash + fsync-error injection at every I/O boundary) passes 1e6 steps; fuzzed log/snapshot parsers | property tests: reverse == inverse(forward); snapshot ⊕ tail == replay-from-scratch; ALICE-style crash-state enumeration; AV-interference test (a process opening files without `FILE_SHARE_DELETE`) | L |
| **M2 Graph semantics + CLI** | node/edge kinds, status machines, guards, leases with fencing, derived state (open_blockers, ready, is_blocker, rollups, suspect), Pearce–Kelly, per-edge delete policies, `mentions` parsing, role write policy, all read/write verbs, `apply`, `log/diff/blame/as-of/undo`, output contract, exit codes | incremental derived state == full recompute under 1e6 random op sequences (property test); every CLI verb documented with an example; `blocking --ids` ≤ 100 µs at 1e5 | property tests vs recompute; golden-output tests; status-lattice tests; delete-policy matrix | L |
| **M3 Agent surface** | `brief`, `pack` (algorithm §7.5), hooks (SessionStart/SubagentStart/SubagentStop/agent-launched/stamp/prompt), core skill + `moirai-orchestrate` + `moirai-report`, `moirai mcp` (rmcp, dual-era, 9 tools, HMAC ctx), opportunistic leader (pipe, group commit, broadcast), `quiet`, `export memory-md`, `lane open/conflicts/merge-check`, `reconcile` (without ancestry cache: explicit `--merged` flag) | one real campaign run on the owner's harness end to end (§7.7) with zero `HDR`/`.slice()`; pack precision@k ≥ 0.8 for rules and decisions on 20 replayed historic briefs (owner-judged); brief ≤ 8,000 chars; hook p50 ≤ 60 ms; leader failover ≤ 1 s; idle CPU 0 % over 60 s | pack budget tests (never over budget, never mid-item cut, drop footer exact); hook payload tests against Claude Code 2.1.28x; 5-minute experiment: do SubagentStart/Stop fire for Workflow `agent()`? [07 §8.6] | L |
| **M4 Staleness & push** | git ancestry cache, `stale` for measurements/artifacts/knowledge, `check #T` (pinned revs), `changes --for-agent`, `watch` plugin monitor, optional PostToolBatch delta, `stats loop/refuted-share`, `doctor agents/hooks`, `export rules` | retraction of a source marks all transitive dependents `suspect` within the same commit (test); `stale` detection matches `git merge-base` on 1,000 random pairs | property tests; harness tests | M |
| **M5 Branches & merge** | `exp/*` refs + overlays, typed 3-way merge, conflict values, violation records, `conflicts/resolve`, `--strict` | 10 branches × 1,000 ops with injected C1–C9 conflicts merge deterministically; post-merge invariants I1–I14 hold or are reported | merge fuzzing vs a reference model | M |
| **M6 Scale & search** | delta snapshots + rollups (1e6), FST + postings text index, cold-pack gc, schema-as-data strengthening migrations with explicit consent | budget table §8 holds at 1e6; checkpoint delta ≤ 100 ms | benchmarks; migration round-trip tests | M |
| **M7 Sync & polish** | `refs/moirai/data` publish/fetch, `uid`-based import with alias map, optional HTTP MCP mode, signed binaries, `winget` | cross-clone round trip without id collision; `push --mirror` doctor check | integration tests | M |

Adoption path (starts at M3): (1) run one new campaign with moirai only; (2) render the `MEMORY.md` resume block from `brief` and stop hand-writing it; (3) new open questions/rulings go to moirai, `OPEN-QUESTIONS.md` becomes a generated view (regenerated, never merged); (4) import the ~50 feedback-type memory rules and the dated tester/reviewer lessons as `rule`/`note` nodes with `authority` and `applies_to` (a one-off agent-assisted import reviewed by the owner); (5) leave the 254 memory files as a read-only archive linked by `artifact` nodes; migrate opportunistically.

---

## 10. Risks — the top five ways this design fails, with mitigations

| # | Failure | Why it is plausible | Mitigation |
|---|---|---|---|
| R1 | **The multi-process file protocol has a latent race** (lost acknowledged write, torn HEAD, stale-lock deadlock after a kill) that surfaces only under real agent load | SQLite's WAL-reset race hid for 16 years; Beads lost 7 of 8 closes under nested-worktree load [08 §3.1; 03 §2.7] | M1 exit gate: deterministic simulation with crash/fsync-error injection + a Windows 16-writer kill-loop; durable-before-ack; read-your-writes test across processes; no shared memory; `doctor --verify` in CI |
| R2 | **Packs are wrong, not just short**: the pack omits the one rule or ruling that mattered, and agents trust it more than the old hand-written brief | precision of retrieval is unmeasured; the owner's rule "no unverified premise in a brief" [01 §3] | drop footers always list ids; owner rulings and critical rules are never degraded below L1; precision@k gate on 20 replayed historic briefs; `check #T` before acting; keep the `HDR` fallback for one campaign |
| R3 | **Agents skip the protocol** (forget `complete`, self-declare a wrong `--agent`, write findings without `failure`) so the graph drifts from reality | LLM compliance is probabilistic; Beads needed `doctor` for exactly this [03 §2.10] | dispatcher pattern (orchestrator persists via `apply`), SubagentStop safety net, TTL leases, kind-specific validation (`failure` mandatory), stamped identity on MCP, I12 artifact checks |
| R4 | **The leader/pipe layer is unreliable on Windows** (squatted pipe, job-object kill, version skew), degrading to per-call flush and no push | Claude Code's own daemon pipe bug; BrowserSkill job-object kill [08 W9, W12] | the leader is an optimisation only; CLI always works direct; version handshake; per-user DACL; failover test; the owner may disable the leader outright |
| R5 | **Scope creep into a general graph DB** (query language, embeddings, many kinds, big schema) inflates the core and delays adoption | Beads grew to 225k LOC and 13 issue types by accretion [03 §2.1] | v1 = M0–M3 only; kinds/edges are a closed list with codes; extensions are data; no query language; embeddings out of the core; a size budget in CI (binary ≤ 10 MB, core crate ≤ 25k LOC est.) |

Secondary risks: Claude Code hook/MCP behaviour changes per release (pin and test per version); `structuredContent` quirk (text-first output); Git 3.0 SHA-256 (32-byte field already); Windows lock-release delay (bounded retry, holder reported); `zstd`'s C dependency in a "from scratch" build (feature-gated; owner decision D6).

---

## 11. Decisions the owner must make (value/scope calls only), with recommended defaults

| # | Decision | Recommended default |
|---|---|---|
| D1 | May read-only roles (architect, critic, researcher, project-analyst) write their own node kinds through MCP while keeping no file Write/Edit? | **Yes**, under the role write policy (§7.6); it removes "the plan does not exist" without breaking separation of duties. |
| D2 | Store location and sharing: one store per repository in `<git-common-dir>/moirai/` (shared by all worktrees), or per user? | **Per repository in the common dir**; a second repository of the owner's gets its own store. |
| D3 | May the moirai MCP server stay resident (0 % CPU, single-digit MB) during quiet benchmark windows, and may it act as leader? | **Yes** to both; `moirai quiet on` guarantees no background work. Owner may set `MOIRAI_LEADER=0`. |
| D4 | Bodies: tiered (≤ 16 KiB in moirai, larger as path+sha256), everything, or pointers only? | **Tiered** (T6). |
| D5 | Durability: lazy class for heartbeats/cursors (may lose the last write on power loss), or flush everything? | **Durability classes** (T8). |
| D6 | "From scratch" boundary: allow the small crates in T10 (incl. `zstd` with its C code), or zero non-std dependencies for the storage core? | **Allow the T10 list**; `zstd` feature-gated. |
| D7 | Identity stamping auto-approves `mcp__moirai__*` calls (`allow`) or prompts (`ask`)? | **`allow`** for reads and lease-scoped writes; the owner can switch to `ask`. |
| D8 | Language of stored text: English for repository-facing kinds (rules, decisions, findings, plan sections), Russian allowed in `brief` rendering and owner `verbatim`? | **English in nodes; `verbatim` kept as written; `brief --lang ru` optional.** |
| D9 | Owner rulings: only the main session may write `authority=owner`, always with a verbatim quote? | **Yes.** |
| D10 | Replace `OPEN-QUESTIONS.md`/`BACKLOG.md`/`MEASUREMENT-QUEUE.md` and the `MEMORY.md` resume block with generated views (regenerated, never merged), or keep them hand-maintained alongside? | **Generated views from M3**, starting with the resume block. |
| D11 | Migration: import the ~50 feedback-type memory rules and dated lessons as nodes now, and archive the 254 memory files as read-only `artifact` links? | **Yes** (agent-assisted import reviewed by the owner); no bulk import of campaign logs. |
| D12 | Cross-machine/cloud agents in v1? | **No**; `uid` is stored from day one so M7 sync needs no migration. |
| D13 | Lease TTL defaults (15 min self-claim, 60 min dispatcher claims) and immediate release when the holder's Claude process is dead? | **15 / 60 min; immediate release on dead PID.** |
| D14 | Commit policy encoded as a rule node: "commit only on explicit request" (CLAUDE.md, developer.md) vs an opposite rule in the owner's private notes? | **Owner picks; moirai stores one `rule` with `authority=owner` and marks the other `superseded`.** |
| D15 | Retention: keep the full op log forever (cold-packed), or squash after N days? | **Forever, cold-packed**; history is ≤ 100 MB/year (est.). |

---

*End of proposal C.*
