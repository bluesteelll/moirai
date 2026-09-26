# 22 — Critique of proposals A, B, C, D: agent-workflow fit, token economy, buildability (revision 2, after the owner update)

*Critique lens: does each design fit the owner's real harness (orchestrator + 9 roles, 3 of them without Bash + Workflow scripts with no filesystem + one git worktree per lane), are its agent-facing outputs cheap and unambiguous, and can the owner build it from scratch in Rust with AI agents? Date: 2026-09-25. Research only; the only file written is this one.*

**What changed in revision 2.** The owner's 2026-09-25 update (R1 full branching, R2 git independence, R3 git-compatible image) is now a hard requirement, fork T3 is replaced by T3′, and forks T14–T16 were added. Proposal D (13) was written after the update and cites the first revision of this file as `[22 §2.1–§2.9]`, `[22 §3.2 issue 1]`, `[22 §3.3 issues 3 and 6]`, `[22 §7.3]` and `[22 §9]`. **The numbering of §2, §3.1–§3.3, §7.3 and §9 is therefore preserved**; where D already adopted an item, the item says so. Sections 4–6 are new (branching in the owner's harness, the git image in the owner's harness, packs/Beads/tokens condensed). The T3 position of revision 1 ("one shared trunk") is withdrawn: R1 overrides it, and §4 says what that costs and how to pay it cheaply.

**Inputs read in full:** [00] digest; [01] roles; [02] orchestration; [03] landscape; [07] agent integration; [08] concurrency and git interop; proposals A (10), B (11), C (12), D (13); critiques [20] (perf/RAM/Windows) and [21] (semantics). Read through the digest and the other critiques' quotations: [04], [05], [06]. Two web checks were made today (2026-09-25) for claims that decide buildability: the gitoxide crate-status page and the Claude Code hooks reference (§10, W-D3 and W-D10). Citations use `[NN §section]`. Every flaw carries a failure scenario or a cited number; where a proposal already handles a problem, I say so and quote the section.

---

## 0. Verdict in one page

| | A "lean embedded" | B "git-faithful" | C "agent-workflow first" | D "branches + git image" |
|---|---|---|---|---|
| **Score on this lens (0–10), R1–R3 as hard requirements** | **4.5** | **5.0** | **4.5** | **7.0** |
| R1 full branching | fails: shared trunk; `exp/*` overlays refuse claims and were never meant for lanes ([A §2 T3], [A §5.4]) | fails: coordination plane "never branched" ([B §3.1], I P1) | fails: "knowledge is scoped, not branched"; `exp/*` at M5 ([C §2 T3]) | meets: `main` is a branch, lanes fork everything, per-client HEAD, merge/rebase/cherry-pick/undo ([D §5a]) |
| R2 git independence | partial: store only in `<git-common-dir>/moirai/`, `MOIRAI_DIR` override, no `init` ([A §4.1]) | partial: same location; has `init` and no-auto-create ([B §4.9]) | partial: same location, no `init` ([C §4.1]) | meets: `.moirai/` discovery chain, pointer files, git as a hint only ([D §5c]) — with one shadowing hole (§3.4 D4) |
| R3 git image | fails: optional binary blobs under `refs/moirai/data`, JSONL export "never re-imported" ([A §5.7]) | fails: `hist` frames as blobs + `export md` never re-imported ([B §5.6]) | fails: same as A at M7 ([C §5.6]) | meets: `.moi` per node, deterministic trees/commits, native and foreign import, round-trip table ([D §5b]) |
| Agent-workflow fit | good on a trunk (dispatcher default, `doc patch`, verdict gating) but no lane branches | best engineering hygiene; lane model closest to R1 but coordination not branched | best surface (`phase_state`, `stats loop`, pack algorithm) on a trunk | good defaults inherited from A/B/C and from revision 1; **four holes in the branch ↔ dispatch loop** (§3.4 D1–D4) that must be closed before lanes go live |
| Token economy | best (13 kinds) | fine | good pack algorithm, 28 kinds | fine: one `branch: … · rev …` header line, `~main` markers, `--across` lines; new vocabulary is orchestrator-only (§4.3) |
| Buildability | best of A/B/C, but fails two hard requirements | worst ordering (lanes before MCP) | leader inside the adoption milestone | heaviest by ~1.5–1.7× A; two long poles (multi-process protocol **and** image round-trip determinism); M1 overloaded; a hand-written git object writer/reader that R2/R3 do not require (§7) |
| Fatal flaws | none as a design; disqualified on R1/R3 | none as a design; disqualified on R1/R3 | none as a design; disqualified on R1/R3 | none; two must-fix defects before the first lane branch is used (D1, D2) |

**Recommendation.** Build D's skeleton (B's engine with [20]/[21] fixes, A's protocol, C's agent surface, D's VCS layer and image) with the grafts of §9 and the sequencing of §7.3: adopt on `main` first, turn the next campaign's lanes into branches only after the four dispatch-loop holes are closed, ship the image at checkpoint granularity through `git fast-import`/`git cat-file` before writing a pack reader/writer by hand, and defer rebase, cherry-pick, op-restore, branch promotion, bundles and SHA-256 out of v1. The smallest slice that already satisfies R1–R3 is S0–S4 of §7.3.

**What full branching costs the owner's workflow, in one sentence:** three new orchestrator rituals per lane (`lane open`, `sync`, `merge`/`resolve`), one `--branch`/marker on every lane-directed command, and a class of "done on lane/x but not on main" states that the design must make impossible to dispatch twice — the rest (packs, briefs, claims, findings, verdicts) is unchanged for the nine roles.

---

## 1. The bar: what the owner's harness actually needs (updated)

Derived from [01 §7–§8], [02 §11–§12], [07 §4–§5, §8] and the owner update.

| # | Need | Evidence | A | B | C | D |
|---|---|---|---|---|---|---|
| N1 | Read-only roles persist their product without file Write | architect/critic/researcher have no Bash [01 §1]; "4 done, 0 errors" with no plan file [01 §7 L2] | MCP M3 | MCP M4 | MCP M3 | MCP M5 (after the image) |
| N2 | Workflow scripts (no filesystem, no shell) can use it; resume re-runs completed agents | [02 §2.2], [07 §4.1] | dispatcher + `apply` + idem §6.6 | same §6.4 | same §6.5 | same + payload- and branch-bound keys ([D §6]) |
| N3 | Hand-offs by id, never truncated text; packs report drops | `.slice()` in 72/347 scripts, 4 incidents [01 §7 L1] | §7.5 footer | §7.5 footer | §7.5 with ids | C's algorithm ([D §7.4]) |
| N4 | Findings are objects across rounds; loop termination is a query | [01 §7 L9], [02 §10.4] | `find` | `find` | `stats loop`, I6 | C's, per branch |
| N5 | Rules with `applies_to` and authority compose the brief; new rules reach the next brief | HDR boilerplate in 36/38 scripts [02 §5.1] | P0 | T0 | C2 | C2 + `~main` rules from `main` ([D §7.4]) |
| N6 | Live cross-lane view: owns/touches, blocks, who runs what | [01 §7 L15] | `touches` | `files_owned` | `owns`, `lane conflicts` | `--across`, store-wide leases ([D §5d]) — **but not completions (§3.4 D1)** |
| N7 | Session brief ≤ 8k chars replacing the MEMORY.md resume block | 21,966 B index at 88 % of cap [02 §10.2]; 10,000-char hook cap [07 §4.1] | yes | yes | yes + `export memory-md` | yes + ahead/behind, staged merges |
| N8 | Measurements carry env + git sha; stale detection | [01 §7 L10] | yes | + `check #ID` | + `env` struct | B's, per branch |
| N9 | Retraction propagates; supersede-in-place reads | [01 §7 L7], [02 F2] | yes | yes | yes | yes, `suspect` derived (D9) |
| N10 | Zero idle CPU; quiet mode; crash-safe on an unstable host | [02 §9] | by construction | `watch` polls (F-B5) | leader, no timers | by construction; exports refuse in quiet mode |
| N11 | Token-cheap ids and lines; ≤ ~10 MCP tools | [06 §12.3], [03 §8.1 item 10] | 9 tools | 10 | 9 | 10 |
| N12 | Beads' failure classes avoided (§6.2) | [03 §8.2] | no store-miss rule | yes | no store-miss rule | yes, **plus a new shadowing path (D4)** |
| **N13** | **R1**: branches for all versioned data, create/switch/list/delete/diff/log/merge (+tags, revert, cherry-pick, undo) | owner update | no | no | no | yes |
| **N14** | **R2**: own VCS, works with no git and outside any repo, git as a hint only | owner update | partial | partial | partial | yes |
| **N15** | **R3**: deterministic image, readable layout, import/round-trip incl. git-side edits, incremental export with id map, lossless/lossy stated | owner update | no | no | no | yes |

A, B and C clear N1–N11 in substance and fail N13/N15; their other layers are graded in §3.1–§3.3 because D reuses them and the winner should know which parts are best. D is the only candidate that clears the bar; §3.4 and §4 are about the price.

---

## 2. Cross-cutting flaws (revision 1 numbering preserved; D's status noted)

These were the findings revision 1 weighted most, because no proposal handled them and each has a concrete failure in the owner's harness. D adopted most of them; what remains is stated per item.

### 2.1 Role write policy keyed on `agent_type` does not match how the scripts spawn agents

A §7.2, B §7.2/§7.4 and C §7.6 enforce the per-role policy on the `agent_type` stamped by the `PreToolUse` hook. Across the 347 scripts, `agentType: 'general-purpose'` occurs 94 times and `'claude'` 41 times [01 §1], `results-analyst` **0** times because its role file is unparseable YAML and Claude Code silently skips it [01 §2.7]; the "writer" step uses `'claude'` [02 §2.1]. **Failure:** a refutation agent spawned as `general-purpose` calls `remember{kind: refutation}`; the policy table has no such row; the engine refuses (exit 6) and the refutation pass — the owner's loop-termination mechanism [01 §3] — stops working inside Workflows, or the engine allows everything and the policy is decorative. **Fix:** key the policy on an explicit role label passed in the dispatch marker (`moirai:role=refuter`) and stamped from `SubagentStart`/the dispatch table, with `agent_type` as fallback and a documented default for unknown types. `doctor agents` must report the unparseable `results-analyst.md`.
**D:** adopted ("role policy keyed on the dispatch label [22 §2.1]", [D §9 M5]).

### 2.2 Dispatcher leases of 60 minutes versus multi-hour Workflow runs

A §6.3, B §6.2 and C D13 take [07 §7.4]'s 15 min / 60 min defaults; the owner's runs last hours (a 7-hour run failed at its analysis stage [01 §2.7]; review loops reached 30 rounds [01 §5.3]; p90 14 agents per run [02 §5.1]). **Failure:** the orchestrator claims #710–#712 with `--ttl 60m`; two hours in, dev#3's lease has expired; `SubagentStop` releases it; the final `apply results.json` presents token 1044 → exit 5, partial batch (exit 8). **Fix:** run-scoped dispatcher leases (`--ttl run`, released by `apply`, `run close` or a dead `bg_task_id`); wall-clock TTL only for self-claims.
**D:** adopted ([D §6] "run-scoped dispatcher leases (`--ttl run`)", [D §7.5 step 6]).

### 2.3 The `agentId → task, lease` mapping does not exist for Workflow agents

