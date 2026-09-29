//! Win32 helpers shared by every Windows module: owned handles, wide strings and WTF-8, the `\\?\` path forms, final
//! paths, the mapping of OS codes to [`VfsErrorKind`] ([OS/fs §6.2], [OS/project §2.3]), handle-relative `NtCreateFile`
//! opens ([OS/fs §5.1]) and positional I/O on synchronous handles ([OS/fs §4.3]).
//!
//! Every handle this crate opens is non-inheritable ([OS/README §5.2] item 1): `CreateFileW` gets no security
//! attributes and `NtCreateFile` no `OBJ_INHERIT`.

#![allow(unsafe_code)]

use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};

use moirai_vfs::{OsCode, RelPath, VfsError, VfsErrorKind};
use windows_sys::Wdk::Foundation::OBJECT_ATTRIBUTES;
use windows_sys::Wdk::Storage::FileSystem::NtCreateFile;
use windows_sys::Win32::Foundation::{
    ERROR_HANDLE_EOF, GetLastError, HANDLE, INVALID_HANDLE_VALUE, NTSTATUS, OBJ_CASE_INSENSITIVE,
    RtlNtStatusToDosError, STATUS_DELETE_PENDING, UNICODE_STRING,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_ID_INFO, FILE_SHARE_DELETE, FILE_SHARE_READ,
    FILE_SHARE_WRITE, FILE_STANDARD_INFO, FileIdInfo, FileStandardInfo,
    GetFileInformationByHandleEx, GetFinalPathNameByHandleW, OPEN_EXISTING, ReadFile, WriteFile,
};
use windows_sys::Win32::System::IO::{IO_STATUS_BLOCK, OVERLAPPED, OVERLAPPED_0, OVERLAPPED_0_0};

/// Every share mode moirai uses: `FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE` ([OS/fs §5.1],
/// [OS/project §2.2]).
pub(crate) const SHARE_ALL: u32 = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;

/// The last Win32 error of this thread.
pub(crate) fn last_error() -> u32 {
    // SAFETY: `GetLastError` reads the calling thread's last-error value; it has no preconditions.
    unsafe { GetLastError() }
}

/// Takes ownership of a handle returned by a Win32 call; `None` for `NULL` and `INVALID_HANDLE_VALUE`.
pub(crate) fn owned(h: HANDLE) -> Option<OwnedHandle> {
    if h.is_null() || h == INVALID_HANDLE_VALUE {
        None
    } else {
        // SAFETY: `h` is a valid handle that the caller just received from the OS and owns exclusively; the
        // `OwnedHandle` closes it exactly once.
        Some(unsafe { OwnedHandle::from_raw_handle(h) })
    }
}

/// The raw value of an owned handle, for FFI calls.
pub(crate) fn raw(h: &OwnedHandle) -> HANDLE {
    h.as_raw_handle()
}

/// The domain of an OS call, which decides how an unlisted code maps ([OS/fs §6.2] rows `Io` and `Other`,
/// [OS/project §2.3]).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum Domain {
    /// A store operation other than a read or write: unlisted codes are `Other`.
    Store,
    /// A store read or write: unlisted codes are `Io` (fault-model item (12)).
    StoreData,
    /// A project-file operation: adds `IsDirectory` (267), the `ERROR_CLOUD_FILE_*` family and 1920.
    Project,
    /// A project-file read: as `Project`, unlisted codes `Io`.
    ProjectData,
}

