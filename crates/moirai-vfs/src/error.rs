//! The error types shared by every seam: `OsCode`, `VfsError` with its `VfsErrorKind`, and `DurabilityFailure`
//! ([OS/fs §4.4.5, §6.1], [OS/README §4.5]).
//!
//! The mapping of OS codes onto kinds is per OS and lives in `moirai-os` ([OS/fs §6.2]); every abstract class of the
//! fault model ([F15 §2.6]) is the kind of the same name (OP-18, closed by spec sync 2a).
//!
//! Two renderings live here because the OS layer and the simulator must print them identically: the OS-error unit
//! `os <code> <SYMBOL>` of [OS/shell §6] item 6 ([`OsCode::unit`]) and the one stderr line of `fail_stop`
//! ([`DurabilityFailure::stderr_line`], [F19 §10.2] row `durability_failure`), which a [`VfsErrorKind::FlushFailed`] error
//! prints too ([`VfsError::durability_line`]).

use core::fmt;

use crate::fs::DurabilityClass;
use crate::proc::OsTag;

/// The raw OS error of [OS/fs §6.1]: a Win32 error code (NTSTATUS values converted with `RtlNtStatusToDosError`, except
/// that `STATUS_DELETE_PENDING` is mapped to [`VfsErrorKind::DeletePending`] before conversion and carries 303
/// `ERROR_DELETE_PENDING`, the code [OS/fs §6.2] lists for the kind) or an `errno`; 0 when there is none. Carried for
/// diagnostics only.
///
/// Output shows a code only as the ASCII OS-error unit `os <code> <SYMBOL>` of [OS/shell §6] item 6 ([`OsCode::unit`]),
/// which the golden harness replaces as a whole with `<OSERR>` ([80 §4] T7); `FormatMessageW` and `strerror` texts are
/// never printed. The `Display` form (`os <code>`) is for internal diagnostics only.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct OsCode(pub i32);

impl OsCode {
    /// No OS code: the failure was detected by moirai itself (for example [`VfsErrorKind::InsufficientSpace`],
    /// [OS/fs §6.2]).
    pub const NONE: OsCode = OsCode(0);

    /// The symbolic name of the code on the OS `os` from moirai's own table ([OS/shell §6] item 6): the Windows `ERROR_*`
    /// name or the `errno` name; `None` for a code the table lacks, for 0 and for [`OsTag::Unspecified`]. The table holds
    /// every code the OS-layer specification names ([OS/fs §6.2], [OS/lock §7], [OS/map §2–§3], [OS/project §2.3],
    /// [OS/proc §6.1], [OS/shell §6]); where two `errno` names share a value, it holds the one that OS's `errno.h`
    /// defines first ([OS/shell §6] item 6): `EAGAIN`, never `EWOULDBLOCK`; on Linux `EOPNOTSUPP` for 95, which `ENOTSUP`
    /// aliases; macOS, where the two differ (45 and 102), each under its own name.
    ///
    /// The `ERROR_CLOUD_FILE_*` family is held by name ([OS/project §2.3], [OS/fs §6.2]: members at 358, 404, 426, 434
    /// and 475 and gaps inside 362–400), so [`OsCode::is_cloud_file_error`] matches it by name, never by a range.
    pub const fn symbol(self, os: OsTag) -> Option<&'static str> {
        match os {
            OsTag::Windows => windows_symbol(self.0),
            OsTag::Linux => linux_symbol(self.0),
            OsTag::MacOs => darwin_symbol(self.0),
            OsTag::Unspecified => None,
        }
    }

    /// The OS-error unit `os <code> <SYMBOL>` of [OS/shell §6] item 6 for a code raised on the OS `os` (the running
    /// build's [`crate::ProcHost::os_tag`]); `<SYMBOL>` is `?` for a code the table lacks.
    pub const fn unit(self, os: OsTag) -> OsErrorUnit {
        OsErrorUnit { code: self, os }
    }

    /// `true` for a Win32 code of the `ERROR_CLOUD_FILE_*` family, matched by name ([OS/fs §6.2] row `CloudOnly`,
    /// [OS/project §2.3]): the codes whose symbol in moirai's table starts with `ERROR_CLOUD_FILE_`. The family has gaps
    /// (359–361, 367–373, 376, 384–385, 399–403 are other errors), so no range test is equivalent.
    pub const fn is_cloud_file_error(self) -> bool {
        match windows_symbol(self.0) {
            Some(s) => starts_with(s.as_bytes(), b"ERROR_CLOUD_FILE_"),
            None => false,
        }
    }
}

