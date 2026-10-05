//! \[F04\] the `HEAD` file: two 4,096-byte `HeadSlot`s (§3) with `SegRef` (§4.1), `ImageCursor` (§4.2),
//! `SeqRingEntry` (§4.3) and `InitParams` (§4.4, [F17 §2.1]); slot classification (§7) and choice (§8).

use crate::lock::check_format;
use crate::prim::{Reader, Result, Writer, err, xxh3_128};

/// Size of `HEAD` ([F04 §2]).
pub const HEAD_LEN: usize = 8192;
/// Size of one slot.
pub const SLOT_LEN: usize = 4096;

/// `InitParams` ([F04 §4.4], [F17 §2.1]), 32 bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InitParams {
    /// `store.log-extent-bytes`: a power of two in [2^16, 2^30].
    pub log_extent_bytes: u64,
    /// `store.hist-frame-commits`: 1 to 65,536.
    pub hist_frame_commits: u32,
    /// `store.hist-frame-bytes`: 4,096 to 1,048,576.
    pub hist_frame_bytes: u32,
    /// The store id, never all zero.
    pub store_id: [u8; 16],
}

impl InitParams {
    /// Decodes 32 bytes (no range check; see [`InitParams::check`]).
    pub fn decode(r: &mut Reader<'_>) -> Result<Self> {
        Ok(InitParams {
            log_extent_bytes: r.u64()?,
            hist_frame_commits: r.u32()?,
            hist_frame_bytes: r.u32()?,
            store_id: r.b16()?,
        })
    }

    /// Encodes 32 bytes.
    pub fn encode(&self, w: &mut Writer) {
        w.u64(self.log_extent_bytes);
        w.u32(self.hist_frame_commits);
        w.u32(self.hist_frame_bytes);
        w.bytes(&self.store_id);
    }

    /// [F17 §2.2] IP-2: every parameter in its range (§3 P01, P03, P04) and `store_id` not all zero.
    pub fn check(&self) -> core::result::Result<(), &'static str> {
        let e = self.log_extent_bytes;
        if !e.is_power_of_two() || !((1 << 16)..=(1 << 30)).contains(&e) {
            return Err("log_extent_bytes is not a power of two in [2^16, 2^30] [F17 §2.2 IP-2]");
        }
        if !(1..=65_536).contains(&self.hist_frame_commits) {
            return Err("hist_frame_commits is not in 1..=65536 [F17 §2.2 IP-2]");
        }
        if !(4_096..=1_048_576).contains(&self.hist_frame_bytes) {
            return Err("hist_frame_bytes is not in 4096..=1048576 [F17 §2.2 IP-2]");
        }
        if self.store_id == [0; 16] {
            return Err("store_id is all zero [F17 §2.2 IP-2]");
        }
        Ok(())
    }
}

/// `SegRef` ([F04 §4.1]), 29 bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct SegRef {
    /// File number, ≥ 1 in a used entry.
    pub file_no: u32,
    /// 0 empty, 1 base, 2 delta, 3 dict.
    pub kind: u8,
    /// Fold bound of a base or delta; 0 for a dict.
    pub upto_lsn: u64,
    /// First 16 bytes of the file's recorded BLAKE3-256 digest.
    pub blake3_16: [u8; 16],
}

impl SegRef {
    /// Decodes 29 bytes.
    pub fn decode(r: &mut Reader<'_>) -> Result<Self> {
        Ok(SegRef {
            file_no: r.u32()?,
            kind: r.u8()?,
            upto_lsn: r.u64()?,
            blake3_16: r.b16()?,
        })
    }

    /// Encodes 29 bytes.
    pub fn encode(&self, w: &mut Writer) {
        w.u32(self.file_no);
        w.u8(self.kind);
        w.u64(self.upto_lsn);
        w.bytes(&self.blake3_16);
    }

    fn is_zero(&self) -> bool {
        *self == SegRef::default()
    }
}

