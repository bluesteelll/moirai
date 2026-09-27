# LQ-Bench fixture store: declarative design

| | |
|---|---|
| **Title** | The seeded LQ-Bench fixture store |
| **Status** | draft, pass 1 pending |
| **Work package** | WP-70 (lane B, role R-BENCH; S6). The generator that builds this store on `moirai-model` is WP-70's second output; it follows this file and never changes it. |
| **Sources** | [50 §7.4] item 1 (what the store must contain); [50 §2.4] (revisions), §2.5 (kinds, edge types, aliases, readings, properties), §2.6 (built-ins, table functions); [50 §3.3] (absent values), §3.4 (counting), §3.5 (order), §3.6 (deleted nodes, N06), §3.7 (walks), §3.8 (derived, runtime and tree-derived state), §3.9 (views), §3.10 (mutations); [50 §4.1] (std named queries); [AR §3.1–§3.6] (header, kinds, edges, invariants, derived state, status machines); [AR §5a.1–§5a.8] (refs, forks, sync, merge, conflicts and violations); [AR §5d.1–§5d.3] (leases, markers, absorbed vectors, node 40); [40 §2.2–§2.9] (file node, uids, anchors, link states), [40 §4.3–§4.5] (file and anchor cascades), [40 §6.5] (R4 built-ins) |
| **Companion files** | [README.md](README.md) (corpus format; cited as README §n); [plan.md](plan.md) (allocation) |

---

## 1. Purpose and principles

This file specifies, declaratively, the ≈ 2,000-node store every LQ-Bench prompt runs against. Batch authors compute
every gold result from it by hand (S6), so it states every fact a task may use and lists the facts it may not use (§16).

1. **Deterministic.** The generator builds the store by replaying the commit schedule of §7 through `moirai-model`, with a
   fixed seed only for filler text. Two runs give the same ids, anchors, fields, edges, statuses, runtime rows and trees.
2. **Hand-computable.** Named referents carry fixed `#N` and handles; filler follows closed-form rules (§12) with the
   totals precomputed (§13); derived state at the fixture clock is tabulated (§11).
3. **Shaped like the owner's work, but synthetic.** A fictional game-engine project ("tessera") with three campaigns, five
   lanes, reviews, rules and file links. No name, path, title or text comes from owner data.
4. **Card-disjoint.** The card's example ids `#51`, `#88`, `#89`, `#93`, `#130` are filler (§4), and `lane/l5np` does not
   exist, so copying a card example never answers a task.
5. **Every construct reachable.** Every node kind, every edge kind and every feature of [50 §7.4] item 1 occurs (§15).

## 2. Global conventions

### 2.1 Clock

| Day | Date (UTC) | Day | Date (UTC) |
|---|---|---|---|
| D0 | 2026-06-01 | D7 | 2026-06-08 |
| D1 | 2026-06-02 | D8 | 2026-06-09 |
| D2 | 2026-06-03 | D9 | 2026-06-10 |
| D3 | 2026-06-04 | D10 | 2026-06-11 |
| D4 | 2026-06-05 | D11 | 2026-06-12 |
| D5 | 2026-06-06 | D12 | 2026-06-13 |
| D6 | 2026-06-07 | D13 | 2026-06-14 |

**NOW = 2026-06-15T09:00:00Z.** Every evaluation at a tip uses NOW as `now()` (README §8). A commit's time (`hlc` wall
part, `created_at`, `updated_at`) is the time §7 lists, in UTC, with seconds zero; its HLC counter is 0.

### 2.2 Actors and roles

| Actor | Role | Acts as |
|---|---|---|
| `orch` | orchestrator | decomposition, filler imports, deletes, merges, syncs, priorities |
| `arch#1` | architect | docs, file captures |
| `crit#1` | architecture-critic | review findings and verdicts |
| `rev#1` | code-reviewer | code-review findings and verdicts |
| `ref#1` | refuter | refutations and status moves of findings |
| `dev#1`, `dev#2`, `dev#3`, `dev#4` | developer | claims and completions |
| `test#1` | tester | measurements, the latency claim |

A node's `created_by` and `created_role` are the actor and role of the commit that created it. `owner` never commits;
`authority = owner` and `owner_quote` are field values.

### 2.3 Schema

The core schema v1: the 13 kinds of [AR §3.2], the 25 edge kinds of [50 §2.5] with their reverse aliases and readings,
and the enum sets of [AR §3.2]. There are no project kinds, project fields or project named queries.

### 2.4 Handles

