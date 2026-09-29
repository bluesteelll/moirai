//! `os::project` on Windows: the complete `ProjectFs` ([`OsProjectFs`], read and write side), file identity, volume
//! capabilities, timestamps and attributes, cloud placeholders ([OS/project]; X-F8, R-14, R-18).
//!
//! - **Roots hold no handle** ([OS/project §2.2]): a root is its canonical text, its `\\?\` form and its root id; every
//!   operation opens what it needs with full sharing and closes it before returning, so no project handle blocks a
//!   rename of an ancestor ([40 §4.7]) and no handle lives between operations (I-F11).
//! - **Identity** is `FILE_ID_128` with the volume key `BLAKE3-128(lp("moirai-vol-key-v1") ‖ lp("win-volume-serial-64")
//!   ‖ lp(u64le(VolumeSerialNumber)))` ([OS/project §3.1, §4.1]); `nFileIndex` is never used.
//! - **Cloud placeholders are never hydrated** ([OS/project §5.10]): attributes come from `GetFileAttributesExW` or the
//!   parent's enumeration; a `RECALL_ON_OPEN` or `OFFLINE` entry is never opened, a `RECALL_ON_DATA_ACCESS` directory is
//!   never enumerated, and content is read only with `allow_hydrate`.
//! - **Denials are never absence** ([OS/project §5.11]): a permission failure is always `AccessDenied`.
//! - **The Windows name check** ([OS/project §2.3], pass 1, P1-15): every method tests every segment of its paths with
//!   `representable_here` ([OS/path §8.1]) before any OS call and returns `InvalidName` (code 123) for a failing one; no
//!   counter moves. `\\?\` paths bypass Win32 name normalisation, so `x::$DATA`, `a:b` or `y.` would otherwise reach a
//!   stream or a literal name.
//! - **Network redirectors** (a `\\?\UNC\` root, a `DRIVE_REMOTE` volume) are the "any other" row of [OS/project §4.3]
//!   whatever file-system name they report: no ids, no by-id lookup, no journal.
//! - **The write side** shares [OS/fs]'s calls: `MoveFileExW(…, MOVEFILE_WRITE_THROUGH)` with the bounded retry on
//!   errors 5 and 32, `FlushFileBuffers` on a directory handle opened with write access, clear-then-delete of a read-only
//!   file (restored if the delete fails) ([OS/project §6]).

#![allow(unsafe_code)]

use std::ffi::OsStr;
use std::ops::ControlFlow;
use std::os::windows::io::OwnedHandle;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};

use moirai_vfs::{
    AbsPath, At, BtimeTrust, CanonicalRoot, CaseRule, CloudRule, DirEquivalence, DurabilityClass,
    DurabilityFailure, EntryNameRef, EnumEnd, FileAttrs, FileIdKind, FsTime, Holder, IdLocate,
    JournalKind, Located, OsCode, OsFileId, OsTag, PathError, PfsCounters, ProjEntry, ProjKind,
    ProjectFs, ProjectRead, ReadOpts, ReadSnapshot, RelPath, RelPathBuf, RenameFailure, RenameRule,
    Renamed, ShareRetry, Stat, StatMode, StatRec, VfsError, VfsErrorKind, VolumeCaps, VolumeKey,
};
use windows_sys::Win32::Foundation::{
    ERROR_HANDLE_EOF, ERROR_MORE_DATA, FILETIME, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE};
use windows_sys::Win32::Globalization::CompareStringOrdinal;
use windows_sys::Win32::Storage::FileSystem::{
    DeleteFileW, ExtendedFileIdType, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_HIDDEN,
    FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_OFFLINE, FILE_ATTRIBUTE_PINNED, FILE_ATTRIBUTE_READONLY,
    FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS, FILE_ATTRIBUTE_RECALL_ON_OPEN,
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_TAG_INFO, FILE_ATTRIBUTE_UNPINNED,
    FILE_BASIC_INFO, FILE_BEGIN, FILE_CASE_SENSITIVE_INFO, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_FLAG_OPEN_REPARSE_POINT, FILE_FLAG_SEQUENTIAL_SCAN, FILE_FULL_DIR_INFO, FILE_ID_128,
    FILE_ID_DESCRIPTOR, FILE_ID_DESCRIPTOR_0, FILE_ID_EXTD_DIR_INFO, FILE_LIST_DIRECTORY,
    FILE_READ_ATTRIBUTES, FileAttributeTagInfo, FileBasicInfo, FileCaseSensitiveInfo,
    FileFullDirectoryInfo, FileFullDirectoryRestartInfo, FileIdExtdDirectoryInfo,
    FileIdExtdDirectoryRestartInfo, FindClose, FindExInfoBasic, FindExSearchNameMatch,
    FindFirstFileExW, GetFileAttributesExW, GetFileAttributesW, GetFileExInfoStandard,
    INVALID_FILE_ATTRIBUTES, OPEN_ALWAYS, OPEN_EXISTING, OpenFileById, ReadFile, RemoveDirectoryW,
    SYNCHRONIZE, SetFileAttributesW, SetFilePointerEx, WIN32_FILE_ATTRIBUTE_DATA, WIN32_FIND_DATAW,
};
use windows_sys::Win32::System::IO::DeviceIoControl;
use windows_sys::Win32::System::Ioctl::{FSCTL_GET_REPARSE_POINT, FSCTL_QUERY_USN_JOURNAL};
use windows_sys::Win32::System::RestartManager::{
    CCH_RM_SESSION_KEY, RM_PROCESS_INFO, RmEndSession, RmGetList, RmRegisterResources,
    RmStartSession,
};
use windows_sys::Win32::System::SystemServices::{
    FILE_CS_FLAG_CASE_SENSITIVE_DIR, FILE_SUPPORTS_USN_JOURNAL, IO_REPARSE_TAG_SYMLINK,
};
use windows_sys::Win32::System::WindowsProgramming::DRIVE_REMOTE;

use super::env::{drive_type, is_cloud_tag, is_unc, volume_name_and_flags};
use super::fs::{
    DirBuf, flush, is_dot_name, move_file, parent_z, record_name, records, u32_at, u64_at,
    units_at, with_share_retry,
};
use super::path::{rewrite_final, verbatim_of_abs};
use super::proc::{lp, mono_ns};
use super::sys::{
    self, Domain, NameCheck, SHARE_ALL, create_file, error, id_info, join_z, last_error,
    open_attrs, raw, rel_wide, segment_ok, standard_info, write_once,
};

// ---------------------------------------------------------------------------------------------------------------------
// Volumes, ids and times ([OS/project §3, §4])

/// The file-system families that decide id kinds, granularities and capabilities on Windows ([OS/project §4.3]).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum FsFamily {
    Ntfs,
    Refs,
    Fat32,
    ExFat,
    Other,
}

/// The family of the volume holding the open handle `h`, whose `\\?\` path (no NUL) is `path` ([OS/project §4.3]). A
/// network redirector is the "any other or a network redirector" row whatever file-system name it reports (an SMB share
/// usually says `NTFS`): no ids, no by-id lookup, no journal, the volume case rule. It is recognised before the volume
/// query, which is skipped for it.
pub(crate) fn volume_family(h: HANDLE, path: &[u16]) -> Result<FsFamily, VfsError> {
    if is_remote(path)? {
        return Ok(FsFamily::Other);
    }
    let (name, _) = volume_name_and_flags(h)?;
    Ok(family_of(&name, false))
}

/// `true` if the `\\?\` path `path` (no NUL) lies on a network redirector: a `\\?\UNC\` path (a share, `\\wsl$`, a
/// mapped drive's final path), or a volume whose `GetDriveTypeW` is `DRIVE_REMOTE`.
fn is_remote(path: &[u16]) -> Result<bool, VfsError> {
    if is_unc(path) {
        return Ok(true);
    }
    Ok(drive_type(&sys::with_nul(path))? == DRIVE_REMOTE)
}

/// The family from a file-system name and the network flag ([OS/project §4.3]).
fn family_of(name: &str, remote: bool) -> FsFamily {
    if remote {
        return FsFamily::Other;
    }
    match name {
        "NTFS" => FsFamily::Ntfs,
        "ReFS" => FsFamily::Refs,
        "FAT" | "FAT32" => FsFamily::Fat32,
        "exFAT" => FsFamily::ExFat,
        _ => FsFamily::Other,
    }
}

/// The capability record of a family ([OS/project §4.3]); `journal` is the USN journal's availability, which counts
/// only on NTFS and ReFS. The "any other or a network redirector" row carries `dir_flush_doubtful` (pass 1, P1-16): set
/// from the class, never by a probe.
fn caps_for(fs: FsFamily, journal: JournalKind) -> VolumeCaps {
    let ntfs_like = matches!(fs, FsFamily::Ntfs | FsFamily::Refs);
    VolumeCaps {
        id_kind: id_kind(fs) as u8,
        id_locate: if ntfs_like {
            IdLocate::ById
        } else {
            IdLocate::None
        },
        journal: if ntfs_like {
            journal
        } else {
            JournalKind::None
        },
        // NTFS: HOLE(F20-btime-ntfs), draft `TunneledNotCopied`; ReFS absent until verified ([OS/project §4.3]).
        btime: if fs == FsFamily::Ntfs {
            BtimeTrust::TunneledNotCopied
        } else {
            BtimeTrust::Absent
        },
        // NTFS: HOLE(F20-ctime-rename), draft true (S-16).
        ctime_on_rename: if fs == FsFamily::Ntfs {
            Some(true)
        } else {
            None
        },
        case_rule: if ntfs_like {
            CaseRule::PerDirFlag
        } else {
            CaseRule::Volume
        },
        case_insensitive_default: true,
        norm_insensitive_always: false,
        norm_follows_case: false,
        cloud: if ntfs_like {
            CloudRule::RecallAttrs
        } else {
            CloudRule::None
        },
        rename_noreplace: RenameRule::Native,
        clone_indicators: false,
        ids_persistent: ntfs_like,
        docids: false,
        mtime_granularity_ns: 0,
        dir_flush_doubtful: fs == FsFamily::Other,
    }
}

