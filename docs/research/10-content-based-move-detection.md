# 10 — Content-based move/rename detection and file fingerprints (R4 research, lens: content)

*moirai research, 2026-09-26. Status: research only, nothing implemented. Lens: find a moved, renamed, split or merged file by its content, and keep node → file references (whole files and locations inside files) valid. Companion lenses (explicit `moirai mv/rm/add` commands, file-system watchers, git hooks) are covered elsewhere. This lens is the safety net that works whoever moved the file.*

Tags: **[M]** measured here (this machine, this session), **[D]** documented by the vendor/maintainers or in source code, **[C]** claimed by a third party, **[I]** my inference.

---

## 0. Answer first

**Recommended fingerprint (per referenced file, ~420 B on disk, 0 B resident at idle):**

| Field | Size | Why |
|---|---|---|
| `path` (repo-relative, `/`, exact case) + worktree binding | ~45 B mean [M] | the primary key; resolution never leaves the bound worktree |
| `oid` = git-blob-compatible SHA-1 of the **EOL-normalised** content | 20 B | exact rename by id, as git does; equals git's index/tree entry for clean text files, so tracked files need no hashing; 72% of BoykoEngine working-tree files are CRLF [M], so raw-byte hashes would not match git or other worktrees |
| `size`, `mtime`, NTFS `file_id` + volume serial | 24 B | git-style stat cache: an unchanged file is never re-hashed; `file_id` recognises a pure move for free |
| `sketch` = bottom-64 of normalised-line hashes (u32) | 256 B | stage-1 candidate ranking; recall@10 = 287–300/300 across all edit classes [M] |
| `weight`, `distinct_lines` (totals of the normalised lines) | 8 B | turn intersections into symmetric similarity and containment in both directions; allow sketch-only estimates when the old content is gone |
| `observed_git_sha` | 20 B | lets moirai replay git's per-commit renames from the bind point |
| per in-file anchor: normalised line hash, ±2-line context hash, line hint, ≤120-char excerpt, optional symbol | ~150 B | content anchors, never line numbers |

**Recommended resolution (lazy + event-triggered, no daemon):** stat the stored path → if dead, try (1) the same NTFS file id among unbound files, (2) the same `oid` among unbound files, (3) git's per-commit renames since `observed_git_sha`, (4) a two-stage similarity search: sketch top-10, then an exact weighted normalised-line score in both directions (taking the maximum with token winnowing when the old blob is in git). Auto-rebind only when the evidence is exact, or when similarity is ≥ 0.5 with a ≥ 0.2 margin over the runner-up. Classify the rest as `split`, `merged`, `ambiguous` or `deleted` and show them to the referrers. Never auto-rebind a guess. For locations inside a file, cascade: exact normalised line with ±2 lines of context → unique exact line → fuzzy proposal flagged as `changed`.

**Key numbers [M]** (BoykoEngine, 2,555 non-ignored files, 70 MB; machine at 97–100% CPU from other load, so absolute times are pessimistic):

- A gitignore-aware walk + stat takes 102–111 ms single-threaded and 26–32 ms with 8 threads. NTFS file ids for every file cost 29 ms via directory enumeration, versus 160 ms when each file is opened.
- Hashing is not the bottleneck. In memory, BLAKE3 runs at 1.1 GB/s, SHA-1 at 1.2 GB/s, SHA-256 at 1.25 GB/s and xxh3 at 3.7 GB/s. A warm serial read alone takes 424 ms for all files, and read + BLAKE3 takes 370–440 ms. The first touch ran at 50 MB/s.
- Line-number anchors rot fast. The same `path:line` still points at the same text for 94% of lines after 2 weeks, 89% after 2 months and **28% after 4 months**. Every changed line is bound silently. Content anchors (the cascade) are 100% / 99.97% / 95.1% correct, with 0 / 0 / 0.9% wrong.
- Git's default 50% threshold misses **7 of the 16** real inexact renames in BoykoEngine history. 6 of the 16 are Rust module splits (`foo.rs → foo/mod.rs + siblings`).
- Over a 4-month span, a pairwise `git diff -M A HEAD` finds **0** renames. Containment still finds the one moved file (94%), and it separates it from 13 truly deleted files (all ≤ 0.18).
- git's line-chunk similarity fails on re-indentation: the correct target ranks first in 12/300 queries versus 296/300 for normalised lines.
- Normalised lines in turn weaken on line reflow (0.40 at 50% joined lines) and identifier renames (0.36). Token winnowing keeps 1.00 and 0.57 there but is weaker on scattered edits (0.53 vs 0.67). Hence stage 2 takes the maximum of the two.
- One similarity query costs 0.6 / 9 / 114 ms for the sketch stage at 1e3 / 1e4 / 1e5 candidates, and 3.1 ms to re-score the top 10 from disk.

---

## 1. What breaks a file reference, and what content can repair

| Event (who does it) | Path | Content | NTFS file id [M] | Content-based repair? |
|---|---|---|---|---|
| rename/move in the same volume (Explorer, `mv`, `Move-Item`, `git mv`) | changes | same | **kept** | yes, exact |
| move + edit (agent renames then edits; module split) | changes | changes | kept if moved first; **new** after any Claude Code Edit/Write | yes up to ~50% churn; beyond that, no |
| copy then delete original (`cp`+`rm`, cross-volume move) | changes | same | **new** | yes, exact |
| split one file into several (`foo.rs → foo/mod.rs + foo/a.rs`) | changes | partitioned | n/a | as a *split* (containment), not as a rename |
| merge into another file | old path gone | contained in host | n/a | as a *merge* (containment of old in host = 1.00) |
| git checkout / stash / reset / rebase touching a file | same | may change | **new** on every rewrite | not a move; path is alive → re-anchor inside |
| editor/agent atomic save (temp + rename) | same | changes | **new** (Claude Edit and Write tools, `os.replace`) | not a move; path alive |
| delete | gone | gone | gone | no; must be reported as dangling |
| directory move | all descendants change | same | kept | yes; one directory rule explains many files |
| case-only rename (`Foo.rs → foo.rs`) | changes on a case-insensitive FS | same | kept [I, not tested] | yes, but only if the path comparison uses exact case |

What content cannot repair: an empty or trivial file (no signal), a file rewritten by more than ~50–70%, a file moved outside the scanned worktree or into an ignored directory, and a choice between identical copies. Those need an explicit record (the `moirai mv` lens) or a question to the user.

---

## 2. Prior art

### 2.1 git: renames are inferred, never recorded

