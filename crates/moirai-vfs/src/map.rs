//! Read-only mappings of sealed files: the `SealedMaps` sub-trait of `Vfs` and its types ([OS/map §3]; X-F6 rules 1–6).
//!
//! Only sealed files are mapped, read-only, whole-file from offset 0; log extents and `HEAD` are never mapped
//! ([OS/map §2]). Before mapping, the caller reads the file's header and passes its `total_len`; `map_sealed` refuses a
//! file whose size differs ([OS/map §4]). A fault inside a mapping (an external truncation, a media error) ends the
//! process with one line and exit 7 ([OS/map §8]; fault-model item (9), [F15 §3.9] FM-9). Mapped bytes are read only through
//! types for which every bit pattern is valid, with every offset and length taken from them checked before use
//! ([OS/map §6]).

use core::fmt;

use crate::error::VfsError;
use crate::fs::VfsTypes;
use crate::path::RelPath;

/// The capacity of the process-global mapping registry of `moirai-os` ([OS/map §7]): a registration that finds no free
/// entry fails with [`MapError::RegistryFull`] and maps nothing. The simulator enforces the same bound per simulated
/// process.
pub const MAP_REGISTRY_SLOTS: usize = 512;

/// The mapping sub-trait of `Vfs` ([OS/map §3]).
pub trait SealedMaps: VfsTypes {
    /// A mapping of one whole sealed file.
    type Map: SealedMap;

    /// Checks `file_size(file) == expected_len`, maps the whole file read-only, and registers the mapping under `name`
    /// (the store-relative file name, used only in the fault message of [OS/map §8]; a view passed by value where
    /// [OS/map §3] writes `&RelPath`, see the [`crate::path`] module documentation). `expected_len` is the `total_len`
    /// the caller read from the file's header ([OS/map §4]). The file handle needs read access and may be closed after
    /// the call.
    fn map_sealed(
        &self,
        file: &Self::File,
        expected_len: u64,
        name: RelPath<'_>,
    ) -> Result<Self::Map, MapError>;

    /// A hint only; errors are ignored and semantics never change ([OS/map §9]).
    fn advise(&self, map: &Self::Map, offset: u64, len: u64, advice: Advice);
}

/// A read-only mapping of a whole sealed file ([OS/map §3]). Dropping it unregisters ([OS/map §7]) and then unmaps.
// A sealed file is never empty (`MapError::Empty`), so the trait has no `is_empty`.
#[allow(clippy::len_without_is_empty)]
pub trait SealedMap: Send + Sync {
    /// Exactly `len()` bytes. Read only through the typed, bounds-checked access of [OS/map §6].
    fn bytes(&self) -> &[u8];

    /// The mapped length, equal to the header's `total_len`.
    fn len(&self) -> u64;
}

/// An access-pattern hint ([OS/map §9]).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Advice {
    /// Index sections: node table, CSR, bitsets.
    Random,
    /// Before a tier-1 FTS scan.
    WillNeed,
    /// A sequential pass.
    Sequential,
}

/// Why a mapping was refused ([OS/map §3]).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MapError {
    /// The file's size differs from the header's `total_len` ([OS/map §4]). Nothing is mapped. The caller re-reads
    /// `HEAD` and retries once, then exits 7.
    SizeMismatch {
        /// The header's `total_len`.
        expected: u64,
        /// The file's size.
        actual: u64,
    },
    /// `expected_len` is 0; no sealed kind has an empty header. A programming error in release builds too.
    Empty,
    /// The mapping registry is full ([OS/map §7]); the caller exits 7. Nothing is mapped.
    RegistryFull,
    /// The OS refused the mapping (including `ERROR_NOT_ENOUGH_MEMORY`, `ENOMEM`, `EACCES`).
    Io(VfsError),
}

impl fmt::Display for MapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MapError::SizeMismatch { expected, actual } => {
                write!(
                    f,
                    "sealed file size {actual} differs from its header's total_len {expected}"
                )
            }
            MapError::Empty => f.write_str("a sealed file cannot be empty"),
            MapError::RegistryFull => write!(
                f,
                "the mapping registry is full ({MAP_REGISTRY_SLOTS} mappings)"
            ),
            MapError::Io(e) => write!(f, "mapping failed: {e}"),
        }
    }
}

impl std::error::Error for MapError {}
