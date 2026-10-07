# The agent workflow: units, the orchestrator and the gate kit

- **Status:** set up on 2026-10-07 from the owner's brief and the plan the owner approved that day; R-HARN
  (`docs/m0/**`, [authors.md](authors.md) §3), reviewed like any other `docs/m0/` file. The orchestrator reads this
  file. Every other agent reads only AGENTS.md "Agent work", its role file in `.claude/agents/` and its brief.
- **Sources:** AGENTS.md ("Git", "Data", "Agent work"); [PLAN.md](PLAN.md) §2.1 (target directories, test tiers),
  §2.5, §3.1 (author roles, S1–S6, "Mechanics"), §5 (commits and pushes); [authors.md](authors.md) §1–§3, §5;
  `xtask/roles.toml`; the module docs of `xtask/src/worktree.rs`, `gate.rs` and `authors.rs`;
  [nightly.md](nightly.md); [measurement-protocol.md](../spec/measurement-protocol.md) (`[MP]`) §2–§4, §7, §8;
  [60 §3.15] (profile L); [tools.md](tools.md) §9, §12, §13. The Claude Code facts are from its documentation as read
  on 2026-10-07 (code.claude.com/docs/en/: `settings`, `settings-reference`, `permissions`, `sub-agents`,
  `workflows`, `prompt-caching`, `agent-view`); a fact that moves with a release is checked again in §15.
- **Rulings:** R-1 to R-4 (§2) are the owner's answers of 2026-10-07. Later rulings are numbered on from R-5 in
  `<ORCH>/RULINGS.md`, and units cite them by number.

## 1. Terms and places

- **Unit**: one reviewable portion of one WP by one author role: at most about 10 commits and 15 files, in one
  subsystem. Its branch is the role's `m0/<role>` and its worktree `<WT_ROOT>/<role>`, both made by
  `cargo xtask worktree <role> --base master` (PLAN §3.1 "Mechanics"). One open unit per role; units run
  concurrently only for different roles. The word "lane" keeps its PLAN §2.1 meaning (the two shared target
  directories, A and B) and its product meaning (`lane/<name>` branches); the brief's "lane" is a unit here.
- **Trunk**: local `master`. Only the orchestrator merges into it, and it pushes it after the merge (R-2).
- **Author role** (PLAN §3.1: R-HARN-I, R-MODEL, …) and **stage role** (`.claude/agents/`: architect,
  architecture-critic, developer, tester, code-reviewer, researcher, project-analyst, results-analyst, doc-writer,
  mechanic). Every agent of a unit is a stage role acting for the unit's author role, bound by that role's S1–S6,
  `deny_read` and write map (AGENTS.md "Agent work").
- **Gate worktree**: a neutral, non-authoring worktree, detached, where only the full gate runs
  (`cargo xtask gate --branch m0/<role>`, PLAN §3.1 "The gate worktree"). There is one per PLAN lane, `gate-a` for
  the lane-A roles and `gate-b` for the lane-B roles of `xtask/roles.toml`, because the gate runs with the target
  directory of the branch's lane: units of different lanes then gate at once, and units of one lane take turns on the
  kit's lock of that worktree (they would wait on cargo's build lock of the shared target directory anyway).
- **Lock set**: the paths a unit may write. **Red-first**, **pin**: AGENTS.md "Agent work". **Mutation**: a deliberate
  bug that a named gate must catch. **Delta-scoped**: only what the last fix or sync changed.
- **Kit**: the PowerShell scripts in `<ORCH>/kit/` that run long gates detached under a budget (§7, Appendix A).
- **Orchestrator session**: an interactive Claude Code session in the main checkout, on `master`. It plans, launches,
  reads verdicts, rules, merges and pushes; it never runs a unit's agents itself.
- **Unit session**: a Claude Code session started in the role worktree, which launches the unit workflow and nothing
  else. A subagent starts in the launching session's working directory and inherits that session's permission rules
  and environment (Claude Code sub-agents documentation). The role's `deny_read` rules and its lane's
  `CARGO_TARGET_DIR` and `CARGO_BUILD_JOBS` live in the role worktree's `.claude/settings.local.json`
  (`xtask/src/worktree.rs`), so only a session started there applies them to every agent of the unit. On Windows
  Claude Code reads that file from the session's starting directory; elsewhere a worktree session reads the main
  checkout's (settings documentation), one more reason units run only on the Windows PC (R-1).

| Place | Path | Made by |
|---|---|---|
| Main checkout (the orchestrator session) | `D:/claude/moirai` | the owner's clone (R-1) |
| `<WT_ROOT>`, the worktree root | `D:/moirai-wt` | `git config moirai.worktree-root` (R-1) |
| Role worktree, one per open unit | `<WT_ROOT>/<role>`, branch `m0/<role>` | `cargo xtask worktree <role> --base master` |
| Gate worktrees | `<WT_ROOT>/gate-a`, `<WT_ROOT>/gate-b` | `git worktree add --detach`, once (§15 step 6) |
| Lane target directories | `D:/moirai-target/laneA`, `D:/moirai-target/laneB` | `git config moirai.target-root`; `xtask worktree` sets them (PLAN §2.1) |
| `<ORCH>`, the orchestration state | `D:/orch/moirai` | §15 step 3 (R-1) |

## 2. The owner's rulings

The owner's answers of 2026-10-07, copied into `<ORCH>/RULINGS.md` word for word in §15 step 7, with the author
identity written out there:

- **R-1 Where units run.** The owner's Windows PC. `<ORCH>` = `D:/orch/moirai` (its own local git repository, never
  pushed to the public repository; a private backup repository is the owner's later call). The main checkout is
  expected at `D:/claude/moirai` (the default `xtask worktree` tests assume), worktrees at `D:/moirai-wt/<role>`
  (`git config moirai.worktree-root`), lane target directories at `D:/moirai-target/laneA|laneB`
  (`git config moirai.target-root`). The kit is PowerShell (Windows PowerShell 5.1 compatible).
- **R-2 Git.** Agents commit on their unit's role branch; only the orchestrator merges into local `master`, after a
  green `cargo xtask gate --branch m0/<role>` in the gate worktree, and then pushes `master` (`git push origin master`)
  automatically. This is a standing permission that replaces AGENTS.md's "Commit or push only when the owner asks"
  for this workflow. Never `--force`, `--no-verify`, hook bypass, `checkout --`, `restore`, `reset --hard`, `stash`,
  `clean`, `amend`. Author identity: the owner's (`Celtokisa`, the identity of the existing commits); never an AI
  co-author trailer or marker (the commit-msg hook and the gate enforce it).
- **R-3 Models.** Sonnet (`model: sonnet`) for mechanical stages only — a `mechanic` role that runs gates through the
  kit and summarises them, applies given patches, formats and rewraps, and does bookkeeping (RESUME rows); it never
  gives a verdict. Opus (`model: opus`) for every other role. The owner chose this against the brief's
  recommendation; the workflow doc records the brief's reason and that the mechanic's escapes are counted separately,
  so the choice can be revisited with data.
- **R-4 Machine share.** Agents may use half of the cores and RAM while the owner works, never more than the existing
  daytime limits of profile L ([60 §3.15], restated in docs/m0/nightly.md's sources) and the disk guard ([MP §8],
  docs/spec/measurement-protocol.md); unit capacity is set by measuring one gate's peak on the PC.

§9 and §10 apply R-3 and R-4. Chat with the owner may be in Russian; every file in the repository and in `<ORCH>`
is in English (AGENTS.md).

## 3. The orchestration state: `<ORCH>`

`<ORCH>` (`D:/orch/moirai`) lives outside the repository and outside every temporary directory: a temporary
scratchpad can be wiped while a campaign still needs its state. It is its own git repository, committed at every
rewrite of `RESUME.md`, and never pushed to the public repository (R-1). It holds:

| Path | What |
|---|---|
| `RESUME.md` | the state the orchestrator restarts from (skeleton below) |
| `RULINGS.md` | R-1 to R-4 (§2) and the orchestrator's numbered rulings from R-5 |
| `inventory.md` | the machine, the tools, the existing worktrees and branches, the kit hash and its self-test, the measured capacity and budgets (§15) |
| `baseline.tsv` | the test IDs (`<package>\|<source>\|<test>`, §7) and statuses of the whole workspace at tier `pr` on `master` (§15 step 8), refreshed per package at each merge (§5.4 step 6); no unit agent reads it (§5.2 step 6) |
| `mechanic-escapes.tsv` | the mechanic's escapes (§9) |
| `embed/<unit>.src.js`, `embed/<unit>.js`, `embed/args_<unit>.json` | the copy of `unit.js` taken at the unit's first launch, the embedded copy made from it, and the arguments of every launch (§6.3, §6.4) |
| `<unit>/` | `spec.md`, `baseline_unit.tsv`, `launch.md`, `run_<k>.txt`, `result_<k>.json`, `cut.md`, `critique.md`, `impl.md`, `gate_r<n>/` (a kit run directory) and `gate_r<n>.md`, `expected_r<n>.tsv`, `tests_r<n>-<m>/` (the tester's kit runs), `test_r<n>.md`, `review_r<n>.md`, `triage_r<n>.md`, `fix_r<n>.md`, `pack.md`, `run<k>/` (the round files of an earlier launch), `wt-<stage>/` (scratch worktrees) |
| `kit/` | the scripts of Appendix A, `budgets.txt`, `caps.txt`, `unit-allow.txt`, `guard.exe`, `locks/`, `selftest-runs/` |

Its `.gitignore` holds `/*/wt-*/`, `log.txt`, `/kit/locks/`, `/kit/selftest-runs/` and `/kit/guard.exe`.

`RESUME.md` skeleton:

```markdown
# RESUME

Trunk: master @ <sha> (pushed: yes|no). Updated <UTC time>.
Capacity: <n> units; build jobs <n> per lane; budgets gate <min>, tests <min>; kit <hash>.

## Units

| unit | role | WP | branch | worktree | base | stage | unit session | run id | script | lock set |
|---|---|---|---|---|---|---|---|---|---|---|

## Running gates

| PID | unit | run dir | budget (min) | started |
|---|---|---|---|---|

## Next, in order

1. …

## Waiting on the owner

- …
```

## 4. Units, worktrees and the trunk

- **One worktree per unit**, the role worktree; its settings carry the role's `deny_read` rules, the lane's target
  directory and `CARGO_BUILD_JOBS` (`xtask/src/worktree.rs`). Two agents never build or edit in one checkout at once:
  inside a unit the stages run one after another in the role worktree, and the tester and the reviewer, which run in
  parallel, only read it.
- **Scratch worktrees** for red-firsts on the parent and for mutations: `git worktree add --detach
  <ORCH>/<unit>/wt-<stage> <sha>`, removed with `git worktree remove` when done. They build into the session's lane
  target directory, never into a private one (the disk headroom is the binding resource, [MP §8]); cargo's build
  lock serialises them with the lane's other builds. `kit/local-settings.ps1` extends the role's `deny_read` rules to
  `<WT_ROOT>/**` and `<ORCH>/**`, so these paths are covered too.
- **Lock sets.** A file outside the unit's lock set, or in another open unit's set, is a STOP. The lock set lies
  inside what the author role may write (authors.md §3); two open units never share a path.
- **Concurrency**: at most the measured capacity (§10), one open unit per role.
- **Disk.** Every gate start checks the disk headroom first (the kit's pre-flight, §10): the volume's available space
  less the growth the counted directories of `kit/caps.txt` may still take up to their caps, as the guard counts it for
  the nightly runner ([MP §8.1]); below the floor, nothing starts. The lane target directories are shared and never
  deleted by a unit. Deleting an old probe or scratch target directory (tools.md §12 item 9) is the orchestrator's or
  the owner's call: never a directory a live process uses or one written in the last two hours unless the owner names
  it, and every deletion is logged in `inventory.md`.
- **Lockfile changes.** `Cargo.lock` and `fuzz/Cargo.lock` change only in the gate worktree (PLAN §3.1; authors.md
  §3), and every per-commit gate runs `--locked`. A unit that changes a manifest so that the lockfile must change:
  1. the developer commits the manifest change and STOPs with the reason `lockfile` (its `--locked` gates cannot run);
  2. the orchestrator runs the full gate once itself, through the kit in the lane's gate worktree
     (`start-gate.ps1 -RunDir <ORCH>/<unit>/lock_<k> -WorkDir <gateWt> -Unit <unit> -RunId <unit>-lock<k> -Kind gate
     -Detach m0/<role> -TargetDir <the lane's target directory> -CmdLine "cargo xtask gate --branch m0/<role>"`):
     its DONE says `lockfile=updated` (the `lock` step resolved the branch's manifests and the build steps ran on the
     result). A unit whose mechanic reported `LOCKFILE` on its own joins here with that run;
  3. the orchestrator relaunches with the resume note `Copy Cargo.lock (and fuzz/Cargo.lock when DONE's changes
     list it) from <gateWt> into <wt> byte for byte, commit it as WP-xx: Record the lockfile the gate resolved for
     <change>, then continue cut.md`;
  4. at the next gate start the kit finds the gate worktree's lockfiles equal to the branch's, stages them and
     switches on (`start-gate.ps1 -Detach`).
  A gate-worktree lockfile that no open unit takes blocks every later start there (exit 2, "lockfiles differ"): STOP
  and ask the owner, since the never-list rules out discarding it.

## 5. The orchestrator's loop

### 5.1 Classify each owner request

- **Direct** (at most about 30 lines in at most two files, an existing test covers it): still a unit, because every
  merge into `master` passes the gate (PLAN §3.1), but a short one: in the unit session a developer, then the
  mechanic's gate run, then a tester, through the Agent tool with the same role files; no cut, critique, review or
  triage.
- **Unit**: one reviewable portion of one WP (§1).
- **Campaign** (larger): a design workflow first (researcher, architect, architecture-critic; at most two rounds,
  the second delta-only), then a STOP for the owner's value calls, then the plan cut into rungs, one unit each. An
  agent that must read a crate's sources runs in a unit session of the author role allowed to read them.
- Before any launch: no agreed measurement or nightly window falls inside the expected run (§10, §14).

### 5.2 Launch a unit

1. **Spec.** Write `<ORCH>/<unit>/spec.md`: the WP and its author role (authors.md §2), the goal, the acceptance
   gates, what is out of scope, the RULINGS it rests on, the lock set. Units are named `u<nn>-<slug>`.
2. **Lock set.** Estimate it from the spec, inside the role's write map and disjoint from every open unit's set
   (`RESUME.md`). The cut STOPs if its touch set leaves it; the orchestrator rules and relaunches.
3. **Room.** The open units are fewer than the capacity, the role has no open unit, and the last `RESUME.md` row of
   the role is closed.
4. **Worktree.** In the main checkout (on `master`, clean): `cargo xtask worktree <role> --base master`. The command
   refuses an existing worktree directory and reuses an existing `m0/<role>` branch, ignoring `--base`; so an existing
   worktree, or a branch that is not merged into `master` (`git merge-base --is-ancestor m0/<role> master` fails), is
   open work of an earlier session: STOP and ask. A merged leftover branch is deleted first (`git branch --delete
   m0/<role>`).
5. **Local settings.** `powershell.exe -NoProfile -ExecutionPolicy Bypass -File <ORCH>/kit/local-settings.ps1 -Worktree
   <WT_ROOT>/<role> -Orch <ORCH> -WtRoot <WT_ROOT>`: it adds `permissions.additionalDirectories` `<ORCH>` and
   `<WT_ROOT>` (agents write their reports under `<ORCH>`, and the session may launch a script kept there only when it
   may read it), the allow rules of `kit/unit-allow.txt`, and the role's `deny_read` rules under `<WT_ROOT>/**` and
   `<ORCH>/**` with Read denies of `<ORCH>/baseline.tsv` and `<ORCH>/baseline/**` (step 6); it keeps every other key and
   rule.
6. **Arguments and the unit's baseline.** Write `<ORCH>/embed/args_<unit>.json` (§6.2): `base` = `git rev-parse
   master`, `gateWt` = the gate worktree of the role's lane, `dryRun` = `false`, `resumeNote` = `""`. Write
   `<ORCH>/<unit>/baseline_unit.tsv`: the first line of `baseline.tsv`, then its rows whose package is one of the
   unit's (the `-p` values of `gates`), for example `grep -E '^(xtask|moirai-vfs)[|]' <ORCH>/baseline.tsv`. A role
   writes and reads its own packages; should a `deny_read` module pattern of the role (`xtask/roles.toml`) name one of
   them, its rows whose test path lies in that module are dropped too, as the gate's filter drops them
   (`xtask/src/cargo.rs`). The tester builds its expected set from this file; `baseline.tsv` itself holds every
   crate's test names, which S2 withholds from most roles, and `kit/local-settings.ps1` denies it to every role with
   `deny_read` rules.
7. **Embedded copy.** Copy the main checkout's `.claude/workflows/unit.js` (on `master`) to
   `<ORCH>/embed/<unit>.src.js`, then `powershell.exe -NoProfile -ExecutionPolicy Bypass -File <ORCH>/kit/embed.ps1
   -Script <ORCH>/embed/<unit>.src.js -ArgsFile <ORCH>/embed/args_<unit>.json -Out <ORCH>/embed/<unit>.js`.
8. **Dry run** in the orchestrator session: `Workflow({ name: "unit", args: <the args with dryRun true> })` must
   return `{ ok: true, args }` with no agent started; it checks the script and the arguments.
9. **Launch file** `<ORCH>/<unit>/launch.md` for launch `k` = 1 (template in §6.3), then start the unit session
   (§6.3).
10. **Record.** A `RESUME.md` row: unit, role, WP, branch, worktree, base, stage `launched`, the unit session's name,
    the run id (from `run_1.txt` once it appears), the script `<ORCH>/embed/<unit>.js`, the lock set. Commit `<ORCH>`.

### 5.3 Between runs

- Wait for `<ORCH>/<unit>/result_<k>.json` in bounded slices (`kit/wait-gate.ps1 -File <path>`), doing other work
  between them.
- Read the result's fields only (§6.5). Open a report, by section (`rg -n`, ranged reads), only on `RED`,
  `CHANGES_REQUESTED`, `STOPPED`, a gate outcome other than `PASS`, or `ok: false`.
- Answer a question as a numbered ruling in `RULINGS.md` and relaunch with a resume note that cites it (§6.4).
- A `LOCKFILE` outcome: §4 "Lockfile changes". A `HANG`, `VOID` or `VACUOUS` gate: read the kit run's DONE and the
  failure excerpt, rule, and relaunch.
- A unit still red after round 3 is not merged: rule, then relaunch fix-only (`DELTA SINCE <head>: fix …`) or split it
  into two units.
- A unit session under `Needs input` in agent view (the first workflow launch, a permission prompt, a classifier
  pause, §8) waits for the owner: STOP and ask the owner to attach to it (`→` in `claude agents`). A reply from the
  peek panel does not answer a dialog (Claude Code agent-view documentation).
- Copy the mechanic escapes the tester reported into `<ORCH>/mechanic-escapes.tsv` (§9).
- Rewrite `RESUME.md` and commit `<ORCH>` after every change of a unit's stage.

### 5.4 Merge (the orchestrator only)

1. **The result.** `ok` is true; `gate` is `PASS`; `kit` equals the current kit hash (`kit/kit-hash.ps1`); `head`
   equals `git rev-parse m0/<role>` (nothing was committed after the verified head); the final `gate_r<n>/DONE` says
   `verdict=PASS`, `lockfile=current` and an empty `changes`, and its `tree` equals the result's `tree`.
2. **The trunk.** In the main checkout, on `master`, `git status --porcelain` prints nothing. If
   `git merge-base --is-ancestor master m0/<role>` fails, the trunk moved: relaunch the unit with the resume note
   `DELTA SINCE <head>: trunk sync only: git merge --no-ff master; resolve a conflict only in a lock-set path, by
   editing and keeping both sides (a conflict in any other path is a STOP); then the per-commit gates of gates (the
   role's own crates) only; no other change.` The re-run is delta-scoped to the unit's own changes and the merge's
   conflict resolutions, never a diff across the merge (§6.1); the mechanic's full gate, filtered for the role, covers
   the rest on the new head. Then start again at step 1.
3. **Merge**: `git merge --no-ff m0/<role> -m "Merge m0/<role>: <unit>"`. The subject names no WP, so `xtask
   authors` treats the merge as an owner commit outside `--branch` mode (authors.md §5 item 6); the commit-msg hook
   checks it for AI markers.
4. **The tree.** `git rev-parse HEAD^{tree}` must equal the verified `tree`. If it does not, do not push: STOP and ask
   the owner (the never-list rules out undoing the merge).
5. **Push** (R-2): `git push origin master`. A refusal (not a fast-forward, a branch ruleset that requires a pull
   request, PLAN §5 V10, the network) is not retried in another form: record it under "Waiting on the owner" and STOP.
6. **Clean up.** The unit session has nothing running. `git worktree remove <WT_ROOT>/<role>` (it refuses a dirty
   worktree: STOP); `git branch --delete m0/<role>` (it refuses an unmerged branch: STOP), so that the role's next unit
   is made from `master` and not from the reused old branch; `git worktree remove <ORCH>/<unit>/wt-<stage>` for every
   scratch worktree left, then `git worktree prune`. The lane target directory stays. In `baseline.tsv`, replace the
   rows of each package the final round's tests runs covered on the verified head with those runs' `id` and `status`
   columns, so that the next unit's expected set matches `master`. Close the row in `RESUME.md`, update the trunk line
   (`pushed: yes`) and commit `<ORCH>`.

## 6. The unit workflow

### 6.1 Stages

`.claude/workflows/unit.js` (the named workflow `unit`): **cut → critique → implement → (mechanic's gate → tester) ∥
review → triage → fix → delta re-verify**, at most three verify rounds.

- **Cut** (architect): `cut.md`, the spec re-located on the tree, the touch set, the commits with their gates and
  red-firsts, the expected test set by name. **Critique** (architecture-critic), one round: `critique.md`.
- **Implement** (developer): commit by commit, red-first, per-commit gates only. It resolves each Critical and
  Important of the critique or refutes it in an addendum at the end of `cut.md`; a Critical that changes the design
  returns the unit to the orchestrator; the round-1 reviewer checks the addendum.
- **Verify**, in parallel because the roles differ (independent refutation), not to save tokens: one arm is the
  mechanic's full gate run through the kit (`gate_r<n>.md`) followed by the tester, who checks that report against
  the kit's own files and adds its own red-firsts, at least two mutations, the pins and the tests counted by name; the
  other arm is the code-reviewer. Round 1 covers the whole unit (`git diff master...HEAD`, read only in lock-set
  paths); later rounds, and a round 1 after a `DELTA SINCE` relaunch, are delta-scoped to the unit's own changes
  (`git log --first-parent <since>..HEAD`: each commit's `git show`, and for a trunk sync only its conflict
  resolutions, `git show --remerge-diff`), with at least two new mutations. A diff across a trunk sync
  (`git diff <since>..HEAD`) is never read: it carries other roles' changes, which S2 may withhold. The mechanic
  runs the full gate in every round, so the last round's run is the verdict on the final head.
- **Triage** (code-reviewer as the adversarial verifier) tries to refute every red, every gate FAIL and every
  Critical and Important; it narrows the fix to what it confirms. Nothing confirmed returns the unit to the
  orchestrator. **Fix** (developer): follow-up commits, never an amend.
- A stage that returns nothing, or a gate outcome other than `PASS` or `FAIL`, returns the unit to the orchestrator
  at once.
- **Context pack** (optional): when two or more checkers need the same orientation, a scout first writes
  `<unit>/pack.md` (at most 15k tokens: files and line ranges, the delta, key facts) and the briefs point at it.

Every `agent()` call passes `agentType` = the role file's name and a `schema`, so every stage answers with a
structured verdict and the script parses nothing.

### 6.2 Arguments

| Field | Value |
|---|---|
| `role` | the author role's name in `xtask/roles.toml` (`r-harn-i`) |
| `unit` | `u<nn>-<slug>` |
| `orch` | `D:/orch/moirai` |
| `wt` | `D:/moirai-wt/<role>` |
| `gateWt` | `D:/moirai-wt/gate-a` or `gate-b`, by the role's lane |
| `branch` | `m0/<role>` (the script refuses anything else) |
| `base` | the `master` sha the unit started from |
| `trunk` | `master` |
| `spec` | `D:/orch/moirai/<unit>/spec.md` |
| `lock` | the lock set, as one string of paths |
| `others` | the other open units and their lock sets, or `none` |
| `gates` | the per-commit gate commands of AGENTS.md with the role's crates filled in |
| `diskFloorGb` | the disk floor of the pre-flight (default 25, [MP §8.1]; operational policy, AGENTS.md) |
| `resumeNote` | `""` on a first launch (§6.4) |
| `dryRun` | `true` returns `{ ok: true, args }` before any agent starts |

### 6.3 Launching

- **By name with args**: in a session whose project holds `.claude/workflows/unit.js`, `Workflow({ name: "unit",
  args: {…} })`, or the prompt "Run /unit with the arguments in `<ORCH>/embed/args_<unit>.json`" (Claude passes them
  as structured data). This is the dry run of §5.2 step 8.
- **By the embedded copy**, for every real launch: `Workflow({ scriptPath: "<ORCH>/embed/<unit>.js" })`, with
  `resumeFromRunId` on a relaunch. The copy pins the script: a relaunch must replay the same script, and `master`'s
  `unit.js` can change between a launch and its relaunch, so every relaunch embeds from `<ORCH>/embed/<unit>.src.js`,
  the copy taken at launch 1 (§5.2 step 7). `kit/embed.ps1` writes it with LF line endings (the runtime rejects CR as
  a hidden control character).
- **The unit session** launches it. The orchestrator starts the session from the role worktree:
  `cd <WT_ROOT>/<role> && "<claude>" --bg --permission-mode auto --name unit-<unit> "Read <ORCH>/<unit>/launch.md and
  do exactly what it says."`, where `<claude>` is the executable recorded in `inventory.md` (tools.md §9: neither
  install is on `PATH`). The permission mode is §8's. A background session keeps its workflow running when no terminal
  is open, and one started inside a linked worktree, as a role worktree is, edits it in place instead of moving into
  a worktree of its own (Claude Code agent-view documentation). Workspace trust is keyed on the main checkout's root
  for every worktree of the repository (permissions documentation), so the trust of `D:/claude/moirai` (§15 step 1)
  covers the role and gate worktrees; if `claude --bg` still exits with `Workspace not trusted`, STOP and ask the
  owner to open Claude Code in the role worktree once and accept the dialog. Where `--bg` is not available (§15 step
  1), STOP and ask the owner to open a session in the role worktree with the same permission mode and paste the one
  line.
- **`launch.md`** (rewritten for every launch `k`):

  ```markdown
  # Launch <k> of unit <unit>

  You are the unit session of <unit>, in <WT_ROOT>/<role>. Do only this, then end your turn:
  1. Start the workflow with the Workflow tool: scriptPath <ORCH>/embed/<unit>.js[, resumeFromRunId <run id>].
  2. Write the run id the tool returns to <ORCH>/<unit>/run_<k>.txt.
  3. When the workflow returns, write its return value as JSON to <ORCH>/<unit>/result_<k>.json. If it fails to
     start or ends with an error, write {"ok": false, "stage": "launch", "note": "<the error>"} there instead.
  Do not read the reports, fix anything or launch anything else, and do not commit, push or open a pull request:
  the workflow's agents commit on the unit's branch (AGENTS.md "Agent work") and the orchestrator merges.
  ```

### 6.4 Relaunch and resume

- A relaunch replays the earlier run in the order its agents started: a completed agent whose prompt is unchanged
  returns its saved result; the first agent whose prompt changed, and every agent after it, runs again; a failed
  agent runs again with every agent that started after it (Claude Code workflows documentation). The same script with
  the same arguments therefore returns every agent from the journal, which §15 step 11 uses as a check.
- New state goes only into the step that must re-run. `resumeNote` reaches only the implement prompt, so the cut and
  the critique replay from the journal. A changed header field (`lock`, `others`, `spec`, …) re-runs the cut and
  overwrites `cut.md`: change one only on purpose.
- A resume note that starts `DELTA SINCE <sha>:` makes round 1 delta-scoped since that verified head: a trunk sync
  (§5.4 step 2), a fix-only relaunch, a lockfile commit (§4).
- Before a relaunch: move the round files of the earlier launch (`impl.md`, `gate_r*`, `expected_r*`, `tests_r*`,
  `test_r*`, `review_r*`, `triage_r*`, `fix_r*`) into `<unit>/run<k>/` (the re-run stages write the same names, and the
  kit refuses an existing run directory); update `args_<unit>.json` and re-run `kit/embed.ps1` with `-Script
  <ORCH>/embed/<unit>.src.js` (never the current `unit.js`, and never the embedded copy: it has no marker line); rewrite
  `launch.md` for `k + 1` with the run id; then send the unit session the same one line (in agent view, or by resuming
  it with `claude --resume` from the role worktree).
- A run is resumable from the session that ran it. Across sessions, resume that session and relaunch with the run
  id. Copying a run's directory under another session's directory in `~/.claude/projects/` is an undocumented
  internal path: a fallback only. A relaunch that reports "nothing to resume" starts over as a new run (new `k`).
- Scripts are plain JavaScript; `Date.now()`, `Math.random()` and a bare `new Date()` throw in the runtime, so
  anything time-dependent comes in through the arguments.

### 6.5 What the workflow returns

`ok`, `unit`, `stage` (`verify` when the rounds ran out or finished; else the stage that returned it: `args`, `cut`,
`critique`, `implement`, `gate`, `test`, `review`, `triage`, `fix`), `note`, `round`, `crit`, `gate` (the mechanic's
outcome), `head`, `tree`, `kit`, `test`, `review`, `critical`, `important`, `reports` (the unit's directory): verdicts
and paths, about a few hundred tokens. Nothing else is read until a verdict calls for it.

## 7. The gate kit

A background shell of the agent harness can be reaped while a long gate still runs, and its children die with it.
The kit therefore runs each long gate in its own minimized console (`Start-Process -WindowStyle Minimized`), under a
time budget, and leaves its state on disk so that any agent, or a new session after a crash, can wait on it, judge it
or stop it.

- **Scripts** (Appendix A; Windows PowerShell 5.1, ASCII): `start-gate.ps1` (start one run), `run-gate.ps1` (the
  detached runner it starts), `wait-gate.ps1` (one bounded wait), `stop-gate.ps1` (stop by PID tree and archive),
  `preflight.ps1` (the floors, through the guard of [MP §8]), `embed.ps1` (§6.3), `local-settings.ps1` (§5.2 step 5,
  §8), `kit-hash.ps1`, `kit-common.ps1` (shared helpers), `selftest.ps1` and `selftest-fake.ps1` (the red-proof).
  Beside them: `budgets.txt` (`gate=<minutes>`, `tests=<minutes>`, §15 step 9), `caps.txt` (the counted directories
  of the pre-flight and their caps, `<dir>=<cap>` per line, from `xtask/nightly.toml` `[guard.caps]`, §10),
  `unit-allow.txt` (§8), `guard.exe` (a copy of `moirai-probes-bin guard`, §15 step 5) and `locks/` (one lock per
  work directory).
- **A run directory** holds `params.txt` and `cmd.txt` (written before the start), `PID` and `owner.txt` (unit, run
  id, kind, the runner's PID and start time, the budget, the work directory, the kit hash, `head`), then `child.pid`,
  `tree.txt` (every process of the run's tree, `pid=start time`), `peak.txt` (the tree's peak working set in bytes),
  `log.txt` (stdout and stderr in order), `exitcode.txt`, `summary.tsv` (one row per gate step or per test, the first
  line stamped with the kit hash) and, last, `DONE` (`verdict`, `exit`, `command_exit`, `kit`, `head`, `tree`,
  `lockfile`, `changes`, `finished`, `note`). A stopped run is renamed `<run dir>.stopped-<UTC time>` with a `STOPPED`
  file: its outputs are void.
- **Verdicts.** `PASS` (exit 0); `FAIL` (1: the command exited non-zero, a row failed — even when the command itself
  exited 0 —, an expected test is missing, or the run changed a file other than a lockfile); `VACUOUS` (7: no gate rows,
  or zero tests: a vacuous green is red); `VOID` (8: the runner failed); `HANG` (124: past the budget, stopped by its
  PID tree). A tests run also fails when a test binary has no package (§7 "Test IDs"). `start-gate.ps1` exits 0
  (started), 1 (pre-flight refused), 2 (usage or precondition: an existing run directory, no `CARGO_TARGET_DIR`, a gate
  worktree that is not clean, a tests run of cargo without `--message-format=json-render-diagnostics`, no `caps.txt`) or
  6 (the work directory is busy). `wait-gate.ps1` exits 0 (DONE), 3 (still running within the budget), 4 (past the
  budget: a hang) or 5 (the runner is gone without DONE).
- **Commands agents run** (from the Bash tool, each after `cd` into its directory):
  - the full gate (the mechanic): `powershell.exe -NoProfile -ExecutionPolicy Bypass -File <ORCH>/kit/start-gate.ps1
    -RunDir <ORCH>/<unit>/gate_r<n> -WorkDir <gateWt> -Unit <unit> -RunId <unit>-r<n> -Kind gate -Detach m0/<role>
    -DiskFloorGb <floor> -CmdLine "cargo xtask gate --branch m0/<role>"`;
  - tests counted by name (the tester): the same script with `-RunDir <ORCH>/<unit>/tests_r<n>-<m> -WorkDir <the
    tree under test> -RunId <unit>-t<n>-<m> -Kind tests -Expected <ORCH>/<unit>/expected_r<n>.tsv -CmdLine "cargo
    test --locked --message-format=json-render-diagnostics -p <crate> … --no-fail-fast"` (the kit sets
    `MOIRAI_TEST_TIER=pr`; `.claude/agents/tester.md` carries the whole sequence);
  - a bounded wait: `… wait-gate.ps1 -RunDir <run dir>` (540 s by default; the shell tool's timeout above it), one
    call per slice, never a sleep loop in the shell;
  - a stop: `… stop-gate.ps1 -RunDir <run dir> -Reason "<why>"`. Never `taskkill /IM` or any kill by image name: it
    stops every unit's builds.
- **Test IDs** are `<package>|<source>|<test>` (`moirai-vfs|unittests src/lib.rs|a::b`, `moirai-vfs|tests/basic.rs|x`,
  `moirai-vfs|doc-tests|<name>`). The source is what cargo's `Running` line prints; the package comes from the
  `compiler-artifact` message that announced the executable, which is why a tests run of cargo must pass
  `--message-format=json-render-diagnostics`, as the gate's own filter attributes binaries (`xtask/src/cargo.rs`). Two
  crates with a `tests/props.rs` are told apart, and an ID does not depend on the `-p` set of the run, so a unit's
  expected set compares with the whole-workspace baseline. A test binary that no message announced gets the package
  `?<executable stem>` and fails the run; a repeated ID gets `#<n>` in output order. An expected-set file holds one ID
  per line, optionally a tab and the status, `#` for comments; `baseline.tsv` has the same form. The tests kind runs
  `cargo test` directly, without the gate's poisoned C toolchain variables ([90 §11.1]): a crate excluded from the gate
  as host-only is excluded here too.
- **The kit hash** (SHA-256 over every kit script) is stamped into every `summary.tsv` and `DONE`. A gate verdict
  counts for a merge only with the current hash (§5.4 step 1). Any change to a kit script changes the hash: run
  `selftest.ps1` again and record the new hash and its result in `inventory.md`.
- **Red-proof.** A gate script is trusted only after it was seen exiting non-zero on a failing row (AGENTS.md).
  `selftest.ps1` runs every kit script the way an agent does on synthetic inputs and checks: a failing row with exit 0,
  a non-zero exit with passing rows, zero rows, zero tests, a missing expected test, test IDs that carry the package
  (two packages with one test file name, a binary no message announced), a tests run without JSON messages, a hang
  stopped by its PID tree, a bounded wait on a live run, a busy work directory, a stop and its archive, a runner that
  dies (the orphaned command is stopped by its recorded PID), the pre-flight below each floor and with a capped
  directory's reserve, through the fallback and the guard, the embedded copy, the local settings, and the `-Detach`
  checkout with its lockfile rules. Each script's own red-proof is the selftest case named after it in Appendix A.

## 8. Settings

**Committed, `.claude/settings.json`** (R-HARN; a change needs the owner's approval, authors.md §3):

- `attribution` (empty commit and PR attribution, no session URL) and `includeCoAuthoredBy: false` stay (A2).
- `subagentPromptCacheTtl: "1h"`: subagents and workflow agents fall outside the main conversation's cache bucket
  and default to a five-minute cache TTL even on a subscription; a unit agent that waits on a build longer than that
  re-writes its whole context into the cache on its next turn. One-hour writes are billed at a higher rate; they pay
  off because unit agents idle past five minutes on builds. Needs Claude Code 2.1.242 or later (prompt-caching
  documentation, "Choose the TTL yourself").
- `permissions.allow`: the gate commands (`cargo xtask gate`, `--branch *`, `--ci`, `--list`), `cargo xtask worktree
  *`, the per-commit gates (`cargo fmt -p *`, `cargo clippy --locked -p *`, `MOIRAI_TEST_TIER=pr cargo test --locked
  -p *`), `git status|diff|log|show|add|commit|merge|merge-base|worktree|switch|branch|rev-parse *`, `git push origin
  master` and `Workflow`. Workflow agents use the session's rules, and an unattended unit otherwise stalls on its
  first prompt. `Workflow` approves a launch only where Claude Code evaluates it as a tool call (`claude -p`, the
  SDK); in an interactive or background session the launch prompt follows the permission mode (workflows
  documentation; "Permission mode" below).
- `permissions.deny`: forced pushes (`--force*`, `-f*`, a `+` refspec), `reset --hard`, `checkout --`, `restore`,
  `stash`, `clean`, `commit --amend`, `--no-verify` and `-c core.hooksPath=`, and the forced forms the broad allow rules
  would otherwise approve: `checkout -f|--force|.`, `switch -f|--force|--discard-changes`, `worktree remove -f|--force`,
  `branch -D|-f|--force` and `merge --abort` (§13: a merge is never aborted). A merged branch is deleted with
  `git branch --delete` (§5.4), which no deny rule matches. A deny rule matches the command as written: `git -C . push`,
  a full path to git or `sh -c` is not matched (permissions documentation), so the list is a backstop and AGENTS.md's
  never-list is the rule.
- No hooks. No PreToolUse "nudge" hook that injects text on every call (each injection is re-read for the rest of the
  agent's life), no hook or index tool that agents must use. Hooks are optional accelerators (AGENTS.md).

**Local, `.claude/settings.local.json`** (this machine, untracked): `kit/local-settings.ps1` adds to the main checkout
(§15 step 6) and to every new role worktree (§5.2 step 5) `permissions.additionalDirectories` `<ORCH>` and
`<WT_ROOT>`, the allow rules of `kit/unit-allow.txt`, and in a role worktree the role's `deny_read` rules under
`<WT_ROOT>/**` and `<ORCH>/**`, with the baseline's Read denies for a role that has `deny_read` rules (§5.2 step 6).
`unit-allow.txt` starts with the kit's one rule:

```text
# Local allow rules for the main checkout and every role worktree (kit/local-settings.ps1).
Bash(powershell.exe -NoProfile -ExecutionPolicy Bypass -File D:/orch/moirai/kit/*)
```

A prompt that stopped an agent of the trial unit (§15 step 10) gets the narrowest rule that covers it, here, and the
rule then reaches every later unit.

- Allow rules and additional directories wait for workspace trust, and the main checkout's trust covers every
  worktree of the repository (permissions documentation, §6.3).
- The rules are Bash rules: agents use the Bash tool (Git Bash on Windows). If the PowerShell tool is turned on, the
  same rules would be needed as `PowerShell(…)` rules.
- **Permission mode.** A unit session runs in auto mode (`--permission-mode auto`, §6.3), and its subagents and
  workflow agents run in the session's mode (sub-agents documentation). In Manual mode every edit and every workflow
  launch would prompt (additional directories make reads, not edits, prompt-free; "don't ask again" is offered only
  for a workflow launched by name), and in `acceptEdits` every launch and every command outside the allow rules
  would, such as a `cd` into a scratch worktree followed by `git` (permissions and workflows documentation). In auto
  mode a classifier reviews the actions instead; deny rules still apply, and the classifier does not count a
  workflow's computed prompts as the owner's requests. A unit can still stop for the owner: the first workflow
  launch in auto mode asks once (the answer is kept in the user settings); writes to protected paths such as
  `.claude/` and `.cargo/` go to the classifier and may be refused, so a unit whose lock set holds them is launched
  only with the owner present; and the classifier pauses auto mode after three refusals in a row or twenty in all
  (permission-modes documentation). The session then waits under `Needs input` (§5.3). Without auto mode
  (unavailable to the account or the model), units run only with the owner attached: §15 step 1 records which.
- On a subscription within the plan's usage, the main conversation already gets the one-hour TTL. On usage credits, an
  API key or a cloud provider, also set `"promptCacheTtl": "1h"` in the user settings: the orchestrator waits on
  workflows.

## 9. Model routing (R-3)

- `mechanic` runs on Sonnet; every other role on Opus (`model` in each role file). A per-call `model` on an
  `agent()` call is used only for a measured exception.
- **The brief's reason against it.** On ground-truthed retrieval questions the cheaper tier was confidently wrong
  several times where the strongest tier never was, and a confident wrong answer is the defect nobody downstream
  catches. Lower reasoning effort also measured worse. Its rule: the strongest model for every role; downgrade a role
  only after an A/B on at least about 25 ground-truthed items that scores confident-wrong separately from missed (a
  small pilot that "ties" proves nothing).
- **How the risk is held.** The mechanic gives no verdict: it copies the kit's outcome, and the tester checks every
  gate report against the kit's own files (`DONE`, `summary.tsv`) before relying on it. Every disagreement is a row of
  `<ORCH>/mechanic-escapes.tsv`: `unit`, `round`, `report`, `kind` (`confident-wrong`: a wrong outcome, head, tree or
  row stated as fact; `missed`: a failing row or a note left out), `detail`.
- **What would change it.** After at least 25 audited gate reports, any `confident-wrong` row is a reason to propose
  returning the mechanic to Opus; the owner rules. Moving any other role off Opus needs the brief's A/B first.

## 10. Resources (R-4)

- **The share.** Half of the logical cores and half of the RAM while the owner works, and never more than profile
  L's daytime limits ([60 §3.15]): the build lanes with their build semaphore (cargo's lock on the lane's target
  directory) and `CARGO_BUILD_JOBS` cap, no rust-analyzer in role worktrees, fuzzers sanitizer-off at no more than two
  targets with `-rss_limit_mb=256`, gate jobs beside the agents within 1 GB in total, and everything refused below
  1.5 GB of free RAM.
- **Proposed ruling R-5: what the 1 GB covers** (the owner rules, §15 step 7). [60 §3.15] lists "the two build lanes"
  apart from "gate jobs beside the agents ≤ 1 GB in total", and `xtask/nightly.toml` `[window.beside-agents]` applies
  the 1 GB to the nightly runner's jobs (PLAN WP-05); PLAN §4 lists it among the general parallelism limits. The
  proposed reading: `cargo xtask gate` and the per-commit gates are build-lane work, held by the lane's build
  semaphore and `CARGO_BUILD_JOBS` within R-4's half of the RAM, and the 1 GB binds the nightly-tier jobs run beside
  the agents. Under the other reading no full gate fits beside the owner's work, and units gate only in agreed
  windows. This is the orchestrator's reading, not the owner's: no full gate starts before the owner rules.
- **Build jobs.** `git config moirai.build-jobs` = half the logical cores divided by the number of lanes that may build
  at once (two), until measurement 21 sets the lane cap (`xtask/src/worktree.rs`, `.cargo/config.toml`). Role
  worktrees made after a change get the new value.
- **Disk.** Before every gate start, the pre-flight's floor (`diskFloorGb`, by default 25 GB, [MP §8.1]) on the
  headroom: the available space less the growth the directories of `kit/caps.txt` may still take up to their caps.
  They are the four counted directories under the target root (`laneA`, `laneB`, `fuzz`, `mutants`) with the caps
  of `xtask/nightly.toml` `[guard.caps]` (nightly.md §3), as the nightly runner passes them to the guard; a change of
  `[guard.caps]` on `master` is copied into `caps.txt`. With today's caps the headroom on the PC can be 0 (nightly.md
  §7 item 3): §15 step 5 checks it before the first gate.
- **Capacity**: the number of units open at once, measured on the PC (§15 step 9): the largest number whose units'
  peak (one full gate's process tree, from `peak.txt`, plus the unit session's own working set) fits in half the RAM
  while at least 1.5 GB stays free. Builds of units in one lane serialise on cargo's lock; the other stages (cut,
  critique, review) do not build.
- **Windows.** No unit runs during an agreed measurement or nightly window (`/private/windows.toml`, nightly.md §2,
  §14).

## 11. Token economy

- Cost is turns times context. Almost all of the token volume is cache re-reads, and most of what is re-read is
  context growing inside one agent; the number of agents is a minor term (spawn, brief and re-orientation).
- **Do not merge stages to save agents.** A merged agent re-reads the first stage's work on every turn of the second,
  which costs more, and merging kills independent refutation. Parallel arms buy wall time, not tokens: run them when
  the inputs or the roles differ.
- **Parallel checkers start from one context pack** (§6.1) when they need the same orientation; otherwise each
  re-explores the same files before its first action. The shared prompt prefix (system prompt, AGENTS.md, the
  workflow's header) is already one cache.
- **Keep caches warm**: the one-hour subagent TTL (§8), detached gates with budgets, bounded waits.
- **Logs stay on disk.** A large tool output kept in an agent's context is re-read on every later turn.
- **Hand off through files.** A resume is a commit plus a handoff of at most a page. Split a long agent only at a
  commit, past about a third of its context window.
- **One owner per heavy gate per round**: the mechanic runs the full gate; the developer runs only the per-commit
  gates; the tester runs the touched crates' tests, not the full gate.
- **Rounds after the first are delta-scoped**, with at least two new mutations: a round-2 mutation can find the
  round's only defect while the review says APPROVED.
- **Keep every checking arm** (critique, triage, the tester's mutations, review each round): they catch the defects.
  Cut one only on a measured escape-rate difference over at least about 25 items.
- **A small always-loaded floor**: a lean AGENTS.md and memory index; typed agents with small tool sets start much
  lighter than a general-purpose agent.
- **The orchestrator**: workflows return verdicts and paths (at most about 2k tokens); reports are read by section;
  the orchestrator session restarts from `RESUME.md` past about 40 % of its context window.

## 12. Memory

- Claude Code's auto memory lives in `%USERPROFILE%\.claude\projects\<project>\memory\`: a `MEMORY.md` index loaded in
  every session and one file per fact. The project is the session's directory, so the orchestrator's memory is the
  main checkout's, and a role worktree's memory starts empty each time the worktree is made: unit agents rely on their
  briefs, never on memory.
- One fact per file (`user`, `feedback`, `project` or `reference`, each with Why and How to apply); the index at one
  short line per file and at most about 15 KB; a "read first" block that holds only the current state line (older
  checkpoints live in the project file).
- An owner ruling is recorded in `RULINGS.md`; a feedback memory points at its number. A measured gotcha is a
  reference memory. Nothing that the repository, its history or `<ORCH>` records is stored again.

## 13. Crash and resume

1. **Orphans.** For every kit run with an `owner.txt` and no `DONE`: if its unit's workflow is not running now (the
   unit session's `/workflows`, the result file), it is an orphan: `stop-gate.ps1 -RunDir <run dir> -Reason orphan`.
   Judge by the unit's workflow, not by process ancestry: kit runs are detached on purpose. An orphan keeps spawning
   builds for hours.
2. **Void outputs.** Gate outputs written before the crash are void; `stop-gate.ps1` archives them as
   `<run dir>.stopped-<time>`. After a machine crash, a corrupt build artifact (a linker or compiler access violation,
   a nonsense link error) is cleaned for that package (`cargo clean -p <package>` with the lane's
   `CARGO_TARGET_DIR`) and rebuilt; the lane directory itself is never deleted.
3. **A worktree left in the middle of `git merge`**: never abort. A developer agent of the unit's role reviews the
   resolved files against both parents (`git show :1:<file>`, `:2:`, `:3:`), in lock-set paths only (a conflict in
   any other path is a STOP), adds them, re-runs the per-commit gates and commits the merge.
4. **Relaunch** each workflow in its unit session with its run id, the new state only in the step that must re-run
   (§6.4).
5. **Rewrite `RESUME.md`** and commit `<ORCH>`.

## 14. Performance measurement

The project measures performance under its protocol ([MP]; [60 §5.2]); the brief's measurement rules map onto it:

- **Quiet windows** are the protocol's idle condition ([MP §2.3]): agreed agent-free windows (`/private/windows.toml`,
  PLAN §5 V4) with no agent session, build, test, load generator or other measurement running. The orchestrator
  launches nothing whose expected run overlaps one, and stops launching before it starts.
- **Count-based runs**: the protocol's tiers, plans and interleaved blocks ([MP §3], [MP §4.3]) fix the processes and
  iterations, not the wall time.
- **Same machine, paired**: the arms are interleaved in one run ([MP §4.1], [MP §4.3]); numbers from different hosts,
  toolchains or conditions are never mixed ([MP §2.1], [MP §7.1]).
- **A "machine was quiet" receipt per process** is worth having, but its pause must stay short beside the samples, or
  it dominates the window's time.
- **The timer** ([MP §4.4], [MP §4.5]): samples far above the timer's resolution need no more repetitions on a faster
  machine.
- Results are judged by a results-analyst against their budgets and gates ([MP §5], [MP §7.3]).

## 15. Local setup

A Claude Code session on the owner's PC follows these steps in order, in the main checkout `D:/claude/moirai`; it then
stays the orchestrator session. Every **STOP** is a question to the owner (in Russian if the owner writes Russian),
asked once, with this session's recommendation; steps that do not depend on the answer go on meanwhile. Every
finding goes into `<ORCH>/inventory.md` (step 1's once step 3 has made `<ORCH>`).

1. **Check the clone and the tools** (read-only, except the fetch).
   - `git rev-parse --show-toplevel` is `D:/claude/moirai`; `git branch --show-current` is `claude/prodolzhai-g331bu`
     (the branch that holds this file); `git status --porcelain` prints nothing.
   - `git fetch origin`; `git rev-parse HEAD` equals `git rev-parse origin/claude/prodolzhai-g331bu` (the message
     that starts this setup has the session fast-forward to it first; any other difference: **STOP**); `git rev-parse
     master` equals `git rev-parse origin/master`. If `master` is behind and `git merge-base --is-ancestor master
     origin/master` holds, **STOP** and propose `git merge --ff-only origin/master` on `master`; any other difference
     is a **STOP** too. A stale `master` would make step 2's merge and every later push (R-2) a non-fast-forward.
   - This session runs in `D:/claude/moirai` with its workspace trust accepted (the dialog appears when Claude Code
     first opens a folder); that trust covers every worktree of the repository (§6.3).
   - `git config core.hooksPath` is the absolute `D:/claude/moirai/.githooks` and `git config moirai.private-guard` is
     `true` (PLAN §2.5). If not: **STOP** — the owner runs these two commands once per clone, or allows this session
     to.
   - `git config user.name` and `user.email` equal the identity on `master`'s commits
     (`git log -5 --format="%an <%ae>" master`). If not: **STOP**.
   - `git worktree list` and `git branch --list "m0/*"`: every existing worktree and role branch is recorded. An
     existing role worktree or an unmerged `m0/*` branch is open work of an earlier session: it is never touched, and
     no unit is launched for that role until the owner rules.
   - `git config moirai.worktree-root`, `moirai.target-root` and `moirai.build-jobs` (unset means `D:/moirai-wt`,
     `D:/moirai-target` and 6).
   - Claude Code: its executable path and version (tools.md §9; at least 2.1.242), whether `--bg` and
     `--permission-mode` appear in its `--help`, and whether auto mode is available (`auto` in the `Shift+Tab` cycle or
     the status bar of this session; §8 "Permission mode"); the Bash tool is Git Bash (`echo $BASH_VERSION`);
     `powershell.exe -NoProfile -Command '$PSVersionTable.PSVersion'` prints 5.1; `git --version` is at least 2.36
     (`git show --remerge-diff`, §6.1).
2. **STOP: merge the branch.** Ask the owner whether `claude/prodolzhai-g331bu` is merged into `master` now, before
   the first unit (this merge is the owner's call, not R-2's), and whether it is then pushed. Units are made from
   `master` and need `.claude/agents/` and `.claude/workflows/` there. With the owner's word: `git switch master`,
   `git merge --no-ff claude/prodolzhai-g331bu`, and `git push origin master` if the owner said so. Steps 3–7 do not
   need the answer; steps 8–12 wait for it.
3. **`<ORCH>`.** Create `D:/orch/moirai` with `embed/`, `kit/` and `kit/locks/`; `git init` there; set the same
   `user.name` and `user.email` as the main checkout; write the `.gitignore` of §3; commit. **STOP: a private backup
   for `<ORCH>`.** Ask whether to add a private remote. The default is none (R-1), and never the public repository;
   continue without one until the owner answers.
4. **`inventory.md`**: the OS edition and build (`cmd /c ver`), logical cores and total RAM (`Get-CimInstance
   Win32_ComputerSystem`), free space on C: and D:, the versions of git, rustup and cargo, Claude Code and PowerShell
   from step 1, the CI (`.github/workflows/pr.yml` runs `cargo xtask gate --ci` on hosted Windows runners), the gate's
   steps (`cargo xtask gate --list`), the existing `.claude/` files, and step 1's worktrees and branches. Facts about
   the machine and the tree only; no owner data.
5. **The kit.** Write every file of Appendix A into `D:/orch/moirai/kit/` exactly as printed (ASCII), then
   `budgets.txt` with `gate=30` and `tests=30` (until step 9), `unit-allow.txt` as in §8, and `caps.txt`: a comment
   line naming `master`'s sha, then one line per entry of `xtask/nightly.toml` `[guard.caps]` on `master`,
   `<target root>/<name>=<cap>` (`D:/moirai-target/laneA=40GB`, `laneB`, `fuzz`, `mutants`). Build the guard
   (`cargo build -p moirai-probes-bin --bin guard --locked` with `CARGO_TARGET_DIR=D:/moirai-target/laneA`) and copy
   the executable to `D:/orch/moirai/kit/guard.exe` (the nightly runner keeps its own copy, nightly.md §1). Run
   `powershell.exe -NoProfile -ExecutionPolicy Bypass -File D:/orch/moirai/kit/selftest.ps1`: it must end with
   `selftest: PASS` and every case `ok`. A case that fails is fixed in the kit script and the self-test re-run until
   it passes; the change is recorded in `inventory.md` and reported to the owner, so that Appendix A is corrected by
   an R-HARN change. Record the kit hash (`kit-hash.ps1`) and the self-test's summary line. Then the pre-flight on
   the real volume: `powershell.exe -NoProfile -ExecutionPolicy Bypass -File D:/orch/moirai/kit/preflight.ps1 -Volume
   D:/moirai-target`, its output line in `inventory.md`. If it refuses `disk-low`: **STOP**, citing nightly.md §7
   item 3, with the headroom from the guard's output and a proposal: the directories under `D:/moirai-target` beside
   the four counted ones (old probe and scratch target directories), each with its size and last write time, that no
   live process uses and that were not written in the last two hours. Only those the owner names are deleted, and
   each deletion is logged (§4); steps 8–12 wait until the pre-flight passes. Commit `<ORCH>`.
6. **Settings and gate worktrees.** `kit/local-settings.ps1 -Worktree D:/claude/moirai -Orch D:/orch/moirai -WtRoot
   D:/moirai-wt` for the main checkout; restart the session if `<ORCH>` is still not readable without a prompt. Then
   the gate worktrees, before any new role worktree (`xtask worktree` writes its deny rules for the worktrees that
   exist): `git worktree add --detach D:/moirai-wt/gate-a HEAD` and `git worktree add --detach D:/moirai-wt/gate-b
   HEAD`. If step 1 found a neutral worktree that earlier sessions used as their gate worktree, record it and ask the
   owner before adding new ones.
7. **`RULINGS.md` and `RESUME.md`.** `RULINGS.md` holds R-1 to R-4 of §2 word for word, with the author identity of
   step 1 written out in R-2, and R-5 of §10 marked "proposed". **STOP: R-5.** Ask the owner to confirm or correct
   it; steps 8–12 wait for the answer, which is recorded in R-5 word for word. `RESUME.md` follows the skeleton of
   §3: the trunk line, no units, "Next" = steps 8–12, "Waiting on the owner" = the open STOPs. Commit `<ORCH>`.
8. **The baseline** (on `master`, after steps 2, 5 and 7). Through the kit, in `gate-a` with lane A's target
   directory:
   - `start-gate.ps1 -RunDir D:/orch/moirai/baseline/gate -WorkDir D:/moirai-wt/gate-a -Unit baseline -RunId
     baseline-gate -Kind gate -Detach master -TargetDir D:/moirai-target/laneA -BudgetMin 120 -CmdLine "cargo xtask
     gate"`, then `wait-gate.ps1` until DONE;
   - the whole workspace's tests by test ID at tier `pr`: the same with `-RunDir D:/orch/moirai/baseline/tests
     -RunId baseline-tests -Kind tests -CmdLine "cargo test --workspace --locked
     --message-format=json-render-diagnostics --no-fail-fast --exclude <crate> …"`, with one `--exclude` for each
     crate `cargo xtask host-only --list` prints and each root `xtask/roots.toml` marks present (the gate's
     exclusions, PLAN §2.1);
   - `baseline.tsv`: a first line `# baseline master=<sha> kit=<hash> <UTC time>`, then the `id` and `status`
     columns of the tests run's `summary.tsv`.
   Every `FAILED` row is re-run alone (`cargo test -p <crate> --locked -- <test> --exact`) before it is blamed; a
   gate step that fails is read in its excerpt. Each baseline red is fixed, or quarantined with
   `#[ignore = "<reason class>: <why>"]` at the site, by a unit of the author role that owns the path (authors.md §3):
   the orchestrator never edits a crate. If the baseline has reds: **STOP**, report them with the proposed units; no
   other unit starts before they are closed, and the owner may take the first of them as the trial of step 10.
9. **Capacity** (R-4, §10), from the baseline gate run: its duration (`owner.txt` `started` to DONE's `finished`),
   its peak working set (`peak.txt`), the free RAM at rest, and lane A's target directory size before and after. Set
   `git config moirai.build-jobs` to half the logical cores divided by two; write `budgets.txt` as
   `gate=` and `tests=` three times the measured durations (at least 10 minutes each); compute the capacity of §10,
   taking 1 GB for a unit session's own working set until step 10 measures one. Record all of it in `inventory.md`
   and in `RESUME.md`'s header; the kit hash is unchanged by `budgets.txt`. Commit `<ORCH>`.
10. **The trial unit**: a real typo fix plus a tiny test, through the whole workflow to the merge and the push.
    - Find a typo in a path R-HARN-I writes and may read, preferably a message or comment in `xtask/src/` so that a
      unit test can pin the corrected text (`rg` for common misspellings); if none is found, **STOP** and ask for one.
    - Role `r-harn-i`, the WP whose authors.md §2 row covers the file (`WP-02` for `xtask/`). If `m0/r-harn-i` or
      `D:/moirai-wt/r-harn-i` exists (step 1), **STOP** and ask which role the trial uses.
    - Run §5.2 steps 1–10: the spec (the fix and one unit test pinning it; the lock set is that one file; acceptance:
      the new test red-first on the parent, the full gate `PASS`), the worktree, the local settings, the arguments,
      the embedded copy, the dry run by name, `launch.md`, the unit session.
    - **STOP: the first launch.** The first workflow launch in auto mode asks once (§8). Ask the owner to attach to the
      unit session (`claude agents`, `→` on its row), approve the launch, and check that the status bar shows auto
      mode; then the owner detaches. A session that is not in auto mode: §8 "Permission mode". Every later
      `Needs input` of the trial (§5.3), such as a question before a commit, is recorded with its cause.
    - Wait (§5.3), then merge (§5.4 steps 1–5), push included, and keep the worktree for one more check:
    - **Relaunch from the journal**: in the unit session, relaunch the same embedded copy with the run id of
      `run_1.txt` and `k` = 2, nothing changed: every agent must return its saved result (`/workflows` shows no agent
      running) and `result_2.json` must equal `result_1.json`.
    - Then §5.4 step 6. Every permission prompt that stopped an agent gets its narrowest rule in `unit-allow.txt`
      (§8). Measure the unit session's working set during the run and correct the capacity of step 9 if needed.
11. **The checks** (record each with its evidence):
    - the files exist: AGENTS.md "Agent work", this file, the ten `.claude/agents/*.md`, `.claude/settings.json`, the
      main checkout's `settings.local.json` with both additional directories; in `<ORCH>`: `inventory.md`,
      `baseline.tsv`, `RESUME.md`, `RULINGS.md`, `kit/` with the self-test passed at the current hash and `caps.txt`,
      `embed/`;
    - the trial: every stage returned a structured answer (the result's fields are set, `/workflows` shows none failed);
      every promised report exists under `<ORCH>/<unit>/` (`baseline_unit.tsv`, `cut.md`, `critique.md`, `impl.md`,
      `gate_r1.md`, `gate_r1/DONE`, `expected_r1.tsv`, `test_r1.md`, `review_r1.md`, and the triage and fix files of any
      later round); no log was pasted into a brief (the prompts in the run's journal carry paths only); the merge's tree
      equals the verified tree; `master` is pushed (`git status -sb` shows it even with `origin/master`); the relaunch
      came back from the journal;
    - the one-hour subagent TTL: after the next session start, at least one subagent transcript of a unit session
      (under `%USERPROFILE%\.claude\projects\`, the role worktree's project) has
      `usage.cache_creation.ephemeral_1h_input_tokens` above 0 (`rg -l "\"ephemeral_1h_input_tokens\":[1-9]"`);
      `/usage` shows the cache statistics from Claude Code 2.1.251.
12. **Report to the owner**: what was created, the STOPs still open, the measured capacity, budgets and kit hash, and
    the first real unit proposed (`RESUME.md` "Next").

## Appendix A. The gate kit

The scripts below are the kit of §7, written into `<ORCH>/kit/` by §15 step 5 exactly as printed. They are Windows
PowerShell 5.1 scripts in ASCII; `.gitattributes`' CRLF rule for `*.ps1` concerns files in the repository, and these
live only in `<ORCH>`. Each script's header states its call, its outputs and its exit codes. The red-proof of each is
the `selftest.ps1` case that names it:

| Script | Red-proof cases in `selftest.ps1` |
|---|---|
| `start-gate.ps1` | `run-dir-reuse-refused`, `start-refused-below-disk-floor`, `lock-released-after-refusal`, `start-refused-without-target-dir`, `start-refused-cmd-metacharacter`, `tests-kind-needs-json-messages`, `busy-work-dir-refused`, `detach-dirty-refused`, `foreign-lockfile-refused`, `lockfile-equal-to-branch-staged-and-switched` |
| `run-gate.ps1` | `gate-fail-row-exit-0`, `gate-pass-rows-exit-3`, `gate-nothing-exit-0`, `tests-zero`, `tests-missing-expected`, `tests-failed-exit-101`, `tests-unannounced-binary`, `tests-ids-carry-package`, `hang-stopped-by-pid-tree`, `detach-records-head-and-tree`, `lockfile-update-reported`, `summary-stamped-with-kit-hash`; the passing controls `gate-pass`, `tests-pass`, `tests-ids-by-name`, `tests-two-packages` |
| `wait-gate.ps1` | `wait-slice-running`, `dead-runner-reported` |
| `stop-gate.ps1` | `stop-gate-archives-and-kills`, `orphan-killed-by-pid`, `lock-released-after-stop` |
| `preflight.ps1` | `preflight-fallback-disk-low`, `preflight-fallback-ram-low`, `preflight-fallback-cap-reserved`, `preflight-no-caps-refused`, `preflight-guard-disk-low`, `preflight-guard-ram-low`, `preflight-guard-cap-reserved`; the controls `preflight-fallback-pass`, `preflight-fallback-cap-control`, `preflight-guard-pass`, `preflight-guard-cap-control` |
| `embed.ps1` | `embed-no-marker-refused`, `embed-two-markers-refused`, `embed-writes-lf-copy` |
| `local-settings.ps1` | `local-settings-malformed-refused`, `local-settings-adds-and-keeps`, `local-settings-idempotent` |
| `kit-common.ps1`, `kit-hash.ps1` | every case above; `summary-stamped-with-kit-hash` |

`selftest.ps1` is red-proved once by a deliberate defect: change the last line of `selftest-fake.ps1` to `exit 0`;
the case `gate-pass-rows-exit-3` (and others) must then report `FAIL` and the run must end `selftest: FAIL` with exit
1. Restore the line (the kit hash returns to its recorded value) and run it again to `selftest: PASS`.

### A.1 `kit/kit-common.ps1`

```powershell
# kit-common.ps1 - shared helpers of the moirai gate kit (docs/m0/workflow.md section 7 and Appendix A).
# Dot-sourced by the other kit scripts; never run on its own. Windows PowerShell 5.1; ASCII only.
Set-StrictMode -Version 2.0
$ErrorActionPreference = 'Stop'
$script:Inv = [System.Globalization.CultureInfo]::InvariantCulture

function Get-UtcNow { (Get-Date).ToUniversalTime() }
function Get-UtcText { (Get-UtcNow).ToString('o', $script:Inv) }

function Fail([int]$Code, [string]$Message) {
    [Console]::Error.WriteLine($Message)
    exit $Code
}

function Test-NoSpace([string[]]$Values) {
    foreach ($v in $Values) { if ($null -ne $v -and $v -match '\s') { return $false } }
    return $true
}

function Write-Text([string]$Path, [string]$Text) {
    # UTF-8 without a BOM, LF line endings.
    $enc = New-Object System.Text.UTF8Encoding($false)
    [System.IO.File]::WriteAllText($Path, $Text.Replace("`r`n", "`n"), $enc)
}

function Publish-Once([string]$Path, [string]$Text) {
    # Written in full under a temporary name, then moved into place: a reader never sees a partial file.
    $tmp = $Path + '.tmp'
    Write-Text $tmp $Text
    [System.IO.File]::Move($tmp, $Path)
}

function Read-Lines([string]$Path) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { return @() }
    return @([System.IO.File]::ReadAllLines($Path, [System.Text.Encoding]::UTF8))
}

function Read-KeyValues([string]$Path) {
    $h = @{}
    foreach ($line in (Read-Lines $Path)) {
        $i = $line.IndexOf('=')
        if ($i -gt 0) { $h[$line.Substring(0, $i).Trim()] = $line.Substring($i + 1).Trim() }
    }
    return $h
}

function Get-KitHash {
    # SHA-256 over "name:sha256" of every kit script, sorted by name; stamped into every summary.tsv and DONE.
    $lines = @(Get-ChildItem -LiteralPath $PSScriptRoot -Filter '*.ps1' | Where-Object { -not $_.PSIsContainer } |
        Sort-Object Name | ForEach-Object {
            '{0}:{1}' -f $_.Name, (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
        })
    $sha = [System.Security.Cryptography.SHA256]::Create()
    $bytes = [System.Text.Encoding]::UTF8.GetBytes(($lines -join "`n"))
    return (($sha.ComputeHash($bytes) | ForEach-Object { $_.ToString('x2') }) -join '')
}

function Get-ProcStart([int]$Id) {
    # The start time identifies a process together with its PID, so a reused PID is never taken for it.
    $p = Get-Process -Id $Id -ErrorAction SilentlyContinue
    if ($null -eq $p) { return '' }
    try { return $p.StartTime.ToUniversalTime().ToString('o', $script:Inv) } catch { return '' }
}

function Test-SameProcess([string]$Id, [string]$Start) {
    if ([string]::IsNullOrEmpty($Id) -or [string]::IsNullOrEmpty($Start)) { return $false }
    $s = Get-ProcStart ([int]$Id)
    return ($s -ne '' -and $s -eq $Start)
}

function Get-Tree([int]$Root) {
    # The process and its descendants by parent links. A child must not be older than its parent, so a reused
    # parent PID never adopts an unrelated process.
    $all = @(Get-CimInstance -ClassName Win32_Process -Property ProcessId, ParentProcessId, WorkingSetSize, CreationDate)
    $byId = @{}
    $kids = @{}
    foreach ($p in $all) {
        $byId[[int]$p.ProcessId] = $p
        $pp = [int]$p.ParentProcessId
        if (-not $kids.ContainsKey($pp)) { $kids[$pp] = New-Object System.Collections.ArrayList }
        [void]$kids[$pp].Add($p)
    }
    $out = New-Object System.Collections.ArrayList
    if (-not $byId.ContainsKey($Root)) { return ,$out }
    $seen = @{}
    $queue = New-Object System.Collections.Queue
    $queue.Enqueue($byId[$Root])
    while ($queue.Count -gt 0) {
        $p = $queue.Dequeue()
        $id = [int]$p.ProcessId
        if ($seen.ContainsKey($id)) { continue }
        $seen[$id] = $true
        [void]$out.Add($p)
        if ($kids.ContainsKey($id)) {
            foreach ($k in $kids[$id]) {
                if ($null -eq $p.CreationDate -or $null -eq $k.CreationDate -or $k.CreationDate -ge $p.CreationDate) {
                    $queue.Enqueue($k)
                }
            }
        }
    }
    return ,$out
}

function Stop-Tree([int]$Id) {
    # By PID and its descendants (taskkill /T), never by image name: an image-name kill stops every unit's builds.
    $tk = Join-Path $env:SystemRoot 'System32\taskkill.exe'
    $old = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try { $o = (& $tk /PID $Id /T /F 2>&1 | Out-String) } finally { $ErrorActionPreference = $old }
    return $o.Trim()
}

function Invoke-Git([string]$Dir, [string[]]$GitArgs) {
    $old = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try { $out = (& git -C $Dir @GitArgs 2>&1 | Out-String) } finally { $ErrorActionPreference = $old }
    return @{ Code = $LASTEXITCODE; Out = $out.TrimEnd() }
}

function Get-StatusPaths([string]$Dir) {
    # "<XY> <path>" lines of git status --porcelain; Code is git's exit code.
    $st = Invoke-Git $Dir @('status', '--porcelain')
    $lines = @($st.Out -split "`n" | ForEach-Object { $_.TrimEnd() } | Where-Object { $_ -ne '' })
    return @{ Code = $st.Code; Lines = $lines; Paths = @($lines | ForEach-Object { $_.Substring(3).Trim() }) }
}

function Test-OnlyLockfiles($Status) {
    if ($Status.Lines.Count -eq 0) { return $false }
    foreach ($l in $Status.Lines) {
        $p = $l.Substring(3).Trim()
        if ($l.StartsWith('??') -or ($p -ne 'Cargo.lock' -and $p -ne 'fuzz/Cargo.lock')) { return $false }
    }
    return $true
}

function ConvertTo-Field([string]$Text) { return $Text.Replace("`t", ' ').Replace("`r", ' ').Replace("`n", ' ') }

function Get-GateRows([string[]]$Lines) {
    # cargo xtask gate prints "-- <step>: PASS|FAIL (<n> findings, <s> s)" per step and "xtask gate: PASS|FAIL" last.
    $rows = New-Object System.Collections.ArrayList
    $final = ''
    foreach ($l in $Lines) {
        $m = [regex]::Match($l, '^-- (\S+): (PASS|FAIL) \((\d+) findings, ([0-9.]+) s\)\s*$')
        if ($m.Success) {
            [void]$rows.Add([pscustomobject]@{
                Id = $m.Groups[1].Value; Status = $m.Groups[2].Value
                Detail = ('{0} findings, {1} s' -f $m.Groups[3].Value, $m.Groups[4].Value)
            })
            continue
        }
        $f = [regex]::Match($l, '^xtask gate: (PASS|FAIL)\s*$')
        if ($f.Success) { $final = $f.Groups[1].Value }
    }
    return @{ Rows = $rows; Final = $final }
}

function Get-JsonField($Object, [string]$Name) {
    # A property of a parsed JSON object, or $null when it is absent (StrictMode forbids reading a missing one).
    if ($null -eq $Object -or $Object -isnot [System.Management.Automation.PSCustomObject]) { return $null }
    $prop = $Object.PSObject.Properties[$Name]
    if ($null -eq $prop) { return $null }
    return $prop.Value
}

function Get-FileLeaf([string]$Path) { return $Path.Substring([Math]::Max($Path.LastIndexOf('/'), $Path.LastIndexOf('\')) + 1) }

function Get-PackageName([string]$Id) {
    # cargo's package id: "<source url>#[<name>@]<version>" (cargo 1.77 and later) or "<name> <version> (<source>)".
    $h = $Id.LastIndexOf('#')
    if ($h -lt 0) { return @($Id.Trim() -split '\s+')[0] }
    $frag = $Id.Substring($h + 1)
    $at = $frag.IndexOf('@')
    if ($at -gt 0) { return $frag.Substring(0, $at) }
    $url = $Id.Substring(0, $h)
    $q = $url.IndexOf('?')
    if ($q -ge 0) { $url = $url.Substring(0, $q) }
    return (Get-FileLeaf $url.TrimEnd('/'))
}

function Get-ArtifactMaps([string[]]$Lines) {
    # cargo's compiler-artifact messages (cargo test --message-format=json-render-diagnostics): each executable's file
    # name (lower case) -> its package, and each library target's name -> its package, for "Doc-tests <lib>".
    $exe = @{}
    $lib = @{}
    foreach ($l in $Lines) {
        if (-not $l.StartsWith('{"reason":"compiler-artifact"')) { continue }
        try { $j = $l | ConvertFrom-Json } catch { continue }
        $id = [string](Get-JsonField $j 'package_id')
        if ($id -eq '') { continue }
        $pkg = Get-PackageName $id
        $x = Get-JsonField $j 'executable'
        if ($null -ne $x -and [string]$x -ne '') { $exe[(Get-FileLeaf ([string]$x)).ToLowerInvariant()] = $pkg }
        $t = Get-JsonField $j 'target'
        $kinds = @(Get-JsonField $t 'kind' | Where-Object { $null -ne $_ } | ForEach-Object { [string]$_ })
        $isLib = @($kinds | Where-Object { @('lib', 'rlib', 'dylib', 'cdylib', 'staticlib', 'proc-macro') -contains $_ }).Count -gt 0
        $name = [string](Get-JsonField $t 'name')
        if ($isLib -and $name -ne '') {
            $k = $name.Replace('-', '_')
            if (-not $lib.ContainsKey($k) -or $id.Contains('path+file')) { $lib[$k] = $pkg }
        }
    }
    return @{ Exe = $exe; Lib = $lib }
}

function Get-TestRows([string[]]$Lines) {
    # One row per "test <name> ... ok|FAILED|ignored" line, named "<package>|<source>|<test>". The source and the
    # executable come from cargo's "Running <source> (<executable>)" line before it (stdout and stderr in one log, in
    # order), the package from the compiler-artifact message that announced that executable, so two packages with a
    # tests/props.rs are told apart and an id does not depend on the -p set. "Doc-tests <lib>" gives
    # "<package>|doc-tests|<test>". A test binary that no compiler-artifact message announced gets the package
    # "?<executable stem>" (run-gate fails such a run). A repeated id gets "#<n>" in output order.
    $maps = Get-ArtifactMaps $Lines
    $rows = New-Object System.Collections.ArrayList
    $count = @{}
    $target = '?|?'
    foreach ($l in $Lines) {
        if ($l.StartsWith('{')) { continue }
        $r = [regex]::Match($l, '^\s*Running (.+?) \((.+)\)\s*$')
        if ($r.Success) {
            $leaf = Get-FileLeaf $r.Groups[2].Value
            $stem = [regex]::Replace([regex]::Replace($leaf, '\.exe$', ''), '-[0-9a-f]{16}$', '')
            $pkg = '?' + $stem
            if ($maps.Exe.ContainsKey($leaf.ToLowerInvariant())) { $pkg = $maps.Exe[$leaf.ToLowerInvariant()] }
            $target = '{0}|{1}' -f $pkg, $r.Groups[1].Value.Replace('\', '/')
            continue
        }
        $d = [regex]::Match($l, '^\s*Doc-tests (\S+)\s*$')
        if ($d.Success) {
            $c = $d.Groups[1].Value
            $pkg = $c.Replace('_', '-')
            if ($maps.Lib.ContainsKey($c)) { $pkg = $maps.Lib[$c] }
            $target = '{0}|doc-tests' -f $pkg
            continue
        }
        $t = [regex]::Match($l, '^test (.+?) \.\.\. (ok|FAILED|ignored)\b(.*)$')
        if ($t.Success) {
            $id = '{0}|{1}' -f $target, $t.Groups[1].Value
            if ($count.ContainsKey($id)) { $count[$id] = $count[$id] + 1; $id = '{0}#{1}' -f $id, $count[$id] }
            else { $count[$id] = 1 }
            [void]$rows.Add([pscustomobject]@{
                Id = $id; Status = $t.Groups[2].Value; Detail = $t.Groups[3].Value.Trim().TrimStart(',').Trim()
            })
        }
    }
    return ,$rows
}

function Get-Missing($Rows, [string]$ExpectedFile) {
    # Expected ids (one "<id><TAB><status>" or "<id>" per line; "#" starts a comment) absent from the run.
    $have = @{}
    foreach ($r in $Rows) { $have[$r.Id] = $true }
    $missing = New-Object System.Collections.ArrayList
    foreach ($line in (Read-Lines $ExpectedFile)) {
        if ($line.Trim() -eq '' -or $line.StartsWith('#') -or $line.StartsWith("id`t")) { continue }
        $id = $line.Split("`t")[0]
        if (-not $have.ContainsKey($id)) {
            [void]$missing.Add([pscustomobject]@{ Id = $id; Status = 'MISSING'; Detail = 'expected, not run' })
        }
    }
    return ,$missing
}

function ConvertTo-ClaudeAbs([string]$Path) {
    # D:/x/y -> //d/x/y, the absolute-path form of Claude Code permission rules (as xtask worktree writes them).
    $p = $Path.Replace('\', '/').TrimEnd('/')
    if ($p.Length -ge 2 -and $p[1] -eq ':' -and [char]::IsLetter($p[0])) {
        return '//' + [char]::ToLowerInvariant($p[0]) + $p.Substring(2)
    }
    return '/' + $p
}
```

### A.2 `kit/start-gate.ps1`

```powershell
# start-gate.ps1 - start one long gate detached, in its own minimized console, under a time budget
# (docs/m0/workflow.md section 7 and Appendix A).
#
#   powershell.exe -NoProfile -ExecutionPolicy Bypass -File <kit>/start-gate.ps1 -RunDir <new dir> -WorkDir <dir>
#     -Unit <unit> -RunId <id> -Kind gate|tests -CmdLine "<command>" [-Detach <branch>] [-BudgetMin <min>]
#     [-TargetDir <dir>] [-Jobs <n>] [-Expected <tsv>] [-DiskFloorGb <gb>] [-RamFloorGb <gb>] [-Guard <exe>]
#     [-CapsFile <file>]
#
# -Detach <branch> (the gate worktree): under the lock, the work directory must be clean - except Cargo.lock and
# fuzz/Cargo.lock when they equal the branch's, which are then staged - and is switched to the branch tip, detached
# (git switch --detach), as cargo xtask gate --branch requires; the tip is recorded as head.
# The command's words are split on white space (no word may contain a space), and the runner hands them to cmd.exe,
# so none of & | < > ^ % " may appear in them. The run directory must not exist. A tests run of cargo needs
# --message-format=json-render-diagnostics: its test ids take the package from cargo's compiler-artifact messages.
# Defaults: -BudgetMin from <kit>/budgets.txt (<kind>=<minutes>), -TargetDir and -Jobs from CARGO_TARGET_DIR and
# CARGO_BUILD_JOBS (set by the role worktree's settings.local.json), -Guard <kit>/guard.exe, -CapsFile <kit>/caps.txt
# (the counted directories of the pre-flight, preflight.ps1).
# One run per work directory at a time: the lock is <kit>/locks/<work dir>.lock.
# Writes in the run directory: params.txt and cmd.txt (before the start), PID and owner.txt (after it); the runner
# (run-gate.ps1) writes the rest. Exit codes: 0 started; 1 pre-flight refused; 2 usage or precondition;
# 6 the work directory is busy (another live run holds its lock).
param(
    [Parameter(Mandatory = $true)][string]$RunDir,
    [Parameter(Mandatory = $true)][string]$WorkDir,
    [Parameter(Mandatory = $true)][string]$Unit,
    [Parameter(Mandatory = $true)][string]$RunId,
    [Parameter(Mandatory = $true)][ValidateSet('gate', 'tests')][string]$Kind,
    [Parameter(Mandatory = $true)][string]$CmdLine,
    [string]$Detach = '',
    [double]$BudgetMin = 0,
    [string]$TargetDir = $env:CARGO_TARGET_DIR,
    [string]$Jobs = $env:CARGO_BUILD_JOBS,
    [string]$Expected = '',
    [double]$DiskFloorGb = 25,
    [double]$RamFloorGb = 1.5,
    [string]$Guard = '',
    [string]$CapsFile = ''
)
. (Join-Path $PSScriptRoot 'kit-common.ps1')

if (-not (Test-NoSpace @($RunDir, $WorkDir, $TargetDir, $Expected, $Unit, $RunId, $PSScriptRoot))) {
    Fail 2 'start-gate: paths, the unit and the run id must not contain white space'
}
if (-not (Test-Path -LiteralPath $WorkDir -PathType Container)) { Fail 2 "start-gate: no work directory $WorkDir" }
if ([string]::IsNullOrEmpty($TargetDir)) {
    Fail 2 'start-gate: CARGO_TARGET_DIR is unset: start the gate from the unit session (its role worktree sets it) or pass -TargetDir'
}
if ($Expected -ne '' -and -not (Test-Path -LiteralPath $Expected -PathType Leaf)) { Fail 2 "start-gate: no expected set $Expected" }
if (Test-Path -LiteralPath $RunDir) { Fail 2 "start-gate: $RunDir exists: every run gets a new directory" }
$argv = @($CmdLine.Trim() -split '\s+' | Where-Object { $_ -ne '' })
if ($argv.Count -lt 1) { Fail 2 'start-gate: empty -CmdLine' }
if ($CmdLine -match '[&|<>^%"]') { Fail 2 'start-gate: -CmdLine must not contain & | < > ^ % or a double quote' }
if ($Kind -eq 'tests' -and (Get-FileLeaf $argv[0]) -match '^cargo(\.exe)?$' -and -not ($argv -contains '--message-format=json-render-diagnostics')) {
    Fail 2 'start-gate: a tests run of cargo needs --message-format=json-render-diagnostics (test ids carry the package)'
}
if ($BudgetMin -le 0) {
    $b = Read-KeyValues (Join-Path $PSScriptRoot 'budgets.txt')
    if (-not $b.ContainsKey($Kind)) { Fail 2 "start-gate: no -BudgetMin and no '$Kind=' line in $PSScriptRoot/budgets.txt" }
    $BudgetMin = [double]::Parse($b[$Kind], $script:Inv)
}
if ($BudgetMin -le 0) { Fail 2 'start-gate: the budget must be positive' }

# The lock of the work directory.
$lockDir = Join-Path $PSScriptRoot 'locks'
if (-not (Test-Path -LiteralPath $lockDir)) { New-Item -ItemType Directory -Path $lockDir | Out-Null }
$lock = Join-Path $lockDir ((($WorkDir.Replace('\', '/').TrimEnd('/').ToLowerInvariant()) -replace '[^a-z0-9]+', '_') + '.lock')
if (Test-Path -LiteralPath $lock) {
    $held = Read-KeyValues $lock
    $age = ((Get-UtcNow) - (Get-Item -LiteralPath $lock).LastWriteTimeUtc).TotalSeconds
    if ($held.ContainsKey('pid') -and (Test-SameProcess $held['pid'] $held['start'])) {
        Fail 6 ("start-gate: busy: {0} is held by run {1} (PID {2}); wait on it: wait-gate.ps1 -RunDir {3}" -f $WorkDir, $held['run'], $held['pid'], $held['run_dir'])
    }
    if (-not $held.ContainsKey('pid') -and $age -lt 120) { Fail 6 "start-gate: busy: a run is starting in $WorkDir" }
    $was = (Read-Lines $lock) -join '; '
    Add-Content -LiteralPath ($lock + '.log') -Value ('{0} stale lock removed: {1}' -f (Get-UtcText), $was)
    Remove-Item -LiteralPath $lock -Force
}
try {
    $fs = [System.IO.File]::Open($lock, [System.IO.FileMode]::CreateNew, [System.IO.FileAccess]::Write, [System.IO.FileShare]::None)
    $fs.Close()
} catch { Fail 6 "start-gate: busy: another start took $WorkDir" }
Write-Text $lock ("state=starting`nrun={0}`n" -f $RunId)
function Stop-Start([int]$Code, [string]$Message) { Remove-Item -LiteralPath $lock -Force; Fail $Code $Message }

# The checkout of the gate worktree, under the lock.
$head = ''
if ($Detach -ne '') {
    $st = Get-StatusPaths $WorkDir
    if ($st.Code -ne 0) { Stop-Start 2 "start-gate: $WorkDir is not a git work directory" }
    if ($st.Lines.Count -gt 0) {
        if (-not (Test-OnlyLockfiles $st)) { Stop-Start 2 ('start-gate: {0} is not clean: {1}' -f $WorkDir, ($st.Lines -join '; ')) }
        $same = Invoke-Git $WorkDir (@('diff', '--quiet', $Detach, '--') + $st.Paths)
        if ($same.Code -ne 0) { Stop-Start 2 ("start-gate: the lockfiles in {0} differ from {1}'s: the orchestrator rules (docs/m0/workflow.md section 4)" -f $WorkDir, $Detach) }
        $add = Invoke-Git $WorkDir (@('add', '--') + $st.Paths)
        if ($add.Code -ne 0) { Stop-Start 2 ('start-gate: git add failed: {0}' -f $add.Out) }
    }
    $sw = Invoke-Git $WorkDir @('switch', '--detach', $Detach)
    if ($sw.Code -ne 0) { Stop-Start 2 ('start-gate: git switch --detach {0} failed: {1}' -f $Detach, $sw.Out) }
    $h = Invoke-Git $WorkDir @('rev-parse', 'HEAD')
    $t = Invoke-Git $WorkDir @('rev-parse', ($Detach + '^{commit}'))
    if ($h.Code -ne 0 -or $h.Out.Trim() -ne $t.Out.Trim()) { Stop-Start 2 "start-gate: HEAD of $WorkDir is not the tip of $Detach" }
    $head = $h.Out.Trim()
}

# The pre-flight: the guard of [MP 8], or the fallback check of the floors.
$volume = $TargetDir
if (-not (Test-Path -LiteralPath $volume -PathType Container)) { $volume = Split-Path -Parent $TargetDir }
$pf = Join-Path $PSScriptRoot 'preflight.ps1'
& $pf -Volume $volume -DiskFloorGb $DiskFloorGb -RamFloorGb $RamFloorGb -Guard $Guard -CapsFile $CapsFile
$pfCode = $LASTEXITCODE
if ($pfCode -eq 2) { Stop-Start 2 'start-gate: the pre-flight could not run (exit 2)' }
if ($pfCode -ne 0) { Stop-Start 1 "start-gate: pre-flight refused (exit $pfCode): nothing started" }

$hash = Get-KitHash
New-Item -ItemType Directory -Path $RunDir | Out-Null
$params = @(
    "unit=$Unit", "run_id=$RunId", "kind=$Kind", "workdir=$WorkDir", "target_dir=$TargetDir", "jobs=$Jobs",
    ('budget_min={0}' -f $BudgetMin.ToString($script:Inv)), "expected=$Expected", "lock=$lock", "kit=$hash", "head=$head"
)
Write-Text (Join-Path $RunDir 'params.txt') (($params -join "`n") + "`n")
Write-Text (Join-Path $RunDir 'cmd.txt') (($argv -join "`n") + "`n")
try {
    $runner = Join-Path $PSScriptRoot 'run-gate.ps1'
    $ps = Join-Path $PSHOME 'powershell.exe'
    $a = '-NoProfile -ExecutionPolicy Bypass -File "{0}" -RunDir "{1}"' -f $runner, $RunDir
    $p = Start-Process -FilePath $ps -ArgumentList $a -WindowStyle Minimized -PassThru
} catch { Stop-Start 2 ('start-gate: the runner did not start: {0}' -f $_.Exception.Message) }
$start = Get-ProcStart $p.Id
$started = Get-UtcText
Write-Text (Join-Path $RunDir 'PID') ("{0}`n" -f $p.Id)
$owner = @(
    "unit=$Unit", "run_id=$RunId", "kind=$Kind", ('pid={0}' -f $p.Id), "pid_start=$start", "started=$started",
    ('budget_min={0}' -f $BudgetMin.ToString($script:Inv)), "workdir=$WorkDir", "kit=$hash", "head=$head"
)
Write-Text (Join-Path $RunDir 'owner.txt') (($owner -join "`n") + "`n")
Write-Text $lock ("pid={0}`nstart={1}`nrun={2}`nrun_dir={3}`n" -f $p.Id, $start, $RunId, $RunDir)
Write-Output ('start-gate: started {0} ({1}): runner PID {2}, budget {3} min, run dir {4}, head {5}, kit {6}' -f $RunId, $Kind, $p.Id, $BudgetMin.ToString($script:Inv), $RunDir, $head, $hash)
exit 0
```

### A.3 `kit/run-gate.ps1`

```powershell
# run-gate.ps1 - the detached runner that start-gate.ps1 starts in its own minimized console (docs/m0/workflow.md
# section 7 and Appendix A). Never started by hand.
#
# Runs cmd.txt in the work directory of params.txt through cmd.exe, stdout and stderr into one log.txt in order,
# under the budget; every 5 s it records the process tree (tree.txt: pid=start) and its peak working set (peak.txt).
# Past the budget it stops the tree by PID (a hang). Then it writes exitcode.txt, summary.tsv (stamped with the kit
# hash) and, last, DONE, which for a -Detach run also carries head, tree (git rev-parse HEAD^{tree}) and lockfile
# (updated when the run left Cargo.lock or fuzz/Cargo.lock changed). Verdicts and exit codes: PASS 0; FAIL 1 (the
# command exited non-zero, a row failed, an expected test is missing, a test binary has no package, or the run
# changed a file other than a lockfile); VACUOUS 7 (nothing to count: no gate rows or zero tests); VOID 8 (the runner
# failed); HANG 124 (past the budget).
param([Parameter(Mandatory = $true)][string]$RunDir)
. (Join-Path $PSScriptRoot 'kit-common.ps1')

$Par = Read-KeyValues (Join-Path $RunDir 'params.txt')
$argv = @(Read-Lines (Join-Path $RunDir 'cmd.txt') | Where-Object { $_ -ne '' })
$log = Join-Path $RunDir 'log.txt'
$verdict = 'VOID'
$final = 8
$cmdExit = ''
$note = ''
$peak = [int64]0
$rows = New-Object System.Collections.ArrayList
$treeSeen = [ordered]@{}
$tree = ''
$lockfile = ''
$changes = ''
try {
    $env:CARGO_TARGET_DIR = $Par['target_dir']
    if ($Par['jobs'] -ne '') { $env:CARGO_BUILD_JOBS = $Par['jobs'] }
    if ($Par['kind'] -eq 'tests') { $env:MOIRAI_TEST_TIER = 'pr' }
    $app = Get-Command -Name $argv[0] -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($null -eq $app) { throw ('not found: {0}' -f $argv[0]) }
    $words = @('"' + $app.Path + '"') + @($argv | Select-Object -Skip 1)
    $line = '"{0} > "{1}" 2>&1"' -f ($words -join ' '), $log.Replace('/', '\')
    $cmdExe = Join-Path $env:SystemRoot 'System32\cmd.exe'
    $c = Start-Process -FilePath $cmdExe -ArgumentList ('/d /s /c ' + $line) -WorkingDirectory $Par['workdir'] -NoNewWindow -PassThru
    $null = $c.Handle
    Write-Text (Join-Path $RunDir 'child.pid') ("pid={0}`nstart={1}`n" -f $c.Id, (Get-ProcStart $c.Id))
    $budgetMs = [int64]([double]::Parse($Par['budget_min'], $script:Inv) * 60000)
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    $hang = $false
    while (-not $c.WaitForExit(5000)) {
        $ws = [int64]0
        foreach ($p in (Get-Tree $c.Id)) {
            $ws += [int64]$p.WorkingSetSize
            $id = [string]$p.ProcessId
            if (-not $treeSeen.Contains($id)) { $treeSeen[$id] = Get-ProcStart ([int]$p.ProcessId) }
        }
        Write-Text (Join-Path $RunDir 'tree.txt') ((@($treeSeen.Keys | ForEach-Object { '{0}={1}' -f $_, $treeSeen[$_] }) -join "`n") + "`n")
        if ($ws -gt $peak) { $peak = $ws; Write-Text (Join-Path $RunDir 'peak.txt') ("{0}`n" -f $peak) }
        if ($sw.ElapsedMilliseconds -gt $budgetMs) {
            $hang = $true
            $note = 'past the budget; stopped by PID tree: ' + (Stop-Tree $c.Id)
            foreach ($k in @($treeSeen.Keys)) { if (Test-SameProcess $k $treeSeen[$k]) { $null = Stop-Tree ([int]$k) } }
            $null = $c.WaitForExit(60000)
            break
        }
    }
    if ($hang) {
        $verdict = 'HANG'
        $final = 124
    } else {
        $c.WaitForExit()
        $cmdExit = [string]$c.ExitCode
        Write-Text (Join-Path $RunDir 'exitcode.txt') ($cmdExit + "`n")
        $lines = Read-Lines $log
        $failed = 0
        $unknown = 0
        if ($Par['kind'] -eq 'gate') {
            $g = Get-GateRows $lines
            foreach ($r in $g.Rows) { [void]$rows.Add($r) }
            $failed = @($rows | Where-Object { $_.Status -ne 'PASS' }).Count
            $empty = ($rows.Count -eq 0 -or $g.Final -eq '')
            if ($g.Final -eq 'FAIL') { $failed = $failed + 1 }
        } else {
            foreach ($r in (Get-TestRows $lines)) { [void]$rows.Add($r) }
            $empty = ($rows.Count -eq 0)
            if ($Par['expected'] -ne '') { foreach ($r in (Get-Missing $rows $Par['expected'])) { [void]$rows.Add($r) } }
            $failed = @($rows | Where-Object { $_.Status -eq 'FAILED' -or $_.Status -eq 'MISSING' }).Count
            $unknown = @($rows | Where-Object { $_.Id.StartsWith('?') }).Count
        }
        $other = 0
        if ($Par['head'] -ne '') {
            $tr = Invoke-Git $Par['workdir'] @('rev-parse', 'HEAD^{tree}')
            if ($tr.Code -eq 0) { $tree = $tr.Out.Trim() }
            $st = Get-StatusPaths $Par['workdir']
            $changes = ($st.Paths -join ' ')
            $lockfile = 'current'
            foreach ($p in $st.Paths) {
                if ($p -eq 'Cargo.lock' -or $p -eq 'fuzz/Cargo.lock') { $lockfile = 'updated' } else { $other++ }
            }
        }
        if ($cmdExit -ne '0') { $verdict = 'FAIL'; $final = 1; $note = "the command exited $cmdExit" }
        elseif ($other -gt 0) { $verdict = 'FAIL'; $final = 1; $note = "the run changed files other than the lockfiles: $changes" }
        elseif ($empty) { $verdict = 'VACUOUS'; $final = 7; $note = 'nothing to count: a vacuous green is red' }
        elseif ($unknown -gt 0) { $verdict = 'FAIL'; $final = 1; $note = "$unknown test results of binaries no compiler-artifact message announced: run cargo test with --message-format=json-render-diagnostics" }
        elseif ($failed -gt 0) { $verdict = 'FAIL'; $final = 1; $note = "$failed failing or missing rows" }
        else { $verdict = 'PASS'; $final = 0 }
    }
} catch {
    $verdict = 'VOID'
    $final = 8
    $note = 'runner error: ' + $_.Exception.Message
} finally {
    $hash = Get-KitHash
    $finished = Get-UtcText
    $head = '# kit={0} run={1} unit={2} kind={3} verdict={4} exit={5} command_exit={6} peak_ws_bytes={7} finished={8}' -f `
        $hash, $Par['run_id'], $Par['unit'], $Par['kind'], $verdict, $final, $cmdExit, $peak, $finished
    $body = @($rows | ForEach-Object { '{0}{1}{2}{1}{3}' -f (ConvertTo-Field $_.Id), "`t", $_.Status, (ConvertTo-Field $_.Detail) })
    Write-Text (Join-Path $RunDir 'summary.tsv') (((@($head, "id`tstatus`tdetail") + $body) -join "`n") + "`n")
    $done = @("verdict=$verdict", "exit=$final", "command_exit=$cmdExit", "kit=$hash", ('head={0}' -f $Par['head']), "tree=$tree",
        "lockfile=$lockfile", ('changes={0}' -f (ConvertTo-Field $changes)), "finished=$finished", ('note={0}' -f (ConvertTo-Field $note)))
    Publish-Once (Join-Path $RunDir 'DONE') (($done -join "`n") + "`n")
    $lock = $Par['lock']
    if ($lock -ne '' -and (Test-Path -LiteralPath $lock)) {
        $held = Read-KeyValues $lock
        if ($held.ContainsKey('pid') -and $held['pid'] -eq [string]$PID) { Remove-Item -LiteralPath $lock -Force }
    }
}
exit $final
```

### A.4 `kit/wait-gate.ps1`

```powershell
# wait-gate.ps1 - one bounded wait on a kit run, or on a file (docs/m0/workflow.md section 7 and Appendix A).
#
#   powershell.exe -NoProfile -ExecutionPolicy Bypass -File <kit>/wait-gate.ps1 -RunDir <run dir> [-SliceSec 540]
#   powershell.exe -NoProfile -ExecutionPolicy Bypass -File <kit>/wait-gate.ps1 -File <path> [-SliceSec 540]
#
# Waits at most -SliceSec seconds (keep it below the shell tool's timeout), polling every 5 s, and prints one line.
# Exit codes: 0 DONE (the DONE lines follow; the run's own verdict is in them) or the file exists; 3 still running
# within the budget (call again); 4 past the budget and the grace without DONE: a hang, stop it with stop-gate.ps1;
# 5 the runner is gone without DONE: the run is void, archive it with stop-gate.ps1; 2 usage.
param([string]$RunDir = '', [string]$File = '', [int]$SliceSec = 540, [double]$GraceMin = 5)
. (Join-Path $PSScriptRoot 'kit-common.ps1')

if (($RunDir -eq '') -eq ($File -eq '')) { Fail 2 'wait-gate: give exactly one of -RunDir and -File' }
if ($SliceSec -lt 1) { Fail 2 'wait-gate: -SliceSec must be at least 1' }
$sw = [System.Diagnostics.Stopwatch]::StartNew()
if ($File -ne '') {
    while ($true) {
        if (Test-Path -LiteralPath $File) { Write-Output "wait-gate: present: $File"; exit 0 }
        if ($sw.Elapsed.TotalSeconds -ge $SliceSec) { Write-Output "wait-gate: not yet: $File"; exit 3 }
        Start-Sleep -Seconds 5
    }
}
$done = Join-Path $RunDir 'DONE'
$own = Read-KeyValues (Join-Path $RunDir 'owner.txt')
if (-not $own.ContainsKey('pid')) { Fail 2 "wait-gate: $RunDir has no owner.txt: not a kit run" }
$started = [datetime]::Parse($own['started'], $script:Inv, [System.Globalization.DateTimeStyles]::RoundtripKind)
$budget = [double]::Parse($own['budget_min'], $script:Inv)
while ($true) {
    if (Test-Path -LiteralPath $done) {
        Write-Output ('wait-gate: DONE {0}' -f $own['run_id'])
        Read-Lines $done | ForEach-Object { Write-Output $_ }
        exit 0
    }
    $elapsed = ((Get-UtcNow) - $started.ToUniversalTime()).TotalMinutes
    if (-not (Test-SameProcess $own['pid'] $own['pid_start'])) {
        Start-Sleep -Seconds 2
        if (Test-Path -LiteralPath $done) { continue }
        Write-Output ('wait-gate: DEAD {0}: the runner (PID {1}) is gone without DONE after {2:N1} min: the run is void' -f $own['run_id'], $own['pid'], $elapsed)
        exit 5
    }
    if ($elapsed -gt $budget + $GraceMin) {
        Write-Output ('wait-gate: HANG {0}: {1:N1} min, budget {2} min: stop it with stop-gate.ps1' -f $own['run_id'], $elapsed, $budget)
        exit 4
    }
    if ($sw.Elapsed.TotalSeconds -ge $SliceSec) {
        $peak = (Read-Lines (Join-Path $RunDir 'peak.txt')) -join ''
        Write-Output ('wait-gate: RUNNING {0}: {1:N1} of {2} min, peak working set {3} bytes' -f $own['run_id'], $elapsed, $budget, $peak)
        exit 3
    }
    Start-Sleep -Seconds 5
}
```

### A.5 `kit/stop-gate.ps1`

```powershell
# stop-gate.ps1 - stop a kit run by its own PID tree and archive its outputs as void (docs/m0/workflow.md sections
# 7 and 13, Appendix A).
#
#   powershell.exe -NoProfile -ExecutionPolicy Bypass -File <kit>/stop-gate.ps1 -RunDir <run dir> -Reason "<why>"
#
# Stops the runner, its child and every process the runner recorded in tree.txt, each only while its PID still has
# the recorded start time (a reused PID is never touched), each with its descendants (taskkill /PID /T), never by
# image name. Releases the run's lock, writes STOPPED and renames the run directory to <run dir>.stopped-<UTC time>.
# Exit codes: 0 nothing of the run is alive any more; 1 a recorded process survived; 2 usage.
param([Parameter(Mandatory = $true)][string]$RunDir, [Parameter(Mandatory = $true)][string]$Reason)
. (Join-Path $PSScriptRoot 'kit-common.ps1')

if (-not (Test-Path -LiteralPath $RunDir -PathType Container)) { Fail 2 "stop-gate: no run directory $RunDir" }
$own = Read-KeyValues (Join-Path $RunDir 'owner.txt')
$P = Read-KeyValues (Join-Path $RunDir 'params.txt')
$targets = New-Object System.Collections.ArrayList
if ($own.ContainsKey('pid')) { [void]$targets.Add(@($own['pid'], $own['pid_start'])) }
$child = Read-KeyValues (Join-Path $RunDir 'child.pid')
if ($child.ContainsKey('pid')) { [void]$targets.Add(@($child['pid'], $child['start'])) }
foreach ($line in (Read-Lines (Join-Path $RunDir 'tree.txt'))) {
    $i = $line.IndexOf('=')
    if ($i -gt 0) { [void]$targets.Add(@($line.Substring(0, $i), $line.Substring($i + 1))) }
}
$killed = New-Object System.Collections.ArrayList
foreach ($t in $targets) {
    if (Test-SameProcess $t[0] $t[1]) { [void]$killed.Add(('{0}: {1}' -f $t[0], (Stop-Tree ([int]$t[0])))) }
}
Start-Sleep -Seconds 2
$alive = @($targets | Where-Object { Test-SameProcess $_[0] $_[1] } | ForEach-Object { $_[0] })
if ($P.ContainsKey('lock') -and $P['lock'] -ne '' -and (Test-Path -LiteralPath $P['lock'])) {
    $held = Read-KeyValues $P['lock']
    if ($own.ContainsKey('pid') -and $held.ContainsKey('pid') -and $held['pid'] -eq $own['pid']) { Remove-Item -LiteralPath $P['lock'] -Force }
}
$stamp = (Get-UtcNow).ToString('yyyyMMddTHHmmssZ', $script:Inv)
$text = @("reason=$(ConvertTo-Field $Reason)", "stopped=$(Get-UtcText)", ('killed={0}' -f (ConvertTo-Field ($killed -join ' | '))), ('alive={0}' -f ($alive -join ' ')))
Write-Text (Join-Path $RunDir 'STOPPED') (($text -join "`n") + "`n")
$leaf = Split-Path -Leaf $RunDir
$newName = '{0}.stopped-{1}' -f $leaf, $stamp
$renamed = $false
for ($i = 0; $i -lt 15 -and -not $renamed; $i++) {
    try { Rename-Item -LiteralPath $RunDir -NewName $newName; $renamed = $true } catch { Start-Sleep -Seconds 2 }
}
if (-not $renamed) { [Console]::Error.WriteLine("stop-gate: could not rename $RunDir (a file is still open); STOPPED is written") }
Write-Output ('stop-gate: {0}: killed {1} tree(s); alive: {2}; archived as {3}' -f $leaf, $killed.Count, $(if ($alive.Count) { $alive -join ' ' } else { 'none' }), $(if ($renamed) { $newName } else { '(not renamed)' }))
if ($alive.Count -gt 0) { exit 1 }
exit 0
```

### A.6 `kit/preflight.ps1`

```powershell
# preflight.ps1 - refuse a big build below the RAM or disk floor (docs/m0/workflow.md sections 7 and 10, Appendix A).
#
#   powershell.exe -NoProfile -ExecutionPolicy Bypass -File <kit>/preflight.ps1 -Volume <dir on the work volume>
#     [-DiskFloorGb 25] [-RamFloorGb 1.5] [-Guard <exe>] [-CapsFile <file>] [-InjectFreeGb <gb>] [-InjectRamGb <gb>]
#
# The disk floor is on the headroom of [MP 8.1]: the volume's available space less each counted directory's remaining
# growth up to its cap, as the nightly runner counts it. The counted directories and their caps are the lines
# "<absolute dir>=<cap>" of -CapsFile (default <kit>/caps.txt, copied from xtask/nightly.toml [guard.caps]; "#"
# comments; caps in the guard's units, such as 40GB); a missing file or one with no directory is refused (exit 2).
# With the guard binary (default <kit>/guard.exe, a copy of moirai-probes-bin guard, [MP 8]) it asks the guard:
# guard --volume <dir> --dir <dir> <cap>... --ram-floor <gb>GB --disk-floor <gb>GB, the injections as
# --inject-volume-available and --inject-available-physical. Without it, it reads the volume's available space, the
# directories' sizes (regular files, reparse points not followed) and the available physical memory itself and applies
# the same rule. GB is 10^9 bytes ([MP 1.3]). The -Inject options exist to red-prove this script and are never passed
# by a real run. Exit codes: 0 pass; 1 refused; 2 usage or unreadable.
param(
    [Parameter(Mandatory = $true)][string]$Volume,
    [double]$DiskFloorGb = 25,
    [double]$RamFloorGb = 1.5,
    [string]$Guard = '',
    [string]$CapsFile = '',
    [double]$InjectFreeGb = -1,
    [double]$InjectRamGb = -1
)
. (Join-Path $PSScriptRoot 'kit-common.ps1')

if (-not (Test-Path -LiteralPath $Volume -PathType Container)) { Fail 2 "preflight: no directory $Volume" }
if ($Guard -eq '') { $Guard = Join-Path $PSScriptRoot 'guard.exe' }
if ($CapsFile -eq '') { $CapsFile = Join-Path $PSScriptRoot 'caps.txt' }
function Format-Gb([double]$Gb) { return [string]::Format($script:Inv, '{0}GB', $Gb) }
function ConvertFrom-Quantity([string]$Text) {
    # A byte quantity of [MP 8.2]: a decimal number and an optional unit.
    $m = [regex]::Match($Text.Trim(), '^([0-9]+(?:\.[0-9]+)?)(B|KB|MB|GB|TB|KiB|MiB|GiB|TiB)?$')
    if (-not $m.Success) { return -1 }
    $u = @{ '' = 1; 'B' = 1; 'KB' = 1e3; 'MB' = 1e6; 'GB' = 1e9; 'TB' = 1e12; 'KiB' = 1024; 'MiB' = 1048576; 'GiB' = 1073741824; 'TiB' = 1099511627776 }
    return [double]::Parse($m.Groups[1].Value, $script:Inv) * [double]$u[$m.Groups[2].Value]
}
function Get-DirBytes([string]$Path) {
    # The logical lengths of the regular files below Path; a missing directory is 0, a vanished entry counts 0.
    if (-not (Test-Path -LiteralPath $Path -PathType Container)) { return [double]0 }
    $sum = [double]0
    $stack = New-Object System.Collections.Stack
    $stack.Push((New-Object System.IO.DirectoryInfo($Path)))
    while ($stack.Count -gt 0) {
        $d = $stack.Pop()
        try { $entries = @($d.EnumerateFileSystemInfos()) } catch [System.IO.DirectoryNotFoundException] { continue }
        foreach ($e in $entries) {
            if (($e.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) { continue }
            if ($e -is [System.IO.DirectoryInfo]) { $stack.Push($e); continue }
            try { $sum += [double]$e.Length } catch [System.IO.FileNotFoundException] { }
        }
    }
    return $sum
}

if (-not (Test-Path -LiteralPath $CapsFile -PathType Leaf)) { Fail 2 "preflight: no caps file $CapsFile (docs/m0/workflow.md section 15 step 5)" }
$caps = New-Object System.Collections.ArrayList
foreach ($line in (Read-Lines $CapsFile)) {
    $t = $line.Trim()
    if ($t -eq '' -or $t.StartsWith('#')) { continue }
    $i = $t.LastIndexOf('=')
    if ($i -le 0) { Fail 2 "preflight: not '<dir>=<cap>' in ${CapsFile}: $t" }
    $bytes = ConvertFrom-Quantity $t.Substring($i + 1)
    if ($bytes -lt 0) { Fail 2 "preflight: not a byte quantity in ${CapsFile}: $t" }
    [void]$caps.Add([pscustomobject]@{ Dir = $t.Substring(0, $i).Trim(); Cap = $t.Substring($i + 1).Trim(); Bytes = $bytes })
}
if ($caps.Count -eq 0) { Fail 2 "preflight: $CapsFile names no counted directory" }

if (Test-Path -LiteralPath $Guard -PathType Leaf) {
    $a = @('--volume', $Volume)
    foreach ($c in $caps) { $a += @('--dir', $c.Dir, $c.Cap) }
    $a += @('--ram-floor', (Format-Gb $RamFloorGb), '--disk-floor', (Format-Gb $DiskFloorGb))
    if ($InjectFreeGb -ge 0) { $a += @('--inject-volume-available', (Format-Gb $InjectFreeGb)) }
    if ($InjectRamGb -ge 0) { $a += @('--inject-available-physical', (Format-Gb $InjectRamGb)) }
    $old = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try { $out = (& $Guard @a 2>&1 | Out-String) } finally { $ErrorActionPreference = $old }
    $code = $LASTEXITCODE
    Write-Output ('preflight: source=guard exit={0} dirs={1} {2}' -f $code, $caps.Count, (ConvertTo-Field $out.Trim()))
    if ($code -eq 0) { exit 0 }
    if ($code -eq 2) { exit 2 }
    exit 1
}

try {
    if ($InjectFreeGb -ge 0) { $free = [double]$InjectFreeGb * 1e9 }
    else {
        $root = [System.IO.Path]::GetPathRoot((Resolve-Path -LiteralPath $Volume).ProviderPath)
        $free = [double](New-Object System.IO.DriveInfo($root)).AvailableFreeSpace
    }
    if ($InjectRamGb -ge 0) { $ram = [double]$InjectRamGb * 1e9 }
    else { $ram = [double](Get-CimInstance -ClassName Win32_OperatingSystem).FreePhysicalMemory * 1024 }
    $reserved = [double]0
    foreach ($c in $caps) { $reserved += [Math]::Max([double]0, $c.Bytes - (Get-DirBytes $c.Dir)) }
} catch { Fail 2 ('preflight: a reading failed: {0}' -f $_.Exception.Message) }
$headroom = [Math]::Max([double]0, $free - $reserved)
$refuse = New-Object System.Collections.ArrayList
if ($ram -lt $RamFloorGb * 1e9) { [void]$refuse.Add('ram-low') }
if ($headroom -lt $DiskFloorGb * 1e9) { [void]$refuse.Add('disk-low') }
$verdict = 'pass'
if ($refuse.Count -gt 0) { $verdict = 'refuse ' + ($refuse -join ',') }
Write-Output ([string]::Format($script:Inv, 'preflight: source=fallback verdict={0} ram={1:F2}GB floor={2}GB available={3:F2}GB reserved={4:F2}GB headroom={5:F2}GB floor={6}GB dirs={7} volume={8}', $verdict, ($ram / 1e9), $RamFloorGb, ($free / 1e9), ($reserved / 1e9), ($headroom / 1e9), $DiskFloorGb, $caps.Count, $Volume))
if ($refuse.Count -gt 0) { exit 1 }
exit 0
```

### A.7 `kit/embed.ps1`

```powershell
# embed.ps1 - make the per-launch copy of a workflow script with its arguments embedded (docs/m0/workflow.md
# section 6, Appendix A).
#
#   powershell.exe -NoProfile -ExecutionPolicy Bypass -File <kit>/embed.ps1 -Script <repo>/.claude/workflows/unit.js
#     -ArgsFile <ORCH>/embed/args_<unit>.json -Out <ORCH>/embed/<unit>.js
#
# Replaces the one line that starts "const A = args || {}" with "const A = <the JSON object>", keeps every other
# line, and writes the copy as UTF-8 without a BOM with LF line endings (the workflow runtime rejects CR as a hidden
# control character). Exit codes: 0 written; 2 usage, the args file is not one JSON object, or the marker line is
# missing or repeated.
param(
    [Parameter(Mandatory = $true)][string]$Script,
    [Parameter(Mandatory = $true)][string]$ArgsFile,
    [Parameter(Mandatory = $true)][string]$Out
)
. (Join-Path $PSScriptRoot 'kit-common.ps1')

if (-not (Test-Path -LiteralPath $Script -PathType Leaf)) { Fail 2 "embed: no script $Script" }
if (-not (Test-Path -LiteralPath $ArgsFile -PathType Leaf)) { Fail 2 "embed: no args file $ArgsFile" }
$src = [System.IO.File]::ReadAllText($Script, [System.Text.Encoding]::UTF8).Replace("`r`n", "`n")
$json = [System.IO.File]::ReadAllText($ArgsFile, [System.Text.Encoding]::UTF8).Replace("`r`n", "`n").Trim()
if (-not $json.StartsWith('{') -or -not $json.EndsWith('}')) { Fail 2 "embed: $ArgsFile must hold one JSON object" }
try { $null = $json | ConvertFrom-Json } catch { Fail 2 "embed: $ArgsFile is not JSON: $($_.Exception.Message)" }
$rx = New-Object System.Text.RegularExpressions.Regex('^const A = args \|\| \{\}.*$', [System.Text.RegularExpressions.RegexOptions]::Multiline)
$found = $rx.Matches($src)
if ($found.Count -ne 1) { Fail 2 ("embed: expected exactly one line starting 'const A = args || {{}}' in {0}, found {1}" -f $Script, $found.Count) }
$m = $found[0]
$line = 'const A = ' + $json + ' // EMBEDDED by kit/embed.ps1 from ' + $ArgsFile.Replace('\', '/')
$text = $src.Substring(0, $m.Index) + $line + $src.Substring($m.Index + $m.Length)
if ($text.Contains("`r")) { Fail 2 'embed: the copy would contain CR' }
if (([regex]::Matches($text, '(?m)^const A = \{')).Count -ne 1) { Fail 2 'embed: the copy does not hold exactly one embedded argument object' }
Write-Text $Out $text
Write-Output ('embed: wrote {0} ({1} bytes) from {2} and {3}' -f $Out, ([System.Text.Encoding]::UTF8.GetByteCount($text)), $Script, $ArgsFile)
exit 0
```

### A.8 `kit/local-settings.ps1`

```powershell
# local-settings.ps1 - add the workflow's local keys to a checkout's .claude/settings.local.json, keeping every key
# and rule already there (docs/m0/workflow.md sections 5 and 8, Appendix A).
#
#   powershell.exe -NoProfile -ExecutionPolicy Bypass -File <kit>/local-settings.ps1 -Worktree <checkout>
#     -Orch <ORCH> -WtRoot <worktree root> [-AllowFile <kit>/unit-allow.txt]
#
# Adds: permissions.additionalDirectories <ORCH> and <worktree root>; the allow rules of -AllowFile (one per line,
# "#" comments); and, for every Read deny rule that `cargo xtask worktree` wrote for this checkout's own path, the
# same pattern under <worktree root>/** and <ORCH>/**, so the role's deny_read also covers the gate worktrees and
# every scratch worktree made later (PLAN 3.1 "Mechanics"); for such a role also Read denies of <ORCH>/baseline.tsv
# and <ORCH>/baseline/**, whose test names span every crate (a unit reads its own baseline_unit.tsv). Creates the
# file when it is missing (the main checkout).
# Idempotent. Exit codes: 0 written and verified; 1 the verification failed; 2 usage or the file is not JSON.
param(
    [Parameter(Mandatory = $true)][string]$Worktree,
    [Parameter(Mandatory = $true)][string]$Orch,
    [Parameter(Mandatory = $true)][string]$WtRoot,
    [string]$AllowFile = ''
)
. (Join-Path $PSScriptRoot 'kit-common.ps1')

function Get-Prop($o, [string]$n) {
    if ($null -ne $o -and $null -ne $o.PSObject.Properties[$n]) { return $o.$n }
    return $null
}
function Set-Prop($o, [string]$n, $v) {
    if ($null -ne $o.PSObject.Properties[$n]) { $o.$n = $v } else { $o | Add-Member -NotePropertyName $n -NotePropertyValue $v }
}
function Get-List($o, [string]$n) { return @(@(Get-Prop $o $n) | Where-Object { $null -ne $_ } | ForEach-Object { [string]$_ }) }
function Add-Unique([System.Collections.ArrayList]$List, [string]$Item) { if (-not ($List -contains $Item)) { [void]$List.Add($Item) } }

if (-not (Test-Path -LiteralPath $Worktree -PathType Container)) { Fail 2 "local-settings: no checkout $Worktree" }
if ($AllowFile -eq '') { $AllowFile = Join-Path $PSScriptRoot 'unit-allow.txt' }
$dir = Join-Path $Worktree '.claude'
$file = Join-Path $dir 'settings.local.json'
if (Test-Path -LiteralPath $file) {
    $raw = [System.IO.File]::ReadAllText($file, [System.Text.Encoding]::UTF8)
    try { $j = $raw | ConvertFrom-Json } catch { Fail 2 "local-settings: $file is not JSON" }
    if ($null -eq $j -or $j -isnot [System.Management.Automation.PSCustomObject]) { Fail 2 "local-settings: $file is not a JSON object" }
} else {
    if (-not (Test-Path -LiteralPath $dir)) { New-Item -ItemType Directory -Path $dir | Out-Null }
    $j = New-Object psobject
}
$perm = Get-Prop $j 'permissions'
if ($null -eq $perm) { $perm = New-Object psobject; Set-Prop $j 'permissions' $perm }
$oldDeny = @(Get-List $perm 'deny')
$oldAllow = @(Get-List $perm 'allow')
$oldDirs = @(Get-List $perm 'additionalDirectories')
$oldEnv = Get-Prop $j 'env'
$envBefore = ''
if ($null -ne $oldEnv) { $envBefore = ($oldEnv | ConvertTo-Json -Compress -Depth 5) }

$deny = New-Object System.Collections.ArrayList
foreach ($r in $oldDeny) { Add-Unique $deny $r }
$prefix = 'Read(' + (ConvertTo-ClaudeAbs $Worktree) + '/'
$added = New-Object System.Collections.ArrayList
foreach ($r in $oldDeny) {
    if ($r.StartsWith($prefix, [System.StringComparison]::OrdinalIgnoreCase) -and $r.EndsWith(')')) {
        $glob = $r.Substring($prefix.Length, $r.Length - $prefix.Length - 1)
        foreach ($root in @($WtRoot, $Orch)) {
            $rule = 'Read(' + (ConvertTo-ClaudeAbs $root) + '/**/' + $glob + ')'
            Add-Unique $deny $rule
            Add-Unique $added $rule
        }
    }
}
if ($added.Count -gt 0) {
    foreach ($rule in @(('Read(' + (ConvertTo-ClaudeAbs $Orch) + '/baseline.tsv)'), ('Read(' + (ConvertTo-ClaudeAbs $Orch) + '/baseline/**)'))) {
        Add-Unique $deny $rule
        Add-Unique $added $rule
    }
}
$allow = New-Object System.Collections.ArrayList
foreach ($r in $oldAllow) { Add-Unique $allow $r }
$wantAllow = @(Read-Lines $AllowFile | ForEach-Object { $_.Trim() } | Where-Object { $_ -ne '' -and -not $_.StartsWith('#') })
foreach ($r in $wantAllow) { Add-Unique $allow $r }
$dirs = New-Object System.Collections.ArrayList
foreach ($d in $oldDirs) { Add-Unique $dirs $d }
$wantDirs = @($Orch.Replace('\', '/').TrimEnd('/'), $WtRoot.Replace('\', '/').TrimEnd('/'))
foreach ($d in $wantDirs) { Add-Unique $dirs $d }

Set-Prop $perm 'deny' ([object[]]$deny.ToArray())
Set-Prop $perm 'allow' ([object[]]$allow.ToArray())
Set-Prop $perm 'additionalDirectories' ([object[]]$dirs.ToArray())
Write-Text $file (($j | ConvertTo-Json -Depth 10) + "`n")

# Verify by reading the file back.
try { $v = [System.IO.File]::ReadAllText($file, [System.Text.Encoding]::UTF8) | ConvertFrom-Json } catch { Fail 1 "local-settings: $file does not read back as JSON" }
$vp = Get-Prop $v 'permissions'
$vDeny = @(Get-List $vp 'deny')
$vAllow = @(Get-List $vp 'allow')
$vDirs = @(Get-List $vp 'additionalDirectories')
$problems = New-Object System.Collections.ArrayList
foreach ($r in (@($oldDeny) + @($added))) { if (-not ($vDeny -contains $r)) { [void]$problems.Add("deny lost: $r") } }
foreach ($r in (@($oldAllow) + @($wantAllow))) { if (-not ($vAllow -contains $r)) { [void]$problems.Add("allow lost: $r") } }
foreach ($d in (@($oldDirs) + @($wantDirs))) { if (-not ($vDirs -contains $d)) { [void]$problems.Add("directory lost: $d") } }
$vEnv = Get-Prop $v 'env'
$envAfter = ''
if ($null -ne $vEnv) { $envAfter = ($vEnv | ConvertTo-Json -Compress -Depth 5) }
if ($envAfter -ne $envBefore) { [void]$problems.Add('env changed') }
if ($problems.Count -gt 0) { Fail 1 ('local-settings: verification failed: ' + ($problems -join '; ')) }
Write-Output ('local-settings: {0}: {1} deny rules ({2} added under the roots), {3} allow rules, directories {4}' -f $file, $vDeny.Count, $added.Count, $vAllow.Count, ($vDirs -join ', '))
exit 0
```

### A.9 `kit/kit-hash.ps1`

```powershell
# kit-hash.ps1 - print the kit's hash (docs/m0/workflow.md section 7, Appendix A): the value every summary.tsv and
# DONE carries. A gate verdict counts only when its kit hash equals this value.
. (Join-Path $PSScriptRoot 'kit-common.ps1')
Write-Output (Get-KitHash)
exit 0
```

### A.10 `kit/selftest-fake.ps1`

```powershell
# selftest-fake.ps1 - the stand-in command of selftest.ps1 (docs/m0/workflow.md Appendix A): prints the lines of a
# file, appends a line to -Touch (to change a tracked file), sleeps, and exits with the given code. Never used by a
# real run.
param([string]$Lines = '', [int]$Code = 0, [int]$SleepSec = 0, [string]$Touch = '')
if ($Lines -ne '') { foreach ($l in [System.IO.File]::ReadAllLines($Lines)) { [Console]::Out.WriteLine($l) } }
if ($Touch -ne '') { [System.IO.File]::AppendAllText($Touch, "# touched by selftest-fake`n") }
if ($SleepSec -gt 0) { Start-Sleep -Seconds $SleepSec }
exit $Code
```

### A.11 `kit/selftest.ps1`

```powershell
# selftest.ps1 - red-prove every kit script (docs/m0/workflow.md section 7 and Appendix A).
#
#   powershell.exe -NoProfile -ExecutionPolicy Bypass -File <kit>/selftest.ps1
#
# Runs each script the way an agent does (powershell.exe -File) on synthetic inputs under
# <kit>/selftest-runs/<UTC time>/ and checks its exit code and outputs: a failing row must give a non-zero exit even
# when the command itself exits 0, zero rows or zero tests must not pass, a missing expected test must fail, test ids
# must carry the package, a hang must be stopped by its PID tree, a busy work directory must be refused, and the
# pre-flight must refuse below its floors, counting the reserve of the capped directories. Prints one line per case and "selftest: PASS" or "selftest: FAIL". Exit codes: 0 every case behaved;
# 1 a case misbehaved. Takes about three minutes. Never run beside a unit's gate in the same work directory.
. (Join-Path $PSScriptRoot 'kit-common.ps1')

$K = $PSScriptRoot
$PS = Join-Path $PSHOME 'powershell.exe'
$base = Join-Path $K ('selftest-runs\' + (Get-UtcNow).ToString('yyyyMMddTHHmmssZ', $script:Inv))
New-Item -ItemType Directory -Path $base | Out-Null
$results = New-Object System.Collections.ArrayList
$script:n = 0
$capDir = Join-Path $base 'capdir'
New-Item -ItemType Directory -Path $capDir | Out-Null
$caps = Join-Path $base 'caps.txt'
Write-Text $caps "# selftest: one empty counted directory with a negligible cap`n$capDir=1MB`n"
$caps10 = Join-Path $base 'caps-10.txt'
Write-Text $caps10 "$capDir=10GB`n"

function Invoke-Kit([string]$Name, [string[]]$KitArgs) {
    $old = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try { $out = (& $PS -NoProfile -ExecutionPolicy Bypass -File (Join-Path $K $Name) @KitArgs 2>&1 | Out-String) } finally { $ErrorActionPreference = $old }
    return @{ Code = $LASTEXITCODE; Out = $out.Trim() }
}
function Add-Result([string]$Case, [string]$Expected, [string]$Got, [bool]$Ok) {
    [void]$results.Add([pscustomobject]@{ Case = $Case; Expected = $Expected; Got = $Got; Ok = $Ok })
    $mark = 'ok  '
    if (-not $Ok) { $mark = 'FAIL' }
    [Console]::Out.WriteLine(('{0} {1}: expected {2}, got {3}' -f $mark, $Case, $Expected, $Got))
}
function New-Lines([string]$Name, [string[]]$Lines) {
    $p = Join-Path $base ($Name + '.lines')
    Write-Text $p (($Lines -join "`n") + "`n")
    return $p
}
function Get-FakeCmd([string]$LinesFile, [int]$Code, [int]$SleepSec, [string]$Touch = '') {
    $c = '{0} -NoProfile -ExecutionPolicy Bypass -File {1} -Code {2} -SleepSec {3}' -f $PS, (Join-Path $K 'selftest-fake.ps1'), $Code, $SleepSec
    if ($LinesFile -ne '') { $c = $c + ' -Lines ' + $LinesFile }
    if ($Touch -ne '') { $c = $c + ' -Touch ' + $Touch }
    return $c
}
function Start-Detached([string]$Case, [string]$Repo, [string]$Branch, [string]$Touch = '') {
    $run = Join-Path $base ('run-' + $Case)
    $a = @('-RunDir', $run, '-WorkDir', $Repo, '-Unit', 'selftest', '-RunId', ('selftest-' + $Case), '-Kind', 'gate',
        '-CmdLine', (Get-FakeCmd (New-Lines $Case $pass) 0 0 $Touch), '-Detach', $Branch, '-BudgetMin', '2',
        '-TargetDir', $base, '-DiskFloorGb', '0', '-RamFloorGb', '0', '-CapsFile', $caps)
    return @{ Run = $run; Start = (Invoke-Kit 'start-gate.ps1' $a) }
}
function Start-Fake([string]$Case, [string]$Kind, [string]$LinesFile, [int]$Code, [int]$SleepSec, [double]$Budget, [string]$Work = '', [string]$Expected = '') {
    $script:n++
    if ($Work -eq '') { $Work = Join-Path $base ('work' + $script:n); New-Item -ItemType Directory -Path $Work | Out-Null }
    $run = Join-Path $base ('run-' + $Case)
    $a = @('-RunDir', $run, '-WorkDir', $Work, '-Unit', 'selftest', '-RunId', ('selftest-' + $Case), '-Kind', $Kind,
        '-CmdLine', (Get-FakeCmd $LinesFile $Code $SleepSec), '-BudgetMin', $Budget.ToString($script:Inv),
        '-TargetDir', $base, '-DiskFloorGb', '0', '-RamFloorGb', '0', '-CapsFile', $caps)
    if ($Expected -ne '') { $a += @('-Expected', $Expected) }
    $r = Invoke-Kit 'start-gate.ps1' $a
    return @{ Run = $run; Work = $Work; Start = $r }
}
function Wait-Done([string]$Run, [int]$Slice = 150) {
    $r = Invoke-Kit 'wait-gate.ps1' @('-RunDir', $Run, '-SliceSec', [string]$Slice)
    $d = Read-KeyValues (Join-Path $Run 'DONE')
    return @{ Wait = $r; Done = $d }
}
function Test-TreeDead([string]$Run) {
    $alive = 0
    foreach ($f in @('child.pid')) {
        $c = Read-KeyValues (Join-Path $Run $f)
        if ($c.ContainsKey('pid') -and (Test-SameProcess $c['pid'] $c['start'])) { $alive++ }
    }
    foreach ($line in (Read-Lines (Join-Path $Run 'tree.txt'))) {
        $i = $line.IndexOf('=')
        if ($i -gt 0 -and (Test-SameProcess $line.Substring(0, $i) $line.Substring($i + 1))) { $alive++ }
    }
    return ($alive -eq 0)
}
function Test-Verdict([string]$Case, [string]$Kind, [string[]]$Lines, [int]$Code, [string]$WantVerdict, [int]$WantExit, [string]$Expected = '') {
    $lf = ''
    if ($Lines.Count -gt 0) { $lf = New-Lines $Case $Lines }
    $s = Start-Fake $Case $Kind $lf $Code 0 2 '' $Expected
    if ($s.Start.Code -ne 0) { Add-Result $Case 'start 0' ('start {0}: {1}' -f $s.Start.Code, $s.Start.Out); return $null }
    $w = Wait-Done $s.Run
    $got = '{0} {1} (wait {2})' -f $w.Done['verdict'], $w.Done['exit'], $w.Wait.Code
    Add-Result $Case ('{0} {1} (wait 0)' -f $WantVerdict, $WantExit) $got ($w.Wait.Code -eq 0 -and $w.Done['verdict'] -eq $WantVerdict -and $w.Done['exit'] -eq [string]$WantExit)
    return $s.Run
}

$pass = @('-- fmt: PASS (0 findings, 0.1 s)', '-- test: PASS (0 findings, 0.2 s)', 'xtask gate: PASS')
$r1 = Test-Verdict 'gate-pass' 'gate' $pass 0 'PASS' 0
Test-Verdict 'gate-fail-row-exit-0' 'gate' @('-- fmt: FAIL (1 findings, 0.1 s)', '-- test: PASS (0 findings, 0.2 s)', 'xtask gate: FAIL') 0 'FAIL' 1 | Out-Null
Test-Verdict 'gate-pass-rows-exit-3' 'gate' $pass 3 'FAIL' 1 | Out-Null
Test-Verdict 'gate-nothing-exit-0' 'gate' @() 0 'VACUOUS' 7 | Out-Null
# cargo's compiler-artifact messages (--message-format=json-render-diagnostics) name each executable's package.
function Get-Artifact([string]$Pkg, [string]$Kind, [string]$Name, [string]$Exe) {
    $x = 'null'
    if ($Exe -ne '') { $x = '"D:\\t\\debug\\deps\\' + $Exe + '"' }
    return '{"reason":"compiler-artifact","package_id":"path+file:///D:/w/crates/' + $Pkg + '#0.1.0","manifest_path":"D:\\w\\crates\\' + $Pkg + '\\Cargo.toml","target":{"kind":["' + $Kind + '"],"crate_types":["' + $Kind + '"],"name":"' + $Name + '","src_path":"D:\\w\\crates\\' + $Pkg + '\\src\\lib.rs","edition":"2021","doc":true,"doctest":true,"test":true},"profile":{"test":true},"features":[],"filenames":[],"executable":' + $x + ',"fresh":true}'
}
$tl = @((Get-Artifact 'demo-pkg' 'lib' 'demo' ''), (Get-Artifact 'demo-pkg' 'lib' 'demo' 'demo-0123456789abcdef.exe'),
    '     Running unittests src\lib.rs (D:\t\debug\deps\demo-0123456789abcdef.exe)', '', 'running 2 tests',
    'test a::b ... ok', 'test a::c ... ignored, slow: the long seed', '', 'test result: ok. 1 passed; 0 failed; 1 ignored',
    '   Doc-tests demo', '', 'running 1 test', 'test src\lib.rs - f (line 3) ... ok')
$rt = Test-Verdict 'tests-pass' 'tests' $tl 0 'PASS' 0
if ($null -ne $rt) {
    $ids = @(Read-Lines (Join-Path $rt 'summary.tsv') | Select-Object -Skip 2 | ForEach-Object { $_.Split("`t")[0] })
    $want = @('demo-pkg|unittests src/lib.rs|a::b', 'demo-pkg|unittests src/lib.rs|a::c', 'demo-pkg|doc-tests|src\lib.rs - f (line 3)')
    Add-Result 'tests-ids-by-name' ($want -join ' ; ') ($ids -join ' ; ') (@($want | Where-Object { $ids -contains $_ }).Count -eq $want.Count)
}
$tp = @((Get-Artifact 'alpha' 'test' 'props' 'props-1111111111111111.exe'), (Get-Artifact 'beta' 'test' 'props' 'props-2222222222222222.exe'),
    '     Running tests\props.rs (D:\t\debug\deps\props-1111111111111111.exe)', 'test t ... ok',
    '     Running tests\props.rs (D:\t\debug\deps\props-2222222222222222.exe)', 'test t ... ok')
$rp = Test-Verdict 'tests-two-packages' 'tests' $tp 0 'PASS' 0
if ($null -ne $rp) {
    $ids = @(Read-Lines (Join-Path $rp 'summary.tsv') | Select-Object -Skip 2 | ForEach-Object { $_.Split("`t")[0] })
    Add-Result 'tests-ids-carry-package' 'alpha|tests/props.rs|t ; beta|tests/props.rs|t' ($ids -join ' ; ') (($ids -contains 'alpha|tests/props.rs|t') -and ($ids -contains 'beta|tests/props.rs|t') -and $ids.Count -eq 2)
}
Test-Verdict 'tests-unannounced-binary' 'tests' @('     Running tests\y.rs (D:\t\debug\deps\y-0123456789abcdef.exe)', 'test t ... ok') 0 'FAIL' 1 | Out-Null
Test-Verdict 'tests-zero' 'tests' @('running 0 tests', 'test result: ok. 0 passed; 0 failed') 0 'VACUOUS' 7 | Out-Null
$exp = Join-Path $base 'expected.tsv'
Write-Text $exp "demo-pkg|unittests src/lib.rs|a::b`tok`ndemo-pkg|unittests src/lib.rs|a::gone`tok`n"
Test-Verdict 'tests-missing-expected' 'tests' $tl 0 'FAIL' 1 $exp | Out-Null
Test-Verdict 'tests-failed-exit-101' 'tests' @('     Running tests\x.rs (D:\t\debug\deps\x-0123456789abcdef.exe)', 'test t ... FAILED') 101 'FAIL' 1 | Out-Null

if ($null -ne $r1) {
    $hash = Get-KitHash
    $first = (Read-Lines (Join-Path $r1 'summary.tsv'))[0]
    Add-Result 'summary-stamped-with-kit-hash' "kit=$hash" $first ($first.Contains("kit=$hash") -and ((Get-KitHash) -eq $hash))
}

# A hang: the budget (0.2 min) passes while the command sleeps; the runner stops the tree by PID.
$h = Start-Fake 'hang' 'gate' '' 0 300 0.2
if ($h.Start.Code -eq 0) {
    $w = Wait-Done $h.Run 150
    Add-Result 'hang-stopped-by-pid-tree' 'HANG 124, tree dead' ('{0} {1}, tree dead {2}' -f $w.Done['verdict'], $w.Done['exit'], (Test-TreeDead $h.Run)) ($w.Done['verdict'] -eq 'HANG' -and $w.Done['exit'] -eq '124' -and (Test-TreeDead $h.Run))
} else { Add-Result 'hang-stopped-by-pid-tree' 'start 0' $h.Start.Out $false }

# A bounded wait on a live run, a busy work directory, then stop-gate.
$s = Start-Fake 'running' 'gate' '' 0 300 10
if ($s.Start.Code -eq 0) {
    Start-Sleep -Seconds 6
    $w = Invoke-Kit 'wait-gate.ps1' @('-RunDir', $s.Run, '-SliceSec', '6')
    Add-Result 'wait-slice-running' '3' ([string]$w.Code) ($w.Code -eq 3)
    $b = Start-Fake 'busy' 'gate' '' 0 1 1 $s.Work
    Add-Result 'busy-work-dir-refused' '6' ([string]$b.Start.Code) ($b.Start.Code -eq 6)
    $st = Invoke-Kit 'stop-gate.ps1' @('-RunDir', $s.Run, '-Reason', 'selftest')
    $arch = @(Get-ChildItem -LiteralPath $base | Where-Object { $_.Name -like 'run-running.stopped-*' })
    $dead = $false
    if ($arch.Count -eq 1) { $dead = Test-TreeDead $arch[0].FullName }
    Add-Result 'stop-gate-archives-and-kills' '0, archived, tree dead' ('{0}, archived {1}, tree dead {2}' -f $st.Code, $arch.Count, $dead) ($st.Code -eq 0 -and $arch.Count -eq 1 -and $dead)
    $again = Start-Fake 'after-stop' 'gate' (New-Lines 'after-stop' $pass) 0 0 1 $s.Work
    Add-Result 'lock-released-after-stop' 'start 0' ([string]$again.Start.Code) ($again.Start.Code -eq 0)
    if ($again.Start.Code -eq 0) { Wait-Done $again.Run | Out-Null }
} else { Add-Result 'wait-slice-running' 'start 0' $s.Start.Out $false }

# A runner that dies without DONE: wait-gate reports it, stop-gate kills the orphaned command.
$d = Start-Fake 'dead-runner' 'gate' '' 0 300 10
if ($d.Start.Code -eq 0) {
    Start-Sleep -Seconds 7
    $own = Read-KeyValues (Join-Path $d.Run 'owner.txt')
    Stop-Process -Id ([int]$own['pid']) -Force
    $w = Invoke-Kit 'wait-gate.ps1' @('-RunDir', $d.Run, '-SliceSec', '30')
    Add-Result 'dead-runner-reported' '5' ([string]$w.Code) ($w.Code -eq 5)
    $st = Invoke-Kit 'stop-gate.ps1' @('-RunDir', $d.Run, '-Reason', 'selftest orphan')
    $arch = @(Get-ChildItem -LiteralPath $base | Where-Object { $_.Name -like 'run-dead-runner.stopped-*' })
    $dead = $false
    if ($arch.Count -eq 1) { $dead = Test-TreeDead $arch[0].FullName }
    Add-Result 'orphan-killed-by-pid' '0, tree dead' ('{0}, tree dead {1}' -f $st.Code, $dead) ($st.Code -eq 0 -and $dead)
} else { Add-Result 'dead-runner-reported' 'start 0' $d.Start.Out $false }

# Preconditions of start-gate.
$x = Start-Fake 'gate-pass' 'gate' '' 0 0 1
Add-Result 'run-dir-reuse-refused' '2' ([string]$x.Start.Code) ($x.Start.Code -eq 2)
$wk = Join-Path $base 'work-pf'
New-Item -ItemType Directory -Path $wk | Out-Null
$pfa = @('-RunDir', (Join-Path $base 'run-pf'), '-WorkDir', $wk, '-Unit', 'selftest', '-RunId', 'selftest-pf', '-Kind', 'gate',
    '-CmdLine', (Get-FakeCmd '' 0 0), '-BudgetMin', '1', '-TargetDir', $base, '-DiskFloorGb', '1000000', '-RamFloorGb', '0', '-CapsFile', $caps)
$pf = Invoke-Kit 'start-gate.ps1' $pfa
Add-Result 'start-refused-below-disk-floor' '1' ([string]$pf.Code) ($pf.Code -eq 1)
$lockLeft = @(Get-ChildItem -LiteralPath (Join-Path $K 'locks') -Filter '*work_pf.lock').Count
Add-Result 'lock-released-after-refusal' '0 locks' ([string]$lockLeft) ($lockLeft -eq 0)
$savedTarget = $env:CARGO_TARGET_DIR
$env:CARGO_TARGET_DIR = $null
$nt = Invoke-Kit 'start-gate.ps1' @('-RunDir', (Join-Path $base 'run-nt'), '-WorkDir', $wk, '-Unit', 'selftest', '-RunId', 'selftest-nt',
    '-Kind', 'gate', '-CmdLine', (Get-FakeCmd '' 0 0), '-BudgetMin', '1', '-DiskFloorGb', '0', '-RamFloorGb', '0')
$env:CARGO_TARGET_DIR = $savedTarget
Add-Result 'start-refused-without-target-dir' '2' ([string]$nt.Code) ($nt.Code -eq 2)
$mc = Invoke-Kit 'start-gate.ps1' @('-RunDir', (Join-Path $base 'run-mc'), '-WorkDir', $wk, '-Unit', 'selftest', '-RunId', 'selftest-mc',
    '-Kind', 'gate', '-CmdLine', 'cargo xtask gate ^& echo', '-BudgetMin', '1', '-TargetDir', $base, '-DiskFloorGb', '0', '-RamFloorGb', '0')
Add-Result 'start-refused-cmd-metacharacter' '2' ([string]$mc.Code) ($mc.Code -eq 2)
$mf = Invoke-Kit 'start-gate.ps1' @('-RunDir', (Join-Path $base 'run-mf'), '-WorkDir', $wk, '-Unit', 'selftest', '-RunId', 'selftest-mf',
    '-Kind', 'tests', '-CmdLine', 'cargo test --locked -p demo --no-fail-fast', '-BudgetMin', '1', '-TargetDir', $base, '-DiskFloorGb', '0', '-RamFloorGb', '0', '-CapsFile', $caps)
Add-Result 'tests-kind-needs-json-messages' '2' ([string]$mf.Code) ($mf.Code -eq 2)

# The pre-flight, through the fallback (a missing guard path) and, when present, through the guard.
$none = Join-Path $base 'no-guard.exe'
$p1 = Invoke-Kit 'preflight.ps1' @('-Volume', $base, '-DiskFloorGb', '25', '-RamFloorGb', '0', '-InjectFreeGb', '1', '-Guard', $none, '-CapsFile', $caps)
Add-Result 'preflight-fallback-disk-low' '1' ([string]$p1.Code) ($p1.Code -eq 1)
$p2 = Invoke-Kit 'preflight.ps1' @('-Volume', $base, '-DiskFloorGb', '0', '-RamFloorGb', '1.5', '-InjectRamGb', '0.5', '-Guard', $none, '-CapsFile', $caps)
Add-Result 'preflight-fallback-ram-low' '1' ([string]$p2.Code) ($p2.Code -eq 1)
$p3 = Invoke-Kit 'preflight.ps1' @('-Volume', $base, '-DiskFloorGb', '0', '-RamFloorGb', '0', '-Guard', $none, '-CapsFile', $caps)
Add-Result 'preflight-fallback-pass' '0' ([string]$p3.Code) ($p3.Code -eq 0)
# 30 GB available less the 10 GB the empty capped directory may still grow is below the 25 GB floor; 40 GB is not.
$p7 = Invoke-Kit 'preflight.ps1' @('-Volume', $base, '-DiskFloorGb', '25', '-RamFloorGb', '0', '-InjectFreeGb', '30', '-Guard', $none, '-CapsFile', $caps10)
Add-Result 'preflight-fallback-cap-reserved' '1' ([string]$p7.Code) ($p7.Code -eq 1)
$p8 = Invoke-Kit 'preflight.ps1' @('-Volume', $base, '-DiskFloorGb', '25', '-RamFloorGb', '0', '-InjectFreeGb', '40', '-Guard', $none, '-CapsFile', $caps10)
Add-Result 'preflight-fallback-cap-control' '0' ([string]$p8.Code) ($p8.Code -eq 0)
$p9 = Invoke-Kit 'preflight.ps1' @('-Volume', $base, '-DiskFloorGb', '0', '-RamFloorGb', '0', '-Guard', $none, '-CapsFile', (Join-Path $base 'no-caps.txt'))
Add-Result 'preflight-no-caps-refused' '2' ([string]$p9.Code) ($p9.Code -eq 2)
$g = Join-Path $K 'guard.exe'
if (Test-Path -LiteralPath $g) {
    $p4 = Invoke-Kit 'preflight.ps1' @('-Volume', $base, '-DiskFloorGb', '25', '-InjectFreeGb', '1', '-CapsFile', $caps)
    Add-Result 'preflight-guard-disk-low' '1' ([string]$p4.Code) ($p4.Code -eq 1)
    $p5 = Invoke-Kit 'preflight.ps1' @('-Volume', $base, '-InjectRamGb', '1', '-InjectFreeGb', '200', '-CapsFile', $caps)
    Add-Result 'preflight-guard-ram-low' '1' ([string]$p5.Code) ($p5.Code -eq 1)
    $p6 = Invoke-Kit 'preflight.ps1' @('-Volume', $base, '-InjectRamGb', '8', '-InjectFreeGb', '200', '-CapsFile', $caps)
    Add-Result 'preflight-guard-pass' '0' ([string]$p6.Code) ($p6.Code -eq 0)
    $p10 = Invoke-Kit 'preflight.ps1' @('-Volume', $base, '-InjectRamGb', '8', '-InjectFreeGb', '30', '-CapsFile', $caps10)
    Add-Result 'preflight-guard-cap-reserved' '1' ([string]$p10.Code) ($p10.Code -eq 1)
    $p11 = Invoke-Kit 'preflight.ps1' @('-Volume', $base, '-InjectRamGb', '8', '-InjectFreeGb', '40', '-CapsFile', $caps10)
    Add-Result 'preflight-guard-cap-control' '0' ([string]$p11.Code) ($p11.Code -eq 0)
} else { [Console]::Out.WriteLine('note: no guard.exe in the kit: the guard cases are skipped (section 15 copies it)') }

# embed.ps1.
$js = Join-Path $base 'w.js'
$aj = Join-Path $base 'a.json'
Write-Text $aj "{ `"unit`": `"u1`", `"dryRun`": true }`n"
Write-Text $js "export const meta = { name: 'w', description: 'd' }`nconst B = 1`n"
$e1 = Invoke-Kit 'embed.ps1' @('-Script', $js, '-ArgsFile', $aj, '-Out', (Join-Path $base 'e1.js'))
Add-Result 'embed-no-marker-refused' '2' ([string]$e1.Code) ($e1.Code -eq 2)
Write-Text $js "export const meta = { name: 'w', description: 'd' }`r`nconst A = args || {} // EMBED`r`nconst A = args || {}`r`n"
$e2 = Invoke-Kit 'embed.ps1' @('-Script', $js, '-ArgsFile', $aj, '-Out', (Join-Path $base 'e2.js'))
Add-Result 'embed-two-markers-refused' '2' ([string]$e2.Code) ($e2.Code -eq 2)
Write-Text $js "export const meta = { name: 'w', description: 'd' }`nconst A = args || {} // EMBED`nreturn A`n"
[System.IO.File]::WriteAllText($js, ([System.IO.File]::ReadAllText($js)).Replace("`n", "`r`n"))
$e3 = Invoke-Kit 'embed.ps1' @('-Script', $js, '-ArgsFile', $aj, '-Out', (Join-Path $base 'e3.js'))
$t3 = ''
if (Test-Path -LiteralPath (Join-Path $base 'e3.js')) { $t3 = [System.IO.File]::ReadAllText((Join-Path $base 'e3.js')) }
Add-Result 'embed-writes-lf-copy' '0, no CR, embedded' ('{0}, CR {1}, embedded {2}' -f $e3.Code, $t3.Contains("`r"), $t3.Contains('const A = { "unit": "u1"')) ($e3.Code -eq 0 -and -not $t3.Contains("`r") -and $t3.Contains('const A = { "unit": "u1"'))

# local-settings.ps1 on a synthetic worktree.
$wt = Join-Path $base 'wt-demo'
New-Item -ItemType Directory -Path (Join-Path $wt '.claude') | Out-Null
$abs = ConvertTo-ClaudeAbs $wt
$seed = "{`n  `"permissions`": { `"deny`": [ `"Read($abs/crates/secret/**)`" ] },`n  `"env`": { `"CARGO_TARGET_DIR`": `"D:/t/laneA`", `"CARGO_BUILD_JOBS`": `"4`" }`n}`n"
Write-Text (Join-Path $wt '.claude\settings.local.json') $seed
$allowFile = Join-Path $base 'allow.txt'
Write-Text $allowFile "# test`nBash(echo selftest)`n"
$orch = Join-Path $base 'orch'
$root = Join-Path $base 'wtroot'
$l1 = Invoke-Kit 'local-settings.ps1' @('-Worktree', $wt, '-Orch', $orch, '-WtRoot', $root, '-AllowFile', $allowFile)
$t1 = [System.IO.File]::ReadAllText((Join-Path $wt '.claude\settings.local.json'))
$wantRule = 'Read(' + (ConvertTo-ClaudeAbs $root) + '/**/crates/secret/**)'
$wantBase = 'Read(' + (ConvertTo-ClaudeAbs $orch) + '/baseline.tsv)'
$ok1 = ($l1.Code -eq 0 -and $t1.Contains($wantRule.Replace('\', '/')) -and $t1.Contains($wantBase) -and $t1.Contains('Bash(echo selftest)') -and $t1.Contains('D:/t/laneA'))
Add-Result 'local-settings-adds-and-keeps' '0, rules added (baseline denied), env kept' ('{0}: {1}' -f $l1.Code, $l1.Out) $ok1
$l2 = Invoke-Kit 'local-settings.ps1' @('-Worktree', $wt, '-Orch', $orch, '-WtRoot', $root, '-AllowFile', $allowFile)
$t2 = [System.IO.File]::ReadAllText((Join-Path $wt '.claude\settings.local.json'))
Add-Result 'local-settings-idempotent' '0, unchanged' ('{0}, unchanged {1}' -f $l2.Code, ($t1 -eq $t2)) ($l2.Code -eq 0 -and $t1 -eq $t2)
Write-Text (Join-Path $wt '.claude\settings.local.json') "{ not json`n"
$l3 = Invoke-Kit 'local-settings.ps1' @('-Worktree', $wt, '-Orch', $orch, '-WtRoot', $root, '-AllowFile', $allowFile)
Add-Result 'local-settings-malformed-refused' '2' ([string]$l3.Code) ($l3.Code -eq 2)

# start-gate -Detach on a synthetic git repository: the checkout under the lock, the lockfile rules, DONE's head,
# tree and lockfile fields.
$repo = Join-Path $base 'repo'
New-Item -ItemType Directory -Path $repo | Out-Null
$gi = @('-c', 'user.name=selftest', '-c', 'user.email=selftest@invalid', '-c', 'commit.gpgsign=false')
Invoke-Git $repo @('init', '-q') | Out-Null
Invoke-Git $repo @('config', 'core.autocrlf', 'false') | Out-Null
Write-Text (Join-Path $repo 'a.txt') "a`n"
Write-Text (Join-Path $repo 'Cargo.lock') "lock 1`n"
Invoke-Git $repo @('add', '--', 'a.txt', 'Cargo.lock') | Out-Null
Invoke-Git $repo ($gi + @('commit', '-q', '-m', 'base')) | Out-Null
Invoke-Git $repo @('switch', '-q', '-c', 'm0/demo') | Out-Null
Write-Text (Join-Path $repo 'a.txt') "a`nb`n"
Invoke-Git $repo ($gi + @('commit', '-q', '-am', 'demo')) | Out-Null
Invoke-Git $repo @('switch', '-q', '-c', 'm0/demo-lock') | Out-Null
Write-Text (Join-Path $repo 'Cargo.lock') "lock 2`n"
Invoke-Git $repo ($gi + @('commit', '-q', '-am', 'lock')) | Out-Null
Invoke-Git $repo @('switch', '-q', '--detach', 'm0/demo') | Out-Null
$tip = (Invoke-Git $repo @('rev-parse', 'm0/demo')).Out.Trim()
$tipTree = (Invoke-Git $repo @('rev-parse', 'm0/demo^{tree}')).Out.Trim()
$c1 = Start-Detached 'detach-clean' $repo 'm0/demo'
if ($c1.Start.Code -eq 0) {
    $w = Wait-Done $c1.Run
    Add-Result 'detach-records-head-and-tree' "PASS, head $tip, tree $tipTree, lockfile current" ('{0}, head {1}, tree {2}, lockfile {3}' -f $w.Done['verdict'], $w.Done['head'], $w.Done['tree'], $w.Done['lockfile']) ($w.Done['verdict'] -eq 'PASS' -and $w.Done['head'] -eq $tip -and $w.Done['tree'] -eq $tipTree -and $w.Done['lockfile'] -eq 'current')
} else { Add-Result 'detach-records-head-and-tree' 'start 0' $c1.Start.Out $false }
Write-Text (Join-Path $repo 'stray.txt') "x`n"
$c2 = Start-Detached 'detach-dirty' $repo 'm0/demo'
Add-Result 'detach-dirty-refused' '2' ([string]$c2.Start.Code) ($c2.Start.Code -eq 2)
Remove-Item -LiteralPath (Join-Path $repo 'stray.txt') -Force
$c3 = Start-Detached 'gate-writes-lockfile' $repo 'm0/demo' (Join-Path $repo 'Cargo.lock')
if ($c3.Start.Code -eq 0) {
    $w = Wait-Done $c3.Run
    Add-Result 'lockfile-update-reported' 'lockfile updated' ('lockfile {0}, changes {1}' -f $w.Done['lockfile'], $w.Done['changes']) ($w.Done['lockfile'] -eq 'updated')
} else { Add-Result 'lockfile-update-reported' 'start 0' $c3.Start.Out $false }
$c4 = Start-Detached 'lockfile-differs' $repo 'm0/demo-lock'
Add-Result 'foreign-lockfile-refused' '2' ([string]$c4.Start.Code) ($c4.Start.Code -eq 2)
Write-Text (Join-Path $repo 'Cargo.lock') "lock 2`n"
$c5 = Start-Detached 'lockfile-equals-branch' $repo 'm0/demo-lock'
$h5 = (Invoke-Git $repo @('rev-parse', 'HEAD')).Out.Trim()
$t5 = (Invoke-Git $repo @('rev-parse', 'm0/demo-lock')).Out.Trim()
Add-Result 'lockfile-equal-to-branch-staged-and-switched' "start 0, HEAD $t5" ('start {0}, HEAD {1}' -f $c5.Start.Code, $h5) ($c5.Start.Code -eq 0 -and $h5 -eq $t5)
if ($c5.Start.Code -eq 0) { Wait-Done $c5.Run | Out-Null }

$bad = @($results | Where-Object { -not $_.Ok })
Write-Output ('selftest: {0} of {1} cases behaved; kit {2}; runs in {3}' -f ($results.Count - $bad.Count), $results.Count, (Get-KitHash), $base)
if ($bad.Count -gt 0) { Write-Output 'selftest: FAIL'; exit 1 }
Write-Output 'selftest: PASS'
exit 0
```