| Referent | Handle form | Example |
|---|---|---|
| named node | a cluster prefix and a name (§5) | `TC-3` (task #276) |
| filler node | `F<k>`, k the filler ordinal of §12.1 | `F417` |
| node a write creates | `new<n>`, the n-th created node (§2.5) | `new1` = `#2001` |
| commit | `c.<name>` (§7) | `c.sync-net` |
| anchor | `a<n>` (§9.4) | `a7` |
| lease | `LS-<task #N>` (§8.1) | `LS-276` |
| tree | `T-<name>` (§3.4) | `T-main` |
| git commit of the fixture repository | `g0`, `g1`, `g2`, `gA`, `gT`, `gS` (§3.3) | `g2` |

In gold results a handle is written `@<handle>` (README §7.2).

### 2.5 Id allocation

`#N` is allocated store-wide, in creation order, never reused ([AR §3.4] I1). This design fixes every `#N`: the schedule
of §7 creates nodes in increasing `#N` order, and inside one commit the statements run in the order of the ids they
create. Artifacts and the root node are derived-uid nodes ([40 §2.3]); each is created once (§9.1). At NOW `next_id` is
2001, so a write task that creates nodes creates `#2001`, `#2002`, … (`new1`, `new2`, …). Anchors are numbered from the
store's anchor counter in capture order: `a1`…`a16` (§9.4).

### 2.6 Commits, sequence numbers and placeholders

§7 lists all 103 commits in store order with a **design sequence number** `s1`…`s103`. The design numbers assume that
the first commit of the store is `c.import`. The format chapters fix whether `init` writes an earlier commit and how
records other than commits relate to `seq`, so gold results name commits by handle (`@c.sync-net`) and phrasings and
gold queries write `{{seq:c.sync-net}}` and `{{commit:c.sync-net}}` (README §3.2). Relative revisions (`main~5`,
`main@3`, `REF@<datetime>`, ranges) are computed from §7 and §11.6 and are written literally.

### 2.7 Manifest and self-check

The generator writes a manifest next to the store it builds: every handle with its `#N` and uid, every commit handle with
its id and sequence number, every anchor handle with its number, every lease handle with its lease id. Before a run the
generator checks the store against this file:

- every named `#N` and anchor number equals §4, §5 and §9.4;
- every fact of §11 (statuses, derived flags, ready sets, elsewhere sets, rollups, `blockers()` rows) holds per view;
- every link and anchor state of §9 holds on its tree;
- every total of §13 holds.

A mismatch is a finding against this file or against the model, triaged in review; the generator never re-assigns an
id or rewrites a fact to make the check pass. Checking designed facts against the model is not deriving gold results
from it: the golds come from this file (S6).

## 3. Refs and trees

### 3.1 Refs

| Ref | Kind | Forked from (the fork follows the listed commit) | Own commits (§7) | Tip at NOW | Main absorbed up to | State at NOW |
|---|---|---|---|---|---|---|
| `main` | work | — | 73 (§11.6) | `c.filler-d13` | — | — |
| `lane/tools` | work | `main` at `c.lane-tools-open` (D1 09:00) | `c.tools-d1-done`, `c.tools-d2-done`, `c.tools-d3-cancel`, `c.tools-d4-done`, `c.tools-file-mv`, `c.sync-tools` | `c.sync-tools` | `c.filler-d5` (sync D6) | merged into `main` by `c.merge-tools`; the ref is kept |
| `lane/audio` | work | `main` at `c.lane-audio-open` (D2 09:00) | `c.audio-b1-claim`, `c.audio-b1-done`, `c.audio-s2-edit`, `c.audio-b2-done`, `c.sync-audio`, `c.audio-b4-claim`, `c.filler-d12-audio`, `c.audio-measure` | `c.audio-measure` | `c.filler-d8` (sync D9) | one unresolved `TextHunk` (§10) |
| `lane/net` | work | `main` at `c.lane-net-open` (D3 09:00) | `c.net-c1-done`, `c.net-c4-prio`, `c.net-review`, `c.net-refute`, `c.net-c2-done`, `c.net-new-tasks`, `c.net-c3-claim`, `c.sync-net`, `c.filler-d12-net` | `c.filler-d12-net` | `c.filler-d10` (sync D11) | one `FieldEdit`, one `StatusFork`, unresolved (§10) |
| `lane/assets` | work | `main` at `c.lane-assets-open` (D4 09:00) | `c.assets-a3-claim`, `c.assets-a3-done`, `c.assets-a7-replace` | `c.assets-a7-replace` | the fork only (its D12 sync staged) | merge staged (§10) |
| `lane/shaders` | work | `main` at `c.lane-shaders-open` (D8 09:00) | `c.shaders-x7-delete`, `c.shaders-s1` | `c.shaders-s1` | the fork only | — |
| `plan/q3` | plan | `main` at `c.hdoc-edit2` (D10 14:00) | `c.plan-reprio` | `c.plan-reprio` | the fork only | status fields read-only (I33′) |
| `tags/m1` | tag | points at `c.h2-prio1` (created D4 16:30, pinned) | — | `c.h2-prio1` | — | immutable |
| `merge/lane/assets/from/main` | merge | staging ref of the sync step of `merge lane/assets --into main` | `c.staged-assets` | `c.staged-assets` | — | one `DanglingEdge` violation (§10) |

A sync commit (`c.sync-tools`, `c.sync-audio`, `c.sync-net`) and the staged commit `c.staged-assets` have two parents:
the lane's previous tip first, then the main commit named in the column "Main absorbed up to". The merge commit
`c.merge-tools` has two parents: `main`'s previous tip `c.filler-d5` first, then `c.sync-tools`.

### 3.2 Lane nodes

Each lane has a `lane` node on `main`, created by its `c.lane-<x>-open` commit; the branch is forked from `main`'s tip
right after that commit, so every lane contains its own lane node. Fields: `git_branch` `u/<x>`, `base_sha` the 40-hex id
of `g0`, `moirai_branch` `lane/<x>`. No `worktree_path` artifact is registered (open point 2).

| #N | Handle | Title | Status on `main` at NOW (commit) |
|---|---|---|---|
| #500 | `LN-tools` | `tools` | `merged` (`c.lane-tools-merged`) |
| #600 | `LN-audio` | `audio` | `active` |
| #700 | `LN-net` | `net` | `active` |
| #800 | `LN-assets` | `assets` | `merge_pending` (`c.lane-assets-pending`) |
| #1200 | `LN-shaders` | `shaders` | `active` |

`LN-net` `MERGE_AFTER` `LN-assets` (created by `c.lane-assets-open`).

### 3.3 The fixture git repository

One git repository with these commits (author and committer `fixture <fixture@example.invalid>`):

| Commit | Time | Parent | Branch tip of | Change |
|---|---|---|---|---|
| `g0` | D0 07:00 | — | — | the initial tree: every file of §9.1 except `src/render/postfx.rs`, `src/net/legacy_codec.rs`, `docs/net/protocol.md`; plus `src/audio/voice.rs` and `src/audio/graph/.keep`, which no node links |
| `g1` | D5 10:30 | `g0` | — | adds `src/render/postfx.rs` |
| `g2` | D7 09:30 | `g1` | `main` | deletes `src/net/ring.rs` and adds `src/net/ring_a.rs` and `src/net/ring_b.rs`, both with `ring.rs`'s blob id |
| `gA` | D2 10:00 | `g0` | `u/audio` | edits `src/audio/voice.rs` (a file with no node) |
| `gT` | D5 14:30 | `g0` | `u/tools` | renames `tools/build/cache.py` to `tools/build/depcache.py`, unchanged content |
| `gS` | D0 06:00 | — | none | a commit of a scratch repository; **not** in the fixture repository's object store |

`u/net`, `u/assets` and `u/shaders` point at `g0`.

### 3.4 Trees and bindings

| Tree | Git HEAD | Bound to (designated tree of) | Content |
|---|---|---|---|
| `T-main` | `main` at `g2` | `main` | §9.2 |
| `T-audio` | `u/audio` at `gA` | `lane/audio` | `gA`'s checkout, no uncommitted change |
| `T-net` | `u/net` at `g0` | `lane/net` | `g0`'s checkout |
| `T-assets` | `u/assets` at `g0` | `lane/assets` | `g0`'s checkout |
| `T-shaders` | `u/shaders` at `g0` | `lane/shaders` | `g0`'s checkout |
| `T-tools` | `u/tools` at `gT` | `lane/tools` | `gT`'s checkout |

The harness passes the bound tree of the view's branch (README §8). `plan/q3`, `tags/m1` and the staging ref have no tree.

## 4. Id allocation map

| Ids | Count | Created by (§7) | Day | Ref | Content |
|---|---|---|---|---|---|
| #1–#199 | 199 | `c.import` | D0 08:00 | main | filler block FB0 (§12) — contains the card ids #51, #88, #89, #93, #130 |
| #200–#206 | 7 | `c.areas` | D0 08:10 | main | areas (§5.1) |
| #207–#239 | 33 | `c.knowledge` | D0 08:20 | main | rules, decisions, questions, notes (§5.2) |
| #240–#259 | 20 | `c.campaign-a` | D0 09:00 | main | campaign A (§5.3) |
| #260–#272 | 13 | `c.campaign-b` | D0 09:30 | main | campaign B (§5.4) |
| #273–#284 | 12 | `c.campaign-c` | D0 10:00 | main | campaign C (§5.5) |
| #285–#289 | 5 | `c.tools` | D0 10:30 | main | tooling (§5.6) |
| #290–#292 | 3 | `c.history` | D0 11:00 | main | history cluster (§5.7) |
| #293–#299 | 7 | `c.deletes` | D0 11:30 | main | deletion cluster (§5.8) |
| #300–#312 | 13 | `c.files` | D0 12:00 | main | root node and artifacts (§9.1) |
| #313–#499 | 187 | `c.filler-d0` | D0 18:00 | main | filler FB-D0 |
| #500–#506 | 7 | D1 commits | D1 | main | `LN-tools`, round-1 review of plan A (§5.9) |
| #507–#599 | 93 | `c.filler-d1` | D1 18:00 | main | filler FB-D1 |
| #600–#603 | 4 | D2 commits | D2 | main | `LN-audio`, `MA-1`, `VA-3`, `FC-1` |
| #604–#699 | 96 | `c.filler-d2` | D2 18:00 | main | filler FB-D2 |
| #700 | 1 | `c.lane-net-open` | D3 09:00 | main | `LN-net` |
| #701–#799 | 99 | `c.filler-d3` | D3 18:00 | main | filler FB-D3 |
| #800–#804 | 5 | D4 commits | D4 | main | `LN-assets`, round-2 review of plan A |
| #805–#899 | 95 | `c.filler-d4` | D4 18:00 | main | filler FB-D4 |
| #900–#902 | 3 | D5 commits | D5 | main | artifact `FI-postfx`, `TA-11`, `TA-10` |
| #903–#999 | 97 | `c.filler-d5` | D5 18:00 | main | filler FB-D5 |
| #1000–#1099 | 100 | `c.filler-d6` | D6 18:00 | main | filler FB-D6 |
| #1100–#1199 | 100 | `c.filler-d7` | D7 18:00 | main | filler FB-D7 |
| #1200 | 1 | `c.lane-shaders-open` | D8 09:00 | main | `LN-shaders` |
| #1201–#1299 | 99 | `c.filler-d8` | D8 18:00 | main | filler FB-D8 |
| #1300 | 1 | `c.shaders-s1` | D9 10:30 | lane/shaders | `TS-1` (lane-only) |
| #1301–#1304 | 4 | `c.net-review`, `c.net-refute` | D9 | lane/net | `FN-1`…`FN-3`, `FN-R1` (lane-only) |
| #1305–#1399 | 95 | `c.filler-d9` | D9 18:00 | main | filler FB-D9 |
| #1400 | 1 | `c.assets-a7-replace` | D10 12:00 | lane/assets | `TA-7r` (lane-only) |
| #1401–#1402 | 2 | `c.net-new-tasks` | D10 13:00 | lane/net | `TC-9`, `TC-10` (lane-only) |
| #1403–#1499 | 97 | `c.filler-d10` | D10 18:00 | main | filler FB-D10 |
| #1500–#1599 | 100 | `c.filler-d11` | D11 18:00 | main | filler FB-D11 |
| #1600–#1649 | 50 | `c.filler-d12-net` | D12 18:00 | lane/net | filler FB-D12N (lane-only) |
| #1650–#1699 | 50 | `c.filler-d12-audio` | D12 18:30 | lane/audio | filler FB-D12A (lane-only) |
| #1700 | 1 | `c.audio-measure` | D13 09:00 | lane/audio | `MB-1` (lane-only) |
| #1701 | 1 | `c.run-net` | D13 10:00 | main | `RN-1` |
| #1702–#2000 | 299 | `c.filler-d13` | D13 18:00 | main | filler FB-D13 |

Totals: 144 named ids, 1,856 filler ids, 2,000 in all. **Ids that exist only on a lane**: #1300 (lane/shaders), #1301–#1304,
#1401, #1402, #1600–#1649 (lane/net), #1400 (lane/assets), #1650–#1700 (lane/audio): 109 ids. On `main` they yield no row
and notice N06 ([50 §3.6]).

## 5. Named nodes

Common rules for this section:

- **Bodies.** Every task, doc, note, rule, decision, question, finding, verdict and measurement has a body; areas,
  artifacts, lanes and runs have none. An English body is `Synthetic body of <handle>.` followed by the node's extra
  sentence if the tables give one. A Cyrillic body (marked **RU**) is `Синтетический текст <handle>.` followed by the
  Russian sentence given. No body contains `#` except the two listed in §6 under `MENTIONS`.
- **Absent fields.** A kind field not listed is absent. `abstract` is absent on every named node. Header defaults:
  priority P2, criticality `normal`, authority `agent`.
- **Status** is the status on `main` at NOW unless the column says otherwise; §7 has every change, §11 every lane.

### 5.1 Areas and the root node

| #N | Handle | Title | path_globs | Parent | Status |
|---|---|---|---|---|---|
| #200 | `AR-render` | Rendering | `src/render/**` | — | active |
| #201 | `AR-shaders` | Shader compilation | `shaders/**`, `src/render/shader/**` | #200 | active |
| #202 | `AR-audio` | Audio | `src/audio/**` | — | active |
| #203 | `AR-net` | Networking | `src/net/**` | — | active |
| #204 | `AR-assets` | Asset pipeline | `src/assets/**`, `tools/import/**` | — | active |
| #205 | `AR-tools` | Build tooling | `tools/**`, `scripts/**` | — | active |
| #206 | `AR-docs` | Documentation | `docs/**` | — | active |
| #300 | `ROOT` | `root:project` (the root node of root `project`, [40 §2.4]) | — | — | active; `root` = `project`, `path_moves` empty |

### 5.2 Knowledge core (all created by `c.knowledge`, D0 08:20, `orch`)

**Rules.** `applies_to` holds globs and, where given, roles; an empty `applies_to` means every path ([AR §3.2]).

| #N | Handle | Title | Status | Criticality | Authority | Enforcement | applies_to | Extra |
|---|---|---|---|---|---|---|---|---|
| #207 | `RU-cache-backup` | Never delete an asset cache without a verified backup | active | critical | owner | must | empty (all paths) | `owner_quote` "Synthetic owner ruling RU-cache-backup." |
| #208 | `RU-render-io` | Never block the render thread on disk I/O | active | critical | orchestrator | must | `src/render/**` | |
| #209 | `RU-audio-alloc` | Never allocate on the audio callback thread | active | critical | owner | must | `src/audio/**`; roles `developer` | `owner_quote` "Synthetic owner ruling RU-audio-alloc." |
| #210 | `RU-net-tick` | Stamp every snapshot with the server tick | active | high | orchestrator | must | `src/net/**` | |
| #211 | `RU-fixtures` | Generate test fixtures; never copy captured data | active | normal | orchestrator | should | `tests/**`; roles `tester` | |
| #212 | `RU-log-old` | Log with printf-style macros | superseded | normal | agent | should | `src/**` | |
| #213 | `RU-log-new` | Log through the structured tracing facade | active | normal | orchestrator | should | `src/**` | |
| #214 | `RU-tex-full` | Import textures at full resolution | active | high | research | should | `src/assets/**` | |
| #215 | `RU-tex-down` | Downscale textures above 4096 pixels at import | active | high | measured | must | `src/assets/**` | |
| #216 | `RU-pin-deps` | Pin every dependency to an exact patch version | retracted | low | agent | should | `Cargo.toml` | |
| #217 | `RU-shader-validate` | Run the shader validator before every merge | proposed | high | orchestrator | must | `shaders/**` | |
| #218 | `RU-snapshot-version` | Не менять формат снимка без новой версии протокола | active | critical | owner | must | `src/net/**` | **RU** "Любое изменение формата снимка требует новой версии протокола."; `owner_quote` "Synthetic owner ruling RU-snapshot-version." |
| #219 | `RU-handshake` | Retry a failed handshake at most three times | active | normal | orchestrator | should | `src/net/handshake/**` | |

**Decisions.**

| #N | Handle | Title | Status | Authority | Extra |
|---|---|---|---|---|---|
| #220 | `DE-atlas-2k` | Pack sprites into 2048-pixel atlases | accepted | orchestrator | |
| #221 | `DE-atlas-1k` | Pack sprites into 1024-pixel atlases | superseded | orchestrator | |
| #222 | `DE-mixer-graph` | Mix audio as a static node graph | accepted | owner | `owner_quote` "Synthetic owner ruling DE-mixer-graph." |
| #223 | `DE-delta` | Delta-compress snapshots against the last acknowledged tick | proposed | agent | |
| #224 | `DE-multicast` | Use UDP multicast for LAN discovery | rejected | agent | |

**Questions.**

| #N | Handle | Title | Status | q_kind | asked_of | Extra |
|---|---|---|---|---|---|---|
| #225 | `QU-tga` | Does the importer keep legacy TGA support? | open | scope | owner | |
| #226 | `QU-latency` | What is the mixer latency budget? | answered | values | owner | `answer` "10 ms at 48 kHz" |
| #227 | `QU-rollback` | Should snapshots support rollback? | dropped (`c.c8-cancel`) | scope | owner | |
| #272 | `QB-bus` | Which bus layout ships first? | open | unclear | orchestrator | created by `c.campaign-b` |

**Notes.**

| #N | Handle | Title | note_kind | Status | Criticality | Authority | applies_to | Extra |
|---|---|---|---|---|---|---|---|---|
| #228 | `NO-staging-ring` | Texture uploads stall when the staging ring is full | hazard | active | high | measured | `src/render/**` | "Uploads apply backpressure when the ring fills." |
| #229 | `NO-voice-prealloc` | Preallocate voice buffers when the mixer starts | lesson | active | normal | agent | `src/audio/**` | |
| #230 | `NO-assets-checkpoint` | Asset pipeline checkpoint after review round 1 | checkpoint | active | normal | orchestrator | `src/assets/**` | |
| #231 | `NO-importer-summary` | Summary of the importer review | summary | active | normal | agent | `src/assets/**` | |
| #232 | `NO-old-importer` | Old importer notes | note | **deleted** on main by `c.x6-delete` (no replacement) | normal | agent | `tools/import/**` | |
| #233 | `NO-migration-guide` | Importer migration guide | note | active; **suspect** | normal | agent | `tools/import/**` | |
| #234 | `NO-packet-loss` | Буфер снимков переполняется при потере пакетов | hazard | active | high | measured | `src/net/**` | **RU** "При потере пакетов буфер снимков растёт без ограничения." |
| #235 | `NO-sprite-padding` | Sprite padding of one pixel is enough | lesson | retracted | normal | agent | `src/assets/**` | |
| #236 | `NO-bus-v1` | Mixer bus naming, first draft | note | superseded | normal | agent | `src/audio/**` | |
| #237 | `NO-bus-v2` | Mixer bus naming | note | active | normal | orchestrator | `src/audio/**` | |
| #238 | `NO-frame-pacing` | Frame pacing notes | note | active | normal | agent | `src/render/**` | "See also #239 for the vsync side." |
| #239 | `NO-vsync` | Vsync interaction notes | note | active | normal | agent | `src/render/**` | **RU** "Вертикальная синхронизация задерживает кадр на один интервал." |

### 5.3 Campaign A: asset pipeline (`c.campaign-a`, D0 09:00, `orch`, unless stated)

| #N | Handle | Title | P | Labels | work_kind | Parent | Status on main at NOW | Other fields |
|---|---|---|---|---|---|---|---|---|
| #240 | `TA-0` | Asset pipeline v2 | 1 | campaign, assets | design | — | open (container) | |
| #241 | `TA-1` | Define the asset manifest schema | 1 | assets | impl | #240 | done (`c.a1-done`) | assignee dev#1; estimate 3 |
| #242 | `TA-2` | Content-addressed asset store | 1 | assets | impl | #240 | in_progress (`c.a2-claim`); lease `LS-242`; gated by #804 | assignee dev#1; estimate 5 |
| #243 | `TA-2a` | Choose the content hash | 2 | assets | research | #240 | done (`c.a2a-done`) | assignee dev#1 |
| #244 | `TA-2b` | Store eviction policy | 2 | assets | impl | #240 | done (`c.a2b-done`) | assignee dev#1 |
| #245 | `TA-3` | Texture importer | 1 | assets, render | impl | #240 | open (done on lane/assets) | assignee dev#1; estimate 8; files_owned `src/assets/texture/**` |
| #246 | `TA-4` | Mesh importer | 2 | assets | impl | #240 | open; gated by #506 (accepted on main, open on lane/assets) | estimate 5; files_owned `src/assets/mesh/**` |
| #247 | `TA-5` | Legacy TGA import path | 3 | assets | impl | #240 | open; **suspect** | |
| #248 | `TA-6` | Import validation | 2 | assets | test | #240 | open (container) | |
| #249 | `TA-6a` | Validate texture sizes | 2 | assets, test | test | #248 | open | |
| #250 | `TA-6b` | Validate mesh bounds | 3 | assets, test | test | #248 | open | |
| #251 | `TA-7` | Old importer shim | 2 | assets | debt | #240 | open (deleted on lane/assets) | |
| #252 | `TA-8` | Importer benchmarks | 2 | assets, perf | measure | #240 | open | estimate 2; extra "Measure throughput and backpressure on the reference asset set." |
| #253 | `TA-9` | Asset pipeline user docs | 3 | assets, docs | doc | #240 | cancelled (`c.a9-cancel`) | |
| #901 | `TA-11` | Handle zero-byte assets | 2 | assets | fix | #240 | open | created by `c.a11-discovered` (D5, dev#1) |
| #902 | `TA-10` | Hash the asset store | 2 | assets | impl | #240 | cancelled, resolution `duplicate` | created and cancelled by `c.dup` (D5) |
| #1400 | `TA-7r` | Importer shim v2 | 2 | assets | debt | #240 | lane/assets only: open | created by `c.assets-a7-replace` |

Plan documents (doc_kind, status `current`; sections have `parent` #254 and `order` 1–5):

| #N | Handle | Title | doc_kind | Order |
|---|---|---|---|---|
| #254 | `DA-plan` | Asset pipeline v2 plan | plan | — |
| #255 | `DA-s1` | Manifest format | section | 1 |
| #256 | `DA-s2` | Content-addressed store | section | 2 |
| #257 | `DA-s3` | Importers | section | 3 |
| #258 | `DA-s4` | Validation | section | 4 |
| #259 | `DA-s5` | Rollout | section | 5 |

### 5.4 Campaign B: audio mixer (`c.campaign-b`, D0 09:30, `orch`)

| #N | Handle | Title | P | Labels | work_kind | Parent | Status on main at NOW | Other fields |
|---|---|---|---|---|---|---|---|---|
| #260 | `TB-0` | Audio mixer rewrite | 1 | campaign, audio | design | — | open (container) | |
| #261 | `TB-1` | Mixer node graph | 0 | audio | impl | #260 | open (done on lane/audio) | assignee dev#2; estimate 8; files_owned `src/audio/graph/**` |
| #262 | `TB-2` | Voice pool | 1 | audio | impl | #260 | open (done on lane/audio) | assignee dev#2; estimate 3; files_owned `src/audio/voice/**` |
| #263 | `TB-3` | Resampler | 1 | audio | impl | #260 | open | |
| #264 | `TB-4` | Mixer latency measurement | 2 | audio, perf | measure | #260 | open (in_progress on lane/audio); lease `LS-264` | assignee test#1; **RU** "Метрика: задержка микшера под нагрузкой." |
| #265 | `TB-5` | Mixer UI panel | 3 | audio, ui | impl | #260 | open | `defer_until` 2026-06-17T00:00:00Z |
| #266 | `TB-6` | Спектральный анализатор | 2 | audio | impl | #260 | open | **RU** "Анализатор показывает спектр каждой шины." |
| #267 | `TB-7` | Legacy mixer removal | 2 | audio | debt | #260 | frozen | status set at creation (`c.campaign-b` creates it open and freezes it) |

| #N | Handle | Title | doc_kind | Order | Extra |
|---|---|---|---|---|---|
| #268 | `DB-plan` | Mixer rewrite plan | plan | — | |
| #269 | `DB-s1` | Graph model | section | 1 | |
| #270 | `DB-s2` | Voice pool | section | 2 | body v1 "Voices come from a fixed pool of 64." (see §10 for the lane and main edits) |
| #271 | `DB-s3` | Latency budget | section | 3 | **RU** "Задержка микшера не должна превышать 10 мс." |

### 5.5 Campaign C: netcode snapshots (`c.campaign-c`, D0 10:00, `orch`)

| #N | Handle | Title | P | Labels | work_kind | Parent | Status on main at NOW | Other fields |
|---|---|---|---|---|---|---|---|---|
| #273 | `TC-0` | Netcode snapshots | 1 | campaign, net | design | — | open (container) | |
| #274 | `TC-1` | Snapshot ring buffer | 1 | net | impl | #273 | open (done on lane/net) | assignee dev#2; estimate 5; files_owned `src/net/ring*.rs` |
| #275 | `TC-2` | Delta encoder | 1 | net | impl | #273 | open (done on lane/net) | assignee dev#2; estimate 8; files_owned `src/net/delta/**` |
| #276 | `TC-3` | Server tick clock | 0 | net | impl | #273 | open (in_progress on lane/net); lease `LS-276` | assignee dev#2; estimate 3; files_owned `src/net/tick.rs` |
| #277 | `TC-4` | Interest management | 0 | net | impl | #273 | open; priority 2 → 0 by `c.main-c4-prio` | on lane/net: `FieldEdit` conflict (§10) |
| #278 | `TC-5` | Packet loss simulator | 2 | net, test | test | #273 | open; expired lease `LS-278` | |
| #279 | `TC-6` | Bandwidth budget | 3 | net | research | #273 | open | |
| #280 | `TC-7` | Snapshot interpolation | 1 | net | impl | #273 | open | estimate 5 |
| #281 | `TC-8` | Rollback netcode | 3 | net | research | #273 | cancelled (`c.c8-cancel`) | |
| #1401 | `TC-9` | Snapshot compression levels | 2 | net | impl | #273 | lane/net only: open | created by `c.net-new-tasks` |
| #1402 | `TC-10` | Jitter buffer | 1 | net | impl | #273 | lane/net only: open | **RU** "Буфер джиттера сглаживает колебания сети." |

| #N | Handle | Title | doc_kind | Order |
|---|---|---|---|---|
| #282 | `DC-plan` | Snapshot plan | plan | — |
| #283 | `DC-s1` | Ring buffer | section | 1 |
| #284 | `DC-s2` | Delta encoding | section | 2 |

### 5.6 Tooling (`c.tools`, D0 10:30, `orch`)

| #N | Handle | Title | P | Labels | work_kind | Parent | Status on main at NOW | Other fields |
|---|---|---|---|---|---|---|---|---|
| #285 | `TD-0` | Build tooling cleanup | 2 | tools | debt | — | open (container) | |
| #286 | `TD-1` | Cache the dependency graph | 2 | tools | impl | #285 | done (on lane/tools by `c.tools-d1-done`; merged) | assignee dev#3 |
| #287 | `TD-2` | Parallel shader compile | 1 | tools, render | impl | #285 | done (`c.tools-d2-done`; merged) | assignee dev#3 |
| #288 | `TD-3` | Drop the Python build helper | 3 | tools | debt | #285 | cancelled (`c.tools-d3-cancel`; merged) | assignee dev#3 |
| #289 | `TD-4` | Incremental asset cook | 2 | tools, assets | impl | #285 | done (`c.tools-d4-done`; merged) | assignee dev#3 |

### 5.7 History cluster (`c.history`, D0 11:00, `orch`)

| #N | Handle | Title at NOW | P at NOW | Labels at NOW | Status at NOW | History (all on main) |
|---|---|---|---|---|---|---|
| #290 | `TH-1` | Frame pacing investigation (vsync) | 1 | render | done; `reopen_count` 1 | created "Frame pacing investigation", P2, open, assignee dev#4, work_kind research; `c.h1-prio` P2→P1; `c.h1-claim` open→in_progress; `c.h1-done` →done; `c.h1-title` title changed; `c.h1-reopen` done→open (reason "stutter returned"); `c.h1-restart` open→in_progress; `c.h1-done2` →done |
| #291 | `TH-2` | Shader hot reload | 1 | render, tools, hot | open | created P2, labels render, tools, work_kind impl; `c.h2-prio1` P2→P3; `c.h2-label` labels += hot; `c.h2-prio2` P3→P1 |
| #292 | `DH-overview` | Render architecture overview (doc_kind report, current) | — | — | current | body v1 "Synthetic body of DH-overview. Version one."; `c.hdoc-edit1` body v2 "… Version two."; `c.hdoc-edit2` body v3 "… Version three." |

### 5.8 Deletion cluster (`c.deletes`, D0 11:30, `orch`)

| #N | Handle | Title | P | Labels | Status at NOW on main | Deletion |
|---|---|---|---|---|---|---|
| #293 | `TX-1` | Reader registry v1 | 2 | render | **deleted** | `c.x1-delete`: `DELETE #293 REPLACED BY #294 REASON 'superseded by v2'` |
| #294 | `TX-2` | Reader registry v2 | 2 | render | open | the replacement |
| #295 | `TX-3` | Config hot reload | 2 | tools | open | its blocker edge from #293 is re-pointed to #294 |
| #296 | `TX-4` | Watchdog timer | 2 | net | **deleted** | `c.x4-delete`: `DELETE #296 REASON 'obsolete'` (no replacement) |
| #297 | `TX-5` | Crash report upload | 1 | net | open; `has_dangling` | its blocker edge from #296 is flagged; **RU** "Отчёт об аварии отправляется после перезапуска." |
| #298 | `NX-registry` | Registry design notes (note, note_kind note, applies_to `src/render/registry/**`) | — | — | active; **suspect** | extra "This replaces the reader described in #293." |
| #299 | `TX-7` | Legacy shader cache | 3 | render | open (deleted on lane/shaders) | `c.shaders-x7-delete`: `DELETE #299 REASON 'folded into shader cache v2'` on lane/shaders |

All tasks of this table have work_kind impl except #299 (debt). Tombstones: `#293` (`deleted_by` orch, `deleted_reason`
"superseded by v2", `replaced_by` #294), `#296` ("obsolete", no replacement), `#232` on main ("obsolete notes", no
replacement), `#299` on lane/shaders ("folded into shader cache v2", no replacement), `#251` on lane/assets (`replaced_by`
#1400, reason "rewritten as v2").

### 5.9 Lanes, reviews, measurements, verdicts, runs (D1–D13)

| #N | Handle | Kind | Created by (§7) | Ref | Title | Fields | Status at NOW (main unless stated) |
|---|---|---|---|---|---|---|---|
| #501 | `FA-C1` | finding | `c.review-a-r1` | main | Hash collisions are not detected | local_id C1, round 1, severity blocker, f_kind correctness | confirmed (`c.refute-a-r1`) |
| #502 | `FA-C2` | finding | `c.review-a-r1` | main | Importers decode every texture twice | C2, round 1, important, perf | refuted (`c.refute-a-r1`) |
| #503 | `FA-C3` | finding | `c.review-a-r1` | main | Rollout section is vague | C3, round 1, optional, style | withdrawn (`c.withdraw-a-r1`) |
| #504 | `FA-C4` | finding | `c.review-a-r1` | main | Manifest lacks a version field | C4, round 1, important, plan | confirmed (`c.refute-a-r1`) → fixed (`c.a-r1-fix`) |
| #505 | `FA-R1` | finding | `c.refute-a-r1` | main | Each texture is decoded once; the second pass hits the cache | R1, round 1, optional, perf | confirmed (created confirmed) |
| #506 | `VA-1` | verdict | `c.verdict-a-r1` | main | Architecture review round 1 | role architecture-critic, round 1, outcome fail_fixable, return_to architect | accepted (`c.a-r1-fix`); open on lane/assets |
| #601 | `MA-1` | measurement | `c.measure-a` | main | Import time on the reference set | metric import_ms, value 812.0, unit ms, target 1000.0, env host bench1 / profile release / load quiet, measured_on `a1b2c3…` (synthetic 40 hex) | current |
| #602 | `VA-3` | verdict | `c.verdict-a1` | main | Code review of the manifest schema | role code-reviewer, round 1, outcome pass, return_to none | accepted (created accepted) |
| #603 | `FC-1` | finding | `c.review-c-r1` | main | Ring buffer size is unbounded | C1, round 1, important, correctness; **RU** "Размер кольцевого буфера не ограничен." | confirmed (`c.main-fc1-confirm`); on lane/net `StatusFork` (§10) |
| #801 | `FA-C5` | finding | `c.review-a-r2` | main | Eviction ignores pinned assets | C5, round 2, important, correctness | confirmed (`c.refute-a-r2`) |
| #802 | `FA-C6` | finding | `c.review-a-r2` | main | Importer allocations are not pooled | C6, round 2, optional, perf | open |
| #803 | `FA-C7` | finding | `c.review-a-r2` | main | Manifest paths allow directory traversal | C7, round 2, blocker, security; **RU** "Путь в манифесте может выйти за пределы каталога." | open |
| #804 | `VA-2` | verdict | `c.verdict-a-r2` | main | Architecture review round 2 | role architecture-critic, round 2, outcome fail_fixable, return_to developer | open |
| #1300 | `TS-1` | task | `c.shaders-s1` | lane/shaders | Shader cache v2 | P2, labels render, work_kind impl, no parent | lane/shaders only: open |
| #1301 | `FN-1` | finding | `c.net-review` | lane/net | Delta encoder copies every snapshot | W1, round 1, important, perf | lane/net only: open |
| #1302 | `FN-2` | finding | `c.net-review` | lane/net | Ring buffer field names are inconsistent | W2, round 1, optional, style; **RU** "Имена полей кольцевого буфера не совпадают." | lane/net only: open |
| #1303 | `FN-3` | finding | `c.net-review` | lane/net | Tick counter wraps after 2^32 frames | W3, round 1, blocker, correctness | lane/net only: open |
| #1304 | `FN-R1` | finding | `c.net-refute` | lane/net | Ring buffer size is bounded by the tick window | R1, round 1, optional, correctness | lane/net only: confirmed (created confirmed) |
| #1700 | `MB-1` | measurement | `c.audio-measure` | lane/audio | Mixer delay on the reference scene | metric mixer_latency_ms, value 8.4, unit ms, target 10.0, env host bench1 / profile release / load quiet | lane/audio only: current |
| #1701 | `RN-1` | run | `c.run-net` | main | net bench r12 | wf_id r12, started D13 09:30, ended D13 09:55, expected_artifacts empty | green |

Every finding's `failure_scenario` is "Synthetic failure scenario of <handle>.". `created_role` follows §2.2: FA-C1…C7 and
FC-1 architecture-critic; FA-R1 and FN-R1 refuter; FN-1…3 code-reviewer.

### 5.10 Real-session support clusters

The real-session stratum (README §9) maps the owner's referents onto these clusters; synthetic tasks may use them too.

| Question family ([50 §7.4] item 2) | Cluster | Designed answer on `main` at NOW |
|---|---|---|
| review-loop termination | plan #254 and its findings (§5.9) | round 1: raised 4, confirmed 1 (#501), refuted 1 (#502), blocking 1 (#501); round 2: raised 3, confirmed 1 (#801), refuted 0, blocking 1 (#801) — `loop plan=254` |
| refuted share | findings by `created_role` | architecture-critic: round 1 raised 5 (#501–#504, #603), refuted 1, 20.0 %; round 2 raised 3, refuted 0, 0.0 % — `refuted_share role=architecture-critic` |
| what is blocking the merge of lane L | `lane/assets` | the staged sync holds one `DanglingEdge` on `merge/lane/assets/from/main` (§10); lane/assets has no conflicted node |
| rules about files owned by lane L | `lane/net`: the live lease `LS-276` on #276 (files_owned `src/net/tick.rs`) | active rules that apply to `src/net/tick.rs`: #207, #210, #213, #218 (critical: #207, #218) |
| history questions | #290, #291, #292 (§5.7) | §7 lists every change |

## 6. Edges

Every edge below exists on `main` from its commit on, unless the ref column says otherwise. "Stored" gives the stored
direction ([50 §2.5]); reverse aliases read the other way.

| Type | Edges (src → dst) | Created by | Ref / notes |
|---|---|---|---|
| `CHILD_OF` | #201→#200 | `c.areas` | |
| | #241, #242, #243, #244, #245, #246, #247, #248, #251, #252, #253 → #240; #249, #250 → #248; #255–#259 → #254 | `c.campaign-a` | #251→#240 ends with #251's deletion on lane/assets |
| | #261–#267 → #260; #269–#271 → #268 | `c.campaign-b` | |
| | #274–#281 → #273; #283, #284 → #282 | `c.campaign-c` | on plan/q3 #278 moves under #285 (`c.plan-reprio`) |
| | #286–#289 → #285 | `c.tools` | |
| | #901 → #240 | `c.a11-discovered` | |
| | #902 → #240 | `c.dup` | |
| | #1400 → #240 | `c.assets-a7-replace` | lane/assets only |
| | #1401, #1402 → #273 | `c.net-new-tasks` | lane/net only |
| `BLOCKS` | #241→#243, #243→#245, #241→#245 (shortcut), #243→#244, #242→#246, #245→#246, #245→#248 (exogenous for #249, #250), #225→#247 (question → task) | `c.campaign-a` | |
| | #261→#263, #262→#264, #272→#266 (question → task) | `c.campaign-b` | |
| | #274→#275, #274→#280 (shortcut), #275→#280, #276→#280, #276→#277 | `c.campaign-c` | |
| | #293→#295 | `c.deletes` | re-pointed to **#294→#295** by `c.x1-delete` |
| | #296→#297 | `c.deletes` | **flagged** by `c.x4-delete` (source deleted, no replacement) |
| | #252→#251 | `c.gate-a7` | main only; the source of the staged `DanglingEdge` |
| | #1401→#1402 | `c.net-new-tasks` | lane/net only |
| | #279→#278 | `c.plan-reprio` | plan/q3 only |
| `GATES` | #506→#246 | `c.verdict-a-r1` | #506 accepted by `c.a-r1-fix` on main; still open on lane/assets |
| | #804→#242 | `c.verdict-a-r2` | #804 open, fail_fixable |
| `ANSWERS` | #222→#226 | `c.knowledge` | |
| `SCOPED_TO` | #219→#203, #228→#200, #229→#202, #234→#203, #237→#202 | `c.knowledge` | |
| | #601→#204 | `c.measure-a` | |
| `DUPLICATE_OF` | #902→#242 | `c.dup` | |
| `DEPENDS_ON` | #256→#255, #257→#256, #258→#257, #259→#258, #259→#255 (shortcut) | `c.campaign-a` | |
| | #271→#269 | `c.campaign-b` | |
| | #284→#283 | `c.campaign-c` | |
| `SUPERSEDES` | #213→#212, #220→#221, #237→#236 | `c.knowledge` | |
| `DERIVED_FROM` | #231→#230 | `c.knowledge` | |
| | #298→#293 | `c.deletes` | into a tombstone after `c.x1-delete` → #298 suspect |
| | #506→#501 | `c.verdict-a-r1` | |
| | #804→#801 | `c.verdict-a-r2` | |
| `CITES` | #233→#232 (pinned at `c.knowledge`) | `c.knowledge` | into a tombstone after `c.x6-delete` → #233 suspect |
| `IMPLEMENTS` | #245→#220 | `c.campaign-a` | |
| | #261→#222 | `c.campaign-b` | |
| | #275→#223 | `c.campaign-c` | |
| `REFUTES` | #505→#502 | `c.refute-a-r1` | |
| | #1304→#603 | `c.net-refute` | lane/net only |
| `CONFIRMS` | #505→#501 | `c.refute-a-r1` | |
| | #601→#215 | `c.measure-a` | |
| `VERIFIES` | #601→#241 | `c.measure-a` | |
| | #602→#241 | `c.verdict-a1` | |
| | #1700→#261 | `c.audio-measure` | lane/audio only |
| `ADDRESSES` | #242→#504 | `c.a-r1-fix` | |
| `ABOUT` | #501→#256; #502→#257; #502→#258; #503→#259; #504→#255 | `c.review-a-r1` | #502 is about **two** sections |
| | #603→#283 | `c.review-c-r1` | |
| | #801→#256; #802→#257; #803→#258; #803→#259 | `c.review-a-r2` | #803 is about **two** sections |
| | #1301→#284; #1302→#283; #1303→#283; #1303→#284 | `c.net-review` | lane/net only; #1303 about two sections |
| | filler finding F<k> → filler doc F<k+1> | the filler commit that creates both | §12.2 |
| `DISCOVERED_FROM` | #901→#245 | `c.a11-discovered` | |
| `PRODUCED` | #1701→#307 | `c.run-net` | |
| `CONSUMED` | #1701→#305 | `c.run-net` | |
| `RUNS_IN` | #1701→#700 | `c.run-net` | |
| `MERGE_AFTER` | #700→#800 | `c.lane-assets-open` | |
| `CONTRADICTS` | #215→#214 (symmetric) | `c.knowledge` | |
| `MENTIONS` | #238→#239 (from #238's body) | `c.knowledge` | |
| | #298→#293 (from #298's body) | `c.deletes` | into a tombstone after `c.x1-delete` |
| `RELATES` | #238→#239 (symmetric) | `c.knowledge` | |
| `AT` | a1–a15 (§9.4) | `c.files` | |
| | a16: #292→#900 | `c.postfx-link` | |

No other edge exists. In particular, no filler node has a `BLOCKS`, `CHILD_OF` or `AT` edge, and no named node has an
edge to or from a filler node.

## 7. Commit schedule

All 103 commits in store order. "Design seq" is §2.6's reading aid. Times are UTC on the day given. Every commit's
message is its handle without the `c.` prefix.

| Seq | Handle | Time | Ref | Actor | Effect |
|---|---|---|---|---|---|
| s1 | `c.import` | D0 08:00 | main | orch | creates filler FB0 #1–#199 |
| s2 | `c.areas` | D0 08:10 | main | orch | creates #200–#206 |
| s3 | `c.knowledge` | D0 08:20 | main | orch | creates #207–#239 and their edges (§6) |
| s4 | `c.campaign-a` | D0 09:00 | main | orch | creates #240–#259 and their edges |
| s5 | `c.campaign-b` | D0 09:30 | main | orch | creates #260–#272 (#267 frozen) and their edges |
| s6 | `c.campaign-c` | D0 10:00 | main | orch | creates #273–#284 and their edges |
| s7 | `c.tools` | D0 10:30 | main | orch | creates #285–#289 |
| s8 | `c.history` | D0 11:00 | main | orch | creates #290–#292 |
| s9 | `c.deletes` | D0 11:30 | main | orch | creates #293–#299 and their edges |
| s10 | `c.files` | D0 12:00 | main | arch#1 | captures from `T-main` at `g0`: #300 (root), #301–#312, anchors a1–a15 (§9) |
| s11 | `c.filler-d0` | D0 18:00 | main | orch | filler FB-D0 |
| s12 | `c.lane-tools-open` | D1 09:00 | main | orch | creates #500; then `lane/tools` forks |
| s13 | `c.review-a-r1` | D1 14:00 | main | crit#1 | creates #501–#504 with `ABOUT` edges |
| s14 | `c.refute-a-r1` | D1 16:00 | main | ref#1 | creates #505 (`REFUTES` #502, `CONFIRMS` #501); #501 open→confirmed, #502 open→refuted, #504 open→confirmed |
| s15 | `c.withdraw-a-r1` | D1 17:00 | main | crit#1 | #503 open→withdrawn |
| s16 | `c.verdict-a-r1` | D1 17:30 | main | crit#1 | creates #506 (`GATES` #246, `DERIVED_FROM` #501) |
| s17 | `c.filler-d1` | D1 18:00 | main | orch | filler FB-D1 |
| s18 | `c.lane-audio-open` | D2 09:00 | main | orch | creates #600; then `lane/audio` forks |
| s19 | `c.a1-claim` | D2 10:00 | main | dev#1 | #241 open→in_progress (lease taken, later released) |
| s20 | `c.a1-done` | D2 11:00 | main | dev#1 | #241 in_progress→done |
| s21 | `c.measure-a` | D2 11:30 | main | test#1 | creates #601 with its edges |
| s22 | `c.verdict-a1` | D2 11:45 | main | rev#1 | creates #602 (`VERIFIES` #241) |
| s23 | `c.h1-prio` | D2 12:00 | main | orch | #290 priority 2→1 |
| s24 | `c.review-c-r1` | D2 16:00 | main | crit#1 | creates #603 (`ABOUT` #283) |
| s25 | `c.filler-d2` | D2 18:00 | main | orch | filler FB-D2 |
| s26 | `c.lane-net-open` | D3 09:00 | main | orch | creates #700; then `lane/net` forks |
| s27 | `c.a2-claim` | D3 10:00 | main | dev#1 | #242 open→in_progress; lease `LS-242` |
| s28 | `c.a2a-done` | D3 11:00 | main | dev#1 | #243 open→in_progress→done (one commit) |
| s29 | `c.h1-claim` | D3 12:00 | main | dev#4 | #290 open→in_progress |
| s30 | `c.hdoc-edit1` | D3 13:00 | main | arch#1 | #292 body v1→v2 |
| s31 | `c.tools-d1-done` | D3 14:00 | lane/tools | dev#3 | #286 open→in_progress→done |
| s32 | `c.audio-b1-claim` | D3 15:00 | lane/audio | dev#2 | #261 open→in_progress |
| s33 | `c.filler-d3` | D3 18:00 | main | orch | filler FB-D3 |
| s34 | `c.lane-assets-open` | D4 09:00 | main | orch | creates #800 and `MERGE_AFTER` #700→#800; then `lane/assets` forks |
| s35 | `c.review-a-r2` | D4 10:00 | main | crit#1 | creates #801–#803 with `ABOUT` edges |
| s36 | `c.refute-a-r2` | D4 11:00 | main | ref#1 | #801 open→confirmed |
| s37 | `c.a-r1-fix` | D4 11:30 | main | orch | #504 confirmed→fixed; `ADDRESSES` #242→#504; #506 open→accepted |
| s38 | `c.verdict-a-r2` | D4 12:00 | main | crit#1 | creates #804 (`GATES` #242, `DERIVED_FROM` #801) |
| s39 | `c.c8-cancel` | D4 13:00 | main | orch | #281 open→cancelled; #227 open→dropped |
| s40 | `c.tools-d2-done` | D4 14:00 | lane/tools | dev#3 | #287 open→in_progress→done |
| s41 | `c.tools-d3-cancel` | D4 15:00 | lane/tools | orch | #288 open→cancelled |
| s42 | `c.h2-prio1` | D4 16:00 | main | orch | #291 priority 2→3; then tag `tags/m1` (D4 16:30, pinned) |
| s43 | `c.filler-d4` | D4 18:00 | main | orch | filler FB-D4 |
| s44 | `c.a9-cancel` | D5 09:00 | main | orch | #253 open→cancelled |
| s45 | `c.h1-done` | D5 10:00 | main | dev#4 | #290 in_progress→done |
| s46 | `c.postfx-link` | D5 11:00 | main | arch#1 | captures from `T-main` at `g1`: #900 and anchor a16 (#292→#900) |
| s47 | `c.tools-d4-done` | D5 12:00 | lane/tools | dev#3 | #289 open→in_progress→done |
| s48 | `c.a11-discovered` | D5 13:00 | main | dev#1 | creates #901 (`CHILD_OF` #240, `DISCOVERED_FROM` #245) |
| s49 | `c.dup` | D5 14:00 | main | orch | creates #902 (`CHILD_OF` #240, `DUPLICATE_OF` #242), then #902 open→cancelled (resolution duplicate) |
| s50 | `c.tools-file-mv` | D5 15:00 | lane/tools | dev#3 | `moirai file mv tools/build/cache.py tools/build/depcache.py` in `T-tools`: #310's observation becomes path `tools/build/depcache.py`, `observed_git` `gT`, `relink` `explicit/intent`, `aliases` += `tools/build/cache.py` |
| s51 | `c.filler-d5` | D5 18:00 | main | orch | filler FB-D5 |
| s52 | `c.sync-tools` | D6 09:59 | lane/tools | orch | sync of `main` (up to s51) into `lane/tools`; no conflict |
| s53 | `c.merge-tools` | D6 10:00 | main | orch | merge of `lane/tools` into `main`: #286, #287, #289 done, #288 cancelled and #310's observation land on `main` |
| s54 | `c.lane-tools-merged` | D6 10:05 | main | orch | #500 status →merged |
| s55 | `c.x1-delete` | D6 11:00 | main | orch | `DELETE #293 REPLACED BY #294` (§5.8) |
| s56 | `c.h1-title` | D6 12:00 | main | dev#4 | #290 title → "Frame pacing investigation (vsync)" |
| s57 | `c.filler-d6` | D6 18:00 | main | orch | filler FB-D6 |
| s58 | `c.x4-delete` | D7 09:00 | main | orch | `DELETE #296` (flags #296→#297) |
| s59 | `c.h1-reopen` | D7 10:00 | main | orch | `REOPEN #290 REASON 'stutter returned'`: done→open, `reopen_count` 0→1 |
| s60 | `c.audio-b1-done` | D7 11:00 | lane/audio | dev#2 | #261 in_progress→done |
| s61 | `c.audio-s2-edit` | D7 12:00 | lane/audio | arch#1 | #270 body → "Voices come from a fixed pool of 128." |
| s62 | `c.filler-d7` | D7 18:00 | main | orch | filler FB-D7 |
| s63 | `c.lane-shaders-open` | D8 09:00 | main | orch | creates #1200; then `lane/shaders` forks |
| s64 | `c.x6-delete` | D8 10:00 | main | orch | `DELETE #232 REASON 'obsolete notes'` |
| s65 | `c.h1-restart` | D8 11:00 | main | dev#4 | #290 open→in_progress |
| s66 | `c.audio-b2-done` | D8 12:00 | lane/audio | dev#2 | #262 open→in_progress→done |
| s67 | `c.main-s2-edit` | D8 13:00 | main | arch#1 | #270 body → "Voices come from a growable pool that starts at 64." |
| s68 | `c.h2-label` | D8 14:00 | main | orch | #291 labels += hot |
| s69 | `c.assets-a3-claim` | D8 15:00 | lane/assets | dev#1 | #245 open→in_progress (lease taken; released by s80) |
| s70 | `c.a2b-done` | D8 16:00 | main | dev#1 | #244 open→in_progress→done |
| s71 | `c.filler-d8` | D8 18:00 | main | orch | filler FB-D8 |
| s72 | `c.sync-audio` | D9 09:00 | lane/audio | orch | sync of `main` (up to s71) into `lane/audio`: `TextHunk` conflict value on `#270.body` (§10) |
| s73 | `c.shaders-x7-delete` | D9 10:00 | lane/shaders | orch | `DELETE #299` |
| s74 | `c.shaders-s1` | D9 10:30 | lane/shaders | dev#4 | creates #1300 |
| s75 | `c.h1-done2` | D9 11:00 | main | dev#4 | #290 in_progress→done |
| s76 | `c.tga-rm` | D9 12:00 | main | orch | `moirai file rm tools/import/tga.rs` in `T-main`: #306 status present→removed, reason "TGA support dropped" (#247 becomes suspect) |
| s77 | `c.net-c1-done` | D9 13:00 | lane/net | dev#2 | #274 open→in_progress→done |
| s78 | `c.net-c4-prio` | D9 14:00 | lane/net | orch | #277 priority 2→3 |
| s79 | `c.net-review` | D9 15:00 | lane/net | rev#1 | creates #1301–#1303 with `ABOUT` edges |
| s80 | `c.assets-a3-done` | D9 16:00 | lane/assets | dev#1 | #245 in_progress→done |
| s81 | `c.net-refute` | D9 17:00 | lane/net | ref#1 | creates #1304 (`REFUTES` #603); #603 open→refuted |
| s82 | `c.filler-d9` | D9 18:00 | main | orch | filler FB-D9 |
| s83 | `c.net-c2-done` | D10 09:00 | lane/net | dev#2 | #275 open→in_progress→done |
| s84 | `c.main-c4-prio` | D10 10:00 | main | orch | #277 priority 2→0 |
| s85 | `c.hdoc-edit2` | D10 11:00 | main | arch#1 | #292 body v2→v3; `plan/q3` forks after it (D10 14:00) |
| s86 | `c.assets-a7-replace` | D10 12:00 | lane/assets | orch | creates #1400 (`CHILD_OF` #240); `DELETE #251 REPLACED BY #1400 REASON 'rewritten as v2'` |
| s87 | `c.net-new-tasks` | D10 13:00 | lane/net | orch | creates #1401, #1402 and `BLOCKS` #1401→#1402 |
| s88 | `c.plan-reprio` | D10 15:00 | plan/q3 | orch | `MOVE #278 UNDER #285`; `BLOCKS` #279→#278; #246 priority 2→1 |
| s89 | `c.main-fc1-confirm` | D10 16:00 | main | ref#1 | #603 open→confirmed |
| s90 | `c.filler-d10` | D10 18:00 | main | orch | filler FB-D10 |
| s91 | `c.net-c3-claim` | D11 09:00 | lane/net | dev#2 | #276 open→in_progress; lease `LS-276` |
| s92 | `c.sync-net` | D11 10:00 | lane/net | orch | sync of `main` (up to s90) into `lane/net`: `FieldEdit` on `#277.priority`, `StatusFork` on `#603.status` (§10) |
| s93 | `c.gate-a7` | D11 11:00 | main | orch | `BLOCKS` #252→#251 |
| s94 | `c.filler-d11` | D11 18:00 | main | orch | filler FB-D11 |
| s95 | `c.staged-assets` | D12 10:00 | merge/lane/assets/from/main | orch | step 0 of `merge lane/assets --into main`: the sync of `main` (up to s94) into `lane/assets` stages with one `DanglingEdge` (§10); `main` and `lane/assets` do not move |
| s96 | `c.lane-assets-pending` | D12 10:05 | main | orch | #800 status →merge_pending |
| s97 | `c.h2-prio2` | D12 11:00 | main | orch | #291 priority 3→1 |
| s98 | `c.audio-b4-claim` | D12 12:00 | lane/audio | test#1 | #264 open→in_progress; lease `LS-264` |
| s99 | `c.filler-d12-net` | D12 18:00 | lane/net | orch | filler FB-D12N |
| s100 | `c.filler-d12-audio` | D12 18:30 | lane/audio | orch | filler FB-D12A |
| s101 | `c.audio-measure` | D13 09:00 | lane/audio | test#1 | creates #1700 (`VERIFIES` #261) |
| s102 | `c.run-net` | D13 10:00 | main | orch | creates #1701 with `RUNS_IN`, `PRODUCED`, `CONSUMED` |
| s103 | `c.filler-d13` | D13 18:00 | main | orch | filler FB-D13 |

## 8. Runtime state at NOW

### 8.1 Leases

| Handle | Node | Holder (role) | Branch | Claimed | Expires | State at NOW |
|---|---|---|---|---|---|---|
| `LS-242` | #242 | dev#1 (developer) | main | D3 10:00 with `c.a2-claim` | 2026-06-15T12:00:00Z | live |
| `LS-264` | #264 | test#1 (tester) | lane/audio | D12 12:00 with `c.audio-b4-claim` | 2026-06-15T18:00:00Z | live |
| `LS-276` | #276 | dev#2 (developer) | lane/net | D11 09:00 with `c.net-c3-claim` | 2026-06-15T17:00:00Z | live |
| `LS-278` | #278 | dev#3 (developer) | main | D10 17:00, claim without start (no commit) | 2026-06-12T12:00:00Z | **expired**, holder gone, not reclaimed |

Every other claim of §7 released its lease at the completing commit. Holders of live leases are alive (their anchors
resolve in the model). The benchmark caller `bench` holds no lease, so a live lease on a task excludes it from `ready`.

### 8.2 Markers

Markers are the cache of I26′ ([AR §3.4]); §11 states the resulting `settled_elsewhere` and `deleted_elsewhere` sets per
view, which is what tasks use. The raw rows of `markers()` are not designed (§16).

### 8.3 R4 runtime rows

`FILEOBS`, `PENDING`, `ANCHORRES` and `GITFACTS` rows exist only where §9.2 lists them as evidence. No settle ran after
D11 except where §9.2 says so.

## 9. File links

### 9.1 Artifacts (created by `c.files` from `T-main` at `g0` unless stated; root `project`)

| #N | Handle | `path` on main at NOW | Registered path (`origin_path`) | artifact_kind | Status on main | Observation |
|---|---|---|---|---|---|---|
| #300 | `ROOT` | — | — | — (area) | active | root node, created immediately before #301 |
| #301 | `FI-upload` | `src/render/upload.rs` | same | source | present | `g0` |
| #302 | `FI-frame` | `src/render/frame.rs` | same | source | present | `g0` |
| #303 | `FI-mixer` | `src/audio/mixer.rs` | same | source | present | `g0` |
| #304 | `FI-assets-plan` | `docs/plan/assets.md` | same | doc | present | `g0` |
| #305 | `FI-ring` | `src/net/ring.rs` | same | source | present | `g0` |
| #306 | `FI-tga` | `tools/import/tga.rs` | same | source | **removed** (`c.tga-rm`, reason "TGA support dropped") | `g0` |
| #307 | `FI-perf` | `docs/perf/results.md` | same | doc | present | `g0` |
| #308 | `FI-manifest` | `src/assets/manifest.rs` | same | source | present | `g0` |
| #309 | `FI-quarantine` | `tests/quarantine.rs` | same | source | present | `g0` |
| #310 | `FI-cache-py` | `tools/build/depcache.py`; `aliases` `tools/build/cache.py` | `tools/build/cache.py` | source | present | `gT` (after `c.tools-file-mv`, merged by `c.merge-tools`) |
| #311 | `FI-legacy-codec` | `src/net/legacy_codec.rs` | same | source | present | `gS` (a commit absent from the fixture repository) |
| #312 | `FI-protocol` | `docs/net/protocol.md` | same | doc | **planned** (linked with `--planned`) | none |
| #900 | `FI-postfx` | `src/render/postfx.rs` | same | source | present | `g1` (created by `c.postfx-link`) |

On lanes, each artifact has the path and status of the view: on `lane/tools` #310 moved at `c.tools-file-mv`; on every
lane that has not received `c.merge-tools` (lane/assets) #310 is at `tools/build/cache.py`; #306 is `removed` only on views
containing `c.tga-rm` (main, lane/net, plan/q3); #900 exists on views containing `c.postfx-link` (every view except
lane/assets and tags/m1).

### 9.2 Tree `T-main` at NOW, and the evidence each state rests on

`T-main`'s HEAD is `g2`. The working tree differs from `g2` only as listed. Contents are synthetic text; "unrelated" means
sketch containment below 0.05 in both directions against the last observed content; "similar" means symmetric similarity
between 0.78 and 0.84 and containment above 0.8 (HOLE(lqb-r14-margins)).

| Artifact | On disk in `T-main` | Evidence (runtime rows the generator writes) | File-level state |
|---|---|---|---|
| #301 | unchanged | `FILEOBS` stat quadruple current | `ok` |
| #302 | present, same file id; edited: the line of a2's quote moved 40 lines down (still unique); a3's quoted line deleted | `FILEOBS` stat current after the edit's settle | `ok` |
| #303 | `src/audio/mixer.rs` renamed (uncommitted) to `src/audio/graph/mixer.rs`, same file id, content unchanged | `FILEOBS(#303, T-main)` holds the file id; no settle since | `moved-auto` (E3, file id; not yet recorded) |
| #304 | renamed (uncommitted) to `docs/plan/assets-v2.md`, same file id, then edited in place (similar) | `FILEOBS` holds the file id | `moved-needs-confirm` (E3 moved and edited in place: strong) |
| #305 | `src/net/ring.rs` absent; `src/net/ring_a.rs` and `src/net/ring_b.rs` from `g2`, both with #305's blob | no `FILEOBS` file id match (checkout recreated the files) | `ambiguous` (E6: one commit adds two paths with the deleted path's blob) |
| #306 | absent | status `removed` | `deleted` |
| #307 | present, same path, new file id, unrelated content (≥ 5 lines) | `FILEOBS(#307, T-main)` state `replaced` from a settle at D11 16:00; the stat quadruple unchanged since | `replaced` |
| #308 | present, same file id, edited: `struct Manifest`'s body changed (a10's span edited); a copied block duplicates a11's quoted line with the same prefix, suffix and window | `FILEOBS` stat current | `ok` |
| #309 | deleted from the working tree (still in `g2`); no file with its content or basename anywhere | `FILEOBS` holds the old id | `missing` |
| #310 | `tools/build/cache.py` present (`g2` has it); `tools/build/depcache.py` absent | — | `pending` (tree gate G3: an alias is in τ(`g2`)) |
| #311 | absent; the path never occurs in the repository's history | — | `unverified` (commit not in this repository) |
| #312 | absent | status `planned` | `planned` |
| #900 | present, committed in `g1`, unchanged | `FILEOBS` stat current | `ok` |

**Tree `T-audio`** (HEAD `gA`, which lacks `g1`): `src/render/postfx.rs` is absent and no alias exists, so on
`lane/audio` #900 is **`absent-in-tree`** with detail `diverged` (`gA` is not an ancestor of `g1`). No other link state is
designed on `T-audio`, and none on `T-net`, `T-assets`, `T-shaders` or `T-tools` (§16).

### 9.3 File-level states on `main` at NOW (`link_state(f)`, `f.state`)

| State | Artifact | | State | Artifact |
|---|---|---|---|---|
| ok | #301, #302, #308, #900 | | replaced | #307 |
| moved-auto | #303 | | missing | #309 |
| moved-needs-confirm | #304 | | pending | #310 |
| ambiguous | #305 | | unverified | #311 |
| deleted | #306 | | planned | #312 |

`absent-in-tree` occurs on `lane/audio` (#900). Together the fixture shows all twelve primary and tree-relative states of
[40 §2.9].

### 9.4 Anchors (`AT` edges) and link states on `main` at NOW

Each row is one `AT` edge with one anchor ([50 §3.4] rule 1: one binding per anchor). `a.anchor` prints the handle.

| Anchor | Source | Target | `a.kind` | `a.mode` | `a.watch` | `a.scope` / quote | Anchor state (`a.state`) | `link_state(a)` |
|---|---|---|---|---|---|---|---|---|
| a1 | #228 | #301 | symbol | live | header | `rust:fn upload_texture` | fresh | ok |
| a2 | #290 | #302 | quote | live | span | quote `let budget = frame_budget_ms();` | moved | ok |
| a3 | #290 | #302 | quote | live | span | quote `vsync_wait(swapchain);` | orphaned | stale-anchor |
| a4 | #261 | #303 | symbol | live | header | `rust:impl Mixer/fn process` | fresh | moved-auto |
| a5 | #229 | #303 | quote | live | span | quote `voices.reserve(MAX_VOICES);` | fresh | moved-auto |
| a6 | #254 | #304 | heading | live | header | `md:Rollout` | not resolved | moved-needs-confirm |
| a7 | #274 | #305 | symbol | live | header | `rust:struct SnapshotRing` | not resolved | ambiguous |
| a8 | #247 | #306 | file | live | header | — | not resolved | deleted |
| a9 | #252 | #307 | file | live | header | — | not resolved | replaced |
| a10 | #241 | #308 | symbol | live | span | `rust:struct Manifest` | edited | stale-anchor |
| a11 | #214 | #308 | quote | live | span | quote `max_texture_size: u32,` | ambiguous | stale-anchor |
| a12 | #211 | #309 | file | live | header | — | not resolved | missing |
| a13 | #286 | #310 | file | live | header | — | not resolved | pending |
| a14 | #276 | #311 | file | live | header | — | not resolved | unverified |
| a15 | #282 | #312 | file | live | header | — | not resolved | planned |
| a16 | #292 | #900 | symbol | live | header | `rust:fn apply_bloom` | fresh | ok (on lane/audio: absent-in-tree) |

**Node-level `link_state(n)` on main** (the most severe over its anchors, [40 §2.9] order): #228 ok; #290 stale-anchor;
#261 moved-auto; #229 moved-auto; #254 moved-needs-confirm; #274 ambiguous; #247 deleted; #252 replaced; #241 stale-anchor;
#214 stale-anchor; #211 missing; #286 pending; #276 unverified; #282 planned; #292 ok. Every other node: absent.

**`std.links_broken` on main** (anchors whose `link_state(a) <> 'ok'`): a3–a15, 13 rows. With `scope=240`: a8, a9, a10
(3 rows). With `scope=273`: a7, a14 (2 rows). With `scope=285`: a13 (1 row).

**`file()` on main:** `file('src/audio/mixer.rs')` = #303; `file('tools/build/cache.py')` = #310 through an alias
(notice N11 names `tools/build/depcache.py`); `file('tools/build/depcache.py')` = #310; `file('src/audio/graph/mixer.rs')`
= absent (the move is not recorded); `file('src/net/ring_a.rs')` = absent.

**`suspect` from R4:** #247 (its `AT` target #306 is `removed`) on every view containing `c.tga-rm`.

## 10. Merge state at NOW

**Conflict values** (`conflicts()` on the lane; `conflicted` nodes leave `ready` and `unblocked` there):

| View | Key | Node | Class | Base | Ours (the lane) | Theirs (`main`) | Commit |
|---|---|---|---|---|---|---|---|
| lane/audio | `#270.body` | #270 | TextHunk | "Voices come from a fixed pool of 64." | "Voices come from a fixed pool of 128." | "Voices come from a growable pool that starts at 64." | `c.sync-audio` |
| lane/net | `#277.priority` | #277 | FieldEdit | 2 | 3 | 0 | `c.sync-net` |
| lane/net | `#603.status` | #603 | StatusFork | open | refuted | confirmed | `c.sync-net` |

`main`, lane/assets, lane/shaders, lane/tools and plan/q3 have no conflict value.

**Staged violation** (`USE merge/lane/assets/from/main CALL violations()`): one row, key `edge:#252:blocks:#251`, class
`DanglingEdge`, detail "main added #252 BLOCKS #251; lane/assets deleted #251 (replaced_by #1400)", suggested resolution
`REPOINT #1400`. The staging ref is read-only except for `RESOLVE`.

**Completed merge:** `c.merge-tools` (lane/tools into main, clean after `c.sync-tools`).

## 11. Derived state at NOW

### 11.1 `main`

Named tasks on `main` (the 44 live named tasks; filler tasks have no edges, so every open filler task is `unblocked` and
`ready`, and every filler task has `open_blockers` 0).

| #N | Status | P | open_blockers | unblocked | blocked | ready | is_blocker | elsewhere | Other |
|---|---|---|---|---|---|---|---|---|---|
| #240 | open | 1 | 0 | no (container) | no | no | no | — | children 13, done 5, ready_to_close no |
| #241 | done | 1 | 0 | no | no | no | no | — | link_state stale-anchor |
| #242 | in_progress | 1 | §16 | no | §16 | no | yes | — | lease LS-242; gated by #804 (open) |
| #243 | done | 2 | 0 | no | no | no | no | — | |
| #244 | done | 2 | 0 | no | no | no | no | — | |
| #245 | open | 1 | 0 (#241, #243 done) | yes | no | no | yes | settled on lane/assets | |
| #246 | open | 2 | 2 (#242, #245) | no | yes | no | no | — | gate #506 accepted |
| #247 | open | 3 | 1 (#225) | no | yes | no | no | — | suspect |
| #248 | open | 2 | 1 (#245, exogenous) | no (container) | yes | no | no | — | children 2, done 0 |
| #249 | open | 2 | 0 | no | yes (inherited) | no | no | — | |
| #250 | open | 3 | 0 | no | yes (inherited) | no | no | — | |
| #251 | open | 2 | 1 (#252) | no | yes | no | no | deleted on lane/assets | |
| #252 | open | 2 | 0 | yes | no | **yes** | yes | — | |
| #253 | cancelled | 3 | 0 | no | no | no | no | — | |
| #901 | open | 2 | 0 | yes | no | **yes** | no | — | |
| #902 | cancelled | 2 | 0 | no | no | no | no | — | resolution duplicate |
| #260 | open | 1 | 0 | no (container) | no | no | no | — | children 7, done 0 |
| #261 | open | 0 | 0 | yes | no | no | yes | settled on lane/audio | |
| #262 | open | 1 | 0 | yes | no | no | yes | settled on lane/audio | |
| #263 | open | 1 | 1 (#261) | no | yes | no | no | — | |
| #264 | open | 2 | 1 (#262) | no | yes | no | no | — | lease LS-264 (branch lane/audio) |
| #265 | open | 3 | 0 | no (deferred) | no | no | no | — | defer_until 2026-06-17 |
| #266 | open | 2 | 1 (#272) | no | yes | no | no | — | |
| #267 | frozen | 2 | 0 | no | no | no | no | — | |
| #273 | open | 1 | 0 | no (container) | no | no | no | — | children 8, done 1 |
| #274 | open | 1 | 0 | yes | no | no | yes | settled on lane/net | |
| #275 | open | 1 | 1 (#274) | no | yes | no | yes | settled on lane/net | |
| #276 | open | 0 | 0 | yes | no | no (leased) | yes | — | lease LS-276 (branch lane/net) |
| #277 | open | 0 | 1 (#276) | no | yes | no | no | — | |
| #278 | open | 2 | 0 | yes | no | **yes** | no | — | lease LS-278 expired |
| #279 | open | 3 | 0 | yes | no | **yes** | no | — | |
| #280 | open | 1 | 3 (#274, #275, #276) | no | yes | no | no | — | |
| #281 | cancelled | 3 | 0 | no | no | no | no | — | |
| #285 | open | 2 | 0 | no (container) | no | no | no | — | children 4, done 4, ready_to_close yes |
| #286 | done | 2 | 0 | no | no | no | no | — | |
| #287 | done | 1 | 0 | no | no | no | no | — | |
| #288 | cancelled | 3 | 0 | no | no | no | no | — | |
| #289 | done | 2 | 0 | no | no | no | no | — | |
| #290 | done | 1 | 0 | no | no | no | no | — | reopen_count 1 |
| #291 | open | 1 | 0 | yes | no | **yes** | no | — | |
| #294 | open | 2 | 0 | yes | no | **yes** | yes | — | |
| #295 | open | 2 | 1 (#294) | no | yes | no | no | — | |
| #297 | open | 1 | 1 (flagged, #296 deleted) | no | yes | no | no | — | has_dangling |
| #299 | open | 3 | 0 | yes | no | no | no | deleted on lane/shaders | |

Summary sets on `main` (named nodes only):

| Set | Members |
|---|---|
| `ready` | #252, #278, #279, #291, #294, #901 (6); with filler 534 |
| `unblocked` | #245, #252, #261, #262, #274, #276, #278, #279, #291, #294, #299, #901 (12); with filler 540 |
| `blocked` | #246, #247, #248, #249, #250, #251, #263, #264, #266, #275, #277, #280, #295, #297 (14; #242 excluded, §16) |
| `is_blocker` (tasks) | #242, #245, #252, #261, #262, #274, #275, #276, #294 (9) |
| `std.blocking` (is_blocker and not settled elsewhere) | #242, #252, #276, #294 (4) |
| `settled_elsewhere` | #245, #261, #262, #274, #275 |
| `deleted_elsewhere` | #251, #299 |
| `suspect` | #233, #247, #298 |
| `has_dangling` | #297 |
| `conflicted` | none |
| `answered` (questions) | #226 |
| `claimed` (live leases) | #242, #264, #276 |
| deleted (tombstones, `:DELETED`) | #232, #293, #296 |

### 11.2 `lane/audio`

View: `main` up to `c.filler-d8` plus the lane's own commits. Differences from §11.1 for named nodes:

- Statuses: #261 done, #262 done, #264 in_progress (lease LS-264), #290 in_progress, #291 priority 3, #277 priority 2,
  #244 done, #270 **conflicted**. #306 is `present` (the file rm is later), so #247 is not suspect; #233 and #298 are.
- Present: #601–#603, #801–#804 and #900 (created on `main` before `c.filler-d8`). Absent: the edge #252→#251, #1300–#1304,
  #1400–#1402. Lane-only here: #1650–#1700.
- `ready` (named): #252, #263, #278, #279, #291, #294, #901 (7).
- `settled_elsewhere`: #245, #274, #275, #290. `deleted_elsewhere`: #251, #299.
- Rollups: #260 children 7, done 2; #240 children 13, done 5; #273 children 8, done 1.

### 11.3 `lane/net`

View: `main` up to `c.filler-d10` plus the lane's own commits.

- Statuses: #274 done, #275 done, #276 in_progress (lease LS-276), #277 **conflicted** (priority `FieldEdit`), #603
  **conflicted** (`StatusFork`), #290 done, #291 priority 3; lane-only #1301–#1304, #1401, #1402, #1600–#1649.
- #280 has open_blockers 1 (#276) and is blocked; #1402 is blocked by #1401; #251 has no `BLOCKS` in-edge here.
- `ready` (named): #252, #278, #279, #291, #294, #901, #1401 (7).
- `settled_elsewhere`: #245, #261, #262. `deleted_elsewhere`: #251, #299.
- `suspect`: #233, #247, #298.
- Rollups: #273 children 10 (#274–#281, #1401, #1402), done 3 (#274, #275, #281).

### 11.4 `lane/assets`

View: `main` up to `c.lane-assets-open` plus the lane's own commits.

- Statuses: #241 done, #242 in_progress (lease LS-242), #243 done, #244 **open**, #245 done, #251 **deleted** (replaced
  by #1400), #253 open, #281 open, #286–#289 open, #290 in_progress, #291 priority 2 without `hot`, #293 open, #296 open
  (so #297 is blocked by an open #296, not flagged), #232 live; #506 open (gating #246); #801–#804, #900–#902 absent;
  #1400 present (lane-only).
- `ready` (named): #249, #250, #252, #278, #279, #291, #294, #1400 (8). #246 is blocked by #242 (in progress); #244 is
  unblocked but settled elsewhere.
- `settled_elsewhere`: #244, #253, #261, #262, #274, #275, #281, #286, #287, #288, #289, #290.
  `deleted_elsewhere`: #232, #293, #296, #299.
- `suspect`: none.
- Rollups: #240 children 11 (#241, #242, #243, #244, #245, #246, #247, #248, #252, #253, #1400), done 3 (#241, #243,
  #245); #248 children 2, done 0.

`lane/shaders`: #299 deleted (tombstone), #1300 open (lane-only); `settled_elsewhere` #244, #245, #261, #262, #274, #275,
#290; `deleted_elsewhere` #232, #251. `plan/q3`: as `main` at `c.hdoc-edit2` plus §7 s88 (#278 under #285, blocked by
#279; #246 priority 1). No `ready` set is designed for `lane/shaders`, `lane/tools` or `plan/q3` (§16).

### 11.5 `blockers()` facts on `main`

| Call | Rows (blocker, depth, via, reason, flagged, elsewhere) |
|---|---|
| `blockers(#280)` | (#274, 1, -, direct, false, lane/net); (#275, 1, -, direct, false, lane/net); (#276, 1, -, direct, false, -) |
| `blockers(#277)` | (#276, 1, -, direct, false, -) |
| `blockers(#275)` | (#274, 1, -, direct, false, lane/net) |
| `blockers(#263)` | (#261, 1, -, direct, false, lane/audio) |
| `blockers(#264)` | (#262, 1, -, direct, false, lane/audio) |
| `blockers(#266)` | (#272, 1, -, direct, false, -) — a question |
| `blockers(#247)` | (#225, 1, -, direct, false, -) — a question |
| `blockers(#249)`, `blockers(#250)` | (#245, 1, #248, inherited, false, lane/assets) |
| `blockers(#248)` | (#245, 1, -, direct, false, lane/assets) |
| `blockers(#246)` | (#242, 1, -, direct, false, -); (#245, 1, -, direct, false, lane/assets) |
| `blockers(#245)` | no rows (#241 and #243 are done) |
| `blockers(#249, transitive: true)` | (#245, 1, #248, inherited, false, lane/assets) |
| `blockers(#251)` | (#252, 1, -, direct, false, -) |
| `blockers(#295)` | (#294, 1, -, direct, false, -) |
| `blockers(#297)` | (#296, 1, -, direct, true, -) — #296 is a tombstone |
| `blockers(#1402)` on lane/net | (#1401, 1, -, direct, false, -) |
| `blockers(#252)`, `blockers(#279)`, `blockers(#291)` | no rows |

### 11.6 `main`'s first-parent chain and ref positions

`main~n` (= `main@n`, because every move of `main` was one commit) for n = 0…72:

| n | Commit | n | Commit | n | Commit | n | Commit |
|---|---|---|---|---|---|---|---|
| 0 | `c.filler-d13` | 19 | `c.lane-shaders-open` | 38 | `c.a-r1-fix` | 57 | `c.verdict-a-r1` |
| 1 | `c.run-net` | 20 | `c.filler-d7` | 39 | `c.refute-a-r2` | 58 | `c.withdraw-a-r1` |
| 2 | `c.h2-prio2` | 21 | `c.h1-reopen` | 40 | `c.review-a-r2` | 59 | `c.refute-a-r1` |
| 3 | `c.lane-assets-pending` | 22 | `c.x4-delete` | 41 | `c.lane-assets-open` | 60 | `c.review-a-r1` |
| 4 | `c.filler-d11` | 23 | `c.filler-d6` | 42 | `c.filler-d3` | 61 | `c.lane-tools-open` |
| 5 | `c.gate-a7` | 24 | `c.h1-title` | 43 | `c.hdoc-edit1` | 62 | `c.filler-d0` |
| 6 | `c.filler-d10` | 25 | `c.x1-delete` | 44 | `c.h1-claim` | 63 | `c.files` |
| 7 | `c.main-fc1-confirm` | 26 | `c.lane-tools-merged` | 45 | `c.a2a-done` | 64 | `c.deletes` |
| 8 | `c.hdoc-edit2` | 27 | `c.merge-tools` (^2 = `c.sync-tools`) | 46 | `c.a2-claim` | 65 | `c.history` |
| 9 | `c.main-c4-prio` | 28 | `c.filler-d5` | 47 | `c.lane-net-open` | 66 | `c.tools` |
| 10 | `c.filler-d9` | 29 | `c.dup` | 48 | `c.filler-d2` | 67 | `c.campaign-c` |
| 11 | `c.tga-rm` | 30 | `c.a11-discovered` | 49 | `c.review-c-r1` | 68 | `c.campaign-b` |
| 12 | `c.h1-done2` | 31 | `c.postfx-link` | 50 | `c.h1-prio` | 69 | `c.campaign-a` |
| 13 | `c.filler-d8` | 32 | `c.h1-done` | 51 | `c.verdict-a1` | 70 | `c.knowledge` |
| 14 | `c.a2b-done` | 33 | `c.a9-cancel` | 52 | `c.measure-a` | 71 | `c.areas` |
| 15 | `c.h2-label` | 34 | `c.filler-d4` | 53 | `c.a1-done` | 72 | `c.import` |
| 16 | `c.main-s2-edit` | 35 | `c.h2-prio1` (= `tags/m1`) | 54 | `c.a1-claim` | | |
| 17 | `c.h1-restart` | 36 | `c.c8-cancel` | 55 | `c.lane-audio-open` | | |
| 18 | `c.x6-delete` | 37 | `c.verdict-a-r2` | 56 | `c.filler-d1` | | |

`ref_seq` on `main` is 73 − n. Ref positions (`refs()`, ahead and behind by absorbed-vector arithmetic, [AR §5a.2]):

| Ref | Own commits (`ref_seq` of tip) | absorbed main at | behind main | ahead of main |
|---|---|---|---|---|
| lane/tools | 6 | `c.filler-d5` (ref_seq 45) | 28 | 0 (merged) |
| lane/audio | 8 | `c.filler-d8` (60) | 13 | 8 |
| lane/net | 9 | `c.filler-d10` (67) | 6 | 9 |
| lane/assets | 3 | `c.lane-assets-open` (32) | 41 | 3 |
| lane/shaders | 2 | `c.lane-shaders-open` (54) | 19 | 2 |
| plan/q3 | 1 | `c.hdoc-edit2` (65) | 8 | 1 |

`log(main..lane/net)` lists lane/net's 9 own commits; `log(lane/net..main)` lists `c.gate-a7`, `c.filler-d11`,
`c.lane-assets-pending`, `c.h2-prio2`, `c.run-net`, `c.filler-d13` (6).

**Clean nodes for `rev`, `updated` and `updated_at` questions** (every op touching them, edge ops included, is listed and
the last one is a field, status or body op): #290 (last `c.h1-done2`), #291 (`c.h2-prio2`), #292 (`c.hdoc-edit2`), #277 on
main (`c.main-c4-prio`), #603 on main (`c.main-fc1-confirm`), #270 on main (`c.main-s2-edit`), #244 (`c.a2b-done`), #253
(`c.a9-cancel`), #281 (`c.c8-cancel`), #265 and #279 (creation only). Every filler node is clean (created, never changed).

## 12. Filler

### 12.1 Ordinal and blocks

Filler ids are numbered by the **filler ordinal** k, 0-based, over filler ids in ascending `#N`: `k = #N − offset`.

| Block | Ids | k range | offset | Commit | Ref |
|---|---|---|---|---|---|
| FB0 | #1–#199 | 0–198 | 1 | `c.import` | main |
| FB-D0 | #313–#499 | 199–385 | 114 | `c.filler-d0` | main |
| FB-D1 | #507–#599 | 386–478 | 121 | `c.filler-d1` | main |
| FB-D2 | #604–#699 | 479–574 | 125 | `c.filler-d2` | main |
| FB-D3 | #701–#799 | 575–673 | 126 | `c.filler-d3` | main |
| FB-D4 | #805–#899 | 674–768 | 131 | `c.filler-d4` | main |
| FB-D5 | #903–#999 | 769–865 | 134 | `c.filler-d5` | main |
| FB-D6 | #1000–#1099 | 866–965 | 134 | `c.filler-d6` | main |
| FB-D7 | #1100–#1199 | 966–1065 | 134 | `c.filler-d7` | main |
| FB-D8 | #1201–#1299 | 1066–1164 | 135 | `c.filler-d8` | main |
| FB-D9 | #1305–#1399 | 1165–1259 | 140 | `c.filler-d9` | main |
| FB-D10 | #1403–#1499 | 1260–1356 | 143 | `c.filler-d10` | main |
| FB-D11 | #1500–#1599 | 1357–1456 | 143 | `c.filler-d11` | main |
| FB-D12N | #1600–#1649 | 1457–1506 | 143 | `c.filler-d12-net` | lane/net |
| FB-D12A | #1650–#1699 | 1507–1556 | 143 | `c.filler-d12-audio` | lane/audio |
| FB-D13 | #1702–#2000 | 1557–1855 | 145 | `c.filler-d13` | main |

### 12.2 Attributes by rule

Let r = k mod 10 and j = k div 10. W is the English list and WR the Russian list of §12.3; `a = k mod 20`,
`b = (k div 20) mod 20`, `c = (k div 400) mod 20`.

| r | Kind | Title | Status | Other fields |
|---|---|---|---|---|
| 0 | task | `Backlog item <k 4 digits>: <W[a]> <W[b]>` | done | P4, labels backlog, work_kind fix |
| 1 | task | same form | open | P3, labels backlog, work_kind impl, estimate (j mod 5) + 1 |
| 2 | task | same form | open | P4, labels backlog, work_kind doc, estimate (j mod 5) + 1 |
| 3 | task | same form | open | P3, labels backlog and chore, work_kind debt |
| 4 | task | same form | cancelled | P4, labels backlog, work_kind research |
| 5 | note | `Archive note <k>: <W[a]> <W[b]>` | active | note_kind note, applies_to `archive/**` |
| 6 | note | same form | active | note_kind lesson, applies_to `archive/**` |
| 7 | finding | `Archive finding <k>: <W[a]> <W[b]>` | open if j mod 3 = 0, confirmed if 1, refuted if 2 | local_id `L<k>`, round (j mod 3) + 1, severity optional, f_kind style, failure_scenario "Synthetic failure scenario of F<k>."; `ABOUT` filler doc F<k+1> when F<k+1> is created by the same commit, else no edge |
| 8 | doc | `Archive report <k>: <W[a]> <W[b]>` | current | doc_kind report, no parent |
| 9 | measurement | `Archive metric <k>: <W[a]> <W[b]>` | current | metric filler_ms, value (k mod 97) + 0.5, unit ms, no target |

Every filler node: criticality normal, authority agent, no parent, no assignee, no files_owned, no `defer_until`,
`created_by` orch, `created_role` orchestrator, `created_at` its block's commit time, never changed after creation (a done
or cancelled filler task is created and set to its status inside its block's commit). Filler bodies: `<W[a]> <W[b]>
<W[c]>. Routine entry with no follow-up.` in English, or, when j mod 10 = 3, `<WR[a]> <WR[b]> <WR[c]>. Плановая запись
без продолжения.` in Russian (190 of 1,856 filler bodies). Filler titles are ASCII.

### 12.3 Vocabularies

- **W** (index 0–19): amber, basalt, cobalt, dune, ember, fjord, garnet, harbor, indigo, juniper, kelp, lichen, marble,
  nectar, onyx, pollen, quartz, russet, sable, tundra.
- **WR** (index 0–19): янтарь, базальт, кобальт, дюна, уголь, фьорд, гранат, гавань, индиго, можжевельник, ламинария,
  лишайник, мрамор, нектар, оникс, пыльца, кварц, ржавчина, соболь, тундра.

No named title, body or field contains a word of W, WR, or the words `backlog`, `item`, `archive`, `routine`, `entry`,
`follow-up`; no filler text contains a signal term of §14.

### 12.4 Which filler each view contains

| View | Filler k ranges |
|---|---|
| main | 0–1456, 1557–1855 (1,756 nodes) |
| lane/tools | 0–865 |
| lane/audio | 0–1164, 1507–1556 |
| lane/net | 0–1356, 1457–1506 |
| lane/assets, tags/m1 | 0–673 |
| lane/shaders | 0–1065 |
| plan/q3 | 0–1259 |

## 13. Totals on `main` at NOW

| Kind | Named | Filler | Total | By status (total) |
|---|---|---|---|---|
| task | 44 | 880 | 924 | open 559 (31 + 528), done 183 (7 + 176), in_progress 1, cancelled 180 (4 + 176), frozen 1 |
| note | 12 | 351 | 363 | active 361, retracted 1, superseded 1 |
| finding | 9 | 175 | 184 | open 61 (2 + 59), confirmed 62 (4 + 58), refuted 59 (1 + 58), withdrawn 1, fixed 1 |
| doc | 14 | 175 | 189 | current 189 |
| measurement | 1 | 175 | 176 | current 176 |
| rule | 13 | 0 | 13 | active 10, superseded 1, retracted 1, proposed 1 |
| decision | 5 | 0 | 5 | accepted 2, superseded 1, proposed 1, rejected 1 |
| question | 4 | 0 | 4 | open 2, answered 1, dropped 1 |
| verdict | 3 | 0 | 3 | accepted 2, open 1 |
| artifact | 13 | 0 | 13 | present 11, removed 1, planned 1 |
| run | 1 | 0 | 1 | green 1 |
| lane | 5 | 0 | 5 | active 3, merged 1, merge_pending 1 |
| area | 8 | 0 | 8 | active 8 |
| **all** | **132** | **1,756** | **1,888** | plus 3 tombstones (#232, #293, #296) |

Tasks on `main` by priority (named tasks; no filler task has P0, P1 or P2):

| Priority | Named tasks on main |
|---|---|
| P0 | #261, #276, #277 |
| P1 | #240, #241, #242, #245, #260, #262, #263, #273, #274, #275, #280, #287, #290, #291, #297 (15) |
| P2 | #243, #244, #246, #248, #249, #251, #252, #264, #266, #267, #278, #285, #286, #289, #294, #295, #901, #902 (18) |
| P3 | #247, #250, #253, #265, #279, #281, #288, #299 (8) |

Filler tasks by priority on main: P3 352 (r = 1 and 3, all open), P4 528.

Labels on `main` (tasks): backlog 880 (filler only), chore 176 (filler only), assets 17 (#240–#253, #289, #901, #902),
audio 8, net 10 (#273–#281, #297), render 6 (#245, #287, #290, #291, #294, #299), tools 7 (#285–#289, #291, #295), test 3
(#249, #250, #278), perf 2 (#252, #264), campaign 3, docs 1 (#253), ui 1 (#265), hot 1 (#291).

Assignee on `main`: present on 16 tasks (#241–#245, #261, #262, #264, #274–#276, #286–#290; values in §5), absent on the
other 28 named tasks and all 880 filler tasks (908). `estimate` present on 11 named tasks (§5) and on the 352 open filler
tasks with r = 1 or 2 (values (j mod 5) + 1).

## 14. Search signal vocabulary

`search()` and `text_match()` tasks use only these terms, one term per task. Each term occurs, as the exact token
(case-insensitive), in exactly the fields listed and nowhere else in the store.

| Term | Field | Nodes |
|---|---|---|
| `stall` | title | #228 |
| `atlases` | title | #220, #221 |
| `latency` | title | #226, #264, #271 |
| `handshake` | title | #219 |
| `eviction` | title | #244, #801 |
| `interpolation` | title | #280 |
| `traversal` | title | #803 |
| `backpressure` | body | #228, #252 |
| `пакетов` | title and body | #234 |
| `анализатор` | title and body | #266 |
| `задержка` | body | #264, #271 |

A search gold result is the set of listed nodes that also meet the task's other conditions (kinds, statuses, view); its
`order` is `set` (HOLE(lqb-search-scorer) in README). On `lane/net`, #801 and #264 exist; on `lane/assets`, #801 does
not.

## 15. Coverage

| [50 §7.4] item 1 feature | Where |
|---|---|
| 3 campaigns | `TA-0` #240, `TB-0` #260, `TC-0` #273 |
| 5 lanes with 14 days of history | lane/tools, lane/audio, lane/net, lane/assets, lane/shaders; D0–D13 (§7) |
| 2 merges with conflict values | `c.sync-audio` (TextHunk), `c.sync-net` (FieldEdit, StatusFork); plus the clean `c.merge-tools` |
| one staged violation | `merge/lane/assets/from/main`, DanglingEdge (§10) |
| deleted nodes with and without replacement | with: #293 → #294 (main), #251 → #1400 (lane/assets); without: #296, #232 (main), #299 (lane/shaders) |
| flagged blockers | #296 → #297 |
| exogenous inheritance | #245 → container #248, inherited by #249, #250 |
| settled-elsewhere markers | §11 (#245, #261, #262, #274, #275 on main) |
| ids that exist only on a lane | 109 ids (§4) |
| cancelled tasks | #253, #281, #288, #902 and 176 filler tasks |
| precedence DAGs with shortcut edges | #241→#243→#245 with #241→#245; #274→#275→#280 with #274→#280; `DEPENDS_ON` #259→#258→…→#255 with #259→#255 |
| findings about several sections | #502, #803, #1303 |
| file-link artifacts with anchors in every [40] state over a fixture tree | §9 (all 12 link states; anchor states fresh, moved, edited, ambiguous, orphaned) |
| Cyrillic text in 10 % of bodies | 201 of 1,973 bodies (190 filler, 11 named) = 10.2 % |

Node kinds: all 13 ([AR §3.2]) — task, doc, note, rule, decision, question, finding, verdict, measurement (#601,
#1700), artifact, run (#1701), lane (#500…), area. Edge kinds: all 25 of [50 §2.5] (§6). Statuses used per kind: task
open, in_progress, done, cancelled, frozen; finding open, confirmed, refuted, withdrawn, fixed; rule active, superseded,
retracted, proposed; decision accepted, superseded, proposed, rejected; question open, answered, dropped; note active,
retracted, superseded; verdict open, accepted; artifact present, removed, planned; lane active, merged, merge_pending;
run green; doc current; measurement current; area active. Revisions: every form of [50 §2.4] is answerable — refs,
`HEAD`, commits and sequence numbers (placeholders), `~n`, `^2` (at `main~27`), `@n`, `@datetime` (reflog by §7 times),
`a..b`, `a...b`.

## 16. Facts tasks must not depend on

These depend on engine choices the design documents leave open or state inconsistently; no gold result may rest on them.

1. **Gates in derived counters.** [AR §3.3] and X5 say a `GATES` edge constrains completion only; [AR §3.5] counts gates
   in `open_blockers`. So: #242's `open_blockers` and `blocked` on every view that contains the open verdict #804 (every
   view except lane/assets, where `open_blockers` is 0 and `blocked` is no); `blockers(#242)`; `blockers(#246,
   transitive: true)`, which reaches #242; and on lane/assets the `open_blockers` count of #246 (the open verdict #506
   gates it there; #246 is blocked by #242 either way). The `GATES` edges themselves, #242's `ready` and `unblocked` (no: in progress) and #246's `blocked`,
   `unblocked` and `ready` are designed. Every claim in §7 happens while no open `fail_*` verdict gates the claimed task.
2. `topo`, `depth`, and the order `std.ready` gives to tasks of equal priority.
3. `rev`, `updated`, `updated_at` of nodes not in §11.6's clean list (edge ops may or may not touch an endpoint's revision).
4. The value a conflicted field reads on its branch (#270.body on lane/audio; #277.priority and #603.status on lane/net)
   and anything computed from it there (`loop plan=282` and `refuted_share` on lane/net); ask `conflicts()` instead.
5. The expired lease `LS-278` in `leases()` or `claimed`, and raw `markers()` rows.
6. Link and anchor states on trees other than `T-main`, except #900 on `T-audio`; `a.state` of anchors whose file did not
   resolve.
7. Search ranking beyond sets; search with more than one term; tokenization beyond exact whole tokens.
8. `staleness()`, `relevant_to()`, `changes()` feeds and `std.delta`.
9. Absolute sequence numbers and commit ids except through placeholders.
10. The relative order of nodes created in one commit when a task orders by `created_at` alone.
11. Transitive `blockers()` where a blocker is reachable at two depths (`blockers(#280, transitive: true)` reaches #274 at
    depths 1 and 2).
12. `ready`, `unblocked` and `blocked` on lane/shaders, lane/tools and plan/q3.
13. `diff` or `log` rows over ranges that include filler commits, unless the task asks for a count or scopes the diff
    (`scope:` a campaign root).

## Holes

| Id | What | Decided by | Candidates | Constraint the value must meet |
|---|---|---|---|---|
| HOLE(lqb-r14-margins) | the exact synthetic contents of #304 (moved and edited), #307 (replaced), #308 (a10 edited, a11 ambiguous) and #302 (a2 moved, a3 orphaned) | the frozen R-14 resolver constants of chapter 20 (WP-14b draft, values from the replay of WP-76, fixed at WP-81a) | [40 §4.4]'s v1 constants (strong similarity ≥ 0.5 with margin 0.2; replaced containment < 0.29 both ways; fuzzy quote ≥ 0.75 with margin 0.02) or replay-tuned values | every designated state of §9 holds with a margin of at least 0.15 from each threshold candidate; the generator's self-check asserts the states against the model before any run |

## Open points for the review

1. **Root node allocation.** #300 (the root node) is allocated immediately before the first file node #301 in
   `c.files`. [40 §2.4] says the root node is "created with the root's first file node" without an order; chapter 18
   (WP-14) fixes it. If it fixes the opposite order, #300 and #301 swap and nothing else changes.
2. **Lane nodes.** A lane node is created on `main` before the fork, so each lane holds its own lane node, and it carries no
   `worktree_path` artifact ([40 §2.8] maps `lane.worktree_path` to an `abs` artifact). An `abs` artifact per lane would
   shift every later id; if the schema requires it, this design is re-issued with the new ids.
3. **Gates versus `open_blockers`** (§16 item 1): [AR §3.3] (X5) and [AR §3.5] disagree on whether an open `fail_*`
   verdict counts as an open blocker. The fixture puts the open gating verdicts only on tasks whose derived flags do not
   depend on it: #804 on the in-progress leaf #242, and #506 (open only on lane/assets) on #246, which an in-progress task
   blocks. The design also keeps containers out of every claim, because `unblocked` excludes containers ([AR §3.5]) and a
   claim needs `ready`. The review should settle the gate rule in chapter 08.
4. **`children_done` counts cancelled children**, reading `done` as the virtual field of [AR §3.1] (done or cancelled).
   #240 has 5 done children on `main` (#241, #243, #244 done; #253 and #902 cancelled). If chapter 08 counts status `done`
   only, #240 has 3, #273 has 0 and #285 has 3 of 4 (not `ready_to_close`), and the rollup rows of §11 change.
5. **Sync and staged commits have two parents** (the lane's tip, then `main`'s absorbed commit), so `log(main..lane/net)`
   holds the sync commit and not `main`'s commits before it. [AR §5a.3] describes sync as "merge `main` into the lane"
   without stating its parent list; chapter 12 confirms.
6. **`replaced` at a read.** [40 §4.3] records `replaced` in `FILEOBS` at a settle; this design assumes a read with an
   unchanged stat quadruple renders the recorded state, as [40 §4.3] states for `ambiguous (path reused)`.
7. **The `unverified` artifact #311** needs an observation at a commit absent from the repository (`gS`). The generator
   captures it from a scratch tree and drops the scratch repository's objects; if the model's file-link entry points cannot
   express that, #311 is re-designed as `unverified (budget)` with a per-task `fs` budget.
8. **Tokenizer.** Search golds assume case-insensitive whole-token matching without stemming; the signal terms of §14 are
   chosen so that no other inflection of them occurs anywhere in the store, which keeps the golds valid if the frozen
   tokenizer stems.
9. **Filler created in a final status.** A done or cancelled filler task is created open and moved to its status in the
   same commit; its settled marker is absorbed by every view that contains it.
10. **Seq numbering** (§2.6) is left to the format chapters; the corpus uses placeholders.
11. **No `worktree bind` or `ClientHead` records** are designed; the harness passes the tree explicitly (README §8), so
    binding records do not affect any answer.
