//! [F03] the `LOCK` file: `LockHdr` (§4), `ProcId` and `ParentRec` (§5), `WriterDiag` (§6), `LeaderRec` (§7), the 256
//! `SlotRec`s (§8) and the 32-byte holder `Anchor` (§10), with the checksums of §11.
//!
//! The records other than `LockHdr` read as *absent* when their checksum fails (§6.3, §7.3, §8.5); the decoders here
//! report that as [`Rec::Absent`] and decode a record only when its checksum matches, so that an all-zero record (the
//! initial content, §2.3) decodes as absent and re-encodes as zeros. A `WriterDiag` or `LeaderRec` whose checksum
//! matches but whose fields break the record's rules is [`Rec::Unusable`]: "holder not recorded" (§6.3 WD-4) or not
//! used (§7.2 LR-2), while `LOCK` itself stays valid.

use crate::prim::{Error, Reader, Result, Writer, err, xxh3_64};

/// Size of `LOCK` ([F03 §2.1]).
pub const LOCK_LEN: usize = 36_864;
/// `ROLE_BASE` = 2^62 ([F03 §3]).
pub const ROLE_BASE: u64 = 1 << 62;
/// `SLOT_BASE` = 2^62 + 2^16 ([F03 §3]).
pub const SLOT_BASE: u64 = (1 << 62) + (1 << 16);
/// Number of liveness slots ([F03 §4.1]).
pub const N_SLOTS: usize = 256;
/// Size of one `SlotRec` ([F03 §8.1]).
pub const SLOT_REC_LEN: usize = 128;
/// Offset of `WriterDiag` ([F03 §2.2]).
pub const WRITER_DIAG_OFF: usize = 2048;
/// Offset of `LeaderRec`.
pub const LEADER_OFF: usize = 3072;
/// Offset of `SlotRec[0]`.
pub const SLOTS_OFF: usize = 4096;

/// The lock-byte offset of each role ([F03 §3]); `quiet(k)` for quiet 0 … quiet 8 (§3.1).
pub mod bytes {
    use super::{ROLE_BASE, SLOT_BASE};
    /// writer, rank 4.
    pub const WRITER: u64 = ROLE_BASE;
    /// leader, rank 1.
    pub const LEADER: u64 = ROLE_BASE + 1;
    /// maintenance, rank 2.
    pub const MAINTENANCE: u64 = ROLE_BASE + 2;
    /// flush, rank 3.
    pub const FLUSH: u64 = ROLE_BASE + 4;
    /// quiet 0 at `ROLE_BASE` + 3, quiet 1 … 8 at `ROLE_BASE` + 5 … + 12 ([F03 §3.1]).
    pub const fn quiet(k: u64) -> u64 {
        if k == 0 {
            ROLE_BASE + 3
        } else {
            ROLE_BASE + 4 + k
        }
    }
    /// slot i.
    pub const fn slot(i: u64) -> u64 {
        SLOT_BASE + i
    }
}

/// A record that is absent (checksum mismatch), unusable (checksum match, fields break its rules) or decoded
/// ([F03 §6.3], §7.2, §7.3, §8.5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Rec<T> {
    /// The checksum failed; the raw bytes are kept for a byte-identical re-encode.
    Absent(Vec<u8>),
    /// The checksum matched but a field breaks the record's rules (non-zero reserved bytes, a value outside its
    /// enumeration, an all-zero `nonce`, a malformed `fstr`): WD-4's "holder not recorded", LR-2's "not used". The raw
    /// bytes are kept for re-encoding, with the first rule broken.
    Unusable(Vec<u8>, Error),
    /// The checksum matched and the record decoded.
    Present(T),
}

impl<T> Rec<T> {
    /// The record of a checksum-matching region: `decoded` or, when that failed, [`Rec::Unusable`].
    fn checked(raw: &[u8], decoded: Result<T>) -> Rec<T> {
        match decoded {
            Ok(v) => Rec::Present(v),
            Err(e) => Rec::Unusable(raw.to_vec(), e),
        }
    }

    /// The bytes of an absent or unusable record, which re-encode as read.
    pub fn raw(&self) -> Option<&[u8]> {
        match self {
            Rec::Absent(b) | Rec::Unusable(b, _) => Some(b),
            Rec::Present(_) => None,
        }
    }
}

/// `LockHdr` ([F03 §4.1]), 64 bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LockHdr {
    /// `flags`: no bit defined; must be 0 (LH-2).
    pub flags: u16,
    /// `created_hlc`: diagnostics only.
    pub created_hlc: u64,
}

