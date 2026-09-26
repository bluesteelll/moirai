# 01 — BoykoEngine agentic workflow, role-agent lens

**Research only.** Nothing in the BoykoEngine repository was changed or built. Engine subject matter is
deliberately abstracted away; where a role file uses an engine example, only the workflow pattern is
kept.

| | |
|---|---|
| Date of study | 2026-09-25 |
| Repository state | the BoykoEngine repository, branch `feat/multi-paradigm-render`, HEAD `49f2fcfb` (2026-09-22) |
| Role files read in full | all 9 files in `.claude/agents/` (23,162 words total) |
| Supporting material read | `CLAUDE.md` §Agents / §Orchestration discipline / §Rules for agents; `.claude/settings.json`; `.claude/hooks/*.py`; git history of `.claude/agents`; 25 workflow-related files of the project's Claude Code memory; metadata-only statistics of `docs/` registers and of 347 Workflow scripts |
| External docs | Claude Code sub-agent and hooks reference (fetched 2026-09-25) |

**Evidence tags used throughout**

- **[M]** MEASURED by me on 2026-09-25 (a command was run; the number is reproducible at the commit above).
- **[C]** CLAIMED by a source (role file, `CLAUDE.md`, memory file, commit message); I did not re-verify it.
- **[D]** DOCUMENTED externally; URL and fetch date in §10.
- **[I]** INFERENCE of mine from the above.

---

## 0. TL;DR

1. BoykoEngine runs a **nine-role pipeline** driven by one orchestrator (the main chat) and, since
   August 2026, by **Workflow scripts** (347 scripts on disk [M]). The canonical feature path is
   `researcher → architect ⇄ architecture-critic → developer(s) ⇄ code-reviewer → tester → results-analyst → commit/merge → doc-writer`,
   with `project-analyst` as an out-of-band read-only analyst.
2. Every role emits a **fixed Markdown report template** whose sections are, in effect, a typed
   schema: plans with decisions/alternatives/trade-offs/open questions; reviews with severity-tiered
   remarks carrying mandatory *Failure/Consequence* and *Confidence* fields; test reports with
   failures and benchmark-vs-target rows; a final verdict with a "return to phase" pointer.
3. That schema is **never stored as data**. It lives in (a) the orchestrator's context window,
   (b) `${…}` string interpolation between workflow `agent()` calls, (c) hand-maintained Markdown
   registers (`docs/OPEN-QUESTIONS.md` alone is 68,452 words [M]), (d) 255 memory files (182,746
   words [M]), and (e) commit messages. The project's own memory catalogues the failures that
   follow: silent truncation of hand-offs, plans that never became files because the architect cannot
   write files, rulings lost across branches, union-merges that resurrect `OPEN` on decided items,
   summaries that outlive their retraction, dead line anchors, stale role prompts.
4. The **final gate role is currently not loadable**: `results-analyst.md` has invalid YAML
   frontmatter [M], and Claude Code silently skips such files [D]. The owner's private notes recorded the
   symptom ("agent type not found") but misdiagnosed the cause as a missing file [M].
   This is a live example of the exact failure moirai is meant to prevent: a recorded "fact" with no
   verification link that turns out wrong.
5. moirai maps naturally onto this: **tasks/plans/findings/verdicts/measurements/rules/decisions/
   open-questions/deferrals as typed nodes**, `part_of / blocks / supersedes / refutes / verifies /
   derived_from / addresses / measured_on` as edges, **role identity on every commit**, a **status
   lattice that merges monotonically across branches**, and a **budgeted "context pack" query**
   replacing hand-built prompt headers. §8 lists per-role integration points.

---

## 1. Inventory of the role agents

All values [M] from frontmatter at HEAD `49f2fcfb`.

| Role | `tools` | `model` | `effort` | Words | Writes files? | Runs shell? |
|---|---|---|---|---|---|---|
| architect | Read, Glob, Grep, WebSearch, WebFetch, **Agent** | opus | — | 2,426 | no | no |
| researcher | WebSearch, WebFetch, Read, Glob, Grep | opus | — | 1,440 | no | no |
| architecture-critic | Read, Glob, Grep, WebSearch, WebFetch | opus | — | 2,937 | no | no |
| developer | Read, Write, Edit, Glob, Grep, Bash | opus | — | 2,569 | yes | yes |
| code-reviewer | Read, Glob, Grep, Bash, WebSearch, WebFetch | opus | — | 3,329 | no | yes |
| tester | Read, Write, Edit, Glob, Grep, Bash | opus | — | 2,993 | yes (tests) | yes |
| results-analyst | Read, Glob, Grep, Bash, WebSearch, WebFetch | opus | — | 2,579 | no | yes |
| project-analyst | Read, Glob, Grep, Bash, WebSearch, WebFetch | opus | — | 2,911 | no | yes |
| doc-writer | Read, Write, Edit, Glob, Grep, Bash, WebFetch | opus | — | 1,978 | yes (public docs) | yes |

Observations:

- **No role uses `effort`, `memory`, `isolation`, `hooks`, `mcpServers`, `skills`, `maxTurns`,
  `disallowedTools` or `permissionMode`** [M], although all are supported sub-agent frontmatter
  fields [D]. Effort and model are instead set **per call** inside Workflow scripts: across 347
  scripts, `effort: 'high'` 284×, `'medium'` 11×, `'xhigh'` 11×, `'low'` 5×, `'max'` 2×;
  `model: 'fable'` 145×, `'opus'` 109× [M, textual occurrences]. The frontmatter `model: opus` is
  therefore a default that scripts routinely override.
