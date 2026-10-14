//! The `Checkpoint` record of a delta checkpoint ([F05 §9.9], `05-log.md:728-807`; u21 cut §3.7): kind 9, durable, its
//! payload with bits 0 (`set_change`), 1 (`window`) and 5 (`files_added`), the record forms of `SegRef` ([F04 §4.1],
//! embedded, `05-log.md:490`) and `FileEntry` ([F05 §8.4], `:492-505`), and its one-record group, framed by
//! [`super::rec::raw_group`]: kind 9 is outside the encoder's and the scan's subset (`tail/mod.rs`), which refuse it.
//!
//! The payload's fields in order (`05-log.md:734-754`): `ckflags` `u16`; `append_hlc` `hlc` (`u64`); `next_file_no`
//! `uvar32`; with bit 0 `upto_lsn` (`lsnv`), `active_log` (`uvar32`), `n_segments` (`u8`) and the `SegRef`s; with bit 1
//! `window_start` (`lsnv`), `n_refs` (`uvar32`) and the `RefList`s, each `ref_id`, `n` and n × (`dlsn`, `dhlc`), both
//! `uvar64` (`:771-777`); with bit 5 `n_added` (`uvar32`) and the `FileEntry`s. Bits 2, 3, 4 and 6 (a runtime-only
//! fold, promotions, retirements, released files) are outside the probe's subset; bits 10–15 are reserved-zero.

use super::enc::{Reader, put_uvar};
use super::head::{SEG_REF_LEN, SegRef};
use super::rec::{RecHdr, chain, raw_group, record_checksum};
use super::{REC_HDR_LEN, TRAILER_LEN, flag};

/// The record kind ([F05 §7], `05-log.md:381`).
pub const KIND: u8 = 9;

/// The `ckflags` bits ([F05 §9.9], `05-log.md:756-769`).
pub mod ckflag {
    /// Bit 0 `set_change`: fields 4–7.
    pub const SET_CHANGE: u16 = 1 << 0;
    /// Bit 1 `window`: fields 8–10.
    pub const WINDOW: u16 = 1 << 1;
    /// Bit 2 `runtime_fold`.
    pub const RUNTIME_FOLD: u16 = 1 << 2;
    /// Bit 3 `promotions`.
    pub const PROMOTIONS: u16 = 1 << 3;
    /// Bit 4 `retirements`.
    pub const RETIREMENTS: u16 = 1 << 4;
    /// Bit 5 `files_added`: fields 16–17.
    pub const FILES_ADDED: u16 = 1 << 5;
    /// Bit 6 `files_released`.
    pub const FILES_RELEASED: u16 = 1 << 6;
    /// Bit 7 `fts_tier2`.
    pub const FTS_TIER2: u16 = 1 << 7;
    /// Bits 10–15, reserved-zero.
    pub const RESERVED: u16 = 0xFC00;
}

/// `FileEntry` ([F05 §8.4], `05-log.md:498-505`): `FileRefV` (`family` `u8`, `ref_id` `refid` for family 5,
/// `file_no` `uvar32`), then `total_len` `uvar64` and `digest` `b16`.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct FileEntry {
    /// The family ([F11 §2.5]).
    pub family: u8,
    /// The `<ref_id>` of a `seg.b<ref_id>.<K>` (family 5); 0 otherwise and not written.
    pub ref_id: u32,
    /// The file number, ≥ 1.
    pub file_no: u32,
    /// The file's `total_len`.
    pub total_len: u64,
    /// The first 16 bytes of the digest its header records.
    pub digest: [u8; 16],
}

/// A ref's commits in the window ([F05 §9.9] `RefList`, `05-log.md:771-777`): (lsn, `append_hlc`) per commit, lsns
/// strictly increasing.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RefList {
    /// The ref.
    pub ref_id: u32,
    /// (lsn, hlc) of each commit, in log order.
    pub entries: Vec<(u64, u64)>,
}

