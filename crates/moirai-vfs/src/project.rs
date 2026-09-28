//! Project files: the complete `ProjectFs` trait (read and write side) and the R4 evidence types `OsFileId`, `FsTime`,
//! `FileAttrs` and `VolumeCaps` ([OS/project]; X-F8).
//!
//! `ProjectFs` is the seam through which moirai touches the user's files: the trees R4 links into, the OS trash, and the
//! two store directories a file verb uses as rename targets or stamps. It is separate from `Vfs` because it has its own
//! simulator (`moirai-projfs-sim`, FL-2) and its own evidence model ([80 §2.11]). It is complete at M0 (PLAN §6.2 R2): it
//! has no `journal_since` (A1P-17; E2 is not built, a later E2 adds it additively) and no copy operation (a cross-volume
//! `file mv` is refused, [OS/project §6.4], A1P-01).
//!
//! The byte layouts of `OsFileId` (57 B), `FsTime` (9 B), `FileAttrs` (u32) and the `VolumeCaps` snapshot (16 B) are
//! given here because their values are OS-defined; [F11 §12] embeds them unchanged. All of them are runtime values:
//! never versioned, merged, exported or hashed (I-F4).

use core::hash::{Hash, Hasher};
use core::ops::{BitOr, ControlFlow};

use crate::error::{DurabilityFailure, VfsError};
use crate::fs::ShareRetry;
use crate::path::{AbsPath, CanonicalRoot, EntryName, PathError, RelPath, RelPathBuf};

/// A place for one operation: a root and a path under it; the empty path is the root itself ([OS/project §2.1]).
///
/// `path` is a `RelPath<'a>` view held by value where [OS/project §2.1] writes `&'a RelPath` (see the [`crate::path`]
/// module documentation); `At` stays two references wide and `Copy`.
pub struct At<'a, R> {
    /// The root.
    pub root: &'a R,
    /// The path under the root.
    pub path: RelPath<'a>,
}

impl<'a, R> At<'a, R> {
    /// The place `path` under `root`.
    pub const fn new(root: &'a R, path: RelPath<'a>) -> At<'a, R> {
        At { root, path }
    }

    /// The root itself.
    pub const fn root_of(root: &'a R) -> At<'a, R> {
        At {
            root,
            path: RelPath::ROOT,
        }
    }
}

// Manual impls: a derive would require `R: Copy`, and roots are handles.
impl<R> Clone for At<'_, R> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<R> Copy for At<'_, R> {}

