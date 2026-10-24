//! The `HEAD` file's slot ([F04 §3]; u21 cut §3.7): `HeadSlot` of 4,096 bytes with its
//! `xxh3_128`, the slot checks of [F04 §7], the selection of [F04 §8.1] and the publish of
//! [F04 §9.1]: read both slots with one 8,192-byte `read_at`, select the newest valid slot S, build S′ from
//! it with the publish's changes and `slot_seq` + 1, and write S′ with one 4,096-byte `write_at` into the slot that
//! does not hold S. `HEAD` is never flushed on this path (1PC+C, [F04 §9.1]): the publish reaches the file only
//! through [`SlotFile`], which has no flush.
//!
//! - **`HeadSlot`** (§3.1): every field at its offset, byte-packed; `segments[k]` at 100 + 29k, `image_cursor[k]` at
//!   364 + 10k, `seq_ring[k]` at 404 + 16k (§3.2); `xxh3_128` over `[0, 4080)` at 4080 as (`low64`, `high64`), each
//!   little-endian ([F01 §7.2]), which is `u128::to_le_bytes`.
//! - **The checks** (§7): magic, checksum and format (absent or fatal), reserved bytes (check 4) and ranges (check 5,
//!   with [F17 §2.2] IP-2 for `init` and `project_oid_algo`).
//! - **The selection** (§8.1): a fatal slot is an error; of two valid slots the greater `slot_seq`, equal ones requiring
//!   identical bytes, and both requiring identical `init` and `project_oid_algo` (IP-3); one valid slot; no valid slot
//!   is an error (the probe does not re-read).

use moirai_vfs::StoreFs;
use xxhash_rust::xxh3::xxh3_128;

/// A slot's length ([F04 §2], [F04 §3.1]).
pub const SLOT_LEN: usize = 4_096;
/// `magic` ([F04 §3.1]).
pub const MAGIC: [u8; 4] = *b"KLTH";
/// `SegRef`'s length ([F04 §4.1]).
pub const SEG_REF_LEN: usize = 29;
/// The entries of `segments` ([F04 §3.1]).
pub const SEGMENTS_MAX: usize = 8;
/// Where `xxh3_128` sits ([F04 §3.1]).
pub const XXH3_AT: usize = 4_080;

/// `SegRef.kind` ([F04 §4.1]).
pub mod seg_kind {
    /// `base`.
    pub const BASE: u8 = 1;
    /// `delta`.
    pub const DELTA: u8 = 2;
    /// `dict`.
    pub const DICT: u8 = 3;
}

/// `HEAD.flags` ([F04 §5.2]).
pub mod flags {
    /// Bit 0 `quiet`.
    pub const QUIET: u16 = 1;
    /// Bit 1 `fts_tier2`.
    pub const FTS_TIER2: u16 = 2;
    /// Bits 4–15, reserved-zero.
    pub const RESERVED: u16 = 0xFFF0;
}

fn u16_le(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn u32_le(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().expect("4 bytes"))
}

fn u64_le(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().expect("8 bytes"))
}

/// `SegRef` ([F04 §4.1]): `file_no` `u32` at 0, `kind` `u8` at 4, `upto_lsn` `u64` at 5, `blake3_16`
/// at 13. A `Checkpoint` record embeds the same 29 bytes ([F05 §8.3]).
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct SegRef {
    /// `file_no`.
    pub file_no: u32,
    /// `kind`.
    pub kind: u8,
    /// `upto_lsn`.
    pub upto_lsn: u64,
    /// `blake3_16`: the first 16 bytes of the file's recorded digest.
    pub blake3_16: [u8; 16],
}

impl SegRef {
    /// The 29 bytes.
    pub fn encode(&self) -> [u8; SEG_REF_LEN] {
        let mut b = [0u8; SEG_REF_LEN];
        b[0..4].copy_from_slice(&self.file_no.to_le_bytes());
        b[4] = self.kind;
        b[5..13].copy_from_slice(&self.upto_lsn.to_le_bytes());
        b[13..29].copy_from_slice(&self.blake3_16);
        b
    }

    /// The fields of 29 bytes.
    pub fn decode(b: &[u8]) -> SegRef {
        SegRef {
            file_no: u32_le(b, 0),
            kind: b[4],
            upto_lsn: u64_le(b, 5),
            blake3_16: b[13..29].try_into().expect("16 bytes"),
        }
    }
}

