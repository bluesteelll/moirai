//! [F11] runtime tables: the section body `RtHdr` (§2.1), rows, keys and order (§2.2), the heap and `HeapRef` (§2.3),
//! forms and dead rows (§2.4), the shared field types (§2.5, §12.1–§12.3), every table's row (§3–§13) and the row images
//! log records carry (§2.9).
//!
//! Rows are described by static field tables (one per section, in offset-table order) and decoded into typed values
//! ([`FV`]); each table then applies its own rules. One decoder serves the section form and the row-image form.

use std::cmp::Ordering;

use crate::commit::{CKey, KVal};
use crate::lock::{Anchor, ProcId};
use crate::prim::{Oid, Reader, Result, Writer, blake3_128, err, utf8};
use crate::value::PathVal;

/// `Stamp` ([OS/clock §3.1]), 24 bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Stamp {
    /// Wall clock ms.
    pub wall: u64,
    /// Writer's boot hash; 0 = Unknown-boot.
    pub boot_hash: u64,
    /// Boot clock ns.
    pub mono: u64,
}

impl Stamp {
    /// `Stamp::NEVER` ([OS/clock §4.1]).
    pub const NEVER: Stamp = Stamp {
        wall: u64::MAX,
        boot_hash: 0,
        mono: u64::MAX,
    };

    /// Decodes 24 bytes; `mono` is 0 when `boot_hash` is 0, except in `NEVER` ([OS/clock §3.1], §4.1).
    pub fn decode(r: &mut Reader<'_>) -> Result<Self> {
        let at = r.offset();
        let s = Stamp {
            wall: r.u64()?,
            boot_hash: r.u64()?,
            mono: r.u64()?,
        };
        if s.boot_hash == 0 && s.mono != 0 && s != Stamp::NEVER {
            return err(
                at + 16,
                "Stamp.mono is not 0 with boot_hash 0 [OS/clock §3.1]",
            );
        }
        Ok(s)
    }

    /// Encodes 24 bytes.
    pub fn encode(&self, w: &mut Writer) {
        w.u64(self.wall);
        w.u64(self.boot_hash);
        w.u64(self.mono);
    }
}

/// `OsFileId` ([F11 §12.1]), 57 bytes; kept verbatim (a foreign or reserved value is absent, not invalid).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct OsFileId {
    /// 0 none … 4 darwin_fileid.
    pub kind: u8,
    /// Volume key.
    pub vol_key: [u8; 16],
    /// Object id.
    pub id: [u8; 16],
    /// Parent directory id.
    pub parent: [u8; 16],
    /// Reserved, zero to be interpretable.
    pub aux: u32,
    /// macOS document id.
    pub docid: u32,
}

impl OsFileId {
    /// Decodes 57 bytes.
    pub fn decode(r: &mut Reader<'_>) -> Result<Self> {
        Ok(OsFileId {
            kind: r.u8()?,
            vol_key: r.b16()?,
            id: r.b16()?,
            parent: r.b16()?,
            aux: r.u32()?,
            docid: r.u32()?,
        })
    }

    /// Encodes 57 bytes.
    pub fn encode(&self, w: &mut Writer) {
        w.u8(self.kind);
        w.bytes(&self.vol_key);
        w.bytes(&self.id);
        w.bytes(&self.parent);
        w.u32(self.aux);
        w.u32(self.docid);
    }

    /// [F11 §12.1]: interpretable when the kind is known, a kind-0 value is all zero and `aux` is zero.
    pub fn interpretable(&self) -> bool {
        self.kind <= 4 && self.aux == 0 && (self.kind != 0 || *self == OsFileId::default())
    }
}

/// `FsTime` ([F11 §12.2]), 9 bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct FsTime {
    /// `unix_ns`.
    pub ns: i64,
    /// Decimal exponent 0–10, `0xFF` absent.
    pub gran: u8,
}

impl FsTime {
    /// Decodes 9 bytes.
    pub fn decode(r: &mut Reader<'_>) -> Result<Self> {
        Ok(FsTime {
            ns: r.i64()?,
            gran: r.u8()?,
        })
    }

    /// Encodes 9 bytes.
    pub fn encode(&self, w: &mut Writer) {
        w.i64(self.ns);
        w.u8(self.gran);
    }
}

/// `VolumeCaps` ([F11 §12.3]), 16 bytes, kept verbatim (a reserved value makes it uninterpretable, not invalid).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct VolumeCaps {
    /// Capability flags.
    pub flags: u32,
    /// Id kind.
    pub id_kind: u8,
    /// Birth-time class.
    pub btime: u8,
    /// Case rule.
    pub case_rule: u8,
    /// Cloud class.
    pub cloud: u8,
    /// Effective mtime granularity.
    pub mtime_granularity_ns: u64,
}

impl VolumeCaps {
    /// Decodes 16 bytes.
    pub fn decode(r: &mut Reader<'_>) -> Result<Self> {
        Ok(VolumeCaps {
            flags: r.u32()?,
            id_kind: r.u8()?,
            btime: r.u8()?,
            case_rule: r.u8()?,
            cloud: r.u8()?,
            mtime_granularity_ns: r.u64()?,
        })
    }

    /// Encodes 16 bytes.
    pub fn encode(&self, w: &mut Writer) {
        w.u32(self.flags);
        w.u8(self.id_kind);
        w.u8(self.btime);
        w.u8(self.case_rule);
        w.u8(self.cloud);
        w.u64(self.mtime_granularity_ns);
    }
}

/// `BindingExt` ([F18 §3.2]), 40 bytes, kept verbatim: an invalid extension makes its row non-designated, never the
/// section invalid ([F11 §5.2]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BindingExt {
    /// Bit 0 `designated`.
    pub bflags: u8,
    /// Object format of `base`.
    pub base_algo: u8,
    /// `_reserved` as read.
    pub reserved: [u8; 2],
    /// Symbol, class `git-branch`.
    pub expected_ref: u32,
    /// Fixed 32-byte id slot.
    pub base: [u8; 32],
}

impl BindingExt {
    /// Decodes 40 bytes.
    pub fn decode(r: &mut Reader<'_>) -> Result<Self> {
        Ok(BindingExt {
            bflags: r.u8()?,
            base_algo: r.u8()?,
            reserved: r.array()?,
            expected_ref: r.u32()?,
            base: r.b32()?,
        })
    }

    /// Encodes 40 bytes.
    pub fn encode(&self, w: &mut Writer) {
        w.u8(self.bflags);
        w.u8(self.base_algo);
        w.bytes(&self.reserved);
        w.u32(self.expected_ref);
        w.bytes(&self.base);
    }

    /// True when all 40 bytes are zero.
    pub fn is_zero(&self) -> bool {
        self.bflags == 0
            && self.base_algo == 0
            && self.reserved == [0; 2]
            && self.expected_ref == 0
            && self.base == [0; 32]
    }

    /// [F18 §3.2] rule 3: the extension is valid (rules 2 and 3).
    pub fn valid(&self) -> bool {
        if self.bflags & 0xFE != 0 || self.reserved != [0; 2] || self.base_algo > 2 {
            return false;
        }
        if self.base_algo == 0 && self.base != [0; 32] {
            return false;
        }
        if self.base_algo == 1 && self.base[20..] != [0; 12] {
            return false;
        }
        self.bflags & 1 != 0 || (self.base_algo == 0 && self.expected_ref == 0)
    }
}

/// A settle epoch ([F11 §12.4]), 32 bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Epoch {
    /// 0 full-tree, 1 lane-owned, 2 partial.
    pub scope_kind: u8,
    /// Lane `ref_id` for lane-owned; else 0.
    pub scope_ref: u32,
    /// Scope digest.
    pub digest: [u8; 16],
    /// Settle HLC.
    pub hlc: u64,
}

impl Epoch {
    /// Decodes and validates 32 bytes.
    pub fn decode(r: &mut Reader<'_>) -> Result<Self> {
        let at = r.offset();
        let scope_kind = r.u8()?;
        if scope_kind > 2 {
            return err(at, "Epoch.scope_kind outside 0-2 [F11 §12.4]");
        }
        r.zeros(3, "Epoch._reserved")?;
        let e = Epoch {
            scope_kind,
            scope_ref: r.u32()?,
            digest: r.b16()?,
            hlc: r.u64()?,
        };
        if (e.scope_kind != 1 && (e.scope_ref != 0 || e.digest != [0; 16]))
            || (e.scope_kind == 1 && e.scope_ref == 0)
        {
            return err(
                at,
                "Epoch scope_ref or digest breaks its scope_kind [F11 §12.4]",
            );
        }
        Ok(e)
    }

    /// Encodes 32 bytes.
    pub fn encode(&self, w: &mut Writer) {
        w.u8(self.scope_kind);
        w.zeros(3);
        w.u32(self.scope_ref);
        w.bytes(&self.digest);
        w.u64(self.hlc);
    }
}

/// An `OidSlot` ([F11 §2.5]): `algo` then the fixed 32-byte slot.
pub fn decode_oid_slot(r: &mut Reader<'_>) -> Result<Oid> {
    let algo = r.u8()?;
    r.oid_slot(algo)
}

/// Encodes an `OidSlot`.
pub fn encode_oid_slot(o: &Oid, w: &mut Writer) {
    w.u8(o.algo_byte());
    w.oid_slot(o);
}

/// The section tables of [F11 §1.3] and their tags ([F11 §2.8]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Table {
    /// `REFS` 0x0201.
    Refs,
    /// `PINS` 0x0202.
    Pins,
    /// `HEADS` 0x0203.
    Heads,
    /// `LEASES` 0x0204.
    Leases,
    /// `MARKERS` 0x0205.
    Markers,
    /// `MARKERS_OLD` 0x0206.
    MarkersOld,
    /// `IDEM` 0x0207.
    Idem,
    /// `ALLOC` 0x0208.
    Alloc,
    /// `UIDX` 0x0209.
    Uidx,
    /// `CURSORS` 0x020A.
    Cursors,
    /// `SESSMARKS` 0x020B.
    Sessmarks,
    /// `BACKUPS` 0x020C.
    Backups,
    /// `TREES` 0x0210.
    Trees,
    /// `FILEOBS` 0x0211.
    FileObs,
    /// `PENDING` 0x0212.
    Pending,
    /// `FSINTENT` 0x0213.
    FsIntent,
    /// `FPRINT` 0x0214.
    FPrint,
    /// `JOURNALCUR` 0x0215.
    JournalCur,
    /// `DIRMAP` 0x0216.
    DirMap,
    /// `PREFIXEV` 0x0217.
    PrefixEv,
    /// `ANCESTRY` 0x0218.
    Ancestry,
    /// `GITRENAMES` 0x0219.
    GitRenames,
    /// `ANCHORRES` 0x021A.
    AnchorRes,
    /// `CONFLICTS` 0x0031 (versioned).
    Conflicts,
    /// `GLOBIDX` 0x0084 (versioned).
    GlobIdx,
    /// `SCHEMAIDS` 0x0101 ([F09 §14.3], an [F11 §2.1] body).
    SchemaIds,
    /// `FILES` 0x0102 ([F09 §14.4], an [F11 §2.1] body).
    Files,
}

/// Every table.
pub const TABLES: [Table; 27] = [
    Table::Refs,
    Table::Pins,
    Table::Heads,
    Table::Leases,
    Table::Markers,
    Table::MarkersOld,
    Table::Idem,
    Table::Alloc,
    Table::Uidx,
    Table::Cursors,
    Table::Sessmarks,
    Table::Backups,
    Table::Trees,
    Table::FileObs,
    Table::Pending,
    Table::FsIntent,
    Table::FPrint,
    Table::JournalCur,
    Table::DirMap,
    Table::PrefixEv,
    Table::Ancestry,
    Table::GitRenames,
    Table::AnchorRes,
    Table::Conflicts,
    Table::GlobIdx,
    Table::SchemaIds,
    Table::Files,
];

/// A fixed field kind of a row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum K {
    /// `u8`.
    U8,
    /// `u16`.
    U16,
    /// `u32`.
    U32,
    /// `u64`.
    U64,
    /// `i64`.
    I64,
    /// `b16`.
    B16,
    /// `b32`.
    B32,
    /// A fixed 32-byte id slot of [F01 §7.5] whose algorithm is the row's `algo` field.
    Slot32,
    /// Reserved zero bytes.
    Z(usize),
    /// `OidSlot`, 33.
    OidSlot,
    /// `FileRef`, 9.
    FileRef,
    /// `Stamp`, 24.
    Stamp,
    /// `Anchor`, 32.
    Anchor,
    /// `ProcId`, 32.
    ProcId,
    /// `OsFileId`, 57.
    OsFileId,
    /// `FsTime`, 9.
    FsTime,
    /// `FileAttrs`, 4.
    Attrs,
    /// `VolumeCaps`, 16.
    Caps,
    /// `BindingExt`, 40.
    Binding,
    /// A `HeapRef` and its slice type.
    Heap(S),
}

impl K {
    /// Width in the row.
    pub const fn width(self) -> usize {
        match self {
            K::U8 => 1,
            K::U16 => 2,
            K::U32 | K::Attrs => 4,
            K::U64 | K::I64 | K::Heap(_) => 8,
            K::B16 => 16,
            K::B32 | K::Slot32 | K::Anchor | K::ProcId => 32,
            K::Z(n) => n,
            K::OidSlot => 33,
            K::FileRef | K::FsTime => 9,
            K::Stamp => 24,
            K::OsFileId => 57,
            K::Caps => 16,
            K::Binding => 40,
        }
    }
}

