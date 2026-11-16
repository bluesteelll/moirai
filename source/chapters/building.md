---
title: How it is built, and where it stands
nav: Roadmap & status
description: The principles, specification, reference model, tests and agent process behind Moirai, the roadmap from M0 to M11, and what the public repository holds today.
---

# How it is built, and where it stands

Moirai is built slowly on purpose. The on-disk format is specified to the byte before any engine code exists. A deliberately naive reference model is written first, so the engine has something independent to be checked against. Every component is then built once, to its final specification, behind gates that try hard to break it. This chapter explains those rules, shows the roadmap from M0 to M11, and lists what the public repository holds today.

> [!NOTE]
> **Status in one paragraph**
>
> Nothing is released, and nothing can be installed or used yet. The project is in milestone M0, "contract and evidence": the specification, the reference model, test harnesses, fixtures and the first measurements exist. The storage engine is built in M1, which starts after the on-disk format version 1 is frozen with the git tag `format-v1`. There is no release date.

## Build principles {#principles}

- **Written from scratch.** The storage engine, the version control, the git object layer and the query language are implemented inside the project, in Rust. That includes the log and segment codecs, the lock protocol, the columnar graph and its bitsets, the merge engine and its three-way text merge, the git object reader and writer, and the query language's lexer, parser, planner and executor, with no parser generator and no regular-expression library.
- **No third-party database, anywhere.** SQLite and every other embedded database (redb, LMDB, sled, RocksDB and the like) are excluded from the product, and also as a temporary backend, a test oracle, a benchmark baseline or a point of comparison. A dependency lint scans every lock file the project keeps and fails if one appears. The same lint rejects git libraries, because the git object layer is the project's own.
- **Built once, in dependency order.** Every component is built to its final specification and passes all of its gates before anything is built on top of it. There are no interim or throwaway stages, no temporary modes, no node limits and no weakened guarantees taken to ship sooner. A milestone that has passed its gates is changed later only through an extension point that was itself certified earlier, such as the storage engine's registry of segment sections.
- **Used only when finished.** The maintainer's own agent workflow starts using Moirai only when the release gate is met. Nothing goes into daily use half-built.
- **Small, pure-Rust dependencies.** The allowed dependencies are leaf libraries such as `zerocopy`, `blake3` in its pure-Rust mode, `xxhash-rust`, `sha1` and `sha2`, a pure-Rust compression codec chosen by measurement, `serde_json`, and `windows-sys` or `libc` inside the OS layer only. Every crate forbids `unsafe` code except the OS layer, where each unsafe block carries a written safety argument.

