# OS layer: per-OS mapping appendix (every seam method to its OS calls)

| Field | Value |
|---|---|
| Title | OS layer specification, part 2: one table per OS mapping every method of `ProjectFs`, `Clock`, `ProcHost`, `Meter`, `os::path`, `os::ipc`, `os::term` and `os::test_host` to its calls, and an index of part 1's mappings for `StoreFs`, `Locks`, `SealedMaps` and `EnvGuard` |
| Status | draft, pass 1 pending |
| Work package | WP-17b (role R-SPEC-P), part 2 of WP-17 ([PLAN §3.2] item 1: "the mapping appendix for Windows, Linux and macOS") |
| Sources | [80 §2.1]–[80 §2.13] (every per-OS row); [AR §14] (compact mapping); [80 §5.2] (port scope); the part-2 chapters [OS/proc], [OS/clock], [OS/mem], [OS/project], [OS/path], [OS/shell], whose text is normative where this table abbreviates it |
| Reconciled with | [OS/README §1.3, §3, Appendix A]; the Appendix A of [OS/fs], [OS/lock], [OS/map] and [OS/env] (part 1's own per-OS tables, indexed in §5 and not repeated) |

---

## 1. How to read this appendix

- Each row names a method (or a module function) and the OS calls that implement it, in call order. **The chapter named in
  the last column is normative**; this appendix abbreviates it for the implementer and the reviewer, and a disagreement is
  a defect of this appendix.
- **Windows** rows are built from M0 (PLAN §6.2 R1), except where a row says "not built at M0". **Linux** and **macOS**
  rows are the frozen contract of the port phase ([80 §5]); nothing in M0–M11 builds or runs them ([80] X7).
- Every call is a public, documented interface ([80] X9) unless the row says *harness only* (libproc on macOS) or
  *test-only* (`os::test_host`), or names the one undocumented read whose absence selects a degraded mode
  (`kern.bootsessionuuid`).
- Every handle and descriptor is non-inheritable (`bInheritHandle = FALSE`; `O_CLOEXEC`), and every project-file open
  uses full sharing on Windows ([OS/README §5.2], [OS/project §2.2]).
- "`\\?\p`" is the long-path form of [OS/path §6]; "`root_fd`" is a project root's descriptor ([OS/project §2.2]).

## 2. Windows 11, x64 (built from M0)

### 2.1 `ProjectFs` and `os::path`

| Method | Windows calls | Spec |
|---|---|---|
| `canonical_root(dir)` | `GetFullPathNameW` (relative input) → `CreateFileW(\\?\p, FILE_READ_ATTRIBUTES, share R\|W\|D, OPEN_EXISTING, FILE_FLAG_BACKUP_SEMANTICS)` → `GetFinalPathNameByHandleW(FILE_NAME_NORMALIZED \| VOLUME_NAME_DOS)` → `GetFileInformationByHandleEx(FileIdInfo)` → `GetVolumeInformationByHandleW` (file-system name for the id kind) → `CloseHandle` | [OS/path §4.1] |
| `canonical_abs(p)` | existing: as `canonical_root`; absent: `GetFullPathNameW` + lexical normalisation | [OS/path §5] |
| `cli_path(arg, cwd, tree)` | `std::env::args_os` (the MSVC rules over `GetCommandLineW`) gives `arg`; `canonical_root(cwd)`; then pure string work | [OS/path §7], [OS/shell §4] |
| `representable_here(seg)` | none (pure: device names, trailing `.`/space, reserved characters, ≤ 255 UTF-16 units) | [OS/path §8.1] |
| `user_config_path()` | the `APPDATA` environment variable (`std::env::var_os`); no `SHGetKnownFolderPath` | [OS/path §10] |
| OS name → `EntryName` | UTF-16 → UTF-8, or WTF-8 bytes when not valid | [OS/path §2.4, §6] |
| `open_root(c)` | `CreateFileW(\\?\c.text, FILE_READ_ATTRIBUTES, …, FILE_FLAG_BACKUP_SEMANTICS)` → `GetFileInformationByHandleEx(FileIdInfo)` (root-id check) → `CloseHandle`; the `Root` keeps text and id only | [OS/project §2.2] |
| `volume(root)` | `CreateFileW(root, FILE_READ_ATTRIBUTES, …)` → `GetVolumeInformationByHandleW` (name, `FILE_SUPPORTS_USN_JOURNAL`) → `GetFileInformationByHandleEx(FileIdInfo)` (`VolumeSerialNumber` → `vol_key`) → `DeviceIoControl(FSCTL_QUERY_USN_JOURNAL)` (journal availability only) | [OS/project §4.1–§4.3] |
| `case_equivalent(dir)` | `CreateFileW(dir, FILE_READ_ATTRIBUTES, …, FILE_FLAG_BACKUP_SEMANTICS)` → `GetFileInformationByHandleEx(FileCaseSensitiveInfo)`; `ERROR_INVALID_PARAMETER` → the volume rule | [OS/project §4.5] |
| `trash_dirs(root)` | none: `<drive>:/$Recycle.Bin` derived from the root's text | [OS/project §5.7] |
| `measure_mtime_granularity(stamp_dir)` | repeated `touch_stamp`; the mono clock (`QueryPerformanceCounter`) for the budget | [OS/project §4.4] |
| `stat(at, Read)` | `GetFileAttributesExW(\\?\p, GetFileExInfoStandard)`; if `FILE_ATTRIBUTE_REPARSE_POINT`: `FindFirstFileExW(\\?\p, FindExInfoBasic, …)` for the reparse tag (`dwReserved0`), `FindClose` | [OS/project §5.1] |
| `stat(at, WithId)` | `CreateFileW(\\?\p, FILE_READ_ATTRIBUTES, share R\|W\|D, OPEN_EXISTING, FILE_FLAG_BACKUP_SEMANTICS \| FILE_FLAG_OPEN_REPARSE_POINT)` → `GetFileInformationByHandleEx` (`FileBasicInfo`, `FileStandardInfo`, `FileIdInfo`, `FileAttributeTagInfo`) → `CloseHandle`; the parent's id: the same open of the parent + `FileIdInfo`. Never on an entry last seen cloud-only | [OS/project §5.1, §5.10] |
| `disk_spelling(at)` | `CreateFileW(\\?\p, FILE_READ_ATTRIBUTES, …, FILE_FLAG_BACKUP_SEMANTICS \| FILE_FLAG_OPEN_REPARSE_POINT)` → `GetFinalPathNameByHandleW(FILE_NAME_NORMALIZED \| VOLUME_NAME_DOS)` → `CloseHandle` | [OS/project §5.3] |
| `enumerate(dir, visit)` | `CreateFileW(\\?\dir, FILE_LIST_DIRECTORY \| FILE_READ_ATTRIBUTES, …, FILE_FLAG_BACKUP_SEMANTICS)` → `GetFileInformationByHandleEx(FileIdInfo)` → `GetFileInformationByHandleEx(FileIdExtdDirectoryRestartInfo)`, then `FileIdExtdDirectoryInfo` until `ERROR_NO_MORE_FILES` (64 KiB, 8-byte aligned) → `CloseHandle`. Never on a `RECALL_ON_DATA_ACCESS` directory | [OS/project §5.2] |
| `locate_id(root, id, recorded)` | `CreateFileW(root, FILE_READ_ATTRIBUTES, …)` (volume hint) → `OpenFileById(hint, {sizeof, ExtendedFileIdType, FILE_ID_128}, FILE_READ_ATTRIBUTES, share R\|W\|D, NULL, FILE_FLAG_BACKUP_SEMANTICS \| FILE_FLAG_OPEN_REPARSE_POINT)` → `GetFinalPathNameByHandleW(FILE_NAME_NORMALIZED \| VOLUME_NAME_DOS)` → `CloseHandle` ×2 | [OS/project §5.4] |
| `file_handle_digest(at)` | none: `Ok(None)` | [OS/project §5.4] |
| `read_for_hash(at, opts)` | `GetFileAttributesExW` (placeholder gate) → `CreateFileW(\\?\p, GENERIC_READ, share R\|W\|D, OPEN_EXISTING, FILE_FLAG_SEQUENTIAL_SCAN \| FILE_FLAG_OPEN_REPARSE_POINT)` → `GetFinalPathNameByHandleW` (containment) | [OS/project §5.5] |
| `Reader::read` / `rewind` / `snapshot` / `identity` / drop | `ReadFile` / `SetFilePointerEx(0, FILE_BEGIN)` / `GetFileInformationByHandleEx(FileStandardInfo, FileBasicInfo)` / `GetFileInformationByHandleEx(FileIdInfo)` / `CloseHandle` | [OS/project §5.5] |
| `read_link(at, out)` | `CreateFileW(\\?\p, FILE_READ_ATTRIBUTES, …, FILE_FLAG_OPEN_REPARSE_POINT \| FILE_FLAG_BACKUP_SEMANTICS)` → `DeviceIoControl(FSCTL_GET_REPARSE_POINT)` (`IO_REPARSE_TAG_SYMLINK` only; `PrintName`, `\` → `/`) → `CloseHandle` | [OS/project §5.6] |
| `busy_holders(at)` | `RmStartSession` → `RmRegisterResources` (one file) → `RmGetList` → `RmEndSession`; a directory → `Unsupported` | [OS/project §5.8] |
| `touch_stamp(at)` | `CreateFileW(\\?\p, GENERIC_WRITE, share R\|W\|D, OPEN_ALWAYS)` → `WriteFile` (one byte `00` at offset 0) → `CloseHandle` → `GetFileAttributesExW` | [OS/project §5.9] |
| `rename_noreplace(from, to, retry)` | `MoveFileExW(\\?\from, \\?\to, MOVEFILE_WRITE_THROUGH)`; errors 5/32 retried per `ShareRetry` (`Sleep` steps of [OS/fs §6.3]) | [OS/project §6.1], [OS/fs §4.8] |
| `sync_dir(dir)` | `CreateFileW(\\?\dir, GENERIC_READ \| GENERIC_WRITE, share R\|W\|D, OPEN_EXISTING, FILE_FLAG_BACKUP_SEMANTICS)` → `FlushFileBuffers` → `CloseHandle` | [OS/project §6.2], [OS/fs §4.4.3] |
| `durable_rename(from, to, retry)` | `rename_noreplace` → `sync_dir(parent(from))` → `sync_dir(parent(to))` | [OS/project §6.3] |
| `unlink(at, retry)` | `GetFileAttributesW`; if read-only `SetFileAttributesW(attrs & !READONLY)` → `DeleteFileW(\\?\p)` (retried per `ShareRetry`); on failure `SetFileAttributesW(attrs)` | [OS/project §6.3], [OS/fs §4.7] |
| `remove_dir(at, retry)` | `RemoveDirectoryW(\\?\p)` (retried per `ShareRetry`) | [OS/project §6.3] |
| `durable_unlink(at, retry)` | `unlink` or `remove_dir` → `sync_dir(parent)` | [OS/project §6.3] |
| `counters()` | none (relaxed atomics) | [OS/project §2.4] |

### 2.2 `Clock`

| Method | Windows calls | Spec |
|---|---|---|
| `wall_ms()` | `GetSystemTimePreciseAsFileTime` → `(f − 116 444 736 000 000 000).div_euclid(10 000)`; plus the test-host offset in `test-host` builds | [OS/clock §2.1, §9] |
| `mono_ns()` | `QueryPerformanceCounter` × 10^9 / `QueryPerformanceFrequency` (u128) | [OS/clock §2.1] |
| `boot_ns()` | `QueryInterruptTimePrecise` × 100 (HOLE(OS-win-boot-clock)) | [OS/clock §2.1] |

### 2.3 `ProcHost` (`os::proc`, `os::spawn`)

| Method | Windows calls | Spec |
|---|---|---|
| `os_tag()` | none: 1 | [OS/proc §2] |
| `self_id()` | `GetCurrentProcessId`; `GetProcessTimes(GetCurrentProcess())` creation `FILETIME` → ns since the epoch; `boot_identity()` → `boot_hash` | [OS/proc §3] |
| `parent()` | `CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS)` → `Process32FirstW`/`Process32NextW` (own entry's `th32ParentProcessID`) → `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, parent)` → `GetProcessTimes` → `CloseHandle` ×2 | [OS/proc §3.3] |
| `boot_identity()` | the `BootId` member of `KUSER_SHARED_DATA` at `0x7FFE0000`; if 0: `RegGetValueW(HKLM, …\Memory Management\PrefetchParameters, "BootId", RRF_RT_REG_DWORD)`; `RegGetValueW(HKLM, SOFTWARE\Microsoft\Cryptography, "MachineGuid", RRF_RT_REG_SZ \| RRF_SUBKEY_WOW6464KEY)`; BLAKE3-128 per [OS/proc §4.2]; cached | [OS/proc §4] |
| `alive(p)` | `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION \| SYNCHRONIZE, p.pid)` → `WaitForSingleObject(h, 0)` → `GetProcessTimes` → `CloseHandle` | [OS/proc §6.1] |
| `watch_parent()` | the snapshot walk of `parent()` → `OpenProcess(SYNCHRONIZE \| PROCESS_QUERY_LIMITED_INFORMATION, ppid)` → `GetProcessTimes` (reuse guard) | [OS/proc §7] |
| `new_wake()` / `Wake::signal` | `CreateEventW(NULL, FALSE, FALSE, NULL)` / `SetEvent` | [OS/proc §7] |
| `wait_parent_or_wake(w, wake)` | `WaitForMultipleObjects(2, {parent, event}, FALSE, INFINITE)` | [OS/proc §7] |
| `parent_image()` | the parent's `PROCESSENTRY32W.szExeFile` from the snapshot | [OS/proc §8] |
| `spawn_gc_child(exe, args, cwd)` (not built at M0) | `CreateProcessW(exe, cmdline, NULL, NULL, FALSE, BELOW_NORMAL_PRIORITY_CLASS \| DETACHED_PROCESS \| CREATE_NEW_PROCESS_GROUP \| CREATE_BREAKAWAY_FROM_JOB, NULL, cwd, …)`; on `ERROR_ACCESS_DENIED` the same without `CREATE_BREAKAWAY_FROM_JOB`; `CloseHandle` ×2 | [OS/proc §11] |
| `enter_background()` | `SetPriorityClass(GetCurrentProcess(), PROCESS_MODE_BACKGROUND_BEGIN)`; `SetProcessInformation(GetCurrentProcess(), ProcessMemoryPriority, {MEMORY_PRIORITY_LOW})` | [OS/proc §11] |

### 2.4 `Meter` (`os::mem`; `os::proc::peak_of_child`)

| Method | Windows calls | Spec |
|---|---|---|
| `free_space(dir)` | `GetDiskFreeSpaceExW` | [OS/fs §4.11] |
| `private_now()` / `private_peak()` | `GetProcessMemoryInfo(GetCurrentProcess(), PROCESS_MEMORY_COUNTERS_EX)` → `PrivateUsage` / `PeakPagefileUsage` | [OS/mem §3] |
| `available_physical()` | `GlobalMemoryStatusEx` → `ullAvailPhys` | [OS/mem §5] |
| `prepare_child(cmd)` / `bind_child(t, child)` | none: `ChildTicket(0)` / nothing | [OS/proc §9] |
| `peak_of_child(child)` | `WaitForSingleObject(child, INFINITE)` → `GetProcessMemoryInfo(child, PROCESS_MEMORY_COUNTERS_EX)` → `PeakPagefileUsage` | [OS/proc §9] |
| `child_private_now(child)` | `GetProcessMemoryInfo(child, PROCESS_MEMORY_COUNTERS_EX)` → `PrivateUsage` | [OS/mem §7] |
| `child_threads(child)` | `CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD)` → `Thread32First`/`Thread32Next`, count by `th32OwnerProcessID` | [OS/mem §7] |
| `cpu_times(child?)` | `GetProcessTimes` (user, kernel `FILETIME` × 100) | [OS/mem §7] |
| `heap_counts()` / `reset_heap_high_water()` | none: `CountingAlloc`'s atomics (installed by probe roots only) | [OS/mem §6] |

### 2.5 `os::ipc` (only if the leader is built; not built at M0)

| Function | Windows calls | Spec |
|---|---|---|
| `bind_endpoint` | `GetTokenInformation(TokenUser)` + `ConvertSidToStringSidW` (the `<u>` part) → `InitializeSecurityDescriptor` + `SetEntriesInAclW` (the user's SID only) → `CreateNamedPipeW(\\.\pipe\moirai-<u>-<s>, PIPE_ACCESS_DUPLEX \| FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_TYPE_BYTE \| PIPE_REJECT_REMOTE_CLIENTS, …)` | [OS/proc §12] |
| `accept` / `connect` | `ConnectNamedPipe` / `CreateFileW(\\.\pipe\…, GENERIC_READ \| GENERIC_WRITE, 0, OPEN_EXISTING)` | [OS/proc §12] |
| `peer` | `GetNamedPipeClientProcessId` (server side), `GetNamedPipeServerProcessId` (client side) | [OS/proc §12] |

### 2.6 `os::term`

| Contract item | Windows | Spec |
|---|---|---|
| console or not | `std::io::IsTerminal` (`GetConsoleMode`, plus MSYS pty detection) | [OS/shell §10] |
| writing text | `std::io::Stdout`/`Stderr`: `WriteConsoleW` after UTF-8 → UTF-16 on a console, `WriteFile` of the bytes otherwise | [OS/shell §6] |
| reading stdin | `std::io::Stdin`: `ReadConsoleW` on a console (Ctrl+Z ends input), `ReadFile` otherwise | [OS/shell §5.2] |
| broken pipe | `ERROR_BROKEN_PIPE` (109), `ERROR_NO_DATA` (232) → `ErrorKind::BrokenPipe` → exit 0 | [OS/shell §6] |

### 2.7 `os::test_host` (feature `test-host`)

| Function | Windows calls | Spec |
|---|---|---|
| `kill(child)` | `TerminateProcess(child, 1)` | [OS/proc §13] |
| `suspend(pid)` / `resume(pid)` | `OpenProcess(PROCESS_SUSPEND_RESUME)` → `NtSuspendProcess` / `NtResumeProcess` (ntdll; test-only, declared locally) → `CloseHandle` | [OS/proc §13] |
| `small_volume(bytes)` | `CreateVirtualDisk` + `AttachVirtualDisk` (VHDX; elevation, owner-run only) | [OS/proc §13] |
| wall offset | `std::env::var("MOIRAI_TEST_WALL_OFFSET_MS")` at start; `set_wall_offset_ms` | [OS/proc §13], [OS/clock §9] |

### 2.8 `windows-sys` feature families used by part 2 [I]

`Win32_Foundation`, `Win32_Storage_FileSystem` (files, `OpenFileById`, `MoveFileExW`, `GetFinalPathNameByHandleW`,
`GetVolumeInformationByHandleW`, `GetDiskFreeSpaceExW`), `Win32_System_IO` and `Win32_System_Ioctl`
(`DeviceIoControl`, `FSCTL_GET_REPARSE_POINT`, `FSCTL_QUERY_USN_JOURNAL`), `Win32_System_Threading` (processes, events,
priorities, `SetProcessInformation`), `Win32_System_ProcessStatus` (`GetProcessMemoryInfo`),
`Win32_System_SystemInformation` (`GlobalMemoryStatusEx`, `GetSystemTimePreciseAsFileTime`),
`Win32_System_WindowsProgramming` (`QueryInterruptTimePrecise`), `Win32_System_Performance` (`QueryPerformanceCounter`),
`Win32_System_Diagnostics_ToolHelp` (snapshots), `Win32_System_Registry` (`RegGetValueW`), `Win32_System_RestartManager`
(`Rm*`), `Wdk_System_SystemServices` (`KUSER_SHARED_DATA`); for the leader only `Win32_System_Pipes`, `Win32_Security`,
`Win32_Security_Authorization`; for `test-host` only `Win32_Storage_Vhd`. WP-33 confirms the family of each item against
the pinned `windows-sys` release (README Appendix A lists part 1's).

## 3. Linux ≥ 5.10, 64-bit, static musl (port phase)

### 3.1 `ProjectFs` and `os::path`

| Method | Linux calls | Spec |
|---|---|---|
| `canonical_root(dir)` | `open(dir, O_PATH \| O_DIRECTORY \| O_CLOEXEC)` → `readlink("/proc/self/fd/<fd>")` → per component in a casefold parent (`ioctl(FS_IOC_GETFLAGS)`): `getdents64` of the parent, the entry whose `d_ino` is the component's (`statx`), confirmed by `name_to_handle_at` → `statx` + `name_to_handle_at` for the root id → `fstatfs` | [OS/path §4.2] |
| `canonical_abs(p)` | existing: as `canonical_root` (`O_PATH` without `O_DIRECTORY`); absent: `getcwd` + lexical normalisation | [OS/path §5] |
| `cli_path` | argv bytes (must be UTF-8); `canonical_root(cwd)` | [OS/path §7] |
| `representable_here(seg)` | none (≤ 255 bytes) | [OS/path §8.1] |
| `user_config_path()` | `XDG_CONFIG_HOME`, `HOME`, else `getpwuid_r(geteuid())` | [OS/path §10] |
| `open_root(c)` | `open(c.text, O_PATH \| O_DIRECTORY \| O_CLOEXEC)` → `statx` + `name_to_handle_at` (root-id check); the `Root` keeps the descriptor | [OS/project §2.2] |
| `volume(root)` | `fstatfs(root_fd)` (`f_type`, `f_fsid` → `vol_key`) → `name_to_handle_at(root_fd, "", …, AT_EMPTY_PATH)` (ids trusted?) → `statx(root_fd, "", AT_EMPTY_PATH, STATX_BTIME)` (`stx_mask`) → `ioctl(FS_IOC_GETFLAGS)` support (casefold-capable) | [OS/project §4.1–§4.3] |
| `case_equivalent(dir)` | `openat2(…, O_RDONLY \| O_DIRECTORY)` → `ioctl(FS_IOC_GETFLAGS)` & `FS_CASEFOLD_FL` | [OS/project §4.5] |
| `trash_dirs(root)` | `XDG_DATA_HOME`/`HOME`; the mount point from `/proc/self/mountinfo`; `getuid` | [OS/project §5.7] |
| `measure_mtime_granularity` | repeated `touch_stamp`; `CLOCK_MONOTONIC` for the budget | [OS/project §4.4] |
| `stat(at, Read \| WithId)` | `statx(root_fd, rel, AT_SYMLINK_NOFOLLOW, STATX_BASIC_STATS \| STATX_BTIME)` + `name_to_handle_at(root_fd, rel, …, AT_SYMLINK_NOFOLLOW)` when ids are trusted | [OS/project §5.1] |
| `disk_spelling(at)` | per component in a casefold parent: `getdents64` of the parent, matched by inode | [OS/project §5.3] |
| `enumerate(dir, visit)` | `openat2(root_fd, rel, {O_RDONLY \| O_DIRECTORY \| O_CLOEXEC, RESOLVE_BENEATH \| RESOLVE_NO_SYMLINKS})` → `getdents64` (32 KiB) → `close` | [OS/project §5.2] |
| `locate_id` | none: `NotLocatable` (the frontier of [80 §2.11.3] runs on `enumerate`, `stat`, `file_handle_digest`) | [OS/project §5.4] |
| `file_handle_digest(at)` | `name_to_handle_at(root_fd, rel, fh(MAX_HANDLE_SZ), &mount_id, AT_SYMLINK_NOFOLLOW)` → BLAKE3-256(`u32le(handle_type) ‖ f_handle`)[0..8] | [OS/project §3.1, §5.4] |
| `read_for_hash(at, opts)` | `openat2(root_fd, rel, {O_RDONLY \| O_CLOEXEC \| O_NOFOLLOW \| O_NOATIME, RESOLVE_BENEATH \| RESOLVE_NO_SYMLINKS})`; `EPERM` → again without `O_NOATIME`; `posix_fadvise(POSIX_FADV_SEQUENTIAL)` | [OS/project §5.5] |
| `Reader::read` / `rewind` / `snapshot` / `identity` / drop | `read` / `lseek(0, SEEK_SET)` / `statx(fd, "", AT_EMPTY_PATH)` / `statx` + `name_to_handle_at(fd, "", …, AT_EMPTY_PATH)` / `close` | [OS/project §5.5] |
| `read_link(at, out)` | `readlinkat(root_fd, rel)` | [OS/project §5.6] |
| `busy_holders` | none: `Ok(vec![])` | [OS/project §5.8] |
| `touch_stamp(at)` | `openat(root_fd, rel, O_WRONLY \| O_CREAT \| O_CLOEXEC, 0o644)` → `pwrite(1 byte 00, 0)` → `close` → `statx` | [OS/project §5.9] |
| `rename_noreplace(from, to, retry)` | `renameat2(from_fd, from, to_fd, to, RENAME_NOREPLACE)`; `EINVAL` on a file: `linkat` + `unlinkat`; `retry` ignored | [OS/project §6.1] |
| `sync_dir(dir)` | `openat(root_fd, dir, O_RDONLY \| O_DIRECTORY \| O_CLOEXEC)` → `fsync` → `close` | [OS/project §6.2] |
| `durable_rename` / `durable_unlink` | `rename_noreplace` / `unlink`, then `sync_dir` of the parent(s) | [OS/project §6.3] |
| `unlink` / `remove_dir` | `unlinkat(root_fd, rel, 0)` / `unlinkat(root_fd, rel, AT_REMOVEDIR)` | [OS/project §6.3] |

### 3.2 `Clock`

| Method | Linux calls | Spec |
|---|---|---|
| `wall_ms()` | `clock_gettime(CLOCK_REALTIME)` | [OS/clock §2.1] |
| `mono_ns()` | `clock_gettime(CLOCK_MONOTONIC)` | [OS/clock §2.1] |
| `boot_ns()` | `clock_gettime(CLOCK_BOOTTIME)` | [OS/clock §2.1] |

### 3.3 `ProcHost`

| Method | Linux calls | Spec |
|---|---|---|
| `self_id()` | `getpid`; `/proc/self/stat` field 22 and `sysconf(_SC_CLK_TCK)`; `stat("/proc/self/ns/pid")` → `st_ino` | [OS/proc §3.2] |
| `parent()` | `getppid`; `/proc/<ppid>/stat` field 22 | [OS/proc §3.3] |
| `boot_identity()` | `read("/proc/sys/kernel/random/boot_id")` | [OS/proc §4.2] |
| `alive(p)` | `/proc/<pid>/stat` (state, field 22); `ENOENT` → Dead; `EACCES` → Unknown | [OS/proc §6.1] |
| `watch_parent()` / `wait_parent_or_wake` | `pidfd_open(getppid(), 0)` → re-check `getppid()`; `poll({pidfd, eventfd})`; never `PR_SET_PDEATHSIG` | [OS/proc §7] |
| `new_wake()` / `signal` | `eventfd(0, EFD_CLOEXEC)` / `write(8 bytes)` | [OS/proc §7] |
| `parent_image()` | `/proc/<ppid>/comm` | [OS/proc §8] |
| `spawn_gc_child` | `posix_spawn` with `POSIX_SPAWN_SETSID`, streams on `/dev/null` | [OS/proc §11] |
| `enter_background()` | `setpriority(PRIO_PROCESS, 0, 10)`; `ioprio_set(IOPRIO_WHO_PROCESS, 0, IOPRIO_CLASS_IDLE)` | [OS/proc §11] |

### 3.4 `Meter`

| Method | Linux calls | Spec |
|---|---|---|
| `free_space(dir)` | `fstatvfs` | [OS/fs §4.11] |
| `private_now()` | `/proc/self/smaps_rollup` (`Anonymous + Swap − LazyFree`) + `/proc/self/status` `VmPTE` | [OS/mem §3] |
| `private_peak()` | cgroup-v2 `memory.peak` of the process's own leaf cgroup (`/proc/self/cgroup`), else `MeterError` | [OS/mem §3] |
| `available_physical()` | `/proc/meminfo` `MemAvailable` | [OS/mem §5] |
| `prepare_child` / `bind_child` / `peak_of_child` | `mkdir` of a leaf under the delegated cgroup; the child writes its pid to `cgroup.procs` before `exec`; `memory.peak`; `rmdir` of the leaf | [OS/proc §9] |
| `child_private_now` / `child_threads` / `cpu_times` | `/proc/<pid>/smaps_rollup` / `/proc/<pid>/status` `Threads` / `/proc/<pid>/stat` fields 14–15 | [OS/mem §7] |

### 3.5 `os::ipc`, `os::term`, `os::test_host`

| Item | Linux calls | Spec |
|---|---|---|
| `bind_endpoint` / `connect` / `peer` | `mkdir(0700)` + `lstat` check → `socket(AF_UNIX, SOCK_STREAM \| SOCK_CLOEXEC)` → `bind` + `listen` / `connect` / `getsockopt(SO_PEERCRED)` | [OS/proc §12] |
| `os::term` | `std::io` stdio; `IsTerminal` (`isatty`); `EPIPE` with `SIGPIPE` ignored → exit 0 | [OS/shell §10] |
| `kill` / `suspend` / `resume` | `kill(SIGKILL)` / `kill(SIGSTOP)` / `kill(SIGCONT)` | [OS/proc §13] |
| `small_volume` | a loop-mounted image (privileged CI job) | [OS/proc §13] |

## 4. macOS ≥ 14, arm64 (port phase)

### 4.1 `ProjectFs` and `os::path`

| Method | macOS calls | Spec |
|---|---|---|
| `canonical_root(dir)` | `open(dir, O_RDONLY \| O_DIRECTORY \| O_CLOEXEC)` → `fcntl(F_GETPATH)` → per component `getattrlist(ATTR_CMN_NAME)` → `getattrlist(ATTR_CMN_FILEID, ATTR_VOL_UUID)` | [OS/path §4.3] |
| `canonical_abs(p)` | as `canonical_root`; absent: `getcwd` + lexical normalisation | [OS/path §5] |
| `user_config_path()` | as Linux | [OS/path §10] |
| `open_root(c)` | `open(c.text, O_RDONLY \| O_DIRECTORY \| O_CLOEXEC)` → `getattrlist` (root-id check) | [OS/project §2.2] |
| `volume(root)` | `fstatfs` (`f_fstypename`, `MNT_LOCAL`) → `getattrlist(ATTR_VOL_UUID, ATTR_VOL_CAPABILITIES)` (`VOL_CAP_FMT_PERSISTENTOBJECTIDS`, `VOL_CAP_FMT_PATH_FROM_ID`, `VOL_CAP_FMT_CASE_SENSITIVE`, `VOL_CAP_INT_RENAME_EXCL`) | [OS/project §4.1–§4.3] |
| `case_equivalent(dir)` | the volume's `VOL_CAP_FMT_CASE_SENSITIVE`; normalisation-insensitive always | [OS/project §4.5] |
| `trash_dirs(root)` | `HOME` → `~/.Trash`; the volume root → `.Trashes/<uid>` | [OS/project §5.7] |
| `stat(at, Read \| WithId)` | `getattrlistat(root_fd, rel, …, FSOPT_NOFOLLOW)` (`ATTR_CMN_OBJTYPE`, `FILEID`, `PARENTID`, `MODTIME`, `CHGTIME`, `CRTIME`, `ADDEDTIME`, `FLAGS`, `ATTR_FILE_DATALENGTH`, `ATTR_CMNEXT_EXT_FLAGS`, `ATTR_CMNEXT_CLONE_REFCNT`) | [OS/project §5.1] |
| `disk_spelling(at)` | per component `getattrlist(ATTR_CMN_NAME)` | [OS/project §5.3] |
| `enumerate(dir, visit)` | `openat(root_fd, rel, O_RDONLY \| O_DIRECTORY \| O_CLOEXEC \| O_NOFOLLOW_ANY)` → `getattrlistbulk` (the attributes of `stat`) → `close` | [OS/project §5.2] |
| `locate_id(root, id, recorded)` | `fsgetpath(buf, len, &fsid, fileid)`; containment by prefix | [OS/project §5.4] |
| `file_handle_digest` | none: `Ok(None)` | [OS/project §5.4] |
| `read_for_hash(at, opts)` | `setiopolicy_np(IOPOL_TYPE_VFS_ATIME_UPDATES, IOPOL_SCOPE_PROCESS, IOPOL_ATIME_UPDATES_OFF)` and `(IOPOL_TYPE_VFS_MATERIALIZE_DATALESS_FILES, …, IOPOL_MATERIALIZE_DATALESS_FILES_OFF)` once per process → `fstatat` (`SF_DATALESS`) → `openat(root_fd, rel, O_RDONLY \| O_CLOEXEC \| O_NOFOLLOW_ANY)` → `fstat` re-check → `fcntl(F_RDAHEAD, 1)` | [OS/project §5.5] |
| `Reader::*` | `read` / `lseek` / `fstat` / `fgetattrlist(ATTR_CMN_FILEID)` / `close` | [OS/project §5.5] |
| `read_link(at, out)` | `readlinkat(root_fd, rel)` | [OS/project §5.6] |
| `busy_holders` | none: `Ok(vec![])` | [OS/project §5.8] |
| `touch_stamp(at)` | as Linux (`openat` → `pwrite` → `close` → `fstatat`) | [OS/project §5.9] |
| `rename_noreplace(from, to, retry)` | `renameatx_np(from_fd, from, to_fd, to, RENAME_EXCL)` iff `VOL_CAP_INT_RENAME_EXCL`, else `Unsupported` | [OS/project §6.1] |
| `sync_dir(dir)` | `fsync(dirfd)` → `fcntl(dirfd, F_FULLFSYNC)` | [OS/project §6.2] |
| `durable_rename` | `renameatx_np` → `fsync` of both parents → one `F_FULLFSYNC` on the second (`sync_group`) | [OS/project §6.3] |
| `unlink` / `remove_dir` / `durable_unlink` | `unlinkat` / `unlinkat(AT_REMOVEDIR)` / then `sync_dir` | [OS/project §6.3] |

### 4.2 `Clock`

| Method | macOS calls | Spec |
|---|---|---|
| `wall_ms()` | `clock_gettime(CLOCK_REALTIME)` | [OS/clock §2.1] |
| `mono_ns()` | `clock_gettime_nsec_np(CLOCK_UPTIME_RAW)` | [OS/clock §2.1] |
| `boot_ns()` | `mach_continuous_time()` × `numer` / `denom` (`mach_timebase_info`) | [OS/clock §2.1] |

### 4.3 `ProcHost`

| Method | macOS calls | Spec |
|---|---|---|
| `self_id()` / `parent()` | `getpid` / `getppid`; `sysctl({CTL_KERN, KERN_PROC, KERN_PROC_PID, pid})` → `kp_proc.p_starttime` | [OS/proc §3] |
| `boot_identity()` | `sysctlbyname("kern.bootsessionuuid")` (undocumented; absence or denial → Unknown-boot mode) | [OS/proc §4.2] |
| `alive(p)` | `sysctl(KERN_PROC_PID)`: `EPERM` → Unknown; `ESRCH`/empty → Dead; `p_stat = SZOMB` → Dead | [OS/proc §6.1] |
| `watch_parent()` / `wait_parent_or_wake` / `new_wake` | `kqueue` → `EVFILT_PROC` `NOTE_EXIT` on `getppid()` → re-check `getppid()` → `kevent`; `EVFILT_USER` with `NOTE_TRIGGER` as the wake | [OS/proc §7] |
| `parent_image()` | `kinfo_proc.kp_proc.p_comm` | [OS/proc §8] |
| `spawn_gc_child` | `posix_spawn` with `POSIX_SPAWN_SETSID`, streams on `/dev/null` | [OS/proc §11] |
| `enter_background()` | `setpriority(PRIO_DARWIN_PROCESS, 0, PRIO_DARWIN_BG)`; `setiopolicy_np(IOPOL_TYPE_DISK, IOPOL_SCOPE_PROCESS, IOPOL_THROTTLE)` | [OS/proc §11] |

### 4.4 `Meter`

| Method | macOS calls | Spec |
|---|---|---|
| `free_space(dir)` | `fstatfs` | [OS/fs §4.11] |
| `private_now()` / `private_peak()` | `task_info(mach_task_self(), TASK_VM_INFO)` → `phys_footprint` / `ledger_phys_footprint_peak` | [OS/mem §3] |
| `available_physical()` | `host_statistics64(HOST_VM_INFO64)` → `(free_count + inactive_count + purgeable_count) × vm_kernel_page_size` [I] | [OS/mem §5] |
| `peak_of_child` | `proc_pid_rusage(RUSAGE_INFO_V6).ri_lifetime_max_phys_footprint` before the reap (harness only) | [OS/proc §9] |
| `child_private_now` / `child_threads` / `cpu_times` | `proc_pid_rusage` `ri_phys_footprint` / `proc_pidinfo(PROC_PIDTASKINFO).pti_threadnum` / `ri_user_time`, `ri_system_time` (harness only) | [OS/mem §7] |

### 4.5 `os::ipc`, `os::term`, `os::test_host`

| Item | macOS calls | Spec |
|---|---|---|
| `bind_endpoint` / `connect` / `peer` | `confstr(_CS_DARWIN_USER_TEMP_DIR)` → `mkdir(0700)` + `lstat` → `socket(AF_UNIX)` → `bind` + `listen` / `connect` / `getpeereid` + `getsockopt(LOCAL_PEERPID)` | [OS/proc §12] |
| `os::term` | as Linux | [OS/shell §10] |
| `kill` / `suspend` / `resume` / `small_volume` | as Linux; `hdiutil create` + `attach` for the volume | [OS/proc §13] |

## 5. Index of part 1's mappings (`StoreFs`, `Locks`, `SealedMaps`, `EnvGuard`)

Part 1 maps its sub-traits of `Vfs` in the Appendix A of its own files; they are not repeated here, so that one table owns
each row.

| Sub-trait | Methods | Per-OS table |
|---|---|---|
| `StoreFs` ([OS/fs §3]) | `open_root`, `create_root`, `open`, `create_new`, `create_dir`, `remove_dir`, `list_dir`, `read_at`, `read_exact_at`, `write_at`, `sync`, `sync_dir`, `sync_group`, `fail_stop`, `create_extent`, `recycle_extent`, `seal`, `unlink`, `rename_noreplace`, `rename_replace`, `swap_dirs`, `swap_recover`, `file_size`, `identity`, `root_identity`, `path_identity`, `free_space`, `advise_dontneed`, `counters` | [OS/fs] Appendix A (rows A1–A12 and the rows after them) and §4.4–§5 |
| `Locks` ([OS/lock §4]) | `lock_client`, `lock_data`, `try_acquire`, `acquire_within`, `release`, `probe`, `holds`, `holds_any_role`, `foreign_lock_check` | [OS/lock] Appendix A, §7, §8, §11 |
| `SealedMaps` ([OS/map §3]) | `map_sealed`, `advise`, unmap on drop; the registry; the fault handler | [OS/map] Appendix A |
| `EnvGuard` ([OS/env §2]) | `classify`, `probe_store`, `check_os_version`, `doctor_warnings` | [OS/env] Appendix A |
| `Clock`, `ProcHost` | as §2.2–§2.3, §3.2–§3.3, §4.2–§4.3 of this file | this file |

## 6. Calls never used by the part-2 surfaces

| Never | Why | Spec |
|---|---|---|
| `std::fs::rename`, `std::fs` file access outside `moirai-os` | replaces silently on Unix; bypasses I-F11's gates | [OS/README §2.5], [OS/project §1] |
| `MOVEFILE_REPLACE_EXISTING`, `MOVEFILE_COPY_ALLOWED` on a project rename; `rename(2)` for `file mv` | a `file mv` must never replace or copy | [OS/project §6.1] |
| any copy of a project file (`CopyFileExW`, `copy_file_range`, `clonefile`) | cross-volume moves are refused (A1P-01) | [OS/project §6.4] |
| opening a cloud-only entry in an automatic path; enumerating a `RECALL_ON_DATA_ACCESS` directory | hydration | [OS/project §5.10] |
| `nFileIndex`, `st_ino` alone as identity | ReFS −1; ext4 lowest-free reuse | [OS/project §3.1, §3.2] |
| `open_by_handle_at`, `openbyid_np` | need a capability or an entitlement | [80 §2.11.1] |
| the Windows system boot time (`NtQuerySystemInformation(SystemTimeOfDayInformation)`) as boot identity | it moves when the clock is set | [OS/proc §4.1] |
| `PR_SET_PDEATHSIG` | fires when the spawning thread exits | [OS/proc §7] |
| libproc (`proc_pidinfo`, `proc_pid_rusage`) in product code | private (X9); harness only | [OS/proc §3.2], [OS/mem §7] |
| `SHGetKnownFolderPath` for the user-scope config | loads `shell32.dll` | [OS/path §10] |
| `FormatMessageW`, `strerror` texts in output | localised, not ASCII | [OS/shell §6] |
| an abstract Unix socket; a `$TMPDIR`-derived endpoint; a socket inside `.git/moirai` | [80 §2.8] | [OS/proc §12] |
| `ru_maxrss`, `/proc/*/status` RSS as a gated quantity | includes file-backed pages | [OS/mem §2] |

## Holes

The part-2 holes are listed in their chapters and only referenced here: HOLE(OS-win-boot-source) ([OS/proc]),
HOLE(OS-win-boot-clock) ([OS/clock]), HOLE(OS-pfs-gran-probe-k) and HOLE(OS-pfs-gran-probe-budget) ([OS/project]).
[OS/project §4.3]'s NTFS rows take HOLE(F20-btime-ntfs) and HOLE(F20-ctime-rename), which chapter 20 owns.

| Id | What | Decided by | Candidates | Constraint the value must meet |
|---|---|---|---|---|
| — | none of its own | — | — | — |

## Open points for the review

| # | Point | Resolution in this file | For |
|---|---|---|---|
| 1 | PLAN WP-17 asks for "the mapping appendix for Windows, Linux and macOS", while part 1 wrote an Appendix A in each of its files | this file maps the part-2 surfaces per OS and indexes part 1's appendices (§5), so every method of `Vfs`, `ProjectFs` and `Meter` has exactly one per-OS row owner | WP-17a |
| 2 | The `windows-sys` feature families of §2.8 are named from memory of the crate's layout [I] | WP-33 confirms each against the pinned release; a different family name changes no call | WP-33 |
| 3 | `NtSuspendProcess`/`NtResumeProcess` are undocumented | allowed only in `os::test_host` (test builds), as GT4's suspend variant needs ([AR §8.2]); never reachable from the product root ([OS/README §2.4]) | R-REV-P |