/// The kind a Win32 error code maps to ([OS/fs §6.2], [OS/project §2.3]).
pub(crate) fn kind_of(code: u32, domain: Domain) -> VfsErrorKind {
    let project = matches!(domain, Domain::Project | Domain::ProjectData);
    match code {
        2 | 3 => VfsErrorKind::NotFound,
        80 | 183 => VfsErrorKind::AlreadyExists,
        145 => VfsErrorKind::NotEmpty,
        5 => VfsErrorKind::AccessDenied,
        1920 if project => VfsErrorKind::AccessDenied,
        32 | 33 => VfsErrorKind::SharingViolation,
        303 => VfsErrorKind::DeletePending,
        39 | 112 | 1295 => VfsErrorKind::DiskFull,
        19 => VfsErrorKind::ReadOnlyVolume,
        1 | 50 => VfsErrorKind::Unsupported,
        17 => VfsErrorKind::CrossDevice,
        170 => VfsErrorKind::Busy,
        123 | 206 => VfsErrorKind::InvalidName,
        23 | 483 | 1117 => VfsErrorKind::Io,
        267 if project => VfsErrorKind::IsDirectory,
        // The `ERROR_CLOUD_FILE_*` family, matched by name ([OS/project §2.3], [OS/fs §6.2]): its members lie at 358,
        // 362–398 with gaps, 404, 426, 434 and 475, so no range is equivalent.
        c if project && OsCode(c as i32).is_cloud_file_error() => VfsErrorKind::CloudOnly,
        _ => match domain {
            Domain::StoreData | Domain::ProjectData => VfsErrorKind::Io,
            Domain::Store | Domain::Project => VfsErrorKind::Other,
        },
    }
}

/// A [`VfsError`] from a Win32 code.
pub(crate) fn error(code: u32, domain: Domain, call: &'static str) -> VfsError {
    VfsError::new(kind_of(code, domain), OsCode(code as i32), call)
}

/// A [`VfsError`] from the calling thread's last error.
pub(crate) fn last(domain: Domain, call: &'static str) -> VfsError {
    error(last_error(), domain, call)
}

/// The Win32 code of an NTSTATUS ([OS/fs §6.1]: converted with `RtlNtStatusToDosError`).
pub(crate) fn nt_code(status: NTSTATUS) -> u32 {
    // SAFETY: `RtlNtStatusToDosError` is a pure table lookup with no preconditions.
    unsafe { RtlNtStatusToDosError(status) }
}

/// A [`VfsError`] from an NTSTATUS: `STATUS_DELETE_PENDING` is `DeletePending` before conversion ([OS/fs §6.1]). Its
/// code is 303 `ERROR_DELETE_PENDING`, the Win32 code [OS/fs §6.2] lists for the kind: `RtlNtStatusToDosError` turns
/// the status into 5 `ERROR_ACCESS_DENIED`, which would print a denial for a delete-pending name.
pub(crate) fn nt_error(status: NTSTATUS, domain: Domain, call: &'static str) -> VfsError {
    if status == STATUS_DELETE_PENDING {
        return VfsError::new(VfsErrorKind::DeletePending, OsCode(303), call);
    }
    let code = nt_code(status);
    VfsError::new(kind_of(code, domain), OsCode(code as i32), call)
}

/// `true` for a successful NTSTATUS (`NT_SUCCESS`).
pub(crate) fn nt_success(status: NTSTATUS) -> bool {
    status >= 0
}

// ---------------------------------------------------------------------------------------------------------------------
// Wide strings

/// `s` as UTF-16 followed by a NUL.
pub(crate) fn wide_z(s: &std::ffi::OsStr) -> Vec<u16> {
    let mut v: Vec<u16> = s.encode_wide().collect();
    v.push(0);
    v
}

/// `units` followed by a NUL.
pub(crate) fn with_nul(units: &[u16]) -> Vec<u16> {
    let mut v = Vec::with_capacity(units.len() + 1);
    v.extend_from_slice(units);
    v.push(0);
    v
}

/// Appends the WTF-8 bytes of `units` to `out`: valid UTF-16 as UTF-8, an unpaired surrogate as its generalized
/// three-byte form ([OS/path §2.4]: "WTF-8 of the UTF-16 name on Windows").
pub(crate) fn push_wtf8(out: &mut Vec<u8>, units: &[u16]) {
    push_wtf8_units(out, units.iter().copied());
}

