# 12 — Precedents: how existing tools keep links to files alive (R4)

*Research for moirai. Date: 2026-09-26. Lens: PRECEDENTS. Status: research only, nothing implemented. The only file written in the repository is this report. Probes ran outside the repository (the probe scripts are not published); their temporary files were deleted.*

**Evidence tags.** **[M]** measured here, on the owner's machine (Windows 11 26200, NTFS, non-elevated). **[D]** documented behaviour, from primary docs, specs or source code. **[C]** claimed by a third party: issue reporters, forum users, vendor blogs. **[S]** secondary source such as a news site or someone's summary. **[I]** my inference. Timing caveat: during the probes the machine was busy with other lanes (WMI reported 100% CPU load when I checked; `git --version` alone took 4.3 s). Treat all timings below as upper bounds. I dropped my `git diff -M` timing because load made it meaningless.

**Owner requirement R4 (2026-09-26, translated).** Links from moirai to files must not break when files move. Two ways to get there: moirai commands to add, move and delete files, or a synchronisation system that re-binds references automatically when the user or an agent moves files by other means. Nodes will reference whole files and possibly locations inside files.

---

## 0. Summary

1. **Every tool that rewrites links on rename only does it for moves made through its own interface.** This holds for Obsidian, VS Code with LSP, IntelliJ, Dendron, Foam, Logseq, Unity, Unreal, DVC and `hg mv`. A move made outside the tool arrives as *delete + create*, or is never seen at all. The APIs say so explicitly. VS Code's rename events are "not fired when files change on disk, e.g triggered by another application" [D]. LSP `willRenameFiles` is sent "as long as the rename is triggered from within the client" [D]. LSP's watched-file events have no *Renamed* type at all: only Created, Changed and Deleted [D]. Obsidian "sees a delete followed by a create — so the links are not updated" [D, plugin README]. Explicit commands are **necessary but never sufficient**.
2. **Tools that handle outside moves either watch continuously or repair lazily.**
   - Watchers work only while the tool is running. Obsidian's External Rename Handler plugin pairs deletes and creates by inode "only if Obsidian is running during the external renames" [D]. org-roam catches only renames done through Emacs `rename-file` [D].
   - Lazy repair runs when a link is used or when an explicit check runs:
     - org-id rescans every file when an ID is not found [D];
     - Windows `IShellLink::Resolve` asks the tracking service and then searches nearby folders [D];
     - Zotero offers to relink all missing files under a base directory [C];
     - `p4 reconcile`, TortoiseSVN "Repair move" and `hg addremove -s` pair missing files with new ones by content [D];
     - git infers renames at diff and merge time and stores none [D].

   moirai's zero-idle-CPU rule excludes watchers, which leaves **lazy repair plus explicit commands**. That is the same combination the most robust precedents use.
3. **Identity must not be the path.**
   - The robust systems give each file an ID and treat the path as a mutable attribute. Examples: Unity GUIDs in `.meta` files, Godot 4.4 `uid://` with `.uid` sidecars, Unreal object paths with redirectors, org-mode `:ID:`, Notion page IDs, DLT object IDs, macOS bookmark CNIDs.
   - Where the ID lives *next to* the file (sidecars), the recurring failure is the sidecar not moving with it. Unity: "if you move or rename an asset outside of Unity, you must move or rename the `.meta` file to match" [D]. Godot has the same rule [D].
   - Where the ID lives *inside* the file, the recurring failures are duplicates on copy (org-id warnings [D], Unity "catastrophic" duplicate GUIDs [C]) and clutter. Godot testers found comment-embedded UIDs "quite disruptive" and rejected them [D].
   - moirai can keep the ID **inside its own store** (a file node with a uid and a versioned `path` field). Repository files then carry nothing.
4. **OS-level file identity helps, but on this machine it does not survive the agents' own edits [M].**
   - The NTFS 128-bit file ID survives rename, move, directory rename, `git mv` and in-place writes.
   - It changes on temp-file-then-rename saves, `ReplaceFileW`, copy+delete, and every git operation that rewrites a file (checkout of a differing file, stash, stash pop, `checkout --`).
   - **Claude Code's own Edit and Write tools replace the file: new file ID, NTFS object ID dropped [M].**
   - The NTFS object ID (the key DLT uses) survives `ReplaceFileW` but not temp+rename. It survives git rewrites only because NTFS *tunneling* re-attaches it when a same-name file is re-created within the ~15 s tunnel window. The same probe after 16.5 s lost it [M].
   - A non-admin process *can* find a moved file's new path from a remembered file ID via `OpenFileById` + `GetFinalPathNameByHandleW`, in about 0.5 ms and without scanning [M]. The USN change journal, by contrast, needs admin to read (access denied, error 5) [M]. That is why Everything installs a service [C].
   - Conclusion: use the file ID as a machine-local **hint** in a repair cascade, never as identity.
5. **How files actually move in the owner's main repo [M].**
   - BoykoEngine HEAD has 1,392 commits and 159 rename events in 16 commits. 150 of the 159 are exact-content renames (git R100), and 149 keep their basename.
   - One commit (a611da46, 2026-07-02, subject "test(render): shadow-motion A/B diagnostic harness") moved 131 docs into `docs/archive/` as a side effect.
   - Today, **89 of the 159 moved-away paths are still referenced verbatim: 188 references in 114 tracked files**.
   - 16% of tracked files (420/2,556) have non-unique basenames, e.g. `mod.rs` ×87, `Cargo.toml` ×29, `lib.rs` ×25. Obsidian-style "unique filename" resolution therefore cannot be the answer for code.
   - Content hashing plus a directory-prefix rule would have repaired almost all of these moves automatically.
6. **Locations inside files need selectors that do not depend on line numbers.**
   - ctags stores search patterns by default because line numbers go stale [D].
   - Emacs bookmarks store 16 characters of text on each side of the position [D].
   - Hypothesis falls back from range, to position, to a fuzzy match on the quote plus its prefix and suffix [D]. Its worst reported failure is an annotation that fails to anchor *without being reported as an orphan* [C].
   - Swimm re-syncs code snippets in CI from git history, or fails the check and asks a human to re-select the snippet [C].
   - Doorstop stamps each referenced file with a sha256 and flags "suspect" links when a fingerprint changes [D].
7. **Recommended combination for moirai (§6):**
   - a file node (uid + versioned repo-relative `path` + `aliases` + content hash) as the target of every file reference;
   - explicit `moirai file mv|rm|add|relink` as the precise, same-commit path;
   - a lazy, deterministic **reconcile cascade** run at command time, in fail-open hooks and at `merge-check`: path → file-ID hint → exact content → directory-prefix rule → basename + similarity → *missing/ambiguous, reported, never silently retargeted*;
   - old paths kept as aliases (redirectors);
   - anchors as selector bundles;
   - Doorstop-style `suspect` propagation when content changes;
   - a strict `check` gate.

   No watchers, no mandatory IDs inside code files, no dependence on git in the core.

---

## 1. The problem space

Operations that can invalidate a file reference, and who performs them in the owner's workflow.

| Operation | Typical actor in the owner's workflow | What a path-keyed reference sees |
|---|---|---|
| rename in place, move to another dir, rename a parent dir | owner (Explorer, IDE); agents via `mv`, `git mv`, `Move-Item`; bulk reorganisations inside unrelated commits (§3.5) | path gone; another path appears |
| edit in place | IDEs, some tools | path fine, content changed |
| atomic save (write temp, rename over) | **Claude Code Edit/Write [M]**, many editors | path fine; *new file identity* |
| delete | owner, agents, `git rm`, git checkout of a branch without the file | path gone, nothing appears |
| copy (+ delete = cross-volume move) | Explorer across drives, sync tools | new identity, maybe same content |
| branch switch / stash / rebase in a worktree | agents, merge scripts; 44 worktrees [01/02] | files vanish and reappear per checkout, and IDs change [M] |
| move *inside* a file (code moved within or between files) | agents, refactors | `file:line` rot: "184 of 282 anchors dead" [C, 01]; an 8-pass citation-repair campaign [C, 02] |

