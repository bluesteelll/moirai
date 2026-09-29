//! `os::fs` on Windows: the `StoreFs` sub-trait of `Vfs` for [`OsVfs`] ([OS/fs]; X-F5).
//!
//! - **Roots** are open directory handles; every file operation resolves its `RelPath` relative to the root's handle
//!   with `NtCreateFile` (`OBJECT_ATTRIBUTES.RootDirectory`), never through a path string ([OS/fs §2.2, §5.1], P10). The
//!   calls that take path strings — `MoveFileExW`, `DeleteFileW`, `SetFileAttributesW`, `RemoveDirectoryW`,
//!   `GetDiskFreeSpaceExW` — use the root's canonical final path in `\\?\` form, captured at open ([OS/fs §2.2]).
//! - **Sharing** is always `FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE`; no open uses write-through or
//!   no-buffering ([OS/fs §5.1]).
//! - **Durability classes** ([OS/fs §4.4], Appendix A rows A8–A13): `durable` = `NtFlushBuffersFileEx(…,
//!   FLUSH_FLAGS_FILE_DATA_SYNC_ONLY)`, `durable+meta` = `FlushFileBuffers`, `durable-name` = `FlushFileBuffers` on a
//!   directory handle with write access, `sync_group` = each member by its class. A class the volume refuses is
//!   `Unsupported` and never falls back to another call (open point 4). Any failure is a [`DurabilityFailure`] for
//!   [`StoreFs::fail_stop`]: one stderr line, then `TerminateProcess(self, 7)` ([OS/fs §4.4.5]); a flush embedded in
//!   `create_root`, `swap_dirs` or `swap_recover` is returned instead as `VfsError` of kind `FlushFailed` ([OS/fs §4.1]).
//! - **Renames** carry `MOVEFILE_WRITE_THROUGH` and never replace unless asked ([OS/fs §4.8]); errors 5, 32 and 33 are
//!   retried per [`ShareRetry`] ([OS/fs §6.3]).
//! - **Counters** are process-wide relaxed atomics, one increment per operation ([OS/fs §4.13]).

#![allow(unsafe_code)]

use std::os::windows::io::OwnedHandle;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

use moirai_vfs::{
    Access, DirEntry, DurabilityClass, DurabilityFailure, EntryKind, EntryName, ExtentMethod,
    FileIdentity, FreeSpace, GroupMember, OpenHint, OsCode, OsTag, RelPath, RootAccess, RootRole,
    ShareRetry, StoreFs, StoreVolume, SwapOutcome, SwapRecovery, SyncKind, VfsCounters, VfsError,
    VfsErrorKind, VfsTypes,
};
use windows_sys::Wdk::Storage::FileSystem::{
    FILE_CREATE, FILE_DIRECTORY_FILE, FILE_NON_DIRECTORY_FILE, FILE_OPEN, FILE_SEQUENTIAL_ONLY,
    FILE_SYNCHRONOUS_IO_NONALERT, NtFlushBuffersFileEx,
};
use windows_sys::Win32::Foundation::{
    ERROR_NO_MORE_FILES, GENERIC_ALL, HANDLE, LocalFree, NTSTATUS, STATUS_INVALID_DEVICE_REQUEST,
    STATUS_INVALID_PARAMETER, STATUS_NOT_SUPPORTED,
};
use windows_sys::Win32::Security::Authorization::{
    EXPLICIT_ACCESS_W, GRANT_ACCESS, GetSecurityInfo, NO_MULTIPLE_TRUSTEE, SE_FILE_OBJECT,
    SetEntriesInAclW, SetSecurityInfo, TRUSTEE_IS_SID, TRUSTEE_IS_USER, TRUSTEE_W,
};
use windows_sys::Win32::Security::{
    ACL, DACL_SECURITY_INFORMATION, GetTokenInformation, PSECURITY_DESCRIPTOR,
    SUB_CONTAINERS_AND_OBJECTS_INHERIT, TOKEN_QUERY, TOKEN_USER, TokenUser,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateDirectoryW, DeleteFileW, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_NORMAL,
    FILE_ATTRIBUTE_READONLY, FILE_ATTRIBUTE_REPARSE_POINT, FILE_BASIC_INFO, FILE_END_OF_FILE_INFO,
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FULL_DIR_INFO, FILE_GENERIC_READ, FILE_GENERIC_WRITE,
    FILE_LIST_DIRECTORY, FILE_READ_ATTRIBUTES, FILE_TRAVERSE, FileBasicInfo, FileEndOfFileInfo,
    FileFullDirectoryInfo, FileFullDirectoryRestartInfo, FlushFileBuffers, GetDiskFreeSpaceExW,
    GetFileAttributesW, GetFileInformationByHandleEx, INVALID_FILE_ATTRIBUTES,
    MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW, OPEN_EXISTING, READ_CONTROL,
    RemoveDirectoryW, SYNCHRONIZE, SetFileAttributesW, SetFileInformationByHandle, WRITE_DAC,
};
use windows_sys::Win32::System::Console::{GetStdHandle, STD_ERROR_HANDLE};
use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;
use windows_sys::Win32::System::SystemServices::FLUSH_FLAGS_FILE_DATA_SYNC_ONLY;
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, OpenProcessToken, TerminateProcess,
};

use super::proc::mono_ns;
use super::sys::{
    self, Domain, NameCheck, NtOpen, SHARE_ALL, create_file, error, id_info, join_z, last,
    last_error, nt_code, nt_open, nt_success, raw, read_once, rel_wide, standard_info, verbatim,
    write_once,
};

/// The store-side seam for the running Windows process ([OS/README §2.2]): a zero-sized handle over the process-global
/// state of [OS/README §5.4] (the lock registry, the mapping registry, the fault handler, the counters). Every value
/// shares that state; constructing several changes nothing.
#[derive(Copy, Clone, Debug, Default)]
pub struct OsVfs;

impl OsVfs {
    /// The seam value.
    pub const fn new() -> OsVfs {
        OsVfs
    }
}

/// An open root directory ([OS/fs §2.2]). Cheap to clone: clones share one handle.
#[derive(Clone, Debug)]
pub struct OsRoot {
    pub(crate) inner: Arc<RootInner>,
}

/// What a root keeps with its handle ([OS/fs §4.1]).
#[derive(Debug)]
pub(crate) struct RootInner {
    /// `FILE_LIST_DIRECTORY | FILE_TRAVERSE | FILE_READ_ATTRIBUTES | SYNCHRONIZE`, full sharing ([OS/fs §5.1]).
    pub(crate) handle: OwnedHandle,
    pub(crate) role: RootRole,
    pub(crate) access: RootAccess,
    /// The canonical final path in `\\?\` form, without NUL or trailing separator (a drive root keeps its `\`).
    pub(crate) path: Box<[u16]>,
    /// The cached directory-flush handle of the root itself ([OS/fs §4.4.3]: "may cache").
    flush: OnceLock<OwnedHandle>,
}

