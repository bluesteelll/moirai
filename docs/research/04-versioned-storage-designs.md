# 04 — Versioning a database like git: designs, internals, and candidate architectures for moirai

Research date: 2026-09-25. Lens: how to version a graph database the way git versions code. This is research only; nothing here was implemented.

Evidence labels used throughout:

| Label | Meaning |
|---|---|
| **[MEASURED]** | A number someone actually measured and published, with the source named. The measurer is usually the vendor, so treat it as vendor-measured unless noted otherwise. |
| **[CLAIMED]** | A statement of capability or performance with no published methodology, or one that comes from marketing or a summary page. |
| **[DOC]** | Documented behavior or format from an official spec, doc, or source file. |
| **[DERIVED]** | My own arithmetic or inference from the facts above. It is an estimate for sizing, not a measurement. |
| **[UNVERIFIED]** | General engineering knowledge I did not re-verify in this session. The web-search budget ran out part way through (see §13). |

Scope note: per the owner's relayed request, this report uses no information about the owner's other project. It only assumes that project's workflow style: an orchestrator plus role subagents, Workflow scripts, and git worktrees with parallel agents.

---

## 0. TL;DR

1. **Two families of version store exist, and they cost very different amounts per small commit.**
   - *State-snapshot Merkle stores* (git trees, Dolt/Noms prolly trees, Irmin, MST/HAMT) rewrite the root-to-leaf path on every commit. Dolt documents the minimum per-mutation cost as a little over "4 KB × tree depth" [DOC, S11].
   - A DoltLite user measured this on the typical agent workload of one row per commit. After GC, 1,063,396 commits occupied 55 GB, about 52 KB per commit. Before GC, 3.9M commits occupied 341 GB [MEASURED, S26].
   - *Log/changeset stores* (Datomic datoms, event sourcing, git-bug ops, the SQLite session extension, the jj operation log) cost roughly the size of the change, which is hundreds of bytes [DERIVED].
   - moirai's workload is many tiny commits from agents. For that workload the state-snapshot family is one to two orders of magnitude heavier on disk. Its RAM also grows with history, because Dolt keeps its chunk index in memory at about 1% of the store size [CLAIMED, S17].
2. **Merge quality comes from knowing intent, not from the storage structure.**
   - The best merges come from systems that merge typed operations or cells with graph-aware validation afterwards:
     - Dolt's cell-wise 3-way merge plus its `dolt_constraint_violations` tables [DOC, S20].
     - TerminusDB's "build layer → validate schema → advance head" flow [DOC, S33].
     - Kleppmann's move operation, which skips unsafe ops that would create cycles [DOC, S67].
   - jj shows the other key trick: **conflicts are data**. A merge always produces a commit, conflicted values are stored in it, and resolution happens later [DOC, S52]. That suits autonomous agents, who must never be blocked by a merge prompt.
3. **Recommended core: Architecture A**, an op-log commit DAG with a materialized copy-on-write state and delta-overlay branches (§9).
   - Canonical data is an append-only, content-addressed log of commits whose payloads are changesets with before-images, modeled on the SQLite session extension.
   - The current state of the trunk is materialized in a copy-on-write B+tree (the LMDB/redb design family). It holds forward and reverse edge indexes, Datomic-VAET style, so a delete immediately knows every node that referenced the deleted one.
   - Short-lived agent branches are small overlays, as in TerminusDB delta layers.
   - Merges are field-level 3-way merges followed by graph validation for cycles, dangling edges and delete-versus-modify. They emit first-class conflict records in the style of jj and Dolt.
   - Merkle hashing is used for commit ids and optional checkpoint digests. It is not the main index.
4. **Store location: the git *common dir*** (`.git/moirai/`, or wherever `git rev-parse --git-common-dir` points).
   - Every worktree sees it instantly [DOC, S4], which the coordination features ("who claimed the task", "what is blocked") need.
   - Commits are linked to git by recording the git HEAD SHA plus a hash-algorithm tag, and optionally by a `Moirai-Commit:` trailer.
   - Push/pull, if needed later, can go through a custom ref, as Dolt does with `refs/dolt/data` on git remotes [DOC, S21].
5. **IDs: never use bare sequential integers across branches or clones.**
   - Beads switched to hash IDs specifically to prevent merge collisions between agents and branches [DOC, S27].
   - Small integers remain *safe* if all branches live in one store (the common dir) and ids are allocated under that store's write lock. They collide only when separate clones on different machines sync. This is an owner decision (§12).

---

## 1. What moirai's versioning layer must do (distilled from the brief)

| Requirement | Consequence for versioning |
|---|---|
| 1e3–1e6 nodes, mostly small text | Live data is roughly 0.3–300 MB [DERIVED, assuming ~300 B per node plus ~3 edges per node]. The whole current state fits in RAM, but the owner wants minimal RAM, so prefer an mmap'd on-disk structure with a small hot set. |
| Many small commits from agents | Per-commit overhead dominates. A structure whose cost is O(depth × page) per commit multiplies history size by ~100× compared with O(change) [DERIVED]. |
| Parallel agents on separate branches/worktrees that later merge | Branch creation must be O(1). Merge must be automatic, deterministic and non-blocking. Conflicts must be representable and queryable. |
| Graph semantics: subtasks (tree), blockers (DAG), links | Merge must preserve invariants: the hierarchy stays acyclic, the blocker graph stays acyclic, and there are no dangling references. |
| "Maximally synchronous" reference consistency | A reverse index (who references X) must be part of each version's state, and deletes must cascade or tombstone atomically within a commit. |
| History, diff, branches, possibly merge | Content-addressed commit DAG, refs, reflog-like audit trail, diff between any two commits, per-node history ("blame"). |
| Max performance, minimal RAM, Windows 11, portable | Few large append-only files rather than many small files. Group-commit fsync. No reliance on POSIX-only semantics (see §8). |
| Fit with the project's real git repo | Link moirai commits to git SHAs. Survive git rebase/squash, where SHAs change. Plan for the SHA-256 default in Git 3.0 [DOC, S8]. |

---

## 2. The design space in one page

Three orthogonal questions define every system below.

1. **What is stored per version?**
   - **Independent copies (IC)**: a full snapshot per version.
   - **Change-based (CB)**: deltas or ops.
   - **Timestamp-based (TB)**: facts annotated with validity intervals.
   - This is the RDF-archive taxonomy used by the BEAR benchmark and OSTRICH, which found that hybrids beat any single strategy [DOC, S75].
   - Git and Dolt are *IC with structural sharing*: each commit names a full tree, but unchanged subtrees are shared by hash.
   - Datomic and XTDB are TB. Event sourcing, git-bug and SQLite changesets are CB. TerminusDB is CB layers with periodic IC rollups.
2. **How is a version identified?** Either a content hash of the state or log (git, Dolt, jj ops, Irmin, TerminusDB layer ids), or a sequence number or timestamp (Datomic `t`, XTDB system time).
3. **How is a merge computed?**
   - *State 3-way merge*: diff base→ours and base→theirs, then combine. Git, Dolt, Irmin and TerminusDB work this way.
   - *Operation replay/rebase*: git-bug, jj rebase, the SQLite rebaser.
   - *Commutative patches*: Pijul.
   - *CRDT union*: Automerge, Loro, Yjs.
   - *No merge*: Datomic and XTDB have time travel but no branches.

---

## 3. System deep dives

For each system: model → commits, branches, diff, merge, history → costs (storage per change, RAM to open, write amplification, GC) → how graph-level conflicts would surface → status in 2026 → what moirai should borrow or avoid.

### 3.1 Git: object model, packfiles, refs, reflog, GC

**Model** [DOC].
- Content-addressed objects (blob, tree, commit, tag) form a Merkle DAG. Refs are names pointing at commits. The reflog is a per-ref append-only log.
- Objects are written "loose": one zlib file per object. They are later packed with delta compression. `OFS_DELTA` refers to its base by a negative offset in the same pack, and `REF_DELTA` by object id [S2].
- The pack index v2 is a 256-entry fanout followed by, per object, the object id (20 B for SHA-1, 32 B for SHA-256), a CRC32 (4 B) and an offset (4 B, or 8 B for large offsets) [S2]. That is about 28 B per object for SHA-1 [DERIVED].
- Delta example from Pro Git: a 22 KB file edited by one line is stored as the full new version plus a **9-byte delta** for the old one. Git keeps the most recent version intact and stores older versions as deltas [DOC, S7].

**Branches, diff, merge.**
- A branch is a ref. Diff walks two trees and skips subtrees whose hashes are equal, so the cost is proportional to the changed paths.
- Merge is a 3-way merge per path, and text files are merged by line.
- Custom merge drivers can be plugged in per path via `.gitattributes`. The driver *command* must be defined in `.git/config` or `~/.gitconfig`, not in the versioned attributes file [DOC, S6]. That means every clone has to install it separately, which is a real friction point for data kept in the working tree.

**History queries.**
- The `commit-graph` file stores parents, root tree, commit date and a *generation number*. If gen(A) ≤ gen(B), A cannot reach B, which prunes graph walks.
- Commit-graphs are written as incremental chains that merge when a level exceeds a size ratio (default X=2) or 64,000 commits [DOC, S5].
- moirai should copy generation numbers for fast LCA and "is X an ancestor of Y" queries.

**Refs at scale.**
- Reftable is a binary ref and reflog format. On Android's 866k refs: packed-refs takes 62.2 MB and ~409 ms per lookup by name (cold cache), while reftable takes 36.1 MB and ~34 µs; reflog storage drops from 173 MB to 5 MB [MEASURED by the git project, S3].
- Reftable becomes the default for new repositories in Git 3.0, together with SHA-256 and a Rust requirement. LWN reports Git 2.56 in late September 2026 and 3.0 planned alongside a 2.99 LTS around April 2027 [DOC/news, S8]. Git 2.54 is installed on the owner's machine.

**GC** [DOC, S1].
- `gc.auto=6700` loose objects triggers packing. `gc.autoPackLimit=50` packs triggers consolidation.
- Reflog entries expire after 90 days, or 30 days if unreachable.
- Unreachable objects are pruned after 2 weeks, via *cruft packs* by default.

