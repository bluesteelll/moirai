//! The storage seam of moirai: the `Vfs` trait (durability classes, `sync_dir`, `sync_group`, `rename_noreplace` and
//! `rename_replace`, `swap_dirs`, the `map_sealed` contract, the environment-guard types), the complete `ProjectFs`
//! trait (read and write side), the `Meter` trait (free space, available physical memory, child peak, heap high-water
//! mark), `LockBytes` with the target-independent in-process grant table, the wall, monotonic and boot clocks, and the
//! evidence types `ProcId`, `BootId`, `Liveness`, `OsFileId` and `VolumeCaps`.
//!
//! Product crate with no dependencies, shared by the simulator (`moirai-vfs-sim`) and the OS layer (`moirai-os`);
//! checked by GT20 (e) on every target. It contains no OS call, no `cfg(target_os)` and no `unsafe` code
//! ([OS/README §2.1]). Every other crate is generic over the seams (`V: Vfs`, `P: ProjectFs`, `M: Meter`) and never
//! boxes them: nothing on the commit path uses `dyn` ([80 §1] X4, [OS/README §4.1]).
//!
//! Module map ([OS/README §2.1] placement table):
//!
//! | Module | Contents | Specification |
//! |---|---|---|
//! | [`error`] | `OsCode` and its OS-error unit, `VfsError`, `VfsErrorKind`, `DurabilityFailure` and its `fail_stop` line | [OS/fs §4.4.5, §6], [OS/shell §6], [F19 §10.2] |
//! | [`fs`] | `VfsTypes`, `StoreFs`, roots, files, durability classes, renames, the swap outcomes, counters | [OS/fs], [F15 §4, §5] |
//! | [`lock`] | lock bytes, the `Locks` trait, `Grant`, `LockError` | [OS/lock §2–§4], X-F4 |
//! | [`grant`] | `GrantTable`, the pure in-process lock-ownership state machine | [OS/lock §5] |
//! | [`map`] | `SealedMaps`, `SealedMap`, `Advice`, `MapError` | [OS/map §3] |
//! | [`env`](mod@env) | `EnvGuard` and the classification types | [OS/env §2] |
//! | [`proc`] | `ProcHost`, `OsTag`, `ProcId`, `BootId`, `BootIdentity`, `Liveness` | [OS/proc §2–§10] |
//! | [`clock`] | `Clock`, `Stamp`, `DeadlineState`, `hlc_next` | [OS/README §4.4], [OS/clock] |
//! | [`entropy`] | `Entropy`, the random source | [OS/README §4.6] |
//! | [`meter`] | `Meter` and its reading types | [OS/README §4.3], [OS/mem] |
//! | [`path`] | `RelPath`, `RelPathBuf`, `AbsPath`, `CanonicalRoot`, `EntryName`, `EntryNameRef`, `PathError` | [OS/path §2, §9, §11] |
//! | [`project`] | `ProjectFs`, `OsFileId`, `FsTime`, `FileAttrs`, `VolumeCaps` and the project-file types | [OS/project] |
//! | [`swap`] | `SwapIntent`: the side names and the byte-exact codec of the swap intent file (with a crate-private XXH3-64) | [OS/fs §4.9.2, §4.9.3] |
//!
//! Sources: [80 §2.1–§2.3, §2.7, §2.11]; `docs/m0/PLAN.md` §2.2, §6.2 R2; `docs/spec/os/` (WP-17); [F15 §4, §5].

pub mod clock;
pub mod entropy;
pub mod env;
pub mod error;
pub mod fs;
pub mod grant;
pub mod lock;
pub mod map;
pub mod meter;
pub mod path;
pub mod proc;
pub mod project;
pub mod swap;
mod xxh3;

