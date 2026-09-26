# 05 — A fast, low-RAM embedded storage engine in Rust for moirai

*Research report · lens: storage engine performance and RAM · date: 2026-09-25 · status: research only (nothing implemented)*

---

## 0. How to read this report

**Evidence labels.** Every number carries one of these labels:

| Label | Meaning |
|---|---|
| **MEASURED-HERE** | Measured during this research on the owner's machine (see §2). Methodology is given, and the numbers can be reproduced. |
| **MEASURED-EXT** | A third party measured it with a published method (paper, benchmark repo, maintainer's post with numbers). |
| **CLAIMED** | A vendor or maintainer states it, or it comes from a secondary source, and no reproducible method is available here. |
| **ESTIMATE** | My own arithmetic from the assumptions in §3. Treat it as a budget to check, not a fact. |

**Versions.** Crate versions and dates were read from the crates.io API on 2026-09-25 (`https://crates.io/api/v1/crates/<name>`). Repository activity was read from the GitHub API on the same day.

**Scope.** This report covers the storage and performance layer only: file formats, I/O, memory layout, indexing, process model, and Windows specifics. Product semantics (the task and knowledge model, the MCP and skills design) are covered only where they drive storage decisions.

---

## 1. Executive summary

1. **Process spawn dominates CLI latency on this machine, not the database.**
   - A small Rust executable costs **20–38 ms spawn-to-exit at p50** at moderate load, and **~73 ms** when the CPU is saturated (MEASURED-HERE).
   - Claude Code's Bash tool adds a Git-Bash wrapper on top: **~109 ms p50** under load. PowerShell costs ~600 ms (MEASURED-HERE).
   - For comparison, opening a file and mapping it costs **~0.2 ms** (MEASURED-HERE).
   - So the engine's open path must be O(1) and well under 1 ms. The real speed win for agents is a long-lived **MCP server process**, not a faster engine.
2. **RAM is genuinely scarce on the owner's machine.** At measurement time:
   - 1.8 GB of 15.8 GB RAM was free.
   - 16 `claude`/`node` processes held 2.0 GB of working set and 3.5 GB of private memory.
   - Every moirai process (CLI or MCP server) should therefore target **single-digit MB of private memory**. Data should live in the **OS page cache through read-only memory maps (mmap)**, which all processes share, rather than in per-process buffer pools. Mainstream engines ship large per-process defaults:
     - redb: 1 GiB cache cap.
     - fjall: 32 MiB block cache, up to 4 worker threads, and memtables.
     - SQLite: 2 MB page cache per connection.
3. **Each durable commit costs about 1.8–1.9 ms on this SSD.** The drive is an SK hynix HFM512GD3JX013N consumer NVMe on NTFS (MEASURED-HERE):
   - `FlushFileBuffers` after a 4 KiB write: p50 1.83–1.97 ms, p99 up to 5.7 ms.
   - `NtFlushBuffersFileEx(FLUSH_FLAGS_FILE_DATA_SYNC_ONLY)`: 1.73 ms.
   - 64 pages plus one flush: 3.05 ms. Group commit is therefore roughly a 40× lever.
   - `FILE_FLAG_WRITE_THROUGH` returned in **0.14 ms**. That is probably *not* power-loss durable on consumer hardware. The PostgreSQL developers reached the same conclusion.
   - Design for **one flush per commit** (checksummed commit record, as redb's "1PC+C" does), and batch flushes in the MCP server or daemon.
4. **Windows semantics shape the design.**
   - Byte-range locks are **mandatory**: other processes cannot read or write the locked range.
   - Rust's `File::lock()` locks the whole 0..2^64 range. So **never lock the data file; use a separate lock file.**
   - A mapped file **cannot be truncated or extended** while views exist.
   - Mapped views and `ReadFile`/`WriteFile` are **not guaranteed to be coherent**.
   - I/O errors inside a mapped view surface as structured exceptions, which crash a Rust process.
   - Together this points to **immutable (sealed) segments mapped read-only**, plus a log tail written and read with explicit I/O.
5. **Microsoft Defender charges per file open, not per byte.** SQLite measured that antivirus slowed "direct-to-disk" writes by about 10× while barely affecting a single database file. Defender real-time protection is ON here.
   - Keep the store to **a handful of files**, never one file per node.
   - Defender's asynchronous "performance mode" applies only to Dev Drive (ReFS), not to NTFS.
6. **mmap is appropriate for moirai, but only for reads of immutable data.**
   - The CIDR 2022 paper shows mmap failing when the working set exceeds RAM, for writes, and at high I/O rates. Its measured losses were 2–20× versus `fio`.
   - None of those regimes apply to a store of roughly 10 MB–1.3 GB that is mostly read and mostly cached.
   - Keep writes explicit (append plus flush), as LMDB does ("read-only mmap by default"). redb dropped its mmap backend because its soundness could not be proven, which is another argument for keeping mapped data immutable.
7. **Engine choice.**
   - For history, branches and diffs, an **append-only operation log** is the natural source of truth. Its current state should be materialized into **checkpointed, zero-copy, mmap-able segments**:
     - fixed-layout node tables,
     - forward and reverse CSR adjacency,
     - roaring bitmaps for status and labels,
     - an FST term dictionary.
   - This is **Option A** in §16 and is the recommended design.
   - A copy-on-write B+tree (redb/LMDB style) is the strong alternative (**Option B**). It wins on random updates at 1e6 nodes, but versioning costs about depth × 4 KiB per commit.
   - An LSM tree buys little at this scale and adds background threads and RAM (fjall, RocksDB).
8. **Zero-copy formats: pay the validation cost deliberately.**
   - rkyv reads are ~1 ns unvalidated, but **validated** reads cost about the same as full deserialization (MEASURED-EXT: 274 µs vs 1.2 ms deserialize on the `log` dataset).
   - For hot structures, prefer `zerocopy` fixed-layout little-endian structs with explicit bounds checks. They are safe with no validation pass.
   - Use a tagged-varint field block for user-defined typed fields, so the schema can evolve.
9. **Choose hashes by role, not speed.** At moirai's record sizes the hash cost is negligible: SHA-256 runs at **1.94 GB/s** here thanks to SHA-NI (MEASURED-HERE).
   - Commit and object IDs: **BLAKE3-256**, which is cryptographic and fast.
   - Page and record checksums: **xxh3** (redb uses XXH3-128).
   - Do not use SHA-1.
10. **Full-text search and vectors should be tiered by scale.**
    - At 1e4 nodes, brute-force scanning of zstd-compressed bodies costs milliseconds.
    - At 1e5–1e6, a minimal inverted index is enough: FST dictionary plus delta-varint or roaring postings.
    - tantivy costs at least **15 MB per indexing thread** (12 MB baseline) and uses many files. Keep it optional.
    - Vectors: int8 384-dimensional embeddings cost 384 B per node, so flat search is enough up to about 1e5. The embedding model, not the index, dominates RAM.
11. **Prior art warns about concurrency.** beads, a git-backed issue tracker and agent memory tool, moved through several concurrency models:
    - SQLite plus a daemon, added specifically for "multiple agents" corruption and lock contention.
    - Dolt with a SQL server.
    - Embedded Dolt holding an exclusive lock, where a second concurrent opener gets an error.
    - Server and proxied-server modes.
    - moirai's **multi-process concurrency model must be designed on day 1**.

---

## 2. Measurements taken on the owner's machine (MEASURED-HERE)

**Machine:**

| Component | Value |
|---|---|
| CPU | AMD Ryzen 9 5900HS (Zen 3, 8 cores / 16 threads, SHA-NI, AVX2, no AVX-512) |
| RAM | 16 GB (15.8 GB visible) |
| Disk | SK hynix HFM512GD3JX013N NVMe, 512 GB, consumer drive. C: and D: are NTFS partitions on this one disk. |
| OS | Windows 11 Home 10.0.26200 |
| Power | High performance plan, on AC power |
| Defender | Real-time protection enabled (`Get-MpComputerStatus`) |
| Background load during tests | 30–100% CPU (other agent sessions running) |

The load is representative of the owner's real multi-agent use, but it adds noise, so min, p50 and p90 are all reported.

**Method.**
- Spawn timings use .NET `Process.Start` with redirected stdio, measured spawn-to-exit over n = 20–60 runs after 2 warm-ups.
- File I/O used 4 KiB writes on a scratch file in the session temp directory (same SSD).
- mmap timings used a compiled C# probe (`MemoryMappedFile`).
- No project files were created.

### 2.1 Process spawn (spawn-to-exit)

| Executable | CPU load | min | p50 | p90 |
|---|---|---|---|---|
| `hostname.exe` (tiny native) | ~100% | 39.0 ms | 57.5 ms | 182 ms |
| `hostname.exe` | 30–50% | 26.7 | 32.6 | 100.7 |
| `cmd.exe /c exit` | ~100% | 44.0 | 55.5 | 71.8 |
| Rust `mdbook-regex --help` (2.5 MB exe) | 30–50% | 20.2 | 25.2 | 48.5 |
| Rust `mdbook-regex --help` | ~100% | 56.8 | 73.1 | 108.4 |
| Rust `cargo-machete --version` (9 MB exe) | 30–50% | 23.7 | 37.9 | 51.1 |
| Rust `mdbook --version` (12 MB exe) | ~100% | 34.7 | 46.3 | 60.2 |
| `git.exe --version` (mingw64, direct) | 30–50% | 33.4 | 42.4 | 65.8 |
| Git `bash.exe -c true` (the wrapper used by the agent Bash tool) | ~100% | 93.4 | 109.0 | 235.2 |
| `powershell.exe -NoProfile -Command exit` | ~100% | 483.7 | 602.4 | 891.0 |
| `python -c pass` | ~100% | 77.1 | 363.8 | 524.9 |

**Takeaways.**
- The process creation floor on this box is about **20–40 ms**, whatever the executable size (2.5 MB vs 12 MB makes no consistent difference).
- The agent's shell wrapper adds about **100 ms**.
- These numbers are consistent with published comparisons: Linux launches programs ">20× faster" than Windows, and the Windows result is "very sensitive to background services such as Windows Defender" (CLAIMED/qualitative, [bitsnbites](https://www.bitsnbites.eu/benchmarking-os-primitives/)).
- Defender scanning of freshly built executables slowed cargo builds by 40–55% ([rust-lang/cargo#5028](https://github.com/rust-lang/cargo/issues/5028), MEASURED-EXT).

### 2.2 Durability and file I/O

| Operation (4 KiB unless noted) | min | p50 | p90 | p99 |
|---|---|---|---|---|
| Append + `FlushFileBuffers` | 1.765 ms | **1.931** | 3.266 | 5.729 (max 13.7) |
| Overwrite in place + `FlushFileBuffers` | 1.737 | **1.831** (re-run: 1.970) | 1.920 | 4.420 |
| Overwrite + `NtFlushBuffersFileEx(FLUSH_FLAGS_FILE_DATA_SYNC_ONLY)` | 1.660 | **1.731** | 1.815 | 3.315 |
| Overwrite, `FILE_FLAG_WRITE_THROUGH` | 0.119 | **0.139** | 0.187 | 0.356 |
| Overwrite, buffered, no flush | 0.006 | 0.006 | 0.007 | 0.045 |
| 64 × 4 KiB (256 KiB) + one `FlushFileBuffers` | 2.641 | **3.052** | 3.948 | 26.5 |
| `CreateFile` + read 4 KiB + close (existing file) | 0.163 | **0.172** | 0.201 | 0.354 |

**Interpretation.**
- The floor of about 1.7 ms is the drive's volatile-cache flush. The drive has no power-loss protection, so this is expected.
  - Jan 2026 ext4 measurements: Samsung 990 Pro fsync 2.97 ms, Crucial T500 0.89 ms, enterprise drives with PLP 1.6–12 µs ([Small Datum](http://smalldatum.blogspot.com/2026/01/ssds-power-loss-protection-and-fsync.html), MEASURED-EXT).
- Data-sync-only saves about 0.1–0.25 ms by skipping metadata. Appending (which changes file size) costs about 0.1 ms more than overwriting in place.
- `WRITE_THROUGH` returning in 0.14 ms suggests the flag may not reach stable media on consumer drives. It is **not accepted as durable** here; see §6.2.

### 2.3 Memory mapping (compiled C# probe, 1 MiB file already in the OS cache)

| Operation | p50 |
|---|---|
| Open + `CreateFileMapping` + `MapViewOfFile` (read-only) | **0.222 ms** (p90 0.369) |
| First touch of a page (soft fault, page already in the standby/cache list) | **~1.0 µs per page** |
| Re-touch of a mapped page | 0.064 µs per page (includes .NET accessor overhead) |
| `FileStream` seek + read 4 KiB from cache | 6.96 µs per page (includes .NET overhead) |

**Takeaway.** Once a file is mapped, a cached page costs about 1 µs the first time and nanoseconds afterwards. An explicit read through a managed stream costs about 7 µs. For a read-mostly store, mapped reads are the cheapest path on Windows as well as on Linux.

### 2.4 Memory context

| Item | Value |
|---|---|
| Free RAM | 1,823 MB of 15,776 MB |
| `claude`/`node` processes | 16 processes, 2,009 MB working set, 3,481 MB private |
| Idle native console process (`more.com`) | 4.5 MB working set (mostly shared DLL pages), **0.69 MB private** |

A statically linked Rust CLI should land in the same range: about 1–3 MB private (ESTIMATE).

### 2.5 Hashing on this CPU (Python 3.14 `hashlib`, which uses OpenSSL)

| Hash | Bulk throughput (64 MiB) | 256 B message (includes ~0.8 µs Python call overhead) |
|---|---|---|
| SHA-1 | 2.00 GB/s | 0.88 µs |
| SHA-256 | **1.94 GB/s** (SHA-NI) | 0.92 µs |
| BLAKE2b | 0.67 GB/s | 0.99 µs |
| BLAKE2s | 0.47 GB/s | 1.30 µs |
| MD5 | 0.81 GB/s | 1.19 µs |

BLAKE3 and xxh3 were not installed and were **not measured** here. Published figures are in §11.

---

## 3. Workload model used for budgets (ESTIMATE)

| Parameter | Assumption | Why |
|---|---|---|
| Nodes N | 1e4 / 1e5 / 1e6 | Scales requested by the brief |
| Fixed node header | 40–48 B: id, kind, status, flags, priority, timestamps, offsets | See §9 layout |
| Title | 60 B average | Short task and knowledge titles |
| Body | 1 KiB raw average; about 340 B after zstd with a dictionary (~3×) | See §12 |
| Typed fields | ~4 per node, ~6 B each as tagged varints (≈24 B) | e.g. `done: bool`, `priority: u8`, `due: date` |
| Edges | Out-degree ≈3 (subtask/parent, blocks, relates). Stored forward **and** reverse, ~5 B per entry (u32 target + type) → ≈30 B per node | Reverse index needed for "synchronous" referential consistency |
| History | ~10 operations per node lifetime, 60 B raw each, ~20–60 B stored (batch-compressed) | Operation log |
| FTS postings | ~90 unique terms per node × ~1.5 B (delta-varint) ≈ 135 B, plus dictionary | §13 |
| Optional vectors | 384-dimensional int8 = 384 B per node | §13 |
| Concurrency | 1–16 processes (orchestrator plus subagents across worktrees), about 1–3 writers active at once, reads dominate | Brief |

**Derived sizes (ESTIMATE):**

| N | On-disk current state + FTS | + history | + int8 vectors | "Index-only" hot set (headers + CSR + bitmaps, ≈86 B per node) |
|---|---|---|---|---|
| 1e4 | ~6.6 MB | ~9–13 MB | +3.8 MB | ~0.9 MB |
| 1e5 | ~66 MB | ~90–130 MB | +38 MB | ~8.6 MB |
| 1e6 | ~0.66 GB | ~0.9–1.3 GB | +384 MB | ~86 MB |

Even at 1e6 nodes the whole store fits in the page cache of a 16 GB machine. **The hot set that list, filter and graph queries touch is 1–86 MB**, and processes share it through mmap.

---

## 4. Reference engines (baselines and design donors, not necessarily dependencies)

### 4.1 Status snapshot (2026-09-25)

**redb 4.3.0** (2026-09-15)
- Design: copy-on-write B+trees, one file. Explicit I/O with its own cache; the mmap backend was removed in 0.14 (2023-03-26) because it was "infeasible to prove that it was sound".
- Durability: 1 fsync per commit ("1PC+C": XXH3-128-checksummed commit slots plus a "god byte"). Two-phase commit is optional.
- Multi-process: the default `ExclusiveWriter` **locks the whole file**. 4.3 added experimental multi-process read-write (`SingleWriter`/`MultiWriter`) using byte-range locks on Linux, Apple and Windows.
- RAM defaults: `cache_size` cap **1 GiB** (a lazy upper bound).
- Notes:
  - v3 (2025-08-09) cut the minimum file from ~2.5 MiB to **~50 KiB**.
  - 4.2 made `Durability::None` commits about 2× faster.
  - 4.3 fixed a Windows-only hang when a write reported zero bytes written.
  - Sources: [README](https://github.com/cberner/redb), [CHANGELOG](https://github.com/cberner/redb/blob/master/CHANGELOG.md), [design.md](https://github.com/cberner/redb/blob/master/docs/design.md), [db.rs](https://github.com/cberner/redb/blob/master/src/db.rs).

**LMDB through heed 0.22.1** (2026-04-07)
- Design: copy-on-write B+tree with shadow paging; **read-only mmap** plus `write()`.
- Durability: two meta pages; fsync per commit by default.
- Multi-process: **yes**, the most mature option (lock file plus reader table).
- RAM defaults: none of its own; relies on the OS page cache.
- Notes:
  - Windows file growth depends on the LMDB branch (verified in source on 2026-09-25):
    - The **0.9 release branch** (`mdb.RE/0.9`) calls `SetEndOfFile` to the full `mapsize` at open. Its source comment reads "Windows won't create mappings for zero length files and won't map more than the file size. Just set the maxsize right now". See also [node-lmdb#159](https://github.com/Venemo/node-lmdb/issues/159), closed as "not a bug".
    - **`mdb.master`** uses undocumented `NtCreateSection(SEC_RESERVE)` and `NtMapViewOfSection(MEM_RESERVE)`, so the file grows incrementally (ITS#8324; [lmdbjava#68](https://github.com/lmdbjava/lmdbjava/issues/68)).
    - heed builds `lmdb-master-sys`, i.e. the master branch; confirm at pin time.
  - Map size must be chosen up front.
  - It is a C dependency.

**fjall 3.1.10** (2026-08-30; 3.0 on 2026-01-02)
- Design: LSM tree with keyspaces, a journal, LZ4, and key-value separation.
- Durability: by default flushes to OS buffers only; `persist(PersistMode)` fsyncs.
- Multi-process: **no**. "A single database may not be loaded in parallel from separate processes."
- RAM defaults: 32 MiB block cache, 512 MiB journal cap, worker threads = min(cores, 4), plus memtables ([db_config.rs](https://github.com/fjall-rs/fjall/blob/main/src/db_config.rs)).
- Notes: about 42k LoC, ~2.2 MB binary and ~3.5 s compile, against RocksDB's ~700k LoC, ~12 MB and ~40 s (CLAIMED, [Fjall 3 post](https://fjall-rs.github.io/post/fjall-3/)).

**RocksDB** (crate 0.25.0)
- Design: LSM tree.
- Durability: WAL, fsync configurable.
- Multi-process: single writer process; secondary instances are read-only.
- RAM defaults: memtables of 64 MB by convention × `max_write_buffer_number`, a block cache, and bloom filters at 10 bits per key ([wiki](https://github.com/facebook/rocksdb/wiki/Memory-usage-in-RocksDB)).
- Notes: a heavy C++ build.

**SQLite** (rusqlite 0.40.2)
- Design: B-tree with a rollback journal or WAL.
- Durability: WAL with `synchronous=FULL` is durable. `NORMAL` "might roll back following a power loss".
- Multi-process: **yes**. In WAL mode readers do not block the writer, but there is one writer at a time, and the `-shm` file is mapped.
- RAM defaults: `cache_size=-2000`, i.e. **~2 MB per connection** ([pragma](https://www.sqlite.org/pragma.html)).
- Notes:
  - `sqlite3_open` reads and parses the schema on first use (Hipp, [forum](https://sqlite.org/forum/info/a30bc374143a41a265b0b61fea81cf27a37ee7df88ece408ca250d1b03cd08dc)).
  - WAL auto-checkpoints at 1000 pages ([wal](https://www.sqlite.org/wal.html)).

**Turso (formerly Limbo) 0.7.2** (2026-09-25)
- Design: a Rust rewrite of SQLite with MVCC (`BEGIN CONCURRENT`), tantivy-based full-text search, and io_uring on Linux.
- Multi-process: not documented here.
- Notes: **not yet 1.0**. It says "keep independent backups" and tracks SQLite 3.50.4 ([repo](https://github.com/tursodatabase/turso)).

**sled 0.34.7** (the last stable release; the repo was last pushed 2026-04-04)
- Design: Bw-tree and log-structured.
- Durability: fsync every 500 ms by default.
- Notes: the README says the project is in "beta" and "uses too much space sometimes". Users reported 3–6 GB RSS with many tiny objects ([#986](https://github.com/spacejam/sled/issues/986)). **Avoid.**

**canopydb 0.2.5** (2025-11-22; no activity since)
- Design: compact B+tree with prefix/suffix truncation, MVCC reads, OCC writes, and an optional WAL.
- Durability: sync commits or asynchronous WAL fsync (e.g. every 500 ms).
- RAM defaults: page cache.
- Notes: "early stage… Do not trust it with production data" ([repo](https://github.com/arthurprs/canopydb)).

**native_db 0.8.2** (2025-07-08)
- Design: a typed layer over redb with secondary indexes and watch/subscribe.
- Durability and multi-process behaviour are inherited from redb.
- Notes: its API is unstable ([repo](https://github.com/vincent-herlemont/native_db)).

**SurrealKV 0.21.4**
- Design: LSM tree with MVCC versioning ([repo](https://github.com/surrealdb/surrealkv)).
- RAM: its README claims a "resting footprint ~328 MB" in its own 50k-key benchmark (CLAIMED; configuration-dependent).

**Kùzu** (archived on GitHub; last push 2025-10-10)
- Design: columnar storage with forward and backward **CSR adjacency** ([CIDR 2023](https://www.cidrdb.org/cidr2023/papers/p48-jin.pdf)).
- Notes: a useful design reference, but it shows the risk of depending on a single-vendor engine.

### 4.2 The one apples-to-apples published comparison (MEASURED-EXT)

The redb README benchmark was run on a Ryzen 9 9950X3D with a Samsung 9100 PRO on Linux. Per the [bench source](https://github.com/cberner/redb/blob/master/crates/redb-bench/src/lib.rs):
- keys are 24 B and values 150 B;
- the bulk load is 5M items;
- "individual writes" are 1,000 separate durable commits;
- "random reads" are 1M reads;
- the cache is 4 GB.

| | redb | lmdb | rocksdb | fjall | sqlite |
|---|---|---|---|---|---|
| Individual durable writes (1,000 commits) | **920 ms (0.92 ms/commit)** | 1,598 | 2,432 | 3,488 | 7,040 |
| Batch writes | 1,595 | 942 | 451 | **353** | 2,625 |
| Random reads (1M) | 1,138 (1.14 µs/read) | **637 (0.64 µs)** | 2,911 | 2,177 | 4,283 |
| Random reads, 32 threads | 410 | **125** | 1,100 | 576 | 26,536 |
| Uncompacted size | 4.00 GiB | 2.61 GiB | **893 MiB** | 1,001 MiB | 1.09 GiB |
| Compacted size | 1.69 GiB | 1.26 GiB | **455 MiB** | 1,001 MiB | 557 MiB |

canopydb's own run of the same suite on an i9-12900H reports individual writes of **404 ms** and batch writes of 555 ms, with LMDB still fastest on reads ([BENCHMARKS.md](https://github.com/arthurprs/canopydb/blob/master/BENCHMARKS.md), MEASURED-EXT).

**What this means for moirai.**
- Point reads from an mmap B+tree (LMDB) are about 2× faster than from a B-tree that copies through an explicit cache (redb), and 3–7× faster than LSM or SQLite. At moirai's scale all of them are microseconds, well below the ~25 ms process floor.
- On this machine the durable-commit floor is about 1.8 ms, set by the flush (§2.2), so engine differences in commit cost disappear.
- The "individual writes" row mostly measures **how many flushes each engine issues per commit**. redb's single-fsync protocol is the design to copy.
- B-trees have 2–4× space amplification before compaction. Leaving history and branches in place makes this worse (§8).
- **No published benchmark was found for any of these engines on Windows/NTFS.** moirai must produce its own (§17).

### 4.3 What to borrow from each

**LMDB**
- Read-only mmap, with writes done by explicit I/O.
- Two meta pages for atomic commit.
- A reader table so multiple processes can safely reclaim pages.
- Chu's defence: LMDB is "under 64KB of object code", and with a read-only map "all map pages are always clean", so the OS can evict them freely ([Symas](https://www.symas.com/post/are-you-sure-you-want-to-use-mmap-in-your-dbms), CLAIMED).

**redb**
- The 1PC+C commit: data and checksums are written first, then the primary bit is flipped, then **one** fsync. Recovery checks the checksum and transaction id, and falls back to the other slot.
- The "quick repair" option, which persists allocator state so crash recovery costs O(1).
- It assumes atomic single-byte writes, fsync durability, and "powersafe overwrite" ([design.md](https://github.com/cberner/redb/blob/master/docs/design.md)).

**SQLite**
- Keep it to one file (antivirus friendliness, §6.4).
- The WAL with a bounded checkpoint threshold.
- Its very honest durability documentation.

**fjall / RocksDB**
- Journal compression and per-block checksums.
- Bloom filters at 10 bits per key, about 1% false positives. moirai does not need them for exact-id lookups on dense ids.

**TerminusDB**
- Immutable **layers**: each commit is a layer of added and removed edges, named by a 20-byte id, with periodic delta "rollups".
- Succinct structures: front-coded dictionaries, bit sequences with rank/select, wavelet trees ([whitepaper](https://assets.terminusdb.com/research/succinct-data-structures-and-delta-encoding.pdf)).
- Its marketing states 13.57 B per triple (CLAIMED).
- The Rust `terminusdb-store` repo was last pushed 2024-03-11 (stale).

**Kùzu / Neo4j**
- Double-indexed forward and backward CSR adjacency (Kùzu).
- Fixed-size records for O(1) offset addressing. Neo4j's record format uses 15 B node, 34 B relationship and 41 B property records ([Neo4j KB](https://neo4j.com/developer/kb/understanding-data-on-disk/)).

---

## 5. mmap vs explicit I/O with your own buffer pool

### 5.1 The case against: CIDR 2022 (MEASURED-EXT)

Crotty, Leis and Pavlo, ["Are You Sure You Want to Use MMAP in Your DBMS?"](https://db.cs.cmu.edu/papers/2022/cidr2022-p13-crotty.pdf), CIDR 2022.

**Setup.** AMD EPYC 7713 (64 cores) with 512 GB RAM, of which 100 GB was available as page cache; 10× Samsung PM1733; Linux 5.11. The workloads were **read-only**, which the authors call the best case for mmap.

**Random reads over a 2 TB range with 100 threads:**
- `fio` with O_DIRECT sustained about **900K reads/s**.
- mmap with `MADV_RANDOM` matched it for 27 s. It then **dropped to nearly zero for ~5 s** when the page cache filled, and recovered to about **half** of `fio`.

**Sequential scans:** mmap was about **20× worse** with 10 SSDs. The summary line is that mmap is "2–20× worse than fio" once eviction starts.

**Root causes named in the paper:**
- TLB shootdowns (IPIs costing "thousands of cycles");
- single-threaded `kswapd` eviction;
- page-table contention;
- no transactional control over write-back;
- blocking I/O stalls;
- errors delivered as signals.

**Their own "maybe use mmap" criteria** are "working set (or the entire database) fits in memory and the workload is read-only".

### 5.2 Rebuttal and middle ground

- **Symas (LMDB).** mmap is safe when the map is read-only and writes go through `write()`. Buffer pools need "orders of magnitude more code" ([post](https://www.symas.com/post/are-you-sure-you-want-to-use-mmap-in-your-dbms), CLAIMED).
- **vmcache and exmap** (Leis et al., SIGMOD 2023). The middle ground is a real buffer manager that uses virtual memory for page-id translation. Linux page-table manipulation becomes the bottleneck on fast devices, so they add a kernel module (exmap) ([paper](https://www.cs.cit.tum.de/fileadmin/w00cfj/dis/_my_direct_uploads/vmcache.pdf), [code](https://github.com/viktorleis/vmcache)). This is Linux-only research and not applicable on Windows.
- **redb** removed mmap entirely because soundness could not be proven: Rust references into a map that another process can change behind them are unsound ([CHANGELOG 0.14.0](https://github.com/cberner/redb/blob/master/CHANGELOG.md)).

### 5.3 How that maps to moirai

| mmap failure mode (CIDR) | Does it apply to moirai? |
|---|---|
| Working set larger than RAM, so the eviction storm hits | **No.** The hot set is 1–86 MB and the full store is at most ~1.3 GB (§3). |
| Transactional safety of writes through the map | **Avoided** if the map is read-only and data is immutable after sealing. |
| I/O stalls on page fault | Small. The data is usually cached (soft fault ≈1 µs, §2.3). A cold NVMe read costs ~100 µs per page, and there is no concurrency target to defend. |
| Error handling (SIGBUS, or `EXCEPTION_IN_PAGE_ERROR` on Windows) | **Real.** Rust has no structured exception handling, so a bad sector or a file removed underneath the map crashes the process. Mitigate with checksums verified when a segment is sealed, plus the Windows guarantee that mapped files cannot be truncated (§6.1). |
| TLB shootdowns and `kswapd` at 1M+ IOPS | **Not applicable** at agent request rates. |
| Soundness of `&T` into shared memory (the redb argument) | **Avoided** if mapped segments are immutable for their whole life. Writers never modify a sealed segment; they write new ones. |

**Multi-process sharing** is the big advantage for moirai. With mmap, N agent processes reading the same segments share **one** physical copy through the OS page cache. The Windows docs note that soft faults are satisfied from pages that are in "the working set of some other process" ([Working Set](https://learn.microsoft.com/en-us/windows/win32/memory/working-set)). Per-process buffer pools duplicate the data N times: SQLite's 2 MB per connection, redb's cache capped at 1 GiB, fjall's 32 MiB block cache.

**Recommendation.**
- Use **mmap for sealed, immutable segments** (read-only views).
- Use **explicit `WriteFile` plus flush for the log tail and commit records**, and read the tail with explicit reads.
- **Never modify a mapped region in place.**
- Count RAM as **private bytes** (per process) plus **shared working set** (the page cache) separately. Task Manager's "Memory" column is private working set; mapped file pages show up as shareable.

---

## 6. Windows specifics that change the design

### 6.1 File mapping growth and truncation

- **The mapping size is fixed when it is created.** You cannot grow a mapping in place with documented APIs. The undocumented route is `NtCreateSection` with `SECTION_EXTEND_SIZE` and `SEC_RESERVE`, plus `NtExtendSection`, and memory committed that way cannot be decommitted ([Jeremy Ong, 2024-11-03](https://www.jeremyong.com/winapi/io/2024/11/03/windows-memory-mapped-file-io/)).
- **A mapped file cannot be truncated or extended.** Microsoft: "UnmapViewOfFile must be called first to unmap all views and call CloseHandle to close the file mapping object before you can call SetEndOfFile" ([SetEndOfFile](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-setendoffile)).
  - With several processes mapping the same file, **no process can shrink it** while any view is alive.
  - Design around it by **never truncating**: grow by adding new segments, or preallocate fixed-size segments; reclaim space by writing a new file and switching to it.
- **A writable mapping larger than the file extends the file**, and the new bytes are "not guaranteed to be zero". Mapping a zero-length file fails with `ERROR_FILE_INVALID` ([CreateFileMapping](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-createfilemappinga)).
- **LMDB on Windows.**
  - The 0.9 release branch sets the file size to the full `mapsize` at open (preallocated), where Linux uses sparse files ([node-lmdb#159](https://github.com/Venemo/node-lmdb/issues/159)). This is a footgun for per-project stores.
  - `mdb.master` avoids it only by using undocumented NTDLL section APIs ([mdb.master mdb.c](https://github.com/LMDB/lmdb/blob/mdb.master/libraries/liblmdb/mdb.c) vs [mdb.RE/0.9 mdb.c](https://github.com/LMDB/lmdb/blob/mdb.RE/0.9/libraries/liblmdb/mdb.c)).
  - moirai should not need either trick if it maps only sealed, fixed-size files.

### 6.2 Coherence and flush semantics

- **Mapped views of the same file are coherent** within and across processes (for local files).
- **Mapped views and `ReadFile`/`WriteFile` are "not necessarily coherent"** ([CreateFileMapping](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-createfilemappinga)). In practice the cache manager usually keeps them coherent, and LMDB relies on that, but the documentation does not guarantee it. This is one more reason to **map only sealed data**, and to read the mutable tail with explicit reads.
- **`FlushViewOfFile` "does not flush the file metadata, and it does not wait… until the changes are flushed from the underlying hardware disk cache"**. You need `FlushFileBuffers` after it ([FlushViewOfFile](https://learn.microsoft.com/en-us/windows/win32/api/memoryapi/nf-memoryapi-flushviewoffile)).
- **Rust std**: `File::sync_all()` calls `FlushFileBuffers`, and `sync_data()` **is the same call**. There is no data-only variant ([std/sys/fs/windows.rs](https://github.com/rust-lang/rust/blob/main/library/std/src/sys/fs/windows.rs)).
  - PostgreSQL maps `fdatasync` to `NtFlushBuffersFileEx(FLUSH_FLAGS_FILE_DATA_SYNC_ONLY)`. That "flush[es] the drive cache" and skips non-essential metadata ([pgsql-hackers thread](https://www.postgresql.org/message-id/CA+hUKG+F0EL4Up6yVYbbcWse4xKaqW4wc2xpw67Pq9FjmByWVg@mail.gmail.com), [MS docs](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/nf-ntifs-ntflushbuffersfileex)).
  - Measured here it saves ~0.1–0.25 ms per commit (§2.2). It is available through `windows-sys`.
- **`FILE_FLAG_WRITE_THROUGH`**: the PostgreSQL developers found that Windows SATA drivers "neither pass the 'FUA' flag down to the device nor fall back to sending a full cache flush command", so write-through is unreliable on consumer drives and its `pg_test_fsync` numbers are "too good to be true". They proposed making fdatasync (the `NtFlushBuffersFileEx` path) the default. The 0.14 ms measured here fits that pattern. **Do not rely on write-through for durability.**
- **Microsoft's own guidance**: `FlushFileBuffers` "can be inefficient when used after every write". Batch writes and flush at commit points ([FlushFileBuffers](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-flushfilebuffers)).

### 6.3 Locking semantics (critical for multi-process)

- `LockFileEx` locks are **mandatory**: "Locking a portion of a file for exclusive access denies all other processes both read and write access to the specified region". However, "Locking a region of a file does not prevent reading or writing from a mapped file view" ([LockFileEx](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-lockfileex)).
- **Locks are not released promptly when a process dies.** On termination "the time it takes for the operating system to unlock these locks depends upon available system resources". An agent crash does not free the lock instantly, so use timeouts or retries.
- **Rust std `File::lock`/`try_lock`** (stable since 1.89, [PR #136794](https://github.com/rust-lang/rust/pull/136794)) calls `LockFileEx` over the **whole 0..(u32::MAX, u32::MAX) range**. Locking the data file itself would therefore block every other process's `ReadFile` on it.
  - **Lock a dedicated `LOCK` file**, or lock byte ranges far beyond EOF. SQLite's lock-byte page at 1 GiB exists for exactly this historical reason ([fileformat](https://www.sqlite.org/fileformat.html)).
  - redb 5.0 is moving to "byte-range locks alone, rather than also with the whole-file lock".
- **Share modes.** Rust std opens files with `FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE`. That is good: other processes can rename or delete segments that are still open.
- **Rename and delete.**
  - `fs::rename` uses `MoveFileExW(MOVEFILE_REPLACE_EXISTING)`, falling back to `FILE_RENAME_FLAG_POSIX_SEMANTICS`.
  - `fs::remove_file` uses `DeleteFileW`, falling back to POSIX delete. Under Win32 semantics a file "won't actually be deleted until all file handles are closed" (std source).
  - **Segment garbage collection must tolerate delete-pending and sharing-violation errors and retry later.** Verify this behaviour on the target build (§18).

### 6.4 Antivirus (Microsoft Defender)

- **SQLite measured it:** "anti-virus software slows down direct-to-disk by an order of magnitude whereas it impacts SQLite writes very little", because many separate files each get scanned ([fasterthanfs](https://www.sqlite.org/fasterthanfs.html), MEASURED-EXT).
- **Performance mode is Dev Drive only.** It defers scans ("open now, scan later"), but "can run only on Dev Drive" (ReFS) and never applies to NTFS volumes ([MS Learn, updated 2026-09-15](https://learn.microsoft.com/en-us/defender-endpoint/microsoft-defender-endpoint-antivirus-performance-mode)). Dev Drive with Defender performance mode is claimed to speed builds by up to 30% (CLAIMED, [Windows blog](https://blogs.windows.com/windowsdeveloper/2023/06/01/dev-drive-performance-security-and-control-for-developers/)).
- **Exclusions need admin** (reading them failed here without elevation), so moirai cannot assume them.

**Design rules that follow:**
- (a) Keep a **small, stable set of files** per store; target fewer than 10.
- (b) Avoid create/delete churn per command: append to preallocated segments.
- (c) Never keep one file per node or per commit. Git's loose-object pattern is exactly what antivirus punishes.
- (d) Keep the executable's path stable. Defender re-scans rebuilt binaries; install to a fixed location instead of running from `target/`.

### 6.5 Portability note

Everything above is expressible with `windows-sys` on Windows and `libc` on Unix. The design keeps its portable core simple: a lock file, append plus flush, read-only mapping of immutable files, and atomic rename. Only the data-only flush (`NtFlushBuffersFileEx` vs `fdatasync`) and lock details are platform shims.

---

## 7. Log vs B+tree vs LSM for this workload

The workload (§3) is:
- small records,
- read-dominated, with many point and filter queries plus graph traversals,
- low write rate (human or agent pace: a few to a few hundred commits per minute, bursty),
- a hard requirement for history, branches and diffs,
- a total size ≤ ~1.3 GB, with typical projects likely 1e3–1e5 nodes.

| Property | Append-only log + checkpointed immutable segments | Copy-on-write B+tree (LMDB/redb) | LSM (fjall/RocksDB) |
|---|---|---|---|
| Write per commit | Ops appended (~100 B–2 KB) + 1 flush. Periodic checkpoint rewrites segments. | Path copy: depth × 4 KiB pages + 1 flush (redb 1PC+C) | WAL append + memtable; flush and compaction in background threads |
| Read path | mmap'd arrays, O(1) by dense id; + tail overlay | O(log n) page walk; ~0.6–1.1 µs per read (MEASURED-EXT) | memtable + levels + bloom filters; 2–3 µs per read (MEASURED-EXT) |
| Space amplification | Low for the current state. History grows with ops (compressible). | 1.3–2.4× vs compacted (MEASURED-EXT) + retained versions | Lowest after compaction |
| Versioning fit | **Native.** The log *is* history; branches are refs to log positions plus snapshots. Diff is op ranges. | Good: each commit is a root, and old roots are snapshots. But retaining them blocks page reuse, and diffing needs a tree walk. | Poor: needs sequence-number MVCC plus retention. SurrealKV does this, but at a RAM cost (CLAIMED 328 MB). |
| Multi-process readers | Trivial: immutable files; the head is read with a checksum | LMDB: yes. redb: experimental in 4.3. | Usually single process (fjall: no) |
| RAM floor | Tiny: mmap plus a small overlay | Tiny with mmap reads; explicit cache otherwise | Memtables + block cache + filters + threads |
| Open cost | Read head + map segments + replay the tail (bounded by checkpoint policy) | Read meta page(s); O(1) after a clean shutdown | Replay journal (up to 512 MiB default cap in fjall) + load manifests |
| Implementation risk from scratch | Medium: format, checkpointing, overlay merge | High: allocator, free-page GC, repair, split/merge | High: compaction, manifests, filters |

**Crash consistency pattern (all options).**
- Every log record carries a length, a checksum (xxh3-64), and a monotonically increasing LSN.
- The commit record carries the commit hash and an LSN range.
- On open, scan forward from the last known durable LSN and **ignore a torn tail** (first bad checksum means end of log). This is the WAL recovery rule used by SQLite, RocksDB and fjall.
- Pointer records such as HEAD use **two alternating slots**, each with a sequence number and checksum (LMDB meta pages, redb commit slots).
- **One flush per commit** when head information lives inside the log record (1PC+C); two flushes if the head is a separate file that must be durable.
- **Preallocate log segments.** Overwrite-in-place flushes are cheaper than appends that change file size:
  - here: 1.83 vs 1.93 ms (§2.2);
  - Linux: 1.24 vs 2.63 ms fdatasync ([BonsaiDb](https://bonsaidb.io/blog/durable-writes/), MEASURED-EXT).

**Checkpoint policy (ESTIMATE):**
- Checkpoint when the log tail exceeds ~4 MB or ~4,096 ops, or when an MCP server or daemon is idle.
- Tail replay at open is then bounded to about ≤1–3 ms.
- A checkpoint writes a new delta segment. Occasional *rollups* merge deltas into the base, in the style of TerminusDB rollups or LSM tiering.
- A full base rewrite takes roughly 10–30 ms at 1e4, 0.1–0.3 s at 1e5, and 1–3 s at 1e6 (sequential NVMe writes at 1–2 GB/s plus serialization). **At 1e6 checkpoints must be incremental.**

---

## 8. Storage cost of git-like versioning

| Mechanism | Cost per commit | Diff | Branch | Merge support | Notes |
|---|---|---|---|---|---|
| **Operation log (event sourcing)**, jj/TerminusDB-like | ~ Σ op sizes (100 B–2 KB), 1 flush | O(ops in range) | O(1) ref | 3-way at op/field level (needs a merge base) | Needs periodic snapshots for fast state reads |
| **Persistent CoW B+tree roots** (LMDB/redb shadow paging, kept) | depth × 4 KiB (~12–16 KiB at 1e5–1e6, ESTIMATE); pages retained forever unless squashed | Tree walk skipping shared page ids: O(changed pages) | O(1) root copy | Key-level 3-way from diffs | Keeping all history means ~12 KiB × commits: 10k commits ≈ 120 MB (ESTIMATE) |
| **Prolly tree** (Dolt) | "minimum of 4Kb multiplied by the depth of the tree" per edit ([Dolt](https://www.dolthub.com/blog/2024-03-03-prolly-trees/)) | Scales with the size of the difference; history-independent, so identical content means identical hashes | O(1) | Structural 3-way | Content addressing gives dedup and fast sync. Average chunk 4 KiB ([docs](https://docs.dolthub.com/architecture/storage-engine/prolly-tree)). |
| **Layer deltas** (TerminusDB) | Adds + removes per commit | Layer contents | Pointer to parent layer | Via deltas | Query cost grows with depth until rollup |

**Recommendation.**
- Make the **op log the durable history**: commits are hashed with BLAKE3, and the parent pointers form a DAG.
- Keep **materialized snapshots per branch head** (and optionally per tagged commit) as mmap'd segments.
- Answer "diff A..B" from ops when A and B share recent history, or from snapshot comparison otherwise.
- This keeps per-commit cost small (bytes, not 4 KiB × depth), and history compresses well with zstd.

**Stable global ids vs dense internal ids.** Merges across branches and worktrees require ids that do not collide when two branches create nodes independently.
- **External id:** a u64 made of (replica/actor 16 bit, counter 48 bit), or a random 64-bit value. The birthday collision probability for 1e6 random ids in 2^64 is about 2.7e-8 (ESTIMATE).
- **Internal id:** a dense **u32 row index per snapshot**, used by CSR arrays, bitmaps and columns, with an id-map from external to internal (sorted array or small hash) stored in the snapshot.

This gives compact structures without sacrificing mergeability.

---

## 9. On-disk layout and zero-copy serialization

### 9.1 Options (MEASURED-EXT unless noted)

The [rust_serialization_benchmark](https://github.com/djkoloski/rust_serialization_benchmark) latest run is 2026-09-10 on an AMD EPYC 9V74 with rustc 1.100 nightly. `log` dataset:

| Format | Serialize | Deserialize | Zero-copy access | Validated read | Size (bytes) |
|---|---|---|---|---|---|
| rkyv 0.8 | 178.9 µs | 1.219 ms | **1.09 ns** | **274.3 µs** | 1,011,488 |
| flatbuffers | 763.6 µs | n/a | 2.18 ns | 42.2 µs | 1,276,368 |
| capnp | 480.2 µs | 1.234 ms | 64.2 ns | 856.2 µs | 1,443,216 |
| bitcode | 96.8 µs | 1.145 ms | n/a | n/a | **703,710** |
| postcard | 367.0 µs | 1.740 ms | n/a | n/a | 724,953 |
| serde_json | 3.196 ms | 4.854 ms | n/a | n/a | 1,827,461 |

**Current state of the main crates:**

- **rkyv 0.8.18.**
  - Relative pointers, 32-bit by default, with the root stored at the **end** of the buffer. Little-endian and aligned by default; endianness, alignment and pointer width are configurable ([format](https://rkyv.org/format.html)).
  - Validation through `bytecheck` makes untrusted data safe ([validation](https://rkyv.org/validation.html)). The table shows validation costs about as much as deserializing.
  - **There is no schema evolution.** Changing a type means migrating data.
- **zerocopy 0.8.59.**
  - Derive `FromBytes` / `IntoBytes` / `KnownLayout` / `Immutable` / `Unaligned` / `TryFromBytes`.
  - Byte-order-aware integers (`little_endian::U32`).
  - `ref_from_bytes` / `ref_from_prefix` give compile-time-checked, **validation-free, safe** views for types where any bit pattern is valid ([docs](https://docs.rs/zerocopy/latest/zerocopy/)).
- **bytemuck 1.25.2** provides similar Pod casts, but alignment must be handled yourself.
- **flatbuffers 25.12.19** has cheap validated reads (42 µs above), schema evolution, and codegen. It is heavier to build with.

### 9.2 Recommended layout for moirai segments (design sketch)

**Fixed-width columns, "structure of arrays", u32 row index.** Each column is a `#[repr(C)]` little-endian array read through zerocopy:
- `id_ext: [u64]` (sorted, for binary search), or a separate sorted `(ext, row)` map;
- `kind: [u8]`, `status: [u8]`, `flags: [u16]`, `prio: [u8]`;
- `created, updated: [u32]` (seconds since a store epoch);
- `title_off: [u32]` into a title blob, and `body_off: [u32]` (plus a length) into a body blob holding zstd frames;
- `fields_off: [u32]` into a field-block blob.

**Adjacency (§10).**
- Forward CSR: `out_off: [u32; N+1]` with `out_dst: [u32]` and `out_type: [u8]`.
- Reverse CSR: the same, for incoming edges.

**Bitmaps.** One serialized roaring bitmap per status, kind and tag (§10.3).

**Dictionaries.**
- A symbol table for field names, kinds, tags and statuses (u16/u32 symbols): a small sorted string array.
- An FST for the FTS vocabulary.

**Field blocks.** Tagged varints: `(field_sym: varint, type: u8, value)`. Booleans take zero value bytes (the type carries true/false). Integers use zigzag varints; strings are a length plus bytes, or an interned symbol. This keeps schema evolution trivial.

**Integrity.** A segment footer holds column offsets, xxh3-64 checksums per column or block, the format version, and the BLAKE3 hash of the logical content.

**Why zerocopy over rkyv for the hot path.**
- Safe access with **no validation pass**: every column is plain-old-data and bounds-checked at slice time. Opening a segment costs O(1) instead of O(size).
- It is layout-stable and easy to version.
- rkyv remains reasonable for small, rarely read, complex blobs (for example rich "decision" documents) when their schema is owned by moirai.

**Integers.**
- LEB128 varints: 1 byte for values under 128, 2 bytes under 16,384, 3 bytes under 2,097,152.
- Delta-encode sorted adjacency and postings: neighbours created together, such as subtasks, often produce 1-byte deltas.
- Fixed u32 in CSR arrays gives O(1) random access. Delta-varint in postings and history gives size. Choose per structure.

**Strings and interning.** Interned symbols replace repeated strings (status names, tags, field names, kinds). Titles and bodies stay in blobs; never create one heap `String` per node in memory (§15).

---

## 10. Compact graph representation

### 10.1 Choices

| Representation | Bytes per edge (per direction) | Mutation | Traversal | Fit |
|---|---|---|---|---|
| KV "edge rows" in a B-tree (key = src‖type‖dst) | ~17 B key + B-tree overhead ≈ **30–40 B** (ESTIMATE) | O(log n) | Range scan per node | Simple, mutable, heavy |
| Per-node `Vec<u32>` in memory | 4 B + 24 B `Vec` header per node + allocator slop | O(1) amortized | Pointer chasing | Bad RAM per node (§15) |
| `SmallVec<[u32; 4]>` inline | 24 B inline (no heap up to 4 edges) | O(1) | Good | Fine for a *mutable overlay* |
| **CSR (offsets + targets)** | **4–5 B** (+ 4 B per node offsets) | Rebuild (immutable) | Sequential, cache-friendly | Snapshot segments (Kùzu, graph analytics) |
| CSR with delta-varint targets | ~1.5–2.5 B | Rebuild | Needs decode | Cold, large segments |
| Neo4j record format | 34 B relationship record (+15 B node) | O(1) by id | Linked-list chains | Reference only |

**Recommended hybrid** (the same shape as LSM or TerminusDB layers):
- An **immutable CSR in each snapshot segment**, forward and reverse.
- Plus a **small mutable overlay** built from the log tail: added and removed edges held in `SmallVec`s or a sorted Vec of `(src, type, dst, ±)`.
- Readers merge base and overlay on the fly.
- Checkpoints fold the overlay into a new CSR.

### 10.2 Referential consistency ("node 40 deleted → everyone knows immediately")

- With a **reverse index** (reverse CSR plus overlay), a delete is a single transaction: tombstone node 40 and emit edge-removal ops for every incoming and outgoing edge.
- MVCC snapshots mean every reader of the new head sees the consistent result. Readers on an older snapshot see the old consistent state.
- **Cost:** O(degree) ops per delete, and edges stored twice (≈2× edge bytes, i.e. about 30 B per node in §3).
- **Cross-process freshness.** A long-lived MCP server checks the head sequence number on each request:
  - reading a 4 KiB head file costs ~0.17 ms (§2.2);
  - polling a mapped head costs nanoseconds, because views are coherent across processes (§6.2).
- **Policy is a product decision** (§18): cascade, detach, or refuse to delete a node that still has references, and what happens to a merge that adds an edge to a node deleted on the other branch.

### 10.3 Roaring bitmaps for status, label and "is-blocker" sets

- **Containers** ([RoaringFormatSpec](https://github.com/RoaringBitmap/RoaringFormatSpec)):
  - array: **2 B per value** up to 4,096 values per 65,536-id chunk;
  - bitmap: **8 KiB fixed**;
  - run: 2 + 4 × runs bytes.
- **Worst case at N = 1e6:** 16 chunks, so ≤ 128 KiB per set. A 1% selective set of 10k ids is about 20 KiB (ESTIMATE).
- **Zero-copy frozen form:** CRoaring's "frozen" layout, exposed by the `croaring` 2.8.0 crate, can be queried in place from a mapped file. Pure-Rust `roaring` 0.11.5 deserializes (copies).
- **"Ids of all blocking tasks"** can be computed as `not_done ∧ has_outgoing(blocks)`. Maintain a derived `is_blocker` bitmap at checkpoint time, plus the overlay delta. The query is microseconds up to 1e6 (ESTIMATE), dominated by printing the ids.

### 10.4 Bytes per node and per edge (ESTIMATE, moirai-style SoA)

| Component | Bytes |
|---|---|
| Fixed columns (ids, kind, status, flags, prio, timestamps, 3–4 offsets) | 40–48 per node |
| CSR offsets (forward + reverse) | 8 per node |
| CSR targets + types (forward + reverse) | ~10 per edge (5 per direction) |
| Status, kind and tag bitmaps | ≈0.02–1 per node (set-density dependent) |
| **Topology + metadata total**, at 3 edges per node | **≈80–90 per node** |

---

## 11. Hashing: content addressing vs checksums

| Hash | Speed | Width | Cryptographic? | Source |
|---|---|---|---|---|
| XXH3 (AVX2 / SSE2) | 59.4 / 31.5 GB/s | 64/128 | No | [xxHash README](https://github.com/Cyan4973/xxHash), i7-9700K (MEASURED-EXT) |
| XXH64 | 19.4 GB/s | 64 | No | same |
| SHA-1 | 0.8 GB/s on i7-9700K (no SHA-NI); **2.0 GB/s here** | 160 | **Broken for collisions** (SHAttered 2017, Shambles 2020) | xxHash README; MEASURED-HERE; [git hash transition](https://git-scm.com/docs/hash-function-transition) |
| SHA-256 | **1.94 GB/s here** (SHA-NI) | 256 | Yes | MEASURED-HERE |
| BLAKE2b / 2s | 0.67 / 0.47 GB/s here | 512/256 | Yes | MEASURED-HERE |
| BLAKE3 | "4× BLAKE2b, 8× SHA-512, 12× SHA-256" single-threaded at 16 KiB on AVX-512 (Cascade Lake, which lacks SHA-NI). For inputs ≤1 KiB it "closely mirrors BLAKE2s". Multithreaded with rayon. | 256 (XOF) | Yes | [BLAKE3 paper](https://github.com/BLAKE3-team/BLAKE3-specs/blob/master/blake3.pdf) (MEASURED-EXT, different hardware) |

**For moirai.**
- **Commit and object hashes (content addressing, dedup, integrity across machines): BLAKE3-256.** It is cryptographic and has an excellent Rust crate (`blake3` 1.8.7, SIMD, no C). It may be truncated to 128–160 bits in indexes: a 128-bit hash only reaches birthday risk around 2^64 objects.
  - On *this* CPU with SHA-NI, SHA-256 is already 1.94 GB/s, so BLAKE3's advantage is ~2–3× at best on AVX2 (ESTIMATE, not measured).
  - Irrelevant for 100 B–10 KB records either way: under 1 µs per hash.
- **Page, block and record checksums: xxh3-64/128** (`xxhash-rust` 0.8.18 or `twox-hash` 2.1.4). redb uses XXH3-128 in its commit slots and offers two-phase commit specifically to defend against "theoretical collision attacks" on XXH3 with malicious input ([design.md](https://github.com/cberner/redb/blob/master/docs/design.md)).
- **SHA-1: no.** **SHA-256: only if moirai objects must interoperate with git.** Git 2.51 marked SHA-256 as the Git 3.0 default; forges lag, and GitHub had no SHA-256 repo support as of the cited reports ([LWN](https://lwn.net/Articles/1042172/), secondary).

---

## 12. Compression of text bodies

| Codec | Ratio (Silesia) | Compress | Decompress | Source |
|---|---|---|---|---|
| zstd 1.5.7 `-1` | 2.896 | 510 MB/s | 1,550 MB/s | [zstd README](https://github.com/facebook/zstd), i7-9700K @4.9 GHz (MEASURED-EXT) |
| zstd `--fast=4` | 2.146 | 665 MB/s | 2,050 MB/s | same |
| lz4 1.10.0 | 2.101 | 675 MB/s | 3,850 MB/s | same |
| lz4_flex 0.14 (safe), 66 KB JSON | — | 1,272 MB/s | 4,540 MB/s | [lz4_flex](https://github.com/PSeitz/lz4_flex), **Ryzen 7 5900HX** (almost the owner's CPU) |
| lz4_flex (safe), 10 MB Dickens | — | 259 MB/s | 2,338 MB/s | same |

**Small records need dictionaries.**
- Per-record compression of ~1 KB bodies without a dictionary gives poor ratios.
- zstd's dictionary builder targets exactly this. The README's reference set is "github-users", about 10k records of ~1 KB each.
- Secondary sources summarize the gain as moving from an average ratio of ~2.8 (level 3, small files) to about 10 with a dictionary. The gain is claimed to come "without any speed loss" ([Collet 2016](http://fastcompression.blogspot.com/2016/02/compressing-small-data.html), [zstd README](https://github.com/facebook/zstd); ratios CLAIMED/secondary; **measure on real moirai notes**).

**Recommendation.**
- Titles stay uncompressed, because listings must be fast.
- Bodies over ~200 B get **zstd level 3 with a per-store trained dictionary** (dictionary id stored in the segment; retrain at rollup).
- The log is compressed per batch (frame per checkpoint chunk). fjall 3 compresses journal values by default and reports "~15% latency reduction for compressible data" (CLAIMED, [Fjall 3](https://fjall-rs.github.io/post/fjall-3/)).
- lz4 only if profiling shows zstd decode on the hot path. At ~1.5 GB/s, decoding a 340 B body costs about 0.2 µs (ESTIMATE).

---

## 13. Full-text search and vectors with low RAM

| Option | RAM | Files | Latency (ESTIMATE unless cited) | Notes |
|---|---|---|---|---|
| **Brute force** over mmap'd zstd bodies (memchr / aho-corasick) | ~0 beyond page cache | 0 extra | 1e4 (≈3.4 MB compressed → ≈10 MB text): ~2–3 ms decompress + ~2–5 ms scan | Good enough at 1e4; no index maintenance |
| **Minimal inverted index**: FST term dictionary + postings (delta-varint or roaring) in the segment | Mapped; per query only the touched pages | 0 extra (inside segments) | 1e5–1e6: ~0.1–5 ms per term | FST sizes (MEASURED-EXT, [BurntSushi](https://burntsushi.net/transducers/)): 119k words → **324 KB** (built with 9.4 MB); 15.7M Wikipedia titles (384 MB) → **157 MB** in 18.3 s with 34.1 MB; 1.6B URLs (134 GB) → 27 GB with 56 MB. `fst` 0.4.7 is mature but last released in 2021. |
| **tantivy 0.26.2** (BM25, phrases, fuzzy) | Indexing: **≥15 MB per thread; "baseline consumption is 12MB"** ([index_writer.rs](https://github.com/quickwit-oss/tantivy/blob/main/src/indexer/index_writer.rs)). Search: mmap directory. | Many segment files (Defender cost, §6.4) | Claims "<10ms startup, perfect for command line tools" (CLAIMED, [README](https://github.com/quickwit-oss/tantivy)) | Turso stores tantivy files in 512 KB B-tree chunks with merges disabled (`NoMergePolicy`) to keep it transactional ([Turso](https://turso.tech/blog/beyond-fts5)); a pattern to borrow if tantivy is adopted |
| **SQLite FTS5** as a side index | ~2 MB page cache per connection | 1 (+WAL/-shm) | MemX (Rust + libSQL): FTS5 made keyword search "1,100x" faster at 100k records; end-to-end search "under 90 ms" ([arXiv 2603.16171](https://arxiv.org/abs/2603.16171), MEASURED-EXT) | Proven, but adds a second engine |

**Vectors (optional semantic recall).**

| N | f32 (1,536 B) | int8 (384 B) | binary (48 B) | Flat int8 scan (ESTIMATE, SIMD, 1 core) |
|---|---|---|---|---|
| 1e4 | 15 MB | 3.8 MB | 0.5 MB | < 1 ms |
| 1e5 | 154 MB | 38 MB | 4.8 MB | ~5–10 ms |
| 1e6 | 1.5 GB | 384 MB | 48 MB | ~50–100 ms: use a binary prefilter + int8 rerank, or HNSW |

- HNSW adds about M × 2 × 4–5 B per node at layer 0. `usearch` 2.26.2 offers uint40 neighbour ids, int8 and binary quantization, and can **"view large indexes from disk without loading into RAM"** ([usearch](https://github.com/unum-cloud/usearch)).
- **The embedding model dominates RAM, not the index.**
  - Static-embedding models such as model2vec `potion-base-8M` have 7.5M parameters (~30 MB in f32, ESTIMATE). They are claimed to be up to 50× smaller and 500× faster than sentence-transformers (CLAIMED, [model2vec](https://github.com/MinishLab/model2vec)).
  - Transformer models (e.g. MiniLM) cost ~90 MB or more (ESTIMATE).
  - Keep embedding generation **out of the core engine**: an optional feature or separate process, with vectors stored as a column.

---

## 14. CLI cold start, daemon vs embedded, and multi-process access

### 14.1 Latency decomposition of one agent CLI call on this machine

| Stage | Cost | Label |
|---|---|---|
| Agent Bash tool spawns Git Bash | ~93–109 ms p50 (under load) | MEASURED-HERE |
| Bash spawns `moirai.exe` | ~20–73 ms p50 (load-dependent) | MEASURED-HERE (similar Rust executables) |
| Open store: head read + map 2–4 files | ~0.2 ms per file (≈0.5–1 ms total) | MEASURED-HERE (open+map 0.22 ms) + ESTIMATE |
| Replay log tail (≤4k ops) | ≤1–3 ms | ESTIMATE |
| Query: point / list / bitmap | µs to ~1 ms | ESTIMATE |
| Durable write commit | ~1.8–2 ms | MEASURED-HERE |
| **Total** | **~115–190 ms, of which the engine is ≤ ~5 ms** | |

**Implications.**
1. The engine's cold path must never be O(history) or O(store size): no full-file validation, no index rebuild on open, and bounded tail replay.
2. Beyond that, making the engine faster does not help CLI users. **Skipping process spawns does.** An MCP server is a persistent stdio process per agent session. Each tool call is a JSON-RPC message over a pipe, and Windows pipe round trips are about **11 µs** (named pipe, byte mode) or 18 µs (anonymous pipe), against 19 µs for TCP loopback ([ipc-bench](https://github.com/smithtrenton/ipc-bench): Win11 26200, 7950X3D, 2026-09-04, MEASURED-EXT).
3. Avoid an async runtime in the CLI. A thread-per-core runtime is wasted work in a 5 ms process (ESTIMATE). Use `std` I/O and one thread.
4. Keep the executable at a stable install path; §6.4 covers Defender rescans.

### 14.2 Daemon vs embedded

| Model | Idle RAM | Per-call latency | Concurrency story | Failure modes |
|---|---|---|---|---|
| **Embedded everywhere** (CLI and MCP each open files directly) | **0** when nothing is running; per MCP process ~2–8 MB private + shared page cache | CLI: spawn + ~1–5 ms. MCP: µs–ms. | File-level protocol: lock file for writers, lock-free readers of immutable segments + checksummed head | Stale lock after a crash (Windows unlock delay, §6.3). Recovery runs in whichever process opens next. |
| **Shared daemon** (all clients go through it) | 5–50 MB always resident (ESTIMATE; depends on caches) | CLI: spawn **still paid** + ~11 µs IPC; saves only the ~1 ms open | Serializes writes trivially; can do group commit | Lifecycle (start, upgrade, stale sockets, per-user/per-repo), single point of failure |
| **Hybrid**: embedded by default; MCP servers act as warm clients; optional `moirai serve` for heavy features (FTS rebuild, embeddings, compaction) | 0 by default | Best of both | Same file protocol, so the daemon is only an optimization | Two code paths to test |

**Case study: beads** ([CHANGELOG](https://github.com/gastownhall/beads/blob/main/CHANGELOG.md), primary):
- **0.9.9 (2025-10-17)** added a daemon with RPC to "serialize SQLite writes" and fix "database corruption, git lock contention, and ID counter conflicts with multiple agents".
- **Later**, storage moved to Dolt. A secondary source says v0.50 removed the daemon (CLAIMED).
- **1.0.0 (2026-04-02)** made embedded Dolt the default, with an "exclusive flock… held for store lifetime". A "Second concurrent opener gets a clear error".
- **1.x** then grew server, shared-server and "proxied-server" modes.
- It also audited per-invocation startup costs. For example, a 20.2 ± 1.2 ms machine-id probe ran on every command on macOS, and an extra re-exec for telemetry was removed.

**Lessons:**
- (a) An exclusive-lock embedded engine is incompatible with a multi-agent workflow.
- (b) Retrofitting a daemon later is churn.
- (c) Audit the per-invocation fixed cost explicitly.

**Recommended multi-process protocol (engine-level, works without a daemon):**
- Directory `.moirai/` (or a per-user store) contains:
  - `LOCK`: an empty file, `LockFileEx`-ed by writers only.
  - `HEAD`: two 4 KiB slots, each holding a sequence number, the checksum, the committed LSN, the current segment set and the branch refs. Readers take the valid slot with the highest sequence number.
  - `log.NNN`: preallocated segments with append-only checksummed records.
  - `seg.NNN`: immutable snapshots, mapped read-only.
- **Writer:**
  1. `try_lock` on `LOCK` with backoff and timeout.
  2. Read HEAD and apply ops.
  3. Append the commit record carrying the new head information (1PC+C).
  4. Flush with the data-sync-only call.
  5. Optionally write the HEAD slot without a flush, since it can be recovered from the log.
  6. Unlock.
- **Readers never lock**:
  1. Read HEAD.
  2. Map the segments.
  3. Replay the log up to the committed LSN, validating checksums and stopping at the first bad one.
- **Segment GC:**
  - Delete a segment only when it is unreferenced by HEAD and older than a grace period.
  - Tolerate open handles: files are opened with `FILE_SHARE_DELETE`, and the delete is retried later.
  - Optionally keep a reader registry through shared byte-range locks on `LOCK` offsets, the way LMDB does, if exact reclamation is needed.

This mirrors LMDB's and redb 4.3's multi-process design but avoids byte-range locks on data files.

---

## 15. Allocators, arenas and avoiding per-node heap objects

**Why per-node objects are expensive (ESTIMATE).** A naive in-memory node looks like `struct Node { id: u64, kind: String, title: String, body: String, fields: HashMap<String, Value>, children: Vec<u64>, blocks: Vec<u64>, tags: Vec<String> }`:
- about **200 B inline**;
- 8–10 heap allocations, each with ~16–32 B allocator rounding and header;
- so roughly **350–450 B per node before content**, or 350–450 MB per 1e6 nodes;
- and poor locality.

The SoA plus arena layout in §9–§10 needs **≈80–90 B per node** for topology and metadata, and content stays in the mapped file.

**Rules.**
- In the CLI, **do not materialize nodes.** Answer queries straight from mapped columns and return borrowed `&str` from blobs, decompressing into a per-request arena.
- **Use a per-request bump arena** (`bumpalo` 3.20.3): allocate freely during one command or MCP request, then reset. There are no per-object frees and no fragmentation in long-lived MCP servers.
- **Mutable overlay:** sorted `Vec`s of POD tuples and `SmallVec<[u32; 4]>` (24 B inline) for small adjacency deltas. No `Rc`/`Arc` graphs.
- **Global allocator.**
  - Rust on Windows defaults to the process heap (`HeapAlloc`).
  - For a short, single-threaded CLI the allocator is noise next to a 20–70 ms spawn (ESTIMATE).
  - For long-lived multi-threaded servers, **mimalloc** (v3.5.3, 2026-09-16; Rust crate `mimalloc` 0.1.52) claims "~0.2% meta-data" overhead and says v3 "may use significantly less memory" for some workloads through better cross-thread sharing (CLAIMED, [mimalloc](https://github.com/microsoft/mimalloc)).
  - Decide by measuring private bytes and p99 on the MCP server, not by default.
  - `snmalloc-rs` 0.7.5 is an alternative.
  - `tikv-jemallocator` 0.7.0 support on MSVC was not verified here.
- **Threads:** none in the CLI. At most one background thread in the MCP server or daemon, for checkpoints and compaction. For comparison, fjall defaults to up to 4 worker threads per process.

---

## 16. Recommended storage-engine design space for moirai

The three options below are ordered by recommendation. All three share the multi-process protocol in §14.2, the Windows rules in §6, BLAKE3 commit ids with xxh3 checksums (§11), and SoA and arena memory rules (§15).

### Option A (recommended): "Log + immutable mmap'd snapshot segments" (an event-sourced columnar graph)

**Structure**
- An **append-only op log** holds history: commits form a DAG of BLAKE3-hashed commit records containing op batches.
- **Per-branch materialized state** lives in immutable segment files: SoA node columns, forward and reverse CSR, roaring or frozen bitmaps, the symbol table, the FST, postings, and zstd-with-dictionary body blobs, with a footer of checksums.
- Segments are mapped read-only and accessed through `zerocopy` views.
- A small **overlay** is built from the tail of the log since the last checkpoint.
- **Checkpoint** writes a delta segment. A **rollup** occasionally merges deltas into a new base (tiered, so 1e6 never rewrites everything at once).

**Why it fits**
- History, branches and diffs come almost for free.
- Readers are lock-free and share pages across processes.
- The RAM floor is tiny.
- Open time is O(1) plus bounded tail replay.
- There are few files, which suits Defender.
- It is fully portable.

**Risks**
- Segment-merge logic and correctness of overlay-plus-base reads.
- Heavy random-update bursts at 1e6 pile up overlays; the mitigation is more frequent delta checkpoints.
- A custom format needs a fuzzer and a crash-test harness (see §17).

### Option B: "Copy-on-write B+tree single file, with read-only mmap readers"

**Structure**
- One file of 4 KiB pages holding several trees:
  - nodes, keyed by external id;
  - edges forward and reverse (`src‖type‖dst`);
  - field index and bitmaps (stored as values);
  - commits, whose roots map commit to root page.
- Writes go through explicit `WriteFile` with a redb-like 1PC+C commit. Readers use a read-only map, the LMDB approach.
- **Versioning** keeps old roots as commits. Branches are named roots. Diff walks two trees and skips identical page ids. GC is reachability-based page reclamation, which needs a reader registry.
- History may still be kept as an op log so old roots can be dropped. This hybrid limits space growth to about 12–16 KiB per retained commit (ESTIMATE).

**Why it fits**
- The best general-purpose mutable structure: O(log n) random updates with no rebuilds.
- The fastest proven read path (LMDB: 0.64 µs per read, MEASURED-EXT).
- A single file.

**Risks**
- It is the hardest to build from scratch: page allocator, free lists, split and merge, repair after crash, and multi-process page reclamation.
- Windows cannot grow mappings in place, so remap on growth (the new view must be created for the larger size).
- Space amplification of 1.3–2.4× (MEASURED-EXT).

### Option C (prototype or cache only): "RAM-resident graph in a daemon + WAL + snapshot"

**Structure**
- A daemon loads the whole graph into SoA structures, appends ops to a WAL, and snapshots periodically. The snapshot can use the same zero-copy format as Option A, so load is just a map.
- CLI and MCP clients talk to it over a named pipe.

**Why consider it**
- The simplest code and the fastest queries in-process.
- It is a good **first prototype**, used to validate the data model and query language quickly.

**Why not as the final design**
- Idle RAM equals the dataset: about 10 MB at 1e4, 100–300 MB at 1e5, and 1+ GB at 1e6 (ESTIMATE). That breaks the hard RAM requirement on a 16 GB machine that already runs 16 agent processes.
- It adds daemon lifecycle problems (the beads lesson).
- It does not remove the CLI spawn cost.

### 16.1 Budgets (ESTIMATE; warm OS page cache; excluding process spawn, which §2.1 puts at 20–73 ms)

**Private RAM per process** (shared page-cache pages are listed separately)

| N | A: CLI | A: MCP server | B: CLI | B: MCP server | C: daemon |
|---|---|---|---|---|---|
| 1e4 | 1.5–3 MB | 3–8 MB | 1.5–3 MB | 3–8 MB | 15–30 MB |
| 1e5 | 2–4 MB | 4–10 MB | 2–4 MB | 4–12 MB | 100–300 MB |
| 1e6 | 3–6 MB | 6–16 MB | 3–6 MB | 8–24 MB | 1–2 GB |

**Shared page cache touched** (index-only / whole store)

| N | Option A | Option B | Option C |
|---|---|---|---|
| 1e4 | 0.9 / 9–13 MB | 1–2 / 12–25 MB | n/a (private) |
| 1e5 | 8.6 / 90–130 MB | 10–20 / 120–250 MB | n/a (private) |
| 1e6 | 86 / 0.9–1.3 GB | 100–200 / 1.2–2.5 GB | n/a (private) |

**Latency**

| Operation | N | Option A | Option B | Option C (in-daemon; add ~11 µs IPC) |
|---|---|---|---|---|
| Open store | 1e4 | 0.3–1 ms | 0.2–0.5 ms | n/a |
|  | 1e5 | 0.5–1.5 ms | 0.2–0.5 ms | n/a |
|  | 1e6 | 0.5–3 ms (bounded tail) | 0.3–0.8 ms | n/a |
| Get node by id | all | 1–5 µs | 1–3 µs | < 1 µs |
| "Ids of all blocking tasks" | 1e4 | 10–50 µs | 0.1–0.5 ms (edge scan or index) | ~10 µs |
|  | 1e5 | 50–300 µs | 0.5–3 ms | ~50 µs |
|  | 1e6 | 0.3–3 ms | 3–30 ms without a maintained bitmap | ~0.5 ms |
| Durable commit | all | ~2 ms (1 flush) | ~2 ms (1 flush, 1PC+C) | ~2 ms, or group commit |
| FTS query | 1e4 | 5–15 ms (brute force) | 5–15 ms | 1–5 ms |
|  | 1e5 | 1–5 ms (FST + postings) | 1–5 ms | < 1 ms |
|  | 1e6 | 2–20 ms | 2–20 ms | 1–5 ms |
| Checkpoint / compaction (background) | 1e4 | 10–30 ms | n/a | 10–30 ms snapshot |
|  | 1e5 | 0.1–0.3 s | occasional vacuum: ~0.2–0.5 s | 0.1–0.3 s |
|  | 1e6 | 1–3 s full rollup; deltas ~10–100 ms | ~2–5 s vacuum | 1–3 s |

**Cold caches** (after a reboot): add about 50–100 µs per 4 KiB page actually touched on consumer NVMe. That figure extrapolates the CIDR paper's "NVMe latency of roughly 100 µs"; it was not measured here. A point query touches about 3–6 pages, so ≈0.3–0.6 ms.

**Baselines.** Before writing Option A or B from scratch, implement the moirai workload against **redb 4.x** (pure Rust; one flush per commit; experimental multi-process) and **heed/LMDB** (mmap, multi-process) as correctness oracles and performance baselines. Also add SQLite (WAL, `synchronous=FULL`) as the conservative reference. The from-scratch engine must at least match them on the moirai-specific benchmarks in §17 to justify its existence.

---

## 17. Measurement plan (what to build first, before the engine)

1. **Harness.** Use `hyperfine -N --warmup 5` (not installed here; `winget install sharkdp.hyperfine` or `cargo install hyperfine`) for CLI spawn-to-exit, and a Rust bench binary for in-process operations. Record:
   - p50, p90 and p99;
   - **private bytes** and **peak working set** (`GetProcessMemoryInfo`);
   - the number of flush calls per command;
   - page faults (`PROCESS_MEMORY_COUNTERS.PageFaultCount`).
2. **Environments.**
   - Defender real-time on (the default);
   - optionally the store on a Dev Drive with performance mode, for comparison;
   - an idle machine vs a machine under the owner's typical load (N concurrent agents). Report both, because §2 shows load doubles spawn latency.
3. **Workloads.**
   - (a) Point get.
   - (b) List by status.
   - (c) "Blocking task ids".
   - (d) Transitive subtasks and blockers.
   - (e) Create/update/delete with referential cascade.
   - (f) Commit, branch, diff and merge.
   - (g) Full-text search.
   - (h) Many-process contention: 8–16 processes mixing reads and writes.
   - (i) Crash tests: kill -9 or power-cut simulation, torn writes, a truncated log.
4. **Scales.** 1e3 / 1e4 / 1e5 / 1e6 nodes, using synthetic corpora with realistic body sizes. **Measure the real ratio of zstd-with-dictionary on the owner's notes.**
5. **Baselines.** redb 4.x, heed, and SQLite (rusqlite, WAL) with the same record layout.
6. **Budget gates for CI:**
   - engine time ≤ 5 ms per CLI command at 1e5;
   - private memory ≤ 4 MB per CLI and ≤ 10 MB per MCP server at 1e5;
   - 1 flush per commit;
   - no O(N) work on open.

---

## 18. Open questions only the owner can answer

- **Scale and shape.** Realistic node counts per project and across all projects. Average body size. Does one store serve one repo, or one user across many repos?
- **Placement.**
  - Should the store live in the repo (e.g. `.moirai/`)?
  - Should it be committed to git: binary segments, or a text export such as op-log JSONL for git merges?
  - Or should it live outside the repo in a user-level directory?
  - This drives the Defender, file-count and merge strategy.
- **Worktrees vs moirai branches.**
  - Should each git worktree or agent session get its own moirai branch automatically, with merges when git branches merge?
  - Or is there one shared "live" state that all agents read and write concurrently?
- **Concurrency expectations.** The maximum number of processes writing at the same moment, and whether a writer may wait (and for how long) or must never block.
- **Durability.**
  - Is losing the last ~100 ms of writes after a power cut acceptable in exchange for group commit?
  - Or must every CLI write be flushed (~2 ms each on this SSD)?
- **Delete semantics.** Cascade, detach or refuse when references exist. How to resolve a merge where one branch deletes a node and the other adds an edge to it.
- **Search.**
  - Is semantic (embedding) search required?
  - If so, may moirai run a local embedding model, and within what RAM budget?
  - Or will embeddings come from the agent or an external API?
- **Environment control.** Is using a Dev Drive or adding a Defender exclusion acceptable (admin rights), or must moirai perform well with default Defender on NTFS?
- **Platforms.** Windows x64 only, or also Linux/macOS (CI, other machines) and Windows ARM64? This affects mmap and lock shims and the SIMD choices.
- **Dependencies.** Does "from scratch" allow small, well-audited crates (zerocopy, xxhash, blake3, zstd, roaring, fst, bumpalo), or must formats and codecs be hand-written too?
- **Daemon tolerance.** Is an optional background service acceptable at all, or must everything work with zero resident processes?

---

## 19. Sources

**Engines and storage**
- redb: <https://github.com/cberner/redb> · CHANGELOG <https://github.com/cberner/redb/blob/master/CHANGELOG.md> · design <https://github.com/cberner/redb/blob/master/docs/design.md> · bench params <https://github.com/cberner/redb/blob/master/crates/redb-bench/src/lib.rs> · Builder/ConcurrencyMode <https://github.com/cberner/redb/blob/master/src/db.rs> · v3.0.0 <https://github.com/cberner/redb/releases/tag/v3.0.0>
- fjall: <https://github.com/fjall-rs/fjall> · 3.0 post <https://fjall-rs.github.io/post/fjall-3/> · config <https://github.com/fjall-rs/fjall/blob/main/src/db_config.rs>
- sled: <https://github.com/spacejam/sled> · <https://github.com/spacejam/sled/issues/986>
- canopydb: <https://github.com/arthurprs/canopydb> · <https://github.com/arthurprs/canopydb/blob/master/BENCHMARKS.md>
- heed/LMDB: <https://github.com/meilisearch/heed> · LMDB source (mdb.master) <https://github.com/LMDB/lmdb/blob/mdb.master/libraries/liblmdb/mdb.c> · (0.9 branch) <https://github.com/LMDB/lmdb/blob/mdb.RE/0.9/libraries/liblmdb/mdb.c> · <https://github.com/Venemo/node-lmdb/issues/159> · <https://github.com/lmdbjava/lmdbjava/issues/68> · Symas <https://www.symas.com/post/are-you-sure-you-want-to-use-mmap-in-your-dbms>
- RocksDB memory: <https://github.com/facebook/rocksdb/wiki/Memory-usage-in-RocksDB>
- SQLite: <https://www.sqlite.org/wal.html> · <https://www.sqlite.org/pragma.html> · <https://www.sqlite.org/fasterthanfs.html> · <https://www.sqlite.org/fileformat.html> · <https://sqlite.org/forum/info/a30bc374143a41a265b0b61fea81cf27a37ee7df88ece408ca250d1b03cd08dc>
- Turso: <https://github.com/tursodatabase/turso> · <https://turso.tech/blog/beyond-fts5>
- native_db: <https://github.com/vincent-herlemont/native_db> · SurrealKV: <https://github.com/surrealdb/surrealkv>
- Kùzu (archived): <https://github.com/kuzudb/kuzu> · CIDR 2023 <https://www.cidrdb.org/cidr2023/papers/p48-jin.pdf>
- Neo4j on-disk: <https://neo4j.com/developer/kb/understanding-data-on-disk/>
- Dolt prolly trees: <https://docs.dolthub.com/architecture/storage-engine/prolly-tree> · <https://www.dolthub.com/blog/2024-03-03-prolly-trees/>
- TerminusDB whitepaper: <https://assets.terminusdb.com/research/succinct-data-structures-and-delta-encoding.pdf>
- beads CHANGELOG: <https://github.com/gastownhall/beads/blob/main/CHANGELOG.md>

**mmap and I/O research**
- Crotty, Leis, Pavlo, CIDR 2022: <https://db.cs.cmu.edu/papers/2022/cidr2022-p13-crotty.pdf>
- vmcache (SIGMOD 2023): <https://www.cs.cit.tum.de/fileadmin/w00cfj/dis/_my_direct_uploads/vmcache.pdf> · <https://github.com/viktorleis/vmcache>
- BonsaiDb durable writes: <https://bonsaidb.io/blog/durable-writes/>
- Small Datum, SSD fsync (Jan 2026): <http://smalldatum.blogspot.com/2026/01/ssds-power-loss-protection-and-fsync.html>

**Windows**
- LockFileEx: <https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-lockfileex>
- FlushFileBuffers: <https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-flushfilebuffers>
- FlushViewOfFile: <https://learn.microsoft.com/en-us/windows/win32/api/memoryapi/nf-memoryapi-flushviewoffile>
- SetEndOfFile: <https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-setendoffile>
- CreateFileMapping: <https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-createfilemappinga>
- Working Set: <https://learn.microsoft.com/en-us/windows/win32/memory/working-set>
- NtFlushBuffersFileEx: <https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/nf-ntifs-ntflushbuffersfileex>
- PostgreSQL volatile write caches thread: <https://www.postgresql.org/message-id/CA+hUKG+F0EL4Up6yVYbbcWse4xKaqW4wc2xpw67Pq9FjmByWVg@mail.gmail.com>
- Defender performance mode: <https://learn.microsoft.com/en-us/defender-endpoint/microsoft-defender-endpoint-antivirus-performance-mode>
- Dev Drive blog: <https://blogs.windows.com/windowsdeveloper/2023/06/01/dev-drive-performance-security-and-control-for-developers/>
- Windows memory-mapped I/O (Jeremy Ong): <https://www.jeremyong.com/winapi/io/2024/11/03/windows-memory-mapped-file-io/>
- Rust std Windows fs impl: <https://github.com/rust-lang/rust/blob/main/library/std/src/sys/fs/windows.rs> · File::lock stabilization <https://github.com/rust-lang/rust/pull/136794>
- OS primitive benchmarks: <https://www.bitsnbites.eu/benchmarking-os-primitives/> · cargo/Defender <https://github.com/rust-lang/cargo/issues/5028>
- Windows IPC bench (2026-09-04): <https://github.com/smithtrenton/ipc-bench>

**Serialization, hashing, compression, search**
- rust_serialization_benchmark: <https://github.com/djkoloski/rust_serialization_benchmark> · rkyv format <https://rkyv.org/format.html> · validation <https://rkyv.org/validation.html> · zerocopy <https://docs.rs/zerocopy/latest/zerocopy/>
- xxHash: <https://github.com/Cyan4973/xxHash> · BLAKE3 paper <https://github.com/BLAKE3-team/BLAKE3-specs/blob/master/blake3.pdf> · BLAKE3 repo <https://github.com/BLAKE3-team/BLAKE3>
- Git hash transition: <https://git-scm.com/docs/hash-function-transition> · LWN <https://lwn.net/Articles/1042172/>
- zstd: <https://github.com/facebook/zstd> · small-data dictionaries <http://fastcompression.blogspot.com/2016/02/compressing-small-data.html> · lz4_flex <https://github.com/PSeitz/lz4_flex>
- tantivy: <https://github.com/quickwit-oss/tantivy> · memory budget <https://github.com/quickwit-oss/tantivy/blob/main/src/indexer/index_writer.rs>
- FST: <https://burntsushi.net/transducers/>
- MemX (arXiv 2603.16171): <https://arxiv.org/abs/2603.16171>
- usearch: <https://github.com/unum-cloud/usearch> · model2vec <https://github.com/MinishLab/model2vec>
- Roaring format: <https://github.com/RoaringBitmap/RoaringFormatSpec> · croaring <https://docs.rs/croaring>
- mimalloc: <https://github.com/microsoft/mimalloc>

**Crate versions and dates** (crates.io API, 2026-09-25): redb 4.3.0 (09-15), fjall 3.1.10 (08-30), lsm-tree 3.1.10, heed 0.22.1 (04-07), lmdb-master-sys 0.2.6, sled 0.34.7, canopydb 0.2.5 (2025-11-22), rocksdb 0.25.0 (08-16), rusqlite 0.40.2, turso 0.7.2 (09-25), native_db 0.8.2 (2025-07-08), surrealkv 0.21.4, rkyv 0.8.18, zerocopy 0.8.59, bytemuck 1.25.2, flatbuffers 25.12.19, blake3 1.8.7, xxhash-rust 0.8.18, twox-hash 2.1.4, roaring 0.11.5, croaring 2.8.0, tantivy 0.26.2, fst 0.4.7 (2021-06-06), zstd 0.14.0, lz4_flex 0.14.0, usearch 2.26.2, hnsw_rs 0.3.4, sqlite-vec 0.1.9, mimalloc 0.1.52, snmalloc-rs 0.7.5, tikv-jemallocator 0.7.0, bumpalo 3.20.3, smallvec 1.16.2, memmap2 0.9.11.