**Many tiny commits.**
- libgit2's fsync PR benchmarked a transaction-per-commit workload at **3.2k commits/s without fsync but 40.8 commits/s with fsync**. The same PR measured 25.3k i/s → 1.43k i/s for many objects per commit [MEASURED, S9]. This is 2017 data on a Linux SATA SSD and is old, but the ratio is the lesson: per-commit durability is bounded by fsync, so group commit is mandatory.

**Worktrees** [DOC, S4].
- "All refs starting with `refs/` are shared" across worktrees, except `refs/bisect`, `refs/worktree` and `refs/rewritten`. `HEAD` is per-worktree.
- So anything moirai stores in the common dir or under `refs/moirai/*` is automatically visible to every agent's worktree.

**Storing a graph *in* git: cost per change** [DERIVED].
- Assume one file per node in a 256-way fan-out directory (1e5 nodes, so ~390 entries per leaf tree at ~54 B each).
- A single-field edit writes a new blob (~0.2 KB), a new leaf tree (~21 KB), a new root tree (~7 KB) and a commit (~0.3 KB). That is ~28 KB of loose objects, because SHAs barely compress, in about 4 new files per commit.
- `gc --auto` would fire about every 1.7k commits. After repack the deltas shrink to around 100 B per commit.
- This is the same "path copy" tax as prolly trees, plus per-file overhead on NTFS.

**Graph conflicts in git.**
- As text: concurrent field edits become line conflicts only if the lines are adjacent. Delete versus modify becomes a "deleted by them" conflict.
- An edge to a node deleted on the other branch merges *cleanly* and silently dangles.
- Cycles merge cleanly and silently.
- Id collisions: two new files with the same name become an add/add conflict.
- Git has no notion of invariants, so every graph check must run after the merge.

**Borrow:** content-addressed commit ids, refs plus reflog, generation numbers, pack-style large segment files with a fanout index, cruft-style delayed pruning, and the worktree ref sharing rules.
**Avoid:** one object file per write, and path-copy trees as the primary history mechanism for tiny commits.

### 3.2 Dolt: prolly trees, chunk store, cell-level merge (with DoltLite and Beads as case studies)

**Model** [DOC, S11, S12].
- Tables are prolly trees: content-addressed B-trees whose chunk boundaries are chosen from content.
- Dolt uses **keys only** to decide boundaries, and targets a **normal distribution around 4 KB** chunks using a CDF-based boundary probability. Noms produced a geometric size distribution instead.
- Chunks are addressed by SHA-512 truncated to 20 bytes.
- Diff "scales with the size of the differences, not the size of the tree".
- Random write cost is `(1+k/w)·log_k(n)`, and "every edit to a table in Dolt is a minimum of 4Kb multiplied by the depth of the tree".
- A commit points at a root value, which maps table names to table roots, and commits form a Merkle DAG [DOC, S11].

**Storage engine.**
- The chunk store (NBS) keeps a table-file index of 8-byte prefix map + lengths + 12-byte suffixes. The index "is loaded into memory at server start up" [DOC, S12].
- Dolt engineers measured that table-file indexes take "about 1% of the size of a Dolt database": 10 GB of RAM for a 1 TB database [MEASURED/CLAIMED, S17].
- The *chunk journal* (v0.75, 2023) appends all chunks to one file. On sysbench it moved writes from 3.7× slower than MySQL to 1.3× [MEASURED by DoltHub, S18].
- *Archives* use zstd dictionary compression. On 170 public DBs they save ~39% (grouped chunks) or ~35% (random-sample dictionaries) compared with Snappy. DoltHub itself corrected an earlier "~50%" claim as selection-biased [MEASURED, S13].
- AutoGC and archives became the default in Dolt 1.75 (2025-10-20) [DOC, S14]. AutoGC triggers after the database grows by 125 MB [DOC, S15].
- Full GC memory "began to scale" with transactions since the last GC, sometimes needing more RAM than the host had. Incremental GC, opt-in, arrived in v1.86.6 (2026-04-28) [DOC, S16].

**Branches, diff, merge.**
- A branch is a ref to a commit. `dolt_diff_<table>` and `dolt_history_<table>` expose diffs and history as SQL.
- Merge is **cell-wise 3-way**. A conflict means the same row and column changed to different values; conflicts land in `dolt_conflicts_<table>` with base/ours/theirs columns.
- Merges can also produce **constraint violations**: foreign keys, unique, check, not-null. These are recorded in `dolt_constraint_violations_<table>`.
- A commit needs these tables cleared unless `@@dolt_allow_commit_conflicts=1` is set [DOC, S20]. DoltHub's FK-merge post covers exactly the case of "add a reference on one branch, delete the parent on the other" [DOC, S22].
- Merges are limited to two parents [DOC, S20].

**Structural sharing, measured.**
- With append-mostly data, history is cheap: ~425 KB/day with history against ~330 KB/day raw.
- Adding a secondary index whose inserts scatter across leaves pushed history growth to **26 MB/day**. The store reached "over 32GB" against 604 MB without history [MEASURED by DoltHub, S19].
- **Lesson for moirai:** random ids and scattered updates (an agent touching arbitrary old nodes) are the worst case for path-copy history.

**Small-commit workload, measured (DoltLite).**
- DoltLite is a SQLite fork with a prolly-tree engine. It is about 18,000 new lines of C, with 8 lines of SQLite changed [DOC, S23].
- A user loaded sample DBs with **one INSERT plus `dolt_commit` per row**:
  - 1,063,396 commits: 438 GB before GC, 55 GB after, and GC took 64 minutes.
  - 3,919,015 commits: 341 GB, and GC failed on internal 2 GiB caps.
  - Test rig: WSL2 with 19.5 GiB RAM and a 16 GiB container limit [MEASURED by a user, S26].
- That is about **52 KB retained per single-row commit** [DERIVED from MEASURED].
- My own estimate agrees: 1e6 nodes × 300 B → ~75k leaf chunks → depth ~4 → 16 KB per tree × 3 trees (nodes, out-edges, in-edges) ≈ 48 KB per commit [DERIVED].
- DoltHub's own benchmarks: DoltLite autocommit writes run **3.1–4.0× slower than SQLite** (~400 µs vs ~125 µs). Reads are at parity with a file-backed SQLite, or about 1.2× slower in memory [MEASURED by DoltHub, S24, S25].

**Case study, Beads.**
- Beads (Steve Yegge) is the closest existing product to moirai: a "memory upgrade for your coding agent" that is a graph issue tracker with blocks/parent-child/relates-to links and a `ready` queue [DOC, S27].
- It began as SQLite plus a JSONL export in git, with a 5 s debounce [DOC, S29].
- In early February 2026 it moved to Dolt exclusively. DoltHub says this let it "scale another order of magnitude" [CLAIMED by vendor, S28].
- Server mode was mandatory for a while. DoltHub then restored an embedded default for single-agent users in April 2026 [DOC, S28].
- IDs are hash-based (`bd-a1b2`, with hierarchical suffixes `.1`, `.1.1`) specifically to avoid merge collisions [DOC, S27].
- A community Rust fork, `beads_rust`, keeps the "classic" SQLite+JSONL-in-git design. It merges `issues.jsonl` 3-way against a saved `beads.base.jsonl` [DOC, S30].
- Beads 1.3.0 (2026-09-15) derives ids as UUIDv5 over the content for some tables [DOC, S27 CHANGELOG].
- Dolt can use **a plain git remote as a Dolt remote**, storing its data under `refs/dolt/data` [DOC, S21]. That is a precedent for keeping a database's history in the project repo without touching the working tree.

**RAM to open a version.**
- Opening a version costs O(depth) chunk reads, the same for any historical version. This is prolly trees' big advantage over log replay.
- Process RAM is dominated by the chunk index (~1% of store) and caches [CLAIMED, S17]. At about 50 GB of history (1e6 tiny commits) that is ~0.5 GB of index [DERIVED].

**Graph conflicts in Dolt** (if nodes and edges are tables):
- Concurrent field edits become a cell conflict.
- Delete versus modify becomes a row conflict.
- An edge to a deleted node becomes an FK constraint violation.
- A blocker cycle is **not detected**, because SQL has no acyclicity constraint. It needs an application check.
- Id collision on insert becomes a primary-key conflict.

**Borrow:** conflicts and violations as *queryable tables*, content-defined chunking if Merkle checkpoints are wanted, generational GC (commit-reachable chunks go to "oldgen"), and the git-remote-as-sync-transport idea.
**Avoid:** committing every agent write as a full Merkle root, and holding the chunk index in RAM.

### 3.3 TerminusDB: immutable delta layers over succinct structures

**Model** [DOC, S33].
- A graph is a stack of immutable **layers**, each named by 20 bytes. The base layer holds the triples in succinct structures borrowed from HDT: front-coded dictionaries, bit sequences, and a wavelet tree for predicate-first lookup.
- Each child layer holds new dictionary entries, the *added* triples, and a membership set of *removed* triples. Each layer points to its parent.
- A **label** (a file holding a 20-byte layer id) is a branch head. Branching means creating another layer on the same parent.
- A write builds a layer, "commit[s] the layer builder without advancing head", checks schema constraints on the hypothetical DB, and only then advances the label.
- Reads walk the layer stack, so depth costs query time. **Delta rollups** compress stacks while keeping the individual commits queryable.

**Memory.**
- The architecture is in-memory: queries run against the succinct representation. TerminusDB claims "approximately 13 bytes per triple" on billion-triple datasets [CLAIMED, S34].
- For moirai at 1e6 nodes × ~10–15 triples that would be ~130–200 MB of RAM [DERIVED from CLAIMED], which conflicts with the minimal-RAM goal.

**Merge.**
- TerminusDB replays one branch's commits onto another and detects conflicts at triple/document level. Its JSON diff/patch API exposes structured patches [DOC, S37].