/// `s` starts with `prefix` (a `const` form of `<[u8]>::starts_with`).
const fn starts_with(s: &[u8], prefix: &[u8]) -> bool {
    if s.len() < prefix.len() {
        return false;
    }
    let mut i = 0;
    while i < prefix.len() {
        if s[i] != prefix[i] {
            return false;
        }
        i += 1;
    }
    true
}

impl fmt::Display for OsCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "os {}", self.0)
    }
}

/// The ASCII OS-error unit `os <code> <SYMBOL>` ([OS/shell §6] item 6, [F19 §2.4]); made by [`OsCode::unit`].
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct OsErrorUnit {
    code: OsCode,
    os: OsTag,
}

impl fmt::Display for OsErrorUnit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let symbol = self.code.symbol(self.os).unwrap_or("?");
        write!(f, "os {} {symbol}", self.code.0)
    }
}

/// Win32 error codes ([OS/fs §6.2], [OS/lock §7.1], [OS/map §2–§3], [OS/project §2.3, §5.2, §5.4], [OS/proc §6.1],
/// [OS/shell §6] item 4), with every `ERROR_CLOUD_FILE_*` code winerror.h defines ([OS/project §2.3] names the family).
const fn windows_symbol(code: i32) -> Option<&'static str> {
    Some(match code {
        1 => "ERROR_INVALID_FUNCTION",
        2 => "ERROR_FILE_NOT_FOUND",
        3 => "ERROR_PATH_NOT_FOUND",
        5 => "ERROR_ACCESS_DENIED",
        8 => "ERROR_NOT_ENOUGH_MEMORY",
        17 => "ERROR_NOT_SAME_DEVICE",
        18 => "ERROR_NO_MORE_FILES",
        19 => "ERROR_WRITE_PROTECT",
        23 => "ERROR_CRC",
        32 => "ERROR_SHARING_VIOLATION",
        33 => "ERROR_LOCK_VIOLATION",
        39 => "ERROR_HANDLE_DISK_FULL",
        50 => "ERROR_NOT_SUPPORTED",
        80 => "ERROR_FILE_EXISTS",
        87 => "ERROR_INVALID_PARAMETER",
        109 => "ERROR_BROKEN_PIPE",
        112 => "ERROR_DISK_FULL",
        123 => "ERROR_INVALID_NAME",
        145 => "ERROR_DIR_NOT_EMPTY",
        170 => "ERROR_BUSY",
        183 => "ERROR_ALREADY_EXISTS",
        206 => "ERROR_FILENAME_EXCED_RANGE",
        232 => "ERROR_NO_DATA",
        267 => "ERROR_DIRECTORY",
        303 => "ERROR_DELETE_PENDING",
        358 => "ERROR_CLOUD_FILE_SYNC_ROOT_METADATA_CORRUPT",
        362 => "ERROR_CLOUD_FILE_PROVIDER_NOT_RUNNING",
        363 => "ERROR_CLOUD_FILE_METADATA_CORRUPT",
        364 => "ERROR_CLOUD_FILE_METADATA_TOO_LARGE",
        365 => "ERROR_CLOUD_FILE_PROPERTY_BLOB_TOO_LARGE",
        366 => "ERROR_CLOUD_FILE_PROPERTY_BLOB_CHECKSUM_MISMATCH",
        374 => "ERROR_CLOUD_FILE_TOO_MANY_PROPERTY_BLOBS",
        375 => "ERROR_CLOUD_FILE_PROPERTY_VERSION_NOT_SUPPORTED",
        377 => "ERROR_CLOUD_FILE_NOT_IN_SYNC",
        378 => "ERROR_CLOUD_FILE_ALREADY_CONNECTED",
        379 => "ERROR_CLOUD_FILE_NOT_SUPPORTED",
        380 => "ERROR_CLOUD_FILE_INVALID_REQUEST",
        381 => "ERROR_CLOUD_FILE_READ_ONLY_VOLUME",
        382 => "ERROR_CLOUD_FILE_CONNECTED_PROVIDER_ONLY",
        383 => "ERROR_CLOUD_FILE_VALIDATION_FAILED",
        386 => "ERROR_CLOUD_FILE_AUTHENTICATION_FAILED",
        387 => "ERROR_CLOUD_FILE_INSUFFICIENT_RESOURCES",
        388 => "ERROR_CLOUD_FILE_NETWORK_UNAVAILABLE",
        389 => "ERROR_CLOUD_FILE_UNSUCCESSFUL",
        390 => "ERROR_CLOUD_FILE_NOT_UNDER_SYNC_ROOT",
        391 => "ERROR_CLOUD_FILE_IN_USE",
        392 => "ERROR_CLOUD_FILE_PINNED",
        393 => "ERROR_CLOUD_FILE_REQUEST_ABORTED",
        394 => "ERROR_CLOUD_FILE_PROPERTY_CORRUPT",
        395 => "ERROR_CLOUD_FILE_ACCESS_DENIED",
        396 => "ERROR_CLOUD_FILE_INCOMPATIBLE_HARDLINKS",
        397 => "ERROR_CLOUD_FILE_PROPERTY_LOCK_CONFLICT",
        398 => "ERROR_CLOUD_FILE_REQUEST_CANCELED",
        404 => "ERROR_CLOUD_FILE_PROVIDER_TERMINATED",
        426 => "ERROR_CLOUD_FILE_REQUEST_TIMEOUT",
        434 => "ERROR_CLOUD_FILE_DEHYDRATION_DISALLOWED",
        475 => "ERROR_CLOUD_FILE_US_MESSAGE_TIMEOUT",
        483 => "ERROR_DEVICE_HARDWARE_ERROR",
        995 => "ERROR_OPERATION_ABORTED",
        997 => "ERROR_IO_PENDING",
        1117 => "ERROR_IO_DEVICE",
        1224 => "ERROR_USER_MAPPED_FILE",
        1295 => "ERROR_DISK_QUOTA_EXCEEDED",
        1920 => "ERROR_CANT_ACCESS_FILE",
        _ => return None,
    })
}

