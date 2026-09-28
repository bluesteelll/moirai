//! The environment guard: the `EnvGuard` sub-trait of `Vfs` and the classification types ([OS/env §2]; X-F6's
//! allow-lists and refusals, X-F5's "no downgrade" at `init`).
//!
//! A store is used only where every one of its guarantees holds ([80 §2.6]): a per-OS allow-list in which every allowed
//! file system is crash-gated, the full probe of the calls moirai needs at `init` and `restore`, and the OS-version
//! check. A refusal is exit 7 with its reason; nothing is downgraded ([80 §1] X5).

use core::fmt;

use crate::error::{OsCode, VfsError};
use crate::fs::VfsTypes;

/// The environment-guard sub-trait of `Vfs` ([OS/env §2]).
pub trait EnvGuard: VfsTypes {
    /// Classifies the volume and location of an open store root (role `Store`): at every open with `Open` (one volume
    /// query plus cheap checks, est. ≤ 20 µs), at `doctor` with `Full` ([OS/env §4]).
    fn classify(
        &self,
        store: &Self::Root,
        depth: ClassifyDepth,
    ) -> Result<Classification, VfsError>;

    /// `init` and `restore` only: `classify(Full)`, the OS-version check and the durable-write, lock and rename probes of
    /// [OS/env §5], in `store`'s `tmp/`. `Err` only for failures that are not about the location (for example `tmp/`
    /// missing).
    fn probe_store(&self, store: &Self::Root) -> Result<ProbeOutcome, VfsError>;

    /// Every open ([OS/env §6]).
    fn check_os_version(&self) -> Result<OsVersion, Refusal>;

    /// `doctor` only; never a refusal ([OS/env §8]).
    fn doctor_warnings(&self, store: &Self::Root) -> Vec<EnvWarning>;
}

/// How deep `classify` looks ([OS/env §1, §4]).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ClassifyDepth {
    /// Every open: the checks that fit the open-time budget.
    Open,
    /// `init`, `restore`, `doctor`: every check, without writes.
    Full,
}

/// The result of a classification ([OS/env §2]).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Classification {
    /// An allowed local volume.
    Local(StoreVolume),
    /// A refused location (exit 7 with the reason).
    Refused(Refusal),
}

/// What the guard learned about an allowed store volume ([OS/env §2]).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct StoreVolume {
    /// The file system.
    pub fs: FsKind,
    /// How `create_extent`/`recycle_extent` make zeros on this file system ([OS/fs §4.5]).
    pub extent_method: ExtentMethod,
    /// The volume is mounted read-only: readers work, writers exit 7 (`ReadOnlyVolume`).
    pub read_only: bool,
    /// A removable or external drive: allowed, and a `doctor` warning ([OS/env §8]).
    pub removable: bool,
}

/// An allowed file system ([OS/env §2, §3]); every one is crash-gated.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum FsKind {
    /// Windows NTFS.
    Ntfs,
    /// Linux ext4 (port phase).
    Ext4,
    /// Linux XFS (port phase).
    Xfs,
    /// Linux btrfs (port phase).
    Btrfs,
    /// macOS APFS (port phase).
    Apfs,
    /// Test builds only ([OS/env §9]): tmpfs `--ephemeral` or a LazyFS FUSE mount. `moirai-os` returns it only under its
    /// `test-host` feature, which the product root never enables.
    Ephemeral,
}

/// How a file system makes the zeros of a log extent ([OS/env §2], [OS/fs §4.5]).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ExtentMethod {
    /// Write zeros over the whole extent from one reused buffer of at most 1 MiB.
    ZeroFill,
    /// `fallocate(FALLOC_FL_WRITE_ZEROES)`, with a per-call fallback to `ZeroFill`.
    WriteZeroes,
    /// `ftruncate` after a free-space check of twice the extent; hole punching to recycle.
    Sparse,
}

/// A file-system name as the OS reports it (Windows `GetVolumeInformationByHandleW`, macOS `f_fstypename`), or the
/// Linux `statfs` magic as `0x` + 8 lower-case hex digits ([OS/env §2]). ASCII, at most 32 bytes.
#[derive(Copy, Clone, Eq, PartialEq, Hash)]
pub struct FsName {
    len: u8,
    bytes: [u8; 32],
}

impl FsName {
    /// The longest name.
    pub const MAX_LEN: usize = 32;

