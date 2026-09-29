//! The store file system: the `StoreFs` sub-trait of `Vfs` and its types ([OS/fs §2, §3]), with the durability classes
//! and namespace operations of [F15 §4, §5].
//!
//! `StoreFs` serves every file under the store directory, every file of an image destination or backup directory, and
//! the store directory's own parent ([OS/fs §1]). Project files never go through it ([`crate::ProjectFs`]).
//!
//! Durability, in the terms of the fault model ([F15 §4.1]):
//!
//! | Class | Call | Guarantee |
//! |---|---|---|
//! | `lazy` | `write_at` only | visible to every client of every process once the write returns; survives any process death; durable at the next successful covering flush by any client; may be lost at a system crash or through a failed flush in any process |
//! | `durable` | `sync(Data)` | every covered write below the durable size is on stable media when the call returns success (FM-2.1) |
//! | `durable+meta` | `sync(DataAndMeta)` | `durable`, plus the file's size and allocation (FM-2.2) |
//! | `durable-name` | `sync_dir(dir)` | every create, rename or unlink in `dir` whose effect instant preceded the call becomes durable once its other conditions hold (FM-2.3) |
//! | `sync_group` | `sync_group(members)` | every member durable by its class when the call returns success (FM-2.6) |
//!
//! A class the location cannot provide refuses the store; no call is ever replaced by a weaker one ([80 §1] X5). Any error
//! from a class other than `lazy` is a [`DurabilityFailure`] that goes to [`StoreFs::fail_stop`] ([OS/fs §4.4.5]).

use crate::env::StoreVolume;
use crate::error::{DurabilityFailure, VfsError};
use crate::path::{EntryName, RelPath};

/// Handle types shared by every sub-trait of `Vfs` ([OS/README §4.1]). All handles are owned values; dropping one closes
/// it.
pub trait VfsTypes {
    /// An open directory that relative operations are based on ([OS/fs §2.2]).
    type Root: Send + Sync;
    /// An open file ([OS/fs §2.3]): positional only, usable from several threads.
    type File: Send + Sync;
}

/// The role of a root ([OS/fs §2.2]).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum RootRole {
    /// A store directory: the environment guard classifies it ([OS/env §4]) and extents may be created in it.
    Store,
    /// Any other directory: an image destination, a backup directory, the parent of a store directory.
    Other,
}

/// What a root may be used for ([OS/fs §2.2]).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum RootAccess {
    /// Readers: open, read, list, identity. No create, write, sync, rename or unlink.
    Read,
    /// Writers, maintenance, `init`, `restore`, exports, backups.
    ReadWrite,
}

/// The access of an open file ([OS/fs §2.3]).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Access {
    /// Read only.
    Read,
    /// Read and write; requires a root opened `ReadWrite`.
    ReadWrite,
}

/// An open hint ([OS/fs §2.3, §4.12]).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum OpenHint {
    /// No hint.
    Normal,
    /// Bulk passes (rollup, backup, full export, retirement, repair, `links check --all`): sequential read-ahead and no
    /// cache retention ([80 §2.12] "Bulk passes").
    Sequential,
}

/// The durability classes of [80 §2.3.1] ([OS/fs §2.4], [F15 §4.1]). `Lazy` has no call: a write is lazy until a
/// covering flush.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum DurabilityClass {
    /// `write_at` only.
    Lazy,
    /// `sync(Data)`.
    Durable,
    /// `sync(DataAndMeta)`.
    DurableMeta,
    /// `sync_dir`.
    DurableName,
    /// `sync_group`.
    SyncGroup,
}

impl DurabilityClass {
    /// The class's name as [80 §2.3.1] and [F15 §4.1] spell it: `lazy`, `durable`, `durable+meta`, `durable-name`,
    /// `sync_group`; the `<class>` of `fail_stop`'s line ([F19 §10.2] row `durability_failure`).
    pub const fn as_str(self) -> &'static str {
        match self {
            DurabilityClass::Lazy => "lazy",
            DurabilityClass::Durable => "durable",
            DurabilityClass::DurableMeta => "durable+meta",
            DurabilityClass::DurableName => "durable-name",
            DurabilityClass::SyncGroup => "sync_group",
        }
    }
}

