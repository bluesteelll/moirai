# 71 — Audit of the integrated design on the MINIMAL RAM axis

*Audit of the design of record after the R4/R5 integration and roadmap v2. Date: 2026-09-26. Status: design review only; nothing is implemented. Inputs read: `docs/ARCHITECTURE-RESEARCH.md` in full ([AR], sections 0–12 and the Review log); `research/design/40-file-links-design.md` ([40], rev 2, incl. §2.5–§2.7, §4.2–§4.8, §7 and the Review log); `research/design/50-query-language-design.md` ([50], rev 2, incl. §5.5–§5.12 and the review-log dispositions); `research/design/60-roadmap.md` ([60], issue 2, incl. §2.5, §2.6, §3.5, §4, §5); the critique [20] (perf/RAM/Windows), [41 §1] and [51 §4.2]; research reports [02], [05], [07], [08], [10] and, where they add Windows memory facts, [17]. Web sources checked on 2026-09-26 are listed in §9.*

---

## 0. Summary

**Verdict.** The design keeps its central RAM idea intact — immutable, read-only-mapped segments shared by every process, no daemon, no buffer pool — and the per-query `mem` budget of [50 §5.10] is sound. But the private-memory target ("single-digit MB per process", [05 §2.4]; gates "≤ 4 MB CLI, ≤ 10 MB + 1 MB × min(active branches, 8) MCP at 1e5", [AR §8.1]) is not met by the specification as written, and several paths have no memory bound at all. Recomputed from the documented layouts, the as-written worst cases are:

| Process kind | Stated | Recomputed as written (worst realistic case) | Cause |
|---|---|---|---|
| CLI/hook on `main`, quiet window | 2–4 MB | 3.8–9.3 MB | 8× tail cap replayed by every process (RAM-M5) |
| CLI reading a 14-day, daily-synced lane | +0.2–1 MB | +1–5 MB typical, up to +8 MiB | lane view carries `main`'s synced windows (RAM-M1) |
| read verb resolving an anchor | +≤ 0.3 MB + file | +3.7–4 MB at the owner's largest file, +32 MiB at `files.max-read-bytes` | whole-buffer file functions (RAM-M4) |
| `TX` at the default cap (10,000 ops) | not budgeted | +1.9–2.4 MB (agent max 50k ops: +9.5–12 MB) | write working set outside `mem` (RAM-M6) |
| MCP server after a 16-lane fan-out | ≤ 18 MB | 12–45 MB, retained after eviction | overlay sizes × K = 8; heap retention (RAM-M1, RAM-M2) |
| MCP server during a rollup | ≤ 18 MB | +17–35 MB transient, at every scale | dictionary retrain + TOPO rebuild inside the server (RAM-M3) |
| import of the default (checkpoint) image | not budgeted | 70–100 MB at 1e5; ≈ 0.7–1 GB at 1e6, and the commit record no longer fits a 64 MiB log extent from ≈ 0.37 M nodes | one commit holds the whole store (RAM-B1) |
| all moirai processes, 16 sessions + 16 concurrent CLI/hooks | not stated | ≈ 0.1–0.35 GB on a fan-out day; up to ≈ 1.1 GB for the servers alone | per-process overlays × 16 servers (RAM-M2) |

The findings: **1 blocker, 6 major, 9 minor**. The blocker and parts of two majors are **format items that must enter the M0 freeze** (a bounded commit record with a segment-backed changeset for bulk commits; byte-bounded `hist` frames; an actor-symbol width that does not overflow); everything else is engine policy, store parameters, `config` defaults or gates, and none needs an owner decision: no third-party code, no interim stage, no daemon, no data leaving the machine. With the fixes, every process kind fits the single-digit-MB target at 1e4–1e6 except explicit bulk maintenance (rollup, `doctor --verify`, repair, checkpoint import), which gets its own bounded budget of ≤ 32 MB at 1e6 in a transient low-priority process. §6 is the proposed budgets table with a gate per row.

**Top five fixes, in order of RAM saved.**
1. **Promote branches eagerly** (on sync, at each checkpoint for branches that synced, and after a rollup for branches pinned to the superseded base), and cap a branch overlay at 1 MiB including sync windows (RAM-M1). This moves per-branch state from private memory in every process into one shared mapped segment and stops old pinned bases from duplicating the hot index in the page cache.
2. **Bound the MCP server by bytes, not by K**: Σ branch overlays ≤ 4 MiB, per-view region arenas released whole, and an aggregate gate for 16 servers (RAM-M2).
3. **Take rollup out of the MCP server**: a detached, short-lived, low-memory-priority `moirai gc --rollup --if-needed` child with a streaming rollup ≤ 32 MB at 1e6 (RAM-M3).
4. **Reserve a segment-backed changeset in format v1** so no commit — checkpoint import, `migrate`, `rm --cascade`, `links import`, a large `TX` — is ever held whole in private memory or in one log record (RAM-B1).
5. **Stream every read of a project file** with fixed buffers (two-pass `oid`, chunked anchor search), so `files.max-read-bytes` stops being a private-memory size (RAM-M4).

---

## 1. Scope, method and conventions

**Axis.** Private memory (Windows commit charge: `PeakPagefileUsage`, the metric [60 §5.1] gates) and shared page-cache use (file-backed pages of mapped segments and of files read through the cache), per process kind: CLI, MCP server, hooks, settle passes, merge/sync, image export/import, LQ query execution, the file-link resolver including fingerprints and sketches, and the **full-recompute production paths**. I read the task's "reference-free production paths" as the product's own from-scratch recomputations — `doctor --verify`, `repair --rebuild-from-log`, rollup, full Kahn at merge, checkpoint import, backup — which run without the reference model and must not inherit its materialise-everything strategy (the model itself runs only at ≤ 2e3 nodes on the test host, [60 §4.4] item 8, so its RAM is out of scope).

