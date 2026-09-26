# 40 — File-link subsystem design (R4)

*moirai design, 2026-09-26, revision 2. Status: design only. Nothing is implemented, and this file is the only repository file written. It turns the five R4 research reports [09]–[13] into one subsystem that fits the design of record [AR] (`docs/ARCHITECTURE-RESEARCH.md`) and the roadmap of record [60] (`docs/research/design/60-roadmap.md`). It follows the owner decisions of 2026-09-26: moirai's own engine from the start; no SQLite or other third-party embedded database; early adoption not a goal; no interim or throwaway stages; every component built to its final specification; milestones ordered by technical dependency. Revision 2 applies the adversarial review [41]. Each of its 4 blockers, 12 major issues and 18 minor issues is resolved, or rejected with a reason, in the Review log at the end. Where this design departs from a report or from [41], it says so and gives the reason. On 2026-09-26 this design was integrated into [AR] (as [AR] §5e and the sections it touches) and reconciled with [50] and [60]; the few edits that integration made here are listed at the end of the Review log, and this file stays normative for R4's detail. Amended again on 2026-09-26 by the final editorial pass over the priority audits [70]–[74] (speed, RAM, correctness, tokens, feasibility and configuration): the Review log's priority-audit entry lists every change. Amended on 2026-09-26 for owner decision #32 by [80] (`design/80-cross-platform-design.md`, revision 2, which answers its review [81]): the file-identity providers, tagged runtime layouts, path rules and per-OS resolver rules below hold for Windows, Linux and macOS; Windows is built in M6, and Linux and macOS in the unscheduled port phase (Review log). Amended on 2026-09-26 for the owner's answers to [AR §11]: every R4 owner decision of §9.2 is decided as recommended, `links import` is built and run, and the replay corpora stay out of the public repository (Review log, last entry).*

**Evidence tags.** **[M]** measured on the owner's machine, either in the cited report or by the probes in §0.3. **[D]** documented by a vendor, a specification or source code. **[C]** claimed by a third party. **[I]** inference or design decision. **est.** arithmetic whose inputs are shown. Untagged statements are design decisions.

**Sources used throughout.**

