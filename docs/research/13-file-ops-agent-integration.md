# 13 — File links in the agent harness, versioning and worktrees (R4, lens: agent integration)

*moirai research, 2026-09-26. Status: research and design only. Nothing is implemented, and this report is the only file written in the repository. Lens: how moirai's references to project files survive rename, move and delete when the movers are agents, the owner, editors and git, across ~44 worktrees and moirai's own branches (R1), without git (R2), and through the git image (R3). Companion reports written the same day: [10] content-based move detection (fingerprints, similarity, in-file anchors) and [12] precedents. This report uses their conclusions and does not repeat them. The probe scripts are not published. Large temporary files were deleted.*

**Evidence tags.** **[M]** measured here, on the owner's machine, in this session. **[D]** documented in primary docs, specs or source. **[C]** claimed by a third party. **[I]** my inference or design proposal. References such as `[30 §5a.7]` point into `docs/research/design/30-synthesis.md`, and `[07 §4.2]` into report 07.

**Timing caveat.** Every timing probe ran while the machine was at **100% CPU** (WMI and `\Processor(_Total)` sampled 3× at 100%; 16 `claude` processes were resident). `git --version` took **2.1 s** here, against 74 ms idle in [08 §2]. Treat every absolute time below as a pessimistic upper bound. Counts and behaviours do not depend on load.

---

## 0. Answer first

**Recommendation: a hybrid (§8, mechanism H).** It has four parts:

1. **Explicit file verbs record intent.** `moirai file mv|rm|add|new|relink` change the filesystem and the graph together. A durable intent record is written first, then the rename, then the graph commit. If the process crashes in between, recovery decides what happened by looking at the filesystem (§3).
2. **Lazy reconciliation is the safety net and catches everything else.** At SessionStart, pack, show and merge time, moirai stats only the linked paths, and only against the viewer's own worktree. It re-binds automatically only on exact evidence and shows every other case as a status (§4.1). This is what catches moves made by agents' raw `mv`, by Python scripts, by git checkout/merge/apply/stash and by the owner in Explorer. It needs no daemon and no watcher.
3. **Two optional accelerators, both off by default, never correctness-bearing:**
   - a Claude Code `PostToolUse` hook filtered to `Bash(mv *)`, `Bash(rm *)` and PowerShell `Move-Item`/`Rename-Item`/`Remove-Item`, running `async` (§4.2);
   - owner-installed git `post-commit`, `post-checkout` and `post-merge` blocks, chained after graphify's existing hooks (§4.3).
4. **Links are versioned; resolution is not.** The file node (uid, root-relative `path`, content `oid`, anchors on edges) branches and merges like any other node. A re-bind is an ordinary op on the branch bound to the tree where it was seen. The resolution cache and the "pending" observations from unbound worktrees are runtime tables, like leases, and are never exported. Path conflicts at merge are resolved *by observation* in the merged worktree (§5, §6).

**Why not explicit-only.** Agents do not use special move verbs today [M]:

- 121,571 shell calls in the owner's transcripts contain **148 raw `mv`** against **7 `git mv`**, plus **63 Python renames** (`os.rename`, `shutil.move`, …).
- 57% of those `mv` calls have `$VAR` or glob arguments, 21% run inside loops, `xargs` or `-exec`, and 64% use only relative paths.
- Git operations that rewrite the tree without any move verb are frequent: `git apply` ×189, `git merge` ×85, `worktree add` ×88, `checkout`/`switch`/`restore` ×116, `stash` ×43, `reset --hard` ×9.

Explicit verbs are necessary for intent ("deleted on purpose, replaced by X") but can never be sufficient. This matches [12 §0.1].

**Why not auto-only.** Some things only an explicit command can capture:

- intent: delete versus lost, and moves into ignored or outside paths;
- edited-and-moved files beyond ~50% churn [10 §0];
- exact glob rewrites for directory moves.

Some things auto-detection gets wrong:

- **42% of agent `mv` calls move a file to a temp, scratch or `target`-like path and usually back again** [M]. An eager auto-rebinder would thrash on these. A lazy one never sees them.

**Why not command parsing or watchers as truth.**

- **Command parsing** is too weak to be truth, for the reasons above. It is also tied to the filter rules: per the hooks docs, multi-word hook `if` patterns like `Bash(git mv *)` still fire on any command with `$()`, backticks or `$VAR` [D], and that is **37.6% of the owner's Bash calls** [M].
- **Watchers** are ruled out on four counts:
  - the zero-idle-CPU rule;
  - `ReadDirectoryChangesW` loses events [08 W10];
  - on NTFS a directory handle held open on a subdirectory makes renaming any **ancestor** fail with error 5 [M, §1.4];
  - the USN journal needs admin rights [12 §0.4].

**Headline measurements [M]:**

