//! The op-locating read of the probe's tail overlay (WP-53e; R-40 O-2, R-42): T1's diagnostic, a read that decodes
//! only the ops of the node it reads ([`Overlay::get_op`]), beside the whole-record read [`Overlay::get`].
//!
//! **What locates the op** is the record format, not the overlay's tables: [F09 §4.6], [F11 §9] and [F05 §10.4] index
//! no position inside a `Commit` record.
//!
//! | Fact | Specification |
//! |---|---|
//! | `RecHdr.len` and `kind`: the payload's extent and the record's kind, without its payload | [F05 §3.1] (`05-log.md:118-129`), [F05 §3.3] (`:141-151`) |
//! | a `SymDefs` block opens the payload when flag bit 2 is set, walkable by `n_defs` and each `vstr` length | [F05 §8.1] (`05-log.md:419-435`) |
//! | the changeset follows the header part and precedes `n_bodies` and the bodies | [F06 §4.1] (`06-commit.md:170-179`), [F06 §4.3] (`:252-259`) |
//! | the op frame: `op` u8, `len` uvar32, the body; "The length lets a reader skip ops without decoding them" | [F06 §7.1] (`06-commit.md:671-679`) |
//! | every op begins with its owner (`id`, or an edge's `src`), then `prev` | [F06 §7.4] (`06-commit.md:726`), [F06 §7.5.1] (`:849-852`) |
//! | the ops are sorted by (owner, rank, detail), so a reader stops once it passes the owner it looks for | [F06 §7.9] (`06-commit.md:1008-1018`) |
//!
//! **The frame walk** ([`Overlay::get_op`]). For each record of the owner's chain, from the node index's lsn:
//! - one 32-byte header read, with [`Overlay::get`]'s refusals;
//! - the payload `[lsn + 32, lsn + len − t)` read forward through the caller's buffer: each read starts at the first byte
//!   the walk needs that the buffer does not hold and asks for `max(chunk, need)` bytes, clipped to the payload's end;
//! - the `SymDefs` block and the header part walked by their lengths (`affected` by its varints: [F06 §4.4.13] gives it
//!   no byte length), then `n_ops`;
//! - each frame's head (`op`, `len`, the owner): a frame of an owner below n is skipped by its length, n's frames are
//!   held and decoded with [`Op::decode`], and the first frame past n ends the walk. `n_bodies` and the bodies are
//!   never read.
//!
//! Then it does as `get` does: the first of n's ops that sets the key gives the value, else their `prev` leads to the
//! older record or out of the tail.
//!
//! **What differs from `get`.** The buffer is the caller's and is reused: only its allocation carries over, and the bytes
//! it holds are dropped at each record's start, so a read never reuses an earlier record's I/O. No trailer is read; no
//! frame of another owner is decoded, and none past the held window is read; no body is read. It does not re-validate:
//! the replay's scan validated every record ([F05 §5.2]), and a replay never hashes bodies ([F06 §8] BD-2,
//! `06-commit.md:1053-1054`). So it returns what `get` returns for every (n, key) on a valid tail; on a corrupt record
//! the two may differ, because `get` decodes every op of the record.

use super::{Key, Overlay, Probe, Value, sets};
use crate::tail::commit::{Op, presence};
use crate::tail::enc::{Bound, Reader};
use crate::tail::rec::{RecHdr, sym};
use crate::tail::scan::LogSource;
use crate::tail::{REC_HDR_LEN, TRAILER_LEN, flag, kind};
use std::cmp::Ordering;

/// The most bytes of a frame's head: `op` (1), `len` (`uvar32`, at most 5) and the owner (`uvar32`, at most 5).
pub const FRAME_HEAD_MAX: usize = 11;

/// A window over one record's bytes below `to`, read through the caller's buffer: `buf[..len]` holds `[at, at + len)`.
struct Window<'s, 'b, S: LogSource> {
    source: &'s mut S,
    buf: &'b mut Vec<u8>,
    chunk: usize,
    lsn: u64,
    to: u64,
    at: u64,
    len: usize,
}

impl<'s, 'b, S: LogSource> Window<'s, 'b, S> {
    /// A window over `[from, to)` of the record at `lsn` that holds nothing yet.
    fn new(
        source: &'s mut S,
        buf: &'b mut Vec<u8>,
        chunk: usize,
        lsn: u64,
        from: u64,
        to: u64,
    ) -> Window<'s, 'b, S> {
        Window {
            source,
            buf,
            chunk,
            lsn,
            to,
            at: from,
            len: 0,
        }
    }