/// [F04 §4.1] order and multiplicity of the used entries: at most one base (entry 0), then deltas by increasing
/// `file_no`, then at most one dict, last; `upto_lsn` non-decreasing along base and delta; a dict's `upto_lsn` is 0.
/// Returns the greatest base or delta `upto_lsn`.
pub fn check_segment_set(segs: &[SegRef]) -> core::result::Result<Option<u64>, &'static str> {
    let mut stage = 0u8; // 0 before base, 1 after base, 2 in deltas, 3 after dict
    let mut last_delta = 0u32;
    let mut max_upto: Option<u64> = None;
    for (i, s) in segs.iter().enumerate() {
        if s.file_no == 0 {
            return Err("a used SegRef has file_no 0 [F04 §4.1]");
        }
        match s.kind {
            1 => {
                if i != 0 {
                    return Err("a base SegRef is not entry 0 [F04 §4.1]");
                }
                stage = 1;
            }
            2 => {
                if stage == 3 {
                    return Err("a delta SegRef follows the dict [F04 §4.1]");
                }
                if stage == 2 && s.file_no <= last_delta {
                    return Err("delta SegRefs are not in increasing file_no [F04 §4.1]");
                }
                last_delta = s.file_no;
                stage = 2;
            }
            3 => {
                if stage == 3 {
                    return Err("more than one dict SegRef [F04 §4.1]");
                }
                if s.upto_lsn != 0 {
                    return Err("a dict SegRef has a non-zero upto_lsn [F04 §4.1]");
                }
                stage = 3;
            }
            _ => return Err("a used SegRef has kind outside 1-3 [F04 §7]"),
        }
        if s.kind != 3 {
            if let Some(m) = max_upto
                && s.upto_lsn < m
            {
                return Err("upto_lsn decreases along the base and delta SegRefs [F04 §4.1]");
            }
            max_upto = Some(s.upto_lsn);
        }
    }
    Ok(max_upto)
}

/// `ImageCursor` ([F04 §4.2]), 10 bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct ImageCursor {
    /// Destination number; 0 = empty entry.
    pub dest: u8,
    /// Object format.
    pub algo: u8,
    /// Greatest exported `seq`.
    pub seq: u64,
}

/// `SeqRingEntry` ([F04 §4.3]), 16 bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct SeqRingEntry {
    /// Commit `seq`; 0 = empty.
    pub seq: u64,
    /// lsn of its `Commit` record.
    pub lsn: u64,
}

/// A `HeadSlot` ([F04 §3.1]), decoded after the checksum matched.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeadSlot {
    /// §5.2 flag bits 0–3.
    pub flags: u16,
    /// Publish counter.
    pub slot_seq: u64,
    /// Store epoch.
    pub epoch: u64,
    /// Visibility bound.
    pub committed_lsn: u64,
    /// End of the last flushed group.
    pub durable_lsn: u64,
    /// Boot identity; zero = none.
    pub boot_id: [u8; 16],
    /// Configuration generation.
    pub config_gen: u32,
    /// First unfolded lsn.
    pub checkpoint_lsn: u64,
    /// Newest covered commit `seq`.
    pub commit_seq: u64,
    /// Next `#N`.
    pub next_id: u32,
    /// Next `aN`.
    pub next_anchor: u32,
    /// Greatest fencing token.
    pub fence: u64,
    /// Oldest unretired extent.
    pub active_log: u32,
    /// Used entries of `segments`.
    pub n_segments: u8,
    /// All eight entries.
    pub segments: [SegRef; 8],
    /// Table pointers.
    pub refs_lsn: u64,
    /// `pins_lsn`.
    pub pins_lsn: u64,
    /// `heads_lsn`.
    pub heads_lsn: u64,
    /// `markers_lsn`.
    pub markers_lsn: u64,
    /// Export cursors.
    pub image_cursor: [ImageCursor; 4],
    /// The newest commits by `seq mod 32`.
    pub seq_ring: [SeqRingEntry; 32],
    /// The init-fixed parameters.
    pub init: InitParams,
    /// First group of the current epoch.
    pub epoch_lsn: u64,
    /// Sealed-file allocator.
    pub next_file_no: u32,
    /// Ref-id allocator.
    pub next_ref_id: u32,
    /// `project` root algorithm.
    pub project_oid_algo: u8,
    /// HLC sequence maximum.
    pub hlc_seq: u64,
    /// Greatest commit `hlc`.
    pub hlc_commit: u64,
}