| Topic | Result |
|---|---|
| Owner's git history, all refs | 201 renames in 1,886 commits; 186 are 100%-similar pure moves; one commit moved 131 docs into `docs/archive/` |
| Memory corpus | 301 distinct repo paths; 58 are dead today but re-bind **uniquely** through git's rename history |
| Docs anchors | 10,790 `path:line` anchors across 611 files; only 36 point to missing files (line validity is [10]'s subject) |
| Worktrees | 44 in total; 39 are ancestors of trunk, and 36 of those lag it by 272–1,036 commits; tree sizes range from 1,590 to 15,873 files. "Absent in this worktree" is the *normal* state for most links in most trees, not "broken" |
| Windows move failures | A `MoveFileExW` of a file fails with 32 while any process holds it open without `FILE_SHARE_DELETE` or maps it. A directory fails with 5/32 while any file inside is open, a process has its cwd inside, or a watch handle sits on a subdirectory |
| Move cost | 4–10 ms p50 per file rename, up to 156 ms max under load; renaming a directory holding 1,000 files takes 9.5 ms. Batch moves must rename directories, not files |
| Claude Code `Edit`/`Write` | Replace the file (new NTFS file id) [M], so file ids cannot be identity (as [10] and [12] also found) |
| `core.autocrlf=true` in BoykoEngine | Working-tree bytes are CRLF while the index blob is LF, so a raw-byte hash ≠ git blob id. Fingerprints must be EOL-normalised [10] |
| `bashEditDiff` (Claude Code ≥ 2.1.269; owner on 2.1.281) | In the owner's 699 recorded diffs, **99.1% are flagged `shared`** (parallel agents in one repo) and none carries a created/deleted flag. Not usable as move evidence in this workflow |
| Git hooks | `git merge --no-commit` (42 of the owner's 85 merges), `git apply`, `stash`, `reset --hard`, `cherry-pick` and `git mv` fire **no** checkout/merge hook. Only `post-commit`/`reference-transaction` or `post-index-change` see them. Git hooks can only be accelerators |

---

## 1. Measured facts about how files move in the owner's workflow

### 1.1 Transcript census: how agents move files [M]

Scope: every `*.jsonl` transcript of the owner's BoykoEngine Claude Code sessions (`~/.claude/projects/<project>/`): 4,044 files, 2.99 GB. Tool calls:

| Tool | Calls |
|---|---|
| `Bash` | 120,448 |
| `Read` | 29,248 |
| `Edit` | 19,519 |
| `Grep` | 17,331 |
| `Write` | 4,235 |
| `Glob` | 1,264 |
| `PowerShell` | 1,123 |

There is **no `MultiEdit`** tool in 2026 (§1.6). Commands were split into simple segments on newlines, `;`, `&&`, `||` and `|`, with heredoc bodies removed. Script: `scan_moves2.py`.

| Move form (first word of a segment) | Count | `$VAR`/glob args | in loop/`xargs`/`-exec` | only relative args | >2 args | temp/scratch/`target`-like path |
|---|---|---|---|---|---|---|
| `mv` | 148 | 84 (57%) | 31 (21%) | 94 (64%) | 23 | 62 (42%) |
| `git mv` | 7 | 3 | 3 | 7 | 0 | 0 |
| `Move-Item` / `Rename-Item` / `move` / `ren` / `robocopy /MOV` | ≈0 (two regex false positives) | — | — | — | — | — |
| Python `os.rename`/`os.replace`/`shutil.move`/`.rename(` | 63 | n/a | n/a | n/a | n/a | n/a |
| **Shell calls containing any move** | **185 of 121,571 (1.52 per 1,000)** | | | | | |

Sampled forms (calibration only; the text stays local):

- `mv <lanes-dir>/<lane>/crates/…/x.rs $S/x.moved-out.rs`, and later the reverse move;
- `mv tests/foo.rs "$SNAP/foo.rs"`;
- `git mv "crates/…/seam_compile_fail/$f.rs" "crates/…/seam_pass/$f.rs"` inside a loop;
- multi-source `mv a b c quarantine/`.

The recurring pattern is **quarantine and restore**: move a test or fixture aside, run something, move it back.

Tree-changing git operations (segment-level, `scan_gitops.py`):

| Op | Count | Op | Count |
|---|---|---|---|
| `git apply` | 189 | `git stash` push/save | 21 |
| `git worktree add` / `remove` | 88 / 30 | `git stash pop/apply` | 22 |
| `git merge` (not `merge-base`) | 85, of which **42 `--no-commit`** | `git reset --hard` / other | 9 / 7 |
| `git switch` | 45 | `git cherry-pick` / `rebase` | 3 / 0 |
| `git checkout <ref>` / `checkout -- <paths>` / `restore` | 27 / 40 / 4 | `git rm` / `git mv` | 9 / 7 |
| `git commit` / `git add` | 984 / 995 | | |

Deletions: `rm` appears in 957 shell calls (7.9 per 1,000) and `Remove-Item` in 33. These are first-pass regex counts over whole commands, many of them on scratch paths.

Substitutions: **37.6%** of Bash commands contain `$()`, backticks or `$VAR` (45,313 of 120,560). `$VAR` alone appears in 34.4% and `$()`/backticks in 10.3%. Script: `scan_subst.py`.

**[I] What this means.**

- Agents move files with generic tools, often with computed arguments, and often only temporarily.
- The git operations that move files (apply, merge, checkout, stash) outnumber explicit moves about 3:1.
- A design that depends on agents calling a special verb, or on parsing command text, will miss most moves.
- A design that re-binds eagerly on every observed disappearance will thrash on quarantine-and-restore.

### 1.2 How files moved in git history, and what rotted [M]

Scope: `git log --all -M --diff-filter=R`, read-only, over all 1,886 commits since 2025-02-27. Script: `renames_all.txt` analysis.

| Quantity | Value |
|---|---|
| Rename entries (unique old→new) | **201**, in 29 commits |
| Similarity | 100%: 186; 90–99%: 3; 50–89%: 12 (git's default threshold is 50% [D git-diff]; [10] shows it misses 7 of 16 real inexact renames) |
| Kind | directory change only: 182; name only: 14; both: 5; case-only: 0 |
| Bulk | one commit moved **131** `docs/*.md` into `docs/archive/` (2026-07-02); two 13-file fixture moves (2026-09-25); a crate rename `crates/boyko-ecs → crates/boyko_ecs`; a module split `ecs/memory → crates/boyko_memory/src` |
| By extension | `.md` 131, `.rs` 34, `.pose` 24, others ≤ 2 |
| Deletions / additions (all refs) | 82 / 16,023 paths |

Path references in the auto-memory corpus: 255 files, 424 path occurrences, 14 of them with a `:line` anchor, **301 distinct paths**. They were resolved against the trunk tree (`integ/unified`) and the owner's checkout. Script: `linkrot.py`.

| Class | Paths |
|---|---|
| live in trunk and in the owner's checkout | 168 |
| live in trunk only (the owner's checkout is 365 commits behind) | 5 |
| **dead, but a unique git rename chain leads to a live path** | **58** |
| dead: deleted in git or other | 3 |
| never in git, basename unique in trunk (mostly elided `crates/…/x.rs` or crate-relative `shaders/x.hlsl` forms) | 50 |
| never in git, basename ambiguous / no match | 6 / 11 |

`file:line` anchors in trunk docs (excluding `docs/measurements` and `docs/archive`) number **10,790**, across 611 distinct files. **36** point to a file missing in trunk and 4 point past EOF. Whether the line still holds the cited text is [10]'s subject: 28% line fidelity after 4 months.

**[I] What this means.**

- File-level rot is dominated by **one kind of event, a bulk directory move**, and git's rename history repairs almost all of it exactly: 58 of 61 dead, once-tracked memory references.
- The second source of "rot" is not rot at all. Agents write elided and root-ambiguous paths. Links therefore need an explicit root and must be created by resolving a real path, not by parsing prose.
- At 611 anchored files and 10.8k anchors, a stat-only link check of every linked file is a sub-100 ms operation (§1.7).

### 1.3 The worktree landscape [M]

`git worktree list` gives 44 entries, 8 of them detached. Read-only `ls-tree`, `rev-list` and `merge-base` against trunk `integ/unified` (75bea42e):

| Group | Worktrees | Files in HEAD tree | Behind / ahead of trunk |
|---|---|---|---|
| trunk (`<lanes-dir>/<trunk>`) and `u/phys-l10` | 2 | 15,873 | 0–5 / 0 |
| active lanes (`u/research-0925` and two unpublished lane branches) | 3 | 15,841–15,857 | 21–25 / 4–8 |
| old, diverged (`feat/ecs-native-storage`, one detached tree) | 2 | 1,592–2,466 | 395–1,036 / 1–3 |
| stale, already merged (ancestors of trunk) | 36 | 1,590–5,166 | **272–1,036 / 0** |
| owner's main checkout `<workspace>/BoykoEngine` (`feat/multi-paradigm-render`, an ancestor of trunk) | 1 | 2,556 | 365 / 0 |

Other facts:

- 39 of 44 worktree HEADs are ancestors of trunk (the 36 stale ones, trunk, `u/phys-l10` and the owner's checkout).
- 12,900 of trunk's 15,873 files are `docs/measurements/**` fixtures. The code and docs part is ~3k files.
- Repository config [M]: `core.hooksPath = <repo>\.git\hooks` (absolute, so one directory serves every worktree), `core.ignorecase = true`, `core.autocrlf = true`, no `.gitattributes`, no fsmonitor.
- The existing hooks are graphify's `post-checkout` and `post-commit`.

**[I] What this means.**

- "The file is not here" is the *normal* answer for most links in most worktrees. It must be told apart from "the file moved" or "the file was deleted" *before* any search runs. Otherwise every read in a stale lane triggers expensive and wrong re-binding.
- The main checkout is neither trunk nor where agents work [02 §7.1]. The tree that `main` resolves against must therefore be configured (`files.main-tree = <lanes-dir>/<trunk>`), not assumed to be the repository root.

### 1.4 Windows behaviour of an explicit move [M]

Probe: `probe_sharing.py`. Raw `CreateFileW`/`MoveFileExW` through ctypes; the holder is either another handle in the same process or a second process, as noted. NTFS, Defender real-time protection on, non-elevated.

| Situation | Rename the **file** | Rename its **directory** |
|---|---|---|
| another handle open with share `R\|W` (no `DELETE`), which is what CRT `open()` does | **32** (sharing violation) | **5** (access denied) |
| another handle open with share `R\|W\|D` | ok | **5** |
| a second process has the file open (Python `open`) | **32** | **5** |
| a second process has its **cwd inside** the directory | ok (a file inside) | **32** |
| directory watch handle (`FILE_LIST_DIRECTORY`, share `R\|W\|D`, i.e. how `ReadDirectoryChangesW` watchers open it) on `sub/` | ok (file inside `sub/`); `sub/` itself ok | **parent of `sub/`: 5** |
| a second process holds a read-only **memory map** of the file | **32** (delete also 32) | — |
| running `.exe` image | ok (delete also ok on this Windows build) | — |
| destination exists, no flag | **183** | — |
| destination exists, `MOVEFILE_REPLACE_EXISTING` | ok (replaces) | directory: **5** (as documented [D]) |
| case-only rename `Readme.md → README.md` | ok; afterwards `stat("Readme.md")` **still succeeds** (case-insensitive) | — |

File identity through the NTFS file index (`st_ino`):

| Operation | Id kept? |
|---|---|
| rename in the same directory / move into a subdirectory | kept / kept |
| `git mv` | kept |
| copy then delete (cross-volume move, `cp`+`rm`) | **changed** |
| write a temp file, then replace over the original (atomic save) | **changed** |
| in-place truncate and rewrite | kept |
| **Claude Code `Edit` tool**, and `Write` overwriting an existing file | **changed** (the tool replaces the file) |
| `git switch` to a branch where the file sits under the old name | **changed** (git re-creates it) |

The Claude Code `Edit`/`Write` rows were measured by editing a scratch file with this session's own tools, reading the id before and after each call.

Rename timing (`probe_timing.py`, 200 files × 4 rounds at 100% CPU):

| Case | Time |
|---|---|
| `MoveFileExW` of a file, p50 | 3.9–9.5 ms |
| p90 | 8–77 ms |
| max | 156 ms |
| Files left to settle for 10 s first | no better (p50 ≈ 5.4 ms) |
| One `MoveFileExW` of a directory containing 1,000 files | **9.5 ms** |
| `MOVEFILE_WRITE_THROUGH` on a same-volume rename | no measurable difference. It only guarantees a flush for copy+delete moves [D MoveFileExW] |

The docs also say:

- Moving a *directory* requires the same drive [D].
- `MOVEFILE_COPY_ALLOWED` "succeeds leaving the source file intact" if the original cannot be deleted [D]. A cross-volume move can therefore silently become a copy.
- **Transactional NTFS is deprecated**: "Microsoft strongly recommends developers utilize alternative means … TxF may not be available in future versions of Microsoft Windows", and TxF is unsupported on ReFS, including Dev Drive [D MoveFileTransacted].
- There is **no OS transaction** that spans a rename and a database commit.
- `RmGetList` (Restart Manager) names the processes that hold a registered *file*. It returns `ERROR_ACCESS_DENIED` "if a path registered … is a directory" [D]. moirai can therefore name the holder for file failures but not for directory failures.

**[I] Consequences for `moirai file mv`:**

- Rename directories as one operation, never file by file (131 files file-by-file ≈ 0.5–20 s under load; one directory rename ≈ 10 ms).
- Never use `REPLACE_EXISTING` by default.
- Treat 5 and 32 as expected, retry them for about 1 s, then fail with a diagnosis: open handle (named via `RmGetList` for files), a process's cwd inside the directory, or a watcher on a subdirectory.
- Record the on-disk case, because case-only renames are invisible to stat on Windows.
- Refuse cross-volume directory moves.
- Verify the content `oid` after any cross-volume file move before deleting the source.

### 1.5 Which git hooks fire, measured [M]

Probe: `probe_githooks.sh`. Scratch repository, git 2.54.0.windows.1, logging hooks for `post-checkout`, `post-merge`, `post-rewrite`, `post-commit`, `reference-transaction` (committed state) and `post-index-change`.

| Command | Hooks that fired |
|---|---|
| `git mv a b` | `post-index-change` only (and the NTFS file id is kept) |
| `git commit` | `reference-transaction` (HEAD's branch, `AUTO_MERGE`), `post-commit` |
| `git switch <branch>` / `checkout <branch>` | `post-checkout <old> <new> 1`, `reference-transaction` ×3 |
| `git checkout <ref> -- <path>` / **`git restore`** | `post-checkout … 0`. Restore is not named in the docs [D], but it fires |
| `git merge --no-ff` (clean, committed) | `post-merge 0` |
| **`git merge --no-commit`**, then `git commit` | **no `post-merge`**; only `post-commit` at the commit (the docs: post-merge is not run "if the merge failed due to conflicts" [D]) |
| `git merge --ff-only` | `post-merge 0` |
| `git reset --hard` | **no checkout/merge hook**; `reference-transaction` only |
| `git stash` / `stash pop` | `reference-transaction` (`refs/stash`); `pop` also fired `post-checkout … 0` |
| `git cherry-pick` | `post-commit` + `reference-transaction` |
| `git rebase` | `post-checkout … 1` at start, `post-commit` per commit, `post-rewrite rebase` |
| `git commit --amend` | `post-rewrite amend` |
| `git worktree add` | `post-checkout 0000… <new> 1` |
| **`git apply`** of a rename patch | **nothing** |

Timing sensitivity: with the six logging hooks installed, the 17-command probe took more than 120 s, because every hook is an `sh.exe` spawn and load was 100%. `reference-transaction` reached its *committed* state 1–10 times per command (5 for one `stash`, 10 for one `rebase`). Each transaction also invokes the hook for its preparing and prepared states [D], so the real number of invocations is higher. `post-index-change` fired up to 6 times for one `stash`. A no-op-hook cost probe could not finish within 300 s under load.

**[I] Conclusions.**

- `reference-transaction` and `post-index-change` are far too frequent to host moirai work on Windows.
- `post-checkout`, `post-merge` and `post-commit` together see branch switches, clean merges and every commit. The last is the only hook that sees renames produced by `apply`, `--no-commit` merges and plain `mv`, but only once they are committed.
- Git hooks miss uncommitted moves entirely, and they miss the owner's dominant merge form (`--no-commit`, 42 of 85) until its later commit.

Git hooks run with cwd = the worktree root [D githooks], so a hook knows which worktree it serves. The owner's absolute `core.hooksPath` means one installed block covers all 44 worktrees [M config; I].

The Claude Code Bash sandbox is not supported on native Windows [08 §2, D], so nothing *technical* stops an agent from writing `.git/hooks`. Hook installation must be an owner action by **policy**. The owner already bans `core.hooksPath` changes after an agent used one to bypass hooks [02 §5.1].

### 1.6 Claude Code harness facts for 2026 [D, M]

The current hook event list has 33 events [D hooks]:

> SessionStart, Setup, UserPromptSubmit, UserPromptExpansion, PreToolUse, PermissionRequest, PermissionDenied, PostToolUse, PostToolUseFailure, PostToolBatch, Notification, MessageDisplay, SubagentStart, SubagentStop, TaskCreated, TaskCompleted, Stop, StopFailure, TeammateIdle, InstructionsLoaded, ConfigChange, **CwdChanged**, DirectoryAdded, **FileChanged**, **WorktreeCreate**, **WorktreeRemove**, PreCompact, PostCompact, PreModelSwitch, PostModelSwitch, Elicitation, ElicitationResult, SessionEnd.

There is no file-move tool; moves happen through `Bash`/`PowerShell`. `MultiEdit` appears neither in the current reference nor in any of the owner's transcripts [M: 0 files].

| Capability | What the docs say [D] | Relevance to R4 [I/M] |
|---|---|---|
| `FileChanged` | "Claude Code detects changes with a filesystem watcher, not by inspecting tool calls, so it runs the hook no matter what changed the file". `event` ∈ `change`, `add`, `unlink`. Watch list = matcher filenames in cwd + `watchPaths` (absolute) returned by SessionStart/CwdChanged/FileChanged. No decision control; stderr goes to the user only | Could deliver `unlink` for linked files without a moirai process. **Risk [M+I]:** if Claude Code's watcher holds per-directory handles, as `ReadDirectoryChangesW` watchers do, then watching linked files in many directories makes renames of their *ancestor* directories fail with error 5 (§1.4) for the whole session. Not verified for Claude Code's implementation → off by default; at most a small hot set |
| `CwdChanged` | `old_cwd`, `new_cwd`; can return `watchPaths` | Not needed |
| `WorktreeCreate` / `WorktreeRemove` | Create **replaces** git worktree creation; Remove fires at session exit / subagent finish | Do not use Create [07 §4.2]. Remove could mark a lane's tree gone, but the lazy `doctor lanes` check already covers it |
| `PostToolUse` Bash/PowerShell | `tool_input.command`; `tool_response` = `stdout`, `stderr`, `exit_code` | Hint source only |
| **`tool_response.bashEditDiff`** (≥ 2.1.269) | "changed files" under the repo while the command ran: `changedFiles` (≤ 200 absolute paths), `files` (≤ 5 diffs with `created`/`deleted`), `skipped` for `git checkout`/`stash`, **`shared` when another Bash call ran in the same repo concurrently**. "best effort and in public beta … Use the list to find what to review, not to enforce a policy". Recorded by default only in auto/bypass modes; `bashEditDiffEnabled` is a user/managed setting | In the owner's transcripts [M]: 699 diffs, all in BoykoEngine sessions; **693 (99.1%) `shared`**; 26 non-empty; **0** with a created/deleted flag; 1 `unavailable`. In a 16-agent parallel workflow it is almost always attributed as shared. Use it as a doorbell at most |
| `if` filter | One permission rule per handler. `Bash(mv *)` matches any subcommand whose command word is `mv`, including inside `$()`. **Patterns naming more than the command word (`Bash(git mv *)`) run anyway on `$()`, backticks or `$VAR`**, and so does any command whose name Claude Code cannot determine | Use only single-word patterns (`Bash(mv *)`, `Bash(rm *)`). Multi-word patterns would fire on ~37.6% of the owner's Bash calls [M] |
| `cwd` in hook input | "`cwd` follows Claude … the worktree root after Claude enters a worktree, and the new directory after Claude runs `cd`"; `${CLAUDE_PROJECT_DIR}` stays at the session start | Relative paths in `mv` args resolve against `cwd`; the tree is found from `cwd` |
| File-tool paths | `tool_input.file_path` "always absolute", with backslashes on Windows | `Write`/`Edit` hooks can register `file new` for free, but they do not move files |
| Subagents | Tool hooks fire inside subagents with `agent_id`/`agent_type` | Attribution of observed moves |
| Subagent transcripts | Do not persist `toolUseResult` [M: this session] | `bashEditDiff` cannot be mined after the fact from Workflow agents |

Owner's Claude Code version: **2.1.281** [M, transcript `version` field].

### 1.7 Costs of the lazy check [M]

Probes: `rsprobe` (Rust, std only), `walkidx.py`. Load 100%.

| Operation | Cost |
|---|---|
| Existence + size + mtime of one path via `GetFileAttributesExW` (no handle) | **17–67 µs** |
| The same via Rust `std::fs::metadata` (opens a handle) | 61–139 µs (2–4× slower). The first pass over 15.9k cold paths took 4.8–15.8 s, the Defender per-open effect [05 §6.4] |
| Python `os.stat` (attribute fast path) | 37–42 µs |
| Parse `.git/worktrees/<trunk>/index` (15,879 entries, 2.7 MB, v2) | Rust 15–74 ms; Python 91 ms |
| Index entries whose size+mtime still match the file | 15,873 of 15,879, so blob ids are reusable without hashing, but only as git's LF-normalised ids |
| Full walk of the trunk worktree (15.9k files, 5.0k dirs, skipping `.git`/`target`) | 0.43–0.5 s Python; 2.2–2.7 s Rust `read_dir` (varied with load) |
| Full walk of the owner's checkout (7.5k files incl. untracked, 891 dirs) | 0.09–1.5 s |
| CRLF check of one tracked `.rs` file | `git ls-files --eol` → `i/lf w/crlf`; `hash-object --no-filters` ≠ index blob id; `hash-object` (filters on) = index blob id |

[I] Budget: checking the 611 anchored doc files takes ~10–40 ms. A pack's ≤ 50 links take 1–3 ms. A deep search (full walk) is a 0.1–3 s explicit command, never a hook.

---

## 2. What a file link is: identity, path and observation

### 2.1 Two layers

| Layer | Contents | Versioned per moirai branch? | Exported to the git image? |
|---|---|---|---|
| **Link (intent and identity)** | a **file node** (uid, `root`, `path`, content `oid` per [10], `size`, `observed` = moirai commit of the last reconciliation, `observed_git` = git HEAD of the observing tree, `kind`); edges from knowledge/task nodes to it, carrying **anchor** props per [10 §0] | **yes**: an ordinary node; re-binds are ops | **yes** (`.moi`) |
| **Resolution (where it is in *this* tree now)** | `FILEOBS` cache keyed by (file `#N`, tree key = worktree root + HEAD oid): resolved path, status, stat triple, `oid`; `PENDING` observations; `FSINTENT` records | **no**: store-level runtime, like leases and `settled`/`deleted` markers [30 §5d.1] | **no** (extends I36′) |

**[I] Why the split.** Git versions the *files*, and moirai versions the *links*.

- Where a file sits in a particular worktree is a fact about that worktree's git tree and working copy. Merging it as data would duplicate git.
- *Which* file a note is about, *which* symbol it cites, and the last path that was agreed on a branch are knowledge, and they must branch, merge and travel in the image (R1, R3).
- Re-binds therefore land as commits only on the branch whose tree observed them (§5). All other sightings stay runtime.

### 2.2 Where the file node lives in the schema

Two options were considered:

- **(a)** Add a 14th kind `file`.
- **(b)** Widen the existing `artifact` kind ("pointer to a file outside the store", `path`, `sha256`, `bytes`, `artifact_kind`, `excerpt` [30 §3.2]).

The recommendation is **(b)**. Kinds stay at 13 and the skill does not grow. Details:

- `artifact_kind` gains `source`, `doc`, `asset`, `generated` and `dir` alongside the run-output kinds.
- New fields: `root`, `oid`, `observed`, `observed_git`.
- `sha256` stays for run outputs.

New edge: `at` (any node → artifact), historical class, props = anchor (symbol / normalised-line hash / context hash / ≤ 120-char excerpt / line hint) per [10 §0]. Its delete policy is "tombstone ref + src `suspect`".

`finding.where {file:symbol@sha}` [30 §3.2] becomes an `at` edge. A `cites` to a file becomes `at` with a `pinned_commit` prop.

### 2.3 Every path-bearing field in the synthesis, and how R4 treats it

| Field (synthesis) | Today | Under R4 [I] |
|---|---|---|
| `artifact.path` | string | the file node's `path`; re-bound |
| `finding.where` (file:symbol@sha) | string | `at` edge + anchor props |
| `task.files_owned` (set<glob>) | globs | stay globs. An explicit directory move rewrites globs whose **literal prefix** (text before the first wildcard) lies under the moved prefix, in the same commit. Lazy checks flag globs that match nothing in the tree as `suspect` |
| `note/rule.applies_to.globs`, `area.path_globs` | globs | same as above |
| paths inside note/rule **bodies** | prose | not re-written: bodies are versioned text, and rewriting would create merge noise. Packs annotate a mention that resolves to a file node with its current path. Parsing prose into links only happens for exact root-relative paths that resolve at write time (the `#N` sigil rule's analogue [30 §3.3]); §1.2 shows prose paths are often elided or root-ambiguous |
| `lane.worktree_path`, `run.script_path`, `run.journal_path` | absolute | `root = abs` artifacts: existence check only, no re-binding. `git worktree move` breaks `worktree_path`; `doctor lanes` reports it |

### 2.4 Roots

A link's `path` is relative to a **named root**, stored as a symbol:

| Root | Meaning |
|---|---|
| `project` | the git worktree top-level containing the caller, or the directory bound by `worktree bind` when there is no git (R2) |
| `memory`, `scratch` | per-machine directories, mapped in `config` |
| `abs` | absolute paths |

Paths are stored with `/` separators, in the on-disk case read back after the operation, as UTF-8 bytes (git's rule; no Unicode normalisation).

Only `project` links are re-bound across worktrees. The same `project:crates/x/src/a.rs` resolves separately in each of the 44 trees.

---

## 3. Explicit commands

### 3.1 Verbs

These are CLI only. MCP `write` gains only the record-only op kinds `relink` and `link_file`, because:

- The MCP server is shared across worktrees [08 §2].
- Three roles have no `Write`/`Bash` [07 §5.4]. Letting them move files through moirai would bypass their tool envelopes.

```
moirai file add PATH.. [--kind source|doc|asset] [--link #N [--anchor sym:NAME | line:@quote.txt]]   # register existing files
moirai file new PATH --stdin [--link #N]                     # create + register (doc-writer, orchestrator)
moirai file mv SRC.. DST [--git] [--dry-run] [--retry-ms 1000]
moirai file rm PATH.. [--reason T] [--replaced-by PATH|#F] [--trash] [--dry-run]
moirai file relink #F|PATH --to NEW [--after]                # record a move that already happened (hg's `rename --after`)
moirai file revert COMMIT                                    # FS-aware inverse of a commit that moved/removed files
moirai links check [--scope #N | --path GLOB | --all] [--tree DIR] [--deep] [--budget-ms N] [--ids]
moirai links fix [#F.. | --auto] [--to PATH] [--drop] [--yes]
moirai hooks install --git [--dry-run]                       # owner-run; appends marked blocks after graphify's
moirai hook fs-changed | git-post-commit | git-post-checkout | git-post-merge
```

Every write verb takes the synthesis's `--branch`, `--lease`, `--idempotency-key` and `--if-rev` [30 §7.1]. Exit codes follow the synthesis. 8 ("partial batch") is used when some sources of a multi-source `mv` fail.

### 3.2 Crash-safe protocol for `file mv` / `file rm`

The main constraint: no OS transaction spans the rename and the moirai commit (TxF is deprecated [D]). The moirai writer byte must not be held across a rename that can take 150 ms or block on error 32 (§1.4) [30 §4.5].

1. **Plan.** Resolve the branch (lease → directory binding → …) [30 §5a.4]. Stat the sources; the destination must not exist. Refuse:
   - a directory move across volumes;
   - a move into a path that git ignores, unless `--allow-ignored`.

   Compute `oid` for single files (for directories, the linked files' `oid`s are already known). List the linked file nodes under the source prefix through a path index; see note 1 after this list.
2. **Intent (durable, ~2 ms).** Append the runtime record `FsIntent{id, op, [(src, dst, oid)], dir: bool, branch, pid, hlc}` in its own durable commit group. It is not a graph change.
3. **Filesystem.** Do one `MoveFileExW` (no `REPLACE_EXISTING`) per source path, one for the whole directory. On 5 or 32, retry with backoff for ≤ `--retry-ms`, then fail with a diagnosis: `RmGetList` names file holders; for directories the message names the likely classes (a shell with its cwd inside, an open file, a subdirectory watcher). For cross-volume files: copy → flush → verify `oid` → then delete the source. `--git` delegates the rename to `git mv` (§3.4). `rm` deletes, or moves to `.moirai/trash/<intent>/` on the same volume with `--trash`.
4. **Commit (durable, ~2 ms).** Graph ops go into one commit together with `FsIntentDone{id}`:
   - `SetField(path)` on every affected file node;
   - glob rewrites (§2.3);
   - for `rm`, file node `status = deleted` with `reason`/`replaced_by` and the referrer policies (historical `at` edges get a tombstone rendering and their sources become `suspect`; `files_owned` entries are dropped with a notice);
   - commit trailer `Moirai-Relink: explicit` and, for directories, `Moirai-Move-Prefix: <old>/ -> <new>/` (§6.3).

   The commit uses CAS on each file node's `rev_seq`. On a CAS failure (someone re-bound it meanwhile), the op is replaced by an observation-based resolution (§4.1 step 4).
5. **Recovery.** At any writer's next open, or in `doctor`, each `FsIntent` without a matching `Done` is classified by observation:

| Filesystem state | Decision |
|---|---|
| src present, dst absent (oid = src) | the rename never happened → `FsIntentAborted` |
| src absent, dst present, oid matches | the rename happened → write the step-4 commit (roll forward) |
| both present (cross-volume copy not finished, or recreated) | `ambiguous`; delete nothing; show in `brief` |
| neither present | `missing`; show in `brief` |
| intent's pid alive | leave it; the owner is still working |

**[I] Why keep an intent record when lazy reconciliation would find the moved file anyway:**

- It preserves *intent*: an `rm` interrupted before its commit must not look like "lost".
- It makes a multi-path move one logical operation.
- It lets a second `file mv` on an overlapping path refuse while the first intent's pid is alive.

Cost: two durable flushes (~4 ms [05 §2.2]) plus the rename(s), under ~15 ms for a single file with Defender at idle (est.; 4–156 ms measured under 100% load).

Notes on the plan step:

1. The path index is a sorted `(root, path) → #N` column of artifact rows. At the owner's scale (~600–2,000 linked files) it is a few KB and is built into the overlay [I].
2. Idempotency: the key defaults to `blake3(op, src, dst, oid)`, so a Workflow resume that re-runs `file mv` lands once [30 §6.4].

### 3.3 Batch and directory moves

- `file mv dirA dirB` is **one** rename (9.5 ms for 1,000 files [M]) and **one** commit with k `SetField(path)` ops, where k = linked files under `dirA`.

  Example: moving the 131-file `docs/` → `docs/archive/` [M] with, say, 60 linked files ≈ 60 × ~40 B = 2.4 KB of ops [30 §4.3].
- Multi-source `file mv a b c dir/`: one intent listing all sources. Items that fail at the filesystem step (32/5) are reported, the others commit, and the command exits 8.
- The `Moirai-Move-Prefix` trailer records the directory move so that merges can compose it (§6.3).

### 3.4 Interplay with git (R2-safe)

- **Default: moirai never touches the git index.** Git records trees, not renames, and detects renames by content at diff and merge time (`-M`, default 50%; `merge.directoryRenames` defaults to `conflict` [D]). A filesystem move followed by the agent's usual `git add -A -- <src> <dst>` produces the same commit as `git mv`. moirai prints that hint.

  Touching the index by default would risk re-creating the owner's recorded incident: another agent's staged deletion committed and pushed [01 §7]. It would also collide with the git bans that 26 of 38 briefs carry [02 §5.1].
- `--git`: inside a git worktree, with a tracked source and role policy permitting, run `git mv` (74 ms idle [08 §2]; 2 s under load [M]) inside the same intent protocol. It lives in an optional adapter; the core never links or requires git (R2).
- `git mv` done by an agent directly fires only `post-index-change` (§1.5). moirai learns about it lazily, or through the filtered Claude hook if the command word is `git`, which is not recommended (§4.2).

### 3.5 Undo, revert, cherry-pick: history never moves files

**[I] Rule:** no moirai history operation touches the working tree. That covers `undo`, `revert`, `cherry-pick`, `checkout`, `merge`, `sync` and `image import`. They change links only.

A plain `revert` of a commit carrying `FsIntentDone` does three things:

- it warns that the commit moved files on disk;
- it suggests `moirai file revert <commit>` instead, which runs the inverse filesystem operations through the §3.2 protocol and records the link change;
- if forced, it leaves a link that the next check marks `diverged` (the branch says A, the tree says B) rather than silently re-binding it back.

Importing an image never replays filesystem operations (R3).

---

## 4. Automatic paths

### 4.1 Lazy reconciliation, the core mechanism

**Triggers.** None is a background process.

| Trigger | Scope | Budget |
|---|---|---|
| `SessionStart` hook (`moirai hook session-start`, already in the design [30 §7.5]) | links of the brief's items + links under the bound lane's `files_owned`, then others until the budget | ≤ 150 ms of stats (~2.5–7k paths at the measured 20–60 µs [M]); the brief shows ≤ 3 link lines + a count |
| `pack`, `show`, `get`, MCP `get`/`pack` | links of the rendered nodes | ≤ 50 stats ≈ 1–3 ms |
| writes that add a link (`link --file`, `file add`) | the new path must exist (or `--planned`) | 1 stat |
| merge ritual: `merge-check` and, **after the code merge**, `links check --tree <trunk>` | links touched by either side | as above |
| `SubagentStart` role pack | as `pack` | as `pack` |
| explicit `links check [--deep]` | anything | unbounded; `--deep` walks the tree (0.1–3 s [M]) |

**Algorithm for one link in one tree.** Tree T has root R and HEAD H; the link has `path` p, `oid` o, `observed_git` g.

1. **Stat** `R/p` with `GetFileAttributesExW`.
   - Present with size and mtime equal to the cache → `ok`.
   - Present but changed → rehash (or take the blob id from the git index when the index stat matches [M §1.7]). Same `oid` → `ok`. Otherwise `modified`: a content drift, not a move. The anchors re-resolve per [10], and the link stays.
   - On a case-insensitive filesystem, "present" can hide a case-only rename (§1.4). When the cached on-disk name differs, re-read the real name through directory enumeration and record it.
2. **Absent → first ask "should this tree have it?"** (§5.2). With git, if `g` is not an ancestor of H (commit-graph generation numbers, cached as lazy facts [30 §5c]), the status is **`absent-in-tree`**: no search and no re-bind.
3. **Evidence cascade.** Take the first source that yields a unique candidate:
   - (i) a moirai op on this branch after `observed` (already applied to `path`);
   - (ii) a runtime `PENDING` observation for (p, T) from a hook;
   - (iii) the git rename chain `g..H` restricted to p. This needs git objects: `git diff-tree -M` when git is present, or the image module's pack reader once it exists (v1.1 [30 §2.10]). The step is skipped otherwise.
   - (iv) the same `oid` among index entries added since `g`, or files in p's directory and its sibling directories;
   - (v) the NTFS file-id hint [10, 12];
   - (vi) only with `--deep` or explicit `fix`: the similarity search of [10].
4. **Classify and act.**

| Evidence | Status | Action |
|---|---|---|
| exact `oid` match, unique; or git rename R100 | `moved` (confidence 1.0) | **auto-apply** (see "Where auto-applied re-binds are written" below) |
| git rename ≥ 90% or [10]'s "≥ 0.5 with ≥ 0.2 margin", unique, with one-side directory evidence | `moved?` (0.9) | auto-apply only if owner policy allows (§9 Q3); otherwise propose |
| several candidates, split, or merged | `ambiguous` | a conflict-like record; `links fix #F --to …` |
| nothing | `missing` | **grace**: never auto-deleted; the pack says "missing since c…, last at p" |
| target outside the root or ignored (from a hook observation) | `outside` | runtime only; no commit |

**Where auto-applied re-binds are written.** A re-bind lands as a commit only when T is bound to the branch being viewed (§5). Otherwise it is recorded as `PENDING` and shown as `pending`.

**[I] Rules that stop thrashing and wrong writes:**

- **Monotone re-binding.** Automatic re-binds never move a link back to a path that the branch's own history moved it away from. Returning requires explicit `relink`, `file revert`, or git evidence (a commit that renames it back).
- **Quarantine grace.** A `missing` link produces no write. If the file is back by the next check, nothing happened. This is exactly the 42% temp-move pattern of §1.1.
- **Deletion is never inferred from absence alone.** A link reaches `deleted` only by `file rm`, by `links fix --drop`, or, if the owner allows (§9 Q4), by a git commit that deletes the path, seen on the trunk tree.
- **Idempotent re-bind writes.** The key is `relink:<uid>:<new path>:<oid>`. Two agents that see the same move in the same bound tree land one commit.

**Directory inference.** When at least 90% of a branch's linked files under prefix P moved with the same P → P′ rewrite, `links fix` records it as a directory move: a `Moirai-Move-Prefix` trailer plus glob rewrites. Git's merge applies the same majority idea to new files (`merge.directoryRenames`) [D].

### 4.2 Claude Code hooks as accelerators (optional)

| Hook | Filter | Handler (exec form, `async: true`, fail-open) | Value | Cost |
|---|---|---|---|---|
| `PostToolUse` | `if: "Bash(mv *)"`, plus separate handlers for `Bash(rm *)`, `PowerShell(Move-Item *)`, `PowerShell(Rename-Item *)`, `PowerShell(Remove-Item *)` | `moirai hook fs-changed`: reads `cwd`, `tool_input.command` and, if present, `tool_response.bashEditDiff`. Parses arguments **only to narrow where to look**; then runs §4.1 steps 1–4 for linked paths under those arguments, or under `cwd` when parsing fails | turns "next session" into "next tool call" for the 148 `mv` segments and ~957 `rm` calls | fires on ~0.9% of shell calls (185 moves + ~960 deletions per 121,571 [M]) × one spawn (15–73 ms idle [05 §2.1]); async, so it does not block the agent |
| `PreToolUse` (optional nudge) | `if: "Bash(mv *)"` | if an argument resolves to a linked file: `additionalContext` "3 moirai links point at src/a.rs; `moirai file mv` keeps them and their intent" | makes agents record intent | same spawn; must stay **non-blocking**, like the owner's existing graphify `PreToolUse` nudges [02 §4.1] |
| `SessionStart` | — | runs the §4.1 check within budget (already in the design) | covers moves made between sessions: the owner in Explorer, git operations, other lanes | ≤ 150 ms |
| `UserPromptSubmit` | — | the synthesis's delta [30 §7.5] gains one line per re-bind or missing link that touches the agent's leased or cited nodes | pushes news to the model | ≤ 600 chars |

**Not recommended:**

- `Bash(git *)` and any multi-word pattern. It fires on every git command, or on 37.6% of all Bash calls [M, D].
- `FileChanged` with `watchPaths` over many files. The ancestor-rename hazard is [M] for raw watch handles and unverified for Claude Code's watcher.
- `WorktreeCreate` [07].
- `bashEditDiff` as evidence, because it is 99% `shared` in the owner's runs [M].

### 4.3 Git hooks as accelerators (optional, owner-installed)

`moirai hooks install --git` appends marker-delimited blocks (`# moirai-hook-start` … `# moirai-hook-end`) after graphify's blocks in the shared `core.hooksPath`. It never replaces a hook and is idempotent. The owner runs it, by policy. On Windows there is no sandbox preventing agents from doing it (§1.5), so the role write policy must refuse `hooks install` from agents.

| Hook | Handler | Catches | Misses |
|---|---|---|---|
| `post-commit` | `moirai hook git-post-commit`: `diff-tree -M --diff-filter=RD HEAD~1 HEAD` intersected with the path index → evidence into `PENDING`, or a direct re-bind when the tree is bound | every **committed** rename or delete, however it was made (`mv`, `apply`, `--no-commit` merge, cherry-pick, rebase commits) | uncommitted moves; `reset --hard`; `stash` |
| `post-checkout` (flag 1) | mark the tree's `FILEOBS` entries stale (O(1): the tree key changes with HEAD) | branch switches, worktree add | file-level checkouts are fine to ignore |
| `post-merge` | as `post-checkout`, plus `links check --scope merged` | clean committed merges | `--no-commit` merges (42 of 85 [M]); those are caught at their `post-commit` |

**[I] Cost.** Each firing is `sh.exe` plus a moirai spawn. Graphify already pays this per commit (it runs Python). moirai's block should add ≤ 1 spawn per commit (~984 `git commit` calls in the corpus [M]).

`reference-transaction` and `post-index-change` are rejected: up to 10 committed transactions and 6 index writes per command [M], each a spawn. `post-rewrite` adds nothing beyond `post-commit`.

Without git (R2), none of this exists and nothing breaks.

### 4.4 Assessment: can a hook observe moves by parsing commands?

Answer: **partially, and only as a hint** [M, I].

- **What parsing gets right.** Literal two-argument `mv src dst` (≈ 40% of `mv` [M]).
- **What parsing gets wrong or misses:**
  - variables and globs: 57% of `mv` calls;
  - loops, `xargs` and `-exec`: 21%;
  - multi-source moves: 16%;
  - relative paths that need the shell's cwd *at that segment*, which `cd a && mv x y` changes mid-command;
  - Python renames (63);
  - every git operation (§1.1);
  - PowerShell pipelines.
- **What makes it safe.** The hook never trusts the parse. It uses the parse to choose *which linked paths to stat*, then decides from the filesystem and content. A failed parse degrades to "stat the linked files under `cwd`". This is the same principle as [12]'s conclusion: explicit capture is precise when used, and observation is the truth.

### 4.5 Why no watcher, even optionally, in v1

**[I]** The zero-idle-CPU rule [02 §9], plus event loss on overflow [08 W10], plus the ancestor-rename hazard [M §1.4] (a watcher holding subdirectory handles turns every directory-move attempt in that subtree into error 5), plus the USN journal needing admin [12]. A `moirai watch` mode could exist later in the M6 leader: one recursive watch at the root, which does not block renames of subdirectories [M], and never during `quiet`. It is not needed for correctness.

---

## 5. Worktrees and branches

### 5.1 Which tree a reader resolves against

The first rule that applies wins [I]:

1. an explicit `--tree DIR`;
2. the caller's cwd → git worktree top-level. For hooks, the `cwd` input field [D]; for the CLI, the process cwd;
3. the lease → run → lane → `lane.worktree_path` (MCP calls from any worktree [08 §2]);
4. the branch → bound directory (`worktree bind`);
5. `config files.main-tree` for `main`. For BoykoEngine this is the trunk worktree (`<lanes-dir>/<trunk>`), **not** the repository root, which is 365 commits behind and off-limits to agents [M, 02 §7.1].

Every rendered link names its tree in the result header: `files @ <lanes-dir>/l5np (u/l5np 7c1e0a, dirty)`.

### 5.2 "Absent in this tree" versus "moved"

39 of 44 worktrees are ancestors of trunk, and 36 of them lag it by 272–1,036 commits [M]. A link created on trunk yesterday is therefore absent in most trees.

`observed_git` (the git HEAD of the tree where the link was last reconciled) plus commit-graph ancestry answers "should this tree contain this version of the file?" in microseconds [30 §5c]:

| Situation | Answer | Action |
|---|---|---|
| `observed_git` ∉ ancestors(H) | `absent-in-tree` (the tree is behind, or on another line) | no search, no write |
| `observed_git` ∈ ancestors(H) and the path is absent | "moved or deleted since" | run the §4.1 cascade over `observed_git..H` |
| no git (R2) | fall back to "absent → cascade (ii)–(v)" | never auto-delete |

The synthesis already reads commit-graph generation numbers textually and caches ancestry as lazy facts [30 §5c].

### 5.3 Who may write a re-bind

This is the rule that keeps branches honest [I]:

| Observing tree | Writes a versioned re-bind on | Otherwise |
|---|---|---|
| a lane worktree bound to `lane/x` (`lane open --worktree`) | `lane/x` | — |
| the tree configured as `files.main-tree` (trunk) | `main` | — |
| an unbound worktree (`wf_*`, harness `isolation: worktree` trees [07 §4.4], scratch `_split-*`, `mq-*`) | **nothing** | a `PENDING` runtime observation keyed by (git branch, tree, p → p′, oid). It is promoted automatically when the same move is later seen in a bound tree, typically trunk after the code merges, or confirmed with `links fix` |
| a stale tree (`absent-in-tree`) | nothing | nothing |

This mirrors the synthesis rule that scratch and `wf_*` worktrees write to `main` [30 §2.3], with one correction. **File-location facts from an unbound tree are about a git line that has not reached trunk.** Writing them to `main` would make `main`'s view wrong for the trunk tree until the code merges. Other kinds of write from those trees still go to `main` as designed.

### 5.4 The lane lifecycle with files

1. `lane open l5np --worktree <lanes-dir>/l5np --git-branch u/l5np` binds the tree [30 §5c].
2. The developer runs `moirai file mv crates/a/x.rs crates/b/x.rs`. The filesystem move happens in `<lanes-dir>/l5np` and the re-bind lands on `lane/l5np`. `main` and the trunk tree still say `crates/a/x.rs`, which stays **consistent**.
3. Another agent in the lane runs a raw `mv`. Lazy (or the hook) sees it in the bound tree and writes the re-bind on `lane/l5np`.
4. Merge ritual, in the synthesis's order: `merge-check` → `moirai merge lane/l5np --into main` → `git merge u/l5np` [30 §7.6 step 8]. **Hazard [I]:** between the moirai merge and the git merge, `main` says `crates/b/x.rs` while the trunk tree still has `crates/a/x.rs`. A naive check would re-bind *back*. It is prevented because:
   - the re-bind op carries `observed_git` = the lane commit containing the move, which is not an ancestor of the trunk HEAD, so the status is `pending` (code not merged yet), not `missing`;
   - the monotone rule (§4.1) forbids automatic back-moves.
5. After `git merge` and its commit (a `--no-commit` merge fires no `post-merge` [M]), the merge script runs `moirai links check --tree <lanes-dir>/<trunk> --since <pre-merge HEAD>`. That confirms the paths and resolves any path conflict by observation (§6.2). This line belongs in the `moirai-branches` skill ritual [30 §7.5].
6. For uncommitted moves in a lane at merge time (a dirty tree), `merge-check` lists links whose lane re-binds rest on uncommitted filesystem state: "commit the move first".

### 5.5 Moves that happen only on one git branch

When a file moves on git branch B1 but not B2, each tree resolves its own truth. Each moirai branch carries the path its own tree observed. When B1 merges into B2 (code) and `lane/B1` into the corresponding moirai branch (links), the typed merge takes B1's path (B2 unchanged = base). The post-merge observation confirms it.

When the two branches' histories disagree, §6.2 applies.

---

## 6. Versioning (R1) and the git image (R3)

### 6.1 Ops and provenance

A re-bind is an ordinary `SetField{artifact #F, path, old, new}`, plus `SetField(oid)` when the content changed. There is no new op kind for re-binds.

Provenance rides in the commit [30 §4.3], as trailers in the image:

- `Moirai-Relink: explicit | hook | lazy | git | merge-observation`
- `Moirai-Relink-Evidence: oid | git-R100 | git-R93 | similarity:0.91 | user`
- `Moirai-Move-Prefix: docs/ -> docs/archive/` (zero or more)

Anchor re-resolution changes the `line hint` edge prop. That prop is **runtime-cached, not versioned**: line hints are derived. Only the symbol, content hash, context hash and excerpt are versioned, and they change only when the author re-anchors.

### 6.2 Merge rules for links

These are an addition to [30 §5a.7].

| Case (base → ours / theirs) | Rule | Result |
|---|---|---|
| both re-bound the same file to the same path | equal → take | clean |
| one side re-bound, the other untouched | take the change | clean |
| different new paths | `FieldEdit` conflict value | **resolution by observation**: at the next `links check` in the merged tree, if exactly one candidate exists with the `oid`, auto-`Resolve` (trailer `merge-observation`); otherwise `ambiguous` in packs |
| ours = directory move P→P′ (commit carries `Move-Prefix`), theirs = rename inside P (`P/a` → `P/sub/b`) | **`PathCompose`**: apply the prefix rewrite to theirs' new value (`P′/sub/b`) | clean; verified by observation. The inferred-prefix version mirrors git's `merge.directoryRenames` (default `conflict` [D]); moirai composes only when a `Move-Prefix` hint exists and the composed path is then observed |
| ours = `status deleted` (`file rm`), theirs = re-bound or content changed | `DeleteVsModify` [30 §5a.8] | policy: **resurrect if the file exists in the merged tree**, else delete wins |
| ours changed `oid` (content), theirs changed `path` | disjoint keys | clean: moved and edited |
| both changed the anchor excerpt on the same `at` edge | `FieldEdit` on the edge prop | conflict value; the pack shows base text [30 N15] |
| an `at` edge added on one side to a file node deleted on the other | `at` is historical → tombstone rendering, never `DanglingEdge` | clean; the source is flagged `suspect` |

**[I] Why resolution by observation is right here.** The merged *code* tree, produced by git, is the authority on where files are. moirai's merge only has to avoid inventing a location, and to pick the observed one when its own two histories disagree. This keeps the path conflict class out of the agents' manual-resolution queue in the common case.

**Alternative considered and rejected: a directory-node tree** (Kleppmann moves over `dir` nodes, so that a directory rename is one op and composes automatically):

- Git has no directory identity. A partial directory move (split) has no single right answer.
- It adds a second hierarchy beside `parent`, with its own depth rule.
- It adds nodes for every ancestor directory.

`Move-Prefix` hints plus observation give the same composition for the observed cases at a fraction of the schema weight.

### 6.3 Image serialization

This extends [30 §5b.2]. A file node:

```
moirai-node 1
uid: 5b1e0c7a9d2f4e6b8a1c3d5e7f901234
kind: artifact
title: crates/boyko_ecs/src/ecs/core/entity.rs
status: present
field artifact_kind: source
field bytes: 18231
field observed: c4471a…(64 hex) 2026-09-26T09:12:40.118Z
field observed_git: sha1:75bea42e34a5942eb6593b2867480e8fbc95eed1
field oid: sha1:de177738b58e970465382658e69b18745029e248
field path: crates/boyko_ecs/src/ecs/core/entity.rs
field root: project
```

A referrer's out-edge with anchor props:

```
edge at -> 5b1e0c7a9d2f4e6b8a1c3d5e7f901234 anchor=sym:spawn_entity ctx=b3:9c1f… line=b3:41aa… excerpt="pub fn spawn_entity(&mut self" pin=c4410…
```

Notes:

- `oid` is [10]'s git-blob-compatible id of the EOL-normalised content (algorithm-tagged, so SHA-256 repositories work). It is **not** the raw-byte hash, which differs under `autocrlf=true` [M].
- No store-local data: no `#N`, no tree key, no `FILEOBS`/`PENDING`/`FSINTENT` [30 N4, I36′].
- Tombstoned file nodes are files, like every tombstone [30 §5b.1].
- A git-side hand edit of `field path:` imports as a foreign `SetField`. It is validated like any op, **moves nothing on disk**, and shows up at the next check as `diverged` if the tree disagrees.
- Round trip: lossless for link intent and last reconciled paths. Resolution state is not carried, by design.

### 6.4 R2: what works without git

Everything except git evidence (§4.1 cascade iii), git hooks, `absent-in-tree` ancestry, `--git` and `bashEditDiff`, which only exists in git repositories [D]:

- explicit verbs;
- stat checks;
- same-directory and sibling `oid` search;
- similarity search;
- intent recovery.

`observed_git` is empty. A missing file in a non-git tree always runs the cascade, bounded by `--budget-ms`, and is never auto-deleted.

---

## 7. Agent UX

### 7.1 Statuses and markers

Markers appear only on links that are not `ok`. Lengths are estimates at ~3.5–4 chars/token for English.

| Status | Pack/show marker | Chars |
|---|---|---|
| `ok` | none | 0 |
| `modified` | `[changed since c4410; anchor spawn_entity re-found at L132]` | ~60 |
| `moved` | `[moved from src/ecs/entity.rs · c4471 exact]` | ~45 |
| `pending` | `[moved on u/l5np (not in this tree yet) → crates/b/x.rs]` | ~60 |
| `ambiguous` | `[ambiguous: 2 candidates → moirai links fix #F812]` | ~50 |
| `missing` | `[missing since c4470 (last src/x.rs) → moirai links fix #F812]` | ~60 |
| `absent-in-tree` | `[not in this worktree: behind trunk 365]` | ~40 |
| `deleted` | `[deleted c4480 by dev#2: "merged into y.rs" → y.rs]` | ~50 |
| `diverged` | `[branch says a.rs, tree has b.rs → moirai links fix]` | ~55 |

Packs add one header segment, `files: 38 ok · 2 moved · 1 missing` (~40 chars), and fold `absent-in-tree` into that count instead of per-link markers. The brief shows at most 3 link lines, plus `moirai links check` in the footer when more exist.

**[I] Token cost.** For a typical pack with 1–3 non-ok links: ~50–200 chars (15–50 tokens) plus a 40-char header. Skill additions: ~120 tokens (`file mv/rm/add/relink`, `links check/fix`, the rule "prefer `moirai file mv` for linked files; raw moves are reconciled later").

### 7.2 Example I/O

```
$ moirai links check --scope #88
branch: lane/l5np · rev 4471 · files @ <lanes-dir>/l5np (u/l5np 7c1e0a)
checked 41 links in 2.1 ms · 37 ok · 1 moved (applied) · 1 ambiguous · 1 missing · 1 absent-in-tree
#F812 moved     crates/a/x.rs -> crates/b/x.rs     oid exact · applied c4472 (lazy)
#F813 ambiguous docs/plan.md -> docs/archive/plan.md | docs/l5/plan.md   (similarity 0.97 / 0.95)
      → moirai links fix #F813 --to docs/archive/plan.md
#F820 missing   tests/quarantine_me.rs  since c4470 (last seen 3 min ago) · no candidate · grace
#F9   absent-in-tree  docs/measurements/2026-09-24/analysis.md  (tree behind trunk 25)

$ moirai file mv docs/l5 docs/archive/l5
branch: lane/l5np · intent i-19 · rename dir (1 op, 9 ms) · 14 links re-bound · 2 globs rewritten (files_owned #89, applies_to #212)
commit c4473 · Moirai-Move-Prefix: docs/l5/ -> docs/archive/l5/
hint: git add -A -- docs/l5 docs/archive/l5

$ moirai file mv crates/boyko_ecs crates/boyko_core
error[fs_busy]: MoveFileExW failed with 32 after 1.0 s (4 retries)
  directory rename blocked: a process has an open handle or its current directory inside crates/boyko_ecs
  (Restart Manager cannot name directory holders; common causes: a shell cd'ed inside, rust-analyzer, an editor watcher)
intent i-20 aborted; nothing changed · (exit 7)
```

### 7.3 MCP surface

No new tool is needed; this stays within the synthesis's ten [30 §7.2]:

- `get`/`pack` render file links with markers against the resolved tree (§5.1).
- `find` gains presets `links:broken` and `links:pending`.
- `write` gains record-only ops `link_file{node, path, anchor}` and `relink{file, to, evidence}`. The server validates that the target exists in the caller's tree (`ctx.cwd` from the stamp, or the lease's lane).
- Filesystem-changing verbs stay CLI-only (§3.1).

### 7.4 Conflicts and confidence surfaced to agents

`ambiguous` links are conflict-like records:

- They count in `stats` and `brief`.
- They take a node out of `ready` only when the node's `acceptance` names the file. This is opt-in per task; the default is informational.
- They resolve through `links fix`, which writes a `Resolve`-style commit with `Moirai-Relink: user`.

Thresholds come from [10] (exact → auto; ≥ 0.5 with a ≥ 0.2 margin → propose). The owner decides whether "propose" can ever auto-apply (§9 Q3).

---

## 8. Candidate end-to-end mechanisms

**E — Explicit-only.** `moirai file mv/rm/add/new/relink` with the §3.2 intent protocol. Links change only through these verbs. `links check` exists but only reports. No hooks.

**A — Auto-only.** No filesystem verbs. Links are re-bound by lazy reconciliation (§4.1) at SessionStart, pack and merge time, plus the optional accelerators (§4.2, §4.3). Deletion is inferred from git deletion commits.

**H — Hybrid (recommended).** E's verbs for intent, precision and directory/glob rewrites. A's lazy reconciliation for everything else. Per-tree resolution with the §5.3 write rule. Accelerators optional and off by default.

| Criterion | E (explicit-only) | A (auto-only) | **H (hybrid)** |
|---|---|---|---|
| Survives agent raw `mv` (148 of 185 moves [M]) | **no**, unless the agent remembers the verb | yes, at the next check (exact `oid`) | yes: immediately with the hook, else at the next check |
| Survives Python/script renames (63) | no | yes | yes |
| Survives git checkout/merge/apply/stash/reset | no (git moves files, moirai is not told) | yes (tree-aware; `absent-in-tree` for stale trees) | yes |
| Survives the owner moving files in Explorer | no | yes | yes |
| Survives move + heavy edit (> 50% churn) | **yes** (exact) | often no → `ambiguous`/`missing` [10] | yes if explicit, else as A |
| Captures intent (deleted vs lost, `replaced_by`) | **yes** | only via git deletion commits | yes for explicit; lazy never infers deletion |
| Directory moves + glob rewrite | exact | inferred (≥ 90% prefix rule) → sometimes `ambiguous` | exact when explicit, inferred otherwise |
| Quarantine-and-restore (42% of `mv`) | not affected | not affected if lazy; thrashes if eager | not affected (grace + monotone rule) |
| Crash safety | intent log + observation recovery | observational and idempotent | both |
| Idle CPU | 0 | 0 | 0 |
| Per-call overhead | none outside the verbs | stats on reads (1–3 ms/pack; ≤ 150 ms at SessionStart) + optional hooks | as A + verbs |
| Worktree correctness | re-binds land on the bound branch; nothing for unbound trees | per-tree resolution, `PENDING` for unbound | both |
| Merge (R1) | typed path merge (ops) | typed + resolution by observation | typed + `PathCompose` + observation |
| Git image (R3) | exact paths per commit | paths as last reconciled | as E for explicit, as A otherwise |
| Without git (R2) | full | reduced evidence (no rename chains or ancestry) but works | as A |
| Agent tokens | skill +~120; agents must comply (probabilistic [30 risk 7]) | markers only (15–50 tokens/pack) | both |
| Build size (est., Rust LOC excl. [10]'s matcher) | ~2–3k (verbs, intent, recovery, Windows errors, glob rewrite) | ~3–4k (resolver, caches, cascade, statuses, `absent-in-tree`, pending, hooks) + [10]'s matcher | ~5–7k + [10]'s matcher |
| Main failure mode | silent staleness whenever anyone moves a file another way; the 58 memory references that git's rename history repairs (§1.2) would have stayed dead | ambiguity after edits and splits; lost intent; wrong re-bind if thresholds are loose | complexity; two paths to test |

**Recommendation: H, built in slices that follow the synthesis roadmap [30 §9]:**

| Slice | R4 content |
|---|---|
| S1 (trunk graph + CLI) | artifact-as-file node, `at` edges with anchors, `file add`, `link --file`, stat check on `show`/`pack` (`ok` / `modified` / `missing` only) |
| S2 (packs/hooks) | SessionStart check within budget; `links check/fix`; markers; `file mv/rm/relink` with the intent protocol; the exact-evidence cascade (i, iv, v); glob rewrite on explicit directory moves |
| S3 (branches) | per-tree resolution; `absent-in-tree` via ancestry; `PENDING` for unbound trees; merge rules (`PathCompose`, observation resolution, monotone rule); the `links check --tree` line in the merge ritual; the git rename-chain source (iii) |
| S4 (image) | `.moi` for artifact nodes and anchored edges; relink trailers; import never touches the filesystem |
| Later | Claude `PostToolUse(mv/rm)` hook; owner-installed git blocks; `--deep` similarity search [10]; `file revert`; the optional watch mode in the M6 leader |

**Revisit triggers [I]:**

- If one campaign shows more than ~5% of lazily detected moves ending `ambiguous`, make the PreToolUse nudge default-on and consider a hard skill rule to use `file mv` for linked files.
- If SessionStart link checks exceed 150 ms at the owner's scale, check only brief-scoped links at SessionStart and the rest at pack time.

---

## 9. Questions only the owner can answer

1. **How much discipline for agents.** Should agents be *required* to use `moirai file mv/rm` for linked files, through a skill rule plus a non-blocking PreToolUse nudge? Or is lazy reconciliation acceptable as the primary path, with the verbs optional? The answer sets whether the nudge hook is on by default.
2. **Git hooks.** May moirai append its blocks to BoykoEngine's shared hooks directory (`<repo>\.git\hooks`, next to graphify's `post-checkout`/`post-commit`), installed by you? Or should there be no git hooks at all? Each block costs one `sh.exe` plus moirai spawn per commit.
3. **What may re-bind without asking.**
   - Option A: only exact evidence (identical content, or a git R100 rename) re-binds automatically, and everything else is proposed.
   - Option B: unique high-similarity matches ([10]'s ≥ 0.5 with a ≥ 0.2 margin, or git ≥ 90%) may also auto-apply, with a marker.
4. **Deletion.** Should a trunk commit that deletes a linked path mark the link `deleted` automatically, or only `moirai file rm` / `links fix --drop`?
5. **Git index.** Should `moirai file mv` stage the move (like `git mv`) by default, or never touch the index (recommended, because of the staged-deletion incident)?
6. **Scope of links.** Only files in the project repository? Or also the memory directory, scratchpads and other repositories? Each extra root needs a per-machine mapping and is never re-bound across machines.
7. **Unbound worktrees.** Must moves seen in `wf_*`, harness and scratch worktrees stay out of `main` until the code reaches trunk (recommended)? Or may they update `main` immediately, which is faster but wrong for the trunk tree until the merge?
8. **Paths written in prose.** Should path mentions inside note and rule bodies become links automatically, as `#N` mentions do? Or should only explicit links exist?

---

## 10. Risks and unverified items

| # | Risk / unverified | Status | Mitigation |
|---|---|---|---|
| 1 | Claude Code's `FileChanged` watcher may hold per-directory handles and block ancestor renames | unverified for Claude Code; mechanism [M] | do not use `watchPaths` beyond a handful of files |
| 2 | A wrong automatic re-bind (identical copies, template files, `mod.rs` ×87 basenames [12]) | inherent | auto only on a unique exact `oid` or a git R100 rename; basename never counts as evidence alone; monotone rule |
| 3 | Re-binds written to the wrong branch from unbound trees | design | §5.3 write rule; `PENDING` table |
| 4 | Window between the moirai merge and the git merge | design | `observed_git` ancestry → `pending`; monotone rule; `links check --tree` after the code merge |
| 5 | The owner's `--no-commit` merges and `git apply` fire no merge hook | [M] | `post-commit` + lazy |
| 6 | Directory moves blocked by agents' shells sitting in the directory (cwd) | [M] | bounded retry, explicit diagnosis, exit 7; nothing half-applied thanks to the intent record |
| 7 | Timing numbers taken at 100% CPU | [M] caveat | re-measure in S0 on an idle machine: stat/GetFileAttributesExW per path, rename p50/p99, hook spawn |
| 8 | `bashEditDiff` may improve (the field shape "may change" [D]) | [D] | keep it as an optional doorbell input; re-evaluate the `shared` rate after the harness changes |
| 9 | Case-only renames invisible on NTFS | [M] | record on-disk case after explicit ops; `links check` compares with the real case read back through directory enumeration |
| 10 | Path conflicts on merge when neither candidate exists in the merged tree | design | stays a conflict value; the pack shows base text; `links fix` |

---

## 11. Sources

**Primary documentation (fetched or read 2026-09-26):**

- Claude Code hooks reference, including events, `FileChanged`, `CwdChanged`, `WorktreeCreate/Remove`, `PostToolUse` Bash/PowerShell input, `tool_response.bashEditDiff`, `if`-field Bash matching, and "`cwd` follows Claude": https://code.claude.com/docs/en/hooks. A copy fetched on 2026-09-25 by report 07 was read locally; the event list was re-fetched today.
- Claude Code settings reference (`bashEditDiffEnabled`, scope user or managed): https://code.claude.com/docs/en/settings-reference
- Claude Code environment variables (`CLAUDE_CODE_BASH_EDIT_DIFF`): https://code.claude.com/docs/en/env-vars (local copy from 2026-09-25)
- githooks (post-checkout, post-merge, post-rewrite, reference-transaction, post-index-change; hook cwd): https://git-scm.com/docs/githooks
- git-merge (`merge.directoryRenames` default `conflict`, `merge.renameLimit`): https://git-scm.com/docs/git-merge. Local git 2.54 docs were used.
- git-diff `--find-renames` (default similarity 50%): https://git-scm.com/docs/git-diff
- gitformat-index (stat data and object name per entry): https://git-scm.com/docs/gitformat-index
- git-worktree (config shared across worktrees; `extensions.worktreeConfig`): https://git-scm.com/docs/git-worktree
- MoveFileExW (directories same drive; `COPY_ALLOWED` leaves the source if the delete fails; `REPLACE_EXISTING` errors on directories; `WRITE_THROUGH` flushes copy+delete moves): https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-movefileexw
- MoveFileTransactedW (TxF deprecation note; unsupported on ReFS/SMB): https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-movefiletransactedw
- RmGetList (lists processes using registered resources; directories → `ERROR_ACCESS_DENIED`): https://learn.microsoft.com/en-us/windows/win32/api/restartmanager/nf-restartmanager-rmgetlist
- W3C Web Annotation Data Model (TextQuoteSelector `exact`/`prefix`/`suffix`; TextPositionSelector "very brittle"): https://www.w3.org/TR/annotation-model/

**Precedents** (details in [12]):

- Fossil `mv` records the rename only, unless `--hard` or the `mv-rm-files` setting: https://www3.fossil-scm.org/home/help/mv, https://www3.fossil-scm.org/home/help?cmd=mv-rm-files
- Mercurial `rename --after` and `addremove -s` (similarity; default 100): https://repo.mercurial-scm.org/hg/help/addremove
- VS Code Markdown link update on file move, supported only for moves in the VS Code explorer: https://code.visualstudio.com/docs/languages/markdown, https://github.com/microsoft/vscode/issues/164522

**Internal:**

- Research reports: `docs/research/02` (orchestration, worktrees, hazards), `05` (spawn/flush/Defender), `07` (hooks, MCP), `08` (Windows hazards W1–W12, worktrees, git interop), `10` (content detection, fingerprints, anchors), `12` (precedents, NTFS ids, USN admin requirement).
- Design: `docs/research/design/30-synthesis.md` (§3 schema, §4.5 write path, §5a merge, §5b image, §5c git independence, §5d runtime state, §7 agent surface, §9 roadmap).

**Measurements (this session; the probe scripts are not published):**

| Script | What it measures |
|---|---|
| `scan_transcripts.py`, `scan_moves2.py`, `scan_gitops.py`, `scan_subst.py`, `scan_bed.py` | transcript census, aggregate counts only |
| `renames_all.txt` analysis, `linkrot.py`, `doc_anchors.txt` check | git history and link rot, read-only git |
| `wt.tsv` / `wt_div.tsv` | worktree divergence |
| `probe_sharing.py`, `probe_timing.py` | Windows move semantics and timing |
| `probe_githooks.sh` and the `--no-commit`/`apply` probe | git hook firing matrix, in throw-away repositories |
| `walkidx.py`, `rsprobe` | stat/index/walk costs |
| Claude Code `Write`/`Edit` file-id probe | tool file-replacement behaviour |

Every probe repository and sandbox was deleted after use, and so were the intermediate data files: tree listings, the rename log, the anchor list and command samples. Only the small scripts and count outputs remain (~100 KB).

*End of report 13.*