    /// Holds `[pos, pos + need)`, clipped to the window's end, keeping what it holds from `keep` (at most `pos`) on. A
    /// read starts at the first byte needed that the buffer does not hold and asks for `max(chunk, missing)` bytes,
    /// clipped to the window's end. The buffer grows when it must and never shrinks.
    fn hold(&mut self, keep: u64, pos: u64, need: usize) -> Result<&[u8], String> {
        let end = pos.saturating_add(need as u64).min(self.to);
        if keep > pos || pos > end {
            return Err(format!(
                "the walk passes the end of the record at lsn {}",
                self.lsn
            ));
        }
        let held = self.at + self.len as u64;
        if keep < self.at || end > held {
            if keep >= self.at && keep <= held {
                let off = (keep - self.at) as usize;
                self.buf.copy_within(off..self.len, 0);
                self.len -= off;
            } else {
                self.len = 0;
            }
            self.at = keep;
            let start = self.at + self.len as u64;
            let want = (end - start).max(self.chunk as u64).min(self.to - start) as usize;
            let total = self.len + want;
            if self.buf.len() < total {
                self.buf.resize(total, 0);
            }
            let mut done = 0;
            while done < want {
                let got = self
                    .source
                    .read_at(start + done as u64, &mut self.buf[self.len + done..total])?;
                if got == 0 {
                    return Err(format!(
                        "the log ends inside the record at lsn {}",
                        self.lsn
                    ));
                }
                done += got;
            }
            self.len = total;
        }
        let a = (pos - self.at) as usize;
        Ok(&self.buf[a..a + (end - pos) as usize])
    }

    /// The `k` bytes at `*pos`, which moves past them.
    fn fixed(&mut self, pos: &mut u64, k: usize) -> Result<&[u8], String> {
        let at = *pos;
        *pos += k as u64;
        let lsn = self.lsn;
        let b = self.hold(at, at, k)?;
        if b.len() < k {
            return Err(format!("lsn {lsn}: the record ends inside a field"));
        }
        Ok(b)
    }

    /// A `u8` at `*pos`.
    fn u8(&mut self, pos: &mut u64) -> Result<u8, String> {
        Ok(self.fixed(pos, 1)?[0])
    }

    /// A little-endian `u32` at `*pos`.
    fn u32(&mut self, pos: &mut u64) -> Result<u32, String> {
        let b = self.fixed(pos, 4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// A `uvar` of `bound` at `*pos`.
    fn uvar(&mut self, pos: &mut u64, bound: Bound) -> Result<u64, String> {
        let lsn = self.lsn;
        let mut r = Reader::new(self.hold(*pos, *pos, bound.max_len())?);
        let v = r
            .uvar(bound)
            .map_err(|e| format!("lsn {lsn}: {}", e.what))?;
        *pos += r.pos() as u64;
        Ok(v)
    }
}

/// Reads the 32-byte header at `lsn` with [`Overlay::get`]'s refusals: no record there, or not a `Commit`.
fn header<S: LogSource>(source: &mut S, lsn: u64) -> Result<RecHdr, String> {
    let mut h = [0u8; REC_HDR_LEN];
    let mut done = 0;
    while done < REC_HDR_LEN {
        let got = source.read_at(lsn + done as u64, &mut h[done..])?;
        if got == 0 {
            return Err(format!("the log ends inside the record at lsn {lsn}"));
        }
        done += got;
    }
    let hdr = RecHdr::decode(&h);
    if (hdr.len as usize) < REC_HDR_LEN || hdr.lsn != lsn {
        return Err(format!("no record at lsn {lsn}"));
    }
    if hdr.kind != kind::COMMIT {
        return Err(format!("the record at lsn {lsn} is not a Commit"));
    }
    Ok(hdr)
}

/// Where n's frames lie in a record: `[first, end)`, `frames` of them, held in the buffer from lsn `at` on.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct Span {
    first: u64,
    end: u64,
    frames: usize,
    at: u64,
}

impl Span {
    /// n's frames' bytes in the buffer the walk filled.
    fn bytes<'b>(&self, buf: &'b [u8]) -> &'b [u8] {
        &buf[(self.first - self.at) as usize..(self.end - self.at) as usize]
    }
}