/// [`push_wtf8`] over any sequence of UTF-16 units, so a caller can decode straight from a record's little-endian bytes
/// without collecting the units first.
pub(crate) fn push_wtf8_units(out: &mut Vec<u8>, units: impl IntoIterator<Item = u16>) {
    for r in char::decode_utf16(units) {
        match r {
            Ok(c) => {
                let mut b = [0u8; 4];
                out.extend_from_slice(c.encode_utf8(&mut b).as_bytes());
            }
            Err(e) => {
                let u = u32::from(e.unpaired_surrogate());
                out.extend_from_slice(&[
                    0xE0 | (u >> 12) as u8,
                    0x80 | ((u >> 6) & 0x3F) as u8,
                    0x80 | (u & 0x3F) as u8,
                ]);
            }
        }
    }
}

/// The WTF-8 bytes of `units` (the tests' form; the listing paths decode into a reused buffer).
#[cfg(test)]
pub(crate) fn wtf8(units: &[u16]) -> Vec<u8> {
    let mut out = Vec::with_capacity(units.len());
    push_wtf8(&mut out, units);
    out
}

/// Characters no NTFS name may hold, besides `\` and `/` ([OS/fs §2.1]; `:` would address an alternate data stream).
const RESERVED: [u16; 7] = [
    b'<' as u16,
    b'>' as u16,
    b':' as u16,
    b'"' as u16,
    b'|' as u16,
    b'?' as u16,
    b'*' as u16,
];

/// The use-time checks of [OS/fs §2.1] for one store segment on Windows: at most 255 UTF-16 code units, none of
/// `< > : " | ? *`, not ending in `.` or a space. The `\\?\` and handle-relative forms bypass Win32 name normalisation,
/// so such a name would otherwise be created literally.
pub(crate) fn store_segment_ok(seg: &str) -> bool {
    let mut n = 0usize;
    for u in seg.encode_utf16() {
        if RESERVED.contains(&u) {
            return false;
        }
        n += 1;
    }
    n <= 255 && !seg.ends_with('.') && !seg.ends_with(' ')
}

/// Which name check [`rel_wide`] applies to every segment before any OS call.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum NameCheck {
    /// The store use-time checks of [OS/fs §2.1] ([`store_segment_ok`]).
    Store,
    /// The Windows name check of [OS/project §2.3] (pass 1, P1-15): [OS/path §8.1]'s `representable_here`, which also
    /// holds the 255-unit component limit of [OS/path §6].
    Project,
}

/// `true` if `seg` passes `check`.
pub(crate) fn segment_ok(seg: &str, check: NameCheck) -> bool {
    match check {
        NameCheck::Store => store_segment_ok(seg),
        NameCheck::Project => super::path::representable_here(seg),
    }
}

/// The UTF-16 of `rel` with `\` separators, without a NUL, after `check` on every segment. The `\\?\` and
/// handle-relative forms bypass Win32 name normalisation, so a failing segment (`x::$DATA`, `a:b`, `y.`, `CON`, `a*b`) is
/// `InvalidName` (Win32 code 123) before any OS call ([OS/fs §2.1], [OS/project §2.3], [OS/path §6]).
pub(crate) fn rel_wide(
    rel: RelPath<'_>,
    check: NameCheck,
    call: &'static str,
) -> Result<Vec<u16>, VfsError> {
    let mut v = Vec::with_capacity(rel.as_str().len());
    for (i, seg) in rel.segments().enumerate() {
        if !segment_ok(seg, check) {
            return Err(VfsError::new(VfsErrorKind::InvalidName, OsCode(123), call));
        }
        if i > 0 {
            v.push(u16::from(b'\\'));
        }
        v.extend(seg.encode_utf16());
    }
    Ok(v)
}

/// `base` (a `\\?\` path without a trailing separator) joined with `rel`, NUL-terminated. The empty `rel` names `base`
/// itself. A drive root `\\?\X:\` keeps its separator.
pub(crate) fn join_z(base: &[u16], rel: &[u16]) -> Vec<u16> {
    let mut v = Vec::with_capacity(base.len() + rel.len() + 2);
    v.extend_from_slice(base);
    if !rel.is_empty() {
        if v.last() != Some(&u16::from(b'\\')) {
            v.push(u16::from(b'\\'));
        }
        v.extend_from_slice(rel);
    }
    v.push(0);
    v
}

