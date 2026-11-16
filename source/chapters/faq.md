---
title: Questions and answers
nav: FAQ
description: Short answers to the questions people ask first about Moirai, each with a link to the chapter that explains it.
---

# Questions and answers

Short answers to the questions newcomers ask most often. Each answer links to the chapter that explains it in full. Like the rest of this book, the answers describe the designed system: Moirai is not released yet.

## What is Moirai? {#what-is-moirai}

A versioned graph database, written from scratch in Rust, for projects where AI coding agents do much of the work. It keeps a project's tasks, with their subtasks and blockers, together with the project's memory: rules, notes, decisions, questions, review findings, verdicts and measurements. Every agent on the project reads and writes the same store through a command line or an MCP server. The data has its own branches and merges, can be saved into git as readable text, and can point into project files with links that survive moves. See [Why Moirai](why.html) and [The graph](concepts.html).

## Can I use it today? {#can-i-use-it}

No. Nothing is released and nothing can be installed. The project is in milestone M0, "contract and evidence". What exists is the byte-level specification of the on-disk format, a complete reference model in Rust that serves as the test oracle, fixtures, probes and measurement tools. The storage engine is built in M1, after version 1 of the on-disk format is frozen with the git tag `format-v1`, and the `moirai` command-line binary arrives in a later milestone. The command sessions in this book are designed output taken from the specification, not recordings. See [Where it stands now](building.html#status).

## When will it be released? {#release-date}

The project does not publish a release date. Twelve milestones, M0 to M11, lead to the release, and a milestone ends when all of its gates are green; scope is never cut to hold a date. The last milestone adds nothing new: it is the [release gate](glossary.html#term-release-gate), a list of conditions that includes a 72-hour soak test without a lost commit. The size estimates in the plan are checked against the measured pace of work at the ends of M0 and M1. See [Roadmap](building.html#roadmap) and [No release date](building.html#no-date).

## Why not SQLite or an existing database? {#why-not-sqlite}

The project decided to write its own engine and to use no third-party embedded database anywhere: not SQLite, redb, LMDB, sled, RocksDB or the like, and not even as a temporary backend, a test oracle or a benchmark. A dependency lint fails the build if one appears.

The design also asks for an engine shaped around agent work: every record versioned on branches and merged by its type, commits that store small changesets rather than copies of whole pages, read-only memory-mapped files that every process shares through the operating system's page cache, and a lock and flush protocol that many short-lived processes can use without a server. In Moirai these properties are part of the on-disk format itself. The test oracle is a deliberately naive [reference model](glossary.html#term-reference-model) written in Rust. See [Written from scratch](why.html#from-scratch) and [Under the hood: Klotho](engine.html).

## Why not just Markdown files or an issue tracker? {#why-not-markdown}

Nothing checks a Markdown file. The same rule is copied into three files and later edited in one; a task list says a task is done while the plan says it is open; nobody computes which tasks can start. When git branches meet, git merges those files line by line without knowing what a line means, so a resolved item can come back as open. Moirai keeps each fact once, with an id and a type, checks every write, computes which tasks are ready, and merges by type.

Moirai is not meant to replace an issue tracker that people use: it has no web interface and no sync with issue trackers in its first release. It serves the agents inside one repository. It keeps tasks together with the project's memory, versions both on branches that match the agents' lines of work, needs no network, and prints compact output sized for a context window. See [The problem](why.html#the-problem).

## Does it need git? {#does-it-need-git}

No. With no `git` program, no git library and no repository, Moirai still runs every read and write command, branches and merges, undo and revert, the hooks, the MCP server, every query, and export and import of the git image to a local directory or a bundle file. Its version control is its own, and it reads and writes git's file formats with its own code, inside the process. It starts the `git` program only in four optional places, such as pushing the image to a remote, and only when git is installed. Inside a git repository, Moirai keeps its store in git's directory so that every worktree finds it. See [Git is optional](git-image.html#git-optional).

## Are Moirai branches the same as git branches? {#moirai-and-git-branches}

No. Moirai [branches](glossary.html#term-branch) version the graph, and git branches version the code. A Moirai branch never appears, switches or disappears because git did something. You open one explicitly for a line of parallel work, a [lane](glossary.html#term-lane): `moirai lane open` creates the branch, binds a git worktree to it and records both in a lane node. Inside that worktree every `moirai` command reads and writes the lane's branch. When the work is done, Moirai merges the graph and git merges the code. See [Lanes and git worktrees](versions.html#lanes).

## Does it run a server or a daemon? {#server-or-daemon}

No. Moirai is one binary. Every command opens the store, does its work and exits; there is no long-running background process, timer, file watcher or polling loop. Maintenance runs only when a command crosses a threshold: a rollup, for example, runs in a child process that the command starts and that exits when its work is done. The [MCP server](glossary.html#term-mcp-server) is the same binary in a long-running mode that a harness starts for one agent session; to the store it is one more client, and while it is idle it waits on its input and uses no CPU. Many processes share the store through file locks and read-only memory-mapped files. See [Many processes, no server](engine.html#process-model).

## Does it call an LLM or send data anywhere? {#llm-and-network}

No. Moirai's own code never calls a language model or a network service, and it has no telemetry. Work that needs a model, such as capturing decisions at the end of a task or curating records that look alike, is done by the agents in their harness, guided by skills that Moirai ships. The results enter the store as ordinary writes with the status `proposed` and a record of who wrote them, and a model never rewrites a stored fact. The only commands that move data between machines are `moirai image push` and `image pull`, and the transfer itself is done by the ordinary `git` command; a push goes only to remotes that the configuration allows. See [Moirai never calls a model](agents.html#llm-principle).

## Which agents and harnesses does it work with? {#harnesses}

Any [harness](glossary.html#term-harness) with a shell tool or a stdio MCP client. Claude Code and Codex are the first-class targets: they get the full set of hooks, conformance tests and token accounting. Every other harness, such as Copilot, Cursor, Gemini CLI, OpenCode, Zed or Goose, is served through a minimal contract: the `moirai` command on the `PATH`, the stdio MCP server, a short block at the top of `AGENTS.md` and a portable skill. Hooks only save calls and keep agents up to date; without them an agent makes one more call, and it never loses correctness. See [Harnesses](agents.html#harnesses).

## Which operating systems does it support? {#operating-systems}

Windows, Linux and macOS are all part of the design: one on-disk format and one lock and commit protocol for the three. Windows is built and tested first, in every milestone up to the release. The Linux and macOS implementations come in a port phase after the Windows release, which is not yet scheduled; a port may add an implementation but never change the format. Meanwhile, every merge type-checks the code for Linux and macOS, and every dependency must be pure Rust, so a dependency that needs native code fails at once. See [Windows first, Linux and macOS by design](engine.html#cross-platform).

## Where is the data stored? {#where-is-the-data}

In one [store](glossary.html#term-store) per repository, on the local disk. Inside a git repository, `moirai init` creates it in git's common directory, `<git-common-dir>/moirai/`: outside every working tree, so `git add -A` never picks it up, and shared by every worktree of the repository. Outside git it is a `.moirai/` directory. The store must be on a local file system (on Windows, NTFS); network drives and cloud-synced folders are refused. To keep a copy elsewhere or move data to another machine, you export the [git image](glossary.html#term-git-image), and `moirai backup` makes a copy of the store itself. See [Where the store lives](engine.html#store-location).

## How does it stay fast and small? {#fast-and-small}

The current state lives in files that are never changed after they are written, and every process maps them read-only, so the operating system keeps one copy of the hot data in its page cache for all agents on the machine. No daemon holds the graph in memory. Opening a store reads a small header file and replays a short, bounded part of the log, so it costs the same however long the history is. Readers take no locks; a writer holds the single writer lock only to append its commit, and one disk flush can cover many writers.

The design targets are at most 5 ms of engine time for a read command such as `moirai ready` on a store of 100,000 nodes, at most 4 MB of private memory for a read command or an ordinary write, and zero CPU when idle. They are targets with gates that enforce them, not measurements of a finished product. See [Budgets](engine.html#budgets).

## What happens when two agents write at once? {#concurrent-writes}

On the same branch, writes take turns at the store's writer lock, which a plain write holds only for tens of microseconds, and neither agent fails because the other committed first. Each write is one commit, all or nothing.

To keep two agents off the same task, an agent claims a task before it works on it. The claim creates a [lease](glossary.html#term-lease) that every branch sees. A [fencing token](glossary.html#term-fencing-token) makes sure that an agent whose lease was lost cannot overwrite the work of the agent that took over, and if an agent disappears, its lease runs out instead of holding the task forever. A write can also carry a [guard](glossary.html#term-cas-guard), such as "only if the status is still `in_progress`"; if another agent changed the node first, nothing is written and the error shows the current value. Agents on different branches work in isolation, and a typed merge brings their work together later. See [Leases and claims](concepts.html#leases) and [A write that lost the race](tour.html#guarded-write).

## What happens when a file moves? {#file-moves}

A [file link](glossary.html#term-file-link) is anchored on the file's identity and on quoted text with its context, never on a bare line number. When a linked file is renamed, moved or edited, the next read finds it again by evidence, such as the operating system's file id or a rename inside one git commit, and shows its [link state](glossary.html#term-link-state), for example `moved-auto`. The new path enters the history only at a settle point, and only on exact evidence. When the evidence is weaker, Moirai proposes a candidate and waits for a decision, and a missing file is never treated as deleted. A link never points at the wrong file or text without saying so. See [File links that survive moves](file-links.html).

## Why its own query language? {#own-query-language}

So that commands and queries can never disagree. Every read command, such as `moirai ready`, is a [named query](glossary.html#term-named-query) in Lachesis, and every write command a named mutation, so one parser and one planner stand behind the whole interface, and "ready" or "blocked" has exactly one definition. Lachesis looks like Cypher because language models have seen far more Cypher than GQL, the newer graph query standard, and it also accepts common GQL spellings. It adds what Moirai needs: the engine's own state as built-ins (`t.ready`, `blockers()`), versions inside a query (`USE main~5`), file-link states, and guarded writes in which one `TX` block is one commit. Its errors are written for models and name one fix each, and a benchmark, LQ-Bench, must show that models write it correctly before the language is frozen. See [Lachesis, the query language](lachesis.html).

## How do agents get context without huge prompts? {#context}

Moirai computes it. At the start of a session an agent gets a [brief](glossary.html#term-brief), a digest of the store within 8,000 bytes by default. Before a task it asks for a [context pack](glossary.html#term-context-pack): the rules for its role, the task with its acceptance criteria, the relevant plan sections, findings and measurements, fitted into a byte budget per role. Entries are shortened before any is dropped, and when something is dropped, the first and the last line say how much and which command fetches the rest. Output is compact everywhere: ids first, one line per record, and short ids such as `#812` instead of long random ones. See [Context packs and briefs](agents.html#packs-and-briefs) and [How tokens are saved](agents.html#tokens).

## How is correctness checked? {#correctness}

Mostly by trying hard to break it. A deliberately naive [reference model](glossary.html#term-reference-model) of all of Moirai's logic, written in Rust before the engine, is the oracle: from M1 on, the engine is compared with it command by command, including commit ids. The storage code runs on a simulated file system that injects faults; a crash is injected at every write, flush and publish point, and recovery must keep every acknowledged commit. Real processes are killed in loops on Windows, every parser is fuzzed, and mutation testing must show that the tests catch at least 90 % of small deliberate bugs. Once a gate is mandatory, it stays mandatory.

One gap is stated openly: a test rig that cuts the power of a real machine runs only after the release, so surviving a real power loss remains an unverified risk until then. See [The test workstream](building.html#test-workstream) and [Crash safety](engine.html#crash-safety).

## Who builds it, and how? {#who-builds-it}

AI coding agents (Claude Code sessions) write it, following a written procedure that separates duties. Work is split into small [units](glossary.html#term-unit), each carried out by separate agents in roles such as developer, tester and code reviewer. The agent that makes a change never declares it correct. Every behavioural claim is first shown failing without the change ([red-first](glossary.html#term-red-first)). Only one orchestrating session merges, after a full gate in a neutral worktree. Tests and CI use synthetic data only. See [Built by AI agents under a strict process](building.html#agent-process).

## What license does it use? {#license}

Apache-2.0. Every dependency must carry an Apache-2.0-compatible license, such as MIT, BSD, ISC or Zlib, and must be pure Rust. The source code, the design documents and the specification are public at [github.com/bluesteelll/moirai](https://github.com/bluesteelll/moirai). See [License and how to follow the project](building.html#license-and-follow).