impl OsRoot {
    /// The root's role.
    pub fn role(&self) -> RootRole {
        self.inner.role
    }

    /// The root's access.
    pub fn access(&self) -> RootAccess {
        self.inner.access
    }

    pub(crate) fn raw(&self) -> HANDLE {
        raw(&self.inner.handle)
    }

    pub(crate) fn path(&self) -> &[u16] {
        &self.inner.path
    }

    /// The `\\?\` path of `rel` under this root, NUL-terminated, after the store use-time checks.
    pub(crate) fn path_of(
        &self,
        rel: RelPath<'_>,
        call: &'static str,
    ) -> Result<Vec<u16>, VfsError> {
        Ok(join_z(
            &self.inner.path,
            &rel_wide(rel, NameCheck::Store, call)?,
        ))
    }

    fn writable(&self, call: &'static str) -> Result<(), VfsError> {
        match self.inner.access {
            RootAccess::ReadWrite => Ok(()),
            RootAccess::Read => Err(VfsError::new(VfsErrorKind::AccessDenied, OsCode(5), call)),
        }
    }
}

/// An open store file ([OS/fs §2.3]): positional only, synchronous, usable from several threads.
#[derive(Debug)]
pub struct OsFile {
    pub(crate) handle: OwnedHandle,
    pub(crate) writable: bool,
}