/// The project-file seam ([OS/project §2.1]). Implemented by `moirai_os::OsProjectFs`; simulated by
/// `moirai-projfs-sim` (FL-2). Every failure is a [`VfsError`] with [OS/fs §6.2]'s mapping; a permission failure is
/// always `AccessDenied`, never absence ([OS/project §5.11]).
pub trait ProjectFs: Send + Sync + 'static {
    /// An opened directory used as the base of relative operations: a tree root, a named root, `<store>/tmp` or
    /// `<store>/trash/<intent>`. Holds no OS handle on Windows ([OS/project §2.2]); never outlives the command, hook
    /// invocation or MCP request that opened it.
    type Root: Send + Sync;
    /// One project file open for a streaming read ([OS/project §5.5]).
    type Reader: ProjectRead;

    // --- Roots, volumes and path conversions ([OS/path]) ---

    /// The canonical root of `dir` ([OS/path §4], P9).
    fn canonical_root(&self, dir: &std::path::Path) -> Result<CanonicalRoot, VfsError>;
    /// The machine-local absolute path of `p` ([OS/path §5], P12).
    fn canonical_abs(&self, p: &std::path::Path) -> Result<AbsPath, VfsError>;
    /// A CLI path argument as a path under `tree` ([OS/path §7]).
    fn cli_path(
        &self,
        arg: &std::ffi::OsStr,
        cwd: &std::path::Path,
        tree: &CanonicalRoot,
    ) -> Result<RelPathBuf, PathError>;
    /// Opens a canonical root, checking that its directory still has `root_id` (`Stale` otherwise) ([OS/project §2.2]).
    fn open_root(&self, root: &CanonicalRoot) -> Result<Self::Root, VfsError>;
    /// The volume key and capabilities of the root's volume, computed once per volume per command ([OS/project §4.1]).
    fn volume(&self, root: &Self::Root) -> Result<(VolumeKey, VolumeCaps), VfsError>;
    /// The case and normalisation equivalence one directory observes ([OS/project §4.5]).
    fn case_equivalent(&self, dir: At<'_, Self::Root>) -> Result<DirEquivalence, VfsError>;
    /// The OS trash locations relevant to a root ([OS/project §5.7]).
    fn trash_dirs(&self, root: &Self::Root) -> Result<Vec<AbsPath>, VfsError>;
    /// The effective mtime granularity of the volume that holds `stamp_dir` (`<store>/tmp/`), in ns ([OS/project §4.4]).
    fn measure_mtime_granularity(&self, stamp_dir: &Self::Root) -> Result<u64, VfsError>;

    // --- Read side ---

    /// `lstat` semantics ([OS/project §5.1]); `Absent` for "no such entry" and "a parent is not a directory".
    fn stat(&self, at: At<'_, Self::Root>, mode: StatMode) -> Result<Stat, VfsError>;
    /// The on-disk spelling of every component of `at` ([OS/project §5.3]).
    fn disk_spelling(&self, at: At<'_, Self::Root>) -> Result<RelPathBuf, VfsError>;
    /// Calls `visit` for every entry of `dir` except `.` and `..`, in the file system's order, until it returns `Break`
    /// ([OS/project §5.2]). A `RECALL_ON_DATA_ACCESS` directory is `CloudOnly`, never enumerated.
    fn enumerate<F>(&self, dir: At<'_, Self::Root>, visit: F) -> Result<EnumEnd, VfsError>
    where
        F: FnMut(&ProjEntry) -> ControlFlow<()>;
    /// Finds the object with identity `id` ([OS/project §5.4]); `recorded` is its last known attributes (a cloud-only
    /// record is `NotLocatable`, never opened).
    fn locate_id(
        &self,
        root: &Self::Root,
        id: &OsFileId,
        recorded: FileAttrs,
    ) -> Result<Located, VfsError>;
    /// The Linux `hgen` of the entry ([OS/project §3.1, §5.4]); `None` on Windows and macOS and where unsupported.
    fn file_handle_digest(&self, at: At<'_, Self::Root>) -> Result<Option<[u8; 8]>, VfsError>;
    /// Opens one project file for FL-1's streaming reader ([OS/project §5.5]): the placeholder gate, a no-follow open
    /// with no access-time update, and the containment check.
    fn read_for_hash(
        &self,
        at: At<'_, Self::Root>,
        opts: ReadOpts,
    ) -> Result<Self::Reader, VfsError>;
    /// Appends a symbolic link's target text to `out`, as git stores it ([OS/project §5.6], P8).
    fn read_link(&self, at: At<'_, Self::Root>, out: &mut Vec<u8>) -> Result<(), VfsError>;
    /// Diagnostics for a failed rename or delete of a file: the processes holding it ([OS/project §5.8]).
    fn busy_holders(&self, at: At<'_, Self::Root>) -> Result<Vec<Holder>, VfsError>;
    /// Writes one byte at offset 0 of `at` (created if absent) and returns its `mtime`; lazy ([OS/project §5.9]).
    fn touch_stamp(&self, at: At<'_, Self::Root>) -> Result<FsTime, VfsError>;

    // --- Write side ([OS/project §6]; the calls, flags, error mapping and retry bound of [OS/fs §4.7, §4.8, §6.3]) ---

    /// A no-replace rename of a file or directory between two roots on one volume; makes nothing durable
    /// ([OS/project §6.1]).
    fn rename_noreplace(
        &self,
        from: At<'_, Self::Root>,
        to: At<'_, Self::Root>,
        retry: ShareRetry,
    ) -> Result<Renamed, VfsError>;
    /// `durable-name` for a project directory ([OS/project §6.2]); idempotent; any error goes to `fail_stop`.
    fn sync_dir(&self, dir: At<'_, Self::Root>) -> Result<(), DurabilityFailure>;
    /// `rename_noreplace`, then `sync_dir` of both parents ([OS/project §6.3]).
    fn durable_rename(
        &self,
        from: At<'_, Self::Root>,
        to: At<'_, Self::Root>,
        retry: ShareRetry,
    ) -> Result<Renamed, RenameFailure>;
    /// Removes a file's name (Windows clears `FILE_ATTRIBUTE_READONLY` first and restores it if the delete fails).
    fn unlink(&self, at: At<'_, Self::Root>, retry: ShareRetry) -> Result<(), VfsError>;
    /// Removes an empty directory (`NotEmpty` otherwise).
    fn remove_dir(&self, at: At<'_, Self::Root>, retry: ShareRetry) -> Result<(), VfsError>;
    /// `unlink` (or `remove_dir` for an empty directory), then `sync_dir` of the parent ([OS/project §6.3]).
    fn durable_unlink(
        &self,
        at: At<'_, Self::Root>,
        retry: ShareRetry,
    ) -> Result<(), RenameFailure>;

    // --- Instrumentation ---

    /// Cumulative counts for the process ([OS/project §2.4]).
    fn counters(&self) -> PfsCounters;
}

/// One open project file ([OS/project §5.5]). Dropping it closes the handle.
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

/// A durable operation whose namespace step may have happened: the caller must know which ([OS/project §2.1]).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RenameFailure {
    /// Nothing changed on disk (the rename or unlink itself failed).
    NotDone(VfsError),
    /// The rename or unlink happened, then a directory flush failed: the caller passes this to `fail_stop`.
    NotDurable(DurabilityFailure),
}

/// Cumulative `ProjectFs` counts for the process ([OS/project §2.4], A1P-07).
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct PfsCounters {
    /// `rename_noreplace` and the rename step of `durable_rename`.
    pub renames: u64,
    /// `sync_dir` calls, including those inside `durable_*` (the "directory flushes" of a verb).
    pub dir_syncs: u64,
    /// `unlink`, `remove_dir` and the unlink step of `durable_unlink`.
    pub unlinks: u64,
    /// `stat` calls.
    pub stats: u64,
    /// `enumerate` calls.
    pub dir_reads: u64,
    /// `locate_id` and `file_handle_digest` calls that reached the OS.
    pub id_lookups: u64,
    /// `read_for_hash` opens.
    pub content_opens: u64,
    /// Bytes returned by `ProjectRead::read`.
    pub bytes_read: u64,
}

// ---------------------------------------------------------------------------------------------------------------------
// File identity ([OS/project §3.1, §3.2])

/// The kind of an [`OsFileId`] ([OS/project §3.1]): the tag that says how `id` is encoded. A volume never mixes kinds.
#[repr(u8)]
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum FileIdKind {
    /// No trusted id; every other field is zero.
    None = 0,
    /// Windows NTFS: `FILE_ID_128`.
    Ntfs128 = 1,
    /// Windows ReFS: `FILE_ID_128`.
    Refs128 = 2,
    /// Linux: `u64le(ino) ‖ hgen`.
    LinuxIno = 3,
    /// macOS: `u64le(fileid) ‖ 8 zero bytes`.
    DarwinFileId = 4,
}

impl FileIdKind {
    /// The kind for a byte; `None` for the reserved values 5–255.
    pub const fn from_u8(v: u8) -> Option<FileIdKind> {
        match v {
            0 => Some(FileIdKind::None),
            1 => Some(FileIdKind::Ntfs128),
            2 => Some(FileIdKind::Refs128),
            3 => Some(FileIdKind::LinuxIno),
            4 => Some(FileIdKind::DarwinFileId),
            _ => None,
        }
    }
}

/// The tagged R4 file identity ([OS/project §3.1], [F11 §12.1]; X-F8), 57 bytes little-endian, packed: `kind` u8 at 0,
/// `vol_key` `[u8; 16]` at 1, `id` `[u8; 16]` at 17, `parent` `[u8; 16]` at 33, `aux` u32 at 49 (reserved, always zero, so not a field),
/// `docid` u32 at 53.
///
/// Every value encodes canonically: a value of kind `none` encodes as 57 zero bytes whatever its other fields hold
/// ([OS/project §3.1]: "every other field is zero"), and `==` and `Hash` compare the canonical form, so such a value
/// equals [`OsFileId::NONE`] and `from_bytes(&v.to_bytes()) == Some(v)` for every `v`. Otherwise `==` compares every
/// field; object identity is [`OsFileId::same_object`], which ignores `parent` and `docid`.
#[derive(Copy, Clone, Debug)]
pub struct OsFileId {
    /// How `id` is encoded.
    pub kind: FileIdKind,
    /// The volume key ([OS/project §4.1]).
    pub vol_key: VolumeKey,
    /// The object's id, encoded per kind.
    pub id: [u8; 16],
    /// The parent directory's id in the same encoding; all zero when unknown.
    pub parent: [u8; 16],
    /// The macOS document id when owner decision #21 (d) enables it; else zero.
    pub docid: u32,
}

impl OsFileId {
    /// The encoded length.
    pub const LEN: usize = 57;

    /// Kind `none`: no trusted id.
    pub const NONE: OsFileId = OsFileId {
        kind: FileIdKind::None,
        vol_key: VolumeKey([0; 16]),
        id: [0; 16],
        parent: [0; 16],
        docid: 0,
    };

    /// `true` for kind `none` (no trusted id).
    pub const fn is_none(&self) -> bool {
        matches!(self.kind, FileIdKind::None)
    }

    /// The canonical form: [`OsFileId::NONE`] for every value of kind `none`, else the value itself.
    pub const fn canonical(self) -> OsFileId {
        if self.is_none() { Self::NONE } else { self }
    }

    /// The identity rule, frozen with X-F8 ([OS/project §3.2]): the same kind (not `none`), the same volume key and the
    /// same id. `parent` and `docid` never take part.
    pub fn same_object(&self, other: &OsFileId) -> bool {
        self.kind != FileIdKind::None
            && self.kind == other.kind
            && self.vol_key == other.vol_key
            && self.id == other.id
    }

    /// The Linux `id` encoding: `u64le(ino) ‖ hgen` ([OS/project §3.1]).
    pub fn linux_id(ino: u64, hgen: [u8; 8]) -> [u8; 16] {
        let mut id = [0u8; 16];
        id[..8].copy_from_slice(&ino.to_le_bytes());
        id[8..].copy_from_slice(&hgen);
        id
    }

    /// The macOS `id` encoding: `u64le(fileid) ‖ 8 zero bytes` ([OS/project §3.1]).
    pub fn darwin_id(fileid: u64) -> [u8; 16] {
        let mut id = [0u8; 16];
        id[..8].copy_from_slice(&fileid.to_le_bytes());
        id
    }

    /// Encodes the 57 bytes of the canonical form: `aux` is written as zero, and a value of kind `none` as 57 zero bytes.
    pub fn to_bytes(&self) -> [u8; 57] {
        let mut b = [0u8; 57];
        if self.is_none() {
            return b;
        }
        b[0] = self.kind as u8;
        b[1..17].copy_from_slice(&self.vol_key.0);
        b[17..33].copy_from_slice(&self.id);
        b[33..49].copy_from_slice(&self.parent);
        b[53..57].copy_from_slice(&self.docid.to_le_bytes());
        b
    }

    /// Decodes the 57 bytes; `None` if the value is uninterpretable — a reserved kind, kind 0 with any non-zero byte,
    /// or a non-zero `aux` — which callers treat as absent ([80 §1] X1).
    pub fn from_bytes(b: &[u8; 57]) -> Option<OsFileId> {
        let kind = FileIdKind::from_u8(b[0])?;
        if b[49..53] != [0; 4] || (kind == FileIdKind::None && b.iter().any(|&x| x != 0)) {
            return None;
        }
        let mut vol_key = [0u8; 16];
        vol_key.copy_from_slice(&b[1..17]);
        let mut id = [0u8; 16];
        id.copy_from_slice(&b[17..33]);
        let mut parent = [0u8; 16];
        parent.copy_from_slice(&b[33..49]);
        let mut docid = [0u8; 4];
        docid.copy_from_slice(&b[53..57]);
        Some(OsFileId {
            kind,
            vol_key: VolumeKey(vol_key),
            id,
            parent,
            docid: u32::from_le_bytes(docid),
        })
    }
}

impl PartialEq for OsFileId {
    fn eq(&self, other: &OsFileId) -> bool {
        let (a, b) = (self.canonical(), other.canonical());
        a.kind == b.kind
            && a.vol_key == b.vol_key
            && a.id == b.id
            && a.parent == b.parent
            && a.docid == b.docid
    }
}

impl Eq for OsFileId {}

impl Hash for OsFileId {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let c = self.canonical();
        c.kind.hash(state);
        c.vol_key.hash(state);
        c.id.hash(state);
        c.parent.hash(state);
        c.docid.hash(state);
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Timestamps and attributes ([OS/project §3.3, §3.4])

/// A file-system timestamp ([OS/project §3.3], [F11 §12.2]), 9 bytes: `ns` i64 at 0 (ns since the Unix epoch; 0 when
/// absent), `gran` u8 at 8 (the source's nominal resolution as a decimal exponent `e`, units of at most 10^e ns,
/// `e` = 0…10; `0xFF` = absent; 11…254 reserved, read as absent).
///
/// Every value encodes canonically: an absent time (a `gran` above [`FsTime::GRAN_MAX`]) encodes as
/// [`FsTime::ABSENT`]'s bytes whatever its `ns` holds ([OS/project §3.3], [F11 §12.2]: `ns` is 0 when absent), and `==`
/// and `Hash` compare the canonical form, so every absent time is equal and `from_bytes(&t.to_bytes()) == t` for every
/// `t`. Build values with [`FsTime::new`], which normalises.
#[derive(Copy, Clone, Debug)]
pub struct FsTime {
    /// Nanoseconds since 1970-01-01T00:00:00Z.
    pub ns: i64,
    /// The nominal resolution exponent, or [`FsTime::GRAN_ABSENT`].
    pub gran: u8,
}

impl FsTime {
    /// The encoded length.
    pub const LEN: usize = 9;
    /// The `gran` value of an absent time.
    pub const GRAN_ABSENT: u8 = 0xFF;
    /// The largest valid `gran`.
    pub const GRAN_MAX: u8 = 10;
    /// An absent time.
    pub const ABSENT: FsTime = FsTime {
        ns: 0,
        gran: Self::GRAN_ABSENT,
    };

    /// The time `ns` with resolution exponent `gran`; [`FsTime::ABSENT`] when `gran` is absent or reserved (above
    /// [`FsTime::GRAN_MAX`]).
    pub const fn new(ns: i64, gran: u8) -> FsTime {
        if gran > Self::GRAN_MAX {
            Self::ABSENT
        } else {
            FsTime { ns, gran }
        }
    }

    /// The canonical form: [`FsTime::ABSENT`] for every absent time, else the value itself.
    pub const fn canonical(self) -> FsTime {
        Self::new(self.ns, self.gran)
    }

    /// The Windows conversion of a `FILETIME` `f` ([OS/project §3.3]): `(f − 116 444 736 000 000 000) × 100` ns with
    /// resolution exponent `gran`; a `FILETIME` of 0, or a value outside the i64 range after conversion, is absent.
    pub fn from_filetime(f: u64, gran: u8) -> FsTime {
        if f == 0 {
            return FsTime::ABSENT;
        }
        let ns = (i128::from(f) - 116_444_736_000_000_000) * 100;
        match i64::try_from(ns) {
            Ok(ns) => FsTime::new(ns, gran),
            Err(_) => FsTime::ABSENT,
        }
    }

    /// `true` for an absent time (including a reserved `gran`).
    pub const fn is_absent(&self) -> bool {
        self.gran > Self::GRAN_MAX
    }

    /// The nominal resolution 10^gran in ns; `None` when absent.
    pub fn nominal_ns(&self) -> Option<u64> {
        if self.is_absent() {
            None
        } else {
            Some(10u64.pow(u32::from(self.gran)))
        }
    }

    /// The granularity G that [F20 §5.1] uses: `max(10^gran, VolumeCaps.mtime_granularity_ns)`; `None` when absent.
    pub fn granularity_ns(&self, measured_ns: u64) -> Option<u64> {
        self.nominal_ns().map(|n| n.max(measured_ns))
    }

    /// Encodes the 9 bytes of the canonical form (an absent time as `ns` 0 and `gran` `0xFF`).
    pub fn to_bytes(&self) -> [u8; 9] {
        let c = self.canonical();
        let mut b = [0u8; 9];
        b[..8].copy_from_slice(&c.ns.to_le_bytes());
        b[8] = c.gran;
        b
    }

    /// Decodes the 9 bytes. An absent `gran` (`0xFF`) or a reserved one (11…254) reads as [`FsTime::ABSENT`], whose `ns`
    /// is 0 whatever the stored bytes hold, so every absent time compares equal.
    pub fn from_bytes(b: &[u8; 9]) -> FsTime {
        if b[8] > Self::GRAN_MAX {
            return FsTime::ABSENT;
        }
        let mut ns = [0u8; 8];
        ns.copy_from_slice(&b[..8]);
        FsTime {
            ns: i64::from_le_bytes(ns),
            gran: b[8],
        }
    }
}

impl PartialEq for FsTime {
    fn eq(&self, other: &FsTime) -> bool {
        let (a, b) = (self.canonical(), other.canonical());
        a.ns == b.ns && a.gran == b.gran
    }
}

impl Eq for FsTime {}

impl Hash for FsTime {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let c = self.canonical();
        c.ns.hash(state);
        c.gran.hash(state);
    }
}

/// Normalised file attributes ([OS/project §3.4], [F11 §12.2]): a u32 with bits 0–10 defined and 11–31 reserved zero.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Hash)]
pub struct FileAttrs(u32);

impl FileAttrs {
    /// No attribute.
    pub const NONE: FileAttrs = FileAttrs(0);
    /// Bit 0: Windows `FILE_ATTRIBUTE_READONLY`; Unix no write bit in `st_mode & 0o222`.
    pub const READONLY: FileAttrs = FileAttrs(1 << 0);
    /// Bit 1: Windows `FILE_ATTRIBUTE_HIDDEN`; macOS `UF_HIDDEN`.
    pub const HIDDEN: FileAttrs = FileAttrs(1 << 1);
    /// Bit 2: Windows `FILE_ATTRIBUTE_REPARSE_POINT`.
    pub const REPARSE_POINT: FileAttrs = FileAttrs(1 << 2);
    /// Bit 3: Windows `FILE_ATTRIBUTE_RECALL_ON_OPEN` (cloud-only).
    pub const RECALL_ON_OPEN: FileAttrs = FileAttrs(1 << 3);
    /// Bit 4: Windows `FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS` (cloud-only).
    pub const RECALL_ON_DATA_ACCESS: FileAttrs = FileAttrs(1 << 4);
    /// Bit 5: Windows `FILE_ATTRIBUTE_OFFLINE` (cloud-only).
    pub const OFFLINE: FileAttrs = FileAttrs(1 << 5);
    /// Bit 6: macOS `SF_DATALESS` (cloud-only).
    pub const DATALESS: FileAttrs = FileAttrs(1 << 6);
    /// Bit 7: Windows `FILE_ATTRIBUTE_PINNED`.
    pub const PINNED: FileAttrs = FileAttrs(1 << 7);
    /// Bit 8: Windows `FILE_ATTRIBUTE_UNPINNED`.
    pub const UNPINNED: FileAttrs = FileAttrs(1 << 8);
    /// Bit 9: a reparse tag of the `IO_REPARSE_TAG_CLOUD` family.
    pub const CLOUD_REPARSE: FileAttrs = FileAttrs(1 << 9);
    /// Bit 10: macOS clone indicators (`EF_MAY_SHARE_BLOCKS` or a non-zero clone refcount).
    pub const CLONE_MAY_SHARE: FileAttrs = FileAttrs(1 << 10);
    /// Every defined bit.
    pub const DEFINED_BITS: u32 = (1 << 11) - 1;
    /// The cloud-only bits (3–6, [40 §4.6]).
    const CLOUD_ONLY_BITS: u32 = (1 << 3) | (1 << 4) | (1 << 5) | (1 << 6);

    /// `None` if a reserved bit is set.
    pub const fn from_bits(bits: u32) -> Option<FileAttrs> {
        if bits & !Self::DEFINED_BITS == 0 {
            Some(FileAttrs(bits))
        } else {
            None
        }
    }

    /// The defined bits of `bits`; reserved bits dropped.
    pub const fn from_bits_truncate(bits: u32) -> FileAttrs {
        FileAttrs(bits & Self::DEFINED_BITS)
    }

    /// The u32 form.
    pub const fn bits(self) -> u32 {
        self.0
    }

    /// `true` if every bit of `other` is set.
    pub const fn contains(self, other: FileAttrs) -> bool {
        self.0 & other.0 == other.0
    }

    /// Cloud-only: any of bits 3, 4, 5 or 6 ([OS/project §3.4]); such an entry is never opened or hydrated by an
    /// automatic path.
    pub const fn is_cloud_only(self) -> bool {
        self.0 & Self::CLOUD_ONLY_BITS != 0
    }
}

impl BitOr for FileAttrs {
    type Output = FileAttrs;

    fn bitor(self, rhs: FileAttrs) -> FileAttrs {
        FileAttrs(self.0 | rhs.0)
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Volumes ([OS/project §4])

/// The volume key ([OS/project §4.1]): BLAKE3-128 of `lp("moirai-vol-key-v1") ‖ lp(N) ‖ lp(S)` for one fixed source per
/// OS (computed by `moirai-os`).
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct VolumeKey(pub [u8; 16]);

/// How objects are found by id on a volume ([OS/project §4.2]).
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum IdLocate {
    /// No lookup by id.
    None = 0,
    /// `OpenFileById` (Windows) or `fsgetpath` (macOS).
    ById = 1,
    /// The changed-directory frontier (Linux).
    Frontier = 2,
}

/// The change journal a volume offers; availability only, E2 is not built ([OS/project §4.2]).
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum JournalKind {
    /// None.
    None = 0,
    /// The NTFS/ReFS USN journal.
    Usn = 1,
    /// macOS FSEvents.
    FsEvents = 2,
}

/// How far a creation time can be trusted by the copy rule ([OS/project §4.2, §8]).
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum BtimeTrust {
    /// No creation time.
    Absent = 0,
    /// Tunnelled within a window, not copied (NTFS draft).
    TunneledNotCopied = 1,
    /// Cannot be set by tools (Linux `statx` btime).
    Unforgeable = 2,
    /// Copied by clones (APFS).
    CopiedByClones = 3,
}

/// How case sensitivity is decided on a volume ([OS/project §4.2]).
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum CaseRule {
    /// Case-sensitive everywhere.
    Sensitive = 0,
    /// A per-directory flag (NTFS, ext4 casefold, f2fs).
    PerDirFlag = 1,
    /// One rule for the volume.
    Volume = 2,
}

/// How cloud placeholders show on a volume ([OS/project §4.2]).
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum CloudRule {
    /// No placeholders.
    None = 0,
    /// Windows recall attributes.
    RecallAttrs = 1,
    /// macOS dataless files.
    Dataless = 2,
}

/// How a no-replace rename works on a volume ([OS/project §4.2]).
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum RenameRule {
    /// Not supported (a macOS volume without `VOL_CAP_INT_RENAME_EXCL`).
    Unsupported = 0,
    /// A native no-replace rename.
    Native = 1,
    /// Linux `link` + `unlink` for files only.
    LinkUnlinkFiles = 2,
}

/// The capability record of a volume ([OS/project §4.2]); converts to and from the 16-byte `TREES` snapshot.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct VolumeCaps {
    /// 0–4, as [`OsFileId::kind`].
    pub id_kind: u8,
    /// Lookup by id.
    pub id_locate: IdLocate,
    /// Journal availability.
    pub journal: JournalKind,
    /// Creation-time trust.
    pub btime: BtimeTrust,
    /// Whether a same-volume rename updates `ChangeTime`; `None` = unverified.
    pub ctime_on_rename: Option<bool>,
    /// Case rule.
    pub case_rule: CaseRule,
    /// `PerDirFlag`: a directory without the flag; `Volume`: the volume. Always `false` under `Sensitive`.
    pub case_insensitive_default: bool,
    /// APFS, HFS+.
    pub norm_insensitive_always: bool,
    /// Linux casefold: normalization-insensitive exactly where case-insensitive.
    pub norm_follows_case: bool,
    /// Cloud placeholders.
    pub cloud: CloudRule,
    /// No-replace renames.
    pub rename_noreplace: RenameRule,
    /// Candidates carry clone indicators ([`FileAttrs::CLONE_MAY_SHARE`]).
    pub clone_indicators: bool,
    /// Ids survive unmount and reboot (tmpfs: false).
    pub ids_persistent: bool,
    /// macOS document ids in use (#21 (d)).
    pub docids: bool,
    /// Effective, measured granularity ([OS/project §4.4]); 0 = not measured, use the nominal.
    pub mtime_granularity_ns: u64,
}

impl VolumeCaps {
    /// The snapshot length.
    pub const SNAPSHOT_LEN: usize = 16;

    /// The 16-byte snapshot of [OS/project §4.2] ([F11 §12.3]). A value with a reserved combination (an `id_kind` above 4,
    /// `case_insensitive_default` under `Sensitive`) encodes to a snapshot that `from_snapshot` rejects.
    pub fn to_snapshot(&self) -> [u8; 16] {
        let mut flags = u32::from(self.case_insensitive_default)
            | u32::from(self.norm_insensitive_always) << 1
            | u32::from(self.norm_follows_case) << 2;
        if let Some(v) = self.ctime_on_rename {
            flags |= 1 << 3 | u32::from(v) << 4;
        }
        flags |= (self.id_locate as u32) << 5
            | (self.journal as u32) << 7
            | (self.rename_noreplace as u32) << 9
            | u32::from(self.clone_indicators) << 11
            | u32::from(self.ids_persistent) << 12
            | u32::from(self.docids) << 13;
        let mut b = [0u8; 16];
        b[..4].copy_from_slice(&flags.to_le_bytes());
        b[4] = self.id_kind;
        b[5] = self.btime as u8;
        b[6] = self.case_rule as u8;
        b[7] = self.cloud as u8;
        b[8..].copy_from_slice(&self.mtime_granularity_ns.to_le_bytes());
        b
    }

    /// Decodes a snapshot; `None` if it is uninterpretable (a reserved bit or value set, `ctime_on_rename`'s value bit
    /// without its known bit, or bit 0 set under `case_rule` 0). Its `TREES` row then behaves like a first settle.
    pub fn from_snapshot(b: &[u8; 16]) -> Option<VolumeCaps> {
        let mut f = [0u8; 4];
        f.copy_from_slice(&b[..4]);
        let flags = u32::from_le_bytes(f);
        if flags >> 14 != 0 || (flags & 1 << 4 != 0 && flags & 1 << 3 == 0) {
            return None;
        }
        let bit = |n: u32| flags & 1 << n != 0;
        let id_locate = match (flags >> 5) & 3 {
            0 => IdLocate::None,
            1 => IdLocate::ById,
            2 => IdLocate::Frontier,
            _ => return None,
        };
        let journal = match (flags >> 7) & 3 {
            0 => JournalKind::None,
            1 => JournalKind::Usn,
            2 => JournalKind::FsEvents,
            _ => return None,
        };
        let rename_noreplace = match (flags >> 9) & 3 {
            0 => RenameRule::Unsupported,
            1 => RenameRule::Native,
            2 => RenameRule::LinkUnlinkFiles,
            _ => return None,
        };
        FileIdKind::from_u8(b[4])?;
        let btime = match b[5] {
            0 => BtimeTrust::Absent,
            1 => BtimeTrust::TunneledNotCopied,
            2 => BtimeTrust::Unforgeable,
            3 => BtimeTrust::CopiedByClones,
            _ => return None,
        };
        let case_rule = match b[6] {
            0 => CaseRule::Sensitive,
            1 => CaseRule::PerDirFlag,
            2 => CaseRule::Volume,
            _ => return None,
        };
        if case_rule == CaseRule::Sensitive && bit(0) {
            return None;
        }
        let cloud = match b[7] {
            0 => CloudRule::None,
            1 => CloudRule::RecallAttrs,
            2 => CloudRule::Dataless,
            _ => return None,
        };
        let mut g = [0u8; 8];
        g.copy_from_slice(&b[8..]);
        Some(VolumeCaps {
            id_kind: b[4],
            id_locate,
            journal,
            btime,
            ctime_on_rename: if bit(3) { Some(bit(4)) } else { None },
            case_rule,
            case_insensitive_default: bit(0),
            norm_insensitive_always: bit(1),
            norm_follows_case: bit(2),
            cloud,
            rename_noreplace,
            clone_indicators: bit(11),
            ids_persistent: bit(12),
            docids: bit(13),
            mtime_granularity_ns: u64::from_le_bytes(g),
        })
    }
}

/// The equivalence one directory observes ([OS/project §4.5]); observed at resolve time, never baked into keys.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct DirEquivalence {
    /// Names equal under case folding name one entry.
    pub case_insensitive: bool,
    /// Names equal under normalisation name one entry.
    pub norm_insensitive: bool,
}

// ---------------------------------------------------------------------------------------------------------------------
// Read side ([OS/project §5])

/// Which `stat` to make ([OS/project §5.1]).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum StatMode {
    /// The read path: the cheapest call; an id only where it comes with the call.
    Read,
    /// With the object's id wherever the volume has ids.
    WithId,
}

/// The result of `stat` ([OS/project §5.1]).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Stat {
    /// The entry exists.
    Present(StatRec),
    /// No such entry, or a parent is not a directory.
    Absent,
}

/// The kind of a project entry ([OS/project §5.1]); unlike the store's `EntryKind` it has a symbolic-link kind.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ProjKind {
    /// A regular file.
    File,
    /// A directory.
    Dir,
    /// A symbolic link (Windows `IO_REPARSE_TAG_SYMLINK`).
    Symlink,
    /// Anything else: junctions, WSL links, app-execution aliases, devices. Never read, followed or a candidate.
    Other,
}

/// What `stat` reports ([OS/project §5.1]).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct StatRec {
    /// The entry's kind.
    pub kind: ProjKind,
    /// The size in bytes.
    pub size: u64,
    /// Last write.
    pub mtime: FsTime,
    /// Last change (absent from a Windows `Read`-mode stat).
    pub ctime: FsTime,
    /// Creation.
    pub btime: FsTime,
    /// macOS date added.
    pub added: FsTime,
    /// Normalised attributes.
    pub attrs: FileAttrs,
    /// Windows reparse tag; 0 elsewhere or when not a reparse point.
    pub reparse_tag: u32,
    /// The identity, with `parent` filled when known.
    pub id: Option<OsFileId>,
    /// Link count; 0 = unknown.
    pub nlink: u32,
}

/// One entry of an enumeration ([OS/project §5.2]).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjEntry {
    /// The name as the OS returned it ([OS/path §2.4]).
    pub name: EntryName,
    /// The entry's kind.
    pub kind: ProjKind,
    /// `Some` where the enumeration call returns the attributes (Windows, macOS).
    pub stat: Option<StatRec>,
    /// Linux `d_ino`, for the frontier; `None` elsewhere.
    pub ino_hint: Option<u64>,
}

/// How an enumeration ended ([OS/project §5.2]).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum EnumEnd {
    /// Every entry was visited.
    Complete,
    /// `visit` returned `Break`.
    Stopped,
}

/// Where `locate_id` found an object ([OS/project §5.4]).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Located {
    /// Under the root passed in.
    InRoot(RelPathBuf),
    /// Under one of `trash_dirs()`.
    InTrash(AbsPath),
    /// On the volume, outside the root and the trash.
    Elsewhere(AbsPath),
    /// No object has this id on its volume.
    Gone,
    /// This volume has no by-id lookup, the id is on another volume, or the record shows the object was cloud-only.
    NotLocatable,
}

/// Options of `read_for_hash` ([OS/project §5.5]).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct ReadOpts {
    /// `true` only for explicit verbs given `--allow-hydrate`.
    pub allow_hydrate: bool,
}

/// Size and last-write time read through an open handle ([OS/project §5.5]).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct ReadSnapshot {
    /// The size in bytes.
    pub size: u64,
    /// Last write.
    pub mtime: FsTime,
}

