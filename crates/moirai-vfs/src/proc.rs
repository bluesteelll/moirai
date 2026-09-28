//! Process identity, boot identity and liveness: the `ProcHost` sub-trait of `Vfs` and its types ([OS/proc §2–§10];
//! X-F2).
//!
//! `ProcId` is diagnostics only: no correctness decision reads it ([OS/proc §3.4]). The boot identity is 16 opaque bytes,
//! constant for exactly one boot and invariant under wall-clock changes, suspend and hibernation; a process that cannot
//! read it runs in Unknown-boot mode and keeps using the store ([OS/proc §4, §5]). Liveness is three-valued, and
//! `Unknown` never ends a lease, recovers an intent or reclaims anything ([OS/proc §6]). The byte layouts that go on disk
//! inside `LOCK` records are [F03 §5]'s, which embed `ProcId` unchanged.

use crate::error::VfsError;

/// The OS tag byte ([OS/proc §2], registry [F01 §3.2]): the OS that wrote a runtime value. 4–255 are reserved and
/// uninterpretable.
#[repr(u8)]
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum OsTag {
    /// Never written by a process as its own tag; uninterpretable.
    Unspecified = 0,
    /// Windows.
    Windows = 1,
    /// Linux.
    Linux = 2,
    /// macOS.
    MacOs = 3,
}

impl OsTag {
    /// The tag for a byte; `None` for the reserved values 4–255.
    pub const fn from_u8(v: u8) -> Option<OsTag> {
        match v {
            0 => Some(OsTag::Unspecified),
            1 => Some(OsTag::Windows),
            2 => Some(OsTag::Linux),
            3 => Some(OsTag::MacOs),
            _ => None,
        }
    }
}

/// The 32-byte diagnostic process identity ([OS/proc §3]). Plain data; every field is public.
///
/// Layout (little-endian, packed): `os` u8 at 0, `flags` u8 at 1, reserved u16 at 2, `pid` u32 at 4, `start` u64 at 8,
/// `boot_hash` u64 at 16, `pidns` u64 at 24.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct ProcId {
    /// The OS tag of the process ([`OsTag`] as a byte).
    pub os: u8,
    /// Bit 0 `start_known`, bit 1 `start_boot_relative`, bit 2 `boot_known`, bit 3 `pidns_known`; bits 4–7 reserved.
    pub flags: u8,
    /// The OS process id.
    pub pid: u32,
    /// The process start time in nanoseconds ([OS/proc §3.2]); 0 when `start_known` is clear. Compared for equality
    /// only, between two `ProcId`s of one OS tag.
    pub start: u64,
    /// `boot_hash` of the process's boot identity ([OS/proc §4.3]); 0 in Unknown-boot mode.
    pub boot_hash: u64,
    /// Linux: the inode number of `/proc/self/ns/pid`; 0 elsewhere and when `pidns_known` is clear.
    pub pidns: u64,
}

impl ProcId {
    /// The encoded length.
    pub const LEN: usize = 32;
    /// `start` holds the start time.
    pub const START_KNOWN: u8 = 1 << 0;
    /// `start` counts from boot (Linux), not from the Unix epoch.
    pub const START_BOOT_RELATIVE: u8 = 1 << 1;
    /// `boot_hash` holds a known boot.
    pub const BOOT_KNOWN: u8 = 1 << 2;
    /// `pidns` holds the PID namespace.
    pub const PIDNS_KNOWN: u8 = 1 << 3;
    /// The defined flag bits.
    const FLAGS_MASK: u8 = 0x0F;

    /// Encodes [OS/proc §3.1] exactly; reserved bits and bytes are written as zero.
    pub fn to_bytes(&self) -> [u8; 32] {
        let mut b = [0u8; 32];
        b[0] = self.os;
        b[1] = self.flags & Self::FLAGS_MASK;
        b[4..8].copy_from_slice(&self.pid.to_le_bytes());
        b[8..16].copy_from_slice(&self.start.to_le_bytes());
        b[16..24].copy_from_slice(&self.boot_hash.to_le_bytes());
        b[24..32].copy_from_slice(&self.pidns.to_le_bytes());
        b
    }

