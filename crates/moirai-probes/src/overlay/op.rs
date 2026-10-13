//! The op-locating reads of the probe's tail overlay (WP-53e; R-40 O-2, R-42): T1's diagnostic, reads that decode
//! only the ops of the node they read ([`Overlay::get_op`] and its floor [`Overlay::get_op_at`]), beside the
//! whole-record read [`Overlay::get`].
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
//! **The offset read** ([`Overlay::get_op_at`], the floor `read.op.at` of R-42 Q2). The node index's entry has 4 spare
//! bytes (u32 `n` and u64 `lsn` make 16 B with padding; `AR:709` budgets "12–16 B per op"). Process state can hold
//! there where n's frames start in its newest record and how many there are ([`OpEntry`]: a 24-bit offset from the
//! record's lsn, header included, and an 8-bit frame count), so no format change is needed. The probe keeps that state
//! in a copy of the node index, [`OpIndex`], built in its setup by the frame walk; in the design these are the node
//! entry's own bytes. The read takes n's newest record with no header read, no header walk and no frame skip: a first
//! read of [`FRAME_HEAD_MAX`] bytes at `lsn + off`, then per frame the rest of it and, while frames of n remain, the
//! next frame's head (k frames, at most k + 1 reads). No read leaves the group, so none crosses its extent's end
//! ([F05 §4.4] G-1): from a frame's first byte the group holds at least 13 bytes (a frame of at least 4, `n_bodies`,
//! then the trailer or the group's next record). It decodes n's frames, refuses a frame of another owner (the index is
//! not this log's), and then does as `get` does, the older records by the frame walk.
//!
//! It is a floor of the frame walk ([MP §4.1]): a subset of its work, the same decode of n's frames and the same walk of
//! the older records, without the newest record's header read, header walk, frame skip and bytes before n's frames. The
//! paired difference `read.op − read.op.at` is the cost of locating n's frames in the newest record (R-42 Q2).
//!
//! **What differs from `get`.** The buffer is the caller's and is reused: only its allocation carries over, and the bytes
//! it holds are dropped at each record's start, so a read never reuses an earlier record's I/O. No trailer is read; no
//! frame of another owner is decoded, and none past the held window is read; no body is read. It does not re-validate:
//! the replay's scan validated every record ([F05 §5.2]), and a replay never hashes bodies ([F06 §8] BD-2,
//! `06-commit.md:1053-1054`). So both reads return what `get` returns for every (n, key) on a valid tail; on a corrupt
//! record they may differ, because `get` decodes every op of the record.

use super::{Key, NodeEntry, Overlay, Probe, Value, sets};
use crate::tail::commit::{Op, presence};
use crate::tail::enc::{Bound, DecodeError, Reader};
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
            Ok::<_, DecodeError>((len, body, r.uvar32()?))
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

/// One entry of the offset index: [`NodeEntry`]'s layout with its 4 spare bytes used by `at` (R-42 Q2).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct OpEntry {
    /// The owner `#N`.
    pub n: u32,
    /// Bits 0–23: the offset of n's first frame from the record's lsn, header included; bits 24–31: n's frame count in
    /// that record ([`pack`]).
    pub at: u32,
    /// The lsn of the newest `Commit` whose ops have `#N` as owner.
    pub lsn: u64,
}

// The offset and the frame count cost no RAM beyond the node entry's 16 B (`AR:709`).
const _: () = assert!(
    std::mem::size_of::<OpEntry>() == std::mem::size_of::<NodeEntry>()
        && std::mem::size_of::<NodeEntry>() == 16
);

/// The first offset that does not fit `at`'s 24 bits.
pub const OFFSET_LIMIT: u64 = 1 << 24;
/// The most frames `at`'s 8 bits count.
pub const FRAMES_MAX: usize = 255;

/// Packs the offset of n's first frame and n's frame count into `at`; `None` when the offset is 2^24 or more, or the
/// count is not 1–255. The count is needed: an owner may have several frames in one record (F06 §7.9 orders them by
/// rank), and a reader at its last frame cannot tell the next owner's frame from `n_bodies` without it.
pub const fn pack(off: u64, frames: usize) -> Option<u32> {
    if off >= OFFSET_LIMIT || frames == 0 || frames > FRAMES_MAX {
        None
    } else {
        Some(((frames as u32) << 24) | off as u32)
    }
}