/// The volume key of a volume serial ([OS/project §4.1]).
pub(crate) fn vol_key(serial: u64) -> VolumeKey {
    let mut h = blake3::Hasher::new();
    lp(&mut h, b"moirai-vol-key-v1");
    lp(&mut h, b"win-volume-serial-64");
    lp(&mut h, &serial.to_le_bytes());
    let mut k = [0u8; 16];
    k.copy_from_slice(&h.finalize().as_bytes()[..16]);
    VolumeKey(k)
}

fn id_kind(fs: FsFamily) -> FileIdKind {
    match fs {
        FsFamily::Ntfs => FileIdKind::Ntfs128,
        FsFamily::Refs => FileIdKind::Refs128,
        FsFamily::Fat32 | FsFamily::ExFat | FsFamily::Other => FileIdKind::None,
    }
}

/// The `OsFileId` of an open object ([OS/project §3.1]); kind `none` on a volume without trusted ids.
pub(crate) fn file_id_of(h: HANDLE, fs: FsFamily, parent: [u8; 16]) -> Result<OsFileId, VfsError> {
    let kind = id_kind(fs);
    if kind == FileIdKind::None {
        return Ok(OsFileId::NONE);
    }
    let info = id_info(h).map_err(|e| error(e, Domain::Project, "GetFileInformationByHandleEx"))?;
    Ok(OsFileId {
        kind,
        vol_key: vol_key(info.VolumeSerialNumber),
        id: info.FileId.Identifier,
        parent,
        docid: 0,
    })
}

/// The nominal granularities `(mtime/ctime, btime)` and whether `ChangeTime` exists, per family ([OS/project §3.3]).
fn grans(fs: FsFamily) -> (u8, u8, bool) {
    match fs {
        FsFamily::Ntfs | FsFamily::Refs | FsFamily::Other => (2, 2, true),
        FsFamily::Fat32 => (10, 7, false),
        FsFamily::ExFat => (7, 7, false),
    }
}

fn ft_u64(ft: &FILETIME) -> u64 {
    (u64::from(ft.dwHighDateTime) << 32) | u64::from(ft.dwLowDateTime)
}

/// Normalised attributes ([OS/project §3.4]).
fn norm_attrs(attrs: u32, tag: u32) -> FileAttrs {
    let map = [
        (FILE_ATTRIBUTE_READONLY, FileAttrs::READONLY),
        (FILE_ATTRIBUTE_HIDDEN, FileAttrs::HIDDEN),
        (FILE_ATTRIBUTE_REPARSE_POINT, FileAttrs::REPARSE_POINT),
        (FILE_ATTRIBUTE_RECALL_ON_OPEN, FileAttrs::RECALL_ON_OPEN),
        (
            FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS,
            FileAttrs::RECALL_ON_DATA_ACCESS,
        ),
        (FILE_ATTRIBUTE_OFFLINE, FileAttrs::OFFLINE),
        (FILE_ATTRIBUTE_PINNED, FileAttrs::PINNED),
        (FILE_ATTRIBUTE_UNPINNED, FileAttrs::UNPINNED),
    ];
    let mut out = FileAttrs::NONE;
    for (bit, a) in map {
        if attrs & bit != 0 {
            out = out | a;
        }
    }
    if attrs & FILE_ATTRIBUTE_REPARSE_POINT != 0 && is_cloud_tag(tag) {
        out = out | FileAttrs::CLOUD_REPARSE;
    }
    out
}

/// The kind of an entry from its attributes and reparse tag ([OS/project §5.1]): a symbolic link is
/// `IO_REPARSE_TAG_SYMLINK`; a cloud reparse point keeps its file or directory kind; every other reparse point
/// (junctions, WSL links, app-execution aliases) is `Other`.
fn kind_of(attrs: u32, tag: u32) -> ProjKind {
    let dir = attrs & FILE_ATTRIBUTE_DIRECTORY != 0;
    if attrs & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        if tag == IO_REPARSE_TAG_SYMLINK {
            return ProjKind::Symlink;
        }
        if !is_cloud_tag(tag) {
            return ProjKind::Other;
        }
    }
    if dir { ProjKind::Dir } else { ProjKind::File }
}

// ---------------------------------------------------------------------------------------------------------------------
// Counters ([OS/project §2.4])

struct Counters {
    renames: AtomicU64,
    dir_syncs: AtomicU64,
    unlinks: AtomicU64,
    stats: AtomicU64,
    dir_reads: AtomicU64,
    id_lookups: AtomicU64,
    content_opens: AtomicU64,
    bytes_read: AtomicU64,
}

static COUNTERS: Counters = Counters {
    renames: AtomicU64::new(0),
    dir_syncs: AtomicU64::new(0),
    unlinks: AtomicU64::new(0),
    stats: AtomicU64::new(0),
    dir_reads: AtomicU64::new(0),
    id_lookups: AtomicU64::new(0),
    content_opens: AtomicU64::new(0),
    bytes_read: AtomicU64::new(0),
};

fn bump(c: &AtomicU64, n: u64) {
    c.fetch_add(n, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------------------------------------------------
// Types

/// The project-file seam for the running Windows process ([OS/README §2.2]). It caches each volume's capability record
/// by volume key for its own life, which is one command ([OS/project §4.1]); construct one per command.
#[derive(Debug, Default)]
pub struct OsProjectFs {
    caps: Mutex<Vec<(VolumeKey, VolumeCaps)>>,
}

impl OsProjectFs {
    /// A seam value with an empty capability cache.
    pub fn new() -> OsProjectFs {
        OsProjectFs::default()
    }
}

/// An opened project root ([OS/project §2.2]): text and root id, no OS handle.
#[derive(Clone, Debug)]
pub struct OsProjectRoot {
    text: AbsPath,
    path: Box<[u16]>,
    root_id: OsFileId,
    fs: FsFamily,
    vol_key: VolumeKey,
}

impl OsProjectRoot {
    /// The canonical text of the root.
    pub fn text(&self) -> &AbsPath {
        &self.text
    }

    /// The root directory's identity as it was canonicalised.
    pub fn root_id(&self) -> OsFileId {
        self.root_id
    }
}

/// One project file open for a streaming read ([OS/project §5.5]); dropping it closes the handle.
#[derive(Debug)]
pub struct OsReader {
    handle: OwnedHandle,
    fs: FsFamily,
}

/// The `\\?\` path of `at`, NUL-terminated, after the Windows name check of [OS/project §2.3] (pass 1, P1-15): every
/// segment must pass [OS/path §8.1]'s `representable_here`, else `InvalidName` (Win32 code 123) before any OS call.
/// Every method builds its paths here (parents are cut from the checked path), so `x::$DATA`, `a:b`, `y.`, `y `, `CON`,
/// `a*b` and `q?` never reach the OS, whose `\\?\` form would read them as a stream, a wildcard or a literal name.
fn path_of(at: At<'_, OsProjectRoot>, call: &'static str) -> Result<Vec<u16>, VfsError> {
    Ok(join_z(
        &at.root.path,
        &rel_wide(at.path, NameCheck::Project, call)?,
    ))
}

/// `GetFileAttributesExW(GetFileExInfoStandard)`: attributes, times and size without opening the entry.
fn attr_data(p: &[u16]) -> Result<WIN32_FILE_ATTRIBUTE_DATA, u32> {
    let mut d = WIN32_FILE_ATTRIBUTE_DATA::default();
    // SAFETY: `p` is NUL-terminated; `d` is a live `WIN32_FILE_ATTRIBUTE_DATA`, the buffer of this level.
    let ok = unsafe {
        GetFileAttributesExW(
            p.as_ptr(),
            GetFileExInfoStandard,
            (&mut d as *mut WIN32_FILE_ATTRIBUTE_DATA).cast(),
        )
    };
    if ok != 0 { Ok(d) } else { Err(last_error()) }
}

/// `GetFileAttributesW`.
fn attrs_of(p: &[u16]) -> Result<u32, u32> {
    // SAFETY: `p` is NUL-terminated.
    let a = unsafe { GetFileAttributesW(p.as_ptr()) };
    if a == INVALID_FILE_ATTRIBUTES {
        Err(last_error())
    } else {
        Ok(a)
    }
}

/// The reparse tag of the entry `p` names, from its parent's directory entry (`FindFirstFileExW`); 0 if unknown.
fn reparse_tag(p: &[u16]) -> u32 {
    let mut data = WIN32_FIND_DATAW::default();
    // SAFETY: `p` is NUL-terminated; `data` is a live `WIN32_FIND_DATAW`; no filter, no flags.
    let h = unsafe {
        FindFirstFileExW(
            p.as_ptr(),
            FindExInfoBasic,
            (&mut data as *mut WIN32_FIND_DATAW).cast(),
            FindExSearchNameMatch,
            core::ptr::null(),
            0,
        )
    };
    if h == INVALID_HANDLE_VALUE {
        return 0;
    }
    // SAFETY: `h` is the search handle just returned, closed once.
    unsafe { FindClose(h) };
    data.dwReserved0
}

/// The placeholder gate of `read_for_hash` re-checked through the open handle `h`, before the first read
/// ([OS/project §5.5] step 3, pass 1, P1-38; mapping appendix §2.1): `GetFileInformationByHandleEx(FileAttributeTagInfo)`;
/// an entry that became cloud-only between the attribute read and the open (a file replaced by a placeholder) is
/// `CloudOnly` unless `opts.allow_hydrate`. Opening a placeholder hydrates nothing; only a data read would.
fn handle_gate(h: HANDLE, opts: ReadOpts) -> Result<(), VfsError> {
    if opts.allow_hydrate {
        return Ok(());
    }
    let t = sys::file_info(h, FileAttributeTagInfo, FILE_ATTRIBUTE_TAG_INFO::default())
        .map_err(|e| error(e, Domain::Project, "GetFileInformationByHandleEx"))?;
    let tag = if t.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        t.ReparseTag
    } else {
        0
    };
    if norm_attrs(t.FileAttributes, tag).is_cloud_only() {
        return Err(VfsError::new(
            VfsErrorKind::CloudOnly,
            OsCode::NONE,
            "read_for_hash",
        ));
    }
    Ok(())
}

/// How `unlink` and `durable_unlink` remove an entry ([OS/project §6.3]).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum UnlinkClass {
    /// A file, a file symbolic link or another non-directory reparse point: `DeleteFileW`.
    NotDir,
    /// A directory, a cloud directory placeholder included: `IsDirectory` for `unlink`, `remove_dir` for
    /// `durable_unlink`.
    Dir,
    /// A junction or a directory symbolic link (`DIRECTORY | REPARSE_POINT`, a non-cloud tag): `RemoveDirectoryW`, which
    /// removes the link and not its target.
    DirLink,
}