/// A process holding a file open ([OS/project §5.8]).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Holder {
    /// The process id.
    pub pid: u32,
    /// The application name as the OS reports it (ASCII-escaped by the renderer).
    pub name: Box<str>,
}

// ---------------------------------------------------------------------------------------------------------------------
// Write side ([OS/project §6])

/// How a no-replace rename happened ([OS/project §6.1]).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Renamed {
    /// One rename.
    Renamed,
    /// Linux's file fallback (`linkat` + `unlinkat`): the crash state "both names, one inode" is an `FsIntent`
    /// recovery state.
    LinkedThenUnlinked,
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn hash_of<T: Hash>(v: &T) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        v.hash(&mut h);
        h.finish()
    }

    fn caps() -> VolumeCaps {
        VolumeCaps {
            id_kind: 1,
            id_locate: IdLocate::ById,
            journal: JournalKind::Usn,
            btime: BtimeTrust::TunneledNotCopied,
            ctime_on_rename: Some(true),
            case_rule: CaseRule::PerDirFlag,
            case_insensitive_default: true,
            norm_insensitive_always: false,
            norm_follows_case: false,
            cloud: CloudRule::RecallAttrs,
            rename_noreplace: RenameRule::Native,
            clone_indicators: false,
            ids_persistent: true,
            docids: false,
            mtime_granularity_ns: 15_625_000,
        }
    }

    #[test]
    fn ntfs_snapshot_bytes() {
        let b = caps().to_snapshot();
        // flags: bit 0, bit 3 + bit 4, id_locate 1 << 5, journal 1 << 7, rename 1 << 9, ids_persistent 1 << 12.
        let flags: u32 = 1 | 1 << 3 | 1 << 4 | 1 << 5 | 1 << 7 | 1 << 9 | 1 << 12;
        assert_eq!(b[..4], flags.to_le_bytes());
        assert_eq!(b[4..8], [1, 1, 1, 1]);
        assert_eq!(b[8..], 15_625_000u64.to_le_bytes());
        assert_eq!(VolumeCaps::from_snapshot(&b), Some(caps()));
    }

    #[test]
    fn uninterpretable_snapshots() {
        let good = caps().to_snapshot();
        let mut b = good;
        b[3] = 0x80; // a reserved flag bit
        assert_eq!(VolumeCaps::from_snapshot(&b), None);
        b = good;
        b[0] &= !(1 << 3); // the value bit of ctime_on_rename without its known bit
        assert_eq!(VolumeCaps::from_snapshot(&b), None);
        b = good;
        b[4] = 5;
        assert_eq!(VolumeCaps::from_snapshot(&b), None);
        b = good;
        b[6] = 0; // sensitive, with bit 0 set
        assert_eq!(VolumeCaps::from_snapshot(&b), None);
        b = good;
        b[0] |= 3 << 5; // id_locate 3
        assert_eq!(VolumeCaps::from_snapshot(&b), None);
    }

    #[test]
    fn os_file_id_rules() {
        let a = OsFileId {
            kind: FileIdKind::Ntfs128,
            vol_key: VolumeKey([7; 16]),
            id: [1; 16],
            parent: [2; 16],
            docid: 0,
        };
        let moved = OsFileId {
            parent: [9; 16],
            ..a
        };
        assert!(a.same_object(&moved), "parent never takes part");
        assert!(!a.same_object(&OsFileId {
            vol_key: VolumeKey([8; 16]),
            ..a
        }));
        assert!(!a.same_object(&OsFileId {
            kind: FileIdKind::Refs128,
            ..a
        }));
        assert!(
            !OsFileId::NONE.same_object(&OsFileId::NONE),
            "kind none is never an identity"
        );
        let b = a.to_bytes();
        assert_eq!(b[0], 1);
        assert_eq!(b[1..17], [7; 16]);
        assert_eq!(b[49..53], [0; 4]);
        assert_eq!(OsFileId::from_bytes(&b), Some(a));
        let mut bad = b;
        bad[50] = 1; // aux
        assert_eq!(OsFileId::from_bytes(&bad), None);
        bad = b;
        bad[0] = 5;
        assert_eq!(OsFileId::from_bytes(&bad), None);
        let mut none = OsFileId::NONE.to_bytes();
        assert_eq!(OsFileId::from_bytes(&none), Some(OsFileId::NONE));
        none[20] = 1;
        assert_eq!(
            OsFileId::from_bytes(&none),
            None,
            "kind 0 with a non-zero byte"
        );
        // A value of kind `none` built with stray fields encodes as NONE and compares equal to it.
        let stray = OsFileId {
            kind: FileIdKind::None,
            id: [3; 16],
            docid: 9,
            ..a
        };
        assert_eq!(stray.to_bytes(), [0; 57]);
        assert_eq!(stray, OsFileId::NONE);
        assert_eq!(hash_of(&stray), hash_of(&OsFileId::NONE));
        assert_eq!(OsFileId::from_bytes(&stray.to_bytes()), Some(stray));
        assert_ne!(a, OsFileId { docid: 1, ..a }, "`==` compares every field");
        assert_eq!(OsFileId::linux_id(0x0102, [9; 8])[..2], [2, 1]);
        assert_eq!(OsFileId::darwin_id(5)[8..], [0; 8]);
    }

    #[test]
    fn fs_times_and_attrs() {
        // 1970-01-01 is FILETIME 116444736000000000.
        assert_eq!(
            FsTime::from_filetime(116_444_736_000_000_000, 2),
            FsTime { ns: 0, gran: 2 }
        );
        assert_eq!(FsTime::from_filetime(116_444_736_000_000_001, 2).ns, 100);
        assert_eq!(FsTime::from_filetime(0, 2), FsTime::ABSENT);
        assert_eq!(
            FsTime::from_filetime(u64::MAX, 2),
            FsTime::ABSENT,
            "outside the i64 range"
        );
        assert_eq!(
            FsTime::from_filetime(116_444_736_000_000_000 - 10_000_000, 2).ns,
            -1_000_000_000
        );
        assert_eq!(
            FsTime::from_filetime(1, 2),
            FsTime::ABSENT,
            "1601 is outside the i64 nanosecond range"
        );
        let t = FsTime { ns: -5, gran: 7 };
        assert_eq!(FsTime::from_bytes(&t.to_bytes()), t);
        assert_eq!(t.granularity_ns(15_625_000), Some(15_625_000));
        assert_eq!(t.granularity_ns(0), Some(10_000_000));
        let mut reserved = t.to_bytes();
        reserved[8] = 11;
        assert_eq!(FsTime::from_bytes(&reserved), FsTime::ABSENT);
        reserved[8] = FsTime::GRAN_ABSENT;
        assert_eq!(
            FsTime::from_bytes(&reserved),
            FsTime::ABSENT,
            "an absent time decodes with ns 0"
        );
        assert_eq!(
            FsTime::from_bytes(&FsTime::ABSENT.to_bytes()),
            FsTime::ABSENT
        );
        assert!(FsTime::ABSENT.is_absent() && FsTime::ABSENT.nominal_ns().is_none());
        // An absent time built with a stray `ns`, or with a reserved `gran`, encodes as ABSENT and compares equal to it.
        for stray in [FsTime { ns: 5, gran: 0xFF }, FsTime { ns: -1, gran: 11 }] {
            assert_eq!(stray.to_bytes(), FsTime::ABSENT.to_bytes());
            assert_eq!(stray, FsTime::ABSENT);
            assert_eq!(hash_of(&stray), hash_of(&FsTime::ABSENT));
        }
        assert_eq!(FsTime::new(5, 0xFF).ns, 0);
        assert_eq!(FsTime::new(5, 3), FsTime { ns: 5, gran: 3 });
        assert_ne!(FsTime::new(5, 3), FsTime::new(6, 3));
        assert_eq!(
            FsTime::from_filetime(116_444_736_000_000_000, 12),
            FsTime::ABSENT
        );

        let a = FileAttrs::READONLY | FileAttrs::RECALL_ON_OPEN;
        assert!(a.contains(FileAttrs::READONLY) && a.is_cloud_only());
        assert!(!(FileAttrs::PINNED | FileAttrs::CLOUD_REPARSE).is_cloud_only());
        assert_eq!(FileAttrs::from_bits(1 << 11), None);
        assert_eq!(
            FileAttrs::from_bits_truncate(u32::MAX).bits(),
            FileAttrs::DEFINED_BITS
        );
    }

    /// A snapshot whose flag word has the 14 defined bits random and, one time in ten, one reserved bit; whose bytes 4–7
    /// are each drawn from the field's valid range plus one reserved value; and whose granularity is any u64.
    fn near_valid_snapshot() -> impl Strategy<Value = [u8; 16]> {
        let reserved = prop_oneof![9 => Just(0u32), 1 => (14u32..32).prop_map(|n| 1u32 << n)];
        (
            any::<u16>(),
            reserved,
            0u8..=5,
            0u8..=4,
            0u8..=3,
            0u8..=3,
            any::<u64>(),
        )
            .prop_map(|(defined, reserved, id_kind, btime, case, cloud, gran)| {
                let flags = (u32::from(defined) & 0x3FFF) | reserved;
                let mut b = [0u8; 16];
                b[..4].copy_from_slice(&flags.to_le_bytes());
                b[4] = id_kind;
                b[5] = btime;
                b[6] = case;
                b[7] = cloud;
                b[8..].copy_from_slice(&gran.to_le_bytes());
                b
            })
    }

    /// An encoding with kind 0–5; `aux` zero nine times in ten; for kind 0, an all-zero body half the time.
    fn near_valid_os_file_id() -> impl Strategy<Value = [u8; 57]> {
        let aux = prop_oneof![9 => Just(0u32), 1 => 1u32..];
        (
            0u8..=5,
            proptest::collection::vec(any::<u8>(), 52),
            aux,
            any::<bool>(),
        )
            .prop_map(|(kind, body, aux, zero_body)| {
                let mut b = [0u8; 57];
                b[0] = kind;
                if !(kind == 0 && zero_body) {
                    b[1..49].copy_from_slice(&body[..48]);
                    b[53..57].copy_from_slice(&body[48..]);
                }
                b[49..53].copy_from_slice(&aux.to_le_bytes());
                b
            })
    }

    fn arb_caps() -> impl Strategy<Value = VolumeCaps> {
        (
            (
                0u8..=4,
                0u8..3,
                0u8..3,
                0u8..4,
                proptest::option::of(any::<bool>()),
                0u8..3,
                any::<bool>(),
            ),
            (
                any::<bool>(),
                any::<bool>(),
                0u8..3,
                0u8..3,
                any::<bool>(),
                any::<bool>(),
                any::<bool>(),
                any::<u64>(),
            ),
        )
            .prop_map(
                |(
                    (id_kind, loc, jr, bt, ctime, case, cid),
                    (nia, nfc, cl, rn, clone, persist, docids, gran),
                )| {
                    let case_rule = [CaseRule::Sensitive, CaseRule::PerDirFlag, CaseRule::Volume]
                        [usize::from(case)];
                    VolumeCaps {
                        id_kind,
                        id_locate: [IdLocate::None, IdLocate::ById, IdLocate::Frontier]
                            [usize::from(loc)],
                        journal: [JournalKind::None, JournalKind::Usn, JournalKind::FsEvents]
                            [usize::from(jr)],
                        btime: [
                            BtimeTrust::Absent,
                            BtimeTrust::TunneledNotCopied,
                            BtimeTrust::Unforgeable,
                            BtimeTrust::CopiedByClones,
                        ][usize::from(bt)],
                        ctime_on_rename: ctime,
                        case_rule,
                        case_insensitive_default: cid && case_rule != CaseRule::Sensitive,
                        norm_insensitive_always: nia,
                        norm_follows_case: nfc,
                        cloud: [CloudRule::None, CloudRule::RecallAttrs, CloudRule::Dataless]
                            [usize::from(cl)],
                        rename_noreplace: [
                            RenameRule::Unsupported,
                            RenameRule::Native,
                            RenameRule::LinkUnlinkFiles,
                        ][usize::from(rn)],
                        clone_indicators: clone,
                        ids_persistent: persist,
                        docids,
                        mtime_granularity_ns: gran,
                    }
                },
            )
    }

    proptest! {
        #![proptest_config(crate::testing::proptest_config())]

        /// "`VolumeCaps` converts to and from the snapshot losslessly" ([OS/project §4.2]).
        #[test]
        fn volume_caps_snapshot_is_lossless(c in arb_caps()) {
            prop_assert_eq!(VolumeCaps::from_snapshot(&c.to_snapshot()), Some(c));
        }

        /// Over near-valid snapshots (each field drawn from its valid range plus one reserved value, a reserved flag bit
        /// now and then): a snapshot decodes exactly when no reserved field or combination is set, and then re-encodes
        /// to the same bytes.
        #[test]
        fn volume_caps_decoding_is_canonical(b in near_valid_snapshot()) {
            let mut f = [0u8; 4];
            f.copy_from_slice(&b[..4]);
            let flags = u32::from_le_bytes(f);
            let field = |shift: u32| (flags >> shift) & 3;
            let valid = flags >> 14 == 0
                && !(flags & 1 << 4 != 0 && flags & 1 << 3 == 0)
                && field(5) != 3
                && field(7) != 3
                && field(9) != 3
                && b[4] <= 4
                && b[5] <= 3
                && b[6] <= 2
                && b[7] <= 2
                && !(b[6] == 0 && flags & 1 != 0);
            let decoded = VolumeCaps::from_snapshot(&b);
            prop_assert_eq!(decoded.is_some(), valid, "snapshot {:?}", b);
            if let Some(c) = decoded {
                prop_assert_eq!(c.to_snapshot(), b);
            }
        }

        /// Over near-valid encodings (kinds 0–5, `aux` mostly zero, kind 0 with an all-zero body half the time): an
        /// encoding decodes exactly when its kind is defined, `aux` is zero and kind 0 has every other byte zero, and then
        /// re-encodes to the same bytes.
        #[test]
        fn os_file_id_decoding_is_canonical(b in near_valid_os_file_id()) {
            let kind = b[0];
            let valid = kind <= 4 && b[49..53] == [0; 4] && (kind != 0 || b.iter().all(|&x| x == 0));
            let decoded = OsFileId::from_bytes(&b);
            prop_assert_eq!(decoded.is_some(), valid, "encoding {:?}", b);
            if let Some(id) = decoded {
                prop_assert_eq!(id.to_bytes(), b);
            }
        }

        /// Every value, whatever its fields, encodes canonically and decodes back to a value equal to it.
        #[test]
        fn every_os_file_id_round_trips(kind in 0u8..=4, vol in any::<[u8; 16]>(), id in any::<[u8; 16]>(),
                                        parent in any::<[u8; 16]>(), docid in any::<u32>()) {
            let v = OsFileId { kind: FileIdKind::from_u8(kind).unwrap(), vol_key: VolumeKey(vol), id, parent, docid };
            let bytes = v.to_bytes();
            prop_assert_eq!(OsFileId::from_bytes(&bytes), Some(v));
            prop_assert_eq!(bytes == [0; 57], kind == 0);
        }

        /// Every time, whatever its fields, encodes canonically and decodes back to a value equal to it; every absent
        /// time encodes as `ABSENT`.
        #[test]
        fn every_fs_time_round_trips(ns in any::<i64>(), gran in any::<u8>()) {
            let t = FsTime { ns, gran };
            let bytes = t.to_bytes();
            prop_assert_eq!(FsTime::from_bytes(&bytes), t);
            prop_assert_eq!(t.is_absent(), bytes == FsTime::ABSENT.to_bytes());
            prop_assert_eq!(t == FsTime::ABSENT, gran > FsTime::GRAN_MAX);
        }

        #[test]
        fn os_file_id_round_trips(kind in 1u8..=4, vol in any::<[u8; 16]>(), id in any::<[u8; 16]>(),
                                  parent in any::<[u8; 16]>(), docid in any::<u32>()) {
            let v = OsFileId { kind: FileIdKind::from_u8(kind).unwrap(), vol_key: VolumeKey(vol), id, parent, docid };
            prop_assert_eq!(OsFileId::from_bytes(&v.to_bytes()), Some(v));
        }
    }
}