/// What `StoreFs::sync` makes durable ([OS/fs §2.4, §4.4.2]).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum SyncKind {
    /// Class `durable`: the file's data within its current size.
    Data,
    /// Class `durable+meta`: data plus the file's size and allocation.
    DataAndMeta,
}

impl SyncKind {
    /// The durability class this kind gives.
    pub const fn class(self) -> DurabilityClass {
        match self {
            SyncKind::Data => DurabilityClass::Durable,
            SyncKind::DataAndMeta => DurabilityClass::DurableMeta,
        }
    }
}

/// One member of a `sync_group` (class `sync_group`, [OS/fs §2.4, §4.4.4]).
pub enum GroupMember<'a, R, F> {
    /// A file, made durable by `kind`.
    File {
        /// The file; its handle must have write access.
        file: &'a F,
        /// The class the member needs.
        kind: SyncKind,
    },
    /// A directory, made `durable-name`. `dir: None` names the root directory itself.
    Dir {
        /// The root the directory is relative to.
        root: &'a R,
        /// The directory under `root`, or the root itself (a view, by value: see the [`crate::path`] module
        /// documentation).
        dir: Option<RelPath<'a>>,
    },
}

impl<R, F> Clone for GroupMember<'_, R, F> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<R, F> Copy for GroupMember<'_, R, F> {}

/// The retry policy for Windows sharing violations (errors 5 and 32) on renames and deletes ([OS/fs §2.5, §6.3]). It is
/// ignored on Linux and macOS. A caller that holds the writer or flush byte always passes `None` ([OS/fs §6.3]).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ShareRetry {
    /// One attempt. Used by GC deletes, which retry at the next GC ([AR §4.1]).
    None,
    /// Retry sharing violations for at most `total_ms` of monotonic time.
    Bounded {
        /// The bound, in milliseconds of monotonic time since the first attempt.
        total_ms: u32,
    },
}

/// HOLE(OS-share-retry-ms) ([OS/fs §6.3] and its hole table), in milliseconds: the `total_ms` of
/// [`ShareRetry::Bounded`] for image-export renames, `packed-refs` and loose-ref replace-renames, the store `config`
/// rename and the clean-up unlinks of the `init` probe ([OS/env §5] step 6). The draft value is 1,000 ms (the
/// `--retry-ms` default that `file mv` gets, [40 §3.4]); WP-81a fills the hole from measurements 8 and 15, here and only
/// here, since the OS layer and the simulator both read this constant.
pub const OS_SHARE_RETRY_MS: u32 = 1_000;

impl ShareRetry {
    /// The longest single sleep of the schedule ([OS/fs §6.3] step 2).
    pub const MAX_SLEEP_MS: u32 = 64;

    /// The schedule of [OS/fs §6.3]: after `retries` failed re-attempts (0 after the first attempt) and `elapsed_ms` of
    /// monotonic time since the first attempt, the next sleep before re-attempting — 1, 2, 4, 8, 16, 32 ms, then 64 ms
    /// per step — or `None` when the elapsed time plus that sleep would exceed the bound (the caller then returns the
    /// last error). Always `None` for [`ShareRetry::None`].
    pub const fn next_sleep_ms(self, retries: u32, elapsed_ms: u64) -> Option<u32> {
        match self {
            ShareRetry::None => None,
            ShareRetry::Bounded { total_ms } => {
                let sleep = if retries >= 6 {
                    Self::MAX_SLEEP_MS
                } else {
                    1 << retries
                };
                if elapsed_ms.saturating_add(sleep as u64) > total_ms as u64 {
                    None
                } else {
                    Some(sleep)
                }
            }
        }
    }
}

/// A process-local identity of a file or directory ([OS/fs §2.6]): two handles name the same object iff their
/// identities are equal. Not the R4 `OsFileId` ([`crate::OsFileId`]); never versioned, hashed or exported. Used by the
/// `LOCK` identity check ([OS/lock §9]) and stored in exactly one place, the swap intent ([OS/fs §4.9.3]).
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct FileIdentity {
    /// Windows `FILE_ID_INFO.VolumeSerialNumber`; Linux and macOS `st_dev` zero-extended.
    pub volume: u64,
    /// Windows `FILE_ID_INFO.FileId` as returned; Linux and macOS `st_ino` as u64 little-endian in bytes 0–7, then zero.
    pub file: [u8; 16],
}