/// The check of [F04 §7] a slot failed after its checksum matched, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fault {
    /// 3 (`format` other than 1), 4 (a reserved bit or byte, a non-zero empty entry) or 5 (a range or ordering).
    pub check: u8,
    /// The rule broken.
    pub reason: String,
}

impl Fault {
    fn new(check: u8, reason: impl Into<String>) -> Fault {
        Fault {
            check,
            reason: reason.into(),
        }
    }
}

impl core::fmt::Display for Fault {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "check {}: {}", self.check, self.reason)
    }
}

/// The classification of one slot ([F04 §7]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SlotClass {
    /// Checks 1–3: wrong magic, checksum mismatch, or `format` 0. Raw bytes kept.
    Absent(Vec<u8>),
    /// Checks 3–5 failed after a matching checksum: exit 7. The decoded fields are kept when the slot has `format` 1
    /// and every reserved byte zero, so the slot still re-encodes byte for byte ([`HeadSlot::encode`]); a slot of
    /// another format, or with a non-zero reserved byte, keeps none.
    Fatal(Fault, Option<Box<HeadSlot>>),
    /// Every check passed.
    Valid(Box<HeadSlot>),
}

/// The reserved regions of a slot ([F04 §3.1]) as (name, offset, width).
const RESERVED: [(&str, usize, usize); 4] = [
    ("_pad0", 97, 3),
    ("_reserved0", 916, 108),
    ("_pad1", 1073, 7),
    ("_reserved1", 1096, 2984),
];

impl HeadSlot {
    /// Classifies 4,096 bytes by [F04 §7], checks 1–6, in order. Every checksum-valid slot of `format` 1 has its fields
    /// decoded before checks 4 and 5.
    pub fn classify(b: &[u8], base: usize) -> Result<SlotClass> {
        if b.len() != SLOT_LEN {
            return err(base, "a HEAD slot is 4096 bytes [F04 §2]");
        }
        if &b[..4] != b"MOIR" {
            return Ok(SlotClass::Absent(b.to_vec()));
        }
        let (lo, hi) = xxh3_128(&b[..4080]);
        let mut r = Reader::with_base(&b[4080..], base + 4080);
        if r.u64()? != lo || r.u64()? != hi {
            return Ok(SlotClass::Absent(b.to_vec()));
        }
        let format = u16::from_le_bytes([b[4], b[5]]);
        if format == 0 {
            return Ok(SlotClass::Absent(b.to_vec()));
        }
        if let Err(e) = check_format(format, base + 4, "HEAD") {
            return Ok(SlotClass::Fatal(Fault::new(3, e.reason), None));
        }
        let s = Self::decode_fields(b, base)?;
        if let Some((name, off, _)) = RESERVED
            .iter()
            .find(|(_, off, w)| b[*off..off + w].iter().any(|&x| x != 0))
        {
            return Ok(SlotClass::Fatal(
                Fault::new(4, format!("HEAD.{name} at {off} is not zero")),
                None,
            ));
        }
        if s.flags & 0xFFF0 != 0 {
            return Ok(SlotClass::Fatal(
                Fault::new(4, "HEAD flags bits 4-15 are not zero"),
                Some(Box::new(s)),
            ));
        }
        match s.check_ranges() {
            Ok(()) => Ok(SlotClass::Valid(Box::new(s))),
            Err(f) => Ok(SlotClass::Fatal(f, Some(Box::new(s)))),
        }
    }