impl OsFile {
    pub(crate) fn raw(&self) -> HANDLE {
        raw(&self.handle)
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Counters ([OS/fs §2.7, §4.13])

pub(crate) struct Counters {
    pub(crate) opens: AtomicU64,
    pub(crate) creates: AtomicU64,
    pub(crate) bytes_read: AtomicU64,
    pub(crate) bytes_written: AtomicU64,
    pub(crate) sync_data: AtomicU64,
    pub(crate) sync_meta: AtomicU64,
    pub(crate) sync_dir: AtomicU64,
    pub(crate) renames: AtomicU64,
    pub(crate) unlinks: AtomicU64,
    pub(crate) maps: AtomicU64,
    pub(crate) mapped_bytes: AtomicU64,
    pub(crate) share_retries: AtomicU64,
}

pub(crate) static COUNTERS: Counters = Counters {
    opens: AtomicU64::new(0),
    creates: AtomicU64::new(0),
    bytes_read: AtomicU64::new(0),
    bytes_written: AtomicU64::new(0),
    sync_data: AtomicU64::new(0),
    sync_meta: AtomicU64::new(0),
    sync_dir: AtomicU64::new(0),
    renames: AtomicU64::new(0),
    unlinks: AtomicU64::new(0),
    maps: AtomicU64::new(0),
    mapped_bytes: AtomicU64::new(0),
    share_retries: AtomicU64::new(0),
};

pub(crate) fn bump(c: &AtomicU64, n: u64) {
    c.fetch_add(n, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------------------------------------------------
// Shared steps

/// The bounded retry of [OS/fs §6.3] around one attempt: errors 5, 32 and 33 are re-attempted after 1, 2, 4, 8, 16,
/// 32, then 64 ms per step until the monotonic time since the first attempt plus the next sleep would exceed the
/// bound; every re-attempt adds one to `share_retries`.
pub(crate) fn with_share_retry<T>(
    retry: ShareRetry,
    mut attempt: impl FnMut() -> Result<T, u32>,
) -> Result<T, u32> {
    let start = mono_ns();
    let mut retries = 0u32;
    loop {
        match attempt() {
            Ok(v) => return Ok(v),
            Err(e @ (5 | 32 | 33)) => {
                let elapsed_ms = mono_ns().saturating_sub(start) / 1_000_000;
                match retry.next_sleep_ms(retries, elapsed_ms) {
                    None => return Err(e),
                    Some(ms) => {
                        std::thread::sleep(std::time::Duration::from_millis(u64::from(ms)));
                        retries += 1;
                        bump(&COUNTERS.share_retries, 1);
                    }
                }
            }
            Err(e) => return Err(e),
        }
    }
}

/// `MoveFileExW(from, to, MOVEFILE_WRITE_THROUGH [| MOVEFILE_REPLACE_EXISTING])` with the bounded retry
/// ([OS/fs §4.8]); never `MOVEFILE_COPY_ALLOWED`.
pub(crate) fn move_file(
    from_z: &[u16],
    to_z: &[u16],
    replace: bool,
    retry: ShareRetry,
) -> Result<(), u32> {
    let flags = MOVEFILE_WRITE_THROUGH
        | if replace {
            MOVEFILE_REPLACE_EXISTING
        } else {
            0
        };
    with_share_retry(retry, || {
        // SAFETY: both arguments are NUL-terminated UTF-16 strings that outlive the call.
        let ok = unsafe { MoveFileExW(from_z.as_ptr(), to_z.as_ptr(), flags) };
        if ok != 0 { Ok(()) } else { Err(last_error()) }
    })
}

/// `FlushFileBuffers` of one handle.
pub(crate) fn flush(h: HANDLE) -> Result<(), u32> {
    // SAFETY: `h` is a valid handle owned by the caller for the duration of the call.
    let ok = unsafe { FlushFileBuffers(h) };
    if ok != 0 { Ok(()) } else { Err(last_error()) }
}

/// A `durable-name` failure.
fn name_failure(call: &'static str, code: u32) -> DurabilityFailure {
    DurabilityFailure {
        class: DurabilityClass::DurableName,
        call,
        kind: sys::kind_of(code, Domain::StoreData),
        os: OsCode(code as i32),
    }
}

/// The open of a directory-flush handle relative to a root ([OS/fs §5.1] row "directory-flush handle").
const FLUSH_DIR: NtOpen = NtOpen {
    access: FILE_GENERIC_READ | FILE_GENERIC_WRITE,
    disposition: FILE_OPEN,
    options: FILE_DIRECTORY_FILE | FILE_SYNCHRONOUS_IO_NONALERT,
    attributes: 0,
};

/// `durable-name` of a directory named by an absolute `\\?\` path (NUL-terminated): `FlushFileBuffers` on a handle
/// opened with `GENERIC_READ | GENERIC_WRITE` and `FILE_FLAG_BACKUP_SEMANTICS` ([OS/fs §4.4.3]); used by `create_root`
/// for the parent and by `swap_recover` for the parents the intent names.
pub(crate) fn flush_dir_path(path_z: &[u16]) -> Result<(), DurabilityFailure> {
    let h = create_file(
        path_z,
        FILE_GENERIC_READ | FILE_GENERIC_WRITE,
        SHARE_ALL,
        OPEN_EXISTING,
        FILE_FLAG_BACKUP_SEMANTICS,
    )
    .map_err(|e| name_failure("CreateFileW", e))?;
    bump(&COUNTERS.sync_dir, 1);
    flush(raw(&h)).map_err(|e| name_failure("FlushFileBuffers", e))
}

/// The parent of a `\\?\` path (no NUL, no trailing separator), NUL-terminated; `None` for a volume root.
pub(crate) fn parent_z(path: &[u16]) -> Option<Vec<u16>> {
    let bs = u16::from(b'\\');
    // A drive root `\\?\X:\` has no parent.
    if path.len() <= 7 && path.last() == Some(&bs) {
        return None;
    }
    let i = path.iter().rposition(|&u| u == bs)?;
    let head = &path[..i];
    if head.len() < 6 {
        return None;
    }
    let mut v = head.to_vec();
    if v.len() == 6 && v[5] == u16::from(b':') {
        v.push(bs);
    }
    v.push(0);
    Some(v)
}

// ---------------------------------------------------------------------------------------------------------------------
// The owner ACE of a store directory ([OS/fs §4.1], [90 §5.4])

/// A `LocalFree` guard for memory the security API allocates.
struct LocalMem(*mut core::ffi::c_void);

impl Drop for LocalMem {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: the pointer was allocated by the security API with `LocalAlloc` and is freed exactly once here.
            unsafe { LocalFree(self.0) };
        }
    }
}

/// The token user's SID of this process, as the bytes of a `TOKEN_USER` buffer (8-byte aligned).
pub(crate) fn token_user() -> Result<Vec<u64>, u32> {
    let mut token: HANDLE = core::ptr::null_mut();
    // SAFETY: `GetCurrentProcess` returns a pseudo-handle; `token` is a live local the call writes.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(last_error());
    }
    let token = sys::owned(token).ok_or(6u32)?;
    let mut len = 0u32;
    // SAFETY: a null buffer of length 0 asks for the size, which the call writes to `len`.
    unsafe { GetTokenInformation(raw(&token), TokenUser, core::ptr::null_mut(), 0, &mut len) };
    let mut buf = vec![0u64; (len as usize).div_ceil(8).max(1)];
    // SAFETY: `buf` is writable for at least `len` bytes and 8-byte aligned, as `TOKEN_USER` requires.
    let ok = unsafe {
        GetTokenInformation(
            raw(&token),
            TokenUser,
            buf.as_mut_ptr().cast(),
            (buf.len() * 8) as u32,
            &mut len,
        )
    };
    if ok == 0 {
        return Err(last_error());
    }
    Ok(buf)
}

/// Gives the directory `path_z` an explicit inheritable ACE granting the process's user full control (object and
/// container inherit): `GetSecurityInfo` + `SetEntriesInAclW(GRANT_ACCESS, GENERIC_ALL,
/// SUB_CONTAINERS_AND_OBJECTS_INHERIT, the token user's SID)` + `SetSecurityInfo(DACL_SECURITY_INFORMATION)`
/// ([OS/fs] Appendix A row A2).
fn grant_owner_ace(path_z: &[u16]) -> Result<(), VfsError> {
    let user = token_user().map_err(|e| error(e, Domain::Store, "GetTokenInformation"))?;
    // SAFETY: `user` holds a `TOKEN_USER` written by `GetTokenInformation`, 8-byte aligned; its SID pointer points into
    // the same buffer, which lives until the end of this function.
    let sid = unsafe { (*(user.as_ptr() as *const TOKEN_USER)).User.Sid };
    let h = create_file(
        path_z,
        READ_CONTROL | WRITE_DAC,
        SHARE_ALL,
        OPEN_EXISTING,
        FILE_FLAG_BACKUP_SEMANTICS,
    )
    .map_err(|e| error(e, Domain::Store, "CreateFileW"))?;
    let mut old: *mut ACL = core::ptr::null_mut();
    let mut sd: PSECURITY_DESCRIPTOR = core::ptr::null_mut();
    // SAFETY: `h` has `READ_CONTROL`; the out-pointers are live locals; owner, group and SACL are not requested.
    let rc = unsafe {
        GetSecurityInfo(
            raw(&h),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            core::ptr::null_mut(),
            core::ptr::null_mut(),
            &mut old,
            core::ptr::null_mut(),
            &mut sd,
        )
    };
    if rc != 0 {
        return Err(error(rc, Domain::Store, "GetSecurityInfo"));
    }
    let _sd = LocalMem(sd);
    let ea = EXPLICIT_ACCESS_W {
        grfAccessPermissions: GENERIC_ALL,
        grfAccessMode: GRANT_ACCESS,
        grfInheritance: SUB_CONTAINERS_AND_OBJECTS_INHERIT,
        Trustee: TRUSTEE_W {
            pMultipleTrustee: core::ptr::null_mut(),
            MultipleTrusteeOperation: NO_MULTIPLE_TRUSTEE,
            TrusteeForm: TRUSTEE_IS_SID,
            TrusteeType: TRUSTEE_IS_USER,
            ptstrName: sid.cast(),
        },
    };
    let mut new: *mut ACL = core::ptr::null_mut();
    // SAFETY: `ea` is one valid entry whose SID lives in `user`; `old` is the DACL inside `sd` (still allocated); `new`
    // is a live local.
    let rc = unsafe { SetEntriesInAclW(1, &ea, old, &mut new) };
    if rc != 0 {
        return Err(error(rc, Domain::Store, "SetEntriesInAclW"));
    }
    let _new = LocalMem(new.cast());
    // SAFETY: `h` has `WRITE_DAC`; `new` is a valid ACL allocated by `SetEntriesInAclW`; the other parts are null.
    let rc = unsafe {
        SetSecurityInfo(
            raw(&h),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            core::ptr::null_mut(),
            core::ptr::null_mut(),
            new,
            core::ptr::null(),
        )
    };
    if rc != 0 {
        return Err(error(rc, Domain::Store, "SetSecurityInfo"));
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------------
// Directory listing

/// An 8-byte-aligned buffer for the directory-information classes.
pub(crate) struct DirBuf(Vec<u64>);

impl DirBuf {
    /// A buffer of `bytes` bytes (rounded up to 8).
    pub(crate) fn new(bytes: usize) -> DirBuf {
        DirBuf(vec![0u64; bytes.div_ceil(8)])
    }

    /// The buffer's memory for an OS call to fill.
    pub(crate) fn as_mut_ptr(&mut self) -> *mut u8 {
        self.0.as_mut_ptr().cast()
    }

    pub(crate) fn bytes(&self) -> &[u8] {
        // SAFETY: a `[u64]` of `n` elements is `8n` initialized bytes with no padding; `u8` has no alignment need; the
        // borrow of `self` keeps the memory alive and unaliased by writers.
        unsafe { core::slice::from_raw_parts(self.0.as_ptr().cast(), self.0.len() * 8) }
    }

    /// Fills the buffer with the next batch of `class` from `h`; `false` at the end (`ERROR_NO_MORE_FILES`).
    pub(crate) fn fill(&mut self, h: HANDLE, class: i32) -> Result<bool, u32> {
        // SAFETY: the buffer is writable for `8 × len` bytes and 8-byte aligned, as the directory classes require.
        let ok = unsafe {
            GetFileInformationByHandleEx(
                h,
                class,
                self.0.as_mut_ptr().cast(),
                (self.0.len() * 8).min(u32::MAX as usize) as u32,
            )
        };
        if ok != 0 {
            Ok(true)
        } else {
            match last_error() {
                ERROR_NO_MORE_FILES => Ok(false),
                e => Err(e),
            }
        }
    }
}

pub(crate) fn u32_at(b: &[u8], at: usize) -> u32 {
    let mut w = [0u8; 4];
    w.copy_from_slice(&b[at..at + 4]);
    u32::from_le_bytes(w)
}

pub(crate) fn u64_at(b: &[u8], at: usize) -> u64 {
    let mut w = [0u8; 8];
    w.copy_from_slice(&b[at..at + 8]);
    u64::from_le_bytes(w)
}

/// The UTF-16 units of a name of `len` bytes at `at`.
pub(crate) fn units_at(b: &[u8], at: usize, len: usize) -> Vec<u16> {
    b[at..at + len]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c))
        .collect()
}

/// `true` for the names `.` and `..`, given as the UTF-16LE bytes of a directory record's name.
pub(crate) fn is_dot_name(name: &[u8]) -> bool {
    name == b".\0" || name == b".\0.\0"
}

/// Replaces `out` with the WTF-8 of the UTF-16LE name of `len` bytes at `at` in the record `rec` ([OS/path §2.4]),
/// decoding straight from the record: with `out` reused across records, an entry costs only the allocation of its
/// `EntryName`.
pub(crate) fn record_name(out: &mut Vec<u8>, rec: &[u8], at: usize, len: usize) {
    out.clear();
    let units = rec[at..at + len]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c));
    sys::push_wtf8_units(out, units);
}