/// The `\\?\` form of an absolute path, without a NUL and without a trailing separator (except a drive root
/// `\\?\X:\`): `X:\…` becomes `\\?\X:\…`, `\\server\share\…` becomes `\\?\UNC\server\share\…`, a `\\?\` path is kept
/// ([OS/path §6]). A relative path is made absolute and lexically normalised by `GetFullPathNameW` first
/// (`std::path::absolute`); `None` if `p` cannot be made absolute or is a device path (`\\.\`).
pub(crate) fn verbatim(p: &std::path::Path) -> Option<Vec<u16>> {
    let abs = std::path::absolute(p).ok()?;
    let units: Vec<u16> = abs.as_os_str().encode_wide().collect();
    let bs = u16::from(b'\\');
    let q = u16::from(b'?');
    let dot = u16::from(b'.');
    let mut v: Vec<u16>;
    if units.starts_with(&[bs, bs, q, bs]) {
        v = units
            .iter()
            .map(|&u| if u == u16::from(b'/') { bs } else { u })
            .collect();
    } else if units.starts_with(&[bs, bs, dot, bs]) {
        return None;
    } else if units.starts_with(&[bs, bs]) {
        v = "\\\\?\\UNC\\".encode_utf16().collect();
        v.extend_from_slice(&units[2..]);
    } else if units.len() >= 2 && units[1] == u16::from(b':') {
        v = "\\\\?\\".encode_utf16().collect();
        v.extend_from_slice(&units);
    } else {
        return None;
    }
    // Strip a trailing separator unless the path is a drive root (`\\?\X:\`, 7 units).
    while v.len() > 7 && v.last() == Some(&bs) {
        v.pop();
    }
    if v.len() == 6 && v[5] == u16::from(b':') {
        v.push(bs);
    }
    Some(v)
}

/// `GetFinalPathNameByHandleW(h, FILE_NAME_NORMALIZED | VOLUME_NAME_DOS)`, without the NUL: every component in its
/// on-disk spelling, in `\\?\` form ([OS/path §4.1] step 3).
pub(crate) fn final_path(h: HANDLE) -> Result<Vec<u16>, u32> {
    let mut buf: Vec<u16> = vec![0; 512];
    loop {
        // SAFETY: `buf` is a writable buffer of `buf.len()` UTF-16 units; `h` is a valid handle owned by the caller.
        let n = unsafe { GetFinalPathNameByHandleW(h, buf.as_mut_ptr(), buf.len() as u32, 0) };
        if n == 0 {
            return Err(last_error());
        }
        let n = n as usize;
        if n < buf.len() {
            buf.truncate(n);
            return Ok(buf);
        }
        // `n` is the size needed, including the NUL.
        buf.resize(n + 1, 0);
    }
}

/// Opens an absolute `\\?\` path (NUL-terminated) with `CreateFileW`, no security attributes (non-inheritable).
pub(crate) fn create_file(
    path_z: &[u16],
    access: u32,
    share: u32,
    disposition: u32,
    flags: FILE_FLAGS_AND_ATTRIBUTES,
) -> Result<OwnedHandle, u32> {
    debug_assert_eq!(path_z.last(), Some(&0));
    // SAFETY: `path_z` is a NUL-terminated UTF-16 string that outlives the call; the security attributes and template
    // handle are null, which the call allows.
    let h = unsafe {
        CreateFileW(
            path_z.as_ptr(),
            access,
            share,
            core::ptr::null(),
            disposition,
            flags,
            core::ptr::null_mut(),
        )
    };
    owned(h).ok_or_else(last_error)
}

/// Opens an existing object for attributes only (`FILE_READ_ATTRIBUTES`, full sharing), with `flags`.
pub(crate) fn open_attrs(
    path_z: &[u16],
    flags: FILE_FLAGS_AND_ATTRIBUTES,
) -> Result<OwnedHandle, u32> {
    create_file(
        path_z,
        windows_sys::Win32::Storage::FileSystem::FILE_READ_ATTRIBUTES,
        SHARE_ALL,
        OPEN_EXISTING,
        flags,
    )
}

