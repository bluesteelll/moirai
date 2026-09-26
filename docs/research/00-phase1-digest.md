

# wf-roles (docs/research/01-boyko-workflow-roles.md)

## Summary
Scope: all 9 role files in the BoykoEngine repository's `.claude/agents/` read in full (HEAD 49f2fcfb, 2026-09-22). I also read the workflow parts of CLAUDE.md, the hooks, the git history of the agent files, 25 workflow-related memory files, and statistics only from the docs registers and 347 Workflow scripts. No engine content was studied, nothing was built, and nothing outside the report file was changed. Every fact is tagged: measured by me [M], claimed by a source [C], documented externally [D], or my inference [I].

Pipeline: one orchestrator (the main chat, and since August 2026 also Workflow scripts) drives researcher -> architect <-> architecture-critic -> developer(s) <-> code-reviewer -> tester -> results-analyst -> commit/merge -> doc-writer. project-analyst works outside this path as a read-only analyst. All 9 roles set model: opus. None uses the effort, memory, isolation, hooks, mcpServers or omitClaudeMd frontmatter fields [M], although Claude Code supports them all [D]. Scripts set model and effort per call instead: fable 145 times, opus 109; effort high 284 times [M]. The model-routing rule exists in four places that disagree in wording (frontmatter, CLAUDE.md, two memory files).

Per-role contracts (full cards are in the report):
- **architect:** a fixed plan template (goal, target metrics, decisions with What/Why/rejected Alternatives/Trade-off, implementation steps, metrics and validation, open questions) plus a readiness checklist. From revision 2 it returns a PATCH: removed text quoted verbatim, added text, and dependent sections. It has no Write access.
- **architecture-critic and code-reviewer:** verdict APPROVED or CHANGES REQUESTED, with red/yellow/green remarks C#/W#/O#. Each remark must carry a Consequence or Failure field and a Confidence of CONFIRMED or PLAUSIBLE. "No remarks" is a valid result, and neither may approve while a red or yellow remark is open.
- **developer:** implements exactly what the plan says, reports deviations, and stops to escalate when the plan is unclear. It does not test or commit.
- **tester:** stops if the build fails, has strict anti-vacuity rules, and reports benchmarks against the plan's targets.
- **results-analyst:** returns ACCEPTED, REWORK or RETHINK using a 10-item checklist and numeric thresholds, and names which phase the work returns to.

Measured problems:
1. results-analyst.md has invalid YAML frontmatter (": " inside its description). Claude Code silently skips such files [D]. The owner's private notes record the "agent type not found" symptom but give the wrong cause. Scripts use agentType 'claude' as a workaround and never use results-analyst (0 of 347 scripts).
2. Role prompts carry stale context: "three crates" when there are 27; an "ecs branch" that was merged on 2026-07-09; a git-show instruction given to a role that has no Bash.
3. Rules contradict each other: the commit policy has three sources and two positions. The critic is told to re-read the whole plan, but the architect's patch protocol and the owner's rule that a round reviews only the delta say otherwise.
4. About a dozen learned rules exist only in memory and hand-written script headers, not in the role files. Examples: the refutation pass between critic and fixer, a retest not counting as a re-review, freezing rejected candidates with a revival condition, one worktree per system, splitting plans into files, and keeping unverified premises out of briefs.

Implicit state machine: Proposed -> (NeedsOwner) -> Researching -> Designing -> DesignReview -> Refuting -> DesignApproved -> Implementing (-> Blocked) -> CodeReview -> Testing -> Analysis -> Accepted -> Committed -> Merged -> Documented. REWORK or RETHINK routes back to the architect, developer or tester. Side states are Deferred, Frozen (losing candidate) and MeasurementQueued. There are 8 gates (G0 scope through G7 public push). Review loops have no round cap. By the owner's rule, a loop ends when the list of confirmed blockers is empty after the refutation pass.

Implicit data model: Campaign/Lane, Task/Subtask, Plan, PlanRevision/Patch, Decision, Alternative/Candidate, Research/Source, Finding, Refutation, Verdict, ImplementationReport, Test/Gate, TestRun, Measurement (with its environment and commit), Rule, Lesson, OpenQuestion, Deferral/Backlog item, QueuedMeasurement, TechDebt, Claim, Run, Checkpoint and DocPage. The edges are part_of, blocks, depends_on, implements, addresses, refutes/confirms, verifies, supersedes, rejected_for, derived_from, measured_on, returns_to, raised_by/decided_by, applies_to, owns, cites, mirrors, reopens_if and contradicts. Scripts use 15 different verdict vocabularies [M]; the report proposes one canonical set of outcome classes.

Where the current mechanism loses information or wastes effort:
- Hand-offs are passed as `${}` text and cut with `.slice()` in 72 of 347 scripts. Four truncation incidents are recorded, and each was noticed by an agent, not the orchestrator.
- Read-only roles cannot save their output. One workflow reported 4 agents done and 0 errors when no plan file existed.
- Registers are free-text prose: OPEN-QUESTIONS.md is 68,452 words. Rulings made on one branch never reached another, and a union merge printed decided items as OPEN again in 10 places.
- Prose merges get counters wrong (the text said 188; the true count was 191).
- Corrections are added as pointers, so retracted numbers get quoted.
- 184 of 282 line anchors are dead [C], and addressing another plan by document name creates no back-link.
- Findings are not objects. Their identity across review rounds exists only in the orchestrator's context.
- Measurements are not tied to host, load, scale or commit.
- Memory is 255 files and 182,746 words. Its index is 21,966 bytes and near a claimed load limit, and 78 files are not linked from it.
- There are 50 plan files totalling 470k words.
- 118 scripts use hand-built context headers.
- CLAUDE.md (4,593 words) loads into every agent. One synthesis agent built 673k tokens of context.
- Parallel lanes coordinate file ownership through prose.

moirai integration (proposed operations):
- **Orchestrator:** `brief` at session start and `pack` (a context pack under a token budget that reports what it dropped) before every spawn.
- **Architect:** `plan put` and `plan patch`, stored per section, plus `decision add` with alternatives.
- **Critic and reviewer:** `finding add` (severity, failure scenario, confidence, kind) and `verdict set`.
- **Refuters:** `finding refute` or `finding confirm`.
- **Developer:** `claim-files`, `question add --blocks`, and `impl report --implements`.
- **Tester:** `testrun`, `gate --red-when --mutation-proved`, and `measure --env --commit`.
- **results-analyst:** compares metrics with targets and sets a verdict with `--return-to`.
- **Other writers:** project-analyst files findings with globally unique IDs; doc-writer records `page deps`; freezing a losing candidate uses `candidate freeze`.
- **Harness hooks:** read-only roles get moirai access through `mcpServers`; SubagentStart/Stop hooks record Run nodes, and SessionStart and PreCompact hooks handle checkpoints [D]. Whether these hooks can inject context was not verified.

Requirements this lens puts on moirai:
- Every write records the role that made it, and each role has a write policy.
- Statuses are typed and move only through allowed transitions.
- Branch merges are field-aware: an OPEN/RESOLVED status can only move forward, and counters are recomputed rather than merged as text.
- A normal read returns the current value; history stays available.
- Nodes have stable IDs and every link can be followed from both ends. Deleting a node leaves a marker that every node linking to it sees at once.
- Plans are stored per section, so a patch is a real diff.
- Every measurement carries its environment and git commit.
- Rules and claims record where they came from and whether they were checked.
- A live view shows which lane owns which files and who is blocked on whom.

Estimated scale: 10^4 to 10^5 nodes, which is compatible with the RAM goals.