/// A slice type ([F11 §2.3]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum S {
    /// `text`.
    Text,
    /// `path` ([F08 §5.2]).
    Path,
    /// `list<u32>`.
    ListU32,
    /// `list<u64>`.
    ListU64,
    /// `list<AbsorbedEntry>` (§3.4).
    Absorbed,
    /// `list<PinHolder>` (§4).
    PinHolders,
    /// `list<vstr>`.
    ListVstr,
    /// `list<SensEntry>` (§12.4).
    Sens,
    /// `list<Epoch>` (§12.4).
    Epochs,
    /// The `FILEOBS` `var` sequence (§12.5).
    FileObsVar,
    /// `n_items` × `IntentItem` (§12.7).
    Items,
    /// The `GITRENAMES` `list` sequence (§12.12).
    Renames,
    /// Opaque bytes (`IDEM.result`).
    Raw,
    /// A `ckey` (`CONFLICTS.key`).
    CKey,
    /// A `kval` of the row's key class (`CONFLICTS` sides).
    KVal,
}

/// A row's field table: (name, kind).
pub type Fields = &'static [(&'static str, K)];

const REFS: Fields = &[
    ("ref_id", K::U32),
    ("kind", K::U8),
    ("flags", K::U8),
    ("_reserved", K::Z(2)),
    ("tip", K::B16),
    ("tip_lsn", K::U64),
    ("gen", K::U32),
    ("ref_seq_next", K::U32),
    ("base_pin", K::U64),
    ("fork_commit", K::B16),
    ("fork_lsn", K::U64),
    ("fork_seq", K::U64),
    ("fork_ref_id", K::U32),
    ("ops_since_fork", K::U32),
    ("overlay_ops", K::U32),
    ("overlay_bytes", K::U32),
    ("promoted_seg", K::U32),
    ("ops_total", K::U64),
    ("bytes_total", K::U64),
    ("trunk_mark_ops", K::U64),
    ("trunk_mark_bytes", K::U64),
    ("name", K::Heap(S::Text)),
    ("absorbed", K::Heap(S::Absorbed)),
    ("moves", K::Heap(S::ListU64)),
    ("message", K::Heap(S::Text)),
];

const PINS: Fields = &[
    ("file", K::FileRef),
    ("refcount", K::U32),
    ("holders", K::Heap(S::PinHolders)),
];

const HEADS: Fields = &[
    ("key", K::B16),
    ("kind", K::U8),
    ("flags", K::U8),
    ("os", K::U8),
    ("_reserved", K::Z(1)),
    ("ref_id", K::U32),
    ("detached", K::B16),
    ("detached_lsn", K::U64),
    ("root_id", K::OsFileId),
    ("binding", K::Binding),
    ("hlc", K::U64),
    ("text", K::Heap(S::Text)),
];

const LEASES: Fields = &[
    ("n", K::U32),
    ("lease_id", K::U64),
    ("kind", K::U8),
    ("flags", K::U8),
    ("role", K::U16),
    ("holder", K::U32),
    ("branch", K::U32),
    ("run", K::U32),
    ("token", K::U64),
    ("claimed_hlc", K::U64),
    ("ttl_ms", K::U64),
    ("expires", K::Stamp),
    ("anchor", K::Anchor),
    ("bound", K::B16),
    ("root_session", K::B16),
    ("proc", K::ProcId),
    ("files_owned", K::Heap(S::ListVstr)),
];

const MARKERS: Fields = &[
    ("n", K::U32),
    ("ref_id", K::U32),
    ("commit", K::B16),
    ("kind", K::U8),
    ("status", K::U8),
    ("cause", K::U8),
    ("flags", K::U8),
    ("ref_seq", K::U32),
    ("actor", K::U32),
    ("outcome", K::U8),
    ("_reserved", K::Z(3)),
    ("hlc", K::U64),
    ("seq", K::U64),
    ("emit_lsn", K::U64),
    ("holders", K::Heap(S::ListU32)),
];

const IDEM: Fields = &[
    ("key", K::B16),
    ("payload", K::B16),
    ("ref_id", K::U32),
    ("branch_sym", K::U32),
    ("ref_seq", K::U32),
    ("flags", K::U8),
    ("_reserved", K::Z(3)),
    ("append_hlc", K::U64),
    ("origin_lsn", K::U64),
    ("result", K::Heap(S::Raw)),
];

const ALLOC: Fields = &[("uid", K::B16), ("ref_id", K::U32), ("create_seq", K::U32)];

const UIDX: Fields = &[("uid", K::B16), ("n", K::U32)];

const CONFLICTS: Fields = &[
    ("n", K::U32),
    ("class", K::U8),
    ("prov", K::U8),
    ("_reserved", K::Z(2)),
    ("commit", K::B16),
    ("key", K::Heap(S::CKey)),
    ("base", K::Heap(S::KVal)),
    ("ours", K::Heap(S::KVal)),
    ("theirs", K::Heap(S::KVal)),
];

const GLOBIDX: Fields = &[
    ("n", K::U32),
    ("field", K::U32),
    ("prefix_len", K::U32),
    ("glob", K::Heap(S::Text)),
];

const TREES: Fields = &[
    ("key", K::B16),
    ("os", K::U8),
    ("flags", K::U8),
    ("root_id", K::OsFileId),
    ("caps", K::Caps),
    ("last_head", K::OidSlot),
    ("last_settle_hlc", K::U64),
    ("dirty_count", K::U32),
    ("dirty_head", K::OidSlot),
    ("dirty_hlc", K::U64),
    ("root_text", K::Heap(S::Text)),
    ("sens", K::Heap(S::Sens)),
    ("epochs", K::Heap(S::Epochs)),
];

const FILEOBS: Fields = &[
    ("n", K::U32),
    ("tree", K::B16),
    ("state", K::U8),
    ("flags", K::U8),
    ("resolver_version", K::U16),
    ("n_proposals", K::U8),
    ("file_id", K::OsFileId),
    ("size", K::U64),
    ("mtime", K::FsTime),
    ("ctime", K::FsTime),
    ("creation", K::FsTime),
    ("added", K::FsTime),
    ("attrs", K::Attrs),
    ("last_oid", K::OidSlot),
    ("verified_at", K::U64),
    ("missing_since", K::U64),
    ("var", K::Heap(S::FileObsVar)),
];

const PENDING: Fields = &[
    ("n", K::U32),
    ("tree", K::B16),
    ("class", K::U8),
    ("source", K::U8),
    ("flags", K::U8),
    ("evidence", K::U8),
    ("oid", K::OidSlot),
    ("creation", K::FsTime),
    ("head", K::OidSlot),
    ("hlc", K::U64),
    ("from", K::Heap(S::Path)),
    ("to", K::Heap(S::Path)),
];

const FSINTENT: Fields = &[
    ("intent_id", K::U64),
    ("op", K::U8),
    ("state", K::U8),
    ("flags", K::U8),
    ("reason", K::U8),
    ("branch", K::U32),
    ("tree", K::B16),
    ("anchor", K::Anchor),
    ("proc", K::ProcId),
    ("hlc", K::U64),
    ("closed_lsn", K::U64),
    ("closed_hlc", K::U64),
    ("n_items", K::U32),
    ("items", K::Heap(S::Items)),
];

const FPRINT: Fields = &[
    ("oid", K::OidSlot),
    ("blob", K::B16),
    ("file", K::U32),
    ("flags", K::U8),
];

const JOURNALCUR: Fields = &[
    ("kind", K::U8),
    ("vol_key", K::B16),
    ("instance", K::B16),
    ("cursor", K::U64),
];

const DIRMAP: Fields = &[
    ("tree", K::B16),
    ("dir", K::OsFileId),
    ("mtime", K::FsTime),
    ("flags", K::U8),
    ("path", K::Heap(S::Text)),
];

const PREFIXEV: Fields = &[
    ("tree", K::B16),
    ("root", K::U16),
    ("flags", K::U8),
    ("_reserved", K::Z(1)),
    ("rebound", K::U32),
    ("remaining", K::U32),
    ("first_hlc", K::U64),
    ("from", K::Heap(S::Text)),
    ("to", K::Heap(S::Text)),
];

const ANCESTRY: Fields = &[
    ("algo", K::U8),
    ("a", K::Slot32),
    ("b", K::Slot32),
    ("answer", K::U8),
    ("flags", K::U8),
];

const GITRENAMES: Fields = &[
    ("algo", K::U8),
    ("commit", K::Slot32),
    ("parent", K::Slot32),
    ("commit_time", K::I64),
    ("author_time", K::I64),
    ("flags", K::U8),
    ("n_renames", K::U32),
    ("n_groups", K::U32),
    ("list", K::Heap(S::Renames)),
];

const ANCHORRES: Fields = &[
    ("anchor", K::B16),
    ("oid", K::OidSlot),
    ("resolver_version", K::U16),
    ("state", K::U8),
    ("detail", K::U8),
    ("flags", K::U8),
    ("first_line", K::U32),
    ("last_line", K::U32),
    ("score_num", K::U64),
    ("score_den", K::U64),
];

const CURSORS: Fields = &[
    ("session", K::B16),
    ("agent", K::B16),
    ("feed", K::U8),
    ("_reserved", K::Z(3)),
    ("task", K::U32),
    ("cursor_seq", K::U64),
    ("hlc", K::U64),
];

const SESSMARKS: Fields = &[
    ("session", K::B16),
    ("agent", K::B16),
    ("rev", K::U64),
    ("hlc", K::U64),
    ("rules", K::Heap(S::ListU32)),
];

const SCHEMAIDS: Fields = &[
    ("space", K::U8),
    ("kind", K::U8),
    ("id", K::U16),
    ("name", K::U32),
    ("value", K::U32),
    ("flags", K::U8),
    ("_reserved", K::Z(3)),
];

const FILES: Fields = &[
    ("file", K::FileRef),
    ("dest", K::U8),
    ("algo", K::U8),
    ("flags", K::U8),
    ("_reserved", K::Z(4)),
    ("from_lsn", K::U64),
    ("upto_lsn", K::U64),
    ("total_len", K::U64),
    ("digest16", K::B16),
];

const BACKUPS: Fields = &[
    ("committed_lsn", K::U64),
    ("hlc", K::U64),
    ("digest", K::B32),
    ("dir", K::Heap(S::Text)),
];

/// Delta-segment form of a runtime table ([F11 §1.3] "Form").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeltaForm {
    /// Snapshot in every segment.
    Snapshot,
    /// Layer in a delta.
    Layer,
    /// Versioned ([F09 §4.7]).
    Versioned,
}

impl Table {
    /// The section tag ([F11 §2.8]).
    pub fn tag(self) -> u16 {
        match self {
            Table::Refs => 0x0201,
            Table::Pins => 0x0202,
            Table::Heads => 0x0203,
            Table::Leases => 0x0204,
            Table::Markers => 0x0205,
            Table::MarkersOld => 0x0206,
            Table::Idem => 0x0207,
            Table::Alloc => 0x0208,
            Table::Uidx => 0x0209,
            Table::Cursors => 0x020A,
            Table::Sessmarks => 0x020B,
            Table::Backups => 0x020C,
            Table::Trees => 0x0210,
            Table::FileObs => 0x0211,
            Table::Pending => 0x0212,
            Table::FsIntent => 0x0213,
            Table::FPrint => 0x0214,
            Table::JournalCur => 0x0215,
            Table::DirMap => 0x0216,
            Table::PrefixEv => 0x0217,
            Table::Ancestry => 0x0218,
            Table::GitRenames => 0x0219,
            Table::AnchorRes => 0x021A,
            Table::Conflicts => 0x0031,
            Table::GlobIdx => 0x0084,
            Table::SchemaIds => 0x0101,
            Table::Files => 0x0102,
        }
    }

    /// The table of a tag.
    pub fn from_tag(tag: u16) -> Option<Table> {
        TABLES.iter().copied().find(|t| t.tag() == tag)
    }

