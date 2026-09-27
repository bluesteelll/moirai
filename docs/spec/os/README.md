# OS layer: modules, seams and crate placement

| Field | Value |
|---|---|
| Title | OS layer specification, part 1: module map, the `Vfs` / `ProjectFs` / `Meter` seams, crate placement |
| Status | draft, pass 1 pending |
| Work package | WP-17a (role R-SPEC-P), part 1 of WP-17 ([PLAN §3.2] item 1). Part 2 (WP-17b) writes the remaining module files listed in §3 |
| Files of part 1 | `README.md` (this file), `fs.md`, `lock.md`, `map.md`, `env.md` |
| Sources | [80 §1] X3, X4, X5, X8, X9; [80 §2.1] (module table and boundary rules); [80 §2.9] (metering); [80 §3.1] X-F4, X-F5, X-F6; [AR §14]; [AR §2.2] T2; [AR §4.10] "The OS layer" bullet; [60 §2.5] row "`Vfs`/`ProjectFs`" (audits) and row "Cross-platform"; [90 §11.1] (composition root, GT20 (e)); PLAN §2.1, §2.2 (rows `moirai-vfs`, `moirai-os`, `moirai-vfs-sim`, `moirai-probes`, `moirai-probes-bin`), §3.3 (gap "Placement of `ProjectFs`, `Meter` and the grant table; `os::fs`/`mem`/`proc` additions"), §6.2 R1, R2, R8, R17, R18; review `docs/spec/reviews/a1-P.md` findings A1P-02, A1P-07, A1P-09, A1P-10, A1P-17 and its R1/R2 conditions |
| Reconciled with | part 2's drafts [OS/proc] (open points 1, 6), [OS/clock] §1, [OS/mem] (open points 1, 2), [OS/path] (open point 1) |

Citation forms used in `docs/spec/os/`: `[OS/<file> §x]` for this directory (`[OS/README §4.1]`, `[OS/fs §4.4]`),
`[Fnn]` for format chapters (their section numbers are not yet fixed, so this part cites the chapter only), `[CFG]` for
`docs/spec/config.md`, and the design-document forms `[AR §x]`, `[40 §x]`, `[80 §x]`, `[90 §x]`, `[60 §x]`.

---

## 1. Scope and conventions

### 1.1 What the OS layer is

The OS layer is the only place where moirai's code differs by operating system ([80] X3). It has two parts:

- **The seam** — traits and target-independent types in the crate `moirai-vfs`. Every other crate is written against it
  and is generic over it (`V: Vfs`, `P: ProjectFs`, `M: Meter`); nothing on the commit path uses `dyn` ([80] X4).
- **The implementations** — one per target family, in the crate `moirai-os`, selected at compile time and never swapped.
  Test doubles of the seam (`moirai-vfs-sim`, later `moirai-projfs-sim`) are test-only crates.

One on-disk format and one protocol hold on Windows, Linux and macOS ([80] X1, X2). This specification therefore states
every contract once, OS-independently, and then maps it to each OS in an appendix. Windows is built and tested in
M0–M11; the Linux and macOS rows are the frozen contract for the port phase ([80] X7, X8): a port adds implementations
of these rows and never changes a byte or a rule.

### 1.2 Normative language and types

- **Must / never** state requirements an implementation and its conformance tests check. **Should** states a
  recommendation whose violation is not a defect.
- Signatures are Rust 2024 (`rust-version = "1.98"`, PLAN §2.1). They are the contract WP-30 implements in
  `moirai-vfs` ("signatures equal WP-17's", PLAN WP-30); names of private helpers are not part of it.
- On-disk integers are little-endian ([80] X1). Part 1 defines one on-disk structure of its own, the swap intent of
  [OS/fs §4.9.3] (with the 24-byte `FileIdentity` it embeds, [OS/fs §2.6]); part 2's files state the layouts they fix
  (for example `ProcId`, [OS/proc §3.1], and `Stamp`, [OS/clock §3.1]); every other byte the OS layer touches belongs to
  a format chapter.
- A value decided by an M0 measurement is written `HOLE(<id>)` and listed in the file's Holes table.
- `[I]` marks an OS behaviour that this specification relies on but no source has measured; each such item names the
  M0 measurement or WP-33 test that verifies it.

### 1.3 Where each contract lives