- **Model-routing history** [M from `git log -- .claude/agents`]: 2026-05-23 `6fa64383` removed all
  `model:` lines (inherit session model); 2026-07-09 `4cb9f0da` split Opus (4 judgement roles) /
  Sonnet (5 "mechanical" roles); 2026-07-27 `fef01042` moved all nine to Opus ("the 'mechanical'
  half was never mechanical"); 2026-09-17 `f20bdafe` recorded the measurement behind that decision in
  `CLAUDE.md` [C: 27-question retrieval test, Opus 27/27 found and 0 confidently-wrong vs Sonnet
  23/27 and 4]. The owner's private notes add a fourth layer: a per-role model preference for the
  deciding roles, later suspended by an addendum [C]. **The routing truth is spread over four places
  that disagree in wording.**
- A **`.zcode/agents/` mirror** of all nine files exists for another tool; at HEAD it is byte-identical
  modulo CR [M]. It is maintained by hand in the same commits (`b0bff31f`, `d4ee35f9` touch both) [M].
- **Actual usage frequency** (occurrences of `agentType: '<role>'` in 347 Workflow scripts [M]):
  developer 213, tester 204, architecture-critic 110, architect 95, general-purpose 94,
  code-reviewer 71, project-analyst 66, researcher 65, `claude` 41, doc-writer 10,
  **results-analyst 0**.

---

## 2. Per-role contracts

Each card lists what the role file says; items marked ⚠ are contradictions or staleness I found.

### 2.1 architect

| Aspect | Contract |
|---|---|
| Purpose | Design a feature/subsystem **before any code**; output is a plan, never code. |
| Inputs at start | The feature request (from the orchestrator). **Mandatory**: launch `researcher` via the `Agent` tool with a concrete query before designing. Existing code via Glob/Grep/Read. On later rounds: the critic's remarks, relayed by the orchestrator. |
| Output | Plan in a fixed template: *Goal* (perf + functionality) · *Context and constraints* incl. **target metrics** · *Key decisions* (each: **What / Why / Alternatives (rejected + why) / Trade-off**) · *Data structures* · *Public API* · *Algorithms for critical paths* · *Multithreading model* · *Integration* · *Implementation plan (for the developer)* as numbered steps with file · *Metrics and validation* (benchmarks, mandatory tests, runtime invariants) · *Open questions*. A **readiness checklist** where every item is checked or marked N/A with a reason. |
| Revision protocol | From revision 2 onward return a **PATCH**, not the whole plan: per change, the section heading, the **removed text quoted verbatim**, the added text, and **the sections whose invariants this change depends on** "so the critic knows what to re-read". Every round must state *what changed*; a round that repeats an argument without new evidence "is not a round"; do not re-litigate a withdrawn remark; do not reopen your own decision "unless a measurement forces it". |
| Hand-off | → `architecture-critic` (via orchestrator). After approval, three downstream consumers read different sections: developer ← *Implementation plan*; tester ← *Metrics and validation*; results-analyst ← *target metrics*. |
| Stop / escalation | No explicit stop rule. Must decide ("Do NOT leave 'we could use X or Y'"); unresolved items go to *Open questions* "so the critic and the user can discuss". |
| Separation of duties | No implementation code; no unjustified decisions; no copying reference designs without understanding them. |
| Loop | Architect ⇄ critic until the critic approves. |
| Records | Decisions + rejected alternatives + trade-offs; open questions; per-round changelog; verbatim-removed text (explicitly a guard against **silent drops**: the file quotes the project's own history — "Rev 1 and Rev 2 dropped it silently"). |
| Must remember across sessions [I] | Current revision number and its lineage; which critic remarks are settled / withdrawn / rebutted; decisions with their rejected alternatives; which sections depend on which invariants. |
| ⚠ | The file justifies the patch rule with "`MESHLET-VIRTUAL-GEOMETRY-PLAN.md` stands at Rev 39 and 36,427 words" [C] — **verified exactly**: 36,427 words, highest `Rev` number 39 [M]. |
| ⚠ stale | "Workspace of three crates" — the workspace has 27 crate directories [M]. It tells the architect to study "the `ecs` branch" via `git show origin/ecs:…`, but that branch was merged into master on 2026-07-09 (`4417e6a2`) [M], and the architect has **no Bash** to run `git show` (the file acknowledges this in the same sentence). |
| ⚠ tooling | Has no Write/Edit: in Workflow runs the plan exists only as the agent's returned text (see §6, item L2). |

### 2.2 researcher

| Aspect | Contract |
|---|---|
| Purpose | Gather state-of-the-art practice from primary sources **before** an architectural decision. |
| Inputs | A concrete question (from the architect or orchestrator). The role decomposes it into sub-questions, runs several searches in parallel, fetches primary sources, and checks what already exists in the repo. If another branch is needed it "notes this in the output, and the orchestrator will switch". |
| Output | *TL;DR* (3–5 bullets) · per-reference-system *Approach / Algorithm / Data structures / Trade-offs / Source* · comparative table · key algorithms · pitfalls · academic works · *Applicability* (take directly / adapt / does not fit) · *Open questions for the architect* · numbered *Sources* list. |
| Quality rules | No invented facts ("no reliable information found"); every non-trivial claim cited; **fact vs opinion** distinguished; freshness and versions checked; depth over breadth; concrete numbers. |
| Separation of duties | Must not propose an architecture ("that is the architect's work"). |
| Records | Source URLs with role (fact/opinion), version sensitivity, open questions. |
| Must remember [I] | Prior research per topic and its date (to avoid re-researching and to know when it is stale); source reliability. The file embeds a large static "map of primary sources" — curated knowledge that would be better as data. |

### 2.3 architecture-critic

| Aspect | Contract |
|---|---|
| Purpose | Find problems in the architect's plan before development starts. |
| Inputs | The full plan; the existing code (to check consistency/duplication); on later rounds, the updated plan and its own previous remarks. |
| Output | *Verdict* `APPROVED` / `CHANGES REQUESTED` · remarks in three tiers: 🔴 **C#** critical (blockers), 🟡 **W#** important, 🟢 **O#** optional · each remark: *Where* (plan section), *Problem*, **Consequence (mandatory)**, **Confidence `CONFIRMED` (traced, with file:line) or `PLAUSIBLE`**, *Why critical*, *What is needed* · *Positive* · *Open questions*. |
| Anti-false-positive rules | "A topic the plan does not mention is not by itself a defect." "APPROVED with no remarks is a valid, complete critique." Severity "is earned by the Consequence field, never by suspicion — do not launder a PLAUSIBLE into a 🔴". An unsubstantiated doubt goes to *Open questions* "with the measurement or citation that would settle it". Cites Kamoi et al., TACL 2024 (arXiv 2406.01297) on self-correction without an external oracle [C]. |
| Iteration | Re-read the **whole** plan; mark each previous remark ✅ resolved or keep it open with what is still missing; add new remarks; loop until no 🔴/🟡 remain. |
| Separation of duties | No code; must not design ("point out direction, not the solution"); must not approve with 🔴/🟡 open; no style nitpicks. |
| Records | Remark IDs (C1, W1, O1 — **per report**, not globally unique), resolution marks ✅ across rounds, positives (what must be preserved), open questions. |
| Must remember [I] | The identity of each remark across rounds (C1 of round 3 vs C1 of round 4), rebuttals received and how they were evaluated, refuted remarks (so they are not re-raised). |
| ⚠ contradiction | "Re-read the whole plan" conflicts with (a) the architect's patch protocol, which exists so the critic re-reads only dependent sections, and (b) an owner-derived rule in the owner's private notes: **a round's scope is the delta**, because re-reading unchanged text keeps turning up new issues forever [C]. |
| ⚠ missing | That rule's central mechanism — a **refutation pass between critic and fixer**, where each finding must survive an attempt to disprove it before it causes an edit — is **not in this file**; it lives only in memory and in Workflow scripts (174 of 347 scripts mention "refut" [M]). |

### 2.4 developer

| Aspect | Contract |
|---|---|
| Purpose | Implement the **approved** plan precisely. "Multiple developer agents may be launched in parallel for independent features." |
| Inputs | The approved plan (read fully before any code); files to modify and neighbours for conventions; in parallel mode, a **file partition** that the orchestrator "is required" to make non-overlapping. |
| Output | *Modified files* / *New files* (path + one line) · *Conformance to plan* (✅ per decision with `file:line`, ⚠️ **deviation: what and why**) · *Unsafe blocks* inventory (location + invariant quote) · *Checks* (compile + lint results; "tests were not run — that is for the tester") · *Known limitations / TODO* · "Ready for code review". |
| Stop / escalation | Plan unclear → "stop and ask the orchestrator. Do not guess." Case not covered → ask the orchestrator, "who will consult the architect". Compiler contradicts the plan's types → "the plan may be wrong (then escalate)". File outside scope needs changing → note it in the report. Dead-looking code → leave it, note it. |
| Separation of duties | No architecture decisions; no optimisation beyond the plan; **no tests, no test runs**; **no git commits** ("that is the orchestrator's job on user request"); no edits outside task files; no deletions not ordered by the plan. |
| Records | Deviations from plan, TODOs/limitations, unsafe inventory, out-of-scope needs. |
| Must remember [I] | Which plan step each change implements; file ownership in the current parallel batch; outstanding deviations awaiting architect confirmation. |
| ⚠ | Mentions the `ecs` branch as a separate line of code (stale since 2026-07-09 [M]). |

### 2.5 code-reviewer

| Aspect | Contract |
|---|---|
| Purpose | Find bugs, performance problems, principle violations and plan divergences in the developer's code. |
| Inputs | The developer's report (files, self-assessed conformance, unsafe list, limitations), **the code itself** ("do not rely on the report alone"), the plan. |
| Output | *Verdict* `APPROVED` / `CHANGES REQUESTED` · *Build checks* · remarks 🔴 C# / 🟡 W# / 🟢 O# each with *Where* (`file:line`), *Problem*, **Failure (mandatory: input / interleaving / call order / target)**, **Confidence `CONFIRMED` / `PLAUSIBLE`**, *Why critical*, *What to do*, quoted code · *Positive* · *Open questions for the developer*. "Returning nothing is a valid, complete review." |
| Iteration | Re-read **only the changed places** (whole file if the fix is large); re-run build/lint; each previous remark ✅ closed or ❌ still open with explanation; new problems added; loop until APPROVED. |
| Separation of duties | Never edits code; architectural problems are **escalated to the orchestrator**, not fixed by review; no test runs; no APPROVED while 🔴/🟡 remain. |
| Evidence cited | Three arXiv papers on reviewer false positives [C]. Spot check [D]: arXiv 2509.01494 (SWR-Bench, 1,000 PRs) exists as cited, but the "8–17% precision" figure is not in its abstract; arXiv 2603.18740's abstract reports 97% (32/33) for a *context-manipulation attack*, which is narrower than the file's paraphrase "reviewers instructed to hunt for problems flag 68–97% of already-correct code". Not wrong per se, but **unverifiable from the role file alone** — an example of a claim that should carry a provenance/verification link. |
| ⚠ contradiction | 🟡 is defined as "must fix, but does not block merging the whole feature", yet the file also says "Do NOT mark APPROVED while 🔴 or 🟡 remain". |
| Must remember [I] | Remark identity across rounds; which fixes claim to address which remark; the *kind* of each remark (see "retest is not re-review", §2.6). |

### 2.6 tester

| Aspect | Contract |
|---|---|
| Purpose | Build, write and run tests and benchmarks for code the reviewer approved; report coverage, failures and measured performance. |
| Inputs | The approved plan, **especially "Metrics and validation"**; modified/new files; existing tests (style). |
| Gate at entry | Build in all profiles; "Any build error — **STOP**, return the report to the orchestrator. Don't write tests for code that doesn't compile." |
| Output | *Build* · *Test coverage* (unit / integration / property / concurrency-model tests, per file with counts and names) · *Run results* (the literal `running N tests` line) · *Failures* F#: file, what it checks, expected, received, trace, possible cause · *Benchmarks* table: operation, time, throughput, **vs target from the plan** · *Comparison with baseline* · *Coverage* · *Notes/TODO* · "Ready for results-analyst". |
| Anti-vacuity rules (dated, measured by the project) | `running 0 tests` is a vacuous pass; a test "you have never seen fail is a claim, not a gate" — name what makes each test red and, where cheap, prove it by mutation; skips are not passes (separate counts); toolchain/host and flag-source traps; stale-build "false freshness". Each rule carries a date and an incident (2026-07-23, 2026-08-10, 2026-09-17) [C]. |
| Separation of duties | Never fixes production code ("you document the failure"); never changes the architecture; never hides failures; never deletes tests. |
| Hand-off | Failures → orchestrator → developer. Benchmarks worse than plan → "a flag for the results-analyst". |
| Must remember [I] | Baselines (numbers **plus the environment they were taken in**), known-red targets, expected test counts per target (for anti-vacuity), toolchain facts. The file itself has become a store of dated lessons — 3 of its last 5 commits (`f20bdafe`, `b0bff31f`, `d4ee35f9`, all 2026-09-17) add dated lessons; the other 2 only change the `model:` line [M from git log]. |

### 2.7 results-analyst

| Aspect | Contract |
|---|---|
| Purpose | Final decision after design → implementation → testing: did the feature meet its goals, and if not, **to which phase does it return**. |
| Inputs | The approved plan, the developer's report, the reviewer's report, the tester's report — "read (or ask the orchestrator to pass through)". Then re-run all checks itself ("don't blindly trust the reports"), inspect generated code. |
| Output | **`ACCEPTED` / `REWORK` / `RETHINK`** · metrics table *Target / Actual / Delta / Status* · a 10-item checklist (build, lint, tests, coverage, UB checker, concurrency checker, targets met, no regressions, documented unsafe, plan implemented) · regressions vs baseline · tech debt (`file:line`, priority) · *Problems* P#: problem, impact, **root cause**, **Return to: architect / developer / tester**, **acceptance criteria for re-review** · recommendations for future features. |
| Decision rule | Numeric thresholds: e.g. ≤ target×1.1 accepted, ×1.1–1.5 rework, > ×2.0 rethink; regressions ≤5% / 5–15% / >25%; zero failed tests for ACCEPTED. "ACCEPTED only when goals are achieved… or the plan must be adjusted (but that's the architect's job)." |
| Separation of duties | No fixes; the verdict is **a recommendation** — "the orchestrator may contest it with the user". |
| ⚠ **not loadable** | Its `description:` contains `verdict: feature accepted`; the `: ` makes the frontmatter invalid YAML — PyYAML fails with "mapping values are not allowed here"; the other eight files parse [M]. Claude Code "reads no fields from the file, skips it, and writes the parse error to the debug log" for YAML that does not parse [D]. A note in the owner's private notes records that a 7-hour Workflow run failed at its analysis stage with "agent type not found" and concludes that the role file is missing from .claude/agents [C] — the symptom is right, **the recorded cause is wrong** (the file exists; it fails to parse) [M+D]. Workaround in use: `agentType: 'claude'` with the role described in the prompt (41 occurrences of `'claude'` in scripts [M]). `CLAUDE.md` still lists the role [M]. |
| Must remember [I] | Baselines and previous verdicts; accepted known limitations; debt created per feature; rework history (how many times, to which phase). |

### 2.8 project-analyst

| Aspect | Contract |
|---|---|
| Purpose | Read-only general analyst for open questions about existing code, outside the feature pipeline. |
| Modes and outputs | **A** explanation/navigation (TL;DR, where, how, why, connections, pitfalls) · **B** security audit (findings **V-###** with category, where, reproduction, impact, recommendation; unsafe inventory table) · **C** bug hunting (**B-###**: what happens / should happen / trigger / root cause / *covered by a test?*) · **D** performance (**P-###**: where, problem, impact estimate, confirmation) · **E** tech debt (**D-###**: type, cost of leaving / cost of fixing S/M/L, cross-references) · **F** comparison with external systems (with sources). |
| Inputs | The user's question; the internal feature map as "first port of call", then search. |
| Separation of duties | Never edits; never installs tools (notes absence instead); **never issues accepted/not-accepted verdicts**; never proposes architecture. |
| Records | ID-prefixed findings — but IDs restart per report [I], so V-001 of two audits collide. |
| ⚠ stale | "On master right now there's only memory. On the `ecs` branch there's much more" — stale since 2026-07-09 [M]. |
| Must remember [I] | Prior findings on the same scope and their fate (fixed? became a task? refuted?), so a second audit does not re-report or contradict the first. |

### 2.9 doc-writer

| Aspect | Contract |
|---|---|
| Purpose | Public, user-facing documentation only (a static-site book + generated API docs). |
| Two-layer rule | **Internal docs are for agents, maintained by the architect/orchestrator; public docs are written only by doc-writer.** Internal docs are a *source*, never a target, for this role. |
| Inputs | Task (page / system / sync after change / release notes / section); internal docs, source, existing pages; ambiguities "clarify with the orchestrator, don't invent". Large outlines (>500 lines) are shown to the orchestrator first. |
| Output | Pages + table-of-contents registration; report: created/modified pages, TOC updated, diagrams, code examples verified, **cross-links (to / from)**, build status, open questions, suggested follow-up. |
| Rules | No invented facts; performance numbers only measured or labelled "target"; no TODO stubs published; build must pass. |
| Must remember [I] | Which source facts each page depends on (so pages can be flagged when the source changes). `CLAUDE.md` notes a doc-writer's output "is gated by nothing at all" and that repairing doc rot introduced new falsehoods in 3 of 5 measured attempts [C]. |

---

## 3. Cross-cutting rules the roles depend on (not in the role files)

These are where the orchestrator's behaviour is defined. Each is a candidate moirai *rule* node.

| Rule | Where it lives | Status |
|---|---|---|
| Separation of duties (developer doesn't test; reviewer doesn't fix; critic doesn't design; project-analyst doesn't edit) | `CLAUDE.md` §Separation of duties + each role file | consistent [M] |
| Clarify before acting; Plan Mode for ≥3 files; **only VALUES/SCOPE go to the owner, perf/architecture forks are decided with numbers** | `CLAUDE.md` §Orchestration discipline; owner's private notes | consistent |
| Sub-agents cannot ask the user, so a sub-agent that hits ambiguity "stops and escalates to the orchestrator" | `CLAUDE.md`, `clarify_gate.py` docstring | consistent |
| Any difficulty → tell the owner in chat **and** append to `docs/OPEN-QUESTIONS.md`; mark `RESOLVED` with date and ruling, never delete | owner's private notes; register header | consistent |
| Commit policy | `CLAUDE.md`: "Never commit without an explicit user request"; `developer.md`: commit is "the orchestrator's job on user request"; owner's private notes: commit and push automatically, superseding the earlier per-action gating | **⚠ contradictory across 3 sources** [M] |
| Review loops: no round cap; stop when the list of **confirmed** blockers after refutation is empty; round scope = delta; count the refuted share (a critic whose findings are mostly refuted carries no weight that round) | owner's private notes (owner rule) | **not in role files** [M] |
| Retest is not re-review: a fix for a complexity/performance finding must be re-reviewed for the property and guarded by a scaling test | owner's private notes | **not in role files** [M] |
| Trust targeted verification (don't respawn a full tester run for a localized green change) | owner's private notes | not in role files |
| Freeze, don't delete, losing candidates: annotated tag + registry row with a **revival condition** and a declared staleness caveat | owner's private notes (owner rule) | not in role files |
| Parallel developers only on disjoint files; one worktree + branch **per system**, outside the repo tree | owner's private notes | not in role files |
| Split plans into several files; ~800–1,000 lines triggers a split | owner's private notes (owner rule) | not in role files |
| Never put an unverified premise in a sub-agent brief — mark each claim VERIFIED (with command) or ASSUMPTION | owner's private notes | not in role files |
| Never kill processes by image name; only the PID tree you started | owner's private notes; stated in Workflow headers | not in role files |
| No agent activity at all during timed measurement windows | owner's private notes | not in role files |

**[I]** The role files encode the *static* contract; the *learned* contract (roughly a dozen rules
that change how roles loop, verify and hand off) lives in memory files and is re-injected into each
Workflow script by hand (118 scripts define a shared `HDR` header block [M]).

---

## 4. How roles are actually invoked (orchestration substrate)

- **Main chat as orchestrator** chooses roles and runs loops (`CLAUDE.md` §Agents).
- **Workflow scripts** (JS, 347 on disk under the project's Claude Code state [M]) declare `phases`,
  run `agent(prompt, {agentType, model, effort, schema, label, phase})`, fan out with `parallel()`,
  and pass results between phases by **string interpolation** (`${design}`), sometimes truncated
  with `.slice(0, N)` (72 scripts [M]). 199 scripts use structured-output `schema` [M]. Results are
  cached per agent call, and a failed run is resumed with `resumeFromRunId` [C, owner's private notes].
- A typical script (inspected: an "evidence → research → design → critique" workflow) opens with a
  hand-written `HDR` that pins: worktree path, branch, **base commit**, toolchain environment,
  forbidden git operations, a warning that other agents work in other worktrees, the **current defect state
  with numbers and the date they were measured**, and language/search rules [M]. That header is a
  manually assembled **context pack**.
- **Git worktrees** isolate lanes (one per system, e.g. `<lanes-dir>/<system>`), plus
  `.claude/worktrees/agent-*` and `wf_*` created by the harness [M].
- **Hooks** [M]: `UserPromptSubmit` → `clarify_gate.py` (reminds, never blocks); `PreToolUse` on
  Read/Glob and Bash → "graphify-first" nudges. No `SubagentStart/Stop`, `SessionStart`,
  `TaskCreated/Completed` or `PreCompact` hooks are used [M], though those events exist [D].
- **Cross-session memory**: `~/.claude/projects/<project>/memory/` — 255 files
  (48 feedback, 102 project, 104 reference, 1 index), 182,746 words, 2.45 MB [M]. `MEMORY.md` is a
  21,966-byte index [M] (a memory file says a 24.4 KB limit once caused a partial load [C]); its top
  section is a hand-written, Russian-language **resume checkpoint** (trunk commit, live workflow run
  ids, worktree states, what to resume) [M]. 701 wiki-links to 198 targets, ~13 unresolved (about
  half are real dangling names, the rest are code in brackets) [M]; **78 topic files are not linked
  from the index** [M] (reachable only by search). 242 files carry an origin-session id from 47
  distinct sessions; 107 lack a `modified:` timestamp [M].

---

## 5. Synthesis (a) — the implicit workflow state machine

### 5.1 Task lifecycle (feature path)

```mermaid
stateDiagram-v2
    [*] --> Proposed
    Proposed --> NeedsOwner: ambiguous VALUES/SCOPE\n(OpenQuestion, blocks)
    NeedsOwner --> Proposed: RESOLVED (owner ruling)
    Proposed --> Researching: orchestrator (or architect) spawns researcher
    Researching --> Designing: research report
    Designing --> DesignReview: plan rev N (full at N=1, PATCH after)
    DesignReview --> Refuting: CHANGES_REQUESTED\n(findings with Consequence+Confidence)
    Refuting --> Designing: >=1 finding CONFIRMED
    Refuting --> DesignApproved: all blockers REFUTED
    DesignReview --> DesignApproved: APPROVED (no open red/yellow)
    DesignApproved --> Implementing: file partition per developer\n(worktree per system)
    Implementing --> Blocked: plan unclear / not covered /\ncompiler contradicts plan
    Blocked --> Designing: orchestrator consults architect
    Blocked --> Implementing: orchestrator answers
    Implementing --> CodeReview: implementation report
    CodeReview --> Implementing: CHANGES_REQUESTED\n(delta re-review; perf findings need re-review of the property)
    CodeReview --> Designing: architectural problem escalated
    CodeReview --> Testing: APPROVED
    Testing --> Implementing: BUILD_FAILED (stop) / test failure F#
    Testing --> Analysis: test report (+ benchmarks vs targets)
    Analysis --> Accepted: ACCEPTED (10/10 criteria)
    Analysis --> Designing: REWORK->architect / RETHINK
    Analysis --> Implementing: REWORK->developer
    Analysis --> Testing: REWORK->tester
    Accepted --> Committed: orchestrator (policy disputed, see section 3)
    Committed --> Merged: feature branch -> integration line
    Merged --> Documented: doc-writer (optional)
    Documented --> [*]
    Merged --> [*]
    DesignApproved --> Deferred: parked with revival condition
    Implementing --> Frozen: losing candidate (tag + registry)
    Analysis --> MeasurementQueued: needs idle-machine numbers
    MeasurementQueued --> Analysis: measurement recorded
```

### 5.2 Gates

| Gate | Owner | Pass condition (as written) | Evidence artifact |
|---|---|---|---|
| G0 scope | orchestrator/owner | request unambiguous; ≥3 files → Plan Mode approval; VALUES/SCOPE → owner | chat, `OPEN-QUESTIONS.md` |
| G1 design | architecture-critic | no unresolved 🔴/🟡 (and, per memory, only refutation-surviving findings count) | review report |
| G2 code | code-reviewer | build + lint clean, no unresolved 🔴/🟡 | review report |
| G3 test-entry | tester | builds in all profiles | test report |
| G4 test | tester | tests pass with non-vacuous counts; benches vs target recorded | test report |
| G5 acceptance | results-analyst | 10 criteria + numeric thresholds | verdict report |
| G6 commit/merge | orchestrator (+owner) | verified green; explicit paths; branch per feature | commit |
| G7 public push | owner | outward-facing; confirm unless authorised | — |

### 5.3 Loops and termination

| Loop | Round input | Termination | Known pathology (project's own record) |
|---|---|---|---|
| architect ⇄ critic | PATCH + dependent sections | critic APPROVED; owner rule: **no round cap**, stop when confirmed-blocker list is empty after refutation | up to **30 rounds** before the rule [C]; a critic asked to find problems keeps finding them |
| developer ⇄ code-reviewer | changed places | APPROVED | "retest ≠ re-review" (an exponential algorithm passed a 51-test green suite after a claimed fix) [C] |
| tester → developer | failure reports | tests green | vacuous greens (0 tests, compiled-away models, skips) |
| analyst → phase X | P# problems with acceptance criteria | ACCEPTED | verdict role currently unloadable (§2.7) |
| repair loops on gates/docs | — | rule in the owner's private notes: after two consecutive rounds find the previous incomplete, stop extending and **bound the claim** to the mechanism that exists [C] | 13 doc-rot repairs, 7 of which wrote new false statements [C] |

### 5.4 Parallelism and side paths

- **Parallel**: independent research lenses; multiple developers on disjoint files; independent
  reviews; refuters in parallel; lanes in separate worktrees (three concurrent lanes observed on
  2026-09-23 [C]).
- **Serial by rule**: review after implementation; testing after review; timed measurements with
  **no** concurrent agent activity.
- **Side paths**: `project-analyst` produces findings that become backlog items or tasks;
  `doc-writer` consumes accepted work; `researcher` can be called directly.

---

## 6. Synthesis (b) — the implicit data model

### 6.1 Entity kinds

| Kind | Produced by | Consumed by | Fields evidenced in role templates / registers |
|---|---|---|---|
| **Campaign / Lane** | orchestrator | all | name, branch, worktree path, base commit, toolchain env, status |
| **Task** (and **Subtask** by `part_of`) | orchestrator; architect's *Implementation plan* steps | developer, reviewer, tester | title, target files (ownership set), plan step ref, status, assignee role, parallel batch |
| **Plan** | architect | critic, developer, tester, analyst | goal, constraints, **target metrics**, sections, revision number, readiness checklist (item → checked / N/A + reason) |
| **Plan revision / Patch** | architect | critic | rev N, per change: section, removed text (verbatim), added text, depends-on sections, changelog line |
| **Decision** | architect; owner rulings | developer, critic, analyst | what, why, **rejected alternatives (each with reason)**, trade-off, decided-by (role/owner), date |
| **Candidate (rejected/frozen)** | orchestrator/analyst | future architects | what it was, losing number + measurement cell, build flag, git tag, **revival condition**, staleness caveat |
| **Research report / Source** | researcher | architect | question, sub-questions, per-source claim, URL, version/date, fact vs opinion, applicability (take/adapt/reject) |
| **Finding / Review remark** | critic, reviewer, project-analyst | architect, developer | id (C#/W#/O#, V/B/P/D-###), severity (🔴/🟡/🟢 or critical/important/informational), where (section or file:line), problem, **failure / consequence scenario**, **confidence CONFIRMED/PLAUSIBLE**, what is needed, status across rounds (open / ✅ closed / ❌ still open / withdrawn / refuted) |
| **Refutation attempt** | refuter agent / orchestrator | orchestrator | target finding, outcome (CONFIRMED / REFUTED / PARTIAL), evidence (command/grep) |
| **Verdict** | critic, reviewer, analyst, refuters | orchestrator | target, role, round, outcome, **return-to phase**, acceptance criteria — 15 distinct outcome vocabularies in scripts (§6.3) |
| **Implementation report** | developer | reviewer, analyst | files changed/new, conformance per decision (file:line), deviations (what/why), unsafe inventory, checks run, limitations |
| **Test / Gate** | tester | analyst | name, file, what it checks, **what makes it red**, mutation-proved (y/n), expected count |
| **Test run** | tester | analyst | command, `running N tests`, passed / failed / skipped (separate), failures F# (expected, received, trace, suspected cause) |
| **Measurement** | tester, analyst | analyst, architect | metric, value, unit, **target**, delta, status; environment: commit, host triple, profile, machine load, data scale; baseline ref |
| **Rule / Standing instruction** | owner (via orchestrator), orchestrator lessons | all roles | statement, scope (roles/phases it applies to), provenance (owner verbatim vs agent-derived), date, supersedes |
| **Lesson / Failure class** (memory "reference") | orchestrator | orchestrator, briefs | incident, mechanism, how to detect, how to apply, related lessons |
| **Open question** | any role → orchestrator | owner | date, situation, options, **what it blocks**, kind (VALUES / SCOPE / unclear), status OPEN / RESOLVED (date + ruling) |
| **Deferral / Backlog item** | architect, analyst, orchestrator | future planning | id (e.g. BL-N), status (❓ open question · 🔬 needs measurement · 💡 idea · ⏸ deferred), reason, **graduates to a plan when decided** |
| **Queued measurement** | orchestrator | tester | what it decides (so it can be struck if decided elsewhere), preconditions (idle machine), command |
| **Tech-debt item** | analyst, project-analyst | future planning | file:line, priority, cost of leaving / fixing |
| **Claim** | every role | everyone downstream | statement, VERIFIED (with command) or ASSUMPTION, source |
| **Run / Agent invocation** | Workflow harness | orchestrator | run id, agent id, role, model, effort, phase, label, input size, truncation, result |
| **Checkpoint** | orchestrator | next session | trunk commit, live runs, lanes and their state, what to resume |
| **Doc page** | doc-writer | readers | path, cross-links to/from, source facts it depends on, build status |

### 6.2 Relation kinds

| Edge | From → To | Evidence in the role contracts |
|---|---|---|
| `part_of` | subtask → task → campaign; plan section → plan | implementation plan steps; multi-file plans |
| `blocks` / `blocked_by` | open question / failing gate / missing dependency → task | register convention "what it blocks"; developer escalation |
| `depends_on` | task → task (ordering); plan section → section (invariants) | architect patch "sections whose invariants this change depends on"; parallel-developer rule |
| `implements` | code change / task → plan decision / step | developer "conformance to plan" |
| `addresses` | fix → finding | reviewer ✅/❌ per remark |
| `refutes` / `confirms` | refutation or measurement → finding / claim | refutation pass; CONFIRMED/PLAUSIBLE |
| `verifies` | test / measurement / review → task, fix or claim | tester, analyst, "retest ≠ re-review" |
| `supersedes` | decision/rule/measurement/plan rev → older one | plan revisions; "RESOLVED rather than deleted"; supersede-in-place lesson |
| `rejected_for` | alternative / candidate → decision | "Alternatives: what was rejected and why"; frozen candidates |
| `derived_from` | plan → research; decision → measurement; finding → source | researcher → architect; analyst root cause |
| `measured_on` | measurement → commit, environment | host change 2026-09-17; "measured on a tree without the fix" lesson |
| `returns_to` | verdict → phase/role | analyst "Return to: architect/developer/tester" |
| `raised_by` / `decided_by` | node → role / owner | role authorship; owner rulings |
| `applies_to` | rule → role / phase / path scope | cross-cutting rules (§3) |
| `owns` | lane/developer → file set | parallel file partition |
| `cites` | any → source URL (with version/date) | researcher, reviewer citations |
| `mirrors` | doc → translated doc | frozen Russian mirror; `.zcode` agent mirror |
| `reopens_if` | frozen candidate / deferral → condition | freeze rule |
| `contradicts` | node ↔ node | commit-policy and critic-scope contradictions (§3, §2.3) |

### 6.3 Observed verdict vocabularies (inputs to a canonical status model)

Distinct `verdict` enum shapes in the scripts, by frequency [M]:

| # | Enum | Proposed canonical outcome mapping [I] |
|---|---|---|
| 13 | APPROVED, CHANGES_REQUESTED | pass / fail-fixable |
| 9 | APPROVED, NEEDS_FIX, REJECTED | pass / fail-fixable / fail-fundamental |
| 9 | APPROVED, APPROVED_WITH_CHANGES, REJECTED | pass / pass-with-conditions / fail-fundamental |
| 5 | CONFIRMED, REFUTED, PARTIAL | (claim) holds / refuted / holds-narrower |
| 5 | APPROVED, REVISE, REJECT | pass / fail-fixable / fail-fundamental |
| 4 | APPROVED, REVISE | pass / fail-fixable |
| 3 | buildable, defective | pass / fail-fixable |
| 3 | SOUND, DEFECTIVE | pass / fail-fixable |
| 2 | recommended, viable, ruled-out | (candidate) preferred / acceptable / rejected |
| 2 | adopt, adapt, reject, already-shipped | (idea) take / take-modified / reject / n-a |
| 1 each | violation / probable-violation / legitimate-exception / unclear; sound / fixable / broken; holds / holds-narrower / refuted / reversed / could-not-check; confirmed-zero-cost / zero-cost-with-caveat / costs / could-not-check | … / unknown |

Template-level vocabularies add `ACCEPTED / REWORK / RETHINK` (analyst) and `APPROVED /
CHANGES REQUESTED` (critic, reviewer). **[I]** A verdict is really `(target kind, outcome class,
return-to, conditions)`; moirai should store the role's raw label *and* a canonical outcome class so
cross-role queries ("everything currently failing") work.

---

## 7. Synthesis (c) — where today's mechanism is lossy, manual, stale-prone or token-expensive

Each item: mechanism → evidence → cost.

**L1. Hand-offs are unstructured text passed by interpolation, and truncation is silent.**
`.slice(0, N)` on agent inputs in 72/347 scripts [M]. Four recorded incidents [C, owner's private
notes]: a synthesis received 3 of 5 lenses; a critic received a plan cut mid-section and raised a
blocker about gates it could not see; twice an
architect's output arrived in two messages and `agent()` returned only the last, so a writer saved a
headless fragment as the design file. In every case **an agent noticed, not the orchestrator**.

**L2. Read-only roles cannot persist their product.** Architect / critic / researcher /
project-analyst have no Write [M]. The owner's private notes record [C]: two workflows asked the
architect to write a plan file under `docs/`; both returned 35 KB and 70 KB of text noting that
Write/Edit were disabled, no file appeared, the next-phase critic reported the plan as not reviewable
because it did not exist, and the workflow reported **4 agents done with 0 errors**. Recovery meant
cutting the text out of `journal.jsonl` by hand.

**L3. Role catalogue drift with no health check.** `results-analyst` is unparseable [M] and silently
skipped [D]; the memory note diagnosing it is wrong [M]; `CLAUDE.md` still advertises it [M]; the
analysis stage of a 7-hour run failed on it [C]. Role prompts carry stale project context (3 crates
vs 27; a branch merged 11 weeks ago; an instruction to run `git show` given to a role without Bash)
[M]. `CLAUDE.md` contains a sentence duplicated verbatim within one paragraph ("The previous split
put…" twice, line 207) [M].

**L4. The same rule lives in 2–4 places and they disagree.** Commit policy (3 sources, 2 positions);
model routing (frontmatter, `CLAUDE.md`, two memory files, per-call script overrides); critic re-read
scope (critic file vs architect patch protocol vs owner-derived memory rule) [M]. No source marks
which one supersedes which, except by prose.

**L5. Registers are append-only prose with statuses as free text.** `docs/OPEN-QUESTIONS.md`:
68,452 words, 5,891 lines, 87 top-level entries, 95 commits, `RESOLVED` appears in 19 headings and
81 times overall [M]. Its newest entry (2026-09-03) documents that **twelve owner rulings made on one
branch never reached another**, whose register "still listed all fourteen as OPEN — and printed one
of them backwards" [C, quoted from the register]. `docs/BACKLOG.md` uses emoji status codes and
states that an item "graduates to a PLAN doc + phase only when it is decided" [M]. The Russian
mirror of the register is frozen at 19,551 words vs 68,452 English [M].

**L6. Git merge semantics are wrong for status claims.** The owner's private notes [C]: merging a
73-commit branch, 7 register documents conflicted in 48 hunks; the prescribed union policy (keep both
sides) **re-imported pre-ruling text, so two RESOLVED ballots were printed OPEN again in 10 places**;
picking a side loses records instead. The project had to write a dedicated census test (an id marked
RULED cannot also be marked OPEN) and run it after every register merge. The owner's private notes
[C]: two branches each changed a prose counter 185→188 (+3 each); git merged the identical edits
silently; the truth was 191.

**L7. Supersession by pointer leaks retracted values.** The owner's private notes [C]: agents
returned a summary of verified rules with the original numbers, while the evidence journal had a later
superseding row that downgraded them; the orchestrator quoted the retracted numbers to the owner.
Lesson recorded: corrections must rewrite the value **in place**, not add a pointer further down.

**L8. Line-number anchors rot; addressing by document creates no back-links.** The owner's private
notes: 184 of 282 anchors dead [C]. The owner's private notes [C]: a sentence written to prevent two
plans from building the same thing was invisible from both sides because it addressed the other plan
*by document*, not by step — zero mutual mentions, so neither grep found it. The owner's private
notes [C]: a blocker that cited a decision saying the opposite, written in the same commit that made
it unnecessary; a campaign waited on a capability that already existed.

**L9. Findings are not objects.** Remark IDs are per report (C1, W1…); the ✅/❌ ledger across rounds
lives in the orchestrator's context; the refutation pass, the refuted-share metric and the
"retest ≠ re-review" distinction are not in the role files (§3). Result: the orchestrator must carry
finding identity across rounds in its own context window, and loses it at session boundaries.

**L10. Measurements lack a bound environment.** The build host changed on 2026-09-17 and pinned
numbers from before are only comparable under the old triple [C, `tester.md`]; timed runs require an
idle machine and even "doc-only" agents break that rule [C]; "measured on a tree without the fix"
requires a `merge-base --is-ancestor` check before quoting [C]; micro-benchmarks at L1 scale
overstate effects [C]. None of this is attached to the number itself; it is prose around it.

**L11. Cross-session state is a hand-maintained, size-capped index.** 255 memory files / 182,746
words / 2.45 MB; index 21,966 bytes near a claimed 24.4 KB load limit; 78 topic files unreachable from
the index; ~6 real dangling wiki-links; a Russian-language resume checkpoint listing run ids and
trunk commits, updated manually [M]. A note on shell-mangled memory writes exists in the owner's
private notes (title only read) — writing the store is itself error-prone.

**L12. Plans are enormous monolithic documents.** 50 `*PLAN*.md` files totalling 470,311 words;
the largest are 51,893 / 44,799 / 41,562 / 39,335 / 36,427 words [M]. The owner ordered splitting
plans into files after measured anchor rot inside monoliths [C, owner's private notes]; the architect's PATCH
rule exists because re-emitting "five figures of words" per round costs most of an agent and risks
damaging settled sections [C].

**L13. Context packs are hand-assembled per script.** 118 scripts define an `HDR` block with
worktree, branch, base commit, toolchain, forbidden operations, current defect state and dates [M];
77 scripts reference `OPEN-QUESTIONS`, 84 point agents at `docs/` files to read [M]. Each is written
by the orchestrator from memory and can carry stale premises — a rule in the owner's private notes
against unverified premises in sub-agent briefs exists because 13 of the orchestrator's prescriptions
were refuted by implementers, each costing a full round [C].

**L14. Token cost.** `CLAUDE.md` is 4,593 words and is loaded into every agent (no role sets
`omitClaudeMd` [M]); role prompts are 1,440–3,329 words each [M]; plans are passed inline; a
synthesis agent reached **673,204 cache-creation tokens** and a 140 KB JSON return [C]; `journal.jsonl`
is the only reliable copy of long results because task output files truncate at ~30 KB [C].

**L15. Parallel lanes coordinate through prose.** File ownership is stated in briefs; `git commit`
takes the whole index and once shipped another agent's staged deletion (red and already pushed) [C];
an agent's `taskkill /IM cargo.exe` would have killed three lanes' builds [C]; lanes wait on files
owned by other sessions [C]. There is no shared, live view of "who owns what, who is blocked on whom".

---

## 8. Synthesis (d) — moirai integration points

Operation names below are **proposals** for moirai's CLI/MCP surface, chosen to match the moments in
the role contracts. "Node/edge" refers to §6.

### 8.1 Per-role integration table

| Role | Moment | moirai operation (CLI ≈ MCP tool) | Nodes / edges written or read |
|---|---|---|---|
| orchestrator | session start / resume | `moirai brief --lane <l>` → checkpoint, live runs, blocked tasks, open owner questions | reads Checkpoint, Run, Task(status), OpenQuestion(OPEN) — replaces the top of `MEMORY.md` |
| orchestrator | new request | `moirai task new --campaign C --title … [--part-of T]` | Task, `part_of` |
| orchestrator | ambiguity (VALUES/SCOPE) | `moirai question add --kind scope --blocks T --options …` | OpenQuestion, `blocks` |
| owner (via orchestrator) | ruling | `moirai question resolve Q --ruling "<verbatim>" --by owner` | status→RESOLVED (monotone), Decision `decided_by owner`, unblocks T |
| orchestrator | before each spawn | `moirai pack T --role developer --budget 12k` → budgeted context pack (rules applying to role, plan sections for the step, open findings, env/worktree/base commit) **with an explicit "dropped: N items of kind K" footer** | reads Rule `applies_to`, Plan, Finding, Lane — replaces `HDR` and `.slice()` |
| researcher | before searching | `moirai find research --topic … --since …` | prior Research, Source (avoid repeats; detect staleness) |
| researcher | end | `moirai research add --question … --sources …` | Research, Source(url, version, date, fact/opinion), `cites` |
| architect | start | `moirai pack T --role architect` | Research `derived_from`, prior Decisions, rejected Candidates with `reopens_if` |
| architect | end of rev 1 | `moirai plan put P --section <h> --text …` (section-granular) + `moirai decision add --alternatives …` | Plan, PlanSection, Decision, Alternative `rejected_for`, target Measurement specs, Task steps `part_of` plan |
| architect | rev N ≥ 2 | `moirai plan patch P --section <h> --remove "<verbatim>" --add … --depends-on <h2,h3>` | PlanRevision `supersedes`; the "silent drop" guard becomes a check (removed text must match stored text) |
| architecture-critic | review | `moirai finding add --target P#section --severity blocker --consequence … --confidence plausible` ; `moirai verdict set P --role critic --round k --outcome fail-fixable` | Finding, Verdict, `raised_by critic` |
| critic (round k+1) | scope | `moirai diff P --since-round k` → only changed sections + dependents | reads PlanRevision, `depends_on` (enforces "round scope = delta") |
| refuter | between critic and fixer | `moirai finding refute F --evidence "<cmd/grep>"` / `confirm F` | Refutation, `refutes`/`confirms`; only CONFIRMED findings block |
| orchestrator | loop control | `moirai query "findings(target=P, status=confirmed, severity>=important, open)"`; `moirai stats refuted-share --role critic --round k` | termination = empty confirmed-blocker set |
| developer | start | `moirai pack T --role developer` ; `moirai claim-files T --paths …` | Task, `owns` file set (conflict = error, not prose) |
| developer | ambiguity | `moirai question add --kind unclear --blocks T --to orchestrator` ; `moirai task status T blocked` | OpenQuestion, `blocks` |
| developer | end | `moirai impl report T --implements <decision ids> --deviation "…"` | ImplementationReport, `implements`, Deviation (open until architect confirms) |
| code-reviewer | review | `moirai finding add --target <file:symbol@commit> --failure … --confidence confirmed --kind perf\|correctness\|…` | Finding (kind drives closure rule) |
| code-reviewer | re-review | `moirai finding close F --addressed-by <commit/change> --verified-by review` | `addresses`, `verifies`; **policy: a perf/complexity finding cannot be closed by a test run alone** |
| tester | start | `moirai pack T --role tester` → *Metrics and validation* section, expected test counts, baselines with environment | Plan targets, Measurement baselines |
| tester | end | `moirai testrun add --cmd … --ran N --passed … --failed … --skipped …` ; `moirai gate add --red-when "…" --mutation-proved` ; `moirai measure add --metric … --value … --env <host,profile,load,scale> --commit <sha>` | TestRun, Gate, Measurement `measured_on`, Failure → `blocks` T |
| results-analyst | verdict | `moirai query "metrics(plan=P) vs targets"` (computed Delta/Status); `moirai verdict set T --role analyst --outcome rework --return-to developer --criteria …` | Verdict `returns_to`, TechDebt, KnownLimitation |
| project-analyst | audit | `moirai find findings --scope <path>` before; `moirai finding add --kind security --id-scope global` after | Finding with globally unique ids; links to Task if it becomes work |
| doc-writer | start / end | `moirai page deps <page>` ; `moirai page put <page> --depends-on <decision/measurement ids>` | DocPage `derived_from`; pages flagged stale when a dependency is superseded |
| orchestrator | commit / merge | `moirai commit --lane l --git <sha>` ; `moirai merge <branch>` | moirai commit carries role, run id, model, effort, git sha |
| orchestrator | freeze a losing candidate | `moirai candidate freeze X --tag <git tag> --lost-by <measurement> --reopen-if "<condition>"` | Candidate, `rejected_for`, `reopens_if` |
| orchestrator | lesson learned | `moirai rule add --applies-to tester --provenance owner\|derived --supersedes R` | Rule, `supersedes`, `applies_to` |

### 8.2 Harness wiring points

| Point | Use | Status |
|---|---|---|
| Sub-agent frontmatter `mcpServers` | expose moirai's MCP tools per role — lets read-only roles (architect, critic, researcher, project-analyst) write **their own node kinds** without getting Write/Edit on files, which removes L2 without breaking separation of duties | field exists [D] |
| Sub-agent frontmatter `memory` (`user` / `project` / `local`) | alternative per-agent memory directory; moirai could replace or back it | field exists [D]; unused [M] |
| `SubagentStart` / `SubagentStop` hooks | record a Run node automatically (agent type, id, transcript path); detect "0 errors but no product" by checking the promised node exists | events exist [D]; payload details and whether `SubagentStart` can inject context **not verified** |
| `SessionStart` / `PreCompact` | inject `moirai brief`; snapshot checkpoint before compaction | events exist [D]; injection semantics not verified |
| `TaskCreated` / `TaskCompleted` | mirror harness tasks into moirai tasks | events exist [D] |
| Workflow scripts | replace `${…}` interpolation and `.slice()` with node ids + `moirai pack`; replace `HDR` with `moirai pack --lane` | [I] requires agents to have moirai access (MCP or CLI via Bash) |
| A role-catalogue health check | `moirai doctor agents` parses every `.claude/agents/*.md` frontmatter and reports skips (would have caught `results-analyst`) | [I] |

### 8.3 Requirements on moirai that this lens implies

1. **Role/actor identity on every write**, plus a declarative **write policy per role** (critic may
   create Finding/Verdict on Plan but not edit Plan; developer may not set a Verdict on its own work;
   a Finding closes only with `addresses` + `verifies` from a different role). This turns the
   separation-of-duties prose into enforcement.
2. **Typed statuses with guarded transitions and a canonical outcome class** (§6.3), keeping the
   role's raw label.
3. **Branch-aware merge with per-field semantics**: status fields merge along a lattice
   (OPEN < RESOLVED; reopening is an explicit event, never a merge artefact); counters and measured
   values are not merged as text but recomputed/re-measured; free text merges as text. This is the
   direct answer to L5/L6.
4. **Supersede-in-place reads**: default queries return the current value; history stays reachable
   (git-like), never the other way round (L7).
5. **Stable ids and bidirectional edges**; references by id, never by line number; every edge
   queryable from both ends (L8). Deleting a node leaves a tombstone that every referrer sees
   immediately (the owner's "node 40 deleted" requirement) — consistent with the project convention
   "mark RESOLVED rather than delete".
6. **Section-granular plans** so a patch is a first-class diff, "round scope = delta" is a query, and
   the verbatim-removed-text guard is mechanical (L12).
7. **Budgeted context packs with explicit drop accounting** (L1, L13, L14).
8. **Measurement nodes with a mandatory environment and git commit**, and an ancestor check
   ("does the measured commit contain fix X?") delegated to git (L10).
9. **Provenance on rules and claims**: owner-verbatim vs agent-derived, VERIFIED (with command) vs
   ASSUMPTION; a claim can be `refuted` without deleting it (L3's misdiagnosis, L7).
10. **Live lane view**: file ownership claims, cross-lane `blocks`, and "who is running what" (L15).

**Scale estimate [I]:** today's whole textual state is ~0.9 M words (733,603 in `docs/*.md` +
182,746 in memory) [M] across 1,882 commits [M]. Even fully atomised into findings, decisions,
measurements and sections, that is on the order of 10⁴–10⁵ nodes — small enough that moirai's RAM
target can be met with a compact on-disk format and a small hot index; the hot queries are
`blockers(T)`, `open findings(P)`, `pack(T, role)`, `diff(P, round)`, and status-lattice merges.

---

## 9. Open questions for the owner

1. **Source of truth vs view.** Should moirai *replace* `OPEN-QUESTIONS.md`, `BACKLOG.md`,
   `MEASUREMENT-QUEUE.md`, plan files and the memory directory, or remain the store from which
   Markdown views are generated (so git review of prose stays possible)?
2. **Commit policy.** `CLAUDE.md` and `developer.md` say "commit only on explicit request"; the
   owner's private notes say to commit and push automatically. Which one should moirai encode as the rule?
3. **Write access for read-only roles.** May architect / critic / researcher / project-analyst write
   their own node kinds (plans, findings, research) into moirai via MCP, while still having no file
   Write/Edit?
4. **Store topology.** One live store shared by all worktrees and lanes (instant cross-lane
   visibility), or one store per git branch that merges with the code? Should moirai branches map
   1:1 onto git branches?
5. **Deletion semantics.** Is a true delete ever wanted, or is every removal a tombstone
   ("RESOLVED, not deleted"), with referrers notified?
6. **Language of stored text.** The memory index and checkpoints are in Russian, repository
   artifacts in English. Which should moirai node text use, and must both exist?
7. **Owner identity.** Should rulings carry the owner's verbatim words as a distinct actor that no
   agent can impersonate?
8. **Plan storage.** Full plan text as section nodes in moirai, or pointers to Markdown files
   (split per the owner's plan-splitting rule)?
9. **Migration.** Import the 255 memory files and the register histories, or start clean and link
   by reference?
10. **Model and effort routing as data.** Keep in frontmatter and scripts, or make role policy
    (model, effort, tools, write permissions) moirai data read by the harness?
11. **Out of scope here, but found:** `results-analyst.md` is skipped by Claude Code because of its
    frontmatter; the memory note about it records the wrong cause. Do you want that reported to the
    BoykoEngine session (this research does not touch that repository)?

---

## 10. Sources

### Local (all in the BoykoEngine repository, HEAD `49f2fcfb`, 2026-09-22, unless noted)

- `.claude/agents/{architect,researcher,architecture-critic,developer,code-reviewer,tester,results-analyst,project-analyst,doc-writer}.md` — read in full.
- `.claude/settings.json`, `.claude/hooks/clarify_gate.py`, `.claude/hooks/graphify_read_gate.py`.
- `CLAUDE.md` lines 125–266 (§Documentation layers, §Agents, §Orchestration discipline, §Communication, §Rules for agents).
- Git commits: `457a04fe`, `4444b30b`, `6fa64383` (2026-05-23); `4cb9f0da`, `4417e6a2` (2026-07-09); `fef01042` (2026-07-27); `f20bdafe`, `b0bff31f`, `d4ee35f9` (2026-09-17); merges `e09abb9b` (2026-09-21), `2e543e55` (2026-09-23).
- `docs/OPEN-QUESTIONS.md`, `docs/BACKLOG.md`, `docs/MEASUREMENT-QUEUE.md`, `docs/REMAINING-GAPS.md` — headers, conventions and statistics only.
- `docs/*PLAN*.md` — word counts and revision numbers only.
- Memory: the owner's private Claude Code memory (`~/.claude/projects/<project>/memory/`, not published) — `MEMORY.md` (index, top section) and 25 workflow-related topic files.
- Workflow scripts: 347 `*.js` files in the owner's private Claude Code project state (not published) — statistics only; one evidence-and-design script inspected for structure.

### External (fetched 2026-09-25)

- Claude Code — sub-agents reference: https://code.claude.com/docs/en/sub-agents — frontmatter fields (`tools`, `disallowedTools`, `model` incl. `fable`, `permissionMode`, `maxTurns`, `skills`, `mcpServers`, `hooks`, `memory`, `background`, `omitClaudeMd`, `effort`, `isolation`, `color`, `initialPrompt`, `experimental`); nesting "up to three layers below the main conversation"; invalid YAML → file skipped, error only in debug log; only the top-level sub-agent's summary returns.
- Claude Code — hooks reference: https://code.claude.com/docs/en/hooks — event list including `SessionStart`, `SubagentStart`, `SubagentStop`, `TaskCreated`, `TaskCompleted`, `PreCompact`, `PostCompact`, `WorktreeCreate`; matcher for sub-agent events is the agent type. Context-injection semantics for `SubagentStart`/`SessionStart` were **not** confirmed from the fetched text.
- arXiv 2509.01494, "SWR-Bench: Assessing LLM Performance in Real-World Code Review Comment Generation", Zeng et al., submitted 2025-09-01: https://arxiv.org/abs/2509.01494 — 1,000 manually verified PRs confirmed; the precision range cited by `code-reviewer.md` is not in the abstract.
- arXiv 2603.18740, "Measuring and Exploiting Contextual Bias in LLM-Assisted Security Code Review", Alexopoulos et al., submitted 2026-03-19: https://arxiv.org/abs/2603.18740 — abstract reports 97% (32/33) for a bias-exploitation attack; narrower than the role file's paraphrase.
- Cited by role files, not re-verified: Kamoi et al., TACL 2024, https://arxiv.org/abs/2406.01297; arXiv 2603.00539, https://arxiv.org/html/2603.00539v1.