/// Walks the records of one filled buffer: calls `each(record)` for every record (a slice starting at the record).
pub(crate) fn records(buf: &[u8], mut each: impl FnMut(&[u8]) -> bool) {
    let mut at = 0usize;
    loop {
        let rec = &buf[at..];
        if !each(rec) {
            return;
        }
        let next = u32_at(rec, 0) as usize;
        if next == 0 {
            return;
        }
        at += next;
    }
}

/// Kind of a store directory entry from its attributes ([OS/fs §2.7]): a reparse point (a link, a junction) is `Other`.
fn entry_kind(attrs: u32) -> EntryKind {
    if attrs & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        EntryKind::Other
    } else if attrs & FILE_ATTRIBUTE_DIRECTORY != 0 {
        EntryKind::Dir
    } else {
        EntryKind::File
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// fail_stop ([OS/fs §4.4.5])

/// A fixed stack buffer for one output line, written without allocating.
pub(crate) struct LineBuf {
    buf: [u8; 512],
    len: usize,
}

impl LineBuf {
    pub(crate) const fn new() -> LineBuf {
        LineBuf {
            buf: [0; 512],
            len: 0,
        }
    }

    /// The line written so far followed by one LF (the byte `write_str` always keeps free).
    pub(crate) fn finish(&mut self) -> &[u8] {
        self.buf[self.len] = b'\n';
        &self.buf[..=self.len]
    }
}

impl core::fmt::Write for LineBuf {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        // Keep one byte for the final LF; a longer text is cut, never the LF.
        let room = self.buf.len() - 1 - self.len;
        let n = s.len().min(room);
        self.buf[self.len..self.len + n].copy_from_slice(&s.as_bytes()[..n]);
        self.len += n;
        Ok(())
    }
}

/// Writes `line` (with its LF) to stderr with one `WriteFile`, then ends the process with `code` through
/// `TerminateProcess(GetCurrentProcess(), code)`: no destructors, no `atexit` handlers ([OS/fs §4.4.5], [OS/map §8.2]).
pub(crate) fn write_and_terminate(line: &[u8], code: u32) -> ! {
    // SAFETY: `GetStdHandle` has no preconditions; the handle may be invalid or null, which `WriteFile` then reports as
    // a failure that is ignored (the exit code still tells the parent).
    unsafe {
        let err = GetStdHandle(STD_ERROR_HANDLE);
        let mut n = 0u32;
        windows_sys::Win32::Storage::FileSystem::WriteFile(
            err,
            line.as_ptr(),
            line.len() as u32,
            &mut n,
            core::ptr::null_mut(),
        );
    }
    // SAFETY: terminating the current process through its pseudo-handle has no preconditions; the call does not return.
    unsafe { TerminateProcess(GetCurrentProcess(), code) };
    std::process::abort()
}

// ---------------------------------------------------------------------------------------------------------------------
// The trait

impl VfsTypes for OsVfs {
    type Root = OsRoot;
    type File = OsFile;
}

/// Open parameters of store files ([OS/fs §5.1]).
fn file_open(access: Access, hint: OpenHint) -> NtOpen {
    let rw = match access {
        Access::Read => 0,
        Access::ReadWrite => FILE_GENERIC_WRITE,
    };
    let seq = match hint {
        OpenHint::Normal => 0,
        OpenHint::Sequential => FILE_SEQUENTIAL_ONLY,
    };
    NtOpen {
        access: FILE_GENERIC_READ | SYNCHRONIZE | rw,
        disposition: FILE_OPEN,
        options: FILE_NON_DIRECTORY_FILE | FILE_SYNCHRONOUS_IO_NONALERT | seq,
        attributes: 0,
    }
}

fn identity_of(h: HANDLE, call: &'static str) -> Result<FileIdentity, VfsError> {
    let info = id_info(h).map_err(|e| error(e, Domain::Store, call))?;
    Ok(FileIdentity {
        volume: info.VolumeSerialNumber,
        file: info.FileId.Identifier,
    })
}