    /// The section name.
    pub fn name(self) -> &'static str {
        match self {
            Table::Refs => "REFS",
            Table::Pins => "PINS",
            Table::Heads => "HEADS",
            Table::Leases => "LEASES",
            Table::Markers => "MARKERS",
            Table::MarkersOld => "MARKERS_OLD",
            Table::Idem => "IDEM",
            Table::Alloc => "ALLOC",
            Table::Uidx => "UIDX",
            Table::Cursors => "CURSORS",
            Table::Sessmarks => "SESSMARKS",
            Table::Backups => "BACKUPS",
            Table::Trees => "TREES",
            Table::FileObs => "FILEOBS",
            Table::Pending => "PENDING",
            Table::FsIntent => "FSINTENT",
            Table::FPrint => "FPRINT",
            Table::JournalCur => "JOURNALCUR",
            Table::DirMap => "DIRMAP",
            Table::PrefixEv => "PREFIXEV",
            Table::Ancestry => "ANCESTRY",
            Table::GitRenames => "GITRENAMES",
            Table::AnchorRes => "ANCHORRES",
            Table::Conflicts => "CONFLICTS",
            Table::GlobIdx => "GLOBIDX",
            Table::SchemaIds => "SCHEMAIDS",
            Table::Files => "FILES",
        }
    }

    /// The row's field table.
    pub fn fields(self) -> Fields {
        match self {
            Table::Refs => REFS,
            Table::Pins => PINS,
            Table::Heads => HEADS,
            Table::Leases => LEASES,
            Table::Markers | Table::MarkersOld => MARKERS,
            Table::Idem => IDEM,
            Table::Alloc => ALLOC,
            Table::Uidx => UIDX,
            Table::Cursors => CURSORS,
            Table::Sessmarks => SESSMARKS,
            Table::Backups => BACKUPS,
            Table::Trees => TREES,
            Table::FileObs => FILEOBS,
            Table::Pending => PENDING,
            Table::FsIntent => FSINTENT,
            Table::FPrint => FPRINT,
            Table::JournalCur => JOURNALCUR,
            Table::DirMap => DIRMAP,
            Table::PrefixEv => PREFIXEV,
            Table::Ancestry => ANCESTRY,
            Table::GitRenames => GITRENAMES,
            Table::AnchorRes => ANCHORRES,
            Table::Conflicts => CONFLICTS,
            Table::GlobIdx => GLOBIDX,
            Table::SchemaIds => SCHEMAIDS,
            Table::Files => FILES,
        }
    }

    /// The row size ([F11 §1.3]).
    pub fn row_size(self) -> usize {
        self.fields().iter().map(|(_, k)| k.width()).sum()
    }

    /// The byte offset of field `name` in a row (the widths of the fields before it, [F11 §2.2] offset tables).
    pub fn field_offset(self, name: &str) -> usize {
        self.fields()
            .iter()
            .take_while(|(n, _)| *n != name)
            .map(|(_, k)| k.width())
            .sum()
    }

    /// The delta form ([F11 §1.3]).
    pub fn delta_form(self) -> DeltaForm {
        match self {
            Table::Refs
            | Table::Pins
            | Table::Heads
            | Table::Leases
            | Table::Markers
            | Table::Trees
            | Table::FsIntent
            | Table::JournalCur
            | Table::PrefixEv
            | Table::Backups
            | Table::Files => DeltaForm::Snapshot,
            Table::Conflicts | Table::GlobIdx => DeltaForm::Versioned,
            _ => DeltaForm::Layer,
        }
    }

    /// The `derived-optional` flag ([F11 §2.8]).
    pub fn derived_optional(self) -> bool {
        matches!(
            self,
            Table::FPrint | Table::DirMap | Table::Ancestry | Table::GitRenames | Table::AnchorRes
        )
    }

    /// Tables whose rows carry a `dead` bit 7 in `flags` in a layer ([F11 §2.4]).
    pub fn has_dead(self) -> bool {
        matches!(
            self,
            Table::FileObs
                | Table::Pending
                | Table::FPrint
                | Table::DirMap
                | Table::Ancestry
                | Table::GitRenames
                | Table::AnchorRes
        )
    }

    /// The fields forming the key ([F11 §2.2] and each section's sort key).
    fn key_fields(self) -> &'static [&'static str] {
        match self {
            Table::Refs => &["ref_id"],
            Table::Pins => &["file"],
            Table::Heads => &["kind", "key"],
            Table::Leases => &["n", "lease_id"],
            Table::Markers | Table::MarkersOld => &["n", "ref_id", "commit", "emit_lsn"],
            Table::Idem => &["key"],
            Table::Alloc => &[],
            Table::Uidx => &["uid"],
            Table::Cursors => &["session", "agent", "feed", "task"],
            Table::Sessmarks => &["session", "agent"],
            Table::Backups => &["dir"],
            Table::Trees => &["key"],
            Table::FileObs => &["n", "tree"],
            Table::Pending => &["n", "tree", "from", "to"],
            Table::FsIntent => &["intent_id"],
            Table::FPrint => &["oid"],
            Table::JournalCur => &["vol_key"],
            Table::DirMap => &["tree", "dir"],
            Table::PrefixEv => &["tree", "root", "from", "to"],
            Table::Ancestry => &["algo", "a", "b"],
            Table::GitRenames => &["algo", "commit"],
            Table::AnchorRes => &["anchor", "oid", "resolver_version"],
            Table::Conflicts => &["n", "key"],
            Table::GlobIdx => &["prefix", "n", "field", "glob"],
            Table::SchemaIds => &["space", "kind", "name", "value"],
            Table::Files => &["file"],
        }
    }
}

/// A typed field value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FV {
    /// An unsigned integer of any fixed width.
    U(u64),
    /// `i64`.
    I(i64),
    /// A byte string (`b16`, `b32`, a fixed slot, reserved bytes).
    Bytes(Vec<u8>),
    /// `OidSlot`.
    Oid(Oid),
    /// `FileRef` (family, ref_id, file_no).
    FileRef(u8, u32, u32),
    /// `Stamp`.
    Stamp(Stamp),
    /// `Anchor`.
    Anchor(Anchor),
    /// `ProcId`.
    ProcId(ProcId),
    /// `OsFileId`.
    OsFileId(OsFileId),
    /// `FsTime`.
    FsTime(FsTime),
    /// `VolumeCaps`.
    Caps(VolumeCaps),
    /// `BindingExt`.
    Binding(BindingExt),
    /// A heap slice.
    Slice(Slice),
}

/// A decoded heap slice.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Slice {
    /// `text`.
    Text(String),
    /// `path`.
    Path(PathVal),
    /// `list<u32>`.
    U32s(Vec<u32>),
    /// `list<u64>`.
    U64s(Vec<u64>),
    /// `list<AbsorbedEntry>`.
    Absorbed(Vec<(u32, u32)>),
    /// `list<PinHolder>` (kind, ref_id, set_lsn).
    PinHolders(Vec<(u8, u32, u64)>),
    /// `list<vstr>`.
    Vstrs(Vec<String>),
    /// `list<SensEntry>` (equiv, path).
    Sens(Vec<(u8, String)>),
    /// `list<Epoch>`.
    Epochs(Vec<Epoch>),
    /// The `FILEOBS` `var` sequence.
    FileObsVar(Box<FileObsVar>),
    /// Intent items.
    Items(Vec<IntentItem>),
    /// The `GITRENAMES` list.
    Renames(Box<Renames>),
    /// Opaque bytes.
    Raw(Vec<u8>),
    /// A `ckey`.
    CKey(CKey),
    /// A `kval`.
    KVal(Box<KVal>),
}

/// The `FILEOBS` `var` sequence ([F11 §12.5]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileObsVar {
    /// Enumerated spelling when it differs.
    pub path_seen: Option<String>,
    /// `moved-auto` target and its exact evidence token.
    pub target: Option<(PathVal, u8)>,
    /// Recorded detail parts.
    pub details: Vec<Detail>,
    /// Proposals in resolver order.
    pub proposals: Vec<Proposal>,
}

/// A detail slot value ([F11 §12.5] `Detail.args`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Arg {
    /// `<path>`.
    Path(PathVal),
    /// `<score>` as (num, den).
    Score(u64, u64),
    /// `<g7>` as an `oidv`.
    G7(Oid),
    /// `<n>`.
    N(u32),
    /// The optional `, runner-up <score>` of code 15.
    RunnerUp(Option<(u64, u64)>),
}

/// A recorded detail part.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Detail {
    /// A detail code of [F18 §4.6].
    pub code: u8,
    /// Its stored slots.
    pub args: Vec<Arg>,
}

/// A proposal ([F11 §12.5]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Proposal {
    /// Evidence class 1–4.
    pub class: u8,
    /// Proposal-class token 13–26.
    pub evidence: u8,
    /// Candidate path.
    pub path: PathVal,
    /// Score numerator.
    pub score_num: u64,
    /// Score denominator.
    pub score_den: u64,
}

/// An `FSINTENT` item ([F11 §12.7]); `outcome` is `None` in the [F05 §9.15] record form.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IntentItem {
    /// Bit 0 `dir`.
    pub itflags: u8,
    /// Outcome (row form only).
    pub outcome: Option<u8>,
    /// Source path.
    pub src: PathVal,
    /// Destination path (`op` 1).
    pub dst: Option<PathVal>,
    /// File `oid` (not for a directory).
    pub oid: Option<Oid>,
}

/// One ambiguous rename group ([F11 §12.12]): the blob `oid`, its source paths and its destination paths.
pub type RenameGroup = (Oid, Vec<Vec<u8>>, Vec<Vec<u8>>);

/// The `GITRENAMES` list ([F11 §12.12]) and the [F05 §9.25] renames fact share these parts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Renames {
    /// Exact renames (from, to).
    pub pairs: Vec<(Vec<u8>, Vec<u8>)>,
    /// Ambiguous groups (blob, froms, tos).
    pub groups: Vec<RenameGroup>,
}

/// A decoded row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    /// Its table.
    pub table: Table,
    /// One value per field of [`Table::fields`].
    pub vals: Vec<FV>,
}

impl Row {
    fn idx(&self, name: &str) -> usize {
        self.table
            .fields()
            .iter()
            .position(|(n, _)| *n == name)
            .unwrap_or_else(|| panic!("{} has no field {name}", self.table.name()))
    }

    /// A field's value.
    pub fn get(&self, name: &str) -> &FV {
        &self.vals[self.idx(name)]
    }

    /// An integer field.
    pub fn u(&self, name: &str) -> u64 {
        match self.get(name) {
            FV::U(v) => *v,
            other => panic!("{name} is not an integer: {other:?}"),
        }
    }

    /// A byte-string field.
    pub fn bytes(&self, name: &str) -> &[u8] {
        match self.get(name) {
            FV::Bytes(b) => b,
            other => panic!("{name} is not bytes: {other:?}"),
        }
    }

    /// A slice field.
    pub fn slice(&self, name: &str) -> &Slice {
        match self.get(name) {
            FV::Slice(s) => s,
            other => panic!("{name} is not a slice: {other:?}"),
        }
    }

    /// The symbol references of the row as (class code of [F05 §8.1], id), for SD-3 over the row images of log records
    /// ([F11 §2.9]): the fixed symbol fields (`HEADS.binding.expected_ref` `git-branch`, `LEASES.role` and `holder`,
    /// `MARKERS.actor`, `IDEM.branch_sym` `ref`, `GLOBIDX.field` and the `SCHEMAIDS` names `name`, `PREFIXEV.root`
    /// `root`) and every root in its slices (paths, the `FILEOBS` target, `<path>` details and proposals, intent items)
    /// and every symbol of a `CONFLICTS` key and side. Id 0 ("none") is not a reference.
    pub fn symbol_refs(&self, out: &mut Vec<(u8, u32)>) {
        use crate::log::sym;
        let fixed = |name: &str| self.u(name) as u32;
        match self.table {
            Table::Heads => {
                if let FV::Binding(b) = self.get("binding") {
                    out.push((sym::GIT_BRANCH, b.expected_ref));
                }
            }
            Table::Leases => {
                out.push((sym::ROLE, fixed("role")));
                out.push((sym::ACTOR, fixed("holder")));
            }
            Table::Markers | Table::MarkersOld => out.push((sym::ACTOR, fixed("actor"))),
            Table::Idem => out.push((sym::REF, fixed("branch_sym"))),
            Table::GlobIdx => out.push((sym::NAME, fixed("field"))),
            Table::SchemaIds => {
                out.push((sym::NAME, fixed("name")));
                out.push((sym::NAME, fixed("value")));
            }
            Table::PrefixEv => out.push((sym::ROOT, fixed("root"))),
            _ => {}
        }
        let root = |p: &PathVal| (sym::ROOT, u32::from(p.root));
        for v in &self.vals {
            let FV::Slice(s) = v else { continue };
            match s {
                Slice::Path(p) => out.push(root(p)),
                Slice::FileObsVar(fv) => {
                    if let Some((p, _)) = &fv.target {
                        out.push(root(p));
                    }
                    for d in &fv.details {
                        for a in &d.args {
                            if let Arg::Path(p) = a {
                                out.push(root(p));
                            }
                        }
                    }
                    out.extend(fv.proposals.iter().map(|p| root(&p.path)));
                }
                Slice::Items(items) => {
                    for it in items {
                        out.push(root(&it.src));
                        if let Some(d) = &it.dst {
                            out.push(root(d));
                        }
                    }
                }
                Slice::CKey(k) => k.symbol_refs(out),
                Slice::KVal(k) => k.symbol_refs(out),
                _ => {}
            }
        }
    }
}

/// Sort-key parts ([F01 §6.6]: integers numerically, byte strings bytewise, tuples field by field).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum KP {
    I(u64),
    B(Vec<u8>),
}

fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// [F20 §1.2] an exact rational in lowest terms: `den` ≥ 1, `num` 0 only as 0/1.
fn lowest_terms(num: u64, den: u64) -> bool {
    den >= 1
        && if num == 0 {
            den == 1
        } else {
            gcd(num, den) == 1
        }
}

/// The stored slots of a detail code's text template ([F18 §4.6], [F11 §12.5] `Detail.args`).
fn detail_slots(code: u8) -> Option<&'static [u8]> {
    // 1 path, 2 score, 3 g7, 4 n, 5 runner-up
    Some(match code {
        2 | 29 => &[1],
        13 | 14 | 16 | 18 => &[2],
        15 => &[2, 5],
        22 | 33 => &[4],
        25 => &[1, 1],
        34 => &[2, 2],
        35 => &[3, 3],
        43 | 49 => &[3],
        1
        | 3..=12
        | 17
        | 19..=21
        | 23
        | 24
        | 26..=28
        | 30..=32
        | 36..=42
        | 44..=48
        | 50..=61
        | 63..=70 => &[],
        _ => return None,
    })
}

/// [F11 §12.5]: the detail codes a row of `state` may record, as (first code's set, second code's set).
fn detail_allowed(state: u8, codes: &[u8]) -> bool {
    let one = |lo: u8, hi: u8| codes.len() == 1 && (lo..=hi).contains(&codes[0]);
    match state {
        1 => codes.len() == 1 && (codes[0] == 2 || codes[0] == 3),
        3 => one(10, 21),
        4 => one(22, 29),
        6 => codes == [34] || codes == [34, 35],
        8 => one(37, 45),
        12 => one(53, 61),
        _ => false,
    }
}