- **Model.** Linus in April 2005: a rename "really doesn't exist in the git model"; git tracks content, not what happened to it [D: [public-inbox](https://public-inbox.org/git/Pine.LNX.4.58.0504141102430.7211@ppc970.osdl.org/), [gist](https://gist.github.com/borekb/3a548596ffd27ad6d948854751756a08)].
- **Pipeline** (`diffcore-rename.c`) [D: [source](https://github.com/git/git/blob/master/diffcore-rename.c)]:
  1. **Exact renames.** `find_exact_renames()` hashes sources by blob oid and pairs destinations with the same oid.
  2. **Basename matching.** `find_basename_matches()` pairs files whose basename is unique on both sides, then still requires `estimate_similarity()`. A source comment justifies it: "over 76% of file renames in linux just moved files to a different directory but kept the same basename". This is Newren's "optimization batch 7" [C: [lore](https://lore.kernel.org/git/CABPp-BE9dPYgTsrAKjjmPTfy-xY56ajg-1ZYPf7X97YR0T_n3Q@mail.gmail.com/T/)].
  3. **Inexact matrix.** Everything left is compared all-pairs, keeping `NUM_CANDIDATE_PER_DST = 4` per destination. A size pre-check rejects a pair whose size difference alone makes the minimum score unreachable.
  4. **Rename limit.** The exhaustive part is skipped when `num_sources × num_destinations > rename_limit²`.
- **Similarity** (`diffcore-delta.c`) [D: [source](https://github.com/git/git/blob/master/diffcore-delta.c)]. Content is cut into chunks ending at LF or after 64 bytes. For text, the CR of a CRLF is skipped. Each chunk is hashed modulo `HASHBASE = 107927`, and the byte counts are summed per hash. `src_copied = Σ min(src_cnt, dst_cnt)` and `score = src_copied / max(src_size, dst_size)`.
  - Consequence [M §5.8]: whitespace is significant inside a chunk, so re-indenting a file (wrapping it in a `mod {}` or `impl` block) drops the score to ~0.02.
- **Knobs** [D: [git-diff](https://git-scm.com/docs/git-diff), [merge-config](https://git-scm.com/docs/merge-config)]:
  - `-M` threshold defaults to **50%**; `-M100%` means exact only.
  - `-B` break defaults to 50%/60%.
  - `-C` detects copies only from files modified in the same change; `--find-copies-harder` also considers unmodified files and is "very expensive".
  - `diff.renameLimit` defaults to **1000**; `merge.renameLimit` defaults to **7000**. The exhaustive portion is O(N²).
  - Other implementations disagree on the threshold: JGit uses 60% [C: [jgit#110](https://github.com/eclipse-jgit/jgit/issues/110)]; gitoxide's `gix_diff::Rewrites` defaults to `percentage = Some(0.5)`, `limit = 1000`, `copies = None` and `track_empty = false` [D: [docs.rs](https://docs.rs/gix-diff/latest/gix_diff/struct.Rewrites.html)].
- **Directory renames** (merge/rebase/cherry-pick only, not `git diff`) [D: [doc](https://git-scm.com/docs/directory-rename-detection)]:
  - A directory is inferred as renamed from the majority of its files' renames.
  - It is not renamed if it still exists on both sides.
  - On a split, "the directory with the most renames, wins".
  - Files added to the old directory are relocated.
- **Remembering renames** [D: [doc](https://git-scm.com/docs/remembering-renames/2.33.0.html)]. merge-ort caches upstream renames across the picks of one rebase, in memory, only while the picks are conflict-free.
- **Index stat cache** [D: [racy-git](https://git-scm.com/docs/racy-git)]:
  - Per entry, git caches mode, mtime, ctime, uid, gid, ino and size (dev and nsec are optional), and a file whose stat matches is "unchanged".
  - "Racily clean" entries have mtime ≥ the index file's timestamp. They are content-checked, and when the index is written their cached size is smudged to 0.
  - Git for Windows sets `st_ino = 0` [C: search summary of [compat/mingw.c](https://github.com/git/git/blob/master/compat/mingw.c)].
  - `core.fsmonitor=true` uses the built-in daemon on Windows/macOS; `core.untrackedCache` skips unchanged directories by directory mtime [D: [git-config](https://git-scm.com/docs/git-config)].
- **User-level experience** [C: [Ole Begemann, 2025-12-15](https://oleb.net/2025/git-file-renaming/)]. Detection fails exactly when a rename coincides with heavy edits, and "more often than not, my reason for renaming a file *is* that I made substantial edits". The advice: rename in a standalone commit.

### 2.2 Mercurial: renames are recorded, with a similarity fallback

- `hg mv` / `hg cp` "mark dest as copies of sources" [D: [hg rename](https://www.mercurial-scm.org/help/commands/rename)]. The copy source is stored as `copy`/`copyrev` metadata in the destination's filelog revision, behind a `\1\n` header [C: [revlog notes](https://ngoldbaum.github.io/posts/revlog/)].
- `-A/--after` records a move that was already done by other means.
- For moves done outside hg, `hg addremove -s N` compares every removed file with every added file. N ranges from 0 (off) to 100 (identical only), and **100 is the default** [D: [hg addremove](https://www.mercurial-scm.org/help/commands/addremove)].
- Filelog-centric copy tracing was slow on big histories, which led to experimental changeset-centric storage (`experimental.copies.write-to=changeset-only`) [C: [mercurial-devel D6936](https://www.mail-archive.com/mercurial-devel@mercurial-scm.org/msg47185.html)].
- Lesson [I]: even a VCS that records renames needs a similarity fallback for out-of-band moves, and it defaults that fallback to exact.

### 2.3 Jujutsu (2025–2026)

- The design doc *Copy tracking and tracing* [D: [jj docs](https://docs.jj-vcs.dev/latest/design/copy-tracking/)] proposes a **record** model: a `CopyId` per file entry (`File { id, executable, copy_id }`) that hashes a `CopyHistory` DAG.
- It rejects pure detection because "It's hard to make this model scale to very large repos". It cites full-history detection taking 165 s in git.git and 13 h in Nixpkgs.
- The 8-step plan is not implemented.
- Through the git backend, jj detects renames and copies for display. The 0.44/0.45 changelogs (Aug–Sep 2026) fix renames and copies missing from merge-commit diffs [D: [changelog](https://docs.jj-vcs.dev/latest/changelog/)].
- Rebase does **not** follow renames:
  - issue #47 has been open since 2021 [D: [#47](https://github.com/jj-vcs/jj/issues/47)];
  - #6940 (July 2025, jj 0.30) reports the rebase conflict [D: [#6940](https://github.com/jj-vcs/jj/issues/6940)];
  - a manual workaround is described in [C: [Watson, 2025-05-23](https://blog.eliaswatson.dev/posts/jj_rebase_rename/)].

### 2.4 Sapling (Meta)

- Native Sapling repos record renames in file headers. In git-backed repos there is nothing to record, so it detects renames by content similarity using `xdiff::edit_cost` with a maximum cost, which makes the check O(N) instead of O(N²) [D: [copytracing](https://sapling-scm.com/docs/dev/internals/copytracing/), [git modes](https://sapling-scm.com/docs/git/git_support_modes/)].
- **Bisect-based copy tracing** finds the commit where path P1 was deleted by bisecting between C1 and C2, reads the rename there, and recurses. That costs O(M·log H) versus O(M·N·H) for the full algorithm.
- The older heuristics covered only two rename shapes: same directory with a new name, and same name in a new directory.
- Lesson [I]: resolving **per step in history** (at the commit that deleted the path) is both cheaper and more accurate than one comparison across a long span. §5.9 confirms this on BoykoEngine.

### 2.5 Perforce

- `p4 move` needs the file opened for add or edit first. It records a move/delete + move/add pair. `-k` changes only server metadata; `-r` is rename-only without opening [D: [p4 move](https://help.perforce.com/helix-core/server-apps/cmdref/current/Content/CmdRef/p4_move.html)].
- For moves done outside p4, `p4 reconcile -M` pairs adds and deletes "if they are similar enough" and prints "… moved from" [D: [p4 reconcile](https://help.perforce.com/helix-core/server-apps/cmdref/current/content/CmdRef/p4_reconcile.html)].
- Thresholds are the server configurables `dm.status.matchlines`/`matchsize`. At `matchlines=100` the client skips the diff and compares LF-normalised checksums [C: [forum](https://forums.perforce.com/index.php?%2Ftopic%2F5393-reconcile-doesnt-recognize-file-renamesmoves=)].

### 2.6 Pijul

- Each file has a name vertex and a separate "inode" vertex. That makes renames commute with edits and with directory renames [D: [manual/theory](https://pijul.org/manual/theory.html)].
- Identity is recorded (`pijul mv`); an out-of-band move looks like a delete plus an add [I].

### 2.7 OS-level identity (Windows)

- **NTFS file id.** "In the NTFS file system, a file keeps the same file ID until it is deleted". ReFS has 128-bit ids, and its 64-bit id is not guaranteed unique. `ReplaceFile` keeps the *replacement's* id [D: [BY_HANDLE_FILE_INFORMATION](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/ns-fileapi-by_handle_file_information)].
  - §5.4 shows that git, Claude Code's Edit/Write tools and atomic saves all replace files, so the id survives moves but not edits [M].
- **Distributed Link Tracking.** It inserts an object id into a shortcut's target so the shortcut can find the file after a rename or move, even across volumes or machines. When that fails, the shell link falls back to a heuristic tree search using "the last known path of the file and file information that includes the creation date, file size, and file name and extension" [D: [MS docs](https://learn.microsoft.com/en-us/windows/win32/fileio/distributed-link-tracking-and-object-identifiers)].
  - This is the same layering recommended here: an identity token, then a metadata+content heuristic.

### 2.8 Locations inside files

- **W3C Web Annotation Data Model** (Recommendation, 2017-02-23) defines `TextQuoteSelector` (exact + optional prefix/suffix) and `TextPositionSelector` (start/end offsets). It warns that the position form is "very brittle with regards to changes to the resource" [D: [W3C](https://www.w3.org/TR/annotation-model/)]. §5.10 measures the same brittleness for `path:line`.
- **Hypothesis** stores three selectors: range (XPath), text position (character offsets), and text quote (exact text + 32-char prefix/suffix). Re-anchoring tries them in that order, then a context-first fuzzy search (Bitap from diff-match-patch) with an acceptance threshold, then a quote-only fuzzy search [D: [Hypothesis, 2013](https://web.hypothes.is/blog/fuzzy-anchoring/)].
- **CodeShovel** (ICSE 2021) builds method-level histories across file moves. On its oracle of 20 Java projects it reports 90% recall and 99% precision [C: [paper](https://www.cs.ubc.ca/~rtholmes/papers/icse_2021_grund.pdf)].
- **RefactoringMiner** detects Move Class/Method with AST matching, but only for Java (and recently JS) [D: [repo](https://github.com/tsantalis/RefactoringMiner)]. No Rust.

### 2.9 Summary

| System | Identity carrier | Out-of-band moves | Default threshold | Known gap |
|---|---|---|---|---|
| git | none (content) | inferred at diff time | 50% (JGit 60%) | rename + heavy edit; O(N²) limit 1000/7000; splits pick one piece |
| Mercurial | recorded copy metadata | `addremove -s` | 100% (exact) | fallback off by default |
| jj | (planned CopyId) | git-backend detection for display | git's | rebase does not follow renames |
| Sapling | recorded in native repos | `edit_cost` similarity in git repos, bisect | not documented | — |
| Perforce | recorded `p4 move` | `reconcile -M` | configurable | — |
| Pijul | inode vertex | none | — | out-of-band = delete+add |
| Windows shell links | NTFS object id | heuristic search (path, ctime, size, name, ext) | — | no content |
| Hypothesis | selectors | fuzzy quote + context | tunable | — |

The convergent design [I]: **record when you can, detect when you cannot, and only auto-accept exact or clearly dominant matches.**

---

## 3. Fingerprint options

### 3.1 Exact content hash

- Cryptographic strength is irrelevant here. The hash is an identity *hint*, and a match is re-verified by size, or by content when needed [I].
- What matters:
  - **equality with git's blob id**, so tracked files cost nothing and `git log --find-object` works;
  - **EOL normalisation**: with `core.autocrlf=true` (set system-wide here [M]), 1,833 of 2,555 working-tree files contain CRLF, while git blobs are LF [M];
  - **speed**, which does not matter because I/O dominates (§5.6).
- Choice: SHA-1 over `"blob <n>\0" + normalised bytes` = the git blob id. Normalisation means CRLF→LF when the file is text by git's auto rule (no NUL in the first 8,000 bytes); binary files are hashed raw.
  - This matches synthesis §5b's rule "SHA-1 for anything that leaves the machine" [I].
  - BLAKE3-128 would be equally fast (§5.6) but cannot be looked up in git.

### 3.2 Cheap pre-filters (no content read)

- **size + mtime** (git's stat cache). They detect "unchanged at the same path" but never a move.
- **NTFS file id.** A cheap and exact signal for a pure move within a volume. It is lost on any rewrite (§5.4).
- **NTFS ChangeTime.** It updates on the moved entry, and for a directory move only on the directory [M §5.5]. That makes "ChangeTime > last scan" a cheap way to scope candidates.
- **Basename and extension.** 148 of 150 exact renames in BoykoEngine kept their basename [M]. Only 0.12% of random file pairs share a basename, 48% share an extension, and 28.5% pass git's 50% size pre-check [M].

### 3.3 Similarity measures (evaluated in §5.8)

| Measure | Unit | Robust to | Weak on | Cost |
|---|---|---|---|---|
| git spanhash score | LF/64-byte chunks, byte-weighted | line edits, CRLF | **re-indent** (every line changes), reflow | 4.1 KB/doc features; ~10 µs/pair |
| **weighted normalised-line multiset** (recommended) | trimmed, whitespace-collapsed lines > 3 chars, byte-weighted | edits, re-indent, CRLF; gives **containment** both ways | line reflow (joins/splits) | 3.4 KB/doc exact; 256 B sketch |
| Jaccard of line sets / 3-line shingles | sets | exact moves | shingles decay 3× faster under edits (0.19 vs 0.51 at 30% edits) | — |
| MinHash / bottom-k (KMV) [Broder 1997] | sketch of a set | scalable pre-ranking | estimator noise | 0.6 µs/pair |
| SimHash 64-bit [Charikar 2002; Manku 2007] | bit vector | near-exact duplicates (web: k=3 of 64 bits) | fails at 30% edits (top-1 11/300) | 8 B, ns/pair |
| Token winnowing (MOSS) [Schleimer, Wilkerson, Aiken 2003] | k-token hashes, window minimum | reflow, re-indent, partial copies | identifier renames | §5.8 table |
| FastCDC chunk sets [Xia et al., ATC 2016] | gear-hash content-defined chunks | inserts in large binary/text | tiny files; compressed binaries change wholesale | GB/s-class |

- **References:**
  - MinHash: [Broder 1997](https://dblp.org/rec/conf/sequences/Broder97.html) and [Broder 2000](https://cs.brown.edu/courses/cs253/papers/nearduplicate.pdf).
  - SimHash: [Manku, Jain, Das Sarma 2007](https://research.google.com/pubs/archive/33026.pdf), which found 64-bit fingerprints with k = 3 reasonable at 8B pages [D].
  - Winnowing: [Schleimer et al. 2003](https://theory.stanford.edu/~aiken/publications/papers/sigmod03.pdf), which guarantees a shared fingerprint for any match longer than the guarantee threshold [D].
  - FastCDC: [USENIX ATC'16](https://www.usenix.org/system/files/conference/atc16/atc16-paper-xia.pdf) [D].
- **FastCDC verdict [I]:** it adds nothing for source text, where lines already are content-defined chunk boundaries. It only matters if moirai ever dedups large binaries. For binary assets (`.spv`, `.png`, fonts), use the exact hash + file id + basename/size heuristics and do no similarity.
- **Language-aware normalisation [I]:**
  - whitespace/EOL normalisation is mandatory;
  - comment stripping is not worth a per-language parser;
  - token-level winnowing is a fallback for reflow (rustfmt re-wrapping);
  - symbol paths (tree-sitter: `impl Foo / fn bar`) are the right key for *in-file* anchors in code, but not for file identity.

---

## 4. Machine and method

- **Machine:** AMD Ryzen 9 5900HS (8C/16T), 16 GB RAM, NVMe SSD (HFM512GD3JX013N), Windows 11 Home 10.0.26200, Defender real-time protection **on**.
- **Tools:** Rust 1.98.1, git 2.54.0.windows.1, Python 3.14.5.
- **Load:** during every run, CPU load was 97–100%. Other sessions were compiling (rustc, miri, rust-analyzer), desktop applications were running, and Defender was scanning. Absolute times are therefore **pessimistic**; ratios are more reliable. Where it mattered, I report min and median over 5–7 repetitions.
- **Target:** the BoykoEngine repository (main checkout), read-only. The walk excluded `.git/` and `target/`, applied `.gitignore` via the `ignore` crate 0.4.25 (hidden files included), and found **2,555 files** in 283 directories, 69.9 MB. `git ls-files -co --exclude-standard` lists 2,556 [M].
- **Git access:** only read-only commands with `GIT_OPTIONAL_LOCKS=0` (`log`, `diff <tree> <tree>`, `ls-tree`, `cat-file`, `ls-files`). No `git status`, which can write the index.
- **Probes** (not published):
  - `fpprobe` (Rust: walk/hash/sim/sim2/ram/times);
  - `anchors.py`, `missing.py`, `inexact.py`, `winnow.py`;
  - `fileid-test/` (a scratch git repo created there).
  - One deviation: the cross-volume file-id test moved a 11-byte probe file to `D:/tmp` and back within the same command. `D:/tmp` is not a repository, and the file no longer exists there (verified).

---

## 5. Measurements

### 5.1 Tree shape [M]

| Metric | Value |
|---|---|
| files / dirs / bytes | 2,555 / 283 / 69.9 MB |
| file size p10 / p50 / p90 / p99 / max | 885 B / 10.2 KB / 55 KB / 333 KB / 1.87 MB |
| mean repo-relative path length | 45.3 B |
| unique basenames | 2,252. 420 files share a basename: `mod.rs` 87, `Cargo.toml` 29, `lib.rs` 25, `README.md` 8 |
| identical-content groups | 15 groups, 30 files (1.2%): `.claude/agents/*.md` ≡ `.zcode/agents/*.md` mirrors, PNG fixtures |
| near-duplicates (normalised-line similarity ≥ 0.99, text) | 24 of 2,417 (1.0%): the mirrors, `sdf_ssao{,_low,_high}.comp.hlsl`, `playground.rs` ≈ `_hud_probe.rs` |
| files containing CRLF in the working tree | 1,833 / 2,555 (`core.autocrlf=true` in the system gitconfig) |
| git worktrees of this repo | 44 (40 under `<lanes-dir>`, 3 under `.claude/worktrees`, 1 main) |

### 5.2 Walk + stat [M]

| Variant | median | min |
|---|---|---|
| `ignore` serial walk, names only | 137.7 ms | 116.6 ms |
| `ignore` serial walk + metadata (size, mtime; free from `FindNextFile` on Windows) | 111.1 ms | 102.3 ms |
| `ignore` parallel walk + metadata, 2 / 4 / 8 / 16 threads | 62.8 / 44.7 / 32.1 / 28.5 ms | 51.2 / 36.9 / 26.9 / 25.6 ms |
| NTFS file id by opening each file (`CreateFileW` + `GetFileInformationByHandle`) | 159.7 ms (62 µs/file) | — |
| NTFS file id + size + times by **directory enumeration** (`GetFileInformationByHandleEx(FileIdBothDirectoryInfo)`) over the 283 walked dirs | 28.9 ms (11 µs/entry) | — |

- The directory-enumeration id equals the handle id in 255/255 checked files.
- Extrapolation [I]: 1e5 files ≈ 4 s serial or ≈ 1.1–1.3 s with 8 threads for walk+stat, and ≈ 1.1 s for directory-enumeration ids. That is fine for an on-demand scan and far too slow for a per-command hot path, so moirai must not walk on every command.

### 5.3 git as a helper [M]

| Operation (PowerShell `Measure-Command`, 3–5 runs) | Time |
|---|---|
| `git --version` (process spawn floor) | 130–300 ms |
| `git rev-parse HEAD` | 180–750 ms |
| `git ls-files -s` (295 KB index, 2,556 entries) | 235–800 ms |
| `git ls-files -o --exclude-standard` | 620–990 ms |
| `git diff --no-renames --name-status HEAD~500 HEAD` (797 A, 3 D, 535 M) | 180–250 ms |
| same with `-M` | 275–360 ms |
| `git log -M --diff-filter=R` over all 1,392 commits | 0.43 s (bash) |
| `git log --follow` on one file | ~2.5 s (bash) |

- Spawning git costs more than the whole in-process walk.
- moirai should read git **in-process** (gix: index, trees, and `Rewrites` rename tracking), not shell out per resolution [I].

### 5.4 Which operations keep the NTFS file id [M]

These were run in a scratch repo under the probe directory, with `os.stat().st_ino` (the NTFS file index).

| Operation | File id |
|---|---|
| `mv` within a volume; `git mv` | **kept** |
| `git checkout` of a branch where the file is identical | kept |
| in-place write (Python append; PowerShell `Set-Content`, `Add-Content`) | kept |
| `git checkout` of a branch where the file differs (each switch) | **new** |
| `git stash` / `git stash pop` | new, new |
| `git reset --hard` on a modified file; `git rebase` touching the file | new |
| write temp + `os.replace` (atomic save) | new |
| `cp` | new |
| move to another volume (C: → D: and back) | new each time |
| **Claude Code `Edit` tool** | **new** |
| **Claude Code `Write` tool** (overwrite) | **new** |

- Consequence: in this workflow, agents edit files with Edit/Write, and git rewrites them on every checkout and rebase in 44 worktrees.
- The file id is therefore valid **only as a "pure move since the last scan" signal**. It is never a durable identity.

### 5.5 NTFS timestamps on moves [M]

- **Moving a file** (`src/g2.rs → docs/g2.rs`) keeps its id, CreationTime and LastWriteTime. It sets the file's **ChangeTime** to the move time and updates LastWrite/Change of both parent directories.
- **Moving a directory** (`deep → deep_moved`) updates only the directory's ChangeTime. Its descendants' ChangeTimes do not change.
- So a scan can scope candidates cheaply: entries with ChangeTime or CreationTime after the last scan, plus the whole subtree of any directory whose ChangeTime is after it. All three times come free with `FileIdBothDirectoryInfo` [M].

### 5.6 Hashing throughput [M]

| Pass | Time | Files/s | MB/s |
|---|---|---|---|
| first read + BLAKE3, serial (first touch in this process; includes Defender) | 1,407 ms | 1,816 | 50 |
| warm read + BLAKE3, serial (3 reps) | 370–440 ms | 5.8–6.9k | 159–189 |
| warm read only, serial | 424 ms | 6.0k | 165 |
| warm read + BLAKE3, 4 / 8 threads | 136 / 91 ms | 18.8k / 28.0k | 515 / 764 |

In-memory, single thread, all 2,555 files, best of 5:

| Algorithm | All files | < 4 KB (717 files) | 4–64 KB (1,629) | 64 KB–1 MB (206) | ≥ 1 MB (3) |
|---|---|---|---|---|---|
| BLAKE3 (`blake3` 1.8, runtime SIMD) | 1,132 MB/s | 2.16 µs/file | 17.5 µs | 99 µs | 1,767 MB/s |
| xxh3-64 (`twox-hash` 2.1, baseline x86-64 target) | 3,695 MB/s | 0.12 µs | 8.3 µs | 82 µs | 2,995 MB/s |
| xxh3-128 | 3,717 MB/s | 0.12 µs | 4.5 µs | 74 µs | 8,008 MB/s |
| SHA-1 of the git blob (`sha1` 0.10) | 1,207 MB/s | 2.16 µs | 14.1 µs | 137 µs | 1,745 MB/s |
| SHA-256 (`sha2` 0.10, SHA-NI) | 1,252 MB/s | 1.13 µs | 15.2 µs | 126 µs | 1,327 MB/s |
| git spanhash features (my HashMap port) | 169 MB/s | — | — | — | — |
| normalised lines + 3-shingles | 386 MB/s | — | — | — | — |

- Vendor claims: XXH3 at 59.4 GB/s on large data with AVX2 [D: [xxHash](https://github.com/Cyan4973/xxHash)]; BLAKE3 "much faster than MD5, SHA-1, SHA-2" [D: [BLAKE3](https://github.com/BLAKE3-team/BLAKE3)]. On files of this size, per-call overhead dominates, and my build did not enable AVX2 for xxh3.
- **Conclusion:** the hash is ≤ 15% of read+hash. File open/read under Defender costs ~150 µs/file warm and ~550 µs/file cold. So the stat cache (never re-read an unchanged file) matters 5–10× more than the choice of hash [M/I].
- Cost of a full rehash at 1e5 files [I]: ~17 s warm serial, ~3.6 s with 8 threads, ~55 s cold. It must never happen implicitly.

### 5.7 Rename history of BoykoEngine [M]

`git log -M` over all 1,392 commits reachable from HEAD (merge diffs not shown):

| Class | Count |
|---|---|
| renames at the default `-M50%` | 159 |
| of which exact (R100) | 150. 131 are one bulk `docs/ → docs/archive/`; the rest are crate/module reshuffles. **148 of 150 kept the basename** |
| inexact renames at `-M20%` | 16. **7 of them score < 50%**, so default git shows add+delete |
| pure deletions / additions | 73 / 2,650 |

The 16 inexact renames, re-scored at the commit where they happened (`inexact.py`):

| git | normalised sym. | old-in-new | new-in-old | old → new | other new files receiving ≥ 10% of the old file |
|---|---|---|---|---|---|
| R047 | 0.47 | 0.47 | 0.61 | `task.rs → task/mod.rs` | scoped.rs 22%, detached.rs 12% |
| R049 | 0.50 | 0.50 | **1.00** | `emit.rs → emit/mod.rs` | shaders.rs 38%, cf.rs 12% |
| R044 | 0.43 | 0.43 | **0.98** | `rhi_impl.rs → rhi_impl/device.rs` | encoder.rs 33%, mod.rs 24% |
| R070 | 0.70 | 0.70 | 0.99 | `gpu_scene.rs → gpu_scene/mod.rs` | csm.rs 14% |
| R046 | 0.45 | 0.45 | **1.00** | `component_registry.rs → component_registry/mod.rs` | serialize.rs 25%, clone.rs 13%, required.rs 12% |
| R066 | 0.64 | 0.91 | 0.64 | `solver.rs → solver/mod.rs` | — |
| R095 | 0.94 | 0.97 | 0.94 | `entity.rs → entity/entity.rs` | — |
| R037 | 0.37 | **0.83** | 0.37 | `derive_event.rs → event_attribute.rs` | — |
| R073 / R065 / R097 / R058 / R052 / R052 / R024 / R023 | 0.22–0.99 | … | … | test/shader renames | … |

- **The dominant inexact pattern in this Rust codebase is the module split.** Its signature is new-in-old ≈ 1.0 for the new `mod.rs` together with several siblings that each receive part of the old file. The whole-file similarity of any one piece is often < 0.5.
- git reports whichever piece is largest, or nothing. The right model for a moirai reference is a **split**: the file-level reference fans out, and each in-file anchor re-resolves to the piece that holds its text.

### 5.8 Similarity quality (synthetic edits over real files) [M]

- **Corpus.** The 2,417 text files of BoykoEngine.
- **E1 (background noise).** For each file, I took the best score against any *other* file.
- **E2 (moves).** 300 random files with ≥ 20 lines. For each, I built a variant and searched for it among all other files plus the variant.
  - "top-1" means the variant strictly beats every other file. The ceiling is 295–296/300 because 4–5 sampled files have an identical or near-identical twin, which ties.
  - edit*N*: *N*% of lines replaced with random text of the same length.
  - split_half: the moved file is the first half.
  - merged_into: the original is appended to another random file.

**E1: share of files whose nearest *other* file scores at least x (n = 2,417)**

| Measure | ≥ 0.3 | ≥ 0.5 | ≥ 0.7 | ≥ 0.8 | ≥ 0.9 | ≥ 0.99 |
|---|---|---|---|---|---|---|
| git score | 10.5% | 4.1% | 2.3% | 1.7% | 1.2% | 0.3% |
| normalised-line weighted sym. | 9.6% | 3.8% | 2.1% | 1.4% | 1.2% | 1.0% |
| containment (A in B) | 14.7% | 6.2% | 3.4% | 2.6% | 2.1% | 1.8% |
| 3-line-shingle Jaccard | 3.7% | 1.7% | 1.1% | 1.1% | 1.1% | 0.8% |

For a random query, the best *non-matching* file has normalised sym. p90 = **0.24** and containment p90 = 0.29.

**E2: top-1 accuracy / mean score of the true variant (300 queries per row)**

| Variant | git score | line-set Jaccard | 3-shingle Jaccard | SimHash-64 | **norm. weighted sym.** (p10) | **containment old-in-new** | sketch stage-1 recall@10 (KMV-64 / rare-line KMV-64) |
|---|---|---|---|---|---|---|---|
| exact | 295 / 0.98 | 295 / 1.00 | 295 / 1.00 | 295 | 296 / 1.00 (1.00) | 296 / 1.00 | 300 / 300 |
| CRLF | 294 / 0.98 | 295 / 1.00 | 295 / 1.00 | 295 | (normalised away) | — | — |
| re-indent (+4 spaces) | **12 / 0.02** | 295 / 1.00 | 295 / 1.00 | 295 | 296 / 1.00 (1.00) | 296 / 1.00 | 300 / 300 |
| edit 10% | 292 / 0.88 | 292 / 0.80 | 291 / 0.55 | 187 | 296 / 0.89 (0.85) | 296 / 0.90 | 300 / 300 |
| edit 30% | 289 / 0.69 | 285 / 0.51 | 282 / 0.19 | 11 | 295 / 0.68 (0.62) | 295 / 0.70 | 300 / 300 |
| edit 50% | 284 / 0.48 | 274 / 0.30 | 231 / 0.05 | 1 | 291 / 0.47 (0.41) | 287 / 0.50 | 299 / 300 |
| edit 70% | 271 / 0.29 | 251 / 0.16 | 127 / 0.01 | 0 | 276 / 0.28 (0.22) | 271 / 0.31 | 289 / 299 |
| split (first half) | 288 / 0.51 | 288 / 0.53 | 290 / 0.49 | 203 | 292 / 0.53 (0.46) | 290 / 0.53; **new-in-old 295 / 1.00** | 300 / 300 |
| merged into another file | 258 / 0.54 | 258 / 0.55 | 282 / 0.55 | 196 | 263 / 0.54 (**0.10**) | **296 / 1.00 (1.00)** | 287 / 298 |

Readings:
- The weighted normalised-line score matches git on edits and is immune to re-indentation and CRLF.
- Its two containment directions turn "split" (new-in-old = 1.0) and "merge" (old-in-new = 1.0) into signals that symmetric measures blur (p10 0.10 for merges).
- SimHash and 3-line shingles are unsuitable for moved-and-edited files.
- Bottom-64 sketches keep the true target in the top 10 in 287–300/300 cases. Hashing only rare lines (lines in ≤ 3 files) raises that to 298–300/300, so sketch → exact re-score is a sound two-stage design.

Token winnowing versus lines under reflow and identifier renames: see §5.8b.

### 5.8b Reflow and identifier renames [M]

`winnow.py` ran 50 random tracked `.rs/.md/.hlsl/.toml` files (≥ 30 lines) against 2,221 files. Winnowing used 5-token k-grams with a window of 4, giving 2,045 fingerprints per document on average. It is compared with the weighted normalised-line score.

| Variant | Winnowing (token Jaccard): top-1 / mean | Normalised lines: top-1 / mean |
|---|---|---|
| reflow 20% (join 20% of adjacent line pairs, as a re-wrap would) | 50/50 / **1.00** | 50/50 / 0.70 |
| reflow 50% | 50/50 / **1.00** | 49/50 / **0.40** (below 0.5) |
| edit 30% of lines | 50/50 / 0.53 | 50/50 / **0.67** |
| rename 20% of identifiers (≥ 4 chars, every occurrence) | 50/50 / **0.57** | 50/50 / **0.36** (below 0.5) |

- For both measures, the best *non-matching* file averages 0.07–0.08.
- Readings:
  - lines win on scattered edits;
  - tokens win on reflow and identifier renames (e.g. a type rename that touches many lines);
  - both keep the correct file ranked first here, but only the maximum of the two stays above the 0.5 auto-threshold in every row.
- Winnowing costs ~8 KB of fingerprints per document, which is too much to store per reference. It is also slow in Python (57 s to fingerprint 2,221 files).
- So [I]: compute it only at stage 2, on the ≤ 10 candidates, and only when the old content is available (git blob). Use `score = max(line_sym, token_jaccard)`.

### 5.9 Long span vs per-step resolution on real history [M]

`missing.py` looked at the text files present at `8d008b49` (2026-05-23, 119 files) and gone from HEAD (2026-09-22). `git diff -M 8d008b49 HEAD` reports **0 renames**. Of the 18 missing files:

- **1** is findable by content. `component_registry.rs`: 94% of its normalised lines are in `component_registry/mod.rs`, although the symmetric similarity is only 0.31 because `mod.rs` grew. Git misses it at any sensible threshold, even though the per-commit history has `R046` for it.
- **4** are empty or 2-line files (`tuple/mod.rs`, `component_tuple.rs`, `component_tuple_trait.rs`, `iterators.rs`). Content carries no signal.
- **13** were truly deleted or rewritten. Their best containment in any HEAD file is ≤ 0.18, and ≤ 0.08 on rare lines only. There are **0 false positives** at a 0.5 threshold.

Conclusion [M/I]: over long spans, symmetric similarity collapses as the new file accumulates unrelated edits, while containment of the *old* content survives. Resolving at each step, as git per-commit and Sapling's bisect do, is more reliable than resolving late. moirai should resolve at the earliest event, and when it must resolve late it should score by containment.

### 5.10 Location anchors inside files: survival on real history [M]

**Method (`anchors.py`).**
- For random `.rs`/`.md` files at commit A, I took up to 3 definition/heading lines (`fn`, `struct`, `impl`, `mod`, `#`…) and 2 random non-blank lines per file.
- **Ground truth:** a `difflib` line alignment of the A version against the HEAD version, following git renames. An aligned line has "survived"; otherwise it "changed" (edited or deleted).
- **Strategies:**
  - S0 = keep `path:line`;
  - S1 = nearest exact normalised line;
  - S2 = exact line with ±2 lines of context;
  - S4 = cascade: S2, then S1 only if the match is unique, else unresolved; for changed lines, a fuzzy proposal (line 0.6 + context 0.4 similarity ≥ 0.75) is reported separately.

| Span (A → HEAD) | Anchors (survived / changed) | S0 `path:line` correct on survived | S0 silently bound on changed | S1 wrong | S2 unresolved | **S4 correct / wrong / unresolved** (survived) | S4 on changed: false bind / unresolved (of which a fuzzy "edited" proposal) |
|---|---|---|---|---|---|---|---|
| 2 weeks (`128233be`) | 2,843 / 7 | 2,680 (94.3%) | 7 / 7 | 4 (0.1%) | 16 | **2,843 / 0 / 0** | 0 / 7 (0) |
| 2 months (`d93e425e`) | 2,867 / 19 | 2,565 (89.5%) | 19 / 19 | 10 (0.35%) | 43 | **2,866 / 0 / 1** | 2 / 17 (6) |
| 4 months (`8d008b49`) | 224 / 89 | **62 (27.7%)** | 89 / 89 | 13 (5.8%) | 64 | **213 / 2 / 9** (95.1 / 0.9 / 4.0%) | 2 / 87 (20) |

- A line-number anchor never reports failure: it always points somewhere. After 4 months, 72% of line anchors whose text still exists point at other text, and 100% of anchors whose text changed point at whatever took their place.
- This is the mechanism behind the "184 of 282 line anchors dead" figure in the owner's memory corpus (digest [01]) [I].
- Context-plus-content anchors fail loudly (unresolved) far more often than they fail silently (wrong).

### 5.11 Cost of one resolution [M]

Single thread, min of 5.

| Step | N = 1e3 | N = 1e4 | N = 1e5 |
|---|---|---|---|
| stage 1: bottom-64 sketch compare against N candidates | 0.61 ms | 9.05 ms | 114 ms |
| exact weighted-line score against N precomputed multisets | 4.0 ms | 59.6 ms | 738 ms |
| git spanhash score against N precomputed feature sets | 100 ms (first run, noisy) | 88 ms | 1,031 ms (~10 µs/pair) |
| SimHash Hamming against N | ≤ 2 ms at every N (noise-dominated) | | |

| Step | Time |
|---|---|
| stage 2: read + normalise + score the top 10 candidates from disk | **3.1 ms** (median) |
| worst case, no index: read + normalise + score all 2,417 text files from disk | 2.9–4.2 s with my unoptimised normaliser under full load; the read alone is 0.42 s serial / 0.09 s with 8 threads (§5.6) |

Feature sizes per text document: raw 27 KB, git spanhash 4.1 KB, weighted-line multiset 3.4 KB (12 B/entry), bottom-64 sketch 512 B as u64 or 256 B as u32.

### 5.12 RAM of a fingerprint index [M]

Measured with a counting global allocator. Paths are real BoykoEngine paths, replicated with a prefix above 2,555.

| Structure | 1e3 | 1e4 | 1e5 |
|---|---|---|---|
| core: path `Box<str>` + size + mtime + file id + 128-bit hash + map by hash + map by basename | 224 KB (224 B/entry) | 1.63 MB (163 B/entry) | 14.0 MB (140 B/entry) |
| + 32 × u32 sketch for **every** file | +128 KB | +1.28 MB | +12.8 MB |
| + 64 × u64 KMV for every file | +512 KB | +5.12 MB | +51.2 MB |
| + SimHash-64 | +8 KB | +80 KB | +0.8 MB |

Recommended layout [I]:
- sketches only for **referenced** files. The owner's graph is 0.3–0.5M nodes over three years (digest), but file references are likely 1e3–1e4.
- a path snapshot for "what is new since the last scan" (core row) only if ChangeTime scoping is not enough.
- both on disk in the store, read during a scan, **nothing resident at idle**.

| Scenario | RAM during a scan | At idle |
|---|---|---|
| 1e3 refs, 1e4-file tree | ~0.4 MB refs + optional 1.6 MB snapshot | 0 |
| 1e4 refs, 1e5-file tree | ~4.2 MB refs + optional 14 MB snapshot | 0 |
| 1e5 refs (every file referenced) | ~42 MB, streamed from disk during the scan | 0 |

---

## 6. Where candidates come from (lazy search scope)

The cost of similarity is proportional to the candidate set, so the scope matters more than the metric. In priority order:

1. **Nothing to search.**
   - If the stored path still exists with the same (size, mtime, file id), the file is unchanged.
   - If it exists with a different stat, re-hash it. Same `oid`: unchanged. Different `oid`: *content changed at the same path*, which is not a move → re-resolve in-file anchors and mark referrers stale per the synthesis `stale` semantics.
2. **Unbound new paths in the same worktree.** These are paths that exist now, are not the stored path of any live reference, and appeared since the last scan.
   - Cheapest: entries with CreationTime/ChangeTime after the last scan, from directory enumeration (§5.5).
   - Fallback: diff against a stored path snapshot (git's untracked-cache idea, keyed on directory mtime).
3. **git knowledge, in-process via gix:**
   - staged renames (index vs HEAD);
   - per-commit renames between `observed_git_sha` and HEAD, followed step by step, not one long-span diff (§5.9);
   - untracked files (read-only; never `git status`).
4. **Priors inside the candidate set:**
   - same basename (148/150 real exact renames; 76% in Linux per git's source);
   - same extension;
   - size within git's pre-check;
   - **directory-rename inference**: once two or more references moved `X/ → Y/`, prefer `Y/` for the rest, which explains the 131-file `docs → docs/archive` move with one rule.
5. **Last resort:** a gitignore-aware walk of the bound worktree (27–111 ms here; ~1–4 s at 1e5), then the sketch stage over all unbound text files.

Scope rules:
- **Never search across worktrees.** The same content exists in up to 44 worktrees, and a file reference is bound to a (branch → worktree) pair.
- **Never treat a still-bound path as a candidate.** This one rule removes most duplicate-content ambiguity: when one of two identical mirrors is moved, the untouched one is still bound to its own reference.

---

## 7. Ambiguity rules

| Situation | Signal | Decision |
|---|---|---|
| exactly one unbound candidate with the same `oid` or file id | exact | **auto-rebind** (`moved`) |
| several unbound candidates with the same `oid` (true copies) | exact, tie | tie-break by same basename → nearest directory (path edit distance) → most recent ChangeTime. If still tied: `ambiguous`, list the candidates |
| old path still exists **and** a new path has the same content | copy | keep the reference; optionally note the `copied_to` path |
| best sym ≥ 0.5, margin ≥ 0.2 over the runner-up | moved + edited | **auto-rebind**, mark referrers `changed since bound` |
| old-in-new ≥ 0.8 and new-in-old < 0.5 | merged into host | rebind to the host with a `merged` flag; in-file anchors re-resolve inside the host |
| ≥ 2 candidates with new-in-old ≥ 0.8 whose old-in-new shares sum to ≥ 0.6 | split | **do not pick one**; file-level reference becomes `split → {pieces}`, and in-file anchors resolve individually |
| best in [0.3, 0.5), or margin < 0.2 | weak | propose only (CLI/MCP shows candidates with scores) |
| empty or tiny file (< ~5 normalised lines) | no content signal | only file id / exact-oid+basename; otherwise ask |
| nothing ≥ 0.3 | deleted | reference is dangling: tombstone semantics, every referrer is told (R-integrity) |
| stored path alive but content equals *another* reference's old content | swap (a↔b) or rename-over | check swaps before declaring "content changed" |

Threshold rationale [M/I]:
- 0.5 is git's default.
- It sits above the p90 background (0.24/0.29) and below the p10 score at 30% churn (0.62).
- 3.8% of files have some other file ≥ 0.5, so the margin rule, not the threshold, protects the near-duplicate families (mirrors, shader variants).

---

## 8. Recommended fingerprint + resolution algorithm

### 8.1 Stored state

```
FileRef {                       // one per (referenced file, bound worktree/branch)
  path: RelPath,                // exact case, '/' separators; compare case-insensitively on NTFS
  oid: [u8; 20],                // git blob id of EOL-normalised content (raw for binary)
  size: u64, mtime: i64,        // stat cache (racy rule: mtime >= last_scan_start => re-verify)
  file_id: u64, vol: u32,       // hint only: pure-move detector, never identity
  weight: u32, distinct: u32,   // Σ bytes / count of distinct normalised lines (text only)
  sketch: [u32; 64],            // bottom-64 of normalised-line hashes (text only); restrict to rare
                                // lines when a per-worktree line-frequency table is available
  observed_git_sha: [u8; 20],   // HEAD of the bound worktree when last verified
  state: bound | changed | moved(from) | merged(into) | split(pieces) | ambiguous(cands) | dangling
}
Anchor {                         // location inside a file; never a bare line number
  file: FileRefId,
  line_hash: u64,               // normalised text of the anchored line
  ctx_hash: u64,                // normalised ±2-line window
  line_hint: u32,               // last resolved line (tie-break only)
  excerpt: String,              // <= 120 chars, for fuzzy proposal and display
  symbol: Option<String>,       // e.g. "impl Foo / fn bar" or "## Heading" (optional, tree-sitter later)
}
```

### 8.2 Resolve(ref), run lazily on dereference and eagerly on events

1. `stat(path)` via directory enumeration of the parent.
   - Same size, mtime and file id → **bound**, done.
   - Different → re-hash. Same `oid` → bound (touch only). Different `oid` → `changed`: re-anchor its `Anchor`s (§8.3) and mark referrers suspect.
2. Path missing → build the candidate set C (§6).
3. Look in C for the same `(vol, file_id)` → verify size → **moved**.
4. Look in C for the same `oid`: one hit → **moved**; several → tie-break (§7).
5. If the file was git-tracked at `observed_git_sha`, follow per-commit renames up to HEAD with gix, using `Rewrites{percentage: Some(0.2)}` so that splits are visible. Accept R ≥ 50 directly. For R in 20–49, confirm with containment and the split test of §7, because 7 of the 16 real inexact renames fall in that band (§5.7).
6. Stage 1 (text only): rank C by sketch similarity (rare-line sketch) and keep the top 10.
   - If C is empty or too small, widen to all unbound text files of the worktree (one walk).
7. Stage 2: read the ≤ 10 candidates and classify with §7.
   - **Old side, exact:** when the old `oid` is in the git object database (the file was committed or staged at some point), read that blob in-process and compute exact sym, old-in-new and new-in-old from weighted normalised lines. Take the maximum with token winnowing (§5.8b).
   - **Old side, estimated:** otherwise, estimate old-in-new as the fraction of the 64 stored sketch hashes present in the candidate's full line-hash set, with σ ≈ 0.06 [I]. Derive new-in-old from the stored distinct-line count.
8. Persist the outcome as a moirai operation on the bound branch, e.g. `FileRefMoved{from, to, evidence, score}`, so the history of a reference is versioned and merges like everything else. Notify referrers.

### 8.3 Re-anchor(anchor) inside a resolved file

This cascade was measured in §5.10.

1. Exact `ctx_hash` window match; nearest to `line_hint` if repeated.
2. Else exact `line_hash`, but only if unique.
3. Else fuzzy (line 0.6 + context 0.4 ≥ 0.75) → **proposal** with state `changed`, never a silent rebind.
4. Else `unresolved` → referrers are told.

For a `split` file, run steps 1–3 against every piece.

### 8.4 When it runs (zero idle CPU)

- **Events, when present:**
  - `moirai mv/rm/add` (certain; the other lens);
  - Claude Code `PostToolUse` on Bash commands that move files (`mv`, `git mv`, `Move-Item`, `ren`) → targeted resolve of refs under the touched paths;
  - `PostToolUse` Edit/Write on a referenced file → re-anchor;
  - optional git `post-checkout`/`post-merge`/`post-rewrite` hooks, which need owner consent to install.
- **Session start / `moirai doctor`:** step 1 for all refs of the current worktree. With directory enumeration of only the parent directories involved, that is ~11 µs per entry listed [M], i.e. milliseconds for 1e3 refs [I].
- **On dereference (lazy):** a context pack or `moirai show` that touches a dead path resolves it inline, within a bounded budget. If the budget runs out, it shows `unresolved (resolving…)`.
- **No watcher is required for correctness.** A watcher, if the other lens recommends one, only shortens the time to detection, and §5.9 shows that detecting early is worth a lot.

### 8.5 Failure modes

| # | Failure mode | Evidence | Mitigation |
|---|---|---|---|
| F1 | move + > 50–70% edit: content no longer identifies the file | E2: edit70 top-1 276/300 but mean score 0.28 (< threshold) | resolve early (events, per-commit git); explicit `moirai mv`; otherwise report `dangling` + candidates |
| F2 | module split: no single target | 6/16 inexact renames; 7/16 below git's 50% | `split` state + per-anchor resolution |
| F3 | empty/trivial files | 4/18 missing files had 0–2 lines | exact oid + basename only; else ask |
| F4 | identical copies and mirrors | 1.2% of files in exact-dup groups; 1.0% near-dups | only unbound candidates; tie-break; else `ambiguous` |
| F5 | boilerplate-heavy files (Cargo.toml, mod.rs, `.stderr` tests) | background p90 0.24; 420 files share a basename | margin rule; rare-line weighting in the sketch |
| F6 | re-indent / reformat | git top-1 12/300 on re-indent | whitespace-normalised lines (296/300) |
| F7 | line reflow (rustfmt re-wrap) | lines drop to 0.40 at 50% reflow; tokens keep 1.00 (§5.8b) | stage-2 score = max(lines, token winnowing) when the old blob is available |
| F7b | identifier rename across a file (type/field rename) | lines 0.36, tokens 0.57 (§5.8b) | same as F7; in-file anchors fall back to the fuzzy proposal |
| F7c | old content unavailable for stage 2 (never committed, never staged) | exact scores need both sides | estimate containment from the stored 64-hash sketch (σ ≈ 0.06 [I]); optionally store the full line-hash set (~1.1 KB mean as u32) for referenced files |
| F8 | EOL differences across worktrees / git | 72% CRLF in the working tree | hash EOL-normalised content = git blob id |
| F9 | case-only rename on NTFS | — | exact-case comparison from directory listing |
| F10 | cross-worktree false match | 44 worktrees with identical content | candidates never leave the bound worktree |
| F11 | file moved outside the worktree or into an ignored path | invisible to the walk | report `dangling`; allow explicit rebind |
| F12 | scan during an atomic save (temp file present, original gone) | Claude Edit/Write replace files (§5.4) | ignore temp patterns; racy rule: re-check entries newer than scan start |
| F13 | swap (a↔b) or rename-over | — | cross-check changed-in-place files against other refs' old `oid` |
| F14 | long-span lazy resolution | pairwise diff found 0 renames over 4 months | per-commit replay; containment instead of symmetric score |
| F15 | file id used as identity | new id after git checkout/stash/rebase, Claude Edit/Write, atomic save | hint only; always verify by size/content |
| F16 | binary assets moved and re-exported | compressed formats change wholesale | exact oid + file id + basename/size; no similarity |

---

## 9. Relation to the other R4 options

- **Explicit `moirai mv/rm/add`.** Certain and cheap, like `hg mv` and `p4 move`. It misses everything done out of band: Explorer, agent shell `mv`, git checkout/rebase, editor refactors.
  - Every VCS that records moves still ships a detector for exactly that gap (`hg addremove -s`, `p4 reconcile -M`, Sapling on git repos).
- **Automatic re-binding.** This report's pipeline. It covers every actor, but it is probabilistic beyond exact matches.
- **Recommendation [I]: both.** Explicit commands and hooks produce *recorded* moves (evidence = `recorded`). The content pipeline produces *detected* moves (evidence = `file_id | oid | git | similarity(score)`). Auto-apply only the exact ones.
  - moirai must not trust a guess any more than the owner does. That is the same stance as PLANFENCE-style deterministic staleness in the digest.

---

## 10. Implications for moirai

1. **Never store line numbers as identity.** Store content anchors (line + context hashes). Measured: 72% of `path:line` anchors silently wrong after 4 months; cascade anchors 95% correct and 0.9% wrong.
2. The file-reference key is `(worktree binding, path)` plus the `oid` = git blob id of EOL-normalised bytes. It is equal to git's index for tracked files and robust to autocrlf.
3. The NTFS file id is a free pure-move detector. Directory enumeration costs 11 µs/entry, and any rewrite resets the id: git checkout/stash/rebase, Claude Edit/Write, atomic save.
4. Similarity uses weighted normalised lines with containment both ways, not git's chunk score and not SimHash. It is robust to re-indent and CRLF, and it recognises splits and merges. Stage 2 takes the maximum with token winnowing to cover reflow and identifier renames.
5. Search in two stages: a 256 B sketch per referenced file, then exact re-score of ≤ 10 candidates (3 ms).
6. Resolve early and per step. Long-span pairwise comparison loses renames that the per-commit history keeps.
7. **Split is a first-class outcome** in Rust codebases (`foo.rs → foo/mod.rs + siblings`), not an error.
8. Read git in-process: gix index/trees/rename tracking. Spawning git costs 130–750 ms per call here.
9. RAM: ~420 B per referenced file on disk, 0 at idle; ~14 MB for an optional path snapshot of 1e5 files.
10. No resident watcher is needed for correctness, which fits the ~zero-idle-CPU requirement.

### Owner calls this lens cannot make

1. **Auto-rebind policy.** May moirai re-bind silently on similarity (≥ 0.5 with a 0.2 margin, flagged `changed`)? Or must every non-exact re-bind wait for a confirmation (owner or agent)?
2. **What gets indexed.** Should only files referenced by nodes be tracked (recommended: ~420 B per reference)? Or a full per-worktree file index, so moirai can answer "where did X go" for any file (~14 MB per 1e5 files per worktree during scans)?
3. **Hooks.** May moirai install git hooks (`post-checkout`/`post-merge`/`post-rewrite`) and Claude Code `PostToolUse` move hooks in BoykoEngine and its 44 worktrees?
4. **Out-of-tree references.** Must references to gitignored or out-of-repo files be supported (`assets/models`, scratchpads, `~/.claude` memory)? They are invisible to a gitignore-aware walk.
5. **Splits.** When a referenced file splits, should its file-level references fan out to all pieces automatically, or be flagged for a decision?
6. **Symbol anchors.** Are text-only content anchors enough, or should code anchors also carry tree-sitter symbol paths? That adds per-language parsers to the binary.

---

## 11. Sources

- git: [diffcore-rename.c](https://github.com/git/git/blob/master/diffcore-rename.c), [diffcore-delta.c](https://github.com/git/git/blob/master/diffcore-delta.c), [git-diff](https://git-scm.com/docs/git-diff), [merge-config](https://git-scm.com/docs/merge-config), [directory-rename-detection](https://git-scm.com/docs/directory-rename-detection), [remembering-renames](https://git-scm.com/docs/remembering-renames/2.33.0.html), [racy-git](https://git-scm.com/docs/racy-git), [git-config](https://git-scm.com/docs/git-config), [compat/mingw.c](https://github.com/git/git/blob/master/compat/mingw.c), [Newren batch 7](https://lore.kernel.org/git/CABPp-BE9dPYgTsrAKjjmPTfy-xY56ajg-1ZYPf7X97YR0T_n3Q@mail.gmail.com/T/), [Linus 2005](https://public-inbox.org/git/Pine.LNX.4.58.0504141102430.7211@ppc970.osdl.org/), [Begemann 2025](https://oleb.net/2025/git-file-renaming/), [JGit #110](https://github.com/eclipse-jgit/jgit/issues/110), [gix-diff Rewrites](https://docs.rs/gix-diff/latest/gix_diff/struct.Rewrites.html)
- Mercurial: [addremove](https://www.mercurial-scm.org/help/commands/addremove), [rename](https://www.mercurial-scm.org/help/commands/rename), [revlog notes](https://ngoldbaum.github.io/posts/revlog/), [D6936](https://www.mail-archive.com/mercurial-devel@mercurial-scm.org/msg47185.html)
- Jujutsu: [copy-tracking design](https://docs.jj-vcs.dev/latest/design/copy-tracking/), [changelog](https://docs.jj-vcs.dev/latest/changelog/), [#47](https://github.com/jj-vcs/jj/issues/47), [#6940](https://github.com/jj-vcs/jj/issues/6940), [Watson 2025](https://blog.eliaswatson.dev/posts/jj_rebase_rename/)
- Sapling: [copytracing](https://sapling-scm.com/docs/dev/internals/copytracing/), [git support modes](https://sapling-scm.com/docs/git/git_support_modes/)
- Perforce: [p4 move](https://help.perforce.com/helix-core/server-apps/cmdref/current/Content/CmdRef/p4_move.html), [p4 reconcile](https://help.perforce.com/helix-core/server-apps/cmdref/current/content/CmdRef/p4_reconcile.html), [forum on matchlines](https://forums.perforce.com/index.php?%2Ftopic%2F5393-reconcile-doesnt-recognize-file-renamesmoves=)
- Pijul: [theory](https://pijul.org/manual/theory.html), [two changes to changes](https://pijul.org/posts/2021-06-28-two-changes/)
- Windows: [BY_HANDLE_FILE_INFORMATION](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/ns-fileapi-by_handle_file_information), [Distributed Link Tracking](https://learn.microsoft.com/en-us/windows/win32/fileio/distributed-link-tracking-and-object-identifiers)
- Anchoring: [Hypothesis fuzzy anchoring](https://web.hypothes.is/blog/fuzzy-anchoring/), [W3C Web Annotation Data Model](https://www.w3.org/TR/annotation-model/), [CodeShovel](https://www.cs.ubc.ca/~rtholmes/papers/icse_2021_grund.pdf), [RefactoringMiner](https://github.com/tsantalis/RefactoringMiner)
- Fingerprints: [Broder 1997](https://dblp.org/rec/conf/sequences/Broder97.html), [Broder 2000](https://cs.brown.edu/courses/cs253/papers/nearduplicate.pdf), [Manku et al. 2007](https://research.google.com/pubs/archive/33026.pdf), [Winnowing 2003](https://theory.stanford.edu/~aiken/publications/papers/sigmod03.pdf), [FastCDC 2016](https://www.usenix.org/system/files/conference/atc16/atc16-paper-xia.pdf), [BLAKE3](https://github.com/BLAKE3-team/BLAKE3), [xxHash](https://github.com/Cyan4973/xxHash), [ignore WalkBuilder](https://docs.rs/ignore/latest/ignore/struct.WalkBuilder.html)
- moirai context: `docs/research/00-phase1-digest.md` (184/282 dead anchors; `observed_git_sha` staleness), `docs/research/design/30-synthesis.md` (`artifact` node, `stale` derivation, hooks).

## Appendix: probe artefacts

The probe scripts and raw outputs are not published. They were:

- Code: `fpprobe/src/{main.rs,sim2.rs}` (subcommands `walk`, `hash`, `sim`, `sim2`, `ram`, `times`), `anchors.py`, `missing.py`, `inexact.py`, `winnow.py`.
- Raw outputs: `walk.txt`, `hash.txt`, `sim.txt`, `sim2.txt`, `ram.txt`, `anchors_*.txt`, `missing_8d008b49.txt`, `inexact.txt`, `renames_M20.txt`, `renames_M50.txt`, `winnow.txt`.
- The build directory (`fpprobe/target`, 88 MB) and the scratch repo `fileid-test/` were deleted after the runs.