/// Zeros over `[0, len)` of `file` from one reused zero buffer of at most 256 KiB ([OS/fs §4.5] `ZeroFill`: "at most
/// 1 MiB"; the smaller buffer bounds the transient memory).
fn zero_fill(v: &OsVfs, file: &OsFile, len: u64) -> Result<(), VfsError> {
    const CHUNK: u64 = 256 * 1024;
    let zeros = vec![0u8; len.min(CHUNK) as usize];
    let mut at = 0u64;
    while at < len {
        let n = (len - at).min(CHUNK) as usize;
        v.write_at(file, at, &zeros[..n])?;
        at += n as u64;
    }
    Ok(())
}

impl StoreFs for OsVfs {
    fn open_root(
        &self,
        dir: &Path,
        role: RootRole,
        access: RootAccess,
    ) -> Result<OsRoot, VfsError> {
        if !dir.is_absolute() {
            return Err(VfsError::new(
                VfsErrorKind::InvalidName,
                OsCode::NONE,
                "open_root",
            ));
        }
        let path = verbatim(dir)
            .ok_or_else(|| VfsError::new(VfsErrorKind::InvalidName, OsCode::NONE, "open_root"))?;
        let h = create_file(
            &sys::with_nul(&path),
            FILE_LIST_DIRECTORY | FILE_TRAVERSE | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            SHARE_ALL,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
        )
        .map_err(|e| error(e, Domain::Store, "CreateFileW"))?;
        let std = standard_info(raw(&h))
            .map_err(|e| error(e, Domain::Store, "GetFileInformationByHandleEx"))?;
        if !std.Directory {
            return Err(VfsError::new(
                VfsErrorKind::NotFound,
                OsCode(267),
                "open_root",
            ));
        }
        let fin = sys::final_path(raw(&h))
            .map_err(|e| error(e, Domain::Store, "GetFinalPathNameByHandleW"))?;
        Ok(OsRoot {
            inner: Arc::new(RootInner {
                handle: h,
                role,
                access,
                path: fin.into_boxed_slice(),
                flush: OnceLock::new(),
            }),
        })
    }

    fn create_root(&self, dir: &Path, role: RootRole) -> Result<OsRoot, VfsError> {
        if !dir.is_absolute() {
            return Err(VfsError::new(
                VfsErrorKind::InvalidName,
                OsCode::NONE,
                "create_root",
            ));
        }
        let path = verbatim(dir)
            .ok_or_else(|| VfsError::new(VfsErrorKind::InvalidName, OsCode::NONE, "create_root"))?;
        let path_z = sys::with_nul(&path);
        // SAFETY: `path_z` is NUL-terminated and outlives the call; no security attributes (the default DACL).
        if unsafe { CreateDirectoryW(path_z.as_ptr(), core::ptr::null()) } == 0 {
            return Err(last(Domain::Store, "CreateDirectoryW"));
        }
        bump(&COUNTERS.creates, 1);
        // The clean-up removal of the new, empty directory: one `RemoveDirectoryW`, counted in `unlinks` when it
        // succeeds as `remove_dir` counts it ([OS/fs §4.13]: every operation counts each OS call it issues; the
        // simulator's clean-up counts the same). Its own error is ignored.
        let remove_new = || {
            // SAFETY: `path_z` is NUL-terminated and outlives the call.
            if unsafe { RemoveDirectoryW(path_z.as_ptr()) } != 0 {
                bump(&COUNTERS.unlinks, 1);
            }
        };
        if role == RootRole::Store
            && let Err(e) = grant_owner_ace(&path_z)
        {
            // The directory is ours and empty: remove it, so a retry does not meet `AlreadyExists`.
            remove_new();
            return Err(e);
        }
        // `durable-name` on the parent: the new directory survives a power loss before the first acknowledgement.
        //
        // A failure of that flush ([OS/fs §4.1], spec sync 2a) removes the new, empty directory (the removal's own error
        // ignored), so a later run does not meet `AlreadyExists`, and is `FlushFailed` with the flush's code and call:
        // the caller (`init`, `restore`) exits 7 with the `durability-failure` text and issues no further write, flush,
        // create or namespace call. No commit is at stake yet, so it is not a `DurabilityFailure` for `fail_stop`.
        if let Some(parent) = parent_z(&path)
            && let Err(f) = flush_dir_path(&parent)
        {
            remove_new();
            return Err(f.embedded());
        }
        self.open_root(dir, role, RootAccess::ReadWrite)
    }

    fn open(
        &self,
        root: &OsRoot,
        rel: RelPath<'_>,
        access: Access,
        hint: OpenHint,
    ) -> Result<OsFile, VfsError> {
        if access == Access::ReadWrite {
            root.writable("NtCreateFile")?;
        }
        let name = rel_wide(rel, NameCheck::Store, "NtCreateFile")?;
        let h = nt_open(root.raw(), &name, file_open(access, hint), Domain::Store)?;
        bump(&COUNTERS.opens, 1);
        Ok(OsFile {
            handle: h,
            writable: access == Access::ReadWrite,
        })
    }

    fn create_new(&self, root: &OsRoot, rel: RelPath<'_>) -> Result<OsFile, VfsError> {
        root.writable("NtCreateFile")?;
        let name = rel_wide(rel, NameCheck::Store, "NtCreateFile")?;
        let o = NtOpen {
            disposition: FILE_CREATE,
            attributes: FILE_ATTRIBUTE_NORMAL,
            ..file_open(Access::ReadWrite, OpenHint::Normal)
        };
        let h = nt_open(root.raw(), &name, o, Domain::Store)?;
        bump(&COUNTERS.creates, 1);
        Ok(OsFile {
            handle: h,
            writable: true,
        })
    }

    fn create_dir(&self, root: &OsRoot, rel: RelPath<'_>) -> Result<(), VfsError> {
        root.writable("NtCreateFile")?;
        let name = rel_wide(rel, NameCheck::Store, "NtCreateFile")?;
        let o = NtOpen {
            access: FILE_LIST_DIRECTORY | SYNCHRONIZE,
            disposition: FILE_CREATE,
            options: FILE_DIRECTORY_FILE | FILE_SYNCHRONOUS_IO_NONALERT,
            attributes: 0,
        };
        drop(nt_open(root.raw(), &name, o, Domain::Store)?);
        bump(&COUNTERS.creates, 1);
        Ok(())
    }

    fn remove_dir(&self, root: &OsRoot, rel: RelPath<'_>) -> Result<(), VfsError> {
        root.writable("RemoveDirectoryW")?;
        let p = root.path_of(rel, "RemoveDirectoryW")?;
        // SAFETY: `p` is NUL-terminated and outlives the call.
        if unsafe { RemoveDirectoryW(p.as_ptr()) } == 0 {
            return Err(last(Domain::Store, "RemoveDirectoryW"));
        }
        bump(&COUNTERS.unlinks, 1);
        Ok(())
    }

