# 09 — File identity and change tracking at the OS level (R4 research, lens: OS signals)

*Research for moirai requirement **R4** (2026-09-26): links from moirai nodes to project files must survive rename, move and delete, either through moirai file commands or through automatic re-binding when files are moved by any means. This report covers only the **operating-system layer**: which OS-provided identity and change signals exist, which edit and move patterns keep or break them, what they cost, and what a lazy pass without a daemon can use on Windows without administrator rights. Content fingerprinting, git rename detection and anchors inside files belong to other lenses and appear here only where the OS evidence forces them.*

*Date: 2026-09-26. Nothing in moirai was implemented. The probe scripts are not published. Repositories were only walked and stat-ed read-only; no file was created in or modified in any repository, and no data (file contents) of BoykoEngine was read. Tags: **[M]** measured here, **[D]** documented by the vendor or a spec, **[C]** claimed by a third party, **[I]** my inference.*

---

## 0. Executive summary

**Short answer.** Windows gives moirai three usable identity signals: the path, the NTFS file ID and the NTFS object ID. It also gives two change feeds: the USN journal and ReadDirectoryChangesW. None of them survives the edit pattern that moirai's main writers use.

1. **The NTFS file ID (`FILE_ID_128` plus the 64-bit volume serial) follows a file through every rename and move within one volume**, whoever does it: Explorer's engine, `mv`, `git mv`, PowerShell `Move-Item`, a directory rename or a case-only rename [M]. It can be turned back into the current path **without admin rights** (`OpenFileById` + `GetFinalPathNameByHandleW`) [M].
2. **Any operation that replaces a file with a new file gives it a new file ID.** This includes Claude Code's own `Write` and `Edit` tools: both write a temp file and rename it over the target. So *every agent edit changes the file ID* [M], which confirms [anthropics/claude-code#92419](https://github.com/anthropics/claude-code/issues/92419) [C]. The same happens with `sed -i`, Python `os.replace`, JetBrains "safe write", copy+delete (which is also what a move across volumes or worktrees is), and every git operation that rewrites file content: checkout, switch, stash, reset --hard, restore, merge and rebase [M]. git never keeps the ID of a file whose content changes.
3. **NTFS object IDs, which the Distributed Link Tracking service and `.lnk` shortcuts use, follow the *name* through a delete-and-recreate within about 15 s ("tunneling").** So they survive git's rewrites, `perl -i`, PowerShell 5.1 `Move-Item -Force` and rename-away-then-create saves [M]. They do **not** survive the rename-over pattern that Claude Code, `sed -i` and `os.replace` use (`MoveFileEx(REPLACE_EXISTING)`) [M]. `ReplaceFileW` kept the object ID in only 4 of 6 trials without a backup file (2 of 2 with one) [M]. Object IDs must also be *written* onto the user's files: that bumps ChangeTime but not mtime or the creation time, and `git status` stays clean [M]. ReFS and Dev Drive have no object IDs at all [D].
4. **The USN change journal can be read by a normal user.** `FSCTL_READ_USN_JOURNAL` and `FSCTL_ENUM_USN_DATA` return *access denied* without elevation [M]. The Windows 10 1607+ code `FSCTL_READ_UNPRIVILEGED_USN_JOURNAL` [D, SDK header] works through any file or directory handle opened with `FILE_READ_ATTRIBUTES` [M]. It returns the 128-bit file ID, the parent ID, the USN and the reason flags for **every** change on the volume, **with the file names removed** [M]. So a tool can replay "what changed since cursor X" in 1–9 ms for a few hundred records, with no resident process [M]. **But the owner's D: volume (all repos and worktrees) has no journal** (`ERROR_JOURNAL_NOT_ACTIVE`, 1179) [M]. Creating one needs admin once [D]. The 32 MB journal on C: kept only **1.1–1.7 h** of history under today's load [M].
5. **ReadDirectoryChangesW needs a resident process and loses events when its consumer stalls.** An overflow drops the whole buffer; my sentinel file's event was lost [M]. It cannot exclude subtrees, and the live worktree tree produced ~34 events/s, almost all from cargo build output [M].
6. **A pass without a daemon is affordable if it is scoped.** Enumerating a directory with `FileIdExtdDirectoryInfo` returns 128-bit IDs for every entry from one handle per directory: 12–22 µs per file warm, 43–51 ms for the BoykoEngine main checkout (3,603 files) [M]. Opening a handle per file costs 53–374 µs, `OpenFileById` 103–544 µs, and `OpenFileById` plus the path 240–570 µs [M]. Walking all 44 worktrees took **12 s warm and 123 s cold** for 208,498 files, so a pass must never walk more than the current worktree [M].

**Verdict for R4 on Windows.** OS identity is reliable as a **move detector**, not as file identity. A pass should use the path first and then the file ID. With these rules alone, moirai handles every pure rename or move and every in-place replace at the same path, including agent edits. It loses the file only when a replace and a move happen between two passes: edit-then-move, a git checkout that moves a file, copy+delete, or a move to another volume or worktree. Those cases need the content-fingerprint and git lenses. The USN journal would make the pass exact, but on D: it needs a one-time admin step. Object IDs add little for moirai's own writers and cost a write to the user's files. Worktrees give every lane its own file IDs, so an OS ID is only ever a cache valid inside one worktree.

---

## 1. Environment and method