/// The walk's steps 1–4 on the record at `lsn`: its header, its payload's head (the `SymDefs` block and the header part
/// up to `n_ops`), then the frames up to the first past n. Returns n's frames, held in `buf`, or `None` when the record
/// holds no frame of n.
fn locate<S: LogSource>(
    source: &mut S,
    buf: &mut Vec<u8>,
    chunk: usize,
    lsn: u64,
    n: u32,
) -> Result<Option<Span>, String> {
    let hdr = header(source, lsn)?;
    let t = if hdr.group_end() { TRAILER_LEN } else { 0 } as u64;
    let from = lsn + REC_HDR_LEN as u64;
    let to = (lsn + u64::from(hdr.len)).saturating_sub(t).max(from);
    let mut w = Window::new(source, buf, chunk, lsn, from, to);
    let mut pos = from;
    if hdr.flags & flag::SYMDEFS != 0 {
        let defs = w.uvar(&mut pos, Bound::U16)?;
        for _ in 0..defs {
            let class = w.u8(&mut pos)?;
            let id = if class == sym::ROLE || class == sym::ROOT {
                Bound::U16
            } else {
                Bound::U32
            };
            w.uvar(&mut pos, id)?;
            let text = w.uvar(&mut pos, Bound::U32)?;
            pos += text;
        }
    }
    // The header part of [F06 §4.3] for the probe's presence subset (`tail/commit.rs`, `Commit::decode`).
    let p = w.u32(&mut pos)?;
    if p & presence::RESERVED != 0 || p & !presence::SUBSET != 0 {
        return Err(format!(
            "lsn {lsn}: presence {p:#010x} outside the probe's subset"
        ));
    }
    pos += 32; // commit_id
    match w.u8(&mut pos)? {
        0 => {}
        1 => {
            pos += 16; // id16
            w.uvar(&mut pos, Bound::U64)?; // the parent's lsn
        }
        k => return Err(format!("lsn {lsn}: n_parents {k}")),
    }
    for b in [Bound::U32, Bound::U64, Bound::U32, Bound::U32] {
        w.uvar(&mut pos, b)?; // gen_no, seq, ref_sym, ref_id
    }
    if p & presence::REF_OLD != 0 {
        pos += 16;
    }
    if p & presence::PREV_ON_REF != 0 {
        w.uvar(&mut pos, Bound::U64)?;
    }
    w.uvar(&mut pos, Bound::U32)?; // ref_seq
    pos += 2 + 8; // kind, import; hlc
    for _ in 0..4 {
        w.uvar(&mut pos, Bound::U32)?; // actor, role, session, schema_version
    }
    if p & presence::GIT != 0 {
        let algo = w.u8(&mut pos)?;
        let gflags = w.u8(&mut pos)?;
        let d = match algo {
            1 => 20,
            2 => 32,
            _ => 0,
        };
        if gflags & 1 != 0 {
            pos += d;
        }
        w.uvar(&mut pos, Bound::U32)?; // branch
        w.uvar(&mut pos, Bound::U32)?; // worktree
        if gflags & 2 != 0 {
            pos += d;
        }
    }
    if p & presence::IDEM != 0 {
        pos += 32;
    }
    pos += 2; // stmt_origin, actor_src
    w.uvar(&mut pos, Bound::U32)?; // stmt_sym
    if p & presence::STMT_HASH != 0 {
        pos += 16;
    }
    w.uvar(&mut pos, Bound::U64)?; // append_delta
    if p & presence::MSG != 0 {
        let msg = w.uvar(&mut pos, Bound::U32)?;
        pos += msg; // the message, by its vstr length
    }
    if p & presence::AFFECTED != 0 {
        let ids = w.uvar(&mut pos, Bound::U32)?;
        pos += 1; // affected_complete
        for _ in 0..ids {
            w.uvar(&mut pos, Bound::U32)?;
        }
    }
    pos += 32; // changeset_digest
    let n_ops = w.uvar(&mut pos, Bound::U32)?;
    // The frames: skip the owners below n, keep n's, stop at the first past n ([F06 §7.9]).
    let mut found: Option<(u64, usize)> = None;
    for _ in 0..n_ops {
        let q = pos;
        let keep = found.map_or(q, |f| f.0);
        let mut r = Reader::new(w.hold(keep, q, FRAME_HEAD_MAX)?);
        let head = (|| {
            r.u8()?;
            let len = r.uvar32()?;
            let body = r.pos();
            Ok::<_, crate::tail::enc::DecodeError>((len, body, r.uvar32()?))
        })();
        let (len, body, owner) = head.map_err(|e| format!("lsn {lsn}: {}", e.what))?;
        let next = q + body as u64 + u64::from(len);
        match (owner.cmp(&n), found) {
            (Ordering::Less, None) => pos = next,
            (Ordering::Equal, f) => {
                found = Some(f.map_or((q, 1), |(a, k)| (a, k + 1)));
                pos = next;
            }
            _ => break,
        }
    }
    let Some((first, frames)) = found else {
        return Ok(None);
    };
    w.hold(first, first, (pos - first) as usize)?;
    Ok(Some(Span {
        first,
        end: pos,
        frames,
        at: w.at,
    }))
}

