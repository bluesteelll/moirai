//! The `HEAD` file ([F04]): two 4 KiB slots in the G18 layout, their validity (§7), the choice of a slot (§8) and the
//! bytes a publish writes (§9). The toy writes the product's slot byte for byte.

use crate::codec::hash128;
use crate::format::{Counters, InitParams, SegRef};

/// The length of `HEAD` ([F04 §2]).
pub const HEAD_LEN: usize = 8192;
/// The length of one slot.
pub const SLOT_LEN: usize = 4096;
/// `"MOIR"`.
pub const MAGIC: [u8; 4] = *b"MOIR";

/// `flags` bit 0: quiet mode.
pub const FLAG_QUIET: u16 = 1 << 0;
/// `flags` bit 1: FTS tier 2.
pub const FLAG_FTS: u16 = 1 << 1;
/// `flags` bit 2: read-only.
pub const FLAG_READONLY: u16 = 1 << 2;
/// `flags` bit 3: retired.
pub const FLAG_RETIRED: u16 = 1 << 3;

/// One `HeadSlot` ([F04 §3.1]), decoded.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Slot {
    /// `format`.
    pub format: u16,
    /// `flags`.
    pub flags: u16,
    /// `slot_seq`.
    pub slot_seq: u64,
    /// `epoch`.
    pub epoch: u64,
    /// `committed_lsn`.
    pub committed_lsn: u64,
    /// `durable_lsn`.
    pub durable_lsn: u64,
    /// `boot_id`.
    pub boot_id: [u8; 16],
    /// `config_gen`.
    pub config_gen: u32,
    /// `checkpoint_lsn`.
    pub checkpoint_lsn: u64,
    /// The log-derived counters (`commit_seq`, `next_id`, `next_anchor`, `fence`, `next_file_no`, `next_ref_id`,
    /// `hlc_seq`, `hlc_commit`).
    pub counters: Counters,
    /// `active_log`.
    pub active_log: u32,
    /// `segments[0 .. n_segments)`.
    pub segments: Vec<SegRef>,
    /// `refs_lsn`.
    pub refs_lsn: u64,
    /// `pins_lsn`.
    pub pins_lsn: u64,
    /// `heads_lsn`.
    pub heads_lsn: u64,
    /// `markers_lsn`.
    pub markers_lsn: u64,
    /// `seq_ring`: (seq, lsn) per `seq mod 32`.
    pub seq_ring: [(u64, u64); 32],
    /// `init`.
    pub init: InitParams,
    /// `epoch_lsn`.
    pub epoch_lsn: u64,
    /// `project_oid_algo`.
    pub project_oid_algo: u8,
}

/// How a slot reads ([F04 §7]).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SlotRead {
    /// Magic or checksum mismatch, or `format` 0: torn, never written, damaged.
    Absent,
    /// Passes its checksum but fails a check of §7 items 3–5: a writer defect; exit 7.
    Fatal(&'static str),
    /// Valid.
    Valid(Box<Slot>),
}