fn decode_detail(r: &mut Reader<'_>) -> Result<Detail> {
    let at = r.offset();
    let code = r.u8()?;
    let Some(slots) = detail_slots(code) else {
        return err(
            at,
            format!("detail code {code} is not in the registry [F18 §4.6]"),
        );
    };
    let mut args = Vec::with_capacity(slots.len());
    for s in slots {
        let a_at = r.offset();
        args.push(match s {
            1 => Arg::Path(PathVal::decode(r)?),
            2 => {
                let (n, d) = (r.u64()?, r.u64()?);
                if !lowest_terms(n, d) {
                    return err(a_at, "detail score not in lowest terms [F11 §12.5]");
                }
                Arg::Score(n, d)
            }
            3 => {
                let o = r.oidv()?;
                if o == Oid::None {
                    return err(a_at, "detail <g7> of algorithm none [F11 §12.5]");
                }
                Arg::G7(o)
            }
            4 => Arg::N(r.u32()?),
            _ => {
                let f = r.u8()?;
                match f {
                    0 => Arg::RunnerUp(None),
                    1 => {
                        let (n, d) = (r.u64()?, r.u64()?);
                        if !lowest_terms(n, d) {
                            return err(a_at, "runner-up score not in lowest terms [F11 §12.5]");
                        }
                        Arg::RunnerUp(Some((n, d)))
                    }
                    _ => return err(a_at, "runner-up flag is not 0 or 1 [F11 §12.5]"),
                }
            }
        });
    }
    Ok(Detail { code, args })
}

fn encode_detail(d: &Detail, w: &mut Writer) {
    w.u8(d.code);
    for a in &d.args {
        match a {
            Arg::Path(p) => p.encode(w),
            Arg::Score(n, dd) => {
                w.u64(*n);
                w.u64(*dd);
            }
            Arg::G7(o) => w.oidv(o),
            Arg::N(n) => w.u32(*n),
            Arg::RunnerUp(None) => w.u8(0),
            Arg::RunnerUp(Some((n, dd))) => {
                w.u8(1);
                w.u64(*n);
                w.u64(*dd);
            }
        }
    }
}

/// Decodes intent items; `with_outcome` is the [F11 §12.7] row form, else the [F05 §9.15] record form.
pub fn decode_items(
    r: &mut Reader<'_>,
    n: usize,
    op: u8,
    with_outcome: bool,
) -> Result<Vec<IntentItem>> {
    let mut v = Vec::with_capacity(n.min(r.remaining()));
    for _ in 0..n {
        let at = r.offset();
        let itflags = r.u8()?;
        if itflags & 0xFE != 0 {
            return err(at, "IntentItem itflags bits 1-7 are not zero [F11 §12.7]");
        }
        let outcome = if with_outcome {
            let o_at = r.offset();
            let o = r.u8()?;
            if o > 5 {
                return err(o_at, "IntentItem outcome outside 0-5 [F11 §12.7]");
            }
            Some(o)
        } else {
            None
        };
        let src = PathVal::decode(r)?;
        let dst = if op == 1 {
            Some(PathVal::decode(r)?)
        } else {
            None
        };
        let oid = if itflags & 1 == 0 {
            let o_at = r.offset();
            let o = r.oidv()?;
            if o == Oid::None {
                return err(o_at, "IntentItem oid of algorithm none [F11 §12.7]");
            }
            Some(o)
        } else {
            None
        };
        v.push(IntentItem {
            itflags,
            outcome,
            src,
            dst,
            oid,
        });
    }
    Ok(v)
}

/// Encodes intent items in the form they were decoded in.
pub fn encode_items(items: &[IntentItem], w: &mut Writer) {
    for it in items {
        w.u8(it.itflags);
        if let Some(o) = it.outcome {
            w.u8(o);
        }
        it.src.encode(w);
        if let Some(d) = &it.dst {
            d.encode(w);
        }
        if let Some(o) = &it.oid {
            w.oidv(o);
        }
    }
}

/// Decodes the renames parts ([F11 §12.12] `list`, [F05 §9.25] fields 6–9) with their counts.
pub fn decode_renames(
    r: &mut Reader<'_>,
    algo: crate::prim::Algo,
    n_pairs: usize,
    n_groups: Option<usize>,
) -> Result<Renames> {
    let mut pairs = Vec::with_capacity(n_pairs.min(r.remaining()));
    for _ in 0..n_pairs {
        pairs.push((r.vbytes()?.to_vec(), r.vbytes()?.to_vec()));
    }
    let n_groups = match n_groups {
        Some(n) => n,
        None => r.count(1)?,
    };
    let mut groups = Vec::with_capacity(n_groups.min(r.remaining()));
    for _ in 0..n_groups {
        let blob = r.digest(algo)?;
        let nf = r.count(1)?;
        let mut froms = Vec::with_capacity(nf);
        for _ in 0..nf {
            froms.push(r.vbytes()?.to_vec());
        }
        let nt = r.count(1)?;
        let mut tos = Vec::with_capacity(nt);
        for _ in 0..nt {
            tos.push(r.vbytes()?.to_vec());
        }
        groups.push((blob, froms, tos));
    }
    Ok(Renames { pairs, groups })
}

/// Encodes the renames parts; `with_group_count` writes the `n_groups` varint of the [F05 §9.25] record form.
pub fn encode_renames(rn: &Renames, w: &mut Writer, with_group_count: bool) {
    for (f, t) in &rn.pairs {
        w.vbytes(f);
        w.vbytes(t);
    }
    if with_group_count {
        w.uvar(rn.groups.len() as u64);
    }
    for (b, f, t) in &rn.groups {
        w.digest(b);
        w.uvar(f.len() as u64);
        for x in f {
            w.vbytes(x);
        }
        w.uvar(t.len() as u64);
        for x in t {
            w.vbytes(x);
        }
    }
}

/// Where a row is being decoded from, for the dead-row and form rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    /// A section of the given form (1 snapshot, 2 layer).
    Section(u8),
    /// The image of an upsert row in a log record ([F11 §2.9]).
    Upsert,
    /// The delete image of a log record.
    Delete,
}

impl Place {
    fn allows_dead(self, t: Table) -> bool {
        match self {
            Place::Section(2) => t.has_dead(),
            Place::Delete => true,
            _ => false,
        }
    }
}

fn read_fixed(r: &mut Reader<'_>, k: K, algo: u8) -> Result<(FV, Option<(u32, u32)>)> {
    Ok((
        match k {
            K::U8 => FV::U(u64::from(r.u8()?)),
            K::U16 => FV::U(u64::from(r.u16()?)),
            K::U32 => FV::U(u64::from(r.u32()?)),
            K::U64 => FV::U(r.u64()?),
            K::I64 => FV::I(r.i64()?),
            K::B16 => FV::Bytes(r.b16()?.to_vec()),
            K::B32 => FV::Bytes(r.b32()?.to_vec()),
            K::Slot32 => {
                let at = r.offset();
                let b = r.b32()?;
                Reader::with_base(&b, at).oid_slot(algo)?;
                FV::Bytes(b.to_vec())
            }
            K::Z(n) => {
                r.zeros(n, "a reserved row field")?;
                FV::Bytes(vec![0; n])
            }
            K::OidSlot => FV::Oid(decode_oid_slot(r)?),
            K::FileRef => {
                let at = r.offset();
                let family = r.u8()?;
                let ref_id = r.u32()?;
                let file_no = r.u32()?;
                if !(1..=9).contains(&family) || file_no == 0 || (family != 5 && ref_id != 0) {
                    return err(at, "FileRef family, ref_id or file_no invalid [F11 §2.5]");
                }
                FV::FileRef(family, ref_id, file_no)
            }
            K::Stamp => FV::Stamp(Stamp::decode(r)?),
            K::Anchor => FV::Anchor(Anchor::decode(r)?),
            K::ProcId => FV::ProcId(ProcId::decode(r)?),
            K::OsFileId => FV::OsFileId(OsFileId::decode(r)?),
            K::FsTime => FV::FsTime(FsTime::decode(r)?),
            K::Attrs => {
                let at = r.offset();
                let v = r.u32()?;
                if v >> 11 != 0 {
                    return err(at, "FileAttrs bits 11-31 are not zero [F11 §12.2]");
                }
                FV::U(u64::from(v))
            }
            K::Caps => FV::Caps(VolumeCaps::decode(r)?),
            K::Binding => FV::Binding(BindingExt::decode(r)?),
            K::Heap(_) => {
                let off = r.u32()?;
                let len = r.u32()?;
                return Ok((FV::U(0), Some((off, len))));
            }
        },
        None,
    ))
}

fn write_fixed(v: &FV, k: K, w: &mut Writer, heap: (u32, u32)) {
    match (k, v) {
        (K::U8, FV::U(x)) => w.u8(*x as u8),
        (K::U16, FV::U(x)) => w.u16(*x as u16),
        (K::U32 | K::Attrs, FV::U(x)) => w.u32(*x as u32),
        (K::U64, FV::U(x)) => w.u64(*x),
        (K::I64, FV::I(x)) => w.i64(*x),
        (K::B16 | K::B32 | K::Slot32 | K::Z(_), FV::Bytes(b)) => w.bytes(b),
        (K::OidSlot, FV::Oid(o)) => encode_oid_slot(o, w),
        (K::FileRef, FV::FileRef(f, r, n)) => {
            w.u8(*f);
            w.u32(*r);
            w.u32(*n);
        }
        (K::Stamp, FV::Stamp(s)) => s.encode(w),
        (K::Anchor, FV::Anchor(a)) => a.encode(w),
        (K::ProcId, FV::ProcId(p)) => p.encode(w),
        (K::OsFileId, FV::OsFileId(o)) => o.encode(w),
        (K::FsTime, FV::FsTime(t)) => t.encode(w),
        (K::Caps, FV::Caps(c)) => c.encode(w),
        (K::Binding, FV::Binding(b)) => b.encode(w),
        (K::Heap(_), FV::Slice(_)) => {
            w.u32(heap.0);
            w.u32(heap.1);
        }
        (k, v) => panic!("field kind {k:?} holds {v:?}"),
    }
}

fn row_u(fixed: &[(FV, Option<(u32, u32)>)], t: Table, name: &str) -> u64 {
    let i = t
        .fields()
        .iter()
        .position(|(n, _)| *n == name)
        .expect("field");
    match &fixed[i].0 {
        FV::U(v) => *v,
        _ => 0,
    }
}

fn decode_slice(
    b: &[u8],
    at: usize,
    s: S,
    t: Table,
    fixed: &[(FV, Option<(u32, u32)>)],
    conflict_class: Option<u8>,
) -> Result<Slice> {
    let mut r = Reader::with_base(b, at);
    let out = match s {
        S::Text => Slice::Text(utf8(r.bytes(b.len())?, at)?.to_owned()),
        S::Path => {
            let p = PathVal::decode(&mut r)?;
            Slice::Path(p)
        }
        S::ListU32 => {
            if !b.len().is_multiple_of(4) {
                return err(at, "list<u32> length is not a multiple of 4 [F11 §2.3]");
            }
            let mut v = Vec::with_capacity(b.len() / 4);
            while !r.is_empty() {
                v.push(r.u32()?);
            }
            Slice::U32s(v)
        }
        S::ListU64 => {
            if !b.len().is_multiple_of(8) {
                return err(at, "list<u64> length is not a multiple of 8 [F11 §2.3]");
            }
            let mut v = Vec::with_capacity(b.len() / 8);
            while !r.is_empty() {
                v.push(r.u64()?);
            }
            Slice::U64s(v)
        }
        S::Absorbed => {
            if !b.len().is_multiple_of(8) {
                return err(
                    at,
                    "list<AbsorbedEntry> length is not a multiple of 8 [F11 §3.4]",
                );
            }
            let mut v = Vec::with_capacity(b.len() / 8);
            while !r.is_empty() {
                v.push((r.u32()?, r.u32()?));
            }
            Slice::Absorbed(v)
        }
        S::PinHolders => {
            if !b.len().is_multiple_of(13) {
                return err(
                    at,
                    "list<PinHolder> length is not a multiple of 13 [F11 §4]",
                );
            }
            let mut v = Vec::with_capacity(b.len() / 13);
            while !r.is_empty() {
                v.push((r.u8()?, r.u32()?, r.u64()?));
            }
            Slice::PinHolders(v)
        }
        S::ListVstr => {
            let mut v = Vec::new();
            while !r.is_empty() {
                v.push(r.vstr()?.to_owned());
            }
            Slice::Vstrs(v)
        }
        S::Sens => {
            let mut v = Vec::new();
            while !r.is_empty() {
                let e_at = r.offset();
                let equiv = r.u8()?;
                if equiv & 0xFC != 0 {
                    return err(e_at, "SensEntry equiv bits 2-7 are not zero [F11 §12.4]");
                }
                v.push((equiv, r.vstr()?.to_owned()));
            }
            Slice::Sens(v)
        }
        S::Epochs => {
            if !b.len().is_multiple_of(32) {
                return err(at, "list<Epoch> length is not a multiple of 32 [F11 §12.4]");
            }
            let mut v = Vec::with_capacity(b.len() / 32);
            while !r.is_empty() {
                v.push(Epoch::decode(&mut r)?);
            }
            Slice::Epochs(v)
        }
        S::FileObsVar => {
            let flags = row_u(fixed, t, "flags");
            let state = row_u(fixed, t, "state");
            let np = row_u(fixed, t, "n_proposals") as usize;
            let path_seen = if flags & 1 != 0 {
                Some(r.vstr()?.to_owned())
            } else {
                None
            };
            let target = if state == 2 {
                let p = PathVal::decode(&mut r)?;
                let e_at = r.offset();
                let ev = r.u8()?;
                if !(1..=12).contains(&ev) {
                    return err(e_at, "FILEOBS target_ev outside 1-12 [F11 §12.5]");
                }
                Some((p, ev))
            } else {
                None
            };
            let mut details = Vec::new();
            if flags & 2 != 0 {
                let n_at = r.offset();
                let n = r.u8()?;
                if !(1..=2).contains(&n) {
                    return err(n_at, "FILEOBS n_details outside 1-2 [F11 §12.5]");
                }
                for _ in 0..n {
                    details.push(decode_detail(&mut r)?);
                }
                let codes: Vec<u8> = details.iter().map(|d| d.code).collect();
                if !detail_allowed(state as u8, &codes) {
                    return err(
                        n_at,
                        format!(
                            "FILEOBS details {codes:?} not recordable for state {state} [F11 §12.5]"
                        ),
                    );
                }
            }
            let mut proposals = Vec::with_capacity(np);
            for _ in 0..np {
                let p_at = r.offset();
                let class = r.u8()?;
                let evidence = r.u8()?;
                let path = PathVal::decode(&mut r)?;
                let (score_num, score_den) = (r.u64()?, r.u64()?);
                if !(1..=4).contains(&class)
                    || !(13..=26).contains(&evidence)
                    || !lowest_terms(score_num, score_den)
                {
                    return err(
                        p_at,
                        "FILEOBS proposal class, evidence or score invalid [F11 §12.5]",
                    );
                }
                proposals.push(Proposal {
                    class,
                    evidence,
                    path,
                    score_num,
                    score_den,
                });
            }
            Slice::FileObsVar(Box::new(FileObsVar {
                path_seen,
                target,
                details,
                proposals,
            }))
        }
        S::Items => {
            let n = row_u(fixed, t, "n_items") as usize;
            let op = row_u(fixed, t, "op") as u8;
            Slice::Items(decode_items(&mut r, n, op, true)?)
        }
        S::Renames => {
            let algo_b = row_u(fixed, t, "algo") as u8;
            let n_p = row_u(fixed, t, "n_renames") as usize;
            let n_g = row_u(fixed, t, "n_groups") as usize;
            if b.is_empty() && n_p == 0 && n_g == 0 {
                Slice::Renames(Box::new(Renames {
                    pairs: Vec::new(),
                    groups: Vec::new(),
                }))
            } else {
                let algo = crate::prim::Algo::from_byte(algo_b, at)?;
                Slice::Renames(Box::new(decode_renames(&mut r, algo, n_p, Some(n_g))?))
            }
        }
        S::Raw => Slice::Raw(r.bytes(b.len())?.to_vec()),
        S::CKey => Slice::CKey(CKey::decode(&mut r)?),
        S::KVal => {
            let class = conflict_class.expect("key decoded before the sides");
            Slice::KVal(Box::new(KVal::decode(&mut r, class)?))
        }
    };
    r.finish("a heap slice")?;
    Ok(out)
}