/// What a record's frames of n give: the value of the key, or n's `prev`.
enum Step {
    Value(Value),
    Prev(u64),
}

/// Decodes n's `frames` frames from `bytes` (the record at `lsn`) and applies [`Overlay::get`]'s rule: the first that
/// sets `key` gives its value; otherwise their `prev`.
fn step(bytes: &[u8], frames: usize, n: u32, key: &Key, lsn: u64) -> Result<Step, String> {
    let mut r = Reader::new(bytes);
    let mut prev = 0;
    for _ in 0..frames {
        let op = Op::decode(&mut r).map_err(|e| format!("lsn {lsn}: {e}"))?;
        if op.owner() != n {
            return Err(format!(
                "lsn {lsn}: a frame of #{} where #{n}'s was located",
                op.owner()
            ));
        }
        prev = op.prev();
        if let Some((_, v)) = sets(&op).into_iter().find(|(k, _)| k == key) {
            return Ok(Step::Value(v));
        }
    }
    Ok(Step::Prev(prev))
}

impl Overlay {
    /// The value of `key` of node `n`, as [`Overlay::get`] gives it on a valid tail, decoding only n's ops: the frame
    /// walk of the module doc, through `buf` (reused across calls, never shrunk) in reads of at least `chunk` bytes.
    pub fn get_op<S: LogSource>(
        &self,
        source: &mut S,
        n: u32,
        key: &Key,
        chunk: usize,
        buf: &mut Vec<u8>,
    ) -> Result<Probe, String> {
        let Ok(i) = self.nodes.binary_search_by_key(&n, |e| e.n) else {
            return Ok(Probe::NotInTail);
        };
        self.walk_op(source, self.nodes[i].lsn, n, key, chunk, buf)
    }

    /// The frame walk from the record at `lsn` back along n's `prev` chain.
    fn walk_op<S: LogSource>(
        &self,
        source: &mut S,
        mut lsn: u64,
        n: u32,
        key: &Key,
        chunk: usize,
        buf: &mut Vec<u8>,
    ) -> Result<Probe, String> {
        loop {
            let Some(s) = locate(source, buf, chunk, lsn, n)? else {
                return Err(format!("the record at lsn {lsn} holds no op of #{n}"));
            };
            match step(s.bytes(buf), s.frames, n, key, lsn)? {
                Step::Value(v) => return Ok(Probe::Tail(v)),
                Step::Prev(p) => match self.older(lsn, p) {
                    Some(l) => lsn = l,
                    None => return Ok(Probe::NotInTail),
                },
            }
        }
    }

