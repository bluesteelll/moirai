# 08 — Concurrency, live sync and interop with the project's real git repo

Research report for **moirai** (graph memory + task tracker + knowledge store for AI coding agents,
Rust, Windows 11 first, portable). Lens: transaction model across OS processes, daemon vs no daemon,
live change propagation to agents, branching in step with git, crash safety.
Date: 2026-09-25. Research only; nothing was implemented.

## 0. Conventions and method

Every factual statement carries one of these tags:

| Tag | Meaning |
|---|---|
| **[M]** | **Measured** by me on the owner's machine today (numbers, commands given in §2). |
| **[S]** | **Source-verified**: I read the code, header or spec text myself (URL + version given). |
| **[D]** | **Documented** by the maintainers or vendor. It is a primary source, but I did not verify the behaviour myself. |
| **[C]** | **Claimed** by third parties (issue reports, blog posts, articles). |
| **[I]** | **Inference**: my own reasoning. Treat it as a hypothesis to test. |

Sources are primary where possible: repos, source files, specs, vendor docs and maintainers' posts.
Versions and dates were checked on crates.io, GitHub releases and changelogs on 2026-09-25.
The user's relayed instruction was that only the *agentic workflow* of the owner's other repo
(BoykoEngine) is relevant, not its project content. §1 therefore looks only at workflow shape:
worktrees, branches, agents, processes and hooks.

---

## 1. Executive summary

1. **Throughput does not decide the architecture. Per-call latency, RAM duplication and staleness
   do.** The owner's workflow produces bursts of about 16 concurrent agents (the Workflow default cap) and
   about 19 git commits a day [M]. A single writer at about 2 ms per fsync [M] handles that easily. The
   costs that actually hurt are: process spawn per CLI call (34–74 ms [M], versus about 60 µs for a
   named-pipe round trip [M]); each long-lived process holding its own cache (16 `claude.exe`
   processes are running right now [M]); and agents acting on facts that are already stale in their
   context window.
2. **Every serious embedded store in Rust is single-process, with one important exception.** fjall
   3.x locks the database against multi-process access [D]. Turso does not support multi-process
   access [C]. Embedded Dolt (Beads) serialises processes with an exclusive file lock per operation [D].
   **redb 4.3.0 (2026-09-14)** added *experimental* multi-process read-write access. It uses a
   **byte-range-lock protocol with no shared memory**, and crashed processes need no stale-lock
   cleanup because "the OS automatically releases file locks when a process crashes" [S]. This is the
   best available blueprint for moirai's own file-level protocol.
3. **Beads is the closest precedent, and it swung between designs.** It went from SQLite plus JSONL
   in git plus a daemon (2025) to no daemon, with about 24k LOC deleted, and server-only Dolt (v0.50–0.56,
   Feb 2026). It then brought embedded Dolt back as the default (v0.63, Mar 2026; v1.0, Apr 2026). It
   removed the JSONL/merge-driver sync (about 70k LOC deleted, together with SQLite, tombstones and the
   3-way merge engine) and moved its data out of git branches entirely. Its worktrees now **share one
   store**, and cross-machine sync goes through `refs/dolt/data` [D][C]. A May 2026 proposal to
   re-introduce a daemon cites 150–230 ms per `bd` call and a Dolt server at 400–550 % CPU with 4 idle
   agents, caused by per-connection setup [C].
4. **Live propagation to agents is feasible today without research-preview features.** It works
   through **Claude Code hooks that call an MCP tool directly** (`type: "mcp_tool"`, so no process
   spawn) on `PostToolBatch`, `UserPromptSubmit` and `SubagentStart`. The hook returns
   `additionalContext`, which Claude reads before its next model request [D]. MCP resource
   subscriptions are specified (spec 2026-07-28, `subscriptions/listen`), but Claude Code documents no
   path from them into the model's context [D]. Channels (`notifications/claude/channel`) are a
   research preview; custom channels need a `--dangerously-load-development-channels` flag, and one
   bug report of non-delivery was closed "not planned" [D][C].
5. **A moirai branch should *not* automatically follow the git branch** for coordination data (tasks,
   claims, blockers). Evidence: the owner's repo has 44 worktrees, 107 local branches and several
   detached-HEAD worktrees [M]. Claude Code creates and deletes `worktree-<name>` branches on its own
   [D]. The owner's own research report names "no shared, live view of who owns what" across lanes as a
   pain point [C]. Beads and Taskmaster both refused automatic branch switching [D]. Knowledge nodes
   should instead carry **git provenance** (branch, base commit, worktree), with visibility computed
   from git ancestry. Explicit moirai branches remain for what-if planning.
6. **Recommended architecture: "C — embedded-first with an opportunistic leader".**
   - One store per repository in `$(git rev-parse --git-common-dir)/moirai/`. It is shared by all
     worktrees, including the ones under `<lanes-dir>\*`, and it is never in a working tree.
   - The file format and lock protocol are multi-process-safe on their own terms (single writer, MVCC
     readers, byte-range locks at a huge offset, two-phase commit).
   - Any long-lived moirai MCP server can take a "leader" lock byte. It then serves a named pipe for
     group commit, a warm cache and change broadcast.
   - The CLI forwards to the leader when one exists and opens the file directly when none does.
   - Push is an optimisation layered on a polled, monotonically increasing commit sequence, so
     correctness never depends on a daemon being alive.
   - Git interop is limited to (a) provenance metadata, (b) optional publishing of history as git
     objects under `refs/moirai/*` for backup and cross-machine sync, and (c) an optional *derived*
     text export for review. That export is never the sync channel.

---

## 2. Measured baseline on the owner's machine (2026-09-25)

Hardware and OS [M]: Windows 11 Home Single Language 10.0.26200; AMD Ryzen 9 5900HS (8C/16T);
15.4 GB RAM. `C:` and `D:` are both **NTFS** (`Get-Volume`); neither is a Dev Drive/ReFS. **Microsoft
Defender real-time protection is ON** (`Get-MpComputerStatus`: `RealTimeProtectionEnabled=True`,
`AMRunningMode=Normal`).

| Quantity | Value | How measured |
|---|---|---|
| Spawn and exit of a tiny native exe (`hostname.exe`) from PowerShell | **34.0 ms** avg (n=50) | `& hostname.exe` loop with Stopwatch |
| Same via `System.Diagnostics.Process.Start` | **54.4 ms** avg (n=50) | .NET Process API |
| `git --version` (Git for Windows wrapper) | **74.2 ms** avg (n=50) | loop |
| Named-pipe round trip, 32 bytes, PowerShell client and PowerShell server job | **≈60 µs** avg (n=2000), which *includes* about 5 µs of PowerShell loop overhead and PowerShell on the server side | `NamedPipeServerStream` / `NamedPipeClientStream` |
| Named-pipe connect (first) | 15 ms (includes JIT warm-up) | same |
| 4 KiB write + `FlushFileBuffers` (fsync), NTFS | **1.9–2.35 ms** (D: 1.96, C: 2.35) | `FileStream.Flush(true)`, n=200 |
| 4 KiB write with `FILE_FLAG_WRITE_THROUGH`, no flush | **0.21 ms** | `FileOptions.WriteThrough`. Whether this is power-safe depends on the device honouring FUA; treat durability as **unverified**. |
| `LockFileEx`/`UnlockFileEx` of 1 byte at offset 2^62 (far past EOF) | **1.9–9.6 µs** | `FileStream.Lock/Unlock`. Shows that lock bytes far past EOF work on NTFS. |

Owner's agentic workflow shape (BoykoEngine, metadata only) [M]:

| Fact | Value |
|---|---|
| `git worktree list` entries | **44**, under `.claude/worktrees/*` (harness-created, including `wf_*` Workflow worktrees) and `<lanes-dir>\*` (manual "lanes"; several named `mq-*`) |
| Detached-HEAD worktrees | several (for example two of the first four listed) |
| Local branches (`refs/heads`) | **107**; 55 remote-tracking refs |
| Commits in the last 30 days (all refs) | **561** (about 19 a day) |
| Running `claude` processes right now | **16**, 1.86 GB working set in total (about 116 MB each) |
| `rust-analyzer` working set right now | 988 MB WS / 2.9 GB private (1 process) |

Workflow facts from the sibling report `01-boyko-workflow-roles.md` [C, from that report]:
- 347 Workflow scripts. Lanes each have their own worktree plus branch plus base commit.
- Parallel developers work on disjoint files.
- "Timed measurements with **no** concurrent agent activity."
- One agent's kill by image name (`taskkill /IM cargo.exe`) would have killed three lanes' builds.
- "There is no shared, live view of who owns what, who is blocked on whom."

