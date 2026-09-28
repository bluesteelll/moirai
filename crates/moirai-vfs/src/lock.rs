//! `LockBytes`: the lock bytes of the store's `LOCK` file, the `Locks` sub-trait of `Vfs` and its types
//! ([OS/lock §2–§4]; X-F1's lock-byte part, X-F4).
//!
//! Every lock byte lies beyond the end of `LOCK` (a 36 KiB file, never resized), so no lock covers a byte that holds
//! data; every lock is one byte and exclusive ([OS/lock §2], contract item 1). In-process ownership is decided by the
//! pure [`crate::GrantTable`] before any kernel call (item 2); the kernel lock is taken on one handle per role per
//! process, and every slot grant has its own handle.
//!
//! The lock order is `Slot` (0) < `Leader` (1) < `Maintenance` (2) < `Flush` (3) < `Writer` (4); only `Writer` and
//! `Flush` are ever waited for, and a client may wait only for a byte of higher rank than every byte it holds
//! ([OS/lock §6]). Programming errors — a reentrant acquisition, a wait on a byte that is not waitable, a wait that breaks
//! the order, an acquisition through a `Probe`-mode client, releasing a grant through another client — panic in every
//! build ([OS/lock §4]).

use core::fmt;

use crate::error::{OsCode, VfsError};
use crate::fs::VfsTypes;
use crate::grant::{ClientId, TableId};

/// The base offset of the role bytes, 2^62 ([OS/lock §2]).
pub const ROLE_BASE: u64 = 1 << 62;
/// The base offset of the liveness slots, 2^62 + 2^16.
pub const SLOT_BASE: u64 = (1 << 62) + (1 << 16);
/// The number of liveness slots; equals `LockHdr.n_slots` of [F03 §4.1].
pub const N_SLOTS: u16 = 256;
/// Reserved; never locked by moirai; probed by `foreign_lock_check` ([OS/lock §11]).
pub const FOREIGN_CHECK_BYTE: u64 = ROLE_BASE + 63;

/// A liveness-slot index, 0..=255 ([OS/lock §2, §10]).
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct SlotIndex(u8);

impl SlotIndex {
    /// `Some` for `i < 256`.
    pub const fn new(i: u16) -> Option<SlotIndex> {
        if i < N_SLOTS {
            Some(SlotIndex(i as u8))
        } else {
            None
        }
    }

    /// The index.
    pub const fn get(self) -> u8 {
        self.0
    }

    /// The slot's record offset in `LOCK` is `4096 + 128 × i` ([OS/lock §10], [F03 §8.1]).
    pub const fn record_offset(self) -> u64 {
        4096 + 128 * self.0 as u64
    }
}

/// One lock byte of `LOCK` ([OS/lock §2]). The offsets are frozen by X-F1; [F03 §3] owns them.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum LockByte {
    /// The innermost byte: the appender, the flush holder's scan and re-write, every publisher. Rank 4; waitable.
    Writer,
    /// The optional leader, for its lifetime (only if built). Rank 1; try only.
    Leader,
    /// Checkpoint, promotion, rollup, GC. Rank 2; try only.
    Maintenance,
    /// The quiet-advisory byte ([AR §4.1]); probed or tried, never waited; no rank.
    Quiet,
    /// The flush holder; boot-change recovery. Rank 3; waitable.
    Flush,
    /// A liveness slot. Rank 0; try only.
    Slot(SlotIndex),
}

impl LockByte {
    /// Offset of the byte in `LOCK` ([OS/lock §2] table). The only source of lock offsets (contract item 1).
    pub const fn offset(self) -> u64 {
        match self {
            LockByte::Writer => ROLE_BASE,
            LockByte::Leader => ROLE_BASE + 1,
            LockByte::Maintenance => ROLE_BASE + 2,
            LockByte::Quiet => ROLE_BASE + 3,
            LockByte::Flush => ROLE_BASE + 4,
            LockByte::Slot(i) => SLOT_BASE + i.0 as u64,
        }
    }

    /// Rank in the lock order; `None` for `Quiet`, which is never waited for.
    pub const fn rank(self) -> Option<u8> {
        match self {
            LockByte::Slot(_) => Some(0),
            LockByte::Leader => Some(1),
            LockByte::Maintenance => Some(2),
            LockByte::Flush => Some(3),
            LockByte::Writer => Some(4),
            LockByte::Quiet => None,
        }
    }

    /// Every byte except a slot.
    pub const fn is_role(self) -> bool {
        !matches!(self, LockByte::Slot(_))
    }

    /// `Writer` and `Flush` only: the bytes `acquire_within` accepts.
    pub const fn waitable(self) -> bool {
        matches!(self, LockByte::Writer | LockByte::Flush)
    }