impl LockHdr {
    /// Decodes and validates LH-2 ([F03 §4.2]): magic, checksum, `format` = 1, the four constants, `flags` = 0 and
    /// `_reserved` = 0.
    pub fn decode(r: &mut Reader<'_>) -> Result<Self> {
        let start = r.offset();
        let raw = r.rest();
        if raw.len() < 64 {
            return r.fail("LockHdr needs 64 bytes [F03 §4.1]");
        }
        let sum = xxh3_64(&raw[..56]);
        if r.array::<4>()? != *b"MLCK" {
            return err(start, "LockHdr magic is not MLCK [F03 §4.1]");
        }
        let at = r.offset();
        let format = r.u16()?;
        check_format(format, at, "LOCK")?;
        let at = r.offset();
        let flags = r.u16()?;
        if flags != 0 {
            return err(at, "LockHdr.flags is not 0 [F03 §4.2 LH-2]");
        }
        let at = r.offset();
        if r.u16()? != N_SLOTS as u16 {
            return err(at, "LockHdr.n_slots is not 256 [F03 §4.1]");
        }
        let at = r.offset();
        if r.u16()? != SLOT_REC_LEN as u16 {
            return err(at, "LockHdr.slot_rec_size is not 128 [F03 §4.1]");
        }
        let at = r.offset();
        if r.u64()? != ROLE_BASE {
            return err(at, "LockHdr.role_base is not 2^62 [F03 §4.1]");
        }
        let at = r.offset();
        if r.u64()? != SLOT_BASE {
            return err(at, "LockHdr.slot_base is not 2^62 + 2^16 [F03 §4.1]");
        }
        let created_hlc = r.u64()?;
        r.zeros(20, "LockHdr._reserved")?;
        let at = r.offset();
        if r.u64()? != sum {
            return err(at, "LockHdr.xxh3 mismatch [F03 §11]");
        }
        Ok(LockHdr { flags, created_hlc })
    }

    /// Re-encodes the 64 bytes, checksum recomputed.
    pub fn encode(&self, w: &mut Writer) {
        let s = w.len();
        w.bytes(b"MLCK");
        w.u16(1);
        w.u16(self.flags);
        w.u16(N_SLOTS as u16);
        w.u16(SLOT_REC_LEN as u16);
        w.u64(ROLE_BASE);
        w.u64(SLOT_BASE);
        w.u64(self.created_hlc);
        w.zeros(20);
        let sum = xxh3_64(&w.as_slice()[s..s + 56]);
        w.u64(sum);
    }
}

/// [F01 §9.1] format-version check: 0 invalid, above 1 refused naming both versions.
pub fn check_format(format: u16, at: usize, file: &str) -> Result<()> {
    match format {
        1 => Ok(()),
        0 => err(at, format!("{file}: format 0 is invalid [F01 §9.1]")),
        v => err(
            at,
            format!("{file}: format {v} is newer than 1; exit 7 [F01 §9.1]"),
        ),
    }
}

/// `ProcId` ([F03 §5.1]), 32 bytes. Decoding never fails on content: an uninterpretable `ProcId` stays valid inside its
/// record (§5.1); [`ProcId::interpretable`] tells.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct ProcId {
    /// OS tag ([F01 §3.2]).
    pub os: u8,
    /// Bits 0–3 defined, 4–7 reserved-zero.
    pub flags: u8,
    /// `_reserved` u16 as read (must be 0 to be interpretable).
    pub reserved: u16,
    /// OS process id.
    pub pid: u32,
    /// Start time in ns.
    pub start: u64,
    /// `boot_hash`.
    pub boot_hash: u64,
    /// Linux pid namespace inode.
    pub pidns: u64,
}

impl ProcId {
    /// Decodes 32 bytes.
    pub fn decode(r: &mut Reader<'_>) -> Result<Self> {
        Ok(ProcId {
            os: r.u8()?,
            flags: r.u8()?,
            reserved: r.u16()?,
            pid: r.u32()?,
            start: r.u64()?,
            boot_hash: r.u64()?,
            pidns: r.u64()?,
        })
    }

    /// Encodes 32 bytes.
    pub fn encode(&self, w: &mut Writer) {
        w.u8(self.os);
        w.u8(self.flags);
        w.u16(self.reserved);
        w.u32(self.pid);
        w.u64(self.start);
        w.u64(self.boot_hash);
        w.u64(self.pidns);
    }

    /// [F03 §5.1]: `os` ∈ 1–3 and every reserved bit and byte zero.
    pub fn interpretable(&self) -> bool {
        (1..=3).contains(&self.os) && self.flags & 0xF0 == 0 && self.reserved == 0
    }
}

