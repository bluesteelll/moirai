# 02 — The BoykoEngine agentic workflow, orchestration layer

*Research for moirai. Written 2026-09-25. Read-only study; nothing in the studied repository or harness
directories was modified.*

**Scope.** This report covers only the **agentic workflow** of the BoykoEngine repository: how agents are
defined, orchestrated, handed work, gated and parallelised, and how state and knowledge move between
workflow phases, runs and sessions. The engine itself (crates, rendering, physics, content, backlog) is
**out of scope**. Where a workflow file uses an engine example it has been reduced to the workflow
pattern. Engine identifiers such as lane names or rung ids are reduced to their shape (`<lane>`, `C<n>`)
in this published version; the private Workflow scripts are described, not named.

**Lens.** The orchestration layer. The question is what moirai must hold to become the single source of
truth for orchestration state plus project knowledge, and what it must not hold.

---

## 0. Method and evidence legend

| Tag | Meaning |
|---|---|
| **[M]** | **Measured** by me in this session with a command over the files named (counts, sizes, greps). The numbers can be reproduced. |
| **[C]** | **Claimed** in the studied files (CLAUDE.md, memory files, script comments). I did not re-measure it; it is reported as the orchestrator or owner recorded it. |
| **[I]** | **Inferred** by me from the evidence. It is a reasoned interpretation, not a measurement. |
| **[D]** | External documentation, cited with URL and version/date. |

**Material read**

- The BoykoEngine repository's `CLAUDE.md` (33,212 B [M]): only the sections *Documentation — two layers* (l.125), *Agents* (l.130–207), *Orchestration discipline* (l.209–214), *Communication* (l.216–224), *Rules for agents → Separation of duties, Git* (l.257–266), *Code navigation* (l.277–388) and *graphify* (l.390–398), each read as agent tooling.
- `.claude/settings.json`, `.claude/settings.local.json`, `.claude/hooks/{clarify_gate,graphify_bash_gate,graphify_read_gate}.py`, `.zcode/config.json`, and the frontmatter plus the workflow sections of the 9 files in `.claude/agents/`.
- **Workflow scripts: 38 in total, 392,819 bytes [M].** 34 of them sit under the harness's encoded project directories (21 distinct directories, including the one encoded from the memory folder). The other 4 are parameterized scripts in the most recent session's scratchpad (a trunk-merge template, a merge-queue driver, and two implementation/lane scripts). I read 19 in full or in their control flow: design-survey and plan-revision scripts, implementation and fix scripts, verify-and-close scripts, citation-repair passes, doc-fix scripts, a quiet-window preparation script, critic scripts, a judge-panel round, the trunk-merge template and the merge-queue driver. For all 38 I computed pattern-prevalence statistics (§5.1). The scripts are private and not published.
- The harness's own run records under `~/.claude/projects/<project>/*/subagents/workflows/wf_*/`: `journal.jsonl` plus `agent-*.jsonl` and `*.meta.json`, measured for **structure and size only**.
- Cross-session memory `~/.claude/projects/<project>/memory/`, **mechanism only**: the format of `MEMORY.md`, the frontmatter schema, counts and sizes, the link graph, and 17 workflow-practice and staleness topic files (6 working agreements, 11 hazards) read in full (summarised in §10.4).
- `git worktree list` and `git branch --list` on the main checkout (read-only).
- External documentation (§15): Claude Code memory, hooks and sub-agent docs, `git-worktree(1)`, the MCP resources spec, and the harness's own `workflow-authoring` reference as loaded in this session.

---

## 1. Executive summary

1. **The orchestrator is a human-grade project manager running on top of files.** One main session, the
   "orchestrator", decomposes owner goals into **campaigns → phases → lanes (one git worktree + branch
   each) → rungs/steps (commit-sized) → rounds/passes → findings**. It launches Workflow scripts, about 11
   per day in September [M]. Each script is a deterministic JS state machine over role agents (developer,
   tester, code-reviewer, architect, architecture-critic, researcher, writer…). All durable state is spread
   over **five unconnected stores**:
   - the **harness run journal** (`journal.jsonl`, keyed by content hash, resumable only in the same session [C]);
   - **session scratchpads** (reports, rulings, sha256 manifests, patches; up to 7.8 GB per session [M]);
   - **git** (worktrees, branches, commits: 44 worktrees and 107 local branches [M]);
   - **auto-memory** (`MEMORY.md` plus 254 topic files, 2.45 MB [M]);
   - **in-repo docs/registers** (designs, `00-RULINGS.md`, `OPEN-QUESTIONS.md`, `MEASUREMENT-QUEUE.md`, ledgers).
2. **Nothing links these stores except absolute paths and ids pasted into prose.** A single memory
   topic file holds **100 background-task ids, 78 workflow-run ids and 51 branch names in 141 KB, with no
   headings [M]**. The resume block at the top of `MEMORY.md` is **8 stacked checkpoint lines, of which
   only the newest is current [M]**.
3. **The dominant failure class is "a stale or partial record that reads as current".** The memory
   records these cases:
   - retractions that do not reach summaries [C];
   - union merges that bring back pre-ruling "OPEN" statuses in 10 places [C];
   - identical counter edits auto-merged into one [C];
   - silent truncation of agent inputs and outputs, four recorded incidents [C];
   - shell-mangled memory writes [C];
   - `file:line` citations rotting as code moves (an 8-pass repair campaign [M from scripts]);
   - "0 errors" workflows whose promised files do not exist [C].

   These are exactly the failures a **typed, versioned graph with tombstones, supersession edges and
   provenance** removes by construction.
4. **moirai therefore needs:**
   - (a) orchestration node kinds (Campaign, Phase, Lane, Rung, Run, Report, Finding, Ruling, Gate/Pin, OwnerQuestion, MeasurementItem) next to knowledge kinds (Rule, Hazard, Decision, Note);
   - (b) edges for decomposition, blocking, supersession/retraction, evidence/provenance, "touches file set" and merge order;
   - (c) **branches that mirror git lanes**, with a merge whose semantics are *not* textual union: ruled beats open, counters add deltas, superseded records lose authority but stay;
   - (d) three canned queries — **session-start brief**, **phase context pack**, **merge-time check** — each bounded to a few thousand tokens;
   - (e) zero background CPU, because the owner's "quiet window" rule forbids any agent activity during timed runs [C].
5. **What must stay out of moirai:**
   - agent transcripts (2.2 GB of run dirs [M]) and scratchpad bulk (GBs [M]);
   - code-symbol indexing (LSP, graphify and semble already do this);
   - build artifacts and golden images;
   - gate execution itself;
   - LLM-driven rewriting of stored facts.

---

## 2. How agents are defined

### 2.1 Roster and tool envelopes [M]

The nine role files in `.claude/agents/` range from 11.5 KB to 23.6 KB. `.zcode/agents/` is a
**byte-identical copy** (`diff -rq` prints nothing) kept for a second client, and `.zcode/config.json`
duplicates the hook wiring. That makes two copies of each definition with no link between them. [M]

| Role | `tools:` (frontmatter) | `model:` | Can write files? | Output contract (section in the role file) |
|---|---|---|---|---|
| `architect` | Read, Glob, Grep, WebSearch, WebFetch, **Agent** | opus | **no** | Plan with fixed headings; from rev 2 on, a **PATCH**: removed text quoted verbatim + added text + dependent sections |
| `architecture-critic` | Read, Glob, Grep, WebSearch, WebFetch | opus | **no** | Verdict + 🔴 Critical / 🟡 Important / 🟢 Optional remarks with ids C1/W1/O1 |
| `developer` | Read, Write, Edit, Glob, Grep, Bash | opus | yes | "Implementation" report: modified/new files, conformance to plan, unsafe list, checks, "Ready for code review" |
| `code-reviewer` | Read, Glob, Grep, Bash, WebSearch, WebFetch | opus | no | Verdict + C/W/O remarks with `file:line` |
| `tester` | Read, Write, Edit, Glob, Grep, Bash | opus | yes | Build + test + bench report |
| `results-analyst` | Read, Glob, Grep, Bash, WebSearch, WebFetch | opus | no | ACCEPTED / REWORK (names the phase to return to) / RETHINK |
| `project-analyst` | Read, Glob, Grep, Bash, WebSearch, WebFetch | opus | no | Q&A / audit / bug hunt / perf report |
| `researcher` | WebSearch, WebFetch, Read, Glob, Grep | opus | no | Research summary with sources |
| `doc-writer` | Read, Write, Edit, Glob, Grep, Bash, WebFetch | opus | yes | Public docs pages |

- **Separation of duties** (CLAUDE.md l.257–261) [C]:
  - developer does not run tests;
  - reviewer does not fix;
  - critic does not dictate design;
  - analyst does not edit.

  The workflow scripts enforce this through role-scoped prompts, and they add a "writer" step because
  the design roles cannot write (below).