/// The inverse of [`pack`]: (the offset, the frame count).
pub const fn unpack(at: u32) -> (u64, usize) {
    ((at & 0x00FF_FFFF) as u64, (at >> 24) as usize)
}

/// The offset index: a copy of the node index whose entries also say where n's frames start in its newest record and
/// how many there are, sorted by `#N`. The probe's setup state; in the design these are the node entry's own bytes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpIndex {
    entries: Box<[OpEntry]>,
}

impl OpIndex {
    /// Locates each node entry's frames in its newest record with the frame walk (`chunk` and `buf` as
    /// [`Overlay::get_op`]'s). Refuses, naming `#n` and the lsn, a record with no frame of n, an offset of 2^24 or
    /// more and more than 255 frames: a silent fallback would no longer be the floor the record names.
    pub fn build<S: LogSource>(
        o: &Overlay,
        source: &mut S,
        chunk: usize,
        buf: &mut Vec<u8>,
    ) -> Result<OpIndex, String> {
        let mut entries = Vec::with_capacity(o.nodes.len());
        for e in o.nodes.iter() {
            let Some(s) = locate(source, buf, chunk, e.lsn, e.n)? else {
                return Err(format!(
                    "the record at lsn {} holds no op of #{}",
                    e.lsn, e.n
                ));
            };
            let off = s.first - e.lsn;
            let at = pack(off, s.frames).ok_or_else(|| {
                format!(
                    "#{}'s {} frames at lsn {} + {off} do not fit the entry's 24-bit offset and 8-bit count",
                    e.n, s.frames, e.lsn
                )
            })?;
            entries.push(OpEntry {
                n: e.n,
                at,
                lsn: e.lsn,
            });
        }
        Ok(OpIndex {
            entries: entries.into_boxed_slice(),
        })
    }

    /// The entries, sorted by `#N`.
    pub fn entries(&self) -> &[OpEntry] {
        &self.entries
    }
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