/// The parameters of one `NtCreateFile` ([OS/fs §5.1] table).
#[derive(Copy, Clone, Debug)]
pub(crate) struct NtOpen {
    /// Desired access.
    pub(crate) access: u32,
    /// `FILE_OPEN`, `FILE_CREATE`, …
    pub(crate) disposition: u32,
    /// Create options.
    pub(crate) options: u32,
    /// File attributes for a create.
    pub(crate) attributes: u32,
}

/// `NtCreateFile` of `name` (UTF-16 with `\` separators, no NUL) relative to the directory handle `root`
/// (`OBJECT_ATTRIBUTES.RootDirectory`), with `OBJ_CASE_INSENSITIVE` as Win32 does and full sharing ([OS/fs §5.1]). The
/// empty name opens `root`'s own directory again. The name is at most 32,767 UTF-16 units (a `UNICODE_STRING`), else
/// `InvalidName`.
pub(crate) fn nt_open(
    root: HANDLE,
    name: &[u16],
    o: NtOpen,
    domain: Domain,
) -> Result<OwnedHandle, VfsError> {
    let bytes = name.len() * 2;
    if bytes > usize::from(u16::MAX) - 1 {
        return Err(VfsError::new(
            VfsErrorKind::InvalidName,
            OsCode(206),
            "NtCreateFile",
        ));
    }
    let us = UNICODE_STRING {
        Length: bytes as u16,
        MaximumLength: bytes as u16,
        Buffer: name.as_ptr().cast_mut(),
    };
    let oa = OBJECT_ATTRIBUTES {
        Length: core::mem::size_of::<OBJECT_ATTRIBUTES>() as u32,
        RootDirectory: root,
        ObjectName: &us,
        Attributes: OBJ_CASE_INSENSITIVE,
        SecurityDescriptor: core::ptr::null(),
        SecurityQualityOfService: core::ptr::null(),
    };
    let mut iosb = IO_STATUS_BLOCK::default();
    let mut h: HANDLE = core::ptr::null_mut();
    // SAFETY: every pointer argument points to a live local (`h`, `oa`, `us` through `oa`, `iosb`) or is null where
    // the call allows it (allocation size, EA buffer); `name` outlives the call; `NtCreateFile` does not write through
    // `us.Buffer`.
    let status = unsafe {
        NtCreateFile(
            &mut h,
            o.access,
            &oa,
            &mut iosb,
            core::ptr::null(),
            o.attributes,
            SHARE_ALL,
            o.disposition,
            o.options,
            core::ptr::null(),
            0,
        )
    };
    if !nt_success(status) {
        return Err(nt_error(status, domain, "NtCreateFile"));
    }
    owned(h).ok_or_else(|| VfsError::new(VfsErrorKind::Other, OsCode::NONE, "NtCreateFile"))
}

// ---------------------------------------------------------------------------------------------------------------------
// Positional I/O and handle queries

fn overlapped_at(offset: u64) -> OVERLAPPED {
    OVERLAPPED {
        Internal: 0,
        InternalHigh: 0,
        Anonymous: OVERLAPPED_0 {
            Anonymous: OVERLAPPED_0_0 {
                Offset: offset as u32,
                OffsetHigh: (offset >> 32) as u32,
            },
        },
        hEvent: core::ptr::null_mut(),
    }
}