- **Model routing** (CLAUDE.md l.148–207) [C]: every role runs on Opus, an owner decision justified by two
  in-repo measurements. One is a 27-question retrieval test with the "confidently wrong" column: Sonnet 4,
  Opus 0. The other is a before/after of the repository's own commit history: "self-refutations" 6.45×
  higher in the Opus era, confounder named. **The stated tuning knobs are effort, batch size and
  redundant arms, not the model tier.** `effort:` appears in 10 of 38 scripts [M].
- **Gotchas the orchestrator recorded about agent definitions** [C, owner's private notes]:
  - `architect`, `architecture-critic`, `project-analyst` and `researcher` have no Write/Edit. Asked to write a plan file under `docs/`, they return the text and the file never appears. The next phase then finds no plan while the workflow reports 4 agents done with 0 errors. Later scripts route file-writing to a "WRITER" step with no `agentType`, or to `agentType: 'claude'` (5 of 38 scripts [M]).
  - `results-analyst` is listed in CLAUDE.md, but the orchestrator recorded `agent type not found` when a Workflow used it [C, owner's private notes]. The file exists in `.claude/agents/` today [M], so this record is itself possibly stale. That is an example of the failure class in §11.

**[I] Implication.** Role definitions are knowledge with provenance: tool envelope, model, output contract.
The orchestrator needs to *query* them ("which roles can write?"). A graph node per role, with a
`tools` field and a `defined_in` edge to the file and its hash, would have prevented both gotchas.

### 2.2 How agents are invoked [M, D]

- Agents are spawned **only from Workflow scripts or the Agent tool**. In scripts the call is
  `agent(prompt, {label, phase, agentType, schema, effort})`.
- Per the harness reference:
  - `agent()` returns the final text, or a schema-validated object;
  - it returns `null` when the agent dies;
  - `parallel()` is a barrier;
  - `workflow({scriptPath}, args)` nests exactly one level;
  - concurrency is capped at min(16, CPUs−2);
  - `Date.now()` and `Math.random()` are banned so resume stays deterministic.

  (Source: Claude Code `workflow-authoring` reference, loaded in this session on 2026-09-25; no public URL.) [D]
- Sub-agents can also declare `memory: user|project|local`, which gives each a persistent directory with
  its own `MEMORY.md`, and `isolation: worktree`
  (<https://code.claude.com/docs/en/sub-agents>, fetched 2026-09-25) [D]. **None of the nine BoykoEngine
  roles uses either field [M].** State is instead passed explicitly by the orchestrator.

---

## 3. Orchestrator responsibilities

Assembled from CLAUDE.md (l.146, 209–214, 257–266), the scripts, and the working-agreement memories.
Tags show where each comes from.

| # | Responsibility | Evidence |
|---|---|---|
| O1 | **Clarify scope before acting**; ask the owner only VALUES/SCOPE questions; decide performance and architecture forks itself "with numbers" | CLAUDE.md l.211 [C]; scripts separate VALUES/SCOPE questions for the owner from decisions (an item in one design-survey script's acceptance bar) [M] |
| O2 | **Plan-Mode threshold**: ≥3 files, or not describable in one sentence | CLAUDE.md l.212 [C] |
| O3 | **Decompose** goals into lanes/rungs; **partition files** so that parallel developers never touch the same file | `developer.md` "Parallel work" [M]; owner's private notes [C] |
| O4 | **One worktree + branch per system/lane**, outside the repo (`<lanes-dir>/<system>`) | owner's private notes [C]; 44 worktrees [M] |
| O5 | **Write the brief (HDR)** for each run: tree, branch, HEAD, base, toolchain prefix, trees not to touch, git bans, disk/crash rules, evidence rules, the output contract | HDR constants in 38/38 scripts [M] |
| O6 | **Issue binding rulings** on critic/review findings (`R<n>` ids, per-rung `*_rulings.md` files, `00-RULINGS.md`) that agents must implement rather than re-open | 15/38 scripts mention rulings [M] |
| O7 | **Run review loops until confirmed blockers are empty**, with an adversarial *refutation* step between critic and fixer, and delta-scoped re-review | owner's private notes [C]; triage steps in 6/38 [M] |
| O8 | **Merge lanes into the trunk one at a time**, each gated on its own tester GREEN; decide the merge order | a merge-queue script and a trunk-merge template [M]; MEMORY.md resume lines record the one-by-one merge order [M] |
| O9 | **Commit/push** only per owner rule. The authorship rule: no AI co-author marker | CLAUDE.md l.264–266 [C]; 11/38 scripts repeat it [M] |
| O10 | **Manage the machine**: ≤3 build lanes at a time, disk ≥15 GB, per-lane `CARGO_TARGET_DIR`, stop everything in quiet windows, no machine-wide kills | a quiet-window preparation script; owner's private notes [C/M] |
| O11 | **Persist the resume state** (the MEMORY.md resume block plus the campaign topic file), and recreate the cron watchdog after a session change | MEMORY.md resume block [M] |
| O12 | **Record lessons** as `feedback-*` (owner working agreements) and `reference-*` (measured hazards) | 48 + 104 files [M] |

---

## 4. Harness-level gates and hooks

### 4.1 Hooks [M]

| Hook | Event / matcher | Behaviour | Blocks? |
|---|---|---|---|
| `clarify_gate.py` | `UserPromptSubmit` | Injects a reminder as `additionalContext` iff the prompt contains an EN/RU imperative verb **and** has no file/path/symbol locator **and** is shorter than 500 chars | never (reminder only; fail-open on any exception) |
| `graphify_bash_gate.py` | `PreToolUse` / `Bash` | If `graphify-out/graph.json` exists and any pipeline stage's *leading* command is a search tool (grep, rg, find, …), injects "graphify first; fall back once" | never |
| `graphify_read_gate.py` | `PreToolUse` / `Read\|Glob` | Same nudge for source-file reads; excludes `.claude/`, `graphify-out/`, `target/`, `book/`, `.git/` | never |

- All three hooks are **nudges**. They use the documented `hookSpecificOutput.additionalContext` channel
  (<https://code.claude.com/docs/en/hooks>, fetched 2026-09-25) [D].
- The documented event set also includes `SessionStart` (which can inject `additionalContext`),
  `SubagentStart/Stop`, `TaskCreated/Completed`, `WorktreeCreate/Remove`, `PreCompact/PostCompact`
  and `SessionEnd` [D]. **None of these is wired in the project's `.claude/settings.json` or
  `.zcode/config.json` [M].** I did not inspect user-level settings. Resume is manual: the orchestrator
  reads MEMORY.md.
- `settings.local.json` holds 4 ad-hoc one-off Bash allow-rules [M]. Permission state accretes the same
  way memory does.

**[I] Implication.** moirai can connect to the harness with no protocol invention:
- `SessionStart` → inject the session-start brief;
- `WorktreeCreate/Remove` → fork or close a lane branch;
- `SubagentStop`/`TaskCompleted` → record run results;
- `PreCompact` → checkpoint.

A hook must **fail open** like the existing ones, so that a moirai outage never blocks the owner.

### 4.2 In-script gates (the orchestration's real control flow) [M]

Scripts turn free-text agent output into control decisions with four conventions:

| Convention | Prevalence (of 38) | Example |
|---|---|---|
| **First-line verdict**: `GREEN`/`RED`, `DONE`/`STOPPED`, `FIX WRITTEN: …`, `PREP READY: …` | 12 | a regex test for `GREEN` at the start of the verdict line |
| **Header counts**: `VERDICT: APPROVED \| CHANGES_REQUESTED; CRITICAL=<n>; IMPORTANT=<n>` parsed by regex | 16 | a regex that extracts the two counts from the header line |
| **Structured output** (`schema:`) with enums (`APPROVED`/`CHANGES_REQUESTED`; `CONFIRMED`/`PARTIAL`/`DOWNGRADED`/`REFUTED`; `adopt`/`adapt`/`reject`) | 18 | a verdict schema with an enum, a blocker list and a report field |
| **Harness-notice stripping**: skip lines starting with `[harness` before reading the verdict | 10 (every multi-agent script dated 2026-09-21 or later; the only later script without it, a quiet-window preparation script, has a single agent) | a verdict-extraction helper, copy-pasted |

Other gate behaviour:

- **Death handling.** Every `agent()` result is null-checked, and the run returns `{status: '<stage>-died'}`,
  e.g. `repair-died`, `tests-blocked`, `review-not-approved`, `verify-not-green`. Measured over the
  harness journals: **162 of 3,294 started agents failed (4.9%)** [M].
- **Bounded loops.** Some loops are capped: test rounds ≤2, review passes ≤3, critic passes ≤5. Owner rule:
  **no round cap**; the stop condition is that no confirmed blockers remain after refutation [C,
  owner's private notes]. The scripts meet it half-way: caps of 3–5 plus a triage agent that
  labels every finding `CONFIRMED | REFUTED | DEFERRED-OUT-OF-SCOPE` and outputs `CONFIRMED=<n>` [M].
- **Conditional stages.** A pre-registered measurement decides whether a later implementation stage runs:
  one implementation script runs its second stage only if Step 0's band is CONFIRMED or PARTIAL [M].
- **Sequential gating across nested workflows.** The merge queue runs the second merge only if the first
  child workflow's tester returned GREEN [M].

**[I] Implication.** Verdicts, findings and their status transitions are the most frequently produced
**typed** facts in this workflow, yet they travel as regex-parsed text. moirai should give MCP tools that
*write* them as typed nodes, e.g. `finding.add` or `verdict.record`, so the next phase *queries* them
instead of re-parsing prose.

---

## 5. Workflow script anatomy

### 5.1 Corpus measurements [M]

- **38 scripts, 392,819 B.** Dated 2026-08-28 … 2026-09-25 (the ones that survived on disk).
- **Harness journals: 401 runs in 17 sessions.** By run-directory month: 6 in June, 6 in July, 122 in
  August, 267 in September (to the 25th), i.e. **about 11 runs per day in September**.
- **Agents per run:** p50 4, p90 14, max 108. **3,294 agent starts, 2,985 results, 162 failures.**
- **Result payload:** p50 8,825 B, p90 38,254 B, max 153,311 B. All journals together: 46.8 MB; largest
  single journal 2.04 MB.
- **Run directories** including agent transcripts: 2.2 GB. Top-level session transcripts: 39 files,
  420.7 MB.
- **Where the scripts live.** The 34 historic scripts are scattered over **21** encoded project
  directories. The directory name comes from the *current working directory* of the session that launched
  them, e.g. `…-memory` and `…-subagents-workflows-wf-<previous run>`. In one chain, a design script
  was stored under its predecessor run's directory, and the implementation script that followed was
  stored under the design run's directory; this suggests the orchestrator's cwd had drifted into the
  previous run's transcript directory [I]. Finding "the script that produced X" therefore needs a filesystem search.

**Prevalence of standing-rule boilerplate repeated in HDR blocks** (scripts containing the pattern, of 38):

| Pattern | Count | What it encodes |
|---|---|---|
| "English" | 36 | language rule for repository artifacts |
| git bans (`stash`, `checkout --`, `--force`, …) | 26 | destructive-git ban |
| `RUSTFLAGS` (never set) | 22 | toolchain rule |
| `schema:` | 18 | structured verdicts |
| `VERDICT` | 16 | review verdict contract |
| `ruling`, `RULINGS` | 15, 9 | binding orchestrator/owner rulings passed by path |
| `===== BEGIN =====` / `END` | 15 | full upstream report inlined between markers (anti-truncation) |
| "First line" contract | 12 | parsable verdict line |
| `Co-Authored-By` ban | 11 | authorship rule |
| "SAME command" (cd per call) | 11 | Bash-tool cwd reset hazard |
| `sha256` / `md5` | 11 / 5 | restore proofs, tree manifests |
| `effort:` | 10 | per-call effort knob |
| `CONFIRMED` / `triage` | 9 / 6 | adversarial refutation of findings |
| machine-fault note | 7 | machine-instability rule (single unreproduced red ≠ defect) |
| `df -h` | 6 | disk guard |
| `args` | 4 (one from 2026-09-03 passing worktree and journal paths; three from 2026-09-25 as reusable templates; the merge-queue script passes args to a child via `workflow()`) | parameterized reusable scripts |

**[I] Implication.** About a dozen standing rules are **copied by hand into every brief**. The rule set
drifted: `hooksPath` was added to the git ban only after an agent used it [C, owner's private notes]; the
image-name kill ban was added after `taskkill /IM cargo.exe` [C]. Rules are graph nodes with
`applies_to(role|phase|lane)` edges. A **context-pack query** can then compose the HDR, and a new rule
reaches every future brief the moment it is recorded.

### 5.2 The canonical script skeleton [M]

```
meta {name, description, phases[]}                 // pure literal
const TREE/WT/TGT/TRUNK/SP/OLD = '<absolute paths, shas>'   // SP = this session's scratchpad, OLD = an earlier session's
const HDR = `<standing rules + lane location + inputs by path + binding rulings>`
const SCHEMAs …; const verdict/green/done/counts = <parsers>
phase('X'); const a = await agent(HDR + ROLE + TASK, {label, phase, agentType, schema})
if (!ok(a)) return {a, note: 'stopped because …'}  // explicit stop points
… loops: verify(round) = parallel([tester, reviewer]) → triage → fix → verify(round+1)
return {every intermediate result}                  // becomes the run's record in journal.jsonl
```

### 5.3 Pattern catalogue (abstracted from engine specifics)

| ID | Pattern | Stages | Representative script(s) | State produced |
|---|---|---|---|---|
| **P1** | **Multi-lens survey → design → critique → revise → write** | K parallel read-only lenses (data, logic, prior plans, external reference) with schema `{report, open[]}` → architect rev 1 (text) → critic `{verdict, blocking[], non_blocking[], preserve[]}` → architect rev 2 answering every finding FIX/REFUTE/ACCEPT-AS-OPEN → writer puts research + design + critique log into repo docs | a design-unification script; a plan-revision script (critic loop ≤5 with a writer appending a per-pass critique log) | design doc revisions, a critique log, "lost lenses" recorded explicitly |
| **P2** | **Implement → (test ∥ review) → adversarial triage → fix rounds** | developer (red-first tests, mutations) → parallel tester + reviewer → a triage agent tries to **refute** every red item and Critical/Important remark → developer fixes only CONFIRMED → repeat ≤3 | three fix-implementation and lane scripts | reports `impl.md`, `test_r<n>.md`, `review_r<n>.md`, `triage_r<n>.md`, `fix_r<n>.md`; local commits |
| **P3** | **Numbered repair passes with an independent verifier** | repairer (dry run, then write) → verifier re-derives by *content*; pass N+1 is scoped by pass N's finite defect list; the final pass is checked **mechanically with the previous verifier's scripts**, with no new audit | citation-repair pass scripts (5 surviving scripts of an 8-pass campaign) | register sections "Pass N", debt tables, strike-through corrections |
| **P4** | **Parallel lanes in separate worktrees** | `parallel([laneA(), laneB()])`; each lane has `{tree, target dir, branch}`, its own HDR, and is told the other tree is not its own | a two-lane completion script | per-lane status strings, e.g. `green`, `tests-blocked`, `review-not-approved` |
| **P5** | **Pre-registered decision gate** | a measurement step classifies the result into bands fixed *before* the run; later stages run or are skipped by band | an implementation script (Step 0 → second stage) | band + measured value; skip reason logged |
| **P6** | **Commit split with proofs** | in a detached scratch worktree, build N patches that reproduce the lane byte-for-byte (blob-hash proof); each intermediate tree built and gated; commit messages drafted | a verify-and-close script (Split stage), a round-finish script | `<lane>_c<N>.patch`, `msg_<lane>_c<N>.txt`, per-commit receipts |
| **P7** | **Trunk merge (parameterized) and merge queue** | a trunk-merge template with `args {branch, tip, slug, title, reports, gates, pins, extra}`: `merge --no-commit --no-ff` → hand-resolve conflicts keeping both intents → re-derive anchor gates by content → root gates + branch gates + trunk value pins → one merge commit → an independent tester re-verifies the union. A merge-queue script runs two of these **sequentially via nested `workflow()`**; the second runs only on GREEN | the trunk-merge template; the merge-queue script | merge report, verify report, merge commit sha |
| **P8** | **Quiet-window preparation** | a single agent enumerates pending timed items by priority, builds A/B binaries from exported trees, runs untimed checks, and writes **one driver script the orchestrator runs alone** (no agent may run during timed passes); checkpoints after each pass | a quiet-window preparation script | `plan.md`, `run_window.sh`, `progress.txt`, `WINDOW_DONE` |
| **P9** | **Judge panel → synthesis** | one adversarial judge per candidate (default REJECT), schema'd verdicts `adopt/adapt/reject/already-shipped` → synthesis for the owner | a judge-panel round script (reads a *previous run's* candidates file from its run directory) | verdict set; candidates frozen with a revive condition (owner's private notes [C]) |
| **P10** | **Resumed stage** | the prompt opens with a RESUMED RUN preamble saying that a previous run was cut off. It names the pre-snapshot (status, git diff and sha256 manifest files), the per-arm summaries already done, and the rule not to redo finished arms but to re-run missing or inconclusive ones | a verify-and-close script (resumed stage) | continues the same scratch folder |

---

## 6. Decomposition vocabulary (the de facto schema)

The orchestrator uses a stable, hierarchical id vocabulary that is **referenced across files, runs and
sessions**. [M: extracted from scripts and MEMORY.md]

| Level | Examples of ids | Where they live today |
|---|---|---|
| Owner order / campaign | a campaign name with the date of the owner's order; a one-word campaign name | a `project-*` memory file; the owner's words quoted verbatim in HDR `OWNER` blocks |
| Phase | lettered phases with a status word ("Phase <X> closed", "Phase <X> exit") and cross-phase waits on a rung | MEMORY.md resume lines; plan `00-OVERVIEW` |
| Lever / track | `L<n>` | `levers/00-RULINGS.md`, per-lever design folders |
| Lane | `<lanes-dir>/<lane>` + a `perf/<topic>` branch + a per-lane target dir | HDR constants; MEMORY.md |
| Rung / step / commit | `C<n>`, `S<n>`, `U<n>`, `R<n>`, `KC-<nn>`, `UG-<nn>` | design docs; commit messages |
| Revision | rev 1, rev 2 (patch), rev 2.2, rev 2.3 (delta), rev 2.4 | `0N-DESIGN-REVx.md` numbered files |
| Round / pass | `r1..r3`, `p1..p5`, `pass 4..8` | report file names `test_r2.md` |
| Finding | critic `C1`, `W1–W5`, `O1–O11`, `OQ1–OQ3`; blockers `B1`; test findings `F1–F3`; `G1` | review files; rulings reference them |
| Ruling | `R<n>`, `Q<n>`, "ruling <n>" | per-rung `*_rulings.md` files, `00-RULINGS.md` sections |
| Test / mutation | `T<n>`, `M<n>`, `M-Rev<n>`, `<track>-N<n>` | designs (pre-registered), test reports |
| Gate / pin | `UG-<nn>`, `G-L<n>-<n>`, golden-image hashes, census counts | HDR `PINS` strings, CLAUDE.md prose |
| Measurement item | `P1..P5` (quiet window), `MQ:` entries | `MEASUREMENT-QUEUE.md`, window plans |
| Owner question | "VALUES/SCOPE", ballots `GB-5`, `GB-6` | `OPEN-QUESTIONS.md` register |

**[I] Implication.** This vocabulary *is* moirai's node taxonomy. It is already hierarchical (subtask),
already has blockers (a phase waiting for another phase's rung, a rung waiting for a quiet window, a
second merge only after the first is GREEN), and already has supersession (rev N patches rev N−1; rulings amend designs; strike-through plus a
pointer replaces a register row). Today all of it is encoded as **string ids in prose**. Referential
integrity therefore depends on grep.

---

## 7. Parallel lanes, worktrees, branches and merge-back

### 7.1 Measured state [M]

- **44 registered worktrees** (`git worktree list`). They include:
  - the main checkout, the owner's, which is off-limits to agents;
  - 3 under `.claude/worktrees/` (harness-created, 2 detached);
  - about 40 under `<lanes-dir>/<system>`, some `detached`: split and measurement scratch trees `_split-*`, `mq-<sha>`.
- **107 local branches, 49 of them `u/*`.**
- **Trunk:** an integration branch, checked out in its own lane worktree.
- **One `CARGO_TARGET_DIR` per lane**, under `<lanes-dir>/_targets/<lane>-msvc`. Target directories are
  sometimes shared and warm (one lane's target dir reused by another), with explicit rules against relinking while another agent runs
  executables from the same directory [M].

### 7.2 Lane lifecycle (reconstructed) [M/I]

1. **Fork.** `git switch -c u/<lane> <trunk sha>` inside the lane's worktree. The script quotes the base
   sha, and HDR states that the trunk commit is an ancestor.
2. **Work in rungs.** Each rung is a local commit made after its tester is GREEN (one fix script: commit
   only after the stage's tester reports green; never amend; follow-up commits for fixes).
3. **Declare the lane's footprint.** HDR lists the files a sibling lane edits and tells the agent to keep
   its edits in those shared files local, so the later merge is mechanical, and to name every shared file
   in its report (one fix-implementation script).
4. **Sync with the trunk before merging** (a lane-sync script referenced in the campaign file), then record
   the "tip" sha.
5. **Merge queue.** The orchestrator orders merges (A → B → C) and runs them one per nested
   trunk-merge call, gated on GREEN.
6. **Post-merge repair.** Re-derive citations that moved, by content; check counters that both branches
   changed; run the "ruled vs open" census over registers.
7. **Push** only on the owner's word. Lanes whose results are not chosen are **frozen with an annotated
   tag + a register row + a revive condition**, then deleted (owner's private notes [C]).

### 7.3 Merge-time hazards recorded in memory [C]

| Hazard | Mechanism | Current defence |
|---|---|---|
| **Clean merge-tree ≠ union compiles** | two lanes changed one type in different files | full build and gates on the merged tree |
| **Identical edits auto-merge into one** | both branches rewrote the prose counter "185" → "188" (+3 each); git kept 188, truth is 191 | re-measure every in-prose number on the union; grep old and intermediate values |
| **Union merge reimports pre-ruling text** | a keep-both-sides policy in registers brought back two RULED ballots as OPEN in 10 places | strike-through + dated pointer; a `ruled_vs_open` census test after every register merge; run the census on ours-only vs theirs-only to attribute |
| **Anchor rot after merge** | a moved file shifts `file:line` citations across the corpus | UG-10 re-derivation by content (a number changes only to the line holding its exact old text); never a new waiver |
| **Shared checkout races** | the index or a file is taken by another agent; snapshot names collide in a shared scratchpad | worktree per system; snapshot dirs unique per agent; never two mutating agents in one checkout |

**[I] Implication.** Git merges *text*; the orchestration needs to merge *claims*. moirai's merge must be
**field-typed**:
- a status lattice where ruled/closed dominates open;
- counters stored as per-branch deltas;
- notes that keep both sides, with the superseded one losing authority by an edge rather than by deleted text;
- **conflicts on authority-bearing fields surfaced, never auto-unioned.**

---

## 8. What state moves where

### 8.1 Inside a run (between phases) [M]

- **JS variables holding full agent outputs**, concatenated into the next prompt. Upstream reports go
  between `===== BEGIN =====` / `===== END =====` markers (15/38 scripts) so a truncation is visible, or
  as schema objects via `JSON.stringify`.
- **Truncation hazard.** Several scripts used `.slice(0, N)`: one implementation script caps a report at
  20,000 characters. The memory records **four silent-truncation incidents**, e.g. a synthesis that got
  3 of 5 lenses and a critic that got a plan cut mid-section [C, owner's private notes]. There is also
  an output form: when an architect's reply was split over two messages, `agent()` returned only the last
  one, and the writer saved a fragment [C].
- **Fix adopted.** Pass inputs **by path**. The agent reads the file, and the critic is told to declare
  Blocking if the text does not start with the expected heading or end with the expected section (one critic script).
- **The harness journal** records each call: `journal.jsonl` lines are
  `{type: started|result|failed|launched, key: "v2:<sha256>", agentId, [label, phase], [result]}` [M]. The
  key is a content hash of the call, so an identical prefix replays from cache on resume [D].

### 8.2 Between runs in one session [M]

- **Scratchpad files** under the session's scratchpad directory (`<scratchpad>/<topic>/`) hold:
  - reports `<stage>_r<n>.md`;
  - rulings `*_rulings.md`;
  - manifests `*_worktree_sha256.txt`;
  - patches `*.patch`;
  - commit messages `msg_*.txt`;
  - reusable scripts `*.js` (21 found at depth ≤3 [M]).

  A new run is handed these by absolute path, with an instruction to read the prior reports in full.
- **Tree manifests as preconditions.** A brief states that the uncommitted tree is the reviewed diff
  (12 modified + 3 untracked files listed in a `*_worktree_sha256.txt` manifest), tells the agent to
  verify all 15 before it starts, and to stop if the tree differs.
- **Design spread over numbered files** with patch semantics: the effective specification is v1 with
  Rev 2 applied, then Rev 3 applied (`A-01-design-v1.md`, `A-03-design-rev2.md` = patch,
  `A-05-design-rev3.md` = patch, `A-06-critique-p3.md` whose accepted optionals are part of the spec).
  The agent must compose the effective spec itself.

### 8.3 Between sessions [M/C]

- **Session scratchpads are session-scoped.** A new session gets a new scratchpad, so scripts carry both
  `SP` (the current session) and `OLD` (an earlier session) paths. One merge-queue script references **three**
  sessions' scratchpads [M].
- **Scratchpad sizes** reach **7.8 GB, 4.3 GB, 2.5 GB, 1.6 GB** (by session) [M]. They hold exported
  trees, binaries and build output next to the reports. Build targets placed *inside* a scratchpad have
  contributed to disk-full events on the development machine [C].
- **Run ids are not portable across sessions.** The owner's private notes record that run ids from an
  earlier session cannot be resumed [C]. After an API failure killed agents, the orchestrator relaunched
  with new ids and recorded both [M, owner's private notes].
- **The only cross-session index is auto-memory** (§10), plus the in-repo registers
  (`00-RULINGS.md`, `OPEN-QUESTIONS.md`, `MEASUREMENT-QUEUE.md`).

### 8.4 Resume patterns [M]

| Pattern | Mechanism |
|---|---|
| Harness resume | `Workflow({scriptPath, resumeFromRunId})`: the longest unchanged prefix of `agent()` calls is served from `journal.jsonl` [D]; only within a session [C] |
| Prompt-level resume | RESUMED RUN preamble with pre-snapshot, finished-arm summaries, and an instruction to continue from where it stopped (P10) |
| Precondition manifests | tree sha256 manifest checked before start, STOP on mismatch |
| Memory checkpoint | the MEMORY.md resume block: timestamp, session id, trunk sha + pushed?, live runs (bg task id + wf id + worktree), uncommitted files per worktree, merge order, a pointer to the end of the campaign file |
| Owner STOP points | 🛑 lines: date and time of the owner's stop, trunk state (pushed or not), uncommitted files per worktree, what was switched off |
| Watchdog | a cron job re-created on every session change |
| Journal salvage | when an agent's output was cut, recover the text from `journal.jsonl` (`type: result`) or `agent-*.jsonl` [C] |

---

## 9. Quiet windows and machine budget (constraints on moirai itself)

- **Timed measurements run only when the owner declares the machine quiet.** In the window the
  orchestrator stops every lane, and **no agent may run during a timed pass** because claude.exe itself
  costs 6–8% CPU [C, owner's private notes].
- **The idle check** requires 0 cargo/rustc/test processes on three polls and CPU < 5% over 10 s
  (a quiet-window preparation script [M]).
- **Disk:** every lane HDR says stop under 15 GB free; `CARGO_INCREMENTAL=0` because incremental caches
  contributed to disk-full events [C].
- **Hardware:** the development machine has experienced OS crashes and disk-full events. A single
  unreproduced red is reported as such and never "fixed" [C].

**[I] Implication for moirai's hard requirements.**
- **Idle cost must be ≈0 CPU, with no polling, no background compaction and no file watchers.** Any
  compaction or GC happens only on explicit command.
- **Writes must be crash-safe** (atomic commit, checksum) because the host has experienced OS crashes.
- The DB must be **small on disk**. The owner's machine is disk-starved by build caches, not by knowledge.

---

## 10. The cross-session memory mechanism

### 10.1 Harness contract [D]

Per <https://code.claude.com/docs/en/memory> (fetched 2026-09-25):
- auto-memory lives at `~/.claude/projects/<project>/memory/`, derived from the git repository, so **all
  worktrees of one repo share one memory directory**;
- `MEMORY.md` is an index, and **"the first 200 lines of `MEMORY.md`, or the first 25KB, whichever comes
  first, are loaded at the start of every conversation"**;
- topic files are read on demand;
- memory files are exempt from transcript retention cleanup.

### 10.2 `MEMORY.md` format [M]

- **72 lines, 21,966 B.** That is **88% of the 25 KB load cap** but only 36% of the 200-line cap. Lines
  are long (up to 1,590 B), and the text is mostly Cyrillic, which is 2 bytes per character in UTF-8, so
  the byte cap binds first.
- An earlier incident pushed it to 32 KB, above the limit, so it only partly loaded [C, owner's private notes].
- **Sections:**
  1. a resume block: **8 checkpoint lines** (newest first) + 10 lines of key lessons (🔑);
  2. a recent-campaign section;
  3. a feedback / working-agreements section;
  4. an environment section;
  5. a closed / standing section: one pointer to an overflow index file.
- **Line grammar:** `- [<markers> **<timestamp> <headline>**: <state…>](<topic-file>.md)`. Several links
  share one line, separated by `·`.
- **Markers:**
  - 🔴 ×1–5: urgency, 5 = newest checkpoint;
  - 🛑 ×4: owner STOP;
  - 🔑 / 🔑🔑: key lesson;
  - ⚠️: hazard.
- **Kinds of state found in the 8 checkpoint lines** [M]:
  - wall-clock timestamps and session ids;
  - trunk sha + pushed/not pushed;
  - background task ids and workflow run ids;
  - worktree path per live lane;
  - branch names;
  - **merge order** (lane A → lane B → lane C);
  - **uncommitted file counts per worktree**;
  - ready-to-merge lists;
  - phase open/closed;
  - orchestrator decisions;
  - owner STOP and resume words;
  - suspected root causes;
  - cron watchdog ids;
  - a pointer to the end of the campaign topic file for details.

### 10.3 Topic files [M]

- **254 topic files + the index, 2,452,125 B.** By file-name prefix: reference 104, project 102, feedback 48.
- **Frontmatter schema** (YAML), in 247 of 254 files; **7 files have none**:
  ```yaml
  name: <file stem>
  description: "<one-paragraph summary, p50 221 chars, max 674>"
  metadata:
    node_type: memory          # 244 files
    type: reference|project|feedback   # 103 / 96 / 48
    originSessionId: <uuid>    # 242 files; 47 distinct sessions
    modified: <ISO-8601>       # only 148 files (Jul 11, Aug 38, Sep 99)
  ```
  One file's prefix and `type` disagree (a `reference-*` file has `type: feedback`).
- **Sizes:** p50 48 lines, p90 144, max 2,227 lines. The five largest are campaign logs (`project-*`) of
  275 KB, 141 KB, 130 KB, 105 KB and 54 KB. They are **append-only journals** masquerading as notes.
- **The campaign file that MEMORY.md points to for details at its end** is 893 lines and 141,132 B,
  with **0 headings**, 100 distinct background task ids, 78 workflow run ids and 51 branch names. Its
  newest resume entries sit at l.816–874. **The last 14 lines are the original 2026-09-18 "How to apply"
  plan, now stale**, so the pointer to the end is only approximately true.
- **Link graph.**
  - The index has 184 links (176 unique), **0 dangling**.
  - **78 topic files are not linked from the index.** 32 of them are neither indexed nor wiki-linked from
    any other memory, so they are reachable only by `ls` or grep.
  - Topic files carry **701 `[[wiki-links]]`** in 216 files. **20 of those occurrences, to 13 distinct
    targets, do not resolve**; a few are false positives like `[[bin]]`. The real breaks are a missing
    type prefix (a link written without the `project-` prefix of its target), a `.md` suffix, and
    renamed or deleted files.
- **Supersession is textual.** A correction is written as a dated CORRECTED marker citing an owner
  ruling inside the same file, followed by what the row previously said. A retraction lives as a line
  lower in the same file or in another file. No mechanism marks the original as non-authoritative.

### 10.4 Staleness and workflow lessons the memory itself records

Read in full: 17 private topic files (6 working agreements, 11 hazards) on token economy, review-loop
termination, plan splitting, worktrees per system, parallel developers, frozen candidates, doc-rot
repair, retractions and summaries, union merges, workflow truncation, shell-mangled writes, write-less
architect roles, identical-edit merges, synthesis-agent limits, shared snapshot names, mtime guards and
measured-input rot. They are summarised below, not published.

| Lesson | Tag | Relevance to moirai |
|---|---|---|
| **Token economy.** Keep always-loaded context lean; subagents isolate heavy logs; structured outputs; an MCP server with access to source and a network path out is a supply-chain risk | [C] | moirai's MCP server must have **no network egress**. Its brief output must be tight. |
| **Review loops terminate by refutation, not by caps.** Critics invent findings when they are asked to find them; every finding needs a concrete failure scenario with `file:line`; refute before fixing; the round scope is the delta; track the refutation rate per critic; an empty list is success | [C] | Findings need a status (`open → confirmed/refuted/deferred → fixed`), a `refuted_by` edge, and **a per-critic refutation ratio query**. |
| **Split plans into files.** Monolithic plans rot: one insertion shifts every coordinate below it, and a retraction 200 lines below its claim coexists with it; defects cluster on the seams between files | [C] | Records are small nodes; "effective document" is a query over nodes and patches, not a growing file. |
| **Worktree per system.** One checkout shared by all agents gives index races, file races and blocking on another lane's files. Worktrees live outside the repo to avoid config leaks. Watch the disk | [C] | moirai lanes = worktrees; record each lane's footprint (files touched). |
| **Parallel developer agents.** Run independent steps in parallel; partition files; never parallelise steps that touch one file | [C] | A `touches` edge set per rung lets the orchestrator query "is this plan parallel-safe?". |
| **Freeze rejected candidates.** Tag first, then delete; keep a register with the losing number and a **revive condition**, and have the register declare its own staleness | [C] | A `Candidate` node with `status=frozen`, `tag`, `revive_condition`, `stale_after`. |
| **Repairing doc rot is risky.** 7 of 13 repairs wrote new falsehoods; each gate round claimed more coverage than it had; a doc asserting its own freshness is the riskiest line; a warning about staleness goes stale too | [C] | Claims need **provenance** (command, commit, date) and "verified_at" rather than prose "verified". |
| **A summary outlives its retraction.** Numbers were quoted from a summary field while a superseding row in the evidence log had already retracted them | [C] | **Retraction must propagate to every derived summary (`derived_from` edges)**, which is the "maximally synchronous" requirement applied to knowledge. |
| **Union merge reimports pre-ruling text** (10 places) | [C] | Typed merge (§7.3). |
| **Workflow input/output truncation is silent** (4 incidents) | [C] | Pass ids, not text; each record's size is known and returned in full or paged explicitly. |
| **The shell mangles memory writes.** Backticks in `python -c` are command-substituted, leaving holes, and the tool still prints "ok" | [C] | Writes go through a typed API (CLI with stdin/JSON, or MCP) with **read-after-write verification** and a content hash. |
| **Measured input rots without an edit.** A number derived from a corpus goes stale when the corpus changes, not when the sentence changes; `git log -S` cannot find it | [C] | A `Pin`/`Measurement` node stores the command + tree/commit it was measured at, and is flagged stale when that tree moves. |
| **Shared snapshot names give false restores;** an **mtime guard is not an edit guard** (546/1545 files touched in the same minute by a ritual) | [C] | Scratch identity must be per agent (nonce); "what changed" = git status/diff, not timestamps. moirai should not infer change from mtimes. |
| **Synthesis agents with huge context** (673k tokens; cache TTL 5 min), a session limit killing an agent mid-workflow, and a task output file truncated at about 30 KB (use `journal.jsonl`) | [C] | Hand agents ids plus small, bounded query results, not whole corpora. |

---

## 11. Failure modes of the current state keeping

Consolidated. Each row: symptom → evidence → root cause in the storage model → what moirai must do.

| # | Failure mode | Evidence | Root cause | moirai requirement |
|---|---|---|---|---|
| F1 | **Stale index lines.** 8 stacked checkpoint lines, only one current; the pointer to the details at the file's end lands next to a stale tail | MEMORY.md resume block; campaign file tail [M] | the index is hand-written prose; no "current" pointer; no expiry | "current state" is a **computed view** (latest checkpoint per campaign); superseded checkpoints are auto-hidden but kept in history |
| F2 | **Retraction does not reach summaries** | owner's private notes [C] | the summary is a copy, not a view; no `derived_from` link | `derived_from` edges; retracting a node marks every dependent summary **stale** at once |
| F3 | **Union merge resurrects pre-ruling status** | 10 places [C] | text merge of status-bearing records | typed merge: status lattice, supersession by edge, conflicts surfaced |
| F4 | **Identical counter edits collapse** | 185→188 twice = 188, truth 191 [C] | counters stored as absolute text | counters as per-branch deltas, or recomputed from the source query at merge |
| F5 | **Silent truncation** of inputs and outputs | 4 incidents [C]; a 20,000-character `.slice()` cap in scripts [M] | text passed by value into prompts | pass **node ids**; bounded paging with explicit "k of n" and a total count |
| F6 | **Shell-mangled writes**, reported as success | [C] | memory written via shell strings | typed write API; the returned content hash is checked; no shell quoting in the path |
| F7 | **Citation/anchor rot** (`file:line`) after code moves | the 8-pass citation-repair campaign; about 229 citations rotted; 199 of 359 numbers were already false before the lane [C]; UG-10 re-derivation in every merge brief [M] | positional references with no content fingerprint | if moirai stores code references, store **(path, symbol or quoted text, content hash, commit)**, and re-resolve by content, never by line offset (open question: should it at all?) |
| F8 | **Huge monolithic records** | 275 KB / 141 KB campaign logs; 4.35 MB ledger split later [C/M]; MEMORY.md at 88% of its cap [M] | append-only prose files | small typed nodes; logs as event nodes; bounded briefs |
| F9 | **"0 errors" but the promised artifact is missing** | architect cannot write [C] | output contract not checked against the tool envelope | a `produces` expectation on Run nodes; the run closes only when the artifact node exists (path + hash) |
| F10 | **Run ids not resumable across sessions**; scripts scattered over 21 cwd-derived dirs; state spread over three sessions' scratchpads | [C]; [M] | state keyed by session and cwd | `Run` nodes keyed by id with `script_path`, `args`, `journal_path`, `session`; lane state is independent of session |
| F11 | **Per-run facts hard-coded in briefs** (HEAD shas, pins, known reds) drift into the next run | HDR constants in 38/38 [M]; `PINS` default string in the trunk-merge template | facts live in script text | the brief is assembled from current nodes (lane tip, pins at trunk, known reds) at launch |
| F12 | **Orphaned and dangling knowledge** | 32 unreachable files; ~8 real dangling wiki targets [M] | untyped links by filename | referential integrity in the store: a link to a deleted node resolves to a **tombstone** carrying the reason and replacement |
| F13 | **Research conclusions quoted as owner rulings** | a later correction in the owner's private notes found no owner mark beside the row that had been cited as an owner ruling [C] | provenance not recorded per claim | every Rule/Decision has an `authority` field (owner / orchestrator / measured / research) and a source |
| F14 | **Rule drift across briefs** | the same dozen rules copy-pasted into 5–36 of 38 scripts [M]; bans added after incidents [C] | rules duplicated in text | Rule nodes with `applies_to`; the context pack composes them |
| F15 | **Duplicate agent definitions** (`.claude/agents` ≡ `.zcode/agents`) and a stale memory note claiming a role file is missing | [M] | copies with no link | out of scope to fix, but moirai notes should cite the source file hash so staleness is detectable |

---

## 12. Requirements for moirai as the single source of truth

### 12.1 Node kinds

Knowledge kinds (Rule, Hazard, Decision, Note) come from the brief. Orchestration kinds are derived
from §6. Typed fields are listed only when load-bearing.

| Kind | Key typed fields | Replaces today |
|---|---|---|
| `Campaign` | `title`, `owner_order` (verbatim), `opened_at`, `status` | `project-*` topic file header |
| `Phase` | `status: open\|closed`, `exit_criteria`, `closed_at`, `closing_commit` | "Phase A closed" prose |
| `Lane` | `worktree_path`, `branch`, `base_sha`, `tip_sha`, `target_dir`, `dirty_files:int`, `status: active\|ready_to_merge\|merged\|frozen\|abandoned` | HDR constants; MEMORY.md resume lines |
| `Task` / `Rung` / `Step` | `done:bool`, `status`, `kind` (design, impl, fix, merge, measure, doc), `pre_registered: bool` | rung ids in designs |
| `Run` (workflow run) | `wf_id`, `bg_task_id`, `session_id`, `script_path`, `args`, `journal_path`, `status` (running, green, red, stopped, died), `started_at`, `ended_at` | ids pasted into MEMORY.md |
| `AgentCall` (optional, see Q2) | `label`, `role`, `phase`, `status`, `result_ref`, `bytes` | `journal.jsonl` lines |
| `Session` | `id`, `started_at`, `stop_point`, `stopped_by: owner\|crash\|api_error` | 🛑 / new-session lines |
| `Report` / `Artifact` | `path`, `sha256`, `bytes`, `kind` (design, critique, test, review, triage, fix, merge, verify, patch, manifest, message), `first_line_verdict` | scratchpad files |
| `DesignRev` | `rev` ("2.3"), `is_patch: bool`, `path`, `sha256` | `0N-DESIGN-REVx.md` |
| `Finding` | `local_id` (C1, W2, O3, F1), `severity`, `status: open\|confirmed\|refuted\|deferred\|fixed\|withdrawn`, `failure_scenario`, `evidence_ref`, `confidence: CONFIRMED\|PLAUSIBLE` | review/critique text parsed by regex |
| `Ruling` / `Decision` | `authority: owner\|orchestrator\|measured`, `text`, `date`, `binding: bool`, `scope` | `*_rulings.md`, `00-RULINGS.md`, orchestrator decisions in MEMORY.md |
| `OwnerQuestion` | `kind: VALUES\|SCOPE`, `options[]`, `status: open\|answered`, `answer`, `answered_at` | `OPEN-QUESTIONS.md` rows, ballots |
| `Gate` | `name`, `command`, `can_fail_proof` (mutation id), `leg` | HDR gate lists |
| `Pin` / `Measurement` | `value`, `unit`, `command`, `measured_at_commit`, `machine_state` (quiet or loaded), `status: current\|moved_declared\|stale` | pin strings in HDR, census counts in prose |
| `GateResult` | `result: green\|red`, `numbers`, `reproduced: bool` | report lines |
| `Mutation` | `id`, `site`, `expected_red_test`, `compiles: bool`, `result` | mutation tables |
| `MeasurementItem` | `priority`, `binaries[]`, `stop_rule`, `status: queued\|run\|voided\|done` | quiet-window plans, `MEASUREMENT-QUEUE.md` |
| `Candidate` | `status: frozen`, `tag`, `losing_number`, `revive_condition`, `stale_after` | `*-REJECTED.md` |
| `Rule` (feedback) | `text`, `authority`, `applies_to` (roles, phases, lanes, all), `since` | `feedback-*`, HDR boilerplate |
| `Hazard` (reference) | `symptom`, `mechanism`, `defence`, `measured: bool`, `incidents:int` | `reference-*` |
| `Note` | free text + `type` (note, finding, decision, critical) | generic memory |
| `Role` | `name`, `tools[]`, `can_write: bool`, `model`, `output_contract`, `defined_in` (path + hash) | `.claude/agents/*.md` |
| `Checkpoint` | `at`, `summary`, `next_actions[]`, `merge_queue[]` (ordered lane refs) | MEMORY.md resume lines |

### 12.2 Edge kinds

| Edge | From → To | Semantics / integrity rule |
|---|---|---|
| `subtask_of` | Phase→Campaign, Lane→Phase, Rung→Lane, Task→Task | tree (recursive); a parent's `done` may be derived |
| `blocks` / `depends_on` | Task→Task, Lane→Lane (merge order), MeasurementItem→QuietWindow | a DAG; **a query "what is unblocked now" is a first-class need**; deleting a blocker notifies dependents |
| `merge_after` | Lane→Lane | the ordered merge queue |
| `runs_in` | Run→Lane | a run belongs to exactly one lane (or none, if read-only research) |
| `produced` / `consumed` | Run→Report; Run→Report/DesignRev/Ruling | evidence of what a phase actually read; required to answer "which runs used the retracted spec?" |
| `patches` | DesignRev→DesignRev | the effective spec = fold of patches; **cycle-free** |
| `critiques` / `reviews` | Report→DesignRev or Report→Lane tip | — |
| `raises` | Report→Finding | — |
| `confirms` / `refutes` | triage Report→Finding | a finding's status is derived from these plus rulings |
| `rules_on` | Ruling→Finding, Ruling→OwnerQuestion | binding; "do not re-open" |
| `fixes` | Commit/Report→Finding | — |
| `supersedes` / `retracts` | any→same kind | **the old node keeps its text but loses authority**; every `derived_from` dependent is flagged stale |
| `derived_from` | summary Note/Checkpoint → source nodes | the propagation path for F1/F2 |
| `gated_by` | Rung/Lane→Gate | — |
| `pins` | Gate→Pin | — |
| `measured_at` | Pin→commit (field) or Lane | stale when the lane tip moves past it without a re-measure |
| `touches` | Rung/Lane→path (value node or string set) | conflict query between concurrent lanes |
| `owned_by` | Rung→DesignRev/plan of a sibling | a sibling plan owns the rung |
| `applies_to` | Rule→Role/Phase/Lane/`*` | brief composition |
| `cites` | Note→Note | referential integrity; deletion gives a tombstone |
| `resumes` | Session→Session, Run→Run | resume lineage across sessions |

### 12.3 Consistency semantics ("maximally synchronous")

- **Tombstones, not hard deletes.** A deleted node leaves a tombstone: `deleted_at_commit`, `by`,
  `reason`, `replaced_by?`. Any read that traverses a reference to it returns the tombstone *in the same
  query result*. That is the brief's "node 40 deleted → referrers immediately know", and it is also
  history-preserving, as the owner's "strike, never delete" register convention demands. [M] 6 of 38
  scripts prescribe strike-through-with-pointer, and 14 of 38 forbid deletion or allow it only by
  absolute path under the run's scratch directory.
- **Supersession is an edge, not an edit.** Authority is computed: a node is *authoritative* iff no
  non-tombstoned `supersedes`/`retracts` edge points at it. Summaries (`derived_from`) inherit staleness
  transitively. This is the direct fix for F1, F2 and F3.
- **Write atomicity plus read-after-write verification.** A write returns the stored content hash, and
  the caller can assert it (F6).
- **Reverse-edge indexes are mandatory.** Every query in §12.5 walks edges backwards: "who depends on
  X", "who derived from X", "which runs consumed X".

### 12.4 Versioning and branching that mirror worktree lanes

| Need | Evidence | Requirement |
|---|---|---|
| A lane forks from a trunk commit and works in isolation for days | 44 worktrees, 49 `u/*` branches [M] | **moirai branch per lane**, created at the same moment as the git branch. Candidate trigger: the `WorktreeCreate` hook [D] or an explicit `moirai lane open`. It records `base_sha` |
| Lane-local knowledge (findings, reports, pins moved by the lane) must not leak to other lanes until merge | per-lane pins (a moved value pin is a defect unless declared) [M] | branch-scoped writes; the trunk view excludes unmerged lanes |
| **Cross-lane global knowledge** (rules, hazards, owner rulings) must be visible to all lanes immediately | rules added mid-campaign apply to every later brief [C] | a **global (unbranched) namespace**, or auto-propagation of rule/hazard kinds to all live branches. This is a design fork (see Q5) |
| Merge with typed semantics | F3, F4 | field-level merge policies per kind: status lattice, counters as deltas, sets unioned, authority by edge; **surface conflicts on binding fields** |
| Diff between trunk and lane at merge time | the merge brief lists what the branch brings, conflicts, anchor counts and gates with numbers [M] | `moirai diff <lane> <trunk>` as structured change sets (added/closed findings, moved pins, new rulings) that feed the merge commit message |
| History and blame | "when did this become untrue" needs git archaeology today (`git log -G`) [C] | per-node history with the commit (moirai) + git sha + session + author role for every change |
| Sequential merge queue | an ordered three-lane merge queue in the resume lines [M] | `merge_after` edges + a query "next mergeable lane" (all blockers merged, own tester GREEN) |
| Frozen lanes and candidates | [C] | `status=frozen` branches stay readable, never merged |

### 12.5 Query needs by moment

| Moment | Query (proposed CLI shape) | Must return | Bound |
|---|---|---|---|
| **Session start** (replaces the MEMORY.md resume block) | `moirai brief` | current checkpoint per open campaign; live lanes (path, branch, tip, dirty count, status); runs in flight or last result per lane; merge queue; open blockers; **open owner questions**; stop point + who stopped; the top-N hazards/rules flagged critical; stale-flagged summaries | ≤ ~2–4k tokens; must fit comfortably under the 25 KB / 200-line MEMORY.md budget if rendered into it [D] |
| **Before a phase** (replaces hand-built HDR) | `moirai pack --lane L --role tester --phase verify` | lane location (tree, branch, base, target dir); rules `applies_to` this role/phase; **effective spec** (DesignRev fold) by path + hash; binding rulings; open confirmed findings; gates + current pins + known reds; files touched by other active lanes (do-not-touch list); prior reports by id/path | a complete list, never truncated; each item ≤ a line plus a path |
| **During review loops** | `moirai findings --lane L --status open,confirmed` · `moirai critic-stats --role architecture-critic` | findings by status; refutation ratio per critic and per round; delta since the last round | — |
| **Before parallel fan-out** | `moirai conflicts --rungs a,b,c` | overlap of `touches` sets; shared target dirs | — |
| **At merge time** | `moirai merge-check --lane L` | lane gates + results; declared pin moves vs pins moved; counters both sides changed; rulings on the trunk that the lane predates; findings still open; merge-order prerequisites | — |
| **After merge / anytime** | `moirai stale` | nodes whose `measured_at` commit is behind the lane tip; summaries derived from retracted nodes; dangling refs (tombstones) | — |
| **Owner-facing** | `moirai questions --open` | VALUES/SCOPE questions with options | — |
| **Brief-driven examples** | `moirai blockers --ids` · `moirai note add --critical "…"` | the ids of all blocking tasks; a critical note written | — |

### 12.6 Size and frequency estimates (derived from §5.1 and §10) [I, grounded in M]

| Quantity | Current measurement | moirai estimate |
|---|---|---|
| Workflow runs | ~11/day (Sep), 401 total [M] | Run nodes: ~4k/year |
| Agent calls | p50 4, p90 14 per run; 3,294 total [M] | if recorded: ~40k/year (small nodes, ~200 B each without bodies) |
| Reports / artifacts | ~5–10 per run (impl, test_rN, review_rN, triage, fix, merge, verify) [M] | ~30–60k/year as path + hash nodes (~300 B) |
| Findings | ~5–20 per critique/review round [I from C/W/O numbering up to O11] | ~20k/year |
| Knowledge notes (rules, hazards, projects) | 254 files, +99 modified in Sept [M] | ~1–2k nodes/year (more granular than files) |
| Edges | 701 wiki-links over 254 notes (~2.8/note) [M]; orchestration adds ~3–6 per run/finding | ~5× node count |
| Total after 3 years | — | ~0.3–0.5M nodes, ~2M edges, **metadata well under 500 MB even uncompressed; resident index a few tens of MB** |
| Bodies | result p50 8.8 KB, p90 38 KB, max 153 KB [M]; `.md` files in one session's scratchpad: p50 ~19 KB, p90 ~106 KB, max 1.1 MB [M]. That scratchpad also contains exported source trees, so these figures are indicative only | **the decision to store bodies dominates size** (see Q3): with bodies, ~1–3 GB/year; with path + hash only, negligible |
| Write rate | bursty: ≤16 concurrent agents per workflow [D], several workflows in parallel lanes | peaks of tens of writes/s; steady state ≈0 |
| Read rate | a brief per session start; a pack per agent launch (~11 runs × p50 4 agents/day) | ~50–500 queries/day; latency target ≪100 ms so it never shows up in agent time |

### 12.7 Integration surface

- **CLI** first: the owner's scripts and agents already live in Bash; the tool must be quoting-safe per F6, so it takes JSON on stdin.
- **MCP server**, stdio, local only, no network egress. This satisfies the supply-chain objection in the owner's private notes [C]. MCP resources with `subscribe`/`notifications/resources/updated` [D: <https://modelcontextprotocol.io/specification/2025-06-18/server/resources>] give a standard channel for "node changed/deleted" pushes to a connected client.
- **Hooks**, all fail-open:
  - `SessionStart` → `moirai brief`;
  - `WorktreeCreate`/`WorktreeRemove` → lane open/close;
  - `SubagentStop` or `TaskCompleted` → run result capture;
  - `PreCompact` → checkpoint [D].
- **Skills:** "open lane", "record verdict", "merge-check", "session brief". These replace the copy-pasted HDR and verdict parsers.
- **Workflow scripts** have **no filesystem or Node API** [D]. They can reach moirai only through agents calling MCP/CLI, or through `args` the orchestrator fills from `moirai pack` before launching. The script API cannot call moirai directly, so **the orchestrator-side pack-then-launch pattern is the realistic integration** [I].

---

## 13. Anti-requirements (what must NOT go into moirai)

| Do not store | Why | Evidence |
|---|---|---|
| **Agent transcripts, harness journals** | 2.2 GB of run dirs, 420 MB of session transcripts [M]; the harness already owns them, with its own resume semantics [D] | store `journal_path` + `agentId` pointers only |
| **Scratchpad bulk**: exported trees, binaries, build targets, golden images, diff PNGs | GBs per session [M]; build targets contributed to disk-full events [C] | store path + sha256 + kind |
| **Code-symbol index** | LSP (identity), semble (prose), graphify (architecture) are already measured and routed in CLAUDE.md l.277–398 [C] | moirai references code; it does not index it |
| **Gate execution / CI** | gates are cargo tests and scripts with their own proofs | moirai stores gate *definitions* and *results*, not runners |
| **LLM-driven summarisation that rewrites stored facts** | repairing doc rot wrote new falsehoods in 7 of 13 attempts [C]; summaries outlive retractions [C] | summaries are computed views or explicitly `derived_from` nodes; never silent rewrites |
| **Owner chat and secrets** | none needed. A keyword grep of the 255 memory files for `password\|api key\|secret\|bearer` found only 2 hits, both the word "secretly" [M] | reject credential-shaped content on write |
| **Background activity**: polling, watchers, auto-compaction, telemetry, network calls | quiet-window idle rule [C]; supply-chain rule for MCP [C] | on-demand only |
| **Textual "union" merge of authority-bearing records** | F3 | — |
| **Numbers without provenance** | F7; measured input rots without an edit [C] | a `Pin` must carry command + commit |
| **One growing record per campaign** | F8 | enforce small nodes (e.g. a soft cap on body size per kind) |
| **Policy enforcement** (blocking tool calls) | the existing hooks deliberately never block [M]; blocking belongs to hooks, not to the memory | moirai answers queries; hooks decide |

---

## 14. Open questions only the owner can answer

1. **Auto-memory coexistence.** Should moirai *replace* `~/.claude/projects/<project>/memory/`, or *render into* `MEMORY.md` as a generated view, given that the harness always loads that file [D]? And what becomes of the 254 existing topic files: migrate, freeze, or leave as a read-only archive?
2. **Granularity of run records.** Record every `agent()` call (~40k/year), or only runs + reports + findings?
3. **Bodies.** Store report and design bodies inside moirai (content-addressed, versioned with the branch), or only path + sha256 pointing into scratchpads and the repo? This choice sets the disk footprint by 2–3 orders of magnitude (§12.6).
4. **Lane ↔ branch coupling.** Create a moirai branch automatically for every git worktree/branch (hook-driven), or only when the orchestrator opens a lane explicitly? There are 44 worktrees today, several of them scratch or detached [M].
5. **Global vs branched knowledge.** Are rules, hazards and owner rulings global (visible to every lane at once), or branched and merged like lane state? A hazard discovered in one lane today reaches other lanes only through the next brief.
6. **Language.** Memory is mostly Russian; repository artifacts must be English (CLAUDE.md l.219) [C]. Which language for moirai records, and are they "repository artifacts"?
7. **Code references.** Should moirai store `file:line` style references at all (with a content fingerprint and re-resolution), or leave all code positions to docs and the anchor gates?
8. **Who writes.** Every agent, or only the orchestrator plus writer roles? Read-only roles (architect, critic, analyst, researcher) cannot write files by design. Should they be allowed to write *findings* to moirai via MCP, and does that change the separation of duties?
9. **Location and sharing.** Keep the DB inside the repository (versioned, pushable, visible to anyone who clones) or under `~/.claude` (private, machine-local)? One DB per project, or one global DB (the owner has at least BoykoEngine and another project in the same environment)?
10. **Retention.** History forever (tombstones and superseded nodes kept indefinitely), or pruning by age? The register convention says "strike, never delete".
11. **Quiet windows.** May the moirai MCP server stay resident (0% CPU, a few MB RSS) during timed measurement windows, or must it be fully stopped like every agent?
12. **Authority vocabulary.** Is the three-level authority (owner ruling / orchestrator decision / measured fact / research claim) the right one, and may the orchestrator mark something "owner" only with a verbatim quote attached?

---

## 15. Sources

**External**

| Claim used | Source (fetched 2026-09-25) |
|---|---|
| MEMORY.md load limit "first 200 lines … or first 25KB"; memory dir shared by all worktrees of a repo; topic files read on demand; memory excluded from transcript cleanup | Claude Code docs, *How Claude remembers your project*: <https://code.claude.com/docs/en/memory> (redirected from docs.claude.com) |
| Hook events (SessionStart, UserPromptSubmit, PreToolUse, SubagentStart/Stop, TaskCreated/Completed, WorktreeCreate/Remove, PreCompact/PostCompact, SessionEnd …); `additionalContext`, `permissionDecision`, exit code 2 blocks | Claude Code docs, *Hooks reference*: <https://code.claude.com/docs/en/hooks> |
| Sub-agent frontmatter (`tools`, `model`, `memory: user\|project\|local`, `isolation: worktree`, `effort`); nesting up to three layers | Claude Code docs, *Subagents*: <https://code.claude.com/docs/en/sub-agents> |
| Workflow script API (`agent`, `parallel`, `pipeline`, `workflow` one level, `args`, null on death, concurrency cap, `resumeFromRunId` longest-unchanged-prefix cache, `journal.jsonl`, `Date.now()` banned) | Claude Code `workflow-authoring` skill reference as loaded in this session (harness-bundled documentation; no public URL) |
| Linked worktrees share everything except per-worktree files such as `HEAD` and `index`; `refs/` shared, pseudo refs per worktree | `git-worktree(1)`, git 2.54.0: <https://git-scm.com/docs/git-worktree> |
| MCP resources `subscribe` / `listChanged`, `resources/subscribe`, `notifications/resources/updated`, `notifications/resources/list_changed` | MCP specification 2025-06-18, *Resources*: <https://modelcontextprotocol.io/specification/2025-06-18/server/resources> |

**Local (read-only)**

- BoykoEngine repository: `CLAUDE.md` (sections listed in §0); `.claude/settings.json`; `.claude/settings.local.json`; `.claude/hooks/*.py`; `.zcode/config.json`; `.claude/agents/*.md`; `.zcode/agents/*.md`.
- Scripts: 34 Workflow scripts in the owner's private Claude Code project directories and 4 parameterized scripts from one session's scratchpad (private; not published).
- Harness run records: `~/.claude/projects/<project>/*/subagents/workflows/wf_*/{journal.jsonl,agent-*.jsonl,agent-*.meta.json}` (structure and size only).
- Memory: `~/.claude/projects/<project>/memory/MEMORY.md` and the 17 topic files summarised in §10.4 (mechanism only; private, not published).
- `git worktree list` and `git branch --list` in the BoykoEngine checkout (read-only), run 2026-09-25.