/// The class of the entry at the checked path `p` from its attributes (and its reparse tag, read from the parent's
/// directory entry, only for a directory reparse point).
fn unlink_class(p: &[u16], attrs: u32) -> UnlinkClass {
    if attrs & FILE_ATTRIBUTE_DIRECTORY == 0 {
        UnlinkClass::NotDir
    } else if attrs & FILE_ATTRIBUTE_REPARSE_POINT != 0 && !is_cloud_tag(reparse_tag(p)) {
        UnlinkClass::DirLink
    } else {
        UnlinkClass::Dir
    }
}

/// "No such entry" or "a parent is not a directory" ([OS/project §5.1]).
fn is_absent(code: u32) -> bool {
    matches!(code, 2 | 3 | 267)
}

/// A case-insensitive (ordinal, the OS's upcase table) prefix strip: the rest of `s` after `prefix`, if `s` starts with
/// it ignoring case. Used only to classify a final path under `<drive>:/$Recycle.Bin/` (`locate_id`); containment
/// compares exactly ([`under_root`]).
fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    let n = prefix.chars().count();
    let split = s.char_indices().nth(n).map_or(s.len(), |(i, _)| i);
    if s.chars().count() < n {
        return None;
    }
    let (head, rest) = s.split_at(split);
    let (a, b): (Vec<u16>, Vec<u16>) = (
        head.encode_utf16().collect(),
        prefix.encode_utf16().collect(),
    );
    // SAFETY: both buffers are valid for the counts passed.
    let r =
        unsafe { CompareStringOrdinal(a.as_ptr(), a.len() as i32, b.as_ptr(), b.len() as i32, 1) };
    // CSTR_EQUAL = 2.
    (r == 2).then_some(rest)
}

/// The final path of an open handle as P12 text.
fn final_text(h: HANDLE) -> Result<String, VfsError> {
    let fin =
        sys::final_path(h).map_err(|e| error(e, Domain::Project, "GetFinalPathNameByHandleW"))?;
    rewrite_final(&fin).ok_or_else(|| {
        VfsError::new(
            VfsErrorKind::InvalidName,
            OsCode(123),
            "GetFinalPathNameByHandleW",
        )
    })
}

/// The part of `text` under the root, if it lies under it (the root itself gives the empty path): the root's text
/// followed by `/` must be an exact byte prefix of `text` ([OS/project §5.5] step 3, [OS/path §7] step 5). Both texts
/// are `GetFinalPathNameByHandleW` results in on-disk spelling, so no case folding is needed, and none is allowed: in a
/// tree with per-directory case sensitivity a sibling that differs from the root only in case is outside it.
fn under_root<'a>(root: &OsProjectRoot, text: &'a str) -> Option<&'a str> {
    let r = root.text.as_str();
    let rest = text.strip_prefix(r)?;
    if rest.is_empty() || r.ends_with('/') {
        Some(rest)
    } else {
        rest.strip_prefix('/')
    }
}

fn dir_failure(call: &'static str, code: u32) -> DurabilityFailure {
    DurabilityFailure {
        class: DurabilityClass::DurableName,
        call,
        kind: sys::kind_of(code, Domain::ProjectData),
        os: OsCode(code as i32),
    }
}

/// `durable-name` of the directory `p` (NUL-terminated): `CreateFileW(GENERIC_READ | GENERIC_WRITE, full sharing,
/// OPEN_EXISTING, FILE_FLAG_BACKUP_SEMANTICS)`, `FlushFileBuffers`, `CloseHandle` ([OS/project §6.2]).
fn sync_dir_path(p: &[u16]) -> Result<(), DurabilityFailure> {
    bump(&COUNTERS.dir_syncs, 1);
    let h = create_file(
        p,
        GENERIC_READ | GENERIC_WRITE,
        SHARE_ALL,
        OPEN_EXISTING,
        FILE_FLAG_BACKUP_SEMANTICS,
    )
    .map_err(|e| dir_failure("CreateFileW", e))?;
    flush(raw(&h)).map_err(|e| dir_failure("FlushFileBuffers", e))
}

/// The parent directory of `at`'s `\\?\` path, NUL-terminated.
fn parent_path(at: At<'_, OsProjectRoot>, call: &'static str) -> Result<Vec<u16>, VfsError> {
    let p = path_of(at, call)?;
    parent_z(&p[..p.len() - 1])
        .ok_or_else(|| VfsError::new(VfsErrorKind::InvalidName, OsCode::NONE, call))
}

/// `durable-name` of the parents `a` and `b` of a rename, once if they are one directory ([OS/project §6.3]).
///
/// "One directory" is decided on the exact spelling: both paths are the root's final path joined with checked
/// `RelPath` segments, so two equal paths are one directory. Paths that differ only in case are flushed twice. In a tree
/// with per-directory case sensitivity (`CaseRule::PerDirFlag`) they can be two directories, and skipping the second
/// flush would leave the rename's new entry pending (FM-2.3); where they are one directory, the second flush is a
/// harmless repeat.
fn sync_parents(a: &[u16], b: &[u16]) -> Result<(), DurabilityFailure> {
    sync_dir_path(a)?;
    if a != b {
        sync_dir_path(b)?;
    }
    Ok(())
}

/// The mtime granularity probe's parameters, the widest candidates of the holes ([OS/project §4.4]; filled by WP-81a
/// from measurement 15): HOLE(OS-pfs-gran-probe-k) and HOLE(OS-pfs-gran-probe-budget).
const GRAN_PROBE_K: usize = 4;
const GRAN_PROBE_BUDGET_NS: u64 = 100_000_000;

/// The name `touch_stamp` writes in `<store>/tmp/` ([OS/project §5.9], open point 18).
const SETTLE_STAMP: RelPath<'static> = RelPath::literal("settle.stamp");

