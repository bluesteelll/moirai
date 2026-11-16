---
title: Why Moirai
nav: Why Moirai
description: The problem Moirai solves for projects where AI coding agents do much of the work, and the idea, requirements, priorities and limits of its design.
---

# Why Moirai

AI coding agents change code quickly, but they forget what they learned when a session ends, and they keep a project's plans, tasks and rules in text files that no tool understands. Moirai is a small database built for that gap: one typed graph that holds a project's work and its memory, versioned like git and shared by every agent that works on the project.

> [!NOTE]
> **Status**
>
> Moirai is not released, and you cannot install it yet. The project is in its first milestone, "contract and evidence": the specification of the on-disk format, a complete reference model in Rust that serves as the test oracle, test fixtures and measurement tools. The storage engine is built in the next milestone, after the on-disk format is frozen. This book describes the designed system. The [chapter on how Moirai is built](building.html) has the plan.

## The problem {#the-problem}

Picture a repository where AI coding agents do most of the work. A human, or an orchestrating agent, splits the work into tasks. Several agents work at the same time, each in its own git worktree on its own branch; this book calls such a line of work a *lane*. Reviewers write findings, testers record measurements, and the human in charge makes decisions and sets rules. Five problems appear quickly.

### Agents forget between sessions {#agents-forget}

Every session starts with an empty context window. Whatever an agent learned (a rule, a trap in the build, the reason a design was rejected) is lost unless someone wrote it down and it is loaded again next time. A memory file that is loaded whole into every session grows until it costs more than it helps.

### Project state lives in Markdown, and Markdown drifts {#markdown-drifts}

Plans, task lists, rules, decision logs and open questions usually live in Markdown files in the repository. Text is easy to write, but nothing checks it. The same rule is copied into three files and later edited in only one. A correction is added below the old text, so the old number is still quoted. A task list says a task is done while the plan says it is open. And nothing computes which tasks can start now: someone has to read the files and decide.

### Branches and worktrees make the copies diverge {#branches-diverge}

Every git branch carries its own copy of those files. When branches meet, git merges them line by line, without knowing what a line means. A rule decided on one branch never reaches the others. A merge of two task lists can turn a resolved item back into an open one. A count written in prose no longer matches the real count after the merge.

Parallel lanes also step on each other. Two lanes start the same task. A task finished on one branch is handed out again on another, because that branch has not seen the completion yet. Which lane owns which files is agreed in prose, if at all.

### References to files rot {#references-rot}

A note that says "see `src/net/pool.rs`, line 120" is wrong as soon as someone edits above that line, renames the file or moves its directory. Agents move and rename files all the time. Line numbers in old notes and plans quietly stop pointing at the right place.

### Context is expensive, so briefs grow {#context-costs}

Every byte loaded into an agent's context costs tokens and attention. So whoever dispatches agents writes a brief for each one by hand: the rules that apply, an excerpt of the plan, the open findings. Briefs grow with the project, get cut at arbitrary points, and silently lose the line that mattered.

### What Moirai does instead {#what-moirai-changes}

| What goes wrong today | What Moirai does |
|---|---|
| Knowledge is lost between sessions. | Rules, notes, decisions and findings are typed records in one store. Each session starts with a short, budgeted brief built from them. |
| Text files drift and duplicate each other. | Every fact has one place and an id. The engine checks every write against the schema and its invariants. |
| Nobody computes what can start. | The engine computes which tasks are ready and which are blocked, from subtasks and blockers. |
| Merges of text lose meaning. | Moirai has its own branches and a typed merge: statuses, counters, sets and text each merge by their own rule, and a real conflict is kept as data until someone resolves it. |
| Two lanes take the same task. | A claim on a task is visible from every branch, and a task completed or deleted on one branch is never handed out again from another. |
| File references rot. | File links are anchored on quoted text and its context, never on a bare line number, and follow a file through moves and renames. |
| Briefs are written by hand. | A context pack is built for one task and one role under a byte budget, and states what it left out and how to get it. |

## The idea {#the-idea}