## Implications
- [high] Role outputs are fixed Markdown templates that amount to an unstored typed schema (decisions/alternatives/trade-offs, severity-tiered remarks with mandatory Failure/Consequence + Confidence, test failures, metrics vs targets, verdict + return-to phase). | EVIDENCE: All 9 files in the BoykoEngine repository's .claude/agents/ read in full [M]; templates quoted per role in report section 2. | IMPLICATION: moirai's first node/edge kinds should mirror these templates (Plan, PlanSection, Decision, Alternative, Finding, Verdict, TestRun, Measurement, OpenQuestion) so roles fill fields instead of prose; the templates are a ready-made schema spec.
- [high] Read-only roles (architect, critic, researcher, project-analyst) cannot persist their product; workflows reported success while the plan file never existed. | EVIDENCE: Frontmatter tools lists [M]; 35 KB and 70 KB plans returned as text, the critic could not review them, and the workflow reported 4 agents done with 0 errors [C, owner's private notes]. | IMPLICATION: Expose moirai via MCP (mcpServers frontmatter [D]) with per-role write policy so these roles can write only their own node kinds (plan sections, findings, research) without file Write/Edit; add a 'promised node exists' check at SubagentStop.
- [high] Hand-offs between agents are string interpolation with silent truncation, and agent() returns only the last message. | EVIDENCE: 72 of 347 Workflow scripts use .slice(0, N) [M]; four recorded truncation incidents including a critic receiving a plan cut mid-section and two architect outputs split across two messages [C, owner's private notes]. | IMPLICATION: moirai needs a budgeted 'context pack' query keyed by task + role that selects by priority per item kind and reports dropped counts explicitly; hand-offs pass node ids, not text.
- [high] Status registers are append-only prose, and git merges corrupt status claims: rulings on one branch never reached another, union merge resurrected RESOLVED items as OPEN in 10 places, identical counter edits merged into a wrong total. | EVIDENCE: docs/OPEN-QUESTIONS.md 68,452 words / 87 entries / 95 commits [M]; its 2026-09-03 entry on 12 lost rulings [C]; union-merge and identical-edit incidents [C, owner's private notes]. | IMPLICATION: moirai's git-like versioning must merge per field with semantics: status fields on a monotone lattice (reopen only as explicit event), counters/measurements recomputed not text-merged, free text merged as text. This is a core design requirement, not a nicety.
- [high] Corrections are appended as pointers, so retracted values keep being read and quoted. | EVIDENCE: A summary kept being quoted after a later evidence row had superseded its numbers [C, owner's private notes]; register convention 'mark RESOLVED rather than delete' [M]. | IMPLICATION: Default reads must return the current (superseding) value with history reachable via supersedes edges; deletions become tombstones visible to all referrers, matching the owner's 'node 40 deleted' requirement.
- [high] Line-number and by-document references rot and create no back-links. | EVIDENCE: 184 of 282 anchors dead [C, owner's private notes]; a collision-prevention sentence invisible from both plans because it addressed the other plan by document [C, owner's private notes]; ~6 real dangling wiki-links and 78 unindexed files in the memory corpus [M]. | IMPLICATION: Stable ids, bidirectional edge indexes, dangling-reference detection and plan sections as addressable nodes; never store line numbers as identity.
- [high] The final-verdict role is not loadable: results-analyst.md frontmatter is invalid YAML, Claude Code silently skips it, and project memory recorded a wrong cause. | EVIDENCE: PyYAML: 'mapping values are not allowed here' only for results-analyst.md [M]; docs: YAML that does not parse -> file skipped, error only in debug log [D, code.claude.com/docs/en/sub-agents, 2026-09-25]; the owner's private notes record a wrong cause [C]; 0 uses of agentType 'results-analyst' in 347 scripts [M]. | IMPLICATION: moirai should model claims with verification status and 'refutes' edges (a wrong recorded cause becomes refutable, not silently authoritative), and could offer an 'agents doctor' check of role definitions.
- [high] About a dozen learned workflow rules (refutation pass, round scope = delta, retest is not re-review, freeze rejected candidates, worktree per system, split plans, verified premises in briefs) exist only in memory files and per-script headers, and some contradict role files or CLAUDE.md (commit policy, critic re-read scope, model routing). | EVIDENCE: Report section 3 table [M]; 118 scripts define hand-written HDR blocks [M]; commit policy stated differently in CLAUDE.md, developer.md and the owner's private notes [M]. | IMPLICATION: Rule nodes with applies_to (role/phase), provenance (owner-verbatim vs derived), supersedes and contradicts edges; context packs inject exactly the rules that apply to the spawned role, replacing hand-maintained headers and making contradictions queryable.
- [high] Review-loop termination depends on finding identity across rounds and on refutation outcomes, which currently live only in the orchestrator's context. | EVIDENCE: Remark ids are per report (C1/W1) [M]; owner rule: stop when confirmed-blocker list after refutation is empty, no round cap, count refuted share [C, owner's private notes]; loops once ran to 30 rounds [C]; 174 scripts mention refutation [M]. | IMPLICATION: Findings need globally stable ids, status across rounds, refutation edges and a closure policy by finding kind (perf/complexity findings closable only by re-review + scaling guard, not by a test run); the termination condition becomes a query.
- [medium] Measurements are only meaningful with environment context that today lives in surrounding prose (host triple change on 2026-09-17, idle-machine rule, data scale, whether the measured commit contains the fix). | EVIDENCE: tester.md dated traps [C]; agents breaking the idle rule, and a summary quoting an L1-scale microbenchmark [C, owner's private notes]; results-analyst numeric thresholds vs plan targets [M]. | IMPLICATION: Measurement nodes require env fields + measured_on git commit; targets are typed fields on plan nodes so metric-vs-target status and regressions are computed, and ancestor checks can be delegated to git.
- [medium] Parallel lanes coordinate file ownership and blocking through prose in briefs, leading to index races, a near machine-wide kill and lanes waiting on files owned by other sessions. | EVIDENCE: developer.md parallel section [M]; one worktree per system, a foreign staged deletion committed and pushed, and a machine-wide process kill that would have hit other lanes [C, owner's private notes]. | IMPLICATION: moirai should hold live, cross-worktree lane state: owns(file set) claims with conflict errors, cross-lane blocks edges, and run registry; this argues for a store shared across worktrees (owner decision needed).
- [medium] Cross-session state is a hand-maintained memory corpus near its load limit, plus a manually written resume checkpoint. | EVIDENCE: 255 files, 182,746 words, 2.45 MB; MEMORY.md 21,966 bytes; 78 files unindexed; checkpoint lines with run ids and trunk commits, in Russian [M]; claimed 24.4 KB index load limit [C]. | IMPLICATION: A 'moirai brief' query at SessionStart (hook exists [D]) can replace the checkpoint section and index, returning only live tasks, runs, blockers and open owner questions within a budget.
- [medium] Plans are huge and revised by patch; reviewers should read only changed and dependent sections. | EVIDENCE: 50 plan files, 470,311 words; largest 51,893 words; the 36,427-word / Rev 39 figure cited in architect.md verified exactly [M]; owner rule to split plans [C, owner's private notes]. | IMPLICATION: Store plans as section nodes with depends_on edges and revision history; 'diff since round k' plus dependents becomes the critic's input, and the verbatim-removed-text guard becomes a mechanical check.
- [medium] Verdict vocabularies are inconsistent across roles and scripts. | EVIDENCE: 15 distinct verdict enum shapes in 347 scripts plus template vocabularies APPROVED/CHANGES REQUESTED and ACCEPTED/REWORK/RETHINK [M]. | IMPLICATION: Store the raw role label plus a canonical outcome class (pass / pass-with-conditions / fail-fixable / fail-fundamental / unknown / n-a) and a target kind, so cross-role queries work.
- [low] Scale of the whole textual state is modest. | EVIDENCE: 733,603 words in docs/*.md + 182,746 in memory; 1,882 commits [M]. | IMPLICATION: Estimated 10^4-10^5 nodes when atomised [I]; the RAM/performance targets are achievable with a compact on-disk format and a small hot index; the hot queries are blockers(T), open findings(P), pack(T, role), diff(P, round), status merges.

## Open questions
- Should moirai replace docs/OPEN-QUESTIONS.md, BACKLOG.md, MEASUREMENT-QUEUE.md, plan files and the memory directory, or be the store that generates Markdown views of them, so prose can still be reviewed in git?
- Commit policy conflict: CLAUDE.md and developer.md say commit only on explicit request; the owner's private notes say commit and push automatically. Which rule should moirai encode?
- May the read-only roles (architect, architecture-critic, researcher, project-analyst) write their own node kinds into moirai through MCP, while still having no file Write/Edit?
- Store topology: one live store shared by all worktrees and lanes (instant visibility across lanes), or one store per git branch that merges with the code? Should moirai branches map 1:1 onto git branches?
- Deletion: is a true hard delete ever wanted, or is every removal a tombstone ('RESOLVED, not deleted') that notifies every node linking to it?
- Language of stored node text: the memory index and checkpoints are in Russian, repository artifacts in English. Which language should moirai use, and must both exist?
- Should owner rulings be stored as a distinct actor with the owner's verbatim words, so no agent can write one?
- Plans: full text stored as section nodes in moirai, or pointers to split Markdown files?
- Migration: import the 255 existing memory files and the register histories, or start clean and link to them?
- Model/effort/tools policy per role: keep it in agent frontmatter and scripts, or make it moirai data that the harness reads?
- Out of scope, but found: results-analyst.md in BoykoEngine is silently skipped because its frontmatter is invalid YAML, and the memory note about it records the wrong cause. Should this be reported to the BoykoEngine session?


# wf-orchestration (docs/research/02-boyko-workflow-orchestration.md)

## Summary
This is a read-only study of the BoykoEngine agentic workflow, limited to orchestration. Engine content was skipped. Sources:
- the workflow sections of CLAUDE.md;
- the hooks and settings in `.claude/settings.json` and `.zcode/config.json`;
- 9 agent definitions;
- 38 Workflow scripts (392,819 B), 19 of them read in full or in their control flow and all 38 measured for pattern prevalence;
- the harness run journals, measured for structure and size only;
- the auto-memory mechanism: MEMORY.md format, frontmatter schema, link graph, 17 workflow and staleness memory files read;
- read-only `git worktree list` and `git branch --list`;
- external docs, fetched 2026-09-25: Claude Code memory, hooks and sub-agent docs, git-worktree 2.54.0, MCP spec 2025-06-18, and the harness `workflow-authoring` reference.

Facts are tagged Measured, Claimed, Inferred or Documented throughout.

**How the workflow works**
- **Orchestrator.** The main session decomposes owner orders into campaigns, then phases, then lanes (one git worktree plus branch plus build-target dir each, under `<lanes-dir>/<system>`), then rungs/steps (commit-sized), then rounds/passes, then findings.
- **Agents.** Nine roles, all on Opus. Separation of duties: developer does not test, reviewer does not fix, critic does not design. architect, critic, analyst and researcher cannot write files, so scripts add a "writer" step.
- **Hooks.** Three fail-open nudges: clarify-before-acting on `UserPromptSubmit`, and graphify-first on `PreToolUse`. SessionStart, WorktreeCreate, SubagentStop and PreCompact are not wired in the project settings.
- **Workflow volume (measured).** 401 runs in 17 sessions, about 11 per day in September. Agents per run: p50 4, p90 14, max 108. 3,294 agents started; 162 (4.9%) died. Result payload p50 8.8 KB, max 153 KB.
- **Script skeleton.** `meta`, then absolute-path constants for the current session's scratchpad (SP) and older sessions' (OLD), then a long HDR brief, then schemas and verdict parsers, then phases and bounded loops, ending in a status return.
- **Recurring patterns:**
  - multi-lens survey → design → critique → revise → writer;
  - implement → parallel test and review → adversarial triage (each finding CONFIRMED, REFUTED or DEFERRED) → fix rounds of at most 3;
  - numbered repair passes with an independent verifier (an 8-pass citation-repair campaign);
  - parallel lanes in separate worktrees;
  - pre-registered decision bands that gate later stages;
  - commit split with blob-hash proofs;
  - a parameterized trunk-merge script chained by nested `workflow()` into a sequential merge queue gated on GREEN;
  - quiet-window preparation, where no agent may run during timed passes;
  - judge panels;
  - prompt-level "RESUMED RUN" with pre-snapshots and sha256 tree manifests.
- **Verdicts.** Mostly free text parsed by regex: first-line GREEN/DONE in 12 scripts, `CRITICAL=n;IMPORTANT=n` in 16, harness-notice stripping in 10. Only 18 of 38 use schemas.
- **Boilerplate.** The same dozen standing rules are copy-pasted into briefs: English in 36/38, git bans in 26/38, RUSTFLAGS in 22/38, cwd-reset in 11/38. New bans were added only after incidents.

**Where state lives: five unlinked stores**
- The harness `journal.jsonl`: content-hash keys, resumable only within a session.
- Session-scoped scratchpads: reports, rulings, manifests, patches, reusable scripts. They reach 7.8 GB per session, and new runs reference old sessions' scratchpads by absolute path.
- Git: 44 worktrees, 107 local branches, 49 `u/*`.
- Auto-memory: 254 topic files plus the index, 2.45 MB.
- In-repo registers: rulings, open questions, measurement queue.

Scripts end up in 21 different cwd-encoded directories.

MEMORY.md is 72 lines and 21,966 B, which is 88% of the harness's 25 KB load cap. Its resume block has 8 stacked checkpoint lines, and only the newest is current. The lines hold:
- session ids, background-task ids, workflow ids;
- worktree paths, branches, trunk sha and whether it was pushed;
- uncommitted-file counts, merge order;
- owner STOP points, decisions, cron-watchdog ids.

That block points to a 141 KB campaign log with no headings, holding 100 task ids, 78 run ids and 51 branch names, whose tail is a stale 09-18 plan. Frontmatter is name, description, and metadata with node_type, type, an origin-session id and modified; 7 files have no frontmatter, and only 148 have `modified`. 78 files are not in the index and 32 are unreachable from any link. Of 701 wiki-links, 20 occurrences (13 distinct targets, about 8 real) do not resolve.

**Failure modes of the current state keeping**
- Stale index lines.
- Retractions that do not reach summaries.
- Union merges that resurrect pre-ruling OPEN statuses (10 places).
- Identical counter edits auto-merged into one.
- Silent input and output truncation (4 incidents).
- Shell-mangled memory writes.
- `file:line` citation rot.
- Huge monolithic records.
- "0 errors" runs whose promised files are missing.
- Run ids not portable across sessions.
- Per-run facts hard-coded into briefs.
- Research claims quoted as owner rulings.

**What moirai needs**
- **Node kinds:** Campaign, Phase, Lane, Task/Rung, Run (optionally AgentCall), Session, Report/Artifact (path plus sha256), DesignRev (patch fold), Finding (status machine), Ruling/Decision (with authority), OwnerQuestion, Gate/Pin/GateResult/Mutation, MeasurementItem, Candidate (frozen, with a revive condition), Rule (`applies_to`), Hazard, Note, Role, Checkpoint.
- **Edge kinds:** `subtask_of`, `blocks`/`depends_on`, `merge_after`, `runs_in`, `produced`/`consumed`, `patches`, `raises`, `confirms`/`refutes`, `rules_on`, `fixes`, `supersedes`/`retracts`, `derived_from`, `gated_by`, `pins`, `measured_at`, `touches`, `owned_by`, `applies_to`, `cites`, `resumes`.
- **Consistency:** tombstones instead of hard deletes; authority computed from supersession edges; stale flags propagating transitively along `derived_from`; reverse-edge indexes; read-after-write hashes.
- **Branching:** one branch per lane, mirroring the git worktree. The merge must be typed, not textual: status lattice where ruled beats open, counters as deltas, superseded records kept but non-authoritative, conflicts surfaced on binding fields. Rules, hazards and owner rulings probably belong in a global namespace.
- **Queries:**
  - a session-start brief of 2–4k tokens, renderable into MEMORY.md;
  - a per-phase context pack that replaces the hand-written HDR: rules for the role, the effective spec, rulings, open findings, gates and pins, other lanes' file footprints;
  - findings and critic refutation stats;
  - a conflict check before fan-out;
  - a merge check;
  - a stale scan.
- **Scale:** roughly 0.3–0.5M nodes over three years; metadata is small. Whether to store bodies decides the footprint, from negligible to 1–3 GB per year.
- **Integration:** CLI taking JSON on stdin; a local stdio MCP server with no network egress, using resource subscriptions for change notices; fail-open hooks on SessionStart, WorktreeCreate/Remove, SubagentStop/TaskCompleted and PreCompact. Workflow scripts have no filesystem access, so the realistic pattern is orchestrator-side pack-then-launch through `args`.
- **Hard constraint:** zero idle CPU (no watchers, polling or background compaction), because of the quiet-window rule, plus crash-safe writes on an unstable host.

**What must stay out:** transcripts and journals, scratchpad bulk and build artifacts, code-symbol indexing, gate execution, LLM rewriting of stored facts, textual union merges, numbers without provenance, one growing record per campaign, policy enforcement, and secrets.

## Implications
- [high] Orchestration state is spread over five unlinked stores (harness journals, session scratchpads, git worktrees/branches, auto-memory, in-repo registers). Only absolute paths and ids pasted into prose connect them. | EVIDENCE: [M] 401 run journals in 17 sessions; scratchpads up to 7.8 GB per session; 44 worktrees and 107 local branches; 255 memory files (2.45 MB). One memory topic file holds 100 task ids, 78 run ids and 51 branch names in 141 KB with no headings. One merge-queue script references three sessions' scratchpads. | IMPLICATION: moirai should be the linking layer: Run, Lane, Report and Session nodes storing ids, paths and sha256 hashes that point into the other stores, not copies of their bulk.
- [high] Resume state is kept as hand-written, stacked index lines; old checkpoints are never retired. | EVIDENCE: [M] The MEMORY.md resume block has 8 checkpoint lines and only the newest is current. The campaign file that line points to ends with a stale 2026-09-18 plan. | IMPLICATION: 'Current state' must be a computed view (latest checkpoint per campaign, live lanes, merge queue), with superseded checkpoints hidden by supersession edges but kept in history.
- [high] Retractions and corrections do not reach summaries or copies. | EVIDENCE: [C, owner's private notes] Numbers were quoted from a summary field after the evidence log had retracted them. [C, owner's private notes] A warning about staleness survived after the fix. | IMPLICATION: Store derived_from edges and propagate stale flags transitively and synchronously when a source is retracted or superseded. This is the 'maximally synchronous' requirement applied to knowledge, not only to deletion.
- [high] Git's text merge corrupts status-bearing records and counters. | EVIDENCE: [C] A union merge of registers printed two RULED ballots as OPEN in 10 places. [C] Identical +3 counter edits on two branches auto-merged to 188 instead of 191. | IMPLICATION: moirai merges must be typed per field: a status lattice where ruled or closed beats open, counters stored as deltas or recomputed, sets unioned, authority decided by edges, and conflicts on binding fields surfaced instead of auto-unioned.
- [high] Parallel work is organised as one git worktree plus branch plus build-target dir per system/lane, merged back one at a time through a GREEN-gated queue. | EVIDENCE: [M] 44 worktrees, 49 u/* branches; per-lane CARGO_TARGET_DIR; a parameterized trunk-merge script chained by nested workflow() in a merge-queue script. MEMORY.md records merge order 'X -> Y -> Z'. | IMPLICATION: moirai needs one branch per lane (forked at the lane's base sha, ideally on the WorktreeCreate hook), merge_after edges, a 'next mergeable lane' query, and a structured lane-vs-trunk diff that feeds the merge brief and commit message.
- [high] Standing rules are copy-pasted into every brief, and new rules reach later briefs only by hand. | EVIDENCE: [M] Over 38 scripts: English rule in 36, git bans in 26, RUSTFLAGS rule in 22, cwd-reset rule in 11, crash-handling rule in 7. [C] The hooksPath and image-name-kill bans were added only after incidents. | IMPLICATION: Rule nodes with applies_to(role, phase, lane) edges, plus a 'context pack' query that composes the brief. A new rule then reaches every future brief as soon as it is recorded.
- [high] Verdicts and findings, the most frequent typed facts, travel as free text parsed by regex. | EVIDENCE: [M] First-line GREEN/DONE parsing in 12/38 scripts, CRITICAL=n;IMPORTANT=n regex in 16/38, '[harness' line stripping in 10/38; schemas used in only 18/38. | IMPLICATION: Provide MCP/CLI operations that record verdicts and findings as typed nodes (status open/confirmed/refuted/deferred/fixed, severity, failure scenario, evidence reference). Later phases query them instead of re-parsing prose.
- [high] Review loops end by adversarially refuting findings, and the owner wants critic quality visible. | EVIDENCE: [C, owner's private notes] Up to 30 rounds; stop when no confirmed blockers remain after refutation; track the refutation rate per critic. [M] Triage steps with CONFIRMED=<n> in the latest scripts. | IMPLICATION: The Finding lifecycle needs refutes/confirms/rules_on edges and aggregate queries (refutation ratio per critic, per round, delta since last round).
- [high] Agent inputs and outputs are silently truncated when passed by value. | EVIDENCE: [C, owner's private notes] 4 incidents (a synthesis saw 3 of 5 lenses; a critic saw a plan cut mid-section; an architect's output split over two messages lost its head). [M] A 20,000-character .slice() cap in one implementation script. | IMPLICATION: moirai results must be complete or explicitly paged ('k of n', total count), and workflows should pass node ids and paths rather than inlined text.
- [medium] Memory writes through the shell were silently corrupted. | EVIDENCE: [C, owner's private notes] Backticks were command-substituted and the tool still printed 'ok'. | IMPLICATION: The CLI must take JSON on stdin (no shell-quoting of content), and every write should return a content hash the caller can check.
- [high] Timed measurements need an idle machine; even agents break the idle rule. | EVIDENCE: [C, owner's private notes] claude.exe costs 6-8% CPU. [M] One quiet-window preparation script's idle check requires CPU < 5% over 10 s and 0 build processes. | IMPLICATION: moirai must use about zero CPU when idle: no file watchers, polling, background compaction or telemetry; maintenance only on explicit command. It also needs crash-safe atomic writes, because the development machine has experienced OS crashes and disk-full events.
- [high] Run ids and scratchpad state are tied to a session and to the working directory. | EVIDENCE: [C, owner's private notes] Run ids from another session cannot be resumed. [M] The 34 historic scripts are spread over 21 cwd-encoded project directories; scripts carry SP and OLD scratchpad paths. | IMPLICATION: Run nodes must store wf_id, background task id, session, script_path, args and journal_path, and lane state must be independent of any session, so a new session can resume from moirai alone.
- [high] The session-start channel is a small, byte-limited file loaded every session. | EVIDENCE: [D] code.claude.com/docs/en/memory: the first 200 lines or 25 KB of MEMORY.md are loaded. [M] MEMORY.md is at 21,966 B (88% of the cap), mostly 2-byte Cyrillic. | IMPLICATION: The session brief must be bounded (about 2-4k tokens). It can be rendered into MEMORY.md or injected by a SessionStart hook's additionalContext.
- [medium] Measured numbers and pins go stale when the corpus or tree moves, and are hard-coded into briefs. | EVIDENCE: [C, owner's private notes] A measured input went stale without any edit to the record. [M] A default PINS string is hard-coded in the trunk-merge script; HEAD shas are constants in every HDR. | IMPLICATION: Pin/Measurement nodes need the measuring command, measured_at commit and machine state, with a stale flag when the lane tip moves past measured_at. Briefs should read current pins from moirai at launch.
- [high] Knowledge links rot and orphan files accumulate without referential integrity. | EVIDENCE: [M] 32 topic files are neither indexed nor wiki-linked; 20 wiki-link occurrences (13 distinct targets, about 8 real) do not resolve; 7 files lack frontmatter; 1 file's prefix and type disagree. | IMPLICATION: Deletion leaves a tombstone (reason, replaced_by) that every referrer sees immediately, and links are edges validated on write, not filenames.
- [medium] Tool envelopes and output contracts of roles are not checked, so runs 'succeed' without the artifacts they promised. | EVIDENCE: [C, owner's private notes] The workflow reported 4 agents done with 0 errors, but no plan file existed. [M] 4 of 9 roles lack Write/Edit. | IMPLICATION: Role nodes should carry the tool envelope and can_write, and Run nodes should declare expected artifacts; a run closes only when those artifact nodes (path plus hash) exist.
- [high] Workflow scripts cannot touch the filesystem or network; only agents and the orchestrator can. | EVIDENCE: [D] workflow-authoring reference: no filesystem or Node API; args passed verbatim; workflow() nests one level. [M] 4 recent scripts are parameterized through args. | IMPLICATION: The realistic integration is orchestrator-side 'pack then launch' (moirai output passed as args) plus agents calling the MCP server or CLI, with hooks as fail-open side channels.
- [medium] Claim authority is not recorded, so research conclusions were cited as owner rulings. | EVIDENCE: [C, owner's private notes] A later correction found no owner mark beside the table row that had been cited as an owner ruling. | IMPLICATION: Every Rule, Decision and Ruling needs an authority field (owner / orchestrator / measured / research) and a source; 'owner' should require a verbatim quote.
- [medium] Volume is modest in node count but large in bodies. | EVIDENCE: [M] About 11 runs/day, p50 4 agents per run, 3,294 agents total; result bodies p50 8.8 KB, p90 38 KB, max 153 KB; run directories 2.2 GB; scratchpads in GBs. | IMPLICATION: Metadata stays around 0.3-0.5M nodes and 2M edges over three years, well within a small-RAM design. Whether bodies are stored decides the disk footprint by 2-3 orders of magnitude, so it must be an explicit owner decision.

## Open questions
- Should moirai replace Claude Code auto-memory (~/.claude/projects/<project>/memory/), or render MEMORY.md as a generated view, given the harness always loads MEMORY.md? What happens to the 254 existing topic files: migrate, freeze as a read-only archive, or leave as they are?
- Record granularity: every agent() call (~40k/year), or only workflow runs, reports and findings?
- Store report and design bodies inside moirai (content-addressed and versioned), or only path + sha256 pointers into scratchpads and the repo?
- Create a moirai branch automatically for every git worktree/branch (hook-driven; 44 worktrees exist today, several scratch or detached), or only when the orchestrator explicitly opens a lane?
- Are rules, hazards and owner rulings global (visible to every lane at once), or branched and merged like lane state?
- Which language for moirai records, given that memory is mostly Russian but repository artifacts must be English? Do moirai records count as repository artifacts?
- Should moirai store code references (file:line) at all, with content fingerprints and re-resolution by content, or leave code positions entirely to docs and the existing anchor gates?
- Who may write: every agent, or only the orchestrator and writer roles? May read-only roles (architect, critic, analyst, researcher) write findings through MCP, and does that change the separation of duties?
- Where should the DB live: inside the repository (versioned, pushable) or under ~/.claude (private, machine-local)? One DB per project or one global DB across projects (e.g. BoykoEngine and the owner's other projects)?
- Retention: keep tombstones and superseded nodes forever ('strike, never delete'), or prune by age?
- May a resident moirai MCP server (near 0% CPU, a few MB RSS) stay up during quiet measurement windows, or must it be stopped like every agent?
- Is the proposed authority vocabulary (owner ruling / orchestrator decision / measured fact / research claim) right, and must an 'owner' authority always carry a verbatim quote?


# landscape (docs/research/03-landscape-agent-memory-and-trackers.md)

## Summary
Landscape of agent-memory systems and agent-oriented task trackers, as of 2026-09-25. Nothing was installed or run.

Evidence tags:
- [M] measured here via the GitHub, PyPI, npm and crates.io APIs.
- [R] a third-party reported measurement, mostly from GitHub issues.
- [C] a vendor or author claim.
- [D] documented behaviour from docs or source.
- [S] a secondary source.

BEADS (deepest dive).
- Repository: now gastownhall/beads, 27.4k stars, v1.3.0 on 2026-09-15 [M].
- Data model worth copying [D]:
  - A wide issue record and 19 typed edge kinds.
  - Four of those edges affect readiness: blocks, parent-child (propagates blocked state down to children), conditional-blocks and waits-for.
  - The rest are informational: related, discovered-from, supersedes, duplicates, caused-by, validates, tracks, and others.
  - Other pieces: a `bd ready` queue with `--explain`, cycles rejected at write time, and an atomic `--claim`.
  - Since v1.3: leases with heartbeat, and compare-and-set guards.
- IDs: moved from sequential to hash-based in v0.20.1, because sequential IDs collided across agents and branches. Display length adapts via a birthday-bound calculation (4, 5, 6+ characters) [D].
- Storage history:
  - SQLite plus JSONL committed to git, plus a per-workspace daemon (Oct 2025 – Jan 2026).
  - Dolt-only from early Feb 2026. The daemon (~24k lines) and SQLite, the JSONL sync layer, the 3-way merge engine and tombstones (~70k+ lines in total) were deleted [S].
  - Embedded Dolt restored as the default (Apr 2026).
  - The `bd serve` HTTP server re-added in v1.3 to amortise the cost of starting a process and a DB connection on every call.
  - JSONL is now an export only. Sync rides the existing git remote under `refs/dolt/data`, with cell-level merges.
- Reported failures [R]:
  - 7 of 8 acknowledged `bd close` calls lost with 8 agents in nested worktrees (#4767).
  - Phantom empty databases in worktrees: `bd prime` returned 0 of 96 memories "for weeks" (#6551/#6552).
  - Orphaned dependency rows (#4673, #6487) and a stale denormalised `is_blocked` flag hiding ready work (#6608). Integrity is enforced in application code, and `bd doctor` repairs it.
  - A 120.6 GB spike from a recursive SQL cycle check on 2,682 issues (#4475).
  - Idle Dolt servers at ~2 GB RSS and 8–38% CPU each (#4282); 41 connections/s using 5 cores (#3760); 5–10 s per command in remote mode (#4102).
- Other measured facts:
  - The Windows release zip is 54 MB [M].
  - v1.2.1 was an untested release that auto-migrated schemas and stranded users [D].
  - The author claims the codebase is fully agent-written ("vibe coded"), 225k lines of Go [S].
  - Beads' own docs recommend CLI plus hooks over MCP: about 1–2k tokens against 10–50k [C].
- Derived and competing projects:
  - beads_rust `br`: Rust, FrankenSQLite plus JSONL, 1.1k stars.
  - mcp_agent_mail: agent messaging plus advisory file leases.
  - Small Rust trackers: bones (CRDT event log), braid (Automerge), grite (event log in git refs, sled), chainlink, PlanDB (containment graph plus cross-cutting dependencies), task-graph-mcp.

KNOWLEDGE MEMORY. None of these models tasks or readiness, and none enforces references at the engine level across versions.
- MCP memory server: rewrites the whole JSONL file on every mutation and locks only within one process. It does validate relation endpoints and cascades relation deletes [D].
- Basic Memory: Markdown plus a SQLite index. Allows forward references and keeps permalinks stable across renames [D].
- Letta: memory blocks plus archival memory. Since 2026-02-12, Letta Code's MemFS keeps memory in git: every edit is a commit, and memory subagents work in worktrees [D].
- mem0: moved to ADD-only extraction plus fused semantic, BM25 and entity retrieval. Claims 92.5 on LoCoMo and 94.4 on LongMemEval [C].
- Zep/Graphiti: bi-temporal validity; contradicted facts are invalidated rather than deleted. Needs Neo4j, FalkorDB or Neptune; Kuzu support was deprecated after Kuzu was archived [D].
- Cognee, LangMem (stalled since Oct 2025 [M]), A-MEM (research), MemOS (Neo4j + Qdrant, or SQLite FTS5 locally): see the report.
- Claude Code:
  - Auto memory preloads 200 lines or 25 KB of `MEMORY.md` and is shared across worktrees, but it is not loaded into subagents.
  - Shared task lists via `CLAUDE_CODE_TASK_LIST_ID`, and agent teams claim tasks with file locks.
  - The Task tools are omitted on newer models because their definitions cost context [D].

TRACKERS.
- Task Master: `tasks.json` with dotted sequential IDs. Its MCP tool tiers cost ~5k, 10k and 21k tokens [C]. Last release Mar 2026.
- Backlog.md: sequential IDs force scanning local and remote branches to avoid duplicates [D].
- Shrimp: stale.
- GitHub: issue dependencies went GA in Aug 2025.
- Linear: offers a remote MCP server.
- CCPM: one worktree per epic.
- Vibe Kanban: its company shut down 2026-04-10.
- Non-agent prior art: git-bug (operation log in git refs with Lamport clocks), Radicle COBs (Rust, a DAG of operations), TaskChampion.

BENCHMARKS.
- LoCoMo is saturated and its LLM judge is weak: it accepted 62.81% of deliberately wrong answers [S].
- A filesystem-plus-grep agent scored 74.0% on LoCoMo against mem0-graph's 68.5% [C].
- MemDelta: changing only the embedding model moves accuracy 6.2 points and flips conclusions [C].
- AMA-Bench: similarity retrieval is "lossy" on agent trajectories, and causal graph structure helps [C].
- MemPalace's 100% claims were retracted [S].
- No benchmark measures task-state correctness, precedence of superseded rules, or integrity under concurrency, so moirai needs its own evaluation.

GAP. No system combines all of these:
- A native embedded graph engine that updates reverse edges in the same atomic commit.
- Git-like commits, branches and merges at node and field level.
- One typed schema covering both tasks and knowledge.
- Readiness maintained as an incremental index.
- A small, fast, Windows-first binary with no required server.
- Correct behaviour with many processes across worktrees.
- A CLI, skill and hooks interface with context packs capped by a token budget.

## Implications
- [high] Sequential IDs collide across agents and branches; hash IDs fixed it | EVIDENCE: Beads switched to hash IDs in v0.20.1 because 'sequential IDs break when multiple agents create issues simultaneously' (docs/core-concepts/hash-ids.md, adaptive-ids.md). Task Master uses dotted sequential IDs. Backlog.md scans local and remote branches (checkActiveBranches, activeBranchDays) to avoid duplicate TASK-N IDs (ADVANCED-CONFIG.md). | IMPLICATION: Use random or hashed 128-bit internal node IDs, never reused. Show short adaptive-length aliases with prefix resolution. Treat hierarchical display IDs as views, not identity.
- [high] Keeping two sources of truth (DB and export) causes silent data loss | EVIDENCE: Beads classic, SQLite plus JSONL: #3931 'bd update rewrites issues.jsonl … silently dropping issues', Discussion #380 'Database out of sync with JSONL', #4135 six failure modes rooted in the 'Dolt/JSONL dual-source identity model'. | IMPLICATION: One canonical store. Every JSONL, Markdown or CLAUDE.md projection is a derived, one-way export and is never re-imported implicitly.
- [high] Referential integrity and derived state kept in application code drift | EVIDENCE: Beads #4673 (2,902 orphaned dependency rows), #6487 (804,727 orphaned wisp rows), #6608 (stale is_blocked hides ready work after raw-SQL fixes in bd doctor). bd delete rewrites text mentions to [deleted:ID] only in code. | IMPLICATION: Make reverse-edge indexes and readiness engine invariants, updated atomically on one mutation path. A delete writes a tombstone version that referrers resolve at read time. Repair tools only verify. Add property tests that the reverse index equals the inverse of the forward index.
- [high] Writing graph algorithms as recursive SQL is dangerous | EVIDENCE: Beads #4475: the bd doctor cycle check, a recursive CTE with string paths up to depth 100, drove Dolt to a 120.6 GB peak on 2,682 issues. The fix proposed is the Go DFS already used by bd dep cycles. | IMPLICATION: Implement traversal, cycle detection and critical path natively with bounded memory. Reject cycles on blocking edge kinds at write time. Every traversal takes an explicit node, edge or token budget.
- [high] Per-call process and connection startup dominates agent workloads | EVIDENCE: Beads #3760: 41 connections/s and 393–557% CPU with 4 idle agents; a cold call takes ~230 ms. #4102: bd ready 5.1 s and bd stats 10.3 s in remote mode. v1.3.0 added bd serve 'instead of a bd subprocess forked per call'. | IMPLICATION: Design the CLI for a low-millisecond cold start: a memory-mapped embedded store with no handshake. An optional long-lived process for MCP may speed things up but must never hold required state.
- [high] Server-backed storage costs RAM and CPU and breaks change detection | EVIDENCE: Beads #4282: orphaned dolt sql-server processes at ~2 GB RSS and 8–38% CPU each while idle. #2050: server mode lost file-watch change detection and caused port conflicts. The Windows release zip is 54 MB [M]. | IMPLICATION: Embed the store with no mandatory daemon. Any helper process exits when idle and has a RAM ceiling. Expose a cheap change notification (commit-sequence marker, file, or named pipe) so 'maximally synchronous' consumers do not need a server.
- [high] Multi-process and multi-worktree correctness is where agent trackers actually fail | EVIDENCE: Beads #4767: 7 of 8 acknowledged closes lost with a coordinator plus 8 agents in nested worktrees. #6551/#6552: a phantom empty embedded DB in a worktree returned 0 of 96 memories for weeks. #4135: split-brain after a branch switch. | IMPLICATION: Locate the store deterministically from the git common dir, so all worktrees share one store. Never auto-create a store on a miss inside a worktree. Confirm durability before acknowledging a write. Guarantee read-your-writes across processes. Make a Windows stress test (orchestrator plus N agent processes in N worktrees) a release gate.
- [medium] Git-native versioning of agent state is the trend, but no one pairs it with a typed graph and integrity | EVIDENCE: Beads on Dolt: cell-level merge, refs/dolt/data on the existing git remote. Letta Code MemFS (2026-02-12): every memory edit is a commit, subagents work in git worktrees and merge. grite, bones, git-bug and Radicle COBs use append-only operation logs with deterministic merge. | IMPLICATION: Build moirai's commit model as an append-only operation log with causal ordering, materialised as rebuildable indexes, with type-aware merge rules: a monotone status lattice, last-writer-wins for scalars, union for sets, and explicit rules for delete versus concurrent new reference. Reserve a refs/moirai/* path for sync over git.
- [high] Tool-schema token cost decides adoption | EVIDENCE: Beads docs: CLI plus hooks is ~1–2k tokens against 10–50k for MCP [C]. Task Master's 7, 15 and 36 tool tiers cost ~5k, 10k and 21k tokens [C]. Claude Code omits its Task tools on newer models because 'definitions and reminders take up context' [D]. | IMPLICATION: Make CLI plus skill plus hooks primary. Keep MCP thin (about 7 tools or fewer). Default to compact JSON output with a --brief style. Give context queries an explicit token budget.
- [medium] Simple, well-driven lexical and structural retrieval is competitive; vector-only is lossy for agents | EVIDENCE: Letta: a filesystem-plus-grep agent scored 74.0% on LoCoMo against mem0-graph's 68.5% [C]. MemDelta: changing only the embedding model moves accuracy 6.2 points and flips conclusions [C]. AMA-Bench: similarity retrieval is 'lossy' on agent trajectories [C]. mem0 (2026) fuses semantic, BM25 and entity signals [C]. | IMPLICATION: v1 retrieval: exact ID and typed filters, FTS/BM25, and graph-neighbourhood expansion within a budget. Embeddings come later as an optional plugin index with a RAM guard.
- [high] Knowledge must be append-only with supersession, not overwritten | EVIDENCE: mem0 moved to ADD-only extraction (2026). Graphiti invalidates contradicted facts using valid_at/invalid_at. The 01-boyko report records summaries outliving their retractions and union-merges resurrecting OPEN items. | IMPLICATION: Rules, decisions and findings get supersedes and refutes edges plus validity and invalidation metadata. Queries default to currently valid knowledge and can ask 'as of commit X'.
- [medium] Context injected at session start must be budgeted and tiered | EVIDENCE: Claude Code preloads only the first 200 lines or 25 KB of MEMORY.md and does not load main auto memory into subagents [D]. Letta MemFS always loads system/ plus the file tree [D]. OpenViking uses L0/L1/L2 tiers [D]. Mastra's append-only prefix keeps prompt-cache hit rates high [C]. | IMPLICATION: Give every node a one-line abstract. Provide `moirai prime --budget N` per role, deterministically ordered so prompts cache well, delivered via SessionStart and SubagentStart hooks.
- [high] Release and migration discipline is part of the product | EVIDENCE: Beads v1.2.1 was an untested release that auto-migrated schema v53 to v65 and stranded users. The v0.50 backend swap broke the ecosystem, e.g. vscode-beads. Beads pins Dolt v2.2.0 because of a regression. | IMPLICATION: Publish a versioned on-disk format spec. Migrations need explicit consent and readers stay backward-compatible. Add crash-recovery and fuzz tests. Keep the core small and owned rather than inheriting a large dependency.
- [high] Public memory benchmarks do not measure moirai's job and vendor scores are unreliable | EVIDENCE: LoCoMo's judge accepted 62.81% of deliberately wrong answers [S]. MemPalace's 100% claims were retracted [S]. Zep and mem0 dispute each other's LoCoMo numbers [C]. None tests task readiness, integrity or concurrency. | IMPLICATION: Build a moirai evaluation suite from the owner's workflow traces: state correctness under concurrent worktrees, precision@k of context packs for rules and decisions, tokens, and latency.

## Open questions
- Git coupling: should moirai data travel with the repo (a private ref namespace on the remote, state per git branch) or live beside it (one per-machine store shared by all worktrees)? Should moirai branches map 1:1 to git branches or worktrees, or stay independent as in Dolt?
- Is cross-machine sync (other PCs, cloud agents) required in v1, or is single-machine enough?
- What is the maximum number of concurrent writer processes (orchestrator plus subagents plus Workflow scripts across worktrees), and what per-call latency is acceptable?
- When a referenced node is deleted, should the default be refuse, cascade, or tombstone and mark referrers? Should mentions inside free text count as references?
- On a merge conflict (same field changed on both sides, or delete versus a new link), should moirai resolve automatically by type rules, or surface a conflict for the orchestrator or a human?
- Should rules, decisions and findings be plain notes, or typed facts with validity intervals and supersession ('what was the rule at commit X')?
- Is exact, full-text and graph search enough for v1, or are embeddings required? If required, is a local embedding model's RAM cost acceptable?
- Which interop matters: Beads JSONL import, GitHub Issues sync, Claude Code Tasks (~/.claude/tasks), CLAUDE.md or .claude/rules projections?
- Retention policy: keep full history forever, or allow agent-written compaction of old closed tasks (archive before discard)?
- What default token budget may a SessionStart or SubagentStart context pack consume?
- Should moirai replace Claude Code's native Tasks and auto memory in the owner's workflow, or complement them?
- Is a human UI (TUI or web viewer) needed, or are agents and the CLI enough?
- Will moirai be private or open source? This affects whether Beads-compatible import is worth doing.


# versioning (docs/research/04-versioned-storage-designs.md)

## Summary
Scope: how to version a graph database the way git versions code, for moirai (1e3–1e6 small-text nodes, many tiny agent commits, parallel agents on branches/worktrees, low RAM, Windows 11). Systems surveyed: git, Dolt/DoltLite (with Beads as a case study), TerminusDB, Datomic, XTDB v2, Irmin, Noms/prolly trees, Merkle Search Trees, HAMT, Jujutsu, Fossil, Pijul/Sanakirja, Automerge/Loro/Yjs, event sourcing and git-bug, the SQLite session extension, and LMDB/redb. For each: commits, branches, diff, merge, history, per-change storage, RAM, write amplification, GC, and how graph conflicts would surface. All 2025–2026 versions were checked on crates.io, release pages and changelogs. Labels used: MEASURED, CLAIMED, DOC, DERIVED, UNVERIFIED.

Core finding: there are two families, and their cost per tiny commit differs by one to two orders of magnitude.
- **State-snapshot Merkle stores** (git trees, Dolt prolly trees, Irmin, MST/HAMT) rewrite the root-to-leaf path on every commit.
  - Dolt documents at least "4 KB × tree depth" per mutation.
  - A DoltLite user measured one-INSERT-per-commit workloads: 1,063,396 commits used 438 GB before GC and 55 GB after (~52 KB retained per commit). 3.9M commits reached 341 GB and GC failed on 2 GiB internal caps.
  - My own estimate for moirai's schema (nodes, out-edges and in-edges trees at 1e6 nodes) is ~48 KB per commit.
  - DoltHub measured that scattered writes are the worst case: 26 MB/day of history versus 425 KB/day.
  - Dolt keeps its chunk index in RAM at about 1% of store size, so history growth also grows RAM.
- **Log/changeset stores** (Datomic datoms, event sourcing, git-bug ops, SQLite changesets, the jj op log) cost roughly the size of the change: about 0.3–0.6 KB per small commit, or about 0.3–0.6 GB per 1e6 commits (estimate).

Merge quality comes from typed intent plus validation, not from the storage structure.
- Dolt merges cell-wise 3-way and records conflicts and FK/unique violations in queryable system tables.
- TerminusDB builds a layer, validates it against the schema, and only then advances the branch label.
- jj stores conflicts as values inside commits, so merges never block, and resolution is a later commit. This suits autonomous agents.
- Kleppmann's move operation, used by Loro, handles tree cycles by skipping unsafe moves in timestamp order. No CRDT prevents cycles in a general DAG such as blockers, so explicit post-merge cycle detection is needed.
- Git merges dangling edges and cycles cleanly and silently.
- The SQLite session extension provides a ready-made conflict taxonomy that maps onto graph conflicts: DATA is a concurrent field edit, NOTFOUND is delete-vs-modify, CONFLICT is an id collision, CONSTRAINT is a schema violation, FOREIGN_KEY is a dangling edge. moirai needs one more class, CYCLE.

Git facts that matter:
- Worktrees share all refs under refs/ except refs/bisect, refs/worktree and refs/rewritten, so a store in the git common dir or under refs/moirai/* is visible to every agent's worktree instantly.
- Custom merge-driver commands must live in .git/config, which is not versioned.
- A 2017 libgit2 benchmark measured 3.2k commits/s without fsync and 40 commits/s with fsync, so group commit is mandatory.
- Git 3.0 will default new repos to SHA-256 and reftable, with Rust required. LWN puts it around April 2027.
- Dolt can store its data in a git remote under refs/dolt/data, which is a precedent for syncing database history through the project remote.

Beads (Yegge) is the closest existing product. It moved from SQLite plus JSONL-in-git to Dolt in February 2026 (DoltHub claims an order-of-magnitude scale gain), then restored an embedded mode in April 2026. It uses hash IDs specifically to avoid merge collisions. The community fork beads_rust keeps the classic design.

Recommended core, Architecture A: an op-log commit DAG with a materialized copy-on-write state and delta-overlay branches.
- Canonical data is append-only segment files of content-addressed commits whose payloads are changesets with before-images, SQLite-session style. That gives inversion and undo, as-of by reverse application, and blame.
- A jj-style op log records every mutation for undo and audit.
- The trunk's current state is materialized in a from-scratch copy-on-write B+tree (LMDB/redb family) holding nodes, out-edges, a VAET-style reverse index (so a delete instantly knows every node that referenced it), the ready-queue indexes, and per-node op chains for history.
- Agent branches are small overlays, as in TerminusDB, and large ones can be promoted via copy-on-write forks.
- Merges are field-level 3-way with typed rules: status lattice, add-wins sets, diff3 for text, Kleppmann moves for the hierarchy. Validators then check blocker cycles, dangling edges, forest shape and schema, and emit first-class conflict and violation records.
- Old states are served by sparse pinned checkpoints plus replay. MST checkpoint digests and a refs/moirai export can be added later for sync.
- Estimated trade-offs: best write performance, RAM and disk; the best graph-aware merge; medium-to-large build complexity, with B+tree crash-safety testing as the long pole.

Alternatives evaluated, with trade-off ratings:
- **B. Dolt-in-Rust prolly/MST store.** Any past version opens uniformly in O(depth), but ~40–55 GB per 1e6 tiny commits, GC scaling problems, and a large build (DoltLite needed ~18k lines of C and about 2,000 PRs).
- **C1. Git object DB.** Best git fit, but worst write performance and Windows risk from many loose files and GC.
- **C2. Working-tree JSONL.** Simplest to build, but worktrees diverge and cannot coordinate, and merges are line-based.
- **D. CRDT core.** Merges never fail, but losers are hidden by last-writer-wins, the whole document sits in RAM, and it is very large to build from scratch.

Proposed operating model:
- Coordination state (task status, claims, blockers) lives on a shared trunk in the git common dir, with linearizable writes, and is not branched.
- Branch-scoped knowledge lives on per-worktree moirai branches that merge when the git branch merges.
- Moirai commits record git HEAD SHA plus hash algorithm, git branch, worktree, agent and session ids. The SHA link is best-effort provenance only, because rebase and squash rewrite SHAs.
- Use 128-bit ids with short display prefixes. Sequential integers are safe only inside a single store with locked allocation.

Before committing to A, benchmark on the owner's Windows NVMe: fsync-per-commit versus group commit, copy-on-write versus WAL B+tree RSS and growth, merge-engine throughput on synthetic conflicting branches, old-version query cost against checkpoint spacing, and a DoltLite control run with moirai's schema.

Limitations:
- The 200-call web-search budget ran out partway through. Some items are marked unverified: Sanakirja benchmarks, Datomic per-datom sizes, Ink & Switch Patchwork, NTFS small-file and Defender costs.
- Many measured numbers are vendor-measured and were not reproduced.
- The only file created is the report. My temporary files were deleted.

## Implications
- [high] Path-copy Merkle state stores (prolly trees, git trees, MST/HAMT) cost several KB × tree depth per commit, which is catastrophic for many tiny agent commits. | EVIDENCE: Dolt docs: every edit costs at least 4 KB × tree depth (https://www.dolthub.com/docs/architecture/storage-engine/prolly-tree). DoltLite issue #2936 [MEASURED by a user, one INSERT per commit]: 1,063,396 commits used 438 GB before GC and 55 GB after (~52 KB/commit); 3.9M commits reached 341 GB and GC failed (https://github.com/dolthub/doltlite/issues/2936). DoltHub [MEASURED]: scattered index inserts produced 26 MB/day of history versus 425 KB/day (https://www.dolthub.com/blog/2024-04-12-study-in-structural-sharing/). | IMPLICATION: Don't use per-write Merkle roots as moirai's history mechanism. Use an append-only changeset/op log as canonical history (~0.3–0.6 KB per commit, estimated), and use Merkle structures only for commit ids and optional checkpoint digests.
- [medium] Dolt-style stores keep a chunk index in RAM proportional to total store size, so RAM grows with history. | EVIDENCE: DoltHub 2022 [CLAIMED/MEASURED]: table-file indexes are about 1% of database size, i.e. 10 GB of RAM for 1 TB (https://www.dolthub.com/blog/2022-02-28-dolt-storage-layer-memory-optimizations/). Dolt block store docs say the index is loaded into memory at startup. Dolt incremental GC post (v1.86.6, 2026-04-28) says GC memory scaled with transactions since the last GC. | IMPLICATION: To meet the minimal-RAM requirement, moirai's indexes should be mmap'd on-disk structures whose resident set tracks the hot working set, not total history. This favors a copy-on-write B+tree for current state plus a log for history.
- [high] Conflicts should be stored as data so merges never block agents, and graph invariants must be checked after merge because no storage model enforces them. | EVIDENCE: jj stores conflicted values in commits and simplifies them algebraically on rebase (https://docs.jj-vcs.dev/latest/technical/conflicts/). Dolt records conflicts and FK/unique violations in dolt_conflicts_* and dolt_constraint_violations_* tables (https://www.dolthub.com/docs/sql-reference/version-control/merges/). TerminusDB builds a layer, validates it, then advances the head (2020 whitepaper). Git merges dangling edges and cycles silently. | IMPLICATION: moirai merges should always produce a commit containing conflict values (base/ours/theirs) and violation records (DanglingEdge, Cycle, DeleteVsModify, IdCollision). Expose them via CLI/MCP queries, and exclude affected tasks from the ready queue until they are resolved.
- [high] Merging two acyclic blocker graphs can create cycles, and no CRDT or DB surveyed prevents DAG cycles. Tree cycles have a proven deterministic solution. | EVIDENCE: Kleppmann et al.'s move operation skips unsafe moves in timestamp order and is formally verified (https://martin.kleppmann.com/papers/move-op.pdf), and Loro implements it (https://loro.dev/blog/movable-tree). Weidner's survey lists cycle strategies such as a time-out zone, server rejection, topological skip, and hiding the newest edge (https://mattweidner.com/2023/09/26/crdt-survey-2.html). Dolt/SQL has no acyclicity constraint. | IMPLICATION: Use Kleppmann move semantics for the parent/child hierarchy. For blockers, run incremental cycle detection while replaying the incoming branch's edge additions in deterministic order, and emit Cycle violation records (optionally auto-dropping the newest edge, with a log entry).
- [high] A reverse index must be part of each version's state for 'maximally synchronous' reference consistency. | EVIDENCE: Datomic's VAET reverse index over reference attributes (https://docs.datomic.com/indexes/index-model.html). Dolt detects dangling references only as FK violations at merge time. SQLite session reports FOREIGN_KEY conflicts only when changesets are applied (https://www.sqlite.org/session/c_changeset_conflict.html). | IMPLICATION: The materialized state should maintain in-edge tables transactionally. A delete then tombstones the node and handles incident edges in the same commit (O(degree)), and merges find dangling edges cheaply.
- [high] Git worktrees share every ref under refs/ and the common dir, so a store placed there is instantly visible to all agents, while working-tree data diverges per worktree. | EVIDENCE: git-worktree docs: all refs starting with refs/ are shared except refs/bisect, refs/worktree and refs/rewritten; HEAD is per-worktree (https://git-scm.com/docs/git-worktree). Custom merge-driver commands must be in .git/config, not versioned (https://git-scm.com/docs/gitattributes). Nesbitt 2026: custom refs and notes are not fetched by default (https://nesbitt.io/2026/08/20/issues-in-the-repo.html). | IMPLICATION: Put moirai's primary store in the git common dir (.git/moirai/). Coordination state then lives on one shared trunk, and knowledge is branch-scoped on per-worktree branches. Add an optional refs/moirai/* export for push/pull (Dolt precedent: refs/dolt/data) and an optional non-canonical JSONL export for PR review.
- [high] Sequential integer IDs collide across branches and clones. The closest existing agent tracker switched to hash IDs for this reason. | EVIDENCE: Beads README: hash-based IDs such as bd-a1b2 prevent merge collisions in multi-agent/multi-branch workflows (https://github.com/gastownhall/beads). jj change ids are 16 random bytes (https://docs.jj-vcs.dev/latest/glossary/). git-bug derives entity ids from the hash of the first operation. | IMPLICATION: Use 128-bit ids (random, or UUIDv7 for insert locality) internally, with short unique-prefix display ids. Sequential numbers like 'node 40' are safe only if all branches live in one store and ids are allocated under its write lock. Cross-machine sync would break that.
- [medium] Per-commit fsync dominates tiny-commit throughput, and storage engines hit Windows-specific I/O issues. | EVIDENCE: libgit2 PR #4030 [MEASURED 2017, Linux SSD]: 3.2k commits/s without fsync, 40.8 with fsync (https://github.com/libgit2/libgit2/pull/4030). redb 4.2 fixed a Windows-only commit hang (https://github.com/cberner/redb/blob/master/CHANGELOG.md). LMDB pre-sizes its file to the full map size on Windows (https://github.com/Venemo/node-lmdb/issues/159). | IMPLICATION: Design for group commit with per-class durability levels. Prefer a few large append-only files over one file per object. Grow mmap'd files in chunks. Run Windows CI and fault-injection tests from day one. Benchmark fsync on the owner's NVMe first.
- [medium] A two-level history (a fine-grained op log plus a meaningful commit DAG) with lock-free concurrent operations fits multi-agent use. | EVIDENCE: jj records every command as a content-addressed operation and view. Concurrent op heads are merged 3-way on the next command, and undo/restore/--at-op work at the operation level (https://github.com/jj-vcs/jj/blob/main/docs/technical/concurrency.md, https://github.com/jj-vcs/jj/blob/main/docs/operation-log.md). | IMPLICATION: moirai should log every mutation as an operation (undo, audit, crash recovery) and let 'commits' be agent/session units. If there is no daemon, a jj-style op-heads protocol is an alternative to a single-writer lock.
- [high] Canonical immutable data plus rebuildable derived indexes makes schema evolution and recovery safe. | EVIDENCE: Fossil: derived tables 'contain no new information' and are rebuilt with fossil rebuild. Tickets are field-level change artifacts applied in timestamp order (last-writer-wins) (https://fossil-scm.org/home/doc/trunk/www/tech_overview.wiki, https://fossil-scm.org/home/doc/trunk/www/tickets.wiki). | IMPLICATION: Treat the op log as canonical and every index (materialized state, reverse edges, ready-queue counts) as derived and rebuildable. Avoid silent timestamp last-writer-wins for coordination fields.
- [medium] Building a prolly-tree VCS storage layer from scratch is a large effort even with heavy AI assistance, and the mature Rust building blocks are still young in 2026. | EVIDENCE: DoltLite: ~18k new lines of C, about 2,000 PRs and 12 format changes before beta v0.50.0 (Aug 2026) (https://www.dolthub.com/blog/2026-08-31-doltlite-beta/). redb multi-process access is experimental in 4.3.0 (2026-09-14). sanakirja 2.0 and pijul 1.0 are beta. The prollytree crate has ~3.7k downloads (crates.io, queried 2026-09-25). | IMPLICATION: Architecture A (log plus copy-on-write B+tree) is the smaller from-scratch build. Budget most effort for storage-engine crash testing. Architecture B is only worth it if uniform O(depth) access to any historical version is a hard requirement.
- [medium] CRDTs provide merges that never fail and version DAGs, but hide semantic conflicts and keep documents in RAM. | EVIDENCE: Automerge picks a deterministic winner and keeps losers via getConflicts (https://automerge.org/docs/reference/documents/conflicts/). Automerge 3.0 compressed in-memory history [MEASURED: Moby Dick 700 MB to 1.3 MB] (https://automerge.org/blog/automerge-3/). git-bug replays by Lamport clock and never surfaces conflicts (Nesbitt 2026). Loro offers checkout/fork/shallow snapshots. | IMPLICATION: Borrow CRDT techniques (hybrid logical clocks, add-wins sets, fractional ordering, Kleppmann moves) as merge rules inside moirai. Don't adopt a CRDT as the storage/merge core.

## Open questions
- Should task coordination state (status, claims, blockers, assignments) be branch-isolated at all, or live on a single shared trunk with only knowledge and notes scoped to branches (the proposed model)?
- Should a node deleted on one branch be visible as deleted to other branches before merge ('maximally synchronous'), even though that breaks branch isolation?
- Are human-friendly sequential integer ids ('node 40') required? If so, is one store per machine with lock-allocated ids acceptable, or are short hash ids (e.g. m-7f3a) fine?
- Commit granularity: should every agent mutation be a moirai commit (jj-style auto-snapshot), or only session/task boundaries with the op log keeping fine-grained undo?
- Default merge policy: should merges never block and just record conflicts and violations, or must some classes (blocker cycles, delete-vs-modify on tasks) make the merge fail until resolved?
- Is a status lattice for automatic merges acceptable (e.g. done beats in_progress beats open), or must every concurrent status change surface as a conflict?
- Where must data survive: is a store in the git common dir (lost if .git is deleted, not pushed) acceptable, or must moirai history be pushed to the git remote (refs/moirai) or exported to tracked files from day one?
- Will agents on other machines or in cloud sessions write to the same moirai? That makes sync and globally unique ids day-one requirements.
- Retention: keep all ops forever, or squash to checkpoints after N days/commits? Is semantic 'memory decay' (summarizing closed tasks) part of the storage layer or a separate layer?
- Process model: will a long-running daemon (the MCP server) own the database, or must every CLI invocation open the store directly with multi-process locking?
- How strict is 'from scratch': are small well-tested crates allowed (blake3, zstd, gix for the git link), or must the storage layer have zero storage-engine dependencies?
- Is uniform fast access to any arbitrary historical version a hard requirement? It favors the heavier Merkle-state Architecture B over the op-log Architecture A.


# storage-perf (docs/research/05-rust-storage-perf-ram.md)

## Summary
Lens: building a fast, low-RAM embedded storage engine in Rust for moirai, which must run on Windows 11. Every number in the report is labelled MEASURED-HERE (measured on the owner's machine during this research), MEASURED-EXT (published measurement), CLAIMED, or ESTIMATE.

**Measured on the owner's machine**
- Hardware: Ryzen 9 5900HS, 16 GB RAM, SK hynix consumer NVMe, NTFS. Defender real-time protection was on. CPU load from other agents was 30–100% during the tests.
- Process spawn is expensive:
  - A small Rust executable takes 20–38 ms at p50 from spawn to exit, and about 73 ms when the CPU is saturated.
  - The Git-Bash wrapper that the agent Bash tool uses adds about 109 ms. PowerShell costs about 600 ms.
- Durable writes cost about 2 ms each:
  - `FlushFileBuffers` after a 4 KiB write: p50 1.83–1.97 ms, p99 up to 5.7 ms.
  - `NtFlushBuffersFileEx` in data-sync-only mode: 1.73 ms.
  - 64 pages followed by one flush: 3.05 ms. Group commit is therefore a big lever.
  - `FILE_FLAG_WRITE_THROUGH` returned in 0.14 ms, which is likely not durable on consumer drives, so the report does not treat it as durable.
- File and memory-map access is cheap:
  - Opening a file and reading 4 KiB: 0.17 ms.
  - Opening and mapping a file read-only: 0.22 ms.
  - First touch of a mapped page that is already cached: about 1 µs. A managed-stream read of the same page: about 7 µs.
- RAM is tight: 1.8 GB of 15.8 GB was free, and 16 claude/node processes used 3.5 GB of private memory.
- SHA-256 runs at 1.94 GB/s on this CPU thanks to its SHA hardware instructions.
- Conclusion from these numbers: the engine is at most about 5 ms of a 115–190 ms agent CLI call. What matters is an O(1) open path and a persistent MCP server that avoids spawning a process per call, not micro-optimizing the engine.

**Reference engines (current state)**
- **redb 4.3.0** (2026-09-15):
  - Copy-on-write B+trees, one fsync per commit using checksummed commit slots.
  - Default per-process cache cap is 1 GiB.
  - Its default mode locks the whole file. Multi-process read-write is experimental as of 4.3.
  - It removed its mmap backend in 0.14 because its soundness could not be proven.
- **LMDB via heed 0.22.1**:
  - Fastest reads in the only apples-to-apples published benchmark: 0.64 µs per random read vs 1.14 µs for redb.
  - Mature multi-process support.
  - On Windows, the 0.9 release branch preallocates the file to its full map size. The master branch uses undocumented NTDLL section calls to grow the file instead.
- **fjall 3.1.10**: an LSM tree that runs in a single process only. Defaults are a 32 MiB cache, a 512 MiB journal, and up to 4 worker threads.
- **SQLite**: 2 MB page cache per connection by default; well proven.
- **Turso**: not yet at 1.0.
- **sled**: stale beta. **canopydb**: early stage. **Kùzu**: archived.
- **Prior art on concurrency.** beads (a git-backed agent issue tracker) went through several concurrency models:
  - SQLite with a daemon added for multi-agent corruption;
  - embedded Dolt with an exclusive lock, where a second opener gets an error;
  - server and proxied-server modes.
  - The lesson: design moirai's multi-process model from day 1.

**Windows rules the design must follow**
- Byte-range locks are mandatory, and Rust's `File::lock` locks the entire range, so the data file must never be locked. Use a separate LOCK file.
- A mapped file cannot be truncated or extended, and mapped views are not guaranteed to be coherent with ReadFile/WriteFile. So map only immutable, sealed segments, and handle the log tail with explicit I/O.
- In Rust std, `sync_data` equals `sync_all` (`FlushFileBuffers`). Use `NtFlushBuffersFileEx` data-only for commits.
- Defender charges per file opened, so keep fewer than about 10 files and never one file per node. Defender's asynchronous performance mode only applies to Dev Drive (ReFS).
- The CIDR 2022 mmap paper's failure modes are working sets larger than RAM, writes through the map, and very high I/O rates. None of them apply to moirai's read-mostly store of 10 MB–1.3 GB when the map is read-only.
- mmap also shares one copy of the pages across all agent processes, which saves RAM compared with per-process buffer pools.

**Formats**
- rkyv access costs about 1 ns unvalidated, but validated access (274 µs) costs about as much as deserializing (1.2 ms) on the benchmark's log dataset.
- For hot data, prefer `zerocopy` fixed-layout columns. They are safe with no validation pass.
- Store user-defined typed fields as tagged varints so the schema can evolve.
- Graph layout:
  - forward and reverse CSR adjacency (compressed sparse row arrays) at about 10 B per edge;
  - roaring bitmaps for status and tag sets, at most 128 KiB per set at 1e6 nodes;
  - about 80–90 B per node for topology plus metadata;
  - naive per-node Rust structs would cost about 350–450 B per node before any content.

**Hashing, compression, search**
- Use BLAKE3 for commit and object ids, xxh3 for checksums, and avoid SHA-1.
- Compress bodies with zstd using a per-store trained dictionary. The ratio gain for small records is claimed, not measured here; measure it on real notes.
- Full-text search by scale:
  - about 1e4 nodes: brute force;
  - 1e5–1e6: an FST term dictionary plus postings;
  - tantivy needs at least 15 MB per indexing thread and uses many files, so keep it optional.
- Vectors: int8 384-dimensional embeddings are 38 MB at 1e5 nodes. The embedding model, not the index, dominates RAM.

**Recommended design space**
- **(A) Recommended.** Append-only operation log as history (BLAKE3-hashed commit DAG).
  - Current state is materialized into immutable, memory-mapped, zero-copy columnar segments: node columns, forward/reverse CSR, bitmaps, FST.
  - A small overlay covers the log tail since the last checkpoint.
  - Checkpoints and rollups are incremental.
  - Readers never lock; writers hold a LOCK file and commit with one flush.
- **(B)** A copy-on-write B+tree in one file, written with explicit I/O and read through a read-only map (LMDB/redb style). Best for random updates at 1e6 nodes, but the hardest to build, and retained versions cost about 12–16 KiB per commit.
- **(C)** A RAM-resident daemon holding the whole graph. Fine as a prototype only: its idle RAM (100–300 MB at 1e5, over 1 GB at 1e6) breaks the RAM requirement.

**Estimated budgets for Option A** (warm cache, excluding spawn)
- Private memory per process: CLI about 2–6 MB; MCP server about 4–16 MB.
- Shared hot page cache: about 0.9 / 8.6 / 86 MB at 1e4 / 1e5 / 1e6 nodes.
- Latency: open 0.3–3 ms; get by id 1–5 µs; "ids of all blocking tasks" from 10 µs to 3 ms; durable commit about 2 ms.

**Before building from scratch**, benchmark redb 4.x, heed and SQLite on the moirai workload as oracles and baselines, with CI budget gates.

**Scratch files.** The measurement probe files (fsync probe .bin files and PDF text extractions) are not published. No project files other than the report were created.

## Implications
- [high] Process spawn dominates CLI latency on the owner's Windows machine; the storage engine is a small fraction of each agent call. | EVIDENCE: MEASURED-HERE: a small Rust exe takes 20–38 ms p50 spawn-to-exit at moderate load and ~73 ms under 100% CPU. The Git-Bash wrapper used by the agent Bash tool takes ~109 ms p50. File open+map takes 0.22 ms. Windows named-pipe round trip is ~11 µs (ipc-bench, 2026-09-04, MEASURED-EXT). | IMPLICATION: Make open O(1): no O(history) replay, no full-file validation, no index rebuild; keep the engine under 5 ms per command. Serve agents mainly through a long-lived MCP server so hot paths skip process spawns. Do not use an async runtime or threads in the CLI.
- [high] RAM is scarce during multi-agent sessions, and mainstream engines use large per-process caches. | EVIDENCE: MEASURED-HERE: 1.8 GB free of 15.8 GB; 16 claude/node processes held 3.5 GB private. Defaults: redb cache cap 1 GiB (src/db.rs); fjall 32 MiB cache, up to 4 worker threads and memtables; SQLite 2 MB per connection. | IMPLICATION: Target single-digit MB of private memory per CLI or MCP process. Keep data in the OS page cache through read-only maps, which all processes share, instead of per-process buffer pools. Measure private bytes and shared working set separately in CI.
- [high] A durable commit costs about 1.7–2 ms on this consumer NVMe; batching makes flushes much cheaper per write, and write-through is not trustworthy. | EVIDENCE: MEASURED-HERE: FlushFileBuffers p50 1.83–1.97 ms (p99 5.7 ms); NtFlushBuffersFileEx(DATA_SYNC_ONLY) 1.73 ms; 64 pages plus one flush 3.05 ms; FILE_FLAG_WRITE_THROUGH 0.14 ms. The PostgreSQL thread reports that Windows SATA drivers do not pass FUA. Rust std sync_data == sync_all == FlushFileBuffers. | IMPLICATION: Use one flush per commit with checksummed commit records (redb 1PC+C pattern), use the data-sync-only flush through windows-sys, preallocate log segments, and group-commit in the MCP server or daemon. Offer durability levels.
- [high] Windows byte-range locks are mandatory, and Rust's File::lock locks the entire file range. | EVIDENCE: LockFileEx docs: an exclusive lock denies other processes read and write access, but not access through mapped views; locks are released late after a crash. Rust std windows.rs calls LockFileEx(0, u32::MAX, u32::MAX). redb 5.0 moves to byte-range-only locks. | IMPLICATION: Never lock data files. Use a dedicated LOCK file for writers, make readers lock-free with double-buffered checksummed HEAD slots, and use timeouts or backoff for stale locks after a crash.
- [high] On Windows, mapped files cannot be resized, and mapped views are not guaranteed coherent with ReadFile/WriteFile. | EVIDENCE: SetEndOfFile docs require all views to be unmapped first. CreateFileMapping docs: mapped views and ReadFile/WriteFile are 'not necessarily coherent', and view access needs structured exception handling. LMDB 0.9 preallocates the file to mapsize; LMDB master uses undocumented NtCreateSection(SEC_RESERVE). | IMPLICATION: Map only immutable, sealed segment files (fixed size, never truncated). Write and read the mutable log tail with explicit I/O. Reclaim space by writing new files and swapping HEAD. Tolerate delete-pending errors during garbage collection.
- [high] Microsoft Defender's cost is per file opened, not per byte. | EVIDENCE: SQLite measured antivirus slowing direct-to-disk writes by about 10× while barely affecting a single database file. Defender performance mode works only on Dev Drive (ReFS). Defender scanning slowed cargo builds by 40–55% (cargo#5028). Real-time protection is on here. | IMPLICATION: Keep a small, stable set of files (fewer than about 10), with no file per node or commit and no per-command create/delete churn. Install the binary at a stable path. Benchmark with Defender on by default.
- [high] mmap is appropriate for moirai's read-mostly store if the map is read-only and the data is immutable. | EVIDENCE: CIDR 2022 (MEASURED-EXT): mmap is 2–20× worse than fio only once the working set exceeds RAM, and the authors say it may be fine if the data fits in memory and is read-only. Symas: LMDB uses a read-only map with write() for writes. redb dropped mmap over soundness when mapped memory can change. MEASURED-HERE: a soft fault costs about 1 µs per page, versus about 7 µs for a managed-stream read. | IMPLICATION: Read sealed segments through read-only maps with zerocopy views. Never mutate mapped regions. Guard against I/O errors with checksums at seal time. Rely on page sharing across the many agent processes.
- [medium] An append-only op log with checkpointed immutable columnar segments fits the requirements best (Option A). | EVIDENCE: Comparison in §7–8: an op log costs bytes per commit versus depth × 4 KiB for copy-on-write B-trees or Dolt prolly trees; history, branches and diffs come natively; readers can be lock-free; open is O(1) plus a bounded tail. TerminusDB layers and jj op logs are precedents. | IMPLICATION: Adopt Option A as the default. Keep Option B (copy-on-write B+tree with read-only map readers) as the fallback if random-update load at 1e6 makes overlays too heavy. Use Option C (RAM daemon) only as a prototype.
- [medium] Validated zero-copy access costs about as much as deserializing; fixed-layout zerocopy structs avoid that cost. | EVIDENCE: rust_serialization_benchmark 2026-09-10, log dataset: rkyv unvalidated access 1.09 ns, validated read 274 µs, deserialize 1.22 ms; flatbuffers validated read 42 µs. zerocopy FromBytes/KnownLayout gives safe views without a validation pass. rkyv has no schema evolution. | IMPLICATION: Use structure-of-arrays columns of little-endian zerocopy types with u32 row indexes. Store user-defined typed fields as a tagged-varint field block. Reserve rkyv for rare, complex, moirai-owned blobs.
- [medium] A compact graph layout keeps topology plus metadata at about 80–90 B per node; naive Rust structs cost several times more. | EVIDENCE: ESTIMATE: CSR forward+reverse at ~10 B per edge; roaring bitmaps at most 128 KiB per set at 1e6 nodes (RoaringFormatSpec: array 2 B per value, bitmap container 8 KiB); a naive struct with String, HashMap and Vec fields is ~350–450 B per node before content. References: Kùzu uses forward/backward CSR; Neo4j uses 15/34 B records. | IMPLICATION: Use immutable forward and reverse CSR in segments plus a small mutable overlay. Maintain a derived is_blocker bitmap so 'blocking task ids' is a bitmap query. Use stable external u64 ids with dense internal u32 row ids per snapshot so merges do not collide.
- [high] Hash choice is irrelevant for speed at moirai's record sizes; choose by role. | EVIDENCE: MEASURED-HERE: SHA-256 1.94 GB/s with SHA-NI; BLAKE2b 0.67 GB/s. xxHash README: XXH3 31–59 GB/s. BLAKE3 paper: 12× SHA-256 on AVX-512 hardware without SHA-NI, but for inputs ≤1 KiB it performs like BLAKE2s. Git 3.0 is moving to SHA-256. | IMPLICATION: Use BLAKE3-256 for commit and object ids (possibly truncated in indexes), xxh3-64/128 for page and record checksums, and SHA-256 only if git object interop is ever required. Do not use SHA-1.
- [medium] Full-text search and vector search can be tiered cheaply; tantivy and embedding models are the RAM-heavy parts. | EVIDENCE: tantivy: at least 15 MB per indexing thread, 12 MB baseline (index_writer.rs). FST sizes: 324 KB for 119k words; 157 MB for 15.7M titles. MemX: SQLite FTS5 search under 90 ms at 100k records. int8 384-d vectors = 38 MB at 1e5 nodes. model2vec 8M-parameter model is about 30 MB. | IMPLICATION: Use brute force at 1e4 nodes, a built-in FST plus postings at 1e5–1e6, and make tantivy and embedding generation optional features or separate processes. Use flat int8 vector search up to about 1e5 nodes.
- [high] Prior art shows that the multi-process concurrency model is the hardest part to retrofit. | EVIDENCE: beads CHANGELOG: 0.9.9 (2025-10-17) added a daemon to serialize SQLite writes for multiple agents; 1.0.0 (2026-04-02) used embedded Dolt with an exclusive lock, where a second opener gets an error; later versions added server and proxied-server modes; a startup audit removed per-invocation costs. | IMPLICATION: Build the file-level multi-writer and multi-reader protocol and a many-process contention test in the first milestone. Keep any daemon strictly optional. Track fixed per-invocation cost in CI.
- [high] No published benchmark exists for these engines on Windows/NTFS. | EVIDENCE: The redb README and canopydb benchmarks are Linux (Samsung 9100 PRO and i9-12900H); fjall's are ext4. None was found for Windows. | IMPLICATION: Before and during the from-scratch build, benchmark redb 4.x, heed and SQLite on the moirai workload on the owner's machine as correctness oracles and performance baselines, with CI gates such as engine time ≤5 ms per command and private memory ≤4 MB for the CLI and ≤10 MB for the MCP server at 1e5 nodes.

## Open questions
- Realistic scale: how many nodes per project and across all projects, and what are typical body sizes? Budgets differ a lot between 1e4 and 1e6 nodes.
- Where should the store live? Inside each repo (e.g. .moirai/, committed to git as binary segments or as a text op-log export) or in a user-level directory outside the repo?
- Should each git worktree or agent session automatically get its own moirai branch that merges when git branches merge, or should all agents share one live state concurrently?
- What is the maximum number of processes writing at the same moment, and may a writer block (for how long) or must it never wait?
- Durability: is losing the last ~100 ms of writes after a power cut acceptable in exchange for group commit, or must every CLI write be flushed (~2 ms each on this SSD)?
- Delete semantics: should deleting a node that is still referenced cascade, detach, or be refused? How should merges resolve 'deleted on one branch, linked on the other'?
- Is semantic (embedding) search required? If so, may moirai run a local embedding model, and within what RAM budget, or will vectors come from the agent or an external API?
- Is using a Dev Drive or adding Defender exclusions acceptable (admin rights), or must moirai be fast on default NTFS with Defender real-time protection on?
- Target platforms: Windows x64 only, or also Linux/macOS (CI, other machines) and Windows ARM64?
- Does 'written from scratch' allow small audited crates (zerocopy, blake3, xxhash, zstd, roaring/croaring, fst, bumpalo), or must codecs and formats be hand-written too?
- Is an optional resident background service acceptable at all, or must everything work with zero resident processes when no agent is running?


# data-model (docs/research/06-graph-data-model-integrity.md)

## Summary
Research-only report on moirai's graph data model, referential integrity, IDs and queries. No code was written. The only file created is the report. The web-search budget ran out near the end, so a few facts are marked unverified.

1) What existing systems do.
- Beads is the closest prior system: a Dolt-backed graph issue tracker for agents. It has ~20 dependency types, hash IDs that grow on collision, dotted child IDs, and hard delete that removes links in both directions and rewrites neighbours' text to [deleted:ID].
- Its 2026 issue tracker is a catalogue of measured integrity failures:
  - a parent/child plus blocks deadlock that its cycle detector cannot see (#6506, PR #5131);
  - dotted IDs that stop matching the hierarchy after a reparent;
  - write skew in the stored is_blocked flag under REPEATABLE READ (263/263 repro, 61-hour production gap);
  - a graph-wide recompute that makes bd close take 5–24 s on 4,483 issues;
  - a recursive CTE taking 7.46 s vs 0.19 s;
  - a traversal with no visited set that reached 17.4 GB RSS;
  - three commands that count ready/blocked differently.
- GitHub (sub-issues ≤100 per parent and ≤8 levels; blocked-by GA 2025-08-21, ≤50), Linear (UUID plus TEAM-123 alias; relation types; auto-close of parent/children; archive rules), Jira (typed links with inward/outward labels) and Claude Code Tasks (pending/in-progress/completed, blockedBy, claims under file locks) were also reviewed. Claude Code's Task tools are off by default on newer models to save context.
- Delete-policy vocabulary: Gel's per-link on-target-delete, Datomic's retractEntity (removes the entity and all references to it) plus its VAET reverse index, and TypeDB 3 (no dangling relations).
- Bitemporal memory: Graphiti invalidates contradicted facts instead of deleting them.
- Schema evolution: TerminusDB classifies changes as weakening or strengthening.

2) Recommended data model.
- A typed property graph with the schema stored as data, and a columnar node header (~48 B per node, estimated).
- Kind-specific typed fields. Binary typed edges keyed by (src, kind, dst), stored in both directions. Relationships with more than two participants become nodes.
- Datom-style records only for the change log.
- Status is an enum plus a resolution field. `blocked` is always derived, never stored.
- Node kinds: task (subtasks are tasks with a parent), rule, note, decision (ADR-like), finding, question, summary, area.
- Criticality is a separate field from priority.

3) Integrity.
- Each edge kind is either structural (must point to a live node; delete policy restrict, cascade or drop) or historical (may point to a deleted node, shown as a tombstone).
- Every reference is an edge, including #N mentions parsed from text. Deleting a node runs one transaction that walks its reverse list, applies the policies, marks dependents as suspect, and writes a tombstone to history.
- Soft delete is unnecessary because the DB is versioned.
- Merges must report referential violations and new cycles as conflicts, as Dolt does, instead of running cascades.

4) Acyclicity.
- Use Pearce–Kelly (in petgraph's Acyclic and the incremental-topo crate). A 2016 thesis measured it as the best all-round algorithm.
- Apply it to one combined precedence graph: blocks edges plus child→parent completion edges.
- A node may not block its own descendants. Blockers are inherited only from outside the ancestor's subtree.
- Merges run a full O(V+E) check.
- One writer at a time, readers on snapshots.

5) IDs.
- Sequential #N, never reused, from a store-global counter that is not versioned, so parallel branches cannot collide. Dolt's single-server global auto-increment and Postgres sequences work the same way.
- Short IDs cost few tokens: BAML reports ~24 tokens per UUID and 5–7 vs 29–68 errors with Claude Haiku. They also allow dense array and bitset indexing.
- A UUIDv7 uid plus an alias map is needed only if stores on different machines merge.
- No hierarchical IDs. No kind prefixes.

6) Derived state.
- Maintained eagerly and only for affected nodes, each predicate defined once: open_blockers counter, ready bitset, rollups, suspect/stale propagation, orphans.
- Critical path is computed on demand from the Pearce–Kelly topological order.
- Property tests check that incremental values equal a full recompute.

7) Memory semantics.
- Transaction time comes free from commits; valid time is optional.
- Code-anchored staleness: record observed_git_sha and applies_to paths, and flag knowledge whose files changed since.
- Supersede and retract are atomic edge-plus-status operations. Retraction marks transitive dependents (via reverse derived-from/cites edges) as suspect; MemTX (2026) proposes the same typed cascade.
- Citations can pin revisions (#40@r7), following PLANFENCE, where an executor that only checked freshness acted on a stale plan in 30/30 tasks.
- Provenance is recorded per transaction (Datomic reified transactions). Confidence is an enum. Knowledge is scoped through area nodes and path globs.
- STALE (2026) found LLMs detect stale memory at best 55.2%, so staleness should be computed deterministically.

8) Queries.
- Purpose-built commands (ready, blockers, show, tree, notes --path, changes --since, claim, stale, brief) plus a GitHub-style filter syntax.
- No Cypher: Text2Cypher execution accuracy was ~50% for GPT-4.
- Output is one line per node, with --ids, --fields, pagination and --json. The MCP tool count stays small.

The report also covers alternatives and trade-offs, 12 invariants, an anti-pattern checklist and a source list.

## Implications
- [high] Derived blocked/ready state maintained with SQL on top of a general-purpose versioned DB, under snapshot isolation, drifts and slows down at small scale | EVIDENCE: Beads #6716: write skew 263/263, 61 h invisible (bd 1.3.0-dev, 2026-09-24); #5939: close takes 5-24 s on 4,483 issues; #6128: 7.46 s vs 0.19 s recursive CTE on Dolt 2.2.4; #6105: three commands count differently | IMPLICATION: moirai should keep derived state in the engine, maintained eagerly and only for affected nodes, with every write serialized through one writer. Each predicate (ready/blocked) is defined once, and a doctor --verify command plus property tests check it against a full recompute.
- [high] Cycle detection that ignores some of the edge kinds used for readiness lets deadlocks through | EVIDENCE: Beads #6506 (2026-09-12) and PR #5131 (2026-09-18): parent-child inheritance plus blocks edges deadlock ready work while bd dep cycles reports nothing | IMPLICATION: Check acyclicity on one combined precedence graph (blocks plus child->parent completion edges) with Pearce-Kelly. Forbid a node from blocking its own descendants, inherit blockers only from outside the subtree, and never treat containers as ready to work.
- [high] Pearce-Kelly is the best practical incremental cycle detection algorithm for sparse graphs and already exists in Rust | EVIDENCE: Sigurdsson, Chalmers MSc 2016 (measured; PK best all-round, HKMST-Sparse best under 2% density); petgraph acyclic::Acyclic try_add_edge (docs.rs 0.8.3); incremental-topo crate | IMPLICATION: Use PK (write a custom version or borrow the design). Its topological order also serves topological listing and critical-path computation. Bulk imports run one final Kahn check.
- [high] Merging branches can produce referential violations and cycles that neither branch had | EVIDENCE: Dolt does not run cascades during merges and records violations in dolt_constraint_violations_*, refusing to commit until they are resolved (DoltHub blog 2021-07-20); Kleppmann et al. on concurrent tree moves creating cycles | IMPLICATION: Merges must re-validate every invariant with a full O(V+E) pass and emit explicit conflict records (delete/modify, dangling structural edge, cycle), with optional deterministic auto-resolution policies.
- [medium] Sequential IDs are the most token- and RAM-efficient and are merge-safe only when one allocator is shared by all branches | EVIDENCE: BAML: ~24 tokens per UUID; Claude Haiku made 5-7 errors with integer IDs vs 29-68 with UUIDs. Anthropic tool guide (2025-09-11) recommends meaningful or 0-indexed IDs. Dolt keeps a global auto_increment across branches in single-server mode; doltlite #2684 shows per-branch allocation colliding. Beads moved off sequential IDs because of branch collisions. | IMPLICATION: Allocate #N from a store-global, non-versioned, monotonic counter and never reuse numbers. That removes the need for generation tags and allows direct array and bitset indexing. Add a UUIDv7 uid and an alias map only if stores on different machines must merge.
- [high] Hierarchical dotted IDs mix identity with position and cause bugs | EVIDENCE: Beads PR #5131: after reparent 'the ID keeps its old dotted prefix while the edge moves'; Task Master #795 (2025-06-16): subtask 1.3 reported as 1.1.3, after which status updates fail | IMPLICATION: Keep IDs flat and kind-agnostic, and show the hierarchy only as a computed display path.
- [high] Per-edge-kind delete policies are the established way to keep references consistent automatically | EVIDENCE: Gel on target delete restrict/delete source/allow/deferred restrict; Datomic retractEntity removes references too and VAET provides reverse lookup; TypeDB 3 removes relations that have no role players; Beads bd delete removes links in both directions | IMPLICATION: Classify edge kinds as structural (restrict, cascade or drop) or historical (tombstone allowed). Store adjacency in both directions and update it in the same transaction, so deleting #40 updates every referrer atomically. Parse #N text mentions into edges instead of rewriting text.
- [medium] Versioning makes soft delete unnecessary and harmful | EVIDENCE: Brandur Leach, 'Soft deletion probably isn't worth it' (2022): it leaks into code and weakens foreign keys; Beads also dropped soft delete in v0.50+ | IMPLICATION: Use hard delete in the current state. Resolve tombstones from commit history (who deleted it, when, why) when a dead ID is rendered.
- [medium] LLMs are unreliable at detecting stale or retracted memory on their own | EVIDENCE: STALE (arXiv 2605.06527, May 2026): best model 55.2%. PLANFENCE (arXiv 2609.03340, Sept 2026): an executor that only checked freshness acted on the obsolete plan in 30/30 workflows, while pinned-citation validation had no invalid actions. MemTX (July 2026): typed cascading repair. | IMPLICATION: Compute staleness deterministically: propagate a suspect flag along reverse derived-from/cites edges, pin citation revisions (#40@r7), and flag code-anchored knowledge whose applies_to files changed since observed_git_sha.
- [medium] Every tool definition and every tool output costs agent context | EVIDENCE: Claude Code tools reference: Task tools are left out on newer models because definitions and reminders take up context. Anthropic guide: concise 72 vs detailed 206 tokens, 25k-token cap on tool responses. Text2Cypher: GPT-4 execution accuracy 49-50%. | IMPLICATION: Offer purpose-built commands plus a GitHub-style filter syntax, line-oriented output with --ids/--fields/pagination, and roughly six MCP tools. Deliver the CLI through a skill. Leave any Datalog/GQL layer for later, as an optional power tool.
- [medium] Provenance is cheapest when stored per transaction instead of per node | EVIDENCE: Datomic reified transactions: the datomic.tx tempid lets you annotate a transaction with provenance and the user who caused it | IMPLICATION: Store actor, session, git SHA and message on each moirai commit. Nodes keep only created_tx and updated_tx (u32).
- [medium] A combined task+knowledge schema needs to evolve safely | EVIDENCE: TerminusDB classifies schema changes as weakening (backwards compatible) or strengthening (needs migration) | IMPLICATION: Keep a built-in core schema plus project extensions stored as versioned data. Weakening changes apply and merge freely; strengthening changes need explicit migrations validated at merge. Never reuse an enum's integer code.

## Open questions
- Must moirai stores on different machines or clones ever merge? If yes, every node needs a global uid (UUIDv7) and an alias map. If no, a store-global sequential #N counter is enough.
- Where should the store live relative to git worktrees: one shared store (for example under the git common dir) with a moirai branch per worktree, or one store per worktree? This decides whether a single ID allocator and a single writer are possible.
- Must moirai branches mirror git branches automatically (created, merged and deleted with them), or are they independent and managed explicitly?
- When a blocker is deleted (not completed), should its dependents become unblocked (drop and notify), stay blocked until reviewed, or should the delete be refused?
- Should blockers on a parent or epic automatically block all its subtasks (inherited from outside the subtree only), or should only explicit edges block?
- Which knowledge kinds and lifecycles are wanted beyond task/rule/note/decision/finding/question/summary/area (for example experiment, gotcha, glossary)? Are ADR-style immutable decisions desired?
- Should agents, roles and sessions be first-class nodes (for assignee/author edges and per-agent queries) or plain fields?
- Is a depth cap on task decomposition wanted (GitHub uses 8 levels)? Should containers auto-close when all children are done?
- What token budget should the always-injected brief of critical rules and notes have, and should it be injected through a SessionStart hook?
- Does real-world valid time (valid_from/valid_to) matter, or is code-anchored staleness (observed_git_sha plus changed files) enough?
- Is a user-facing query language (Datalog or a GQL subset) wanted at all in v1, or only commands plus filters?
- What project sizes should moirai be designed for (nodes and edges per project), so the RAM and latency targets can be fixed and benchmarked?


# agent-integration (docs/research/07-agent-integration-cli-mcp-skills.md)

## Summary
Scope: how agents should talk to moirai, meaning the CLI, the MCP server, Claude Code skills and hooks, multi-agent concurrency, and how the owner's orchestrator, role-subagent, Workflow and worktree harness would use it. From BoykoEngine I read only the agent workflow configuration (agents, hooks, settings), as the owner instructed. Claims are labelled [M] measured on the owner's machine in this session, [D] documented in a primary source, [C] third-party claim, [I] my recommendation.

State of the art (September 2026):
- MCP 2026-07-28 made the protocol stateless. Sessions and the initialize handshake are gone; version and capabilities travel in per-request _meta, and there is a new server/discover call.
- subscriptions/listen replaces resources/subscribe for pushing change notifications.
- Server-initiated requests such as elicitation are replaced by a retry pattern (multi round-trip requests, MRTR).
- Tasks moved to an extension. List results are cacheable. Roots, Sampling and Logging are deprecated.
- The spec's "Stateful Tools" guidance is to use explicit handles with a stated lifetime, which is exactly how moirai leases should work.
- rmcp 3.4.1 (2026-09-23) is the official Rust SDK. It is Tier 1, dual-era, and implements subscriptions and MRTR.
- Claude Code 2.1.281 [M]:
  - Tool search is on by default, so only tool names and server instructions (up to 2,048 characters) load up front.
  - MCP output warns at 10k tokens and is capped at 25k tokens by default.
  - Stdio servers are still connected with the legacy handshake unless MCP_PROTOCOL_NEGOTIATION=auto.
  - list_changed is supported, but nothing an MCP server sends is pushed into the model's context.
  - Open issues report that when a tool returns both text and structuredContent, only structuredContent reaches the model. So model-facing tools should return compact text.

Claude Code extension points, verified from docs:
- SessionStart, UserPromptSubmit and SubagentStart can inject additionalContext. SubagentStart injects it into the subagent itself.
- SubagentStop provides last_assistant_message and agent_transcript_path, and can block once.
- PreToolUse can rewrite tool input (updatedInput). This allows deterministic stamping of agent_id, agent_type and cwd onto moirai MCP calls.
- Subagents run in the background by default, so PostToolUse on the Agent tool returns async_launched plus agentId and prompt, not the result.
- Hook strings are capped at 10,000 characters; the overflow goes to a file the model is not asked to read.
- mcp_tool hooks do not run at SessionStart launch, so SessionStart must be a command hook calling the CLI.
- SessionStart fires again with source=compact, so no PreCompact hook is needed.
- A WorktreeCreate hook replaces git worktree creation, so it must not be used for notification.
- Exec-form hooks (command plus args) avoid the shell and its quoting problems.
- Plugin monitors are session-long background commands whose output reaches Claude (interactive sessions only). Channels are a gated research preview and are incompatible with 2026-07-28.
- A resumed Workflow re-runs the failed agent and every agent started after it, even completed ones. Scripts have no shell or filesystem access. The default concurrency limit is 16 agents.
- Task tools are off by default on new models.
- A workflow subagent sees the parent's CLAUDE_CODE_SESSION_ID, and there is no per-agent ID variable [M].

CLI vs MCP. Benchmarks showing MCP costs 4–32x more (Scalekit) came from a 43-tool remote server without deferred loading. Zechner found cost parity and concluded "just make a good CLI". Beads recommends CLI plus hooks. On the owner's machine I measured:
- Rust CLI spawn: p50 about 15 ms.
- Through Git Bash: about 45–52 ms.
- Named-pipe round trip: p50 about 20 µs.
- A lean 11-tool moirai MCP surface: about 4.9k characters of schema (roughly 1.2–1.5k tokens, estimated), 216 characters of names up front.
- Windows PowerShell 5.1 strips embedded quotes and drops empty arguments when calling native programs.

Verdict: one core with two front ends.
- The CLI is required: for hooks, for bootstrapping, and for Bash-capable agents.
- The MCP server is required: 3 of the 9 roles (architect, architecture-critic, researcher) have no Bash tool; per-tool allowlists enable per-role write policy; JSON arguments avoid Windows quoting problems; and hooks can stamp agent identity deterministically.

Recommended architecture:
- One Rust binary with four modes: the CLI; `moirai daemon`, a single writer per repository with its store in the git common dir so all worktrees share it; `moirai mcp`, a thin stdio shim built on rmcp, dual-era; and `moirai hook <event>`.
- Clients talk to the daemon over a named pipe (Unix socket on Unix), using newline-delimited JSON-RPC, the framing the MCP spec recommends for custom transports.
- The daemon auto-starts via lock-file election, checks its version against the client, exits when idle, and a --no-daemon mode using LockFileEx is kept as a fallback.
- Per-process file locking is rejected. Windows locks are mandatory: an exclusive lock blocks other readers, and release after a crash is not immediate.
- A jj-style lock-free merge is rejected for coordination, because two agents could both claim the same task.

Claims:
- A lease carries a fencing token and a TTL.
- A dead Claude process (checked by PID) or SubagentStop releases the lease.
- An optional async heartbeat and a reclaim sweep cover the rest.
- Every mutation takes compare-and-set guards (--if-rev, --if-status), and a failed guard returns the current value.
- Every write takes an idempotency key, which Workflow resume makes mandatory.
- Beads 1.3.0 converged on the same design: leases, heartbeat, reclaim, guards, and exit code 13 for a failed guard.

"Maximally synchronous" works in four levels:
- L0: a delete tombstones the node and marks its references dangling in the same transaction.
- L1: other processes see it on their next read (daemon is the source of truth; every response carries rev).
- L2: an event bus pushes changes to `moirai watch` and MCP subscriptions.
- L3: the model learns at its next turn, through stale-reference markers in tool results, a UserPromptSubmit delta, or a plugin monitor.

Output contract:
- Compact text, one record per line, ids first, deterministic order, explicit truncation footers.
- --ids for pipes; --json/--jsonl with a versioned envelope.
- Errors carry a hint and the current value.
- Distinct exit codes, and empty results exit 0 (Claude Code treats exit 1 as failure except for grep and similar).
- No ANSI colour or prompts when not a TTY; text bodies read from stdin.

For the owner's harness, the default for Workflow runs is the orchestrator as dispatcher and single writer:
- The orchestrator claims tasks in bulk and passes the leases and a run id through args.
- Agents return schema output, as 199 of the owner's 347 scripts already do.
- The orchestrator persists everything with one idempotent `moirai apply`.
- `moirai pack --role --budget` replaces the hand-written HDR headers and .slice() truncation.
- Critic and reviewer verdicts become review nodes that block completion.
- Merge-time `moirai reconcile` promotes knowledge proposed on the branch and closes tasks whose evidence commits have reached main.

Hook set: SessionStart (a brief of about 8k characters or less), UserPromptSubmit (delta), SubagentStart (role pack), PostToolUse on Agent (maps agentId to task), SubagentStop (safety net), PreToolUse on mcp__moirai__.* (stamp).

Skills: a core `moirai` skill, `moirai-orchestrate`, and `moirai-report` preloaded into the implementing roles. Ship as a plugin, with the binary installed separately.

Phasing: P0 CLI plus SessionStart and the skill; P1 daemon, leases, apply and watch; P2 MCP, allowlists and stamping; P3 monitors, rules export and optional HTTP.

Unverified: whether SubagentStart/SubagentStop fire for Workflow agent() calls, and whether exec-form hooks resolve `moirai` to moirai.exe via PATH on Windows.

## Implications
- [high] Only command hooks run at SessionStart; mcp_tool hooks are skipped at launch because there is no MCP client context yet. | EVIDENCE: code.claude.com/docs/en/hooks, MCP tool hook fields: 'SessionStart fires before the servers are available ... Claude Code skips the event's mcp_tool hooks'. | IMPLICATION: moirai must ship a CLI (`moirai hook session-start`) even if the MCP server is the main agent interface. An MCP-only design cannot bootstrap session context.
- [high] Three of the owner's nine role agents (architect, architecture-critic, researcher) have no Bash tool. | EVIDENCE: [M] frontmatter `tools:` in the BoykoEngine repository's .claude/agents/*.md. Claude Code docs: `tools` accepts mcp__<server>__* and exact MCP tool names. | IMPLICATION: A CLI-only moirai cannot serve these roles. Provide MCP tools with per-role allowlists, or have the orchestrator ingest their schema output. Enforce the role write policy server-side using the hook-stamped agent_type.
- [high] Claude Code defers MCP tool schemas by default (tool search). Only names and server instructions (at most 2,048 characters) load up front. | EVIDENCE: [D] code.claude.com/docs/en/mcp, 'Scale with MCP tool search'. [M] The 11-tool moirai sketch is 4,924 characters of schema; the names-only listing is 216 characters. | IMPLICATION: The '10–50k tokens per request' objection to MCP does not apply to a lean moirai server. Mark 3–5 hot tools alwaysLoad and defer the rest. Put the when-to-use guidance at the start of `instructions`.
- [medium] When a tool returns both text content and structuredContent, Claude Code reportedly forwards only structuredContent to the model. | EVIDENCE: GitHub anthropics/claude-code #55677 (closed, not planned) and #79944 (open, 2026-07-21). | IMPLICATION: Model-facing moirai tools should return compact text without outputSchema/structuredContent, or make structuredContent itself compact. Offer JSON only on request.
- [high] Workflow resume re-runs the first failed agent and every agent started after it, even completed ones. Workflow scripts have no shell or filesystem access. | EVIDENCE: [D] code.claude.com/docs/en/workflows: 'Behavior and limits' and 'Resume after a pause'. | IMPLICATION: Every moirai write needs an idempotency key (e.g. run:<id>/agent:<label>). The default Workflow pattern should be: orchestrator claims tasks in bulk, passes leases through args, agents return schema output, and the orchestrator persists it with one idempotent `moirai apply` batch.
- [high] Hook-injected text is capped at 10,000 characters per string. The overflow goes to a file with a 2,000-character preview, and Claude is not asked to read the file. | EVIDENCE: [D] code.claude.com/docs/en/hooks, 'JSON output' and 'Add context for Claude'. | IMPLICATION: `moirai brief` and `moirai pack` must budget themselves (about 8k characters or less), put critical rules first, and end with an explicit 'dropped N items' footer.
- [medium] SubagentStart can inject additionalContext into the subagent, but it does not receive the prompt. SubagentStop provides last_assistant_message. PostToolUse on the Agent tool returns agentId plus the prompt, with status async_launched by default. | EVIDENCE: [D] code.claude.com/docs/en/hooks, SubagentStart/SubagentStop sections and the Agent tool_response table. | IMPLICATION: Map agentId to task from a prompt marker in PostToolUse(Agent), inject a role pack at SubagentStart, and release or flag leases at SubagentStop. This settles report 01's open verification items. Whether these hooks fire for Workflow agent() calls is still unverified.
- [high] PreToolUse hooks can replace tool input (updatedInput) and receive agent_id, agent_type and cwd. Subagent tool processes see only the parent's CLAUDE_CODE_SESSION_ID. | EVIDENCE: [D] hooks docs, 'PreToolUse decision control' and 'Common input fields'. [M] env inspection from a workflow subagent: no per-agent id variable exists. | IMPLICATION: Attribute writes deterministically by stamping ctx onto mcp__moirai__* calls through a PreToolUse hook. Do not trust model-supplied identity. Note that stamping implies an allow or ask permission decision.
- [high] MCP change notifications do not reach the model in Claude Code. Channels are a gated research preview and cannot be used by servers that negotiate 2026-07-28. Plugin monitors stream a background command's output to Claude for the whole session, in interactive sessions only. | EVIDENCE: [D] Claude Code docs: channels comparison table ('nothing is pushed to the session'), channels 'Research preview', plugins/components 'Monitors'. | IMPLICATION: 'Maximally synchronous' must be guaranteed at the database level (atomic tombstone plus reference update, rev on every response). Delivery to the model goes through tool results, UserPromptSubmit deltas and a `moirai watch` plugin monitor. Do not depend on channels or resource subscriptions.
- [high] Windows LockFileEx locks are mandatory: an exclusive lock denies other processes both read and write, and OS release after a crash is not immediate. A local named-pipe round trip costs about 20 µs versus about 15 ms for a process spawn. | EVIDENCE: [D] learn.microsoft.com LockFileEx remarks. [M] named-pipe echo p50 19.5 µs (n=5,000); Rust exe spawn p50 14.9 ms; Git Bash spawn p50 44–52 ms. | IMPLICATION: Use a single-writer daemon per repository (auto-started, lock-file election, idle exit, version handshake) with a --no-daemon LockFileEx fallback. Avoid per-process locking of the live store. Keep per-tool-call hooks narrow.
- [high] Beads 1.3.0 (2026-09-15) adopted leases (lease_expires_at, heartbeat, reclaim), compare-and-set guards (--if-assignee, --if-status, expected_version) with a dedicated exit code 13, an events journal, and a stable JSON envelope. It also removed its daemon in favour of embedded Dolt (single writer) plus an optional SQL server. | EVIDENCE: [D] github.com/gastownhall/beads releases v1.3.0; beads.gascity.com coordination, worktrees and json-schema docs; DoltHub blog 2026-04-02. | IMPLICATION: Validates moirai's claim design (lease, fencing token, guards, idempotency, reclaim). It also warns that daemons carry real operational cost that must be mitigated (skew checks, idle exit, doctor, fallback mode).
- [high] Windows PowerShell 5.1 strips embedded quotes and drops empty arguments when invoking native executables. | EVIDENCE: [M] printf.exe received 'Critical: never call unsafe code in hot loop' (quotes stripped) and lost an empty argument. [D] about_Parsing: 7.3 changed native argument passing, and 5.1 uses the Legacy behaviour. | IMPLICATION: The moirai CLI must take free-text bodies from stdin or @file rather than argv. MCP JSON arguments avoid the problem entirely, which is an argument for MCP on writes with long text.
- [medium] A stdio MCP server's cwd and CLAUDE_PROJECT_DIR belong to the session. Subagents with isolation: worktree run in their own worktree, branched from the default branch. | EVIDENCE: [D] Claude Code mcp docs (CLAUDE_PROJECT_DIR) and sub-agents docs (isolation: worktree). | IMPLICATION: Keep one store per repository, located via the git common dir. Record worktree, branch and HEAD provenance on every write (stamped for MCP, read from .git for the CLI). Keep coordination state global. Mark branch-local knowledge 'proposed' until merge-time reconciliation.
- [high] Claude Code's Bash tool treats exit code 1 as failure for every command except grep-like tools, and shows only about 10k characters of a failed result. | EVIDENCE: [D] code.claude.com/docs/en/tools-reference, 'Output limits'. | IMPLICATION: moirai must exit 0 on empty results. Reserve distinct non-zero codes for real conditions (not found, guard conflict, lease lost, blocked, store unavailable). Put an actionable hint and the current value in error output.
- [high] rmcp 3.4.1 is the official Tier-1 Rust SDK: dual-era, stateless for 2026-07-28, with a legacy session mode, macro-derived schemas and a tokio dependency. Claude Code still negotiates the legacy handshake with stdio servers by default. | EVIDENCE: [D] crates.io rmcp API; github.com/modelcontextprotocol/rust-sdk README; MCP issue #3179; Claude Code mcp docs 'MCP client runtimes'. | IMPLICATION: Use rmcp for `moirai mcp`, built dual-era. Keep the storage core free of tokio and MCP dependencies so the hot CLI path stays a small synchronous pipe client.

## Open questions
- Workflow dispatch default: should the orchestrator claim tasks in bulk and persist agents' schema output with one idempotent `moirai apply` (recommended), or should every agent write its own nodes directly?
- Permission posture: may moirai MCP calls be auto-approved by the identity-stamping PreToolUse hook (permissionDecision allow), or should writes prompt (ask)?
- Branch semantics: should knowledge created in a feature worktree stay 'proposed' until merge-time reconciliation (recommended), or should moirai branches mirror git branches one-to-one?
- Is a background per-repository `moirai daemon` process acceptable on your machine (per-user named pipe, exits when idle)? Or must the first phases stay daemonless, giving up push notifications and a hot in-memory index?
- Push into the orchestrator: are plugin monitors (interactive sessions only) acceptable? Do you ever run the orchestrator headless with -p, where monitors do not start?
- May the read-only roles (architect, architecture-critic, researcher) write their own node kinds (plans, findings, research) through MCP while keeping no file Write/Edit?
- Should critical path-scoped rules be exported into .claude/rules/moirai/ so Claude Code loads them natively when matching files are touched? If so, committed or gitignored?
- Lease defaults: is 15 minutes right for self-claimed tasks and 60 minutes for dispatcher claims across a Workflow run? Should a dead Claude Code process (PID check) release its leases immediately?
- Will non-Claude agents or tools (Codex, Cursor, CI bots) use moirai? That would raise the priority of an MCP HTTP mode and a frozen --json contract.
- Distribution: a plugin plus a separately installed binary (recommended), or a plugin that ships binaries in bin/, which blocks claude.ai/Cowork installs?


# concurrency-sync (docs/research/08-concurrency-sync-git-interop.md)

## Summary
Report 08 covers concurrency, live sync and git interop for moirai. Every fact in it is tagged: [M] measured on the owner's machine today, [S] checked in source or spec, [D] maintainer or vendor docs, [C] third-party claims, [I] my inference.

MEASURED BASELINE [M]
- Windows 11 26200, Ryzen 9 5900HS, 15.4 GB RAM. C: and D: are both NTFS, not Dev Drive. Defender real-time protection is on.
- Starting a tiny exe costs 34 ms; `git --version` costs 74 ms.
- A named-pipe round trip costs about 60 µs (this includes PowerShell overhead).
- A 4 KiB write plus FlushFileBuffers takes about 2 ms. With write-through and no flush it takes 0.2 ms, but whether that survives power loss is unverified.
- LockFileEx on a byte at offset 2^62 works on NTFS and takes 2–10 µs.
- Owner's workflow (BoykoEngine, metadata only):
  - 44 worktrees, some detached-HEAD, split between `.claude/worktrees` and `<lanes-dir>` lanes.
  - 107 local branches; 561 commits in the last 30 days.
  - 16 claude processes running right now.
- Workflow scripts run up to 16 agents at once. Subagents share their parent session's MCP connection [D].
- So throughput is not the constraint. The real costs are per-call process spawn, each process keeping its own cache, and agents acting on stale context.

TRANSACTION MODELS
- SQLite WAL is the reference model: one writer, snapshot readers, shared memory, a checkpoint that long readers can starve, and `data_version` for noticing other processes' commits.
- SQLite's WAL-reset corruption race existed from 3.7.0 to 3.51.2 and was fixed in 3.51.3 (2026-03-13). Antithesis says deterministic simulation reproduced it in about 15 minutes. Lesson: the cross-process coordination protocol is the hardest part to get right.
- LMDB's shared reader table needs stale-slot cleanup. On Windows, 0.9.36 grows the file to the full map size up front [S].
- fjall 3.x locks out other processes [D]. Turso does not support multi-process access [C]. Embedded Dolt takes an exclusive lock per operation [D].
- redb 4.3.0 (2026-09-14) added experimental multi-process read-write, with no shared memory:
  - lock bytes sit at offset 2^62;
  - each reader holds a shared lock on its own transaction-id byte;
  - two-phase commit is mandatory;
  - "The OS automatically releases file locks when a process crashes" [S].
  This is the best blueprint for moirai's file-level protocol.

WINDOWS HAZARDS
- LockFileEx locks are mandatory (they block ReadFile/WriteFile but not mapped views), and release after a crash can be delayed. Put lock bytes past EOF and retry with a bound.
- Defender, the Search indexer and OneDrive cause sharing violations on replace-by-rename. Commit in place; use Rust ≥1.85 POSIX-style rename and retry on errors 5 and 32 for exports.
- A memory-mapped file cannot be truncated (`ERROR_USER_MAPPED_FILE`). Use pread/pwrite (redb removed mmap in 0.14).
- The named-pipe default ACL gives read access to Everyone. Set an explicit ACL, `FILE_FLAG_FIRST_PIPE_INSTANCE`, and `PIPE_REJECT_REMOTE_CLIENTS`.
- A daemon spawned from an agent harness dies when the harness kills its job object (BrowserSkill #268).
- `ReadDirectoryChangesW` drops the whole buffer on overflow, so file watching can only be a "doorbell".
- Claude Code's own Windows daemon has an open named-pipe lifecycle bug (#66483).

DAEMONS
- Surveyed: watchman, git fsmonitor, Bazel (3 h idle timeout), sccache (600 s), rust-analyzer (one per editor, about 1 GB right now [M]) and jj (no daemon).
- Beads is the closest precedent:
  - Its daemon was removed around v0.50 (about 24k lines deleted).
  - Its JSONL sync, merge driver and tombstones were deleted (about 70k lines).
  - Dolt server mode was followed by embedded Dolt returning as default (v0.63 / v1.0).
  - May 2026 proposal #3760 wants an opt-in daemon again, because each `bd` call costs 150–230 ms and a Dolt server burned 400–550 % CPU with 4 idle agents [C].
- Key insight [I]: a Claude session's stdio MCP server already lives as long as the session and serves all its subagents. It can act as the leader without a separately managed daemon.

LIVE PROPAGATION
- Three layers, each with its own guarantee:
  - L1, the store: deletion and edge cleanup happen atomically, and a change-log record with a monotonic sequence number is written in the same transaction.
  - L2, process caches: monotonic reads, keyed by that sequence number.
  - L3, agent context: cannot be invalidated. Staleness is bounded to one tool batch, and every write carries preconditions (expected version or status).
- Beads 1.3.0 ended up with the same pieces: compare-and-set flags, leases with heartbeats, and a cursor-based events journal.
- Practical push today [D]: `mcp_tool` hooks on PostToolBatch, UserPromptSubmit or SubagentStart return `additionalContext` before the next model request, with no process spawn. They can pass `${cwd}` so moirai knows the worktree.
- MCP resource subscriptions exist in the 2026-07-28 spec, but the notification carries only a URI and Claude Code documents no way for it to reach the model.
- Channels are a research preview, need a `--dangerously-…` flag for custom servers, and a non-delivery bug was closed not planned. Don't rely on them.
- An `asyncRewake` long-poll hook might wake idle sessions. Untested.

GIT INTEROP
- Compared how others keep metadata next to git: Beads classic and current, Dolt git remotes (`refs/dolt/data`), git-bug (operation log with Lamport clocks), git-appraise (notes merged with `cat_sort_uniq`), Fossil, Radicle, jj, Taskmaster, and Claude Code tasks.
- Git facts that constrain any in-tree approach:
  - custom merge drivers are configured in `.git/config`, so they are not cloned;
  - GitHub's server-side merge ignores custom drivers;
  - the union driver can reorder lines;
  - custom refs are not fetched by a normal clone, so `git push --mirror` from such a clone deletes them (Beads #5266).
- Recommendation: moirai branches should not follow git branches automatically.
  - All worktrees share one live timeline for tasks, claims and blockers.
  - Every write records git provenance (worktree, branch or detached, HEAD, base commit, lane, agent).
  - Knowledge nodes can be scoped to a branch or commit. Whether they apply is computed from git ancestry, so merging the code branch makes them effective with no data movement.
  - Explicit moirai branches remain for what-if planning.
- Location: store in `<git-common-dir>/moirai/`, shared by all worktrees and outside every working tree.
- Optional: publish history as git objects under `refs/moirai/*` for backup and cross-machine sync (needs a fetch refspec and a doctor check).
- Optional: a derived text export for review, which is never the sync channel.

CRASH SAFETY
- One preallocated file, copy-on-write pages, two checksummed meta slots, two-phase commit.
- No shared-memory coordination; only byte-range locks, which the OS releases on crash.
- An fsync failure is fatal (per the ATC'20 fsync-failure paper).
- Different operations get different durability guarantees.
- Graph invariants (no dangling edges, acyclic blockers) are checked at commit and after merges, like Dolt's post-merge constraint violations.
- Backups are taken as snapshots at a transaction boundary.
- Refuse network and OneDrive locations.
- Test with deterministic simulation, fault injection and Windows kill-loops.

CANDIDATE ARCHITECTURES
- A: embedded multi-process, no daemon.
- B: single-owner daemon with thin clients.
- C (recommended): embedded-first with an opportunistic leader.
  - A's protocol is always in force, so correctness never depends on a live leader.
  - A long-lived MCP server takes a leader lock byte, serves a pipe, group-commits and broadcasts changes.
  - The CLI forwards to the leader if one is up, otherwise opens the file directly.
  - If the leader dies, another takes over.
  - A quiet mode pauses background work during the owner's benchmarks.
- The report has a trade-off table (latency, RAM, write cost, complexity, blast radius, portability) and a 17-scenario failure-mode table covering each architecture.

NOTES
- WebSearch hit the session's 200-search budget near the end. The last checks (LMDB preallocation, redb mmap removal) used WebFetch and raw source files instead.

## Implications
- [high] Process spawn on the owner's machine costs 34-74 ms, while a named-pipe round trip costs about 60 us. | EVIDENCE: [M] hostname.exe 34.0 ms avg, Process.Start 54.4 ms, git --version 74.2 ms (n=50); .NET named-pipe echo 32 B about 60 us avg (n=2000, includes PowerShell overhead). | IMPLICATION: Hot paths must not spawn processes: agent queries and per-turn change deltas should go through the long-lived MCP server and mcp_tool hooks. The CLI stays for humans, Bash-driven agents and scripts. Separately, the on-disk format must be 'open and query instantly' so the direct CLI path costs only the spawn.
- [high] A durable commit on NTFS/NVMe costs about 2 ms per flush, so a two-phase commit costs about 4 ms. The workload is bursty with about 16 concurrent agents but low in volume. | EVIDENCE: [M] 4 KiB write + FlushFileBuffers 1.9-2.35 ms; write-through 0.21 ms (durability unverified). [M] 561 commits/30 days in BoykoEngine. [D] Workflow scripts default to 16 concurrent agents. | IMPLICATION: One writer at a time is enough. Add group commit, done by a leader process, plus durability classes: claims and deletes durable, heartbeats and cursors lazy. Multiple concurrent writers are not needed.
- [high] redb 4.3.0 (2026-09-14) shipped an experimental multi-process read-write protocol that uses only byte-range locks at offset 2^62, with no shared memory. The OS releases a crashed process's locks. | EVIDENCE: [S] redb docs/design.md 'Multi-process concurrency' and CHANGELOG 4.3.0. [M] 1-byte LockFileEx at 2^62 works on NTFS in 2-10 us. | IMPLICATION: Use this pattern for moirai's file-level protocol: writer byte, per-transaction reader bytes, header lock, mandatory two-phase commit. Avoid LMDB-style shared-memory reader tables, which need stale-slot sweeping.
- [high] Mainstream Rust embedded stores are single-process or use a coarse exclusive lock per operation. | EVIDENCE: [D] fjall 3.x exclusive lock ('multi-process access (which is not supported)'); [C] Turso: 'Multi-process access is not supported'; [D] embedded Dolt in Beads opens and closes the engine under an exclusive lock per operation; redb multi-process arrived only in 4.3, as experimental. | IMPLICATION: Multi-process safety must be designed into moirai's file format and lock protocol from day one. It cannot be bolted onto a single-process engine later.
- [high] Windows file semantics are hostile to naive designs: byte-range locks are mandatory, lock release after a crash can be delayed, AV and indexers cause sharing violations on rename, mapped files cannot be truncated, and LMDB 0.9 preallocates the whole map size. | EVIDENCE: [S] LockFileEx remarks; [S] LMDB mdb.c 0.9.36 mdb_env_map comment; [C] ERROR_USER_MAPPED_FILE reports; [C] ERROR_ACCESS_DENIED/SHARING_VIOLATION reports; [M] Defender real-time protection is on. | IMPLICATION: Put lock bytes past EOF. Commit in place in one preallocated file with no rename-over. Use pread/pwrite with a user-space cache instead of mmap writes. Retry errors 5/32 with a bound. Use few, stable files. Sign release binaries.
- [medium] Separately managed daemons are a recurring reliability problem, yet per-invocation setup cost pushes projects back toward long-lived processes. | EVIDENCE: [C] Beads removed its daemon/RPC (~24k LOC, v0.50) after Windows daemon failures (#1379), then #3760 (2026-05) proposed an opt-in 'bd serve' because of 150-230 ms per call and 400-550% Dolt CPU. [C] Claude Code's Windows daemon pipe bug (#66483). [C] job-object kill of spawned daemons (BrowserSkill #268). | IMPLICATION: Do not build a separately auto-started daemon. Let a long-lived moirai MCP server become an opportunistic leader (lock byte plus named pipe), and let the CLI fall back to direct access when no leader exists (architecture C).
- [high] Subagents that reference an MCP server by name share the parent session's connection, so one MCP process serves subagents running in other worktrees. | EVIDENCE: [D] Claude Code sub-agents doc: 'String references share the parent session's connection.' [D] CLAUDE_PROJECT_DIR is stable and roots/list returns the launch directory. | IMPLICATION: The MCP server cannot infer the worktree or lane from its own cwd. Tools must take an explicit lane/cwd argument, and hooks must pass ${cwd}/${session_id} into mcp_tool calls.
- [medium] Claude Code can inject context before every model request without spawning a process, via mcp_tool hooks on PostToolBatch, UserPromptSubmit or SubagentStart. MCP resource-update notifications and channels are not dependable ways to reach the model. | EVIDENCE: [D] hooks docs (mcp_tool type, additionalContext, PostToolBatch 'before Claude Code sends the next request'); [D] MCP 2026-07-28 notifications/resources/updated carries only a URI and keeps no subscription state across stdio reconnects; [D] channels are a research preview needing a dev flag; [C] #45563 non-delivery closed not planned. | IMPLICATION: Implement push to agents as relevance-filtered deltas pulled by hooks, using a per-session cursor over the commit sequence. MCP subscriptions and channels can be optional extras at most. Behaviour of asyncRewake long-polling for idle sessions needs a prototype.
- [high] An LLM's context cannot be invalidated, so 'every node immediately knows node 40 is gone' is only achievable inside the store and caches. Beads converged on compare-and-set guards and leases. | EVIDENCE: [D] Beads v1.3.0 (2026-09-15): compare-and-set updates via --if-assignee/--if-status (exit code 13) and expiring claim leases with heartbeat; events journal written in the same transaction with a cursor. | IMPLICATION: Deleting a node removes or tombstones its incident edges atomically, and a change-log row is written in the same transaction. Every mutation carries preconditions and fails with an explanation of what changed and who changed it. Claims are leases renewed by hooks.
- [high] File-system watching is lossy on Windows. | EVIDENCE: [D] ReadDirectoryChangesW discards the entire buffer on overflow (lpBytesReturned=0). | IMPLICATION: A watcher, named event or pipe message can only mean 'go re-read the sequence'. The lossless change feed must be a monotonic commit sequence plus a change log written in the same transaction.
- [medium] Branch identity in the owner's workflow is unstable and coordination spans branches. | EVIDENCE: [M] 44 worktrees, 107 local branches, several detached-HEAD worktrees, short-lived wf_* worktrees; [D] Claude Code auto-creates and deletes worktree-<name> branches; [C] report 01: 'no shared, live view of who owns what' across lanes; [D] Beads data is 'not committed to the current Git branch'; [D] Taskmaster has 'no automatic tag switching'. | IMPLICATION: Do not auto-follow git branches. Keep one live timeline shared by all worktrees for tasks, claims and blockers. Record git provenance on every write. Scope knowledge to a branch or commit, with visibility computed from git ancestry. Keep explicit moirai branches for planning only.
- [high] In-tree text as the source of truth with merge drivers is fragile and was abandoned by the closest precedent. | EVIDENCE: [D] gitattributes: drivers are defined in .git/config (not cloned) and the union driver scrambles line order; [C] GitHub server-side merge ignores custom drivers; [C] Beads deleted its JSONL sync, merge engine and tombstones (~70k LOC); [D] Beads: JSONL import 'cannot infer that records absent from an export were deleted'. | IMPLICATION: Any in-repo text must be a deterministic derived export that is regenerated, never merged. Tombstones are required so deletions survive merges and sync.
- [medium] $GIT_COMMON_DIR is shared by all linked worktrees, and sandboxed agents may write there. Custom refs are not fetched by a normal clone and can be deleted by a mirror push. | EVIDENCE: [D] git-worktree docs on GIT_COMMON_DIR and shared refs; [D] Claude Code sandboxing allows writes to the shared .git except hooks/ and config; [D] Dolt: the ref 'will not even be cloned, fetched, or pulled by Git'; [C] Beads #5266: push --mirror deletes refs/dolt/data. | IMPLICATION: Store live data in <git-common-dir>/moirai/. Publish history for backup and cross-machine sync as git objects under refs/moirai/* with a documented fetch refspec and a doctor check that detects missing refs.
- [high] Multi-process storage bugs hide for years, and fsync error handling is broadly broken. | EVIDENCE: [D][C] SQLite WAL-reset race present 3.7.0-3.51.2, found by Antithesis DST in about 15 minutes; [D] ALICE (OSDI'14) found 60 crash-consistency vulnerabilities in 11 systems; [D] ATC'20: no studied application handled fsync failures adequately. | IMPLICATION: Budget for deterministic multi-process simulation with crash and fsync-error injection, plus Windows TerminateProcess kill-loops with 16 writers, before trusting the protocol. Treat fsync failure as fatal (crash, then recover).
- [medium] The owner kills processes wholesale and runs timing measurements that require no concurrent activity. | EVIDENCE: [C] report 01: 'taskkill /IM cargo.exe would have killed three lanes' builds'; 'timed measurements with no concurrent agent activity'; [D] Claude Code re-signals agent processes when a run is stopped. | IMPLICATION: Any moirai process can die at any instant, so the design must degrade gracefully and never depend on one process. moirai needs a quiet mode with no background compaction or checkpointing during benchmarks.
- [medium] Per-process caches multiply RAM and are defeated by cross-process writes under a redb-style protocol. | EVIDENCE: [M] 16 claude processes running (1.86 GB total); [S] redb invalidates all cached pages whenever it observes a new foreign transaction id; [M] rust-analyzer about 1 GB WS as an example of per-client in-memory state. | IMPLICATION: Concentrate caching and secondary indexes in the leader. Followers keep tiny caches. Idle RAM of the MCP server and leader must be measured before committing to cache sizes (current figures are estimates).
- [medium] The owner's volumes are plain NTFS with Defender real-time scanning. Dev Drive offers asynchronous scanning on ReFS. | EVIDENCE: [M] Get-Volume shows NTFS on C: and D:; Get-MpComputerStatus RealTimeProtectionEnabled=True; [D] Dev Drive docs (ReFS, performance mode, min 50 GB); [C] 'up to 30%' build gains. | IMPLICATION: moirai must perform acceptably on NTFS with real-time AV, which means few files and in-place writes. Dev Drive can be documented as an optional speed-up, not assumed.

## Open questions
- Is project knowledge (rules, decisions, findings) ever branch-specific enough that you want moirai branches bound to git branches? Or is 'provenance + ancestry-based scoping' enough: notes tagged with branch and commit, becoming effective when the code merges?
- Should moirai data ever leave this machine, for example published as git objects under refs/moirai/* on the GitHub remote for backup and cross-machine sync? The BoykoEngine docs deploy to public GitHub Pages; is the repo public, and may agent notes be public?
- One moirai store per repository (inside .git, shared by all its worktrees including `<lanes-dir>` lanes), or one store per user under %LOCALAPPDATA% with cross-repository views?
- How much can be lost on power loss for low-value writes (heartbeats, read cursors, status pings)? For example 'the last ~100 ms'. Claims, deletions and decisions would always be durable.
- Is it acceptable for the moirai MCP server to act as an opportunistic 'leader' process while a Claude session is open? Alternatively: zero long-lived moirai processes (simpler, slower, no push), or a real background daemon (faster, but more lifecycle risk on Windows)?
- Default deletion policy: tombstone plus automatic edge cleanup, or refuse deletion while referenced (restrict) unless --cascade? Should it differ per edge type (blocks vs part_of vs mentions)?
- How much unsolicited context may moirai inject into agents per tool batch? For example only changes touching nodes the agent claimed, owns or is blocked on, capped at N tokens. Or should agents only pull on demand?
- Should moirai mirror or replace Claude Code's native Task tools (TaskCreate/TaskUpdate, agent-team task lists) via the TaskCreated/TaskCompleted hooks, or ignore them?
- Are Linux/macOS (where Claude Code's Bash sandbox restricts Unix sockets) first-class targets, or is 'portable' limited to 'builds and works on Windows first'?
- Would you format a Dev Drive (ReFS, Defender performance mode) for repositories and the moirai store, or must moirai be tuned only for plain NTFS with real-time scanning?
- What do the `<lanes-dir>/mq-*` worktrees represent (a merge queue)? Should moirai record 'branch merged' events explicitly from that process, rather than inferring them lazily from git ancestry?
- Must moirai stay completely silent (no background compaction, checkpointing or fsync bursts) whenever a benchmark lane is running, and how should it learn that (explicit 'moirai quiet on' vs a lane status flag)?