    fn list_dir(&self, root: &OsRoot, dir: Option<RelPath<'_>>) -> Result<Vec<DirEntry>, VfsError> {
        let name = rel_wide(
            dir.unwrap_or(RelPath::ROOT),
            NameCheck::Store,
            "NtCreateFile",
        )?;
        let o = NtOpen {
            access: FILE_LIST_DIRECTORY | SYNCHRONIZE,
            disposition: FILE_OPEN,
            options: FILE_DIRECTORY_FILE | FILE_SYNCHRONOUS_IO_NONALERT,
            attributes: 0,
        };
        let h = nt_open(root.raw(), &name, o, Domain::Store)?;
        let mut buf = DirBuf::new(64 * 1024);
        let mut out = Vec::new();
        let name_at = core::mem::offset_of!(FILE_FULL_DIR_INFO, FileName);
        let attrs_at = core::mem::offset_of!(FILE_FULL_DIR_INFO, FileAttributes);
        let len_at = core::mem::offset_of!(FILE_FULL_DIR_INFO, FileNameLength);
        let mut class = FileFullDirectoryRestartInfo;
        let mut name = Vec::with_capacity(64);
        while buf
            .fill(raw(&h), class)
            .map_err(|e| error(e, Domain::Store, "GetFileInformationByHandleEx"))?
        {
            class = FileFullDirectoryInfo;
            records(buf.bytes(), |rec| {
                let len = u32_at(rec, len_at) as usize;
                if !is_dot_name(&rec[name_at..name_at + len]) {
                    record_name(&mut name, rec, name_at, len);
                    out.push(DirEntry {
                        name: EntryName::from_os_bytes(&name),
                        kind: entry_kind(u32_at(rec, attrs_at)),
                    });
                }
                true
            });
        }
        Ok(out)
    }

    fn read_at(&self, file: &OsFile, offset: u64, buf: &mut [u8]) -> Result<usize, VfsError> {
        let mut done = 0usize;
        while done < buf.len() {
            let n = read_once(file.raw(), offset + done as u64, &mut buf[done..])
                .map_err(|e| error(e, Domain::StoreData, "ReadFile"))?;
            if n == 0 {
                break;
            }
            done += n;
        }
        bump(&COUNTERS.bytes_read, done as u64);
        Ok(done)
    }

    fn read_exact_at(&self, file: &OsFile, offset: u64, buf: &mut [u8]) -> Result<(), VfsError> {
        let n = self.read_at(file, offset, buf)?;
        if n < buf.len() {
            return Err(VfsError::new(
                VfsErrorKind::UnexpectedEof,
                OsCode::NONE,
                "ReadFile",
            ));
        }
        Ok(())
    }

    fn write_at(&self, file: &OsFile, offset: u64, buf: &[u8]) -> Result<(), VfsError> {
        let mut done = 0usize;
        while done < buf.len() {
            let n = write_once(file.raw(), offset + done as u64, &buf[done..])
                .map_err(|e| error(e, Domain::StoreData, "WriteFile"))?;
            if n == 0 {
                return Err(VfsError::new(VfsErrorKind::Io, OsCode::NONE, "WriteFile"));
            }
            done += n;
            bump(&COUNTERS.bytes_written, n as u64);
        }
        Ok(())
    }

    fn sync(&self, file: &OsFile, kind: SyncKind) -> Result<(), DurabilityFailure> {
        assert!(
            file.writable,
            "os::fs: sync on a handle without write access ([OS/fs §4.4.2])"
        );
        match kind {
            SyncKind::Data => {
                bump(&COUNTERS.sync_data, 1);
                let mut iosb = IO_STATUS_BLOCK::default();
                // SAFETY: the handle is valid for the call; no parameters (null, 0); `iosb` is a live local.
                let status = unsafe {
                    NtFlushBuffersFileEx(
                        file.raw(),
                        FLUSH_FLAGS_FILE_DATA_SYNC_ONLY,
                        core::ptr::null(),
                        0,
                        &mut iosb,
                    )
                };
                if nt_success(status) {
                    Ok(())
                } else {
                    Err(data_sync_failure(status))
                }
            }
            SyncKind::DataAndMeta => {
                bump(&COUNTERS.sync_meta, 1);
                flush(file.raw()).map_err(|e| DurabilityFailure {
                    class: DurabilityClass::DurableMeta,
                    call: "FlushFileBuffers",
                    kind: sys::kind_of(e, Domain::StoreData),
                    os: OsCode(e as i32),
                })
            }
        }
    }

    fn sync_dir(&self, root: &OsRoot, dir: Option<RelPath<'_>>) -> Result<(), DurabilityFailure> {
        // A root opened `Read` cannot flush a directory: a durability failure `AccessDenied` (code 5, `call`
        // `FlushFileBuffers`), issued without an OS call ([OS/fs §4.4.3]).
        if root.inner.access == RootAccess::Read {
            return Err(name_failure("FlushFileBuffers", 5));
        }
        let dir = dir.unwrap_or(RelPath::ROOT);
        bump(&COUNTERS.sync_dir, 1);
        if dir.is_root() {
            let h = match root.inner.flush.get() {
                Some(h) => h,
                None => {
                    let h =
                        nt_open(root.raw(), &[], FLUSH_DIR, Domain::StoreData).map_err(|e| {
                            DurabilityFailure {
                                class: DurabilityClass::DurableName,
                                call: e.call,
                                kind: e.kind,
                                os: e.os,
                            }
                        })?;
                    // A racing thread may have cached one first; either handle serves.
                    let _ = root.inner.flush.set(h);
                    root.inner
                        .flush
                        .get()
                        .expect("os::fs: the flush handle was just cached")
                }
            };
            flush(raw(h)).map_err(|e| name_failure("FlushFileBuffers", e))
        } else {
            let name =
                rel_wide(dir, NameCheck::Store, "NtCreateFile").map_err(|e| DurabilityFailure {
                    class: DurabilityClass::DurableName,
                    call: e.call,
                    kind: e.kind,
                    os: e.os,
                })?;
            let h = nt_open(root.raw(), &name, FLUSH_DIR, Domain::StoreData).map_err(|e| {
                DurabilityFailure {
                    class: DurabilityClass::DurableName,
                    call: e.call,
                    kind: e.kind,
                    os: e.os,
                }
            })?;
            flush(raw(&h)).map_err(|e| name_failure("FlushFileBuffers", e))
        }
    }