impl Slot {
    /// The 4,096 bytes of the slot, with its checksum.
    pub fn to_bytes(&self) -> [u8; SLOT_LEN] {
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
        let c = &self.counters;
        b[68..76].copy_from_slice(&c.commit_seq.to_le_bytes());
        b[76..80].copy_from_slice(&c.next_id.to_le_bytes());
        b[80..84].copy_from_slice(&c.next_anchor.to_le_bytes());
        b[84..92].copy_from_slice(&c.fence.to_le_bytes());
        b[92..96].copy_from_slice(&self.active_log.to_le_bytes());
        b[96] = self.segments.len() as u8;
        for (k, s) in self.segments.iter().enumerate().take(8) {
            let at = 100 + 29 * k;
            b[at..at + 29].copy_from_slice(&s.to_bytes());
        }
        b[332..340].copy_from_slice(&self.refs_lsn.to_le_bytes());
        b[340..348].copy_from_slice(&self.pins_lsn.to_le_bytes());
        b[348..356].copy_from_slice(&self.heads_lsn.to_le_bytes());
        b[356..364].copy_from_slice(&self.markers_lsn.to_le_bytes());
        for (k, &(seq, lsn)) in self.seq_ring.iter().enumerate() {
            let at = 404 + 16 * k;
            b[at..at + 8].copy_from_slice(&seq.to_le_bytes());
            b[at + 8..at + 16].copy_from_slice(&lsn.to_le_bytes());
        }
        b[1024..1056].copy_from_slice(&self.init.to_bytes());
        b[1056..1064].copy_from_slice(&self.epoch_lsn.to_le_bytes());
        b[1064..1068].copy_from_slice(&c.next_file_no.to_le_bytes());
        b[1068..1072].copy_from_slice(&c.next_ref_id.to_le_bytes());
        b[1072] = self.project_oid_algo;
        b[1080..1088].copy_from_slice(&c.hlc_seq.to_le_bytes());
        b[1088..1096].copy_from_slice(&c.hlc_commit.to_le_bytes());
        let (lo, hi) = hash128(&b[..4080]);
        b[4080..4088].copy_from_slice(&lo.to_le_bytes());
        b[4088..4096].copy_from_slice(&hi.to_le_bytes());
        b
    }

    /// Classifies and decodes a slot's 4,096 bytes by [F04 §7].
    pub fn read(b: &[u8]) -> SlotRead {
        if b.len() < SLOT_LEN || b[0..4] != MAGIC {
            return SlotRead::Absent;
        }
        let (lo, hi) = hash128(&b[..4080]);
        if u64_le(b, 4080) != lo || u64_le(b, 4088) != hi {
            return SlotRead::Absent;
        }
        let format = u16::from_le_bytes([b[4], b[5]]);
        if format == 0 {
            return SlotRead::Absent;
        }
        if format > 1 {
            return SlotRead::Fatal("HEAD slot of a later format version");
        }
        let flags = u16::from_le_bytes([b[6], b[7]]);
        let n = b[96] as usize;
        // Check 4: reserved bytes.
        if flags & 0xFFF0 != 0
            || b[97..100].iter().any(|&x| x != 0)
            || b[916..1024].iter().any(|&x| x != 0)
            || b[1073..1080].iter().any(|&x| x != 0)
            || b[1096..4080].iter().any(|&x| x != 0)
            || n > 8
            || b[100 + 29 * n..332].iter().any(|&x| x != 0)
            || b[364..404].iter().any(|&x| x != 0)
        {
            return SlotRead::Fatal("HEAD slot with a non-zero reserved byte");
        }
        let mut segments = Vec::with_capacity(n);
        for k in 0..n {
            let mut s = [0u8; 29];
            s.copy_from_slice(&b[100 + 29 * k..129 + 29 * k]);
            segments.push(SegRef::from_bytes(&s));
        }
        let mut seq_ring = [(0u64, 0u64); 32];
        for (k, e) in seq_ring.iter_mut().enumerate() {
            *e = (u64_le(b, 404 + 16 * k), u64_le(b, 412 + 16 * k));
        }
        let mut init = [0u8; 32];
        init.copy_from_slice(&b[1024..1056]);
        let mut boot_id = [0u8; 16];
        boot_id.copy_from_slice(&b[40..56]);
        let s = Slot {
            format,
            flags,
            slot_seq: u64_le(b, 8),
            epoch: u64_le(b, 16),
            committed_lsn: u64_le(b, 24),
            durable_lsn: u64_le(b, 32),
            boot_id,
            config_gen: u32_le(b, 56),
            checkpoint_lsn: u64_le(b, 60),
            counters: Counters {
                commit_seq: u64_le(b, 68),
                next_id: u32_le(b, 76),
                next_anchor: u32_le(b, 80),
                fence: u64_le(b, 84),
                next_file_no: u32_le(b, 1064),
                next_ref_id: u32_le(b, 1068),
                hlc_seq: u64_le(b, 1080),
                hlc_commit: u64_le(b, 1088),
            },
            active_log: u32_le(b, 92),
            segments,
            refs_lsn: u64_le(b, 332),
            pins_lsn: u64_le(b, 340),
            heads_lsn: u64_le(b, 348),
            markers_lsn: u64_le(b, 356),
            seq_ring,
            init: InitParams::from_bytes(&init),
            epoch_lsn: u64_le(b, 1056),
            project_oid_algo: b[1072],
        };
        match s.range_problem() {
            Some(p) => SlotRead::Fatal(p),
            None => SlotRead::Valid(Box::new(s)),
        }
    }