    /// Decodes [OS/proc §3.1]; `None` if the value is uninterpretable (reserved bits or bytes set, `os` not 1–3). Such a
    /// value is displayed as `?` and `alive` answers `Unknown` for it.
    pub fn from_bytes(b: &[u8; 32]) -> Option<ProcId> {
        if !(1..=3).contains(&b[0]) || b[1] & !Self::FLAGS_MASK != 0 || b[2] != 0 || b[3] != 0 {
            return None;
        }
        Some(ProcId {
            os: b[0],
            flags: b[1],
            pid: le_u32(b, 4),
            start: le_u64(b, 8),
            boot_hash: le_u64(b, 16),
            pidns: le_u64(b, 24),
        })
    }
}

fn le_u32(b: &[u8], at: usize) -> u32 {
    let mut x = [0u8; 4];
    x.copy_from_slice(&b[at..at + 4]);
    u32::from_le_bytes(x)
}

fn le_u64(b: &[u8], at: usize) -> u64 {
    let mut x = [0u8; 8];
    x.copy_from_slice(&b[at..at + 8]);
    u64::from_le_bytes(x)
}

/// The boot identity: 16 opaque bytes ([OS/proc §4.2]), BLAKE3-128 of a per-OS source with the `moirai-boot-id-v1`
/// domain prefix (computed by `moirai-os`).
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct BootId(pub [u8; 16]);

impl BootId {
    /// `boot_hash` of [OS/proc §4.3]: the first 8 bytes read little-endian, with bit 0 forced to 1 so that a known boot
    /// never hashes to 0 (0 means "unknown" in every field that carries a `boot_hash`).
    pub fn hash(&self) -> u64 {
        le_u64(&self.0, 0) | 1
    }
}

/// A process's boot identity, read once at its first need and cached for its lifetime ([OS/proc §4.4]).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum BootIdentity {
    /// The boot is known.
    Known(BootId),
    /// Unknown-boot mode ([OS/proc §5]), with the reason.
    Unknown(UnknownBoot),
}

impl BootIdentity {
    /// The `boot_hash` this identity writes: [`BootId::hash`] when known, 0 in Unknown-boot mode (rule U4).
    pub fn boot_hash(&self) -> u64 {
        match self {
            BootIdentity::Known(b) => b.hash(),
            BootIdentity::Unknown(_) => 0,
        }
    }
}

/// Why the boot identity is unknown ([OS/proc §4.2, §4.4]).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum UnknownBoot {
    /// The source does not exist on this system.
    SourceAbsent,
    /// A sandbox or another principal denied the read.
    Denied,
    /// The source's value has the wrong form.
    Malformed,
    /// This build runs in Unknown-boot mode by decision (HOLE(OS-win-boot-source) option (c)).
    DisabledByBuild,
}

/// The parent record carried in `SlotRec` ([OS/proc §3.3]).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct ParentRec {
    /// The parent's pid.
    pub pid: u32,
    /// The parent's start time, in the unit of [OS/proc §3.2] for this OS.
    pub start: u64,
    /// Whether `start` could be read.
    pub start_known: bool,
}

/// Liveness, three-valued everywhere ([OS/proc §6]). `Unknown` never ends a lease, never recovers an intent and never
/// reclaims anything.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Liveness {
    /// The process (or anchor) is alive.
    Alive,
    /// It is gone.
    Dead,
    /// It cannot be told.
    Unknown,
}

/// What ended a `wait_parent_or_wake` ([OS/proc §7]).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum WatchEvent {
    /// The parent process exited: the MCP server releases its slot (by exiting) and exits with code 0.
    ParentExited,
    /// Another thread of the server signalled the wake object.
    Woken,
}

/// Signals a waiting `wait_parent_or_wake` from another thread ([OS/proc §7], §10).
pub trait Wake: Send + Sync {
    /// Wakes the waiter.
    fn signal(&self);
}

/// The process seam ([80 §2.1] `os::proc`, `os::spawn`; [OS/proc §10]); a supertrait of `Vfs` ([OS/README §4.1]).
/// Implemented by `moirai_os::OsVfs` and by the simulator's `SimVfs` (per simulated process, [OS/proc §10.1]).
pub trait ProcHost {
    /// A registered watch of the parent process.
    type ParentWatch: Send;
    /// The wake object of the parent watch.
    type Wake: Wake;

    /// The OS tag of this build ([OS/proc §2]).
    fn os_tag(&self) -> OsTag;