/// The `errno` values Linux and Darwin share ([OS/fs §6.2], [OS/lock §3, §7.2], [OS/map §3], [OS/project §2.3],
/// [OS/proc §6.1]).
const fn common_errno_symbol(code: i32) -> Option<&'static str> {
    Some(match code {
        1 => "EPERM",
        2 => "ENOENT",
        3 => "ESRCH",
        4 => "EINTR",
        5 => "EIO",
        9 => "EBADF",
        12 => "ENOMEM",
        13 => "EACCES",
        16 => "EBUSY",
        17 => "EEXIST",
        18 => "EXDEV",
        20 => "ENOTDIR",
        21 => "EISDIR",
        22 => "EINVAL",
        28 => "ENOSPC",
        30 => "EROFS",
        32 => "EPIPE",
        _ => return None,
    })
}

/// Linux `errno` values (the generic table, identical on x86_64 and aarch64).
const fn linux_symbol(code: i32) -> Option<&'static str> {
    match common_errno_symbol(code) {
        Some(s) => Some(s),
        None => Some(match code {
            11 => "EAGAIN",
            36 => "ENAMETOOLONG",
            39 => "ENOTEMPTY",
            40 => "ELOOP",
            74 => "EBADMSG",
            84 => "EILSEQ",
            95 => "EOPNOTSUPP",
            117 => "EUCLEAN",
            122 => "EDQUOT",
            _ => return None,
        }),
    }
}

/// Darwin `errno` values.
const fn darwin_symbol(code: i32) -> Option<&'static str> {
    match common_errno_symbol(code) {
        Some(s) => Some(s),
        None => Some(match code {
            35 => "EAGAIN",
            45 => "ENOTSUP",
            62 => "ELOOP",
            63 => "ENAMETOOLONG",
            66 => "ENOTEMPTY",
            69 => "EDQUOT",
            92 => "EILSEQ",
            94 => "EBADMSG",
            102 => "EOPNOTSUPP",
            _ => return None,
        }),
    }
}

