# OS layer: environment guard (`os::env`)

| Field | Value |
|---|---|
| Title | `os::env` — where a store may live: per-OS allow-lists, the classification at every open, the full probe at `init`/`restore`, the OS-version check and `doctor`'s warnings |
| Status | draft, pass 1 pending |
| Work package | WP-17a (role R-SPEC-P), part 1 of WP-17 |
| Sources | [80 §2.1] (`os::env` row); [80 §2.6] (decision, allow-list table, "Also" bullets); [80 §2.2.2] (macOS 14 floor, "Unsupported" row); [80 §2.3.1] (no downgrade, `ENOTSUP` refuses); [80 §2.13] (minimum versions); [80 §3.1] X-F6, X-F5; [AR §4.1] rules paragraph; [AR §14]; [AR §11] #15 (Dev Drive); [AR §10] risk 17; [60 §2.5] protocol decision (m); [60 §5.2] item 22; [90 §5] (sandboxes, exit-7 texts, the store directory's ACE); research reports [X17 §3.7], [X18 §9] as cited by [80] |

---

## 1. Scope

A store is used only where every one of its guarantees holds ([80 §2.6]). `os::env` is the `EnvGuard` sub-trait of `Vfs`
([OS/README §4.1]). It answers three questions:

- **Is this location allowed?** A per-OS **allow-list in which every allowed file system is crash-gated**. An unknown
  or ungated type is refused with its name. Adding a type is a code change backed by that type's crash evidence (the GT4
  and GT15 variants on a volume of that type), never a configuration key ([80 §2.6], X-F6). In M0–M11 NTFS is admitted on
  the GT1/GT3 fault-model evidence and GT4 on real NTFS; its GT15 variant runs with the OS-crash rig, deferred to after the
  release ([AR §10] risk 17).
- **Does this location honour the calls moirai needs?** At `init` and `restore` the full probe performs a durable write,
  a directory sync, a byte-range lock at 2^62 and a no-replace rename (§5). A refusal of any of them refuses the store;
  nothing is downgraded ([80 §2.3.1]).
- **Is this OS new enough?** The OS-version check (§6).

When it runs:

| Moment | Depth | Who |
|---|---|---|
| `init`, `restore` (target directory) | `probe_store`: full classification, OS-version check, durable-write, lock and rename probes (§5) | the command |
| every open of a store, by every process (readers included) | `classify(Open)` — one volume query plus the cheap checks of §4 (est. ≤ 20 µs, [80 §2.6]) — and the OS-version check | store discovery ([F02]) |
| `doctor` | `classify(Full)` without writes, plus `doctor_warnings` (§8) | `doctor` |

A refusal is exit 7 ("store unavailable", [AR §7.1]) with the refusal's reason and, where there is one, the file system's
name; the texts are [F19]'s.

---

## 2. API

