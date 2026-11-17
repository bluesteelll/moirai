---
title: File links that survive moves
nav: File links
description: How Moirai lets a task, note or rule point at a project file or a passage inside it, and keeps that pointer right when files are renamed, moved, edited or deleted.
---

# File links that survive moves

A task, a note or a rule in Moirai can point at a project file, or at a passage inside one. The pointer follows the file when someone renames, moves or edits it. When it cannot follow with certainty, it says so and names the next step. It never points at the wrong file or the wrong text without saying so.

## The problem: citations rot {#the-problem}

Agents cite code the way people do: `src/conn.rs:120`, `docs/design/routing.md`, "the `read_request` function". Such a citation is right on the day it is written. Then the project changes.

- **Lines move.** One edit above line 120 makes the number point at different code, and nothing tells the reader.
- **Files move.** People rename files in an editor. Agents run `mv`, `Move-Item` or a Python rename. Git rewrites the tree on `checkout`, `merge`, `rebase`, `stash`, `apply` and more. Most moves never go through `git mv`, so a tool that updates links only on its own moves misses them.
- **Files disappear.** A file is deleted. Sometimes a new, unrelated file later appears at the same path.

For an agent, a citation that silently points at the wrong code is worse than a broken one: the agent reads the wrong text and trusts it. So every link that is not exactly right carries a visible state and a one-line next step.

Moirai combines two mechanisms. Explicit file commands record a move or a deletion precisely. For everything else, Moirai finds the file again by evidence when it next reads the link, and records the new location only when the evidence is exact.

## What a link is {#what-a-link-is}