| Contract | File | Frozen items it carries |
|---|---|---|
| Store files: open, create, positional I/O, the durability classes, `sync_dir`, `sync_group`, extents, seal, unlink, the three rename forms, sizes and identity, free space, sharing modes, error mapping, the swap intent | [OS/fs] | X-F5 (the class-to-call mapping, no downgrade, no direct I/O); fault-model items (5), (8) sharing and delete-pending, (10), (12) as the OS layer reports them |
| `LockBytes`: lock bytes, the contract items 1–10, the in-process grant table, lock order, bounded waits, probes, `LOCK` identity | [OS/lock] | X-F4; the lock-byte map of X-F1 (restated from [F03]) |
| Read-only mappings of sealed files, the `total_len` check, the mapping registry and the fault handler | [OS/map] | X-F6 rules 1–6 (mapping policy) |
| Environment guard: classification at every open, the full probe at `init`/`restore`, the OS-version check, per-OS allow-lists | [OS/env] | X-F6 (allow-lists and refusals); X-F5's "no downgrade" at `init` |
| `ProcId`, boot identity and Unknown-boot mode, liveness, parent watch, the `ProcHost` seam | [OS/proc] (part 2) | X-F2 |
| The wall, monotonic and boot clocks; stamps and lease deadlines; the clock rules of fault-model item (7) | [OS/clock] (part 2) | X-F2 (deadline form) |
| Metering (the `Meter` methods' sources, `CountingAlloc`) | [OS/mem] (part 2) | — |
| Paths P1–P12, `RelPath` and the other path types, canonical root | [OS/path] (part 2) | X-F7, X-F9 (names) |
| `ProjectFs` (R4 file identity and change tracking; project-file renames and deletes) | [OS/project] (part 2) | X-F8 |
| Shell transport rules T1–T10 | [OS/shell] (part 2) | X-F12 |
| Part 2's per-OS mapping appendix | [OS/mapping-appendix] (part 2) | — |
| IPC endpoint (leader only), spawn, terminal, test host | part 2 (file not yet named) | — |

---

## 2. Crates and placement

This section closes the PLAN §3.3 gap "Placement of `ProjectFs`, `Meter` and the grant table" for WP-17.

### 2.1 `moirai-vfs` — the seam (product crate)

`moirai-vfs` holds every trait and every target-independent type of the OS layer. It has **no dependencies**
(PLAN §2.2), carries `#![forbid(unsafe_code)]` (PLAN §2.1), uses no `cfg(target_os)`, `cfg(windows)`, `cfg(unix)` or
`std::os::*` (GT20 (d)), and is in the GT20 (e) cross-target check. It contains:

| Item | Kind | Specified in |
|---|---|---|
| `Vfs` and its sub-traits `VfsTypes`, `StoreFs`, `Locks`, `SealedMaps`, `EnvGuard`, `Clock`, `ProcHost` | traits | §4.1; [OS/fs], [OS/lock], [OS/map], [OS/env]; `Clock` signature §4.4, semantics [OS/clock]; `ProcHost` [OS/proc §10] |
| `ProjectFs` | trait | §4.2 (surface), [OS/project] (signatures) |
| `Meter` | trait | §4.3; sources in [OS/mem] |
| `GrantTable` — the in-process lock-ownership state machine | struct, pure (no I/O, no clock, no thread) | [OS/lock §5] |
| `LockByte`, `SlotIndex`, `Acquired`, `Grant`, `ProbeResult`, `LockError`, `LockMode` | types | [OS/lock §2, §4] |
| `RelPath`, `RelPathBuf`, `AbsPath`, `CanonicalRoot`, `EntryName`, `PathError` | types | [OS/path §2, §11] (one `RelPath` type for store and project paths; [OS/fs §2.1] adds only use-time checks) |
| `RootAccess`, `RootRole`, `Access`, `OpenHint`, `SyncKind`, `DurabilityClass`, `GroupMember`, `FileIdentity`, `FreeSpace`, `DirEntry`, `EntryKind`, `VfsCounters`, `ShareRetry`, `SwapOutcome`, `SwapRecovery`, `VfsError`, `VfsErrorKind`, `OsCode`, `DurabilityFailure` | types | [OS/fs §2, §4.9, §6] |
| `SealedMap`, `Advice`, `MapError` | types | [OS/map §3] |
| `ClassifyDepth`, `Classification`, `StoreVolume`, `FsKind`, `ExtentMethod`, `FsName`, `Refusal`, `CloudKind`, `OsVersion`, `ProbeReport`, `ProbeOutcome`, `EnvWarning` | types | [OS/env §2] |
| `Meter`'s `ChildPeak`, `PeakKind`, `HeapCounts`, `CpuTimes`, `ChildTicket`, `MeterError` | types | §4.3 |
| `Stamp`, `DeadlineState` and the HLC helper | types | [OS/clock §3, §4, §10] |
| `OsTag`, `ProcId`, `BootId`, `BootIdentity`, `Liveness`, `ParentRec`, `WatchEvent` | types | [OS/proc §10]; byte layouts that go on disk in [F03] |
| `OsFileId`, `VolumeCaps` | types | layouts in [F11]; semantics in [OS/project] |

**Why the grant table is here and not in `moirai-os`.** [80 §2.1] places "the in-process lock table" in a
target-independent part of `moirai-os`. It moves to `moirai-vfs` (PLAN §6.2 R2, confirmed by `a1-P.md` R2) because the
simulator must run the same table: fault-model tests with several clients in one process (X-F4's in-process two-client
case) are only meaningful if the simulator and the real layer decide in-process ownership with one piece of code, and
`moirai-vfs-sim` cannot depend on `moirai-os`, which exports nothing on the three cross targets and would fail GT20 (e).
The table is pure logic, so the move changes no semantics; [OS/lock §5] specifies it as a state machine that both
implementations drive. The move is recorded as a refinement of [80 §2.1] (open point 1).

### 2.2 `moirai-os` — the only OS crate (product crate)

- It is the only crate that may declare `windows-sys` or `libc` as a direct dependency and the only crate that may use
  `cfg(target_os)`, `cfg(windows)`, `cfg(unix)` and `std::os::*` ([80] X3; GT20 (d), enforced from M0, PLAN §2.1).
- It is the only crate with `unsafe` code (FFI, mappings, the fault handler, `CountingAlloc`); each `unsafe` block carries
  a safety comment (PLAN §2.1). The mapping safety argument of [OS/map §6] is recorded there.
- Its allowed dependencies are `moirai-vfs`, `windows-sys` (`cfg(windows)`), `libc` (`cfg(unix)`, from the port) and
  `blake3` (PLAN §2.2). `blake3` serves `BootId`/`vol_key` hashing (part 2).
- It implements `Vfs`, `ProjectFs` and `Meter` for the build target. At M0 only the Windows modules exist (PLAN §6.2 R1):
  `fs` (with `free_space`), `lock`, `map`, `env`, `proc`, `mem`, `project`, `path`, the `Meter` implementation and
  `test_host` behind the `test-host` cargo feature. The Unix modules are configured out and export nothing on the
  cross targets; there is no stub and no interim code ([90 §11.1]).
- Module tree (normative for placement, [80 §2.1]): `src/lib.rs` re-exports one implementation type per seam;
  `src/windows/{fs, lock, map, env, proc, mem, path, project, ipc, spawn, term, test_host}.rs`;
  `src/unix/…` (code shared by Linux and macOS: file I/O, OFD locks and the waiter thread, `mmap`, the `SIGBUS` handler);
  `src/linux/…`; `src/macos/…`. Each file carries `#![cfg(...)]` at module level only.
- The implementation types are `moirai_os::OsVfs`, `moirai_os::OsProjectFs` and `moirai_os::OsMeter`. Each is a zero-sized
  or small handle over process-global state (§5.4); constructing two values in one process shares that state.

### 2.3 Test doubles

| Crate | Implements | Rule |
|---|---|---|
| `moirai-vfs-sim` (test-only, M0) | `Vfs` | enforces fault-model items 1–12 ([F15]); runs the real `GrantTable` once per simulated process ([OS/lock §5.1]); several clients per simulated process; seeded determinism |
| `moirai-projfs-sim` (test-only, FL-2) | `ProjectFs` | per-OS `VolumeCaps` profiles as data ([80 §3.2]) |
| `moirai-probes` (test-only library) | — | generic over `V: Vfs` and `M: Meter`; never names `moirai-os` |

### 2.4 Dependency direction and composition roots

- Shared crates depend on `moirai-vfs`, never on `moirai-os` ([80 §5.5] (b)).
- Only a composition root, listed in `xtask/roots.toml`, depends on `moirai-os` (PLAN §2.1, §6.2 R17): `moirai` (M8) and
  `moirai-probes-bin` (M0). A root wires `OsVfs`, `OsProjectFs` and `OsMeter` into generic code and holds no logic.
- The composition-root lint refuses the `test-host` feature of `moirai-os` in the product root `moirai`; only
  `moirai-probes-bin` and test targets may enable it. [OS/env §9]'s test admissions compile only under that feature.
- `CountingAlloc` ([OS/mem §6]) is installed as `#[global_allocator]` only by probe roots, never by `moirai`, so the product
  pays no atomic per allocation (`a1-P.md`, R1 condition 2). `Meter::heap_counts` returns `None` in a binary that did not
  install it.

### 2.5 What enforces the placement

| Check | Rule | Source |
|---|---|---|
| GT20 (d), source part | `cfg(target_os)`, `cfg(windows)`, `cfg(unix)`, `std::os::*` only in `moirai-os`, tests included | PLAN §2.1 |
| GT20 (d), dependency part | `windows-sys` and `libc` as direct dependencies only in `moirai-os` | [80 §5.5] (a) |
| GT20 (d), product crates | `File::lock`, `std::fs::rename`, and direct `std::fs` file access (`File::open`, `File::create`, `OpenOptions`, `read_dir`, `metadata`, `remove_file`, `remove_dir`, `create_dir*`, `copy`, `hard_link`) forbidden in every product crate other than `moirai-os` | [80 §2.1] boundary rules; `a1-P.md` A1P-10 |
| GT20 (e) | `moirai-vfs` and `moirai-os` type-check for `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`, `aarch64-apple-darwin` and `x86_64-pc-windows-msvc` | [90 §11.1] |
| `#![forbid(unsafe_code)]` | in `moirai-vfs` and every crate except `moirai-os` | PLAN §2.1 |

The last-but-two row narrows [80 §2.1]'s "never used anywhere" for `File::lock` and `std::fs::rename` to product crates:
test and tool crates may use them on their own scratch files, never on a store file or a project file (A1P-10; open
point 2).

---

## 3. Module list

The twelve modules of [80 §2.1]. "Built at M0" is the Windows implementation of PLAN §2.2 and §6.2 R1; every module is
specified at M0.

| Module | Surface (names are the contract) | Seam | Spec file | Part | Built at M0 (Windows) |
|---|---|---|---|---|---|
| `os::fs` | `open_root`, `create_root`, `open`, `create_new`, `create_dir`, `remove_dir`, `list_dir`, `read_at`, `read_exact_at`, `write_at`, `sync`, `sync_dir`, `sync_group`, `fail_stop`, `create_extent`, `recycle_extent`, `seal`, `unlink`, `rename_noreplace`, `rename_replace`, `swap_dirs`, `swap_recover`, `file_size`, `identity`, `root_identity`, `path_identity`, `free_space`, `advise_dontneed`, `counters` | `Vfs` (`StoreFs`) | [OS/fs] | 1 | yes |
| `os::lock` | `lock_client`, `try_acquire`, `acquire_within`, `release`, `probe`, `holds_any_role`, `foreign_lock_check`; the kernel half under the `GrantTable` | `Vfs` (`Locks`) | [OS/lock] | 1 | yes |
| `os::map` | `map_sealed`, `advise`, unmap on drop; the mapping registry; the fault handler | `Vfs` (`SealedMaps`) | [OS/map] | 1 | yes |
| `os::env` | `classify`, `probe_store`, `check_os_version`, `doctor_warnings` | `Vfs` (`EnvGuard`) | [OS/env] | 1 | yes |
| `os::proc` | `ProcId` of self and parent; the boot identity; `alive` (diagnostics); `watch_parent`; the clocks' sources | `Vfs` (`ProcHost`, `Clock`), injected | [OS/proc], [OS/clock] | 2 | yes (with `peak_of_child` for the `Meter`) |
| `os::ipc` | `bind_endpoint`, `connect`, peer credentials (leader only) | — | part 2 (not yet named) | 2 | no (leader not decided) |
| `os::mem` | `private_now`, `private_peak`, `available_physical`, `peak_of_child`, the running-child readings of §4.3, `CountingAlloc` | `Meter` | [OS/mem] | 2 | yes |
| `os::path` | OS path ↔ stored path; `canonical_root`; reserved and unrepresentable names | `ProjectFs` | [OS/path] | 2 | yes |
| `os::project` | the `ProjectFs` surface of §4.2 | `ProjectFs` | [OS/project] | 2 | yes (the complete trait, PLAN §6.2 R1) |
| `os::spawn` | the detached `moirai gc` child; priorities; no inheritable handles | — | part 2 (not yet named) | 2 | no |
| `os::term` | console vs pipe; UTF-8 output; broken pipe | — | part 2 (not yet named) | 2 | no |
| `os::test_host` (feature `test-host`) | `kill`, `suspend`, `resume`, `small_volume`, clock offset ([OS/clock §9]) | — | part 2 (not yet named) | 2 | yes |

Additions to [80 §2.1]'s surface made by part 1, each explained in its file ([OS/fs] open point 1): `open_root`,
`create_root`, `create_dir`, `remove_dir`, `list_dir`, `read_exact_at`, `fail_stop`, `recycle_extent`, `swap_recover`,
`root_identity`, `path_identity`, `free_space`, `advise_dontneed` and `counters` in `os::fs` (`open_store_file` is split
into `open` and `create_new`); `lock_client`, `holds_any_role` and `foreign_lock_check` in `os::lock`; `doctor_warnings`
in `os::env`. Together they close the PLAN §3.3 gap "`os::fs` … additions" for WP-17 (open point 3). The `mem` and
`proc` additions (`private_now`, `private_peak`, `available_physical`, `peak_of_child`, the running-child readings and the
pre-spawn hook) are fixed at seam level in §4.3 and specified by [OS/mem] and [OS/proc].

---

## 4. The seams

### 4.1 `Vfs`

`Vfs` is the store-side seam. It is the union of six sub-traits, one per module file, over one set of handle types:

```rust
/// Handle types shared by every sub-trait. All handles are owned values; dropping one closes it.
pub trait VfsTypes {
    /// An open directory that relative operations are based on ([OS/fs §2.2]).
    type Root: Send + Sync;
    /// An open file ([OS/fs §2.3]).
    type File: Send + Sync;
}

pub trait StoreFs: VfsTypes { /* [OS/fs §3] */ }
pub trait Locks: VfsTypes { /* [OS/lock §4] */ }
pub trait SealedMaps: VfsTypes { /* [OS/map §3] */ }
pub trait EnvGuard: VfsTypes { /* [OS/env §2] */ }
pub trait Clock { /* §4.4; semantics [OS/clock] */ }
pub trait ProcHost { /* [OS/proc §10] */ }

/// The store-side seam. Implemented by `moirai_os::OsVfs` and by `moirai_vfs_sim::SimVfs`.
pub trait Vfs: StoreFs + Locks + SealedMaps + EnvGuard + Clock + ProcHost + Send + Sync + 'static {}
impl<T> Vfs for T where T: StoreFs + Locks + SealedMaps + EnvGuard + Clock + ProcHost + Send + Sync + 'static {}
```

`ProcHost` is a supertrait of `Vfs` rather than a separate bound ([OS/proc] open point 1): every store process needs the
boot identity before its first read ([AR §4.2] boot check), so a separate bound would appear on every generic store
function; the simulator implements it per simulated process ([OS/proc §10.1]).

Rules:
- Every method takes `&self`. An implementation value is cheap to clone or share; its process-global state follows §5.4.
- Store code is generic over `V: Vfs` and never boxes it ([80] X4).
- Every store file goes through `Vfs` ([80 §2.1]); so does every file of an image destination and of a backup directory
  (roots of role `Other`, [OS/fs §2.2]), because their protocol points use the durability classes ([80 §2.3.2]).
- Project files (the user's tree) never go through `Vfs`; they go through `ProjectFs`.

### 4.2 `ProjectFs`

`ProjectFs` is the project-file seam. It is separate from `Vfs` because it has its own simulator ([80] X4, [60] P1) and its
own evidence model ([80 §2.11]). Its signatures are written by part 2 ([OS/project]); this section fixes its placement and
surface so that WP-30 creates the complete trait once (PLAN §6.2 R2):

- **Placement:** trait in `moirai-vfs`; implementation `moirai_os::OsProjectFs`; simulator `moirai-projfs-sim` (FL-2).
- **Surface** ([80 §2.1] `os::path` and `os::project`, amended by `a1-P.md`):
  `volume(dir) → (VolumeKey, VolumeCaps)`, `stat`, `enumerate`, `locate_id`, `file_handle_digest`, `case_equivalent`,
  `read_for_hash`, `rename_noreplace`, `trash_dirs`, `durable_rename`, `durable_unlink`, **`sync_dir`** (A1P-02: intent
  recovery re-establishes the namespace barrier before it rolls forward), the path conversions and `canonical_root`.
- **Not in the M0 trait:** `journal_since` (A1P-17: E2 is not built, [AR §11] #41; `JOURNALCUR`/`JournalCursor` stay
  reserved in the format, and a later E2 adds the method additively).
- **Open for part 2:** the cross-volume `file mv` of a file (A1P-01) either becomes a refusal or a `ProjectFs` copy
  operation with its own protocol point; [OS/project] records the disposition.
- **Shared primitives:** `ProjectFs::rename_noreplace`, `durable_rename`, `durable_unlink` and `sync_dir` use the same
  per-OS calls, flags and error mapping as [OS/fs §4.7, §4.8, §4.4.3] (including `MOVEFILE_WRITE_THROUGH` on Windows and
  the bounded retry on errors 5 and 32 of [OS/fs §6.3]); project-file opens follow [80 §2.12]'s "Project-file reads"
  row, which [OS/project] specifies. Project roots are held as text and root id, not as open directory handles
  ([OS/path §6]), whereas store roots are open handles ([OS/fs §2.2]); both are P10 forms.

### 4.3 `Meter`

`Meter` is the measurement seam. The product reads it too: the query engine sets its `mem` budget from `private_now`
([50 §5.10], A1P-09), and the store's guards read free space ([OS/fs §4.11]). It is therefore a product trait, not a
probe-only one.

```rust
pub trait Meter: Send + Sync + 'static {
    /// Free and total bytes of the volume that holds `dir` ([OS/fs §4.11]).
    fn free_space(&self, dir: &std::path::Path) -> Result<FreeSpace, VfsError>;
    /// This process's private bytes now (the quantity every RSS gate uses, [80 §2.9]).
    fn private_now(&self) -> Result<u64, MeterError>;
    /// This process's peak private bytes so far.
    fn private_peak(&self) -> Result<u64, MeterError>;
    /// Physical memory the OS reports as available without paging, in bytes.
    fn available_physical(&self) -> Result<u64, MeterError>;
    /// Peak private bytes of a child that has exited and been waited for; call before `child` is dropped.
    fn peak_of_child(&self, child: &std::process::Child) -> Result<ChildPeak, MeterError>;
    /// Live and high-water heap bytes counted by `CountingAlloc`; `None` if this binary did not install it (§2.4).
    fn heap_counts(&self) -> Option<HeapCounts>;
    /// Sets the heap high-water mark to the live count; a no-op when `CountingAlloc` is not installed.
    fn reset_heap_high_water(&self);

    // Running children (measurement 19 and [60 §5.1]'s idle-CPU and MCP steady-state rows; [OS/mem §7]).
    /// Private bytes of a running child now.
    fn child_private_now(&self, child: &std::process::Child) -> Result<u64, MeterError>;
    /// Number of threads of a running child.
    fn child_threads(&self, child: &std::process::Child) -> Result<u32, MeterError>;
    /// User and kernel CPU time of this process (`None`) or of a child.
    fn cpu_times(&self, child: Option<&std::process::Child>) -> Result<CpuTimes, MeterError>;

    // The pre-spawn hook ([OS/proc] open point 6, [OS/mem] open point 2). It never spawns: GT20 (a) keeps spawns out of
    // product crates; the caller spawns with `std::process::Command`.
    /// Before spawning a child whose `peak_of_child` will be read: arrange for the OS to track its peak (Linux: a
    /// delegated leaf cgroup the child joins before `exec`). Windows and macOS: nothing to arrange.
    fn prepare_child(&self, cmd: &mut std::process::Command) -> Result<ChildTicket, MeterError>;
    /// After the spawn: binds the ticket to the child, so `peak_of_child(child)` finds what `prepare_child` set up.
    fn bind_child(&self, ticket: ChildTicket, child: &std::process::Child);
}

/// Opaque; `Copy` so that a caller that drops a child without reading its peak loses nothing.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct ChildTicket(pub u64);

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct CpuTimes { pub user_ns: u64, pub kernel_ns: u64 }

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct ChildPeak { pub private_peak_bytes: u64, pub kind: PeakKind }

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum PeakKind {
    /// A true peak (Windows `PeakPagefileUsage`, Linux cgroup-v2 `memory.peak`, macOS `ledger_phys_footprint_peak`).
    Peak,
    /// Read at exit only (Linux without a delegated cgroup): reported as "at-exit, not peak" ([80 §2.9]).
    AtExit,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct HeapCounts { pub live_bytes: u64, pub high_water_bytes: u64 }

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MeterError { pub os: OsCode, pub what: &'static str }
```

- `free_space` is specified in [OS/fs §4.11]; every other method is specified with its per-OS sources in [OS/mem]
  (and `peak_of_child` in [OS/proc §9]), whose sources [80 §2.9] already fixes (for example
  `PROCESS_MEMORY_COUNTERS_EX.PrivateUsage` and `PeakPagefileUsage` on Windows).
- The running-child readings and `reset_heap_high_water` are [OS/mem §7]'s proposal, adopted here so the seam is complete
  at M0 (PLAN §6.2 R2: no interim form). `prepare_child`/`bind_child` are no-ops on Windows (the M0 build); their Linux
  behaviour is a port-phase item of [OS/mem], and until it exists the Linux readings return `MeterError`, never a guess.
- `MeterError` is never a store error; a failed reading makes a measurement or a budget fall back as [OS/mem] states,
  never a store refusal.
- `moirai-probes` is generic over `M: Meter`; `moirai-probes-bin` passes `moirai_os::OsMeter` (PLAN §2.2).

### 4.4 `Clock` (signature fixed here; semantics in [OS/clock])

PLAN §2.2 places the clock in `moirai-vfs`. [OS/lock] needs the monotonic clock for its deadlines, and the simulator
steps all three clocks (fault-model item (7)), so the signature is fixed here:

```rust
pub trait Clock {
    /// Wall clock: milliseconds since the Unix epoch, UTC. May step backward or forward between two calls.
    fn wall_ms(&self) -> i64;
    /// Monotonic clock in nanoseconds from an unspecified origin; never goes backward within a process.
    fn mono_ns(&self) -> u64;
    /// Boot clock in nanoseconds since boot: monotonic and including time spent in suspend ([80 §2.7.1]).
    fn boot_ns(&self) -> u64;
}
```

[OS/clock] owns the per-OS sources (for `boot_ns`: `QueryInterruptTimePrecise`, `CLOCK_BOOTTIME`,
`mach_continuous_time`), stamps and deadlines; [OS/proc] owns the boot identity.

### 4.5 Types shared across the files

| Type | Meaning | Defined in |
|---|---|---|
| `OsCode` | the raw OS error code (`GetLastError` value or `errno`), carried for diagnostics only; rendered as `<OSERR>` in golden files ([80 §4] T7) | [OS/fs §6.1] |
| `VfsError` | every non-durability error of `Vfs`, `ProjectFs` and `Meter::free_space`: a kind plus the `OsCode` | [OS/fs §6.1] |
| `DurabilityFailure` | an error from a non-lazy durability class; consumed only by `fail_stop` | [OS/fs §4.4.5] |
| `FileIdentity` | a process-local identity of an open file (volume, file id), used by the `LOCK` identity check and the swap intent | [OS/fs §2.6] |

---

## 5. Rules for every implementation

### 5.1 Selection and weakening

1. **One implementation per seam per target, chosen at compile time** ([80] X4). There is no runtime switch between two
   implementations of one concern; the environment guard chooses among *methods* of one implementation (for example the
   extent-creation method, [OS/fs §4.5]), never between implementations.
2. **Nothing is silently weakened** ([80] X5). Where an OS or a file system cannot give a guarantee, the store is refused
   ([OS/env]) or the answer is marked (`Unknown`, `unverified`, Unknown-boot mode). No durability call falls back to a
   weaker one; no lock falls back to `flock`; no probe error reads as `Free`.
3. **Public, documented interfaces only** ([80] X9): documented Win32; the WDK-documented `NtFlushBuffersFileEx`,
   `NtCreateFile`, `RtlGetVersion`, `RtlNtStatusToDosError` and `KUSER_SHARED_DATA`; documented Linux syscalls and `/proc`
   files; the public SDK of macOS 14, plus `kern.bootsessionuuid` whose absence selects Unknown-boot mode ([OS/proc]). A
   private interface may only select a degraded mode, never gate correctness.

### 5.2 Handles, children and threads

1. Every handle and descriptor the OS layer opens is non-inheritable: `O_CLOEXEC` on Unix, `bInheritHandle = FALSE` (or no
   security attributes) on Windows ([80 §2.2.1] item 7).
2. No process spawns a child while any of its lock clients holds a role byte ([OS/lock §3] item 7); `os::spawn` asserts
   `Locks::holds_any_role() == false` before it spawns.
3. The OS layer creates one kind of thread only: on Unix, the lock-waiter thread of [OS/lock §7.2] (stack 64 KiB), which
   exists while a contended acquisition is pending and, if abandoned, until it obtains and releases the byte
   ([80 §2.1]). At idle there are no timers and no threads beyond the MCP server's stdin reader ([AR §2.2]).

### 5.3 Never used

| Never | Why | Where the rule is checked |
|---|---|---|
| `File::lock`, `flock` on a store file | `flock` conflicts with OFD locks on macOS and is invisible to them on Linux; on Windows it locks the whole range ([80 §2.1]) | GT20 (d) (product crates); [OS/lock §11] `doctor` check |
| `std::fs::rename` | replaces silently on Unix; the explicit forms are `rename_noreplace`, `rename_replace`, `swap_dirs` ([80 §2.1]) | GT20 (d) |
| `FILE_FLAG_WRITE_THROUGH`, `FILE_FLAG_NO_BUFFERING` as durability | not trusted on consumer drives; direct I/O would need an aligned format ([80 §2.3.1], [AR §2.8]) | [OS/fs §5] (open flags) |
| `F_BARRIERFSYNC`, plain `fsync` as `durable` on macOS | ordering only; data stays in the drive cache ([80 §2.3.1]) | [OS/fs] Appendix A |
| `O_DIRECT`, `RWF_DSYNC` | rejected ([80 §2.3.1], [81] m3) | [OS/fs] Appendix A |
| `MAP_POPULATE`, `mlock`, `mremap` | [80 §2.5] rule 7 | [OS/map §9] |
| process-associated `fcntl` locks (`F_SETLK`), `F_OFD_SETLKWTIMEOUT` | close pitfall; per-sleep bound ([80 §2.2.2]) | [OS/lock] Appendix A |

### 5.4 Process-global state

The OS layer keeps three pieces of process-global state. Each is created lazily, lives until the process exits, and is
shared by every implementation value in the process:

| State | Content | Specified in |
|---|---|---|
| lock registry | one `GrantTable` (with its kernel handles) per `LOCK` file identity opened by the process | [OS/lock §5.1] |
| mapping registry | a fixed-size, lock-free table of mapped address ranges | [OS/map §7] |
| fault handler | installed once, at the first `map_sealed` | [OS/map §8] |

The simulator keeps the same state **per simulated process**, not per OS process, so several simulated processes in one
test process keep their independent ownership ([OS/lock §5.1]).

---

## Appendix A. Per-OS placement and dependencies

| Item | Windows (built from M0) | Linux (port) | macOS (port) |
|---|---|---|---|
| Source directory in `moirai-os` | `src/windows/` | `src/unix/` + `src/linux/` | `src/unix/` + `src/macos/` |
| FFI crate | `windows-sys` (`cfg(windows)`; a `windows-link` release, no import-library build script, PLAN §2.4) | `libc` (`cfg(unix)`), checked against musl | `libc` (`cfg(unix)`); constants absent from `libc` (for example `F_OFD_SETLKW` if missing) declared locally in `src/macos/` |
| `windows-sys` feature families used by part 1 | `Win32_Foundation`, `Win32_Storage_FileSystem`, `Win32_System_IO`, `Win32_System_Threading`, `Win32_System_Memory`, `Win32_System_Diagnostics_Debug` (vectored handler), `Win32_Security`, `Win32_Security_Authorization` (the store directory's owner ACE), `Win32_Storage_CloudFilters` (sync-root detection), `Win32_System_SystemInformation`, `Wdk_Storage_FileSystem` (`NtFlushBuffersFileEx`, `NtCreateFile`), `Wdk_System_SystemServices` (`RtlGetVersion`) | — | — |
| Minimum OS the implementation targets ([80 §2.13]) | Windows 11 x64; refusal floor in [OS/env §6] | kernel ≥ 5.10, 64-bit, static musl | macOS 14, arm64; linked with `MACOSX_DEPLOYMENT_TARGET=14.0` |
| Test-host feature | `test-host` (kill, suspend, resume, clock offset) | same name | same name |

---

## Holes

| Id | What | Decided by | Candidates | Constraint the value must meet |
|---|---|---|---|---|
| — | none in this file | — | — | — |

The part-1 module files carry their own holes: [OS/fs] (the share-retry bound), [OS/lock] (the two wait bounds and the
release-delay distribution, referenced from [CFG] and [F15]), [OS/env] (none), [OS/map] (none).

## Open points for the review

| # | Point | Resolution in this part | For |
|---|---|---|---|
| 1 | [80 §2.1] puts the in-process lock table in `moirai-os`; PLAN §6.2 R2 puts it in `moirai-vfs` | `moirai-vfs`, as a pure state machine driven by both `moirai-os` and the simulator ([OS/lock §5]); no semantic change; `a1-P.md` R2 asks exactly for this record | R-REV-P |
| 2 | [80 §2.1]'s "never used anywhere" for `File::lock` and `std::fs::rename` is enforced for product crates only (PLAN §2.1 scopes) | narrowed to product crates, with direct `std::fs` file access added to the product-crate ban (A1P-10); test and tool crates may use them on their own scratch files only | R-REV-P |
| 3 | PLAN §3.3 gap "`os::fs`/`mem`/`proc` additions" | `os::fs` gains the operations listed in §3, each justified in [OS/fs]; `mem`/`proc` gain `private_now`, `private_peak`, `available_physical`, `peak_of_child`, the running-child readings and the pre-spawn hook at seam level (§4.3), specified by [OS/mem] and [OS/proc] | R-REV-P, WP-30 |
| 4 | `Meter` gains `private_now` and `private_peak` (A1P-09); `ProjectFs` gains `sync_dir` and drops `journal_since` (A1P-02, A1P-17) | applied at seam level (§4.2, §4.3) | WP-30, WP-17b |
| 5 | A1P-01 (cross-volume `file mv` of a file) needs either a refusal or a `ProjectFs` copy operation with a protocol point | left to [OS/project] (WP-17b) and chapter 16 (WP-16); this part fixes nothing that prejudges it | WP-16, WP-17b |
| 6 | Part 2's files are `proc.md`, `clock.md`, `mem.md`, `path.md`, `project.md`, `shell.md`, `mapping-appendix.md` ([OS/proc] header); `os::ipc`, `os::spawn`, `os::term` and `os::test_host` have no named file yet | §1.3 and §3 follow part 2's list; the four modules stay "part 2 (not yet named)" until WP-17b places them | WP-17b |
| 7 | The `Clock` signature is fixed here because [OS/lock] needs `mono_ns` | [OS/clock] keeps it unchanged (its §1); if it must change, this file and [OS/lock §7] change together | WP-17b |
| 8 | The implementation value types (`OsVfs`, `OsProjectFs`, `OsMeter`) are named here so that `moirai-probes-bin` can wire them | names are part of the contract WP-33 implements | WP-33 |
| 9 | [OS/proc] open point 1: `ProcHost` as a separate bound or a supertrait of `Vfs` | supertrait (§4.1): every store process needs the boot identity before its first read | WP-17b, WP-30 |
| 10 | [OS/mem] open point 1 proposes running-child readings and `reset_heap_high_water`; [OS/proc] open point 6 and [OS/mem] open point 2 ask whether `Meter` gets a pre-spawn hook now | adopted now (§4.3): `child_private_now`, `child_threads`, `cpu_times`, `reset_heap_high_water`, and the pair `prepare_child`/`bind_child` (a hook that never spawns, so GT20 (a) keeps spawns out of product crates); `peak_of_child`'s signature is unchanged. Adding them later would change the seam after M0, which R2 excludes | WP-17b, WP-30 |
| 11 | [OS/path] open point 1: one `RelPath` type for store and project paths, or two | one type, with [OS/path §2.1]'s grammar (store names are a subset); [OS/fs §2.1] keeps only use-time checks for store operations (for example Windows refuses a component NTFS cannot hold) | WP-17b, WP-30 |
