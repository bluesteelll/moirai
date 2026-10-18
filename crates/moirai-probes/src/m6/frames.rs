//! Measurement 6's `hist` frames (cut D-3): the records of the probe's synthetic log tails, cut into frames by
//! [F10 §4.2], as the inputs of the `hist` quantities.
//!
//! - **The tails**: four tails of measurement 10's `rec` family at the profile ([`crate::tail::synth::generate`]):
//!   threshold P07 (4 MiB in production), N = 1e6, seeds `xxh3_64("m6.hist.<j>")` for j = 0 to 3.
//! - **The records**: each tail is walked from its first byte one `RecHdr` at a time, and its `Noop` records (the
//!   rotation pads) are dropped, since retirement keeps every record but the pad ([F10 §4.1]).
//! - **The cut**: each tail is one retired extent. Before a record joins the current frame, the frame is closed if it
//!   is not empty and its raw bytes plus the record's would exceed P04, or the record is a `Commit` and the frame
//!   already holds P03 of them. A record longer than P04 is a split frame of its own, of ⌈len / P04⌉ blocks, all but
//!   the last of exactly P04 bytes. Every other frame is one block. P03 and P04 are 256 and 1 MiB in production, 4 and
//!   4 KiB in the test profile ([`crate::tail::Params`]).
//!
//! The measured inputs are the blocks, each at most 1 MiB raw ([F10 §4.2]), and every frame is measured:
//! a `hist` frame never uses a dictionary ([F10 §3.4]). They follow the generator (its filler text and
//! random body bytes), not real logs, so `F10-hist-codec` gets no real-log evidence from them.

use super::corpus::Input;
use crate::tail::rec::RecHdr;
use crate::tail::synth::{Clause, Target, generate};
use crate::tail::{Params, REC_HDR_LEN, kind};
use std::ops::Range;

/// The tails a `hist` corpus is cut from.
pub const TAILS: u64 = 4;
/// The node count of their base.
pub const N: u32 = 1_000_000;

/// The kept records of a tail's bytes, in order: every record but the `Noop` pads.
pub fn records(bytes: &[u8]) -> Result<Vec<&[u8]>, String> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let hdr: &[u8; REC_HDR_LEN] = bytes
            .get(at..at + REC_HDR_LEN)
            .and_then(|h| h.try_into().ok())
            .ok_or_else(|| format!("a record header cut short at offset {at}"))?;
        let h = RecHdr::decode(hdr);
        let len = h.len as usize;
        if len < REC_HDR_LEN || at + len > bytes.len() {
            return Err(format!("a record of {len} bytes at offset {at}"));
        }
        if h.kind != kind::NOOP {
            out.push(&bytes[at..at + len]);
        }
        at += len;
    }
    Ok(out)
}

/// The frames of `records` by F10 §4.2 with P03 = `commits` and P04 = `bytes`: each frame as its blocks.
pub fn cut(records: &[&[u8]], commits: u32, bytes: u32) -> Vec<Vec<Vec<u8>>> {
    let p04 = bytes as usize;
    let mut frames = Vec::new();
    let mut cur: Vec<u8> = Vec::new();
    let mut held = 0u32;
    for r in records {
        let commit = r[4] == kind::COMMIT;
        if !cur.is_empty() && (cur.len() + r.len() > p04 || (commit && held >= commits)) {
            frames.push(vec![std::mem::take(&mut cur)]);
            held = 0;
        }
        if r.len() > p04 {
            frames.push(r.chunks(p04).map(<[u8]>::to_vec).collect());
            continue;
        }
        cur.extend_from_slice(r);
        held += u32::from(commit);
    }
    if !cur.is_empty() {
        frames.push(vec![cur]);
    }
    frames
}

/// The `hist` corpus: every block as an input, ids `hist/<j>/<frame>.<block>` in cut order, and each frame's range
/// of blocks.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Frames {
    /// The blocks, in order.
    pub blocks: Vec<Input>,
    /// Each frame's blocks.
    pub frames: Vec<Range<usize>>,
}