fn encode_slice(s: &Slice) -> Vec<u8> {
    let mut w = Writer::new();
    match s {
        Slice::Text(t) => w.bytes(t.as_bytes()),
        Slice::Path(p) => p.encode(&mut w),
        Slice::U32s(v) => v.iter().for_each(|x| w.u32(*x)),
        Slice::U64s(v) => v.iter().for_each(|x| w.u64(*x)),
        Slice::Absorbed(v) => v.iter().for_each(|(a, b)| {
            w.u32(*a);
            w.u32(*b);
        }),
        Slice::PinHolders(v) => v.iter().for_each(|(k, r, l)| {
            w.u8(*k);
            w.u32(*r);
            w.u64(*l);
        }),
        Slice::Vstrs(v) => v.iter().for_each(|x| w.vstr(x)),
        Slice::Sens(v) => v.iter().for_each(|(e, p)| {
            w.u8(*e);
            w.vstr(p);
        }),
        Slice::Epochs(v) => v.iter().for_each(|e| e.encode(&mut w)),
        Slice::FileObsVar(fv) => {
            if let Some(p) = &fv.path_seen {
                w.vstr(p);
            }
            if let Some((p, ev)) = &fv.target {
                p.encode(&mut w);
                w.u8(*ev);
            }
            if !fv.details.is_empty() {
                w.u8(fv.details.len() as u8);
                for d in &fv.details {
                    encode_detail(d, &mut w);
                }
            }
            for p in &fv.proposals {
                w.u8(p.class);
                w.u8(p.evidence);
                p.path.encode(&mut w);
                w.u64(p.score_num);
                w.u64(p.score_den);
            }
        }
        Slice::Items(v) => encode_items(v, &mut w),
        Slice::Renames(rn) => encode_renames(rn, &mut w, false),
        Slice::Raw(b) => w.bytes(b),
        Slice::CKey(k) => k.encode(&mut w),
        Slice::KVal(k) => k.encode(&mut w),
    }
    w.into_vec()
}

/// Decodes one row's fixed bytes at `r` and its slices through `slice_at(i, off, len)`, which returns the slice bytes
/// and their absolute offset, or an error.
fn decode_row_with<'h>(
    t: Table,
    r: &mut Reader<'_>,
    mut slice_at: impl FnMut(usize, u32, u32) -> Result<(&'h [u8], usize)>,
) -> Result<Row> {
    let fields = t.fields();
    let mut fixed = Vec::with_capacity(fields.len());
    let mut algo = 0u8;
    for (name, k) in fields {
        let v = read_fixed(r, *k, algo)?;
        if *name == "algo"
            && let FV::U(a) = v.0
        {
            algo = a as u8;
        }
        fixed.push(v);
    }
    let mut vals = Vec::with_capacity(fields.len());
    let mut conflict_class = None;
    for (i, (_, k)) in fields.iter().enumerate() {
        if let (K::Heap(s), Some((off, len))) = (k, fixed[i].1) {
            let (b, at) = slice_at(i, off, len)?;
            let sl = decode_slice(b, at, *s, t, &fixed, conflict_class)?;
            if let Slice::CKey(ck) = &sl {
                conflict_class = Some(ck.class());
            }
            vals.push(FV::Slice(sl));
        } else {
            vals.push(fixed[i].0.clone());
        }
    }
    Ok(Row { table: t, vals })
}

/// The heap refs of a row in field order: (field index, len).
fn heap_lens(row: &Row) -> Vec<(usize, Vec<u8>)> {
    row.table
        .fields()
        .iter()
        .enumerate()
        .filter_map(|(i, (_, k))| match (k, &row.vals[i]) {
            (K::Heap(_), FV::Slice(s)) => Some((i, encode_slice(s))),
            _ => None,
        })
        .collect()
}

/// Encodes a row's fixed bytes with heap offsets starting at `*heap_off`, advancing it, and appends slices to `heap`.
fn encode_row(row: &Row, w: &mut Writer, heap: &mut Vec<u8>, zero_refs: bool) {
    let slices = heap_lens(row);
    let mut si = slices.iter();
    for (i, (_, k)) in row.table.fields().iter().enumerate() {
        let href = if let K::Heap(_) = k {
            let (_, b) = si.next().expect("slice");
            let off = if zero_refs { 0 } else { heap.len() as u32 };
            heap.extend_from_slice(b);
            (off, b.len() as u32)
        } else {
            (0, 0)
        };
        write_fixed(&row.vals[i], *k, w, href);
    }
}

/// Decodes a row image ([F11 §2.9]): fixed bytes with every `HeapRef.off` = 0, then the slices in field order.
pub fn decode_image(t: Table, r: &mut Reader<'_>, place: Place) -> Result<Row> {
    let fixed_len = t.row_size();
    let base = r.offset();
    let all = r.rest();
    if all.len() < fixed_len {
        return r.fail(format!(
            "{} row image needs {fixed_len} fixed bytes",
            t.name()
        ));
    }
    let mut fr = Reader::with_base(&all[..fixed_len], base);
    let mut cursor = fixed_len;
    let row = decode_row_with(t, &mut fr, |_, off, len| {
        if off != 0 {
            return err(base, "a row image HeapRef has off != 0 [F11 §2.9]");
        }
        let len = len as usize;
        if cursor + len > all.len() {
            return err(
                base + cursor,
                "a row image slice runs past its record [F11 §2.9]",
            );
        }
        let s = &all[cursor..cursor + len];
        let at = base + cursor;
        cursor += len;
        Ok((s, at))
    })?;
    r.bytes(cursor)?;
    check_row(&row, place, base)?;
    Ok(row)
}

/// Encodes a row image.
pub fn encode_image(row: &Row, w: &mut Writer) {
    let mut heap = Vec::new();
    encode_row(row, w, &mut heap, true);
    w.bytes(&heap);
}

/// `RtHdr` ([F11 §2.1]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RtHdr {
    /// Rows (IDEM: slots).
    pub n_rows: u32,
    /// Row size.
    pub row_size: u16,
    /// 1 snapshot, 2 layer.
    pub form: u8,
    /// Section-specific.
    pub aux: u32,
    /// Index region length.
    pub index_len: u32,
    /// Heap length.
    pub heap_len: u64,
}

/// A decoded runtime or versioned section body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    /// Its table.
    pub table: Table,
    /// The header.
    pub hdr: RtHdr,
    /// The rows (IDEM: every slot, unused ones included).
    pub rows: Vec<Row>,
    /// The `REFS` name index.
    pub index: Vec<u32>,
}

/// The segment kinds a section may appear in ([F09 §2.1] `seg_kind`, [F11 §2.4]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SegKind {
    /// Base segment.
    Base,
    /// Delta segment.
    Delta,
    /// Promoted-branch segment.
    Branch,
    /// Changeset segment.
    Changeset,
}

/// Decodes a section body of `t` found in a segment of kind `seg`, with every rule of [F11 §2] and the table's own.
pub fn decode_section(t: Table, b: &[u8], base: usize, seg: SegKind) -> Result<Section> {
    let mut r = Reader::with_base(b, base);
    let n_rows = r.u32()?;
    let rs_at = r.offset();
    let row_size = r.u16()?;
    if usize::from(row_size) != t.row_size() {
        return err(
            rs_at,
            format!(
                "{} row_size {row_size} is not {} [F11 §2.1]",
                t.name(),
                t.row_size()
            ),
        );
    }
    let f_at = r.offset();
    let form = r.u8()?;
    let want_form = match (t.delta_form(), seg) {
        (DeltaForm::Versioned, SegKind::Base) => 1,
        (DeltaForm::Versioned, _) => 2,
        (_, SegKind::Base) => 1,
        (DeltaForm::Snapshot, SegKind::Delta) => 1,
        (DeltaForm::Layer, SegKind::Delta) => 2,
        _ => {
            return err(
                f_at,
                format!("{} in a branch or changeset segment [F11 §2.4]", t.name()),
            );
        }
    };
    if form != want_form {
        return err(
            f_at,
            format!(
                "{} form {form} where {want_form} is required [F11 §2.4]",
                t.name()
            ),
        );
    }
    r.zeros(1, "RtHdr._reserved")?;
    let aux_at = r.offset();
    let aux = r.u32()?;
    let index_len = r.u32()?;
    let heap_len = r.u64()?;
    let hdr = RtHdr {
        n_rows,
        row_size,
        form,
        aux,
        index_len,
        heap_len,
    };
    let want_index = if t == Table::Refs {
        4 * u64::from(n_rows)
    } else {
        0
    };
    if u64::from(index_len) != want_index {
        return err(
            aux_at + 4,
            format!("{} index_len is not {want_index} [F11 §2.1]", t.name()),
        );
    }
    let total = 24 + u64::from(n_rows) * u64::from(row_size) + u64::from(index_len) + heap_len;
    if total != b.len() as u64 {
        return err(
            base,
            format!(
                "{} section length {} is not 24 + rows + index + heap = {total} [F11 §2.1]",
                t.name(),
                b.len()
            ),
        );
    }
    if !matches!(t, Table::Refs | Table::Idem | Table::Alloc) && aux != 0 {
        return err(
            aux_at,
            format!("{} RtHdr.aux is not zero [F11 §2.1]", t.name()),
        );
    }
    let rows_len = n_rows as usize * usize::from(row_size);
    let rows_bytes = &b[24..24 + rows_len];
    let idx_bytes = &b[24 + rows_len..24 + rows_len + index_len as usize];
    let heap_start = 24 + rows_len + index_len as usize;
    let heap = &b[heap_start..];
    let mut rows = Vec::with_capacity(n_rows as usize);
    let mut expect_off: u64 = 0;
    let mut rr = Reader::with_base(rows_bytes, base + 24);
    let idem_flags_at = t.field_offset("flags");
    for i in 0..n_rows as usize {
        let row_at = rr.offset();
        // [F11 §8]: a slot whose result is not inline (an unused slot included) holds the zero `HeapRef` and takes no
        // place in the heap; every other slice, an empty inline result included, sits at the running heap offset
        // (§2.3: "also when len is 0"). `encode_section` writes the same.
        let zero_ref =
            t == Table::Idem && rows_bytes[i * usize::from(row_size) + idem_flags_at] & 4 == 0;
        let row = decode_row_with(t, &mut rr, |_, off, len| {
            let (off64, len64) = (u64::from(off), u64::from(len));
            if zero_ref {
                if (off, len) != (0, 0) {
                    return err(
                        row_at,
                        "an IDEM result HeapRef without result_inline is not zero [F11 §8]",
                    );
                }
            } else if off64 != expect_off {
                return err(
                    row_at,
                    format!(
                        "{} HeapRef off {off} breaks the canonical heap order (want {expect_off}) [F11 §2.3]",
                        t.name()
                    ),
                );
            }
            if off64 + len64 > heap_len {
                return err(
                    row_at,
                    format!("{} HeapRef out of the heap [F11 §2.3]", t.name()),
                );
            }
            if !zero_ref {
                expect_off = off64 + len64;
            }
            Ok((
                &heap[off as usize..(off64 + len64) as usize],
                base + heap_start + off as usize,
            ))
        })?;
        check_row(&row, Place::Section(form), row_at)?;
        rows.push(row);
    }
    if expect_off != heap_len {
        return err(
            base + heap_start,
            format!("{} heap has bytes no HeapRef covers [F11 §2.3]", t.name()),
        );
    }
    let mut index = Vec::with_capacity(n_rows as usize);
    let mut ir = Reader::with_base(idx_bytes, base + 24 + rows_len);
    while !ir.is_empty() {
        index.push(ir.u32()?);
    }
    let s = Section {
        table: t,
        hdr,
        rows,
        index,
    };
    check_section(&s, base, seg)?;
    Ok(s)
}

