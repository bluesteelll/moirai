# OS layer: store file system (`os::fs`)

| Field | Value |
|---|---|
| Title | `os::fs` — store-file operations, durability classes, renames, sharing modes and error mapping |
| Status | draft, pass 1 pending |
| Work package | WP-17a (role R-SPEC-P), part 1 of WP-17 |
| Sources | [80 §2.1] (`os::fs` row, boundary rules); [80 §2.3.1] (classes and calls); [80 §2.3.2] (protocol points, owned by [F16]); [80 §2.3.3] (extent creation); [80 §2.3.4] (flush failure); [80 §2.3.5] items (2), (3), (5), (8), (10), (12); [80 §2.5] rule 2 (seal); [80 §2.12] rows "Bulk passes", "Store files' sharing", "Temporary files", "Store `config` rewrite"; [80 §3.1] X-F5; [AR §2.8] T8; [AR §4.1] rules paragraph; [AR §4.10]; [AR §6.5]; [40 §3.4] step 3 (errors 5/32, `--retry-ms`); [60 §2.5] fault-model row items (5), (8), protocol decisions (a), (c), (f), (h); [60 §5.1] "Counts"; [90 §5.4] (store-directory ACE); PLAN §6.1 #3 (`MOVEFILE_WRITE_THROUGH`); `docs/spec/reviews/a1-P.md` A1P-07; research reports [X17 §3.1–§3.8], [13 §1.4], [05 §6] as cited by [80] |

---

## 1. Scope

`os::fs` is the `StoreFs` sub-trait of `Vfs` ([OS/README §4.1]). It serves:

- every file under the store directory (`<git-common-dir>/moirai/` or `.moirai/`, [F02]): `HEAD`, `LOCK` (data records
  only; its locks are [OS/lock]), `config`, log extents, sealed files, `tmp/`;
- every file of an image destination and of a `backup` directory, whose protocol points also use the durability classes
  ([80 §2.3.2], [80 §3.2] git object layer row);
- the store directory's own parent, for `init`, `restore` and the swap intent (§4.9).