Relevant Claude Code runtime facts [D]:
- Workflow scripts run **up to 16 concurrent agents by default**, configurable up to 256
  ([workflows](https://code.claude.com/docs/en/workflows)).
- Subagents that reference an MCP server by name **share the parent session's connection**
  ([sub-agents](https://code.claude.com/docs/en/sub-agents)). One moirai MCP process therefore serves
  a whole session, including subagents running in *other* worktrees.
- Worktrees default to `.claude/worktrees/<name>` on branch `worktree-<name>`. They are swept after
  `cleanupPeriodDays`. Claude Code holds `git worktree lock` while an agent runs
  ([worktrees](https://code.claude.com/docs/en/worktrees)).
- The Bash sandbox is **not supported on native Windows**. On Linux and macOS it allows writes to the
  shared `.git` of a linked worktree, except `hooks/` and `config` inside it
  ([sandboxing](https://code.claude.com/docs/en/sandboxing)).

**Implications [I]:**
1. Any design that makes an agent's *every* read a fresh CLI process pays about 35–75 ms before doing
   any work. MCP calls and `mcp_tool` hooks avoid this.
2. Since one MCP process serves many worktrees, the MCP server cannot infer "which worktree or lane"
   from its own cwd. Tools and hooks must pass it explicitly.
3. Background activity by moirai (compaction, checkpointing) must be controllable ("quiet mode"),
   because the owner runs timing benchmarks that require no concurrent activity.
4. Any moirai process can be killed at any moment (`taskkill /IM moirai.exe`), so crash safety is a
   design requirement, not an edge case.

---

## 3. Transaction models for an embedded store shared by several OS processes

### 3.1 SQLite WAL — the reference model

- **Semantics [D]** ([wal.html](https://sqlite.org/wal.html)):
  - There is one writer at a time.
  - Readers do not block writers and writers do not block readers.
  - Each read transaction sees a fixed snapshot ("the end mark is unchanged for the duration of the
    transaction").
- **Shared memory [D]:** processes share a memory-mapped `-shm` wal-index. This is why WAL "does not
  work over a network filesystem".
- **Checkpoint starvation [D]:** "if … there is always at least one active reader, then no checkpoints
  will be able to complete and hence the WAL file will grow without bound."
- **Durability knob [D]** ([pragma.html](https://sqlite.org/pragma.html)): WAL with
  `synchronous=NORMAL` "might roll back following a power loss". `FULL` is ACID with an extra sync
  per commit.
- **Cross-process change detection [D]:** `PRAGMA data_version` changes when *another* connection
  commits. The value is local to a connection, and your own commits do not change it. `update_hook`
  is not cross-process ([forum](https://sqlite.org/forum/info/e4dd574a6a5d0d29)).
- **Lock placement [D]** ([fileformat2](https://www.sqlite.org/fileformat2.html)): SQLite reserves a
  *lock-byte page* at offset 1 GiB, historically because Windows used **mandatory** locking. The docs
  say modern OSes are advisory. On Windows, however, `LockFileEx` locks are *still* mandatory for
  `ReadFile`/`WriteFile` (see §4), so the precaution remains relevant there [S].
- **Corruption catalogue [D]** ([howtocorrupt](https://sqlite.org/howtocorrupt.html)):
  - POSIX `close()` cancels every lock the process holds on the file.
  - Two copies of SQLite in one process break lock coordination.
  - Deleting a hot journal corrupts.
  - Copying a live database file produces a torn copy.
  - A Windows-only recovery race (fixed 3.7.16.2).
- **WAL-reset bug.** A checkpoint and a concurrent WAL reset race, causing silent permanent
  corruption. It needs two or more connections in separate threads or processes. It was present from
  **3.7.0 (2010) through 3.51.2 (2026-01-09)** and fixed in **3.51.3 (2026-03-13)**
  [D][C] ([howtocorrupt](https://sqlite.org/howtocorrupt.html);
  [Tailscale](https://tailscale.com/blog/sqlite-wal-reset-bug)). Antithesis reports reproducing it
  "within 15 minutes" with a generic concurrent-write-plus-checkpoint workload under deterministic
  simulation ([Antithesis, 2026-08-12](https://antithesis.com/blog/2026/wal-reset-bug/)) [C].
  **Lesson [I]:** the hardest part of a multi-process store is the *coordination* protocol, not the
  B-tree. It needs deterministic simulation from day one.

### 3.2 LMDB — shared reader table and memory map

Source: LMDB `lmdb.h` (mdb.master) and `mdb.c` (0.9 branch, **0.9.36, "Aug 6, 2026"**)
([lmdb.h](https://github.com/LMDB/lmdb/blob/mdb.master/libraries/liblmdb/lmdb.h),
[mdb.c 0.9](https://github.com/LMDB/lmdb/blob/mdb.RE/0.9/libraries/liblmdb/mdb.c)).

- **Model [S]:** "Writes are fully serialized; only one write transaction may be active at a time …
  readers run with no locks; writers cannot block readers." Copy-on-write means "no special recovery
  procedures after a system crash".
- **Reader table [S]:** readers live in a lock file whose slots default to 126 (`mdb_env_set_maxreaders`).
  - "Stale reader transactions left behind by an aborted program cause further writes to grow the
    database quickly." The fix is a periodic `mdb_reader_check`.
  - Stale *writers* are cleared automatically on Windows.
  - "Avoid long-lived transactions."
  - "Do not use LMDB databases on remote filesystems."
  - "Do not have open an LMDB database twice in the same process."
- **Windows file growth [S].** In 0.9.x, `mdb_env_map` sets the file length to the **full map size**
  up front: "Windows won't create mappings for zero length files and won't map more than the file
  size. Just set the maxsize right now." `mdb.master` instead uses the undocumented `NtCreateSection`
  with `SEC_RESERVE` to avoid that.
- **Lesson [I]:** a shared-memory reader table needs stale-slot detection, and a dead reader pins old
  pages until it is detected. Memory maps on Windows fight file growth and shrinking (§4).

### 3.3 redb — from single-process to a byte-range-lock multi-process protocol

- **History [S]** ([CHANGELOG](https://github.com/cberner/redb/blob/master/CHANGELOG.md)):
  - The mmap backend was replaced by a safe file backend in 0.11 and **removed in 0.14 (2023-03-26)**.
  - `ReadOnlyDatabase` (several reader processes, under a shared lock that excludes writers) arrived
    in **3.0.0 (2025-08-09)**.
  - **4.3.0 (2026-09-14)** added "experimental support for multi-process read-write access … behind
    the `experimental-multiprocess` feature flag".
  - The latest crate is 4.3.0, dated 2026-09-15 on crates.io.
  - Earlier, the maintainer closed a multi-process request as a duplicate
    ([#932, 2025-01-06](https://github.com/cberner/redb/issues/932)) [D].
- **Protocol [S]** ([design.md](https://github.com/cberner/redb/blob/master/docs/design.md), "Multi-process concurrency"):
  - Modes are *Exclusive writer*, *Single writer* (one writing process, many reading processes) and
    *Multi-writer*.
  - Lock bytes sit at a base of **2^62**: writer byte, shared-writer byte, shared-reader byte,
    whole-file reader byte, a header lock at `0..320`, and an **active-transaction range** in which a
    reader holds a *shared lock on byte `TXN_BASE + txn_id`*.
  - To find the oldest live reader, a writer takes the header lock exclusively and "scans the active
    transaction range stopping at the first byte it cannot lock exclusively".
  - Cache invalidation: "observing a new committed transaction id means that all pages the process
    does not have pinned … may have been freed", and redb drops its entire cache at that point.
  - **Crashes:** "The OS automatically releases file locks when a process crashes. No further recovery
    is required beyond the normal database repair process."
  - Restrictions in multi-process modes: 2-phase commit is mandatory ("to ensure that committed pages
    are flushed before the transaction becomes visible to another process"), `Durability::None` is
    refused, and read-only processes skip corruption detection on open.
- **Measured:** the lock-byte-at-2^62 idea works on NTFS at 2–10 µs per lock/unlock [M].
- **Lesson [I]:** this design has no shared memory, no reader table and no stale-slot sweeper. Dead
  processes release their reader registration automatically, which is exactly the property moirai
  needs on Windows. The cost is that every process invalidates its cache wholesale on each foreign
  commit, so per-process caches become ineffective under steady cross-process writes. That argues for
  one long-lived cache owner (§8, architecture C).

### 3.4 Other engines

| Engine | Multi-process story | Source |
|---|---|---|
| **fjall 3.x** (3.0.0 on 2026-01-02; 3.1.10 on 2026-08-30) | "uses Rust's new file locking API to exclusively lock a database to protect it from multi-process access (which is not supported)" | [D] [fjall 3 post](https://fjall-rs.github.io/post/fjall-3/), crates.io |
| **Turso** (Rust SQLite rewrite) | MVCC `BEGIN CONCURRENT` (preview); "Multi-process access is not supported" | [C] [Turso blog](https://turso.tech/blog/concurrent-writes-on-turso-cloud) |
| **Embedded Dolt** (Beads default since v0.63) | each op "opens the Embedded Dolt engine, executes work within a SQL transaction, then closes the engine. Concurrent callers block until acquiring the lock"; Beads 1.3.0 added `*.gate.lock` files (shared for normal commands, exclusive for maintenance) | [D] [DoltHub 2026-04-02](https://www.dolthub.com/blog/2026-04-02-restoring-beads-classic/); Beads CHANGELOG |
| **Dolt sql-server** (Beads "server mode") | real multi-writer, but auto-commit per write is **off** because "Firing DOLT_COMMIT after every write under concurrent load causes 'database is read only' errors" | [D] [beads docs/architecture/dolt.md](https://github.com/gastownhall/beads/blob/main/docs/architecture/dolt.md) |
| **jj (Jujutsu)** | **no locks at all**. Operations and views are content-addressed objects; concurrent operations create divergent op-log heads that are later 3-way merged ("no changes to the repo will be lost … conflicting changes … will appear as conflicts"). It is designed to survive even Dropbox/NFS sync. | [D] [jj concurrency](https://docs.jj-vcs.dev/latest/technical/concurrency/) |

### 3.5 Comparison for moirai's needs

| Model | Readers | Writers | Crash of a participant | Windows risk | Fit for moirai |
|---|---|---|---|---|---|
| SQLite WAL (shm + byte locks) | MVCC snapshot | 1 | recovery on next open; shm rebuilt | mmap'd shm; checkpoint starvation by long readers | Proven, but brings a C dependency and SQL. It is a fallback, not the "from scratch" goal. |
| LMDB (shared reader table, mmap) | MVCC, lock-free | 1 | stale reader slots until `reader_check` | file preallocated to mapsize (0.9); cannot truncate mapped file | Fast reads, but slot hygiene and mmap growth problems |
| redb multi-process (byte-range locks) | MVCC | 1 (or N serialized) | **OS releases locks; nothing to sweep** | wholesale cache invalidation | **Best blueprint for the file protocol** |
| Exclusive lock per operation (embedded Dolt, fjall, redb default) | one process at a time | 1 | lock released | high contention from many short CLI calls | Too coarse for 16 concurrent agents plus long-lived MCP servers |
| jj lock-free op log | snapshot of an op | any; conflicts merged later | nothing to recover (content-addressed) | none special | A strong idea for the **history/branch layer** (§6), too loose for "claim is atomic" semantics |

---

## 4. What breaks on Windows (and the mitigations)

| # | Hazard | Evidence | Mitigation for moirai |
|---|---|---|---|
| W1 | **Byte-range locks are mandatory.** An exclusive `LockFileEx` region "denies all other processes both read and write access"; a shared lock denies writes to everyone, including the locker. However, "Locking a region of a file does not prevent reading or writing from a mapped file view." | [S] [LockFileEx](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-lockfileex) | Put lock bytes **far past any data** (redb uses 2^62; SQLite uses 1 GiB). "Locking a region that goes beyond the current end-of-file position is not an error" [S]. Measured working at 2^62 [M]. |
| W2 | **Lock release after process death is not instant.** "the time it takes for the operating system to unlock these locks depends upon available system resources." | [S] same page | Lock acquisition uses bounded retry with jitter. Never treat "lock busy" as "writer alive" for longer than a timeout. Report the holder when it is known. |
| W3 | **Sharing violations on rename/replace.** Defender, the Search indexer, Explorer preview and OneDrive open files without `FILE_SHARE_DELETE`, so an atomic replace fails with `ERROR_ACCESS_DENIED` (5) or `ERROR_SHARING_VIOLATION` (32) | [C] multiple issue reports, e.g. [hardy #332](https://github.com/charlesmsiegel/hardy/issues/332), [patina #67](https://github.com/kvnxiao/patina/issues/67) | No "write temp file, then rename over" on the commit path; commit in place inside one preallocated file. For exports, use Rust ≥ **1.85** `std::fs::rename` (POSIX-semantics rename when available, [PR #131072](https://github.com/rust-lang/rust/pull/131072) [S]) plus bounded retry on errors 5 and 32. |
| W4 | **Memory maps versus file size.** You cannot map a zero-length file or map beyond EOF (LMDB comment [S]). `SetEndOfFile` fails with `ERROR_USER_MAPPED_FILE` (1224) while any process has a view mapped, so compaction or truncation needs every process to unmap | [S] LMDB mdb.c 0.9.36; [C] [realm-core #1569](https://github.com/realm/realm-core/issues/1569), [swesonga](https://blog.swesonga.org/2023/01/07/cannot-truncate-mapped-file-in-windows/) | Use **pread/pwrite plus a user-space cache** (redb removed mmap for safety [S]). Grow the file in large extents. Shrink only in exclusive/offline maintenance. |
| W5 | **Antivirus scanning** of new or changed files and of new executables. Defender real-time protection is on [M]. Beads documents Kaspersky PDM false positives on `bd.exe` and ships PE version info and Authenticode signing to reduce them | [M]; [D] [beads antivirus.md](https://github.com/gastownhall/beads/blob/main/docs/reference/antivirus.md) | Keep a **small, stable set of files** (one data file, one lock/meta file). Do not use one file per node or per commit. Sign release binaries and embed PE metadata. Optionally document Dev Drive with Defender performance mode (asynchronous scanning, ReFS; ≥ 50 GB; "up to 30 %" build gains [C]) ([Dev Drive](https://learn.microsoft.com/en-us/windows/dev-drive/)). |
| W6 | **Process creation is expensive**: 34–74 ms [M] | [M] | The hot paths (MCP tools, `mcp_tool` hooks) must not spawn processes. The CLI stays for humans, scripts and Bash-based agents. |
| W7 | **No AF_UNIX in Rust std on Windows.** The PR to add it was closed over licensing ([#147335](https://github.com/rust-lang/rust/pull/147335)). Windows has had AF_UNIX since 10 (2018), but crates wrap named pipes: `interprocess` "local sockets" are named pipes on Windows and UDS elsewhere; tokio has `net::windows::named_pipe` | [D] [interprocess docs](https://docs.rs/interprocess/latest/interprocess/local_socket/index.html) | Named pipes on Windows, UDS on Unix, behind one abstraction. |
| W8 | **Named-pipe security.** The default DACL gives "read access to members of the Everyone group and the anonymous account". A malicious squatter can create the pipe first | [S] [CreateNamedPipe](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-createnamedpipea) | Set an explicit DACL (current user SID only), `FILE_FLAG_FIRST_PIPE_INSTANCE`, `PIPE_REJECT_REMOTE_CLIENTS`. Put the user SID and repo hash in the pipe name. Clients verify the server PID/owner. |
| W9 | **Daemons spawned from agent harnesses die with the job object.** "the daemon inherits the parent process's job object membership. When the harness terminates the calling process tree, the job object kill cascades" | [C] [Tencent BrowserSkill #268, 2026-09-17](https://github.com/Tencent/BrowserSkill/issues/268) | If a detached daemon is used: `CREATE_BREAKAWAY_FROM_JOB` + `DETACHED_PROCESS`, which may still be refused by the job's limits. Better: no separate daemon (architecture C). |
| W10 | **Directory watching loses events.** `ReadDirectoryChangesW` discards the *whole* buffer on overflow, with `lpBytesReturned = 0` | [D] [ReadDirectoryChangesW](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-readdirectorychangesw) | A file watcher can only be a *doorbell* ("something changed, re-read the sequence number"), never the change feed itself. |
| W11 | **Network, cloud-synced and removable locations.** WAL "does not work over a network filesystem"; LMDB forbids remote FS | [D] | Detect and refuse (or degrade to exclusive mode) on network drives or OneDrive-managed folders. |
| W12 | **Even Claude Code's own Windows daemon has named-pipe lifecycle bugs.** The supervisor runs but "never creates its control named pipe"; the service then "idles for 50 seconds, then exits (cycle repeats)" | [C] [anthropics/claude-code #66483 (2026-06-09, open)](https://github.com/anthropics/claude-code/issues/66483) | Evidence that daemon lifecycle on Windows is a real reliability cost (§5). |

Durability primitives on Windows [S]:
- `FlushFileBuffers` flushes file data and metadata. Microsoft warns it "can be inefficient when used
  after every write" and suggests `FILE_FLAG_NO_BUFFERING | FILE_FLAG_WRITE_THROUGH` for many small
  critical writes ([FlushFileBuffers](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-flushfilebuffers)).
- `MoveFileEx(MOVEFILE_WRITE_THROUGH)` only guarantees flushing for a *copy-and-delete* move
  ([MoveFileEx](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-movefileexa)).
- Measured: flush about 2 ms, write-through about 0.2 ms [M]. A two-fsync commit therefore costs
  about 4 ms, capping one serial writer at about 250 durable commits/s unless commits are grouped [I].

---

## 5. Daemon architectures

### 5.1 Survey

| System | Process model | Auto-start | Idle shutdown | IPC | Notes |
|---|---|---|---|---|---|
| **watchman** | one persistent server per user | yes: the client "will attempt to start it if it doesn't exist"; `--no-spawn` opts out | watches are reaped after `idle_reap_age_seconds` (default 432000 = 5 days); the server itself persists | UDS; named pipes on Windows ("the closest equivalent") | [D] [cli-options](https://facebook.github.io/watchman/docs/cli-options), [config](https://facebook.github.io/watchman/docs/config) |
| **git fsmonitor--daemon** | one per repository working directory (socket or pipe rendezvous in the git dir) | yes, when `core.fsmonitor=true` ("will … automatically start it") | none documented | Simple-IPC: named pipe on Windows, UDS in `.git` elsewhere | [D] [git-fsmonitor--daemon](https://git-scm.com/docs/git-fsmonitor--daemon), [simple-ipc](https://git.github.io/htmldocs/technical/api-simple-ipc.html) |
| **Bazel** | server per output base | yes | `--max_idle_secs` default **10800 s (3 h)** | local socket/gRPC; clients block on the output-base lock (`--block_for_lock`) | [D] [Bazel CLI ref](https://bazel.build/reference/command-line-reference) |
| **sccache** | local server | yes | `SCCACHE_IDLE_TIMEOUT` default **600 s**; builds that idle longer lose the server | TCP localhost | [C] (docs / issues [#204](https://github.com/mozilla/sccache/issues/204)) |
| **rust-analyzer** | **one server per editor workspace**, nothing shared | spawned by the editor | exits with the editor | stdio LSP | about 1 GB WS right now [M]; reports of 5–22 GB [C] ([#19402](https://github.com/rust-lang/rust-analyzer/issues/19402)). Shows the cost of per-client in-memory state. |
| **jj** | **no daemon**; optional watchman for fsmonitor | n/a | n/a | n/a | correctness from lock-free op log [D] |
| **Beads ≤ v0.49** | `bd daemon` per workspace on `.beads/bd.sock` | yes (client "spawns a daemon and waits for the socket") | yes | UDS; **Windows problems** ("daemon daemonization failures, silent daemon crashes", [#1379](https://github.com/steveyegge/beads/issues/1379), closed not planned) | [C] |
| **Beads v0.50+** | daemon and RPC **removed** (~24k LOC); CLI uses embedded Dolt directly | — | — | — | [C] [vscode-beads #65](https://github.com/jdillon/vscode-beads/issues/65) |
| **Beads proposal #3760 (2026-05-06)** | opt-in `bd serve` on a UDS, **CLI falls through to direct execution if the socket is absent** | opt-in | — | UDS | prototype claims connection rate 41 → 0.4 per second and Dolt CPU 393–557 % → 30–80 % [C] ([#3760](https://github.com/gastownhall/beads/issues/3760)) |
| **Claude Code background sessions (Windows)** | supervisor daemon on `\\.\pipe\cc-daemon-<id>-control` | yes | "idles for 50 seconds, then exits" | named pipe | open lifecycle bug [C] (#66483) |

### 5.2 Pros and cons for moirai

**For a long-lived owner process** [I, grounded in the measurements above]:
- One cache instead of N; the redb-style wholesale invalidation disappears.
- **Group commit** amortises the ~4 ms two-fsync cost across agents.
- Real **push** of change events.
- Warm in-memory secondary indexes (reverse edges, ready-set, full text) without rebuilding them per
  process.
- One place to enforce leases, claims and quotas.
- About 60 µs IPC versus 34–74 ms spawn.

**Against a separate daemon binary with its own lifecycle:**
- Auto-start races; the Windows job-object kill (W9); stale or squatted pipes (W8).
- **Version skew** between client and daemon after `cargo install`.
- Antivirus heuristics on long-running unsigned binaries (W5).
- The UDS sandbox restriction on Linux and macOS: sandboxed Bash needs `allowUnixSockets`
  ([sandboxing](https://code.claude.com/docs/en/sandboxing)) [D].
- Idle-timeout tuning (sccache users hit a 600 s default [C]).
- A single point that `taskkill /IM` wipes out [C, owner workflow].
- The Beads history: the daemon was removed after instability, then a daemon was proposed again for
  performance [C].
- Background activity conflicts with the owner's "no concurrent activity during timed measurements"
  rule [C].

**The key observation [I]:** Claude Code *already* runs a long-lived process per session, namely the
**stdio MCP server**. It lives exactly as long as the session and is shared by all its subagents [D].
Making one of these the leader gives the daemon's benefits without a separately managed daemon
lifecycle. This is architecture C.

**RAM when idle [I, must be measured]:** a Rust process with an empty cache is expected to use only a
few MB of private memory. The dominant factor is the configured cache size multiplied by the number of
processes that keep one. With 16 sessions [M], per-process caches of 16 MB would cost about 256 MB. A
leader with a 64 MB cache plus followers with about 1 MB caches would cost about 80 MB.

---

## 6. Live change propagation: "node 40 was deleted by someone else"

### 6.1 Three layers, three different guarantees

| Layer | Who can be stale | What "synchronous" can mean | Mechanism |
|---|---|---|---|
| **L1 — store** | nobody: every transaction reads one consistent snapshot | **Referential integrity inside the deleting transaction**: edges to 40 are removed or tombstoned atomically, and the change log gets `{seq, op=delete, node=40, affected=[12,17], actor}` in the *same* commit. There is never a dangling edge at the head. | engine invariant plus commit sequence |
| **L2 — process caches** (CLI, MCP servers) | a process's cached pages or indexes | **monotonic reads**: each new read transaction starts at the latest committed seq; caches are invalidated by seq comparison | poll the header seq (one 4 KiB read), a doorbell, or leader push |
| **L3 — agent context windows** | the model, which read node 12 "blocked by 40" ten turns ago | **bounded staleness**: the agent learns of the change no later than its next tool batch or prompt; **every write is validated against the head** | hooks with `additionalContext`, plus compare-and-set writes |

L3 can never be "immediate": an LLM context cannot be invalidated. The achievable contract [I] has two
parts:
- **(a) no write based on a stale read can succeed silently.** Every mutation carries the expected
  version or preconditions, and on mismatch it gets an error that explains *what changed and who
  changed it*.
- **(b) relevant changes are pushed into the context before the agent's next model request.**

Beads converged on the same pattern:
- In 1.3.0: "Compare-and-Set Updates: Atomic conditional updates using `--if-assignee` and
  `--if-status` flags with exit code 13 distinguishing guard mismatches".
- Also in 1.3.0: "Claims now carry expiring leases with heartbeat" ([releases](https://github.com/gastownhall/beads/releases), v1.3.0 2026-09-15) [D].
- For machine consumers, Beads has an **events journal**: "Every committed issue mutation writes one
  ordered record, in the same transaction as the mutation itself, and a consumer reads those records
  with a cursor". `bd events tail --follow` "polls once a second" ([events-journal.md](https://github.com/gastownhall/beads/blob/main/docs/reference/events-journal.md)) [D].

### 6.2 Mechanisms available in the Claude Code harness (verified in docs, 2026-09)

| Mechanism | Status | Latency | Reliability and caveats | Source |
|---|---|---|---|---|
| **`mcp_tool` hook on `PostToolBatch`** returning `additionalContext` | GA | at the end of every tool batch, "before Claude Code sends the next request to the model" | no process spawn: it calls a tool on the already-connected moirai MCP server; `input` supports `${…}` substitution from the hook JSON, e.g. `${cwd}`, `${session_id}`; output capped at 10,000 chars; fires once per batch, not per tool | [D] [hooks](https://code.claude.com/docs/en/hooks) |
| `mcp_tool` hook on `UserPromptSubmit` / `SubagentStart` / `Stop` | GA | at each prompt, subagent start, or stop | `SessionStart` `mcp_tool` hooks are **skipped at launch** ("no MCP client context"), so use a `command` hook there | [D] same |
| `async` hook / `asyncRewake` hook | GA | an async hook's output arrives "on the next conversation turn"; `asyncRewake` "wakes Claude on exit code 2" with stderr as a system reminder | an `asyncRewake` hook that long-polls moirai (`moirai wait --relevant-to <agent>`) could wake an *idle* session; its `timeout` is enforced (default 600 s). **Untested idea [I].** | [D] same |
| `FileChanged` hook | GA | on file change | "No decision control. Used for side effects" (no `additionalContext`); only good as a doorbell | [D] same |
| MCP `list_changed` | supported by Claude Code | refreshes the tools/prompts/resources lists | carries only list changes, not data | [D] [mcp](https://code.claude.com/docs/en/mcp) |
| MCP resource subscriptions | in spec 2026-07-28 (`subscriptions/listen`, replacing `resources/subscribe`) | `notifications/resources/updated` carries **only the URI** | "On stdio, if the connection is terminated … the server holds no subscription state", so updates are lossy across reconnects; Claude Code documents resources only as @-mention attachments, with **no documented delivery of updates to the model** | [D] [spec resources](https://modelcontextprotocol.io/specification/latest/server/resources), [subscriptions](https://modelcontextprotocol.io/specification/2026-07-28/basic/patterns/subscriptions) |
| **Channels** (`notifications/claude/channel`) | research preview; requires v2.1.80+ | immediate, even into an idle session | custom servers need `--dangerously-load-development-channels`; Team/Enterprise orgs must enable them; not registered for servers negotiating MCP revision 2026-07-28; bug report of silent non-delivery on Windows closed "not planned" (Apr 2026) | [D] [channels](https://code.claude.com/docs/en/channels); [C] [#45563](https://github.com/anthropics/claude-code/issues/45563) |
| Workflow scripts (JS) | GA | between phases | the orchestrator script can query moirai itself | [D] [workflows](https://code.claude.com/docs/en/workflows) |

**Recommended propagation path [I]:**
1. The writer commits and the change log gets seq N.
2. The leader (or each MCP server, by polling the header seq every 50–250 ms, or when woken by a
   doorbell) notices seq N.
3. At the session's next `PostToolBatch`, an `mcp_tool` hook calls
   `moirai.delta(since=<cursor>, cwd=${cwd}, session=${session_id})`.
4. The server computes a **relevance-filtered** digest, for example: "node 40 deleted by agent X at
   14:02; it was blocking your task 12, so 12 is now READY; your claim on 17 expires in 3 min". It
   returns this as `additionalContext` and advances the cursor.

The cursor key is (session, agent), stored in the MCP server's memory. On restart it falls back to
"since the session's last seen seq", which is persisted in the store.

**Why not the file watcher as the feed [S]:** `ReadDirectoryChangesW` drops the whole buffer on
overflow (W10). A monotonically increasing sequence plus a change log is the only lossless feed. File
events, named events and pushes are just "go look now" signals.

### 6.3 Deletion semantics that make "everyone knows" true at L1

- **Tombstone plus atomic edge rewrite** [I]:
  - `delete 40` writes a tombstone (id, deleted_by, seq, reason) and removes or marks every incident
    edge *in the same transaction*.
  - Reverse adjacency must be indexed, so the incident set is O(degree) and does not require a scan.
  - The change log records the affected neighbours so L3 can target them.
- **Why keep tombstones:** without them, merges cannot distinguish "deleted" from "never existed".
  Beads learned this with JSONL, whose import "is upsert-only; it cannot infer that records absent from
  an export were deleted" ([sync-concepts.md](https://github.com/gastownhall/beads/blob/main/docs/core-concepts/sync-concepts.md)) [D].
- **Restrict versus cascade** is a policy per edge type. For example, `blocks` could cascade to
  "unblocked", while `part_of` could refuse deletion of a parent that still has open children unless
  `--cascade` is given. That is the graph analogue of SQL foreign-key `RESTRICT`/`CASCADE` [I].

---

## 7. Branching the store versus git worktrees and branches

### 7.1 How other tools store metadata alongside git

| Tool | Where the data lives | Follows git branch? | Merge model | Documented pain |
|---|---|---|---|---|
| **Beads classic** (2025 – Feb 2026) | SQLite cache plus `.beads/issues.jsonl` committed in the tree; optional hidden `sync.branch` worktree under `.git/beads-worktrees/` | JSONL followed branches | custom driver `bd merge %A %O %A %B` via `.gitattributes`; 3-way merge by id; tombstones | JSONL, DB and worktree diverging on Windows ([#1379](https://github.com/steveyegge/beads/issues/1379)); stale worktrees blocking checkouts; orphaned `merge.beads.*` config after uninstall ([#1710](https://github.com/gastownhall/beads/issues/1710)); "the daemon's `--auto-commit --auto-push --auto-pull` flags don't automate the complete workflow" ([tiby.fr, 2026-03-22](https://tiby.fr/articles/i-built-a-distributed-issue-tracker-i-didnt-need)) [C]. **All of it deleted (~70k LOC)** [C]. |
| **Beads now** (v1.x) | embedded Dolt in `.beads/embeddeddolt/` (gitignored), **one store shared by all worktrees** | **No**: "Issue changes are stored in Dolt, not committed to the current Git branch" | Dolt cell-level merge; cross-clone via `bd dolt push/pull` to `refs/dolt/data` on the *same* git remote | `refs/dolt/data` is not fetched by plain clones, so `git push --mirror` from such a clone **deletes it** ([#5266](https://github.com/gastownhall/beads/issues/5266)) [C]; a non-atomic multi-step migration left workspaces half-migrated ([gastown #1302](https://github.com/steveyegge/gastown/issues/1302)) [C] |
| **Dolt git remotes** (v1.81.10, 2026-02-13) | DB chunks as git blobs under a custom ref; local bare-repo cache; push via `--force-with-lease` CAS loop | n/a | CAS retry: "if the push fails (someone else pushed first), loop back to fetch" | "this ref will not even be cloned, fetched, or pulled by Git" [D] ([announce](https://www.dolthub.com/blog/2026-02-13-announcing-git-remote-support-in-dolt/), [design](https://www.dolthub.com/blog/2026-02-19-supporting-git-remotes-as-dolt-remotes/)) |
| **git-bug** (v0.11.0, 2026-09-22) | each entity is a chain of commits under `refs/bugs/<id>`, each carrying an OperationPack | no (own refs) | **operation log** ordered by Lamport clocks, then by pack id; conflict-free by construction | [D] [data model](https://github.com/git-bug/git-bug/blob/master/doc/design/data-model.md) |
| **git-appraise** (last push 2023-08) | git notes under `refs/notes/devtools/*`, one JSON object per line | no | `cat_sort_uniq` notes merge, conflict-free for line-atomic records | [D] [README](https://github.com/google/git-appraise) |
| **Fossil tickets** | ticket-change artifacts inside the repo DB | n/a | replay in timestamp order; per-field replace or append; ticket tables "can always be reconstructed" | [D] [tickets.wiki](https://fossil-scm.org/home/doc/trunk/www/tickets.wiki) |
| **Radicle COBs** | `refs/cobs/<type>/<id>`, a commit DAG per object | no | union of graphs, reduced in topological (causal) order | [D] [protocol guide](https://radicle.dev/guides/protocol) |
| **jj** | op log in `.jj/`; change-id written as a git **commit header** (since 0.30) | its own model | 3-way merge of op heads | the header "is not preserved through a rebase operation" by git [C] ([PR #6162](https://github.com/jj-vcs/jj/pull/6162)) |
| **Taskmaster** | `tasks.json` in the tree, with "tagged task lists" | **manual**: `add-tag --from-branch`, "no automatic tag switching" (listed as a future enhancement) | textual; conflicts resolved with a `move` command | [D] [task-master docs](https://github.com/eyaltoledano/claude-task-master/blob/main/docs/tutorial.md) |
| **Claude Code Tasks / agent teams** | `~/.claude/tasks/{team}/` (outside the repo) | no | "Task claiming uses file locking to prevent race conditions"; dependencies auto-unblock | [D] [agent-teams](https://code.claude.com/docs/en/agent-teams) |

**Git-level facts that constrain any in-tree approach [S/D]:**
- Custom merge drivers are defined in `.git/config`, so the driver definition is **not cloned**. The
  built-in `union` driver "tends to leave the added lines in the resulting file in random order … Do
  not use this if you do not understand the implications" ([gitattributes](https://git-scm.com/docs/gitattributes)).
- **GitHub's server-side merge does not run custom merge drivers**, and the PR stays "CONFLICTING"
  ([community #9288](https://github.com/orgs/community/discussions/9288) and repeated issue reports) [C].
- Per-worktree versus shared refs: every `refs/` is shared except `refs/bisect`, `refs/worktree` and
  `refs/rewritten`. `HEAD` is per worktree. `git rev-parse --git-common-dir` locates the shared
  `.git` ([git-worktree](https://git-scm.com/docs/git-worktree)) [D].
- If moirai publishes many refs, the **reftable** backend (Git 2.45+) gives "atomic reference updates
  that scale with the size of the reference update" ([Git 2.45 highlights](https://github.blog/open-source/git/highlights-from-git-2-45/)) [D].

### 7.2 Should a moirai branch follow the git branch automatically?

**Arguments for:**
- Knowledge such as decisions and rules often describes *code as of a branch*.
- An abandoned branch's notes should not pollute `main`.
- A git merge would carry the notes along.

**Arguments against, with evidence:**

1. **Coordination state is cross-branch by nature.** The orchestrator in `main`'s worktree assigns
   tasks to agents in `<lanes-dir>\lane-x`. A blocker resolved in lane x must unblock lane y *now*, not
   after a git merge. The owner's own pain point is "no shared, live view" across lanes [C].
2. **Branch identity is unstable in the owner's workflow.** Several worktrees are detached HEAD [M].
   Claude Code auto-creates `worktree-<name>` branches and deletes them on cleanup [D]. Workflow
   worktrees `wf_*` are short-lived [M]. Auto-follow would constantly create and orphan moirai
   branches.
3. **Silent context switches.** `git switch` inside a worktree would change what the agent
   "remembers" mid-task.
4. **Git operations do not map to DB merges.** Rebase, squash, cherry-pick and GitHub web merges run
   no local hooks (post-merge runs only for local merges).
5. **Precedent.** Beads moved from branch-following JSONL to "Issue changes are stored in Dolt, not
   committed to the current Git branch" [D]. Taskmaster kept tag switching manual [D].

**Recommendation [I]: no automatic following. Use provenance plus ancestry-based visibility instead.**
- One **live timeline** (moirai `main`) is shared by all worktrees. All tasks, claims, blockers and
  leases live there.
- Every node and every write records **git provenance**: `{worktree path, branch or "detached",
  HEAD sha, base commit, lane id, agent/session}`. The CLI gets this from cwd; MCP tools get it from
  `${cwd}` in hook input or an explicit `lane` argument.
- Knowledge nodes can carry a **scope**: `global`, or `code@<branch|commit>`. Queries resolve
  visibility against git. A node scoped to branch `feat/x` at commit `c` is shown as "applies" in a
  worktree whose HEAD contains `c` (`git merge-base --is-ancestor`, answered through a cached ancestry
  index). Otherwise it is shown as "pending on feat/x". When `feat/x` is merged into `main`, detected
  lazily by ancestry, the node becomes effective for everyone with no data movement. When the branch
  is deleted unmerged, the node becomes "orphaned scope" and is queued for human review, not deleted.
- **Explicit moirai branches** (Dolt-like) stay available for what-if planning, speculative
  decomposition and "try a refactor plan". They are created and merged deliberately; a lane *may*
  choose to pin one.

### 7.3 Where to store the live store

| Option | Pros | Cons |
|---|---|---|
| **G1 — `$GIT_COMMON_DIR/moirai/`** (for example `<repo>\.git\moirai\`) **[recommended default]** | Shared by every linked worktree automatically, including the manual `<lanes-dir>\*` lanes. Never shows in `git status`; `git clean -fdx` and `git worktree remove` cannot touch it. Precedent for tools keeping state in `.git`: fsmonitor's socket, Beads' old `.git/beads-worktrees`. The Linux/macOS sandbox allows writes to the shared `.git` except `hooks/` and `config` [D]. | Lost if the clone is deleted, so backup/publishing is needed (G3). Not visible in PR review. Tools that copy `.git` copy the DB too; it must be copy-safe or excluded. |
| **G2 — in-tree text as source of truth** (JSONL or file-per-node plus merge driver) | Reviewable in PRs; travels with branches. | Each worktree holds its *own* copy, so agents in different lanes do not see each other until a merge (fails requirement 1 above). Merge driver not cloned; GitHub merges ignore it. Commit churn. Many small files mean more antivirus work (W5). Deletions need tombstones. **Beads deleted this path** [C]. |
| **G3 — git objects under `refs/moirai/*`** (git-bug, Dolt `refs/dolt/data`, git-appraise style) **[recommended for backup and cross-machine sync]** | Uses the existing remote and credentials. Content-addressed and immutable. No working-tree noise. Pushing is a CAS (`--force-with-lease`), as in Dolt. | Not fetched by default; needs a refspec (`+refs/moirai/*:refs/moirai/*`). **`git push --mirror` from a clone without them deletes them** [C]. Needs pack hygiene. |
| **Per-user store** (`%LOCALAPPDATA%\moirai\<repo-id>`) | Survives deleting the clone; one location for cross-repo views. | Identifying the repo is fragile (root-commit hash versus remote URL versus path). Diverges from the repo's lifetime. |

**Optional derived export** (for PR review and LLM-readable diffs) [I]:
- A deterministic, sorted, one-record-per-line export, regenerated by the CLI and marked
  `linguist-generated`.
- Conflicts are resolved by **regenerating** it, never by merging. Beads' current docs describe JSONL
  the same way: "an export … not the canonical cross-machine sync channel" [D].

---

## 8. Crash safety and corruption recovery

### 8.1 Evidence base

- **ALICE (OSDI 2014)** found **60 vulnerabilities** in 11 widely used systems, including databases
  and version-control systems, because application crash-consistency protocols rely on file-system
  "persistence properties" that vary between file systems
  ([USENIX](https://www.usenix.org/conference/osdi14/technical-sessions/presentation/pillai)) [D].
- **"Can Applications Recover from fsync Failures?" (ATC 2020)** found that none of PostgreSQL, LMDB,
  LevelDB, SQLite or Redis handled fsync errors adequately. It reports corruption or data loss, and
  LMDB reporting failures when the on-disk state was actually correct
  ([USENIX](https://www.usenix.org/conference/atc20/presentation/rebello)) [D].
- SQLite's WAL-reset race went undetected for 16 years; deterministic simulation found it in minutes
  [C] (§3.1).
- redb's stated assumptions: atomic single-byte writes, durable after `fsync`, and "powersafe
  overwrite". It uses dual commit slots plus checksums; two-phase commit is *mandatory* in its
  multi-process modes [S].
- The owner's workflow kills processes wholesale (`taskkill /IM …`), and Claude Code "re-signals"
  agent processes when a run stops [C][D].

### 8.2 Principles for moirai [I]

1. **One preallocated data file, copy-on-write pages, two meta slots with checksums and a monotonic
   transaction id.** Always use two-phase commit when more than one process can observe the file: data
   fsync, then meta fsync. Recovery means taking the newest meta slot whose checksum and page checksums
   verify. There is no log to replay unless the engine chooses a WAL.
2. **No shared-memory coordination state.** All inter-process coordination goes through **byte-range
   locks at 2^62+** (the redb protocol). A dead process's locks vanish with it (with a delay, W2), so
   there is nothing to sweep and no PID-file truth.
3. **Treat fsync failure as fatal:** abort the process, re-open and recover. Never retry the fsync
   and continue (fsyncgate lesson, ATC 2020).
4. **Durability classes per operation.** Claims, deletions and decisions are durable before
   acknowledgement. Heartbeats, lease renewals and read-cursor updates can be *group-committed* or
   batched lazily. This is the SQLite `synchronous=NORMAL` trade-off, applied selectively.
5. **Graph-level invariants are checked at commit** and verified by `moirai doctor`:
   - no edge to a missing or tombstoned node at the head;
   - the `blocks` graph is acyclic;
   - `part_of` is a forest;
   - every reverse index matches its forward edges;
   - change-log seqs are contiguous.
   A post-merge result that violates an invariant becomes a **violation record** that must be
   resolved before the merge commits. This is the Dolt analogue: "Dolt can produce invalid merges even
   after conflicts are resolved … this merge will not be able to be committed until the foreign key
   violations are resolved" ([Dolt conflicts](https://www.dolthub.com/docs/concepts/dolt/git/conflicts)) [D].
6. **Backups are snapshots at a transaction boundary, never raw copies of a live file.** SQLite lists
   copying a DB mid-transaction as a corruption cause [D]. G3 publishing (§7.3) doubles as an
   off-machine backup.
7. **Refuse unsafe locations** (W11) and **never use the mmap write path** (W4).
8. **Testing:**
   - deterministic simulation of N writers and readers with injected crashes at every I/O boundary
     (ALICE-style crash-state enumeration);
   - fsync-error injection;
   - on Windows: loops that kill random processes (`TerminateProcess`) with 16 concurrent writers;
   - AV-style interference (a process that opens the data file without `FILE_SHARE_DELETE`);
   - lock-release-delay simulation.

---

## 9. Candidate architectures

The engine internals are covered elsewhere; this section fixes only the *process and sync* shape. In
all three, the git interop is G1 plus optional G3, plus provenance-based scoping (§7).

### A — Embedded multi-process, no long-lived coordinator

```
 claude session 1 ── stdio ── moirai-mcp ─┐
 claude session 2 ── stdio ── moirai-mcp ─┤   byte-range locks @2^62 (writer byte,
 agent Bash ─────────────── moirai CLI ───┼──► reader bytes per txn id, header lock)
 hooks (command) ────────── moirai CLI ───┘         │
                                                    ▼
                    <git-common-dir>/moirai/store.db   (+ change-log table, seq in header)
```

- Every process opens the file directly. Writers take the writer byte (bounded wait with jitter);
  readers register on active-transaction bytes.
- Each process caches pages and invalidates them on a foreign seq.
- Change detection: poll the header seq at hook time. There is no cross-process push.

### B — Single-owner daemon, thin clients

```
 moirai-mcp / CLI / hooks ──named pipe \\.\pipe\moirai-<sid>-<repohash> (UDS on Unix)──►  moirai-d
                                                                                            │ exclusive lock
                                                                                            ▼
                                                                                         store.db
```

- The daemon is auto-started by the first client (detached, with breakaway from the job object) and
  stops after an idle timeout with no clients and no subscriptions.
- The engine can be single-process only (simpler): an in-process MVCC, one cache and group commit.
- Pushes events to subscribed MCP servers.
- When the daemon is unreachable, clients either start it or fail. Read-only direct open is possible
  only if no daemon holds the exclusive lock.

### C — Embedded-first with an opportunistic leader (hybrid) **[recommended]**

```
 claude session 1 ── moirai-mcp (LEADER: holds leader byte, serves pipe, group commit, warm cache, broadcast)
 claude session 2 ── moirai-mcp (follower: forwards writes; tiny cache; receives events; also polls seq)
 agent Bash / CLI ─┬─ leader pipe present?  yes → forward (µs IPC)
                   └─ no  → open store directly (architecture A protocol)
 all processes ───► <git-common-dir>/moirai/store.db  (A's lock protocol is always in force)
```

- Correctness is A's multi-process protocol. **The leader is only an optimisation.**
- Leadership is a byte-range lock. The first long-lived process to take it becomes leader: normally
  the first moirai MCP server of any session, or an explicit `moirai serve` for CLI-only use. There is
  no separate daemon lifecycle, no auto-spawn and no job-object problem. The leader lives exactly as
  long as its Claude session.
- **Leader death**: the lock is released and another MCP server takes it within the retry interval.
  Followers reconnect, and followers never *depend* on push because they also poll the seq.
- Writes from followers are forwarded, so the leader group-commits them. A follower whose pipe breaks
  retries, then writes directly under A's protocol. Leader and direct writers are serialised by the
  same writer byte, so they can never both write.
- Propagation: the leader broadcasts `{seq, node ids, kinds}` to followers. Each session's
  `PostToolBatch` `mcp_tool` hook pulls a relevance-filtered delta (§6.2).
- A "quiet" flag suspends background compaction and checkpointing during the owner's timed benchmarks.

### 9.1 Trade-off table

Numbers are from §2 [M]; "est." means an estimate [I].

| Criterion | A — embedded, no leader | B — daemon | C — embedded + leader |
|---|---|---|---|
| MCP tool read latency | in-process: µs to ms (warm cache); cold after each foreign commit | IPC about 60 µs + query | leader: in-process; follower: about 60 µs + query |
| CLI call latency | spawn 34–74 ms + open + cold cache | spawn 34–74 ms + IPC; the daemon's warm cache helps queries | spawn + IPC if a leader is up, else as A |
| Durable write cost | about 4 ms (2 fsyncs) per commit, serialised; no grouping across processes | group commit: about 4 ms shared by all concurrent writers | group commit through the leader; about 4 ms solo when no leader |
| Writer contention with 16 agents | writer-byte queue; fine at this workload, poor tail latency | none (queue inside the daemon) | none via the leader; A-style queue when no leader |
| Push to agents | none; hooks poll the seq | real push | real push (leader) plus polling fallback |
| RAM with 16 sessions | N × cache (e.g. 16 × 16 MB = 256 MB est.) | 1 × cache + thin clients (est. ≤ 80 MB) | 1 leader cache + N tiny caches (est. ≈ 80 MB) |
| Engine complexity | **high**: multi-process protocol (redb 4.3 needed years to ship it experimentally) | **low**: single-process MVCC | **high** (same as A) plus a forwarding RPC |
| Operational complexity | lowest | highest: auto-start, idle timeout, version skew, job objects, pipe security, AV | medium: pipe security and version handshake; no lifecycle management |
| Failure blast radius | one process | the daemon is a single point; `taskkill /IM moirai.exe` stops all service until restart | the leader moves; direct mode always works |
| Windows specifics | W1–W6, W10, W11 | W1, W5–W9, W12 | W1–W8, W10, W11 (no W9) |
| Portability | best | needs UDS on Unix; sandbox needs `allowUnixSockets` | the CLI works without a pipe (sandbox-safe fallback) |
| Precedent | SQLite WAL, redb multiprocess, Beads embedded (coarser) | watchman, Bazel, sccache, Beads ≤ 0.49, Claude Code bg daemon | Beads #3760 proposal ("falls through to direct execution"); git fsmonitor (optional accelerator) |

### 9.2 Failure-mode analysis

| Scenario | A | B | C |
|---|---|---|---|
| **Writer process killed mid-commit** (`taskkill`, session stop) | Uncommitted pages are unreachable. The OS releases the writer byte, possibly after a delay (W2). The next writer verifies the meta slot and continues. | The daemon dies: every client errors, then reconnects and restarts it (auto-start), with recovery on open. In-flight group-commit batches are lost and clients must retry idempotently. | If the leader dies: as B for its in-flight batch, but followers fall back to direct mode immediately and a new leader emerges. A direct writer dies: as A. |
| **Power loss** | Two-phase commit plus checksummed meta slots; operations marked lazy-durable may roll back. | same | same |
| **fsync returns an error** | process aborts; next open recovers | daemon aborts; restart | leader aborts; failover |
| **Two processes start at once** | irrelevant | auto-start race: needs a first-pipe-instance plus lock-file winner check | leader election by lock byte; the loser becomes a follower |
| **Antivirus or indexer holds the data file open** | no effect on in-place writes; open must allow `FILE_SHARE_READ\|WRITE\|DELETE`; exports retry on errors 5 and 32 | same | same |
| **Long-running reader** (an MCP server holding a snapshot) | blocks page reuse and compaction (redb refuses compaction while a foreign read transaction is active) | the daemon controls snapshot lifetimes | the leader controls them; followers must keep read transactions short (per request) |
| **Stale agent context** (acts on a deleted node 40) | CAS/precondition failure with an explanatory error; the next hook delta explains | same | same |
| **Agent's subagent in another worktree calls a shared MCP tool** | the MCP server cannot know the worktree, so tools require `lane`/`cwd` arguments; hooks pass `${cwd}` | same | same |
| **Git branch switched in a worktree** | provenance of new writes changes; scoped-knowledge visibility recomputed; no data moves | same | same |
| **Worktree removed while an agent runs** | store unaffected (it lives in the common dir); the lane record is marked gone via a `WorktreeRemove` hook or lazy check | same | same |
| **Clone deleted or moved to another machine** | data lost unless G3 was pushed; `moirai bootstrap` re-hydrates from `refs/moirai/*` | same | same |
| **`git push --mirror` from a clone without `refs/moirai/*`** | remote history deleted (the Dolt/Beads precedent); needs a doctor check plus a documented refspec | same | same |
| **moirai upgraded while running** (format or protocol change) | old processes may hold locks; a format version in the header plus refusal-on-newer is required | **version skew**: the daemon must detect a newer client and restart itself | the leader advertises its protocol version; a newer follower refuses to forward and runs direct |
| **Store on OneDrive or a network drive** | must refuse (locks and fsync semantics) | same | same |
| **Owner runs timed benchmarks** | no background work | the daemon may compact or checkpoint at a bad time | the leader honours a "quiet" flag; followers do no background work |
| **Linux/macOS sandboxed Bash** | works (file writes into the shared `.git` are allowed) | UDS blocked unless allowed | falls back to direct mode |

---

## 10. Recommendation and staging (research-level)

1. **Adopt C**, but stage it:
   - **Stage 1** is A's file format and lock protocol, used by the CLI and by one MCP server per
     session. Design it as multi-process from day one, because retrofitting is the hard part (redb
     took from 0.x to 4.3 to ship it as experimental).
   - **Stage 2** adds the leader pipe (group commit, broadcast, shared cache).
   - **Stage 3** adds G3 publishing to `refs/moirai/*` and the derived export.
2. **Make the on-disk format "open and query instantly"**: persisted forward and reverse edge indexes,
   ready-set and status indexes, and no load phase. Then the direct path costs only the spawn when
   there is no leader. This is exactly the per-invocation cost Beads/Dolt suffered from (150–230 ms
   per call [C]).
3. **Propagation contract** (§6):
   - L1: atomic referential integrity plus a same-transaction change log with a monotonic seq.
   - L2: caches keyed by seq.
   - L3: `PostToolBatch` / `UserPromptSubmit` / `SubagentStart` `mcp_tool` hooks delivering
     relevance-filtered deltas.
   - Every write carries preconditions (expected version or status).
   - Claims are leases with heartbeats renewed by the same hooks.
4. **Git**: G1 location; provenance on every write; scope-by-ancestry for knowledge; no
   auto-following; explicit moirai branches for planning only; a refspec plus a doctor check against
   `push --mirror` deletion.
5. **Windows hygiene**:
   - lock bytes at 2^62+;
   - pread/pwrite, no mmap writes;
   - one preallocated file grown in large extents;
   - pipe DACL restricted to the current user, first instance only, remote clients rejected;
   - bounded retries on errors 5 and 32;
   - signed binaries with PE metadata;
   - refuse network and OneDrive paths;
   - a quiet mode for benchmarks.
6. **Verification before trusting any of it**:
   - deterministic multi-process simulation with crash and fsync-error injection;
   - a Windows kill-loop test with 16 concurrent writers;
   - measure idle RAM of the MCP server and leader, and the p50/p99 latency of CLI calls and hook
     deltas, on this machine.

---

## 11. Open questions (owner decisions)

See the structured output. In brief:
- Is knowledge ever branch-specific enough to justify moirai branches bound to git branches, or is
  provenance plus ancestry scoping sufficient?
- Should moirai data ever leave the machine (G3 to the GitHub remote), or stay local-only?
- One store per repository or one per user?
- The acceptable durability window for low-value writes.
- Is a leader running inside the MCP server acceptable, or should a separate daemon be avoided or
  required entirely?
- Is Linux/macOS support with the sandbox a real target?
- Would the owner format a Dev Drive?

---

## 12. Sources (accessed 2026-09-25)

**Storage engines and papers**
- SQLite WAL — https://sqlite.org/wal.html
- SQLite: how to corrupt — https://sqlite.org/howtocorrupt.html
- SQLite PRAGMA — https://sqlite.org/pragma.html
- SQLite file format (lock-byte page) — https://www.sqlite.org/fileformat2.html
- SQLite forum on data_version — https://sqlite.org/forum/info/e4dd574a6a5d0d29
- Antithesis, "Breaking the WAL" (2026-08-12) — https://antithesis.com/blog/2026/wal-reset-bug/
- Tailscale on the WAL-reset bug — https://tailscale.com/blog/sqlite-wal-reset-bug
- LMDB `lmdb.h` (mdb.master) — https://github.com/LMDB/lmdb/blob/mdb.master/libraries/liblmdb/lmdb.h
- LMDB `mdb.c` (0.9 branch, 0.9.36, Aug 6 2026) — https://github.com/LMDB/lmdb/blob/mdb.RE/0.9/libraries/liblmdb/mdb.c
- redb design.md — https://github.com/cberner/redb/blob/master/docs/design.md
- redb CHANGELOG (4.3.0, 2026-09-14) — https://github.com/cberner/redb/blob/master/CHANGELOG.md
- redb issue #932 — https://github.com/cberner/redb/issues/932
- fjall 3 announcement — https://fjall-rs.github.io/post/fjall-3/ (release dates from crates.io: 3.0.0 on 2026-01-02, 3.1.10 on 2026-08-30)
- Turso concurrent writes — https://turso.tech/blog/concurrent-writes-on-turso-cloud
- jj concurrency — https://docs.jj-vcs.dev/latest/technical/concurrency/
- jj change-id header, PR #6162 — https://github.com/jj-vcs/jj/pull/6162
- ALICE (OSDI 2014) — https://www.usenix.org/conference/osdi14/technical-sessions/presentation/pillai
- fsync failures (ATC 2020) — https://www.usenix.org/conference/atc20/presentation/rebello

**Windows and Rust platform**
- LockFileEx — https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-lockfileex
- CreateNamedPipe — https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-createnamedpipea
- MoveFileEx — https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-movefileexa
- FlushFileBuffers — https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-flushfilebuffers
- ReadDirectoryChangesW — https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-readdirectorychangesw
- Dev Drive — https://learn.microsoft.com/en-us/windows/dev-drive/
- Rust PR #131072, POSIX rename (1.85) — https://github.com/rust-lang/rust/pull/131072
- Rust PR #130999, `File::lock` (stable in 1.89) — https://github.com/rust-lang/rust/pull/130999
- Rust PR #147335, Windows UDS (closed) — https://github.com/rust-lang/rust/pull/147335
- interprocess local sockets — https://docs.rs/interprocess/latest/interprocess/local_socket/index.html
- tokio named pipes — https://docs.rs/tokio/latest/tokio/net/windows/named_pipe/index.html
- Mapped-file truncation: https://github.com/realm/realm-core/issues/1569 and https://blog.swesonga.org/2023/01/07/cannot-truncate-mapped-file-in-windows/
- AV and sharing-violation reports: https://github.com/charlesmsiegel/hardy/issues/332 and https://github.com/kvnxiao/patina/issues/67
- Job-object kill of a daemon — https://github.com/Tencent/BrowserSkill/issues/268
- Claude Code Windows daemon pipe bug — https://github.com/anthropics/claude-code/issues/66483

**Daemons**
- watchman — https://facebook.github.io/watchman/docs/cli-options and https://facebook.github.io/watchman/docs/config
- git fsmonitor--daemon — https://git-scm.com/docs/git-fsmonitor--daemon
- git Simple-IPC — https://git.github.io/htmldocs/technical/api-simple-ipc.html
- Bazel command-line reference — https://bazel.build/reference/command-line-reference
- sccache idle shutdown — https://github.com/mozilla/sccache/issues/204
- rust-analyzer memory — https://github.com/rust-lang/rust-analyzer/issues/19402

**Claude Code and MCP**
- Hooks — https://code.claude.com/docs/en/hooks
- MCP — https://code.claude.com/docs/en/mcp
- Channels — https://code.claude.com/docs/en/channels
- Sub-agents — https://code.claude.com/docs/en/sub-agents
- Worktrees — https://code.claude.com/docs/en/worktrees
- Agent teams — https://code.claude.com/docs/en/agent-teams
- Workflows — https://code.claude.com/docs/en/workflows
- Sandboxing — https://code.claude.com/docs/en/sandboxing
- Channel non-delivery bug — https://github.com/anthropics/claude-code/issues/45563
- MCP spec 2026-07-28, resources — https://modelcontextprotocol.io/specification/latest/server/resources
- MCP spec 2026-07-28, subscriptions — https://modelcontextprotocol.io/specification/2026-07-28/basic/patterns/subscriptions

**Beads and Dolt**
- Beads repo (v1.3.0 on 2026-09-15; v1.3.1-rc.1 on 2026-09-21) — https://github.com/gastownhall/beads and https://github.com/gastownhall/beads/releases
- Beads docs (raw, main):
  - `docs/reference/worktrees.md`
  - `docs/architecture/dolt.md`
  - `docs/core-concepts/sync-concepts.md`
  - `docs/reference/events-journal.md`
  - `docs/multi-agent/coordination.md`
  - `docs/reference/antivirus.md`
  - `docs/recovery/merge-conflicts.md`
- DoltHub, "Restoring Beads Classic" (2026-04-02) — https://www.dolthub.com/blog/2026-04-02-restoring-beads-classic/
- Beads discussion #2332 — https://github.com/steveyegge/beads/discussions/2332
- vscode-beads #65 (daemon removal) — https://github.com/jdillon/vscode-beads/issues/65
- gastown #1302 — https://github.com/steveyegge/gastown/issues/1302
- Beads #1379 (Windows) — https://github.com/steveyegge/beads/issues/1379
- Beads #3760 (`bd serve` proposal) — https://github.com/gastownhall/beads/issues/3760
- Beads #5266 (`push --mirror`) — https://github.com/gastownhall/beads/issues/5266
- Beads #1710 (orphaned merge-driver config) — https://github.com/gastownhall/beads/issues/1710
- "I Built a Distributed Issue Tracker I Didn't Need" (2026-03-22) — https://tiby.fr/articles/i-built-a-distributed-issue-tracker-i-didnt-need
- beads_rust — https://github.com/Dicklesworthstone/beads_rust
- Dolt git remotes announcement (2026-02-13) — https://www.dolthub.com/blog/2026-02-13-announcing-git-remote-support-in-dolt/
- Dolt git remotes design (2026-02-19) — https://www.dolthub.com/blog/2026-02-19-supporting-git-remotes-as-dolt-remotes/
- Dolt conflicts — https://www.dolthub.com/docs/concepts/dolt/git/conflicts

**Git-native metadata**
- git-bug data model — https://github.com/git-bug/git-bug/blob/master/doc/design/data-model.md
- git-appraise — https://github.com/google/git-appraise
- gitattributes — https://git-scm.com/docs/gitattributes
- git-worktree — https://git-scm.com/docs/git-worktree
- GitHub `merge=union` discussion — https://github.com/orgs/community/discussions/9288
- Git 2.45 highlights (reftable) — https://github.blog/open-source/git/highlights-from-git-2-45/
- Fossil tickets — https://fossil-scm.org/home/doc/trunk/www/tickets.wiki
- Radicle protocol guide — https://radicle.dev/guides/protocol
- Taskmaster tutorial — https://github.com/eyaltoledano/claude-task-master/blob/main/docs/tutorial.md