/// `ParentRec` ([F03 §5.2]), 16 bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct ParentRec {
    /// Parent pid.
    pub pid: u32,
    /// `_reserved` u32 as read (checked by the record's validity, §8.5).
    pub reserved: u32,
    /// Parent start time.
    pub start: u64,
}

impl ParentRec {
    /// Decodes 16 bytes.
    pub fn decode(r: &mut Reader<'_>) -> Result<Self> {
        Ok(ParentRec {
            pid: r.u32()?,
            reserved: r.u32()?,
            start: r.u64()?,
        })
    }

    /// Encodes 16 bytes.
    pub fn encode(&self, w: &mut Writer) {
        w.u32(self.pid);
        w.u32(self.reserved);
        w.u64(self.start);
    }
}

/// `WriterDiag` ([F03 §6.1]), 512 bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WriterDiag {
    /// The holder's acquisition counter.
    pub seq: u64,
    /// The holder process.
    pub proc: ProcId,
    /// Primary session hash; zero when none.
    pub session_hash: [u8; 16],
    /// `cmd`, `fstr<400>` (§6.2).
    pub cmd: String,
    /// `hlc`-form acquisition time.
    pub hlc: u64,
    /// `activity`, 1–7 (§6.1).
    pub activity: u8,
}

impl WriterDiag {
    /// Decodes 512 bytes; a checksum mismatch is [`Rec::Absent`]; a matching record with non-zero `_reserved`, a
    /// malformed `cmd` or an unknown `activity` is [`Rec::Unusable`] (WD-4 prints "holder not recorded").
    pub fn decode(r: &mut Reader<'_>) -> Result<Rec<Self>> {
        let raw = r.bytes(512)?;
        let base = r.offset() - 512;
        if xxh3_64(&raw[..504]) != u64::from_le_bytes(raw[504..].try_into().expect("8 bytes")) {
            return Ok(Rec::Absent(raw.to_vec()));
        }
        Ok(Rec::checked(raw, Self::decode_fields(raw, base)))
    }

    fn decode_fields(raw: &[u8], base: usize) -> Result<Self> {
        let mut r = Reader::with_base(raw, base);
        let seq = r.u64()?;
        let proc = ProcId::decode(&mut r)?;
        let session_hash = r.b16()?;
        let cmd = r.fstr(400)?.to_owned();
        let hlc = r.u64()?;
        let at = r.offset();
        let activity = r.u8()?;
        if !(1..=7).contains(&activity) {
            return err(
                at,
                format!("WriterDiag.activity {activity} is not 1-7 [F03 §6.1]"),
            );
        }
        r.zeros(39, "WriterDiag._reserved")?;
        Ok(WriterDiag {
            seq,
            proc,
            session_hash,
            cmd,
            hlc,
            activity,
        })
    }

    /// Encodes 512 bytes, checksum recomputed.
    pub fn encode(&self, w: &mut Writer) {
        let s = w.len();
        w.u64(self.seq);
        self.proc.encode(w);
        w.bytes(&self.session_hash);
        w.fstr(400, &self.cmd);
        w.u64(self.hlc);
        w.u8(self.activity);
        w.zeros(39);
        let sum = xxh3_64(&w.as_slice()[s..s + 504]);
        w.u64(sum);
    }
}

/// `LeaderRec` ([F03 §7.1]), 512 bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LeaderRec {
    /// Election counter.
    pub seq: u64,
    /// Leader process.
    pub proc: ProcId,
    /// Endpoint protocol version; 1.
    pub proto: u16,
    /// 1 named pipe, 2 Unix socket.
    pub endpoint_kind: u8,
    /// `endpoint`, `fstr<200>`.
    pub endpoint: String,
    /// Election nonce, never all zero.
    pub nonce: [u8; 16],
}

impl LeaderRec {
    /// Decodes 512 bytes: a checksum mismatch is [`Rec::Absent`] (§7.3); a matching record that fails §7.2 LR-2's byte
    /// checks (reserved bytes, `endpoint_kind`, `proto`, `nonce`, the `fstr`) is [`Rec::Unusable`] (not used).
    pub fn decode(r: &mut Reader<'_>) -> Result<Rec<Self>> {
        let raw = r.bytes(512)?;
        let base = r.offset() - 512;
        if xxh3_64(&raw[..504]) != u64::from_le_bytes(raw[504..].try_into().expect("8 bytes")) {
            return Ok(Rec::Absent(raw.to_vec()));
        }
        Ok(Rec::checked(raw, Self::decode_fields(raw, base)))
    }