impl FileIdentity {
    /// The encoded length ([OS/fs §2.6]).
    pub const LEN: usize = 24;

    /// The Unix form: `st_dev` and `st_ino` ([OS/fs §2.6]).
    pub const fn from_dev_ino(dev: u64, ino: u64) -> FileIdentity {
        let ino = ino.to_le_bytes();
        let mut file = [0u8; 16];
        let mut i = 0;
        while i < 8 {
            file[i] = ino[i];
            i += 1;
        }
        FileIdentity { volume: dev, file }
    }

    /// The 24-byte little-endian form of [OS/fs §2.6]: `volume` at 0, `file` at 8.
    pub fn to_bytes(&self) -> [u8; 24] {
        let mut out = [0u8; 24];
        out[..8].copy_from_slice(&self.volume.to_le_bytes());
        out[8..].copy_from_slice(&self.file);
        out
    }

    /// Decodes the 24-byte form; every bit pattern is a value.
    pub fn from_bytes(b: &[u8; 24]) -> FileIdentity {
        let mut volume = [0u8; 8];
        volume.copy_from_slice(&b[..8]);
        let mut file = [0u8; 16];
        file.copy_from_slice(&b[8..]);
        FileIdentity {
            volume: u64::from_le_bytes(volume),
            file,
        }
    }
}

/// Free and total bytes of a volume ([OS/fs §2.7, §4.11]).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct FreeSpace {
    /// Bytes available to this user (quotas applied where the OS applies them).
    pub available: u64,
    /// Total bytes of the volume.
    pub total: u64,
}

/// One entry of a store directory ([OS/fs §2.7]); `.` and `..` are never reported.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DirEntry {
    /// [OS/path §2.4]: `Utf8` when the name is valid Unicode and passes P4, else `Unrepresentable` with the OS bytes.
    pub name: EntryName,
    /// The entry's kind.
    pub kind: EntryKind,
}

/// The kind of a store directory entry ([OS/fs §2.7]).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum EntryKind {
    /// A regular file.
    File,
    /// A directory.
    Dir,
    /// Anything else (a link, a device).
    Other,
}

/// Process-wide instrumentation, monotonic, read with relaxed atomics ([OS/fs §2.7, §4.13]; [60 §5.1] "Counts",
/// A1P-07). Every operation increments its counter exactly once per OS call it issues. The simulator keeps the same
/// counters per simulated process.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct VfsCounters {
    /// Opens of existing files.
    pub opens: u64,
    /// Exclusive creates of files and directories.
    pub creates: u64,
    /// Bytes returned by reads.
    pub bytes_read: u64,
    /// Bytes written.
    pub bytes_written: u64,
    /// Class `durable` calls (every one is a log-extent data flush in the protocol).
    pub sync_data: u64,
    /// Class `durable+meta` calls.
    pub sync_meta: u64,
    /// Class `durable-name` calls, including the directory members of `sync_group`.
    pub sync_dir: u64,
    /// macOS only: `F_FULLFSYNC` issued as the device barrier of `sync_dir` or `sync_group`; 0 elsewhere.
    pub full_barriers: u64,
    /// Renames (each rename call, including the steps of `swap_dirs` and `swap_recover`).
    pub renames: u64,
    /// Unlinks.
    pub unlinks: u64,
    /// Sealed-file mappings created.
    pub maps: u64,
    /// Bytes mapped by those mappings.
    pub mapped_bytes: u64,
    /// Attempts beyond the first made under `ShareRetry::Bounded`.
    pub share_retries: u64,
}

/// The result of `swap_dirs` ([OS/fs §4.9]).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum SwapOutcome {
    /// The native exchange (Linux `RENAME_EXCHANGE`, macOS `RENAME_SWAP`), [OS/fs §4.9.1].
    Exchanged,
    /// The renames guarded by the swap intent ([OS/fs §4.9.2]; three renames, [F15 §5.6]).
    TwoRenames,
}