Moirai puts all of this into one small, typed graph. A [node](glossary.html#term-node) is one record, such as a task or a rule; an edge is a typed connection between two nodes. The graph holds three things.

- **Work.** Tasks, with subtasks (a `parent` edge) and blockers (a `blocks` edge: A must be done before B starts). The engine computes which tasks are ready to start; nobody maintains that list by hand.
- **Memory.** Notes, rules, decisions, questions, review findings, verdicts of reviews and tests, measurements with their environment, and documents such as plans. Each has typed fields and a status.
- **Links to files.** A node can point at a project file, or at a place inside it: a heading, a function, a quoted passage. The link survives renames, moves and edits.

Around the graph Moirai provides:

- **Version control like git.** Branches, commits, merges, tags, history, revert and undo, for every piece of data, with a merge that knows what each field means. See [the chapter on versions](versions.html).
- **A git image.** The store can be written into a git repository as text, one file per node, and read back. The data can travel through any git remote and be browsed in any git tool, but git is not needed to run Moirai. See [the chapter on the git image](git-image.html).
- **A query language.** [Lachesis](lachesis.html) (LQ) is a query language in the style of Cypher. Every read command is a named query in it, and most write commands are named mutations, so one parser and one planner stand behind both. Commands that act on branches and history, such as a merge, stay outside the language.
- **Interfaces for agents.** A command-line tool, `moirai`; an MCP server with ten tools for agents that have no shell; skills, which are short instruction files that teach an agent how to use Moirai; and optional hooks that give an agent a brief when its session starts and a short list of relevant changes with each prompt. See [the chapter on agents](agents.html).

The last piece replaces the hand-written brief. An agent asks for a *context pack* for its task and role. Moirai collects the rules that apply to that role and those files, the task itself with its acceptance criteria, the plan sections it implements, the findings about it and the relevant measurements, and fits them into a byte budget. When something does not fit, the first and the last line of the pack say how much was dropped and which command fetches the rest. Nothing is cut silently.

The [picture on the home page](index.html#big-picture) shows where Moirai sits between the agents, the project files and git.

Three names recur in this book. **Moirai** is the product and `moirai` its single command. **Klotho** is the storage engine, a library inside that one binary. **Lachesis** is the query language. Inside a git repository the store lives in a `moirai` directory under git's common directory (`<git-common-dir>/moirai/`), outside every working tree, so all worktrees of the repository share one store without extra setup. Outside a git repository it is a `.moirai/` directory.

## The requirements {#requirements}

The project started from a short brief: a graph database written from scratch in Rust, with typed fields, subtasks and blockers, references that stay consistent when something is deleted, a command-line tool, an MCP server and skills, high speed and low memory use. Five requirements were added on top of it, and they shape most of the design.

| # | Requirement | In plain words |
|---|---|---|
| 1 | Branches that merge | All data (tasks, statuses, blockers, rules, notes, decisions, findings, measurements, even the schema) lives on branches that can be created, compared and merged, with history, tags, revert, cherry-pick, a reflog and undo. |
| 2 | Its own version control | The version control is Moirai's own and does not depend on git. The core never starts a git process and never links a git library. Moirai branches have their own life; they are not created automatically for git branches or worktrees. |
| 3 | A git-compatible image | Moirai can save its branches and history into a git repository as deterministic, human-readable text, and load them back. That is how the data is backed up, shared and reviewed with ordinary git tools. |
| 4 | File links that survive moves | A node can link to a project file or a place inside it, and the link survives renames, moves, edits and deletions, whoever makes them. See [the chapter on file links](file-links.html). |
| 5 | Its own query language | Every question an agent can ask is a query in Lachesis, including every built-in read command. |

One more requirement from the brief deserves a name of its own: **"when a node is deleted, everyone knows"**. Deleting a task, a rule or a file node updates everything that referenced it in the same commit, and nothing that depended on it is silently unblocked. [The graph](concepts.html#deletion) explains how.

## Four priorities {#priorities}

The design judges every choice on four priorities: **speed, minimal RAM, correctness and minimal agent tokens**. Each priority has budgets (numeric targets) and gates (tests that fail the build when a target is missed). The numbers below are design targets, not measured results.

Speed
: Agents call Moirai many times per task, so each call must cost almost nothing next to the agent's own work. A command opens the store in about a millisecond, because opening costs the same whatever the size of the history. The target for the engine time of a read command such as `moirai ready` is at most 5 ms on a store of 100,000 nodes.

Minimal RAM
: A developer machine running many agent sessions has little memory to spare. Data lives in read-only memory-mapped files whose pages every Moirai process shares, and no daemon holds the graph in memory. The target is at most 4 MB of private memory for one command-line or hook process.

Correctness
: A task tracker that loses an acknowledged "done" under load is worse than none. Crash safety is part of the on-disk format: a write is acknowledged only after it is durable on disk. Every invariant the engine enforces has a gate, and a separate, deliberately naive reference model written in Rust serves as the oracle: the engine's answers must match the model's.

Minimal agent tokens
: Whatever Moirai prints ends up in an agent's context window. Ids are short numbers such as `#812`, not long UUIDs. Output is compact, one line per record, with the ids first. Packs and briefs have byte budgets; the default brief at session start is at most 8,000 bytes, and the change list added to a prompt is at most 600 bytes. The MCP tool descriptions are loaded only when an agent needs them, and each skill has a size limit; the core skill targets at most 800 tokens.

## Design principles {#principles}

### Written from scratch {#from-scratch}

Moirai does not use SQLite or any other third-party embedded database, anywhere: not as the storage backend, not as a temporary stepping stone, not as a test oracle and not as a benchmark. The log, the storage format, the graph structures, the merge engine, the git object layer and the query language are all written for this project. Dependencies are limited to small leaf libraries such as hashing, compression, operating-system bindings and JSON, plus an MCP protocol library and its async runtime that only the MCP server uses, and every one of them must be pure Rust under an Apache-2.0-compatible license.

### One binary, no daemon, zero idle CPU {#no-daemon}

Moirai is one static binary. A command opens the store, does its work and exits. There is no server to start, no background process, no timer, no file watcher and no polling. The MCP server is an ordinary client of the store that lives as long as the agent session that started it; when it is idle it waits on its input and uses no CPU. Maintenance runs only when a command or request crosses a threshold, and stops when that work is done.

### Built once, to the final specification {#final-spec}

Every component is built in dependency order, once, to its final specification, and passes its full gates before anything is built on top of it. There are no interim versions, temporary modes or throwaway prototypes in the product. That is why the on-disk format is specified and frozen before the first line of the engine is written.

### Any agent harness {#any-harness}

An agent [harness](glossary.html#term-harness) is the program that runs an AI coding agent, such as Claude Code or Codex. Moirai requires very little from a harness: the `moirai` command on the `PATH`, a stdio MCP server, a short block of instructions at the top of `AGENTS.md` and a portable skill. Claude Code and Codex get deeper integration through hooks, but every hook is an accelerator with a fallback: without it an agent loses some freshness or makes one more call, never correctness. Rights come from the [leases](concepts.html#leases) an agent presents, not from a hook or a harness variable.

### No LLM and no network inside {#no-llm}

Moirai never calls a language model, and its own code never makes a network call. It provides only typed, validated, versioned and crash-safe commands. The work that needs intelligence, such as capturing decisions and rules at the end of a task, harvesting old session transcripts, or merging similar records, is done by the agents themselves, guided by skills that Moirai ships. Their results enter the graph as ordinary writes with the status `proposed` and a record of who wrote them, where a human or the orchestrator can review them. No model ever rewrites a stored fact. When you push the git image to a remote, the transport is the ordinary `git` command.

### Windows, Linux and macOS {#three-systems}

One on-disk format and one protocol are designed for all three operating systems. Windows is built and tested first. A type check of the whole workspace for Linux and macOS is a gate from the start, which is one reason every dependency must be pure Rust.

## What Moirai deliberately is not {#not}

A short list of things the design rules out, each for a stated reason.

- **Not a server.** No daemon, no auto-started background service, no telemetry, no network calls.
- **Not built on another database, and not built on git.** Git's object store is not the live store, and no file in the working tree is the source of truth. The git image is an export, read back explicitly.
- **Not a mirror of your git branches.** Moirai branches are opened explicitly for lanes; they do not appear and disappear with git branches and worktrees.
- **Not a merge that never fails.** Moirai does not hide conflicts. A real disagreement between two branches becomes a conflict value that someone resolves.
- **No soft delete as a status, no hierarchical or prefixed ids, no UUIDs shown to agents.**
- **Not a vector store.** No embeddings and no vector index in the core.
- **Not an archive.** It does not store session transcripts, journals, build artifacts or code-symbol indexes, and it does not store secrets: content that looks like a credential is rejected on write.
- **Not an LLM tool.** No language model call, and no model rewriting stored facts.
- **Not a policy enforcer.** Moirai answers queries; it does not block an agent's tool calls.
- **Not hands-on with your files.** No file watcher, no ids written into source code, no rewriting of path mentions in your documents. Files move only when you ask Moirai to move them.
- **No silent format migration.** Opening a store never migrates its on-disk format. A reader refuses a newer format, and migration is an explicit command.
- **No human interface and no sync with issue trackers** in the first release, and no MCP over HTTP.

## Who it is for {#who}

Moirai is designed for a developer or a small team that runs AI coding agents on a real code base, especially when several agents work at once. The design grew from one such workflow: an orchestrating session splits work into tasks and dispatches role agents (an architect, a critic, developers, a code reviewer, a tester) into parallel lanes, each lane a git worktree. The node kinds mirror that workflow: findings that survive review rounds, verdicts of each role, measurements tied to the commit and machine they were taken on, and rules that reach every lane.

A single agent on a single branch gains something too: a memory that survives sessions, a task list that computes what is ready, and file links that do not rot.

All agents working on one repository on one machine share that repository's one store. Data moves between machines through the git image. Moirai is a poor fit if you want a web dashboard for humans, a hosted service, or a two-way sync with an issue tracker; none of these is planned for the first release.

## Where to go next {#next}

- [The graph](concepts.html): nodes, kinds, statuses, edges, derived state, deletion and leases.
- [A tour](tour.html) of a designed session, command by command.
- [Branches, merges and history](versions.html): how lanes branch and merge.
- The full design, for readers who want depth: [docs/ARCHITECTURE-RESEARCH.md](https://github.com/bluesteelll/moirai/blob/master/docs/ARCHITECTURE-RESEARCH.md) on GitHub.