    fn decode_fields(raw: &[u8], base: usize) -> Result<Self> {
        let mut r = Reader::with_base(raw, base);
        let seq = r.u64()?;
        let proc = ProcId::decode(&mut r)?;
        let at = r.offset();
        let proto = r.u16()?;
        if proto != 1 {
            return err(at, "LeaderRec.proto is not 1 [F03 §7.1]");
        }
        let at = r.offset();
        let endpoint_kind = r.u8()?;
        if !(1..=2).contains(&endpoint_kind) {
            return err(at, "LeaderRec.endpoint_kind is not 1 or 2 [F03 §7.1]");
        }
        r.zeros(5, "LeaderRec._reserved")?;
        let endpoint = r.fstr(200)?.to_owned();
        let at = r.offset();
        let nonce = r.b16()?;
        if nonce == [0; 16] {
            return err(at, "LeaderRec.nonce is all zero [F03 §7.1]");
        }
        r.zeros(240, "LeaderRec._reserved2")?;
        Ok(LeaderRec {
            seq,
            proc,
            proto,
            endpoint_kind,
            endpoint,
            nonce,
        })
    }

    /// Encodes 512 bytes, checksum recomputed.
    pub fn encode(&self, w: &mut Writer) {
        let s = w.len();
        w.u64(self.seq);
        self.proc.encode(w);
        w.u16(self.proto);
        w.u8(self.endpoint_kind);
        w.zeros(5);
        w.fstr(200, &self.endpoint);
        w.bytes(&self.nonce);
        w.zeros(240);
        let sum = xxh3_64(&w.as_slice()[s..s + 504]);
        w.u64(sum);
    }
}

/// `SlotRec` ([F03 §8.1]), 128 bytes, decoded when its checksum matches.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlotRec {
    /// Record version.
    pub ver: u8,
    /// 0 free, 1 session, 2 intent.
    pub kind: u8,
    /// Writer's OS tag.
    pub os: u8,
    /// §8.3 flags.
    pub flags: u8,
    /// The slot index.
    pub slot: u16,
    /// `_reserved` u16 as read.
    pub reserved: u16,
    /// Holding nonce.
    pub nonce: u64,
    /// Primary session hash.
    pub session_hash: [u8; 16],
    /// Alias hash.
    pub alias_hash: [u8; 16],
    /// Holder process.
    pub proc: ProcId,
    /// Parent process.
    pub parent: ParentRec,
    /// Slot take time.
    pub acquired_hlc: u64,
    /// `_reserved2` as read.
    pub reserved2: [u8; 16],
}

impl SlotRec {
    /// Decodes 128 bytes at position `i`; a checksum mismatch is [`Rec::Absent`].
    pub fn decode(r: &mut Reader<'_>) -> Result<Rec<Self>> {
        let raw = r.bytes(SLOT_REC_LEN)?;
        let base = r.offset() - SLOT_REC_LEN;
        if xxh3_64(&raw[..120]) != u64::from_le_bytes(raw[120..].try_into().expect("8 bytes")) {
            return Ok(Rec::Absent(raw.to_vec()));
        }
        let mut r = Reader::with_base(raw, base);
        Ok(Rec::Present(SlotRec {
            ver: r.u8()?,
            kind: r.u8()?,
            os: r.u8()?,
            flags: r.u8()?,
            slot: r.u16()?,
            reserved: r.u16()?,
            nonce: r.u64()?,
            session_hash: r.b16()?,
            alias_hash: r.b16()?,
            proc: ProcId::decode(&mut r)?,
            parent: ParentRec::decode(&mut r)?,
            acquired_hlc: r.u64()?,
            reserved2: r.b16()?,
        }))
    }

    /// Encodes 128 bytes, checksum recomputed.
    pub fn encode(&self, w: &mut Writer) {
        let s = w.len();
        w.u8(self.ver);
        w.u8(self.kind);
        w.u8(self.os);
        w.u8(self.flags);
        w.u16(self.slot);
        w.u16(self.reserved);
        w.u64(self.nonce);
        w.bytes(&self.session_hash);
        w.bytes(&self.alias_hash);
        self.proc.encode(w);
        self.parent.encode(w);
        w.u64(self.acquired_hlc);
        w.bytes(&self.reserved2);
        let sum = xxh3_64(&w.as_slice()[s..s + 120]);
        w.u64(sum);
    }

    /// The `harness` field of §8.3 (bits 2–3).
    pub fn harness(&self) -> u8 {
        (self.flags >> 2) & 3
    }