/// Every non-durability error of `Vfs`, `ProjectFs` and `Meter::free_space` ([OS/fs §6.1]): a kind, the raw OS code and
/// the name of the OS call (or moirai check) that failed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VfsError {
    /// What the caller reacts to ([OS/fs §6.2] "Callers' reaction").
    pub kind: VfsErrorKind,
    /// The raw code, diagnostics only.
    pub os: OsCode,
    /// The failing call, for example `"NtCreateFile"` or `"renameat2"`.
    pub call: &'static str,
}

impl VfsError {
    /// A new error.
    pub const fn new(kind: VfsErrorKind, os: OsCode, call: &'static str) -> VfsError {
        VfsError { kind, os, call }
    }

    /// For a [`VfsErrorKind::FlushFailed`] error, the one `durability_failure` line its caller prints before it exits 7
    /// ([F19 §10.2]: "a `FlushFailed` is `durable-name`"; [OS/fs §4.1, §6.2]), formatted as
    /// [`DurabilityFailure::stderr_line`] formats it, with `<oserr>` the OS-error unit for a code raised on `os`; `None`
    /// for every other kind.
    pub const fn durability_line(&self, os: OsTag) -> Option<DurabilityLine> {
        match self.kind {
            VfsErrorKind::FlushFailed => Some(DurabilityLine {
                call: self.call,
                class: DurabilityClass::DurableName,
                code: self.os,
                os,
            }),
            _ => None,
        }
    }
}

impl fmt::Display for VfsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {} ({})", self.call, self.kind, self.os)
    }
}

impl std::error::Error for VfsError {}

/// The kinds of [OS/fs §6.1], extended by the five kinds [OS/project §2.3] needs for project files.
///
/// `#[non_exhaustive]` keeps a port free to report the same kinds from new codes; it never lets a port add a behaviour
/// ([OS/fs §6.1]).
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
#[non_exhaustive]
pub enum VfsErrorKind {
    /// The path or a parent is absent (Win32 2, 3; `ENOENT`, `ENOTDIR`).
    NotFound,
    /// Exclusive create or a no-replace rename onto an existing name (Win32 80, 183; `EEXIST`).
    AlreadyExists,
    /// Removal of a non-empty directory (Win32 145; `ENOTEMPTY`).
    NotEmpty,
    /// Access refused (Win32 5, also the Win32 form of a delete-pending name, a mapped file's delete and a read-only
    /// file's write or delete; `EACCES`, `EPERM`). Never mapped to absence ([OS/project §5.11]).
    AccessDenied,
    /// A Windows sharing conflict (Win32 32, 33); Unix has none.
    SharingViolation,
    /// The name belongs to a file deleted while another handle holds it open (`STATUS_DELETE_PENDING`, Win32 303;
    /// [OS/fs §6.4]).
    DeletePending,
    /// Disk full or quota (Win32 112, 39, 1295; `ENOSPC`, `EDQUOT`); from a sync it is a `fail_stop` ([OS/fs §4.4.5]).
    DiskFull,
    /// The sparse-extent early warning of [OS/fs §4.5] (`OsCode(0)`).
    InsufficientSpace,
    /// The volume is write-protected (Win32 19; `EROFS`).
    ReadOnlyVolume,
    /// The location cannot provide a class or call; the store is refused, never downgraded ([80 §2.3.1]).
    Unsupported,
    /// The operation would cross volumes or mounts (Win32 17; `EXDEV`); the fault model's `CrossVolume` ([F15 §2.6]).
    CrossDevice,
    /// A busy object (Win32 170; `EBUSY`).
    Busy,
    /// A name the OS cannot hold or that fails a use-time check ([OS/fs §2.1]; Win32 123, 206; `ENAMETOOLONG`,
    /// `EILSEQ`).
    InvalidName,
    /// `read_exact_at` met the end of the file before the end of the buffer.
    UnexpectedEof,
    /// A read, write or device failure (fault-model item (12); [F15 §3.12] FM-12).
    Io,
    /// Any other code (including `ELOOP` and a `RESOLVE_BENEATH` `EXDEV` on a store open, [OS/fs §5.2]).
    Other,
    /// A durability-class flush embedded in `create_root`, `swap_dirs` or `swap_recover` failed ([OS/fs §4.1, §4.9,
    /// §6.1]); the error carries that flush's `OsCode` and `call` ([`DurabilityFailure::embedded`]). The caller exits 7
    /// with the `durability-failure` text ([`VfsError::durability_line`]), never retries, and issues no further write,
    /// flush, create or namespace call ([F15 §3.13]). Never the `kind` of a [`DurabilityFailure`], which it only wraps.
    FlushFailed,
    /// `ProjectFs`: the operation would hydrate a cloud-only entry ([OS/project §5.10]).
    CloudOnly,
    /// `ProjectFs`: a content read of a symbolic link; use `read_link` ([OS/project §5.5]).
    IsSymlink,
    /// `ProjectFs`: a content read or unlink of a directory (Win32 267; `EISDIR`).
    IsDirectory,
    /// `ProjectFs`: an opened object's final path is not under the root ([OS/project §5.5]).
    OutsideRoot,
    /// `ProjectFs`: the root's identity changed since it was canonicalised ([OS/project §2.2]).
    Stale,
}

