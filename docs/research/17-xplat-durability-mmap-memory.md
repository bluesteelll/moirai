# 17 — Cross-platform durability, mmap and memory accounting (Linux, macOS, Windows)

Lens: **durability, memory mapping and memory accounting** for moirai now that the owner has made Linux and macOS first-class
targets alongside Windows (owner requirement, 2026-09-26). This report gives the Linux and macOS equivalents of every
Windows-specific mechanism in this lens, their hazards, and one abstraction that keeps **one on-disk format and one protocol
semantics** on all three operating systems.

Inputs read: [00-phase1-digest], [05-rust-storage-perf-ram] §2, §5, §6, §14, §15; [08-concurrency-sync-git-interop] §8;
[ARCHITECTURE-RESEARCH] §2.8, §4 (esp. §4.1, §4.5, §4.7, §4.10), §6.1, §6.5, §8.1–§8.2; [40-file-links-design] §2.6;
[60-roadmap] §2.5 (`Vfs` fault model and protocol decisions), M0 items 1/11/17, M1 gates, GT15, measurement protocol.

Other lenses (locks and IPC, file identity, agent integration and quoting) are out of scope here. They are mentioned only
where they touch durability or memory.

---

## 0. How to read this report

**Evidence tags:**

| Tag | Meaning |
|---|---|
| [M] | measured in this session (probes described in §0.1) |
| [D] | vendor or project documentation (man page, docs site) |
| [S] | source code read in this session (kernel, XNU, libc, Rust std, databases) |
| [C] | third-party claim or measurement (blog, issue tracker, mailing list, benchmark post) |
| [I] | inference by this report |

### 0.1 Probes and environment

- **No Linux probe was possible.** `wsl -l -v` and `wsl --status` both returned "The Windows Subsystem for Linux is not
  installed" [M]. Docker, Podman and QEMU are not on the PATH either [M]. Every Linux statement below is therefore [D], [S] or [C].
- **No macOS is available.** macOS statements come from the XNU sources, the Apple man pages in the XNU tree, Apple
  developer documentation, and measurements published by others.
- **Windows probes** (Rust 1.98.1, release build, Windows 11 26200, NTFS on C:, Defender on, 90–95 % CPU load from other
  agents) ran as local probe programs (probe scripts are not published):
  - `dirsync/` checks what a portable "make this directory's entries durable" operation maps to on Windows (§3.4);
  - `flushfloor/` re-measures the 4 KiB flush floor, to give the cross-OS tables a same-day Windows baseline (§3.2).
- Web sources were fetched on 2026-09-26. Source files were read from the upstream `master`/`main` branches on that day.

---

## 1. Executive summary

1. **macOS `fsync` is not a durability primitive.** Apple's man page says `fsync` moves data to the drive, but the drive
   "may not physically write the data … for quite some time and it may be written in an out-of-order sequence". The call
   that flushes the drive cache is `fcntl(F_FULLFSYNC)` [D].
   - Rust std already maps both `File::sync_data` and `File::sync_all` to `F_FULLFSYNC` on Apple targets, with no fallback
     [S]. Go, LMDB and RocksDB do the same [S].
   - Apple's bundled SQLite quietly turns `PRAGMA fullfsync` into `F_BARRIERFSYNC` [C]. The man page describes that barrier
     as ordering only: "no assumption should be made on what has been persisted" [D].
   - moirai's `durable` class must therefore be `F_FULLFSYNC` on macOS, and nothing weaker.

2. **A durable flush costs much more on Apple silicon than on the owner's Windows NVMe.**
   - M1-class machines: about 17–20 ms per flush, i.e. 55–60 IOPS (marcan 2022; Jens Axboe's 2025 kernel patch; geth
     issue 2024) [C].
   - M3 and M5 machines: p50 of 3.1–3.9 ms, p99 of 11.6 ms, and 10.7 ms median with 12 concurrent writers [C].
   - The owner's Windows machine: 0.45–1.9 ms p50 [M]. Consumer NVMe on Linux ext4: 0.45–2.8 ms `fdatasync` [C].
   - Two consequences for the design as written:
     - The 16-writer p99 ≤ 50 ms gate cannot hold on macOS while every writer flushes serially inside the writer lock.
     - T8's own revisit trigger ("sustained flush p99 > 10 ms") already fires on a current MacBook Pro.
   - Group commit therefore has to be settled at M0, for all three OSes, as a semantics-preserving part of the protocol.
     It can no longer be an optional Windows-driven leader.

3. **Linux `fdatasync` is the right `durable` mapping, and the zero-filled-extent trick (G11) carries over to ext4 and
   XFS.**
   - `fdatasync` flushes the metadata "needed … for a subsequent data retrieval", including a size change [D].
   - On ext4 a data-sync of a pure overwrite waits only for an already-committed transaction and then sends one device
     flush [S].
   - Unwritten extents from `fallocate` cost about 5× in O_DSYNC overwrite IOPS (20.1k vs 98.8k) [C, LWN].
     `FALLOC_FL_WRITE_ZEROES` (ext4 from Linux 6.17, XFS from 7.3) creates written, zeroed extents cheaply [C].
   - On copy-on-write file systems (APFS, btrfs, ZFS) zero-filling buys nothing.
   - Keep **one format** ("a log extent reads as zeros beyond its tail") and make **how** an extent is created a per-file-system choice.

4. **Namespace durability must become an explicit `Vfs` operation.**
   - Linux requires `fsync` on the directory for a create, rename or unlink to survive a crash [D].
   - macOS needs `fsync(dir)` followed by an `F_FULLFSYNC` barrier [D][C].
   - On Windows, `FlushFileBuffers` on a directory handle works only when the handle was opened with
     `FILE_FLAG_BACKUP_SEMANTICS` and write access: it took 0.07 ms p50, and a read-only handle fails with error 5 [M].
     Rust std cannot open a directory on Windows without that flag [M].
   - The fault model's current rule, "metadata operations survive as a prefix of issue order", is NTFS-shaped. It is not
     guaranteed on btrfs or with ext4 fast-commit [I]. Replace it with "a metadata operation is durable only after
     `sync_dir` of its parent".