impl OsProjectFs {
    /// `stat(at, Read)` of the checked path `p` of `at`.
    fn stat_read(&self, at: At<'_, OsProjectRoot>, p: &[u16]) -> Result<Stat, VfsError> {
        let d = match attr_data(p) {
            Ok(d) => d,
            Err(e) if is_absent(e) => return Ok(Stat::Absent),
            Err(e) => return Err(error(e, Domain::Project, "GetFileAttributesExW")),
        };
        let attrs = d.dwFileAttributes;
        let tag = if attrs & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            reparse_tag(p)
        } else {
            0
        };
        let (mg, bg, _) = grans(at.root.fs);
        Ok(Stat::Present(StatRec {
            kind: kind_of(attrs, tag),
            size: (u64::from(d.nFileSizeHigh) << 32) | u64::from(d.nFileSizeLow),
            mtime: FsTime::from_filetime(ft_u64(&d.ftLastWriteTime), mg),
            ctime: FsTime::ABSENT,
            btime: FsTime::from_filetime(ft_u64(&d.ftCreationTime), bg),
            added: FsTime::ABSENT,
            attrs: norm_attrs(attrs, tag),
            reparse_tag: tag,
            id: None,
            nlink: 0,
        }))
    }

    /// `stat(at, WithId)` of the checked path `p` of `at`.
    fn stat_with_id(&self, at: At<'_, OsProjectRoot>, p: &[u16]) -> Result<Stat, VfsError> {
        // Never open a placeholder that recalls on open ([OS/project §5.10] rule 3).
        match attrs_of(p) {
            Ok(a) if a & (FILE_ATTRIBUTE_RECALL_ON_OPEN | FILE_ATTRIBUTE_OFFLINE) != 0 => {
                return Err(VfsError::new(VfsErrorKind::CloudOnly, OsCode::NONE, "stat"));
            }
            Ok(_) => {}
            Err(e) if is_absent(e) => return Ok(Stat::Absent),
            Err(e) => return Err(error(e, Domain::Project, "GetFileAttributesW")),
        }
        let h = match open_attrs(p, FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT) {
            Ok(h) => h,
            Err(e) if is_absent(e) => return Ok(Stat::Absent),
            Err(e) => return Err(error(e, Domain::Project, "CreateFileW")),
        };
        let q = |e| error(e, Domain::Project, "GetFileInformationByHandleEx");
        let basic =
            sys::file_info(raw(&h), FileBasicInfo, FILE_BASIC_INFO::default()).map_err(q)?;
        let std = standard_info(raw(&h)).map_err(q)?;
        let tag = sys::file_info(
            raw(&h),
            FileAttributeTagInfo,
            FILE_ATTRIBUTE_TAG_INFO::default(),
        )
        .map_err(q)?;
        let parent = match parent_z(&p[..p.len() - 1]) {
            Some(pp) => open_attrs(&pp, FILE_FLAG_BACKUP_SEMANTICS)
                .ok()
                .and_then(|ph| id_info(raw(&ph)).ok())
                .map_or([0; 16], |i| i.FileId.Identifier),
            None => [0; 16],
        };
        let fs = at.root.fs;
        let id = file_id_of(raw(&h), fs, parent)?;
        let (mg, bg, has_ctime) = grans(fs);
        let attrs = tag.FileAttributes;
        let rtag = if attrs & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            tag.ReparseTag
        } else {
            0
        };
        Ok(Stat::Present(StatRec {
            kind: kind_of(attrs, rtag),
            size: std.EndOfFile as u64,
            mtime: FsTime::from_filetime(basic.LastWriteTime as u64, mg),
            ctime: if has_ctime {
                FsTime::from_filetime(basic.ChangeTime as u64, mg)
            } else {
                FsTime::ABSENT
            },
            btime: FsTime::from_filetime(basic.CreationTime as u64, bg),
            added: FsTime::ABSENT,
            attrs: norm_attrs(attrs, rtag),
            reparse_tag: rtag,
            id: (!id.is_none()).then_some(id),
            nlink: std.NumberOfLinks,
        }))
    }

    /// One enumeration pass with the extended-id classes (NTFS, ReFS) or the plain ones (no ids); `Err(None)` when the
    /// extended class is not supported by the file system, so the caller retries with the plain one.
    fn enumerate_with<F>(
        &self,
        dir: At<'_, OsProjectRoot>,
        h: HANDLE,
        ext: bool,
        visit: &mut F,
    ) -> Result<EnumEnd, Option<VfsError>>
    where
        F: FnMut(&ProjEntry<'_>) -> ControlFlow<()>,
    {
        let fs = dir.root.fs;
        let (mg, bg, has_ctime) = grans(fs);
        let kind = id_kind(fs);
        let dir_id = if ext {
            id_info(h)
                .map_err(|e| Some(error(e, Domain::Project, "GetFileInformationByHandleEx")))?
        } else {
            Default::default()
        };
        let vk = vol_key(dir_id.VolumeSerialNumber);
        let (restart, next) = if ext {
            (FileIdExtdDirectoryRestartInfo, FileIdExtdDirectoryInfo)
        } else {
            (FileFullDirectoryRestartInfo, FileFullDirectoryInfo)
        };
        macro_rules! at {
            ($t:ty, $f:ident) => {
                core::mem::offset_of!($t, $f)
            };
        }
        let (o_c, o_w, o_ch, o_eof, o_attr, o_len, o_ea, o_name) = if ext {
            (
                at!(FILE_ID_EXTD_DIR_INFO, CreationTime),
                at!(FILE_ID_EXTD_DIR_INFO, LastWriteTime),
                at!(FILE_ID_EXTD_DIR_INFO, ChangeTime),
                at!(FILE_ID_EXTD_DIR_INFO, EndOfFile),
                at!(FILE_ID_EXTD_DIR_INFO, FileAttributes),
                at!(FILE_ID_EXTD_DIR_INFO, FileNameLength),
                at!(FILE_ID_EXTD_DIR_INFO, ReparsePointTag),
                at!(FILE_ID_EXTD_DIR_INFO, FileName),
            )
        } else {
            (
                at!(FILE_FULL_DIR_INFO, CreationTime),
                at!(FILE_FULL_DIR_INFO, LastWriteTime),
                at!(FILE_FULL_DIR_INFO, ChangeTime),
                at!(FILE_FULL_DIR_INFO, EndOfFile),
                at!(FILE_FULL_DIR_INFO, FileAttributes),
                at!(FILE_FULL_DIR_INFO, FileNameLength),
                // For a reparse point, `EaSize` holds the reparse tag in the plain class.
                at!(FILE_FULL_DIR_INFO, EaSize),
                at!(FILE_FULL_DIR_INFO, FileName),
            )
        };
        let o_id = at!(FILE_ID_EXTD_DIR_INFO, FileId);
        let mut buf = DirBuf::new(64 * 1024);
        let mut class = restart;
        let mut stopped = false;
        // One name buffer reused across records, which each entry borrows ([OS/project §5.2]): a scan allocates nothing
        // per entry.
        let mut name = Vec::with_capacity(64);
        loop {
            match buf.fill(h, class) {
                Ok(true) => {}
                Ok(false) => break,
                Err(87 | 1 | 50) if ext && class == restart => return Err(None),
                Err(e) => {
                    return Err(Some(error(
                        e,
                        Domain::Project,
                        "GetFileInformationByHandleEx",
                    )));
                }
            }
            class = next;
            records(buf.bytes(), |rec| {
                let len = u32_at(rec, o_len) as usize;
                if is_dot_name(&rec[o_name..o_name + len]) {
                    return true;
                }
                record_name(&mut name, rec, o_name, len);
                let attrs = u32_at(rec, o_attr);
                let tag = if attrs & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                    u32_at(rec, o_ea)
                } else {
                    0
                };
                let k = kind_of(attrs, tag);
                let id = (ext && kind != FileIdKind::None).then(|| {
                    let mut id = [0u8; 16];
                    id.copy_from_slice(&rec[o_id..o_id + 16]);
                    OsFileId {
                        kind,
                        vol_key: vk,
                        id,
                        parent: dir_id.FileId.Identifier,
                        docid: 0,
                    }
                });
                let entry = ProjEntry {
                    name: EntryNameRef::from_os_bytes(&name),
                    kind: k,
                    stat: Some(StatRec {
                        kind: k,
                        size: u64_at(rec, o_eof),
                        mtime: FsTime::from_filetime(u64_at(rec, o_w), mg),
                        ctime: if has_ctime {
                            FsTime::from_filetime(u64_at(rec, o_ch), mg)
                        } else {
                            FsTime::ABSENT
                        },
                        btime: FsTime::from_filetime(u64_at(rec, o_c), bg),
                        added: FsTime::ABSENT,
                        attrs: norm_attrs(attrs, tag),
                        reparse_tag: tag,
                        id,
                        nlink: 0,
                    }),
                    ino_hint: None,
                };
                if visit(&entry).is_break() {
                    stopped = true;
                    return false;
                }
                true
            });
            if stopped {
                return Ok(EnumEnd::Stopped);
            }
        }
        Ok(EnumEnd::Complete)
    }
}

impl ProjectFs for OsProjectFs {
    type Root = OsProjectRoot;
    type Reader = OsReader;

    fn canonical_root(&self, dir: &Path) -> Result<CanonicalRoot, VfsError> {
        super::path::canonical_root(dir)
    }

    fn canonical_abs(&self, p: &Path) -> Result<AbsPath, VfsError> {
        super::path::canonical_abs(p)
    }

    fn cli_path(
        &self,
        arg: &OsStr,
        cwd: &Path,
        tree: &CanonicalRoot,
    ) -> Result<RelPathBuf, PathError> {
        super::path::cli_path(arg, cwd, tree)
    }

    fn open_root(&self, root: &CanonicalRoot) -> Result<OsProjectRoot, VfsError> {
        let bad = || VfsError::new(VfsErrorKind::InvalidName, OsCode::NONE, "open_root");
        if root.os != OsTag::Windows {
            return Err(bad());
        }
        let path = verbatim_of_abs(root.text.as_str()).ok_or_else(bad)?;
        let h = open_attrs(&sys::with_nul(&path), FILE_FLAG_BACKUP_SEMANTICS)
            .map_err(|e| error(e, Domain::Project, "CreateFileW"))?;
        let fs = volume_family(raw(&h), &path)?;
        let info = id_info(raw(&h))
            .map_err(|e| error(e, Domain::Project, "GetFileInformationByHandleEx"))?;
        if !root.root_id.is_none() {
            let now = file_id_of(raw(&h), fs, [0; 16])?;
            if !now.same_object(&root.root_id) {
                return Err(VfsError::new(
                    VfsErrorKind::Stale,
                    OsCode::NONE,
                    "open_root",
                ));
            }
        }
        Ok(OsProjectRoot {
            text: root.text.clone(),
            path: path.into_boxed_slice(),
            root_id: root.root_id,
            fs,
            vol_key: vol_key(info.VolumeSerialNumber),
        })
    }

