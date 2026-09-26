# 10 — Proposal A: "Lean Embedded" moirai

*Architecture proposal. Date: 2026-09-25. Status: design only, nothing implemented. Angle: optimise first for the owner's hard requirements (minimal RAM, maximum performance, zero idle CPU, no mandatory resident process, O(1) open, crash safety on NTFS with Defender), then make versioning and agent features complete in their leanest viable form.*

**How to read the evidence.** Citations are `[NN §section]` into the eight research reports (`docs/research/01..08`). Numbers carry the tag of the report they come from: **measured** (on the owner's machine, or a published measurement), **claimed** (vendor or third party), or **estimate** (my arithmetic; the inputs are given). Anything not tagged is a design decision.

---

## 1. Thesis

1. moirai is one static Rust binary that opens a small, immutable, memory-mapped store in about a millisecond; every process (CLI, MCP server, hook) is an equal embedded client, and nothing is resident when no agent runs.
2. History is an append-only, checksummed operation log (bytes per commit); current state is a set of sealed columnar segments plus a bounded in-process overlay built from the log tail, so open is O(segments + tail), never O(history) or O(nodes).
3. One writer at a time, elected by a byte-range lock on a dedicated `LOCK` file; readers never lock; a commit is one append plus one data-only flush (about 2 ms measured), which is enough for the owner's write rate without a leader or group commit.
4. Referential consistency is an engine invariant: a delete walks the reverse adjacency, applies per-edge-kind policies, writes tombstone and change record in the same commit; other processes see it on their next read (monotonic by sequence number) and agents see it at their next hook or tool result.
5. Coordination state lives on one shared trunk; knowledge carries git provenance; explicit branches are cheap delta overlays with typed 3-way merge and conflicts stored as data; agents reach all of it through a CLI plus skill plus hooks and a nine-tool MCP server.

The angle I optimised for is the owner's stated hard constraint set, taken literally: single-digit MB private memory per process, sub-5 ms engine time per command, zero background CPU, nothing to start or stop before a benchmark, and no data loss when a process is killed mid-write. Every feature below states its RAM or latency cost; features whose cost could not be justified were deferred to a later milestone rather than watered down.

---

## 2. Positions on the known forks (T1–T13)

| Fork | Position | Evidence | What would change my mind |
|---|---|---|---|
| **T1 Materialized state** | **Immutable read-only-mmap'd columnar segments (one base + up to 4 delta segments) plus an explicit-I/O log tail replayed into a bounded in-process overlay.** No copy-on-write B+tree in the hot path. | Mapped page touch ≈1 µs vs ≈7 µs for a stream read; open+map 0.22 ms; all measured [05 §2.3]. Windows cannot resize or truncate a mapped file and views are not guaranteed coherent with `WriteFile`, so mapped data must be immutable [05 §6.1–6.2]. A CoW B+tree is "the hardest to build from scratch" and costs depth × 4 KiB per retained version [05 §16 Option B; 04 §3.14]. mmap shares one physical copy across the 16 agent processes [05 §5.3]. | Measured overlay cost at 1e6 nodes under random-update bursts exceeding budget (open > 5 ms or point read > 50 µs after a 4,096-op tail), or a hard requirement for uniform O(depth) access to arbitrary historical versions. Fallback is Option B of [05 §16] behind the same lock protocol. |
| **T2 Process model** | **Purely embedded multi-process.** Writer byte on `LOCK`, lock-free readers, no leader, no daemon, no pipe. The MCP server is an ordinary embedded client that happens to live long. | Write volume is tiny: ~19 git commits/day, bursts of ≤16 agents [08 §2]; one writer at ~2 ms/flush serves ~400 commits/s, so group commit buys nothing [08 §1.2]. Push cannot reach the model except through hooks anyway [08 §6.2; 07 §2.3]. Daemons are the recurring failure in prior art (Beads removed ~24k LOC of daemon; Windows pipe bugs; job-object kills) [08 §5; 03 §8.2]. A shared page cache already gives the "warm cache" a leader would provide [05 §5.3]. Zero idle CPU and quiet mode follow trivially: no moirai process has threads or timers. | Measured writer-lock wait p99 > 50 ms with 16 concurrent writers on the owner's machine, or a harness feature that lets a server push into the model. Then add the opportunistic leader of [08 §9 C] as an optimisation: it would hold the same writer byte, so the file protocol does not change. |
| **T3 Branch model** | **Shared live trunk for everything coordination-like (tasks, status, claims, blockers, rules, rulings, findings, verdicts); knowledge carries git provenance and a `proposed` flag until `reconcile`; explicit `exp/*` what-if branches only, implemented as delta overlays that rebase on read.** No automatic moirai branch per worktree. | 44 worktrees, several detached, short-lived `wf_*` trees [08 §2, §7.2]; cross-lane blocking must be live today, not after a git merge [01 §7 L15; 02 §7.3]; the union-merge disasters were register text, which becomes typed trunk nodes here [01 §7 L5–L6]; Beads and Taskmaster both refused auto-following [08 §7.1]. | If the owner wants speculative per-lane task decomposition hidden from other lanes. Then `moirai lane open` creates an `exp/` branch for that lane; the overlay mechanism already exists. |
| **T4 IDs** | **Store-global sequential u32 `#N`, never reused, allocated under the writer lock, stored in `HEAD`, not versioned; plus a cold 64-bit random `uid` column for future import/sync.** | `#40` ≈ 2 tokens vs ≈ 24 for a UUID, 5–7 vs 29–68 agent errors [06 §9.1]; dense ids allow direct array and bitset indexing, the cheapest layout [05 §8, §10.4; 06 §9.2]; one store per repo makes the counter collision-free [04 §0.5; 06 §9.2]. `uid` costs 8 B/node in a column nobody touches (8 MB mapped at 1e6). | Cross-machine sync in v1. Then `uid` becomes identity, `#N` a per-store alias map, and `import` remaps edges [06 §9.2]. |
| **T5 Deletion** | **Hard delete in current state (row keeps only the `deleted` flag), a 24-byte tombstone record (id, tx, reason, replaced_by), and per-edge-kind policies: `parent` restrict (or `--cascade`/`--reparent`), `blocks` drop-and-notify, `answers`/`scoped_to` restrict, all historical kinds keep the edge and render a tombstone; `derived_from`/`cites` sources become `suspect`.** Trunk deletes are visible to all processes at once; branch merges surface delete-vs-modify and dangling-edge records. | [06 §6, §8.2] policy vocabulary (Gel, Datomic, TypeDB); Beads integrity failures from application-level cleanup [03 §2.10]; tombstones needed so merges can tell "deleted" from "never existed" [08 §6.3]; soft delete is unnecessary in a versioned store [06 §8.1]. | If the owner wants deletion refused whenever *any* referrer exists ("strike, never delete" taken literally). Then the default becomes restrict for every structural kind and `rm` requires `--force`. |
| **T6 Bodies** | **Tiered:** title and a 200-char abstract always in the store; bodies ≤ 64 KiB stored inline as zstd frames with a per-store dictionary; larger bodies stored as artifact pointers (path, sha256, bytes, kind) plus a 2 KiB excerpt, with `--inline` to override. | Result payloads p50 8.8 KB, p90 38 KB, max 153 KB [02 §5.1]; scratchpads are session-scoped and reach 7.8 GB, so pointers into them rot [02 §8.3]; owner's disk is starved by build caches, not knowledge [02 §9]; dictionary compression of ~1 KB records is claimed ~3× better than plain zstd, unmeasured [05 §12]. Estimate: 30–60k artifacts/year × ~4 KB compressed ≈ 120–240 MB/year. | Measured dictionary ratio on real notes under 2×, or the owner wanting the store fully self-contained (raise the cap, add a content-addressed blob segment). |
| **T7 Merge semantics** | **Typed per-field 3-way rules; conflicts as data; the merge always produces a commit; violation records for dangling edges and cycles; tasks carrying a conflict are excluded from `ready`; post-merge full validators (forest, Kahn on the combined precedence graph, reverse-index equality).** Status fields merge on a forward-only lattice; counters are never merged (recomputed); sets are add-wins; text is line diff3 with overlapping hunks as a conflict value. Claims and leases are trunk-only and never merged. | Conflict taxonomy and algorithm [04 §5, §9 A]; jj conflicts-as-values and Dolt violation tables [04 §3.8, §3.2]; merged acyclic graphs can form cycles [06 §7.3]; the owner's register history demands a status lattice and recomputed counters [01 §8.3 item 3; 02 §7.3]. | Owner preference for strict merges (refuse to commit while a class of conflict is open). The engine supports `--strict`; only the default flips. |
| **T8 Durability** | **One data-only flush per commit (`NtFlushBuffersFileEx`, 1.73 ms measured), 1PC+C: the commit record is the truth, `HEAD` is a cache written without a flush.** Two durability classes: `durable` (every graph mutation, claim, delete) and `lazy` (heartbeats, read cursors, stamps) appended without a flush. `apply` batches N ops into one commit, so bulk writes cost one flush. No group commit across processes. | Flush costs [05 §2.2]; write-through is not trusted [05 §6.2]; redb 1PC+C pattern [05 §4.3]; low volume [08 §1.2]. | A sustained commit rate above ~100/s from many processes. Then the leader of T2 adds group commit. |
| **T9 Agent surface** | **CLI + skill + hooks first; a nine-tool MCP server second (needed by the three roles without Bash and for per-role write policy).** Hooks: SessionStart (brief ≤ 8k chars), SubagentStart (role pack), SubagentStop (lease safety net), UserPromptSubmit (delta since cursor), PreToolUse(`mcp__moirai__.*`) stamp. Leases with fencing tokens, CAS guards, idempotency keys with 7-day retention. | Role tool envelopes [07 §5.4]; hook capabilities and 10,000-char cap [07 §4.1–4.2]; PowerShell 5.1 mangles argv, so bodies travel on stdin or as MCP JSON [07 §5.2]; Beads 1.3 converged on leases, guards, exit codes [07 §7.4]; Workflow resume re-runs completed agents, so idempotency is mandatory [07 §4.1]. | Confirmation that channels reach GA and deliver into idle sessions; then a `watch` push path is worth its cost. |
| **T10 From-scratch boundary** | **Hand-written: file format, log, segments, columns, CSR, bitsets, term dictionary, lock protocol, overlay, Pearce–Kelly, merge engine, CLI parsing.** Allowed crates: `zerocopy` (validation-free views), `blake3`, `xxhash-rust` (xxh3), `zstd` (codec only), `windows-sys`/`libc` (bindings), `serde_json` (JSON I/O), and in the separate `moirai-mcp` crate only: `rmcp` + `tokio`. Dev-only: a property-test crate and `hyperfine`. No `memmap2`, `roaring`, `fst`, `petgraph`, `sled`, `redb`. | Validated zero-copy costs as much as deserialising; `zerocopy` avoids it [05 §9.1]; pure-Rust `roaring` copies on deserialize and `fst` is unmaintained since 2021 [05 §10.3, §13]; the storage-engine crates that matter are experimental or beta in 2026 [04 §11]; "keep the core small and owned" [03 §8.2]. | If the owner accepts a C dependency for a proven multi-process engine (LMDB via `heed`) as the materialized state; then M0 shrinks by roughly a third and T1 becomes Option B with `heed`. |
| **T11 Search** | **Typed filters + graph expansion are the primary retrieval; FTS tier 1 (brute-force scan of titles and abstracts, opt-in bodies) at any scale; tier 2 (term dictionary + postings built at rollup) switches on above 20k nodes; embeddings out of v1 and out of the core (optional external process later).** | Brute force is milliseconds at 1e4; FST + postings is 0.1–5 ms at 1e5–1e6; tantivy costs ≥ 15 MB per thread and many files [05 §13]; lexical + structural retrieval is competitive with vector memory for agents [03 §6.3]. | An owner use case needing semantic recall over free text at scale, with an accepted RAM budget for a local model. |
| **T12 Schema** | **Fixed core kinds and header fields compiled in; kind-specific fields declared in a schema-as-data registry versioned in the store; project extensions allowed as weakening changes (add field, add enum value, add kind) applied instantly; strengthening changes need `moirai migrate`.** v1 kinds: `task, doc, note, rule, decision, question, finding, verdict, measurement, artifact, run, lane, area` (13). | [06 §4] weakening/strengthening; [01 §6, §8] and [02 §12.1] node kinds the owner's roles produce; enum integers never reused [06 §4]. | Owner wants kinds hard-coded only (simpler, no registry) or fully schemaless (rejected: typos, no invariants) [06 §3.1]. |
| **T13 Scope of v1** | **Adoptable after M2:** engine + graph core + CLI + skill + SessionStart/SubagentStart/Stop hooks + claims + `pack`/`brief` + `apply` with idempotency, on a shared trunk. MCP (M3), versioning surface (M4), FTS tier 2 / reconcile / exports (M5) follow. | The hot queries the owner needs first are `blockers`, `ready`, `pack`, `brief`, findings across rounds [01 §8.3; 02 §12.5]; those need no branches or FTS. | If the read-only roles must write from day one (then M3 moves before M2's hooks). |

---

## 3. Data model

### 3.1 Node header (hot, columnar, one row per id)

Every node has a fixed 40-byte header stored as structure-of-arrays columns in segments (little-endian, `zerocopy` views). Rows in the base segment are id-dense (`row = id − 1`); deleted ids keep a row with only the `deleted` flag set.

| Column | Type | Meaning |
|---|---|---|
| `kind` | u8 | one of the 13 core kinds or a project kind (≥ 64) |
| `status` | u8 | kind-specific enum (see 3.6); `blocked` is never stored |
| `resolution` | u8 | for closed tasks: completed, wontdo, duplicate, superseded, obsolete; 0 otherwise |
| `priority` | u8 | P0–P4 (scheduling) |
| `criticality` | u8 | critical / high / normal / low (surfacing) |
| `confidence` | u8 | verified / observed / inferred / speculative / n-a |
| `flags` | u16 | bit0 deleted, bit1 suspect, bit2 pinned, bit3 claimed, bit4 has_dangling, bit5 container, bit6 proposed, bit7 conflict, bit8 archived |
| `rev` | u32 | increments on every change to this node (CAS guard) |
| `parent` | u32 | 0 = none |
| `created_tx`, `updated_tx` | u32, u32 | transaction numbers (provenance is on the commit) |
| `title_off` | u32 | offset into the segment's title blob (length-prefixed UTF-8, ≤ 200 B) |
| `body_off` | u32 | offset into the body blob (0 = none) |
| `fields_off` | u32 | offset into the tagged-varint field block |
| `open_blockers` | u16 | derived: incoming `blocks` edges whose source is not done |
| `open_blockers_exo` | u16 | derived: those blockers that are outside this node's own subtree |
| `children_open` | u16 | derived |
| `children_total` | u16 | derived |

Cold columns (separate arrays, touched only by the queries that need them): `uid: u64`, `topo: u32` (Pearce–Kelly order in the precedence graph), `last_lsn: u64` (head of the node's op chain in the log), `defer_until: u32`, `due: u32`.

Typed fields not in the header are stored in the field block as `(field_sym varint, type u8, value)`; booleans carry no value bytes; integers are zigzag varints; strings are either interned symbols or length-prefixed bytes; node references are u32 ids. `done` is not a stored boolean: it is derived from `status` (`done ⇔ status ∈ {done, cancelled}` for tasks, `answered` for questions, `accepted` for verdicts) and exposed as a virtual field so agents can filter `done:false`.

### 3.2 Node kinds shipped in v1

| Kind | Purpose | Kind-specific typed fields (beyond the header) | Status set |
|---|---|---|---|
| `task` | unit of work; subtasks are tasks with `parent` | `assignee` sym, `role` enum, `acceptance` text, `labels` sym-set, `touches` glob-set, `estimate` u16 | open, in_progress, done, cancelled, deferred |
| `doc` | plan, plan section, report body; sections are docs with `parent` | `doc_kind` enum (plan, section, report, patch), `revision` u16, `word_count` u32 | draft, current, superseded, archived |
| `note` | descriptive knowledge, gotcha, hazard | `note_kind` enum (note, hazard, lesson, checkpoint), `observed_git_sha` [32]B, `applies_to` glob-set | active, superseded, retracted, archived |
| `rule` | normative ("never X") | `enforcement` enum (must, should), `applies_to_roles` enum-set, `applies_to` glob-set, `authority` enum (owner, orchestrator, measured, research), `owner_quote` text | active, superseded, retracted, archived |
| `decision` | ADR-like; never edited after acceptance, only superseded | `context` text, `tradeoff` text, `authority` enum, `alternatives` text-list (rejected options with reasons) | proposed, accepted, rejected, superseded |
| `question` | owner or orchestrator question | `question_kind` enum (values, scope, unclear), `asked_of` enum, `options` text-list, `ruling` text | open, answered, dropped |
| `finding` | review or critique remark, measurement observation, bug root cause | `severity` enum (blocker, important, optional), `local_id` sym (C1, W2), `failure_scenario` text, `evidence` text, `finding_kind` enum (correctness, perf, complexity, security, design, doc), `round` u16 | open, confirmed, refuted, deferred, fixed, withdrawn |
| `verdict` | gate result by a role | `raw_label` sym (APPROVED …), `outcome` enum (pass, pass_with_conditions, fail_fixable, fail_fundamental, unknown, n_a), `return_to` enum, `round` u16, `criteria` text | open, accepted, superseded |
| `measurement` | number with its environment | `metric` sym, `value` f64, `unit` sym, `target` f64, `command` text, `git_sha` [32]B, `host` sym, `load` enum (quiet, loaded), `scale` sym | current, moved, stale |
| `artifact` | pointer to a file outside the store | `path` text, `sha256` [32]B, `bytes` u64, `artifact_kind` enum, `excerpt` text | present, missing |
| `run` | a Workflow run or agent call | `wf_id` sym, `bg_task_id` sym, `session_id` sym, `script_path` text, `journal_path` text, `expected_artifacts` u8 | running, green, red, stopped, died |
| `lane` | a worktree + branch + base | `worktree` text, `branch` sym, `base_sha` [32]B, `tip_sha` [32]B, `target_dir` text | active, ready_to_merge, merged, frozen, abandoned |
| `area` | scope node (subsystem, directory, topic) | `path_globs` glob-set | active, archived |

Campaign, phase, rung, candidate and checkpoint from [02 §12.1] are `task` (with `labels`), `task`, `task`, `decision` with status rejected plus a `revive_condition` field, and `note` with `note_kind=checkpoint` respectively. Fewer kinds keep the bitset set small.

### 3.3 Edge kinds

Edges are binary, typed, keyed by `(src, kind, dst)` with set semantics, stored in both directions. An optional 8-byte property record `(created_tx u32, pinned_rev u32)` exists only for `cites`.

| Edge (src → dst) | Class | Acyclicity | On dst deleted | On src deleted |
|---|---|---|---|---|
| `parent` (child → parent) | structural | forest, depth ≤ 12 | **restrict**; `--cascade` deletes subtree; `--reparent` moves children up | drop, update parent rollups |
| `blocks` (A → B) | structural | part of the precedence DAG `blocks ∪ child→parent` (Pearce–Kelly) | **drop and notify**: B's counters decrement, B listed in the change record | drop |
| `duplicate_of` | structural | chain length 1 | restrict or repoint to canonical | drop |
| `answers` (note/decision → question) | structural | n/a | restrict | drop; question reopens |
| `scoped_to` (knowledge → area) | structural | n/a | restrict or reassign to parent area | drop |
| `runs_in` (run → lane) | structural | n/a | restrict | drop |
| `produced` / `consumed` (run → doc/artifact) | historical | n/a | tombstone | keep |
| `supersedes` (new → old) | historical | acyclic | tombstone | keep; old stays superseded |
| `derived_from` (summary → source) | historical | acyclic by construction | tombstone; **src marked suspect** | keep |
| `cites` (any → knowledge, `pinned_rev`) | historical | none | tombstone; **src marked suspect** | keep |
| `depends_on` (doc section → section) | historical | acyclic | tombstone | keep |
| `discovered_from` (new → task) | historical | acyclic by construction | tombstone | keep |
| `relates` | historical | none | tombstone | keep |
| `refutes` / `confirms` / `verifies` / `addresses` / `implements` (finding, measurement, verdict, run → finding/decision/rule/task) | historical | none | tombstone | keep |
| `mentions` (any → any, parsed from `#N` in title/body at write time) | historical | none | tombstone; renders `#40 (deleted c812 …)` | recomputed from text |

A verdict gates work through a `blocks` edge from the verdict node to the task: an open verdict with outcome `fail_*` is "not done" and therefore blocks; setting it `accepted` (or superseding it) unblocks in the same commit. That makes "no open blocking review" a plain readiness rule rather than a special case.

### 3.4 Invariants (checked on every write, re-checked after merge and by `doctor --verify`)

| ID | Invariant |
|---|---|
| I1 | Ids are unique across the store, allocated under the writer lock, never reused. |
| I2 | Every structural edge has live endpoints in the current state. |
| I3 | Historical edges may reference deleted ids; those ids resolve through the tombstone table. |
| I4 | `parent` is a forest with depth ≤ 12. |
| I5 | `blocks ∪ child→parent` is acyclic; no node blocks its own descendant; blockers are inherited from outside the subtree only. |
| I6 | `supersedes(new, old)` implies `old.status = superseded` in the same commit; `answers` implies `question.status = answered`. |
| I7 | Status transitions follow the kind's machine; `blocked` and `done` are never stored. |
| I8 | Derived counters, `topo`, bitsets and the reverse CSR equal a full recomputation. |
| I9 | Every mutation belongs to one commit that carries provenance (actor, role, session, git head, worktree). |
| I10 | A merge result satisfies I1–I9 or carries explicit conflict and violation records; nodes with a conflict flag are never ready. |
| I11 | The change log sequence is contiguous and every acknowledged commit is durable. |

### 3.5 Derived state (each predicate defined once, in the engine)

| Value | Definition | Maintenance | Cost |
|---|---|---|---|
| `open_blockers[n]` | count of `blocks` in-edges whose source is not done | ±1 on source status change or edge add/remove | O(out-degree) |
| `open_blockers_exo[n]` | as above, excluding sources inside n's subtree | decided at edge-add time by walking the source's ancestors (O(depth)) | O(depth) |
| **ready** bitset | `kind=task ∧ status=open ∧ ¬deleted ∧ ¬conflict ∧ children_open=0 ∧ open_blockers=0 ∧ ¬live_lease ∧ defer_until ≤ now ∧ no ancestor with open_blockers_exo>0` | membership recomputed for touched nodes only; the ancestor clause is checked at query time by walking ≤ 12 parents per candidate | O(touched + depth) |
| `is_blocker` bitset | not done ∧ has an outgoing `blocks` edge to a not-done node | on edge/status change | O(out-degree) |
| `children_open`, `children_total` | direct children | on child status change or reparent, along the ancestor chain | O(depth) |
| `ready_to_close` | container with `children_open = 0` | from rollups | O(1) |
| `suspect` | a `derived_from`/`cites` target was retracted, superseded, deleted, or moved past `pinned_rev` | propagate along reverse `derived_from`/`cites` closure | O(closure) |
| `stale` (measurement, note) | `git_sha`/`observed_git_sha` is behind the lane tip and `applies_to` files changed (checked at `reconcile`/`stale`, not on the hot path) | on demand | O(k) |
| critical path | longest path over open tasks in the precedence DAG, restricted to a subtree | on demand, DP over `topo` order | O(V+E) of subtree |
| `topo` | Pearce–Kelly dynamic topological order | affected region only on edge add; full Kahn after merge or bulk import | small |

Property tests assert I8 after random op sequences, including deletes and merges.

### 3.6 Status machines (guarded transitions)

- task: `open → in_progress → done`; `open|in_progress → cancelled|deferred`; `deferred → open`; `done → open` only through `reopen` (explicit event, never a merge artefact). Lattice for merge: `open < in_progress < done`, `cancelled` and `deferred` are conflicts against `done`.
- finding: `open → confirmed|refuted|deferred|withdrawn`; `confirmed → fixed`; `fixed` requires an `addresses` edge from a different actor than the raiser and, for `finding_kind ∈ {perf, complexity}`, a `verifies` edge whose source is a review or measurement, not a test run alone (the "retest is not re-review" rule [01 §3]).
- question: `open → answered` (with an `answers` edge and a ruling) or `dropped`; reopen explicit.
- verdict: `open → accepted|superseded`.
- knowledge (note, rule, decision): `active|accepted → superseded` only together with a `supersedes` edge; `retracted` with a reason.

---

## 4. Storage engine

### 4.1 Files

All files live in `<git-common-dir>/moirai/` (found by reading the `.git` file and `commondir` without spawning git [07 §6.2]); `MOIRAI_DIR` overrides. Network drives and OneDrive-managed paths are refused [08 §4 W11].

| File | Size | Mapped? | Role |
|---|---|---|---|
| `LOCK` | 0 B data; lock bytes 0 (writer), 1 (compactor); holder info at offset 4096 (unlocked) | no | election only; never holds data |
| `HEAD` | 8 KiB = 2 × 4 KiB slots | no (explicit 4 KiB reads) | cache of the current sequence, segment set, refs, counters |
| `log.NNN` | preallocated in 16 MiB extents up to 64 MiB, then rotate | no (explicit I/O) | canonical history: commits, lazy records, checkpoint markers |
| `seg.base.G` | ≈ 90–100 B/node + titles + bodies | yes, read-only | full materialized state at generation G |
| `seg.delta.G.K` | ≈ 60 B per touched node + new bodies | yes, read-only | touched rows since the previous checkpoint (≤ 4 at a time) |
| `dict.D` | 32–110 KiB | yes | zstd dictionary D (retrained at rollup) |

Typical file count: 6–10. History grows one `log` file per 64 MiB (~150k commits at 0.4 KB, estimate). Defender charges per file open [05 §6.4], so there is no per-node or per-commit file and no create/delete churn on the commit path.

### 4.2 HEAD slot (4,096 B)

```
off  size  field
0    8     magic "MOIRAI\0\1"
8    2     format_version (readers refuse newer)
10   2     flags: bit0 quiet, bit1 fts_tier2
12   4     seq_low (u64 seq at 12..20)
20   8     committed_lsn        (last durable commit record end)
28   8     checkpoint_lsn       (first log record not folded into a segment)
36   4     log_file             (NNN of the active log)
40   4     next_id              (u32 counter, non-versioned)
44   4     next_tx
48   8     next_token           (fencing tokens)
56   4     dict_id
60   2     seg_count            (≤ 8)
62   2     ref_count            (≤ 64; overflow spills to a refs table in the base segment)
64   8×12  segs[8]              {id u32, kind u8, gen u32, pad}  = 96 B
160  64×44 refs[64]             {name_sym u32, commit_id [32], seq u64} = 2,816 B
2976 32×16 seq_ring[32]         {seq u64, lsn u64} recent commits for cheap `changes --since`
3488 592   reserved
4080 16    xxh3-128 over bytes 0..4080
```

Writers alternate slots. Readers take the slot with the valid checksum and the higher `seq`. The slot is written after the commit flush and is **not** flushed itself: if it is stale, open scans the log forward from `committed_lsn` and reconstructs it from commit records (each commit record carries the post-commit counters). This is the redb "1PC+C" idea applied to a separate cache file [05 §4.3].

### 4.3 Log records

```
RecordHeader (16 B): magic u8 (0xA7) | kind u8 | flags u8 | pad u8 | len u32 | lsn u64
payload (len bytes)
Trailer (8 B): xxh3-64 over header+payload
```

Kinds: `1 Commit` (durable), `2 Lazy` (heartbeat, cursor, stamp; appended, not flushed), `3 Checkpoint` (segment set changed), `4 Pad` (fills to the extent boundary).

Commit payload:

```
seq u64 | tx u32 | commit_id [32] (BLAKE3 of everything after it)
parent_count u8 | parents [32]×n
branch u16 | actor_sym u32 | role u8 | session_sym u32
ts_ms u64
git_head [32] | git_algo u8 | git_branch_sym u32 | worktree_sym u32
idem_key [16] (xxh3-128 of the caller's key, or zero)
counters_after: next_id u32 | next_tx u32 | next_token u64
msg: varint len + bytes
op_count u32
ops...
```

Ops (tag u8 + varints; every op that changes a value carries its before-image, SQLite-changeset style [04 §3.13]):

| Op | Fields | Typical size (estimate) |
|---|---|---|
| `Create` | id, kind, title, fields block | 80–200 B |
| `SetField` | id, field_sym, old, new | 8–40 B |
| `SetStatus` | id, old, new, resolution | 6–9 B |
| `AddEdge` / `RemoveEdge` | src, kind, dst, (props) | 8–12 B |
| `Move` | id, old_parent, new_parent | 8–12 B |
| `Delete` | id, reason_sym, replaced_by, before-image (header + edge list) | 40 B + 5 B/edge |
| `Body` | id, hash [16], zstd frame or artifact pointer | body size |
| `Claim` / `Release` / `Complete` | id, holder_sym, token u64, expires_at u32 | 16–24 B |
| `Ref` | name_sym, old_commit, new_commit | 70 B |
| `Schema` | weakening change record | 20–100 B |

A small commit is ≈ 200 B of header plus 20–100 B of ops, i.e. 0.3–0.6 KB, which matches the log-store family costed in [04 §6]. Each op also stores `prev_lsn` for its node (varint), so a node's history is a backward chain of preads.

### 4.4 Segment layout

A segment is one file: a 64-byte header, a table of column descriptors, the columns, and a footer.

```
Header (64 B): magic | format | kind (base/delta) | gen | first_id | row_count | edge_count | dict_id | created_seq | column_count
Column descriptors (32 B each): name_sym | offset u64 | bytes u64 | element_size u8 | xxh3-64
Columns (8-byte aligned):
  ids            [u32; rows]         (delta only; base is id-dense)
  header columns  as in §3.1 (each its own array)
  cold columns    uid, topo, last_lsn, defer_until, due
  fwd_off        [u32; rows+1]  fwd_dst [u32; E]  fwd_kind [u8; E]
  rev_off        [u32; rows+1]  rev_dst [u32; E]  rev_kind [u8; E]
  edge_props     sorted (src,kind,dst) → props for `cites`
  bitsets        one frozen bitset per kind, per status value, ready, is_blocker, suspect, deleted, claimed, proposed
  symbols        sorted string table (u32 sym → offset)
  titles         blob of varint-len UTF-8
  fields         blob of tagged-varint field blocks
  bodies         blob of [u32 len][zstd frame]
  tombstones     sorted {id u32, tx u32, reason_sym u32, replaced_by u32, pad} (16 B)
  commits        {seq u64, lsn u64, log_file u32, commit_id_prefix u64} (28 B per commit; base only)
  idem           sorted {key [16], seq u64, result_hash u64} (32 B), entries younger than 7 days
  leases         {id u32, holder_sym u32, token u64, expires u32} (24 B per live lease)
  terms/postings tier-2 FTS (base only, optional)
Footer: BLAKE3-256 of the logical content, format, column table offset, xxh3-64 of the footer
```

A **frozen bitset** is a hand-written two-container structure: per 65,536-id chunk either a sorted u16 array (≤ 4,096 members, 2 B each) or an 8 KiB bitmap, with a 16-byte chunk index; it is queried in place from the mapping. Worst case at 1e6 ids: 128 KiB per set [05 §10.3].

**Delta segments** contain only rows for ids touched since the previous checkpoint (sorted `ids` array, binary search), the complete forward and reverse adjacency lists of touched nodes (list-replacement semantics: the newest segment that holds a node's list wins), bitset add/remove id lists, new bodies and symbols, new tombstones and lease rows. A reader resolves a node by probing overlay, then deltas newest to oldest, then base.

### 4.5 Overlay (in-process, built from the log tail)

The overlay is the decoded form of records between `checkpoint_lsn` and the log end: a hash map `id → NodeDelta` (header patch, field patches, body), sorted vectors of `(src, kind, dst, ±)`, bitset ± lists, tombstones, leases, and `idem` entries. Its size is bounded by the checkpoint policy (≤ 4,096 ops or ≤ 4 MiB of log), so it costs at most ~0.5–1 MB of private memory and 1–3 ms to build (estimate). A long-lived process keeps its overlay and its `replayed_lsn`; on each request it reads `HEAD` (one 4 KiB pread, ~10–170 µs) and appends only new records. A process whose segment set differs from `HEAD` remaps (rare: only after a checkpoint or rollup).

### 4.6 Write path (one command, one commit)

1. Take the writer byte on `LOCK` with exponential backoff and jitter, timeout 2 s (exit 7 with the holder's PID and command from offset 4096).
2. Re-read `HEAD`; replay new tail records into the overlay (monotonic view).
3. If an `idem_key` is given and present in `idem`, return the recorded result and unlock.
4. Validate: schema, CAS guards (`--if-rev`, `--if-status`, `--lease`), role write policy, status machine, restrict policies, Pearce–Kelly for new `blocks`/`parent` edges, forest depth.
5. Apply ops to a scratch copy of the overlay, computing derived deltas (counters, bitsets, `suspect` closure, tombstones, `mentions` edges parsed from text).
6. Serialize the commit record with before-images; append; `NtFlushBuffersFileEx(DATA_SYNC_ONLY)` (1.7–2 ms measured [05 §2.2]); if the flush fails, abort the process (fsync failure is fatal [08 §8.2]).
7. Write the alternate `HEAD` slot (no flush); append a `Lazy` stamp if any.
8. If the tail now exceeds the checkpoint threshold and quiet mode is off, write a delta segment (§4.10) before unlocking.
9. Unlock. Print the result with `rev`, `seq`, affected ids.

Engine cost excluding the flush: tens of microseconds for a small commit (estimate). `apply` runs steps 4–7 once for N ops.

### 4.7 Read path

A read never locks. It reads `HEAD`, maps the listed segments if not already mapped, replays the tail into the overlay, and answers from `overlay → delta_k … delta_1 → base`. Point lookups are O(1) in the base (id-dense) and O(log rows) per delta. Adjacency is the newest list found. Bitset queries compose base bitset, delta ± lists and overlay ± lists on the fly; the result is iterated, never materialised beyond the requested page.

### 4.8 Open path

1. Open `HEAD`, read both slots (2 × 4 KiB), choose the valid one with the higher `seq`.
2. Map the ≤ 5 listed segments read-only (0.22 ms each measured [05 §2.3]) and the dictionary.
3. Open the active log; scan from `committed_lsn` to the end, verifying checksums, stopping at the first bad record (torn tail); any complete commit beyond `HEAD.committed_lsn` is applied and `HEAD` is treated as stale (a writer will rewrite it).
4. Replay records from `checkpoint_lsn` into the overlay.

Total: ≈ 0.6–1.5 ms warm at 1e4–1e5 nodes, ≈ 1–4 ms at 1e6 (more segment bytes touched lazily, same bounded tail); estimate built from the measured primitives. No step is O(node count) or O(history).

### 4.9 Indexes

| Index | Structure | Where | Cost |
|---|---|---|---|
| id → row | id-dense base; sorted `ids` in deltas; hash map in overlay | segments / overlay | 0 B extra in base |
| forward / reverse adjacency | CSR (`off`, `dst`, `kind`) | segments; sorted ± vectors in overlay | ≈ 5 B per edge per direction + 8 B per node |
| kind, status, ready, is_blocker, suspect, deleted, claimed, proposed | frozen bitsets | segments; ± lists in overlay | ≈ 0.02–1 B per node per set |
| precedence order | `topo` column | segments / overlay | 4 B per node |
| commits (seq → lsn) | sorted array | base segment | 28 B per commit |
| node history | `last_lsn` column + `prev_lsn` chain in ops | segments / log | 8 B per node |
| idempotency keys | sorted array, 7-day retention | segments / overlay | 32 B per key |
| leases | sorted array | segments / overlay | 24 B per live lease |
| symbols | sorted string table | segments | strings once |
| text (tier 1) | none; scan titles/abstracts (and bodies with `--bodies`) | — | 0 |
| text (tier 2) | front-coded sorted term array + delta-varint postings per field group | base segment | ≈ 1.5 B per posting (estimate; 324 KB for 119k terms is the published FST reference point [05 §13]) |

### 4.10 Checkpoint, rollup, GC

- **Checkpoint** (delta segment): triggered by the writer when the tail exceeds 4,096 ops or 4 MiB; writes touched rows, lists, bodies; flushes the segment; appends a `Checkpoint` record (second flush); updates `HEAD`. Cost 5–50 ms at any N (proportional to the tail, not the store; estimate).
- **Rollup** (new base): when delta count > 4 or delta bytes > base/4, and quiet mode is off; merges base + deltas + tail into `seg.base.G+1`, rebuilds bitsets, `topo` (one Kahn pass), commit index, tier-2 terms, and retrains the dictionary if bodies grew > 25%. Cost 10–30 ms at 1e4, 0.1–0.3 s at 1e5, 1–3 s at 1e6 [05 §7, §16.1]; at 1e6 rollups are rare (every ~20k ops) and can be forced off-hours with `moirai compact`.
- **Who does it:** the process that crosses the threshold, inside its writer lock, or the long-lived MCP server at the end of a request (it holds no timer; it only checks a counter after serving). `moirai compact` runs it explicitly. `moirai quiet on` raises thresholds 8× and disables rollup.
- **GC:** old segments are deleted after `HEAD` has pointed elsewhere for 60 s. Files are opened with `FILE_SHARE_DELETE`, so a process that still maps an old segment keeps it readable (delete-pending) and the delete is retried later [05 §6.3]. A reader that read an old `HEAD` and then fails to open a delete-pending segment re-reads `HEAD` and retries (bounded).
- **History** is never GC'd by default; `moirai history squash --before <date>` (explicit, owner command) rewrites old log files into a checkpoint plus commit headers.

### 4.11 Crash safety

- Every record has a length, LSN and xxh3-64; recovery is "scan forward, stop at the first bad checksum" (the WAL rule shared by SQLite, RocksDB, fjall [05 §7]).
- A commit is acknowledged only after its flush returns. `HEAD` is a cache; the two checksummed slots make a torn `HEAD` write harmless.
- Segments are written to a temp name, flushed, then referenced by a flushed `Checkpoint` record before `HEAD` points at them, so `HEAD` can never reference a partial segment. There is no rename-over of a live file (Defender/indexer sharing violations [08 §4 W3]); the temp name simply becomes the final name because nobody else opens it before `HEAD` lists it.
- Flush failure aborts the process; the next opener recovers (ATC'20 lesson [08 §8.1]).
- `doctor --verify` recomputes every derived structure from base + log and compares; `doctor --fsck` verifies all column checksums and the BLAKE3 footers.
- Readers of sealed segments trust seal-time checksums; an I/O error inside a mapped view is a structured exception Rust cannot catch, so `doctor --fsck` exists and the store is refused on non-local volumes [05 §5.3].

### 4.12 Windows specifics

- Locks via `LockFileEx` on `LOCK` only; data files are never locked (Rust's `File::lock` locks the whole range [05 §6.3]). Lock release after a crash may lag, so waiters use bounded retry and report the recorded holder PID and whether that PID is alive [08 §4 W2].
- Flush via `NtFlushBuffersFileEx(FLUSH_FLAGS_FILE_DATA_SYNC_ONLY)` through `windows-sys`; `FILE_FLAG_WRITE_THROUGH` is not trusted [05 §6.2].
- Mappings via `CreateFileMappingW`/`MapViewOfFile` read-only, opened with `FILE_SHARE_READ|WRITE|DELETE`; a mapped file is never resized; growth happens by new files [05 §6.1].
- Log growth by `SetEndOfFile` in 16 MiB steps (the log is never mapped).
- ≤ 10 files, no per-command file creation, stable install path for the binary (Defender rescans rebuilt executables [05 §6.4]).
- Bodies enter through stdin, `@file` or MCP JSON, never argv (PowerShell 5.1 strips quotes [07 §5.2]). Output is UTF-8 bytes regardless of code page.
- Unix builds swap `fdatasync`, `flock`/`fcntl` byte locks and `mmap`; nothing else differs.

---

## 5. Versioning

### 5.1 Commits and the operation log

Every write command produces exactly one commit (jj-style auto-commit at mutation granularity [04 §3.8]); `apply` makes one commit for a batch. Commits are content-addressed (BLAKE3-256 over the record after the id), carry their parents, and form a DAG; the log is their canonical storage, so history costs bytes per commit rather than 4 KiB × depth [04 §0.1, §6]. The "op log vs commit DAG" distinction of jj collapses here: each commit *is* the operation, and `moirai undo` is the inverse of the last commit by the same actor (before-images make inversion exact).

### 5.2 Refs and branches

`HEAD.refs` maps names to commit ids: `trunk`, `exp/<name>`, and tags. Creating a branch is one `Ref` op. A branch's commits carry `branch = k` in their header. A branch is a **delta overlay that rebases on read**: the branch view is the current trunk state with the branch's ops applied on top (the same overlay machinery as the log tail, keyed by branch). Branch ops are invisible to trunk readers. There is no trunk snapshot per branch, which is what keeps branches free of RAM and disk cost; a branch with more than 50k ops is "promoted" by writing a branch delta segment (same format, `kind = branch`). Optional `--pinned` branches freeze the trunk view at the fork sequence by reverse-applying trunk commits since the fork (cost O(trunk ops since fork), on demand).

### 5.3 Diff

`moirai diff A..B` for two commits on one line of history is the union of ops between them, folded per `(node, field)` and edge key, O(ops in range); for commits on different branches it is the two changesets since the LCA (found with generation numbers stored in the commit index, git commit-graph style [04 §3.1]). Output is one line per node with the fields that changed, or `--json`.

### 5.4 Merge algorithm

Input: source ref S (branch), target ref T (trunk by default). Steps:

1. Collect S's ops since its fork; group by key `(node, field)` for fields, `(src, kind, dst)` for edges, `node` for create/delete/move.
2. For each key, the **base** is the before-image recorded in the earliest S op for that key; **ours** is T's current value; **theirs** is S's final value.
3. Apply the per-type rule: unchanged on one side → take the other; equal changes → take; otherwise the field-type rule:
   - status lattice: forward moves merge (`open→in_progress` vs `open→done` → `done`); a backward move against a forward move is a conflict;
   - enum/scalar: conflict value `Merge{base, ours, theirs}` stored in the field, node flagged `conflict`;
   - counter (`priority`? no: priority is a scalar) — derived counters are never merged, recomputed at the end;
   - sets (`labels`, `applies_to`): add-wins union with removals relative to base;
   - text (`body`, `acceptance`): line diff3; overlapping hunks become a conflict value carrying both hunks;
   - `parent` moves: Kleppmann move semantics in timestamp order; a move that would create a cycle is skipped and recorded [04 §3.11];
   - delete vs modify: conflict by default; per-kind policy `delete_wins` (tasks) or `resurrect` (knowledge) selectable;
   - new structural edge to a node deleted on the other side: `DanglingEdge` violation record; the edge is kept but flagged `has_dangling` and treated as a blocker until resolved.
4. Apply the merged changeset to a scratch overlay on T; run validators: forest, Kahn over `blocks ∪ child→parent` (a cycle yields a `Cycle` violation naming the edges; the tasks involved leave `ready`), reverse-index equality, status machines, schema.
5. Write one merge commit with two parents containing the changeset, conflict values and violation records; advance T unless `--strict` and conflicts exist.
6. `moirai conflicts` lists them; `moirai resolve #id field=value` writes ordinary commits that clear the flags.

Coordination ops (`Claim`, `Release`, `Complete`) are refused on branches, so C10 (double claim) cannot arise [04 §5].

### 5.5 Conflict taxonomy

| Code | Class | Default outcome |
|---|---|---|
| C1 | same scalar field changed differently | conflict value (status: lattice first) |
| C1b | same text field, overlapping hunks | conflict value with both hunks |
| C1c | set field | merged, no conflict |
| C2 | delete vs modify | conflict; per-kind policy optional |
| C3 | structural edge to a deleted node | `DanglingEdge` violation, edge flagged |
| C4 | cycle in the precedence graph | `Cycle` violation, tasks unready |
| C4b | cycle in the hierarchy | later move skipped, recorded |
| C5 | id collision | impossible (global counter); reported if an import produces one |
| C7 | strengthening schema change on one side | conflict; merge refused until `migrate` |
| C9 | derived state disagreement | never merged; recomputed |
| C10 | double claim | impossible (trunk-only) |

### 5.6 History, as-of, blame

- `moirai log [#id] [--since seq]`: commit headers from the commit index (base) plus the tail; per-node history walks the `prev_lsn` chain, O(edits to that node) preads of ≤ 1 KiB each.
- `moirai show #id --at <commit|seq>`: walk the chain backwards applying before-images until the target sequence; cost O(edits after the target).
- `moirai at <commit> -- <query>`: whole-graph as-of. Recent targets reverse-apply from the current state (O(ops since target)); older targets replay forward from genesis into a temp overlay file (O(history), explicit and warned). Optional retention of one base segment per week (`--keep-checkpoints weekly`) bounds this at the cost of disk. This is the lean position: per-node history is fast, whole-graph time travel is a cold operation.
- `moirai blame #id field`: the commit that last set the field, from the chain.

### 5.7 Git linkage

Every commit records `git_head` (32 bytes plus algorithm tag, SHA-256-ready for Git 3.0 [04 §3.1]), git branch, worktree and lane. These are provenance, not foreign keys; rebase and squash rewrite SHAs [04 §7]. Knowledge written from a non-default branch gets `flags.proposed` and a `scope = code@<sha>` field; `moirai reconcile --lane #L --merged-at <sha>` (explicit orchestrator step or a git `post-merge` hook) clears `proposed` for nodes whose scope commit is an ancestor of the merge (one `git merge-base --is-ancestor` per distinct scope commit, batched), marks tasks whose evidence commits landed as `done-on-trunk`, and flags abandoned lanes. Optional M5: publish log and base files as git blobs under `refs/moirai/data` for backup and cross-machine bootstrap (Dolt precedent, with the documented refspec and a `doctor` check against `push --mirror` deletion [08 §7.3]). A sorted JSONL export exists for review and is never re-imported implicitly [03 §8.2].

---

## 6. Concurrency and sync

### 6.1 Processes

All processes are equal embedded clients: the orchestrator's stdio MCP server (one per Claude session, shared by its subagents [08 §2]), every CLI call from up to 16 Workflow agents or 20 subagents in other worktrees, every hook invocation, and a second session's MCP server in another lane. None has threads or timers. Idle CPU is zero because nothing is idle: a process either serves a request or blocks on stdin.

### 6.2 Lock protocol

- Writers: exclusive `LockFileEx` on `LOCK` byte 0; bounded retry (jittered backoff from 1 ms, timeout 2 s). After acquiring, write `{pid, start_ms, command}` at offset 4096 so waiters can print the holder. Expected hold time 2–3 ms per commit; with 16 agents issuing at most a few writes per second each, queueing delay is well under a flush time (estimate; the M0 exit gate measures p99 with 16 writers).
- Readers: no lock, no registration. Correctness comes from immutable segments and the monotonic sequence.
- Compaction: byte 1, taken by whoever runs a rollup, in addition to byte 0.
- The protocol is a strict subset of redb 4.3's [08 §3.3]: no reader bytes are needed because segments are never modified in place and GC uses a grace period plus delete-pending semantics instead of exact reader tracking.

### 6.3 Leases and claims

`claim #T --agent A --ttl 15m` is one durable commit: checks `ready(T)`, no live lease, then writes `Claim{T, A, token, expires}` where `token = HEAD.next_token++` is a store-wide fencing token. `complete`, `release`, `set --lease` must present the lease; an older token fails with exit 5 and the current holder. `heartbeat` appends a `Lazy` record extending `expires` (not flushed; a power loss shortens the lease at worst). Expiry is time-based and checked at read time (a dead holder's lease is ignored once expired; `reclaim --older-than 30m` writes explicit releases). Optional liveness checks: `CLAUDE_PID` recorded in the lease and tested by the reader (a dead PID means "expired now"), and the `SubagentStop` hook, which releases or flags leases held by the stopping `agent_id`. Defaults: 15 min for self-claims, 60 min for dispatcher claims across a Workflow run [07 §7.4].

### 6.4 Change feed

Every commit has a `seq`; each commit record lists its ops and an `affected` list (dependents unblocked, referrers of a tombstone, sources marked suspect). `moirai changes --since <seq> [--about #ids | --for-agent A]` reads records from the LSN found via `HEAD.seq_ring` (last 32 commits) or the base commit index (older), filters by relevance (nodes the agent claimed, is blocked on, authored, or cited) and prints one line per change. Every command's header line prints the current `seq`, so callers always know what to pass. There is no file watcher (lossy on Windows [08 §4 W10]) and no push; the feed is pulled by hooks at turn boundaries.

### 6.5 "Node 40 deleted", end to end

1. **Store (same commit).** `rm #40 --reason "dup of #52" --replaced-by #52`: the writer walks #40's reverse list (O(degree)): restrict kinds refuse unless `--cascade`; `blocks` in-edges from #40 to #12 are dropped and `#12.open_blockers` decremented (#12 may enter `ready`); `cites`/`derived_from` sources (#77) get `suspect`; historical edges stay and now point at a dead id; the row keeps `deleted`; a tombstone `{40, tx, "dup of #52", 52}` is written; the commit record lists affected `[12, 77, …]`; one flush; `seq = N`.
2. **Other processes.** On their next operation they pread `HEAD`, see `seq N > replayed`, replay the new record into their overlay (microseconds), and every read they answer afterwards is consistent: `#12` shows as ready, `#77` shows `suspect`, any reference to `#40` renders `#40 (deleted c812 by dev#2: dup of #52 → #52)`. Monotonic reads hold across processes without a daemon.
3. **Agents.** The next `UserPromptSubmit` hook in the orchestrator's session prints `since seq M: #40 deleted (dup of #52); #12 now READY; #77 suspect` (nothing if nothing relevant changed). A subagent learns at its next moirai tool result, whose header carries the seq and whose rendering of `#40` is the tombstone line. A write that still assumes #40 (`set #40 …`, `--if-rev` on #12 with an old rev) fails with exit 3/4 and the current value, so no stale-context write succeeds silently [08 §6.1].
4. **Branches.** A what-if branch that modified #40 or added a structural edge to it sees C2/C3 records at merge time; trunk readers never see a dangling structural edge.

### 6.6 Idempotency

Every write accepts `--idempotency-key K` (MCP: `idempotency_key`). The writer hashes it (xxh3-128), looks it up in `idem` (overlay, deltas, base), and on a hit returns the original result without writing. Entries live 7 days and ride the segment pipeline (32 B each). Workflow resume, MCP client restarts and model retries are therefore safe [07 §7.4]. `apply` keys the whole batch and, optionally, each item (`run:<id>/agent:<label>/<n>`).

### 6.7 Quiet mode

`moirai quiet on|off` toggles a `HEAD` flag: thresholds for checkpoint rise 8×, rollups are disabled, `compact` refuses. Because no moirai process has background work in any mode, the flag only prevents the foreground maintenance that a busy writer would otherwise trigger. The idle rule of [02 §9] is satisfied by construction.

---

## 7. Agent interface

### 7.1 CLI surface

Conventions: compact line-oriented text by default (ids first, deterministic order), `--ids`, `--json`/`--jsonl` with a versioned envelope, empty results exit 0, distinct exit codes (1 internal, 2 usage, 3 not found, 4 guard conflict, 5 lease, 6 precondition/blocked, 7 store locked, 8 partial batch), bodies via `--stdin`/`@file`, `--agent` defaulting to `$MOIRAI_AGENT` then the hook stamp then `session:<id>`, no ANSI unless a TTY, never prompts [07 §6.2].

```
# context
moirai brief   [--scope #id] [--role R] [--budget-chars 8000] [--more]
moirai pack    #id --role R [--budget 12000] [--json]
# read
moirai ready   [--scope #id] [--role R] [--limit 20] [--cursor c] [--ids] [--explain #id]
moirai show    #id.. [--full] [--neighbors N] [--at <commit|seq>]
moirai find    [TEXT] [kind:task status:open prio:<=1 area:net done:false suspect:true] [--bodies] [--ids]
moirai blockers #id [--transitive] [--ids]        moirai blocking [--scope #id] [--ids]
moirai tree    #id [--depth N]                    moirai stale [--scope #id]
moirai changes --since SEQ [--about #id..] [--for-agent A]
# write (all: --idempotency-key K, --agent A, --if-rev N, --if-status S)
moirai add task|doc|note|rule|decision|question|finding|verdict|measurement|artifact|run|lane|area "title" [--parent #id] [--blocked-by #id,..] [--field k=v].. [--body -|@file]
moirai set     #id [k=v].. [--status S] [--resolution R]
moirai link    #a --blocks|--parent|--relates|--supersedes|--cites|--refutes|--confirms|--verifies|--addresses|--derived-from|--answers|--scoped-to #b
moirai unlink  #a --<kind> #b
moirai move    #id --parent #p
moirai doc patch #section --remove @old.txt --add @new.txt [--depends-on #s1,#s2]
moirai rm      #id [--reason T] [--replaced-by #id] [--cascade|--reparent] [--dry-run] [--yes]
moirai apply   FILE|- [--idempotency-key K] [--dry-run]
# coordination
moirai claim   #id.. | --next [--scope #id] [--role R] --agent A [--ttl 15m]
moirai heartbeat L       moirai release L       moirai reclaim --older-than 30m
moirai complete #id --lease L --outcome done|failed|abandoned --summary -|TEXT [--evidence commit:sha|#id..]
moirai reopen  #id --reason T
# versioning
moirai log [#id] [--since SEQ]   moirai diff A..B   moirai blame #id FIELD   moirai undo
moirai branch [name|--list|--delete name]   moirai merge exp/x [--into trunk] [--strict]
moirai conflicts   moirai resolve #id k=v   moirai at <commit> -- <read command>
moirai reconcile --lane #id --merged-at SHA
# integration and maintenance
moirai hook session-start|prompt|subagent-start|subagent-stop|agent-launched|stamp
moirai mcp                     (separate face; see 7.2)
moirai quiet on|off            moirai compact [--rollup]   moirai doctor [--verify|--fsck|agents|hooks]
moirai export jsonl|rules [--to DIR]   moirai backup push|pull   moirai migrate
```

Example outputs (the owner's two requests first):

```
$ moirai blocking --ids
seq 812 · 3 blocking
#31
#44
#58

$ moirai rule --stdin --criticality critical --applies-to-roles developer,tester --authority owner <<'EOF'
Never kill processes by image name; only the PID tree you started.
EOF
seq 813 · created #201 rule critical "Never kill processes by image name; only the PID tree you started." hash 9f3c…e1

$ moirai ready --scope #120 --role developer
seq 813 · 3 ready of 9 open (scope #120)
#150  task open P1  "Reclaim sweep: expire dead-PID leases"   parent:#120 est:3
#151  task open P1  "Fencing token check in complete/release" parent:#120 est:2
#154  task open P2  "Doctor: report stale leases"             parent:#120 est:1

$ moirai ready --explain #152
#152 task open P1 "Heartbeat hook" is NOT ready:
  open_blockers=1: #151 "Fencing token check…" (open, claimed by wf:r7/dev2, lease L-19 expires in 41m)
  ancestors: #120 has no exogenous blockers

$ moirai show #12
seq 815 · #12 task in_progress P1 "Wire lease reclaim"  rev 9  parent:#7  claimed:dev#1 (L-9, 11m left)
  blockers: #40 (deleted c812 by dev#2: dup of #52 → #52) [dropped, seq 814]
  cites:    #77 rule "…" (suspect since seq 814)
  body (412 B): Reclaim must run … (moirai show #12 --full)

$ moirai set #12 --status done --if-rev 8
error[guard_conflict]: #12 rev is 9, expected 8 (changed at seq 814 by dev#2: status in_progress, blocker #40 dropped)
current: #12 task in_progress P1 "Wire lease reclaim" rev 9
hint: re-run with --if-rev 9 or without a guard
(exit 4)
```

### 7.2 MCP tools (nine)

`moirai mcp` is a stdio server built with rmcp (dual-era) in a separate crate; the storage core has no tokio. It returns compact text only (Claude Code forwards only `structuredContent` when both are present [07 §2.6]); `format: "json"` is available on request. Server `instructions` (≤ 2,048 chars) front-load when to call `brief`, `claim`, `complete`, `remember`.

| Tool | Purpose | Key params | Load |
|---|---|---|---|
| `brief` | digest at start of work | `scope`, `agent`, `budget_chars` | alwaysLoad |
| `ready` | unblocked, unclaimed tasks | `scope`, `role`, `limit`, `cursor`, `explain` | alwaysLoad |
| `get` | nodes by id, concise or full, with neighbours; `at` for as-of | `ids[]`, `detail`, `neighbors`, `at` | alwaysLoad |
| `find` | filter syntax + text | `query`, `text`, `bodies`, `limit`, `cursor` | deferred |
| `claim` | claim / next / heartbeat / release | `action`, `id`, `scope`, `agent`, `lease`, `ttl_s` | alwaysLoad |
| `complete` | finish a claimed task; returns newly ready ids | `id`, `lease`, `outcome`, `summary`, `evidence[]`, `idempotency_key` | alwaysLoad |
| `remember` | one knowledge node (rule, note, decision, finding, question, verdict, measurement) | `kind`, `title`, `text`, `fields{}`, `about[]`, `criticality`, `idempotency_key` | alwaysLoad |
| `write` | atomic batch: create/update/link/unlink/move/doc_patch with local `$refs` and guards | `ops[]`, `idempotency_key`, `dry_run` | deferred |
| `changes` | delta since a seq, or one node's history | `since_seq`, `id`, `about[]`, `limit` | deferred |

Every tool accepts `ctx {agent_id, agent_type, cwd, session_id}` which the PreToolUse stamp hook overwrites (`updatedInput`), so attribution never depends on the model. The engine enforces a per-role write policy on `ctx.agent_type` (architect: doc, decision, question, finding; critic: finding, verdict, question; researcher: note, finding; developer: task status via lease, finding, question, measurement; tester: measurement, finding, verdict(test); results-analyst: verdict; orchestrator: everything). Client-side `tools:` allowlists in agent frontmatter are convenience; the engine check is enforcement. Attribution against a deliberately hostile model is out of scope (the owner runs the agents).

### 7.3 Skills

- `moirai` (core, model-invocable, ≤ 1.5k tokens): the verbs, output conventions, `--agent`, stdin for bodies, exit codes, one example per verb; links `reference.md`. Optional dynamic context line: `` !`moirai brief --budget-chars 3000` ``.
- `moirai-orchestrate` (user-invocable, preloaded into the orchestrator): dispatch patterns, `apply` ingest format, verdict routing, `reconcile` ritual, quiet-window checklist.
- `moirai-report` (preloaded into developer, tester, reviewer): how to finish — `complete` fields, evidence format, `remember --kind finding`.

Shipped as a plugin (`skills/`, `hooks/hooks.json`, `.mcp.json`); the binary is installed separately [07 §9.6].

### 7.4 Hooks (exec form, fail-open, explicit short timeouts)

| Event | Command | Effect |
|---|---|---|
| `SessionStart` (startup, resume, clear, compact) | `moirai hook session-start` | `additionalContext` = brief ≤ 8,000 chars; replaces the hand-written MEMORY.md resume block [02 §10.2] |
| `UserPromptSubmit` | `moirai hook prompt` | delta since the session cursor, relevance-filtered; prints nothing when empty; ≤ 5 ms engine |
| `SubagentStart` | `moirai hook subagent-start` | role pack: critical rules for `agent_type`, protocol line, the task pack if a `moirai:task=#id` marker is known for this agent |
| `PostToolUse` (matcher `Agent`, async) | `moirai hook agent-launched` | maps `agentId → task, lease` from the prompt marker |
| `SubagentStop` | `moirai hook subagent-stop` | releases or flags leases held by `agent_id`; stores `last_assistant_message` as a `needs-triage` note if a lease was still open; blocks at most once |
| `PreToolUse` (matcher `mcp__moirai__.*`) | `moirai hook stamp` | `updatedInput` with `ctx`, `permissionDecision: allow` (owner may choose `ask`) |
| `PostToolBatch` (optional, v1.1) | `mcp_tool: changes` | pulls a relevance-filtered delta without a process spawn [08 §6.2] |

No `PreCompact` hook: `SessionStart` fires again with `source=compact` [07 §4.2]. Hook cost is one process spawn (15–50 ms measured [07 §5.2]) at turn boundaries only.

### 7.5 Context-pack algorithm

Inputs: anchor node `#T`, role `R`, budget `B` (chars for hooks, tokens ≈ chars/3.5 for `pack`), current `seq`.

1. Candidate classes in fixed priority order (class, then criticality desc, then `updated_tx` desc, then id asc; deterministic for prompt caching):
   - P0 critical rules whose `applies_to_roles` includes R and whose `applies_to` globs intersect the areas of `#T` (always included; if they alone exceed B, the pack fails loudly rather than truncating a rule);
   - P1 `#T` itself with title, status, acceptance, parent chain titles, lane location (worktree, branch, base, target dir);
   - P2 binding decisions and rulings linked to `#T` or its ancestors (`cites`, `implements`, `answers`);
   - P3 open confirmed findings about `#T` or the docs it implements, with `failure_scenario`;
   - P4 blockers and blocked-by with status and holder; sibling lanes' `touches` sets (do-not-touch list);
   - P5 effective doc sections for R (developer: implementation plan; tester: metrics and validation; critic: sections changed since the last round plus their `depends_on` closure);
   - P6 measurements and pins relevant to `#T` with environment and stale flags;
   - P7 recent notes scoped to the same areas, newest first.
2. Each item renders as one line (≤ 200 chars: id, kind, status, title, key fields) plus, for P1–P3 and P5, a body excerpt capped per class (P5 sections are included in full up to 4 KiB each, then excerpted).
3. Greedy fill by class; within a class stop at the first item that does not fit and record `dropped[class] += remaining`.
4. Footer: `seq 815 · budget 12000 · used 11840 · dropped: 2 notes (P7), 1 measurement (P6) → moirai pack #150 --role developer --more`. Never silent truncation [01 §7 L1].
5. `--more` returns the next page of the dropped classes. Cost: a few hundred node reads, 2–10 ms (estimate).

`brief` is the same algorithm with anchor = the scope root (or all open campaigns), classes: critical rules, open owner questions, in-flight claims with expiry, ready tasks, verdicts since the session's last seq, stale/suspect counts.

### 7.6 Walk-through: one BoykoEngine-style campaign

Roles: orchestrator (main chat), architect and architecture-critic (no Bash, MCP), developer and tester (Bash, CLI, in worktrees). Owner order: "implement lease reclaim in the physics lane".

1. **Session start.** `SessionStart` injects the brief: 3 critical rules, 1 open owner question, 2 in-flight leases (one expired), 5 of 12 ready tasks, verdicts since last session, `dropped: 7 ready tasks → moirai brief --more`.
2. **Decomposition.** Orchestrator: `moirai add task "Lease reclaim" --labels campaign` → `#120`; `moirai add lane demo --field worktree=<lanes-dir>/demo branch=u/lease-reclaim base_sha=<sha> target_dir=<lanes-dir>/_targets/demo-msvc` → `#121`; `moirai link #121 --scoped-to #120` is not needed: the lane's tasks get `parent:#120` and `runs_in` edges from runs.
3. **Research.** Architect (MCP): `find kind:note,decision area:physics text:"lease"` then `get`; writes `remember kind=finding` with `confidence=observed` for each verified premise (the "no unverified premise" rule [01 §3] becomes checkable: the pack later includes only findings with confidence ≥ observed).
4. **Plan.** Architect: one `write` batch (idempotency key `run:r7/architect/rev1`) creating doc `#130` (plan) with sections `#131..#136` (`parent:#130`, `depends_on` edges between sections), decisions `#140..#142` with `alternatives`, tasks `#150..#154` under `#120` with `blocks` edges (`#151 → #152`), each task `implements` a decision. One commit, one flush, 0.4 KB per op.
5. **Critique round 1.** Critic: `pack #130 --role architecture-critic --budget 16000` (P5 = all sections on round 1); writes findings `#160..#163` (`about #133`, severity, `failure_scenario`, `confidence`) and verdict `#164` (`raw_label=CHANGES REQUESTED`, `outcome=fail_fixable`) which `blocks #150`. Idempotency key `run:r7/critic/r1`.
6. **Refutation.** Refuter agents: `moirai link #170 --refutes #161` where `#170` is a finding with evidence, then `moirai set #161 --status refuted`; `#160` gets `--confirms`. Termination query for the orchestrator: `moirai find kind:finding about:#130 status:confirmed severity:>=important` → empty means design approved. A `--json` variant gives per-critic refuted share.
7. **Patch round 2.** Architect: `write` with `doc_patch{#133, remove: "<verbatim>", add: "<new>", depends_on: [#134]}`; the engine refuses (exit 4) if the removed text is not a substring of the current body, which is the "silent drop" guard made mechanical [01 §2.1]. Critic round 2: `changes --since <seq of r1> --about #130` returns the changed sections and their `depends_on` closure only ("round scope = delta"). Verdict `#164` set `accepted`; `#150` becomes ready in the same commit.
8. **Dispatch.** Orchestrator: `moirai claim #150 #151 --agent wf:r7/dev1,wf:r7/dev2 --ttl 60m --json` → leases `L-18`, `L-19`, tokens 1043, 1044. Workflow launched with `args {run: r7, tasks: [{id, lease}]}`; each `agent()` prompt carries `moirai:task=#150 lease=L-18`.
9. **Implementation in worktrees.** dev1 (Bash, in `<lanes-dir>/demo`): `moirai pack #150 --role developer --budget 12000` → rules, plan section `#135`, decisions, do-not-touch list from `#151.touches`. Works; `moirai add finding "…" --about #150 --confidence observed` for a deviation; returns schema output. `SubagentStop`: lease `L-18` still open → orchestrator's `apply results.json --idempotency-key run:r7` completes `#150` with `evidence commit:abc123`, which releases the lease and returns `newly ready: #152`. A resume that re-runs dev1 hits the idempotency key and changes nothing.
10. **Testing.** Tester: `pack #150 --role tester` → the metrics-and-validation section, expected test counts, baselines with environment. Writes `measurement` nodes (`metric`, `value`, `target`, `command`, `git_sha`, `host`, `load=quiet`) and a verdict `#180 (test)`; a red test is a finding that `blocks #150`'s parent's closure (`children_open` stays > 0 until fixed).
11. **Acceptance.** results-analyst: `find kind:measurement about:#120` compares `value` vs `target` (computed delta and status in the output), writes verdict `#181` with `return_to=developer` or `accepted`. `complete #120` refuses (exit 6) while any child is open or a `fail_*` verdict blocks it.
12. **Merge.** Orchestrator: `moirai show #121 --neighbors 2` for the merge brief (gates, measurements, open findings, lanes that must merge first via `blocks` between lane nodes); `git merge`; `moirai reconcile --lane #121 --merged-at <sha>` promotes `proposed` knowledge, sets `#121` merged, marks measurements whose `git_sha` is behind the new trunk as `moved`.
13. **A deletion on the way.** Orchestrator notices `#153` duplicates `#151`: `moirai rm #153 --reason "dup of #151" --replaced-by #151 --dry-run` prints the referrer impact (1 `blocks` edge to drop, 2 `mentions`), then `--yes`. The next `UserPromptSubmit` in the tester's session (if it cited `#153`) prints the tombstone line; `#152`'s blocker list renders the tombstone; nothing dangles.
14. **Next session.** `moirai brief` computes the checkpoint from live nodes; nobody edits a resume block by hand.

---

## 8. Performance and RAM budget

Machine: Ryzen 9 5900HS, 16 GB, consumer NVMe, NTFS, Defender on [05 §2]. Workload model: 3 edges per node, 60 B titles, 1 KiB raw bodies (≈ 340 B compressed, claimed ratio), 10 commits per node lifetime [05 §3]. Process spawn (20–73 ms native, +100 ms under Git Bash, measured [05 §2.1]) is excluded; it dominates every CLI call and is outside the engine.

| Quantity | 1e4 nodes | 1e5 nodes | 1e6 nodes | Derivation |
|---|---|---|---|---|
| **Private RSS, CLI or hook process** | 1.5–3 MB | 1.5–3 MB | 2–4 MB | static Rust exe pages (1–2 MB; a native console process measured 0.69 MB private [05 §2.4]) + bounded overlay (≤ 4,096 ops × ~80–200 B ≈ 0.3–0.8 MB) + per-request arena (≤ output size, ≤ 256 KiB) + 1e6: slightly larger symbol/handle tables. Estimate; independent of N by design. |
| **Private RSS, MCP server** | 4–8 MB | 4–8 MB | 5–9 MB | CLI figure + tokio runtime and rmcp/serde (≈ 2–4 MB; rmcp servers reported ~7–11 MB RSS total in HTTP benchmarks, claimed [07 §2.7]). Estimate. |
| **Shared page cache, hot index only** (headers 40 B + CSR offsets 8 B + edges 30 B + topo 4 B + bitsets ≈ 1 B) | 0.83 MB | 8.3 MB | 83 MB | 83 B/node × N; shared by all processes; evictable [05 §10.4]. Estimate. |
| **Shared page cache, whole store touched** (+ titles 60 B, fields 24 B, bodies 340 B, cold columns 32 B) | ≈ 5.4 MB | ≈ 54 MB | ≈ 0.54 GB | (83 + 456) B/node. Only pages actually read are resident. |
| **Disk, history** | ≈ 40 MB | ≈ 0.4 GB | ≈ 4 GB before squash | 1e5 / 1e6 / 1e7 commits × 0.4 KB; zstd of log extents (opt-in at rotation) roughly halves it (claimed ratio). |
| **Open (warm)** | 0.6–1.2 ms | 0.7–1.5 ms | 1–4 ms | 2 × 4 KiB HEAD reads (0.17 ms each measured) + 2–5 maps × 0.22 ms (measured) + tail replay ≤ 1–3 ms (estimate, bounded by policy). |
| **Get by id** | 1–5 µs | 1–5 µs | 1–5 µs | overlay probe + ≤ 4 delta binary searches + 3–6 page touches at ≈ 1 µs each when the page is cached in the process for the first time [05 §2.3]. Estimate. |
| **`ready` (page of 20)** | 5–20 µs | 20–100 µs | 0.3–1 ms | bitset AND over N/8 bytes (1.25 KB / 12.5 KB / 125 KB) + ancestor walk ≤ 12 per candidate + rendering. Estimate. |
| **`blockers #T --transitive`** | 5–50 µs | 5–50 µs | 5–100 µs | BFS over the reverse CSR, O(reachable), typically < 100 nodes. Estimate. |
| **`blocking --ids` (all blockers)** | 2 µs + print | 10 µs + print | 60 µs + print | `is_blocker ∧ ¬done` bitset scan; printing ≈ 0.1 ms per 1,000 ids. Estimate. |
| **`pack` / `brief`** | 1–5 ms | 2–8 ms | 3–12 ms | a few hundred node reads + body decompression (≈ 0.2 µs per 340 B body at 1.5 GB/s, claimed) + rendering. Estimate. |
| **Durable commit (engine)** | 2–2.5 ms p50, ≈ 6 ms p99 | same | same | lock 2–10 µs (measured [08 §2]) + validate + append + data-only flush 1.73 ms p50 / 3.3 ms p99 (measured), FlushFileBuffers p99 5.7 ms [05 §2.2]. |
| **Checkpoint (delta)** | 5–20 ms | 5–30 ms | 10–50 ms | proportional to the tail (≤ 4,096 ops), plus one segment flush and one log flush. Estimate. |
| **Rollup (new base)** | 10–30 ms | 0.1–0.3 s | 1–3 s | sequential rewrite at 1–2 GB/s plus serialization [05 §7]; every ~20k ops; never in quiet mode. |
| **FTS tier 1 (titles + abstracts)** | 0.2–1 ms | 2–10 ms | 20–80 ms | scan of 260 B/node with memchr-class throughput. Estimate. Tier 2 above 20k nodes: 0.1–5 ms per term [05 §13]. |
| **Idle CPU, all modes** | 0 | 0 | 0 | no threads, timers, watchers or polling exist. |

CI measures private bytes and peak working set with `GetProcessMemoryInfo`, counts flushes per command, and gates: engine ≤ 5 ms per CLI command at 1e5, private ≤ 4 MB CLI and ≤ 10 MB MCP at 1e5, exactly one flush per commit, no O(N) work on open [05 §17].

---

## 9. Build plan

Sizes are estimates of relative effort (share of the whole) and lines of Rust excluding tests. Every milestone ends with its benchmarks run on the owner's machine with Defender on and under typical agent load, both reported.

| Milestone | Scope | Exit criteria | Size |
|---|---|---|---|
| **M0 Engine core** | `Vfs` trait with Windows and in-memory implementations; `HEAD`, log records, `LOCK` protocol, segment writer/reader, overlay, open/commit/replay, checkpoint, rollup, GC; `doctor --fsck` | Windows kill-loop: 16 writer processes, `TerminateProcess` at random points, 10,000 iterations, zero lost acknowledged commits, zero corrupt opens. Deterministic multi-process simulation with crash-point enumeration at every write/flush boundary and fsync-error injection passes. Open ≤ 2 ms at 1e5; one flush per commit; writer-wait p99 measured with 16 processes. | 25 % · ≈ 5–6k LOC |
| **M1 Graph core** | node/edge model, schema registry, field blocks, CSR + bitsets, derived state, Pearce–Kelly, delete policies and tombstones, `mentions` parsing, leases and fencing, idempotency table, change feed | Property tests: incremental derived state equals full recomputation after 10^5 random ops including deletes and reparent; cycle rejection under fuzzing; `doctor --verify` clean after kill loops. `ready`/`blockers` latencies within §8 at 1e5 and 1e6 synthetic corpora. | 20 % · ≈ 4–5k LOC |
| **M2 CLI, skill, hooks** | the §7.1 surface (minus versioning verbs), output contract and exit codes, `apply`, `pack`/`brief`, SessionStart/UserPromptSubmit/SubagentStart/Stop/agent-launched hooks, core skill, `doctor agents\|hooks` | Owner adoption gate: one real Workflow replaces its hand-written HDR with `moirai pack`, uses dispatcher claims plus `apply`, and resumes correctly after a forced failure. Hook output never exceeds 8,000 chars; drop footers verified. CLI private RSS ≤ 4 MB. | 15 % · ≈ 3–4k LOC |
| **M3 MCP** | `moirai-mcp` crate on rmcp, nine tools, stamp hook, role write policy, plugin packaging, `.mcp.json` | Architect and critic complete a critique round without Bash; attribution correct in 100 % of stamped calls; server private RSS ≤ 10 MB at 1e5; tool listing ≤ 5k chars of schema. | 8 % · ≈ 1.5k LOC |
| **M4 Versioning surface** | `log`, `diff`, `blame`, `show --at`, `at`, `undo`, branches (rebase-on-read overlays), merge engine with conflict values and violation records, validators, `conflicts`/`resolve`, `reconcile` | Merge fuzzing: 10 branches × 1,000 ops with injected C1–C4 classes; every conflict class detected; post-merge invariants hold; no merge blocks an agent unless `--strict`. As-of correctness vs replay oracle. | 17 % · ≈ 4–5k LOC |
| **M5 Search, export, backup** | tier-2 FTS at rollup, `find --bodies`, `export jsonl\|rules`, `backup push\|pull` via `refs/moirai/data`, `stale`, dictionary training | Search ≤ 5 ms per term at 1e5; export deterministic and regenerable; backup round-trip restores a store byte-identical at the logical level; `doctor` detects a missing remote ref. | 7 % · ≈ 2–2.5k LOC |
| **M6 Hardening and gates** | DST harness extended to merges and GC, Defender-interference test (a process holding files open without `FILE_SHARE_DELETE`), lock-release-delay simulation, benchmark suite with hyperfine, CI budgets, format migration path (`migrate`) | All §8 budgets met on the owner's machine; format version bump tested with refusal-on-newer and explicit migration. | 8 % · ≈ 2–3k LOC + harness |

Total: roughly 22–26k lines of Rust plus tests (estimate). The long pole is M0's crash and multi-process testing, as every report warns [04 §11; 08 §8.1]. Baselines for honesty: before M1, the moirai workload is run against redb 4.x and SQLite (WAL, `synchronous=FULL`) on the same machine; the from-scratch engine must beat them on open time and private RSS and match them on commit latency to justify itself [05 §16.1].

Test strategy summary: (1) property tests against full recomputation for every derived structure and for merge outcomes; (2) deterministic simulation of N processes over the in-memory `Vfs` with seeded scheduling, crash-state enumeration and fsync failures (the SQLite WAL-reset class of bug was found this way in 15 minutes [08 §3.1]); (3) real Windows kill loops with 16 writers and readers in 16 worktrees; (4) benchmark gates in CI with private bytes, working set, flush counts and page faults recorded per command.

---

## 10. Risks and the top five ways this design fails

| # | Failure | Why it is plausible | Mitigation |
|---|---|---|---|
| 1 | **A multi-process or crash-recovery bug loses an acknowledged write or corrupts the store.** | The coordination protocol is the hardest part; SQLite's WAL-reset race lived 16 years [08 §3.1]; Beads lost 7 of 8 acknowledged closes under agent load [03 §2.7]. | M0 exit gates (DST with crash enumeration, kill loops), acknowledge only after flush, `HEAD` as a cache with log as truth, `doctor --verify` after every kill-loop run, read-back hash returned by every write. |
| 2 | **Overlay and delta layering makes reads slow or memory-heavy under write bursts at 1e6 nodes.** | Each read probes overlay plus up to 4 deltas; a burst of 100k scattered updates before a rollup is the worst case for list-replacement deltas [05 §16 risks]. | Checkpoint thresholds by ops and bytes; rollup by delta ratio; measured budgets at 1e6 in M1; fallback to Option B (CoW B+tree behind the same lock protocol) if `get` exceeds 50 µs after a full tail. |
| 3 | **Windows lock-release delay after a killed writer stalls all writers.** | Documented: unlock time "depends upon available system resources" [05 §6.3]; the owner kills processes wholesale [02 §9]. | Bounded retry with the recorded holder PID; if the PID is dead the waiter keeps retrying up to 2 s and reports clearly (exit 7); no correctness depends on the lock being released promptly, only liveness. If measured delays exceed seconds, add a lease-style writer token in `HEAD` with a takeover rule. |
| 4 | **Agents bypass the protocol** (forget `complete`, invent ids, write stale values) so the graph drifts from reality. | LLMs skip steps; identity is self-declared for CLI calls; Workflow resume replays agents. | Dispatcher-claims pattern with orchestrator `apply` as the default; `SubagentStop` safety net; TTL leases; CAS guards that return current values; idempotency keys; `mentions` parsed so text references are tracked even when agents forget `link`. |
| 5 | **Merge semantics creep** turns M4 into an open-ended project. | Every field type needs a rule and a validator; branches invite feature requests [04 §11]. | v1 has one trunk; branches are explicit and rare; the field type set is closed (bool, int, enum-with-lattice, text, set, ref); merge engine ships after adoption (M4) and only for `exp/*` branches. |

Other risks tracked: an I/O error inside a mapped view crashes the process (documented, `doctor --fsck`, local volumes only); Defender or an indexer holding a segment open delays GC (delete-pending is tolerated); dictionary compression ratio on real notes may be lower than claimed (measured in M5 before relying on it); the zstd crate is a C dependency (accepted for the codec only; `lz4_flex` is the pure-Rust fallback at a worse ratio); format evolution (versioned header, refusal on newer, explicit `migrate`, never an auto-migration on open [03 §8.2]).

---

## 11. Decisions the owner must make

Only value or scope calls; each with the recommended default.

| # | Decision | Recommended default |
|---|---|---|
| 1 | **Store location:** `<git-common-dir>/moirai/` (shared by all worktrees, lost with the clone) vs a per-user directory keyed by repository. | Common dir; `backup push` to `refs/moirai/data` from M5 for durability beyond the clone. |
| 2 | **Bodies:** inline cap (64 KiB) and whether pointers into session scratchpads are acceptable for larger artifacts. | 64 KiB inline; larger as artifact pointers with excerpt; raise the cap if scratchpads are considered ephemeral. |
| 3 | **Read-only roles writing their own node kinds through MCP** (architect, critic, researcher) while keeping no file Write/Edit. | Yes, under the engine's per-role write policy. |
| 4 | **Permission posture for moirai MCP calls:** the stamp hook auto-approves (`allow`) or prompts (`ask`). | `allow`; the role policy is the guard. |
| 5 | **Deletion default:** drop-and-notify for `blocks` and restrict for `parent` (recommended) vs refuse any delete while referenced ("strike, never delete"). | Recommended policies; `rm` always prints the impact and requires `--yes`. |
| 6 | **Lease defaults:** 15 min self-claim, 60 min dispatcher claim; immediate expiry when the holder's Claude PID is dead. | As stated. |
| 7 | **Cross-machine sync or cloud agents writing to the same store in v1.** | No; `uid` is reserved so it can be added without a format change. |
| 8 | **Retention:** history forever with explicit `history squash`, vs automatic pruning by age. | Forever; squash is an owner command. |
| 9 | **Workflow dispatch style:** orchestrator claims in bulk and persists results with one idempotent `apply` (recommended) vs every agent writes its own nodes. | Dispatcher pattern; agents may still `remember` findings directly. |
| 10 | **Migration of the existing memory corpus (255 files) and registers.** | Import rules, hazards and owner rulings as typed nodes with `authority` and a `cites` pointer to the source file hash; leave the rest as a read-only archive linked by artifact nodes. |
| 11 | **Language of stored text.** | English for repository-facing nodes; the engine is byte-agnostic. |
| 12 | **Whether a resident MCP server may stay up during quiet measurement windows.** | It may: it uses 0 % CPU and a few MB; `moirai quiet on` guarantees no maintenance runs. |