**Status.**
- DFRNT took over maintenance in 2025. v12 (2025-12-08) runs the auto-optimizer rollups by default and replaced JSON parsing with a 5× faster serde parser; stable 12.0.7 is from August 2026 [DOC, S35].
- The storage library `terminusdb-store` is **Rust** (Apache-2.0), tokio-based and HDT-derived [DOC, S36]. It is the closest existing Rust precedent for "versioned graph store from scratch".

**Graph conflicts.**
- Concurrent edits of a single-valued property show up as a cardinality violation found by schema checking.
- Delete versus modify becomes a removed triple against an added triple on the same subject.
- Dangling references become schema violations if the property is typed as a reference.
- Cycles are not checked unless modeled.

**Borrow:** delta layers as cheap branch overlays; the "build → validate → advance label" commit protocol; rollups; branch = label file.
**Avoid:** fully in-memory succinct structures for the whole store, which are fast but RAM-hungry and awkward for many tiny writes, since each layer is a set of immutable files.

### 3.4 Datomic: an immutable fact log with as-of queries

**Model** [DOC, S38, S39].
- Data is datoms `[e a v tx added?]`. Retractions are datoms with `added=false`.
- Four covering indexes: **EAVT** (row view), **AEVT** (column view), **AVET** (value lookup, only for `:db/index` or unique attributes in Pro) and **VAET**, a reverse index over reference attributes, which is exactly "who points at node 40".
- Indexes are "shallow trees of segments, where each segment typically contains thousands of datoms", rebuilt by background indexing jobs and merged with an in-memory index of recent changes. This is LSM-like.
- A peer cache miss costs "1–2 segment fetches … on the order of 1 millisecond" [CLAIMED].

**History.**
- `as-of`, `since` and `history` are filters over the same indexes, and querying "now" pays no filtering penalty [DOC, S39].
- `d/with` gives speculative databases but "does not let you branch the past" [DOC, S39]. There are no persistent branches and no merge.

**Status.** All editions have been free, with Apache-2.0 binaries, since April 2023 [DOC, S40]. Datomic runs on the JVM, so it is not embeddable in Rust.

**Graph conflicts.** Not applicable: Datomic has one linear transaction log and serializes writes through a single transactor.

**Borrow:** an attribute-level fact/op model; retractions as first-class tombstones; the **VAET reverse index**; "now" served from an unfiltered current index while history is a filter; tx ids as a monotone clock.
**Avoid:** relying on a single serialized writer for multi-branch work.

### 3.5 XTDB v2: bitemporal SQL on Arrow and an LSM over object storage

- GA as 2.0.0 in June 2025, with v2.1.0 in December 2025 and v2.2.0-beta after that [DOC, S41].
- Every table is bitemporal: system time plus valid time per SQL:2011. The primary index is an LSM trie of Apache Arrow files on object storage, with a log for coordination [DOC, S41]. A background GC removes superseded files.
- It has no branching and no merge.
- **Relevance to moirai is low.** Valid time ("this rule is valid from X to Y") is better modeled as ordinary fields. Bitemporality is not branching.

### 3.6 Irmin: a git-like store as an OCaml library