Four priorities decide every trade-off: speed, minimal memory, correctness and minimal agent tokens. Each has numeric budgets and a gate that enforces them; the engine chapter lists [a selection of the budgets](engine.html#budgets).

## Specification first {#specification}

The design is written down in an architecture document and five normative design documents: file links, the query language, the roadmap, cross-platform support and harness independence. From them, milestone M0 writes a byte-level specification in `docs/spec/`. Its standard is that two independent implementers who follow it produce the same bytes. Every fixed-size structure has an offset table (offset, width, type, name, meaning) in little-endian order, with every padding and reserved byte stated.

| Part | What it fixes |
|---|---|
| Format (21 chapters) | the store layout, the `LOCK` and `HEAD` files, the log and its records, commits and their canonical form, the data model, segments, sealed files, runtime tables, version control, invariants, the git image, the fault model, the commit protocol, store parameters, file links, errors and output, and the file-link resolver's constants and scanners |
| OS layer (12 files) | files and durability, locks, memory mappings, the file-system guard, processes and clocks, memory metering, paths, project files and shell transport, each mapped to Windows, Linux and macOS |
| Query language (9 files) | the grammar, lexical rules, the canonical syntax tree, a JSON form, error texts, the result envelope, the standard library, the agent's reference card and the GQL spellings |
| Store API, configuration, measurement protocol | the logical commands that both the engine and the model implement; every configuration key; how budgets are measured |
| Rule tables (9 files) | the merge table, status machines, delete policies, link merge rules, context-pack classes, the readiness state definition, the role write policy and the policy keys, written so the reference model reads them as data |

### Reviews and named holes {#reviews-and-holes}

Independent reviewers read the specification through three lenses: performance, memory and Windows behaviour; semantics and correctness; and agent fit, tokens and buildability. A reviewer never reviews text it wrote. A finding is a *blocker* when two independent implementers could produce different bytes or a rule of the design is broken, and *major* when it must be fixed before the freeze. The freeze requires zero open blocker and major findings.

Some values cannot be settled by argument. They are written into the text as [named holes](glossary.html#term-named-hole), `HOLE(<id>)`, and indexed in one file together with their candidates and the measurement that decides each one. For example, the number of log operations after which a checkpoint runs is a hole: a measurement replays a full log tail at several candidate thresholds and the chosen value must keep opening a store within its time budget. A coverage index maps every frozen item to the sections that specify it, the fixtures that exercise it and the model function that implements it.

The freeze is the git tag `format-v1`. After it, any change to a frozen byte reopens M0, and every milestone that has already passed runs its gates again.

## The reference model {#reference-model}

Before the engine exists, a complete and deliberately naive [reference model](glossary.html#term-reference-model) of Moirai's logic is written in Rust, in the crate `moirai-model`. It is the executable specification: every rule is written there first, as a function that cites its source. It implements the same logical command interface as the engine, and from M1 on the engine is compared with it command by command, including commit ids and the full state of every touched branch.

A model helps only if it is not the engine in disguise, so wherever the engine optimizes, the model uses the plain definitional algorithm:

| Concern | Engine | Reference model |
|---|---|---|
| Current state | base and delta segments plus an in-memory overlay | an ordered map, built by replaying every changeset from the first commit |
| Derived state ("ready", "unblocked", blockers) | maintained incrementally, stored as bitsets | recomputed from scratch on every query |
| Cycle checks | an incremental topological order | a depth-first search over the whole graph |
| Branch views | pinned segments plus the branch's operations | a replay of the commit graph from its root |
| Commit ids | the product codec | its own canonical-form encoder, so ids are compared byte for byte |
| Queries | a parser, a planner and a vectorized executor | its own small parser and a nested-loop evaluator |

More guards keep the two apart. The model's author never reads engine code, and engine authors never read the model. The rule tables are data documents that the maintainer signs off. Where a fully independent oracle exists, it is used as well: the git command-line tool for git objects and images, and a simulator's ground truth for file identity. The model is test-only: it is never linked into the binary and never used as a backend. A separate format oracle decodes every frozen structure independently, starting from hand-written hex fixtures.

## The test workstream {#test-workstream}

Testing is a workstream of its own, planned at about a fifth of the build effort, and it starts in M0 rather than at the end.

- **A simulated file system with fault injection.** All storage code is written against a file-system interface. Its in-memory implementation enforces a written [fault model](glossary.html#term-fault-model): any subset of unflushed sectors may be lost and one may be torn; a rename or delete is durable only after its directory is synced; a failed flush may leave old or new bytes; the disk may fill up; processes may pause for any length of time; the wall clock may jump.
- **[Crash-point enumeration](glossary.html#term-crash-point-enumeration).** A crash is injected at every write, flush and publish boundary. At each point, every subset of a file's unflushed sectors is tried while there are at most 12 of them (4,096 states), at least 10,000 random subsets beyond that, and every combination of the two `HEAD` slots. After each crash, recovery must keep every acknowledged effect, and a process started after the crash must see all of them before any writer runs.
- **Multi-process simulation.** Many simulated processes run under seeded scheduling, with crashes, failed flushes, late lock releases and clock jumps injected; the model re-checks every read.
- **Kill loops on Windows.** Sixteen real writer and reader processes on different branches are killed at random points, suspended for up to two minutes, or hit by one-hour clock steps, for 10,000 iterations per variant. Every process reports each acknowledgement to the test harness before it prints it, and not one may be lost.
- **Fuzzers** for every parser: log records and segments, the `.moi` text format, git objects, query text and command-line arguments.
- **[Mutation testing](glossary.html#term-mutation-testing).** At every milestone exit, small deliberate changes (mutants) are made to the semantic code, and the test suites must catch at least 90 % of them. A catalogue of deliberate protocol bugs, such as skipping the flush, publishing before the flush or deleting a file before the `HEAD` barrier, must be caught every time.
- **Differential tests** against the reference model, with at least a million commands per milestone, and against the git command-line tool (`git fsck`, `git cat-file`, `git merge-base`) for every exported image.

The commit protocol is first implemented in a small "toy log" with switchable seeded bugs, so the crash enumerator is shown to find every one of them before the real engine exists. The author who plants the bugs is never the author of the enumerator, so the harness cannot be tuned to its own bugs.

### The gates in plain words {#gate-catalogue}

A failing gate blocks the exit of its milestone and of every later one. Once a gate is mandatory, it stays mandatory. The gates range from crash enumeration, simulation, kill loops and differential tests against the model to the budgets, the token ledger and lints that keep the code independent of git and of third-party databases.

<details>
<summary>The full list of gates</summary>

| Gate | What it checks | Mandatory from |
|---|---|---|
| Crash enumeration | no acknowledged record is lost at any crash point | M1 (on the toy log in M0) |
| Differential against the model | engine and model agree on results, commit ids and state | M1, extended in each milestone |
| Multi-process simulation | no lost record; reads only move forward; reads agree with the model | M1 |
| Windows kill loop | no lost commit, lease, branch move or marker after real process kills | M1, extended up to M10 |
| Fuzzing | no crash and no hang | with each component |
| Properties | derived state equals a full recomputation; merges are deterministic | M2, M3 |
| Git as an oracle | agreement with the git command-line tool | M4, M5 |
| Image round trip | export, import and export again gives identical bytes | M5 |
| Query metamorphic tests | a condition and its negation split the data exactly; result pages never skip or repeat | M7 |
| Verified fixtures | recorded scenarios give the expected results; the maintainer verifies the core set | with each feature |
| Budgets | every time and memory budget, measured under the protocol | per budget |
| Contract and harness conformance | golden command output, shells, hooks and MCP handshakes | M8 |
| Query accuracy benchmark | agents write correct queries in Lachesis | before the query surface is frozen, then on the product |
| Soak | 72 hours of mixed load with kill loops, without a lost commit | M11 |
| Mutation testing | at least 90 % of mutants caught | every milestone exit |
| File-link pattern matrix | 36 move and edit patterns on a simulator and on real NTFS | M6 |
| State oracles | readiness, sync and lease rules against their definitions | M0 on the model, then on the engine |
| Token ledger | every byte placed into agent contexts, against the token budgets | M9 |
| Independence lints | no git process outside four named call sites, no database or git library, OS code only in the OS layer, pure Rust for every target | M0 and M1 |

</details>

## The release gate {#release-gate}

Release means "complete and hardened". The [release gate](glossary.html#term-release-gate) requires, among other things:

- every milestone exited with all of its gates, and every feature planned for the release built;
- branches, the own version control, the git image, file links and the query language shown end to end through the binary, the hooks and the MCP server;
- the multi-process simulation run nightly for at least four weeks, and the kill loops in every variant clean for the last 14 nights on the release-candidate commit;
- a 72-hour soak with kill loops across branches, with no lost acknowledged commit and no finding from `moirai doctor --verify`;
- at least ten million differential cases per component with no open disagreement, a mutation kill rate of at least 90 % and at least seven CPU-days of fuzzing per fuzzer;
- every budget met at 10,000, 100,000 and 1,000,000 nodes and at the expected real scale, with no number still an estimate;
- drills for backup, restore, repair, image restore and a full disk, and an upgrade drill to a synthetic format version 2;
- a final review through the three lenses with no open blocker or major finding, and no open defect of major severity.

The power-loss test rig is not part of this gate. It is deferred to after the release, and until it runs, real power loss stays a stated, unverified risk.

## Built by AI agents under a strict process {#agent-process}

Moirai is written by AI coding agents (Claude Code sessions) that follow a written procedure, `docs/m0/workflow.md`, and the rules in `AGENTS.md`. The procedure is built so that no single agent both makes a change and declares it correct.

- **Units of work.** A [unit](glossary.html#term-unit) is one reviewable piece of one work package by one author role: at most about ten commits and fifteen files. It runs on the role's branch, `m0/<role>`, in its own git worktree, and it may write only the paths that its role owns.
- **Author roles with separation rules.** Who may write what, and who must not read what, is fixed per role. The model's author never reads engine code. The fixture author, the format-oracle author and the product-codec author are three different authors. The author of the seeded bugs is never the author of the crash enumerator. Reviewers never review their own text.
- **[Red-first](glossary.html#term-red-first).** Every behavioural claim and every new or changed gate must first be shown failing, for the predicted reason, on the tree without the change, and then passing with it.
- **Counting by name.** Every test run is compared, test by test, with the expected set of tests. A run that reports zero tests where tests are expected counts as a failure.
- **Merging.** Only one orchestrating session merges into the trunk, and only after a green full gate (`cargo xtask gate --branch m0/<role>`) in a separate, neutral worktree. Before the `format-v1` tag, units whose tests read each other's bytes, such as fixtures and the oracle that decodes them, land together as one set, or not at all.

<details>
<summary>Stage roles, tiers and deliberate bugs</summary>

- **Stage roles.** Inside a unit, separate agents act as architect, architecture critic, developer, tester, verifier and code reviewer (which also triages findings), and a mechanic only runs gates and summarises them. The developer never gives the final verdict. The tester and the verifier never commit and never fix. The reviewer finds problems and never fixes them.
- **Tiers.** A unit runs at one of three depths. LIGHT is a text change with no change of behaviour and no test change. HEAVY covers product behaviour, crash-safety evidence, bytes and ids, rule changes, open decisions and the merge gate itself; it goes through a written plan, a critique of the plan, the implementation, and then a full gate with an independent tester, in parallel with a code review, followed by triage and fixes, for up to three rounds. STANDARD is everything else. When in doubt, the higher tier applies.
- **Deliberate bugs.** In a HEAVY unit the tester plants at least two mutations to show that the gates catch them; in a STANDARD unit the verifier plants one.

</details>

Tests and CI use synthetic data only.

## Roadmap {#roadmap}

The work is split into twelve milestones in dependency order. Each one is finished, with all of its gates green, before the next one builds on it.

<figure class="diagram">
<svg viewBox="0 0 720 320" role="img" aria-labelledby="building-d1-t">
  <title id="building-d1-t">The milestone path from M0 to M11, with the current position at M0</title>
  <defs><marker id="building-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path class="d-arrow" d="M0,0 L10,5 L0,10 z"/></marker></defs>
  <text class="d-text-strong" x="60" y="22" text-anchor="middle">We are here</text>
  <path class="d-edge-accent" d="M60,30 L60,52" marker-end="url(#building-arrow)"/>
  <path class="d-box-accent" d="M20,54 H100 A8,8 0 0 1 108,62 V102 A8,8 0 0 1 100,110 H20 A8,8 0 0 1 12,102 V62 A8,8 0 0 1 20,54 Z"/>
  <text class="d-text-strong" x="60" y="76" text-anchor="middle">M0</text>
  <text class="d-text-small" x="60" y="96" text-anchor="middle">Contract</text>
  <path class="d-box" d="M140,54 H220 A8,8 0 0 1 228,62 V102 A8,8 0 0 1 220,110 H140 A8,8 0 0 1 132,102 V62 A8,8 0 0 1 140,54 Z"/>
  <text class="d-text-strong" x="180" y="76" text-anchor="middle">M1</text>
  <text class="d-text-small" x="180" y="96" text-anchor="middle">Storage</text>
  <path class="d-box" d="M260,54 H340 A8,8 0 0 1 348,62 V102 A8,8 0 0 1 340,110 H260 A8,8 0 0 1 252,102 V62 A8,8 0 0 1 260,54 Z"/>
  <text class="d-text-strong" x="300" y="76" text-anchor="middle">M2</text>
  <text class="d-text-small" x="300" y="96" text-anchor="middle">Graph core</text>
  <path class="d-box" d="M380,54 H460 A8,8 0 0 1 468,62 V102 A8,8 0 0 1 460,110 H380 A8,8 0 0 1 372,102 V62 A8,8 0 0 1 380,54 Z"/>
  <text class="d-text-strong" x="420" y="76" text-anchor="middle">M3</text>
  <text class="d-text-small" x="420" y="96" text-anchor="middle">Versions</text>
  <path class="d-box" d="M500,54 H580 A8,8 0 0 1 588,62 V102 A8,8 0 0 1 580,110 H500 A8,8 0 0 1 492,102 V62 A8,8 0 0 1 500,54 Z"/>
  <text class="d-text-strong" x="540" y="76" text-anchor="middle">M4</text>
  <text class="d-text-small" x="540" y="96" text-anchor="middle">Git objects</text>
  <path class="d-box" d="M620,54 H700 A8,8 0 0 1 708,62 V102 A8,8 0 0 1 700,110 H620 A8,8 0 0 1 612,102 V62 A8,8 0 0 1 620,54 Z"/>
  <text class="d-text-strong" x="660" y="76" text-anchor="middle">M5</text>
  <text class="d-text-small" x="660" y="96" text-anchor="middle">Git image</text>
  <path class="d-edge" d="M108,82 L132,82" marker-end="url(#building-arrow)"/>
  <path class="d-edge" d="M228,82 L252,82" marker-end="url(#building-arrow)"/>
  <path class="d-edge" d="M348,82 L372,82" marker-end="url(#building-arrow)"/>
  <path class="d-edge" d="M468,82 L492,82" marker-end="url(#building-arrow)"/>
  <path class="d-edge" d="M588,82 L612,82" marker-end="url(#building-arrow)"/>
  <text class="d-text-small" x="120" y="130" text-anchor="middle">tag format-v1</text>
  <path class="d-edge" d="M660,110 L660,148 L60,148 L60,172" marker-end="url(#building-arrow)"/>
  <path class="d-box" d="M20,174 H100 A8,8 0 0 1 108,182 V222 A8,8 0 0 1 100,230 H20 A8,8 0 0 1 12,222 V182 A8,8 0 0 1 20,174 Z"/>
  <text class="d-text-strong" x="60" y="196" text-anchor="middle">M6</text>
  <text class="d-text-small" x="60" y="216" text-anchor="middle">File links</text>
  <path class="d-box" d="M140,174 H220 A8,8 0 0 1 228,182 V222 A8,8 0 0 1 220,230 H140 A8,8 0 0 1 132,222 V182 A8,8 0 0 1 140,174 Z"/>
  <text class="d-text-strong" x="180" y="196" text-anchor="middle">M7</text>
  <text class="d-text-small" x="180" y="216" text-anchor="middle">Lachesis</text>
  <path class="d-box" d="M260,174 H340 A8,8 0 0 1 348,182 V222 A8,8 0 0 1 340,230 H260 A8,8 0 0 1 252,222 V182 A8,8 0 0 1 260,174 Z"/>
  <text class="d-text-strong" x="300" y="196" text-anchor="middle">M8</text>
  <text class="d-text-small" x="300" y="216" text-anchor="middle">CLI</text>
  <path class="d-box" d="M380,174 H460 A8,8 0 0 1 468,182 V222 A8,8 0 0 1 460,230 H380 A8,8 0 0 1 372,222 V182 A8,8 0 0 1 380,174 Z"/>
  <text class="d-text-strong" x="420" y="196" text-anchor="middle">M9</text>
  <text class="d-text-small" x="420" y="216" text-anchor="middle">Packs, hooks</text>
  <path class="d-box" d="M500,174 H580 A8,8 0 0 1 588,182 V222 A8,8 0 0 1 580,230 H500 A8,8 0 0 1 492,222 V182 A8,8 0 0 1 500,174 Z"/>
  <text class="d-text-strong" x="540" y="196" text-anchor="middle">M10</text>
  <text class="d-text-small" x="540" y="216" text-anchor="middle">MCP server</text>
  <path class="d-box" d="M620,174 H700 A8,8 0 0 1 708,182 V222 A8,8 0 0 1 700,230 H620 A8,8 0 0 1 612,222 V182 A8,8 0 0 1 620,174 Z"/>
  <text class="d-text-strong" x="660" y="196" text-anchor="middle">M11</text>
  <text class="d-text-small" x="660" y="216" text-anchor="middle">Hardening</text>
  <path class="d-edge" d="M108,202 L132,202" marker-end="url(#building-arrow)"/>
  <path class="d-edge" d="M228,202 L252,202" marker-end="url(#building-arrow)"/>
  <path class="d-edge" d="M348,202 L372,202" marker-end="url(#building-arrow)"/>
  <path class="d-edge" d="M468,202 L492,202" marker-end="url(#building-arrow)"/>
  <path class="d-edge" d="M588,202 L612,202" marker-end="url(#building-arrow)"/>
  <path class="d-edge-dashed" d="M660,230 L660,260" marker-end="url(#building-arrow)"/>
  <text class="d-text-small" x="652" y="250" text-anchor="end">after the release</text>
  <path class="d-box-muted" d="M380,262 H700 A8,8 0 0 1 708,270 V298 A8,8 0 0 1 700,306 H380 A8,8 0 0 1 372,298 V270 A8,8 0 0 1 380,262 Z"/>
  <text class="d-text" x="540" y="288" text-anchor="middle">Later, unscheduled: Linux and macOS ports</text>
</svg>
<figcaption>The project is at M0. The tag <code>format-v1</code> ends M0 and starts M1. M11, release hardening, ends at the release gate; the Linux and macOS ports come after the release.</figcaption>
</figure>

| Milestone | What it delivers |
|---|---|
| M0 Contract and evidence | the frozen on-disk format version 1, the byte-level specification, the reference model, test harnesses, fixtures and measurements |
| M1 Storage engine | the log, `HEAD`, locks and group commit, recovery, segments, checkpoints, garbage collection, backup and restore |
| M2 Graph core | node kinds, schema, invariants, derived state such as "ready", delete policies, leases, idempotency, the change feed and full-text search |
| M3 Version control | branches, merge and sync, undo, revert, cherry-pick, tags, the reflog and the operation log |
| M4 Git object layer | reading and writing git objects, packs, refs and bundles inside the process, with SHA-1 and SHA-256 |
| M5 Git image | deterministic export and import of the store as a git repository of text files |
| M6 File links | the runtime that keeps links to project files correct through moves, renames, edits and deletions |
| M7 Query language | Lachesis: parser, planner, executor, transactions and the standard library of named queries |
| M8 Command-line interface | every command with a frozen output contract, tested in Git Bash and PowerShell |
| M9 Agent interface | context packs, briefs, hooks and skills |
| M10 MCP server | the ten MCP tools and the hook handlers that run inside the server |
| M11 Release hardening | nothing new: the release gate |

The graph side of file links and their merge rules are built inside M2 and M3, not as a late add-on, so no certified milestone has to be reopened for them. Leaf components, such as the git object layer, can be built in a second lane beside the critical path.

<details>
<summary>Why the agent interface comes before the MCP server</summary>

The MCP server's tools and its hook handlers serve exactly the context packs, briefs and hook logic that M9 builds. An MCP server built first would need that logic built inside it or stubbed out, which would be an interim stage. So M9 certifies packs, briefs, hooks and skills through the command line, and M10 adds the MCP transport for the same logic. Nothing is used before the release gate, so the order changes which gates run first, not what users receive.

</details>

## Where it stands now {#status}

Milestone M0 is under way. This is what the public repository holds today. There is no tag yet; `master` is the trunk.

### The specification {#status-spec}

The specification index, `docs/spec/README.md`, lists 21 format chapters, 12 OS-layer files, 9 query-language files, the Store API with 20 example files, the configuration chapter, the measurement protocol and 9 rule files, plus the coverage and hole indexes. Every one of these files is still a draft, and nothing is frozen. Review pass 1 is closed. Review pass 2 has been run through all three lenses, and its findings are waiting for the authors of each part to resolve them. Many file headers, and the index, still show the older status "draft, pass 1 pending"; a pass-2 finding records that. The reports of the review passes, with their dispositions and closure checks, are in `docs/spec/reviews/`.

### The crates {#status-crates}

M0 writes no engine code. The only product code at this stage is the code the measurements and the file-link libraries need. The other crates are test harnesses and tools.

<details>
<summary>The fifteen crates in the repository today</summary>

| Crate | Kind | What it is |
|---|---|---|
| `moirai-vfs` | product | the storage seam: the file-system interface, the project-file interface, the metering interface, the in-process lock table, clocks and evidence types |
| `moirai-os` | product | the OS layer: its Windows modules (files, locks, mappings, environment checks, processes, memory, project files, paths); the Unix modules are switched off for now |
| `moirai-diff` | product | a bit-parallel Myers matcher and a histogram line diff, used by file anchors now and by the three-way merge later |
| `moirai-files` | product | the pure file-link primitives: path rules and Unicode case folding, text detection and content ids, id derivation, anchor capture and resolution, scope scanners for Rust, Markdown and TOML, ignore matchers and similarity |
| `moirai-model` | test-only | the reference model, with its own query-language parser and evaluator and its own canonical-form encoder |
| `moirai-format-oracle` | test-only | an independent decoder and re-encoder of every frozen structure, and the grammar check of the text formats |
| `moirai-vfs-sim` | test-only | the in-memory file system that enforces the fault model, and the crash enumerator |
| `moirai-toylog` | test-only | a toy log that implements the commit protocol with switchable seeded bugs, for validating the crash harness and for measurements |
| `moirai-probes` | test-only | the measurement framework, layout micro-benchmarks and probe bodies |
| `moirai-probes-bin` | test-only | the Windows-only program that connects the probes to the OS layer |
| `moirai-replay` | test-only | the harness for replaying recorded file-history corpora, and differential tests against git and a parser oracle |
| `moirai-tsoracle` | host-only | a tree-sitter-based oracle for the Rust scope scanner; no other crate depends on it |
| `moirai-tokcount` | tool | offline token counts, and the headless Claude Code call used to read reported token usage |
| `moirai-lqbench` | test-only | the query-language accuracy benchmark; created but not yet filled (its task corpus and plans are under `fixtures/lqbench/`) |
| `moirai-harness-stub` | test-only | a hand-written MCP stdio loop for harness probes; created but not yet filled |

</details>

Beside the crates, `xtask` is the repository's tool runner: the full gate, the lints, git hooks, fixture assembly, worktrees and nightly jobs. `fixtures/` holds hand-written hex fixtures, canonical-form cases, `.moi` files, query fixtures and Unicode tables. `docs/measurements/m0/` holds the first two measurement reports: the commit cycle on a log file with its flush floor, and sixteen writer processes contending for the lock. Recent merges switched the format's byte-level names to Klotho, for example the `KLTH` magic at the start of a `HEAD` slot and the `klotho-` hash-domain strings.

The product itself does not exist yet. The engine crates are created from M1 on, and the `moirai` binary crate in M8.

### What comes next {#status-next}

- Close the specification reviews and fill the named holes from measurements.
- Tag `format-v1`. M0 ends there, and M1, the storage engine, starts.
- After the tag, and before M1 creates its first crate, rename the engine crates to `klotho-*` names with a reviewed script; the M1 plan records the new names.
- A few M0 evidence items, such as the query-language accuracy benchmark, are scheduled as entry gates of the later milestones that first need them.

## Linux and macOS later {#ports}

Linux and macOS are part of the design from the start, and the cross-target type check runs on every merge from M0. Their implementations are built in a port phase after the Windows release. The phase is documented with its own gates, but it is not scheduled. It needs Linux CI with virtualization, an Apple-silicon Mac and a decision about that hardware. A port never changes the format or the protocol: if a port found that it had to, that would be a specification defect, M0 would reopen, and every passed milestone would run its gates again.

## No release date {#no-date}

The project does not publish a release date. Scope is never cut to hold a date; a milestone ends when its gates are green. The size estimates in the plan are checked against the measured pace of work at the exits of M0 and M1 and revised then.

## The largest risks {#risks}

| Risk | How the design answers it |
|---|---|
| The multi-process file protocol loses or corrupts an acknowledged write on Windows | readers stop at the published bound; the branch move is part of the commit record; flush errors are fatal; the frozen fault model, crash enumeration, simulation and kill loops gate M1 before anything is built on it, and they are repeated in later milestones |
| The from-scratch engine takes far longer than estimated | the format, not the engine, is the contract; the engine is split into storage, graph core and version control, each certified on its own; the simulator, the crash enumerator and the reference model are built in M0, and the kill loops gate M1; scope is not cut to hold a date |
| A real power loss loses data that killing processes cannot reveal | the fault model, crash enumeration over unflushed writes, acknowledgement only after a covering flush, backups and daily image exports; the real power-loss rig runs only after the release |
| Branch isolation fights the workflow: work on a lane misses rules or completions made on `main` | rules from `main` in every context pack, "behind main" notices, markers that keep finished or deleted tasks from being handed out on any branch, sync checks |
| The git image drifts and stops being deterministic | no store-local data in hashed content, byte-identical round-trip tests, and an exporter that re-parses what it writes |

## License and how to follow the project {#license-and-follow}

Moirai is licensed under Apache-2.0; see `LICENSE` and `NOTICE` in the repository. Every dependency must carry an Apache-2.0-compatible license (MIT, Apache-2.0, BSD, ISC, Zlib or similar).

The repository is public at [github.com/bluesteelll/moirai](https://github.com/bluesteelll/moirai). Good places to start reading:

- [`docs/ARCHITECTURE-RESEARCH.md`](https://github.com/bluesteelll/moirai/blob/master/docs/ARCHITECTURE-RESEARCH.md): the design of record, with the roadmap and the budgets;
- [`docs/spec/README.md`](https://github.com/bluesteelll/moirai/blob/master/docs/spec/README.md): the specification index (each file's own header gives its current status);
- [`docs/m0/PLAN.md`](https://github.com/bluesteelll/moirai/blob/master/docs/m0/PLAN.md): the plan for milestone M0;
- [`docs/m0/workflow.md`](https://github.com/bluesteelll/moirai/blob/master/docs/m0/workflow.md) and [`AGENTS.md`](https://github.com/bluesteelll/moirai/blob/master/AGENTS.md): how the agents work.

Progress shows up as merges into `master`. The tag `format-v1` will mark the end of M0.