/// One `HeadSlot` ([F04 §3.1]).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct HeadSlot {
    /// `format` (offset 4).
    pub format: u16,
    /// `flags` (6).
    pub flags: u16,
    /// `slot_seq` (8).
    pub slot_seq: u64,
    /// `epoch` (16).
    pub epoch: u64,
    /// `committed_lsn` (24).
    pub committed_lsn: u64,
    /// `durable_lsn` (32).
    pub durable_lsn: u64,
    /// `boot_id` (40).
    pub boot_id: [u8; 16],
    /// `config_gen` (56).
    pub config_gen: u32,
    /// `checkpoint_lsn` (60).
    pub checkpoint_lsn: u64,
    /// `commit_seq` (68).
    pub commit_seq: u64,
    /// `next_id` (76).
    pub next_id: u32,
    /// `next_anchor` (80).
    pub next_anchor: u32,
    /// `fence` (84).
    pub fence: u64,
    /// `active_log` (92).
    pub active_log: u32,
    /// `segments` (100), `n_segments` (96) of them.
    pub segments: Vec<SegRef>,
    /// `refs_lsn` (332).
    pub refs_lsn: u64,
    /// `pins_lsn` (340).
    pub pins_lsn: u64,
    /// `heads_lsn` (348).
    pub heads_lsn: u64,
    /// `markers_lsn` (356).
    pub markers_lsn: u64,
    /// `image_cursor` (364): (`dest`, `algo`, `seq`); `dest` 0 is empty.
    pub image_cursor: [(u8, u8, u64); 4],
    /// `seq_ring` (404): (`seq`, `lsn`); `seq` 0 is empty.
    pub seq_ring: [(u64, u64); 32],
    /// `init` (1024).
    pub init: [u8; 32],
    /// `epoch_lsn` (1056).
    pub epoch_lsn: u64,
    /// `next_file_no` (1064).
    pub next_file_no: u32,
    /// `next_ref_id` (1068).
    pub next_ref_id: u32,
    /// `project_oid_algo` (1072).
    pub project_oid_algo: u8,
    /// `hlc_seq` (1080).
    pub hlc_seq: u64,
    /// `hlc_commit` (1088).
    pub hlc_commit: u64,
}

impl HeadSlot {
    /// The 4,096 bytes with `magic` and `xxh3_128`; refuses more than 8 segments.
    pub fn encode(&self) -> Result<[u8; SLOT_LEN], String> {
        if self.segments.len() > SEGMENTS_MAX {
            return Err(format!("{} segments", self.segments.len()));
        }
        let mut b = [0u8; SLOT_LEN];
        b[0..4].copy_from_slice(&MAGIC);
        b[4..6].copy_from_slice(&self.format.to_le_bytes());
        b[6..8].copy_from_slice(&self.flags.to_le_bytes());
        b[8..16].copy_from_slice(&self.slot_seq.to_le_bytes());
        b[16..24].copy_from_slice(&self.epoch.to_le_bytes());
        b[24..32].copy_from_slice(&self.committed_lsn.to_le_bytes());
        b[32..40].copy_from_slice(&self.durable_lsn.to_le_bytes());
        b[40..56].copy_from_slice(&self.boot_id);
        b[56..60].copy_from_slice(&self.config_gen.to_le_bytes());
        b[60..68].copy_from_slice(&self.checkpoint_lsn.to_le_bytes());
        b[68..76].copy_from_slice(&self.commit_seq.to_le_bytes());
        b[76..80].copy_from_slice(&self.next_id.to_le_bytes());
        b[80..84].copy_from_slice(&self.next_anchor.to_le_bytes());
        b[84..92].copy_from_slice(&self.fence.to_le_bytes());
        b[92..96].copy_from_slice(&self.active_log.to_le_bytes());
        b[96] = self.segments.len() as u8;
        for (k, s) in self.segments.iter().enumerate() {
            let at = 100 + SEG_REF_LEN * k;
            b[at..at + SEG_REF_LEN].copy_from_slice(&s.encode());
        }
        b[332..340].copy_from_slice(&self.refs_lsn.to_le_bytes());
        b[340..348].copy_from_slice(&self.pins_lsn.to_le_bytes());
        b[348..356].copy_from_slice(&self.heads_lsn.to_le_bytes());
        b[356..364].copy_from_slice(&self.markers_lsn.to_le_bytes());
        for (k, &(dest, algo, seq)) in self.image_cursor.iter().enumerate() {
            let at = 364 + 10 * k;
            b[at] = dest;
            b[at + 1] = algo;
            b[at + 2..at + 10].copy_from_slice(&seq.to_le_bytes());
        }
        for (k, &(seq, lsn)) in self.seq_ring.iter().enumerate() {
            let at = 404 + 16 * k;
            b[at..at + 8].copy_from_slice(&seq.to_le_bytes());
            b[at + 8..at + 16].copy_from_slice(&lsn.to_le_bytes());
        }
        b[1024..1056].copy_from_slice(&self.init);
        b[1056..1064].copy_from_slice(&self.epoch_lsn.to_le_bytes());
        b[1064..1068].copy_from_slice(&self.next_file_no.to_le_bytes());
        b[1068..1072].copy_from_slice(&self.next_ref_id.to_le_bytes());
        b[1072] = self.project_oid_algo;
        b[1080..1088].copy_from_slice(&self.hlc_seq.to_le_bytes());
        b[1088..1096].copy_from_slice(&self.hlc_commit.to_le_bytes());
        let sum = xxh3_128(&b[..XXH3_AT]);
        b[XXH3_AT..].copy_from_slice(&sum.to_le_bytes());
        Ok(b)
    }

