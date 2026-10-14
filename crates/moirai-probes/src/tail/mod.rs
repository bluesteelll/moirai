//! Measurement 10's log tail (WP-53d): the record and group codec of [F05] for the subset of record kinds a tail of
//! commits and settles holds, the `Commit` payload of [F06] for local ordinary commits, the runtime records of a
//! settle ([F05 §9.18], [F05 §9.23] with [F11 §12.4], [F11 §12.5]), and the streaming scan of [F05 §5].
//!
//! `[Fnn §x]` cites `docs/spec/format/nn-*.md`; every size, offset and code below also names its line, so a reviewer
//! can check it against the chapter (R-24 Q3). The codec is a probe's, not the product's: no frozen byte depends on it.
//!
//! | Module | What it holds | Specification |
//! |---|---|---|
//! | [`enc`] | `uvar16/32/64`, `svar64`, `bool8`, `vbytes`, `vstr`, little-endian fixed widths, and a refusing reader | [F01 §5], [F01 §6.2] (`01-conventions.md:236-377`) |
//! | [`rec`] | `RecHdr`, the record checksum, groups and the chain trailer, the chain value at a boundary, `ExtentHead`, `Noop`, `SymDefs`, placement G-1 to G-5, and `encode` | [F05 §3], [F05 §4], [F05 §8.1], [F05 §9.12], [F05 §9.28] (`05-log.md:116-266`, `:419-460`, `:887-894`, `:1111-1139`) |
//! | [`commit`] | the `Commit` payload of a local ordinary commit and its eight op kinds; item 1a's locator in the form a tail chooses ([`Locator`]: none, or design-A's A1), and each owner's frame span in a payload | [F06 §4], [F06 §5], [F06 §6], [F06 §7], [F06 §8] (`06-commit.md:168-534`, `:536-576`, `:600-865`, `:1040-1067`); `design-A.md:104-129` (A1, a probe form until the 1a spec unit, R-47) |
//! | [`rt`] | `TreeReg` (sub 2, the settle epoch) and the `FileObs` row batch with the 187-byte row image | [F05 §8.5], [F05 §9.18], [F05 §9.23] (`05-log.md:507-521`, `:974-981`, `:1006-1032`); [F11 §2.9], [F11 §12.1]–§12.5 (`11-runtime-tables.md:278-298`, `:764-914`) |
//! | [`ckpt`] | the `Checkpoint` record (kind 9, durable) of a delta checkpoint: its payload, the `SegRef` and `FileEntry` forms, and its one-record group, framed by [`rec::raw_group`] outside the encoder's and the scan's subset | [F05 §8.2], [F05 §8.4], [F05 §9.9] (`05-log.md:462-505`, `:728-807`) |
//! | [`scan`] | `LogSource` and `MemLog`, the streaming scan with record and group validity, and `decode` | [F05 §5], [F05 §10] (`05-log.md:268-341`, `:1232-1307`) |
//! | [`store`] | `VfsLog` (a `LogSource` over `log.<n>` files), the open floor (`HEAD` with one 8,192-B read, 8 sealed files read and mapped), the fixture writer | `60-roadmap.md:790`; [F04 §2], [F04 §4.1]; [F09 §2.1]; `docs/spec/os/map.md:84-90` |
//! | [`head`] | `HeadSlot` with its `xxh3_128`, the slot checks, the selection of F04 §8.1 and the publish into the other slot | [F04 §3], [F04 §7], [F04 §8.1], [F04 §9.1] (`04-head.md:34-121`, `:342-397`, `:421-438`) |
//! | [`synth`] | the generator of measurement 10's tails: the spec-derived mix, the stop rules, the tail measures | [F17 §5.1]–§5.3 (`17-store-parameters.md:313-369`); [70 S8]; [60 §5.2] item 2 |
//!
//! **The subset.** The tail holds exactly five kinds: `ExtentHead` (28), `Noop` (12, the rotation pad), `Commit` (1),
//! `TreeReg` (23, sub 2) and `FileObs` (18, upsert rows). The encoder and the scan refuse every other kind, and every
//! payload feature outside the subset, with an error that says so ("outside the probe's subset"); they refuse a
//! malformed byte with the rule it breaks. Node kinds, statuses, resolutions, edge kinds and field names are opaque
//! codes the probe chooses in small fixed ranges ([F08], [F18] and [F20] enumerate them; the probe uses no table of
//! theirs).

pub mod ckpt;
pub mod commit;
pub mod enc;
pub mod head;
pub mod rec;
pub mod rt;
pub mod scan;
pub mod store;
pub mod synth;

#[cfg(test)]
pub(crate) mod memfs;
#[cfg(test)]
pub(crate) mod testdata;