impl VfsErrorKind {
    /// A stable lower-case name for diagnostics (not a frozen user text; [F19 §10.2] owns those).
    pub const fn as_str(self) -> &'static str {
        match self {
            VfsErrorKind::NotFound => "not found",
            VfsErrorKind::AlreadyExists => "already exists",
            VfsErrorKind::NotEmpty => "directory not empty",
            VfsErrorKind::AccessDenied => "access denied",
            VfsErrorKind::SharingViolation => "sharing violation",
            VfsErrorKind::DeletePending => "delete pending",
            VfsErrorKind::DiskFull => "disk full",
            VfsErrorKind::InsufficientSpace => "insufficient space",
            VfsErrorKind::ReadOnlyVolume => "read-only volume",
            VfsErrorKind::Unsupported => "unsupported",
            VfsErrorKind::CrossDevice => "cross-device",
            VfsErrorKind::Busy => "busy",
            VfsErrorKind::InvalidName => "invalid name",
            VfsErrorKind::UnexpectedEof => "unexpected end of file",
            VfsErrorKind::Io => "I/O error",
            VfsErrorKind::Other => "other error",
            VfsErrorKind::CloudOnly => "cloud-only entry",
            VfsErrorKind::IsSymlink => "is a symbolic link",
            VfsErrorKind::IsDirectory => "is a directory",
            VfsErrorKind::OutsideRoot => "outside the root",
            VfsErrorKind::Stale => "stale root",
            VfsErrorKind::FlushFailed => "flush failed",
        }
    }
}

impl fmt::Display for VfsErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// An error returned by a non-lazy durability class ([OS/fs §4.4.5]). It must be passed to `StoreFs::fail_stop`; it has
/// no other consumer, with two exceptions: the `init` probe turns it into a refusal ([OS/env §5] step 3), and a flush
/// embedded in `create_root`, `swap_dirs` or `swap_recover` is returned as a `VfsError` of kind
/// [`VfsErrorKind::FlushFailed`] ([`DurabilityFailure::embedded`], [OS/fs §4.1]).
///
/// Any error from any class other than `lazy` aborts the process without an acknowledgement, and the flush is never
/// retried on the same handle ([80 §2.3.1], [F15 §4.3]).
#[must_use]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DurabilityFailure {
    /// The class whose call failed.
    pub class: DurabilityClass,
    /// The failing OS call, for example `"NtFlushBuffersFileEx"`.
    pub call: &'static str,
    /// The kind the OS code maps to ([OS/fs §6.2]).
    pub kind: VfsErrorKind,
    /// The raw code, diagnostics only.
    pub os: OsCode,
}