/// One `ReadFile` at `offset` on a synchronous handle ([OS/fs §5.1]: the form of Rust std's `seek_read`); 0 at or
/// beyond the end of the file.
pub(crate) fn read_once(h: HANDLE, offset: u64, buf: &mut [u8]) -> Result<usize, u32> {
    let len = buf.len().min(u32::MAX as usize) as u32;
    let mut ov = overlapped_at(offset);
    let mut n = 0u32;
    // SAFETY: `buf` is writable for `len` bytes; `ov` and `n` are live locals; `h` is a synchronous handle, so the call
    // completes before it returns and nothing refers to `ov` afterwards.
    let ok = unsafe { ReadFile(h, buf.as_mut_ptr(), len, &mut n, &mut ov) };
    if ok != 0 {
        Ok(n as usize)
    } else {
        match last_error() {
            ERROR_HANDLE_EOF => Ok(0),
            e => Err(e),
        }
    }
}

/// One `WriteFile` at `offset` on a synchronous handle; returns the bytes written.
pub(crate) fn write_once(h: HANDLE, offset: u64, buf: &[u8]) -> Result<usize, u32> {
    let len = buf.len().min(u32::MAX as usize) as u32;
    let mut ov = overlapped_at(offset);
    let mut n = 0u32;
    // SAFETY: `buf` is readable for `len` bytes; `ov` and `n` are live locals; the handle is synchronous, so the call
    // completes before it returns.
    let ok = unsafe { WriteFile(h, buf.as_ptr(), len, &mut n, &mut ov) };
    if ok != 0 {
        Ok(n as usize)
    } else {
        Err(last_error())
    }
}

/// `GetFileInformationByHandleEx` of a fixed-size class into a `T`.
pub(crate) fn file_info<T: Copy>(h: HANDLE, class: i32, init: T) -> Result<T, u32> {
    let mut v = init;
    // SAFETY: `v` is a live, properly aligned `T` of `size_of::<T>()` bytes, which is the buffer the class writes.
    let ok = unsafe {
        GetFileInformationByHandleEx(
            h,
            class,
            (&mut v as *mut T).cast(),
            core::mem::size_of::<T>() as u32,
        )
    };
    if ok != 0 { Ok(v) } else { Err(last_error()) }
}

/// `FileIdInfo`: the volume serial and the 128-bit file id ([OS/fs §4.10]).
pub(crate) fn id_info(h: HANDLE) -> Result<FILE_ID_INFO, u32> {
    file_info(h, FileIdInfo, FILE_ID_INFO::default())
}

