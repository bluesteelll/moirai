# OS layer: read-only mappings of sealed files (`os::map`)

| Field | Value |
|---|---|
| Title | `os::map` — what may be mapped, the `total_len` size check, typed access, the mapping registry and the in-page-error / `SIGBUS` handler |
| Status | draft, pass 1 pending |
| Work package | WP-17a (role R-SPEC-P), part 1 of WP-17 |
| Sources | [80 §2.1] (`os::map` row); [80 §2.5] rules 1–8; [80 §2.3.2] (sealed-file protocol point); [80 §2.3.5] items (9), (10); [80 §3.1] X-F6; [AR §4.1] `Mapped?` column and rules paragraph; [AR §4.7]; [AR §4.10] "Mappings"; [AR §6.1] (region arenas released on a segment-set change); [AR §7.1] exit code 7; [60 §2.5] protocol decision (m) and fault-model items (9), (10); [60 §5.2] item 22; PLAN §6.1 #14 (VHDX deferred: an in-process raised `EXCEPTION_IN_PAGE_ERROR`); research report [X17 §4] as cited by [80] |

---

## 1. Scope

`os::map` is the `SealedMaps` sub-trait of `Vfs` ([OS/README §4.1]). It maps **sealed store files** read-only, checks
their size first, and turns an I/O fault inside a mapping into a one-line message and exit 7. It freezes the mapping
policy of X-F6 ([80 §2.5] rules 1–6); the `total_len` field itself lives in each sealed kind's header ([F09], [F10]).

---

## 2. What may be mapped

1. **Only sealed files, read-only and whole-file from offset 0** ([80 §2.5] rule 1). A file is sealed when it is
   completely written, `durable+meta` and `durable-name` have returned for it, it carries the read-only attribute
   ([OS/fs §4.6]), and a durable `Checkpoint` or commit names it. The sealed kinds are the segments (`seg.base.G`,
   `seg.dK`, `seg.b<ref_id>.K`), `cs.NNNN`, `hist.NNNN`, `blobs.NNNN` (mapped lazily at the first body read), `dict.D` and
   `gitmap.NNNN` ([AR §4.1]). **Log extents and `HEAD` are never mapped**; they are read with `read_at` ([OS/fs §4.3]).
2. **Sealed means read-only on disk** ([80 §2.5] rule 2): `seal` sets `FILE_ATTRIBUTE_READONLY` or mode `0o444`
   ([OS/fs §4.6]); GC clears the attribute before deleting on Windows ([OS/fs §4.7]).
3. **A sealed file is never truncated, extended, written, renamed over, or given a reused name** ([80 §2.5] rule 3); file
   numbers are monotonic ([AR §4.1]). On Windows the OS enforces most of this for a mapped file (truncation fails with
   `ERROR_USER_MAPPED_FILE`; a rename over it fails with error 32); on Unix only the protocol and the read-only mode do.
4. **No page size enters the format**: whole-file mappings need no offset alignment, and macOS on arm64 uses 16 KiB
   pages ([80 §2.5] rule 1).
5. Mapped views and `read`/`write` of the same file need not be coherent on Windows; because mapped files never change,
   coherence is irrelevant on every OS ([X17 §4.1]).

---

## 3. API

```rust
pub trait SealedMaps: VfsTypes {
    type Map: SealedMap;

    /// Checks `file_size(file) == expected_len`, maps the whole file read-only, and registers the mapping under `name`
    /// (the store-relative file name, used only in the fault message of §8). `expected_len` is the `total_len` the
    /// caller read from the file's header (§4).
    fn map_sealed(&self, file: &Self::File, expected_len: u64, name: &RelPath) -> Result<Self::Map, MapError>;

    /// A hint only; errors are ignored and semantics never change (§9).
    fn advise(&self, map: &Self::Map, offset: u64, len: u64, advice: Advice);
}

/// A read-only mapping of a whole sealed file. Dropping it unregisters (§7) and then unmaps.
pub trait SealedMap: Send + Sync {
    /// Exactly `len()` bytes. Read only through the typed, bounds-checked access of §6.
    fn bytes(&self) -> &[u8];
    fn len(&self) -> u64;
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Advice { Random, WillNeed, Sequential }

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MapError {
    /// The file's size differs from the header's `total_len` (§4). Nothing is mapped.
    SizeMismatch { expected: u64, actual: u64 },
    /// `expected_len` is 0; no sealed kind has an empty header. A programming error in release builds too.
    Empty,
    /// The mapping registry is full (§7); the caller exits 7. Nothing is mapped.
    RegistryFull,
    /// The OS refused the mapping (including `ERROR_NOT_ENOUGH_MEMORY`, `ENOMEM`, `EACCES`).
    Io(VfsError),
}
```

