# 11 — Proposal B: "git-faithful" moirai

*Architecture proposal for moirai. Date: 2026-09-25. Status: design only; nothing implemented. Evidence citations use [01]..[08] with the section name of the source report; the digest is [00]. Numbers marked **est.** are my estimates, derived as stated; numbers marked **[M]** were measured in the cited report on the owner's machine.*

---

## 1. Thesis and angle

1. moirai is **git for a typed graph**: a content-addressed commit DAG whose commits are *typed changesets with before-images* (SetField/AddEdge/Delete/Move, each carrying the old value), refs and a reflog, three-way merge at (node, field) and (src, kind, dst) granularity with per-type merge functions, conflicts stored as data, and per-node op chains that give `log`, `blame`, `diff` and as-of reads at the cost of the change, not of the tree.
2. Git semantics are bent in exactly one place: the **coordination plane** (task status, claims/leases, `blocks`, lanes, runs) is a single linearizable trunk that is never branched, because two worktrees must not both claim task #12 and then "merge" it [07 §7.2 option D; 08 §7.2]. Everything else, the **knowledge plane** (plans, sections, decisions, findings, verdicts, measurements, rules, notes), is branchable per lane and merges when the lane's git branch merges.
3. Lane branches are **sparse and live-rebased**: a lane branch forks only the knowledge plane, and reads on a lane always see trunk HEAD underneath the lane's own overlay, so "node 40 was deleted" reaches every lane immediately while the lane's own proposals stay isolated until merge [04 §12 Q2; 08 §6.3].
4. The store lives in `<git-common-dir>/moirai/` (shared by all 44 worktrees [08 §2]), every commit records git provenance, lane refs record the git base and tip, and history can be published under `refs/moirai/*` on the project remote (Dolt's `refs/dolt/data` precedent [04 §7]).
5. Angle optimised for: the owner's phrase "versioned like git", made precise for a graph of tasks and knowledge, while staying inside the hard budget (private RSS in single-digit MB per process, one flush per commit, zero idle CPU, Windows-first).

---

## 2. Positions on the known forks T1–T13

| Fork | Position | Evidence | What would change my mind |
|---|---|---|---|
| **T1** materialized state | **Immutable, sealed, read-only-mmap'd columnar segments (base + ≤3 tiered deltas) plus a small in-process overlay built from the op-log tail.** The op log is canonical; segments are packfiles, the tail is loose objects. The two checksummed HEAD slots and the "commit in place, never rename-over" rule are borrowed from the CoW-page design. | Path-copy/CoW state costs 12–16 KiB per retained commit vs ~0.3–0.6 KB for changesets [04 §3.14, §6]; mapped sealed files share one physical copy across the 16 agent processes and cost ~1 µs per cached page [05 §2.3, §5.3]; Windows forbids truncating or extending mapped files, so map only immutable files [05 §6.1]; a CoW B+tree is "the hardest to build from scratch" with allocator, free lists and multi-process page reclamation [05 §16 Option B]. | If the M1 benchmark shows overlay-merged reads more than ~2× slower than a B+tree point read under the owner's real update rate at 1e5–1e6, or if synchronous delta checkpoints on the commit path exceed 100 ms at 1e6 even when incremental, switch the materialized trunk state to a CoW B+tree file and keep the op log canonical. |
| **T2** process model | **Embedded multi-process with byte-range locks on a dedicated `LOCK` file (never on data files) in v1; the long-lived MCP server becomes an *opportunistic leader* (leader byte + named pipe, group commit, change broadcast) in v1.5; no separately auto-started daemon, ever.** Zero idle CPU: no threads, no watchers, no polling; checkpoints run inside the committing writer when a threshold trips, and a `quiet` flag in HEAD defers them. | LockFileEx locks are mandatory and block ReadFile on the locked range; Rust `File::lock` locks the whole file [05 §6.3]; a 1-byte lock at 2^62 works on NTFS in 2–10 µs [08 §2]; daemons die with the harness job object and Claude Code's own Windows daemon has a pipe-lifecycle bug [08 §4 W9, W12]; Beads deleted its daemon then proposed one again for per-call cost [08 §5.1]; the MCP server already lives as long as the session and serves all its subagents [08 §5.2]. | If M2 measurements show MCP tool p50 > 5 ms at 1e5 because every foreign commit invalidates the follower cache, promote the leader from v1.5 to v1. If the owner forbids any long-lived moirai process during quiet windows, drop the leader entirely (the embedded protocol is always in force). |
| **T3** branch model | **Two planes. Coordination plane = one live trunk, never branched. Knowledge plane = a sparse `lane/<name>` branch per opened lane, forked at the lane's git base, merged into trunk by the merge-queue step; explicit `exp/<name>` full branches (everything forked, claims forbidden) for what-if planning in v2.** Lane reads are live-rebased: trunk HEAD underneath the lane overlay. | Rulings made on one branch never reached another (12 lost) and union merges resurrected RESOLVED as OPEN in 10 places [01 §7 L5–L6]; the owner wants a live view of who owns what across lanes [02 §7.3]; branch identity is unstable (detached worktrees, harness-created `worktree-*` branches) so auto-following git is rejected [08 §7.2]; lane-local knowledge (moved pins, findings) must not leak before merge [02 §12.4]. | If after one campaign the owner never merges a lane branch (everything is written to trunk with provenance), collapse lanes to "provenance + `proposed` flag + ancestry-scoped visibility" [08 §7.2] and keep only `exp/` branches. If the owner wants task decomposition to be speculative per lane, move `task` creation (not status/claims) to the knowledge plane. |
| **T4** IDs | **Both.** Store-global, never-reused sequential `#N` (u32 row id, allocated under the writer lock from a non-versioned counter) is the display and index identity; a 128-bit `uid` (random, generated at create) is written once per node and is the identity used in the *canonical hashed form* of commits and in `refs/moirai/*` export/import. | UUIDs cost ~24 tokens and 5–10× more Claude Haiku errors than integers [06 §9.1]; sequential ids are merge-safe only under one shared allocator, which the single common-dir store provides [06 §9.2; 04 §12 Q3]; Beads switched to hash ids because sequential ids collided across clones [03 §2.4]; a commit id must not depend on which store allocated `#N` if history is ever pushed. | If the owner rules out cross-machine sync and clones forever, drop `uid` (saves 16 B/node and one index). If cloud agents must write to the same graph from another machine in v1, uid becomes the primary key and `#N` becomes a per-store alias from day one. |
| **T5** deletion | **Hard delete in the current state + tombstone row (`flags.deleted`, `deleted_tx`) + full before-image in the commit.** Per-edge-kind policy: structural `parent` restrict (`--cascade` / `--reparent`), `blocks` drop-and-notify, `answers`/`scoped_to`/`duplicate_of` restrict-or-reassign; historical edges keep the dead id and render `#40 (deleted c812 by dev#2: "dup of #52")`; sources of `derived_from`/`cites` become `suspect`. Cross-branch: trunk deletes are visible on lanes immediately (live-rebased reads); lane deletes are proposals that surface as DeleteVsModify conflicts or DanglingEdge violations at merge. | Datomic `retractEntity` + VAET, Gel per-link policies, Beads' `[deleted:ID]` text rewriting to avoid [06 §8]; soft delete is unnecessary in a versioned store [06 §8.1]; Beads' JSONL import "cannot infer that records absent … were deleted" so tombstones are required for merges [08 §6.3]; the owner's register convention is "strike, never delete" [02 §12.3]. | If the owner wants every delete to be a plain status (`archived`) with no engine-level removal, replace the delete op with a `Tombstone` status transition and keep edges intact; the reverse-index walk stays the same. |
| **T6** bodies | **Tiered.** Bodies ≤ 64 KiB (sections, decisions, findings, notes, rules, report summaries) are stored in moirai as content-addressed, zstd-dictionary-compressed blobs (git blobs, deduplicated across revisions and branches). Larger artifacts (full reports, patches, manifests, exported trees) are `artifact` pointers: path + sha256 + bytes + kind. | diff3 and the verbatim-removed-text guard need the text in the store [01 §2.1, §8.1]; result payloads are p50 8.8 KB, p90 38 KB, max 153 KB [02 §5.1]; scratchpads reach 7.8 GB and must stay out [02 §13]; zstd dictionaries move small-record ratios from ~2.8 to ~10 (claimed, must be measured on real notes) [05 §12]. | If the owner wants moirai to stay under ~50 MB/year of disk regardless of use, lower the inline cap to 8 KiB and store everything else as pointers. If the owner wants PR-reviewable prose, add the derived Markdown export (never re-imported) rather than moving bodies out. |
| **T7** merge semantics | **Typed per-field 3-way merge, conflicts as data, trunk never advanced while a conflict or violation exists.** Status: monotone lattice with explicit reopen event; enum scalars: equal-or-conflict; counters: never merged, recomputed; sets: add-wins relative to base; text: line diff3, overlapping hunks → conflict value; `parent`: Kleppmann move; edges: set semantics + validators (dangling structural edge, forest, acyclicity, schema). A merge always produces a commit; a dirty one lands on `refs/merge/<lane>` and is resolved by further commits, then fast-forwarded to trunk. | jj stores conflicts as values so merges never block agents; TerminusDB validates before advancing the label; Dolt refuses to commit while `dolt_constraint_violations` is non-empty [04 §5, §8]; identical +3 counter edits merged to 188 when the truth was 191 [02 §7.3]; a merge of two acyclic graphs can be cyclic, and no CRDT prevents DAG cycles [04 §3.11]. | If agents in practice cannot resolve conflicts and the orchestrator always picks "theirs", add a per-kind auto-policy (`lane-wins-for-findings`, `trunk-wins-for-rules`) as data on the schema, keeping the conflict record as an audit entry. |
| **T8** durability | **Durability classes, one flush per commit, no background thread.** `durable` (default for every CLI/MCP write: append record → `NtFlushBuffersFileEx(DATA_SYNC_ONLY)` → publish HEAD slot without flush) and `lazy` (heartbeats, read cursors, session marks: appended and published, flushed by the next durable commit). Group commit only through the v1.5 leader. | A durable commit costs 1.7–2.0 ms p50 here; 64 pages + one flush costs 3.05 ms, so batching is a 40× lever; write-through returned in 0.14 ms and is not trusted [05 §2.2, §6.2]; the workload is ~19 git commits/day and bursts of 16 agents, so one serial writer suffices [08 §1]. | If the owner accepts losing the last ~100 ms of *all* writes after power loss, make every commit lazy with a 100 ms flush deadline enforced by the committing process (still no thread). |
| **T9** agent surface | **CLI (stdin bodies, JSON on demand) + core skill + hooks as primary; MCP with 10 tools for the three Bash-less roles and for typed writes; context packs with explicit budgets and drop footers; leases with fencing tokens; idempotency keys on every write.** | architect, architecture-critic and researcher have no Bash [07 §5.4]; SessionStart cannot call MCP tools, so the CLI is mandatory [07 §4.1]; Workflow resume re-runs completed agents, so idempotency is mandatory [07 §4.1]; PowerShell 5.1 strips quotes from argv [07 §5.2]; hook output is capped at 10,000 characters [07 §4.1]. | If Claude Code starts delivering MCP resource updates to the model, the `changes`-by-hook path becomes optional. If hooks turn out not to fire for Workflow `agent()` calls (unverified [07 §8.6]), the dispatcher pattern (orchestrator claims and `apply`s) is the only path and the SubagentStart pack is dropped. |
| **T10** "from scratch" | **Hand-written:** log/segment formats, HEAD/lock protocol, columnar graph (CSR forward/reverse), bitmaps' frozen layout, op-log versioning, merge engine, validators, FST-less brute-force search at ≤1e4, IPC framing. **Allowed leaf crates:** `zerocopy`, `blake3`, `xxhash-rust`, `zstd` (C, statically linked; `lz4_flex` if C is refused), `roaring` (mutable overlay only), `fst` (v1.1, ≥1e5 nodes), `windows-sys`/`libc`, `rmcp`+`tokio` only in the `mcp` front-end, `bumpalo`, `smallvec`. **Excluded from the core:** SQLite, redb, LMDB, fjall, gix, petgraph, tantivy, any async runtime. | Validated zero-copy access costs as much as deserialising; `zerocopy` views need no validation pass [05 §9.1]; BLAKE3/xxh3 by role [05 §11]; rmcp is the Tier-1 SDK but pulls tokio, so keep it out of the hot CLI [07 §2.7]; Rust building blocks for versioned stores are young (redb multi-process experimental, sanakirja beta) [04 §11]. | If the owner forbids any C code, replace `zstd` with `lz4_flex` and accept ~2× worse body compression. If the owner allows `gix`, use it for `refs/moirai/*` publishing instead of shelling out to git. |
| **T11** search | **Typed filters (GitHub-style `kind:task status:open prio:<=1 area:net`) + graph expansion verbs (`tree`, `blockers --transitive`, `neighbors`) + FTS tiers: brute force over mapped blobs at ≤1e4, FST + delta-varint postings as a segment section at ≥1e5. Embeddings out of the core; an optional int8 vector column later.** | Text2Cypher execution accuracy ~50% [06 §12.2]; filesystem+grep beat mem0-graph on LoCoMo and similarity retrieval is lossy on agent trajectories [03 §6.3]; brute force at 1e4 costs ~5–8 ms, FST at 1e5–1e6 0.1–5 ms [05 §13]. | If the owner's corpus grows past ~1e5 knowledge nodes with long bodies and agents ask semantic questions the filters cannot express, add the int8 vector column (38 MB at 1e5) with an external embedder. |
| **T12** schema | **Fixed core kinds and header fields in the engine (fast path), schema-as-data for project extensions (new kinds, new fields, new enum values), versioned in the same commit DAG; weakening changes merge freely, strengthening changes need an explicit migration commit re-validated at merge; enum integers never reused.** v1 kinds: task, lane, run, question, plan, section, decision, finding, verdict, measurement, rule, note, area (13). | TerminusDB weakening/strengthening [06 §4]; the role templates are a ready-made schema [01 §6.1]; Beads' wide 60-field record grew as orchestration concepts were pushed in [03 §2.2]. | If the owner wants agents to invent kinds freely, allow an `extra` tagged-varint map per node but keep it un-indexed and flagged by `doctor`. |
| **T13** v1 scope | **M0–M2 (below): trunk-only engine + CLI (`ready`, `blockers`, `blocking --ids`, `claim`, `rule --critical`, `brief`, `pack`, `log`, `diff`, `blame`, `show @commit`) + SessionStart/SubagentStart hooks + core skill.** Lane branches and merge are M3; MCP is M4; leader and `refs/moirai` are M5–M6. | The smallest thing that replaces the hand-written HDR, `.slice()` truncation and the MEMORY.md resume block is `brief` + `pack` + leases on a shared trunk [02 §12.5; 01 §8]; three roles need MCP only once they write nodes directly [07 §5.4]. | If the owner's first use is the merge queue rather than briefs, pull M3 ahead of M2. |

---

## 3. Data model

### 3.1 Planes and scope

Every node kind belongs to one plane:

| Plane | Kinds | Where it lives | Who may write | Branch behaviour |
|---|---|---|---|---|
| **coordination** | task, lane, run, question(status fields) | trunk only | any actor, serialized by the writer lock | never branched; visible to all worktrees at once |
| **knowledge** | plan, section, decision, finding, verdict, measurement, rule, note, area, question(text fields) | trunk (authoritative) and lane branches (proposals) | per-role policy (§7.4) | forked sparsely per lane; merged with typed rules |

A lane-branch node carries `flags.proposed = 1` until it is merged. A trunk read never shows proposed nodes unless `--include-proposed lane/x` is given. A lane read shows trunk HEAD plus its own overlay (§5.3).

### 3.2 Common header (all kinds, 56 B fixed, columnar)

| Field | Type | Notes |
|---|---|---|
| `id` | u32 `#N` | row index in the base segment; never reused |
| `uid` | u128 | separate sorted column; identity in canonical commit hashes and exports |
| `kind` | u8 | enum from the core schema or the project schema |
| `status` | u8 | kind-specific enum (below) |
| `resolution` | u8 | for closed tasks and closed findings |
| `priority` | u8 | P0–P4, scheduling |
| `criticality` | u8 | critical/high/normal/low, surfacing order in briefs |
| `flags` | u16 | `deleted`, `suspect`, `has_dangling`, `claimed`, `pinned`, `proposed`, `container`, `stale`, `archived` |
| `plane` | u8 | coordination / knowledge |
| `rev` | u32 | increments on every change; compare-and-set target |
| `parent` | u32 | `#N` or NONE |
| `created_tx`, `updated_tx` | u32, u32 | commit seq numbers; provenance is per commit |
| `last_op_lsn` | u64 | head of the node's op chain (blame, as-of) |
| `open_blockers`, `children_total`, `children_done`, `dangling` | u16 ×4 | derived counters, maintained incrementally |
| `title_off`, `fields_off`, `body_ref` | u32 ×3 | offsets into title blob, field block, blob table |
| `labels` | interned set | in the field block |

`done: bool` is a *derived* accessor: `status == done`. It is not stored twice (Beads #6105 lesson [06 §2.2]).

### 3.3 Node kinds and typed fields

| Kind | Plane | Status enum (lattice order) | Kind-specific fields |
|---|---|---|---|
| **task** | coord | `open < in_progress < done`; side states `deferred`, `cancelled` | `resolution` {completed, wontdo, duplicate, superseded, obsolete}; `work_kind` {design, impl, fix, merge, measure, doc, research}; `assignee` (sym); `lease` {holder sym, token u64, expires u32, pid u32}; `defer_until` u32; `acceptance` text; `estimate` u16; `files_owned` set<glob> (the developer's `claim-files`); `pre_registered` bool |
| **lane** | coord | `active < ready_to_merge < merge_pending < merged`; side `frozen`, `abandoned` | `worktree_path`, `git_branch`, `base_sha` [u8;32]+algo, `tip_sha`, `target_dir`, `moirai_branch` (ref sym), `dirty_files` u16 |
| **run** | coord | `running < (green \| red \| stopped \| died)` | `wf_id`, `bg_task_id`, `session_id`, `script_path`, `args_hash`, `journal_path`, `started`, `ended`, `expected_artifacts` set |
| **question** | coord (status) + knowledge (text) | `open < answered`; side `dropped` | `q_kind` {values, scope, unclear}; `asked_of` {owner, orchestrator, architect}; `options` list<text>; `answer` text (verbatim); `authority` |
| **plan** | knowledge | `draft < approved`; side `superseded` | `rev` u16; `targets` list<{metric sym, value f64, unit sym, op}>; `readiness` list<{item, checked, na_reason}> |
| **section** | knowledge | `active`; side `removed` | `heading` text; `body` blob (diff3-merged); `order` fractional index; `rev` u16 |
| **decision** | knowledge | `proposed < accepted`; side `rejected`, `superseded` | `context`, `what`, `why`, `tradeoff` text; `alternatives` list<{text, rejected_why}>; `authority` {owner, orchestrator, measured, research}; `owner_quote` text (required when authority = owner) |
| **finding** | knowledge | `open < (confirmed \| refuted) < (fixed \| deferred \| withdrawn)` | `local_id` ("C1"); `severity` {critical, important, optional}; `f_kind` {correctness, perf, complexity, security, plan, style}; `failure_scenario` text; `confidence` {confirmed, plausible}; `where` {section ref \| file:symbol@sha}; `round` u8 |
| **verdict** | knowledge | immutable once written | `role` sym; `round` u8; `raw_label` text; `outcome` {pass, pass_with_conditions, fail_fixable, fail_fundamental, unknown, na}; `return_to` {architect, developer, tester, none}; `criteria` text |
| **measurement** | knowledge | `current`; side `stale`(derived), `retracted` | `metric` sym; `value` f64; `unit` sym; `target` f64; `command` text; `measured_on` sha; `env` {host sym, profile sym, load {quiet, loaded}, scale sym}; `baseline` ref |
| **rule** | knowledge | `active`; side `superseded`, `retracted`, `archived` | `enforcement` {must, should}; `applies_to` {roles set, phases set, globs set}; `authority`; `since` u32; `owner_quote` |
| **note** | knowledge | `active`; side `superseded`, `retracted`, `archived` | `note_kind` {note, hazard, lesson, checkpoint, summary}; `applies_to` globs; `observed_git_sha`; `confidence` {verified, observed, inferred, speculative}; `review_after` u32 |
| **area** | knowledge | `active`; side `archived` | `path_globs` set |

A "critical note about the project" is a `note` or `rule` with `criticality = critical`; it sorts first in every brief and pack.

### 3.4 Edge kinds

Edges are `(src, kind, dst)` with set semantics and an optional 8-byte property record `{created_tx u32, pinned_rev u32}`. Both directions are indexed in the same transaction.

| Edge (src → dst) | Class | Plane rule | Acyclic? | On dst deleted | On src deleted |
|---|---|---|---|---|---|
| `parent` (child → parent) | structural | same plane | forest, depth ≤ 8 | **restrict** (or `--cascade`, `--reparent`) | drop; rollups updated |
| `blocks` (A → B) | structural | coord only (trunk) | yes, on `blocks ∪ child→parent` (PK) | drop | drop + event "blocker #A deleted"; `B.open_blockers--` |
| `merge_after` (lane → lane) | structural | coord | yes | drop | drop |
| `runs_in` (run → lane) | structural | coord | n/a | restrict | drop |
| `answers` (decision/note → question) | structural | knowledge → coord | ≤1 active | restrict | drop; question reopens |
| `scoped_to` (knowledge → area) | structural | knowledge | n/a | restrict or reassign to parent area | drop |
| `duplicate_of` (dup → canonical) | structural | same plane | chain length 1 | restrict | drop |
| `supersedes` (new → old) | historical | knowledge | yes | tombstone ref | old stays superseded; doctor warns |
| `refutes` / `confirms` (finding/measurement → finding/decision/rule) | historical | knowledge | no | tombstone ref | — |
| `verifies` (measurement/verdict → finding/task/decision) | historical | knowledge → any | no | tombstone ref | — |
| `derived_from` (note/plan/decision → source) | historical | knowledge | yes by construction | tombstone ref + **src `suspect`** | — |
| `cites` (any → knowledge; `pinned_rev`) | historical | any | no | tombstone ref + **src `suspect`** | — |
| `about` (finding/verdict/measurement → task/section/plan) | historical | knowledge → any | no | tombstone ref | — |
| `discovered_from` (task/finding → task) | historical | any → coord | yes | tombstone ref | — |
| `implements` (task → decision/section) | historical | coord → knowledge | no | tombstone ref | — |
| `addresses` (task/note → finding) | historical | any | no | tombstone ref | — |
| `mentions` (any → any; parsed from `#N` in text) | historical | any | no | tombstone ref, rendered `#40 (deleted c812 …)` | recomputed from text |
| `relates` (canonical direction) | historical | any | no | tombstone ref | — |

Plane rule: a structural edge whose **source** is on trunk never targets a lane-local (proposed) node; the writer refuses it with exit 6 and the hint "merge lane/x first or write on trunk".

### 3.5 Invariants (checked on every write, re-checked at merge)

| ID | Invariant |
|---|---|
| I1 | `#N` is unique across all branches and never reused; `uid` is unique. |
| I2 | Every structural edge has live endpoints in the same visible version. |
| I3 | Historical edges may reference dead ids; those resolve through the tombstone row. |
| I4 | `parent` is a forest (≤1 parent, no cycle, depth ≤ 8). |
| I5 | `blocks ∪ child→parent` is acyclic; no node blocks its own descendant; blockers are inherited from outside the ancestor's subtree only [06 §7.2]. |
| I6 | `supersedes` is acyclic and its target has status `superseded` in the same commit. |
| I7 | The target of `duplicate_of` is canonical. |
| I8 | Status transitions follow the kind's lattice; `blocked` is never stored. |
| I9 | Every derived counter and bitmap equals a full recomputation (`doctor --verify`, property tests). |
| I10 | Every mutation belongs to exactly one commit with provenance (actor, role, session, git head, lane). |
| I11 | Fields conform to the schema version of their commit. |
| I12 | Trunk HEAD satisfies I1–I11 at all times; a merge with unresolved conflicts or violations never advances trunk. |
| P1 | Coordination-plane nodes and their structural edges exist only on trunk. |
| P2 | A lane overlay contains only knowledge-plane ops. |
| P3 | Reverse adjacency equals the inverse of forward adjacency in every visible version. |

### 3.6 Derived state (maintained eagerly, only for affected nodes, defined once)

| Derived | Definition | Update rule / cost |
|---|---|---|
| `open_blockers[n]` | count of `blocks` in-edges whose src is not done, plus exogenous inherited blockers of ancestors | on src → done / reopen / edge add/remove: ±1 along out-edges, O(out-degree); ancestor inheritance walks up ≤ 8 |
| `ready` bitmap | task ∧ `open` ∧ leaf ∧ `open_blockers = 0` ∧ no live lease ∧ `defer_until ≤ now` ∧ no open blocking verdict | recomputed for touched nodes only |
| `is_blocker` bitmap | not done ∧ has out `blocks` to an open task; "ids of all blocking tasks" = this bitmap | O(1) per edge change |
| rollups | `children_total`, `children_done`; `ready_to_close` = all children done | O(depth) on child status change or reparent |
| `suspect` | a source reached by reverse `derived_from`/`cites` was retracted, superseded, deleted, or moved past `pinned_rev` | transitive over derivation edges only, O(closure) |
| `stale` (measurement/note) | `measured_on`/`observed_git_sha` is not an ancestor of the lane tip, or files under `applies_to` changed since | computed lazily per query with a cached `(sha, sha) → bool` ancestry table filled by `git merge-base --is-ancestor` |
| `dangling` | historical edges pointing at dead ids | maintained on delete |
| `diverged` (read-time only, lanes) | the lane overlay set a field whose trunk value changed since the lane's base | computed while merging overlay and trunk row; not stored |
| critical path | longest open path on the precedence DAG under a subtree | on demand, DP over the PK topological order |

---

## 4. Storage engine

### 4.1 Files (≤ 9 per store, all under `<git-common-dir>/moirai/`)

| File | Role | Size / growth | Access |
|---|---|---|---|
| `HEAD` | two 4 KiB checksummed slots | fixed 8 KiB | pread/pwrite; readers take the valid slot with the higher `slot_seq` |
| `LOCK` | empty; byte-range locks only | 0 B | `LockFileEx` bytes: 0 writer, 1 leader, 2 maintenance, 3 quiet-mode advisory |
| `log.NNNN` | preallocated 64 MiB rolling op-log segments (≤ 4 kept live; older ones are sealed and either kept as history or folded into `hist.NNNN`) | ~0.3–0.6 KB per commit (est.) | append + explicit reads; never mapped while active |
| `hist.NNNN` | sealed, zstd-framed history of retired log segments (commit headers + changesets), with a commit index section | ~0.15–0.3 KB per commit after zstd (est., ratio 2–4× on tagged varints) | mmap read-only |
| `seg.base` | materialized trunk state at `upto_lsn` (columns, CSR, bitmaps, symtab, blob table, uid index, idempotency index) | 86 B/node index + bodies (§8) | mmap read-only |
| `seg.d1`..`seg.d3` | tiered delta segments on top of base | small | mmap read-only |
| `blobs.NNNN` | content-addressed body store (BLAKE3-keyed, zstd with dictionary `dict.zst` stored in `seg.base`) | bodies only | mmap read-only |

No file is ever truncated or renamed over while it can be mapped: growth is by new files, reclamation by writing a new file and switching HEAD, deletion of old files tolerates delete-pending and sharing violations with bounded retry [05 §6.1, §6.3; 08 §4 W3–W4]. The store is refused on network and OneDrive paths [08 §4 W11].

### 4.2 HEAD slot (4 KiB, little-endian, `zerocopy`)

```
struct HeadSlot {
  magic: [u8;4] = "MOIR", format: u16, flags: u16 (bit0 = quiet mode),
  slot_seq: u64,               // monotonically increasing per publish
  committed_lsn: u64,          // visibility bound for readers
  commit_seq: u64,             // last trunk commit seq (the change-feed cursor)
  next_id: u32, _pad: u32,     // #N allocator (non-versioned)
  fence: u64,                  // lease fencing-token allocator (non-versioned)
  active_log: u32, n_segments: u8, n_refs: u8, _pad2: u16,
  segments: [SegRef; 8],       // {file_no u32, kind u8, upto_lsn u64, blake3_16 [u8;16]}  = 29 B each
  refs: [RefEntry; 96],        // {name_sym u16, kind u8 (branch/merge/tag), commit_id16 [u8;16], lsn u64, base_lsn u64, gen u32} = 39 B each
  refs_overflow_lsn: u64,      // when n_refs > 96, the full table is a RefTable record in the log
  xxh3: u64                    // checksum over the slot
}
```

96 inline refs cover the 44-worktree case with room; a `RefUpdate` record in the log is the reflog of every ref change, so `moirai reflog trunk` is a log scan filtered by ref.

### 4.3 Log records

Every log record:

```
struct RecHdr { len: u32, kind: u8, flags: u8 (bit0 = lazy), _pad: u16, lsn: u64, xxh3: u64 }   // 24 B
```

Record kinds: `Commit`, `RefUpdate`, `Lazy` (heartbeat/cursor/mark), `RefTable`, `Checkpoint` (segment set change), `Idem` (idempotency result cache), `Noop` (segment padding).

`Commit` body:

```
commit_id      [u8;32]   BLAKE3 over the canonical form (uids instead of #N, sorted ops, no lsn/offsets)
n_parents      u8        (1 for ordinary commits, 2 for merges; 0 for root)
parents        [ {commit_id16 [u8;16], lsn u64} ; n ]      // 24 B each; the 32-byte id is recoverable from the target record
gen            u32       1 + max(parent.gen)   (commit-graph generation number, for LCA and "is-ancestor")
seq            u64       per-store monotonic commit sequence (the change feed)
ref            u16       symbol of the branch this commit was made on
hlc            u64       hybrid logical clock (ms << 16 | counter)
actor, role    u16, u8   symbols: agent label, role enum
session        u32       symbol of CLAUDE_CODE_SESSION_ID
git            { algo u8, head [u8;32], branch u16, worktree u16, base [u8;32] }   // 101 B
idem_key       [u8;16]   BLAKE3-128 of the caller's idempotency key (zero if none)
msg_len        u16, msg  UTF-8
affected_len   u16, affected [u32]   // ids whose derived state changed (for the change feed)
n_ops          u16, ops[]
```

Ops (tagged varints; `#N` as LEB128, values as `(type u8, payload)`; every op that touches a node carries `prev` = delta from this commit's lsn to the node's previous op lsn, so the per-node op chain is a linked list through the log):

| Op | Encoding | Typical size (est.) |
|---|---|---|
| `Create` | tag, node, uid[16], kind, plane, parent, fields… | 40–120 B |
| `Delete` | tag, node, prev, before-image ref (blob hash of the full record) | 24 B (+ blob) |
| `SetField` | tag, node, prev, field sym, old (type, val), new (type, val) | 8–24 B |
| `AddEdge` / `RemoveEdge` | tag, src, kind, dst, prev(src), prev(dst), props | 8–14 B |
| `Move` | tag, node, prev, old parent, new parent, old order, new order | 12–20 B |
| `SetBody` | tag, node, prev, old blob hash[16], new blob hash[16] | 36 B |
| `Schema` | tag, change kind (weaken/strengthen), payload | varies |
| `Conflict` | tag, key, class u8, base/ours/theirs values | 30–200 B |
| `Violation` | tag, class u8, edge or cycle description, suggested resolution | 20–100 B |

A commit that sets two fields and adds one edge is ~230–320 B before compression (est.: 24 header + ~200 commit header + ~40 ops). This matches the 0.3–0.6 KB per small commit estimate in [04 §6] and is 100× below the ~52 KB measured for DoltLite single-row commits [04 §3.2].

### 4.4 Segment layout (sealed, mapped read-only, `zerocopy` views)

```
SegHdr { magic "MSEG", format u16, seg_kind u8 (base|delta|hist|blobs), n_rows u32, base_seq u64, upto_lsn u64,
         n_sections u16, sections: [ {tag u16, off u64, len u64, xxh3 u64} ], blake3 [u8;32] }
```

Sections of `seg.base` (rows are dense by `#N`; dead rows keep their tombstone header):

| Section | Layout | Bytes per row (est.) |
|---|---|---|
| `NODE` | `[NodeHdr; n]` fixed 56 B | 56 |
| `UID` | sorted `[(u128, u32)]` | 20 |
| `TITLE_OFF`, `TITLE_BLOB` | u32 offsets + UTF-8 | 4 + ~60 |
| `FIELDS_OFF`, `FIELDS_BLOB` | u32 + tagged-varint field blocks `(field sym varint, type u8, value)` | 4 + ~24 |
| `BODY_REF` | u32 index into blob table | 4 |
| `OUT_OFF`, `OUT_DST`, `OUT_KIND` | CSR: `[u32; n+1]`, `[u32; e]`, `[u8; e]` (sorted by kind then dst) | 4 + 5/edge |
| `IN_OFF`, `IN_SRC`, `IN_KIND` | reverse CSR | 4 + 5/edge |
| `EDGE_PROPS` | sparse `(edge idx u32, created_tx u32, pinned_rev u32)` | rare |
| `BM_STATUS[k]`, `BM_KIND[k]`, `BM_READY`, `BM_BLOCKER`, `BM_DELETED`, `BM_SUSPECT`, `BM_PROPOSED` | frozen roaring-style containers (array ≤ 4096 values at 2 B, else 8 KiB bitmap, else runs) | ≤ 128 KiB per set at 1e6 |
| `SYMTAB` | sorted string table for kinds, fields, labels, actors, refs | small |
| `BLOBTAB` | `[(blake3_16, blobs_file u32, off u64, len u32, raw_len u32)]` | per unique body |
| `IDEM` | open-addressing table `(key16 → commit lsn)` for the retention window | 24 B per key |
| `CHAIN_TAIL` | optional: last op lsn per row when it differs from `NODE.last_op_lsn` (not needed; kept in NODE) | 0 |
| `FST`, `POST` (v1.1, ≥1e5) | term dictionary + delta-varint postings | ~135 B/node |

Delta segments have the same sections plus an `IDS` sorted column (the rows they override) and a `TOMB` list of edge removals since the base. A reader merges base ← d1 ← d2 ← d3 ← overlay with "last writer wins per row / per edge key".

Topology + metadata cost: 56 + 20 + 8 (CSR offsets) + 10 per edge × 3 edges + ~1 bitmap byte ≈ **115 B per node** (est.; [05 §10.4] gives 80–90 B without the 20 B uid column and the wider header).

### 4.5 Write path (one commit)

1. `try_lock(LOCK byte 0, exclusive)` with jittered backoff up to 2 s (exit 7 on timeout; the holder's pid and start time are in the `RefUpdate`-free `WriterInfo` bytes at `LOCK` 64..128 for diagnostics only).
2. Read the valid HEAD slot; if `committed_lsn` moved since this process's snapshot, replay the tail into the overlay (µs per op).
3. Validate preconditions (`--if-rev`, `--if-status`, lease token, role policy, plane rules).
4. Apply ops to a private working overlay: allocate `#N` from `next_id`, update forward and reverse adjacency, run PK on `blocks ∪ child→parent` for new precedence edges, update derived counters and bitmaps for touched nodes, compute `affected`.
5. Serialize the commit record (compute `commit_id` over the canonical form), append to the active log segment (preallocated, so the write does not change file size [05 §7]).
6. `NtFlushBuffersFileEx(FLUSH_FLAGS_FILE_DATA_SYNC_ONLY)` on the log file (~1.7 ms p50 [M, 05 §2.2]); skipped for `lazy`.
7. Publish: write the other HEAD slot with `committed_lsn`, `commit_seq`, `next_id`, `fence` and the ref move (no flush; recoverable from the log).
8. If the tail since the last checkpoint exceeds 4,096 ops or 4 MiB and quiet mode is off: write a delta segment (10–100 ms est., see §8), publish a `Checkpoint` record and the new segment set.
9. Unlock. Return `{commit_id, seq, rev per touched node, affected, newly_ready}`.

Crash at any step: the log record is either fully present with a valid checksum or ignored; a record that was flushed but never published is re-published by the next opener (the client saw no ack and retries with its idempotency key, which hits the `IDEM` index). An fsync error is fatal for the process; the next opener recovers [08 §8.2].

### 4.6 Read path

1. pread the two HEAD slots (or read the mapped `HEAD` page; ~0.17 ms cold, nanoseconds warm [05 §2.2]); pick the valid slot with the higher `slot_seq`.
2. Map the segment set named in the slot (0.22 ms per map [M]; maps are cached per process and re-used while the set is unchanged). A missing segment file (GC raced) means "re-read HEAD and retry", bounded.
3. Replay log records `(last_seen_lsn, committed_lsn]` into the in-process overlay: sorted `Vec<(id, field, value)>` plus `SmallVec<[u32;4]>` adjacency deltas [05 §10.1].
4. Serve the query from mapped columns + overlay; strings are borrowed from the map, decompressed bodies go into a per-request bump arena.
5. No locks are taken by readers.

Open cost: HEAD read + ≤ 8 maps + ≤ 4,096-op replay = **0.3–3 ms** (est., [05 §14.1]).

### 4.7 Indexes

| Index | Structure | Where |
|---|---|---|
| id → row | direct (dense `#N`) | base + delta `IDS` binary search + overlay |
| uid → id | sorted `UID` column, binary search | segments |
| adjacency forward/reverse | CSR + overlay deltas | segments + process |
| status/kind/ready/blocker/deleted/suspect/proposed | frozen bitmaps + overlay bitsets | segments + process |
| commit id → lsn | fanout(256) + sorted `[(id16, lsn)]` in `hist` and in the active-log side index | history |
| per-node op chain | `NODE.last_op_lsn` → `prev` links in ops | log/hist |
| idempotency key → result | `IDEM` table + overlay map, 7-day retention (est. default) | segments |
| refs and reflog | HEAD table + `RefUpdate` records | HEAD + log |
| text | brute force at ≤1e4; `FST`+`POST` at ≥1e5 (v1.1) | blobs / segments |
| git ancestry cache | `(sha32, sha32) → bool` table in the newest delta | segments |

### 4.8 Checkpoint, compaction, GC (no background thread; only at commit time or on `moirai gc`)

- **Delta checkpoint**: fold the overlay into a new `seg.dK`; tiered: when d3 exists, fold d1..d3 into a new d1. Cost ~O(overlay) plus the rewritten delta.
- **Rollup**: rewrite `seg.base` from base + deltas. 10–30 ms at 1e4, 0.1–0.3 s at 1e5, 1–3 s at 1e6 (est., [05 §7]); at 1e6 only on explicit `moirai gc` or when deltas exceed 25% of base.
- **History retirement**: a full `log.NNNN` is compressed into `hist.NNNN` (zstd frames of 256 commits with a per-frame commit index). Nothing is ever dropped by default; `moirai gc --squash-before <date>` can replace old changesets with checkpoint snapshots while keeping commit headers for lineage (TerminusDB metadata-repository idea [04 §9]).
- **Blob GC**: blobs unreferenced by any live row, any retained history and any ref older than 14 days are dropped when `blobs.NNNN` is rewritten (cruft-pack style delay [04 §3.1]).
- **Quiet mode**: `moirai quiet on` sets HEAD flag bit0; all step-8 checkpoints are deferred and `moirai gc` refuses; the tail may grow (open cost grows linearly, bounded by a warning at 10× threshold). The lane status flag `measuring` on any lane also implies quiet.

### 4.9 Windows specifics

- Locks only on `LOCK`, bytes 0..1024 for roles and diagnostics; never on data files [05 §6.3].
- Segments and blobs are opened with `FILE_SHARE_READ|WRITE|DELETE` and mapped read-only; the active log is written with `WriteFile` and read with `ReadFile` (mapped views and WriteFile are not guaranteed coherent [05 §6.2]).
- Durable flush through `windows-sys` `NtFlushBuffersFileEx(DATA_SYNC_ONLY)`; `FILE_FLAG_WRITE_THROUGH` is never treated as durable [05 §6.2].
- Retry errors 5 and 32 with bounded backoff on deletes and on the derived export's rename; commit path never renames.
- Lock release after a crashed writer may be delayed; the writer wait is bounded and the error names the last known holder [08 §4 W2].
- Signed release binary at a stable install path (Defender re-scans rebuilt executables) [05 §6.4].
- Named pipe (v1.5 leader): explicit DACL for the current user SID, `FILE_FLAG_FIRST_PIPE_INSTANCE`, `PIPE_REJECT_REMOTE_CLIENTS`, name `\\.\pipe\moirai-<sid>-<blake3_16(store path)>` [08 §4 W8].
- Store discovery reads `.git` (file or dir) then `commondir` without spawning git; a miss inside a worktree never auto-creates a store (Beads phantom-DB lesson [03 §2.7]).

---

## 5. Versioning

### 5.1 Objects and refs

| Git concept | moirai concept |
|---|---|
| blob | content-addressed body (BLAKE3, zstd-dict) |
| tree | the materialized state at a commit (implicit; identified by the commit, reconstructible by replay) — no per-commit Merkle root (T1) |
| commit | `Commit` record: parents, generation number, seq, HLC, actor/role/session, git provenance, message, changeset with before-images |
| ref / branch | `trunk`, `lane/<name>`, `exp/<name>` (v2), `merge/<lane>` (staging), `tags/<name>` |
| HEAD (per worktree) | the worktree → branch binding recorded on the `lane` node; the CLI resolves it from cwd |
| reflog | `RefUpdate` records in the log |
| index / staging | none; every write is a commit (jj-style auto-commit). Session-level grouping is a *label* (`--run`, `--round`) on commits, not a separate object |
| `git log` | commit-index walk; `moirai log [ref] [--graph] [--node #N] [--actor] [--role] [--since seq]` |
| `git blame` | per-field last-setter from the node's op chain |
| `git diff` | fold of changesets between two commits, grouped by node and field, rendered as typed hunks |
| `git merge` | §5.4 |
| `git tag` | `RefUpdate` of kind tag; used to pin the state at a git release or a merge |
| `git gc` | §4.8 |
| `git push/pull` | `moirai push/pull` over `refs/moirai/*` (M6) |

Commit granularity: every CLI/MCP write is one commit (auto-commit), because the op log must carry undo and audit for each agent action [04 §3.8]. `moirai apply` makes one commit for a whole batch. History therefore grows by roughly one commit per agent write: at ~11 runs/day × p50 4 agents × ~20 writes ≈ 1k commits/day ≈ 0.3–0.6 MB/day uncompressed (est.), ~0.1–0.2 GB/year after zstd.

### 5.2 History and as-of queries

- `moirai show #40@c812` (or `@seq:4410`, `@2026-09-20T10:00`): walk `#40`'s op chain backwards from HEAD applying inverse ops (before-images) until the chain passes the target commit. Cost O(edits to #40), typically ≤ 10 ops ≈ 10–30 µs (est.). Membership of a chain op in the target's ancestry: on trunk it is `lsn ≤ target.lsn`; on lanes it is `gen`-pruned parent walking (commits with `gen > target.gen` cannot be ancestors [04 §3.1]).
- `moirai log #40`: the same chain rendered forward with commit meta (actor, role, git head, lane, message).
- `moirai blame #40`: for each field and edge, the last op that set it and its commit.
- Whole-graph as-of (`moirai checkout --detached c812` for a read session, or `moirai diff c800..c812 --all`): reverse-apply changesets from HEAD if the distance is ≤ 50k ops (est. 5–50 ms), else map the nearest pinned checkpoint segment set at or before the target and replay forward. Pinned checkpoints: every merge into trunk and one per week (policy, `gc.pin`).
- `moirai changes --since <seq> [--for-agent A] [--touching #12,#17]`: scan commits `(seq, HEAD]` and filter by ids ∪ `affected`. This is the change feed [08 §6.1].

### 5.3 Branches

- `moirai lane open <name> --worktree <path> [--git-branch b] [--base <sha>]` creates the `lane` node (trunk), the ref `lane/<name>` at trunk HEAD with `base_lsn = HEAD.lsn`, and binds the worktree path to the branch.
- The CLI resolves the current branch from cwd → `.git` → worktree path → lane node; MCP calls take `ctx.cwd` from the stamp hook [07 §4.4]; `--branch` overrides.
- A lane branch is **sparse**: only knowledge-plane ops are recorded on it; a coordination-plane write from a lane-bound process is committed on trunk with `git.worktree`/`ref` provenance pointing at the lane.
- **Live-rebased reads** on a lane: `row = trunk_row(HEAD) ⊕ lane_overlay(row)`, field by field. A field set on the lane whose trunk value changed since `base_lsn` is rendered with `~diverged`. A trunk node deleted since the base is rendered as deleted even if the lane cites it (the lane's node gets `suspect`). `--pure` reads the lane at `base_lsn ⊕ overlay` (the git view).
- The lane overlay is rebuilt on open by walking the lane ref's commit chain down to `base_lsn` (O(lane ops); ~1–3k ops per lane in the owner's workflow, est. from ~5–20 findings per round × rounds [02 §12.6]). A lane above 50k ops is promoted to its own delta segment (v2).
- `moirai lane sync` advances `base_lsn` after a clean 3-way merge of trunk into the lane's overlay (a "rebase" of the proposal onto current trunk; conflicts as data on the lane).
- `moirai branch exp/<name> [--from c]` (v2) forks everything including the coordination plane as a snapshot; claims and leases are refused on `exp/` branches; merging back uses the full validator set.

### 5.4 Merge algorithm

Input: `moirai merge lane/x [--into trunk] [--policy P]`.

1. **Base** = `lane.base_lsn` (the fork or last sync point). Because trunk commits are serialized, the LCA is this point by construction; for `exp/` branches and for imported histories the LCA is found by generation-number-pruned parent walks [04 §3.1].
2. **theirs** = the lane overlay (ops since base, folded per key). **ours** = trunk ops since base touching the same keys, found by walking each touched node's op chain back to `base_lsn` (O(touched nodes × their edits since base)).
3. **Partition by key**: `(node, field)`, `(src, kind, dst)`, `(node, existence)`, `(node, parent)`. Disjoint keys commute and apply directly (Pijul's commutation insight [04 §3.10]). Overlapping keys go to the typed merge function for the field's type:

| Field type | Merge function | Result when both sides changed differently |
|---|---|---|
| status (lattice) | join on the lattice; a side state (`deferred`, `cancelled`, `retracted`) vs a forward move → conflict | conflict `StatusFork` unless one dominates; reopen is never produced by a merge |
| enum scalar (severity, confidence, authority, outcome) | equal → fine | conflict `FieldEdit{base, ours, theirs}` |
| number (priority, value, target) | equal → fine; `--policy max` for priority | conflict `FieldEdit` |
| counters (`open_blockers`, rollups) | never merged; recomputed after apply (C9) | — |
| set (labels, applies_to, files_owned, options) | add-wins: `base ∪ (ours−base_removed) ∪ (theirs−base_removed)` minus removals relative to base | no conflict |
| text (body, failure_scenario, what/why) | line-level diff3 against the base blob | overlapping hunks → conflict value `TextHunk{base, ours, theirs}` stored as the field value, rendered with `<<<<<<<` markers on read |
| body blob of a `section` | diff3 + the verbatim-removed-text guard (each removed line must exist in base) | as text, plus `Violation{RemovedTextNotInBase}` |
| `parent` / `order` | Kleppmann move: apply moves in HLC order, skip a move that would create a cycle, log `Violation{HierarchyCycle}`; sibling order = fractional index, HLC tie-break | deterministic, no conflict |
| existence | delete on one side, modify on the other → `DeleteVsModify` conflict; `--policy delete-wins\|resurrect` | conflict |
| `owner_quote`, `authority = owner` | trunk wins; a lane may never override an owner ruling; the lane's change becomes `Violation{OwnerFieldEdited}` | — |

4. **Apply** the merged ops to a candidate overlay on top of trunk HEAD (TerminusDB "commit the layer without advancing the label" [04 §3.3]).
5. **Validate** on the candidate: I2 dangling structural edges (reverse index of nodes deleted on trunk since base ∩ lane edge targets, O(degree)); I4 forest; I5/I6 acyclicity (PK incremental on the candidate; full Kahn only for `exp/` merges); I7; I8 lattice; I11 schema; P1/P2 plane rules; the semantic checks: `duplicate` hint (same kind + same title within the same parent) and `contradiction` hint (two active rules with the same `applies_to` and a `contradicts` edge or identical heading).
6. **Emit the merge commit** with two parents (trunk HEAD, lane tip), the applied ops, and one `Conflict`/`Violation` op per problem, each with a suggested resolution (the edge kind's delete policy for `DanglingEdge`; `theirs` for `FieldEdit` on findings; `ours` for rules).
7. **Advance**: if the merge commit has zero conflict/violation ops, it is appended to trunk in the same writer transaction (fast-forward) and proposed nodes lose `flags.proposed`. Otherwise it is written on `refs/merge/x`, the lane status becomes `merge_pending`, and the CLI prints the conflicts (exit 6). `moirai resolve <key> --take ours|theirs|base|--value V` and `moirai resolve --all --policy P` append resolution commits on `refs/merge/x`; `moirai merge --continue` re-validates and fast-forwards. Tasks whose knowledge is in a pending merge are not excluded from `ready` (they are trunk-plane), but the `pack` for them prints "merge pending: 2 conflicts on plan sections".

Conflict taxonomy (SQLite session classes extended for graphs [04 §3.13, §5]):

| Class | Meaning | Default resolution |
|---|---|---|
| `FieldEdit` | same scalar/enum field changed differently | conflict value stored; per-kind policy optional |
| `StatusFork` | incompatible lattice moves | conflict |
| `TextHunk` | overlapping diff3 hunks | conflict value with markers |
| `DeleteVsModify` | one side deleted, the other modified | conflict; `delete-wins` / `resurrect` policies |
| `DanglingEdge` | structural edge to a node deleted on the other side | violation; suggested = edge policy (drop for `blocks`, restrict for `parent`) |
| `HierarchyCycle` | concurrent reparent | auto: Kleppmann skip + violation log |
| `Cycle` | precedence cycle (only `exp/` merges) | violation; `--policy drop-newest` |
| `IdCollision` | only on import (`uid` clash) | conflict; never overwrite |
| `SchemaConflict` | incompatible strengthening changes | conflict |
| `OwnerFieldEdited` | lane changed an owner-authority field | violation; trunk wins |
| `Duplicate` / `Contradiction` | semantic hints | hint only, never blocks |

### 5.5 Diff

`moirai diff <A>..<B> [--node #N] [--kind finding] [--stat]` folds the changesets on the path between A and B (for trunk: the seq range; for a lane vs trunk: theirs-since-base and ours-since-base side by side, like `git diff base...lane`):

```
moirai diff trunk...lane/demo --stat
lane/demo: 14 commits since base c4410 (2026-09-21 14:02)
  + section ×3 (plan #88: "Narrowphase batching", "Memory layout", "Validation")
  ~ section #91 body: +12 −4 lines, 1 hunk overlaps trunk (c4470 by architect@lane/beta) → TextHunk
  + finding ×9 (5 confirmed, 3 refuted, 1 open) about plan #88
  + measurement ×2 (metric narrowphase_ms, measured_on <sha>, load=quiet)
  ~ rule #12 applies_to +{tester}   (add-wins, no conflict)
  ! cites #40 (deleted on trunk c4455 by dev#2: "dup of #52") → suspect: finding #203
trunk since base: 31 commits, 2 touch keys the lane touched
```

### 5.6 Git linkage

| Mechanism | Detail |
|---|---|
| Store location | `<git-common-dir>/moirai/`; shared by every linked worktree [08 §7.3 G1]; `MOIRAI_DIR` override for tests; refused on network/OneDrive paths |
| Provenance on every commit | `git.head` (+algorithm tag, sized for SHA-256 before Git 3.0 [04 §3.1]), `git.branch` or `detached`, `git.worktree`, `git.base`, lane, actor, role, session |
| Lane ↔ git branch | explicit `moirai lane open` (recommended in the orchestrator's lane-fork step); `moirai doctor lanes` lists worktrees with no lane and lanes whose worktree is gone; an optional `post-checkout` git hook calls `moirai lane bind` |
| Merge trigger | the merge-queue step (the owner's trunk-merge Workflow pattern [02 §5.3 P7]) runs `moirai merge-check lane/x`, then `git merge`, then `moirai merge lane/x`; an optional git `post-merge` hook runs `moirai reconcile`, which detects merged lanes by `git merge-base --is-ancestor <lane.tip_sha> HEAD` and runs the moirai merge if clean, else reports |
| Ancestry queries | shell out to `git merge-base --is-ancestor` (~74 ms [M, 08 §2]) with a persistent `(sha, sha) → bool` cache; v2 may read the `commit-graph` file's generation numbers directly [04 §3.1] |
| Measurement validity | `measurement.measured_on` must be an ancestor of the lane tip when quoted; otherwise `stale`; `moirai check #task` verifies every `cites` pinned revision and every `measured_on` (PLANFENCE-style pinned citations [06 §11.2]) |
| Git commit trailer | optional `prepare-commit-msg` hook adds `Moirai-Commit: <id16>`; `moirai log --git` cross-lists |
| Backup and sync (M6) | `moirai push` writes retired `hist.NNNN` frames and a `head.json` as git blobs in a commit chain under `refs/moirai/data` (Dolt precedent [04 §7]); `moirai pull` imports foreign commits, mapping `uid → #N` and re-verifying canonical hashes; `doctor` checks the fetch refspec and warns about `push --mirror` from clones lacking the ref [08 §7.3 G3] |
| Derived export | `moirai export md --to docs/moirai/` regenerates sorted Markdown views (registers, plan sections) marked `linguist-generated`; never re-imported [08 §7.3] |

---

## 6. Concurrency and sync

### 6.1 Processes and locks

| Process | Role | Store access |
|---|---|---|
| `moirai` CLI (per agent call, per hook) | transient | opens the store directly (§4.6); writes under the writer byte |
| `moirai mcp` (one per Claude session, serves all subagents) | long-lived | same protocol; keeps maps and overlay warm; in v1.5 may hold the leader byte and serve the pipe |
| `moirai watch` (plugin monitor, optional) | long-lived reader | polls HEAD `slot_seq` only when woken by a lease/timer inside the hook cadence; no busy loop (it blocks on the pipe when a leader exists, else sleeps 250 ms between HEAD reads: ~0 CPU) |

Writer protocol: exclusive lock on `LOCK` byte 0; bounded wait with jitter; the lock is released by the OS on crash (delayed) [08 §4 W2]. Readers take no locks. Maintenance (`gc`, rollup) takes byte 2 exclusively and byte 0. Leader (v1.5) takes byte 1; followers forward writes over the pipe and fall back to direct access if the pipe breaks; leader and direct writers are serialized by byte 0, so they can never both write [08 §9 C].

Each process caches: mapped segments (shared pages), the overlay since the segment set's `upto_lsn`, the symbol table, and the lane overlay it is bound to. Cache invalidation is by `committed_lsn`: a new value means "replay the tail", never "drop everything", because sealed segments never change.

### 6.2 Leases and claims (coordination plane, trunk)

- `moirai claim #12 --agent dev#1 --ttl 15m` is one durable commit: the task must be `open`, ready, and without a live lease. Result `{lease: L-<n>, token: <fence>, expires}`. The fencing token comes from `HEAD.fence`, monotonic per store; `complete`, `set`, `release` must carry the lease and a stale token fails with exit 5 (Kleppmann's fencing argument [07 §7.4]).
- Liveness: TTL; `heartbeat` (lazy commit); the holder's pid recorded so `reclaim` can check whether the Claude process is alive; `SubagentStop` releases or flags; `reclaim --older-than 30m` sweeps.
- Compare-and-set on every mutation: `--if-rev`, `--if-status`, `--if-holder`; a failed guard returns the current value (exit 4).
- Idempotency: `--idempotency-key run:<id>/agent:<label>/<n>`; the `IDEM` index returns the original result on retry, which Workflow resume requires [07 §4.1].
- Mutexes for non-task resources (benchmark slot, merge slot): a `task` of `work_kind = mutex` with the same lease semantics.

### 6.3 "Node 40 deleted" end to end

1. **Store (L0)**: a CLI in worktree B (or the MCP server) runs `moirai rm #40 --detach`. Under the writer lock: restrict policies are checked on #40's in-edges (reverse CSR + overlay, O(degree)); `blocks` edges out of #40 are dropped and each dependent gets `open_blockers--`, possibly entering `ready`; historical in-edges stay and their sources get `dangling++`; sources of `derived_from`/`cites` get `suspect`; the node row gets `deleted`, `deleted_tx`; the commit's `affected` lists every touched referrer. One flush, ~2–3 ms. The tombstone view is "#40 deleted c812 by dev#2: 'dup of #52'".
2. **Other processes (L1)**: every read transaction begins with a HEAD read; `committed_lsn` moved, so the tail is replayed (µs). Any process, in any worktree, on any lane branch, now sees #40 as deleted; lane overlays that reference #40 render `suspect`. There is no cache that can hold the old state past the next read.
3. **Change feed (L2)**: `moirai changes --since 4409` returns `c812 rm #40 (task "…") by dev#2 → affected #12 (now READY), #17 (cites → suspect), #203`. With a leader (v1.5), followers receive `{seq, ids}` on the pipe within ~60 µs [M, 08 §2] and `moirai watch` prints the line.
4. **Agent context (L3)**: (a) the next tool result that renders #40 shows the tombstone; (b) the `PostToolBatch` / `UserPromptSubmit` `mcp_tool` hook calls `changes(since=<cursor>, cwd=${cwd}, session=${session_id})` and injects "#40 deleted by dev#2; it blocked your #12 → #12 is READY" as `additionalContext`, relevance-filtered to nodes the agent claimed, owns, is blocked on or cites, and capped at 2,000 characters; (c) any write carrying `--if-rev` on #40, or a `link --blocks #40`, fails with exit 4/3 and the tombstone; (d) a `SubagentStart` pack for a new agent never includes #40.

Bounded staleness at L3 is one tool batch; every write is validated against HEAD, so no write based on a stale read can succeed silently [08 §6.1].

### 6.4 Idempotency and resume

All write verbs and `apply` take a key; keys are hashed to 16 bytes; the result cache retains `{commit_id, seq, ids created (with `$ref` mapping), rev per node}` for 7 days (est. default). `apply` batches with local `$refs` are all-or-nothing and keyed once per run.

---

## 7. Agent interface

### 7.1 CLI (line-oriented, ids first, deterministic order, `--json` on demand, bodies via stdin/@file, exit 0 on empty)

```
# context
moirai brief [--role R] [--lane L] [--budget-chars 8000] [--more]
moirai pack #ID --role R [--budget 12k] [--lane L]
# read
moirai ready [--scope #ID] [--role R] [--limit N] [--cursor C] [--ids]
moirai blocking [--scope #ID] --ids            # ids of all blocking tasks (is_blocker bitmap)
moirai blockers #ID [--transitive] [--explain]
moirai show #ID.. [@COMMIT] [--full] [--neighbors N] [--pure]
moirai find 'kind:finding status:confirmed about:#88' [--text "..."] [--ids]
moirai tree #ID [--depth N]        moirai notes --path src/net/x.rs
moirai changes --since SEQ [--for-agent A] [--touching #..]
# write (every verb: --agent, --idempotency-key, --if-rev, --if-status, --lane)
moirai add task "title" [--parent #] [--blocks #..] [--blocked-by #..] [--field k=v]..
moirai rule|note|decision|finding|verdict|measurement --stdin|@file [--critical] [--about #..] [--applies-to tester,glob]
moirai set #ID k=v.. [--status S] [--lease L]
moirai link #A --blocks|--parent|--cites|--supersedes|--refutes|--answers #B      moirai unlink ...
moirai rm #ID [--detach|--cascade|--reparent] [--dry-run] [--yes]
moirai apply FILE|- [--idempotency-key K] [--dry-run]
# coordination
moirai claim #ID.. | --next [--scope #] --agent A [--ttl 15m]      moirai heartbeat L   moirai release L
moirai complete #ID --lease L --outcome done|failed|abandoned --summary - [--evidence ..]   moirai reopen #ID --reason ..
moirai reclaim --older-than 30m
# versioning
moirai log [REF] [--graph] [--node #ID] [--actor A] [--since SEQ] [--git]
moirai diff A..B | A...B [--node #] [--stat]         moirai blame #ID
moirai lane open NAME --worktree PATH [--git-branch B] [--base SHA]   moirai lane sync|close|freeze NAME
moirai merge-check lane/X        moirai merge lane/X [--policy P]      moirai resolve KEY --take ours|theirs|base|--value V
moirai merge --continue|--abort  moirai tag NAME [COMMIT]              moirai reflog REF
moirai check #ID                 # pinned citations and measured_on still valid?
# store
moirai init | doctor [store|lanes|agents|hooks] | gc [--squash-before DATE] | quiet on|off | push | pull | export md --to DIR
moirai hook session-start|subagent-start|subagent-stop|agent-launched|prompt|stamp      moirai mcp   moirai watch
```

Example outputs:

```
$ moirai blocking --ids
#12
#17
#40

$ moirai blockers #51 --explain
#51 task open P1 "Wire lease reclaim"  blocked
  #12 task in_progress P1 "Byte-range lock protocol"   (direct, lease dev#1 11m left)
  #17 task open P2 "HEAD slot format"                   (direct)
  #7  task open P0 "Storage engine M1"                  (inherited from ancestor #9, exogenous)
… 0 more · rev 4412

$ moirai rule --critical --applies-to tester,developer --stdin <<'EOF'
Never kill processes by image name; only the PID tree you started.
EOF
#212 rule active critical  applies_to={tester,developer}  c4413 by orchestrator (owner_quote: no)

$ moirai show #40
#40 task deleted  (c812 2026-09-25 14:02 by dev#2@lane/beta: "dup of #52")   replaced_by: #52
  referrers: #12 (was blocked_by, dropped → READY), #17 (cites, dangling), #203 (finding about → suspect)

$ moirai log --node #91 
c4470  2026-09-22 09:14  architect@lane/beta   git <sha>..  section #91 body +12 −4  "rev 2: split narrowphase batching"
c4433  2026-09-21 18:40  architect@lane/beta   git <sha>..  section #91 created under plan #88
```

Error contract: stderr `error[guard_conflict]: #12 rev is 19, you passed 17. current: status=in_progress lease=dev#3 (11m). hint: re-read and retry with --if-rev 19` and exit codes 0 ok · 1 internal · 2 usage · 3 not found · 4 guard conflict · 5 lease lost · 6 precondition/merge pending · 7 store locked/unavailable · 8 partial batch [07 §6.2].

### 7.2 MCP tools (10; compact text output, no `structuredContent` by default [07 §2.6])

| Tool | Purpose | Key params | Load |
|---|---|---|---|
| `brief` | session/role digest within a budget | `role`, `lane`, `budget_chars` | alwaysLoad |
| `pack` | per-task, per-role context pack with drop footer | `id`, `role`, `budget_tokens`, `lane` | alwaysLoad |
| `ready` | unblocked, unclaimed tasks | `scope`, `role`, `limit`, `cursor` | alwaysLoad |
| `get` | nodes by id, optionally at a commit, with neighbours | `ids[]`, `at`, `detail`, `neighbors`, `pure` | deferred |
| `find` | filter syntax + text | `query`, `text`, `limit`, `cursor` | deferred |
| `remember` | write a rule/note/decision/finding/verdict/measurement (knowledge plane; goes to the caller's lane branch) | `kind`, `text`, `fields`, `about[]`, `applies_to`, `idempotency_key` | alwaysLoad |
| `write` | batch create/update/link/unlink with guards (tasks, edges, sections) | `ops[]`, `if_rev`, `idempotency_key` | deferred |
| `claim` | claim / next / heartbeat / release | `action`, `id`, `scope`, `agent`, `lease`, `ttl_s` | alwaysLoad |
| `complete` | finish a claimed task; returns newly ready ids | `id`, `lease`, `outcome`, `summary`, `evidence[]`, `idempotency_key` | alwaysLoad |
| `changes` | change feed since a seq, or one node's history | `since_seq`, `id`, `for_agent`, `limit` | deferred |

Every tool accepts `ctx` stamped by the PreToolUse hook (`agent_id`, `agent_type`, `cwd`, `session_id`, HMAC with a per-session secret); the server enforces the role write policy on the stamped `agent_type`, the client allowlist is convenience [07 §9.3]. Versioning verbs (`merge`, `resolve`, `lane`) are CLI-only: they are orchestrator rituals.

### 7.3 Skills and hooks

Skills: `moirai` (core, ~60 lines: verbs, `--agent`, stdin bodies, exit codes, one example per verb; links `reference.md`), `moirai-orchestrate` (lane open → claims → `apply` → merge-queue ritual), `moirai-report` (preloaded into developer/tester/reviewer: how to `complete` and `remember`). All exec-form command hooks call `moirai.exe` directly [07 §9.5]:

| Hook | Action |
|---|---|
| `SessionStart` (startup/resume/compact) | `moirai hook session-start` → brief ≤ 8,000 chars, critical rules first, drop footer |
| `UserPromptSubmit` | `changes` delta since the session cursor; prints nothing if empty |
| `SubagentStart` | role pack: critical rules that apply to `agent_type`, protocol line, the task id if `agent-launched` recorded one |
| `PostToolUse` matcher `Agent` (async) | parse `moirai:task=#51 lease=L-9` from the prompt; record `agent_id → task, lease` |
| `PostToolBatch` (`mcp_tool`, when an MCP server is connected) | relevance-filtered delta as `additionalContext` (§6.3 step 4) |
| `SubagentStop` | release or flag leases; store `last_assistant_message` as a `needs-triage` note if a lease is still open |
| `PreToolUse` matcher `mcp__moirai__.*` | stamp `ctx`; `permissionDecision` per owner choice |

`WorktreeCreate` is not used (it replaces worktree creation) [07 §4.2]; `PreCompact` is unnecessary because `SessionStart(compact)` re-injects.

### 7.4 Role write policy (enforced server-side on the stamped role)

| Role | May write |
|---|---|
| orchestrator | everything on trunk; lane open/merge |
| architect | plan, section, decision (proposed), question; never task status, never verdicts |
| architecture-critic, code-reviewer | finding, verdict about plans/tasks; never fixes (no `addresses`) |
| refuter | `refutes`/`confirms` edges and finding status; never new findings |
| developer | task claim/complete, `files_owned`, implementation notes, question; never verdicts on own task |
| tester | measurement, testrun notes, finding (kind = correctness); never task status except `complete` of its own claim |
| results-analyst | verdict with `return_to`; never fixes |
| owner (via orchestrator with verbatim quote) | `authority = owner` fields; no agent can set them without `owner_quote` |

### 7.5 Context-pack algorithm

`pack #T --role R --budget B --lane L`:

1. Collect candidates in tiers, each item rendered as one line + optional body:
   - T0 (never dropped): the task line, its lease, its lane (tree, branch, base, target dir), critical rules whose `applies_to` matches R or the lane's files;
   - T1: the effective plan sections for T (`implements` edges → sections, folded through `supersedes`), binding decisions with `authority = owner`, open confirmed findings about T or its sections, open questions blocking T;
   - T2: measurements and pins current at the lane tip (with env), other active lanes' `files_owned` (do-not-touch list), rules with `should` enforcement;
   - T3: related notes by area (`scoped_to` of the task's files), the last verdict, prior reports by id/path.
2. Sort within a tier by criticality, then priority, then id (deterministic, cache-friendly).
3. Fill: take T0 in full; for T1..T3, add items while the token estimate (chars/3.5, est.) stays under B; bodies are included for T1 only when the remaining budget allows, otherwise replaced by `#id "title" (body 2.1k chars, moirai show #id)`.
4. Footer: `dropped: 3 notes, 1 measurement, 2 related tasks → moirai pack #T --more` plus `rev <seq>` so the agent can call `changes --since`.
5. Suspect or stale items are included with a `~suspect`/`~stale` marker rather than silently dropped.

### 7.6 Walk-through: one campaign

Roles: orchestrator (main session), architect and architecture-critic (no Bash; MCP), developer and tester (worktree `<lanes-dir>/demo`, branch `u/demo`, CLI).

1. **Session start.** `SessionStart` → `moirai brief` prints the current checkpoint, live lanes (`beta active, tip <sha>, 14 dirty files`), merge queue (`alpha → beta → gamma`), open owner questions and the three critical rules. This replaces the hand-maintained resume block in MEMORY.md [02 §10.2].
2. **Decompose.** `moirai add task "Narrowphase batching" --parent #7` → `#88`; subtasks `#89..#93` with `--blocked-by` edges; `moirai question add --kind scope --blocks #93 …` for the one VALUES question. All on trunk, visible to every lane at once.
3. **Open the lane.** `moirai lane open demo --worktree <lanes-dir>/demo --git-branch u/demo --base <sha>` → lane node `#94`, ref `lane/demo` at trunk HEAD.
4. **Design.** The architect is spawned with `mcpServers: moirai` and the `SubagentStart` pack. It calls `remember{kind:"plan", …}` and `write{ops:[section×6, decision×3]}` with `ctx.cwd = <lanes-dir>/demo`, so the nodes land on `lane/demo` as `proposed`. Its output is now data, not a 70 KB message that no one saved [01 §7 L2].
5. **Critique loop.** The critic calls `pack #88 --role architecture-critic` (only sections changed since the last round plus their `depends_on` dependents: "round scope = delta"), then `remember{kind:"finding", local_id:"C1", severity:"critical", confidence:"plausible", about:["#91"]}`. The refuter marks `#203 refuted --evidence "grep …"`. Termination is the query `find 'kind:finding status:confirmed severity:critical about:#88'` returning nothing [01 §3]. The architect's rev-2 patch is `write{ops:[{set:"#91", body:…}]}`; the removed-text guard is enforced at merge and at write on the lane.
6. **Implement.** The orchestrator claims in bulk: `moirai claim #89 #90 --agent wf:r7/dev#1 --ttl 60m --json`, passes ids and leases through Workflow `args`. The developer runs `moirai pack #89 --role developer --budget 12k`, edits, and returns schema output; `SubagentStop` sees the lease and stores the final message as a `needs-triage` note if `complete` was not called. The orchestrator persists with `moirai apply results.json --idempotency-key run:r7`.
7. **Test and measure.** The tester writes `moirai measurement --metric narrowphase_ms --value 3.9 --target 4.5 --command "…" --measured-on 9ab1… --env host=ryzen9,profile=release,load=quiet --stdin` on the lane; `moirai check #89` confirms `9ab1…` contains the fix commit (ancestry cached).
8. **Verdict.** The results-analyst writes `verdict outcome=pass return_to=none about=#88`. `complete #89 --lease L-9 --outcome done` returns "newly ready: #93".
9. **Merge queue.** The trunk-merge step runs `moirai merge-check lane/demo` (gates, moved pins, findings still open, rulings on trunk the lane predates, `merge_after` prerequisites), then `git merge`, then `moirai merge lane/demo`. Two lanes both edited rule `#12`'s `applies_to` (add-wins, clean) and section `#91`'s body (one overlapping hunk → `TextHunk`). The merge commit lands on `refs/merge/demo`; the orchestrator runs `moirai resolve '#91.body' --take theirs`, `moirai merge --continue`; trunk advances; nine findings, two measurements and six sections lose `proposed`; the lane is `merged`.
10. **Next session.** `brief` shows the merged lane, the two remaining lanes and the one open owner question, in ≤ 8k characters, generated from the graph rather than typed by hand.

---

## 8. Performance and RAM budget

Assumptions (from [05 §3]): 3 edges/node stored twice, ~60 B titles, ~24 B fields, bodies 1 KiB raw → ~340 B zstd-dict (claimed ratio, to be measured), ~10 ops per node lifetime, warm OS page cache, spawn excluded (20–73 ms [M]).

| Quantity | 1e4 nodes | 1e5 nodes | 1e6 nodes | Derivation |
|---|---|---|---|---|
| Index-only segment bytes (node hdr 56 + uid 20 + CSR 8 + 30 edges + bitmaps ~1) | ~1.2 MB | ~12 MB | ~115 MB | 115 B/node (§4.4) |
| + titles, fields, blob table | +0.9 MB | +9 MB | +90 MB | ~90 B/node |
| Bodies (zstd-dict) | +3.4 MB | +34 MB | +340 MB | 340 B/node (claimed ratio) |
| History per 1e5 commits | ~30 MB raw / ~10–15 MB `hist` | same | same | 0.3 KB/commit raw, zstd 2–3× (est.) |
| **Private RSS, CLI** | 1.5–3 MB | 2–4 MB | 3–6 MB | static binary + overlay ≤ 4k ops (≤ ~200 KB) + arena; `more.com` idle is 0.69 MB private [M, 05 §2.4] |
| **Private RSS, MCP server** | 3–6 MB | 4–10 MB | 6–16 MB | + tokio/rmcp (~7–11 MB RSS reported for rmcp HTTP servers [07 §2.7]) + lane overlay (≤ 1 MB at 10k lane ops) + idempotency map |
| **Private RSS, leader (v1.5)** | +2 MB | +4 MB | +16 MB | change-broadcast ring (64k events × 32 B) + warm hash of hot rows |
| **Shared page cache touched** (hot set / whole store) | 1.2 / 6 MB | 12 / 55 MB | 115 / 0.55 GB | index-only / index + titles + bodies; shared by all processes through mapped pages |
| Open store | 0.3–1 ms | 0.5–1.5 ms | 0.5–3 ms | HEAD read 0.17 ms + ≤ 8 maps × 0.22 ms + ≤ 4k-op replay [M, 05 §2.2–2.3] |
| `get #N` | 1–5 µs | 1–5 µs | 1–5 µs | row + overlay lookup + 3–6 page touches |
| `ready` (list 20) | 10–50 µs | 50–300 µs | 0.3–3 ms | bitmap AND + overlay + render |
| `blocking --ids` | 10–50 µs | 50–300 µs | 0.3–3 ms | `is_blocker` bitmap iteration, output-bound |
| `blockers #N --transitive` | 5–50 µs | 10–200 µs | 20 µs–2 ms | reverse CSR walk with visited bitset, O(reachable) |
| `show #N@commit` | 10–30 µs | same | same | op chain ≤ 10 ops, `hist` frame decode ≤ 256 commits |
| Durable commit (small) | ~2.0 ms | ~2.0 ms | ~2.2 ms | 1 flush 1.7–2.0 ms [M] + serialize + PK (µs) |
| Commit with delta checkpoint (every ~4k ops) | +10–30 ms | +20–60 ms | +30–100 ms | delta = overlay fold, O(touched rows) |
| Full rollup (`gc`, explicit) | 10–30 ms | 0.1–0.3 s | 1–3 s | sequential rewrite at 1–2 GB/s [05 §7] |
| Merge of a 2k-op lane | 3–10 ms | 5–20 ms | 10–40 ms | key partition + 2k typed merges + PK on candidate + validators O(touched) |
| Lane overlay rebuild on open (2k ops) | 0.2–1 ms | same | same | log/hist scan of the lane chain |
| `changes --since` (1k commits) | 1–3 ms | same | same | sequential log scan + filter |
| Brute-force FTS | 5–15 ms | (FST) 1–5 ms | (FST) 2–20 ms | [05 §13] |

CI gates (from [05 §17], adjusted): engine ≤ 5 ms per command at 1e5; private RSS ≤ 4 MB CLI / ≤ 10 MB MCP at 1e5; exactly one flush per durable commit; no O(history) or O(store) work on open; merge of 2k ops ≤ 50 ms at 1e5.

---

## 9. Build plan

Relative size (est.): engine (formats, lock/HEAD protocol, segments, overlay, checkpoints) 35%; versioning (commit DAG, chains, diff/blame/as-of, lanes, merge, validators) 25%; CLI + pack/brief 15%; MCP + hooks + skills 10%; test harness (simulation, kill loops, benches, property tests) 15%. Rough total ~18–25k lines of Rust plus ~8–12k of tests (est.).

| Milestone | Scope | Exit criteria | Tests |
|---|---|---|---|
| **M0 Oracle & format spec** (small) | Written on-disk format spec v1 (HEAD, records, segments); the moirai workload implemented against `redb` and `heed` as correctness oracles and baselines [05 §16.1]; fsync/group-commit microbenchmark on the owner's NVMe | spec reviewed; baseline numbers for get/ready/commit at 1e4–1e6 recorded | bench harness with p50/p90/p99, private bytes, flush counts |
| **M1 Engine, trunk only** (large) | log + HEAD + LOCK protocol, node/edge ops, CSR segments + overlay, bitmaps, delta checkpoints, tombstone delete with per-edge policies, PK acyclicity, derived counters/ready, idempotency, durability classes | open ≤ 3 ms at 1e6; one flush per commit; `doctor --verify` clean after 1e6 random ops; 16-process Windows kill-loop for 1 h with no lost acknowledged write and no invariant violation | property tests (incremental == full recompute; reverse == inverse(forward); invert(changeset) restores state); deterministic multi-process simulation with a virtual file/lock layer and crash injection at every I/O boundary, fsync-error injection, lock-release-delay injection; `cargo-fuzz` on record and segment parsers; AV-interference test (a process holding files without `FILE_SHARE_DELETE`) |
| **M2 CLI + brief/pack + hooks + skill** (medium) | the §7.1 read/write/coordination verbs, leases with fencing, `log/diff/blame/show@`, `brief`, `pack`, SessionStart/SubagentStart/SubagentStop/agent-launched hooks, core skill, `doctor agents` | the owner's orchestrator runs one real campaign with `brief` replacing the MEMORY.md block and `pack` replacing HDR; output contract frozen (`--json v1`); CLI private RSS ≤ 4 MB at 1e5 | golden-output tests; hook payload fixtures from Claude Code 2.1.x; PowerShell 5.1 argv tests (bodies via stdin only) |
| **M3 Lanes and merge** (large) | sparse lane branches, live-rebased reads, `lane open/sync/close`, typed 3-way merge, conflict/violation records, `refs/merge`, `resolve`, `merge-check`, `reconcile`, ancestry cache, `check` | 10 synthetic lanes × 1k ops with all conflict classes merge deterministically; merge ≤ 50 ms at 1e5; replaying the owner's recorded register incidents (union-merge resurrecting OPEN, 185→188→191 counter) yields the correct result | property tests: merge is deterministic, disjoint-key merges commute, a clean merge equals sequential application; validators never let a dangling structural edge or a cycle reach trunk (I12 fuzz) |
| **M4 MCP** (medium) | `moirai mcp` on rmcp (dual-era), 10 tools, stamp hook + HMAC ctx, role write policy, plugin packaging | architect, critic and researcher write plans/findings without file access in a real round; schema listing ≤ 5k chars; MCP server private RSS ≤ 10 MB at 1e5 | conformance tests against Claude Code's legacy and 2026-07-28 handshakes; `structuredContent` regression test |
| **M5 Leader & change feed** (medium) | leader byte, named pipe with DACL, forwarding, group commit, broadcast, `watch`, `PostToolBatch` delta hook, quiet mode integration | follower tool p50 ≤ 1 ms; group commit of 16 concurrent writers ≤ 4 ms total; leader death → new leader within 500 ms with no lost write; zero CPU when idle (measured over 10 min) | pipe fuzzing; leader kill loops; idle-CPU gate |
| **M6 Git publishing and export** (small–medium) | `push/pull` over `refs/moirai/data` with uid mapping, `export md`, `Moirai-Commit` trailer helper, `doctor` refspec/mirror checks | a clone on a second machine bootstraps the full history and re-verifies every canonical hash; import of a foreign lane merges with `IdCollision` handling | round-trip property test (push → fresh pull → identical state and hashes) |
| **M7 Search tier & extensions** (medium) | FST + postings section, schema-as-data extensions with weakening/strengthening, `exp/` full branches with full validators, optional vector column | FTS at 1e5 ≤ 5 ms; a strengthening migration commit re-validated at merge | as above plus migration fuzz |

Benchmark gates run on every PR on Windows with Defender on (the owner's real environment) and, separately, on an idle machine; regressions block.

---

## 10. Risks and the top five ways this design could fail

| # | Failure mode | Why it is likely | Mitigation |
|---|---|---|---|
| 1 | **The multi-process file protocol silently loses or corrupts writes on Windows** (delayed lock release, mapped-view/WriteFile incoherence on the tail, AV holding files, fsync errors) | SQLite's WAL-reset race hid for 16 years; ALICE found 60 crash bugs in 11 systems; fsync failure handling is broadly broken [08 §8.1]; Beads lost 7 of 8 acknowledged closes under agent load [03 §2.7] | the tail is never mapped; sealed files never change; one flush before publish; recovery re-publishes flushed-but-unpublished commits; deterministic simulation with crash/fsync/lock-delay injection from M1; 16-writer kill loops as a release gate; fsync error = abort |
| 2 | **Typed merge rules produce plausible but wrong results** (a lattice that hides a real disagreement, add-wins sets resurrecting a removed label, diff3 merging semantically conflicting hunks) | merge semantics creep is the recurring risk in every versioned store [04 §11]; the owner's incidents are exactly silent-merge failures [01 §7 L6] | conflicts as data by default; only sets and hierarchy auto-resolve; owner-authority fields never auto-merge; every merge is replayable and `moirai log --graph` shows both parents; property tests with the recorded incidents as fixtures; per-kind policies are opt-in data |
| 3 | **The two-plane / sparse-branch model confuses agents** (a write meant for a lane lands on trunk, or a lane is never opened and knowledge is never `proposed`) | 44 worktrees with unstable branch identity [08 §7.2]; agents forget protocol [07 §10 R4] | trunk is the safe default (everything is visible, provenance is recorded); lanes are explicit orchestrator rituals; `doctor lanes` and the pack header state the bound branch; `--lane` is stamped by hooks, not typed by the model |
| 4 | **Hooks do not fire for Workflow `agent()` calls, or `additionalContext` limits change**, so the L3 propagation and the SubagentStart pack never reach agents | unverified [07 §8.6]; hook strings capped at 10,000 chars [07 §4.1] | the dispatcher pattern (orchestrator claims, `pack` output passed via `args`, `apply` on return) needs no hooks; a 5-minute experiment is the first M2 task; briefs self-budget with drop footers |
| 5 | **The from-scratch engine takes far longer than the design suggests** ("95% of the effort is testing" [04 §3.14]), and the owner's workflow keeps drifting meanwhile | redb needed years for experimental multi-process; DoltLite ~2,000 PRs to beta [04 §3.2] | M0 oracles let M2's CLI ship on `redb` if M1 slips (the format spec, not the engine, is the contract); the versioning layer is engine-agnostic (op log + materialized state interface); scope M1 to trunk only |

Further risks: git SHA links rot after rebase/squash (provenance only, `uid`-keyed identity never depends on them); Git 3.0 SHA-256 default (32-byte fields from day one); zstd dictionary ratio unmeasured (fall back to no-dict, measure on real notes in M0); Defender exclusions cannot be assumed (≤ 9 files, in-place writes).

---

## 11. Decisions the owner must make (value/scope calls only)

| # | Decision | Recommended default |
|---|---|---|
| 1 | **Store bodies in moirai or pointers only?** Sets the disk footprint by ~2 orders of magnitude and whether diff3/removed-text guards can work. | Inline up to 64 KiB with dedup and dictionary compression; artifacts as path + sha256 pointers. |
| 2 | **Will moirai ever be pushed, cloned or written from another machine or cloud session?** Decides whether `uid` and `push/pull` are v1 or later. | Keep `uid` from day one (cheap); ship `push/pull` in M6, single-machine until then. |
| 3 | **Lane branches for knowledge, or trunk-only with provenance and a `proposed` flag?** | Lane branches (M3): the register-merge incidents show branch-scoped knowledge is real; revisit after one campaign. |
| 4 | **Default delete policy for `blocks`**: drop-and-notify (dependents may become ready) or restrict? | Drop-and-notify; `parent`/`answers`/`scoped_to` restrict. |
| 5 | **May the read-only roles (architect, critic, researcher) write their own node kinds through MCP** while keeping no file Write/Edit? | Yes, under the server-side role policy (§7.4). |
| 6 | **Auto-approve moirai MCP calls via the stamp hook, or prompt on writes?** | Auto-approve reads and lane-plane writes; `ask` for `rm`, `merge`, owner-authority fields. |
| 7 | **Language of stored text** (memory is Russian, repository artifacts English). | English for all node text; owner quotes stored verbatim in whatever language they were given. |
| 8 | **Retention**: all history forever, or squash changesets older than N months into checkpoints (headers kept)? | Forever by default; `gc --squash-before` available; ~0.1–0.2 GB/year est. |
| 9 | **Quiet mode signalling**: explicit `moirai quiet on/off` in the window driver script, or inferred from a lane status `measuring`? | Both; the explicit flag wins. May the MCP server stay resident (0% CPU) during windows? Recommended yes. |
| 10 | **Store location**: `<git-common-dir>/moirai/` (lost with the clone; one per repo) or `%LOCALAPPDATA%\moirai\<repo-id>` (survives clone deletion; fragile repo identity)? | Common dir, with M6 `push` as the backup. |
| 11 | **Migration**: import the 254 memory files and the registers as `note`/`rule`/`question` nodes with `derived_from` pointers, or start clean? | Start clean; import the ~50 `feedback-*` rules and open questions only, with `authority` set from their wording. |
| 12 | **Dev Drive / Defender exclusion** acceptable for the store and worktrees? | Optional speed-up only; the design must meet its gates on plain NTFS with real-time scanning on. |