    /// The record `prev` leads to from `lsn`, as [`Overlay::get`] follows it: none when `prev` is 0 or leaves the tail.
    fn older(&self, lsn: u64, prev: u64) -> Option<u64> {
        if prev == 0 || prev > lsn || lsn - prev < self.start {
            None
        } else {
            Some(lsn - prev)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::naive::Naive;
    use super::super::{Base, NodeEntry};
    use super::*;
    use crate::tail::commit::Commit;
    use crate::tail::rec::{Payload, Record};
    use crate::tail::scan::MemLog;
    use crate::tail::synth::{Clause, Target, generate, tests::test_target};
    use crate::tail::testdata::{self, CHAIN_IN, START};
    use crate::tail::{Params, TAIL_READ_CHUNK};

    /// The hand-built tail's base: 50 live nodes of kind 1, status `#N mod 4`.
    const BASE: Base = Base {
        n: 50,
        kind: 1,
        statuses: 4,
    };

    /// A tail to probe: its bytes from `start`, its records, its overlay, its naive map, and keys the tail never sets.
    struct Probed {
        name: String,
        start: u64,
        bytes: Vec<u8>,
        records: Vec<Record<'static>>,
        overlay: Overlay,
        naive: Naive,
        unset: Vec<(u32, Key)>,
    }

    /// The hand-built tail, with the five keys the tail never sets of `overlay/mod.rs`'s test.
    fn hand_built() -> Probed {
        let hb = testdata::hand_built();
        let end = START + hb.bytes.len() as u64;
        let overlay = Overlay::replay(
            &mut MemLog::new(START, &hb.bytes),
            &testdata::expect(),
            START,
            end,
            CHAIN_IN,
            BASE,
            TAIL_READ_CHUNK,
        )
        .unwrap();
        Probed {
            name: "hand-built".into(),
            start: START,
            naive: Naive::of(&hb.records, &BASE),
            bytes: hb.bytes,
            records: hb.records,
            overlay,
            unset: vec![
                (40, Key::Status),
                (5, Key::Status),
                (7, Key::Field(1)),
                (51, Key::Hier),
                (3, Key::Edge(1, 51)),
            ],
        }
    }

    /// A test-profile `ops` tail of 120 ops over N = 50, so owners repeat and reads walk `prev` (as `equals_naive`).
    fn generated(seed: u64) -> Probed {
        let mut t = test_target(Clause::Ops, 50, seed);
        t.threshold = 120;
        t.rt_bound = Some(u64::MAX);
        let tail = generate(&t, true).unwrap();
        let overlay = tail.overlay().unwrap();
        Probed {
            name: format!("seed {seed}"),
            start: tail.start,
            naive: Naive::of(&tail.records, &tail.base),
            bytes: tail.bytes,
            records: tail.records,
            overlay,
            unset: vec![(1, Key::Field(99)), (60, Key::Hier)],
        }
    }

    fn probed() -> Vec<Probed> {
        let mut v = vec![hand_built()];
        v.extend((1..=4).map(generated));
        v
    }

    /// The keys a test reads: every key of the naive map, the tail's unset keys, and every key class of each node of
    /// the node index.
    fn keys(p: &Probed) -> Vec<(u32, Key)> {
        let mut k: Vec<(u32, Key)> = p.naive.values.keys().cloned().collect();
        k.extend(p.unset.iter().cloned());
        for e in p.overlay.nodes() {
            for key in [
                Key::Exist,
                Key::Status,
                Key::Hier,
                Key::Body,
                Key::Field(1),
                Key::Edge(1, 1),
            ] {
                k.push((e.n, key));
            }
        }
        k
    }

    /// The `Commit` records of `records`.
    fn commits<'r>(records: &'r [Record<'static>]) -> Vec<(u64, &'r Commit<'static>)> {
        records
            .iter()
            .filter_map(|r| match &r.payload {
                Payload::Commit(c) => Some((r.lsn, &**c)),
                _ => None,
            })
            .collect()
    }

    /// The frame walk returns what the whole-record read returns ([`Overlay::get`]) for every key the tail sets, every
    /// key class of every node it touched and keys it never sets, at chunks of 1, 13, 64 and 64 KiB through one reused
    /// buffer; every value of the naive map is among them, so the comparison is not vacuous.
    #[test]
    fn op_read_equals_the_whole_record_read() {
        let mut buf = Vec::new();
        for p in probed() {
            let mut set = 0;
            for (n, key) in keys(&p) {
                let want = p.overlay.get(&mut MemLog::new(p.start, &p.bytes), n, &key);
                if let Some(v) = p.naive.values.get(&(n, key.clone())) {
                    assert_eq!(want, Ok(Probe::Tail(v.clone())), "{} #{n} {key:?}", p.name);
                    set += 1;
                }
                for chunk in [1, 13, 64, TAIL_READ_CHUNK] {
                    let got = p.overlay.get_op(
                        &mut MemLog::new(p.start, &p.bytes),
                        n,
                        &key,
                        chunk,
                        &mut buf,
                    );
                    assert_eq!(got, want, "{} chunk {chunk} #{n} {key:?}", p.name);
                }
            }
            assert!(set >= p.naive.values.len() && set > 10, "{}: {set}", p.name);
        }
    }

    proptest::proptest! {
        #![proptest_config(crate::tail::enc::tests::config())]

        /// The frame walk equals the whole-record read on generated tails, at a random chunk.
        #[test]
        fn op_read_equals_the_whole_record_read_on_generated_tails(
            seed in proptest::prelude::any::<u64>(),
            chunk in 1usize..=4_096,
        ) {
            let p = generated(seed);
            let mut buf = Vec::new();
            for (n, key) in keys(&p) {
                let want = p.overlay.get(&mut MemLog::new(p.start, &p.bytes), n, &key);
                let got = p.overlay.get_op(&mut MemLog::new(p.start, &p.bytes), n, &key, chunk, &mut buf);
                proptest::prop_assert_eq!(got, want, "#{} {:?}", n, key);
            }
        }
    }

    /// The bytes of each op's frame in `c`, in op order.
    fn frame_lens(c: &Commit<'_>) -> Vec<u64> {
        let (mut out, mut scratch) = (Vec::new(), Vec::new());
        c.ops
            .iter()
            .map(|op| {
                out.clear();
                op.encode(&mut out, &mut scratch).unwrap();
                out.len() as u64
            })
            .collect()
    }

    /// The walk skips the frames of other owners without decoding them ([F06 §7.1]) and stops past its owner
    /// ([F06 §7.9]). On a record of at least three owners, with no body and more than `chunk` + 11 bytes from the second
    /// owner's first frame to `n_bodies` (test-profile tails at N = 1e6, so the record is its owners' newest), a read of
    /// its first owner with a key that owner's first op sets there, at `chunk` 64: the value is `get`'s, fewer bytes than
    /// the payload are read, and none at or past `n_bodies`. With the second owner's frame tag set to 0 (invalid,
    /// `06-commit.md:702`), `get` fails on it and the walk still returns the first owner's value.
    #[test]
    fn op_read_skips_other_owners_without_decoding_them() {
        const CHUNK: usize = 64;
        let mut found = None;
        'seeds: for seed in 1..=16 {
            let mut t = test_target(Clause::Ops, 1_000_000, seed);
            t.threshold = 200;
            t.rt_bound = Some(u64::MAX);
            let tail = generate(&t, true).unwrap();
            let o = tail.overlay().unwrap();
            for r in &tail.records {
                let Payload::Commit(c) = &r.payload else {
                    continue;
                };
                let mut owners: Vec<u32> = c.ops.iter().map(Op::owner).collect();
                owners.dedup();
                if owners.len() < 3 || !c.bodies.is_empty() {
                    continue;
                }
                let t_len = if r.group_end { TRAILER_LEN } else { 0 } as u64;
                let end = r.lsn + r.encoded_len().unwrap() - t_len;
                let n_bodies = end - 1;
                let lens = frame_lens(c);
                let k = c
                    .ops
                    .iter()
                    .take_while(|op| op.owner() == owners[0])
                    .count();
                let second = n_bodies - lens[k..].iter().sum::<u64>();
                let newest = o.nodes().iter().any(|e| {
                    *e == NodeEntry {
                        n: owners[0],
                        lsn: r.lsn,
                    }
                });
                if newest && n_bodies - second > (CHUNK + FRAME_HEAD_MAX) as u64 {
                    let key = sets(&c.ops[0])[0].0.clone();
                    found = Some((
                        tail.clone(),
                        o,
                        r.lsn,
                        end,
                        n_bodies,
                        second,
                        owners[0],
                        key,
                    ));
                    break 'seeds;
                }
            }
        }
        let (tail, o, lsn, end, n_bodies, second, n, key) =
            found.expect("a record of three owners with enough bytes after the second");
        let want = o.get(&mut MemLog::new(tail.start, &tail.bytes), n, &key);
        assert!(matches!(want, Ok(Probe::Tail(_))), "{want:?}");
        let mut buf = Vec::new();
        let mut src = MemLog::new(tail.start, &tail.bytes);
        let mut failed = Vec::new();
        let got = o.get_op(&mut src, n, &key, CHUNK, &mut buf);
        if got != want {
            failed.push(format!("#{n} {key:?}: {got:?}, not {want:?}"));
        }
        let payload = end - lsn - REC_HDR_LEN as u64;
        let read: u64 = src
            .reads
            .iter()
            .filter(|r| r.0 >= lsn + REC_HDR_LEN as u64)
            .map(|r| r.1 as u64)
            .sum();
        if read >= payload {
            failed.push(format!(
                "{read} bytes of a {payload}-byte payload read: {:?}",
                src.reads
            ));
        }
        if src.reads.iter().any(|&(a, l)| a + l as u64 > n_bodies) {
            failed.push(format!(
                "a read at or past n_bodies ({n_bodies}): {:?}",
                src.reads
            ));
        }
        let mut bad = tail.bytes.clone();
        bad[(second - tail.start) as usize] = 0;
        let whole = o.get(&mut MemLog::new(tail.start, &bad), n, &key);
        assert!(
            whole
                .as_ref()
                .is_err_and(|e| e.contains("op tag 0 is invalid")),
            "{whole:?}"
        );
        let got = o.get_op(&mut MemLog::new(tail.start, &bad), n, &key, CHUNK, &mut buf);
        if got != want {
            failed.push(format!(
                "with the second owner's tag 0: {got:?}, not {want:?}: the walk decodes its frame"
            ));
        }
        assert!(failed.is_empty(), "{}", failed.join("; "));
    }

    /// One production `ops` tail of one op whose message's `vstr` length takes 3 bytes (at least 2^14 bytes), found as
    /// `production_tails_complete_past_the_message_cap` finds the cap (`tail/synth.rs`).
    fn long_message_tail() -> crate::tail::synth::Tail {
        for seed in 0..256 {
            let tail = generate(
                &Target {
                    clause: Clause::Ops,
                    threshold: 1,
                    mult: 1,
                    n: 1_000,
                    seed,
                    params: Params::PRODUCTION,
                    rt_bound: None,
                },
                true,
            )
            .unwrap();
            if commits(&tail.records)
                .iter()
                .any(|(_, c)| c.msg.as_ref().is_some_and(|m| m.len() >= 1 << 14))
            {
                return tail;
            }
        }
        panic!("no seed of 256 gives a message of 2^14 bytes or more");
    }

    /// The walk's header part (the `SymDefs` block, the presence-gated fields, the message by its length, `affected` by
    /// its varints) reaches `n_ops`: for every `Commit` record and every owner in it, the located frames decode to
    /// exactly `Commit::decode`'s ops of that owner, at chunks of 13 and 64 KiB; owners 0 and 2^32 − 1 have none. The
    /// records are the hand-built tail's (a `SymDefs` block, an owner with two frames), the test-profile tails', and a
    /// production commit with a 3-byte message length.
    #[test]
    fn located_frames_equal_the_decoded_ops() {
        let mut tails: Vec<(String, u64, Vec<u8>, Vec<Record<'static>>)> = probed()
            .into_iter()
            .map(|p| (p.name, p.start, p.bytes, p.records))
            .collect();
        let long = long_message_tail();
        tails.push(("long message".into(), long.start, long.bytes, long.records));
        let mut buf = Vec::new();
        let mut owners_seen = 0;
        for (name, start, bytes, records) in &tails {
            for (lsn, c) in commits(records) {
                let mut owners: Vec<u32> = c.ops.iter().map(Op::owner).collect();
                owners.dedup();
                for chunk in [13, TAIL_READ_CHUNK] {
                    for &n in &owners {
                        let mut src = MemLog::new(*start, bytes);
                        let s = locate(&mut src, &mut buf, chunk, lsn, n)
                            .unwrap()
                            .unwrap_or_else(|| panic!("{name} lsn {lsn}: no frame of #{n}"));
                        let mut r = Reader::new(s.bytes(&buf));
                        let got: Vec<Op<'_>> =
                            (0..s.frames).map(|_| Op::decode(&mut r).unwrap()).collect();
                        assert_eq!(r.remaining(), 0, "{name} lsn {lsn} #{n}");
                        let want: Vec<&Op<'_>> = c.ops.iter().filter(|o| o.owner() == n).collect();
                        assert_eq!(
                            got.iter().collect::<Vec<_>>(),
                            want,
                            "{name} lsn {lsn} #{n} chunk {chunk}"
                        );
                        owners_seen += 1;
                    }
                    for n in [0, u32::MAX] {
                        let mut src = MemLog::new(*start, bytes);
                        assert_eq!(
                            locate(&mut src, &mut buf, chunk, lsn, n),
                            Ok(None),
                            "{name} lsn {lsn} #{n}"
                        );
                    }
                }
            }
        }
        assert!(owners_seen > 200, "{owners_seen}");
    }
}