The file handle passed to `map_sealed` must have read access; after the call the caller may close it, because the mapping
keeps the file referenced on every OS.

---

## 4. The size check (rule 4)

Every sealed kind's header carries `total_len u64`, the file's exact length ([80 §2.5] rule 4, X-F6): `SegHdr`
(segments, `cs`, `hist`, `blobs`, [F09]), the `gitmap` page header and the 32-byte `dict` header `{magic "MDIC",
total_len u64, blake3_16, _ [4]}` ([F10]). Before mapping:

1. The caller reads the header with one `read_exact_at` ([OS/fs §4.3]) and checks the header's own magic and checksum
   (per [F09]/[F10]); a bad header is the same failure as step 4.
2. The caller calls `map_sealed(file, total_len, name)`.
3. `map_sealed` reads the size (`GetFileInformationByHandleEx(FileStandardInfo)` / `GetFileSizeEx`, or `fstat`) and
   returns `SizeMismatch` unless it equals `expected_len`. It maps exactly `expected_len` bytes.
4. On `SizeMismatch` the caller re-reads `HEAD` and retries once (the newest `HEAD` may name another file set); if the
   newest `HEAD` still names the file and the sizes still disagree, it exits 7 naming the file and
   `moirai doctor --fsck` (text in [F19]).

This is the Unix counterpart of the Windows delete-pending retry ([80 §2.5] rule 4). A truncation between step 3 and a
later read cannot be excluded on Unix; §8 turns it into exit 7 (fault-model item (9)).

---

## 5. Mapping calls

| | Windows | Linux | macOS |
|---|---|---|---|
| Map | `CreateFileMappingW(h, NULL, PAGE_READONLY, 0, 0, NULL)` → section; `MapViewOfFile(section, FILE_MAP_READ, 0, 0, 0)` → base; `CloseHandle(section)` (the view keeps it alive) | `mmap(NULL, len, PROT_READ, MAP_SHARED, fd, 0)` | `mmap(NULL, len, PROT_READ, MAP_SHARED, fd, 0)` |
| Unmap | `UnmapViewOfFile(base)` | `munmap(base, len)` | `munmap(base, len)` |
| Never | `FILE_MAP_WRITE`, `FILE_MAP_COPY`, large pages, a view offset other than 0 | `MAP_POPULATE`, `MAP_PRIVATE`, `mlock`, `mremap`, `PROT_WRITE` | same as Linux |

The page cache backs every read-only mapping with one shared copy of the file's pages on all three OSes, so a mapping
costs page-table entries, not private memory ([X17 §4.5]). Bytes of the last page beyond the file's end are never read
(§6).

---

## 6. Typed, bounds-checked access (rule 5) and the safety argument

- Mapped bytes are read **only through types for which every bit pattern is valid** — `zerocopy::FromBytes` views of
  integers and byte arrays in the M1 codec; never `bool`, enums or `char` — and nothing read from a mapping is transmuted
  to a type with invalid bit patterns ([80 §2.5] rule 5).
- **Every offset or length taken from mapped bytes is checked before use** (`get()`, never `get_unchecked`). A changed
  byte then yields a checksum failure or a wrong answer, never an out-of-bounds access.