    /// The fields of a slot's bytes (no check; [`classify`] checks). `n_segments` above 8 reads 8 entries.
    pub fn decode(b: &[u8]) -> HeadSlot {
        let n = usize::from(b[96]).min(SEGMENTS_MAX);
        HeadSlot {
            format: u16_le(b, 4),
            flags: u16_le(b, 6),
            slot_seq: u64_le(b, 8),
            epoch: u64_le(b, 16),
            committed_lsn: u64_le(b, 24),
            durable_lsn: u64_le(b, 32),
            boot_id: b[40..56].try_into().expect("16 bytes"),
            config_gen: u32_le(b, 56),
            checkpoint_lsn: u64_le(b, 60),
            commit_seq: u64_le(b, 68),
            next_id: u32_le(b, 76),
            next_anchor: u32_le(b, 80),
            fence: u64_le(b, 84),
            active_log: u32_le(b, 92),
            segments: (0..n)
                .map(|k| SegRef::decode(&b[100 + SEG_REF_LEN * k..]))
                .collect(),
            refs_lsn: u64_le(b, 332),
            pins_lsn: u64_le(b, 340),
            heads_lsn: u64_le(b, 348),
            markers_lsn: u64_le(b, 356),
            image_cursor: std::array::from_fn(|k| {
                let at = 364 + 10 * k;
                (b[at], b[at + 1], u64_le(b, at + 2))
            }),
            seq_ring: std::array::from_fn(|k| {
                let at = 404 + 16 * k;
                (u64_le(b, at), u64_le(b, at + 8))
            }),
            init: b[1024..1056].try_into().expect("32 bytes"),
            epoch_lsn: u64_le(b, 1056),
            next_file_no: u32_le(b, 1064),
            next_ref_id: u32_le(b, 1068),
            project_oid_algo: b[1072],
            hlc_seq: u64_le(b, 1080),
            hlc_commit: u64_le(b, 1088),
        }
    }
}

/// What a slot is by [F04 §7]'s checks.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Class {
    /// Checks 1–3: never written, torn or damaged; or `format` 0.
    Absent,
    /// Check 3's `format` > 1, or checks 4–5: a defect; the store stops (exit 7).
    Fatal(String),
    /// Every check holds.
    Valid(Box<HeadSlot>),
}

/// [F17 §2.2] IP-2 on an `init` block ([F17 §2.1], [F17 §2.2]): `log_extent_bytes` a power of
/// two in [2^16, 2^30], `hist_frame_commits` 1–65,536, `hist_frame_bytes` 4,096–1,048,576, `store_id` not all zero.
fn ip2(init: &[u8; 32]) -> bool {
    let e = u64_le(init, 0);
    let (fc, fb) = (u32_le(init, 8), u32_le(init, 12));
    e.is_power_of_two()
        && (1 << 16..=1 << 30).contains(&e)
        && (1..=65_536).contains(&fc)
        && (4_096..=1_048_576).contains(&fb)
        && init[16..32].iter().any(|&x| x != 0)
}

