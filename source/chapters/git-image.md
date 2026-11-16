---
title: Saving into git
nav: Git image
description: How Moirai writes its graph and history into an ordinary git repository as readable text files, and how it loads edits made there back into the store.
---

# Saving into git

Moirai can write its whole graph and its history into an ordinary git repository: one small text file per node, and one git commit per Moirai commit or per checkpoint. Any git tool can read, diff and move that repository, and Moirai can load it back, including edits that people made with git. The store stays the source of truth. Git is a place to keep and move a copy of the data, and Moirai never needs git to run.

## Why write the graph into git {#why}

This copy is called the [git image](glossary.html#term-git-image). It gives you four things:

- **Travel.** The image is a normal git repository, so any git remote can carry it, and a plain `git clone` fetches every exported branch and tag.
- **Review.** Every node is a text file. `git diff` shows exactly which tasks, rules and notes changed, and any code-review tool can show the change to a person.
- **Loading back.** An image imports into another store, or into the same store after people edited it with git. Every imported commit is checked by the same merge engine as a local merge.
- **A copy outside the store.** By default the store lives inside the project's git directory and would be lost with it. A daily export of `main` and of every live lane keeps a copy of all work, including work that is not merged yet, in a separate repository.

Two rules shape the design. First, the mapping is a pure function of the data: two stores that hold the same Moirai commit, and have dropped the same bodies, write byte-identical git objects for it to destinations with the same object format and the same setting for anchor quotes. Second, the store is canonical: the image is derived from it, and git is never asked to answer a query.

<figure class="diagram">
<svg viewBox="0 0 720 320" role="img" aria-labelledby="git-image-d1-t">
  <title id="git-image-d1-t">The store exports to a separate image repository, which travels through a git remote and is imported back</title>
  <defs><marker id="git-image-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path class="d-arrow" d="M0,0 L10,5 L0,10 z"/></marker></defs>
  <rect class="d-box-accent" x="20" y="60" width="180" height="110" rx="8"/>
  <text class="d-text-strong" x="110" y="92" text-anchor="middle">Moirai store</text>
  <text class="d-text-small" x="110" y="116" text-anchor="middle">the source of truth</text>
  <text class="d-text-small" x="110" y="138" text-anchor="middle">&lt;git-common-dir&gt;/moirai/</text>
  <rect class="d-box" x="280" y="60" width="180" height="110" rx="8"/>
  <text class="d-text-strong" x="370" y="92" text-anchor="middle">image repository</text>
  <text class="d-text-small" x="370" y="116" text-anchor="middle">a bare git repository</text>
  <text class="d-text-small" x="370" y="138" text-anchor="middle">one .moi file per node</text>
  <rect class="d-box" x="540" y="60" width="160" height="110" rx="8"/>
  <text class="d-text-strong" x="620" y="92" text-anchor="middle">git remote</text>
  <text class="d-text-small" x="620" y="116" text-anchor="middle">any git host</text>
  <text class="d-text-small" x="620" y="138" text-anchor="middle">clone, fetch, push</text>
  <line class="d-edge-accent" x1="202" y1="95" x2="278" y2="95" marker-end="url(#git-image-arrow)"/>
  <text class="d-text-small" x="240" y="85" text-anchor="middle">export</text>
  <line class="d-edge" x1="278" y1="135" x2="202" y2="135" marker-end="url(#git-image-arrow)"/>
  <text class="d-text-small" x="240" y="155" text-anchor="middle">import</text>
  <line class="d-edge" x1="462" y1="95" x2="538" y2="95" marker-end="url(#git-image-arrow)"/>
  <text class="d-text-small" x="500" y="85" text-anchor="middle">push</text>
  <line class="d-edge" x1="538" y1="135" x2="462" y2="135" marker-end="url(#git-image-arrow)"/>
  <text class="d-text-small" x="500" y="155" text-anchor="middle">pull</text>
  <text class="d-text-muted" x="110" y="196" text-anchor="middle">gitmap records each export</text>
  <text class="d-text-muted" x="620" y="196" text-anchor="middle">push and pull use git</text>
  <line class="d-edge-dashed" x1="370" y1="173" x2="370" y2="227" marker-start="url(#git-image-arrow)" marker-end="url(#git-image-arrow)"/>
  <text class="d-text-small" x="380" y="204">read and commit</text>
  <rect class="d-box-muted" x="250" y="230" width="240" height="60" rx="8"/>
  <text class="d-text-strong" x="370" y="252" text-anchor="middle">git tools and people</text>
  <text class="d-text-small" x="370" y="274" text-anchor="middle">log, diff, review, hand edits</text>
</svg>
<figcaption>Export writes git objects into a separate image repository. Push and pull move that repository through any git remote. Import reads new commits back into the store, including commits that people made with ordinary git tools.</figcaption>
</figure>

## What the image looks like {#tree}

Each git commit of the image holds a tree with a fixed layout:

```text
.klotho-image                    the format marker
nodes/<h1>/<h2>/<uid>.moi        one file per node, live or deleted
schema/kinds.moi                 the project's own kinds
schema/fields.moi                the project's own fields and values
schema/edges.moi                 the project's own edge kinds
schema/policy.moi                policy rows
schema/queries/<q>.moi           one file per saved named query
refs/heads.moi, refs/tags.moi    checkpoint commits only: the checkpointed ref
```

- **Files are named by uid.** Every node has a [uid](glossary.html#term-uid), a 128-bit identity written as 32 hex digits; for most kinds it is random. `<h1>` is its first two digits and `<h2>` the next two, so the files spread over 65,536 small directories. A commit that touches k nodes rewrites only the root tree, the `nodes/` tree and at most k trees on each of the two levels below it; git shares everything else with the previous commit.
- **No short ids.** The short ids you type, such as `#31`, belong to one store, so no file name and no file content uses them. A store's own numbers travel separately, as a hint (see [Where the image goes](#destination)).
- **Portable names.** Every name in the tree is lower-case hex or a fixed ASCII word, so the tree checks out on case-insensitive file systems on Windows, Linux and macOS.
- **Out-edges only.** An edge is written in the file of the node it starts from. Adding "#31 blocks #33" changes the file of `#31` only, and an importer rebuilds the incoming edges. A rule cited by thousands of nodes does not change at every new citation.
- **Deleted nodes keep a file.** A deleted node becomes a tombstone file that stays for the life of the image. A file that disappears from the tree therefore always means a deletion made outside Moirai.

The marker file holds exactly three lines: the image format version, the git object format of the repository, and the schema version.

```text
klotho-image 1
object-format: sha1
schema-version: 1
```

Nothing that belongs to one store is in the tree: no store id, no short id, no sequence number. That is what lets two stores write the same root tree for the same commit.

## A node file {#node-file}

A node file is canonical text: for a given state there is exactly one correct byte sequence, and the exporter is the only writer of that form. Here is a task from a fictional project, `tinyhttp`, at `nodes/1d/60/1d603c9a852269c3c1999802c8096f81.moi`:

```moi
klotho-node 1
uid: 1d603c9a852269c3c1999802c8096f81
kind: task
title: Parse chunked request bodies
status: in_progress
priority: P1
parent: bb385cbb661b2c11aa530d9d4d0e1086
order: a1
created: caabe37fbc25f8f1060b7642d0afed1da6c1cdc823615a20f956f3b49fdd066f9 2026-10-29T11:02:47.508Z
updated: c6fc5a6ba2f9796994af5dddcfbed9d0d03f53ade37b98e02a819f2fff618473b 2026-11-03T16:40:12.031Z
field assignee: dev#2
field estimate: 3
field work_kind: impl
label http1
label parser
incr reopen_count +1 cf01a96e74df2272dd98c9e459ce44971b10f55714559e773b82fa94ab18404ea
edge blocks -> 76283b800bb759c8971d23cff0681316
edge cites -> 3c0d6718493b2f50b2382e744ed8d4b4 pin=cd29c8a17ca87c083b4956969e5b1d2a370fc3e850f174eb3982c3feeead985ba
edge mentions -> 50fd4f8ea2c235126ef09a2c10f6cd4e
---
Read each chunk size as hexadecimal and reject sizes above max_body.
Trailer fields after the last chunk are ignored for now.


```

Reading it from the top:

- `klotho-node 1` names the file format and its version. (Klotho is the storage engine inside Moirai; the image uses its name.)
- Header lines come in a fixed order: `uid`, `kind`, `title`, `status`, `resolution`, `priority`, `criticality`, `confidence`, `authority`, `parent`, `order`, `created`, `updated`, `deleted`, `flags`. A `priority`, `criticality`, `confidence` or `authority` line whose value is the default is left out, and so is a `resolution` line at `none`; a `status:` line is written at every status, the kind's initial status included. This task's criticality is the default, `normal`, so there is no `criticality:` line, and a task at the default priority `P2` would have no `priority:` line.
- `parent` and `order` place the node in the task tree. A reference to another node is always that node's uid.
- `created` and `updated` name the commits that created the node and last changed this file, followed by the commit's time so a person can read it. An importer uses only the commit id of `created`.
- `field` lines hold the other fields, sorted by name; `label` lines hold the labels.
- The `incr` line is one entry of a counter's ledger (see [below](#counters)).
- `edge` lines are the node's outgoing edges. `pin=` pins a citation to one commit.
- Everything after `---` is the body.

Derived state is never written: no `ready`, no count of open blockers, no "conflicted" flag. An importer recomputes all of it.

### Bodies are byte-exact {#bodies}

The body is written as its bytes followed by exactly one extra newline, and a reader strips exactly one. A body that ends in a newline, like the one above, therefore gives a file that ends with two newlines, and the file never ends without a newline, so `git diff` never shows "No newline at end of file". Markdown hard breaks (two trailing spaces), a `---` line inside the body and several final newlines all survive. The only change a body ever gets happens in the store when it is written: CR LF and a lone CR become LF. An importer applies the same rule to a file that arrives from git with CR LF, and never touches the bytes otherwise.

A body that the store has dropped, for example a secret pasted by mistake, is exported as its hash and the reason for the drop (`--- dropped <hash> secret`), never as text.

### Counters as ledger lines {#counters}

A counter such as `reopen_count` is never a single `field` line. It is a ledger: one `incr` line per commit that changed it, with the signed change and the commit id. The value is the sum of the lines. Two branches that both increase a counter add two different lines, so neither increase is lost when the changes come together.

### Conflicts in a file {#conflicts-in-files}

A [conflict value](versions.html#conflicts) left by a merge is exported too, as a `conflict` line with its class and three sides. While the line exists, the ordinary line for that key is left out. For a task with three open conflicts:

```moi
conflict body class=TextHunk base="Chunks are read into one buffer.\n" ours="Chunks are streamed to the handler.\n" theirs="Chunks are read into a pooled buffer.\n"
conflict field.estimate class=FieldEdit base=3 ours=5 theirs=
conflict status class=StatusFork base=in_progress ours=done/completed theirs=cancelled/obsolete
```

That file has no `---` body section, no `field estimate:` line and no `status:` line until the conflicts are resolved. The empty `theirs=` means that side removed the estimate. Structural violations never appear in an image: they exist only on staging refs, and staging refs are never exported.

<details>
<summary>What a deleted node's file looks like</summary>

A tombstone keeps the uid, the kind, the title, the deleting commit, the reason and the replacement, and the outgoing edges the store keeps for the dead node. Here task `#44` was deleted without a replacement while it still blocked `#31`:

```moi
klotho-node 1
uid: 6d34a01602266e14073edcf6b792dc62
kind: task
title: Fixed-length body limits
deleted: c919c2b236948a403dabebf30b6b6fa372b81d483c184ae329a57d9fbea8b2e8d 2026-11-04T10:21:09.442Z
field reason: superseded by the streaming reader
edge blocks -> 1d603c9a852269c3c1999802c8096f81 flagged
```

The `flagged` blocker edge survives the round trip. After any import, `#31` stays out of the ready queue until someone resolves that edge, so a deletion never unblocks work silently.

</details>

## Commits and trailers {#commits}

An export writes git commits at one of two granularities, chosen per destination:

- **`checkpoint`** (the default): one git commit per export of each branch that changed, holding the branch's state at that moment. The commits in between are folded into it.
- **`commit`**: one git commit per Moirai commit, with the full history. This covers every commit reachable from the exported branches through both parents, so the `main` commits that a lane's sync refers to are exported with the lane.

Every git commit is built from Moirai data alone. Its tree is the image at that commit. Its parents are the git commits of the Moirai parents; merges and syncs have two. Author and committer are the same line, made from the commit's actor and role, for example `klotho/dev#2 <developer@klotho.invalid>`, with the time taken from the commit's own clock. The exporter's identity and wall clock never enter, and no signature or extra header is written, so exporting the same commit again gives the same bytes.

The message is the Moirai message, an empty line and a block of [trailers](glossary.html#term-trailer). For the commit that last changed the task above, a claim on `lane/chunked`:

```text
claim --start

Klotho-Commit: c6fc5a6ba2f9796994af5dddcfbed9d0d03f53ade37b98e02a819f2fff618473b
Klotho-Kind: ordinary
Klotho-Ref: lane/chunked
Klotho-Hlc: 117553496852463619
Klotho-Actor: dev#2
Klotho-Role: developer
Klotho-Session: claude:s7
Klotho-Git-Head: sha1:30be61019cea5cb18bae586f614ddff4c5a32a68
Klotho-Git-Branch: chunked
Klotho-Worktree: /work/tinyhttp-lanes/chunked
Klotho-Schema: 1
Klotho-Ops: 1
Klotho-Idem: 20a73b5a599d8486e079c2166fbd6f11
```

A Moirai commit id is the BLAKE3-256 hash of the commit's canonical form. Every field that enters that hash has exactly one place in the git commit: the kind; the parents' ids, which are the `Klotho-Commit` trailers of the parent commits; the hybrid clock in `Klotho-Hlc` (the author time alone loses the milliseconds); the actor, role and session; the code-repository context of the write (git head, branch, worktree and base); the message; the schema version; the origin of a revert or cherry-pick; and the changes themselves, which are the difference between this tree and the first parent's tree. An importer rebuilds the canonical form from these carriers, recomputes the hash and compares it with `Klotho-Commit`. `Klotho-Ops` is a cheap pre-check: the number of changes. `Klotho-Ref` and `Klotho-Idem` are informational and are not hashed.

A checkpoint commit has no `Klotho-Commit`, because it stands for a range of commits rather than one. Its message is `checkpoint <ref>` with the trailers `Klotho-Kind: checkpoint`, `Klotho-Head` (the newest Moirai commit it covers), `Klotho-Ref` and `Klotho-Folded` (how many commits it folds, from which to which). Its parent is the previous checkpoint of the same branch.

<details>
<summary>All trailers</summary>

| Trailer | Written when | Hashed? |
|---|---|---|
| `Klotho-Commit` | every native commit | it is the id being checked |
| `Klotho-Kind` | always: `ordinary`, `merge`, `sync`, `revert`, `cherry-pick` or `checkpoint` | yes |
| `Klotho-Parent` | only when a parent's stated id differs from the one its git parent gives | yes |
| `Klotho-Head`, `Klotho-Folded` | checkpoint commits | no |
| `Klotho-Ref` | the branch the commit landed on | no |
| `Klotho-Hlc` | every native commit | yes |
| `Klotho-Actor`, `Klotho-Role`, `Klotho-Session` | when not empty | yes |
| `Klotho-Git-Head`, `Klotho-Git-Branch`, `Klotho-Worktree`, `Klotho-Git-Base` | when the write happened in a git worktree | yes |
| `Klotho-Origin` | reverts and cherry-picks | yes |
| `Klotho-Sync-Base` | syncs: a readable copy of the second parent's id | no (a copy) |
| `Klotho-Foreign-Git` | a commit first imported from git, written again to another destination | yes |
| `Klotho-Schema`, `Klotho-Ops` | every native commit | `Klotho-Schema` yes; `Klotho-Ops` no |
| `Klotho-Idem` | writes that carried an idempotency key | no |

An exporter writes the trailers in one fixed order, and an importer accepts them in any order. A trailer whose value would be empty is not written.

</details>

## Where the image goes {#destination}

By default the image is a separate bare git repository next to the project: `<parent of the main worktree>/<main worktree name>-moirai.git`. For a project checked out at `/work/tinyhttp`, that is `/work/tinyhttp-moirai.git`. A separate repository keeps the size of the image's history out of the project's own repository. `moirai image export --create` sets it up with an explicit object format, plain-file ref storage, automatic `git gc` turned off, and `*.moi -text diff` in its `info/attributes`, so git does not convert line ends in node files in that repository. A clone does not receive this file; for a clone that converts line ends, the importer turns CR LF back into LF.

| Moirai ref | Git ref in the image repository |
|---|---|
| `main` | `refs/heads/main` |
| `lane/<name>` | `refs/heads/lane/<name>` |
| `plan/<name>` | `refs/heads/plan/<name>`, when selected |
| `tags/<name>` | `refs/tags/<name>` |
| staging refs (`merge/*`, `import/*`) | never exported |

By default an export selects `main`, `tags/*` and `lane/*`. Beside these, each exporting store keeps one side ref, `refs/klotho/meta/<store-id>`. It is never part of any commit's tree and is not hashed. It holds the store id, the image format version, the granularity of each exported ref, the last exported sequence number, and alias files that map each uid to that store's short id. An importer reads it only as a hint. With `--with-oplog`, a second side ref, `refs/klotho/ops/<store-id>`, carries branch-pointer moves and client heads, so reflog history can be reproduced elsewhere.

### When exports happen {#cadence}

- `main` gets a checkpoint after each merge into `main` (setting `image.export.on-merge-to-main`, on by default), once a day, and on every explicit export.
- Every live lane gets a checkpoint once a day and on every explicit export. Its parent is the lane's previous checkpoint, so unmerged lane work always has a copy outside the store.
- The daily export needs no timer. The first session start after `image.export.max-age` (one day by default) runs an incremental export. A harness without hooks runs `moirai image export --if-older` instead, as the orchestrator's first step and on a few write paths. This is best effort: an export that a sandbox refuses prints one line and never fails the write that carried it, and `moirai brief` and `moirai doctor` warn when the last export is too old.

Images use SHA-1 object ids by default, for anything that leaves the machine; SHA-256 is allowed for local repositories. One destination never mixes the two, and `moirai doctor image` warns before a SHA-256 image is pushed to a remote that has not announced support for it.

## Export, import, push and pull {#export-import}

| Command | What it does |
|---|---|
| `moirai image export [--to DEST] [--refs ...] [--granularity checkpoint\|commit] [--create] [--if-older D]` | Writes the new commits of the selected refs into a repository or a bundle file. It is incremental: for each ref it walks back only to the newest commit already exported. |
| `moirai image import [--from DEST\|FILE] [--refs ...]` | Reads new git commits from a repository or a bundle file, checks them and appends them to the store. |
| `moirai image push [REMOTE]` | Sends the image repository to a git remote. It runs `git` when git is installed and otherwise prints the exact command. It pushes only to remotes listed in `image.allowed-remotes`. |
| `moirai image pull [REMOTE]` | Fetches from a remote, then imports. |
| `moirai image doctor [--rebuild-map]`, `image show`, `image gc` | Check the image against the store, show one image commit, and repack the image repository. |

The store keeps a table called [gitmap](glossary.html#term-gitmap): which git commit each Moirai commit became, per destination and object format. An export writes its pack files and refs to disk first and records them in gitmap last, so a crash leaves the destination at most one step ahead of gitmap, which the next export detects and repairs, and never behind it. gitmap can also be rebuilt from the image itself by reading the `Klotho-Commit` trailers. Each ref update checks that the ref still has the value this store last saw; if someone else moved it, the export stops (exit code 6) and asks for an import first.

Export and import hold the store's writer lock only to append their records, so agents keep working while an export runs. The design target for an incremental checkpoint export of `main` and two lanes is 50 ms or less.

A designed session in the `tinyhttp` project. First an export:

```console
$ moirai image export
export main tags/* lane/* (checkpoint, sha1) -> /work/tinyhttp-moirai.git
  main: 1 checkpoint commit (Klotho-Folded: 14 commits), 9 blobs, 21 trees
  lane/chunked: 1 checkpoint commit, 3 blobs; lane/keepalive: unchanged
  refs updated: refs/heads/main, refs/heads/lane/chunked, refs/klotho/meta/0c4e9a71...
```

*Designed output.*

Then a colleague clones the image repository with plain git. On `main` they fix a typo in the `title:` line of a node file and commit. On `lane/chunked` they run `git merge --allow-unrelated-histories main` (at checkpoint granularity the checkpoint chains of different branches share no git commit), get a text conflict in one node file, and commit that file with git's conflict markers still in it. They push both branches back. The next import reads both commits:

```console
$ moirai image import
import refs/heads/main: 1 new git commit
  4be1a90 foreign  author git:dev@example.org "Fix typo in task title" -> 1 op SetField(#31.title) -> applied on main
import refs/heads/lane/chunked: 1 new git commit
  9c03d7e foreign merge  nodes/1d/60/1d603c9a852269c3c1999802c8096f81.moi: line 9 matches no rule (git conflict marker) -> staged on import/lane/chunked (ImageParse)
```

*Designed output.*

The first commit has no `Klotho-Commit` trailer, so it is a [foreign commit](glossary.html#term-foreign-commit): its changed lines become one ordinary field change on `main`. The second commit left git's conflict markers in a node file. That file does not parse, so the commit waits on the staging ref `import/lane/chunked`, and `lane/chunked` in the store does not move.

### Edits made with git {#foreign}

A git commit whose trailers verify is a native commit: it is appended with its own id and marked verified. A checkpoint commit (`Klotho-Kind: checkpoint`, no `Klotho-Commit`) is imported as an import-checkpoint commit with a deterministic id. Every other commit is foreign. A native commit whose hash does not match is demoted to foreign on its own; its children still verify against the parent ids they state. A foreign commit gets a deterministic id: its kind is `ordinary` (or `merge` for two parents), its actor is `git:<author email>`, its time comes from the committer time and is always later than its parents', its message is the git message, and the git commit id is recorded with it. Two stores that import the same foreign commit therefore give it the same id. Every imported commit is checked by the same validators as a merge.

| What happened in git | What the import does |
|---|---|
| A `.moi` file was edited by hand. | A foreign commit. The changed lines become ordinary changes: a field, a status, an edge, the body. |
| Two exported branches were merged with `git merge`. | The commit is imported as a typed Moirai merge of the two parents' states. Git's merged files are used only to see how a person resolved conflicting lines of text. Counters come from the typed merge, never from a text merge. Anything else that disagrees becomes a conflict value. |
| A node file was deleted. | A foreign deletion, written back as a tombstone on the next export. If the deleted node still blocked other tasks, its blocker edges stay, flagged, so those tasks remain out of the ready queue: a deletion by hand never unblocks work silently. |
| History was rewritten with rebase or squash. | Trailers survive in commit messages. A commit whose tree matches the recomputed one is mapped again; the others are foreign. Export refuses to move a ref it does not recognise until an import has run. |
| Two stores export to one repository. | The second store's ref update fails. It imports first, verifying the other store's native commits, and then exports. |

When the local branch has moved on since the imported commits were based on it, the imported chain lands on `import/<ref>`, and the importer merges it into the local branch with the typed merge, over their last common exported commit. The setting `image.import-merge = stage` stages every such import for a person to merge instead.

Short ids are local to a store, so an import matches nodes by uid. A short id from the other store's side ref is kept only when it is still free here; otherwise the node gets a new local number, and `moirai show` prints the old one beside it, as in `#40 (was #17 in store 6f2a…)`. The text of an imported body is never re-read for `#N` mentions, because those numbers belong to the store that wrote them; mention edges come only from `edge mentions` lines.

### When a file does not parse {#parse-errors}

An import never takes a file it does not understand. These problems stage the violation `ImageParse`, with the path, on `import/<ref>`; the local branch does not move:

- leftover merge markers (`<<<<<<<`, `=======`, `>>>>>>>` lines);
- a line that matches no rule of the format, or an unknown header key;
- a line repeated, such as two `status:` lines or two `conflict` lines for one key;
- a `uid:` line that differs from the file name;
- a kind, field, edge kind or value that the commit's schema does not know, or a value that does not fit its field's type;
- a missing or wrong `.klotho-image` marker, or an unexpected file under `nodes/`, `schema/` or `refs/`.

Within the format the importer is tolerant. It accepts a byte-order mark, CR LF line ends, trailing spaces, lines in another order, a default value written out, and a hand-written node file without `created:` and `updated:` lines, and it normalises them all.

A staged import whose violations name a key is finished like a staged merge: `moirai resolve` on `import/<ref>`, then `moirai merge --continue import/<ref> --into <ref>` (see [Structural violations](versions.html#violations)). An `ImageParse` violation names no key, only the file's path.

## What survives a round trip {#round-trip}

| Data | Survives export and import? |
|---|---|
| Nodes, fields, labels, counters, edges with their properties, schema, named queries | Yes. |
| Bodies | Yes, byte for byte. |
| Tombstones, with their reason, replacement and flagged blockers | Yes. |
| Conflict values | Yes. |
| [File links](file-links.html): file nodes, anchors, directory moves | Yes. Anchor quotes can be written in full or as digests only (`hash-only`). Where each file is in a given worktree right now is not exported; the importing store finds the files in its own trees. |
| The commit graph and commit metadata (parents, merges, syncs, revert origins, actor, role, session, message) | Yes, at `commit` granularity. |
| Moirai commit ids | Yes, recomputed and verified. |
| Branch and tag names | Yes. |
| Commits between two checkpoints | No, at `checkpoint` granularity: the state at each checkpoint is exact, the path to it is folded. |
| Short ids such as `#31` | Best effort, through the side ref and the alias map. |
| Reflog, operation log and client heads | Only with `--with-oplog`. |
| Structural violations and the choice a resolution made | No, by design: violations live only on staging refs, and a resolution is exported as the value it produced. |
| Leases, completion and deletion markers, idempotency results, the id counter, internal snapshots | No, by design: this is runtime state. The importing store derives its markers again from the imported commits and merges. |
| Derived state, such as the ready queue | Recomputed, never written. |
| The store's own file bytes | No: the image is a logical copy, not a backup of files. Moirai has a separate `moirai backup` for that. |

The design states these as tests. At `commit` granularity, export, a fresh import and a second export must give byte-identical objects and refs, at a scale of 100,000 nodes and 100,000 commits, in both object formats. At `checkpoint` granularity, every exported branch must arrive with an identical state, and the tasks behind flagged blockers must still be out of the ready queue.

## Git is optional {#git-optional}

Moirai never requires git. With no `git` program, no git library and no repository, it still runs every read and write command, branches and merges, tags, undo and revert, the hooks, the MCP server, every query, and `image export` and `image import` to and from a local repository directory or a bundle file.

That is possible because Moirai has its own git object layer, written from scratch and run in-process. It reads and writes loose objects, pack files and their indexes, commit-graph files, loose refs and `packed-refs` with git's lock-file protocol, and bundles, in SHA-1 and SHA-256, and it reads repositories that store refs as reftable. No git library is linked, and the build's dependency check forbids one. The git command-line tool serves as an independent check in tests and, when it is installed, in the four optional places below.

Reading git's files in-process, without starting git, Moirai finds the repository a command runs in, records which git commit, branch and worktree a write happened in, and answers "is this git commit an ancestor of the worktree's tip?" for its `check` and `stale` commands. It starts `git` only when git is installed, and only in four explicit, optional places: `image push` and `image pull` (otherwise it prints the command); ref updates in an image repository that uses reftable; `moirai doctor lanes --refresh-graph`; and `moirai file mv --git`. None of them is on a read, query, claim or hook path.

### Where the store lives {#store-location}

The store itself is not in the image: it is a directory of binary files that only Moirai writes. Where `moirai init` puts it, and how a command finds it, is described on the engine page, in [Where the store lives](engine.html#store-location). Inside a git repository the store goes into git's own directory, at `<git-common-dir>/moirai/`, and is deleted together with that directory, which is one more reason for the daily export and for `moirai backup`.

> [!NOTE]
> **Where this stands**
>
> This chapter describes the designed system. The image format, including the grammar of `.moi` files and every trailer, is specified byte by byte in a chapter of the draft format specification. That chapter is waiting for review and is not frozen yet; milestone M0 freezes the format. In the build order, the in-process git object layer is milestone M4 and the git image is milestone M5; see [how Moirai is being built](building.html). Nothing here can be installed or run yet. The specification is in [docs/spec/format/14-image.md](https://github.com/bluesteelll/moirai/blob/master/docs/spec/format/14-image.md).