/// The length of `RecHdr` ([F05 §3.1], `05-log.md:118-129`).
pub const REC_HDR_LEN: usize = 32;
/// The chain trailer that ends a group's last record ([F05 §3.3], [F05 §4.2], `05-log.md:143-149`, `:169-176`).
pub const TRAILER_LEN: usize = 8;
/// The smallest group: one record with an empty payload and the trailer ([F05 §3.1], [F05 §4.4],
/// `05-log.md:122`, `:217`).
pub const MIN_GROUP_LEN: u64 = 40;
/// The fixed payload length of `ExtentHead` ([F05 §9.28], `05-log.md:1113-1114`).
pub const EXTENT_HEAD_PAYLOAD_LEN: usize = 98;
/// H, the length of an extent-head group: 32 + 98 + 8 ([F05 §4.4], [F05 §9.28], `05-log.md:191`, `:221-222`,
/// `:1113-1114`).
pub const EXTENT_HEAD_GROUP_LEN: u64 = 138;
/// R, the rotation reserve H + 40 ([F05 §4.4] G-2, `05-log.md:191-195`; [F17 §4.4] W3, `17-store-parameters.md:287-289`).
pub const ROTATION_RESERVE: u64 = 178;
/// The length of `HEAD`, read with one `read_at` ([F04 §2], `04-head.md:25`, `:31`).
pub const HEAD_LEN: usize = 8_192;
/// The length of `InitParams` ([F04 §4.4], `04-head.md:147-153`).
pub const INIT_PARAMS_LEN: usize = 32;
/// The raw body cap ([F06 §8], `06-commit.md:1049`; [AR §2.6]).
pub const BODY_RAW_CAP: u32 = 65_536;
/// The fixed buffer the tail is read through: the overlay builder "streams through a fixed buffer, never reads extents
/// whole" (`docs/ARCHITECTURE-RESEARCH.md:871`; [20] F-D1, `20-critique-perf-ram-windows.md:40`).
pub const TAIL_READ_CHUNK: usize = 65_536;

/// The record kinds of the subset ([F05 §7], `05-log.md:371-402`).
pub mod kind {
    /// `Commit`, durable (`05-log.md:373`).
    pub const COMMIT: u8 = 1;
    /// `Noop`, lazy: the rotation pad (`05-log.md:384`).
    pub const NOOP: u8 = 12;
    /// `FileObs`, lazy, `K_RT` (`05-log.md:390`).
    pub const FILE_OBS: u8 = 18;
    /// `TreeReg`, lazy, `K_RT` (`05-log.md:395`).
    pub const TREE_REG: u8 = 23;
    /// `ExtentHead`, durable (`05-log.md:400`).
    pub const EXTENT_HEAD: u8 = 28;
    /// The greatest valid kind; 0 and 31–255 are invalid (`05-log.md:404`).
    pub const MAX_VALID: u8 = 30;
}

/// Whether `kind` is in `K_RT`, the lazy runtime kinds 18–26 ([F05 §7] runtime column, `05-log.md:390-398`, `:405`;
/// [F17 §5.1], `17-store-parameters.md:318`).
pub const fn in_k_rt(kind: u8) -> bool {
    kind >= 18 && kind <= 26
}

/// The `RecHdr.flags` bits ([F05 §3.2], `05-log.md:131-139`).
pub mod flag {
    /// Bit 0: the record's class is `lazy`.
    pub const LAZY: u8 = 1;
    /// Bit 1: the record ends its group with the chain trailer.
    pub const GROUP_END: u8 = 2;
    /// Bit 2: the payload begins with a `SymDefs` block.
    pub const SYMDEFS: u8 = 4;
    /// Bits 3–7, reserved-zero.
    pub const RESERVED: u8 = 0xF8;
}

/// The extent length E and the lsn mapping of [F05 §2.3] (`05-log.md:58-67`): extent `n(L) = (L >> k) + 1`, offset
/// `o(L) = L & (E − 1)`.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Geometry {
    e: u64,
}

impl Geometry {
    /// E must be a power of two in [2^16, 2^30] ([F05 §2.2], `05-log.md:47-50`; [F17] P01, `17-store-parameters.md:166`).
    pub const fn new(e: u64) -> Option<Geometry> {
        if e.is_power_of_two() && e >= 1 << 16 && e <= 1 << 30 {
            Some(Geometry { e })
        } else {
            None
        }
    }

    /// E.
    pub const fn e(&self) -> u64 {
        self.e
    }

    /// `n(L)`, the extent file number that holds lsn L.
    pub const fn extent(&self, lsn: u64) -> u64 {
        (lsn >> self.e.trailing_zeros()) + 1
    }

    /// `o(L)`, L's offset inside its extent.
    pub const fn offset(&self, lsn: u64) -> u64 {
        lsn & (self.e - 1)
    }

    /// The first lsn of extent `n` (n ≥ 1).
    pub const fn extent_start(&self, n: u64) -> u64 {
        (n - 1) * self.e
    }

    /// The first lsn after the extent that holds `lsn`.
    pub const fn extent_end(&self, lsn: u64) -> u64 {
        self.extent_start(self.extent(lsn) + 1)
    }
}

