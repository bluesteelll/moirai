# moirai — instructions for agents

moirai is a from-scratch Rust graph database that serves AI coding agents as a memory system, a task tracker and a store
of project rules and decisions. It has its own git-like version control, a git-compatible image, file links that survive
moves, and its own query language (Lachesis). The project is at the design stage: there is no code yet.

Start with [docs/ARCHITECTURE-RESEARCH.md](docs/ARCHITECTURE-RESEARCH.md). Its §0 summarizes the design, §9 has the roadmap
and §11 has the owner decisions. Detailed designs are in [docs/research/design/](docs/research/design/) and the research
reports in [docs/research/](docs/research/).

## Git

- Every commit is authored by the owner. **Never add `Co-Authored-By: Claude` or any other AI co-author trailer or marker
  to a commit message.** Never add "Generated with Claude Code" or a similar line to a pull request description.
- In the unit workflow ("Agent work" below), the owner's standing permission of 2026-10-07 (R-2) applies: agents
  commit on their unit's role branch; only the orchestrator merges into local `master`, after a green
  `cargo xtask gate --branch m0/<role>` in the gate worktree, and then pushes it with `git push origin master`.
  Outside that workflow, commit or push only when the owner asks. Never use `--force` or `--no-verify` without
  explicit permission.

## License

- Apache-2.0, copyright Mark Boyko. See LICENSE and NOTICE.
- Keep NOTICE intact.
- Every dependency must have an Apache-2.0-compatible license (MIT, Apache-2.0, BSD, ISC, Zlib and similar).

## Data

- This repository is public. Never commit owner-derived data: corpora, real session prompts, recorded briefs, load
  fixtures, memory contents, or anything under `private/`. Tests and CI use synthetic data only.
- Never commit secrets, tokens or credentials.

## Binding rules (details in docs/ARCHITECTURE-RESEARCH.md)

- No SQLite or any other third-party embedded database, anywhere: not as a backend, a test oracle or a benchmark. The
  engine is written from scratch. The test oracle is a naive reference model written in Rust.
- Every component is built to its final specification in dependency order. There are no interim or throwaway stages.
- The priorities are speed, minimal RAM, correctness and minimal agent tokens. Each has budgets and gates.
- Windows, Linux and macOS are all designed for. Windows is built and tested now. `cargo check` for the Linux and macOS
  targets is a gate from M0, so every dependency must be pure Rust.
- The system must work with Claude Code, Codex and other harnesses. Hooks are optional accelerators.
- Runtime and operational policy goes into config keys with defaults, not into owner questions.
- Repository artifacts are written in English. Chat with the owner may be in Russian.

## Agent work

The procedure, the orchestrator's loop and the Claude Code mechanics are in
[docs/m0/workflow.md](docs/m0/workflow.md). A **unit** is one reviewable portion of one WP by one author role, on the
role's branch `m0/<role>` in its worktree (PLAN §3.1 "Mechanics"). The trunk is local `master`. `<ORCH>` is the
orchestration state outside the repository (workflow.md §3).

- **Layout.** `crates/` (the workspace, PLAN §2.2), `xtask/` (the gate and its lints; author roles and lanes in
  `xtask/roles.toml`), `docs/spec/` (the specification), `docs/m0/` (the M0 plan, `authors.md`, tools, nightly,
  workflow), `fixtures/`, `fuzz/`, `.githooks/`. Lane target directories live outside the repository (PLAN §2.1).
- **Gates.**
  - The full verdict: `cargo xtask gate --branch m0/<role>`, only in the neutral gate worktree detached at the branch
    tip, run through the kit. It is the only place `Cargo.lock` changes. CI runs `cargo xtask gate --ci`.
  - Per commit, the developer runs only its own crates: `cargo fmt -p <crates> -- --check`,
    `cargo clippy --locked -p <crates> --all-targets -- -D warnings` and
    `MOIRAI_TEST_TIER=pr cargo test --locked -p <crates> --no-fail-fast`.
  - Every run runs everything (`--keep-going`, `--no-fail-fast`) and never stops at the first failure. A test run with
    `running 0 tests` where tests are expected is red.
- **Separation of duties.** Stage roles (architect, architecture-critic, developer, tester, code-reviewer, which also
  triages, researcher, project-analyst, results-analyst, doc-writer, mechanic) sit on top of PLAN §3.1's author roles.
  Every agent of a unit obeys the unit's author role: S1–S6, its `deny_read` in `xtask/roles.toml` and the write map of
  `docs/m0/authors.md` §3. `deny_read` binds every way of reading, Bash included (`cat`, `rg`,
  `git show <rev>:<path>`, a `git diff` over those paths). A file you need but may not read is a review finding in
  your report; never read it. The developer never gives the final verdict; the tester owns the full suite, the pins
  and the mutations and never commits; the reviewer finds and never fixes; the critic critiques and never redesigns;
  the researcher and the analysts never edit the repository; the mechanic runs and summarises and never gives a
  verdict. An agent that meets an ambiguity stops and escalates in its report.
- **Plan Mode.** An interactive change that touches three or more files goes through Plan Mode. Inside a unit the
  critiqued `cut.md` is the approved plan, and a subagent never enters Plan Mode.
- **Gate discipline.**
  - Red-first: every behavioural claim and every new gate is shown failing for the predicted reason on the tree
    without the change, then passing with it.
  - A gate script is trusted only after it was seen to exit non-zero on a failing row.
  - Tests are counted by name against the expected set. Equal counts over different sets, or zero tests, are a
    vacuous green.
  - Every skipped or ignored test states its reason class at the site: needs-hardware, needs-network,
    needs-credentials, slow, flaky, generator or deferred.
  - A single red you cannot reproduce is re-run alone before it is blamed. A pin (a committed expected value: a
    snapshot, golden file, hash or count) is never re-blessed from one odd run.
- **Tokens.** Logs stay on disk. Search with `rg -n` and read in ranges of about 200 lines; read summaries, verdict
  lines and failure excerpts, never whole logs. Save your report to the path your brief names and return only the
  verdict. Briefs and reports carry paths, not pasted documents. Every shell command starts with `cd` into your own
  worktree. Long runs go through the kit under a budget; waits are bounded; a hung run is stopped by its own PID tree,
  never by image name.
- **Git in agent work.**
  - Commit only on your unit's branch, with an explicit path list after reading `git diff --cached --stat`. Subjects
    start `WP-xx:` with a WP that `docs/m0/authors.md` §2 gives the unit's author role.
  - Never `--force` or another forced form (`-f`, `branch -D`, `switch --discard-changes`, `checkout .`),
    `--no-verify`, a hook bypass (`-c core.hooksPath=`), `checkout --`, `restore`, `reset --hard`, `stash`, `clean`,
    `merge --abort` or `--amend`. Only the orchestrator merges into `master` and pushes it ("Git" above).
- Orchestrator: read `docs/m0/workflow.md` and `<ORCH>/RESUME.md` before launching anything.
