# 41 — Adversarial review of the file-link design (R4)

*moirai design review, 2026-09-26. Status: review only. Nothing is implemented, and this file is the only file written in the repository. Target: `docs/research/design/40-file-links-design.md` [40], read in full, checked against the design of record [AR] (`docs/ARCHITECTURE-RESEARCH.md` §3, §4, §5, §7) and the R4 reports [09]–[13]. It also applies the binding owner decisions of 2026-09-26: moirai's own engine from the start, no third-party embedded database, early adoption not a goal, no interim stages, every component built to its final specification, and every format field reserved from day one. Method: construct concrete scenarios, trace them through [40]'s cascade, write rules and merge rules, and report where the design re-binds to the wrong file, silently loses a link, needs a resident process, costs too much, or breaks R1–R3.*

**Evidence tags.** **[M]** measured here by the probes in §1. **[D]** documented by a vendor, a specification or source code. **[C]** claimed by a third party. **[I]** inference. A citation such as `[40 §4.3]` points into the design under review.

---

## 0. Verdict

**CHANGES REQUIRED.** The architecture is right: a hybrid of explicit verbs plus lazy, deterministic re-binding, no daemon, and link intent split from per-tree resolution. The anchor model and the intent protocol are sound, and most of the measured claims check out (§4). But four defects are **blockers**, because they either produce silent wrong bindings under the design's own "exact" label or freeze a wrong decision into format v1:

| # | Blocker | Breaks |
|---|---|---|
| B1 | Derived file uids resurrect removed or deleted identities at merge and bind old referrers to unrelated files. The derivation is also machine-dependent (case folding, NTFS upcase table, store-local `rev_seq`) | DR1, R1, R3 |
| B2 | The tree gate and the freshness rule have no ancestry source on the daily path: the commit-graph misses trunk and every active lane **[M]**. The fallbacks the design names are spawn modes that [AR] forbids on hook paths and that the owner decision forbids as interim stages | R1, DR3/DR10, owner decision |
| B3 | "Exact" `oid` evidence re-binds to *pre-existing* identical copies: agent backups (`x.rs.bak`), vendored copies and mirrors. A copy-then-edit-then-move re-binds to the stale copy, and a deletion becomes a silent re-bind | DR1, R4.3 |
| B4 | `PathPrefix` is an *event* inside a canonical form that [AR] defines as a *state diff*. Its meaning is undefined for merge, sync, revert, cherry-pick, checkpoint-granularity and foreign commits, so two stores can compose the same merge differently | R3 (I28′/I30′/I38′), format freeze |

There are also 12 **major** issues and 18 **minor** issues (§3). None of them needs a resident process. The zero-idle-CPU property holds by construction: no thread, timer or watcher exists outside a foreground command or a short-lived hook. The RAM budget holds as well. The cost problems are latency on the read path and in hooks (M7, B2), not idle load.

**What holds up** (and should be kept as is): the two-layer split (§2.1); "reads compute, settle points write"; "absence is never deletion"; unbound-only candidates; quote + context + scope anchors with capture-time uniqueness; no nearest-to-hint tie-break; the crash-safe `file mv` protocol with observation-based recovery; never touching the git index by default; single-word hook `if` patterns; the refusal of watchers and `FileChanged`; and budgets that end in `unverified` rather than a guess.

---

## 1. Method and probes [M]

The probes ran in a private working directory (probe scripts are not published). Repositories were only read, through `git` with `GIT_OPTIONAL_LOCKS=0` and through a stat of `.git/objects/info`. Scratch repositories were created in the probe directory and deleted afterwards. Only two small scripts remain (~3 KB).