/// A `Checkpoint` payload of the probe's subset: bits 0, 1 and 5.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Checkpoint {
    /// `ckflags`.
    pub ckflags: u16,
    /// `append_hlc`.
    pub append_hlc: u64,
    /// `next_file_no`.
    pub next_file_no: u32,
    /// `upto_lsn` (bit 0): the new `checkpoint_lsn`.
    pub upto_lsn: u64,
    /// `active_log` (bit 0).
    pub active_log: u32,
    /// `segments` (bit 0): the new set.
    pub segments: Vec<SegRef>,
    /// `window_start` (bit 1): the previous `checkpoint_lsn`.
    pub window_start: u64,
    /// `ref_lists` (bit 1), by ref id.
    pub ref_lists: Vec<RefList>,
    /// `added` (bit 5).
    pub added: Vec<FileEntry>,
}

/// The bits of the probe's subset.
const SUBSET: u16 = ckflag::SET_CHANGE | ckflag::WINDOW | ckflag::FILES_ADDED;

/// Refuses `ckflags` the subset does not hold or [F05 §9.9] forbids: reserved bits; bits 2, 3, 4, 6; none of 0, 3, 5,
/// 6; bit 1 without bit 0.
fn check_flags(f: u16) -> Result<(), String> {
    if f & ckflag::RESERVED != 0 {
        return Err(format!("ckflags {f:#06x} sets reserved bits 10-15"));
    }
    if f & ckflag::WINDOW != 0 && f & ckflag::SET_CHANGE == 0 {
        return Err("ckflags bit 1 (window) without bit 0 (set_change)".into());
    }
    if f & (ckflag::SET_CHANGE | ckflag::PROMOTIONS | ckflag::FILES_ADDED | ckflag::FILES_RELEASED)
        == 0
    {
        return Err("ckflags without bit 0, 3, 5 or 6".into());
    }
    if f & !(SUBSET | ckflag::FTS_TIER2 | 0x0300) != 0 {
        return Err(format!("ckflags {f:#06x}: outside the probe's subset"));
    }
    Ok(())
}

impl Checkpoint {
    /// Appends the payload.
    pub fn encode(&self, out: &mut Vec<u8>) -> Result<(), String> {
        let f = self.ckflags;
        check_flags(f)?;
        if f & ckflag::WINDOW != 0 && self.upto_lsn <= self.window_start {
            return Err("bit 1 with upto_lsn ≤ window_start".into());
        }
        out.extend_from_slice(&f.to_le_bytes());
        out.extend_from_slice(&self.append_hlc.to_le_bytes());
        put_uvar(out, u64::from(self.next_file_no));
        if f & ckflag::SET_CHANGE != 0 {
            put_uvar(out, self.upto_lsn);
            put_uvar(out, u64::from(self.active_log));
            let n = u8::try_from(self.segments.len())
                .ok()
                .filter(|&n| n <= 8)
                .ok_or("more than 8 segments")?;
            out.push(n);
            for s in &self.segments {
                out.extend_from_slice(&s.encode());
            }
        }
        if f & ckflag::WINDOW != 0 {
            put_uvar(out, self.window_start);
            put_uvar(out, self.ref_lists.len() as u64);
            for (i, l) in self.ref_lists.iter().enumerate() {
                if l.entries.is_empty() || i > 0 && self.ref_lists[i - 1].ref_id >= l.ref_id {
                    return Err("a RefList with no entry, or ref lists not by ref id".into());
                }
                put_uvar(out, u64::from(l.ref_id));
                put_uvar(out, l.entries.len() as u64);
                let (mut lsn, mut hlc) = (self.window_start, 0u64);
                for (j, &(at, h)) in l.entries.iter().enumerate() {
                    let dlsn = at.checked_sub(lsn).filter(|&d| d > 0 || j == 0);
                    let dhlc = h.checked_sub(hlc);
                    let (Some(dlsn), Some(dhlc)) = (dlsn, dhlc) else {
                        return Err(format!("ref {}: entry {j} goes back", l.ref_id));
                    };
                    put_uvar(out, dlsn);
                    put_uvar(out, dhlc);
                    (lsn, hlc) = (at, h);
                }
            }
        }
        if f & ckflag::FILES_ADDED != 0 {
            if self.added.is_empty() {
                return Err("bit 5 with no added file".into());
            }
            put_uvar(out, self.added.len() as u64);
            for a in &self.added {
                out.push(a.family);
                if a.family == 5 {
                    put_uvar(out, u64::from(a.ref_id));
                }
                put_uvar(out, u64::from(a.file_no));
                put_uvar(out, a.total_len);
                out.extend_from_slice(&a.digest);
            }
        }
        Ok(())
    }

