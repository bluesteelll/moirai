# 19 — Cross-platform file identity, change tracking and paths (R4 resolver on Linux and macOS)

*Research for the owner requirement of 2026-09-26: moirai must work on macOS and Linux as well as Windows, and all three are first-class. This lens covers **file identity, change tracking and paths** for the R4 file-link resolver ([40] §2.4, §2.6, §4.3, §4.7). It gives the Linux and macOS equivalents of every Windows mechanism the design uses, the hazards that differ, and a platform abstraction that keeps **one on-disk format and one resolver semantics** on all three OSes. Other lenses cover locks, durability, IPC, liveness and shell quoting; this report touches them only where the resolver depends on them.*

*Date: 2026-09-26. Design stage; nothing is implemented. Probes and downloaded sources were kept locally (probe scripts are not published). No repository was modified. **WSL is not installed** on the owner's machine (`wsl -l -v` → "The Windows Subsystem for Linux is not installed") [M], so **no Linux or macOS behaviour was measured**. Every Linux and macOS claim below comes from man pages, vendor documentation or kernel/XNU/HFS/libc/git source read for this report. Tags: **[M]** measured here, **[D]** documented (man page, vendor doc, spec), **[S]** read in source code, **[C]** third-party claim, **[I]** my inference.*

**Documents this report builds on.** [09] `docs/research/09-file-identity-os-level.md`; [13] `docs/research/13-file-ops-agent-integration.md`; [40] `docs/research/design/40-file-links-design.md`; [60] `docs/research/design/60-roadmap.md`; [AR] `docs/ARCHITECTURE-RESEARCH.md`; [50] `docs/research/design/50-query-language-design.md`.

---

## 0. Executive summary

**Short answer.** Linux gives the resolver less than Windows, and macOS gives it about the same or more. The resolver's rules can stay the same on all three OSes. What changes per OS is which evidence sources exist and what they cost.