/// The form of item 1a's locator in a tail's op frames (R-47 item 1a, R-50; design-A's A1, u20 cut D-1). The form is a
/// parameter of the probe, not a frozen byte: the 1a spec unit's form, if it differs, is one more variant with its own
/// [`commit::put_loc`], [`commit::get_loc`] and validation, and a re-run.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Hash)]
pub enum Locator {
    /// No locator: the night's form, [F06 §7.4]'s op bodies as they are.
    #[default]
    None,
    /// Design-A's A1 (`design-A.md:104-129`): in the first frame of each owner in a record, when `prev` ≠ 0, `prev_at`
    /// and `prev_len` (`uvar32` each) follow `prev`. Later frames of the same owner carry none.
    A1,
}

/// Item 1a's value: where the owner's frames lie in the record its `prev` names. `at` counts from that record's lsn,
/// its header included; `len` is the bytes of the owner's frames there. (0, 0) when the writer does not know them: the
/// record lies below the checkpoint (SoT-4), the writer's overlay holds no entry for it (SoT-2, a lane), or it is a bulk
/// commit (SoT-3) (u20 cut §3.1).
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Hash, PartialOrd, Ord)]
pub struct PrevLoc {
    /// `prev_at`.
    pub at: u32,
    /// `prev_len`.
    pub len: u32,
}

/// The shape of a tail beyond F17's parameters: the locator's form and the depth option.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Shape {
    /// The form of item 1a's locator.
    pub locator: Locator,
    /// The depth option (u20 cut §3.4, M-A1's depth arm): when d > 0, the base node #⌈N/2⌉
    /// ([`synth::hot_node`]) is an owner in each of the tail's first d commits and in no other, its first op setting
    /// field [`synth::HOT_FIELD`] and the later ones its status; 0 is none.
    pub hot: u32,
}

impl Shape {
    /// The night's shape, no locator. Every quantity measured before u20 keeps it, so its tails stay byte-identical
    /// (u20 cut D-3).
    pub const NIGHT: Shape = Shape {
        locator: Locator::None,
        hot: 0,
    };

    /// Design-A's A1, the decided format (R-50).
    pub const A1: Shape = Shape {
        locator: Locator::A1,
        hot: 0,
    };
}

/// The store parameters a tail depends on ([F17 §3], `17-store-parameters.md:164-178`). The holes P06–P11 take their
/// design figures until WP-81a fills them ([F17 §3], `:215-217`), which is also what the grid's candidates replace.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Params {
    /// P01 `store.log-extent-bytes` (E).
    pub e: u64,
    /// P03 `store.hist-frame-commits` (carried in `InitParams`).
    pub hist_frame_commits: u32,
    /// P04 `store.hist-frame-bytes` (carried in `InitParams`).
    pub hist_frame_bytes: u32,
    /// P05 `store.commit.inline-max-bytes`.
    pub p05: u64,
    /// P06 `store.checkpoint.ops`.
    pub p06: u64,
    /// P07 `store.checkpoint.bytes`.
    pub p07: u64,
    /// P08 `store.checkpoint.body-bytes`.
    pub p08: u64,
    /// P09 `store.tail.max-overlay-bytes`.
    pub p09: u64,
    /// P10 `store.tail.max-overlay-bytes.quiet`.
    pub p10: u64,
    /// P11 `quiet.tail-cap-multiplier`.
    pub p11: u64,
    /// P12 `store.tail.runtime-bytes`.
    pub p12: u64,
    /// P13 `maintenance.cli-threshold-multiplier`.
    pub p13: u64,
    /// The tail's shape: [`Shape::NIGHT`] in both columns.
    pub shape: Shape,
}

const KIB: u64 = 1_024;
const MIB: u64 = 1_024 * 1_024;

impl Params {
    /// The production column, holes at their design figures (`17-store-parameters.md:166-178`, `:215-217`).
    pub const PRODUCTION: Params = Params {
        e: 64 * MIB,
        hist_frame_commits: 256,
        hist_frame_bytes: MIB as u32,
        p05: MIB,
        p06: 4_096,
        p07: 4 * MIB,
        p08: 32 * MIB,
        p09: MIB,
        p10: 2 * MIB,
        p11: 8,
        p12: 2 * MIB,
        p13: 2,
        shape: Shape::NIGHT,
    };

    /// The test profile column (`17-store-parameters.md:166-178`).
    pub const TEST: Params = Params {
        e: 64 * KIB,
        hist_frame_commits: 4,
        hist_frame_bytes: 4 * KIB as u32,
        p05: 4 * KIB,
        p06: 8,
        p07: 2 * KIB,
        p08: 8 * KIB,
        p09: KIB,
        p10: 2 * KIB,
        p11: 2,
        p12: KIB,
        p13: 2,
        shape: Shape::NIGHT,
    };

    /// The geometry of E.
    pub fn geometry(&self) -> Geometry {
        Geometry::new(self.e).expect("P01 is a power of two in [2^16, 2^30]")
    }
}