    /// Check 5 of [F04 §7]: the ranges and orderings.
    fn range_problem(&self) -> Option<&'static str> {
        let c = &self.counters;
        if self.epoch == 0 || c.next_id == 0 || c.next_anchor == 0 || self.active_log == 0 {
            return Some("HEAD slot with a zero epoch or counter");
        }
        if c.next_file_no == 0 || !self.init.valid() || !matches!(self.project_oid_algo, 1 | 2) {
            return Some("HEAD slot with an invalid init block");
        }
        let e = self.init.log_extent_bytes;
        if !self.epoch_lsn.is_multiple_of(e)
            || self.epoch_lsn > self.checkpoint_lsn
            || self.checkpoint_lsn > self.durable_lsn
            || self.durable_lsn > self.committed_lsn
        {
            return Some("HEAD slot whose log bounds are out of order");
        }
        let active_start = u64::from(self.active_log - 1).saturating_mul(e);
        if self.epoch_lsn > active_start
            || active_start > self.checkpoint_lsn
            || (active_start == self.checkpoint_lsn && self.checkpoint_lsn != self.epoch_lsn)
        {
            return Some("HEAD slot whose active_log is out of range");
        }
        // [F04 §4.1] order and multiplicity: at most one base, entry 0 when present; then the deltas, oldest first (file
        // numbers increasing); then at most one dict, last. upto_lsn does not decrease along the base and delta entries.
        let mut upto = self.epoch_lsn;
        let mut last_delta = 0u32;
        for (k, s) in self.segments.iter().enumerate() {
            let dict_before = self.segments[..k].iter().any(|x| x.kind == 3);
            if !(1..=3).contains(&s.kind)
                || s.file_no == 0
                || (s.kind == 1 && k != 0)
                || dict_before
                || (s.kind == 2 && s.file_no <= last_delta)
            {
                return Some("HEAD slot with an invalid segment entry");
            }
            if s.kind == 2 {
                last_delta = s.file_no;
            }
            if s.kind != 3 {
                if s.upto_lsn < upto {
                    return Some("HEAD slot whose segments go backwards");
                }
                upto = s.upto_lsn;
            }
        }
        if upto != self.checkpoint_lsn {
            return Some("HEAD slot whose checkpoint_lsn differs from its segments");
        }
        // Check 4: an empty seq_ring entry (seq 0) is all zero; check 5: a non-empty entry k has seq mod 32 = k and seq
        // at most commit_seq.
        for (k, &(seq, lsn)) in self.seq_ring.iter().enumerate() {
            if (seq == 0 && lsn != 0) || (seq != 0 && (seq % 32 != k as u64 || seq > c.commit_seq))
            {
                return Some("HEAD slot with an invalid seq_ring entry");
            }
        }
        None
    }
}

fn u64_le(b: &[u8], at: usize) -> u64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&b[at..at + 8]);
    u64::from_le_bytes(a)
}