1. **Linux `(st_dev, st_ino)` behaves like the NTFS file ID for identity** [D/S/I]. It survives rename and move within one mount. It is replaced on every rename-over save: atomic temp+rename saves, `sed -i`, Claude Code `Edit`/`Write` (reported on macOS with measured inode numbers [C, claude-code#92419]), and git checkout, which unlinks the file and re-creates it with `open(O_CREAT|O_EXCL)` [S, git `entry.c`]. There are four differences from NTFS:
   - there is no tunneling;
   - a rename sets the moved inode's **ctime** on ext4, btrfs and XFS [S];
   - ext4 **reuses the lowest free inode number** [S], but stamps each new inode with a random 32-bit generation [S], which the unprivileged `name_to_handle_at` exposes [D/S];
   - rename fails with `EXDEV` across mount points **even for the same filesystem mounted twice** (bind mounts) [D], across btrfs subvolumes [S] and for overlayfs lower-layer directories [D]. `mv` then silently copies and deletes, which gives the files new inodes.
2. **Linux has no unprivileged "id → path" call and no persistent change journal.** `open_by_handle_at` needs `CAP_DAC_READ_SEARCH` [D]. The later relaxation still needs capabilities and covers directory handles only [S]. inotify and fanotify only work while a process stays resident. Unprivileged fanotify is limited to inode marks [D]. btrfs tree-search and send need `CAP_SYS_ADMIN` [S]. `zfs diff` needs snapshots and a delegated permission [D]. **A daemon-less Linux resolver therefore has four inputs: path, inode (through a scoped walk), content and git.** E2 does not exist on Linux. E3 and E3d become a bounded search, which this report calls the **changed-directory frontier**. It works because POSIX updates the parent directory's mtime on every entry change [D], the same property git's untracked cache relies on [D].
3. **macOS has an `OpenFileById` equivalent.** `fsgetpath(fsid, objid)` (10.13+) returns the path of a file id without privilege. For a non-root caller the kernel checks search permission on every path component [S, XNU `fsgetpath_internal`]. `openbyid_np` is reserved for platform binaries or holders of an entitlement [S]. `getattrlistbulk` returns name, `ATTR_CMN_FILEID`, `ATTR_CMN_PARENTID`, times and sizes for a whole directory in one call [D], which makes it the equivalent of `FileIdExtdDirectoryInfo`. APFS allocates file-system object ids from a per-volume counter (`apfs_next_obj_id`) [D, APFS Reference]. HFS+ and APFS both declare persistent object ids [D].
4. **macOS FSEvents is a better USN journal for moirai.** Events carry **paths**, and since 10.13 they also carry the **inode**. Streams are per device, with 64-bit event ids and a `sinceWhen` replay that works after a reboot, followed by a `HistoryDone` sentinel [D]. They are advisory. Clients must rescan on `MustScanSubDirs` or dropped events and check the per-device UUID [D]. A non-root client sees only events in directories it can reach [D]. Rename halves can arrive out of order; they are paired by inode [C]. The cost of a replay has not been measured.
5. **macOS document ids survive safe-saves.** The APFS spec requires an implementation to keep the document id when the inode at a path is replaced [D]. The HFS+ source transfers it on rename-over and through a per-thread delete/create tombstone [S]. It exists only on files with the owner-settable `UF_TRACKED` flag [S], and setting that flag writes metadata. That makes it an owner decision, like NTFS object ids, but it covers the case object ids miss: rename-over, which is how agent edits save.
6. **Creation time means different things on each OS.** On macOS, `clonefile` (`cp -c`, and Finder duplicates on APFS [C]) copies every attribute, including creation time [D]. `ATTR_CMN_CRTIME` can be written [D]. So **equal creation time is not corroboration on macOS**. The resolver must use the clone indicators (`EF_MAY_SHARE_BLOCKS`, `ATTR_CMNEXT_CLONEID`) and `ATTR_CMN_ADDEDTIME` instead. On Linux, userspace cannot set the birth time [I], but it may be missing (`stx_mask`) [D].
7. **Paths follow three different equivalence rules**:
   - NTFS is case-insensitive per directory and normalization-*sensitive*: NFC and NFD `café.txt` were two files [M];
   - Linux is byte-exact, except in casefold directories, which compare NFD plus case fold [D];
   - APFS is case-insensitive by default and normalization-insensitive but normalization-preserving [D]. HFS+ stores NFD [D].

   On macOS, git's init/clone probe sets `core.precomposeUnicode=true` [S]. Git for Windows 2.54 rejects `a\b.txt`, `CON.txt`, `aux`, `x.`, `y `, `a:b.txt` and `q?.txt` as invalid paths, and a checkout of `ok/Name.txt` plus `ok/name.txt` on Windows keeps only one of the two [M]. The stored path key must therefore follow git's spelling everywhere, with macOS precomposition for untracked names. The `PATHIDX` fold must include normalization. The root path must be canonicalized per OS. This session's own environment lists both `D:\tmp` and `d:\tmp` as working directories [M].
8. **Recommended abstraction.** One `ProjectFs` trait with a per-volume **capability record**. One tagged `OsFileId` (57 B packed) and one tagged `JournalCursor` in the runtime tables. The resolver consumes **normalized evidence records**, so rules, states and thresholds are identical everywhere. A capability that is missing turns a source off; it never changes a rule. The GT17 pattern matrix runs on NTFS, ext4, XFS, btrfs (including a subvolume boundary), case-insensitive APFS and case-sensitive APFS.
9. **The image has a path portability bug.** `schema/queries/<name>.moi` uses the query name as a file name. LQ identifiers allow upper-case letters and back-quoted arbitrary text [50 §7], so two queries whose names differ only in case, or names that contain `:` or `\`, cannot be checked out on Windows or macOS.

---

## 1. Environment and method

| Item | Value |
|---|---|
| Owner machine | Windows 11 Home Single Language 10.0.26200, NTFS [M, 09 §1] |
| WSL | **not installed** (`wsl -l -v`, `wsl --status` → exit 50, "not installed") [M]. Installing it would change system configuration, so I did not install it |
| macOS | not available |
| Measured here (Windows) | NTFS normalization and case-variant behaviour (`probe/norm_probe.py`); Git for Windows path validity and case-collision checkout (`git update-index --cacheinfo`, `git clone` in a scratch repository, deleted afterwards) |
| Sources read | Linux master: `fs/ext4/{namei,ialloc,super}.c`, `fs/libfs.c`, `fs/xfs/libxfs/xfs_dir2.c`, `fs/xfs/xfs_super.c`, `fs/btrfs/{inode,ioctl,super}.c`, `fs/fhandle.c`, `include/uapi/linux/{fs,limits}.h`, `Documentation/filesystems/overlayfs.rst`, `Documentation/admin-guide/ext4.rst`, `Documentation/filesystems/tmpfs.rst`. XNU main: `bsd/vfs/{vfs_syscalls,vfs_lookup,kpi_vfs,vfs_cache,doc_tombstone}.c`, `bsd/sys/{stat,syslimits}.h`. Apple HFS `core/hfs_vnops.c`, Apple `copyfile.c`, Apple Libc `stdlib/FreeBSD/realpath.c`. git master: `entry.c`, `compat/precompose_utf8.c`, `Documentation/config/core.adoc`. The `FSEvents.h` header from the macOS 11.3 SDK (phracker mirror). The Apple File System Reference (2020-06-22) PDF, converted to text. Local copies were kept with the probes (not published) |

Source line numbers change with every commit, so this report names functions, not lines.

---

## 2. Which R4 mechanisms are OS-specific today

This is where [40] and [60] depend on Windows, with the equivalents found below.

| [40] mechanism | Windows source | Linux equivalent | macOS equivalent | Section |
|---|---|---|---|---|
| File id in `FILEOBS` (`FILE_ID_128` + 64-bit volume serial) | `FileIdInfo` | `(stx_dev, stx_ino)`, plus the generation from `name_to_handle_at` where available | `ATTR_CMN_FILEID` (= `st_ino`) + volume UUID | 3.1, 4.1 |
| Parent-directory id (E3d) | `FileIdExtdDirectoryInfo` parent / directory's own id | directory inode (`d_ino` from `getdents64`) | `ATTR_CMN_PARENTID` | 3.3, 4.1 |
| `OpenFileById` + `GetFinalPathNameByHandleW` (E3, E3d) | unprivileged [M, 09] | **none unprivileged**; scoped search (frontier) | `fsgetpath()` unprivileged [S] | 3.3, 4.4 |
| Bulk enumeration with ids | `FileIdExtdDirectoryInfo` | `getdents64` (`d_ino`, `d_type`) + `statx` when needed | `getattrlistbulk` | 3.1, 4.1 |
| Read-path stat (`GetFileAttributesExW`, no id) | 17–67 µs [M, 13] | `statx` returns the inode for free | `lstat`/`getattrlist` return the id for free | 7 |
| USN journal replay (E2) | unprivileged read; D: has no journal [M, 09] | **none** | **FSEvents** per-device history | 3.4, 4.5 |
| Creation time (copy rule, E7) | tunneled on rewrites; copies get a new one | `stx_btime` (may be absent), not settable [I] | `st_birthtime`, **copied by clones, settable** [D] | 3.2, 4.2 |
| ChangeTime (E7) | NTFS ChangeTime | ctime, **set by rename** on ext4/btrfs/XFS [S] | ctime (behaviour on rename not verified); `ATTR_CMN_ADDEDTIME` [D] | 3.1, 4.2 |
| NTFS object ids / tunneling (not used by default) | `FSCTL_CREATE_OR_GET_OBJECT_ID` | none | **document ids** (`UF_TRACKED`) | 4.3 |
| Case sensitivity per directory | `FileCaseSensitiveInfo` | `FS_CASEFOLD_FL` per directory; mount-level for vfat/exFAT/NTFS3/CIFS | `VOL_CAP_FMT_CASE_SENSITIVE` per volume | 3.5, 4.6 |
| Unicode normalization | none (NFC ≠ NFD) [M] | none, except casefold directories (NFD) [D] | APFS insensitive and preserving; HFS+ stores NFD [D] | 4.6, 9 |
| Cloud placeholders | `RECALL_ON_*` attributes | no standard | `SF_DATALESS` + `IOPOL_TYPE_VFS_MATERIALIZE_DATALESS_FILES` [D/S] | 4.7 |
| Recycle Bin rule | `$Recycle.Bin` | freedesktop `~/.local/share/Trash`, `$topdir/.Trash-$uid` [D] | `~/.Trash`, `/.Trashes/<uid>` [I] | 7 |
| Sharing violations 32/5 (pattern-matrix row 21) | yes [M, 13] | **none**; instead `EXDEV`, `EBUSY`, `EACCES` | none; also `EPERM` from TCC and SIP [I] | 7 |
| Root/tree identity (`TREES` key = BLAKE3 of the exact top-level) | textual | symlinks, bind mounts | `/tmp` → `/private/tmp`, firmlinks, case-insensitive spellings | 9 |
| [60] "Unix `Vfs` and `ProjectFs`: excluded (owner priority)" | — | now required | now required | 8 |

---

## 3. Linux

### 3.1 `(dev, ino)` semantics

- **Rename and move within one mount keep the inode.** Rename changes directory entries only; if the new path exists, it is replaced atomically [D, rename(2)]. So `mv`, `git mv`, `os.rename` and directory renames keep the inode of the file and of every descendant [I from the rename semantics; the same as NTFS in 09 §3].
- **Every replace-by-new-file makes a new inode.** That includes temp+rename saves such as Claude Code `Edit`/`Write`: the reporter of [anthropics/claude-code#92419](https://github.com/anthropics/claude-code/issues/92419) measured the inode going from 5588409 to 5588677 on macOS [C], and the tool's code path does not depend on the OS [I]. It also includes `sed -i`, `os.replace`, JetBrains safe write, vim, and git. vim's `'backupcopy'` defaults to `auto`, which renames the original and writes a new file whenever renaming has no side effects [D, vimhelp], so vim usually changes the inode on Unix [I]. On Windows the MSYS2 vim build kept the id [M, 09]. git checkout removes the old file and creates a new one with `open(path, O_WRONLY|O_CREAT|O_EXCL)` (`create_file` in `entry.c`) [S]. It does this on every OS, so the resolver's git rules carry over.
- **A rename sets ctime.** ext4's `ext4_rename` carries the comment "Like most other Unix systems, set the ctime for inodes on a rename" and sets it on the moved inode and on any replaced one [S]. btrfs calls `simple_rename_timestamp`, which sets the ctime of both directories and of the moved inode [S, `fs/libfs.c`]. XFS sets `XFS_ICHGTIME_CHG` on the source inode in `xfs_dir_rename` [S]. POSIX requires only the parents' mtime and ctime to change, and says some implementations update the renamed file's ctime and some do not [D]. **Consequence:** on Linux, "ctime newer than the last settle" catches moved files whose mtime did not change. That gives E7 a cheap filter that NTFS provides through ChangeTime [I].
- **Inode reuse.** ext4's `find_inode_bit` takes the next zero bit of the group's inode bitmap; it skips recently deleted inodes only when the filesystem has **no** journal [S]. A freed inode number is therefore handed out again quickly [I]. Under repeated atomic saves the inode numbers of one path can alternate between a few values [I]. Every new ext4 inode gets `i_generation = get_random_u32()` [S], and the ext4 file handle is `generic_encode_ino32_fh`, which holds the inode and the generation [S]. `name_to_handle_at` needs no privilege [D] and so returns a **reuse-safe identity** without opening the file. XFS derives inode numbers from the on-disk location, so they are reused as well [I]. btrfs inode numbers are unique only within a subvolume: its documentation says to use the pair `subvolumeid:inodenumber` [D, btrfs docs].
- **A different `st_dev` changes what a move is.**
  - **Bind mounts:** rename(2) fails with `EXDEV` across mount points "even if the same filesystem is mounted on both" [D]. `mv` then copies and deletes: new inodes, new birth times [I].
  - **btrfs subvolumes:** `btrfs_getattr` reports `stat->dev = root->anon_dev` (one device number per subvolume) and fills `STATX_SUBVOL` [S]. `btrfs_rename` returns `-EXDEV` when source and destination are in different subvolumes, unless the renamed object is itself a subvolume link [S]. A snapshot is a new subvolume with the same inode numbers [D].
  - **overlayfs** (containers, dev containers): directories report the overlay's `st_dev`. Non-directories may report the lower or upper layer's `st_dev`/`st_ino`, and both can change during the object's lifetime, unless all layers share one filesystem or `xino` is on [D, overlayfs.rst]. Renaming a lower or merged directory returns `EXDEV` by default, and `mv` copies it recursively; with `redirect_dir` the rename works [D].
  - **Mount points:** `d_ino` of a mount-point entry differs from `st_ino` [D, readdir(3)].
- **`d_ino` and `d_type` from `getdents64`** give ids and types without a stat. Only some filesystems (btrfs, ext2/3/4 named) fully support `d_type`, and `DT_UNKNOWN` must be handled [D, readdir(3)].
- **Filesystems whose inode numbers are not reliable:** vfat, exFAT and most network or FUSE filesystems synthesize or proxy inode numbers [I]. The resolver should trust ids only on a known list of filesystem types (§8.2).

### 3.2 Birth time

- `statx(2)` (Linux 4.11, glibc 2.28) returns `stx_btime` when the filesystem fills it. `stx_mask` says which fields were filled [D]. Rust's `Metadata::created()` returns `statx` btime on Linux ≥ 4.11 and an `Err` where it is unavailable [D, Rust std docs].
- **Userspace cannot set a birth time on Linux.** `utimensat` sets only atime and mtime [I from the API; no btime setter exists]. So equal btime with a different inode cannot come from a copy tool. It also cannot come from tunneling, which Linux does not have. In practice, equal btime with a different inode never happens, and a same-inode move is caught by the inode itself [I].
- Birth time may be missing on some filesystems and configurations. The resolver must treat "no btime" as "no evidence" [I].

### 3.3 Re-opening by identity, and inode → path

| Call | Privilege | Use for moirai |
|---|---|---|
| `name_to_handle_at(dfd, name, handle, &mnt_id, flags)` | none [D] | yes: a reuse-safe id (inode + generation on ext4 [S]); `AT_HANDLE_FID` (6.5) for identification only; `AT_HANDLE_MNT_ID_UNIQUE` (6.12) [D]. Mount ids from mountinfo are reused and are not persistent [D] |
| `open_by_handle_at(mount_fd, handle, flags)` | `CAP_DAC_READ_SEARCH` [D] | **no** |
| relaxation "fhandle: relax open_by_handle_at() permission checks" (commit 620c266f) | still needs `CAP_DAC_READ_SEARCH` in the caller's user namespace plus `CAP_SYS_ADMIN` over the filesystem or mount namespace; **directory handles only** (`O_DIRECTORY`) [S] | **no** for an unprivileged CLI on the host |
| `readlink("/proc/self/fd/N")` | none | path of a descriptor already open; it does not help find a file whose path is unknown [I] |

**Consequence.** Linux has no unprivileged equivalent of `OpenFileById`. E3 and E3d must search, and the search must be bounded. §8.3 specifies the **changed-directory frontier**, which limits the search to directories whose entry set changed since the last settle.

### 3.4 Change tracking: nothing persistent without privilege

| Mechanism | Resident process needed | Privilege | Notes |
|---|---|---|---|
| inotify | yes | none | Not recursive: one watch per directory [D]. `IN_MOVED_FROM`/`IN_MOVED_TO` share a cookie, but pairing is "inherently racy" and one half can be missing [D]. Overflow is `IN_Q_OVERFLOW` [D]. Network filesystems and pseudo-filesystems are not covered [D]. `max_user_watches` defaults to 1 % of RAM within [8192, 1048576] since 5.11 [D, commit 92890123]. A new subdirectory can fill up before its watch exists [D] |
| fanotify, unprivileged (5.13+, 5.10.220) | yes | none, with limits | Only **inode** marks: `FAN_MARK_MOUNT` and `FAN_MARK_FILESYSTEM` are refused. File-handle reporting (`FAN_REPORT_FID`) is required. No permission events. Bounded queue and mark counts [D, fanotify_init(2)]. `FAN_REPORT_DFID_NAME` since 5.9; `FAN_RENAME` since 5.17 [D]. The handles cannot be opened without `CAP_DAC_READ_SEARCH` (§3.3) |
| btrfs `find-new` / `send --no-data` | no | `CAP_SYS_ADMIN` (`btrfs_ioctl_tree_search{,_v2}`, `btrfs_ioctl_send`) [S] | no |
| `zfs diff` | no | a snapshot plus the delegated `diff` permission [D, zfs-allow(8)] | reports renames (`R`) [D]; creating a snapshot per settle is a system change |
| audit (`auditd`) | yes | root | no |

**Verdict [I].** On Linux the lazy resolver cannot replay history. Its inputs are the path, the inode (from a scoped walk), timestamps (ctime and parent-directory mtime, §8.3), content and git. A live inotify watcher in the MCP server could at most mark directories dirty while a session runs, the same role [40 §4.7] allows `ReadDirectoryChangesW`, and it pays one watch per directory.

### 3.5 Paths on Linux

- Names are arbitrary bytes except `/` and NUL. `NAME_MAX` is 255 bytes and `PATH_MAX` is 4096, including the NUL [S, `uapi/linux/limits.h`]. `PATH_MAX` is a per-syscall limit, not a filesystem limit; deeper trees need `openat`-relative walks [I].
- **Non-UTF-8 names are legal**, and git stores them as bytes. moirai's `path` type is UTF-8 (R-1), so such a file cannot be linked. It must be refused at link time and skipped as a candidate, just as Windows names with unpaired surrogates are refused [40 §2.4].
- **Case-insensitive directories exist on Linux.** ext4 with the `casefold` feature makes a directory case-insensitive when `+F` is set on it while it is empty. Comparison normalizes to NFD and then compares bytes; the encoding defaults to UTF-8 12.1 [D, admin-guide/ext4.rst]. tmpfs supports `casefold` mounts with per-directory `+F` [D, tmpfs.rst]. userspace reads the state with `FS_IOC_GETFLAGS` → `FS_CASEFOLD_FL` or with `FS_IOC_FSGETXATTR` → `FS_XFLAG_CASEFOLD`. The flag can also appear on non-directories when a filesystem derives case-insensitivity from the mount [S, `uapi/linux/fs.h`]. vfat, exFAT, NTFS3 and CIFS mounts are case-insensitive at mount level [I].

---

## 4. macOS

### 4.1 File ids, parent ids and bulk enumeration

- `ATTR_CMN_FILEID` is a 64-bit id, unique within the mounted volume and equal to `st_ino`. `ATTR_CMN_PARENTID` is the parent directory's id. `ATTR_CMNEXT_LINKID` is distinct per hard link and persistent on volumes with `VOL_CAP_FMT_PERSISTENTOBJECTIDS`, "such as HFS+ and APFS" [D, getattrlist(2)].
- APFS assigns file-system object ids from `apfs_next_obj_id`, "the next identifier that will be assigned" [D, APFS Reference]. The reference says explicitly that transaction ids and document ids are never reused. I found no such sentence for object ids, but a per-volume counter makes reuse unlikely in practice [I]. That is a better position than NTFS MFT slots, one of which was reused 165 times in 2,000 create/delete cycles [M, 09 §2.1], or ext4's lowest-free rule (§3.1). Each APFS inode also stores `parent_id` and `create_time` on disk [D].
- **"fileID vs inode".** On APFS and HFS+ the POSIX inode number, `ATTR_CMN_FILEID` and `d_ino` are the same value [D]. Apple's warning that a file's ID may change after a reboot [D, File System Programming Guide, cited in 09 §6] is about file-reference URLs, `NSURLFileResourceIdentifierKey` and volumes without persistent ids (FAT, SMB) [I]. The resolver should read `VOL_CAP_FMT_PERSISTENTOBJECTIDS` and `VOL_CAP_FMT_PATH_FROM_ID` per volume and treat ids as absent where they are not set.
- **`getattrlistbulk`** (OS X 10.10+) returns the requested attributes for many entries of a directory in one call. `ATTR_CMN_NAME` and `ATTR_CMN_RETURNED_ATTRS` are mandatory, and not every attribute is available for every entry [D]. One call returns name, object type, file id, parent id, size, modification time, creation time, added time, extended flags and document id. It is the macOS counterpart of `FileIdExtdDirectoryInfo` [I].
- **Readdir order.** APFS directory records are keyed by a hash of the name on normalization-insensitive volumes [D, APFS Reference `j_drec_hashed_key_t`]. Enumeration order therefore differs from NTFS's collation order and from ext4's hash-tree order. The resolver must sort candidates by exact bytes before any tie-breaking (§8.5).

### 4.2 Clones, copies and creation time

- **`clonefile(2)`** (OS X 10.12+) creates a new file that shares data blocks with the source. Its "attributes and extended attributes" are identical to the source's, except ownership, setuid/setgid bits and (optionally) ACLs [D]. The clone gets a **new file id** [I: it is a new inode; APFS marks it `INODE_WAS_CLONED` [D]], but it **keeps the creation time** [I from "identical attributes"; to be measured]. `cp -c` uses `clonefile` [D, cp(1)], and Finder's Duplicate on APFS is widely reported to clone [C].
- **`copyfile` with `COPYFILE_STAT`** sets only the modification and access times on the copy (`fsetattrlist` with `ATTR_CMN_MODTIME | ATTR_CMN_ACCTIME` in `copyfile_stat`) [S]. `cp -p` preserves mtime, atime, flags, mode, owner, ACLs and extended attributes; creation time is not listed [D, cp(1)].
- **HFS+ clamps creation time.** When `utimes` sets an mtime earlier than the creation time, HFS+ lowers the creation time to the mtime ("ensure that the creation time is always at least as old as the modification time") [S, `hfs_vnops.c`]. APFS behaviour is not documented [I].
- **Creation time can be written.** `ATTR_CMN_CRTIME` is marked "(read/write)" [D].
- **Clone indicators:** `ATTR_CMNEXT_EXT_FLAGS` with `EF_MAY_SHARE_BLOCKS` ("may share blocks with another file"), `ATTR_CMNEXT_CLONEID` (identifies the data stream), `ATTR_CMNEXT_CLONE_REFCNT` [D]. FSEvents also has `kFSEventStreamEventFlagItemCloned` (10.13) [D, FSEvents.h].
- **`ATTR_CMN_ADDEDTIME`**: the time the object "was created or renamed into its containing directory" [D]. On macOS this answers "arrived in this directory since the last settle" directly, without relying on ctime.

**Consequence for [40 §4.3]'s copy rule.** On macOS the line "q came from E4 and q.creation = FILEOBS.creation → exact" is unsafe. Take `cp -c plan.md plan-v2.md` (a same-volume clone with equal creation time), and later the original is deleted. E4 then finds `plan-v2.md` with an equal `oid` and an equal creation time, and would re-bind the link to a copy the owner made on purpose. On macOS, equal creation time must count only when the candidate has no clone indicator, and even then only as STRONG (§7). On Linux the line is sound, because birth time cannot be copied, but it rarely fires: a same-mount move keeps the inode, and E3 finds it first [I].

### 4.3 Document ids: the macOS analogue of object ids, which also survives rename-over

- `ATTR_CMN_DOCUMENT_ID` (32-bit) is assigned by the kernel "to track the data regardless of where it gets moved". It "survives safe saves" and is "sticky to the path it was assigned to" [D, getattrlist(2)]. The APFS spec says implementations "must preserve the document identifier when the inode at that path is replaced" (`INO_EXT_TYPE_DOCUMENT_ID`). Document ids are allocated from `apfs_next_doc_id` and never reused [D, APFS Reference].
- **How HFS+ implements it (the only public source) [S, `hfs_vnops.c`, `doc_tombstone.c`]:**
  - A document id is generated only for an item with `UF_TRACKED` set (`hfs_should_generate_document_id`).
  - **Rename-over** (`rename(tmp, tracked)`): when the destination is tracked and the source has no id, the destination's id and `UF_TRACKED` move to the source inode. This is exactly the Claude Code `Edit`/`Write` and `sed -i` shape [S].
  - **Delete then create, or rename-away then create**, in the same directory under the same name: the kernel saves a per-thread "tombstone" (`doc_tombstone_get()` uses `current_uthread()`), and the next create or rename-in by **the same thread** inherits the id [S]. git's checkout (unlink, then `O_EXCL` create in the same thread when `checkout.workers` = 1) fits this shape [I].
  - Copies do not get the id [I].
- `UF_TRACKED` (0x40) lies inside `UF_SETTABLE` (0x0000ffff), so **the file's owner can set it** [S, `sys/stat.h`]. The same header adds: "We no longer issue notifications for deletes or renames for files which have UF_TRACKED set" [S]. What those notifications are is not clear. **FSEvents behaviour for tracked files must be measured before anyone relies on both together.**
- **Assessment [I].** Document ids would make edit-then-move exact on macOS, and within a scoped walk they would find a moved and edited file whose inode changed. That is the 5 % case [40 §0.3] leaves as a proposal. The price is writing a flag onto the user's files: ctime changes, and `git status` should stay clean because git does not store flags. [40] DR9 forbids such writes by default, so this is an owner decision, parallel to NTFS object ids [09 §9.5 decision 3]. The ids are volume-local, they are not exported, and a `volume_key` must qualify them.

### 4.4 Re-opening by identity, and id → path

| Call | Privilege | Notes |
|---|---|---|
| `fsgetpath(buf, len, &fsid, objid)` (10.13+) | **none**. For non-root callers the kernel sets `BUILDPATH_CHECKACCESS` (`bpflags = vfs_context_suser(ctx) ? BUILDPATH_CHECKACCESS : 0`, where `suser()` returns 0 only for root), so the caller needs search permission on each component. A MAC hook (`mac_vnode_check_fsgetpath`) can deny it inside a sandbox [S, `vfs_syscalls.c`; D, fsgetpath(2)] | **The `OpenFileById` + path equivalent.** Returns the firmlinked path by default (`FSOPT_NOFIRMLINKPATH` asks for the other form). `EINVAL` above 8192 bytes [D]. The volume needs `VOL_CAP_FMT_PATH_FROM_ID` [D] |
| `openbyid_np(fsid, objid, flags)` | `vfs_context_can_open_by_id`: platform binary or the open-by-id entitlement [S] | **no** |
| `/.vol/<fsid>/<id>` paths | normal permission checks after translation (`vfs_getrealpath`). The kernel comment says future OS versions "may not support them" [S, `vfs_lookup.c`] | legacy; prefer `fsgetpath` |
| `fcntl(fd, F_GETPATH)` / `F_GETPATH_NOFIRMLINK` | none | path of an open descriptor [D] |

### 4.5 FSEvents as E2

**What is documented** (Apple File System Events Programming Guide and the `FSEvents.h` header, macOS 11.3 SDK):
- A persistent per-volume database of changes lets a client see what changed while it was not running [D]. Streams can be per host or **per device**. Per-device event ids increase monotonically on that disk, and paths are relative to the volume root. Apple advises per-device streams for software that persists state [D].
- **Persisting state.** Store the last event id and the device UUID from `FSEventsCopyUUIDForDevice`. A different UUID means the volume was reformatted or replaced, or its history purged. A stored id higher than the current one means a restore, a wrap or a purge. Either way, rescan [D].
- **Replay.** With `sinceWhen`, historical events come first, then an event with `kFSEventStreamEventFlagHistoryDone`, then live events [D, header]. `kFSEventStreamCreateFlagFullHistory` (10.15) delivers the whole first chunk that contains `sinceWhen`, because events near an unclean restart can otherwise be skipped [D, header].
- **Granularity.** The default is directory-level [D]. `kFSEventStreamCreateFlagFileEvents` (10.7) gives per-item flags: Created, Removed, Renamed, Modified, InodeMetaMod, IsFile/IsDir/IsSymlink, IsHardlink and IsLastHardlink (10.10), Cloned (10.13) [D]. `kFSEventStreamCreateFlagUseExtendedData` (10.13) delivers a dictionary per event with the path and, for file events, `kFSEventStreamEventExtendedFileIDKey` (the inode) [D].
- **Loss and coalescing.** Nearby events in a directory and its subdirectories may coalesce into one event with `MustScanSubDirs`, which requires a recursive rescan of that path. `KernelDropped` and `UserDropped` also set `MustScanSubDirs` [D]. The list is "advisory": a disk modified by an older OS, or by another OS, has its history discarded, and backup software should still sweep periodically [D].
- **Security.** A non-root client receives no event for a directory it cannot reach through normal permissions, and its event ids are not necessarily consecutive. Only root is guaranteed every event. `.fseventsd` is root-only and its format is private. An empty `.fseventsd/no_log` file disables logging on a volume [D].

**Claims and inferences:**
- **Rename pairing.** Rename events come as two `ItemRenamed` events, old path and new path. They can arrive out of order, and a maintainer of the Node `fsevents` package proposed pairing them by inode through extended data [C, fsevents/fsevents#361]. The on-disk v2 records (10.13+) store a node id [C, Plaso `fseventsd` parser], so historical replays can carry inodes too [I; to be measured].
- **Retention** is long in practice: logs typically go back to the last major OS upgrade [C, Eclectic Light 2017]. It is not specified [D: none].
- **TCC and privacy.** FSEvents needs no entitlement; only the permission check above applies [D]. Whether TCC (the Documents, Desktop, Downloads and iCloud Drive protections) also filters FSEvents deliveries to a Terminal-launched CLI is **unverified** [I]. On arm64, macOS requires every native binary to be at least ad-hoc signed, and the Rust toolchain's linker does that [I]. An unsigned-by-Developer-ID CLI can still use FSEvents [I].
- **A daemon-less replay** is possible [I]:
  1. create a per-device stream with `sinceWhen = cursor`, `FileEvents | UseExtendedData | UseCFTypes | FullHistory` and latency 0;
  2. schedule it on a dispatch queue and start it;
  3. collect events until `HistoryDone`;
  4. stop, invalidate and release the stream.

  `fseventsd` belongs to the OS, not to moirai, so this keeps [40] DR3's rule of no moirai daemon. **Cost is unmeasured**: IPC to `fseventsd` plus decompression of logs since the cursor [I].

**How E2 maps onto FSEvents [I]:**
- `ItemRenamed` pairs with the same file id give an exact old-path → new-path pair, which is better than the unprivileged USN read, which carries no names [09 §4.1].
- `Removed` for the old id plus `Renamed` or `Created` of a new id at the same path is the rename-over signature.
- Every result is still verified in the tree ([40 §4.7] "every result is verified").
- `MustScanSubDirs`, dropped events, a changed UUID or a cursor above the current id all mean "history lost": fall back to the frontier walk (§8.3) for the root.

### 4.6 Paths on macOS

- **APFS** accepts only valid UTF-8 names and preserves both case and normalization. It exists in case-sensitive and case-insensitive variants on macOS; case-insensitive is the default [D, APFS FAQ]. Since macOS High Sierra both variants are **normalization-insensitive**, using hashes of the normalized name, with Unicode 9.0 rules. At the time of the FAQ, APFS refused names that contain code points unassigned in Unicode 9.0 [D]. The superblock flags are `APFS_INCOMPAT_CASE_INSENSITIVE` and `APFS_INCOMPAT_NORMALIZATION_INSENSITIVE` [D, APFS Reference].
- **HFS+** stores names normalized (a decomposed form) in UTF-16 [D, APFS FAQ]. A name created in NFC reads back as NFD [I].
- **git on macOS.** `git init` and `git clone` probe by creating an NFC name and checking whether the NFD spelling resolves. On APFS and HFS+ it does, so git writes `core.precomposeunicode=true` into the repository config (`probe_utf8_pathname_composition`) [S]. git then precomposes names read from `readdir` to NFC [D/S]. `core.ignoreCase` is probed and set the same way [D], and `core.protectHFS` defaults to true on macOS [D].
- **Limits.** `NAME_MAX` is 255 bytes and `PATH_MAX` is 1024 bytes, but XNU notes that "HFS & APFS may support names longer than NAME_MAX bytes" [S, `sys/syslimits.h`]. HFS+ allows 255 UTF-16 units, which can exceed 255 UTF-8 bytes [I]. A 1024-byte `PATH_MAX` is small, so deep trees need `*at()` walks [I].
- **Canonical spelling.** Apple's `realpath(3)` replaces each component with its on-disk name through `getattrlist(ATTR_CMN_NAME)`, which "matters on case-insensitive filesystems" [S, Libc `realpath.c`]. `F_GETPATH` returns the firmlinked form, and `F_GETPATH_NOFIRMLINK` the other [D]. `/tmp`, `/var` and `/etc` are symlinks into `/private` [I, well known]. `/Users/x` and `/System/Volumes/Data/Users/x` are the same directory through a firmlink [I].
- **Case sensitivity** is a volume property, `VOL_CAP_FMT_CASE_SENSITIVE` in `ATTR_VOL_CAPABILITIES` [D]. `pathconf` does not document `_PC_CASE_SENSITIVE` [D: absent from pathconf(2)].

### 4.7 Dataless files, TCC and other macOS specifics

- **iCloud Drive "dataless" files** carry `SF_DATALESS`, a synthetic, read-only flag [S, `sys/stat.h`]. `setiopolicy_np(IOPOL_TYPE_VFS_MATERIALIZE_DATALESS_FILES, IOPOL_SCOPE_PROCESS, IOPOL_MATERIALIZE_DATALESS_FILES_OFF)` disables materialization for the process [D]. This is the macOS counterpart of [40 §4.6]'s OneDrive rule: in automatic paths, never read content of an entry with `SF_DATALESS` [I]. With "Desktop & Documents Folders" enabled in iCloud Drive, a repository under `~/Documents` can be evicted to dataless files [I].
- `IOPOL_TYPE_VFS_ATIME_UPDATES` with `IOPOL_ATIME_UPDATES_OFF` stops access-time updates for the process [D]. It is useful because moirai reads project files to hash them. On Linux the equivalent is `O_NOATIME`, permitted only for the file's owner [I].
- **TCC.** Files under `~/Documents`, `~/Desktop`, `~/Downloads` and iCloud Drive are protected, and access is attributed to the responsible app (Terminal, iTerm2, VS Code) [I]. `doctor` should warn when a root lies under a protected folder, and the pattern matrix should include such a root on macOS CI [I].

---

## 5. Windows cross-checks measured today

| Probe | Result |
|---|---|
| Create `café.txt` in NFC (`636166c3a92e747874`) and in NFD (`63616665cc812e747874`) in one NTFS directory on C: | **two distinct files**, different file ids. NTFS is normalization-sensitive [M] |
| `stat("Readme.md")` versus `stat("README.MD")` | same file id. A zero-write case probe works [M] |
| Git for Windows 2.54.0: `git update-index --add --cacheinfo` with Linux-legal names | rejected as "Invalid path": `a\b.txt`, `CON.txt`, `aux`, `x.`, `y ` (trailing space), `a:b.txt`, `q?.txt` [M] |
| The same, with `ok/Name.txt` and `ok/name.txt` | both accepted into the index. `git clone` on Windows warns that the paths collided and checks out only `name.txt` [M] |
| This session's environment | lists both `D:\tmp` and `d:\tmp` as additional working directories, so one directory is spelled two ways in practice [M] |

---

## 6. Survival matrix across the three OSes

"Id" means the OS file id at the file's final path: NTFS `FILE_ID_128`, Linux `(dev, ino)`, macOS `ATTR_CMN_FILEID`. Windows rows are measured [M, 09 §3]. Linux and macOS rows follow from the documented semantics and the sources above; they are unmeasured.

| Operation | Windows NTFS | Linux ext4/XFS/btrfs | macOS APFS |
|---|---|---|---|
| Rename or move within one volume or mount | id kept [M] | inode kept; ctime set [S] | id kept [I] |
| Directory rename or move | directory and child ids kept [M] | kept [I] | kept [I]; `ADDEDTIME` of the moved directory updated [D] |
| Move across volume, bind mount, btrfs subvolume or overlay lower directory | copy+delete: new id [M] | `EXDEV` → `mv` copies: new inodes, new btime [D/S] | across volumes: copy, new id [I] |
| Temp + rename-over: Claude Code `Edit`/`Write`, `sed -i`, `os.replace` | new id; creation time tunneled for `MoveFileEx(REPLACE_EXISTING)`, not for Claude Code [M] | new inode, new btime, no tunneling [I] | new id [C #92419]; **document id kept if `UF_TRACKED`** [D/S] |
| In-place write | id kept [M] | kept [I] | kept [I] |
| vim, default `backupcopy=auto` | id kept (MSYS2 build) [M] | usually a new inode (rename strategy) [D/I] | usually a new id [D/I] |
| git checkout, reset, stash or merge of a changed file | new id; object id tunneled [M] | new inode (unlink + `O_EXCL` create) [S] | new id; document id inherited through the same-thread tombstone if tracked [S HFS+ / I APFS] |
| Copy | new id [M] | new inode, new btime [I] | `cp -c`/clone: new id, **same creation time** [D/I]; `cp -p`: new creation time [S] |
| Hard link | same id [M] | same inode [I] | same file id; distinct `LINKID` [D] |
| Delete + recreate the same name within 15 s | object id and creation time tunneled [M] | nothing kept [I] | document id kept if tracked **and** the same thread does both [S] |
| Trash | `$Recycle.Bin`, id kept [M] | freedesktop Trash on the same mount: rename, inode kept [D/I] | `~/.Trash` or `/.Trashes/<uid>`: rename, id kept [I] |
| Id reuse | MFT slot reused with a 16-bit sequence [M] | ext4 lowest-free reuse [S]; random 32-bit generation [S] | counter-allocated; no reuse expected [D/I] |

---

## 7. Per-OS evidence sources for the resolver (E1–E8)

The cascade of [40 §4.3] stays as written. This table gives each source's realization and availability per OS. **A source that does not exist on an OS contributes nothing. No rule changes.**

| Source | Windows (as designed) | Linux | macOS |
|---|---|---|---|
| **Read-path stat** | `GetFileAttributesExW`: size, times, attributes, no id | `statx(AT_SYMLINK_NOFOLLOW, BASIC_STATS\|BTIME)`: **id included**, so reads detect a replacement in place and a swap, removing [40 §2.6]'s read limitation | `lstat` or `getattrlist`: **id included**; `st_flags` shows `SF_DATALESS` |
| **Settle enumeration** | `FileIdExtdDirectoryInfo` | `getdents64` (`d_ino`, `d_type`), then `statx` only for entries whose size or times matter | `getattrlistbulk` (name, type, fileid, parentid, size, mtime, crtime, addedtime, ext flags, [docid]) |
| **E1** intent, hooks | FSINTENT; PENDING; `PostToolUse` on `Bash(mv *)`, `PowerShell(Move-Item *)` … | same; the Bash patterns only | same; the Bash patterns only |
| **E2** journal | unprivileged USN read where a journal exists | **none** | **FSEvents** per-device replay (§4.5); cursor = (device UUID, event id) |
| **E3d** parent-directory id | `OpenFileById(dir id)` | frontier search for the directory's `d_ino` (§8.3) | `fsgetpath(fsid, parent id)` |
| **E3** file id | `OpenFileById` + path; exact if size and mtime equal | frontier search for `d_ino`; verify (size, mtime ns), plus generation if stored | `fsgetpath(fsid, fileid)`; same verification |
| **E4** near + copy rule | creation-time line valid | valid; btime often present, never copied | creation-time line **demoted**: only when the candidate has no clone indicator (`EF_MAY_SHARE_BLOCKS` clear, `CLONE_REFCNT` = 0), and then STRONG, not exact |
| **E5** prefix | OS-independent | same | same |
| **E6** git | OS-independent (in-process reader) | same | same |
| **E7** changed since the last settle | ChangeTime or CreationTime > last settle | ctime > last settle (rename sets ctime [S]) **or** inside a frontier directory | `ADDEDTIME` or ctime > last settle, or inside a frontier directory |
| **E8** edited + moved | basename + sketch | same | same; with the owner's opt-in, an equal document id in the scanned scope is **exact** (§4.3) |
| **Trash rule (P6)** | `$Recycle.Bin` | `$XDG_DATA_HOME/Trash`, `$topdir/.Trash/$uid`, `$topdir/.Trash-$uid` [D] | `~/.Trash`, `/.Trashes/<uid>` [I] |
| **Cloud rule** | `RECALL_*`, `OFFLINE` attributes | none standard; untrusted filesystem types get no ids | `SF_DATALESS` plus the process I/O policy |

**Expected cost per OS, in order of magnitude [I; all to be measured in M0 on real hardware].**
- Linux `statx` of a warm path should cost about 1 µs, with no antivirus per-open cost. A read-time E3 or E3d is dominated by the frontier: one `statx` per known directory, plus `getdents64` of the directories that changed.
- macOS `fsgetpath` is one B-tree lookup plus a path build.
- A macOS FSEvents replay's fixed cost is a Mach round trip to `fseventsd` plus log reads. It could exceed the 1–9 ms of an incremental USN replay [09 §4.2].

The read budget of [40 §4.2] (≤ 20 ms per command; ≤ 5 ms p50 for 50 links in one moved directory) holds on Windows and, very likely, on macOS [I]. **On Linux, the ≤ 5 ms gate for a moved directory depends on the frontier cost of the tree.** If a measured tree breaks it, the link renders `unverified (budget)` on the read and resolves at the next settle, which is the documented behaviour of P7 [40 §4.1], not a new rule.

---

## 8. Recommendation: one platform abstraction

### 8.1 Principles

1. **One format, tagged.** Every OS-specific datum lives in runtime tables ([40] I-F4: never versioned, merged, exported or hashed), and each such value carries a **kind tag**. A row whose tag this platform cannot interpret is ignored, as if the row were absent. So a store opened on another OS (a shared disk, dual boot, WSL next to Windows) behaves like a first settle and never gives a false hit [I].
2. **One semantics.** The resolver sees `Evidence` records of the classes [40 §4.3] defines, not OS calls. OS modules only produce evidence. Rules, thresholds, states, strings and the classification table stay in one module that does not depend on the platform. A missing capability means a source yields nothing, never a different rule.
3. **Determinism across OSes.** Every set the resolver ranks is sorted by exact path bytes first, because enumeration order differs by filesystem (§4.1). Case folding and normalization for `PATHIDX` use one frozen function (R-14), whatever the OS. OS behaviour, meaning "does this directory treat A and a as one file?", is observed at resolve time and never baked into keys.
4. **Verify every OS hit.** This is [40]'s rule, and it matters more on Linux, where inode numbers are reused (§3.1).
5. **No privilege, no daemon, no metadata writes by default**, on every OS. FSEvents replay is allowed because `fseventsd` is part of the OS, as the NTFS journal is.

### 8.2 The `ProjectFs` trait and its capability record

This sketch fixes the contract; names are illustrative.

```rust
pub struct VolumeCaps {
    pub id_kind: IdKind,               // None | Ntfs128 | Refs128 | LinuxIno | DarwinFileId
    pub ids_persistent: bool,          // NTFS/ReFS; ext4/xfs/btrfs/f2fs/bcachefs/tmpfs(session); APFS/HFS+ (VOL_CAP_FMT_PERSISTENTOBJECTIDS)
    pub id_locate: IdLocate,           // ById (Windows OpenFileById, macOS fsgetpath) | Frontier (Linux) | None
    pub journal: JournalKind,          // None | Usn | FsEvents
    pub btime: BtimeTrust,             // Absent | Unforgeable (Linux) | TunneledNotCopied (NTFS) | CopiedByClones (APFS/HFS+)
    pub ctime_on_rename: bool,         // ext4/btrfs/xfs: true [S]; others: probe/false
    pub case_rule: CaseRule,           // PerDirFlag (NTFS FileCaseSensitiveInfo, Linux FS_CASEFOLD_FL) | Volume (APFS/HFS+) | Sensitive
    pub norm_insensitive: bool,        // APFS/HFS+ true; Linux casefold dirs true; NTFS false [M]
    pub cloud: CloudRule,              // RecallAttrs (Windows) | Dataless (macOS) | None
    pub rename_noreplace: bool,        // Windows MoveFileExW w/o REPLACE_EXISTING; Linux renameat2(RENAME_NOREPLACE); macOS renamex_np(RENAME_EXCL)
    pub mtime_granularity_ns: u32,     // NTFS 100, ext4/APFS 1, HFS+ 1e9
}

pub trait ProjectFs {
    fn volume(&self, dir: &RootedPath) -> io::Result<(VolumeKey, VolumeCaps)>;
    fn canonical_root(&self, p: &Path) -> io::Result<CanonicalRoot>;          // §9 rule P9
    fn stat(&self, p: &RelPath) -> io::Result<Option<StatRec>>;                 // lstat semantics, id when cheap
    fn enumerate(&self, dir: &RelPath, sink: &mut dyn FnMut(EntryRec)) -> io::Result<()>;
    fn locate_id(&self, root: &CanonicalRoot, id: &OsFileId, budget: &mut Budget) -> Located; // InRoot(path) | Outside(Trash|Elsewhere) | Gone | Unknown(budget)
    fn journal_since(&self, cur: &JournalCursor, budget: &mut Budget) -> JournalReplay;       // normalized events or HistoryLost
    fn case_equivalent(&self, dir: &RelPath, a: &[u8], b: &[u8]) -> io::Result<bool>;        // flag or zero-write probe
    fn read_for_hash(&self, p: &RelPath, max: u64) -> io::Result<ReadOutcome>;             // no-atime, never materializes placeholders
    fn rename_noreplace(&self, from: &RelPath, to: &RelPath) -> io::Result<()>;            // never std::fs::rename (§8.6)
}
```

`EntryRec` carries the name as exact bytes (UTF-16 → WTF-8 on Windows, raw bytes on Unix), kind, size, mtime, ctime, btime, `OsFileId`, parent id, and the optional macOS fields (added time, clone flags, document id). `StatRec` is the same without the name.

**Trusted id filesystems** (ids are used for E3, E3d and FILEOBS):
- Windows: NTFS, ReFS (`FILE_ID_128` only);
- Linux, by `statfs.f_type`: ext4, XFS, btrfs, f2fs, bcachefs, and tmpfs within a boot;
- macOS: APFS and HFS+ (capability bits).

On every other filesystem, `id_kind = None`: overlayfs unless verified `xino`, NFS, CIFS/SMB, vfat, exFAT, FUSE, and 9p/drvfs, which is what WSL uses for `/mnt/c`. There the resolver runs on path, content and git only.

### 8.3 The changed-directory frontier (how Linux gets E3 and E3d without privilege)

- **Runtime table `DIRMAP` (tree, directory id) → (root-relative path, directory mtime).** A settle's enumeration writes it, as a lazy record ([AR] durability class "lazy"); reads use it and never write it [40 I-F5]. Size is about 50–70 B per directory (5,000 directories ≈ 0.3 MB on disk for the owner's trunk worktree [M, 13 §1.7: 5.0k directories]) [I].
- **Why it is complete.** Every new directory entry lies in a directory whose mtime changed, or inside a directory created since the last settle. POSIX requires rename to update both parents' mtime [D]. Git's untracked cache depends on the same property and offers `--test-untracked-cache` to check it per filesystem [D]. On filesystems with coarse timestamps (HFS+ 1 s, FAT 2 s), or when a directory's mtime is at or after the previous scan's start, the directory counts as changed ("racy", as in git) [I].
- **Algorithm (read or settle).**
  1. `statx` each `DIRMAP` directory of the tree. A missing one is a moved or deleted source.
  2. `getdents64` each directory whose mtime changed, and recursively each new subdirectory found there.
  3. Build `d_ino` → path for the entries found.
  4. E3d: a stored parent-directory id found among the directory entries gives the new directory path. E3: a stored file id found gives the new file path.
  5. Budget exhausted → `unverified (budget)`.
- **Other uses [I].** It is also a cheap E7 on every OS (only files under frontier directories can be new), and it would help a Windows tree with no USN journal (D: today). On Windows and macOS, `locate_id` still uses the direct call, and the frontier only feeds E7.

### 8.4 Runtime record layouts (format reservations R-7, R-8, R-18)

| Reservation | Current ([40 §2.6, §2.11]) | Proposed |
|---|---|---|
| `FILEOBS` id fields | 64-bit volume serial, `FILE_ID_128`, parent `FILE_ID_128`, creation time | **`OsFileId`** = `{kind u8, vol_key [u8;16], id [u8;16], parent [u8;16], gen u32, docid u32}`, 57 B packed; kinds `ntfs128`, `refs128`, `linux_ino` (id = ino, `gen` from `name_to_handle_at` if taken), `darwin_fileid` (`docid` if the owner opted in). Timestamps as i64 ns since the Unix epoch plus a granularity byte; fields `mtime`, `ctime`, `btime` (optional), `added` (optional, macOS) |
| `vol_key` | 64-bit NTFS serial | Windows: 64-bit volume serial, zero-extended. Linux: filesystem UUID from `FS_IOC_GETFSUUID` (Linux 6.9 [C]), else `statfs.f_fsid`, and for btrfs the subvolume id (`STATX_SUBVOL`, 6.10 [D]). ext4's `f_fsid` comes from the UUID and btrfs's mixes fsid and subvolume [S]; **XFS's `f_fsid` is the device number** [S] and can change across boots, so rows may go stale; verification catches that. macOS: `ATTR_VOL_UUID` [D] |
| `USNCUR` | `(UsnJournalID, NextUsn)` per volume serial | **`JOURNALCUR`** = `{kind u8 (usn \| fsevents), vol_key [u8;16], instance [u8;16] (UsnJournalID \| FSEvents device UUID), cursor u64}` |
| `TREES` | root path, volume serial, case-sensitivity map, … | + `canonical_root` bytes, root directory `OsFileId`, `norm_insensitive` flag, platform tag |
| new | — | **`DIRMAP`** (§8.3), lazy |
| `FSINTENT.pid_start` | Windows process start time | tagged per OS (Linux: `/proc/<pid>/stat` start time plus `/proc/sys/kernel/random/boot_id`; macOS: `proc_pidinfo(PROC_PIDTBSDINFO)` start time). The details belong to the concurrency lens |

None of these rows is exported, so R3 is unaffected [40 §5.7].

### 8.5 Resolver rules that need a per-OS clarification (not a rule change)

1. **Copy rule (E4).** "q.creation = FILEOBS.creation → exact" applies only when `VolumeCaps.btime ∈ {Unforgeable, TunneledNotCopied}` **and** the candidate shows no clone indicator. On macOS (`CopiedByClones`), equal creation time is at most STRONG.
2. **"Spelling differs on disk" replaces "case differs on disk"** [40 §2.4]. When stat of the stored spelling succeeds but enumeration returns another spelling that is equal under the directory's equivalence (case, normalization, or both), the state is `ok (spelling differs on disk)` and nothing is written. On a normalization-*sensitive* directory (NTFS, Linux without casefold), a missing path plus **exactly one** entry in the same directory whose NFC form equals NFC(p) also renders `ok (normalization differs on disk)`. Two such entries render `ambiguous (normalization collision)` [I; rule proposed].
3. **Reads may use ids where stat returns them** (Linux, macOS). A swap or a replacement in place then shows at read time. That is additional exactness from the same rule ("stat quadruple equals FILEOBS → ok"), not a new rule.
4. **Sort before tie-breaking**: candidate lists and E6 groups are sorted by exact bytes (§8.1 point 3).

### 8.6 `file mv` and `file rm` per OS (only what bears on identity)

- **Never `std::fs::rename`.** On Unix it is rename(2), which silently replaces an existing destination file, and it fails across mount points [D, Rust std docs]. Use `renameat2(RENAME_NOREPLACE)`: Linux 3.15+, glibc 2.28; ext4 since 3.15, btrfs, tmpfs and CIFS since 3.17, XFS since 4.0; `EINVAL` where unsupported [D, rename(2)]. The fallback is `link()`+`unlink()` for files, and a refusal for directories. On macOS use `renamex_np(RENAME_EXCL)`, gated by `VOL_CAP_INT_RENAME_EXCL` [D].
- **`EXDEV` is the Unix counterpart of "cross-volume".** It covers bind mounts, btrfs subvolumes and overlay lower directories. Apply [13 §3.2]'s cross-volume rules: refuse directory moves; for files, copy → flush → verify `oid` → delete.
- **There are no sharing violations on Unix,** so pattern-matrix row 21 needs Unix variants: `EBUSY` (mount point), `EACCES`/`EPERM` (permissions, SIP, TCC), and a process with its cwd inside a directory, which does *not* block a rename on Unix [I].

### 8.7 Tests (GT17 and FL-2)

- **FL-2 simulator: three OS profiles.**
  - *NTFS:* as today.
  - *ext4:* lowest-free inode reuse with random generations, ctime on rename, `EXDEV` at mount and subvolume boundaries, casefold directories, non-UTF-8 names.
  - *APFS:* case- and normalization-insensitive lookup that preserves spelling, counter-allocated ids, clones that keep creation time, document ids with rename-over transfer and a same-thread tombstone, dataless placeholders, FSEvents with coalescing, `MustScanSubDirs`, out-of-order rename halves and UUID resets.
- **GT17 on real filesystems:** NTFS (Windows host); ext4, XFS and btrfs with a subvolume boundary (Linux CI); case-insensitive APFS on the default macOS runner, plus a case-sensitive APFS disk image, which `hdiutil create -fs "Case-sensitive APFS"` and `attach` make without root [I]. A Linux casefold directory needs `mkfs -O casefold` and a mount, so it runs in a privileged CI job [I].
- **New matrix rows [I]:**
  - `cp -c x y`, then `rm x` (macOS clone + delete);
  - an ext4 inode ping-pong under repeated atomic saves;
  - a move across a btrfs subvolume or a bind mount (`EXDEV` → copy);
  - an overlay lower-directory rename;
  - roots spelled `/tmp/...` versus `/private/tmp/...`, mixed-case `cd` on APFS, and `D:\` versus `d:\`;
  - NFC/NFD twins on ext4 and on NTFS, and an NFD name on APFS;
  - a non-UTF-8 name on ext4;
  - an FSEvents replay after a reboot, and after `FSEventsPurgeEventsForDeviceUpToEventId`;
  - a dataless iCloud file;
  - a root under `~/Documents` (TCC).
- **M0 measurements to add** (the owner's machine cannot provide them):
  - Linux `statx`/`getdents64` walk and frontier costs on ext4 and btrfs;
  - macOS `getattrlistbulk` walk, `fsgetpath` latency, and FSEvents replay latency for cursors 1 h, 1 day and 1 week old;
  - whether historical FSEvents carry `fileID`;
  - APFS behaviour: ctime on rename, clone creation time, document-id transfer on rename-over and on git checkout, and the `UF_TRACKED` notification caveat.

---

## 9. Cross-OS path canonicalization rules

These rules make a link's stored bytes identical on all three OSes, so an image exported on Windows imports on Linux or macOS with the same `path`, the same derived uids and the same merges ([40] DR7, DR8).

| # | Rule | Why |
|---|---|---|
| P1 | A stored path is root-relative, uses `/` separators, has no empty, `.` or `..` segment, and is valid UTF-8 as exact bytes (unchanged from [40] I-F8) | the format stays one string type |
| P2 | **Tracked file: the spelling of git's HEAD tree**, bytes as git stores them, on every OS (unchanged; [40 §2.3]) | git's tree is the one spelling all three OSes share |
| P3 | **Untracked file: the OS's enumerated spelling, with git's precomposition applied on macOS.** On a normalization-insensitive volume (APFS, HFS+), store `NFC(name)` when the repository's `core.precomposeUnicode` is true, or always when there is no git (R2). On Linux and Windows, never normalize | git on macOS records NFC [S]. Without this rule, a file linked before its first commit would re-bind to a different spelling once committed, and a Linux store would disagree |
| P4 | **Refuse at link time, on every OS:** a component that contains `\` or a C0 control character; a name that is not UTF-8 on Linux; a name with an unpaired surrogate on Windows | `\` is a separator on Windows, and Git for Windows rejects it [M]. Such names cannot be expressed in the format |
| P5 | **Portable by default.** `file mv`, `file add` and `link` warn about, and `file mv` refuses to *create*, names that some supported OS cannot hold: Windows reserved device names, a trailing dot or space, any of `<>:"\|?*` [M: Git for Windows rejects them], a component over 255 UTF-8 bytes, and a name that collides with a sibling under fold (P6). `--allow-nonportable` overrides this. Such paths already in git are linkable, and render `missing (not representable on this OS)` where they cannot exist | one repository is checked out on three OSes; case-colliding paths already lose a file on Windows [M] |
| P6 | **`PATHIDX` fold, one frozen function (R-14):** `fold_v1(x) = NFD(simple_casefold(NFD(x)))` over a fixed Unicode version. It is used for collision detection and index order only, never for identity | covers NTFS/APFS case-insensitivity and APFS/HFS+/casefold normalization-insensitivity with one key. NTFS's `$UpCase` table, APFS (Unicode 9.0 [D]) and ext4 (Unicode 12.1 [D]) each fold with their own table, so the OS's real behaviour is observed at resolve time (§8.5 rule 2) |
| P7 | `origin_path` (the uid derivation input, [40 §2.3]) follows P2 and P3 | the same file gets the same uid in every store |
| P8 | Symlinks: lstat semantics; a link names the link itself; a symlink's `oid` is computed over its target text, as git does. On Windows with `core.symlinks=false`, git checks the symlink out as a text file with the same bytes, so the `oid` is equal [I from git's blob rule] | same `oid` on all OSes |
| P9 | **Canonical root.** `TREES` key = BLAKE3 over the OS-canonical top-level: Windows `GetFinalPathNameByHandleW`; macOS `F_GETPATH` on an fd of the top-level (firmlinked form, on-disk case) [D/S]; Linux `realpath`. Store the root directory's `OsFileId` too, and treat two keys with the same root id as one tree, refusing a second binding ([40] I-F12) | `D:\` versus `d:\` [M]; `/tmp` versus `/private/tmp`; case-insensitive `cd` spellings on APFS; bind mounts on Linux |
| P10 | Walk with `openat`/`fstatat`-relative calls (Unix) and `\\?\` or handle-relative opens (Windows) | macOS `PATH_MAX` is 1024 [S]; Linux's is 4096 [S] |

**The image itself (R3).**
- `nodes/<h1>/<h2>/<uid>.moi` uses lower-case hex names and is portable [AR §5b.1].
- **`schema/queries/<name>.moi` is not portable.** LQ's `ident` allows letters in both cases and back-quoted arbitrary text [50 §7]. Two queries `Foo` and `foo` would collide on checkout on Windows and macOS, and names containing `:`, `\` or a trailing dot cannot be checked out on Windows [M: Git for Windows rejects them].
- **Fix (owner or [50] author):** export the file as `schema/queries/<blake3(name)[0..16] hex>.moi` and keep the real name in the file's `name:` line, which already exists [50 §4.4]. The alternative is to restrict query names to `[a-z0-9_.]` and refuse fold-duplicates.

---

## 10. Consequential edits for other documents

| Document | Edit |
|---|---|
| [60] §1 exclusions row "Unix `Vfs` and `ProjectFs`: excluded (owner priority)" | now **in release**. FL-2 gains the ext4 and APFS profiles; M6's exit gate GT17 runs on NTFS, ext4, XFS, btrfs and APFS (case-insensitive and case-sensitive); FL-10 gains an FSEvents reader next to the USN reader |
| [40] §2.6 `FILEOBS`, `USNCUR`, `TREES`; §2.11 R-7, R-8, R-18 | the tagged `OsFileId`, `JOURNALCUR`, `DIRMAP` and `TREES` fields of §8.4 |
| [40] §2.4 path rules, case bullets | P3–P6 and "spelling differs on disk" (§8.5 rule 2); the per-OS case and normalization sources of §2 |
| [40] §4.3 E2, E3, E3d, E4 copy rule, E7 | the per-OS realizations of §7; the copy-rule restriction of §8.5 rule 1 |
| [40] §4.6 cloud rows; §4.7 accelerators | macOS `SF_DATALESS` plus the I/O policy; FSEvents as an optional accelerator used automatically where present; owner decision on `UF_TRACKED` |
| [40] §8.3.1 pattern matrix | per-OS variants of rows 4, 19, 21, 28 and 30, and the new rows of §8.7 |
| [40] DR4 "Windows 11 first, non-admin" | "Windows, Linux, macOS; non-admin on all three" |
| [50] §4.4 image file name of named queries | §9 fix |
| [AR] §5b.1 | the named-query path rule |

---

## 11. Risks and unverified items

| Item | Status | Mitigation |
|---|---|---|
| All Linux and macOS behaviour | **unmeasured** (no WSL, no Mac) | the M0 measurement list of §8.7; CI runners |
| FSEvents history carries inodes; replay cost; TCC filtering | [C]/[I] | measure; without inodes, rename pairing falls back to the path plus a stat |
| APFS document-id transfer (APFS is closed source) | [D] spec only | measure; opt-in only |
| `UF_TRACKED` "no notifications" comment | [S], meaning unclear | measure before combining document ids and FSEvents |
| Clone creation time equal on APFS | [I] from "identical attributes" | measure; §8.5 rule 1 is safe in either case |
| Linux frontier cost on large trees | [I] | measure; the read budget falls back to `unverified (budget)` |
| XFS `f_fsid` changes when device numbers change | [S] | verification makes rows go stale safely; prefer `FS_IOC_GETFSUUID` |
| Overlay, network, FUSE and WSL drvfs roots | [D/I] | `id_kind = None` mode; `doctor` warns |
| Claude Code's Bash sandbox on Linux (bubblewrap) and macOS (Seatbelt) may block moirai's store writes from inside a sandboxed shell | [I] | belongs to the agent-integration lens |

---

## 12. Sources

**Linux (man pages, kernel source, docs)**
- rename(2): https://man7.org/linux/man-pages/man2/rename.2.html
- open_by_handle_at(2) / name_to_handle_at: https://man7.org/linux/man-pages/man2/open_by_handle_at.2.html
- statx(2): https://man7.org/linux/man-pages/man2/statx.2.html
- inotify(7): https://man7.org/linux/man-pages/man7/inotify.7.html
- fanotify_init(2): https://man7.org/linux/man-pages/man2/fanotify_init.2.html ; fanotify_mark(2): https://man7.org/linux/man-pages/man2/fanotify_mark.2.html
- readdir(3): https://man7.org/linux/man-pages/man3/readdir.3.html
- fhandle relaxation, commit 620c266f: https://github.com/torvalds/linux/commit/620c266f394932e5decc4b34683a75dfc59dc2f4 ; source `fs/fhandle.c` (`may_decode_fh`): https://github.com/torvalds/linux/blob/master/fs/fhandle.c
- inotify watch default, commit 92890123: https://github.com/torvalds/linux/commit/92890123749bafc317bbfacbe0a62ce08d78efb7
- ext4 rename ctime (`ext4_rename`): https://github.com/torvalds/linux/blob/master/fs/ext4/namei.c ; inode allocation and generation (`find_inode_bit`, `__ext4_new_inode`): https://github.com/torvalds/linux/blob/master/fs/ext4/ialloc.c ; export ops and `f_fsid`: https://github.com/torvalds/linux/blob/master/fs/ext4/super.c
- `simple_rename_timestamp`: https://github.com/torvalds/linux/blob/master/fs/libfs.c
- XFS rename ctime (`xfs_dir_rename`): https://github.com/torvalds/linux/blob/master/fs/xfs/libxfs/xfs_dir2.c ; XFS `f_fsid`: https://github.com/torvalds/linux/blob/master/fs/xfs/xfs_super.c
- btrfs getattr (`anon_dev`, `STATX_SUBVOL`) and rename `EXDEV`: https://github.com/torvalds/linux/blob/master/fs/btrfs/inode.c ; tree-search `CAP_SYS_ADMIN`: https://github.com/torvalds/linux/blob/master/fs/btrfs/ioctl.c ; send: https://github.com/torvalds/linux/blob/master/fs/btrfs/send.c ; `f_fsid`: https://github.com/torvalds/linux/blob/master/fs/btrfs/super.c
- btrfs subvolumes: https://btrfs.readthedocs.io/en/latest/Subvolumes.html
- overlayfs: https://github.com/torvalds/linux/blob/master/Documentation/filesystems/overlayfs.rst
- ext4 casefold: https://github.com/torvalds/linux/blob/master/Documentation/admin-guide/ext4.rst ; tmpfs casefold: https://github.com/torvalds/linux/blob/master/Documentation/filesystems/tmpfs.rst ; `FS_CASEFOLD_FL`, `FS_IOC_GETFSUUID`: https://github.com/torvalds/linux/blob/master/include/uapi/linux/fs.h ; limits: https://github.com/torvalds/linux/blob/master/include/uapi/linux/limits.h
- POSIX rename: https://pubs.opengroup.org/onlinepubs/9799919799/functions/rename.html
- OpenZFS zfs-diff(8): https://openzfs.github.io/openzfs-docs/man/master/8/zfs-diff.8.html ; zfs-allow(8): https://openzfs.github.io/openzfs-docs/man/master/8/zfs-allow.8.html
- freedesktop Trash specification: https://specifications.freedesktop.org/trash/latest/

**macOS (Apple docs, man pages, XNU, HFS, libc)**
- getattrlist(2): https://keith.github.io/xcode-man-pages/getattrlist.2.html ; getattrlistbulk(2): https://keith.github.io/xcode-man-pages/getattrlistbulk.2.html
- fsgetpath(2): https://keith.github.io/xcode-man-pages/fsgetpath.2.html ; fcntl(2): https://keith.github.io/xcode-man-pages/fcntl.2.html ; rename(2)/renamex_np: https://keith.github.io/xcode-man-pages/rename.2.html
- clonefile(2): https://keith.github.io/xcode-man-pages/clonefile.2.html ; copyfile(3): https://keith.github.io/xcode-man-pages/copyfile.3.html ; cp(1): https://keith.github.io/xcode-man-pages/cp.1.html ; chflags(2): https://keith.github.io/xcode-man-pages/chflags.2.html ; setiopolicy_np(3): https://keith.github.io/xcode-man-pages/setiopolicy_np.3.html ; pathconf(2): https://keith.github.io/xcode-man-pages/pathconf.2.html
- File System Events Programming Guide: https://developer.apple.com/library/archive/documentation/Darwin/Conceptual/FSEvents_ProgGuide/UsingtheFSEventsFramework/UsingtheFSEventsFramework.html ; overview: https://developer.apple.com/library/archive/documentation/Darwin/Conceptual/FSEvents_ProgGuide/TechnologyOverview/TechnologyOverview.html ; security: https://developer.apple.com/library/archive/documentation/Darwin/Conceptual/FSEvents_ProgGuide/FileSystemEventSecurity/FileSystemEventSecurity.html
- `FSEvents.h` (macOS 11.3 SDK mirror): https://github.com/phracker/MacOSX-SDKs/blob/master/MacOSX11.3.sdk/System/Library/Frameworks/CoreServices.framework/Versions/A/Frameworks/FSEvents.framework/Versions/A/Headers/FSEvents.h
- APFS FAQ: https://developer.apple.com/library/archive/documentation/FileManagement/Conceptual/APFS_Guide/FAQ/FAQ.html ; Apple File System Reference (2020-06-22): https://developer.apple.com/support/downloads/Apple-File-System-Reference.pdf
- File System Programming Guide (file ids and bookmarks): https://developer.apple.com/library/archive/documentation/FileManagement/Conceptual/FileSystemProgrammingGuide/AccessingFilesandDirectories/AccessingFilesandDirectories.html
- XNU `fsgetpath_internal`, `openbyid_np`, `vfs_context_can_open_by_id`: https://github.com/apple-oss-distributions/xnu/blob/main/bsd/vfs/vfs_syscalls.c ; volfs translation: https://github.com/apple-oss-distributions/xnu/blob/main/bsd/vfs/vfs_lookup.c ; `vfs_context_suser`: https://github.com/apple-oss-distributions/xnu/blob/main/bsd/vfs/kpi_vfs.c ; `build_path`: https://github.com/apple-oss-distributions/xnu/blob/main/bsd/vfs/vfs_cache.c ; document tombstones: https://github.com/apple-oss-distributions/xnu/blob/main/bsd/vfs/doc_tombstone.c ; flags: https://github.com/apple-oss-distributions/xnu/blob/main/bsd/sys/stat.h ; limits: https://github.com/apple-oss-distributions/xnu/blob/main/bsd/sys/syslimits.h
- HFS+ document ids and creation-time clamp: https://github.com/apple-oss-distributions/hfs/blob/main/core/hfs_vnops.c
- copyfile `copyfile_stat`: https://github.com/apple-oss-distributions/copyfile/blob/main/copyfile.c
- Apple Libc `realpath`: https://github.com/apple-oss-distributions/Libc/blob/main/stdlib/FreeBSD/realpath.c

**git, Rust, vim**
- git config core (`ignoreCase`, `precomposeUnicode`, `protectHFS`, `protectNTFS`, `untrackedCache`): https://github.com/git/git/blob/master/Documentation/config/core.adoc
- git precompose probe: https://github.com/git/git/blob/master/compat/precompose_utf8.c ; checkout `create_file`: https://github.com/git/git/blob/master/entry.c
- git untracked cache (directory mtime): https://git-scm.com/docs/git-update-index
- Rust `std::fs::rename`: https://doc.rust-lang.org/std/fs/fn.rename.html ; `Metadata::created`: https://doc.rust-lang.org/std/fs/struct.Metadata.html
- vim `'backupcopy'`: https://vimhelp.org/options.txt.html

**Third-party claims [C]**
- Claude Code Edit/Write replace the inode (macOS, measured inode numbers): https://github.com/anthropics/claude-code/issues/92419
- FSEvents rename events out of order; pair by inode: https://github.com/fsevents/fsevents/issues/361
- fseventsd v2 records carry node ids (Plaso parser): https://plaso.readthedocs.io/en/latest/_modules/plaso/parsers/fseventsd.html ; FSEventsParser: https://github.com/dlcowen/FSEventsParser
- FSEvents retention in practice (Eclectic Light, 2017): https://eclecticlight.co/2017/09/12/watching-macos-file-systems-fsevents-and-volume-journals/
- `FS_IOC_GETFSUUID` added in Linux 6.9 (search summary; OpenZFS PR): https://github.com/openzfs/zfs/pull/19030

## 13. Reproducing

- `probe/norm_probe.py` creates NFC and NFD twins plus a case variant in `probe/work_norm/`, prints the directory listing as hex, compares file ids, and the directory is deleted afterwards.
- The git probe was: `git init`; `git hash-object -w --stdin`; `git update-index --add --cacheinfo 100644,<blob>,<path>` for each name; `git commit`; `git clone`. It ran in `probe/gitpaths/` and `probe/clone1/`, both deleted.
- The downloaded sources are in `src/`. `apfs_ref.txt` is `pdftotext -layout` of the APFS Reference.