/// Classifies one slot's bytes by [F04 §7]'s checks 1–5.
pub fn classify(b: &[u8]) -> Class {
    if b.len() != SLOT_LEN || b[0..4] != MAGIC {
        return Class::Absent;
    }
    if b[XXH3_AT..] != xxh3_128(&b[..XXH3_AT]).to_le_bytes() {
        return Class::Absent;
    }
    let s = HeadSlot::decode(b);
    match s.format {
        0 => return Class::Absent,
        1 => {}
        f => return Class::Fatal(format!("HEAD format {f}, above 1")),
    }
    let fatal = |why: &str| Class::Fatal(format!("HEAD slot: {why}"));
    // Check 4: reserved bytes, and the empty entries all zero.
    let n = usize::from(b[96]);
    let zero = |r: std::ops::Range<usize>| b[r].iter().all(|&x| x == 0);
    if s.flags & flags::RESERVED != 0
        || !zero(97..100)
        || !zero(1073..1080)
        || !zero(916..1024)
        || !zero(1096..XXH3_AT)
        || (n.min(SEGMENTS_MAX)..SEGMENTS_MAX)
            .any(|k| !zero(100 + SEG_REF_LEN * k..100 + SEG_REF_LEN * (k + 1)))
        || (0..4).any(|k| b[364 + 10 * k] == 0 && !zero(364 + 10 * k..374 + 10 * k))
        || (0..32).any(|k| s.seq_ring[k].0 == 0 && !zero(404 + 16 * k..420 + 16 * k))
    {
        return fatal("a reserved byte or an empty entry is not zero (check 4)");
    }
    // Check 5: ranges.
    if s.epoch == 0
        || n > SEGMENTS_MAX
        || s.next_id == 0
        || s.next_anchor == 0
        || s.active_log == 0
        || s.next_file_no == 0
    {
        return fatal("epoch, n_segments or a counter out of range (check 5)");
    }
    if !ip2(&s.init) || !matches!(s.project_oid_algo, 1 | 2) {
        return fatal("init fails IP-2 or project_oid_algo is not 1 or 2 (check 5)");
    }
    let e = u64_le(&s.init, 0);
    let al = u64::from(s.active_log - 1) * e;
    if !s.epoch_lsn.is_multiple_of(e)
        || !(s.epoch_lsn <= s.checkpoint_lsn
            && s.checkpoint_lsn <= s.durable_lsn
            && s.durable_lsn <= s.committed_lsn)
        || !(s.epoch_lsn <= al && al <= s.checkpoint_lsn)
        || al == s.checkpoint_lsn && s.checkpoint_lsn != s.epoch_lsn
    {
        return fatal("epoch_lsn or the orderings of §5.4 and §5.8 (check 5)");
    }
    let mut upto = 0u64;
    let mut prev_kind = 0u8;
    let mut prev_no = 0u32;
    for (k, r) in s.segments.iter().enumerate() {
        let order = match r.kind {
            seg_kind::BASE => k == 0,
            seg_kind::DELTA => {
                prev_kind != seg_kind::DICT && (prev_kind != seg_kind::DELTA || r.file_no > prev_no)
            }
            seg_kind::DICT => prev_kind != seg_kind::DICT,
            _ => false,
        };
        if !order || r.file_no == 0 {
            return fatal("a segments entry's kind, number or order (check 5)");
        }
        if r.kind != seg_kind::DICT {
            if r.upto_lsn < upto {
                return fatal("segments' upto_lsn decreases (check 5)");
            }
            upto = r.upto_lsn;
        }
        (prev_kind, prev_no) = (r.kind, r.file_no);
    }
    if s.checkpoint_lsn != s.epoch_lsn.max(upto) {
        return fatal(
            "checkpoint_lsn is not the greater of epoch_lsn and the set's upto_lsn (check 5)",
        );
    }
    let cursors: Vec<(u8, u8)> = s
        .image_cursor
        .iter()
        .filter(|c| c.0 != 0)
        .map(|c| (c.0, c.1))
        .collect();
    if cursors.iter().any(|c| !matches!(c.1, 1 | 2))
        || (1..cursors.len()).any(|i| cursors[..i].contains(&cursors[i]))
    {
        return fatal("an image_cursor entry (check 5)");
    }
    if s.seq_ring
        .iter()
        .enumerate()
        .any(|(k, r)| r.0 != 0 && (r.0 % 32 != k as u64 || r.0 > s.commit_seq))
    {
        return fatal("a seq_ring entry (check 5)");
    }
    Class::Valid(Box::new(s))
}