/// `FileStandardInfo` ([OS/fs §4.10]).
pub(crate) fn standard_info(h: HANDLE) -> Result<FILE_STANDARD_INFO, u32> {
    file_info(h, FileStandardInfo, FILE_STANDARD_INFO::default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn code_mapping_follows_the_tables() {
        use VfsErrorKind as K;
        let cases = [
            (2, K::NotFound),
            (3, K::NotFound),
            (80, K::AlreadyExists),
            (183, K::AlreadyExists),
            (145, K::NotEmpty),
            (5, K::AccessDenied),
            (32, K::SharingViolation),
            (33, K::SharingViolation),
            (303, K::DeletePending),
            (112, K::DiskFull),
            (39, K::DiskFull),
            (1295, K::DiskFull),
            (19, K::ReadOnlyVolume),
            (50, K::Unsupported),
            (1, K::Unsupported),
            (17, K::CrossDevice),
            (170, K::Busy),
            (123, K::InvalidName),
            (206, K::InvalidName),
            (23, K::Io),
            (1117, K::Io),
            (483, K::Io),
        ];
        for (code, kind) in cases {
            assert_eq!(kind_of(code, Domain::Store), kind, "code {code}");
            assert_eq!(kind_of(code, Domain::Project), kind, "code {code}");
        }
        assert_eq!(kind_of(9999, Domain::Store), K::Other);
        assert_eq!(kind_of(9999, Domain::StoreData), K::Io);
        assert_eq!(kind_of(267, Domain::Store), K::Other);
        assert_eq!(kind_of(267, Domain::Project), K::IsDirectory);
        assert_eq!(kind_of(391, Domain::Project), K::CloudOnly);
        assert_eq!(kind_of(391, Domain::Store), K::Other);
        // The family by name: every `ERROR_CLOUD_FILE_*` constant of the pinned binding, and none of the codes in its
        // gaps (359–361 and 367–373 are file-system virtualization and other errors, 399–400 others).
        use windows_sys::Win32::Foundation as F;
        for code in [
            F::ERROR_CLOUD_FILE_SYNC_ROOT_METADATA_CORRUPT,
            F::ERROR_CLOUD_FILE_PROVIDER_NOT_RUNNING,
            F::ERROR_CLOUD_FILE_METADATA_CORRUPT,
            F::ERROR_CLOUD_FILE_METADATA_TOO_LARGE,
            F::ERROR_CLOUD_FILE_PROPERTY_BLOB_TOO_LARGE,
            F::ERROR_CLOUD_FILE_PROPERTY_BLOB_CHECKSUM_MISMATCH,
            F::ERROR_CLOUD_FILE_TOO_MANY_PROPERTY_BLOBS,
            F::ERROR_CLOUD_FILE_PROPERTY_VERSION_NOT_SUPPORTED,
            F::ERROR_CLOUD_FILE_NOT_IN_SYNC,
            F::ERROR_CLOUD_FILE_ALREADY_CONNECTED,
            F::ERROR_CLOUD_FILE_NOT_SUPPORTED,
            F::ERROR_CLOUD_FILE_INVALID_REQUEST,
            F::ERROR_CLOUD_FILE_READ_ONLY_VOLUME,
            F::ERROR_CLOUD_FILE_CONNECTED_PROVIDER_ONLY,
            F::ERROR_CLOUD_FILE_VALIDATION_FAILED,
            F::ERROR_CLOUD_FILE_AUTHENTICATION_FAILED,
            F::ERROR_CLOUD_FILE_INSUFFICIENT_RESOURCES,
            F::ERROR_CLOUD_FILE_NETWORK_UNAVAILABLE,
            F::ERROR_CLOUD_FILE_UNSUCCESSFUL,
            F::ERROR_CLOUD_FILE_NOT_UNDER_SYNC_ROOT,
            F::ERROR_CLOUD_FILE_IN_USE,
            F::ERROR_CLOUD_FILE_PINNED,
            F::ERROR_CLOUD_FILE_REQUEST_ABORTED,
            F::ERROR_CLOUD_FILE_PROPERTY_CORRUPT,
            F::ERROR_CLOUD_FILE_ACCESS_DENIED,
            F::ERROR_CLOUD_FILE_INCOMPATIBLE_HARDLINKS,
            F::ERROR_CLOUD_FILE_PROPERTY_LOCK_CONFLICT,
            F::ERROR_CLOUD_FILE_REQUEST_CANCELED,
            F::ERROR_CLOUD_FILE_PROVIDER_TERMINATED,
            F::ERROR_CLOUD_FILE_REQUEST_TIMEOUT,
            F::ERROR_CLOUD_FILE_DEHYDRATION_DISALLOWED,
            F::ERROR_CLOUD_FILE_US_MESSAGE_TIMEOUT,
        ] {
            assert_eq!(kind_of(code, Domain::Project), K::CloudOnly, "code {code}");
        }
        for gap in [359, 360, 361, 367, 370, 373, 376, 384, 385, 399, 400] {
            assert_eq!(kind_of(gap, Domain::Project), K::Other, "code {gap}");
        }
        assert_eq!(kind_of(1920, Domain::Project), K::AccessDenied);
        assert_eq!(kind_of(9999, Domain::ProjectData), K::Io);
    }

    #[test]
    fn delete_pending_status_maps_before_conversion() {
        let e = nt_error(STATUS_DELETE_PENDING, Domain::Store, "NtCreateFile");
        assert_eq!(e.kind, VfsErrorKind::DeletePending);
        assert_eq!(e.os, OsCode(303));
    }

    #[test]
    fn store_segments() {
        assert!(store_segment_ok("HEAD"));
        assert!(store_segment_ok("seg.base.7"));
        for bad in ["a:b", "x.", "x ", "a*b", "q?", "<", ">", "\"", "|"] {
            assert!(!store_segment_ok(bad), "{bad:?}");
        }
        assert!(store_segment_ok(&"a".repeat(255)));
        assert!(!store_segment_ok(&"a".repeat(256)));
        // 255 UTF-16 units, counted as units, not bytes: 255 × U+00E9 is 510 UTF-8 bytes.
        assert!(store_segment_ok(&"é".repeat(255)));
        let rel = RelPath::new("a/b.c").unwrap();
        for check in [NameCheck::Store, NameCheck::Project] {
            assert_eq!(
                String::from_utf16(&rel_wide(rel, check, "t").unwrap()).unwrap(),
                "a\\b.c"
            );
            assert!(rel_wide(RelPath::ROOT, check, "t").unwrap().is_empty());
            // Both checks refuse a segment Windows would store literally or read as a stream or a wildcard.
            for bad in ["a./b", "x::$DATA", "a:b", "d/y.", "y ", "a*b", "q?"] {
                let e = rel_wide(RelPath::new(bad).unwrap(), check, "t").unwrap_err();
                assert_eq!(
                    (e.kind, e.os, e.call),
                    (VfsErrorKind::InvalidName, OsCode(123), "t"),
                    "{bad:?} under {check:?}"
                );
            }
        }
        // Device names are the project check's: store names are a fixed grammar that holds none ([OS/fs §2.1]).
        for dev in ["CON", "d/nul.txt", "COM1", "lpt\u{B9}"] {
            let rel = RelPath::new(dev).unwrap();
            assert!(rel_wide(rel, NameCheck::Project, "t").is_err(), "{dev:?}");
        }
        assert!(!segment_ok(&"a".repeat(256), NameCheck::Project));
        assert!(segment_ok(&"é".repeat(255), NameCheck::Project));
    }

    #[test]
    fn verbatim_forms() {
        let s =
            |p: &str| verbatim(std::path::Path::new(p)).map(|v| String::from_utf16(&v).unwrap());
        assert_eq!(s("C:\\a\\b\\").as_deref(), Some("\\\\?\\C:\\a\\b"));
        assert_eq!(s("C:/a/./b/../c").as_deref(), Some("\\\\?\\C:\\a\\c"));
        assert_eq!(s("C:\\").as_deref(), Some("\\\\?\\C:\\"));
        assert_eq!(
            s("\\\\srv\\share\\x").as_deref(),
            Some("\\\\?\\UNC\\srv\\share\\x")
        );
        assert_eq!(s("\\\\?\\D:\\x").as_deref(), Some("\\\\?\\D:\\x"));
        assert_eq!(s("\\\\.\\PhysicalDrive0"), None);
        let z = join_z(
            &"\\\\?\\C:\\".encode_utf16().collect::<Vec<_>>(),
            &[u16::from(b'x')],
        );
        assert_eq!(
            String::from_utf16(&z[..z.len() - 1]).unwrap(),
            "\\\\?\\C:\\x"
        );
    }

    #[test]
    fn wtf8_of_unpaired_surrogates() {
        assert_eq!(
            wtf8(&"ok é".encode_utf16().collect::<Vec<_>>()),
            "ok é".as_bytes()
        );
        assert_eq!(wtf8(&[0x78, 0xD800]), vec![0x78, 0xED, 0xA0, 0x80]);
        assert_eq!(wtf8(&[0xDFFF]), vec![0xED, 0xBF, 0xBF]);
    }

    proptest! {
        #![proptest_config(crate::windows::testing::proptest_config())]

        /// WTF-8 of valid UTF-16 is its UTF-8; of any sequence, it is valid UTF-8 exactly when the input has no unpaired
        /// surrogate, and every unit produces at least one byte.
        #[test]
        fn wtf8_agrees_with_utf8(units in proptest::collection::vec(any::<u16>(), 0..40)) {
            let out = wtf8(&units);
            match String::from_utf16(&units) {
                Ok(s) => prop_assert_eq!(out, s.into_bytes()),
                Err(_) => prop_assert!(core::str::from_utf8(&out).is_err()),
            }
        }
    }
}