/// The result of `swap_recover` ([OS/fs §4.9.4]).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum SwapRecovery {
    /// No intent file exists.
    NoIntent,
    /// The intent existed but nothing had been renamed; the intent was removed.
    NothingDone,
    /// The swap was undone: `a` holds what it held before.
    RolledBack,
    /// The swap was finished: `a` holds what `b` held.
    Completed,
}

/// The store-file sub-trait of `Vfs` ([OS/fs §3]). Every operation on a file names a root and a [`RelPath`] and resolves
/// the path relative to the root's handle, never through an absolute path string ([80 §2.10] P10, [OS/fs §2.2]).
///
/// Paths are passed by value as `RelPath<'_>` views where [OS/fs §3] writes `&RelPath` (see the [`crate::path`] module
/// documentation). `dir: None` and the empty `RelPath` both name the root ([OS/fs §2.1]).
pub trait StoreFs: VfsTypes {
    // Roots ([OS/fs §4.1])

    /// Opens the absolute directory `dir` with `role` and `access`. A root of role `Store` is classified by the caller
    /// through `EnvGuard::classify` before any other use. A missing directory is `NotFound`.
    fn open_root(
        &self,
        dir: &std::path::Path,
        role: RootRole,
        access: RootAccess,
    ) -> Result<Self::Root, VfsError>;

    /// Creates the directory `dir` (its parent must exist), makes the creation durable (`durable-name` on the parent),
    /// and returns it as `open_root(dir, role, ReadWrite)` would. Role `Store` on Windows adds the owner ACE of
    /// [90 §5.4]. An existing `dir` is `AlreadyExists`. If the embedded `durable-name` flush fails, the new, empty
    /// directory is removed (the removal's own error ignored) and the error is [`VfsErrorKind::FlushFailed`] with the
    /// flush's code and call ([OS/fs §4.1]): the caller exits 7 and issues no further write, flush, create or namespace
    /// call. A failed directory creation may leave the empty directory in place ([F15 §5.2], NS-4).
    ///
    /// [`VfsErrorKind::FlushFailed`]: crate::VfsErrorKind::FlushFailed
    fn create_root(&self, dir: &std::path::Path, role: RootRole) -> Result<Self::Root, VfsError>;

    // Names ([OS/fs §4.2])

    /// Opens an existing regular file. `ReadWrite` requires a root opened `ReadWrite`. A delete-pending file is
    /// `DeletePending` ([OS/fs §6.4]).
    fn open(
        &self,
        root: &Self::Root,
        rel: RelPath<'_>,
        access: Access,
        hint: OpenHint,
    ) -> Result<Self::File, VfsError>;

