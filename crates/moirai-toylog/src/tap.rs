//! Protocol notes for the crash enumerator's checks ([F13 §1.4] "The toy vehicle", [F16 §17.2] "Where the detectors
//! live"): the toy reports through a [`Tap`] the protocol steps that no `Vfs` event names — the start and end of phase 1
//! and of phase 3, a maintenance run's decision, the id of an acknowledged `FsIntent`. A harness on the simulator turns
//! each into a note of the enumerator's vocabulary (`moirai_vfs_sim::enumerate::NOTE_PHASE`, `NOTE_MAINT_DECISION`) or a
//! namespace expectation of its ledger; elsewhere [`NoTap`] drops them. The predicates over them are the enumerator's,
//! never the toy's (PLAN §3.1 S4).
//!
//! A note is emitted by the task that performs the step, between two of its `Vfs` calls — right before the step's first
//! call for a beginning, right after its last call for an end — so it carries that task and interleaves with its events
//! exactly.
//!
//! The toy also names the store files an operation uses ([`Note::Uses`]), so that a harness can say which files a
//! refusal of the operation concerns ([F16 §17.2] avail; [F15] FM-10 "Crash gates": under an external act the gate
//! asserts detection, a refusal that names the file); whether a fault explains the refusal is the enumerator's call.

use moirai_vfs::RelPathBuf;

/// One protocol note.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Note {
    /// Phase 1 of a write begins (`true`) or ends (`false`) ([F16] P-25: no writer or flush byte is held).
    Phase1(bool),
    /// Phase 3, the maintenance after the acknowledgement, begins or ends ([F16] P-51: no writer or flush byte is held).
    Phase3(bool),
    /// A maintenance run is decided ([F17 §5.2]), after its probe round of the quiet bytes and before the first record
    /// of the run ([F03 §3.1] rule 2). A run that quiet mode defers, or that the maintenance byte keeps out, decides
    /// nothing and emits no note.
    MaintenanceDecided {
        /// A trigger decided it ([F17 §5.2]), not an explicit command.
        automatic: bool,
        /// A delta checkpoint below the quiet cap ([F17 §5.3]): the kind quiet mode defers.
        below_cap: bool,
    },
    /// The `FsIntent` group of a `file mv` or `file rm` was acknowledged ([40 §3.4] step 2, [F16] P-16), before the
    /// namespace change: `key` is the operation's idempotency key, `lsn` the intent's id, which names its trash
    /// directory (`trash/<lsn>/`, [F16 §13.1]).
    IntentAcked {
        /// The idempotency key.
        key: u64,
        /// The intent id.
        lsn: u64,
    },
    /// The operation reads the store file, or finds it missing where it needs it: `HEAD`'s slots ([F04 §8.1]), an extent
    /// it scans or reads the chain bytes of ([F05 §4.3], §5), a segment or `hist` file it loads ([F16] P-59, P-68). Every
    /// file the state it refuses from rests on is among the files it used, so a `store_corrupt` refusal concerns them
    /// ([F16 §17.2] avail).
    Uses(StoreFile),
}

/// A file of the store directory that an operation uses ([`Note::Uses`], [F02 §6.3]).
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum StoreFile {
    /// `HEAD`, the slot file ([F04 §8]).
    Head,
    /// A sealed file: `log.<no>`, `hist.<no>` or `seg.base.<no>` by `family` ([`crate::format::family`]).
    Sealed {
        /// The family.
        family: u8,
        /// The file number.
        no: u32,
    },
}

impl StoreFile {
    /// The extent `log.<n>`.
    pub fn log(n: u32) -> StoreFile {
        StoreFile::Sealed {
            family: crate::format::family::LOG,
            no: n,
        }
    }

    /// Its name in the store directory.
    pub fn name(self) -> RelPathBuf {
        match self {
            StoreFile::Head => crate::store::rel("HEAD"),
            StoreFile::Sealed { family, no } => crate::store::sealed_name(family, no),
        }
    }
}

/// The sink of the toy's protocol notes.
pub trait Tap: Clone + Send + Sync + 'static {
    /// Records one note.
    fn note(&self, n: Note);
}

/// A tap that drops every note (the real `Vfs`, measurements).
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct NoTap;

impl Tap for NoTap {
    fn note(&self, _n: Note) {}
}
