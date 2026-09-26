# 11 — Links to locations inside files that survive edits and moves (R4, lens: in-file anchors)

*moirai research, 2026-09-26. Status: research only, nothing implemented. Lens: how a moirai node should point at a place **inside** a file (a line range, a function, a section of a plan) so that the reference survives edits, reformatting, code moving inside the file, and file moves, and so that moirai can say deterministically when the reference has become ambiguous or stale. File identity (which file a path now means after a move or a delete) belongs to the sibling lenses [10] and [12]; this report takes a resolved file as input and resolves the location inside it.*

Tags: **[M]** measured here (this machine, this session), **[D]** documented by the vendor, the maintainers, the spec or the paper's authors, **[C]** claimed by a third party, **[I]** my inference.

**Machine caveat.** During every timing below the machine (Ryzen 9 5900HS laptop, Windows 11) was at 97–100% total CPU load from other work: desktop applications, other agents' `rustc` and probes. Absolute times are upper bounds. Ratios between methods measured in the same run are reliable; single numbers are not.

**Read-only guarantee.** Every probe read the BoykoEngine repository through `git grep/blame/show/cat-file/diff/log/rev-list/ls-tree` with `GIT_OPTIONAL_LOCKS=0`, or through plain file reads. `git -L :func:` used a scratch attributes file passed with `-c core.attributesFile`. Nothing was written to any repository. Probe code and raw outputs are not published (§9 lists them); the build directories (1.2 GB) and large tag files were deleted.

---

## 0. Answer first

### 0.1 The recommended anchor model

An anchor is a small record: *file ref + up to four selectors + a verification stamp*. Agents never write selectors. They write `path:line[-line]`, `path::Type/fn`, or `path#Heading`, and moirai **captures** the rest from the file at write time. Anchors are **resolved lazily** (on read, on `moirai check`, from git or agent hooks), never by a daemon. Resolution is a **pure function** of the anchor, the file bytes and optionally the old blob, so its answer is deterministic.

| # | Selector | Content (typical size) | Role | Required? |
|---|---|---|---|---|
| S1 | **quote** (W3C TextQuoteSelector) | `exact` = the span's text, normalized (CRLF→LF, lines trimmed), ≤ 64–128 B, plus `prefix`/`suffix` of 32 B. A span longer than ~4 lines stores a **range**: start quote + end quote (W3C RangeSelector) | the identity of the span; found by exact search, then fuzzy search | yes, for every span anchor |
| S2 | **structural scope** | Rust: name path `impl ComponentPool/fn write_at_unchecked_initialized`. Markdown: heading path (the heading texts, with leading numbering kept separately). TOML: `[table].key`. HLSL/JS: top-level function name when a cheap scanner finds one, else none | narrows the search; readable rendering; coarse survival when the quote is destroyed | optional (per language) |
| S3 | **position hint** | `line_start..line_end` + `observed_blob` (git blob id of the file when captured) + `observed_commit` | fast path; diff-based mapping; display | yes |
| S4 | **marker** | an explicit ID written into the file (`<!-- moirai:a812 -->`, Obsidian-style `^a812`) | strongest; survives anything that keeps the marker | **opt-in**, only in moirai-owned prose (plans, specs) |
| — | `span_hash` | xxh3-64 of the normalized span | fresh vs edited in O(1) | yes |
| — | `pinned` flag | `{commit, path, lines}` that is never re-resolved (a GitHub-permalink-style historical reference) | citations of *what the code was* | explicit mode |

**Resolution order** (first success wins; each step runs only if the previous failed):

1. **File unchanged** since the last verification (stat, then content hash) → keep the cached result.
2. **Hint check**: the normalized text at `line_start..` still hashes to `span_hash` → `fresh`.
3. **Marker** (if any) → `fresh` if the quote matches, else `edited`.
4. **Exact quote**, first inside the S2 scope, then in the whole file:
   - 1 hit → `moved`;
   - several hits → context score (prefix/suffix) → if still tied, map the hint through a line diff from `observed_blob` → unique winner by margin → `moved`, else `ambiguous`.
5. **Line-diff mapping** from `observed_blob` to the current blob (in-process Histogram diff): the hint lands in an unchanged region → `moved`. It lands in a changed hunk → that hunk (± 1 KB) becomes the window for step 6.
6. **Fuzzy quote** (Myers bit-parallel, ≤ 25% errors), in the hunk or hint window, then inside the scope, then in the whole file. Score = 50·quote + 20·prefix + 20·suffix + 2·position (Hypothesis weights). Accept at quote similarity ≥ 0.75 with a top-2 margin ≥ 0.02 → `edited`; below the margin → `ambiguous`.
7. **Scope only**: the S2 symbol or heading still exists, the quote does not → `edited` (coarse).
8. **Cross-file** (`check --deep`, or when the file lens reports a split): exact quote over files changed since `observed_commit` → `moved` (other file).
9. Otherwise → `orphaned`. Keep the last-known excerpt and commit for display.

**States** (derived, cached, deterministic):

| State | Meaning | What moirai does |
|---|---|---|
| `fresh` | span text identical at the hint | nothing |
| `moved` | identical text, unique, at another place (or in a moved file) | update the cached hint silently; no version write |
| `edited` | best match is approximate, or only the scope survived | show the excerpt ("was: …"); referrers become **suspect**; cleared by `moirai anchor repin/ack` (a versioned write) |
| `ambiguous` | ≥ 2 candidates inside the margin | never auto-pick; list ≤ 3 candidates; `repin` |
| `orphaned` | nothing found | show "was: <excerpt> @ <commit>"; referrers suspect |
| `file_missing` / `file_ambiguous` | the file lens could not resolve the file | delegated to [10]/[12] |
| `pinned` | historical citation, never re-resolved | shown as `path@sha:L` |

These states map onto sibling [12]'s `{ok, relocated, fuzzy, orphaned}`:
- `ok` = `fresh`;
- `relocated` = `moved`;
- `fuzzy` = `edited`;
- `ambiguous` is added because the measurements show it is a real, frequent (0.5–8%) outcome that must not be folded into a guess.

### 0.2 Key numbers [M]

1. **The owner's corpus holds 34,195 `path:line` citations across its 526 tracked Markdown files** (23,665 → `.rs`, 5,432 → `.toml`, 4,549 → `.md`, 538 → HLSL). **38.1% name a bare basename** (e.g. `light.rs:1011-1015`, `runner.rs:213`). In a random sample of 1,500:
   - **15.9% already named an ambiguous basename in the commit that introduced them** (`lib.rs`, `mod.rs`, `Cargo.toml` …);
   - 4.0% named a path that did not exist in that commit.

   So one citation in five is unresolvable to a single file before any anchor question arises.
2. **Real citation survival** (1,180 citations that resolved to a file; state at introduction versus HEAD; *line valid* means the same line number still holds the same text):

   | Horizon since the citation was written | n | line number still valid | exact quote (+context) resolves | + fuzzy | orphaned/weak |
   |---|---|---|---|---|---|
   | ~90 commits (≈ 11 days) | 989 | 82.8% | 98.4% | 98.6% | 0.9% |
   | 200–500 commits | 48 | 39.6% | 81.2% | 87.5% | 10.4% |
   | 500+ commits (2.5–3 months) | 109 | 32.1% | 84.4% | 90.8% | 9.2% |
   | files that changed at all since citation | 375 | **27.2%** | **90.1%** | **93.3%** | 6.4% |

   - Quote anchors rescue **91% of the dead line anchors**, and line-diff mapping rescues 87%.
   - Where both give a line, the two agree in **99.7%** of cases.
   - A line-number anchor never reports failure. It silently points at other text; the owner's "184 of 282 anchors dead" [C, 01] is this.
3. **Capture-time uniqueness** (random non-trivial lines; Rust / Markdown / HLSL):

   | Anchor form | Rust | Markdown | HLSL |
   |---|---|---|---|
   | the line alone | 85.4% | 94.9% | 95.1% |
   | the line ±2 lines of context | 98.7% | 99.9% | 98.9% |
   | a 64-B quote + 32-B prefix/suffix | 98.2% | 99.8% | 98.9% |

   **12.3% of real single-line citations were ambiguous on the day they were written.** A quote must carry context, and capture must extend it until the quote is unique.
4. **Resolution cost**, per anchor in 350–450 KB files (synthetic edits, 300 anchors per case):
   - exact path: **10–40 µs** median;
   - fuzzy, windowed ±16 KB: **0.17–0.69 ms**;
   - fuzzy over the whole file: 1.4–7.3 ms;
   - orphan (a full scan that fails): 1.4–7.8 ms;
   - line diff (imara-diff Histogram): **0.3–0.45 ms** plus 0.8 ms interning, per file pair;
   - `similar` diff: 12–53 ms;
   - diff-match-patch Bitap (what Hypothesis originally used): **40–410 ms**, and it returned *no match* with a wrong hint.