Constraints that filter the precedents:
- **zero idle CPU**: no watchers, no polling, no background work [02 §9], [30 §8];
- **Windows 11 first**: `ReadDirectoryChangesW` loses events on overflow [08 W10];
- **R1**: every datum branches;
- **R2**: no git dependency in the core;
- **R3**: git image.

Also, the "node 40" rule: everyone referencing something that was deleted must know about it.

---

## 2. Taxonomy of mechanisms

| Id | Mechanism | Examples | Needs a running process? |
|---|---|---|---|
| **M1** | **Explicit command**: move, rename or delete through the tool, which rewrites references | Obsidian, VS Code/LSP, IntelliJ, Dendron, Foam, Logseq, Unity, Unreal, DVC `move`, `hg mv`, `svn move`, `p4 move` | only during the command |
| **M2** | **Watcher**: observe file-system events live and pair delete+create into renames | Obsidian's vault watcher, External Rename Handler, IDE VFS, org-roam autosync (Emacs-internal), Godot/Unity editors | yes, continuously |
| **M3** | **Lazy repair / reconcile**: detect missing targets when used or on demand; pair by identity, content or heuristics | org-id rescan on miss, `IShellLink::Resolve` + DLT, macOS bookmark resolve, Zotero relink, `p4 reconcile`, TortoiseSVN Repair move, `hg addremove -s`, git rename detection, org-roam `db-sync`, Swimm CI | no |
| **M4** | **ID in or next to the file**: the reference carries an ID, the file carries the same ID | org `:ID:`, Logseq `id::`, Obsidian `^block`, Dendron frontmatter `id`, Zettlr IDs, Unity `.meta` GUID, Godot `.uid`, OpenFastTrace tags | no, but needs a scan or index |
| **M5** | **Path-independent resolution**: resolve by unique name or content, not full path | Obsidian "shortest path when possible", Logseq page titles, Doorstop filename search, git-annex/LFS/DVC content hashes | no |
| **M6** | **Forwarding records (redirectors)**: leave old → new mappings behind | Unreal redirectors, HTTP 301 (dvc.org → doc.dvc.org observed while researching), Notion slug redirect, DLT server move table | no |
| **M7** | **Gate**: refuse or flag at commit/CI time | git-annex pre-commit fix, Unity meta triggers, mcp_agent_mail pre-commit guard, Swimm CI, Bazel's loud build errors + Gazelle, VS Code Markdown link validation, Dendron Doctor | no |
| **M8** | **OS file identity**: inode / NTFS file ID / NTFS object ID / APFS CNID | DLT, macOS aliases/bookmarks, External Rename Handler's inode map | M8 on its own: no. DLT itself relies on the TrkWks service |

---

## 3. Measurements on the owner's machine [M]

Scripts: `{ident_probe,tunnel_probe,openbyid_probe,stat_probe,walk_probe}.py` (probe scripts are not published). Files were created only in a scratch directory outside the repository. The BoykoEngine repo was only read (`git log`, `git ls-files`, stat, read).

### 3.1 Which identity signals survive which operation

A fresh file, then one operation, then compare. Object IDs were created by the probe with `FSCTL_CREATE_OR_GET_OBJECT_ID`, and that works non-elevated.

| Operation | NTFS 128-bit file ID kept? | NTFS object ID kept? | Notes |
|---|---|---|---|
| rename, same directory | yes | yes | |
| move to another directory (same volume) | yes | yes | |
| rename a parent directory | yes | yes | |
| in-place truncate + write | yes | yes | |
| atomic save: write temp + `os.replace` (MoveFileEx REPLACE_EXISTING) | **no** | **no** (none) | the usual editor/tool save pattern |
| `ReplaceFileW` | **no** | yes | matches the docs: the object ID is preserved, "the resulting file has the same file ID as the replacement file" [D] |
| copy + delete (the shape of a cross-volume move) | **no** | **no** | copies never preserve object IDs [D] |
| `git mv` | yes | yes | git does a plain rename |
| git checkout of a branch where the file differs | **no** | yes* | *by tunneling, see 3.2 |
| `git stash` / `git stash pop` / `git checkout -- file` | **no** | yes* | *by tunneling |
| git checkout round-trip where the file is identical | yes | yes | git leaves unchanged files alone |
| **Claude Code `Edit` tool** | **no** | **no** (dropped) | the file is replaced |
| **Claude Code `Write` tool, overwriting** | **no** | **no** (dropped) | the file is replaced |

The changed file IDs from git showed the same MFT record number with an incremented sequence number (…`15`→`16`→`17`…`158e6c`) [M]. The record is freed and reused, and a stale ID never resolves to the new file.

### 3.2 NTFS tunneling carries the object ID, for about 15 seconds

| Case | file ID same | object ID same | creation time same |
|---|---|---|---|
| delete, re-create *same name* at once | no | **yes** | yes |
| delete, wait 16.5 s, re-create same name | no | no | no |
| delete, create a *different* name | no | no | n/a |

Documentation and news coverage describe tunneling as carrying the short name and creation time over a ~15 s window [S: windowslatest.com 2026-09-21; archived KB 172190]. That it also carries the **object ID** is my measurement. Consequence: an object-ID tracker "survives" git checkouts only by timing accident [I].

### 3.3 Relocation by ID without scanning

A file was moved from `a/f.txt` to `b/deep/g.txt`. `OpenFileById` (ExtendedFileIdType, with a directory handle as the volume hint) followed by `GetFinalPathNameByHandleW` returned the new path. The same worked with ObjectIdType. Cost was ~0.49 ms per lookup, non-elevated, on the loaded machine, in Python. After an atomic save, the old file ID no longer opens (error 87) [M].

A resolved path outside the worktree root must be read as "gone", for example a file in `$Recycle.Bin` after an Explorer delete. macOS aliases are reported to follow files into the Trash [C, Eclectic Light].

### 3.4 USN change journal

- Opening `\\.\C:` or `\\.\D:` with `GENERIC_READ` failed with error 5 (access denied), non-elevated.
- With desired access 0 the handle opened, but `FSCTL_QUERY_USN_JOURNAL` failed (error 1).
- Per-file `FSCTL_READ_FILE_USN_DATA` works non-elevated (a V2 record, 72 bytes) [M].

The Everything search tool installs a service because "low level read access to NTFS volumes" needs elevation [C, voidtools]. The journal is not available to moirai without a service or elevation.

### 3.5 How files actually move in BoykoEngine (read-only `git log -M`, HEAD)

| Quantity | Value |
|---|---|
| commits reachable from HEAD | 1,392 |
| rename events / commits containing them | 159 / 16 |
| exact-content renames (R100) / renames with edits | 150 / 9 |
| renames keeping the basename (pure moves) | 149 |
| cross-directory moves / same-directory renames | 154 / 5 |
| largest batch | 131 docs → `docs/archive/` in a611da46 (2026-07-02), a commit whose subject is a render test harness |
| deletions | 73 in 38 commits |
| moved-away old paths still referenced **verbatim** in tracked text files today | **89 of 159**: **188 references in 114 files** (e.g. `docs/PHASE-XI-PLAN.md` ×8, `crates/boyko_ecs/src/ecs/core/iters/query.rs` ×7) |
| of those 89, basename unique in today's tree | 86 |
| tracked files with non-unique basenames | 420 of 2,556 (16%); 117 names; `mod.rs` ×87, `Cargo.toml` ×29, `lib.rs` ×25, `README.md` ×8 |

Some of the 188 references may be deliberately historical, for example inside archived plans. They are still references to paths that no longer exist, and nothing flags them today.

### 3.6 Cost of lazy checks (upper bounds, loaded machine, Python)