    fn volume(&self, root: &OsProjectRoot) -> Result<(VolumeKey, VolumeCaps), VfsError> {
        let mut cache = self.caps.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((_, c)) = cache.iter().find(|(k, _)| *k == root.vol_key) {
            return Ok((root.vol_key, *c));
        }
        // The family was decided at `open_root`, a network redirector included ([OS/project §4.3]); only NTFS and ReFS
        // need the volume query, for the USN journal's availability.
        let journal = if matches!(root.fs, FsFamily::Ntfs | FsFamily::Refs) {
            let h = open_attrs(&sys::with_nul(&root.path), FILE_FLAG_BACKUP_SEMANTICS)
                .map_err(|e| error(e, Domain::Project, "CreateFileW"))?;
            let (_, flags) = volume_name_and_flags(raw(&h))?;
            if flags & FILE_SUPPORTS_USN_JOURNAL != 0 {
                let mut out = [0u64; 16];
                let mut ret = 0u32;
                // SAFETY: `out` is a writable 128-byte buffer (a `USN_JOURNAL_DATA_V0`–`V2` fits); no input;
                // synchronous.
                let ok = unsafe {
                    DeviceIoControl(
                        raw(&h),
                        FSCTL_QUERY_USN_JOURNAL,
                        core::ptr::null(),
                        0,
                        out.as_mut_ptr().cast(),
                        128,
                        &mut ret,
                        core::ptr::null_mut(),
                    )
                };
                if ok != 0 {
                    JournalKind::Usn
                } else {
                    JournalKind::None
                }
            } else {
                JournalKind::None
            }
        } else {
            JournalKind::None
        };
        let caps = caps_for(root.fs, journal);
        cache.push((root.vol_key, caps));
        Ok((root.vol_key, caps))
    }

    fn case_equivalent(&self, dir: At<'_, OsProjectRoot>) -> Result<DirEquivalence, VfsError> {
        let p = path_of(dir, "CreateFileW")?;
        let h = open_attrs(&p, FILE_FLAG_BACKUP_SEMANTICS)
            .map_err(|e| error(e, Domain::Project, "CreateFileW"))?;
        let ci = match sys::file_info(
            raw(&h),
            FileCaseSensitiveInfo,
            FILE_CASE_SENSITIVE_INFO::default(),
        ) {
            Ok(i) => i.Flags & FILE_CS_FLAG_CASE_SENSITIVE_DIR == 0,
            // No per-directory flag on this file system: the volume rule (insensitive on every Windows volume).
            Err(1 | 50 | 87) => true,
            Err(e) => return Err(error(e, Domain::Project, "GetFileInformationByHandleEx")),
        };
        Ok(DirEquivalence {
            case_insensitive: ci,
            norm_insensitive: false,
        })
    }

    fn trash_dirs(&self, root: &OsProjectRoot) -> Result<Vec<AbsPath>, VfsError> {
        let t = root.text.as_str();
        let b = t.as_bytes();
        if b.len() >= 3 && b[1] == b':' {
            let bin = AbsPath::from_string(format!("{}:/$Recycle.Bin", &t[..1])).map_err(|_| {
                VfsError::new(VfsErrorKind::InvalidName, OsCode::NONE, "trash_dirs")
            })?;
            Ok(vec![bin])
        } else {
            Ok(Vec::new())
        }
    }

    fn measure_mtime_granularity(&self, stamp_dir: &OsProjectRoot) -> Result<u64, VfsError> {
        let at = At::new(stamp_dir, SETTLE_STAMP);
        let start = mono_ns();
        let mut seen: Vec<i64> = Vec::with_capacity(GRAN_PROBE_K);
        loop {
            let t = self.touch_stamp(at)?;
            if !seen.contains(&t.ns) {
                seen.push(t.ns);
            }
            if seen.len() >= GRAN_PROBE_K || mono_ns().saturating_sub(start) >= GRAN_PROBE_BUDGET_NS
            {
                break;
            }
        }
        let nominal = 10u64.pow(u32::from(grans(stamp_dir.fs).0));
        let raw_ns = if seen.len() >= 2 {
            seen.windows(2)
                .map(|w| w[1].abs_diff(w[0]))
                .filter(|&d| d > 0)
                .min()
                .unwrap_or(nominal)
        } else {
            mono_ns().saturating_sub(start)
        };
        Ok(raw_ns.div_ceil(nominal).max(1) * nominal)
    }

    fn stat(&self, at: At<'_, OsProjectRoot>, mode: StatMode) -> Result<Stat, VfsError> {
        // The name check comes first: a refused name makes no OS call and moves no counter.
        let call = match mode {
            StatMode::Read => "GetFileAttributesExW",
            StatMode::WithId => "CreateFileW",
        };
        let p = path_of(at, call)?;
        bump(&COUNTERS.stats, 1);
        match mode {
            StatMode::Read => self.stat_read(at, &p),
            StatMode::WithId => self.stat_with_id(at, &p),
        }
    }

    fn disk_spelling(&self, at: At<'_, OsProjectRoot>) -> Result<RelPathBuf, VfsError> {
        let p = path_of(at, "CreateFileW")?;
        match attrs_of(&p) {
            Ok(a)
                if a & (FILE_ATTRIBUTE_RECALL_ON_OPEN
                    | FILE_ATTRIBUTE_OFFLINE
                    | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS)
                    != 0 =>
            {
                return Err(VfsError::new(
                    VfsErrorKind::CloudOnly,
                    OsCode::NONE,
                    "disk_spelling",
                ));
            }
            Ok(_) => {}
            Err(e) => return Err(error(e, Domain::Project, "GetFileAttributesW")),
        }
        let h = open_attrs(
            &p,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
        )
        .map_err(|e| error(e, Domain::Project, "CreateFileW"))?;
        let text = final_text(raw(&h))?;
        let rest = under_root(at.root, &text).ok_or_else(|| {
            VfsError::new(VfsErrorKind::OutsideRoot, OsCode::NONE, "disk_spelling")
        })?;
        RelPathBuf::new(rest)
            .map_err(|_| VfsError::new(VfsErrorKind::InvalidName, OsCode::NONE, "disk_spelling"))
    }

