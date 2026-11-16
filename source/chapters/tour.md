---
title: A session, start to finish
nav: A tour
description: A guided walk through one fictional project, from moirai init to a merged parallel lane, with designed command-line output.
---

# A session, start to finish

This tour follows one small, fictional project through a working day: create a store, plan tasks, hand one to an agent, record what was learned, recover from a write that lost a race, and merge a parallel line of work. Every command and flag below is part of Moirai's designed command line, and the output follows its designed output contract; where the specification leaves the wording of a line open, the page shows a plausible form.

> [!NOTE]
> **Designed output, not a recording**
>
> Moirai is not released and cannot be installed yet. The sessions on this page follow the output forms the specification defines. A few lines whose wording it leaves open, such as the `init` message, the claim and lane lines and the bodies of the brief and the pack, are illustrative. The project, ids, commit ids, times and paths are invented for the example.

## The project {#the-project}

tinyhttp is a small HTTP/1.1 server. Its code lives in a git repository at `/work/tinyhttp`. Today's goal is keep-alive support: one TCP connection serves several requests in a row.

Two parties do the work. You act as the [orchestrator](glossary.html#term-orchestrator): you plan, review and merge. An agent called `dev-1` writes code in its own shell. Later a second line of work, TLS support, runs in parallel.

In practice agents type most of these commands. Here you type them yourself, so each step is visible. Two conventions help you read the output:

- The first line of almost every result is a **header**, for example `branch: main | rev 5 | 2 rows`. In a header the branch always comes first; the brief and the pack start with a line of their own instead, which begins with the command's name. `rev` is the store-wide [sequence number](glossary.html#term-rev) of the commit the result was read from; where a command expects a [revision](glossary.html#term-revision), that commit is written `s5`. A write prints `rev 4 -> 5` instead.
- Every record starts with its id, such as `#3`, and takes one line.

## Create the store {#create-the-store}

```console
$ cd /work/tinyhttp
$ moirai init
init: new store /work/tinyhttp/.git/moirai | branch main | rev 0
```

*Designed output.*

Inside a git repository, `moirai init` puts the [store](glossary.html#term-store) in git's common directory, here `.git/moirai`. That place is outside every working tree, so `git add -A` never picks up store files, and every linked worktree of the repository finds the same store. Outside a repository, `init` creates `./.moirai/` instead. A new store has one branch, `main`, and no commits yet.

Verbs that read no branch, such as `init`, `doctor` or `config`, print no header. Their first line starts with the verb's own name.

Now try to add the first task:

```console
$ moirai add task 'Keep-alive support'
error[E406 role_policy]: this write needs a lease
  = help: an orchestrator presents its session lease with --lease (mint it once per session: moirai claim --role orchestrator --session)
$ moirai claim --role orchestrator --session --agent orch
branch: main | rev 0
1 CALL tx.claim(role: 'orchestrator'): lease L-1 holder orch, expires 2026-11-16T21:30Z
$ export MOIRAI_LEASE=L-1
```

*Designed output.*

The write was refused with exit code 6. In Moirai, write rights come only from a [lease](glossary.html#term-lease) that the caller presents. A caller with no lease may only record findings, notes and questions through the MCP `remember` tool. The error names exactly one fix, and that fix is the next command.

The session lease `L-1` gives its holder the orchestrator's rights. When you work through Claude Code or Codex with Moirai's hooks installed, the session-start hook mints this lease for you. At a bare terminal you mint it once and put it in `MOIRAI_LEASE`, so later commands present it without `--lease`. With no harness session to anchor it, the lease lives 12 hours, and every write that presents it renews it. [Working with agents](agents.html#leases-and-roles) explains the kinds of leases.

## Plan the work {#plan-the-work}

```console
$ moirai add task 'Keep-alive support'
branch: main | rev 0 -> 1 | committed c5e2b8a01 | lease L-1
1 CREATE task #1 "Keep-alive support"
affected: newly ready #1
$ moirai add task 'Parse the Connection header' --parent 1
branch: main | rev 1 -> 2 | committed c0d94f7e2 | lease L-1
1 CREATE task #2 "Parse the Connection header" UNDER #1
affected: newly ready #2; #1
$ moirai add task 'Reuse the connection in the accept loop' --parent 1 --blocked-by 2
branch: main | rev 2 -> 3 | committed c8b13c6d4 | lease L-1
1 CREATE task #3 "Reuse the connection in the accept loop" UNDER #1
2 CREATE #2 -[:BLOCKS]-> #3
affected: #2 #3
```

*Designed output.*

Every write is one commit. The header shows the sequence number before and after, the short commit id and the lease the write presented. A full commit id is `c` followed by 64 hexadecimal digits, a hash of the commit; results show `c` and the first eight of them. Then comes one line per statement: each write verb is a named mutation of [Lachesis](lachesis.html), Moirai's query language, so the result shows what it did in that language. The last line lists the ids whose derived state changed; `newly ready` names tasks that can now be started.

After the second command, `#1` is no longer ready. A task with children is a container. It never appears among the ready tasks, so no agent is handed it directly; agents work on its children.

Two habits of the command line show here. Short titles go in single quotes. Ids in arguments are bare (`--parent 1`, not `--parent #1`), because `#` starts a comment in Git Bash and PowerShell.

```console
$ moirai add task 'Idle timeout for kept-alive sockets' --parent 1 --blocked-by 3
branch: main | rev 3 -> 4 | committed c2a9f05b7 | lease L-1
1 CREATE task #4 "Idle timeout for kept-alive sockets" UNDER #1
2 CREATE #3 -[:BLOCKS]-> #4
affected: #3 #4
$ moirai add task 'Keep-alive integration tests' --parent 1 --blocked-by 3
branch: main | rev 4 -> 5 | committed c7c41e3d8 | lease L-1
1 CREATE task #5 "Keep-alive integration tests" UNDER #1
2 CREATE #3 -[:BLOCKS]-> #5
affected: #5
$ moirai tree 1
branch: main | rev 5 | 5 rows
#1 task open P2 "Keep-alive support" children:0/4
  #2 task open P2 "Parse the Connection header" parent:#1
  #3 task open P2 "Reuse the connection in the accept loop" parent:#1 blockers:#2 BLOCKED
  #4 task open P2 "Idle timeout for kept-alive sockets" parent:#1 blockers:#3 BLOCKED
  #5 task open P2 "Keep-alive integration tests" parent:#1 blockers:#3 BLOCKED
```

*Designed output.*

Each row of `tree` is a **node line**, the format most read verbs use (a few, such as `changes` below, print their own line shape): id, kind, status, priority (`P2` is the default), the title in quotes, then keys such as `parent:`, `blockers:` (the open blockers) and `children:` (done out of total), and last the flags in capital letters. Titles are always quoted, because they are text that people and agents wrote.

<figure class="diagram">
<svg viewBox="0 0 720 232" role="img" aria-labelledby="tour-d1-t">
  <title id="tour-d1-t">The keep-alive plan at rev 5: four subtasks under task 1, linked by blocks edges</title>
  <defs><marker id="tour-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path class="d-arrow" d="M0,0 L10,5 L0,10 z"/></marker></defs>
  <rect class="d-box-muted" x="16" y="16" width="520" height="200" rx="8"/>
  <text class="d-text-strong" x="276" y="44" text-anchor="middle">#1 Keep-alive support (a container)</text>
  <rect class="d-box-accent" x="40" y="104" width="136" height="64" rx="8"/>
  <text class="d-text-strong" x="108" y="130" text-anchor="middle">#2</text>
  <text class="d-text-small" x="108" y="152" text-anchor="middle">Connection header</text>
  <rect class="d-box" x="208" y="104" width="136" height="64" rx="8"/>
  <text class="d-text-strong" x="276" y="130" text-anchor="middle">#3</text>
  <text class="d-text-small" x="276" y="152" text-anchor="middle">reuse connection</text>
  <rect class="d-box" x="376" y="64" width="136" height="56" rx="8"/>
  <text class="d-text-strong" x="444" y="88" text-anchor="middle">#4</text>
  <text class="d-text-small" x="444" y="108" text-anchor="middle">idle timeout</text>
  <rect class="d-box" x="376" y="144" width="136" height="56" rx="8"/>
  <text class="d-text-strong" x="444" y="168" text-anchor="middle">#5</text>
  <text class="d-text-small" x="444" y="188" text-anchor="middle">integration tests</text>
  <path class="d-edge" d="M176,136 L206,136" marker-end="url(#tour-arrow)"/>
  <path class="d-edge" d="M344,126 L374,98" marker-end="url(#tour-arrow)"/>
  <path class="d-edge" d="M344,146 L374,168" marker-end="url(#tour-arrow)"/>
  <text class="d-text-strong" x="632" y="84" text-anchor="middle">At rev 5</text>
  <text class="d-text-small" x="632" y="112" text-anchor="middle">ready: #2</text>
  <text class="d-text-small" x="632" y="136" text-anchor="middle">blocking: #2, #3</text>
  <text class="d-text-small" x="632" y="160" text-anchor="middle">blocked: #3, #4, #5</text>
  <text class="d-text-small" x="632" y="184" text-anchor="middle">arrow = blocks</text>
</svg>
<figcaption>The plan as a graph. An arrow from one task to another means the first must finish before the second starts. Only #2 can start now; #2 and #3 hold up other work.</figcaption>
</figure>

## What can start, and what holds things up {#ready-and-blocking}

```console
$ moirai ready
branch: main | rev 5 | live | 1 row
#2 task open P2 "Parse the Connection header" parent:#1
$ moirai blocking
branch: main | rev 5 | live | 2 rows
#2 task open P2 "Parse the Connection header" parent:#1
#3 task open P2 "Reuse the connection in the accept loop" parent:#1 blockers:#2 BLOCKED
$ moirai blocking --ids
#2
#3
```

*Designed output.*

`ready` lists the tasks an agent may start now. A task is [ready](glossary.html#term-ready) when it is open, has no open [blocker](glossary.html#term-blocker) (its own or one inherited from an ancestor), is not a container, is not deferred, has no live lease of another agent, and was not already finished or deleted on another branch. The order is fixed: priority, then id. `blocking` answers the opposite question: which unfinished tasks hold up other unfinished tasks.

The `live` flag in both headers means the result depends on runtime state, such as leases, and so describes the store at this moment. `--ids` prints only ids, one per line, with no header, for scripts and pipes. If such a list does not fit its byte limit (24,000 bytes by default), Moirai cuts it: it prints only whole lines, at most 8,000 bytes of them (the limit for the output of a failed call), exits with code 10, and writes the count and a cursor to stderr, so a cut never drops ids silently.

## Claim a task, then complete it {#claim-and-complete}

The agent `dev-1` picks up the ready task. In its own shell:

```console
$ moirai claim 2 --agent dev-1 --start
branch: main | rev 5 -> 6 | committed c41a7d0e2
1 CALL tx.claim([#2]): lease L-2 holder dev-1, expires 2026-11-16T09:55Z; status open->in_progress
affected: none
```

*Designed output.*

The claim gives `dev-1` its own lease, `L-2`, on task `#2`. `--start` also moves the task to `in_progress` in the same commit. Any caller may claim a single task for itself in the developer or tester role; claims for other roles and claims of many tasks at once need the orchestrator. While `dev-1` holds the lease, `#2` is not ready for anyone else.

With no harness session to anchor it, this lease has a 15-minute deadline. Every write that presents the lease renews it, and `moirai heartbeat L-2` renews it during long work with no writes. If `dev-1` disappears, its lease runs out instead of holding the task forever.

`dev-1` reads its context pack (shown [below](#brief-and-pack)), writes the code, commits it with git as commit `4f2c9e1`, and reports:

```console
$ moirai complete 2 --lease L-2 --outcome done --evidence commit:4f2c9e1 --summary - <<'EOF'
Connection header tokens compared without regard to case; 6 unit tests
EOF
branch: main | rev 6 -> 7 | committed c6d2a91f0 | lease L-2
1 CALL tx.complete(#2): status in_progress->done (resolution completed); lease L-2 released into settled
affected: newly ready #3 | markers: settled #2 (main)
```

*Designed output.*

`complete` must present the task's lease. The summary comes on standard input through a quoted heredoc: bodies, summaries and other long text never go into command arguments, because quoting rules differ between bash, zsh, PowerShell and cmd. Only a short one-line value, such as a title, goes there, in single quotes. The summary becomes the commit message, with the evidence added after it. The outcome can be `done`, `failed` or `abandoned`; the task closes in each case, with a different resolution.

The lease is released, and a `settled` marker records that `#2` was finished on `main`. Other branches see the marker, so no branch hands the same task to a second agent. The result also says that `#3` is newly ready, so the orchestrator needs no extra query to find the next task.

## Record a rule, a note and a decision {#record-knowledge}

Work on `#2` taught the team three things. You record them:

```console
$ moirai rule --critical --applies-to 'role:developer,path:src/conn/**' --stdin <<'EOF'
Every socket read in src/conn has a timeout
EOF
branch: main | rev 7 -> 8 | committed c9e2f4a17 | lease L-1
1 CREATE rule #6 "Every socket read in src/conn has a timeout"
2 SET #6.text = $text
affected: none
$ moirai add note 'Clients send Connection: Keep-Alive in any letter case' --body - <<'EOF'
Seen in captures from curl, wrk and two browsers: "keep-alive", "Keep-Alive"
and "KEEP-ALIVE". Compare the header tokens without regard to case.
EOF
branch: main | rev 8 -> 9 | committed c3b8d1e64 | lease L-1
1 CREATE note #7 "Clients send Connection: Keep-Alive in any letter case"
2 SET #7.body = $body
affected: none
$ moirai decision --about 3 --stdin <<'EOF'
Reuse a connection only after the whole response is written
EOF
branch: main | rev 9 -> 10 | committed c6a0c9b38 | lease L-1
1 CREATE decision #8 "Reuse a connection only after the whole response is written"
2 SET #8.body = $text
3 CREATE #8 -[:ABOUT]-> #3
affected: none
```

*Designed output, shortened: the effect each `SET` line prints after a colon (the field's old and new value) is left out.*

A **rule** is guidance that agents must follow. `--applies-to` scopes it to roles and file paths; `--critical` makes sure it reaches every agent it applies to. The glob value is single-quoted because an unquoted `*` is expanded by bash and makes zsh stop with an error. A **note** records a fact; its body arrives on standard input. A **decision** records a choice, here linked to task `#3` with an `ABOUT` edge. In every statement, free text travels as a parameter such as `$text`, never pasted into the statement itself. A rule keeps its wording in a field of its own, `text`; the other kinds keep long text in `body`. The options `--stdin`, `--body -` and `--summary -` all read text from standard input; they belong to different verbs and fill different fields.

Rules and decisions start in the status `proposed`. Notes start `active`. A proposed record is shown to agents, but marked as not yet reviewed. Someone, usually the orchestrator, reviews the queue:

```console
$ moirai q proposed
branch: main | rev 10 | 2 rows
#8 decision proposed "Reuse a connection only after the whole response is written"
#6 rule proposed critical "Every socket read in src/conn has a timeout" applies_to:role:developer,path:src/conn/**
$ moirai set 6 --status active
branch: main | rev 10 -> 11 | committed c2d71f5a0 | lease L-1
1 SET #6.status = $status: status proposed->active
affected: none
```

*Designed output.*

`moirai q proposed` runs a [named query](glossary.html#term-named-query) of the standard library; every read verb is one, so `moirai ready` is the same as `moirai q ready`. The rule is now active. The decision stays proposed for the moment.

## What an agent sees: brief and pack {#brief-and-pack}

The next morning a new agent session starts in the repository. Its first view of the project is the [brief](glossary.html#term-brief):

```console
$ moirai brief
moirai brief | branch main | rev 11 | 226/8,000 B
#6 rule active critical "Every socket read in src/conn has a timeout" applies_to:role:developer,path:src/conn/**
needs review: 1 proposed | newest #8 | list: moirai q proposed
```

*Designed output, abridged: only some of the brief's lines are shown.*

The brief is a digest of the whole store for the start of a session: live lanes and their leases, runs in flight, staged merges, open questions for the project owner, critical rules, new verdicts, broken file links, and the number of records waiting for review. Its first line states the bytes used and the budget, 8,000 bytes by default. With hooks installed, the session-start hook puts the brief into the agent's context before its first turn. Without hooks, the agent's first call is `moirai brief`, and the result is the same.

Before `dev-1` starts on `#3`, it asks for its [context pack](glossary.html#term-context-pack):

```console
$ moirai pack 3 --role developer
moirai pack #3 developer | branch main | rev 11 | 408/16,000 B | digest 11-5b0e7c42
#6 rule active critical "Every socket read in src/conn has a timeout" applies_to:role:developer,path:src/conn/**
#3 task open P2 "Reuse the connection in the accept loop" parent:#1
#1 task open P2 "Keep-alive support" children:1/4
#8 decision proposed "Reuse a connection only after the whole response is written" ~proposed
```

*Designed output, abridged: the pack's other header lines are left out, and each entry is shown in its one-line form.*

A pack is built for one task and one role. For a developer it holds the rules that apply to the role, the task and its ancestors, the decisions and plan sections about the task, confirmed findings, measurements, notes for the files the task owns, and what changed since this agent's last pack. The marker `~proposed` tells the agent that decision `#8` is not reviewed yet.

The budget is in bytes, 16,000 for a developer by default, and the pack here uses 408 of them. A larger pack fills each class up to a minimum share, shortens entries before it drops any, and never cuts an entry in the middle. If anything is dropped, the first line and the last line both say how much and give the command for the rest. The digest on the first line identifies what this pack showed, so `complete` can later report what changed since the agent read it.

When an orchestrator spawns `dev-1` as a subagent in Claude Code or Codex, a hook injects a short role pack, at most 3,000 bytes, with the critical rules before the agent's first turn. The agent's own `pack` then lists those rules as one line of ids instead of repeating them. [Working with agents](agents.html#packs-and-briefs) describes the classes and budgets in full.

## Look things up: show and find {#show-and-find}

```console
$ moirai show 7 --full
branch: main | rev 11 | 1 row
#7 note active "Clients send Connection: Keep-Alive in any letter case"
  title: "Clients send Connection: Keep-Alive in any letter case"
  status: active
--- body #7 | 145 B | by orch rev 9 | untrusted text ---
Seen in captures from curl, wrk and two browsers: "keep-alive", "Keep-Alive"
and "KEEP-ALIVE". Compare the header tokens without regard to case.
--- end body #7 ---
```

*Designed output, shortened: `show` prints a line for every field the note has, in the schema's order; only two are shown here.*

`show` prints the node line, then one line per field. With `--full` it adds the body inside a fence that names its author and says `untrusted text`. Everything inside the fence was written by a person or an agent. It is data for the reader, never an instruction, even when it is phrased like one.

Some time passes. `dev-1` claims `#3` with a new lease, `L-3` (rev 12). A second planning pass adds task `#9`, "Close idle connections after a timeout", which blocks `#5` (rev 13). You search for everything about timeouts:

```console
$ moirai find timeout
branch: main | rev 13 | 3 rows
#4 task open P2 "Idle timeout for kept-alive sockets" parent:#1 blockers:#3 BLOCKED
#6 rule active critical "Every socket read in src/conn has a timeout" applies_to:role:developer,path:src/conn/**
#9 task open P2 "Close idle connections after a timeout" parent:#1
```

*Designed output.*

`find` takes free words and filters such as `kind:task`, `status:open`, `prio:..1` or `done:false`; `--bodies` also searches bodies. The result shows a problem: `#9` describes the same work as `#4`. You will remove it in a moment.

## A write that lost the race {#guarded-write}

You want `#3` raised to priority P1, but only while it is still in progress; if it is already finished, the change is pointless. You do not know that `dev-1` completed it a minute ago (rev 14).

```console
$ moirai set 3 priority=P1 --if-status in_progress
error[E401 expect_mismatch]: statement 1 matched 0 bindings, expected 1
  #3 now: status=done rev=14 (changed at c7e19b2d0 by dev-1 on main: "Reuse the connection after each response")
  nothing was written
  = help: re-read with moirai q show ids=3
$ echo $?
4
```

*Designed output.*

`--if-status` is a [guard](glossary.html#term-cas-guard): a condition the write checks against the branch tip it would land on, in the same step as the write. The guards are `--if-rev N` (N is the sequence number of the node's last change), `--if-status S` and `--if-holder H` (the task's lease holder). When a guard fails, nothing is written and the command exits with code 4. The error shows the current value, who changed it, at which commit and with which message, and it names one fix.

Guards exist because an agent cannot take back what is already in its context window. A value it read ten minutes ago may be wrong now. A guarded write turns that stale knowledge into a clear refusal with the facts needed to decide again. Every error text fits in 600 bytes.

## Delete with a dry run first {#delete-dry-run}

```console
$ moirai rm 9 --replaced-by 4 --reason duplicate --dry-run
branch: main | rev 14 | tx (dry) | would commit 3 changes
1 DELETE #9 task "Close idle connections after a timeout" REPLACED BY #4
    #9 -[:BLOCKS]-> #5    re-pointed: #4 -[:BLOCKS]-> #5
+ #4 task edge BLOCKS BLOCKS #5 added
- #9 task edge BLOCKS BLOCKS #5 removed
~ #9 task existence task "Close idle connections after a timeout" deleted ("duplicate", replaced_by #4)
affected: #4 #5 | invariants ok
targets: 3f6c0a9e1b7d4c2a8e5f0b1d9c7a6e42
to apply: send the same TX without DRY, with IF TARGETS '3f6c0a9e1b7d4c2a8e5f0b1d9c7a6e42'
$ moirai rm 9 --replaced-by 4 --reason duplicate --yes
branch: main | rev 14 -> 15 | committed c1f8a6d23 | lease L-1
1 DELETE #9 task "Close idle connections after a timeout" REPLACED BY #4
    #9 -[:BLOCKS]-> #5    re-pointed: #4 -[:BLOCKS]-> #5
affected: #4 #5 | markers: deleted #9 (main)
```

*Designed output.*

The dry run writes nothing and exits 0. It shows what the delete would touch: each edge with the policy that applies to it, the changes as diff rows, and a check that every invariant still holds. A diff row gives the change (`+` added, `-` removed, `~` changed), the node and its kind, the part of the node that changes (here `edge`), the name of that part (the edge kind `BLOCKS`), and what happened (for an edge: its kind, its target, and `added` or `removed`). Here the edge "#9 blocks #5" moves to the replacement, so `#5` now waits for `#4`. The last two lines give a digest of the change's target set and say how to apply the same change as a Lachesis block; at the command line, the same command with `--yes` applies it. Historical references, such as a note that cites a deleted node, stay in place and point at a tombstone, and a citing record is marked as suspect. Moirai never prompts; `rm` without `--yes` prints the dry run.

After `--yes`, `#9` is a [tombstone](glossary.html#term-tombstone): it keeps its id, its reason and its replacement, any reference to it renders as `#9(deleted)`, and `moirai show 9` prints the tombstone and exits with code 3. The `deleted` marker tells other branches not to hand `#9` to anyone. Only the orchestrator and the owner may delete nodes, and only through the command line.

## Catch up: changes since a revision {#changes}

```console
$ moirai changes --since 13
branch: main | rev 15 | 7 rows
rev 14 main #3 ~ status dev-1
rev 14 main #3 end lease L-3 dev-1
rev 14 main #3 settled marker s14 dev-1
rev 15 main #4 + edge BLOCKS orch
rev 15 main #9 - edge BLOCKS orch
rev 15 main #9 ~ existence task orch
rev 15 main #9 deleted marker s15 orch
```

*Designed output.*

The [change feed](glossary.html#term-change-feed) has one line per change: the sequence number of its commit, branch, node, operation, the part of the node that changed, and who changed it. Lease events and markers are entries too. `--about 3,4` narrows the feed to some nodes, `--for-agent A` keeps only the changes that matter to one agent (nodes it claimed, waits on, wrote or cited), and `--all` adds every other branch. With hooks installed, the same feed reaches an agent as a short delta before each prompt, at most 600 bytes and nothing at all when nothing changed.

## A parallel lane {#parallel-lane}

TLS support can go ahead while keep-alive work continues. You create a git worktree at `/work/tinyhttp-tls` on a new git branch `tls`, with git's usual command, and open a [lane](glossary.html#term-lane) for it:

```console
$ moirai lane open tls --worktree /work/tinyhttp-tls --git-branch tls
branch: main | rev 15 -> 16 | committed c0b6e3f81 | lease L-1
lane #10 "tls" | lane/tls forked at c0b6e3f81 | /work/tinyhttp-tls bound to lane/tls
$ cd /work/tinyhttp-tls
$ moirai add task 'TLS listener on port 8443' --parent 1
branch: lane/tls | rev 16 -> 17 | committed c5f0d2a19 | lease L-1
1 CREATE task #11 "TLS listener on port 8443" UNDER #1
affected: newly ready #11
```

*Designed output.*

A lane joins three things: a branch of the graph (`lane/tls`), a git worktree, and a lane node on `main` that records them. The worktree directory is bound to the branch, so every command run inside it reads and writes `lane/tls`, and every header says so in its first field. Sequence numbers are store-wide, so the commits of one branch can skip numbers.

Meanwhile, back in the main worktree, you accept decision `#8` (rev 18). On the lane:

```console
$ moirai ready
branch: lane/tls | rev 17 | live | 2 rows | behind main 1
#4 task open P2 "Idle timeout for kept-alive sockets" parent:#1
#11 task open P2 "TLS listener on port 8443" parent:#1
$ moirai sync
branch: lane/tls | rev 17 -> 19 | committed c8d3a0b75 | lease L-1
sync main into lane/tls: landed | 0 conflicts | 0 violations
$ moirai merge lane/tls --into main
branch: main | rev 18 -> 20 | committed c2f9e6a48 | lease L-1
merge lane/tls into main: landed | 0 conflicts | 0 violations
```

*Designed output.*

`behind main 1` in the header warns that the lane lacks one commit from `main`. `sync` merges `main` into the lane; `merge` brings the lane's work into `main`, and it syncs first by itself when `main` has moved. Merges are typed per field and per edge. A conflict becomes a value that an agent or a person resolves, and a change that would break an invariant stops the merge on a separate staging branch until someone decides.

Moirai merges the graph; git still merges the code (`git merge tls`). Then `moirai lane close tls` records the lane as merged and removes the directory binding. The page on [versions and branches](versions.html) covers lanes, conflicts, staging and undo in depth.

## What the tour showed {#what-you-saw}

- One store per repository, outside every working tree, shared by all worktrees.
- Tasks with parents and blockers, and two questions answered directly: what can start (`ready`) and what holds work up (`blocking`).
- Write rights that come only from leases, from `claim` to `complete`.
- Rules, notes and decisions, with a review queue for proposed records.
- Budgeted context for agents: the brief at session start and a pack per task.
- Guarded writes that refuse stale changes and show the current value.
- Deletes that show their effects first and leave tombstones behind.
- A change feed, and a parallel lane that syncs and merges like a git branch.

Next: [Working with agents](agents.html) explains the surfaces agents use, and [Lachesis](lachesis.html) shows the query language behind every verb.
