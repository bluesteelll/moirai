---
title: "Under the hood: Klotho"
nav: The engine
description: How Klotho, the storage engine inside the moirai binary, stores the graph, lets many agent processes share it without a server, survives crashes and is held to its budgets.
---

# Under the hood: Klotho

Moirai is one program, `moirai`. Inside it is [Klotho](glossary.html#term-klotho), a storage engine written from scratch for one job: keep a small, versioned graph on local disk, open it in about a millisecond, and let many agent processes read and write it at the same time without a server. This chapter explains where the data lives, how a write becomes durable, what happens after a crash, and which budgets the engine is designed to meet.

> [!NOTE]
> **Designed, not built yet**
>
> The engine is built in milestone M1, after the on-disk format version 1 is frozen; every number below is a design target. See [Where it stands now](building.html#status).

## One binary with the engine inside {#one-binary}

Moirai is designed as one static Rust binary. The command-line interface, the hooks that a harness calls and the MCP server are all the same `moirai` executable, started with different arguments. Klotho is a library compiled into that binary. It is not a separate service, and nothing else has to be installed or started.

Klotho is not a wrapper around an existing database: the project uses no SQLite and no third-party storage engine anywhere. The log, the segments, the lock protocol, the version control and the git object layer are hand-written in Rust. When the engine crates are created, they take `klotho-*` names; the product, the binary and the command stay `moirai`.

## Where the store lives {#store-location}

A [store](glossary.html#term-store) is a directory. `moirai init` places it where agents find it without extra setup:

| Where you run `moirai init` | Where the store goes |
|---|---|
| inside a git repository (any worktree) | `<git-common-dir>/moirai/`: outside every working tree, so `git add -A` never picks up store files, and shared by all worktrees of the repository without any pointer files |
| in the top-level directory of the main worktree, with `--here` | `.moirai/` in that directory, added to git's local exclude file |
| outside any git repository | `./.moirai/` |
| in any directory, with `--link STORE` | no new store; a small pointer file `.moirai` that names an existing one |

Every command finds its store in the same order: an explicit `--store DIR`, the `MOIRAI_DIR` environment variable, a walk up the directory tree looking for `.moirai`, and finally a hint from the enclosing git repository. Discovery never creates a store; if nothing is found, the command lists every path it examined.

A store is used only on a local file system that the crash tests cover; on Windows that is NTFS. Network drives, cloud-synced folders such as OneDrive, FAT, exFAT, ReFS and paths into WSL are refused with the reason named. A store is touched by one operating-system kernel only, because byte-range locks do not work across a kernel boundary.

### The files in a store {#store-files}

A store normally holds about a dozen files: `HEAD`, `LOCK`, `config`, up to four log extents, one base and up to three delta segments, and possibly a compression dictionary. Other files are added as history is retired, bodies are sealed and branches are promoted. Apart from `HEAD`, `LOCK`, `config`, the log extents and the `tmp/` and `trash/` directories, every file is *sealed*: written once, made durable, marked read-only on disk and never changed again.

| File | What it holds | How it is read |
|---|---|---|
| `HEAD` | two 4 KiB checksummed slots: the current set of segment files, the bound up to which readers may read the log, counters | at fixed offsets; never mapped |
| `LOCK` | 36 KiB of diagnostics and session records; the lock bytes themselves lie beyond the end of the file | never mapped |
| `config` | the store's configuration keys, in git-config syntax | as text |
| `log.N` | the append-only log, in 64 MiB extents; at most four are active | explicit reads and writes; never mapped |
| `seg.base.G`, `seg.dK` | the current state: one base segment and up to three delta segments | mapped read-only |
| `seg.b<ref_id>.K` | the segment of a long-lived branch that has been promoted | mapped read-only |
| `hist.N` | retired log extents, compressed, with a commit index | mapped read-only |
| `blobs.N`, `dict.D` | long text bodies, content-addressed and compressed; the compression dictionary | mapped on first use |
| `gitmap.N` | the map between moirai commit ids and git object ids | mapped read-only |
| `cs.N` | the changeset of one very large commit | mapped as one more delta layer |
| `tmp/`, `trash/` | temporary files; project files moved aside by `moirai file rm --trash` | never mapped |

A few rules hold for all of them. The store grows by creating new files. No file is truncated, and no file is renamed over while a process may have it mapped. A file is deleted only after `HEAD` has pointed elsewhere for a grace period of 60 seconds and no branch or tag still pins it. There are no per-node and no per-commit files, so the number of files does not grow with the size of the graph.

<details>
<summary>Why only sealed files are memory-mapped</summary>

Memory mapping lets every process read the same bytes straight from the operating system's page cache, with no copying. On Windows, however, a mapped file cannot be resized, and a mapped view is not kept consistent with ordinary writes to the same file. Klotho therefore maps only files that never change again, and reads and writes the log with explicit I/O calls. Every sealed file states its own length in its header, and the length is checked against the real file size before mapping.

</details>

## History in a log, current state in segments {#log-and-segments}

Klotho keeps two views of the same data.

- **The [log](glossary.html#term-log) is the canonical history.** Every change is appended as a record with a 32-byte header: its length, its position in the log (the log sequence number, or LSN), the store's [epoch](glossary.html#term-epoch) (a random number chosen when the store is created) and an xxh3 checksum. A record is valid only at its own position and only in its own store. Nothing in the log is rewritten in place.
- **The [segments](glossary.html#term-segment) hold the current state** of the main branch in columns: node headers, titles and fields, the outgoing and incoming edges of every node in compact arrays, precomputed bitsets that answer questions such as "which tasks are unblocked", a symbol table, and references to bodies. Segments are immutable. Checkpoints produce them from the log.

Between two checkpoints, the newest part of the log, the *tail*, is not yet in any segment. Each process replays the tail into a small in-memory [overlay](glossary.html#term-overlay). To find a node, a reader looks in the overlay first, then in the delta segments from newest to oldest, then in the base segment.

This split makes opening a store cheap. Opening reads `HEAD`, maps at most eight segment files, and replays a tail whose length is capped by the checkpoint threshold. No step reads the whole history, so the cost of opening does not grow with the length of the history. The design target for opening a store, with a warm cache and a full tail, is at most 1 ms at 10,000 nodes and at most 3 ms at 1,000,000 nodes.

<details>
<summary>How small a commit is</summary>

A commit stores a net changeset: the operations it performed, each with its before-image, so that every commit can be inverted for undo and revert. A typical small agent commit, with one parent, a short message and a few operations, is estimated at 0.35 to 0.5 KB before compression. Designs that copy whole pages of a tree for each commit pay kilobytes for every tiny change; a changeset pays bytes.

About 99 bytes per node are read by most queries; they sit in the page cache that all moirai processes share.

</details>

## Content addressing {#content-addressing}

Every commit has an id that is the BLAKE3-256 hash of a [canonical form](glossary.html#term-canonical-form) of the commit. The canonical form is a specified byte sequence: the commit's kind, its parents, its clock value, who made it, the git commit it was made against, its message, the schema version, the commit it reverts or cherry-picks and the git commit it was imported from (each left empty when there is none), and a digest of its net changeset. Two stores that hold the same commit compute the same id, and the git image carries every field needed to recompute the id after a round trip through git. Data that belongs to one store only, such as the store's own id, is kept out of the hashed form.

Long text bodies are content-addressed too, by a 128-bit BLAKE3 hash, so an identical body is stored once. The chapters [Branches, merges and history](versions.html) and [Saving into git](git-image.html) show how these ids are used.

## Many processes, no server {#process-model}

There is no daemon and no server process that holds the graph in memory. Every `moirai` command that an agent runs starts a process that opens the store, does its work and exits. The [MCP server](glossary.html#term-mcp-server) that a harness session starts is the same binary in a long-running mode. To the engine it is one more client: it maps the same files and takes the same locks.

| Process | Lifetime | What it does with the store |
|---|---|---|
| `moirai` command or hook | milliseconds | reads directly; takes the writer lock only for the commit step |
| `moirai mcp` | one agent session | keeps its mappings and a small, size-capped cache of branch views; does routine maintenance in slices of at most 5 ms between requests |
| `moirai gc` | one maintenance job | writes a new base segment (a rollup) and collects garbage; started explicitly or as a detached low-priority child |

**Readers take no locks.** They read the immutable segments through memory maps, so the operating system's page cache holds one copy of the hot data for all processes on the machine. They replay the log only up to the bound published in `HEAD`, so a reader never sees a durable change before the flush that covers it.

**Writers take turns.** At most one process at a time holds the writer lock: an exclusive lock on one byte of the `LOCK` file. On Windows this is `LockFileEx`; on Linux and macOS it is an open-file-description lock. A writer waits for the lock for a bounded time, set by a configuration key (2 seconds in the design; an M0 measurement fixes the default). If the wait runs out, the command fails with a message that names the process holding the lock, instead of hanging.

**Nothing polls in the background.** There are no timers, no file watchers and no polling loops. The only maintenance that outlives a command is the rollup child described [below](#maintenance): a process starts it, detached, when a threshold is crossed, and it does its one job and exits. The MCP server has at most two operating-system threads, and both are blocked while it is idle. Idle CPU use is zero by construction.

<details>
<summary>Why not a daemon</summary>

A daemon that keeps the graph in RAM would use memory equal to the data set even when no agent is working, which breaks the memory budget, and daemons have been a recurring source of failures in earlier agent tools. An optional "leader" process that forwards writes is specified, but it is built only if measurements show that it is needed, and correctness never depends on it.

</details>

### The three-phase write {#three-phase-write}

Every write command, from setting one field to merging a branch, runs in three phases, so that the writer lock is held as briefly as possible.

1. **Compute, without the lock.** The process opens the store, catches up with the log, resolves its branch, validates the change (schema, guards, lease, status rules, cycle checks) and builds the complete changeset and its canonical form against a snapshot.
2. **Commit, under the lock.** The process takes the writer lock and scans what other writers appended since its snapshot. If none of it touched anything the change read or wrote, the prepared commit is attached to the new tip in constant time; otherwise it is recomputed once, or phase 1 runs again, at most twice. The process appends its records as one group and releases the lock. For a plain write the lock is held for tens of microseconds. The process then makes the group durable through group commit (next section), which takes the writer lock again only briefly.
3. **After the commit.** The process prints the result and, if a threshold was crossed, does some maintenance.

### One flush for many writers {#group-commit}

Asking the disk to flush is the slowest step of a commit. If every writer flushed on its own, sixteen agents finishing at once would wait for sixteen flushes in a row.

Klotho uses leaderless [group commit](glossary.html#term-group-commit) instead. A writer whose group is not yet durable takes a second lock byte, the *flush byte*, and then the writer lock again. It re-writes the unflushed range of the log, releases the writer lock and flushes once. It then takes the writer lock again for a moment to publish a new `HEAD` slot that covers every group appended before the flush began. A writer that appended in the meantime finds its group already covered and does not flush again. The targets are at most one flush per durable commit, exactly one for a writer that is alone, and at most three flushes for a burst of sixteen writers.

No process acts as a coordinator, so the protocol also works for agents in sandboxes that cannot reach another process. Each group's checksum is chained to that of the group before it. Before reporting success, a writer re-reads its own chained trailer. If another process's failed flush made the operating system drop unflushed pages, a lost group shows up there, and the writer retries with the same idempotency key instead of reporting a commit that is not on disk.

<figure class="diagram">
<svg viewBox="0 0 720 330" role="img" aria-labelledby="engine-d1-t">
  <title id="engine-d1-t">The write path: compute, append under the writer lock, one shared flush, publish, acknowledge; readers take no lock</title>
  <defs><marker id="engine-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path class="d-arrow" d="M0,0 L10,5 L0,10 z"/></marker></defs>
  <text class="d-text-small" x="12" y="22">Writer A runs one write command; time runs from left to right</text>
  <path class="d-box" d="M20,34 H124 A8,8 0 0 1 132,42 V118 A8,8 0 0 1 124,126 H20 A8,8 0 0 1 12,118 V42 A8,8 0 0 1 20,34 Z"/>
  <text class="d-text-strong" x="72" y="60" text-anchor="middle">1 Compute</text>
  <text class="d-text-small" x="72" y="84" text-anchor="middle">no lock held</text>
  <text class="d-text-small" x="72" y="104" text-anchor="middle">on a snapshot</text>
  <path class="d-box" d="M164,34 H268 A8,8 0 0 1 276,42 V118 A8,8 0 0 1 268,126 H164 A8,8 0 0 1 156,118 V42 A8,8 0 0 1 164,34 Z"/>
  <text class="d-text-strong" x="216" y="60" text-anchor="middle">2 Append</text>
  <text class="d-text-small" x="216" y="84" text-anchor="middle">writer lock held</text>
  <text class="d-text-small" x="216" y="104" text-anchor="middle">re-check, append</text>
  <path class="d-box" d="M308,34 H412 A8,8 0 0 1 420,42 V118 A8,8 0 0 1 412,126 H308 A8,8 0 0 1 300,118 V42 A8,8 0 0 1 308,34 Z"/>
  <text class="d-text-strong" x="360" y="60" text-anchor="middle">3 Flush</text>
  <text class="d-text-small" x="360" y="84" text-anchor="middle">flush lock held</text>
  <text class="d-text-small" x="360" y="104" text-anchor="middle">one disk flush</text>
  <path class="d-box" d="M452,34 H556 A8,8 0 0 1 564,42 V118 A8,8 0 0 1 556,126 H452 A8,8 0 0 1 444,118 V42 A8,8 0 0 1 452,34 Z"/>
  <text class="d-text-strong" x="504" y="60" text-anchor="middle">4 Publish</text>
  <text class="d-text-small" x="504" y="84" text-anchor="middle">new HEAD slot</text>
  <text class="d-text-small" x="504" y="104" text-anchor="middle">bound moves on</text>
  <path class="d-box-accent" d="M596,34 H700 A8,8 0 0 1 708,42 V118 A8,8 0 0 1 700,126 H596 A8,8 0 0 1 588,118 V42 A8,8 0 0 1 596,34 Z"/>
  <text class="d-text-strong" x="648" y="60" text-anchor="middle">5 Acknowledge</text>
  <text class="d-text-small" x="648" y="84" text-anchor="middle">own trailer</text>
  <text class="d-text-small" x="648" y="104" text-anchor="middle">checked first</text>
  <path class="d-edge" d="M132,80 L156,80" marker-end="url(#engine-arrow)"/>
  <path class="d-edge" d="M276,80 L300,80" marker-end="url(#engine-arrow)"/>
  <path class="d-edge" d="M420,80 L444,80" marker-end="url(#engine-arrow)"/>
  <path class="d-edge" d="M564,80 L588,80" marker-end="url(#engine-arrow)"/>
  <path class="d-box" d="M164,166 H268 A8,8 0 0 1 276,174 V214 A8,8 0 0 1 268,222 H164 A8,8 0 0 1 156,214 V174 A8,8 0 0 1 164,166 Z"/>
  <text class="d-text" x="216" y="190" text-anchor="middle">Writers B, C</text>
  <text class="d-text-small" x="216" y="208" text-anchor="middle">append too</text>
  <path class="d-edge-dashed" d="M276,194 L348,194 L348,128" marker-end="url(#engine-arrow)"/>
  <text class="d-text-small" x="358" y="160">one flush</text>
  <text class="d-text-small" x="358" y="176">covers A, B, C</text>
  <path class="d-edge-accent" d="M504,126 L504,250" marker-end="url(#engine-arrow)"/>
  <text class="d-text-small" x="514" y="200">readers see</text>
  <text class="d-text-small" x="514" y="216">the new state</text>
  <path class="d-box-muted" d="M20,252 H700 A8,8 0 0 1 708,260 V308 A8,8 0 0 1 700,316 H20 A8,8 0 0 1 12,308 V260 A8,8 0 0 1 20,252 Z"/>
  <text class="d-text" x="360" y="278" text-anchor="middle">Readers take no lock: mapped segments, plus the log up to the published bound</text>
  <text class="d-text-small" x="360" y="300" text-anchor="middle">Many processes share the same read-only pages in the operating system's cache</text>
</svg>
<figcaption>Writer A computes its change without any lock and holds the writer lock to re-check and append. The writer that flushes takes the writer lock again, briefly, to re-write the pending range and to publish. Writer A shares one flush with every writer that appended before it, and it reports success only after the new <code>HEAD</code> slot is published and its own chained trailer checks out. Readers never wait for a lock. Box 1 is phase 1 of the three-phase write; boxes 2 to 5 are phase 2.</figcaption>
</figure>

## Crash safety {#crash-safety}

The engine is designed so that a process can be killed at any instruction, and the machine can crash at any moment, without losing a commit that was acknowledged.

- **All or nothing.** Records are written in groups, for example a commit together with its lease and marker records. Recovery adopts a group whole or not at all, and the log ends at the first incomplete or invalid group. A task can therefore never appear completed without the marker that stops other branches from handing it out again.
- **Acknowledged means durable.** A commit is reported as done only after a flush that began after the commit was written has returned, its effects are published, and the writer has checked its own chained trailer.
- **The log is the truth; `HEAD` is a cache.** `HEAD` has two checksummed 4 KiB slots. A publish writes the new state into the older slot, and a reader takes the valid slot with the higher sequence number, so a torn write of one slot does no harm. `HEAD` is not flushed on every commit; it records how far the log is known to be durable and the boot of the machine in which the store was created or last recovered after a reboot. After a reboot, the first process re-scans the log from the last durable point and flushes `HEAD` once before anyone reads, so no reader sees a view from before the crash.
- **A failed flush is fatal.** The process stops, and the next process that flushes re-writes the unflushed range instead of trusting pages that may have been dropped.
- **Deletion waits for both slots.** Before maintenance deletes or recycles any file, both `HEAD` slots are made to name the new state and `HEAD` is flushed, so no surviving slot can point at a deleted file.

`moirai doctor --verify` recomputes every derived structure and branch head and compares them with what is stored; `moirai doctor --fsck` verifies checksums; `moirai repair --rebuild-from-log` rebuilds every segment and history file from the log.

> [!WARNING]
> **A stated gap**
>
> These guarantees are tested with a simulated file system that injects faults, with exhaustive crash-point enumeration, and with loops that kill real processes on Windows (see [the test workstream](building.html#test-workstream)). A test rig that repeatedly crashes or hard-powers-off a Windows virtual machine is deferred until after the release. Until it runs, surviving a real power loss is an explicit, unverified risk, and the design assumes that the disk honours a flush request.

## Durability classes and quiet mode {#durability}

Not every record needs the same protection. Each kind of record has a [durability class](glossary.html#term-durability-class), and each class maps to exact system calls on each operating system.

| Class | Used for | Guarantee |
|---|---|---|
| `durable` | every graph change; claim, complete and release; deletions; branch and tag moves; the markers that keep finished tasks from being handed out again; idempotency results; checkpoints | acknowledged only after a flush that covers it; survives power loss |
| `lazy` | heartbeats, read cursors, session marks, and evidence that the file-link resolver collects | appended and published at once, flushed by the next covering flush; may be lost after a power loss, an operating-system crash or a failed flush |
| `durable+meta` | creating log extents and sealed files; the `HEAD` barrier | the file's data and its size are durable |
| `durable-name` | a create, rename or delete that a durable record depends on | the directory entry is durable, because the parent directory is synced |

Graph changes are always `durable`; no configuration key can change that. The durable flush is `NtFlushBuffersFileEx` with its data-only flag on Windows, `fdatasync` on Linux and `fcntl(F_FULLFSYNC)` on macOS, never a weaker call. If a location cannot provide a class, the store is refused there rather than silently weakened.

### Quiet mode {#quiet-mode}

`moirai quiet on` is for benchmark windows on the same machine. It switches off the maintenance that busy writers would otherwise do in the foreground: no automatic checkpoint until the log tail reaches a multiple of its usual threshold (eight in the design; an M0 measurement sets the final value), and then exactly one bounded checkpoint runs; and no rollup, promotion, garbage collection, image export or import unless forced. A lane whose status is `measuring` implies it; `moirai quiet off` ends it.

## Checkpoints, rollups and garbage collection {#maintenance}

- **Delta [checkpoint](glossary.html#term-checkpoint).** When the log tail grows past a threshold (counted in operations, in bytes and in overlay memory), the tail is folded into a new delta segment, and its bodies are compressed and sealed into a `blobs` file. This happens outside the writer lock, and readers keep using the old segments until the next publish. The MCP server does this work in slices of at most 5 ms between requests; a command process does it only when the tail is twice over the threshold. Three delta segments are folded into one.
- **[Rollup](glossary.html#term-rollup).** When the delta segments grow past 25 % of the base, a new base segment is written. A rollup never runs inside the MCP server or a command or hook process. It runs only in a separate `moirai gc` process, started explicitly or spawned detached at low CPU, memory and I/O priority. It streams its input, so its memory stays bounded; the target is at most 24 MB at 100,000 nodes.
- **History retirement.** A full log extent is compressed into a `hist` file. Nothing is dropped by default.
- **Garbage collection.** `moirai gc` keeps everything reachable from a branch, a tag, a pin or a reflog entry younger than 90 days (`gc.reflog-expire`). Unreachable commits older than 14 days (`gc.cruft-delay`) are dropped from history; their headers are kept unless you ask otherwise. Files are deleted only after a 60-second grace period (`gc.delete-grace`) and the two-slot `HEAD` barrier. All three are configuration keys.

## Backup and restore {#backup-restore}

`moirai backup DIR` writes a copy of the store that is consistent at a commit boundary. It flushes every copied file and the directory before it reports success, and it records the backup in the log, so the warning about the age of the last backup is truthful.

`moirai restore DIR --into EMPTY_DIR` swaps a backup into place under the writer and maintenance locks, gives the restored store a new epoch, and marks the old store as retired, so every long-running process finds the store again.

A second, independent copy comes from the [git image](glossary.html#term-git-image). The default export policy writes a checkpoint of `main` at every merge into `main` and one per day for `main` and for every live lane, so even unmerged work on a lane has a copy outside the store. See [Saving into git](git-image.html).

## Windows first, Linux and macOS by design {#cross-platform}

Windows is built and tested first: every milestone up to the release is implemented and gated on Windows. Linux and macOS are designed now, not later. The on-disk format, the lock and commit protocol and the operating-system layer are specified for all three systems and are frozen together with format version 1. The Linux and macOS implementations come in a port phase after the Windows release, which is not yet scheduled. A port may add an implementation; it may never change the format or the protocol.

- **One format.** Every byte layout is little-endian and 64-bit and does not depend on the page size or the path separator. A backup restores on another system, and an image imports anywhere.
- **One protocol, written for the weakest system.** The rules assume the hardest failure behaviour of the three: directory entries that need their own sync, failed flushes that may drop pages, late lock release after a crash, reused inode numbers. The test file-system simulator enforces this model, so tests on Windows already exercise Unix failure behaviour.
- **One OS layer.** Only one crate, today named `moirai-os`, contains operating-system-specific code. A lint rejects `cfg(windows)`, `std::os` and direct use of `windows-sys` or `libc` anywhere else. Each target has exactly one implementation of each interface, chosen at compile time.
- **Pure Rust only.** No dependency may compile or link C, C++ or assembly. On every merge, the workspace is type-checked for Linux on x86-64 and ARM64 and for macOS on ARM64, with the C toolchains disabled, so a dependency that needs native code fails at once. Only the thin binary entry point, which is checked for Windows, and a few reviewed host-only test tools are left out of this check.

| Topic | Windows | Linux | macOS |
|---|---|---|---|
| Writer and flush locks | `LockFileEx` | open-file-description locks (kernel 3.15 or later) | open-file-description locks (macOS 14 or later) |
| Durable flush | `NtFlushBuffersFileEx`, data only | `fdatasync` | `fcntl(F_FULLFSYNC)` |
| File systems admitted | NTFS | ext4, XFS, btrfs | local APFS |
| Status | built and gated in every milestone up to the release | specified; ported later | specified; ported later |

## Budgets {#budgets}

Each of the four priorities, speed, memory, correctness and agent tokens, has numeric budgets. The table below is a selection. All values are design targets. Times are engine time on a warm cache; they exclude starting the process, which the operating system pays for every command-line call.

| Budget | Target |
|---|---|
| Engine time of a read command on `main` (`get`, `ready`, `blocking` and similar) | ≤ 5 ms at 10,000, 100,000 and 1,000,000 nodes |
| Opening a store with a full log tail | ≤ 1 ms, ≤ 1.5 ms and ≤ 3 ms at 10,000, 100,000 and 1,000,000 nodes |
| Fetching one node | ≤ 5 µs |
| A durable commit | median at most one disk flush plus 0.5 ms; at most one flush per commit |
| Holding the writer lock | ≤ 5 ms at the 99th percentile, for every kind of write |
| Waiting for the writer lock with 16 writers at once | ≤ 50 ms at the 99th percentile |
| An MCP read call | median ≤ 5 ms |
| Memory of a command or hook process | ≤ 4 MB of private memory at all three sizes |
| Memory of the MCP server | ≤ 8 MB plus a 4 MiB branch cache in steady state; ≤ 16 MB at peak; ≤ 2 threads; ≤ 3 MB if it is never called |
| All moirai processes under a load of 16 sessions | ≤ 256 MB in total |
| Idle CPU | zero CPU time and zero context switches |
| Tokens at session start (brief, skill listing, MCP tool names) | ≤ 3,000 tokens |
| Tokens added per spawned agent, not counting its context pack | ≤ 2,000 tokens (≤ 1,500 for an agent that works through MCP) |
| MCP tool schemas | ≤ 5,000 bytes for all ten tools; none loaded up front by default |

**How the budgets are gated.** Each budget names its gate and the milestone from which the gate is mandatory, and it stays mandatory after that. Time and memory gates run under a written measurement protocol on a reference Windows 11 machine with real-time antivirus scanning on, both idle and while a recorded load of 16 agents is replayed, with sample sizes set by how long the measured operation takes. Several gates are relative to a physical floor measured in the same run, such as the cost of one flush, so they do not depend on one particular disk. The hosted CI runners build the code, run the lints and run the unit, property and differential test suites on synthetic data; that is a correctness signal only, never a timing gate. Token budgets are checked by a [token ledger](glossary.html#term-token-ledger) that records every byte moirai places into an agent's context and counts it with the tokenizer of the model that the harness runs.

## Configuration {#configuration}

Every operational policy is a configuration key with a type and a documented default. You change behaviour by setting a key, not by rebuilding.

- **Two scopes.** Store keys live in `<store>/config` and apply to every process and worktree that uses the store, so all writers behave alike. User keys, such as paths on this machine and how stores are discovered, live in `%APPDATA%\moirai\config` on Windows and in `$XDG_CONFIG_HOME/moirai/config` on Linux and macOS. Each key has exactly one scope.
- **Precedence.** A command-line flag beats an environment variable, which beats the configuration file, which beats the built-in default.
- **A typed registry.** The binary carries every key's type, range, default, scope and reload class, and `moirai config list --defaults` prints it. `moirai config set` validates a value and refuses an invalid one. In a file edited by hand, an unknown key is a warning, and an invalid value falls back to the default; `moirai config check` and `moirai doctor` report both.
- **Reloading.** Command and hook processes read the configuration for every command. The long-running MCP server notices a change through a counter in `HEAD` that `config set` increments, and reads the configuration again on its next request.

The files use git-config syntax:

```text
# moirai store configuration
default-branch = main
[gc]
    reflog-expire = 90d
    cruft-delay = 14d
```

`moirai config get KEY` prints the value in effect for a key.