| Tag | Document |
|---|---|
| [AR] | `docs/ARCHITECTURE-RESEARCH.md`, the design of record: §3 data model, §4 storage, §5 version control and git image, §7 agent interface |
| [60] | `docs/research/design/60-roadmap.md`, the roadmap of record (M0–M11, dependency-ordered, no interim stages) |
| [41] | `docs/research/design/41-file-links-critique.md`, the review this revision answers |
| [50] | `docs/research/design/50-query-language-design.md` (R5), for the `AT` pattern and the `link_state()` built-in (§6.5 is the R4 part of that language) |
| [09] | `docs/research/09-file-identity-os-level.md`: OS identity signals, USN journal, cost of reading identities |
| [10] | `docs/research/10-content-based-move-detection.md`: fingerprints, similarity, rename history, resolution cascade |
| [11] | `docs/research/11-in-file-anchors.md`: selectors, citation survival, capture uniqueness, resolution cost |
| [12] | `docs/research/12-precedents-link-maintenance.md`: how other tools keep links alive |
| [13] | `docs/research/13-file-ops-agent-integration.md`: transcript census, worktrees, Windows move semantics, hooks, the hybrid mechanism |
| [14], [16] | query-language reports (R5), used for built-ins (§6.5) and the Windows shell transport rules |
| [30] | `docs/research/design/30-synthesis.md`, superseded by [AR]; cited only where [AR] points to it |
| [60d] | `docs/research/design/60-engine-first-roadmap-draft1-adoption-driven.md`, superseded by [60]; no longer used |
| [70]–[74] | `docs/research/design/70-audit-speed.md` … `74-audit-feasibility-config.md`, the priority audits whose R4 findings this file applies (Review log, last entry) |
| [80] | [80-cross-platform-design.md](80-cross-platform-design.md): the cross-platform design (owner decision #32), normative for the OS layer, the per-OS R4 rules (§2.10–§2.11) and the items frozen at M0 |
| [81] | [81-cross-platform-critique.md](81-cross-platform-critique.md): the adversarial review of [80]'s first revision |
| [X17] | [../17-xplat-durability-mmap-memory.md](../17-xplat-durability-mmap-memory.md): cross-platform durability, mappings and memory |
| [X18] | [../18-xplat-locking-ipc-processes.md](../18-xplat-locking-ipc-processes.md): cross-platform locking, IPC and processes |
| [X19] | [../19-xplat-file-identity-change-tracking.md](../19-xplat-file-identity-change-tracking.md): cross-platform file identity and change tracking (R4) |
| [X20] | [../20-xplat-toolchain-shells-ci-crash-testing.md](../20-xplat-toolchain-shells-ci-crash-testing.md): cross-platform toolchains, shells, CI and crash testing |

**Shell rule for every example.** Ids are written bare (`812`), and anchor ids are written `a17`. At the start of an unquoted token, `#` begins a comment in Git Bash and in PowerShell 5.1, and `@` is the splatting operator in PowerShell 5.1 [M, 16 §6.10; re-measured in §0.3]. File arguments never start with `/`, because Git Bash rewrites them to `C:/Program Files/Git/…` [M, 16 §6.10]. Text goes in through `-f PATH`, `--quote-file PATH` or `--stdin`, never `@file`.

---

## 0. Summary

### 0.1 The design in sixteen decisions

1. **Hybrid mechanism.** Explicit file verbs record intent precisely. Lazy, deterministic re-binding catches everything else: agents' raw `mv`, Python renames, git checkout/merge/apply/stash, and the owner in Explorer. There is no watcher, no daemon, no timer and no polling. Explicit verbs alone cannot work. Agents made 148 raw `mv` calls against 7 `git mv` calls and 63 Python renames [M, 13 §1.1]. Tree-rewriting git operations (`apply` ×189, `merge` ×85, `switch` ×45, `checkout <ref>` ×27, `stash` push/pop ×43, …) outnumber explicit move commands about 3:1, and any of them can move a linked file, although only 29 of 1,886 commits contain renames [M, 13 §1.1–§1.2]. Every tool that rewrites links only on its own moves has the same gap [D, 12 §0.1].
2. **A link is an `at` edge from any node to a file node.** The file node is the widened `artifact` kind. The edge carries one or more **anchors**: the whole file, a heading, a symbol, a quote, a quote range, or, as a last resort, a line window.
3. **Two layers.** *Link intent* is versioned per branch, merged and exported: root-relative path, content id, anchors, removal intent, move provenance, and the history of directory moves. *Resolution* is runtime and tree-relative, and it is never versioned or exported: link states, OS file ids, stat caches, evidence and proposals. **Directory-move history is versioned state** (the `path_moves` set on a per-root node), never a commit event (§2.4).
4. **File-node identity is derived from exact bytes, and a merge never resurrects it.** uid = BLAKE3 over (root, the registration path exactly as git's HEAD tree or the directory enumeration spells it, the predecessor at that path). The predecessor is chosen by commit (generation, id) among removed, engine-deleted and aliased nodes. Two lanes that link the same file create the same node. A node created on one side of a merge while the other side removed it is re-keyed deterministically, never revived (§2.3, §5.5).
5. **Content identity is moirai's own `oid`**: a git-blob-shaped hash over bytes normalised with git's own binary test. Git evidence always uses git's blob ids read from git trees (`observed_blob`); correctness never assumes `oid` equals a git blob id.
6. **The OS file id is a per-tree hint, never identity.** Every Claude Code `Edit`/`Write`, every atomic save and every git rewrite of a changed file gives the file a new id [M, 09 §3]. The id of the file's **parent directory** survives a rename or move of any ancestor directory [M, §0.3]. One `OpenFileById` therefore resolves every linked file under a moved directory.
7. **Reads never write.** `show`, `pack`, `brief`, `get`, `links check` and queries compute link states live. A moved or vanished file is visible to every referrer at its next read, without any write; that is how the "node 40" rule applies to files. Versioned re-binds are written only at **settle points**, after a ≥ 50 ms quiescence re-check that drops re-binds whose source path came back (§4.2).
8. **Only exact evidence re-binds automatically, and equal content is not exact by itself.** An equal-`oid` candidate is exact only when a same-volume creation time, a git rename inside one commit, or captured intent corroborates it. A candidate that coexisted with the original is a copy and never a target. Deletion is never inferred from absence. A path reused by unrelated content is flagged `replaced` (§4.4).
9. **Anchors are selector bundles captured from what agents already write** (`path:L-M`, `path::Type/fn`, `path#Heading`). A bundle holds a quote with context, a ±16-line context window, a scope and a position hint. A bare line number is never an identity. An anchor uid is derived from a length-prefixed capture digest that is stored with it (§2.7).
10. **Resolution is per tree, and the tree's own committed content is the first gate.** An in-process read of git's HEAD tree decides "this tree moved it", "this tree has not received the move" and "this tree is behind" before any search; ancestry comes second (§5.2). **Only a branch's single designated tree, with HEAD on the lane's git line, writes re-binds.** Every other tree is a reader, and its observations stay `pending` (§5.3).
11. **The in-process git object reader is a hard dependency** ([60] M4): ancestry over split commit-graph chains, HEAD-tree lookups and per-commit exact renames. R4 never spawns a git process.
12. **Merges stay pure functions of history.** Link fields merge by typed rules. Directory moves compose through the versioned `path_moves` state. Path disagreements land as conflict values; a later settle resolves them by observation only in a tree that has received every side (§5.5).
13. **R3.** File nodes are `.moi` files. Anchors are `anchor` lines in the referrer's `.moi`. `path_moves` is an ordinary field of the root node. R4 adds no trailer and no commit annotation. Machine-local data (file ids, volume serials, mtimes, stat caches) is never hashed or exported.
14. **R2.** Everything works without git. Git only adds evidence: HEAD trees, per-commit renames and ancestry.
15. **Accelerators are optional, and correctness never depends on them.** They are: a harness `PostToolUse` evidence hook for moves and removals (Claude Code: `mv`/`rm`/`Move-Item`/`Rename-Item`/`Remove-Item` matchers; Codex: every shell call filtered in-process by an `mcp_tool` handler, plus `apply_patch`'s `*** Move to:` lines; [90 §3.2]); the `Write|Edit` evidence hook (spawn-free when hooks run as `mcp_tool` handlers); and owner-installed git hook blocks. USN-journal replay is not built, because D: has no journal [M, 09 §4.2] and C:'s keeps 1–2 h ([74 A13]). Cloud-only placeholders (OneDrive) are never hydrated by an automatic path (§4.6).
16. **Costs.** Link rendering adds ≤ 3 ms to a pack of 50 links when all are present, and about 3 ms (a ≤ 5 ms p50 gate) when all 50 sit in one moved directory. SessionStart settles within a hard 150 ms budget. Idle CPU is 0. moirai never holds, maps, locks or writes metadata to project files.

### 0.2 The guarantee to the owner

- **Moves and deletions made through moirai** in a bound tree land in one commit. Every referrer sees them on its next read, and the deletion intent (`reason`, `replaced_by`) is recorded. In a tree that is not bound to a branch, `file mv` and `file rm` refuse (exit 5) and print the bind command. They never record intent that a crash, a `gc` or an image transport could lose.
- **Moves made by anything else** show correctly at the next read of any referrer: as `moved-auto` when the evidence is exact, otherwise as a proposal, `ambiguous` or `missing`. They are recorded as versioned re-binds at the next settle point in the branch's designated tree. Only exact evidence re-binds without a human or agent decision.
- **No link ever points silently at the wrong file or the wrong text.** Equal content alone never re-binds. A printed fix never accepts a guess. A guess accepted by an agent stays marked `[accepted guess]` until another role or the owner confirms it. Every link that is not `ok` carries a state and a one-line next step.
- **A deletion made by something else** leaves the link `missing`, with its last path, forever visible until someone decides. No edge is dropped. A path reused by unrelated content shows `replaced`.
- **A tree that has not received a move says so.** Only a tree that is strictly behind the observation folds such links into a header count. Every other unresolved link (diverged lines, patch-integrated history, old `pending` rows) is marked per link.

### 0.3 Probes [M]

Probes were run for revision 1 and for this revision (probe scripts are not published). Repositories and transcripts were only read. Scratch files were created inside the probe directory and deleted.

**`argv.py`** prints `sys.argv`. It was run under Git Bash (Git for Windows 2.54) and Windows PowerShell 5.1.26100.9444:

| Token (unquoted) | Git Bash receives | PowerShell 5.1 receives |
|---|---|---|
| `#812 tail` | nothing (comment) | nothing (comment) |
| `@a17` | `@a17` | **nothing** (splat of an undefined variable) |
| `a17`, `812` | intact | intact |
| `docs/plan.md#Storage` | intact | intact |
| `crates/x.rs::Foo/bar`, `crates/x.rs:10-20`, `x.rs@c4410:12` | intact | intact |
| `src/{a}.rs` | not probed | `src/` (brace block) |
| `'crates/x.rs::impl Foo/bar'` (quoted, with a space) | intact | intact |

Consequences: anchor handles are the bare `a17`, never `@a17`; `#` is allowed *inside* a token (`path#Heading`); a scope that contains spaces must be quoted.

**`dirid.py`** (NTFS on C:, non-elevated, `FileIdInfo` 128-bit ids):

| Operation | Result |
|---|---|
| rename a directory in place | directory id kept |
| move a directory under another parent (same volume) | directory id kept; the ids of its child directory and child file kept |
| `OpenFileById(directory id)` + `GetFinalPathNameByHandleW` after the move | resolves to the new path; 0.17 / 0.19 / 0.40 ms (min / p50 / max, n = 20) |
| copy the directory, delete the original, then `OpenFileById(old id)` | fails (error 87): a copy gets new ids |

**`editmove.py`** is a census over the 4,052 transcripts of the owner's BoykoEngine sessions (3.3 GB; [13 §1.1] scanned 4,044 of them). It merges each session's main and subagent transcripts in timestamp order and parses `mv`/`git mv` segments. It prints aggregate counts only:

| Quantity | Count |
|---|---|
| move items (one per source argument) | 197 (193 `mv`, 4 `git mv`) |
| unresolvable (`$VAR`, glob or brace arguments) | 76 |
| resolved source paths | 121 |
| … edited by Claude Code `Edit`/`Write` earlier in the same session | **6 (5.0 %)** |
| … directories holding a file edited earlier in the session | 0 |
| … not edited earlier in the session | 115 |
| resolved move items whose shell call continues with `sed -i`/`perl -pi` | 10 |

So edit-then-move, the case in which a Claude Code edit has made the stored file id stale before the move (§4.6), is about 5 % of resolvable moves in this workflow. Moves of *linked* files may differ, and the rule in §9.2 decision 4 keeps it under observation.

**Probes of [41] used here:**
- the commit-graph chain covers 1,796 of 1,889 commits; the HEADs of trunk and of all four active lanes are not in it, and 7 of 46 worktree HEADs are uncovered;
- 4 of 50 trunk first-parent deletions (34 of 283 on all refs) were later re-added at the same path;
- with `core.ignorecase=true`, `mv a.rs A.rs` leaves git's index, HEAD and new worktrees at `a.rs`;
- `mv` keeps a file's creation time; `cp` and `cp -p` do not, and `cp -p` keeps the mtime;
- trunk's first-parent line has 675 non-merge commits against 47 merges, and 132 commits are not reachable from trunk;
- OneDrive is present on the machine.

The repository now has **46 worktrees** [M, 41 §1]; [13] measured 44, and its per-group counts below are cited as measured then.

---

## 1. Requirements

### 1.1 R4, restated precisely

- **R4.1 Reference.** A node can reference a project file as a whole, or a location inside it: a line span, a symbol, a Markdown section or a quoted passage.
- **R4.2 Survival.** A reference survives:
  - rename, move, directory move and case-only rename;
  - edits of the file's content;
  - all of the above, whoever performs them: moirai, the owner (Explorer, an IDE), an agent (Bash `mv`, PowerShell `Move-Item`, Python `os.rename`, Claude Code `Write`/`Edit`), or git (`mv`, `checkout`, `switch`, `merge`, `rebase`, `stash`, `reset`, `restore`, `apply`, `cherry-pick`).
- **R4.3 Deletion.** When a referenced file is deleted, every referrer knows. When moirai performs the deletion, it records the intent: on purpose, with a reason, and optionally replaced by another file. When something else deletes the file, referrers see it as missing. The edge is never dropped silently.
- **R4.4 Mechanisms.** The owner accepts explicit moirai file commands, automatic re-binding, or both. This design uses both (§0.1 decision 1).

### 1.2 Definitions

| Term | Meaning |
|---|---|
| **root** | a named base directory for paths: `project` (the current tree), a configured named root such as `memory`, or `abs` (§2.4) |
| **tree** | one working copy of a project root, identified by its exact git top-level (a nested `.claude/worktrees/*` checkout is its own tree), or a directory bound with `worktree bind` when there is no git (R2). The owner has 46 worktrees of one repository [M, 41 §1] |
| **file node** | an `artifact` node (§2.2): the identity of one logical file across moves and edits |
| **root node** | the per-root `area` node that holds the root's `path_moves` history (§2.4) |
| **link to a file** | an `at` edge from a node to a file node that carries a `file` anchor |
| **link into a file** | an `at` edge that carries a span anchor (§2.7) |
| **τ(C)** | the committed tree of git commit C, read in process through the git object reader |
| **resolution** | answering "where is this file and this span in tree T now, and in what state" (§4). It is a pure function of the versioned link, the tree snapshot (files and git objects) and the resolver version |
| **settle point** | a command that may write versioned re-binds (§4.2). Reads never do |
| **evidence** | a reason to believe the file now lives at path q. It is **exact**, **strong** or **weak** (§4.4) |
| **designated tree** | the single tree bound to a moirai branch: `lane.worktree_path` for a lane, `files.main-tree` for `main`, or one `worktree bind` (§5.1) |
| **writer tree** | a designated tree whose HEAD is on the branch's git line (§5.3). Only writer trees write re-binds |
| **reader tree** | any other eligible tree: it resolves and displays, and its observations go to `PENDING` |
| **fresh** | tree T is fresh for file node F when T has received the observation F's current value rests on (§5.3) |

### 1.3 Derived requirements

| # | Requirement | Source |
|---|---|---|
| DR1 | **No silent wrong binding.** A link never points at a different file, or a different passage, without a visible non-`ok` state. A wrong link is worse than a broken one | [11 §1], [12 L9], [10 §7], [41 B3] |
| DR2 | **Loud failure.** Every unresolved link is listed in packs, in `links check` and in merge-check. A glob that matches nothing is flagged. Only "this tree is strictly behind" may be folded into a count | [12 L15], [41 M3] |
| DR3 | **Zero idle CPU.** No watcher, daemon, timer or polling. Work happens only inside foreground commands and hooks | [AR §6.6, §12] |
| DR4 | **Windows, Linux and macOS; non-admin on all three.** One resolver semantics fed by per-OS evidence providers and a per-volume capability record ([80 §2.11]); Windows built in M6, Linux and macOS in the unscheduled port phase (owner decision #32) | [09 §6], [X19], [80] |
| DR5 | **R1.** Link intent branches and merges. Resolution is per tree. A file-location fact is never written to a branch from a tree that is not that branch's writer tree | [13 §5.3], [41 M2] |
| DR6 | **R2.** Every function works without git. Git only adds evidence | [AR §5c] |
| DR7 | **R3.** Link intent round-trips through the image at every granularity. No machine-local datum is hashed or exported. Two stores that hold the same history compute the same link state and compose the same merges | [AR §5b.5 rule 7, §5b.7 gate 3], [41 B1, B4] |
| DR8 | **Determinism.** Resolution is a pure function of (link, tree snapshot, resolver version). A merge is a pure function of the histories. Identity derivation uses no machine-local input | [AR I28′], [11 §4.3], [41 B1] |
| DR9 | **Hands off the user's files.** By default moirai does not write IDs into files, does not create NTFS object IDs or alternate data streams, does not rewrite text, and does not hydrate cloud placeholders. It never keeps a project file open or mapped beyond one read | [12 §4.11], [13 §1.4], [41 M8] |
| DR10 | **Performance.** Link rendering adds ≤ 3 ms to a 50-link pack. The engine budgets of [AR §8] are unchanged. No R4 path spawns a process | [AR §8.1], [41 B2] |
| DR11 | **Agents write what they already write** (`path:L-M`, `path::Type/fn`, `path#Heading`). A bare basename that matches several files is refused at write time | [11 §2.1]: 38.1 % bare basenames, 15.9 % already ambiguous when written |
| DR12 | **"Node 40" for files.** An explicit removal is one commit that reaches every referrer. A detected disappearance is visible to every referrer at its next read | [AR §5d.3] |
| DR13 | **No interim modes.** Every rule of this design is built in the milestone that first needs it, to its final specification; no rule has a temporary substitute | owner decision 2026-09-26, [60 §1.1], [41 M9] |

### 1.4 Which patterns are handled how

The frequencies come from the owner's own history and transcripts. They are cited as workflow facts, not as project content.

**Handled automatically on exact evidence, with no confirmation (state `moved-auto` or `ok`):**

| Pattern | Frequency / evidence | Exact evidence used (§4.3) |
|---|---|---|
| Pure rename or move of a file within one tree and one volume (Explorer, `mv`, `Move-Item`, `git mv`, `os.rename`) whose file id this tree has recorded since its last edit | 186 of 201 git renames were 100 % similar [M, 13 §1.2]; 148 raw `mv`, 7 `git mv` and 63 Python renames in 121,571 shell calls [M, 13 §1.1] | file id with unchanged size and mtime (E3) |
| Directory rename or move, including bulk moves (one commit moved 131 docs into an archive directory) | [M, 12 §3.5; 13 §1.2] | the parent directory's id (E3d): one `OpenFileById` per moved directory [M, §0.3] |
| Edit at the same path by any writer, including replace-by-rename (Claude Code `Edit`/`Write`, `sed -i`, IDE safe-write) and git rewrites | every agent edit changes the file id [M, 09 §3.1] | not a move: path present → `ok`, and anchors re-resolve |
| A committed move that reaches the tree through git (`checkout`, `switch`, `merge`, `rebase`, `cherry-pick`, `apply` then commit) | the file id is lost [M, 09 §3.2]; no hook fires for `apply`, `reset` or `--no-commit` merges [M, 13 §1.5] | per-commit exact renames from git trees (E6), read in process |
| Case-only rename committed in git (`git mv -f a.rs A.rs`) | 0 in history [M, 13 §1.2] | HEAD's tree spelling (§2.4) |
| Move then edit, or edit then move, when an accelerator captured the move | — | hook-captured file-id chain (E1; the edit-evidence hook keeps the id current, §4.7) |
| Quarantine and restore (move aside, run, move back) | 42 % of agent `mv` calls target temp, scratch or `target`-like paths [M, 13 §1.1] | nothing is written: reads show live state, the quiescence re-check drops a settle that races the restore, and settle points see the file back in place |
| This tree is strictly behind the observation (36 stale worktrees lag trunk by 272–1,036 commits) | [M, 13 §1.3] | HEAD-tree gate and ancestry → `absent-in-tree`, with no search |

**Proposed, with confirmation (state `moved-needs-confirm`, `ambiguous`, `stale-anchor` or `replaced`):**

| Pattern | Why it is not automatic | Evidence |
|---|---|---|
| Edit then move of an agent-edited file with no captured intent and no settle in between | the stored file id is dead and the content changed. Measured: 6 of 121 resolvable moved paths (5.0 %) had been edited earlier in the same session [M, §0.3] | `moved-needs-confirm (edited+moved)` from a same-basename new file with containment ≥ 0.8 (E8) |
| Copy + delete, or a move across volumes inside the root, with no git, journal or hook corroboration | an equal-content file could be a pre-existing copy (a `.bak`, a mirror) [41 B3] | `moved-needs-confirm (identical copy)` |
| Move plus heavy edit with no captured intent | similarity is a guess. Git's default 50 % threshold misses 7 of 16 real inexact renames [M, 10 §5.7] | `moved-needs-confirm` when similarity ≥ 0.5 with a margin ≥ 0.2 (`--deep`) |
| Rust module split (`foo.rs` → `foo/mod.rs` plus siblings) | there is no single target; 6 of 16 inexact renames were splits [M, 10 §5.7] | `moved-needs-confirm (split)`, detected at settle from the git commit (§4.4); span anchors resolve piece by piece |
| Merge into a host file | the host file is a different file | `moved-needs-confirm (merged)` |
| Several identical candidates, such as mirrors (1.2 % of files are in exact-duplicate groups [M, 10 §5.1]) | a choice between copies is a guess | `ambiguous`, candidates listed |
| Rename-over (`mv b a`) or a swap | two readings are possible | `ambiguous (rename-over / swap)` |
| In-file edits that destroy or duplicate the cited text | the claim may be stale | `stale-anchor (edited / orphaned / ambiguous)` |
| A path reused by unrelated content (deleted, later re-created; or rewritten wholesale) | rare: 4 of 50 trunk first-parent deletions were later re-added [M, 41 §1], but silent if unflagged | `replaced` (§4.4) |
| A deletion done by someone else (missing path, git deletion commit, journal `DELETE`) | absence is not intent [13 §4.1] | `missing`; one command (`links fix … --drop`) records it |
| A case-only rename on disk that git has not recorded | git keeps the old spelling under `core.ignorecase=true` [M, 41 §1] | `ok` with the detail `spelling differs on disk`; recorded only through `file mv` or a commit |

**Out of scope, deliberately:**
- Re-binding files outside every configured root: an `abs` root gets an existence check and an `oid` check only.
- Following a file into a different repository or onto another machine. It travels only as a path, through the image.
- A file whose content was rewritten beyond recognition and moved, with no captured intent. It stays `missing` with candidates, and a human or agent decides.
- Rewriting textual path mentions inside repository files. moirai reports them (`links mentions`), following VS Code's default of "never" for Markdown [D, 12 §4.2].
- Semantic code tracking, such as a symbol moved to another crate *and* renamed. Quote search and scope search catch many of these cases; language servers remain the tool for the rest.
- Gitignored build output as an automatic re-bind target.
- Hydrating cloud-only placeholders in any automatic path.

---

## 2. Data model

### 2.1 Two layers

| Layer | Contents | Versioned per branch, merged | Hashed and exported in the image |
|---|---|---|---|
| **Link intent** | file nodes (§2.2); root nodes with `path_moves` (§2.4); `at` edges with anchors (§2.7); removal intent | yes | yes |
| **Resolution and evidence** | per-tree states, OS file ids and parent-directory ids, volume serials, the stat quadruple, the latest observed `oid`, proposals, pending observations, intent records for file operations, fingerprints, USN cursors, git-derived fact caches, tree bindings' observational parts (§2.6) | no (store-level runtime) | no |

Git versions the files and moirai versions the links [13 §2.1]. Where a file sits in one working copy is a fact about that working copy, and merging it as data would duplicate git. Which file and which passage a note is about is knowledge, and it must branch, merge and travel. The history of directory moves is also knowledge: merges and old mentions depend on it, so it is versioned state (§2.4), never an event attached to a commit [41 B4].

### 2.2 The file node: the widened `artifact` kind

[AR §3.2] defines `artifact` as a "pointer to a file outside the store" (`path`, `sha256`, `bytes`, `artifact_kind`, `excerpt`). This design keeps the kind count at 13 and makes `artifact` the single file-reference kind [13 §2.2]. Its `sha256` field is replaced by `oid`, and [AR] I14's "sha256 read back" becomes "oid read back".

| Field | Type | Merge class (§5.5) | Notes |
|---|---|---|---|
| `uid` | u128, **derived** (§2.3) | identity | the same logical file on every branch and in every store |
| `origin_path` | `path` | identity (immutable) | the registration path, exact bytes (§2.3) |
| `origin_pred` | u128 or absent | identity (immutable) | the predecessor uid used in the derivation; absent when there was none |
| `root` | sym | scalar | `project`, a named root, or `abs` (§2.4) |
| `path` | `path` (§2.11 R-1) | **observation composite** | root-relative, `/` separators, exact bytes, UTF-8 |
| `oid` | `oid` {algo, 20 or 32 B} | observation composite | moirai content id when the path was last set (§2.5). Content drift at the same path is tracked in runtime (`FILEOBS.last_oid`) and is not written on every edit |
| `bytes` | u64 | observation composite | raw size at that observation |
| `observed_git` | `oid` (git commit) or empty | observation composite | HEAD of the observing tree when the path was set |
| `observed_blob` | `oid` (git blob) or empty | observation composite | git's own blob id at `path` in τ(`observed_git`); empty when the path was not committed there (an **uncommitted observation**) or without git. It replaces revision 1's `observed_dirty` flag, which it implies |
| `relink` | text `how/evidence[/score]` | observation composite | provenance of the current path (§2.11 R-17): `explicit/intent`, `explicit/intent-recovered`, `lazy/file-id`, `lazy/dir-id`, `lazy/oid+ctime`, `git/r100`, `git/case`, `hook/file-id`, `journal/usn`, `owner/manual`, `owner/similarity/0.81`, `agent/manual`, `agent/similarity/0.81`, `confirmed/similarity/0.81`, `merge-observation/<evidence>`, `merge-compose/prefix` |
| `aliases` | set<path> | add-wins set | former paths; they resolve old mentions and old images. An alias never captures a new file |
| `artifact_kind` | enum | scalar | adds `source`, `doc`, `asset`, `generated`, `dir` to the run-output kinds of [AR] |
| `status` | `planned` < `present`; side state `removed` | status lattice | `planned` = linked before it exists; `removed` = deleted on purpose (§3.5) |
| `reason`, `replaced_by` | text, ref | scalar | set by `file rm` and by `links fix --drop` |
| `excerpt` | text | scalar | kept for run outputs |
| `title` | derived from `path` | — | not stored for artifacts; this saves about 45 B per node [M, 10 §5.1: mean path 45.3 B] |

The six observation fields (`path`, `oid`, `bytes`, `observed_git`, `observed_blob`, `relink`) form **one composite merge key** (§5.5). A move changes the path, the provenance and usually the content id together, so they must merge together.

### 2.3 Identity: derived uids

```
lp(x)          = u32-le(len x) ‖ x                                   (every field length-prefixed)
uid(file node) = BLAKE3-128( lp("moirai-file-v1") ‖ lp(root name) ‖ lp(origin_path) ‖ lp(origin_pred or empty) )
uid(root node) = BLAKE3-128( lp("moirai-root-v1") ‖ lp(root name) )
```

- **`origin_path` is exact bytes, never folded.** It is the path as git's HEAD tree spells it when git tracks the file (found by a case-insensitive lookup on a case-insensitive directory), otherwise as the directory enumeration returns it. Case-insensitive collision detection belongs to `PATHIDX` and to resolution (§2.4), never to identity. So the same file at the same committed path gets the same uid on Windows, on Linux, in a case-sensitive directory and in another store [41 B1].
- **The predecessor** is chosen on the registering branch view V, among the file nodes that once held `origin_path` but do not hold it now: nodes with status `removed` at that path, engine-deleted nodes (tombstones) at that path, and live nodes whose `aliases` contain it. The winner is the node whose last change of path, status or existence was made by the commit with the greatest **(generation, commit id)**. Both are store-independent ([AR §4.3, §5a.1]); revision 1's store-local `rev_seq` is gone. The predecessor is empty when there is none.
- **Dead uids are never re-created.** If the uid computed on V is known to the store as removed or deleted on some branch head and live on none, registration re-derives with that uid as the predecessor, and repeats until the result is not known as dead. The merge rule of §5.5 re-keys a dual creation against a removal to the **same** uid (`uid′ = uid(root, origin_path, U)`), so a store that registered before learning of the removal and a store that registered after it converge.
- **Why derived.** Lanes often cite the same file concurrently. With random uids, every such pair would become two nodes at merge and would need unifying. With derived uids, both lanes create the same uid, and the merge sees one node created on both sides (§5.5).
- **`#N` reuse.** Creating a file node whose uid the store already knows, on any branch, reuses that uid's `#N`. That keeps [AR] I1 (a `#N` binds to at most one uid, store-wide) intact and makes `#812` mean the same file in every lane. A re-keyed node gets a new `#N`; the old one stays with the removed node.
- **`created` of a node created on both sides** is the creating commit with the least (generation, commit id). `.moi` carries one `created` commit, so the rule must be fixed [41 B1].
- **Exceptions to [AR].** The import rule `IdCollision` ("a uid live with a different `created` commit", [AR §5b.6 step 4]) does not apply to derived-uid kinds. For them, a Create of an existing uid is equal existence plus a field merge.
- **Verification.** The derivation inputs are stored (`origin_path`, `origin_pred`), so `doctor --verify` and image import can recompute every file uid. A foreign node whose uid does not match is accepted as foreign, flagged in `image doctor`, and treated as random.
- **The residual case.** One file can be registered at *different* paths on two branches whose trees were at different git commits. This yields two nodes for one file. The cascade reports it (§4.3: the candidate path is bound to another node with the same `oid`), and `links fix A --same-as B` unifies the two: A becomes `removed{reason: same-as, replaced_by: B}` and its anchors are re-pointed, in one commit. When git shows an exact rename from A's path to B's inside one commit of the writer tree's history, settle performs the unification itself (evidence `git/r100`).
- **Known limit.** Two lanes that register *unrelated* new files at the same new path create one node, exactly as git sees one path; git's own add/add conflict decides the content, and §4.4's `replaced` check flags the node if the merged content is unrelated to either observation.

Anchor uids are also derived (§2.7), so identical captures made on two lanes merge into one anchor.

### 2.4 Paths, roots, directories and globs

**Roots.**

| Root | Meaning | Re-binding | Per-machine config |
|---|---|---|---|
| `project` | the tree: a git worktree top-level, or the bound directory without git | yes, per tree | none (paths are relative) |
| named (`memory`, `notes`, …) | a directory configured per machine in `config roots.<name>` | yes, inside that root, with the same cascade | yes; an unmapped root renders `unmapped root` |
| `abs` | a machine-local absolute path (`lane.worktree_path`, `run.script_path`, `run.journal_path`) in the form of [80] P12: `/` separators; on Windows `X:/…` with the drive letter upper-cased; on Linux and macOS with its leading `/` | no; existence and `oid` checks only; never compared across machines or OSes (a path that does not exist here renders `missing`, with no candidate search) | n/a |

Session scratchpads are refused as link targets by default. They are session-scoped and grow to GBs [02 §8.3 via AR]. A run output there should be stored as an `artifact` with an inline body (§9.2 decision 2).

**Path rules (invariant I-F8).**
- A stored path is root-relative with `/` separators. It has no empty, `.` or `..` segment and no leading `/`. The one exception is root `abs`, which stores a machine-local absolute path in the form of [80] P12 (on Windows `X:/…` with the drive letter upper-cased; on Linux and macOS the leading `/` is kept). It is valid UTF-8, stored as exact bytes (git's HEAD spelling when tracked, else the enumerated spelling, precomposed to NFC on a normalization-insensitive macOS volume as git records it — [80 §2.10] P3), with no other Unicode normalisation (git's rule [13 §2.4]).
- Refused at link time on every OS, with a clear error: a component containing `\` or a C0 control character, a Linux name that is not UTF-8, a Windows name with an unpaired surrogate ([80] P4). Such a path already in git renders `unrepresentable path` and is never a candidate.
- Paths longer than 260 characters are handled through `\\?\` on Windows and relative `openat` walks on Unix ([80] P10).
- `file mv` refuses to create, and `link`/`file add` warn about, names some supported OS cannot hold ([80] P5: Windows device names, a trailing dot or space, `<>:"|?*`, a component over 255 bytes, a sibling equal under `fold_v1`); the policy is the key `files.portable-names = refuse | warn` (store, hot, default `refuse`, [AR §13]), and `--allow-nonportable` overrides it per command. Such paths already in git stay linkable and render `missing (not representable on this OS)` where they cannot exist.
- **Case.**
  - Versioned data compares paths as exact bytes: I-F1 allows one live node per (root, exact path).
  - `PATHIDX` orders keys by (root, fold(path), path), where fold is `fold_v1 = NFD(full_casefold(NFD(x)))` — full case folding (CaseFolding.txt statuses C and F) and normalization at Unicode 17.0.0 (a resolver constant, R-14; [80] P6), so it merges every pair NTFS, APFS or an ext4 casefold directory merges (ß = ss included). Case variants are adjacent, so a collision probe is one index step.
  - At resolve time the directory's equivalence decides what the disk does: case sensitivity from `FileCaseSensitiveInfo` (Windows), `FS_CASEFOLD_FL` (Linux) or `VOL_CAP_FMT_CASE_SENSITIVE` (macOS), and normalization-insensitivity on APFS, HFS+ and Linux casefold directories ([80 §2.11.1]).
  - **Twins** ([80 §2.11.4] rule 2, [81 M4]). Two live nodes — or a node and another entry of τ(H) — whose paths are equal under that equivalence, but which another OS holds as distinct files (a Linux commit of `docs/Plan.md` and `docs/plan.md`, or NFC/NFD twins), form a twin set. Only the twin whose recorded content (its `oid` in τ(H), or `last_oid`) equals the file on disk resolves normally, whatever spelling the enumeration returns (git keeps the first name and writes the last content on a colliding checkout); every other twin renders `missing (not representable on this OS)` and is never re-bound; when no twin's content matches, or several do, every twin renders `ambiguous (case collision)` or `ambiguous (normalization collision)`. Twins that are really one file are merged with `links fix --same-as`. No link is `ok` on a spelling match alone.
  - **A case-only or normalization-only difference on disk is not a move.** On a case-insensitive directory, stat of the stored spelling succeeds and enumeration returns another spelling [M, 13 §1.4]. Outside a twin set the link is `ok` with the detail `spelling differs on disk (A.rs)`. On a normalization-sensitive directory (NTFS, Linux without casefold), a missing path with exactly one NFC-equal entry is `ok (normalization differs on disk)`, and two such entries are `ambiguous (normalization collision)` ([80 §2.11.4] rule 2). Git keeps the old spelling in its index, HEAD and new worktrees under `core.ignorecase=true` [M, 41 §1], so recording the disk spelling would make every other tree re-bind it back. A case change is recorded only when τ(HEAD) of a writer tree spells the new case (a committed `git mv -f`; provenance `git/case`) or through `file mv`.
  - On a case-sensitive directory (Linux, macOS with a case-sensitive volume, or a case-sensitive NTFS directory), `a.rs` and `A.rs` are different names and the normal rules apply.
- **Reparse points.** A link names the link itself. Resolution never follows a symlink or junction out of the root.
- **Hard links.** Two paths share one file id. File-id evidence (E3) excludes paths already bound to another node.

**Directories.** `artifact_kind = dir` links a whole directory. Evidence for directories:
- the directory's own file id, which NTFS keeps when the directory or any ancestor is renamed or moved [M, §0.3] (git recreates directories on checkout, so the id does not survive a git rewrite);
- `path_moves` entries;
- the file nodes under the directory moving together.

**The root node and `path_moves`.** Each root that holds at least one file node has one **root node**: kind `area`, uid derived from the root name (§2.3), title `root:<name>`, fields `root` and `path_moves`. It is created in the same commit as the root's first file node on a branch. Two lanes create the same uid, so their merge sees equal existence.

`path_moves` is a set of `pathmove` entries `{hlc, class, from, to, git}` (R-1):
- `from` and `to` are root-relative directory prefixes ending in `/`;
- `hlc` is the hlc of the commit that adds the entry. It is part of the value, so every store orders entries identically;
- `git` is the git commit in which the move was observed or committed (empty without git);
- `class` records how the move is known:

| Class | Recorded by | Rewrites globs, drives merge composition | Feeds aliases and E5 |
|---|---|---|---|
| `explicit` | `file mv` of a directory (§3.4) | yes | yes |
| `confirmed` | `links fix --prefix FROM TO` | yes | yes |
| `committed` | a settle in a writer tree that saw, through E6, every tracked file under `from/` renamed to `to/` inside one git commit of the tree's history | yes | yes |
| `observed` | a settle that re-bound every linked node under `from/` to `to/` on exact evidence, possibly across several passes (runtime `PREFIXEV` accumulates the count), after `from/` disappeared from the tree | **no** | yes |

The `observed` class exists because an in-tree quarantine (`mv tests/fixtures tests/_disabled/…` and back) looks exactly like a directory move to a lazy observer; it may add aliases, but it must never rewrite owner-authored globs twice or enter merge composition [41 m12].

`path_moves` is an ordinary set field, so:
- it lives in the `.moi` tree diff and survives checkpoint folding, git-side edits, revert and cherry-pick (they change it like any field);
- it merges as an add-wins union;
- merge composition applies the entries added on a side since the LCA in the deterministic order (hlc, from, to) (§5.5);
- it needs no op of its own, no canonical-form item and no trailer [41 B4].

**Globs** (`task.files_owned`, `note/rule.applies_to.globs`, `area.path_globs`) stay pattern fields rooted at `project`. They are not file nodes [13 §2.3].
- An `explicit`, `confirmed` or `committed` directory move rewrites every glob whose *literal prefix*, up to the last `/` before the first wildcard, starts with `from/`. The rewrite lands in the same commit as the `path_moves` entry.
- A glob that matches nothing in the reading tree renders `[glob matches nothing]` in packs and `links check`. That is the loud failure Bazel's `allow_empty = False` gives [D, 12 §4.7], and exactly what `.claude/rules` globs do not give today [D, 12 §4.9].

**Paths inside prose.** Node bodies are versioned text, and they are never rewritten. At render time, packs annotate an exact root-relative path mention that resolves through a node's current path, its aliases or `path_moves`: `docs/PHASE-X-PLAN.md (now docs/archive/PHASE-X-PLAN.md)`. `links mentions` lists such mentions. Only mentions that resolve to a real path at write time become `at` links, and then only when the author asks with `--at`. This is the path analogue of the `#N` sigil rule [AR §3.3].

### 2.5 Content identity and fingerprints

```
is_text(b) := no NUL byte in b, no CR in b that is not followed by LF,
              and (printable(b) >> 7) >= nonprintable(b)             (git's convert_is_binary statistics
                                                                      over the whole buffer [D, git convert.c])
norm(b)    := is_text(b) ? every CR LF replaced by LF : b
oid(b)     := H("blob " ‖ decimal(len norm(b)) ‖ 0x00 ‖ norm(b))
              H = SHA-1 (tag sha1), or SHA-256 (tag sha256) when the repository's objectFormat is sha256
```

`printable` and `nonprintable` count bytes exactly as git's `gather_stats` does: bytes ≥ 32 other than 127 and the controls BS, HT, ESC and FF are printable; other controls and DEL are nonprintable; a final `^Z` is not counted. Revision 1 used git's *diff* heuristic (no NUL in the first 8,000 bytes), which normalises files with lone CRs that git's autocrlf treats as binary [41 m1].

- **Why this function.**
  - 72 % of the owner's working-tree files are CRLF under `core.autocrlf=true`, while git blobs are LF [M, 10 §5.1]. A raw-byte hash would differ between git and the working tree, and between worktrees.
  - For text files that git converts normally, this `oid` equals git's blob id. It differs where git's "safer autocrlf" keeps CRLF because the index already held CRLF [D, git convert.c].
  - **Equality with git is never assumed.** Every git comparison (E6, the HEAD-tree gate, old blobs for similarity) uses git's own blob ids read from git trees, and the composite stores the git blob id separately (`observed_blob`). moirai always computes its own `oid` from bytes, and a stat cache avoids rehashing unchanged files [10 §5.6].
- **Cost.** The hash is ≤ 15 % of read + hash. Open and read under Defender cost about 150 µs per file warm and about 550 µs cold [M, 10 §5.6].
- **Crates.** SHA-1 and SHA-256 are leaf crates already allowed for the git object layer ([AR] T10, [60] M4). The core `files` module uses them too; this is a boundary note for §8.4, not a new dependency class.
- The rule is frozen in format v1 (R-14), because `oid` values are versioned data.
- **Streaming, bounded memory** ([71 RAM-M4]). The header needs the normalised length before the first hashed byte, so `oid` is computed in **two passes** over one fixed 128 KiB buffer per thread: pass 1 gathers the `is_text` statistics, the normalised length, the line hashes a window needs and the sketch; pass 2 streams the normalised bytes into the hasher. No project-file read ever holds a whole file: a 16 MiB file costs the fixed buffer plus a line-hash array capped at `files.max-line-hashes` (65,536 lines = 512 KiB), and `files.max-read-bytes` (16 MiB) is the largest file whose content is examined at all, not a buffer size.

**Fingerprint (`FPRINT`)** is content-addressed, keyed by `oid`, and never versioned. It is computed in the same read as the `oid` for text files: normalisation runs at 386 MB/s [M, 10 §5.6]. About 300 B per content version:

| Item | Size | Use |
|---|---|---|
| `nlines`, `nbytes` (normalised) | 8 B | size prefilter; a CRLF copy is within `nbytes … nbytes + nlines` |
| `weight`, `distinct` | 8 B | turn sketch intersections into containment in both directions [10 §8.1] |
| `sketch`: bottom-64 u32 hashes of normalised lines (trimmed, whitespace collapsed, lines ≤ 3 characters dropped) | 256 B | similarity stage 1: recall@10 of 287–300 out of 300 across every edit class [M, 10 §5.8]; the `replaced` test (§4.4); E8 |

- **Retention.** A fingerprint is kept only while its `oid` is the current or latest-observed content of a live file node on a live branch head. It is garbage-collected with blobs [AR §4.9].
- **Not exported.** An importing store rebuilds fingerprints whenever it can read the content. A file that vanished before the importing store ever saw it can therefore be re-bound there only by exact evidence and git history, not by similarity. This is a stated limitation (§5.7).

### 2.6 Runtime tables: OS identity, caches, evidence

These tables are store-level runtime state, like leases and markers [AR §5d.1]. They are keyed by `#N` or by tree. They are never versioned, never merged, never exported and never hashed (invariant I-F4).

| Table | Key | Contents | Durability | Size |
|---|---|---|---|---|
| `TREES` | tree key = BLAKE3-16(canonical top-level, or the canonical bound directory: per component with on-disk names, looked up by the root's `OsFileId` first; [80] P9) | canonical root path, the root directory's `OsFileId`, `os` tag, a 16-byte `VolumeCaps` snapshot (incl. the measured mtime granularity), case- and normalization-sensitivity map, cloud-root flag, last HEAD seen, last settle `hlc`, first-settle-done flag, journal cursor ref (`JOURNALCUR`; reserved; E2 is not built); the **settle epoch list** `{scope digest, hlc}` and the **dirty row** `{count, HEAD, hlc}` written only by settles, `links sync`, git hook blocks and the edit-evidence hook, never by a read ([70 S5, S8]) | lazy | ~110 B per tree + 24 B per epoch |
| binding rows (in [AR]'s `HEADS` bindings) | tree key | moirai branch, `designated` flag, **expected git ref** and **base commit** (R-15) | durable, like every [AR] binding | ~60 B per binding |
| `FILEOBS` | (`#F`, tree) | `path_seen` (when it differs from the branch value), the tagged `OsFileId` (57 B: kind, volume key, id, **parent-directory id**, document id; on Linux the id is the inode plus a file-handle digest that carries the generation; [80 §2.11.2]), size, mtime, ctime, **creation time** and added time (i64 ns + granularity), attributes (incl. the cloud bits of §4.6), `last_oid`, **`verified_at`** (hlc of the last settle that saw F present at its path in this tree; a settle writes the row only when the quadruple, state or proposals changed, so the effective value is max(row, the newest `TREES` epoch whose scope covered F and saw it unchanged), [70 S8]), state (incl. `ambiguous (path reused; original at q)`, [72 M13]), ≤ 3 proposals `{path, evidence, score}`, `missing_since`, `resolver_version` | lazy | 96–150 B |
| `PENDING` | (`#F`, tree, from, to) | evidence class, captured `oid`, captured creation time, git HEAD of the observing tree, hlc, source (evidence hook, reader-tree settle) | lazy; 30-day retention (evidence only, never intent: §3.4) | ~100 B |
| `FSINTENT` | intent id | `FsIntent{op, items[(src, dst, oid \| dir)], branch, tree, holder anchor of kind `intent` (the CLI's own liveness slot and nonce, [AR §6.2], [80 §2.7.2]; `ProcId` as diagnostics), hlc}`, then `FsIntentDone` or `FsIntentAborted` | **durable** | ~70 B + items |
| `FPRINT` | `oid` | §2.5 | lazy (derivable) | ~300 B |
| `JOURNALCUR` | volume key | `{kind (usn \| fsevents), vol_key, instance, cursor}` ([80 §2.11.2]) | lazy | 41 B |
| `PREFIXEV` | (tree, root, from, to) | linked nodes under `from/` re-bound exactly to `to/` so far, nodes remaining, first hlc | lazy | ~80 B |
| `DIRMAP` | (tree, directory `OsFileId`) | root-relative path and mtime of each directory a settle enumerated: the Linux frontier's input for E3/E3d and an E7 filter on every OS ([80 §2.11.3]) | lazy (derivable; section derived-optional) | ~50–70 B per directory |
| `GITFACTS` | commit id (and commit pairs) | cached pure functions of git objects: ancestry answers ([AR]'s `ANCESTRY` facts), per-commit exact-rename lists with ambiguous identical-blob groups, commit times | lazy (derivable) | ~40 B + 60 B per rename |
| `ANCHORRES` | (anchor uid, file `oid`, resolver version) | state, span, score of the anchor cascade (§4.5), so a read in any process reuses a result a settle or the edit-evidence hook computed ([70 S7]) | lazy (derivable) | ~32 B |

Written only by writer paths: settles, the file verbs, and hooks that append runtime evidence. Hooks take the writer byte only to append a lazy record, with no flush of their own [AR §2.8]; such records sit above `HEAD.durable_lsn` until the next covering flush (an evidence record appended behind a pending durable group becomes visible with that flush, [AR §4.5]), and a lost lazy tail after an OS crash or a failed flush is the end of the log, never corruption ([AR §4.2], [72 B1]). Reads may *use* `GITFACTS`, `FILEOBS` and `ANCHORRES` but append nothing (I-F5); they compute missing git facts in process within the read-path caps of §4.8 and keep them for the command only.

The OS id is stored as a tagged `OsFileId` ([80 §2.11.2]): on Windows `FILE_ID_128` plus a key derived from the 64-bit volume serial, on Linux a key derived from `f_fsid` plus the inode and a file-handle digest that carries the inode generation, on macOS a key derived from the volume UUID plus the file id. A row whose kind this OS cannot interpret counts as absent, and identity is always the whole `OsFileId`. `nFileIndex` is never used, because on ReFS the 64-bit id can be −1 [D, 09 §2.5]. Every file-id hit is verified (§4.3 E3), because MFT slots are reused aggressively: one slot was reused 165 times in 2,000 create/delete cycles [M, 09 §2.1]. On Linux an inode number alone is never identity: ext4 hands out the lowest free inode, so a directory and a file deleted together can return as unrelated objects with the same numbers, and only the file-handle digest tells them apart ([80 §2.11.4] rule 8, [81 B2]).

**The stat quadruple** is (size, mtime, file id, creation time). Settles read all four through directory enumeration (`FileIdExtdDirectoryInfo`). Reads use `GetFileAttributesExW`, which returns size, mtime, creation time and attributes but no file id, so a swap of two files with equal size and mtime is invisible to a read until the next settle. This limitation is documented, not hidden [41 m7]. On Linux and macOS a read's `statx`/`lstat` returns the id too (on Linux with one `name_to_handle_at` for its generation), so there a swap is visible to reads ([80 §2.11.4] rule 3).

### 2.7 Anchors

An anchor is a record owned by the `at` edge from the referrer to the file node. It has a store-local number `aN`, allocated from a new `HEAD.next_anchor` counter (§2.11 R-6), and a derived 128-bit uid for the image:

```
captured   = BLAKE3-128( lp(file uid at capture) ‖ lp(kind) ‖ lp(scope) ‖ lp(quote.exact) ‖ lp(end.exact) ‖ lp(occurrence) )
anchor uid = BLAKE3-128( lp("moirai-anchor-v1") ‖ lp(src uid) ‖ lp(captured) )
```

Every field is length-prefixed, so no two different captures can collide by concatenation [41 m4]. `captured` is stored with the anchor and never changes. A `repin` changes the selectors but neither `captured` nor the uid, so the derivation stays verifiable after a repin and after a file-node re-key (§5.5). **Capture de-duplication:** a new capture on (src, dst) whose selectors equal an existing anchor's *current* selectors reuses that anchor, so an identical capture after a repin never duplicates it.

**Versioned fields.** All selector fields form one merge key that changes only when the anchor is repinned:

| Field | Content | Size (typical) |
|---|---|---|
| `kind` | `file` \| `heading` \| `symbol` \| `quote` \| `range` \| `lines` | 1 B |
| `mode` | `live` \| `pinned` (a historical citation, never re-resolved; rendered `path@c4410:L12-18`) | 1 B |
| `watch` | `header` (default for `heading` and `symbol`) \| `span` (default for `quote`, `range`, `lines`; opt-in for `file`, which then means a content pin) | 1 B |
| `scope` | language + segments: Rust `struct LockFile / impl LockFile / fn acquire` with an optional trait qualifier; Markdown heading text path with numbering kept in a separate field; TOML `table.path / key` | 40–60 B |
| `quote` | W3C TextQuoteSelector: `exact` ≤ 64 B by default (up to 128 B when needed), `prefix`/`suffix` 32 B (extended up to 64 B at capture until unique). CRLF→LF, lines trimmed | 60–130 B |
| `end` | a second quote, for `range` (a span longer than 4 lines or 128 B; W3C RangeSelector) | 0–130 B |
| `occurrence` | u16, only when quote + context + scope is still not unique at capture (~1–2 % [M, 11 §2.4]) | 0–2 B |
| `window` | up to 16 non-trivial normalised lines before and after the span, as u16 hashes, plus the span's offset in the window | ≤ 68 B |
| `hint` | line range at capture, in the captured content | 8 B |
| `blob` | `oid` of the file at capture | 21 B |
| `git` | observed git commit (optional) | 0–21 B |
| `span_hash` | xxh3-64 of the normalised span (for `header` watch: of the header) | 8 B |
| `captured` | the capture digest above | 16 B |
| `marker` | an opt-in in-file marker id (prose only; §9.2 decision 3) | usually 0 |
| `resolver` | resolver version at capture | 2 B |

The total is about **265–415 B per anchor, typically 345 B** [I, derived from 11 §4.1 plus the 16-B digest]. The owner's 34,195 citations [M, 11 §0.2] would take about 11.8 MB on disk and 0 B resident at idle.

**Departure from [11]: a context window instead of old blobs.** [11] resolves duplicate quotes and dead line anchors with a line diff from the blob captured with the anchor. That needs the old content, which R2 cannot guarantee without git, or without moirai keeping snapshots of about 10–15 MB [11 Q1]. This design stores a ±16-line window of line hashes instead:
- a duplicate quote is disambiguated by aligning the stored window against each candidate's surroundings (LCS over ≤ 32 tokens, µs);
- a `lines` anchor is mapped by aligning the window in the current file.

This is an **[I]** design. Its test gate (§8.3.4) requires that, on a replay of the owner's citation sample, the window tie-break agrees with full-diff mapping in ≥ 99 % of duplicate-quote cases. Where the old blob is available in process (through the git object reader, looked up by git's own blob id), the resolver uses it as a further tie-break. When it is not, the resolver still reaches a verdict.

**Authoring forms** (what agents type; all are safe unquoted in both shells unless they contain spaces, §0.3):

| Form | Captured kind | Example |
|---|---|---|
| `path` | `file` | `docs/plan/storage.md` |
| `path:L` or `path:L-M` | `quote` (≤ 4 non-trivial lines and ≤ 128 B), else `range`; `lines` when the span has no non-trivial text; an enclosing scope is added when a scanner finds one | `crates/engine/src/log.rs:210-214` |
| `path::A/B` | `symbol` (Rust items; TOML `table/key`), Serena-style name path [D, 11 §3.5] with `Type[Trait]/method` for trait impls | `crates/engine/src/lock.rs::LockFile/acquire` |
| `path#H` or `path#H1/H2` | `heading` (Markdown; numbering such as `3.2` or `§` is stripped into its own field, and the slug is only a rendering) | `docs/plan/storage.md#Recovery` |
| `path@<commit>:L-M` | `quote` with `mode = pinned` | `docs/plan/storage.md@c4410:12-18` |
| `--quote-file PATH` (preferred) or `--quote -` (stdin), together with a path | `quote` from literal text; it must be unique or disambiguated | — |

Quote text from a file or stdin has a leading UTF-8 BOM stripped, and input that contains U+FFFD is refused (exit 2): Windows PowerShell 5.1 pipes non-ASCII text to native programs as `?`, and Claude Code's PowerShell tool prefixes stdin with a BOM [M, 16 §6.10]. 157 of the owner's headings are Cyrillic [M, 11 §2.5], so `--quote-file` is the form the skill teaches [41 m9].

**Capture** runs at write time, on the caller's resolved tree:
1. **Resolve the path.**
   - A root-relative path is used as given, then respelled to git's HEAD spelling or the enumerated spelling (§2.4).
   - A token without `/` that is not found at the root is searched as a basename, first in the path index, then (in explicit verbs) with one tree walk. A unique match is expanded and stored in full. Several matches are an error that lists them (exit 2), which removes the 15.9 % of citations ambiguous at authoring [M, 11 §2.1].
   - A path that is not found is refused unless `--planned` is given (§2.9).
   - A cloud-only placeholder (§4.6) is refused unless `--allow-hydrate` is given.
2. **Read the file** once. It is opened with `FILE_SHARE_READ|WRITE|DELETE` and closed immediately. The `oid` and `FPRINT` come from the same read. The file node is registered with its derived uid.
3. **Build the selectors** for the form.
   - Spans skip lines that are blank or contain only braces (2.6 % of real citations point at such lines [M, 11 §2.3]).
   - A symbol's header quote is its first line up to `{` or `;`.
   - A heading's quote is the heading line.
4. **Make it unique.** The resolver runs on the captured file itself. If the quote is not unique there:
   - the prefix and suffix are widened to 64 B;
   - then the enclosing scope is added;
   - then an `occurrence` index is recorded.
   The result line reports it (`duplicate text: occurrence 2 of 3 in scope`). With ±2 lines of context, 98.7 % of Rust lines, 99.9 % of Markdown lines and 98.9 % of HLSL lines are unique [M, 11 §2.4]. Name paths are 99.88 % unique [M, 11 §2.6].
5. **Record** the window, the hint, the `blob`, the git commit, the span hash, `captured` and the resolver version.

Capture costs one read plus a line index, about 0.1 ms [M, 11 §4.2], plus a scope scan (§2.7.1).

#### 2.7.1 Scope scanners

The design has hand-written, deterministic scanners with no C dependency:

| Language | Scanner | Why |
|---|---|---|
| Rust | a tokenizer that tracks comments, strings, raw strings, char literals versus lifetimes and brace nesting, and emits `mod`/`impl [Trait for] T`/`fn`/`struct`/`enum`/`trait`/`const`/`static`/`macro_rules!` item headers with their spans | a scope narrows the search to a median of 10 lines, and it turns 17 of 24 quote orphans into coarse survivals [M, 11 §2.3, §2.6]. tree-sitter-rust would add a C runtime and +1.8 MB [M, 11 §2.9] (§9.2 decision 9) |
| Markdown | a fence-aware ATX/setext line scanner | 0.17 ms per file, against 722 ms for tree-sitter-md on one 5,900-line file [M, 11 §2.8] |
| TOML | a table and key line scanner | 96.4 % of TOML citations never moved [M, 11 §2.3] |
| everything else (HLSL, JS, PowerShell, Python, YAML, JSON) | none; quote + window only | tree-sitter-hlsl fails on 23.9 % of the owner's shaders [M, 11 §2.9] |

A scope never replaces the quote. It narrows the search and names the anchor. If the scope does not resolve (the symbol was renamed), the whole file is searched [11 §4.5], and a fuzzy header match then considers only headers of the same item kind (§4.5). The scanners are validated against tree-sitter used as a **test-only oracle**; §8.3 has the target.

### 2.8 Edges

- **`at`** (any node → `artifact`) is a new edge kind of the *historical* class ([AR §3.3]).
  - If the file node itself is deleted (`moirai rm 812`, rare), `at` becomes a tombstone ref and the source becomes `suspect`.
  - If the file is *removed* (`file rm`, status `removed`), the source becomes `suspect` through an extension of the derived `suspect` rule, which is graph-only (§2.9).
  - The CSR holds one `at` adjacency entry per (src, dst). Anchors live in the `ANCHORS` section, keyed (src, dst, anchor) (§2.11 R-8).
- The edge key gains an optional 128-bit **discriminator**, the anchor uid, so one node can cite several places in one file. Set semantics remain per (src, `at`, dst, anchor).
- **Mapping of existing path-bearing fields** [13 §2.3]:
  - `finding.where {file:symbol@sha}` becomes an `at` edge with a symbol anchor;
  - a `cites` to a file becomes `at` with `mode = pinned`;
  - `produced`/`consumed` (run → artifact) stay as they are, and their target artifacts are now file nodes;
  - `lane.worktree_path`, `run.script_path` and `run.journal_path` become `abs` artifacts: existence only; `doctor lanes` reports a moved worktree.

### 2.9 Link states

**File level.** Derived per tree at resolve time, except `removed` and `planned`, which are versioned status.

| State | Meaning | Written? |
|---|---|---|
| `ok` | path present in the tree. Details: `changed since c…` when the content differs from `oid` (not a move); `spelling differs on disk (A.rs)`, `normalization differs on disk` | no |
| `moved-auto` | found elsewhere with exact evidence. Details: `recorded c4474`, `not yet recorded`, `reader tree: not recorded`, `uncommitted in the main tree: not recorded` | written at the next settle in the branch's writer tree, when that tree is fresh (§5.3) |
| `moved-needs-confirm` | one strong or weak candidate. Details: `identical copy`, `edited+moved`, `similar 0.81`, `split`, `merged`, `moved differently on this line` | proposal kept in `FILEOBS` only |
| `ambiguous` | ≥ 2 candidates. Details: `rename-over`, `swap`, `merge conflict` (a composite conflict value, §5.5), `case collision`, `PathClaim` | proposal only |
| `deleted` | status `removed` (`file rm`, `links fix --drop`), with reason and `replaced_by` | versioned |
| `replaced` | path present, but the file there was re-created with content unrelated to the last observed content (§4.4) | no; `FILEOBS` |
| `missing` | absent, no candidate, including "moved outside the root, into ignored output, or into the Recycle Bin" and "deleted in git c…" | no; `missing_since` in `FILEOBS` |
| `absent-in-tree` | this tree cannot contain the observation. Detail `behind` (the tree is a strict ancestor of the observation: folded into the header count) or `diverged` (marked per link, §5.2) | no |
| `pending` | this tree still has an alias; the move happened on a line it has not received. Rendered per link, with its age once older than `files.pending-escalate` (14 days) | no |
| `planned` | linked with `--planned`; binds when the file appears (§3.2) | versioned status |
| `unverified` | not determinable now. Details: `budget`, `cloud-only`, `commit not in this repository`, `no tree` | no |

**Anchor level.** Derived and cached per (anchor uid, file `oid`, resolver version). Never versioned [11 §4.4].

| State | Meaning |
|---|---|
| `fresh` | the span hash matches at the hint (for `header` watch: the header matches) |
| `moved` | identical text, unique, found elsewhere in the file. This is silent: only the cached hint changes |
| `edited` | the best match is fuzzy (quote similarity ≥ 0.75 with a margin), or only the scope survived |
| `ambiguous` | ≥ 2 candidates within the margin |
| `orphaned` | nothing found |

**The link state an agent sees** is the file state, refined by the anchor state when the file resolved (`ok` or `moved-auto`). The owner asked for six primary states; [41 M5] adds a seventh, `replaced`:

| Primary state | File × anchor |
|---|---|
| `ok` | file `ok`, anchor `fresh`/`moved` (or a `file` anchor) |
| `moved-auto` | file `moved-auto`, anchor `fresh`/`moved` |
| `moved-needs-confirm` | file `moved-needs-confirm` (all details) |
| `ambiguous` | file `ambiguous` |
| `deleted` | file `removed` |
| `replaced` | file `replaced` (anchors are not resolved against unrelated content) |
| `stale-anchor` | file `ok`/`moved-auto`, anchor `edited`/`ambiguous`/`orphaned` (the sub-reason is shown) |

The tree-relative states `missing`, `absent-in-tree`, `pending`, `planned` and `unverified` complete the vocabulary. The strings, the details and the header strings `files: no tree bound` and `reading only: tree on <ref>, branch expects <ref>` are frozen in the output contract (R-16).

**Severity order** (adopted from [50 §2.6] at the integration of 2026-09-26, with `replaced` placed by this design). Where one state must summarise several anchors — `link_state(n)` of a node, the pack header, `brief` — the most severe wins, in the order `missing, replaced, deleted, ambiguous, moved-needs-confirm, stale-anchor, unverified, pending, planned, absent-in-tree, moved-auto, ok`. `replaced` ranks just below `missing` and above `deleted`: a deletion is recorded intent that every referrer already sees as `suspect`, whereas a replaced path shows unrelated content that a reader could mistake for the cited file.

**Effect on referrers.** There are two separate mechanisms, because only one of them is a function of graph state:

- **Graph-derived `suspect`** ([AR §3.5], maintained eagerly in the write path): an `at` edge whose file node is `removed` or deleted makes its source `suspect`. It is a function of graph state only, so it can live in the header flag and in bitsets.
- **Tree-derived link staleness**: `stale-anchor`, `missing`, `ambiguous`, `replaced`. It depends on the file system, so it is a *read-time* predicate, like [AR]'s `stale`. It is never stored in the header, never gates `ready`, and is valid only at branch tips with a resolved tree ("unknown at past views", matching [16 §4.5] for `stale`). Packs show it. `merge-check --strict-links` can gate on it (§5.4).

Removing a file on purpose (`deleted`) is therefore visible to the engine's derived state on every branch that receives the commit. A file that vanished, was replaced, or whose cited text was edited is visible at every read.

### 2.10 Invariants

| Id | Invariant |
|---|---|
| I-F1 | On every branch view, at most one live file node with status `present` or `planned` exists per (root, exact path). The rule is enforced at write time. After a merge, a duplicate is a `PathClaim` conflict value, never a silent pair (§5.5). Case-insensitive collisions are a resolve-time state (§2.4), because case sensitivity is a property of a tree, not of versioned data |
| I-F2 | A file node's uid equals the §2.3 derivation over its stored `origin_path` and `origin_pred`; a root node's uid equals its derivation; an anchor uid equals the §2.7 derivation over its stored `captured`. A Create of a uid already known to the store reuses its `#N` |
| I-F3 | Every `at` edge carries ≥ 1 anchor. Every anchor belongs to exactly one `at` edge. Anchor uids are unique per (src, dst) |
| I-F4 | OS file ids, volume serials, mtimes, creation times, stat caches, resolution states, proposals, `PENDING`, `FSINTENT`, `FPRINT`, `PREFIXEV`, `GITFACTS`, `TREES` and USN cursors never appear in versioned, hashed or exported data |
| I-F5 | Read verbs (`show`, `pack`, `brief`, `get`, `find`, `q`, `links check`, MCP reads) append nothing to the log |
| I-F6 | An automatic re-bind is written only with exact evidence (or under owner policy B, §9.2 decision 1), only by the branch's writer tree, only when that tree is fresh for the node, only after the quiescence re-check (§4.2), and, on `main`, only for an observation committed in the writer tree's HEAD (§5.3) |
| I-F7 | `removed` is never inferred from absence. It is written only by `file rm`, `links fix --drop`, `links fix --same-as`, or (policy §9.2 decision 6) a deletion commit seen on the main tree |
| I-F8 | Path rules of §2.4 |
| I-F9 | No live span anchor has a bare line number as its only selector. Every `quote`, `range`, `symbol` and `heading` anchor carries a quote, and every `lines` anchor carries a window |
| I-F10 | `resolve(link, tree snapshot, resolver_version)` is a pure function. Thresholds and pattern lists are constants of the resolver version, and a version bump is a visible event (results carry the version) |
| I-F11 | moirai opens project files only with `FILE_SHARE_READ\|WRITE\|DELETE`, never maps or locks them, closes each handle before touching the next file, and never opens the content of a cloud-only entry in an automatic path |
| I-F12 | **Binding uniqueness.** Each moirai branch has at most one designated tree, and each tree is designated for at most one branch. Trees are identified by their exact git top-level; a binding of a directory never covers a nested worktree inside it |
| I-F13 | **No content-only re-bind.** An automatic re-bind never rests on equal content alone: an equal-`oid` candidate needs corroboration by creation time, by a git rename inside one commit, or by captured intent, and a candidate that coexisted with the original is never a target (§4.4) |
| I-F14 | **No resurrection.** A derived uid that is removed or deleted on a branch view is never live again on that view through registration or merge; only `links fix --restore` or `Undelete` (explicit, recorded) bring it back |

### 2.11 Format reservations: in format spec v1, before the first byte

Early adoption is not a goal, and the format is frozen at [60] M0's exit, so every field the subsystem needs is reserved now. [60 §2.5] carries this table (brought to revision 2 at the integration of 2026-09-26, §8.4) and [AR §4.6] places each row; this table is authoritative.

| # | Reservation | Where in [AR] / [60] |
|---|---|---|
| R-1 | value types `path` (root sym u16 + varint-length UTF-8, exact bytes), `oid` (algo u8 + 20 or 32 B; content ids, git blob ids and git commit ids) and `pathmove` (`{hlc u64, class u8, from path, to path, git oid-or-empty}`) in the closed type set | [AR §3.1] field block; [60 §2.5] "ops and values" |
| R-2 | the `artifact` field set of §2.2, including `origin_path`, `origin_pred` and `observed_blob`; status values `planned`, `removed`; merge classes `observation` (composite) and `identity` (immutable) in the schema's merge-class enum; `area` fields `root` and `path_moves` | [AR §3.2, §5a.7] |
| R-3 | schema column `uid_derivation ∈ {random, file-key, root-key, anchor-key}` per kind or record; the length-prefixed derivation functions of §2.3 and §2.7, the predecessor order by (generation, commit id), the dead-uid re-derivation rule and the merge re-key rule of §5.5 are part of the format spec | [AR §2.12] schema-as-data |
| R-4 | edge kind `at` (historical); an optional 128-bit discriminator in the edge key; the anchor record layout including `captured`; op `SetEdgeProps{src, kind, dst, disc, old, new}` for repins (`AddEdge`/`RemoveEdge` carry the anchor as props) | [AR §3.3, §4.3]; [60 §2.5] typed property blocks |
| R-5 | the root node (§2.4): its uid derivation and its two fields. **No op, no canonical-form item, no image trailer and no commit annotation** for directory moves [41 B4] | [AR §3.2] `area` |
| R-6 | `HEAD.next_anchor u32` (the reserved `_ u32` after `next_id`) | [AR §4.2] |
| R-7 | log record kinds `FsIntent`, `FsIntentDone`, `FsIntentAborted` (durable); `FileObs`, `Pending`, `FPrint`, `JournalCursor` (was `UsnCursor`), `DirMap`, `TreeReg` (with settle epochs and the dirty row), `PrefixEv`, `GitFacts`, `AnchorRes` (lazy) ([70 S5, S7, S8], [80 §2.11.2]) | [AR §4.3]; [60 §2.5] reserved kinds |
| R-8 | segment sections `PATHIDX` (sorted (root, fold(path), path) → `#N` for present or planned nodes), `ALIASIDX`, `ANCHORS` (sorted (src#, dst#, anchor#) → record), `ANCHOR_UID`, `FILEOBS`, `PENDING`, `FSINTENT`, `FPRINT` (`oid` → blob ref), `JOURNALCUR`, `DIRMAP` (derived-optional), `TREES`, `PREFIXEV`, `GITRENAMES` (beside [AR]'s `ANCESTRY`), `ANCHORRES` (runtime) and `GLOBIDX` (versioned, path globs by literal prefix) ([70 S7, S17]) | [AR §4.4]; [60 §2.5] `PATHIDX` |
| R-9 | blob class "fingerprint" in `blobs.NNNN`, keyed through `FPRINT` | [AR §4.1] |
| R-10 | the byte layout of the anchor selector block as an edge-property value inside canonical-form item 10's edge class `(uid, at, dst uid, props) → present \| absent`, with the discriminator in the key; quote, prefix and suffix enter the canonical form **only as BLAKE3-128 digests** (`quote_h`, `prefix_h`, `suffix_h`), so a hash-only image destination changes no commit id ([72 M6]). No new key class | [AR §4.6] item 10 |
| R-11 | `.moi` grammar: `anchor` lines (§5.7) with the digests on every line and the text in `full` mode, artifact field names, `planned`/`removed` status values, the block encoding of `pathmove` sets, the `text-unavailable` anchor sub-state | [AR §5b.2]; [60 §2.5] image format v1 |
| R-12 | invariants I-F1…I-F14 | [AR §3.4] |
| R-13 | config keys `roots.<name>`, `files.main-tree`, `files.main-ref`, `files.max-read-bytes` (default 16 MiB), `files.ignore` (defaults `target/`, `node_modules/`, `build/` when a root has no ignore file), `files.policy.auto` (`exact` \| `strong`), `files.hooks.evidence` (`on`), `files.hooks.edit-evidence` (`auto`: on when hooks run as spawn-free `mcp_tool` handlers, [70 S3, S7]), `files.read-budget-ms`, `files.session-start-cap-ms`, `files.links-sync-ms` (time budgets only; the E6 window bound is an R-14 constant), `files.pending-escalate` (14 d), `files.max-line-hashes` (65,536), `files.settle.others-after` (24 h), `files.read.max-uncached-ancestry` (1), `files.read.max-e6-commits` (32), `files.deep.threads` (8), `files.deep.content-readers` (2), `files.cloud` (`metadata-only` \| `refuse`), `files.scratchpads` (`refuse` \| `allow`), `files.deletion-inference` (`explicit` \| `main-tree-commits`), `files.mv-git` (`false` \| `true`), `files.confirm-roles` (`orchestrator,owner`), `files.portable-names` (`refuse` \| `warn`, [80] P5), `image.dest.<name>.anchor-text` (`full` \| `hash-only`). The keys after `files.pending-escalate` were added at the integration and by the priority audits (2026-09-26) so that every operational call of §9.2 is a documented key rather than an owner decision; `files.hooks.nudge` and `files.usn` were dropped with the features they switched ([74 A13, A17]; a later E2 build would bring the OS-neutral `files.journal = auto \| off`, [AR §11] #41); types, scopes and reload classes are in [AR §13] | [AR §4.1] `config`, [AR §11] |
| R-14 | the resolver-version constant table as a spec appendix: thresholds (§4.4, §4.5), the `is_text` rule (§2.5), the case fold `fold_v1` (full case folding and NFD at Unicode 17.0.0), the window-hash function, the never-candidate pattern list (§4.3), the quiescence delay (50 ms), the E6 window bounds (§4.3, incl. the 2,000-commit bound formerly called `files.budget.window`), the E3d identity rule and the path-reuse check of §4.3 ([72 M13]); and the per-OS rules of [80 §2.11.4]: the copy rule's creation-time line only for `VolumeCaps.btime = TunneledNotCopied`, with no clone indicator and a creation time unique in the E4 scope; the twin rule and "spelling differs on disk" with the normalization rule; reads with ids; candidates sorted by exact bytes; the never-candidate additions and per-OS trash locations; `EXDEV` as cross-volume; Unix busy states; an inode number alone never identity; denials as `Unknown`; the frontier's racy threshold from a file-system timestamp | new |
| R-15 | binding-row extension: expected git ref and base commit per designated tree; the binding-uniqueness invariant I-F12 | [AR §5a.4] bindings |
| R-16 | frozen state, detail and header strings of §2.9 (incl. `replaced`); `spelling differs on disk` (replacing `case differs on disk`), `normalization differs on disk`, `ambiguous (normalization collision)`, `unrepresentable path`, `missing (not representable on this OS)` ([80 §2.10–§2.11]) | [AR §7.1] output contract |
| R-17 | the `relink` provenance vocabulary of §2.2, with `agent/*` distinct from `owner/*` and `confirmed/*` | new |
| R-18 | the `FILEOBS` row layout of §2.6 (the tagged `OsFileId` with the parent-directory id, i64 ns timestamps with granularity incl. ctime and added time, attributes, `verified_at`; [80 §2.11.2]) | [AR §4.4] |

---

## 3. Explicit commands

### 3.1 Verbs, and why they are namespaced

[AR §7.1] already has node verbs named `add`, `rm`, `move`, `link` and `unlink`. File operations therefore live under `moirai file`, link maintenance under `moirai links`, and `link`/`unlink` gain `--at`.

```
moirai link   ID --at SPEC [--at SPEC].. [--watch header|span] [--planned] [--quote-file PATH | --quote -] [--allow-hydrate]
moirai unlink ID --at aN|PATH
moirai file add    PATH.. [--kind source|doc|asset|generated|dir] [--root NAME]
moirai file mv     SRC.. DST [--git] [--dry-run] [--retry-ms 1000] [--allow-ignored]
moirai file rm     PATH.. [--reason T] [--replaced-by PATH|ID] [--trash] [--recursive] [--dry-run] [--yes]
moirai file relink PATH|ID --to PATH --after          # record a move already done by other means (hg rename --after)
moirai file revert COMMIT                             # FS-aware inverse of a commit that moved or removed files
moirai file where  PATH|ID [--evidence]               # current path, aliases, state, evidence, proposals, pending moves
moirai links check [--scope ID | --path GLOB | --all] [--tree DIR] [--deep] [--budget-ms N] [--strict] [--ids]
moirai links sync  [--scope ID | --path GLOB | --all] [--tree DIR] [--deep] [--budget-ms N] [--since REV]
moirai links fix   ID|aN (--accept --expect PATH | --to PATH | --confirm | --accept-replacement
                          | --drop [--reason T] [--replaced-by PATH|ID] | --same-as ID | --split
                          | --repin [--at SPEC] | --pin | --restore) [--dry-run] [--yes]
moirai links fix   --prefix FROM TO
moirai links mentions [PATH | --moved] [--in nodes|repo]
moirai links import --from-markdown GLOB [--dry-run]  # one-off migration tool, §9.2 decision 12
moirai hooks install --git [--dry-run]                # owner-run; appends marked blocks
moirai hook  fs-evidence | git-post-commit | git-post-checkout | git-post-merge
```

Every write verb takes the global `--branch`, `--lease`, `--idempotency-key`, `--agent` and `--if-rev` options, and uses the exit codes of [AR §7.1]:
- 2 usage, including an ambiguous basename and quote input containing U+FFFD;
- 3 not found;
- 4 guard conflict;
- 5 branch or tree mismatch, including `file mv`/`file rm` outside the branch's writer tree (§3.4);
- 6 precondition, for example `--strict` with non-`ok` links, `--accept` whose `--expect` differs from the current proposal, or `--repin` without `--at` after a fuzzy match;
- 7 file busy or store unavailable;
- 8 partial batch;
- 9 idempotency mismatch.

### 3.2 `link` and `unlink`

- `link ID --at SPEC` runs capture (§2.7) and writes one commit: the file node (Create or reuse, plus the root node on the root's first link), the `at` edge and its anchors.
- `--planned` links a path that does not exist yet, for example a task that will create `crates/engine/src/sync/lock.rs`. The file node is `planned`, and its composite records the planning tree's HEAD in `observed_git`. The first settle in a writer tree that finds the path turns it `present` with its first observation **only if** that tree's HEAD descends from the planning commit, or the file's creation time is later than the planning commit's time. A stale tree that holds a historical file at that path never binds it [41 m11].
- `unlink ID --at a17` removes one anchor, and removes the edge with its last anchor. File nodes without referrers are kept (history, aliases). `links check` ignores them unless `--all-files` is given.
- **Role policy.** Anyone who may write the referring node may link from it ([AR §7.3]), so critics can anchor findings and architects can anchor plan sections through MCP (§6.3). `link` works from any eligible tree (§5.1): a first registration is an observation of a file that exists there, not a re-bind.

### 3.3 `file add`

`file add` registers files explicitly with no link. Most registrations happen implicitly through `link --at`. It is idempotent: an existing node is printed, not duplicated.

### 3.4 `file mv`: semantics and crash-safe protocol

**Semantics.**
- One filesystem move plus one commit that re-points every linked file node under the source, rewrites globs whose literal prefix lies under a moved directory, adds the old paths to `aliases`, and, for a directory, adds an `explicit` entry to the root node's `path_moves`.
- A directory is **one** rename: 9.5 ms for 1,000 files [M, 13 §1.4]. The 131-file bulk move of the owner's history would take ≈ 0.5–20 s file by file under load (131 × 3.9–156 ms per rename, est. from [M, 13 §1.4]).
- Multi-source `mv a b c dir/` is one intent. Items that fail are reported, the rest commit, and the command exits 8.
- **moirai never touches the git index by default.** A filesystem move followed by the usual `git add -A -- SRC DST` produces the same commit as `git mv`, because git records trees, not renames. moirai prints that hint. Touching the index by default would risk repeating the owner's recorded incident, where another agent's staged deletion was committed [13 §3.4]. `--git` runs `git mv` through the explicit git transport entry point (§9.2 decision 7); it is the only R4 path that may start a git process, and only on request.
- **Writer tree only.** `file mv` and `file rm` run only in the writer tree of the caller's branch (§5.3). Anywhere else (a harness `isolation: worktree` tree, a `wf_*` tree, a tree whose HEAD left the lane's git line) they refuse with exit 5 and print `moirai worktree bind DIR BRANCH` or `moirai lane open …`. Revision 1 performed the move and kept the intent in a lazy, 30-day, unexported `PENDING` row; a crash, a `gc`, a long-lived lane or an image transport could lose it [41 M4]. A raw `mv` stays possible and is caught lazily once the code reaches a writer tree (§9.2 decision 13).

**Protocol.** There is no OS transaction that spans a rename and a commit. TxF is deprecated and is unsupported on ReFS and Dev Drive [D, 13 §1.4]. The protocol keeps the writer byte out of the rename, which can take up to 156 ms or block on error 32 [M, 13 §1.4].

1. **Plan** (read only):
   - resolve the branch and the tree; refuse (exit 5) unless the tree is the branch's writer tree;
   - stat the sources; the destination must not exist;
   - refuse a directory move across volumes, a move into an ignored or outside path unless `--allow-ignored`, a move of a cloud-only placeholder across volumes unless `--allow-hydrate`, and a move of a path currently leased by another `FsIntent`;
   - compute `oid` for single files;
   - list the affected file nodes by a `PATHIDX` range scan **and an `ALIASIDX` probe** [41 m10]. A source that is only an *alias* of F (for example, `sync` brought `main`'s re-bind of F while this lane's code still has the old path) is accepted only when this tree is fresh for F (§5.3). Otherwise the verb refuses with exit 6: `#812 was re-bound to b on this branch from a line this tree has not received (trunk c…); merge that code first, or move it with a raw mv and let the settle propose it`;
   - list the affected globs by literal prefix.
2. **Intent.** Append the durable `FsIntent` record in its own flushed group (~2 ms [AR §2.8]). It is runtime, not a graph change.
3. **Filesystem.**
   - **Windows:** `MoveFileExW(src, dst)` — never `MOVEFILE_REPLACE_EXISTING`, never `MOVEFILE_COPY_ALLOWED` — then `durable-name` on **both** parent directories (a `FlushFileBuffers` of each directory handle) before the commit that records the move, because a data-only flush of the log does not persist another file's rename ([72 M8]). A no-replace rename followed by `durable-name` of both parents is the rename point of every OS ([80 §2.3.2], [81 m1]); the M0 rig calibration (item 17) decides only whether `MOVEFILE_WRITE_THROUGH` is also needed on Windows.
   - **Linux and macOS** (port phase): `renameat2(RENAME_NOREPLACE)` or `renamex_np(RENAME_EXCL)` — never `rename(2)`, which replaces — followed by `durable-name` on both parent directories before the commit. Where Linux lacks `RENAME_NOREPLACE` (`EINVAL`), a file moves by `link` + `unlink`, whose interrupted state (both names, one file id) the recovery below rolls forward; a directory move is refused there, and a macOS volume without `VOL_CAP_INT_RENAME_EXCL` refuses `file mv` (exit 7). `EXDEV` (another mount, a btrfs subvolume, an overlay lower directory) takes the cross-volume path below. `EBUSY`, `EACCES` and `EPERM` replace errors 5/32 in the diagnosis, and there is no `RmGetList` ([80 §2.3.2, §2.11.4]).
   - On error 5 or 32, retry with backoff for ≤ `--retry-ms` (default 1 s), then fail with a diagnosis. For files, `RmGetList` names the holders. For directories, `RmGetList` returns access denied [D, 13 §1.4], so the message names the likely classes: a shell with its cwd inside, an open file, a watcher on a subdirectory [M, 13 §1.4].
   - A cross-volume *file* move is copy → flush → verify the destination `oid` → delete the source.
   - `--git` delegates to `git mv`.
4. **Commit.** One durable commit that carries:
   - `SetField(observation)` for every affected node (`relink: explicit/intent`, `aliases += old path`, `observed_git` = HEAD, `observed_blob` empty until the move is committed);
   - for a directory, the `path_moves` entry `{hlc, explicit, from/, to/, HEAD}` on the root node;
   - glob rewrites;
   - `FsIntentDone` in the same flushed group.
   Each node is CAS-guarded on `rev_seq`. If someone re-bound a node in between, that node's op is replaced by an observation-based resolution (§4.3).
5. **Recovery** at any writer's next open, or in `doctor`, for every open `FsIntent` whose **intent anchor** is Dead — the CLI holds its own liveness slot for the life of the intent, so a crashed CLI's intent is recoverable at once (Unknown leaves it alone; a boot change makes every holder Dead) ([AR §6.2], [80 §2.7.2], [72 B2]):

| Filesystem state | Decision |
|---|---|
| source present with the intent's `oid`, destination absent | the rename never happened → `FsIntentAborted` |
| source present with a different `oid`, destination absent | the rename never happened and the source was edited since → `FsIntentAborted` (content drift is re-observed at the next settle) [41 m14] |
| source absent, destination present, `oid` matches | the rename happened → write the step-4 commit (roll forward), provenance `explicit/intent-recovered` |
| source absent, destination present with a different `oid` | the rename happened and the file was edited afterwards → roll forward, provenance `explicit/intent-recovered`, detail `content changed after the move` [41 m14] |
| both present with one file id (an interrupted Linux `link` + `unlink` fallback) | the rename happened → finish the unlink, then roll forward, provenance `explicit/intent-recovered` ([80 §2.3.2]) |
| both present (unfinished cross-volume copy, or the source re-created) | `ambiguous`; nothing is deleted; shown in `brief` and `doctor` |
| neither present | `missing`; shown in `brief` |
| intent's holder Alive or Unknown | left alone; a second `file mv` on overlapping paths is refused (exit 7) |

Why keep an intent record when lazy re-binding would find the file anyway [13 §3.2]:
- it preserves intent, so an interrupted `rm` never looks "lost";
- it makes a multi-path move one logical operation;
- it covers move plus heavy edit, which content evidence cannot.

Cost: two flushes (~4 ms) plus the rename (p50 4–10 ms under load [M, 13 §1.4]) plus µs per op.

### 3.5 `file rm`: safe delete

`file rm` runs only in the writer tree of the caller's branch, like `file mv`. On every OS, the deletion or the move to `trash/` is followed by `durable-name` of the parent directory (or directories) before the commit ([80 §2.3.2]).

1. `--dry-run` is the default without `--yes`, following IntelliJ Safe Delete [D, 12 §4.2]. It prints the impact:
   - whole-file links;
   - span anchors;
   - glob entries that would match nothing afterwards;
   - prose mentions (report only);
   - pending intents.
2. With `--yes`: an `FsIntent`, then `unlink` (a directory needs `--recursive`), or `--trash`, which moves the file into `<store>/trash/<intent>/` by `rename_noreplace` when the store is on the same volume and refuses otherwise; then `durable-name` on the parent directory — for `--trash` on **both** parents, the source directory and the trash directory — before the commit, so a bugcheck or power loss cannot resurrect a file the graph records as removed ([72 M8]). These are the `ProjectFs`/`Vfs` classes; the per-OS calls are in [80 §2.3.2] (on Windows `DeleteFileW` or a `MoveFileExW` without `REPLACE_EXISTING`, then a `FlushFileBuffers` of each directory handle, 0.07 ms; `MOVEFILE_WRITE_THROUGH` only if M0 item 17 requires it); after a boot change `doctor` compares recent `FsIntentDone` outcomes with the file system. `moirai gc` purges trash after `gc.trash-expire` (14 days).
3. One commit:
   - status `removed`, `reason`, `replaced_by`;
   - with `--replaced-by Q`, whole-file anchors are re-pointed to Q's node, and span anchors are re-resolved inside Q, re-pointed if `fresh`/`moved`, otherwise kept on the removed node as `stale-anchor(orphaned)`;
   - globs are left and flagged;
   - referrers become `suspect` through the derived rule (§2.9).
   This is "node 40" for files: one commit, and every referrer is known through the reverse index [AR §5d.3].
4. Recovery follows the §3.4 table: the source still present means aborted; the source gone means roll forward.

`links fix ID --drop` records a removal that already happened, and changes nothing on disk. It works from any eligible tree, because it records a decision, not an observation.

### 3.6 `relink --after`, `revert`, `where`

- **`file relink P --to Q --after`** records a move made by other means (Mercurial's `rename --after`, TortoiseSVN's "Repair move" [D, 12 §4.6]). Q must exist in the caller's tree. Provenance is `owner/manual` or `agent/manual` by the actor's role. It asserts a move the actor performed; the printed next steps never suggest it for a proposal.
- **`file revert COMMIT`** runs the inverse filesystem operations of a commit that carried `FsIntentDone` through the same protocol, and records the link change.
- **History verbs never touch the working tree** [13 §3.5]: `undo`, `revert`, `cherry-pick`, `merge`, `sync`, `checkout` and `image import`. A plain `revert` of a file-moving commit warns ("this commit moved files on disk; `moirai file revert` moves them back"). The graph-only revert lands, and the next settle re-binds the link to wherever the file actually is, because links follow files (§4.1 P5).
- **`file where`** prints the current path per eligible tree, aliases, the provenance of the current path, pending moves, proposals and the `path_moves` entries that cover the path. **`--evidence`** adds, for each proposal: containment in both directions, sketch similarity, basename and directory relation, the git per-commit pair and its similarity, the candidate's creation time against `FILEOBS`, and the exact `links fix … --accept --expect PATH` command. This is the command packs print next to a proposal (§6.2).

### 3.7 `links check`, `links sync`, `links fix`, `links mentions`

| Verb | Writes? | What it does |
|---|---|---|
| `links check` | **never** (I-F5) | resolves the scope against the tree and prints states, evidence and proposals. `--deep` adds similarity search (still no write). `--strict` exits 6 when any link is not `ok` |
| `links sync` | **settle point** | resolves, then writes one commit with every exact re-bind that the write rule allows (§5.3), after the quiescence re-check (§4.2). It updates `FILEOBS`, keeps proposals, promotes `PENDING`, resolves merge path conflicts by observation where §5.5 allows, and records `path_moves` entries when §4.4 allows. In a reader tree it writes only `PENDING` rows. Default budget 2 s; `--deep` has a 10 s default budget, streams candidates, and is refused in quiet mode unless `--force` |
| `links fix` | yes (user evidence) | see below |
| `links mentions` | never | lists textual mentions, in node bodies and, with `--in repo`, in tracked text files (a read-only scan), of paths that resolve through aliases or `path_moves` to a current path. It covers cases like the owner's 188 verbatim references to 89 moved-away paths [M, 12 §3.5] |

**`links fix` actions** [41 M6]:
- `--accept --expect PATH` takes the current top proposal, re-evaluated at fix time, only if it still equals PATH (exit 6 otherwise). `--accept` without `--expect` is a usage error. Provenance is `owner/<evidence>/<score>` for the owner, `agent/<evidence>/<score>` for any agent role. An agent-accepted non-exact re-bind renders `[accepted guess c… · confirm: moirai links fix 815 --confirm]` in packs and `links check` until confirmed.
- `--confirm` turns `agent/*` into `confirmed/*`. It must come from an actor other than the acceptor, with a role the policy allows (default: orchestrator or owner; §9.2 decision 16).
- `--to PATH` is a manual path; `PATH` must exist in the caller's tree; provenance as for `--accept`.
- `--accept-replacement` answers `replaced`: it records a new observation at the same path (new `oid`, `relink` `owner|agent/replacement`) and re-resolves the anchors against the new content. `--drop` is the other answer.
- `--drop` removes (status `removed`). `--same-as ID` unifies nodes (§2.3). `--restore` reverts a node's last re-bind, or brings back a removed node (I-F14's explicit door).
- `--split` re-points span anchors to the pieces that hold their quotes and whole-file anchors to the piece holding the largest share of the old content; the old node becomes `removed{reason: split}`.
- On anchors: `--repin` recaptures at the current match **only** when that match is exact (anchor state `moved`, or an exact quote inside a uniquely resolved scope). After a fuzzy or scope-only match, `--repin` requires `--at SPEC` naming the intended place (exit 6 otherwise), because the nearest fuzzy match can be a sibling symbol [41 M6, §2 S13]. `--pin` turns the anchor into a historical citation at its captured commit; `--drop` removes the anchor.
- `--prefix FROM TO` confirms an inferred directory move: a `confirmed` `path_moves` entry plus glob rewrites.

### 3.8 Examples (the output contract of [AR §7.1])

```
$ moirai link 51 --at crates/engine/src/lock.rs::LockFile/acquire
branch: lane/l5np | rev 4471 | files @ <lanes-dir>/l5np (u/l5np 7c1e0a)
#51 at #812 crates/engine/src/lock.rs  a17 symbol LockFile::acquire L88-131 (header watched)  c4472

$ moirai link 51 --at lock.rs:120
error[ambiguous_path]: 'lock.rs' matches 3 files in <lanes-dir>/l5np:
  crates/engine/src/lock.rs
  crates/cli/src/lock.rs
  tests/fixtures/lock.rs
hint: write the root-relative path (exit 2)

$ moirai links check --scope 88
branch: lane/l5np | rev 4473 | files @ <lanes-dir>/l5np (u/l5np 7c1e0a, dirty 3)
42 links (30 files, 12 anchors) in 2.6 ms | 35 ok | 1 moved-auto | 1 moved-needs-confirm | 1 missing | 1 stale-anchor | 1 replaced | 2 absent-in-tree(behind)
#812 moved-auto           crates/engine/src/lock.rs -> crates/engine/src/sync/lock.rs   file-id | not yet recorded (moirai links sync)
#815 moved-needs-confirm  docs/plan/storage.md -> docs/plan/storage-v2.md   similar 0.81, runner-up 0.22 -> verify: moirai file where 815 --evidence
#820 missing              tests/quarantine_me.rs   since 3 min | no candidate | nothing written
#826 replaced             docs/perf/findings.md   re-created with unrelated content (containment 0.04/0.07) -> verify: moirai file where 826 --evidence
a31  stale-anchor(edited) crates/engine/src/log.rs::Log/append  0.86  was: "pub fn append(&mut self, rec: &Record)" -> verify: moirai file where a31 --evidence

$ moirai file where 815 --evidence
#815 docs/plan/storage.md  moved-needs-confirm  (tree <lanes-dir>/l5np, u/l5np 7c1e0a)
  candidate docs/plan/storage-v2.md: similarity 0.81 | old-in-new 0.84 | new-in-old 0.79 | same directory | basename differs
            git: c7d0e1 renamed storage.md -> storage-v2.md at 78 % | created 2026-09-26 09:40 (after the last verification)
  runner-up docs/plan/storage-notes.md: similarity 0.22
  accept after reading both files: moirai links fix 815 --accept --expect docs/plan/storage-v2.md

$ moirai links sync --scope 88
branch: lane/l5np | rev 4473 -> 4474 | files @ <lanes-dir>/l5np (u/l5np 7c1e0a)
recorded 1 exact move: #812 crates/engine/src/lock.rs -> crates/engine/src/sync/lock.rs (lazy/file-id)
kept 2 proposals (#815 #826) | 1 missing in grace (#820) | quiescence re-check 52 ms | commit c4474

$ moirai file mv docs/plan docs/archive/plan
branch: lane/l5np | rev 4474 | files @ <lanes-dir>/l5np (u/l5np 7c1e0a)
intent i-19 | 1 directory rename (9 ms) | 14 links re-pointed | path_moves += explicit project:docs/plan/ -> docs/archive/plan/
2 globs rewritten (files_owned of #89, applies_to of #212) | commit c4475
hint: git add -A -- docs/plan docs/archive/plan

$ moirai file mv docs/notes docs/archive/notes          # run in a harness worktree that is not bound
error[not_writer_tree]: <lanes-dir>/wf_<id> is not the writer tree of lane/l5np (bound tree: <lanes-dir>/l5np)
  bind it: moirai worktree bind <lanes-dir>/wf_<id> lane/x   (a branch has one designated tree)
  or move with a raw mv; links re-bind when the code reaches <lanes-dir>/l5np (exit 5)

$ moirai file rm docs/archive/plan/old-lock.md --replaced-by docs/archive/plan/lock.md
branch: lane/l5np | rev 4475 | dry run (add --yes)
#830 docs/archive/plan/old-lock.md would be removed (file deleted; node kept as removed)
  whole-file links: #51 #77 -> re-pointed to #831 docs/archive/plan/lock.md
  anchors: a40 (#88 heading "Lease reclaim") found in #831 (fresh) -> re-pointed
           a41 (#90 quote) not found in #831 -> stays on #830 as stale-anchor(orphaned)
  prose mentions: #212 body line 4 (report only)

$ moirai file mv crates/engine crates/core
error[fs_busy]: MoveFileExW failed with 32 after 1.0 s (4 retries)
  directory rename blocked: a process has an open handle or its current directory inside crates/engine
  (Restart Manager cannot name directory holders; common causes: a shell cd'ed inside, an editor, a watcher)
intent i-20 aborted; nothing changed (exit 7)
```

---

## 4. Automatic re-binding

### 4.1 Principles

| # | Principle | Why |
|---|---|---|
| P1 | **Reads compute, settle points write.** A read shows `moved-auto (not yet recorded)`. The versioned re-bind is written at the next settle point | [AR] makes `pack` a pure read, and readers take no locks. A write per read would also thrash on quarantine-and-restore (42 % of `mv` [M, 13 §1.1]) |
| P2 | **Only exact evidence is auto-applied.** A single exact candidate, with ties refused | similarity is probabilistic, and a wrong bind is silent (DR1) |
| P3 | **Absence is never deletion.** Missing links keep their edges and their last path, forever visible | Logseq lost references by tearing edges down on reload [C, 12 §4.1] |
| P4 | **Candidates are unbound paths only**: paths that are not the current path of another live file node on the branch view | removes most identical-copy ambiguity: an untouched mirror stays bound to its own node [10 §6] |
| P5 | **Links follow files.** The tree is the truth about where a file is. The graph records it, at settle, on the branch whose writer tree observed it | [13 §6.2] "the merged code tree, produced by git, is the authority" |
| P6 | **Never re-bind outside the root, into ignored output, into never-candidate names, into cloud-only entries, or into the Recycle Bin.** Those cases become `missing` with a note | quarantine moves go to scratch or `target`-like paths [M, 13 §1.1]; a Recycle Bin id resolves under `$Recycle.Bin` [M, 09 §2.2] |
| P7 | **Determinism and budgets.** Every step is a pure function of inputs that the command reads. Every trigger has a budget. When a budget runs out, the state is `unverified`, never a guess | DR8, DR10 |
| P8 | **Equal content is not identity.** An equal-`oid` file becomes a target only with corroboration (§4.3 copy rule) | agent backups, mirrors and vendored copies are pre-existing identical files [41 B3]; 1.2 % of files are in exact-duplicate groups [M, 10 §5.1] |
| P9 | **The tree's own committed content comes first.** Whether this tree moved the file, has not received the move, or is behind is read from τ(HEAD) before ancestry and before any search | ancestry alone fails after patch integration and history rewrites [41 M3]; the commit-graph misses every fresh HEAD [M, 41 §1] |
| P10 | **Quiescence before writing.** A settle re-checks every source path ≥ 50 ms after it last saw it absent, and drops the re-bind if the path came back | atomic saves rename the original away for milliseconds [41 M10] |

### 4.2 When it runs

| Trigger | Kind | Scope | Budget | Writes |
|---|---|---|---|---|
| `show`, `pack`, `brief`, `get`, `find`, `q`, MCP reads | read | links of the rendered nodes | ≤ 20 ms per command for file work (≤ 50 links ≈ 1–3 ms when present; ≤ 5 ms p50 when all sit in one moved directory) | never |
| `links check` | read | chosen scope | `--budget-ms` (default none for explicit) | never |
| `SessionStart` hook (`moirai hook session-start`, already in [AR §7.5]) | **settle** | links of the brief's items and the bound lane's `files_owned` links; the rest of the tree only when its last full settle is older than `files.settle.others-after` (24 h) ([70 S8]) | **150 ms** hard cap [13 §4.1], including the quiescence wait when it writes; ≤ 16 KB of log when nothing changed | exact re-binds on the branch, in its writer tree; `PENDING` in a reader tree; one `TreeReg` epoch and `FILEOBS` rows only for files that changed |
| `links sync`; MCP `write` with the named mutation behind `links sync` (`name` + `params[]`, §6.3) | **settle** | chosen scope | 2 s from the CLI (`files.links-sync-ms`); ≤ 200 ms per MCP call with a continuation cursor, so one call never stalls the server's other callers ([70 S9]) | yes |
| `link`, `file add`/`mv`/`rm`/`relink`, `links fix` | **settle** (for the links touched) | the command's links | — | yes (within the command's commit) |
| `complete ID` | **settle** | links of the completed task's subtree | ≤ 20 ms plus the quiescence wait if it writes, all outside the writer byte | yes, as a **separate commit after `complete`'s own**, so `tx.complete` stays a pure `TX` and the verb equals its named mutation ([72 M11]) |
| merge ritual: `links sync --tree <main tree> --since <pre-merge HEAD>` after the git merge commits [13 §5.4] | **settle** | links touched by the merged lane | 2 s | yes |
| git `post-commit` block (owner-installed, optional) | **settle** | links under paths the commit changed (tree diff against the first parent, in process) | 200 ms | yes |
| Harness move-evidence hook: Claude Code `PostToolUse` on `Bash(mv *)`, `Bash(rm *)`, `PowerShell(Move-Item *)`, `PowerShell(Rename-Item *)`, `PowerShell(Remove-Item *)`; Codex `PostToolUse` on `^Bash$` with an in-process filter of `${tool_input.command}`; Tier B per [90 §3.2] (`mcp_tool` on the session's server where connected, else an async command hook; under Codex `mcp_tool` or off) | **evidence** | linked paths under the parsed arguments, or under `cwd` when parsing fails | 100 ms | runtime `FILEOBS`/`PENDING` only (lazy records) |
| Harness edit-evidence hook: Claude Code `PostToolUse` on `Write\|Edit`; Codex `PostToolUse` on `^apply_patch$` once probe P5 of [90 §10.5] confirms its input field (paths parsed from the patch; a `*** Move to:` line is exact move evidence) (`files.hooks.edit-evidence = auto`: on with `mcp_tool` transport) | **evidence** | the edited file, if linked | 0.3–0.7 ms | its file id, stat quadruple, `last_oid`, the tree's dirty row and its `ANCHORRES` rows ([70 S7]) |
| `UserPromptSubmit` delta hook | read | links cited or leased by the agent: state changes since the session's last prompt | ≤ 600 B ([90 §9.2]) | never |

**Every settle write is CAS-guarded** ([72 M11]): a re-bind carries the file node's `rev_seq` as read when its resolution started, and is dropped (left to the next settle) if another writer changed the node meanwhile, so a stale observation never overwrites a newer one. Settles resolve outside the writer byte, quiescence wait included, and take it only to commit ([AR §4.5]).

Why hooks capture evidence and do not settle: a move followed by an edit destroys both exact signals, since the path is gone and a replace-by-rename editor gives the file a new id and new content [M, 09 §9.2]. A hook that fires right after the `mv` captures the file-id chain while it is still exact. The next settle turns that captured evidence into a versioned re-bind, even after the content has changed. Quarantine-and-restore produces no commits at all.

**Quiescence.** Every settle that would write at least one re-bind waits until ≥ 50 ms have passed since it last saw each re-bind's source path absent, re-stats those paths, and drops every re-bind whose source is present again. One wait per settle serves all its re-binds. The racy-entry rule of revision 1 (re-check entries whose mtime is at or after the scan start) is kept for candidates, but it cannot catch a renamed-away original, which keeps its old mtime [41 M10].

**Quiet mode** ([AR §6.6]). Evidence hooks and git hook blocks read the quiet flag from `HEAD` and exit before any other I/O. `SessionStart` does stat only: no hashing, no enumeration, no E6, no writes. `--deep` and `--all` are refused unless `--force`.

### 4.3 The file cascade

Inputs:
- file node F on the reading branch: `root`, `path` p, `oid` o, `observed_git` g, `observed_blob` b, `aliases` A, `status`;
- runtime `FILEOBS(F, T)`, which may be absent: file id, parent-directory id, stat quadruple, `last_oid`, `verified_at`;
- tree T with root R, and with git: HEAD H and its committed tree τ(H), read through the in-process git object reader ([60] M4).

```
0. ELIGIBILITY AND STATUS
   T not eligible (§5.1) → header "files: no tree bound"; every link unverified(no tree); stop.
   status removed → deleted.  status planned → §3.2.  root abs → existence and oid check only.

1. STAT p   (reads: GetFileAttributesExW, 17-67 µs [M 13 §1.7]; settles: directory enumeration with
             FileIdExtdDirectoryInfo, 12-22 µs per entry warm [M 09 §8]; per OS [80 §2.11.1]: statx (+ one
             name_to_handle_at for identity) and getdents64 on Linux, lstat and getattrlistbulk on macOS)
   cloud-only entry (§4.6) → decide from size, mtime and id only; content-dependent answers unverified(cloud-only)
   present → twin set (§2.4): only the twin whose content is on disk resolves; the others
             missing (not representable on this OS); no match or several → ambiguous (case/normalization collision)
           → case- or normalization-only difference under the directory's equivalence → ok ("spelling differs on disk")
           → stat quadruple equals FILEOBS → ok (FILEOBS.state may carry ambiguous(path reused), shown as such)
           → else ok, content changed if oid(p) ∉ {o, last_oid}  (hash only when anchors need the bytes, or at settle)
           → settle only: REPLACED test (§4.4)
           → settle only: file id ≠ FILEOBS.file_id → one OpenFileById(FILEOBS.file_id) (0.24-0.57 ms; macOS: fsgetpath;
             Linux: DIRMAP frontier, a hit counting only with the stored handle digest, else unverified(budget);
             identity = the whole OsFileId, [80 §2.11.4] rule 8); the original alive at q with a size+mtime or oid
             match → ambiguous (path reused; original at q), recorded in FILEOBS [72 M13]
           → settle only: rename-over/swap check — oid(p) equals the recorded oid of another live node G whose own
             path is absent → ambiguous(rename-over) for F and G; mutual → ambiguous(swap)
           → settle only, writer tree: τ(H) spells p with another case → re-bind to git's spelling (git/case)
   absent  → 2

2. TREE GATE (git present; §5.2). The first matching row wins.
   G1  p ∈ τ(H)                              → this tree moved or deleted it, uncommitted → search, E1-E5, E7, E8
   G2  g ≠ ∅ and g is an ancestor of H       → this tree received the observation and moved it in its history
                                               → search, E1-E8, with E6 over g..H
   G3  an alias of F is in τ(H)              → pending (this tree has not received the move); no search
   G4  otherwise                             → E6 over the integration window W, from p and from every alias:
         a chain that starts at p            → search result; this tree held p, so it is fresh for F
         a chain that starts at an alias only→ moved-needs-confirm (moved differently on this line)
         nothing                             → absent-in-tree: "behind" when H is an ancestor of g, else "diverged";
                                               unverified(commit not in this repository) when g is not in the local
                                               object store and W found nothing
   W = merge-base(g, H)..H when g is in the local object store; otherwise H's first-parent commits whose committer
       time is ≥ (the observation's hlc − 1 day), at most 2,000 commits (an R-14 constant).
   Git work is counted in fs units (1 per object decoded + 1 per 4 KiB inflated); a READ uses the GITFACTS and ANCESTRY
   caches plus at most one uncached ancestry pair and 32 E6 commits per command, else the link is
   unverified (git: moirai links sync | moirai check); settles and check run the full window [70 S6].
   No git: search E1-E5, E7, E8; freshness is trivially true (§5.8).

3. EXACT EVIDENCE. Candidates are unbound (P4), inside R, not ignored, not cloud-only, and match no never-candidate
   pattern (P6). Sources run in order; the first source that yields exactly one exact candidate q wins; otherwise
   the best STRONG result becomes the proposal.
   E1  intent   an open or recovered FSINTENT for F; PENDING rows for F from this tree; PENDING rows from other trees
                only when their captured oid equals oid(q) here [41 m5]. Exact when the recorded evidence class is
                exact (explicit intent, a file-id chain, a journal chain); a hook/argv row is STRONG
   E2  journal  (not built, [74 A13]; reserved: a USN chain from FILEOBS.file_id since the cursor, incl. the rename-over
                signature, verified present; revisit when a bound tree sits on a journaled volume whose journal outlives
                the median settle interval)
   E3d dir id   p's parent directory is absent at its path: OpenFileById(FILEOBS.parent_dir_id) (Linux: frontier over DIRMAP, a hit counting only with the stored
                handle digest; macOS: fsgetpath) → D′ (0.17-0.40 ms
                [M §0.3]; once per distinct parent per command) → q = D′/basename(p) present → exact only if q has
                FILEOBS.file_id, or equal size and mtime, or oid(q) ∈ {o, last_oid}; otherwise the REPLACED test of
                §4.4 runs at q → replaced, or moved-needs-confirm (directory moved, file replaced) [72 M13]
   E3  file id  OpenFileById(FILE_ID_128) + GetFinalPathNameByHandleW (0.24-0.57 ms [M 09 §8]; Linux: frontier, a d_ino
                hit counting only with the stored handle digest; macOS: fsgetpath) → q; identity = the whole OsFileId; exact if size and
                mtime equal FILEOBS, or oid(q) ∈ {o, last_oid}; otherwise STRONG (moved and edited in place)
   E4  near     enumerate dirname(p), its parent, and the targets of p's aliases and path_moves entries; hash only
                size-compatible files; oid(q) ∈ {o, last_oid} → COPY RULE
   E5  prefix   path_moves entries of class explicit, confirmed or committed that cover p: Y/rest present with
                oid ∈ {o, last_oid} → exact (the entry is recorded intent); present with another oid → STRONG.
                Entries of class observed, or ≥ 2 sibling nodes moved X/→Y/ in this pass → STRONG only
   E6  git      per-commit exact renames over the gate's window (g..H under G2, W under G4), computed in process from
                git trees: a path deleted and a path added inside one commit with an equal git blob id; first-parent
                chain; a merge commit is diffed against its first parent; renames are followed step by step [10 §5.9].
                Several deleted or added paths sharing one blob id in one commit → ambiguous, never paired by order
                [41 m6]. Inexact per-commit pairs are scored with git's old blob → STRONG/WEAK (§4.4)
   E7  tree oid (settle with budget left, or --deep; never on the tree's first settle) unbound files whose ChangeTime
                or CreationTime (Linux: ctime; macOS: ADDEDTIME or ctime) is later than T's last settle, plus the
                subtrees of directories changed since then (Linux and macOS: the DIRMAP frontier, [80 §2.11.3])
                [M 10 §5.5]; oid(q) ∈ {o, last_oid} → COPY RULE
   E8  edited   (settle) a file with p's basename that appeared since T's last settle, with sketch containment ≥ 0.8 in
                both directions against FPRINT(last_oid or o) → STRONG: moved-needs-confirm (edited+moved)

   COPY RULE for an equal-oid candidate q from E4 or E7 [41 B3]. The first matching line wins:
     E6 shows p → q inside one commit of the window, or E1/E2 names q           → exact
     q came from E4 and q.creation = FILEOBS.creation (a same-volume move keeps
       it [M 41 §1]; a MoveFileEx(REPLACE_EXISTING) rewrite tunnels it
       [M 09 §2.3], Claude Code's Edit/Write do not), VolumeCaps.btime =
       TunneledNotCopied, q shows no clone indicator, and q.creation is unique
       among the files E4 enumerated and differs from the recorded creation
       time of every other file node in that scope [80 §2.11.4 rule 1]         → exact
     q.creation ≠ FILEOBS.creation and q.creation < FILEOBS.verified_at
       (q existed while F was verified at p)                                    → a copy: never a candidate
     otherwise, including "no FILEOBS row for F in T" and a tree-wide (E7)
       candidate with an equal creation time                                    → STRONG: moved-needs-confirm (identical copy)
   Creation-time equality counts only for near candidates (E4), because a copy tool that preserves creation times
   defeats it [I]. E4 searches only p's directory, its parent and alias targets, where a same-name backup is
   already a never-candidate; a same-volume move keeps the file id anyway, so E3 finds most such moves first.
   It also counts only where it is unique: creation times are tick-granular, and a checkout creates
   many files per tick, so identical siblings (pkg/LICENSE, pkg/sub/LICENSE) share one [81 M5]; this fixes
   the line on Windows too. On Linux, where birth times are never unique, and on macOS, where clones copy
   them, the line never makes a candidate exact: at most STRONG.

4. SIMILARITY (links sync --deep / links check --deep only; never on reads or hooks):
   candidates C = unbound text files, ordered by same basename (148 of 150 exact renames kept it [M 10 §5.7])
   → same extension → nearby directory → new since the last settle. Candidates without a fingerprint are read and
   sketched on the way, streamed, within the budget (§7.2).
   stage 1: bottom-64 sketch against F's FPRINT, top 10;
   stage 2: re-read the ≤ 10 candidates (3.1 ms [M 10 §5.11]) and score symmetric similarity plus containment in both
   directions. The old side is exact when git's old blob can be read in process (by observed_blob), and is otherwise
   estimated from the sketch (σ ≈ 0.06 [I, 10 §8.2]). Score = max(line measure, token winnowing) when the old blob is
   available [M 10 §5.8b].

5. CLASSIFY (§4.4). Nothing → missing, or unverified if the budget ran out.
   E2/E3/E3d resolving outside R, into ignored output, into the Recycle Bin or the OS trash, or to a never-candidate name →
   missing ("moved to <place>"), never re-bound.

6. WRITE (settles only; §5.3): writer tree, fresh for F, quiescence re-check passed, committed on main.
```

**Never-candidate patterns** (resolver constants, R-14): `*.tmp`, `*.tmp.*`, `*___jb_tmp___`, `*___jb_old___`, `*~`, `*.bak`, `*.orig`, `*.old`, `*.rej`, `*.swp`, `*.swo`, `4913`, `.#*`, `~$*`, `sed??????` (MSYS `sed -i` temporaries), and, in a cloud sync root, `<stem>-<X>.<ext>` beside a linked `<stem>.<ext>` (§4.6). A candidate whose name is p's basename plus any suffix is never a target either [41 M10].

**What settle writes** (§5.3 decides where):
- For `moved-auto`: one `SetField(observation)` with path q, current `oid`, bytes, `observed_git` = H, `observed_blob` = git's blob id at q in τ(H) (empty if q is not committed), and `relink` = `<trigger>/<evidence>`, plus `aliases += p`.
- For `planned` → present: the first observation (§3.2 rule).
- For a qualifying directory (§4.4): a `path_moves` entry, plus glob rewrites for the `committed` class.
- Nothing else is versioned. States, proposals, `missing_since`, `last_oid`, file ids and `PREFIXEV` counts go to runtime tables.

### 4.4 Classification and thresholds (resolver v1 constants)

| Evidence | Condition | State | Auto-applied |
|---|---|---|---|
| exact: E1, E2, E3 unchanged, E3d, E5 via a recorded entry, E6 exact, E4/E7 passing the copy rule | exactly one candidate | `moved-auto` | **yes** |
| exact, tie | ≥ 2 exact candidates | `ambiguous` (≤ 3 candidates listed) | no |
| identical copy | equal `oid`, no corroboration, not a copy by the copy rule | `moved-needs-confirm (identical copy)` | no |
| strong | unique: E3 moved and edited in place; E5 with changed content or from an `observed` entry or sibling inference; E6 per-commit pair ≥ 90 %; E8 edited+moved; similarity ≥ 0.5 with margin ≥ 0.2 and one directory or basename corroboration | `moved-needs-confirm` | only under policy B (§9.2 decision 1) |
| split | at settle: an E6 inexact pair whose commit also added ≥ 1 other file, where ≥ 2 added files have new-in-old ≥ 0.8 and their old-in-new shares sum to ≥ 0.6 (old blob from git); or the same test in stage 4 | `moved-needs-confirm (split)` | no; `links fix --split` |
| merged | old-in-new ≥ 0.8 and new-in-old < 0.5 in a host file | `moved-needs-confirm (merged)` | no |
| weak | best in [0.3, 0.5) or margin < 0.2; git pair 20–49 % confirmed by containment | `moved-needs-confirm` or `ambiguous` | no |
| tiny file | < 5 normalised lines or < 64 B | exact only from E1, E2, E3, E3d and E6; otherwise `ambiguous`/`missing` | — |
| none | nothing ≥ 0.3 | `missing` | — |

Revision 1's row "exact, tie broken by name" is removed: picking one of several identical copies by basename is a guess, and it contradicted P2 [41 B3].

The thresholds are git's 50 % default [D, 10 §2.1] and [10 §7]'s measured rationale:
- 0.5 sits above the p90 background of unrelated files (0.24 symmetric, 0.29 containment);
- it sits below the p10 score at 30 % churn (0.62);
- the margin rule protects near-duplicate families.

**`replaced`** [41 M5]. At a settle, when p is present and all of these hold, the state is `replaced` ("path reused by different content since c…"), and nothing is written:
- `oid(p) ∉ {o, last_oid}`;
- the file id differs from `FILEOBS` (git's delete-and-re-add, `rm` then `Write`, and every Claude Code `Write` give a new id [M, 09 §3]);
- both contents are text with ≥ 5 normalised lines, and F is not `artifact_kind = generated`;
- sketch containment between the new content and the last observed content (`FPRINT(last_oid)`, else `FPRINT(o)`) is below 0.29, the p90 background of unrelated files [M, 10 §5.8], in both directions.
**`ambiguous (path reused; original at q)`** ([72 M13]). At a settle, when p is present but its file id differs from `FILEOBS` and a lookup of the recorded id — `OpenFileById` on Windows, `fsgetpath` on macOS, the `DIRMAP` frontier on Linux with a hit counting only with the stored handle digest (else `unverified (budget)`) ([80 §2.11.4] rule 8) — finds the original alive elsewhere (q) with a size and mtime or `oid` match, the state is `ambiguous (path reused; original at q)`: a directory promote-replace (`mv storage storage_old; mv storage_v2 storage`) no longer keeps a link `ok` on the v2 file, whose symbol anchor would otherwise resolve `fresh` there. It is recorded in `FILEOBS.state`, so reads with an unchanged stat quadruple render it too; `links fix --to q` or `--accept-replacement` settles it.

[41] also required a changed creation time. That condition is not used: Claude Code's `Edit` and `Write` change the creation time on every edit, and NTFS tunneling restores it when a file is deleted and re-created under the same name within about 15 s [M, 09 §2.3, §3.1], so it separates neither case. When E6 shows the path deleted in one commit and re-added in a later one, the detail names both commits. Fixes: `links fix --accept-replacement` or `--drop`.

**Automatic `path_moves` entries.**
- `committed`: in a writer tree, E6 shows every tracked file under `from/` renamed exactly to `to/` inside one commit of the window, and `from/` is absent from τ(H). The entry and its glob rewrites are written with the re-binds.
- `observed`: `PREFIXEV` shows every linked present node under `from/` re-bound exactly to `to/`, across any number of passes, `from/` no longer exists in T, and at least 2 nodes moved. The entry feeds aliases and E5 only. [41 M7] showed that revision 1's "every node in this pass" condition never fired under budgets; accumulation across passes fixes that.
- Otherwise the settle proposes `links fix --prefix FROM TO`.

### 4.5 The anchor cascade

This runs once the file resolved (`ok`/`moved-auto`) and its content changed since the anchor's cached result (cache key: anchor uid, file `oid`, resolver version — the runtime `ANCHORRES` table, written by settles and the edit-evidence hook and read by every process, so a CLI read of an edited file does not re-run the cascade another process already ran, [70 S7]). It does not run on `replaced` content or on the `text-unavailable` sub-state's quote step (§5.7). The scan streams through the fixed buffer: an exact or fuzzy quote search by chunks with an overlap of the longest selector, window alignment over the capped line-hash array ([71 RAM-M4]).

1. **Hint.** The normalised text at `hint` hashes to `span_hash` → `fresh`. For `header` watch, the header is compared.
2. **Marker** (opt-in prose only): marker found and the quote matches → `fresh`, else `edited`.
3. **Exact quote.** Search inside the scope region if a scope resolves uniquely, then in the whole file (`memmem` on the normalised text, 14–19 µs per 32-B quote on 350–450 KB files [M, 11 §2.8]). For a `range`, both quotes must be found in order, within twice the captured span length.
   - One hit → `moved` (or `fresh` at the hint).
   - Several hits:
     1. score each hit by exact prefix/suffix agreement, and keep a unique best with margin ≥ 0.1;
     2. otherwise score by window alignment (LCS of the stored window against the ±16 lines around each hit), and keep a unique best with margin ≥ 0.15;
     3. otherwise use `occurrence` within the scope if one was recorded;
     4. otherwise `ambiguous`.
   - Nearest-to-hint is **never** a tie-break. It picked the wrong one of 8 duplicates where a line diff was right [M, 11 §2.3].
4. **Fuzzy quote.** Myers bit-parallel matching with k ≤ ⌊0.25·|quote|⌋, first within ±16 KB of the hint, then in the scope, then in the whole file (0.17–0.69 ms windowed; ≤ 8.6 ms for a whole file [M, 11 §2.8]). Score = (50·quote + 20·prefix + 20·suffix + 10·window) / 100. Accept a quote similarity ≥ 0.75 with a top-2 margin ≥ 0.02 → `edited`, else `ambiguous`. **For a `symbol` or `heading` anchor whose scope did not resolve**, only headers of the same item kind (a `fn` for a `fn`, an ATX heading of the same level) are candidates, and the top-2 margin must be ≥ 0.1; a renamed `acquire` with a sibling `acquire_shared` therefore ends `ambiguous`, not pinned to the sibling [41 m8]. The `len/2` error budget is rejected, because it turned orphans into silent matches [M, 11 §2.7].
5. **Scope only.** The symbol or heading still resolves uniquely while the quote does not → `edited` (coarse).
6. **`lines` kind.** Align the window in the current file. The span's own lines are unchanged → `moved`/`fresh`; otherwise `orphaned`.
7. **Cross-file** (`--deep`, or when the file is `split`): exact quote over the pieces, or over files changed since the anchor's `git` commit → a proposal to re-point to another file node.
8. Otherwise → `orphaned`, rendered with the captured quote and commit.

**Watch semantics.**
- A `header`-watched symbol or heading whose body changed stays `fresh`, with a detail line `body changed since capture`. Referrers are not flagged, because a plan section or a function grows all the time.
- A `span`-watched anchor turns `edited` whenever its span changes.
- A `file` anchor with `watch = span` is a Doorstop-style content pin [D, 12 §4.10]: `edited` whenever `oid ≠ blob`.

Measured basis: on 1,180 real citations, quote plus context resolves 96.3 % overall and 90.1 % of those whose files changed, against 27.2 % for line numbers [M, 11 §2.3]. Over 4 months the cascade was 95.1 % correct and 0.9 % wrong, against 27.7 % valid line anchors [M, 10 §5.10].

### 4.6 Special patterns

| Pattern | Handling |
|---|---|
| Replace-by-rename edit at the same path (Claude Code `Edit`/`Write`, `sed -i`, `os.replace`, safe-write) | path present → `ok`; `FILEOBS` refreshes the file id at the next settle; a wholesale rewrite with unrelated content → `replaced` |
| Edit then move (no hook): 6 of 121 resolvable moved paths, 5.0 % [M, §0.3] | the stored id is dead and `oid` changed: E3 and E4 fail; E8 → `moved-needs-confirm (edited+moved)`; exact if a settle ran between the edit and the move |
| Move then edit (no hook) | E3 finds the id when the writer edits in place (strong, "moved and edited in place"); a replace-by-rename editor loses it → E8 |
| Either order, with the evidence hook | E1: captured exact → `moved-auto`. `mv a b && sed -i … b` in one shell call runs the async hook only after the whole call, so it is already edit-after-move: the hook records `PENDING{a→b, hook/argv}` as strong evidence, which E8 corroborates |
| Directory rename or move (by any tool that renames) | E3d: one `OpenFileById` per moved parent directory re-binds every linked file below it exactly; `PREFIXEV` accumulates towards an `observed` entry |
| Directory moved by git (checkout, merge; directories are re-created) | E6 over the window; a whole-directory rename inside one commit becomes a `committed` entry with glob rewrites |
| Module split | settle-time split detection from E6's inexact pair (§4.4); anchors resolve piece by piece (§4.5 step 7) |
| Rename-over (`mv b a`) | a's node F: present, content equals G's recorded `oid`; G: missing → both `ambiguous(rename-over)` with "b was renamed over a?" and a two-way fix |
| Swap (a↔b) | both present with each other's `oid` → `ambiguous(swap)` at settle; a read cannot see it when size and mtime are equal (no file id from `GetFileAttributesExW`, §2.6); `links fix --accept --expect` swaps both paths in one commit |
| Quarantine and restore | nothing written unless a settle point runs while the file is away *and* the quiescence re-check still sees it away; a settle into ignored/outside/temp paths is refused (P6); an in-tree quarantine re-binds and the restore re-binds back (links follow files); its directory moves are `observed` entries at most, which never rewrite globs |
| Atomic save racing a settle: JetBrains safe write (`x.rs` → `x.rs___jb_old___`, temp → `x.rs`, delete), MSYS `sed -i` temporaries, vim `4913`/`.swp`, Emacs `.#x`, Office `~$x` | the transient names are never-candidates, a name equal to p plus a suffix is never a target, and the quiescence re-check drops the re-bind when p comes back [41 M10] |
| Backup copy, then edit, then move (`cp lock.rs lock.rs.bak`; `Edit lock.rs`; `mv lock.rs sync/lock.rs`) | `lock.rs.bak` is a never-candidate (`*.bak`, p + suffix) and a copy by the copy rule; `sync/lock.rs` → E8 `moved-needs-confirm (edited+moved)`; never re-bound to the backup [41 B3] |
| Identical mirror (`.zcode/agents/dev.md` ≡ `.claude/agents/dev.md`); the linked copy's directory is deleted; first settle in a fresh worktree | no `FILEOBS` row → the copy rule never yields exact; E7 does not run on a first settle → `missing` (or an `identical copy` proposal from E4) [41 B3] |
| Case-only rename on disk (`mv a.rs A.rs` under `core.ignorecase=true`) | `ok (spelling differs on disk)`; nothing written; git's spelling is recorded once a writer tree's HEAD has it (`git/case`) or through `file mv` [41 M1] |
| Case or normalization twins committed on another OS (`docs/Plan.md` and `docs/plan.md`; NFC and NFD `café.md`) and checked out on a case- or normalization-insensitive volume | the twin whose recorded content is on disk resolves; the others `missing (not representable on this OS)`; no content match → `ambiguous (case collision)` / `ambiguous (normalization collision)`; never `ok` on a spelling match alone ([80 §2.11.4] rule 2) |
| `git switch` in a bound tree to another branch | the tree's HEAD leaves the lane's git line → it becomes a reader tree; the header says `reading only: tree on u/other, branch expects u/l5np`; nothing is written [41 M2] |
| Patch integration (`git apply` ×189, `cherry-pick` ×3 [M, 13 §1.1], rebase, `--amend`, squash), then a later move on trunk | G4: E6 over the integration window follows the chain from p → exact in the trunk writer tree; with no chain, `absent-in-tree (diverged)` is marked per link, never folded [41 M3] |
| Delete to the Recycle Bin | E3 resolves under `$Recycle.Bin` → `missing (in Recycle Bin)`; `links fix --drop` records it |
| Cross-volume move out of the root | `missing (moved outside root)` |
| Owner moves files in Explorer while no moirai process runs | seen at the next read (live); recorded at the next settle |
| Worktree strictly behind the observation | `absent-in-tree (behind)` (§5.2); no search |
| Image imported on a machine without the files, or without the git objects | an ineligible tree → `files: no tree bound`, no per-link work; `observed_git` missing from the object store → only the bounded E6 history search, then `unverified (commit not in this repository)`; never a filesystem search [41 M12] |

**Cloud-synced roots (OneDrive)** [41 M8]. OneDrive is present on the owner's machine [M, 41 §1]. Reading a `FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS` file or enumerating such a directory fetches content from the remote store [D, Microsoft Learn file attribute constants].
- Attributes come from the enumeration record or from `GetFileAttributesExW`, which read no content.
- In automatic paths (reads, settles, hooks), moirai never opens the content of an entry marked `RECALL_ON_DATA_ACCESS`, `RECALL_ON_OPEN` or `OFFLINE`, and never enumerates a directory marked `RECALL_ON_DATA_ACCESS`. It decides from size, mtime and file id only. An answer that needs content is `unverified (cloud-only)`.
- Explicit verbs (`link`, `links check --deep`, a cross-volume `file mv`) refuse to read a placeholder unless `--allow-hydrate` is given.
- On a sync conflict the online version keeps the name and the local copy gets the device name appended [D, Microsoft Support]. A `<stem>-<X>.<ext>` beside a linked `<stem>.<ext>` in a cloud root is therefore never a candidate, and `doctor` lists such copies beside linked paths, because the link then shows the other device's content (`ok, changed` or `replaced`).
- `doctor` detects cloud sync roots (cloud-filter reparse tags on the root or an ancestor) and warns. `files.cloud = refuse` refuses links under them (§9.2 decision 14).

**macOS iCloud Drive and File Provider dataless files** (`SF_DATALESS`; port phase): never read by an automatic path — `SF_DATALESS` is checked on every read path, and the process policy `IOPOL_MATERIALIZE_DATALESS_FILES_OFF` is a second line; answers that need content are `unverified (cloud-only)`; explicit verbs need `--allow-hydrate` ([80 §2.11.1]).

### 4.7 Accelerators

Git evidence is **not** an accelerator in this revision: HEAD-tree lookups, ancestry and per-commit renames are core resolver inputs, read in process through the git object reader ([60] M4), and nothing in R4 spawns git (§4.3, §5.2) [41 B2].

**USN journal (Windows) — not built** ([74 A13]): D: holds every repository and all worktrees and has no journal, and C:'s 32 MB journal keeps only 1–2 h under load, so E2 would contribute nothing on the owner's trees. The design below is kept as the specification for the revisit trigger (a bound tree on a journaled volume whose journal outlives the median settle interval), and `JournalCursor`/`JOURNALCUR` (formerly `UsnCursor`/`USNCUR`, now tagged for USN or FSEvents, [80 §2.11.2]) stay reserved so adding it is additive.
- `FSCTL_READ_UNPRIVILEGED_USN_JOURNAL` works through any handle opened with `FILE_READ_ATTRIBUTES`, without admin rights [M, 09 §4.1].
- Requirement: 8-byte-aligned buffers; a misaligned buffer returns error 87 [M, 09 §4.1].
- Records carry 128-bit ids, parent ids and reasons, without names. Paths come from `OpenFileById`, or from the settle's own enumeration.
- Replay follows [09 §4.3]:
  - a per-volume cursor;
  - if the cursor is below `FirstUsn` or the `JournalID` changed, the replay falls back to the walk;
  - records are filtered to ids in `FILEOBS` and their parent directories;
  - rename chains and the rename-over signature are recognised;
  - every result is verified in the tree.
- **D: has no journal** [M, 09 §4.2]. Creating one would be a one-time admin action on the owner's machine, outside moirai.

**FSEvents (macOS, port phase) — not built** ([AR §11] #41): the per-device history is the natural E2 on macOS: it carries paths and, since 10.13, inodes, and it replays after a reboot. Linux has no unprivileged persistent journal. E2 stays excluded on every OS until its trigger fires ([80 §2.11.4]).

**Harness hooks (optional; §9.2 decision 4).** The bullets below describe Claude Code; the last one states what differs in Codex; harnesses without these events rely on the git hooks below and on lazy settles ([90 §2.5, §3.2]).
- The `PostToolUse` evidence hook uses single-word `if` patterns. Multi-word patterns like `Bash(git mv *)` run anyway when a command contains `$()`, backticks or `$VAR` [D, Claude Code hooks docs, re-fetched 2026-09-26], and that is 37.6 % of the owner's Bash calls [M, 13 §1.1]. `PowerShell(Move-Item *)` also matches the aliases `mv`, `move` and `mi`, because "common aliases are canonicalized before matching" [D, Claude Code permissions reference, via 41 §4].
- The handler parses arguments only to *narrow which linked paths to stat*. It then decides from the filesystem [13 §4.4]. When parsing fails, it falls back to linked paths under `cwd`.
- `bashEditDiff` is not used: 99.1 % of recorded diffs are flagged `shared` [M, 13 §1.1].
- `FileChanged` with `watchPaths` is not used. A watcher holding subdirectory handles makes renames of ancestor directories fail with error 5 [M, 13 §1.4], and Claude Code's watcher implementation is unverified.
- The `Write|Edit` evidence hook refreshes the file id, stat quadruple, `last_oid`, the tree's dirty row and the `ANCHORRES` rows of an edited *linked* file, which makes edit-then-move exact and spares every later read the re-hash. Its default is `auto` ([70 S3, S7]): **on** whenever the hooks run as `mcp_tool` handlers on the session's moirai server (0.3–0.7 ms per edit, ≈ 7–17 s over the census's 23,754 `Edit`/`Write` calls [M, 13 §1.1]), **off** under command hooks, where each edit would cost a 15–73 ms spawn (6–29 CPU-minutes over the census); E8 still turns an unobserved edit-then-move into a proposal instead of `missing`.
- **Codex** ([90 §3.7]). Its `PostToolUse` matcher is a regex on the tool name only, so the move hook matches every shell call (`^Bash$`, also on Windows) and must be an `mcp_tool` handler that filters `${tool_input.command}` in-process — a command hook would spawn `cmd.exe` and moirai on every shell call — or be off. Edits arrive through `apply_patch`, whose patch text names the paths (`*** Update File:`, `*** Add File:`, `*** Delete File:`); its `*** Move to:` lines are exact move evidence. The `apply_patch` hook is installed only after probe P5 confirms its input field, because a missing field fails an asynchronous `mcp_tool` hook silently. There is no `if` filter; as above, the parse only narrows which linked paths to stat.

**Git hooks (optional; owner-installed).**
- `moirai hooks install --git` appends marker-delimited blocks (`# moirai-hook-start` … `# moirai-hook-end`) after graphify's existing blocks, in the shared `core.hooksPath` (one directory serves every worktree [M, 13 §1.3]). It never replaces a hook, and the role policy refuses it from agents.
- The blocks:
  - `post-commit` is a settle point for the committed paths. It is the only hook that sees `apply`, `--no-commit` merges and plain `mv` [M, 13 §1.5];
  - `post-checkout` (flag 1) marks the tree's cache stale;
  - `post-merge` settles the merged scope.
- `reference-transaction` and `post-index-change` are rejected: they fire up to 10 and 6 times per command [M, 13 §1.5].

### 4.8 Cost bounds and zero idle CPU

- **Reads.** 1 stat per link. A missing link adds at most: one HEAD-tree lookup (a few tree objects, ~0.1–0.5 ms est.); one ancestry answer per distinct `observed_git` (≤ 1 ms with a commit-graph, ≤ 5 ms without, [60] M4's budget); one `OpenFileById` per distinct missing parent directory (0.17–0.40 ms [M, §0.3]) or per link otherwise (0.24–0.57 ms [M, 09 §8]); ≤ 2 directory enumerations; ≤ 4 hashes. That is ≈ 1–5 ms per missing link, and one moved directory costs one lookup for all its links. When ≥ 4 rendered links share a directory, one enumeration of it (8–22 µs per entry) replaces their stats ([70 S8, S14]). The per-command cap is 20 ms, after which links render `unverified (budget)`; git work on a read is capped at one uncached ancestry pair and 32 E6 commits and counted in `fs` units (§4.3). Git facts computed on a read are kept for the command only.
- **Hooks.** Evidence ≤ 100 ms; SessionStart ≤ 150 ms including the quiescence wait; `post-commit` ≤ 200 ms. All run as foreground subprocesses and exit.
- **Explicit bulk verbs** (`links check --all`, `links sync --all`, `--deep`):
  - they enumerate each needed directory once (enumeration beats per-file stat by 3–20× [M, 09 §8]);
  - they hash only size-compatible candidates;
  - they may use **scoped worker threads** that end with the command (8 threads walked 3.4–4.2× faster [M, 10 §5.2]; `files.deep.threads`), at most 2 of which read file content at once (`files.deep.content-readers`), with 256 KiB stacks ([71 RAM-M4, RAM-m8]). This does not break [AR] T2's zero-idle-CPU rule: no thread outlives the command and no timer exists. It is recorded as a clarification for §8.4;
  - they never walk other worktrees. All 44 took 12 s warm and 123 s cold [M, 09 §8].
- **Quiet mode** ([AR §6.6]): §4.2.
- **Idle.** No moirai process has a timer, watcher or open project-file handle between requests, and no thread outlives a command except the MCP server's blocked stdin reader. The MCP server keeps a ≤ 256 KiB resolution LRU and 0 % CPU; anchor results shared across processes live in `ANCHORRES`.

---

## 5. Worktrees and branches

### 5.1 Tree identity, eligibility, and which tree a command uses

**Identity.** A tree is identified by its exact git top-level, read textually from the `.git` file or directory [AR §2.14], or, without git, by its bound directory. A nested checkout such as `.claude/worktrees/*` inside the main checkout is its own tree. [AR §5a.4]'s longest-prefix binding still resolves the *branch* of a command, but R4 designates trees by exact top-level only, so a binding of a directory never makes a nested worktree a designated tree [41 m13].

Paths are stored relative to their root (§2.4). The same `project:crates/engine/src/lock.rs` resolves separately in each of the 46 worktrees. The tree for a command is the first that applies [13 §5.1]:

1. `--tree DIR`;
2. the caller's cwd → its tree. For hooks this is the `cwd` input field, which "follows Claude" [D, 13 §1.6];
3. lease → run → lane → `lane.worktree_path` (MCP calls from any worktree);
4. branch → its designated tree (`worktree bind`);
5. `main` → `config files.main-tree`, for example the trunk worktree. The repository's main checkout can be hundreds of commits behind trunk [M, 13 §1.3]. If this is unset, `main` uses the main worktree and `doctor` warns.

**Eligibility** [41 M12]. The chosen tree gets per-link work only if one of these holds:
- it is the designated tree of some branch;
- it is a git worktree of the repository whose common directory holds the store (the placement of [AR §2.14]);
- it already has `FILEOBS` rows;
- `--tree` named it explicitly.

Otherwise, for example after an image import on a machine without the files, or in the directory that merely holds a `.moirai` store, the header says `files: no tree bound (moirai worktree bind DIR BRANCH)`, every link is `unverified (no tree)`, and nothing is stat'ed, enumerated or searched. Without git, `moirai init` binds its own directory as `main`'s designated tree, because creating the store there is the explicit act.

Every result names its tree on the first line: `files @ <lanes-dir>/l5np (u/l5np 7c1e0a, dirty 3)`. A reader tree adds why it is a reader: `files @ <lanes-dir>/wf_<id> (u/other 91aa02) · reading only: branch lane/l5np expects u/l5np`.

### 5.2 "Absent in this tree" versus "moved"

36 of the 44 worktrees [13] measured were stale ancestors of trunk, lagging it by 272–1,036 commits [M, 13 §1.3]. "Not here" is the normal answer for many links in many trees. The tree gate (§4.3 step 2) answers "should this tree contain this observation, and did this tree move it?" before any search.

It reads **the tree's own committed content first**: whether p, or an alias of F, is in τ(HEAD). Ancestry comes second. Revision 1 asked only "is `observed_git` an ancestor of HEAD?", which stays "no" forever after `git apply` (×189 in the census), `cherry-pick`, a rebase, an `--amend`, a squash, or a lane abandoned after its moirai merge. The next real move of such a file was then `absent-in-tree` folded into a header count, or `pending` forever, with no tree allowed to repair it [41 M3]. Trunk's first-parent line has 675 non-merge commits against 47 merges, and 132 commits are not reachable from trunk [M, 41 §1].

| Situation | Gate row | State | Search? | May write? |
|---|---|---|---|---|
| p is in τ(H) but not on disk: this tree moved or deleted it, uncommitted | G1 | the cascade's result | yes | yes (a writer tree; on `main` only once committed) |
| `observed_git` is an ancestor of H (or equal): this tree received the observation and moved the file in its history | G2 | the cascade's result, with E6 over `observed_git..H` | yes | yes |
| an alias of F is in τ(H): the move happened on a line this tree has not received | G3 | `pending` | no | no |
| otherwise, and E6 over the integration window finds a chain from p (patch integration, rebase, squash) | G4 | the cascade's result | E6 only | yes |
| … a chain from an alias only | G4 | `moved-needs-confirm (moved differently on this line)` | E6 only | no |
| … nothing, and H is an ancestor of `observed_git` | G4 | `absent-in-tree (behind)`: folded into the header count | no | no |
| … nothing, and the lines diverged | G4 | `absent-in-tree (diverged)`: marked per link | no | no |
| … nothing, and `observed_git` is not in the local object store | G4 | `unverified (commit not in this repository)` | E6 only | no |
| no git (R2) | — | the cascade | yes, never auto-delete | yes |

**Ancestry and HEAD trees are read in process.** The commit-graph chain of the owner's repository covers 1,796 of 1,889 commits, and misses the HEADs of trunk and of every active lane [M, 41 §1]; every fresh `observed_git` is newer than the graph by construction. Revision 1's fallbacks ("one cached `git merge-base` spawn per pair"; "where only the spawn-based backend exists") were spawn modes: 130–750 ms per spawn, 2.1 s under load [M, 10 §5.3], inside a 150 ms `SessionStart` cap, and interim stages the owner decision forbids [41 B2]. This revision makes the in-process git object reader of [60] M4 a hard dependency of the resolver. It reads loose objects, packs and `.idx` files, the split commit-graph chain (`objects/info/commit-graphs/commit-graph-chain`) with generation data (GDA2), commits and trees [41 m16]. Ancestry is a generation-pruned walk that parses only the commits beyond the graph (95 at the time of [41]'s probe), within [60] M4's budget of ≤ 1 ms per pair with a graph and ≤ 5 ms without. Settles cache answers as `GITFACTS` lazy facts; reads compute them for the command only. No R4 path starts a git process; `file mv --git` is the only exception, and only on request (§3.4).

### 5.3 Who may write a re-bind

**Writer tree** [41 M2]. Tree T may write re-binds for branch B only if both hold:
1. T is B's **designated tree**: `lane.worktree_path` for a lane, `files.main-tree` for `main`, or the single `worktree bind` of another branch. Binding uniqueness (I-F12) makes this one tree: `worktree bind` refuses a second tree for B (exit 5) unless `--replace`.
2. T's HEAD is **on B's git line**: its symbolic ref equals the binding's expected ref (`lane.git_branch`; `files.main-ref`, for example `integ/unified`, for `main`), or a detached HEAD descends from the binding's base commit and is an ancestor or a descendant of the expected ref's tip.

Otherwise T is a **reader tree**. It resolves and displays, and its settles write only `PENDING` rows.

Revision 1 compared only the tree's binding with the reading branch. A lane tree switched to another git branch (`git switch u/other`, ×45 in the census) then wrote that branch's paths to the lane, and the freshness rule locked the lane out after the switch back. Two trees bound to one branch flipped a path at each settle [41 M2].

| Observing tree | Versioned re-bind written on | Otherwise |
|---|---|---|
| the writer tree of `lane/x` | `lane/x` | — |
| the writer tree of `main` | `main`, **committed observations only**: q is in τ(H) (`observed_blob` set) | an uncommitted observation stays in `FILEOBS` (`uncommitted in the main tree: not recorded`) and is recorded at the next settle after the commit [41 m18] |
| a designated tree whose HEAD left the branch's git line | nothing | `PENDING` |
| any other eligible tree (`wf_*`, harness `isolation: worktree` trees, a second tree of one branch, nested `.claude/worktrees/*`) | nothing | `PENDING` |
| a writer tree that is not fresh for F (below) | nothing for F | `pending`, `absent-in-tree` or a proposal |

The main tree's committed-only rule exists because an uncommitted observation written to `main` would reach every lane at its next `sync`, and every lane tree would then re-bind it back.

**Freshness rule (I-F6).** Writer tree T is **fresh** for F, and may overwrite F's observation, only if one of these holds:
- F's `observed_git` is empty (no git, or a first observation made without git);
- `observed_git` is an ancestor of, or equal to, H (gate row G2);
- p is in τ(H): the tree's own commit holds the stored path (G1);
- E6 over the integration window found a chain that starts at p: the tree's history held the stored path (G4).

T has then seen what the current value was based on. The rule replaces [13 §4.1]'s "monotone re-binding" rule. It handles the window between the moirai merge and the git merge (§5.4) by data, not by history walking, and it lets restores and reverts converge on where the file actually is (P5).

**Conflict values.** For a `FieldEdit` conflict on the composite (§5.5), T resolves by observation only when it is fresh for **every** side's value, and only with exact evidence for the chosen path. Until then the link renders `ambiguous (merge conflict: b.rs | c.rs)`. [41 m18] proposed "the newest side". That is rejected: a trunk settle between the moirai merge and the git merge would then pick one lane's path before git has merged the other lane's code ([41 §2 S9]); waiting until the tree is fresh for every side makes the resolution an observation of the merged code.

**Promotion.** At a settle in writer tree T, a `PENDING` entry (F: p→q) from any tree is promoted to a versioned re-bind when p is absent in T, q is present in T, T is fresh for F, and either T's own evidence for q is exact, or the pending evidence was exact and E6 in T's window shows p renamed to q. Moves observed in reader trees thereby reach trunk once the code merges [13 §5.3], and never before (§9.2 decision 10).

### 5.4 The lane lifecycle, with files

1. `lane open l5np --worktree <lanes-dir>/l5np --git-branch u/l5np --base 7c1e0a` binds the tree with expected ref `u/l5np` and base `7c1e0a` ([AR §7.6] step 3). It is `lane/l5np`'s designated tree.
2. `file mv crates/engine/src/lock.rs crates/engine/src/sync/lock.rs` in `<lanes-dir>/l5np` lands on `lane/l5np`, with `observed_git` = the lane HEAD and `observed_blob` empty until the agent commits. `main` still says `…/lock.rs`, and that is consistent, because the trunk tree still has it.
3. A raw `mv` by another lane agent is visible live, captured as evidence by the hook if enabled, and written on `lane/l5np` at the next settle in `<lanes-dir>/l5np`. The same `mv` in a harness worktree of that agent is displayed there and kept in `PENDING` until the code reaches `<lanes-dir>/l5np`.
4. **Merge ritual** ([AR §7.6] step 8, extended):
   - `merge-check lane/l5np --into main` lists every lane re-bind that rests on an uncommitted observation (empty `observed_blob`): "commit the move first";
   - `--strict-links` also refuses `missing`, `ambiguous`, `replaced` and `stale-anchor` links of the lane's nodes;
   - then `moirai merge lane/l5np --into main`, then `git merge u/l5np`, then its commit, then `moirai links sync --tree <lanes-dir>/joltab --since <pre-merge HEAD>`.
   Between the moirai merge and the git merge, `main` says `…/sync/lock.rs` while the trunk tree has `…/lock.rs`. The alias `…/lock.rs` is in τ(trunk HEAD), so gate row G3 classifies the link as `pending`. The link is never re-bound back.
5. After the git merge, the path is present in the trunk tree (`ok`), or gate row G2 applies. The settle resolves any composite conflict by observation once the trunk tree is fresh for every side (§5.3).

### 5.5 Merge rules for link fields

These extend [AR §5a.7]. **The merge engine never reads the filesystem or git**, so merges stay deterministic, and imported foreign merges recompute identically in every store ([AR] I30′, I28′). Conflicts that only the filesystem can settle land as conflict values. A *later*, separate commit made by a settle in a writer tree resolves them, with provenance `merge-observation/<evidence>`.

| Key | Case (base → ours / theirs) | Rule | Result |
|---|---|---|---|
| observation composite | one side changed | take it | clean |
| | both changed, same path | take dst's composite (content drift is re-observed at the next settle) | clean |
| | both changed, different paths, and one side's root node gained, since the LCA, a `path_moves` entry of class `explicit`, `confirmed` or `committed` P→P′ that covers the other side's old and new values | **compose**: apply P→P′ to the other side's new path; add the pre-composition path to `aliases`; `relink = merge-compose/prefix` | clean; verified at the next settle |
| | both changed, different paths otherwise | `FieldEdit` conflict value on the composite | node `conflicted`; resolved by observation in a writer tree fresh for every side (§5.3); otherwise `links fix` |
| existence (derived-uid kinds) | the same uid created on both sides since the LCA, both live | equal existence; `created` = the creating commit with the least (generation, commit id); composite by the rows above | clean or `FieldEdit` |
| | created on side S since the LCA while the other side's final state for that uid is `removed` or engine-deleted | **re-key, never resurrect** [41 B1]: S's node becomes uid′ = uid(root, `origin_path`, U) — the uid a registration with predecessor U derives (§2.3) — with S's fields and `origin_pred = U`; S's anchors are re-pointed to uid′ in the merge commit; U keeps the other side's state and referrers | clean |
| | existed at the LCA; engine-deleted on one side, modified on the other | [AR]'s `DeleteVsModify` conflict value (no automatic policy for `artifact`) | conflicted |
| `status` | one side changed (for example `file rm` → `removed`) | take it; the other side's composite change is kept as data on the removed node | clean |
| | `planned` vs `present` | lattice join → `present` | clean |
| | both changed to incomparable values (`present` vs `removed`) | `StatusFork` conflict value. It is **never** resolved by path presence in the merged tree, because the file at that path may be an unrelated one [41 B1] | `links fix --drop` or `--restore` |
| (root, exact path) | two live present nodes with different uids at one exact path after the merge | `PathClaim` conflict value on both | settle unifies when E6 in a fresh writer tree shows an exact rename from one node's path to the other's; else `links fix --same-as` |
| anchor existence | add/remove | add-wins set on (src, dst, anchor uid); identical captures share a uid | clean |
| anchor selectors | one side repinned | take it | clean |
| | both repinned differently | `FieldEdit` | settle keeps the candidate that resolves `fresh` in the merged tree; if both or neither do, the conflict stays |
| globs | added or rewritten | add-wins set; entries added on one side are composed through the other side's `explicit`/`confirmed`/`committed` `path_moves` entries added since the LCA. A composition changes a key only one side touched, so a `sync` records the composed value in its residue — every key whose merged value differs from what `main`'s window alone yields ([AR §5a.3], [72 M5]) | clean |
| `path_moves` (root node) | any | add-wins union; composition applies entries in (hlc, from, to) order | clean |
| `at` to a node deleted with `rm` on the other side | historical class | tombstone ref, source `suspect` | clean, never `DanglingEdge` |

The dual-creation re-key is a pure function of the two histories, so every store computes the same uid′, and it equals the uid a later local registration derives from the removal (§2.3). A property test covers "removed on one branch, unrelated file re-created at the same path on another, merge" (§8.3.2 P13).

Git's `merge.directoryRenames` defaults to `conflict` [D, 13 §6.2]. So when git did *not* compose a directory move, a composed moirai path is corrected at the next settle: the pre-composition alias is checked by E4 and E6, and the link re-binds on exact evidence. Composition therefore only removes conflict noise in the common case, and observation keeps correctness.

**Rejected alternative:** a directory-node tree with Kleppmann moves over directory nodes [13 §6.2]. Git has no directory identity, a partial directory move has no single right answer, and it would add a second hierarchy beside `parent`.

### 5.6 History operations and the filesystem

No moirai history operation touches the working tree (§3.6). After an `undo`, `revert` or `cherry-pick` that changed a path, the next settle re-binds to where the file is, if exact evidence exists; otherwise the link is `missing`. The `revert` warning names `file revert`. `path_moves` entries are field values: a revert of the commit that added one removes it, and a cherry-pick adds it, through the ordinary set rules. Image import of a hand-edited `field path:` is a foreign `SetField`, validated like any op. It moves nothing on disk and shows at the next read as `moved-auto`, `missing` or `ok`.

### 5.7 Serialisation in the git image (R3)

**A file node** follows [AR §5b.2]. `title` is omitted, because it is derived. Fields are sorted bytewise:

```
moirai-node 1
uid: 3f0c9d0e5a8b4c7d91e2f3a4b5c6d7e8
kind: artifact
status: present
created: c9b2e6c1… 2026-09-21T14:02:11.483Z
updated: c4474a0f… 2026-09-26T09:12:40.118Z
field aliases: [crates/engine/src/lock.rs]
field artifact_kind: source
field bytes: 18231
field observed_blob: sha1:de177738b58e970465382658e69b18745029e248
field observed_git: sha1:75bea42e34a5942eb6593b2867480e8fbc95eed1
field oid: sha1:de177738b58e970465382658e69b18745029e248
field origin_path: crates/engine/src/lock.rs
field path: crates/engine/src/sync/lock.rs
field relink: lazy/file-id
field root: project
```

`origin_pred` appears only when the derivation had a predecessor.

**The root node** is an ordinary `area` node. `path_moves` uses the block form, one JSON array per entry, sorted by (hlc, from, to); the hlc is written as a 20-digit zero-padded decimal so that bytewise order equals numeric order:

```
moirai-node 1
uid: 9d41c0e2a7b35f18e6d04c9a2b7f3e51
kind: area
title: root:project
status: active
created: c9b2e6c1… 2026-09-21T14:02:11.483Z
updated: c4475b1e… 2026-09-26T09:20:03.551Z
field path_moves: <<
  ["00117336598351118336","explicit","docs/plan/","docs/archive/plan/","sha1:7c1e0a4d…"]
>>
field root: project
```

**Anchors** are lines in the **referrer's** `.moi`. The referrer owns its citations, just as it owns its out-edges ([AR]: out-edges are in the source file). The `at` adjacency is implied by the anchor lines and is rebuilt on import, so no `edge at` line exists. Anchor lines are sorted by (dst uid, anchor uid). Props come in a fixed order, and values are bare when they are safe and JSON-string-escaped otherwise, as for `conflict` lines:

```
anchor 7c2d4e6f8a0b1c3d5e7f9a1b3c5d7e9f -> 3f0c9d0e5a8b4c7d91e2f3a4b5c6d7e8 kind=symbol mode=live watch=header scope="rust:struct LockFile/impl LockFile/fn acquire" quote="pub fn acquire(&self, timeout: Duration)" prefix="/// Blocks until the writer byte is ours." suffix="-> Result<Guard> {" hint=88-131 window=k3Jd9Qa1… span=xxh3:9c1f0a2b3c4d5e6f blob=sha1:de17…e029e248 git=sha1:75be…eed1 captured=5e1f0c7a9b2d4e6f8a1c3e5f7b9d0a2c v=1
anchor 9a1b3c5d7e9f7c2d4e6f8a0b1c3d5e7f -> 3f0c9d0e5a8b4c7d91e2f3a4b5c6d7e8 kind=file mode=live watch=header captured=0b2d4f6a8c1e3a5c7e9b1d3f5a7c9e0b v=1
```

- **`window`** is base64url of the u16 hashes, with no padding.
- **Tombstone files** keep their anchor lines as retained out-edges, the same rule as [AR] I39′.
- **Commit mapping.** R4 adds **no trailer and no commit annotation**. A relink commit's provenance is in the node's `relink` field, and directory-move history is in the root node's `path_moves` field; both are part of the tree diff, so the importer rebuilds them by [AR §4.6]'s one rule for every commit kind, including checkpoint, sync, revert, cherry-pick and foreign commits [41 B4].
- **Never exported:** `FILEOBS`, `PENDING`, `FSINTENT`, `FPRINT`, `JOURNALCUR`, `DIRMAP`, `TREES`, `PREFIXEV`, `GITFACTS`, file ids, volume keys, creation times and mtimes (I-F4). Fingerprints are rebuilt by the importing store from readable content (§2.5).
- **Round trip.** Link intent is lossless at every granularity: paths, `oid`, the observation fields, aliases, removal intent, anchors, `path_moves` and derived uids, which recompute identically from their stored inputs. Resolution state is not carried, by design. Because `path_moves` is head state, a checkpoint-granularity export carries the full directory-move history, and a later merge composes paths identically in the exporting and the importing store ([AR §5b.7] gate 3).
- **Import validation.**
  - An `anchor` line whose dst uid is unknown is a tombstone ref, because `at` is historical.
  - A malformed anchor line is `ImageParse`.
  - A file node, root node or anchor whose uid does not match its derivation over its stored inputs is accepted as foreign and flagged in `image doctor`. The uid is then treated as random.
- **Anchor text** ([72 M6]). `quote`, `prefix` and `suffix` put source excerpts into the image: most of the ~18 MB of anchor lines at the owner's scale (§7.3). The canonical form hashes them only as BLAKE3-128 digests (`quote_h`, `prefix_h`, `suffix_h`, R-10), and every `anchor` line carries the digests; a destination with `image.dest.<name>.anchor-text = full` (the default) also carries the text, which the importer verifies against its digest (`ImageParse` on a mismatch), and `hash-only` omits the text for a destination whose access rights differ. Commit ids are identical either way, so a hash-only import verifies every native commit. An anchor imported without its text is in the **`text-unavailable`** sub-state: it resolves by hint, window and scope only, never `fresh` by quote, until `links fix --repin --at …` recaptures it from a tree. The destination's mode is recorded in the unhashed side ref, which I28′ names.

### 5.8 Independence from git (R2)

| Function | Without git | With git |
|---|---|---|
| link capture, anchors, explicit verbs, intent recovery | full | full; `--git` adapter optional |
| stat, case, E1–E5, E7, E8, similarity, anchor cascade | full | full |
| tree gate (`absent-in-tree`, `pending`) | no HEAD tree and no ancestry: a missing file always runs the cascade and is never auto-deleted | HEAD tree and ancestry through the in-process reader |
| E6 per-commit renames, old blobs for similarity stage 2, split detection at settle | unavailable (stage 2 uses sketch estimates; splits only under `--deep`) | available |
| `observed_git`, `observed_blob` | empty | set |
| writer-tree rule | the designated tree (no git line to check) | designated tree on the branch's git line |
| ignore rules | `.gitignore`-format files if present, else the `files.ignore` defaults (`target/`, `node_modules/`, `build/`) [41 m15] | `.gitignore` files, `.git/info/exclude` and `core.excludesFile` |
| git hooks, `--git` | n/a | optional |
| USN journal | not built ([74 A13]) | same |

A non-git project root is the directory bound by `worktree bind`, or the directory where `moirai init` created the store (§5.1).

---

## 6. Agent interface

### 6.1 CLI

The verbs are those of §3.1. The rules of [AR §7.1] and [16 §6.10] apply:
- ids first, one line per record, deterministic order;
- `--ids` and `--json v1`;
- exit codes 0–10;
- every result starts with [AR §7.1]'s header `branch: <ref> | rev <seq> | <n> rows`, and every file-bearing result adds `files @ <tree> (<git branch> <HEAD>, dirty N)`, or `files: no tree bound`, or a `reading only` note (§5.1);
- bare ids and bare `aN`.

`links check --json v1` emits one object per link:

```
{"v":1,"branch":"lane/l5np","rev":4473,"tree":"<lanes-dir>/l5np","writer":true,"data":[
 {"file":812,"anchor":null,"state":"moved-auto","path":"crates/engine/src/lock.rs",
  "now":"crates/engine/src/sync/lock.rs","evidence":"file-id","recorded":false},
 {"file":815,"anchor":null,"state":"moved-needs-confirm","detail":"similar","score":0.81,
  "candidate":"docs/plan/storage-v2.md","next":"moirai file where 815 --evidence"},
 {"file":811,"anchor":"a31","state":"stale-anchor","reason":"edited","score":0.86,
  "was":"pub fn append(&mut self, rec: &Record)","next":"moirai file where a31 --evidence"}]}
```

### 6.2 How links appear in packs and briefs

**Header segment.** `files @ <lanes-dir>/l5np: 38 ok | 2 moved | 1 missing | 3 absent-in-tree(behind)` (~45 characters). Only `absent-in-tree (behind)` is folded into the count [13 §7.1]. `absent-in-tree (diverged)`, `pending` older than 14 days and every other non-`ok` state are marked per link [41 M3].

**Per-link rendering.** A marker appears only on links that are not `ok`. The path is abbreviated to its basename only when the basename is unique in the rendered view [11 §4.6]. **No marker ever prints a command that accepts a guess**: proposals print the evidence command, and exact states print the settle command [41 M6].

| State | Rendering | ≈ characters |
|---|---|---|
| `ok` (span) | `lock.rs:88-131 LockFile::acquire` | 30–75 |
| `moved-auto` | `… [moved from engine/src/lock.rs \| exact]` | +45 |
| `moved-auto` accepted as a guess | `… [accepted guess c4480 \| confirm: moirai links fix 815 --confirm]` | +65 |
| `moved-needs-confirm` | `… [moved? -> storage-v2.md 0.81 \| verify #815]` | +44 |
| `ambiguous` | `… [ambiguous: 2 candidates \| verify #815]`; `… [ambiguous: path reused, original at storage_old/log.rs \| verify #815]` | +40–70 |
| `deleted` | `… [deleted c4480 "merged into y.rs" -> y.rs]` | +50 |
| `replaced` | `… [REPLACED: unrelated content since c4470 \| verify #826]` | +50 |
| `stale-anchor` | `… [EDITED 0.86; was: "pub fn append(&mut self, rec: &Record)"]` | +50–90 |
| `missing` | `… [missing since c4470, last at tests/q.rs \| verify #820]` | +50 |
| `pending` | `… [moved on lane/l5np -> sync/lock.rs; not in this tree yet]` (+ ` \| 16 d` when old) | +60 |
| `absent-in-tree (diverged)` | `… [not in this tree: lines diverged since c4410 \| verify #812]` | +60 |

Markers are ASCII (`->`, `|`; the leading `…` in the table stands for the link's own rendering and is not printed) and name the evidence command once per result through a legend line, `verify #N: moirai file where N --evidence` ([73 F13]). A typical pack with 1–3 non-`ok` links adds 15–50 tokens plus the header [13 §7.1]. Rendering is budgeted inside the pack's classes ([AR §7.4]):
- C3 lists the target task's links (its `at` anchors and `files_owned`);
- C7 selects hazards and notes by the **reverse index** file → anchors → referrers, plus `applies_to` globs.

"Notes about the files this task touches" thus becomes an index query instead of glob matching. `notes --path` uses the same index. Non-`ok` links of critical rules or owner rulings are never dropped below L1.

### 6.3 MCP changes

No new tool is added; the count stays at ten, because R5's `query` replaces `find` [50 §6.3]:

| Tool | Change |
|---|---|
| `get`, `pack`, `brief` | render links against the tree resolved from `tree`, Codex's `sandboxCwd` or `ctx.cwd` (the Claude stamp), the lease's lane, or the branch's designated tree ([90 §4.1]); the header names the tree and whether it is a writer |
| `query` (replaces `find`) | the former presets are named queries of [50 §4.1]: `links_broken` (not `ok`), `links_pending`, `links_proposals`, `links_guesses` (unconfirmed `agent/*` provenance), `files_removed`, `files_replaced`; the built-ins of §6.5 in free-form LQ |
| `write` | the named mutations behind `link --at` (`node, spec, watch, planned`), `unlink` (`node, anchor\|path`), `file relink --after` (`from, to`), `links fix` (`target, action, expect, …`; `accept` requires `expect`; `confirm` must come from another actor) and `links sync` (`scope, budget_ms`; a settle point), called through `write`'s `name` + `params[]`; the JSON op batch of the same operations (`link_file`, `unlink_file`, `record_move`, `links_fix`, `links_sync`) stays on the CLI's `apply` ([90 §6.6]) |

**Filesystem-changing verbs stay CLI-only** (`file mv`, `file rm`, `file revert`) [13 §3.1]:
- the MCP server is shared across worktrees;
- three roles have no `Write`/`Bash` tools, and file moves through moirai would bypass their tool envelopes [AR §7.3].

**Role policy rows** (added to [AR §7.3]):
- `link_file` for every role that may write the referring node;
- `links_fix` for the orchestrator, developer, tester, architect (for docs) and owner; `confirm` for the orchestrator and the owner by default (§9.2 decision 16);
- `links_sync` for every role, because it only records exact observations;
- `hooks install` for the owner only.

### 6.4 Hooks

| Hook | Handler | Effect | Default |
|---|---|---|---|
| `SessionStart` (existing) | `moirai hook session-start` | the brief plus a settle within 150 ms; the brief shows ≤ 3 link lines and a `moirai links check` footer when more exist; stat only in quiet mode | on |
| `UserPromptSubmit` (existing) | `moirai hook prompt` | ≤ 600 B delta, now including state changes of links the agent cites or leases | on |
| `SubagentStart` (existing) | role pack | links rendered as in §6.2 | on |
| `PostToolUse` — Claude Code: `Bash(mv *)`, `Bash(rm *)`, `PowerShell(Move-Item *)`, `PowerShell(Rename-Item *)`, `PowerShell(Remove-Item *)`; Codex: `^Bash$`, filtered in-process ([90 §3.7]); `async: true` | `moirai hook fs-evidence` (under Codex the `mcp_tool` handler `hook_fs_evidence`) | captures evidence (file-id chains, `oid` at the new path, argv-parsed destinations as strong evidence) into `PENDING`/`FILEOBS`; no versioned write. Fires on ~0.9 % of shell calls [M, 13 §4.2]. Exits after one flag read in quiet mode | **on** (§9.2 decision 4) |
| `PostToolUse` `Write\|Edit` (Claude Code); `^apply_patch$` (Codex, after probe P5) | `mcp_tool` `fs-evidence --edit` on the session's server | refreshes the file id, stat quadruple, `last_oid`, the dirty row and `ANCHORRES` of an edited linked file, so a later move stays exact and reads do not re-hash it | `auto` (on with `mcp_tool` transport, off with command hooks) |
| git `post-commit` / `post-checkout` / `post-merge` | `moirai hook git-*` | settle for committed paths; cache invalidation; merged scope. Exit after one flag read in quiet mode | not installed |

Every hook is fail-open and prints nothing the model sees; with `hooks.transport = auto` each runs as an `mcp_tool` handler on the session's moirai server where it is connected (Claude Code, Codex) and as an exec-form command otherwise ([AR §7.5]); a harness without these events relies on the git hook blocks and on lazy settles, which lose no link ([90 §2.5]). When the writer byte is busy, an evidence hook drops its record and exits 0; the next settle recomputes. The `PreToolUse` move nudge of revision 2 is not built (excluded by decision with a revisit trigger, [74 A17]).

### 6.5 Query language (R5) built-ins

Built-ins follow [14 §1 item 6] ("engine-derived state exposed as built-ins, so a query and a verb can never disagree"). This is the R4 part of LQ, in [50 §2.5–§2.6]'s spellings (settled at the integration of 2026-09-26; Review log):

| Built-in | Kind | Valid at |
|---|---|---|
| `file('crates/engine/src/lock.rs' [, root])`, `f.path`, `f.root`, `f.aliases`, `f.oid`, `f.status`, `f.relink`, `f.origin_path`, `f.observed_blob` | versioned (`PATHIDX`, then `ALIASIDX` with notice N11 naming the current path) | any view |
| `(n)-[a:AT]->(f:artifact)` — one binding per anchor, because the edge key carries the discriminator — with `a.kind`, `a.mode`, `a.watch`, `a.scope`, `a.quote`, `a.hint`, `a.anchor` | versioned | any view |
| `CALL root_moves('project') YIELD hlc, class, from, to, git` → the root node's `path_moves` entries | versioned | any view |
| `link_state(x)` — for an `AT` edge variable the state an agent sees for that anchor (§2.9); for an artifact its file state; for any other node the most severe state over its anchors (§2.9 order); `f.state`, `a.state` (the anchor-level state) | **tree-derived, live**: resolved by this design's resolver against the caller's resolved tree (§5.1 chain), stats and reads charged to LQ's `fs` budget, `unverified` for links the budget did not reach (exit 10); reads never write (I-F5) | tips only, with a resolved tree; at a past view error E302, not a value, because under LQ's two-valued logic `unknown` would satisfy `<> 'ok'` [50 §3.8] |
| `CALL links(scope: n) YIELD node, anchor, file, path, kind, scope, state, evidence, fix` — `links check`'s rows | tree-derived, live, as above | tips only, with a resolved tree |
| `applies(r, 'crates/engine/**')` | versioned glob match (LQ has no method calls; `r.applies(…)` is E004 with this rewrite) | any view |

`link_state` returns this design's frozen strings (§2.9), `replaced` included. Revision 2 of this design had specified a cached `link_status` read from `FILEOBS` as of the last settle; that contradicted P1 and §4.2 (reads, `q` among them, compute link states live), and a cached answer could disagree with `links check` on the same tree, so the live form of [50] is the one built.

`links check --scope 88` is the named query `std.links_broken` of [50 §4.1]:

```
MATCH (n)-[a:AT]->(f) WHERE n IN subtree(#88) AND link_state(a) <> 'ok' RETURN f, a, link_state(a)
```

and the two are tested for equality.

### 6.6 Skill additions (core `moirai` skill, ≈ 210–240 tokens: 840 characters [73 F9], inside the core skill's ≤ 800-token budget)

> Link files with `moirai link ID --at PATH`, `PATH:L-M`, `PATH::Type/fn` or `PATH#Heading`: root-relative paths, bare ids, quotes around anything with spaces; pass quoted text with `--quote-file`. Never write `#` or `@` at the start of an unquoted argument. To move or delete a linked file, prefer `moirai file mv` / `moirai file rm` in your lane's worktree: they keep every link and record why. Raw `mv`/`git mv` are fine; moirai re-binds exact moves by itself. A link marked `moved?`, `ambiguous`, `missing`, `REPLACED` or `EDITED` needs you: run the evidence command its `verify #N` names (`moirai file where N --evidence`), open the candidate, and accept only what you verified (`moirai links fix ID --accept --expect PATH`, or `--repin --at SPEC`). Your acceptance stays marked as a guess until the orchestrator confirms it. `moirai links check` never changes anything.

---

## 7. Performance and RAM budget

### 7.1 Unit costs (measured on the owner's loaded machine unless marked; upper bounds)

| Operation | Cost | Source |
|---|---|---|
| existence + size + mtime + creation time + attributes of one path (`GetFileAttributesExW`) | 17–67 µs | [M, 13 §1.7]; ~25 µs [M, 12 §3.6] |
| directory enumeration with 128-bit ids (`FileIdExtdDirectoryInfo`) | 12–22 µs per entry warm; ~10× cold | [M, 09 §8] |
| `OpenFileById` + path, file | 0.24–0.57 ms | [M, 09 §8] |
| `OpenFileById` + path, directory | 0.17–0.40 ms | [M, §0.3] |
| open + read + hash one file under Defender | ~150 µs warm, ~550 µs cold, + size at ~1.2 GB/s | [M, 10 §5.6] |
| similarity stage 1 against 1e3 / 1e4 / 1e5 **precomputed** sketches | 0.61 / 9.05 / 114 ms | [M, 10 §5.11] |
| similarity stage 2 (top 10 from disk) | 3.1 ms | [M, 10 §5.11] |
| anchor resolution: exact / fuzzy windowed / whole-file fuzzy | 10–40 µs / 0.17–0.69 ms / 1.4–8.6 ms | [M, 11 §2.8] |
| USN incremental replay | 1.1–9.2 ms | [M, 09 §4.2] |
| file rename p50 / max; directory of 1,000 files | 3.9–9.5 ms / 156 ms; 9.5 ms | [M, 13 §1.4] |
| ancestry of one pair, in process | ≤ 1 ms with a commit-graph, ≤ 5 ms without | [60] M4 budget (to be measured at M4) |
| HEAD-tree lookup of one path | ~0.1–0.5 ms for the first path in a directory, µs for the next | est.: 3–6 tree objects decoded from a pack |
| quiescence re-check | 50 ms per writing settle, shared by all its re-binds | R-14 constant |
| durable commit | ~2 ms p50 | [M, AR §8.1] |

### 7.2 Time budget by scale

N = tracked (linked) files. The anchor count is assumed at 3N. The owner's realistic scale is about 2.2k cited files and 10–35k citations [M, 11 §2.1], between the first two columns.

| Operation | N = 1e3 | N = 1e4 | N = 1e5 | Note |
|---|---|---|---|---|
| link rendering in `pack`/`show` (≤ 50 links, all present) | 1–3 ms | 1–3 ms | 1–3 ms | independent of N; gate (§7.4) ≤ 3 ms p50 idle, ≤ 5 ms loaded |
| … all 50 missing under one moved directory | ≈ 3 ms typical, ≤ 8 ms under load | same | same | est.: 2 × 50 stats + 1 HEAD-tree lookup + 1 directory `OpenFileById` + cached ancestry; gate (§7.4) ≤ 5 ms p50 warm |
| … with 10 linked files edited since the last settle | ≤ 5 ms p50 | same | same | `ANCHORRES` and the edit-evidence hook keep the results; without them 8–15 ms [70 S7]; gate (§7.4) ≤ 5 ms p50 |
| one missing link on a read (gate + E1–E5) | 1–5 ms | 1–5 ms | 1–5 ms | the 20 ms read cap bounds the total |
| `SessionStart` settle | brief- and lane-scoped links; the rest only after 24 h | same | same | hard cap 150 ms; ≤ 16 KB of log and 5–30 ms when nothing changed [70 S8] |
| `links check --all`, all present, warm | 17–67 ms | 0.17–0.67 s | 1.7–6.7 s per-path stat; 1.2–2.2 s by enumeration; ÷3–4 with scoped threads | cold ×5–10; explicit verb |
| settle writing k exact moves | +52 ms + µs × k | same | same | one quiescence wait + one commit |
| `--deep`, per missing link | 3 ms + stage 1 over the sketched candidates + 0.15–0.55 ms per candidate that must be read and sketched | same | same | an unsketched tree of 1e3 / 1e4 files costs ≈ 0.15–0.55 s / 1.5–5.5 s warm (est., [M, 10 §5.6] per file) and ~5× cold; streamed in basename-first order within the 10 s default budget [41 m3] |
| `file mv` of one file | 8–15 ms (+ process spawn) | same | same | intent + rename + commit |
| `file mv` of a directory with k linked files | 9.5 ms rename + ~2 ms + k `SetField` ops | same | same | 1e3 ops ≈ 40–60 KB |
| merge of a lane | + the typed rule per touched link key; `path_moves` composition O(entries added since the LCA × touched keys) | same | same | inside [AR]'s ≤ 50 ms gate at 1e5 nodes |

### 7.3 Disk and RAM

| Quantity | N = 1e3 | N = 1e4 | N = 1e5 | Derivation |
|---|---|---|---|---|
| file nodes (versioned) | 0.26 MB | 2.6 MB | 26 MB | ~260 B each (60-B header, uid, fields incl. a 45-B path, a 45-B `origin_path` and a 21-B `observed_blob`) |
| anchors (versioned, 3N) | 1.0 MB | 10.4 MB | 104 MB | ~345 B each (§2.7) |
| root nodes | < 1 KB | < 1 KB | < 10 KB | one per root; ~70 B per `path_moves` entry |
| indexes (`PATHIDX`, `ALIASIDX`, `ANCHORS`, reverse) | 0.1 MB | 1 MB | 10 MB | ~55 B per path + 16 B per anchor |
| `FILEOBS` (3 trees in use) | 0.3–0.45 MB | 2.9–4.5 MB | 29–45 MB | 96–150 B per (file, tree); rows for trees idle > 30 days are dropped by `gc` |
| `FPRINT` | 0.36 MB | 3.6 MB | 36 MB | ~300 B × ~1.2 content versions per file |
| `GITFACTS` | < 1 MB | < 1 MB | < 1 MB | per commit touched by E6 windows; derivable, dropped by `gc` |
| image growth (full export) | ~2 MB | ~20 MB | ~200 MB raw | ~0.45 KB per file `.moi` + ~0.52 KB per anchor line; owner scale ≈ 1 + 18 MB |
| **idle RAM**, all processes | 0 | 0 | 0 | nothing resident; MCP LRU ≤ 256 KiB |
| private RSS, read verb | ≤ 4 MB in total (+ ≤ 0.5 MB for fixed 128 KiB read buffers and a capped line-hash array, whatever the file size) | same | same | a 16 MiB file included [71 RAM-M4]; `files.max-read-bytes` (16 MiB) is the largest file whose content is examined |
| private RSS, hook / SessionStart | ≤ 1 MB | ≤ 1 MB | ≤ 1 MB | |
| private RSS, `links check --all` | ≤ 1 MB | ≤ 2 MB | ≤ 8 MB | indexes are mapped (shared pages); per-directory maps are transient |
| private RSS, `--deep` | ≤ 2 MB | ≤ 4 MB | ≤ 16 MB | candidates streamed; top-10 per missing link; ≤ 2 content readers at once, even with 16 MiB files present |

All versioned and runtime tables live in segments or the log. Only pages actually touched are resident, and they are shared through the page cache [AR §4.7].

### 7.4 Gates (added to [AR §8.1] and [60]'s release gate)

The timing and RAM rows are GT11 rows, measured on the owner's laptop — nightly in its agent-free windows and at each exit (profile L, [AR §11] #34) — never on the hosted runners of the public repository (#36), whose Windows Server images are not the gated profile; the assertions on synthetic data (log bytes, handles, placeholders on the simulator, the spawn lint) also run in the hosted PR checks, and the OneDrive and real-NTFS cases run on the laptop. None of these gates reads the owner-derived corpora of §8.3.4.

- Link work in a 50-link pack ≤ 3 ms p50 warm idle and ≤ 5 ms loaded when all links are present, ≤ 5 ms p50 warm when all 50 are missing under one moved directory [41 M7], and ≤ 5 ms p50 when 10 of the files were edited since the last settle [70 S7]; read-path git work ≤ 10 ms p99 on a trunk tree 1,000 commits past the observations [70 S6]; `SessionStart` settle ≤ 150 ms under a 16-agent load, including the quiescence wait, and ≤ 16 KB of log when nothing changed [70 S8].
- `links check --all` at 1e4 ≤ 0.7 s warm.
- Private RSS ≤ 4 MB for read verbs (also resolving anchors in a 16 MiB file) and hooks, ≤ 8 MB for `--all` and ≤ 16 MB for `--deep` at 1e5 with 16 MiB files present [71 RAM-M4].
- **Zero log bytes appended by any read verb** (I-F5), asserted by counting.
- No project-file handle open when a command returns (I-F11), asserted with a handle-count probe; no cloud placeholder hydrated by an automatic path (attributes unchanged, no read of a recall-flagged file), asserted on the simulator and on a OneDrive test folder.
- **No process spawned by any R4 path** except `file mv --git`, asserted by [60]'s spawn lint (GT20 (a), mandatory from M1, with its call sites from M4) extended to R4 [41 B2].
- 0 % CPU for the MCP server over 10 idle minutes, unchanged.

---

## 8. Build plan and tests

### 8.1 Placement by dependency on [60]'s milestones

[60] (issue 2) orders the build as M0 contract → M1 storage → M2 graph core → M3 version control → M4 git object layer (second lane from M1's `Vfs` certification point) → M5 git image → M6 file-link runtime → M7 query language → M8 CLI → M9 agent interface → M10 MCP → M11 release. R4 attaches to those components as follows. Every row is built to its final specification in the milestone named, and passes that milestone's gates before anything builds on it. The layer names FL-0…FL-9 are kept from revision 1 so that [41] and this file cross-reference; revision 1's FL-10 is dissolved (E2 and E6 are resolver inputs of FL-4 in M6, over the git API of M4; the hooks are FL-9 in M9). FL-5 is the CLI surface of the file and link verbs (M8); their semantics — capture, the intent protocol and its recovery — are `Store` API commands built with FL-4 in M6, as [60 §2.1] C6 places them. *This table was renumbered from [60]'s issue-1 order when [AR] integrated this design (2026-09-26, Review log); the content of each layer is unchanged.*

| Milestone | R4 content built there | Depends on | Size (est., Rust LOC excl. tests) | R4 exit criteria and gates |
|---|---|---|---|---|
| **M0** contract | **FL-0**: reservations R-1…R-18 in format v1; `.moi` ABNF additions (anchor lines, artifact and root-node fields, `pathmove` blocks, hash-only anchors); the resolver constant table (R-14); the link logic in the reference model's specification; M0 measurement item 15 extended (§8.3.6) | — | spec text | golden byte fixtures for every reserved record, section, value type and `.moi` line decode; spec review; [60 §2.5] carries R-1…R-18 (done 2026-09-26, §8.4) |
| **M0** (both lanes; [60 §3.1] items 6 and 11) | **FL-1** pure libraries: path rules and the fixed case fold; `is_text`, EOL normalisation, `oid`, normalised lines, sketch, token winnowing, similarity and containment; anchor capture and resolve; Myers bit-parallel (~100 lines [11 §3.4]); histogram line diff **shared with the diff3 of [AR] T10**; Rust/Markdown/TOML scope scanners; gitignore matcher; never-candidate patterns | none (leaf crates `sha1`, `sha2`, `xxhash-rust`, `blake3`) | 6–8k | unit and property tests; the offline replay of §8.3.4 meets its targets |
| **M1** storage engine | the reserved record kinds and sections (R-7, R-8) encoded, folded into segments, recovered and swept by the final multi-process protocol | M0 | ≈ 0.5k inside M1 | M1's crash enumeration and kill loops with R4 records in the generated workloads |
| **M6**, built in the second lane from M0 exit ([60 §3.7]) | **FL-2** `ProjectFs` seam (it has a test double, so [60] P1 allows it): enumerate with ids and attributes, stat, `OpenFileById` for files and directories, read with share-delete, `MoveFileExW`, delete, `RmGetList`, case sensitivity, long paths, cloud attributes, `sync_dir` (a directory flush, [72 M8]). Windows implementation (`windows-sys`) plus a **deterministic simulator** that models fault-model item (2)'s namespace crash semantics, file and directory ids, creation times and tunneling, replace-by-rename, sharing violations (32/5), the Recycle Bin, case-insensitivity, git-style checkout rewrites, cloud placeholders and the atomic-save sequences of §4.6; the `FsIntent` protocol on the simulator | the `Vfs` conventions frozen at M0 | 3–4k | the pattern matrix (§8.3.1) passes on the simulator and on real NTFS |
| **M2** graph core | **FL-3**: artifact and root-node fields; derived uids with predecessor selection, dead-uid re-derivation, `#N` reuse and verification; `at` edges with anchors and the discriminator; `PATHIDX` (exact keys in fold order), `ALIASIDX`, `ANCHORS`, reverse index; the `suspect` extension; `planned`/`removed`; I-F1–I-F3, I-F8, I-F9, I-F14 | M1 | 2–3k | property tests against the reference model (§8.3.2 P10, P13) |
| **M3** version control | **FL-7**: the merge rules of §5.5 — the observation composite, the dual-creation re-key and the `created` rule, `StatusFork` without presence resolution, `PathClaim`, `path_moves` union and composition, glob composition, anchors. ([60 §3.4] already places prefix composition in M3.) | M2 | 1–1.5k | P8, P13, P14 on random histories against the model; merge ≤ 50 ms at 1e5 unchanged |
| **M4** git object layer | R4's API on the reader, all inside [60] M4's scope: HEAD symbolic ref and detached HEAD; tree lookup by path returning blob id and mode; per-commit exact renames on the first-parent chain, merges diffed against their first parent, identical-blob groups reported as ambiguous; ancestry and merge-base over split commit-graph chains with GDA2 plus the commits beyond the graph; commit times | M1 | ≈ 0.3k of R4-facing API inside M4 | [60] M4's GT7, plus: per-commit exact renames equal `git diff-tree -M100%` over the owner's history; ancestry equals `git merge-base --is-ancestor` for all 46 worktree HEADs against trunk, including the 7 the commit-graph misses |
| **M5** git image | **FL-8**: artifact fields, root nodes, `anchor` lines and `pathmove` blocks in the `.moi` codec; derived-uid validation on import; no R4 trailer | M2, M3, M4 | 0.8–1.2k | [AR §5b.7] gates 0–3 with file nodes, anchors, `path_moves` and dual-creation re-keys in the corpus; gate 3 with composed directory moves at checkpoint granularity; P10, P14 |
| **M6** file-link runtime | **FL-4** resolver and settle: tree identity and eligibility; the writer-tree predicate and bindings (R-15, I-F12); the gate G1–G4 with the read-path git caps; the cascade E1 and E3–E8 (E6 over M4; E2 not built, [74 A13]); the copy rule and the E3d identity rule; the path-reuse check; `ANCHORRES`, `TreeReg` epochs and the dirty row; the settle CAS; streamed file reads; classification incl. `replaced` and settle-time splits; quiescence; the write rule, freshness and conflict resolution by observation; `PENDING` promotion; cloud rules; the runtime tables registered in M1's section registry; `link`, `unlink`, `file *`, `links *` as `Store` API commands with capture, the intent protocol and recovery (the semantics of FL-5) | M1–M4; FL-1, FL-2 | 6–7.5k | §8.3 end to end through a permanent file-link test driver; crash-point enumeration (§8.3.5); I-F5, I-F6, I-F7, I-F11, I-F13 asserted; P1–P7, P9, P11, P12, P15; the replay corpora of §8.3.4 re-run on the product; the budgets of §7.4 through the driver |
| **M7** query language | **FL-6** built-ins in the relation and built-in registry: the versioned `file()`, `AT` edge variables, `root_moves()`, and the tree-derived `link_state()`, `f.state`, `a.state`, `links()` in [50]'s spellings (§6.5) | M6; [50] LQ-2, LQ-4 | 0.5–1k | verb == named query on random graphs; `links_broken` = `links check` on the corpora; zero log bytes from reads |
| **M8** CLI | **FL-5**: the CLI surface of `link`, `unlink`, `file *`, `links *`, `hooks install --git` under the frozen contract; the §0.3 argv table as transport fixtures | M6, M7 | 1–1.5k | [60] M8's gates; the pattern matrix re-run through the binary |
| **M9** agent interface | **FL-9**: pack/brief rendering (§6.2), the hooks of §6.4 (`mcp_tool` or command transport), `hooks install --git`, the skill card; `links import` built and rehearsed ([AR §11] #23, decided 2026-09-26: it runs at the cutover after the owner reviews the ambiguous list; [74 A16]) | M6–M8 | 1–1.5k | hook payload fixtures; pack budget tests incl. the all-missing and edited-files gates of §7.4 |
| **M10** MCP | the ops, presets and role rows of §6.3 | M9 | ~0.3k | conformance tests |
| **M11** release | the R4 budgets of §7.4 inside the release gate | all | — | §7.4 on the owner's machine |

The total is about **22–30k lines of Rust plus 10–14k of tests** (est.). At the ratio of [60]'s first issue (≈ 2.4–3.3 units per 1k lines including tests) that is **≈ 53–99 units**, of which ≈ 22–40 are second-lane work (FL-1, FL-2). [60] issue 2 sizes each layer in its host milestone at ≈ 3 units per 1k lines and allocates **≈ 68–93 units** in total (the sum of the layer ranges below) (FL-1 19–25 in M0, FL-3 6–9 in M2, FL-7 3–4.5 in M3, FL-8 2.5–3.5 in M5, the whole of M6 29–39, FL-6 ≈ 1.5 in M7, FL-5 3–4.5 in M8, FL-9 and `links import` 3–5 in M9, the MCP operations ≈ 1 in M10), inside this band; its calendar already includes them ([60 §7]; [AR §9]).

**No interim modes** (DR13, [41 M9]). Revision 1 had a spawn backend for E6 and ancestry, and a "first complete layer FL-0 to FL-5 on one branch and one tree", with the tree gate, the freshness rule and the write rule in a later layer. Its resolver would have written without the gate until that layer landed. In this plan:
- the resolver's git inputs come from M4, which precedes M6; there is no spawn backend;
- the gate, the freshness rule, the write rule and the bindings are part of M6 itself, so the resolver and settle behave identically from their first build;
- E1 reads `PENDING` rows whether M6's reader-tree settles or M9's hooks write them; M9 adds writers of evidence rows and changes no rule;
- there is no "useful slice" milestone: the owner adopts at [60]'s release gate.

### 8.2 What is deliberately not staged

Each item below would have been a shortcut, and the design does not take it:
- **The runtime tables and the intent protocol ship with the first file verb.** There is no "record-only" first version of `file mv`.
- **Anchors ship with the quote, the window, the scope and the capture digest.** A line-number-only first version would rot 72 % of anchors in 4 months [M, 10 §5.10].
- **Derived uids, their stored inputs and the observation composite (with `observed_blob`) are in the first format.** Random uids first would force a later migration of every file node.
- **Directory-move history is versioned state from day one.** An op or a trailer could not be turned into state after images exist.
- **The git object reader is a hard dependency, not an accelerator.** A spawn-based fallback would be an interim stage and would break the hook budgets.
- **The writer-tree rule, the freshness rule and the copy rule are in the first resolver.** A resolver without them would write wrong facts that later versions could not tell apart from right ones.

### 8.3 Tests

#### 8.3.1 Move/edit pattern matrix (Windows, real NTFS, scratch directories only; the port phase runs the matrix on ext4, XFS, btrfs (including a subvolume boundary), a Linux casefold directory, and case-insensitive and case-sensitive APFS, with the rows [X19 §8.7] and [80 §5.3] add — directory-and-file inode reuse, empty-file reuse, `rm` + re-create at one path, case and NFC/NFD twins, identical files created in one tick)

Each row runs in a throwaway directory or git repository created by the test harness, never in a user repository. It is checked for:
- (a) the state on the next read;
- (b) the state and the versioned write after a settle;
- (c) (b) with the `PostToolUse` evidence hook;
- (d) (b) with the edit-evidence hook on (the USN variant of revision 2 is dropped with E2, [74 A13]);
- (e) the anchor states of a quote, a symbol and a heading anchor.

| # | Operation | Expected after settle (no accelerator) |
|---|---|---|
| 1 | rename in place; move to a sibling directory (`MoveFileExW`, Explorer `Shell.Application.MoveHere`, Git Bash `mv`, `Move-Item`, `os.rename`) | `moved-auto` (file id), recorded |
| 2 | rename the parent directory; move a directory under another parent | `moved-auto` (directory id) for every linked file below; an `observed` `path_moves` entry once all are re-bound |
| 3 | `git mv` + commit; a whole-directory `git mv` in one commit | `moved-auto`, recorded; a `committed` entry with glob rewrites |
| 4 | case-only rename on disk with `core.ignorecase=true`; then `git mv -f` + commit | `ok (spelling differs on disk)`, nothing written; after the commit, a `git/case` re-bind |
| 5 | replace-by-rename edit at the same path: temp + `MoveFileExW(REPLACE_EXISTING)` (the Claude Code `Edit`/`Write` shape [M, 09 §3.1]), `sed -i`, `os.replace`, a JetBrains-style safe write, `ReplaceFileW` | `ok`, content changed; anchors `fresh`/`moved`/`edited` as edited |
| 6 | in-place edit (`Set-Content`, append, vim) | `ok` |
| 7 | edit then move; move then edit (in-place writer); move then edit (replace-by-rename writer) | `moved-auto` if a settle ran between; else `moved-needs-confirm (edited+moved)`, strong, `moved-needs-confirm (edited+moved)`; with the hook: `moved-auto` in all three |
| 8 | copy + delete in the same tree; cross-volume move out and back | `moved-needs-confirm (identical copy)`; `moved-auto` when E6 corroborates |
| 9 | `git checkout`/`switch` in a writer tree on its line bringing a move (`merge --ff-only`); `rebase`; `stash`/`stash pop`; `reset --hard`; `restore`; `apply` + commit; `merge --no-commit` + commit | `moved-auto` via E6, or `absent-in-tree (behind)` when the tree is behind |
| 10 | `git switch` of a designated tree to another branch | reader tree: nothing written; the header names both refs |
| 11 | directory move of 131 files; a directory of 2,000 files with 300 linked, 40 of them agent-edited since the last settle | E3d re-binds all linked files exactly, edited ones included; one `observed` entry; the 50-link pack gate of §7.4 |
| 12 | module split `foo.rs` → `foo/mod.rs` + siblings, committed | `moved-needs-confirm (split)` at settle; anchors re-resolve per piece under `--split` |
| 13 | merge into a host file | `moved-needs-confirm (merged)` |
| 14 | identical mirror copies: one moved; the linked copy's directory deleted, then the first settle in a fresh worktree | `moved-auto` (file id); `missing` or `identical copy`, never a re-bind |
| 15 | `cp x x.bak`, edit `x`, `mv x d/x` | `moved-needs-confirm (edited+moved)` to `d/x`; never `x.bak` |
| 16 | rename-over (`mv b a`); swap | `ambiguous (rename-over / swap)` |
| 17 | quarantine to a scratch path, then restore; a restore that lands within 50 ms of a settle's scan | no commit; `missing (moved outside root)` in between; the quiescence re-check drops the re-bind |
| 18 | atomic save racing a settle: JetBrains `___jb_old___`, MSYS `sed -i` temporary, vim `4913`/`.swp`, Emacs `.#x`, Office `~$x` | no re-bind written; `ok` afterwards |
| 19 | delete; delete to the Recycle Bin; `git rm` + commit | `missing` (with the place); nothing written |
| 20 | path reuse: `rm` then an unrelated `Write` at the same path; a wholesale Claude `Write` with unrelated content; the same on a `generated` artifact | `replaced`; `replaced`; `ok, changed` |
| 21 | `moirai file mv`/`rm` with a holder open without share-delete; with a shell `cd`'ed inside; with a watcher on a subdirectory | exit 7, intent aborted, graph unchanged |
| 22 | `moirai file mv` killed at every step boundary, with the source or the destination edited in between | recovery per the §3.4 table |
| 23 | `moirai file mv`/`rm` in an unbound harness worktree; `file mv` of an alias this tree has not received | exit 5; exit 6 |
| 24 | 46-worktree shape: a stale tree reading a link created on trunk; a diverged tree | `absent-in-tree (behind)`, folded, no search; `absent-in-tree (diverged)`, marked |
| 25 | a lane move merged into `main` before the git merge | `pending` on the trunk tree (G3), then `ok`/`moved-auto` after the git merge |
| 26 | patch integration of a lane move into trunk (`git apply`, `cherry-pick`, squash), then trunk moves the file again | `moved-auto` in the trunk writer tree through the E6 chain from p |
| 27 | a second tree bound to a branch | refused (exit 5); with `--replace`, the old tree becomes a reader |
| 28 | OneDrive: a cloud-only placeholder linked; a conflict copy `name-DEVICE.ext` beside a linked file | no hydration (attributes unchanged, no content read); `unverified (cloud-only)`; the conflict copy is never a candidate; `doctor` lists it |
| 29 | image import on a machine without the files; with the files but without the lane commits | `files: no tree bound`, no per-link work; `unverified (commit not in this repository)` after the bounded E6 search, no filesystem search |
| 30 | link to a file > `files.max-read-bytes`; a long path (> 260); Cyrillic and NFD names; reserved names; quote input with a BOM or U+FFFD | handled per §2.4 and §2.7 |
| 31 | no git: quarantine into `target/` with no ignore file | `missing (moved into ignored output)` through the `files.ignore` defaults; never re-bound |
| 32 | directory promote-replace: `mv storage storage_old; mv storage_v2 storage` with anchors in `storage/log.rs` ([72 M13]) | `ambiguous (path reused; original at storage_old/log.rs)`; never `ok` on the v2 file |
| 33 | `rm src/net/x.rs`, an unrelated `Write` of `src/net/x.rs`, then `mv src/net src/transport`, all before a settle ([72 M13]) | `replaced` or `moved-needs-confirm (directory moved, file replaced)`; never `moved-auto` to the unrelated file |
| 34 | case twins from another OS ([80 §2.11.4] rule 2, [81 M4]): a view holding file nodes for `docs/Plan.md` and `docs/plan.md` (from a Linux-authored image, or registered in a case-sensitive NTFS directory), resolved in a case-insensitive NTFS directory that holds one of the two files; the same pair in a per-directory case-sensitive NTFS directory (`FileCaseSensitiveInfo` set) holding both; an NFC and an NFD `café.md` in one NTFS directory | case-insensitive: only the twin whose recorded content (`oid` in τ(H), or `last_oid`) equals the file on disk resolves `ok`, whatever spelling enumeration returns; the other renders `missing (not representable on this OS)`; neither is re-bound, and neither is `ok (spelling differs on disk)` on a spelling match; no content match, or several → both `ambiguous (case collision)`. Case-sensitive directory: two distinct files, both `ok`. NFC/NFD: NTFS is normalization-sensitive, so two distinct files, both `ok`, no twin set |
| 35 | identical files created in one tick, under the copy rule ([80 §2.11.4] rule 1, [81 M5]): one checkout writes `pkg/LICENSE` and `pkg/sub/LICENSE` with equal content and one creation-time tick, both linked; the node of `pkg/LICENSE` loses its file (deleted, or moved out of the root) before a settle | the creation-time line makes no candidate exact — the time is not unique in the E4 scope and equals another node's recorded creation time: `pkg/LICENSE` renders `missing` or `moved-needs-confirm (identical copy)`, never `moved-auto` to `pkg/sub/LICENSE` (P1) |
| 36 | a colliding checkout, which keeps the first name and writes the last content: `git checkout` into a case-insensitive NTFS directory of a commit holding `docs/Plan.md` (content A) and `docs/plan.md` (content B), so that one file named `docs/Plan.md` holds B; with both twins linked, with one, and with neither; then an edit of that file | the node of `docs/plan.md` (content B) resolves `ok` under the on-disk spelling `docs/Plan.md`; the node of `docs/Plan.md` renders `missing (not representable on this OS)`; nothing is re-bound or written; after the edit (no twin's content matches) both render `ambiguous (case collision)` |

#### 8.3.2 Simulator and property tests

These run on the FL-2 simulator. Random operation sequences from rows 1–36 are generated with **ground-truth identity** (the simulator knows which file is which). Generators include agent backups, mirrors and vendored copies, fresh worktrees, atomic-save races, path reuse, patch integration and branch switches in designated trees [41 B3]. Properties:

| # | Property |
|---|---|
| P1 | **No wrong automatic re-bind.** Every versioned re-bind with non-`owner`, non-`agent` provenance points at the ground-truth file (precision 1.0 for the exact class) |
| P2 | Every pure move inside the tree of a file whose id the tree recorded since its last edit, and every linked file under a renamed directory, is re-bound after one settle (recall 1.0 for these classes) |
| P3 | `resolve` is a pure function: same inputs → bit-identical output across runs and processes |
| P4 | Reads append nothing (I-F5) |
| P5 | Settle is idempotent: a second settle with no filesystem change writes nothing |
| P6 | Quarantine-and-restore between two settles writes nothing, and so does a restore inside a settle's quiescence window |
| P7 | A tree that is not fresh for F never writes F; a reader tree (unbound, off its git line, a second tree of the branch, a nested worktree) never writes a versioned re-bind |
| P8 | Merge determinism and commutativity for every key class of §5.5; merges never read the filesystem or git |
| P9 | Resolution by observation after a merge converges to the ground truth when exact evidence exists, and never runs before the tree is fresh for every side |
| P10 | `export → import → export` is byte-identical with anchors, `path_moves` and derived uids; two stores that import the same bundle derive the same uids; a case-insensitive Windows store and a simulated case-sensitive store register the same committed path to the same uid |
| P11 | Every anchor state agrees with the reference model's brute-force search, which enumerates all occurrences |
| P12 | Deletion is never inferred: no `removed` without an explicit door (I-F7) |
| P13 | **No resurrection** (I-F14): "removed on one branch, unrelated file re-created at the same path on another, merge" never resolves the old referrers to the new file, and the re-keyed uid equals the uid a local registration after the removal derives, in either order and in either store |
| P14 | A merge after a checkpoint-granularity export and import composes directory moves identically in the exporting and the importing store |
| P15 | After patch integration (`apply`, `cherry-pick`, rebase, squash) of a lane move, the next move on trunk is re-bound in the trunk writer tree or marked per link; it is never folded into a count |

The **reference model** is the naive in-memory Rust model that [60 §4] uses as its oracle. It gains a brute-force resolver: it scans every unbound file for `oid` equality, applies the copy rule by definition over the simulated creation times, and scores every candidate. The production cascade must return a subset-consistent answer: the same state, or a more conservative one, never a different target.

From M6 the link differential (GT2) sweeps `VolumeCaps` over the Windows, Linux and macOS profiles as input data, which tests the per-OS R-14 rules (the copy rule on Linux birth times and macOS clones, the twin rule on APFS, whole-id identity) on Windows. The ext4 (lowest-free inode reuse with random generations) and APFS behaviour profiles of the `ProjectFs` simulator, and a replay-corpus case built from a real checkout's one-tick creation times, are built in the port phase ([80 §5.2]); the one-tick case, the case twins and the colliding checkout run on real NTFS in M6 as matrix rows 34–36 (§8.3.1).

#### 8.3.3 Fuzzing ambiguous cases

- **Tree generators:** identical copies, mirrors and backups; near-duplicate families (shader variants, `mod.rs` ×87, `Cargo.toml` ×29 [M, 12 §3.5]); boilerplate-heavy files; tiny files; splits; merges; swaps; rename-over; Unicode (Cyrillic, NFC versus NFD); CRLF/LF mixing and lone CRs; long and reserved names; cloud-placeholder attributes.
- **Anchor generators:** duplicated quotes, reflow, re-indent, identifier renames, sibling symbols with shared prefixes, trivial lines, quotes spanning edits.
- **Oracle:** ground truth. **Assertion:** no silent wrong bind at the file or anchor level; every generated ambiguity surfaces as `ambiguous` or a proposal.
- **Parsers:** the `.moi` `anchor` line and `pathmove` block parsers and the authoring-spec parser are fuzzed as well.

#### 8.3.4 Replay corpora

These are read-only walks of the owner's repositories and transcripts, the same method as [10]/[11]/[13]. They are owner-derived: they stay in the gitignored local directory on the owner's laptop, never in the public repository and never on a hosted runner ([AR §11] #36, #37; [60 §3.1] item 7), and the gates that replay them run on the laptop.

| Corpus | Target |
|---|---|
| git rename history (201 renames, 186 exact [M, 13 §1.2]) replayed per commit with links on the old paths | 100 % of exact renames `moved-auto`; 0 wrong |
| the 1,180-citation sample of [11 §2.2] captured at authoring, resolved at HEAD | ≥ 96 % resolved (the quote result of [M, 11 §2.3]); window tie-break agrees with full-diff mapping in ≥ 99 % of duplicate-quote cases (§2.7); 0 silent wrong at the exact class |
| 188 verbatim references to moved-away paths [M, 12 §3.5] | `links mentions` finds all of them through aliases and `path_moves` |
| 301 memory paths, 58 dead with a unique git rename chain [M, 13 §1.2] | 58 re-bound by E6 |
| 34 delete-then-re-add events on all refs, 4 on trunk's first-parent line [M, 41 §1] | `replaced` wherever the re-added content is unrelated (containment < 0.29 both ways); never for a re-add of related content |
| the 46 worktree HEADs of the owner's repository | every gate decision equals a brute-force computation (tree listings plus `git merge-base` as the oracle), including the 7 HEADs the commit-graph misses |
| the transcript census of §0.3 | every resolvable edit-then-move becomes `moved-needs-confirm (edited+moved)`, not `missing` |
| Rust scope scanner against tree-sitter-rust (test-only oracle) on 1,573 files | name paths agree on ≥ 99.5 % of items |

#### 8.3.5 Crash-point enumeration

The engine's deterministic simulation enumerates a crash at every write, flush and publish boundary of the `file mv`/`rm` protocol, and between the filesystem call and each flush. Assertions:
- an intent is never lost;
- the graph is never half-applied;
- recovery matches §3.4 in every case, including the edited-source and edited-destination rows.

This is combined with sharing-violation and delay injection from the `ProjectFs` simulator and with OS-crash semantics for namespace operations: the simulator implements fault-model item (2), so a rename or delete not followed by its barrier can be lost while the commit survives, and the test asserts that the protocol's `MOVEFILE_WRITE_THROUGH` and directory flush make that impossible ([72 M8]); GT15 compares `FsIntent` outcomes with the file system after every reboot.

#### 8.3.6 Performance gates and M0 measurements

§7.4, run on the owner's laptop (Windows 11 with Defender on, profile L), idle and under the replayed 16-agent load fixture ([60 §5.1]). [60] M0 measurement item 15 (file-system costs through the `Vfs`) is extended with: `OpenFileById` on directories (measured here, §0.3), creation-time behaviour of `mv`, `cp`, Claude Code tools and tunneling on the owner's D: volume, directory-id stability on D:, and the attribute bits of a OneDrive placeholder read without hydration.

### 8.4 Consequential edits for other documents

This revision edits no other file. These are the edits it implies.

**Applied on 2026-09-26** by the integration step (see the [AR] Review log's integration entry and this file's Review log): every [AR] row below is carried by [AR] §2.2, §2.10, §3.1–§3.5, §4.1–§4.8, §5a.4, §5a.7–§5a.8, §5b, §5c, §5d.1, the new §5e, §6.4, §6.6, §7.1–§7.5, §8.1–§8.2, §9–§12; every [60] row by [60] §2.1, §2.5 (R-1…R-18), §2.6, §3.3–§3.10, §3.13 (GT17 rows 1–31) and §3.14; the [50] rows were reconciled in both directions — [50]'s spellings and live semantics for the link-state built-ins were adopted here (§6.5), and `replaced`, the six presets, `root_moves()` and the revision-2 field names were added to [50]. Milestone labels in the rows below are [60]'s issue-1 labels: "M8" is now M6 (file-link runtime).

**[AR] (`ARCHITECTURE-RESEARCH.md`):**

| [AR] section | Edit |
|---|---|
| T10, §1 row 1 | `sha1`/`sha2` also in the core `files` module; hand-written histogram diff shared by diff3 and anchors; no tree-sitter (unless §9.2 decision 9 says otherwise); scoped worker threads allowed inside explicit bulk verbs only |
| §2.14, §5c | R4 reads HEAD trees, ancestry and per-commit renames in process through the git object layer; no R4 path spawns git (the CM7 amendment of [60 §9] already removes the `check`/`stale` spawn) |
| §3.2 `artifact`, `area` | fields and statuses of §2.2 incl. `origin_path`, `origin_pred`, `observed_blob`; `sha256` → `oid`; derived uid; `area` gains `root` and `path_moves` |
| §3.3 | edge `at` (historical) with anchors and the edge discriminator; `finding.where` and file `cites` map to `at` |
| §3.4 | I-F1–I-F14; I1 and the import `IdCollision` rule adjusted for derived uids; I14 reads "oid read back" |
| §3.5 | `stale(artifact)` is replaced by the tree-derived link state; `suspect` gains "`at` target removed" |
| §4.2–4.4 | `next_anchor`; the record kinds and sections of R-7 and R-8 |
| §4.6 | anchor selector blocks as edge-property values of item 10; **no item 11** for R4 |
| §5a.4 | binding rows gain the expected git ref and base; one designated tree per branch; R4 designates trees by exact top-level |
| §5a.7 | the merge rules of §5.5, including the dual-creation re-key |
| §5b.2, §5b.4 | `anchor` lines and `pathmove` blocks; **no** `Moirai-Path-Prefix` trailer |
| §5d.1 | the runtime tables of §2.6 |
| §7.1–7.5 | the verbs of §3.1; the MCP ops and presets of §6.3; the pack header segment; the hooks of §6.4; role policy rows |
| §8.1 | the budget rows of §7 |
| §10, §11, §12 | the risks and decisions of §9; anti-requirements: no file watcher, no in-file IDs in code, no NTFS object IDs, no memory-mapping or holding of project files, no hydration of cloud placeholders, no git process on any R4 path |

**[60] (`60-roadmap.md`):**

| [60] section | Edit |
|---|---|
| §2.5 log | reserved R4 kinds per R-7 (`FsIntent`, `FsIntentDone`, `FsIntentAborted`, `FileObs`, `Pending`, `FPrint`, `JournalCursor`, `DirMap`, `TreeReg`, `PrefixEv`, `GitFacts`) |
| §2.5 commit body, canonical form, image format | R4 needs **no** commit annotations: drop `Moirai-Relink`, `Moirai-Relink-Evidence` and `Moirai-Move-Prefix`. If no other component needs the annotations block, drop canonical item 11 as well: an event inside a state-diff canonical form is exactly [41 B4]'s defect |
| §2.5 ops and values | value types `path`, `oid`, `pathmove` (R-1) |
| §2.5 segments | `PATHIDX` ordered by (root, fold(path), path), plus the other sections of R-8 |
| §3.3, §3.4 (M2, M3) | FL-3 in M2; FL-7's merge rules in M3 |
| §3.5 (M4) | the R4-facing API and exit criteria of §8.1's M4 row |
| §3.9 (M8; now §3.7, M6) | scope, dependencies and gates of §8.1's M6 row; E2 (USN) moves here from the accelerators |
| §3.10 (M9) | R4 accelerators are the hooks of §6.4 only |
| §3.14 | §9.2 decisions 13 and 15 are due before M0 exits (13 could add composite fields; 15 needs the hash-only anchor form in the ABNF); the others before M8 starts. *Superseded at the integration by §9.2's classification: owner decisions #3, #9, #13 before M0, #12 before the cutover; the rest are config keys or design rules* |
| §7 | R4 ≈ 53–99 units (est.) against the 20–31-unit placeholder; re-issue the calendar |

**[50] (`50-query-language-design.md`):**

| [50] section | Edit |
|---|---|
| edge table, `AT` row | anchor props per §2.7 (kind, mode, watch, scope, quote, end, occurrence, window, hint, blob, git, span_hash, captured) instead of the placeholder `symbol`, `line_hint`, `excerpt`, `context_hash` |
| `link_status(a)` | the vocabulary of §2.9, with the mapping of §6.5; add `link_checked(a)`, `root_moves(root)` and the procedure `CALL links.check($scope)`. *Superseded at the integration: [50]'s `link_state()`, `links()` and live semantics were adopted here (§6.5); [50] gained `replaced`, `root_moves()` and the six presets* |
| F13 | `PATHIDX` layout per R-8 |

---

## 9. Risks, failure modes and owner decisions

### 9.1 Risks and failure modes

| # | Risk / failure mode | Likelihood / impact | Evidence | Mitigation |
|---|---|---|---|---|
| 1 | A wrong automatic re-bind (identical copies, mirrors, backups, boilerplate, transient names) | low / high | 1.2 % of files are in exact-duplicate groups [M, 10 §5.1]; [41 B3, M10] | exact-only auto; the copy rule; never-candidate patterns; unbound-only candidates; ties → `ambiguous`; quiescence; P1 with copy generators |
| 2 | Moved and edited files end `moved-needs-confirm`, and agents ignore or rubber-stamp the proposals | medium / medium | git misses 7 of 16 inexact renames at 50 % [M, 10 §5.7]; edit-then-move 5.0 % [M, §0.3]; LLM compliance is probabilistic [AR risk 7] | the evidence command instead of a bare accept; `--expect`; guesses marked until confirmed; E8; revisit trigger in §9.2 decision 4 |
| 3 | File facts written to the wrong branch (reader trees, switched trees, the merge window) | low / high | 46 worktrees, 8 detached when [13] counted [M, 13 §1.3]; `git switch` ×45 [M, 13 §1.1] | the writer-tree rule, binding uniqueness, the freshness rule, `PENDING`, P7 |
| 4 | Derived-uid resurrection or collision | low / medium | 4 of 50 trunk deletions re-added at the same path [M, 41 §1] | exact-byte derivation; predecessors incl. removed and deleted nodes; the merge re-key; `PathClaim`; P13 |
| 5 | Anchor false matches (duplicates, loose fuzzy, sibling symbols) | low / high | 12.3 % of single-line citations ambiguous at authoring [M, 11 §0.2] | uniqueness at capture; margins; same-kind headers with margin 0.1; the window tie-break; `ambiguous`; `--repin` needs `--at` after fuzzy matches |
| 6 | The window tie-break underperforms full-diff mapping | medium / low | [I] | replay gate (§8.3.4); the in-process old blob as a further tie-break when git is present |
| 7 | Hand-written Rust scanner errors (macros, raw strings) | medium / low | — | the quote is the identity; a failed scope falls back to the whole file; tree-sitter oracle gate |
| 8 | Performance cliffs: cold cache, Defender, huge files, cold git packs | medium / medium | cold walks ~10× slower [M, 09 §8] | budgets → `unverified`; enumeration over stat; the stat cache; the size cap; `GITFACTS` caching at settles |
| 9 | Explicit moves blocked by open handles or cwd | high / low | error 32/5 measured [M, 13 §1.4] | bounded retry, diagnosis, exit 7, intent aborted cleanly |
| 10 | Unfinished cross-volume copy | low / medium | `MOVEFILE_COPY_ALLOWED` can leave the source [D, 13 §1.4] | copy → flush → verify → delete; recovery `ambiguous`; never deletes on doubt |
| 11 | ReFS / Dev Drive file-id semantics on cross-directory moves unmeasured | low / low | [09 §2.5] | file ids and directory ids are verified hints only; measure before relying |
| 12 | MFT slot reuse aliases a stored id | low / low | one slot reused 165 times [M, 09 §2.1] | 128-bit id with sequence; every hit verified by size/mtime, `oid` or relative name |
| 13 | Harness changes (hook `if` semantics, `PostToolUse` fields) | medium / low | the `if` filter is "best-effort" [D, hooks docs] | accelerators only; the parse only narrows; fixtures per Claude Code version |
| 14 | Growth of anchors and image size at extreme scale | low / medium | ~200 MB raw image at 1e5 files and 3e5 anchors (§7.3) | checkpoint-granularity image default [AR]; `gc` of fingerprints, git facts and idle tree rows; `image.dest.<name>.anchor-text` |
| 15 | Glob fields silently empty after moves outside moirai | medium / medium | Claude Code rule globs fail silently [D, 12 §4.9] | loud `glob matches nothing`; rewrite on explicit, confirmed or committed directory moves |
| 16 | A resolver-version bump flips many states at once | low / low | — | the version is stamped in results and anchors; a bump is announced in `brief` |
| 17 | The in-process git reader disagrees with git (deep delta chains, split commit-graphs, GDA2, a newer pack format) | medium / medium | the owner's repository uses a split chain [M, 41 §1] | [60] M4's GT7 against the git CLI, including the 46 worktree HEADs; a git upgrade re-runs GT7 |
| 18 | `replaced` fires on legitimate wholesale rewrites | medium / low | Claude `Write` gives a new id on every write [M, 09 §3.1] | `generated` artifacts exempt; the p90 background threshold; one-command `--accept-replacement` |
| 19 | Unconfirmed agent guesses accumulate | medium / low | — | the `links_guesses` named query; the count in `brief`; confirmations by the orchestrator |
| 20 | Some path hydrates a cloud placeholder | low / medium | OneDrive present [M, 41 §1] | I-F11; the attribute gate inside `ProjectFs`; pattern-matrix row 28 asserts no hydration |

### 9.2 Owner decisions

These are value or scope calls only; each had a recommended default. **Decided on 2026-09-26** (the owner's answers to [AR §11]: "Everything else I approve as you wrote it"): the four owner decisions below — #3 (a)–(c) ([AR §11] #21), #9 (#22), #12 (#23) and #13 (#20) — are decided as their recommended defaults; #3 (d) ([AR §11] #21 (d), macOS document ids) stays for the macOS port with its default, no, recorded. (Historical: decisions 13 and 15 shape format v1 and had to be settled before [60] M0 exits, the others before this design's runtime milestone; #13 is decided and #15 is now the config key `image.dest.<name>.anchor-text`.)

**Classification at the integration (2026-09-26)**, under the owner's rule that only what no configuration key can change later (format, branch model, merge semantics, identity, the product's code boundary, money or hardware, data leaving the machine) is an owner decision, and that runtime and operational policy is a documented `config` key with a default the owner may change ([AR §11]):
- **owner decisions** — #3 (may moirai write into repository files; [AR §11] #21, decided: no/no/no), #9 (tree-sitter in the product; #22, decided: test-only oracle), #12 (import the existing citations; #23 — decided 2026-09-26: yes, `links import` is built in M9 and run at the cutover, [74 A16]), #13 (explicit verbs outside the writer tree: refuse, or versioned pending-intent fields; #20, decided: refuse);
- **config keys** (R-13, [AR §13]) — #1 `files.policy.auto`, #2 `roots.<name>` (user scope) and `files.scratchpads`, #4 `files.hooks.evidence` and `files.hooks.edit-evidence` (now `auto`; git hook blocks stay an owner-run install command), #6 `files.deletion-inference`, #7 `files.mv-git`, #14 `files.cloud` (user scope), #15 `image.dest.<name>.anchor-text` (what may leave the machine at all is [AR §11] #16), #16 `files.confirm-roles`; the reference model and the tests implement every allowed value. By the priority audits ([74 A13, A17]) #5 (USN journal) and #8's nudge are no longer keys: E2 and the nudge are not built, each with a revisit trigger;
- **design rules, not decisions** — #10 (moves seen in reader trees stay pending) and #11 (splits are proposals): their alternatives would write unverified facts to a branch, which DR1 and I-F6 forbid.

"M8" in this section is [60]'s issue-1 label for this design's runtime, now M6.

| # | Decision | Options | Default (decided 2026-09-26 for #3 (a)–(c), #9, #12, #13; a config default for the keys; #3 (d) for the macOS port) | Consequence of the alternative |
|---|---|---|---|---|
| 1 | **What may re-bind without a decision?** | A: exact evidence only. B: also unique strong candidates (similarity ≥ 0.5 with margin ≥ 0.2, git pair ≥ 90 %, moved-and-edited by file id, edited+moved), marked | **A** | B removes most `moved-needs-confirm` prompts; the risk is a plausible but wrong file for a note, visible only through its marker. Under B an automatic strong re-bind renders like an accepted guess until confirmed |
| 2 | **Which files may be linked?** | project roots only; plus named roots (`memory`, other repositories); plus scratchpads | project + named roots, with re-binding inside each; `abs` for existence only; scratchpads refused | allowing scratchpads creates links that rot by design (session-scoped, GBs) |
| 3 | **May moirai modify repository files?** (a) rewrite textual path mentions after moves; (b) write opt-in markers (`<!-- moirai:a17 -->`) into moirai-owned Markdown; (c) create NTFS object IDs; (d) set macOS document ids (`UF_TRACKED`) on linked files ([AR §11] #21 (d), due before the macOS port) | yes / no, per item | **no / no / no / no**; `links mentions` reports | (a) has a long tail of misses and false positives [12 L14]; (b) makes prose anchors survive anything but edits repository files; (c) does not survive agent edits [M, 09 §2.3]; (d) makes rename-over and edit-then-move exact on macOS but writes a flag into the owner's files (ctime changes; git stores no flags) |
| 4 | **Accelerator hooks** (now config keys, [AR §13]) | the `PostToolUse` evidence hook for `mv`/`rm`/`Move-Item`/`Rename-Item`/`Remove-Item`; the `Write`/`Edit` evidence hook; git hook blocks | evidence hook **on**; `Write`/`Edit` hook **`auto`** — on when the hooks run as spawn-free `mcp_tool` handlers (0.3–0.7 ms per edit, [70 S3, S7]), off under command hooks; git blocks **not installed**; install `post-commit` if one campaign shows many `moved-needs-confirm` | under command hooks each event is one process spawn (15–73 ms, zero idle CPU), and the `Write`/`Edit` hook would have fired ~24k times over the census period [M, 13 §1.1]; git blocks add one spawn per commit beside graphify's |
| 5 | **USN journal on D:** — moot: E2 is not built ([74 A13]) | — | — | revisit when a bound tree sits on a journaled volume whose journal outlives the median settle interval |
| 6 | **Deletion inference** | explicit only (`file rm`, `links fix --drop`); also a deletion commit seen on the main tree marks `removed` | explicit only; git deletions render as `missing (deleted in git c…)` with a one-command fix | automatic marks remove a manual step but turn a git mistake into recorded intent |
| 7 | **Git index** | `file mv` never stages; `--git` by default | never stages; `--git` opt-in | default staging risks committing other agents' staged changes [13 §3.4] |
| 8 | **Agent discipline** | skill rule "prefer `moirai file mv`"; plus the nudge hook; plus a hard rule | skill rule only; the nudge is not built (revisit when raw `mv` of linked files exceeds a measured share after the skill card ships, [74 A17]) | a hard rule adds friction without closing the gap (Python and git moves) and would be a blocking hook, an anti-requirement |
| 9 | **Code-symbol scopes** | hand-written scanners (Rust, Markdown, TOML); tree-sitter (C runtime, +1.8 MB Rust, 8.9 MB with five grammars [M, 11 §2.9]) | hand-written, with tree-sitter as a test-only oracle | tree-sitter brings C code into the product and parse errors on 23.9 % of HLSL [M, 11 §2.9] |
| 10 | **Moves seen in reader trees** | stay `pending` until a writer tree sees them; write to `main` immediately | pending | writing immediately makes `main` wrong for the trunk tree until the code merges [13 §5.3] |
| 11 | **Splits** | a proposal plus `links fix --split`; automatic fan-out | proposal | automatic fan-out guesses which piece a whole-file link meant |
| 12 | **Existing citations** (34,195 `path:line` in docs [M, 11 §2.1]; 188 dangling references [M, 12 §3.5]; 301 memory paths [M, 13 §1.2]) | import once with `moirai links import` (~20 % need a human or agent choice [M, 11 §2.3]); leave them as legacy text | import (decided 2026-09-26, [AR §11] #23: `links import` is built in M9 and run at the cutover after the owner reviews the ambiguous list, [74 A16]); batches of one idempotency key each ([71 RAM-B1]); list the ambiguous ones for review | leaving them keeps today's silent rot in the old corpus |
| 13 | **Explicit verbs in trees that are not a branch's writer tree** [41 M4] | refuse (exit 5, print the bind command); version the intent as `pending_move`/`pending_removal` fields on the caller's branch, applied once the code reaches a writer tree | **refuse** | versioning the intent adds two composite fields to format v1 and a promotion path; refusing takes the explicit verb away from harness worktrees, where a raw `mv` is still caught lazily |
| 14 | **Cloud-synced roots** (OneDrive) [41 M8] | metadata-only (never hydrate in automatic paths; `--allow-hydrate` for explicit verbs); refuse links under cloud roots | **metadata-only**, with a `doctor` warning | refusing is simpler but blocks links to OneDrive documents; automatic hydration costs network traffic and disk, and changes the user's file state |
| 15 | **Anchor text in the image** [41 m17] (a config key) | full quotes; hash-only per destination (`image.dest.<name>.anchor-text`) | full for the owner's default destination (the separate bare repository); hash-only available per destination; commit ids identical in both modes (digests in the canonical form, [72 M6]) | hash-only anchors import in the `text-unavailable` sub-state and lose the `was: "…"` rendering until repinned from a tree |
| 16 | **Who confirms an agent-accepted guess** [41 M6] | the orchestrator or the owner; any second role; the owner only | orchestrator or owner, never the acceptor | owner-only turns confirmations into a backlog; any second role weakens the check |

---

## 10. Sources

**Internal** (all under `docs/`):
- `ARCHITECTURE-RESEARCH.md` (design of record)
- `research/design/60-roadmap.md` (roadmap of record)
- `research/design/41-file-links-critique.md` (the review this revision answers)
- `research/design/50-query-language-design.md`
- `research/09-file-identity-os-level.md`
- `research/10-content-based-move-detection.md`
- `research/11-in-file-anchors.md`
- `research/12-precedents-link-maintenance.md`
- `research/13-file-ops-agent-integration.md`
- `research/14-query-languages-and-llm-writability.md`
- `research/16-versioned-querying-mutations-safety.md` (§6.10 Windows transport rules)
- `research/design/30-synthesis.md`
- [research/design/80-cross-platform-design.md](80-cross-platform-design.md) [80] and its review [research/design/81-cross-platform-critique.md](81-cross-platform-critique.md) [81]
- [research/17-xplat-durability-mmap-memory.md](../17-xplat-durability-mmap-memory.md) [X17], [research/18-xplat-locking-ipc-processes.md](../18-xplat-locking-ipc-processes.md) [X18], [research/19-xplat-file-identity-change-tracking.md](../19-xplat-file-identity-change-tracking.md) [X19], [research/20-xplat-toolchain-shells-ci-crash-testing.md](../20-xplat-toolchain-shells-ci-crash-testing.md) [X20]
- owner decisions of 2026-09-26 (engine first, no SQLite, no interim stages) as recorded in the owner's private notes and in [60]

**External primary sources relied on** (verified by the cited reports on 2026-09-25/26; the hooks page was re-fetched for revision 1 on 2026-09-26; [41] fetched the permissions reference, git `convert.c`, and the two Microsoft pages on 2026-09-26):
- Claude Code hooks reference (`if` permission-rule filter, best-effort matching, `async` command hooks, `PostToolUse`): https://code.claude.com/docs/en/hooks
- Claude Code permissions reference (PowerShell rules; aliases canonicalized before matching): https://code.claude.com/docs/en/permissions
- git `convert.c` (`gather_stats`, `convert_is_binary`, safer autocrlf): https://github.com/git/git/blob/master/convert.c
- Microsoft Learn, file attribute constants (`RECALL_ON_DATA_ACCESS`, `RECALL_ON_OPEN`, `OFFLINE`): https://learn.microsoft.com/en-us/windows/win32/fileio/file-attribute-constants
- Microsoft Support, duplicate files in OneDrive (device name appended on a conflict): https://support.microsoft.com/en-us/onedrive/duplicate-files-in-onedrive
- W3C Web Annotation Data Model (TextQuoteSelector, RangeSelector, refinedBy): https://www.w3.org/TR/annotation-model/
- Microsoft: `MoveFileExW`, `ReplaceFileW`, `OpenFileById`/`FILE_ID_INFO`, `FSCTL_READ_UNPRIVILEGED_USN_JOURNAL` (Windows SDK `winioctl.h`), `RmGetList`, `MoveFileTransactedW` (TxF deprecation), ReFS and Dev Drive feature tables. URLs in [09 §10] and [13 §11]
- git: `diffcore-rename.c`, `git-diff -M`, `merge.directoryRenames`, githooks, racy-git, commit-graph chains and generation data. URLs in [10 §11] and [13 §11]
- Precedents (Obsidian, VS Code/LSP file operations, IntelliJ Safe Delete, Unity/Godot/Unreal, Doorstop, Hypothesis, `hg addremove -s`, `p4 reconcile`): URLs in [12 §9] and [11 §7]

**Probes** (probe scripts are not published):
- revision 1: `argv.py` (§0.3);
- this revision: `dirid.py` (directory ids; its scratch tree is deleted by the script) and `editmove.py` (transcript census; aggregate counts only, nothing written);
- [41]'s probes (`cgcover.py`, `pathreuse.py`).
No web request was made for this revision.

---

## Review log

Revision 2 (2026-09-26) answers [41]. "Adopted" means [41]'s fix is part of the design as written above; "changed" means adopted with a stated modification; "rejected" gives the reason.

### Blockers

| Issue | Resolution | Where |
|---|---|---|
| **B1** derived-uid resurrection and machine dependence | **Adopted, one change.** Derivation over exact bytes (git's HEAD spelling, else the enumerated spelling), never folded; case collisions detected in `PATHIDX` order and at resolve time; predecessor chosen by (generation, commit id) among removed, engine-deleted and aliased nodes; dead uids re-derived at registration; merge rule "re-key, never resurrect" for a dual creation against a removal, landing on the same uid as a local re-derivation; `StatusFork` never resolved by path presence; `created` = least (generation, commit id); stored derivation inputs (`origin_path`, `origin_pred`) make every uid verifiable; I-F14; P10, P13. Change: `origin_*` fields are added so that derivations can be checked on import, which [41] did not require | §2.2, §2.3, §2.4, §2.10, §5.5, §5.7, §8.3.2 |
| **B2** no ancestry source on the daily path; spawn fallbacks | **Adopted.** The in-process git object reader ([60] M4: loose, pack, idx, split commit-graph chain with GDA2, commits, trees) is a hard dependency of the resolver; ancestry is a generation-pruned walk over the commits beyond the graph; answers cached in `GITFACTS`; every spawn fallback removed; the only git process is the opt-in `file mv --git`. [AR]'s v1/v1.1 split of the object layer is already resolved by [60 §1.3]; the matching [AR] edit is listed | §0.1, §4.3, §4.7, §5.2, §7.4, §8.1, §8.4 |
| **B3** exact-`oid` re-binds to pre-existing copies | **Changed.** The copy rule (a git rename in one commit, captured intent, or, for near candidates only, an equal creation time corroborates; a candidate created before `verified_at` with another creation time is a copy; otherwise `identical copy` proposal). Change: creation-time equality does not make a tree-wide (E7) candidate exact, because a copy tool that preserves creation times would defeat it; E7 never on a first settle; the basename tie-break removed; the never-candidate list extended (incl. the OneDrive conflict suffix) plus the p + suffix rule; I-F13; P1 generators; matrix rows 14–15 | §4.3, §4.4, §4.6, §2.10, §8.3 |
| **B4** `PathPrefix` as an event in a state-diff canonical form | **Changed.** Directory-move history is the `path_moves` set field on a per-root `area` node, merged as an add-wins union and composed in (hlc, from, to) order; item 11 and the trailer are gone; R-5 and R-10 rewritten. Change: an entry carries the recording commit's hlc and the observing git commit instead of [41]'s `commit16`, because a commit cannot contain its own id (the id hashes the changeset that would contain it); and entries carry a class, so lazily observed moves never rewrite globs or compose (m12) | §0.1, §2.4, §2.11, §5.5, §5.7, §8.3.2 P14 |

### Major issues

| Issue | Resolution | Where |
|---|---|---|
| **M1** case-only renames diverge from git | **Adopted, narrowed.** A case-only difference on disk is `ok (case differs on disk)`; the canonical spelling is git's HEAD spelling, else the registration spelling; a case change is recorded only from a writer tree's HEAD (`git/case`) or through `file mv`. The git *index* is not read: HEAD suffices for committed case changes, and index parsing is outside [60] M4's scope | §2.4, §4.3, §4.6, matrix row 4 |
| **M2** a bound tree on the wrong git line; two trees per branch | **Adopted.** Writer tree = the single designated tree with HEAD on the expected ref or a detached HEAD on the lane line; binding uniqueness I-F12; expected ref and base reserved (R-15); other trees are readers and say why | §1.2, §5.1, §5.3, matrix rows 10 and 27, P7 |
| **M3** ancestry is the wrong predicate after patch integration or history rewrites | **Adopted.** Gate on τ(HEAD) first (G1 local change, G3 alias → pending), ancestry second (G2), then E6 over the integration window from p and from aliases (G4); freshness admits G1 and a chain from p; `observed_blob` added to the composite; `absent-in-tree` folded only when the tree is strictly behind; old `pending` rows escalate per link | §2.2, §2.9, §4.3, §5.2, §5.3, §6.2, P15, matrix row 26 |
| **M4** explicit verbs in unbound trees | **Adopted: refuse.** `file mv`/`file rm` run only in the writer tree (exit 5 elsewhere); explicit verbs never create `PENDING` rows; the alternative (versioned intent fields) is owner decision 13 | §0.2, §3.4, §3.5, §9.2 |
| **M5** path reuse renders `ok` | **Changed.** New primary state `replaced` at settle when the `oid` and the file id changed and containment against the last observed content is below 0.29 both ways; `generated` artifacts exempt; fixes `--accept-replacement` or `--drop`; string reserved. Rejected part: the "creation time changed" condition, because Claude Code edits change the creation time every time and NTFS tunneling restores it on a quick delete-and-re-create [M, 09 §2.3, §3.1] | §2.9, §3.7, §4.4, matrix row 20, §8.3.4 |
| **M6** printed fixes turn guesses into accepted re-binds | **Adopted.** No bare accept is ever printed; packs print `file where … --evidence`; `--accept` requires `--expect`; `agent/*` provenance renders `[accepted guess]` until `--confirm` by another actor (owner decision 16); `--repin` needs `--at` after a fuzzy or scope-only match; the skill says "verify before accepting" | §2.2, §3.6, §3.7, §6.2, §6.3, §6.6 |
| **M7** directory moves slow per file; automatic prefix never fires | **Adopted.** The parent directory's id in `FILEOBS`; E3d resolves a moved directory with one `OpenFileById` (directory ids survive renames and moves, measured here [M, §0.3]); `PREFIXEV` accumulates across passes; all-missing pack gate (≤ 5 ms p50) | §2.6, §4.3, §4.4, §7.2, §7.4, matrix row 11 |
| **M8** cloud-synced roots | **Adopted.** Attribute gate (never open recall-flagged content, never enumerate recall-flagged directories in automatic paths); `unverified (cloud-only)`; `--allow-hydrate` for explicit verbs; conflict-copy pattern; `doctor` detection; `files.cloud`; owner decision 14 | §2.7, §4.6, I-F11, R-13, matrix row 28 |
| **M9** interim modes and build order | **Adopted.** The build is placed on [60]'s milestones by dependency; FL-4 depends on M4; the gate, freshness, write rule and bindings are in M8 itself; spawn fallbacks and the "useful slice" removed; DR13 | §1.3, §8.1, §8.2 |
| **M10** atomic-save races | **Adopted.** Quiescence re-check (≥ 50 ms, then re-stat, drop if back) before any settle write; the p + suffix rule; extended never-candidate patterns | §4.1 P10, §4.2, §4.3, §4.6, matrix rows 17–18, P6 |
| **M11** stale E3 after Claude Code edits | **Changed.** Measured: 6 of 121 resolvable moved paths (5.0 %) were edited earlier in the same session [M, §0.3]; §1.4 corrected; a cheap settle stage E8 (same-basename new file, containment ≥ 0.8 → `edited+moved` proposal). Rejected parts: enabling the `Write\|Edit` hook by default (the measurement is below [41]'s own 10 % threshold; a revisit trigger is set in decision 4), and MCP-side batching (the MCP server has no background work and does not see edits) | §0.3, §1.4, §4.3, §4.6, §4.7, §6.4, §9.2 |
| **M12** import on a machine without the files or the objects | **Changed.** Tree eligibility (designated, a worktree of the store's repository, existing `FILEOBS` rows, or `--tree`); `files: no tree bound` with no per-link work; `unverified (commit not in this repository)`; E7 never on a first settle. Change: a bounded E6 history search still runs when `observed_git` is missing, because it reads git objects, not the filesystem, and it is what repairs patch-integrated history (M3) | §4.3, §4.6, §5.1, matrix row 29 |

### Minor issues

| Issue | Resolution | Where |
|---|---|---|
| m1 `oid` vs git blob id | Adopted: git's `convert_is_binary` statistics define `is_text`; git lookups use git's own blob ids (`observed_blob`, trees) | §2.5 |
| m2 misquotes | Adopted: "0.5–20 s" is now stated for the 131-file move; the 3:1 ratio is stated as tree-rewriting git operations, with 29 of 1,886 commits containing renames | §0.1, §3.4 |
| m3 `--deep` cost | Adopted: unsketched candidates are read and sketched at 0.15–0.55 ms each, streamed within a 10 s default budget | §3.7, §7.1, §7.2 |
| m4 anchor uid collisions and repins | Adopted: length-prefixed derivation over a stored `captured` digest; capture de-duplication by current selectors | §2.7 |
| m5 E1 too loose | Adopted: same-tree rows, or other-tree rows verified by the captured `oid` at q | §4.3 |
| m6 E6 pairing and merges | Adopted: identical-blob groups → ambiguous; first-parent chain; merges against their first parent | §4.3 |
| m7 swap invisible on reads; "stat triple" undefined | Adopted: the stat quadruple (size, mtime, file id, creation time) at settle; the read-path limitation documented | §2.6, §4.6 |
| m8 splits only under `--deep`; thin symbol margin | Adopted: settle-time split detection from E6 with git's old blob; same-kind headers and margin 0.1 for symbols and headings | §4.4, §4.5 |
| m9 `--quote -` under PowerShell 5.1 | Adopted: `--quote-file` preferred; BOM stripped; U+FFFD refused | §2.7, §3.1, §6.6 |
| m10 `file mv` after a `sync` | Adopted: `ALIASIDX` probe plus the freshness rule; exit 6 when the tree has not received the re-bind | §3.4 |
| m11 `planned` binds historical files | Adopted: bind only in trees descending from the planning commit, or for files created after it | §3.2 |
| m12 lazy prefixes rewrite globs | Adopted: `observed` entries feed aliases and E5 only | §2.4, §4.4 |
| m13 nested worktrees | Adopted: trees identified by exact top-level; bindings never cover a nested worktree | §5.1, I-F12 |
| m14 recovery rows | Adopted: edited-source (aborted) and edited-destination (roll forward with a detail) rows | §3.4 |
| m15 quiet mode I/O; ignore rules without git | Adopted: hooks exit after one flag read; SessionStart stat only; `files.ignore` defaults; `info/exclude` and `core.excludesFile` read | §4.2, §5.8, R-13 |
| m16 commit-graph chain; worktree count | Adopted: split chains and GDA2 in the reader requirements; 46 worktrees throughout (44 where [13]'s measurements are quoted) | §5.2, §8.1 |
| m17 anchor text in the image | Adopted as owner decision 15 (`image.anchor-text`) | §5.7, §9.2, R-13 |
| m18 freshness of conflict values; dirty main-tree observations | **Changed.** The main tree writes committed observations only (adopted). For conflict values, a tree resolves only when fresh for **every** side; [41]'s "newest side" is rejected because a trunk settle between the moirai merge and the git merge would then pick one lane's path before git merged the other ([41 §2 S9]) | §5.3 |

### Other corrections in this revision

- The superseded draft roadmap [60d] is replaced by the roadmap of record [60] throughout; its R4 placeholders are superseded by §2.11 (edit list in §8.4).
- §6.5 aligns with [50]'s `link_status` built-in (a cached state, never a filesystem call) and adds the live `links.check` procedure.
- Revision 1's FL-10 layer is dissolved: E2 (USN) and E6 are resolver inputs built in M8 and M4; the hooks are M9.
- Anchor and file-node sizes, image growth and `FILEOBS` sizes are recomputed for the new fields (§2.7, §7.3).

### Integration into [AR] (2026-09-26)

The edit list of §8.4 was applied to [AR] (new §5e plus the sections it names) and to [60]. Reconciling this file with [50] (revision 2) and [60] (issue 2) — each had been written against the other's previous revision — changed this file as follows; the decisions and their evidence are recorded in [AR]'s Review log, integration entry.

| Change | Why | Where |
|---|---|---|
| Link-state built-ins take [50]'s spellings and live semantics: `link_state(x)`, `f.state`, `a.state`, `CALL links(scope:)`, `applies(r, glob)`; `link_status`/`link_checked` (a cached `FILEOBS` read) and `CALL links.check` are withdrawn; tree-derived state at a past view is E302, not "unknown" | this design's own P1 and §4.2 make every read, `q` included, compute link states live and write nothing (I-F5); a cached built-in could disagree with `links check` on the same tree; under LQ's two-valued logic a value `unknown` would satisfy `<> 'ok'` | §6.5 |
| `find` presets become the named queries `links_broken`, `links_pending`, `links_proposals`, `links_guesses`, `files_removed`, `files_replaced` reached through `query`; the MCP tool count stays ten | [50 §6.3]: `query` replaces `find` | §6.3 |
| A severity order over a node's anchors, with `replaced` placed after `missing` | [50 §2.6] needs one for `link_state(n)` and did not know `replaced` | §2.9 |
| §8.1 renumbered to [60] issue 2 (image M5, runtime M6, query language M7, CLI M8); FL-2 in the second lane from M0 exit; FL-5 is the CLI surface (M8) of verbs whose semantics are built with FL-4 in M6; FL-6 in M7; sizes re-split, total unchanged (22–30k lines) | [60 §2.4], [60 §3.7]; the old labels contradicted the roadmap of record | §4.7, §8.1, §8.4, §9.2 |
| The size paragraph cites [60] issue 2's allocation (≈ 67–93 units) instead of issue 1's 20–31-unit placeholder | [60 §3], [60 §7] | §8.1 |
| R-13 gains `files.hooks.evidence`, `files.hooks.edit-evidence`, `files.hooks.nudge`, `files.scratchpads`, `files.usn`, `files.deletion-inference`, `files.mv-git`, `files.confirm-roles`; §9.2's calls are classified into 4 owner decisions, 10 config keys and 2 design rules | the owner's rule of 2026-09-26: operational policy is a `config` key, not an owner decision | §2.11 R-13, §9.2 |
| §2.11 and §8.4 state that [60 §2.5] now carries R-1…R-18 and that §8.4 has been applied | [60]'s placeholders were revision 1's R-1…R-14 (with `digest`, a `PathPrefix` op, canonical item 11 and a trailer) | §2.11, §8.4 |

Milestone labels inside the earlier Review-log rows above ("M8" for this design's runtime, "M9" for the hooks) are [60]'s issue-1 labels; read M8 as M6.

### Priority audits (2026-09-26)

The audits [70]–[74] judged the integrated design on speed, RAM, correctness, tokens, feasibility and configuration; [AR]'s Review log (priority-audit entry) dispositions every finding. The R4 changes made here:

| Change | Finding | Where |
|---|---|---|
| `oid` in two streaming passes over fixed 128 KiB buffers; anchor search by chunks; `files.max-line-hashes`; `files.max-read-bytes` redefined; ≤ 2 `--deep` content readers | [71 RAM-M4] | §2.5, §4.5, §4.8, §7.3, R-13 |
| `TREES` settle epochs and dirty row; `FILEOBS` rows only on change with `verified_at` = max(row, covering epoch); `SessionStart` scope = brief + lane `files_owned`, others after 24 h; per-directory enumeration | [70 S5, S8, S14] | §2.6, §4.2, §4.8, §7.2 |
| Runtime `ANCHORRES`; the `Write\|Edit` evidence hook `auto` (spawn-free `mcp_tool`); new record kind `AnchorRes` (R-7) and sections `ANCHORRES`, `GLOBIDX` (R-8) | [70 S3, S7, S17] | §2.6, §2.11, §4.5, §4.7, §6.4, §7.2, §7.4 |
| Read-path git work counted in `fs` units and capped (1 uncached ancestry pair, 32 E6 commits); the E6 window bound is an R-14 constant, `files.budget.*` time budgets only | [70 S6] | §4.3, §4.8, R-13, R-14 |
| `complete`'s settle is a separate commit after `complete`'s own; every settle re-bind CAS-guarded on `rev_seq`; settles resolve outside the writer byte | [72 M11], [70 S2] | §4.2 |
| E3d exact only with an identity match; a present path with a changed file id → `ambiguous (path reused; original at q)`; matrix rows 32–33 | [72 M13] | §4.3, §4.4, §6.2, §8.3.1, R-14 |
| `MOVEFILE_WRITE_THROUGH` in `file mv`, a directory flush after `file rm`/`--trash`; intent liveness by the holder anchor; namespace crash semantics in the simulator | [72 M8, B2] | §2.6, §3.4, §3.5, §8.3.5 |
| Anchor text hashed only through digests (R-10, R-11); `text-unavailable` sub-state after a hash-only import; `image.dest.<name>.anchor-text` | [72 M6] | §2.11, §5.7, §9.2 #15 |
| One-sided glob composition recorded in a sync's residue | [72 M5] | §5.5 |
| USN evidence (E2) and the move nudge not built, reservations kept; `files.usn` and `files.hooks.nudge` dropped | [74 A13, A17] | §4.3, §4.7, §6.4, §8.1, §8.3.1, §9.2 #4, #5, #8, R-13 |
| `links import` built only if #23 = yes, decided before M9; imports batched | [74 A16], [71 RAM-B1] | §8.1, §9.2 #12 |
| `links_sync` through MCP ≤ 200 ms per call with a cursor | [70 S9] | §4.2 |
| ASCII markers with a factored `verify #N` legend; the file-link card measured at ≈ 210–240 tokens inside the ≤ 800-token core skill | [73 F9, F13] | §6.2, §6.6 |
| Gates: 50-link pack ≤ 3 ms idle / ≤ 5 ms loaded, ≤ 5 ms with 10 edited files; read-path git ≤ 10 ms p99; settle ≤ 16 KB when unchanged; RSS with 16 MiB files | [70 S6–S8, S14], [71 RAM-M4] | §7.2–§7.4 |

The resolver's purity (I-F10) is unchanged: every new rule is an R-14 constant, every new budget is a key whose exhaustion yields `unverified`, never a different target.

### Verification pass (2026-09-26)

[AR]'s Review log (last entry) lists the pass. R4 changes: the total is ≈ 68–93 units, the sum of the per-layer ranges (§8.1); R-13 names the time-budget keys as [AR §13] does (`files.read-budget-ms`, `files.session-start-cap-ms`, `files.links-sync-ms`); the spawn assertion of §7.4 is [60]'s GT20. In [AR] and [60], M6's scope now reads E1 and E3–E8 (E2 excluded), its exit carries matrix rows 1–33, the settle-concurrency scenario, the 10-edited-files budget, the promote-replace generators and the I-F11 handle and no-hydration assertions, and E2, the move nudge and the other design-team exclusions await owner decision #41 ([AR §11]); if the owner chooses to build E2, it returns with `files.usn`. *(Superseded: an E2 build now brings the OS-neutral `files.journal`, [AR §11] #41.)* Nothing in this design changed.

### Cross-platform design (2026-09-26)

Owner decision #32 ([AR §11]) makes Linux and macOS design targets now; Windows is built in M6, and Linux and macOS in the unscheduled port phase ([80], revision 2, which answers its review [81]; summary in [AR §14]).
- **DR4** reads "Windows, Linux and macOS; non-admin on all three".
- **§2.4 path rules:** untracked names stored as NFC on normalization-insensitive macOS volumes (P3); `\`, control characters and non-UTF-8 names refused on every OS (P4); `file mv` refuses to create names some OS cannot hold (P5); the fold becomes `fold_v1 = NFD(full_casefold(NFD(x)))` at Unicode 17.0.0 (P6); **twin sets** — paths another OS holds as distinct files but this directory merges — resolve only by content, the others `missing (not representable on this OS)` ([81] M4); "case differs on disk" becomes "spelling differs on disk", with a normalization rule (§1.4, §2.4, §2.9, §4.3, §4.6 and §8.3.1 row 4 follow).
- **§2.6:** `FILEOBS` carries the tagged `OsFileId`, whose Linux id includes the file-handle digest that carries the inode generation, because ext4 reuses the lowest free inode ([81] B2); `USNCUR` becomes `JOURNALCUR` (usn or fsevents); `DIRMAP` is new, for the Linux changed-directory frontier; `TREES` gains the canonical root, the root id and a 16-byte `VolumeCaps` snapshot; `FSINTENT` holds an intent anchor, so a crashed CLI's intent is recovered at once; lazy evidence may be lost after a failed flush; §4.7, §5.7 and §8.4 use the new table names.
- **R-7, R-8, R-14, R-16 and R-18** are updated accordingly.
- **§3.4–§3.5:** every OS renames with a no-replace rename followed by `durable-name` on both parents ([81] m1); `EXDEV` is cross-volume; an interrupted Linux `link` + `unlink` fallback is a recovery state ([81] m2).
- **§4.3:** per-OS realizations of STAT, E3d, E3 and E7; an inode number alone is never identity; **the copy rule's creation-time line now requires a creation time unique in the E4 scope** and applies only where creation times cannot be copied ([81] M5) — this also fixes a pre-existing Windows defect (identical files created in one tick of a checkout).
- **§4.6 and §4.7:** the twin row; macOS dataless files; FSEvents as the natural E2 under #41.
- **§8.3:** the port-phase matrix (with inode-reuse, twin and one-tick rows), and the `VolumeCaps` sweep in GT2 from M6.
- **§9.2:** decision 3 gains (d), macOS document ids.
- **Unchanged:** the cascade, the states apart from the new strings, the thresholds and every invariant. A missing evidence source contributes nothing and never changes a rule.

### Verification pass after decision #32 (2026-09-26)

[AR]'s Review log (XV3, XV4, XV6, XV7, XV10, XV14) lists the findings. Changes here: the `abs` root and I-F8 follow [80] P12 (a machine-local absolute path: `X:/…` with an upper-case drive letter on Windows, the leading `/` kept on Unix, never compared across OSes) (§2.4); P5's policy is the key `files.portable-names` (§2.4, R-13); matrix rows 34–36 (case twins from another OS, identical files created in one tick, the colliding checkout) run on real NTFS in M6 (§8.3.1, §8.3.2); `file rm` step 2 is stated in `Vfs` classes with `durable-name` on both parents (§3.5); the path-reuse check is mapped per OS (§4.3); the sources list [80], [81] and [X17]–[X20].

*End of 40-file-links-design.md.*

### Harness-agnostic design (2026-09-26)

Owner decisions #43 (Codex and other harnesses) and #44 (pure-Rust dependencies, cross-target type check) are applied from [90] (revision 2, after its review [91]): the evidence hooks are harness hooks with a Codex rendering (`^Bash$` filtered in-process by an `mcp_tool` handler; `apply_patch` for edits, whose `*** Move to:` lines are exact move evidence, installed once probe P5 confirms its input field) and remain accelerators; tree resolution also takes Codex's `sandboxCwd`; the MCP `write` tool reaches the link operations as named mutations, the JSON op batch staying on the CLI; the delta hook's budget is in bytes. No R4 rule, reservation, constant or state changes. Edited: §0.1 (15), §4.2, §4.7, §6.3, §6.4 and this log.

### Verification pass after decisions #43 and #44 (2026-09-26)

[AR]'s Review log (HV1–HV22) lists the pass. Changes here: §3.8's rendered examples and §6.2's markers in ASCII (`|`, `->`), §6.1's exit codes 0–10 and [AR §7.1]'s header (HV14); §4.2's MCP `links sync` row names `write` with the named mutation (HV2); §6.4's delta ≤ 600 B (HV11); §9.1's `image.dest.<name>.anchor-text` and the `files.journal` note in this log (HV20). No R4 rule, reservation, constant or state changes.

### The owner's answers of 2026-09-26 on [AR §11]

The owner answered (verbatim translation, [AR] binding inputs): "For now no additional machine will be used, everything is here. Two lanes in parallel. For now benchmarks only on Opus 5.5. The moirai project itself will be stored in a public repository on GitHub. Record that all commits must be made WITHOUT Claude co-authorship. Everything else I approve as you wrote it." Changes here: §9.2 records #3 (a)–(c), #9, #12 and #13 as decided at their recommended defaults, #3 (d) left for the macOS port; §8.1's M9 row and §9.2 #12 say that `links import` is built in M9 and run at the cutover after the owner reviews the ambiguous list ([AR §11] #23). The replay corpora of §8.3.4 are owner-derived: with the repository public ([AR §11] #36), they stay in the gitignored local directory on the owner's laptop and never reach a commit or a hosted runner (#37; [60 §3.1] item 7), and FL-1's replay gates run on the laptop. No R4 rule, reservation, constant or state changes.

### Verification pass after the owner's answers (2026-09-26)

[AR]'s Review log lists the pass. Changes here: §7.4 is headed "Gates" and says that its timing and RAM rows are GT11 rows measured on the owner's laptop, never on the hosted runners, while the synthetic assertions also run in the hosted PR checks; §7.2's rows say "gate (§7.4)"; §8.3.4 states that the replay corpora stay in the gitignored local directory and that their gates run on the laptop (#36, #37); §8.3.6 names the laptop and the replayed load fixture; §9.2's intro, owner-decision bullet and default column state the decisions instead of due dates. No R4 rule, reservation, constant or state changes.
