---
title: Questions and answers
nav: FAQ
description: Short answers to the questions people ask first about Moirai, each with a link to the chapter that explains it.
---

# Questions and answers

Short answers to the questions newcomers ask most often. Each answer links to the chapter that explains it in full.

## What is Moirai? {#what-is-moirai}

A versioned graph database, written from scratch in Rust, for projects where AI coding agents do much of the work. It keeps a project's tasks together with its memory (rules, notes, decisions, findings and measurements) in one store that every agent reads and writes through a command line or an MCP server. See [Why Moirai](why.html) and [The graph](concepts.html).

## Can I use it today? {#can-i-use-it}

No. Nothing is released and nothing can be installed. The project is in milestone M0, writing and testing its specification; the storage engine is built in M1, after version 1 of the on-disk format is frozen. See [Where it stands now](building.html#status).

## When will it be released? {#release-date}

The project does not publish a release date. Twelve milestones, M0 to M11, lead to the release; a milestone ends when all of its gates are green, and scope is never cut to hold a date. The last milestone, M11, adds nothing new and ends at the [release gate](glossary.html#term-release-gate), which includes a 72-hour soak test without a lost commit. See [Roadmap](building.html#roadmap) and [No release date](building.html#no-date).

## Why not SQLite or an existing database? {#why-not-sqlite}

The project decided to write its own engine and to use no third-party embedded database anywhere, not even as a test oracle or a benchmark; a dependency lint fails the build if one appears. An engine of its own can build what agent work needs into the on-disk format itself: records versioned and merged by type, and many short-lived processes sharing one store without a server. See [Written from scratch](why.html#from-scratch) and [Under the hood: Klotho](engine.html).

## Why not just Markdown files or an issue tracker? {#why-not-markdown}

Nothing checks a Markdown file: copies of a rule drift apart, nothing computes which tasks can start, and git merges such files line by line, so a resolved item can come back as open. Moirai keeps each fact once, with an id and a type, checks every write and merges by type. It does not replace an issue tracker that people use: it serves the agents inside one repository and has no web interface in its first release. See [The problem](why.html#the-problem).

## Does it need git? {#does-it-need-git}

No. Without git, Moirai still runs every read and write command, branches and merges, and the export of the git image to a directory or a bundle file, because it reads and writes git's formats with its own code. It starts the `git` program only in four optional places, such as pushing the image to a remote. See [Git is optional](git-image.html#git-optional).

## Are Moirai branches the same as git branches? {#moirai-and-git-branches}

No. Moirai [branches](glossary.html#term-branch) version the graph, and git branches version the code; a Moirai branch is never created, switched or deleted because git did something. You open one explicitly for a [lane](glossary.html#term-lane) with `moirai lane open`, which binds a git worktree to it. When the work is done, Moirai merges the graph and git merges the code. See [Lanes and git worktrees](versions.html#lanes).

## Does it run a server or a daemon? {#server-or-daemon}

No. Moirai is one binary: every command opens the store, does its work and exits, and there is no long-running background process, timer or file watcher. The [MCP server](glossary.html#term-mcp-server) is the same binary in a long-running mode for one agent session, and while it is idle it uses no CPU. See [Many processes, no server](engine.html#process-model).

## Does it call an LLM or send data anywhere? {#llm-and-network}

No. Moirai's own code never calls a language model or a network service, and it has no telemetry. Work that needs a model is done by the agents in their harness, guided by skills that Moirai ships, and enters the store as ordinary writes with the status `proposed`. The only commands that move data between machines are `moirai image push` and `image pull`; the `git` command does the transfer, and a push goes only to remotes that the configuration allows. See [Moirai never calls a model](agents.html#llm-principle).

## Which agents and harnesses does it work with? {#harnesses}

Any [harness](glossary.html#term-harness) with a shell tool or a stdio MCP client. Claude Code and Codex are the first-class targets, with the full set of hooks; every other harness needs only the `moirai` command, the MCP server, a short block in `AGENTS.md` and a portable skill. Without hooks an agent makes one more call, but it never loses correctness. See [Harnesses](agents.html#harnesses).

## Which operating systems does it support? {#operating-systems}

Windows, Linux and macOS are all part of the design, with one on-disk format and one lock and commit protocol for all three. Windows is built and tested first. Linux and macOS come in a port phase after the Windows release, and every merge already type-checks the code for them. See [Windows first, Linux and macOS by design](engine.html#cross-platform).

## Where is the data stored? {#where-is-the-data}

In one [store](glossary.html#term-store) per repository, on the local disk. Inside a git repository it is in git's common directory, `<git-common-dir>/moirai/`, shared by every worktree and never picked up by `git add -A`; outside git it is a `.moirai/` directory. Network drives and cloud-synced folders are refused. See [Where the store lives](engine.html#store-location).

## How does it stay fast and small? {#fast-and-small}

Its current state lives in files that are never changed after they are written and that every process maps read-only, so the operating system keeps one copy of the hot data for all agents, and no daemon holds the graph in memory. The design targets, enforced by gates, include at most 5 ms of engine time and 4 MB of private memory for a read command. See [Budgets](engine.html#budgets).

## What happens when two agents write at once? {#concurrent-writes}

On the same branch, writes take turns at the writer lock, which a plain write holds for tens of microseconds, and each write is one all-or-nothing commit. To keep two agents off one task, an agent first claims it, which creates a [lease](glossary.html#term-lease) that every branch sees; a write can also carry a [guard](glossary.html#term-cas-guard), a condition that makes it write nothing if another agent changed the node first. See [Leases and claims](concepts.html#leases) and [A write that lost the race](tour.html#guarded-write).

## What happens when a file moves? {#file-moves}

A [file link](glossary.html#term-file-link) is anchored on the file's identity and on quoted text with its context, never on a bare line number. After a rename, move or edit, the next read finds the file again by evidence and shows its [link state](glossary.html#term-link-state), such as `moved-auto`; when the evidence is weak, Moirai proposes a candidate and waits for a decision. See [File links that survive moves](file-links.html).

## Why its own query language? {#own-query-language}

So that commands and queries can never disagree: every read command, such as `moirai ready`, is a [named query](glossary.html#term-named-query) in Lachesis, so "ready" has exactly one definition. Lachesis looks like Cypher, which language models know well, adds what Moirai needs, such as versions inside a query and guarded writes, and has errors written for models. See [Lachesis, the query language](lachesis.html).

## How do agents get context without huge prompts? {#context}

Moirai computes it. At the start of a session an agent gets a [brief](glossary.html#term-brief), a digest of the store within 8,000 bytes by default. Before a task it asks for a [context pack](glossary.html#term-context-pack): the rules, the task and the related records for its role, fitted into a byte budget, with a note of what was left out. See [Context packs and briefs](agents.html#packs-and-briefs) and [How tokens are saved](agents.html#tokens).

## How is correctness checked? {#correctness}

Mostly by trying hard to break it. The engine is compared command by command with a naive [reference model](glossary.html#term-reference-model) written in Rust before it, crashes are injected at every write, flush and publish point, and mutation testing must catch at least 90 % of small deliberate bugs. One gap is open: a real power-loss test runs only after the release. See [The test workstream](building.html#test-workstream) and [Crash safety](engine.html#crash-safety).

## Who builds it, and how? {#who-builds-it}

AI coding agents (Claude Code sessions) write it, following a written procedure that separates duties: different agents act as developer, tester and code reviewer, and the agent that makes a change never declares it correct. Every behavioural claim is first shown failing without the change ([red-first](glossary.html#term-red-first)). See [Built by AI agents under a strict process](building.html#agent-process).

## What license does it use? {#license}

Apache-2.0. Every dependency must carry an Apache-2.0-compatible license, such as MIT, BSD, ISC or Zlib, and must be pure Rust. The source code, the design documents and the specification are public at [github.com/bluesteelll/moirai](https://github.com/bluesteelll/moirai). See [License and how to follow the project](building.html#license-and-follow).