    /// Decodes a payload of the subset; its fields fill it exactly.
    pub fn decode(payload: &[u8]) -> Result<Checkpoint, String> {
        let e = |d: super::enc::DecodeError| d.to_string();
        let mut r = Reader::new(payload);
        let f = r.u16().map_err(e)?;
        check_flags(f)?;
        let mut c = Checkpoint {
            ckflags: f,
            append_hlc: r.u64().map_err(e)?,
            next_file_no: r.uvar32().map_err(e)?,
            ..Checkpoint::default()
        };
        if f & ckflag::SET_CHANGE != 0 {
            c.upto_lsn = r.uvar64().map_err(e)?;
            c.active_log = r.uvar32().map_err(e)?;
            let n = r.u8().map_err(e)?;
            if n > 8 {
                return Err(format!("n_segments {n}"));
            }
            for _ in 0..n {
                c.segments
                    .push(SegRef::decode(r.take(SEG_REF_LEN).map_err(e)?));
            }
        }
        if f & ckflag::WINDOW != 0 {
            c.window_start = r.uvar64().map_err(e)?;
            if c.upto_lsn <= c.window_start {
                return Err("bit 1 with upto_lsn ≤ window_start".into());
            }
            for _ in 0..r.uvar32().map_err(e)? {
                let ref_id = r.uvar32().map_err(e)?;
                let n = r.uvar32().map_err(e)?;
                if n == 0 {
                    return Err(format!("ref {ref_id}: a RefList with no entry"));
                }
                let (mut lsn, mut hlc) = (c.window_start, 0u64);
                let mut entries = Vec::with_capacity(n as usize);
                for j in 0..n {
                    let (dl, dh) = (r.uvar64().map_err(e)?, r.uvar64().map_err(e)?);
                    if dl == 0 && j > 0 {
                        return Err(format!("ref {ref_id}: lsns not strictly increasing"));
                    }
                    lsn = lsn.checked_add(dl).ok_or("an lsn past 2^64")?;
                    hlc = hlc.checked_add(dh).ok_or("an hlc past 2^64")?;
                    entries.push((lsn, hlc));
                }
                c.ref_lists.push(RefList { ref_id, entries });
            }
        }
        if f & ckflag::FILES_ADDED != 0 {
            let n = r.uvar32().map_err(e)?;
            if n == 0 {
                return Err("bit 5 with n_added 0".into());
            }
            for _ in 0..n {
                let family = r.u8().map_err(e)?;
                let ref_id = if family == 5 {
                    r.uvar32().map_err(e)?
                } else {
                    0
                };
                c.added.push(FileEntry {
                    family,
                    ref_id,
                    file_no: r.uvar32().map_err(e)?,
                    total_len: r.uvar64().map_err(e)?,
                    digest: r.array().map_err(e)?,
                });
            }
        }
        r.end().map_err(e)?;
        Ok(c)
    }
}