    /// Reads every field of a `format` 1 slot ([F04 §3.1]) without judging it; the reserved regions are skipped
    /// ([`RESERVED`]).
    fn decode_fields(b: &[u8], base: usize) -> Result<HeadSlot> {
        let mut r = Reader::with_base(b, base);
        r.bytes(6)?; // magic, format
        let flags = r.u16()?;
        let slot_seq = r.u64()?;
        let epoch = r.u64()?;
        let committed_lsn = r.u64()?;
        let durable_lsn = r.u64()?;
        let boot_id = r.b16()?;
        let config_gen = r.u32()?;
        let checkpoint_lsn = r.u64()?;
        let commit_seq = r.u64()?;
        let next_id = r.u32()?;
        let next_anchor = r.u32()?;
        let fence = r.u64()?;
        let active_log = r.u32()?;
        let n_segments = r.u8()?;
        r.bytes(3)?; // _pad0
        let mut segments = [SegRef::default(); 8];
        for s in &mut segments {
            *s = SegRef::decode(&mut r)?;
        }
        let refs_lsn = r.u64()?;
        let pins_lsn = r.u64()?;
        let heads_lsn = r.u64()?;
        let markers_lsn = r.u64()?;
        let mut image_cursor = [ImageCursor::default(); 4];
        for c in &mut image_cursor {
            *c = ImageCursor {
                dest: r.u8()?,
                algo: r.u8()?,
                seq: r.u64()?,
            };
        }
        let mut seq_ring = [SeqRingEntry::default(); 32];
        for e in &mut seq_ring {
            *e = SeqRingEntry {
                seq: r.u64()?,
                lsn: r.u64()?,
            };
        }
        r.bytes(108)?; // _reserved0
        let init = InitParams::decode(&mut r)?;
        let epoch_lsn = r.u64()?;
        let next_file_no = r.u32()?;
        let next_ref_id = r.u32()?;
        let project_oid_algo = r.u8()?;
        r.bytes(7)?; // _pad1
        let hlc_seq = r.u64()?;
        let hlc_commit = r.u64()?;
        r.bytes(2984)?; // _reserved1
        Ok(HeadSlot {
            flags,
            slot_seq,
            epoch,
            committed_lsn,
            durable_lsn,
            boot_id,
            config_gen,
            checkpoint_lsn,
            commit_seq,
            next_id,
            next_anchor,
            fence,
            active_log,
            n_segments,
            segments,
            refs_lsn,
            pins_lsn,
            heads_lsn,
            markers_lsn,
            image_cursor,
            seq_ring,
            init,
            epoch_lsn,
            next_file_no,
            next_ref_id,
            project_oid_algo,
            hlc_seq,
            hlc_commit,
        })
    }

    /// [F04 §7] checks 4 (the zero entries) and 5 (ranges and orderings); the reserved regions and flag bits are
    /// [`HeadSlot::classify`]'s.
    pub fn check_ranges(&self) -> core::result::Result<(), Fault> {
        let zero = |m: &str| Err(Fault::new(4, m));
        let range = |m: String| Err(Fault::new(5, m));
        let n = usize::from(self.n_segments);
        if n > 8 {
            return range("n_segments above 8".into());
        }
        if self.segments[n..].iter().any(|s| !s.is_zero()) {
            return zero("a segments entry at index >= n_segments is not zero");
        }
        for c in &self.image_cursor {
            if c.dest == 0 && *c != ImageCursor::default() {
                return zero("an empty image_cursor entry is not zero");
            }
        }
        for e in &self.seq_ring {
            if e.seq == 0 && *e != SeqRingEntry::default() {
                return zero("an empty seq_ring entry is not zero");
            }
        }
        if self.epoch == 0 {
            return range("epoch is 0".into());
        }
        if self.next_id == 0
            || self.next_anchor == 0
            || self.active_log == 0
            || self.next_file_no == 0
        {
            return range("next_id, next_anchor, active_log and next_file_no must be >= 1".into());
        }
        if let Err(m) = self.init.check() {
            return range(m.into());
        }
        if !(1..=2).contains(&self.project_oid_algo) {
            return range("project_oid_algo is not 1 or 2 [F17 §2.2 IP-2]".into());
        }
        let e = self.init.log_extent_bytes;
        if !self.epoch_lsn.is_multiple_of(e) {
            return range("epoch_lsn is not a multiple of E".into());
        }
        if !(self.epoch_lsn <= self.checkpoint_lsn
            && self.checkpoint_lsn <= self.durable_lsn
            && self.durable_lsn <= self.committed_lsn)
        {
            return range(
                "epoch_lsn <= checkpoint_lsn <= durable_lsn <= committed_lsn breaks [F04 §5.4]"
                    .into(),
            );
        }
        let active_start = u64::from(self.active_log - 1) * e;
        let strict_ok = active_start < self.checkpoint_lsn || self.checkpoint_lsn == self.epoch_lsn;
        if !(self.epoch_lsn <= active_start && active_start <= self.checkpoint_lsn && strict_ok) {
            return range(
                "epoch_lsn <= (active_log - 1) * E <= checkpoint_lsn breaks [F04 §5.8]".into(),
            );
        }
        let max_upto = match check_segment_set(&self.segments[..n]) {
            Ok(m) => m,
            Err(m) => return range(m.into()),
        };
        let want = max_upto.map_or(self.epoch_lsn, |m| m.max(self.epoch_lsn));
        if self.checkpoint_lsn != want {
            return range(
                "checkpoint_lsn differs from max(epoch_lsn, greatest base/delta upto_lsn)".into(),
            );
        }
        let used: Vec<_> = self.image_cursor.iter().filter(|c| c.dest != 0).collect();
        for (i, c) in used.iter().enumerate() {
            if !(1..=2).contains(&c.algo) {
                return range("an image_cursor entry has algo outside {1, 2}".into());
            }
            if used[..i]
                .iter()
                .any(|d| d.dest == c.dest && d.algo == c.algo)
            {
                return range("two image_cursor entries carry the same (dest, algo)".into());
            }
        }
        for (k, en) in self.seq_ring.iter().enumerate() {
            if en.seq != 0 && (en.seq % 32 != k as u64 || en.seq > self.commit_seq) {
                return range(format!(
                    "seq_ring entry {k} has seq {} out of place",
                    en.seq
                ));
            }
        }
        Ok(())
    }