impl DurabilityFailure {
    /// The one stderr line `fail_stop` writes ([OS/fs §4.4.5] item 2; [F19 §10.2] row `durability_failure`), without its
    /// final LF: `error[durability_failure]: <call> (<class>) failed: <oserr>; outcome unknown: re-run with the same key
    /// or check moirai changes`, with `<class>` the class name of [`DurabilityClass::as_str`] and `<oserr>` the OS-error
    /// unit for a code raised on `os` (the running build's [`crate::ProcHost::os_tag`]). ASCII; formatting it allocates
    /// nothing.
    pub const fn stderr_line(&self, os: OsTag) -> DurabilityLine {
        DurabilityLine {
            call: self.call,
            class: self.class,
            code: self.os,
            os,
        }
    }

    /// This failure as the error `create_root`, `swap_dirs` and `swap_recover` return for a flush embedded in them
    /// ([OS/fs §4.1, §4.9]): kind [`VfsErrorKind::FlushFailed`] with the flush's `OsCode` and `call`.
    pub const fn embedded(&self) -> VfsError {
        VfsError::new(VfsErrorKind::FlushFailed, self.os, self.call)
    }
}

impl fmt::Display for DurabilityFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} ({}) failed: {} ({})",
            self.call,
            self.class.as_str(),
            self.kind,
            self.os
        )
    }
}

impl std::error::Error for DurabilityFailure {}

/// The `durability_failure` line ([F19 §10.2]) of a [`DurabilityFailure`] ([`DurabilityFailure::stderr_line`]) or of a
/// [`VfsErrorKind::FlushFailed`] error ([`VfsError::durability_line`]). It copies what it prints, so it borrows nothing.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct DurabilityLine {
    call: &'static str,
    class: DurabilityClass,
    code: OsCode,
    os: OsTag,
}