    /// This process's `ProcId` ([OS/proc §3]); never fails — unreadable fields are flagged unknown.
    fn self_id(&self) -> ProcId;

    /// The parent record ([OS/proc §3.3]).
    fn parent(&self) -> Result<ParentRec, VfsError>;

    /// The boot identity, read once per process and cached ([OS/proc §4.4]).
    fn boot_identity(&self) -> BootIdentity;

    /// Diagnostics only ([OS/proc §6.1]).
    fn alive(&self, p: &ProcId) -> Liveness;

    /// Identifies the parent once, at server start ([OS/proc §7]); a parent that already exited gives a fired watch.
    fn watch_parent(&self) -> Result<Self::ParentWatch, VfsError>;

    /// A new wake object for `wait_parent_or_wake`.
    fn new_wake(&self) -> Result<Self::Wake, VfsError>;

    /// Blocks until the parent exits or `wake` is signalled; event-driven, no timer and no polling ([OS/proc §7]).
    fn wait_parent_or_wake(
        &self,
        w: &Self::ParentWatch,
        wake: &Self::Wake,
    ) -> Result<WatchEvent, VfsError>;

    /// The lower-case base name of the parent's executable without `.exe`; diagnostics-grade ([OS/proc §8]).
    fn parent_image(&self) -> Option<Box<str>>;

    /// Spawns the detached, low-priority `moirai gc` child and returns its pid ([OS/proc §11]). The only spawn in product
    /// code; asserts that no role byte is held.
    fn spawn_gc_child(
        &self,
        exe: &std::path::Path,
        args: &[&str],
        cwd: &std::path::Path,
    ) -> Result<u32, VfsError>;

    /// Lowers this process's own CPU, memory and I/O priority (the `gc` child calls it first; bulk passes too)
    /// ([OS/proc §11]).
    fn enter_background(&self);
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn os_tags() {
        assert_eq!(OsTag::Windows as u8, 1);
        assert_eq!(OsTag::from_u8(3), Some(OsTag::MacOs));
        assert_eq!(OsTag::from_u8(4), None);
    }

    #[test]
    fn proc_id_layout() {
        let p = ProcId {
            os: 1,
            flags: ProcId::START_KNOWN | ProcId::BOOT_KNOWN | 0xF0,
            pid: 0x0102_0304,
            start: 5,
            boot_hash: 7,
            pidns: 0,
        };
        let b = p.to_bytes();
        assert_eq!(b[..8], [1, 0b0101, 0, 0, 4, 3, 2, 1]);
        assert_eq!(b[8], 5);
        assert_eq!(b[16], 7);
        // Reserved flag bits are written as zero.
        assert_eq!(ProcId::from_bytes(&b), Some(ProcId { flags: 0b0101, ..p }));
        let mut bad = b;
        bad[0] = 0;
        assert_eq!(ProcId::from_bytes(&bad), None);
        bad = b;
        bad[1] |= 0x10;
        assert_eq!(ProcId::from_bytes(&bad), None);
        bad = b;
        bad[3] = 1;
        assert_eq!(ProcId::from_bytes(&bad), None);
    }

    #[test]
    fn boot_hash_is_never_zero() {
        assert_eq!(BootId([0; 16]).hash(), 1);
        assert_eq!(
            BootId([0x10, 0x32, 0, 0, 0, 0, 0, 0x80, 9, 9, 9, 9, 9, 9, 9, 9]).hash(),
            0x8000_0000_0000_3211
        );
        assert_eq!(BootIdentity::Unknown(UnknownBoot::Denied).boot_hash(), 0);
    }

    proptest! {
        #![proptest_config(crate::testing::proptest_config())]

        #[test]
        fn proc_id_round_trips(os in 1u8..=3, flags in 0u8..16, pid in any::<u32>(), start in any::<u64>(),
                               boot_hash in any::<u64>(), pidns in any::<u64>()) {
            let p = ProcId { os, flags, pid, start, boot_hash, pidns };
            prop_assert_eq!(ProcId::from_bytes(&p.to_bytes()), Some(p));
        }

        #[test]
        fn known_boot_hash_is_odd(id in any::<[u8; 16]>()) {
            prop_assert_eq!(BootId(id).hash() & 1, 1);
        }
    }
}