fn u32_le(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

/// The choice of a slot ([F04 §8.1]).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Choice {
    /// The newest valid slot, and which slot (0 = A, 1 = B) holds it.
    Newest(Box<Slot>, usize),
    /// Both slots absent: read again (at most twice more), then exit 7 for `repair`.
    NoneValid,
    /// A fatal slot, or two valid slots with equal `slot_seq` and different bytes, or different `init` blocks: exit 7.
    Fatal(&'static str),
}

/// Chooses a slot from the 8,192 bytes of `HEAD` by [F04 §8.1]. `skip_fatal` is P-61's seeded bug: a fatal slot is
/// treated as absent and the other slot is used.
pub fn choose(head: &[u8], skip_fatal: bool) -> Choice {
    if head.len() < HEAD_LEN {
        return Choice::NoneValid;
    }
    let a = Slot::read(&head[..SLOT_LEN]);
    let b = Slot::read(&head[SLOT_LEN..HEAD_LEN]);
    let fix = |r: SlotRead| match r {
        SlotRead::Fatal(_) if skip_fatal => SlotRead::Absent,
        other => other,
    };
    let (a, b) = (fix(a), fix(b));
    match (a, b) {
        (SlotRead::Fatal(m), _) | (_, SlotRead::Fatal(m)) => Choice::Fatal(m),
        (SlotRead::Valid(x), SlotRead::Valid(y)) => {
            if x.init != y.init || x.project_oid_algo != y.project_oid_algo {
                return Choice::Fatal("HEAD slots with different init blocks");
            }
            if x.slot_seq == y.slot_seq {
                if head[..SLOT_LEN] == head[SLOT_LEN..HEAD_LEN] {
                    Choice::Newest(x, 0)
                } else {
                    Choice::Fatal("HEAD slots with equal slot_seq and different bytes")
                }
            } else if x.slot_seq > y.slot_seq {
                Choice::Newest(x, 0)
            } else {
                Choice::Newest(y, 1)
            }
        }
        (SlotRead::Valid(x), SlotRead::Absent) => Choice::Newest(x, 0),
        (SlotRead::Absent, SlotRead::Valid(y)) => Choice::Newest(y, 1),
        (SlotRead::Absent, SlotRead::Absent) => Choice::NoneValid,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use proptest::prelude::*;

    pub(crate) fn sample() -> Slot {
        Slot {
            format: 1,
            flags: 0,
            slot_seq: 3,
            epoch: 0x1234,
            committed_lsn: 400,
            durable_lsn: 300,
            boot_id: [5; 16],
            config_gen: 0,
            checkpoint_lsn: 0,
            counters: Counters {
                commit_seq: 33,
                ..Counters::EMPTY
            },
            active_log: 1,
            segments: Vec::new(),
            refs_lsn: 138,
            pins_lsn: 0,
            heads_lsn: 0,
            markers_lsn: 0,
            seq_ring: {
                let mut r = [(0, 0); 32];
                r[1] = (33, 200);
                r
            },
            init: InitParams {
                log_extent_bytes: 1 << 16,
                hist_frame_commits: 4,
                hist_frame_bytes: 4096,
                store_id: [7; 16],
            },
            epoch_lsn: 0,
            project_oid_algo: 1,
        }
    }

    #[test]
    fn slots_round_trip_at_the_frozen_offsets() {
        let s = sample();
        let b = s.to_bytes();
        assert_eq!(&b[..4], b"MOIR");
        assert_eq!(u64_le(&b, 8), 3);
        assert_eq!(u64_le(&b, 32), 300);
        assert_eq!(u64_le(&b, 1056), 0);
        assert_eq!(Slot::read(&b), SlotRead::Valid(Box::new(s.clone())));
        // A torn slot is absent; a checksummed slot with a bad field is fatal.
        let mut torn = b;
        torn[100] ^= 0xFF;
        assert_eq!(Slot::read(&torn), SlotRead::Absent);
        let mut bad = s.clone();
        bad.durable_lsn = 500;
        assert!(matches!(Slot::read(&bad.to_bytes()), SlotRead::Fatal(_)));
    }

    #[test]
    fn the_newest_valid_slot_is_chosen_and_a_fatal_slot_stops() {
        let a = sample();
        let mut b = sample();
        b.slot_seq = 4;
        let mut head = vec![0u8; HEAD_LEN];
        head[..SLOT_LEN].copy_from_slice(&a.to_bytes());
        head[SLOT_LEN..].copy_from_slice(&b.to_bytes());
        assert!(matches!(choose(&head, false), Choice::Newest(s, 1) if s.slot_seq == 4));
        head[SLOT_LEN + 5] ^= 1;
        assert!(matches!(choose(&head, false), Choice::Newest(s, 0) if s.slot_seq == 3));
        let mut bad = b.clone();
        bad.durable_lsn = 999;
        head[SLOT_LEN..].copy_from_slice(&bad.to_bytes());
        assert!(matches!(choose(&head, false), Choice::Fatal(_)));
        // P-61's seeded bug falls back to the other slot.
        assert!(matches!(choose(&head, true), Choice::Newest(s, 0) if s.slot_seq == 3));
        assert_eq!(choose(&[0u8; HEAD_LEN], false), Choice::NoneValid);
    }

    fn seg(file_no: u32, kind: u8, upto_lsn: u64) -> SegRef {
        SegRef {
            file_no,
            kind,
            upto_lsn,
            digest: [0; 16],
        }
    }

    fn fatal(s: &Slot) -> bool {
        matches!(Slot::read(&s.to_bytes()), SlotRead::Fatal(_))
    }

    #[test]
    fn an_empty_seq_ring_entry_is_all_zero() {
        let mut s = sample();
        assert!(!fatal(&s));
        s.seq_ring[5] = (0, 1234);
        assert!(fatal(&s), "check 4: an empty entry with an lsn");
        let mut s = sample();
        s.seq_ring[2] = (33, 10);
        assert!(fatal(&s), "check 5: seq mod 32 differs from the index");
        let mut s = sample();
        s.seq_ring[2] = (34, 10);
        assert!(fatal(&s), "check 5: seq above commit_seq");
    }

    #[test]
    fn segment_entries_follow_their_order_and_multiplicity() {
        let with = |segs: Vec<SegRef>| {
            let mut s = sample();
            s.checkpoint_lsn = segs
                .iter()
                .filter(|x| x.kind != 3)
                .map(|x| x.upto_lsn)
                .max()
                .unwrap_or(0);
            s.durable_lsn = s.durable_lsn.max(s.checkpoint_lsn);
            s.committed_lsn = s.committed_lsn.max(s.durable_lsn);
            s.segments = segs;
            s
        };
        assert!(!fatal(&with(vec![
            seg(4, 1, 100),
            seg(6, 2, 150),
            seg(7, 2, 200),
            seg(9, 3, 0)
        ])));
        assert!(!fatal(&with(vec![seg(6, 2, 150)])));
        // A second base, a base after a delta, deltas out of order or repeated, a dict before the end, two dicts.
        assert!(fatal(&with(vec![seg(4, 1, 100), seg(5, 1, 150)])));
        assert!(fatal(&with(vec![seg(6, 2, 100), seg(4, 1, 150)])));
        assert!(fatal(&with(vec![seg(7, 2, 100), seg(6, 2, 150)])));
        assert!(fatal(&with(vec![seg(6, 2, 100), seg(6, 2, 150)])));
        assert!(fatal(&with(vec![seg(9, 3, 0), seg(6, 2, 150)])));
        assert!(fatal(&with(vec![
            seg(4, 1, 100),
            seg(9, 3, 0),
            seg(10, 3, 0)
        ])));
        // upto_lsn going backwards, and a checkpoint_lsn other than the greatest upto_lsn.
        assert!(fatal(&with(vec![seg(4, 1, 200), seg(6, 2, 150)])));
        let mut s = with(vec![seg(4, 1, 100)]);
        s.checkpoint_lsn = 50;
        assert!(fatal(&s));
    }

    proptest! {
        #![proptest_config(crate::testing::proptest_config())]

        #[test]
        fn reading_garbage_never_panics(b in proptest::collection::vec(any::<u8>(), 0..SLOT_LEN + 8)) {
            let _ = Slot::read(&b);
        }

        #[test]
        fn valid_slots_round_trip(seq in 1u64.., committed in 0u64..1 << 40, cgen in any::<u32>()) {
            let mut s = sample();
            s.slot_seq = seq;
            s.committed_lsn = committed.max(300);
            s.config_gen = cgen;
            prop_assert_eq!(Slot::read(&s.to_bytes()), SlotRead::Valid(Box::new(s)));
        }
    }
}