    /// The value of `key` of node `n`, as [`Overlay::get`] gives it on a valid tail: the offset read of the module doc
    /// in n's newest record, located by `idx` (an [`OpIndex`] of this overlay), then the frame walk (`chunk`, `buf`).
    pub fn get_op_at<S: LogSource>(
        &self,
        idx: &OpIndex,
        source: &mut S,
        n: u32,
        key: &Key,
        chunk: usize,
        buf: &mut Vec<u8>,
    ) -> Result<Probe, String> {
        let Ok(i) = idx.entries.binary_search_by_key(&n, |e| e.n) else {
            return Ok(Probe::NotInTail);
        };
        let OpEntry { lsn, at, .. } = idx.entries[i];
        let (off, frames) = unpack(at);
        let first = lsn + off;
        let mut w = Window::new(source, buf, 0, lsn, first, u64::MAX);
        let mut q = first;
        for k in 0..frames {
            let mut r = Reader::new(w.hold(first, q, FRAME_HEAD_MAX)?);
            let head = (|| {
                r.u8()?;
                let len = r.uvar32()?;
                Ok::<_, DecodeError>(r.pos() as u64 + u64::from(len))
            })();
            let end = q + head.map_err(|e| format!("lsn {lsn}: {}", e.what))?;
            let next = if k + 1 < frames { FRAME_HEAD_MAX } else { 0 };
            w.hold(first, q, (end - q) as usize + next)?;
            q = end;
        }
        let bytes = w.hold(first, first, (q - first) as usize)?;
        match step(bytes, frames, n, key, lsn)? {
            Step::Value(v) => Ok(Probe::Tail(v)),
            Step::Prev(p) => match self.older(lsn, p) {
                Some(l) => self.walk_op(source, l, n, key, chunk, buf),
                None => Ok(Probe::NotInTail),
            },
        }
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
    use super::super::Base;
    use super::super::naive::Naive;
    use super::*;
    use crate::tail::commit::Commit;
    use crate::tail::rec::{Payload, Record};
    use crate::tail::scan::MemLog;
    use crate::tail::synth::{Clause, Target, generate, tests::test_target};
    use crate::tail::testdata::{self, CHAIN_IN, START};
    use crate::tail::{Params, TAIL_READ_CHUNK};
    use std::collections::BTreeMap;

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

        /// The frame walk and the offset read equal the whole-record read on generated tails, at a random chunk.
        #[test]
        fn op_read_equals_the_whole_record_read_on_generated_tails(
            seed in proptest::prelude::any::<u64>(),
            chunk in 1usize..=4_096,
        ) {
            let p = generated(seed);
            let mut buf = Vec::new();
            let idx = OpIndex::build(&p.overlay, &mut MemLog::new(p.start, &p.bytes), chunk, &mut buf).unwrap();
            for (n, key) in keys(&p) {
                let want = p.overlay.get(&mut MemLog::new(p.start, &p.bytes), n, &key);
                let got = p.overlay.get_op(&mut MemLog::new(p.start, &p.bytes), n, &key, chunk, &mut buf);
                proptest::prop_assert_eq!(&got, &want, "#{} {:?}", n, key);
                let at = p.overlay.get_op_at(&idx, &mut MemLog::new(p.start, &p.bytes), n, &key, chunk, &mut buf);
                proptest::prop_assert_eq!(&at, &want, "the offset read: #{} {:?}", n, key);
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

    /// The bytes of `ops`' frames.
    fn span_of(ops: &[&Op<'_>]) -> u64 {
        let (mut out, mut scratch) = (Vec::new(), Vec::new());
        for op in ops {
            op.encode(&mut out, &mut scratch).unwrap();
        }
        out.len() as u64
    }

    /// The offset read returns what the whole-record read returns ([`Overlay::get`]) for the keys of
    /// `op_read_equals_the_whole_record_read`, at chunks of 1, 64 and 64 KiB; the offset index's (n, lsn) are the node
    /// index's; and each entry's `frames` frames at `lsn + off` decode to exactly `Commit::decode`'s ops of n in that
    /// record. The hand-built tail's #3 has two frames in its newest record (`Move`, then `SetField` 4).
    #[test]
    fn offset_read_equals_the_whole_record_read() {
        let mut buf = Vec::new();
        for p in probed() {
            let mut src = MemLog::new(p.start, &p.bytes);
            let idx = OpIndex::build(&p.overlay, &mut src, TAIL_READ_CHUNK, &mut buf).unwrap();
            for (n, key) in keys(&p) {
                let want = p.overlay.get(&mut MemLog::new(p.start, &p.bytes), n, &key);
                for chunk in [1, 64, TAIL_READ_CHUNK] {
                    let got = p.overlay.get_op_at(
                        &idx,
                        &mut MemLog::new(p.start, &p.bytes),
                        n,
                        &key,
                        chunk,
                        &mut buf,
                    );
                    assert_eq!(got, want, "{} chunk {chunk} #{n} {key:?}", p.name);
                }
            }
            let pairs: Vec<(u32, u64)> = idx.entries().iter().map(|e| (e.n, e.lsn)).collect();
            let nodes: Vec<(u32, u64)> = p.overlay.nodes().iter().map(|e| (e.n, e.lsn)).collect();
            assert_eq!(pairs, nodes, "{}", p.name);
            let by_lsn: BTreeMap<u64, &Commit<'_>> = commits(&p.records).into_iter().collect();
            let mut multi = 0;
            for e in idx.entries() {
                let (off, frames) = unpack(e.at);
                let want: Vec<&Op<'_>> = by_lsn[&e.lsn]
                    .ops
                    .iter()
                    .filter(|o| o.owner() == e.n)
                    .collect();
                let mut r = Reader::new(&p.bytes[(e.lsn + off - p.start) as usize..]);
                let got: Vec<Op<'_>> = (0..frames).map(|_| Op::decode(&mut r).unwrap()).collect();
                assert_eq!(got.iter().collect::<Vec<_>>(), want, "{} #{}", p.name, e.n);
                multi += usize::from(frames > 1);
            }
            assert!(p.name != "hand-built" || multi >= 1, "#3's two frames");
        }
    }

    /// The offset read touches only n's frames in its newest record: over a `MemLog`, for every entry of the tails of
    /// `offset_read_equals_the_whole_record_read` and for each key n's ops there set, `Status` and a key never set, the
    /// first read is [`FRAME_HEAD_MAX`] bytes at `lsn + off`; the reads at the newest record lie in
    /// `[lsn + off, lsn + off + span + 11)` (span: n's frames' bytes), are at most frames + 1, and none is at `lsn`;
    /// when the key is not in the newest record, the later reads are the frame walk's from `lsn − prev`, and there are
    /// none when `prev` leaves the tail. On a copy whose newest record is 0xFF outside n's frames, its header included,
    /// every key n's ops set there still reads `get`'s value on the clean log.
    #[test]
    fn offset_read_touches_only_the_owners_frames_in_the_newest_record() {
        let mut buf = Vec::new();
        let mut walk = Vec::new();
        // The first failure of each clause, so that one run shows every clause that fails.
        let mut failed: BTreeMap<&str, String> = BTreeMap::new();
        let mut walked = 0;
        for p in probed() {
            let mut src = MemLog::new(p.start, &p.bytes);
            let idx = OpIndex::build(&p.overlay, &mut src, TAIL_READ_CHUNK, &mut buf).unwrap();
            let by_lsn: BTreeMap<u64, (&Record<'_>, &Commit<'_>)> = p
                .records
                .iter()
                .filter_map(|r| match &r.payload {
                    Payload::Commit(c) => Some((r.lsn, (r, &**c))),
                    _ => None,
                })
                .collect();
            for e in idx.entries() {
                let (off, frames) = unpack(e.at);
                let first = e.lsn + off;
                let (rec, c) = by_lsn[&e.lsn];
                let own: Vec<&Op<'_>> = c.ops.iter().filter(|o| o.owner() == e.n).collect();
                let span = span_of(&own);
                let set: Vec<Key> = own.iter().flat_map(|op| sets(op)).map(|(k, _)| k).collect();
                let mut ks = set.clone();
                ks.extend([Key::Status, Key::Field(99)]);
                for key in ks {
                    let what = format!("{} #{} {key:?}", p.name, e.n);
                    let mut src = MemLog::new(p.start, &p.bytes);
                    let got =
                        p.overlay
                            .get_op_at(&idx, &mut src, e.n, &key, TAIL_READ_CHUNK, &mut buf);
                    let want = p
                        .overlay
                        .get(&mut MemLog::new(p.start, &p.bytes), e.n, &key);
                    if got != want {
                        failed
                            .entry("value")
                            .or_insert(format!("{what}: {got:?}, not {want:?}"));
                    }
                    if src.reads.first() != Some(&(first, FRAME_HEAD_MAX)) {
                        failed.entry("first read").or_insert(format!(
                            "{what}: the first read is {:?}, not ({first}, {FRAME_HEAD_MAX})",
                            src.reads.first()
                        ));
                    }
                    let k = src.reads.iter().take_while(|r| r.0 >= e.lsn).count();
                    let (newest, rest) = src.reads.split_at(k);
                    let top = first + span + FRAME_HEAD_MAX as u64;
                    if newest.len() > frames + 1
                        || newest
                            .iter()
                            .any(|&(a, l)| a == e.lsn || a < first || a + l as u64 > top)
                    {
                        failed.entry("newest reads").or_insert(format!(
                            "{what}: reads {newest:?} outside [{first}, {top}) or more than {} of them",
                            frames + 1
                        ));
                    }
                    let mut later = MemLog::new(p.start, &p.bytes);
                    if let (false, Some(l)) =
                        (set.contains(&key), p.overlay.older(e.lsn, own[0].prev()))
                    {
                        let _ =
                            p.overlay
                                .walk_op(&mut later, l, e.n, &key, TAIL_READ_CHUNK, &mut walk);
                        walked += 1;
                    }
                    if rest != later.reads.as_slice() {
                        failed.entry("later reads").or_insert(format!(
                            "{what}: the reads after the newest record {rest:?}, not the frame walk's {:?}",
                            later.reads
                        ));
                    }
                }
                let mut ff = p.bytes.clone();
                let at = (e.lsn - p.start) as usize;
                let len = rec.encoded_len().unwrap() as usize;
                for (i, b) in ff[at..at + len].iter_mut().enumerate() {
                    let lsn = e.lsn + i as u64;
                    if lsn < first || lsn >= first + span {
                        *b = 0xFF;
                    }
                }
                for key in &set {
                    let want = p.overlay.get(&mut MemLog::new(p.start, &p.bytes), e.n, key);
                    let got = p.overlay.get_op_at(
                        &idx,
                        &mut MemLog::new(p.start, &ff),
                        e.n,
                        key,
                        TAIL_READ_CHUNK,
                        &mut buf,
                    );
                    if got != want {
                        failed.entry("0xFF copy").or_insert(format!(
                            "{} #{} {key:?} on the 0xFF copy: {got:?}, not {want:?}",
                            p.name, e.n
                        ));
                    }
                }
            }
        }
        assert!(
            failed.is_empty(),
            "{}",
            failed.into_values().collect::<Vec<_>>().join("\n")
        );
        assert!(walked > 20, "{walked} reads went on past the newest record");
    }

    /// The offset read refuses a frame of another owner: the index is not this log's (module doc). For every two
    /// owners a and b with the same newest record in the tails of `offset_read_equals_the_whole_record_read`, an index
    /// whose entry of a holds b's `at` makes every read of a (`Status`, `Exist`, `Hier`, `Field(1)`) fail with "a frame
    /// of #b where #a's was located", never a value built from b's frames.
    #[test]
    fn offset_read_refuses_another_owners_frames() {
        let mut buf = Vec::new();
        let mut tried = 0;
        let mut failed = Vec::new();
        for p in probed() {
            let mut src = MemLog::new(p.start, &p.bytes);
            let idx = OpIndex::build(&p.overlay, &mut src, TAIL_READ_CHUNK, &mut buf).unwrap();
            let es = idx.entries();
            for (i, a) in es.iter().enumerate() {
                let Some(b) = es.iter().find(|b| b.lsn == a.lsn && b.n != a.n) else {
                    continue;
                };
                let mut v = es.to_vec();
                v[i].at = b.at;
                let bad = OpIndex {
                    entries: v.into_boxed_slice(),
                };
                let want = Err(format!(
                    "lsn {}: a frame of #{} where #{}'s was located",
                    a.lsn, b.n, a.n
                ));
                for key in [Key::Status, Key::Exist, Key::Hier, Key::Field(1)] {
                    tried += 1;
                    let got = p.overlay.get_op_at(
                        &bad,
                        &mut MemLog::new(p.start, &p.bytes),
                        a.n,
                        &key,
                        TAIL_READ_CHUNK,
                        &mut buf,
                    );
                    if got != want {
                        failed.push(format!(
                            "{} #{} {key:?} at #{}'s frames: {got:?}",
                            p.name, a.n, b.n
                        ));
                    }
                }
            }
        }
        assert!(tried > 100, "{tried} reads of a shared newest record");
        assert!(
            failed.is_empty(),
            "{} of {tried} reads not refused, the first: {}",
            failed.len(),
            failed[0]
        );
    }

    /// `at` packs a 24-bit offset and an 8-bit frame count: `Some` at (32, 1) and (2^24 − 1, 255), `None` at (2^24, 1),
    /// (32, 0) and (32, 256); `unpack` inverts `pack`; an entry is 16 bytes.
    #[test]
    fn offset_entry_packs_the_offset_and_the_frame_count() {
        assert_eq!(pack(32, 1), Some((1 << 24) | 32));
        assert_eq!(pack(OFFSET_LIMIT - 1, FRAMES_MAX), Some(u32::MAX));
        for (off, k) in [(OFFSET_LIMIT, 1), (32, 0), (32, FRAMES_MAX + 1)] {
            assert_eq!(pack(off, k), None, "({off}, {k})");
        }
        for (off, k) in [(32, 1), (77, 3), (OFFSET_LIMIT - 1, FRAMES_MAX)] {
            assert_eq!(unpack(pack(off, k).unwrap()), (off, k));
        }
        assert_eq!(std::mem::size_of::<OpEntry>(), 16);
    }
}