impl fmt::Display for DurabilityLine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "error[durability_failure]: {} ({}) failed: {}; outcome unknown: re-run with the same key or check moirai changes",
            self.call,
            self.class.as_str(),
            self.code.unit(self.os)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn os_error_units() {
        assert_eq!(
            OsCode(32).unit(OsTag::Windows).to_string(),
            "os 32 ERROR_SHARING_VIOLATION"
        );
        assert_eq!(OsCode(16).unit(OsTag::Linux).to_string(), "os 16 EBUSY");
        assert_eq!(OsCode(45).unit(OsTag::MacOs).to_string(), "os 45 ENOTSUP");
        assert_eq!(
            OsCode(95).unit(OsTag::Linux).to_string(),
            "os 95 EOPNOTSUPP"
        );
        assert_eq!(OsCode(11).unit(OsTag::Linux).to_string(), "os 11 EAGAIN");
        assert_eq!(OsCode(35).unit(OsTag::MacOs).to_string(), "os 35 EAGAIN");
        assert_eq!(
            OsCode(391).unit(OsTag::Windows).to_string(),
            "os 391 ERROR_CLOUD_FILE_IN_USE"
        );
        // A code the table lacks, no code, and an unspecified OS.
        assert_eq!(OsCode(9999).unit(OsTag::Windows).to_string(), "os 9999 ?");
        assert_eq!(OsCode::NONE.unit(OsTag::Linux).to_string(), "os 0 ?");
        assert_eq!(OsCode(5).symbol(OsTag::Unspecified), None);
        assert_eq!(OsCode(-1).symbol(OsTag::Windows), None);
    }

    /// Every symbol is an ASCII identifier of the OS's family, and no two codes of one OS share a symbol.
    #[test]
    fn symbol_tables_are_well_formed() {
        for (os, prefix) in [
            (OsTag::Windows, "ERROR_"),
            (OsTag::Linux, "E"),
            (OsTag::MacOs, "E"),
        ] {
            let mut seen: Vec<&str> = Vec::new();
            for code in 0..2_000 {
                if let Some(s) = OsCode(code).symbol(os) {
                    assert!(s.starts_with(prefix), "{s}");
                    assert!(
                        s.bytes()
                            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_'),
                        "{s}"
                    );
                    assert!(!seen.contains(&s), "{s} appears twice for {os:?}");
                    seen.push(s);
                }
            }
        }
        // Linux and Darwin number the non-shared codes differently.
        assert_eq!(OsCode(39).symbol(OsTag::Linux), Some("ENOTEMPTY"));
        assert_eq!(OsCode(66).symbol(OsTag::MacOs), Some("ENOTEMPTY"));
        assert_eq!(OsCode(122).symbol(OsTag::Linux), Some("EDQUOT"));
        assert_eq!(OsCode(69).symbol(OsTag::MacOs), Some("EDQUOT"));
    }

    #[test]
    fn durability_failure_line() {
        let failure = DurabilityFailure {
            class: DurabilityClass::Durable,
            call: "NtFlushBuffersFileEx",
            kind: VfsErrorKind::DiskFull,
            os: OsCode(112),
        };
        let line = failure.stderr_line(OsTag::Windows).to_string();
        assert_eq!(
            line,
            "error[durability_failure]: NtFlushBuffersFileEx (durable) failed: os 112 ERROR_DISK_FULL; outcome unknown: \
             re-run with the same key or check moirai changes"
        );
        assert!(line.is_ascii() && !line.contains('\n'));
        let dir = DurabilityFailure {
            class: DurabilityClass::DurableName,
            call: "fsync",
            kind: VfsErrorKind::Io,
            os: OsCode(5),
        };
        assert!(
            dir.stderr_line(OsTag::Linux)
                .to_string()
                .starts_with("error[durability_failure]: fsync (durable-name) failed: os 5 EIO;")
        );
        assert_eq!(
            dir.to_string(),
            "fsync (durable-name) failed: I/O error (os 5)"
        );
    }

    #[test]
    fn vfs_errors_display() {
        let e = VfsError::new(VfsErrorKind::SharingViolation, OsCode(32), "MoveFileExW");
        assert_eq!(e.to_string(), "MoveFileExW: sharing violation (os 32)");
        assert_eq!(e.durability_line(OsTag::Windows), None);
    }

    /// [OS/fs §4.1, §6.2], [F19 §10.2]: an embedded flush failure is a `FlushFailed` error carrying the flush's code and
    /// call, and prints the `durability_failure` line with the class `durable-name` whatever class the flush had.
    #[test]
    fn an_embedded_flush_failure_is_flush_failed() {
        let f = DurabilityFailure {
            class: DurabilityClass::DurableMeta,
            call: "FlushFileBuffers",
            kind: VfsErrorKind::Io,
            os: OsCode(1117),
        };
        let e = f.embedded();
        assert_eq!(
            e,
            VfsError::new(VfsErrorKind::FlushFailed, OsCode(1117), "FlushFileBuffers")
        );
        assert_eq!(
            e.durability_line(OsTag::Windows).unwrap().to_string(),
            "error[durability_failure]: FlushFileBuffers (durable-name) failed: os 1117 ERROR_IO_DEVICE; outcome \
             unknown: re-run with the same key or check moirai changes"
        );
        assert_eq!(VfsErrorKind::FlushFailed.as_str(), "flush failed");
        // The line owns its parts: it outlives the failure it was made from.
        let line = {
            let g = DurabilityFailure {
                class: DurabilityClass::Durable,
                call: "fdatasync",
                kind: VfsErrorKind::Io,
                os: OsCode(5),
            };
            g.stderr_line(OsTag::Linux)
        };
        assert!(
            line.to_string()
                .contains("fdatasync (durable) failed: os 5 EIO;")
        );
    }

    /// The `ERROR_CLOUD_FILE_*` family is matched by name ([OS/project §2.3]): exactly the 32 codes winerror.h names, with
    /// the gaps inside 358–400 excluded.
    #[test]
    fn the_cloud_file_family_is_matched_by_name() {
        let family: Vec<i32> = (0..2_000)
            .filter(|&c| OsCode(c).is_cloud_file_error())
            .collect();
        let mut want: Vec<i32> = vec![358];
        want.extend(362..=366);
        want.extend([374, 375]);
        want.extend(377..=383);
        want.extend(386..=398);
        want.extend([404, 426, 434, 475]);
        assert_eq!(family, want);
        for gap in [359, 360, 361, 367, 373, 376, 384, 385, 399, 400] {
            assert!(!OsCode(gap).is_cloud_file_error(), "{gap}");
        }
        assert!(!OsCode(5).is_cloud_file_error());
    }
}