    /// [F03 §8.5]: the record is live at position `i`. Returns the first failing rule, if any.
    pub fn liveness(&self, i: usize) -> core::result::Result<(), &'static str> {
        if self.ver != 1 {
            return Err("ver is not 1");
        }
        if !(1..=2).contains(&self.kind) {
            return Err("kind is not 1 or 2");
        }
        if !(1..=3).contains(&self.os) {
            return Err("os is not 1-3");
        }
        if usize::from(self.slot) != i {
            return Err("slot differs from its position");
        }
        if self.nonce == 0 {
            return Err("nonce is 0");
        }
        if self.flags & 0xF0 != 0
            || self.reserved != 0
            || self.reserved2 != [0; 16]
            || self.parent.reserved != 0
        {
            return Err("a reserved bit or byte is not zero");
        }
        if self.harness() == 3 {
            return Err("harness is 3");
        }
        if self.kind == 1 && (!(1..=2).contains(&self.harness()) || self.session_hash == [0; 16]) {
            return Err("a session record needs harness 1 or 2 and a non-zero session hash");
        }
        if self.kind == 2 && self.alias_hash != [0; 16] {
            return Err("an intent record has a non-zero alias hash");
        }
        Ok(())
    }
}

/// `Anchor` ([F03 §10.1]), 32 bytes. Decoding keeps every byte; [`Anchor::interpretable`] applies §10.4.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Anchor {
    /// 0 none, 1 session, 2 intent, 3 leader, 4 session-ttl.
    pub kind: u8,
    /// OS tag.
    pub os: u8,
    /// Slot hint or exact slot.
    pub slot: u16,
    /// `_reserved` u32 as read.
    pub reserved: u32,
    /// `id`: the session hash (kinds 1, 4) or the two `u64` of §10.2 (kinds 2, 3).
    pub id: [u8; 16],
    /// `boot_hash`.
    pub boot_hash: u64,
}

impl Anchor {
    /// Decodes 32 bytes.
    pub fn decode(r: &mut Reader<'_>) -> Result<Self> {
        Ok(Anchor {
            kind: r.u8()?,
            os: r.u8()?,
            slot: r.u16()?,
            reserved: r.u32()?,
            id: r.b16()?,
            boot_hash: r.u64()?,
        })
    }

    /// Encodes 32 bytes.
    pub fn encode(&self, w: &mut Writer) {
        w.u8(self.kind);
        w.u8(self.os);
        w.u16(self.slot);
        w.u32(self.reserved);
        w.bytes(&self.id);
        w.u64(self.boot_hash);
    }

    /// [F03 §10.2] `nonce` of kinds 2 and 3.
    pub fn nonce(&self) -> u64 {
        u64::from_le_bytes(self.id[..8].try_into().expect("8 bytes"))
    }

    /// [F03 §10.4] interpretability.
    pub fn interpretable(&self) -> bool {
        if self.kind > 4 || self.reserved != 0 {
            return false;
        }
        match self.kind {
            0 => self.os == 0 && self.slot == 0 && self.id == [0; 16] && self.boot_hash == 0,
            1 | 4 => (1..=3).contains(&self.os) && self.id != [0; 16] && self.slot < 256,
            2 => (1..=3).contains(&self.os) && self.nonce() != 0 && self.slot < 256,
            _ => (1..=3).contains(&self.os) && self.slot == 0,
        }
    }
}

/// The whole `LOCK` file ([F03 §2.2]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LockFile {
    /// The header.
    pub hdr: LockHdr,
    /// `_reserved_a` (ignored on read; kept for re-encoding, reported by `nonzero_reserved`).
    pub reserved_a: Vec<u8>,
    /// Writer diagnostics.
    pub writer_diag: Rec<WriterDiag>,
    /// `_reserved_b`.
    pub reserved_b: Vec<u8>,
    /// Leader record.
    pub leader: Rec<LeaderRec>,
    /// `_reserved_c`.
    pub reserved_c: Vec<u8>,
    /// The 256 slot records.
    pub slots: Vec<Rec<SlotRec>>,
}

impl LockFile {
    /// Decodes the 36,864 bytes of `LOCK` ([F03 §2.1]: any other size makes the store unavailable).
    pub fn decode(b: &[u8]) -> Result<Self> {
        if b.len() != LOCK_LEN {
            return err(
                0,
                format!("LOCK is {} bytes, not 36864 [F03 §2.1]", b.len()),
            );
        }
        let mut r = Reader::new(b);
        let hdr = LockHdr::decode(&mut r)?;
        let reserved_a = r.bytes(1984)?.to_vec();
        let writer_diag = WriterDiag::decode(&mut r)?;
        let reserved_b = r.bytes(512)?.to_vec();
        let leader = LeaderRec::decode(&mut r)?;
        let reserved_c = r.bytes(512)?.to_vec();
        let mut slots = Vec::with_capacity(N_SLOTS);
        for _ in 0..N_SLOTS {
            slots.push(SlotRec::decode(&mut r)?);
        }
        r.finish("LOCK")?;
        Ok(LockFile {
            hdr,
            reserved_a,
            writer_diag,
            reserved_b,
            leader,
            reserved_c,
            slots,
        })
    }