All three map agent id to task by parsing a `moirai:task=#51 lease=L-9` marker in `PostToolUse(Agent)`. Workflow `agent()` calls are not the Agent tool; whether `SubagentStart`/`SubagentStop`/`PostToolUse(Agent)` fire for them is unverified [07 §8.6, §10 R3]; `SubagentStart` receives no prompt [07 §4.2]. Verified again today: the hooks reference says nothing about Workflow scripts (§10 W-all-2). **Consequence:** for the owner's main spawning path (401 runs, 3,294 agent starts [02 §5.1]) the `SubagentStop` lease safety net cannot know which lease the stopping agent held. **Fix:** the dispatcher pattern is the only supported Workflow pattern until the 5-minute experiment passes; the safety net is run-scoped (§2.2), not agent-scoped.
**D:** adopted the experiment as the first M2 task ([D §7.3]); the dispatcher default stands.

### 2.4 `#N` mention parsing collides with the numbers already in the owner's notes

All three parse `#N` in text into `mentions` edges. The owner's corpus is full of GitHub issue numbers written exactly that way (`#4767`, `#6551`, `#4475`, `#3760` [03 §2]). **Failure:** past ~5,000 nodes, a note saying "Beads #4767 lost 7 of 8 closes" creates a `mentions` edge to moirai node #4767; `rm #4767 --dry-run` lists the note as a referrer; the impact report the deletion feature exists to make trustworthy is polluted. **Fix:** a sigil rule (`#N` only when not preceded by an alphanumeric and not followed by `/` or `.digit`), parse only `N < next_id`, render parsed mentions as "text mention" in impact reports; ASCII renderings, not `†`.
**D:** adopted ([D §3 D8]).

### 2.5 `chars / 3.5` token budgets are wrong for the owner's Russian text

The owner's MEMORY.md is "mostly Cyrillic, 2 bytes per character" [02 §10.2]; rulings are stored verbatim. Cyrillic tokenises far denser per character than English (my estimate 1.5–2.5 chars/token versus 3.5–4.5; unmeasured). **Failure:** `pack --budget 12000` fills 42,000 characters of Russian rulings and lands at ~17–25k tokens, near the 25k MCP cap [07 §4.1]. **Fix:** budget in characters (the hook cap is in characters anyway); per-script token ratios measured in M0.
**D:** adopted ("budgets in characters with per-script token estimates", [D §7.4]; Cyrillic ratio in M0).

### 2.6 Rules with no `applies_to` can silently vanish from packs

A's P0 requires `applies_to_roles` to include R **and** globs to intersect the task's areas; B's T0 "matches R or the lane's files"; neither says what an *empty* set matches. **Failure:** a critical rule forbidding machine-wide process kills, entered with `moirai rule --critical --stdin` and no `--applies-to`, is absent from every developer pack — the rule that exists because an agent killed three lanes' builds [01 §3]. **Fix:** empty `applies_to` = `*`; packs print the global critical-rule count in the header.
**D:** adopted ([D §7.4] "empty `applies_to` = `*`", header count).

### 2.7 The engine is built before the value of packs and briefs is validated

A: 60 % of the work precedes the first adoption gate; B has the only escape hatch ("ship on `redb` if M1 slips", [B §10 R5]); C's precision gate sits at the end of M3. **Fix:** an engine trait with a throw-away oracle backend so packs/briefs are adopted before the from-scratch engine exists.
**D:** adopted ("Oracle backend first (S0 of [22 §7.3])", [D §2 T13], [D §9 M0]). But D's M1 (large) now bundles refs, branches, pins, overlays, promotion, checkout, undo, revert, tag and GC with the engine, and M2 only adopts on `main`; see §7.1 on splitting M1.

### 2.8 Test infrastructure is under-budgeted relative to the reports' own warnings

[04 §11], [08 §8.1]: SQLite's WAL-reset race lived 16 years and fell to deterministic simulation in ~15 minutes; ALICE found 60 crash bugs in 11 systems. The simulator with a virtual file/lock layer, crash-point enumeration, fsync-error and lock-delay injection is a medium project on its own; 20–25 % of the whole.
**D:** adopted ("deterministic simulator + kill loops + fuzzers 20 %, its own milestone", [D §9]). The image adds a second determinism test surface (byte-identical round trips in two object formats, fuzzed `.moi` and pack parsers) that D lists under M4 but does not size; §7.1 sizes it.

### 2.9 The graph starts empty; packs are only as good as what was imported

None of A/B/C imported the *current pins and known reds* that every HDR hard-codes [02 F11]; the first campaign's packs would lack the "current defect state with numbers and dates" block and the orchestrator would keep writing HDR — a dual source of truth for the content packs were meant to own. **Fix:** a one-off import of the dozen standing rules (`applies_to`, default `*`), current pins as `measurement` nodes with `git_sha`, live lanes and open owner questions in the smallest slice.
**D:** adopted ([D §9 M2] "one-off import of the dozen standing rules, current pins, live lanes [22 §2.9]").

**New cross-cutting item under R1 (applies to any full-branching design, i.e. to D and to any variant the owner picks):**

### 2.10 Branch identity must not depend on the current directory alone

Every proposal resolves "which lane am I in" from the caller's `cwd` ([B §5.3] "The CLI resolves the current branch from cwd → `.git` → worktree path → lane node"; [D §5a.4] "the registered binding for the current directory … the binding for the git worktree"). The owner's harness breaks that assumption in three measured ways: the Bash tool resets `cwd` per call, which is why 11 of 38 scripts carry a rule to repeat the `cd` inside the same command on every call [02 §5.1]; the stdio MCP server's `cwd` belongs to the session, not the subagent, and subagents in other worktrees share the parent's connection [07 §4.4], [08 §2]; and the identity stamp that could carry `cwd` is a `command` hook (verified today: `mcp_tool` hooks return only text, no `updatedInput`, §10 W-all-1), which [20 §1.4] prices at 20–73 ms per call and therefore narrows to writes. The consequence for D is spelled out in §3.4 D2–D3; the design rule is: **every lane-directed call carries its branch explicitly, and a lease resolves to its branch** (§9 graft 2).

---

## 3. Per-proposal critique

### 3.1 Proposal A — "Lean embedded"

**Against R1–R3.** A's branches are "delta overlays that rebase on read" with `Claim/Release/Complete` refused on branches ([A §5.2], [A §5.4]) — what-if branches, not lanes; task status *can* be set on `exp/*` and merges through the lattice ([21] X13). No `checkout`, no per-client HEAD, no tags, no reflog. A's git linkage is provenance plus an optional "publish log and base files as git blobs under `refs/moirai/data`" ([A §5.7]) — a backup of binary segments, not a readable image, with no import path. Store discovery is `<git-common-dir>/moirai/` with `MOIRAI_DIR` override and no `init` ([A §4.1]). **A fails R1 and R3 and only partially meets R2.**

**Strengths (on this lens, still the best available for the layers D reuses).** The smallest engine skeleton with zero idle CPU by construction ([A §2 T2], [A §6.1]); the dispatcher pattern as the default with `apply` keyed by run and the honest "attribution against a hostile model is out of scope" ([A §7.2], [A §7.6 steps 8–9]); `doc patch #section --remove @old --add @new` with the engine refusing when the removed text is not a substring ([A §7.1], walk-through step 7) — the architect's verbatim-removed-text guard [01 §2.1] made mechanical; the `--explain` and guard-conflict outputs ([A §7.1]) are the best "actionable error with the current value" examples [07 §3]; baselines before M1 with the rule that the from-scratch engine must beat them ([A §9]).