| Check | Cost |
|---|---|
| existence + size + mtime via `GetFileAttributesExW` (no handle), 2,556 tracked files | 65–75 ms, **~25 µs/file** |
| open handle + read 128-bit file ID, 2,556 files | 227–327 ms (~90–130 µs/file) |
| full enumeration of the BoykoEngine tree (7,549 files outside `.git`/`target`), names+sizes+mtimes | 150–230 ms |
| `git ls-files` (spawn + index read) | 250–285 ms |
| BLAKE2b of all 2,556 tracked files (70 MB) | 2.1 s warm, 4.9 s cold |
| directory mtime changes on: rename in/out, move in/out, atomic save | yes |
| directory mtime changes on: in-place edit | no |

Implication [I]: verify only the references being rendered (µs each). Enumerate the tree only when something is missing. Hash only size-matched candidates, never the whole tree on a routine command. Directory mtimes are a cheap filter for "did anything enter or leave this directory".

---

## 4. Precedents in detail

Each entry gives the mechanism, what breaks, reported failures and the lesson.

### 4.1 Note-taking and personal knowledge tools

**Obsidian.**
- *Mechanism:* M1 + M5 + M2 (watcher, but no rename pairing).
  - An in-app rename rewrites links: "Obsidian can automatically update internal links in your vault when you rename a file" (Settings → Files and links) [D].
  - Links resolve by name. With "Shortest path when possible", `[[Note]]` resolves to the file with that name anywhere in the vault, so a pure move of a uniquely named note breaks nothing. A forum user notes links may not break "if note names are unique and folder paths aren't used in links" [C].
  - Block links write an identifier into the target note ("a blank space followed by a caret `^` and the block identifier") [D]. That is M4, injected lazily on first reference.
- *What breaks:*
  - External renames: "Rename the same file in your file manager, from a script, or through a sync client, and Obsidian sees a delete followed by a create — so the links are not updated" [D, External Rename Handler README]. Requests for this date back to 2021 (Hazel rules) [C].
  - Files moved while Obsidian is closed: an open feature request from Nov 2024 [C].
  - Name resolution is "global": when a second file with the same name appears, links point "to whichever file matches", which silently retargets or forces full paths [C, forum 2022].
  - Links to nested headings not updating on heading rename [C, forum 2025].
- *Workaround:* the External Rename Handler plugin keeps a persistent path↔inode map in IndexedDB. It pairs a delete with a later create of the same inode and reports it to Obsidian as one rename, with a configurable deletion-rename timeout [D, source `path-ino-map.ts`]. It "works only if Obsidian is running during the external renames" and rebuilds its map at startup, so offline renames are lost [D].
- *Lesson:* in-app rewriting plus name-based resolution covers the common case. Everything outside the app becomes delete+create, and even with a persistent inode map, pairing needs someone watching. Name-based resolution fails silently once a name stops being unique.