5. **Behaviour after a failed fsync differs by OS and file system** (ATC'20 [D]; PostgreSQL wiki [C]):
   - ext4 and XFS keep the new bytes in pages that are now marked clean;
   - btrfs reverts the page to the old content;
   - macOS invalidates the buffers;
   - Linux reports a write-back error once per open file description.

   Keep "flush failure is fatal". Fix M0 protocol decision (a) to **re-write the record's bytes, then flush**; this works
   on every OS. Drop the "verify through an unbuffered read" alternative, which is not portable: O_DIRECT needs alignment,
   and `F_NOCACHE` is only a hint.

6. **mmap: Unix removes the protection Windows gave for free.**
   - On Windows a mapped file cannot be truncated. On Linux and macOS any process with write permission can truncate it.
     The next access past the new end of file, or any page-in I/O error, raises `SIGBUS` (Linux `mm/filemap.c`; XNU
     `ux_exception.c`) [S].
   - Rust cannot recover from that. `MAP_NOSIGBUS` was proposed in 2021 but is not in mainline [C].
   - Precedents: ripgrep and MaxMind readers crash the same way when a file is truncated under them, for example by `cp`.
   - **One mmap policy works on all three OSes:**
     - map only sealed, immutable, fully flushed files, read-only and whole-file;
     - set sealed files read-only (`0444` or the read-only attribute) so casual truncation fails;
     - never truncate, extend, rename over or reuse a mapped file's name;
     - check the file size against the durable checkpoint before mapping;
     - read mapped bytes only through all-bit-patterns-valid types with checked bounds;
     - turn `SIGBUS` or `EXCEPTION_IN_PAGE_ERROR` inside a registered mapping into a crash-class exit that names the file.
       The protocol already survives a crash.

7. **Memory accounting needs a per-OS definition of "private".**

   | OS | Private metric | Peak available? |
   |---|---|---|
   | Windows | `PeakPagefileUsage` (commit charge) | yes |
   | macOS | `phys_footprint` = anonymous + compressed + IOKit + purgeable-nonvolatile + page tables, excluding clean file pages [S] | yes: `ledger_phys_footprint_peak` [S] |
   | Linux | `RssAnon` / `Pss_Anon` | no peak counter; `/proc/*/status` values "may not be very precise" [D] |

   - Recommended Linux measure: a per-run cgroup-v2 leaf with `memory.peak`, with the store files cached from outside that
     cgroup [D], cross-checked against `smaps_rollup` at exit.
   - The baselines differ: a minimal macOS process already has a footprint of about 2 MB [C], against 0.69 MB private on
     Windows [M].
   - Gate `private_peak − private_peak(empty binary, same build) ≤ 3.3 MB` on all three OSes, plus a portable `heap_peak`
     from a counting allocator.

8. **Process spawn on Linux and macOS costs 10–100× less than on Windows:**

   | OS | Spawn-to-exit | Source |
   |---|---|---|
   | Linux | about 0.15–1 ms | [C] |
   | macOS | about 1.5 ms steady state, plus a 0.3–5 s XProtect scan on the first launch of each new or changed binary | [C] |
   | Windows | 15–74 ms | [M] |

   - The CLI path becomes cheap on Unix, so the engine's O(1) open path and ≤ 5 ms budget dominate there.
   - T9 (CLI + skill primary, ten-tool MCP) does not change.
   - On macOS the flush cost, not the spawn cost, is what makes a long-lived process valuable. It gives group commit a
     natural home.

---

## 2. Inventory: which mechanisms in this lens are OS-specific today

| Design element (where) | Windows mechanism (current design) | Linux equivalent | macOS equivalent | Main hazard off Windows |
|---|---|---|---|---|
| `durable` commit flush [AR §2.8, §4.5 step 7, §6.5] | `NtFlushBuffersFileEx(FLUSH_FLAGS_FILE_DATA_SYNC_ONLY)` | `fdatasync` (alternative: `O_DIRECT` + `RWF_DSYNC` FUA write, §3.1) | `fcntl(fd, F_FULLFSYNC)` | macOS `fsync` does not flush the drive cache; `F_FULLFSYNC` costs 3–20 ms |
| Log extent creation, "zero-filled once + one full `FlushFileBuffers`" (G11) [AR §4.1] | write zeros + `FlushFileBuffers` | write zeros, or `fallocate(FALLOC_FL_WRITE_ZEROES)` (ext4 ≥ 6.17, XFS ≥ 7.3), + `fsync` + `fsync(dir)` | `ftruncate` (sparse) + `F_FULLFSYNC` + dir sync | on CoW file systems (APFS, btrfs, ZFS) an overwrite still allocates; zero-fill only wastes writes |
| `HEAD` published without flush; durable `HEAD` barrier before deletion [AR §4.2; 60 §2.5 (c)] | `FlushFileBuffers(HEAD)` | `fsync(HEAD)` (or `fdatasync`: fixed size) | `F_FULLFSYNC(HEAD)` | none, once mapped |
| Segment written under a temp name, then referenced by a flushed `Checkpoint` [AR §4.1, §4.10] | create + flush (NTFS journal orders metadata) | needs `fsync(dir)` before the `Checkpoint` that names the file | `fsync(dir)` + barrier | a durable `Checkpoint` could name a file whose directory entry was lost |
| Read-only mapping of sealed files [AR §4.1, §4.7] | `CreateFileMappingW` / `MapViewOfFile`, `PAGE_READONLY`; truncation impossible while mapped | `mmap(PROT_READ, MAP_SHARED)`; truncation allowed → `SIGBUS` | same; `SIGBUS` | an external `cp` or `truncate` crashes readers |
| Delete-pending tolerance, `FILE_SHARE_DELETE` [AR §4.1] | delete-pending until the last handle closes | `unlink` is immediate; open and mapped inodes stay valid | same | none (simpler) |
| Flush failure is fatal [AR §6.5] | behaviour after failure unknown [C] | error reported once per open file description; pages marked clean (ext4/XFS) or reverted (btrfs) | buffers invalidated; a later `fsync` may succeed [C] | retrying the flush proves nothing; adopting by re-flush is wrong |
| `Vfs` fault model [60 §2.5] | NTFS wording: `DATA_SYNC_ONLY`, metadata as a prefix of issue order, `ERROR_DISK_FULL`, errors 5/32 | needs `fsync(dir)`, `ENOSPC` on overwrite (CoW), `EIO` semantics | needs `F_FULLFSYNC`/barrier semantics, `ENOSPC` on overwrite (APFS CoW) | the model must be the **weakest** of the three |
| RSS gate: "peak private bytes via `GetProcessMemoryInfo`" [60 §5.1] | `PeakPagefileUsage` | no peak anonymous counter; `RssAnon`, `Pss_Anon`, cgroup `memory.peak` | `phys_footprint`, `ledger_phys_footprint_peak`, `ri_lifetime_max_phys_footprint` | different definitions and baselines |
| Physical floors (M0 items 1 and 11) | owner's NVMe | Linux host (ext4 + XFS, consumer NVMe) | Apple-silicon Mac (internal SSD) | floors differ ×10 between OSes |
| OS-crash rig (GT15) | Windows guest in VirtualBox/VMware, host cache off | `dm-log-writes` / `dm-flakey`; QEMU/KVM `cache=none` | Virtualization.framework VM, `VZDiskImageSynchronizationMode.full` | needs owner hardware for macOS |
| Spawn budget [AR §8.1] | 15–73 ms + Git-Bash ~109 ms | ~0.15–1 ms | ~1.5 ms; XProtect on first exec | the engine share grows |
| Per-file AV cost (Defender) [AR §4.10] | per open or close | only enterprise EDR via `fanotify` permission events [I] | XProtect / Gatekeeper on the first launch of a new binary [C] | install at a stable path; sign and notarize on macOS |
| Store refusal on network and OneDrive paths [AR §4.1] | OneDrive, UNC | NFS, SMB/CIFS, FUSE (incl. sshfs), 9p (WSL `/mnt/*`), virtiofs, tmpfs, overlayfs `volatile` | not `MNT_LOCAL`; iCloud Drive / File Provider roots; SMB/AFP/NFS/WebDAV | durability and mmap semantics break silently |

---

## 3. Durability

### 3.1 Primitive semantics per OS

| Primitive | Data to the device | Drive cache flushed | Metadata | Notes | Source |
|---|---|---|---|---|---|
| **Linux `fsync(fd)`** | yes | yes (cache flush / FUA; the "barrier" is on by default) | all inode metadata | does **not** make the directory entry durable: "an explicit fsync() on a file descriptor for the directory is also needed" | [D] [fsync(2)](https://man7.org/linux/man-pages/man2/fsync.2.html) |
| **Linux `fdatasync(fd)`** | yes | yes | only metadata "needed … to allow a subsequent data retrieval", e.g. a size change; not mtime | ext4: waits for the inode's `i_datasync_tid` transaction, then `blkdev_issue_flush` if needed [S] [ext4/fsync.c](https://github.com/torvalds/linux/blob/master/fs/ext4/fsync.c); `fdatasync` on ext3/ext4 was broken before 3.6 (LMDB comment) [S] | [D] fsync(2) |
| **Linux `O_DSYNC` / `pwritev2(RWF_DSYNC)`** | yes | yes | as `fdatasync` | buffered: equals write + `fdatasync`. **`O_DIRECT` + DSYNC** into already-written blocks can use a single **FUA** write and skip the cache flush: "Use a FUA write if we need datasync semantics and this is a pure overwrite" [S] [iomap/direct-io.c](https://github.com/torvalds/linux/blob/master/fs/iomap/direct-io.c) | [S] |
| **Linux `sync_file_range`** | starts write-back only | **no** | **no** | "extremely dangerous … does not flush disk write caches and thus does not provide any data integrity" | [D] [sync_file_range(2)](https://man7.org/linux/man-pages/man2/sync_file_range.2.html) |
| **Linux `syncfs` / `sync`** | whole file system | yes | yes | useful for `backup`/`doctor`, not for commits; before 5.8, `syncfs` did not report write-back errors (PostgreSQL docs) | [D] [PG error handling](https://www.postgresql.org/docs/current/runtime-config-error-handling.html) |
| **macOS `fsync(fd)`** | to the drive | **no**; the drive may reorder | yes | "This is not a theoretical edge case. This scenario is easily reproduced with real world workloads and drive power failures." | [D] [XNU fsync.2](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/man/man2/fsync.2) |
| **macOS `fcntl(F_FULLFSYNC)`** | yes | **yes** | yes | "As this drains the entire queue of the device and acts as a barrier, data that had been fsync'd on the same device before is guaranteed to be persisted when this call returns." Implemented on HFS, FAT, UDF, APFS; "Certain FireWire drives have also been known to ignore the request" | [D] [XNU fcntl.2](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/man/man2/fcntl.2) |
| **macOS `fcntl(F_BARRIERFSYNC)`** | yes | barrier only | yes | "guaranteed to be persisted before any other I/O that would follow the barrier, although no assumption should be made on what has been persisted or not when this call returns"; HFS and APFS; "requires hardware support, which Apple SSDs are guaranteed to provide" | [D] XNU fcntl.2 |
| **macOS `fdatasync`** | — | no | — | exists as an undeclared syscall; per darwin-xnu it shares code with `fsync` and does not flush drive caches (T. Munro, pgsql-hackers, 2021-01-15) | [C] [pgsql-hackers](https://www.postgresql.org/message-id/CA+hUKGLfe_ogA5VDi1Jxj_MPVJLcif8T_GuaW0aOs+pBZT033A@mail.gmail.com) |
| **Windows `NtFlushBuffersFileEx(DATA_SYNC_ONLY)`** | yes | yes | "only metadata that is necessary for data retrieval" | current design | [05 §6.2] |
| **Windows `FlushFileBuffers`** | yes | yes | yes | Rust std `sync_data` == `sync_all` == this call | [05 §6.2] |
| **Windows `FILE_FLAG_WRITE_THROUGH`** | — | not reliable on consumer drives | — | rejected in [05 §6.2] | [C] |

**Rust std** [S] ([library/std/src/sys/fs/unix.rs](https://github.com/rust-lang/rust/blob/master/library/std/src/sys/fs/unix.rs)):

| Target | `File::sync_all` | `File::sync_data` |
|---|---|---|
| Apple | `fcntl(F_FULLFSYNC)`, no fallback on `ENOTSUP` | `fcntl(F_FULLFSYNC)`, no fallback on `ENOTSUP` |
| Linux, FreeBSD, … | `fsync` | `fdatasync` |
| Windows | `FlushFileBuffers` | `FlushFileBuffers` |

`File::lock` on Unix is `flock`, not a byte range. That belongs to the concurrency lens. Note that redb takes
`F_OFD_SETLK` byte-range locks at 2^62 on **both** Linux and Apple [S]
([range_lock.rs](https://github.com/cberner/redb/blob/master/src/tree_store/page_store/file_backend/range_lock.rs)).

### 3.2 Costs

| Platform / device | Operation | Latency | Source |
|---|---|---|---|
| Owner's Win 11, SK hynix HFM512GD3JX013N, NTFS, earlier session | 4 KiB overwrite + `DATA_SYNC_ONLY` | p50 1.73 ms, p99 3.3 ms | [M, 05 §2.2] |
| same machine, **this session**, 90–95 % CPU | 4 KiB overwrite into a zero-filled 8 MiB file + `sync_data` (`FlushFileBuffers`), n = 580 | p50 **0.451** ms, p90 0.536, p99 0.987 | [M] `flushfloor/` |
| same machine, this session | 4 KiB append + `sync_data`, n = 580 | p50 0.456 ms, p99 0.916 | [M] |
| same machine, this session | create + 4 KiB + `sync_data`, n = 200 | p50 0.67–0.76 ms | [M] `dirsync/` |
| Linux ext4, Samsung 990 Pro (no PLP) | 16 KB `fdatasync` / `fsync` | 2.78 / 2.97 ms | [C] [Small Datum 2026-01](http://smalldatum.blogspot.com/2026/01/ssds-power-loss-protection-and-fsync.html) |
| Linux ext4, Crucial T500 | 16 KB `fdatasync` / `fsync` | 0.45 / 0.89 ms | [C] same |
| Linux ext4, enterprise NVMe with PLP | 16 KB `fdatasync` | 0.7–10 µs | [C] same |
| Linux ext4 (Zhang Yi's test device) | O_DSYNC 4k overwrite IOPS: unwritten extents vs written extents | 20.1k vs 98.8k IOPS | [C] [LWN 1018299](https://lwn.net/Articles/1018299/) |
| Linux on Apple M1 NVMe (Asahi) | cache flush | "17-18 msec … 55-60 IOPS" | [C] [J. Axboe, LKML 2025-02-11](https://lkml.iu.edu/2502.1/04706.html) |
| macOS, M1 | 1-sector write + `F_FULLFSYNC` | ~58 IOPS (~17 ms), "~20ms to flush to NAND" | [C] [marcan, HN 2022](https://news.ycombinator.com/item?id=30370551) |
| macOS, M1 Pro | 1,000 × Go `File.Sync` (= `F_FULLFSYNC`) | 18 s total (~18 ms each) | [C] [go-ethereum #28754, 2024-01](https://github.com/ethereum/go-ethereum/issues/28754) |
| macOS 26.6.2, MacBook Pro M3 (Mac15,3) | SQLite commit with `fullfsync=ON` | p50 **3.1 ms**, p99 **11.6 ms**, max 19.0 ms (282 commits/s) vs p50 0.04 ms with plain `fsync` | [C] [t3code #13544, 2026-09](https://github.com/pingdotgg/t3code/issues/13544) |
| macOS, M5 Pro | `F_FULLFSYNC` median, 1 writer / 12 writers | **3.9 ms / 10.7 ms** (plain `fsync` 0.02 / 0.10 ms) | [C] [pnpm PR #15354](https://github.com/pnpm/pnpm/pull/15354) |
| macOS (unspecified Mac) | 5.2 MB payload: `fsync` / `F_FULLFSYNC` / Rust `sync_all` | 0.95 / 6.8 / 8.1 ms | [C] [vanedb #110](https://github.com/vanedb/vanedb/issues/110) |
| older Intel MacBook Pro | `pg_test_fsync` `fsync_writethrough` (`F_FULLFSYNC`) | ~41 ops/s (24 ms) | [C] [pgsql-general, HFS+ thread](https://www.postgresql.org/message-id/CF71BE2D.15126%25mllaguno@coverity.com) |

**Readings.**

- The Windows flush floor on this very machine was **0.45 ms today and 1.73–1.93 ms in [05]**, both under load [M].
  - The registry shows no "turn off write-cache buffer flushing" override: no `CacheIsPowerProtected` or
    `UserWriteCacheSetting` values under the disk's device parameters [M].
  - The cause is unknown, perhaps drive state. It confirms that **floors must be re-measured interleaved in the same
    run on every OS**, as [60 §5.1] already requires.
- **Apple silicon is 2–10× slower per durable flush** than the owner's NVMe, and the p99 is heavy.
  - `F_FULLFSYNC` drains the whole device queue [D]. Concurrent callers therefore partly combine (12 writers: 10.7 ms
    median instead of 12 × 3.9 ms) [C]. A single process holding a lock gets no such combining.
- Plain `fsync` on macOS at 0.02–0.04 ms is the number that makes "macOS databases are fast" benchmarks misleading
  [C]. **Any cross-OS benchmark of moirai must state the flush primitive.**

### 3.3 What established engines do per platform

| Engine | Linux | macOS | Windows | Source |
|---|---|---|---|---|
| **SQLite (upstream)** | `fdatasync` only when built with `HAVE_FDATASYNC`; otherwise `fsync`. "We do not trust systems to provide a working fdatasync()" | `F_FULLFSYNC` **only** with `PRAGMA fullfsync` (default off); on failure "fall back to attempting an fsync()"; the directory sync uses plain `fsync` and ignores errors | `FlushFileBuffers` | [S] [os_unix.c](https://github.com/sqlite/sqlite/blob/master/src/os_unix.c) `full_fsync()`, `unixSync()` |
| **SQLite (Apple's system build)** | — | `PRAGMA fullfsync=ON` is implemented as `F_BARRIERFSYNC`; the source is not public | — | [C] [BonsaiDB 2022](https://bonsaidb.io/blog/acid-on-apple/), [M. Tsai 2025](https://mjtsai.com/blog/2025/09/05/sqlite-on-macos-not-acid/) |
| **LMDB 0.9** | meta page written through an `O_DSYNC` fd; `BROKEN_FDATASYNC` unless the kernel is ≥ 3.6 | `MDB_FDATASYNC(fd) = fcntl(fd, F_FULLFSYNC)` | `FlushFileBuffers` | [S] [mdb.c](https://github.com/LMDB/lmdb/blob/mdb.RE/0.9/libraries/liblmdb/mdb.c) |
| **redb** | `File::sync_data` → `fdatasync` | → `F_FULLFSYNC` (via Rust std) | → `FlushFileBuffers` | [S] [optimized.rs](https://github.com/cberner/redb/blob/master/src/tree_store/page_store/file_backend/optimized.rs) |
| **RocksDB** | `fdatasync` / `fsync` | `F_FULLFSYNC` for both `Sync` and `Fsync` (`HAVE_FULLFSYNC`) | `FlushFileBuffers` | [S] [io_posix.cc](https://github.com/facebook/rocksdb/blob/main/env/io_posix.cc) |
| **Go `os.File.Sync`** | `fsync` | `F_FULLFSYNC`; falls back to `fsync` on `ENOTSUP` (SMB mounts, #64215) | `FlushFileBuffers` | [S] [fd_fsync_darwin.go](https://github.com/golang/go/blob/master/src/internal/poll/fd_fsync_darwin.go) |
| **PostgreSQL** | `wal_sync_method` default `fdatasync` | default is **not** write-through; "On macOS, write caching can be prevented by setting wal_sync_method to fsync_writethrough" | default `open_datasync`; `fdatasync` maps to `NtFlushBuffersFileEx` | [D] [WAL config](https://www.postgresql.org/docs/current/runtime-config-wal.html), [reliability](https://www.postgresql.org/docs/current/wal-reliability.html) |
| **PostgreSQL on fsync error** | PANIC (`data_sync_retry=off` default), since PG 12, back-patched | same | same | [D] [PG error handling](https://www.postgresql.org/docs/current/runtime-config-error-handling.html), [C] [PG wiki Fsync Errors](https://wiki.postgresql.org/wiki/Fsync_Errors) |

**Lessons.**

- **(a)** Every engine that claims power-loss durability on macOS uses `F_FULLFSYNC`, and the popular defaults (system
  SQLite, PostgreSQL, CPython `os.fsync` [C]) do not. moirai must not inherit a default. It calls the primitive itself
  through the `Vfs`.
- **(b)** Go and SQLite silently degrade to `fsync` on `ENOTSUP`. Rust std returns the error. moirai should **refuse the
  store** (exit 7, "location cannot provide durable commits"). A silent downgrade would make `durable` a lie on SMB and
  similar mounts, which the store refuses anyway.

### 3.4 Namespace (directory) durability

| OS | What makes create / rename / unlink durable | Cost | Source |
|---|---|---|---|
| Linux | `fsync(open(dir, O_RDONLY\|O_DIRECTORY))` after the operation. ext4 forces a **full journal commit** for a directory fsync ("Fastcommit does not really support fsync on directories … Force a full commit") | one journal commit + one flush | [D] fsync(2); [S] ext4/fsync.c |
| macOS | `fsync(dirfd)` and then a device barrier: `F_FULLFSYNC` on the directory fd, or on any file of the same volume afterwards, since `F_FULLFSYNC` persists everything fsync'd before on that device | one `F_FULLFSYNC` can cover a whole group | [D] fcntl.2; pattern [C] (fsync temp → rename → fsync dir → one barrier) |
| Windows | `FlushFileBuffers` on a directory handle opened with `FILE_FLAG_BACKUP_SEMANTICS` **and** `GENERIC_WRITE`. A read-only directory handle fails with `ERROR_ACCESS_DENIED`. Rust std `File::open(dir)` fails with error 5 unless the flag is added through `custom_flags` | **0.072 ms p50**, 0.10–0.11 ms p90 (n = 200, right after a file flush) | [M] `dirsync/` |

**Ordering.** The current fault model ([60 §2.5] item 2) lets metadata operations "survive a crash as a prefix of their
issue order on the volume".

- That matches a single global journal (NTFS; ext4 without fast-commit; XFS, whose fsync forces the log up to the
  inode's LSN) [I].
- btrfs logs only fsynced inodes in its log tree between transaction commits. ext4 fast-commit commits per-inode changes.
  In both cases an unsynced operation issued earlier in another directory can be lost while a later fsynced one survives [I].
- CrashMonkey found, among others, "rename() not being atomic and files disappearing after fsync()" in btrfs [D]
  ([OSDI'18](https://www.usenix.org/conference/osdi18/presentation/mohan)).
- **The portable rule** is therefore to issue an explicit `sync_dir` whenever a later durable record depends on a name
  existing or not existing.

### 3.5 Flush-failure semantics

| OS / FS | Page state after a failed write-back | Error reporting | Source |
|---|---|---|---|
| Linux ext4 (ordered) | marked **clean**, holds the **new** bytes (disk differs) | immediate; later fsyncs do not retry | [D] ATC'20 Table 1 |
| Linux ext4 (`data=journal`) | clean, new bytes | reported on the **next** fsync (delayed) | [D] ATC'20 |
| Linux XFS | clean, new bytes; metadata faults shut the file system down | immediate | [D] ATC'20 |
| Linux btrfs | clean, **reverted** to the old content; the next append can leave a hole | immediate | [D] ATC'20 |
| Linux ≥ 4.13 / 4.16 VFS | errseq: reported to every open file description that could have written the data; lost if the inode is evicted | once per description | [D] fsync(2); [C] PG wiki |
| macOS | "buffers are invalidated" (`vfs_bio.c`); later calls may return success despite loss | — | [C] PG wiki |
| FreeBSD (for contrast) | re-dirtied and retried | — | [D] ATC'20 lesson #5 |
| Windows | unknown | unknown | [C] PG wiki |

ATC'20 ([Rebello et al.](https://www.usenix.org/conference/atc20/presentation/rebello)) lesson #8 [D]: "Applications
should read the on-disk content of files when performing recovery". On ext4 and XFS the page cache can serve bytes that
never reached the disk.

**Consequences for the M0 protocol decisions [60 §2.5]:**

- **"Flush failure aborts the process"** stays, on all three OSes. It must also cover `ENOSPC`, `EDQUOT` and `EROFS`
  (ext4 `errors=remount-ro`, XFS shutdown).
- **Decision (a).** Adopting a record after a failed flush must **re-write its bytes, then flush**. That is correct on
  every OS:
  - ext4/XFS: the adopter reads the new bytes from the clean page and re-dirties them, the "re-dirty by writing back"
    remedy of lesson #7;
  - btrfs: the adopter reads the old bytes, so the checksum fails and the log ends there, correctly, because the failed
    writer never acknowledged;
  - macOS: the adopter reads what is on disk.

  The alternative "verify through an unbuffered read" is not portable:
  - Linux `O_DIRECT` needs block-aligned buffers and offsets;
  - macOS `F_NOCACHE` only "turns data caching off" for that fd [D] and gives no documented guarantee that resident
    pages are bypassed [I].

  **Choose re-write everywhere.**
- **Fault-model item 3** ("a failed flush leaves the content of its range indeterminate forever — later reads may return
  the new bytes") should say "**new or old bytes, possibly changing between reads**", so the simulator also exercises the
  btrfs and macOS behaviours.

### 3.6 Preallocation, file growth and G11 across file systems

G11's purpose is that every commit is a pure data overwrite, so the per-commit data-sync flushes no allocation metadata.

| File system | Extent creation that gives cheap overwrite-in-place | What `fallocate` / `F_PREALLOCATE` gives | Notes |
|---|---|---|---|
| NTFS | write zeros + `FlushFileBuffers` (current G11) | `SetEndOfFile` still advances the valid data length (VDL) per write [20 §2] | as designed |
| ext4 | `fallocate(FALLOC_FL_WRITE_ZEROES)` (Linux ≥ 6.17), else write zeros; then `fsync` + `sync_dir` | plain `fallocate` / `ZERO_RANGE` create **unwritten** extents; the first write converts them, with journal I/O (5× slower O_DSYNC overwrite) | [C] [LWN 1018299](https://lwn.net/Articles/1018299/), [kernelnewbies 6.17](https://kernelnewbies.org/Linux_6.17) |
| XFS | `FALLOC_FL_WRITE_ZEROES` (Linux ≥ 7.3, per Phoronix), else write zeros | unwritten extents, as ext4 | [C] [Phoronix](https://www.phoronix.com/news/XFS-FALLOC-FL-WRITE-ZEROES) |
| btrfs | CoW: every overwrite allocates. Setting `FS_NOCOW_FL` (`chattr +C`) on the **empty** file allows in-place overwrite but disables btrfs data checksums (moirai has its own) | preallocated extents are overwritten in place only once | [I]; the `sync_file_range(2)` man page notes CoW makes "an overwrite of existing allocated blocks … impossible" [D] |
| ZFS | always CoW; with compression on, all-zero blocks become holes, so zero-fill is a no-op in space and pure waste in I/O | — | [I] |
| APFS | Apple: "Apple File System uses copy-on-write to avoid in-place changes to file data" [D] ([APFS FAQ](https://developer.apple.com/library/archive/documentation/FileManagement/Conceptual/APFS_Guide/FAQ/FAQ.html)); local Time Machine snapshots also pin blocks [I] | `F_PREALLOCATE` (`F_ALLOCATECONTIG`, `F_ALLOCATEALL`, `F_PEOFPOSMODE`) reserves space without changing the size; follow with `ftruncate` [D] fcntl.2 | zero-fill gains nothing; use sparse `ftruncate` |
| tmpfs | n/a (memory) | — | no durability at all |

**One format, a per-file-system creation method.**

- The format invariant is: a log extent is exactly `extent_size` bytes long, and every byte beyond the durable tail
  **reads as zero**. Recovery scans to the first record whose length, checksum or epoch does not match.
- Explicit zeros, `WRITE_ZEROES`, unwritten extents, sparse holes and punched holes (`FALLOC_FL_PUNCH_HOLE`, macOS
  `F_PUNCHHOLE` for G25 recycling) all read as zeros. The on-disk result is therefore the same format on every OS.
- The `Vfs` chooses the creation method at `init` and at each extent rotation, by file-system type. The fault model keeps
  "a lost unflushed sector reverts to its previous content (zeros in a fresh extent)".
- ext4 in `data=writeback` mode can expose **stale blocks of other files** after a crash [I]. The per-record xxh3 and the
  epoch make that harmless: garbage never validates.

**Disk-full on CoW file systems.** On APFS, btrfs and ZFS **any overwrite, and any flush, can fail with `ENOSPC`**, even
inside "preallocated" space [I]. Fault-model item 5 ("any allocating or extending write … may fail") must widen to "any
write or flush". A cheap early warning: on sparse extent creation, `statvfs` must show at least 2 × `extent_size` free,
otherwise refuse the rotation (exit 7) [I].

### 3.7 File-system and environment hazards, and detection at open

| Hazard | Effect | Detection (at open / `doctor`) | Action |
|---|---|---|---|
| **tmpfs** (`TMPFS_MAGIC`) | "lives completely in the page cache and optionally on swap"; lost on unmount [D] ([tmpfs](https://docs.kernel.org/filesystems/tmpfs.html)) | `statfs.f_type` | refuse unless `--ephemeral` (tests only); never run RAM gates on tmpfs (§5.2) |
| **overlayfs `volatile`** | "Volatile mounts are not guaranteed to survive a crash"; sync calls are skipped [D] ([overlayfs](https://docs.kernel.org/filesystems/overlayfs.html)) | `/proc/self/mountinfo` super options | refuse `durable` (exit 7) |
| overlayfs, lower-layer files mapped `MAP_SHARED` | "subsequent changes to the file are not reflected in the memory mapping" [D] | — | harmless: moirai maps only its own sealed files, created in the upper layer |
| **NFS, SMB/CIFS, FUSE** (sshfs, virtiofs), **9p** (WSL2 `/mnt/c`, `/mnt/d`) | weak or absent flush, lock and mmap semantics | `f_type` ∈ {`NFS_SUPER_MAGIC`, `SMB2_MAGIC_NUMBER`, `CIFS_MAGIC_NUMBER`, `FUSE_SUPER_MAGIC`, `V9FS_MAGIC`} | refuse (the analogue of the OneDrive refusal); an allow-list of local types (ext4, XFS, btrfs, ZFS, f2fs, bcachefs) is safer than a deny-list [I] |
| **macOS non-local volumes** (SMB, AFP, NFS, WebDAV) | `F_FULLFSYNC` → `ENOTSUP` | `statfs.f_flags & MNT_LOCAL` [S]; `ENOTSUP` from `F_FULLFSYNC` | refuse |
| **macOS iCloud Drive / File Provider roots** (e.g. `~/Library/Mobile Documents`), Dropbox | sync clients rewrite, evict or dataless files: the OneDrive analogue | path prefix + File Provider attributes (identity lens) | refuse, as OneDrive |
| ext4 `barrier=0` / `nobarrier`; ZFS `sync=disabled` | flushes silently not sent | `mountinfo` (ext4); ZFS not reliably detectable without `zfs get` | `doctor` warns; document |
| Linux `queue/write_cache` set to "write through" on a device with a volatile cache | "might not be safe … since that will also eliminate cache flushes issued by the kernel" [D] ([sysfs-block ABI](https://github.com/torvalds/linux/blob/master/Documentation/ABI/stable/sysfs-block)) | read `/sys/block/<dev>/queue/write_cache`, `/queue/fua` | `doctor` reports them; no auto-detection of lying drives |
| External USB/FireWire drives that ignore flush | "Certain FireWire drives have also been known to ignore the request" [D] | not detectable | document; the design assumes the drive honours FLUSH (as RG3) |
| Windows "turn off write-cache buffer flushing" | flush not sent | registry `CacheIsPowerProtected` (none set here [M]) | `doctor` warns |

### 3.8 Recommendation: one durability abstraction

#### 3.8.1 Named classes (the semantics are OS-independent)

| Class | Guarantee (identical on all OSes) | Windows | Linux | macOS |
|---|---|---|---|---|
| **`lazy`** (heartbeats, cursors, session marks: unchanged membership) | visible to every process after publish; survives a **process** crash; may be lost after an OS crash or power loss; becomes durable at the next `durable` barrier on the same file | `WriteFile` | `pwrite` | `pwrite` |
| **`durable`** (unchanged membership, [AR §6.5]) | acknowledged only after the record's bytes are on stable media, assuming the drive honours FLUSH | `NtFlushBuffersFileEx(FLUSH_FLAGS_FILE_DATA_SYNC_ONLY)` | `fdatasync` (the `O_DIRECT`+`RWF_DSYNC` FUA path only if M0 shows it wins) | `fcntl(F_FULLFSYNC)`; `ENOTSUP` → refuse the store |
| **`durable+meta`** (extent creation, size changes, `HEAD` barrier) | as `durable`, plus the file's size and allocation | `FlushFileBuffers` | `fsync` | `F_FULLFSYNC` |
| **`durable-name`** (create, rename, unlink the protocol depends on) | the directory entry change survives power loss | `FlushFileBuffers(dir handle, BACKUP_SEMANTICS + GENERIC_WRITE)` [M] | `fsync(dirfd)` | `fsync(dirfd)` + `F_FULLFSYNC` barrier (on `dirfd`, or folded into the group barrier) |
| **group barrier** `sync_group(files…, dirs…)` | everything listed is durable when it returns | flush each | `fdatasync`/`fsync` each (every one is a device flush) | `fsync` each + **one** `F_FULLFSYNC` last (the man page's drain-and-barrier guarantee) |

- **No `ordered` class.** `F_BARRIERFSYNC` would give macOS a cheap "prefix-safe, may lose the tail" class. Linux and
  Windows have no primitive that is both cheap and ordering-only, so such a class would have to be `durable` there, and
  the protocol does not need it.
- **One error policy.** Any error from any class other than `lazy` (`EIO`, `ENOSPC`, `EDQUOT`, `EROFS`, `ENOTSUP`,
  Windows errors) aborts the process without acknowledging. The next writer recovers by **re-write + flush**.
- **Environment guard at open (§3.7).** The store refuses locations where `durable` cannot be provided. `durable` is
  never silently weakened.

#### 3.8.2 `Vfs` trait surface in this lens (a sketch for the M0 specification)

```text
write_at(file, off, bytes)            read_at(file, off, len)
sync(file, Data | DataAndMeta)        sync_dir(dir)
sync_group(&[(file|dir, kind)])       create_extent(path, size)   // per-FS method, §3.6
seal(file)                            // durable+meta, then set 0444 / FILE_ATTRIBUTE_READONLY (§4.6)
map_sealed(file, expected_len)        // §4.6
unlink(path)                          // Windows: clear the read-only attribute, tolerate delete-pending
statfs_class(path) -> Local{fs} | Refused{reason}
```

#### 3.8.3 Fault-model amendments (to [60 §2.5], frozen at M0; the weakest model of the three OSes)

1. Item 2 becomes three rules:
   - `sync(Data)` makes durable the file's data within its current size;
   - `sync(DataAndMeta)` also makes durable its size and allocation;
   - **a create, rename or unlink is durable only after `sync_dir` of its parent. Before that, any subset of the
     unsynced metadata operations may be lost** (the NTFS "prefix" rule is dropped).
2. Item 3: after a failed flush, reads of the range may return old **or** new bytes and may change between reads.
3. Item 5: **any** write (including an overwrite of a written or zero-filled range) and any flush may fail with disk-full.
4. Item 8: lock release after death is immediate on Unix and delayed on Windows. Keep the delay parameter; it is a superset.
5. New item 9: a read through a mapping of a sealed file may terminate the process (`SIGBUS` / `EXCEPTION_IN_PAGE_ERROR`)
   when the file was truncated externally or the medium fails. The simulator treats this as a process crash at that point.
6. New item 10 (Unix): other processes may truncate or rewrite any store file they have permission for (there are no share
   modes). Sealed files are `0444`, so this requires deliberate action. The simulator injects an external truncation
   of a sealed file.

#### 3.8.4 Protocol touch-points

- **Segment and blob creation.** Write the file → `sync(DataAndMeta)` → `sync_dir` → only then append and flush the
  `Checkpoint` that names it.
  - On macOS these fold into one `sync_group`: two plain `fsync`s and one `F_FULLFSYNC`.
  - If the temp-name-then-rename step is kept, the rename must be followed by `sync_dir` before the `Checkpoint`.
  - Simpler alternative [I]: create the file under its **final** monotonic number. A file that no durable
    `Checkpoint` names is an orphan by definition, so no rename is needed.
- **GC and retirement.** Durable `HEAD` barrier (`durable+meta`) → `unlink` → `sync_dir`, all off the commit path.
  This is unchanged in spirit and gains the directory sync.
- **Log rotation.** `create_extent` → `sync_dir` before the first commit in the new extent is acknowledged.

### 3.9 The 16-writer burst on macOS: group commit becomes a protocol question

As written, each writer takes `LOCK` byte 0, appends, flushes, publishes and releases [AR §4.5]. The last acknowledgement
of a 16-writer burst therefore costs about 16 × the flush time:

| Platform | Flush p50 | Serial 16-writer last ack | Gate "writer-wait p99 ≤ 50 ms" |
|---|---|---|---|
| owner's Windows NVMe | 0.45–1.9 ms [M] | 7–32 ms | passes |
| Linux consumer NVMe | 0.45–2.8 ms [C] | 7–45 ms | marginal on no-PLP drives |
| macOS M3/M5 | 3.1–3.9 ms, p99 11.6 ms [C] | ~50–65 ms p50-based; p99 far above | **fails** |
| macOS M1 | ~17–20 ms [C] | ~290–320 ms | **fails** |

T8's revisit trigger ("sustained flush p99 > 10 ms → pipelined group commit") is met by the published M3 p99 (11.6 ms)
[C]. Because the protocol must be identical on all OSes and is frozen at M0, the choice cannot wait for a Windows
measurement. Two semantics-preserving options belong to the concurrency lens [I]:

1. **Leader group commit.** This is the optional leader of [AR §2.2], made mandatory: one flush per batch, each client
   acknowledged after the flush that covers its record.
2. **Leaderless flush outside the writer lock**, as in PostgreSQL's `XLogFlush`:
   - append under byte 0 and release;
   - flush;
   - acknowledge when a flush that **started after** your append has returned (your own, or one recorded by another
     process in a shared `flushed_lsn`);
   - publish `HEAD` only up to the flushed LSN, so that readers never see non-durable state.

   `fdatasync` and `F_FULLFSYNC` flush the whole file, and the latter the whole device queue, so a later flush covers
   earlier appends on all three OSes [D]. The recovery adopter already handles an appended-but-unflushed record, by
   re-write + flush.

Whichever is chosen, gates must be **floor-relative per OS**. A writer-wait p99 of at most about `16 × flush_p50 × 1.25`
without group commit, or at most about `2–3 × flush_p99` with it, is a better gate than an absolute 50 ms that no Mac can
meet [I].

---

## 4. Memory mapping

### 4.1 Coherence with read/write

| OS | Mapped views vs `read`/`write` of the same file | Source |
|---|---|---|
| Linux | one page cache per inode (`address_space`) serves both `filemap_fault` and `read`/`write`, so they are coherent | [S] mm/filemap.c; mmap(2) `MAP_SHARED` "visible to other processes" [D] |
| macOS | Unified Buffer Cache: "MAP_SHARED mapped areas must be coherent with any other read/write operations" | [D] [Apple Kernel Programming Guide, BSD overview](https://developer.apple.com/library/archive/documentation/Darwin/Conceptual/KernelProgramming/BSD/BSD.html) (UBC) |
| Windows | views vs `ReadFile`/`WriteFile` "not necessarily coherent" | [D] [05 §6.2] |
| All | `MAP_PRIVATE`: "It is unspecified whether changes made to the file after the mmap() call are visible" | [D] mmap(2) |

moirai maps only sealed files and reads the mutable log and `HEAD` with `pread`. Coherence is therefore **irrelevant on
all three OSes**; the rule written for Windows is also the portable rule.

### 4.2 Faults on mapped reads

| OS | Trigger | Signal / exception | Source |
|---|---|---|---|
| Linux | page index ≥ `i_size` (file truncated or shorter than the mapping) | `VM_FAULT_SIGBUS` | [S] mm/filemap.c `filemap_fault` |
| Linux | read error while paging in ("Try to re-read it _once_") | `VM_FAULT_SIGBUS` | [S] same |
| Linux | write into a `PROT_READ` mapping | `SIGSEGV` | [D] mmap(2) |
| macOS | `EXC_BAD_ACCESS` with any code other than `KERN_INVALID_ADDRESS` (pager error, beyond EOF) | `SIGBUS` | [S] [XNU ux_exception.c](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/uxkern/ux_exception.c) |
| Windows | I/O error, lost network, disk full on sparse or compressed files | `EXCEPTION_IN_PAGE_ERROR`; Microsoft advises SEH around every access | [D] [Reading and Writing From a File View](https://learn.microsoft.com/en-us/windows/win32/memory/reading-and-writing-from-a-file-view) |
| Windows | truncation of a mapped file | impossible (`ERROR_USER_MAPPED_FILE`) | [D] [05 §6.1] |

**Truncation is the Unix-specific hazard.** Nothing in the OS stops another process that has write permission from
truncating a mapped segment.

- **Precedents:**
  - ripgrep documents that it "may abort unexpectedly when using memory maps" if a file is truncated [C]
    ([ripgrep #581](https://github.com/BurntSushi/ripgrep/issues/581));
  - MaxMind GeoIP readers crash when the database is updated with `cp`, which truncates in place [C]
    ([zuff.dev 2023](https://zuff.dev/posts/sigbus/));
  - uutils `tac` dropped mmap for regular files for the same reason [C]
    ([PR #11326](https://github.com/uutils/coreutils/pull/11326)).
- **Kernel escape hatches:**
  - `MAP_NOSIGBUS` (Ming Lin, 2021) would have mapped zero pages instead [C] ([LWN 860419](https://lwn.net/Articles/860419/)).
    It is not in the current mmap(2) flag list [I].
  - Linux ≥ 5.14 `MADV_POPULATE_READ` returns `EFAULT` "because a SIGBUS would have been generated" [D]
    ([madvise(2)](https://man7.org/linux/man-pages/man2/madvise.2.html)). It narrows but does not close the window
    (time-of-check vs time-of-use), and it is Linux-only.

**Rust.** No `sigsetjmp`/`siglongjmp` recovery is sound. Go has `debug.SetPanicOnFault`; Rust has no equivalent. What
remains is to prevent, detect and die cleanly.

### 4.3 The soundness debate

- redb 0.14 (2023-03-26): "The mmap backend has been removed because it was infeasible to prove that it was sound" [S]
  ([CHANGELOG](https://github.com/cberner/redb/blob/master/CHANGELOG.md)).
- memmap2: "All file-backed memory map constructors are marked `unsafe` because of the potential for Undefined Behavior
  (UB) using the map if the underlying file is subsequently modified, in or out of process". The suggested precautions
  are "file permissions, locks or process-private (e.g. unlinked) files", which are "platform specific and limited" [D]
  ([docs.rs memmap2](https://docs.rs/memmap2/latest/memmap2/struct.Mmap.html)).
- moirai's position [I]:
  - redb had to reason about mapped pages that its **own** writers change.
  - moirai's mapped files are immutable for their whole life, by protocol.
  - The residual risk is an external writer acting against permissions (§4.6 rules 2 and 3).
  - If every read of mapped bytes goes through types for which **every bit pattern is valid** (`zerocopy::FromBytes`:
    integers and byte arrays, never `bool`, enums or `char`), and every offset derived from mapped bytes is
    bounds-checked, then a changed byte yields a wrong answer or a checksum failure, never an out-of-bounds access.
  - The formal "`&[u8]` must not change" UB remains, but only for that external-writer case.
  - Record this argument in the `Vfs` safety comment. It is the same argument LMDB relies on implicitly.

### 4.4 Growth, remapping, page size and advice

| Topic | Linux | macOS | Windows | Policy |
|---|---|---|---|---|
| Grow a mapping | `mremap` (memmap2 `remap` is Linux-only [D]) | none | none (undocumented section APIs) [05 §6.1] | never grow; map whole sealed files from offset 0 |
| Page size | 4 KiB on x86-64; 4/16/64 KiB on arm64 | **16 KiB** on Apple silicon (`PAGE_MAX_SHIFT 14`) [S] ([XNU vm_param.h](https://github.com/apple-oss-distributions/xnu/blob/main/osfmk/mach/arm/vm_param.h)) | 4 KiB pages, 64 KiB view-offset granularity | no page size in the format; whole-file maps avoid offset alignment |
| Read-around on fault | 128 KiB readahead (`VM_READAHEAD_PAGES`), disabled by `MADV_RANDOM` (`VM_RAND_READ`) [S] | `madvise` / `F_RDADVISE` [D] | cache-manager heuristics; `PrefetchVirtualMemory` | `MADV_RANDOM` on index sections (NODE, CSR, bitsets); `MADV_WILLNEED` / `MADV_SEQUENTIAL` right before a tier-1 FTS scan. These are performance hints only, and may differ per OS |
| Fault-around | maps up to 64 KiB of already-cached neighbours per fault (`fault_around_pages = 65536 >> PAGE_SHIFT`) [S] | — | — | raises `RssFile`, never private memory (§5) |
| Beyond EOF inside the last page | "remaining bytes in the partial page … are zeroed" [D] | "zero-filled" [D] XNU mmap.2 | — | never rely on it; size-check first |

### 4.5 Page-cache sharing across processes

- All three OSes back a read-only file mapping with the one cached copy of the file's pages:
  - Linux page cache per inode [S];
  - macOS UBC [D];
  - Windows section object, with soft faults served from "the working set of some other process" [D] [05 §5.3].
- The per-process cost is page-table entries. With 16 KiB pages, macOS needs a quarter of the entries per mapped byte.
- The [AR §8.1] claim that the RAM target holds "because data is shared through read-only mappings" is therefore valid
  on all three OSes, **provided the store is on a real file system**. On tmpfs the pages are shmem: they are counted as
  `RssShmem` and are never reclaimable by write-back (§5.2).

### 4.6 Recommendation: one mmap policy for all three OSes

1. **Map only sealed files.** A file is sealed when it is completely written, `sync(DataAndMeta)` + `sync_dir` have
   returned, and a durable `Checkpoint` names it. The log extents and `HEAD` are never mapped. Mappings are read-only and
   whole-file: `PROT_READ` + `MAP_SHARED` on Unix; `PAGE_READONLY` + `FILE_MAP_READ` on Windows.
2. **Seal = read-only on disk.** After sealing, `fchmod(0444)` on Unix or `FILE_ATTRIBUTE_READONLY` on Windows.
   - A non-root `cp`, `truncate`, editor save or `O_TRUNC` open then fails with `EACCES` instead of crashing readers [I].
   - GC must clear the attribute before `DeleteFileW` on Windows. Unix `unlink` needs only directory permission.
   - Do not use macOS `UF_IMMUTABLE` or Linux `chattr +i`: the first blocks moirai's own GC; the second needs
     `CAP_LINUX_IMMUTABLE`.
3. **Never truncate, extend, write, rename over or reuse the name of a sealed file.** File numbers are monotonic. This
   is already a rule [AR §4.1]; it now carries the Unix safety argument as well.
4. **Size check before mapping.** `fstat(fd).st_size` must equal the length recorded in the durable `Checkpoint`.
   Otherwise the process refuses the file ("re-read `HEAD`, retry once, then exit 7 naming the file and suggesting
   `doctor --fsck`"). This is the Unix counterpart of the Windows delete-pending retry.
5. **Typed, bounds-checked access.**
   - Mapped bytes are read only through `FromBytes` types.
   - Every length or offset read from a mapping is checked against the mapping's length before use. `get()` is used,
     never `get_unchecked`.
   - Nothing is transmuted to types with invalid bit patterns.
6. **Crash-class handler.**
   - Every mapping is registered in a process-global table of address ranges (a lock-free, signal-safe fixed array).
   - Unix: a `SIGBUS` handler (`SA_SIGINFO`). Windows: a vectored exception handler for `EXCEPTION_IN_PAGE_ERROR`.
   - If the fault address lies in a registered mapping, the handler writes one line (file number, offset, errno) to
     stderr with async-signal-safe `write(2)` or `WriteFile`, then calls `_exit(10)` ("store I/O fault"). Otherwise it
     re-raises the default action.
   - The protocol already treats any process death as a crash: readers hold no locks, and a writer dying mid-commit is
     recovered by the next writer.
   - The long-lived MCP server gets the same behaviour; see open question 8.
7. **No `MAP_POPULATE`, no `mlock`, no `mremap`.** `MADV_POPULATE_READ` may be used on Linux as an optional pre-check on
   first map of a small section. It changes no semantics.
8. **Tests.** The Unix kill loop adds an "external truncation of a random sealed file" variant. The expected result: the
   affected readers exit with code 10, no acknowledged commit is lost, and `doctor --fsck` names the file.
   `repair --rebuild-from-log` restores it, because sealed files are derived.

---

## 5. Memory accounting

### 5.1 The metrics per OS

| Question | Windows | Linux | macOS |
|---|---|---|---|
| Private memory now | `PrivateUsage` / `PagefileUsage` = commit charge ("total amount of private memory that the memory manager has committed") [D] | `RssAnon` (+ `VmSwap`, + `VmPTE` page tables) in `/proc/self/status`; exact: `Anonymous`, `Swap`, `Pss_Anon` in `/proc/self/smaps_rollup` [D] ([proc docs](https://docs.kernel.org/filesystems/proc.html)) | `task_vm_info.phys_footprint` [S] |
| Private **peak** | `PeakPagefileUsage` [D] | **none per process.** cgroup-v2 `memory.peak` (per cgroup; resettable per fd) [D] ([cgroup-v2](https://docs.kernel.org/admin-guide/cgroup-v2.html)) | `task_vm_info.ledger_phys_footprint_peak` (rev 3) [S]; `proc_pid_rusage(RUSAGE_INFO_V4+).ri_lifetime_max_phys_footprint` [S] ([XNU resource.h](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/sys/resource.h)) |
| Shared file-backed | shareable working set | `RssFile`, `Pss_File` | `external` pages; clean file pages are excluded from the footprint |
| Definition in source | — | `VmRSS = RssAnon + RssFile + RssShmem`; RSS fields "handled in an asynchronous manner and the value may not be very precise" [D] | `phys_footprint = (internal − alternate_accounting) + (internal_compressed − alternate_accounting_compressed) + iokit_mapped + purgeable_nonvolatile + purgeable_nonvolatile_compressed + page_table` [S] ([XNU task.c](https://github.com/apple-oss-distributions/xnu/blob/main/osfmk/kern/task.c)) |
| Tooling | `GetProcessMemoryInfo`, VMMap | `smaps_rollup`, cgroup `memory.stat` (`anon`, `file`, `kernel`, `pagetables`) | `footprint(1)`: "A process's 'footprint' is equal to the total of all Dirty memory" [D] ([footprint(1)](https://keith.github.io/xcode-man-pages/footprint.1.html)); `vmmap --summary` |

### 5.2 Pitfalls that make naive cross-OS gates wrong

- **`getrusage().ru_maxrss` includes file-backed pages and uses different units:** kilobytes on Linux, bytes on macOS
  (mimalloc: "macos reports in bytes"; "Linux/BSD report in KiB") [S]
  ([mimalloc prim.c](https://github.com/microsoft/mimalloc/blob/main/src/prim/unix/prim.c)). It must not be the gate.
- **Linux `/proc/<pid>/status` RSS values are approximate** [D]. Use `smaps_rollup`, which walks the page tables
  (exact but costly), and read it once at exit.
- **Commit vs resident.** Windows `PeakPagefileUsage` counts committed but untouched pages. macOS and Linux count only
  touched pages. A Windows figure is therefore an upper bound on the same code's Unix figure, except for the loader and
  libc baselines below.
- **`MADV_FREE`d pages stay in Linux RSS until memory pressure**; `smaps` shows them as `LazyFree` [D]. Allocators that
  use `MADV_FREE` (glibc does not by default; mimalloc is configurable) inflate `RssAnon` without real cost. Subtract
  `LazyFree` or use `MADV_DONTNEED` in the gated build [I].
- **tmpfs stores:** mapped segments count as `RssShmem`, and in cgroups they are charged as shmem to whoever faulted them.
  CI runners often have `/tmp` on tmpfs. **Never run RAM gates with the store on tmpfs** [I].
- **cgroup charging:** "A memory area is charged to the cgroup which instantiated it" [D]. For `memory.peak` to measure
  only private memory, the harness must create and pre-read the store files **outside** the measured leaf cgroup.
  `memory.peak` also includes kernel memory (page tables, dentries for the files opened); this is small for moirai.
- **Fault-around** (Linux, 64 KiB) and 16 KiB pages (macOS) raise the **shared** RSS per touched row, not private memory.

### 5.3 Baselines differ per OS

| OS | Minimal process private memory | Source |
|---|---|---|
| Windows 11 (owner) | 0.69 MB private (`more.com`) | [M, 05 §2.4] |
| macOS 15.5 (Intel) | `zsh` footprint 2,072 KB: `__DATA` 562 KB, `MALLOC_NANO` 360 KB, dyld private 304 KB; `phys_footprint_peak` 2,172 KB | [C] [unixtutorial](https://www.unixtutorial.org/macos-footprint-command/) |
| macOS, minimal Rust binary | est. ~1.0–1.5 MB: dyld and libSystem dirty data plus malloc zones | [I]; M0 measure |
| Linux, minimal Rust binary | est. 0.2–0.5 MB `RssAnon` (glibc dynamic: relocated `.data`/GOT, TLS, stack; static musl lower) | [I]; M0 measure |

- **Consequence.** The absolute "≤ 4 MB private per CLI" gate [AR §8.1] leaves about 3.3 MB of headroom on Windows and
  perhaps only 2.5 MB on macOS. The moirai-attributable memory is the same everywhere.
- **Extra macOS rule.** Link only `libSystem`: no CoreFoundation, Security or IOKit.
  - rand 0.6 on macOS pulled in Security.framework and CoreFoundation initialisation. That took startup from
    1.6 ms to 7.6 ms and pre-main from 1.19 ms to 9.33 ms [C] ([rand #733](https://github.com/rust-random/rand/issues/733)).
  - It also adds dirty pages to the footprint [I].
  - Gate it in CI with `otool -L`. On Linux, gate the `ldd` output: only libc, libm and libgcc_s, or static.

### 5.4 Allocators

| Allocator | RAM behaviour relevant to moirai | Source |
|---|---|---|
| glibc malloc | arena limit `8 × cores` on 64-bit (`M_ARENA_TEST` 8); `M_MMAP_THRESHOLD` starts at 128 KiB but "when blocks larger than the current threshold … are freed, the threshold is adjusted upward", up to 32 MiB on 64-bit; `M_TRIM_THRESHOLD` 128 KiB | [D] [mallopt(3)](https://man7.org/linux/man-pages/man3/mallopt.3.html) |
| musl (mallocng) | small and static-friendly; ripgrep: musl's allocator "appears to be substantially worse", so ripgrep uses jemalloc on musl | [S] [ripgrep main.rs](https://github.com/BurntSushi/ripgrep/blob/master/crates/core/main.rs) |
| macOS libmalloc | nano zone (`MALLOC_NANO`, ~360 KB dirty in `zsh` [C]) + magazine zones; the reusable (`MADV_FREE_REUSABLE`) pages leave the footprint | [C][I] |
| Windows heap | `HeapAlloc` (Rust default) | [05 §15] |
| mimalloc | `purge_delay` 10 ms; `arena_eager_commit=2` "just on overcommit systems"; `arena_reserve` 1 GiB (virtual); `mi_process_info` reports `peak_commit` from its own statistics on every OS | [S] [mimalloc.h](https://github.com/microsoft/mimalloc/blob/main/include/mimalloc.h), [stats.c](https://github.com/microsoft/mimalloc/blob/main/src/stats.c) |

**Relevance.**

- **CLI.** It is single-threaded with a per-request bump arena [AR §4.7, 05 §15], so the allocator hardly matters for the
  CLI gate [I].
- **MCP server** (long-lived, `current_thread`):
  - With glibc, the dynamic mmap threshold can move freed arena chunks into the `brk` heap, where they are trimmed only
    from the top. A server that once built a large overlay can therefore keep that RSS.
  - Two fixes: pin `M_MMAP_THRESHOLD` / `M_TRIM_THRESHOLD` with `mallopt`, which disables the dynamic adjustment [D]; or
    use one allocator on all OSes.
- **Recommendation:** measure the system allocator against mimalloc at M0 on all three OSes, on the MCP server's
  `private_peak` after a 16-lane fan-out and after returning to idle, and choose **one allocator for all three** if it
  wins [I]. The C dependency is an owner call (open question 5).

### 5.5 Recommendation: the per-OS RSS gate

Two gated quantities, reported by every measured run on every OS:

1. **`heap_peak`**, portable and deterministic. The `measure` build wraps the global allocator with a counting layer
   (live bytes and a high-water mark, no extra syscalls) and prints the peak at exit.
   - The same code gives nearly the same number on all three OSes.
   - This is the gate that catches moirai regressions without OS noise: CLI ≤ 2.5 MB at 1e5; MCP ≤ 6 MB + 1 MB ×
     min(active branches, 8) (derived from the [AR §8.1] rows) [I].
2. **`private_peak`**, the OS truth:

   | OS | Definition | How it is read |
   |---|---|---|
   | Windows | `PROCESS_MEMORY_COUNTERS_EX.PeakPagefileUsage` | at exit (unchanged) |
   | macOS | `task_vm_info.ledger_phys_footprint_peak` from `task_info(mach_task_self(), TASK_VM_INFO)` | at exit, in process; the harness can cross-check with `proc_pid_rusage(RUSAGE_INFO_V6).ri_lifetime_max_phys_footprint` before reaping |
   | Linux | cgroup-v2 `memory.peak` of a per-run leaf cgroup: the store is created and pre-read by the harness outside the leaf; the store is on ext4 or XFS, never tmpfs. Cross-checked with `smaps_rollup` (`Anonymous + Swap − LazyFree`) + `VmPTE` at exit. Without delegated cgroups (a desktop without systemd delegation), fall back to the at-exit value, flagged "at-exit, not peak" | harness + at-exit read |

   - **The gate is floor-relative,** consistent with the flush-floor gates:
     `private_peak(moirai command) − private_peak(empty Rust binary, same toolchain, same allocator, same OS, same run) ≤ 3.3 MB`
     for the CLI at 1e5. That is 4 MB minus the Windows 0.69 MB floor.
   - The MCP gate follows the same rule, as the [AR §8.1] value minus the floor.
   - The absolute per-OS values are reported, not gated.

Shared memory (`Pss_File`, the Windows shareable working set, macOS `resident_size − phys_footprint`) is reported for the
RAM story but never gated.

---

## 6. Process spawn and open costs; does the CLI-vs-MCP balance change?

| Step | Windows (owner) | Linux | macOS |
|---|---|---|---|
| Spawn-to-exit of a small native binary | 15–74 ms p50 depending on load [M, 05 §2.1, 08 §2] | C hello world 0.15–0.24 ms (static/dynamic, a commenter's machine); 0.5 ms on Graviton 3 [C] ([Lemire 2022](https://lemire.me/blog/2022/08/09/hello-world-is-slower-in-c-than-in-c-linux/)); Rust hello world "always ~1ms" [C] (rand #733) | Rust hello world 1.6 ± 0.5 ms (macOS 10.14, 2019) [C] (rand #733); `git` spawn about 1.5 ms on an M1 mac mini [C] ([desktop #13639](https://github.com/desktop/desktop/issues/13639)) |
| First launch of a new or changed binary | Defender scan of new executables (cargo +40–55 %) [C, 05 §2.1] | none by default; enterprise EDR via `fanotify` [I] | XProtect/Gatekeeper scan: build scripts 0.48–3.88 s vs 0.06–0.14 s with the scan off; the service is single-threaded [C] ([Nethercote 2025](https://nnethercote.github.io/2025/09/04/faster-rust-builds-on-mac.html)); first launch of a 333 MB binary 5.3–5.5 s, second 0.02 s [C] (pnpm PR #15354) |
| Agent Bash-tool wrapper | Git-Bash ~109 ms p50 [M] | `bash -c` with the harness's shell snapshot: unknown, est. 2–10 ms [I] | `zsh`/`bash` with snapshot: unknown, est. 5–20 ms [I] |
| Rust std `Command` | `CreateProcessW` | `posix_spawn` (vfork-style on glibc ≥ 2.24) [C] ([rust #87764](https://github.com/rust-lang/rust/issues/87764)) | `posix_spawn` |
| Open + map 8 sealed files | 8 × 0.22 ms ≈ 1.8 ms [M] | est. < 0.1 ms total [I] | est. < 0.2 ms total [I] |
| Durable commit flush | 0.45–1.9 ms [M] | 0.45–2.8 ms consumer NVMe [C] | **3–20 ms** [C] |

**Does the balance change?**

1. **The CLI becomes cheap on Unix.** A Linux CLI call could cost ~3–15 ms end to end instead of 115–190 ms. The
   engine's ≤ 5 ms and the O(1) open (tail replay ≤ 3 ms at a full tail) then become **the dominant term**. The
   open-path and tail-replay gates matter more on Unix than on Windows [I].
2. **T9 does not change.** CLI + skill stay primary and the ten-tool MCP server serves the Bash-less roles. The reasons
   were never only spawn cost: Bash-less roles, warm branch overlays, and hook contexts that cannot call MCP.
   - The G6 decision ("stamp writes only, because a stamp hook costs +15–73 ms") would be cheap to revisit on Unix
     (+1–3 ms). **Keep one hook policy on all OSes**; Windows remains the binding case.
3. **On macOS the long-lived process matters for durability, not spawn.** With 3–20 ms per flush, a process that
   batches commits (§3.9) saves more than it saves on spawn. That gives the group-commit component a natural home in the
   MCP server or leader.
4. **macOS distribution rules.**
   - Sign and notarize the binary and install it at a stable path, so XProtect scans only after install or upgrade.
   - Never run moirai from a freshly built `target/` directory inside hooks.
   - Link no CoreFoundation or Security (§5.3).
   - The M0 spawn floor (item 11) must be measured **after** a warm-up launch, with the first-launch cost reported
     separately.

---

## 7. Test infrastructure per OS (crash rigs, kill loops)

| Gate | Windows (current plan) | Linux | macOS |
|---|---|---|---|
| **GT15 OS-crash loop** | Win 11 guest in VirtualBox/VMware, host cache off, NotMyFault or power-off | **(1) `dm-log-writes`**: logs every write and replays exactly what had persisted at each flush or FUA ("buffered until a flush request arrives") [D] ([dm-log-writes](https://docs.kernel.org/admin-guide/device-mapper/log-writes.html)). Crash states are enumerated at block level against the real ext4/XFS/btrfs code: seconds per state, runs on hosted Ubuntu runners with `sudo` [I]. (2) `dm-flakey` `drop_writes` power-cut emulation, as xfstests uses it [I]. (3) QEMU/KVM `cache=none` + hard `virsh destroy` for whole-OS panics | Virtualization.framework VM (UTM/Tart) with `VZDiskImageSynchronizationMode.full`, where guest flushes become "their counterpart synchronization commands" on the host [D] ([Apple docs](https://developer.apple.com/documentation/virtualization/vzdiskimagesynchronizationmode)); force stop = power-off. Needs an Apple-silicon Mac (owner hardware); the fidelity calibration (M0 item 17) applies unchanged |
| **Calibration (item 17)** | as planned | with `dm-log-writes`, calibration is by construction: unflushed writes are absent from replay | as planned (an unflushed write must be lost at least once) |
| **GT4 kill loops** | `TerminateProcess`, `NtSuspendProcess`, VHDX disk-full, clock steps | `SIGKILL`, `SIGSTOP`/`SIGCONT`, a loop-mounted small ext4/XFS image for disk-full, clock steps in a VM or a time namespace (monotonic only) [I] | `SIGKILL`, `SIGSTOP`, a small APFS disk image (`hdiutil`) for disk-full |
| **New variant (Unix)** | — | external truncation of a sealed file (§4.6 rule 8) | same |
| **Floors (M0 items 1, 11)** | owner's machine | a Linux host on consumer NVMe (ext4 + XFS; btrfs as best-effort) | an Apple-silicon Mac; M1-class and M3-or-newer if available |

The simulator (`Vfs` fault model) remains the primary gate on every OS. The per-OS rigs verify that the real `Vfs`
implementations match the model (GT15's purpose), and they must run on each OS's `Vfs`.

---

## 8. Consolidated edits proposed for the design documents

| Document / section | Edit |
|---|---|
| [AR §2.8], [AR §6.5] | Replace the Windows call names with the class table of §3.8.1. Add `durable+meta`, `durable-name` and `sync_group`. Add "ENOTSUP/refusal: never weaken". Add the macOS numbers to the "Revisit trigger": the trigger is met on current Macs, so decide group commit at M0 (§3.9) |
| [AR §4.1] rules | Add "sealed files are set read-only". Add "segment and blob files are created under their final number (or temp + rename + `sync_dir`) and `sync_dir` precedes the `Checkpoint` that names them". Widen the store refusal to the Unix list of §3.7 |
| [AR §4.1] `log.NNNN` row | "zero-filled at creation" → "reads as zero beyond the tail; created by the per-file-system method of §3.6, followed by `durable+meta` and `sync_dir`" |
| [AR §4.10] | Replace "Unix builds swap `fdatasync`, `fcntl` byte locks and `mmap`" with a pointer to the `Vfs` class table, the mmap policy (§4.6) and the environment guard |
| [60 §2.5] `Vfs` fault model | Amend items 2, 3, 5 and 8; add items 9 and 10 (§3.8.3) |
| [60 §2.5] protocol decision (a) | "re-write its bytes, then flush"; remove "or verifies them through an unbuffered read" |
| [60 §2.5] protocol decision (c) | Add "… then `unlink`, then `sync_dir`" |
| [60] P1 / exclusions ("Unix `Vfs` and `ProjectFs` excluded") | Superseded by the owner requirement of 2026-09-26: the Unix `Vfs` is in M0/M1, with conformance suites per OS |
| [60 §5.1] RSS row | Replace with §5.5 (`heap_peak` + per-OS `private_peak`, floor-relative) |
| [AR §8.2] M0 measurements | Items 1, 11 and 12 on **three** hosts. Add: `F_FULLFSYNC` vs `fsync` vs `F_BARRIERFSYNC` p50/p99 with 1/4/16 concurrent processes (macOS); `fdatasync` vs `O_DIRECT\|RWF_DSYNC` (FUA) on zero-filled vs `WRITE_ZEROES` vs unwritten extents (Linux ext4/XFS); `sync_dir` cost per OS; empty-binary `private_peak` floor per OS; harness shell-wrapper spawn per OS; first-launch scan cost (macOS) |
| [AR §8.1] budget table | Add per-OS columns for spawn, open+map and durable commit; restate the writer-wait gate as floor-relative |
| [60] GT15 / item 17 | Per-OS rigs (§7) |

---

## 9. Open questions (owner-only calls)

1. **macOS durability cost.**
   - Is Apple silicon a durability-grade target, i.e. `F_FULLFSYNC` on every `durable` commit: ~3–4 ms p50 and ~12 ms p99
     on M3/M5, ~18 ms on M1?
   - If yes, group commit (leader or leaderless, §3.9) becomes a mandatory M1 component on all three OSes.
   - The alternative, a weaker macOS-only class using `F_BARRIERFSYNC`, would break "one protocol semantics" and is
     **not recommended**.
2. **Supported Linux file systems.**
   - Proposed tiers: ext4 and XFS first-class; btrfs and ZFS best-effort (performance not gated); everything in §3.7's
     refusal list refused.
   - Should the store on WSL2 `/mnt/c` and `/mnt/d` (9p) be refused, or allowed behind an explicit flag?
3. **Test hardware.**
   - The macOS crash rig and floors need an Apple-silicon Mac (M3 or newer; an M1 as well if the owner wants the worst
     case covered).
   - Linux floors need a Linux box on consumer NVMe. Hosted runners can run `dm-log-writes` but are not floor-grade.
   - Who provides these, or is macOS power-loss coverage "simulator only" until hardware exists?
4. **RSS gate form.** Floor-relative `private_peak − empty-binary ≤ 3.3 MB` (recommended), or an absolute 4 MB on every
   OS, which leaves macOS about 1–1.5 MB less headroom?
5. **Allocator.** May moirai link mimalloc (C, pinned) as its one allocator on all three OSes, or must it use each
   platform's system allocator (with `mallopt` pinning on glibc)?
6. **Linux binary form.** glibc dynamic (which minimum glibc?) or musl static (then mimalloc or jemalloc is effectively
   required, per ripgrep's experience)?
7. **Minimum OS versions.**
   - Linux kernel ≥ 5.10 is suggested: errseq ≥ 4.16, `smaps_rollup` fields, cgroup v2; `MADV_POPULATE_READ` is ≥ 5.14
     and optional; `WRITE_ZEROES` is ≥ 6.17 and optional.
   - For macOS, ≥ 13 or ≥ 14? The APIs above exist on all current releases, but the test matrix needs a floor.
8. **The MCP server on a mapped-read fault.** Is "exit with code 10 and let the harness restart it" acceptable for the
   long-lived MCP server when a sealed file is truncated externally or the disk fails? The alternative is a `pread`-only
   mode for the server, which gives up zero-copy shared pages and raises its private RAM.
9. **macOS sync-client locations.** Refuse iCloud Drive, Dropbox and other File-Provider folders exactly like OneDrive?
   (This is shared with the file-identity lens.)
10. **WSL2 for probes.** Installing it (admin rights, reboot) would let future research lenses measure Linux behaviour
    on the owner's machine. The measurements would be indicative only: ext4 on a VHDX.

---

## 10. Sources

**Linux (documentation and source)**
- fsync(2): https://man7.org/linux/man-pages/man2/fsync.2.html
- sync_file_range(2): https://man7.org/linux/man-pages/man2/sync_file_range.2.html
- mmap(2): https://man7.org/linux/man-pages/man2/mmap.2.html
- madvise(2): https://man7.org/linux/man-pages/man2/madvise.2.html
- mallopt(3): https://man7.org/linux/man-pages/man3/mallopt.3.html
- /proc documentation: https://docs.kernel.org/filesystems/proc.html
- cgroup v2: https://docs.kernel.org/admin-guide/cgroup-v2.html
- overlayfs: https://docs.kernel.org/filesystems/overlayfs.html
- tmpfs: https://docs.kernel.org/filesystems/tmpfs.html
- dm-log-writes: https://docs.kernel.org/admin-guide/device-mapper/log-writes.html
- sysfs-block ABI: https://github.com/torvalds/linux/blob/master/Documentation/ABI/stable/sysfs-block
- ext4 fsync: https://github.com/torvalds/linux/blob/master/fs/ext4/fsync.c
- iomap direct I/O (FUA): https://github.com/torvalds/linux/blob/master/fs/iomap/direct-io.c
- mm/filemap.c (SIGBUS paths, readahead): https://github.com/torvalds/linux/blob/master/mm/filemap.c
- mm/memory.c (fault-around): https://github.com/torvalds/linux/blob/master/mm/memory.c
- FALLOC_FL_WRITE_ZEROES: https://lwn.net/Articles/1018299/ · https://kernelnewbies.org/Linux_6.17 · https://www.phoronix.com/news/XFS-FALLOC-FL-WRITE-ZEROES
- MAP_NOSIGBUS proposal: https://lwn.net/Articles/860419/
- Apple M1 NVMe flush cost (Axboe): https://lkml.iu.edu/2502.1/04706.html · Hellwig's reply: https://lkml.iu.edu/hypermail/linux/kernel/2502.1/04863.html

**macOS / XNU / Apple**
- fsync.2: https://github.com/apple-oss-distributions/xnu/blob/main/bsd/man/man2/fsync.2
- fcntl.2 (`F_FULLFSYNC`, `F_BARRIERFSYNC`, `F_PREALLOCATE`, `F_NOCACHE`, `F_PUNCHHOLE`): https://github.com/apple-oss-distributions/xnu/blob/main/bsd/man/man2/fcntl.2
- mmap.2: https://github.com/apple-oss-distributions/xnu/blob/main/bsd/man/man2/mmap.2
- ux_exception.c (SIGBUS mapping): https://github.com/apple-oss-distributions/xnu/blob/main/bsd/uxkern/ux_exception.c
- task_info.h (`task_vm_info`, `ledger_phys_footprint_peak`): https://github.com/apple-oss-distributions/xnu/blob/main/osfmk/mach/task_info.h
- task.c (phys_footprint definition): https://github.com/apple-oss-distributions/xnu/blob/main/osfmk/kern/task.c
- resource.h (`rusage_info_v4+`): https://github.com/apple-oss-distributions/xnu/blob/main/bsd/sys/resource.h
- vm_param.h (arm64 page shift): https://github.com/apple-oss-distributions/xnu/blob/main/osfmk/mach/arm/vm_param.h
- APFS FAQ: https://developer.apple.com/library/archive/documentation/FileManagement/Conceptual/APFS_Guide/FAQ/FAQ.html
- Kernel Programming Guide, BSD overview (UBC): https://developer.apple.com/library/archive/documentation/Darwin/Conceptual/KernelProgramming/BSD/BSD.html
- VZDiskImageSynchronizationMode: https://developer.apple.com/documentation/virtualization/vzdiskimagesynchronizationmode
- footprint(1): https://keith.github.io/xcode-man-pages/footprint.1.html
- Reducing disk writes (F_BARRIERFSYNC guidance; page not machine-readable in this session): https://developer.apple.com/documentation/xcode/reducing-disk-writes

**Windows**
- Reading and Writing From a File View (EXCEPTION_IN_PAGE_ERROR): https://learn.microsoft.com/en-us/windows/win32/memory/reading-and-writing-from-a-file-view
- Earlier Windows sources: [05 §6], [08 §4]

**Engines and runtimes (source)**
- Rust std unix fs: https://github.com/rust-lang/rust/blob/master/library/std/src/sys/fs/unix.rs
- SQLite os_unix.c: https://github.com/sqlite/sqlite/blob/master/src/os_unix.c
- LMDB mdb.c (0.9): https://github.com/LMDB/lmdb/blob/mdb.RE/0.9/libraries/liblmdb/mdb.c
- redb backend and locks: https://github.com/cberner/redb/tree/master/src/tree_store/page_store/file_backend · CHANGELOG: https://github.com/cberner/redb/blob/master/CHANGELOG.md
- RocksDB io_posix.cc: https://github.com/facebook/rocksdb/blob/main/env/io_posix.cc
- Go fd_fsync_darwin.go: https://github.com/golang/go/blob/master/src/internal/poll/fd_fsync_darwin.go
- mimalloc: https://github.com/microsoft/mimalloc
- memmap2: https://docs.rs/memmap2/latest/memmap2/struct.Mmap.html
- ripgrep allocator comment: https://github.com/BurntSushi/ripgrep/blob/master/crates/core/main.rs

**PostgreSQL**
- WAL configuration: https://www.postgresql.org/docs/current/runtime-config-wal.html
- Reliability: https://www.postgresql.org/docs/current/wal-reliability.html
- Error handling (`data_sync_retry`, `syncfs`): https://www.postgresql.org/docs/current/runtime-config-error-handling.html
- Fsync Errors wiki: https://wiki.postgresql.org/wiki/Fsync_Errors
- fdatasync on macOS (T. Munro): https://www.postgresql.org/message-id/CA+hUKGLfe_ogA5VDi1Jxj_MPVJLcif8T_GuaW0aOs+pBZT033A@mail.gmail.com

**Papers**
- Rebello et al., "Can Applications Recover from fsync Failures?", ATC 2020: https://www.usenix.org/conference/atc20/presentation/rebello
- Mohan et al., CrashMonkey/ACE, OSDI 2018: https://www.usenix.org/conference/osdi18/presentation/mohan
- Crotty, Leis, Pavlo, CIDR 2022 (via [05 §5.1])

**Third-party measurements and claims**
- marcan (HN): https://news.ycombinator.com/item?id=30370551 · summary: https://mjtsai.com/blog/2022/02/17/apple-ssd-benchmarks-and-f_fullsync/
- Eclectic Light, "How can you trust a disk to write data?": https://eclecticlight.co/2022/02/18/how-can-you-trust-a-disk-to-write-data/
- go-ethereum #28754: https://github.com/ethereum/go-ethereum/issues/28754 · golang/go #43342: https://github.com/golang/go/issues/43342
- t3code #13544 (M3, 2026-09): https://github.com/pingdotgg/t3code/issues/13544
- pnpm PR #15354 (M5 Pro): https://github.com/pnpm/pnpm/pull/15354
- vanedb #110: https://github.com/vanedb/vanedb/issues/110
- BonsaiDB, "SQLite on macOS: Not ACID": https://bonsaidb.io/blog/acid-on-apple/ · M. Tsai 2025: https://mjtsai.com/blog/2025/09/05/sqlite-on-macos-not-acid/
- Small Datum, SSD fsync 2026-01: http://smalldatum.blogspot.com/2026/01/ssds-power-loss-protection-and-fsync.html
- Lemire, hello-world startup: https://lemire.me/blog/2022/08/09/hello-world-is-slower-in-c-than-in-c-linux/
- rand #733 (macOS startup): https://github.com/rust-random/rand/issues/733
- rust-lang/rust #87764 (posix_spawn/vfork): https://github.com/rust-lang/rust/issues/87764
- Nethercote, Faster Rust builds on Mac (XProtect): https://nnethercote.github.io/2025/09/04/faster-rust-builds-on-mac.html
- desktop/desktop #13639: https://github.com/desktop/desktop/issues/13639
- ripgrep #581: https://github.com/BurntSushi/ripgrep/issues/581 · SIGBUS from mmap'd files: https://zuff.dev/posts/sigbus/ · uutils PR #11326: https://github.com/uutils/coreutils/pull/11326
- macOS footprint example: https://www.unixtutorial.org/macos-footprint-command/
- bitsnbites OS primitives (2017): https://www.bitsnbites.eu/benchmarking-os-primitives/