    /// The byte at `offset`, if it is one of the table's bytes (the reserved range 2^62 + 5 … 2^62 + 63 is none).
    pub const fn from_offset(offset: u64) -> Option<LockByte> {
        match offset {
            o if o == ROLE_BASE => Some(LockByte::Writer),
            o if o == ROLE_BASE + 1 => Some(LockByte::Leader),
            o if o == ROLE_BASE + 2 => Some(LockByte::Maintenance),
            o if o == ROLE_BASE + 3 => Some(LockByte::Quiet),
            o if o == ROLE_BASE + 4 => Some(LockByte::Flush),
            o if o >= SLOT_BASE && o < SLOT_BASE + N_SLOTS as u64 => {
                Some(LockByte::Slot(SlotIndex((o - SLOT_BASE) as u8)))
            }
            _ => None,
        }
    }
}

/// How a client uses `LOCK` ([OS/lock §4]).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum LockMode {
    /// Readers, hooks and CLIs that only read `LOCK`'s records and probe bytes. No acquisition.
    Probe,
    /// Writers, maintenance, MCP servers, `init`/`restore` after creation, `file mv`/`file rm`.
    Acquire,
}

/// The lock sub-trait of `Vfs` ([OS/lock §4]); the contract items 1–10 of X-F4 hold identically on every OS.
pub trait Locks: VfsTypes {
    /// One client instance ([80 §2.1] `LockFile`). Dropping it releases every grant it holds (kernel unlock) and leaves
    /// every queue it waits in.
    type Client: Send;

    /// Opens `LOCK` in the store root, checks its identity ([OS/lock §9.1]) and registers a client in the process's grant
    /// table for that `LOCK`. `LOCK` absent → `NoLockFile`; never creates it.
    fn lock_client(&self, store: &Self::Root, mode: LockMode) -> Result<Self::Client, LockError>;

    /// The client's data handle on `LOCK` (read-only in `Probe` mode), for the records of [F03 §4–§8].
    fn lock_data<'a>(&self, client: &'a Self::Client) -> &'a Self::File;

    /// Never waits. `Busy` without a kernel call if the byte is held or being acquired in this process.
    fn try_acquire(&self, client: &mut Self::Client, byte: LockByte)
    -> Result<Acquired, LockError>;

    /// Bounded blocking wait (G1): `Writer` or `Flush` only, lock order checked ([OS/lock §6]). `within_ms = 0` is a try.
    /// Returns `Granted` or `Busy`, never both; a grant that races the deadline is either returned or released
    /// (contract item 5).
    fn acquire_within(
        &self,
        client: &mut Self::Client,
        byte: LockByte,
        within_ms: u32,
    ) -> Result<Acquired, LockError>;

    /// Always unlocks in the kernel. `grant` must belong to `client`.
    fn release(&self, client: &mut Self::Client, grant: Grant);

    /// `Held`, `Free` or `Unknown`; every probe error is `Unknown`, never `Free` (contract item 6, [OS/lock §8]).
    fn probe(&self, client: &Self::Client, byte: LockByte) -> ProbeResult;

    /// Does this client hold `byte`?
    fn holds(&self, client: &Self::Client, byte: LockByte) -> bool;

    /// Process-wide: does any client of any grant table in this process hold a role byte? (Contract item 7: no spawn
    /// while holding one.)
    fn holds_any_role(&self) -> bool;

    /// `doctor` only: is a lock that moirai never takes present on `LOCK` ([OS/lock §11])?
    fn foreign_lock_check(&self, client: &Self::Client) -> ProbeResult;
}

/// The result of an acquisition ([OS/lock §4]).
#[must_use]
#[derive(Debug, Eq, PartialEq)]
pub enum Acquired {
    /// The byte is held; the grant is consumed by `release`.
    Granted(Grant),
    /// The byte is held elsewhere (or the wait timed out).
    Busy,
}

/// Proof of one held byte ([OS/lock §4]). Not `Clone`, not `Copy`; consumed by `release`. Only a [`crate::GrantTable`]
/// creates one, for the client it granted the byte to.
#[must_use]
#[derive(Debug, Eq, PartialEq)]
pub struct Grant {
    byte: LockByte,
    client: ClientId,
    table: TableId,
}

impl Grant {
    /// Made by the grant table only.
    pub(crate) const fn new(byte: LockByte, client: ClientId, table: TableId) -> Grant {
        Grant {
            byte,
            client,
            table,
        }
    }

    /// The held byte.
    pub fn byte(&self) -> LockByte {
        self.byte
    }

    /// The client the byte was granted to.
    pub fn client(&self) -> ClientId {
        self.client
    }

    /// The table that granted it.
    pub fn table(&self) -> TableId {
        self.table
    }
}