**Serious issues (revision 1, with D's status).**
1. `--ids` prints a header line (`seq 812 · 3 blocking`), so `--ids | xargs moirai show` passes `seq` and `812` as ids ([A §7.1] vs [07 §6.2]). **D: fixed** ("`--ids` never prints a header", [D §7.1]).
2. `blocking --ids` includes verdict nodes, because `is_blocker` counts any outgoing `blocks` edge ([A §3.5]); the owner asked for blocking *tasks*. **D: fixed** (`blocking` defaults to `kind:task`; verdicts use `gates`, [D §3 D5]).
3. No store-miss rule and no `init` verb ([A §4.1]); Beads #6551/#6552 created a phantom store in a worktree [03 §2.7]. **D: fixed in the chain, reopened by `init` (§3.4 D4).**
4. No merge-time and loop-statistics queries; walk-through step 12 substitutes `show #121 --neighbors 2` for the merge check [02 §12.5]. **D: fixed** (`merge-check`, C's `stats loop`).
5. No hook experiment in M2. **D: fixed** (M0).
6. §2.1–§2.9 all apply. **D: adopted.**

**Over-engineering / cut candidates:** hand-written frozen bitsets and a front-coded term dictionary are real code and test surface ([A §2 T10]); the frozen container is justified by [05 §10.3] (pure-Rust `roaring` copies on deserialize), the term dictionary is not needed below ~20k nodes. `--pinned` branches by reverse-applying trunk commits and whole-graph `at` by genesis replay are fine as documented cold paths.

**Fatal flaws:** none as a design. **Score: 4.5** — best buildability and token discipline of A/B/C; disqualified on R1 and R3.

### 3.2 Proposal B — "git-faithful"

**Against R1–R3.** B's coordination plane (task, lane, run, question status) "never branched; visible to all worktrees at once" ([B §3.1], I P1); lane branches carry only knowledge-plane ops ([B §5.3] "A lane branch is sparse"); `exp/` full branches at v2 refuse claims but not status edits ([21] X13). **R1 says status, done and blockers must branch, so B fails R1 as written.** R3: `moirai push` writes retired `hist.NNNN` frames and `head.json` as git blobs under `refs/moirai/data` ([B §5.6]) — a transport of binary frames with uid re-verification, not a readable image; `export md` is "never re-imported". **Fails R3.** R2: `init`, no-auto-create, `MOIRAI_DIR` "for tests" ([B §4.9]) — partial.

**Strengths.** B is the engine and merge base D builds on, and on this lens the parts worth keeping are: `moirai init` and "a miss inside a worktree never auto-creates a store" ([B §4.9]); M0 = format spec + oracle backend with "ship on `redb` if M1 slips" ([B §10 R5]); the verbatim-removed-text guard enforced at write and at merge ([B §5.4]); `moirai check #ID` (pinned citations and `measured_on` ancestry, PLANFENCE-style [06 §11.2]) and `notes --path` ([B §7.1]); `export md --to docs/moirai/` as a regenerated, `linguist-generated` view ([B §5.6]); the fullest conflict taxonomy (`OwnerFieldEdited`, `RemovedTextNotInBase`, hints); the role table with "developer: never verdicts on own task" ([B §7.4]); `refs/merge/<lane>` staging so trunk never advances with a structural violation ([B §5.4 step 7]).

**Serious issues (revision 1 numbering preserved; re-read under R1).**
1. **Verdicts on a lane are invisible to the trunk-side `ready`/`claim`.** Findings, verdicts and sections are knowledge-plane and land on `lane/<name>` as `proposed` when written with `ctx.cwd` in a worktree; "a trunk read never shows proposed nodes" ([B §3.1]); `ready` requires "no open blocking verdict" ([B §3.6]) but `blocks` is trunk-only ([B §3.4]). **Failure:** the critic writes `verdict{outcome: fail_fixable}` about plan #88 on `lane/l5np`; the orchestrator in the main checkout runs `ready --scope #88` and `claim #89 #90`; nothing blocks #89; "whether the design is approved" again lives only in the orchestrator's context [01 §7 L9]. *Under R1 this becomes the general "which branch does dispatch read?" question; D's answer is "dispatch on the lane" ([D §5d.2]) plus `--across`, and §3.4 D1 shows the mirror-image hole D still has.*
2. **Cross-lane findings are invisible by construction** until merge; two lanes reviewing the same shared file (the sibling-lane shared-file situation of [02 §7.2]) never see each other's findings. *Under R1 this is the intended git-like isolation; D's `--across` view is the right mitigation and B lacks it.*
3. **Lanes are an explicit ritual with unstable identity**: 44 worktrees, several detached, harness-created `wf_*`/`worktree-*` trees [08 §2, §7.2]; an `isolation: worktree` subagent branches from the default branch [07 §4.4] and maps to no lane. *D inherits this and adds `worktree bind` and a default to `main` for unbound directories ([D §11 decision 3]) — the right default; §3.4 D3 covers the `cwd` hazard.*
4. M3 (lanes, live-rebased reads, `lane sync`, `refs/merge`, typed merge, validators, ancestry cache) is "large" and precedes MCP (M4); the three Bash-less roles wait behind the merge engine. *D repeats this ordering (M3 merge, M4 image, M5 MCP); see §7.3.*
5. `moirai watch` polls HEAD every 250 ms without a leader ([B §6.1]) — banned by [02 §9]. **D: fixed** (`watch` leader-only, [D §9 M6]).
6. Canonical commit hashes over `uid` cost a lookup per op and exist only for push/pull. *Under R3 this cost is justified: D's `Moirai-Commit` trailer verification depends on store-independent hashes ([D §4.2]).* Withdrawn.
7. §7.1 example: `blocking --ids` lists `#40` while the next example shows `#40` deleted — a spec-hygiene defect.
8. §2.1–§2.9 apply. **D: adopted.**

**Over-engineering for a *trunk-first* v1 (still true under R1 for the *first* milestone):** live-rebased reads with `--pure`, `lane sync`, `refs/merge`, `--continue|--abort`, `tag`, `reflog`, 96 inline refs with overflow, `hist` frames, HLC + generation numbers — all needed eventually under R1, none needed to adopt packs and briefs on `main`. HMAC-sealed `ctx` is not enforcement ([21 §3.2 minor]); D dropped it.

**Fatal flaws:** none as a design. **Score: 5.0** — the closest of A/B/C to R1 (refs, staging, reflog, tags, sparse lanes exist) and the best hygiene; disqualified on R1 (coordination never branched) and R3.

### 3.3 Proposal C — "Agent-workflow first"

**Against R1–R3.** "Coordination state lives on one live trunk … explicit moirai branches exist only for what-if planning" ([C §1 item 3], [C §2 T3]); `exp/*` at M5 as "the same overlay structure as the tail" ([C §5.2]) with the merge engine "used … by nothing in v1's daily path" ([C §2 T7]). **Fails R1.** `refs/moirai/data` publish at M7, `export memory-md` never re-imported ([C §5.6], [C §7.4]). **Fails R3.** No `init` ([C §4.1]). **Partial R2.**

**Strengths (the agent surface D takes verbatim).** `phase_state` with the 14 states of [01 §5.1] and `return_to` on verdicts ([C §3.2]); `stats loop #P` ("raised 3, confirmed 2, refuted 1 (33 %); confirmed blockers: 1 → continue") and `stats refuted-share` ([C §3.6], [C §7.7]) — the owner's rule [01 §3] as two commands; I11 ("retest is not re-review") and I12 (a run closes green only when every expected artifact has a `produced` node with a read-back sha256) ([C §3.4]); the pack algorithm with three renderings, per-class quotas, degrade-before-drop, owner rulings never below L1, dropped ids listed, `since_round` ([C §7.5]); `export memory-md` so the always-loaded 25 KB channel carries the computed checkpoint ([C §7.4]); the adoption path and the HDR fallback for one campaign ([C §9], R2); `lane conflicts` and `lane merge-check` ([C §7.1]).

**Serious issues (revision 1 numbering preserved).**
1. **28 node kinds and 35 edge kinds in v1** ([C §3.2–§3.3]) is Beads' wide-record accretion [03 §2.2] pre-loaded; the ~60-line skill cannot teach 28 kinds; unknown symbols are "kept, not dropped" ([C §3.1]) — a typo sink. **D: fixed** (A's 13 kinds + C's fields, [D §2 T12]).
2. **`pack` is a write** ([C §7.5 step 5] records `consumed` edges from the caller's `run`); with no `run` node for Workflow agents (§2.3) it either records nothing or creates orphans. **D: fixed** ("`pack` is a pure read (no `consumed` edges unless `--record-run`)", [D §7.4]).
3. **Direct MCP writes inside Workflows with model-typed counter keys** replay wrong on resume: re-run critics emit different findings under the same keys and get the *old* results back ([C §7.7]; [21] X3). **D: fixed** (payload-bound keys, exit 9; "Workflow agents that write directly use content-hash keys [22 §3.3 issue 3]", [D §6]).
4. The opportunistic leader (pipe, DACL, forwarding, group commit, broadcast, failover) is bundled into M3, the first adoption milestone ([C §9]). **D: fixed** (leader M6).
5. The precision@k gate on "20 replayed historic briefs" is unmeasurable under C's own start-clean import ([C §9 M3] vs adoption step 4–5). **D: not adopted, but D has no replay gate either**; §7.3 S2 states an adoption gate the owner can judge.
6. **`lease` as a node kind** consumes `#N` for ~40k leases/year [02 §12.6] and puts leases in the same id space as tasks; `holds` restrict leaves `rm` on a claimed task with no release path. **D: fixed** (leases are runtime records; `rm` refuses unless `--release`, [D §3 D7]).
7. Two consistency slips: history sized at "10 ops × 60 B" (undercounts ~7× against C's own 0.3–0.6 KB commits, [C §8] vs [C §5.1]); "roaring bitmaps, frozen (queryable in place)" with the pure-Rust crate ([C §4.3]) — pure-Rust `roaring` copies on deserialize [05 §10.3]. **D: uses A's frozen bitsets.**
8. No `init`/store-miss rule. **D: fixed in the chain (see D4).**
9. §2.1–§2.9 apply; C's role table is the most exposed to §2.1. **D: adopted.**

**Fatal flaws:** none as a design. **Score: 4.5** — best fit and adoption thinking of A/B/C, all of which D absorbed; disqualified on R1 and R3.

### 3.4 Proposal D — "Branches + git image"

**Against R1–R3.** R1: refs of kinds `work`/`plan`/`merge`/`tag`, `branch`/`checkout`/`--list`/`-d`/`diff`/`log`/`merge`/`rebase`/`cherry-pick`/`revert`/`undo`/`op restore`/`tag`/`reflog` ([D §5a], [D §7.1]); every versioned datum branches ("No planes", [D §3 D1]); runtime state enumerated with its branch interaction ([D §5d.1]). R2: `--store` → `MOIRAI_DIR` → walk-up `.moirai` dir/pointer → git hint, "a miss never creates a store", no `git` spawned by the core, a 60-line textual reader for provenance ([D §5c]). R3: `.moi` canonical text per node under a 2×2-hex `uid` fan-out, tombstones as files, conflicts as lines, trailers with `Moirai-Commit` verified on import, native/foreign import through the merge validators, `gitmap`, destinations, granularity, lossless/lossy table ([D §5b.1–§5b.10]). **D meets R1–R3 on paper.** Everything below is about whether the owner's harness can *use* it and whether the owner can *build* it.

**Strengths (on this lens).**
- D is the only proposal that answers the owner update, and it does so by delta against A/B/C rather than by a fourth engine — the right economy for a from-scratch project.
- It adopted nearly every revision-1 fix (§2.1–§2.9) and the [20]/[21] grafts as defaults: `--ids` header-free, `blocking` = tasks, run-scoped leases, dispatch-label role policy, chars budgets, `*` default, sigil rule, payload-bound keys, oracle-first, simulator as its own milestone, import of standing rules and pins.
- The branch UX is confined to the orchestrator: "Versioning and image verbs stay CLI-only" ([D §7.2]); subagents write to whatever branch their directory is bound to; the 10 MCP tools are C's nine plus a read-only `branch` ([D §7.2]). The nine roles' protocol (pack → work → complete/remember) is unchanged.
- `--across` views and `~main` rules in every pack ("owner rulings are never hidden by branching", [D §7.4]) are the right two mitigations for isolation; the SessionStart brief prints the bound branch, ahead/behind and staged merges ([D §7.3]).
- Store-wide leases keyed by `uid` that carry the branch they were taken on ([D §5d.1]) are the right primitive; `plan/*` branches that cannot mark work done ([D §3 D2]) close [21] X13.
- The image design is careful where it matters for R3: bodies are the tail of the node file so `git diff` shows prose changes, in-edges are never serialised, tombstones stay as files so "deleted ≠ absent" survives git merges, conflict values are representable, the exporter re-parses what it writes, foreign commits are validated by the same engine and never trusted as text merges ([D §5b.1–§5b.9]). The failure-mode table ([D §5b.9]) is the best in any proposal.
- Explicit lossless/lossy statement and per-scale cost table ([D §5b.7], [D §5b.10]); `doctor image --rebuild-map` makes `gitmap` derivable.

**Serious issues.** Each with a failure scenario traced through D's own text.

**D1 (must-fix before any lane branch is used) — a task completed on a lane becomes READY again on `main` once its lease is released.** [D §5d.1]: "`ready` on any branch excludes nodes leased by another holder". [D §5d.2]: "A task completed on `lane/x` is `done` on `lane/x` only. `main` learns it at `merge lane/x --into main`". [D §10 risk 1]: "leases are store-wide so double work is impossible even when views diverge". **Trace.** `#89` is created on `main`, forked into `lane/l5np`, claimed by dev#1 with a run-scoped lease (branch `lane/l5np`). dev#1 runs `complete #89 --lease L-18` → `done` on `lane/l5np`, lease released (the walk-through's step 7, [D §7.5]). The lane is not merged for two days (the owner's lanes live for days [01 §5.4]; the merge queue is sequential and GREEN-gated [02 §7.2]). The orchestrator's next `SessionStart` brief on `main` lists `#89 open` under READY (no lease, `open` on `main`); `claim --next --scope #88` on `main` hands `#89` to dev#4. The owner's stated fear — "two lanes must not both build #12" ([D §2 T16]) — is realised by the design's own default; the "double work is impossible" claim is false the moment `complete` releases the lease. D's answer "dispatch happens on the lane branch" ([D §5d.2]) holds only if the orchestrator never runs `ready`/`brief`/`claim --next` on `main` for work that has a lane — but the brief *is* on `main`, and `moirai blocking --ids` (the owner's headline query) on `main` will list `#89`'s dependents as still blocked and `#89` as a live blocker. **Fix (small, no shared plane):** `complete` writes a store-level runtime record `completed {uid, branch, commit_id, holder, hlc}` next to the lease table; `ready`, `claim`, `blocking` and `brief` on every branch treat a node with a live `completed` record as "done on `<branch>` (unmerged)": not claimable, not listed as a blocker, rendered with the branch; the record is dropped when the completing commit is an ancestor of the reader's tip (gen-pruned walk, µs, [D §5a.1]) or the branch is deleted (then it becomes a triage line in `doctor`/`brief`). This is the lease mechanism D already has, extended by one state; ~30k records/year at ~40 B. The same treatment should cover `rm` on a lane (`deleted {uid, branch}`) so `main` never dispatches a task a lane deleted.

**D2 (must-fix) — the branch of an unstamped MCP read is the session default, not the caller's lane.** [D §7.2]: "Every tool resolves the branch from the stamped `ctx.cwd` binding (§5a.4) … The stamp hook matches write tools only (G6)." Verified today (§10 W-all-1): `mcp_tool` hooks "read the tool's text content the same way it reads command-hook stdout" and document no `updatedInput`, so the stamp must stay a `command` hook — [20 §1.4]'s 20–73 ms spawn per stamped call is real, and G6 (stamp writes only) is the right economy. But then reads carry no `ctx.cwd`. **Trace.** Architect (no Bash, MCP, in a Workflow for `lane/l5np`) calls `write{ops:[plan, section×6]}` — stamped, `ctx.cwd = <lanes-dir>/l5np`, lands on `lane/l5np` (walk-through step 4). Critic calls `pack{target:#88, role:architecture-critic}` — a read, unstamped, `ctx` absent, client key falls through [D §5a.4]'s chain to `config.default-branch` = `main`. The plan is not on `main`. The critic answers "NOT REVIEWABLE — the plan does not exist" — the exact incident of [01 §7 L2] reproduced by the branching layer. **Fix:** every tool takes an explicit `branch` parameter (model-typed from a `moirai:branch=lane/l5np` marker the dispatcher puts in the prompt, ~6 tokens), validated against the lease's branch whenever `lease` is also given (§9 graft 2); reads without `branch`, `lease` or a stamp use the session's `checkout` (client key `session:<id>`), and the server prints the resolved branch in its first line (D already does). Stamping every tool is the alternative; it costs a spawn per read and is the owner's call (§11 decision 3).

**D3 (serious) — directory binding versus the Bash tool's per-call `cwd` reset.** [D §5a.4] resolves the CLI's branch from "the registered binding for the current directory … matched by longest bound prefix". The Bash tool resets `cwd` between calls; 11 of 38 scripts carry a rule to repeat the `cd` inside the same command on every call, because agents forget it [02 §5.1]. **Trace.** dev#1 in `<lanes-dir>/l5np` runs `cd <lanes-dir>/l5np && cargo test`, then in the next call `moirai complete #89 --lease L-18 --summary -` without the `cd`. The call resolves to the session cwd (the owner's main checkout, bound to `main`): "`complete`/`set --lease` write on the claimer's current branch (must equal the lease's branch unless `--move-lease`)" → exit 5, lease branch mismatch; the developer retries with `--move-lease` (the error will suggest it) and completes `#89` on `main` while the implementation is on `u/l5np`. A read (`pack #89`) from the wrong cwd silently returns the `main` view with no plan sections. **Fix:** when `--lease` is given and `--branch` is not, the branch is the lease's branch (D records it) — no `cwd` involved; `pack --lease L-18` likewise; the dispatch marker carries `branch=`; `--move-lease` requires an explicit branch and prints a warning. With this rule the cwd binding is a convenience for humans and interactive sessions, never the only source of truth for an agent.

**D4 (serious, Beads #6551/#6552 class) — `moirai init` inside a worktree shadows the shared store.** [D §5c] discovery order: flag → env → walk-up `.moirai` → git hint. Step 3 wins over step 4, and `moirai init` "creates `<dir>/.moirai/`" anywhere; the exit-7 message prints "`moirai init` instructions". **Trace.** A developer's CLI call in `<lanes-dir>/l5np` fails with exit 7 for any reason (a stale pointer file, a moved store, a typo in `MOIRAI_DIR`); the model reads "run `moirai init`" and runs it in the worktree; a fresh empty store now sits in `<lanes-dir>/l5np/.moirai/` and shadows `<git-common-dir>/moirai/` for every process whose cwd is under the lane; the lane's writes go to the phantom; the orchestrator on `main` sees nothing; nothing warns. Beads' phantom DB "returned 0 of 96 memories for weeks" [03 §2.7]. D's "a miss never creates a store" is true of *discovery* but `init` is one model-typed command away. **Fix:** `init` refuses when a git hint resolves to an existing store unless `--force --shadow`; the exit-7 hint names the hinted store and says `init --link <store>`; `doctor store` lists directories whose walk-up `.moirai` shadows a git-hint store; pointer files carry the store id so a stale pointer is detected, not followed.

**D5 (serious) — hooked `sync` on `SubagentStart` can land conflict values as a side effect of spawning an agent.** [D §10 risk 1]: "`sync` is one command and can be hooked to `SubagentStart`". `sync` = `merge main --into <lane>`; value conflicts "land as conflict values (jj) unless `--strict`" ([D §5a.7 step 7]). **Trace.** The architect edited section `#91` on `lane/l5np`; the orchestrator corrected `#91`'s heading on `main` (both sides editing one section is the register-incident shape [02 §7.3]). The tester is spawned; the hook runs `sync`; the merge lands a `TextHunk` conflict value on `#91` on the lane and flags `#91` `conflicted`; the tester's pack renders `#91` "with `<<<<<<<` markers on read" ([D §5a.7]) inside the "Metrics and validation" section. A hook with a 10 s timeout has changed the lane's state, no agent asked for it, and the conflict is now the tester's problem. **Fix:** the hook runs `sync --check`; it auto-applies only when the preview has zero conflicts and zero violations; otherwise it emits the "behind main: N commits, 1 conflict on #91 → moirai sync" notice D already prints. The same rule for any future auto-merge.

**D6 (serious for orchestrator ergonomics) — every lane-directed orchestrator command needs `--branch`, and `apply` defaults to the wrong branch.** The orchestrator's client key is its directory → `main` ([D §5a.4]); `apply` batches "land on the branch named in the batch (default: the orchestrator's client branch)" ([D §6]). **Trace.** After a run on `lane/l5np`, the orchestrator runs `moirai apply results.json --idempotency-key run:r7` without `--branch`; the batch's `complete #89 --lease L-18` fails (lease on `lane/l5np`, batch on `main`) → exit 8 partial; the findings and verdict in the same batch either landed on `main` (all-or-nothing? D does not say for branch mismatches inside a batch) or the whole batch failed. With three concurrent lanes ([01 §5.4]) and `parallel([laneA(), laneB()])` scripts ([02 §5.3] P4), the orchestrator juggles branches on every call. **Fix:** derive the branch from the run: the idempotency key `run:<id>` names a `run` node, `run --runs_in--> lane`, the lane node carries `moirai_branch` ([B §3.3]); `apply` uses it unless `--branch` overrides; a batch with a lease whose branch differs from the batch branch is refused before any write. `lane open` should also accept `--run` bindings.

**D7 (over-engineering for v1) — verbs and machinery R1 lists as "ideally" that the owner's next campaign will not use.** `rebase --onto` with `--continue|--abort` (history rewriting with pin re-fork), `cherry-pick`, `op log`/`op restore` (jj-style restore of *all* refs), `plan/*` and `exp` branch kinds, branch promotion to `seg.b<ref>.K` delta segments with `TOUCH` bitmaps, `--with-oplog` export, bundles v2/v3, SHA-256 images, `OFS_DELTA` in the writer, multi-pack readers, `--via fast-import` *and* a hand-written writer. Each is defensible eventually; together they are the reason D's M1 and M4 are "large" and "medium–large". §7.2 lists what to defer and why the owner's workflow does not miss it: the owner's lanes are ~1–3k ops ([D §5a.3]'s own estimate) so promotion (8k threshold) rarely triggers if `sync` does not append trunk ops (§9 graft 8); the owner never cherry-picks knowledge between lanes today; `op restore` has no analogue in the owner's rituals.

**D8 (buildability) — M1 is A's M0 + M1 + M4 in one milestone, before any CLI.** [D §9 M1] "log/HEAD/LOCK protocol (G1, X2, epoch), delta segments, tombstones with X4, I5′ PK, derived state, leases, idempotency, refs/reflog/`ClientHead`, branch fork with pins, overlay build, promotion, `checkout`, `log`, `diff`, `show@`, `blame`, `undo`, `revert`, `tag`, GC with reflog expiry" with the exit gate "50 branches × 2k ops each readable within budget". Revision 1's §2.7 objection returns in a new form: the from-scratch engine *and* the VCS layer both precede adoption, and D's own escape hatch ("packs and briefs prove themselves on the oracle backend if M1 slips") only covers `main`. **Fix:** split M1 into M1a (engine, trunk only, A's M0 exit gates) and M1b (refs, branches, overlays, undo, tag, GC); M2 (CLI, packs, hooks) depends on M1a only; M1b lands with M3 (merge) when the next campaign's lanes are ready to use branches (§7.3).

**D9 (image cost and churn) — 1:1 commits for every lane branch into a project repo is the wrong default.** [D §5b.10]: incremental export of one commit touching 3 nodes ≈ 25 KB raw / 12 KB zlib and 11 loose files; ~1k commits/day → ~4 GB/year undeltified, ~0.5 GB after `git gc` (est.). [D §11 decision 1] recommends a separate bare repo at `commit` granularity. On the owner's disk-starved machine [02 §9] and with lanes that are deleted after merge (`branch -d`, reflog 90 days), exporting every lane's every commit produces gigabytes of trees for branches that will not exist in a month, and a `git gc` ritual the owner does not run today. **Fix:** default export set = `main` + `tags/*`; lanes on request (`--refs lane/*`); default granularity for the *project* repo = one checkpoint per merge into `main` plus daily; 1:1 only for the separate image repo and only if the owner asks for per-commit archaeology. D allows all of this ([D §5b.8] hybrid); the defaults should be the cheap ones.

**D10 (buildability; verified today) — the hand-written git object writer/reader is not required by R2/R3 and gix already has the pieces D says are missing.** [D §2 T10]: "**Not** `gix`/`git2` in the product (gix has pack writing and ref transactions but no push, and SHA-256 is listed as parity work still to do)". The gitoxide crate-status page (fetched 2026-09-25, §10 W-D10) lists loose-object writing, "write index along with the new pack", and "delete, create or update single ref or multiple refs while handling the reflog … writing loose refs into packed-refs" as **done**, push as partial and SHA-256 as ongoing — exactly as D states. But neither gap matters for D's own design: transport is "outside the core: `git push …` run by the user, a hook, or `moirai image push` which simply spawns `git` if present" ([D §5b.6]), and the default object format is SHA-1 ([D §11 decision 2]). So the stated reasons do not force a hand-written writer (D sizes it at ~2.5–3.5k lines; with the reader's delta resolution, idx v2, bundles, `packed-refs`, fuzzing and the differential tests I put it at 4–6k lines plus tests, §7.1). Two cheaper R3-compliant paths exist and D already half-supports one: (a) `--via fast-import` as the *primary* writer (a text stream; object ids are a function of content, so determinism is unaffected; ~600–900 lines) plus `git cat-file --batch`/`git rev-list` for import when git is present; (b) `gix` for read and write, a dependency the owner may reject under "from scratch" (§11 decision 4). Hand-writing the pack layer is a legitimate *later* milestone for "no git installed at all", which R2 asks of the core, not of the image.

**D11 (token/latency, minor) — `--across` costs 3× branch reads and un-promoted overlays are scanned.** [D §8]: "`ready --across` (3 refs) +3× branch cost + `TOUCH` bitmap AND — promoted branches only; un-promoted overlays are scanned (≤ 6k ops)". Fine at the owner's 3–5 live lanes; the brief should not default to `--across` over all 50 refs. Note only.

**D12 (ordering, minor) — the merge-queue step runs `git merge` before `moirai merge`.** [D §7.5 step 8]: "After `git merge u/l5np` in the code repo: `moirai merge lane/l5np --into main`". If the moirai merge stages a `DanglingEdge`, code is merged and state is not, and the brief's "staged merges first" line is the only signal. Run `moirai merge-check` and `moirai merge` (staging on violation) *before* the code merge; `--continue` after; the merge queue is already a script ([02 §5.3] P7), so the order costs nothing.

**D13 (spec hygiene, minor).** [D §5d.1] says a lease "on a node that is deleted on the holder's branch is released with a triage note" while [D §3 D7] says `rm` "refuse[s] while a live lease covers a node in scope unless `--release`" — the first case can only arise through a merge that deletes; say so. [D §6] "Node 40 end to end: §5d.3 within a branch = [A §6.5] steps 1–3" — but [A §6.5] step 1 uses drop-and-notify, which D replaced with X4's flagging; cite [D §5d.3] instead. Neither changes behaviour.

**Over-engineering / cut candidates for v1:** D7's list; the `merge/*` staging is worth keeping (structural violations must never land); `plan/*` collapses into `exp` (one read-only kind) if kept at all; `image show COMMIT` and `image doctor` are cheap and worth keeping.

**Fatal flaws:** none. D1 and D2 are wrong answers on the owner's daily dispatch path and must be fixed before the first lane branch; both fixes are small and stay inside D's own primitives.

**Score: 7.0.** The only R1–R3-compliant design, built as a disciplined delta with almost every earlier fix adopted; loses points for the two dispatch-loop holes (D1, D2), the `cwd`/`init` hazards (D3, D4), a hook that can mutate lane state (D5), orchestrator ergonomics under many lanes (D6), and a build that is ~1.5–1.7× A with an avoidable hand-written git layer and an overloaded M1 (D7–D10).

---

## 4. Full branching in the owner's harness: how agents pick, switch and merge moirai branches, and what it costs

This section answers the owner-update questions on this lens. The reference workflow is [02 §7.2] (fork a lane → rungs → declare footprint → sync → merge queue → post-merge repair) with three concurrent lanes [01 §5.4], ~11 Workflow runs a day, p50 4 agents per run [02 §5.1].

### 4.1 Who ever touches a branch verb

| Actor | Branch operations it performs | Under D | Recommended (this critique) |
|---|---|---|---|
| Orchestrator (main session) | `lane open` (= `branch` + `worktree bind` + lane node), `sync`, `merge-check`, `merge`, `resolve`, `merge --continue`, `branch -d`, `tag`, `image export` | CLI-only orchestrator rituals ([D §7.2]); `moirai-branches` skill preloaded ([D §7.3]) | same; plus `apply` derives its branch from the run (D6) and `sync` is gated on a clean preview (D5) |
| Workflow script | none (no shell) | passes `branch` in `args` ([D §7.5 step 6]) | same; the dispatch marker carries `branch=` and `lease=` |
| developer, tester, code-reviewer, results-analyst, project-analyst, doc-writer (Bash) | none; write on the branch resolved for them | directory binding ([D §5a.4]) | lease-resolved branch first, marker second, directory third (D3) |
| architect, architecture-critic, researcher (MCP) | none | stamped `ctx.cwd` on writes only ([D §7.2]) | explicit `branch` tool parameter validated against `lease` (D2) |
| Hooks | `SessionStart` prints the bound branch; `SubagentStart` may `sync` | ([D §7.3]) | `sync --check` only (D5) |

So agents never *pick* or *switch* branches; the orchestrator binds a lane's worktree once, and every call from that lane carries its branch through the lease or the marker. That is the same shape as today's HDR ("you are in `<lanes-dir>/l5np` on u/l5np, base `<sha>`") turned into data, and it is the cheapest possible UX for full branching.

### 4.2 The branch lifecycle per lane, priced

| Step | Command(s) | Durable commits | Wall time (est., from [D §5a.11]) | Who |
|---|---|---|---|---|
| Fork the lane | `moirai lane open l5np --worktree <lanes-dir>/l5np --git-branch u/l5np --base <sha>` | 3 | ~6–9 ms | orchestrator, once per lane |
| Dispatch a round | `ready --branch lane/l5np --ids`, `claim … --branch lane/l5np --ttl run` | 1 per claim | ms | orchestrator, per round |
| Work | `pack`, `add finding`, `complete`, `remember` — unchanged | as today | + overlay build 1–5 ms on the first read per process | the nine roles |
| Persist | `apply results.json --idempotency-key run:r7` (branch from the run) | 1 | ms | orchestrator, per run |
| Take rulings/rules from `main` | `sync --check` → `sync` | 1 merge commit | 5–40 ms | orchestrator (or a gated hook), ~daily per lane |
| Merge back | `merge-check`, `merge lane/l5np --into main`, `resolve …`, `merge --continue`, `branch -d` | 2–4 | 10–80 ms + resolution | orchestrator, once per lane, in the merge-queue script |
| Publish | `image export` (main + tags, checkpoint) | 1 (`gitmap`) | 0.1–2 s | orchestrator or a post-merge hook |

Roughly 6–12 extra CLI calls per lane over its life, plus one `--branch`/marker per lane-directed orchestrator call. Against the owner's ~11 runs/day this is noise in wall time; the cost is attention, which is why the marker and lease-resolution rules matter more than any millisecond.

### 4.3 Token cost of branching, per interaction

| Item | Delta | Where |
|---|---|---|
| First line of every CLI/MCP result: `branch: lane/l5np · rev 4471` | ≈ 8–10 tokens | all callers |
| Dispatch marker `moirai:task=#89 lease=L-18 branch=lane/l5np` | ≈ 6 tokens more than today's marker | every agent prompt |
| Pack C1 header: `branch`, `ahead/behind main`, `staged merges`; `~main` markers on unmerged critical rules | ≈ 30–80 tokens | every pack |
| Brief: one line per live lane with ahead/behind + staged merges + lanes with live leases | ≈ 60–150 tokens at 3–5 lanes | orchestrator, per session start |
| `--across` lines (`on main: done c9b1 (not merged into lane/l5np; run moirai sync)`) | ≈ 15 tokens per divergent key | only when asked |
| `moirai-branches` skill (lane open → sync → merge-check → merge → resolve → export) | ≈ 0.8–1.2k tokens | orchestrator only (preloaded) |
| Conflict-resolution vocabulary (`resolve 'edge:#203:blocks:#40' --take repoint:#52`, `'#91.body' --take theirs`) | 0 if printed verbatim in the merge output (D does) | orchestrator |

Under 3 % of a typical 8–12k-token pack; nothing reaches the nine roles except the header line and the marker. Prompt-cache note: per-branch packs vary by branch, but per-task packs already defeat cross-agent prefix sharing; C's class order (lane header first, rules second) keeps the shared-per-lane prefix stable, and D keeps that order.

### 4.4 What full branching changes about *correctness* of the daily queries (and the fixes)

| Query | On a shared trunk (A/C) | Under D as written | Under D + grafts 1–3 |
|---|---|---|---|
| `ready`/`claim --next` on `main` while lanes work | correct (status is global) | **wrong after `complete` on a lane releases the lease (D1)** | correct: `completed` runtime record excludes the node everywhere until merged |
| `blocking --ids` on `main` | correct | lists lane-completed blockers as live | correct with the record |
| `pack` for a critic in a Workflow (MCP, read) | correct | **`main` view; plan missing (D2)** | correct with explicit `branch`/`lease` |
| `complete` from a CLI whose cwd reset | correct | exit 5 or `--move-lease` onto `main` (D3) | correct: lease resolves the branch |
| owner ruling on `main` reaching a lane's developer | immediate | after `sync` (hook notice meanwhile); `~main` rule in packs | same; `sync --check` gated |
| cross-lane blocker (A on lane/x, B on lane/y) | immediate | after `lane/x → main → lane/y` or a direct cross-merge; `--across` explains | same (this is R1's intended semantics; the `completed` record additionally stops B's blocker from being re-dispatched) |
| two lanes supersede one rule | conflict at write (X12 on trunk) | `SupersedeFork` at merge ([D §5a.7]) | same |

The cross-lane blocker row is the honest cost of R1: liveness that the reports wanted "now" [08 §7.2] arrives at `sync`. D's `--across` and hook notices make it *visible*; only the owner's fallback "`shared` field class" ([D §2 T3′]) would make it *automatic*, and I agree with D that this is a decision to take after one campaign, not before.

### 4.5 Does full branching complicate the agent UX?

For the nine roles: no, provided D2/D3 are fixed — the branch is carried by the lease and the marker, the protocol is unchanged, and the only new thing they ever see is a header line and `~main` markers. For the orchestrator: yes, moderately — three rituals per lane, a merge that can stage, and a `resolve` vocabulary. That load is where it belongs (the orchestrator already runs the merge queue by script [02 §5.3] P7). For Workflow scripts: one more `args` field. For the owner reading the store: the `.moi` image (§5) is the first time the graph is browsable in a familiar tool.

---

## 5. The git image in the owner's harness (R3)

### 5.1 What the owner gets from it

1. **Backup.** `.moirai/` under the common dir is lost with the clone ([08 §7.3] G1 con); the image in a separate bare repo or under `refs/moirai/*` is the backup story, and it is why D puts the image (M4) before MCP (M5) ([D §9], adoption path). That ordering is defensible for the owner's unstable host [02 §9] — one OS-crash-induced loss of the graph would end adoption.
2. **Review in a familiar tool.** The owner's registers (`OPEN-QUESTIONS.md`, rulings, plans) are reviewed in git today [01 §7 L5]; `.moi` files with the body as the tail give `git log -- nodes/…` and `git diff` over prose ([D §5b.1]); B's `export md` (regenerated Markdown views) remains the *readable* projection for humans, and the two are complementary (§9 graft 10).
3. **A second machine or cloud agents later** without a moirai-specific sync protocol: `import` of native commits with verified `Moirai-Commit` trailers, foreign commits validated by the merge engine ([D §5b.6]).

### 5.2 What agents see

Nothing, by default: the image is written by the orchestrator (hook or ritual), never read by the nine roles. If an agent *does* open a `.moi` file, it meets 32-hex `uid`s in every edge line (~24 tokens each [06 §9.1]) and a `# 12` alias hint; D's reasons for uid-named files (kinds change, `#N` is store-local, determinism across stores, [D §5b.1]) are sound, and the price is paid only by humans reading raw diffs. Recommend the skill say "never grep the image; use `moirai show`".

### 5.3 The cheapest R3-compliant path (see D10)

| Component | D's plan | Cheapest R3-compliant v1 | Deferred |
|---|---|---|---|
| `.moi` encoder/decoder, canonicalisation, self-check re-parse | hand-written (M4) | same — this *is* R3 | — |
| Tree layout, commit metadata mapping, trailers, `gitmap`, `image_cursor` | hand-written | same | — |
| Object writer | hand-written loose/pack/idx/bundle; `--via fast-import` alternative | **`git fast-import` stream as the primary writer** (deterministic ids; marks = moirai commit ids [D §12 S3]); hand-written loose writer for ≤ 64 objects optional | pack/idx/bundle writer, `OFS_DELTA` (M7) |
| Object reader (import) | hand-written loose + pack + delta + `packed-refs` | **`git cat-file --batch` / `git rev-list` / `git diff-tree`** when git is present | pack reader (M7, for "no git installed" import) |
| Import semantics: native verification, foreign commits, `ImageParse`, staging | hand-written | same — this *is* R3 | — |
| Destinations | four ([D §5b.8]) | separate bare repo + `refs/moirai/*` | orphan branch, tracked directory |
| Granularity | `commit` default for separate repo | `checkpoint` per merge into `main` + daily; `commit` opt-in | — |
| Object format | follows destination | SHA-1 | SHA-256 (M7) |
| Refs exported | all | `main` + `tags/*`; lanes on request | — |

R2 asks that the *core* work without git; R3 asks that git be "a storage/transport target". Exporting into git with `git fast-import` when the owner has Git for Windows 2.54 installed [07 §1] satisfies both; the hand-written pack layer is the "no git at all" luxury that can wait for M7 (or be bought with `gix`, §11 decision 4).

### 5.4 Round trip and the merge engine

D's strongest R3 point is that git-side edits and merges are imported as *foreign* commits and validated by the same typed engine ("the store never adopts git's text merge as truth without validation", [D §5b.9]). Two consequences the skill and `doctor` must state: (a) a git-side merge of two exported lanes produces line-level merges of `.moi` files that will pass validation *and be wrong semantically* whenever the typed rule would have produced a conflict value (e.g. both sides moved `status` differently on separate lines — one line wins in git's merge; D's importer sees a clean file and one `SetField`) — the import should re-run the typed 3-way against the git parents' trees, not just diff against the first parent, when the git commit has two parents; (b) `id:` hints are honoured "when free", so two stores that both allocated `#12` for different nodes will renumber one on import — `show` prints the alias ([D §5b.6 step 5]) but any `#N` typed into prose on the other store now points at the wrong node; the sigil rule (§2.4) and `uid`-keyed edges limit the blast radius to text mentions.

---

## 6. Packs, Beads' failure history, token economy (condensed; D column added)

### 6.1 Do context packs replace hand-written briefs?

Partly, and only after the bootstrap import (§2.9). Mapping the HDR blocks measured in [02 §5.1] onto pack classes: lane location (38/38) → C1 from the lane node (now with branch, ahead/behind); standing rules (36/26/22/11) → rule nodes after import, `*` default; binding rulings (15) → `authority=owner` nodes, `~main` when unmerged; current defect state/pins → measurement nodes after import; do-not-touch → `owns`/`lane conflicts`; inputs by path → artifact nodes; output contract and task instruction → stay in the script by design. Three conditions decide adoption: completeness at launch (§2.9), no silent omissions (§2.5, §2.6 — both fixed in D), and paging rather than excerpting for 24 KB sections (C's L0/L1/L2 degrade, kept by D). Under branching add a fourth: the pack must say *which branch it was computed on and how far behind `main` that branch is* — D's C1 header does ([D §7.4]).

### 6.2 Beads' failure history: coverage matrix

| Beads failure [03 §8.2] | A | B | C | D |
|---|---|---|---|---|
| Dual source of truth (#3931/#380) | yes | yes | yes | yes: store canonical, image derived, `gitmap` derivable ([D §5b]) |
| Daemon lifecycle, split-brain (#4135, #1379) | yes | mostly (v1.5 leader) | leader in v1 | yes: leader M6, embedded contract |
| Server RAM/CPU (#4282, #3760) | yes | yes | yes | yes |
| Per-call cost 150–230 ms (#3760, #4102) | yes | yes | yes | yes (+1–5 ms branch overlay) |
| Lost acknowledged writes (#4767) | yes (M0 gates) | yes | yes | yes (X2/F-A1 in the spec) |
| **Phantom store in a worktree (#6551/#6552)** | no rule | yes | no rule | chain yes; **`init` shadowing (D4)** |
| Orphan rows / stale `is_blocked` (#4673, #6487, #6608) | yes | yes | yes | yes |
| Recursive-SQL cycle check 120 GB (#4475) | yes | yes | yes | yes |
| Sequential-id collisions | yes | yes | yes | yes (+ alias map on import) |
| Untested auto-migration (1.2.1) | yes | yes | yes | yes (schema as data, `migrate`) |
| Wide record / 13 types by accretion | yes | yes | **no (28)** | yes |
| MCP schema bloat | yes | yes | yes | yes (10 tools) |
| Agents forget to close (#6626) | SubagentStop net | same | same + I12 | same + run-scoped leases |
| Hierarchy in ids (PR #5131) | yes | yes | yes | yes |
| **New under R3: data in git branches diverging per worktree (Beads classic JSONL, [08 §7.1])** | n/a | n/a | n/a | avoided: the image is an export, never the working copy; tracked-directory destination is checkpoint-only and its git-side merges are imported as foreign |

### 6.3 Token economy: outputs and tool surfaces

All four print `#N` with titles, one line per record, ids first, deterministic order, explicit truncation footers [07 §6.2]; empty results exit 0. D adds exit 9 (idempotency payload mismatch) — good, distinct. MCP surfaces: A 9 / B 10 / C 9 / D 10 tools, compact text, no `structuredContent`, ≤ 2,048-char instructions; ≈ 1.2–1.5k tokens of schema on first `ToolSearch` [07 §5.1]. Skills: D preloads `moirai-branches` into the orchestrator only. Hooks: one spawn per boundary event (15–50 ms [07 §5.2]); the identity stamp is a `command` hook per stamped MCP call (verified, §10) — D's "writes only" keeps reads spawn-free at the price of D2. Unicode sigils: D uses ASCII. Branch deltas: §4.3.

---

## 7. Buildability

### 7.1 Relative effort per component (my estimate; A = 100)

Derived from the proposals' own LOC ranges (A ≈ 22–26k Rust + tests; B ≈ 18–25k + 8–12k; D ≈ 26–32k + 10–14k) and the reports' testing warnings [04 §11, §3.14; 08 §8.1]. Shares are of D's full build.

| Component | Share of D | Units (A = 100) | Notes |
|---|---|---|---|
| Engine core (HEAD/LOCK/log/segments/overlay/checkpoint/GC, Windows I/O, recovery, epoch, blocking lock wait) | 18 % | 25 | identical across proposals with [20]'s fixes |
| Deterministic simulator + crash/fsync/lock-delay injection + 16-writer kill loops + fuzzers | 18 % | 25 | D budgets 20 %; add the image's round-trip and parser fuzzing here (+3) |
| Graph semantics (13 kinds, fields, CSR, bitsets, derived state, I5′ PK, delete policies with X4, leases, idempotency, change feed) | 10 % | 15 | |
| VCS layer: refs, reflog, `ClientHead`, pins, branch overlays, `checkout`, `log`/`diff`/`show@`/`blame`, `undo`, `revert`, `tag`, GC with reflog expiry | 9 % | 14 | A's M4 was 4 units for trunk history verbs; branches add pins/overlays/heads |
| Merge engine, validators, `merge/*` staging, `resolve`/`--continue` rebase, `sync`, `merge-check`, `--across` | 9 % | 14 | B's M3 |
| **rebase, cherry-pick, op restore, branch promotion (`seg.b*`, `TOUCH`), `plan/*` kind** | 4 % | 6 | **deferrable (§7.2)** |
| `.moi` encoder/decoder, tree/commit mapping, `gitmap`, export/import semantics, foreign-commit validation, `image doctor`/`show` | 8 % | 12 | the irreducible R3 core |
| **Hand-written git object writer/reader (loose, pack v2 + idx, bundles, delta resolution, `packed-refs`) + differential tests** | 6 % | 9 | **deferrable: fast-import + cat-file first (D10), or `gix`** |
| CLI + output contract + `apply` + C's pack/brief + hooks + skills + `init`/`--link`/`worktree bind` | 10 % | 15 | |
| MCP (rmcp dual-era, stamp, role policy, packaging, `branch` tool) | 3 % | 5 | |
| Leader/pipe/group commit/`watch`, FTS tier 2, schema strengthening, `OFS_DELTA`, `--with-oplog` | 5 % | 8 | deferrable |
| **Total** | 100 % | **≈ 148–158** | D full; **≈ 118–128** for the §7.3 slice through S5 |

Two long poles instead of one: the multi-process protocol (every report's warning) and image determinism across two implementations of the same canonical rules (D §10 risk 3 names it; hg-git and cinnabar keep explicit maps for this reason [D §12 S8–S9]). The second pole is mostly *testing*: byte-identical `export → import → export` in SHA-1 (SHA-256 later), fuzzed `.moi` parsing against pathological bodies (block strings, `---` inside bodies, CRLF), and the two-parent import case of §5.4.

Consequences: (1) D's "26–32k + 10–14k" is plausible only for the slice; the full list with the pack layer, rebase/cherry-pick, promotion and leader is nearer 30–38k + 14–18k (est.); (2) D's M1 as written is ~40 units before any CLI (D8); (3) the image core (12 units) is smaller than the writer/reader it does not need (9 units).

### 7.2 What to cut or defer from v1 (D as the base)

Defer: `rebase --onto`, `cherry-pick`, `op log`/`op restore`, `plan/*` (keep one read-only kind name for later), branch promotion to delta segments (owner's lanes ≈ 1–3k ops; §9 graft 8 keeps overlays small), hand-written pack writer/reader and bundles (D10), SHA-256 images, `OFS_DELTA`, `--with-oplog`, tracked-directory and orphan-branch destinations, `refs/moirai/ops`, `ready --across` over all refs (keep `show`/`blockers --across`), the leader and `watch` (M6 already), FTS tier 2 (M7 already), HMAC `ctx` (dropped already).

Keep even though optional: `merge/*` staging (structural violations never land), `tag` (cheap, pins as-of), `undo` (ref-level, one commit), `sync --check`, `--across` on `show`/`blockers`, `image doctor`/`image show`, `doctor store|lanes|image|agents`, `export md` (B), `check #ID` (B), `notes --path` (B), `doc patch` (A), `stats loop`/`refuted-share` (C), `export memory-md` (C), `lane conflicts`/`merge-check` (C/B), run-scoped leases, the hook experiment, the import of standing rules/pins/lanes.

### 7.3 The smallest adoptable slice that already satisfies R1–R3

Ordered so that adoption on `main` precedes branches, branches precede the image, and the from-scratch engine can land behind the same trait without holding adoption hostage. Sizes: S ≈ a week, M ≈ 2–3 weeks, L ≈ 4–6 weeks of one owner plus agents (est.).

**S0 — contract, oracle, measurements (S).** On-disk format spec v1 (records with LSN + epoch, HEAD with refs/pins/heads/`gitmap` cursor), the engine trait (open/commit/replay/bitmap/CSR/overlay accessors, *branch view = pin ⊕ ops* in the interface even if the oracle implements only `main`), a throw-away backend on `redb` 4.x. Measurements on the owner's box with Defender on: open→append→flush→close of a 64 MiB file ([20] G8), loose-object create cost, blocking-lock contention with 16 writers, zstd dictionary ratio, Cyrillic token ratio, `git` presence on PATH, exec-form PATH resolution, and the 5-minute hook experiment [07 §8.6]. No user-facing verbs yet.

**S1 — trunk graph + CLI on `main` (M).** A's 13 kinds + C's `phase_state`/`return_to` + D's `gates` edge and schema-as-data; verbs `init` (with the D4 guard), `--link`, `worktree bind`, `add`, `set`, `link/unlink`, `move`, `doc patch`, `rm --dry-run/--yes` (X4 flagging, `--replaced-by` re-pointing), `apply` with `$refs` and payload-bound keys, `ready`, `blocking --ids` (tasks), `blockers --explain`, `show`, `tree`, `find`, `changes --since`, `claim` (run-scoped and TTL, store-wide, branch-carrying), `complete` (writing the `completed` runtime record, D1), `reopen`, `stats loop/refuted-share`, `lane conflicts`, `doctor store|agents`. Output contract and exit codes frozen (`--json v1`, exit 9 included). **R2 satisfied here** (discovery chain, no git in the core). Every verb already accepts `--branch`/`--lease` resolution (D3) even though only `main` exists.

**S2 — packs, brief, hooks, skill, import (M).** C's pack algorithm with D's C1 header, chars budgets, `*` default, global critical-rule count, `~main` rule slot (empty until S3); `brief`; `SessionStart` + `UserPromptSubmit` hooks; `export memory-md`; core skill + `moirai-report`; one-off import of the dozen standing rules, current pins, live lanes, open owner questions. **Adoption gate:** one real campaign on `main` with no HDR and no hand-written resume block; the dispatcher pattern is the only Workflow pattern; the orchestrator judges that no fact the HDR carried was missing from the packs (a diff of HDR vs pack, owner-judged — replaces C's unmeasurable precision@k).

**S3 — branches for the next campaign's lanes (L). R1 satisfied here.** Refs, reflog, `ClientHead`, pins, branch overlay (pin ⊕ trunk ops ⊕ branch ops, no promotion), `branch`/`checkout`/`--list`/`-d`/`tag`/`undo`/`log --graph`/`diff A...B`/`show@`/`blame`; `lane open`; typed 3-way merge with B's rules and [21]'s invariants, `merge/*` staging, `resolve`, `merge --continue` (rebase of resolutions), `sync --check`/`sync`, `merge-check`, `show`/`blockers --across`; `completed`/`deleted` runtime records honoured on every branch; `apply` branch from the run; `moirai-branches` skill. Gate: the owner's two register incidents replay correctly ([B §9 M3]); a lane completes a task and `main`'s `ready`/`blocking` never re-dispatch it before the merge (the D1 property test); 10 synthetic lanes × 1k ops with every conflict class merge deterministically.

**S4 — git image, checkpoint granularity (M). R3 satisfied here.** `.moi` encoder/decoder with self-check, tree layout, commit mapping and trailers, `gitmap`, export of `main` + `tags/*` at checkpoint granularity through `git fast-import` into a separate bare repo (SHA-1), `refs/moirai/*` as a second destination, import via `git cat-file --batch` with native verification, foreign commits and `ImageParse` staging, two-parent foreign merges re-run through the typed 3-way (§5.4), `image doctor --rebuild-map`, `image show`. Gate: `export → fresh import → export` byte-identical for 1e5 nodes; `git fsck` clean; a hand edit, a git-side merge with markers, a file deletion and a squash each produce the specified result ([D §9 M4]).

**S5 — MCP (S–M).** rmcp dual-era, C's nine tools + `branch`, explicit `branch` parameter on every tool with lease validation (D2), stamp on writes only, role policy on the dispatch label, packaging. Gate: architect and critic complete a round on a lane branch without Bash.

**S6 — the from-scratch engine behind the trait (L),** with the simulator and kill loops as its exit gate, swapped in when it beats the oracle on open time and private RSS [05 §16.1]; branch overlays and pins move with it. If the owner rules the oracle out entirely, S6 becomes S1's prerequisite and ~60 % of the work again precedes feedback — the ordering all three original proposals had.

**Later:** 1:1 image granularity, lane export, hand-written pack layer or `gix`, SHA-256, rebase/cherry-pick/op restore, promotion, leader/`watch`, FTS tier 2, schema strengthening, `shared` field class if one campaign shows chronic staleness.

Versus D's own plan: S1–S2 adopt before any branch code exists; S3 is D's M1b + M3 in one place with the four dispatch-loop fixes as gates; S4 is D's M4 minus the writer/reader; S5 is D's M5; S6 is D's M1a made swappable. R1–R3 are all met by the end of S4, on the oracle if need be.

---

## 8. Positions on the forks T1–T16 (this lens; T3′ replaces T3)

| Fork | Best position on this lens | Why (and what would change it) |
|---|---|---|
| **T1** materialized state | **D's** (B's segments + overlay with the [20]/[21] fixes as spec); v1 without branch promotion; branch view = pin ⊕ ops. | Fewest moving parts that support R1; promotion only matters for lanes > 8k ops, which §9 graft 8 avoids. Changes if a lane's first read exceeds ~20 ms at the owner's lane sizes. |
| **T2** process model | **A/D: purely embedded + blocking `LockFileEx` wait; the MCP server is an ordinary client; leader M6 or never.** | Zero idle CPU by construction [02 §9]; no push reaches the model [07 §2.3]; Beads' daemon history [03 §2.1]. Changes if M0 shows CLI commits > 20 ms from Defender close cost ([20] G8). |
| **T3′** branch model under R1 | **D's full branching with four amendments:** one moirai branch per *lane* opened by `lane open` (never per git branch or per worktree automatically); the branch of an agent call resolved from `--branch`/marker → lease → directory, never directory alone; `completed`/`deleted` store-level runtime records so no branch re-dispatches a lane's finished or deleted work; `sync` gated on a clean preview. `shared` field class only after one campaign shows chronic staleness. | R1 (owner update) overrides [08 §7.2]/[02 §7.3]; D1–D5 are the concrete holes; the amendments keep git-like isolation while making the two daily wrong answers impossible. Changes if the owner rules that status must be live across lanes without `sync` — then the `shared` class moves into S3. |
| **T4** IDs | **Both (D):** `#N` display + `uid` identity in canonical hashes and the image; alias map on import; mentions only to `N < next_id`. | R3 round trip needs store-independent identity; `#N` stays token-cheap [06 §9.1]. |
| **T5** deletion | **D** (hard delete + tombstone + X4 flagging/re-pointing + `rm` refuses under a live lease unless `--release`) + tombstone files in the image + a `deleted` runtime record visible across branches (§9 graft 1). | [21] X4 in A's own walkthrough; Beads' "imports cannot infer deletions" [08 §6.3]; D1's mirror image for deletes. |
| **T6** bodies | **A/B/D: 64 KiB inline, content-addressed, body as the tail of `.moi`.** | diff3, removed-text guard and `git diff` over prose need the text in-store and in the image; 16 KiB (C) is too small for 24 KB sections [01 §7 L12]. |
| **T7** merge semantics | **D/B:** structural violations staged on `merge/*`, value conflicts land unless `--strict`, `Incr` counters, `rev` above both sides, `supersedes` ≤ 1 active; git-side two-parent imports re-run the typed 3-way (§5.4). | [21] X10–X12; the owner's incidents were silent merges [02 §7.3]. |
| **T8** durability | **D** (one data-only flush, HEAD unflushed, lazy class, fsync fatal, zero-filled extents; ref moves and `ClientHead` durable). | [05 §2.2]; [20] G11. |
| **T9** agent surface | **C's pack + D's branch header + explicit `branch` tool parameter + lease-resolved branch + dispatch marker with `branch=`; stamp as a `command` hook on writes only (verified); CLI-only branch verbs; `moirai-branches` skill orchestrator-only.** | §2.10, D2, D3; [07 §5.4] roles; 20–73 ms per stamped call [20 §1.4]. Changes if a harness release lets `mcp_tool` hooks return `updatedInput` — then stamp everything for free. |
| **T10** "from scratch" boundary | **D's leaf list** (`zerocopy`, `blake3`, `xxhash-rust`, `zstd`, `windows-sys`/`libc`, `serde_json`; `sha1`/`sha2`/`miniz_oxide` in the image module; `rmcp`+`tokio` in `moirai mcp` only) **plus the image writer/reader as an owner decision:** `git fast-import`/`cat-file` first (no new dependency, git present on the owner's box), `gix` as an option (loose/pack/idx/ref writing verified done), hand-written pack layer last. Storage engines allowed as oracles and as the S0–S5 throw-away backend. | D10; [07 §1] Git for Windows 2.54 present; "keep the core small and owned" [03 §8.2] applies to the *core*, not to a git codec. |
| **T11** search | **A's tiers**, results carry `seq` and `branch`. | [05 §13], [03 §6.3]. |
| **T12** schema | **D:** 13 kinds + `phase_state`/`return_to` + `gates`, schema-as-data on every branch, `schema/*.moi` in the image; weakening merges, strengthening needs `migrate`. | [06 §4]; Beads accretion [03 §2.2]; 28 kinds rejected (§3.3 issue 1). |
| **T13** v1 scope | **§7.3's S0–S5** on the oracle, S6 the engine. | §2.7, D8. |
| **T14** git independence | **D's chain** (`--store`, `MOIRAI_DIR`, walk-up `.moirai` dir/pointer with store id, git hint read textually) **with the `init` guard and shadow detection (D4)**; `worktree bind`; no `git` spawned by the core. | R2; Beads phantom DB [03 §2.7]. |
| **T15** git image | **D's format** (`.moi` per node, uid 2×2 fan-out, out-edges only, tombstones as files, conflict lines, trailers, `gitmap`) **with cheap defaults:** separate bare repo, SHA-1, `main` + tags, checkpoint per merge + daily, `git fast-import` writer and `git cat-file` reader first; 1:1 commits, lanes, pack writer/reader, bundles, SHA-256 later. Lossless/lossy exactly as [D §5b.7], plus "intermediate commits between checkpoints" stated as lossy in the default mode. | D9, D10; [02 §9] disk; [04 §3.1] path-copy tax. |
| **T16** coordination under branching | **D's table** ([D §5d.1]) **plus two runtime records:** `completed {uid, branch, commit, holder}` and `deleted {uid, branch, commit}`, honoured by `ready`/`claim`/`blocking`/`brief` on every branch until the commit is an ancestor of the reader's tip; leases store-wide and branch-carrying; idempotency payload- and branch-bound with the branch derived from the run; change feed store-wide with `ref` per entry. Status/`done` versioned per branch and merged by lattice; `reopen` vs `done` = `StatusFork`. "Node 40 deleted on one branch": D's table ([D §5d.3]) plus the `deleted` record so no other branch dispatches #40 or lists it as a live blocker before it has received the delete. | D1; [07 §7.4] "two lanes must not both build #12"; R1's "justify runtime state" clause: both records are "who is doing / has done what now", not history, exactly like leases. |

---

## 9. Grafts the winner (D's skeleton) should adopt

1. **Store-level `completed` and `deleted` runtime records** (from this critique, D1/T16): written by `complete`/`rm`, honoured by `ready`, `claim`, `blocking`, `brief` on every branch, cleared by ancestry or turned into a triage line on branch delete. Property test: no branch can claim or list as a blocker a node another live branch completed or deleted.
2. **Lease-resolved branch and an explicit `branch` parameter** (D2/D3/§2.10): `--lease L` fixes the branch for CLI and MCP; every MCP tool takes `branch`, validated against the lease; the dispatch marker becomes `moirai:task=#89 lease=L-18 branch=lane/l5np role=developer`; `--move-lease` needs an explicit branch and warns; the server prints the resolved branch first (D already does).
3. **`init` guard and shadow detection** (D4): refuse `init` where a git hint resolves to an existing store unless `--force --shadow`; exit-7 hint names the hinted store and `init --link`; pointer files carry the store id; `doctor store` reports shadowing.
4. **`sync --check` gating for any hooked or automatic merge** (D5): auto-apply only clean previews; otherwise notify.
5. **`apply` derives its branch from the run** (D6): `run:<id>` → lane → `moirai_branch`; a batch is refused before any write when a lease's branch differs from the batch branch.
6. **Split M1** (D8) into engine (trunk) and VCS layer; adopt on `main` after S2; branches with the next campaign (S3).
7. **Image defaults** (D9): export `main` + `tags/*`, checkpoint per merge into `main` plus daily, separate bare repo, SHA-1; 1:1 and lanes opt-in; `git gc` guidance in the skill.
8. **`sync` that does not grow the overlay when the merge is clean and disjoint**: when a lane's ops since the fork touch no key that `main` touched since the fork, `sync` may re-fork the pin at `main`'s tip instead of appending `main`'s ops as a merge changeset (a rebase that changes no state and no `#N`; the lane's own commits keep their ids only if the canonical form excludes parents — it does not, so the image sees new commit ids for the lane, which is acceptable for unexported lanes and must be documented). This keeps lane overlays at ~1–3k ops for the life of a lane and defers branch promotion out of v1. Owner decision if lanes are exported 1:1.
9. **`git fast-import` as the primary image writer and `git cat-file --batch` as the primary reader** (D10, §5.3); hand-written pack layer or `gix` later (§11 decision 4); two-parent foreign commits re-run through the typed 3-way (§5.4).
10. **From B:** `notes --path`, `check #ID`, `export md` as a regenerated view next to the image, "developer never writes a verdict on its own task" and `owner_quote` checks in the engine (D keeps B's role table? D §7 delta does not say; make it explicit), `merge-check` before `git merge` in the merge-queue script (D12).
11. **From A:** `doc patch` with the removed-text guard on the CLI (D inherits B's merge-time guard; the write-time verb should exist too), the guard-conflict and `--explain` output formats.
12. **From C:** everything D already took, plus `resource` mutex nodes with lease semantics for the benchmark slot and the merge queue ([C §3.2], [07 §7.4] "merge slots").
13. **From revision 1 (already in D, keep them as gates, not options):** run-scoped leases, dispatch-label role policy, `--ids` header-free, `blocking` tasks only, sigil rule, chars budgets, `*` default, global critical-rule count, payload-bound keys, simulator as its own milestone, hook experiment in M0, import of rules/pins/lanes in S2.

---

## 10. Claims that are wrong or unverified

| Where | Claim | Status |
|---|---|---|
| **W-all-1** (verified today) — A §7.4, B §7.3, C §7.4, D §7.2 | The identity stamp on `mcp__moirai__*` can be cheap / can be an `mcp_tool` hook | The hooks reference (code.claude.com/docs/en/hooks, fetched 2026-09-25) documents five hook types; for `mcp_tool` hooks "Claude Code reads the tool's text content the same way it reads command-hook stdout" and lists no `updatedInput`/`permissionDecision`; the stamp must therefore stay a `command` hook (a 20–73 ms spawn per stamped call, [20 §1.4]). D's "writes only" is right; D's "every tool resolves the branch from the stamped `ctx.cwd`" is therefore false for reads (D2). |
| **W-all-2** (re-verified today) | `SubagentStart`/`SubagentStop`/`PostToolUse(Agent)` fire for Workflow `agent()` calls | The hooks reference says nothing about Workflow scripts; still unverified [07 §8.6]; D schedules the experiment in M0. |
| W-all-3 | exec-form hooks resolve `moirai` via PATH on Windows | Unverified [07 §9.5]; C/D mention the absolute-path fallback; S0 measures it. |
| W-all-4 | zstd dictionary ratio ≈ 3× on ~1 KB notes | Claimed, unmeasured [05 §12]; S0 measures it. |
| W-all-5 | rmcp server private RSS 4–16 MB | Extrapolated from an HTTP benchmark at 10.9 MB RSS [07 §2.7]; stdio + `current_thread` unmeasured; D adopts G12 and a ≤ 10 MB gate. |
| W-all-6 | tokens ≈ chars / 3.5 | Wrong for Cyrillic by ~2× (my estimate, unmeasured); D budgets in chars and measures ratios in M0. |
| **W-D1** — D §10 risk 1, §5d.1 | "leases are store-wide so double work is impossible even when views diverge" | **Wrong** once `complete` releases the lease: the task is `open` and unleased on `main` and re-enters `ready` there (D1). |
| **W-D2** — D §7.2 | "Every tool resolves the branch from the stamped `ctx.cwd` binding" together with "The stamp hook matches write tools only (G6)" | **Contradictory** for reads (D2); see W-all-1. |
| **W-D3** — D §5c, §7.1 | "a miss never creates a store" as the phantom-store defence | True for discovery; `init` in a worktree creates a shadowing store one hint away (D4). |
| **W-D4** — D §10 risk 1, §7.3 | `sync` "can be hooked to `SubagentStart`" | Unsafe as stated: the hook may land conflict values on the lane (D5). |
| **W-D5** — D §6 | `apply` batches "land on the branch named in the batch (default: the orchestrator's client branch)" | Wrong default for lane results (D6); derive from the run. |
| **W-D6** — D §2 T10, §12 S5 | "gix has pack writing and ref transactions but no push, and SHA-256 is listed as parity work" | **Verified accurate** (crate-status.md, fetched 2026-09-25: loose-object and pack + index writing, ref transactions and packed-refs writing done; push under "needs plumbing"; "Git 3.0 compatibility (SHA-256, reftable)" as cross-cutting work; no fast-import). The inference "therefore hand-write the writer/reader" does not follow: transport is via the git CLI in D's own design and SHA-1 is the default (D10). |
| W-D7 — D §5b.10 | Loose-object create on NTFS with Defender ≈ 1–3 ms each | Est., "pending M0" by D's own label; decides the loose/pack threshold. |
| W-D8 — D §5b.10, §8 | ~1k moirai commits/day → ~4 GB/year of undeltified image at 1:1 | Rests on B's ~1k/day estimate (11 runs × p50 4 agents × ~20 writes, [B §5.1]); [02 §12.6] gives tens of writes/s at peak and ≈ 0 steady; plausible, unmeasured; the checkpoint default (D9) makes it moot. |
| W-D9 — D §5a.3, §5a.11 | 50 live branches pin only 2–4 base files | Assumes forks within one rollup interval; stale `wf_*`/worktree bindings that are never `branch -d`'d pin more; `doctor` warns, `branch -d` releases; fine if `lane open` is per lane (3–5 live), not per worktree (44). |
| W-D10 — D §9 | 26–32k lines of Rust + 10–14k of tests | Est.; plausible for the §7.3 slice; the full list (pack layer, rebase/cherry-pick, promotion, leader) is nearer 30–38k + 14–18k (my est., §7.1). |
| W-D11 — D §5a.7 step 2, §5a.11 | merge of a 2k-op lane vs 5k trunk ops in 5–20 ms at 1e4 | Fixed [21 §3.2 item 10]'s chain-read objection by sequential folds; still an estimate; the M3 gate (≤ 50 ms at 1e5) will measure it. |
| W-D12 — D §5b.9 | git-side merges of `.moi` files are "re-validated (typed invariants, not text, decide)" | Partly: the importer diffs against the *first* parent, so a clean git text merge of two divergent typed edits is imported as one `SetField` with no conflict record (§5.4); needs the two-parent re-merge. |
| W-D13 — D §12 S1 | jj: "Commits with conflicts cannot be represented in Git"; change ids in non-standard headers | Cited from jj's own docs by D; not re-verified here; used only as a contrast. |
| W-D14 — D §5b.4 | `<role>@moirai.invalid` is a reserved TLD | True (RFC 2606). |
| **Revision-1 entries, with D's status:** | | |
| A §2 T10 | "`fst` is unmaintained since 2021" | Overstated: [05 §13] says "mature but last released in 2021". D does not use `fst`. |
| A §7.2, B §7.2, C §7.2 | "Claude Code forwards only `structuredContent` when both are present" stated as fact | GitHub issues (#55677 closed not-planned, #79944 open) [07 §2.6]; behaviour to test per release; the text-first decision is right regardless (D keeps it). |
| A §7.1 example | `blocking --ids` prints a header | Defect; **D fixed**. |
| A §4.10 | rollup inside an MCP request at 0.1–0.3 s at 1e5 | Acceptable if stated; D moves rollups to explicit `gc`/MCP-after-request ([20] G9). |
| B §7.1 example | `blocking --ids` lists `#40` while `#40` is deleted | Inconsistent; D's examples are consistent. |
| B §6.1 | `moirai watch` "sleeps 250 ms between HEAD reads: ~0 CPU" | Violates [02 §9]; **D fixed** (leader-only). |
| B §2 T3 (decision 3) | "the register-merge incidents show branch-scoped knowledge is real" | The incidents [01 §7 L5–L6] are prose union-merges of status fields; they show the need for a status lattice, not for branch scoping. Under R1 branch scoping is required anyway, so the point is moot for the decision but the citation is still wrong. |
| C §8 | history 6 / 60 / 600 MB from "10 ops × 60 B" | Undercounts ~7× against C's own commit size; A's 40 MB at 1e4 is consistent; D uses B's 0.3 KB/commit. |
| C §3.4, §4.3, T10 | "roaring bitmaps, frozen (queryable in place)" with the pure-Rust crate | Wrong per [05 §10.3]; **D uses A's frozen containers**. |
| C §7.6 | "refuter" and "project-analyst" rows keyed on stamped `agent_type` | "refuter" is not an agent type [01 §1]; **D keys on the dispatch label**. |
| C M3 gate | precision@k ≥ 0.8 on 20 replayed historic briefs | Unmeasurable under C's start-clean path; D has no replay gate; §7.3 S2 substitutes an owner-judged HDR-vs-pack diff. |
| C §7.1 example | `set #12 --status done --lease L-9` answered with "you sent `--if-rev 5`" | Example does not match the command. |
| A, B, C, D (via [04 §3.14]) | "95 % of the effort is testing" | A lobste.rs thread; the direction is right, the number is folklore. |

---

## 11. Owner decisions that would change this verdict

1. **Accept the four dispatch-loop amendments (grafts 1–4) as part of R1?** Without them, full branching re-dispatches lane-completed work from `main` (D1) and breaks the Bash-less critique loop (D2); with them, D is the design to build. If the owner instead wants status live across lanes without `sync`, D's `shared` field class moves into S3 and T3′ becomes "full branching for knowledge, shared status" — closer to B.
2. **One moirai branch per lane (`lane open`) or automatic branches per git worktree/branch?** Per lane is recommended: 44 worktrees and 107 branches [08 §2], many detached or harness-created, would pin segment sets and produce merge rituals nobody asked for. `wf_*` and scratch worktrees write to `main` ([D §11 decision 3]).
3. **Stamp every MCP call (identity + branch for reads, +20–73 ms per call) or an explicit `branch` parameter typed from the dispatch marker (free, model-typed, validated against the lease)?** Recommended: the parameter, until a harness release lets `mcp_tool` hooks rewrite input.
4. **Image codec: `git fast-import`/`cat-file` (no new dependency, needs git present), `gix` (large dependency, pack/idx/ref writing already done), or a hand-written pack layer (D's plan, 4–6k lines + fuzzing)?** Recommended: fast-import first; hand-write or adopt `gix` in M7 only if "no git installed" import/export is ever needed.
5. **Image defaults: separate bare repo at checkpoint granularity for `main` + tags (recommended) versus 1:1 commits for all branches?** Decides disk (0.2 vs 0.5–4 GB/year est.) and whether `git gc` becomes a ritual.
6. **May `moirai image push/pull` spawn `git` when present?** D says yes with a printed fallback; recommended yes.
7. **May S0–S5 run on a throw-away oracle backend behind the engine trait (branch views included in the interface), with the from-scratch engine landing in S6?** If no, ~60 % of the build precedes feedback (§2.7).
8. **Which R1 "ideally" verbs are v1?** Recommended: `tag`, `undo`, `revert` yes; `rebase`, `cherry-pick`, `op restore` deferred (D7).
9. **`sync` policy: manual, hooked-when-clean (recommended), or always hooked?** Decides whether a hook may ever change lane state (D5).
10. **Graft 8 (clean `sync` re-forks the pin instead of appending a merge changeset)?** Keeps lane overlays small and defers promotion, at the cost of rewritten lane commit ids in a 1:1 image; acceptable if lanes are exported at checkpoint granularity or not at all.

*End of critique 22, revision 2.*