/// Re-encodes a section body.
pub fn encode_section(s: &Section) -> Vec<u8> {
    let mut rows = Writer::new();
    let mut heap = Vec::new();
    for row in &s.rows {
        // [F11 §8]: an IDEM row whose result is not inline holds the zero HeapRef and no heap bytes; an inline result,
        // empty or not, takes the running heap offset (§2.3), as `decode_section` requires.
        let zero_ref = s.table == Table::Idem && row.u("flags") & 4 == 0;
        if zero_ref {
            encode_row(row, &mut rows, &mut Vec::new(), true);
        } else {
            encode_row(row, &mut rows, &mut heap, false);
        }
    }
    let mut w = Writer::new();
    w.u32(s.hdr.n_rows);
    w.u16(s.hdr.row_size);
    w.u8(s.hdr.form);
    w.u8(0);
    w.u32(s.hdr.aux);
    w.u32(s.hdr.index_len);
    w.u64(heap.len() as u64);
    w.bytes(rows.as_slice());
    for i in &s.index {
        w.u32(*i);
    }
    w.bytes(&heap);
    w.into_vec()
}

fn is_zero_fv(v: &FV) -> bool {
    match v {
        FV::U(x) => *x == 0,
        FV::I(x) => *x == 0,
        FV::Bytes(b) => b.iter().all(|&x| x == 0),
        FV::Oid(o) => *o == Oid::None,
        FV::FileRef(..) => false,
        FV::Stamp(s) => *s == Stamp::default(),
        FV::Anchor(a) => *a == Anchor::default(),
        FV::ProcId(p) => *p == ProcId::default(),
        FV::OsFileId(o) => *o == OsFileId::default(),
        FV::FsTime(t) => *t == FsTime::default(),
        FV::Caps(c) => *c == VolumeCaps::default(),
        FV::Binding(b) => b.is_zero(),
        FV::Slice(s) => encode_slice(s).is_empty(),
    }
}

fn key_parts(row: &Row) -> Vec<KP> {
    let t = row.table;
    let mut v = Vec::new();
    for name in t.key_fields() {
        if *name == "prefix" {
            let Slice::Text(g) = row.slice("glob") else {
                unreachable!()
            };
            let p = row.u("prefix_len") as usize;
            v.push(KP::B(g.as_bytes()[..p.min(g.len())].to_vec()));
            continue;
        }
        match row.get(name) {
            FV::U(x) => v.push(KP::I(*x)),
            FV::Bytes(b) => v.push(KP::B(b.clone())),
            FV::Oid(o) => {
                v.push(KP::I(u64::from(o.algo_byte())));
                v.push(KP::B(o.digest().to_vec()));
            }
            FV::FileRef(f, r, n) => {
                v.push(KP::I(u64::from(*f)));
                v.push(KP::I(u64::from(*r)));
                v.push(KP::I(u64::from(*n)));
            }
            FV::OsFileId(o) => {
                v.push(KP::I(u64::from(o.kind)));
                v.push(KP::B(o.vol_key.to_vec()));
                v.push(KP::B(o.id.to_vec()));
            }
            FV::Slice(s) => v.push(KP::B(encode_slice(s))),
            other => panic!("key field {name} of kind {other:?}"),
        }
    }
    v
}

/// The literal prefix length of a glob ([F11 §11], [F08 §5.4.3]).
pub fn glob_prefix_len(g: &str) -> usize {
    let b = g.as_bytes();
    let wild = b.iter().position(|c| matches!(c, b'*' | b'?' | b'['));
    let upto = wild.unwrap_or(b.len());
    b[..upto]
        .iter()
        .rposition(|&c| c == b'/')
        .map_or(0, |i| i + 1)
}

/// The row rules of one table ([F11 §3]–§13) and the dead-row rule (§2.4).
fn check_row(row: &Row, place: Place, at: usize) -> Result<()> {
    let t = row.table;
    let fail = |m: String| err(at, format!("{}: {m}", t.name()));
    if t.has_dead() || t == Table::PrefixEv {
        let flags = row.u("flags");
        if flags & 0x80 != 0 {
            let dead_ok = if t == Table::PrefixEv {
                place == Place::Delete
            } else {
                place.allows_dead(t)
            };
            if !dead_ok {
                return fail("a dead row outside a layer or delete image [F11 §2.4]".into());
            }
            if flags != 0x80 {
                return fail("a dead row has flag bits other than dead [F11 §2.4]".into());
            }
            let keys = t.key_fields();
            for (i, (name, _)) in t.fields().iter().enumerate() {
                if keys.contains(name) || *name == "flags" {
                    continue;
                }
                if !is_zero_fv(&row.vals[i]) {
                    return fail(format!("a dead row has non-zero {name} [F11 §2.4]"));
                }
            }
            return Ok(());
        }
        if place == Place::Delete {
            return fail("a delete image without the dead bit [F11 §2.9]".into());
        }
    }
    let u = |n: &str| row.u(n);
    match t {
        Table::Refs => {
            let kind = u("kind");
            if !(1..=6).contains(&kind) || u("flags") & 0xFC != 0 {
                return fail("kind outside 1-6 or reserved flag bits [F11 §3.1]".into());
            }
            let tip_zero = row.bytes("tip").iter().all(|&x| x == 0);
            if tip_zero != (u("tip_lsn") == 0) || (tip_zero && u("gen") != 0) {
                return fail("tip, tip_lsn and gen disagree [F11 §3.1]".into());
            }
            if u("ref_seq_next") == 0 {
                return fail("ref_seq_next is 0 [F11 §3.1]".into());
            }
            let fork_zero = row.bytes("fork_commit").iter().all(|&x| x == 0);
            if fork_zero && (u("fork_lsn") != 0 || u("fork_seq") != 0 || u("fork_ref_id") != 0) {
                return fail("fork fields set without a fork commit [F11 §3.1]".into());
            }
            if let Slice::Absorbed(a) = row.slice("absorbed")
                && a.windows(2).any(|w| w[0].0 >= w[1].0)
            {
                return fail("absorbed vector not sorted and unique [F11 §3.4]".into());
            }
            if let Slice::U64s(m) = row.slice("moves")
                && m.len() > 32
            {
                return fail("more than 32 moves [F11 §3.5]".into());
            }
            if let (Slice::Text(msg), true) = (row.slice("message"), kind != 5)
                && !msg.is_empty()
            {
                return fail("a message on a ref that is not a tag [F11 §3.1]".into());
            }
        }
        Table::Pins => {
            let Slice::PinHolders(h) = row.slice("holders") else {
                unreachable!()
            };
            if h.is_empty() || u("refcount") != h.len() as u64 {
                return fail("refcount differs from holders.len / 13, or is 0 [F11 §4]".into());
            }
            if h.iter().any(|x| !(1..=4).contains(&x.0)) || h.windows(2).any(|w| w[0] >= w[1]) {
                return fail("holders kinds invalid or not sorted and unique [F11 §4]".into());
            }
        }
        Table::Heads => {
            let kind = u("kind");
            if !(1..=3).contains(&kind) || u("flags") & 0xFE != 0 {
                return fail("kind outside 1-3 or reserved flag bits [F11 §5]".into());
            }
            let FV::OsFileId(root) = row.get("root_id") else {
                unreachable!()
            };
            if (root.kind == 0) != (u("os") == 0) {
                return fail("os is 0 exactly when root_id.kind is 0 [F11 §5]".into());
            }
            if kind != 1 && root.kind != 0 {
                return fail("a root_id on a row that is not a directory row [F11 §5]".into());
            }
            let detached = u("flags") & 1 != 0;
            let d_zero = row.bytes("detached").iter().all(|&x| x == 0) && u("detached_lsn") == 0;
            if (detached && u("ref_id") != 0) || (!detached && !d_zero) {
                return fail("detached fields disagree with the flag [F11 §5]".into());
            }
            let FV::Binding(bx) = row.get("binding") else {
                unreachable!()
            };
            if !bx.is_zero() && (kind != 1 || detached) {
                return fail(
                    "a binding on a row that is not an attached directory [F11 §5.2]".into(),
                );
            }
            let Slice::Text(text) = row.slice("text") else {
                unreachable!()
            };
            if blake3_128(text.as_bytes()).as_slice() != row.bytes("key") {
                return fail("key is not blake3_16(text) [F11 §5.1]".into());
            }
        }
        Table::Leases => {
            let (n, kind, flags) = (u("n"), u("kind"), u("flags"));
            if !(1..=2).contains(&kind) || (kind == 1) != (n != 0) || flags & 0xFC != 0 {
                return fail("kind, n or flags invalid [F11 §6.1]".into());
            }
            if flags & 2 != 0 && kind != 2 {
                return fail("session_role on a task lease [F11 §6.1]".into());
            }
            let FV::Stamp(exp) = row.get("expires") else {
                unreachable!()
            };
            let run_scoped = flags & 1 != 0;
            if run_scoped != (*exp == Stamp::NEVER) {
                return fail("run_scoped differs from expires = NEVER [F11 §6.1]".into());
            }
            if run_scoped && (u("run") == 0 || u("ttl_ms") != 0) {
                return fail("a run-scoped lease without run or with a ttl [F11 §6.1]".into());
            }
            if !run_scoped && u("ttl_ms") == 0 {
                return fail("a lease that is not run-scoped has ttl 0 [F11 §6.1]".into());
            }
            let FV::Anchor(a) = row.get("anchor") else {
                unreachable!()
            };
            if !matches!(a.kind, 0 | 1 | 4) {
                return fail("anchor kind not 0, 1 or 4 [F11 §6]".into());
            }
            let Slice::Vstrs(g) = row.slice("files_owned") else {
                unreachable!()
            };
            if g.windows(2).any(|w| w[0].as_bytes() >= w[1].as_bytes())
                || (kind == 2 && !g.is_empty())
            {
                return fail(
                    "files_owned unsorted, duplicated, or on a role lease [F11 §6]".into(),
                );
            }
        }
        Table::Markers | Table::MarkersOld => {
            let kind = u("kind");
            let settled = kind == 1;
            if !(1..=3).contains(&kind)
                || (settled && !(1..=2).contains(&u("status")))
                || (!settled && u("status") != 0)
                || !(1..=5).contains(&u("cause"))
                || u("flags") & 0xFE != 0
                || u("outcome") > 3
                || (!settled && (u("actor") != 0 || u("outcome") != 0))
            {
                return fail(
                    "kind, status, cause, flags, actor or outcome invalid [F11 §7]".into(),
                );
            }
            let Slice::U32s(h) = row.slice("holders") else {
                unreachable!()
            };
            if h.windows(2).any(|w| w[0] >= w[1])
                || ((kind == 3 || t == Table::MarkersOld) && !h.is_empty())
            {
                return fail(
                    "holders unsorted, or non-empty in a cleared or inert row [F11 §7]".into(),
                );
            }
        }
        Table::Idem => {
            let flags = u("flags");
            if flags & 0xF8 != 0 {
                return fail("reserved flag bits [F11 §8]".into());
            }
            let Slice::Raw(res) = row.slice("result") else {
                unreachable!()
            };
            if flags & 1 == 0 {
                let zero = row.vals.iter().all(is_zero_fv);
                if !zero {
                    return fail("an unused slot is not all zero [F11 §8]".into());
                }
            } else if flags & 4 == 0 && !res.is_empty() {
                return fail("result bytes without result_inline [F11 §8]".into());
            }
        }
        Table::Alloc => {
            if u("create_seq") == 0 && !row.vals.iter().all(is_zero_fv) {
                return fail("a hole (create_seq 0) is not all zero [F11 §9.1]".into());
            }
        }
        Table::Uidx => {
            if u("n") == 0 {
                return fail("#N 0 [F11 §9.2]".into());
            }
        }
        Table::Conflicts => {
            let Slice::CKey(k) = row.slice("key") else {
                unreachable!()
            };
            if u("n") != u64::from(k.owner().unwrap_or(0)) {
                return fail("n differs from the key's owner [F11 §10]".into());
            }
            if !(1..=7).contains(&u("class")) {
                return fail("class outside 1-7 [F12 §6.1]".into());
            }
            if u("prov") > u64::from(k.class() == 1) {
                return fail("prov invalid for the key class [F11 §10]".into());
            }
        }
        Table::GlobIdx => {
            let Slice::Text(g) = row.slice("glob") else {
                unreachable!()
            };
            if g.is_empty() || u("prefix_len") != glob_prefix_len(g) as u64 {
                return fail("prefix_len differs from the glob's literal prefix [F11 §11]".into());
            }
        }
        Table::Trees => {
            let flags = u("flags");
            if flags & 0xF0 != 0 {
                return fail("reserved flag bits [F11 §12.4]".into());
            }
            if flags & 4 == 0
                && (u("dirty_count") != 0
                    || u("dirty_hlc") != 0
                    || *row.get("dirty_head") != FV::Oid(Oid::None))
            {
                return fail("dirty fields set without dirty_present [F11 §12.4]".into());
            }
            let Slice::Sens(sens) = row.slice("sens") else {
                unreachable!()
            };
            if sens
                .windows(2)
                .any(|w| w[0].1.as_bytes() >= w[1].1.as_bytes())
            {
                return fail("sensitivity map not sorted by path and unique [F11 §12.4]".into());
            }
            let Slice::Epochs(ep) = row.slice("epochs") else {
                unreachable!()
            };
            if ep
                .windows(2)
                .any(|w| (w[0].scope_kind, w[0].scope_ref) >= (w[1].scope_kind, w[1].scope_ref))
            {
                return fail(
                    "epochs not sorted by (scope_kind, scope_ref) and unique [F11 §12.4]".into(),
                );
            }
        }
        Table::FileObs => {
            let state = u("state");
            if ![1, 2, 3, 4, 6, 8, 12].contains(&state)
                || u("flags") & 0x7C != 0
                || u("n_proposals") > 3
            {
                return fail("state, flags or n_proposals invalid [F11 §12.5]".into());
            }
            let Slice::FileObsVar(v) = row.slice("var") else {
                unreachable!()
            };
            if (u("flags") & 2 != 0) != !v.details.is_empty() {
                return fail("has_detail differs from the recorded parts [F11 §12.5]".into());
            }
        }
        Table::Pending => {
            let (class, source, ev) = (u("class"), u("source"), u("evidence"));
            let ev_ok = match source {
                1 => [1, 3, 10, 22].contains(&ev),
                2 => (3..=9).contains(&ev) || (13..=26).contains(&ev),
                _ => false,
            };
            if !(1..=4).contains(&class) || !ev_ok || u("flags") & 0x7F != 0 {
                return fail("class, source, evidence or flags invalid [F11 §12.6]".into());
            }
        }
        Table::FsIntent => {
            let (op, state, flags, reason) = (u("op"), u("state"), u("flags"), u("reason"));
            if !(1..=3).contains(&op) || !(1..=3).contains(&state) || flags & 0xF8 != 0 {
                return fail("op, state or flags invalid [F11 §12.7]".into());
            }
            if (state == 3) != (reason != 0) || reason > 5 {
                return fail("reason set exactly for an aborted intent, 1-5 [F11 §12.7]".into());
            }
            if (state == 1) != (u("closed_lsn") == 0 && u("closed_hlc") == 0) || u("n_items") == 0 {
                return fail(
                    "closed fields disagree with the state, or no items [F11 §12.7]".into(),
                );
            }
            let FV::Anchor(a) = row.get("anchor") else {
                unreachable!()
            };
            if a.kind != 2 {
                return fail("the intent anchor is not kind 2 [F11 §12.7]".into());
            }
            let Slice::Items(items) = row.slice("items") else {
                unreachable!()
            };
            for it in items {
                let o = it.outcome.unwrap_or(0);
                if (state == 2) == (o == 0) {
                    return fail("item outcome 0 exactly while open or aborted [F11 §12.7]".into());
                }
            }
        }
        Table::FPrint => {
            if u("flags") & 0x7F != 0 || u("file") == 0 {
                return fail("flags invalid or file 0 in a live row [F11 §12.8]".into());
            }
        }
        Table::JournalCur => {
            if u("kind") > 2 {
                return fail("kind outside 0-2 [F11 §12.9]".into());
            }
        }
        Table::DirMap => {
            if u("flags") & 0x7F != 0 {
                return fail("reserved flag bits [F11 §12.10]".into());
            }
        }
        Table::PrefixEv => {
            if u("flags") & 0x7F != 0 {
                return fail("reserved flag bits [F11 §12.11]".into());
            }
            let (Slice::Text(f), Slice::Text(to)) = (row.slice("from"), row.slice("to")) else {
                unreachable!()
            };
            if !f.ends_with('/') || !to.ends_with('/') {
                return fail("from or to does not end in / [F11 §12.11]".into());
            }
        }
        Table::Ancestry => {
            if !(1..=2).contains(&u("algo")) || u("answer") > 1 || u("flags") & 0x7F != 0 {
                return fail("algo, answer or flags invalid [F11 §12.12]".into());
            }
        }
        Table::GitRenames => {
            let flags = u("flags");
            if !(1..=2).contains(&u("algo")) || flags & 0x7C != 0 || flags & 3 == 0 {
                return fail("algo or flags invalid [F11 §12.12]".into());
            }
            let renames = flags & 1 != 0;
            if !renames
                && (row.bytes("parent").iter().any(|&x| x != 0)
                    || u("n_renames") != 0
                    || u("n_groups") != 0)
            {
                return fail("rename fields set without renames [F11 §12.12]".into());
            }
            if flags & 2 == 0
                && (!matches!(row.get("commit_time"), FV::I(0))
                    || !matches!(row.get("author_time"), FV::I(0)))
            {
                return fail("times set without times [F11 §12.12]".into());
            }
        }
        Table::AnchorRes => {
            let (state, detail) = (u("state"), u("detail"));
            if !(1..=5).contains(&state) || u("flags") & 0x7F != 0 {
                return fail("state or flags invalid [F11 §12.13]".into());
            }
            if (state == 3) != (detail == 62 || detail == 63) || (state != 3 && detail != 0) {
                return fail("detail is 62 or 63 exactly for edited [F11 §12.13]".into());
            }
            let (f, l) = (u("first_line"), u("last_line"));
            let span_ok = if state >= 4 {
                f == 0 && l == 0
            } else {
                f >= 1 && l >= f
            };
            if !span_ok || !lowest_terms(u("score_num"), u("score_den")) {
                return fail("span lines or score invalid [F11 §12.13]".into());
            }
        }
        Table::Cursors => {
            let feed = u("feed");
            if !(1..=2).contains(&feed) || (feed == 1) != (u("task") == 0) {
                return fail("feed outside 1-2, or task disagrees with it [F11 §13.1]".into());
            }
        }
        Table::Sessmarks | Table::Backups => {}
        Table::SchemaIds => {
            let (space, kind, id) = (u("space"), u("kind"), u("id"));
            let id_ok = match space {
                1 | 2 => (64..=254).contains(&id),
                3 => true,
                _ => false,
            };
            if !id_ok || (space != 3 && (kind != 0 || u("value") != 0)) || u("flags") != 0 {
                return fail("space, kind, id, value or flags invalid [F09 §14.3]".into());
            }
        }
        Table::Files => {
            let FV::FileRef(family, _, _) = *row.get("file") else {
                unreachable!()
            };
            let flags = u("flags");
            if ![2, 5, 6, 7, 8, 9].contains(&family) || flags & 0xFE != 0 {
                return fail("family not hist, seg-branch, blobs, dict, gitmap or cs, or reserved flags [F09 §14.4]".into());
            }
            let gitmap_ok = if family == 8 {
                u("dest") >= 1 && (1..=2).contains(&u("algo"))
            } else {
                u("dest") == 0 && u("algo") == 0
            };
            let reserved_ok = flags == 0
                || ((family == 6 || family == 9)
                    && u("upto_lsn") == 0
                    && u("total_len") == 0
                    && row.bytes("digest16").iter().all(|&x| x == 0));
            let lsn_ok = (family == 2 || u("from_lsn") == 0)
                && ([2, 5, 9].contains(&family) || u("upto_lsn") == 0);
            if !gitmap_ok || !reserved_ok || !lsn_ok {
                return fail(
                    "dest, algo, reserved flag or lsn fields break the family rules [F09 §14.4]"
                        .into(),
                );
            }
        }
    }
    Ok(())
}