It never serves project files (the user's tree); those go through `ProjectFs` ([OS/project], part 2), which reuses this
file's rename calls, flags and error mapping ([OS/README §4.2]).

What this file freezes (X-F5): the four durability classes and `sync_group` with their per-OS calls (§4.4, Appendix A);
no downgrade on any refusal; no direct-I/O path; every rename point as a no-replace rename followed by `durable-name` on
both parents, with `rename_replace` and `swap_dirs` for the points that replace or exchange (§4.8, §4.9); the swap
intent's bytes (§4.9.3). Which protocol point uses which class is frozen in [F16] ([80 §2.3.2]); §4.4.6 restates it for
the implementer.

---

## 2. Types

### 2.1 `RelPath`

`os::fs` names files by root plus `RelPath`. There is **one** `RelPath` type for store and project paths, defined in
[OS/path §2.1] and [OS/path §11] (`RelPath`, `RelPathBuf`, `PathError`): valid UTF-8 compared as exact bytes; segments
separated by `/`; no leading or trailing `/`, no empty, `.` or `..` segment; no `\` and no C0 control character (P1,
P4); the empty value denotes the root itself. Store names are a subset: decimal numbers and fixed ASCII words ([F02],
X-F10). `RelPath<'_>` is a `Copy` view passed by value ([OS/path §2.1]). Where this file writes `Option<RelPath<'_>>` for a
directory, `None` and the empty `RelPath` both name the root.

The type sets no length limit. **Use-time checks** of `os::fs` (the operation fails with `InvalidName`, nothing is
created):

| Windows | Linux | macOS |
|---|---|---|
| a segment longer than 255 UTF-16 code units; a segment containing `<`, `>`, `:`, `"`, `\|`, `?` or `*` (`:` would address an NTFS alternate data stream); a segment ending in `.` or a space (the `\\?\` and handle-relative forms bypass Win32 name normalisation, so such a name would be created literally) | a segment longer than 255 bytes (`ENAMETOOLONG`) | a segment longer than 255 UTF-8 bytes |

Portability of names moirai creates (P5, P11) is enforced where the names are built ([OS/path §8]), not here.

Store file names themselves follow [F02] (decimal numbers and fixed ASCII words, [80] X-F10); `RelPath` also admits the
names an image destination needs (for example `refs/heads/lane/x`, `objects/pack/pack-<hex>.pack`).

### 2.2 Roots

```rust
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum RootRole {
    /// A store directory: the environment guard classifies it ([OS/env §4]) and extents may be created in it.
    Store,
    /// Any other directory: an image destination, a backup directory, the parent of a store directory.
    Other,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum RootAccess {
    /// Readers: open, read, list, identity. No create, write, sync, rename or unlink.
    Read,
    /// Writers, maintenance, `init`, `restore`, exports, backups.
    ReadWrite,
}
```

A root is an open directory handle. **Every operation on a file names a root and a `RelPath` and resolves the path
relative to the root's handle**, never through an absolute path string ([80 §2.10] P10). Consequently a process that opened
a store directory keeps resolving names in that directory even if the directory is renamed under it (a `restore` swap,
§4.9): it cannot silently mix files of two stores, and it learns of the swap from the old store's `HEAD.retired` flag
([AR §4.2]). The exceptions are the Windows calls that take path strings — `MoveFileExW` (§4.8, §4.9), `DeleteFileW` and
`SetFileAttributesW` (§4.7), `RemoveDirectoryW` (§4.2) and `GetDiskFreeSpaceExW` (§4.11) — built from the root's
canonical final path in `\\?\` form; the protocol issues the renames and deletes among them only under a lock that
excludes the swap, after the `retired` check ([F16]).

### 2.3 Files

```rust
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Access { Read, ReadWrite }

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum OpenHint {
    Normal,
    /// Bulk passes (rollup, backup, full export, retirement, repair, `links check --all`): sequential read-ahead and no
    /// cache retention ([80 §2.12] "Bulk passes").
    Sequential,
}
```

A `File` handle is positional only: it has no cursor that operations depend on, and it may be used by several threads
(the MCP server has at most two, [AR §2.2]).

### 2.4 Durability

```rust
/// The durability classes of [80 §2.3.1]. `Lazy` has no call: a write is lazy until a covering flush.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum DurabilityClass { Lazy, Durable, DurableMeta, DurableName, SyncGroup }

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum SyncKind {
    /// Class `durable`: the file's data within its current size.
    Data,
    /// Class `durable+meta`: data plus the file's size and allocation.
    DataAndMeta,
}

/// One member of a `sync_group` (class `sync_group`).
pub enum GroupMember<'a, R, F> {
    File { file: &'a F, kind: SyncKind },
    /// `dir: None` names the root directory itself.
    Dir { root: &'a R, dir: Option<RelPath<'a>> },
}
```

### 2.5 `ShareRetry`

```rust
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ShareRetry {
    /// One attempt. Used by GC deletes, which retry at the next GC ([AR §4.1]).
    None,
    /// Retry sharing violations for at most `total_ms` of monotonic time (§6.3).
    Bounded { total_ms: u32 },
}
```

### 2.6 `FileIdentity`

```rust
/// A process-local identity of a file or directory. Two handles name the same object iff their identities are equal.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct FileIdentity { pub volume: u64, pub file: [u8; 16] }
```

`FileIdentity` is not the R4 `OsFileId` of [80 §2.11.2] (57 B, tagged, stored in `FILEOBS`); it is never versioned,
hashed or exported. It is used by the `LOCK` identity check ([OS/lock §9]) and stored in exactly one place, the swap intent
(§4.9.3), in this 24-byte form:

| Offset | Width | Type | Name | Meaning |
|---|---|---|---|---|
| 0 | 8 | u64 | `volume` | Windows: `FILE_ID_INFO.VolumeSerialNumber`. Linux and macOS: `st_dev` zero-extended to 64 bits |
| 8 | 16 | [u8; 16] | `file` | Windows: `FILE_ID_INFO.FileId` (the `FILE_ID_128` bytes as the OS returns them). Linux and macOS: `st_ino` as u64 little-endian in bytes 8–15, bytes 16–23 zero |
| total | 24 | | | |

### 2.7 Other types

```rust
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct FreeSpace {
    /// Bytes available to this user (quotas applied where the OS applies them).
    pub available: u64,
    /// Total bytes of the volume.
    pub total: u64,
}

/// `name` is [OS/path §2.4]'s `EntryName`: `Utf8` when the name is valid Unicode and passes P4, else `Unrepresentable`
/// with the OS bytes (WTF-8 of the UTF-16 name on Windows, the raw bytes on Unix).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DirEntry { pub name: EntryName, pub kind: EntryKind }

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum EntryKind { File, Dir, Other }

/// Process-wide instrumentation, monotonic, read with relaxed atomics ([60 §5.1] "Counts"; A1P-07).
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct VfsCounters {
    pub opens: u64,
    pub creates: u64,
    pub bytes_read: u64,
    pub bytes_written: u64,
    /// Class `durable` calls (every one is a log-extent data flush in the protocol).
    pub sync_data: u64,
    /// Class `durable+meta` calls.
    pub sync_meta: u64,
    /// Class `durable-name` calls, including the directory members of `sync_group`.
    pub sync_dir: u64,
    /// macOS only: `F_FULLFSYNC` issued as the device barrier of `sync_dir` or `sync_group`; 0 elsewhere.
    pub full_barriers: u64,
    pub renames: u64,
    pub unlinks: u64,
    pub maps: u64,
    pub mapped_bytes: u64,
    /// Attempts beyond the first made under `ShareRetry::Bounded`.
    pub share_retries: u64,
}
```

The gates that count flushes read `sync_data` and `sync_dir` separately: "2 log flushes and 2 directory flushes" for a
Windows `file mv` (A1P-07; the `ProjectFs` counters of [OS/project] report the same two quantities).

---

## 3. The `StoreFs` trait

```rust
pub trait StoreFs: VfsTypes {
    // Roots (§4.1)
    fn open_root(&self, dir: &std::path::Path, role: RootRole, access: RootAccess) -> Result<Self::Root, VfsError>;
    fn create_root(&self, dir: &std::path::Path, role: RootRole) -> Result<Self::Root, VfsError>;

    // Names (§4.2)
    fn open(&self, root: &Self::Root, rel: RelPath<'_>, access: Access, hint: OpenHint) -> Result<Self::File, VfsError>;
    fn create_new(&self, root: &Self::Root, rel: RelPath<'_>) -> Result<Self::File, VfsError>;
    fn create_dir(&self, root: &Self::Root, rel: RelPath<'_>) -> Result<(), VfsError>;
    fn remove_dir(&self, root: &Self::Root, rel: RelPath<'_>) -> Result<(), VfsError>;
    fn list_dir(&self, root: &Self::Root, dir: Option<RelPath<'_>>) -> Result<Vec<DirEntry>, VfsError>;

    // Positional I/O (§4.3)
    fn read_at(&self, file: &Self::File, offset: u64, buf: &mut [u8]) -> Result<usize, VfsError>;
    fn read_exact_at(&self, file: &Self::File, offset: u64, buf: &mut [u8]) -> Result<(), VfsError>;
    fn write_at(&self, file: &Self::File, offset: u64, buf: &[u8]) -> Result<(), VfsError>;

    // Durability classes (§4.4)
    fn sync(&self, file: &Self::File, kind: SyncKind) -> Result<(), DurabilityFailure>;
    fn sync_dir(&self, root: &Self::Root, dir: Option<RelPath<'_>>) -> Result<(), DurabilityFailure>;
    fn sync_group(&self, members: &[GroupMember<'_, Self::Root, Self::File>]) -> Result<(), DurabilityFailure>;
    fn fail_stop(&self, failure: DurabilityFailure) -> !;

    // Extents and sealing (§4.5, §4.6)
    fn create_extent(&self, root: &Self::Root, rel: RelPath<'_>, len: u64, vol: &StoreVolume) -> Result<Self::File, VfsError>;
    fn recycle_extent(&self, file: &Self::File, len: u64, vol: &StoreVolume) -> Result<(), VfsError>;
    fn seal(&self, file: &Self::File) -> Result<(), VfsError>;

    // Namespace changes (§4.7–§4.9)
    fn unlink(&self, root: &Self::Root, rel: RelPath<'_>, retry: ShareRetry) -> Result<(), VfsError>;
    fn rename_noreplace(&self, from_root: &Self::Root, from: RelPath<'_>, to_root: &Self::Root, to: RelPath<'_>,
                        retry: ShareRetry) -> Result<(), VfsError>;
    fn rename_replace(&self, from_root: &Self::Root, from: RelPath<'_>, to_root: &Self::Root, to: RelPath<'_>,
                      retry: ShareRetry) -> Result<(), VfsError>;
    fn swap_dirs(&self, a_parent: &Self::Root, a: RelPath<'_>, b_parent: &Self::Root, b: RelPath<'_>,
                 retry: ShareRetry) -> Result<SwapOutcome, VfsError>;
    fn swap_recover(&self, a_parent: &Self::Root, a: RelPath<'_>, retry: ShareRetry) -> Result<SwapRecovery, VfsError>;

    // Queries (§4.10–§4.13)
    fn file_size(&self, file: &Self::File) -> Result<u64, VfsError>;
    fn identity(&self, file: &Self::File) -> Result<FileIdentity, VfsError>;
    fn root_identity(&self, root: &Self::Root) -> Result<FileIdentity, VfsError>;
    fn path_identity(&self, root: &Self::Root, rel: RelPath<'_>) -> Result<FileIdentity, VfsError>;
    fn free_space(&self, root: &Self::Root) -> Result<FreeSpace, VfsError>;
    fn advise_dontneed(&self, file: &Self::File, offset: u64, len: u64);
    fn counters(&self) -> VfsCounters;
}
```

Renames against [80 §2.1]'s list: `open_store_file` became `open` and `create_new`; `sync(Data | DataAndMeta)`,
`sync_dir`, `sync_group`, `create_extent`, `seal`, `unlink`, `rename_noreplace`, `rename_replace`, `swap_dirs`,
`file_size` and `identity(file)` keep their names. Added: `open_root`, `create_root`, `create_dir`, `remove_dir`,
`list_dir`, `read_exact_at`, `fail_stop`, `recycle_extent`, `swap_recover`, `root_identity`, `path_identity`,
`free_space`, `advise_dontneed`, `counters` (reasons in the sections below; open point 1).

---

## 4. Operations

### 4.1 `open_root`, `create_root`

- **`open_root(dir, role, access)`.** `dir` must be absolute. The implementation opens the directory itself (Appendix A
  row A1) and keeps, with the handle: the role, the access, and on Windows the canonical final path (from
  `GetFinalPathNameByHandleW`, used only by §4.7–§4.9 and diagnostics). A root of role `Store` is then classified by the
  caller through [OS/env §4] before any other use. A missing directory is `NotFound`.
- **`create_root(dir, role)`.** Creates the directory `dir` (its parent must exist; no recursive creation), makes the
  creation durable (`durable-name` on the parent, so the directory survives a power loss before the first
  acknowledgement), and returns it as `open_root(dir, role, ReadWrite)` would. For role `Store` on Windows it first gives
  the new directory an **explicit inheritable ACE granting the process's user full control** (object and container
  inherit), so files that a sandbox principal creates under it stay writable and deletable by the owner's unsandboxed
  processes ([90 §5.4], [80 §2.12] `srt-win` row). Unix creates the directory with mode `0o777` minus the umask; no ACL
  is added. An existing `dir` is `AlreadyExists`.
- **A failed embedded flush.** If the `durable-name` flush of the parent fails, `create_root` removes the new, empty
  directory (the removal's own error is ignored) and returns `VfsError` with the kind **`FlushFailed`**, carrying the
  flush's `OsCode` and `call` (§6.1). It never returns a `DurabilityFailure`: no commit is at stake yet, the directory
  is removed so that a later run does not meet `AlreadyExists`, and the caller (`init`, `restore`) exits 7 with the
  `durability-failure` text of [F19] and issues no further write, flush, create or namespace call. It never retries.
  The same rule covers the flushes embedded in `swap_dirs` and `swap_recover` (§4.9), which remove nothing: their
  intent stays for `swap_recover`. An ACE failure on the path above also removes the new directory, with its own kind.

### 4.2 `open`, `create_new`, `create_dir`, `remove_dir`, `list_dir`

- **`open(root, rel, access, hint)`** opens an existing regular file relative to `root`. `access = ReadWrite` requires a
  root opened `ReadWrite`. Sharing and flags follow §5. `hint = Sequential` requests the bulk-pass behaviour of §4.12.
  Opening a delete-pending file fails with `DeletePending` (§6.4); a caller that took the name from a `HEAD` it read
  earlier re-reads `HEAD` and retries once ([AR §4.1]; [F16]).
- **`create_new(root, rel)`** creates a regular file that must not exist (`AlreadyExists` otherwise) and opens it
  `ReadWrite`. The new file is empty, and its name is not durable until `sync_dir` of its parent (fault-model item (2)).
  Every store file is created this way: `LOCK` by `init`/`restore` only ([OS/lock §9]), sealed files under their final
  monotonic number, temporary files under `tmp/` ([80 §2.12] "Temporary files": never `%TEMP%` or `$TMPDIR`).
- **`create_dir(root, rel)`** creates one directory level (`AlreadyExists` if present); not durable until `sync_dir` of its
  parent. **`remove_dir(root, rel)`** removes an empty directory (`NotEmpty` otherwise); used for the directories `init`
  created when the guard refuses the location ([OS/env §5]) and for emptied temporary directories.
- **`list_dir(root, dir)`** returns every entry of the directory except `.` and `..`, in unspecified order (callers that
  need an order sort by name bytes, [80 §2.11.4] rule 4). Names that are not valid Unicode or fail P4 come back as
  `EntryName::Unrepresentable`; the orphan sweep ([F02]) removes only names it recognises and `doctor` reports the rest.

### 4.3 `read_at`, `read_exact_at`, `write_at`

- **`read_at(file, offset, buf)`** reads up to `buf.len()` bytes at `offset` and returns the count; fewer bytes than
  requested only at end of file, 0 at or beyond it. Interrupted calls (`EINTR`) are retried inside the call.
- **`read_exact_at`** fails with `UnexpectedEof` if the file ends before `offset + buf.len()`.
- **`write_at(file, offset, buf)`** writes all of `buf` at `offset`, looping over partial writes, or fails. A write beyond
  the end extends the file (zeros between the old end and `offset` read as zero); the protocol never relies on that for
  log extents, which are created at full length (§4.5). A write is class `lazy`: visible to every process at once through
  the page cache, lost after an OS crash or power loss unless a covering flush follows, and indeterminate after a failed
  flush in any process ([80 §2.3.1], fault-model item (3)).
- **Errors.** A read may fail with `Io` (fault-model item (12): `EIO`, a checksum error of the file system, a failing
  medium); the protocol decides what that means by position ([F16]). A write may fail with `DiskFull` on any byte, an
  overwrite of written or zero-filled space included (fault-model item (5)); the command aborts without an
  acknowledgement (protocol decision (f), [F16]). After any write error the content of the written range is
  indeterminate.
- **Concurrency.** Reads concurrent with a write of the same range may return any mix of old and new sectors
  (fault-model item (4)); the format's checksums detect it.

### 4.4 Durability classes

The classes and their guarantees are identical on every OS ([80 §2.3.1]); only the calls differ (Appendix A rows
A10–A13). **A class that the location cannot provide refuses the store; no call is ever replaced by a weaker one**
([80] X5).

#### 4.4.1 `lazy`

No call. Bytes written by `write_at` are visible to every process after the protocol publishes them, survive a crash of
the writing process, may be lost after an OS crash, a power loss or a failed flush in any process, and become durable at
the next flush that covers them ([80 §2.3.1]).

#### 4.4.2 `durable` and `durable+meta`: `sync(file, kind)`

| Kind | Guarantee | Windows | Linux | macOS |
|---|---|---|---|---|
| `Data` (`durable`) | the file's data within its current size is on stable media when the call returns, assuming the drive honours FLUSH | `NtFlushBuffersFileEx(handle, FLUSH_FLAGS_FILE_DATA_SYNC_ONLY, NULL, 0, &iosb)` | `fdatasync(fd)` | `fcntl(fd, F_FULLFSYNC)` |
| `DataAndMeta` (`durable+meta`) | as `Data`, plus the file's size and allocation | `FlushFileBuffers(handle)` | `fsync(fd)` | `fcntl(fd, F_FULLFSYNC)` |

- The handle must have write access; `sync` on a `Read` handle is a programming error (asserted).
- Every flush covers the whole file, whichever process wrote the bytes, which is what makes one flush cover every group
  appended before it on all three OSes ([80 §2.4.2]).
- Never used as durability: plain `fsync` or `F_BARRIERFSYNC` on macOS, `FILE_FLAG_WRITE_THROUGH`, `O_DSYNC`,
  `O_DIRECT` + `RWF_DSYNC`, `sync_file_range` ([80 §2.3.1], [X17 §3.1]).
- On Windows, a `STATUS_INVALID_PARAMETER`, `STATUS_NOT_SUPPORTED` or `STATUS_INVALID_DEVICE_REQUEST` from
  `NtFlushBuffersFileEx` is `Unsupported`: the store is refused (the `init` probe performs this exact call, [OS/env §5]).
  The implementation does not fall back to `FlushFileBuffers`, even though that call is stronger, so that one location
  always takes one path (open point 4).

#### 4.4.3 `durable-name`: `sync_dir(root, dir)`

Makes durable every create, rename and unlink already performed in the directory `dir` (the root itself when `None`).
**A create, rename or unlink is durable only after `sync_dir` of its parent; before that any subset of the unsynced
namespace operations may be lost, in any order** (fault-model item (2) as amended, [80 §2.3.5]).

| Windows | Linux | macOS |
|---|---|---|
| `FlushFileBuffers` on a handle to the directory opened with `FILE_FLAG_BACKUP_SEMANTICS` **and** `GENERIC_WRITE` (a read-only directory handle fails with error 5 [M, X17 §3.4]) | `fsync` on `open(dir, O_RDONLY \| O_DIRECTORY \| O_CLOEXEC)` (on ext4 this forces a full journal commit, [X17 §3.4]) | `fsync(dirfd)`, then the device barrier `fcntl(dirfd, F_FULLFSYNC)` |

- A root opened `ReadWrite` may cache its directory-flush handle; subdirectory handles may be cached per root.
- `sync_dir` on a root opened `Read` is a durability failure with the kind `AccessDenied` (Windows code 5, `call`
  `FlushFileBuffers`), issued without an OS call.
- On Windows every rename additionally carries `MOVEFILE_WRITE_THROUGH` (§4.8) until the post-release rig calibration
  shows it unnecessary (PLAN §6.1 #3); that flag never replaces `sync_dir`, which every rename point requires
  ([80 §2.3.1]).

#### 4.4.4 `sync_group(members)`

Every listed file and directory is durable when the call returns. Members must lie on one volume; if they do not, the
implementation treats each volume's members as a separate group, in the order of their first appearance.

| Windows | Linux | macOS |
|---|---|---|
| each member by its class, in the given order (`sync` or `sync_dir`) | each member by its class, in the given order (each is a device flush) | `fsync` of each member in order (plain `fsync`, directories included), then **one** `fcntl(F_FULLFSYNC)` on the last member of the volume; the man page guarantees it persists everything `fsync`'d on that device before it ([80 §2.3.1], [X17 §3.8.1]) |

`sync_group` is the macOS form of "several `durable+meta` and `durable-name` steps before one record" ([80 §2.3.2]
"on macOS one `sync_group`"); on Windows and Linux it is exactly the sequence of its members.

#### 4.4.5 One error policy: `DurabilityFailure` and `fail_stop`

```rust
/// An error returned by a non-lazy class. It must be passed to `fail_stop`; it has no other consumer.
#[must_use]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DurabilityFailure { pub class: DurabilityClass, pub call: &'static str, pub kind: VfsErrorKind, pub os: OsCode }
```

1. **Any error from any class other than `lazy` aborts the process without an acknowledgement**: `EIO`, `ENOSPC`,
   `EDQUOT`, `EROFS`, `ENOTSUP` and the Windows equivalents including `ERROR_DISK_FULL` ([80 §2.3.1]). The flush is
   never retried on the same handle.
2. The caller hands the failure to **`fail_stop`**, which writes one line to stderr (the text of [F19] for
   `durability-failure`, naming the class, the call and the OS code) and ends the process immediately with **exit code 7**
   ("store unavailable", [AR §7.1]), without running destructors or `atexit` handlers: Windows
   `TerminateProcess(GetCurrentProcess(), 7)`, Unix `_exit(7)`. Its locks are released by the OS at death
   ([OS/lock §3] item 10 on the lag).
3. The next flush holder repairs the log by re-writing the unflushed range under the writer byte before it flushes
   ([80 §2.3.4]; protocol decision (a), [F16]). In the simulator `fail_stop` is a crash of the simulated process at that
   point, with fault-model item (3) applied to its unflushed ranges.
4. `ENOTSUP` from `F_FULLFSYNC` (an SMB mount, some FAT volumes) is the same failure; the environment guard refuses such
   locations at `init` and at every open, so at run time it indicates an ungated configuration ([OS/env §3]).

#### 4.4.6 Protocol points (informative; normative in [F16])

[80 §2.3.2] is the frozen table; this restatement tells WP-33 and WP-40 which operations each point issues.

| Protocol point | `os::fs` sequence |
|---|---|
| Commit group | `write_at` (under the writer byte); later `sync(Data)` on each extent holding bytes of the flushed range, by the flush holder |
| Log extent creation and rotation | `create_extent` → `sync(DataAndMeta)` on it → `sync_dir(store root)`; macOS: `create_extent` → `sync_group([extent DataAndMeta, store root])` |
| Sealed file (segment, `hist`, `blobs`, `cs.NNNN`, `gitmap` page, `dict`) | `create_new` under the final number (or a `tmp/` name) → `write_at`… → `sync(DataAndMeta)` → (if a temporary name: `rename_noreplace` into place) → `seal` → `sync_dir` of every parent involved — before the record that names the file is appended |
| `HEAD` publish | `write_at` only (1PC+C) |
| `HEAD` barrier before a deletion, retirement or recycling | (no-op publish under the writer byte) → `sync(DataAndMeta)` on `HEAD` outside the writer byte → `unlink` → `sync_dir` |
| Boot-change recovery | re-write with `write_at` → `sync(Data)` on the log → publish → `sync(DataAndMeta)` on `HEAD` |
| Store `config` rewrite | `create_new(tmp/…)` → `write_at` → `sync(DataAndMeta)` → `rename_replace` onto `config` → `sync_dir(store root)` (and of `tmp/`) |
| `restore` swap | `swap_dirs` (§4.9), under the writer and maintenance bytes |
| `backup` | per copied file `create_new` → `write_at`… → `sync(DataAndMeta)`; then `sync_dir` of the backup directory; then the durable `Backup` record. Copies are by read and write only: never reflink, `clonefile` or `CopyFileW` block cloning ([80 §2.3.2]) |
| Image export | pack and idx `sync(DataAndMeta)` → `rename_noreplace` into `objects/pack/` → `sync_dir`; a loose object `sync(DataAndMeta)` → `rename_noreplace` into `objects/<xx>/` (an existing name is success) → `sync_dir` of `objects/<xx>/` and, when created, of `objects/` ([F16] P-100); `packed-refs.lock` `sync(DataAndMeta)` → `rename_replace` onto `packed-refs` → `sync_dir` (loose `<ref>.lock` → `<ref>` likewise); only then the `gitmap` commit |

### 4.5 `create_extent`, `recycle_extent`

**The format invariant** is the same everywhere: a log extent is exactly `len` bytes long (`store.log-extent-bytes`,
[F17]) and every byte beyond the durable tail reads as zero ([80 §2.3.3]). How the zeros arise depends on the file system,
which the environment guard reports in `StoreVolume.extent_method` ([OS/env §2]):

| `ExtentMethod` | File systems | `create_extent(root, rel, len, vol)` | `recycle_extent(file, len, vol)` |
|---|---|---|---|
| `ZeroFill` | NTFS (gated); ext4 and XFS on kernels without `FALLOC_FL_WRITE_ZEROES` | `create_new`, then write zeros over `[0, len)` in order from one reused zero buffer of at most 1 MiB | write zeros over `[0, len)`, then set the length to `len` if the file is longer (Windows `SetFileInformationByHandle(FileEndOfFileInfo)`, Unix `ftruncate`) |
| `WriteZeroes` | ext4 on Linux ≥ 6.17, XFS on Linux ≥ 7.3 (port) | `create_new`, then `fallocate(fd, FALLOC_FL_WRITE_ZEROES, 0, len)`; `EOPNOTSUPP` or `EINVAL` → the `ZeroFill` steps (same guarantee, a method fallback, not a weakening) | the same call over `[0, len)`, same fallback |
| `Sparse` | btrfs, APFS (port) | first `free_space(root).available ≥ 2 × len`, else `InsufficientSpace` (the caller refuses the rotation with exit 7, [80 §2.3.3]); then `create_new` and `ftruncate(fd, len)`. btrfs: `FS_NOCOW_FL` on the empty file only if the port's measurement shows it pays; APFS: `F_PREALLOCATE` is not used | btrfs `fallocate(fd, FALLOC_FL_PUNCH_HOLE \| FALLOC_FL_KEEP_SIZE, 0, len)`; APFS `fcntl(fd, F_PUNCHHOLE, {0, 0, len})`; both need a block-aligned range, which holds when `len` is a multiple of 64 KiB (open point 12) |

- Neither call makes anything durable. The caller follows [F16]: `sync(DataAndMeta)` on the extent and `sync_dir` of the
  store root (or one `sync_group` on macOS) before the first commit in the extent is acknowledged.
- **Re-preparation sets the length for every method.** `recycle_extent(file, len, vol)` on a file shorter than `len` first
  extends it to `len` — `ZeroFill` and `WriteZeroes` by writing (or `fallocate`-ing) over `[0, len)`, which extends the
  file; `Sparse` by `ftruncate(fd, len)` (Windows: `SetFileInformationByHandle(FileEndOfFileInfo)`) before the hole
  punch, which keeps the size — so after it returns the file is exactly `len` bytes and reads as zero, whatever the method
  ([F16] P-72 step 2; pass 1, S1-24).
- **The spare extent** of [F16] P-96 is made by `create_extent` under a `tmp/extent.<nonce>` name ([F02 §5.3]), then
  `sync(DataAndMeta)`, `rename_noreplace` onto `log.<n+1>` and `sync_dir` of `tmp/` and the store root. The calls are the
  ones of this section; only the name differs.
- Plain `fallocate` without `WRITE_ZEROES` and `FALLOC_FL_ZERO_RANGE` are never used: they leave unwritten extents that
  cost about 5× in data-sync overwrite rate ([80 §2.3.3], [X17 §3.6]).
- On copy-on-write file systems any later write and any flush may still fail with disk-full (fault-model item (5)).
- `recycle_extent` was introduced for G25 (an extent reused under a new epoch is zero-filled before reuse, [AR §4.10]).
  [F16] P-74 reuses no file that held a group, so in format v1 it serves only the re-preparation in place of a
  `log.<m>` that an interrupted preparation left shorter than `len` and that never held a group ([F16] P-72 step 2).

### 4.6 `seal`

`seal(file)` makes the file **read-only on disk** ([80 §2.5] rule 2): Windows sets `FILE_ATTRIBUTE_READONLY` through
`SetFileInformationByHandle(FileBasicInfo)` with the file's current attributes plus the read-only bit (time fields 0 =
unchanged); Unix `fchmod(fd, 0o444)`. A casual `cp`, `truncate`, editor save or `O_TRUNC` open then fails with an access
error instead of crashing readers ([80 §2.5]). macOS `UF_IMMUTABLE` and Linux `chattr +i` are never used.

- Order ([80 §2.3.2]): write → `sync(DataAndMeta)` → `seal` → `sync_dir`, all before the record that names the file.
- The attribute's own durability is not a correctness condition: a sealed file whose attribute a crash lost is still
  never written by moirai (never truncated, extended, renamed over or reused, [80 §2.5] rule 3), and the size check of
  [OS/map §4] still guards every mapping.

### 4.7 `unlink`

`unlink(root, rel, retry)` removes a file's name.

- **Windows:** read the attributes; if `FILE_ATTRIBUTE_READONLY` is set, clear it (`SetFileAttributesW`), then
  `DeleteFileW` ([80 §2.5] rule 2: "GC clears the attribute before `DeleteFileW`"). Both calls take the `\\?\` form of the
  root's final path joined with `rel`. If another process has the file open with `FILE_SHARE_DELETE`, the delete
  succeeds and the file becomes **delete-pending**: its name stays until the last handle closes, and opens of it fail
  (§6.4). If another process holds it without `FILE_SHARE_DELETE`, or maps it, the call fails with error 5 or 32 [I;
  verified by the M1 `Vfs` conformance suite's AV-interference case and measurement 22's clear-then-delete row]; GC
  passes `ShareRetry::None` and leaves the file for the next GC run ([AR §4.1]). A cleared attribute on a file that could
  not be deleted is harmless: the file is already unreferenced.
- **Linux, macOS:** `unlinkat(root_fd, rel, 0)`. Unlinking needs only directory permission; open and mapped inodes stay
  valid ([80 §2.12]).
- The name removal is durable only after `sync_dir` of the parent (§4.4.3). A resurrected name after a crash is an
  unreferenced file that the orphan sweep removes ([F02]).
- `NotFound` is returned as an error; GC and the orphan sweep treat it as done.

### 4.8 `rename_noreplace`, `rename_replace`

`rename_noreplace` renames a file or a directory between two roots on one volume; a directory is never moved into its
own subtree (the caller never asks for it). `rename_replace` renames files only. Both follow [F15 §5.4] and §5.5, and
§4.9.2 uses the directory form. Neither makes anything durable: **every rename point is followed by `sync_dir` of both
parents** ([80 §2.3.2], X-F5), or one `sync_group` on macOS.

| | `rename_noreplace` (fails `AlreadyExists` if `to` exists) | `rename_replace` (atomically replaces an existing `to`) |
|---|---|---|
| Windows | `MoveFileExW(from, to, MOVEFILE_WRITE_THROUGH)` — never `MOVEFILE_REPLACE_EXISTING`, never `MOVEFILE_COPY_ALLOWED`; paths in `\\?\` form | `MoveFileExW(from, to, MOVEFILE_REPLACE_EXISTING \| MOVEFILE_WRITE_THROUGH)` |
| Linux | `renameat2(from_fd, from, to_fd, to, RENAME_NOREPLACE)`; `EINVAL` (the file system lacks it) → `Unsupported` | `renameat(from_fd, from, to_fd, to)` |
| macOS | `renameatx_np(from_fd, from, to_fd, to, RENAME_EXCL)`; `ENOTSUP` (a volume without `VOL_CAP_INT_RENAME_EXCL`) → `Unsupported` | `renameat(from_fd, from, to_fd, to)` |

- **`MOVEFILE_WRITE_THROUGH` on every Windows rename.** PLAN §6.1 #3 and [80 §2.3.1] keep the flag, besides the directory
  flush, until the post-release rig calibration shows it unnecessary; it has no measurable cost on a same-volume rename
  [M, 13 §1.4]. [F16] owns the rule; this file applies it to every rename `os::fs` and `ProjectFs` issue (open point 5).
- **No link-plus-unlink fallback for store files.** Every allowed store file system supports the no-replace rename
  (NTFS; ext4 since 3.15, XFS since 4.0, btrfs; APFS with `VOL_CAP_INT_RENAME_EXCL`), and the `init` probe checks it
  ([OS/env §5]); `Unsupported` at run time refuses the operation (exit 7). The Linux `link` + `unlink` fallback of
  [80 §2.11.1] is a `ProjectFs` behaviour for project files, whose "both names, one inode" crash state `FsIntent`
  recovery handles ([40 §3.4]).
- `rename_replace` never targets a sealed (read-only) file; on Windows that would fail with error 5.
- Errors 5 and 32 are retried per `retry` (§6.3). `EXDEV` or error 17 is `CrossDevice`.
- Windows renames take path strings; the protocol issues store renames only under the writer or maintenance byte and
  after the `retired` check, which excludes a concurrent `restore` swap (§2.2; [F16]).

### 4.9 `swap_dirs`, `swap_recover` and the swap intent

`swap_dirs(a_parent, a, b_parent, b, retry)` exchanges two directories on one volume: afterwards the name `a` holds what
`b` held, and `b` what `a` held. `restore` uses it to put a restored store in place under the writer and maintenance bytes
([AR §4.10]); `a` and `b` are single components (`RelPath` of one component).

```rust
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum SwapOutcome { Exchanged, TwoRenames }

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum SwapRecovery { NoIntent, NothingDone, RolledBack, Completed }
```

#### 4.9.1 Atomic exchange

Linux `renameat2(a_parent_fd, a, b_parent_fd, b, RENAME_EXCHANGE)`; macOS `renameatx_np(a_parent_fd, a, b_parent_fd, b,
RENAME_SWAP)`. Then `sync_dir` of both parents. Result `Exchanged`. `EINVAL`, `ENOTSUP` or `EOPNOTSUPP` (no exchange on
this file system) → the two-rename form. Windows has no exchange primitive and always uses the two-rename form.

#### 4.9.2 Two renames guarded by an intent

Let `A` = `a_parent/a`, `B` = `b_parent/b`, `T` = `a_parent/<a>.swap-old`, and the intent file `I` = `a_parent/<a>.swap`
([80 §2.3.2]: "two renames guarded by an intent file in the parent directory that `doctor` completes or rolls back").

1. If `I` or `T` exists: fail `AlreadyExists` (the caller prints "run `moirai doctor`" and exits 7).
2. Read `a_id = path_identity(A)` and `b_id = path_identity(B)`.
3. Write `I` (§4.9.3) with `create_new`, `write_at`, `sync(DataAndMeta)`; then `sync_dir(a_parent)`. The create stays
   pending until that `sync_dir`, so a crash inside this step can keep the name `<a>.swap` with lost or partial bytes;
   §4.9.4's row "`I` unreadable" handles it. Step 4 starts only after this step's flushes succeeded, so every later
   state has a readable `I`.
4. `rename_noreplace`-equivalent directory rename `A → T`; `sync_dir(a_parent)`.
5. Directory rename `B → A`; `sync_dir(b_parent)`, and `sync_dir(a_parent)` if it is another directory.
6. Directory rename `T → B`; `sync_dir(a_parent)`, and `sync_dir(b_parent)` if it is another directory.
7. `unlink(I)`; `sync_dir(a_parent)`. Result `TwoRenames`.

Directory renames use the no-replace calls of §4.8 (on Windows `MoveFileExW` with `MOVEFILE_WRITE_THROUGH` and without
`REPLACE_EXISTING`, which fails on directories anyway). On Windows a directory rename fails with error 5 or 32 while any
file inside it is open by any process, even with full sharing, or a process has its working directory inside
([13 §1.4]; WP-33); a handle on the directory itself does not block it. `retry` applies, and a failure after the bound
leaves the intent for `swap_recover` (open point 6). A failed flush embedded in a step is returned as `FlushFailed`
(§4.1) and leaves the intent for `swap_recover`.

#### 4.9.3 The swap intent file (on-disk, little-endian)

File `<a>.swap` in `a_parent`. Fixed header of 64 bytes, then three UTF-8 paths, zero padding and a checksum:

| Offset | Width | Type | Name | Meaning |
|---|---|---|---|---|
| 0 | 4 | [u8; 4] | `magic` | ASCII `MSWP` |
| 4 | 2 | u16 | `version` | 1 |
| 6 | 2 | u16 | `flags` | reserved, must be 0 |
| 8 | 24 | `FileIdentity` (§2.6) | `a_id` | identity of `A` before step 4 |
| 32 | 24 | `FileIdentity` | `b_id` | identity of `B` before step 4 |
| 56 | 2 | u16 | `a_len` | byte length of `a_path`, 1–4096 |
| 58 | 2 | u16 | `b_len` | byte length of `b_path`, 1–4096 |
| 60 | 2 | u16 | `t_len` | byte length of `t_path`, 1–4096 |
| 62 | 2 | u16 | `_` | reserved, must be 0 |
| 64 | `a_len` | UTF-8 | `a_path` | absolute path of `A` in the machine-local form of [80 §2.10] P12 (canonical, `/` separators; Windows `X:/…` with the drive letter upper-cased) |
| 64 + `a_len` | `b_len` | UTF-8 | `b_path` | absolute path of `B`, same form |
| 64 + `a_len` + `b_len` | `t_len` | UTF-8 | `t_path` | absolute path of `T`, same form |
| P = 64 + `a_len` + `b_len` + `t_len` | (8 − P mod 8) mod 8 | zero bytes | `pad` | pads to a multiple of 8 |
| P + pad | 8 | u64 | `xxh3` | XXH3-64 with seed 0 over bytes `[0, P + pad)` |
| total | P + pad + 8 | | | |

Total length = P + pad + 8. A reader accepts the file only if the length, the magic, `version = 1`, the reserved fields and
the checksum all match. Otherwise the intent is **unreadable**: `swap_recover` acts by §4.9.4's row "`I` unreadable", and
outside that row fails with `Io` ("swap intent unreadable") and changes nothing. A read that returns an error is not
this case: `swap_recover` then fails with the read's error and changes nothing. The
intent is machine-local: `FileIdentity` on Linux and macOS carries `st_dev`, which may differ after a reboot, in which case
recovery refuses rather than guesses (open point 7).

#### 4.9.4 `swap_recover`

`swap_recover(a_parent, a, retry)` is run only by `doctor` and by `restore` itself when its own swap failed ([F16] P-85
step 4). Store discovery never runs it: a lock-free reader cannot tell a crashed swap from a running one, so discovery
only probes and retries as [F16] P-86 and [F02 §3.2] state (pass 1, P1-13, S1-26, A1-18). It reads `I`
(none → `NoIntent`), then the identities of the three paths (`NotFound` = absent) and acts by this table; every rename is
followed by `sync_dir` of the parents involved, and removing `I` by `sync_dir(a_parent)`:

| `A` | `B` | `T` | State | Action | Result |
|---|---|---|---|---|---|
| present | present | absent | `I` unreadable (§4.9.3): its write in step 3 never completed (a crash, or a failed flush), so nothing was renamed, since step 4 starts only after `I` is durable | remove `I` | `NothingDone` |
| `a_id` | `b_id` | absent | nothing renamed | remove `I` | `NothingDone` |
| absent | `b_id` | `a_id` | after step 4 | rename `T → A`; remove `I` | `RolledBack` |
| `b_id` | absent | `a_id` | after step 5 | rename `T → B`; remove `I` | `Completed` |
| `b_id` | `a_id` | absent | after step 6 | remove `I` | `Completed` |
| anything else, or `I` unreadable with `A` or `B` absent or `T` present | | | unknown | change nothing; fail `Io` ("swap state unrecognised", or "swap intent unreadable"), exit 7 with the three paths | — |

After `Completed` the caller runs the remaining `restore` steps (the old store, now at `B`, gets `HEAD.retired`, [AR §4.10],
[F16]); after `RolledBack` the restore did not happen and `B` holds the restored copy intact.

### 4.10 `file_size`, `identity`, `root_identity`, `path_identity`

- `file_size(file)`: Windows `GetFileInformationByHandleEx(FileStandardInfo).EndOfFile`; Unix `fstat(fd).st_size`.
- `identity(file)` and `root_identity(root)`: Windows `GetFileInformationByHandleEx(FileIdInfo)`; Unix `fstat` →
  `(st_dev, st_ino)`; encoded as §2.6.
- `path_identity(root, rel)`: the identity of the object the name currently denotes, without opening it for data. Windows:
  open relative to the root with access `FILE_READ_ATTRIBUTES`, `FILE_FLAG_BACKUP_SEMANTICS` (so directories work) and
  full sharing, query `FileIdInfo`, close. Unix: `fstatat(root_fd, rel, &st, AT_SYMLINK_NOFOLLOW)`.

Used by the `LOCK` identity check ([OS/lock §9]) and the swap intent (§4.9).

### 4.11 `free_space`

`free_space(root)` and `Meter::free_space(path)` ([OS/README §4.3]) return the volume's available and total bytes:

| Windows | Linux | macOS |
|---|---|---|
| `GetDiskFreeSpaceExW(root final path)`: `available` = `FreeBytesAvailableToCaller`, `total` = `TotalNumberOfBytes` | `fstatvfs(root_fd)`: `available` = `f_bavail × f_frsize`, `total` = `f_blocks × f_frsize` | `fstatfs(root_fd)`: `available` = `f_bavail × f_bsize`, `total` = `f_blocks × f_bsize` |

Users: the sparse-extent early warning (§4.5); `probes guard` (WP-05's 25 GB check, PLAN §3.2) through `Meter`.

### 4.12 `advise_dontneed` and the sequential hint

Bulk passes read many files once ([80 §2.12]):

| | Windows | Linux | macOS |
|---|---|---|---|
| `OpenHint::Sequential` at open | `FILE_SEQUENTIAL_ONLY` (the `FILE_FLAG_SEQUENTIAL_SCAN` equivalent) | `posix_fadvise(fd, 0, 0, POSIX_FADV_SEQUENTIAL)` | `fcntl(fd, F_RDAHEAD, 1)` and `fcntl(fd, F_NOCACHE, 1)` |
| `advise_dontneed(file, offset, len)` behind each chunk | no-op | `posix_fadvise(fd, offset, len, POSIX_FADV_DONTNEED)` | no-op (`F_NOCACHE` already set) |

Both are hints: errors are ignored and semantics never change. **Precondition (checked by the simulator's assertion
hooks):** `advise_dontneed` is never called on a log extent that holds unflushed groups, because after a failed flush it
could evict clean-but-unwritten pages that the next flush holder must re-write ([80 §2.4.2], [80 §2.12]).

### 4.13 `counters`

`counters()` returns the process-wide `VfsCounters` of §2.7. Every operation increments its counter exactly once per OS
call it issues (a `sync_group` of three members on Windows adds to `sync_data`/`sync_meta`/`sync_dir` per member; on macOS
it adds its `fsync`s to the class counters and one to `full_barriers`). The simulator keeps the same counters per
simulated process.

---

## 5. Sharing modes and open flags

### 5.1 Windows

Store files are opened with `NtCreateFile` relative to the root handle (`OBJECT_ATTRIBUTES.RootDirectory`, name = the
`RelPath` in UTF-16 with `\` separators, `OBJ_CASE_INSENSITIVE` as Win32 does), so that every open resolves inside the
root the process opened (§2.2):

| Open | Desired access | Share access | Disposition | Create options | Attributes |
|---|---|---|---|---|---|
| root directory (`open_root`) | `FILE_LIST_DIRECTORY \| FILE_TRAVERSE \| FILE_READ_ATTRIBUTES \| SYNCHRONIZE` | read, write, delete | open | `FILE_DIRECTORY_FILE \| FILE_SYNCHRONOUS_IO_NONALERT` | — |
| directory-flush handle (`sync_dir`) | `FILE_GENERIC_READ \| FILE_GENERIC_WRITE` (Win32: `GENERIC_READ \| GENERIC_WRITE` with `FILE_FLAG_BACKUP_SEMANTICS`) | read, write, delete | open | `FILE_DIRECTORY_FILE \| FILE_SYNCHRONOUS_IO_NONALERT` | — |
| store file, `Access::Read` | `FILE_GENERIC_READ \| SYNCHRONIZE` | read, write, delete | open | `FILE_NON_DIRECTORY_FILE \| FILE_SYNCHRONOUS_IO_NONALERT` (+ `FILE_SEQUENTIAL_ONLY` for `Sequential`) | — |
| store file, `Access::ReadWrite` | `FILE_GENERIC_READ \| FILE_GENERIC_WRITE \| SYNCHRONIZE` | read, write, delete | open | as above | — |
| `create_new` | as `ReadWrite` | read, write, delete | create (fails if it exists) | as above | `FILE_ATTRIBUTE_NORMAL` |
| `LOCK` role, slot and probe handles | [OS/lock §9] | read, write, delete | open | **no** `FILE_SYNCHRONOUS_IO_*` (overlapped) | — |

- **Sharing: always `FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE`** ([AR §4.1]; [80 §2.12] "Store files'
  sharing"). Other processes may therefore read, write, rename and delete store files that moirai holds open; deletion
  makes them delete-pending (§6.4).
- **Never** `FILE_WRITE_THROUGH` / `FILE_FLAG_WRITE_THROUGH` and never `FILE_NO_INTERMEDIATE_BUFFERING` /
  `FILE_FLAG_NO_BUFFERING` ([AR §2.8]).
- Positional I/O on a synchronous handle is `ReadFile`/`WriteFile` with an `OVERLAPPED` carrying the offset (the form of
  Rust std's `seek_read`/`seek_write`).
- Handles are not inheritable. The root handle must permit relative opens [I; WP-33 test on the M0 build].

### 5.2 Linux and macOS

| Open | Flags |
|---|---|
| root directory | `open(dir, O_RDONLY \| O_DIRECTORY \| O_CLOEXEC)` |
| store file, `Read` | `O_RDONLY \| O_CLOEXEC` |
| store file, `ReadWrite` | `O_RDWR \| O_CLOEXEC` |
| `create_new` | `O_RDWR \| O_CREAT \| O_EXCL \| O_CLOEXEC`, mode `0o666` (umask applies; `seal` sets `0o444`) |
| subdirectory for `sync_dir` | `O_RDONLY \| O_DIRECTORY \| O_CLOEXEC` |

Every relative open resolves beneath the root and never through a symbolic link, as [OS/path §6] requires for project
paths: Linux `openat2(root_fd, rel, {flags, mode, resolve: RESOLVE_BENEATH \| RESOLVE_NO_SYMLINKS})` (Linux ≥ 5.6, inside
the 5.10 floor); macOS `openat(root_fd, rel, flags \| O_NOFOLLOW_ANY)`. A symbolic link where a store file should be is
`ELOOP` (or `EXDEV` from `RESOLVE_BENEATH`), reported as `Other` and exit 7: moirai never creates links in the store.

Unix has no share modes: another process with permission may truncate or rewrite a store file (fault-model item (10));
sealed files are `0o444`, so doing so takes deliberate action ([80 §2.3.5]).

---

## 6. Errors

### 6.1 `VfsError`, `VfsErrorKind`, `OsCode`

```rust
/// The raw OS error: a Win32 error code (NTSTATUS values are converted with `RtlNtStatusToDosError`, except that
/// `STATUS_DELETE_PENDING` is mapped to the kind `DeletePending` before conversion and carries 303
/// `ERROR_DELETE_PENDING`, the code §6.2 lists for the kind; `RtlNtStatusToDosError` would give 5) or an `errno`.
/// 0 when there is none.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct OsCode(pub i32);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VfsError { pub kind: VfsErrorKind, pub os: OsCode, pub call: &'static str }

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
#[non_exhaustive]
pub enum VfsErrorKind {
    NotFound, AlreadyExists, NotEmpty,
    AccessDenied, SharingViolation, DeletePending,
    DiskFull, InsufficientSpace, ReadOnlyVolume,
    Unsupported, CrossDevice, Busy, InvalidName,
    UnexpectedEof, Io, Other,
    // A durability-class flush embedded in `create_root`, `swap_dirs` or `swap_recover` failed (§4.1, §4.9).
    FlushFailed,
    // Reported by `ProjectFs` only ([OS/project §2.3]); never by `StoreFs`.
    CloudOnly, IsSymlink, IsDirectory, OutsideRoot, Stale,
}
```

`VfsErrorKind` is also the `kind` of `DurabilityFailure` (never `FlushFailed`, which only wraps one, §4.1). `#[non_exhaustive]` keeps the port free to report the same
kinds from new codes; it never lets a port add a behaviour. The last five kinds are `ProjectFs`'s ([OS/project §2.3];
pass 1, A1-33): `StoreFs` never returns them, because store files are never cloud placeholders, links or directories
where a file is expected (a link in the store is `Other`, §5.2), and store roots have no root-id check.

### 6.2 Mapping of OS codes to kinds

| Kind | Windows (Win32 code) | Linux, macOS (`errno`) | Callers' reaction ([F16], [F19]) |
|---|---|---|---|
| `NotFound` | 2 `ERROR_FILE_NOT_FOUND`, 3 `ERROR_PATH_NOT_FOUND` | `ENOENT`, `ENOTDIR` | per operation; exit 7 for a file `HEAD` names |
| `AlreadyExists` | 80 `ERROR_FILE_EXISTS`, 183 `ERROR_ALREADY_EXISTS` | `EEXIST` | per operation |
| `NotEmpty` | 145 `ERROR_DIR_NOT_EMPTY` | `ENOTEMPTY` | per operation |
| `AccessDenied` | 5 `ERROR_ACCESS_DENIED` (also the Win32 form of a delete-pending name, a mapped file's delete, and a read-only file's write or delete) | `EACCES`, `EPERM` | writers under a sandbox: exit 7 with the per-harness texts of [90 §5.3]; retried per §6.3 on the Windows rename and delete paths; from `ProjectFs::sync_dir` in a file verb's plan step: exit 7 `no_dir_flush`, nothing changed, and in intent recovery's re-barrier the intent stays open for `doctor` ([OS/project §6.2], [F16] P-71; pass 1, P1-16) |
| `SharingViolation` | 32 `ERROR_SHARING_VIOLATION`, 33 `ERROR_LOCK_VIOLATION` (a foreign whole-range lock over real bytes) | — (Unix has no share modes) | retried per §6.3; then exit 7 "file busy" |
| `DeletePending` | `STATUS_DELETE_PENDING` from `NtCreateFile`; 303 `ERROR_DELETE_PENDING` | — | §6.4 |
| `DiskFull` | 112 `ERROR_DISK_FULL`, 39 `ERROR_HANDLE_DISK_FULL`, 1295 `ERROR_DISK_QUOTA_EXCEEDED` | `ENOSPC`, `EDQUOT` | the command aborts without an acknowledgement (decision (f)); from a sync: `fail_stop` |
| `InsufficientSpace` | — (the sparse-extent early warning, §4.5; `OsCode(0)`) | same | exit 7 |
| `ReadOnlyVolume` | 19 `ERROR_WRITE_PROTECT` | `EROFS` | exit 7 |
| `Unsupported` | 50 `ERROR_NOT_SUPPORTED`, 1 `ERROR_INVALID_FUNCTION`; `STATUS_NOT_SUPPORTED`, `STATUS_INVALID_DEVICE_REQUEST`, `STATUS_INVALID_PARAMETER` from `NtFlushBuffersFileEx` | `ENOTSUP`/`EOPNOTSUPP`; `EINVAL` from a call whose flag the file system lacks (`renameat2`, `fallocate`) | store refused, exit 7 ([80 §2.3.1] no downgrade); from `ProjectFs::sync_dir` in a file verb's plan step: exit 7 `no_dir_flush`, nothing changed, and in intent recovery's re-barrier the intent stays open for `doctor` ([OS/project §6.2], [F16] P-71; pass 1, P1-16) |
| `CrossDevice` | 17 `ERROR_NOT_SAME_DEVICE` | `EXDEV` | exit 7 |
| `Busy` | 170 `ERROR_BUSY` | `EBUSY` | exit 7 "file busy" |
| `InvalidName` | 123 `ERROR_INVALID_NAME`, 206 `ERROR_FILENAME_EXCED_RANGE`; the use-time checks of §2.1 | `ENAMETOOLONG`, `EILSEQ`; the use-time checks of §2.1 | exit 2 for user input, 1 for a name moirai built |
| `UnexpectedEof` | short read in `read_exact_at` | same | per protocol ([F16]) |
| `Io` | 23 `ERROR_CRC`, 1117 `ERROR_IO_DEVICE`, 483 `ERROR_DEVICE_HARDWARE_ERROR`, and any unlisted code on a read or write | `EIO`, `EUCLEAN`, `EBADMSG`, and any unlisted code on a read or write | fault-model item (12), judged by position and by caller ([F16] P-92): below `durable_lsn`, corruption (exit 7 `store_corrupt`); at or above it, a reader's view ends there, and a writer's scan (appender, flush holder, boot recovery, `repair`) appends nothing and exits 7 `store_io_fault` |
| `Other` | any other code | any other code, including `ELOOP` and a `RESOLVE_BENEATH` `EXDEV` on an open (a symbolic link in the store, §5.2) | exit 1 or 7 per operation |
| `FlushFailed` | the `OsCode` of the failed flush embedded in `create_root`, `swap_dirs` or `swap_recover` (§4.1) | same | exit 7 with the `durability-failure` text of [F19]; no retry, and no further write, flush, create or namespace call by the process |
| `CloudOnly` | `ProjectFs` only: decided from the entry's attributes before any open (`RECALL_ON_DATA_ACCESS`, `RECALL_ON_OPEN`, `OFFLINE`); a code of the `ERROR_CLOUD_FILE_*` family if one still occurs (winerror.h has members at 358, 404, 426, 434 and 475 and gaps inside 362–400, so the family is matched by name, not by a range) | macOS `SF_DATALESS`, or the error a read returns while materialisation is off | the answer is `unverified (cloud-only)`; never hydrated ([OS/project §5.10]) |
| `IsSymlink` | `ProjectFs` only: a content read of an entry whose reparse tag is `IO_REPARSE_TAG_SYMLINK` | `ELOOP` from `O_NOFOLLOW` on a content read | the caller uses `read_link` ([OS/project §5.6]) |
| `IsDirectory` | `ProjectFs` only: 267 `ERROR_DIRECTORY`, or the attributes, on a content read or an unlink | `EISDIR` | per operation |
| `OutsideRoot` | `ProjectFs` only: an opened object's final path is not under the root (the containment check of [OS/project §5.5]) | `EXDEV` from `RESOLVE_BENEATH`, `ELOOP` from `O_NOFOLLOW_ANY` on a project open | the object is never read; the resolver treats it as not a candidate ([40 §2.4]) |
| `Stale` | `ProjectFs` only: the directory at a root's path no longer has the root's recorded id ([OS/project §2.2]) | the same check | the caller re-canonicalises the root ([OS/path §4]) |

`EINTR` never surfaces: every call is retried inside the implementation. `EAGAIN` from a regular-file read or write is
treated as `EINTR`.

### 6.3 Errors 5 and 32: bounded retry

Defender, the Search indexer, Explorer's preview and cloud clients open files without `FILE_SHARE_DELETE`; a rename or
delete then fails with error 5 or 32 while they hold the file ([08] W3, [13 §1.4]). Under `ShareRetry::Bounded { total_ms }`
the Windows implementations of `unlink`, `rename_noreplace`, `rename_replace` and the directory renames of `swap_dirs` and
`swap_recover` retry on `AccessDenied` and `SharingViolation`:

1. Attempt; on success return.
2. Otherwise sleep 1 ms, then 2, 4, 8, 16, 32 ms, then 64 ms per step, re-attempting after each sleep, until the monotonic
   time since the first attempt plus the next sleep would exceed `total_ms`; then return the last error.
3. Every attempt after the first adds one to `VfsCounters.share_retries`.

Rules:
- On Linux and macOS `retry` is ignored: there are no sharing violations, and `EBUSY`, `EACCES` and `EPERM` are not
  transient ([80 §2.11.4] rule 7).
- **Never while holding the writer or flush byte.** The writer byte is the innermost lock and nothing waits under it
  ([80 §2.2.3]); a caller that holds the writer or flush byte passes `ShareRetry::None`. The simulator's assertion hooks
  flag a bounded retry issued by a simulated process that holds either byte.
- GC deletes pass `None` and retry at the next GC ([AR §4.1]); image-export and `config` renames pass
  `Bounded { total_ms: HOLE(OS-share-retry-ms) }`; `ProjectFs` renames pass `--retry-ms` (default 1 s, [40 §3.4]).

### 6.4 Delete-pending

A Windows file deleted while another handle holds it open with `FILE_SHARE_DELETE` stays **delete-pending**: its name
exists until the last handle closes, it cannot be opened (`DeletePending`), and a new file of that name cannot be created
(`AlreadyExists`, `AccessDenied` or `DeletePending`; [F15] FM-8.3). moirai tolerates it by design:

- **Which deletes leave a name pending** (WP-33). On Windows 11 NTFS, `DeleteFileW` uses POSIX delete semantics: the
  name disappears at once, even while other handles are open, and nothing stays delete-pending. Only a classic
  `FileDispositionInfo` delete, which moirai never issues but other tools may, leaves the file pending. The rules
  below keep the lingering form, which [F15] FM-8.3 models, because an older build, another file system or another
  tool can still produce it.
- **Names are never reused.** Sealed files, extents and temporary files are created under monotonic numbers ([AR §4.1],
  [80 §2.5] rule 3), so a delete-pending name never blocks a creation.
- **Readers.** A reader that fails to open a file named by a `HEAD` it read earlier with `DeletePending`, `AccessDenied`
  or `NotFound` re-reads `HEAD` and retries once; if the newest `HEAD` still names the file, the failure is exit 7 naming
  the file ([AR §4.1]; [F16]).
- **GC** deletes only files that `HEAD` no longer names, after `gc.delete-grace` ([F17 §11.4], production 60 s) and the
  other conditions of [F16] P-77, and that no pin references ([AR §4.1]); a delete that leaves the file pending, or
  fails, is harmless and repeated at the next GC (pass 1, A1-47).
- On Linux and macOS `unlink` is immediate, and open or mapped inodes stay valid; there is no delete-pending state.

---

## Appendix A. Per-OS call mapping

| # | Operation | Windows 11 (built from M0) | Linux ≥ 5.10 (port) | macOS ≥ 14 (port) |
|---|---|---|---|---|
| A1 | `open_root` | `CreateFileW(\\?\<full path>, FILE_LIST_DIRECTORY \| FILE_TRAVERSE \| FILE_READ_ATTRIBUTES \| SYNCHRONIZE, share R\|W\|D, OPEN_EXISTING, FILE_FLAG_BACKUP_SEMANTICS)`; `GetFinalPathNameByHandleW(VOLUME_NAME_DOS)` kept | `open(dir, O_RDONLY\|O_DIRECTORY\|O_CLOEXEC)` | same as Linux |
| A2 | `create_root` | `CreateDirectoryW`; role `Store`: `GetSecurityInfo` + `SetEntriesInAclW` (`GRANT_ACCESS`, `GENERIC_ALL`, `SUB_CONTAINERS_AND_OBJECTS_INHERIT`, the token user's SID) + `SetSecurityInfo(DACL_SECURITY_INFORMATION)`; then A11 on the parent | `mkdir(dir, 0o777)`; A11 on the parent | same as Linux |
| A3 | `open`, `create_new` | `NtCreateFile` relative to the root, per §5.1 | `openat2(…, RESOLVE_BENEATH \| RESOLVE_NO_SYMLINKS)` per §5.2 | `openat(…, O_NOFOLLOW_ANY)` per §5.2 |
| A4 | `create_dir`, `remove_dir` | `NtCreateFile(FILE_CREATE, FILE_DIRECTORY_FILE)` relative to the root; `RemoveDirectoryW(\\?\…)` | `mkdirat(root_fd, rel, 0o777)`; `unlinkat(root_fd, rel, AT_REMOVEDIR)` | same as Linux |
| A5 | `list_dir` | `GetFileInformationByHandleEx(FileFullDirectoryRestartInfo`, then `FileFullDirectoryInfo)` on a directory handle | `openat` + `getdents64` (through `fdopendir`/`readdir`) | `openat` + `getattrlistbulk` or `readdir` |
| A6 | `read_at`, `read_exact_at` | `ReadFile` with `OVERLAPPED.Offset/OffsetHigh` on the synchronous handle | `pread` (retry `EINTR`) | `pread` |
| A7 | `write_at` | `WriteFile` with `OVERLAPPED` offset; loop on partial writes | `pwrite` loop | `pwrite` loop |
| A8 | `durable` = `sync(Data)` | `NtFlushBuffersFileEx(FLUSH_FLAGS_FILE_DATA_SYNC_ONLY)` | `fdatasync` | `fcntl(F_FULLFSYNC)` |
| A9 | `durable+meta` = `sync(DataAndMeta)` | `FlushFileBuffers` | `fsync` | `fcntl(F_FULLFSYNC)` |
| A10 | never used as durability | `FILE_FLAG_WRITE_THROUGH`, `FILE_FLAG_NO_BUFFERING` | `O_DSYNC`, `O_DIRECT`, `RWF_DSYNC`, `sync_file_range` | plain `fsync` as `durable`, `F_BARRIERFSYNC`, `fdatasync` |
| A11 | `durable-name` = `sync_dir` | `FlushFileBuffers` on a `BACKUP_SEMANTICS` + `GENERIC_WRITE` directory handle | `fsync(dirfd)` | `fsync(dirfd)` + `fcntl(dirfd, F_FULLFSYNC)` |
| A12 | `sync_group` | each member by its class | each member by its class | `fsync` each member, then one `F_FULLFSYNC` on the last member per volume |
| A13 | error from A8, A9, A11, A12 | `fail_stop`: one stderr line, `TerminateProcess(self, 7)` | `fail_stop`: one line, `_exit(7)` | same as Linux |
| A14 | `create_extent` | `ZeroFill` (NTFS) | `WriteZeroes` (ext4 ≥ 6.17, XFS ≥ 7.3) else `ZeroFill`; btrfs `Sparse` | `Sparse` (`ftruncate`) with the 2 × `len` free-space check |
| A15 | `recycle_extent` | write zeros | `FALLOC_FL_WRITE_ZEROES` or write zeros; btrfs `PUNCH_HOLE \| KEEP_SIZE` | `F_PUNCHHOLE` |
| A16 | `seal` | `SetFileInformationByHandle(FileBasicInfo)` with `FILE_ATTRIBUTE_READONLY` added | `fchmod(fd, 0o444)` | `fchmod(fd, 0o444)` |
| A17 | `unlink` | clear `FILE_ATTRIBUTE_READONLY` (`SetFileAttributesW`), `DeleteFileW`; delete-pending tolerated; retry per §6.3 | `unlinkat(root_fd, rel, 0)` | same as Linux |
| A18 | `rename_noreplace` | `MoveFileExW(from, to, MOVEFILE_WRITE_THROUGH)` | `renameat2(…, RENAME_NOREPLACE)`; `EINVAL` → `Unsupported` | `renameatx_np(…, RENAME_EXCL)`; `ENOTSUP` → `Unsupported` |
| A19 | `rename_replace` | `MoveFileExW(from, to, MOVEFILE_REPLACE_EXISTING \| MOVEFILE_WRITE_THROUGH)` | `renameat` | `renameat` |
| A20 | `swap_dirs` | two renames guarded by `<a>.swap` (§4.9.2) | `renameat2(…, RENAME_EXCHANGE)`, else §4.9.2 | `renameatx_np(…, RENAME_SWAP)`, else §4.9.2 |
| A21 | `file_size` | `GetFileInformationByHandleEx(FileStandardInfo)` | `fstat` | `fstat` |
| A22 | `identity`, `root_identity`, `path_identity` | `GetFileInformationByHandleEx(FileIdInfo)`; for a path, a relative open with `FILE_READ_ATTRIBUTES` + `BACKUP_SEMANTICS` | `fstat`; `fstatat(…, AT_SYMLINK_NOFOLLOW)` | same as Linux |
| A23 | `free_space` | `GetDiskFreeSpaceExW` | `fstatvfs` | `fstatfs` |
| A24 | bulk-pass hints | `FILE_SEQUENTIAL_ONLY`; `advise_dontneed` no-op | `POSIX_FADV_SEQUENTIAL`; `POSIX_FADV_DONTNEED` | `F_RDAHEAD`, `F_NOCACHE`; `advise_dontneed` no-op |
| A25 | handle inheritance | never inheritable | `O_CLOEXEC` on every descriptor | same as Linux |
| A26 | sharing | `FILE_SHARE_READ \| WRITE \| DELETE` on every open; errors 5/32 retried (§6.3); delete-pending (§6.4) | none; `EBUSY`/`EACCES`/`EPERM` not retried | same as Linux |

---

## Coverage

The rows of `COVERAGE.md` that cite this file ([F01 §2.7]).

| Item | Part covered here | Section |
|---|---|---|
| `60-AR-Log-extents` (log extents read as zero beyond the tail, created per [80 §2.3.3]) | `create_extent` per file system (`ZeroFill` on NTFS); the extent size is [F17]'s | §4.5 |
| `60-I2-FM(2)` (fault-model item (2)) | namespace durability on the OS side (`sync_dir`) | §4.4.3 |
| `60-I2-FM(3)` (item (3): a failed flush) | fail-stop | §4.4.5 |
| `60-I2-FM(5)` (item (5): disk-full) | writes and disk-full; the error kinds | §4.3, §6.2 |
| `60-I2-FM(8)` (item (8)) | sharing violations and the bounded retry; delete-pending. The lock-release lag is [OS/lock §7.3]'s | §6.3, §6.4 |
| `60-I2-FM(10)` (item (10)) | sealing: read-only on disk; the mapping rules are [OS/map]'s | §4.6 |
| `60-I2-FM(12)` (item (12): read errors) | read errors reported as `Io` | §6.2 |
| `60-I2-PD(a)` (protocol decision (a)) | the OS-side error policy | §4.4.5 |
| `60-I2-PD(c)` (decision (c)) | the `HEAD` barrier outside the writer byte | §4.4.6 |
| `60-I2-PD(f)` (decision (f): disk-full) | disk-full on write | §4.3 |
| `60-I2-PD(l)` (decision (l)) | the barrier flush | §4.4.6 |
| `60-AU-Vfs-classes` (audit row "`Vfs`/`ProjectFs`": the classes) | the classes as OS calls; the per-OS call mapping | §4.4, Appendix A |
| `60-AU-Vfs-renames` (the same row: renames) | `rename_noreplace`, `rename_replace`; `swap_dirs` and the swap intent | §4.8, §4.9 |
| `60-PA-(h)` (the paragraph after the audit rows, (h)) | the rename forms, `MOVEFILE_WRITE_THROUGH` on Windows; the protocol points are [F16]'s | §4.8 |
| `60-AU-CrossPlatform` (the "Cross-platform" summary row) | X-F5, whose parts are the next row | — |
| `X-F5` ([80] X-F5) | the classes, no downgrade, the error policy, the `HEAD` barrier; renames; `swap_dirs`; the per-OS calls. The durability tags of records are [F05]'s, the fault-model items [F15]'s | §4.4, §4.8, §4.9, Appendix A |

## Holes

| Id | What | Decided by | Candidates | Constraint the value must meet |
|---|---|---|---|---|
| `OS-share-retry-ms` | `total_ms` of `ShareRetry::Bounded` for image-export renames, `packed-refs`/loose-ref replace-renames, the store `config` rename (§6.3) and the clean-up unlinks of the `init` probe ([OS/env §5] step 6) | measurement 8 (loose-object create + rename under Defender, n = 1,000; WP-54) and measurement 15 (rename of one file and of a 1,000-file directory under Defender; WP-55), filled by WP-81a | 1,000 ms (the `--retry-ms` default [40 §3.4] gives `file mv`) | ≥ the measured p99 time Defender or the indexer keeps a freshly written file open without `FILE_SHARE_DELETE`, loaded; never spent while the caller holds the writer or flush byte; small enough that `image export` stays inside its own budget ([AR §8.3]) |

Referenced, owned elsewhere: `store.log-extent-bytes` (init-fixed, [F17]; the extent `len` of §4.5).

## Open points for the review

| # | Point | Resolution in this file | For |
|---|---|---|---|
| 1 | [80 §2.1]'s `os::fs` list lacks operations the protocol and `doctor` need | added `open_root`, `create_root`, `create_dir`, `remove_dir`, `list_dir` (orphan sweep, `init`), `read_exact_at`, `fail_stop` (the one error policy as a diverging call, so the simulator can crash at that point), `recycle_extent` (G25), `swap_recover` (`doctor` for the swap intent), `root_identity`/`path_identity` (`LOCK` identity, swap intent), `free_space` (sparse early warning, `Meter`), `advise_dontneed` ([80 §2.12] bulk passes), `counters` ([60 §5.1] counts). Closes the PLAN §3.3 "`os::fs` additions" gap together with [OS/README §3] | R-REV-P, WP-30 |
| 2 | P10 allows `\\?\` or handle-relative opens on Windows | opens are handle-relative (`NtCreateFile` with `RootDirectory`), so a process cannot mix files of two stores across a `restore` swap; renames and deletes stay on `\\?\` paths and are issued only under a lock that excludes the swap (§2.2) | R-REV-P, WP-16 |
| 3 | The swap intent is a new on-disk file (`<a>.swap` in the parent of the swapped directory) whose bytes no design document fixes | frozen here (§4.9.3) because `swap_dirs` is an `os::fs` operation; [F02]'s file list must name `<a>.swap` and `<a>.swap-old` | WP-10 (F02), R-REV-P |
| 4 | `NtFlushBuffersFileEx(DATA_SYNC_ONLY)` refused by a volume | refuse the store; no fallback to `FlushFileBuffers` even though it is stronger, so one location always takes one path and the `init` probe tests the call that runs | R-REV-P |
| 5 | `MOVEFILE_WRITE_THROUGH`: PLAN §6.1 #3 names `file mv`; [80 §2.3.1]'s `durable-name` row reads as every Windows rename | applied to every Windows rename (store, export, swap and `ProjectFs`); [F16] (WP-16) owns the rule and may narrow it only to `file mv` if it records why | WP-16 |
| 6 | On Windows a directory cannot be renamed while any file inside it is open ([13 §1.4]); `restore` swaps the store directory while it holds the writer and maintenance bytes through handles to `LOCK` inside it | [F16] must order the swap so that the restoring process's own `LOCK` handles live outside the swapped directory during the renames, or accept that a Windows `restore` swap fails (exit 7, intent left for `swap_recover`) while any MCP server holds the store open. **WP-33's answer (spec sync 2a):** a directory can be renamed while a handle on the directory itself is open, but not while any file inside it is open, even with full sharing (error 5); [F16] P-85 already closes its handles before the swap | — |
| 7 | `FileIdentity` on Unix uses `st_dev`, which can change across a reboot | `swap_recover` then refuses (state unrecognised) rather than guesses; Linux and macOS use the atomic exchange on every allowed file system, so the intent path is rare there | R-REV-P |
| 8 | Whether a Windows delete of a file that another process maps fails (error 5) or becomes delete-pending | either outcome is tolerated (GC retries next run). **WP-33's answer (spec sync 2a):** the GC delete (clear read-only, then `DeleteFileW`) of a mapped sealed file succeeds, and the mapping stays readable; measurement 22's clear-then-delete row still records it under load | WP-52 |
| 9 | The retry schedule of §6.3 (1 ms doubling to 64 ms) is not given by any design document | fixed here as an implementation contract (no format effect); only its total is a hole | R-REV-P |
| 10 | `create_root` makes the store directory durable (`durable-name` on the parent) although [80 §2.3.2] has no `init` row | added so that a power loss after `init` cannot leave acknowledged commits in a directory whose name was lost; [F16] may list it as a protocol point | WP-16 |
| 11 | `remove_dir` and the `init`-refusal cleanup | `init` removes the directories it created when the guard refuses ([OS/env §5]) | R-REV-P |
| 12 | Hole punching (`recycle_extent` on btrfs and APFS) needs a block-aligned length | [F17] keeps `store.log-extent-bytes` a multiple of 64 KiB in production and in the test profile (64 MiB and 64 KiB today, [60 §2.5]) | WP-16 (F17) |
| 14 | One `RelPath` type for store and project paths ([OS/path] open point 1) | the type and its grammar are [OS/path §2.1]'s; this file adds only use-time checks (§2.1) and resolves every Unix store open beneath the root without following links (§5.2), as [OS/path §6] does for project paths | WP-17b, WP-30 |
| 13 | The sealed-file order of §4.4.6 puts a temporary-name rename before `seal` | [80 §2.3.2] orders write, `durable+meta`, `seal`, `durable-name` and allows a rename followed by `durable-name`; renaming before `seal` keeps every rename on a writable file, and the file is not yet "sealed" in [80 §2.5]'s sense until a durable record names it | WP-16 |
| 15 | Spec sync 2a (WP-31, WP-33): a `durable-name` flush embedded in `create_root` or `swap_dirs` returned `VfsError` and could never reach `fail_stop`; a crash inside §4.9.2 step 3 could leave an unreadable `<a>.swap` that `swap_recover` refused forever; §4.8 allowed files only although §4.9.2 renames directories | **closed:** option (a): an embedded flush failure removes `create_root`'s new directory and returns the new kind `FlushFailed` (exit 7, no retry, no further writes; §4.1, §6.1, §6.2), and `swap_dirs`/`swap_recover` return it with their intent kept; §4.9.4 removes an unreadable intent when `A` and `B` are present and `T` is absent (`NothingDone`), which is sound because step 4 starts only after `I` is durable; §4.8 lets `rename_noreplace` move a directory, never into its own subtree, and keeps `rename_replace` for files ([F15 §5.4]) | — |