| Item | Value |
|---|---|
| OS | Windows 11 Home Single Language 10.0.26200 [M] |
| Volumes | C: and D: are NTFS partitions on **one** NVMe SSD (HFM512GD3JX013N, 512 GB) [M]. `GetVolumeInformationW` flags on both: object IDs Y, open-by-file-id Y, USN journal Y, hard links Y [M] |
| Token | `TokenIsElevated=0`, `TokenElevationType=3` (limited), Medium integrity; `BUILTIN\Administrators` present as **deny-only** [M]. The account is an administrator under UAC, and every measurement ran **non-elevated** |
| Defender | Real-time and on-access protection on, normal mode [M]; exclusions not readable without admin |
| Link tracking | `TrkWks` (Distributed Link Tracking Client) running, automatic start [M] |
| Last-access updates | `DisableLastAccess = 2` (system-managed, **enabled**) [M]. For this reason I never read file *contents* in BoykoEngine or `<lanes-dir>`. Walks used only directory enumeration and handles opened with `FILE_READ_ATTRIBUTES` |
| USN journals | C: **active** (32 MB max, 8 MB allocation delta, record versions 2–4). D: **not active** (`fsutil` and the probe both return 1179) [M] |
| Load during tests | **CPU ~100 %**, 1.7–1.8 GB of 15.4 GB RAM available, 8 cargo + 4 rustc + 16 claude processes from other lanes [M]. **All timings are noisy and reported as ranges over repeated runs.** |
| Toolchain | Rust 1.98.1 (probe `fidprobe`, zero dependencies, raw Win32 imports), Python 3.14.5 + ctypes, PowerShell 5.1, Git for Windows 2.54.0 (MSYS2 vim 9.2, GNU sed, perl) |

**Probes** (not published): `fidprobe/` (Rust: `info`, `mkoid`, `byid`, `usntest`, `usnread`, `walk`, `exts`, `watch`, `seqreuse`, `oidbench`); `survive.py` (a matrix of OS operations, PowerShell and Unix tools); `gitsurvive.py` (git operations in a throwaway repo); `tunnel.py` (tunneling window and rename-over variants); `lnk.ps1` (shell-link resolution); `rdcw.py` (ReadDirectoryChangesW pairs and overflow); `unpriv.py` (input variants of the unprivileged journal read). Test files were created only under `…/os-identity/work*`. One scratch file was sent to the Recycle Bin to observe it there, and I deleted its `$R`/`$I` pair afterwards.

---

## 2. The identity signals Windows offers

### 2.1 NTFS file ID (file reference number)

- **What it is [D].** On NTFS the 64-bit file ID is "the low 48 bits … index of the file's primary record in the MFT; the remaining 16 bits are a sequence number". In `FILE_ID_128` the high 64 bits are zero ([MS-FSCC Appendix B, notes 10–11](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-fscc/d4bc551b-7aaf-4b4f-ba0e-3a75e7c528f0)). The file ID together with the volume serial "uniquely identify a file on a single computer" ([FILE_ID_INFO](https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-file_id_info)). "In the NTFS file system, a file keeps the same file ID until it is deleted", and after `ReplaceFile` "the file ID of the replacement file, not the replaced file, is retained" ([BY_HANDLE_FILE_INFORMATION](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/ns-fileapi-by_handle_file_information)).
- **Confirmed [M].** `nFileIndex` equals the low 64 bits of `FILE_ID_128`, and the high 64 bits are zero on both volumes. `FILE_ID_INFO.VolumeSerialNumber` is 64-bit (16 hex digits); the classic 32-bit serial is its low half (the last 8 hex digits).
- **MFT slots are reused aggressively [M].** 2,000 create+delete cycles in one scratch directory used only **31 distinct MFT indexes**. One slot was reused **165 times**, with the sequence number rising 5→169. So a stored ID can alias a *different, later* file once the 16-bit sequence wraps. That is rare, but the chance grows with churn. Any ID hit must be checked with a second attribute (name, size, or hash).
- **Reading it is cheap if done in bulk [M].** `GetFileInformationByHandleEx(FileIdExtdDirectoryInfo)` on a directory handle returns the 128-bit ID, sizes and times for every entry. No per-file open is needed (§8).

### 2.2 Turning an ID back into a path without admin

`OpenFileById(hint, {ExtendedFileIdType, id}, FILE_READ_ATTRIBUTES)` followed by `GetFinalPathNameByHandleW` worked non-elevated on C: and D: for every ID in the BoykoEngine tree (8,407/8,407, 0 errors) [M]. Two traps:

- **Recycle Bin [M].** A file deleted "to the Recycle Bin" keeps its ID, so the ID resolves to `C:\$Recycle.Bin\S-1-5-21-…\$RGYWE9D.txt`. A resolver must treat "resolved, but outside the worktree" as *trashed or moved out*, not as *moved*.
- **Sharing violations [M].** A rename by moirai (for `moirai mv`) fails with error 32 when another process holds the file without `FILE_SHARE_DELETE`; Python's `open()` is one example. With share-delete, the rename succeeds.

### 2.3 NTFS object IDs, tunneling and Distributed Link Tracking