    /// Re-encodes the 4,096-byte slot with its checksum.
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::new();
        w.bytes(b"MOIR");
        w.u16(1);
        w.u16(self.flags);
        w.u64(self.slot_seq);
        w.u64(self.epoch);
        w.u64(self.committed_lsn);
        w.u64(self.durable_lsn);
        w.bytes(&self.boot_id);
        w.u32(self.config_gen);
        w.u64(self.checkpoint_lsn);
        w.u64(self.commit_seq);
        w.u32(self.next_id);
        w.u32(self.next_anchor);
        w.u64(self.fence);
        w.u32(self.active_log);
        w.u8(self.n_segments);
        w.zeros(3);
        for s in &self.segments {
            s.encode(&mut w);
        }
        w.u64(self.refs_lsn);
        w.u64(self.pins_lsn);
        w.u64(self.heads_lsn);
        w.u64(self.markers_lsn);
        for c in &self.image_cursor {
            w.u8(c.dest);
            w.u8(c.algo);
            w.u64(c.seq);
        }
        for e in &self.seq_ring {
            w.u64(e.seq);
            w.u64(e.lsn);
        }
        w.zeros(108);
        self.init.encode(&mut w);
        w.u64(self.epoch_lsn);
        w.u32(self.next_file_no);
        w.u32(self.next_ref_id);
        w.u8(self.project_oid_algo);
        w.zeros(7);
        w.u64(self.hlc_seq);
        w.u64(self.hlc_commit);
        w.zeros(2984);
        debug_assert_eq!(w.len(), 4080);
        let (lo, hi) = xxh3_128(w.as_slice());
        w.u64(lo);
        w.u64(hi);
        w.into_vec()
    }
}

/// Which slot a process uses ([F04 §8.1]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    /// Slot A (offset 0).
    A,
    /// Slot B (offset 4,096).
    B,
}

/// Why no slot is used ([F04 §8.1]): each is exit 7, `NoValidSlot` after one more read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The slot failed a check of §7 after its checksum matched.
    Fatal(Choice, Fault),
    /// Both slots are valid with different init blocks ([F17 §2.2] IP-3).
    InitMismatch,
    /// Both slots are valid with equal `slot_seq` and different bytes.
    EqualSeq,
    /// No slot is valid: read again, then exit 7 naming `moirai repair`.
    NoValidSlot,
}