/// Section-wide rules: key order and uniqueness, the `REFS` index and `aux`, the `IDEM` layout, `ALLOC` density, the
/// `HEADS` binding uniqueness.
fn check_section(s: &Section, at: usize, seg: SegKind) -> Result<()> {
    let t = s.table;
    let fail = |m: String| err(at, format!("{}: {m}", t.name()));
    match t {
        Table::Idem => {
            let n_used = s.rows.iter().filter(|r| r.u("flags") & 1 != 0).count();
            if s.hdr.aux as usize != n_used {
                return fail("aux differs from the used slot count [F11 §8]".into());
            }
            let cap = (n_used * 2).max(16).next_power_of_two();
            if s.rows.len() != cap {
                return fail(format!("capacity {} is not {cap} [F11 §8]", s.rows.len()));
            }
            let mut keys: Vec<&[u8]> = s
                .rows
                .iter()
                .filter(|r| r.u("flags") & 1 != 0)
                .map(|r| r.bytes("key"))
                .collect();
            keys.sort();
            if keys.windows(2).any(|w| w[0] == w[1]) {
                return fail("two entries with one key [F11 §8]".into());
            }
            let mut slots: Vec<Option<&[u8]>> = vec![None; cap];
            for k in keys {
                let mut i = (crate::prim::le_u64(k) as usize) & (cap - 1);
                while slots[i].is_some() {
                    i = (i + 1) & (cap - 1);
                }
                slots[i] = Some(k);
            }
            for (i, row) in s.rows.iter().enumerate() {
                let have = (row.u("flags") & 1 != 0).then(|| row.bytes("key"));
                if have != slots[i] {
                    return fail(format!(
                        "slot {i} breaks the canonical linear-probing layout [F11 §8]"
                    ));
                }
            }
            return Ok(());
        }
        Table::Alloc => {
            // [F11 §9.1]: a base's range starts at #1; a delta's at its first row's #N, never 0. A delta layer that
            // allocated no id has no first #N, and its `aux` is the zero of §2.1 (a reading; see the WP-95 findings).
            let first = s.hdr.aux;
            let ok = match (seg, s.rows.is_empty()) {
                (SegKind::Base, _) => first == 1,
                (_, true) => first == 0,
                (_, false) => first != 0,
            };
            if !ok {
                return fail(
                    "aux (the first #N) is not 1 in a base, 0 in an empty delta layer, or a non-zero #N [F11 §9.1]"
                        .into(),
                );
            }
            return Ok(());
        }
        _ => {}
    }
    let keys: Vec<Vec<KP>> = s.rows.iter().map(key_parts).collect();
    if keys.windows(2).any(|w| w[0].cmp(&w[1]) != Ordering::Less) {
        return fail("rows not sorted by their key, or two rows with one key [F11 §2.2]".into());
    }
    if t == Table::Refs {
        let max_id = s.rows.iter().map(|r| r.u("ref_id") + 1).max().unwrap_or(0);
        if u64::from(s.hdr.aux) < max_id {
            return fail("aux (next_ref_id) is not above every ref_id [F11 §3.7]".into());
        }
        let name = |i: usize| match s.rows[i].slice("name") {
            Slice::Text(n) => n.as_bytes().to_vec(),
            _ => unreachable!(),
        };
        let mut want: Vec<u32> = (0..s.rows.len() as u32).collect();
        want.sort_by(|&a, &b| {
            (name(a as usize), s.rows[a as usize].u("ref_id"))
                .cmp(&(name(b as usize), s.rows[b as usize].u("ref_id")))
        });
        if s.index != want {
            return fail(
                "the name index is not the permutation by (name, ref_id) [F11 §3.3]".into(),
            );
        }
        let mut live: Vec<Vec<u8>> = (0..s.rows.len())
            .filter(|&i| s.rows[i].u("flags") & 1 == 0)
            .map(name)
            .collect();
        live.sort();
        if live.windows(2).any(|w| w[0] == w[1]) {
            return fail("two rows without deleted share a name [F11 §3.1]".into());
        }
    }
    if t == Table::Heads {
        let mut refs: Vec<u64> = s
            .rows
            .iter()
            .filter(
                |r| matches!(r.get("binding"), FV::Binding(b) if b.valid() && b.bflags & 1 != 0),
            )
            .map(|r| r.u("ref_id"))
            .collect();
        refs.sort_unstable();
        if refs.windows(2).any(|w| w[0] == w[1]) {
            return fail("two designated rows name one ref_id [F11 §5.2, I-F12]".into());
        }
    }
    if t == Table::SchemaIds {
        // [F09 §14.3]: `id` unique per space, and per (kind, field) for space 3.
        let mut ids: Vec<(u64, u64, u64, u64)> = s
            .rows
            .iter()
            .map(|r| {
                let space = r.u("space");
                let (k, f) = if space == 3 {
                    (r.u("kind"), r.u("name"))
                } else {
                    (0, 0)
                };
                (space, k, f, r.u("id"))
            })
            .collect();
        ids.sort_unstable();
        if ids.windows(2).any(|w| w[0] == w[1]) {
            return fail("two rows of one space share an id [F09 §14.3]".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prim::unhex;

    /// [F11 §1.3] row sizes follow from the offset tables.
    #[test]
    fn row_sizes() {
        let want = [
            (Table::Refs, 164),
            (Table::Pins, 21),
            (Table::Heads, 161),
            (Table::Leases, 180),
            (Table::Markers, 72),
            (Table::Idem, 72),
            (Table::Alloc, 24),
            (Table::Uidx, 20),
            (Table::Conflicts, 56),
            (Table::GlobIdx, 20),
            (Table::Trees, 201),
            (Table::FileObs, 187),
            (Table::Pending, 123),
            (Table::FsIntent, 132),
            (Table::FPrint, 54),
            (Table::JournalCur, 41),
            (Table::DirMap, 91),
            (Table::PrefixEv, 52),
            (Table::Ancestry, 67),
            (Table::GitRenames, 98),
            (Table::AnchorRes, 78),
            (Table::Cursors, 56),
            (Table::Sessmarks, 56),
            (Table::Backups, 56),
        ];
        for (t, n) in want {
            assert_eq!(t.row_size(), n, "{}", t.name());
        }
        for t in TABLES {
            assert_eq!(Table::from_tag(t.tag()), Some(t));
        }
    }

    /// [F11 §14]: the worked `MARKERS` snapshot section, 100 bytes.
    pub(super) fn markers_example() -> Vec<u8> {
        let hex = "01000000 4800 01 00 00000000 00000000 0400000000000000 \
                   28000000 03000000 00112233445566778899aabbccddeeff 01010100 11000000 00000000 00000000 \
                   0300006c50c4a001 7611000000000000 4034120000000000 00000000 04000000 \
                   03000000";
        unhex(&hex.replace([' ', '\n'], "")).unwrap()
    }

    /// [F11 §14]: the worked section decodes to the row the example names and re-encodes byte-identically.
    #[test]
    fn markers_worked_example() {
        let b = markers_example();
        assert_eq!(b.len(), 100);
        let s = decode_section(Table::Markers, &b, 0, SegKind::Base).unwrap();
        assert_eq!(s.rows.len(), 1);
        let r = &s.rows[0];
        assert_eq!(r.u("n"), 40);
        assert_eq!(r.u("seq"), 4470);
        assert_eq!(r.u("emit_lsn"), 1_193_024);
        assert_eq!(r.slice("holders"), &Slice::U32s(vec![3]));
        assert_eq!(encode_section(&s), b);
        let mut bad = b.clone();
        bad[88] = 1; // the HeapRef off
        assert!(decode_section(Table::Markers, &bad, 0, SegKind::Base).is_err());
        assert!(decode_section(Table::Markers, &b, 0, SegKind::Branch).is_err());
    }

    fn fileobs_row() -> Row {
        Row {
            table: Table::FileObs,
            vals: vec![
                FV::U(7),
                FV::Bytes(vec![1; 16]),
                FV::U(3),
                FV::U(2),
                FV::U(1),
                FV::U(1),
                FV::OsFileId(OsFileId {
                    kind: 1,
                    vol_key: [2; 16],
                    id: [3; 16],
                    parent: [4; 16],
                    aux: 0,
                    docid: 0,
                }),
                FV::U(100),
                FV::FsTime(FsTime { ns: 5, gran: 2 }),
                FV::FsTime(FsTime { ns: 6, gran: 2 }),
                FV::FsTime(FsTime { ns: 7, gran: 2 }),
                FV::FsTime(FsTime { ns: 0, gran: 0xFF }),
                FV::U(1),
                FV::Oid(Oid::Sha1([9; 20])),
                FV::U(10),
                FV::U(0),
                FV::Slice(Slice::FileObsVar(Box::new(FileObsVar {
                    path_seen: None,
                    target: None,
                    details: vec![Detail {
                        code: 15,
                        args: vec![Arg::Score(81, 100), Arg::RunnerUp(Some((11, 50)))],
                    }],
                    proposals: vec![Proposal {
                        class: 2,
                        evidence: 14,
                        path: PathVal {
                            root: 1,
                            text: "docs/plan/storage-v2.md".into(),
                        },
                        score_num: 81,
                        score_den: 100,
                    }],
                }))),
            ],
        }
    }

    /// [F11 §2.9]: a `FILEOBS` upsert image and its delete image round-trip; the dead-row rules hold.
    #[test]
    fn row_images() {
        let row = fileobs_row();
        let mut w = Writer::new();
        encode_image(&row, &mut w);
        let mut r = Reader::new(w.as_slice());
        assert_eq!(
            decode_image(Table::FileObs, &mut r, Place::Upsert).unwrap(),
            row
        );
        assert!(r.is_empty());
        // [F08 §5.4.1]: a stored path is a non-empty RelPath, in a row image as in a value.
        let mut empty_path = row.clone();
        if let FV::Slice(Slice::FileObsVar(v)) = &mut empty_path.vals[16] {
            v.proposals[0].path.text.clear();
        }
        let mut w = Writer::new();
        encode_image(&empty_path, &mut w);
        assert!(
            decode_image(
                Table::FileObs,
                &mut Reader::new(w.as_slice()),
                Place::Upsert
            )
            .is_err()
        );
        let mut dead = row.clone();
        for (i, (name, _)) in Table::FileObs.fields().iter().enumerate() {
            if !["n", "tree"].contains(name) {
                dead.vals[i] = match &dead.vals[i] {
                    FV::U(_) => FV::U(0),
                    FV::OsFileId(_) => FV::OsFileId(OsFileId::default()),
                    FV::FsTime(_) => FV::FsTime(FsTime::default()),
                    FV::Oid(_) => FV::Oid(Oid::None),
                    FV::Slice(_) => FV::Slice(Slice::FileObsVar(Box::new(FileObsVar {
                        path_seen: None,
                        target: None,
                        details: vec![],
                        proposals: vec![],
                    }))),
                    v => v.clone(),
                };
            }
        }
        dead.vals[3] = FV::U(0x80);
        let mut w = Writer::new();
        encode_image(&dead, &mut w);
        assert_eq!(w.len(), 187);
        assert!(
            decode_image(
                Table::FileObs,
                &mut Reader::new(w.as_slice()),
                Place::Delete
            )
            .is_ok()
        );
        assert!(
            decode_image(
                Table::FileObs,
                &mut Reader::new(w.as_slice()),
                Place::Upsert
            )
            .is_err()
        );
    }

    /// [F11 §8]: the canonical `IDEM` layout; a moved entry is refused.
    #[test]
    fn idem_layout() {
        let entry = |k: u8| Row {
            table: Table::Idem,
            vals: vec![
                FV::Bytes(vec![k; 16]),
                FV::Bytes(vec![1; 16]),
                FV::U(0),
                FV::U(1),
                FV::U(3),
                FV::U(1),
                FV::Bytes(vec![0; 3]),
                FV::U(5),
                FV::U(200),
                FV::Slice(Slice::Raw(vec![])),
            ],
        };
        let empty = Row {
            table: Table::Idem,
            vals: vec![
                FV::Bytes(vec![0; 16]),
                FV::Bytes(vec![0; 16]),
                FV::U(0),
                FV::U(0),
                FV::U(0),
                FV::U(0),
                FV::Bytes(vec![0; 3]),
                FV::U(0),
                FV::U(0),
                FV::Slice(Slice::Raw(vec![])),
            ],
        };
        // keys 0x02.. and 0x03.. have homes 2 and 3 (low byte of the u64 & 15)
        let mut rows = vec![empty.clone(); 16];
        rows[2] = entry(2);
        rows[3] = entry(3);
        let s = Section {
            table: Table::Idem,
            hdr: RtHdr {
                n_rows: 16,
                row_size: 72,
                form: 1,
                aux: 2,
                index_len: 0,
                heap_len: 0,
            },
            rows,
            index: vec![],
        };
        let b = encode_section(&s);
        let back = decode_section(Table::Idem, &b, 0, SegKind::Base).unwrap();
        assert_eq!(back, s);
        let mut moved = s.clone();
        moved.rows.swap(3, 4);
        assert!(decode_section(Table::Idem, &encode_section(&moved), 0, SegKind::Base).is_err());

        // Two inline rows, the second with an empty result, then a commit result: the empty inline result sits at
        // the running offset 2 (§2.3), the commit result holds the zero HeapRef (§8); both directions agree.
        let inline = |k: u8, res: &[u8]| {
            let mut r = entry(k);
            r.vals[5] = FV::U(5);
            r.vals[9] = FV::Slice(Slice::Raw(res.to_vec()));
            r
        };
        let mut rows = vec![empty.clone(); 16];
        rows[2] = inline(2, b"{}");
        rows[3] = inline(3, b"");
        rows[5] = entry(5);
        let s = Section {
            table: Table::Idem,
            hdr: RtHdr {
                n_rows: 16,
                row_size: 72,
                form: 1,
                aux: 3,
                index_len: 0,
                heap_len: 2,
            },
            rows,
            index: vec![],
        };
        let b = encode_section(&s);
        let result_ref = |slot: usize| 24 + slot * 72 + 64;
        assert_eq!(
            &b[result_ref(3)..result_ref(3) + 8],
            &[2, 0, 0, 0, 0, 0, 0, 0]
        );
        assert_eq!(&b[result_ref(5)..result_ref(5) + 8], &[0; 8]);
        assert_eq!(
            decode_section(Table::Idem, &b, 0, SegKind::Base).unwrap(),
            s
        );
        let mut zeroed = b.clone();
        zeroed[result_ref(3)] = 0;
        assert!(decode_section(Table::Idem, &zeroed, 0, SegKind::Base).is_err());
        let mut running = b.clone();
        running[result_ref(5)] = 2;
        assert!(decode_section(Table::Idem, &running, 0, SegKind::Base).is_err());
    }

    /// [F11 §5]: a `HEADS` row of kind 2 or 3 keeps `root_id` at kind 0.
    #[test]
    fn heads_root_id_only_for_directories() {
        let mut w = Writer::new();
        let text = "claude:5e551070";
        let mut row = Row {
            table: Table::Heads,
            vals: vec![
                FV::Bytes(blake3_128(text.as_bytes()).to_vec()),
                FV::U(3),
                FV::U(0),
                FV::U(0),
                FV::Bytes(vec![0]),
                FV::U(0),
                FV::Bytes(vec![0; 16]),
                FV::U(0),
                FV::OsFileId(OsFileId::default()),
                FV::Binding(BindingExt::decode(&mut Reader::new(&[0; 40])).unwrap()),
                FV::U(7),
                FV::Slice(Slice::Text(text.into())),
            ],
        };
        encode_image(&row, &mut w);
        assert!(decode_image(Table::Heads, &mut Reader::new(w.as_slice()), Place::Upsert).is_ok());
        row.vals[3] = FV::U(1);
        row.vals[8] = FV::OsFileId(OsFileId {
            kind: 1,
            vol_key: [2; 16],
            id: [3; 16],
            parent: [0; 16],
            aux: 0,
            docid: 0,
        });
        let mut w = Writer::new();
        encode_image(&row, &mut w);
        let e =
            decode_image(Table::Heads, &mut Reader::new(w.as_slice()), Place::Upsert).unwrap_err();
        assert!(e.reason.contains("not a directory row"), "{e}");
    }

    /// [F11 §11]: the literal prefix of a glob.
    #[test]
    fn glob_prefix() {
        assert_eq!(glob_prefix_len("crates/phys/**"), 12);
        assert_eq!(glob_prefix_len("*.md"), 0);
        assert_eq!(glob_prefix_len("docs/a.md"), 5);
        assert_eq!(glob_prefix_len("a/b[c]/d"), 2);
    }

    /// [OS/clock §4.1]: `NEVER` is valid although `mono` ≠ 0 with `boot_hash` 0.
    #[test]
    fn stamp_never() {
        let mut w = Writer::new();
        Stamp::NEVER.encode(&mut w);
        assert_eq!(
            Stamp::decode(&mut Reader::new(w.as_slice())).unwrap(),
            Stamp::NEVER
        );
        let mut w = Writer::new();
        Stamp {
            wall: 1,
            boot_hash: 0,
            mono: 5,
        }
        .encode(&mut w);
        assert!(Stamp::decode(&mut Reader::new(w.as_slice())).is_err());
    }
}

#[cfg(test)]
mod props {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// [F11 §2], §3: a section with a valid `RtHdr` (the table's `row_size`, the form its segment kind requires, the
        /// `REFS` index length and a `heap_len` that makes the length add up) over sparse rows and an arbitrary heap
        /// reaches the row decoders and checks; it never panics, and an accepted section re-encodes to its bytes.
        #[test]
        fn section_decode_is_canonical(
            t in 0..TABLES.len(),
            n_rows in 0usize..4,
            delta in any::<bool>(),
            aux in prop_oneof![Just(0u32), Just(1), any::<u32>()],
            edits in proptest::collection::vec((any::<usize>(), any::<u8>()), 0..12),
            index in proptest::collection::vec(0u32..4, 4),
            heap in proptest::collection::vec(any::<u8>(), 0..48),
        ) {
            let table = TABLES[t];
            let seg = if delta { SegKind::Delta } else { SegKind::Base };
            let form = match (table.delta_form(), seg) {
                (DeltaForm::Layer | DeltaForm::Versioned, SegKind::Delta) => 2,
                _ => 1,
            };
            let rs = table.row_size();
            let mut rows = vec![0u8; n_rows * rs];
            if !rows.is_empty() {
                let n = rows.len();
                for (at, v) in edits {
                    rows[at % n] = v;
                }
            }
            let index_len = if table == Table::Refs { 4 * n_rows } else { 0 };
            let mut b = Writer::new();
            b.u32(n_rows as u32);
            b.u16(rs as u16);
            b.u8(form);
            b.u8(0);
            b.u32(aux);
            b.u32(index_len as u32);
            b.u64(heap.len() as u64);
            b.bytes(&rows);
            for i in index.iter().take(index_len / 4) {
                b.u32(*i);
            }
            b.bytes(&heap);
            let b = b.into_vec();
            if let Ok(s) = decode_section(table, &b, 0, seg) {
                prop_assert_eq!(encode_section(&s), b);
            }
        }

        /// [F11 §14] with damage: the worked `MARKERS` section with up to three bytes changed is refused or read
        /// canonically.
        #[test]
        fn damaged_markers_is_canonical_or_refused(
            edits in proptest::collection::vec((0usize..100, any::<u8>()), 1..4),
        ) {
            let mut b = super::tests::markers_example();
            for (at, v) in edits {
                b[at] = v;
            }
            if let Ok(s) = decode_section(Table::Markers, &b, 0, SegKind::Base) {
                prop_assert_eq!(encode_section(&s), b);
            }
        }
    }
}