| Probe | Result |
|---|---|
| `cgcover.py`: parse BoykoEngine's split commit-graph chain (`objects/info/commit-graphs/commit-graph-chain`, 2 layers, last written 2026-09-24 16:01) and test every `git worktree list` HEAD | 1,796 of 1,889 commits are in the graph, and 95 commits are newer than it. **7 of 46 worktree HEADs are not covered: trunk `integ/unified` (`<lanes-dir>/joltab`), all four active lanes and 2 detached trees.** Only the stale trees are covered. The repository now has 46 worktrees, not 44 |
| `pathreuse.py`: `git log --no-renames --diff-filter=AD`, looking for a path deleted and later re-added | Trunk first-parent: 50 deletions, **4** delete-then-re-add events. All refs: 283 deletions, **34** events on 34 paths (24 of them `.pose` fixtures) |
| Case-only rename in a scratch repository with `core.ignorecase=true` (the owner's setting [M, 13 §1.3]): `mv a.rs A.rs` | `git status` is clean; `git ls-files` and the HEAD tree still say `a.rs`; after a commit HEAD still has `a.rs`; a new `git worktree add` checks out `a.rs` |
| Creation time (`st_birthtime`) after `mv`, `cp` and `cp -p` in Git Bash | `mv` keeps the original creation time. `cp` and `cp -p` get a new creation time, and `cp -p` also keeps the original mtime, so the copy has equal size, mtime and content |
| Trunk shape (`git rev-list`) | First-parent line of `integ/unified`: 47 merge commits and 675 non-merge commits. 132 commits on other refs are not reachable from trunk |
| Environment | `%USERPROFILE%\OneDrive` exists (`$OneDrive` is set) |

Web checks (2026-09-26): the Claude Code permissions reference (PowerShell rule matching), the hooks reference (`if` matching), git `convert.c` (autocrlf binary detection), and Microsoft Learn's file-attribute constants and OneDrive duplicate-file support pages. The URLs are in §7.

---

## 2. Scenario traces

Each trace follows [40]'s own sections. "Fails" names the issue in §3.

| # | Scenario | Trace through [40] | Outcome |
|---|---|---|---|
| S1 | **Editor atomic save of a linked file**: Claude Code `Edit` (temp + rename-over) | §4.3 step 1: path present → `ok`; content changed → anchors re-resolve; `FILEOBS` refreshes the id at the next settle | **Holds** |
| S1b | … JetBrains safe write (`x.rs` → `x.rs___jb_old___`, temp → `x.rs`, delete `___jb_old___`) while a settle runs | In the window the path is absent. E3 opens the stored id at `x.rs___jb_old___` with equal size and mtime → exact → the settle writes the re-bind. Milliseconds later that name is deleted. At the next settle, E3's id is dead and E4 sees `x.rs` with a new `oid` (the alias exists but the content differs) → `missing`, with the file sitting at its original path. The racy rule (§4.6) re-checks only entries whose mtime ≥ scan start, and the renamed original keeps its old mtime. `___jb_old___` is not in the temp list | **Fails → M10** |
| S2 | **`git checkout` of a branch where the file was moved**, same worktree, and the new branch descends from the observation | Tree bound to `lane/l5np`, now on `u/other`. Absent → ancestry yes → E4 `oid` → `moved-auto`, which is written on `lane/l5np` with `observed_git = tip(u/other)`. After switching back: stat of `b.rs` absent → ancestry(`tip(u/other)`, `tip(u/l5np)`) = no → the alias `a.rs` is present → `pending` "moved on lane/l5np", and the freshness rule forbids any write back | **Fails → M2** |
| S2b | … the new branch does not descend from the observation | `absent-in-tree` (or `pending` via an alias); no write | Holds, **if** ancestry can be computed (B2) |
| S3 | **File copied, then the original edited** (then moved) | `cp lock.rs lock.rs.bak`; Claude Edit on `lock.rs` (new id, new content); `mv lock.rs sync/lock.rs`. E3: the stored id is dead. E4: `lock.rs.bak` is size-compatible and its `oid` equals `o` (or `last_oid`), unique → **exact → re-bound to the backup**. After the settle the link reads `ok` | **Fails → B3** |
| S3b | … copied, then the original edited, not moved | Path present → `ok` | Holds |
| S4 | **Two identical files (vendored copies, mirrors)**: one is linked; its directory is deleted | Normal case: E7 considers only files changed since the last settle → `missing` (correct). Fresh worktree or first settle in the tree: every file is "later than T's last settle", so the untouched mirror is an unbound, unique, equal-`oid` candidate → `moved-auto`. A deletion is re-bound to a different tool's file (`.zcode/agents/*.md` ≡ `.claude/agents/*.md` is a real group [M, 10 §5.1]) | **Fails → B3** |
| S4b | … both copies linked, both directories renamed in one commit | E3 exact if `FILEOBS` is fresh. Otherwise E7 finds 2 exact candidates with the same basename → `ambiguous`. E6: two deleted and two added paths share one blob id → the design must not pair them the way git does, by first match | Holds only with m6 specified |
| S5 | **A file split into two** (`foo.rs` → `foo/mod.rs` + `foo/bar.rs`) | Without `--deep`: E6 yields an inexact per-commit pair (e.g. R047) → `moved-needs-confirm` to `mod.rs`, *not* `split`, because split detection lives only in stage 4 (`--deep`). If the printed `--accept` is run, anchors whose text went to `bar.rs` become `orphaned` | **Weak → m8, M6** |
| S6 | **Directory renamed with 2,000 files** (300 linked, 40 edited by agents since the last settle) | Reads: one `OpenFileById` per link (0.24–0.57 ms) → a 50-link pack hits the 20 ms cap, against [AR]'s 1–5 ms pack budget → `unverified`. SessionStart (150 ms) resolves part of the set. The 40 edited files: E3 id stale, E5 "Y/rest with another oid → STRONG" → 40 proposals. An automatic `PathPrefix` requires *every* node "in this pass" → never, so globs are not rewritten and merge composition gets no prefix | **Fails → M7** |
| S7 | **Linked file deleted, then re-created with different content** | Stat present → `ok`, detail "changed". Whole-file anchors default to `watch = header` → no marker, and packs mark only non-`ok` links | **Fails (silently) → M5** |
| S8 | **Agent runs PowerShell `Move-Item` in worktree A while another agent reads the link in worktree B** | B resolves against B's tree (§5.1) → `ok` at the old path. The hook in A records evidence for tree A only. `PowerShell(Move-Item *)` also matches the aliases `mv`, `move` and `mi` [D, permissions: "Common aliases are canonicalized before matching"] | **Holds.** Caveats: the async hook can race the next tool call (M11), and two trees bound to one branch flip-flop (M2) |
| S9 | **moirai branch merge where both sides re-bound the same link differently** (`a.rs` → `b.rs` on L1, → `c.rs` on L2) | §5.5: `FieldEdit` on the composite → `conflicted`. A trunk settle that runs *between* the moirai merge and the git merge resolves it to `b.rs` by observation; after the git rename/rename conflict is resolved, a later settle re-binds by observation (E6). It converges, at the cost of an early `Resolve`. The freshness rule is undefined for a conflict value | **Holds, with m18** |
| S10 | **git-image export → import on another machine without the files** | Link intent imports losslessly. Resolution: every path absent → `observed_git` is not in the local repository (or there is no repository) → reads say `unverified` forever and settle is undefined. Without git, the "tree" is the directory that holds `.moirai`, all links become `missing`, and a first settle's E7 searches an unrelated directory for equal `oid`s (`LICENSE`, `.gitignore`, empty `mod.rs`) | **Fails → M12** (and B1 on uid agreement) |
| S11 | **File in a OneDrive-synced folder** | Capture, settle hashing, E4/E7 enumeration and anchor resolution read content → cloud-only placeholders are hydrated. A sync conflict leaves the cloud version at the path and the local version as `name-<DEVICE>.ext` → the link is `ok` on the other device's content. [40] never mentions cloud roots | **Fails → M8** |
| S12 | **Case-only rename on NTFS** (`a.rs` → `A.rs`) | Stat succeeds; enumeration gives `A.rs` → `moved-auto` (case), written on the tree's branch. Git keeps `a.rs` **[M]**, so every other tree enumerates `a.rs` ≠ the stored `A.rs` → `moved-auto` back → opposite values on two branches → `FieldEdit` at merge | **Fails → M1** |
| S13 | **Symbol anchor after the symbol is renamed** (`LockFile/acquire` → `lock`) | Scope does not resolve → whole file → fuzzy header quote → `edited` (loud), as designed. Hazard: if a sibling such as `acquire_shared` exists and the rename is `acquire` → `acquire_exclusive`, the sibling is closer (7 inserted characters against 10); the top-2 margin is only 0.02; the pack prints `links fix aN --repin`, which pins the sibling | **Loud, but the printed fix is unsafe → M6, m8** |
| S14 | **Running with no git installed** | E6, git hooks and the tree gate are off; the cascade always runs. It works, except that: `observed_git` values imported from a git machine make ancestry "unknown" forever (M12); ignore rules come only from `.gitignore` files that may not exist, so quarantine into `target/` re-binds (m15); E7 on a first settle (B3) | **Mostly holds** |
| S15 | **History rewrite or patch integration** (`git apply` ×189 and `cherry-pick` ×3 in the census [M, 13 §1.1]; rebase, `--amend`, squash; an abandoned lane whose moirai branch was merged) | The lane's observation carries `observed_git` = a lane commit that is never an ancestor of trunk. When the file later moves on trunk: absent → ancestry = no forever → `absent-in-tree` (folded into the header count, §6.2) or `pending` forever. No search, no write | **Fails (silently) → M3** |
| S16 | `file mv` / `file rm` in an unbound worktree (harness `isolation: worktree`, `wf_*`) | The move happens. The intent goes to `PENDING` (lazy, 30 days, never exported). `rm`'s reason has no destination | **Fails the §0.2 guarantee → M4** |
| S17 | Owner moves in Explorer while no moirai process runs | Seen live at the next read; recorded at the next settle | **Holds** (when ancestry is available, B2) |
| S18 | Quarantine and restore into `target/` | P6: never re-bound; nothing written | **Holds** in git trees; see m15 without git |

---

## 3. Issues

### 3.1 Blockers

#### B1 — Derived file uids resurrect removed identities and are not machine-independent

**Scenario.**
1. `lane/x` forks from `main` at c100.
2. On `main`, an agent links `docs/notes/x.md`, giving uid U = BLAKE3(`project`, fold(`docs/notes/x.md`), ""). Note `#300` cites it. Later, `file rm docs/notes/x.md --reason obsolete --yes` sets U to `removed`.
3. On `lane/x`, whose view never saw U, an agent writes an **unrelated** `docs/notes/x.md` and links it from `#410`. The predecessor is empty on that view, so the uid is again U, and "`#N` reuse" [40 §2.3] gives it U's `#N`.
4. `merge lane/x --into main`: both sides created U, which counts as "equal existence" (§5.5). The status `present` vs `removed` is a `StatusFork`, and the settle rule "path present in the merged tree → present" resurrects U.

`#300`'s whole-file anchor now resolves `ok` against a different document, and its `suspect` flag clears. The same happens after an engine `rm` (tombstone): a tombstoned node is not a "predecessor" under §2.3, so re-linking the path derives the tombstone's uid, and a Create of a deleted uid is undefined; the natural reading is an `Undelete` that brings back its historical in-edges.

**Also machine-dependent.**
- `path-key` is folded "with NTFS upcase semantics on case-insensitive directories and exact on case-sensitive ones" (§2.4). The upcase table belongs to the volume, and case sensitivity is set per directory.
- The predecessor is picked by "highest `rev_seq`", which is a store-local sequence number.
- So the same file at the same path gets different uids on Linux or macOS, in a case-sensitive directory, or in another store. The claim "Two stores that link the same file also agree" (§2.3) is false.
- The derivation is frozen in format v1 (R-3), so this must be settled before FL-0.

**Fix.**
1. Derive from the exact path bytes as enumerated (or as spelled in the git tree when git is present). Never fold. Case-insensitive collision detection belongs to `PATHIDX`, not to identity.
2. Choose the predecessor by a store-independent order: (commit `gen`, commit id). Count as predecessors nodes that are `removed`, nodes that are engine-deleted, and nodes whose `aliases` contain the key.
3. New merge rule for derived-uid kinds: a node created on side S after the LCA, while the other side removed or deleted it, is **never resurrected**. The merge deterministically re-keys S's node to uid′ = BLAKE3("moirai-file-v1" ‖ root ‖ path ‖ U) and re-points S's anchors in the merge commit. This is still a pure function of the histories. Drop "`StatusFork` resolved by path presence" for these kinds.
4. A local Create of a uid the store knows as removed or deleted on any branch re-derives with that uid as its predecessor.
5. Define `created:` for a node created on both sides, e.g. the minimum by (gen, commit id), because `.moi` carries one `created` commit.
6. Add property P1 cases: "removed on one branch, unrelated file recreated on another, merge", and "Windows store + Linux store link the same path".

#### B2 — No ancestry source for the tree gate and the freshness rule on the daily path

**Evidence.**
- **[M]** The commit-graph chain covers 1,796 of 1,889 commits. The HEADs of trunk and of all four active lanes are **not** in it. Every `observed_git` recorded since the last `gc` is newer than the graph by construction.
- [AR §2.14] already says "a commit made minutes ago is usually absent". [AR] also says that pack, brief and **hooks never spawn** git, and that the in-process pack reader is v1.1.
- [40 §4.3] step 2 has settle "compute it (in-process reader, **or one cached `git merge-base` spawn per pair**)", and §4.7 says "where only the spawn-based backend exists".
- The `SessionStart` settle is a hook. One spawn costs 130–750 ms, and 2.1 s under load [M, 10 §5.3; 13], against a 150 ms hard cap.
- Reads with unknown ancestry "continue with E1–E5, final `unverified`". So in exactly the trees that matter, the gate that separates "behind" from "moved" does not work. In a stale tree reading a link observed on trunk yesterday, E4's alias probe finds the *pre-move* file.

**Fix.**
- Make an in-process git object reader a hard, final-spec dependency of the resolver (FL-4), not of FL-10. It must read loose objects, packs and `.idx` files, the **commit-graph chain** (`commit-graphs/commit-graph-chain`, which this repository uses) with generation data, and trees.
- Ancestry is then a generation-pruned walk from H over at most the ~95 commits beyond the graph: milliseconds. Cache the answer in `ANCESTRY`.
- Delete every spawn fallback from [40].
- Record in the roadmap that [AR]'s v1/v1.1 split of the object layer does not survive the owner decision "no interim stages" (see M9).

#### B3 — Exact `oid` evidence re-binds to pre-existing identical copies

**Scenarios.** S3 (a backup copy, then an edit, then a move → re-bound to `lock.rs.bak`), S4 (a mirror, then the directory is deleted, then a first settle in a fresh worktree → re-bound to the mirror), and the §4.4 row "exact, tie broken by name", which picks between identical copies. That row contradicts P2 ("ties refused").

**Why it happens.**
- E4 and E7 accept any unbound, size-compatible file with an equal `oid`.
- Nothing checks that the candidate *appeared* when the original vanished.
- The never-candidate patterns are only `*.tmp.*`, `___jb_tmp___` and `~`.
- E7 has no defined behaviour when a tree has no previous settle (`git worktree add` creates every file anew).

**[M]** A same-volume `mv` keeps the creation time; `cp` and `cp -p` do not. `cp -p` keeps the mtime, so size, mtime and `oid` are all equal. The design already stores the creation time in `FILEOBS` (§2.6) but never uses it.

**Fix.** An equal-`oid` candidate from E4, E5 or E7 counts as **exact** only if one of these holds:
- its creation time equals `FILEOBS.creation` (a same-volume move keeps it);
- E6 shows the rename inside one commit;
- E1 or E2 corroborates it.

The other outcomes:
- A candidate created *before* `FILEOBS.verified_at` coexisted with the original. It is a `copy` and never a move target.
- Any other equal-`oid` candidate becomes `moved-needs-confirm (identical copy)`.
- E7 never runs on a tree's first settle.
- Remove the basename tie-break from the automatic class.
- Add `*.bak`, `*.orig`, `*.old`, `*~`, `*.swp`, `4913`, `.#*`, `___jb_old___`, `sed??????` and the OneDrive `-<DEVICE>` suffix to the never-candidate patterns.
- Add all three scenarios to the P1 generators (§8.3.2).

#### B4 — `PathPrefix` is an event inside a state-diff canonical form

**Evidence.**
- [AR §4.6]: the canonical changeset *equals* the state diff against the first parent, and the importer rebuilds every commit kind by this one rule (I38′).
- [40] R-5 adds "canonical-form item 11 'path-prefix moves'" with a `Moirai-Path-Prefix` trailer, and merges treat prefixes as a "union (historical facts, ordered by commit)".
- Undefined:
  - which prefixes a **checkpoint-granularity** commit carries ([AR]'s default image granularity folds many commits);
  - whether a **sync** commit repeats `main`'s window prefixes;
  - what a **revert** or **cherry-pick** of a prefix commit emits;
  - how a git-side **foreign** commit expresses one.

**Consequence.** After a checkpoint export → import, the importing store lacks prefix history that the exporting store uses for E5 and for merge composition (§5.5). The same later merge then composes paths differently in the two stores. That breaks gate 3 ("`import(export(S1)) ⊕ import(export(S2))` equals `merge`") and the claim that merges are pure functions of history.

**Fix.** Store prefix history as versioned **state**: an add-only set field, e.g. `path_moves` on a per-root `area` node, with entries `{from, to, hlc, commit16}`. It then:
- lives in the `.moi` tree diff;
- survives folding and git-side edits;
- merges as an add-wins union;
- composes in the deterministic order (hlc, from, to);
- needs no item 11 and no trailer.

Rewrite R-5 and R-10 before FL-0.

### 3.2 Major

#### M1 — Case-only renames diverge from git and flip-flop between trees

**[M]** With `core.ignorecase=true`, a plain `mv a.rs A.rs` leaves git's index, HEAD and new worktrees at `a.rs`. [40] records the on-disk `A.rs` as an exact `moved-auto`, and every other tree records `a.rs` back (S12). On a case-sensitive checkout, the stored `A.rs` is simply absent.

**Fix.**
- On a case-insensitive directory, a case-only difference is `ok` with the detail `case differs on disk`; it is not a move.
- The canonical spelling is the git HEAD or index spelling when git is present (through the object reader), and otherwise the spelling at registration.
- A case change is recorded only when HEAD's tree spells it (a committed `git mv -f`) or through `file mv`.

#### M2 — A bound tree on the wrong git line writes to the lane, then the freshness rule locks the lane out

**Scenario.** S2. Two more gaps:
- [40 §5.1] compares only the tree's *binding* with the reading branch, never the checked-out git branch with `lane.git_branch`.
- §5.3 does not require that exactly one tree is bound per moirai branch. Two trees at different states re-bind the path back and forth at each settle.

**Fix.** A tree may write re-binds only if both hold:
- it is the branch's single designated tree (`lane.worktree_path` or `files.main-tree`), enforced as a binding-uniqueness invariant;
- its HEAD is on the lane's git line. The symbolic ref must equal `lane.git_branch`; a detached HEAD must descend from `lane.base_sha` and be comparable with the lane tip.

Otherwise the tree is treated as unbound (`PENDING`) and the header says `tree on u/other, not u/l5np`. Reserve the expected git ref in the binding row.

#### M3 — Ancestry is the wrong predicate after patch integration or history rewrites

**Scenario.** S15. [40 §5.2] asks "is `observed_git` an ancestor of H?". After `git apply`, `cherry-pick`, `rebase`, `--amend`, a squash, or a lane abandoned after a moirai merge, the answer stays "no" forever. The next real move of that file is `absent-in-tree` (folded into a header count, §6.2: a near-silent loss) or `pending` forever, and the freshness rule forbids every tree from repairing it. **[M]** Trunk's first-parent line has 675 non-merge commits against 47 merges, and 132 commits are not reachable from trunk. Whether those non-merge commits are patch integrations was not measured.

**Fix.**
- Gate on the tree's own committed content first, through the object reader:
  - `p` is in HEAD's tree but not on disk → a local move or delete → run the cascade; writes allowed;
  - `p` is not in HEAD's tree but an alias is → `pending` (the tree is behind);
  - neither is → run the cascade when `o` is found in HEAD's tree (patch equivalence), and only then consult ancestry.
- Freshness: T may write if `observed_git` is an ancestor of H, **or** HEAD's tree holds the stored path with the observed blob.
- Store the observed git blob id in the composite (reserve it).
- `pending` or `absent-in-tree` rows older than N days, or whose lane is merged, closed or abandoned, escalate to a per-link marker with a re-observe command. They are never folded into a count.

#### M4 — Explicit verbs in unbound trees break the headline guarantee

§0.2 promises that "moves and deletions made through moirai land in one commit". In an unbound tree (harness `isolation: worktree`, `wf_*`; `worktree add` ×88 [M, 13 §1.1]), `file mv` writes a lazy, unflushed `PENDING` row with 30-day retention that is never exported, and `file rm`'s `reason`/`replaced_by` has no destination. A crash, `gc`, a lane that lives past 30 days (36 stale trees lag 272–1,036 commits), or image transport loses the explicit intent.

**Fix.** Choose one:
- refuse `file mv` and `file rm` in unbound trees (exit 5, with the `lane open`/`worktree bind` command); or
- version the intent: add `pending_move{from, to, git_head}` and `pending_removal{reason, replaced_by}` to the composite, written to the caller's resolved branch and applied by the M3 freshness rule once the code reaches a bound tree.

Either way, `PENDING` rows from explicit verbs are durable and retained until they are promoted or dropped.

#### M5 — Path reuse by unrelated content renders `ok`

**Scenario.** S7. The same happens when Claude `Write` replaces a file's content wholesale. **[M]** It is rare in committed history (4 of 50 first-parent deletions, 34 of 283 on all refs) but invisible when it happens.

**Fix.** At settle, which hashes anyway: when the `oid` changed **and** the file id changed **and** the creation time changed **and** `FPRINT` containment is below the background p90 in both directions (0.29 [M, 10 §5.8]), set a new primary state `replaced` ("path reused by different content since c…"). Nothing is written. The fixes are `links fix --accept-replacement` or `--drop`. Reserve the state string.

#### M6 — Printed one-command fixes and the skill turn guesses into accepted re-binds

**Evidence.**
- Packs print `moirai links fix 815 --accept` beside similarity proposals, and `--repin` beside fuzzy `edited` anchors. `repin` "recaptures at the current best match" (§3.7).
- The skill (§6.6) tells agents to "run the printed `moirai links fix …` command". They will.
- Policy A thereby becomes policy B, recorded with `user/…` provenance that looks like a human decision.
- S13 shows `--repin` pinning a sibling symbol.

**Fix.**
- Never print a bare accept for non-exact states. Print the evidence command instead (`moirai file where 815 --evidence`: containment both ways, basename, git pair).
- Require `--accept --expect PATH` and record provenance `agent/similarity`, rendered `[accepted guess c…]` until a second role or the owner confirms.
- Refuse `--repin` without `--at` after a fuzzy or scope-only match.
- Rewrite the skill line to "open the candidate and verify before accepting".

#### M7 — Directory moves are slow per file, and the automatic prefix never fires under budgets

**Scenario.** S6.

**Fix.**
- Store the parent directory's `FILE_ID_128` in `FILEOBS`. On a miss, resolve the nearest missing ancestor directory **once** by id: directories keep their ids on rename [M, 09 §3.1].
- Every linked file under that directory then re-binds by its relative name, exactly, whatever its content. This is the same reasoning as "path present → `ok`".
- Accumulate prefix evidence across passes in runtime rows, and record the prefix (as state, per B4) once all linked nodes under `from/` are re-bound in whichever pass.
- Add a CI gate for a 50-link pack in which every link is missing (today's gate covers only the all-present case).

#### M8 — Cloud-synced roots (OneDrive) are not addressed

**Evidence.**
- **[M]** OneDrive is present on the owner's machine.
- **[D]** `FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS`: "Reading the file / enumerating the directory will be more expensive than normal, e.g. it will cause at least some of the file/directory content to be fetched from a remote store".
- Capture, settle hashing, E4/E7 enumeration and anchor resolution would hydrate placeholders. That means network traffic, disk use, and a changed local state for the user's file (contrary to DR9), with unbounded latency inside a 150 ms hook.
- **[D]** On a sync conflict, the online version keeps the name and the local copy gets the device name appended. The link becomes `ok` on the other device's content, and the conflict copy is an exact candidate.
- [AR] refuses the store on OneDrive; [40] is silent about project files and named roots.

**Fix.**
- Take attributes from enumeration. In automatic paths, never open the content of `RECALL_ON_DATA_ACCESS`, `RECALL_ON_OPEN` or `OFFLINE` entries, and never enumerate such directories. Report `unverified (cloud-only)` and match only on size, mtime and id.
- Detect cloud sync roots (cloud-filter reparse tags) in `doctor` and warn.
- Add the conflict-copy suffix to the never-candidate patterns.
- Add pattern-matrix rows to §8.3.1.

#### M9 — Interim modes and the build order contradict the owner decision

**Evidence.** [40] contains:
- "Where only the spawn-based backend exists, E6 runs only in explicit verbs" (§4.7);
- "one cached `git merge-base` spawn per pair" (§4.3);
- "The first complete layer is FL-0 to FL-5 … on one branch and one tree … the smallest slice that is useful" (§8.1).

The resolver's tree gate (§4.3 step 2), the freshness rule and the write rule are placed in FL-7. So FL-4's `resolve`/settle would behave differently before FL-7 lands: it would write without the gate. That is a temporary mode, which contradicts "later layers add capabilities and replace nothing".

**Fix.** Order by dependency:
- format → object reader (commits, trees, commit-graph chain) → FL-4, which includes tree identity, the gate, the freshness rule, the write rule and bindings, since these are resolve semantics;
- FL-7 keeps only the merge rules;
- drop the "useful slice" framing and all spawn fallbacks.

#### M10 — Atomic-save races write re-binds to transient names

**Scenario.** S1b. The same risk applies to MSYS `sed -i` temporaries, vim `4913`/`.swp`, Emacs `.#x` and Office `~$`.

**Fix.**
- Before writing any re-bind (settle only), re-stat `p` after ≥ 50 ms and drop the re-bind if `p` came back, whatever the candidates' mtimes.
- Never re-bind into a name that is `p` plus a suffix.
- Extend the patterns as in B3.

#### M11 — Claude Code's own edits keep E3 stale, so "pure moves" of agent-edited files are not exact

**Evidence.**
- Every `Edit`/`Write` gives the file a new id [M, 09 §3.1].
- `FILEOBS` refreshes only at settles and in hooks; reads may not append anything (I-F5); and the `Write|Edit` evidence hook is **off** (§6.4).
- Edits outnumber moves: `Edit` ×19,519 and `Write` ×4,235, against 148 `mv` [M, 13 §1.1]. A linked file that an agent moves was usually edited since the last settle.
- Then E3 is dead, and E4's `oid` ∉ {`o`, `last_oid`}. Similarity runs only under `--deep`, so the result is `missing` until the move is committed.
- §1.4 row 1 ("pure rename or move … exact via E3") therefore overstates coverage for the main actor.
- The async evidence hook also runs after the whole Bash call, so `mv a b && sed -i … b` is already edit-after-move by the time it runs.

**Fix.**
- Measure the share of edit-then-move in the transcript census.
- Enable the `PostToolUse Write|Edit` evidence hook by default, restricted to linked paths (async, zero idle CPU), or let the resident MCP server batch id refreshes.
- Add a cheap settle stage: a same-basename file that is new since the last settle, with sketch containment ≥ 0.8 → a proposal instead of `missing`.

#### M12 — Image import on a machine without the tree, or without the git objects

**Scenario.** S10.

**Fix.**
- A tree is used for resolution only if it is explicitly bound, or already has `FILEOBS` rows. Otherwise the header says `no tree bound`, and no per-link work runs.
- An `observed_git` absent from the local object store → `unverified (commit not in this repository)`, never a search.
- E7 never runs on a first settle (B3).
- Document that fingerprints and runtime tables are rebuilt only when the content is readable.

### 3.3 Minor

| # | Issue | Fix |
|---|---|---|
| m1 | "`oid` equals git's blob id for normally committed text files" (§2.5). Git's autocrlf treats a file as **binary** (no conversion) when it has a lone CR, any NUL anywhere, or more than 1/128 non-printables, and "safer autocrlf" skips conversion when the index already holds CRLF [D, `convert.c`]. [40] normalises when there is no NUL in the first 8,000 bytes, even with lone CRs. Correctness does not depend on equality, but the tie-breaks that "use the old blob from git" silently miss for these files | State the exact rule; look up git blobs by git's own id (from the index or the tree), not by moirai's `oid` |
| m2 | Misquotes: "0.5–20 s file by file" [M, 13 §1.4] was measured for **131** files, not 1,000. "Git operations move files about three times as often as move commands" (§0.1) repeats a count of *tree-rewriting* git operations; only 29 of 1,886 commits contain renames [M, 13 §1.2] | Correct the text |
| m3 | The `--deep` cost row (§7.2) cites stage-1 timings measured over **precomputed** sketches [10 §5.11], but [40] stores sketches only for linked content. Unbound candidates must be read and sketched: seconds at 1e4 files warm, tens of seconds cold | Fix the table; budget and stream the candidate reads |
| m4 | The anchor uid concatenates fields with `‖` and no length prefixes (collisions by construction). `repin` keeps the uid, so every repinned anchor fails "derivation matches" at import and is flagged in `image doctor`; an identical fresh capture after a repin duplicates the anchor | Length-prefix every field; derive from the capture-time selectors and keep them as a field `captured_from`, or drop derivation validation for anchors |
| m5 | E1 accepts `PENDING` rows from *any* tree, which is looser than the promotion rule of §5.3 | E1 uses only the same tree's rows, or rows verified by the captured `oid` at `q` |
| m6 | E6 does not say how to pair identical blobs within one commit, or how to walk merges | Several deleted or added paths with one blob id → `ambiguous`; follow the first-parent chain and diff merges against their first parent |
| m7 | A swap of two files with equal size and mtime is invisible on reads (`GetFileAttributesExW` returns no id); "stat triple" is not defined | At settle, compare (size, mtime, file id, creation time); reads accept the limitation and say so |
| m8 | Split detection runs only in stage 4 (`--deep`), and the fuzzy symbol margin of 0.02 is thin against sibling headers | At settle, classify a split from E6's inexact pair when the commit's other added files receive shares (old blob from git); for symbol anchors with a failed scope, consider only same-kind header candidates, with margin ≥ 0.1 |
| m9 | `--quote -` from default PowerShell 5.1 delivers `????`; under Claude Code's PowerShell tool stdin carries a UTF-8 BOM [M, 16 §6.10]. 157 headings are Cyrillic [M, 11 §2.5] | Prefer `--quote-file`; strip a leading BOM; refuse input containing U+FFFD |
| m10 | After a moirai `sync` brings `main`'s new path while the lane's code still has the old one, `file mv OLD NEW` scans `PATHIDX` only and does not find F | The plan step also consults `ALIASIDX`, then applies M3's rules |
| m11 | `planned` binds purely by path. A stale tree that holds a historical file at that path binds it | Record `observed_git` at planning and bind only in trees that descend from it, or with a creation time after planning |
| m12 | An automatic prefix from lazily observed in-tree quarantine (`mv tests/fixtures tests/_disabled/…` and back) rewrites owner-authored globs twice and enters merge composition | Only explicit, confirmed or git-committed directory moves rewrite globs; a lazy prefix feeds aliases only |
| m13 | Nested worktrees (`.claude/worktrees/*` inside the main checkout) combined with [AR]'s longest-prefix binding make "bound tree" ambiguous | A tree is identified by its exact git top-level; prefix bindings never mark a nested worktree as bound |
| m14 | The recovery table (§3.4) has no rows for "destination present with a different `oid`" (edited after the move) or "source present with a different `oid`" | Destination present → roll forward with a content-drift detail; source present → aborted |
| m15 | Quiet mode: evidence hooks and the `post-commit` settle still do I/O during benchmarks; without git, ignore rules come only from `.gitignore` files (and `core.excludesFile`/`info/exclude` must also be read when git exists) | Hooks exit after one `HEAD` flag read in quiet mode; SessionStart does stat only; `files.ignore` defaults (`target/`, `node_modules/`, `build/`) apply when no ignore file exists |
| m16 | [AR]/[40] say "`objects/info/commit-graph`", but the owner's repository uses a split **chain** [M]; the worktree count is now 46 [M] | The reader supports chains and GDA2; update the counts |
| m17 | Anchor `quote`/`prefix`/`suffix` put ~11 MB of source excerpts into the image repository, which may carry different access rights from the project repository | Owner decision; offer `image.anchor-text = hash-only` for foreign destinations |
| m18 | The freshness rule is undefined for a conflict value, and dirty observations made in the main tree are written to `main` (§5.3) and synced to every lane | Freshness uses the newest side of the conflict; the main tree writes only committed observations (checked through HEAD's tree), dirty ones stay in `FILEOBS` |

---

## 4. Fact check of claims [40] relies on

| Claim ([40] §) | Source | Verdict |
|---|---|---|
| 148 raw `mv`, 7 `git mv`, 63 Python renames in 121,571 shell calls (§0.1, §1.4) | [M, 13 §1.1] | holds |
| "git operations move files about three times as often as move commands" (§0.1) | [13 §1.1] counts tree-rewriting operations, not moves | **overstated** (m2) |
| 186 of 201 git renames are 100 % similar; 131-file bulk move (§1.4) | [M, 13 §1.2] | holds |
| Every Claude Code Edit/Write and every git rewrite of a changed file gives a new id (§0.1) | [M, 09 §3; 10 §5.4; 12 §3.1; 13 §1.4] | holds, and is the root of M11 |
| The unprivileged USN read works without admin; D: has no journal (§0.1, §4.7) | [M, 09 §4.1–4.2] | holds. [12 §3.4] and [13 §0] say the journal "needs admin"; [09] tested the correct handle type and [40] correctly follows it |
| `FILE_ID_128` + volume serial; `nFileIndex` is −1 on ReFS; MFT slot reused 165× (§2.6) | [D/M, 09 §2.1, §2.5] | holds |
| `OpenFileById` + path 0.24–0.57 ms; enumeration 12–22 µs per entry; stat 17–67 µs (§7.1) | [M, 09 §8; 13 §1.7] | holds (measured under load) |
| `oid` = git blob id for normally committed text (§2.5) | [M, 13 §1.7] one file; [D, convert.c] | **qualified** (m1) |
| Sketch recall@10 287–300/300; stage 1 0.6/9/114 ms; stage 2 3.1 ms (§2.5, §4.3) | [M, 10 §5.8, §5.11] | holds for indexed sketches; **misapplied** to `--deep` over unindexed candidates (m3) |
| Git's 50 % threshold misses 7 of 16 real inexact renames; 6 of 16 are splits (§1.4) | [M, 10 §5.7] | holds |
| Per-commit beats long span (0 renames over 4 months) (§4.7) | [M, 10 §5.9] | holds |
| 1.2 % of files in exact-duplicate groups (§1.4) | [M, 10 §5.1] | holds, and it is exactly B3's population |
| Quote + context 96.3 % overall / 90.1 % of changed files vs 27.2 % for line numbers; cascade 95.1 % correct / 0.9 % wrong (§4.5) | [M, 11 §2.3; 10 §5.10] | holds |
| ±2-line uniqueness 98.7/99.9/98.9 %; name paths 99.88 % unique; scope rescues 17 of 24 orphans (§2.7) | [M, 11 §2.4, §2.6, §2.3] | holds |
| tree-sitter-rust +1.8 MB; tree-sitter-md 722 ms against 0.17 ms; HLSL 23.9 % errors (§2.7.1) | [M, 11 §2.8–2.9] | holds |
| Directory rename 9.5 ms for 1,000 files "against 0.5–20 s file by file" (§3.4) | [M, 13 §1.4] | first half holds; the second half was measured for 131 files (m2) |
| Multi-word hook `if` patterns run on `$()`, backticks and `$VAR`; 37.6 % of Bash calls (§4.7) | [D, hooks reference, re-fetched 2026-09-26]; [M, 13 §1.1] | holds |
| `PowerShell(Move-Item *)` also catches `mv`/`move` inside PowerShell (implied by §4.2) | [D, permissions reference: "Common aliases are canonicalized before matching"] | holds (not cited in [40]) |
| `reference-transaction`/`post-index-change` fire up to 10/6 times; `post-commit` is the only hook that sees `apply` and `--no-commit` merges (§4.7) | [M, 13 §1.5] | holds |
| "The tree gate answers … in microseconds, using the `ANCESTRY` cache and commit-graph generation numbers" (§5.2) | [M, this review §1]; [AR §2.14] | **false on the daily path** (B2) |
| 39 of 44 worktrees are ancestors of trunk; 36 lag 272–1,036 commits (§5.2) | [M, 13 §1.3] | holds as measured then; 46 worktrees now [M] |
| "Two stores that link the same file also agree" (§2.3) | derivation inputs (§2.3–2.4) | **false** across platforms and case-sensitive directories (B1) |
| "Every past commit kind … `PathPrefix` hashed and verifiable" (§5.7) | [AR §4.6] state-diff rule | **undefined** for merge, sync, revert, checkpoint and foreign commits (B4) |

---

## 5. Consequences for the format spec and the build order

**Add to or change the reservations of [40 §2.11] before FL-0:**
1. **R-3.** Identity derivation over exact path bytes; store-independent predecessor order; a re-key rule for dual creation against removal (B1).
2. **R-5/R-10.** Replace the `PathPrefix` op, canonical item 11 and the trailer with a versioned add-only set field of prefix moves (B4).
3. **Composite.** Add `observed_blob` (the git blob id at observation) (M3), and `pending_move`/`pending_removal` intent fields, or record the decision to refuse unbound-tree verbs (M4).
4. **`FILEOBS`.** Add the parent directory's `FILE_ID_128` (M7); make creation-time comparison normative (B3); add a `cloud` attribute bit (M8).
5. **Binding row.** Add the expected git ref / base, and a uniqueness invariant per moirai branch (M2).
6. **State strings.** Add `replaced` (M5), `no tree bound`, `unverified (cloud-only)` and `unverified (commit not in this repository)` (M8, M12).
7. **Provenance values.** Add `agent/similarity`, distinct from `user/*` (M6).
8. **R-14.** Put the never-candidate pattern list, the `oid` text rule (with git's exact binary test documented beside it) and the window-hash function into the resolver constant table (B3, M10, m1).
9. **Anchor record.** Length-prefixed derivation, or `captured_from` (m4).

**Build order by dependency (replacing [40 §8.1]):**
- F (format) → E (engine) → G (graph core) → **I-reader** (in-process git object reader: loose, pack, idx, commit-graph chain, trees; final spec, no spawn backend) → FL-4 (resolver, including tree identity, gate, freshness, write rule and bindings) → C/FL-5 → V/FL-7 (merge rules only) → I/FL-8 (image) → A/FL-9 → FL-10 (USN, hooks).
- FL-1 and FL-2 stay parallel with the engine.
- The "one branch, one tree" slice is dropped as a milestone.

---

## 6. Owner decisions this review adds

1. **Unbound worktrees and explicit verbs** (M4): refuse `file mv`/`file rm` there, or version the intent on the caller's branch until the code reaches a bound tree? Recommended: refuse. It is simpler, and the lazy path still catches the raw move.
2. **The `Write|Edit` evidence hook** (M11): on by default for linked paths (one async spawn per edit of a linked file, zero idle CPU), or accept that agent-edited files moved without a commit end `missing` until the commit? Recommended: measure first, then turn it on if edit-then-move is more than ~10 % of moves of linked files.
3. **Cloud roots** (M8): refuse roots under a cloud sync root, or support them in metadata-only mode? Recommended: metadata-only, with a `doctor` warning.
4. **Anchor text in the image** (m17): full quotes, or hashes only for destinations outside the owner's machines?

---

## 7. Sources

**Internal:** `docs/research/design/40-file-links-design.md`; `docs/ARCHITECTURE-RESEARCH.md` §2.10, §2.14, §3, §4, §5a–5d, §6.6, §7.5, §8.1; `docs/research/09`, `10`, `11`, `12`, `13`, `16` (§6.10, PowerShell 5.1 stdin).

**External (fetched 2026-09-26):**
- Claude Code hooks reference, `if` matching table: https://code.claude.com/docs/en/hooks
- Claude Code permissions reference, PowerShell rules ("Common aliases are canonicalized before matching"): https://code.claude.com/docs/en/permissions
- git `convert.c` (`convert_is_binary`, `gather_stats`, safer autocrlf): https://github.com/git/git/blob/master/convert.c
- Microsoft Learn, file attribute constants (`RECALL_ON_DATA_ACCESS`, `RECALL_ON_OPEN`, `PINNED`, `UNPINNED`): https://learn.microsoft.com/en-us/windows/win32/fileio/file-attribute-constants
- Microsoft Support, duplicate files in OneDrive (device name appended on conflict): https://support.microsoft.com/en-us/onedrive/duplicate-files-in-onedrive

**Probes:** `cgcover.py`, `pathreuse.py` (probe scripts are not published). The case-rename repository and the creation-time files were deleted after the runs.

*End of 41-file-links-critique.md.*