    /// Creates an empty regular file that must not exist (`AlreadyExists` otherwise) and opens it `ReadWrite`. Its name
    /// is not durable until `sync_dir` of its parent (FM-2.3).
    fn create_new(&self, root: &Self::Root, rel: RelPath<'_>) -> Result<Self::File, VfsError>;

    /// Creates one directory level (`AlreadyExists` if present); not durable until `sync_dir` of its parent. A failed
    /// creation may leave the new directory in place, empty ([F15 §5.2], NS-4).
    fn create_dir(&self, root: &Self::Root, rel: RelPath<'_>) -> Result<(), VfsError>;

    /// Removes an empty directory (`NotEmpty` otherwise).
    fn remove_dir(&self, root: &Self::Root, rel: RelPath<'_>) -> Result<(), VfsError>;

    /// Every entry of the directory (the root when `None`) except `.` and `..`, in unspecified order.
    fn list_dir(
        &self,
        root: &Self::Root,
        dir: Option<RelPath<'_>>,
    ) -> Result<Vec<DirEntry>, VfsError>;

    // Positional I/O ([OS/fs §4.3])

    /// Reads up to `buf.len()` bytes at `offset`; fewer only at end of file, 0 at or beyond it. `EINTR` is retried
    /// inside. May fail with `Io` (FM-12).
    fn read_at(&self, file: &Self::File, offset: u64, buf: &mut [u8]) -> Result<usize, VfsError>;

    /// Reads exactly `buf.len()` bytes at `offset`, or fails with `UnexpectedEof`.
    fn read_exact_at(&self, file: &Self::File, offset: u64, buf: &mut [u8])
    -> Result<(), VfsError>;

    /// Writes all of `buf` at `offset` (class `lazy`), looping over partial writes, or fails. May fail with `DiskFull`
    /// on any byte (FM-5); after any write error the written range is indeterminate.
    fn write_at(&self, file: &Self::File, offset: u64, buf: &[u8]) -> Result<(), VfsError>;

    // Durability classes ([OS/fs §4.4])

    /// Class `durable` (`Data`) or `durable+meta` (`DataAndMeta`) on a handle with write access. Flushes the whole file,
    /// whichever process wrote the bytes.
    fn sync(&self, file: &Self::File, kind: SyncKind) -> Result<(), DurabilityFailure>;

    /// Class `durable-name`: makes durable every create, rename and unlink already performed in `dir` (the root when
    /// `None`).
    fn sync_dir(
        &self,
        root: &Self::Root,
        dir: Option<RelPath<'_>>,
    ) -> Result<(), DurabilityFailure>;

    /// Makes every member durable by its class ([OS/fs §4.4.4]); on macOS one `F_FULLFSYNC` per volume.
    fn sync_group(
        &self,
        members: &[GroupMember<'_, Self::Root, Self::File>],
    ) -> Result<(), DurabilityFailure>;

    /// The one error policy ([OS/fs §4.4.5]): one stderr line naming the class, the call and the OS code
    /// ([`DurabilityFailure::stderr_line`]), then exit code 7 without destructors (in the simulator: a crash of the
    /// simulated process at this point).
    fn fail_stop(&self, failure: DurabilityFailure) -> !;

    // Extents and sealing ([OS/fs §4.5, §4.6])

    /// Creates a log extent of exactly `len` bytes whose every byte reads as zero, by `vol.extent_method`. Makes nothing
    /// durable; the caller follows [F16 §3].
    fn create_extent(
        &self,
        root: &Self::Root,
        rel: RelPath<'_>,
        len: u64,
        vol: &StoreVolume,
    ) -> Result<Self::File, VfsError>;

    /// Zero-fills an extent for reuse under a new epoch (G25), by `vol.extent_method`.
    fn recycle_extent(
        &self,
        file: &Self::File,
        len: u64,
        vol: &StoreVolume,
    ) -> Result<(), VfsError>;

    /// Makes the file read-only on disk ([80 §2.5] rule 2).
    fn seal(&self, file: &Self::File) -> Result<(), VfsError>;

    // Namespace changes ([OS/fs §4.7–§4.9])

    /// Removes a file's name (Windows clears `FILE_ATTRIBUTE_READONLY` first). Durable only after `sync_dir` of the
    /// parent. `NotFound` is returned as an error.
    fn unlink(
        &self,
        root: &Self::Root,
        rel: RelPath<'_>,
        retry: ShareRetry,
    ) -> Result<(), VfsError>;

    /// Renames a file or a directory between two roots on one volume ([OS/fs §4.8], [F15 §5.4]); fails `AlreadyExists`
    /// if `to` exists. A directory is never moved into its own subtree (the caller never asks for it). Makes nothing
    /// durable: every rename point is followed by `sync_dir` of both parents ([F15 §5.1] NS-5).
    fn rename_noreplace(
        &self,
        from_root: &Self::Root,
        from: RelPath<'_>,
        to_root: &Self::Root,
        to: RelPath<'_>,
        retry: ShareRetry,
    ) -> Result<(), VfsError>;

    /// Renames a file (files only, [OS/fs §4.8], [F15 §5.5]), atomically replacing an existing `to`. Never targets a
    /// sealed file.
    fn rename_replace(
        &self,
        from_root: &Self::Root,
        from: RelPath<'_>,
        to_root: &Self::Root,
        to: RelPath<'_>,
        retry: ShareRetry,
    ) -> Result<(), VfsError>;

    /// Exchanges two single-component directories on one volume ([OS/fs §4.9], [F15 §5.6]). A failed flush embedded in a
    /// step is `FlushFailed` ([OS/fs §4.1]) and leaves the intent for `swap_recover`.
    fn swap_dirs(
        &self,
        a_parent: &Self::Root,
        a: RelPath<'_>,
        b_parent: &Self::Root,
        b: RelPath<'_>,
        retry: ShareRetry,
    ) -> Result<SwapOutcome, VfsError>;

    /// Completes or rolls back an interrupted emulated swap from its intent file ([OS/fs §4.9.4]); an unreadable intent
    /// with `a` present and `<a>.swap-old` absent is removed (`NothingDone`). A failed embedded flush is `FlushFailed`.
    fn swap_recover(
        &self,
        a_parent: &Self::Root,
        a: RelPath<'_>,
        retry: ShareRetry,
    ) -> Result<SwapRecovery, VfsError>;

    // Queries ([OS/fs §4.10–§4.13])

    /// The file's size.
    fn file_size(&self, file: &Self::File) -> Result<u64, VfsError>;

    /// The identity of an open file.
    fn identity(&self, file: &Self::File) -> Result<FileIdentity, VfsError>;

    /// The identity of a root directory.
    fn root_identity(&self, root: &Self::Root) -> Result<FileIdentity, VfsError>;

    /// The identity of the object `rel` currently names, without opening it for data.
    fn path_identity(&self, root: &Self::Root, rel: RelPath<'_>) -> Result<FileIdentity, VfsError>;

    /// Free and total bytes of the root's volume.
    fn free_space(&self, root: &Self::Root) -> Result<FreeSpace, VfsError>;

    /// A hint that the range will not be read again; errors are ignored. Never called on a log extent that holds
    /// unflushed groups ([OS/fs §4.12]).
    fn advise_dontneed(&self, file: &Self::File, offset: u64, len: u64);

    /// The process-wide counters.
    fn counters(&self) -> VfsCounters;
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn share_retry_schedule() {
        assert_eq!(ShareRetry::None.next_sleep_ms(0, 0), None);
        let b = ShareRetry::Bounded { total_ms: 1_000 };
        let mut elapsed = 0u64;
        let mut sleeps = Vec::new();
        let mut retries = 0;
        while let Some(s) = b.next_sleep_ms(retries, elapsed) {
            sleeps.push(s);
            elapsed += u64::from(s);
            retries += 1;
        }
        assert_eq!(&sleeps[..7], &[1, 2, 4, 8, 16, 32, 64]);
        assert!(sleeps[7..].iter().all(|&s| s == 64));
        assert!(elapsed <= 1_000 && elapsed + 64 > 1_000);
        assert_eq!(
            ShareRetry::Bounded { total_ms: 0 }.next_sleep_ms(0, 0),
            None
        );
    }

    #[test]
    fn sync_kind_classes() {
        assert_eq!(SyncKind::Data.class(), DurabilityClass::Durable);
        assert_eq!(SyncKind::DataAndMeta.class(), DurabilityClass::DurableMeta);
        assert_eq!(DurabilityClass::DurableMeta.as_str(), "durable+meta");
        assert_eq!(DurabilityClass::DurableName.as_str(), "durable-name");
        assert_eq!(DurabilityClass::SyncGroup.as_str(), "sync_group");
    }

    #[test]
    fn file_identity_layout() {
        let id = FileIdentity::from_dev_ino(0x0102_0304_0506_0708, 0x1112_1314_1516_1718);
        let b = id.to_bytes();
        assert_eq!(&b[..8], &[8, 7, 6, 5, 4, 3, 2, 1]);
        assert_eq!(&b[8..16], &[0x18, 0x17, 0x16, 0x15, 0x14, 0x13, 0x12, 0x11]);
        assert_eq!(&b[16..], &[0; 8]);
    }

    proptest! {
        #![proptest_config(crate::testing::proptest_config())]

        #[test]
        fn file_identity_round_trips(volume in any::<u64>(), file in any::<[u8; 16]>()) {
            let id = FileIdentity { volume, file };
            prop_assert_eq!(FileIdentity::from_bytes(&id.to_bytes()), id);
        }
    }
}