5. **Whole-corpus verification is cheap:**
   - stat of all 2,247 source/doc files: 198 ms;
   - read + xxh3 of 62.7 MB: 0.54–1.2 s warm, 4.4 s cold.

   An unchanged file never needs re-resolution, and 66% of cited files were byte-identical after the citation was written.
6. **Parsers:**

   | Language | Parser | Files with parse errors (owner's corpus) | Speed (loaded machine) | Static library |
   |---|---|---|---|---|
   | Rust | tree-sitter-rust | **0.1%** of 1,573 | 3.8 MB/s | +1.2 MB (+0.6 MB runtime) |
   | HLSL | tree-sitter-hlsl | **23.9%** of 92 | — | +4.4 MB |
   | Markdown | tree-sitter-md | 1.0% | 0.6 MB/s: **722 ms** for one 5,900-line file | +0.96 MB |

   For Markdown, a fence-aware line scanner finds the same headings in **0.17 ms**. Universal Ctags indexes all Rust in 8.8 s (45,436 tags). HLSL can be tagged by mapping it to C++ (3,622 tags).
7. **Symbol granularity (Rust):**
   - a function spans a median of 10 lines (p90 43, p99 152);
   - a `kind + nested name path` is unique within its file for **26,377 of 26,409 items (99.88%)**;
   - of the 24 citations whose quote was lost, **17 still had their enclosing symbol, unique at HEAD**.
8. **SCIP symbols embed the package version.** `rust-analyzer cargo scipdemo 0.1.0 memory/component_pool/impl#[ComponentPool]write_at_unchecked_initialized().` Any version bump rewrites every symbol. rust-analyzer also needed 17 s on a 3-file crate. SCIP/LSIF monikers are a good *naming syntax* but a bad moirai key and a heavy dependency.
9. **Token cost** (≈3 chars/token for paths and code [I]):
   - today's `path:line`: ~18–21 tokens;
   - resolved rendering `component_pool.rs:1799-1805 ComponentPool::write_at_unchecked_initialized [moved]`: ~28–33;
   - with an `edited` excerpt: ~46–55;
   - a raw W3C JSON selector: 55–65, and a full anchor record: 140–165. The record must stay internal;
   - a moirai handle `@a812`: 2.

---

## 1. What "the anchor rotted" means

| Failure | Mechanism | Loud or silent | Seen in the owner's corpus |
|---|---|---|---|
| **wrong file** | bare basename or relative path matches several files | loud only if someone checks | 15.9% ambiguous basename + 4.0% no such path, *at authoring* [M] |
| **line shift** | insertions/deletions above the target | **silent**: the line number points at other text | 25% of all citations; 73% for files that changed [M] |
| **in-file move** | code moved within the file (reordering, extraction) | silent for line numbers; git diff sees delete+add | rare in the sample (1 case) [M] |
| **cross-file move / split** | function moved to another module; `foo.rs → foo/mod.rs + siblings` | loud (text gone) for content anchors | 6 of 16 inexact renames in history are module splits [M, 10] |
| **edit of the target** | the cited line itself changed | silent for line numbers; *fuzzy* for quotes | 2–6% of citations [M] |
| **deletion** | the code is gone | silent for line numbers; *orphan* for quotes | ~1–9% depending on horizon [M] |
| **duplicate text** | the cited text appears more than once | silent pick of the wrong one | 12.3% of single-line citations at authoring [M] |
| **stale but located** | the text is still there, but the claim about it is no longer true (the code around it changed) | always silent | out of an anchor's reach; see §4.5 |

The design goal is not "never lose an anchor", which is impossible. The goals are:
- **never silently point at the wrong place**;
- **say which of the states above applies**, deterministically.

---

## 2. Measurements on the owner's corpus

### 2.1 Citation census [M]

`git grep -o` over HEAD (`49f2fcfb`, 2026-09-22) for `path.(rs|toml|md|hlsl|hlsli|js|py):N[-M]` in tracked `*.md`:

| Metric | Value |
|---|---|
| citations | **34,195** across the 526 tracked Markdown files |
| by target type | rs 23,665 · toml 5,432 · md 4,549 · hlsl 435 · hlsli 103 · py 11 |
| distinct path strings | 2,233 |
| bare basename (no `/`) | **13,041 (38.1%)** |
| ranges (`:a-b`) | 7,013 (20.5%): 2–5 lines 2,565 · 6–20 lines 2,987 · 21–100 lines 1,274 · > 100 lines 186 |
| most-cited paths | `Cargo.toml` (1,591, ambiguous: 29+ files), `ALLOCATOR-DESIGN-SPACE.md` via an absolute `<workspace>/BoykoEngine/…` path (1,537), `crates/aether/Cargo.toml` (1,052) |

Absolute paths (`<workspace>/BoykoEngine/…`) and worktree paths (`<lanes-dir>/<lane>/…`) also appear. They bind a citation to one checkout, which is the R1 problem in miniature.

### 2.2 Survival study: method

Script `survival.py` (1,500 citations sampled uniformly with a fixed seed; 6 threads; ~5 min each mode):
1. **Authoring commit C.**
   - Mode `first`: the first commit whose diff added the citation string to that doc (`git log -S<cite> --reverse`).
   - Mode `blame`: `git blame` of the doc line.

   Both modes gave results within 1.5 pp of each other. The tables use `first`.
2. **Resolve the path at C.**
   - An exact path is used as is.
   - A unique suffix match is accepted.
   - A bare basename must be unique in the tree at C; otherwise the citation is counted as *unresolvable*.
3. **Target span** = the cited lines at C (capped at 10 lines), whitespace-trimmed per line.
4. **At HEAD**, the following strategies are evaluated:
   - **line**: the same line numbers hold the same text;
   - **quote**: exact block search. For several hits, disambiguate by 2 non-blank context lines before and after, requiring a margin; else `ambiguous`;
   - **fuzzy**: a difflib block similarity with context, Hypothesis-style weights, accepting a quote similarity ≥ 0.75;
   - **git-diff**: map the line through `git diff -U0 -M C HEAD` hunks;
   - **symbol**: the enclosing Rust item, Markdown heading, TOML table or HLSL function found by a regex scan still exists by name.
5. **Corroboration** (a sanity check that the citation was right at C): an identifier from the doc line (backticked, snake_case or CamelCase) appears within ±3 lines of the cited line at C.

**Biases.**
- 62% of sampled citations were introduced by a single checkpoint commit on 2026-09-11 (`ddaeed8e`, +15,175 doc lines), so the short horizon dominates the sample.
- Horizons longer than 200 commits have n = 157.
- Rename detection found no renamed cited files at HEAD. The file-move problem is measured by the sibling lens [10], not here.
- "Correct" cannot be known for sure. The agreement between the two independent methods (quote vs git-diff: 99.7%) is the proxy.

### 2.3 Survival study: results [M]

Population: 1,500 sampled citations. 298 (19.9%) had unresolvable paths at C (238 ambiguous basenames, 60 no such path), 14 cited a line beyond the end of the file at C, and 8 files were deleted by HEAD. That leaves **1,180 evaluated**. Of those:
- 31 (2.6%) cite only braces or blank lines;
- 193 (16.4%) are *not corroborated* by a doc identifier near the cited line (probably already wrong when written: an upper bound);
- 780 (66.1%) point into a file that is byte-identical at HEAD.

| Subset | n | line valid | quote exact (unique + context-resolved) | + fuzzy | ambiguous | orphan/weak | git-diff maps to same text | enclosing symbol unique at HEAD |
|---|---|---|---|---|---|---|---|---|
| all evaluated | 1,180 | 75.0% | 96.3% | 97.3% | 0.7% | 2.0% | 96.8% | 95.3% (3.9% multiple, 0.8% gone) |
| non-trivial, corroborated | 897 | 81.0% | 97.9% | 98.4% | 0.6% | 1.0% | 98.4% | 96.6% |
| non-trivial, **file changed since C** | 375 | **27.2%** | **90.1%** | **93.3%** | 0.3% | 6.4% | 90.1% | 92.1% (2.5% gone) |
| `.rs` non-trivial | 779 | 75.7% | 95.3% | 96.7% | 0.8% | 2.6% | 95.9% | 93.3% |
| `.md` non-trivial | 177 | 59.9% | 98.9% | 99.4% | 0 | 0.6% | 98.9% | 100% (heading) |
| `.toml` non-trivial | 166 | 96.4% | 99.4% | 99.4% | 0 | 0.6% | 99.4% | 99.4% (table) |
| HLSL non-trivial | 26 | 73.1% | 92.3% | 92.3% | 0 | 7.7% | 92.3% | 90.5% |

Other observations:
- **Dead line anchors rescued:** 91.2% by quote(+fuzzy) and 87.1% by git-diff mapping (n = 295).
- **Quote versus git-diff disagreements** (4 cases). Three are fuzzy matches on multi-line ranges. In one, `graph_bridge.rs:3628`, the quote occurs 8 times in the file:
  - the "nearest to the old line" tie-break (Hypothesis' position hint) picked line 3884;
  - the git line map picked line 4953, and there the text matches.

  **Nearest-to-hint is not a safe tie-break. History (a line diff from the old blob) is.**
- **Quote failures versus symbol survival:** of the 24 non-trivial quote orphans/weak matches, **17 still have their enclosing symbol, unique at HEAD** (7 symbols gone). A scope selector turns an orphan into a coarse `edited` anchor about 70% of the time.
- **Resolved quote lies inside the same-named symbol** in 98.9% of cases. The symbol scope is a correct search restriction.
- **Markdown citations rot fastest by line number** (59.9% valid): docs are edited heavily above the cited lines. Their quotes are the most unique (98.9%).
- **TOML citations barely move** (96.4% line-valid). A TOML table/key path is a nearly free structural selector.

### 2.4 Capture-time uniqueness [M]

About 4,700 random non-trivial lines per file type, taken from the working tree (lines that are only `}` or blank were skipped: ~22% of Rust lines). A quote is *unique* if it occurs once in its file. Char-level quotes start at the line's first non-space character.

| Anchor form | Rust | Markdown | HLSL |
|---|---|---|---|
| the line alone | 85.4% | 94.9% | 95.1% |
| line ±1 non-blank line | 96.8% | 99.7% | 98.4% |
| line ±2 non-blank lines | **98.7%** | **99.9%** | **98.9%** |
| 32-B quote | 87.6% | 94.1% | 94.4% |
| 32-B quote + 32-B prefix + 32-B suffix | 97.4% | 99.6% | 98.6% |
| 64-B quote | 94.5% | 98.8% | 97.7% |
| 64-B quote + 32-B prefix + 32-B suffix | **98.2%** | **99.8%** | **98.9%** |

What the table means:
- For about 1.3–1.8% of Rust lines even ±2 lines of context is not enough. This is real duplicate code: generated test cases, copy-pasted pipelines.
- For those lines, capture must add the **scope** (symbol), extend the quote, or as a last resort store an **occurrence index within the scope**. Serena uses the same device: an overload index `[i]` in its name paths [D].
- The text-fragments spec gives the same generation rule: "Use Context Only When Necessary" [D].

### 2.5 Markdown heading anchors [M]

`headings.py`: GitHub-style slugs (lowercase; keep letters, marks, digits, `_`, `-`; space → `-`; duplicates get `-1`, `-2` …) for every ATX heading outside code fences.

| Metric | Value |
|---|---|
| headings at HEAD | 11,576 in 526 files |
| duplicate slugs (got a `-N` suffix) | 113 |
| headings containing Cyrillic | 157 |
| **numbered headings** (`3.2 …`, `§…`, `A1. …`) | **3,719 (32%)** |
| mean heading / slug length | 44.6 / 41.3 chars |

Survival of old headings in files that are still at the same path:

| Old revision | Commits before HEAD | md files then → still at same path | slug still present | heading path (parent chain) present |
|---|---|---|---|---|
| 2026-09-11 | 91 | 463 → 463 | 99.66% (33 of 9,850 lost) | 97.79% |
| 2026-08-31 | 94 | 424 → 424 | 99.95% | 99.87% |
| 2026-08-13 | 229 | 376 → 376 | 99.99% | 99.99% |
| 2026-07-14 | 601 | 268 → 268 | 99.87% | 99.87% |
| 2026-06-14 | 1,078 | 140 → **40** | 98.18% | 97.01% |

What the table means:
- **Headings in the owner's docs are almost never renamed.** Plans grow by appending sections. Heading anchors are robust *inside* a file.
- The weak link is the **file**: 100 of the 140 Markdown files of mid-June are no longer at their path. That is the sibling lenses' job.
- Two slug hazards are real in this corpus, even though the history above did not trigger them often:
  - 32% of headings are numbered, so renumbering changes the slug;
  - duplicate-suffix slugs (`-1`) shift when an earlier duplicate is inserted.

  moirai should therefore key on the **heading text path** with the numbering stripped into a separate field, and treat the slug as a rendering only.

### 2.6 Symbol granularity and identity (Rust) [M]

`tree-sitter-rust` over all 1,573 `.rs` files. A name path is `mod/impl Type/fn name`, and `impl Trait for Type` is a scope of its own.

| Item | n | lines: median | p90 | p99 | max |
|---|---|---|---|---|---|
| `fn` | 18,793 | 10 | 43 | 152 | 4,503 |
| `impl` | 2,246 | 10 | 121 | 885 | 6,427 |
| `struct` | 3,783 | 4 | 16 | 68 | 1,726 |
| `mod` | 1,164 | 1 (`mod x;`) | 189 | 717 | 2,147 |
| `trait` | 93 | 11 | 132 | 1,099 | 1,099 |

- Of 26,409 items, only **32** have a `kind + name path` that is not unique within its file: `cfg`-gated duplicates and macro-generated twins.
- A symbol scope narrows a quote search to a median of 10 lines. It is not a location by itself: a p99 function spans 152 lines.
- Symbol names do **not** survive a rename of the symbol. Only the quote can, and only when the quoted line does not contain the name. Scope and quote are complementary; neither is enough alone.

**LSIF (the owner's `graphify-out/index.lsif`, rust-analyzer, June):**
- 525 documents, 522,411 elements, 8,963 monikers (7,333 export, 1,630 import);
- export identifiers look like `boyko_demo::ui::panel::PanelState::mode`; trait impls look like `stress::impl::DropTracker::Drop::drop`;
- mean identifier length 57 chars, maximum 131;
- 51 identifiers are duplicated (the same crate name in several bench/test targets).

The file itself is UTF-16 encoded (a PowerShell redirect), which is worth knowing for `tools/lsif_to_graphify.py`.

**SCIP (rust-analyzer 1.98.1, `rust-analyzer scip`, 3-file demo crate):**
- 17.3 s wall, dominated by loading the sysroot;
- symbols: `rust-analyzer cargo scipdemo 0.1.0 memory/component_pool/ComponentPool#`, `…/impl#[ComponentPool]write_at_unchecked_initialized().`, `…/impl#[ComponentPool][Store]put().`, `…/impl#[ComponentPool][Drop]drop().`;
- the package **version** is part of every symbol string.

### 2.7 Synthetic edit benchmark: states and correctness versus quote length [M]

`anchor-bench` (Rust, release, LTO). The files are real, read from the working tree:
- `compute.rs`: 6,128 lines, 344 KB;
- `OPEN-QUESTIONS.md`: 5,892 lines, 453 KB.

Each was mutated deterministically: 2% of lines deleted, 3% duplicated lines inserted, 5% of lines edited, one 60-line block moved. There are 300 anchors per quote length, each with 32-B prefix/suffix. The position hint is the old byte offset. *Correct* means the exact byte offset, judged only where the anchored line survived verbatim.

| File | Quote | moved (exact) | ambiguous | edited (fuzzy) | fuzzy-ambiguous | orphaned | correct / judged |
|---|---|---|---|---|---|---|---|
| compute.rs | 32 B | 267 (incl. 1 `fresh`) | **24** | 4 | 1 | 4 | 260/261 |
| compute.rs | 64 B | 264 | 10 | 23 | 2 | 1 | 261/270 |
| compute.rs | 128 B | 240 | 3 | 44 | 3 | 10 | 240/272 |
| compute.rs | 256 B | 207 | 0 | **75** | 8 | 10 | 209/277 |
| OPEN-QUESTIONS.md | 32 B | 291 | 2 | 3 | 1 | 3 | 281/282 |
| OPEN-QUESTIONS.md | 64 B | 284 | 1 | 5 | 2 | 8 | 278/279 |
| OPEN-QUESTIONS.md | 128 B | 252 | 0 | 36 | 4 | 8 | 254/275 |
| OPEN-QUESTIONS.md | 256 B | 229 | 0 | 50 | 3 | 18 | 230/271 |

- **Short quotes are ambiguous** (32 B: 8% in Rust).
- **Long quotes are fragile**: 256 B needs fuzzy matching 18–28% of the time (64 B: 2–8%), and fuzzy start offsets drift.
- The sweet spot is **~64 B exact + 32 B context**. Hypothesis chose 32-char context for the same reason [D, 2013 post].
- For long ranges, store two short quotes (start and end), not one long one.
- A Hypothesis-style error budget (`maxErrors = len/2`) turned 4 of the 32-B orphans into "matches". A looser budget buys recall at the price of silent wrong matches, so moirai should use ≤ 25%.

### 2.8 Cost of the primitives [M] (loaded machine; medians)

| Operation (one ~350–450 KB file) | Time |
|---|---|
| `memmem` all occurrences of a 32-B quote | 14–19 µs |
| exact-path resolution (`moved`), including context scoring | 10–26 µs (second run), 12–44 µs (first run) |
| ambiguous resolution (context scoring of all hits) | 30–305 µs |
| Myers bit-parallel full scan, 32/64-B pattern (`bio` 4.0.1) | 1.3–2.6 ms |
| Myers block ("long") full scan, 128 / 256 / 1,024 B | 2.9–5.6 / 4.7–11.7 / 11–18.5 ms |
| fuzzy resolution, whole file | 1.4–8.6 ms |
| **fuzzy resolution, ±16 KB window around the hint first** | **0.17–0.69 ms** |
| orphan (window fails, then whole file fails) | 1.4–7.8 ms |
| diff-match-patch `match_main` (Bitap, threshold 0.5, distance 1000), 32-char pattern, good hint | 40–171 ms |
| same, wrong hint (half a file away) | 38–410 ms; **returned `None`** on the Markdown file |
| line index (memchr) / xxh3 of every line / xxh3 of the file | 73–84 / 153–181 / 16–26 µs |
| **imara-diff 0.2**: intern + Histogram (Myers) | 0.8–0.9 ms + **0.37–0.44 (0.29–0.42) ms** |
| `similar` 3.2 line diff, Myers / Patience | 11.6–52.8 / 18.8–30.7 ms |
| line mapping through diff hunks | 276–278 of 300 mapped; **100% of mapped correct** (the unmapped ones sit on edited/deleted lines) |
| pulldown-cmark full parse + headings | 9.6–11.8 ms |
| fence-aware ATX line scan | 0.17 ms |

| Operation (whole corpus / tools) | Time |
|---|---|
| stat of 2,247 files (rs/md/toml/hlsl/js/py) | 198 ms |
| read + xxh3 of 62.7 MB (2,247 files) | 4.4 s first touch; 0.54–1.2 s warm |
| process spawn (`git --version`, `cmd /c exit`) from Python | median 139–177 ms (min 81–140) |
| `git log -S` (pickaxe) on one doc | 0.66 s |
| `git blame -L n,n` | 0.34 s |
| `git log -L a,b:file` | 0.38–1.4 s |
| `git log -L :fn:file` for a method in an `impl` | **fails** ("no match") without a `diff=rust` attribute. BoykoEngine has no `.gitattributes`, and git's default funcname only matches unindented lines. With a scratch attributes file: 0.44–0.52 s |
| Universal Ctags 6.2.1 (Rust, 1,573 files) | 8.8 s, 45,436 tags (function 12,778, field 8,948, constant 6,444, method 6,372, struct 3,782, impl 2,243) |
| Universal Ctags (Markdown, 526 files) / (HLSL as C++, 92 files) | 2.5 s, 11,632 tags / 0.6 s, 3,622 tags |

### 2.9 tree-sitter coverage and size [M]

tree-sitter 0.27.0. Grammars: rust 0.24.2, md 0.5.3, toml-ng 0.7.0, javascript 0.25.0, hlsl 0.2.0 (theHamsta).

| Language | Single file | Parse | RSS Δ | Corpus: files / lines | Corpus parse | Files with ERROR/MISSING |
|---|---|---|---|---|---|---|
| Rust | compute.rs, 6,127 lines | 89 ms (3.8 MB/s); tags query 47 ms → 250 definitions | +4.4 MB | 1,573 / 684,237 (33 MB) | 45.6 s | **2 (0.1%)** |
| Markdown (block grammar) | OPEN-QUESTIONS.md | **722 ms** (0.6 MB/s) | +23 MB | 526 / 246,529 (25.7 MB) | 39 s | 5 (1.0%) |
| TOML | PINS.toml, 1,273 lines | 12 ms | +0.5 MB | 41 / 4,774 | 0.05 s | 0 |
| HLSL | sdf_gbuffer_composite.hlsl, 1,903 lines | 57 ms | +2 MB | 92 / 23,768 | 1.1 s | **22 (23.9%)**, 122 error nodes |
| JavaScript | vis-network.min.js (minified, 2.2 MB) | 993 ms; tags → 1,700 definitions | +30 MB | 6 / 168 | 1.65 s | 0 |

Binary size:

| Build | Size |
|---|---|
| bench binary without tree-sitter | 1.05–1.10 MB |
| with all five grammars | **8.91 MB** |

Static libraries: hlsl 4.39 MB · rust 1.21 MB · md 0.96 MB · runtime 0.61 MB · javascript 0.50 MB · toml 0.047 MB. The generated `parser.c` sources are hlsl 20.8 MB, rust 6.5 MB and javascript 2.9 MB.

---

## 3. Prior art, family by family

### 3.1 W3C Web Annotation selectors (Recommendation, 2017) [D]

The model defines nine selector types: Fragment, CSS, XPath, TextQuote, TextPosition, DataPosition, SVG, Range, plus alternatives (several selectors on one target).

- **TextQuoteSelector**: `exact` (required), `prefix`, `suffix`.
  - Selection is in "unicode code points … not in terms of code units".
  - If several matches remain after prefix/exact/suffix, "the selection SHOULD be treated as matching all of the matches". The spec does not pick one; moirai must decide, and the only honest decision is `ambiguous`.
- **TextPositionSelector**: `start`/`end` in code points after the same normalization. The spec itself warns it is "very brittle", and recommends adding a State.
- **RangeSelector**: `startSelector` + `endSelector` of the same class. This is the right shape for long spans (two short quotes).
- **`refinedBy`**: a selector can be refined by another. This maps exactly onto *symbol scope → quote within it*.
- **Several selectors** on one target: they "SHOULD select the same content", and consumers "MUST pick one" if they differ.
- **States** (TimeState with `sourceDate` and `cached`; HttpRequestState): the analogue for code is *the git blob and commit where the anchor was captured*.

*Fit for moirai:*
- Borrow the vocabulary (quote, position, range, refinedBy, state) for the R3 git image, so that anchors are self-describing.
- Do not borrow the positions' unit. moirai works in UTF-8 bytes and lines; LSP defaults to UTF-16; SCIP chooses per document; Kythe uses bytes. The Cyrillic docs make this matter, so convert only at export.
- Add a `SymbolSelector` as a moirai extension. The spec allows other selector types [I].

### 3.2 Hypothesis: the reference implementation of quote anchoring

- **2013** ("Fuzzy Anchoring", csillag) [D]:
  - stores RangeSelector (XPath + offsets), TextPositionSelector, and a TextQuoteSelector with a 32-character prefix/suffix;
  - re-attaches in order: range → position → context-first fuzzy (find the prefix near the expected start, the suffix near the expected end, compare the text between) → quote-only fuzzy;
  - uses a modified google-diff-match-patch (Bitap for matching, Myers for comparison).
- **Current client** (`src/annotator/anchoring/html.ts`, `match-quote.ts`) [D]:
  - order: RangeSelector → TextPositionSelector → TextQuoteSelector;
  - range and position results are **validated** against `quote.exact` ("quote mismatch" otherwise);
  - the position start is passed as a `hint` to the quote matcher;
  - `matchQuote` first collects **all exact matches** (`indexOf` loop). Only if there are none does it run `approx-string-match` (Myers 1999 bit-parallel, O((k/w)·n)) with `maxErrors = min(256, quote.length/2)`;
  - candidates are scored with weights **quote 50, prefix 20, suffix 20, position 2**, normalized by 92;
  - when nothing anchors, the annotation is an **orphan** and is shown in an Orphans tab.
- **Measured orphan rates on the live web** (Aturban, Nelson, Weigle, arXiv 1512.06195) [D]:
  - of 20,953 highlighted-text annotations, "about 22% … can no longer be attached";
  - "53% are in danger of becoming orphans if the live Web page changes";
  - archives re-attach only ~12% of the orphans.

*Lessons that transfer* [I]:
1. Validate every fast path (position, marker, symbol) against the quote.
2. Exact before fuzzy, and fuzzy only as a fallback.
3. The Myers bit-parallel algorithm, not Bitap. Measured: dmp's Bitap is 100–1,000× slower and depends on the hint (§2.8).
4. An explicit orphan state.

*Lessons that do not transfer*:
- The position weight is only 2/92, and ties go to the best total score. On code with duplicates that silently picks one (§2.3, `graph_bridge.rs`). moirai needs an `ambiguous` state with a margin.
- `len/2` errors is too loose for code.

### 3.3 URL text fragments (WICG, shipped in all engines) [D]

- Syntax: `#:~:text=[prefix-,]textStart[,textEnd][,-suffix]`.
- Matching: exact, word-boundary-aware.
- The first match wins: "the first instance of this exact text string is the target".
- Browser support: Chrome 80+, Safari 16.1+, Firefox 131 (October 2024) [C].
- Generation guidance: "Prefer Exact Matching To Range-based", "Use Context Only When Necessary", and an element-ID fallback.

*Fit* [I]:
- It is proof that a quote anchor can be a **compact, human-readable string**. `crates/x.rs#:~:text=copy_nonoverlapping(src,-self.len` is a possible export syntax.
- First-match semantics is exactly the silent-wrong behaviour moirai must avoid. Use the syntax, not the semantics.

### 3.4 Approximate matching engines

| Engine | Algorithm | Pattern limit | Measured here | Verdict |
|---|---|---|---|---|
| google diff-match-patch `match_main` (Rust port `diff-match-patch-rs` 0.5.1) | Bitap with `Match_Threshold` 0.5, `Match_Distance` 1000, `Match_MaxBits` 32 | 32 chars | 40–410 ms per query; `None` with a wrong hint | no |
| `approx-string-match` (Hypothesis, JS) | Myers 1999 bit-parallel, block-based for long patterns | none | — | the model to copy |
| `bio::pattern_matching::myers` (Rust, 4.0.1) | Myers bit-parallel (`u64`/`u128`, `long` blocks); similar to Edlib | none | 1.3–2.6 ms full scan (64 B); 0.17–0.69 ms windowed | **yes** (or a ~100-line hand implementation, to avoid `bio`'s dependency tree) |
| `sassy` 0.2.6 (2025, SIMD approximate search) | SIMD | — | not tested | candidate if fuzzy cost ever matters |

### 3.5 Symbol anchors

| Source | What it gives | Robustness | Cost/coverage | Fit |
|---|---|---|---|---|
| **ctags / Universal Ctags 6.2.1** [D][M] | `name<TAB>file<TAB>address`, where the **address may be a line number or a search pattern `/^…$/`**. The pattern is the original content anchor: vi-era tags already knew line numbers rot. Extension fields `line:`, `implementation:`, `signature:` | pattern survives shifts; fails on edits of the defining line | external binary; Rust/Markdown/TOML (TOML disabled by default); no HLSL (C++ mapping works); 8.8 s for all Rust | good idea (a pattern address), wrong packaging (external tool) |
| **tree-sitter tags** (`queries/tags.scm`: `@definition.function`, `@name`, …) [D] | definitions/references with row/col ranges | as good as the grammar | Rust 0.1% error files; HLSL 23.9%; +1.2 MB (Rust) +0.6 MB runtime; 47 ms tags query for 6k lines | Rust: yes. HLSL: no. Markdown: overkill |
| **LSP** `textDocument/documentSymbol` (name, kind, range, selectionRange, children), `workspace/symbol`, `textDocument/moniker` (scheme, identifier, unique level, kind) [D] | hierarchical symbols; positions in UTF-16 by default (3.17 negotiates utf-8/16/32) | exact | needs a running server (rust-analyzer: minutes and GBs on a big workspace [I]) | capture-time helper only (Claude Code has an LSP tool), never a moirai dependency |
| **rust-analyzer LSIF/SCIP** [D][M] | monikers `crate::path::Type::method` (LSIF); SCIP `<scheme> <manager> <package> <version> <descriptors>`, with descriptor suffixes `/` namespace, `#` type, `.` term, `().` method, `[..]` type parameter, `!` macro | stable across edits; changes on rename/move of the module; **SCIP changes on every version bump** | 17 s for a toy crate; the owner already produces LSIF for graphify | naming syntax to render; optional import; never a key |
| **SCIP governance** [D] | became independent of Sourcegraph (announced 2026-03-25; steering committee from Meta, Uber, Sourcegraph; SEP process) | — | — | a stable spec to borrow from |
| **Kythe** [D] | VName (signature, corpus, root, path, language); anchor nodes with `loc/start`/`loc/end` **byte offsets** and `defines/binding`/`ref` edges | signature "opaque", per analyzer | whole-build indexing | the graph shape is instructive (an anchor is a node), the stack is not |
| **Serena** (MCP, LSP-backed) [D] | agent-facing `name_path` (`MyClass/my_method`, absolute `/MyClass/my_method`, overload index `[i]`); tools `find_symbol`, `replace_symbol_body`, `get_symbols_overview` | LSP-exact | a language server per language | **the authoring syntax agents already use**; copy the `a/b[i]` shape |
| **rustdoc intra-doc links** [D] | `[path::to::Item]`, disambiguators `struct@`, `fn@`, `macro!`; resolved in the scope of the defining module; `rustdoc::broken_intra_doc_links` warns | compiler-checked | only inside Rust docs | proof that path-style symbol references plus a *checker* stay alive in practice |
| **Aider repo map** [D] | tree-sitter tags + graph ranking; default budget 1k tokens (`--map-tokens`) | — | — | symbols are how agents are shown code cheaply |
| **GitHub stack-graphs** [D] | name resolution from tree-sitter | — | **archived 2025-09-09** | a warning about building on ambitious parser-based resolution |

### 3.6 Markdown and prose anchors

- **GitHub heading anchors** [D]: lowercase, "Spaces are replaced by hyphens", other punctuation removed, markup removed, duplicates get "a hyphen and an auto-incrementing integer". Custom anchors `<a name="…">` are allowed and are "not considered by the automatic naming".
- **Obsidian** [D]:
  - block IDs `^block-id` (Latin letters, digits, dashes) appended to a paragraph; linked as `[[note#^id]]`;
  - heading links `[[note#Heading#Sub]]`;
  - "Automatically update internal links" on file rename;
  - block references are "not part of the standard Markdown format".
- **Emacs Org-mode file links** [D] support `::255` (line), `::My Target`, `::*Heading`, `::#custom-id` and `::/regexp/`. That is five selector kinds in one compact syntax, a good precedent for moirai's authoring syntax.
- **Sibling [12] §4.11**: in-file IDs in *code* are disruptive (Godot, Unity, Logseq evidence), and even in prose they must be opt-in.

### 3.7 History-based line tracking (git and research)

- **git** [D][M]:
  - `git log -L<start>,<end>:<file>` and `-L:<funcname>:<file>`. Funcname uses the diff driver's hunk-header regex (a built-in `rust` and `markdown` driver exist but need `.gitattributes`). `-L` does not follow renames and takes no pathspec.
  - `git blame` (and `--reverse`) answer "who last touched this line" or "until when did it exist", not "where is it now".
  - Every call is a process spawn: 0.14–0.18 s on this machine, loaded. Git is a data source, never a hot-path resolver.
- **Reiss, "Tracking source locations"** (ICSE 2008) [D, via LHDiff]: compared many approaches and found that *simple text techniques beat AST-based ones*. It recommended W_BESTI_LINE, which combines content and context similarity.
- **LHDiff** (Asaduzzaman, Roy, Schneider, Di Penta; ICSM 2013) [D]:
  - algorithm: Unix diff for unchanged lines, then candidates by simhash of content and of a context of 4 lines before and after, then normalized Levenshtein, combined as **0.6·content + 0.4·context** (the same weights as Reiss), threshold 0.45, top k = 15;
  - correct mappings on the three benchmarks (Reiss / Eclipse / NetBeans):

    | Technique | Reiss | Eclipse | NetBeans |
    |---|---|---|---|
    | LHDiff | 97.0% | 82.8% | 85.5% |
    | W_BESTI_LINE | 96.7% | 52.6% | 61.9% |
    | Unix diff | 92.5% | 41.0% | 48.4% |
    | git | 92.5% | 45.3% | 53.7% |
    | SDiff (AST-based, Java only) | 86–87% | 70–74% | 71–75% |

  - *Text + context beats both plain diff and syntax-aware tracking.*
- **CodeShovel** (ICSE 2021) [D]: method-level histories across refactorings, "complete and accurate change histories for ~90% of methods". It matches extracted methods by body similarity (≥ 95% for methods under 20 chars, ≥ 82% for longer ones).
- **CodeTracker** (Jodavi & Tsantalis, FSE 2022; block tracking 2024) [D]: refactoring-aware tracking, 99.9% precision/recall for methods, 99.7/99.8% for variables, 99.5% for blocks in ~3.6 s on average. It is Java-only (RefactoringMiner).
- **CodeMapper** (Hu & Pradel, arXiv 2511.05205, Nov 2025) [D]: language-agnostic. Candidate generation from diffs, move detection and fragment search, then similarity-based selection. 71.0–94.5% correct on four datasets in ten languages, "1.5–58.8 absolute percent points" above baselines.

*Lesson* [I]:
- The research frontier confirms the cascade: diff for unchanged regions, similarity with context for changed ones, move detection across files.
- Nothing language-specific is needed to reach ~95% on real histories, and my 90–98% on the owner's citations agrees.
- Symbol-level trackers reach 99%+ but only for one language, with heavy machinery.

### 3.8 Code-anchored products

| Product | Anchor | Relocation | Stale signal |
|---|---|---|---|
| **GitHub permalinks** [D] | commit SHA + path + `#L10-L20` (press `y`) | none, pinned forever; branch URLs "might not be the same when someone looks at it later" | none (a permalink is `pinned`, never stale) |
| **GitHub PR review comments** [D/C] | `line`/`side` + `original_line` + `commit_id` | mapped through the PR diff | "**Outdated**" when the line changed |
| **Gerrit ported comments** (3.4) [D] | per patch-set line/range/file/patch-set comments | ported to the requested revision via the patch-set diff | unresolved comments stay visible |
| **CodeTour** (Microsoft) [D] | `file` + `line`, **or `pattern` (regex)** "to associate steps with line content as opposed to ordinal"; tours can be bound to a git ref/commit | pattern re-search | "tour drift" checks in CI (CodeTour Watch) |
| **Swimm auto-sync** [C, vendor blog] | snippets, "smart tokens", "smart paths" | uses full git history ("line markers, line numbers, token references, size of the change, history of the file"); no shallow clones | auto-sync when confident, otherwise the doc is flagged for human re-selection |
| **CodeStream codemarks** [C] | "permanently connected to the lines of code" | "automatically repositioned as your code changes, even across branches" | undocumented |
| **VS Code Comments API** [I] | `CommentThread.range`, owned by the extension | the extension must persist and re-anchor | — |

*Common pattern*: a content pattern or diff mapping, plus an explicit **outdated / drift / needs review** state, plus pinned references for history. Nobody trusts a bare line number.

---

## 4. The recommended anchor model in detail

### 4.1 Data shape

```text
Anchor                                   // a sub-record of a reference edge, or a small 'anchor' node (see 4.7)
  file        : NodeRef(file)            // file identity from the file lens [10]/[12]; never a bare basename
  mode        : Live | Pinned            // Pinned = historical citation (commit + lines), never re-resolved
  quote       : Quote | Range(Quote, Quote)
     Quote    = { exact: bytes ≤ 128 (normalized: LF, per-line trim), prefix: ≤ 32 B, suffix: ≤ 32 B }
  scope       : Option<Scope>            // Rust  : "impl ComponentPool/fn write_at_unchecked_initialized"
                                         // md    : ["Design", "Storage"] + numbering ["3", "3.2"] kept apart
                                         // toml  : "[workspace.dependencies].memchr"
  occurrence  : Option<u16>              // only when quote+context+scope is still non-unique at capture (~1-2%)
  hint        : { line_start, line_end, observed_blob: [u8;20], observed_commit: Option<[u8;20]> }
  span_hash   : u64                      // xxh3 of the normalized span
  marker      : Option<Sym>              // opt-in, prose only
  captured    : { moirai_commit, git_head, resolver_version }
// derived, NOT versioned: cache keyed by (anchor id, file content hash)
Resolution    = { state, lines, byte_range, score, candidates[≤3], verified_at }
```

Size, using this corpus's measured means: path ref 4 B, scope ~40–60 B, exact ~40–70 B (a median single line), prefix/suffix 64 B, hint + hashes ~60 B. That is **~200–260 B per anchor**; for the owner's 34k citations, **~8 MB on disk and 0 B resident at idle** (mapped segments). Sibling [10] arrived at ~150 B with a 120-char excerpt, and the two designs agree in shape.

### 4.2 Capture (write time) — where authoring cost is decided

Agents keep writing what they already write. moirai parses these forms:
- `path:L` or `path:L-M`;
- `path::Type/fn` (Serena-style name path) or `path::fn`;
- `path#Heading` or `path#Parent/Heading`;
- `path@<sha>:L` (pinned);
- `@a812` (an existing anchor).

Capture then runs these steps:
1. **Resolve the file** through the file lens. A bare basename that matches several files is an **error at write time** ("did you mean …"). This alone removes the 20% of the corpus that is unresolvable today.
2. **Read the span** from the caller's bound worktree (R1 lease) and record `observed_blob`. For a clean tracked file this is the git blob id, and sibling [10] notes that EOL-normalized hashing makes it match git. Record the commit when there is one.
3. **Normalize**: CRLF→LF, trim each line, collapse nothing else. Sibling [10] measured that 72% of BoykoEngine working-tree files are CRLF.
4. **Choose the quote**:
   - span ≤ 4 lines and ≤ 128 B → one quote;
   - otherwise → **range**: a start quote (first non-trivial line) and an end quote (last non-trivial line).

   Skip lines that are only braces or blank (2.6% of citations point at such lines).
5. **Make it unique**: widen the prefix/suffix up to 64 B, then add the scope, then an occurrence index. The measured coverage: ±2-line context 98.7%, 64 B + 32 B context 98.2%, scope brings name paths to 99.88% unique.
6. **Scope**, per language (§4.5).
7. **Hash** the span; stamp `captured`.

Capture cost: one file read plus a line index (~0.1 ms). A Rust scope adds a tree-sitter parse, ~90 ms for a 6k-line file on the loaded machine, only when a scope is wanted. A Markdown or TOML scope costs ~0.2 ms (line scan).

### 4.3 Resolution — order, cost, determinism

This is the order of §0.1 with the measured cost of each step:

| Step | Test | Result | Cost [M] |
|---|---|---|---|
| 0 | file stat (and hash if the stat changed) equals the last verified | cached result | ~25 µs–0.1 ms per file (stat); read + hash ~0.2–2 ms per file |
| 1 | normalized text at hint lines hashes to `span_hash` | `fresh` | line index 80 µs per file, amortized over all anchors in the file |
| 2 | marker present | `fresh`/`edited` | memmem, ~15 µs |
| 3 | exact quote in scope, then in the file | `moved`, or `ambiguous` if the context margin < 0.1 and no diff tie-break | 10–40 µs; 30–300 µs if ambiguous |
| 4 | line diff `observed_blob → current` (imara-diff Histogram, in process) | `moved` if the hint lies in an Equal run; else a hunk window for step 5 | ~1.2 ms per file pair, amortized; needs the old blob (git object or moirai snapshot) |
| 5 | fuzzy quote (Myers, k = ⌊0.25·\|exact\|⌋) in the hunk/hint window, then the scope, then the file; score 50/20/20/2 | `edited` if q ≥ 0.75 and margin ≥ 0.02, else `ambiguous` | 0.17–0.7 ms windowed; ≤ 8 ms whole file |
| 6 | scope still resolves uniquely | `edited` (coarse; the span is unknown) | 0 extra when the scope was resolved in step 3 |
| 7 | (`--deep`) exact quote across files changed since `observed_commit` | `moved` (other file) | memmem ~20+ GB/s over in-memory text; I/O-bound, ~0.5 s warm for the whole repo |
| 8 | — | `orphaned` | — |

**Determinism rules** [I]:
- `resolve(anchor, file_bytes, old_blob?) → (state, location, candidates, evidence)` depends on nothing else. There is no clock, no randomness and no model call.
- The digest records that LLMs detect stale or retracted memory at best 55% of the time [00: STALE, arXiv 2605.06527]. Staleness must be computed.
- Thresholds (the context margin 0.1, the fuzzy margin 0.02, q ≥ 0.75, k ≤ 25%, the window 16 KB) are **constants of `resolver_version`**. They are stored with each result, so a re-run with the same version gives bit-identical output, and a version bump is a visible event.
- Ties are broken by (score desc, byte offset asc) **only for ordering the candidate list**, never to pick a winner. A tie is `ambiguous`.
- The measured failure modes justify each guard:
  - nearest-to-hint picked the wrong duplicate (§2.3);
  - `len/2` errors "found" matches for orphans (§2.7).

**Cost for the whole owner corpus** [I, from §2.8]:
- 34k anchors in ~2.2k files: a `check` is ~0.2 s of stat;
- plus reading and hashing the changed files (34% of evaluated citations pointed into a file that changed over the study window): ~0.3–1 s;
- plus ~30 µs × anchors in changed files: ~0.4 s;
- plus fuzzy work for the ~3–7% that need it: ~1–2 s worst case.

That is **well under 5 s cold for everything, and milliseconds for the handful of anchors a context pack renders**.

### 4.4 States, transitions and what gets versioned

```text
            capture
               │
               ▼
   ┌────────► fresh ◄──────── repin/ack (versioned write: new quote, new observed_blob)
   │           │ file changed
   │           ▼
   │   resolve ──► moved     (cache only; hint refreshed in cache; no write)
   │           ├─► edited    (referrers suspect; shows "was: …")
   │           ├─► ambiguous (candidates; needs repin)
   │           └─► orphaned  (last excerpt + observed_commit; needs repin or drop)
   └────────── file lens: file_missing / file_ambiguous (anchor waits; no in-file resolution)
 pinned: never resolved; always rendered as path@sha:L
```

**Versioned data versus cache.**
- The anchor's selectors and captured stamp are versioned fields: they merge per branch (R1) and export to the git image (R3).
- **Resolutions are a cache keyed by file content hash.** They are not versioned and not merged, and they are recomputed in any worktree.

This matters for R1. The same anchor can be `fresh` on trunk and `moved` on a lane, and storing that would create merge conflicts for no information. Only human or agent decisions (`repin`, `ack`, `drop`) are commits. This follows the synthesis rule that `stale` is computed on demand and cached as lazy facts [30 §2.x].

**Effect on referrers.** `edited` and `orphaned` make the referring node **suspect** through the same derived-staleness path the synthesis defines for `observed_git_sha`/`applies_to` [30]. A finding that cites an edited line shows it in the brief. With the owner's consent (open question 3), a merge gate refuses unacknowledged suspects.

### 4.5 Language coverage and parser dependencies

| Language (owner's corpus) | Scope selector | Implementation | Binary cost | Why |
|---|---|---|---|---|
| **Rust** (1,573 files) | `mod/impl T/fn f` name path, plus `impl Trait for T` | tree-sitter-rust behind a cargo feature (default on) | +1.2 MB grammar, +0.6 MB runtime | 0.1% error files; 99.88% unique name paths; turns 17 of 24 quote orphans into coarse survivals; readable rendering |
| **Markdown** (526) | heading text path, with numbering stripped into its own field | a hand-written fence-aware ATX/setext scanner | ~0 | 0.17 ms vs 722 ms (tree-sitter-md) vs 10 ms (pulldown-cmark); heading texts survive 98–99.9% |
| **TOML** (41) | `[table]` + `key` path | a line scanner (tables and keys) | ~0 | 96.4% of TOML citations never even moved; tree-sitter-toml is tiny (47 KB) if a scanner proves insufficient |
| **HLSL** (92) | top-level function/struct name via regex (display only) | none | 0 | tree-sitter-hlsl: 23.9% files with ERROR nodes and +4.4 MB. Quotes are unique 98.9% with ±2 lines, so quote-only is fine |
| **JavaScript** (workflow scripts; 6 files in the repo) | none (quote only) | none | 0 | tiny files; tree-sitter-javascript (+0.5 MB) is not worth it |
| other text (ps1, py, yaml, json) | none | none | 0 | quote + hint works on any text |
| binary (png, spv, …) | none — whole-file anchors only | — | — | sibling lenses |

Rules [I]:
- A scope never replaces the quote. It narrows the search and names the anchor.
- If the scope fails to resolve (the symbol was renamed), search the whole file. That is exactly the case where the quote survives and the name does not.
- LSP, rust-analyzer, SCIP and LSIF are **optional capture-time helpers** (Claude Code's LSP tool can supply a `documentSymbol` range), never runtime dependencies. They are too heavy and too unstable as keys, because of SCIP's version string.

### 4.6 Rendering to agents and token cost

| Rendering | chars | ≈ tokens [I, ~2.8–3.3 chars/token] | When |
|---|---|---|---|
| `@a812` | 5 | 2 | inside moirai text fields (bodies, findings) |
| `component_pool.rs:1799-1805 ComponentPool::write_at_unchecked_initialized` | 73 | 22–26 | default (`fresh`) |
| … `[moved from :1436]` | 92 | 28–33 | `moved` (only while it adds information) |
| … `[EDITED 0.82; was: "ptr::copy_nonoverlapping(src, dst.add(row * size), size);"]` | 153 | 46–55 | `edited`/`orphaned` |
| today's `crates/…/component_pool.rs:1436-1442` | 59 | 18–21 | — |
| W3C quote selector JSON | 183 | 55–65 | never shown to agents |
| full anchor JSON | 464 | 141–166 | export (R3) only |

Rules [I]:
- Abbreviate a path to its basename **only when the basename is unique in the rendered view**. Otherwise render the shortest unique suffix.
- Show the quote only for non-fresh states.
- Show the scope because it is the most informative 30 chars for an agent. This is the same reason Aider's repo map and Serena show symbols.

### 4.7 Interaction with the rest of moirai

- **File identity** ([10], [12]): the anchor holds a `NodeRef(file)`, so a file move rebinds every anchor without touching them. After a *split*, each anchor re-resolves individually (step 7 across the pieces), as [10] §5 recommends.
- **Edge or node?** Store anchors as **small `anchor` records owned by the reference edge**. Give them a stable short id (`@a812`) so agents can cite them cheaply and `moirai anchor repin @a812` has a handle. Keep a reverse index file → anchors. That gives "which knowledge depends on this file/function" as an O(1) query (the R4 "every referrer knows" requirement) [I].
- **R1 branches**: selectors are versioned fields. Two lanes that repin the same anchor differently are a field conflict. Resolve it deterministically: re-resolve both candidates on the merged tree and keep the one that is `fresh`. If both or neither is fresh, surface the conflict [I].
- **R2 (no git)**:
  - quote + scope + hint resolve **96–97%** without any history;
  - the diff step needs the old blob. With git, read it in process (gix / moirai's pack reader; never spawn git: 0.14–0.18 s per call here). Without git, only if moirai kept a snapshot (open question 1).
- **R3 (git image)**: export anchors in W3C vocabulary: `TextQuoteSelector`, `RangeSelector`, `refinedBy` for scope → quote, and a TimeState-like `{blob, commit}` stamp, with a `moirai:SymbolSelector` extension. Positions as lines, plus UTF-8 byte offsets, labelled as such.
- **Hooks (near-synchronous without a daemon)**:
  - A PostToolUse hook on `Edit`/`Write` receives the tool's arguments (`tool_input`) [D]. For `Edit` those are the file path and the exact `old_string`/`new_string` [I from the tool schema]. moirai can therefore re-resolve anchors in that one file right after an agent edit, with an exact mapping instead of a search.
  - Git `post-checkout`/`post-merge` hooks can mark anchors in changed files for re-resolution.
  - Both cost a process spawn (hooks: +15–73 ms [30]; ~0.1–0.2 s measured here under load) and **zero idle CPU**.
- **The owner's merge rule UG-10** (a cited number is re-derived by its content, never by its line position [02]) is manual quote re-anchoring. moirai automates it and reports `ambiguous`/`orphaned` instead of leaving them to a human pass.

---

## 5. What not to do (measured or documented anti-patterns)

| Anti-pattern | Evidence |
|---|---|
| **Line numbers as identity** | 72.8% dead after the cited file changed; they never fail loudly [M]; "184 of 282 dead" [C, 01]; 72% silently wrong after 4 months [M, 10] |
| **Bare basenames** | 38.1% of citations; 15.9% ambiguous on the day they were written [M] |
| **"Nearest to the old position" as a tie-break** | picked the wrong one of 8 duplicates, where the diff mapping was right (§2.3) [M]; Hypothesis weights position at 2/92 for this reason [D] |
| **diff-match-patch Bitap** | 32-char limit; 40–410 ms; returns nothing with a wrong hint [M] |
| **Long exact quotes** | 256 B: 18–28% need fuzzy matching under light edits, vs 2–8% at 64 B [M] |
| **Short quotes without context** | 32 B: 8% ambiguous in Rust; single lines: 12.3% ambiguous at authoring [M] |
| **Loose error budgets** (`len/2`) | converts orphans into silent matches [M] |
| **First-match semantics** (text fragments, W3C "treat as all matches") | silently wrong on duplicates [D] |
| **SCIP/LSIF strings as keys** | version embedded in every SCIP symbol; 17 s rust-analyzer start even on a toy crate; 51 duplicate LSIF identifiers [M] |
| **tree-sitter for Markdown headings** | 722 ms/file vs 0.17 ms line scan [M] |
| **tree-sitter-hlsl** | 23.9% of the owner's shader files parse with ERROR nodes; +4.4 MB [M] |
| **Spawning git on the resolution path** | 0.14–0.18 s per spawn on Windows here; `-L :fn:` needs `.gitattributes` the repo lacks [M] |
| **A background re-anchoring watcher** | violates the ~zero idle CPU requirement; lazy + hooks cover the need [I] |
| **In-file IDs in code** | [12] §4.11 (clutter, duplication on copy, lost on rewrite) [D/C] |
| **Asking an LLM whether an anchor is stale** | best model 55% [00, STALE] |
| **Persisting resolution results as versioned data** | creates per-lane conflicts with no information; resolution depends only on file bytes [I] |

---

## 6. Open questions (owner value and scope calls)

1. **Old-blob retention without git (R2).**
   - Should moirai keep compressed snapshots of *referenced* files so the diff tie-break and the "was:" display also work outside git or in shallow clones? My estimate is ~10–15 MB zstd for one snapshot generation of the 2.2k cited files [I].
   - Or is quote + scope (96–97% resolution) enough without git?
2. **Opt-in markers in prose.** May moirai write `<!-- moirai:a812 -->` or `^a812` markers into moirai-owned Markdown (plans, specs, registers)? That makes those anchors survive anything but modifies repository files. Code files would never get markers.
3. **Should `edited` or `orphaned` anchors gate merges?** Should they automatically make dependent findings, rules and measurements **suspect** *and* block `merge-check` until acknowledged, or only be displayed?
4. **Rust scope layer in v1?** tree-sitter-rust costs +1.8 MB in the binary and ~90 ms per 6k-line file at capture. It turns ~70% of quote orphans into coarse survivals and gives readable anchors. Include it in v1, or ship quote-only first?
5. **Near-synchronous re-anchoring via hooks.** Enable a PostToolUse(Edit/Write) hook that re-resolves anchors right after agent edits (a spawn per edit, zero idle CPU), or re-anchor only lazily at read/check time?
6. **Migration of the existing 34,195 citations.** Should adoption include a one-time import of the BoykoEngine doc citations into anchors?
   - About 20% have ambiguous or missing paths and would need a human or an agent to choose.
   - About 25% are already dead by line number but ~91% of those are recoverable by quote.
   - Or should the citations stay as legacy text?
7. **Cross-file relocation by default.** Should `moirai check` search other changed files for orphaned quotes (code moved between modules) every time, or only under `--deep`? Every time costs up to ~0.5 s warm on this repo [I].

---

## 7. Sources

Specifications and documentation:
- W3C Web Annotation Data Model (selectors, states, refinedBy): https://www.w3.org/TR/annotation-model/
- URL Fragment Text Directives (WICG): https://wicg.github.io/scroll-to-text-fragment/ ; Firefox 131 release notes: https://developer.mozilla.org/en-US/docs/Mozilla/Firefox/Releases/131
- LSP 3.17 (Position encodings, documentSymbol, moniker): https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/
- SCIP schema (symbol grammar, position encodings, enclosing ranges): https://raw.githubusercontent.com/sourcegraph/scip/main/scip.proto ; governance: https://sourcegraph.com/blog/the-future-of-scip ; https://scip-code.org/
- rust-analyzer SCIP generator: https://rust-lang.github.io/rust-analyzer/rust_analyzer/cli/scip/index.html
- Kythe storage model (VName): https://kythe.io/docs/kythe-storage.html ; Kythe schema (anchor loc/start, loc/end): https://kythe.io/docs/schema/
- tree-sitter code navigation (tags queries): https://tree-sitter.github.io/tree-sitter/4-code-navigation.html ; tree-sitter-hlsl: https://github.com/theHamsta/tree-sitter-hlsl
- Universal Ctags tags(5) file format (tagaddress = line number or search pattern): https://docs.ctags.io/en/latest/man/tags.5.html
- git log -L: https://git-scm.com/docs/git-log ; gitattributes built-in diff drivers (rust, markdown): https://git-scm.com/docs/gitattributes
- GitHub section links and custom anchors: https://docs.github.com/en/get-started/writing-on-github/getting-started-with-writing-and-formatting-on-github/basic-writing-and-formatting-syntax
- GitHub permanent links: https://docs.github.com/en/repositories/working-with-files/using-files/getting-permanent-links-to-files ; PR review comments (line/original_line, outdated): https://docs.github.com/en/rest/pulls/comments
- Gerrit ported comments: https://www.gerritcodereview.com/2020-11-18-gerrit-news-jun-nov-2020.html ; https://www.gerritcodereview.com/3.4.html
- Obsidian links and block references: https://obsidian.md/help/links
- Org-mode search options in file links: https://orgmode.org/manual/Search-Options.html
- rustdoc intra-doc links: https://doc.rust-lang.org/rustdoc/write-documentation/linking-to-items-by-name.html
- Claude Code hooks (PostToolUse input): https://code.claude.com/docs/en/hooks

Source code:
- Hypothesis client anchoring: https://github.com/hypothesis/client/blob/main/src/annotator/anchoring/match-quote.ts ; https://github.com/hypothesis/client/blob/main/src/annotator/anchoring/html.ts
- approx-string-match (Myers bit-parallel): https://github.com/robertknight/approx-string-match-js
- google diff-match-patch: https://github.com/google/diff-match-patch ; Rust port: https://crates.io/crates/diff-match-patch-rs
- bio (Myers): https://docs.rs/bio ; imara-diff: https://crates.io/crates/imara-diff ; similar: https://crates.io/crates/similar
- CodeTour: https://github.com/microsoft/codetour
- Serena (symbol name paths): https://github.com/oraios/serena ; overload index: https://github.com/oraios/serena/issues/515
- GitHub stack-graphs (archived 2025-09-09): https://github.com/github/stack-graphs
- Aider repo map: https://aider.chat/docs/repomap.html

Papers and posts:
- Hypothesis, "Fuzzy Anchoring" (2013): https://web.hypothes.is/blog/fuzzy-anchoring/ ; "Robust Anchoring": https://web.hypothes.is/robust-anchoring/
- Aturban, Nelson, Weigle, "Quantifying Orphaned Annotations in Hypothes.is": https://arxiv.org/abs/1512.06195
- Phelps & Wilensky, "Robust intra-document locations", WWW9 (2000): https://www.semanticscholar.org/paper/Robust-intra-document-locations-Phelps-Wilensky/bf3a9da17f9dbeb2d2d09f4d562c903e4e9b2f2e
- Reiss, "Tracking source locations", ICSE 2008: https://www.semanticscholar.org/paper/Tracking-source-locations-Reiss/0e3e098f5d40b10b09584c416036422fbe3d013b
- Asaduzzaman et al., "LHDiff", ICSM 2013: https://www.cs.usask.ca/~croy/papers/2013/LHDiffFullPaper-preprint.pdf
- Grund et al., "CodeShovel", ICSE 2021: https://www.cs.ubc.ca/~rtholmes/papers/icse_2021_grund.pdf
- Jodavi & Tsantalis, "Accurate Method and Variable Tracking in Commit History", FSE 2022: https://users.encs.concordia.ca/~nikolaos/publications/FSE_2022.pdf ; block tracking: https://arxiv.org/abs/2409.16185
- Hu & Pradel, "CodeMapper" (Nov 2025): https://arxiv.org/abs/2511.05205
- Swimm auto-sync: https://swimm.io/blog/how-does-swimm-s-auto-sync-feature-work
- CodeStream codemarks (vendor description via JetBrains blog): https://blog.jetbrains.com/idea/2019/05/codestream-captures-knowledge-about-your-code-speeds-up-onboarding-of-new-devs-and-improves-code-quality/

moirai context:
- `docs/research/00-phase1-digest.md` (184/282 dead anchors; STALE/PLANFENCE; `observed_git_sha` staleness)
- `docs/research/01-boyko-workflow-roles.md` §L8
- `docs/research/02-boyko-workflow-orchestration.md` (UG-10 re-derivation by content; the 8-pass citation repair)
- `docs/research/design/30-synthesis.md` (`finding.where = section ref | file:symbol@sha`; `stale` derivation; hooks)
- sibling R4 lenses: `docs/research/10-content-based-move-detection.md`, `docs/research/12-precedents-link-maintenance.md`

---

## 8. How the findings map to R4

| R4 ask | Answer from this lens |
|---|---|
| "links to files do not break when files are moved" | an anchor references a **file node**, not a path; a file move is the file lens's job and rebinds every anchor for free (§4.7) |
| "…locations inside files" (implied by nodes that cite code/doc lines) | **quote + scope + hint + hash**, resolved lazily, gives 90–98% survival on the owner's real citations, versus 27–83% for line numbers (§2.3) |
| "moirai commands to delete, add, move" | anchors need only `repin`, `ack`, `drop` and `pin` (historical). Moving code is detected, not commanded (§4.4) |
| "automatic re-binding when the user moves things himself" | deterministic re-resolution on read/check/hook; silent only for `moved`, **loud** for `edited`/`ambiguous`/`orphaned` (§4.3, §4.4) |
| "every referrer knows" | reverse index file → anchors → referrers; `edited`/`orphaned` propagate *suspect* (§4.4, §4.7) |

---

## 9. Probe files (not published)

The probe scripts and raw outputs are not published. They were:

- **Citation survival:**
  - `survival.py`: extraction, sampling, blame/pickaxe, the six strategies;
  - `analyze.py`;
  - raw rows `survival_rows_first.json` and `survival_rows_blame.json`;
  - summaries `survival_summary_first.txt` and `survival_summary.txt` (blame mode).
- **Headings:** `headings.py` → `headings_summary.txt`.
- **Rust benchmarks** (`bench/`, crate `anchor-bench`; `--features ts` adds the five tree-sitter grammars):
  - `bench_nots.txt`: resolution states, primitives, diffs;
  - `bench_ts.txt`: tree-sitter parse, tags, corpus coverage;
  - `bench_spans.txt`: Rust item spans and name-path uniqueness;
  - `bench_corpus_hash.txt`: corpus stat, read and hash.
- **Capture-time uniqueness:** inline script (§2.4); inputs `rs_files.txt`, `md_files.txt`, `hlsl_files.txt`.
- **Cited path census:** `cited_paths.txt`.
- **SCIP demo:** `scipdemo/`. `index.scip` was deleted after its symbols were extracted.
- **Deleted after use:** the build directories (`bench/target*`, ~1.2 GB), the ctags outputs (`tags_*`, 10.5 MB) and a text extraction of the LHDiff paper.