- Irmin is a store library with **user-defined 3-way merge functions per content type** (`Irmin.Merge`). A merge returns `Ok v` or ``Error (`Conflict msg)`` [DOC, S44].
- Backends: irmin-git (git-format compatible), irmin-pack, irmin-fs and in-memory [DOC, S42].
- For Tezos, the irmin-pack GC picks a commit, copies everything reachable into a `prefix` file indexed by offset, keeps newer objects in a `suffix` file, and persists a `mapping` file. Tezos needed "~35GB … for 6 cycles", plus "an additional 40GB" temporarily during each pruning run [MEASURED/CLAIMED by Tarides, S43].
- Latest release is 3.11.0 (2025-06-19), and it is still maintained by Tarides [DOC, S42].
- **Borrow:** typed merge combinators, where each field type declares its own 3-way merge (LWW register, counter, set, text) and the result can be a conflict. **Avoid:** GC designs that need scratch space equal to live data.

### 3.7 Noms, prolly trees, Merkle Search Trees and HAMTs as index structures for Merkle state

| Structure | Canonical (history-independent)? | Ordered / range scans | Write cost | Notes |
|---|---|---|---|---|
| Prolly tree (Noms → Dolt) | Yes | Yes | ≥ chunk × depth [DOC, S11] | Content-defined boundaries via a rolling hash over keys. Noms repo archived 2021-08-28, and its README says "not being maintained" [DOC, S45]. Rust crates `prollytree` 0.4.1 (2026-08-27, ~3.7k downloads) and `prolly-map` 0.7.2 exist but are immature [DOC, crates.io]. |
| Merkle Search Tree (Auvolat & Taïani, SRDS 2019) | Yes | Yes | Path copy | Each key's layer comes from leading zeros of its hash; atproto uses SHA-256 with 2-bit groups, giving fanout 4 [DOC, S46, S47]. Simpler than prolly trees (no rolling hash) and battle-tested at Bluesky scale. Rust crate `merkle-search-tree` 0.8.0 (2024). |
| HAMT (Bagwell; IPLD spec) | Yes (IPLD canonical rules) | **No**: "random for practical purposes" | Path copy | IPLD defaults: bitWidth 8 (256 slots), bucketSize 3 [DOC, S48]. Fine for lookup by id, useless for range or index scans. |

What all three share for moirai: equal subtrees have equal hashes, which gives O(diff) comparison and sync. The cost is that every commit writes depth × node-size bytes. They are worth using as **checkpoint digests** (for example, one MST over `node_id → hash(node)` computed every K commits for verification or sync), not as the per-write store.

### 3.8 Jujutsu (jj): operation log, first-class conflicts, working copy as a commit

**Operation log** [DOC, S50, S51].
- Every command produces an *operation* object that points at parent operations and at a *view*. The view is a snapshot of heads, bookmarks, git refs and each workspace's working-copy commit.
- Operations and views are content-addressed, so they are "safe to write without locking".
- Op heads are empty files named by op id. On completion jj adds the new file and removes the old one.
- Concurrent commands produce divergent op heads. The **next** command notices and "do[es] a 3-way merge of the view objects based on their common ancestor". A bookmark moved concurrently becomes "moved from A to B or C", which is a conflicted ref rather than an error.
- `jj undo`, `jj op revert` and `jj op restore` give whole-repo undo, and `--at-op` reads any past repo state.

**First-class conflicts** [DOC, S52].
- A conflicted value is stored as an odd-length list of trees A, B, C, D, E meaning A+(C−B)+(E−D), and it simplifies algebraically on rebase.
- If all sides made the same change, the conflict resolves automatically. Merged contents are computed lazily where needed.
- Descendants of a conflicted commit can be created, and the conflict can be resolved later.

**Working copy as a commit** [DOC, S53].
- Most commands first snapshot the working copy into the working-copy commit, and that snapshot is itself an operation.
- Multiple workspaces share one repo. A workspace goes *stale* when another workspace rewrites its commit, and `jj workspace update-stale` fixes it.

**Ids** [DOC, S56]. Change ids are 16 bytes, "often randomly generated", and stay stable across rewrites. Commit ids are 20-byte git hashes. A change that ends up with more than one visible commit is flagged "divergent".

**Git coexistence** [DOC, S54, S55].
- jj stores commits as git objects via gitoxide. Extra metadata (change id, predecessors) lives in a side `StackedTable`.
- jj keeps its commits alive against git GC with refs under `refs/jj/`.
- jj does not support git worktrees; it has its own workspaces. v0.45 added per-workspace git HEAD for colocated repos [DOC, S57].

**Status.** jj-lib 0.45.1 was released 2026-09-03 on a monthly cadence [DOC, crates.io].

**Graph conflicts, if moirai adopted jj's model:**
- A concurrent field edit becomes a conflicted field value (base, ours, theirs) stored in the merge commit.
- Delete versus modify becomes a conflict with an "absent" side.
- Dangling edges and cycles become invariant violations that must be recorded alongside.

**Borrow (the most valuable system for moirai's agent workflow):**
1. The two-level history: an **op log** for every write (fine-grained undo and audit) and a **commit DAG** for meaningful units.
2. **Conflicts as stored values**, so merges never block an agent.
3. **Lock-free multi-process writes** via content-addressed ops and op-head files, with automatic view merge.
4. **Stable change ids** that are distinct from content hashes.

### 3.9 Fossil: a VCS inside SQLite

- The canonical data is an unordered set of immutable, hash-named **artifacts** (manifests, control, wiki, ticket-change artifacts and so on). Derived tables in the same SQLite file are rebuilt from them with `fossil rebuild`: the metadata "contains no new information" [DOC, S58, S59].
- Blobs are zlib- and delta-compressed. The SQLite project's 7.1 GB of content fits in under 97 MB, about **74:1** [MEASURED by Fossil, S59].
- **Tickets** are sets of change artifacts with J-cards (field = value, where a `+` prefix means append). State is computed by applying changes in timestamp order, which is **last-writer-wins per field** with no conflict markers [DOC, S60, S31].
- **Borrow:** "canonical immutable log plus rebuildable derived indexes", which makes schema evolution and index rebuilds safe; append-able text fields as an operation type.
- **Avoid:** silent timestamp LWW for anything agents coordinate on, because clock skew between processes decides the winner.

### 3.10 Pijul: patch theory, plus Sanakirja

- The repository is a graph of lines. Patches add vertices and edges, and **independent patches commute**, so merge order does not matter.
- Conflicts are *states of the graph* rather than failures: disconnected live vertices, cycles, and "zombie" vertices. Pijul describes itself as a CRDT [DOC, S62].
- Status: `pijul` 1.0.0-beta.24 (2026-09-14), still beta [DOC, crates.io].
- **Sanakirja**, Pijul's Rust storage engine:
  - A copy-on-write B-tree over mmap or an allocator. One writer and many readers, with cross-process file locks.
  - Optional reference counting enables **O(log n) forks (clones) of tables** [DOC, S63].
  - The author claimed it is faster than LMDB, but I could not retrieve the benchmark page because it renders client-side [CLAIMED, not verified].
  - Stable 1.4.3, with 2.0.0-beta.3 released 2026-07-06 [DOC, crates.io].
- **Borrow:** a commutation analysis for the merge engine. Ops on different nodes or fields commute. Ops on the same scalar field do not. Edge additions commute except when they close a cycle. Also borrow cheap CoW forks as the way to materialize a long-lived branch.

### 3.11 CRDTs (Automerge, Loro, Yjs) as an alternative merge model

- **Automerge**:
  - Concurrent writes to one map key pick a deterministic winner by internal op id, not wall clock. The losers stay retrievable with `getConflicts` [DOC, S65].
  - Automerge 3.0 (2025) keeps the compressed columnar history *in memory*. Pasting Moby Dick went from 700 MB to 1.3 MB, and a document that had not loaded after 17 hours loaded in 9 seconds [MEASURED by the Automerge team, S64].
  - The Rust crate `automerge` is at 0.12.0 (2026-09-16).
- **Loro** (Rust, 1.16.2 on 2026-09-21):
  - Keeps the full edit DAG "like Git", with `checkout(frontiers)`, `fork`/`fork_at`, merge, and *shallow snapshots* (history truncation, like a shallow clone). In one example a snapshot of 5421 B became a 869 B shallow snapshot.
  - The v1.0 blog claims load times for a doc with millions of ops dropping from 16 ms to 1 ms (0.37 ms shallow) [CLAIMED, S66].
  - Its **movable tree** implements Kleppmann et al.'s move operation: ops sorted by Lamport timestamp, *undo–do–redo* on remote arrival, and an op that would create a cycle is **skipped**. Deletion is a move to TRASH, and trashed nodes stay in memory so concurrent moves into a deleted subtree stay consistent [DOC, S66, S67].
  - Benchmarks: 10,000 random moves in 28 ms; 1,000 version switches in 153 ms on a 1,000-node tree, or 701 ms at depth 300 [MEASURED by Loro, S66].
  - Sibling order uses fractional indexes with random jitter, and ties are broken by peer id.
  - Open bugs in 2026 involve `fork_at`/`checkout` stack overflows after concurrent tree moves [DOC, S66 releases/issues].
- **Yjs**: deleted content is garbage-collected unless `doc.gc = false`, which is required to "restore old content", meaning version history [DOC, S69].
- **Cycle-handling catalogue**: Matthew Weidner's CRDT survey lists the options [DOC, S68]:
  1. A "time-out zone" for cycle members.
  2. Server-order rejection, as Figma does.
  3. Topological-order skipping, Kleppmann style.
  4. For forests, *hide* the edge with the largest LWW timestamp at render time without changing state.
- **Merkle-CRDTs** (Sanjuán et al., 2020): a Merkle DAG *is* a logical clock, and a node can carry CRDT payloads [DOC, S70]. That is a direct bridge between git-style commits and CRDT merges.
- **Graph conflicts in a CRDT**: merges never fail, by construction. Concurrent field edits resolve by LWW (optionally with conflicts retained). Delete versus modify depends on type, usually delete-wins or tombstone. Dangling edges are allowed unless edges are modeled as tree moves. Blocker cycles are **not prevented**, because a DAG is not a tree and Kleppmann's move applies to trees only. Ids are unique per actor plus counter.
- **Borrow:** Lamport or hybrid logical clocks as deterministic tie-breakers, add-wins sets for labels and tags, Kleppmann move semantics for the parent/child hierarchy, fractional indexes for child order, and "keep the losers" for LWW.
- **Avoid:** CRDTs as the *only* merge mechanism. Silently choosing a winner hides semantic conflicts that agents need to see, and whole-document-in-RAM runtimes cut against the RAM goal.

### 3.12 Event sourcing plus snapshots (git-bug as a git-hosted instance)

- **Fowler** [DOC, S71]: rebuild state by replaying events; temporal queries by replaying up to T; snapshots to bound replay; "multiple time-lines (analogous to branching)"; pitfalls with external side effects on replay.
- **git-bug** [DOC, S32, S31]:
  - Each entity (bug) is a DAG of git commits under `refs/<namespace>/<id>`. Each commit's tree holds an `OperationPack` blob of JSON ops plus Lamport clock markers.
  - The entity id is the hash of the first op.
  - Divergent tips are merged by replaying ops in Lamport order, with the pack id as tie-breaker. **No conflict is ever surfaced**: when two people set the same field, "the replay picks a winner by clock order".
  - Custom refs are not fetched by default, and every edit adds a commit that each clone carries forever [DOC, S31].
- **Borrow:** an op log as canonical data, a deterministic replay order, and entity id derived from the creation op.
- **Avoid:** surfacing no conflicts at all, and one ref per entity (1e5–1e6 refs means slow ref negotiation even with reftable).

### 3.13 SQLite session extension: changesets, patchsets and a rebaser

- A **changeset** records INSERTs with all values, DELETEs with all old values, and UPDATEs with PK plus old and new values of the changed columns. A **patchset** omits the old values and so detects fewer conflicts.
- A change that is made and then undone within one session disappears. Tables need a declared PRIMARY KEY [DOC, S72].
- Changesets can be **inverted** (undo), **concatenated**, and **rebased**: `sqlite3_rebaser` rewrites a local changeset to reflect how conflicts with a remote changeset were resolved, "so that the same conflicts do not have to be resolved elsewhere" [DOC, S72].
- **Conflict taxonomy** [DOC, S72], which maps almost one-to-one onto graph conflicts:
  - `SQLITE_CHANGESET_DATA`: the row exists but a field no longer has its expected before-value. This is a *concurrent edit of the same field*.
  - `SQLITE_CHANGESET_NOTFOUND`: an update or delete targets a missing row. This is *modify-vs-delete*.
  - `SQLITE_CHANGESET_CONFLICT`: an insert duplicates a PK. This is an *id collision*.
  - `SQLITE_CHANGESET_CONSTRAINT`: UNIQUE, CHECK or NOT NULL is violated. This is a *schema or type violation*.
  - `SQLITE_CHANGESET_FOREIGN_KEY`: FK violations remain at the end. These are *dangling edges*.
  - The handler returns OMIT, REPLACE or ABORT.
- **Borrow:** the changeset format (op plus before-image), which enables inversion (undo, as-of by reverse application), precise conflict detection and blame. Take this taxonomy and add a graph-specific **CYCLE** class.

### 3.14 LMDB, redb and Sanakirja: copy-on-write B+trees as a snapshot base

**LMDB.**
- Copy-on-write B+tree over mmap with MVCC readers and a single writer. There is no WAL.
- Freed pages are reused rather than returned to the OS. Long-lived readers pin old pages and cause growth; early OpenLDAP saw "truly explosive database growth" from this [DOC, S74 Symas].
- **Windows caveat:** the data file is sized to the full `mapsize` up front, whereas Linux and macOS use sparse files. Bindings report this, and py-lmdb enables NTFS sparse files to work around it [DOC, S74].

**redb** (pure Rust, 4.3.0 on 2026-09-14).
- Copy-on-write B+trees. A one-byte "god byte" selects between two commit slots, with checksums instead of a second fsync by default ("1PC+C").
- Readers pin roots. **Savepoints** capture root plus allocator state, "approximately 64KB per 1GB of data", and persistent savepoints must be freed explicitly [DOC, S73].
- Multi-process access was *experimental* in 4.3, and PR #1462 (2026-09-06) hardened the multi-writer open path [DOC, S73].
- There was a Windows-only commit hang, fixed in 4.2 [DOC, S73 CHANGELOG].
- README benchmark on a Ryzen 9950X3D with NVMe [MEASURED by redb author, S73]:

  | | Individual writes | Random reads | Uncompacted size | Compacted size |
  |---|---|---|---|---|
  | redb | 920 ms | 1138 ms | 4.00 GiB | 1.69 GiB |
  | lmdb | 1598 ms | 637 ms | 2.61 GiB | — |
  | sqlite | 7040 ms | — | 1.09 GiB | — |

- **Write amplification of a copy-on-write B+tree** [DERIVED]: each commit rewrites depth × page, about 3–4 × 4 KB = 12–16 KB of I/O. Unlike a Merkle store, the old pages are *freed* unless a snapshot pins them, so disk does not grow with history.
- **Snapshots as versions:** keeping every commit's root is equivalent to a path-copy Merkle store (same ~12–16 KB per commit per tree) but without cross-branch deduplication. So these engines are a good *materialized current state* engine plus occasional pinned checkpoints, not a full history store.
- **Borrow:** the dual commit slot plus checksum protocol, reader pinning, and Sanakirja-style refcounted forks for long-lived branches. Treat the "95% of the effort is testing" warning from the lobste.rs thread [S63] as a real schedule risk.

### 3.15 Other precedents worth knowing

- **In-repo issue trackers** (Nesbitt's survey, 2026-08-20) [DOC, S31]. Four storage families:
  - Files in the tree (Bugs Everywhere, ditz, git-issue): merge by text 3-way; authorship from `git blame`.
  - Orphan branches (ticgit).
  - git notes (git-appraise, which used line-delimited JSON so git's line merge deduplicates).
  - Custom refs (git-bug, Radicle, Gerrit NoteDb).
  - `git clone` fetches files and orphan branches by default, but *not* notes or custom refs.
- **Quit Store** (RDF in git): offers Union, Ours, Theirs, 3-way and a *Context Merge*. Context Merge marks nodes touched by both sides as conflicts even when the triple-level 3-way merge is clean [DOC, S76]. That is exactly the "semantic conflict" layer moirai needs above field merges.

---

## 4. Consolidated comparison of the surveyed systems

"Per small commit" means one or two field edits. Numbers are labeled.

| System | Commit = | Branch = | Diff cost | Merge | History query | Storage per small change | RAM to open/query | Write amp. | GC |
|---|---|---|---|---|---|---|---|---|---|
| Git (files-as-nodes) | commit → tree Merkle root | ref | O(changed paths) | line 3-way per path, custom drivers | log/blame walk; commit-graph gen numbers | ~15–30 KB loose before pack; ~0.1 KB after delta pack [DERIVED] | pack idx ~28 B/object, mmap'd [DOC → DERIVED] | path copy plus a file per object | gc.auto 6700 loose; prune 2 weeks [DOC] |
| Dolt / DoltLite | commit → root value → prolly roots | ref | O(diff) [DOC] | cell 3-way plus conflicts and violations tables [DOC] | `dolt_history_*`, `dolt_diff_*` | ≥4 KB × depth per tree [DOC]; **~52 KB retained per single-row commit** [MEASURED, S26] | chunk index ~1% of store [CLAIMED]; any version O(depth) | high | mark-sweep, generational, incremental opt-in [DOC] |
| TerminusDB | new delta layer | label → layer | layer diff | replay commits; schema validation | walk layers; rollups | small layer per commit (several files) | in-memory, ~13 B/triple [CLAIMED] | low per write, periodic rollup rewrite | rollups; old layers archivable |
| Datomic | tx of datoms | — (none) | since(t) filter | — | as-of / since / history filters [DOC] | ~size of datoms plus later index merges | segment cache; memory index | LSM-like re-indexing | indexing jobs |
| XTDB v2 | tx | — | temporal SQL | — | bitemporal SQL:2011 [DOC] | columnar Arrow | object-store LSM | LSM compaction | background GC |
| Irmin | commit → Merkle tree | branch | tree diff | typed 3-way merge functions [DOC] | commit walk | path copy | small | path copy | prefix/suffix GC; needs temporary space [DOC] |
| MST / HAMT / prolly | root hash | ref | O(diff) | 3-way on keys | — | path copy | O(depth) | path copy | reachability |
| jj | op (+view) and commit | bookmark / anonymous heads | tree diff | 3-way; conflicts stored as values [DOC] | op log, `--at-op`, evolution log | git objects plus op/view objects | index for revsets | git plus op objects | `jj util gc`, `op abandon` |
| Fossil | manifest artifact | tag (control artifact) | manifest diff | text; tickets LWW per field [DOC] | derived tables | delta plus zlib; 74:1 on SQLite repo [MEASURED] | SQLite page cache | low | rebuild derived tables |
| Pijul | patch | channel | patch set diff | commutative; conflicts as graph state [DOC] | patch log | patch size | Sanakirja mmap | CoW B-tree | — |
| Automerge / Loro | change (ops) | fork / frontiers | version diff | CRDT union, deterministic LWW [DOC] | checkout(version) | tens of bytes per op, columnar [CLAIMED] | **whole doc in RAM** (compressed in Automerge 3) [DOC] | low | shallow snapshots (Loro) |
| Event sourcing / git-bug | event batch | stream fork | event range | replay by Lamport clock (git-bug) | replay to T | ~size of event | snapshot plus replay | append-only | snapshots, truncation |
| SQLite session | changeset | — | changeset | apply with 5 conflict classes plus rebaser [DOC] | invert changesets | ~size of change plus before-image | — | — | — |
| LMDB / redb | txn (root flip) | savepoint / fork (Sanakirja) | — | — | pinned snapshot only | path copy, then **freed** | mmap working set | depth × page per commit | free-page reuse |

---

## 5. Graph-level merge conflicts: taxonomy, how systems handle them, and a moirai policy

The table uses the SQLite session classes, extended for graphs.

| # | Conflict | Git (text) | Dolt | jj | CRDT (Automerge/Loro) | TerminusDB | **Proposed moirai default** |
|---|---|---|---|---|---|---|---|
| C1 | Same scalar field edited differently (e.g. `status`, `priority`) | line conflict only if the lines are adjacent | cell conflict row (base/ours/theirs) | conflicted value in the commit | LWW winner; losers kept (Automerge) | cardinality violation | **Conflict value** `Merge{base, ours, theirs}` stored in the field (jj style). An optional per-field policy can auto-resolve: a *lattice* for status (e.g. `done` > `in_progress` > `open`, if the owner wants that), max for priority, and HLC LWW only for fields marked low-stakes. |
| C1b | Same text field edited (note body) | line merge | cell conflict | conflicted content | text CRDT merges characters | conflict | Line-level diff3 inside the field. Overlapping hunks become a conflict value. |
| C1c | Set-valued field (labels, tags) | line merge | cell conflict | conflict | add-wins or OR-set | — | **Add-wins set union** with removals relative to base, so no conflict. |
| C2 | Delete vs modify | "deleted by them" | row conflict | conflict with an absent side | usually delete-wins or tombstone | add/remove clash | **Conflict** by default. Per-type policy options: notes and decisions use "modify resurrects"; tasks use "delete wins, record the lost edit". Deletion is always a tombstone within history, so it can be reversed. |
| C3 | Edge to a node deleted on the other branch | clean merge, silent dangling | **FK constraint violation** row | not modeled | dangling, or TRASH semantics for trees | schema violation | **Violation record** `DanglingEdge{edge, target}`. Default auto-action: keep the edge but mark it *broken*, so it is visible in queries and the "ready" computation treats a broken blocker as a blocker until resolved. The reverse index makes detection O(degree of the deleted node). |
| C4 | Cycle created by merging two acyclic blocker graphs | clean, silent | not detected | not modeled | not prevented for DAGs; for trees, Kleppmann skips the later move [DOC] | not detected | **Violation record** `Cycle{edges}` found by an incremental cycle check while replaying the incoming branch's edge additions in deterministic order (HLC, then id). The merge commit still lands. The blocker edges in the cycle are flagged, and the tasks involved are excluded from `ready` until an agent resolves it. An optional auto-policy drops the newest edge (Weidner #5 / Kleppmann) and logs it. |
| C4b | Cycle in the subtask hierarchy (A parent of B on one side, B parent of A on the other) | — | — | — | Kleppmann move: skip the unsafe op [DOC] | — | **Kleppmann move semantics** (deterministic, proven [S67]) plus a notification record. |
| C5 | Id collision (two different nodes created with the same id) | add/add | PK conflict | change ids are random, so rare | actor-unique ids | IRI clash | **Prevent by construction.** 128-bit ids (random, or UUIDv7 for insert locality). Show a short prefix like beads/jj, or allocate sequential ids under one store lock (see §12). If a collision still happens, treat it as a conflict and never overwrite silently. |
| C6 | Semantic duplicate (both agents created "Fix login bug") | none | none | none | none | none | Not a storage conflict. Offer a post-merge *duplicate hint* (same title or embedding match) and a `duplicates` link type, as beads has [S27]. |
| C7 | Type or schema conflict (field type changed on one branch) | text | schema conflict | — | — | schema check | Schema lives in the same commit DAG. Type changes are ops. A merge with incompatible type changes becomes a conflict. |
| C8 | Sibling order conflict (both inserted child at position 3) | line merge | — | — | fractional index plus jitter plus peer tie-break (Loro) [DOC] | — | Fractional index with jitter and a deterministic tie-break, so no conflict. |
| C9 | Derived state disagreement ("parent done because all children done", "blocked count") | — | — | — | — | — | **Never merge derived data.** Recompute it from the merged primary data inside the merge transaction. |
| C10 | Concurrent claim of the same task by two agents | — | cell conflict | — | LWW (one silently loses) | — | Do not branch coordination state (see §9.3). Claims are linearizable writes on trunk (compare-and-set), so a double claim is rejected at write time instead of discovered at merge. |

**Merge algorithm sketch for a graph with typed ops** [DERIVED design]:
1. Find the LCA using commit generation numbers, as git's commit-graph does [S5].
2. Collect each side's changesets since the LCA. They already contain before-images.
3. Partition them by (node, field) and edge key. Changes on disjoint keys commute and apply directly (Pijul's insight). Overlapping keys go through the per-type 3-way merge function (Irmin's combinator idea).
4. Apply the combined result to an overlay on top of the target state (TerminusDB's "commit layer without advancing head").
5. Run the validators: acyclicity of blockers (incremental DFS or dynamic topological order from the changed edges only), hierarchy-is-a-forest, dangling references (reverse index), and schema/type.
6. Write the merge commit with its conflict values and violation records. These are queryable, like Dolt's system tables, so an agent can run `moirai conflicts` and `moirai violations`.
7. Advance the ref (or not, if the caller asked for a strict merge).

---

## 6. Scale evaluation for moirai

Workload assumptions [DERIVED]:
- N = 1e5 nodes by default (range 1e3–1e6), ~300 B per node payload, E ≈ 3N edges.
- 1e5–1e6 commits over the project's life.
- Commits touch 1–3 fields each. Bursts of about 10 commits/s from about 10 agents.

| Metric | Op-log changesets (A) | Prolly / MST state per commit (B) | Git objects (C1) | CRDT doc (D) |
|---|---|---|---|---|
| Bytes appended per small commit | ~0.3–0.6 KB (ops with before-images ~100 B each plus ~200 B header) [DERIVED] | ~36 KB at N=1e5, ~48 KB at N=1e6 (3 trees); **~52 KB measured** on DoltLite single-row commits [MEASURED, S26] | ~15–30 KB loose; ~0.1–0.5 KB after repack [DERIVED] | tens of bytes per op after columnar compression [CLAIMED, S64] |
| History for 1e6 commits | **~0.3–0.6 GB** (roughly 2–4× less with zstd, not measured) | **~40–55 GB** | ~0.3 GB packed, but tens of GB loose before gc [DERIVED] | < 1 GB (not measured at this node count) |
| RAM to serve "current" queries | mmap'd B+tree hot set: tens of MB at 1e6 nodes [DERIVED] | chunk index ~1% of store, **~0.5 GB at 50 GB** [DERIVED from CLAIMED] plus cache | pack idx ~28 B/object plus a *derived* query index | whole doc resident (size unknown for 1e6 nodes, needs a benchmark) |
| Open an old version | per-node: O(changes to that node) via per-node op chain; whole graph: reverse-apply from current or forward from checkpoint, bounded by checkpoint spacing | **O(depth)**, uniform for all versions | re-materialize the derived index (expensive) or query tree objects directly | checkout = undo/redo (Loro: 1000 switches in 153 ms on a 1000-node tree [MEASURED, S66]) |
| Commit latency floor | one append plus group fsync | chunk writes plus journal fsync (DoltLite autocommit ~400 µs vs SQLite ~125 µs [MEASURED, S25]) | 4+ files plus fsync per file (libgit2: 40 commits/s with fsync, 2017 [MEASURED, S9]) | append plus fsync |
| GC pressure | low: checkpoint plus truncate old segments | high: mark-sweep over millions of chunks; DoltLite GC failed at 3.9M commits [MEASURED, S26] | medium: gc/repack of millions of small objects | low–medium: shallow snapshots |

**Windows-specific notes** (evidence mixed):
- Prefer a few large append-only files over many small files. One-file-per-object patterns (git loose objects, the jj op-head files) cost more on NTFS, where each create is expensive and Defender scans new files [UNVERIFIED, general knowledge].
- LMDB-style fixed map sizes pre-allocate the full file on Windows [DOC, S74]. Grow in chunks and remap instead.
- Windows file locks (`LockFileEx`) are mandatory rather than advisory, and a file mapped or open without `FILE_SHARE_DELETE` cannot be replaced by rename. Any "write temp file, rename over" protocol, such as jj op heads or git lockfiles, must be tested on Windows [UNVERIFIED].
- redb only fixed a Windows-only commit hang in 4.2 (2026) [DOC, S73]. Storage engines hit Windows-specific I/O bugs late, so plan Windows CI from day one.
- fsync (`FlushFileBuffers`) dominates small-commit latency on every OS. Use group commit, with a configurable durability level per write class.

---

## 7. Where the data lives relative to the git repo

| Option | Shared across worktrees instantly | Travels with `git clone/push` | Versioned *with* code history | Merge mechanics | Main risks |
|---|---|---|---|---|---|
| **L1: working tree files** (JSONL / one file per node, like beads classic) | **No.** Each worktree has its own checkout until branches merge. | Yes | Yes (in PRs) | git text merge or custom driver (driver config not versioned [DOC, S6]) | Coordination state diverges between agents; churn in code diffs; binary DB files cannot merge |
| **L2: git common dir** (`$(git rev-parse --git-common-dir)/moirai/`) | **Yes** | No (needs its own sync or export) | No, linked by SHA | moirai's own merge | Lost if `.git` is deleted; not in backups made by clone |
| **L3: custom refs** (`refs/moirai/*`) in the git object DB | Yes (refs are shared [DOC, S4]) | Only with an explicit refspec; not fetched by default [DOC, S31] | No, linked by SHA | moirai's own merge | git ODB is slow for many tiny writes; GC safety needs refs (jj uses `refs/jj/` [DOC, S55]); SHA-256 transition |
| **L4: outside the repo** (per-user app data keyed by repo id) | Yes | No | No | own | Discovery and multi-repo mapping; surprises for users |

**Recommendation** [DERIVED]:
- Use **L2 as the primary store**.
- Add an optional **L3 mirror** (export commits and segments under `refs/moirai/data` for push/pull, following Dolt's `refs/dolt/data` precedent [S21]).
- Add an optional **L1 export** (a sorted, human-readable JSONL snapshot on demand) for reviewers, beads-style. The export is never canonical.

**Linking moirai commits to git commits:**
- Record in each moirai commit: `git_head` (hash bytes plus an algorithm tag, sized for 32-byte SHA-256 before Git 3.0 makes it the default [S8]), `git_branch`, `worktree_id`, `agent_id` and `session_id`.
- Optionally add a `Moirai-Commit: <id>` trailer to git commits via a `prepare-commit-msg` hook or the agent workflow, and/or a `refs/notes/moirai` note.
- git rebase and squash rewrite SHAs, so treat the SHA link as *best-effort provenance*, not a foreign key.
- A stable, jj-style *change id* for moirai commits is what agents should cite.

---

## 8. Lessons that directly shape moirai (distilled)

1. **Per-commit Merkle roots are too expensive for tiny agent writes.** This is measured, not theoretical: ~52 KB per commit [S26], scattered writes are the worst case [S19], and GC falls over at millions of commits [S26].
2. **An op log with before-images is canonical, and everything else is derived and rebuildable** (Fossil [S59], Datomic [S38], git-bug [S32], SQLite changesets [S72]).
3. **The reverse index is part of the versioned state** (Datomic VAET [S38]). It is how a delete immediately reaches every node that referenced it, and how merges find dangling edges in O(degree).
4. **Validate before advancing the ref** (TerminusDB [S33]), and **store conflicts as data** (jj [S52], Dolt [S20]).
5. **Two histories are better than one** (jj [S51]): an op log for every mutation (undo, audit, crash recovery) and a commit DAG for meaningful units.
6. **Hierarchy merges use Kleppmann move semantics** [S67]. Blocker DAG merges need explicit cycle detection plus violation records, because no CRDT prevents DAG cycles [S68].
7. **IDs are globally unique by construction** (beads [S27], jj change ids [S56], Automerge actor ids).
8. **Coordination state should not be branched.** Beads moved to a single shared Dolt database with server mode for multi-writer concurrency [S27, S28]. Agents need a linearizable trunk for claims and status, and branches mainly for speculative knowledge.

---

## 9. Candidate versioning architectures

### Architecture A: op-log commit DAG + materialized CoW state + delta-overlay branches (recommended)

- **Canonical data:** append-only *segment files* (for example 64 MB each) of commits.
  - A commit is a header (parents, change id, author/agent, HLC time, git link, message) plus a *changeset*: typed ops with before-images, such as `SetField`, `AddEdge`, `RemoveEdge`, `Create`, `Tombstone` and `Move`.
  - The commit id is BLAKE3 over header and changeset. The parents are inside the header, so commits form a Merkle DAG and are tamper-evident.
  - A small index maps commit id → (segment, offset), in git pack-idx style with a fanout table.
- **Refs:** a small refs table (name → commit id) plus a ref log. Updates use a two-slot atomic header (the redb god-byte idea) or a single-writer lock.
- **Op log:** every mutation (a jj-style operation) records the ref state before and after, which gives undo and audit. Moirai "commits" can be made per mutation (auto-commit) or per agent session, as the owner decides (§12).
- **Materialized trunk state:** a CoW B+tree file (LMDB/redb design, built from scratch) with these tables:
  - `nodes(id → record)`
  - `out(src, type, dst)` and `in(dst, type, src)`, the reverse index
  - secondary indexes such as `(type, status) → id` and `open_blocker_count` for the ready queue
  - per-node `last_op` pointers, so per-node history is a linked chain through the op segments
- **Branches:**
  - Creating a branch is O(1): a ref.
  - An *active* branch has a **delta overlay**, a small B+tree of changed nodes and edges since its fork point, as in TerminusDB layers. Reads check the overlay first, then the base.
  - A long-lived branch that grows large can be *promoted* to its own materialized state via a CoW fork (Sanakirja-style refcounted pages) or rebuilt by replay.
- **Diff:** the changeset union between two commits, O(ops between them). For far-apart commits, also compare checkpoint digests.
- **Merge:** the §5 algorithm, producing conflict values and violation records.
- **History and as-of:**
  - A single node at commit X: walk the node's op chain, which costs O(edits to that node).
  - The whole graph at an old commit: reverse-apply changesets from the current state (cheap for recent history) or replay forward from a sparse checkpoint.
  - Keep sparse, logarithmically spaced pinned roots (hourly, daily, weekly) as checkpoints. They cost path-copy pages only for what changed since the previous checkpoint.
- **GC and compaction:**
  - Squash old ranges of the op log into checkpoint snapshots after a retention window, keeping commit headers for lineage (the TerminusDB metadata-repository idea [S33]).
  - Reuse free pages in the B+tree. There is no mark-sweep over a chunk universe.
- **Optional Merkle digest:** every K commits, compute an MST [S47] over `node_id → hash(record)` for verification and efficient cross-machine sync.

### Architecture B: Merkle state store ("Dolt in Rust")

- Content-addressed chunk store: an append-only journal plus a fanout index.
- Prolly trees or MSTs for `nodes`, `out`, `in` and each secondary index.
- A commit is a root value (a map of tree roots) plus parents. Branches are refs.
- Diff is a tree comparison, O(diff). Merge is a 3-way merge of trees at key/cell granularity, followed by the same validators as A.
- History means opening any root, O(depth), with no replay.
- GC is generational mark-sweep, as in Dolt [S16].
- To make it viable for agents, batch writes into a *working set* and create commits only at session or task boundaries, which is Dolt's own model. Squash old commits aggressively.

### Architecture C: git-native

- **C1: objects in the project's git ODB.** Nodes are blobs (canonical CBOR/JSON) in fan-out trees. Moirai commits are git commits on `refs/moirai/<branch>`, written via `gix` (gitoxide 0.88.0; it supports loose and pack writing, ref transactions and read/write reftable [S10]). A derived query index (own B+tree) is rebuilt from git, Fossil-style [S59]. Merges are done by moirai, not `git merge`.
- **C2: files in the working tree.** One sorted JSONL (or one file per node) is committed with the code, and a custom merge driver plus a derived cache DB sit on top. This is beads classic / beads_rust [S30].

### Architecture D: CRDT core

- An op-based CRDT graph:
  - per-field LWW registers with HLC, keeping losers
  - add-wins sets for edges and labels
  - Kleppmann tree for the hierarchy
  - a text CRDT, or line-diff LWW, for bodies
- A version DAG of changes with frontiers/version vectors; branches are forks (Loro-style [S66]). Merge is a union of ops that never fails.
- The invariant layer (cycles, dangling edges) is computed at read time. Offending edges are hidden per Weidner #5 [S68], and violation records are emitted.

### Trade-off table

Ratings run from 1 (poor) to 5 (best), with the rationale in each cell. All ratings are [DERIVED].

| Criterion | **A. Op-log + CoW state + overlays** | **B. Merkle state (prolly/MST)** | **C1. Git ODB objects** | **C2. Working-tree files** | **D. CRDT core** |
|---|---|---|---|---|---|
| Write perf (tiny commits) | **5**: one append plus in-place CoW update; group fsync | 2: ≥4 KB × depth × trees per commit [S11]; ~4× SQLite on autocommit [S24] | 1: several loose files per commit plus tree path rewrite; gc churn | 2: rewrite file(s) and a git commit per change | 4: append op |
| Read perf (current state) | **5**: B+tree point and index lookups, no layering | 4: B-tree-like, content-addressed chunk lookups | 2: needs a derived index; cold start rebuilds | 3: derived cache DB | 4: in-memory |
| Old-version queries | 3: per-node O(k); full-graph via reverse/forward replay (bounded by checkpoints) | **5**: any version O(depth) | 4: trees are snapshots, but index rebuild needed | 3: `git show` plus parse | 3: checkout replay |
| RAM | **5**: mmap hot set; overlays ∝ branch changes | 3: chunk index ~1% of store [S17] plus caches | 4: small, but derived index | 4 | 2: whole doc resident |
| Disk (1e6 tiny commits) | **5**: ~0.3–0.6 GB | 1: ~40–55 GB before squashing [S26] | 3: small after repack, large before | 3: packed deltas fine | 4 |
| Build-from-scratch complexity | 3 (M–L): segments plus index, CoW B+tree, overlays, merge engine, validators. B+tree robustness is the long pole ("95% … is testing" [S63]) | 2 (L–XL): chunker, trees, cursors, 3-way tree merge, chunk store, GC. DoltLite is ~18k LOC of C and 2,000 PRs to reach beta [S23, S25] | 3 (M): storage from gix, but a derived index, custom merge and perf work are still needed | **5 (S)** | 1 (XL): CRDT engine plus invariant layer; mature libs took years |
| Merge quality (graph-aware) | **5**: intent-level ops, typed 3-way, validators, first-class conflicts | 4: cell 3-way plus validators, but state diff loses op intent (e.g. "append to note" vs "replace") | 4: moirai-owned merge over trees | 2: line merges of JSON; driver not versioned [S6] | 3: never fails, but LWW hides conflicts; needs an extra layer |
| Fit with the real git repo | 4: common-dir store; SHA links; optional `refs/moirai` export | 4: same; Dolt precedent `refs/dolt/data` [S21] | **5**: lives in the repo; push via refspec | 5, but diverges per worktree | 3: doc blob in git |
| Multi-agent concurrency | 4: single-writer lock with short txns, or a jj-style lock-free op-heads variant | 3: same, plus GC coordination | 2: ref lock contention; `index.lock`/ref locks | 1: agents in different worktrees do not see each other | 4: merge anything |
| Windows risk | Low–Med (own mmap/locking code) | Med (large GC memory; many chunks) | **High** (many small files, Defender, gc) | Med | Low |

### Hybrid recommendation

Use **A as the core** and borrow from the others:
- From **B**: MST checkpoint digests for verification and machine-to-machine sync. Add it later.
- From **C1**: an export/import bridge to `refs/moirai/data` for pushing moirai history through the project's git remote. Add it later.
- From **D**: HLC tie-breaks, add-wins sets, Kleppmann moves and fractional ordering as *merge rules inside A*, not as the storage model.

### 9.3 Branching model aligned with the agent workflow (proposal)

| moirai branch | Maps to | Visibility | Used for |
|---|---|---|---|
| `trunk` | the project, regardless of git branch | immediate for all agents (common dir) | **coordination state**: task status, claims, blockers, assignments; project-wide rules |
| `wt/<worktree-or-git-branch>` (auto-created) | a git worktree/branch | that agent, plus anyone who asks | **speculative knowledge**: findings, decisions, and notes tied to code on that branch. Merged into `trunk` when the git branch merges, via a post-merge hook or orchestrator step. Discarded or archived if the git branch is abandoned. |
| `exp/<name>` | manual | explicit | "what-if" re-plans (e.g. an orchestrator restructuring the task tree) |

With this model, "maximally synchronous" reference consistency holds *within a branch* at write time and *across branches* at merge time. That is the inherent trade-off of branching (§12, Q2).

---

## 10. What to prototype and benchmark first (before committing to A)

1. **Tiny-commit microbenchmark on the owner's Windows 11 NVMe.**
   - Workload: append a 400 B commit and fsync.
   - Compare fsync per commit against group commit at 1, 5 and 20 ms windows.
   - Also time git loose-object writes via gix on the same disk.
   - This decides the durability policy.
2. **CoW B+tree versus in-place B+tree with WAL** for the materialized state at 1e5 and 1e6 nodes: write amplification, RSS, and file growth with a long-lived reader.
3. **Merge engine on synthetic agent branches:** 10 branches × 1,000 ops, including the conflict classes C1–C5 and randomly generated blocker cycles. Measure merge time and conflict record counts.
4. **Old-version query cost:** per-node chain walk versus full-graph reverse-apply over 1e4, 1e5 and 1e6 ops, to choose checkpoint spacing.
5. **Control experiment:** load the same workload into DoltLite or Dolt, to confirm the ~50 KB-per-commit figure for moirai's schema.

---

## 11. Risks

- **Storage engine correctness.** CoW B+trees and crash safety take far more effort to test than to write [S63]. Budget for fault injection (kill mid-write, torn pages) on Windows.
- **Merge semantics creep.** Every new field type needs a merge rule and validators. Keep the type system small: bool, int, enum-with-lattice, text, set, ref, ref-list.
- **Git SHA links rot** after rebase or squash. Links are provenance only.
- **Git 3.0** changes defaults (SHA-256, reftable) for *new* repos [S8]. Any L3 or C1 integration must handle both object formats.
- **Maturity of Rust building blocks in 2026:**
  - redb multi-process access is still experimental (4.3, September 2026) [S73].
  - Sanakirja 2.0 is beta [crates.io].
  - Pijul is beta [crates.io].
  - The prolly-tree crates are young [crates.io].
  - This supports the owner's "from scratch" instinct, but it means no shortcut exists for the hard part.

---

## 12. Open questions only the owner can answer

1. **Branch semantics.** Should agents' *task and coordination* writes be isolated per branch at all? My proposal: no. Coordination lives on a shared trunk, and only knowledge and notes are branch-scoped. Or does the owner want full isolation, where every agent works on a moirai branch mirrored to its git worktree?
2. **Cross-branch visibility of deletes.** Should a node deleted on one branch be visible as deleted to other branches before merge? The brief's "maximally synchronous" wording suggests yes, but that contradicts branch isolation.
3. **ID format.** Are human-friendly sequential integers ("node 40") required?
   - If yes: is a single store per machine (common dir, ids allocated under a lock) acceptable, with collisions possible only across machines or clones?
   - Or are beads/jj-style short hash ids (e.g. `m-7f3a`) acceptable?
4. **Commit granularity.** Should every agent mutation be a moirai commit (jj-style auto-snapshot), or only session/task boundaries, with the op log keeping fine-grained undo? This changes history size by up to about 10–100×.
5. **Merge policy default.** Should merges *never* block and record conflicts (jj style), or must some conflict classes (e.g. blocker cycles, delete-vs-modify on tasks) make the merge fail until resolved?
6. **Where must data survive?** Is losing `.git/` (and so the common-dir store) acceptable, given that the code history would be lost too? Or must moirai data be pushed to the git remote (L3 mirror) or exported to tracked files (L1) from day one?
7. **Multi-machine or cloud agents.** Will agents on other machines or in cloud sessions write to the same moirai? If yes, sync (MST digests or a refs mirror) and globally unique ids become day-one requirements.
8. **Retention.** How long must full-fidelity history be kept (all ops forever, or squash after N days)? Is semantic "memory decay" (beads-style summaries of closed tasks [S27]) in scope for the storage layer or a separate layer?
9. **Process model.** Will there be a long-running daemon (the MCP server) that owns the DB, with the CLI talking to it? Or must each CLI invocation open the store directly with multi-process locking? This changes the concurrency design (single-writer lock versus jj-style lock-free op heads).
10. **Status semantics.** Is a status lattice for automatic merge (e.g. `done` beats `in_progress`) acceptable, or must every concurrent status change surface as a conflict?
11. **Dependencies.** Does "from scratch" allow small, well-tested crates (e.g. `blake3`, `zstd`, `gix` for the git link), or must the storage layer have zero storage-engine dependencies?

---

## 13. Method and limitations

- Sources are primary wherever possible: official docs and specs, repositories and changelogs, maintainer blogs, and papers. Versions and dates were checked against crates.io, GitHub releases and changelogs on 2026-09-25.
- **The session's web-search budget (200 calls) was exhausted partway through.** These items could not be re-verified:
  - The Sanakirja benchmark numbers, because the Pijul blog is a client-rendered SPA.
  - Datomic per-datom storage sizes.
  - Ink & Switch "Patchwork" (git-like branching over Automerge).
  - NTFS small-file and Defender costs.
  - `core.fsync` defaults.
- Several "measured" numbers come from vendors: DoltHub, the Automerge team, Loro, the redb author and Fossil. The DoltLite GC figures come from a user-filed issue [S26]. None were reproduced here. §10 lists the benchmarks that should be run on the owner's hardware.

---

## 14. Sources (accessed 2026-09-25)

**Git**
- [S1] git-gc docs: https://git-scm.com/docs/git-gc
- [S2] Pack format (gitformat-pack): https://git-scm.com/docs/gitformat-pack
- [S3] Reftable spec, with Android/Rails measurements: https://git-scm.com/docs/reftable
- [S4] git-worktree, ref sharing rules: https://git-scm.com/docs/git-worktree
- [S5] commit-graph: https://git-scm.com/docs/commit-graph
- [S6] gitattributes, custom merge driver: https://git-scm.com/docs/gitattributes#_defining_a_custom_merge_driver
- [S7] Pro Git, Packfiles: https://git-scm.com/book/en/v2/Git-Internals-Packfiles
- [S8] LWN, "Looking forward to Git 2.56 — and 3.0" (Sept 2026): https://lwn.net/SubscriberLink/1094575/2385e98583715c2b/
- [S9] libgit2 PR #4030 fsync benchmark (2017): https://github.com/libgit2/libgit2/pull/4030
- [S10] gitoxide crate status: https://github.com/GitoxideLabs/gitoxide/blob/main/crate-status.md (gix 0.88.0, 2026-09-25, crates.io)

**Dolt / DoltLite / Beads**
- [S11] Dolt prolly tree docs: https://www.dolthub.com/docs/architecture/storage-engine/prolly-tree
- [S12] Dolt block store docs: https://www.dolthub.com/docs/architecture/storage-engine/block-store
- [S13] Dolt Archives status update (2025-04-11): https://www.dolthub.com/blog/2025-04-11-archive-update/
- [S14] Dolt 1.75, AutoGC and Archives by default (2025-10-20): https://www.dolthub.com/blog/2025-10-20-dolt-1-75/
- [S15] Automatic GC in sql-server (2025-02-28): https://www.dolthub.com/blog/2025-02-28-announcing-automatic-gc-in-sql-server/
- [S16] Incremental GC, v1.86.6 (2026-04-28): https://www.dolthub.com/blog/2026-04-28-introducing-incremental-garbage-collection/
- [S17] Storage-layer memory optimizations (2022-02-28): https://www.dolthub.com/blog/2022-02-28-dolt-storage-layer-memory-optimizations/
- [S18] Journaling chunk store (2023-03-08): https://www.dolthub.com/blog/2023-03-08-dolt-chunk-journal/
- [S19] A study in structural sharing (2024-04-12): https://www.dolthub.com/blog/2024-04-12-study-in-structural-sharing/
- [S20] Dolt merges, conflicts and constraint violations: https://www.dolthub.com/docs/sql-reference/version-control/merges/
- [S21] Dolt remotes, including git remotes under `refs/dolt/data`: https://www.dolthub.com/docs/concepts/dolt/git/remotes
- [S22] Merging branches with foreign keys (2021-07-20): https://www.dolthub.com/blog/2021-07-20-merging-branches-with-foreign-keys/
- [S23] Introducing DoltLite (2026-03-25): https://www.dolthub.com/blog/2026-03-25-doltlite/
- [S24] How fast is DoltLite? (2026-06-08): https://www.dolthub.com/blog/2026-06-08-how-fast-is-doltlite/
- [S25] DoltLite Beta, v0.50.0 (2026-08-31): https://www.dolthub.com/blog/2026-08-31-doltlite-beta/
- [S26] DoltLite issue #2936, GC at 1.06M / 3.9M commits: https://github.com/dolthub/doltlite/issues/2936
- [S27] Beads README: https://github.com/gastownhall/beads (formerly steveyegge/beads); CHANGELOG 1.3.0 (2026-09-15): https://github.com/steveyegge/beads/blob/main/CHANGELOG.md
- [S28] Restoring Beads Classic (2026-04-02): https://www.dolthub.com/blog/2026-04-02-restoring-beads-classic/
- [S29] A Day in Gas Town (2026-01-15): https://www.dolthub.com/blog/2026-01-15-a-day-in-gas-town/
- [S30] beads_rust: https://github.com/Dicklesworthstone/beads_rust

**In-repo trackers**
- [S31] Andrew Nesbitt, "Issues in the Repo" (2026-08-20): https://nesbitt.io/2026/08/20/issues-in-the-repo.html
- [S32] git-bug data model: https://github.com/git-bug/git-bug/blob/master/doc/design/data-model.md

**TerminusDB**
- [S33] van Otterdijk, Mendel-Gleason, Feeney, "Succinct Data Structures and Delta Encoding for Modern Databases" (2020): https://assets.terminusdb.com/research/succinct-data-structures-and-delta-encoding.pdf
- [S34] What is TerminusDB: https://terminusdb.org/docs/terminusdb-explanation/
- [S35] TerminusDB 12 release (2025-12-08): https://terminusdb.org/blog/2025-12-08-terminusdb-12-release/ ; releases: https://github.com/terminusdb/terminusdb/releases
- [S36] terminusdb-store (Rust): https://github.com/terminusdb/terminusdb-store
- [S37] TerminusDB JSON diff and patch: https://terminusdb.org/docs/json-diff-and-patch/

**Datomic / XTDB**
- [S38] Datomic index model: https://docs.datomic.com/indexes/index-model.html
- [S39] Datomic filters (as-of/since/history/with): https://docs.datomic.com/reference/filters.html
- [S40] "Datomic is Free" (2023-04): https://blog.datomic.com/2023/04/datomic-is-free.html
- [S41] Launching XTDB v2: https://xtdb.com/blog/launching-xtdb-v2 ; v2.0.0: https://github.com/xtdb/xtdb/releases/tag/v2.0.0

**Irmin**
- [S42] Irmin: https://github.com/mirage/irmin ; CHANGES (3.11.0, 2025-06-19): https://github.com/mirage/irmin/blob/main/CHANGES.md
- [S43] Tarides, irmin-pack GC for Tezos (2022-11-10): https://tarides.com/blog/2022-11-10-towards-minimal-disk-usage-for-tezos-bakers/
- [S44] Irmin.Merge API: https://mirage.github.io/irmin/irmin/Irmin/Merge/index.html

**Index structures**
- [S45] Noms (archived 2021-08-28): https://github.com/attic-labs/noms
- [S46] Auvolat and Taïani, Merkle Search Trees (SRDS 2019): https://ftaiani.ouvaton.org/PUBLI/PEER_REV_CONF/2019_auvolat_hal-02303490.html
- [S47] atproto repository spec (MST): https://atproto.com/specs/repository
- [S48] IPLD HAMT spec: https://ipld.io/specs/advanced-data-layouts/hamt/spec/
- [S49] crates.io: prollytree 0.4.1, prolly-map 0.7.2, merkle-search-tree 0.8.0 (queried 2026-09-25)

**Jujutsu**
- [S50] Concurrency: https://github.com/jj-vcs/jj/blob/main/docs/technical/concurrency.md
- [S51] Operation log: https://github.com/jj-vcs/jj/blob/main/docs/operation-log.md
- [S52] Conflicts (technical): https://docs.jj-vcs.dev/latest/technical/conflicts/
- [S53] Working copy: https://docs.jj-vcs.dev/latest/working-copy/
- [S54] Architecture: https://docs.jj-vcs.dev/latest/technical/architecture/
- [S55] Git compatibility: https://docs.jj-vcs.dev/latest/git-compatibility/
- [S56] Glossary: https://docs.jj-vcs.dev/latest/glossary/
- [S57] Releases: https://github.com/jj-vcs/jj/releases (jj-lib 0.45.1, 2026-09-03, crates.io)

**Fossil**
- [S58] File format: https://fossil-scm.org/home/doc/trunk/www/fileformat.wiki
- [S59] Technical overview: https://fossil-scm.org/home/doc/trunk/www/tech_overview.wiki
- [S60] Tickets: https://fossil-scm.org/home/doc/trunk/www/tickets.wiki
- [S61] Delta format: https://fossil-scm.org/home/doc/trunk/www/delta_format.wiki

**Pijul / Sanakirja**
- [S62] Pijul theory: https://pijul.org/manual/theory.html (pijul 1.0.0-beta.24, 2026-09-14, crates.io)
- [S63] Sanakirja docs: https://docs.rs/sanakirja ; "Rethinking Sanakirja" (2021-02-06): https://pijul.org/posts/2021-02-06-rethinking-sanakirja/ (not retrievable, SPA) ; discussion: https://lobste.rs/s/y556cm/rethinking_sanakirja_rust_database (sanakirja 1.4.3 / 2.0.0-beta.3, 2026-07-06)

**CRDTs**
- [S64] Automerge 3.0 (2025): https://automerge.org/blog/automerge-3/ (automerge crate 0.12.0, 2026-09-16)
- [S65] Automerge conflicts: https://automerge.org/docs/reference/documents/conflicts/
- [S66] Loro 1.0: https://loro.dev/blog/v1.0 ; movable tree: https://loro.dev/blog/movable-tree ; releases: https://github.com/loro-dev/loro/releases (loro 1.16.2, 2026-09-21)
- [S67] Kleppmann, Mulligan, Gomes, Beresford, "A highly-available move operation for replicated trees" (IEEE TPDS 2021): https://martin.kleppmann.com/papers/move-op.pdf
- [S68] Matthew Weidner, CRDT Survey Part 2 (2023): https://mattweidner.com/2023/09/26/crdt-survey-2.html
- [S69] Yjs Y.Doc, gc option: https://docs.yjs.dev/api/y.doc
- [S70] Sanjuán et al., Merkle-CRDTs (arXiv:2004.00107, 2020): https://arxiv.org/pdf/2004.00107

**Event sourcing / SQLite**
- [S71] Martin Fowler, Event Sourcing: https://martinfowler.com/eaaDev/EventSourcing.html
- [S72] SQLite session extension: https://www.sqlite.org/sessionintro.html ; conflict types: https://www.sqlite.org/session/c_changeset_conflict.html ; rebaser: https://www.sqlite.org/session/rebaser.html

**CoW B+trees**
- [S73] redb design: https://github.com/cberner/redb/blob/master/docs/design.md ; README benchmarks: https://github.com/cberner/redb ; CHANGELOG (4.0–4.3, 2026): https://github.com/cberner/redb/blob/master/CHANGELOG.md ; PR #1462: https://github.com/cberner/redb/pull/1462
- [S74] LMDB on Windows pre-sizes the file: https://github.com/Venemo/node-lmdb/issues/159 , https://lmdb.readthedocs.io/ ; Symas, "Understanding LMDB database file sizes" (2016): https://www.symas.com/post/understanding-lmdb-database-file-sizes-and-memory-utilization

**RDF versioning**
- [S75] OSTRICH / BEAR (IC/CB/TB): https://rdfostrich.github.io/article-demo/ ; Taelman et al.: https://doi.org/10.2139/ssrn.3248501
- [S76] Quit Store: https://github.com/AKSW/QuitStore ; "Decentralized Evolution and Consolidation of RDF Graphs": https://arxiv.org/pdf/1902.10703