A link is an `at` edge from any node to a [file node](glossary.html#term-file-node). The edge carries one or more [anchors](glossary.html#term-anchor). An anchor names the whole file or one place inside it, so a single task can cite several places in the same file. Agents write anchors in the forms they already use:

| Form | What it anchors | Example |
|---|---|---|
| `path` | the whole file | `docs/design/routing.md` |
| `path:L-M` | a span of lines, stored as its text, not its numbers | `src/http/parse.rs:40-44` |
| `path::Type/fn` | a Rust item or a TOML key | `src/conn.rs::Conn/read_request` |
| `path#Heading` | a Markdown section | `docs/design/routing.md#Fallback` |
| `path@<commit>:L-M` | a pinned historical citation, never re-resolved | `docs/design/routing.md@c4f1a9e20:12-18` |
| a quoted passage (`--quote-file FILE`) | literal text, which must occur once in the file or be made unique | — |

```console
$ moirai link 204 --at src/parse.rs:40-44

```

Paths are relative to the project root. A bare file name that matches several files is refused when the link is written, with the list of matches, so an ambiguous citation never enters the store.

> [!NOTE]
> **Status of two forms**
>
> The `path::Type/fn` and `path#Heading` forms depend on small scanners for Rust, Markdown and TOML that find items and headings. Their specification is written but still under review. Until review accepts it, Moirai refuses these two forms and suggests the `path:L-M` form of the same lines. If the scanners miss the freeze of the on-disk format, the refusal stays until a later format change.

### What an anchor records {#what-an-anchor-records}

An anchor is never a bare line number. When a link is written, Moirai reads the file once and captures a bundle of selectors, about 345 bytes in a typical case:

- **A quote with context:** the exact text plus text before and after it, widened until the quote is unique in the file; if needed, the enclosing scope (once the scanners are accepted) and, as a last resort, the occurrence number. Long spans get an end quote too.
- **A context window:** hashes of up to 16 lines on each side, which tell duplicate quotes apart.
- **A scope:** the enclosing Rust item, Markdown heading path or TOML key. While the scanner specification is under review (see the callout above), no anchor records a scope.
- **A line hint**, the file's content id and git commit at capture, and a hash of the span.
- **A watch mode:** `header` keeps a function or section link fresh while its body grows; `span` reports any change to the cited text.

### File nodes keep their identity {#file-identity}

A file node is a node of kind `artifact`. Its identity is derived once, from the root, the path at which the file was first registered, and the node that held that path before it. A move changes the node's current `path`, never its identity. Former paths are kept as `aliases`: they resolve old mentions but never capture a new file. Two branches that link the same file create the same node.

The node also records a content id, `oid`, a git-blob-shaped hash of the file's bytes. The operating system's file id is only a hint for one working tree, because editors replace a file on save and give it a new id.

## Reading never writes {#reads-are-pure}

Every read computes link states live, against the files in your working tree: `show`, `pack`, `brief`, `get`, `find`, `q`, `links check` and the MCP reads. None of them appends anything to the log. A moved or vanished file is therefore visible to every referrer at its next read, with no write at all.

A read that finds a moved file shows `moved-auto` with the detail `not yet recorded`; the new location enters the versioned history later, at a [settle point](#settling). Reads stay lock-free. A file that an agent moves aside for a test and then back is recorded only if a settle runs while the file is away and its re-check still finds it away; a move to a temporary, ignored or outside path is never recorded.

## Link states {#link-states}

Every anchor of a link has exactly one state in a given working tree. The strings are frozen, so agents and scripts can rely on them.

| State | Meaning |
|---|---|
| `ok` | The file is at its path, and the cited text is where it was or was found unchanged. |
| `moved-auto` | The file was found at a new path by exact evidence. |
| `moved-needs-confirm` | There is one candidate, but the evidence is not exact: a similar file, an identical copy, or a file that was moved and edited. |
| `ambiguous` | There are two or more candidates, or a collision or conflict that only a decision settles. |
| `deleted` | The file was removed on purpose, with a reason and possibly a replacement, or its node was deleted. |
| `replaced` | The path exists, but the file there was re-created with unrelated content. |
| `stale-anchor` | The file was found, but the cited text was edited, became ambiguous or is gone. |
| `missing` | The file is absent and there is no candidate. |
| `absent-in-tree` | This working tree cannot contain the observation: it is behind, or its history diverged. |
| `pending` | The file moved on a line of history that this tree has not received yet. |
| `planned` | The link was made with `--planned` to a file that does not exist yet. |
| `unverified` | The state cannot be decided now. The detail names one reason from a closed list, such as `budget`, `cloud-only`, `no tree`, `git` or `size`. |

Where one state must summarise several anchors, as in a node's overall state or a pack header, the most severe wins, in this order: `missing`, `replaced`, `deleted`, `ambiguous`, `moved-needs-confirm`, `stale-anchor`, `unverified`, `pending`, `planned`, `absent-in-tree`, `moved-auto`, `ok`. `replaced` ranks above `deleted` on purpose: a deletion is recorded intent that every referrer already sees, while a replaced path shows unrelated content that a reader could mistake for the cited file.

<details>
<summary>How file states and anchor states combine</summary>

A link's state is the file's state refined by the anchor's state, which is computed only when the file resolved to `ok` or `moved-auto`. An anchor is `fresh` (the text is where the hint says), `moved` (identical, unique text elsewhere in the file), `edited` (a close match, or only the scope survived), `ambiguous`, `orphaned` (nothing found) or `unverified`. `edited`, `ambiguous` and `orphaned` turn the link into `stale-anchor`; an `unverified` anchor makes it `unverified`.

A file removed on purpose, or a deleted file node, makes every referrer `suspect`: that is graph state, recorded in the same commit. Every other state is computed at read time from the working tree; it never blocks a task from being ready. In queries, `none` (a node without links) and `unresolved` (an anchor whose file did not resolve) keep the built-ins total; see [Lachesis](lachesis.html).

</details>

## A links check session {#links-check-session}

Suppose the small HTTP server `tinyhttp` has a campaign task `#200`, "HTTP/2 support", with subtasks that cite files. Since the citations were written, an agent moved the parser, someone renamed a design note and rewrote part of it in an editor that saves by replacing the file, a fuzz test was deleted, a benchmark note was rewritten from scratch, and the lines that cite a function were rewritten. `moirai links check` lists every link under `#200` that is not `ok`; `--deep` adds a similarity search for files that no exact evidence found:

```console
$ moirai links check --scope 200 --deep
branch: main | rev 4512 | live | 5 rows | files @ tinyhttp (main 3f9c2a1, dirty 2 5m ago)
#812  moved-auto            src/parse.rs -> src/http/parse.rs  file-id | not yet recorded (moirai links sync)
#815  moved-needs-confirm   docs/design/router.md -> docs/design/routing.md  similar 0.81, runner-up 0.22 | verify #815
#820  missing               tests/fuzz_headers.rs  since 3m | no candidate | nothing written
#826  replaced              docs/bench.md  re-created with unrelated content (containment 0.04/0.07) | verify #826
a31  stale-anchor          src/conn.rs:118-131  edited 0.86 | was: "pub fn read_request(b..." | verify a31
verify #N: moirai file where N --evidence

```

The header names the branch, the revision, `live` (the result depends on the working tree) and the tree: its label, git branch, git `HEAD` and dirty file count. Each row starts with a handle: `#N` for a file node, `aN` for one anchor.

`#812 moved-auto`
: The parser moved; its file id, with unchanged size and modification time, proves it. `moirai links sync`, or the next automatic settle, records it.

`#815 moved-needs-confirm`
: The renamed and edited note is 81 % similar to the old content. Similarity is a guess, so Moirai only proposes; `moirai file where 815 --evidence` shows the evidence.

`#820 missing`
: The test file is gone and nothing looks like it. Nothing was written. If the deletion was intended, `moirai links fix 820 --drop` records it.

`#826 replaced`
: A file exists at the old path, but shares almost nothing with the cited version. The answer is `--accept-replacement` or `--drop`.

`a31 stale-anchor`
: The file is in place, but the cited lines changed; the closest match scores 0.86. `was:` shows the start of the cited text.

No line ever prints a command that accepts a guess: proposals point at the evidence command, exact states at the settle command. After reading the evidence, an agent runs `moirai links fix 815 --accept --expect docs/design/routing.md`, which succeeds only if the top proposal is still that path. The link then shows as an accepted guess until a different actor whose role may confirm (by default `orchestrator` or `owner`) confirms it with `--confirm`.

Packs and briefs show one short marker per link that is not `ok`, such as `[moved-needs-confirm 0.81 | verify #815]`, at most 50 bytes, plus one legend line for the `verify` command.

## Following a file across a move and an edit {#following-a-move}

<figure class="diagram">
<svg viewBox="0 0 720 340" role="img" aria-labelledby="file-links-d1-t">
  <title id="file-links-d1-t">A link follows a file through a move and an edit</title>
  <defs><marker id="file-links-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path class="d-arrow" d="M0,0 L10,5 L0,10 z"/></marker></defs>
  <path class="d-box" d="M32,20 h124 a8,8 0 0 1 8,8 v28 a8,8 0 0 1 -8,8 h-124 a8,8 0 0 1 -8,-8 v-28 a8,8 0 0 1 8,-8 z"/>
  <text class="d-text-strong" x="94" y="42" text-anchor="middle" dominant-baseline="middle">task #204</text>
  <line class="d-edge" x1="164" y1="42" x2="196" y2="42" marker-end="url(#file-links-arrow)"/>
  <text class="d-text-small" x="180" y="30" text-anchor="middle">at</text>
  <path class="d-box-muted" d="M204,20 h484 a8,8 0 0 1 8,8 v28 a8,8 0 0 1 -8,8 h-484 a8,8 0 0 1 -8,-8 v-28 a8,8 0 0 1 8,-8 z"/>
  <text class="d-text" x="446" y="42" text-anchor="middle" dominant-baseline="middle">file node #812: one identity; old paths kept as aliases</text>
  <line class="d-edge-dashed" x1="124" y1="64" x2="124" y2="100"/>
  <line class="d-edge-dashed" x1="360" y1="64" x2="360" y2="100"/>
  <line class="d-edge-dashed" x1="596" y1="64" x2="596" y2="100"/>
  <path class="d-box" d="M32,100 h184 a8,8 0 0 1 8,8 v80 a8,8 0 0 1 -8,8 h-184 a8,8 0 0 1 -8,-8 v-80 a8,8 0 0 1 8,-8 z"/>
  <text class="d-text-strong" x="124" y="124" text-anchor="middle" dominant-baseline="middle">src/parse.rs</text>
  <text class="d-text-small" x="124" y="150" text-anchor="middle" dominant-baseline="middle">quote at lines 40-44</text>
  <text class="d-text-small" x="124" y="172" text-anchor="middle" dominant-baseline="middle">link state: ok</text>
  <path class="d-box-accent" d="M268,100 h184 a8,8 0 0 1 8,8 v80 a8,8 0 0 1 -8,8 h-184 a8,8 0 0 1 -8,-8 v-80 a8,8 0 0 1 8,-8 z"/>
  <text class="d-text-strong" x="360" y="124" text-anchor="middle" dominant-baseline="middle">src/http/parse.rs</text>
  <text class="d-text-small" x="360" y="150" text-anchor="middle" dominant-baseline="middle">next read: moved-auto</text>
  <text class="d-text-small" x="360" y="172" text-anchor="middle" dominant-baseline="middle">next settle: re-bind written</text>
  <path class="d-box" d="M504,100 h184 a8,8 0 0 1 8,8 v80 a8,8 0 0 1 -8,8 h-184 a8,8 0 0 1 -8,-8 v-80 a8,8 0 0 1 8,-8 z"/>
  <text class="d-text-strong" x="596" y="124" text-anchor="middle" dominant-baseline="middle">src/http/parse.rs</text>
  <text class="d-text-small" x="596" y="150" text-anchor="middle" dominant-baseline="middle">same quote, now lines 46-50</text>
  <text class="d-text-small" x="596" y="172" text-anchor="middle" dominant-baseline="middle">link state: ok</text>
  <line class="d-edge" x1="224" y1="148" x2="260" y2="148" marker-end="url(#file-links-arrow)"/>
  <text class="d-text-small" x="242" y="136" text-anchor="middle">mv</text>
  <line class="d-edge" x1="460" y1="148" x2="496" y2="148" marker-end="url(#file-links-arrow)"/>
  <text class="d-text-small" x="478" y="136" text-anchor="middle">edit</text>
  <path class="d-box" d="M32,224 h404 a8,8 0 0 1 8,8 v88 a8,8 0 0 1 -8,8 h-404 a8,8 0 0 1 -8,-8 v-88 a8,8 0 0 1 8,-8 z"/>
  <text class="d-text-strong" x="234" y="248" text-anchor="middle" dominant-baseline="middle">Exact evidence: any one of these re-binds</text>
  <text class="d-text-small" x="234" y="272" text-anchor="middle" dominant-baseline="middle">the same file id, with the same size and mtime</text>
  <text class="d-text-small" x="234" y="294" text-anchor="middle" dominant-baseline="middle">a recorded file mv intent</text>
  <text class="d-text-small" x="234" y="316" text-anchor="middle" dominant-baseline="middle">a git rename inside one commit</text>
  <line class="d-edge-accent" x1="360" y1="224" x2="360" y2="198" marker-end="url(#file-links-arrow)"/>
  <path class="d-box-muted" d="M476,224 h212 a8,8 0 0 1 8,8 v88 a8,8 0 0 1 -8,8 h-212 a8,8 0 0 1 -8,-8 v-88 a8,8 0 0 1 8,-8 z"/>
  <text class="d-text-strong" x="582" y="248" text-anchor="middle" dominant-baseline="middle">Never enough alone</text>
  <text class="d-text-small" x="582" y="272" text-anchor="middle" dominant-baseline="middle">equal content: maybe a copy</text>
  <text class="d-text-small" x="582" y="294" text-anchor="middle" dominant-baseline="middle">similar text: only a proposal</text>
  <text class="d-text-small" x="582" y="316" text-anchor="middle" dominant-baseline="middle">absence: never a deletion</text>
</svg>
<figcaption>The file node keeps one identity while the file moves and changes. The move is re-bound only on exact evidence; after the edit, the anchor finds its quoted text six lines lower and the link stays <code>ok</code>.</figcaption>
</figure>

When a read finds that a linked path is gone, Moirai works in three steps.

1. **Ask the tree's own git history.** Moirai reads the tree's committed git tree in process, without starting git. If that tree still holds the linked path, this tree moved or deleted the file itself and has not committed that yet, so the search runs. The search also runs when the tree has received the commit that the last observation came from. If the tree holds one of the file's former paths instead, the move happened on a line of history this tree has not received: `pending`, with no search. Otherwise Moirai looks for exact renames in the commits this tree integrated; if none explains the change, the state is `absent-in-tree`.
2. **Search by evidence**, strongest first. Candidates are only paths no other live file node holds, and never paths outside the root, in ignored build output, in temporary or backup names, in cloud-only placeholders, or in the trash.
3. **Classify.** One exact candidate gives `moved-auto`, two or more give `ambiguous`, a weaker candidate gives `moved-needs-confirm`, nothing gives `missing`. An exhausted budget gives `unverified`, never a guess.

<details>
<summary>The evidence, in the order it is tried</summary>

- **Captured intent:** a `file mv` intent record, or a file-id chain an optional hook captured right after an agent's `mv`.
- **The parent directory's id.** NTFS keeps a directory's id across renames and moves, so one lookup re-binds every linked file below a moved directory; macOS and Linux have their own equivalents. The file found there must still match.
- **The file's own id**, verified by size and modification time or by content, because file systems reuse ids.
- **Nearby directories, old paths and recorded directory moves.**
- **Exact renames inside one git commit**, read from git's trees. Identical blobs are never paired by order.
- **A content-id search** over files created since the last settle.
- **Moved and edited:** a new file with the same name whose content overlaps the old by at least 80 % both ways. This only proposes.

Equal content is not identity: a candidate with the same content id is exact only when a git rename inside one commit or a captured intent corroborates it (on Windows, also an equal creation time that is unique in the scanned area). A file that existed beside the original is a copy, never a target. Similarity search runs only under `--deep` and only proposes. A present path whose content and file id both changed, with less than 29 % overlap both ways, is `replaced`. All thresholds are constants of the resolver version, so the same tree always gives the same answer.

</details>

<details>
<summary>How an anchor finds its text again</summary>

Moirai tries the line hint, the exact quote inside the anchor's scope, then anywhere in the file, using the context and window to choose between duplicates. Then comes a fuzzy match, which needs a similarity of at least 0.75 and a clear margin; a renamed function matches only a header of the same kind, so `acquire` never silently becomes `acquire_shared`. If only the scope survives, the anchor is `edited`; if nothing is found, `orphaned`. It never picks the match nearest to the old line number.

</details>

## Settling: when a new location is recorded {#settling}

Reads show where a file is now; settles write it into the versioned history. The settle points are the `SessionStart` hook (for the session's brief and the lane's own files, with a hard cap of 150 ms), `moirai links sync`, the explicit file verbs and `links fix`, `complete` (as a separate commit after the task's own), the merge ritual, and an optional git `post-commit` hook block. The rules for what a settle may write are strict:

- **By default, only exact evidence re-binds automatically:** the file's identity, a recorded intent, or a git rename inside one commit, with a single candidate. Ties are refused. A configuration key can let a single strong proposal re-bind too; such a re-bind stays marked as a guess until someone confirms it.
- **Equal content alone never re-binds.** An identical file might be a backup, a mirror or a vendored copy.
- **Absence never deletes.** A missing file keeps its edge and its last path, visible to every referrer, until someone decides. Only `file rm`, `links fix --drop`, `--same-as` or `--split`, and a settle that joins two file nodes claiming one path through an exact git rename record a removal, unless a configuration key tells Moirai to accept deletion commits seen in the main tree.
- **A reused path shows `replaced`**, never a silent `ok`.
- **Only the branch's writer tree writes.** Each branch has at most one designated working tree: for a lane, the worktree that `moirai lane open` names; for `main`, the tree set in the configuration key `files.main-tree`; for any other branch, the one tree bound with `moirai worktree bind DIR BRANCH`, which refuses a second tree for the same branch. A branch with no designated tree has no writer tree, so every tree only reads for it. The designated tree writes re-binds only while its git `HEAD` is on the branch's git line, and only for files whose latest observation it has received. On `main`, it writes only moves committed in its `HEAD`. Every other tree is a reader: its observations wait as pending rows until the writer tree sees the same move.
- **Quiescence first.** A settle re-checks each old path at least 50 ms after it last saw it absent, and drops the re-bind if the path came back: editors that save atomically rename the original away for a few milliseconds.
- **No lost updates.** Each re-bind is guarded by the file node's revision as read when its resolution started; if the node changed meanwhile, the re-bind waits for the next settle.

## Explicit verbs {#explicit-verbs}

The verbs that act on files themselves live under `moirai file` and `moirai links`, because `add`, `rm`, `move` and `link` already name node verbs. A file link itself is made with the node verb `moirai link` and its `--at` option, and removed with `moirai unlink --at`.

| Command | What it does |
|---|---|
| `moirai link ID --at SPEC` | Captures an anchor and writes the link in one commit; `--planned` links a file that does not exist yet. |
| `moirai unlink ID --at aN\|PATH` | Removes one anchor, and the edge with its last anchor. |
| `moirai file add PATH..` | Registers files without linking them. |
| `moirai file mv SRC.. DST` | Moves files or a directory with a crash-safe intent record and re-points every link in one commit. |
| `moirai file rm PATH..` | Deletes on purpose, with `--reason` and `--replaced-by`; without `--yes` it is a dry run. |
| `moirai file relink PATH --to PATH --after` | Records a move already made by other means. |
| `moirai file revert COMMIT` | Moves files back for a commit that moved or removed them. |
| `moirai file where PATH\|ID --evidence` | Shows the current path in each tree, old paths, proposals and evidence. |
| `moirai links check` | Lists links that are not `ok`; never writes. |
| `moirai links sync` | A settle: records every exact re-bind the rules allow. |
| `moirai links fix ID\|aN ...` | Records a decision: accept, confirm, point at a path, drop, re-capture an anchor. |
| `moirai links mentions` | Reports plain-text mentions of old paths; never rewrites a file. |
| `moirai links import --from-markdown GLOB` | A one-off migration of existing `path:line` citations in Markdown into links; it lists the ones it cannot place exactly. `--dry-run` shows the plan. |

`file mv`, `file rm` and `file revert` change the file system, so they are command-line only and run only in the writer tree of your branch. Elsewhere they refuse with exit code 5 and suggest a plain `mv`; the links follow by evidence once the code reaches the writer tree. A move across volumes is refused with exit code 7, because a copy followed by a delete has no order that survives a power loss. Moirai touches the git index only with `--git`.

<details>
<summary>The crash-safe protocol behind file mv and file rm</summary>

1. **Plan**, read only: check the tree, the destination and every affected file node, old path and path glob.
2. **Intent:** append a durable intent record to the log.
3. **File system:** rename without replacing anything, then flush both parent directories, so the rename is durable before the commit that records it. A directory moves as one rename.
4. **Commit:** one commit re-points every affected node, keeps old paths as aliases, records a directory move, rewrites path globs under it, and closes the intent.
5. **Recovery:** after a crash, the next writer or `moirai doctor` decides from the file system: roll forward if the rename happened, abort if not, and mark it `ambiguous`, deleting nothing, if both paths hold different files.

`file rm` works the same way. Its commit records `removed` with the reason and replacement, re-points whole-file links, and anchors whose text is found there, to the replacement, and makes every referrer `suspect`.

</details>

## Worktrees, branches and the git image {#worktrees-and-branches}

Agents often work in several git worktrees of one repository at once, and Moirai has its own branches (see [how versions work](versions.html)). File links therefore live in two layers:

|  | Link intent | Resolution |
|---|---|---|
| Contents | file nodes, anchors, `at` edges, removal intent, directory move history | link states, OS file ids, caches, evidence, proposals |
| Scope | one branch | one working tree |
| Versioned and merged | yes | no |
| In the git image | yes | never |

Git versions the files; Moirai versions the links. Branches carry link intent, merges combine it by typed rules, and the [git image](git-image.html) exports it: file nodes are ordinary `.moi` files, anchors are `anchor` lines in the citing node's file, and nothing machine-local is ever exported. A destination can be set to receive anchors without their quoted text.

Resolution is per working tree. A tree that has not received a move says so with `pending` or `absent-in-tree` instead of re-binding the link back. Merges never read the file system, so they stay pure functions of history; a path disagreement becomes a conflict value that a later settle resolves by observation. History commands such as `undo` and `revert` never touch the working tree: links follow the files wherever they are.

## No watcher, no git process, no idle work {#no-watcher}

Moirai runs no file watcher, daemon, timer or polling loop. All work happens inside the commands and hooks you run, so idle CPU use is zero. A watcher would also lose events, and on Windows its handles on subdirectories make renames of their parents fail.

Git evidence comes from an in-process reader of git's objects. No link resolution starts a git process; the only one in this part of Moirai is the opt-in `file mv --git`. Without git everything still works, minus the git evidence, and a missing file is never treated as deleted.

Hooks are optional accelerators, each behind a configuration key. A `PostToolUse` hook on an agent's `mv`, `rm` or `Move-Item` captures the file-id chain while it is still exact; where the harness runs hooks as MCP tool calls that start no new process, an edit hook refreshes a linked file's id after each edit, which keeps an edit followed by a move exact. Git hook blocks are installed only on request, with `moirai hooks install --git`.

Moirai opens project files read-only with full sharing, one at a time, and never maps, locks or writes them, or adds ids or alternate data streams to them. Cloud-only placeholders are never downloaded by an automatic path; such links show `unverified (cloud-only)`.

## Time budgets {#time-budgets}

Link resolution sits on every read, so it has its own budgets. These are design targets, not measurements:

| Situation | Target |
|---|---|
| Link work for a pack with 50 links, all present | ≤ 3 ms median when idle, ≤ 5 ms under load |
| The same pack, all 50 files in one moved directory | ≤ 5 ms median |
| The same pack, 10 files edited since the last settle | ≤ 5 ms median |
| Git work on a read path | ≤ 10 ms at the 99th percentile |
| `SessionStart` settle | ≤ 150 ms, and ≤ 16 KB of log when nothing changed |
| Log bytes written by reads; CPU when idle | zero; zero |
| Memory for a read that resolves anchors in a 16 MiB file | ≤ 4 MB |

Reads count file-system work in units rather than time, so the same tree state always cuts off at the same link. Links a read did not reach are `unverified (budget)`, and the command exits with code 10. The query language exposes the same states through `link_state()`; see [Lachesis](lachesis.html).
