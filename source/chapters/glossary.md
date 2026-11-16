---
title: Glossary
nav: Glossary
description: Every term this book uses, from absent value to writer tree, each with a short definition and a link to the chapter where it matters.
---

# Glossary

Every term this book defines or links, in alphabetical order. Each entry gives a short definition and links to the chapter where the term matters. Terms written in `code` are typed exactly as shown, in commands, queries or files.

Some terms belong to the way the project itself is built, not to the product, such as unit, tier or red-first; their entries say so. Like the rest of the book, the entries describe the designed system.

## Terms {#terms}

Absent value {#term-absent-value}
: In Lachesis, the value of an optional field that is not set. Lachesis uses two-valued logic: a comparison with an absent value is false and its negation is true, so `t.assignee <> 'dev#1'` also returns unassigned tasks. See [The absent value and two-valued logic](lachesis.html#absent-values).

Accelerator {#term-accelerator}
: Something that saves a call or keeps an agent more up to date, but that nothing depends on. Every [hook](#term-hook) of Moirai is an accelerator with a fallback: without it an agent loses some freshness or makes one more call, never correctness. See [Hooks](agents.html#hooks).

Alias {#term-alias}
: A former path of a [file node](#term-file-node). An alias still resolves old mentions of a moved file, but it never captures a new file at that path. The word also appears in [reverse alias](#term-reverse-alias) and in the alias map of the git image's side ref. See [File nodes keep their identity](file-links.html#file-identity).

Anchor {#term-anchor}
: The part of a [file link](#term-file-link) that says where in the file: the whole file, or one place inside it such as a span of lines, a Rust item or a Markdown section. It is stored as quoted text with its context, a window of line hashes and an enclosing scope, never as a bare line number. See [What an anchor records](file-links.html#what-an-anchor-records).

`applies_to` {#term-applies-to}
: A field of rules and notes that says where they apply: a list of roles, phases, lanes and file paths, such as `role:tester` or `path:src/net/**`. An empty list means everywhere. A context pack uses it to choose the rules and hazards for a task. See [Rules, critical notes and authority](concepts.html#rules-and-authority).

`at` edge {#term-at-edge}
: The edge kind of a file link: it goes from any node to a [file node](#term-file-node) and carries one or more [anchors](#term-anchor). See [What a link is](file-links.html#what-a-link-is).

Author role {#term-author-role}
: In the project's own build process, a role that owns a fixed set of paths in the repository, such as the author of the reference model or of the test fixtures. Separation rules fix who may write what and who must not read what; for example, the model's author never reads engine code. See [Built by AI agents under a strict process](building.html#agent-process).

Authority {#term-authority}
: A field that says who stands behind a statement: `owner`, `orchestrator`, `measured`, `research` or `agent`. Context packs order rules by [criticality](#term-criticality), then by authority, and a rule with owner authority must quote the owner's own words. See [Rules, critical notes and authority](concepts.html#rules-and-authority).

Bare repository {#term-bare-repository}
: A git repository with no working tree, only git's own data. By default the [git image](#term-git-image) is written to a separate bare repository next to the project, such as `/work/tinyhttp-moirai.git`. See [Where the image goes](git-image.html#destination).

BLAKE3 {#term-blake3}
: A fast cryptographic hash function. A Moirai commit id is the BLAKE3-256 hash of the commit's [canonical form](#term-canonical-form), and long text bodies are addressed by a 128-bit BLAKE3 hash. See [Content addressing](engine.html#content-addressing).

Blocked {#term-blocked}
: [Derived state](#term-derived-state) of an unfinished task that has an unfinished [blocker](#term-blocker), its own or one inherited from an ancestor. `moirai tree` marks such tasks `BLOCKED`. See [Derived state](concepts.html#derived-state).

Blocker {#term-blocker}
: A task or question that must be finished before another task starts: the source of a `blocks` edge. A deleted blocker never silently unblocks anything; its edge stays as a [flagged edge](#term-flagged-edge). "Blocker" is also the most severe value of a finding's `severity` field (`blocker`, `important`, `optional`), which the project's own specification reviews use too. See [Edges](concepts.html#edges).

Branch {#term-branch}
: A named line of history of the whole graph. Every node, field, edge, body and the schema itself are versioned per branch; `main` is an ordinary branch that cannot be deleted, and other names belong to families such as `lane/`, `plan/` and `tags/`. Moirai branches are its own and are never created because of git branches. See [Branches and refs](versions.html#branches).

Brief {#term-brief}
: A digest of the whole store for the start of a session: live lanes and leases, runs in flight, open questions, critical rules, new verdicts, broken file links and the number of records waiting for review, within 8,000 bytes by default. With hooks it reaches the agent before its first turn; without them the agent's first call is `moirai brief`. See [Context packs and briefs](agents.html#packs-and-briefs).

Budget {#term-budget}
: A fixed limit. Moirai uses the word in three ways: performance budgets, which are design targets such as at most 5 ms for a read command; byte budgets for what reaches an agent's context, such as a pack or an error; and counted budgets that bound the work of one Lachesis query. See [Budgets](engine.html#budgets) and [Budgets and exit code 10](lachesis.html#budgets).

Built-in {#term-built-in}
: A property or function of Lachesis that exposes the engine's own state, such as `t.ready`, `t.unblocked`, `subtree(n)`, `blockers()` or `link_state()`. Built-ins share code with the write path, so a query and a command cannot disagree about what a word means. See [The engine's state as built-ins](lachesis.html#built-ins).

Bundle {#term-bundle}
: A single file in git's bundle format that holds commits and refs. `moirai image export` and `image import` can write to and read from a bundle file instead of a repository, with no git installed. See [Export, import, push and pull](git-image.html#export-import).

Canonical form {#term-canonical-form}
: A specified byte sequence for a piece of data, so that every implementation writes exactly the same bytes for the same content. A commit's canonical form covers its kind, parents, clock value, author, git context, message and changes, and its hash is the commit id; node files of the git image are canonical text too. See [Content addressing](engine.html#content-addressing).

CAS guard {#term-cas-guard}
: A compare-and-set guard: a condition that a write checks against the branch tip it would land on, in the same step as the write, such as the revision, status or lease holder it expects (`--if-rev`, `--if-status`, `--if-holder`). If the guard fails, nothing is written, and the command exits with code 4 and prints the current value and who changed it. See [A write that lost the race](tour.html#guarded-write).

Chained trailer {#term-chained-trailer}
: A short checksum value at the end of each group of records in the [log](#term-log), chained to the value of the group before it. Before a writer reports success, it reads its own chained trailer back, so a group lost through another process's failed flush is found and retried instead of being reported as committed. See [One flush for many writers](engine.html#group-commit).

Change feed {#term-change-feed}
: The store-wide list of changes, one line per change: revision, branch, node, operation and who made it, plus lease and marker events. `moirai changes --since N` reads it, and hooks turn it into a short list of relevant changes before each prompt; nothing pushes it and nothing polls it. See [The change feed](concepts.html#change-feed).

Checkpoint {#term-checkpoint}
: Two related meanings. In the engine, a checkpoint folds the newest part of the log, the [tail](#term-tail), into a new delta [segment](#term-segment), so that opening a store stays cheap. In the git image, a checkpoint commit is one git commit that holds a branch's state at the moment of an export and folds the commits in between; it is the default export granularity. See [Checkpoints, rollups and garbage collection](engine.html#maintenance) and [Commits and trailers](git-image.html#commits).

Claim {#term-claim}
: Taking a task to work on it: `moirai claim ID`, or the MCP `claim` tool, creates a [lease](#term-lease) on the task. The task must be live and ready on the claimer's branch. Claiming alone does not change the status; `--start` or the first write under the lease moves the task to `in_progress`. See [Leases and claims](concepts.html#leases).

Combined landing {#term-combined-landing}
: In the project's own build process, a set of units that must be merged together because a test of one reads the text or bytes of another, such as test fixtures and the oracle that decodes them. The set is tested as one and merged at once, or not at all. See [Built by AI agents under a strict process](building.html#agent-process).

Compare-and-set guard {#term-compare-and-set-guard}
: The full name of a [CAS guard](#term-cas-guard).

Conflict value {#term-conflict-value}
: What a merge stores on a key when its rule cannot decide, for example when both branches set a field to different values: the class of the conflict and the three sides, `base`, `ours` and `theirs`. The merge still lands; the node is marked [conflicted](#term-conflicted) and leaves the ready queue until someone runs `moirai resolve`. See [Conflicts are data](versions.html#conflicts).

Conflicted {#term-conflicted}
: [Derived state](#term-derived-state) of a node that holds an unresolved [conflict value](#term-conflict-value) on this branch. A conflicted task leaves the ready queue until the conflict is resolved. See [Derived state](concepts.html#derived-state).

Container {#term-container}
: A task that has subtasks. A container is never ready as work itself, so no agent is handed it directly; it is [ready to close](#term-ready-to-close) when all its subtasks are done. See [Derived state](concepts.html#derived-state).

Content addressing {#term-content-addressing}
: Naming data by a hash of its content. A Moirai commit id is a [BLAKE3](#term-blake3) hash of the commit's canonical form, so two stores that hold the same commit compute the same id, and an identical text body is stored only once. See [Content addressing](engine.html#content-addressing).

Context pack {#term-context-pack}
: The context Moirai computes for one task and one role: the rules that apply, the task with its ancestors and acceptance criteria, the relevant plan sections, findings, measurements, scoped notes and recent changes, fitted into a byte budget per role (provisionally 16,000 bytes for a developer). Entries are shortened before any is dropped, and anything dropped is counted on the first and the last line, with the command that fetches it. See [Context packs and briefs](agents.html#packs-and-briefs).

Coverage index {#term-coverage-index}
: A file of the specification that maps every frozen item to the sections that specify it, the fixtures that exercise it and the reference-model function that implements it. See [Reviews and named holes](building.html#reviews-and-holes).

Crash-point enumeration {#term-crash-point-enumeration}
: A test that injects a crash at every write, flush and publish point of the storage code on a simulated file system, and tries every subset of the unflushed sectors (when there are at most 12) and every combination of the two [HEAD slots](#term-head-slot). After each crash, recovery must keep every acknowledged commit. See [The test workstream](building.html#test-workstream).

Criticality {#term-criticality}
: A field with the values `critical`, `high`, `normal` and `low`. A note or rule marked `critical` is shown first in every brief and context pack, and always with its full text. See [Rules, critical notes and authority](concepts.html#rules-and-authority).

Cursor {#term-cursor}
: A marker that says where to continue. A long Lachesis result ends with a cursor that fetches the next page, and each agent session keeps a cursor in the [change feed](#term-change-feed), so that hooks show only the changes since its last look. See [WHERE, RETURN, ORDER BY and LIMIT](lachesis.html#clauses).

Deleted marker {#term-deleted-marker}
: A store-level record written when a node is deleted on a branch. Until another branch has received the delete, the marker keeps the node out of that branch's ready queue, blocker lists and claims, so no lane hands out work that another lane deleted. Also called a deletion marker. See [Coordination across branches](versions.html#coordination).

Deletion marker {#term-deletion-marker}
: Another name for a [deleted marker](#term-deleted-marker).

Derived state {#term-derived-state}
: Answers that follow from the graph rather than from anything someone wrote, such as [ready](#term-ready), [blocked](#term-blocked), rollups and [suspect](#term-suspect). The engine computes each one from a single definition shared by the write path, every command and Lachesis; no agent can write it, and the git image never stores it. See [Derived state](concepts.html#derived-state).

Designated tree {#term-designated-tree}
: The one working tree that a branch is paired with for file links. Each branch has at most one designated tree, and each tree serves at most one branch: a lane's tree is the worktree that `moirai lane open` names, `main`'s is set by the configuration key `files.main-tree`, and any other branch's by `moirai worktree bind`. While the tree's git HEAD is on the branch's git line, it is the branch's [writer tree](#term-writer-tree). See [Settling](file-links.html#settling).

diff3 {#term-diff3}
: A line-by-line three-way merge of text against a common base text. Moirai uses it for text fields and bodies; when both sides changed the same lines, the result is a `TextHunk` [conflict value](#term-conflict-value). See [The typed three-way merge](versions.html#merge).

Differential testing {#term-differential-testing}
: Running the same inputs through two independent implementations and comparing the results. The engine is compared with the [reference model](#term-reference-model), at least a million commands per milestone, and every exported image is checked with the git command-line tool. See [The test workstream](building.html#test-workstream).

Dispatcher {#term-dispatcher}
: The pattern an orchestrator follows to run many workers: open a [run](#term-run), claim the tasks in bulk as run-scoped leases, start each worker with a marker line in its prompt, and apply all results in one step with `moirai apply`. Running it again after a crash converges instead of repeating work. See [The dispatcher pattern](agents.html#dispatcher).

`DRY` {#term-dry}
: A keyword of a Lachesis write block: it runs every check without taking the writer lock and prints the changes that would be made and a digest of the target set. Applying the same block with `IF TARGETS` and that digest gives plan and apply. Commands such as `moirai rm` have the same idea as `--dry-run`. See [Writes: one TX block, one commit](lachesis.html#writes).

Durability class {#term-durability-class}
: The level of protection a kind of record gets on disk: `durable` (acknowledged only after a flush that covers it; every graph change), `lazy` (heartbeats, read cursors and similar records that a power loss may drop), and two classes for file sizes and directory entries. If a location cannot give a class, the store is refused there. See [Durability classes and quiet mode](engine.html#durability).

Edge {#term-edge}
: A typed, directed connection between two nodes, such as `blocks` or `parent`. The same edge between the same two nodes exists at most once, and Moirai stores every edge in both directions in the same commit. The core schema has 25 edge kinds, each [structural](#term-structural-edge) or [historical](#term-historical-edge). See [Edges](concepts.html#edges).

Endpoint pair {#term-endpoint-pair}
: What a variable-length path pattern in Lachesis binds: one row per pair of start and end nodes, not one per path, because the number of paths explodes on graphs with shortcuts. A `count(*)` over such a pattern counts endpoint pairs, and notice `N08` says so. See [Counting](lachesis.html#counting).

Epoch {#term-epoch}
: A random number chosen when a store is created and written into the header of every log record, so a record is valid only in its own store. A restore from backup gives the store a new epoch. See [History in a log, current state in segments](engine.html#log-and-segments).

Evidence {#term-evidence}
: What Moirai uses to find a file that moved. Exact evidence, such as the same file id with the same size and modification time, a recorded `file mv` intent or a git rename inside one commit, may [re-bind](#term-re-bind) a link automatically; weaker evidence, such as similar content, only proposes a candidate. `moirai file where ID --evidence` shows it. See [Following a file across a move and an edit](file-links.html#following-a-move).

`EXPECT` {#term-expect}
: A clause required on every `MATCH` target of a Lachesis write block: how many nodes it must match, as an exact count or a range. It is checked again under the writer lock; a mismatch writes nothing and exits with code 4. See [Writes: one TX block, one commit](lachesis.html#writes).

Fail-open {#term-fail-open}
: Built so that a failure lets the work go on. Moirai's hooks fail open: a hook that fails, times out or cannot reach Moirai does not stop the agent, and the context of that event is missing, never wrong. See [Hooks: optional accelerators](agents.html#hooks).

Fault model {#term-fault-model}
: The written list of failures the storage code must survive, enforced by the simulated file system in tests: lost or torn unflushed sectors, renames that are durable only after their directory is synced, failed flushes, a full disk, paused processes and jumps of the clock. See [The test workstream](building.html#test-workstream).

Fencing token {#term-fencing-token}
: A number from a store-wide counter that only grows, carried by every [lease](#term-lease). Completing a task, writing under its lease and releasing it all present the token, so an agent whose lease expired and was taken over cannot overwrite the new holder's work: its write is refused with exit code 5. See [Leases and claims](concepts.html#leases).

File link {#term-file-link}
: A pointer from a node to a project file or a place inside it: an [`at` edge](#term-at-edge) to a [file node](#term-file-node), with one or more anchors. It follows the file through renames, moves and edits, and when it cannot follow with certainty, it shows a [link state](#term-link-state) and a next step. See [File links that survive moves](file-links.html).

File node {#term-file-node}
: A node of kind `artifact` that stands for one logical project file. Its identity is fixed when the file is first registered; a move changes its current path, and former paths are kept as [aliases](#term-alias). See [File nodes keep their identity](file-links.html#file-identity).

Flagged edge {#term-flagged-edge}
: A `blocks` or `gates` edge whose source was deleted without a replacement. The edge stays, marked flagged, and keeps the dependent task out of the ready queue until someone resolves it, so a deletion never silently unblocks work. Flagged edges survive a round trip through the git image. See [Deletion](concepts.html#deletion).

Flush byte {#term-flush-byte}
: A second lock byte of the store's [`LOCK` file](#term-lock-file), used for [group commit](#term-group-commit). The writer that takes it flushes the log once, and that flush covers every group appended before it began. See [One flush for many writers](engine.html#group-commit).

Foreign commit {#term-foreign-commit}
: A git commit in the image that Moirai did not write, or whose trailers do not verify, such as a hand edit of a `.moi` file. On import, its changes become ordinary typed changes under a deterministic commit id, checked by the same validators as a merge. See [Edits made with git](git-image.html#foreign).

Fork {#term-fork}
: Creating a branch from a commit. A fork copies nothing: it records the fork point and keeps the snapshot the branch starts from, with a design estimate of about 2 ms. See [Forking is cheap](versions.html#cheap-forks).

Format oracle {#term-format-oracle}
: A test-only crate that decodes and re-encodes every frozen on-disk structure independently of the product code, starting from hand-written hex fixtures. Its author is not the author of the product's codec. See [The reference model](building.html#reference-model).

`format-v1` {#term-format-v1}
: The git tag that will freeze version 1 of the on-disk format. It ends milestone M0 and starts M1, the storage engine; after it, any change to a frozen byte reopens M0. The tag does not exist yet. See [Reviews and named holes](building.html#reviews-and-holes).

fs unit {#term-fs-unit}
: A counted unit of file-system work that bounds how much a read may do to compute link states (400 per query by default). Counting units instead of time makes the same tree state always stop at the same link; links not reached show `unverified (budget)`, and the command exits with code 10. See [Time budgets](file-links.html#time-budgets).

Fuzzing {#term-fuzzing}
: Testing a parser with large numbers of generated, often malformed inputs. Every parser in Moirai gets a fuzzer: log records and segments, `.moi` files, git objects, query text and command-line arguments. See [The test workstream](building.html#test-workstream).

Garbage collection {#term-garbage-collection}
: `moirai gc`, a separate maintenance process. It keeps everything reachable from a branch, a tag, a [pin](#term-pin) or a reflog entry younger than 90 days, drops unreachable commits older than 14 days, and deletes old files only after a grace period. See [Checkpoints, rollups and garbage collection](engine.html#maintenance).

Gate {#term-gate}
: A test that fails when a target is missed or a rule is broken, such as crash-point enumeration, a time budget or the query accuracy benchmark. Each gate becomes mandatory at a milestone and stays mandatory; a failing gate blocks the exit of its milestone and of every later one. See [The gates in plain words](building.html#gate-catalogue).

Gate worktree {#term-gate-worktree}
: In the project's own build process, a separate, neutral git worktree where the full gate runs on a branch tip before that branch may be merged into the [trunk](#term-trunk). See [Built by AI agents under a strict process](building.html#agent-process).

Git common directory {#term-git-common-directory}
: The directory that all worktrees of one git repository share, usually the `.git` directory of the main worktree. Moirai puts its store there, at `<git-common-dir>/moirai/`, so every worktree finds the same store. See [Where the store lives](git-image.html#store-location).

Git image {#term-git-image}
: A copy of the whole graph and its history written into a git repository as text: one [`.moi` file](#term-moi-file) per node, and one git commit per Moirai commit or per [checkpoint](#term-checkpoint). Any git tool can read and move it, and Moirai can import it back, including edits made with git; the store stays the source of truth. See [Saving into git](git-image.html).

gitmap {#term-gitmap}
: A table in the store that records which git commit each Moirai commit became, per destination and object format. It can be rebuilt from the image by reading the `Klotho-Commit` trailers. See [Export, import, push and pull](git-image.html#export-import).

Group commit {#term-group-commit}
: Sharing one disk flush among many writers. In Klotho it needs no leader: a writer whose records are not yet durable takes the [flush byte](#term-flush-byte) and flushes once for every group appended before it began. The design target for a burst of sixteen writers is at most three flushes. See [One flush for many writers](engine.html#group-commit).

Guard {#term-guard}
: A condition checked at the moment of a write. Status guards belong to a kind's [status machine](#term-status-machine); for example, a task cannot become done while a subtask is unfinished. [CAS guards](#term-cas-guard) are conditions the caller adds, such as `--if-status in_progress`. A failed guard writes nothing and names what stood in the way. See [Statuses and guarded transitions](concepts.html#statuses).

Harness {#term-harness}
: The program that runs an AI coding agent, such as Claude Code or Codex. Moirai needs little from a harness: the `moirai` command on the `PATH`, a stdio MCP server, a short block in `AGENTS.md` and a portable skill. See [Harnesses](agents.html#harnesses).

HEAD (of a client) {#term-head}
: The branch a client reads and writes. A client is a directory, a named client or an agent session, and each has its own HEAD, so many agents can work on different branches of one store at once; `moirai checkout` moves it. Not to be confused with the store's [`HEAD` file](#term-head-slot). See [Many agents, one store](versions.html#heads).

`HEAD` slot {#term-head-slot}
: One of two checksummed 4 KiB slots in the store's `HEAD` file, which name the current segment files, how far readers may read the log, and some counters. A publish writes the older slot, and a reader takes the valid slot with the higher sequence number, so a torn write of one slot does no harm. See [Crash safety](engine.html#crash-safety).

Historical edge {#term-historical-edge}
: An edge that records where something came from or what it is about, such as `supersedes`, `cites` or `mentions`. It may keep pointing at a deleted node, which then shows as a [tombstone](#term-tombstone). See [Edges](concepts.html#edges).

History retirement {#term-history-retirement}
: Maintenance that compresses a full extent of the log into a `hist` file with a commit index. Nothing is dropped by default. See [Checkpoints, rollups and garbage collection](engine.html#maintenance).

Hook {#term-hook}
: Code a harness runs on an event, such as the start of a session or the submission of a prompt. Moirai's hooks put the brief, a role pack or a short list of changes into an agent's context and record evidence for file links. They are optional and [fail open](#term-fail-open): every effect can also be pulled by an ordinary call. See [Hooks: optional accelerators](agents.html#hooks).

Idempotency key {#term-idempotency-key}
: A key that makes a write safe to retry. The key is bound to the write's canonical form with its parameter values (formatting, spelling and variable names do not matter) and to its branch: a replay with the same key and the same write returns the original result, and the same key with a different write exits with code 9. Every MCP write carries one. See [Exit codes](agents.html#exit-codes).

Intent record {#term-intent-record}
: A durable record that `moirai file mv` or `file rm` appends to the log before it touches the file system. After a crash, the next writer or `moirai doctor` uses it to finish the move or to abort it. See [Explicit verbs](file-links.html#explicit-verbs).

Kill loop {#term-kill-loop}
: A test that runs real writer and reader processes on Windows and kills them at random points, suspends them or steps the clock, for 10,000 iterations per variant. Not one acknowledged commit may be lost. See [The test workstream](building.html#test-workstream).

Kind {#term-kind}
: The type of a node. The core schema has thirteen kinds, such as `task`, `rule`, `decision`, `finding` and `artifact`, and a project may add its own. Each kind has its own fields and statuses. See [Node kinds](concepts.html#kinds).

Klotho {#term-klotho}
: The storage engine inside Moirai: a library compiled into the one `moirai` binary and written from scratch in Rust. It keeps the versioned graph on local disk and lets many processes read and write it without a server. The git image uses its name, as in `.klotho-image` and the `Klotho-*` trailers. See [Under the hood: Klotho](engine.html).

Lachesis {#term-lachesis}
: Moirai's own query language (LQ, files `*.lq`), in the style of Cypher. Every read command is a [named query](#term-named-query) in it and every write command a [named mutation](#term-named-mutation). It is named after the Moira who measures the thread. See [Lachesis, the query language](lachesis.html).

Lane {#term-lane}
: A line of parallel work. In Moirai a lane joins three things: a branch `lane/<name>`, a git [worktree](#term-worktree) for the code, and a `lane` node on `main` that records both; `moirai lane open` sets all three up. See [Lanes and git worktrees](versions.html#lanes).

Lease {#term-lease}
: A record that says "this holder is working on this node now", or "this holder acts in this role". Write rights come only from a lease the caller presents. A lease lives at store level, so every branch sees it, and it is never merged or exported; a task that an agent claims for itself is leased for 15 minutes by default, renewed by a write that presents the lease once more than half of that time has passed. See [Leases and claims](concepts.html#leases) and [Leases and roles](agents.html#leases-and-roles).

Ledger line {#term-ledger-line}
: An `incr` line in a `.moi` file: one entry of a counter's ledger, with the signed change and the commit id. A counter's value is the sum of its lines, so increases made on two branches are both kept. See [Counters as ledger lines](git-image.html#counters).

Link intent {#term-link-intent}
: The versioned layer of file links: file nodes, anchors, `at` edges, removal intent and the history of directory moves. It branches, merges and travels in the git image, while [resolution](#term-resolution) is computed per working tree. See [Worktrees, branches and the git image](file-links.html#worktrees-and-branches).

Link state {#term-link-state}
: The state of one anchor of a file link in a given working tree, such as `ok`, `moved-auto`, `moved-needs-confirm`, `missing`, `replaced` or `stale-anchor`. It is computed at read time, and the strings are frozen so that scripts can rely on them. See [Link states](file-links.html#link-states).

`LOCK` file {#term-lock-file}
: A file of the store that holds diagnostics and session records; the bytes that processes lock, for the [writer lock](#term-writer-lock) and the [flush byte](#term-flush-byte), lie beyond its end. It is never memory-mapped. See [The files in a store](engine.html#store-files).

Log {#term-log}
: The store's append-only record of every change, in checksummed records with 32-byte headers. It is the canonical history, and nothing in it is rewritten in place; the current state is built from it into [segments](#term-segment). See [History in a log, current state in segments](engine.html#log-and-segments).

Log sequence number (LSN) {#term-log-sequence-number-lsn}
: The position of a record in the [log](#term-log), written into the record's header. A record is valid only at its own position. See [History in a log, current state in segments](engine.html#log-and-segments).

LQ {#term-lq}
: Short for [Lachesis](#term-lachesis), the query language.

LQ-Bench {#term-lq-bench}
: A permanent benchmark that checks whether a model, given only the short [skill card](#term-skill-card), writes correct Lachesis. It runs a model against a synthetic store with 150 questions in three phrasings and 40 tasks aimed at known traps, and the language is frozen only when its accuracy gates hold. See [LQ-Bench](lachesis.html#lq-bench).

`~main` marker {#term-main-marker}
: The mark on a critical rule that was written on `main` but is not yet merged into the current lane. Such a rule still appears in the lane's context packs, so branching never hides a rule from an agent. See [Coordination across branches](versions.html#coordination).

MCP {#term-mcp}
: The Model Context Protocol, an open protocol through which a harness gives an AI agent access to tools. Moirai offers an [MCP server](#term-mcp-server) for agents and harnesses that have no shell. See [The MCP server](agents.html#mcp-server).

MCP server {#term-mcp-server}
: `moirai mcp`: the same binary in a long-running mode that a harness starts for one agent session. It speaks MCP over stdio and offers ten tools, such as `brief`, `pack`, `query`, `claim`, `complete` and `write`; to the store it is one more client. See [The MCP server](agents.html#mcp-server).

Memory mapping {#term-memory-mapping}
: Letting a process read a file as if it were memory, straight from the operating system's [page cache](#term-page-cache). Klotho maps only [sealed files](#term-sealed-file), read-only, partly because on Windows a mapped file cannot be resized. See [The files in a store](engine.html#store-files).

Merge base {#term-merge-base}
: The newest commit that two branches share. A [three-way merge](#term-three-way-merge) compares each side with it. Because merges into `main` sync first, the daily path has a single merge base. See [The typed three-way merge](versions.html#merge).

Milestone {#term-milestone}
: One of twelve stages, M0 to M11, in which Moirai is built in dependency order. Each milestone is finished, with all of its gates green, before the next one builds on it. The project is in M0. See [Roadmap](building.html#roadmap).

Model profile {#term-model-profile}
: A setting per model family that records how well that model's Lachesis has been measured: `gated`, `compatible` or `unknown`. A model with the `unknown` profile writes only through named mutations, and models that have not passed the benchmark's gates always see the [reading echo](#term-reading-echo). See [LQ-Bench](lachesis.html#lq-bench).

`.moi` file {#term-moi-file}
: A node file of the [git image](#term-git-image): canonical text that starts with `klotho-node 1`, followed by header lines, fields, labels, ledger lines and edges, and the body after a `---` line. Each file is named by the node's [uid](#term-uid). See [A node file](git-image.html#node-file).

Moirai {#term-moirai}
: The product: a versioned graph database for AI coding agents, and its one command, `moirai`. In Greek myth the Moirai are the three Fates; the engine, [Klotho](#term-klotho), and the query language, [Lachesis](#term-lachesis), are named after two of them. See [the home page](index.html) and [Why Moirai](why.html).

Multi-process simulation {#term-multi-process-simulation}
: A test that runs many simulated processes under seeded scheduling, with crashes, failed flushes, late lock releases and clock jumps injected, while the [reference model](#term-reference-model) checks every read. See [The test workstream](building.html#test-workstream).

Mutation testing {#term-mutation-testing}
: Making small deliberate changes, called mutants, to the code and checking that the tests catch them. At every milestone exit the test suites must catch at least 90 % of the mutants in the semantic code, and every bug in a catalogue of deliberate protocol bugs. See [The test workstream](building.html#test-workstream).

`#N` (short id) {#term-n-id}
: The short number you see and type for a node, such as `#812`. The store allocates it, counting up, and never reuses it, and it means the same node on every branch. On the command line it is typed without the `#`, because an unquoted `#` starts a comment in common shells. See [Nodes and their ids](concepts.html#nodes-and-ids).

Named hole {#term-named-hole}
: A value in the specification that only a measurement can settle, written as `HOLE(<id>)` and indexed with its candidates and the measurement that decides it; for example, the number of log operations after which a checkpoint runs. See [Reviews and named holes](building.html#reviews-and-holes).

Named mutation {#term-named-mutation}
: A stored, named write in Lachesis, called as `tx.<name>`. Every write command of the CLI is one, so `moirai claim` runs `tx.claim`, and `--show-tx` prints the write block behind a command. See [Named queries and mutations](lachesis.html#named-queries).

Named query {#term-named-query}
: A stored, named Lachesis query with typed parameters. Every read command is one, from the built-in `std` library, so `moirai ready` is the same as `moirai q ready`. A project can define its own, which are versioned like the rest of the schema. See [Named queries and mutations](lachesis.html#named-queries).

Native commit {#term-native-commit}
: A git commit in the image whose `Klotho-*` trailers verify: Moirai rebuilds the commit's canonical form from them, recomputes its hash and finds it equal to `Klotho-Commit`. It is imported with its own id. See [Edits made with git](git-image.html#foreign).

Node {#term-node}
: One record in the graph, such as a task, a rule, a decision or a project file. Every node has a [kind](#term-kind), a short id `#N`, a permanent [uid](#term-uid) and a title, and optionally a one-line abstract and a body of up to 64 KiB. See [Nodes and their ids](concepts.html#nodes-and-ids).

Object format {#term-object-format}
: The hash function a git repository uses for its object ids: SHA-1 or SHA-256. Images use SHA-1 by default for anything that leaves the machine, SHA-256 is allowed for local repositories, and one destination never mixes the two. See [When exports happen](git-image.html#cadence).

Orchestrator {#term-orchestrator}
: The agent or person that plans the work, dispatches workers into lanes, reviews and merges. In Moirai it is also a role: its session lease lets it write everything on any branch, including the branch, merge, delete and image commands. See [Leases and roles](agents.html#leases-and-roles).

Overlay {#term-overlay}
: A small in-memory structure into which each process replays the log [tail](#term-tail), the changes not yet in any segment. A reader looks in the overlay first, then in the delta segments from newest to oldest, then in the base segment. See [History in a log, current state in segments](engine.html#log-and-segments).

Pack {#term-pack}
: Short for [context pack](#term-context-pack); `moirai pack ID --role ROLE` builds one. See also [role pack](#term-role-pack).

Page cache {#term-page-cache}
: The operating system's memory cache of file contents. Because Klotho's segments are mapped read-only, all Moirai processes on one machine share a single copy of the hot data there. See [Many processes, no server](engine.html#process-model).

Pin {#term-pin}
: A reference that keeps a snapshot of the store's segment files alive. A branch fork, a merge into `main` and `moirai tag NAME --pin` each pin a snapshot, so reading that point stays fast and garbage collection keeps it. In a `.moi` file, `pin=` on a citation edge fixes the citation to one commit. See [History](versions.html#history).

Pointer file {#term-pointer-file}
: A small `.moirai` file, written by `moirai init --link STORE`, that names an existing store, so that commands run in another directory find it. See [Where the store lives](engine.html#store-location).

Port phase {#term-port-phase}
: The planned stage after the Windows release in which the Linux and macOS implementations are built. It is documented with its own gates but not scheduled; a port may add an implementation, never change the format. See [Linux and macOS later](building.html#ports).

`proposed` {#term-proposed}
: The status in which rules and decisions start, and in which text from an agent pipeline enters the store. A proposed record is shown to agents marked `~proposed`, and it becomes active only through an explicit status change, usually by the orchestrator after reviewing `moirai q proposed`. See [Record a rule, a note and a decision](tour.html#record-knowledge).

Proposed status {#term-proposed-status}
: See [`proposed`](#term-proposed).

Provenance {#term-provenance}
: The record of where a change came from. Every change belongs to exactly one commit, which records the actor, its role, the session, the Moirai branch, the git head, branch and worktree it worked in, and a message. See [Rules, critical notes and authority](concepts.html#rules-and-authority).

Quantified path {#term-quantified-path}
: A Lachesis pattern that repeats an edge, in Cypher's spelling (`-[:CHILD_OF*1..2]->`) or in GQL's (`-[:CHILD_OF]->{1,2}`). Evaluation goes level by level with a visited set, so every quantified pattern terminates. See [Quantified paths](lachesis.html#quantified-paths).

Quiescence {#term-quiescence}
: A short wait before a move is recorded. A settle checks an old path again at least 50 ms after it last saw it absent, and drops the re-bind if the file came back, because editors that save atomically rename the original away for a few milliseconds. See [Settling](file-links.html#settling).

Quiet mode {#term-quiet-mode}
: `moirai quiet on`, for benchmark windows on the same machine. It switches off the maintenance that writers would otherwise do in the foreground, such as automatic checkpoints (up to a cap), rollups, garbage collection and image export. A lane in the status `measuring` implies it. See [Quiet mode](engine.html#quiet-mode).

Re-bind {#term-re-bind}
: Recording a file node's new path in the versioned history after a move. A [settle](#term-settle) re-binds automatically, by default only on exact evidence with a single candidate, and only from the branch's [writer tree](#term-writer-tree). `links fix` records a re-bind that someone decided; unless the actor has the `owner` role, it stays marked as a guess until it is confirmed. See [Settling](file-links.html#settling).

Reader tree {#term-reader-tree}
: Any working tree other than a branch's [writer tree](#term-writer-tree). It resolves and shows link states, but its observations wait as pending rows until the writer tree sees the same move. See [Settling](file-links.html#settling).

Reading echo {#term-reading-echo}
: A line at the top of a Lachesis result that says in words what an edge pattern means, such as `reads: b BLOCKS #211 ... | b must finish before #211 starts`. It appears when a pattern starts from a fixed node on an edge whose two ends can be the same kind, or when it uses a [reverse alias](#term-reverse-alias). See [Patterns: kinds, edges and directions](lachesis.html#patterns).

`ready` {#term-ready}
: [Derived state](#term-derived-state) of a task that an agent may start now: it is [unblocked](#term-unblocked), no other agent holds a lease on it, no other branch has completed or deleted it without this branch having caught up, and its defer time has passed. It exists only at the tip of a branch, because leases are live state. See [Derived state](concepts.html#derived-state).

Ready queue {#term-ready-queue}
: The list of [ready](#term-ready) tasks, in a fixed order (priority, then id), as `moirai ready` prints it and as `t.ready` selects it in Lachesis. See [What can start, and what holds things up](tour.html#ready-and-blocking).

`ready_to_close` {#term-ready-to-close}
: [Derived state](#term-derived-state) of a [container](#term-container) whose subtasks are all done. See [Derived state](concepts.html#derived-state).

Red-first {#term-red-first}
: A rule of the project's own build process: every behavioural claim and every new or changed gate is first shown failing, for the predicted reason, on the code without the change, and then passing with it. See [Built by AI agents under a strict process](building.html#agent-process).

Reference model {#term-reference-model}
: A complete, deliberately naive implementation of Moirai's logic in Rust, written before the engine and used only in tests, as the oracle. Where the engine optimizes, the model uses the plain definition, and from M1 on the engine is compared with it command by command, including commit ids. See [The reference model](building.html#reference-model).

Reflog {#term-reflog}
: The list of every move of a branch pointer, with who moved it and why. `moirai reflog REF` prints it, `undo` works from it, and garbage collection keeps its entries for 90 days by default. See [History](versions.html#history).

Release gate {#term-release-gate}
: The conditions for calling Moirai released, "complete and hardened": among others, every milestone exited with all its gates, every budget met at three scales, nightly simulation and kill loops clean, a 72-hour soak, and drills for backup, restore and repair. See [The release gate](building.html#release-gate).

Resolution {#term-resolution}
: Two meanings. As a field, it says how a closed task or finding ended: `completed`, `wontdo`, `duplicate`, `superseded`, `obsolete` or `rework`. For file links, resolution is the layer computed per working tree (link states, file ids, evidence and proposals), which is never versioned or exported. See [Typed fields](concepts.html#fields) and [Worktrees, branches and the git image](file-links.html#worktrees-and-branches).

`result.v1` {#term-result-v1}
: The strict JSON object a dispatched worker ends with: its task, lease, outcome, summary, evidence and the ids it recorded, plus any findings or notes it could not write itself. `moirai apply` reads these objects from where the harness left them. See [The dispatcher pattern](agents.html#dispatcher).

`rev` {#term-rev}
: The revision number in a result header, such as `rev 5`: the store-wide sequence number of the commit the result was read from. A write prints `rev 4 -> 5`. Because the number is store-wide, the revisions of one branch can skip numbers. See [The project](tour.html#the-project).

Reverse alias {#term-reverse-alias}
: A second name for an edge type in Lachesis that reads it in the other direction, such as `BLOCKED_BY` for `BLOCKS`, so that a question can be asked from either end. See [Patterns: kinds, edges and directions](lachesis.html#patterns).

Revision {#term-revision}
: A name for a point in history, in git's style: a branch tip (`main`), a commit prefix, a sequence number (`s4400`), an ancestor (`main~3`), a reflog position or time (`main@2026-11-01`), or a range for `log` and `diff`. Read commands take `--at REV`, and a Lachesis query can start with `USE REV`. See [Reading the past](versions.html#reading-the-past).

Role pack {#term-role-pack}
: A short pack of at most 3,000 bytes that a hook gives a dispatched worker when it starts: the critical rules for its role and the `pack` command for its task. The worker's own pack then lists those rules as one line of ids. See [Hooks: optional accelerators](agents.html#hooks).

Rollup {#term-rollup}
: Two meanings. In the graph, rollups are the counts of a task's direct subtasks and of how many of them are done. In the engine, a rollup writes a new base segment once the delta segments grow past a quarter of it; it runs only in a separate `moirai gc` process. See [Derived state](concepts.html#derived-state) and [Checkpoints, rollups and garbage collection](engine.html#maintenance).

Run {#term-run}
: One run of an agent or a scripted workflow, recorded as a node of kind `run` with the artifacts it is expected to produce. A [dispatcher](#term-dispatcher) claims tasks for a whole run and applies its results in one batch, and a run closes green only when every expected artifact exists. See [The dispatcher pattern](agents.html#dispatcher).

Sandbox {#term-sandbox}
: A harness's limit on what an agent's commands may write. Reads never need write access; a write that a sandbox refuses exits with code 7 and tells the agent to repeat it through the MCP tool or to put it in its `result.v1`, so it is delayed, not lost. See [Sandboxes](agents.html#sandboxes).

Schema as data {#term-schema-as-data}
: Moirai's schema, the kinds, fields, edge kinds and named queries, is itself stored in the graph: versioned per branch, merged and exported like everything else. A project's additions apply at once; changing or retiring an item needs an explicit migration. See [Schema as data](concepts.html#schema).

Sealed file {#term-sealed-file}
: A store file that is written once, made durable, marked read-only and never changed again, such as a segment. Only sealed files are memory-mapped. See [The files in a store](engine.html#store-files).

Seeded bug {#term-seeded-bug}
: A deliberate bug planted to prove that a test harness finds it. The [toy log](#term-toy-log) has switchable seeded bugs that the crash enumerator must find, and the author who plants them never writes the enumerator. See [The test workstream](building.html#test-workstream).

Segment {#term-segment}
: An immutable, memory-mapped file that holds the current state of the graph in columns: node headers, fields, the edges of every node in both directions, and precomputed bitsets. A store has one base segment and up to three delta segments. See [History in a log, current state in segments](engine.html#log-and-segments).

Settle {#term-settle}
: Writing the new locations of moved files into the versioned history. Reads only show where a file is now; a settle records the exact [re-binds](#term-re-bind) that its strict rules allow. See [Settling](file-links.html#settling).

Settle point {#term-settle-point}
: A moment at which a [settle](#term-settle) runs: the session-start hook, `moirai links sync`, the explicit file commands and `links fix`, `complete`, the merge ritual, and an optional git `post-commit` hook. See [Settling](file-links.html#settling).

Settled marker {#term-settled-marker}
: A store-level record written whenever a task becomes `done` or `cancelled` on any branch, by any path. Every other branch then treats the task as finished but not yet merged: it cannot be claimed or dispatched there. The marker stops mattering once that branch's history contains the completion. See [Coordination across branches](versions.html#coordination).

Side ref {#term-side-ref}
: A git ref in the image repository outside every exported branch, `refs/klotho/meta/<store-id>`, that holds one store's hints: its id, its export state and the map from uids to its short ids. It is not part of any commit's tree and is not hashed. See [Where the image goes](git-image.html#destination).

Side state {#term-side-state}
: A status that steps off a kind's main line of work, such as `deferred`, `cancelled` or `frozen` for a task. See [Statuses and guarded transitions](concepts.html#statuses).

Skill {#term-skill}
: A short instruction file that teaches an agent how to use a tool. Moirai ships three: the core `moirai` skill (at most 800 tokens), `moirai-orchestrate` for the orchestrator, and `moirai-ql`, the Lachesis card. See [Skills](agents.html#skills).

Skill card {#term-skill-card}
: The short reference from which agents learn Lachesis, at most 1,000 tokens and 3,500 bytes, shipped as the `moirai-ql` skill. [LQ-Bench](#term-lq-bench) measures whether it is enough. See [LQ-Bench](lachesis.html#lq-bench).

Soak {#term-soak}
: A long test under mixed load. The release gate requires 72 hours of mixed load with kill loops across branches, with no lost acknowledged commit. See [The release gate](building.html#release-gate).

Stage role {#term-stage-role}
: In the project's own build process, the job an agent does inside a [unit](#term-unit): architect, architecture critic, developer, tester, verifier, code reviewer or mechanic. Duties are separated: the developer never gives the final verdict, and the reviewer never fixes what it finds. See [Built by AI agents under a strict process](building.html#agent-process).

Staging ref {#term-staging-ref}
: A ref named `merge/<dst>/from/<src>`, or `import/<ref>` for an import, where a merge that found a [structural violation](#term-structural-violation) waits. The destination does not move; you fix the result with `moirai resolve` and finish with `moirai merge --continue`. See [Structural violations stop the merge](versions.html#violations).

Status machine {#term-status-machine}
: The list of status transitions a kind allows. A write may make only the listed transitions, and some of them have [guards](#term-guard). See [Statuses and guarded transitions](concepts.html#statuses).

Store {#term-store}
: The directory that holds all of Moirai's data for a project: the log, the segments and a few other files. Inside a git repository it is `<git-common-dir>/moirai/`, shared by every worktree; outside one it is `.moirai/`. See [Where the store lives](engine.html#store-location).

Structural edge {#term-structural-edge}
: An edge that carries a rule the engine enforces, such as the order of work (`parent`, `blocks`, `gates`) or an answer to a question. Both ends must be live nodes, except for a [flagged](#term-flagged-edge) blocker edge. See [Edges](concepts.html#edges).

Structural violation {#term-structural-violation}
: A merge result that would break the graph itself, such as a blocker edge to a node the other side deleted (`DanglingEdge`) or a cycle of blockers. It never lands on a branch: the merge stops on a [staging ref](#term-staging-ref). See [Structural violations stop the merge](versions.html#violations).

`suspect` {#term-suspect}
: [Derived state](#term-derived-state) of a node built on something that changed: a source it was derived from, cites or implements was retracted, superseded or deleted, or a file it links to was removed. See [Derived state](concepts.html#derived-state).

Sync {#term-sync}
: `moirai sync`: merging `main` into the current lane with the typed merge, so the lane receives the rules, completions and deletions written on `main`. A sync commit stores only the residue, the keys whose merged value differs from what `main`'s changes alone would give. See [Keeping a lane current: sync](versions.html#sync).

Tag {#term-tag}
: A fixed name for one commit, under `tags/`, which never moves. `moirai tag NAME --pin` also keeps a snapshot there, so reading that point later is fast. See [History](versions.html#history).

Tail {#term-tail}
: The newest part of the [log](#term-log), not yet folded into a segment. Each process replays it into its [overlay](#term-overlay); its length is capped by the checkpoint threshold, so opening a store stays cheap. See [History in a log, current state in segments](engine.html#log-and-segments).

Three-way merge {#term-three-way-merge}
: A merge that compares both sides with their [merge base](#term-merge-base). Moirai's is typed: it compares keys, such as one field of one node, one edge or the existence of one node, rather than lines of files, and settles each key by a rule for its type. See [The typed three-way merge](versions.html#merge).

Tier {#term-tier}
: In the project's own build process, the depth at which a unit runs: LIGHT for text changes with no change of behaviour, HEAVY for product behaviour, crash safety, bytes and ids, rule changes and open decisions, and STANDARD for everything else. When in doubt, the higher tier applies. See [Built by AI agents under a strict process](building.html#agent-process).

Token ledger {#term-token-ledger}
: A part of the test suite that records every byte Moirai places in an agent's context, per harness and model, counts it with that model's tokenizer and checks it against the token budgets. See [Budgets](engine.html#budgets).

Tombstone {#term-tombstone}
: What remains of a deleted node: its row, marked deleted, with its kind, its title, the deleting commit, who made it, the reason and the replacement. Its `#N` is never reused, every reference to it shows the tombstone, and reverting the deleting commit brings the node back. See [Tombstones](concepts.html#tombstones).

Toy log {#term-toy-log}
: A small test implementation of the commit protocol with switchable [seeded bugs](#term-seeded-bug), used to show that the crash enumerator finds every one of them before the real engine exists. See [The test workstream](building.html#test-workstream).

Trailer {#term-trailer}
: A `Key: value` line at the end of a git commit message. Each git commit of the image carries `Klotho-*` trailers that hold every field of the Moirai commit's canonical form, so the commit id can be recomputed after a round trip through git. See [Commits and trailers](git-image.html#commits).

Trunk {#term-trunk}
: In the project's own build process, the main line of the code repository, the `master` branch, into which only one orchestrating session merges. See [Built by AI agents under a strict process](building.html#agent-process).

`TX` block {#term-tx-block}
: A Lachesis write, written as `TX [ON branch] [KEY k] [IF TIP c] [LEASE L] { ... }`. One block is one commit on one branch, all or nothing, and its guards, such as [`EXPECT`](#term-expect), decide whether it lands. See [Writes: one TX block, one commit](lachesis.html#writes).

Typed merge {#term-typed-merge}
: Moirai's merge, which knows the type of every key: statuses join along the kind's order, counter increments add up, sets take the union, text merges line by line, and a real disagreement becomes a [conflict value](#term-conflict-value). See [The typed three-way merge](versions.html#merge).

uid {#term-uid}
: A node's permanent 128-bit identity, random for most kinds and written as 32 hex digits. Commit hashes and the git image use it, because it stays the same when the data moves to another store. See [Nodes and their ids](concepts.html#nodes-and-ids).

`unblocked` {#term-unblocked}
: [Derived state](#term-derived-state) of an open task that is not a container, holds no unresolved conflict, has no unfinished blocker, has no ancestor held back by a blocker from outside its subtree, and whose defer time has passed at the view's time. Unlike [ready](#term-ready), it is valid on any version, also in the past. See [Derived state](concepts.html#derived-state).

Unit {#term-unit}
: In the project's own build process, one reviewable piece of work by one [author role](#term-author-role), at most about ten commits and fifteen files, carried out on the role's branch in its own worktree. See [Built by AI agents under a strict process](building.html#agent-process).

View {#term-view}
: A read-only picture of the graph at one version. A plain branch name gives the live tip, where leases and readiness exist; a tag, a commit or a revision with a suffix gives a past view, where writes are refused and live state is not available. See [Reading the past](versions.html#reading-the-past).

Virtual `done` {#term-virtual-done}
: A boolean field `done` of tasks, questions and verdicts that is not stored but read from the status. Writing `done = true` performs the status transition with all its checks, so the same fact is never kept twice. See [The virtual done field](concepts.html#virtual-done).

Work unit {#term-work-unit}
: A counted unit of query work, such as one row examined or one edge expanded. A Lachesis query may use 2,000,000 by default; counting instead of timing makes the same query on the same version always stop at the same row. See [Budgets and exit code 10](lachesis.html#budgets).

Worker {#term-worker}
: An agent that an orchestrator starts to do one task in one role, such as developer or tester. A dispatched worker gets a [role pack](#term-role-pack) when it starts, records its work directly and ends with a [`result.v1`](#term-result-v1) object. See [The dispatcher pattern](agents.html#dispatcher).

Worktree {#term-worktree}
: A git working tree. One git repository can have several, each on its own branch, which is how parallel agents usually work. A Moirai [lane](#term-lane) binds one worktree to one Moirai branch, and all worktrees of a repository share one store. See [Lanes and git worktrees](versions.html#lanes).

Writer lock {#term-writer-lock}
: An exclusive lock on one byte of the store's `LOCK` file. At most one process holds it, and only to check and append a prepared commit; a waiting writer gives up after 2 seconds by default and names the process that holds it. Readers never take it. See [Many processes, no server](engine.html#process-model).

Writer tree {#term-writer-tree}
: The one working tree whose observations may re-bind a branch's file links: the branch's [designated tree](#term-designated-tree), while its git HEAD is on the branch's git line. Every other tree is a [reader tree](#term-reader-tree). See [Settling](file-links.html#settling).