**Logseq (file graphs).**
- *Mechanism:*
  - Page references are by title (M5). An in-app page rename rewrites references (M1).
  - Block references are UUIDs (M4). The UUID is persisted into the Markdown file as an `id::` property only when the block is referenced; other blocks get temporary IDs that "every re-index generates new" [C, logseq-cli issues #95/#31; logseq #4297].
- *Reported failures:*
  - When a file is reloaded from disk (an external edit or sync), the reload deletes and re-adds the file's blocks, and "references from other pages to that UUID will not be restored — they are deleted when the original block is deleted" (#7362, 2022, closed as stale) [C].
  - The rename-rewrite has a long tail of missed reference forms: inline aliases (#9202), tags with special characters (#4356), nested links (#1489), page properties (#9129) [C].
- *Status:* the DB version, where SQLite is the canonical store and Markdown is export-only, is reported in public beta as 2.0 since July 2026. The file-based app continues as "Logseq OG", maintained without new features [S; D docs/db-version.md].
- *Lesson:* never tear down incoming edges when a target disappears and reappears. Treat that as "unresolved", then reconcile. Rewriting reference *text* across many syntaxes always misses cases. The product itself ended up moving identity into a database.

**Dendron** (VS Code extension, maintenance mode since Feb 2023 [S]).
- *Mechanism:* M1. `Dendron: Rename Note` and hierarchy refactoring update links [D]. Frontmatter carries an `id` that must not change; publishing depends on it [D]. Identity for wiki-links is the dotted file name.
- *Repair:* M7, `Doctor` with `findBrokenLinks`, `fixFrontmatter`, `createMissingLinkedNotes`, and a `regenerateNoteId` that warns it breaks published links [D].
- *Lesson:* a stable ID in frontmatter does not help wiki-links that address by name. The link syntax has to use the ID, or the ID is only metadata.

**Foam** (VS Code).
- *Mechanism:* M1 via VS Code's rename events. "When you rename or move a note or folder, Foam automatically updates all wikilinks pointing to it". The setting is `foam.links.sync.enable`, default `true`, and for standard Markdown links Foam defers to VS Code's `markdown.updateLinksOnFileMove.enabled` [D, `docs/user/features/wikilinks.md` and `package.json`].
- *Reported failure:* folder rename with F2 in the Explorer did not sync links (#1143, Jan 2023) [C].
- *Lesson:* even inside the IDE, directory renames and batch moves are special cases that ship broken first. LSP sends only the folder, "not its children" [D], so consumers must expand it themselves.

**Zettlr.**
- *Mechanism:* M4/M5. Zettelkasten IDs in the file name or content, and links by ID survive title changes [C: this comes from a search snippet of the Zettlr manual; the manual page returned 404 when fetched on 2026-09-26].
- *Reported failure:* after renaming a file so the ID lives in the name, following an ID link failed and created a new file until restart or re-save (#1444, 2020, confirmed/pinned) [C].
- *Lesson:* an ID index is itself a cache that goes stale on rename. It needs refreshing on every observation.

**Org-mode / org-roam.**
- *Mechanism:* M4 + M3 + in-process M2.
  - A node is "any headline or top level file with an ID" [D].
  - `org-roam-db-autosync-mode` installs `find-file-hook`, a buffer-local `after-save-hook`, and advice on `rename-file`, `delete-file` and `vc-delete-file` [D, `org-roam-db.el`]. Only operations performed *through Emacs* are seen.
  - `org-roam-db-sync` re-parses files whose SHA-1 content hash changed, not by mtime [D].
  - org-id: when an ID is not found where recorded, `org-id-find` calls `org-id-update-id-locations`, which scans all agenda files, archives, open files and known files, and warns "Duplicate ID" [D, `org-id.el`].
- *Lesson:* the most robust pattern in this group is references that carry no path, a cache keyed by ID, and a rescan on miss. Its costs are rescans and duplicate IDs after a file is copied.

**Notion** (no files).
- *Mechanism:* M6 + identity-by-ID. Page URLs are `slug-<id>`. The server ignores the slug, looks up the ID and redirects to the current canonical slug [C, Adam Coster]. Moving a page *across workspaces* is implemented as duplication, and "some content or settings in the duplicated page may be broken… links, relations, permissions, page history" [D, Notion Help].
- *Lesson:* identity = ID, path = display attribute, redirect to canonical. A "move" across an identity boundary that is really a copy breaks everything. For moirai the boundaries are stores, and possibly git images and repositories.

### 4.2 IDEs and language servers

**IntelliJ Platform.**
- *Mechanism:* M1 with previews.
  - Move File has "Search for references: … find and update references to the file being moved" [D].
  - **Safe Delete**: "Before IntelliJ IDEA deletes a file or a symbol, it searches for usages and if they are found, IntelliJ IDEA lets you check them", with options "Search in comments and strings" and "Search for text occurrences" [D].
  - External changes arrive through a native file watcher and timestamp-based VFS refresh [D, IntelliJ SDK]. Nothing documents that outside moves become refactorings, and they are refreshed as file-system changes [I].
- *Lesson:* the best explicit delete shows the referrers first and lets the user decide. Rewriting text occurrences outside code is opt-in, because it can hit false positives.

**VS Code.**
- *Mechanism:* M1.
  - `typescript.updateImportsOnFileMove.enabled` defaults to **prompt**.
  - `markdown.updateLinksOnFileMove.enabled` defaults to **never**. It is stable since 1.73 (Oct 2022) and "detects renames of Markdown files, images, and directories" moved or renamed in the Explorer [D, `package.json` on main, 2026; docs].
  - Link validation: `markdown.validate.fileLinks.enabled` and related settings [D].
  - The event API: `onDidRenameFiles` "is triggered by user gestures, like renaming a file from the explorer, and from the workspace.applyEdit-api, but this event is *not* fired when files change on disk, e.g triggered by another application, or when using the workspace.fs-api". When renaming a folder with children, only one event fires [D, `vscode.d.ts`].
- *Reported failures:*
  - Moving several files at once prompted for only one of them (#105110, 2020, fixed in 2023) [C].
  - Markdown link updates not happening in some setups (#167857, not reproducible) [C].
  - Cursor users report that dragging files does not do `git mv` [C].
- *Lesson:* the default for rewriting links in prose is *off*, even at Microsoft. Batch and directory moves are where M1 implementations fail first.

**Language Server Protocol 3.16+ file operations.**
- `workspace/willRenameFiles`: sent "before files are actually renamed as long as the rename is triggered from within the client either by a user action or by applying a workspace edit". It returns a WorkspaceEdit, and "clients might drop results if computing the edit took too long" [D].
- `didRenameFiles` is the matching notification [D].
- `workspace/didChangeWatchedFiles` has **Created, Changed and Deleted only, with no rename type** [D].
- The spec discourages servers from running their own watchers ("getting file system watching on disk right is challenging… not for free") [D].
- *Lesson:* the protocol shape (pre-move hook returns edits; post-move notice) is a good model for an agent-facing `moirai file mv` and for Claude Code hooks. It covers only moves the client initiates.

**rust-analyzer.**
- *Mechanism:* M1 via `willRenameFiles`, which renames a module when its file is renamed.
- *Limit:* the handler is still "Limit to single-level moves for now": same parent directory only, skipping `mod.rs` [D, `handlers/request.rs`, last touched 2026-08-03]. The request to move modules across directories (#8872) has been **open since 2021-05-18** [M, GitHub API].
- *Lesson:* a cross-directory move in a module system is a semantic refactor, not a path rewrite. moirai should record the move and leave code rewriting to language tools.

### 4.3 Operating-system links

**Windows shortcuts, OLE links and Distributed Link Tracking.**
- *Mechanism:* M8 + M3 + a service (TrkWks, running and Automatic on the owner's machine [M]).
  - The docs start from the premise that "Storing a reference to a file or directory by using its path and file name is not reliable" [D].
  - An object ID is an optional per-file attribute. "Rename, backup, and restore operations preserve object IDs. However, copy operations do not" [D].
  - `IShellLink::Resolve` first asks DLT, which finds files moved within or across NTFS volumes. It then falls back to heuristics: the last directory "for an object with a different name but the same attributes and file creation time", then subdirectories nearby, then the desktop and local volumes. Last comes a UI prompt. `SLR_NO_UI` has a default timeout of 3,000 ms, and `SLR_UPDATE` writes the repaired path back into the `.lnk` [D].
  - The docs admit the heuristics "do not always yield positive results, and can be time consuming" [D].
- *Measured here:*
  - object IDs are lost on temp+rename saves and on Claude Code edits;
  - they are carried over by tunneling for ~15 s;
  - `OpenFileById` relocates a file in ~0.5 ms [M].
- *Lesson:* dual keys (path + ID), resolve on use, bounded heuristics, **write the repair back** and prompt only as the last resort. An OS-level ID survives moves, not rewrites. DLT's cross-volume tracking needs an always-on service, which moirai cannot have.

**macOS aliases and bookmarks.**
- *Mechanism:* M8 + M3. "A bookmark can usually be used to re-create a URL to a file even in cases where the file was moved or renamed." File reference URLs (inode-based) "are not safe to store and reuse between launches" [D, Apple File System Programming Guide].
- Resolution uses the stored path first, then the stored identity: volume, CNID and more. Resolving "will cause its saved paths to be updated if they have changed", and aliases "often don't accept substitute files simply on the basis of their path" [C, Eclectic Light tests]. The API returns `bookmarkDataIsStale` so the app re-saves the bookmark [D].
- *Lesson:* store several independent clues, verify identity on resolve (do not accept a same-path impostor blindly), and refresh the stored clues whenever resolution succeeds. That is the "stale" flag.

**USN change journal / Everything.** The journal is the precedent for "catch up on what happened while nobody watched", used by backup tools and indexers [D, MS Change Journals]. Reading it needs elevation [M, 3.4], so Everything ships a service [C]. It is not usable by an unprivileged, zero-daemon tool.

### 4.4 Reference managers

**Zotero linked files.**
- *Mechanism:* M3 + relative paths.
  - Linked attachments are stored as absolute paths, or relative to a "Linked Attachment Base Directory" so the same library works on several machines [D].
  - When a file was "moved or deleted outside of Zotero", the user presses **Locate** in the File Not Found dialog [D].
  - Staff describe that, with a base directory set, Zotero "will automatically offer to reassociate linked files" and relink all files it can find under that base, a bulk relink by shared prefix [C, forums, Aug 2025].
  - Plugins (Zutilo) do bulk path search-and-replace [C].
- *Lesson:* store paths relative to a root, never absolute. When one missing file is found under a new prefix, offer the same prefix rewrite for all its siblings. BoykoEngine's 131-file archive move is exactly this shape [M].

### 4.5 Large-file and data tools (pointer files)

- **git-annex.** Files are symlinks into `.git/annex/objects/<content-key>`. Moving a symlink to another depth breaks the relative link. `git annex fix` "fixes up symlinks that have become broken", which is "useful to run manually when you have been moving the symlinks around", and "is done automatically when committing" [D]. *Lesson:* content-addressed identity, plus a repair that runs at commit time (M7).
- **DVC.** A `.dvc` file records the data path and hash. `dvc move` exists because with a plain `mv` "DVC wouldn't know that we changed the path… as the old location is still found in the corresponding .dvc file". It updates the `.dvc` file and `.gitignore` [D]. While researching, the DVC docs URL itself 301-redirected from dvc.org to doc.dvc.org [M]: an M6 redirector in the wild. *Lesson:* the explicit command exists because the tool cannot see plain `mv`. Users still use plain `mv`.
- **Git LFS.** A pointer file holds `version`, `oid sha256:…` and `size` [D]. Paths are selected by `.gitattributes` filter patterns [D]. A file moved to a path no pattern covers would be committed as a normal blob [I]. *Lesson:* content identity keeps moves harmless to the pointer, but any *path-pattern rule* (like Claude Code rule globs, §4.9) silently stops applying after a move.

### 4.6 Version control

- **git** (inference, M3). Git stores no renames. `diff.renames` defaults to true. `-M` treats a delete/add pair as a rename above a 50% similarity index. Exhaustive detection is O(N²), and `diff.renameLimit` defaults to 1000 [D]. Linus's 2005 position was that files do not matter, only how content moved, and that history tracking belongs at *search time* rather than commit time [C, git list archive]. On the owner's repo, 150/159 renames are exact (R100) [M], so content matching alone would recover almost all of them.
- **Mercurial** (M1 + M3). `hg mv`/`hg cp` record copies explicitly. When users forget, `hg addremove --similarity` "compares every removed file with every added file and records those similar enough as renames". The default is 100, so only identical files match unless configured [D].
- **Jujutsu.** Its copy-tracking design doc critiques both models: git's inference is "hard to make… scale to very large repos", and Mercurial stores only the most recent copy. It proposes copy IDs in tree entries that record a file's past names. It is a design, not implemented [D].
- **SVN / TortoiseSVN** (M3, manual). An outside rename shows up as *missing* + *non-versioned*. "Repair move" pairs exactly two such files in the commit dialog [D].
- **Perforce** (M3, automatic). `p4 reconcile` compares missing and added files and converts pairs into `move/delete` + `move/add` when "sizes and contents are similar" [D].
- *Lesson:* every VCS ends up with explicit moves **plus** after-the-fact pairing of delete/add by content. The pairing is what actually catches real-world moves. moirai's move history can be recorded like hg (explicit, when known) and inferred like git/p4 (when not), and both paths end in the same `Move` record.

### 4.7 Build systems

- **Bazel / Buck.** Sources are listed explicitly in BUILD files or selected by `glob()`, which runs at load time and matches only files in the package [D]. A moved file breaks the build *loudly* (M7). `glob(allow_empty = False)` errors instead of silently matching nothing [D]. Gazelle regenerates and updates BUILD files from sources [D]. Buck2 shares the model [I].
- *Lesson:* explicit file lists are fine **if** a mismatch fails loudly and a generator repairs it cheaply. Silent emptiness is the worst outcome, and the owner's `.claude/rules` globs have exactly that property (§4.9).

### 4.8 Game engines: the most battle-tested asset-reference systems

- **Unity.**
  - *Mechanism:* M4 with sidecars. "The `.meta` files contain the unique ID assigned to the asset". Moving in the Project window moves the `.meta`, "however, if you move or rename an asset outside of Unity, you must move or rename the `.meta` file to match". Losing it means "any reference to that asset is broken" [D].
  - *Reported failures* (Forrest Smith, 2015): missing metas giving different GUIDs per teammate; copying in Explorer duplicating GUIDs, which is "catastrophic… Unity… will replace the one of the two with a newly generated value" [C], and references can retarget to the copy [C].
  - *Recommended defence:* version-control triggers or pre-commit checks (M7) [C].
- **Unreal Engine.**
  - *Mechanism:* M1 + M6. Moving or renaming in the editor "leaves a Redirector in the asset's old location" so that unloaded packages that still reference the old path resolve [D].
  - "Fix Up" re-saves all referencers and deletes the redirector [D].
  - *Failures:* re-creating an asset with the old name errors; "dangling redirectors" block deletes until fixed up [D].
- **Godot 4.4** (Jan 2025).
  - *Mechanism:* M4 sidecars + a path fallback. Because "file paths will break if files are moved", scripts and shaders get `.uid` sidecar files. Scenes store both UID and path, and the path still resolves (with warnings) if the UID is broken [D].
  - *Alternatives rejected:* UIDs as comments inside scripts ("testers found it to be quite disruptive"); a central database, because it would break external moves and create merge conflicts [D].
  - *Community feedback:* thousands of `.uid` files cluttering repositories; users of external IDEs such as Visual Studio forgetting to move them [C, discussion #11574]. Guidance for external editors was requested in #11565 [C].
- *Lesson (strong):* the engines agree. References carry an ID; the path is kept as a fallback or display; editor moves are handled; outside moves need the sidecar to travel; copies must be de-duplicated; a fix-up pass collapses redirectors. For moirai the "sidecar" can be moirai's own store, which removes the "forgot to move the `.meta`" failure entirely. The file carries nothing, so nothing can be left behind.

### 4.9 Agent trackers and agent tooling

| Tool | File-link mechanism | How it rots |
|---|---|---|
| **Beads** (gastownhall/beads, v1.3, 27.4k★ [M]) | No first-class file references. Since #1406 (Jan 2026) an optional `metadata` JSON field, with a convention `{"files":["foo.go","bar.go"]}` (explicit paths, no globs) [C] | Plain strings: nothing validates or updates them when files move [C]. `bd delete` rewrites text mentions of *issue* IDs to `[deleted:ID]` [D, via 03] |
| **Claude Code** | `@path` imports resolve relative to the importing file, max depth 4 [D]. `.claude/rules/*.md` with `paths:` globs load rules "only when Claude works with matching files" [D] | A glob that stops matching after a move fails silently. An invalid glob "matches nothing", and before v2.1.207 one invalid pattern broke `Read` for every file [D]. Auto-memory's directory is "derived from the git repository" [D] |
| **Cursor rules** | `globs` frontmatter ("auto-attached when a matching file is in context") and `@filename` references [D] | Same silent glob failure [I] |
| **mcp_agent_mail** | Advisory **file reservations** on glob patterns with TTL, plus a git pre-commit guard that blocks commits overlapping another agent's reservation [C/D README] | Globs keyed by path; a move escapes the reservation [I] |
| **The owner's own corpus** | `file:line` citations and wiki-links in memory and docs | "184 of 282 anchors dead" [C, 01]; ~229 rotted citations and an 8-pass repair campaign [C, 02]; 20 unresolved wiki-link occurrences [M, 02]; 188 verbatim references to moved-away paths in BoykoEngine [M, §3.5] |

*Lesson:* none of the agent tools maintains file references. The rot measured in the owner's corpus is the default outcome. moirai would be first in its class to treat file references as maintained edges.

### 4.10 Locations inside files

| Precedent | Selector stored | Relocation | Failure handling |
|---|---|---|---|
| **ctags / Universal Ctags** | a search pattern (`/^fn foo(/`) by default ("mixed"); line numbers only where patterns are ambiguous | re-search | patterns have "the advantage of not referencing obsolete line numbers when lines have been added or removed" [D] |
| **Emacs bookmarks** | file, position, `front-context-string` and `rear-context-string` (`bookmark-search-size` = 16 chars) | go to position, search forward for the front context, then back for the rear [D, `bookmark.el`] | a missing file raises `bookmark-error-no-filename`; the user runs `bookmark-relocate` [D] |
| **Hypothesis / W3C Web Annotation** | RangeSelector (DOM) + TextPositionSelector + TextQuoteSelector (exact + prefix + suffix) | range → position → context-first fuzzy → quote-only fuzzy [D, 2013 post] | orphans; reported bug: annotations that "fail to anchor, yet not be reported as orphans" (#954) [C] |
| **Swimm** | code snippets, "smart tokens", "smart paths" | CI checks against full git history, weighing "line markers, line numbers, token references, size of the change, history of the file"; auto-sync or fail and "leave a task for someone… to reselect the snippet"; path changes followed "provided that you use Git" [C, vendor] | needs full history, so shallow clones do not work [C] |
| **Doorstop** (requirements as YAML files) | `references: [{path, type, keyword?, sha}]`; item fingerprints; link fingerprints | search the project for the file name, or search contents for a keyword [D] | a changed fingerprint makes the link **suspect**; `doorstop review` / `clear` [D] |
| **OpenFastTrace** | tags in code comments such as `[impl -> dsn~name~1]`; the ID syntax is `type~name~revision` | a scan rebuilds the trace, so file paths are irrelevant [D] | "Incrementing the revision breaks all incoming links", on purpose, to force re-verification [D] |
| **GitHub permalinks** | commit SHA + path + line | none; pinned forever | branch URLs drift: "the file contents might not be the same when someone looks at it later" [D] |

*Lesson:* the durable combination is (symbol or heading if the language has one) + (quote with context) + (line as a hint only) + (content hash of the region) + (the commit where it was verified). Relocate lazily, and give **explicit states**: ok / relocated / fuzzy / orphaned. Content change without relocation should flag *suspect*, as Doorstop does, not be silently accepted.

### 4.11 Embedding stable IDs inside files: what it costs

| Cost | Evidence |
|---|---|
| Clutter and review noise | Godot testers: embedding UIDs as comments was "quite disruptive" [D]; `.uid` sidecars "cluttering my repository" [C]; Logseq users ask for "more markdown friendly" block references than `id::` lines [C] |
| Copies duplicate IDs | org-id "Duplicate ID" warnings [D]; Unity duplicate GUIDs are "catastrophic" and can retarget references [C] |
| Sidecars must travel with the file | Unity [D], Godot [D]; DVC's `.dvc` files [D] |
| Files that cannot carry IDs | binaries, generated files (`build.rs` output, shaders compiled into assets), vendored/third-party code, lock files [I]; Godot needed sidecars precisely because scripts are plain text with no metadata slot [D] |
| Tool and agent rewrites | an agent that rewrites a whole file may drop or duplicate markers [I]; Claude Code's Write/Edit already replace the file object [M] |
| Reload semantics | Logseq loses incoming references when a file with IDs is reloaded (#7362) [C] |
| Benefit | references survive *any* move by any tool, and a scan rebuilds the index with no state (org-id, OpenFastTrace) [D] |

*Lesson:* do not require IDs in code files. Where the owner wants maximum robustness for prose (for example plan and spec Markdown), an **optional** marker is defensible, as Obsidian `^id` and Logseq `id::` add them lazily on first reference. It must be opt-in because it modifies repository files.

---

## 5. Lessons table

| # | Lesson | Precedent evidence | Consequence for moirai R4 |
|---|---|---|---|
| L1 | In-tool moves (M1) are precise but are bypassed by every outside actor | VS Code/LSP API text [D]; Obsidian [D]; DVC [D]; Unity/Godot [D]; owner: 131-file move inside an unrelated commit [M] | Ship `moirai file mv/rm`, but never rely on agents calling them. A lazy net (M3) is mandatory |
| L2 | Outside moves arrive as delete + create; the pairing must be reconstructed | LSP has no Renamed event [D]; Obsidian [D]; SVN missing/unversioned [D]; `p4 reconcile` [D]; `hg addremove -s` [D] | A reconcile pass that pairs *missing* references with *new* files |
| L3 | Watchers only work while running, and are excluded by the idle rule | External Rename Handler [D]; org-roam hooks [D]; `ReadDirectoryChangesW` overflow [08] | No watcher. Reconcile at command time, in fail-open hooks and at gates |
| L4 | Identity ≠ path; keep the path as a mutable attribute plus a fallback | Unity GUID, Godot UID (path fallback) [D]; Notion ID + slug [C]; org `:ID:` [D] | File node with a moirai uid; `path` is a versioned field; references are edges to the node |
| L5 | Sidecar or in-file IDs fail when copied or not moved along | Unity/Godot [D][C]; org-id duplicates [D] | Keep the ID in moirai's store. Files carry nothing by default |
| L6 | OS file identity survives moves, not rewrites | [M] §3.1; ReplaceFile docs [D]; Apple: file reference URLs not persistent [D] | File ID only as a machine-local, per-worktree **hint**; never exported (R3), never versioned (R1) |
| L7 | Content hashing catches most real moves | git R100 150/159 on the owner's repo [M]; `p4 reconcile` [D]; git-annex/LFS/DVC [D] | Store a content hash per file node; exact-hash pairing is the high-confidence step |
| L8 | Directory moves dominate and deserve a prefix rule | 154/159 cross-directory, 131 in one batch [M]; Zotero relink by base path [C]; LSP sends only the folder [D]; Foam #1143 [C] | Directory-prefix inference and `moirai file mv <dir>` expanding to children, atomically |
| L9 | Name-based resolution silently retargets or goes ambiguous | Obsidian "global" names [C]; Unity duplicate GUID retarget [C]; 16% non-unique basenames [M] | Basename is a *weak* clue; ambiguity is always surfaced, never auto-resolved |
| L10 | Tearing down edges when a target disappears loses data | Logseq #7362 [C]; Beads orphan rows / split brain [03] | A missing target → `missing` state + flagged edges; never delete the edges; reconcile later |
| L11 | Repairs should be written back, with a stale flag | IShellLink `SLR_UPDATE` [D]; bookmarks `isStale` [D]; Unreal Fix Up [D] | A successful relocation is a normal moirai commit (auditable, branchable); record `verified_at` |
| L12 | Keep forwarding records for things you cannot rewrite | Unreal redirectors [D]; HTTP 301 [M]; Notion redirect [C] | `aliases` (old paths) on the file node resolve path mentions in bodies, briefs and old git images |
| L13 | Safe delete lists referrers first | IntelliJ Safe Delete [D]; Unreal can't delete through dangling redirectors [D] | `moirai file rm` = dry-run impact report + per-edge policy (synthesis T5); referrers flagged in the same commit |
| L14 | Rewriting reference *text* has a long tail of misses and false positives | Logseq alias/tag/property misses [C]; IntelliJ text-occurrence options [D]; VS Code Markdown default "never" [D] | Rewriting repository files is opt-in with preview; by default moirai *reports* textual mentions |
| L15 | Loud failure beats silent emptiness | Bazel `allow_empty=False` [D]; Claude Code invalid glob "matches nothing" [D]; Hypothesis silent non-orphans [C] | Every unresolved reference is listed (brief, `check`, gate); glob-type references report "0 matches" as an error |
| L16 | Gates catch what the net missed | git-annex pre-commit fix [D]; Unity triggers [C]; mcp_agent_mail pre-commit guard [C]; Swimm CI [C] | `moirai file check --strict` in `merge-check` and an optional git pre-commit hook |
| L17 | Inside-file anchors need selectors that tolerate edits, plus an orphan state | ctags patterns [D]; Emacs contexts [D]; Hypothesis [D]; Swimm [C] | Anchor = {symbol? heading? quote+prefix+suffix, line hint, region hash, verified_at}; lazy re-anchoring |
| L18 | A content change is a trust event, not only a location event | Doorstop suspect links [D]; OpenFastTrace revisions [D]; owner's private notes: measured values going stale although nothing was edited [02] | When a referenced file's hash changes, dependent findings and measurements become `suspect` (reuses the synthesis `suspect` machinery) |
| L19 | Explicit and inferred moves should produce the same record | hg `mv` vs `addremove -s` [D]; git inference [D]; jj copy-ID design [D] | One `Move{from,to,how: explicit\|file-id\|content\|prefix\|similar\|manual}` op, mergeable like any field change |
| L20 | Relative paths survive machine and worktree changes | Zotero base directory [D]; Claude Code imports relative to file [D] | Store paths relative to the repository root; resolve per worktree (44 worktrees) at read time |

---

## 6. Which mechanisms combine well for moirai

### 6.1 Fit of each mechanism against moirai's constraints

| Mechanism | Zero idle CPU | Windows, non-admin | R1 branches | R2 no git in core | R3 git image | Catches agent/shell moves | Verdict |
|---|---|---|---|---|---|---|---|
| M1 explicit commands | ✔ | ✔ | ✔ (a move is a commit) | ✔ | ✔ | ✘ only if the agent uses them | **use**, as the precise path |
| M2 watcher | ✘ | lossy [08] | — | ✔ | — | ✔ while running | **reject** |
| M3 lazy reconcile | ✔ | ✔ | ✔ (per branch or worktree) | ✔ (own hashing) | ✔ | ✔ at next check | **use**, as the net |
| M4 IDs in files | ✔ | ✔ | ✔ | ✔ | ✔ | ✔ | **optional**, prose only, owner opt-in |
| M5 name/content resolution | ✔ | ✔ | ✔ | ✔ | ✔ | ✔ | **use content**; basename only as a weak clue |
| M6 redirect aliases | ✔ | ✔ | ✔ | ✔ | ✔ (exported) | — | **use** |
| M7 gates | ✔ | ✔ | ✔ | hook optional | ✔ | ✔ at gate | **use** |
| M8 OS file ID | ✔ | ✔ `OpenFileById` [M] | runtime-only | ✔ | never exported | ✔ unless rewritten | **use as a hint** |
| M8 NTFS object ID | ✔ | ✔ | runtime-only | ✔ | never | lost on agent edits [M] | **skip** (writes NTFS metadata to repo files; timing-dependent) |
| USN journal | ✔ | ✘ admin [M] | — | — | — | ✔ | **reject** |

### 6.2 The recommended combination (precedent-derived, not a specification)

**A. Data shape (L4, L5, L6, L12, L20).**
- A **file node** (the synthesis's `artifact` kind [30 §2.6] fits) is the only target of file references. It has:
  - identity = moirai uid / `#N`;
  - versioned fields: `path` (repository-relative), `aliases` (add-wins set of former paths), `content_hash` (BLAKE3), `size`, `state` ∈ {present, changed, missing, ambiguous, deleted};
  - `verified_at` (moirai commit + the git HEAD it was checked against).
- **Store-level runtime hints**, like `gitmap` in [30 §2.16]: per (worktree root, path) the last-seen NTFS file ID, mtime and size. They are machine-local, never versioned, never exported to the git image.
- **Anchors** for locations inside files are sub-records of the reference edge:
  - `symbol` (e.g. a Rust path) or `heading` slug;
  - `quote` + 16–32 characters of prefix and suffix context;
  - `line_hint`;
  - `region_hash`;
  - `anchor_state` ∈ {ok, relocated, fuzzy, orphaned}.

**B. Explicit commands (M1; L1, L8, L13, L19).** One commit each, synchronous for every referrer, which is how the "node 40" guarantee applies to files:
- `moirai file mv <from> <to> [--after] [--dry-run]`. It does the file-system rename itself, or only records a move already made (`--after`, like `hg mv --after` / Repair move). Directories expand to all file nodes under the prefix. Batches are atomic. It never needs git: git infers renames itself [D].
- `moirai file rm <path> [--after] [--dry-run] [--replaced-by <path>]`: safe delete. Lists referrers, applies per-edge policies (restrict, re-point or flag) as in synthesis T5, and tombstones the file node.
- `moirai file add|relink|where|mentions`:
  - `add` registers a file (it also happens implicitly on first reference);
  - `relink` is a manual repair, like Zotero's Locate or TortoiseSVN's Repair move;
  - `where` resolves a node or alias;
  - `mentions` lists textual occurrences of a path in repository files and only reports them (L14).

**C. Lazy reconcile cascade (M3 + M8 hint + M5 + M6; L2, L3, L6–L11).** It runs inside commands and never in the background:

| Step | Test | Confidence | Precedent |
|---|---|---|---|
| 0 | `path` exists in the caller's worktree; size/mtime same → ok; hash differs → `changed` → re-anchor + `suspect` on dependants | — | org-roam hash sync, Doorstop |
| 1 | path missing → `OpenFileById(last file ID)` in the same worktree → new path inside the root | high (OS continuity) | DLT, bookmarks, [M] 0.5 ms |
| 2 | exact content hash equals exactly one *new or unknown* path (candidates = size-matched files from one enumeration) | high | git R100, `p4 reconcile`, git-annex |
| 3 | several missing files share old dir D and were found under D′ → propose D→D′ for the rest | high if ≥ 2 corroborate | Zotero base-path relink |
| 4 | unique basename + similarity ≥ threshold (git's 50% default) | medium → confirm unless the owner opts in | git `-M`, `hg addremove -s` |
| 5 | nothing, or several candidates → `missing` / `ambiguous`; edges kept and flagged; listed in brief/check | — | Logseq #7362 (anti-pattern), Hypothesis orphans |

Applied relocations are ordinary moirai commits carrying `how` and the evidence. They are auditable, revertible, and merge like any `path` edit (L11, L19).

*When it runs.*
- **Per command**, only for references the command renders (brief, pack, get): ~25 µs per existence check [M, upper bound].
- `moirai file check`: all file nodes, with a directory-mtime pre-filter [M 3.6].
- **Fail-open hooks:**
  - Claude Code SessionStart and SubagentStop;
  - optional PostToolUse on Bash/Write/Edit (the Write/Edit hooks know the path, and the file ID changes on those edits [M], so they also refresh hints);
  - optional git `post-checkout`/`post-merge`/`post-rewrite` hooks that pass git's own `--name-status -M` output as a hint feed. That keeps R2, because git stays out of the core.
- **Gates:** `merge-check` and an optional pre-commit hook: `check --strict` fails on missing, ambiguous or orphaned references that are not acknowledged (L15, L16).

**D. Branch and worktree semantics (R1).** `path`, `aliases`, `state` and anchors are versioned fields, so a move made on a lane merges with the lane.
- Both sides renamed the same file differently → a `FieldEdit` conflict value, git's rename/rename.
- Delete vs a new reference → `DanglingEdge` staged.
- Resolution always runs against the worktree bound to the caller's branch [30 §2.14].

A file that is absent because the checkout is on another git commit is *not* a move: re-verify when the bound worktree's HEAD differs from `verified_at` before declaring `missing`. This is the branch-switch split-brain lesson from Beads [03].

**E. What to avoid.**
- File watchers and resident indexers (L3).
- Treating a file ID or object ID as identity (L6).
- The USN journal (needs admin).
- Creating NTFS object IDs on repository files (a metadata write, lost on agent edits, timing-dependent under tunneling) [M].
- Silent retargeting by basename (L9).
- Deleting edges on a miss (L10).
- Rewriting repository text by default (L14).
- Globs as the only binding for rules or reservations (L15).

### 6.3 Guarantee to promise the owner

- Moves and deletes done **through moirai** are synchronous: one commit, and every referrer sees them on its next read.
- Moves done **by anything else** are repaired at the next moirai contact with the reference (render, check, hook, gate). Repair is deterministic and high-confidence for exact-content and file-ID matches. Anything else is reported, never guessed.

No zero-idle-CPU precedent does better. DLT gets closer only by running a service [D].

---

## 7. Risks and anti-patterns seen in precedents

- **Heuristic creep:** DLT's search "can be time consuming" [D]. Keep each cascade step bounded (one enumeration per check, hashing only size-matched candidates) and report budget exhaustion as `unverified`.
- **Two sources of truth:** Beads SQLite vs JSONL [03]. Paths live only in moirai. Any Markdown or `CLAUDE.md` rendering of references is a one-way export.
- **Accumulating redirectors:** Unreal [D]. Aliases need a `fixup` report ("N aliases still mentioned in K files") so they can be retired deliberately.
- **Default-on rewriting of user files:** VS Code keeps Markdown link rewriting at "never" [D]. moirai should match that stance.
- **Ambiguity introduced later:** a new file with an old or alias name. Unreal errors on name reuse [D]; Obsidian silently retargets [C]. moirai: an alias never captures a *new* file; aliases resolve only to their node.

---

## 8. Open questions (owner value and scope calls)

1. **May moirai modify repository files?**
   - (a) Rewrite textual path mentions after a move, like the 188 dangling ones in BoykoEngine.
   - (b) Inject optional ID markers into Markdown plans and docs.
   - Proposed default: no. Report only; opt-in with preview.
2. **Should `moirai file mv/rm` perform the file-system operation, or only record operations agents already did with their own tools (`--after`)?** Both are cheap. The question is which one skills teach as the norm.
3. **Scope of referenceable files:**
   - only files tracked in the project repository or repositories;
   - also untracked or ignored files;
   - also session scratchpads and absolute paths outside any repository (whose rot profile is much worse [02]).
4. **Auto-rebind policy:**
   - may exact-content and file-ID matches rebind silently (still recorded as commits)?
   - may directory-prefix and similarity matches rebind silently, or must they be confirmed?
   - must unresolved references block `merge-check`?
5. **Inside-file locations:** is a stored line number ever acceptable as identity, or only as a hint under symbol, heading or quote anchors? Should a content change of a referenced file mark dependent findings and measurements `suspect` automatically?
6. **Hook budget:** is a PostToolUse hook on every agent Bash/Write/Edit call acceptable (+15–73 ms spawn each [30]), along with git hooks? Or should reconciliation run only at SessionStart, SubagentStop, `merge-check` and on explicit command?
7. **Migration:** should the existing dangling references (188 in BoykoEngine, the anchor rot in memory) be imported and repaired once when moirai is adopted, or left as history?

---

## 9. Sources

Internal: [01] `docs/research/01-boyko-workflow-roles.md`; [02] `docs/research/02-boyko-workflow-orchestration.md`; [03] `docs/research/03-landscape-agent-memory-and-trackers.md`; [08] `docs/research/08-concurrency-sync-git-interop.md`; [30] `docs/research/design/30-synthesis.md`.

**Obsidian:**
- https://obsidian.md/help/links
- https://forum.obsidian.md/t/auto-updated-links-rename-from-file-system/25682
- https://forum.obsidian.md/t/auto-update-links-for-files-moved-while-obsidian-is-closed/91670
- https://forum.obsidian.md/t/scalable-alternative-for-shortest-path-when-possible/31958
- https://forum.obsidian.md/t/plugin-external-rename-handler/93826
- https://github.com/mnaoumov/obsidian-external-rename-handler (src/path-ino-map.ts, src/desktop-external-rename-handler-component.ts)
- https://forum.obsidian.md/t/links-to-nested-heading-not-updating-when-heading-is-renamed/101505

**Logseq:**
- https://github.com/logseq/logseq/issues/7362
- https://github.com/logseq/logseq/issues/4297
- https://github.com/logseq/logseq/issues/9202
- https://github.com/logseq/logseq/issues/4356
- https://github.com/logseq/logseq/issues/1489
- https://github.com/logseq/logseq/issues/9129
- https://github.com/muellerei/logseq-cli/issues/95
- https://github.com/logseq/docs/blob/master/db-version.md
- https://kompozy.io/news/logseq-2-0-db-version-beta

**Dendron:**
- https://wiki.dendron.so/notes/g0iqmyiyxje6ndjmecshb8b/
- https://wiki.dendron.so/notes/ZeC74FYVECsf9bpyngVMU/
- https://wiki.dendron.so/notes/ffec2853-c0e0-4165-a368-339db12c8e4b/
- https://randomgeekery.org/post/2023/02/dendron-is-officially-in-maintenance-mode/

**Foam:**
- https://github.com/foambubble/foam/blob/main/docs/user/features/wikilinks.md
- https://github.com/foambubble/foam/blob/main/packages/foam-vscode/package.json
- https://github.com/foambubble/foam/issues/1143

**Zettlr:**
- https://github.com/Zettlr/Zettlr/issues/1444

**Org-roam / org-id:**
- https://www.orgroam.com/manual.html
- https://github.com/org-roam/org-roam/blob/main/org-roam-db.el
- https://github.com/emacs-mirror/emacs/blob/master/lisp/org/org-id.el

**Notion:**
- https://www.notion.com/help/transfer-content-to-another-account
- https://adamcoster.com/blog/notion-changeable-links

**IntelliJ:**
- https://www.jetbrains.com/help/idea/move-file-dialog.html
- https://www.jetbrains.com/help/idea/safe-delete.html
- https://plugins.jetbrains.com/docs/intellij/virtual-file-system.html

**VS Code:**
- https://code.visualstudio.com/docs/languages/markdown
- https://code.visualstudio.com/updates/v1_73
- https://github.com/microsoft/vscode/blob/main/src/vscode-dts/vscode.d.ts
- https://github.com/microsoft/vscode/blob/main/extensions/typescript-language-features/package.json
- https://github.com/microsoft/vscode/blob/main/extensions/markdown-language-features/package.json
- https://github.com/microsoft/vscode/issues/105110
- https://github.com/microsoft/vscode/issues/167857
- https://forum.cursor.com/t/drag-to-move-file-folder-doesnt-trigger-git-mv/43678

**LSP:**
- https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/
- https://github.com/microsoft/language-server-protocol/blob/gh-pages/_specifications/lsp/3.17/workspace/willRenameFiles.md
- https://github.com/microsoft/language-server-protocol/blob/gh-pages/_specifications/lsp/3.17/workspace/didChangeWatchedFiles.md

**rust-analyzer:**
- https://github.com/rust-lang/rust-analyzer/blob/master/crates/rust-analyzer/src/handlers/request.rs
- https://github.com/rust-lang/rust-analyzer/pull/7009
- https://github.com/rust-lang/rust-analyzer/issues/8872

**Windows:**
- https://learn.microsoft.com/en-us/windows/win32/fileio/distributed-link-tracking-and-object-identifiers
- https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-ishelllinkw-resolve
- https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-replacefilew
- https://learn.microsoft.com/en-us/windows/win32/fileio/change-journals
- https://www.voidtools.com/support/everything/everything_service/
- https://www.windowslatest.com/2026/09/21/microsoft-engineer-explains-why-windows-can-give-a-brand-new-file-the-creation-date-of-a-file-you-already-deleted/
- https://www.betaarchive.com/wiki/index.php/Microsoft_KB_Archive/172190

**macOS:**
- https://developer.apple.com/library/archive/documentation/FileManagement/Conceptual/FileSystemProgrammingGuide/AccessingFilesandDirectories/AccessingFilesandDirectories.html
- https://eclecticlight.co/2019/01/11/aliases-and-bookmarks-are-smarter-than-you-think/

**Zotero:**
- https://www.zotero.org/support/kb/missing_linked_file
- https://forums.zotero.org/discussion/126137/fixing-batches-of-broken-attachment-links-identifying-broken-paths-across-the-system

**Data and large-file tools:**
- https://git-annex.branchable.com/git-annex-fix/
- https://doc.dvc.org/command-reference/move
- https://github.com/git-lfs/git-lfs/blob/main/docs/spec.md

**Version control:**
- https://git-scm.com/docs/git-diff
- https://public-inbox.org/git/Pine.LNX.4.58.0504141102430.7211@ppc970.osdl.org/
- https://gist.github.com/borekb/3a548596ffd27ad6d948854751756a08
- https://www.mercurial-scm.org/help/commands/addremove
- https://docs.jj-vcs.dev/latest/design/copy-tracking/
- https://tortoisesvn.net/repairmoves.html
- https://tortoisesvn.net/docs/release/TortoiseSVN_en/tsvn-dug-rename.html
- https://help.perforce.com/helix-core/server-apps/cmdref/current/content/CmdRef/p4_reconcile.html

**Build systems:**
- https://bazel.build/reference/be/functions#glob
- https://github.com/bazel-contrib/bazel-gazelle

**Game engines:**
- https://docs.unity3d.com/Manual/AssetMetadata.html
- https://www.forrestthewoods.com/blog/managing_meta_files_in_unity/
- https://dev.epicgames.com/documentation/en-us/unreal-engine/asset-redirectors-in-unreal-engine
- https://godotengine.org/article/uid-changes-coming-to-godot-4-4/
- https://github.com/godotengine/godot-proposals/discussions/11574
- https://github.com/godotengine/godot-proposals/issues/11565

**Agent tools:**
- https://github.com/gastownhall/beads/issues/1406
- https://code.claude.com/docs/en/memory
- https://cursor.com/docs/context/rules
- https://github.com/Dicklesworthstone/mcp_agent_mail

**Locations inside files:**
- https://docs.ctags.io/en/latest/man/ctags.1.html
- https://github.com/emacs-mirror/emacs/blob/master/lisp/bookmark.el
- https://web.hypothes.is/blog/fuzzy-anchoring/
- https://github.com/hypothesis/product-backlog/issues/954
- https://swimm.io/blog/how-does-swimm-s-auto-sync-feature-work
- https://github.com/doorstop-dev/doorstop/blob/develop/docs/reference/item.md
- https://github.com/itsallcode/openfasttrace (.agents/skills/openfasttrace/SKILL.md)
- https://docs.github.com/en/repositories/working-with-files/using-files/getting-permanent-links-to-files