impl core::fmt::Display for Refusal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Refusal::Fatal(c, x) => write!(f, "slot {c:?}: {x}; exit 7 naming HEAD [F04 §7]"),
            Refusal::InitMismatch => {
                write!(
                    f,
                    "both slots valid with different init blocks [F17 §2.2 IP-3]"
                )
            }
            Refusal::EqualSeq => write!(
                f,
                "both slots valid with equal slot_seq and different bytes [F04 §8.1]"
            ),
            Refusal::NoValidSlot => {
                write!(f, "HEAD has no valid slot; run moirai repair [F04 §8.1]")
            }
        }
    }
}

/// The decoded `HEAD` file: both slot classifications.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeadFile {
    /// Slot A.
    pub a: SlotClass,
    /// Slot B.
    pub b: SlotClass,
}

impl HeadFile {
    /// Decodes 8,192 bytes ([F04 §2]).
    pub fn decode(b: &[u8]) -> Result<Self> {
        if b.len() != HEAD_LEN {
            return err(0, format!("HEAD is {} bytes, not 8192 [F04 §2]", b.len()));
        }
        Ok(HeadFile {
            a: HeadSlot::classify(&b[..SLOT_LEN], 0)?,
            b: HeadSlot::classify(&b[SLOT_LEN..], SLOT_LEN)?,
        })
    }

    /// Re-encodes: an absent slot keeps its raw bytes; a valid slot, and a fatal one whose fields were kept, is
    /// re-encoded. `None` when a fatal slot kept no fields.
    pub fn encode(&self) -> Option<Vec<u8>> {
        let mut v = Vec::with_capacity(HEAD_LEN);
        for s in [&self.a, &self.b] {
            match s {
                SlotClass::Absent(raw) => v.extend_from_slice(raw),
                SlotClass::Valid(h) | SlotClass::Fatal(_, Some(h)) => {
                    v.extend_from_slice(&h.encode())
                }
                SlotClass::Fatal(_, None) => return None,
            }
        }
        Some(v)
    }