- **Documented [D].** An object ID is "an optional attribute that uniquely identifies a file or directory on a volume … Rename, backup, and restore operations preserve object IDs. However, copy operations do not" ([DLT and Object Identifiers](https://learn.microsoft.com/en-us/windows/win32/fileio/distributed-link-tracking-and-object-identifiers)). `ReplaceFile` is documented to preserve the "Object identifier" and the creation time ([ReplaceFileW](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-replacefilew)). DLT runs only on NTFS: "Links to files on removable media are not maintained", and the service "does not recognize a new NTFS file system volume until the system is re-booted" [D].
- **Creating one needs no elevation [M].** `FSCTL_CREATE_OR_GET_OBJECT_ID` succeeded on a handle opened with only `FILE_READ_ATTRIBUTES` (on my own files). It returned the object ID, a birth volume ID, a birth object ID and a zero domain ID (workgroup machine). It changed **ChangeTime only**, not mtime or creation time. `git status` stayed clean after object IDs were added to tracked files [M]. Creating a `.lnk` with `WScript.Shell` also silently added an object ID to its target [M].
- **Tunneling moves object IDs to the reused *name* [M]. I found no vendor documentation for this.** NTFS tunneling, which preserves creation time and short and long names, is documented for its motivating case: programs that save "by performing a combination of save, delete, and rename operations" ([Old New Thing](https://devblogs.microsoft.com/oldnewthing/20050715-14/?p=34923)); the default 15 s window is set by `MaximumTunnelEntryAgeInSeconds` [C]. I measured that the object ID tunnels as well:
  - delete + recreate under the same name: object ID and creation time kept at delays of 0, 2, 8 and 14 s, **lost at 17 and 25 s** [M];
  - rename `f`→`f.old`, create a new `f`: the **object ID moves from `f.old` to the new `f`**, leaving `f.old` without one (3/3 trials), and the old ID resolves to the new file [M];
  - `MoveFileEx(tmp, f, REPLACE_EXISTING)`: creation time tunneled 4/4, **object ID lost 4/4** [M];
  - `ReplaceFileW` without a backup file: object ID kept in 4 of 6 trials (2 of 4 in one batch, 1 of 1 in each of two others); with a backup file, 2 of 2; creation time kept 4/4 [M]. So "preserves object identifier" is not reliable in practice.
- **Shell links show DLT working [M]** (`lnk.ps1`; the `.lnk` was copied fresh before every resolve, with `SLR_NO_UI`):

| Change to the target after creating the `.lnk` | Tracking only (`SLR_NOSEARCH`) | Search only (`SLR_NOTRACK`) | Default |
|---|---|---|---|
| rename in directory | found (141 ms) | found (33 ms) | found (49 ms) |
| move to sibling directory | found | found | found (58–510 ms) |
| move + rename | found | found | found |
| rename parent directory | found | found | found |
| atomic save at the same path (PS `Move-Item -Force`) | path still valid (3 ms) | same | same |
| copy to other directory + delete original | **not found** (0.8 s) | **not found** (1.0 s) | **not found** |
| move, then atomic save at the new place | found | found | found |

  The documented shell search heuristic looks four directory levels down and up from the last path, the desktop and each fixed drive, using the file's creation date, size, name and extension [D, same page]. That heuristic, not the object ID, is what rescues many of these cases. It is slow and not suitable for moirai.

### 2.4 Creation time

The creation time is tunneled across delete+recreate and across `MoveFileEx(REPLACE_EXISTING)` [M]. Claude Code's `Write` and `Edit` did **not** preserve it: the creation time changed along with the file ID [M]. My guess is that its rename path does not trigger tunneling, for example because it uses POSIX-semantics rename [I]. Creation time is at best a weak tie-breaker, never an identity.

### 2.5 ReFS and Dev Drive

- ReFS supports the USN journal, file IDs and change notifications, but **not object IDs**, transactions or short names ([ReFS overview](https://learn.microsoft.com/en-us/windows-server/storage/refs/refs-overview)) [D]. So on a Dev Drive neither DLT nor object-ID tunneling exists.
- **ReFS 128-bit file ID layout [D]:** "the low 64 bits consists of an index uniquely identifying the file's parent directory … The high 64-bits consists of an index uniquely identifying the file within that directory". The 64-bit ID is set to −1 when the 128-bit value does not map ([MS-FSCC Appendix B](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-fscc/d4bc551b-7aaf-4b4f-ba0e-3a75e7c528f0)). The same spec marks the ReFS ID as "Stable". A forensic study of ReFS 3.4 [C, via search summary of [ScienceDirect S266628172030010X](https://www.sciencedirect.com/science/article/pii/S266628172030010X)] reports that a cross-directory move assigns a new metadata address while ReFS keeps the initial one. **Whether the ID reported to applications changes when a file moves to another directory on ReFS is unmeasured**; I could not create a ReFS volume without admin. Measure this before anyone relies on file IDs on a Dev Drive. Use only `FileIdInfo` (128-bit) there, never `nFileIndex`.
- Dev Drive needs admin to create. By default "Filter Manager will turn OFF all filters on a Dev Drive, with the exception of antivirus filters", and Defender runs in "performance mode" on a trusted Dev Drive ([Dev Drive](https://learn.microsoft.com/en-us/windows/dev-drive/)) [D]. That matters only for timing: moirai runs in user mode and needs no filter.

---

## 3. Survival matrix (measured, same NTFS volume)

"ID kept" means the path the file ends up at has the same `FILE_ID_128` as before. "Old ID →" shows what `OpenFileById(old id)` resolves to afterwards. Object IDs were created before each operation.

### 3.1 OS operations and shells

| Operation | File ID kept | Object ID kept | Old ID → |
|---|---|---|---|
| Rename in place (`MoveFileEx`) | **yes** | yes | new name |
| Move to another directory, same volume | **yes** | yes | new path |
| Rename the parent directory | **yes** | yes | new path |
| Case-only rename | **yes** | yes | new name |
| Hard link, then delete the original name | **yes** | yes | the other link |
| Append; truncate+write in place (`open('w')`) | **yes** | yes | same path |
| Send to Recycle Bin | **yes** | — | `C:\$Recycle.Bin\<SID>\$R…` |
| `Shell.Application.MoveHere` (Explorer engine) | **yes** | yes | new path |
| PowerShell `Set-Content`, `Out-File`, `Add-Content`, `[IO.File]::WriteAllText` | **yes** | yes | same path |
| PowerShell `Move-Item` (rename) | **yes** | yes | new name |
| Git Bash `mv`; `echo new > f` | **yes** | yes | new or same path |
| `rustfmt` | **yes** | yes | same path |
| vim 9.2 (MSYS2 build), `backupcopy` = auto, no or yes | **yes** | yes | same path |
| `CopyFileW` + `DeleteFile` (what a **cross-volume move** does [D: `MoveFileEx` `MOVEFILE_COPY_ALLOWED`]) | no | no (copy has none) | gone |
| `shutil.copy2`+remove; `Copy-Item`+`Remove-Item`; `cp`+`rm` | no | no | gone |
| temp + `MoveFileEx(REPLACE_EXISTING)`; Python `os.replace` | no | **no** | gone |
| temp + `ReplaceFileW` | no | flaky (4/6; 2/2 with a backup file, which keeps the old ID) | gone |
| delete + create same name (≤14 s) | no | **yes** (tunneled) | gone |
| delete + create same name (≥17 s) | no | no | gone |
| rename `f`→`f.old`, create `f`, delete `f.old` (JetBrains "safe write" [C], Emacs-style) | no | yes (8 of 9 trials) | gone |
| PowerShell 5.1 `Move-Item -Force` over an existing file | no | yes (3/3) | gone |
| GNU `sed -i` | no | no | gone |
| `perl -pi -e` | no | yes | gone |
| **Claude Code `Write` tool** | **no** | **no** | gone |
| **Claude Code `Edit` tool** | **no** | **no** | gone |

What the journal records for one Claude Code `Edit` [M, unprivileged journal read]: `CREATE` of a temp file (new ID) in the same directory, then `DATA_EXTEND|CLOSE`; then `DELETE|CLOSE` of the old ID; then `RENAME_OLD` and `RENAME_NEW` of the new ID in the same parent. This matches the `*.tmp.<pid>.<timestamp>` then rename pattern described publicly [C].

### 3.2 git (throwaway repo, Git for Windows 2.54.0)

| git operation | File ID kept | Object ID kept |
|---|---|---|
| `git mv a d/b` | **yes** | yes |
| `checkout`/`switch` of a branch where the file is **unchanged** | **yes** | yes |
| `checkout` of a branch where the file's **content differs** (and back) | no | yes (tunneled) |
| `checkout` of a branch where the file was **moved** (`git mv` committed) | no | **no** |
| `stash`, `stash pop` (modified file) | no | yes |
| `reset --hard` (modified file) | no | yes |
| `reset --hard` (unmodified file) | **yes** | yes |
| `restore <file>` | no | yes |
| `merge --ff-only` bringing a content change | no | yes |
| `merge --ff-only` bringing a move | no | no |
| `rebase` (file changed only on the rebased branch) | no | yes |
| `rebase` (file untouched) | **yes** | yes |
| `worktree add`, `clone`: same path in two copies | different IDs | — |

git rewrites a changed file by unlinking it and creating a new one under the same name. So the **file ID changes on every content change git makes**, and the **object ID survives by tunneling unless the path changes** [M]. A branch switch that moves a file is copy+delete as far as identity goes.

### 3.3 What editors do (documented, not measured here)

- **VS Code** writes editor saves in place: "truncate the file to 0 bytes … write the contents". The request for atomic saves, [microsoft/vscode#98063](https://github.com/microsoft/vscode/issues/98063), is still **open** [C]. Atomic temp+rename writes exist for user data such as settings (issues [#182974](https://github.com/microsoft/vscode/issues/182974), [#195539](https://github.com/microsoft/vscode/issues/195539)) [C]. VS Code is not installed on this machine, so this is unmeasured.
- **JetBrains IDEs**, with "Use safe write" on (the default), write `___jb_tmp___`, rename the original to `___jb_old___`, rename the temp over and delete the old file ([JetBrains support threads](https://intellij-support.jetbrains.com/hc/en-us/community/posts/206864695-Cannot-save-file-Cannot-delete-temporary-file-jb-old-)) [C]. Expected: new file ID, object ID tunneled (my rename-away row) [I].
- **vim**: `'backupcopy'` defaults to "yes" (Vi default for Unix), otherwise "auto", and "no" means "rename the file and write a new one" ([vim help](https://vimhelp.org/options.txt.html)) [D]. The MSYS2 build kept the file ID in every mode [M]. Native gvim and Neovim on Windows were not measured.

---

## 4. The USN change journal

### 4.1 Which calls need administrator rights (measured, non-elevated)

| Handle | QUERY | READ | READ_UNPRIVILEGED | ENUM_USN_DATA | READ_FILE_USN_DATA |
|---|---|---|---|---|---|
| `\\.\C:` opened with access 0 or `FILE_READ_ATTRIBUTES` | err 1 | err 1 | err 1 | err 1 | err 1 |
| `\\.\C:` opened `GENERIC_READ` | open fails, **err 5** | | | | |
| `C:\`, or any directory or file, opened `FILE_READ_ATTRIBUTES` | **ok** | **err 5** | **ok** | **err 5** | **ok** |
| Same handles on **D:** | 1179 (no journal) | 5 | 1179 | 5 | ok (USN = 0) |

- `FSCTL_READ_UNPRIVILEGED_USN_JOURNAL` is `CTL_CODE(FILE_DEVICE_FILE_SYSTEM, 234, METHOD_NEITHER, FILE_ANY_ACCESS)`, declared under `_WIN32_WINNT_WIN10_RS1` (Windows 10 1607) in the Windows SDK 10.0.26100 `winioctl.h`, line 11446 [D]. `windows-sys` 0.61 exports it [D]. It has **no Learn page** (404) [M]. It takes the same `READ_USN_JOURNAL_DATA_V0` or `V1` input [M].
- **Pitfall [M].** With a byte-aligned input buffer it returned `ERROR_INVALID_PARAMETER` (87); with an 8-byte aligned one it worked. `READ_FILE_USN_DATA` with a misaligned buffer returned 1784.
- **What an unprivileged read returns [M].** V3 records with the 128-bit file ID, the parent ID, USN, timestamp, reason and attributes, **with `FileNameLength = 0`**. Records cover every file on the volume, including files in directories the caller cannot open (their parents return 5 from `OpenFileById`). A rename appears as `RENAME_OLD` (old parent) plus `RENAME_NEW` (new parent) under the **same ID**. The new path is obtained with `OpenFileById`, or from the pass's own directory walk.
- **`FSCTL_READ_FILE_USN_DATA` on a single file** gives that file's last USN and its parent ID. On D: it returns USN 0 [M]. It is a cheap "has this file changed since USN u" check, but only where a journal exists.

### 4.2 Availability, size and retention window

- **D: has no journal [M].** Every repository and all 44 worktrees are on D:. Windows does not guarantee a journal on non-system volumes; a third party found the same on GitHub runners' `D:` ([withpointbreak/pointbreak#825](https://github.com/withpointbreak/pointbreak/issues/825)) [C]. Creating one (`fsutil usn createjournal m=<max> a=<delta> D:`) requires an elevated prompt [D/I]. I did not attempt it, because it would change system configuration.
- **The C: journal window is short [M].** With 32 MB maximum and 8 MB allocation delta, two full replays found **325,678 records (26.0 MB) spanning 1.70 h** at 00:38 and **409,689 records (32.8 MB) spanning 1.07 h** at 01:29. That is 53–106 records/s on a loaded machine. After the window, a cursor falls below `FirstUsn` and the replay must fall back to a full scan.
- **Replay cost [M].** A full replay of the C: journal (26–33 MB) through the unprivileged call took **85–341 ms**, at 1.1–3.8 M records/s. Incremental replays of 58–236 records took **1.1 ms**, and 9.2 ms when each record's parent path was also resolved.
- **Estimate for D: [I].** In a 123 s read-only watch, `<lanes-dir>` produced ~34 ReadDirectoryChangesW events/s, ~96 % of a sampled 1,206 of them under one `<lanes-dir>\_targets\…` directory (shared cargo build output) [M]. USN writes several records per logical change (the trace above has 3–4 per file, including `CLOSE`). Assuming 100–150 records/s at ~90 B each (my assumption), a 32 MB journal would last about 0.7–1 h. **512 MB would last roughly 10–16 h and 1 GB roughly 1–1.5 days**, always shorter during large builds.

### 4.3 How to replay lazily without a resident process

1. Store a cursor per volume: `(volume serial, UsnJournalID, NextUsn)`.
2. At the start of a moirai command, open any directory on the volume with `FILE_READ_ATTRIBUTES` and call `FSCTL_QUERY_USN_JOURNAL` (tens of µs [I]). If the journal ID changed, or the cursor is below `FirstUsn`, the history is lost: fall back to the scoped walk (§9).
3. Otherwise call `FSCTL_READ_UNPRIVILEGED_USN_JOURNAL` from the cursor. Keep records whose ID or parent ID is among the stored reference IDs or their parent directory IDs. Look for `RENAME_*`, `DELETE`, `CREATE`, `DATA_*` and `OBJECT_ID`.
4. Recognise the **rename-over signature**: `DELETE|CLOSE` of the old ID, directly followed in USN order by `RENAME_OLD` and `RENAME_NEW` of a new ID in the same parent. It means "X replaced by Y at the same name". Confirm by checking that Y now sits at X's stored path. This is how agent edits and `sed -i` can be chained without names [I, from the measured traces].
5. Save the new cursor in the same commit as any rebinds.

This uses no daemon, no timer and no CPU between commands. The cost is proportional to the volume's activity since the last command, not to the tree size.

---

## 5. ReadDirectoryChangesW (a resident watcher)

- **Pairs [M]**, using `ReadDirectoryChangesExW` with `ReadDirectoryNotifyExtendedInformation`, which gives 64-bit file and parent IDs:
  - rename in a directory → `RENAMED_OLD` a.txt plus `RENAMED_NEW` b.txt, same ID;
  - move to a subdirectory within the watched tree → **`REMOVED` b.txt plus `ADDED` sub\b.txt with the same file ID**. It is not a rename pair: only the ID links them;
  - atomic save → `REMOVED` (old ID) plus `RENAMED_OLD`/`RENAMED_NEW` (new ID from the temp file).
- **Overflow is total loss [M].** In a burst of 2,000 renames (≈4,000 events), a consumer sleeping 1.5 s between reads overflowed on its first read with a 64 KB buffer and also with a 4 KB buffer (`bytes=0`). The event for the `STOP` sentinel file created afterwards was **never delivered**, so the watcher had to be killed. A consumer that never sleeps received all 4,003 events in 457 calls with no overflow. Docs: "If the buffer overflows … the entire contents of the buffer are discarded"; you "should compute the changes by enumerating the directory or subtree" ([ReadDirectoryChangesW](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-readdirectorychangesw)) [D].
- **Needs a live process.** The kernel buffer exists only between the first call and the handle's close [D]. Nothing is recorded while no moirai process runs. The watcher uses no CPU while blocked [I], but pays for every event. A recursive watch **cannot exclude** `target/`, so an in-tree `target/` needs watches on each other top-level subtree plus a non-recursive watch on the root [I].
- **Live rates (123 s, read-only watches) [M]:** `<lanes-dir>` 4,216 events (34/s, mostly cargo output in `_targets`); the workspace directory holding the main checkouts 25 events; the user profile directory 4,072 events (33/s).
- **Prior art.** Git's builtin `fsmonitor--daemon` is "a long running process used to watch a single working directory"; it refuses network-mounted repos by default and uses inotify on Linux ([git-fsmonitor--daemon](https://git-scm.com/docs/git-fsmonitor--daemon)) [D].

---

## 6. Linux and macOS (documented, not measured)

| Signal | Linux | macOS |
|---|---|---|
| Stable ID | `(st_dev, st_ino)`: kept on rename and move within a filesystem; a new inode on temp+rename saves and on copy [D/I]. Inode numbers are reused; file handles carry a generation, so a stale handle gets `ESTALE` even when the inode number is reused ([open_by_handle_at(2)](https://man7.org/linux/man-pages/man2/open_by_handle_at.2.html)) [D] | APFS file ID (inode) kept on rename [I]. Apple warns that "a file's ID may change if the system is rebooted", so file-reference URLs must not be stored; use **bookmarks**, which "can usually be used to re-create a URL to a file even in cases where the file was moved or renamed" ([File System Programming Guide](https://developer.apple.com/library/archive/documentation/FileManagement/Conceptual/FileSystemProgrammingGuide/AccessingFilesandDirectories/AccessingFilesandDirectories.html)) [D] |
| ID → path without privilege | **No.** `open_by_handle_at` requires `CAP_DAC_READ_SEARCH` [D]. A pass must keep its own `inode → path` map from a walk | Bookmark resolution (path + file ID + volume) [D]; `/.vol/<dev>/<ino>` [I] |
| Persistent change history readable by a user | **None** on ext4 or XFS [I] | **FSEvents**: per-volume history with event IDs, replayable with `sinceWhen`. You must rescan on `MustScanSubDirs` or dropped events, and when `FSEventsCopyUUIDForDevice` changes because history was purged. Events are "advisory rather than a definitive list" ([FSEvents guide](https://developer.apple.com/library/archive/documentation/Darwin/Conceptual/FSEvents_ProgGuide/UsingtheFSEventsFramework/UsingtheFSEventsFramework.html)) [D]. This is the macOS counterpart of the unprivileged USN replay |
| Live watcher | **inotify**: "not recursive"; `IN_MOVED_FROM`/`IN_MOVED_TO` share a cookie, but the pair is "not guaranteed … atomically inserted into the queue"; `IN_Q_OVERFLOW` drops events ([inotify(7)](https://man7.org/linux/man-pages/man7/inotify.7.html)) [D]. `max_user_watches` default is 1 % of RAM within [8192, 1048576] since 5.11 ([commit 9289012](https://github.com/torvalds/linux/commit/92890123749bafc317bbfacbe0a62ce08d78efb7)) [D]. **fanotify**: unprivileged since 5.13 (and 5.10.220) with limited functionality, and file handles (`FAN_REPORT_FID`) are required ([fanotify_init(2)](https://man7.org/linux/man-pages/man2/fanotify_init.2.html)). `FAN_MARK_MOUNT` and `FAN_MARK_FILESYSTEM` need `CAP_SYS_ADMIN`; `FAN_RENAME` since 5.17 (5.15.154, 5.10.220) ([fanotify_mark(2)](https://man7.org/linux/man-pages/man2/fanotify_mark.2.html)) [D] | FSEvents streams with `kFSEventStreamCreateFlagFileEvents` for file-level events [D]. The daemon (`fseventsd`) belongs to the OS, not to moirai |

On Linux the lazy pass is the "no journal" variant of §9: a scoped stat walk that builds an `inode → path` map. On macOS the FSEvents history can play the role of the USN cursor.

---

## 7. What moirai's own file commands can and cannot guarantee (OS view)

- `moirai mv a b` on the same volume = `MoveFileExW` without copy: the file ID and object ID are kept, and moirai updates its references in the same commit, so it is exact [M for the ID]. A move across volumes is copy+delete: a new ID, and moirai must record `old path → new path` itself [D/M].
- `moirai rm`: a hard delete, or a move to the Recycle Bin (the file keeps its ID in `$Recycle.Bin`) [M]. Either way moirai knows the intent and can tombstone the reference.
- **Limits.** These commands cover only moves moirai performs. Agents use Bash `mv`, `git mv`, `Write`/`Edit` and git branch operations; the owner uses Explorer and editors. None of those go through moirai (§3), so the commands cannot replace a reconcile pass; they can only reduce how often one is needed. On Windows, a moirai rename fails with error 32 while an editor holds the file without share-delete [M], so the command needs retry and fallback paths.

---

## 8. Cost of reading identities (measured)

All runs are non-elevated, with Defender on, on the loaded machine; ranges cover 2–3 repeats. "Files" excludes `target/` and `.git/` at any depth, except where noted.

| Tree | Files / dirs | `FindFirstFileExW` (no IDs) | `FileIdExtdDirectoryInfo` (128-bit IDs, one handle per dir) |
|---|---|---|---|
| BoykoEngine, whole (incl. `.claude/worktrees`, 4,804 files) | 8,407 / 946 | 180–323 ms | **126–187 ms (15–22 µs/file)** |
| BoykoEngine main checkout (also excluding `worktrees`) | 3,603 / 370 | 170 ms | **42–51 ms (12–14 µs/file)** |
| All 44 worktrees `<lanes-dir>` (also excluding `_targets`) | 208,498 / 40,399 | — | **122.7 s cold, 12.0 s warm** (589 / 58 µs/file) |
| `<lanes-dir>` including the shared `_targets` build dir | 351,943–353,481 / ~67,000 | 311 s cold, 149 s second run | 160.6 s cold, 33.7 s second run |

| Per-file operation, BoykoEngine (8,407 files) | Total | Per file |
|---|---|---|
| `CreateFileW(FILE_READ_ATTRIBUTES)` + `FileIdInfo` + close | 480–3,141 ms | 57–374 µs (main checkout: 53–57 µs) |
| … + `FSCTL_READ_FILE_USN_DATA` | 788–1,041 ms | 94–124 µs |
| `std::fs::metadata` (Rust std) | 555–1,166 ms | 66–139 µs |
| `OpenFileById` (128-bit), no path | 864–1,597 ms | 103–190 µs |
| `OpenFileById` + `GetFinalPathNameByHandleW` | 2,545–4,681 ms | 303–557 µs (64-bit ID variant: 326–498 µs) |

Scratch benchmark (`oidbench`, 500 files on C:, three rounds, per file): `GetFileAttributesExW(path)` 101–119 µs; open by path + `FileIdInfo` 88–150 µs; `OpenFileById(file id)` 148–544 µs; the same plus the path 507–572 µs; **`OpenFileById(object id)` 223–337 µs**; creating an object ID (open + FSCTL) 433 µs. A 2,000-file run under heavier load was 1.5–4× slower on every line. Creating a small file with Defender on cost **2.4–10.8 ms** [M].

What this means [I]:
- **Enumerate directories; do not stat files.** Directory enumeration gives IDs for a whole tree at 12–22 µs per file warm. Per-file handles cost 3–20× more, and ID-to-path resolution 15–40× more.
- **Cold and memory-starved walks are ~10× slower** (589 vs 58 µs/file on `<lanes-dir>`) with only 1.7 GB free. A pass across all worktrees is minutes cold, so a pass must cover the current worktree only.
- `OpenFileById` + path is the expensive step, so use it only for IDs the scoped walk did not find (moved out of scope).

---

## 9. Conclusions

### 9.1 How reliable each signal is for auto-rebinding

| Signal | Survives | Breaks on | Admin? | Cost | Use in moirai |
|---|---|---|---|---|---|
| **Path** (worktree-relative, case-insensitive) | edits of any kind, git rewrites at the same path | every rename or move | no | one directory listing | **primary key** for "is it still there" |
| **NTFS `FILE_ID_128` + volume serial** | rename, move within the volume, directory renames, case changes, hard links, in-place writes, `git mv`, Recycle Bin (as a location) | **every replace-by-new-file**: Claude Code Write/Edit, `sed -i`, `os.replace`, JetBrains safe write, git checkout/stash/reset/restore/merge/rebase of changed files, copy+delete, cross-volume and cross-worktree; MFT slot reuse → rare aliasing | no | 12–22 µs/file (enumeration); 103–557 µs by ID | **move detector** between two passes; verify every hit |
| **NTFS object ID** (+ 15 s tunneling) | the above, plus delete+recreate under the same name (git rewrites, `perl -i`, PS `Move-Item -Force`), rename-away+create (safe-write editors) | rename-over (`MoveFileEx(REPLACE_EXISTING)`: Claude Code, `sed -i`, `os.replace`), `ReplaceFileW` (flaky), copies, git moves across branches, >15 s gaps, ReFS/Dev Drive (none) | no, but **writes metadata** to user files | 223–337 µs by ID; 433 µs to create | optional: helps with git and IDE saves, **not** with agent edits |
| **Creation time** | tunneled rewrites | Claude Code tools; >15 s | no | free with enumeration | tie-breaker only |
| **USN journal** (unprivileged read) | nothing is lost within the window, and every event is observed | journal missing (**D: today**), window exceeded (1.1–1.7 h at 32 MB on C:), journal re-created, no names | read: no; **creating on D:: yes** | 1–9 ms incremental, 85–341 ms full | exact "what changed since cursor" **if** the owner enables it on D: |
| **ReadDirectoryChangesW** | everything while the process runs | overflow drops the whole buffer, no process → no history, cannot exclude subtrees | no | per event | only inside an already-running MCP server, as a dirty-hint |

### 9.2 Patterns where OS signals fail completely

For these, OS evidence alone cannot rebind, and the content and git lenses must decide:
1. **An edit and a move between two passes.** For example an agent `Edit` then `mv`, or `mv` then `Edit`: the old path and the old ID are both gone. The USN journal can chain it (§4.3); without a journal, only the content hash can.
2. **A branch switch or merge that moves a file** (`checkout`, `merge`, `rebase` over a `git mv`): new ID, no tunneling.
3. **Copy+delete**, including moves across volumes and anything that goes through another worktree or clone.
4. **Worktrees.** The same logical file has a different ID in each of the ~44 worktrees [M], so an OS ID never identifies a file across lanes.
5. **Delete to the Recycle Bin** looks like a move unless the resolver checks that the new path is still inside the worktree.

### 9.3 A rebinding pass on Windows with no daemon and no admin

Stored per file reference (the other lenses decide the reference model; these are the OS fields): worktree-relative path; `(volume serial 64, FILE_ID_128)`; parent directory ID; size; mtime; creation time; content hash computed lazily. Stored per volume: the USN cursor, if a journal exists.

Triggers: the first moirai command in a session, any command that reads or writes file references, an explicit `moirai files sync`, and optionally a Claude Code `PostToolUse` hook after Bash, Write or Edit. A hook is a subprocess, so it adds no idle CPU. No timer and no resident process are needed.

1. **Cheap gate.** If the volume has a journal, replay it from the cursor (§4.3). No relevant IDs → done in ~1–10 ms. Journal present but window lost → go to 2 for the whole worktree. No journal (D: today) → go to 2, limited to the directories that hold referenced files. When the pass does run a full worktree walk, it can mark the directories that moved or disappeared.
2. **Scoped enumeration.** List each needed directory once with `FileIdExtdDirectoryInfo` to get `name → (id, size, mtime)` at ~0.1–0.2 ms per directory warm. A full walk of one worktree costs 43–190 ms warm [M].
3. **Path hit, same ID** → unchanged, or edited in place. Refresh the hash only when size or mtime differ.
4. **Path hit, different ID** → **replaced in place**: an agent edit, a git rewrite or a safe save. Rebind the ID and keep the reference; confirm by hash when the file type needs it.
5. **Path miss, ID found in the walk map** → **moved inside the worktree**. Rebind the path and verify the name, size or hash (MFT slot reuse).
6. **Path miss, ID not in the map** → `OpenFileById`. Inside the worktree: moved to a place the pass did not scan. Under `$Recycle.Bin` or outside the worktree: **trashed or moved out**. Not found: go to 7.
7. **Path miss, ID gone** → hand over to the content and git lenses. Candidates are files whose IDs are new since the last pass (from the walk or the journal's `CREATE`/`RENAME_NEW` records); match by exact content hash first, then by similarity. Otherwise mark the reference **dangling, with the last known path**. Never delete the edge silently.
8. Commit all rebinds and the new cursor in one moirai commit, so rebinds are versioned like everything else (R1/R2).

Expected coverage [I, from §3]: steps 3–5 cover all pure renames and moves (user, Explorer, agent `mv`, `git mv`) and all in-place replaces (agent edits, git content rewrites). The residue is limited to the §9.2 patterns.

### 9.4 What this means for moirai's design (R4)

- **An OS file ID is never a node's identity.** It is a per-worktree, per-volume cache of "where did this go". The durable reference is worktree-relative path + content, which other lenses define. Store the OS ID beside it and verify every OS hit.
- **Plan for agent edits to change the file ID every time.** In the owner's workflow the primary writer is Claude Code, so a design that ties links to file IDs would break on the first edit.
- **Object IDs are not worth it by default.** Creating them writes to user files, they do not help with agent edits, and they do not exist on ReFS or Dev Drive. They are worth it only if IDE safe-saves and git branch switches that keep the path turn out to matter; the path-first rule already covers those cases.
- **The USN journal is the only OS feature that makes a daemon-less pass exact and cheap.** A normal user can read it, but D: has none today. Creating it on D: is a one-time admin action and a size choice (§4.2).
- **ReadDirectoryChangesW fits only a process that already runs,** such as the session's MCP server, and only as a hint that marks directories dirty. It conflicts with the zero-idle-CPU and quiet-benchmark goals whenever builds run in the watched tree.
- **Scope every pass to one worktree**: ~50–190 ms warm; 12–123 s across all 44 worktrees.
- **Use `moirai mv/rm` to make moirai-initiated changes exact, not as the only mechanism.** On Windows they need handling for sharing violations (error 32).

### 9.5 Decisions for the owner

1. Allow a one-time elevated `fsutil usn createjournal` on D:, and at what size (32 MB ≈ 1 h; 0.5–1 GB ≈ 10 h to 1.5 days, estimated), or require moirai to stay strictly admin-free with walk-based passes?
2. Is a file reference bound to one worktree's copy, or to the logical repository path? The OS gives each worktree different IDs.
3. May moirai write metadata to project files (NTFS object IDs: ChangeTime bump, invisible to git), or must it stay read-only on the user's tree?
4. How fresh must links be: re-checked on every moirai command (1–190 ms per command), at session start and after tool hooks, or only on demand?
5. May the long-lived MCP server hold a directory watcher during sessions (idle CPU zero, but CPU per event during builds), given the quiet-machine requirement for benchmarks?
6. Is a Dev Drive (ReFS) planned for the repos? It has no object IDs, and the behaviour of file IDs on cross-directory moves is unmeasured.

---

## 10. Sources

Vendor documentation and specifications:
- BY_HANDLE_FILE_INFORMATION: https://learn.microsoft.com/en-us/windows/win32/api/fileapi/ns-fileapi-by_handle_file_information
- FILE_ID_INFO: https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-file_id_info
- MS-FSCC 128-bit file ID: https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-fscc/98860416-1caf-4c80-a9ab-8d61e1ccf5a5 ; Appendix B product behavior (notes 10, 11, 14, 90–94): https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-fscc/d4bc551b-7aaf-4b4f-ba0e-3a75e7c528f0
- Distributed Link Tracking and Object Identifiers: https://learn.microsoft.com/en-us/windows/win32/fileio/distributed-link-tracking-and-object-identifiers
- ReplaceFileW: https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-replacefilew
- FSCTL_QUERY_USN_JOURNAL: https://learn.microsoft.com/en-us/windows/win32/api/winioctl/ni-winioctl-fsctl_query_usn_journal
- FSCTL_READ_USN_JOURNAL: https://learn.microsoft.com/en-us/windows/win32/api/winioctl/ni-winioctl-fsctl_read_usn_journal
- FSCTL_READ_FILE_USN_DATA: https://learn.microsoft.com/en-us/windows/win32/api/winioctl/ni-winioctl-fsctl_read_file_usn_data
- Change journal operations: https://learn.microsoft.com/en-us/windows/win32/fileio/change-journal-operations
- Windows SDK 10.0.26100 `um/winioctl.h`, line 11446 (`FSCTL_READ_UNPRIVILEGED_USN_JOURNAL`, `_WIN32_WINNT_WIN10_RS1`), local file `C:\Program Files (x86)\Windows Kits\10\Include\10.0.26100.0\um\winioctl.h`
- ReadDirectoryChangesW: https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-readdirectorychangesw
- ReFS overview (feature tables): https://learn.microsoft.com/en-us/windows-server/storage/refs/refs-overview
- Dev Drive: https://learn.microsoft.com/en-us/windows/dev-drive/
- Tunneling background (Raymond Chen): https://devblogs.microsoft.com/oldnewthing/20050715-14/?p=34923
- git fsmonitor--daemon: https://git-scm.com/docs/git-fsmonitor--daemon
- vim 'backupcopy': https://vimhelp.org/options.txt.html
- inotify(7): https://man7.org/linux/man-pages/man7/inotify.7.html ; fanotify(7): https://man7.org/linux/man-pages/man7/fanotify.7.html ; fanotify_init(2): https://man7.org/linux/man-pages/man2/fanotify_init.2.html ; fanotify_mark(2): https://man7.org/linux/man-pages/man2/fanotify_mark.2.html ; open_by_handle_at(2): https://man7.org/linux/man-pages/man2/open_by_handle_at.2.html
- Linux inotify watch default commit: https://github.com/torvalds/linux/commit/92890123749bafc317bbfacbe0a62ce08d78efb7
- Apple FSEvents guide: https://developer.apple.com/library/archive/documentation/Darwin/Conceptual/FSEvents_ProgGuide/UsingtheFSEventsFramework/UsingtheFSEventsFramework.html
- Apple File System Programming Guide (bookmarks): https://developer.apple.com/library/archive/documentation/FileManagement/Conceptual/FileSystemProgrammingGuide/AccessingFilesandDirectories/AccessingFilesandDirectories.html

Third-party claims [C]:
- Claude Code Edit/Write replace via rename (new inode): https://github.com/anthropics/claude-code/issues/92419
- NTFS volumes without a USN journal (GitHub runner D:): https://github.com/withpointbreak/pointbreak/issues/825
- VS Code save behaviour: https://github.com/microsoft/vscode/issues/98063 , https://github.com/microsoft/vscode/issues/182974 , https://github.com/microsoft/vscode/issues/195539
- JetBrains safe write: https://intellij-support.jetbrains.com/hc/en-us/community/posts/206864695-Cannot-save-file-Cannot-delete-temporary-file-jb-old- ; https://www.jetbrains.com/help/rider/Saving_and_Reverting_Changes.html
- Tunneling 15 s / `MaximumTunnelEntryAgeInSeconds` (KB172190 archive and forensics write-ups, via search results): https://www.betaarchive.com/wiki/index.php/Microsoft_KB_Archive/172190
- ReFS metadata address on move (via search summary only; the page returned 403): https://www.sciencedirect.com/science/article/pii/S266628172030010X

## 11. Reproducing

The probe scripts named in §1 are not published. The Rust probe builds with `cargo build --release --offline` (no dependencies). Examples: `fidprobe usntest C: <dir>`; `fidprobe usnread C: <dir> attr unpriv first ''`; `fidprobe walk <repo> find,open-attr,usn,resolve,resolve64,resolve-nopath,stdmeta` (set `FID_EXCLUDE=target,.git[,worktrees|_targets]`); `fidprobe watch <dir> 65536 0` (set `FID_VERBOSE=1` to print events); `fidprobe oidbench <scratchdir> 500 3`; `fidprobe seqreuse <scratchdir> 2000`. After the runs I deleted the scratch working directories (`work*`) and logs; only the probe sources and the small binary remain (≈2 MB).