    fn enumerate<F>(&self, dir: At<'_, OsProjectRoot>, mut visit: F) -> Result<EnumEnd, VfsError>
    where
        F: FnMut(&ProjEntry<'_>) -> ControlFlow<()>,
    {
        let p = path_of(dir, "CreateFileW")?;
        bump(&COUNTERS.dir_reads, 1);
        let a = attrs_of(&p).map_err(|e| error(e, Domain::Project, "GetFileAttributesW"))?;
        if a & FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS != 0 {
            return Err(VfsError::new(
                VfsErrorKind::CloudOnly,
                OsCode::NONE,
                "enumerate",
            ));
        }
        // A link is never enumerated: a junction or directory symlink named by `dir` would list its target (the open
        // below follows it, as [OS/project §5.2] specifies), so it is refused as `read_for_hash` refuses it — `IsSymlink`
        // for a symbolic link, `Other` for any other non-cloud reparse point — instead of listing outside the tree.
        if a & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            match kind_of(a, reparse_tag(&p)) {
                ProjKind::Dir | ProjKind::File => {}
                ProjKind::Symlink => {
                    return Err(VfsError::new(
                        VfsErrorKind::IsSymlink,
                        OsCode::NONE,
                        "enumerate",
                    ));
                }
                ProjKind::Other => {
                    return Err(VfsError::new(
                        VfsErrorKind::Other,
                        OsCode::NONE,
                        "enumerate",
                    ));
                }
            }
        }
        // The open of [OS/project §5.2] and the mapping appendix §2.1: no `FILE_FLAG_OPEN_REPARSE_POINT`, so a cloud
        // directory is opened through its provider (it is fully populated: `RECALL_ON_DATA_ACCESS` was refused above).
        let h = create_file(
            &p,
            FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            SHARE_ALL,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
        )
        .map_err(|e| error(e, Domain::Project, "CreateFileW"))?;
        let ext = id_kind(dir.root.fs) != FileIdKind::None;
        match self.enumerate_with(dir, raw(&h), ext, &mut visit) {
            Ok(end) => Ok(end),
            Err(Some(e)) => Err(e),
            Err(None) => self
                .enumerate_with(dir, raw(&h), false, &mut visit)
                .map_err(|e| {
                    e.unwrap_or_else(|| {
                        VfsError::new(
                            VfsErrorKind::Unsupported,
                            OsCode(87),
                            "GetFileInformationByHandleEx",
                        )
                    })
                }),
        }
    }

    fn locate_id(
        &self,
        root: &OsProjectRoot,
        id: &OsFileId,
        recorded: FileAttrs,
    ) -> Result<Located, VfsError> {
        if !matches!(id.kind, FileIdKind::Ntfs128 | FileIdKind::Refs128)
            || id.vol_key != root.vol_key
            || recorded.is_cloud_only()
        {
            return Ok(Located::NotLocatable);
        }
        let hint = open_attrs(&sys::with_nul(&root.path), FILE_FLAG_BACKUP_SEMANTICS)
            .map_err(|e| error(e, Domain::Project, "CreateFileW"))?;
        let desc = FILE_ID_DESCRIPTOR {
            dwSize: core::mem::size_of::<FILE_ID_DESCRIPTOR>() as u32,
            Type: ExtendedFileIdType,
            Anonymous: FILE_ID_DESCRIPTOR_0 {
                ExtendedFileId: FILE_ID_128 { Identifier: id.id },
            },
        };
        bump(&COUNTERS.id_lookups, 1);
        // SAFETY: `hint` is a valid handle on the volume; `desc` is a live descriptor of its declared size; no security
        // attributes.
        let h = unsafe {
            OpenFileById(
                raw(&hint),
                &desc,
                FILE_READ_ATTRIBUTES,
                SHARE_ALL,
                core::ptr::null(),
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            )
        };
        let h = match sys::owned(h) {
            Some(h) => h,
            None => {
                return match last_error() {
                    2 | 87 => Ok(Located::Gone),
                    e => Err(error(e, Domain::Project, "OpenFileById")),
                };
            }
        };
        drop(hint);
        let text = final_text(raw(&h))?;
        if let Some(rest) = under_root(root, &text) {
            return RelPathBuf::new(rest)
                .map(Located::InRoot)
                .map_err(|_| VfsError::new(VfsErrorKind::InvalidName, OsCode::NONE, "locate_id"));
        }
        let abs = AbsPath::from_string(text.clone())
            .map_err(|_| VfsError::new(VfsErrorKind::InvalidName, OsCode::NONE, "locate_id"))?;
        let b = text.as_bytes();
        if b.len() >= 3 && b[1] == b':' && strip_prefix_ci(&text[2..], "/$Recycle.Bin/").is_some() {
            return Ok(Located::InTrash(abs));
        }
        Ok(Located::Elsewhere(abs))
    }

    fn file_handle_digest(&self, at: At<'_, OsProjectRoot>) -> Result<Option<[u8; 8]>, VfsError> {
        // Windows ids need no digest ([OS/project §5.4]); the name check still applies to the path it is given
        // ([OS/project §2.3]), so every method answers an unrepresentable name alike.
        if at
            .path
            .segments()
            .any(|s| !segment_ok(s, NameCheck::Project))
        {
            return Err(VfsError::new(
                VfsErrorKind::InvalidName,
                OsCode(123),
                "file_handle_digest",
            ));
        }
        Ok(None)
    }

    fn read_for_hash(
        &self,
        at: At<'_, OsProjectRoot>,
        opts: ReadOpts,
    ) -> Result<OsReader, VfsError> {
        let p = path_of(at, "CreateFileW")?;
        // 1. The placeholder gate, without opening the entry.
        let d = attr_data(&p).map_err(|e| error(e, Domain::Project, "GetFileAttributesExW"))?;
        let attrs = d.dwFileAttributes;
        let tag = if attrs & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            reparse_tag(&p)
        } else {
            0
        };
        if norm_attrs(attrs, tag).is_cloud_only() && !opts.allow_hydrate {
            return Err(VfsError::new(
                VfsErrorKind::CloudOnly,
                OsCode::NONE,
                "read_for_hash",
            ));
        }
        match kind_of(attrs, tag) {
            ProjKind::File => {}
            ProjKind::Dir => {
                return Err(VfsError::new(
                    VfsErrorKind::IsDirectory,
                    OsCode(267),
                    "read_for_hash",
                ));
            }
            ProjKind::Symlink => {
                return Err(VfsError::new(
                    VfsErrorKind::IsSymlink,
                    OsCode::NONE,
                    "read_for_hash",
                ));
            }
            ProjKind::Other => {
                return Err(VfsError::new(
                    VfsErrorKind::Other,
                    OsCode::NONE,
                    "read_for_hash",
                ));
            }
        }
        // 2. The open: never following a link.
        let h = create_file(
            &p,
            GENERIC_READ,
            SHARE_ALL,
            OPEN_EXISTING,
            FILE_FLAG_SEQUENTIAL_SCAN | FILE_FLAG_OPEN_REPARSE_POINT,
        )
        .map_err(|e| error(e, Domain::Project, "CreateFileW"))?;
        // 3. Containment: the opened object's final path lies under the root.
        let text = final_text(raw(&h))?;
        if under_root(at.root, &text).is_none() {
            return Err(VfsError::new(
                VfsErrorKind::OutsideRoot,
                OsCode::NONE,
                "read_for_hash",
            ));
        }
        // The placeholder gate re-checked through the handle before the first read; on `CloudOnly` the handle is
        // dropped (closed) unread.
        handle_gate(raw(&h), opts)?;
        bump(&COUNTERS.content_opens, 1);
        Ok(OsReader {
            handle: h,
            fs: at.root.fs,
        })
    }

    fn read_link(&self, at: At<'_, OsProjectRoot>, out: &mut Vec<u8>) -> Result<(), VfsError> {
        let p = path_of(at, "CreateFileW")?;
        let h = open_attrs(
            &p,
            FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS,
        )
        .map_err(|e| error(e, Domain::Project, "CreateFileW"))?;
        let mut buf = DirBuf::new(16 * 1024);
        let mut ret = 0u32;
        let b = buf.as_mut_ptr();
        // SAFETY: `b` is the buffer's own memory, writable for 16 KiB (`MAXIMUM_REPARSE_DATA_BUFFER_SIZE`); no input.
        let ok = unsafe {
            DeviceIoControl(
                raw(&h),
                FSCTL_GET_REPARSE_POINT,
                core::ptr::null(),
                0,
                b.cast(),
                16 * 1024,
                &mut ret,
                core::ptr::null_mut(),
            )
        };
        let not_link = || VfsError::new(VfsErrorKind::Other, OsCode::NONE, "read_link");
        if ok == 0 {
            return match last_error() {
                // ERROR_NOT_A_REPARSE_POINT.
                4390 => Err(not_link()),
                e => Err(error(e, Domain::Project, "DeviceIoControl")),
            };
        }
        let bytes = &buf.bytes()[..(ret as usize).min(16 * 1024)];
        if bytes.len() < 20 || u32_at(bytes, 0) != IO_REPARSE_TAG_SYMLINK {
            return Err(not_link());
        }
        let u16_at = |at: usize| usize::from(u16::from_le_bytes([bytes[at], bytes[at + 1]]));
        let (sub_off, sub_len, print_off, print_len) =
            (u16_at(8), u16_at(10), u16_at(12), u16_at(14));
        let base = 20usize;
        let slice = |off: usize, len: usize| -> Result<Vec<u16>, VfsError> {
            let s = base + off;
            if s + len > bytes.len() || !len.is_multiple_of(2) {
                return Err(not_link());
            }
            Ok(units_at(bytes, s, len))
        };
        let mut units = slice(print_off, print_len)?;
        if units.is_empty() {
            units = slice(sub_off, sub_len)?;
            let prefix: Vec<u16> = "\\??\\".encode_utf16().collect();
            if units.starts_with(&prefix) {
                units.drain(..prefix.len());
            }
        }
        for u in &mut units {
            if *u == u16::from(b'\\') {
                *u = u16::from(b'/');
            }
        }
        sys::push_wtf8(out, &units);
        Ok(())
    }

    fn busy_holders(&self, at: At<'_, OsProjectRoot>) -> Result<Vec<Holder>, VfsError> {
        let p = path_of(at, "RmRegisterResources")?;
        let a = attrs_of(&p).map_err(|e| error(e, Domain::Project, "GetFileAttributesW"))?;
        if a & FILE_ATTRIBUTE_DIRECTORY != 0 {
            return Err(VfsError::new(
                VfsErrorKind::Unsupported,
                OsCode::NONE,
                "RmGetList",
            ));
        }
        let mut session = 0u32;
        let mut key = [0u16; CCH_RM_SESSION_KEY as usize + 1];
        // SAFETY: `session` is a live local; `key` is writable for `CCH_RM_SESSION_KEY + 1` units.
        let rc = unsafe { RmStartSession(&mut session, 0, key.as_mut_ptr()) };
        if rc != 0 {
            return Err(error(rc, Domain::Project, "RmStartSession"));
        }
        struct End(u32);
        impl Drop for End {
            fn drop(&mut self) {
                // SAFETY: the session was started above and is ended once.
                unsafe { RmEndSession(self.0) };
            }
        }
        let _end = End(session);
        // The Restart Manager takes the plain Win32 form of the path.
        let s = String::from_utf16_lossy(&p[..p.len() - 1]);
        let plain = if let Some(r) = s.strip_prefix("\\\\?\\UNC\\") {
            format!("\\\\{r}")
        } else {
            s.strip_prefix("\\\\?\\").unwrap_or(&s).to_owned()
        };
        let plain_z = sys::wide_z(plain.as_ref());
        let names = [plain_z.as_ptr()];
        // SAFETY: one NUL-terminated file name that outlives the call; no applications, no services.
        let rc = unsafe {
            RmRegisterResources(
                session,
                1,
                names.as_ptr(),
                0,
                core::ptr::null(),
                0,
                core::ptr::null(),
            )
        };
        if rc != 0 {
            return Err(error(rc, Domain::Project, "RmRegisterResources"));
        }
        let mut infos: Vec<RM_PROCESS_INFO> = vec![RM_PROCESS_INFO::default(); 4];
        loop {
            let (mut needed, mut count, mut reasons) = (0u32, infos.len() as u32, 0u32);
            // SAFETY: `infos` is writable for `count` entries; the three counters are live locals.
            let rc = unsafe {
                RmGetList(
                    session,
                    &mut needed,
                    &mut count,
                    infos.as_mut_ptr(),
                    &mut reasons,
                )
            };
            match rc {
                0 => {
                    return Ok(infos[..count as usize]
                        .iter()
                        .map(|i| {
                            let n = i
                                .strAppName
                                .iter()
                                .position(|&u| u == 0)
                                .unwrap_or(i.strAppName.len());
                            Holder {
                                pid: i.Process.dwProcessId,
                                name: String::from_utf16_lossy(&i.strAppName[..n]).into_boxed_str(),
                            }
                        })
                        .collect());
                }
                ERROR_MORE_DATA if (needed as usize) <= 4096 => {
                    infos.resize(needed as usize + 1, RM_PROCESS_INFO::default());
                }
                rc => return Err(error(rc, Domain::Project, "RmGetList")),
            }
        }
    }

