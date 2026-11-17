---
title: Moirai
nav: Home
description: Moirai is a versioned graph database, written from scratch in Rust, that gives AI coding agents one place for a project's tasks, memory, rules and decisions.
---

# Moirai

Moirai is a versioned graph database, written from scratch in Rust, for projects where AI coding agents do much of the work. It gives every agent on a project one place for the tasks, the project memory, the rules and the decisions, versioned with branches like git and shared by all agents at once. It is one program, `moirai`, with no server. It is not released yet: the project is still writing and testing its specification, and nothing can be installed.

This book explains Moirai to people who have never seen the project. It assumes that you know git, databases in general and AI coding agents such as Claude Code or Codex, but nothing of the project's own vocabulary. New terms are explained where they first appear, and the main ones are collected in the [glossary](glossary.html).

## What it does {#what-it-does}

Moirai keeps a project's work and its memory in one typed graph. A task, a question, a rule, a decision, a note, a review finding and a source file are all [nodes](glossary.html#term-node), and the links between them are [edges](concepts.html#edges). The core schema has 25 kinds of edge, and the order of work is only part of them: a subtask belongs to a task, one task must finish before another starts, a decision answers a question, a new rule supersedes an old one, a review finding is about the code that was reviewed, a note is anchored to a place in a source file. The engine checks every write against the schema and computes what follows from the graph, such as which tasks can start now, so nobody keeps such lists by hand. To get answers out of the graph, Moirai has its own query language, [Lachesis](lachesis.html). One query follows several links at once, so an agent does not have to dig through files and notes by hand: before editing a file, for example, a single query returns every rule and decision that concerns it.

