# OS layer: project files — `ProjectFs`, `OsFileId`, `VolumeCaps` and the R4 runtime sources (`os::project`)

| Field | Value |
|---|---|
| Title | OS layer specification, part 2: the complete `ProjectFs` trait (read and write side), file identity per OS, volume capabilities, timestamps and attributes, cloud placeholders, the sources of `FILEOBS`, `DIRMAP`, `JOURNALCUR` and `TREES` |
| Status | draft, pass 1 pending |
| Work package | WP-17b (role R-SPEC-P), part 2 of WP-17 ([PLAN §3.2] item 1) |
| Sources | [80 §1] X1, X4, X5, X9; [80 §2.1] (`os::project`, `os::path` rows); [80 §2.3.1], [80 §2.3.2] rows "`file mv` rename", "`file rm` and `--trash`"; [80 §2.3.5] items (2), (12); [80 §2.10] P8–P10; [80 §2.11.1] (the capability table), [80 §2.11.2] (tagged layouts), [80 §2.11.3] (the frontier), [80 §2.11.4] (E1–E8 per OS, rules 1–9), [80 §2.11.5]; [80 §2.12] rows "Project-file reads", "Store files' sharing", "Temporary files"; [80 §3.1] X-F5, X-F8; [80 §8.1] B2, M4, M5, m1, m2, m7, m12, m15; [40 §2.4] (reparse points, hard links, directories), [40 §2.5] (two-pass reader), [40 §2.6] (runtime tables, OS id paragraph, stat quadruple), [40 §2.10] I-F4, I-F5, I-F11, [40 §2.11] R-7, R-8, R-14, R-18; [40 §3.4], [40 §3.5]; [40 §4.3] (STAT, E1–E8, copy rule), [40 §4.6] (cloud-synced roots, dataless files), [40 §4.7] (USN, FSEvents: not built), [40 §4.8]; [40 §8.3.6]; [AR §4.10] (namespace durability, `sync_dir`), [AR §5e.3], [AR §8.2] item 15; [60 §2.5] rows "`Vfs`/`ProjectFs`", "Resolver constants (R-14)", "Cross-platform"; [60 §5.2] item 15; PLAN §3.3 gap "Codex `apply_patch` parity in measurement 15"; review `a1-P.md` A1P-01, A1P-02, A1P-05, A1P-07, A1P-10, A1P-17; review `a1-S.md` S-16, S-17; [X19 §8] (research input) |
| Reconciled with | [OS/README §2.1, §2.3, §4.2, §4.5] (placement; surface); [OS/fs §2, §4.4.3, §4.7, §4.8, §6] (`VfsError`, `VfsErrorKind`, `OsCode`, `ShareRetry`, `DurabilityFailure`, the shared rename, unlink and directory-flush calls; its `DirEntry`/`EntryKind` are store types, so this file's are `ProjEntry`/`ProjKind`); [OS/path] (`RelPath` and the other path types); [F01 §3.2, §5.7, §6.3, §7] (OS tags, `unix_ns`, `lp()`, hash framing); [F02 §5.3, §5.4] (`tmp/`, `trash/`); [F20 §1.2, §3.5, §3.6, §5.1, §5.9, §5.12.1] (granularity, twins, spelling, `SKEW`, the NTFS holes, the racy threshold) |

---

## 1. Scope and placement

`ProjectFs` is the seam through which moirai touches the **user's files**: the trees that R4 links into, the OS trash, and
the two store directories a file verb uses as rename targets or stamps (`<store>/trash/…`, `<store>/tmp/`). Every project
file goes through it ([80 §2.1]); product crates other than `moirai-os` open no file themselves ([OS/README §2.5],
A1P-10).

- **Trait and types:** `moirai-vfs` ([OS/README §4.2]). **Implementation:** `moirai_os::OsProjectFs`, Windows built at M0
  ([PLAN §6.2] R1). **Simulator:** `moirai-projfs-sim` (FL-2), §9.
- The trait is **complete at M0** ([PLAN §6.2] R2): read side (§5), write side (§6), roots, volumes and path conversions
  (§2, §4). It has **no** `journal_since` (A1P-17): E2 is not built ([AR §11] #41), `JOURNALCUR`/`JournalCursor` stay
  reserved (§7.1), and a later E2 adds the method additively.
- It performs **no copy**: a cross-volume file move is refused (§6.4, A1P-01).
- The byte layouts of `OsFileId`, `FsTime`, `FileAttrs`, the `VolumeCaps` snapshot and `JOURNALCUR` are given here
  because their values are OS-defined; [F11] embeds them unchanged in `FILEOBS`, `PENDING`, `DIRMAP`, `TREES`,
  `JOURNALCUR` and `FSINTENT`, whose row layouts are [F11]'s.

## 2. The trait

### 2.1 Signature

```rust
/// A place for one operation: a root and a path under it (the empty path is the root itself).
pub struct At<'a, R> { pub root: &'a R, pub path: RelPath<'a> }   // Copy for every R; RelPath<'a> by value ([OS/path §2.1])

pub trait ProjectFs: Send + Sync + 'static {
    /// An opened directory used as the base of relative operations: a tree root, a named root, `<store>/tmp`
    /// or `<store>/trash/<intent>`. Holds no OS handle on Windows (§2.2).
    type Root: Send + Sync;
    /// One project file open for a streaming read (§5.5).
    type Reader: ProjectRead;

    // --- Roots, volumes and path conversions ([OS/path]) ---
    fn canonical_root(&self, dir: &std::path::Path) -> Result<CanonicalRoot, VfsError>;            // [OS/path §4]
    fn canonical_abs(&self, p: &std::path::Path) -> Result<AbsPath, VfsError>;                     // [OS/path §5]
    fn cli_path(&self, arg: &std::ffi::OsStr, cwd: &std::path::Path, tree: &CanonicalRoot)
        -> Result<RelPathBuf, PathError>;                                                          // [OS/path §7]
    fn open_root(&self, root: &CanonicalRoot) -> Result<Self::Root, VfsError>;                     // §2.2
    fn volume(&self, root: &Self::Root) -> Result<(VolumeKey, VolumeCaps), VfsError>;             // §4.1
    fn case_equivalent(&self, dir: At<'_, Self::Root>) -> Result<DirEquivalence, VfsError>;       // §4.5
    fn trash_dirs(&self, root: &Self::Root) -> Result<Vec<AbsPath>, VfsError>;                    // §5.7
    fn measure_mtime_granularity(&self, stamp_dir: &Self::Root) -> Result<u64, VfsError>;         // §4.4

    // --- Read side ---
    fn stat(&self, at: At<'_, Self::Root>, mode: StatMode) -> Result<Stat, VfsError>;             // §5.1
    fn disk_spelling(&self, at: At<'_, Self::Root>) -> Result<RelPathBuf, VfsError>;              // §5.3
    fn enumerate<F>(&self, dir: At<'_, Self::Root>, visit: F) -> Result<EnumEnd, VfsError>
        where F: FnMut(&ProjEntry<'_>) -> core::ops::ControlFlow<()>;                               // §5.2
    fn locate_id(&self, root: &Self::Root, id: &OsFileId, recorded: FileAttrs)
        -> Result<Located, VfsError>;                                                              // §5.4
    fn file_handle_digest(&self, at: At<'_, Self::Root>) -> Result<Option<[u8; 8]>, VfsError>;  // §5.4
    fn read_for_hash(&self, at: At<'_, Self::Root>, opts: ReadOpts) -> Result<Self::Reader, VfsError>; // §5.5
    fn read_link(&self, at: At<'_, Self::Root>, out: &mut Vec<u8>) -> Result<(), VfsError>;      // §5.6
    fn busy_holders(&self, at: At<'_, Self::Root>) -> Result<Vec<Holder>, VfsError>;             // §5.8
    fn touch_stamp(&self, at: At<'_, Self::Root>) -> Result<FsTime, VfsError>;                   // §5.9

    // --- Write side ---
    fn rename_noreplace(&self, from: At<'_, Self::Root>, to: At<'_, Self::Root>, retry: ShareRetry)
        -> Result<Renamed, VfsError>;                                                              // §6.1
    fn sync_dir(&self, dir: At<'_, Self::Root>) -> Result<(), DurabilityFailure>;                 // §6.2
    fn durable_rename(&self, from: At<'_, Self::Root>, to: At<'_, Self::Root>, retry: ShareRetry)
        -> Result<Renamed, RenameFailure>;                                                         // §6.3
    fn unlink(&self, at: At<'_, Self::Root>, retry: ShareRetry) -> Result<(), VfsError>;          // §6.3
    fn remove_dir(&self, at: At<'_, Self::Root>, retry: ShareRetry) -> Result<(), VfsError>;      // §6.3
    fn durable_unlink(&self, at: At<'_, Self::Root>, retry: ShareRetry) -> Result<(), RenameFailure>; // §6.3

    // --- Instrumentation ---
    fn counters(&self) -> PfsCounters;                                                             // §2.4
}

/// One open project file (§5.5).
pub trait ProjectRead {
    /// Reads at the current offset; 0 means end of file.
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, VfsError>;
    /// Returns to offset 0 on the same handle (the second pass of [40 §2.5]).
    fn rewind(&mut self) -> Result<(), VfsError>;
    /// Size and last-write time read through the open handle (A1P-05).
    fn snapshot(&self) -> Result<ReadSnapshot, VfsError>;
    /// The identity of the open object.
    fn identity(&self) -> Result<OsFileId, VfsError>;
}

/// A durable operation whose namespace step may have happened: the caller must know which.
pub enum RenameFailure {
    /// Nothing changed on disk (the rename or unlink itself failed).
    NotDone(VfsError),
    /// The rename or unlink happened, then a directory flush failed: the caller passes this to `fail_stop`.
    NotDurable(DurabilityFailure),
}
```

The remaining types are defined where they are used: `StatMode`, `Stat`, `StatRec`, `ProjKind` (§5.1); `ProjEntry`,
`EnumEnd` (§5.2); `Located` (§5.4); `ReadOpts`, `ReadSnapshot` (§5.5); `Holder` (§5.8); `Renamed` (§6.1); `VolumeKey`,
`VolumeCaps`, `DirEquivalence` (§4); `OsFileId`, `FsTime`, `FileAttrs` (§3); `PfsCounters` (§2.4). `VfsError`,
`ShareRetry` and `DurabilityFailure` are [OS/fs]'s; `RelPath`, `RelPathBuf`, `CanonicalRoot`, `AbsPath`, `EntryName`,
`PathError` are [OS/path]'s. `ProjEntry` and `ProjKind` are distinct from [OS/fs §2.7]'s store `DirEntry` and `EntryKind`,
which have no symbolic-link kind and no attributes.

### 2.2 Roots and handles

- `open_root(c)` checks that the directory named by `c.text` still has `c.root_id` (whole-id identity, §3.2) when
  `c.root_id.kind ≠ 0`; a mismatch is `VfsError` of kind `Stale` (the directory at that path was replaced).
- **Windows:** a `Root` holds the `\\?\` form of the text and the root id, **not an open handle**. Every operation opens what
  it needs and closes it before returning. An open directory handle would make renames of that directory's ancestors fail
  with error 5 ([40 §4.7]), and I-F11 allows no project handle between operations.
- **Linux, macOS (port):** a `Root` holds an `O_PATH` (Linux) or `O_RDONLY | O_DIRECTORY` (macOS) descriptor with
  `O_CLOEXEC`, used for `openat`-relative walks (P10); an open descriptor blocks no rename on Unix.
- A `Root` never outlives the command, the hook invocation or the MCP request that opened it ([40 §4.8] "Idle", [AR §6.6]).
- **Project-file opens** use full sharing (`FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE`), never map or lock a
  project file, and close each handle before the calling thread touches the next file (I-F11, [80 §2.12]). The ≤ 2
  scoped content readers of `--deep` passes ([40 §4.8]) each hold at most one `Reader`.

### 2.3 Errors

`ProjectFs` reports every failure as `VfsError { kind, os, call }` ([OS/README §4.5], [OS/fs §6.1]); the `OsCode` is for
diagnostics (rendered `<OSERR>` in golden files, [OS/shell §6]). It uses [OS/fs §6.2]'s mapping of OS codes to
`VfsErrorKind` unchanged, with these meanings on project files:

| Kind ([OS/fs §6.1]) | Meaning for `ProjectFs` | Windows | Linux, macOS |
|---|---|---|---|
| `NotFound` | the path or a parent is absent, where an operation needs the object (`stat` returns `Absent` instead) | 2, 3 | `ENOENT`, `ENOTDIR` |
| `AlreadyExists` | the destination of a no-replace rename exists | 80, 183 | `EEXIST` |
| `NotEmpty` | `remove_dir` of a non-empty directory | 145 | `ENOTEMPTY` |
| `AccessDenied` | access refused: another principal, a sandbox, TCC, SIP; on a Windows rename or delete also the transient form of a sharing conflict, retried per `ShareRetry` ([OS/fs §6.3]) and returned as is after the bound; from `sync_dir`, a directory that may not be opened for flushing although the rename would be allowed (§6.2, `no_dir_flush`) | 5, 1920 | `EACCES`, `EPERM` |
| `SharingViolation` | a Windows sharing conflict after the `ShareRetry` bound (the verb prints the holders, §5.8) | 32, 33 | — |
| `DeletePending` | the name belongs to a file another process deleted while holding it open | `STATUS_DELETE_PENDING`, 303 | — |
| `DiskFull` | disk full or quota | 39, 112, 1295 | `ENOSPC`, `EDQUOT` |
| `ReadOnlyVolume` | the project volume is write-protected | 19 | `EROFS` |
| `Unsupported` | the volume lacks the capability: a Linux directory rename without `RENAME_NOREPLACE`, a macOS volume without `RENAME_EXCL`, restart-manager holders of a directory; from `sync_dir`, a volume that cannot flush a directory (an SMB share, some FUSE mounts, `\\wsl$`; §6.2, `no_dir_flush`) | 50, 1 | `EINVAL` from `renameat2`, `ENOTSUP` |
| `CrossDevice` | the operation would cross volumes or mounts (§6.4) | 17 | `EXDEV` |
| `Busy` | a busy object (a mount point) | 170 | `EBUSY` |
| `InvalidName` | a component or path exceeds the OS limit or cannot be expressed ([OS/path §6]); on Windows also a segment that fails `representable_here` ([OS/path §8.1]), found before any OS call (below) | 123, 206; the name check | `ENAMETOOLONG`, `EILSEQ` |
| `Io` | read errors and anything else on a read (fault-model item (12)) | 23, 1117, 483, … | `EIO`, … |
| `Other` | any other code | other | other |

**The Windows name check** (pass 1, P1-15). On Windows every `ProjectFs` method tests every segment of every `At` path it
is given, and of every path it builds, with [OS/path §8.1]'s `representable_here` before it makes any OS call, and returns
`InvalidName` for a failing segment without calling the OS. `\\?\` paths bypass Win32 name normalisation ([OS/path §6]),
so without the check a git path such as `x::$DATA` would open the default stream of `x` and hash another file's content,
`a:b` would address the alternate stream `b` of `a`, and a segment ending in `.` or a space would be created literally.
`--allow-nonportable` never relaxes this check; it relaxes only [OS/path §8.2]'s portability issues for other OSes. On
Linux and macOS `representable_here` tests only the length, which the OS enforces anyway.

`ProjectFs` needs five kinds beyond those of store files; [OS/fs §6.1] lists them in `VfsErrorKind`, with their mapping
rows in [OS/fs §6.2] (pass 1, A1-33; open point 6):

| Kind (added) | Meaning | Windows | Linux, macOS |
|---|---|---|---|
| `CloudOnly` | the operation would hydrate a cloud-only entry (§5.10) | decided from the entry's attributes before any open; a code of the `ERROR_CLOUD_FILE_*` family if one still occurs (matched by name: winerror.h has members at 358, 404, 426, 434 and 475 and gaps inside 362–400, [OS/fs §6.2]) | macOS `SF_DATALESS`, or the error a read returns while materialisation is off |
| `IsSymlink` | a content read of a symbolic link (use `read_link`) | reparse tag `IO_REPARSE_TAG_SYMLINK` | `ELOOP` from `O_NOFOLLOW` |
| `IsDirectory` | a content read or unlink of a directory | 267 `ERROR_DIRECTORY`, or the attributes | `EISDIR` |
| `OutsideRoot` | an opened object's final path is not under the root (§5.2, §5.5) | the final-path check | `EXDEV` from `RESOLVE_BENEATH`, `ELOOP` from `O_NOFOLLOW_ANY` |
| `Stale` | the root's identity changed (§2.2) | the root-id check | the root-id check |

**Denials are never absence** ([80 §2.11.4] rule 9, [81] m12): `AccessDenied` is never mapped to `Absent`, `Gone` or
`NotFound` by any method; the resolver maps it to `Unknown` or "source absent".

### 2.4 Counters

`counters()` returns cumulative counts for the process, so that measurements and gates read exact numbers (A1P-07):

```rust
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct PfsCounters {
    pub renames: u64,       // rename_noreplace and the rename step of durable_rename
    pub dir_syncs: u64,     // sync_dir calls, including those inside durable_* (the "directory flushes" of a verb)
    pub unlinks: u64,       // unlink, remove_dir and the unlink step of durable_unlink
    pub stats: u64,         // stat calls that passed the Windows name check (§2.3)
    pub dir_reads: u64,     // enumerate calls that passed the Windows name check (§2.3)
    pub id_lookups: u64,    // locate_id and file_handle_digest calls that reached the OS
    pub content_opens: u64, // read_for_hash opens
    pub bytes_read: u64,    // bytes returned by Reader::read
}
```

A verb's flush accounting reads the store's log data flushes from [OS/fs]'s counters and `dir_syncs` from these, so the
`file mv` gate reads "2 log flushes and 2 directory flushes" ([AR §8.3], A1P-07). As `id_lookups` counts only calls that
reached the OS, `stats` and `dir_reads` count only calls that passed the Windows name check: a call refused with
`InvalidName` before any OS call counts nothing.

## 3. File identity, timestamps and attributes

### 3.1 `OsFileId` (57 bytes, packed, little-endian)

| Offset | Width | Type | Name | Meaning |
|---|---|---|---|---|
| 0 | 1 | u8 | `kind` | 0 `none`, 1 `ntfs128`, 2 `refs128`, 3 `linux_ino`, 4 `darwin_fileid`; 5–255 reserved |
| 1 | 16 | [u8; 16] | `vol_key` | the volume key (§4.1) |
| 17 | 16 | [u8; 16] | `id` | the object's id, encoded per kind (below) |
| 33 | 16 | [u8; 16] | `parent` | the parent directory's id in the same encoding; all zero when unknown |
| 49 | 4 | u32 | `aux` | reserved, zero |
| 53 | 4 | u32 | `docid` | macOS document id if owner decision #21 (d) enables it; else zero |
| total | 57 | | | |

| Kind | `id` bytes 0–15 | Source (entry) | Source (parent) |
|---|---|---|---|
| 0 `none` | all zero; every other field zero | — | — |
| 1 `ntfs128`, 2 `refs128` | `FILE_ID_128.Identifier`, the 16 bytes as returned | `FILE_ID_EXTD_DIR_INFO.FileId` (enumeration) or `FILE_ID_INFO.FileId` (`GetFileInformationByHandleEx(FileIdInfo)`) | `FILE_ID_INFO.FileId` of the directory handle the enumeration used, or of the parent opened for a `WithId` stat |
| 3 `linux_ino` | `u64le(ino) ‖ hgen`, `hgen` = the first 8 bytes of BLAKE3-256 over `u32le(handle_type) ‖ f_handle[0..handle_bytes]` from unprivileged `name_to_handle_at(dirfd, name, …, AT_SYMLINK_NOFOLLOW)` ([80 §2.11.2], [81] B2) | `statx` inode and `name_to_handle_at` | the same two calls on the directory |
| 4 `darwin_fileid` | `u64le(fileid) ‖ 8 zero bytes` | `ATTR_CMN_FILEID` (`getattrlistbulk` or `getattrlist`) | `ATTR_CMN_PARENTID` |

- The kind is the volume's `id_kind` (§4.2); it is never mixed within one volume.
- `nFileIndex` (the 64-bit Windows index) is never used: on ReFS it can be −1 ([40 §2.6]).
- A value whose kind is reserved, or whose kind is 0 with any non-zero byte, or whose `aux` is non-zero, is
  uninterpretable and treated as absent ([80] X1).

**The Rust type** (in `moirai-vfs`):

```rust
#[repr(u8)]
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum FileIdKind { None = 0, Ntfs128 = 1, Refs128 = 2, LinuxIno = 3, DarwinFileId = 4 }  // `from_u8`: None for 5–255
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct VolumeKey(pub [u8; 16]);

#[derive(Copy, Clone, Debug)]          // `PartialEq`, `Eq` and `Hash` below
pub struct OsFileId {
    pub kind: FileIdKind,
    pub vol_key: VolumeKey,
    pub id: [u8; 16],
    pub parent: [u8; 16],
    pub docid: u32,                    // no `aux` field: it is reserved and always zero
}
impl OsFileId {
    pub const LEN: usize = 57;
    pub const NONE: OsFileId;          // kind `none`, every field zero
    pub const fn is_none(&self) -> bool;
    pub const fn canonical(self) -> OsFileId;             // NONE for every value of kind `none`, else itself
    pub fn same_object(&self, other: &OsFileId) -> bool;  // §3.2
    pub fn to_bytes(&self) -> [u8; 57];                   // the canonical form
    pub fn from_bytes(b: &[u8; 57]) -> Option<OsFileId>;  // None when uninterpretable (above)
}
```

- The fields are public; there is no `aux` field. A non-zero `aux` makes `from_bytes` answer `None` (uninterpretable).
- `to_bytes` encodes the canonical form: `aux` is written as zero, and a value of kind `none` as 57 zero bytes whatever
  its other fields hold, so `from_bytes(&v.to_bytes()) == Some(v)` for every `v`.
- `==` and `Hash` compare the canonical form (every field of it), so every value of kind `none` equals `OsFileId::NONE`.
  Object identity is only ever `same_object` (§3.2), which ignores `parent` and `docid`; `==` is not identity.

### 3.2 The identity rule (frozen, X-F8)

```
same_object(a, b) ⇔ a.kind = b.kind ≠ 0  ∧  a.vol_key = b.vol_key  ∧  a.id = b.id
```

`parent`, `aux` and `docid` never take part. On Linux an inode number alone is never identity: `id` includes `hgen`, so an
`rm` and re-create at one path, or the lowest-free reuse of a directory's and a file's inode numbers, never compare equal
([80 §2.11.4] rule 8). Every id hit is still verified by [40 §4.3]'s rules (size and mtime, or `oid`), because MFT slots
are reused aggressively ([40 §2.6]).

### 3.3 `FsTime` (9 bytes)

| Offset | Width | Type | Name | Meaning |
|---|---|---|---|---|
| 0 | 8 | i64 | `ns` | nanoseconds since 1970-01-01T00:00:00Z; 0 when absent |
| 8 | 1 | u8 | `gran` | the nominal resolution of the field's source as a decimal exponent `e`: the source stores the time in units of at most 10^e ns, `e` = 0…10; `0xFF` = absent; 11…254 reserved (uninterpretable → absent) |
| total | 9 | | | |

| Source | `mtime`, `ctime` | `btime` | `added` | `gran` |
|---|---|---|---|---|
| Windows NTFS, ReFS | `LastWriteTime`, `ChangeTime` (`FILETIME` `f` → `(f − 116 444 736 000 000 000) × 100`) | `CreationTime` | absent | 2 (100 ns) |
| Windows FAT32 | as NTFS (`ChangeTime` absent) | `CreationTime` | absent | `mtime` 10 (2 s, rounded up to 10 s), `btime` 7 (10 ms) |
| Windows exFAT | as FAT32 | `CreationTime` | absent | 7 (10 ms) |
| Linux (port) | `statx` `stx_mtime`, `stx_ctime` | `stx_btime` when `STATX_BTIME` is in `stx_mask`, else absent | absent | 0 (1 ns) |
| macOS APFS (port) | `ATTR_CMN_MODTIME`, `ATTR_CMN_CHGTIME` | `ATTR_CMN_CRTIME` | `ATTR_CMN_ADDEDTIME` | 0 |
| macOS HFS+ (port) | as APFS | as APFS | as APFS | 9 (1 s) |

- A `FILETIME` of 0 is absent; a value outside the i64 nanosecond range after conversion is absent.
- `gran` is the **nominal** resolution. The **effective** resolution of a volume's timestamps is coarser on some systems
  (the Windows clock tick; Linux before 6.13, [80 §2.11.1]) and is recorded per volume in `VolumeCaps` (§4.4). The
  granularity G in nanoseconds that [F20 §5.1]'s `teq` and `tge` use is `max(10^gran, VolumeCaps.mtime_granularity_ns)`.
- The value type is [F01]'s `unix_ns` (i64 ns since the epoch) plus this byte ([F01 §5.7]).
- The Windows read-path stat (`GetFileAttributesExW`) returns no `ChangeTime`; a `Read`-mode stat reports it absent (§5.1).

### 3.4 `FileAttrs` (u32, normalised)

| Bit | Name | Windows source | Linux source | macOS source |
|---|---|---|---|---|
| 0 | `READONLY` | `FILE_ATTRIBUTE_READONLY` (0x1) | no write bit in `st_mode & 0o222` | as Linux |
| 1 | `HIDDEN` | `FILE_ATTRIBUTE_HIDDEN` (0x2) | 0 | `UF_HIDDEN` |
| 2 | `REPARSE_POINT` | `FILE_ATTRIBUTE_REPARSE_POINT` (0x400) | 0 | 0 |
| 3 | `RECALL_ON_OPEN` | `FILE_ATTRIBUTE_RECALL_ON_OPEN` (0x40000) | 0 | 0 |
| 4 | `RECALL_ON_DATA_ACCESS` | `FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS` (0x400000) | 0 | 0 |
| 5 | `OFFLINE` | `FILE_ATTRIBUTE_OFFLINE` (0x1000) | 0 | 0 |
| 6 | `DATALESS` | 0 | 0 | `SF_DATALESS` in `st_flags` |
| 7 | `PINNED` | `FILE_ATTRIBUTE_PINNED` (0x80000) | 0 | 0 |
| 8 | `UNPINNED` | `FILE_ATTRIBUTE_UNPINNED` (0x100000) | 0 | 0 |
| 9 | `CLOUD_REPARSE` | reparse tag `t` with `t & 0xFFFF0FFF = 0x9000001A` (the `IO_REPARSE_TAG_CLOUD` family) | 0 | 0 |
| 10 | `CLONE_MAY_SHARE` | 0 | 0 | `EF_MAY_SHARE_BLOCKS` in `ATTR_CMNEXT_EXT_FLAGS`, or `ATTR_CMNEXT_CLONE_REFCNT` ≠ 0 |
| 11–31 | reserved | zero | zero | zero |

**Cloud-only** means any of bits 3, 4, 5 or 6. `FILEOBS`'s "attributes (incl. the cloud bits of [40 §4.6])" is this u32
([F11]).

## 4. Volumes

### 4.1 `VolumeKey` and `volume()`

```
vol_key = BLAKE3-128( lp("moirai-vol-key-v1") ‖ lp(N) ‖ lp(S) )
```

The framing is [F01 §6.3, §7.3]'s (`lp()` per operand, a fixed domain prefix); BLAKE3-128 is the first 16 bytes of
BLAKE3-256 ([F01 §7.1]).

| OS | `N` (ASCII) | `S` |
|---|---|---|
| Windows | `win-volume-serial-64` | `u64le(FILE_ID_INFO.VolumeSerialNumber)` of any handle on the volume |
| Linux (port) | `linux-statfs-f_fsid` | `i32le(f_fsid.__val[0]) ‖ i32le(f_fsid.__val[1])` from `fstatfs` (one fixed-width 8-byte operand) |
| macOS (port) | `darwin-attr-vol-uuid` | the 16 bytes of `ATTR_VOL_UUID` |

`hgen` (§3.1) keeps [80 §2.11.2]'s own framing — `u32le(handle_type)` followed by the handle bytes, with no `lp()` — which
is unambiguous because its first operand has a fixed width ([F01 §7.3] allows an owning chapter's framing on that ground).

One fixed source per OS, so a kernel upgrade never switches the source ([80 §2.11.2], [81] m15). An XFS `f_fsid` derived
from the device number may change across boots: rows then read as absent, never wrong.

`volume(root) → (VolumeKey, VolumeCaps)` computes the capability record of the root's volume **once per volume per
command** ([80 §2.11.1]); the implementation caches it by `VolumeKey` for the life of the process's current command.

### 4.2 `VolumeCaps`: the value and its 16-byte snapshot

```rust
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct VolumeCaps {
    pub id_kind: u8,                    // 0–4, as OsFileId.kind
    pub id_locate: IdLocate,            // None | ById | Frontier
    pub journal: JournalKind,           // None | Usn | FsEvents  (availability only; E2 is not built)
    pub btime: BtimeTrust,              // Absent | TunneledNotCopied | Unforgeable | CopiedByClones
    pub ctime_on_rename: Option<bool>,  // None = unverified
    pub case_rule: CaseRule,            // Sensitive | PerDirFlag | Volume
    pub case_insensitive_default: bool, // PerDirFlag: a directory without the flag; Volume: the volume
    pub norm_insensitive_always: bool,  // APFS, HFS+
    pub norm_follows_case: bool,        // Linux casefold: normalization-insensitive exactly where case-insensitive
    pub cloud: CloudRule,               // None | RecallAttrs | Dataless
    pub rename_noreplace: RenameRule,   // Unsupported | Native | LinkUnlinkFiles
    pub clone_indicators: bool,         // candidates carry clone indicators (§3.4 bit 10)
    pub ids_persistent: bool,           // ids survive unmount and reboot (tmpfs: false)
    pub docids: bool,                   // macOS document ids in use (#21 (d))
    pub mtime_granularity_ns: u64,      // effective, measured (§4.4); 0 = not measured, use the nominal
    pub dir_flush_doubtful: bool,       // the file-system class may refuse a directory flush (§4.3, §6.2)
}
```

`dir_flush_doubtful` (pass 1, P1-16) is set from the file-system class, never by a probe: on the rows of §4.3 whose
volumes are known to refuse or fake a directory flush (a Windows network redirector, `\\wsl$` and other non-local
volumes; Linux NFS, CIFS, FUSE and 9p). It only lets `doctor` and `links` hints warn in advance; the plan step's own
`sync_dir` decides (§6.2).

The snapshot stored in `TREES` ([80 §2.11.2], [F11]):

| Offset | Width | Type | Name | Meaning |
|---|---|---|---|---|
| 0 | 4 | u32 | `flags` | bit 0 `case_insensitive_default`; bit 1 `norm_insensitive_always`; bit 2 `norm_follows_case`; bit 3 `ctime_on_rename` known; bit 4 `ctime_on_rename` value (0 when bit 3 is 0); bits 5–6 `id_locate` (0 none, 1 by-id, 2 frontier, 3 reserved); bits 7–8 `journal` (0 none, 1 usn, 2 fsevents, 3 reserved); bits 9–10 `rename_noreplace` (0 unsupported, 1 native, 2 link-unlink for files, 3 reserved); bit 11 `clone_indicators`; bit 12 `ids_persistent`; bit 13 `docids`; bit 14 `dir_flush_doubtful`; bits 15–31 reserved, zero |
| 4 | 1 | u8 | `id_kind` | 0–4 as `OsFileId.kind`; 5–255 reserved |
| 5 | 1 | u8 | `btime` | 0 absent, 1 tunneled-not-copied, 2 unforgeable, 3 copied-by-clones; 4–255 reserved |
| 6 | 1 | u8 | `case_rule` | 0 sensitive (then bit 0 of `flags` is 0), 1 per-directory flag, 2 volume; 3–255 reserved |
| 7 | 1 | u8 | `cloud` | 0 none, 1 recall attributes, 2 dataless; 3–255 reserved |
| 8 | 8 | u64 | `mtime_granularity_ns` | effective granularity in ns; 0 = not measured |
| total | 16 | | | |

A snapshot with a reserved bit or value set is uninterpretable, and its `TREES` row behaves like a first settle ([80] X1).
`VolumeCaps` converts to and from the snapshot losslessly.

A `VolumeCaps` with `case_rule = Sensitive` and `case_insensitive_default = true` is **invalid**: no implementation
constructs one, and a snapshot that carries it (`case_rule` 0 with bit 0 of `flags` set) is uninterpretable, as the
`case_rule` row states.

`DirEquivalence { case_insensitive: bool, norm_insensitive: bool }` is the equivalence one directory observes (§4.5).

### 4.3 Capability values per file system

| Volume | `id_kind` | `id_locate` | `journal` | `btime` | `ctime_on_rename` | `case_rule` (default) | norm | `cloud` | `rename_noreplace` | other flags | nominal mtime |
|---|---|---|---|---|---|---|---|---|---|---|---|
| Windows NTFS | `ntfs128` | by-id | `usn` if `FILE_SUPPORTS_USN_JOURNAL` and `FSCTL_QUERY_USN_JOURNAL` on a root handle succeeds, else none | HOLE(F20-btime-ntfs), draft tunneled-not-copied | known; value HOLE(F20-ctime-rename), draft true (S-16) | per-dir flag (insensitive) | none | recall attributes | native | `ids_persistent` | 100 ns |
| Windows ReFS | `refs128` | by-id | as NTFS | **absent** until verified (open point 8) | unknown | per-dir flag (insensitive) | none | recall attributes | native | `ids_persistent` | 100 ns |
| Windows FAT32, exFAT | none | none | none | absent | unknown | volume (insensitive) | none | none | native | — | 2 s / 10 ms |
| Windows, any other or a network redirector | none | none | none | absent | unknown | volume (insensitive) | none | none | native | `dir_flush_doubtful` | 100 ns |
| Linux ext4, XFS, btrfs, f2fs, bcachefs, ZFS (port) | `linux_ino` iff an unprivileged `name_to_handle_at` on the root succeeds, else none | frontier (none without ids) | none | unforgeable iff `statx` on the root reports `STATX_BTIME`, else absent | known, true on ext4, btrfs, XFS; unknown elsewhere | per-dir flag (sensitive) on ext4 and f2fs; sensitive elsewhere | follows case where per-dir | none | native iff `renameat2(RENAME_NOREPLACE)` works (probed on the store's `tmp/`, or assumed from the kernel floor for ext4 ≥ 3.15, XFS ≥ 4.0), else link-unlink | `ids_persistent` | 1 ns |
| Linux tmpfs (port) | as above | frontier | none | as above | unknown | sensitive | none | none | native | not persistent | 1 ns |
| Linux vfat, exFAT, overlayfs (unverified `xino`), NFS, CIFS, FUSE, 9p (port) | none | none | none | absent | unknown | volume (insensitive) for vfat/exFAT, sensitive otherwise | none | none | native or link-unlink | `dir_flush_doubtful` for NFS, CIFS, FUSE, 9p | FS-specific |
| macOS APFS (port) | `darwin_fileid` iff `VOL_CAP_FMT_PERSISTENTOBJECTIDS` | by-id iff `VOL_CAP_FMT_PATH_FROM_ID`, else none | `fsevents` | copied-by-clones | unknown (probe) | volume (insensitive unless `VOL_CAP_FMT_CASE_SENSITIVE`) | always | dataless | native iff `VOL_CAP_INT_RENAME_EXCL`, else unsupported | `clone_indicators`, `ids_persistent`; `docids` iff #21 (d) | 1 ns |
| macOS HFS+ (port) | as APFS | as APFS | `fsevents` | copied-by-clones | unknown | volume | always | dataless | as APFS | `ids_persistent` | 1 s |

A missing capability turns a source off and never changes a rule ([80 §2.11.1], X5).

### 4.4 The effective mtime granularity

`measure_mtime_granularity(stamp_dir)` measures the effective timestamp resolution of the volume that holds `stamp_dir`
(`<store>/tmp/`, the only place moirai may write for the purpose) ([80 §2.11.1], [81] m7):

1. Repeatedly call `touch_stamp` on `stamp_dir/settle.stamp` (§5.9), each call immediately after the previous one returns,
   recording each returned `mtime.ns`, until HOLE(OS-pfs-gran-probe-k) distinct values have been seen or
   HOLE(OS-pfs-gran-probe-budget) of mono time has passed.
2. If at least two distinct values were seen, the result is the smallest positive difference between consecutive distinct
   values, rounded up to a multiple of the source's nominal resolution (§3.3); otherwise it is the elapsed mono time of the
   probe, rounded up likewise (a safe over-estimate).
3. The result is recorded in the `TREES` rows of the trees whose `VolumeKey` equals `stamp_dir`'s. A tree on another
   volume records 0 (not measured): the resolver uses the nominal resolution, and the frontier's racy threshold on that
   volume uses the newest directory mtime of the previous scan ([80 §2.11.3]).

The probe runs once per volume per store, at a settle outside hook and MCP-server paths (it would otherwise break their
caps, A1P-06); until it runs, `mtime_granularity_ns` is 0.

### 4.5 `case_equivalent`

| OS | Source | `case_insensitive` | `norm_insensitive` |
|---|---|---|---|
| Windows | `GetFileInformationByHandleEx(dir, FileCaseSensitiveInfo)`; `ERROR_INVALID_PARAMETER` (the file system has no per-directory flag) → the volume rule | `!(Flags & FILE_CS_FLAG_CASE_SENSITIVE_DIR)`, or the volume rule | false ([M], NTFS is normalization-sensitive) |
| Linux (port) | `ioctl(FS_IOC_GETFLAGS)` on the directory | `flags & FS_CASEFOLD_FL ≠ 0` | equal to `case_insensitive` |
| macOS (port) | the volume's `VOL_CAP_FMT_CASE_SENSITIVE` (`getattrlist(ATTR_VOL_CAPABILITIES)`) | `!case_sensitive` | true |

The equivalence is observed at resolve time and never baked into keys ([80 §2.10] P6).

## 5. Read side

### 5.1 `stat`

```rust
pub enum StatMode {
    /// The read path: the cheapest call; an id only where it comes with the call.
    Read,
    /// With the object's id wherever the volume has ids.
    WithId,
}
pub enum Stat { Present(StatRec), Absent }
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ProjKind { File, Dir, Symlink, Other }
pub struct StatRec {
    pub kind: ProjKind,
    pub size: u64,
    pub mtime: FsTime, pub ctime: FsTime, pub btime: FsTime, pub added: FsTime,
    pub attrs: FileAttrs,
    pub reparse_tag: u32,          // Windows reparse tag; 0 elsewhere or when not a reparse point
    pub id: Option<OsFileId>,      // `parent` filled when known
    pub nlink: u32,                // 0 = unknown
}
```

`lstat` semantics everywhere (P8): a symlink is reported as itself. `Absent` covers "no such entry" and "a parent is not a
directory"; every other failure is an error (and a denial is `AccessDenied`, §2.3).

| Mode | Windows (built) | Linux (port) | macOS (port) |
|---|---|---|---|
| `Read` | `GetFileAttributesExW(GetFileExInfoStandard)`: kind, size, `LastWriteTime`, `CreationTime`, attributes; `ctime` absent; `id` `None`; if `REPARSE_POINT` is set, one `FindFirstFileExW(FindExInfoBasic)` on the exact name for the reparse tag (`dwReserved0`), which decides `Symlink` (`IO_REPARSE_TAG_SYMLINK`), cloud (§3.4 bit 9) or `Other` | `statx(root_fd, rel, AT_SYMLINK_NOFOLLOW, STATX_BASIC_STATS \| STATX_BTIME)`; when the volume's `id_kind` is `linux_ino`, one `name_to_handle_at` for `hgen` ([80 §2.11.1] "one `name_to_handle_at` for a linked file whose identity a read relies on") | `getattrlistat(root_fd, rel, …, FSOPT_NOFOLLOW)` with `ATTR_CMN_OBJTYPE`, `FILEID`, `PARENTID`, times, `FLAGS`, `DATALENGTH`: the id comes free |
| `WithId` | `CreateFileW(FILE_READ_ATTRIBUTES, full sharing, OPEN_EXISTING, FILE_FLAG_BACKUP_SEMANTICS \| FILE_FLAG_OPEN_REPARSE_POINT)`, then `GetFileInformationByHandleEx` `FileBasicInfo` (times incl. `ChangeTime`, attributes), `FileStandardInfo` (size, `NumberOfLinks`), `FileIdInfo`, `FileAttributeTagInfo` (reparse tag); the parent's `FileIdInfo` through one open of the parent directory. **Never used on an entry whose last known attributes are cloud-only** (§5.10): its id comes from its parent's enumeration | as `Read` | as `Read` |

On Windows a read therefore sees no file id, so a swap of two files with equal size and mtime is invisible to a read until
the next settle; on Linux and macOS a read sees the id ([40 §2.6], [80 §2.11.4] rule 3).

### 5.2 `enumerate`

```rust
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct ProjEntry<'a> {
    pub name: EntryNameRef<'a>,      // [OS/path §2.4]; borrowed from the enumeration's buffer
    pub kind: ProjKind,
    pub stat: Option<StatRec>,       // Some where the enumeration call returns the attributes (Windows, macOS)
    pub ino_hint: Option<u64>,       // Linux d_ino, for the frontier; None elsewhere
}
pub enum EnumEnd { Complete, Stopped }
```

- **An entry is borrowed.** `ProjEntry<'a>` and its `EntryNameRef<'a>` live only for one call of `visit`; a caller that
  keeps a name takes `entry.name.to_owned()` (an [OS/path §2.4] `EntryName`). A tree scan therefore allocates nothing per
  entry: the implementation converts each name into one buffer reused for the whole enumeration.
- **The directory itself is never a link.** `enumerate` first reads the attributes of `dir` without opening it; if `dir` is
  a symbolic link it fails with `IsSymlink`, and if it is another non-cloud reparse point (a junction, a WSL link) with
  `Other`, so a walk never lists a link's target outside the tree ([40 §2.4]). Only then does it open the directory,
  without `FILE_FLAG_OPEN_REPARSE_POINT`; a cloud directory placeholder counts as a directory (and one marked
  `RECALL_ON_DATA_ACCESS` is refused below). Linux and macOS get the same refusal from the relative open's
  `RESOLVE_NO_SYMLINKS` and `O_NOFOLLOW_ANY` (`ELOOP` → `IsSymlink`).
- **Containment after the open (Windows).** The attribute read and the open are two calls, so a directory that an
  external actor replaces with a junction between them would be followed. Before the first listing call, `enumerate`
  therefore checks the open handle as `read_for_hash` does (§5.5 step 3): `GetFinalPathNameByHandleW` of the handle,
  rewritten as [OS/path §4.1], must equal the root's text (the root itself) or have the root's text followed by `/` as
  an exact byte prefix, never compared ignoring case. Otherwise the handle is closed unread and the result is
  `OutsideRoot`. The cost is one `GetFinalPathNameByHandleW` per directory. Linux and macOS need no check: the open
  itself resolves beneath the root without following links.
- Entries `.` and `..` are never reported. Order is the file system's; **every consumer sorts candidates by exact name
  bytes before any tie-break** ([80 §2.11.4] rule 4).
- `visit` returning `Break` stops the enumeration (`EnumEnd::Stopped`); the handle is closed before `enumerate` returns.
- A directory whose attributes are `RECALL_ON_DATA_ACCESS` is never enumerated: `enumerate` returns `CloudOnly` without
  opening it (§5.10).
- Walks built on `enumerate` never descend into an entry of kind `Symlink` or `Other` ([40 §2.4]).

| | Windows (built) | Linux (port) | macOS (port) |
|---|---|---|---|
| Call | open the directory (`FILE_LIST_DIRECTORY \| FILE_READ_ATTRIBUTES`, full sharing, `FILE_FLAG_BACKUP_SEMANTICS`); `GetFileInformationByHandleEx(FileIdInfo)` once for the directory's own id and the volume serial; then `FileIdExtdDirectoryRestartInfo` and `FileIdExtdDirectoryInfo` into a 64 KiB 8-byte-aligned buffer until `ERROR_NO_MORE_FILES` | `getdents64` on `openat(root_fd, rel, O_RDONLY \| O_DIRECTORY \| O_CLOEXEC)` into a 32 KiB buffer; `stat` is `None`; the caller `statx`es and `name_to_handle_at`s the entries it needs ([80 §2.11.1] "Settle enumeration") | `getattrlistbulk` with name, type, file id, parent id, size, mtime, ctime, crtime, added time, flags, extended flags and clone refcount |
| Per entry | `FileId` (the entry's `FILE_ID_128`), the four times, `EndOfFile`, `FileAttributes`, `ReparsePointTag`, `FileName` (UTF-16); `parent` = the directory's id; `nlink` 0 | name bytes, `d_type`, `d_ino` | all fields of `StatRec`, id with parent |
| Measured cost | 12–22 µs per entry warm ([M, 09 §8]) | — | — |

### 5.3 What `stat` and `enumerate` report under an insensitive equivalence

On a case- or normalization-insensitive directory, `stat` of a stored spelling succeeds when an entry equal to it under
the directory's equivalence exists, whatever its on-disk spelling. The on-disk spelling is visible through `enumerate`
or, for one path, through `disk_spelling`:

```rust
/// The on-disk spelling of every component of `at`, as the directories hold them.
fn disk_spelling(&self, at: At<'_, Self::Root>) -> Result<RelPathBuf, VfsError>;   // a `ProjectFs` method (§2.1)
```

| OS | `disk_spelling` |
|---|---|
| Windows (built) | open with `FILE_READ_ATTRIBUTES`, full sharing, `FILE_FLAG_BACKUP_SEMANTICS \| FILE_FLAG_OPEN_REPARSE_POINT` (an attribute-only open; refused with `CloudOnly` for a cloud-only entry, §5.10); `GetFinalPathNameByHandleW(FILE_NAME_NORMALIZED \| VOLUME_NAME_DOS)`, rewritten as [OS/path §4.1] step 4; the part after the root's text + `/` |
| Linux (port) | per component whose parent has `FS_CASEFOLD_FL`: the entry name `getdents64` returns for the component's inode (as [OS/path §4.2] step 2); other components as given |
| macOS (port) | per component, `getattrlist(ATTR_CMN_NAME)` on the path prefix |

The OS layer reports these facts as they are and decides nothing: the twin rule, "spelling differs on disk" and the
normalization rule ([80 §2.11.4] rule 2, [40 §2.4]) are [F20 §3.5, §3.6]'s. [F20 §3.5] reads the ids of a twin group's
members through `stat(WithId)`, the Windows attribute-only open of §5.1.

### 5.4 `locate_id` and `file_handle_digest`

```rust
pub enum Located {
    InRoot(RelPathBuf),   // the object is under the root passed in
    InTrash(AbsPath),     // under one of trash_dirs() (§5.7)
    Elsewhere(AbsPath),   // on the volume, outside the root and the trash
    Gone,                 // no object has this id on its volume
    NotLocatable,         // this volume has no by-id lookup (id_locate ≠ ById), the id is on another volume, or
                          // `recorded` shows the object was cloud-only (never opened)
}
```

| OS | `locate_id(root, id, recorded)` |
|---|---|
| Windows (built) | requires `id.kind ∈ {1, 2}` and `id.vol_key` = the root's volume key, and `recorded` not cloud-only; else `NotLocatable`. Open the root directory (`FILE_READ_ATTRIBUTES`, full sharing, `FILE_FLAG_BACKUP_SEMANTICS`) as the volume hint; `OpenFileById(hint, {dwSize, ExtendedFileIdType, id.id}, FILE_READ_ATTRIBUTES, full sharing, NULL, FILE_FLAG_BACKUP_SEMANTICS \| FILE_FLAG_OPEN_REPARSE_POINT)`; `GetFinalPathNameByHandleW(FILE_NAME_NORMALIZED \| VOLUME_NAME_DOS)`, rewritten as [OS/path §4.1] step 4; classify by prefix: under the root text + `/` → `InRoot`, under `<drive>:/$Recycle.Bin/` → `InTrash`, else `Elsewhere`. `ERROR_INVALID_PARAMETER` (87) or `ERROR_FILE_NOT_FOUND` → `Gone`; `ERROR_ACCESS_DENIED` → `AccessDenied`. Cost 0.24–0.57 ms per file, 0.17–0.40 ms per directory ([M, 09 §8], [40 §0.3]) |
| Linux (port) | always `NotLocatable`: `open_by_handle_at` needs `CAP_DAC_READ_SEARCH`; the resolver runs the changed-directory frontier ([80 §2.11.3]) over `DIRMAP` with `enumerate`, `stat` and `file_handle_digest` |
| macOS (port) | requires `id.kind = 4` and the volume's `id_locate = ById`; `fsgetpath(buf, len, fsid, fileid)`; classify as Windows with `~/.Trash` and `<volume>/.Trashes/<uid>` as trash; `ENOENT` → `Gone`; `EPERM`/`EACCES` (a MAC hook, TCC) → `AccessDenied` |

`file_handle_digest(at)` returns the Linux `hgen` of §3.1 (one `name_to_handle_at(root_fd, rel, …, AT_SYMLINK_NOFOLLOW)`,
buffer `MAX_HANDLE_SZ` = 128 bytes); `EOPNOTSUPP` → `Ok(None)`. It is used to confirm a frontier hit before the hit
counts ([80 §2.11.3] step 4). On Windows and macOS it returns `Ok(None)`: their ids need no digest.

### 5.5 `read_for_hash` and `ProjectRead`

```rust
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct ReadOpts { pub allow_hydrate: bool }   // true only for explicit verbs given `--allow-hydrate`
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct ReadSnapshot { pub size: u64, pub mtime: FsTime }
```

`read_for_hash(at, opts)` opens one project file for FL-1's streaming reader ([40 §2.5]; the reader consumes this byte
source and opens nothing itself, A1P-10):

1. **Placeholder gate.** Read the entry's attributes without opening it (Windows `GetFileAttributesExW`; macOS the
   `st_flags` of an `fstatat`); if they are cloud-only and `!opts.allow_hydrate`, return `CloudOnly` (§5.10). A directory
   is `IsDirectory`; a symlink is `IsSymlink` (use `read_link`).
2. **Open**, never following a link, with no access-time update:
   - Windows: `CreateFileW(GENERIC_READ, FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE, OPEN_EXISTING,
     FILE_FLAG_SEQUENTIAL_SCAN | FILE_FLAG_OPEN_REPARSE_POINT)` (NTFS access-time updates are system-managed; no per-handle
     option is used);
   - Linux: `openat2(root_fd, rel, O_RDONLY | O_CLOEXEC | O_NOFOLLOW | O_NOATIME, RESOLVE_BENEATH |
     RESOLVE_NO_SYMLINKS)`; `EPERM` (the process does not own the file) → the same without `O_NOATIME`;
     `posix_fadvise(SEQUENTIAL)`;
   - macOS: `openat(root_fd, rel, O_RDONLY | O_CLOEXEC | O_NOFOLLOW_ANY)`; the process policies
     `IOPOL_TYPE_VFS_ATIME_UPDATES = IOPOL_ATIME_UPDATES_OFF` and `IOPOL_TYPE_VFS_MATERIALIZE_DATALESS_FILES =
     IOPOL_MATERIALIZE_DATALESS_FILES_OFF` are set once per process at the first `read_for_hash`; `fstat` re-checks
     `SF_DATALESS` on the descriptor.
3. **Containment.** Windows: `GetFinalPathNameByHandleW` of the handle, rewritten as [OS/path §4.1], must lie under the
   root's text: the root's text followed by `/` must be an **exact byte prefix** of the final path, never compared
   ignoring case (under per-directory case sensitivity a junction to a sibling whose name differs from the root's only in
   case would otherwise pass). Otherwise the handle is closed and the result is `OutsideRoot` (a junction or directory
   symlink on the path, [40 §2.4]). Linux and macOS get this from `RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS` and
   `O_NOFOLLOW_ANY`.
   **Placeholder re-check through the handle** (pass 1, P1-38): before the first read, Windows reads
   `GetFileInformationByHandleEx(FileAttributeTagInfo)` on the open handle; if the entry became cloud-only between step 1
   and the open (a file replaced by a placeholder) and `!opts.allow_hydrate`, the handle is closed unread and the result is
   `CloudOnly`. Opening a placeholder for attributes hydrates nothing; only a data read does. macOS re-checks
   `SF_DATALESS` on the descriptor as step 2 states.
4. The returned `Reader` reads sequentially (`ReadFile`, `read`); `rewind` returns to offset 0 on the **same handle**;
   `snapshot` reads size and last-write time **through the handle** (Windows `GetFileInformationByHandleEx`
   `FileStandardInfo` and `FileBasicInfo`; Unix `fstat`), and `identity` reads the object's `OsFileId` through it.
5. Dropping the `Reader` closes the handle.

**The two-pass rule** (A1P-05; the rule itself is [F20 §2.4]'s): FL-1's reader takes `snapshot()` before pass 1, runs
both passes on one `Reader` (`rewind` between them), and takes `snapshot()` again after pass 2. The content is stable
only if all of these agree: the two snapshot sizes; the raw lengths n1 and n2 of the two passes; the two last-write
times; the raw-content hashes `r1 = r2`; and the N1 normalised bytes pass 1 counted with the bytes pass 2 emitted.
Otherwise it retries once from the start and then reports `Unavailable(unstable)`. A replace-by-rename writer cannot
affect an open handle; an in-place writer is caught by the snapshots and the hashes.

### 5.6 `read_link` (P8)

`read_link(at, out)` appends a symlink's target text to `out`, as git would store it in the symlink's blob:

| OS | Target text |
|---|---|
| Windows (built) | open with `FILE_FLAG_OPEN_REPARSE_POINT \| FILE_FLAG_BACKUP_SEMANTICS`; `DeviceIoControl(FSCTL_GET_REPARSE_POINT)`; the tag must be `IO_REPARSE_TAG_SYMLINK` (else `Other`: not a symlink for moirai); take `PrintName`, or `SubstituteName` without a leading `\??\` when `PrintName` is empty; replace every `\` by `/`; UTF-16 → UTF-8 (WTF-8 bytes if not valid). Junctions (`IO_REPARSE_TAG_MOUNT_POINT`), WSL links and app-execution aliases are kind `Other`, never read, never candidates (open point 11) |
| Linux, macOS (port) | `readlinkat(root_fd, rel)`: the raw bytes |

With `core.symlinks = false` git checks a symlink out as a text file holding the same bytes, so hashing that file gives
the same `oid` ([80 §2.10] P8); `read_for_hash` then applies as for any file.

### 5.7 `trash_dirs`

The OS trash locations relevant to a root, as `AbsPath`s. A located or enumerated path under one of them is "moved to the
trash": `missing (in Recycle Bin)` and never re-bound ([40 §4.3] step 5, [80 §2.11.4] rule 5).

| OS | Locations |
|---|---|
| Windows | `<drive>:/$Recycle.Bin` of the root's volume (none for a UNC root) |
| Linux (port) | `$XDG_DATA_HOME/Trash` (default `$HOME/.local/share/Trash`); `<top>/.Trash/<uid>` and `<top>/.Trash-<uid>`, `<top>` being the mount point of the root's file system (from `/proc/self/mountinfo`) |
| macOS (port) | `$HOME/.Trash`; `<volume root>/.Trashes/<uid>` |

### 5.8 `busy_holders`

Diagnostics for a failed rename or delete of a **file** ([40 §3.4]): Windows — Restart Manager (`RmStartSession`,
`RmRegisterResources` with the one path, `RmGetList`, `RmEndSession`), each `RM_PROCESS_INFO` giving `Holder { pid, name }`
(`strAppName`, ASCII-escaped by the renderer); a directory → `Unsupported` (`RmGetList` refuses directories; the message
then names the likely classes: a shell whose current directory is inside, an open file, a watcher). Linux and macOS →
`Ok(vec![])`: there are no sharing violations on Unix ([80 §2.11.4] rule 7).

```rust
pub struct Holder { pub pid: u32, pub name: Box<str> }
```

### 5.9 `touch_stamp`

`touch_stamp(at)` writes the single byte `00` at offset 0 of the file `at` ([F20 §5.12.1]), creating it with length 1 if
absent (Windows `CreateFileW(GENERIC_WRITE, full sharing, OPEN_ALWAYS)`; Unix `openat(O_WRONLY | O_CREAT | O_CLOEXEC,
0o644)`), closes it, and returns its `mtime` from a `Read` stat. It is used only on `<store>/tmp/settle.stamp`: at the
start of a settle, to take the frontier's racy threshold from a file-system timestamp ([80 §2.11.3], [F20 §5.12.1]), and
by §4.4. It is lazy: no flush, and nothing depends on the stamp surviving a crash. It is the one store file `ProjectFs`
writes, because only `ProjectFs` reads file-system timestamps; [F02 §5.3] lists `settle.stamp` as the one fixed `tmp/`
name, which the orphan sweep may remove (open point 18; pass 1, P1-32, S1-38).

### 5.10 Cloud placeholders are never hydrated

([40 §4.6], [41 M8], [80 §2.11.1] `cloud` row; I-F11.)

1. Attributes come from enumeration records or from calls that read no content (`GetFileAttributesExW`, `fstatat`).
2. No automatic path (reads, settles, hooks) opens the content of a cloud-only entry (§3.4), and no path enumerates a
   directory marked `RECALL_ON_DATA_ACCESS`; answers that need content are `unverified (cloud-only)`.
3. On Windows no path **opens** an entry marked `RECALL_ON_OPEN` or `OFFLINE`, even for attributes: its id and times come
   from its parent's enumeration; `stat(WithId)` and `locate_id` refuse such entries (§5.1, §5.4).
4. Explicit verbs (`link`, `links check --deep`) read a placeholder's content only with `--allow-hydrate`
   (`ReadOpts::allow_hydrate`).
5. macOS additionally sets `IOPOL_MATERIALIZE_DATALESS_FILES_OFF` for the process, so a missed check fails instead of
   materialising.
6. Measurement 15 checks on the owner's OneDrive folder that rules 1–3 leave a placeholder un-hydrated (§10).

### 5.11 Denials are never absence

Every method maps a permission failure to `AccessDenied` (§2.3), which the resolver turns into `Unknown` or "source absent"
([80 §2.11.4] rule 9): `ERROR_ACCESS_DENIED` from `OpenFileById` or an enumeration, `EPERM`/`EACCES` from `fsgetpath`, a
TCC-protected folder's enumeration, `setiopolicy_np`. No method reports `Absent`, `Gone` or `NotFound` for a denial.

## 6. Write side

The write side shares its per-OS calls, flags, error mapping and retry bound with [OS/fs §4.7, §4.8, §4.4.3, §6.3]
([OS/README §4.2]); this section states what differs for project files.

### 6.1 `rename_noreplace`

```rust
pub enum Renamed { Renamed, LinkedThenUnlinked }   // the second: Linux's file fallback
```

| OS | Call |
|---|---|
| Windows (built) | `MoveFileExW(from, to, MOVEFILE_WRITE_THROUGH)` with `\\?\` paths: never `MOVEFILE_REPLACE_EXISTING`, never `MOVEFILE_COPY_ALLOWED`. `MOVEFILE_WRITE_THROUGH` stays until the post-release rig calibration shows it unnecessary; it never replaces the directory flush (owner decision of 2026-09-27, [80 §2.3.2]). Errors 5 and 32 are retried within `retry` ([OS/fs §6.3]; `file mv` passes its `--retry-ms`, default 1 s), and the last error (`AccessDenied` or `SharingViolation`) is returned after the bound; 17 → `CrossDevice`; 80/183 → `AlreadyExists`. Unlike a store rename ([OS/fs §4.8]), a directory may be renamed |
| Linux (port) | `renameat2(from_fd, from, to_fd, to, RENAME_NOREPLACE)`; `EINVAL` (unsupported): a file moves by `linkat` + `unlinkat` (`LinkedThenUnlinked`; the crash state "both names, one inode" is an `FsIntent` recovery state, [40 §3.4]; the store never uses this fallback, [OS/fs §4.8]), a directory is `Unsupported`; `EXDEV` → `CrossDevice` |
| macOS (port) | `renameatx_np(from_fd, from, to_fd, to, RENAME_EXCL)` iff the volume has `VOL_CAP_INT_RENAME_EXCL`, else `Unsupported` (`file mv` exits 7, [81] m2); `EXDEV` → `CrossDevice` |

`from` and `to` may be under different roots (a tree and `<store>/trash/<intent>`); they must be on one volume.

### 6.2 `sync_dir` (`durable-name` for a project directory)

| OS | Call |
|---|---|
| Windows (built) | `CreateFileW(dir, GENERIC_READ \| GENERIC_WRITE, full sharing, OPEN_EXISTING, FILE_FLAG_BACKUP_SEMANTICS)`, `FlushFileBuffers`, `CloseHandle` (0.072 ms p50 [M, X17 §3.4]); the handle of [OS/fs §4.4.3] and §5.1, opened per call because a project root holds no handle (§2.2) |
| Linux (port) | `fsync(openat(root_fd, dir, O_RDONLY \| O_DIRECTORY \| O_CLOEXEC))` |
| macOS (port) | `fsync(dirfd)`, then `fcntl(dirfd, F_FULLFSYNC)` (or folded into a `sync_group`'s last `F_FULLFSYNC`, §6.3) |

`sync_dir` is a non-lazy durability class: any error is a `DurabilityFailure` whose `kind` ([OS/fs §6.1]) names the
cause; it is never retried on the same handle and never downgraded ([80 §2.3.1]). It is idempotent, which the recovery
re-barrier relies on (§6.5). The caller hands the failure to `fail_stop` ([OS/fs §4.4.5]), with two exceptions where no
namespace step depends on it yet or where the process runs it for another process (pass 1, P1-16):

- **The plan step** of `file mv`, `file rm` and `file revert` calls `sync_dir` on every parent it will touch before the
  `FsIntent` group ([API §12.4] step 1). A failure of kind `Unsupported` or `AccessDenied` — an SMB share, some FUSE
  mounts, `\\wsl$`, where flushing a directory handle fails or opening a directory for `GENERIC_WRITE` is denied although
  the rename would be allowed — refuses the verb with exit 7 `no_dir_flush` and nothing changed ([F19 §10.2]); any other
  kind is `fail_stop`.
- **Intent recovery's re-barrier** (§6.5) on such a volume leaves the intent open and reports it through `doctor`
  ([F16] P-71), instead of failing the process that runs recovery.

Without the plan-step call, such a volume would fail the barrier after the rename and fail-stop, and every later
recovery would fail the same way.

### 6.3 Composite and plain operations

| Operation | Sequence | Result on failure |
|---|---|---|
| `durable_rename(from, to, retry)` | `rename_noreplace`, then `sync_dir` of `from`'s parent and of `to`'s parent (once if they are one directory: the two parent paths are byte-identical, or their `FileIdInfo` identities are equal; never a case-folded comparison, since under per-directory case sensitivity `Src` and `src` are two directories, FM-2.3). macOS: `fsync` of both directory descriptors, then **one** `F_FULLFSYNC` on the second (`sync_group`, [80 §2.3.1]). `LinkedThenUnlinked`: the same two flushes | `NotDone(e)` if the rename failed (nothing changed); `NotDurable(f)` if it succeeded and a flush failed |
| `unlink(at, retry)` | Windows: as [OS/fs §4.7] — if `FILE_ATTRIBUTE_READONLY` is set, clear it (`SetFileAttributesW`), then `DeleteFileW`; if the delete then fails, set the attribute back, so a failed `file rm` leaves the user's file unchanged (open point 9). Unix `unlinkat(root_fd, rel, 0)`, which ignores the file's mode, so both OSes delete a read-only file. **A directory link** (a junction or a directory symbolic link: a directory entry that is a non-cloud reparse point) is removed as a link, never its target: Windows `RemoveDirectoryW` (`DeleteFileW` refuses directory links); Unix `unlinkat(root_fd, rel, 0)` without `AT_REMOVEDIR`, since a symlink is not a directory there. A real directory is `IsDirectory`; a cloud directory placeholder counts as a directory | `VfsError` |
| `remove_dir(at, retry)` | Windows `RemoveDirectoryW`; Unix `unlinkat(…, AT_REMOVEDIR)`; the directory must be empty (`NotEmpty` otherwise) | `VfsError` |
| `durable_unlink(at, retry)` | `unlink` (or, for an empty directory, `remove_dir`), then `sync_dir` of the parent | `NotDone` / `NotDurable` as for `durable_rename` |

A `file rm --recursive` removes files and emptied directories bottom-up with the plain operations and then calls `sync_dir`
once on every directory that lost an entry before its commit (fault-model item (2): each unsynced unlink may be lost
independently until its parent is flushed).

### 6.4 Cross-volume moves (A1P-01)

`ProjectFs` has no copy operation. A `file mv` of a file or a directory whose `rename_noreplace` reports `CrossDevice`
(Windows 17, Unix `EXDEV`: another volume, a bind mount, a btrfs subvolume, an overlay lower directory), or whose source
and destination `VolumeKey`s differ in the plan step, is **refused** with exit 7 `cross_volume` and the fix line "move it
with a raw mv; links re-bind by evidence or show a proposal to confirm" ([F19 §10.2] owns the text; pass 1, A1-59: a raw
cross-volume `mv` loses the file id, so an identical copy usually lands as a proposal). This adopts A1P-01's preferred fix: the alternative copy path had no
namespace ordering and could lose the user's only copy on power loss. It supersedes the copy branch of [40 §3.4] step 3
and the `EXDEV` copy clause of [80 §2.11.4] rule 6 for the explicit verb; a raw cross-volume move is still resolved lazily
by evidence (open point 1). `file rm --trash` with the store on another volume was already refused ([40 §3.5]).

### 6.5 The protocol points served ([80 §2.3.2], X-F5)

| Protocol point | `ProjectFs` calls, in order, all before the commit that records the change |
|---|---|
| `file mv` (same volume) | after the durable `FsIntent` group: `durable_rename(src, dst)` — on Windows `MoveFileExW(…, MOVEFILE_WRITE_THROUGH)` + `FlushFileBuffers` on both parents |
| `file rm` | after the `FsIntent`: `durable_unlink(path)` (or the recursive form of §6.3) |
| `file rm --trash` | after the `FsIntent`: the store side creates `<store>/trash/<intent>/` and flushes `<store>/trash/` ([OS/fs §4.2] `create_dir`, [OS/fs §4.4.3] `sync_dir`); then, for item i of the intent, `durable_rename(src, <store>/trash/<intent>/<i>)` ([F02 §5.4] names), whose two flushes are the source's parent and `<store>/trash/<intent>/` |
| intent recovery, roll forward (A1P-02) | before writing the roll-forward commit: `sync_dir` of both parents of the recorded move (for `--trash`: the source directory and the trash directory; for `rm`: the parent), idempotently; for the Linux "both names, one inode" state: `unlink` of the source name, then the same flushes |
| settle, reads | none: settles and reads never change the project tree ([40 §4.3] step 6 writes only to the store) |

Chapter 16 carries these as protocol points and WP-40 seeds a bug for the recovery re-barrier ("roll-forward without the
re-barrier", A1P-02).

## 7. Runtime-row sources (X-F8)

The rows are [F11]'s; this section fixes which `ProjectFs` value fills each OS-defined field. All of them are runtime:
never versioned, merged, exported or hashed (I-F4). A row whose `kind` or `os` tag this OS cannot interpret is absent
([80] X1).

### 7.1 `JOURNALCUR` (41 bytes; reserved, E2 not built)

| Offset | Width | Type | Name | Meaning |
|---|---|---|---|---|
| 0 | 1 | u8 | `kind` | 0 none, 1 `usn`, 2 `fsevents`; 3–255 reserved |
| 1 | 16 | [u8; 16] | `vol_key` | §4.1 |
| 17 | 16 | [u8; 16] | `instance` | `usn`: `u64le(UsnJournalID) ‖ 8 zero bytes`; `fsevents`: the device UUID (`FSEventsCopyUUIDForDevice`); none: zero |
| 33 | 8 | u64 | `cursor` | `usn`: the next USN to read (a non-negative `USN` as u64); `fsevents`: the `FSEventStreamEventId` after the last processed event; none: 0 |
| total | 41 | | | |

No M0–M11 code writes it; the record kind `JournalCursor` and the section stay reserved (R-7, R-8, A1P-17).

### 7.2 `DIRMAP`

`(tree key, directory OsFileId) → (root-relative path, mtime, granularity)` ([80 §2.11.2]): the directory's `OsFileId`
comes from its entry in its parent's enumeration (Windows `FileIdExtdDirectoryInfo`, macOS `getattrlistbulk`) or, for
Linux, from `statx` plus `file_handle_digest` of the directory; the path is the `RelPath` the walk reached it by; the
`mtime` is the directory's own `FsTime` (`ns` and `gran`). Written by settles only, read by reads (I-F5).

### 7.3 `TREES` additions

| Field | Source |
|---|---|
| canonical root bytes | `CanonicalRoot.text` ([OS/path §4]) |
| the root directory's `OsFileId` | `CanonicalRoot.root_id` |
| `os` tag | the writing process's OS tag ([OS/proc §2]) |
| `VolumeCaps` snapshot (16 B) | `volume(root)` (§4.2), with `mtime_granularity_ns` from §4.4 |
| case- and normalization-sensitivity map | `case_equivalent` of the directories that hold linked files |
| cloud-root flag | set when the root or an ancestor carries a cloud reparse tag (§3.4 bit 9) or cloud-only attributes, as `doctor` detects ([40 §4.6]) |

### 7.4 `FILEOBS` (R-18) and `PENDING`

| Field | Source |
|---|---|
| tagged `OsFileId` with the parent-directory id | the settle's enumeration (§5.2), or `stat(WithId)` (§5.1) |
| `path_seen` | the `EntryName` enumerated (when it differs from the branch value); an `Unrepresentable` name is never stored |
| size | `StatRec.size` |
| `mtime`, `ctime`, `btime`, `added` | `FsTime` values (§3.3) |
| attributes | `FileAttrs` (§3.4) |
| `last_oid`, `verified_at`, state, proposals, `missing_since`, `resolver_version` | the resolver ([F18], chapter 20); `verified_at` and `missing_since` are HLCs ([OS/clock §7]) |
| `PENDING` captured creation time | `btime` of the source at capture |

`FSINTENT`'s holder is the intent anchor plus a diagnostic `ProcId` ([OS/proc §3], [F03]).

## 8. The per-OS resolver rules at the OS level (R-14, [80 §2.11.4])

| Rule | What `ProjectFs` provides; the rule itself is chapter 20's |
|---|---|
| 1 copy rule | `btime` with its trust class (`VolumeCaps.btime`) and the clone indicator (`FileAttrs` bit 10, `VolumeCaps.clone_indicators`); uniqueness among E4 candidates is decided by the resolver |
| 2 twins, spelling, normalization | `DirEquivalence` (§4.5) and the enumerated spellings (§5.3) |
| 3 reads with ids | `stat(Read)` returns ids on Linux and macOS, not on Windows (§5.1) |
| 4 sorted candidates | enumeration order is the file system's; consumers sort (§5.2) |
| 5 never-candidates, trash | `trash_dirs` (§5.7); the name patterns are chapter 20's |
| 6 `EXDEV` | `CrossDevice` (§2.3); the explicit verb refuses (§6.4) |
| 7 Unix busy states | `Busy` and `AccessDenied` from `EBUSY`, `EACCES`, `EPERM`, never retried; `busy_holders` is empty on Unix (§5.8) |
| 8 whole-id identity | §3.2 |
| 9 denials | §5.11 |
| frontier racy threshold | `touch_stamp` (§5.9) and `VolumeCaps.mtime_granularity_ns` (§4.4) |

## 9. The simulator's obligations (`moirai-projfs-sim`, FL-2)

The simulator implements this trait over an in-memory tree with ground-truth object identity ([60 §3.7] M6): file ids
(NTFS: sequence-numbered slot reuse; ext4 profile: lowest-free inode reuse with random generations; APFS profile:
counter-allocated ids), replace-by-rename, tunneling of creation times within a window, sharing violations (errors 5 and
32) and delete-pending, the Recycle Bin and OS trash, case- and normalization-insensitivity per directory and per volume,
git-style checkout rewrites, cloud placeholders (and a hydration counter that must stay 0 on automatic paths), and
fault-model item (2) for `sync_dir` (unsynced renames and unlinks lost in any subset after a crash). `VolumeCaps` profiles
for the three OSes are data ([80 §3.2], [80 §5.5] (c)). A profile of a volume that cannot flush a directory (an SMB share,
a FUSE mount, `\\wsl$`) makes `sync_dir` fail with `Unsupported` or `AccessDenied`, at the plan step and at recovery's
re-barrier alike, so FL-2 exercises both paths of §6.2 (pass 1, P1-16). On the Windows profile the name check of §2.3
refuses `:`, reserved characters, device names and trailing dots or spaces before any simulated call (P1-15).

## 10. Measurement 15 rows (WP-55)

Measurement 15 ([AR §8.2] item 15, extended by [40 §8.3.6]) runs through `OsProjectFs` on the owner's project volume,
idle and loaded, and records:

1. `stat(Read)` and `stat(WithId)` costs; enumeration with ids per entry; `OpenFileById` on files and on directories, with
   `GetFinalPathNameByHandleW`.
2. The rename of one file and of a 1,000-file directory **as the protocol performs it**: `MoveFileExW` with
   `MOVEFILE_WRITE_THROUGH` plus `FlushFileBuffers` on both parents (A1P-07), under Defender.
3. For each tool — Git Bash `mv`, `cp`, `cp -p`; PowerShell `Move-Item`, `Copy-Item`; Claude Code `Edit` and `Write`;
   **Codex `apply_patch`** (`*** Update File`, `*** Add File`, `*** Delete File`, `*** Move to`); `git checkout` of a
   changed file — whether the file id, `CreationTime`, `LastWriteTime` and `ChangeTime` change, and NTFS tunneling (delete
   and re-create under the same name within and after the tunneling window). The Codex rows are the parity rows PLAN §3.3
   assigns; without Codex access (V9) they wait, like probes P1–P7, and the R-14 inputs they feed keep their conservative
   defaults.
4. Whether a same-volume rename updates `ChangeTime` (S-16: the copy rule's line 2 may rely on it only if it does).
5. Directory-id stability on the project volume across the operations of item 3.
6. The attribute bits of a OneDrive placeholder read through `GetFileAttributesExW` and through enumeration, and a check
   that neither hydrates it (§5.10).
7. The difference between a file's `LastWriteTime` right after a write and the wall clock read around the write (for
   chapter 20's skew margin, [OS/clock §8]).
8. The effective mtime granularity probe of §4.4 with the candidate parameters of its holes.

## Coverage

The rows of `COVERAGE.md` that cite this file ([F01 §2.7]).

| Item | Part covered here | Section |
|---|---|---|
| `60-AU-Vfs-sims` (audit row "`Vfs`/`ProjectFs`": the fault model in both simulators) | the `ProjectFs` simulator's obligations; the items are [F15 §3]'s and their applicability [F15 §6.5]'s | §9 |
| `60-AU-Vfs-projfs` (the same row: `VolumeCaps`, `OsFileId`, `JOURNALCUR`, `DIRMAP`) | file identity and its per-OS sources; volumes and `VolumeCaps` values; runtime-row sources. The layouts are [F11 §12]'s | §3, §4, §7 |
| `60-AU-R14-identity` (whole-`OsFileId` identity) | the per-kind id encoding | §3.2 |
| `60-AU-R14-denials` (denials as `Unknown`) | the per-OS resolver rules at the OS level | §8 |
| `60-PA-(h)` (the paragraph after the audit rows, (h)) | the `file mv` and `file rm` protocol points; the rename calls are [OS/fs §4.8]'s | §6 |
| `R-14` ([40] R-14) | the btime class and clone indicator per file system; reads with ids, trash places, busy holders, denials never absence; cross-volume moves (`EXDEV`); the per-OS resolver rules at the OS level. The constants are [F20]'s | §4.3, §5, §6.4, §8 |
| `R-18` ([40] R-18: the `FILEOBS` row) | the OS-defined fields; the row sources. The layout is [F11]'s | §3.4, §7.4 |
| `X-F8` ([80] X-F8) | identity and timestamps per OS; volumes, `VolumeCaps`, granularity; the read side (reads with ids, trash, busy holders, denials); cross-volume moves; runtime-row sources; the per-OS resolver rules at the OS level. The layouts are [F11 §12]'s, the resolver rules [F20]'s | §3, §4, §5, §6.4, §7, §8 |

## Holes

| Id | What | Decided by | Candidates | Constraint the value must meet |
|---|---|---|---|---|
| HOLE(OS-pfs-gran-probe-k) | the number of distinct stamp mtimes §4.4 waits for | measurement 15 (WP-55) | 3, 4 | with the budget below, identifies the effective resolution of the owner's project volume (a Windows clock tick of up to 15.625 ms, or 1 ms under a raised timer resolution) in ≥ 99 % of idle and loaded runs |
| HOLE(OS-pfs-gran-probe-budget) | the mono-time budget of the §4.4 probe | measurement 15 (WP-55) | 50 ms, 100 ms | ≥ 2 effective ticks of the slowest resolution in the constraint above plus scheduling noise under load; run only outside hook and server paths, so it never counts against `files.session-start-cap-ms` or an MCP slice |

## Open points for the review

| # | Point | Resolution in this file | For |
|---|---|---|---|
| 1 | A1P-01: the cross-volume `file mv` copy path has no namespace ordering and `ProjectFs` has no copy operation | preferred fix adopted: refusal (§6.4), no copy operation; [40 §3.4]'s copy branch and [80 §2.11.4] rule 6's copy clause are superseded for the explicit verb; lens A confirms the agent-facing text (A1P open point 3); if the review keeps the copy path instead, `ProjectFs` gains `copy_durable(from, to)` with A1P-01's sequence and chapter 16 a protocol point | R-REV-A, WP-16 |
| 2 | A1P-02: recovery must re-establish the directory barrier before rolling forward | `sync_dir` is in the trait (§6.2), idempotent; the recovery row is §6.5; chapter 16 adds the protocol point and WP-40 the seeded bug | WP-16, WP-40 |
| 3 | A1P-05: the two passes of the `oid` reader must use one handle and detect an in-place writer | `ProjectRead::rewind` and `snapshot` (§5.5) | WP-62, WP-64 |
| 4 | A1P-07: flush accounting must separate log flushes from directory flushes | `PfsCounters.dir_syncs` (§2.4) | WP-50, WP-55 |
| 5 | A1P-17: `journal_since` is excluded from the M0 trait | omitted (§1); `JOURNALCUR`'s 41 bytes are still fixed (§7.1) because the format reserves them | WP-30, WP-13 |
| 6 | [OS/fs §6.1]'s `VfsErrorKind` (non-exhaustive) lacks five kinds `ProjectFs` needs | §2.3 uses [OS/fs §6.2]'s mapping unchanged; **pass 1 (A1-33):** [OS/fs §6.1] lists `CloudOnly`, `IsSymlink`, `IsDirectory`, `OutsideRoot` and `Stale`, with mapping rows in [OS/fs §6.2] | WP-30 |
| 7 | [OS/README §4.2] says `sync_dir` shares [OS/fs]'s calls; a flush failure must reach `fail_stop` | `sync_dir` returns `DurabilityFailure`; the composites return `RenameFailure` distinguishing "not done" from "done, not durable" (§2.1, §6.3) | WP-17a, WP-30 |
| 8 | [80 §2.11.1] gives Windows `btime = TunneledNotCopied` without distinguishing ReFS; [F20] makes the NTFS class and ChangeTime-on-rename its holes | NTFS takes HOLE(F20-btime-ntfs) and HOLE(F20-ctime-rename) (§4.3); ReFS reports `absent` until a ReFS measurement shows tunneling and non-copying (a conservative reading: the copy rule then yields at most STRONG) | R-REV-P, WP-14b |
| 9 | Deleting a read-only project file: [40] does not say whether moirai clears the attribute | as [OS/fs §4.7] (clear, then delete), which gives parity with Unix, where `unlink` ignores the mode; unlike a store file, a project file whose delete fails gets its attribute back, so a failed `file rm` changes nothing (§6.3) | R-REV-A |
| 10 | The Windows symlink target text must equal what git stores (`/` separators) | `PrintName` with `\` → `/` (§5.6); WP-74's differential gains a synthetic-symlink case against `git hash-object` when Git for Windows has `core.symlinks = true` (M6 at the latest) | WP-74 |
| 11 | Junctions, WSL links and app-execution aliases have no rule in [40] | kind `Other`: never read, never followed, never candidates (§5.6, §5.2) | R-REV-S |
| 12 | `locate_id` could open a `RECALL_ON_OPEN` placeholder | it takes the recorded `FileAttrs` and returns `NotLocatable` for a cloud-only record (§5.4, §5.10 rule 3) | R-REV-P |
| 13 | [80 §2.11.2] fixes the `FILEOBS` "attributes" and timestamp "granularity byte" without their encodings, and the `VolumeCaps` snapshot without its flag bits and enum values | fixed here: `FileAttrs` (§3.4), `FsTime.gran` (§3.3), the snapshot (§4.2); [F11] embeds them | WP-13 |
| 14 | [80 §2.11.2] gives `vol_key` = BLAKE3-128 of "a fixed source tag ‖ one fixed source per kind" without bytes | §4.1: `BLAKE3-128(lp("moirai-vol-key-v1") ‖ lp(N) ‖ lp(S))`, the framing of [F01 §6.3, §7.3] | WP-13 |
| 15 | PLAN §3.3 assigns "Codex `apply_patch` parity in measurement 15" to WP-17 and WP-55 | §10 item 3 adds the four `apply_patch` operations beside Claude Code's tools; results feed R-14 through WP-81a; without Codex access the rows wait | WP-55 |
| 16 | Whether `file mv` creates a missing destination parent is not stated in [40 §3.4] | `ProjectFs` has no directory creation in project trees; the verb refuses a missing destination parent unless [F18]/[40] decide otherwise (then a `create_dir` with a `sync_dir` of its parent is added) | WP-14 |
| 17 | The effective-granularity probe writes only in `<store>/tmp/`, so it measures only the store's volume | other volumes record 0 and use the nominal resolution; their racy threshold is [F20 §5.12.1]'s largest `DIRMAP` mtime (§4.4) | WP-14b |
| 18 | [F02 §5.3]'s `tmp/` names are `<word>.<nonce>` and do not include `settle.stamp`, which [80 §2.11.3] and [F20 §5.12.1] use | **closed (pass 1, P1-32, S1-38):** [F02 §5.3] and §6.3 list the fixed name `settle.stamp` (written by `ProjectFs::touch_stamp`, never mapped or read as store data; the orphan sweep may remove it, and the next settle recreates it) | — |
| 19 | [OS/README §4.2] says project roots are "held as text and root id, not as open directory handles" | true on Windows (§2.2); on Linux and macOS a project `Root` holds an `O_PATH`/`O_DIRECTORY` descriptor for `openat` walks (P10), which blocks no rename there; README §4.2's sentence may name Windows | WP-17a |
| 20 | [F20 §3.6] takes a path's on-disk spelling from `GetFinalPathNameByHandleW` on Windows, which no listed `ProjectFs` method gave | `disk_spelling` added (§5.3) | WP-30 |
| 21 | [F01] open point 15 proposes the prefix `OS-` for hole ids in `docs/spec/os/` | adopted: HOLE(OS-pfs-gran-probe-k), HOLE(OS-pfs-gran-probe-budget) | WP-10 |
| 22 | Pass 1, P1-15: project paths reach NTFS through `\\?\`, which skips Win32 name normalisation, so `:` (alternate streams), reserved characters, device names and trailing dots or spaces were not refused | the Windows name check of §2.3 on every segment before any OS call, `InvalidName`, never relaxed by `--allow-nonportable`; [F20 §4.9] applies `representable_here` before any cascade call and [F18 §4.6] detail 44 renders it | WP-30, WP-63 |
| 23 | Pass 1, P1-16: a project volume without directory flush failed `file mv` after the rename and blocked intent recovery | the plan step's `sync_dir` and its `no_dir_flush` refusal (§6.2, [API §12.4] step 1, [F19 §10.2]); recovery leaves the intent open with a `doctor` text ([F16] P-71); `VolumeCaps` bit 14 `dir_flush_doubtful` by file-system class (§4.2); FL-2 profiles for both paths (§9) | WP-30, WP-66 |
| 24 | Pass 1, P1-38: `read_for_hash` checked cloud attributes by path and then opened the file | the attributes are re-read through the handle before the first read (§5.5 step 3, [OS/mapping-appendix §2.1]) | WP-30 |
| 25 | Spec sync 2a (WP-30, WP-33, WP-62): points met while building `ProjectFs` | **closed:** §2.3 and [OS/fs §6.2] match the `ERROR_CLOUD_FILE_*` family by name; §2.4 counts `stats` and `dir_reads` only for calls that passed the Windows name check; §3.1 gives the Rust `OsFileId` (public fields, `kind: FileIdKind`, no `aux` field, canonical `to_bytes`, `==`/`Hash` on the canonical form, identity only through `same_object`); §4.2 makes `Sensitive` with `case_insensitive_default = true` invalid; §5.2 refuses `enumerate` on a directory link (`IsSymlink`, else `Other`) and makes `ProjEntry<'a>` borrow its name (`EntryNameRef<'a>`, [OS/path §2.4]), before FL-2 builds on it; §5.5 compares containment as an exact byte prefix and states the two-pass rule as [F20 §2.4]; §6.3 removes a directory link as a link (Windows `RemoveDirectoryW`, Unix `unlinkat` without `AT_REMOVEDIR`) and counts a cloud directory placeholder as a directory | FL-2, WP-33 |
| 26 | Spec sync 2b (WP-30b/31b/33b review): `enumerate`'s attribute check and its open are two calls, so a directory swapped for a junction between them was followed out of the tree; "once if they are one directory" in §6.3 did not say how two parents are compared, and a case-folded comparison missed a flush under per-directory case sensitivity | **closed:** §5.2 checks the open handle's final path as §5.5 step 3 does (`OutsideRoot`), one `GetFinalPathNameByHandleW` per directory; §6.3 (and [OS/fs §4.9.2, §4.9.4]) decide "one directory" by exact spelling or by `FileIdInfo` identity, never case-folded | moirai-os |