/// Selects the newest valid slot of `HEAD`'s 8,192 bytes by [F04 §8.1]: (0 for A or 1 for B,
/// the slot). A fatal slot, two valid slots with equal `slot_seq` and different bytes or with different `init` or
/// `project_oid_algo` (IP-3), and two absent slots are errors.
pub fn select(both: &[u8]) -> Result<(usize, HeadSlot), String> {
    if both.len() != 2 * SLOT_LEN {
        return Err(format!("HEAD of {} bytes, not 8,192", both.len()));
    }
    let (a, b) = (&both[..SLOT_LEN], &both[SLOT_LEN..]);
    match (classify(a), classify(b)) {
        (Class::Fatal(w), _) | (_, Class::Fatal(w)) => Err(w),
        (Class::Valid(x), Class::Valid(y)) => {
            let (x, y) = (*x, *y);
            if x.init != y.init || x.project_oid_algo != y.project_oid_algo {
                return Err(
                    "both slots valid with different init or project_oid_algo (IP-3)".into(),
                );
            }
            if x.slot_seq == y.slot_seq {
                return if a == b {
                    Ok((0, x))
                } else {
                    Err("two valid slots with one slot_seq and different bytes".into())
                };
            }
            Ok(if x.slot_seq > y.slot_seq {
                (0, x)
            } else {
                (1, y)
            })
        }
        (Class::Valid(x), Class::Absent) => Ok((0, *x)),
        (Class::Absent, Class::Valid(y)) => Ok((1, *y)),
        (Class::Absent, Class::Absent) => Err("HEAD has no valid slot".into()),
    }
}

/// `HEAD` as a publish reaches it: positional reads and writes, and no flush (1PC+C, [F04 §9.1]).
pub trait SlotFile {
    /// Reads up to `buf.len()` bytes at `off`.
    fn read_at(&mut self, off: u64, buf: &mut [u8]) -> Result<usize, String>;
    /// Writes `buf` at `off`.
    fn write_at(&mut self, off: u64, buf: &[u8]) -> Result<(), String>;
}

/// `HEAD` through the store seam: an open handle of it.
pub struct VfsSlot<'v, V: StoreFs> {
    /// The store seam.
    pub vfs: &'v V,
    /// `HEAD`'s handle.
    pub file: &'v V::File,
}

impl<V: StoreFs> SlotFile for VfsSlot<'_, V> {
    fn read_at(&mut self, off: u64, buf: &mut [u8]) -> Result<usize, String> {
        self.vfs
            .read_at(self.file, off, buf)
            .map_err(|e| e.to_string())
    }

    fn write_at(&mut self, off: u64, buf: &[u8]) -> Result<(), String> {
        self.vfs
            .write_at(self.file, off, buf)
            .map_err(|e| e.to_string())
    }
}

/// What a publish wrote.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Published {
    /// The slot written: 0 for A, 1 for B.
    pub slot: usize,
    /// S′.
    pub state: HeadSlot,
    /// S′'s bytes.
    pub bytes: [u8; SLOT_LEN],
}

/// A publish ([F04 §9.1]): one 8,192-byte `read_at`, the newest valid slot S by §8, S′ = S with
/// `change` applied and `slot_seq` + 1, `xxh3_128` recomputed, and one 4,096-byte `write_at` of S′ into the slot that
/// does not hold S. No flush.
pub fn publish<F: SlotFile>(
    f: &mut F,
    change: impl FnOnce(&mut HeadSlot),
) -> Result<Published, String> {
    let mut both = vec![0u8; 2 * SLOT_LEN];
    let got = f.read_at(0, &mut both)?;
    if got != both.len() {
        return Err(format!("HEAD read {got} bytes, not 8,192"));
    }
    let (s, cur) = select(&both)?;
    let mut next = cur.clone();
    change(&mut next);
    next.slot_seq = cur.slot_seq + 1;
    let bytes = next.encode()?;
    let other = 1 - s;
    f.write_at((SLOT_LEN * other) as u64, &bytes)?;
    Ok(Published {
        slot: other,
        state: next,
        bytes,
    })
}