    /// Re-encodes the file.
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::new();
        self.hdr.encode(&mut w);
        w.bytes(&self.reserved_a);
        match &self.writer_diag {
            Rec::Present(d) => d.encode(&mut w),
            other => w.bytes(other.raw().expect("absent or unusable")),
        }
        w.bytes(&self.reserved_b);
        match &self.leader {
            Rec::Present(d) => d.encode(&mut w),
            other => w.bytes(other.raw().expect("absent or unusable")),
        }
        w.bytes(&self.reserved_c);
        for s in &self.slots {
            match s {
                Rec::Present(d) => d.encode(&mut w),
                other => w.bytes(other.raw().expect("absent or unusable")),
            }
        }
        w.into_vec()
    }

    /// Offsets of non-zero bytes in the three ignored regions (what `doctor --fsck` reports, [F03 §2.2]).
    pub fn nonzero_reserved(&self) -> Vec<usize> {
        let mut v = Vec::new();
        for (base, reg) in [
            (64, &self.reserved_a),
            (2560, &self.reserved_b),
            (3584, &self.reserved_c),
        ] {
            v.extend(
                reg.iter()
                    .enumerate()
                    .filter(|(_, b)| **b != 0)
                    .map(|(i, _)| base + i),
            );
        }
        v
    }

    /// The live slot records with their index ([F03 §8.5]).
    pub fn live_slots(&self) -> Vec<(usize, &SlotRec)> {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(i, s)| match s {
                Rec::Present(r) if r.liveness(i).is_ok() => Some((i, r)),
                _ => None,
            })
            .collect()
    }
}

/// [F03 §9.2] `session_hash(<harness>:<id>) = BLAKE3-128(UTF-8 bytes)`.
pub fn session_hash(identity: &str) -> [u8; 16] {
    crate::prim::blake3_128(identity.as_bytes())
}

