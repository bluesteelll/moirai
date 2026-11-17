---
title: The graph
nav: The graph
description: "Nodes, kinds, fields, statuses, edges, derived state, deletion, leases and the change feed: the data model that every Moirai command works on."
---

# The graph

Everything Moirai keeps is a node or an edge in one typed graph. A task, a rule, a review finding and a source file are all nodes. "This subtask belongs to that task" and "this task must finish before that one starts" are edges. This chapter explains the parts of the graph and the rules the engine enforces on them.

The examples use a small fictional project, an HTTP server called `tinyhttp`. All ids and titles are made up.

## Nodes and their ids {#nodes-and-ids}

Every [node](glossary.html#term-node) has two identities.

- **`#N`** is a short number, such as `#812`. It is the id you see and type. The store allocates it, counting up, and never reuses it: not after the node is deleted, not on another branch, not after an import. A `#N` means the same node on every branch where that node exists.
- **A `uid`** is a 128-bit identity that never changes; for most kinds it is random. Commit hashes and the [git image](git-image.html) use it, because it stays the same when the data moves to another store. Agents rarely see it.

The short number is a deliberate choice for agents. A long random id costs many tokens on every mention, and models copy long ids with more mistakes. Ids carry no hierarchy and no kind prefix: `#93` stays `#93` when the task moves under another parent. A node's place in a tree is computed when it is shown, never built into its id.

On the command line ids are typed without the `#` (`moirai show 40`; see [shell-safe arguments](agents.html#bodies-on-stdin)). Inside text, `#40` is welcome. When a title, an abstract or a body mentions `#40`, Moirai records a `mentions` edge to that node, so the mention can be followed, and it still leads to the node's tombstone if the node is later deleted.

Every node has a title (one line, at most 200 bytes), an optional one-line abstract and an optional body of up to 64 KiB of text. Larger files stay in the repository and are linked as file nodes.

## Node kinds {#kinds}

Every node has a kind. The core schema defines thirteen kinds, and they cannot be changed. A project can add its own (see [schema as data](concepts.html#schema) below).

| Kind | What it holds |
|---|---|
| `task` | A unit of work. Subtasks are tasks with a parent. A task can say what kind of work it is (design, implementation, fix, test, review and others), who is assigned, its acceptance criteria and which files it owns. |
| `doc` | A document: a plan, a section of a plan, a report or a patch. Sections are docs with a parent, kept in order. |
| `note` | Descriptive knowledge: a note, a hazard, a lesson, a checkpoint or a summary. A hazard is a known danger, such as a trap in the build; it can record its symptom, its mechanism and the defence against it. |
| `rule` | A normative statement ("never do X"), with its strength (must or should), where it applies and why. |
| `decision` | A decision record: the context, what was decided, why, the trade-off and the rejected alternatives. It is never edited after acceptance, only superseded by a newer decision. |
| `question` | A question for the project owner, the orchestrator or the architect, with its options and the answer, kept verbatim. |
| `finding` | A review remark, a root cause or an observation, with a severity and a required failure scenario: what goes wrong, and when. |
| `verdict` | The result of a gate given by a role: pass, pass with conditions, fail, and to whom the work returns. Its fields cannot change after it is written. |
| `measurement` | A number with its context: the metric, value, unit and target, the command that produced it, the git commit it was measured on, and the environment it was measured in. |
| `artifact` | A file node: one logical project file, followed across moves and edits. [File links](file-links.html) point at it. |
| `run` | One run of an agent or a scripted workflow, with the artifacts it is expected to produce. |
| `lane` | A parallel line of work: a git worktree, its git branch and base commit, bound to a Moirai branch. |
| `area` | A scope: a subsystem, a directory or a topic. Knowledge can be scoped to an area. |

A shared resource that only one agent may use at a time, such as a benchmark slot or a merge queue, is a `task` whose work kind is `mutex`. It uses the same [claims](concepts.html#leases) as any task.

### Schema as data {#schema}

The schema itself is data in the graph. It is versioned on each branch, merged and exported like everything else. A project may add kinds, fields (also on the core kinds), values of enumerations, edge kinds and named queries. An addition applies at once. Changing or retiring a kind, field, enumeration value or edge kind needs an explicit migration (`moirai migrate`) on the branch. Named queries are defined, changed and dropped without one. No project item may change a core kind, field or edge kind.

## Typed fields {#fields}

Every field has a type from a small, closed set: boolean, integer, counter, floating-point number, enumeration, text, set, a reference to another node, a reference to a commit, and three types used by file links (a path, a content id and a recorded directory move). The type decides how a value is checked when it is written and how it merges when branches meet. For example, the increments to a counter on both branches are added up, items added to a set on either branch are kept, and text is merged line by line against the common base.

All kinds share a set of common fields: the text of the node, its status, a priority from `P0` to `P4`, a criticality, a confidence, an authority, its place in a hierarchy, labels, a few flags and two timestamps.

<details>
<summary>The common fields in detail</summary>

| Field | Values and meaning |
|---|---|
| `title`, `abstract`, `body` | The text of the node. |
| `status`, `resolution` | The kind's status (next section) and, for a closed task or a finding, how it ended: `completed`, `wontdo`, `duplicate`, `superseded`, `obsolete` or `rework`. |
| `priority` | `P0` (highest) to `P4`; used for scheduling. |
| `criticality` | `critical`, `high`, `normal` or `low`; used for the order in which things are shown. |
| `confidence` | `verified`, `observed`, `inferred` or `speculative`; a finding uses `confirmed` or `plausible`. |
| `authority` | Who stands behind the content: `owner`, `orchestrator`, `measured`, `research` or `agent`. |
| `parent`, `order` | The place of the node in a hierarchy, and its position among its siblings. |
| `labels` | A set of free labels. |
| `pinned`, `archived`, `frozen` | Flags set explicitly. |
| `defer_until`, `due` | Times: a task deferred until a moment is not ready before it. |
| `reason` | Why a node was moved into a side state, for example why it was retracted. |

</details>

Each kind adds its own fields on top, as the table of kinds describes: `acceptance` and `files_owned` on a task, `failure_scenario` and `severity` on a finding, `metric`, `value` and `measured_on` on a measurement, and so on.

### The virtual `done` field {#virtual-done}

Tasks, questions and verdicts have a boolean field `done` that is not stored. It is read from the status: a task is done when its status is `done` or `cancelled`, a question when an answer to it is visible, a verdict when it is `accepted`. Writing `done = true` performs the status transition, with all its checks. Keeping the same fact twice, once as a status and once as a flag, is how a tracker ends up giving two different answers to "is this finished?". Moirai keeps it once.

In Lachesis, completing a task under a claim looks like this:

```lq
TX ON lane/keepalive LEASE 'L-18' { MATCH (t {id: #12}) WHERE t.status = 'in_progress' EXPECT 1 SET t.done = true }
```

The block is one commit. `ON` names the branch the write lands on, here the branch of a lane (branches and lanes are explained in [the chapter on versions](versions.html)). `EXPECT 1` says that exactly one node must match, and `LEASE` presents the claim the writer holds on the task (see [leases](concepts.html#leases) below). If the task has an unfinished subtask, the write is refused and nothing changes.

## Statuses and guarded transitions {#statuses}

Each kind has its own statuses. Most have a main line, along which work moves forward, and side states, which step off that line.

| Kind | Main line | Side states |
|---|---|---|
| `task` | `open` → `in_progress` → `done` | `deferred`, `cancelled`, `frozen` |
| `doc` | `draft` → `current` | `superseded`, `archived` |
| `note` | `active` | `superseded`, `retracted`, `archived` |
| `rule` | `proposed` → `active` | `superseded`, `retracted`, `archived` |
| `decision` | `proposed` → `accepted` | `rejected`, `superseded` |
| `question` | `open` → `answered` | `dropped` |
| `finding` | `open` → `confirmed` → `fixed`; or `open` → `refuted`, `deferred` or `withdrawn` | none |
| `verdict` | `open` → `accepted` | `superseded` |
| `measurement` | `current` | `moved_declared`, `retracted` |
| `artifact` | `planned` → `present` | `removed` |
| `run` | `running` → `green`, `red`, `stopped` or `died` | none |
| `lane` | `active` → `ready_to_merge` → `merge_pending` → `merged` | `frozen`, `abandoned`, `measuring` |
| `area` | `active` | `archived` |

Each kind has a status machine: a write may make only the transitions it lists, and any other transition is refused. Some transitions also have guards, conditions checked at the moment of the write:

- A task cannot become done while one of its subtasks is unfinished, or while a failing verdict gates it.
- A question becomes answered only together with an answer text and an `answers` edge from a decision or a note.
- A performance or complexity finding becomes fixed only when another actor has addressed it and a review verdict verifies it. A passing test alone is not a re-review.
- A run closes green only when every artifact it was expected to produce exists and was read back.
- A done task goes back to open only through an explicit `reopen` with a reason, which also counts the reopening. A merge never reopens a task by accident.
- Knowledge is replaced, not overwritten. A note, rule, decision or doc becomes `superseded` only together with a `supersedes` edge from its replacement, and a note or rule is retracted only with a reason.

When a guard refuses a write, nothing is written, and the error names what stood in the way, for example the unfinished subtasks or the gating verdict. When branches merge, statuses combine by their order: a task that is `done` on one branch and `in_progress` on the other is `done`, while a side state such as `cancelled` against `done` is a conflict that someone must resolve (see [the chapter on versions](versions.html)).

## Edges {#edges}

An edge connects two nodes and has a kind, such as `blocks`. It has a direction and reads in that direction: `(#11)-[:BLOCKS]->(#12)` reads "#11 must finish before #12 starts". The same edge between the same two nodes exists at most once. Moirai stores every edge in both directions in the same commit, so "who points at #40?" is as cheap to answer as "what does #40 point at?".

The core schema has 25 edge kinds in two classes.

- **Structural** edges carry rules the engine enforces, such as the order of work and the hierarchy. Both ends must be live nodes; the only exception is described under [deletion](concepts.html#deletion).
- **Historical** edges record where something came from or what it is about. They may keep pointing at a node after it is deleted.

| Purpose | Edge kinds | Meaning |
|---|---|---|
| Order of work (structural) | `parent`, `blocks`, `gates`, `depends_on`, `merge_after` | `parent`: a subtask points to its task (also used for doc sections and areas). `blocks`: the source, a task or a question, must be finished before the target task starts. `gates`: a verdict holds back the completion of a task while it is open and failing. `depends_on`: one plan section depends on another. `merge_after`: one lane merges after another. |
| Scope and identity (structural) | `answers`, `scoped_to`, `duplicate_of`, `runs_in` | A decision or note answers a question; knowledge is scoped to an area; a node duplicates a canonical one; a run runs in a lane. |
| History of knowledge (historical) | `supersedes`, `derived_from`, `cites`, `implements`, `contradicts` | A newer node replaces an older one of the same kind (at most one active replacement); a note, doc, verdict or file was derived from a source; a node cites knowledge; a task or file implements a decision or a plan section; two rules contradict each other. |
| Review and evidence (historical) | `refutes`, `confirms`, `verifies`, `addresses`, `about` | A finding or measurement refutes or confirms a finding, decision or rule; a measurement or verdict verifies a finding, task or decision; a task or file addresses a finding; a finding, verdict, measurement, question, rule, decision or note is about some node. |
| Origin of work (historical) | `discovered_from`, `produced`, `consumed` | New work was discovered while doing a task; a run wrote or read a node or file. |
| Loose links (historical) | `mentions`, `relates` | `mentions` is written automatically from `#N` in text; `relates` is a plain association. |
| Files (historical) | `at` | A link from any node to a file node, with one or more anchors that say where in the file. See [the chapter on file links](file-links.html). |

The engine keeps the order of work free of loops. The `blocks` and `gates` edges, together with the hierarchy, may never form a cycle, and a write that would close one is refused. A task cannot block its own subtask. The hierarchy is a forest in which no node has more than twelve ancestors.

## Derived state: computed, never stored twice {#derived-state}

Some answers follow from the graph rather than from anything a person or agent wrote: which tasks can start, which are blocked, how many subtasks are done. Moirai computes them in the engine. Each one is defined once and used everywhere: by the write path, by every command and by Lachesis. No agent can write them, so no hand-made copy can go stale. The engine keeps some of them in fast indexes, but those indexes are caches it maintains itself, and tests check that they always equal a full recomputation by the separate reference model.

| State | Meaning |
|---|---|
| `unblocked` | An open task that is not a container, has no unresolved merge conflict, has no unfinished blocker and has no ancestor held back by a blocker from outside its subtree. This is valid on any version, also in the past. |
| `ready` | Unblocked, and also: no other agent holds a lease on it, no other branch has completed or deleted it without this branch having caught up, and its defer time has passed. This is the ready queue that `moirai ready` prints. It exists only at the tip of a branch, because leases are live state. |
| `blocked` | An unfinished task with an unfinished blocker, its own or one inherited from an ancestor. |
| `is_blocker` | An unfinished node that blocks an unfinished task. "Which tasks block anything?" is answered from one index. |
| rollups | For each task, the number of direct subtasks and how many of them are done. |
| `container`, `ready_to_close` | A task with subtasks is a container. A container is never ready as work itself. It is ready to close when all its subtasks are done. |
| `suspect` | A node built on something that changed: a source it was derived from, cites or implements, or a section it depends on, was retracted, superseded or deleted; or a file it links to was removed or deleted. |
| `conflicted` | The node holds an unresolved merge conflict on this branch. It leaves the ready queue until someone resolves it. |
| `answered` | A question with a visible `answers` edge. |

`unblocked` and `blocked` are not opposites. `unblocked` holds only for an open task that could be handed out as work, so a task in progress, a container or a conflicted task that has no blockers is neither unblocked nor blocked.

<figure class="diagram">
<svg viewBox="0 0 720 310" role="img" aria-labelledby="concepts-d1-t">
  <title id="concepts-d1-t">A small task graph: one parent task with five subtasks, a chain of blocks edges, and the two ready subtasks highlighted</title>
  <defs><marker id="concepts-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path class="d-arrow" d="M0,0 L10,5 L0,10 z"/></marker></defs>
  <path class="d-box" d="M218,16 H502 A8,8 0 0 1 510,24 V64 A8,8 0 0 1 502,72 H218 A8,8 0 0 1 210,64 V24 A8,8 0 0 1 218,16 Z"/>
  <text class="d-text-strong" x="360" y="36" text-anchor="middle" dominant-baseline="middle">#10 Ship keep-alive</text>
  <text class="d-text-small" x="360" y="58" text-anchor="middle" dominant-baseline="middle">container: 1 of 5 subtasks done</text>
  <path class="d-edge-dashed" d="M68,150 L250,72" marker-end="url(#concepts-arrow)"/>
  <path class="d-edge-dashed" d="M214,150 L305,72" marker-end="url(#concepts-arrow)"/>
  <path class="d-edge-dashed" d="M360,150 L360,72" marker-end="url(#concepts-arrow)"/>
  <path class="d-edge-dashed" d="M506,150 L415,72" marker-end="url(#concepts-arrow)"/>
  <path class="d-edge-dashed" d="M652,150 L470,72" marker-end="url(#concepts-arrow)"/>
  <path class="d-box-muted" d="M20,150 H116 A8,8 0 0 1 124,158 V218 A8,8 0 0 1 116,226 H20 A8,8 0 0 1 12,218 V158 A8,8 0 0 1 20,150 Z"/>
  <text class="d-text-strong" x="68" y="170" text-anchor="middle" dominant-baseline="middle">#11</text>
  <text class="d-text-small" x="68" y="190" text-anchor="middle" dominant-baseline="middle">Parse header</text>
  <text class="d-text-small" x="68" y="210" text-anchor="middle" dominant-baseline="middle">done</text>
  <path class="d-box-accent" d="M166,150 H262 A8,8 0 0 1 270,158 V218 A8,8 0 0 1 262,226 H166 A8,8 0 0 1 158,218 V158 A8,8 0 0 1 166,150 Z"/>
  <text class="d-text-strong" x="214" y="170" text-anchor="middle" dominant-baseline="middle">#12</text>
  <text class="d-text-small" x="214" y="190" text-anchor="middle" dominant-baseline="middle">Reuse sockets</text>
  <text class="d-text-small" x="214" y="210" text-anchor="middle" dominant-baseline="middle">ready</text>
  <path class="d-box" d="M312,150 H408 A8,8 0 0 1 416,158 V218 A8,8 0 0 1 408,226 H312 A8,8 0 0 1 304,218 V158 A8,8 0 0 1 312,150 Z"/>
  <text class="d-text-strong" x="360" y="170" text-anchor="middle" dominant-baseline="middle">#13</text>
  <text class="d-text-small" x="360" y="190" text-anchor="middle" dominant-baseline="middle">Idle timeout</text>
  <text class="d-text-small" x="360" y="210" text-anchor="middle" dominant-baseline="middle">blocked by #12</text>
  <path class="d-box" d="M458,150 H554 A8,8 0 0 1 562,158 V218 A8,8 0 0 1 554,226 H458 A8,8 0 0 1 450,218 V158 A8,8 0 0 1 458,150 Z"/>
  <text class="d-text-strong" x="506" y="170" text-anchor="middle" dominant-baseline="middle">#14</text>
  <text class="d-text-small" x="506" y="190" text-anchor="middle" dominant-baseline="middle">Write tests</text>
  <text class="d-text-small" x="506" y="210" text-anchor="middle" dominant-baseline="middle">blocked by #13</text>
  <path class="d-box-accent" d="M604,150 H700 A8,8 0 0 1 708,158 V218 A8,8 0 0 1 700,226 H604 A8,8 0 0 1 596,218 V158 A8,8 0 0 1 604,150 Z"/>
  <text class="d-text-strong" x="652" y="170" text-anchor="middle" dominant-baseline="middle">#15</text>
  <text class="d-text-small" x="652" y="190" text-anchor="middle" dominant-baseline="middle">Update docs</text>
  <text class="d-text-small" x="652" y="210" text-anchor="middle" dominant-baseline="middle">ready</text>
  <path class="d-edge" d="M124,188 L158,188" marker-end="url(#concepts-arrow)"/>
  <path class="d-edge" d="M270,188 L304,188" marker-end="url(#concepts-arrow)"/>
  <path class="d-edge" d="M416,188 L450,188" marker-end="url(#concepts-arrow)"/>
  <path class="d-edge-dashed" d="M24,262 L64,262" marker-end="url(#concepts-arrow)"/>
  <text class="d-text-small" x="76" y="262" text-anchor="start" dominant-baseline="middle">parent: a subtask points to its parent</text>
  <path class="d-edge" d="M24,288 L64,288" marker-end="url(#concepts-arrow)"/>
  <text class="d-text-small" x="76" y="288" text-anchor="start" dominant-baseline="middle">blocks: source finishes before target starts</text>
  <path class="d-box-accent" d="M404,253 H436 A4,4 0 0 1 440,257 V267 A4,4 0 0 1 436,271 H404 A4,4 0 0 1 400,267 V257 A4,4 0 0 1 404,253 Z"/>
  <text class="d-text-small" x="452" y="262" text-anchor="start" dominant-baseline="middle">ready: computed by the engine</text>
  <path class="d-box-muted" d="M404,279 H436 A4,4 0 0 1 440,283 V293 A4,4 0 0 1 436,297 H404 A4,4 0 0 1 400,293 V283 A4,4 0 0 1 404,279 Z"/>
  <text class="d-text-small" x="452" y="288" text-anchor="start" dominant-baseline="middle">done: no longer blocks anything</text>
</svg>
<figcaption>Task #10 has five subtasks (dashed parent edges). #11 is done, so #12, which it blocked, is ready. #13 waits for #12, and #14 waits for #13. #15 has no blockers and is ready too. #10 is a container, so it is never handed out as work. Apart from "done", which is a status someone set, every label here is derived by the engine from statuses and edges. (Ready also assumes that no other agent holds a lease on the task.)</figcaption>
</figure>

Two details matter in practice. First, blockers flow down the hierarchy: if a task outside #10 blocked #10, every subtask of #10 would be held back too, and none of them would be ready. Second, a `gates` edge is different from `blocks`: a failing verdict stops a task from being *completed*, but not from being *claimed*, so an agent can still pick up the task and fix what the verdict found.

An agent never has to work out readiness from raw edges. It asks:

```lq
MATCH (t:task) WHERE t.ready AND t IN subtree(#10) RETURN t ORDER BY t.priority LIMIT 10
```

In the graph above this query would return #12 and #15. The built-in command `moirai ready` runs the same definition. On a past version of the graph, `ready` does not exist, because leases are live state; a query there uses `unblocked` instead.

## Rules, critical notes and authority {#rules-and-authority}

Rules and notes keep a project's lessons alive between sessions. Their fields decide who sees them and how much weight they carry.

`criticality`
: A note or rule marked `critical` sorts first in every brief and every context pack. A critical rule keeps its full text in a pack and is never cut to a one-line reference, except that a pack lists only the ids of the critical rules that a hook already showed the agent when it started.

`applies_to`
: Where the knowledge applies, as a list of roles (`role:tester`), phases of the work (a context pack is requested for one role and one phase), lanes and file paths (`path:src/net/**`). An empty list means everywhere. A context pack for a task includes the rules that match its role, phase and lane, and the hazards whose paths overlap the files the task owns.

`authority`
: Who stands behind the statement, from `owner` (the human in charge of the project) through `orchestrator`, `measured` and `research` down to `agent`. Packs order rules by criticality, then by authority. A rule with owner authority must carry the owner's own words, verbatim, in its `owner_quote` field, and only the roles that the write policy allows may set owner authority.

Provenance comes from the commit. Every change belongs to exactly one commit, which records the actor, its role, the session, the Moirai branch, the git head, branch and worktree it worked in, and a message. "Who wrote this rule, in which lane, against which code?" always has an answer.

Branches do not hide rules. A critical rule written on `main` that a lane has not merged yet still appears in that lane's packs, marked `~main`, until the lane syncs with `main`.

Knowledge is replaced rather than edited away. A newer rule supersedes the old one, a finding refutes another, and two rules that disagree can be linked with `contradicts`, so that packs show them side by side. Text that an agent pipeline proposes, for example a decision captured at the end of a task, enters with the status `proposed` and its provenance, and becomes active only through an explicit status change.

## Deletion: everyone that referenced it knows {#deletion}

Deleting a node from a graph raises a question that text files never answer: what happens to everything that pointed at it? Moirai answers it for each edge kind, inside the same commit as the delete.

| When the deleted node is | What happens |
|---|---|
| the parent of subtasks | The delete is refused, unless you ask to delete the whole subtree (`--cascade`) or to move the subtasks up one level (`--reparent`). |
| a blocker of other tasks (the source of `blocks` or `gates`) | With `--replaced-by`, the edge is re-pointed to the replacement. Otherwise, if the deleted node was still an unfinished blocker or a failing verdict, the edge stays, **flagged**. A flagged `blocks` edge keeps the dependent task and its subtasks out of the ready queue until someone resolves the edge. A flagged `gates` edge does not change readiness, but the task cannot be completed until the edge is resolved. A deleted blocker never silently unblocks anything. |
| a node that answered a question | The `answers` edge is dropped. If it was the question's last answer, the question reopens. |
| a question with an answer, an area with knowledge scoped to it, a canonical node with duplicates, or a lane with runs | The delete is refused while those references exist. For an area, the delete option `--reassign` moves the knowledge scoped to it to the parent area in the same commit. |
| the target of a historical edge | The edge stays and now points at the tombstone. If the edge is `derived_from`, `cites`, `implements` or `at`, its source becomes `suspect`. |

A delete is one commit. The engine walks every edge of the deleted node, applies each policy, and writes the tombstone, a `deleted` marker and a change-feed entry that lists every affected node. All of it lands, or none of it does. `moirai rm 40 --dry-run` prints the impact and writes nothing. `moirai rm` refuses while any agent holds a live lease on the node, on any branch, unless you also release the lease.

### Tombstones {#tombstones}

A deleted node does not vanish. Its row stays, marked deleted, with its kind, its title, the deleting commit and who made it, the reason and the replacement. History keeps the full content from before the delete, so reverting the deleting commit brings the node back. Its `#N` is never reused, and the store never removes a tombstone. A reference from a live record shows the id as `#40(deleted)`. A query that names the deleted node by its id returns no rows, only a notice that carries the tombstone: the revision and commit of the delete, who made it, the reason and the replacement.

```text
N01: #40 is deleted in this view (rev 4471 c3f9a2b10 by dev-2 "dup of #52" -> #52)
```

### How everyone finds out {#who-finds-out}

1. The delete commit itself updates every referrer on its branch at once.
2. Every Moirai process checks the head of the store before each operation, so the next read by any agent, in any process, sees the tombstone.
3. The change feed and the hook deltas tell the agents that claimed the node, are blocked on it or cited it.
4. A write that still assumes the old state, for example an update guarded by an old revision number, is refused and prints the current value.

Other branches are not changed by the delete until they merge it, as in git. But the `deleted` marker keeps the node out of their ready queues and claims in the meantime, so no lane hands out work that another lane has deleted. The [chapter on versions](versions.html) covers deletes across branches.

## Leases and claims: who works on what {#leases}

Before an agent works on a task, it *claims* it. A claim creates a [lease](glossary.html#term-lease): a record that says "this holder is working on this node now". The task must be live and ready on the claimer's branch. A lease is not part of the versioned graph. It lives at store level, every branch sees it, and it is never merged or exported. That is what stops two lanes from building the same task at the same time. Claiming does not change the task's status by itself; the first write under the lease, or the `--start` option, moves it from `open` to `in_progress`.

- **Lifetime.** A claim an agent makes for itself lasts 15 minutes by default, a configurable value. Writes that present the lease keep it alive, and long work without writes can send a heartbeat. A dispatcher can instead claim tasks for a whole run of a workflow; such a lease ends when the run's results are applied, when the run is closed or when it is reclaimed explicitly. Moirai checks whether the session that holds a lease is still alive when it reads the lease, with no polling process, and an unclear answer never ends a lease early.
- **Fencing.** Each lease carries a fencing token, a number from a store-wide counter that only grows. Completing the task, writing under the lease and releasing it all present the token. If the lease was lost, for example it expired and another agent claimed the task, the old holder's write is refused and the error names the current holder. A slow agent that wakes up late cannot overwrite the work of the agent that took over.
- **Completion.** `moirai complete` marks the task done on the lease's branch, releases the lease and returns the tasks that became ready. The completion also leaves a `settled` marker, so branches that have not merged the completion yet never hand the task out again.
- **Guards.** Besides leases, any write can carry compare-and-set guards: the revision, status or lease holder it expects to find. A failed guard writes nothing and prints the current value, along with what changed it and who.

## The change feed {#change-feed}

Every commit gets a sequence number from a store-wide counter, and the header line of a result shows the revision it was read from. Each commit records its changes and a list of the nodes it affected: dependents that became unblocked, referrers of a new tombstone, sources that became suspect, and lease and marker events. The result of a write also lists the tasks that became ready. `moirai changes --since 4400` prints one line per change since that point, and can filter them to what matters to one agent: the nodes it claimed, is blocked on, wrote or cited.

Nothing pushes changes, and nothing polls. There is no file watcher and no background process. The feed is read at natural boundaries instead: when the optional hooks are installed, they read it when the user submits a prompt and when a subagent starts, and add a short summary of the relevant changes to the agent's context (by default at most 600 bytes per prompt).

## Where to go next {#next}

- [A tour](tour.html) shows these concepts in a designed session.
- [The chapter on versions](versions.html) explains branches, merges and how completions and deletions travel between lanes.
- [The chapter on Lachesis](lachesis.html) covers queries such as the one above.
- The authoritative data model is in the specification: [docs/spec/format/08-data-model.md](https://github.com/bluesteelll/moirai/blob/master/docs/spec/format/08-data-model.md) on GitHub.