/// The answer of a probe ([OS/lock §3] item 6, §8).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ProbeResult {
    /// Some process (this one included) holds the byte, or a foreign lock covers it.
    Held,
    /// Nobody holds it at the instant of the probe.
    Free,
    /// The probe could not tell (a denial, a read-only descriptor, any error). Never ends a lease or recovers an intent.
    Unknown,
}

/// A failure of the lock layer ([OS/lock §4]). Each is exit 7 for its caller; texts are [F19 §10.2]'s.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LockError {
    /// `LOCK` does not exist: the store is not initialised or is damaged.
    NoLockFile,
    /// The identity check of [OS/lock §9.1] failed twice.
    IdentityMismatch,
    /// `LOCK` cannot be opened for writing (a sandbox, another principal): writers exit 7 with the texts of [90 §5.3].
    AccessDenied {
        /// The raw code.
        os: OsCode,
    },
    /// The kernel refuses byte-range locks here (`EINVAL`, `ENOTSUP`, `EOPNOTSUPP`; Windows `ERROR_NOT_SUPPORTED`,
    /// `ERROR_INVALID_FUNCTION`): the store is refused ([80 §2.2.2] "Unsupported").
    Unsupported {
        /// The raw code.
        os: OsCode,
    },
    /// Any other failure (including a waiter thread that could not be started).
    Io(VfsError),
}

impl fmt::Display for LockError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LockError::NoLockFile => f.write_str("LOCK does not exist"),
            LockError::IdentityMismatch => {
                f.write_str("LOCK was replaced while it was being opened")
            }
            LockError::AccessDenied { os } => write!(f, "LOCK cannot be opened for writing ({os})"),
            LockError::Unsupported { os } => {
                write!(f, "byte-range locks are not supported here ({os})")
            }
            LockError::Io(e) => write!(f, "lock I/O error: {e}"),
        }
    }
}

impl std::error::Error for LockError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_bytes() -> impl Iterator<Item = LockByte> {
        [
            LockByte::Writer,
            LockByte::Leader,
            LockByte::Maintenance,
            LockByte::Quiet,
            LockByte::Flush,
        ]
        .into_iter()
        .chain((0..N_SLOTS).map(|i| LockByte::Slot(SlotIndex::new(i).unwrap())))
    }

    #[test]
    fn offsets_match_the_frozen_map() {
        assert_eq!(LockByte::Writer.offset(), 0x4000_0000_0000_0000);
        assert_eq!(LockByte::Leader.offset(), 0x4000_0000_0000_0001);
        assert_eq!(LockByte::Maintenance.offset(), 0x4000_0000_0000_0002);
        assert_eq!(LockByte::Quiet.offset(), 0x4000_0000_0000_0003);
        assert_eq!(LockByte::Flush.offset(), 0x4000_0000_0000_0004);
        assert_eq!(
            LockByte::Slot(SlotIndex::new(0).unwrap()).offset(),
            0x4000_0000_0001_0000
        );
        assert_eq!(
            LockByte::Slot(SlotIndex::new(255).unwrap()).offset(),
            0x4000_0000_0001_00FF
        );
        assert_eq!(FOREIGN_CHECK_BYTE, 0x4000_0000_0000_003F);
        assert_eq!(SlotIndex::new(256), None);
        assert_eq!(SlotIndex::new(7).unwrap().record_offset(), 4096 + 7 * 128);
        for b in all_bytes() {
            assert_eq!(LockByte::from_offset(b.offset()), Some(b));
            // Every lock byte lies beyond the 36 KiB `LOCK` file (contract item 1).
            assert!(b.offset() > 36 * 1024);
        }
        for reserved in ROLE_BASE + 5..=ROLE_BASE + 63 {
            assert_eq!(LockByte::from_offset(reserved), None);
        }
        assert_eq!(LockByte::from_offset(SLOT_BASE + 256), None);
    }

    #[test]
    fn ranks_and_waitable_set() {
        assert_eq!(LockByte::Quiet.rank(), None);
        assert!(LockByte::Slot(SlotIndex::new(3).unwrap()).rank() < LockByte::Leader.rank());
        assert!(LockByte::Leader.rank() < LockByte::Maintenance.rank());
        assert!(LockByte::Maintenance.rank() < LockByte::Flush.rank());
        assert!(LockByte::Flush.rank() < LockByte::Writer.rank());
        let waitable: Vec<LockByte> = all_bytes().filter(|b| b.waitable()).collect();
        assert_eq!(waitable, [LockByte::Writer, LockByte::Flush]);
        assert_eq!(
            all_bytes().filter(|b| !b.is_role()).count(),
            usize::from(N_SLOTS)
        );
    }
}