Around the graph, Moirai adds what agents working in parallel need: [leases](glossary.html#term-lease) so that two agents never take the same task, branches that merge field by field, a readable copy in git, links into project files that survive moves, and a query language that every read command is built on. Each card opens the chapter that explains one part.

<div class="cards">
<a class="card" href="why.html"><span class="card-title">Why Moirai</span><span class="card-text">Agents forget between sessions, plans in Markdown drift, file references rot. The problem and the idea.</span></a>
<a class="card" href="concepts.html"><span class="card-title">The graph</span><span class="card-text">Tasks, rules, decisions and findings as typed nodes. The engine computes which tasks are ready.</span></a>
<a class="card" href="tour.html"><span class="card-title">A tour</span><span class="card-text">One designed session, from <code>moirai init</code> to a merged parallel lane.</span></a>
<a class="card" href="versions.html"><span class="card-title">Branches and merges</span><span class="card-text">Its own version control: a branch per lane, a typed merge, conflicts kept as data.</span></a>
<a class="card" href="git-image.html"><span class="card-title">Saving into git</span><span class="card-text">The whole graph as readable text files in a git repository, and back again.</span></a>
<a class="card" href="file-links.html"><span class="card-title">File links</span><span class="card-text">Pointers into project files that follow renames, moves and edits.</span></a>
<a class="card" href="lachesis.html"><span class="card-title">Lachesis</span><span class="card-text">A query language in the style of Cypher, behind every read of the graph and most writes.</span></a>
<a class="card" href="agents.html"><span class="card-title">Working with agents</span><span class="card-text">A CLI, an MCP server, skills and hooks, with context sized for each task.</span></a>
</div>

## A first look {#first-look}

Two commands from the [tour](tour.html), in a small fictional project. The first asks which tasks an agent may start now. The second lets an agent called `dev-1` take one of them.

```console
$ moirai ready
branch: main | rev 5 | live | 1 row
#2 task open P2 "Parse the Connection header" parent:#1
$ moirai claim 2 --agent dev-1 --start
branch: main | rev 5 -> 6 | committed c41a7d0e2
1 CALL tx.claim([#2]): lease L-2 holder dev-1, expires 2026-11-16T09:55Z; status open->in_progress
affected: none
```

*Designed output.*

The first line of each result above names the branch and the revision it was read from; a write shows the revision before and after. Every record takes one line and starts with its short id. The engine worked out the answer to `ready` from the task tree, the blockers and the leases. The claim gives `dev-1` a lease on task `#2`, which every branch of the store sees, so no other agent is handed the same work.

## The big picture {#big-picture}

<figure class="diagram">
<svg viewBox="0 0 720 564" role="img" aria-labelledby="index-d1-t">
  <title id="index-d1-t">The big picture: agents in their harnesses use the moirai binary through the CLI, the MCP server and hooks; Lachesis and the Klotho engine sit inside it; the engine works with project files, the store and a git image that travels to a git remote</title>
  <defs><marker id="index-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path class="d-arrow" d="M0,0 L10,5 L0,10 z"/></marker></defs>
  <path class="d-box-muted" d="M24,12 H696 A8,8 0 0 1 704,20 V84 A8,8 0 0 1 696,92 H24 A8,8 0 0 1 16,84 V20 A8,8 0 0 1 24,12 Z"/>
  <text class="d-text-small" x="32" y="27" text-anchor="start" dominant-baseline="middle">AI coding agents in their harnesses, with skills that teach them the commands</text>
  <path class="d-box" d="M48,40 H208 A8,8 0 0 1 216,48 V72 A8,8 0 0 1 208,80 H48 A8,8 0 0 1 40,72 V48 A8,8 0 0 1 48,40 Z"/>
  <text class="d-text" x="128" y="60" text-anchor="middle" dominant-baseline="middle">Claude Code</text>
  <path class="d-box" d="M280,40 H440 A8,8 0 0 1 448,48 V72 A8,8 0 0 1 440,80 H280 A8,8 0 0 1 272,72 V48 A8,8 0 0 1 280,40 Z"/>
  <text class="d-text" x="360" y="60" text-anchor="middle" dominant-baseline="middle">Codex</text>
  <path class="d-box" d="M512,40 H672 A8,8 0 0 1 680,48 V72 A8,8 0 0 1 672,80 H512 A8,8 0 0 1 504,72 V48 A8,8 0 0 1 512,40 Z"/>
  <text class="d-text" x="592" y="60" text-anchor="middle" dominant-baseline="middle">other harnesses</text>
  <path class="d-edge" d="M128,80 L128,124" marker-end="url(#index-arrow)"/>
  <path class="d-edge" d="M360,80 L360,124" marker-end="url(#index-arrow)"/>
  <path class="d-edge" d="M592,80 L592,124" marker-end="url(#index-arrow)"/>
  <path class="d-box-muted" d="M24,124 H696 A8,8 0 0 1 704,132 V364 A8,8 0 0 1 696,372 H24 A8,8 0 0 1 16,364 V132 A8,8 0 0 1 24,124 Z"/>
  <text class="d-text-strong" x="32" y="143" text-anchor="start" dominant-baseline="middle">moirai: one binary, no daemon</text>
  <path class="d-box" d="M48,160 H208 A8,8 0 0 1 216,168 V208 A8,8 0 0 1 208,216 H48 A8,8 0 0 1 40,208 V168 A8,8 0 0 1 48,160 Z"/>
  <text class="d-text" x="128" y="181" text-anchor="middle" dominant-baseline="middle">CLI</text>
  <text class="d-text-small" x="128" y="201" text-anchor="middle" dominant-baseline="middle">moirai &lt;verb&gt;</text>
  <path class="d-box" d="M280,160 H440 A8,8 0 0 1 448,168 V208 A8,8 0 0 1 440,216 H280 A8,8 0 0 1 272,208 V168 A8,8 0 0 1 280,160 Z"/>
  <text class="d-text" x="360" y="181" text-anchor="middle" dominant-baseline="middle">MCP server</text>
  <text class="d-text-small" x="360" y="201" text-anchor="middle" dominant-baseline="middle">stdio, ten tools</text>
  <path class="d-box" d="M512,160 H672 A8,8 0 0 1 680,168 V208 A8,8 0 0 1 672,216 H512 A8,8 0 0 1 504,208 V168 A8,8 0 0 1 512,160 Z"/>
  <text class="d-text" x="592" y="181" text-anchor="middle" dominant-baseline="middle">Hooks (optional)</text>
  <text class="d-text-small" x="592" y="201" text-anchor="middle" dominant-baseline="middle">fail-open</text>
  <path class="d-edge" d="M128,216 L128,240" marker-end="url(#index-arrow)"/>
  <path class="d-edge" d="M360,216 L360,240" marker-end="url(#index-arrow)"/>
  <path class="d-edge" d="M592,216 L592,240" marker-end="url(#index-arrow)"/>
  <path class="d-box" d="M48,240 H672 A8,8 0 0 1 680,248 V272 A8,8 0 0 1 672,280 H48 A8,8 0 0 1 40,272 V248 A8,8 0 0 1 48,240 Z"/>
  <text class="d-text" x="360" y="260" text-anchor="middle" dominant-baseline="middle">Lachesis: queries and mutations of the graph</text>
  <path class="d-edge" d="M360,280 L360,304" marker-end="url(#index-arrow)"/>
  <path class="d-box-accent" d="M48,304 H672 A8,8 0 0 1 680,312 V348 A8,8 0 0 1 672,356 H48 A8,8 0 0 1 40,348 V312 A8,8 0 0 1 48,304 Z"/>
  <text class="d-text-strong" x="360" y="323" text-anchor="middle" dominant-baseline="middle">Klotho: the storage engine</text>
  <text class="d-text-small" x="360" y="343" text-anchor="middle" dominant-baseline="middle">log, segments, locks, branches and history</text>
  <path class="d-edge" d="M136,356 L136,412" marker-end="url(#index-arrow)"/>
  <path class="d-edge" d="M360,356 L360,412" marker-start="url(#index-arrow)" marker-end="url(#index-arrow)"/>
  <path class="d-edge-dashed" d="M584,356 L584,412" marker-start="url(#index-arrow)" marker-end="url(#index-arrow)"/>
  <text class="d-text-small" x="146" y="392" text-anchor="start" dominant-baseline="middle">file links</text>
  <text class="d-text-small" x="370" y="392" text-anchor="start" dominant-baseline="middle">log and segments</text>
  <text class="d-text-small" x="594" y="392" text-anchor="start" dominant-baseline="middle">export / import</text>
  <path class="d-box" d="M48,412 H224 A8,8 0 0 1 232,420 V460 A8,8 0 0 1 224,468 H48 A8,8 0 0 1 40,460 V420 A8,8 0 0 1 48,412 Z"/>
  <text class="d-text" x="136" y="432" text-anchor="middle" dominant-baseline="middle">project files</text>
  <text class="d-text-small" x="136" y="452" text-anchor="middle" dominant-baseline="middle">in each worktree</text>
  <path class="d-box" d="M272,412 H448 A8,8 0 0 1 456,420 V460 A8,8 0 0 1 448,468 H272 A8,8 0 0 1 264,460 V420 A8,8 0 0 1 272,412 Z"/>
  <text class="d-text" x="360" y="432" text-anchor="middle" dominant-baseline="middle">store directory</text>
  <text class="d-text-small" x="360" y="452" text-anchor="middle" dominant-baseline="middle">&lt;git-common-dir&gt;/moirai/</text>
  <path class="d-box" d="M496,412 H672 A8,8 0 0 1 680,420 V460 A8,8 0 0 1 672,468 H496 A8,8 0 0 1 488,460 V420 A8,8 0 0 1 496,412 Z"/>
  <text class="d-text" x="584" y="432" text-anchor="middle" dominant-baseline="middle">image repository</text>
  <text class="d-text-small" x="584" y="452" text-anchor="middle" dominant-baseline="middle">one .moi file per node</text>
  <path class="d-edge-dashed" d="M584,468 L584,500" marker-start="url(#index-arrow)" marker-end="url(#index-arrow)"/>
  <text class="d-text-small" x="574" y="485" text-anchor="end" dominant-baseline="middle">push / pull with git</text>
  <path class="d-box" d="M496,500 H672 A8,8 0 0 1 680,508 V540 A8,8 0 0 1 672,548 H496 A8,8 0 0 1 488,540 V508 A8,8 0 0 1 496,500 Z"/>
  <text class="d-text" x="584" y="517" text-anchor="middle" dominant-baseline="middle">git remote</text>
  <text class="d-text-small" x="584" y="535" text-anchor="middle" dominant-baseline="middle">any git host</text>
  <text class="d-text-muted" x="40" y="512" text-anchor="start" dominant-baseline="middle">dashed: explicit, optional steps</text>
  <text class="d-text-muted" x="40" y="534" text-anchor="start" dominant-baseline="middle">one store per repository</text>
</svg>
<figcaption>Agents reach Moirai through the command line, the MCP server or optional hooks. Skills, short instruction files installed into each harness, teach them the commands; the binary is installed separately. Commands read the graph with Lachesis queries and change it with Lachesis mutations, which Klotho, the storage engine inside the one <code>moirai</code> binary, executes. Klotho keeps all data in one store per repository, which every worktree shares, and its file links point into the project's files. The git image is a text copy of the store in a separate git repository, which ordinary <code>git</code> can push to any remote.</figcaption>
</figure>

Three names appear in the picture. **Moirai** is the product, and `moirai` is its one command. **[Klotho](glossary.html#term-klotho)** is the storage engine, a library inside that binary. **[Lachesis](glossary.html#term-lachesis)** is the query language. In Greek myth the Moirai are the three Fates; Klotho spins the thread and Lachesis measures it.

There is no server to start and no background process. Each command opens the [store](glossary.html#term-store), does its work and exits, and the MCP server lives only as long as the agent session that started it. Inside a git repository the store sits in git's common directory, outside every working tree, so all worktrees of the repository share one store without any setup. Git itself is never needed to run Moirai; it is only one way to keep and move a copy of the data.

## Where it stands {#where-it-stands}

> [!NOTE]
> **Milestone M0: contract and evidence**
>
> Nothing is released, and there is nothing to install yet. The project is in its first milestone, M0, "contract and evidence". The repository holds the byte-level specification of the on-disk format, a complete reference model in Rust that serves as the test oracle, test fixtures, test harnesses and the first measurement tools. The storage engine is built in the next milestone, M1, which starts after version 1 of the on-disk format is frozen with the git tag `format-v1`. There is no release date. [How it is built, and where it stands](building.html) has the roadmap from M0 to M11 and the list of what exists today.

This book therefore describes the designed system. Commands and their output are designed output taken from the specification, not recordings, and every number is a design target, not a measurement. Where the book and the specification in the repository differ, the specification is right.

## How to read this book {#how-to-read}

The chapters build on each other, but each one can be read alone. If you have ten minutes, read the first chapter and the tour.

1. [Why Moirai](why.html): the problem it solves, its requirements, its four priorities and what it deliberately is not.
2. [The graph](concepts.html): nodes, kinds, statuses, edges, derived state such as "ready", deletion and leases.
3. [A session, start to finish](tour.html): one fictional project, command by command, with designed output.
4. [Branches, merges and history](versions.html): Moirai's own version control and how parallel lanes merge.
5. [Saving into git](git-image.html): how the graph is written into a git repository as text and read back.
6. [File links that survive moves](file-links.html): pointers into project files that follow renames and edits.
7. [Lachesis, the query language](lachesis.html): queries, versions inside queries, guarded writes and the accuracy benchmark.
8. [Working with agents](agents.html): the CLI, the MCP server, skills, hooks, context packs and leases.
9. [Under the hood: Klotho](engine.html): the storage engine, crash safety and the performance budgets.
10. [How it is built, and where it stands](building.html): the specification, the reference model, the tests, the roadmap and the status.
11. [Questions and answers](faq.html): short answers to the questions people ask first.
12. [Glossary](glossary.html): every term the book uses, with a link to the chapter where it matters.

---

Moirai is open source under the Apache License 2.0. The source code, the design documents and the specification are on GitHub at [github.com/bluesteelll/moirai](https://github.com/bluesteelll/moirai).