impl Frames {
    /// Every input a frame of its own: the notes and the plan sections.
    pub fn singles(blocks: Vec<Input>) -> Frames {
        let frames = (0..blocks.len()).map(|i| i..i + 1).collect();
        Frames { blocks, frames }
    }

    /// Appends the frames of `tail`'s records, cut with `p`'s P03 and P04.
    pub fn push_tail(&mut self, j: u64, bytes: &[u8], p: &Params) -> Result<(), String> {
        let recs = records(bytes)?;
        for (f, frame) in cut(&recs, p.hist_frame_commits, p.hist_frame_bytes)
            .into_iter()
            .enumerate()
        {
            let first = self.blocks.len();
            for (b, block) in frame.into_iter().enumerate() {
                self.blocks.push(Input {
                    id: format!("hist/{j}/{f:06}.{b:04}"),
                    bytes: block,
                });
            }
            self.frames.push(first..self.blocks.len());
        }
        Ok(())
    }
}

/// The generator's target of tail `j` at `p`.
pub fn target(j: u64, p: Params) -> Target {
    Target {
        clause: Clause::Rec,
        threshold: p.p07,
        mult: 1,
        n: N,
        seed: xxhash_rust::xxh3::xxh3_64(format!("m6.hist.{j}").as_bytes()),
        params: p,
        rt_bound: None,
    }
}