/// `HEAD`'s two slots as one 8,192-byte image: `a` at 0, `b` at 4,096.
pub fn image(a: &HeadSlot, b: &HeadSlot) -> Result<Vec<u8>, String> {
    let mut v = Vec::with_capacity(2 * SLOT_LEN);
    v.extend_from_slice(&a.encode()?);
    v.extend_from_slice(&b.encode()?);
    Ok(v)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::tail::rec::InitParams;

    /// A valid slot of the test profile's E (2^16): the restored state of the full checkpoint's setup (u21 cut §3.7),
    /// a base folded up to E, the tail to E + 5,000.
    pub(crate) fn valid() -> HeadSlot {
        HeadSlot {
            format: 1,
            flags: flags::FTS_TIER2,
            slot_seq: 2,
            epoch: 0x1122_3344_5566_7788,
            committed_lsn: 70_536,
            durable_lsn: 70_536,
            checkpoint_lsn: 65_536,
            commit_seq: 1_040,
            next_id: 61,
            next_anchor: 1,
            fence: 1,
            active_log: 1,
            segments: vec![SegRef {
                file_no: 1,
                kind: seg_kind::BASE,
                upto_lsn: 65_536,
                blake3_16: [0x11; 16],
            }],
            init: InitParams {
                log_extent_bytes: 1 << 16,
                hist_frame_commits: 4,
                hist_frame_bytes: 4_096,
                store_id: [0x5A; 16],
            }
            .encode(),
            next_file_no: 14,
            next_ref_id: 50,
            project_oid_algo: 1,
            hlc_seq: 7 << 16,
            hlc_commit: 7 << 16,
            ..HeadSlot::default()
        }
    }

    /// `HeadSlot` ([F04 §3.1]–§3.2): each field at its offset, little-endian; `n_segments` at 96 and
    /// `segments[k]` at 100 + 29k (`SegRef`: `file_no`, `kind`, `upto_lsn`, `blake3_16`); `image_cursor[k]` at 364 + 10k;
    /// `seq_ring[k]` at 404 + 16k; `init` at 1024; `xxh3_128` at 4080 = XXH3-128 of `[0, 4080)` as (`low64`, `high64`);
    /// the fields read back.
    #[test]
    fn head_slot_fields_sit_at_f04_offsets() {
        let mut s = valid();
        s.boot_id = [0xB0; 16];
        s.config_gen = 0x0403_0201;
        s.segments.push(SegRef {
            file_no: 14,
            kind: seg_kind::DELTA,
            upto_lsn: 70_000,
            blake3_16: [0x22; 16],
        });
        s.checkpoint_lsn = 70_000;
        s.refs_lsn = 0x33;
        s.markers_lsn = 0x44;
        s.image_cursor[1] = (2, 1, 0x55);
        s.seq_ring[3] = (35, 0x66);
        let b = s.encode().unwrap();
        let sum = xxh3_128(&b[..4_080]);
        assert_eq!(
            &b[4_080..4_088],
            &(sum as u64).to_le_bytes(),
            "low64 at 4080"
        );
        assert_eq!(
            &b[4_088..4_096],
            &((sum >> 64) as u64).to_le_bytes(),
            "high64 at 4088"
        );
        assert_eq!(
            &b[0..8],
            &[b'K', b'L', b'T', b'H', 1, 0, 2, 0],
            "magic, format, flags"
        );
        assert_eq!(&b[8..16], &2u64.to_le_bytes(), "slot_seq at 8");
        assert_eq!(&b[16..24], &s.epoch.to_le_bytes(), "epoch at 16");
        assert_eq!(&b[24..32], &70_536u64.to_le_bytes(), "committed_lsn at 24");
        assert_eq!(&b[40..56], &[0xB0; 16], "boot_id at 40");
        assert_eq!(&b[56..60], &[1, 2, 3, 4], "config_gen at 56");
        assert_eq!(&b[60..68], &70_000u64.to_le_bytes(), "checkpoint_lsn at 60");
        assert_eq!(
            &b[76..84],
            &[61, 0, 0, 0, 1, 0, 0, 0],
            "next_id, next_anchor at 76, 80"
        );
        assert_eq!(
            &b[92..100],
            &[1, 0, 0, 0, 2, 0, 0, 0],
            "active_log, n_segments, _pad0"
        );
        let s1 = 100 + 29;
        assert_eq!(
            &b[s1..s1 + 5],
            &[14, 0, 0, 0, 2],
            "segments[1]: file_no, kind"
        );
        assert_eq!(
            &b[s1 + 5..s1 + 13],
            &70_000u64.to_le_bytes(),
            "upto_lsn at +5"
        );
        assert_eq!(&b[s1 + 13..s1 + 29], &[0x22; 16], "blake3_16 at +13");
        assert_eq!(&b[332..340], &0x33u64.to_le_bytes(), "refs_lsn");
        assert_eq!(&b[356..364], &0x44u64.to_le_bytes(), "markers_lsn");
        assert_eq!(
            &b[374..384],
            &[2, 1, 0x55, 0, 0, 0, 0, 0, 0, 0],
            "image_cursor[1] at 374"
        );
        assert_eq!(
            &b[452..468],
            &[[35u8, 0, 0, 0, 0, 0, 0, 0], [0x66, 0, 0, 0, 0, 0, 0, 0]].concat()[..]
        );
        assert_eq!(
            &b[1024..1032],
            &65_536u64.to_le_bytes(),
            "init.log_extent_bytes at 1024"
        );
        assert_eq!(
            &b[1064..1073],
            &[14, 0, 0, 0, 50, 0, 0, 0, 1],
            "next_file_no, next_ref_id, project_oid_algo"
        );
        assert_eq!(
            &b[1080..1088],
            &(7u64 << 16).to_le_bytes(),
            "hlc_seq at 1080"
        );
        assert_eq!(HeadSlot::decode(&b), s);
        assert_eq!(classify(&b), Class::Valid(Box::new(s)));
    }

    /// The slot checks ([F04 §7]): the restored state is valid; check 1–3 make a slot absent (magic,
    /// checksum, format 0) or fatal (format 2); check 4 and check 5 make it fatal, each on one field.
    #[test]
    fn slot_checks_follow_f04_7() {
        let good = valid();
        assert!(matches!(classify(&good.encode().unwrap()), Class::Valid(_)));
        let mut torn = good.encode().unwrap();
        torn[100] ^= 1;
        assert_eq!(classify(&torn), Class::Absent, "checksum");
        let mut magic = good.encode().unwrap();
        magic[0] = b'X';
        assert_eq!(classify(&magic), Class::Absent, "magic");
        let with = |f: &dyn Fn(&mut HeadSlot)| {
            let mut s = good.clone();
            f(&mut s);
            classify(&s.encode().unwrap())
        };
        assert_eq!(with(&|s| s.format = 0), Class::Absent, "format 0");
        assert!(
            matches!(with(&|s| s.format = 2), Class::Fatal(_)),
            "format 2"
        );
        let resealed = |at: usize| {
            let mut b = good.encode().unwrap();
            b[at] = 1;
            let sum = xxh3_128(&b[..4_080]);
            b[4_080..].copy_from_slice(&sum.to_le_bytes());
            classify(&b)
        };
        for at in [7, 98, 950, 1_075, 2_000, 4_079, 100 + 29 + 3] {
            assert!(
                matches!(resealed(at), Class::Fatal(_)),
                "check 4: byte {at}"
            );
        }
        type Change<'x> = &'x dyn Fn(&mut HeadSlot);
        let fatal: [(&str, Change<'_>); 13] = [
            ("epoch 0", &|s| s.epoch = 0),
            ("next_id 0", &|s| s.next_id = 0),
            ("next_anchor 0", &|s| s.next_anchor = 0),
            ("active_log 0", &|s| s.active_log = 0),
            ("next_file_no 0", &|s| s.next_file_no = 0),
            ("project_oid_algo 0", &|s| s.project_oid_algo = 0),
            ("store_id zero", &|s| {
                s.init[16..32].copy_from_slice(&[0; 16])
            }),
            ("E not a power of two", &|s| {
                s.init[0..8].copy_from_slice(&65_537u64.to_le_bytes())
            }),
            ("checkpoint_lsn above durable_lsn", &|s| {
                s.durable_lsn = 65_000
            }),
            ("checkpoint_lsn not the set's upto_lsn", &|s| {
                s.checkpoint_lsn = 65_000
            }),
            ("active_log past checkpoint_lsn", &|s| s.active_log = 3),
            ("a delta before the base", &|s| {
                s.segments.insert(
                    0,
                    SegRef {
                        file_no: 14,
                        kind: 2,
                        ..s.segments[0]
                    },
                )
            }),
            ("seq_ring entry in the wrong place", &|s| {
                s.seq_ring[4] = (35, 1)
            }),
        ];
        for (why, f) in fatal {
            assert!(matches!(with(f), Class::Fatal(_)), "check 5: {why}");
        }
        assert!(
            matches!(with(&|s| s.seq_ring[3] = (35, 1)), Class::Valid(_)),
            "a seq_ring entry in its place"
        );
    }

    /// The selection ([F04 §8.1]) row by row: a fatal slot either side is an error; of two valid
    /// slots the greater `slot_seq` (A newer, B newer); equal `slot_seq` with identical bytes is that state, with
    /// different bytes an error; different `init` (IP-3) an error; one valid slot is chosen; none is an error.
    #[test]
    fn select_follows_f04_8_1() {
        let at = |seq: u64| {
            let mut s = valid();
            s.slot_seq = seq;
            s
        };
        let fatal = {
            let mut s = at(9);
            s.epoch = 0;
            s.encode().unwrap()
        };
        let absent = [0u8; SLOT_LEN];
        let pair = |a: &[u8], b: &[u8]| [a, b].concat();
        let enc = |s: &HeadSlot| s.encode().unwrap();
        assert!(select(&pair(&fatal, &enc(&at(1)))).is_err(), "fatal, valid");
        assert!(select(&pair(&enc(&at(1)), &fatal)).is_err(), "valid, fatal");
        assert_eq!(
            select(&pair(&enc(&at(3)), &enc(&at(2)))),
            Ok((0, at(3))),
            "A newer"
        );
        assert_eq!(
            select(&pair(&enc(&at(2)), &enc(&at(3)))),
            Ok((1, at(3))),
            "B newer"
        );
        assert_eq!(
            select(&pair(&enc(&at(2)), &enc(&at(2)))),
            Ok((0, at(2))),
            "equal and identical"
        );
        let mut other = at(2);
        other.commit_seq += 1;
        assert!(
            select(&pair(&enc(&at(2)), &enc(&other))).is_err(),
            "equal, different"
        );
        let mut init = at(1);
        init.init[16] ^= 1;
        assert!(select(&pair(&enc(&at(2)), &enc(&init))).is_err(), "IP-3");
        assert_eq!(
            select(&pair(&enc(&at(4)), &absent)),
            Ok((0, at(4))),
            "valid, absent"
        );
        assert_eq!(
            select(&pair(&absent, &enc(&at(4)))),
            Ok((1, at(4))),
            "absent, valid"
        );
        assert!(select(&pair(&absent, &absent)).is_err(), "absent, absent");
    }

    /// A logged `HEAD`: its 8,192 bytes and every call, (read or write, offset, length).
    struct Logged {
        bytes: Vec<u8>,
        calls: Vec<(char, u64, usize)>,
    }

    impl SlotFile for Logged {
        fn read_at(&mut self, off: u64, buf: &mut [u8]) -> Result<usize, String> {
            self.calls.push(('r', off, buf.len()));
            let n = buf.len().min(self.bytes.len() - off as usize);
            buf[..n].copy_from_slice(&self.bytes[off as usize..off as usize + n]);
            Ok(n)
        }

        fn write_at(&mut self, off: u64, buf: &[u8]) -> Result<(), String> {
            self.calls.push(('w', off, buf.len()));
            self.bytes[off as usize..off as usize + buf.len()].copy_from_slice(buf);
            Ok(())
        }
    }

    /// The publish ([F04 §9.1]): one 8,192-byte read at 0, then exactly one 4,096-byte write into
    /// the slot that does not hold S (B when S is A, then A when S is B), the slot that holds S untouched; S′ is valid,
    /// S with the change and `slot_seq` + 1. The publish reaches `HEAD` only through [`SlotFile`], which has no flush;
    /// that the arm flushes nothing on `HEAD` is the timed-region test's (`m14`).
    #[test]
    fn publish_writes_the_other_slot_with_the_next_slot_seq() {
        let mut b = valid();
        b.slot_seq = 1;
        let mut f = Logged {
            bytes: image(&valid(), &b).unwrap(),
            calls: Vec::new(),
        };
        let a_before = f.bytes[..SLOT_LEN].to_vec();
        let p = publish(&mut f, |s| s.next_file_no = 16).unwrap();
        let Class::Valid(got) = classify(&f.bytes[SLOT_LEN..]) else {
            panic!(
                "slot B absent after the publish: {:?}",
                classify(&f.bytes[SLOT_LEN..])
            );
        };
        assert_eq!(got.slot_seq, 3, "slot_seq {}, want 3", got.slot_seq);
        assert_eq!(f.calls, [('r', 0, 8_192), ('w', 4_096, 4_096)]);
        assert_eq!(
            &f.bytes[..SLOT_LEN],
            &a_before[..],
            "slot A, which holds S, untouched"
        );
        let mut want = valid();
        want.slot_seq = 3;
        want.next_file_no = 16;
        assert_eq!((p.slot, &p.state, *got), (1, &want, want.clone()));
        assert_eq!(&p.bytes[..], &f.bytes[SLOT_LEN..]);
        // Now S is B: the next publish writes A.
        f.calls.clear();
        let b_before = f.bytes[SLOT_LEN..].to_vec();
        let p = publish(&mut f, |s| s.commit_seq += 1).unwrap();
        assert_eq!(f.calls, [('r', 0, 8_192), ('w', 0, 4_096)]);
        assert_eq!(&f.bytes[SLOT_LEN..], &b_before[..]);
        assert_eq!((p.slot, p.state.slot_seq), (0, 4));
        assert_eq!(select(&f.bytes), Ok((0, p.state)));
    }
}