**Scales and load.** 1e4 / 1e5 / 1e6 nodes (the owner's three-year scale 0.3–0.5 M sits between the last two, [02 §12.6]); 16 concurrent agent processes; 1.8 GB free of 15.8 GB with 16 `claude`/`node` processes holding 3.5 GB private [M, 05 §2.4]; ~50 live branches worst case, 3–5 live lanes typical, p90 14 agents per Workflow run in separate lanes through one session [M, 02 §5.1]; ~1k commits/day store-wide [AR §8.1]; 4 concurrent sessions typical [20 §1.2] and 16 sessions measured [M, 08 §5.2].

**Tags.** [M] measured on the owner's machine (by the report cited); [D] vendor documentation; **claimed** third-party; **est.** arithmetic with inputs shown. Every finding has a location, a failure or cost scenario with numbers, and a fix; each fix says whether it is a format item (must be in the M0 freeze), an engine rule, a store parameter or `config` default, or a gate.

**Not re-raised** (already resolved in the documents; §7 lists what I checked): F-D1 streaming overlay build and G15 [20]; F-D4/G19 per-lane MCP growth and the restated gate [20]; F-D8/G24 lazy parent trees and explicit export budgets [20]; [51] M8's composition of query budgets into one `mem` [50 §5.10]; one overlay at a time for `across()` in the CLI [50 §5.8]; hand-written frozen bitsets instead of copying `roaring` (F-C4). Where a finding here touches one of them, it says what is new.

---

## 2. Unit costs used in the recomputation

| Component | Bytes | Source / tag |
|---|---|---|
| Native console process baseline | 0.69 MB private | [M, 05 §2.4] (`more.com`) |
| Rust CLI baseline (std, static CRT, the binary's `.data`/`.bss`, heap init, argv/env) | 1.0–1.5 MB | est.; [05 §2.4] "about 1–3 MB private (ESTIMATE)" |
| Hot index per node (header 60 + CSR offsets 8 + edges 30 + bitsets ~1) | 99 B, shared | [AR §4.4], [20 §1.1] |
| Whole store touched per node | ~540 B, shared | [AR §8.1] |
| `main`-tail overlay | ≤ 4,096 ops × 80–200 B = 0.33–0.82 MB | [AR §8.1] |
| Quiet-mode tail cap | 8× the threshold: 32,768 ops / 32 MiB records → 2.6–6.5 MB decoded | [AR §2.2, §4.5 step 10] |
| Branch overlay entry | ~150 B per op or folded key | [AR §5a.3] |
| `main` churn folded per key | ≈ 1.5–4k distinct keys per day at 1k commits/day | [20 F-D2] |
| Lazy runtime rows (`FileObs`, `Pending`, `FPrint`, `GitFacts`) | 96–300 B each on disk; ~150–300 B decoded | [40 §2.6] |
| zstd decompression context + digested dictionary (by reference) | ≈ 0.1–0.2 MB | est. (zstd `ZSTD_estimateDCtxSize`, `ZSTD_estimateDDictSize`; confirm at M0) |
| zstd compression context + CDict, level 3, 110 KiB dictionary | ≈ 1–2 MB (level-3 parameters for ≤ 256 KB inputs use 2^17 hash and 2^16 chain entries of 4 B in each of CCtx and CDict) | est. (confirm with `ZSTD_estimateCCtxSize_usingCParams` / `ZSTD_estimateCDictSize` at M0) |
| deflate / inflate state (zlib defaults) | (1 << 17) + (1 << 17) = 256 KiB + a few KB / 32 KiB + ~7 KB | [D, zlib `zconf.h` memory formula] |
| Query `mem` | min(1 MiB, gate headroom), ≥ 256 KiB; agent max 2 MiB CLI / 4 MiB MCP | [50 §5.10] |
| Visited set | N/8: 1.2 / 12 / 122 KiB | [50 §5.5] |
| Body | ≤ 64 KiB raw; ~40 decompressed per pack | [AR §2.6, §8.1] |
| Thread stacks | main thread 1 MiB reserve (MSVC link default); Rust spawned threads 2 MiB reserve; tokio threads 2 MiB; committed only as touched | [D] Rust `std::thread`, tokio `Builder::thread_stack_size` |
| tokio blocking pool | up to 512 threads, 10 s keep-alive; `stdin` "implemented by using an ordinary blocking read on a separate thread" | [D] tokio docs |
| git delta-base cache (git's own default) | 96 MiB per thread | [D] Git for Windows 2.54 `git-config` |
| Page tables | 8 B per touched 4 KiB page + one 4 KiB table page per touched 2 MiB region, private per process | est.; visible in VMMap, not in `PeakPagefileUsage` |
| Owner's project files | p50 10.2 KB, p99 333 KB, max 1.87 MB | [M, 10 §5.1] |

---

## 3. Recomputed private RSS and shared page cache per process kind

"As written" follows the documents literally; "Fixed" assumes the fixes of §5. Private figures are peak private bytes; shared figures are the file-backed pages one invocation brings into the page cache (cold) and, in the last column, what stays shared across all processes.

### 3.1 Private memory

| Process kind | Stated ([AR §8.1], [40 §7.3], [50 §5.12], [60 §5.4]) | As written, 1e4 / 1e5 / 1e6 (est.) | Fixed, 1e4 / 1e5 / 1e6 (est.) | Findings |
|---|---|---|---|---|
| CLI read on `main` (`get`, `ready`, `blocking`, `q` within default budgets) | 1.5–3 / 2–4 / 3–6 MB | 1.5–3.6 / 1.5–3.6 / 1.6–4.1 MB (baseline 1.0–1.5 + tail 0.33–0.82 + zstd D 0.1–0.2 + `mem` ≤ 1 MiB + output ≤ 0.1) | same, ≤ 3.5 / 3.5 / 4 MB gated | — |
| same, during a quiet window | not stated | 3.8–9.3 MB at every scale (tail ×8) | ≤ 4 MB (overlay capped in bytes, compact form) | RAM-M5 |
| same, after a settle or `links sync` wrote runtime rows | not stated | + up to 4–6 MB (4 MiB of lazy records decoded eagerly) | + ≤ 0.5 MB (indexed, decoded on probe) | RAM-M5 |
| CLI reading a lane | +0.2–1 MB | unsynced 14-day lane +0.3–0.9 MB; **daily-synced 14-day lane +1–5 MB, up to +8 MiB** before promotion | + ≤ 1 MiB | RAM-M1 |
| CLI write, small (`set`, `claim`, `complete`) | in CLI row | + scratch copy of the overlay 0.33–0.82 MB | + ≤ 0.1 MB (copy-on-write layer) | RAM-M6 |
| CLI write with a body (`remember`, `rule --stdin`, `doc patch`) | in CLI row | + zstd CCtx/CDict 1–2 MB + scratch copy → 3–5.4 MB | ≤ 4 MB (bodies compressed at seal time, or small-window parameters) | RAM-M6 |
| `TX` / `apply` | "< 64 KiB" per [50 §5.12] Q18–Q25 | + ~190–240 B per op: default cap 10k ops +1.9–2.4 MB; agent max 50k +9.5–12 MB; orchestrator 10× +95–120 MB | charged to a write budget ≤ 1 MiB (≈ 5k ops); larger → segment-backed commit | RAM-M6, RAM-B1 |
| `pack` / `brief` with L2 bodies | arena ≤ 256 KiB | + up to 40 × 64 KiB = 2.5 MiB of decompressed bodies in one arena | + ≤ 128 KiB (one reused body buffer) | RAM-m7 |
| Read verb resolving anchors (R4) | +≤ 0.3 MB + largest anchored file + trees ≤ 256 KiB | + file + normalised copy + line hashes: owner p99 file +0.7 MB, max file +3.7–4 MB, at `files.max-read-bytes` (16 MiB) +32 MiB | + ≤ 0.5 MB (fixed 128 KiB buffers, capped line-hash array) | RAM-M4 |
| Hook: `SessionStart` (brief + settle ≤ 150 ms) | ≤ 1 MB (R4 part) | 2.5–4.6 MB + git tree reads (no cache bound specified) | ≤ 4 MB | RAM-m1 |
| Hook: `SubagentStart` with `sync --check` + auto-apply | not stated | + 0.3–0.6 MB per day the lane is behind; 14 days behind +4–8 MB | ≤ 4 MB; auto-apply refused above a key threshold | RAM-M6 |
| Hook: `fs-evidence`, `stamp`, `prompt` | CLI row | 1.5–3 MB | same | — |
| Settle: `links sync --all` | ≤ 1 / 2 / 8 MB at 1e3 / 1e4 / 1e5 linked files | + per-directory maps × up to 8 threads (≤ 8 MB) + whole-file reads (RAM-M4) | ≤ 8 MB at 1e5 linked files | RAM-M4 |
| `links check --deep` | ≤ 2 / 4 / 16 MB | 8 threads × (candidate file + normalised copy): owner max file ≈ 30 MB; at 16 MiB files ≈ 256 MB | ≤ 16 MB | RAM-M4 |
| MCP server, idle | 3–6 / 4–8 / 6–12 MB + 1 MB × min(active, 8) | base 2.1–4.7 MB (baseline + tokio/rmcp 0.5–2 **claimed** + stdin thread + tail + R4 LRU 0.25) + K × lane overlay (0.3–5 MB each, up to 8 MiB) + retained heap → **12–45 MB after a 16-lane fan-out**, retained after eviction | ≤ 8 MB + Σ overlays ≤ 4 MiB | RAM-M1, RAM-M2, RAM-m2 |
| MCP server, during a query | + `mem` ≤ 4 MiB | same | same (inside the 16 MB peak gate) | — |
| MCP server, during a rollup | not stated | + 17–35 MB transient at every scale, peak retained in the heap | 0 (rollup never runs in the server) | RAM-M3 |
| Merge / sync (2k-op lane vs 5k `main` ops) | time only | 2–5 MB at 1e4–1e5; 15–25 MB at 1e6 when full Kahn runs (> 1,000 precedence edges); a 50k-op lane 20–30 MB | ≤ 8 MB at 1e5, ≤ 16 MB at 1e6 | RAM-M6, RAM-m4 |
| Delta checkpoint / promotion (maintenance holder) | time only | tail or overlay size (≤ 0.8 MB; ≤ 8 MiB for a promotion) + writer buffers | ≤ 8 MB | RAM-M1 |
| Rollup (explicit `gc`, or MCP) | time only | 17–25 MB at 1e4–1e5 (dictionary training ≈ 11 MB of samples + ≈ 6 MB of trainer tables dominates), 20–35 MB at 1e6 (+ TOPO rebuild 8–12 MB) | ≤ 24 / 24 / 32 MB in a transient low-priority process | RAM-M3 |
| `doctor --verify` | not stated | naive: counters 8 MB + ~80 bitsets 10 MB + inverse CSR 24 MB ≈ 40–60 MB at 1e6 | ≤ 16 / 16 / 32 MB (chunked by 65,536 ids) | RAM-m4 |
| `repair --rebuild-from-log` | not stated | if replayed into memory (the model's strategy): ≈ 0.5–1 GB at 1e6 | ≤ 32 MB (checkpoint pipeline from lsn 0, then a streaming rollup) | RAM-m4 |
| `backup` | time only | ~1 MB private; **2× store size through the cache** at normal memory priority | same private; low memory priority, sequential/unbuffered copy | RAM-m5 |
| `image export`, incremental checkpoint (3 nodes) | 5–20 ms | ~0.5–1 MB (lazy trees ~20 KB, deflate 256 KiB, pack writer) | same | — |
| `image export`, full | ~2 / 12–15 / 110–150 MB | as stated | ≤ 2 / 6 / 10 MB (packs of ≤ 65,536 objects) | RAM-m3 |
| `image import`, commit granularity, per commit | not stated | ~1 MB (tree diff + `.moi` parse + overlay) | ≤ 4 MB gated | RAM-m3 |
| `image import`, checkpoint granularity (the default), fresh store | not stated | one commit with every node: ≈ 7–10 / 70–100 MB / 0.7–1 GB; record ≈ 180 B/node of ops (+ 340 B/node of body records) exceeds a 64 MiB extent from ≈ 0.37 M nodes | ≤ 8 / 16 / 32 MB (segment-backed bulk commit, streamed canonical hash) | RAM-B1 |
| LQ query (any view) | ≤ `mem` | ≤ `mem`, except R4 scratch (RAM-M4) and whole-graph as-of beyond ≈ 6.9k ops (E303 by design) | ≤ `mem` | RAM-M4 |
| Git object layer inside any of the above | "decoded git trees ≤ 256 KiB" | no cache bound specified; a git-like delta-base cache defaults to 96 MiB per thread | ≤ 256 KiB CLI/hooks, ≤ 1 MiB MCP | RAM-m1 |

### 3.2 Shared page cache

| Process kind | Pages brought in per invocation (cold), 1e4 / 1e5 / 1e6 (est.) | Stays shared across processes |
|---|---|---|
| CLI point read (`get`, `show`) | 4–6 pages + read-around clusters: ~0.1–0.3 MB at every scale | hot index: 1.0 / 9.9 / 99 MB total, once for all processes |
| `ready` page, `blocking --ids` | bitsets (sorted arrays or 8 KiB chunks; ≤ 128 KiB per set at 1e6) + ~20 rows: ~0.05 / 0.2 / 0.5 MB | same |
| `pack` / `brief` | a few hundred rows + ~40 bodies from `blobs`: ~1–3 MB | same, plus blob pages |
| LQ column scan (unanchored filter on a header column) | `NODE` column: 0.6 / 6 / 60 MB (1 work unit per row; the 2e6 default admits it at 1e6); + field blocks 0.24 / 2.4 / 24 MB | same |
| FTS tier 1 with `--bodies` | 3.4 / 34 / 340 MB of blob pages (tier 2 above 20k nodes avoids this for titles and abstracts) | same |
| Lane read on an old pinned base | a second copy of the hot pages it touches; with b distinct bases in use, up to b × hot index: 2–4 bases → 2–4 / 20–40 / 200–400 MB | **duplicated per base** (RAM-M1) |
| Settle / `links check --all` | `FILEOBS` 96–150 B per (file, tree), `PATHIDX` ~55 B per path; NTFS metadata of enumerated directories | 0.3–0.45 / 2.9–4.5 / 29–45 MB at 1e3 / 1e4 / 1e5 linked files [40 §7.3] |
| `--deep` stage 1 | `FPRINT` ~300 B per content version: 0.36 / 3.6 / 36 MB at 1e3 / 1e4 / 1e5 files | same |
| Git object layer (tree gate, E6, ancestry) | pack/idx/commit-graph pages of the project repository (mapped) | shared with `git` itself |
| Merge / sync | touched rows + the log/`hist` windows: ~1–5 MB | same |
| Rollup, `backup`, full export, repair | reads the whole store and writes a new copy: ~2× 5.5 / 55 / 550 MB through the cache; export also writes 2 / 20–25 / 200–250 MB of pack | **evicts other processes' standby pages** unless issued at low memory priority (RAM-m5) |

### 3.3 System aggregate: 16 agents, S sessions, ~50 branches

| Scenario | As written (est.) | Fixed (est.) |
|---|---|---|
| 16 concurrent CLI/hook processes at 1e5 | 16 × 2–4 MB = 32–64 MB; in a quiet window 16 × 3.8–9.3 = 61–149 MB; each reading a synced lane: +16–80 MB | ≤ 16 × 4 MB = 64 MB |
| MCP servers, S = 4 (one fan-out session + 3 others) | 12–45 + 3 × (3–8) ≈ 21–69 MB | ≤ 4 × 12 = 48 MB |
| MCP servers, S = 16 [M, 08 §5.2] | one fan-out server 12–45 MB + 15 × 3–10 MB ≈ 57–195 MB typical; if every session reads 8 synced lanes: 16 × 30–70 ≈ 0.5–1.1 GB | ≤ 16 × 12 = 192 MB peak, ≤ 16 × 8 = 128 MB idle |
| + one MCP-resident rollup | +17–35 MB (and the peak stays in that server) | 0 in servers; one transient child ≤ 24–32 MB |
| Shared page cache, 50 branches, 1e6 | hot index 99 MB × distinct bases in use (2–4) = 200–400 MB, + whatever scans touch | ≈ 99–200 MB (≤ 2 bases resident) |
| **Total private, typical (S = 4)** | ≈ 55–135 MB (quiet window: up to ≈ 220 MB) | ≤ 112 MB |
| **Total private, worst (S = 16)** | ≈ 0.1–0.35 GB on a fan-out day (CLI/hooks 32–144 MB + servers 57–195 MB); up to ≈ 1.1 GB for the servers alone | ≤ 256 MB |

Against 1.8 GB free, the as-written worst case uses up to 60 % of the headroom in private memory alone, before the page cache; the fixed worst case stays under 15 %.

---

## 4. Where the multiplication comes from

Three structures are private, rebuilt per process and therefore multiplied by the number of processes: the `main`-tail overlay (every process replays it), branch overlays (every process that reads a branch builds its own; the MCP server keeps up to 8), and any cache a long-lived process fills (query arenas, the R4 resolution LRU, the git object layer's caches, and the heap high-water mark left behind by rollups and large merges). Two structures are shared but duplicated: the hot index of each distinct pinned base segment, and every file a bulk path reads through the cache. Two are proportional to history or to all branches: markers and symbols (never pruned), and a commit whose size is unbounded (checkpoint import, `migrate`, cascades, bulk `TX`). The findings follow this map.

---

## 5. Findings

Severity: **blocker** — the design cannot meet a requirement at the owner's scale and the fix must enter the M0 format freeze; **major** — a stated RAM gate is exceeded on a normal daily path, or a path has no bound and can exceed the target by an order of magnitude; **minor** — a bounded excess, a specification gap or a measurement gap.

### RAM-B1 (blocker, format) — Commit size is unbounded; one commit can hold the whole store in private memory and in one log record

**Where.** [AR §4.5 steps 6–7] ("Apply ops to a scratch copy of the branch overlay"; "append it and any `Lease`/`Marker` records in one write"); [AR §4.1] (`log.NNNN` "64 MiB extents"; `hist.NNNN` "sealed zstd frames of 256 commits"); [AR §5b.4, §5b.6 step 2] (checkpoint commits imported as one `import-checkpoint` commit whose changeset is the tree diff against its parent); [AR §4.6] (the canonical hash is over the net changeset "sorted by (uid, key class, …)"); [50 §5.10] (`TX` agent maximum 50,000 ops; `config.query.caps.*` default 10× → 500,000); [60 §2.5] (the store parameters list has no commit-size or record-size limit).

**Problem.** Nothing bounds the size of one commit. Several paths produce very large ones by construction, and each is built whole in private memory — the commit's ops applied to an overlay copy, the serialized record in one buffer, and the net changeset sorted by uid for the canonical hash — then written as one log record into a 64 MiB extent, then retired into a `hist` frame of 256 commits that must be decompressed whole to read any commit in it.

**Scenario (est.).**
- Restore from the off-store copy, i.e. `image import` of the default checkpoint-granularity image into a fresh store (the disaster-recovery path of CM8 and the release gate's image-restore drill): the first checkpoint's tree diff against the empty tree is every node. Ops ≈ `Create` (uid, kind, title ~60 B, fields ~24 B) ~110 B + 3 `AddEdge` ~45 B + `SetBody` ~20 B ≈ 180 B per node, plus ~340 B per node of compressed body records in the same flushed group. At 1e5: an 18 MB record (+ 34 MB of bodies), an overlay of 1e5 rows at 200–400 B ≈ 20–40 MB, a canonical sort buffer of ~5e5 entries × ~50 B ≈ 25 MB → **≈ 70–100 MB private**. At the owner's three-year scale of 0.3–0.5 M nodes the ops alone are 54–90 MB and **exceed the 64 MiB extent from ≈ 0.37 M nodes: the record cannot be appended at all.** At 1e6: 180 MB of ops, ≈ 0.7–1 GB private.
- The cutover's `links import` of 34,195 existing citations ([AR §11 #23]) as one commit: ~34k `AddEdge` with ~345 B anchor props ≈ 12 MB record + ≈ 5–10 MB overlay.
- `rm --cascade` of a campaign subtree, `moirai migrate` of a strengthened field over every node of a kind, `file mv` of a directory with 1e5 linked files, a merge of a long-lived lane: each is one atomic commit of O(affected nodes).
- A `TX` at the agent maximum (50k ops) or the orchestrator ceiling (500k ops): 10–120 MB private (RAM-M6).
- Any later `log`, `show @c…` or `USE c…` that touches a commit in the same `hist` frame decompresses the whole frame: an 18–180 MB transient for a one-line answer.

**Fix (format items for the M0 freeze; no owner decision needed).**
1. **Bounded inline records.** A store parameter `commit.inline-max-bytes` (default 1 MiB, tiny in the test profile) bounds a `Commit` record's changeset. Invariant: every log record fits one extent (checked by the recovery scan).
2. **Segment-backed changeset ("bulk commit").** A commit whose changeset exceeds the bound writes it as a sealed changeset segment `cs.NNNN` in delta-segment layout (rows sorted by `#N`, list replacements, bitset ± lists; bodies to `blobs` directly), **streamed with bounded memory**, flushed before the `Commit` record that names it (file number, length, BLAKE3); the record carries the header, the absorbed vector, `affected_complete = 0` and the canonical hash. Readers map `cs.NNNN` as one more delta layer (shared pages) instead of replaying it into a private overlay; the next checkpoint folds it; `hist` retirement keeps the reference and the file stays pinned as history (GC and the orphan sweep treat it like a sealed segment; the crash-point enumeration gains the "cs flushed, record not" state).
3. **Streamed canonical hash.** For an image import the tree order already equals the canonical uid order (two-level fan-out on the uid hex), so the importer hashes node by node with O(1) memory if the M0 specification fixes the canonical key order inside a node to match the `.moi` line order; every other producer uses an external merge sort with ≤ 1 MiB runs spilled to `<store>/tmp/`, swept by the orphan sweep.
4. **Byte-bounded `hist` frames.** A frame closes at 256 commits **or 1 MiB raw**, whichever comes first (`hist` frame size is already a store parameter, [60 §2.5]); a commit larger than a frame is its own frame, split into independently decompressible blocks of ≤ 1 MiB.
5. **Batching where atomicity is not required.** `links import` and the cutover import commit in batches under one idempotency key per batch; `file mv` of a directory stays one commit (it is atomic) and uses the bulk path.
6. **Gates.** Import of a checkpoint image ≤ 8 / 16 / 32 MB private at 1e4 / 1e5 / 1e6 (M5); a 500k-op `TX` ≤ 16 MB (M7); decoding one commit from `hist` ≤ 2 MB whatever its neighbours (M1).

### RAM-M1 (major) — A synced lane's overlay carries `main`'s absorbed windows: 1–5 MB typical, up to 8 MiB, rebuilt in every process; old pinned bases duplicate the hot index in the page cache

**Where.** [AR §5a.3]: `view(X) = SEG(pin_X) ⊕ … ⊕ sync commit s_k → ops(main, (M_{k-1}, M_k]) ⊕ resolutions(s_k)`; overlay build "Cost O(X's commits + main's commits in synced windows) … Private memory ≤ ~1 MB per branch read (≈ 150 B per op at 6k ops, est.)"; promotion "when `ops_since_fork > 8,192`, the overlay exceeds 8 MiB, **or** … more than 16 checkpoints behind the head (G16)"; "pins retain the base files of the checkpoint sets the branches forked from (2–4 bases …)". [AR §8.1] "CLI reading a lane +0.2–1 MB; branch overlay ≤ 6k ops × ~150 B". [50 §5.5] "bounded by the overlay (≤ 4,096 ops on `main`, ≤ ~6k on a 14-day lane)". [AR §7.5] `SubagentStart` auto-applies every clean `sync`.

**Problem.** The ≤ 1 MB figure counts only the lane's own ops. G17 made `sync` store only resolutions, but the lane's *view* still replays `main`'s ops of every synced window on top of the fork-time pin, so the overlay grows with `main`'s churn since the fork, not with the lane's work. Because hooks auto-apply clean syncs at every subagent start, the normal lane is a frequently synced lane. The 8 MiB byte threshold and the 16-checkpoint age threshold are the only bounds.

**Scenario (est.).** A lane forked 14 days ago, synced daily, with ~700 own commits (~2k ops): own part ≈ 0.3 MB; `main`'s windows ≈ 14 days × 1.5–4k distinct keys per day with overlap ≈ 8–34k keys × 150 B ≈ **1.2–5 MB**. The age trigger fires only after 16 checkpoints (≈ 8–16 days at ~1–2 checkpoints per day), the byte trigger at 8 MiB. Every CLI call on that lane builds it privately (the ≤ 4 MB gate is exceeded as soon as the lane is more than ≈ 1 week behind its pin); every MCP server holds up to 8 such overlays (RAM-M2). In the page cache, lanes pinned before a rollup keep reading the old base: at 1e6 each distinct base contributes up to its own 99 MB of hot index, so 3 bases in use cost ≈ 300 MB of cache instead of 99 MB.

**Fix (store parameters and engine policy; physical promotion exists in M1, policy in M3; no format change).**
1. Count sync-window keys in the promotion size test and set `promotion.overlay-bytes` to 1 MiB (the budget), not 8 MiB.
2. **Promote on sync**: the maintenance step after a `sync` commit (outside the writer byte, like any promotion, 10–100 ms) writes `seg.b<X>.K` and moves `base_pin` to the newest sealed checkpoint of `main`; at every delta checkpoint also promote each branch that synced or gained > `promotion.ops` ops since its last promotion. The per-process overlay is then the branch's own ops since its last promotion plus at most one checkpoint interval of window — ≤ 1 MiB — and the materialised part is one shared mapped file read by every process.
3. **Re-base after rollup**: after a rollup, promote every live branch pinned to the superseded base, so at most two bases are resident at any time and the old one's pages age out.
4. `doctor --verify` already compares `pin ⊕ ops` with promoted segments, so the policy adds no new correctness surface; the reference model is unaffected (it replays from genesis).
5. **Gates (M3):** branch overlay ≤ 1 MiB for a 14-day lane synced daily at 1e5 and 1e6; CLI on such a lane ≤ main + 1 MiB; distinct base files mapped by live branches ≤ 2 after any rollup. Promotion adds ~1 file per synced lane per day, retired by GC.

### RAM-M2 (major) — MCP servers multiply by session, their gate is per process only, and freed overlays stay in the heap

**Where.** [AR §6.1] "`moirai mcp` (one per Claude session …) keeps maps, the `main` overlay and an LRU of ≤ 8 branch overlays warm (G19)"; [AR §8.1] gate "≤ 10 MB + 1 MB × min(active branches, 8)"; [07 §2] "One server process per Claude Code session"; [M, 08 §5.2] "With 16 sessions [M], per-process caches of 16 MB would cost about 256 MB"; [M, 02 §5.1] fan-out p90 14 agents per run through one session; [17 §5.4] (long-lived allocators retain RSS after a large overlay).

**Problem.** (a) The only MCP gate is per process; nothing bounds the sum over S concurrent sessions, and the design never states S. (b) K = 8 is a count, not a size; with RAM-M1's overlays a fan-out server holds 8 × 1–5 MB. (c) Overlays, BM25 view statistics ([50 §5.5], "cached per process per view"), query arenas and merge candidates are built from many small allocations; on the Windows process heap (Rust's default `HeapAlloc`), small blocks come from heap segments and low-fragmentation-heap sub-segments that are released only when entirely free, while large requests are served by `VirtualAlloc` and decommitted on free [D, Microsoft WPT guidance]. An evicted overlay therefore does not reliably return memory, and the server's private bytes ratchet to their peak — which the gate (`PeakPagefileUsage`) does not distinguish from steady state. (d) On a segment-set change the server must drop the overlay entries the new delta covers; the specification says "the overlay since the segment set's `upto_lsn`" but not that the old overlay's memory is released.

**Scenario (est.).** One orchestrator session fans out 14–16 agents into lanes through one server: 8 overlays × 1–5 MB + base 2.1–4.7 MB ≈ 12–45 MB against an 18 MB gate, retained after the run. With 16 sessions (measured), 15 other servers at 3–10 MB: ≈ 57–195 MB of MCP private memory on a normal fan-out day; if every session touches 8 synced lanes, 0.5–1.1 GB.

**Fix (engine rules, store parameters, gates; no daemon, no leader).**
1. `mcp.overlay-bytes` (default 4 MiB): the LRU evicts by total bytes; K = 8 stays as a count ceiling. With RAM-M1 each overlay is ≤ 1 MiB, so 4–8 lanes stay warm and an evicted lane rebuilds in 1–3 ms.
2. **Region allocation for every per-view and per-request structure** (branch overlays, the `main` overlay, BM25 view statistics, query arenas, merge candidates): each lives in its own chunked arena whose chunks (≥ 256 KiB) come straight from `VirtualAlloc` and are released as a unit on eviction, on a segment-set change (rebuild the `main` overlay in a fresh arena, free the old one) and at request end. Nothing long-lived is allocated from the general heap on these paths. This makes the steady state measurable and returns memory without a timer.
3. **Gates (M10):** after a 16-lane fan-out and one further request, private ≤ 8 MB + `mcp.overlay-bytes` at 1e5 and 1e6 (steady state, read at the end of a scripted session, not only the peak); peak ≤ 16 MB including a 4 MiB query; **aggregate**: 16 servers idle Σ ≤ 128 MB at 1e5 in the M11 soak. Also report the heap-only high-water mark of [17 §5.5] (`heap_peak`) so allocator retention is visible.
4. State S in [AR §8.1] (4 typical, 16 measured) and add the aggregate row.

### RAM-M3 (major) — Rollup runs inside the long-lived MCP server; its peak is larger than the server's whole gate at every scale

**Where.** [AR §4.9] "Rollup (new base, rebuilt bitsets, `TOPO` by one Kahn pass, dictionary retrain, tier-2 terms): only on explicit `moirai gc` or **in the resident MCP server after serving a request when deltas exceed 25 % of base**"; [AR §6.1] same; [AR §4.1] `dict.D` 32–110 KiB, "retrained at rollup when bodies grew > 25 %"; [60 §5.4] rollup budget is time only (≤ 0.3 / 3 s).

**Problem.** A rollup's working set is dominated by terms that do not shrink with N: dictionary training needs its samples in memory (a common rule is ~100× the dictionary size, ≈ 11 MB for 110 KiB) plus the trainer's frequency tables (≈ 4 + 2 MB at the default 2^20 table, est.); the TOPO pass needs N-sized arrays (in-degree, queue, output: ≈ 12 MB at 1e6); the tier-2 term merge and bitset rebuild add a few MB. It runs in the one process kind that lives for the whole session and whose heap keeps its peak (RAM-M2), and it writes a new 0.55 GB base at 1e6 through the file cache at normal memory priority (RAM-m5).

**Scenario (est.).** At 1e5 a rollup with retraining peaks at ≈ 17–25 MB inside a server gated at 18 MB; at 1e6 ≈ 20–35 MB plus 1–3 s of a request's latency tail and ~1.1 GB of cache traffic (read old, write new) on a machine with 1.8 GB free.

**Fix (engine rule; no daemon: the child ends with the work).**
1. The MCP server never runs a rollup. After serving a request, when deltas exceed the threshold, quiet mode is off and maintenance byte 2 is free, it **spawns** `moirai gc --rollup --if-needed` detached, at `BELOW_NORMAL_PRIORITY_CLASS`, with thread memory priority `MEMORY_PRIORITY_LOW` and background I/O mode, and returns; the child takes byte 2 (a second spawn exits immediately), streams the rollup and exits. A CLI-only session gets the same trigger from write-path step 10 (spawn, never run inline), so the F-A3/F-B4 latency rule still holds.
2. **Streaming rollup** with a bounded working set: CSR and columns merged row by row from base + deltas; bitsets rebuilt per 65,536-id chunk; `TOPO` renumbered from the PK-maintained positions by one sort of (position, `#N`) pairs (8 MB at 1e6) instead of a Kahn pass, with Kahn kept as the `doctor` check; dictionary retraining on a capped sample (`dict.train-sample-bytes`, default 4 MiB) — the M0 ratio measurement (U12) decides whether the cap costs ratio.
3. **Gate (M1):** rollup peak private ≤ 24 / 24 / 32 MB at 1e4 / 1e5 / 1e6, measured in the child; MCP server private unchanged across a triggered rollup (M10).

### RAM-M4 (major) — Reading a project file is a whole-buffer operation sized by `files.max-read-bytes` (16 MiB), which contradicts both the 4 MB gate and the 1 MiB query `mem`

**Where.** [40 §2.11 R-13] `files.max-read-bytes (default 16 MiB)`; [40 §7.3] "private RSS, read verb: +≤ 0.3 MB + **largest anchored file read** + decoded git trees (≤ 256 KiB) … larger files resolve by stat and a streamed `oid` only"; [40 §2.5] `is_text(b)` and `norm(b)` are defined over the whole buffer and `oid(b) := H("blob " ‖ decimal(len norm(b)) ‖ 0x00 ‖ norm(b))`; [40 §4.3] E4/E7 hash candidates, E8 and `--deep` sketch candidates "streamed"; [40 §4.8] `--deep` may use 8 scoped threads; [50 §5.10] `mem` includes "[40]'s resolution scratch"; [60 §5.4] R4 gate ≤ 4 MB read verbs and hooks, ≤ 16 MB `--all`/`--deep`.

**Problem.** The documents disagree: [40] adds the file size to a read verb's RSS, while [50] charges resolution scratch to a `mem` of ≤ 1 MiB. Taken literally, either the RSS gate breaks on any linked file over ≈ 0.7 MB, or every query touching such a file stops with E502. `oid` needs the normalised length in its header before the first hashed byte, so "a streamed `oid`" is a two-pass computation that [40] does not specify. With 8 scoped threads, `--deep` holds up to 8 candidate files and their normalised copies at once.

**Scenario (est.).** `moirai show 51` where #51 anchors a symbol in the owner's largest file (1.87 MB [M, 10 §5.1]): raw + normalised copy + a line-hash array (~40k lines × 8 B) ≈ +4 MB on a 2–3 MB baseline → 6–7 MB. A generated file at the 16 MiB cap: +32 MiB. `links check --deep` over a tree with 16 MiB files: 8 × 32 MiB ≈ 256 MB against a 16 MB gate.

**Fix (engine rule and a `config` default; the resolver constants of R-14 are unchanged).**
1. Every project-file read goes through one fixed buffer (128 KiB) per thread. `oid` is two passes: pass 1 computes `is_text` statistics, the normalised length, the line hashes needed by the window and the sketch; pass 2 streams the normalised bytes into the hasher. The owner's p99 file (333 KB) costs 2–3 buffer fills.
2. Anchor resolution streams: exact quote and prefix/suffix search by a chunked scan with an overlap of the longest selector; window alignment over a line-hash array capped at `files.max-line-hashes` (default 64k lines = 512 KiB; beyond it the span resolves by quote only and reports `unverified (size)` for window-only anchors); Myers bit-parallel matching streams by construction.
3. Redefine `files.max-read-bytes` as the largest file whose content is examined at all (default 16 MiB unchanged), not a buffer size; charge only the fixed buffers and the line-hash array to `mem`.
4. `--deep`: at most 2 threads read content at once (the other threads only enumerate); sketches of candidates are computed in the same streaming pass.
5. **Gates (M6):** a read verb resolving anchors in a 16 MiB file ≤ 4 MB private; `--deep` with 8 threads over a tree containing 16 MiB files ≤ 16 MB; an LQ query calling `link_state()` on such a file stays within the default `mem`.

### RAM-M5 (major) — The quiet-mode tail and lazy runtime records inflate every process's replay: 2.6–6.5 MB in each CLI, hook and MCP server during benchmark windows

**Where.** [AR §2.2] "Quiet mode: no automatic checkpoints; hard cap at 8× the tail threshold"; [AR §4.5 step 3] every process replays `(replayed_lsn, committed_lsn]` into its overlay; step 10 thresholds "4,096 ops / 4 MiB of records (bodies on a separate 32 MiB threshold)"; [AR §4.3] lazy record kinds `FileObs`, `Pending`, `FPrint`, `UsnCursor`, `TreeReg`, `PrefixEv`, `GitFacts`, heartbeats; [40 §4.2] a `SessionStart` settle covers ~2–8k links, `links sync --all` up to all linked files.

**Problem.** The tail is bounded in ops and record bytes but decoded eagerly into a private overlay by every process. (a) In quiet mode — exactly the owner's benchmark windows, where RAM and interference matter most — the bound is 8×. (b) Lazy runtime rows count toward the 4 MiB record threshold but decode into per-row maps in every process until the next checkpoint, although only settles and the resolver ever read them.

**Scenario (est.).** A 4-hour quiet window with agents still recording measurements: the tail reaches 32,768 ops → each CLI/hook process holds 2.6–6.5 MB of overlay (3.8–9.3 MB total, against the 4 MB gate), 16 of them at a burst 61–149 MB; open time grows with it (≈ 8× the 3–5 ms full-tail figure of U13). Outside quiet mode, one `links sync --all` over 1e4 linked files appends ≈ 1.5 MB of `FileObs`; a SessionStart settle of 8k links ≈ 1.2 MB; every process opening before the next checkpoint decodes them (≈ +1–2 MB each; up to ≈ 4–6 MB at a full 4 MiB of runtime records).

**Fix (engine rule and store parameters; the log format is unchanged).**
1. **Compact overlay**: the replayed tail is kept as a sorted index `#N → newest record lsn` (12–16 B per op) plus the derived-state ± lists (4 B per entry); field values, titles and bodies are decoded on probe from the log pages already in the page cache (a µs `pread` or a copy from a small per-process record cache). Target ≤ 60 B per op → a full 8× quiet tail ≤ 2 MB.
2. **Cap the tail in overlay bytes**: `tail.max-overlay-bytes` (default 1 MiB normal, 2 MiB quiet) joins the op and record thresholds; the quiet cap is expressed in these bytes, so the one bounded delta checkpoint of G10 happens at the same memory level whatever the op mix.
3. **Runtime records are indexed, not decoded**: replay records only their key (tree, `#F`) → lsn; the resolver decodes on use. A separate `tail.runtime-bytes` threshold (default 2 MiB) triggers a runtime-only fold into `FILEOBS`/`FPRINT`/`GITRENAMES` that does not touch graph sections (cheap, outside the writer byte).
4. **Gates (M1, M6):** CLI private ≤ 4 MB with the tail at the quiet cap; ≤ 4 MB after a settle that wrote 1e4 runtime rows; open ≤ 3 ms at the quiet cap.

### RAM-M6 (major) — The write path has no memory budget: `TX`, `apply`, merge, sync, revert, and hooks that auto-apply syncs

**Where.** [50 §5.10] `mem` lists "the bump arena, operator and aggregation state, sort and `collect()` buffers, every visited set, the as-of reverse overlay and replayed ops, [40]'s resolution scratch" — not a `TX`'s candidate overlay or its serialized commit; `TX` caps 10,000 ops default / 50,000 agent maximum / 10× for the orchestrator; [50 §5.12] Q18–Q25 writes "< 64 KiB"; [AR §4.5 step 6] "a scratch copy of the branch overlay"; [AR §5a.7 steps 2–7] two folds, base per key, candidate overlay, validators (full Kahn above 1,000 precedence edges); [AR §4.6] a `sync`'s canonical op list is "the full state diff against the lane parent … `main`'s window materialised"; [AR §7.5] `SubagentStart` "runs `sync --check` for the bound lane and auto-applies only a preview with zero conflicts"; [60 §5.4] merge and sync gates are time only; [AR §2.6] bodies are zstd-dictionary-compressed and written into the log tail at write time.

**Problem and scenario (est.).**
- `TX` at the default cap: 10,000 ops × (~150 B candidate overlay + ~40 B serialized + ~50 B canonical sort) ≈ +2.4 MB → a CLI at 2–3 MB baseline ends at 4.4–5.4 MB; at the agent maximum +12 MB; at the orchestrator ceiling +120 MB (and see RAM-B1).
- A literal scratch copy of the overlay doubles the tail's 0.33–0.82 MB on every write.
- Any write with a body builds a zstd compression context and a digested dictionary: ≈ 1–2 MB (est.), so `remember`/`rule --stdin` from the CLI sits at 3–5.4 MB.
- `sync` of a lane that is d days behind: fold of `main`'s window (1.5–4k keys per day) + the base per touched key + the sorted full-state diff for the canonical hash ≈ 0.3–0.6 MB per day; a lane idle for 14 days, woken by a subagent: +4–8 MB inside a hook gated at 4 MB, and a 16-agent fan-out into idle lanes runs 16 of them at once.
- Merge into `main` at 1e6 with > 1,000 touched precedence edges: full Kahn over the combined graph ≈ 12 MB of N-sized arrays plus the implied exogenous edges (RAM-m4) → 15–25 MB, unbudgeted.

**Fix (engine rules, store parameters, gates).**
1. A **write budget** `wmem` (same rule as `mem`: min(1 MiB, gate headroom), ≥ 256 KiB; agent maximum 4 MiB) charged with the candidate layer, the serialized record and the canonical sort; exceeding it switches to the bulk path of RAM-B1 (explicit verbs) or refuses with E501 naming the batch split (`TX`, `apply`). The 10,000-op default cap stays as a count limit.
2. The scratch "copy" is a copy-on-write layer over the overlay (only touched keys).
3. Compress bodies when they are sealed into `blobs.NNNN` by the maintenance holder (the log tail holds them raw, still under the 32 MiB body threshold), or, if M0 shows the tail must hold compressed bodies, use level-3 parameters fixed for ≤ 64 KiB inputs (window 2^17) and a by-reference dictionary; either way ≤ 0.5 MB per writing process.
4. Hooks auto-apply a sync only when the preview touches ≤ `hooks.sync-auto-keys` keys (default 2,000); otherwise they print the `moirai sync` line (the D5 rule already allows declining).
5. **Gates (M3, M7, M9):** merge of a 2k-op lane ≤ 8 MB at 1e5, ≤ 16 MB at 1e6 including full Kahn; sync of a 14-day-behind lane ≤ 8 MB; a default-cap `TX` ≤ 4 MB CLI; any hook ≤ 4 MB.

### RAM-m1 (minor) — The in-process git object layer has no cache or memory bound

**Where.** [60 §3.5] M4 scope (pack reading "incl. OFS/REF delta resolution", tree diff, ancestry walks, per-commit renames) with time budgets only; [40 §4.3] E6 over windows of up to `files.budget.window` = 2,000 commits; [40 §7.3] "decoded git trees (≤ 256 KiB)".

**Problem.** Resolving a deltified tree or commit requires its base chain (git's default pack depth is 50); git itself keeps a delta-base cache of "96 MiB on all platforms" per thread [D, git-config]. An implementation that copies git's defaults puts a 96 MiB cache in every hook and CLI process that touches the tree gate; one that caches nothing re-inflates chains O(depth²) inside the 150 ms `SessionStart` budget. Neither is specified.

**Fix.** M4 specifies a byte-bounded delta-base LRU (`git.delta-cache-bytes`: 256 KiB in CLI/hooks, 1 MiB in the MCP server), objects larger than the cache streamed, the E6 window processed commit by commit with per-commit memory released, and pack/idx/commit-graph files mapped read-only (shared). Gate (M4): tree lookup, ancestry and a 2,000-commit rename window each ≤ 1 MiB private over the owner's repositories after `git gc --aggressive`.

### RAM-m2 (minor) — The MCP front end is not thread-free, and its RSS is still a claim

**Where.** [AR §2.2] "nothing has threads or timers"; [AR §6.1] MCP "0 % CPU when idle (no threads, no timers)"; [AR §2.10] `rmcp` + `tokio` (`current_thread`); [AR §8.1] "rmcp RSS is **claimed** (U11)"; [60 §5.1] idle gate "zero CPU time and zero context switches over 10 minutes".

**Problem.** tokio's `Stdin` "is implemented by using an ordinary blocking read on a separate thread, and it is impossible to cancel that read" [D, tokio]; `stdout` writes also go through the blocking pool, whose defaults are up to 512 threads with 2 MiB stacks and a 10 s keep-alive [D, tokio `Builder`]. On Windows an anonymous stdin pipe cannot be read asynchronously without such a thread. So the server has at least one permanent extra thread, spawns pool threads for writes, and each pool thread wakes once when its keep-alive expires — a context switch up to 10 s after the last request. The memory is small (committed stack pages + TEB, tens of KB each) but the design's claim is false and the RSS of rmcp over stdio (7–11 MB **claimed** from an HTTP benchmark [07 §2.7]) is still unmeasured.

**Fix.** Build the runtime with `max_blocking_threads(1)`, `thread_stack_size(256 KiB)` and a keep-alive longer than the idle window, or give rmcp a transport whose reader is one dedicated thread blocked in `ReadFile` and whose writer writes synchronously on the runtime thread; restate §2.2/§6.1 as "at most two OS threads, both blocked when idle". Add to M0 a stdio probe of `moirai mcp` (rmcp + the chosen runtime shape) measuring private bytes and thread count; if it exceeds baseline + 2 MB, a hand-written synchronous newline-delimited JSON-RPC loop (a few hundred lines, fewer dependencies — no owner decision needed to use fewer crates) is the alternative. Idle gate: measured from 15 s after the last request, zero context switches thereafter.

### RAM-m3 (minor) — Image export and import budgets: one pack per run makes full export O(objects); import has no budget; `gitmap` lookup structure unspecified

**Where.** [AR §5b.9] and [AR §8.1] "full export private RSS ~2 / 12–15 / 110–150 MB … pack index entries are the O(objects) structure (G24)"; [AR §5b.6] "Export writes one pack per run"; [60 §5.4] "full import of 1e5 ≤ 7 s" with no RSS; [AR §4.1] `gitmap.NNNN` "sealed pages of `(commit_id16, dest, algo, git_oid[32])`" with no stated order.

**Problem.** G24 made parent trees lazy but kept one pack per run, so idx staging grows with objects (≈ 40–48 B each, ~50 MB at 1e6) and the 1e6 full export needs 110–150 MB. Import memory is not stated for either granularity (the checkpoint case is RAM-B1). The export frontier tests every commit against `gitmap`; unsorted pages invite a per-process hash set of all entries (41 B per commit per destination: 4 MB per 1e5 commits).

**Fix.** Close a pack every 65,536 objects (each with its own idx; git handles many packs and `image gc` repacks them) → full export ≤ 2 / 6 / 10 MB; `gitmap` pages sorted by commit id with a fan-out table, probed in place; gate import per commit ≤ 4 MB and full import at commit granularity ≤ 16 MB at 1e5 (M5).

### RAM-m4 (minor) — Full-recompute production paths have no memory budget and invite the model's materialise-everything strategy

**Where.** [AR §4.10] "`doctor --verify` recomputes every derived structure and every branch head … `repair --rebuild-from-log` rebuilds every segment, blob and `hist` file from the log"; [60 §3.3] M2 exit "derived state == recomputation at 1e5 and 1e6"; [AR §5a.7 step 6] full Kahn above 1,000 touched precedence edges; [AR §3.4 I5′] implied edges `{X→D : blocks(X,P) exogenous, D ∈ subtree(P)}`; [60 §5.4] `backup` time only.

**Problem and scenario (est.).** The natural implementation of a recomputation is the reference model's: materialise everything. `doctor --verify` at 1e6: counters 8 MB + ~80 bitsets × 122 KiB ≈ 10 MB + an inverse CSR for the I-P3 check ≈ 24 MB → 40–60 MB, run after every kill-loop iteration in CI and by the owner. `repair --rebuild-from-log` replaying history into memory: ≈ 0.5–1 GB at 1e6. Full Kahn with materialised implied edges: one exogenous blocker on a campaign root with 1e5 descendants adds 1e5 edges (0.8 MB); ten such blockers 8 MB.

**Fix.** `doctor --verify` works per 65,536-id chunk (counters and bitsets of a chunk from the reverse CSR; the inverse check by scanning the forward CSR once per chunk of destination ids): ≤ 16 / 16 / 32 MB. `repair` drives the ordinary checkpoint pipeline from lsn 0 (overlay bounded by the tail threshold, a delta per threshold, tiered folds) and ends with the streaming rollup of RAM-M3: ≤ 32 MB. Kahn derives implied edges on the fly from the parent CSR and never materialises them. Gates in [60 §5.4] at M1 (repair), M2 (`doctor --verify`) and M3 (Kahn).

### RAM-m5 (minor) — Bulk paths pollute the shared page cache at normal memory priority on a machine with 1.8 GB free

**Where.** [AR §7.1] `backup` ("copied 61.3 MB in 0.19 s"), [AR §4.9] rollup ("sequential rewrite"), [AR §5b.6] full export, `hist` retirement, repair, `links check --all`/`--deep`.

**Problem.** Each reads or writes the whole store (≈ 2× 55 MB at 1e5, 2× 550 MB at 1e6, plus history and, for export, the pack) through the cache manager at the default memory priority. Microsoft's guidance is that transient allocations "can … push valuable content out of the system Standby cache when there's memory pressure", and that applications "should use `SetThreadInformation` with `ThreadMemoryPriority` to lower the memory priority of threads that perform background operations or access files and data that are not expected to be accessed again soon" [D]. With 16 agents at 3.5 GB private and 1.8 GB free [M, 05 §2.4], a 1 GB pass evicts the agents' cached files and, under pressure, trims their working sets.

**Fix.** Every bulk path sets `MEMORY_PRIORITY_LOW` (or thread background mode) for its I/O thread and opens its inputs with `FILE_FLAG_SEQUENTIAL_SCAN`; `backup` copies large files unbuffered (`COPY_FILE_NO_BUFFERING`). Measure at M1/M11: system available memory and standby-list size of the idle 16-agent fixture before and after a 1e6 rollup, backup and full export (reported, gated on "agents' working sets unchanged within the noise band").

### RAM-m6 (minor, one format item) — Structures that grow with history: markers, symbols, per-run actors

**Where.** [AR §5d.1] "~30k markers/year at 40 B"; [50 §5.12] `brief_triage` "O(markers)"; [AR §4.9] GC has no marker rule; [AR §2.12] "symbols never GC'd"; [AR §4.3] commit header `actor u16, role u8, session u32 // symbols`; [AR §3.1] `CREATOR: (actor u16, role u16)`; [AR §7.6] actors such as `wf:r7/dev#1`; [AR §6.1] lists the "symbol table" among per-process caches.

**Problem.** Absorbed markers stay in `MARKERS` forever, so every `brief` scans a table that grows ~1.2 MB a year (shared pages, and work units). Actor names embed the run id; at ≈ 11 runs a day × ~4 agents ≈ 16k new actor symbols a year, the u16 actor space overflows in about four years — a format failure found late. If the "symbol table" per-process cache is a materialised name → id map, it is O(history) private in every process.

**Fix.** Prune at checkpoint fold every marker that every live ref has absorbed and whose commit is older than `gc.reflog-expire` (so `undo` inside the reflog window can still re-activate it); symbol lookups by binary search over the mapped `SYMTAB` plus the tail's new symbols, never a per-process map; **format (M0):** widen `actor` to u32 in the commit header and `CREATOR`, or intern the role label as the actor and carry the run id as its own field.

### RAM-m7 (minor) — `pack`/`brief` bodies accumulate in one bump arena

**Where.** [AR §4.7] "bodies decompress into a per-request bump arena"; [AR §8.1] pack = "~40 body decompressions"; [AR §8.1] "arena ≤ 256 KiB"; [50 §5.10] bodies count against `mem`.

**Problem.** A bump arena frees nothing until the request ends: 40 bodies × up to 64 KiB (24 KB plan sections are common [01 §7 L12]) ≈ 1–2.5 MiB, more than the 256 KiB arena and the 1 MiB `mem`, although a 40,000-char pack emits only ~40 KB.

**Fix.** Decompress each candidate body into one reused 64 KiB buffer, render or degrade it to L1/L0 at once, and keep only the rendered text (≤ the output budget) — ≤ 128 KiB for bodies. Gate: pack with 40 L2 candidates ≤ CLI gate (M9).

### RAM-m8 (minor) — Stack depth and thread stacks are unspecified

**Where.** [50 §5.2] recursive-descent + Pratt parser with panic-mode recovery; [AR §3.4] Pearce–Kelly DFS; [50 §5.7] "DFS where only reachability is needed"; [40 §4.8] scoped worker threads.

**Problem.** The main thread has a 1 MiB stack reserve (MSVC default) and Rust worker threads 2 MiB reserved; a recursive parser on deeply nested input or a recursive DFS over a long `BLOCKS` chain at 1e6 overflows it — a process abort, not an exit 10 — and fuzzers will find it. Committed stack pages count in `PeakPagefileUsage`.

**Fix.** Nesting limits in the grammar (expression and pattern depth ≤ 64, E-code), iterative PK, closures and tree walks with explicit heap stacks charged to `mem`/`wmem`, worker threads created with a 256 KiB stack; fuzz targets include depth.

### RAM-m9 (minor) — The RAM gates cover two process kinds at one scale; page tables and steady state are invisible

**Where.** [AR §8.1] CI gates "private RSS ≤ 4 MB CLI / ≤ 10 MB + 1 MB × min(active branches, 8) MCP at 1e5"; [60 §5.1] "Peak private bytes … `PeakPagefileUsage` read at process exit; peak working set reported, not gated"; [60 §5.4] explicit-command rows give time only (rollup, merge, sync, backup, import); [17 §5.5] (`heap_peak`, floor-relative gate) not integrated.

**Problem.** No gate exists at 1e6 for the CLI (stated 3–6 MB), none for merge, sync, rollup, `doctor`, repair or import, none for the system aggregate, none for the MCP steady state (peak only), and page tables (private, per process, ≈ 8 B per touched 4 KiB page + a table page per touched 2 MiB region — ≈ 0.2–1 MB for a process that sweeps the 1e6 hot index) are outside `PeakPagefileUsage`.

**Fix.** Adopt §6's table as [60 §5.4]'s RAM rows; report `heap_peak` beside `PeakPagefileUsage` (floor-relative, [17 §5.5]); record a VMMap summary (private, page table, shareable) for each process kind at M0 and M11.

---

## 6. Proposed budgets table (RAM)

Private = peak private bytes (`PeakPagefileUsage`) unless the row says steady state; "floor" = the empty-Rust-binary baseline measured in the same run ([60 §5.3]). Rows marked *new* have no gate today.

| Metric | Budget | Scale | Gate (milestone, mechanism) |
|---|---|---|---|
| CLI/hook read on `main`, private | ≤ 3.5 MB (≤ floor + 2.8 MB) | 1e4, 1e5; ≤ 4 MB at 1e6 | GT11 from M1, through the binary from M8; 1e6 row *new* |
| CLI on a 14-day lane synced daily, private | ≤ `main` row + 1 MiB | 1e4–1e6 | M3 (*new* workload: daily syncs) |
| Branch overlay, any process | ≤ 1 MiB incl. sync windows | 1e4–1e6 | M3, counted by the overlay allocator (*new*) |
| Distinct base segments mapped by live branches | ≤ 2 after any rollup | 1e5, 1e6 | M3 (*new*) |
| CLI with the tail at the quiet cap, private | ≤ 4 MB; open ≤ 3 ms | 1e5, 1e6 | M1 (*new*) |
| CLI after a settle wrote 1e4 runtime rows, private | ≤ 4 MB | 1e5 | M6 (*new*) |
| CLI write with a body / default-cap `TX`, private | ≤ 4 MB | 1e5 | M2 / M7 (*new*) |
| `TX`/`apply` write working set `wmem` | min(1 MiB, headroom), ≥ 256 KiB; agent max 4 MiB | any | M7 (*new*) |
| Commit record | ≤ 1 MiB of changeset inline; larger → segment-backed | any | M0 format; M1 recovery check (*new*) |
| `hist` frame | ≤ 256 commits and ≤ 1 MiB raw; decoding one commit ≤ 2 MB | any | M0 format; M1 (*new*) |
| Hook (any), incl. `sync` auto-apply | ≤ 4 MB; auto-apply only ≤ 2,000 keys | 1e5 | M9 (*new*) |
| `SessionStart` (brief + settle) | ≤ 4 MB total; R4 part ≤ 1 MB | 1e5 | M9 (R4 part exists) |
| Read verb resolving anchors in a 16 MiB file | ≤ 4 MB | any | M6 (*new* fixture) |
| `links check --all` / `--deep`, 8 threads, 16 MiB files present | ≤ 8 MB / ≤ 16 MB | 1e5 linked files | M6 (fixture *new*) |
| LQ query `mem` incl. R4 buffers | ≤ min(1 MiB, headroom); 2/4 MiB max | any | M7 (exists; R4 part *new*) |
| `pack` with 40 L2 candidates | ≤ CLI row; bodies ≤ 128 KiB | 1e5 | M9 (*new*) |
| MCP server steady state after a 16-lane fan-out | ≤ 8 MB + `mcp.overlay-bytes` (4 MiB) | 1e5, 1e6 | M10 (*new*: steady state, not only peak) |
| MCP server peak, any tool call | ≤ 16 MB incl. a 4 MiB query | 1e5, 1e6 | M10 (replaces 10 MB + 1 MB × 8) |
| MCP threads / idle | ≤ 2 OS threads; 0 CPU and 0 context switches from 15 s after the last request | any | M0 probe, M10 |
| Aggregate, 16 MCP servers idle | Σ ≤ 128 MB | 1e5 | M11 soak (*new*) |
| Aggregate, 16 concurrent CLI/hook processes | Σ ≤ 64 MB | 1e5 | M11 soak (*new*) |
| Merge (2k-op lane) / sync (14 days behind) | ≤ 8 MB; ≤ 16 MB at 1e6 incl. full Kahn | 1e5, 1e6 | M3 (*new*; time gates exist) |
| Delta checkpoint / promotion | ≤ 8 MB | 1e4–1e6 | M1 (*new*) |
| Rollup (transient child, low priority) | ≤ 24 MB (1e4, 1e5), ≤ 32 MB (1e6); MCP private unchanged across it | 1e4–1e6 | M1, M10 (*new*) |
| `doctor --verify` | ≤ 16 MB (1e5), ≤ 32 MB (1e6) | 1e5, 1e6 | M2 (*new*) |
| `repair --rebuild-from-log` | ≤ 32 MB | 1e6 | M1 (*new*) |
| Git object layer per process | delta cache ≤ 256 KiB CLI/hooks, ≤ 1 MiB MCP; tree lookup, ancestry, 2,000-commit rename window ≤ 1 MiB | owner repositories | M4 (*new*) |
| Full export | ≤ 2 / 6 / 10 MB | 1e4 / 1e5 / 1e6 | M5 (tightens ≤ 15 MB at 1e5) |
| Import: per commit / full at commit granularity / checkpoint image | ≤ 4 MB / ≤ 16 MB / ≤ 8, 16, 32 MB | 1e4 / 1e5 / 1e6 | M5 (*new*) |
| Bulk paths' effect on other processes | agents' working sets and standby unchanged within the noise band during rollup, backup, full export at 1e6 | 1e6 | M11, reported (*new*) |
| Idle RAM, CLI/hooks | 0 (no process) | any | exists |

---

## 7. Checked and found sound, or already resolved (not re-raised)

- **Mapped, immutable segments shared by all processes** ([AR T1, §4.7]); frozen bitsets queried in place (F-C4 resolved); strings borrowed from the map; no per-process buffer pool. Sound: the hot index (1.0 / 9.9 / 99 MB) is paid once for all processes.
- **Overlay build streams through a fixed buffer** (F-D1/G15, [20]); never reads an extent whole.
- **Query `mem` composition** ([51] M8 → [50 §5.10]): one budget from the measured headroom, visited sets charged, as-of replay bounded (E303 beyond ≈ 6.9k ops at 1 MiB — a usability limit by design, not a RAM defect).
- **`across()` in the CLI** drops each overlay before the next ([50 §5.8]); `--across` without refs uses promoted `TOUCH` bitmaps only (G19).
- **MCP per-lane growth** was identified (F-D4) and bounded by count (G19); RAM-M2 adds the byte bound, the aggregate and heap retention, which G19 did not cover.
- **Export's parent trees are lazy** (G24); RAM-m3 only tightens the pack-per-run choice.
- **R4 idle state**: no resident R4 structure beyond the MCP server's ≤ 256 KiB resolution LRU ([40 §4.8]); fingerprints and file observations live in mapped sections.
- **Leases, idempotency and client heads** are bounded by retention (30 days, session expiry); `REFS` with absorbed vectors ≈ 18 KB at 50 refs.
- **The reference model** runs at ≤ 2e3 nodes ([60 §4.4] item 8) and is never linked into the binary; its RAM is a test-host matter only.
- **FTS at 1e6** uses tier 2 postings; tier 1 is limited to small stores and the unfolded tail, so a 260 MB title scan does not occur on the default path.

---

## 8. Consequential edits (for the next revision of the documents)

| Document | Section | Edit |
|---|---|---|
| [AR] | §4.1, §4.3, §4.5 steps 6–7, §4.6, §4.9, §4.10 | bulk (segment-backed) changeset, `commit.inline-max-bytes`, byte-bounded `hist` frames, streamed canonical hash, copy-on-write scratch layer, compact overlay, runtime-record indexing, `tail.max-overlay-bytes`, `tail.runtime-bytes`, rollup never in the MCP server, streaming rollup, chunked `doctor --verify`, repair via the checkpoint pipeline (RAM-B1, M3, M5, M6, m4) |
| [AR] | §5a.3, §5a.10 | promotion on sync, at checkpoints for synced branches and after rollup; overlay budget 1 MiB incl. windows; ≤ 2 resident bases (RAM-M1) |
| [AR] | §2.2, §6.1 | MCP: byte-bounded LRU, region arenas, "at most two OS threads", no rollup; the spawned maintenance child (RAM-M2, M3, m2) |
| [AR] | §2.6, §7.4, §7.5 | bodies compressed at seal time; per-candidate body buffer; hook auto-apply threshold (RAM-M6, m7) |
| [AR] | §3.1, §4.3, §4.9, §5d.1 | actor width (format); marker pruning; `SYMTAB` probed in place (RAM-m6) |
| [AR] | §8.1, §8.2 | replace the RAM rows with §3 and §6; add S sessions and the aggregate; VMMap breakdown in M0 item 11 and M11 (RAM-m9) |
| [40] | §2.5, §4.3, §4.8, §7.3, R-13 | two-pass streamed `oid`, fixed buffers, `files.max-line-hashes`, `files.max-read-bytes` redefined, `--deep` content readers ≤ 2 (RAM-M4) |
| [50] | §5.8, §5.9, §5.10, §5.12 | `wmem` for `TX`; R4 scratch = fixed buffers; Q18–Q25 row restated (RAM-M6, M4) |
| [60] | §2.5 | format rows: bulk changeset record and `cs` segment kind, inline bound, `hist` frame byte bound, actor width; store parameters `commit.inline-max-bytes`, `tail.max-overlay-bytes`, `tail.runtime-bytes`, `promotion.overlay-bytes`, `mcp.overlay-bytes`, `dict.train-sample-bytes`, `git.delta-cache-bytes` (RAM-B1, M1, M2, M3, M5, m1, m6) |
| [60] | §3.5, §5.2, §5.4 | M4 memory bounds; M0 stdio probe of the MCP runtime shape; §6's rows as RAM gates by milestone (RAM-m1, m2, m9) |

---

## 9. Web sources checked (2026-09-26)

- tokio `Stdin`: "stdin is implemented by using an ordinary blocking read on a separate thread, and it is impossible to cancel that read" — https://docs.rs/tokio/latest/tokio/io/struct.Stdin.html
- tokio runtime `Builder` defaults: 2 MiB thread stacks, 512 blocking threads, 10 s keep-alive — https://docs.rs/tokio/latest/tokio/runtime/struct.Builder.html
- Windows `SetThreadInformation` / `ThreadMemoryPriority` guidance for background and file-scanning threads — https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-setthreadinformation
- Windows heap types (NT heap, LFH sub-segments, `VirtualAlloc` for large requests) and the standby-cache effect of transient allocations — https://learn.microsoft.com/en-us/windows-hardware/test/wpt/memory-footprint-optimization-exercise-2
- `core.deltaBaseCacheLimit`: "Default is 96 MiB on all platforms" — Git for Windows 2.54 `git-config` documentation (local copy of https://git-scm.com/docs/git-config)

*End of the RAM audit.*