/// The `hist` corpus at `p`: the frames of [`TAILS`] tails.
pub fn hist(p: Params) -> Result<Frames, String> {
    let mut f = Frames::default();
    for j in 0..TAILS {
        let tail = generate(&target(j, p), false)?;
        let len = usize::try_from(tail.end - tail.start).map_err(|e| e.to_string())?;
        f.push_tail(j, &tail.bytes[..len], &p)?;
    }
    Ok(f)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A record of `len` bytes of `kind`: its header, then filler.
    fn rec(kind: u8, len: u32, fill: u8) -> Vec<u8> {
        let h = RecHdr {
            len,
            kind,
            flags: 0,
            reserved: 0,
            lsn: 0,
            epoch: 0,
            xxh3: 0,
        };
        let mut v = h.encode().to_vec();
        v.resize(len as usize, fill);
        v
    }

    fn lens(frames: &[Vec<Vec<u8>>]) -> Vec<Vec<usize>> {
        frames
            .iter()
            .map(|f| f.iter().map(Vec::len).collect())
            .collect()
    }

    /// The F10 §4.2 cut at P03 = 4 and P04 = 4,096: the byte bound closes a frame before a record would take it past
    /// P04, the commit bound before a fifth `Commit` (other records do not count), and a record above P04 is a split
    /// frame of its own of ⌈len / P04⌉ blocks, the frame before it closed and the next record starting a new one.
    /// Every block is at most P04 and the frames hold the records' bytes in order. On a generated tail the same holds.
    #[test]
    fn frames_follow_the_f10_cut() {
        let p = Params::TEST;
        let (c, o) = (kind::COMMIT, kind::EXTENT_HEAD);
        let recs: Vec<Vec<u8>> = vec![
            rec(o, 1_500, 1),
            rec(c, 1_500, 2),
            rec(c, 1_500, 3), // 4,500 > 4,096: a new frame
            rec(c, 100, 4),
            rec(o, 100, 5),
            rec(c, 100, 6),
            rec(c, 100, 7), // the frame now holds 4 commits
            rec(c, 100, 8), // a fifth: a new frame
            rec(o, 10_000, 9),
            rec(c, 100, 10),
        ];
        let refs: Vec<&[u8]> = recs.iter().map(Vec::as_slice).collect();
        let frames = cut(&refs, p.hist_frame_commits, p.hist_frame_bytes);
        assert_eq!(
            lens(&frames),
            [
                vec![3_000],
                vec![1_500 + 400],
                vec![100],
                vec![4_096, 4_096, 1_808],
                vec![100],
            ]
        );
        assert!(
            frames[3].len() >= 2,
            "a split frame holds at least 2 blocks"
        );
        let flat: Vec<u8> = frames.iter().flatten().flatten().copied().collect();
        assert_eq!(flat, recs.concat());
        // A record of exactly P04 fits a frame of its own as one block.
        let exact = [rec(c, 4_096, 1)];
        let refs: Vec<&[u8]> = exact.iter().map(Vec::as_slice).collect();
        assert_eq!(lens(&cut(&refs, 4, 4_096)), [vec![4_096]]);
        // A generated tail at the test profile, its threshold raised so it spans extents.
        let mut f = Frames::default();
        let mut t = target(0, p);
        t.threshold = 48 * 1_024;
        t.rt_bound = Some(u64::MAX);
        let tail = generate(&t, false).unwrap();
        let bytes = &tail.bytes[..(tail.end - tail.start) as usize];
        f.push_tail(0, bytes, &p).unwrap();
        assert!(f.frames.len() >= 12, "{} frames", f.frames.len());
        for r in &f.frames {
            let frame: Vec<u8> = f.blocks[r.clone()]
                .iter()
                .flat_map(|b| b.bytes.clone())
                .collect();
            assert!(f.blocks[r.clone()].iter().all(|b| b.bytes.len() <= 4_096));
            if r.len() == 1 {
                let recs = records(&frame).unwrap();
                let commits = recs.iter().filter(|x| x[4] == kind::COMMIT).count();
                assert!(commits <= 4 && frame.len() <= 4_096);
            }
        }
        assert!(
            f.blocks.windows(2).all(|w| w[0].id < w[1].id),
            "ids in cut order"
        );
    }

    /// The kept records are the tail's in order without its `Noop` pads: on a hand-built tail with a pad, and on a
    /// generated tail that spans extents, whose frames are its bytes minus the pads and start with its `ExtentHead`.
    #[test]
    fn frames_hold_the_tails_records_in_order() {
        let recs = [
            rec(kind::EXTENT_HEAD, 138, 1),
            rec(kind::COMMIT, 300, 2),
            rec(kind::NOOP, 40, 0),
            rec(kind::EXTENT_HEAD, 138, 3),
            rec(kind::COMMIT, 64, 4),
        ];
        let bytes = recs.concat();
        let kept = records(&bytes).unwrap();
        assert_eq!(
            kept,
            [&recs[0][..], &recs[1][..], &recs[3][..], &recs[4][..]]
        );
        assert!(records(&bytes[..bytes.len() - 1]).is_err(), "a cut record");
        assert!(
            records(&rec(kind::COMMIT, 8, 0)[..8]).is_err(),
            "a cut header"
        );
        let p = Params::TEST;
        let mut t = target(1, p);
        t.threshold = 160 * 1_024;
        t.rt_bound = Some(u64::MAX);
        let tail = generate(&t, false).unwrap();
        let bytes = &tail.bytes[..(tail.end - tail.start) as usize];
        let all = records(bytes).unwrap();
        let mut with_pads = 0;
        let mut at = 0;
        while at < bytes.len() {
            let h = RecHdr::decode(bytes[at..at + REC_HDR_LEN].try_into().unwrap());
            with_pads += 1;
            at += h.len as usize;
        }
        assert!(
            with_pads > all.len(),
            "the tail spans extents, so it holds pads"
        );
        assert_eq!(all[0][4], kind::EXTENT_HEAD);
        let mut f = Frames::default();
        f.push_tail(1, bytes, &p).unwrap();
        let flat: Vec<u8> = f.blocks.iter().flat_map(|b| b.bytes.clone()).collect();
        assert_eq!(flat, all.concat());
        let h = hist(p).unwrap();
        assert!(h.blocks.iter().any(|b| b.id.starts_with("hist/3/")));
        assert_eq!(h.frames.last().unwrap().end, h.blocks.len());
    }
}