pub use clock::{Clock, DeadlineState, Stamp, hlc_next};
pub use entropy::Entropy;
pub use env::{
    Classification, ClassifyDepth, CloudKind, EnvGuard, EnvWarning, ExtentMethod, FsKind, FsName,
    OsVersion, ProbeOutcome, ProbeReport, Refusal, StoreVolume,
};
pub use error::{DurabilityFailure, DurabilityLine, OsCode, OsErrorUnit, VfsError, VfsErrorKind};
pub use fs::{
    Access, DirEntry, DurabilityClass, EntryKind, FileIdentity, FreeSpace, GroupMember,
    OS_SHARE_RETRY_MS, OpenHint, RootAccess, RootRole, ShareRetry, StoreFs, SwapOutcome,
    SwapRecovery, SyncKind, VfsCounters, VfsTypes,
};
pub use grant::{
    CancelStep, ClientId, DeadlineStep, Driver, FailStep, GrantStep, GrantTable, KernelHandle,
    KernelResult, KernelState, Outcome, Owner, ProbeStep, ReleaseStep, Step, TableId, TryStep,
    WaitMode, WaitStep,
};
pub use lock::{
    Acquired, FOREIGN_CHECK_BYTE, Grant, LockByte, LockError, LockMode, Locks, N_QUIET, N_SLOTS,
    ProbeResult, QuietIndex, ROLE_BASE, SLOT_BASE, SlotIndex,
};
pub use map::{Advice, MAP_REGISTRY_SLOTS, MapError, SealedMap, SealedMaps};
pub use meter::{ChildPeak, ChildTicket, CpuTimes, HeapCounts, Meter, MeterError, PeakKind};
pub use path::{
    AbsPath, CanonicalRoot, EntryName, EntryNameRef, PathError, RelPath, RelPathBuf, display_name,
};
pub use proc::{
    BootId, BootIdentity, Liveness, OsTag, ParentRec, ProcHost, ProcId, UnknownBoot, Wake,
    WatchEvent,
};
pub use project::{
    At, BtimeTrust, CaseRule, CloudRule, DirEquivalence, EnumEnd, FileAttrs, FileIdKind, FsTime,
    Holder, IdLocate, JournalKind, Located, OsFileId, PfsCounters, ProjEntry, ProjKind, ProjectFs,
    ProjectRead, ReadOpts, ReadSnapshot, RenameFailure, RenameRule, Renamed, Stat, StatMode,
    StatRec, VolumeCaps, VolumeKey,
};
pub use swap::SwapIntent;

/// The store-side seam ([OS/README §4.1]): the union of the seven sub-traits over one set of handle types.
///
/// Implemented by `moirai_os::OsVfs` and by `moirai_vfs_sim::SimVfs`. Store code is generic over `V: Vfs` and never
/// boxes it ([80 §1] X4). Every method of every sub-trait takes `&self`; an implementation value is cheap to clone or share,
/// and its process-global state (the lock registry, the mapping registry, the fault handler) follows
/// [OS/README §5.4]. Every store file, and every file of an image destination or backup directory, goes through it;
/// project files never do (they go through [`ProjectFs`]).
///
/// `ProcHost` is a supertrait rather than a separate bound because every store process needs the boot identity before
/// its first read ([OS/README §4.1], [OS/proc] open point 1); `Entropy` because every store id, epoch, nonce and random
/// uid is drawn through the seam ([OS/README §4.6], open point 12).
pub trait Vfs:
    StoreFs + Locks + SealedMaps + EnvGuard + Clock + ProcHost + Entropy + Send + Sync + 'static
{
}

impl<T> Vfs for T where
    T: StoreFs + Locks + SealedMaps + EnvGuard + Clock + ProcHost + Entropy + Send + Sync + 'static
{
}

/// Helpers shared by the unit tests of every module.
#[cfg(test)]
pub(crate) mod testing {
    /// The proptest configuration of every property of the crate: 256 cases in the `pr` tier (the default), more in the
    /// `nightly` and `exit` tiers named by `MOIRAI_TEST_TIER` (PLAN §2.1); no failure persistence (the seed is printed).
    pub(crate) fn proptest_config() -> proptest::test_runner::Config {
        let cases = match std::env::var("MOIRAI_TEST_TIER").as_deref() {
            Ok("nightly") => 4_096,
            Ok("exit") => 65_536,
            _ => 256,
        };
        proptest::test_runner::Config {
            cases,
            failure_persistence: None,
            ..proptest::test_runner::Config::default()
        }
    }
}