```rust
pub trait EnvGuard: VfsTypes {
    /// Classifies the volume and location of an open store root (role `Store`).
    fn classify(&self, store: &Self::Root, depth: ClassifyDepth) -> Result<Classification, VfsError>;
    /// `init` and `restore` only: `classify(Full)`, the OS-version check and the probes of §5, in `store`'s `tmp/`.
    /// `Err` only for failures that are not about the location (for example `tmp/` missing).
    fn probe_store(&self, store: &Self::Root) -> Result<ProbeOutcome, VfsError>;
    /// Every open.
    fn check_os_version(&self) -> Result<OsVersion, Refusal>;
    /// `doctor` only; never a refusal.
    fn doctor_warnings(&self, store: &Self::Root) -> Vec<EnvWarning>;
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ClassifyDepth { Open, Full }

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Classification { Local(StoreVolume), Refused(Refusal) }

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct StoreVolume {
    pub fs: FsKind,
    /// How `create_extent`/`recycle_extent` make zeros on this file system ([OS/fs §4.5]).
    pub extent_method: ExtentMethod,
    /// The volume is mounted read-only: readers work, writers exit 7 (`ReadOnlyVolume`).
    pub read_only: bool,
    /// A removable or external drive: allowed, and a `doctor` warning (§8).
    pub removable: bool,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum FsKind {
    Ntfs,
    Ext4, Xfs, Btrfs,      // port phase
    Apfs,                  // port phase
    /// Test builds only (§9): tmpfs `--ephemeral` or a LazyFS FUSE mount. `moirai-os` returns it only under its
    /// `test-host` feature, which the product root never enables.
    Ephemeral,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ExtentMethod { ZeroFill, WriteZeroes, Sparse }

/// A file-system name as the OS reports it (Windows `GetVolumeInformationByHandleW`, macOS `f_fstypename`), or the
/// `statfs` magic as `0x` + 8 lower-case hex digits (Linux). ASCII, at most 32 bytes.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct FsName { /* private: len u8, bytes [u8; 32] */ }

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Refusal {
    /// A local file system that is not on this OS's allow-list (ungated or unsupported).     reason id `fs-type`
    FileSystem { name: FsName },
    /// A network or remote volume.                                                           `network`
    Network { name: FsName },
    /// A UNC path, or a path whose final path resolves to UNC (a mapped network drive, `\\wsl$`, `\\wsl.localhost`). `unc`
    Unc,
    /// Another kernel's file system (WSL2's 9p mounts under `/mnt/*`).                       `cross-kernel`
    CrossKernel { name: FsName },
    /// A cloud-managed folder.                                                               `cloud`
    Cloud { kind: CloudKind },
    /// Any FUSE mount (sshfs, virtiofs, the gRPC-FUSE mounts of Docker Desktop and Lima, macFUSE). `fuse`
    Fuse { name: FsName },
    /// Any overlayfs (a container's own layer).                                              `overlay`
    Overlay,
    /// A file system without stable media (tmpfs, ramfs, a RAM disk).                         `volatile`
    Volatile { name: FsName },
    /// The full probe saw a durability call refused (for example `ENOTSUP` from `F_FULLFSYNC`). `no-durable-flush`
    NoDurableFlush { call: &'static str, os: OsCode },
    /// The full probe saw byte-range locks refused.                                          `no-byte-locks`
    NoByteLocks { os: OsCode },
    /// The full probe saw the no-replace rename refused.                                     `no-noreplace-rename`
    NoNoReplaceRename { os: OsCode },
    /// The OS is older than the floor of §6.                                                 `os-too-old`
    OsTooOld { found: OsVersion, minimum: OsVersion },
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum CloudKind {
    /// Windows: a registered Cloud Files sync root (OneDrive and every other provider), a OneDrive folder, or
    /// `RECALL_ON_*`/`OFFLINE` attributes on the store directory or an ancestor.
    WindowsCloudFiles,
    /// macOS: iCloud Drive (`~/Library/Mobile Documents`).
    ICloudDrive,
    /// macOS: a File Provider root (`~/Library/CloudStorage/*`).
    FileProvider,
    /// macOS: a directory with `SF_DATALESS`.
    Dataless,
    /// macOS: `~/Desktop` or `~/Documents` managed by iCloud "Desktop & Documents".
    ICloudDesktopDocuments,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct OsVersion { pub major: u32, pub minor: u32, pub build: u32 }

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProbeReport { pub volume: StoreVolume, pub os: OsVersion }

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProbeOutcome { Admitted(ProbeReport), Refused(Refusal) }

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EnvWarning {
    /// Windows "turn off write-cache buffer flushing" set for the store's disk.
    FlushingDisabled,
    /// ext4 mounted with `barrier=0` or `nobarrier`.
    NoBarrier,
    /// A device whose `queue/write_cache` is set to write-through while it has a volatile cache.
    WriteCacheForcedWriteThrough,
    /// A removable or external drive (flushes may be ignored by some USB bridges).
    RemovableDrive,
    /// An OS release that is allowed but outside the supported and tested set (§6).
    UntestedOsRelease { found: OsVersion },
}
```

The reason ids in the comments (`fs-type`, `network`, …) are the stable tokens [F19]'s refusal texts use.

---

## 3. Allow-lists and refusals

| OS | Allowed (each crash-gated) | Refused, with the reason printed |
|---|---|---|
| Windows | **NTFS** (M1; M0–M11 on GT1/GT3 and GT4 evidence) | **ReFS and Dev Drive** (not yet gated; a Dev Drive is ReFS, so a repository moved onto one cannot hold a store, [AR §11] #15) → `FileSystem`; **FAT, FAT32, exFAT** (no journal, so a rename is not crash-atomic) → `FileSystem`; any other file-system name → `FileSystem`; `GetDriveTypeW = DRIVE_REMOTE` → `Network`; **UNC paths**, including `\\wsl$` and `\\wsl.localhost` (two kernels), and a path whose final path resolves to UNC, as a mapped network drive does → `Unc` (a `subst` drive of a local folder resolves locally and is allowed); **OneDrive roots and `RECALL_ON_*` directories**, and every other registered Cloud Files sync root → `Cloud`; `DRIVE_RAMDISK` → `Volatile` (open point 6) |
| Linux (port) | **ext4, XFS, btrfs** (`statfs.f_type`; each gated in the port phase) | ZFS, f2fs, bcachefs (not yet gated) → `FileSystem`; ext2 and ext3 (share ext4's magic; told apart by `mountinfo`, §4.2) → `FileSystem`; NFS → `Network`; SMB and CIFS → `Network`; FUSE (sshfs; virtiofs and the gRPC-FUSE mounts of Docker Desktop and Lima report it) → `Fuse`; 9p (WSL2 `/mnt/*`) → `CrossKernel`; tmpfs and ramfs → `Volatile`; **every overlayfs** → `Overlay` (a dev container must bind-mount its workspace from an allowed file system; a repository cloned inside the container's own layer cannot hold a store); anything not on the list → `FileSystem` |
| macOS (port) | **APFS**, case-insensitive and case-sensitive, with `MNT_LOCAL` (gated in the port phase) | HFS+ (not yet gated) → `FileSystem`; any volume without `MNT_LOCAL` (SMB, AFP, NFS, WebDAV) → `Network`; macFUSE → `Fuse`; FAT and exFAT → `FileSystem`; iCloud Drive (`~/Library/Mobile Documents`) → `Cloud(ICloudDrive)`; File Provider roots (`~/Library/CloudStorage/*`) → `Cloud(FileProvider)`; `SF_DATALESS` directories → `Cloud(Dataless)`; `~/Desktop` and `~/Documents` when iCloud "Desktop & Documents" manages them → `Cloud(ICloudDesktopDocuments)`; `ENOTSUP` from `F_FULLFSYNC` → `NoDurableFlush` |

**Also** ([80 §2.6]):
- **Cross-kernel sharing is refused both ways.** A Windows moirai and a WSL2 moirai on one store would not exclude each
  other, because their byte-range locks live in different kernels. A repository used from WSL2 keeps its store on the WSL
  ext4 disk. A store is touched by one kernel only.
- **Dev Drive.** Every repository that holds a store stays on NTFS in M0–M11. A Defender exclusion on an NTFS folder is
  unaffected.

---

## 4. Classification at every open (`classify(Open)`)

The store root is already open ([OS/fs §4.1]); every step works on its handle.

### 4.1 Windows (built from M0)

1. `GetVolumeInformationByHandleW(root)` → the file-system name and the volume flags. `FILE_READ_ONLY_VOLUME` sets
   `read_only`.
2. `GetFinalPathNameByHandleW(root, VOLUME_NAME_DOS)`; a result beginning `\\?\UNC\` → `Unc`.
3. `GetVolumePathNameW(final path)` → the volume root; `GetDriveTypeW(volume root)`: `DRIVE_REMOTE` → `Network`;
   `DRIVE_RAMDISK` → `Volatile`; `DRIVE_REMOVABLE` sets `removable`.
4. `GetFileInformationByHandleEx(root, FileAttributeTagInfo)`: `FILE_ATTRIBUTE_RECALL_ON_OPEN` (`0x0004_0000`),
   `FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS` (`0x0040_0000`) or `FILE_ATTRIBUTE_OFFLINE` (`0x0000_1000`) set, or a cloud
   reparse tag (`(tag & !0x0000_F000) == 0x9000_001A`, `IO_REPARSE_TAG_CLOUD` and its variants) → `Cloud(WindowsCloudFiles)`.
5. File-system name exactly `NTFS` → `Local { fs: Ntfs, extent_method: ZeroFill, … }`; any other name → `FileSystem`
   with that name (`ReFS`, `FAT`, `FAT32`, `exFAT`, `UDF`, …).

### 4.2 Linux (port)

1. `fstatfs(root_fd)`: `f_type` decides by Appendix A's magic table; `ST_RDONLY` in `f_flags` sets `read_only`.
2. `0xEF53` is accepted as ext4 at open; `classify(Full)` confirms it through `/proc/self/mountinfo` (the entry whose
   `major:minor` equals the root's `st_dev` must have file-system type `ext4`; `ext2`, `ext3` → `FileSystem`). Reading
   `mountinfo` at every open would cost more than the ≤ 20 µs estimate on hosts with many mounts (open point 4).
3. `extent_method`: ext4 and XFS `WriteZeroes` (with `create_extent`'s per-call fallback to zero-fill), btrfs `Sparse`.

### 4.3 macOS (port)

1. `fstatfs(root_fd)`: `f_fstypename` `apfs` with `MNT_LOCAL` in `f_flags` → `Local { fs: Apfs, extent_method: Sparse }`;
   `hfs` → `FileSystem`; `msdos`, `exfat` → `FileSystem`; a FUSE type (`macfuse`, `osxfuse`) → `Fuse`; no `MNT_LOCAL` →
   `Network`; anything else → `FileSystem`. `MNT_RDONLY` sets `read_only`.
2. `st_flags & SF_DATALESS` on the root → `Cloud(Dataless)`.
3. The root's canonical path (`fcntl(F_GETPATH)`) under `~/Library/Mobile Documents` → `Cloud(ICloudDrive)`; under
   `~/Library/CloudStorage/` → `Cloud(FileProvider)`.

### 4.4 The OS-version check (§6) runs at every open as well.

---

## 5. The full probe at `init` and `restore` (`probe_store`)

`probe_store(store)` runs after `create_root` made the store directory and `create_dir` made `tmp/`, and before `LOCK`
and `HEAD` exist ([80 §2.6]: "classification, a durable write, `sync_dir`, the OFD or `LockFileEx` probe at 2^62, and on
macOS an `F_FULLFSYNC` that must not return `ENOTSUP`"). For `restore` it runs on the directory the restored store is
built in.

1. `classify(Full)` — §4 plus the full-depth checks:
   - Windows: `CfGetSyncRootInfoByPath(final path, CF_SYNC_ROOT_INFO_BASIC, …)` succeeds (the path lies under a
     registered Cloud Files sync root: OneDrive or any other provider) → `Cloud(WindowsCloudFiles)`; the attribute test of
     §4.1 step 4 on every ancestor directory up to the volume root; the canonical path under a folder named by the
     environment variables `OneDrive`, `OneDriveConsumer` or `OneDriveCommercial` → `Cloud(WindowsCloudFiles)` [I;
     verified by measurement 22's OneDrive row].
   - Linux: the `mountinfo` confirmation of §4.2 step 2.
   - macOS: the iCloud "Desktop & Documents" detection — the canonical path resolving into `~/Library/Mobile Documents`
     under `F_GETPATH_NOFIRMLINK`, or the File Provider domain of the folder, whichever the port-phase probe confirms [I].
   A refusal ends the probe.
2. `check_os_version()`.
3. **Durable-write probe:** `create_new(tmp/probe)`; `write_at` 4,096 bytes; `sync(Data)`; `sync(DataAndMeta)`;
   `sync_dir(tmp)`; `sync_dir(store)`. A `DurabilityFailure` here is **not** passed to `fail_stop` — no commit is at
   stake — but becomes `NoDurableFlush { call, os }`. On macOS `sync(Data)` is `F_FULLFSYNC`, so an `ENOTSUP` there is
   caught by this step.
4. **Lock probe:** open `tmp/probe` as a lock handle ([OS/lock §9.2] flags), try-acquire the byte 2^62, release it, close.
   An error other than "busy" → `NoByteLocks { os }` (`LOCK` does not exist yet, so the probe never touches it).
5. **Rename probe:** `create_new(tmp/probe.2)`; `rename_noreplace(tmp/probe → tmp/probe.1)` must succeed and
   `rename_noreplace(tmp/probe.1 → tmp/probe.2)` must fail with `AlreadyExists`; `Unsupported`, or a replace instead of
   the failure, → `NoNoReplaceRename { os }`.
6. **Clean-up:** `unlink` every probe file; `sync_dir(tmp)`.
7. Return `Admitted(ProbeReport { volume, os })`; any refusal of steps 1–5 returns `Refused(refusal)` after step 6.

On any refusal, `init` removes the probe files and the directories it created (`remove_dir`) and exits 7 with the
refusal. The probe files `tmp/probe`, `tmp/probe.1` and `tmp/probe.2` are names [F02] lists (fixed ASCII words and
decimal numbers, X-F10); the orphan sweep removes any left by a crash.

---

## 6. The OS-version check

| OS | Source | Refuse (exit 7, `OsTooOld`) | Allow with `UntestedOsRelease` (a `doctor` warning) | Supported and tested |
|---|---|---|---|---|
| Windows | `RtlGetVersion` (WDK-documented; not the manifest-dependent `GetVersionEx`) | build < 17134 (Windows 10 1803, the technical floor of the APIs used: `FileCaseSensitiveInfo`, [80 §2.13]) | 17134 ≤ build < 22000 (Windows 10: out of support since 2025-10-14, never tested) | build ≥ 22000 (Windows 11, x64); the gated build is the owner's 25H2, build 26200 |
| Linux | `uname().release`, `major.minor` | < 5.10 | — | ≥ 5.10, 64-bit, static musl |
| macOS | `sysctlbyname("kern.osproductversion")` | < 14.0 | — | ≥ 14.0, arm64 |

- macOS 14 is enforced twice: the binary is linked with `MACOSX_DEPLOYMENT_TARGET=14.0` (Rust's default is 11), and this
  check refuses an older system at open, so XNU's private OFD range 10.13–13 is never reached ([80 §2.2.2], [81] m15).
- Architecture (x64, arm64) and word size (64-bit) are compile-time properties of the target and are not checked.

---

## 7. Access, sandboxes and read-only volumes

- **Readers always work where the location is allowed**: they open read-only and take no locks ([AR §2.2], [90 §5.2]).
  A read-only volume (`read_only`) serves readers and refuses writers with `ReadOnlyVolume` (exit 7).
- **A writer that cannot write the store** — `LOCK` or the log cannot be opened for writing (`ERROR_ACCESS_DENIED`,
  `EROFS`, `EPERM`, `EACCES`) — is not an environment refusal: the location is fine and the process lacks rights. It exits
  7 with the per-harness three-line text of [90 §5.3] (the equivalent MCP call, the `result.v1` fallback, the owner's fix,
  never "request escalation"). Detection points: `LockError::AccessDenied` ([OS/lock §4]) and `AccessDenied` from opening a
  log extent ([OS/fs §6.2]).
- **Writes outside the store** (the default image destination beside the main worktree, a `backup DIR` outside the store,
  the `restore` swap) need write access a sandboxed Bash command lacks on Linux and macOS; a sandboxed CLI exits 7 printing
  the exact `allowWrite` entry for the destination's parent ([80 §2.6] m11); texts in [F19].
- **Codex's elevated Windows sandbox** runs commands as separate local users: files they create under the store inherit
  the store directory's owner ACE, which `create_root` adds ([OS/fs §4.1], [90 §5.4]); their liveness probes answer
  `Unknown` ([OS/lock §8]).

---

## 8. `doctor` warnings (never refusals)

None of these can be detected reliably, so none refuses ([80 §2.6], [X17 §3.7]):

| Warning | Windows | Linux (port) | macOS (port) |
|---|---|---|---|
| `FlushingDisabled` | the disk's "turn off Windows write-cache buffer flushing" setting (`CacheIsPowerProtected` / `UserWriteCacheSetting` under the disk's `Device Parameters\Disk` registry key) [X17 §3.7] | — | — |
| `NoBarrier` | — | ext4 mounted `barrier=0` or `nobarrier` (`mountinfo` super options) | — |
| `WriteCacheForcedWriteThrough` | — | `/sys/block/<dev>/queue/write_cache` reads `write through` on a device that has a volatile cache | — |
| `RemovableDrive` | `DRIVE_REMOVABLE` (§4.1 step 3) | `/sys/block/<dev>/removable` = 1 | external volume; detection chosen by the port probe [I] |
| `UntestedOsRelease` | Windows 10 builds 17134–21999 (§6) | — | — |

---

## 9. Test builds

Only a test build of `moirai-os` (feature `test-host`, which the product root never enables, [OS/README §2.4]) may admit,
on explicit request of the test, a tmpfs `--ephemeral` store or a LazyFS FUSE mount ([80 §2.6] "Tests"; [80 §5.2]). Such
a location classifies as `FsKind::Ephemeral` with `ZeroFill`. Product builds refuse both (`Volatile`, `Fuse`). Windows
tests at M0 need no admission: they run on NTFS.

---

## Appendix A. Per-OS detection

| Item | Windows 11 (built from M0) | Linux ≥ 5.10 (port) | macOS ≥ 14 (port) |
|---|---|---|---|
| File-system identity | `GetVolumeInformationByHandleW` name: `NTFS` allowed | `fstatfs.f_type`: `0xEF53` ext4 (confirmed as `ext4` in `mountinfo` at full depth), `0x58465342` XFS, `0x9123683E` btrfs allowed | `fstatfs.f_fstypename`: `apfs` with `MNT_LOCAL` allowed |
| Refused types | `ReFS` (Dev Drive), `FAT`, `FAT32`, `exFAT`, any other name | ZFS `0x2FC12FC1`, f2fs `0xF2F52010`, bcachefs `0xCA451A4E`; NFS `0x6969`; SMB `0x517B`; CIFS `0xFF534D42`; SMB2 `0xFE534D42`; FUSE `0x65735546`; 9p `0x01021997`; tmpfs `0x01021994`; ramfs `0x858458F6`; overlayfs `0x794C7630`; any other magic | `hfs`, `msdos`, `exfat`, FUSE types, no `MNT_LOCAL`, any other name |
| Remote, UNC, cross-kernel | `DRIVE_REMOTE`; final path `\\?\UNC\…` (incl. `\\wsl$`, `\\wsl.localhost`, mapped drives) | NFS, SMB, CIFS → `Network`; 9p → `CrossKernel` | no `MNT_LOCAL` |
| Cloud | open: `RECALL_ON_OPEN`, `RECALL_ON_DATA_ACCESS`, `OFFLINE` attributes or a cloud reparse tag on the store directory; full: `CfGetSyncRootInfoByPath`, the attributes on every ancestor, the `OneDrive*` environment folders | — (no standard; untrusted types are refused anyway) | `~/Library/Mobile Documents`, `~/Library/CloudStorage/*`, `SF_DATALESS`, iCloud-managed `~/Desktop`/`~/Documents` |
| Volatile | `DRIVE_RAMDISK` | tmpfs, ramfs | — |
| Read-only volume | `FILE_READ_ONLY_VOLUME` | `ST_RDONLY` | `MNT_RDONLY` |
| Durable-write probe (§5 step 3) | `NtFlushBuffersFileEx(DATA_SYNC_ONLY)`, `FlushFileBuffers`, directory `FlushFileBuffers` | `fdatasync`, `fsync`, `fsync(dirfd)` | `F_FULLFSYNC` (must not be `ENOTSUP`), `fsync(dirfd)` + `F_FULLFSYNC` |
| Lock probe (§5 step 4) | `LockFileEx` at 2^62 on `tmp/probe` | `F_OFD_SETLK` at 2^62 | `F_OFD_SETLK` (90) at 2^62 |
| Rename probe (§5 step 5) | `MoveFileExW(MOVEFILE_WRITE_THROUGH)` | `renameat2(RENAME_NOREPLACE)` | `renameatx_np(RENAME_EXCL)` |
| OS version | `RtlGetVersion` build ≥ 17134 (≥ 22000 supported) | `uname` ≥ 5.10 | `kern.osproductversion` ≥ 14.0 |
| Verified at M0 | measurement 22: NTFS, ReFS, exFAT, a `subst` drive, a UNC path, a OneDrive folder | port-phase probes | port-phase probes, incl. both Claude Code sandboxes |

---

## Holes

| Id | What | Decided by | Candidates | Constraint the value must meet |
|---|---|---|---|---|
| — | none in this file (the allow-lists and refusals are frozen by X-F6; measurement 22 verifies them on Windows without choosing a value) | — | — | — |

## Open points for the review

| # | Point | Resolution in this file | For |
|---|---|---|---|
| 1 | [80 §2.6] names "OneDrive roots and `RECALL_ON_*` directories" without a detection method | the Cloud Files sync-root API plus attributes plus the `OneDrive*` folders; this refuses every Cloud Files provider (Dropbox, iCloud for Windows, …), the Windows counterpart of the macOS File Provider rule; measurement 22 verifies the OneDrive row | R-REV-P, WP-52 |
| 2 | The open-time check must stay near ≤ 20 µs | at open only the store directory's own attributes and reparse tag are checked for cloud management; the sync-root API, the ancestors and the environment folders run at `init`, `restore` and `doctor`. A repository moved into a cloud folder after `init` is caught at open only if its store directory carries the attributes; `doctor` catches the rest | R-REV-P |
| 3 | [80 §2.13] states "Windows 11" as the minimum and 1803 as the technical floor, but no refusal threshold | refuse below build 17134; allow Windows 10 with a `doctor` warning; supported and tested from 22000 | R-REV-P, owner visibility (V2) |
| 4 | ext2 and ext3 share ext4's `statfs` magic | told apart through `mountinfo` at full depth only (`init`, `restore`, `doctor`), so an ext3 volume mounted after `init` is caught by `doctor`, not at open | R-REV-P (port) |
| 5 | The probe's file names | `tmp/probe`, `tmp/probe.1`, `tmp/probe.2`; [F02] lists them | WP-10 |
| 6 | A Windows RAM disk formatted NTFS passes the name-based allow-list although it cannot provide `durable` | refused as `Volatile` (`DRIVE_RAMDISK`), the counterpart of Linux's tmpfs refusal under X5; [80 §2.6]'s table does not list it, so the review confirms or removes the row | R-REV-P |
| 7 | [OS/fs §4.4.5] makes every durability failure a `fail_stop` | inside `probe_store` a failure is a refusal instead, since no commit is at stake; nowhere else | R-REV-P |
| 8 | macOS detection of iCloud-managed `~/Desktop`/`~/Documents` and of external volumes | left to the port-phase probe, as [80 §2.6] says ([I]) | port phase |
| 9 | `FsName` for Linux carries the magic in hex, not a name | the refusal text prints the magic and, where `mountinfo` was read, the type name | WP-18 (F19 texts) |
