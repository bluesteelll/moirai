# 13 — Proposal D: "Branches + git image" moirai

*Architecture proposal for moirai. Date: 2026-09-25. Status: design only; nothing implemented; this file is the only artifact written. Citations `[NN §name]` point into reports 01–08 under `docs/research/`; `[A]`, `[B]`, `[C]` are proposals 10–12 and `[20]`, `[21]`, `[22]` the three critiques. Numbers are tagged **[M]** (measured on the owner's machine in the cited report), **claimed**, or **est.** (my arithmetic, inputs shown). Web sources fetched today are listed in §12.*

*This proposal implements the owner's 2026-09-25 update (R1 full branching, R2 git independence, R3 git-compatible image). Where it disagrees with [A]/[B]/[C] or with the reports' "shared trunk" recommendation, the owner update wins and the disagreement is stated.*

---

## 1. Thesis and angle

1. **moirai is its own version-control system for a typed graph**: a BLAKE3 content-addressed commit DAG whose commits are typed changesets with before-images, plus refs (branches, tags, staging refs), a per-client HEAD, a reflog/op log with undo, and a 3-way typed merge engine with conflicts-as-data. **Every versioned datum branches** — tasks, subtasks, `status`/`done`, blockers, rules, notes, decisions, findings, verdicts, measurements, schema. Only runtime coordination state (leases with fencing tokens, idempotency results, change-feed cursors, the `#N` allocator, HEAD bindings) is store-level, and §5d specifies exactly how it meets branches.
2. **A branch is cheap on the chosen engine** (log + immutable mmap'd segments + overlay, [A §4]/[B §4]): it is a ref plus an overlay = *pinned checkpoint segment set* ⊕ *trunk ops from that checkpoint to the fork commit* ⊕ *the branch's own ops*. Creation is one 70-byte record; reading costs O(bounded overlay); ~50 live branches cost a few pinned segment files and no resident RAM until read. Long-lived branches are promoted to their own delta segment.
3. **Zero git in the core.** The store is `.moirai/` found by `MOIRAI_DIR`, a `--store` flag, or walking up for a `.moirai` directory or pointer file (git's `.git`-file idea without git); `<git-common-dir>` is a discovery *hint* read from files, never a requirement; moirai branches are independent of git branches and worktrees, with optional explicit binding (`moirai worktree bind`).
4. **The git image is an exact, deterministic serialization** of moirai's commit DAG into git objects: one readable `.moi` text file per node (edges in the source node's file, tombstones as files, conflicts as data lines), a two-level fan-out tree, one git commit per moirai commit (or per checkpoint), trailers carrying the moirai commit id and metadata, refs under `refs/moirai/*` (or an orphan branch, a tracked directory, or a separate repo). A hand-written loose/pack/bundle writer and pack reader make export and import work with no git installed; `git` is only the network transport. Import handles foreign git-side commits, merges and hand edits as first-class "foreign" commits validated by the same merge engine.
5. **Angle optimised for:** R1–R3 taken literally, while keeping [A]'s RAM/latency budget, [B]'s versioning rigour (I12: structural violations never reach a branch head), [C]'s agent surface, and the critiques' fixes (X1–X13, F-A1–F-C8, §2.1–§2.9 of [22]) as defaults rather than options.

---

## 2. Positions on the forks

Sections 3, 4, 7 and 8 are deltas against [A]/[B]/[C]; the positions below say which base is reused.

| Fork | Position | Evidence | What would change my mind |
|---|---|---|---|
| **T1** materialized state | **[B §4.1–4.4] segments + overlay, with the [20]/[21] fixes as spec, not options**: LSN + store epoch in every record header; readers never read past `committed_lsn`; a writer scans to the physical end and republishes before appending (X2/F-A1); delta segments from v1; checkpoints under the maintenance byte, writer byte only for publish (X9/G9); bodies inline in the unmapped tail, separate byte threshold (G7); commit index per sealed log (G3). Branch state = pinned checkpoint set ⊕ overlay (§5a.3). | Windows forbids resizing mapped files and mapped views are not coherent with `WriteFile` [05 §6.1–6.2]; path-copy state costs 12–16 KiB per retained version vs ~0.3–0.6 KB for changesets [04 §3.14, §6]; the critique traces that break A/B/C are all in the edges of this skeleton [20 §0], [21 §1]. | Measured overlay read > 50 µs after a full 4k-op tail at 1e6, or branch overlay builds > 20 ms at the owner's real lane sizes; then a CoW B+tree with Sanakirja-style refcounted forks for branches [04 §3.10]. |
| **T2** process model | **[A]'s purely embedded protocol is the contract**, blocking `LockFileEx` wait on an overlapped handle (G1), no sleep-backoff; the session MCP server is an opportunistic leader only in M6 and only with auto-keyed forwarding (G13). Zero idle CPU by construction. | Convoy under 16 writers with sleep-backoff [20 §1.5]; daemons are the recurring failure [08 §5]; pipe bugs in Claude Code's own daemon [08 §4 W12]. | Writer-wait p99 > 50 ms with 16 writers *after* G1, or a harness push channel that reaches the model [07 §2.3]. |
| **T3 → T3′** branch model | **Full branching (R1)**, replacing [B]'s two planes and [A]/[C]'s shared trunk: `main` is just a branch; `lane/<name>`, `plan/<name>` (coordination status read-only), `exp/<name>` are branch *kinds*, not planes. Isolation is git-like: a branch sees its fork state ⊕ its own commits. Cross-lane liveness comes from **cross-branch read views** (`--across`), store-level leases (§5d) and `moirai sync` (merge `main` into the lane), not from a shared trunk. | Owner update (R1) overrides [04 §9.3], [08 §7.2], [A T3], [C T3]; the register incidents [02 §7.3] are cured by typed merge, which full branching still has; [22 §3.2] issues 1–3 (verdicts invisible to trunk-side `ready`) are answered by `--across` views and by lane-scoped `ready` being the *intended* semantics under R1. | If one campaign shows agents never run `sync` and rules written on `main` never reach lanes in time, add an opt-in `shared:` field class (owner-authority rules and leases already are store-visible) — never a whole shared plane. |
| **T4** IDs | **Both**: store-global never-reused `#N` (u32, allocated under the writer byte, not versioned) for display and dense indexing; 128-bit random `uid` from day one as the identity in canonical commit hashes and in the git image; `#N` is an alias that the image records but never trusts. | Single allocator makes `#N` collision-free across branches of one store [06 §9.2]; export/import needs store-independent identity [04 §3.1]; UUIDs cost ~24 tokens and 5–10× more Haiku errors [06 §9.1]. | Nothing on this angle; `#N`-only would make R3 round-trip impossible. |
| **T5** deletion | Hard delete in current state + tombstone row + full before-image; per-edge-kind policies as [B §3.4] with X4: `--replaced-by` re-points structural edges, otherwise a deleted blocker leaves a *flagged* structural edge that keeps the dependent out of `ready` until `resolve`; `rm` refuses while a live lease exists unless `--release`; tombstones are exported as files so "deleted" ≠ "absent" survives git merges. Cross-branch: §5d. | [21 X4] (A's walkthrough hands #12 to a developer whose prerequisite is open); Beads' import "cannot infer that records absent … were deleted" [08 §6.3]; [06 §8.3]. | Owner rules "a deleted blocker always means no longer required" → drop-and-notify. |
| **T6** bodies | [B T6]: ≤ 64 KiB inline, content-addressed (BLAKE3-16), zstd-dictionary, deduplicated across revisions and branches; larger as `artifact` pointers. In the image a body is the tail of the node's `.moi` file (text, diffable). | diff3 and the removed-text guard need text in-store [01 §2.1]; dedup matters more with branches (every lane re-submits sections). | Owner wants prose only in the repo → pointer-only, text merge rules dropped. |
| **T7** merge | [B §5.4] typed 3-way + [21 §7] invariants: structural violations (`Cycle`, `DanglingEdge`, `HierarchyCycle`, `IdCollision`, `SchemaConflict`, `ImageParse`) never advance a ref — staged on `refs/merge/<name>`; value conflicts (field, text, `StatusFork`, `DeleteVsModify`) land as conflict values (jj) unless `--strict`; incomparable lattice moves are conflicts; counters are `Incr` ops; `rev` after merge > both sides; citations pin commit ids; `supersedes` ≤ 1 active; `plan/*` branches cannot change coordination status. | jj conflicts as values [04 §3.8]; Dolt refuses commits with violations [04 §3.2]; PK's precondition needs an acyclic head [21 X10]; owner's 188-vs-191 counter incident [02 §7.3]. | Owner prefers "always land": acceptable only with `topo`-free fallbacks for flagged components. |
| **T8** durability | One `NtFlushBuffersFileEx(DATA_SYNC_ONLY)` per durable commit, HEAD unflushed (1PC+C), lazy class for heartbeats/cursors, fsync failure fatal, zero-filled log extents (G11). Ref updates and client-HEAD moves are durable. | 1.73–2.0 ms p50 [M, 05 §2.2]; write-through untrusted [05 §6.2]; ATC'20 [08 §8.1]. | Sustained flush p99 > 10 ms under build load → pipelined group commit in the M6 leader. |
| **T9** agent surface | [C §7] pack algorithm and hooks + [B §7.1] CLI + [22 §9] fixes (run-scoped dispatcher leases, role label from the dispatch marker, `--ids` header-free, `blocking` = tasks only, chars budgets, global rules count, payload-bound idempotency). New: every verb takes `--branch`; `pack`/`brief` take `--across`; branch verbs are CLI-only orchestrator rituals. | [07 §5.4] three roles without Bash; [22 §2.1–§2.6]. | Hooks proven not to fire for Workflow `agent()` → dispatcher-only; unchanged otherwise. |
| **T10** from scratch | Hand-written: formats, lock/HEAD protocol, segments, overlay, PK, merge engine, diff3, the git image writer/reader (loose objects, pack v2 + idx, bundle v2, tree/commit encoders, ref files, packed-refs, delta application). Leaf crates: `zerocopy`, `blake3`, `xxhash-rust`, `zstd`, `windows-sys`/`libc`, `serde_json`; image module only: `sha1`, `sha2`, `miniz_oxide` (pure-Rust zlib). `rmcp`+`tokio` (`current_thread`) only in `moirai mcp`. **Not** `gix`/`git2` in the product (gix has pack writing and ref transactions but no push, and SHA-256 is listed as parity work still to do; see §12 S5). | gix crate-status [S5]; SHA-256 at 1.94 GB/s here [M, 05 §2.5]; [04 §11] maturity. | Owner allows `gix` as an optional import accelerator for exotic packs (multi-pack-index, bitmaps). |
| **T11** search | [A T11] tiers; results carry `seq` and branch. | [05 §13]; [03 §6.3]. | — |
| **T12** schema | Fixed core (13 kinds of [A §3.2] + [C]'s `phase_state`/`return_to` fields and `gates` edge) + schema-as-data extensions **versioned like everything else**: `schema/*.moi` in the image; weakening changes merge, strengthening changes need a migration commit re-validated at merge (`SchemaConflict`). | [06 §4]; [22 §3.3] against 28 kinds. | — |
| **T13** v1 scope | Branches are core, so M1 ships refs/branches/checkout/log/diff/undo with the engine; CLI + pack + hooks (M2) adopt on `main`; merge/rebase (M3) and the git image (M4) before MCP (M5). Oracle backend first (S0 of [22 §7.3]) so adoption is not hostage to the engine. | Owner update; [22 §2.7]. | Owner's first target is publishing to git → M4 export moves ahead of M3. |
| **T14** git independence | `.moirai/` discovery chain (§5c); `.moirai` pointer files for any directory; `moirai worktree bind`; a 60-line reader for `.git`/`commondir`/`HEAD`/`packed-refs` used only for *provenance* and as a discovery hint; no `git` process ever spawned by the core; network transport optional via `git` CLI. | R2; Beads phantom-DB in worktrees [03 §2.7]; sandboxed agents may not run git [08 §2]. | — |
| **T15** git image | `.moi` text per node, edges in the source file, tombstones kept, 2×2-hex fan-out on `uid`, one git commit per moirai commit (`--granularity commit`) or per export run (`--granularity checkpoint`, default for project repos), trailers with `Moirai-Commit`, deterministic author/committer/timestamps from commit metadata, own writer (loose for small increments, pack for bulk, bundle for transport), own reader (loose + pack with deltas), id map stored in-store (`gitmap`) and reconstructible from trailers; SHA-1 or SHA-256 follows the destination repo. | Precedents §12: jj keeps commits as git objects but "commits with conflicts cannot be represented in Git" and stores change ids in non-standard headers [S1][S2]; Dolt stores table files as blobs under `refs/dolt/data` with `--force-with-lease` CAS [S6]; git-bug derives entity ids from the first op and orders by Lamport clock [S7]; Fossil keeps a marks-like `.mirror_state` for incremental export [S10]; hg-git/cinnabar keep explicit map files/trees [S8][S9]. | If the owner wants the image reviewed in PRs above all, make the tracked-directory destination the default and checkpoint granularity mandatory there. |
| **T16** coordination under branching | Versioned per branch: all graph data. Store-level runtime: leases (keyed by `uid`, carrying the branch they were taken on), fencing tokens, idempotency results (payload- and branch-bound), change-feed cursors, `next_id`, HEAD bindings, quiet flag, `gitmap`, ancestry cache. Rules in §5d.1. | R1 says status is versioned; a lease is "who is working now", not history; two lanes must not both build #12 [07 §7.4]. | Owner wants leases branch-scoped (parallel what-ifs claiming the same task) → key leases by (uid, branch) with a cross-branch warning. |

---

## 3. Data model (delta against [B §3] and [A §3.2])

**Base:** [B §3.2] header, [B §3.3] kinds and typed fields, [B §3.4] edge kinds, [B §3.6] derived state, with [A §3.6] status machines. Changes:

| Δ | Change | Why |
|---|---|---|
| D1 | **No planes.** The `plane` byte and P1–P3 are removed. Every kind lives on every branch. | R1. |
| D2 | **Branch kinds** on the ref, not on nodes: `work` (default; `main` and `lane/*`), `plan` (coordination fields `status`, `resolution`, `assignee`, and `blocks` edges are read-only; claims refused), `exp` (alias of `plan`), `merge/*` (staging), `tags/*`. | [21 X13]: a what-if branch must not be able to mark work done. |
| D3 | **Header** (56 B, [B §3.2]) drops `plane` and `proposed`; adds `conflicted` flag; `rev u32` becomes `rev_seq u64` = store seq of the commit that last touched the node on *this* branch view, so `rev` after a merge is always above both inputs. CAS guards use `--if-rev <seq>`. | [21 X11]. |
| D4 | **Counters are `Incr` ops** (`note.incidents`, `task.reopen_count`). `verdict.counts` is not stored; it is the query `stats loop`. | [21 X11]; the 188-vs-191 incident [02 §7.3]. |
| D5 | **`gates` edge** (verdict → task, structural, checked for acyclicity together with `blocks`): constrains *completion* (`complete` exits 6), never `claim`; `blocks` constrains start. `task.phase_state` (the 14 states of [01 §5.1]) and `verdict.return_to` from [C §3.2]. | [21 X5]. |
| D6 | **I5′**: acyclicity holds on `blocks ∪ child→parent ∪ {X→D : blocks(X,P), X ∉ subtree(P), D ∈ subtree(P)}`; `link X --blocks P` on a container checks every descendant; `move` re-derives the exogenous classification for edges touching the moved subtree. | [21 X1, X2b]. |
| D7 | **Leases are runtime records, not nodes** (`lease {uid, holder, token u64, expires, run, pid, branch_sym}`); `rm` and `--cascade` refuse while a live lease covers a node in scope unless `--release`. `SubagentStop` stores its triage note even when the task is gone (linked by `uid`, rendered through the tombstone). | [21 S1]; [22 §3.3 issue 6] (leases must not consume `#N`). |
| D8 | **`mentions`** are parsed only for `#N < next_id` and only with the sigil rule of [22 §2.4] (`#N` not preceded by an alphanumeric, not followed by `/` or `.digit`); rendered as "text mention" in impact reports. | [21 X6]; [22 §2.4]. |
| D9 | **`suspect` is purely derived** from `(target state, edge.pinned_commit)`; "re-confirm" re-pins the edge (an op with a before-image). As-of output carries no derived fields unless `--recompute`. | [21 X8, S10]. |
| D10 | **`answered`** on a question is derived from an `answers` edge visible on the reading branch. | [21 S2-B]. |
| D11 | `supersedes`: ≤ 1 active superseder per target (structural cardinality); a second is a write-time conflict on the same branch and a `SupersedeFork` conflict at merge. | [21 X12]. |
| D12 | Every node stores `uid u128` (random at create, never changed) in a cold column; the image and canonical hashes use only `uid`. | T4. |
| D13 | Schema is data on every branch: `kinds`, `fields` (name, type, lattice order), `edges` (class, delete policy, acyclic, cardinality) as `schema` nodes; a strengthening change is a `Schema{strengthen}` op that needs `moirai migrate` on the branch and re-validation at merge. Enum integers are never reused; symbols are never GC'd. | T12; [21 S10]. |

Invariants: [B §3.5] I1–I12 minus P1–P3, plus [21 §7] I5′ and I13′–I24′ (I23′ applies to `plan/*` branches). Derived state: [B §3.6] plus `conflicted` (an unresolved conflict value on this branch) and `diverged` (read-time, `--across` only: another live branch holds a different value for the same key).

---

## 4. Storage engine (delta against [B §4], with the [20]/[21] grafts written into the spec)

### 4.1 Files (≤ 12 per store, all under `<store>/` = `.moirai/`)

| File | Role | Change vs [B §4.1] |
|---|---|---|
| `HEAD` | 2 × 4 KiB checksummed slots | slot gains `epoch u64` (random at `init`, repeated in every record header), `pins[16]` (pinned checkpoint segment sets with refcounts, §5a.3), `n_heads`, `default_ref`, `image_cursor[4]` (last exported seq per destination) |
| `LOCK` | 4 KiB; lock bytes 0 writer, 1 leader, 2 maintenance, 3 quiet; holder diagnostics at offset 2048 (never inside a locked range) | writer acquisition = blocking `LockFileEx` on an overlapped handle + `WaitForSingleObject(2 s)` + `CancelIoEx` (G1); no sleep loop |
| `log.NNNN` | 64 MiB extents, zero-filled once at creation, then `DATA_SYNC_ONLY` per commit (G11) | record header `{len u32, kind u8, flags u8, _ u16, lsn u64, epoch u64, xxh3 u64}` = 32 B (X2 point 4) |
| `hist.NNNN` | sealed zstd frames of 256 commits + a per-frame commit index (id16 → lsn) | the commit index lives here, never in `seg.base` (G3) |
| `seg.base.G`, `seg.dK` | materialized `main` state | a segment set referenced by a branch base or a tag pin is refcounted in `HEAD.pins` and survives rollup |
| `seg.b<refsym>.K` | **branch delta segment** (a promoted branch overlay; the delta format keyed to a ref) | new; written when a branch overlay exceeds 8,192 ops or 8 MiB |
| `blobs.NNNN` | content-addressed bodies | bodies are appended to the log tail first and copied here at checkpoint (G7); `blobs` files are sealed, never extended while mapped |
| `config` | INI text: `default-branch`, `image.*`, `discovery.git-hint`, `lease.*` | new (§5c) |
| `gitmap.NNNN` | sorted `(commit_id16, algo u8, git_oid[32])` pages, sealed; tail entries are `GitMap` log records | new (§5b.7) |

### 4.2 Records

Kinds: `Commit`, `RefUpdate` (the reflog), `ClientHead` (per-client HEAD move), `Lease`, `Idem`, `GitMap`, `Pin`, `Checkpoint`, `Lazy` (heartbeat/cursor), `RefTable`, `Noop`. The `Commit` body is [B §4.3] plus: `ref u16` = the branch the commit was made on; `kind u8` (ordinary, merge, revert, cherry-pick, rebase, import-native, import-foreign); `foreign_git [u8;32]+algo` for imported foreign commits; ops as [B §4.3] plus `Incr{node, field, delta}`, `Conflict`, `Violation`, `Resolve{key, choice}`, `Schema{weaken|strengthen}`.

**Canonical form** hashed into `commit_id` (BLAKE3-256): parent ids, `kind`, `hlc`, actor/role/session *as strings*, git provenance, message, schema version, and the op list with nodes named by `uid`, edges by `(uid, kind name, uid)`, ops sorted by (uid, field name, kind name, dst uid) — never `#N`, `lsn`, `seq`, offsets or symbol numbers. Two stores that import the same image compute the same ids; this is what makes the git trailer `Moirai-Commit` verifiable (§5b.4).

### 4.3 Write, read, open, checkpoint

- **Write path** = [B §4.5] with: (1) the blocking lock wait; (2) scan from `HEAD.committed_lsn` to the first bad checksum or epoch mismatch, adopt and re-flush complete unpublished commits, republish `HEAD`, *then* evaluate the idempotency key (X2, F-B7); (3) preconditions include `--if-rev <seq>`, the lease token, the branch kind (`plan/*` refuses coordination fields), and the payload hash bound to the idempotency key (X3); (4) the commit is appended with `ref` = the client's current branch and the `RefUpdate` for that branch in the same flushed group; (5) no maintenance inside the writer byte.
- **Read path** = [B §4.6] with readers stopping at `committed_lsn` (F-A1) and `HEAD` read only by `pread` (F-B1). Reading a branch: §5a.3.
- **Checkpoint** (delta segment; automatic at 4,096 ops / 4 MiB of records; bodies on a separate 32 MiB threshold) is performed by the committing process *after* releasing the writer byte, under the maintenance byte; readers keep the old set; the writer byte is re-taken for microseconds to append `Checkpoint` and publish. Rollup only on explicit `moirai gc` or in the long-lived MCP server after a request (G9). Quiet mode: no automatic checkpoints; hard cap 8× threshold, then one bounded delta (G10). Orphan temp segments are swept by the next maintenance holder (G14).
- **Open path**: [A §4.8] without the "apply beyond committed_lsn" clause; resolving the client HEAD (§5a.4) adds one table probe.

### 4.4 Indexes (delta)

Added: `REFS` section (name → commit, kind, base pin, fork commit, ops-since-fork) mirrored from `HEAD`; `PINS`; `HEADS` (client key → ref or detached commit); `GITMAP`; a `TOUCH` bitmap per promoted branch segment (rows the branch overrides) so `--across` finds divergent nodes by bitmap AND. `IDEM` retains 30 days (F-A6) and stores `(key16, payload_hash16, branch_sym, result blob ref)`.

### 4.5 Crash safety and Windows

As [B §4.9] and [A §4.11], plus: the epoch in headers rejects records from a restored older copy of the store; `doctor --verify` recomputes every branch head from pin ⊕ ops and compares it with the promoted branch segments; `LOCK` diagnostics sit outside locked bytes; the M0 measurement list of [20 G8] gains one item: the cost of creating ~30 small files (loose objects) on NTFS with Defender on, which decides the image writer's loose-vs-pack threshold (§5b.8).

---

## 5. Versioning

### 5a. moirai's own VCS and branching (T3′)

#### 5a.1 Object model

| Object | Identity | Content | Where |
|---|---|---|---|
| **commit** | BLAKE3-256 of the canonical form (§4.2); `id16` prefix used in indexes; displayed as `c<8 hex>` | header (parents, kind, gen, seq, ref, hlc, actor, role, session, git provenance, idem, message, schema version) + changeset (typed ops with before-images) | `log`/`hist` |
| **changeset op** | position in its commit | `Create`, `Delete`, `SetField`, `Incr`, `SetBody`, `AddEdge`, `RemoveEdge`, `Move`, `Schema`, `Conflict`, `Violation`, `Resolve` | inside the commit |
| **blob** | BLAKE3-128 of raw bytes | zstd-dict frame of a body | `blobs.NNNN` / log tail |
| **ref** | name (`main`, `lane/<n>`, `plan/<n>`, `merge/<n>`, `tags/<n>`) | `{kind, commit_id, lsn, base_pin, fork_commit, fork_seq, ops_since_fork, promoted_seg, gen}` | `HEAD.refs` / `RefTable` |
| **reflog entry** | `(ref, seq)` | `RefUpdate {ref, old, new, reason, actor, hlc}` | log |
| **client head** | client key (blake3_16 of the absolute worktree/dir path, or an explicit `--client` name, or `session:<id>`) | `ClientHead {key, ref \| detached commit, hlc}` | log + `HEADS` |
| **tag** | name under `tags/` | a ref of kind `tag` with an optional message and a `Pin` (§5a.3) | as ref |
| **pin** | segment-set id | refcounted reference to a sealed checkpoint segment set | `HEAD.pins` |

A commit's parents may be on any refs; the DAG is one store-wide graph (as in git), so a branch can fork from a branch, and `main` can be merged into a lane and the lane back into `main` repeatedly. `gen = 1 + max(parent.gen)` (git commit-graph generation numbers [04 §3.1]) prunes ancestor walks: if `gen(A) ≥ gen(B)` then A is not an ancestor of B.

#### 5a.2 Refs and the reflog

- `HEAD.refs[96]` inline; beyond that a `RefTable` record holds the full table and the slot holds only its lsn (as [B §4.2]). Ref names are symbols; 50 live lanes plus tags and staging refs fit inline.
- Every ref move is a `RefUpdate` record written in the same flushed group as the commit that caused it (ordinary commit: `ref: old → new`; merge: target ref; `undo`: explicit). `moirai reflog <ref>` is a log scan filtered by ref symbol; the last 32 moves per ref are also cached in the `REFS` section for O(1) `undo`.
- Ref update is compare-and-swap on `old`; a stale `old` (another process committed on the same branch first) fails with exit 4 and the current tip. Two agents on one lane branch therefore serialize on the writer byte exactly as on a shared trunk.

#### 5a.3 How a branch lives on the engine

A branch view is built from three immutable layers plus one mutable tail:

```
view(X) = SEG(pin_X)                       // sealed checkpoint segment set of `main` at seq P ≤ fork_seq
        ⊕ ops(main, (P, fork_seq])         // trunk ops between that checkpoint and the fork commit
        ⊕ ops(X, (fork_seq, tip_X])        // the branch's own commits (and merges into it)
        ⊕ tail(X)                          // this process's overlay for X since its last replay
```

- **Fork** (`moirai branch lane/l5np [--from main|<commit>]`): one `RefUpdate` (create) + one `Pin` increment on the newest sealed checkpoint set whose `upto_seq ≤ fork_seq` (the fork commit is usually the tip of `main`, so `P` is the last checkpoint, at most 4,096 ops behind). Cost: one durable commit (~2 ms), zero copying. The pin keeps that segment set alive across future rollups of `main`.
- **Overlay build** (on first read of X in a process): replay `ops(main, (P, fork_seq])` and `ops(X, …)` from `log`/`hist` (sequential reads; `hist` frames decode at ~1.5 GB/s) into the per-process overlay structure of [A §4.5]. Bounded by the checkpoint spacing (≤ 4,096 ops) plus the branch's own ops (owner's lanes: ~1–3k ops per lane est. from [02 §12.6]); **1–5 ms est.**, ≤ 1 MB private per branch the process actually reads.
- **Promotion**: when `ops_since_fork` > 8,192 or the overlay exceeds 8 MiB, the next maintenance holder writes `seg.b<X>.K` (a delta segment containing the branch's overridden rows, lists and bitmaps, plus its `TOUCH` bitmap) and moves the branch's `base_pin` forward to `(pin, seg.bX.K)`. The view becomes `SEG(pin) ⊕ seg.bX.K ⊕ ops(X, (promoted_seq, tip])`. Cost O(overlay), 10–100 ms est., never in the writer byte.
- **Merging `main` into X** (`moirai sync`, §5a.7) does *not* move `base_pin`; it appends a merge commit on X whose changeset is `main`'s ops since the LCA, so the overlay grows by those ops. A `sync` that brings > 8k ops triggers promotion, which is the cheap way to "re-base" the physical layer. An explicit `moirai rebase X --onto main` (history rewriting, §5a.9) rewrites X's commits on top of `main`'s tip and re-forks the pin at the new tip.
- **`main` itself** is a branch whose `base_pin` is the live checkpoint set: `view(main) = SEG(current) ⊕ tail`, exactly [B §4.6].
- **Cost with ~50 live branches** (est.): refs 50 × 39 B inline; pins: branches forked within one rollup interval (~20k ops of `main`, roughly two weeks at ~1k commits/day est.) share one base file, so at most 2–4 base files (12 MB each at 1e5, 115 MB at 1e6) plus their deltas stay pinned — 25–50 MB extra disk at 1e5, 0.25–0.5 GB at 1e6; RAM: only overlays of branches a process reads (≤ 1 MB each); `doctor` reports pins held by abandoned branches. Reading a branch forked long ago costs the same as a fresh one because the pin froze its base.

#### 5a.4 HEAD per client and checkout semantics

- **Client key** resolution order: `--branch <ref>` (per command) → `MOIRAI_BRANCH` env → `--client <name>` / `MOIRAI_CLIENT` → the registered binding for the current directory (`moirai worktree bind <dir> <ref>`, matched by longest bound prefix of the absolute path) → the binding for the git worktree if a `.git` hint exists (§5c) → `config.default-branch` (`main`). The MCP server resolves per call from the stamped `ctx.cwd`, so subagents in different worktrees read different branches through one server.
- `moirai checkout <ref|commit>` writes a `ClientHead` record for the resolved client key. Detached reads (`checkout c4410`) are allowed; writes on a detached head are refused unless `--branch-new <name>` (git's `checkout -b`).
- Many agent processes on different branches of one store: each read builds only its branch's overlay; writes serialize on the writer byte; a write is appended with `ref` = the client's branch and CAS on that ref's tip. Nothing about one client's HEAD affects another (the `HEADS` table is per key), and processes never share overlays.
- `moirai branch --list` prints tip, kind, fork point, ops-since-fork, last actor, bound worktrees, and whether the branch is `ahead`/`behind` `main` (gen-pruned ancestry walk, µs).

#### 5a.5 Op log, reflog, undo

The commit log *is* the operation log (jj's two levels collapse because every agent write is one commit [04 §3.8]), and `RefUpdate` + `ClientHead` records make it a complete jj-style op log: any past *view* (set of ref tips) can be restored.

| Verb | Effect | Cost |
|---|---|---|
| `moirai undo [--ref R] [N]` | moves ref R back to the value recorded N `RefUpdate`s ago (default: the client's branch, N = 1); appends a new `RefUpdate{reason: undo}`; nothing is rewritten and the undone commits stay in the log until GC | 1 durable commit |
| `moirai op log` / `moirai op restore <seq>` | lists ref-set changes store-wide; restores *all* refs to their values at seq (jj `op restore`) | scan of `RefUpdate`s; 1 commit |
| `moirai revert <commit> [--onto R]` | appends the inverse changeset (before-images make inversion exact, SQLite-session style [04 §3.13]) as a new commit on R, **after running the validators**; refused with the dependent set if a later commit on R depends structurally on the reverted one (C's rule, [21 §3.1 item 14]) | O(ops in commit) + validation |
| `moirai reflog R` | the ref's move history with actor, reason, hlc | O(moves) |

Data-level undo of "the last thing I did" is `revert` of the client's last commit; ref-level undo is `undo`. Both are appended, never rewritten (the git image stays append-only, §5b).

#### 5a.6 log, diff, show, blame, as-of

- `log [R] [--graph] [--node #N] [--actor] [--since seq] [--all-branches]`: commit-index walk from the ref tip through parents (gen-ordered priority queue, like `git log`); `--node` walks the node's per-branch op chain: `NODE.last_op_lsn` in the branch view → `prev` links, filtered to ancestors of the tip (gen-pruned). Costs: O(commits shown); per-node O(edits to the node).
- `diff A..B` (A ancestor of B): fold of changesets along the path, grouped per key, rendered as [B §5.5]. `diff A...B` (symmetric, for lane vs main): LCA by gen-pruned walk, then both sides' folds side by side, with a `both` column for keys touched on both sides — the merge preview.
- `show #N@<commit|seq|time> [--branch R]`: walk the node's chain backwards from R's view applying before-images until the chain passes the target (membership by gen-pruned ancestry); O(edits after the target).
- `blame #N [field]`: last op per field/edge from the chain with commit metadata.
- Whole-graph as-of (`moirai at <commit> -- <query>`): if the commit is on a pinned set's timeline within 50k ops, reverse-apply from the nearest later pin, else replay forward from the nearest earlier pin; every merge into `main` and every tag pins a checkpoint set (policy `gc.pin`), so the distance is bounded by pin spacing. Output carries no derived fields unless `--recompute` (I18′).

#### 5a.7 Merge algorithm

`moirai merge <src> [--into <dst>] [--policy P] [--strict] [--message M]` (default `dst` = the client's branch; `moirai sync` = `merge main --into <current lane>`).

1. **LCA** of `tip(src)` and `tip(dst)` by gen-pruned bidirectional parent walk over the commit index (multiple LCAs → the newest by gen; recursive-merge of criss-cross bases is *not* implemented in v1: a `CrissCross` violation is emitted and `--base <commit>` must be given).
2. **Changesets**: fold `ops(dst, LCA..tip)` and `ops(src, LCA..tip)` by walking the commits on each side **sequentially through `log`/`hist`** (not per-node chain reads, which are random `pread`s [21 §3.2 item 10]); each fold is a map key → (base value from the earliest before-image, final value).
3. **Partition by key**: `(uid, field)`, `(uid, kind, uid)`, `(uid, existence)`, `(uid, parent)`, `(uid, body)`, `(schema item)`. Disjoint keys commute and apply directly; same-key changes go to the typed merge function:

| Field type | Rule | On disagreement |
|---|---|---|
| status (per-kind lattice from the schema) | join if comparable and both are forward moves; a side state vs a forward move, or incomparable elements (`confirmed` vs `refuted`) | `StatusFork` conflict value |
| enum / number scalar | equal → take; one side = base → take the other | `FieldEdit{base, ours, theirs}` |
| counter (`Incr`) | sum of both deltas over base | never conflicts |
| set | add-wins union with removals relative to base | never conflicts |
| text | line diff3 against the base blob; `section` bodies additionally run the removed-text guard | `TextHunk` conflict value (rendered with `<<<<<<<` markers on read); `RemovedTextNotInBase` violation |
| `parent` / `order` | Kleppmann move in HLC order; a cycle-creating move is skipped and logged | `HierarchyCycle` violation (structural) |
| existence | delete vs modify → `DeleteVsModify`; `--policy delete-wins\|resurrect` per kind | conflict value on the (resurrected) node |
| structural edge to a node deleted on the other side | `DanglingEdge` violation; suggested resolution = the edge kind's delete policy or `--replaced-by` | structural, staged |
| `supersedes` | second active superseder → `SupersedeFork` | conflict value |
| owner-authority fields (`authority = owner`, `owner_quote`) | `main`'s value wins when `dst = main`; otherwise conflict | `OwnerFieldEdited` |
| coordination fields when `src` is a `plan/*` branch | dropped, logged as `PlanStatusIgnored` | hint |
| schema | weakening changes union; strengthening on both sides → `SchemaConflict` | structural |

4. **Apply** to a candidate overlay on `dst`'s view (TerminusDB "commit without advancing the label" [04 §3.3]).
5. **Validate** on the candidate: I2 dangling structural edges (reverse index ∩ deletions on either side), I4 forest, I5′ (full Kahn over the combined precedence graph when the merge touches > 1,000 precedence edges, incremental PK otherwise), I6/I7, D11 cardinality, schema, `plan/*` read-only, duplicate/contradiction hints.
6. **Emit** one merge commit with two parents, the merged ops, one `Conflict` op per value conflict and one `Violation` op per structural problem.
7. **Advance or stage**: zero violations and (`--strict` ⇒ zero conflicts) → the merge commit lands on `dst` (CAS on the tip); nodes carrying conflict values get `conflicted` and leave `ready`. Otherwise the commit lands on `merge/<src>` (kind `merge`), `dst` is untouched, and the CLI prints the violations (exit 6). `moirai resolve <key> --take ours|theirs|base|--value V`, `moirai resolve --all --policy P` append `Resolve` commits on `merge/<src>`; `moirai merge --continue` re-runs steps 4–7 **against the current `dst` tip** using the staged resolutions as an overlay (the SQLite rebaser idea [04 §3.13]; fixes [21 §3.2 item 7]); `merge --abort` deletes the staging ref.

#### 5a.8 Conflict and violation taxonomy

| Class | Kind | Lands on the ref? | Default resolution |
|---|---|---|---|
| `FieldEdit`, `StatusFork`, `TextHunk`, `DeleteVsModify`, `SupersedeFork`, `OwnerFieldEdited` (dst ≠ main) | value conflict | yes (unless `--strict`); node `conflicted` | agent `resolve`; per-kind policies as data |
| `DanglingEdge`, `Cycle`, `HierarchyCycle`, `IdCollision` (import only), `SchemaConflict`, `RemovedTextNotInBase`, `CrissCross`, `ImageParse` (import only) | structural violation | **never**; staged on `merge/<src>` | suggested fix printed; `resolve --policy` |
| `Duplicate`, `Contradiction`, `PlanStatusIgnored` | hint | yes, as a log line | none |

#### 5a.9 revert, cherry-pick, rebase, tags, delete

- `cherry-pick <commit> [--onto R]`: 3-way apply of that commit's changeset with base = its first parent onto R's view; same typed rules and validators; result is a new commit (new id, `kind: cherry-pick`, `origin` in the message trailer).
- `rebase <branch> --onto <ref>`: cherry-picks the branch's commits since the LCA one by one onto the target tip, creating new commits; the old commits stay reachable through the reflog until GC; the branch's `base_pin` is re-forked at the new base. Refused if any cherry-pick stages a structural violation (the user resolves with `rebase --continue`). This is the only history-rewriting verb, and the git image records it as new commits (the old ones remain in the image's reflog ref if exported).
- `tag <name> [commit] [-m]`: a ref of kind `tag`; `--pin` also pins the checkpoint set for fast as-of.
- `branch -d <name>`: refused if not merged into `main` unless `-D`; drops the pin refcount; the reflog keeps the tip for `gc.reflog-expire` days.
- `sync`: `merge main --into <lane>`; `moirai sync --check` = the merge preview only.

#### 5a.10 GC

Reachability = refs ∪ reflog entries younger than `gc.reflog-expire` (default 90 days, git's default [04 §3.1]) ∪ pins. `moirai gc` (explicit, never automatic): (1) rollup of `main`; (2) rewrite `hist` frames dropping unreachable commits older than `gc.cruft-delay` (14 days) — commit *headers* are kept for lineage unless `--prune-headers`; (3) drop segment sets with refcount 0; (4) blob GC as [B §4.8]; (5) `gitmap` compaction. Reflog expiry and cruft delay are the only places a moirai commit can disappear; exported commits never disappear from the image.

#### 5a.11 Cost of the VCS layer on the engine (est.; inputs from [05 §2], [B §8])

| Operation | 1e4 | 1e5 | 1e6 | Derivation |
|---|---|---|---|---|
| `branch` (fork) | 2–3 ms | same | same | one durable commit + pin |
| `checkout` | 2–3 ms (durable `ClientHead`) | same | same | — |
| first read on a branch (overlay build, ≤ 4k trunk ops + 2k branch ops) | 1–5 ms | same | same | sequential log/hist decode |
| subsequent reads | as `main` (+ overlay probe, ~0.2 µs) | | | |
| promotion (8k-op overlay) | 10–40 ms | 20–60 ms | 30–100 ms | delta segment write |
| `merge` of a 2k-op lane vs 5k trunk ops since LCA | 5–20 ms | 10–40 ms | 20–80 ms (full Kahn if > 1k precedence edges: +5–50 ms) | two sequential folds + typed merge + validators |
| `log --node` | 10–50 µs per shown edit | same | same | chain walk; `hist` frame decode ≤ 0.1–0.5 ms per cold frame (F-B9) |
| `diff A...B` | as merge minus apply | | | |
| `undo` | 2 ms | | | one durable commit |
| `revert` | 2 ms + O(ops) validation | | | |
| as-of at a pinned set | reverse-apply ≤ 50k ops: 5–50 ms | | | |
| 50 live branches, disk overhead | ~5 MB | 25–50 MB | 0.25–0.5 GB | 2–4 pinned base files + deltas |
| 50 live branches, RAM | 0 until read; ≤ 1 MB per branch read per process | | | overlay size |

### 5b. The git-compatible image (T15, R3)

Design goals, in order: (1) every moirai commit, branch and tag maps to git objects by a **pure function of moirai data**, so two exporters produce byte-identical objects; (2) a human or an agent can read and diff the image in any git UI; (3) an edit or merge made on the git side is importable as a first-class moirai commit; (4) export and import need no `git` binary. Git is a storage/transport target; the store stays canonical (Beads' dual-source lesson [03 §8.2]).

#### 5b.1 Path layout of one image tree

```
.moirai-image                         format marker (text, §5b.3)
schema/
  kinds.moi  fields.moi  edges.moi    schema-as-data, one file per table, sorted rows
nodes/
  <h1>/<h2>/<uid>.moi                 h1 = uid hex[0..2], h2 = uid hex[2..4]; uid = 32 lowercase hex
refs/                                 present only in `--granularity checkpoint` images: the moirai ref table
  heads.moi  tags.moi                 (name, commit id, kind, fork) rows, so a checkpoint image is self-describing
```

- Two levels of 8-bit fan-out over the `uid`: 65,536 leaf trees. At 1e4 nodes most leaves hold 0–1 entries (many tiny tree objects, ~1e4 of them); at 1e5 ≈ 1.5 entries; at 1e6 ≈ 15. Per touched node a commit rewrites root (256 entries × ~40 B ≈ 10 KB raw), one mid tree (≤ 10 KB) and one leaf (≤ 1 KB) — the path-copy tax of [04 §3.1], bounded by the fan-out rather than growing with N. One level would make leaves 270 KB at 1e6; three levels would make 1e4-node images tree-dominated. Two is the fixed choice; it is part of the format version.
- **Edges are stored in the source node's file** (out-edges only). Adding `A blocks B` rewrites `A.moi` only; the reverse index is rebuilt on import. In-edges are never serialized (a rule cited by 10k nodes would otherwise change on every citation).
- **Tombstones are files**: a deleted node's `.moi` is rewritten as a tombstone (`deleted:` line, reason, replaced-by, deleting commit) and stays forever. A file that *disappears* from the tree is a foreign hard delete (§5b.6).
- Bodies are the tail of the node file, not separate blobs: one blob per node keeps object counts at ~1.0N and makes `git diff` show prose changes. (A `--bodies external` option writes `bodies/<blake3>.txt` blobs and references them; useful only when bodies dominate.)
- No `#N`-named files, no kind-named directories: kinds can change (note → task) and `#N` is store-local; both would create renames and would break determinism across stores.

#### 5b.2 The `.moi` node file format (canonical text, version 1)

Rules (all mandatory; the exporter is the only writer of canonical bytes, the importer accepts a superset):

1. UTF-8, LF line endings, no BOM, exactly one trailing LF, no trailing whitespace. Bytes are preserved as given (no Unicode normalisation).
2. Line 1 is `moirai-node 1`. Then header lines in this fixed order, each `key: value`; absent/empty values are **omitted** (no `-`, no `null`). Order: `uid`, `id`, `kind`, `title`, `status`, `resolution`, `priority`, `criticality`, `confidence`, `authority`, `parent`, `order`, `created`, `updated`, `deleted`, `flags`.
3. Then `field <name>: <value>` lines sorted by `<name>` bytewise; then `label <value>` lines sorted; then `edge <kind> -> <uid> [key=value ...]` lines sorted by (kind name, dst uid, props); then `conflict <key>: base=<v> ours=<v> theirs=<v> [class=…]` lines sorted by key; then `violation <class>: …` lines.
4. Values: integers in decimal; floats in shortest round-trip form (Ryu) with a mandatory `.` or exponent; booleans `true`/`false`; timestamps RFC 3339 UTC with `Z` and millisecond precision; node references as 32-hex `uid`; commit references as `c<64 hex>`; sets as `[a, b, c]` sorted bytewise; strings **bare** when they contain no control characters, no leading/trailing spaces, do not start with `"`, `[` or `{` and are ≤ 4,096 bytes, otherwise **JSON-string-escaped** (`"..."`). Long text fields (`failure_scenario`, `what`, `why`, `acceptance`) use a block form: `field why: <<` on its own line, the raw lines indented by two spaces, terminated by `>>`; block form is chosen iff the value contains a newline.
5. `created`/`updated` are `c<commit-id>` **followed by** the commit's HLC timestamp for readability: `created: c3f9a… 2026-09-21T14:02:11.483Z`. Importers use only the commit id.
6. `id: <N>` is the exporting store's `#N`. It is an **alias hint**: on import the number is kept if free, else a new `#N` is allocated and the pair is written to the alias map (§5b.6). It never contributes to hashes.
7. Derived state (`open_blockers`, `ready`, `suspect`, `is_blocker`, rollups, `conflicted`, `rev_seq`, `lsn`) is never written.
8. The body, if any, follows a line consisting of `---` and runs to end of file, raw. A node without a body has no `---` line. (A body that itself starts with `---`… is unambiguous because the separator is the first `---` line after the header block.)
9. A tombstone file contains only `moirai-node 1`, `uid`, `id`, `kind` (the kind at deletion), `deleted: c<commit> 2026-…Z`, `field reason: …`, `field replaced_by: <uid>` and the `title` (kept so `git log -- nodes/…` stays readable).
10. Conflict values (§5a.8) are written as `conflict` lines with the three values in the same value syntax, so **a merge with conflicts is representable** — unlike jj, where "commits with conflicts cannot be represented in Git" [S1].

**Full example file** — `nodes/01/8f/018f3c2e7a117b3c9d5e4c2f1a0b9e77.moi` (task `#12` of the walk-through, after `#40` was deleted with `--replaced-by #52`):

```
moirai-node 1
uid: 018f3c2e7a117b3c9d5e4c2f1a0b9e77
id: 12
kind: task
title: Wire lease reclaim
status: in_progress
priority: 1
criticality: normal
parent: 018f3c2e7a117b3c9d5e4c2f1a0b9e07
order: a0V
created: c9b2e6c1d4f0a7e3b5c8d1f2a9e4b7c6d3f0a1e2b5c8d7f4a3e6b9c2d5f8a1b4 2026-09-21T14:02:11.483Z
updated: c812e0f3a6b9c2d5e8f1a4b7c0d3e6f9a2b5c8d1e4f7a0b3c6d9e2f5a8b1c4d7 2026-09-25T14:02:40.011Z
field acceptance: reclaim sweeps expired leases; fencing token checked in complete/release
field assignee: dev#1
field estimate: 3
field files_owned: [src/lock.rs, src/reclaim.rs]
field phase_state: implementing
field work_kind: impl
label: l5np
label: physics
edge blocks -> 018f3c2e7a117b3c9d5e4c2f1a0b9e91
edge cites -> 018f3c2e7a117b3c9d5e4c2f1a0b9e12 pin=c4410f0e2d3c4b5a69788796a5b4c3d2e1f0a9b8c7d6e5f4a3b2c1d0e9f8a7b6
edge implements -> 018f3c2e7a117b3c9d5e4c2f1a0b9e40
edge mentions -> 018f3c2e7a117b3c9d5e4c2f1a0b9e52
---
Reclaim must run under the maintenance byte, never inside a request.
See #52 for the lock protocol this depends on.
```

A `git diff` of the commit that deleted `#40` shows, in `018f…9e40.moi` (the old blocker), the header collapsing to a tombstone, and in `018f…9e77.moi` one line changing from `edge blocks -> …9e40` to `edge blocks -> …9e52` (the re-pointed structural edge). No other file changes: in-edges are derived.

#### 5b.3 Format marker `.moirai-image`

```
moirai-image 1
object-format: sha256
store-id: 6f2a…            (128-bit random from `moirai init`; a second store importing this image records it as `origin`)
schema-version: 3
granularity: commit        (or checkpoint)
```

#### 5b.4 Commit metadata mapping (moirai commit → git commit)

| git field | value | note |
|---|---|---|
| `tree` | the image tree at that commit (§5b.1) | full tree; git shares unchanged subtrees by hash |
| `parent` × n | git ids of the moirai parents through `gitmap` | order preserved; merge commits have two parents; `revert`/`cherry-pick`/`rebase` results are ordinary commits with trailers naming the origin |
| `author` | name `moirai/<actor>` (e.g. `moirai/dev#1`), email `<role>@moirai.invalid`, time = `hlc >> 16` ms → seconds, tz `+0000` | `.invalid` is a reserved TLD; no real identity is fabricated |
| `committer` | identical to `author` | **deterministic**: using the exporter's identity or wall clock would make the same moirai commit hash differently on re-export |
| `encoding` | omitted (UTF-8) | |
| message | moirai message, then a blank line, then trailers in this fixed order: `Moirai-Commit: <64 hex>`, `Moirai-Kind: ordinary\|merge\|revert\|cherry-pick\|rebase\|import-native\|import-foreign`, `Moirai-Ref: lane/l5np`, `Moirai-Seq: 4410` (exporting store's seq; informational), `Moirai-Actor: dev#1`, `Moirai-Role: developer`, `Moirai-Session: <id>`, `Moirai-Git-Head: sha1:7c1e0a…` (project-repo provenance), `Moirai-Git-Branch: u/l5np`, `Moirai-Worktree: <lanes-dir>/l5np`, `Moirai-Origin: c…` (revert/cherry-pick source), `Moirai-Schema: 3`, `Moirai-Ops: 3`, `Moirai-Idem: <hex>` (if any) | the message is the first paragraph exactly as stored; trailers are the git `key: value` trailer format so `git interpret-trailers` reads them |

The git commit id is therefore a function of (moirai commit content, parents' git ids, object format). With SHA-1 and SHA-256 destinations the same moirai commit has two git ids; `gitmap` stores both keyed by algorithm.

Refs: `main` → `refs/moirai/heads/main`; `lane/x` → `refs/moirai/heads/lane/x`; `plan/x`, `merge/x` likewise; `tags/v` → `refs/moirai/tags/v` (lightweight; annotated only with `--annotated`, where the tag object's tagger = the author rule above). In a *separate image repo* the prefix is `refs/heads/` and `refs/tags/` so ordinary `git clone` fetches everything; in a *project repo* the `refs/moirai/` namespace keeps the image out of `git branch` and requires the fetch refspec `+refs/moirai/*:refs/moirai/*` (documented; `doctor image` checks it) — the same caveat Dolt documents for `refs/dolt/data` [08 §7.1]. Optional: `refs/moirai/ops/<store-id>` = a commit chain whose tree is `oplog/<seq>.moi` records of `RefUpdate`/`ClientHead` events, exported with `--with-oplog` (needed only to reproduce reflog/undo history on another machine).

#### 5b.5 Determinism and canonicalisation rules (summary)

1. Tree entries sorted per git's rule (bytewise on name, directories compared as `name/`); modes `100644` for files, `040000` for trees; no executable bits, no symlinks.
2. Object format = the destination repo's `extensions.objectFormat` (SHA-1 default for repositories git created before 3.0; SHA-256 when configured [S4]); the image never mixes formats; `gitmap` records the algorithm.
3. Every byte of every blob is produced by §5b.2; the exporter re-parses each file it writes and asserts equality (a cheap self-check, ~1 µs per line).
4. Commit objects follow §5b.4; no `gpgsig`, no `encoding`, no extra headers (jj's non-standard `jj:trees` header is exactly what "not all git tooling preserves" [S1]; everything moirai needs is in trailers and files).
5. `gitmap` is derivable: walking the image and reading `Moirai-Commit` trailers rebuilds it; `moirai image doctor --rebuild-map` does this and reports commits whose recomputed canonical hash ≠ trailer (tampered or foreign).
6. Checkpoint granularity: one git commit per export run whose tree is the branch head state and whose message lists the moirai commits folded into it (`Moirai-Commit` = the head commit id, `Moirai-Folded: <n> commits from c… to c…`); parents = the previous checkpoint commit(s) of that ref. Intermediate moirai commits are not represented; the moirai commit DAG *between* checkpoints is therefore lossy in this mode (§5b.7).

#### 5b.6 Export and import algorithms

**Export** (`moirai image export [--to <gitdir|bundle path>] [--refs main,lane/*] [--granularity commit|checkpoint] [--since-cursor]`):

1. Open the destination: a git directory (project `.git`, its common dir, or a separate bare repo) is recognised by `HEAD` + `objects/` + `refs/`; read `config` for `extensions.objectFormat`; if the destination does not exist and `--create` is given, write a bare repo skeleton (`HEAD`, `config` with `objectFormat`, `objects/`, `refs/`), no `git` process involved. A `.bundle` destination needs nothing.
2. Determine the frontier: for each selected ref, walk the moirai commits from the tip down until a commit already in `gitmap` for this destination's algorithm (the `image_cursor` in `HEAD` gives a starting seq for the common case). Topologically order the new commits (they are already in seq order on one store; imported foreign commits keep their order).
3. For each new commit in order: build the tree by applying the changeset to the **exported tree of its first parent** (kept as an in-memory map `path → blob id` for the working set; the leaf and mid trees of touched paths are re-encoded, unchanged subtrees keep their ids — the same path copy git itself does); encode touched `.moi` files (§5b.2) from the branch view *at that commit* (the exporter reads node state through the as-of machinery, O(edits) per touched node); hash blobs, trees, the commit object; append `(commit_id, algo, oid)` to `gitmap`.
4. Write objects: **loose** when the export produces ≤ 64 objects (typical incremental run: one commit ≈ 1 blob per touched node + ≤ 2 trees per touched node + root + commit); otherwise one **pack** (`pack-<checksum>.pack` + `.idx` v2, no deltas in v1, zlib level 6) written to `objects/pack/`; or a **bundle** (`# v2 git bundle` / `# v3 git bundle` for SHA-256, ref lines, then the pack) when the destination is a file. The loose/pack threshold is set from the M0 Defender measurement (§4.5).
5. Update refs with CAS: read the current ref value, verify it equals the last exported oid for that ref (or is absent), write the new value (`refs/moirai/heads/<name>` loose ref file via write-temp-then-`MoveFileEx`, with the bounded retry on errors 5/32 of [08 §4 W3]; `packed-refs` is left alone, git resolves loose over packed). On mismatch (someone moved the image ref outside moirai) the export stops with exit 6 and prints `moirai image import` as the remedy. Update `image_cursor`.
6. Durability: objects are written and flushed before refs move; a crash leaves unreferenced objects at worst (git-safe). The `gitmap` records are appended under the writer byte in one durable commit after the refs move.

Transport is outside the core: `git push <remote> 'refs/moirai/*:refs/moirai/*'` (or `--mirror` of the separate image repo), run by the user, a hook, or `moirai image push` which simply spawns `git` if present and otherwise prints the command. Bundles travel by any means and are imported with `git bundle unbundle` or directly by moirai.

**Import** (`moirai image import [--from <gitdir|bundle>] [--refs …] [--into-prefix import/]`):

1. Read refs (`refs/moirai/heads/*` or `refs/heads/*` for image repos; loose and `packed-refs`); read objects through the own reader: loose (zlib inflate, `type size\0` header) and packs (idx v2 fan-out lookup, `OFS_DELTA`/`REF_DELTA` application, multi-pack: linear over `.idx` files; no multi-pack-index or bitmap support in v1 — `git repack -a` output is always readable).
2. Walk commits from each ref tip until a commit whose oid is in `gitmap`; order topologically.
3. For each new git commit: parse the message trailers. **Native** commit: `Moirai-Commit` present and not yet in the store → diff the tree against the tree of its (first) parent → parse touched `.moi` files → produce typed ops (a file added = `Create` + fields + edges; changed lines = `SetField`/`AddEdge`/`RemoveEdge`/`Move`/`SetBody`/`Incr` (an integer field that grew by *k* where the schema marks it `counter`); a file turned tombstone = `Delete`; a file removed = `Delete{reason: image:file-removed}`); reconstruct the canonical form with the trailer metadata and **verify** BLAKE3 = `Moirai-Commit`; append as a moirai commit with the same id (kind `import-native`), parents mapped through `gitmap`. A verification failure demotes the commit to foreign.
   **Foreign** commit (no trailer, unknown trailer, or hash mismatch — a hand edit, a `git commit` on the image, a GitHub web edit, a git-side merge): same tree diff → ops; the moirai commit gets a **new** id, `kind: import-foreign`, `actor: git:<author email>`, `hlc` from the committer time, `foreign_git: <oid>`, message = the git message; a two-parent foreign commit becomes a moirai merge commit whose changeset is the diff against its first parent plus a `Violation{ForeignMerge}` hint naming the second parent.
4. **Validate** every imported commit with the merge validators (§5a.7 step 5) against its parent view: `ImageParse` (a `.moi` that does not parse, e.g. git conflict markers `<<<<<<<` left by a git-side merge), `SchemaConflict` (unknown kind/field/enum without a schema change in the same commit), `DanglingEdge`, `Cycle`, `HierarchyCycle`, `IdCollision` (a `uid` already live with different `created` commit). A clean commit is appended and the local ref moves. A commit with structural violations is appended on `merge/import-<ref>` (staging) and the local ref stops there; `moirai resolve` + `merge --continue` finish it exactly like a local merge.
5. **Ids**: `uid` is identity; `id:` hints are honoured when free; collisions go to the alias map `ALIAS (origin store-id, foreign #N) → local #N`, and `show` prints `#40 (was #17 in store 6f2a…)`. `next_id` advances past the highest honoured hint.
6. `gitmap` gets `(commit_id, algo, oid)` for native and foreign commits alike, so the next export of a foreign commit is a no-op (its git object already exists) and the round trip closes.

Both directions run under the maintenance byte for file I/O and take the writer byte only to append commits, so agents keep working during a 1e6-node export.

#### 5b.7 Round-trip guarantees

| Data | Lossless? | Mechanism |
|---|---|---|
| nodes, fields, sets, labels, bodies, edges with props, tombstones (reason, replaced-by), conflict values, violations, schema | **yes** | `.moi` canonical text; tombstones are files |
| commit DAG (parents, merges, revert/cherry-pick origins), commit metadata (actor, role, session, hlc, message, project-git provenance, idempotency key hash, ops count) | **yes** in `commit` granularity | trailers + parents; `Moirai-Commit` verifies the canonical hash |
| moirai commit ids | **yes** | recomputed from canonical form; stored in the trailer |
| branch and tag names, branch kinds | **yes** | ref names; kind in `refs/heads.moi` (checkpoint) or the `Moirai-Ref` trailer |
| `#N` numbers | **best effort** | alias hint; alias map on collision |
| reflog / op log / client HEADs | only with `--with-oplog` | `refs/moirai/ops/<store-id>` |
| leases, idempotency results, change-feed cursors, `next_id`, fencing tokens, pins, segments, lsn/seq | **no, by design** | runtime state; recomputed or re-established on the importing store |
| derived state | recomputed | never serialized |
| intermediate commits between checkpoints (`checkpoint` granularity) | **no** | folded; the head state is exact, the path to it is not |
| git object ids across object formats | different by construction | `gitmap` keeps both |
| bytes of the store files | no | irrelevant: the image is logical |

Property tests (M4 exit): `export(store) → fresh import → export` yields byte-identical objects; `import(export(S1)) ⊕ import(export(S2))` on a third store equals `merge` of the two.

#### 5b.8 Writer, object format, destinations, granularity

- **Writer**: hand-written (§2 T10). Encoders: blob/tree/commit/tag objects, loose object files, pack v2 + idx v2 (hash-length-aware), bundle v2/v3, loose ref files. Reader: loose + pack with delta resolution + `packed-refs`. Estimated size: ~2.5–3.5k lines of Rust with fuzzing, comparable to the `gix` subset it replaces, and it removes a large dependency and the SHA-256 gap noted in gix's status page [S5]. `git fast-import` is supported as an *alternative* writer (`--via fast-import`, emitting the stream of [S3] with `mark`s = moirai commit ids) for users who prefer git to build packs; it is never required.
- **Object format**: follows the destination. For project repos created before Git 3.0 that is SHA-1; SHA-256 repositories exist since 2.29 with `extensions.objectFormat = sha256` and become the default for new repos in 3.0 (~April 2027) [04 §3.1]; the transition document still says that "using SHA-256 based storage on public-facing Git servers is strongly discouraged" until the protocol supports it [S4], so the default for a **separate image repo** is SHA-1 unless `--object-format sha256` is requested, and `doctor image` warns when a SHA-256 image is pushed to a remote that has not advertised support. The 32-byte `gitmap` column already holds either.
- **Destinations** (`config image.destination`, several allowed):

| Destination | Refs | Cloned by default? | Working tree touched? | PR-reviewable? | Use |
|---|---|---|---|---|---|
| project repo, `refs/moirai/*` (default hint) | `refs/moirai/heads/*` | no (needs refspec; `push --mirror` from an unfetched clone deletes them [08 §7.1]) | no | no (via `git log refs/moirai/heads/main`) | backup + cross-machine sync riding the project remote |
| project repo, orphan branch `moirai/image` | `refs/heads/moirai/image` | **yes** | no | yes (branch view) | when the team wants the image visible in every clone |
| tracked directory `docs/moirai/` in the working tree | none (files committed with code) | yes | yes | **yes** | checkpoint granularity only; git-side merges of `.moi` files are expected and imported as foreign merges |
| separate repo (`image.git`, local bare or remote) | `refs/heads/*` | yes | no | yes | recommended default for the owner: isolates history size from the project repo |

- **Granularity**: `commit` (1:1) for separate repos and `refs/moirai/*`; `checkpoint` (one git commit per export run or per `--every N`) for the tracked directory and for teams who want small histories. A hybrid keeps `commit` granularity on the separate repo and pushes checkpoints of `main` into the project repo.

#### 5b.9 Failure modes and how the design answers them

| Scenario | What happens | Answer |
|---|---|---|
| Someone edits a `.moi` by hand and commits on the git side | foreign commit; parse/validation as §5b.6 step 4 | imported as `import-foreign` with actor `git:<email>`; a parse error stages it with `ImageParse` and the offending path; the store stays consistent |
| `git merge` of two exported branches on the git side | git does a per-file 3-way text merge of `.moi` files; clean merges of disjoint lines are imported as a foreign merge commit and re-validated (typed invariants, not text, decide); overlapping edits leave conflict markers | the marked file fails to parse → staged `ImageParse`; the user resolves in git or with `moirai resolve`; **the store never adopts git's text merge as truth** without validation |
| Files deleted on the git side | `Delete{reason: image:file-removed}` with a tombstone on import | a real moirai delete writes a tombstone file, so an absent file is always a foreign act and is visible as such |
| History rewritten on the git side (rebase/squash/filter-repo) | git ids change; trailers survive rebase (message trailers, unlike jj's headers [S1]); commits whose tree equals the recomputed tree are **re-mapped** (`gitmap` updated, no new moirai commit); commits whose tree differs are foreign | `doctor image` lists re-mapped and orphaned commits; the exporter's CAS refuses to move a ref it does not recognise until an import has run |
| `git push --mirror` from a clone that never fetched `refs/moirai/*` | the remote's image refs are deleted | `doctor image` checks the fetch refspec in every clone's config it can read and warns; the store is canonical, so a fresh export restores the refs; the separate-repo destination avoids the problem entirely |
| Image repo `gc` prunes unreferenced objects after a partial export crash | only objects not yet referenced by a ref are lost | the exporter re-emits them (idempotent: object ids are content-addressed) |
| Two stores export to the same destination (two machines) | ref CAS fails for the second | it must `import` first, which merges the other store's commits (native, ids verified), then export again — Dolt's fetch-then-`--force-with-lease` loop [S6] |
| Object-format mismatch (SHA-1 image, SHA-256 clone) | git itself converts on fetch/push only within its interop limits [S4] | moirai treats each format as a distinct destination with its own `gitmap` column; never mixes |
| Line endings / autocrlf on a tracked-directory image | CRLF conversion on checkout would change blob hashes and break determinism | the exporter writes a `.gitattributes` with `*.moi text eol=lf` (kept diffable, not marked generated); the importer accepts CRLF and normalises; the exporter always writes LF |

#### 5b.10 Cost estimates (est.; SHA-256 at 1.94 GB/s [M, 05 §2.5], zlib-6 ~150 MB/s single-thread est., NTFS + Defender loose-object create ~1–3 ms each est. pending M0)

| Quantity | 1e4 nodes | 1e5 nodes | 1e6 nodes | Derivation |
|---|---|---|---|---|
| Full export objects | ~1.0e4 blobs + ~1e4 trees (sparse leaves) | ~1.0e5 blobs + ~6.6e4 trees | ~1.0e6 blobs + ~6.6e4 trees | §5b.1 fan-out |
| Full export bytes (raw / zlib) | 6 MB / 2.5 MB | 60 MB / 25 MB | 0.6 GB / 0.25 GB | ~0.5 KB per `.moi` incl. body; trees 40 B/entry |
| Full export time (pack) | 0.1–0.3 s | 0.5–2 s | 6–20 s | zlib-bound; hashing ≤ 0.5 s at 1e6 |
| Full export time if written loose (rejected path) | 20–60 s | 3–10 min | hours | per-file cost × objects (why packs are mandatory above 64 objects) |
| Incremental export, one commit touching 3 nodes | 3 blobs + ≤ 7 trees + 1 commit ≈ 25 KB raw / 12 KB zlib; 11 loose files | same | same | ~5–35 ms incl. Defender per-file cost |
| Incremental export, 1,000 commits/day batched into one pack | ~12 MB/day zlib without deltas; ~1–2 MB with OFS_DELTA against the previous blob version (v1.1) | | | tree objects dominate; `git gc` in the image repo delta-compresses them further |
| Image repo growth per year, `commit` granularity | ~4 GB undeltified / ~0.5 GB after `git gc` (est.) | | | 365k commits × 12 KB; git deltas trees well |
| Image repo growth per year, `checkpoint` granularity (daily) | ~4 MB/day of changed files → ~1.5 GB/year undeltified, ~0.2 GB after gc | | | |
| Full import | 0.3–1 s | 2–6 s | 30–90 s | parse ~50–100 MB/s + tree walk + validation (Kahn O(V+E)) + moirai commits in batches of 4,096 ops |
| Incremental import, one commit | 5–20 ms | same | same | tree diff of ≤ 3 paths + validation + 1 durable commit |
| `gitmap` size | 41 B per commit per algorithm: 4 MB per 1e5 commits | | | sealed pages |

### 5c. Git independence (T14, R2)

**Store location and discovery** (no `git` process, no git library, in this order; the first hit wins; a miss never creates a store [B §4.9]):

1. `--store <dir>` flag.
2. `MOIRAI_DIR` environment variable.
3. Walk up from the current directory looking for `.moirai`: a **directory** containing `HEAD` (the store itself), or a **file** whose first line is `moiraidir: <absolute or relative path>` (a pointer, like git's `.git` file; written by `moirai init --link <store>`; may also carry `branch: lane/x` as the default binding for that directory).
4. *Hint only*, if `config.discovery.git-hint = true` (default true): if the walk found a `.git` directory or `.git` file, read it textually (`gitdir:` line, then `<gitdir>/commondir`) and try `<git-common-dir>/moirai/` as the store. This serves the owner's 44 worktrees without any pointer files, and it is the only place git layout matters. It is a read of two small files, never a `git` invocation.
5. Otherwise: exit 7 with the expected paths and `moirai init` instructions.

**One store, many working directories, without git**: `moirai init` creates `<dir>/.moirai/`; `moirai init --link <store>` drops a pointer file in any other directory (worktree, scratch dir, a directory on another drive); `moirai worktree bind <dir> <ref>` records the directory → branch binding in the runtime `HEADS` table (also usable through the pointer file's `branch:` line for read-only tools). Pointer files are plain text; if a `.git/info/exclude` exists next to them moirai offers to add `.moirai` to it (never edits git config).

**Provenance without dependency**: when a `.git` is discoverable, the CLI records `git.head`, `git.branch`, `git.worktree` on each commit by reading `HEAD`, `refs/heads/*` and `packed-refs` textually (~60 lines of code); if it is not discoverable the fields are empty. The MCP server takes them from the stamped `ctx`. Ancestry queries for `stale` (`measured_on` behind the tip) use the same reader over the project repo's commit objects when present (loose + pack reader from §5b.6, reused), with the shared lazy-fact cache of [C §4.7]; if the project repo is absent, `stale` degrades to "unknown" and says so. Nothing in the core crate links `gix`/`git2` or shells out.

**Branch independence**: moirai refs have their own namespace and lifecycle; nothing is created, switched or deleted because git did something. Optional conveniences, all explicit: `moirai worktree bind` (directory → moirai branch), the `post-checkout`/`post-merge` git hooks (`moirai hook git-post-merge` runs `merge-check` and offers `moirai merge`), and `moirai lane open <name> --worktree <dir> --git-branch <b> --base <sha>` which does `branch lane/<name>` + `worktree bind` + records the git facts on the lane node. `doctor lanes` lists bindings whose directories are gone.

**What still needs git**: pushing/pulling the image over the network (`git` CLI as transport, optional), and nothing else. Linux/macOS builds replace `windows-sys` shims only.

### 5d. Coordination under branching and "node 40" across branches (T16)

#### 5d.1 Versioned vs runtime state

| State | Versioned per branch | Store-level runtime | Interaction rule |
|---|---|---|---|
| nodes, all typed fields incl. `status`/`done`/`resolution`/`assignee`/`phase_state`, edges incl. `blocks`/`gates`/`parent`, bodies, schema, conflict values | ✔ | | merge rules §5a.7 |
| refs, tags, reflog | (refs are the version pointers) | ✔ | exported with the image / `--with-oplog` |
| **leases/claims** `{uid, holder, token, expires, run, pid, branch}` | | ✔ | keyed by `uid`, never by branch: a lease means "an agent is working on this node now"; **visible from every branch**; `ready` on any branch excludes nodes leased by another holder; `claim` requires the node to be live and ready **on the claimer's branch** and records that branch; `complete`/`set --lease` write on the claimer's current branch (must equal the lease's branch unless `--move-lease`); `release`/`reclaim`/expiry are store-level; a lease on a node that is deleted on the holder's branch is released with a triage note |
| fencing tokens `HEAD.fence` | | ✔ | monotonic store-wide; expiry never bumps the token (I17′) |
| idempotency results | | ✔ | key + payload hash + **branch**: replaying a key on another branch is an error (exit 9) with the original result |
| change-feed `seq` and per-session cursors | | ✔ | `seq` is store-wide over all branches; every feed line carries `ref`; `changes --since S [--branch R\|--all]` filters; the default is the caller's branch plus `main` |
| `next_id`, `uid` generation | | ✔ | single allocator; `#N` unique across branches |
| client HEAD bindings, quiet flag, pins, `gitmap`, alias map, ancestry cache | | ✔ | not versioned; `--with-oplog` exports HEAD moves for audit only |

Why leases are not versioned: a lease on a `plan/*` branch makes no sense, a lease "merged" from a lane would be a claim by a process that may be dead, and the owner's failure to fear is two lanes building the same task [07 §7.4] — which a store-wide lease prevents while status stays per branch.

#### 5d.2 Task status and `done` across branches and at merge

- `status` is a versioned field; `done` is `status == done`. A task completed on `lane/x` is `done` on `lane/x` only. `main` learns it at `merge lane/x --into main` (lattice: `done > in_progress > open`, forward moves join; `cancelled`/`deferred` vs `done` → `StatusFork`). A lane learns `main`'s completions at `sync`.
- `blocks` across branches: `ready(B)` on branch R uses R's view of A's status. If A is done on `lane/x` and B lives on `lane/y`, B is ready on `lane/y` only after `lane/x → main → lane/y` (or a direct `merge lane/x --into lane/y`). `ready --across` and `blockers #B --across` show "A: done on lane/x (c4470, not merged into your branch)" so the orchestrator can `sync` or dispatch the merge; the SessionStart/UserPromptSubmit hooks include a one-line `behind main by N commits (k completions, m rules)` notice for the bound branch.
- `reopen` is an explicit op on a branch; merging a reopen against a `done` on the other side is a `StatusFork` (never silent).
- Verdict gating (`gates`) is versioned like `blocks`: a critic's `fail_fixable` verdict written on `lane/x` gates completion on `lane/x`; the orchestrator dispatching from `main` sees it through `--across` or after `merge`. This is the R1 answer to [22 §3.2 issue 1]: under full branching, dispatch happens *on the lane branch* (`moirai ready --branch lane/x`), so the verdict is visible where the work is.

#### 5d.3 "Node 40 deleted" — within a branch and across branches

Within one branch (L0–L3) the sequence of [A §6.5]/[B §6.3] holds unchanged: one commit, reverse-index walk, per-edge policies with X4 (re-point or flag), tombstone, `affected` list, feed entry, hooks. Across branches:

| Case | On the deleting branch (say `lane/x`) | On another branch (`main`, `lane/y`) before any merge | At merge |
|---|---|---|---|
| `rm #40` on `lane/x`; `main` and `lane/y` still reference it | tombstone; referrers on `lane/x` updated atomically; `blocks` out of #40 re-pointed (`--replaced-by`) or flagged so dependents stay out of `ready` (X4) | **unaffected** (git-like isolation): #40 is live there; `show #40 --across` prints `deleted on lane/x c812 by dev#2 (not merged)`; `pack` adds an advisory line if the caller's branch is behind | `merge lane/x --into main`: if `main` only *reads* #40 → delete wins cleanly, `main`'s referrers get the same atomic treatment; if `main` **modified** #40 → `DeleteVsModify` conflict value (resurrect/delete-wins policy); if `main` **added a structural edge** to #40 → `DanglingEdge` violation, staged on `merge/lane/x` with the suggested resolution (edge policy or `--replaced-by`); historical edges become tombstone refs and their sources `suspect` |
| `rm #40` on `main`; lanes reference it | as above on `main`; every process reading `main` sees the tombstone at its next read | lanes are unaffected until `sync`; the hook notice says `behind main: 1 deletion touching your #12`; `blockers #12` on the lane prints `#40 (deleted on main c812; sync to apply)` | `sync` (merge main → lane) applies the same rules with the lane as `dst`; a flagged blocker on the lane keeps #12 out of `ready` until `resolve` |
| #40 leased by `dev#2` on `lane/y`, deleted on `lane/x` | `rm` prints `leased by dev#2 on lane/y (L-19)`; allowed (the lease is not on this branch's view) unless `--strict-leases` | dev#2 keeps working; its `complete` succeeds on `lane/y` | `DeleteVsModify` at merge; the lease record, if still live, is attached to the conflict for the resolver |
| #40 deleted on `lane/x`, then `lane/x` is deleted unmerged | the deletion never reaches `main` | nothing | `branch -D` warns that 1 deletion and n commits are dropped; reflog keeps them 90 days |
| #40 deleted in the git image by hand (file removed) | — | — | import creates `Delete{image:file-removed}` on the imported ref, then the merge rules above apply when that ref is merged |

The engine-level guarantee is unchanged: **on every branch head, at every commit, no structural edge points at a dead node, every referrer of a tombstone renders it, and every derived counter equals a recompute** (I2, I12/I20′, I18′). What "maximally synchronous" cannot mean under R1 is "a delete on one branch mutates other branches"; it means "every branch that *receives* the delete (by commit, merge, sync or import) applies it atomically, and every other branch can *see* it on request".

---

## 6. Concurrency and sync (delta against [A §6] and [B §6])

- **Processes and locks**: [A §6.1–6.2] with G1 (blocking `LockFileEx` wait, 2 s, holder reported), maintenance byte for checkpoints/promotions/exports/imports, leader byte reserved for M6. Per-branch overlays are per process; nothing is shared except mapped sealed files.
- **Leases and claims**: [B §6.2] + §5d.1 + [22 §2.2] run-scoped dispatcher leases (`--ttl run`, released by `apply`/`run close`/dead `bg_task_id`) + I17′ (same-holder renewal of an expired, unreclaimed lease; expiry never bumps the token).
- **Change feed**: store-wide `seq`; each entry `{seq, ref, commit_id16, kind, affected ids, newly ready ids, lease events}`; `changes --since S` defaults to the caller's branch ∪ `main`; ref moves (merges, undo) appear as entries so a session learns "lane/x merged into main at c9xx". No file watcher; pull at hook boundaries [08 §6.2].
- **Idempotency**: I14′ (payload-bound) + branch-bound; Workflow agents that write directly use content-hash keys [22 §3.3 issue 3]; `apply` batches are keyed once per run and land on the branch named in the batch (default: the orchestrator's client branch).
- **Node 40 end to end**: §5d.3 within a branch = [A §6.5] steps 1–3; step 4 (agent context) additionally injects cross-branch notices for the agent's bound branch (`behind main: …`), capped at 600 characters, relevance-filtered to nodes the agent claimed, is blocked on, or cited.
- **Quiet mode**: as [B §4.8] plus: exports/imports refuse while quiet unless `--force`; the MCP server stays resident at 0 % CPU.

---

## 7. Agent interface (delta against [B §7.1] CLI and [C §7.2–7.5] MCP/hooks/pack)

### 7.1 CLI additions and changes

Conventions of [B §7.1]/[A §7.1] hold (ids first, deterministic order, `--json v1` envelope, bodies via stdin/`@file`, exit codes 0 ok · 1 internal · 2 usage · 3 not found (tombstone printed) · 4 guard conflict (current value) · 5 lease · 6 precondition/staged merge · 7 store unavailable · 8 partial batch · **9 idempotency payload mismatch**). Every verb accepts `--branch R` and `--client NAME`; `--ids` never prints a header; `blocking` defaults to `kind:task`.

```
# branches, refs, history (orchestrator rituals; CLI only)
moirai branch [NAME [--from REF|COMMIT] [--kind work|plan]] | --list | -d|-D NAME
moirai checkout REF|COMMIT [--branch-new NAME]         moirai worktree bind DIR REF | --list | unbind DIR
moirai lane open NAME --worktree DIR [--git-branch B] [--base SHA]     moirai lane close|freeze NAME
moirai sync [--check]                                  # merge main into the current branch
moirai merge SRC [--into DST] [--policy P] [--strict] [--base COMMIT]
moirai merge --continue | --abort                      moirai resolve KEY --take ours|theirs|base|--value V | --all --policy P
moirai conflicts [REF]                                 moirai merge-check SRC [--into DST]
moirai rebase BRANCH --onto REF [--continue|--abort]   moirai cherry-pick COMMIT [--onto REF]
moirai revert COMMIT [--onto REF]                      moirai undo [--ref REF] [N]        moirai op log | restore SEQ
moirai tag NAME [COMMIT] [-m MSG] [--pin]              moirai reflog REF
moirai log [REF] [--graph] [--node #N] [--all-branches] [--since SEQ]
moirai diff A..B | A...B [--node #N] [--stat]          moirai show #N[@COMMIT] [--across]   moirai blame #N [FIELD]
moirai at COMMIT -- <read verb ...>
# cross-branch views (read-only)
moirai ready|blockers|show|find ... --across [REFS]    # adds "on <ref>: <value>" lines for keys that differ
# git image
moirai image export [--to DEST] [--refs ...] [--granularity commit|checkpoint] [--object-format sha1|sha256] [--bundle FILE] [--via fast-import]
moirai image import [--from DEST|FILE] [--refs ...]    moirai image push|pull [REMOTE]   (spawns git if present, else prints the command)
moirai image doctor [--rebuild-map]                    moirai image show COMMIT           # the .moi diff of a moirai commit as git would show it
# store
moirai init [--link STORE] [--default-branch main]     moirai doctor [store|lanes|image|agents|hooks|--verify]
moirai gc [--prune] [--reflog-expire 90d] [--cruft-delay 14d]   moirai quiet on|off
```

Example outputs:

```
$ moirai branch --list
main        c9b2e6c1  work  ops 0 since fork  bound: <workspace>/BoykoEngine
lane/l5np   c4470a11  work  ahead 14 / behind 31  fork c4410  bound: <lanes-dir>/l5np   lease: L-9 (#12 dev#1)
lane/l10    c4455f02  work  ahead 8 / behind 31   fork c4410  bound: <lanes-dir>/l10
plan/split  c4380b77  plan  ahead 3 / behind 60   fork c4300  (coordination read-only)
merge/l10   c4499d20  merge staged: 1 violation (DanglingEdge #40)   -> moirai conflicts merge/l10

$ moirai blockers #51 --explain --across
lane/l5np: #51 task open P1 "Wire lease reclaim"  BLOCKED
  #12 task in_progress P1 "Byte-range lock protocol"  (direct; lease dev#1 L-9 11m)
  #17 task open P2 "HEAD slot format"                   (direct)
     on main: done c9b1 (not merged into lane/l5np; run `moirai sync`)
  #7  task open P0 "Storage engine M1"                  (inherited from ancestor #9, exogenous)

$ moirai merge lane/l10 --into main
merge lane/l10 (c4455f02) into main (c9b2e6c1), base c4410
  applied 212 keys (188 disjoint, 24 typed): status join x9, add-wins x6, diff3 x7 clean, incr x2
  1 violation -> staged on merge/l10 (exit 6):
  DanglingEdge: main added `#203 blocks #40`; lane/l10 deleted #40 ("dup of #52", replaced_by #52)
    suggested: moirai resolve 'edge:#203:blocks:#40' --take repoint:#52
  0 value conflicts

$ moirai image export --to <workspace>/moirai-image.git
export main lane/l5np lane/l10 (sha1): 53 new commits, 171 blobs, 402 trees -> pack-7f3a….pack (1.9 MB)
refs updated: refs/heads/main c9b2e6c1 -> 3e1f…, refs/heads/lane/l5np …, refs/heads/lane/l10 …
gitmap +53 · cursor seq 4471 · 0.41 s

$ moirai image import --from <workspace>/moirai-image.git
import refs/heads/main: 2 new git commits
  a91c… native   Moirai-Commit c9c0… verified  -> applied on main
  b402… foreign  author git:<owner> "fix typo in rule #212"  -> 1 op SetField(#212.body)  -> applied on main as c9c1… (import-foreign)
import refs/heads/lane/l10: 1 new git commit
  c77d… foreign merge (2 parents)  nodes/01/8f/018f…9e40.moi: parse error line 9 (git conflict marker)
  -> staged on merge/import-lane/l10 (ImageParse); resolve in git or `moirai resolve`
```

### 7.2 MCP tools

The nine tools of [C §7.2] (`brief`, `pack`, `get`, `find`, `claim`, `complete`, `remember`, `write`, `changes`) unchanged in shape, plus **`branch`** (read-only: `list`, `status` of the caller's branch, `across` for a set of ids) — ten in total. Every tool resolves the branch from the stamped `ctx.cwd` binding (§5a.4) and prints `branch: lane/l5np · rev 4471` in its first line so the model always knows where it writes. Versioning and image verbs stay CLI-only. The stamp hook matches write tools only (G6).

### 7.3 Skills and hooks

[C §7.3–7.4] with: `SessionStart` brief prints the bound branch, its ahead/behind vs `main`, staged merges, and lanes with live leases; `UserPromptSubmit`/`PostToolBatch` deltas include cross-branch notices (§5d.2); a new `moirai-branches` skill (orchestrator-preloaded) covers `lane open → sync → merge-check → merge → resolve → image export`; `hook git-post-merge` (optional git hook) runs `merge-check` for the lane bound to that worktree. The 5-minute Workflow-hook experiment of [22 §2.3] is the first M2 task.

### 7.4 Context-pack algorithm

[C §7.5] (classes C1–C8, three renderings, per-class quotas, degrade before drop, dropped ids listed) with: C1 lane header also prints `branch`, `ahead/behind main`, `staged merges`; C2 rules are taken from the caller's branch **and**, marked `~main`, critical rules on `main` not yet merged (owner rulings are never hidden by branching); C5/C6 findings and measurements are branch-local unless `--across`; budgets in characters with per-script token estimates [22 §2.5]; empty `applies_to` = `*` [22 §2.6]; the global critical-rule count in the header; `pack` is a pure read (no `consumed` edges unless `--record-run`).

### 7.5 Walk-through: one BoykoEngine-style campaign under full branching

Roles: orchestrator (main session, bound to `main`), architect and architecture-critic (no Bash; MCP with `ctx.cwd = <lanes-dir>/l5np`), developer and tester (Bash, CLI, in `<lanes-dir>/l5np`).

1. **Session start.** Brief on `main`: checkpoint, 3 live lanes with ahead/behind, `merge/l10` staged with one `DanglingEdge`, open owner question #9, 4 critical rules.
2. **Decompose on `main`.** `add task "Narrowphase batching (L5)" --parent #7` → `#88`, subtasks `#89..#93` with `blocks`, question `#9 --blocks #93`. One commit each, all on `main`.
3. **Open the lane.** `moirai lane open l5np --worktree <lanes-dir>/l5np --git-branch u/l5np --base 7c1e0a` → `branch lane/l5np --from main` (fork `c4410`, pin the last checkpoint), `worktree bind`, lane node `#94` on `main`. Cost: three durable commits.
4. **Design on the lane.** The architect's `write{ops:[plan, section×6, decision×3]}` lands on `lane/l5np` (resolved from `ctx.cwd`). The critic's `pack #88 --role architecture-critic` reads `lane/l5np`; its findings and verdict `#164 fail_fixable --gates #89` are lane commits. Loop termination: `find kind:finding status:confirmed severity:>=important about:#88 --branch lane/l5np` empty.
5. **A rule lands on `main` meanwhile.** The orchestrator writes rule `#212 --critical` on `main`. The tester's next pack on the lane shows `#212 ~main (not merged; moirai sync)`; the SubagentStart hook says `behind main by 3 commits (1 critical rule)`. The orchestrator runs `moirai sync --branch lane/l5np` (a merge commit on the lane, clean).
6. **Dispatch on the lane.** `moirai ready --branch lane/l5np --ids` → `#89 #90`; `claim #89 #90 --agent wf:r7/dev#{1,2} --ttl run --branch lane/l5np` (store-wide leases carrying the branch). Workflow `args` carry ids, leases and `branch`.
7. **Implement, test, verdict** on the lane as [B §7.6] steps 6–8; `complete #89 --lease L-18` writes `done` on `lane/l5np`; `#93` becomes ready *on the lane*; on `main` it is still blocked (`ready --across` explains).
8. **Merge queue.** `moirai merge-check lane/l5np --into main` prints the diff preview (`A...B`), rulings on `main` the lane predates (none after the sync), open confirmed findings (0), gates (green), `merge_after` prerequisites (lane/l10 first). After `git merge u/l5np` in the code repo: `moirai merge lane/l5np --into main` → 9 findings, 2 measurements, 6 sections, 5 status moves join; one `TextHunk` on section `#91` (both lanes edited it) lands as a conflict value; `#91` is `conflicted`; `moirai resolve '#91.body' --take theirs`, done. `lane/l5np` status → `merged`; the pin is released.
9. **Image.** `moirai image export` (hook after merge, or the orchestrator) appends 53 commits to `<workspace>/moirai-image.git`; `git push` to the private remote is the owner's call. A colleague's hand edit on the image comes back through `image import` as a foreign commit the next morning; the brief lists it.
10. **Next session.** `brief` regenerates from `main`; the old lane is gone from the list; `reflog lane/l5np` still shows its life.

---

## 8. Performance and RAM budget (delta against [B §8], [20 §1])

Assumptions unchanged from [B §8] (3 edges/node stored twice, 60 B titles, 24 B fields, bodies 340 B zstd-dict *claimed*, warm page cache, spawn excluded: 20–73 ms [M, 05 §2.1]). Rows that differ or are new are marked **Δ**. All numbers are estimates unless tagged [M].

| Quantity | 1e4 | 1e5 | 1e6 | Derivation |
|---|---|---|---|---|
| Hot index bytes (56 B header + 8 B CSR offsets + 30 B edges + ~1 B bitmaps; `uid` cold) | 0.95 MB | 9.5 MB | 95 MB | [20 §1.1] for B's layout |
| Whole store touched (+ titles, fields, blob table, bodies) | ~5.5 MB | ~55 MB | ~0.55 GB | [20 §1.1] |
| History, `commit` granularity, 1e5 commits | ~30 MB raw / ~12 MB `hist` | same | same | 0.3 KB/commit, zstd 2–3× |
| **Δ** pinned checkpoint sets for ~50 live branches (disk) | ~5 MB | 25–50 MB | 0.25–0.5 GB | §5a.3 |
| **Δ** `gitmap` per 1e5 commits, one algorithm | 4 MB | 4 MB | 4 MB | 41 B/entry |
| Private RSS, CLI (`main`) | 1.5–3 MB | 2–4 MB | 3–6 MB | [B §8] |
| **Δ** Private RSS, CLI reading a lane branch | +0.2–1 MB | same | same | overlay ≤ 6k ops × ~150 B |
| Private RSS, MCP server (`current_thread` tokio, G12) | 3–6 MB | 4–8 MB | 6–12 MB | + one overlay per branch its subagents read (≤ 1 MB each; typically 1–3) |
| Shared page cache touched (hot / whole) | 1 / 5.5 MB | 9.5 / 55 MB | 95 MB / 0.55 GB | mapped, shared by all moirai processes |
| Open store (`main`) | 0.3–1 ms | 0.5–1.5 ms | 0.5–3 ms | HEAD pread + ≤ 8 maps × 0.22 ms [M] + tail replay ≤ 3–5 ms worst case [20 U13] |
| **Δ** first read on a lane branch | +1–5 ms | same | same | §5a.3 overlay build |
| `get #N` | 1–5 µs | 1–5 µs | 1–5 µs | [B §8] |
| `ready` (20) / `blocking --ids` | 10–50 µs | 50–300 µs | 0.3–3 ms | bitmap AND + overlay |
| **Δ** `ready --across` (3 refs) | +3× branch cost + `TOUCH` bitmap AND | | | promoted branches only; un-promoted overlays are scanned (≤ 6k ops) |
| `blockers #N --transitive` | 5–50 µs | 10–200 µs | 20 µs–2 ms | reverse CSR walk |
| Durable commit (small) | ~2.0 ms p50, ~6 ms p99 [M] | same | ~2.2 ms | one flush; + Defender close cost pending M0 (F-A5) |
| **Δ** `branch`, `checkout`, `undo`, `tag` | 2–3 ms | same | same | one durable commit each |
| **Δ** `merge` of a 2k-op lane, 5k trunk ops since LCA | 5–20 ms | 10–40 ms | 20–80 ms | §5a.11 (sequential folds, not chain reads) |
| **Δ** branch promotion (8k ops) | 10–40 ms | 20–60 ms | 30–100 ms | delta segment write, maintenance byte |
| Delta checkpoint (auto, outside the writer byte) | 5–30 ms | 10–50 ms | 20–100 ms | O(tail) |
| Rollup (`gc` only) | 10–30 ms | 0.1–0.3 s | 1–3 s | [05 §7] |
| **Δ** `image export`, one commit (3 nodes) | 5–35 ms | same | same | 11 loose objects incl. Defender |
| **Δ** `image export`, full | 0.1–0.3 s | 0.5–2 s | 6–20 s | §5b.10 |
| **Δ** `image import`, full | 0.3–1 s | 2–6 s | 30–90 s | §5b.10 |
| `pack` / `brief` | 1–5 ms | 2–8 ms | 3–12 ms | [A §8] + one overlay if on a lane |
| Idle CPU, all processes | 0 | 0 | 0 | no threads, timers, watchers |

CI gates (from [B §8], [20 §7]): engine ≤ 5 ms per command at 1e5 on `main`; ≤ 10 ms on a lane after overlay build; private RSS ≤ 4 MB CLI / ≤ 10 MB MCP at 1e5; exactly one flush per durable commit; no O(history) or O(N) work on open; merge of a 2k-op lane ≤ 50 ms at 1e5; export → import → export byte-identical; full export of 1e5 nodes ≤ 3 s; writer-wait p99 ≤ 50 ms with 16 writers using the blocking wait.

---

## 9. Build plan

Relative size (est., share of the whole): engine 22 %, deterministic simulator + kill loops + fuzzers 20 % (its own milestone, [22 §2.8]), graph semantics 13 %, VCS layer (refs, branches, overlays, merge, rebase, GC) 15 %, git image (writer, reader, export, import, doctor) 10 %, CLI + pack/brief + hooks + skills 12 %, MCP 4 %, leader/search/extensions 4 %. Roughly 26–32k lines of Rust plus 10–14k of tests (est.; [B §9] + the image and branch work).

| Milestone | Scope | Exit criteria | Tests |
|---|---|---|---|
| **M0 Spec, oracle, measurements** (small) | on-disk format spec v1 incl. records with epoch, refs, pins, `gitmap`; engine trait; the moirai workload on `redb` 4.x as oracle/throw-away backend (S0 of [22 §7.3]); measurements: Defender open-append-flush-close, loose-object create cost, blocking-lock contention with 16 writers, zstd dictionary ratio, Cyrillic token ratio, `git` presence/absence on the owner's PATH; the Workflow-hook experiment | numbers recorded; loose/pack threshold and lock bound fixed | bench harness (p50/p90/p99, private bytes, flush counts) |
| **M1 Engine + refs + branch overlays** (large) | log/HEAD/LOCK protocol (G1, X2, epoch), delta segments, tombstones with X4, I5′ PK, derived state, leases (runtime, D7), idempotency (I14′), refs/reflog/`ClientHead`, branch fork with pins, overlay build, promotion, `checkout`, `log`, `diff`, `show@`, `blame`, `undo`, `revert`, `tag`, GC with reflog expiry | open ≤ 3 ms at 1e6; 16-process Windows kill loop 1 h, zero lost acknowledged commits, `doctor --verify` clean on every branch; 50 branches × 2k ops each readable within budget | DST with crash/fsync/lock-delay injection at every I/O boundary; property tests: pin ⊕ ops == replay-from-genesis per branch; reverse == inverse(forward); invert(changeset) restores; fuzzed record/segment parsers |
| **M2 CLI, pack/brief, hooks, skills, import of standing rules** (medium) | the §7.1 read/write/coordination verbs on any branch, C's pack algorithm with §7.4 changes, SessionStart/UserPromptSubmit/SubagentStart/Stop hooks, `init`/`--link`/`worktree bind`, one-off import of the dozen standing rules, current pins, live lanes [22 §2.9] | one real campaign runs on `main` with no HDR and no hand-written resume block; `--json v1` frozen | golden outputs; PowerShell 5.1 argv tests; hook payload fixtures |
| **M3 Merge, rebase, cherry-pick, staging** (large) | typed 3-way with §5a.7 rules, validators incl. I5′ full Kahn, `merge/*` staging, `resolve`/`--continue` with rebase of resolutions, `sync`, `merge-check`, `--across` views, `plan/*` kinds, `lane open/close` | 10 synthetic lanes × 1k ops with every conflict class merge deterministically; the owner's two register incidents replay correctly; merge ≤ 50 ms at 1e5; a `plan/*` branch cannot mark work done | property tests: merge deterministic, disjoint keys commute, clean merge == sequential apply, no structural violation ever reaches a ref (I20′ fuzz) |
| **M4 Git image** (medium–large) | `.moi` encoder/decoder, tree/commit/tag encoders, loose + pack + bundle writer, loose + pack + `packed-refs` reader with deltas, `gitmap`, export (commit and checkpoint granularity), import (native + foreign + staged), `image doctor`, destinations, `--via fast-import` | export → import → export byte-identical for 1e5 nodes and 1e5 commits in SHA-1 and SHA-256; `git fsck` clean on the image; a hand edit, a git-side merge with markers, a file deletion and a squash each produce the specified result; full export of 1e5 ≤ 3 s | round-trip property tests; differential test against `git cat-file`/`git fsck`/`git log` when git is present; fuzzed `.moi` parser and pack reader |
| **M5 MCP** (medium) | rmcp dual-era, 10 tools, stamp on writes, role policy keyed on the dispatch label [22 §2.1], packaging | architect and critic complete a round on a lane branch without Bash; schema ≤ 5k chars; MCP private RSS ≤ 10 MB at 1e5 | conformance tests for both handshakes; `structuredContent` regression |
| **M6 Leader, change feed push, watch** (medium) | leader byte, pipe with DACL, forwarding with auto-generated keys (F-C5), pipelined group commit, broadcast, `watch` (leader-only, no polling) | follower tool p50 ≤ 1 ms; failover ≤ 500 ms with no lost write; 0 % idle CPU over 10 min | pipe fuzzing; leader kill loops |
| **M7 Search tier 2, schema extensions, deltified packs** (medium) | FST + postings per delta segment, strengthening migrations, `OFS_DELTA` in the image writer, `--with-oplog` | FTS ≤ 5 ms at 1e5; image growth ≤ 1/5 of undeltified | migration fuzz; pack differential tests |

Adoption path: M2 on `main` only (packs and briefs prove themselves on the oracle backend if M1 slips); M3 turns the next campaign's lanes into branches; M4 gives the owner the git image before MCP because R3 is a stated requirement and the image is also the backup story (`.moirai/` is otherwise lost with the clone).

---

## 10. Risks and the top five ways this design could fail

| # | Failure | Why plausible | Mitigation |
|---|---|---|---|
| 1 | **Branch isolation fights the workflow**: agents on lanes miss rules, completions and deletions made on `main` because nobody runs `sync`; the orchestrator dispatches from `main` and sees stale lane state | the reports all recommended a shared trunk for exactly this reason [08 §7.2], [02 §7.3]; the owner's lanes live for days | `~main` rules and owner rulings always in packs; `behind main` notices in every brief/hook; `sync` is one command and can be hooked to `SubagentStart`; `--across` views; leases are store-wide so double work is impossible even when views diverge; if a campaign shows chronic staleness, the T3′ fallback (an opt-in `shared` field class) is a small change |
| 2 | **The multi-process file protocol loses or corrupts an acknowledged write on Windows** (delayed lock release, Defender holding files, torn tail, epoch/restore confusion) | SQLite's WAL-reset race hid 16 years [08 §3.1]; Beads lost 7 of 8 closes [03 §2.7]; branches add pins and promotions to the surface | X2/F-A1 rules in the spec; DST with crash and lock-delay injection from M1; 16-writer kill loops as a release gate; pins and promotions are pure functions of immutable inputs; fsync error = abort |
| 3 | **Round-trip drift in the git image**: a canonicalisation corner (float formatting, escaping, sort order, tree entry order, CRLF) makes re-export differ, or a foreign commit is mis-classified as native | determinism across two implementations of the same rules is the classic failure of export formats; hg-git and cinnabar keep explicit maps because of it [S8][S9] | the exporter re-parses what it writes; `Moirai-Commit` verification demotes to foreign on any mismatch; byte-identical round-trip property tests in SHA-1 and SHA-256; fuzzed parser; format versioned in `.moirai-image` |
| 4 | **The path-copy tax floods a project repo** in `commit` granularity (1k commits/day × ~12 KB → GBs/year before `git gc`), or Defender makes loose writes slow enough that agents feel exports | [04 §3.1]; per-file Defender cost [05 §6.4] | packs above 64 objects; checkpoint granularity for project repos; separate image repo as the default; exports run under the maintenance byte, never in an agent's write path; M7 deltas |
| 5 | **Merge semantics creep and staged merges pile up**: every kind wants its own rule, and orchestrators leave `merge/*` refs unresolved because resolution needs judgement | [04 §11]; [21 §3.1 item 14] shows how easily a rule is wrong | the type set is closed (bool, int, counter, enum-with-lattice, text, set, ref); rules are data on the schema; `resolve --all --policy` for the common cases; the brief lists staged merges first; `merge-check` before every merge so most conflicts are seen on the lane |

Also tracked: SHA-256 images cannot yet be pushed to public servers [S4] (default SHA-1); gix is not used, so pack-format edge cases (multi-pack-index, bitmaps, `.rev`) are unsupported in the reader until seen (documented; `git repack -a -d` normalises); git's object names for the same moirai commit differ between SHA-1 and SHA-256 destinations (by design, both mapped); the `.moi` block-string rules must be fuzzed against pathological bodies; 50+ long-lived branches pin many checkpoint sets (`doctor` warns; `branch -d` releases).

---

## 11. Decisions the owner must make (value/scope calls), with recommended defaults

| # | Decision | Recommended default |
|---|---|---|
| 1 | **Default destination and granularity of the git image**: separate image repo with 1:1 commits, `refs/moirai/*` in the project repo, an orphan branch, or a tracked directory with checkpoints? | Separate bare repo `<parent of the main worktree>/<project>-moirai.git` at `commit` granularity, pushed to a private remote by the owner; optional daily checkpoints into the project repo under `refs/moirai/*`. |
| 2 | **Object format for the image**: SHA-1 (works with every remote today) or SHA-256 (future default, not yet safe on public servers)? | SHA-1 for anything that leaves the machine; SHA-256 allowed for local-only image repos. |
| 3 | **Branch-per-lane ritual**: must every worktree be bound to a moirai branch (`lane open`), or may agents write to `main` from any worktree? | `lane open` for every lane the merge queue will merge; scratch and `wf_*` worktrees write to `main`. |
| 4 | **Coordination liveness**: accept git-like isolation (lanes see `main`'s completions only after `sync`) with `--across` views, or add an opt-in `shared` field class (status/blocks live on `main` for all branches)? | Isolation + `--across` + hooked `sync` for one campaign; revisit. |
| 5 | **Leases store-wide (recommended) or per branch?** | Store-wide, keyed by `uid`, carrying the branch. |
| 6 | **Value conflicts land by default (jj) or `--strict` by default (Dolt)?** Structural violations never land either way. | Land; nodes leave `ready` until resolved. |
| 7 | **Reflog expiry and cruft delay** (how long an undone or deleted-branch commit stays recoverable). | 90 days / 14 days (git's defaults). |
| 8 | **Foreign commits from the git side**: import automatically on `image pull`, or stage every foreign commit for review? | Auto-apply clean foreign commits with actor `git:<email>`; stage on any violation. |
| 9 | **`--with-oplog` export** (HEAD moves and undo history to git)? | Off; the image carries data and commit history, not process history. |
| 10 | **May moirai spawn `git` for network push/pull when present**, or must transport stay entirely manual? | May spawn, prints the command otherwise. |
| 11 | **Bodies**: inline ≤ 64 KiB in `.moi` files (diffable) or external blobs? | Inline. |
| 12 | Unchanged from [B §11]: read-only roles writing through MCP (yes), auto-approve stamped calls (allow reads and lease-scoped writes), language (English; verbatim quotes as given), retention (forever), quiet-mode signalling (explicit flag wins), migration (import ~50 rules and open questions only), Dev Drive optional. | as stated |

---

## 12. Web sources fetched 2026-09-25 (precedents for the image)

- [S1] jj git compatibility — https://docs.jj-vcs.dev/latest/git-compatibility/ ("Commits created by `jj` have a ref starting with `refs/jj/` to prevent GC"; "Change IDs are stored in git commit headers … not preserved by all `git` tooling"; "Commits with conflicts cannot be represented in Git"; conflict trees kept only as `.jjconflict-*` directories with the truth in a `jj:trees` header).
- [S2] jj architecture — https://docs.jj-vcs.dev/latest/technical/architecture/ (GitBackend uses gitoxide; change id and predecessors in a `StackedTable` under `.jj/repo/store/extra/`; `refs/jj/keep/` per operation-log commit).
- [S3] git fast-import — https://git-scm.com/docs/git-fast-import (marks, `--import-marks`/`--export-marks`, incremental import into a populated repo with fast-forward checks, `from refs/heads/branch^0`, `M 100644 inline`, `reset`, `tag`, `N` notes, `gpgsig <algo>`).
- [S4] git hash-function transition — https://git-scm.com/docs/hash-function-transition (`extensions.objectFormat`/`compatObjectFormat`; "Until Git protocol gains SHA-256 support, using SHA-256 based storage on public-facing Git servers is strongly discouraged"; `loose-object-idx`; pack idx v3 for the compat map).
- [S5] gitoxide crate status — https://github.com/GitoxideLabs/gitoxide/blob/main/crate-status.md (loose-object and pack writing and ref transactions implemented; push "still need[s] plumbing"; "Git 3.0 compatibility (SHA-256, reftable)" listed as parity work).
- [S6] Dolt git remotes design — https://www.dolthub.com/blog/2026-02-19-supporting-git-remotes-as-dolt-remotes/ (`refs/dolt/data`; table files as blobs, large ones as `tablefile/0001…` sub-trees; `git commit-tree -p remoteHead`; `git push --force-with-lease` CAS loop; git plumbing CLI as the writer).
- [S7] git-bug data model — https://github.com/git-bug/git-bug/blob/master/doc/design/data-model.md (`refs/<namespace>/<id>`; `OperationPack` JSON blob at `/ops`; Lamport clocks as tree entry names; entity id = hash of the first operation; merge by clock then pack id).
- [S8] git-cinnabar metadata — https://github.com/glandium/git-cinnabar (via search: `refs/cinnabar/metadata` with hg2git/git2hg trees keyed `ab/cd/ef/REMAINDER`).
- [S9] hg-git mapfile — https://pypi.org/project/hg-git/ and https://github.com/akheron/git-hg/blob/master/git-hg (`.hg/git-mapfile`, space-separated `hg-sha git-sha` lines; lossless round trip claimed).
- [S10] Fossil mirror to GitHub — https://fossil-scm.org/home/doc/trunk/www/mirrortogithub.md (`.mirror_state` intermediate state so `fossil git export` is incremental; one-way).
- [S11] Sapling git support modes — https://sapling-scm.com/docs/git/git_support_modes/ (`.git/` compatible formats; Sapling-only data such as mutation kept under `.git/sl`).
- [S12] git pack format — https://git-scm.com/docs/gitformat-pack (`PACK` header, type/size varints, `OFS_DELTA`/`REF_DELTA`, idx v2 fan-out/CRC/offsets/large offsets, SHA-256 packs use 32-byte names).
- [S13] Pro Git, Git objects — https://git-scm.com/book/en/v2/Git-Internals-Git-Objects (`type size\0` + content, zlib, `objects/ab/cdef…`; tree entry and commit text formats).

*End of proposal D.*