    /// `Some` if `name` is printable ASCII and at most 32 bytes.
    pub fn new(name: &str) -> Option<FsName> {
        let b = name.as_bytes();
        if b.len() > Self::MAX_LEN || !b.iter().all(|c| c.is_ascii_graphic() || *c == b' ') {
            return None;
        }
        let mut bytes = [0u8; 32];
        bytes[..b.len()].copy_from_slice(b);
        Some(FsName {
            len: b.len() as u8,
            bytes,
        })
    }

    /// Any OS-reported name: bytes that are not printable ASCII become `?`, and the name is cut at 32 bytes, so that a
    /// refusal can always name the file system.
    pub fn lossy(name: &[u8]) -> FsName {
        let n = name.len().min(Self::MAX_LEN);
        let mut bytes = [0u8; 32];
        for (dst, &src) in bytes.iter_mut().zip(&name[..n]) {
            *dst = if src.is_ascii_graphic() || src == b' ' {
                src
            } else {
                b'?'
            };
        }
        FsName {
            len: n as u8,
            bytes,
        }
    }

    /// The Linux form: the `statfs` magic as `0x` followed by 8 lower-case hex digits.
    pub fn from_linux_magic(magic: u32) -> FsName {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut bytes = [0u8; 32];
        bytes[0] = b'0';
        bytes[1] = b'x';
        for k in 0..8 {
            let nibble = (magic >> (28 - 4 * k)) & 0xF;
            bytes[2 + k] = HEX[nibble as usize];
        }
        FsName { len: 10, bytes }
    }

    /// The name.
    pub fn as_str(&self) -> &str {
        // Every constructor stores printable ASCII only.
        core::str::from_utf8(&self.bytes[..usize::from(self.len)]).unwrap_or("?")
    }
}

impl fmt::Debug for FsName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "FsName({:?})", self.as_str())
    }
}

impl fmt::Display for FsName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Why a location is refused ([OS/env §2, §3]). Each variant carries the stable reason id that [F19 §10.2]'s refusal texts
/// use ([`Refusal::reason_id`]).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Refusal {
    /// A local file system that is not on this OS's allow-list (ungated or unsupported). Reason id `fs-type`.
    FileSystem {
        /// The file system's name.
        name: FsName,
    },
    /// A network or remote volume. `network`.
    Network {
        /// The file system's name.
        name: FsName,
    },
    /// A UNC path, or a path whose final path resolves to UNC (a mapped network drive, `\\wsl$`, `\\wsl.localhost`).
    /// `unc`.
    Unc,
    /// Another kernel's file system (WSL2's 9p mounts under `/mnt/*`). `cross-kernel`.
    CrossKernel {
        /// The file system's name.
        name: FsName,
    },
    /// A cloud-managed folder. `cloud`.
    Cloud {
        /// Which kind.
        kind: CloudKind,
    },
    /// Any FUSE mount (sshfs, virtiofs, the gRPC-FUSE mounts of Docker Desktop and Lima, macFUSE). `fuse`.
    Fuse {
        /// The file system's name.
        name: FsName,
    },
    /// Any overlayfs (a container's own layer). `overlay`.
    Overlay,
    /// A file system without stable media (tmpfs, ramfs, a RAM disk). `volatile`.
    Volatile {
        /// The file system's name.
        name: FsName,
    },
    /// The full probe saw a durability call refused (for example `ENOTSUP` from `F_FULLFSYNC`). `no-durable-flush`.
    NoDurableFlush {
        /// The refused call.
        call: &'static str,
        /// The raw code.
        os: OsCode,
    },
    /// The full probe saw byte-range locks refused. `no-byte-locks`.
    NoByteLocks {
        /// The raw code.
        os: OsCode,
    },
    /// The full probe saw the no-replace rename refused. `no-noreplace-rename`.
    NoNoReplaceRename {
        /// The raw code.
        os: OsCode,
    },
    /// The OS is older than the floor of [OS/env §6]. `os-too-old`.
    OsTooOld {
        /// The running OS.
        found: OsVersion,
        /// The floor.
        minimum: OsVersion,
    },
}