/// [F03 §8.7] the first slot a server tries: H mod 256 with H the first 8 bytes of its primary hash.
pub fn first_slot(primary: &[u8; 16]) -> usize {
    (crate::prim::le_u64(primary) % 256) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hdr_bytes() -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(b"MLCK");
        b.extend_from_slice(&[1, 0, 0, 0, 0, 1, 128, 0]);
        b.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0x40]);
        b.extend_from_slice(&[0, 0, 1, 0, 0, 0, 0, 0x40]);
        b.extend_from_slice(&(0x0001_0000_0000u64 << 16).to_le_bytes());
        b.extend_from_slice(&[0; 20]);
        let s = xxh3_64(&b);
        b.extend_from_slice(&s.to_le_bytes());
        b
    }

    /// [F03 §4.1]: the header's constant bytes as the table spells them (`00 .. 40`, `00 00 01 00 .. 40`).
    #[test]
    fn lock_hdr_from_table() {
        let b = hdr_bytes();
        assert_eq!(b.len(), 64);
        let h = LockHdr::decode(&mut Reader::new(&b)).unwrap();
        let mut w = Writer::new();
        h.encode(&mut w);
        assert_eq!(w.as_slice(), &b[..]);
        let mut bad = b.clone();
        bad[6] = 1;
        assert!(LockHdr::decode(&mut Reader::new(&bad)).is_err());
        let mut v2 = b.clone();
        v2[4] = 2;
        let s = xxh3_64(&v2[..56]);
        v2[56..].copy_from_slice(&s.to_le_bytes());
        assert!(
            LockHdr::decode(&mut Reader::new(&v2))
                .unwrap_err()
                .reason
                .contains("newer")
        );
    }

    /// [F03 §2.3]: a fresh `LOCK` is the header and zeros; every record reads as absent.
    #[test]
    fn initial_lock_file() {
        let mut b = hdr_bytes();
        b.resize(LOCK_LEN, 0);
        let f = LockFile::decode(&b).unwrap();
        assert!(matches!(f.writer_diag, Rec::Absent(_)));
        assert!(matches!(f.leader, Rec::Absent(_)));
        assert!(f.slots.iter().all(|s| matches!(s, Rec::Absent(_))));
        assert!(f.live_slots().is_empty());
        assert_eq!(f.encode(), b);
        assert!(LockFile::decode(&b[..LOCK_LEN - 1]).is_err());
    }

    /// [F03 §8.1], §8.5: a live session slot record built from the offset table; a torn byte makes it absent.
    #[test]
    fn slot_rec_round_trip() {
        let primary = session_hash("claude:0f2c");
        let i = first_slot(&primary);
        let rec = SlotRec {
            ver: 1,
            kind: 1,
            os: 1,
            flags: 0b0000_0111,
            slot: i as u16,
            reserved: 0,
            nonce: 0x55,
            session_hash: primary,
            alias_hash: [0; 16],
            proc: ProcId {
                os: 1,
                flags: 0b0101,
                reserved: 0,
                pid: 4242,
                start: 7,
                boot_hash: 9,
                pidns: 0,
            },
            parent: ParentRec {
                pid: 1,
                reserved: 0,
                start: 2,
            },
            acquired_hlc: 5 << 16,
            reserved2: [0; 16],
        };
        let mut w = Writer::new();
        rec.encode(&mut w);
        assert_eq!(w.len(), 128);
        assert_eq!(&w.as_slice()[16..32], &primary);
        let back = SlotRec::decode(&mut Reader::new(w.as_slice())).unwrap();
        assert_eq!(back, Rec::Present(rec.clone()));
        assert!(rec.liveness(i).is_ok());
        assert!(rec.liveness((i + 1) % 256).is_err());
        let mut torn = w.into_vec();
        torn[40] ^= 1;
        assert!(matches!(
            SlotRec::decode(&mut Reader::new(&torn)).unwrap(),
            Rec::Absent(_)
        ));
    }

    /// [F03 §6.1]: `WriterDiag` with the `cmd` example of §6.2.
    #[test]
    fn writer_diag_round_trip() {
        let d = WriterDiag {
            seq: 3,
            proc: ProcId::default(),
            session_hash: [0; 16],
            cmd: "gc --rollup --if-needed".into(),
            hlc: 1 << 16,
            activity: 4,
        };
        let mut w = Writer::new();
        d.encode(&mut w);
        assert_eq!(w.len(), 512);
        assert_eq!(
            WriterDiag::decode(&mut Reader::new(w.as_slice())).unwrap(),
            Rec::Present(d)
        );
    }

    /// [F03 §6.3] WD-4: a `WriterDiag` with a matching checksum and an `activity` outside 1–7 is unusable ("holder not
    /// recorded"); `LOCK` stays valid and re-encodes byte for byte.
    #[test]
    fn unusable_writer_diag_keeps_lock_valid() {
        let d = WriterDiag {
            seq: 3,
            proc: ProcId::default(),
            session_hash: [0; 16],
            cmd: "commit".into(),
            hlc: 1 << 16,
            activity: 9,
        };
        let mut w = Writer::new();
        d.encode(&mut w);
        match WriterDiag::decode(&mut Reader::new(w.as_slice())).unwrap() {
            Rec::Unusable(raw, e) => {
                assert_eq!(raw, w.as_slice());
                assert!(e.reason.contains("activity"), "{e}");
            }
            other => panic!("{other:?}"),
        }
        let mut b = hdr_bytes();
        b.resize(LOCK_LEN, 0);
        b[WRITER_DIAG_OFF..WRITER_DIAG_OFF + 512].copy_from_slice(w.as_slice());
        let f = LockFile::decode(&b).unwrap();
        assert!(matches!(f.writer_diag, Rec::Unusable(..)));
        assert_eq!(f.encode(), b);
    }

    /// [F03 §7.1]: `LeaderRec` round trip; an all-zero nonce makes a checksum-valid record unusable (LR-2).
    #[test]
    fn leader_rec_round_trip() {
        let d = LeaderRec {
            seq: 1,
            proc: ProcId::default(),
            proto: 1,
            endpoint_kind: 1,
            endpoint: r"\\.\pipe\moirai-x".into(),
            nonce: [9; 16],
        };
        let mut w = Writer::new();
        d.encode(&mut w);
        assert_eq!(
            LeaderRec::decode(&mut Reader::new(w.as_slice())).unwrap(),
            Rec::Present(d.clone())
        );
        let mut z = d;
        z.nonce = [0; 16];
        let mut w = Writer::new();
        z.encode(&mut w);
        assert!(matches!(
            LeaderRec::decode(&mut Reader::new(w.as_slice())).unwrap(),
            Rec::Unusable(..)
        ));
    }

    /// [F03 §10.3] step 5: a kind-0 anchor is 32 zero bytes; §10.4 interpretability.
    #[test]
    fn anchor_rules() {
        let a = Anchor::decode(&mut Reader::new(&[0; 32])).unwrap();
        assert!(a.interpretable());
        let mut w = Writer::new();
        a.encode(&mut w);
        assert_eq!(w.as_slice(), &[0; 32]);
        let mut k2 = Anchor {
            kind: 2,
            os: 1,
            slot: 7,
            ..Anchor::default()
        };
        assert!(!k2.interpretable());
        k2.id[0] = 1;
        assert!(k2.interpretable());
        assert_eq!(k2.nonce(), 1);
        k2.slot = 256;
        assert!(!k2.interpretable());
    }

    /// [F03 §3]: the lock-byte map.
    #[test]
    fn lock_byte_offsets() {
        assert_eq!(bytes::WRITER, 0x4000_0000_0000_0000);
        assert_eq!(bytes::quiet(0), ROLE_BASE + 3);
        assert_eq!(bytes::quiet(1), ROLE_BASE + 5);
        assert_eq!(bytes::quiet(8), ROLE_BASE + 12);
        assert_eq!(bytes::slot(255), 0x4000_0000_0001_00FF);
    }
}