    fn touch_stamp(&self, at: At<'_, OsProjectRoot>) -> Result<FsTime, VfsError> {
        let p = path_of(at, "CreateFileW")?;
        let h = create_file(
            &p,
            GENERIC_WRITE,
            SHARE_ALL,
            OPEN_ALWAYS,
            FILE_ATTRIBUTE_NORMAL,
        )
        .map_err(|e| error(e, Domain::Project, "CreateFileW"))?;
        write_once(raw(&h), 0, &[0]).map_err(|e| error(e, Domain::ProjectData, "WriteFile"))?;
        drop(h);
        let d = attr_data(&p).map_err(|e| error(e, Domain::Project, "GetFileAttributesExW"))?;
        Ok(FsTime::from_filetime(
            ft_u64(&d.ftLastWriteTime),
            grans(at.root.fs).0,
        ))
    }

    fn rename_noreplace(
        &self,
        from: At<'_, OsProjectRoot>,
        to: At<'_, OsProjectRoot>,
        retry: ShareRetry,
    ) -> Result<Renamed, VfsError> {
        let f = path_of(from, "MoveFileExW")?;
        let t = path_of(to, "MoveFileExW")?;
        move_file(&f, &t, false, retry).map_err(|e| error(e, Domain::Project, "MoveFileExW"))?;
        bump(&COUNTERS.renames, 1);
        Ok(Renamed::Renamed)
    }

    fn sync_dir(&self, dir: At<'_, OsProjectRoot>) -> Result<(), DurabilityFailure> {
        let p = path_of(dir, "CreateFileW").map_err(|e| DurabilityFailure {
            class: DurabilityClass::DurableName,
            call: e.call,
            kind: e.kind,
            os: e.os,
        })?;
        sync_dir_path(&p)
    }

    fn durable_rename(
        &self,
        from: At<'_, OsProjectRoot>,
        to: At<'_, OsProjectRoot>,
        retry: ShareRetry,
    ) -> Result<Renamed, RenameFailure> {
        let pf = parent_path(from, "MoveFileExW").map_err(RenameFailure::NotDone)?;
        let pt = parent_path(to, "MoveFileExW").map_err(RenameFailure::NotDone)?;
        let r = self
            .rename_noreplace(from, to, retry)
            .map_err(RenameFailure::NotDone)?;
        sync_parents(&pf, &pt).map_err(RenameFailure::NotDurable)?;
        Ok(r)
    }

    fn unlink(&self, at: At<'_, OsProjectRoot>, retry: ShareRetry) -> Result<(), VfsError> {
        let p = path_of(at, "DeleteFileW")?;
        let attrs = attrs_of(&p).map_err(|e| error(e, Domain::Project, "GetFileAttributesW"))?;
        let dir_link = match unlink_class(&p, attrs) {
            UnlinkClass::Dir => {
                return Err(VfsError::new(
                    VfsErrorKind::IsDirectory,
                    OsCode(267),
                    "DeleteFileW",
                ));
            }
            UnlinkClass::DirLink => true,
            UnlinkClass::NotDir => false,
        };
        let cleared = attrs & FILE_ATTRIBUTE_READONLY != 0;
        if cleared {
            let a = match attrs & !FILE_ATTRIBUTE_READONLY {
                0 => FILE_ATTRIBUTE_NORMAL,
                a => a,
            };
            // SAFETY: `p` is NUL-terminated and outlives the call.
            if unsafe { SetFileAttributesW(p.as_ptr(), a) } == 0 {
                return Err(error(last_error(), Domain::Project, "SetFileAttributesW"));
            }
        }
        // A directory link is removed with `RemoveDirectoryW`, which deletes the link and never its target;
        // `DeleteFileW` refuses directory links.
        let call = if dir_link {
            "RemoveDirectoryW"
        } else {
            "DeleteFileW"
        };
        let r = with_share_retry(retry, || {
            // SAFETY: `p` is NUL-terminated and outlives the call.
            let ok = unsafe {
                if dir_link {
                    RemoveDirectoryW(p.as_ptr())
                } else {
                    DeleteFileW(p.as_ptr())
                }
            };
            if ok != 0 { Ok(()) } else { Err(last_error()) }
        });
        match r {
            Ok(()) => {
                bump(&COUNTERS.unlinks, 1);
                Ok(())
            }
            Err(e) => {
                if cleared {
                    // A failed delete leaves the user's file as it was ([OS/project §6.3], open point 9).
                    // SAFETY: `p` is NUL-terminated and outlives the call.
                    unsafe { SetFileAttributesW(p.as_ptr(), attrs) };
                }
                Err(error(e, Domain::Project, call))
            }
        }
    }

    fn remove_dir(&self, at: At<'_, OsProjectRoot>, retry: ShareRetry) -> Result<(), VfsError> {
        let p = path_of(at, "RemoveDirectoryW")?;
        with_share_retry(retry, || {
            // SAFETY: `p` is NUL-terminated and outlives the call.
            if unsafe { RemoveDirectoryW(p.as_ptr()) } != 0 {
                Ok(())
            } else {
                Err(last_error())
            }
        })
        .map_err(|e| error(e, Domain::Project, "RemoveDirectoryW"))?;
        bump(&COUNTERS.unlinks, 1);
        Ok(())
    }

    fn durable_unlink(
        &self,
        at: At<'_, OsProjectRoot>,
        retry: ShareRetry,
    ) -> Result<(), RenameFailure> {
        let parent = parent_path(at, "DeleteFileW").map_err(RenameFailure::NotDone)?;
        let p = path_of(at, "DeleteFileW").map_err(RenameFailure::NotDone)?;
        let attrs = attrs_of(&p)
            .map_err(|e| RenameFailure::NotDone(error(e, Domain::Project, "GetFileAttributesW")))?;
        match unlink_class(&p, attrs) {
            UnlinkClass::Dir => self.remove_dir(at, retry).map_err(RenameFailure::NotDone)?,
            UnlinkClass::DirLink | UnlinkClass::NotDir => {
                self.unlink(at, retry).map_err(RenameFailure::NotDone)?;
            }
        }
        sync_dir_path(&parent).map_err(RenameFailure::NotDurable)
    }

    fn counters(&self) -> PfsCounters {
        let r = |c: &AtomicU64| c.load(Ordering::Relaxed);
        PfsCounters {
            renames: r(&COUNTERS.renames),
            dir_syncs: r(&COUNTERS.dir_syncs),
            unlinks: r(&COUNTERS.unlinks),
            stats: r(&COUNTERS.stats),
            dir_reads: r(&COUNTERS.dir_reads),
            id_lookups: r(&COUNTERS.id_lookups),
            content_opens: r(&COUNTERS.content_opens),
            bytes_read: r(&COUNTERS.bytes_read),
        }
    }
}

impl ProjectRead for OsReader {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, VfsError> {
        let len = buf.len().min(u32::MAX as usize) as u32;
        let mut n = 0u32;
        // SAFETY: `buf` is writable for `len` bytes; the handle is synchronous (no OVERLAPPED), reading at its offset.
        let ok = unsafe {
            ReadFile(
                raw(&self.handle),
                buf.as_mut_ptr(),
                len,
                &mut n,
                core::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return match last_error() {
                ERROR_HANDLE_EOF => Ok(0),
                e => Err(error(e, Domain::ProjectData, "ReadFile")),
            };
        }
        bump(&COUNTERS.bytes_read, u64::from(n));
        Ok(n as usize)
    }

    fn rewind(&mut self) -> Result<(), VfsError> {
        // SAFETY: the handle is valid; no new-position output.
        let ok =
            unsafe { SetFilePointerEx(raw(&self.handle), 0, core::ptr::null_mut(), FILE_BEGIN) };
        if ok != 0 {
            Ok(())
        } else {
            Err(error(last_error(), Domain::ProjectData, "SetFilePointerEx"))
        }
    }

    fn snapshot(&self) -> Result<ReadSnapshot, VfsError> {
        let q = |e| error(e, Domain::Project, "GetFileInformationByHandleEx");
        let std = standard_info(raw(&self.handle)).map_err(q)?;
        let basic = sys::file_info(raw(&self.handle), FileBasicInfo, FILE_BASIC_INFO::default())
            .map_err(q)?;
        Ok(ReadSnapshot {
            size: std.EndOfFile as u64,
            mtime: FsTime::from_filetime(basic.LastWriteTime as u64, grans(self.fs).0),
        })
    }