/// The `Checkpoint` group: one durable record at `lsn`, `group_end` set, its trailer chained from `seed`, the chain
/// value at `lsn` ([F05 §4.2]–§4.3).
pub fn group(c: &Checkpoint, lsn: u64, epoch: u64, seed: u64) -> Result<Vec<u8>, String> {
    let mut p = Vec::with_capacity(256);
    c.encode(&mut p)?;
    raw_group(KIND, &p, lsn, epoch, seed)
}

/// Reads a `Checkpoint` group back and checks it ([F05 §5.2] rules 2–7, §4.6): one record, its `len` the group's,
/// kind 9 with `group_end` alone (durable), reserved zero, its lsn and epoch, its checksum, and the trailer chained from
/// `seed`. Returns the payload and the trailer.
pub fn read_group(b: &[u8], lsn: u64, epoch: u64, seed: u64) -> Result<(Checkpoint, u64), String> {
    if b.len() < REC_HDR_LEN + TRAILER_LEN {
        return Err(format!("a group of {} bytes", b.len()));
    }
    let h = RecHdr::decode(b[..REC_HDR_LEN].try_into().expect("32 bytes"));
    if h.len as usize != b.len() || h.kind != KIND || h.flags != flag::GROUP_END || h.reserved != 0
    {
        return Err(format!("not one durable Checkpoint record: {h:?}"));
    }
    if (h.lsn, h.epoch) != (lsn, epoch) {
        return Err(format!(
            "lsn {} epoch {}, not {lsn} {epoch}",
            h.lsn, h.epoch
        ));
    }
    if record_checksum(b, true) != h.xxh3 {
        return Err("the record checksum does not match (rule 7)".into());
    }
    let t = b.len() - TRAILER_LEN;
    let trailer = u64::from_le_bytes(b[t..].try_into().expect("8 bytes"));
    if chain(&b[..t], seed) != trailer {
        return Err("the chain trailer does not match (F05 §4.6)".into());
    }
    Ok((Checkpoint::decode(&b[REC_HDR_LEN..t])?, trailer))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tail::head::seg_kind;
    use crate::tail::scan::{MemLog, scan};
    use crate::tail::testdata;

    fn sample() -> Checkpoint {
        Checkpoint {
            ckflags: 0x0023,
            append_hlc: 0x0102_0304_0506_0708,
            next_file_no: 16,
            upto_lsn: 300,
            active_log: 1,
            segments: vec![
                SegRef {
                    file_no: 1,
                    kind: seg_kind::BASE,
                    upto_lsn: 100,
                    blake3_16: [0x11; 16],
                },
                SegRef {
                    file_no: 14,
                    kind: seg_kind::DELTA,
                    upto_lsn: 300,
                    blake3_16: [0x22; 16],
                },
            ],
            window_start: 100,
            ref_lists: vec![RefList {
                ref_id: 0,
                entries: vec![(150, 1_000), (220, 1_300)],
            }],
            added: vec![FileEntry {
                family: 6,
                ref_id: 0,
                file_no: 15,
                total_len: 4_096,
                digest: [0x33; 16],
            }],
        }
    }

    /// The `Checkpoint` payload ([F05 §9.9], `05-log.md:734-777`; `SegRef` [F04 §4.1], `FileEntry` [F05 §8.4]): the
    /// literal bytes of bits 0, 1 and 5 (`ckflags` 0x0023): `ckflags`, `append_hlc`, `next_file_no` 16, `upto_lsn`,
    /// `active_log`, two 29-byte `SegRef`s, `window_start`, one `RefList` whose entries are deltas from `window_start`
    /// and from hlc 0, one `FileEntry`; the round trip; refusals of bits 10–15, of bit 1 without bit 0, of bit 2 (outside
    /// the subset), and of a trailing byte.
    #[test]
    fn checkpoint_record_follows_f05_9_9() {
        let c = sample();
        let mut p = Vec::new();
        c.encode(&mut p).unwrap();
        let mut want = vec![0x23, 0x00, 8, 7, 6, 5, 4, 3, 2, 1, 16, 0xAC, 0x02, 1, 2];
        want.extend([1, 0, 0, 0, 1, 100, 0, 0, 0, 0, 0, 0, 0]);
        want.extend([0x11; 16]);
        want.extend([14, 0, 0, 0, 2, 0x2C, 0x01, 0, 0, 0, 0, 0, 0]);
        want.extend([0x22; 16]);
        want.extend([100, 1, 0, 2, 50, 0xE8, 0x07, 70, 0xAC, 0x02]);
        want.extend([1, 6, 15, 0x80, 0x20]);
        want.extend([0x33; 16]);
        assert_eq!(p, want, "the F05 §9.9 literal");
        assert_eq!(Checkpoint::decode(&p), Ok(c.clone()));
        for (f, why) in [
            (0x0423, "reserved"),
            (0x0002 | 0x0020, "without bit 0"),
            (0x0027, "outside the probe's subset"),
        ] {
            let mut b = p.clone();
            b[0..2].copy_from_slice(&u16::to_le_bytes(f));
            assert!(
                Checkpoint::decode(&b).unwrap_err().contains(why),
                "{f:#06x}: {:?}",
                Checkpoint::decode(&b)
            );
            assert!(
                Checkpoint {
                    ckflags: f,
                    ..c.clone()
                }
                .encode(&mut Vec::new())
                .is_err()
            );
        }
        let mut long = p.clone();
        long.push(0);
        assert!(Checkpoint::decode(&long).is_err());
        let no_files = Checkpoint {
            ckflags: 0x0003,
            added: Vec::new(),
            ..c
        };
        let mut b = Vec::new();
        no_files.encode(&mut b).unwrap();
        assert_eq!(b.len(), want.len() - 21);
        assert_eq!(Checkpoint::decode(&b), Ok(no_files));
    }

    /// The `Checkpoint` group ([F05 §3]–§4; u21 cut §3.7) is one durable record: `len` = 32 + payload + 8, kind 9,
    /// `flags` = `group_end` alone, reserved zero, its lsn and epoch, its checksum over `[0, 24)` and the payload, and
    /// the trailer XXH3-64 of the group before it seeded with the chain value at its lsn; it reads back, refuses another
    /// seed, and the scan refuses kind 9 as outside the probe's subset.
    #[test]
    fn checkpoint_group_is_one_durable_record_with_its_trailer() {
        let x = testdata::expect();
        let (lsn, seed) = (testdata::START + 4_000, 0x5EED_0000_0000_0001u64);
        let c = sample();
        let mut p = Vec::new();
        c.encode(&mut p).unwrap();
        let g = group(&c, lsn, x.epoch, seed).unwrap();
        assert_eq!(g.len(), 32 + p.len() + 8);
        let h = RecHdr::decode(g[..32].try_into().unwrap());
        assert_eq!(
            (h.len as usize, h.kind, h.flags, h.reserved, h.lsn, h.epoch),
            (g.len(), 9, 0x02, 0, lsn, x.epoch)
        );
        assert_eq!(&g[32..32 + p.len()], &p[..]);
        assert_eq!(h.xxh3, record_checksum(&g, true));
        let t = u64::from_le_bytes(g[g.len() - 8..].try_into().unwrap());
        assert_eq!(t, chain(&g[..g.len() - 8], seed), "the trailer");
        assert_eq!(read_group(&g, lsn, x.epoch, seed), Ok((c, t)));
        assert!(
            read_group(&g, lsn, x.epoch, seed ^ 1)
                .unwrap_err()
                .contains("trailer")
        );
        let err = scan(
            &mut MemLog::new(lsn, &g),
            &x,
            lsn,
            lsn + g.len() as u64,
            seed,
            4_096,
            &mut |_| Ok(()),
        )
        .unwrap_err();
        assert!(err.what.contains("outside the probe's subset"), "{err}");
    }
}