    fn sync_group(
        &self,
        members: &[GroupMember<'_, OsRoot, OsFile>],
    ) -> Result<(), DurabilityFailure> {
        // Windows: each member by its class, in the given order ([OS/fs §4.4.4]).
        for m in members {
            match *m {
                GroupMember::File { file, kind } => self.sync(file, kind)?,
                GroupMember::Dir { root, dir } => self.sync_dir(root, dir)?,
            }
        }
        Ok(())
    }

    fn fail_stop(&self, failure: DurabilityFailure) -> ! {
        use core::fmt::Write as _;
        let mut line = LineBuf::new();
        let _ = write!(line, "{}", failure.stderr_line(OsTag::Windows));
        write_and_terminate(line.finish(), 7)
    }

    fn create_extent(
        &self,
        root: &OsRoot,
        rel: RelPath<'_>,
        len: u64,
        vol: &StoreVolume,
    ) -> Result<OsFile, VfsError> {
        match vol.extent_method {
            // NTFS writes zeros; `WriteZeroes` has no Windows call and takes its own fallback, the same steps.
            ExtentMethod::ZeroFill | ExtentMethod::WriteZeroes => {
                let f = self.create_new(root, rel)?;
                zero_fill(self, &f, len)?;
                Ok(f)
            }
            ExtentMethod::Sparse => {
                let free = StoreFs::free_space(self, root)?;
                if free.available < len.saturating_mul(2) {
                    return Err(VfsError::new(
                        VfsErrorKind::InsufficientSpace,
                        OsCode::NONE,
                        "create_extent",
                    ));
                }
                let f = self.create_new(root, rel)?;
                set_len(&f, len)?;
                Ok(f)
            }
        }
    }

    fn recycle_extent(&self, file: &OsFile, len: u64, _vol: &StoreVolume) -> Result<(), VfsError> {
        // Windows has no hole-punching or write-zeroes call on an allowed volume: every method writes zeros, which
        // extends a shorter file. A longer file is cut to `len` (`FileEndOfFileInfo`), so afterwards the file is exactly
        // `len` bytes and reads as zero, whatever its length before ([OS/fs §4.5], pass 1, S1-24).
        zero_fill(self, file, len)?;
        if self.file_size(file)? > len {
            set_len(file, len)?;
        }
        Ok(())
    }

    fn seal(&self, file: &OsFile) -> Result<(), VfsError> {
        let mut info = sys::file_info(file.raw(), FileBasicInfo, FILE_BASIC_INFO::default())
            .map_err(|e| error(e, Domain::Store, "GetFileInformationByHandleEx"))?;
        info.CreationTime = 0;
        info.LastAccessTime = 0;
        info.LastWriteTime = 0;
        info.ChangeTime = 0;
        info.FileAttributes =
            (info.FileAttributes & !FILE_ATTRIBUTE_NORMAL) | FILE_ATTRIBUTE_READONLY;
        set_info(file.raw(), FileBasicInfo, &info)
            .map_err(|e| error(e, Domain::Store, "SetFileInformationByHandle"))
    }

    fn unlink(&self, root: &OsRoot, rel: RelPath<'_>, retry: ShareRetry) -> Result<(), VfsError> {
        root.writable("DeleteFileW")?;
        let p = root.path_of(rel, "DeleteFileW")?;
        let mut call = "GetFileAttributesW";
        with_share_retry(retry, || {
            call = "GetFileAttributesW";
            // SAFETY: `p` is NUL-terminated and outlives the call.
            let attrs = unsafe { GetFileAttributesW(p.as_ptr()) };
            if attrs == INVALID_FILE_ATTRIBUTES {
                return Err(last_error());
            }
            if attrs & FILE_ATTRIBUTE_READONLY != 0 {
                call = "SetFileAttributesW";
                let cleared = match attrs & !FILE_ATTRIBUTE_READONLY {
                    0 => FILE_ATTRIBUTE_NORMAL,
                    a => a,
                };
                // SAFETY: `p` is NUL-terminated and outlives the call.
                if unsafe { SetFileAttributesW(p.as_ptr(), cleared) } == 0 {
                    return Err(last_error());
                }
            }
            call = "DeleteFileW";
            // SAFETY: `p` is NUL-terminated and outlives the call.
            if unsafe { DeleteFileW(p.as_ptr()) } == 0 {
                return Err(last_error());
            }
            Ok(())
        })
        .map_err(|e| error(e, Domain::Store, call))?;
        bump(&COUNTERS.unlinks, 1);
        Ok(())
    }

    fn rename_noreplace(
        &self,
        from_root: &OsRoot,
        from: RelPath<'_>,
        to_root: &OsRoot,
        to: RelPath<'_>,
        retry: ShareRetry,
    ) -> Result<(), VfsError> {
        rename(from_root, from, to_root, to, false, retry)
    }

    fn rename_replace(
        &self,
        from_root: &OsRoot,
        from: RelPath<'_>,
        to_root: &OsRoot,
        to: RelPath<'_>,
        retry: ShareRetry,
    ) -> Result<(), VfsError> {
        rename(from_root, from, to_root, to, true, retry)
    }

    fn swap_dirs(
        &self,
        a_parent: &OsRoot,
        a: RelPath<'_>,
        b_parent: &OsRoot,
        b: RelPath<'_>,
        retry: ShareRetry,
    ) -> Result<SwapOutcome, VfsError> {
        super::swap::swap_dirs(self, a_parent, a, b_parent, b, retry)
    }

    fn swap_recover(
        &self,
        a_parent: &OsRoot,
        a: RelPath<'_>,
        retry: ShareRetry,
    ) -> Result<SwapRecovery, VfsError> {
        super::swap::swap_recover(self, a_parent, a, retry)
    }

    fn file_size(&self, file: &OsFile) -> Result<u64, VfsError> {
        let info = standard_info(file.raw())
            .map_err(|e| error(e, Domain::Store, "GetFileInformationByHandleEx"))?;
        Ok(info.EndOfFile as u64)
    }

    fn identity(&self, file: &OsFile) -> Result<FileIdentity, VfsError> {
        identity_of(file.raw(), "GetFileInformationByHandleEx")
    }

    fn root_identity(&self, root: &OsRoot) -> Result<FileIdentity, VfsError> {
        identity_of(root.raw(), "GetFileInformationByHandleEx")
    }

    fn path_identity(&self, root: &OsRoot, rel: RelPath<'_>) -> Result<FileIdentity, VfsError> {
        let name = rel_wide(rel, NameCheck::Store, "NtCreateFile")?;
        let o = NtOpen {
            access: FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            disposition: FILE_OPEN,
            options: FILE_SYNCHRONOUS_IO_NONALERT,
            attributes: 0,
        };
        let h = nt_open(root.raw(), &name, o, Domain::Store)?;
        identity_of(raw(&h), "GetFileInformationByHandleEx")
    }

    fn free_space(&self, root: &OsRoot) -> Result<FreeSpace, VfsError> {
        free_space_of(root.path())
    }

    fn advise_dontneed(&self, _file: &OsFile, _offset: u64, _len: u64) {
        // Windows: a no-op; `FILE_SEQUENTIAL_ONLY` at open is the whole bulk-pass hint ([OS/fs §4.12]).
    }

    fn counters(&self) -> VfsCounters {
        let r = |c: &AtomicU64| c.load(Ordering::Relaxed);
        VfsCounters {
            opens: r(&COUNTERS.opens),
            creates: r(&COUNTERS.creates),
            bytes_read: r(&COUNTERS.bytes_read),
            bytes_written: r(&COUNTERS.bytes_written),
            sync_data: r(&COUNTERS.sync_data),
            sync_meta: r(&COUNTERS.sync_meta),
            sync_dir: r(&COUNTERS.sync_dir),
            full_barriers: 0,
            renames: r(&COUNTERS.renames),
            unlinks: r(&COUNTERS.unlinks),
            maps: r(&COUNTERS.maps),
            mapped_bytes: r(&COUNTERS.mapped_bytes),
            share_retries: r(&COUNTERS.share_retries),
        }
    }
}

/// The failure of a `durable` flush: `STATUS_INVALID_PARAMETER`, `STATUS_NOT_SUPPORTED` and
/// `STATUS_INVALID_DEVICE_REQUEST` are `Unsupported` (the store is refused, [OS/fs §4.4.2]); every other status maps
/// through its Win32 code.
fn data_sync_failure(status: NTSTATUS) -> DurabilityFailure {
    let code = nt_code(status);
    let kind = match status {
        STATUS_INVALID_PARAMETER | STATUS_NOT_SUPPORTED | STATUS_INVALID_DEVICE_REQUEST => {
            VfsErrorKind::Unsupported
        }
        _ => sys::kind_of(code, Domain::StoreData),
    };
    DurabilityFailure {
        class: DurabilityClass::Durable,
        call: "NtFlushBuffersFileEx",
        kind,
        os: OsCode(code as i32),
    }
}

/// `SetFileInformationByHandle` of a fixed-size class.
fn set_info<T>(h: HANDLE, class: i32, v: &T) -> Result<(), u32> {
    // SAFETY: `v` is a live `T` of `size_of::<T>()` bytes, the buffer the class reads.
    let ok = unsafe {
        SetFileInformationByHandle(
            h,
            class,
            (v as *const T).cast(),
            core::mem::size_of::<T>() as u32,
        )
    };
    if ok != 0 { Ok(()) } else { Err(last_error()) }
}

/// Sets the end of file ([OS/fs §4.5] `Sparse`: the extent's length, reading as zero beyond the valid data).
fn set_len(f: &OsFile, len: u64) -> Result<(), VfsError> {
    let info = FILE_END_OF_FILE_INFO {
        EndOfFile: len as i64,
    };
    set_info(f.raw(), FileEndOfFileInfo, &info)
        .map_err(|e| error(e, Domain::StoreData, "SetFileInformationByHandle"))
}

/// Free and total bytes of the volume holding the `\\?\` directory `path` (no NUL): `GetDiskFreeSpaceExW`
/// ([OS/fs §4.11]).
pub(crate) fn free_space_of(path: &[u16]) -> Result<FreeSpace, VfsError> {
    let mut p = path.to_vec();
    if p.last() != Some(&u16::from(b'\\')) {
        p.push(u16::from(b'\\'));
    }
    p.push(0);
    let (mut avail, mut total) = (0u64, 0u64);
    // SAFETY: `p` is NUL-terminated and outlives the call; the two out-pointers are live locals; the third is null.
    let ok =
        unsafe { GetDiskFreeSpaceExW(p.as_ptr(), &mut avail, &mut total, core::ptr::null_mut()) };
    if ok == 0 {
        return Err(last(Domain::Store, "GetDiskFreeSpaceExW"));
    }
    Ok(FreeSpace {
        available: avail,
        total,
    })
}

/// A rename of a store file between two roots ([OS/fs §4.8]); both roots must be writable.
fn rename(
    from_root: &OsRoot,
    from: RelPath<'_>,
    to_root: &OsRoot,
    to: RelPath<'_>,
    replace: bool,
    retry: ShareRetry,
) -> Result<(), VfsError> {
    from_root.writable("MoveFileExW")?;
    to_root.writable("MoveFileExW")?;
    let f = from_root.path_of(from, "MoveFileExW")?;
    let t = to_root.path_of(to, "MoveFileExW")?;
    move_file(&f, &t, replace, retry).map_err(|e| error(e, Domain::Store, "MoveFileExW"))?;
    bump(&COUNTERS.renames, 1);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parents_of_verbatim_paths() {
        let w = |s: &str| s.encode_utf16().collect::<Vec<u16>>();
        let s = |v: Option<Vec<u16>>| v.map(|v| String::from_utf16(&v[..v.len() - 1]).unwrap());
        assert_eq!(
            s(parent_z(&w("\\\\?\\D:\\a\\b"))).as_deref(),
            Some("\\\\?\\D:\\a")
        );
        assert_eq!(
            s(parent_z(&w("\\\\?\\D:\\a"))).as_deref(),
            Some("\\\\?\\D:\\")
        );
        assert_eq!(s(parent_z(&w("\\\\?\\D:\\"))), None);
        assert_eq!(
            s(parent_z(&w("\\\\?\\UNC\\srv\\share\\x"))).as_deref(),
            Some("\\\\?\\UNC\\srv\\share")
        );
    }

    #[test]
    fn line_buffer_keeps_room_for_the_lf() {
        use core::fmt::Write as _;
        let mut l = LineBuf::new();
        let long = "x".repeat(600);
        l.write_str(&long).unwrap();
        let line = l.finish();
        assert_eq!(line.len(), 512);
        assert_eq!(
            line.last(),
            Some(&b'\n'),
            "a long text is cut, never the LF"
        );
        let mut m = LineBuf::new();
        write!(m, "a{}b", 5).unwrap();
        assert_eq!(m.finish(), b"a5b\n");
    }

    #[test]
    fn entry_kinds() {
        assert_eq!(entry_kind(FILE_ATTRIBUTE_NORMAL), EntryKind::File);
        assert_eq!(entry_kind(FILE_ATTRIBUTE_DIRECTORY), EntryKind::Dir);
        assert_eq!(
            entry_kind(FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT),
            EntryKind::Other
        );
    }
}