    fn identity(&self) -> Result<OsFileId, VfsError> {
        file_id_of(raw(&self.handle), self.fs, [0; 16])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attribute_normalisation() {
        let a = norm_attrs(
            FILE_ATTRIBUTE_READONLY | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS,
            0,
        );
        assert!(a.contains(FileAttrs::READONLY) && a.contains(FileAttrs::RECALL_ON_DATA_ACCESS));
        assert!(a.is_cloud_only());
        let c = norm_attrs(
            FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_PINNED,
            0x9000_601A,
        );
        assert!(
            c.contains(FileAttrs::CLOUD_REPARSE)
                && c.contains(FileAttrs::PINNED)
                && !c.is_cloud_only()
        );
        assert_eq!(
            norm_attrs(0, 0x9000_001A),
            FileAttrs::NONE,
            "a tag without the attribute is ignored"
        );
        assert_eq!(
            FileAttrs::from_bits(norm_attrs(u32::MAX, 0x9000_001A).bits()).map(|x| x.bits() >> 11),
            Some(0)
        );
    }

    #[test]
    fn kinds_from_attributes() {
        assert_eq!(kind_of(FILE_ATTRIBUTE_NORMAL, 0), ProjKind::File);
        assert_eq!(kind_of(FILE_ATTRIBUTE_DIRECTORY, 0), ProjKind::Dir);
        assert_eq!(
            kind_of(FILE_ATTRIBUTE_REPARSE_POINT, IO_REPARSE_TAG_SYMLINK),
            ProjKind::Symlink
        );
        assert_eq!(
            kind_of(
                FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_DIRECTORY,
                0xA000_0003
            ),
            ProjKind::Other,
            "a junction"
        );
        assert_eq!(
            kind_of(
                FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_DIRECTORY,
                0x9000_001A
            ),
            ProjKind::Dir
        );
        assert_eq!(
            kind_of(FILE_ATTRIBUTE_REPARSE_POINT, 0x8000_001B),
            ProjKind::Other,
            "an app-execution alias"
        );
    }

    #[test]
    fn volume_keys_are_framed() {
        let k = vol_key(0x1234);
        assert_ne!(k, vol_key(0x1235));
        let mut h = blake3::Hasher::new();
        h.update(&17u32.to_le_bytes());
        h.update(b"moirai-vol-key-v1");
        h.update(&20u32.to_le_bytes());
        h.update(b"win-volume-serial-64");
        h.update(&8u32.to_le_bytes());
        h.update(&0x1234u64.to_le_bytes());
        assert_eq!(k.0[..], h.finalize().as_bytes()[..16]);
    }

    #[test]
    fn prefixes_and_exact_containment() {
        assert_eq!(strip_prefix_ci("D:/Repo/src", "d:/repo"), Some("/src"));
        assert_eq!(strip_prefix_ci("D:/Répo/x", "D:/RÉPO"), Some("/x"));
        assert_eq!(strip_prefix_ci("D:/Rep", "D:/Repo"), None);
        assert_eq!(strip_prefix_ci("D:/Other", "D:/Repo"), None);
        let root = OsProjectRoot {
            text: AbsPath::new("D:/repo").unwrap(),
            path: Box::from(&[][..]),
            root_id: OsFileId::NONE,
            fs: FsFamily::Ntfs,
            vol_key: VolumeKey([0; 16]),
        };
        assert_eq!(under_root(&root, "D:/repo"), Some(""));
        assert_eq!(under_root(&root, "D:/repo/a/b"), Some("a/b"));
        assert_eq!(under_root(&root, "D:/repository"), None);
        // Containment is exact: a sibling that differs only in case (per-directory case sensitivity) is outside.
        assert_eq!(under_root(&root, "D:/Repo"), None);
        assert_eq!(under_root(&root, "D:/REPO/a"), None);
        let drive = OsProjectRoot {
            text: AbsPath::new("D:/").unwrap(),
            ..root
        };
        assert_eq!(under_root(&drive, "D:/"), Some(""));
        assert_eq!(under_root(&drive, "D:/x/y"), Some("x/y"));
        assert_eq!(under_root(&drive, "E:/x"), None);
    }

    #[test]
    fn granularities() {
        assert_eq!(grans(FsFamily::Ntfs), (2, 2, true));
        assert_eq!(grans(FsFamily::Fat32), (10, 7, false));
        assert_eq!(grans(FsFamily::ExFat), (7, 7, false));
        assert_eq!(family_of("NTFS", false), FsFamily::Ntfs);
        assert_eq!(family_of("ReFS", false), FsFamily::Refs);
        assert_eq!(family_of("exFAT", false), FsFamily::ExFat);
        assert_eq!(family_of("FAT32", false), FsFamily::Fat32);
        assert_eq!(family_of("UDF", false), FsFamily::Other);
    }

    #[test]
    fn a_network_redirector_is_the_other_row_whatever_its_name() {
        // An SMB share or a mapped drive usually reports "NTFS" ([OS/project §4.3] row "any other or a network
        // redirector"): no ids, no by-id lookup, no journal, the volume case rule.
        for name in ["NTFS", "ReFS", "exFAT", "whatever"] {
            assert_eq!(family_of(name, true), FsFamily::Other, "{name}");
        }
        let net = caps_for(family_of("NTFS", true), JournalKind::Usn);
        assert_eq!(net.id_kind, FileIdKind::None as u8);
        assert_eq!(net.id_locate, IdLocate::None);
        assert_eq!(
            net.journal,
            JournalKind::None,
            "a journal counts only on NTFS and ReFS"
        );
        assert_eq!(net.btime, BtimeTrust::Absent);
        assert_eq!(net.ctime_on_rename, None);
        assert_eq!(net.case_rule, CaseRule::Volume);
        assert!(net.case_insensitive_default);
        assert_eq!(net.cloud, CloudRule::None);
        assert_eq!(net.rename_noreplace, RenameRule::Native);
        assert!(!net.ids_persistent);
        assert!(
            net.dir_flush_doubtful,
            "the row's other flag ([OS/project §4.3])"
        );
        assert!(net.is_valid());
        for fs in [
            FsFamily::Ntfs,
            FsFamily::Refs,
            FsFamily::Fat32,
            FsFamily::ExFat,
        ] {
            let c = caps_for(fs, JournalKind::None);
            assert!(!c.dir_flush_doubtful && c.is_valid(), "{fs:?}");
        }
        let local = caps_for(family_of("NTFS", false), JournalKind::Usn);
        assert_eq!(local.id_kind, FileIdKind::Ntfs128 as u8);
        assert_eq!(
            (local.id_locate, local.journal, local.case_rule),
            (IdLocate::ById, JournalKind::Usn, CaseRule::PerDirFlag)
        );
        assert!(local.ids_persistent);
        let refs = caps_for(FsFamily::Refs, JournalKind::None);
        assert_eq!(
            (refs.id_kind, refs.btime),
            (FileIdKind::Refs128 as u8, BtimeTrust::Absent)
        );
        let fat = caps_for(FsFamily::Fat32, JournalKind::None);
        assert_eq!(
            (fat.id_kind, fat.id_locate, fat.case_rule),
            (0, IdLocate::None, CaseRule::Volume)
        );
        // A `\\?\UNC\` path is remote without an OS call; a local lane directory is not.
        let w = |s: &str| s.encode_utf16().collect::<Vec<u16>>();
        assert!(is_remote(&w("\\\\?\\UNC\\srv\\share\\x")).unwrap());
        assert!(is_remote(&w("\\\\?\\unc\\srv\\share")).unwrap());
        let t = crate::windows::testing::TempDir::new("remote");
        let local = sys::verbatim(t.path()).unwrap();
        assert!(!is_remote(&local).unwrap());
    }

    #[test]
    fn the_drive_type_of_a_verbatim_path() {
        use windows_sys::Win32::System::WindowsProgramming::DRIVE_FIXED;
        // `GetDriveTypeW` of the `GetVolumePathNameW` root of a `\\?\X:\…` path: the lane directory is on a fixed disk.
        let t = crate::windows::testing::TempDir::new("drivetype");
        let local = sys::verbatim(t.path()).unwrap();
        assert_eq!(drive_type(&sys::with_nul(&local)).unwrap(), DRIVE_FIXED);
    }

    #[test]
    fn the_handle_gate_refuses_a_placeholder_opened_after_the_attribute_read() {
        use windows_sys::Win32::Storage::FileSystem::SetFileAttributesW;
        // [OS/project §5.5] step 3 (pass 1, P1-38): an entry that became cloud-only between the attribute read and the
        // open is refused through the handle. The handle is opened as `read_for_hash` opens it; `OFFLINE` stands in for
        // a placeholder, as in the integration test.
        let t = crate::windows::testing::TempDir::new("gate");
        let f = t.path().join("f");
        std::fs::write(&f, b"content").unwrap();
        let p = sys::with_nul(&sys::verbatim(&f).unwrap());
        let open = || {
            create_file(
                &p,
                GENERIC_READ,
                SHARE_ALL,
                OPEN_EXISTING,
                FILE_FLAG_SEQUENTIAL_SCAN | FILE_FLAG_OPEN_REPARSE_POINT,
            )
            .unwrap()
        };
        let no = ReadOpts {
            allow_hydrate: false,
        };
        let yes = ReadOpts {
            allow_hydrate: true,
        };
        let h = open();
        assert!(handle_gate(raw(&h), no).is_ok(), "a plain file passes");
        // SAFETY: `p` is NUL-terminated.
        let ok = unsafe { SetFileAttributesW(p.as_ptr(), FILE_ATTRIBUTE_OFFLINE) };
        assert_ne!(ok, 0);
        let e = handle_gate(raw(&h), no).unwrap_err();
        assert_eq!(
            (e.kind, e.call),
            (VfsErrorKind::CloudOnly, "read_for_hash"),
            "the attributes are read through the open handle, after the change"
        );
        assert!(
            handle_gate(raw(&h), yes).is_ok(),
            "`--allow-hydrate` reads it"
        );
        drop(h);
        let h = open();
        assert_eq!(
            handle_gate(raw(&h), no).unwrap_err().kind,
            VfsErrorKind::CloudOnly
        );
        drop(h);
        // SAFETY: `p` is NUL-terminated.
        let ok = unsafe { SetFileAttributesW(p.as_ptr(), FILE_ATTRIBUTE_NORMAL) };
        assert_ne!(ok, 0);
    }
}