#[cfg(test)]
mod props {
    use super::*;
    use proptest::prelude::*;

    fn live_file() -> Vec<u8> {
        let mut b = Vec::with_capacity(LOCK_LEN);
        b.extend_from_slice(b"MLCK");
        b.extend_from_slice(&[1, 0, 0, 0, 0, 1, 128, 0]);
        b.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0x40]);
        b.extend_from_slice(&[0, 0, 1, 0, 0, 0, 0, 0x40]);
        b.extend_from_slice(&(0x0001_0000_0000u64 << 16).to_le_bytes());
        b.extend_from_slice(&[0; 20]);
        let s = xxh3_64(&b);
        b.extend_from_slice(&s.to_le_bytes());
        b.resize(LOCK_LEN, 0);
        let mut w = Writer::new();
        WriterDiag {
            seq: 1,
            proc: ProcId::default(),
            session_hash: [0; 16],
            cmd: "commit".into(),
            hlc: 1 << 16,
            activity: 1,
        }
        .encode(&mut w);
        b[WRITER_DIAG_OFF..WRITER_DIAG_OFF + 512].copy_from_slice(w.as_slice());
        let primary = session_hash("codex:p");
        let i = first_slot(&primary);
        let mut w = Writer::new();
        SlotRec {
            ver: 1,
            kind: 1,
            os: 1,
            flags: 0b0000_0111,
            slot: i as u16,
            reserved: 0,
            nonce: 1,
            session_hash: primary,
            alias_hash: [0; 16],
            proc: ProcId {
                os: 1,
                flags: 0b0101,
                reserved: 0,
                pid: 7,
                start: 1,
                boot_hash: 2,
                pidns: 0,
            },
            parent: ParentRec::default(),
            acquired_hlc: 1 << 16,
            reserved2: [0; 16],
        }
        .encode(&mut w);
        b[SLOTS_OFF + SLOT_REC_LEN * i..SLOTS_OFF + SLOT_REC_LEN * (i + 1)]
            .copy_from_slice(w.as_slice());
        b
    }

    /// Recomputes the checksum of every record after the header ([F03 §11]), so damage reaches the field decoders.
    fn reseal(b: &mut [u8]) {
        let mut regions = vec![(WRITER_DIAG_OFF, 512), (LEADER_OFF, 512)];
        regions.extend((0..N_SLOTS).map(|i| (SLOTS_OFF + SLOT_REC_LEN * i, SLOT_REC_LEN)));
        for (off, len) in regions {
            let sum = xxh3_64(&b[off..off + len - 8]);
            b[off + len - 8..off + len].copy_from_slice(&sum.to_le_bytes());
        }
    }

    proptest! {
        /// [F03 §2.2], §5–§8: arbitrary damage past the header never makes `LOCK` undecodable, with the records'
        /// checksums left torn (the records read as absent and keep their bytes) or recomputed (the fields decode, or
        /// the record is unusable and keeps its bytes); the file re-encodes byte-identically either way.
        #[test]
        fn damaged_lock_reencodes(
            edits in proptest::collection::vec((64..LOCK_LEN, 1..=255u8), 1..8),
            resealed in any::<bool>(),
        ) {
            let mut b = live_file();
            let f = LockFile::decode(&b).unwrap();
            prop_assert!(matches!(f.writer_diag, Rec::Present(_)));
            prop_assert_eq!(f.live_slots().len(), 1);
            for (at, x) in edits {
                b[at] ^= x;
            }
            if resealed {
                reseal(&mut b);
            }
            let f = LockFile::decode(&b).unwrap();
            if resealed {
                prop_assert!(!matches!(f.writer_diag, Rec::Absent(_)));
                prop_assert!(f.slots.iter().all(|s| matches!(s, Rec::Present(_))));
            }
            prop_assert_eq!(f.encode(), b);
        }
    }
}