impl Refusal {
    /// The stable reason token of [OS/env §2] that [F19 §10.2]'s refusal texts use.
    pub const fn reason_id(&self) -> &'static str {
        match self {
            Refusal::FileSystem { .. } => "fs-type",
            Refusal::Network { .. } => "network",
            Refusal::Unc => "unc",
            Refusal::CrossKernel { .. } => "cross-kernel",
            Refusal::Cloud { .. } => "cloud",
            Refusal::Fuse { .. } => "fuse",
            Refusal::Overlay => "overlay",
            Refusal::Volatile { .. } => "volatile",
            Refusal::NoDurableFlush { .. } => "no-durable-flush",
            Refusal::NoByteLocks { .. } => "no-byte-locks",
            Refusal::NoNoReplaceRename { .. } => "no-noreplace-rename",
            Refusal::OsTooOld { .. } => "os-too-old",
        }
    }
}

/// The kind of a cloud-managed folder ([OS/env §2]).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum CloudKind {
    /// Windows: a registered Cloud Files sync root (OneDrive and every other provider), a OneDrive folder, or
    /// `RECALL_ON_*`/`OFFLINE` attributes on the store directory or an ancestor.
    WindowsCloudFiles,
    /// macOS: iCloud Drive (`~/Library/Mobile Documents`).
    ICloudDrive,
    /// macOS: a File Provider root (`~/Library/CloudStorage/*`).
    FileProvider,
    /// macOS: a directory with `SF_DATALESS`.
    Dataless,
    /// macOS: `~/Desktop` or `~/Documents` managed by iCloud "Desktop & Documents".
    ICloudDesktopDocuments,
}

/// An OS version, ordered ([OS/env §6]): Windows `major.minor.build` from `RtlGetVersion`, Linux `major.minor.0` from
/// `uname`, macOS `major.minor.patch` from `kern.osproductversion`.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct OsVersion {
    /// Major version.
    pub major: u32,
    /// Minor version.
    pub minor: u32,
    /// Build (Windows) or patch number.
    pub build: u32,
}

impl fmt::Display for OsVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.build)
    }
}

/// What the full probe established ([OS/env §5] step 7).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProbeReport {
    /// The classified volume.
    pub volume: StoreVolume,
    /// The running OS.
    pub os: OsVersion,
}

/// The result of `probe_store` ([OS/env §5]).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProbeOutcome {
    /// Every check and probe passed.
    Admitted(ProbeReport),
    /// A check or probe refused the location; the probe files were removed.
    Refused(Refusal),
}

/// A `doctor` warning ([OS/env §8]); never a refusal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EnvWarning {
    /// Windows "turn off write-cache buffer flushing" set for the store's disk.
    FlushingDisabled,
    /// ext4 mounted with `barrier=0` or `nobarrier`.
    NoBarrier,
    /// A device whose `queue/write_cache` is set to write-through while it has a volatile cache.
    WriteCacheForcedWriteThrough,
    /// A removable or external drive (flushes may be ignored by some USB bridges).
    RemovableDrive,
    /// An OS release that is allowed but outside the supported and tested set ([OS/env §6]).
    UntestedOsRelease {
        /// The running OS.
        found: OsVersion,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fs_names() {
        assert_eq!(
            FsName::new("NTFS").map(|n| n.to_string()),
            Some("NTFS".to_owned())
        );
        assert!(FsName::new("é").is_none());
        assert!(FsName::new(&"x".repeat(33)).is_none());
        assert_eq!(FsName::lossy(b"Re\xffFS\x01").as_str(), "Re?FS?");
        assert_eq!(FsName::lossy(&[b'a'; 40]).as_str().len(), 32);
        assert_eq!(FsName::from_linux_magic(0xEF53).as_str(), "0x0000ef53");
        assert_eq!(FsName::from_linux_magic(0x9123_683E).as_str(), "0x9123683e");
        assert_eq!(FsName::new("apfs"), Some(FsName::lossy(b"apfs")));
    }

    #[test]
    fn reason_ids_and_versions() {
        let name = FsName::new("ReFS").unwrap();
        assert_eq!(Refusal::FileSystem { name }.reason_id(), "fs-type");
        assert_eq!(
            Refusal::Cloud {
                kind: CloudKind::WindowsCloudFiles
            }
            .reason_id(),
            "cloud"
        );
        let floor = OsVersion {
            major: 10,
            minor: 0,
            build: 17_134,
        };
        assert!(
            OsVersion {
                major: 10,
                minor: 0,
                build: 26_200
            } > floor
        );
        assert!(
            OsVersion {
                major: 10,
                minor: 0,
                build: 17_133
            } < floor
        );
        assert_eq!(floor.to_string(), "10.0.17134");
    }
}
