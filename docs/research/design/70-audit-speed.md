# 70 — Audit of the integrated design: SPEED

*Audit of [AR] (`docs/ARCHITECTURE-RESEARCH.md`, read in full), [40] rev 2, [50] rev 2 and [60] issue 2 on the owner's first priority axis, speed. Date: 2026-09-26. Design documents only; no code was written. One small read-only probe was run on the BoykoEngine repository (`git count-objects`, `git ls-files`, a stat/enumeration timing in Python, `git status` with `GIT_OPTIONAL_LOCKS=0`); nothing in the repository was written. The Claude Code hooks reference was fetched on 2026-09-26 to verify the `mcp_tool` question.*

Tags: **[M]** measured on the owner's machine by the cited report; **[M-70]** measured for this audit; **[D]** vendor documentation fetched 2026-09-26; **est.** arithmetic with the inputs shown. Section references without a document tag are to [AR].

---

## 0. Verdict and summary

**Verdict.** The engine core is fast by construction and most of its budget arithmetic holds: O(1) open, one data-only flush per durable commit, lock-free readers, bitset-anchored `ready`/`blocking`, O(1) marker absorption, µs-scale LQ parsing and anchored patterns, spawn-free ancestry. But the design is **not yet consistent with its own speed gates on the daily path**. One blocker and eight major findings would each make a gate of [60 §5.4] fail at the milestone that measures it, or put tens to hundreds of milliseconds on every lane command, hook or pack. All of them can be fixed before the M0 freeze without changing any owner decision; four of them touch frozen format items and must be settled before M0 exits.