- Enforcement: code review of every crate that reads a `SealedMap`, and a clippy `disallowed_methods` list
  (`get_unchecked`, `get_unchecked_mut`, `slice::from_raw_parts` on mapped memory) in those crates' configuration, added
  by the crate that first reads mappings (M1).
- **Safety comment** (required above the `unsafe` block that creates the slice in `moirai-os`'s `map` module, [80 §2.5]
  rule 5, [X17 §4.3]): the file is immutable for its whole life by protocol (rules 1–3 of §2); an external writer that
  changes it must act against the read-only attribute or mode; if one does, every read is still in bounds of the mapping
  because all access is typed and bounds-checked, so the effect is a wrong value or a checksum failure; truncation
  surfaces as `EXCEPTION_IN_PAGE_ERROR` or `SIGBUS`, which §8 converts into exit 7; the formal "a `&[u8]` must not change"
  obligation is broken only by that external writer.

---

## 7. The mapping registry

A process-global table lets the fault handler decide whether a faulting address lies in a sealed-file mapping and name the
file, without taking a lock.

- **Capacity:** `MAP_REGISTRY_SLOTS = 512` entries; allocated once, at the first `map_sealed` of the process (a process
  that maps nothing pays nothing), and never freed. A registration that finds no free entry fails with
  `MapError::RegistryFull` and maps nothing, so no mapping ever exists that the handler cannot attribute.
- **Entry (in memory, 64 bytes; not an on-disk layout):**

| Field | Type | Meaning |
|---|---|---|
| `gen` | `AtomicU32` | seqlock generation: even = stable, odd = being written |
| `name_len` | `u8` | bytes of `name` in use (0–40) |
| `_` | `[u8; 3]` | unused |
| `start` | `AtomicUsize` | base address of the view; 0 = entry free |
| `len` | `AtomicUsize` | mapped length in bytes |
| `name` | `[u8; 40]` | the store-relative file name, ASCII ([F02]: decimal numbers and fixed ASCII words); a longer name keeps its first 40 bytes |

- **Register** (under a process mutex): pick a free entry; `gen += 1` (odd); write `len`, `name_len`, `name`, `start`;
  `gen += 1` (even) with release ordering. Only then is the `SealedMap` returned.
- **Unregister** (`Drop` of the map, under the mutex): `gen += 1`; `start := 0`; `gen += 1`; then unmap. Rust's borrow
  rules guarantee no reference into the mapping outlives the map.
- **Lookup** (the handler; no lock, no allocation): for each entry, read `gen` (acquire); skip it if odd; read `start`,
  `len` and the name; read `gen` again; if it changed, re-read that entry at most twice more, then skip it. A match is
  `start ≤ addr < start + len`.

---

## 8. The fault handler (rule 6)

### 8.1 Installation

Once per process, at the first `map_sealed`, before the first mapping is returned:

| Windows | Linux, macOS |
|---|---|
| `AddVectoredExceptionHandler(1, handler)` (first in the vectored chain) | `sigaction(SIGBUS, {handler, SA_SIGINFO \| SA_ONSTACK}, &previous)`; `previous` is kept for chaining (it is Rust std's stack-overflow handler or the default [I]) |

### 8.2 Behaviour

1. **Match.** Windows: the exception code is `EXCEPTION_IN_PAGE_ERROR` (`0xC0000006`) and `ExceptionInformation[1]`
   (the faulting address) matches a registry entry. Unix: the signal is `SIGBUS` and `si_addr` matches an entry.
2. **On a match** the handler writes exactly one line to stderr and ends the process with **exit code 7** ("store
   unavailable", [AR §7.1]; a store I/O fault in a mapping is listed under exit 7 there):

   ```
   store I/O fault in <file> at <offset>: run moirai doctor --fsck
   ```

   `<file>` is the entry's name; `<offset>` is `addr − start` in decimal; the line is ASCII and ends with one LF
   ([80 §2.5] rule 6). Windows: one `WriteFile(GetStdHandle(STD_ERROR_HANDLE), …)`, then
   `TerminateProcess(GetCurrentProcess(), 7)`. Unix: one `write(2, …)`, then `_exit(7)`. The text is frozen with the
   error strings of [F19].
3. **Otherwise** the handler passes the fault on. Windows: return `EXCEPTION_CONTINUE_SEARCH`. Unix: if `previous` is an
   `SA_SIGINFO` handler, call it with the same arguments; if it is a plain handler, call it with the signal number; if it
   is `SIG_DFL` or `SIG_IGN`, reinstall `SIG_DFL` and return, so the faulting instruction re-executes under the default
   action (ignoring a synchronous `SIGBUS` would loop).
4. **Async-signal safety.** The handler touches only the registry (§7), a stack buffer of at most 160 bytes in which it
   formats the decimal offset by hand, and the calls named above. It allocates nothing, takes no lock and does not use
   `std`'s formatting.

### 8.3 What the protocol sees

A mapping fault is a crash at that point (fault-model item (9)): readers hold no locks; a writer's pending group is
adopted by the next flush holder ([80 §2.5] rule 6). The long-lived MCP server behaves the same way: command hooks keep
working, since each spawns a fresh process; `mcp_tool` hooks become non-blocking errors until Claude Code restarts the
server, so the context of those events is missing, never wrong ([80 §2.5] rule 6, [AR §7.5]). A `pread`-only server was
rejected because it would give up the shared mapped pages and raise private RAM ([X17 §9] item 8).

---

## 9. Advice (rule 7)

`advise(map, offset, len, advice)` is a hint: errors are ignored and semantics never change.

| `Advice` | Windows | Linux | macOS |
|---|---|---|---|
| `Random` (index sections: node table, CSR, bitsets) | no-op | `madvise(range, MADV_RANDOM)` | `madvise(range, MADV_RANDOM)` |
| `WillNeed` (before a tier-1 FTS scan) | `PrefetchVirtualMemory(GetCurrentProcess(), 1, &range, 0)` | `madvise(range, MADV_WILLNEED)` | `madvise(range, MADV_WILLNEED)` |
| `Sequential` | no-op | `madvise(range, MADV_SEQUENTIAL)` | `madvise(range, MADV_SEQUENTIAL)` |

The range is widened to page boundaries inside the mapping; the page size is read at run time (`sysconf(_SC_PAGESIZE)`)
and never enters the format. Never `MAP_POPULATE`, `mlock` or `mremap` ([80 §2.5] rule 7). `MADV_POPULATE_READ`
(Linux ≥ 5.14) is not used; a port may add it only as a pre-check that changes no semantics ([80 §2.13]).

---

## 10. Mapping lifetime and GC

- A process maps only the segments of its current set and the blob, `hist` and pinned files it actually reads
  ([AR §4.1]); on a segment-set change it releases the old maps with the region arenas that used them ([AR §6.1]).
- GC deletes a file only after `HEAD` has not named it for 60 s and no pin references it; a process that still maps it
  keeps reading valid bytes (Windows: the delete fails or leaves the file delete-pending; Unix: the inode stays valid)
  ([OS/fs §6.4]).
- A mapping outlives the file handle it was made from; the caller may close the handle at once.

---

## 11. Verification

| Where | What |
|---|---|
| Measurement 22 (WP-52, Windows) | `FILE_ATTRIBUTE_READONLY` on sealed files with GC's clear-then-delete; the in-page-error handler on a mapping whose VHDX is taken offline, or, if the owner defers the VHDX (PLAN §6.1 #14), on an in-process `RaiseException(EXCEPTION_IN_PAGE_ERROR, 0, 3, {0, addr, status})` with `addr` inside a registered mapping; the `total_len` check |
| M1 `Vfs` conformance | the handler (exit 7, the exact line), `SizeMismatch` and the retry-once path, `RegistryFull` |
| Simulator (WP-31) | fault-model item (9) as a crash at the read point; item (10) as an external truncation of a sealed file |
| Port kill loop ([80 §2.5] rule 8) | external truncation of a random sealed file: the affected readers exit 7, no acknowledged commit is lost, `doctor --fsck` names the file, `repair --rebuild-from-log` restores it |

---

## Appendix A. Per-OS mapping

| Item | Windows 11 (built from M0) | Linux ≥ 5.10 (port) | macOS ≥ 14 (port) |
|---|---|---|---|
| Size read for the check | `GetFileInformationByHandleEx(FileStandardInfo).EndOfFile` | `fstat(fd).st_size` | `fstat(fd).st_size` |
| Map | `CreateFileMappingW(PAGE_READONLY)` + `MapViewOfFile(FILE_MAP_READ)`, whole file | `mmap(PROT_READ, MAP_SHARED)`, whole file | same as Linux |
| Read-only on disk (`seal`) | `FILE_ATTRIBUTE_READONLY` | `0o444` | `0o444` (never `UF_IMMUTABLE`) |
| Truncation of a mapped file | impossible (`ERROR_USER_MAPPED_FILE`) | possible for a writer with permission → `SIGBUS` on access | same as Linux |
| Fault signal | `EXCEPTION_IN_PAGE_ERROR` (`0xC0000006`); faulting address in `ExceptionInformation[1]` | `SIGBUS`, `si_addr` | `SIGBUS` (XNU maps pager errors and accesses beyond EOF to `SIGBUS`, [X17 §4.2]) |
| Handler | vectored exception handler, first | `sigaction(SA_SIGINFO \| SA_ONSTACK)`, chained to the previous handler | same as Linux |
| Exit | `TerminateProcess(self, 7)` | `_exit(7)` | `_exit(7)` |
| Advice | `PrefetchVirtualMemory` for `WillNeed` | `madvise` | `madvise` |
| Page size | 4 KiB pages, 64 KiB view-offset granularity (irrelevant: offset 0) | 4 KiB (x86_64), 4/16/64 KiB (aarch64) | 16 KiB (Apple silicon) |

---

## Holes

| Id | What | Decided by | Candidates | Constraint the value must meet |
|---|---|---|---|---|
| — | none in this file | — | — | — |

## Open points for the review

| # | Point | Resolution in this file | For |
|---|---|---|---|
| 1 | [X17 §4.6] rule 6 exits with code 10 on a mapping fault; [80 §2.5] rule 6 with code 7 | code 7 ([80] is authoritative for its own reservation; exit 10 means "incomplete result" in [AR §7.1]) | R-REV-P |
| 2 | [X17 §4.6] rule 4 compares the size with a length named by the durable `Checkpoint`; [80 §2.5] rule 4 with the header's `total_len` | the header's `total_len` ([80] wins; [81] m15 showed `SegRef` has no length and no `HEAD` field is added) | R-REV-P |
| 3 | The registry's capacity and allocation are not fixed by any design document ("fixed-size", [80 §2.5]) | 512 entries of 64 bytes (32 KiB), allocated at the first mapping; a full registry refuses the mapping rather than leave a fault unattributable. The bound must stay above the most mappings any process kind holds at once (the MCP server with 8 branch views and their pinned sets is the largest, est. < 200); [F16] or [F17] may state that bound | R-REV-P, WP-16 |
| 4 | `map_sealed` takes the file's name, which [80 §2.1]'s `map_sealed(file, expected_len)` does not | needed for the fault line ("in `<file>`") without a lookup inside the handler | WP-30 |
| 5 | The registry uses a per-entry seqlock so that a concurrent unmap and remap on another thread cannot pair one entry's `start` with another's `len` | stated in §7 | R-REV-P |
| 6 | Chaining to a previous `SIG_IGN` for a synchronous `SIGBUS` | reinstalled as `SIG_DFL` so the fault terminates instead of looping | R-REV-P |
| 7 | If the owner defers the VHDX (PLAN §6.1 #14), measurement 22 exercises the handler with an in-process raised `EXCEPTION_IN_PAGE_ERROR` | the raised exception carries three parameters, the second being an address inside a registered mapping, so the handler's real path runs | WP-52 |
