---
title: Branches, merges and history
nav: Branches & merge
description: How Moirai versions the whole graph per branch, lets many agents work on separate branches of one store at once, and merges their work back with typed rules.
---

# Branches, merges and history

Moirai has its own version control, built into the store. Every task, status, blocker, rule, note, decision and finding lives on a branch. Many agents can work on different branches of one store at the same time, and a typed merge brings their work back to `main`. The merge knows what a status, a counter and a blocker are, so it combines most changes without asking anyone, and it never lets a change break the graph.

## What is versioned {#what-is-versioned}

Everything in the graph is versioned per [branch](glossary.html#term-branch). Each branch has its own state of:

- every node of every kind: tasks, notes, rules, decisions, questions, findings, verdicts, measurements and the rest;
- every field, including `status`, `resolution`, `assignee` and `phase_state`;
- every edge, including `blocks`, `gates` and `parent`, so blockers and the task tree are versioned too;
- the body text of every node;
- the schema: the kinds, fields and edge kinds the project uses;
- conflict values that a merge left for someone to resolve (see [Conflicts are data](#conflicts)).

A small part of the state is not history. It answers the question "who is doing what right now", so it exists once per store, and every branch sees the same copy:

| Store-wide state | Why it is not versioned |
|---|---|
| [Leases](glossary.html#term-lease) (an agent's claim on a task) | A lease merged in from another branch would be a claim by an agent that may no longer exist. One store-wide lease also stops two branches from building the same task. |
| Completion and deletion markers | They tell every branch that a task was finished or deleted on another branch that is not merged yet (see [Coordination across branches](#coordination)). |
| The id allocator | A short id such as `#31` is unique across all branches. |
| Client heads | They record which branch each agent, session or directory works on. |
| Idempotency results and the change feed | They record what already happened in the store: a retried write finds its first result, and the change feed lists the changes of every branch, each marked with its branch. |

## Branches and refs {#branches}

`main` is an ordinary branch. It is created with the store and can never be deleted, but nothing else about it is special: you fork from it, merge into it and read any past point of it like any other branch. Every other name belongs to a fixed family, and the first part of the name decides what kind of ref it is.

| Family | Example | What it is for |
|---|---|---|
| `main` | `main` | The shared line of work. |
| `lane/<name>` | `lane/chunked` | A line of parallel work, usually bound to one git worktree (see [Lanes](#lanes)). |
| `plan/<name>` | `plan/q3` | A what-if plan. You can change blockers, the task tree and gates there, but status, resolution, assignee and claims are read-only. |
| `tags/<name>` | `tags/v0.4` | A fixed name for one commit. A tag never moves. |
| `merge/<dst>/from/<src>` | `merge/main/from/lane/chunked` | A [staging ref](glossary.html#term-staging-ref): a merge that stopped and waits for a fix. Only the merge machinery creates it, and after that it accepts nothing but resolutions. |
| `import/<ref>` | `import/main` | Commits imported from the [git image](git-image.html) that wait for a merge or a fix. |

Names are lower case. `moirai branch chunked` creates `lane/chunked`, `moirai branch q3 --kind plan` creates `plan/q3`, and `moirai tag v0.4` creates `tags/v0.4`. The family words (`main`, `lane`, `plan`, `tags`, `merge`, `from`, `import`, `orphans`) are reserved: none of them can be a whole segment of a name you choose, so every staging name reads in exactly one way. Every ref name also passes git's own ref-name rules and is a valid file name on Windows, Linux and macOS, so it can be copied into git unchanged.

### Forking is cheap {#cheap-forks}

Creating a branch copies nothing. It records the fork point and keeps the newest saved snapshot of `main` at or before it. The design estimate for a fork is about 2 ms. A branch is read as that snapshot, plus `main`'s commits up to the fork, plus the commits made on the branch, and the target for the first read of a lane that has lived for two weeks is 10 ms or less. When a branch drifts far from its snapshot, Moirai writes a fresh snapshot for it outside the write path, so reads stay fast. This changes no history.

## Many agents, one store {#heads}

Each client has its own [HEAD](glossary.html#term-head): the branch it reads and writes. A client is a directory, a named client or an agent session. One agent can work on `lane/chunked` while another works on `lane/keepalive` and an orchestrator reads `main`, all in one store at the same time. Each process builds views only of the branches it reads. Writes from every branch go through the store's single writer lock, one short commit at a time. Two agents that write to the same lane are simply ordered; neither fails because the other committed first. Nothing one client does to its HEAD changes the HEAD of another.

Every result of a command that reads or writes a branch starts with that branch, so an agent always sees where it is:

```text
branch: lane/chunked | rev 5131 | 4 rows | behind main 3
```

`moirai checkout REF` moves the caller's HEAD. Checking out a commit instead of a branch is allowed for reading; a write there is refused unless you name a new branch with `--branch-new`.

<details>
<summary>How a command finds its branch</summary>

The first source that gives a branch wins:

1. an explicit `--branch` (or the `branch` argument of an MCP tool);
2. the branch recorded on the lease the caller presents;
3. the harness's working directory, through its directory binding;
4. the environment variable `MOIRAI_BRANCH`;
5. the dispatch marker a sub-agent received when it started;
6. `--client NAME` or `MOIRAI_CLIENT`;
7. the binding registered for the current directory with `moirai worktree bind`;
8. the binding of the current git worktree;
9. the `default-branch` setting, which is `main` unless changed.

An explicit branch that differs from the lease's branch is refused (exit code 5). An environment variable that a process inherited never outranks the lease the caller presents.

</details>

## Lanes and git worktrees {#lanes}

A [lane](glossary.html#term-lane) is the usual unit of parallel work. It joins three things: a Moirai branch, a git worktree for the code, and a `lane` node that records both. One command sets all three up:

```console
$ moirai lane open chunked --worktree /work/tinyhttp-lanes/chunked --git-branch chunked
branch: main | rev 5040 -> 5041 | committed c3e7a90d1
```

The command creates the `lane` node on `main`, forks `lane/chunked` from that commit and binds the worktree directory to the new branch. From then on, every `moirai` command run inside that worktree reads and writes `lane/chunked` without a flag.

Moirai never creates, switches or deletes a branch because git did something. A new git worktree or git branch does not create a Moirai branch, and work done in a scratch worktree simply writes to `main`. Optional git hooks only help: after a `git merge`, the hook `moirai hook git-post-merge` checks whether the bound lane is ready to merge and offers the `moirai merge` command. `moirai lane close chunked` marks the lane `merged` (when `main` already contains its work) or `abandoned`, and removes the directory binding.

## Keeping a lane current: sync {#sync}

`moirai sync` merges `main` into the current lane. It uses the same typed merge as any other merge, with the lane as the destination. Rules, completed tasks and deletions written on `main` reach the lane this way.

A sync does not copy `main`'s changes into the lane's history. It writes one sync commit with two parents, the lane tip and the tip of `main`, and stores only the residue: the keys whose merged value differs from what `main`'s changes alone would give, usually the keys that both sides changed. When the lane is read, `main`'s changes are found by reference. A lane's history therefore grows with its own work, not with the traffic on `main`.

`moirai sync --check` previews a sync without writing anything. When hooks are installed, the hook that runs as a sub-agent starts applies a sync on its own, but only when the preview is clean: no conflicts, no violations and at most 2,000 changed keys. Otherwise it prints one line, such as `behind main: 4 commits, 1 conflict on #91 -> moirai sync`, and leaves the decision to the orchestrator.

## The typed three-way merge {#merge}

`moirai merge SRC --into DST` merges one branch into another. Like git, it compares both sides with their [merge base](glossary.html#term-merge-base), the newest commit both sides share. Unlike git, it does not compare lines of files. It compares keys: one field of one node, one edge, the existence of one node, its place in the task tree, its body, one schema item. The base of each key is that key's value at the merge base.

A key that only one side changed takes that side's value. A key that both sides changed goes to a rule chosen by the key's type:

| Type of key | Rule | When the rule cannot decide |
|---|---|---|
| Status | Two forward moves are joined along the kind's order of states: `in_progress` on one side and `done` on the other give `done`. | `StatusFork`, for example `done` against `cancelled`, or a reopen against a completion. |
| Numbers, choices and other single values | If one side kept the base value, the other side's value wins. | `FieldEdit`: both sides set different values. |
| Counters | The increments of both sides are added. | Never conflicts. |
| Sets, such as labels | Union of both sides. An element is removed only if a side removed it relative to the base; an add wins over a concurrent remove. | Never conflicts. |
| Text fields and bodies | A line-by-line three-way merge (diff3) against the base text. | `TextHunk`: both sides changed the same lines. |
| Place in the task tree | Moves from both sides are replayed in time order. A move that would put a node under its own descendant is skipped and logged. | A `HierarchyCycle` violation. |
| Existence of a node | A node deleted on one side and only read on the other is deleted. | `DeleteVsModify`: deleted on one side, changed on the other. Until someone resolves it, a per-kind policy decides how the node looks: a task stays deleted, a knowledge node comes back. |
| `supersedes` edges | A node has at most one active replacement. | `SupersedeFork`: the two sides chose different replacements. |
| Fields written with the project owner's authority (`authority = owner`) | `main`'s value wins in a merge into `main`. | `OwnerFieldEdited` in a merge into any other branch. |
| Schema | Additions (new kinds, fields, values) from both sides are combined. | A `SchemaConflict` violation: both sides made the schema stricter. |

After the rules run, validators check the result as a whole: every blocker and gate edge points at a live node, blockers form no cycle, the task tree is still a tree, the schema holds, and every named query the merge touched still parses. A merge always writes a merge commit with two parents, even where git would fast-forward. Merging a branch that the destination already contains writes nothing and reports "already merged". The design target for merging a lane with 2,000 changes is 50 ms or less at 100,000 nodes.

### Merges into main sync first {#sync-first}

Before a branch is merged into `main`, Moirai first syncs `main` into that branch, unless the branch already contains `main`'s tip. Both steps are computed before the store is locked. When the sync lands clean, both are written together; when it leaves conflict values on the lane, the sync is kept and the merge is refused until they are resolved. After the sync, `main`'s tip is the merge base, so the merge into `main` itself cannot conflict. If the sync stops on a problem, the whole merge stops, and the problem is fixed on the lane, never on `main`. A merge into `main` is also refused while the branch still holds unresolved conflict values; `moirai merge-check` lists them before you try.

<figure class="diagram">
<svg viewBox="0 0 720 320" role="img" aria-labelledby="versions-d1-t">
  <title id="versions-d1-t">Branch main with two lanes: lane/a forks, syncs, resolves a conflict and merges back; a merge of lane/b stops on a violation and is staged</title>
  <defs><marker id="versions-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path class="d-arrow" d="M0,0 L10,5 L0,10 z"/></marker></defs>
  <text class="d-text-strong" x="20" y="80" dominant-baseline="middle">lane/a</text>
  <text class="d-text-strong" x="20" y="160" dominant-baseline="middle">main</text>
  <text class="d-text-strong" x="20" y="250" dominant-baseline="middle">lane/b</text>
  <line class="d-edge" x1="90" y1="160" x2="700" y2="160" marker-end="url(#versions-arrow)"/>
  <text class="d-text-muted" x="700" y="182" text-anchor="end">time</text>
  <line class="d-edge" x1="170" y1="80" x2="410" y2="80"/>
  <line class="d-edge" x1="260" y1="250" x2="380" y2="250"/>
  <line class="d-edge" x1="124" y1="153" x2="166" y2="87" marker-end="url(#versions-arrow)"/>
  <text class="d-text-small" x="118" y="120" text-anchor="end">fork</text>
  <line class="d-edge" x1="303" y1="152" x2="327" y2="88" marker-end="url(#versions-arrow)"/>
  <text class="d-text-small" x="300" y="118" text-anchor="end">sync</text>
  <text class="d-text-small" x="326" y="56" text-anchor="middle">conflict lands</text>
  <text class="d-text-small" x="412" y="56" text-anchor="middle">resolve</text>
  <line class="d-edge-accent" x1="414" y1="86" x2="456" y2="154" marker-end="url(#versions-arrow)"/>
  <text class="d-text-small" x="440" y="112">merge</text>
  <line class="d-edge" x1="223" y1="167" x2="257" y2="243" marker-end="url(#versions-arrow)"/>
  <text class="d-text-small" x="232" y="208" text-anchor="end">fork</text>
  <line class="d-edge-dashed" x1="388" y1="250" x2="456" y2="250" marker-end="url(#versions-arrow)"/>
  <text class="d-text-small" x="422" y="238" text-anchor="middle">merge</text>
  <line class="d-edge-dashed" x1="580" y1="168" x2="580" y2="208" marker-end="url(#versions-arrow)"/>
  <text class="d-text-small" x="590" y="190">sync first</text>
  <circle class="d-box" cx="120" cy="160" r="6"/>
  <circle class="d-box" cx="220" cy="160" r="6"/>
  <circle class="d-box" cx="300" cy="160" r="6"/>
  <circle class="d-box" cx="460" cy="160" r="6"/>
  <circle class="d-box" cx="580" cy="160" r="6"/>
  <circle class="d-box" cx="170" cy="80" r="6"/>
  <circle class="d-box" cx="240" cy="80" r="6"/>
  <circle class="d-box" cx="330" cy="80" r="6"/>
  <circle class="d-box" cx="410" cy="80" r="6"/>
  <circle class="d-box" cx="260" cy="250" r="6"/>
  <circle class="d-box" cx="320" cy="250" r="6"/>
  <circle class="d-box" cx="380" cy="250" r="6"/>
  <rect class="d-box-accent" x="460" y="212" width="240" height="80" rx="8"/>
  <text class="d-text-strong" x="580" y="236" text-anchor="middle">merge/lane/b/from/main</text>
  <text class="d-text-small" x="580" y="258" text-anchor="middle">staged: DanglingEdge</text>
  <text class="d-text-small" x="580" y="278" text-anchor="middle">lane/b and main did not move</text>
</svg>
<figcaption>lane/a forks from main, takes main's changes with a sync (a value conflict lands there as data), resolves it and merges back. A merge of lane/b first syncs main into lane/b. That sync finds a structural problem, so it is staged on its own ref and neither branch moves.</figcaption>
</figure>

<details>
<summary>When there is more than one merge base</summary>

Branches that were merged into each other in both directions can have several newest common commits. Moirai then uses one rule, a recursive virtual base: it merges those commits pairwise, oldest first, with the same typed rules, and uses the result as the base. If a key of that virtual base holds a conflict value, the key is clean when both sides now hold the same value and conflicts whenever they differ. So two sides that settled an old conflict differently always conflict again, and two sides that settled it the same way never do. Because merges into `main` sync first, the daily path has a single merge base, and this rule matters only for merges between lanes and for branches of branches.

</details>

## Conflicts are data {#conflicts}

When a rule cannot decide, the merge does not stop. It stores a [conflict value](glossary.html#term-conflict-value) on the key: the class of the conflict and the three sides, `base`, `ours` and `theirs`. The merge commit lands, and the node is marked conflicted. A conflicted node leaves the [ready queue](glossary.html#term-ready-queue), so no agent is sent to a task whose data is in doubt. Every other change of the merge is in place.

`moirai conflicts` lists the open conflict values, and `moirai resolve` settles one by taking a side, the base, or a new value. A resolution is an ordinary commit, with its own history. In this example a sync of `main` into a lane found that both sides changed a task's estimate:

```console
$ moirai conflicts
branch: lane/chunked | rev 5131 | 1 row
#31.estimate FieldEdit base=3 ours=5 theirs=8 (sync rev 5130)
  resolve: TX ON lane/chunked { RESOLVE '#31.estimate' TAKE OURS }   (or THEIRS, BASE, VALUE $text)
$ moirai resolve 31.estimate --take theirs
branch: lane/chunked | rev 5131 -> 5132 | committed c4be19d2e
1 RESOLVE #31.estimate TAKE THEIRS: 8
affected: #31
```

In a sync, `ours` is the lane and `theirs` is `main`. The `resolve:` line shows the same action written in [Lachesis](lachesis.html), the query language. Readers never meet merge markers: a context pack shows a conflicted note as one line with its base text and the command that resolves it. Git-style `<<<<<<<` markers appear only on request, with `moirai show --full --markers`. If you want git's behaviour instead, `merge --strict` treats every conflict value like a violation and stops the merge.

## Structural violations stop the merge {#violations}

Some results would break the graph itself, and they never land on a branch. These are the [structural violations](glossary.html#term-structural-violation):

| Violation | Meaning |
|---|---|
| `DanglingEdge` | A blocker or gate edge would point at a node the other side deleted. |
| `Cycle` | Blockers would form a cycle, so no task in it could ever start. |
| `HierarchyCycle` | The task tree would stop being a tree. |
| `DepthExceeded` | The task tree would grow deeper than 12 levels. |
| `Cardinality` | An edge kind that allows only one target would get a second one (for example two `runs_in` edges, or a `duplicate_of` chain longer than one). |
| `SchemaConflict` | Both sides made the schema stricter in ways that do not fit together. |
| `QueryInvalid`, `QueryCycle` | A saved named query would no longer parse, or named queries would call each other in a loop. |
| `PlanMask` | A merge into a `plan/` branch would change a field that is read-only there, such as `status` or `assignee`. |
| `RemovedTextNotInBase` | The text merge of a document section fails the check on removed text, which guards section text against silent loss. |
| `IdCollision` | Two creations of one random uid, found by a merge or an import. |
| `NotFound` | A revert or cherry-pick targets something that no longer exists. |
| `ImageParse`, `TombstoneRemoved` | Problems found while importing from the git image (see [Saving into git](git-image.html)). |

When a merge finds any of these, it writes its result to a staging ref named `merge/<dst>/from/<src>` instead of the destination. The destination does not move. The command prints each violation and exits with code 6; `moirai conflicts` on the staging ref lists each one with its suggested fix. There is one staging ref per pair of branches, so two lanes can each have a staged sync at the same time; only a second merge of the same pair is refused while the first one waits.

You fix a staged merge with `moirai resolve` on the staging ref, then run `moirai merge --continue`. Continue does not replay old work blindly: it recomputes the merge against the destination's current tip and lays your resolutions over it. A resolution whose key changed on the destination in the meantime is not applied, and the command prints a notice for it. `moirai merge --abort` drops the staging ref instead.

A short session from a fictional project, `tinyhttp`. On `lane/chunked` an agent deleted task `#40`, an old body reader, and named `#52` as its replacement. Meanwhile someone on `main` added the edge `#203 blocks #40`. A merge into `main` starts by syncing `main` into the lane, so a problem found in that sync is staged under the sync's name, `merge/lane/chunked/from/main` (destination `lane/chunked`, source `main`).

```console
$ moirai merge lane/chunked --into main
error[staged]: merge of lane/chunked into main staged on merge/lane/chunked/from/main: 1 violations, 0 conflicts; main did not move
  edge:#203:blocks:#40 DanglingEdge main added #203 blocks #40; lane/chunked deleted #40 (replaced_by #52)
  = help: moirai conflicts merge/lane/chunked/from/main lists each one with its fix
$ moirai conflicts merge/lane/chunked/from/main
branch: merge/lane/chunked/from/main | rev 5120 | staged (read-only) | 1 row
edge:#203:blocks:#40 DanglingEdge main added #203 blocks #40; lane/chunked deleted #40 (replaced_by #52)
  suggested: moirai resolve 'edge:#203:blocks:#40' --take repoint:52, then moirai merge --continue main --into lane/chunked
$ moirai resolve 'edge:#203:blocks:#40' --take repoint:52 --branch merge/lane/chunked/from/main
branch: merge/lane/chunked/from/main | rev 5120 -> 5121 | committed c9d04e17b | staged
1 RESOLVE edge:#203:blocks:#40 TAKE REPOINT #52: the edge becomes #203 -[:BLOCKS]-> #52
affected: #203 #52
next: moirai merge --continue main --into lane/chunked
$ moirai merge --continue main --into lane/chunked
branch: lane/chunked | rev 5119 -> 5122 | committed c81e07b45
$ moirai merge lane/chunked --into main
branch: main | rev 5117 -> 5123 | committed c2a6d9f03
```

The merge into `main` started with its sync of `main` into the lane. That sync found the dangling edge and stopped on `merge/lane/chunked/from/main`; neither branch moved. The `repoint` resolution replaced the deleted end of the edge with `#52`, so the edge became `#203 blocks #52`. `merge --continue` landed the sync on the lane, and the merge into `main` then ran clean.

## History {#history}

Every write is one commit, so the commit log is also a complete log of operations. Commits form one graph across all branches: a branch can fork from another branch, and `main` can be synced into a lane many times before the lane merges back. History is never rewritten. Undo moves a branch pointer back and records that move; the undone commits stay until garbage collection removes them.

| Command | What it does |
|---|---|
| `moirai log [REF] [--graph] [--node N]` | Lists the commits of a branch. `--node` lists only the edits to one node. |
| `moirai diff A..B`, `moirai diff A...B` | Shows the changes between two points as typed changes per key. The three-dot form shows both sides against their merge base and marks keys that both sides changed with `both`: it is the preview of a merge. |
| `moirai blame N [FIELD]` | Shows the last commit that changed each field and edge of a node. |
| `moirai tag NAME [COMMIT] [--pin]` | Names a commit. `--pin` keeps a snapshot there, so reading that point later is fast. |
| `moirai reflog REF` | Lists every move of a branch pointer, with who moved it and why. |
| `moirai undo [--ref REF] [N]` | Moves a branch back by N moves. By default it first checks that the tip is still the one the caller last read. |
| `moirai revert COMMIT` | Adds a commit that exactly inverts another one; commits store the values they replaced, so the inverse is exact. A merge commit is reverted against its first parent with `--mainline 1`. A sync cannot be reverted; it is backed out with `undo` on the lane. |
| `moirai cherry-pick COMMIT --onto REF` | Applies one commit's changes on another branch with the same typed rules and validators. |
| `moirai op log`, `moirai op restore SEQ` | Lists every change to the set of branch tips across the store, and restores every branch to an earlier point. |

A revert is refused when a later commit depends on the reverted one, for example when it added a blocker to a node the reverted commit created; the refusal lists those commits. There is no rebase: it rewrites history, and it would be the one command that changes the ids of lane commits already written to the git image.

### Reading the past {#reading-the-past}

Every read command takes `--at REV`, and a Lachesis query can start with `USE REV`. A [revision](glossary.html#term-revision) names a point in history:

| Revision | Meaning |
|---|---|
| `main`, `lane/chunked` | The tip of a branch. |
| `c9b2e6c1` | A commit, by a prefix of its id. |
| `s4400` | The commit with sequence number 4400 in this store. |
| `main~3`, `main^2` | Three first-parent steps back; the second parent of a merge. |
| `main@2`, `main@2026-11-01` | Where `main` pointed two moves ago; where it pointed at a given time. |
| `main..lane/chunked`, `main...lane/chunked` | Ranges, for `log` and `diff`. |

```console
$ moirai show 31 --at main@2026-11-01
```

```lq
USE main~5 MATCH (t {id: #93}) RETURN t.status, t.unblocked
```

A view of the past is read-only: a write there is refused. Facts that exist only now, such as leases and the ready queue, are not available in a past view, so a query about the past asks whether a task was `unblocked` rather than `ready`. Only a plain branch name, or `HEAD` when it resolves to a branch, gives the live tip; a tag or a suffixed revision always gives a past view, even when it points at the current tip.

## Coordination across branches {#coordination}

Isolation alone would cause double work. Suppose an agent finishes task `#31` on `lane/chunked`, and the lane is not merged yet. On `main` and on every other lane, `#31` is still open, and an orchestrator could send a second agent to build it. Moirai closes this gap with store-wide state that every branch reads.

- **Leases are store-wide.** A claim requires the task to be ready on the claimer's branch and records that branch. While the lease lives, no other branch can claim the task.
- **Completion markers.** Any commit that sets a task to `done` or `cancelled` also writes a store-level [settled marker](glossary.html#term-settled-marker), whatever path produced it: a command, a merge, a sync, a cherry-pick, a revert or an import. Every other branch then treats the task as done on that branch and unmerged: it cannot be claimed, it is not dispatched, and it is not listed as a live blocker.
- **Deletion markers** do the same for nodes deleted on a branch that is not merged yet.
- **Absorbed markers stop mattering.** A marker no longer affects a branch once that branch's history contains the completing or deleting commit: at once on its own branch, at the merge for `main`, and at the next sync for other lanes. Reopening the task clears the marker, and `undo` or deleting a branch recomputes the markers it affects.

The markers are only a fast cache of a rule defined on the branches themselves: a task is kept out of the ready queue on a branch exactly when another live branch holds it done, cancelled or deleted, and this branch's history does not contain that change yet.

The blockers list of a task still shows such a blocker, marked with the branch that finished it. Here an agent on `lane/keepalive` asks what blocks task `#33`:

```console
$ moirai blockers 33
branch: lane/keepalive | rev 5140 | live | 1 row | behind main 3
#31 task open P1 "Parse chunked request bodies" depth=1 reason=direct elsewhere=lane/chunked SETTLED-ELSEWHERE
```

On `lane/keepalive`, `#31` is still `open`, so `#33` stays blocked there until the completion of `#31` reaches it through `main`. But the row shows that `#31` is finished on `lane/chunked`, and no one will be sent to build it a second time. The header says `live` because the result reads store-wide state that is not history. A lane also learns when it falls behind. The result header shows `behind main N`, and the hooks add one line such as `behind main by 4 commits (1 completions, 2 rules, 0 deletions touching your nodes)`, printed only when the counts change. Critical rules written on `main` that a lane has not synced yet still appear in that lane's context packs, marked `~main`, so branching never hides a rule from an agent.

<details>
<summary>A deleted node across branches</summary>

Deleting `#40` on `lane/x` does not change any other branch: `#40` stays live on `main` until the merge. But the deletion marker keeps it out of the ready queue, off the lists of live blockers and out of claims on every branch, and `moirai show 40` on any other branch marks its line `DELETED-ELSEWHERE`. At the merge, a `main` that only read `#40` accepts the delete, a `main` that changed `#40` gets a `DeleteVsModify` conflict value, and a `main` that added a new blocker edge to `#40` stops with a `DanglingEdge` violation, as in the session above. While a node is leased on any branch, `moirai rm` refuses to delete it unless the same command also releases the lease with `--release`.

</details>

> [!NOTE]
> **Where this stands**
>
> Version control is specified in [docs/spec/format/12-vcs.md](https://github.com/bluesteelll/moirai/blob/master/docs/spec/format/12-vcs.md), a draft chapter of the format specification, and the engine's version control is built in milestone M3. [Where it stands now](building.html#status).