| # | Sev. | Finding (short) | Daily-path cost as specified (est. from [M] inputs) | Fix in one line |
|---|---|---|---|---|
| S1 | **blocker** | A lane's view replays **every `main` op in its synced windows**; the 14-day-lane estimate counts only the lane's own commits | 15–120 ms and +1.5–5 MB private per lane CLI command, hook or first MCP read on days ~2–16 of a lane; M1/M3 lane gates (≤ 10 ms) unreachable on a lane that syncs | count synced-window ops in the promotion trigger, derive the threshold from the 10 ms gate, promote after the sync; fixture with syncs in M0 item 3 |
| S2 | major | The write path does its heavy work **inside the writer byte** (tail replay, first overlay build, branch/provenance reads, merge/sync compute, `TX` `MATCH`, `suspect` closure, `complete`'s settle **with its 50 ms quiescence**) | writer-byte hold 8–130 ms per fresh lane writer, 20–160 ms per merge, ≥ 72 ms per `complete` that settles; 16-writer p99 gate (≤ 50 ms) and hold p99 gate (≤ 5 ms) fail | three-phase write: compute against a snapshot before the lock, commit by CAS under it, recompute once only if the tip moved |
| S3 | major | **`mcp_tool` hooks can remove most hook spawns** (verified on the current docs): `UserPromptSubmit`, `SubagentStart`, `SubagentStop`, `PostToolUse`, `PreToolUse`, `SessionStart(clear\|compact)`; the design's premise for a spawning stamp is outdated | 25–110 ms per stamped MCP write, 45–250 ms per `SubagentStart`, 30–180 ms per prompt; the `Write\|Edit` evidence hook is kept off only because of spawn cost | `mcp_tool` variants on the session's moirai server; stamp by server-side context keyed on the idempotency key (or `updatedInput`); M0 experiment |
| S4 | major | Inert `settled`/`deleted` markers are never compacted, so `brief_triage` and every `RuntimeScan(markers)` are **O(markers ever) = O(history)** | ≈ 30k markers/year × 0.2–1 µs = 6–30 ms per year of history on every brief; the "≤ 50 µs" row is internally inconsistent | fold globally inert markers out of `MARKERS` at checkpoint; scan active markers only |
| S5 | major | `dirty N` in every file-bearing header, per-lane dirty counts in `brief`, and other lanes' `files_owned` in every pack header are **O(tracked files) / O(lanes × files) / O(lanes × overlay)** reads; `lane.dirty_files` is a *versioned* field | +15–40 ms warm (×10 cold) per file-bearing command [M-70]; +45–200 ms warm per `brief`; +3–5 other-lane overlays per pack | runtime `TREES.dirty` written by settles/hooks, printed with its age; do-not-touch globs captured into the lease row |
| S6 | major | **Git work on read paths is unmetered or under-metered**: E6 windows up to 2,000 commits, HEAD-tree lookups and uncached ancestry are not (or barely) charged to `fs` units; [50] computes `staleness()` inside every pack, contradicting [AR §2.14/§3.5] | a pack or show can spend up to the 2 s wall deadline on E6; 1–5 ms per uncached pin per pack, repeated by every process | charge git objects to `fs`; reads use cached `GITFACTS`/`ANCESTRY` plus ≤ 1 uncached pair and ≤ 32 E6 commits, else `unverified (git)` |
| S7 | major | The `(anchor, oid)` cache and `last_oid` live only in a process or in `FILEOBS` refreshed at settles, so **every CLI read re-hashes every edited linked file and re-runs its anchors** between settles; the 3 ms gate fixture omits edited files | a 50-link pack with 10 edited files ≈ 8–15 ms of link work, ~4× the 3 ms gate, during exactly the phase (developer editing `files_owned`) when packs are read | runtime `ANCHORRES` rows written by settles and a spawn-free `Write\|Edit` evidence hook (S3); gate with edited files |
| S8 | major | Settles rewrite `FILEOBS` for every verified link (`verified_at`), and `SessionStart` settles "others until the budget" on every start, `/clear` and compaction | 0.2–0.33 MB of lazy records per settle at the owner's 2.2k linked files; 10–33 MB/day; 3–8 extra delta checkpoints/day; opens pushed to the full-tail worst case (3–5 ms vs ≤ 1.5 ms) | per-settle epoch record + rows only for changed files; SessionStart scope = brief + lane `files_owned` |
| S9 | major | The MCP server's single `current_thread` runs **rollup after a request unsliced** (0.1–0.3 s at 1e5, 1–3 s at 1e6), and `links_sync` (2 s budget), merges and packs block every other subagent's calls — and every `mcp_tool` hook once S3 lands | head-of-line stalls up to 3 s for all 16 subagents of a session | resumable ≤ 5 ms slices for rollup/promotion/checkpoint/settle in the server; MCP p99 gate under a burst |
| S10–S18 | minor | fixed per-invocation file opens unbudgeted; incremental image export ignores measured rename cost; unbounded delta scans and a writer-byte cursor append in the prompt hook; flush grouping per verb; the 50-link gate under load; maintenance in an agent's CLI tail; a 1 s hook gate; lane pack composition (per-pack `diff`, C7 glob scan); per-process BM25 statistics and schema load | see §4 | see §4 |

What matters most for the owner's four axes: **process spawns dominate every CLI call (134–189 ms p50, 285–350 ms p90 through Git Bash, of which the engine is 2–7 ms)**, so the largest speed wins are the ones that remove spawns (S3) and the ones that stop per-process recomputation of things that only change at settles or syncs (S1, S5, S6, S7). The engine's micro-costs are already an order of magnitude below the spawn floor.

---

## 1. Inputs used

### 1.1 Measured unit costs

| Quantity | Value | Source |
|---|---|---|
| Git-Bash wrapper of the agent Bash tool (`bash.exe -c true`, ~100 % load) | 93.4 / **109.0** / 235.2 ms (min / p50 / p90) | [M, 05 §2.1] |
| Rust executable spawn-to-exit | 25.2 ms p50 (48.5 p90) at 30–50 % load; 73.1 ms p50 (108.4 p90) at ~100 % | [M, 05 §2.1] |
| Tiny native exe spawn | 34.0 ms avg | [M, 08 §2] |
| `git --version` through Git Bash today | 132–203 ms (n = 3, loaded) | [M-70] |
| Data-only flush (`NtFlushBuffersFileEx`) | 1.66 / **1.73** / 1.82 / 3.32 ms (min / p50 / p90 / p99); append + `FlushFileBuffers` p99 5.7, max 13.7 | [M, 05 §2.2] |
| `CreateFile` + read 4 KiB + close (Defender on) | 0.163 / **0.172** / 0.201 / 0.354 ms | [M, 05 §2.2] |
| open + `CreateFileMapping` + `MapViewOfFile` | **0.222** ms p50, 0.369 p90 | [M, 05 §2.3] |
| first touch of a cached page / re-touch | ~1 µs / 0.06 µs | [M, 05 §2.3] |
| `GetFileAttributesExW` | 17–67 µs | [M, 13 §1.7] |
| directory enumeration with 128-bit ids | 12–22 µs per entry warm, ~10× cold | [M, 09 §8] |
| stat of each of the 2,556 tracked BoykoEngine files / enumeration of their 272 directories (Python, warm) | 33–36 ms (13.0 µs per file) / 23–24 ms (8.3 µs per entry, 2,823 entries) | [M-70] |
| `git status --porcelain -uno` on BoykoEngine, incl. spawn | 155–186 ms (spawn alone 132–203 ms) | [M-70] |
| `OpenFileById` + path: file / directory | 0.24–0.57 ms / 0.17–0.40 ms | [M, 09 §8], [M, 40 §0.3] |
| open + read + hash one file | ~150 µs warm, ~550 µs cold, + size at ~1.2 GB/s | [M, 10 §5.6] |
| anchor resolution: exact / windowed fuzzy / whole-file fuzzy | 10–40 µs / 0.17–0.69 ms / 1.4–8.6 ms | [M, 11 §2.8] |
| file rename p50 / max | 3.9–9.5 ms / 156 ms | [M, 13 §1.4] |
| named-pipe round trip | ≈ 60 µs | [M, 08 §2] |
| git process spawn in a hook-like context | 130–750 ms, 2.1 s under load | [M, 10 §5.3] |
| SHA-1 / SHA-256 | 2.00 / 1.94 GB/s | [M, 05 §2.5] |
| BoykoEngine object store | 0 loose objects, 4 packs + a multi-pack-index, a 2-file commit-graph chain; 2,556 tracked files in 272 directories; 44 worktree entries | [M-70] |

### 1.2 Workload inputs

~1k commits/day store-wide, 3–5 live lanes, ~50 live refs worst case [AR §8.1, est.]; scratch and `wf_*` worktrees write to `main` [§2.3]; ~11 Workflow runs/day with p50 4 / p90 14 agent calls each, bursts of 16 concurrent agents [M, 02 §12.6]; ~30k markers/year [§5d.1]; ≈ 2.2k cited files and 10–35k citations [M, 11 §2.1]; 23,754 `Edit`/`Write` calls in the transcript census [M, 13 §1.1].

### 1.3 Claude Code hook facts verified on 2026-09-26 [D]

From the hooks reference (`code.claude.com/docs/en/hooks`, fetched as Markdown and read in full for these points):

- There are five handler types: `command`, `http`, `mcp_tool`, `prompt`, `agent`. For `mcp_tool`: "The tool's text output is treated like command-hook stdout." The fuller paragraph says the text is parsed by the same exit-code-0 rule as command stdout, i.e. text that starts with `{` and ends with `}` is parsed as the JSON output object, whose `hookSpecificOutput` fields are the event's decision and context fields. A disconnected server, or `isError: true`, is a non-blocking error and execution continues.
- **Events that accept `mcp_tool`**: all five types on `PreToolUse`, `PostToolUse`, `PostToolUseFailure`, `PostToolBatch`, `UserPromptSubmit`, `Stop`, `SubagentStop` and others; `command`/`http`/`mcp_tool` on `SubagentStart`, `PreCompact`, `SessionEnd` and others; `SessionStart` accepts `command` and `mcp_tool`, but its `mcp_tool` hooks are **skipped at launch** (`startup`, `--continue`/`--resume`) and run after `/clear` or a compaction.
- `PreToolUse` decision control lists `permissionDecision`, `updatedInput` (replaces the whole input object) and `additionalContext` inside `hookSpecificOutput`; the only hook-type restriction the reference states for decision objects is on `PermissionRequest` (command or HTTP). `SubagentStart` accepts `hookSpecificOutput.additionalContext`; `UserPromptSubmit` adds plain text or `additionalContext` as context.
- `mcp_tool` `input` string values support `${path}` substitution from the hook's JSON input (the example substitutes `${tool_input.file_path}`); common input fields include `session_id`, `cwd`, `hook_event_name`, and inside a subagent `agent_id` and `agent_type`. Whether a whole object (`${tool_input}`) substitutes as JSON text is **not documented**.
- Command hooks have an exec form (`args` present: spawned directly, no shell) and a shell form (Git Bash on Windows). [AR §7.5] already uses exec form, so a command hook costs one native spawn, not the Git-Bash wrapper.
- Timeouts default to 600 s (30 s on `UserPromptSubmit`); `async` exists for command hooks only; all matching hooks run in parallel.

[22 §10 W-all-1] read the same page on 2026-09-25 and concluded that `mcp_tool` hooks cannot return `updatedInput` because the page "lists no `updatedInput`" for them. The current text parses `mcp_tool` output exactly like command stdout and restricts decision fields by event, not by handler type, so that conclusion does not follow from the documentation (S3). It still needs a five-minute experiment, which S3 specifies.

---

## 2. Hot paths, end to end

Every total below is recomputed from the documented layouts and algorithms and the §1 inputs; "as specified" means the design as written, before the fixes of §4.

### 2.1 CLI cold start and open (read verb on `main`, 1e5)

| Step | Cost | Basis |
|---|---|---|
| agent Bash tool → Git Bash | 109 ms p50, 235 ms p90 | [M] |
| Git Bash → `moirai.exe` | 25–73 ms p50, 48–108 ms p90 | [M] |
| discovery: walk-up probes for `.moirai` and `.git` at 3–6 levels | 6–12 stats × 17–67 µs = 0.1–0.8 ms | §2.14, [M] |
| read `.git` file, `commondir`, `config` | 3 × 0.17 ms = 0.5 ms | [M] |
| binding lookup (`HEADS`, one probe per ancestor directory) | µs | §5a.4 |
| `HEAD` pread of both slots | 0.01–0.17 ms | §4.2 |
| map 4–8 segment files | 0.9–1.8 ms p50 (1.5–3.0 p90) | §4.7, [M] |
| open the active log, replay ≤ 4,096 ops | 0.17 ms + 0–3 ms (3–5 ms at a full 4 MiB tail) | §4.7, [20 U13] |
| query (`get`, `ready` page, `blocking --ids`) | 1 µs – 0.3 ms | §8.1 |
| file-bearing header: git HEAD + ref or `packed-refs` | 2–3 × 0.17 = 0.3–0.5 ms | §7.1 |
| **engine total** | **2.0–7.1 ms** (typically 2.5–4) | |
| **whole call** | **136–189 ms p50; ≈ 285–350 ms p90** | |

The ≤ 5 ms engine gate at 1e5 holds only when the tail is not full and when the fixed per-invocation file opens (7–13 opens ≈ 1.5–3.3 ms) are counted, which no budget row does (S10). [§8.1]'s open row "0.3–1 ms at 1e4, 0.5–1.5 ms at 1e5" cannot contain "≤ 8 maps × 0.22 ms" (up to 1.8 ms p50) plus discovery and config.

### 2.2 `get` / `show`

Engine: `get #N` 1–5 µs [§8.1]; `show` with neighbours and tombstone rendering tens of µs; each rendered link adds one stat (17–67 µs). Header as specified adds `dirty N` for the resolved tree: 15–40 ms warm, ×10 cold (S5). Per call: spawn-dominated, 136–189 ms p50; with the header's dirty count, +15–40 ms.

### 2.3 `ready` / `blockers` / `blocking`

`ready` page: bitset AND + ≤ 12-parent ancestor walk + one `MARKERS` probe and one absorbed-vector lookup per candidate: 50–300 µs at 1e5, 0.3–3 ms at 1e6 [§8.1] — sound (CM1 removed the DAG walk). `blockers --transitive`: reverse CSR walk, 10–200 µs. `blocking --ids`: `is_blocker ∧ kind:task ∧ ¬settled`, ≤ 100 µs at 1e5 — sound as long as `¬settled` is evaluated by probes, not by materialising a bitset from the whole `MARKERS` table (S4). On a lane, add the overlay build (§2.4).

### 2.4 Lane reads: the overlay build

[§5a.3] defines `view(X) = SEG(pin_X) ⊕ ops(main, (P, fork]) ⊕ fold over X's commits`, where each `sync` commit expands **by reference** into `ops(main, (M_{k−1}, M_k])`. The overlay-build paragraph states the cost as O(X's commits + `main`'s commits in synced windows) but prices only the first term: "a 14-day lane with ~700 own commits ≈ 5–10 ms". The second term is `main`'s entire traffic since the fork for any lane that syncs, and lanes sync constantly: [§7.5]'s `SubagentStart` hook auto-applies every clean sync, and merges into `main` are sync-first.

Recomputed for a 14-day lane (est.): `main` receives 300–600 of the ~1k commits/day (scratch and `wf_*` worktrees and the dispatcher's `main`-side writes land there) plus 2–4 lane merges of ~2k folded ops each → 4.2–8.4k commits and ≈ 10–33k ops in the synced windows. At the design's own rate (700 commits ≈ 5–10 ms, i.e. 7–14 µs per commit) that is **30–120 ms** per first read in a process, and at ~150 B per op **1.5–5 MB** of private memory, against "≤ ~1 MB per branch read" and a 4 MB CLI gate whose baseline is already 2–4 MB. Promotion does not rescue it in time: `ops_since_fork > 8,192` counts the lane's own ops (≈ 700 commits), the 8 MiB overlay trigger fires only at ≈ 55k ops, and "16 checkpoints behind" fires after 8–16 days at 1–2 checkpoints per day. On days ~2–16 of every lane, every lane CLI command, every hook on the lane and every MCP LRU miss pays the build (S1).

### 2.5 LQ: parse, bind, plan, execute

| Example ([50 §5.12]) | Engine cost | Comment |
|---|---|---|
| parse + bind + plan, any query ≤ 1 KB | 5–20 µs (3.9 µs measured on the probe parser [M, 15 §12]) | sound; the first query of a process also materialises the view's schema-as-data (est. 20–200 µs), which no row budgets (S10) |
| Q1 lookup, Q3 subtree, Q4 blockers, Q5 closures, Q6/Q7/Q23 aggregates | 10 µs – 0.3 ms at 1e6 | sound |
| Q2 filter + label + top-k | 1–2 ms at 1e6 | sound |
| Q8 search | tier 2: 0.3–5 ms + 1–3 ms of view statistics once per lane process | per-process recomputation on lanes |
| Q9 file links under a directory | 0.1–3 ms, all present | +8–15 ms when files were edited since the last settle (S7) |
| Q12 two refs | +1–10 ms per ref overlay | +30–120 ms per lane ref as specified (S1) |
| Q13 three-dot diff of a 2k-op lane | 1–5 ms at 1e5 | runs inside **every lane pack** through `pack_rules_unmerged` (S17) |
| `brief_triage` | "≤ 50 µs" | O(markers ever): 6–30 ms per year of history (S4) |
| C6 `staleness()` in `pack_measurements` | 1–5 ms per uncached pin, every pack | contradicts [AR §2.14/§3.5] (S6) |
| `TX` writes | ~2 ms p50 (one flush) | the `MATCH` part runs inside the writer byte (S2) |

### 2.6 Durable commit and fsync counts

As specified in [§4.5] and [50 §5.9], step 1 takes the writer byte and steps 2–7 then read `HEAD`, run the recovery probe, replay the tail, resolve the branch, validate and apply — so a writer that has not already opened the store replays up to a full tail (1–5 ms) and, on a lane, builds the overlay (5–10 ms; 30–120 ms per §2.4) while holding the byte. The engine's in-lock minimum is the flush (1.73 ms p50, 3.3 ms p99) plus ~0.2 ms, i.e. ≈ 2–2.5 ms, which is what the 16-writer "32–40 ms last ack" arithmetic of [§6.1] assumes. With the in-lock work as specified, 16 fresh lane writers hold 8–20 ms each (up to 130 ms with S1): **last ack 130–320 ms against the p99 ≤ 50 ms gate** (S2).

| Operation | Flushes | Notes |
|---|---|---|
| ordinary durable commit, `claim`, `complete`, `apply` batch, `TX` | 1 | sound; gated as "exactly one" |
| `lane open` (branch + bind + lane node) | 3 as written ([§7.6] step 3: "three durable commits, ~6–9 ms") | one flushed group suffices (S13) |
| sync-first `merge` into `main` | 1 or 2, unspecified ("in the same writer transaction") | state one group (S13) |
| `file mv` / `file rm` | 2 (`FsIntent`, then the commit with `FsIntentDone`) | required by the crash protocol; sound |
| delta checkpoint | 3 (segment, blob file, `Checkpoint` record), outside the writer byte | sound |
| `image export`, checkpoint | 1 for `gitmap` + pack, idx and every ref file (flush policy unspecified) | S11 |
| lazy records (heartbeats, cursors, `FileObs`, evidence) | 0, but each append takes the writer byte | S8, S12 |

### 2.7 Merge and sync

Compute cost (2k-op lane, 5k trunk ops since the LCA): two folds 1–3 ms + base as-of for ~200 keys 1–5 ms + typed rules + validators = 10–40 ms at 1e5, 20–80 ms at 1e6 (+5–50 ms for full Kahn) [§5a.7]. Sync-first merges into `main` run the sync as step 0 "in the same writer transaction", so a merge holds the byte for sync + merge: **20–80 ms at 1e5, 40–160 ms at 1e6** — during which all 16 agents' writes queue. The `SubagentStart` auto-sync is also a write: under a 16-subagent fan-out the first hook holds the byte 10–40 ms for the sync. Nothing in [§5a.7] puts steps 1–6 outside the lock (S2).

### 2.8 Image export and import

Full export 1e5: 1–2.5 s, full import 2–7 s — explicit verbs, sound. Incremental checkpoint export of `main` + 2 lanes, recomputed with the measured rename cost: one pack + idx written by temp-then-rename (2 × (create 0.2 + write + flush 1.8 + rename 3.9–9.5 ms) ≈ 12–23 ms) + 3 loose ref updates through `.lock` + rename + read-after-write (3 × 6–12 ms ≈ 18–35 ms) + the side ref (≈ 6–12 ms) + the `gitmap` commit (2 ms) ≈ **38–72 ms**, against "5–20 ms" [§5b.9, §8.1] and "≤ 20 ms" [60 §5.4] (S11). Incremental import of one commit (tree diff against the first parent, parse touched `.moi` files, canonical re-hash, validators, one commit): 5–20 ms, plausible.

### 2.9 File-link read and settle paths

| Path | As specified | Budget |
|---|---|---|
| 50-link pack, all present, unchanged since the last settle | 50 stats × 17–67 µs = 0.85–3.35 ms | ≤ 3 ms p50 warm — met idle, missed under load at the upper stat cost (S14) |
| same, 10 linked files edited since the last settle (3 anchors each, a few fuzzy) | + 10 × (150 µs + ~200 µs of hashing) + 25 × 30 µs + 4 × 0.4 ms + 1 × 1.4–8.6 ms ≈ **8–15 ms** | fixture omits it (S7) |
| all 50 missing under one moved directory | 2 × 50 stats + one HEAD-tree lookup (MIDX + pack maps 0.4–0.7 ms, 3–6 trees with delta chains) + one directory `OpenFileById` (0.17–0.40 ms) + cached ancestry ≈ 3–6 ms | ≤ 5 ms p50 — plausible |
| one link absent, gate row G4 (not in τ(HEAD), no alias there) | E6 over W = `merge-base(g, H)..H`, ≤ 2,000 commits × (0.1–1 ms of tree diffing per commit) = up to 0.2–2 s; only the 2 s wall deadline bounds it, because `fs` units do not charge git objects | ≤ 20 ms "file work" (S6) |
| uncached ancestry beyond the commit-graph | generation-pruned walk over the commits the graph lacks (95 at [41]'s probe, growing until the next graph write), each decoded through the MIDX: est. 1–4 ms | ≤ 5 ms [60 M4] — plausible now that every object is packed [M-70]; charged only 4 `fs` units |
| file-bearing header `files @ tree (branch head, dirty N)` | dirty count: 15–40 ms warm (stat or enumerate 2,556 tracked files [M-70] + index parse), ×10 cold | none (S5) |
| `SessionStart` settle | brief items, lane `files_owned`, then "others until the budget": at 2.2k linked files the whole set fits in 37–147 ms of stats; + 50 ms quiescence and one commit when it writes | ≤ 150 ms hard cap — met, but spends the cap on every start and compaction and writes a `FileObs` row per verified link (S8) |
| settle writing k exact moves | 52 ms + µs × k | sound, but see S2 for `complete` |

### 2.10 Hooks

Command hooks are exec form (no Git Bash), so each costs one native spawn (25–73 ms p50, 48–108 ms p90 under load) plus its engine work.

| Hook | Frequency | Command hook, as specified (p50, loaded) | `mcp_tool` on the session's moirai server (est.) |
|---|---|---|---|
| `SessionStart` startup/resume | per session start | spawn + open 1–4 + lane overlay 5–120 + brief 2–12 (+ per-lane dirty 45–200, S5) + settle ≤ 150 → **185–360 ms** (up to ~550 with dirty counts) | not available at launch [D] |
| `SessionStart` clear/compact | per `/clear` and compaction | same | brief + settle, no spawn, warm overlay: 20–160 ms |
| `UserPromptSubmit` | per owner prompt | spawn + open + overlay + delta scan + link re-stat + a cursor append that takes the writer byte → **30–180 ms**, before the model sees the prompt | 0.2–3 ms; cursor kept in the server's memory |
| `SubagentStart` | ~44–150/day, bursts of 16 | spawn + open + overlay + `sync --check` fold 5–40 + optional sync commit (10–40 ms hold) + role pack 2–8 + links → **45–250 ms** per subagent | 3–40 ms, serialised in the server (S9) |
| `PreToolUse` stamp on MCP writes | per MCP write | **25–110 ms** | 0.1–0.5 ms |
| `SubagentStop` | per subagent | 30–120 ms | 1–5 ms |
| `PostToolUse(Agent)`, async | per Agent launch | 25–110 ms of CPU | < 1 ms (synchronous, but fast) |
| `fs-evidence` on `mv`/`rm`, async | ~0.9 % of shell calls | 25–110 ms of CPU | 0.3–1 ms |
| `Write\|Edit` evidence (off by default) | 23,754 edits in the census | 23,754 × 15–73 ms = **6–29 CPU-minutes** — the reason it is off [40 §4.7] | 23,754 × 0.3–0.7 ms ≈ 7–17 s |

### 2.11 MCP calls

Unstamped read: `pread HEAD` (10–170 µs) + tail catch-up + a warm overlay (LRU of 8) + query → 0.1–5 ms [§8.1], sound in isolation. The server is one `current_thread` process per Claude session serving every subagent; long operations are not sliced — rollup after a request (0.1–0.3 s at 1e5, 1–3 s at 1e6), `links_sync` (2 s default budget), merges (10–80 ms), packs with link work (5–30 ms). Queries are sliced for cancellation [50 §5.10]; nothing else is. A 16-subagent burst of `pack` calls serialises to 80–480 ms for the last caller, and one post-request rollup at 1e6 stalls every caller for 1–3 s (S9).

### 2.12 Hidden complexity classes found on hot paths

| Where | Hidden class | Finding |
|---|---|---|
| lane view construction | O(`main` traffic since the fork) | S1 |
| `brief_triage`, any `RuntimeScan(markers)` | O(markers ever) = O(history) | S4 |
| file-bearing headers; `brief` lane lines | O(tracked files); O(lanes × tracked files) | S5 |
| pack C1 do-not-touch list | O(live lanes × lane overlay build) | S5 |
| read-path E6 window | O(commits in the integration window), ≤ 2,000 | S6 |
| `staleness()` in every pack | O(pins × commits beyond the commit-graph) per pack | S6 |
| every settle | O(links in scope) log bytes, even when nothing changed | S8 |
| prompt delta for an old cursor | O(commits since the cursor) | S12 |
| pack C7 | O(knowledge nodes with path globs) | S17 |
| MCP server rollup | O(N) on the request thread | S9 |
| O(worktrees) | none found on a hot path: tree identity, eligibility and bindings are O(1) per command; `links check --all` never walks other worktrees ([40 §4.8]) | — |

---

## 3. The `mcp_tool` question

The task asked whether `mcp_tool` hooks exist for `PostToolUse`, `UserPromptSubmit` and `SubagentStart`, so that per-event hooks can avoid spawns when the moirai MCP server is connected. **Yes, for all three, and also for `PreToolUse`, `SubagentStop`, `PostToolBatch` and `SessionStart` after `/clear` or compaction** (§1.3). Consequences for the design:

1. **Context hooks become spawn-free.** `UserPromptSubmit` (delta), `SubagentStart` (role pack; `additionalContext`), `SubagentStop` (lease safety net), `PostToolUse(Agent)`, the R4 evidence hooks and `SessionStart(clear|compact)` can call tools on the moirai server that the plugin already ships. The server has warm maps and branch overlays, so the per-event cost falls from 30–250 ms to 0.1–40 ms (§2.10).
2. **The stamp can become spawn-free.** Two routes, to be decided by a five-minute M0 experiment (added to [60 §5.2] item 7):
   - (a) `updatedInput`: the `mcp_tool` `PreToolUse` hook passes `${session_id}`, `${agent_id}`, `${agent_type}`, `${cwd}` and the tool input, and the server returns `hookSpecificOutput{permissionDecision, updatedInput}`. This needs whole-object substitution of `${tool_input}` (undocumented) or the server's knowledge of each write tool's fields, and confirmation that `updatedInput` from an `mcp_tool` hook is honoured.
   - (b) **Server-side context, documented features only**: the hook calls `moirai.stamp{session_id, agent_id, agent_type, cwd, key: "${tool_input.idempotency_key}"}`; the server keeps the context in memory keyed by (session, key) and returns only `permissionDecision`; the write tool call that follows on the same connection finds it by its idempotency key. Every MCP write already carries a key (§6.4); a write without a key falls back to the explicit `branch` parameter, as reads do today.
3. **Limits.** `SessionStart` at launch stays a command hook (skipped otherwise [D]). A disconnected server makes an `mcp_tool` hook a non-blocking error, so the context for that event is missing rather than wrong; the server instructions already say "call `brief` first, `pack` before working". `mcp_tool` hooks are synchronous (no `async`), which is fine only because they are sub-millisecond — **S9 must be fixed first**, or a rollup in the server stalls every hook.
4. **Revisit trigger fired.** [§2.9]'s revisit trigger ("a harness release letting `mcp_tool` hooks return `updatedInput` → stamp everything for free") and [20 T9]'s "an `mcp_tool`-type `PreToolUse` hook … removes the write-stamp spawn" are satisfied by the current documentation, subject to the experiment.

---

## 4. Findings

Each finding gives its location, the concrete cost scenario, and a fix that respects the owner decisions (no third-party database, no interim stages, built once to final specification; operational policy as documented `config` keys, not owner decisions). "Format" marks a fix that touches something frozen at M0 exit.

### S1 — blocker — the lane view is O(`main` traffic since the fork)

**Where.** [AR] §5a.3 (view definition, overlay build, promotion), §5a.10 row "first read … 14-day lane 5–10 ms", §8.1 row "First read on a lane", §8.1 CI gate "≤ 10 ms on a lane including the first-read overlay build at a 14-day fork distance", §7.5 `SubagentStart` auto-sync; [60] §3.2 (M1 view construction "by-reference expansion of sync windows"), §5.2 item 3, §5.4 rows "first read of a ref forked … 14k commits ago ≤ 10 ms" (M1) and "lane read incl. first-read overlay at a 14-day fork ≤ 10 ms" (M3).

**Problem.** A sync commit stores only resolutions and expands by reference into `main`'s window, so the view of a lane that syncs includes every `main` op since the fork. The estimate prices only the lane's own commits; the promotion triggers count the lane's own ops, an 8 MiB overlay, or 16 checkpoints of age. None of them tracks the synced windows.

**Scenario (est., §2.4).** Lane `l5np`, 14 days old, synced at every `SubagentStart` (auto-applied when clean) and before its merge. `main` got 300–600 commits/day plus 3 lane merges of ~2k ops. Its view replays 4.2–8.4k `main` commits (10–33k ops): **30–120 ms and 1.5–5 MB private** per first read in a process. That lands on every lane CLI call (5–10 per agent), every `SubagentStart`/`SessionStart` hook on the lane, every `--across`/`USE lane/…` query and every MCP LRU miss, on days ~2–16 of every lane. The M1 fixture ("forked 14k commits ago", no syncs) passes; the daily path fails the M3 gate by 3–12×, and the RSS gate (4 MB, baseline 2–4 MB) by up to 2×.

**Fix.** (1) Define the promotion trigger on the **overlay the view actually builds**: `overlay_ops = ops(main, (P, fork]) + own ops + ops of every synced window since the last promotion`, maintained in the ref table at each commit and sync (O(1)). Set the threshold from the 10 ms gate at the measured build rate (M0 item 3; est. 3–5k ops). (2) Let the process that commits a sync crossing the threshold run the promotion after releasing the writer byte, under the maintenance byte (20–60 ms once), exactly as delta checkpoints already run. (3) Extend M0 item 3 and the M1/M3 gates with the fixture "lane synced at every `SubagentStart` for 14 days while `main` took ~500 commits/day and three lane merges". *Format*: the `overlay_ops` counter in the ref-table entry and its use in `Checkpoint` per-ref lists are frozen items; the thresholds stay store parameters. No owner decision; `sync --refork` (history-rewriting) is not needed.

### S2 — major — heavy work runs inside the writer byte

**Where.** [AR] §4.5 steps 1–7 (lock first, then `HEAD`, recovery, tail replay, branch resolution, validation, apply), §5a.7 step 0 ("in the same writer transaction") and steps 1–7, §3.5 `suspect` closure (10k-op budget), §6.1; [40] §4.2 row `complete` (settle "within the same commit", "≤ 20 ms plus the quiescence wait if it writes"), §4.2 quiescence; [50] §5.9 (steps 3–7), §3.10 / [AR §7.7.1] (5e5 work units in the lock); [60] §5.4 row "writer-wait p99 ≤ 50 ms; writer-byte hold p99 ≤ 5 ms".

**Problem.** Nothing in the specification says that a writer opens the store, replays the tail, builds its branch overlay, resolves its branch, binding and git provenance, and computes its changeset *before* taking byte 0. As written, all of that happens with the byte held; merges, syncs, `TX` `MATCH` evaluation, reverts, cherry-picks, `rm`'s `suspect` closure and `complete`'s settle are computed there too.

**Scenario (§2.6, §2.7).**
- 16 developer agents on lanes each run one write from a fresh CLI process: hold = tail replay 1–5 ms + overlay 5–10 ms (30–120 ms with S1) + provenance 0.5–1 ms + flush 1.7–3.3 ms = 8–20 ms each → **last ack 130–320 ms** (gate: p99 ≤ 50 ms).
- `merge lane/l5np --into main` at 1e5 holds 20–80 ms (sync + merge); at 1e6 40–160 ms + 5–50 ms Kahn.
- `complete 89` whose subtree has a moved linked file: settle ≤ 20 ms + **≥ 50 ms quiescence** + flush → **≥ 72 ms hold** on every completion that re-binds.
- A `TX` at its 5e5-unit cap: 2.5–10 ms; `rm` of a heavily cited rule: up to the 10k-op closure, est. 5–20 ms.

**Fix.** Specify a three-phase write for every verb (engine rule, M1; no leader needed): (1) **before the lock**: open, replay to `committed_lsn` = L0, build the branch overlay, resolve branch/binding/provenance, parse/bind/plan, and compute the full candidate changeset against the snapshot at L0 — for merges and syncs the whole of steps 1–6, for `TX` the `MATCH` targets, for `complete` the settle **including its quiescence wait**, for `rm` the closure; (2) **under the lock**: `pread HEAD`; if `committed_lsn` = L0, append + flush + publish (hold ≈ flush + 0.2 ms); otherwise replay (L0, L1] and re-validate by key: if no touched key's `rev_seq`, no ref tip and no marker involved changed, commit; else recompute once under the lock (or release and retry ≤ 2×). This is [50]'s `DRY` + `IF TIP` generalised; `EXPECT` semantics are preserved because the in-lock path re-evaluates whenever the tip moved. (3) After release: print, then maintenance. Gate: writer-byte hold p99 ≤ 5 ms **per verb class** (merge, sync, `complete` with settle, `TX`, `rm` included) and max ≤ 20 ms; 16-writer p99 ≤ 50 ms with a concurrent merge in the fixture.

### S3 — major — hook spawns that `mcp_tool` hooks remove

**Where.** [AR] §1 row 9 ("`mcp_tool` hooks cannot rewrite input, so the stamp is a command hook"), §2.9 (decision, rejected alternatives, revisit trigger), §7.2 (stamp), §7.5 hooks table; [22] §10 W-all-1, §3.4 D2; [40] §4.7 and §6.4 (`Write|Edit` evidence hook off because it costs "one async process per edit"); [60] §5.2 item 7.

**Problem and evidence.** §1.3 and §3: the current reference parses `mcp_tool` output exactly like command stdout, lists `mcp_tool` for `PreToolUse`, `PostToolUse`, `PostToolBatch`, `UserPromptSubmit`, `SubagentStart`, `SubagentStop` and post-launch `SessionStart`, and restricts decision fields by event, not by handler type. The design spawns `moirai.exe` for all of them.

**Scenario.** Per stamped MCP write 25–110 ms; per `SubagentStart` 45–250 ms (a 16-agent fan-out on a loaded machine spawns 16 processes at once; spawn p90 there is 108–182 ms [M]); per owner prompt 30–180 ms before the model starts. The `Write|Edit` evidence hook, which would make edit-then-move exact and keep S7's caches fresh, stays off solely because 23,754 spawns cost 6–29 CPU-minutes.

**Fix.** (1) Ship `mcp_tool` variants for `UserPromptSubmit`, `SubagentStart`, `SubagentStop`, `PostToolUse(Agent)`, the R4 evidence hooks and `SessionStart` with matcher `clear|compact`; keep `SessionStart` `startup|resume` as the exec-form command hook. (2) Stamp route (b) of §3 (server-side context keyed on `(session, idempotency key)`), or route (a) if the experiment confirms `updatedInput` and whole-object substitution. (3) Keep per-session cursors in the server's memory instead of lazy log records (removes S12's writer-byte append). (4) `config` key `hooks.transport = mcp | command | auto` (default `auto`: `mcp_tool` wherever the plugin's server is configured), written by `moirai hooks install`; operational policy, not an owner decision. (5) Add the experiment to [60 §5.2] item 7 and re-run it in M9. (6) Informational: Bash roles may also read through the already-running server (`pack`, `get`, `query`: 0.1–5 ms instead of 136–189 ms) — a skill-level choice to weigh against the token cost of loading deferred MCP schemas, which is the tokens auditor's call.

### S4 — major — `MARKERS` grows forever, so marker scans are O(history)

**Where.** [AR] §5d.1 ("Cost: ~30k markers/year at 40 B"), §4.4 `MARKERS` section, §3.5 `settled_elsewhere` row, §8.1 R5 row "`brief_triage` ≤ 50 µs … O(markers), ≈ 30k markers/year"; [50] §5.4 rule 5, §5.5 `RuntimeScan` "O(table rows); ~0.2–1 µs per row", §5.12 `brief_triage` row; [60] §5.4 R5 rows.

**Problem.** A marker becomes inert for a branch once that branch absorbs it, but no rule ever removes a marker that every live branch has absorbed, so `MARKERS` holds every completion and deletion ever made. [51 E1]'s fix anchored `brief_triage` on `RuntimeScan(markers)` precisely because the table was assumed small.

**Scenario.** After one year: 30k rows × 0.2–1 µs = **6–30 ms** per `brief_triage`, which runs in every `brief` (every `SessionStart`) and in any query anchored on markers; after three years 18–90 ms. The gates "`brief_triage` ≤ 5 ms at 1e5" and "brief ≤ 8 ms" fail by the end of year one. The table row "≤ 50 µs … O(markers), ≈ 30k markers/year" contradicts itself.

**Fix.** A marker is *globally inert* when it is `cleared` or when every live ref R has `absorbed_R[ref_id] ≥ ref_seq` (deleted refs excluded). At every checkpoint fold, move globally inert markers from `MARKERS` to a cold `MARKERS_OLD` section (kept for `doctor`, triage and history, never scanned by `RuntimeScan`); computing the minimum absorbed position per ref is O(live refs²), ≤ 2,500 lookups. `MARKERS` then holds only unabsorbed completions and deletions (dozens). *Format*: the section split and the fold rule are frozen items. Gate: `brief_triage` ≤ 50 µs with a fixture of 90k inert and 100 active markers.

### S5 — major — per-command worktree scans and per-lane state in headers

**Where.** [AR] §3.2 `lane` fields (`dirty_files` u16, versioned), §7.1 header convention (`files @ <tree> (<git branch> <head>[, dirty N])`), §7.4 C1 ("dirty count, other active lanes' `files_owned` (do-not-touch)"), §7.4 `brief` ("live lanes (… dirty count)"); [40] §5.1, §6.1; [50] §6.4 header; [50] §4.3 `pack_header`.

**Problem.** No section says how `dirty N` is obtained. Computing it means stat-ing or enumerating every tracked file and comparing with the git index — O(tracked files) — on every file-bearing command, and O(lanes × files) in every brief. The alternative reading, the versioned `lane.dirty_files` field, would make every refresh a durable commit on `main` and a `.moi` rewrite in the image. The pack's do-not-touch list needs `files_owned` of tasks on *other* lanes, which lives in those lanes' views: O(live lanes) overlay builds per pack, or a stale value from `main`.

**Scenario [M-70].** Stat of BoykoEngine's 2,556 tracked files: 33–36 ms warm; enumeration of their 272 directories: 23–24 ms; git's own `status -uno` does the same work in ≈ 15–50 ms beyond its spawn. A file-bearing `show` or `pack` therefore pays **+15–40 ms warm (×10 cold)** for one number — 3–8× the whole 5 ms engine budget. `brief` with 3–5 live lanes pays **45–200 ms warm** at every `SessionStart`. The pack header adds 3–5 other-lane overlay builds (15–600 ms as specified, 15–50 ms after S1).

**Fix.** (1) Remove `dirty_files` from the versioned `lane` schema and keep a runtime `TREES.dirty = {count, HEAD, hlc}` row, written only by settles, `links sync`, the git `post-commit`/`post-checkout` blocks and the `Write|Edit` evidence hook (S3); headers print `dirty 3 · 4 min ago` or nothing, and a read never scans a worktree (`links check --dirty` recomputes on request). (2) Capture the leased task's `files_owned` globs into the lease row at `claim` and at any `set files_owned` under the lease (the writer already has that view), so the do-not-touch list is O(live leases) from `LEASES` with no other-branch overlay. *Format*: the `lane` schema change, the `TREES` field and the lease-row field are frozen items; no owner decision.

### S6 — major — git work on read paths is not bounded by the counted budget

**Where.** [40] §4.3 step 2 (gate G4: E6 over W, "at most `files.budget.window` (default 2,000) commits"), §4.8 read bounds (no E6), §7.1 (HEAD-tree lookup "~0.1–0.5 ms", est.), Review log M12 ("a bounded E6 history search still runs" on reads); [50] §5.10 `fs` units ("4 per uncached ancestry pair"; nothing for tree objects or E6), §2.6 `staleness()` ("a query records no fact"), §4.3 C6 `pack_measurements` with `staleness()`; [AR] §2.14 and §3.5 `stale` row ("never on the pack/brief/ready path … print the cached verdict or `unverified`").

**Problem.** [50]'s budgets are "counted, not timed", but the git reader's work on a read — HEAD-tree lookups, uncached ancestry, and E6's per-commit tree diffs — is either uncharged or charged 4 units (≈ 0.1–0.3 ms by the 400-units ≈ 10–30 ms calibration) for a pair that costs 1–5 ms. [50] also makes every `pack` compute `staleness()` for each pinned measurement, which [AR] forbids on the pack path, and a read may not record the answer.

**Scenario.** A `pack` in the trunk tree renders one lane-observed link whose file the lane moved (G4 applies): E6 walks up to 2,000 commits × 0.1–1 ms of tree diffing → **up to 0.2–2 s**, bounded only by the 2 s wall deadline, with a non-deterministic cut — and repeated by every process until a settle caches `GITFACTS`. Separately, a lane pack with 10 current pins recomputes 10 uncached ancestry pairs: **10–50 ms per pack**, every pack, until someone runs `moirai check`.

**Fix.** (1) Charge git work to `fs` units: 1 per object decoded + 1 per 4 KiB inflated, E6 per commit by its objects; recalibrate the 400-unit default at M7 (item "calibration of the query work unit"). (2) On read paths, use `GITFACTS`/`ANCESTRY` caches plus at most one uncached ancestry pair and at most 32 E6 commits per command; beyond that the link or pin is `unverified (git: moirai links sync | moirai check)`. States become unverified, never wrong, so correctness is unaffected. (3) Amend [50 §2.6, §4.3] to match [AR §2.14]: `staleness()` on pack/brief paths reads the cache only; settles and `check` fill it. (4) Gate: read-path git work ≤ 10 ms p99 on a fixture whose trunk tree is 1,000 commits past the observations.

### S7 — major — edited linked files are re-hashed and re-anchored by every CLI read

**Where.** [40] §4.3 step 1 ("stat quadruple equals `FILEOBS` → ok; else … hash only when anchors need the bytes"), §4.5 (anchor cache keyed by anchor uid, file `oid`, resolver version), §4.8 (MCP LRU ≤ 256 KiB), §4.7 (`Write|Edit` evidence hook off), §7.2 row "+0.2–2 ms … cached per (anchor, `oid`)", §7.4 gate; [AR] §8.1 R4 row; [50] §5.5 `LinkResolve`.

**Problem.** `FILEOBS` (stat quadruple, `last_oid`) is refreshed only at settle points, and reads may not write, so the `(anchor, oid)` cache exists only inside one process. Every CLI read is a new process. While a developer edits the files its task owns — exactly the links C3 renders — every `pack`, `show`, `get` and `q` re-reads and re-hashes each edited linked file and re-runs its anchor cascade.

**Scenario (§2.9).** `pack 89 --lease L-18` with 50 links, 10 files edited since `SessionStart`, 30 anchors of which 5 moved and 1 needs a whole-file fuzzy match: 10 × (150 µs + ~200 µs of hashing) + 25 × 30 µs + 4 × 0.4 ms + 1 × 1.4–8.6 ms ≈ **8–15 ms**, 3–5× the 3 ms gate, every pack of the implementation phase; the gate's fixture ("all links present, warm") never exercises it.

**Fix.** (1) A runtime `ANCHORRES` table — (anchor uid, file `oid`, resolver version) → state, span, score, ≈ 32 B — written by settles and evidence hooks (hooks already append runtime rows; I-F5 is untouched because reads still append nothing). (2) Make the `Write|Edit` evidence hook the spawn-free `mcp_tool` variant of S3 (0.3–0.7 ms per edit): it refreshes the file id, stat quadruple and `last_oid` and resolves the file's anchors into `ANCHORRES`; `config files.hooks.edit-evidence = auto` (on when the server is connected). This also turns edit-then-move (5.0 % of resolvable moves) into exact evidence. (3) Add a gate row: 50-link pack with 10 edited files ≤ 5 ms p50 warm. *Format*: `ANCHORRES` is a new runtime section (R-8 extension) and must be reserved before M0 exits.

### S8 — major — settles write a record per verified link, and `SessionStart` always spends its cap

**Where.** [40] §2.6 `FILEOBS.verified_at` ("hlc of the last settle that saw F present"), §4.3 copy rule (uses `verified_at`), §4.2 `SessionStart` row ("then others until the budget"), §7.2 `SessionStart` row; [AR] §7.5 `SessionStart` (startup/resume/clear/compact), §4.5 step 10 (4,096 ops / 4 MiB tail threshold), §4.7 open cost at a full tail.

**Problem.** Keeping `verified_at` current requires a `FileObs` lazy record for every link a settle sees unchanged. `SessionStart` settles brief-scoped links, then the lane's `files_owned`, "then others until the budget", and it fires on startup, resume, `/clear` and every compaction.

**Scenario.** At the owner's ≈ 2.2k linked files the whole set fits the 150 ms cap (37–147 ms of stats), so each `SessionStart` spends ~150 ms and appends 2.2k × 96–150 B ≈ **0.2–0.33 MB** of `FileObs` records. With 16 sessions and a few compactions each (~50–100 settles/day): **10–33 MB/day** of lazy records, a 4 MiB tail every 12–40 settles, 3–8 extra delta checkpoints per day (5–50 ms each in some agent's call tail, S15), and every process's open replaying a tail that is mostly `FileObs` noise — the 3–5 ms full-tail case instead of the ≤ 1.5 ms gate at 1e5.

**Fix.** (1) One `TreeReg` epoch record per settle `{tree, scope digest, hlc}`; `FILEOBS` rows are written only when a file's quadruple, state or proposals change; `verified_at(F)` = max(row value, newest epoch whose scope covered F and saw it unchanged) — the copy rule keeps its meaning. (2) `SessionStart` scope = brief items + the bound lane's `files_owned`; "others" only when the tree's last full settle is older than `files.settle.others-after` (config, default 24 h) or through `links sync`. (3) Group links by directory and enumerate once when ≥ 4 links share a directory (8–22 µs per entry vs 17–67 µs per stat). Result: < 16 KB of records and 5–30 ms of settle when nothing changed. *Format*: the epoch semantics of `TreeReg`/`TREES` are frozen items.

### S9 — major — the MCP server blocks all subagents behind unsliced work

**Where.** [AR] §4.9 (rollup "in the resident MCP server after serving a request when deltas exceed 25 % of base … 1–3 s at 1e6"), §6.1 MCP row, §2.2 (no threads); [40] §4.2 `links sync` (2 s default); [50] §5.10 (only queries are sliced); [60] §5.4 row "unstamped read tool ≤ 5 ms at 1e5".

**Problem.** One `current_thread` server serves every subagent of a session. Rollup, promotion, delta checkpoints, `links_sync` settles, merges and packs run to completion on that thread; only queries yield between slices.

**Scenario.** At 1e6 a rollup triggered after one agent's request stalls every queued MCP call — and, after S3, every `mcp_tool` hook of the session — for **1–3 s**; cancellations cannot be seen during it. A 16-subagent `pack` burst serialises to 80–480 ms for the last caller. The M10 gate measures an unloaded single read and will not see it.

**Fix.** (1) Run rollup, promotion, delta checkpoints and settles in the server as resumable slices of ≤ 5 ms interleaved with requests, holding only the maintenance byte across slices (writers are unaffected, because maintenance never holds byte 0 except to publish); state lives in the server's memory. The slicing machinery already exists for queries. (2) Cap `links_sync` through MCP at 200 ms per call with a continuation cursor. (3) Merges through S2's pre-lock phase. (4) `config maintenance.rollup = mcp-sliced | explicit-only` (default `mcp-sliced`). (5) Gate: MCP p99 ≤ 25 ms and max ≤ 100 ms under a 16-subagent burst with a rollup pending at 1e6. No threads or timers are added.

### S10 — minor — fixed per-invocation cost is unbudgeted

**Where.** [AR] §2.14 (discovery walk-up, `.git`/`commondir`), §4.1 (`config`), §4.7 open, §8.1 open row, §7.1 header provenance; [60] §5.3 floor "HEAD pread + map of 8 files".

**Scenario (§2.1).** 7–13 file opens (walk-up probes, `.git`, `commondir`, `config`, `HEAD`, 4–8 maps, the log, git `HEAD`/ref/`packed-refs`) ≈ 1.5–3.3 ms, plus the first schema materialisation (est. 20–200 µs) — up to two thirds of the 5 ms engine gate — are in no budget row; the open row (0.3–1 ms at 1e4) is below its own component sum.

**Fix.** Gate the count of file opens per command through the `Vfs` counters (e.g. ≤ 10 on a `main` read, ≤ 14 on a lane read, ≤ 18 with a tree gate), map blob and `hist` files lazily as specified, read `packed-refs` only when the loose ref is absent, and restate the open row as "floor(8 maps) + discovery + tail".

### S11 — minor — incremental image export ignores the measured rename cost

**Where.** [AR] §5b.6 steps 3–5, §5b.9 incremental row, §8.1 row "`image export`, one checkpoint 5–20 ms"; [60] §5.4 row "≤ 20 ms".

**Scenario (§2.8).** `main` + 2 lanes at checkpoint granularity: pack + idx by temp-then-rename, three loose ref updates by `.lock` + rename + read-back, the side ref and the `gitmap` commit ≈ **38–72 ms** with rename p50 3.9–9.5 ms [M]; the flush policy for pack, idx and ref files is unstated.

**Fix.** In moirai-owned bare image repositories, update all refs of one run in a single `packed-refs` transaction (one lock file, one rename; loose refs deleted), keep the `.lock` protocol per ref only for live project repositories, state "flush pack, flush idx, flush `packed-refs`, then the `gitmap` commit", and set the budget to ≤ 50 ms against the measured rename floor (M0 item 8).

### S12 — minor — prompt delta: unbounded scan and a writer-byte append in a blocking hook

**Where.** [AR] §6.3 (`changes --since` from `seq_ring` or `hist`), §7.5 `UserPromptSubmit`; [40] §4.2 last row; [AR] §2.8 (`lazy` cursors).

**Scenario.** A resumed session with a cursor two days old reads ~2k commits (≈ 0.6 MB) to print ≤ 600 characters: 5–20 ms; advancing the cursor appends a lazy record under the writer byte, which during a merge (20–80 ms) or a 16-writer burst (≤ 50 ms) blocks the owner's prompt.

**Fix.** Cap the scan at 2,000 commits and print `N older changes: moirai changes --since S`; take the writer byte with a try-lock and skip the cursor advance when busy (the next prompt re-shows); with S3 keep the cursor in the server's memory and write nothing.

### S13 — minor — flush grouping per verb is unspecified

**Where.** [AR] §7.6 step 3 (`lane open`: "three durable commits, ~6–9 ms"), §5a.7 step 0, §5b.6 step 5.

**Fix.** State "one flushed group per verb": `lane open` writes its `RefUpdate`, binding and lane-node commit in one group (1 flush, ~2 ms instead of 6–9); a sync-first merge writes the sync and merge commits in one group; add "flushes per verb" to the `Vfs`-counted gates.

### S14 — minor — the 50-link gate is at its limit under load

**Where.** [40] §7.4, [AR] §8.1 R4 gates ("≤ 3 ms p50 warm").

**Scenario.** Under the 16-agent load fixture a stat costs up to 67 µs [M]: 50 stats alone = 3.35 ms.

**Fix.** Enumerate once per directory when ≥ 4 rendered links share it (S8's grouping), and state the gate as ≤ 3 ms p50 idle / ≤ 5 ms p50 loaded, per [60 §5.1]'s protocol.

### S15 — minor — maintenance runs in an agent's CLI tail

**Where.** [AR] §4.5 step 10, §5a.3 promotion ("the next maintenance holder").

**Scenario.** The process whose commit crosses the tail threshold performs the delta checkpoint (10–50 ms at 1e5, 20–100 ms at 1e6) or a promotion (20–100 ms) before it exits, so that agent's Bash call takes 150–290 ms instead of 136–189 ms; with S8 this happens several times a day.

**Fix.** Prefer the resident server (sliced, S9): it takes maintenance at 1× the threshold after a request; a CLI process takes it only above 2× (and in quiet mode as specified). Report p99.9 of write verbs.

### S16 — minor — the hook gate is too loose to catch regressions

**Where.** [60] §5.4 M9 row ("hook wall p99 under a 16-agent burst ≤ 1 s incl. spawn").

**Fix.** Per-hook budgets from §2.10: `SessionStart` ≤ 300 ms p50 / 500 ms p99 (command); `SubagentStart` ≤ 150 ms p99 (command) / ≤ 40 ms p99 (`mcp_tool`); `UserPromptSubmit` ≤ 120 ms p99 / ≤ 5 ms p99; stamp ≤ 110 ms p99 / ≤ 2 ms p99.

### S17 — minor — pinned-measurement and rule sections of a lane pack recompute per call

**Where.** [50] §4.3 (`pack_rules_unmerged` = `CALL diff(HEAD...main)`; C6 `staleness()`; C7 `applies(k, glob)` against `files_owned`); [AR] §7.4; [60] §5.4 M9 row ("pack/brief engine time ≤ 8 ms at 1e5", lane not stated).

**Scenario (est.).** A lane pack at 1e5 after S1: core 2–8 ms + overlay ≤ 10 ms + three-dot diff 1–5 ms + C7 glob tests over 2–6k path-scoped notes at three years (1–6 ms) + links 1–3 ms = **5–32 ms**, with no lane budget.

**Fix.** State a lane pack budget (≤ 20 ms at 1e5 including the overlay); cache the `~main` rule set per (lane tip, `main` tip) in the server; index path globs by literal prefix (a `GLOBIDX` beside `PATHIDX`, *format*) so C7 is a range probe; S6's cache-only `staleness()`.

### S18 — minor — BM25 view statistics and schema load are per process

**Where.** [50] §5.5 (view statistics "1–3 ms on a lane at first use, cached per process per view"), §5.3 binder over schema-as-data.

**Fix.** Keep them as specified for the CLI, but add both to the budget rows (a lane `find` pays 1–3 ms per call; the first query of a process pays the schema load), and let the server keep them warm. No design change needed beyond the numbers.

### Out-of-axis observation (for the correctness audit)

[§6.2] treats a lease as dead when its recorded `(pid, process start time)` is gone at read time. A lease taken by `moirai claim` from the CLI records a process that exits within ~150 ms; unless the recorded pid is the long-lived holder (the Claude session or the Workflow runner), every CLI-claimed lease would read as dead on the next `ready`. Which pid is recorded is not specified.

---

## 5. Budgets

Budgets are engine or wall time as stated, warm cache, measured under [60 §5.1]'s protocol; "loaded" means the 16-agent fixture. Rows marked *new* or *changed* are this audit's proposals; the others restate the design's numbers that the audit found sound.

| Metric | Budget (1e4 / 1e5 / 1e6) | Scale | Gate |
|---|---|---|---|
| Agent Bash CLI read, spawn-to-exit incl. Git Bash (reported) | ≈ 136–189 ms p50 at every scale; engine share ≤ 5 ms | 1e4 / 1e5 / 1e6 | M8: direct spawn-to-exit ≤ empty-exe floor + 5 ms (kept); wrapper reported |
| Fixed per-invocation file opens (*new*) | ≤ 10 (`main` read) / ≤ 14 (lane) / ≤ 18 (with tree gate) | all | M1/M8 `Vfs` open counter |
| Open (`HEAD` + maps + discovery + tail) (*changed*) | ≤ 1.5 / 2 / 3.5 ms incl. discovery; full tail included | 1e4 / 1e5 / 1e6 | M1, floor-relative |
| `get #N` | ≤ 5 µs | all | M2 |
| `ready` page with 3 years of markers (*changed*) | ≤ 50 µs / 300 µs / 3 ms | 1e4 / 1e5 / 1e6 | M2; fixture 90k inert + 100 active markers |
| `brief_triage` (*changed*) | ≤ 50 µs, independent of marker history | all | M7 |
| Lane first read incl. overlay, lane synced at every `SubagentStart` for 14 days (*changed fixture*) | ≤ 10 ms; overlay ≤ 1 MB private | all | M1 physical, M3 policy (S1) |
| LQ parse + bind + plan (warm / first query incl. schema) (*changed*) | ≤ 20 µs / ≤ 200 µs | all | M7 |
| Anchored 2–3-hop pattern | ~10 µs / 30 µs / 0.3 ms | 1e4 / 1e5 / 1e6 | M7 |
| Durable commit | flush floor + 0.5 ms p50 (≈ 2.2 ms); exactly 1 flush | all | M1 |
| Flushes per verb (*new*) | 1 per verb; `file mv`/`rm` 2; `lane open` 1 | all | M1/M3/M6 `Vfs` counter |
| Writer-byte hold per verb class incl. merge, sync, `TX`, `rm`, `complete` with settle (*changed*) | p99 ≤ 5 ms, max ≤ 20 ms | all | M1 (plain), M3 (merge/sync), M6 (`complete`), M7 (`TX`) |
| Writer wait, 16 writers + one concurrent merge (*changed fixture*) | p99 ≤ 50 ms | all | M1/M3 |
| Merge of a 2k-op lane (compute, outside the byte) | ≤ 20 / 40 / 80 ms | 1e4 / 1e5 / 1e6 | M3 |
| 50-link pack, all present (*changed*) | ≤ 3 ms p50 idle / ≤ 5 ms loaded | all | M6/M9 |
| 50-link pack with 10 edited files (*new*) | ≤ 5 ms p50 warm | all | M6/M9 |
| File-bearing header (tree, HEAD, dirty) (*new*) | ≤ 0.5 ms; zero worktree scans on a read | all | M8 `Vfs` counter |
| Read-path git work (tree gate + ancestry + E6) (*new*) | ≤ 10 ms p99, counted in `fs` units | all | M6/M7 |
| Lane pack engine time (*new*) | ≤ 12 / 20 / 30 ms incl. overlay and links | 1e4 / 1e5 / 1e6 | M9 |
| `SessionStart` settle log bytes when nothing changed (*new*) | ≤ 16 KB | all | M6/M9 `Vfs` counter |
| `SessionStart` hook end-to-end, command (*new*) | ≤ 300 ms p50 / 500 ms p99 loaded | all | M9 |
| `SubagentStart` hook (*new*) | command ≤ 150 ms p99; `mcp_tool` ≤ 40 ms p99 | all | M9 |
| `UserPromptSubmit` hook (*new*) | command ≤ 120 ms p99; `mcp_tool` ≤ 5 ms p99 | all | M9 |
| `PreToolUse` stamp (*new*) | command ≤ 110 ms p99; `mcp_tool` ≤ 2 ms p99 | all | M9/M10 |
| MCP unstamped read under a 16-subagent burst, rollup pending (*new*) | p50 ≤ 5 ms; p99 ≤ 25 ms; max ≤ 100 ms | all | M10 |
| Incremental checkpoint export, `main` + 2 lanes (*changed*) | ≤ 50 ms | all | M4/M5, after M0 item 8 |
| Full export / full import | ≤ 0.3 / 3 / 25 s; ≤ 1 / 7 / 70 s | 1e4 / 1e5 / 1e6 | M5 |
| Delta checkpoint / promotion (maintenance, outside byte 0) | ≤ 30 / 50 / 100 ms; ≤ 40 / 60 / 100 ms | 1e4 / 1e5 / 1e6 | M1 |
| Rollup in the MCP server (*changed*) | resumable slices ≤ 5 ms; total ≤ 0.03 / 0.3 / 3 s | 1e4 / 1e5 / 1e6 | M10 |
| Prompt delta scan (*new*) | ≤ 2,000 commits, ≤ 5 ms | all | M9 |
| Idle CPU | zero CPU time and zero context switches over 10 min | all | M1/M10 |

---

## 6. Checked and found sound (not re-raised)

- O(1) open with a bounded tail, no O(N) on open, two-slot `HEAD` read by `pread` (§4.2, §4.7; [20] F-B1).
- One data-only flush per durable commit; zero-filled extents; `WRITE_THROUGH` rejected (§2.8; [M, 05 §2.2]).
- Blocking overlapped `LockFileEx` wait instead of backoff (G1), once S2's pre-lock phase keeps holds short.
- CM1's O(1) marker absorption for `ready`/`claim`; G15's per-ref index for a lane's *own* commits; G17's merge-by-reference for *storage*.
- Bitset-anchored planning with exact counts, lower-bound pre-flight, resumable plans, layered-frontier closures ([50 §5.4–§5.7]); parse + bind at µs.
- Spawn-free ancestry and tree reads through the in-process git layer ([60] M4); every BoykoEngine object is packed and a multi-pack-index exists [M-70], so the M4 ≤ 1 / ≤ 5 ms ancestry budgets are plausible.
- Exec-form, fail-open hooks; evidence hooks that drop their record when the byte is busy; no watcher, timer or poller; quiet mode.
- R4's directory re-bind through one `OpenFileById` (E3d) and the stat-only read path for unchanged files; `links check --all` never walks other worktrees.
- Explicit-verb costs (full export/import, backup, `links check --all`, `--deep`) are outside agent hot paths and budgeted.

---

## 7. Consequential edits

- **[AR]**: §4.5 (three-phase write, S2); §5a.3, §5a.10, §8.1, §2.3 (overlay-ops promotion trigger, S1); §5a.7 step 0 (compute outside the byte, one flushed group); §4.4 and §5d.1 (`MARKERS`/`MARKERS_OLD`, S4); §3.2, §7.1, §7.4 (`dirty` runtime, lease-captured `files_owned`, S5); §2.9, §1 row 9, §7.2, §7.5 (`mcp_tool` hooks and stamp, S3); §4.9 and §6.1 (sliced maintenance in the server, S9); §5b.6/§5b.9 (packed-refs transaction, S11); §6.3 (bounded delta, S12); §7.6 step 3 (one group, S13); §8.1 budget rows per §5.
- **[40]**: §2.6 (`TreeReg` epochs, `ANCHORRES`, S7/S8), §2.11 (reservations for both), §4.2 (`SessionStart` scope; `complete` settles before the lock), §4.3/§4.8 (read-path git caps, S6), §4.7 and §6.4 (`mcp_tool` evidence hooks, S3/S7), §7.2/§7.4 (edited-files and loaded gates).
- **[50]**: §2.6 and §4.3 C6 (`staleness()` cache-only on pack/brief paths), §5.10 (`fs` units charge git objects), §5.12 (`brief_triage` row), §5.9 (pre-lock `MATCH` with re-evaluation only when the tip moved).
- **[60]**: §5.2 items 3 (sync fixture) and 7 (`mcp_tool` experiment), §5.4 rows per §5, §2.5 format-freeze list (overlay-ops counter, `MARKERS_OLD`, `TREES.dirty` and epochs, `ANCHORRES`, lease-row globs, `GLOBIDX`, the `lane` schema change).

## Sources

[AR] `docs/ARCHITECTURE-RESEARCH.md`; [40] `docs/research/design/40-file-links-design.md` rev 2; [50] `docs/research/design/50-query-language-design.md` rev 2; [60] `docs/research/design/60-roadmap.md` issue 2; [02], [05], [08], [09], [10], [11], [13] reports under `docs/research/`; [20], [22], [41], [51], [61] critiques under `docs/research/design/`; Claude Code hooks reference, `https://code.claude.com/docs/en/hooks` (fetched 2026-09-26) and MCP page `https://code.claude.com/docs/en/mcp` (fetched 2026-09-26, no tool-use id in request metadata documented); [M-70] probes: `git count-objects -v`, `git ls-files`, `git worktree list`, `git status --porcelain` with `GIT_OPTIONAL_LOCKS=0`, and a Python stat/enumeration timing over BoykoEngine's tracked files, all read-only.