    /// [F04 §8.1]: the slot to use, or why none is. The re-read rule of the last row is the caller's.
    pub fn choose(&self) -> core::result::Result<(Choice, &HeadSlot), Refusal> {
        match (&self.a, &self.b) {
            (SlotClass::Fatal(f, _), _) => Err(Refusal::Fatal(Choice::A, f.clone())),
            (_, SlotClass::Fatal(f, _)) => Err(Refusal::Fatal(Choice::B, f.clone())),
            (SlotClass::Valid(a), SlotClass::Valid(b)) => {
                if a.init != b.init || a.project_oid_algo != b.project_oid_algo {
                    return Err(Refusal::InitMismatch);
                }
                if a.slot_seq > b.slot_seq {
                    Ok((Choice::A, a))
                } else if b.slot_seq > a.slot_seq {
                    Ok((Choice::B, b))
                } else if a == b {
                    Ok((Choice::A, a))
                } else {
                    Err(Refusal::EqualSeq)
                }
            }
            (SlotClass::Valid(a), SlotClass::Absent(_)) => Ok((Choice::A, a)),
            (SlotClass::Absent(_), SlotClass::Valid(b)) => Ok((Choice::B, b)),
            (SlotClass::Absent(_), SlotClass::Absent(_)) => Err(Refusal::NoValidSlot),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A slot with [F04 §10]'s initial contents (E = 64 KiB, the test profile of [F17 §12]).
    pub(crate) fn initial_slot(slot_seq: u64) -> HeadSlot {
        HeadSlot {
            flags: 0,
            slot_seq,
            epoch: 0x1234_5678_9ABC_DEF1,
            committed_lsn: 138 + 200,
            durable_lsn: 138 + 200,
            boot_id: [3; 16],
            config_gen: 0,
            checkpoint_lsn: 0,
            commit_seq: 0,
            next_id: 1,
            next_anchor: 1,
            fence: 0,
            active_log: 1,
            n_segments: 0,
            segments: [SegRef::default(); 8],
            refs_lsn: 138 + 60,
            pins_lsn: 0,
            heads_lsn: 0,
            markers_lsn: 0,
            image_cursor: [ImageCursor::default(); 4],
            seq_ring: [SeqRingEntry::default(); 32],
            init: InitParams {
                log_extent_bytes: 1 << 16,
                hist_frame_commits: 4,
                hist_frame_bytes: 4096,
                store_id: [0xA5; 16],
            },
            epoch_lsn: 0,
            next_file_no: 1,
            next_ref_id: 1,
            project_oid_algo: 1,
            hlc_seq: 5 << 16,
            hlc_commit: 0,
        }
    }

    /// [F04 §3.2]: entry offsets; §3.1 the field offsets through a round trip.
    #[test]
    fn slot_offsets() {
        let mut s = initial_slot(1);
        s.segments[0] = SegRef {
            file_no: 7,
            kind: 1,
            upto_lsn: 0,
            blake3_16: [1; 16],
        };
        s.n_segments = 1;
        s.image_cursor[1] = ImageCursor {
            dest: 2,
            algo: 1,
            seq: 9,
        };
        s.seq_ring[3] = SeqRingEntry { seq: 0, lsn: 0 };
        let b = s.encode();
        assert_eq!(b.len(), 4096);
        assert_eq!(&b[0..4], b"MOIR");
        assert_eq!(&b[80..84], &1u32.to_le_bytes());
        assert_eq!(&b[100..104], &7u32.to_le_bytes());
        assert_eq!(b[104], 1);
        assert_eq!(b[364 + 10], 2);
        assert_eq!(&b[1024..1032], &(1u64 << 16).to_le_bytes());
        assert_eq!(b[1072], 1);
        assert!(matches!(HeadSlot::classify(&b, 0).unwrap(), SlotClass::Valid(v) if *v == s));
    }

    /// [F04 §10]: slot A has `slot_seq` 1 and slot B 2; B is chosen; a torn B falls back to A (§8.1 torn-slot rule).
    #[test]
    fn initial_head_and_torn_slot() {
        let mut f = initial_slot(1).encode();
        f.extend_from_slice(&initial_slot(2).encode());
        let h = HeadFile::decode(&f).unwrap();
        assert_eq!(h.choose().unwrap().0, Choice::B);
        assert_eq!(h.encode().unwrap(), f);
        let mut torn = f.clone();
        torn[4096 + 600] ^= 0xFF;
        let h = HeadFile::decode(&torn).unwrap();
        assert!(matches!(h.b, SlotClass::Absent(_)));
        assert_eq!(h.choose().unwrap().0, Choice::A);
        assert_eq!(h.encode().unwrap(), torn);
    }

    /// [F04 §7] check 4 and 5 failures are fatal, not absent; a fatal slot keeps its fields (and re-encodes) unless a
    /// reserved byte is set or its format is not 1.
    #[test]
    fn semantic_failures_are_fatal() {
        let fatal = |b: &[u8]| match HeadSlot::classify(b, 0).unwrap() {
            SlotClass::Fatal(f, fields) => (f.check, fields.map(|s| s.encode() == b)),
            other => panic!("{other:?}"),
        };
        let mut s = initial_slot(1);
        s.epoch = 0;
        assert_eq!(fatal(&s.encode()), (5, Some(true)));
        let mut s = initial_slot(1);
        s.init.store_id = [0; 16];
        assert_eq!(fatal(&s.encode()), (5, Some(true)));
        let mut s = initial_slot(1);
        s.seq_ring[1] = SeqRingEntry { seq: 2, lsn: 5 };
        assert_eq!(fatal(&s.encode()), (5, Some(true)));
        let mut s = initial_slot(1);
        s.flags = 1 << 5;
        assert_eq!(fatal(&s.encode()), (4, Some(true)));
        let mut s = initial_slot(1);
        s.seq_ring[1] = SeqRingEntry { seq: 0, lsn: 5 };
        assert_eq!(fatal(&s.encode()), (4, Some(true)));
        let reseal = |b: &mut Vec<u8>| {
            let (lo, hi) = xxh3_128(&b[..4080]);
            b[4080..4088].copy_from_slice(&lo.to_le_bytes());
            b[4088..].copy_from_slice(&hi.to_le_bytes());
        };
        let mut b = initial_slot(1).encode();
        b[2000] = 1;
        reseal(&mut b);
        assert_eq!(fatal(&b), (4, None));
        let mut b = initial_slot(1).encode();
        b[4] = 2;
        reseal(&mut b);
        assert_eq!(fatal(&b), (3, None));
        let mut f = initial_slot(1).encode();
        f.extend_from_slice(&b);
        assert!(matches!(
            HeadFile::decode(&f).unwrap().choose(),
            Err(Refusal::Fatal(Choice::B, Fault { check: 3, .. }))
        ));
    }

    /// [F04 §8.2]: the nine two-slot states select as the table says.
    #[test]
    fn nine_two_slot_states() {
        let old = |q| initial_slot(q).encode();
        let new = |q| {
            let mut s = initial_slot(q);
            s.committed_lsn += 100;
            s.durable_lsn += 100;
            s.encode()
        };
        let torn = vec![0u8; 4096];
        let pick = |a: &[u8], b: &[u8]| {
            let mut f = a.to_vec();
            f.extend_from_slice(b);
            HeadFile::decode(&f)
                .unwrap()
                .choose()
                .map(|(c, s)| (c, s.committed_lsn))
        };
        assert_eq!(pick(&old(1), &old(2)).unwrap(), (Choice::B, 338));
        assert_eq!(pick(&old(1), &new(2)).unwrap(), (Choice::B, 438));
        assert_eq!(pick(&old(1), &torn).unwrap(), (Choice::A, 338));
        assert_eq!(pick(&new(3), &old(2)).unwrap(), (Choice::A, 438));
        assert_eq!(pick(&new(3), &new(4)).unwrap(), (Choice::B, 438));
        assert_eq!(pick(&new(3), &torn).unwrap(), (Choice::A, 438));
        assert_eq!(pick(&torn, &old(2)).unwrap(), (Choice::B, 338));
        assert_eq!(pick(&torn, &new(2)).unwrap(), (Choice::B, 438));
        assert!(pick(&torn, &torn).is_err());
    }

    /// [F04 §4.1] segment-set order and multiplicity.
    #[test]
    fn segment_set_rules() {
        let r = |k, f, u| SegRef {
            file_no: f,
            kind: k,
            upto_lsn: u,
            blake3_16: [0; 16],
        };
        assert_eq!(
            check_segment_set(&[r(1, 1, 10), r(2, 2, 20), r(3, 3, 0)]),
            Ok(Some(20))
        );
        assert!(check_segment_set(&[r(2, 2, 20), r(1, 1, 10)]).is_err());
        assert!(check_segment_set(&[r(2, 3, 20), r(2, 2, 30)]).is_err());
        assert!(check_segment_set(&[r(1, 1, 30), r(2, 2, 20)]).is_err());
        assert!(check_segment_set(&[r(3, 1, 0), r(2, 2, 20)]).is_err());
        assert_eq!(check_segment_set(&[]), Ok(None));
    }
}

#[cfg(test)]
mod props {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// [F04 §7] checks 1–2: a change to any single byte of a valid slot makes it absent, never valid or fatal.
        #[test]
        fn torn_slot_is_absent(i in 0..SLOT_LEN, x in 1..=255u8) {
            let mut b = tests::initial_slot(1).encode();
            prop_assert!(matches!(HeadSlot::classify(&b, 0).unwrap(), SlotClass::Valid(_)));
            b[i] ^= x;
            prop_assert!(matches!(HeadSlot::classify(&b, 0).unwrap(), SlotClass::Absent(_)));
        }

        /// [F04 §7]: any 4,096 bytes classify without error; a valid slot re-encodes to the same bytes.
        #[test]
        fn classify_any_slot(tail in proptest::collection::vec(any::<u8>(), SLOT_LEN - 4)) {
            let mut b = b"MOIR".to_vec();
            b.extend_from_slice(&tail);
            let (lo, hi) = xxh3_128(&b[..4080]);
            b[4080..4088].copy_from_slice(&lo.to_le_bytes());
            b[4088..].copy_from_slice(&hi.to_le_bytes());
            if let SlotClass::Valid(s) = HeadSlot::classify(&b, 0).unwrap() {
                prop_assert_eq!(s.encode(), b);
            }
        }
    }
}
