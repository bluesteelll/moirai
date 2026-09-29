//! The histogram line diff HD of [F12 §7.5], exactly: the matched pairs that M3's diff3 walks ([F12 §7.5] "diff3"),
//! and the line mapping of the anchors and the replay ([40 §2.7], [AR §2.10] T10).
//!
//! HD(p0, p1, q0, q1) matches the common prefix and suffix of the box P[p0..p1) × Q[q0..q1); then, among the maximal
//! matching regions of the box whose rarity (the least count in P[p0..p1) of their lines) is at most
//! [`MAX_RARITY`], it takes the one least by (rarity, −length, j, i), matches it, and recurses on the boxes before and
//! after it. With no candidate the box matches nothing.
//!
//! **Implementation.** A region is a *candidate* of a box when it is maximal there and its rarity is at most 64; its
//! *key* packs (rarity, −length, j, i) so that integer order is [F12 §7.5]'s order. The engine never recurses: it
//! runs *chains*. A chain starts on a box with a *scan*: the counts of the box's part of P, then every candidate,
//! found through the rare pairs (`P[i] = Q[j]`, count ≤ 64), one region per diagonal run. It then repeats: take the
//! least candidate, match it, and continue on the **larger** child box while the smaller one waits for a chain of
//! its own. Moving to a child removes the lines outside it from the counts.
//!
//! The *region heap* holds lower bounds: every candidate of the current box has an entry no greater than its key,
//! in the region heap or the group heap below, or its key is at least the *threshold* (the least key a memory trim
//! let go). A popped region entry is checked against its region clipped to the current box: equal, it is the least
//! candidate; greater, it goes back with the true key; an empty clip or a rarity above 64 drops it. A candidate
//! whose least count belongs to an unaffected id keeps a key at least its parent region's, because that count is
//! unchanged, a subset of lines has no smaller minimum, and a shorter region orders later; so entries never need
//! eager updates. When nothing below the threshold remains, the box is scanned again.
//!
//! **Groups.** An id v whose count a step lowers to 64 or less (an *affected* id) can give its regions a smaller
//! key. HD's choice is also the least (count of v, −length, j, i) over the rare pairs (x, y) of the box, with v =
//! P\[x\] = Q\[y\] and the region through the pair: the least candidate has the count of one of its lines as its
//! rarity, and a pair's region has a rarity at most the pair's count. So v's candidates move together when its
//! count falls, and v is handled as a *group*:
//!
//! - Every line y of Q holding v has a *slot*: the least (−length, j, i) of a region through a pair (x, y) of the
//!   box. A region of a child box is the clip of the parent's region through the same pair, and clipping raises the
//!   key or empties the region, so a slot stays a lower bound as the box shrinks.
//! - The *group heap* holds an entry (count, a lower bound of v's least slot) per affected id. Until v's slots are
//!   built, the bound is the box's *floor*, a slot longer than any region of the box.
//! - v's slots form a min-heap, built when v's floor entry reaches the top, in one pass over the lines of Q of v
//!   and of every id whose equal floor entry stands next (the ids one step affected together).
//! - Popped, an entry whose count is current has v's top slot evaluated again: unchanged, its region is the least
//!   candidate; else it sinks, and the entry goes back with the true least slot. A count change pushes a new
//!   entry; the old one is dropped when popped.
//!
//! Regions are measured through a cache that keeps, per diagonal, the last run measured on it: a scan resets its
//! box's diagonals and measures each run once, and a build walks its lines of Q in increasing order, so each
//! diagonal forward. Any two boxes of HD are nested or disjoint, and a box is handled after its ancestors, so a
//! cached run through a pair of a box was measured in that box or an ancestor, and clips to the box's region.
//!
//! **Cost.** A line leaves its chain only into a smaller child, so it is counted and scanned in O(log n) chains. A
//! scan visits the rare pairs of its box, at most 64 per line of Q, and measures each run once. A step costs its
//! removed lines plus one group entry per affected id. A group is built once per chain, at one lookup per pair of
//! its id in the box; a slot evaluation costs cnt(v) ≤ 64 lookups; a lookup costs O(1) on a cached run, else the
//! run's length. So a chain that lowers many counts at every step (a cycle of lines each repeated 64 times,
//! against its reversal) builds each group once instead of measuring 64 · 64 pairs per id at every step, and
//! chains that peel one region at a time stay near O(n log n). Two costs are not bounded by these terms: the
//! rescans after a memory trim, and the slot evaluations, since a slot is evaluated again each time a step clips its
//! best region. The worst shapes found (every count at 64, a line with 64 copies facing 32,768, random lines about
//! the limit) take milliseconds at the 65,536-byte bound of a text value, and usual edits microseconds;
//! `tests/perf.rs` (`worst_case_timings`) measures them.
//!
//! **Memory.** O(|P| + |Q|) words plus O(max id): occurrence indexes of P and Q with one row per id, one partner per
//! line of P (pairs are read out in order at the end), one cached run per diagonal, one slot per occurrence in Q of
//! an id of P once a group is built, the lines of one build, the waiting boxes, the affected and built ids of a
//! chain, a region heap capped at max(4,096, (|P| + |Q|) / 16) entries after a trim, and a group heap compacted to
//! its live entries (at most one per affected id) whenever it doubles.

use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::ops::Range;

use crate::LengthError;

/// `DIFF_MAX_RARITY` ([F12 §7.5]): a region is a candidate only if one of its lines occurs at most this often in the
/// box's part of P. Part of format v1: a merged text enters commit ids.
pub const MAX_RARITY: u32 = 64;

/// The longest sequence the diff accepts: positions and lengths are `u32`, and `u32::MAX` is [`UNMATCHED`].
pub const MAX_LEN: usize = u32::MAX as usize - 1;

/// The partner of an unmatched line in [`partners`].
pub const UNMATCHED: u32 = u32::MAX;

/// One matched pair (i, j) of HD: `P[i] = Q[j]` ([F12 §7.5]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Match {
    /// The line index in P.
    pub p: u32,
    /// The line index in Q.
    pub q: u32,
}

/// A maximal run of unmatched lines between two consecutive matched pairs (or an end): P[p_start..p_end) was replaced
/// by Q[q_start..q_end). At least one side is non-empty.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Hunk {
    /// First unmatched line of P.
    pub p_start: u32,
    /// End (exclusive) of the unmatched lines of P.
    pub p_end: u32,
    /// First unmatched line of Q.
    pub q_start: u32,
    /// End (exclusive) of the unmatched lines of Q.
    pub q_end: u32,
}

impl Hunk {
    /// The unmatched lines of P.
    #[must_use]
    pub fn p_range(&self) -> Range<usize> {
        self.p_start as usize..self.p_end as usize
    }

    /// The unmatched lines of Q.
    #[must_use]
    pub fn q_range(&self) -> Range<usize> {
        self.q_start as usize..self.q_end as usize
    }
}

/// The hunks between the matched pairs of a diff of sequences of lengths `p_len` and `q_len`, in order.
///
/// `matches` must be strictly increasing in both coordinates and inside the sequences, as [`Differ::diff`] returns
/// them; other input yields meaningless hunks but never panics.
#[must_use]
pub fn hunks(matches: &[Match], p_len: usize, q_len: usize) -> Hunks<'_> {
    let clamp = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
    Hunks {
        matches,
        next: 0,
        p: 0,
        q: 0,
        p_len: clamp(p_len),
        q_len: clamp(q_len),
        done: false,
    }
}

/// The iterator of [`hunks`].
#[derive(Clone, Debug)]
pub struct Hunks<'m> {
    matches: &'m [Match],
    next: usize,
    p: u32,
    q: u32,
    p_len: u32,
    q_len: u32,
    done: bool,
}

impl Iterator for Hunks<'_> {
    type Item = Hunk;

    fn next(&mut self) -> Option<Hunk> {
        while !self.done {
            let (p_end, q_end) = match self.matches.get(self.next) {
                Some(m) => (m.p, m.q),
                None => {
                    self.done = true;
                    (self.p_len, self.q_len)
                }
            };
            let hunk = Hunk {
                p_start: self.p,
                p_end: p_end.max(self.p),
                q_start: self.q,
                q_end: q_end.max(self.q),
            };
            if !self.done {
                self.next += 1;
                self.p = p_end.saturating_add(1);
                self.q = q_end.saturating_add(1);
            }
            if hunk.p_end > hunk.p_start || hunk.q_end > hunk.q_start {
                return Some(hunk);
            }
        }
        None
    }
}

impl std::iter::FusedIterator for Hunks<'_> {}

/// Fills `out` with the partner of each of the `len` lines of P: `out[i]` = j for a pair (i, j), else [`UNMATCHED`].
/// diff3 reads ma(i) and mb(i) this way ([F12 §7.5]).
pub fn partners(matches: &[Match], len: usize, out: &mut Vec<u32>) {
    out.clear();
    out.resize(len, UNMATCHED);
    for m in matches {
        if let Some(slot) = out.get_mut(m.p as usize) {
            *slot = m.q;
        }
    }
}

/// HD of two sequences of line ids, as a new vector ([`Differ::diff`]).
pub fn diff(p: &[u32], q: &[u32]) -> Result<Vec<Match>, LengthError> {
    let mut out = Vec::new();
    Differ::new().diff(p, q, &mut out)?;
    Ok(out)
}

/// HD of the lines of two texts ([F12 §7.5] lines(·)), as a new vector.
pub fn diff_lines(p: &[u8], q: &[u8]) -> Result<Vec<Match>, LengthError> {
    let mut int = crate::Interner::new();
    let (mut pi, mut qi) = (Vec::new(), Vec::new());
    int.intern_lines(p, &mut pi)?;
    int.intern_lines(q, &mut qi)?;
    diff(&pi, &qi)
}

/// Whether a sequence of `len` lines fits the diff's `u32` positions.
fn fits(len: usize) -> bool {
    len <= MAX_LEN
}

/// Whether ids below `bound` are dense enough for tables indexed by id, for `lines` lines of input in all.
fn dense(bound: usize, lines: usize) -> bool {
    bound <= lines.saturating_mul(4).saturating_add(4096)
}

/// No key: greater than every packed key, whose rarity field is at most 64, and every slot, whose length is at least 1.
const NO_KEY: u128 = u128::MAX;

/// Packs a region so that `u128` order is [F12 §7.5]'s order (rarity, −len, j, i).
#[inline]
fn pack(rarity: u32, len: usize, i: usize, j: usize) -> u128 {
    (u128::from(rarity) << 96)
        | (u128::from(u32::MAX - len as u32) << 64)
        | ((j as u128) << 32)
        | i as u128
}

/// The region (i, j, len) of a packed key.
#[inline]
fn unpack(key: u128) -> (usize, usize, usize) {
    let i = key as u32 as usize;
    let j = (key >> 32) as u32 as usize;
    let len = (u32::MAX - (key >> 64) as u32) as usize;
    (i, j, len)
}

/// The rarity field of a packed key.
#[inline]
fn rarity_of(key: u128) -> u32 {
    (key >> 96) as u32
}

/// Packs the region (i, j, len) through line y of Q so that `u128` order is (−len, j, i), then y: a slot.
#[inline]
fn slot(i: usize, j: usize, len: usize, y: usize) -> u128 {
    (pack(0, len, i, j) << 32) | y as u128
}

/// The line of Q a slot belongs to.
#[inline]
fn line_of(s: u128) -> usize {
    s as u32 as usize
}

/// The key of a slot's region with the rarity field `cnt`.
#[inline]
fn keyed(cnt: u32, s: u128) -> u128 {
    (u128::from(cnt) << 96) | (s >> 32)
}

/// A group heap entry: a key whose rarity field is id v's count and whose region is a lower bound of v's least slot,
/// ordered by key, then id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Entry {
    hi: u64,
    lo: u64,
    v: u32,
}

impl Entry {
    fn new(key: u128, v: usize) -> Self {
        Self {
            hi: (key >> 64) as u64,
            lo: key as u64,
            v: v as u32,
        }
    }

    fn key(self) -> u128 {
        (u128::from(self.hi) << 64) | u128::from(self.lo)
    }
}

/// A box P[p0..p1) × Q[q0..q1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Area {
    p0: usize,
    p1: usize,
    q0: usize,
    q1: usize,
}

impl Area {
    /// Lines on both sides.
    fn size(self) -> usize {
        (self.p1 - self.p0) + (self.q1 - self.q0)
    }

    /// Both sides are non-empty: the box can hold a pair.
    fn is_open(self) -> bool {
        self.p0 < self.p1 && self.q0 < self.q1
    }

    /// The diagonals x + qn − y of the box's pairs, for a Q of `qn` lines.
    fn diagonals(self, qn: usize) -> Range<usize> {
        self.p0 + qn + 1 - self.q1..self.p1 + qn - self.q0
    }

    /// A slot below every slot of the box: longer than its longest region.
    fn floor(self) -> u128 {
        slot(0, 0, (self.p1 - self.p0).min(self.q1 - self.q0) + 1, 0)
    }

    fn to_words(self) -> [u32; 4] {
        [self.p0, self.p1, self.q0, self.q1].map(|x| x as u32)
    }

    fn from_words(a: [u32; 4]) -> Self {
        let [p0, p1, q0, q1] = a.map(|x| x as usize);
        Self { p0, p1, q0, q1 }
    }
}

/// Which child of a split continues the chain. Every choice gives the same pairs; the larger child bounds the work.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Inherit {
    Larger,
    #[cfg(test)]
    Smaller,
    #[cfg(test)]
    Left,
    #[cfg(test)]
    Right,
}

/// The region heap keeps `max(min_cap, lines / cap_div)` entries after a trim, where `lines` counts both inputs;
/// the group heap is compacted when it grows past `max(group_cap, twice its size after the last compaction)`.
#[derive(Clone, Copy, Debug)]
struct Tuning {
    min_cap: usize,
    cap_div: usize,
    group_cap: usize,
    inherit: Inherit,
}

impl Tuning {
    const DEFAULT: Self = Self {
        min_cap: 4096,
        cap_div: 16,
        group_cap: 1024,
        inherit: Inherit::Larger,
    };
}

/// The `live` of an id whose slots are not built in the current chain.
const UNBUILT: u32 = u32::MAX;

/// Per id: its count in the current box's part of P; where its positions start in the occurrence indexes of P and Q
/// (row v + 1 holds the ends), which is also where its slots start; and how many slots its heap holds.
#[derive(Clone, Copy, Debug)]
struct Row {
    cnt: u32,
    p: u32,
    q: u32,
    live: u32,
}

const EMPTY_ROW: Row = Row {
    cnt: 0,
    p: 0,
    q: 0,
    live: UNBUILT,
};

/// A run of matches on one diagonal: the pairs (x − y + j + k, j + k) for j ≤ j + k < end, for any pair (x, y) of it.
#[derive(Clone, Copy, Debug, Default)]
struct Run {
    j: u32,
    end: u32,
}

/// Work counters, for the complexity tests.
#[cfg(test)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Stats {
    /// Chains started, and boxes scanned (each chain's first scan included).
    chains: usize,
    scans: usize,
    /// Lines of P counted and lines of Q walked by scans.
    scanned: usize,
    /// Regions offered by scans.
    regions: usize,
    /// Lines of P that steps removed from the counts.
    removed: usize,
    /// Entries popped from both heaps, region heap trims, and group heap compactions.
    pops: usize,
    trims: usize,
    compactions: usize,
    /// Groups built, and slots evaluated again after their build.
    builds: usize,
    evals: usize,
    /// Pair lookups by builds and evaluations, runs measured, and the lines those measurements walked.
    lookups: usize,
    runs: usize,
    walked: usize,
}

/// The scratch of the diff engine, reused across calls.
#[derive(Clone, Debug)]
struct Engine {
    /// `bound + 1` rows: ids below `bound` and a final row with the ends.
    rows: Vec<Row>,
    /// Positions of each id in P, increasing, at `rows[v].p..rows[v + 1].p`; the same for Q (ids of P only).
    p_pos: Vec<u32>,
    q_pos: Vec<u32>,
    /// `part[i]`: the partner of P's line i, or [`UNMATCHED`].
    part: Vec<u32>,
    /// Per diagonal x − y + |Q|: the last run measured on it.
    runs: Vec<Run>,
    /// Per occurrence in `q_pos`, once a group is built: v's slots form a min-heap at
    /// `rows[v].q..rows[v].q + rows[v].live`.
    slots: Vec<u128>,
    heap: BinaryHeap<Reverse<u128>>,
    thresh: u128,
    cap: usize,
    groups: BinaryHeap<Reverse<Entry>>,
    group_cap: usize,
    /// Boxes waiting for a chain.
    pending: Vec<[u32; 4]>,
    affected: Vec<u32>,
    /// Ids whose slots the current chain built.
    built: Vec<u32>,
    /// The lines of Q of a build, each with the index of its slot.
    lines: Vec<(u32, u32)>,
    tuning: Tuning,
    #[cfg(test)]
    stats: Stats,
}

/// The histogram diff, with scratch buffers that are reused from one call to the next ([F12 §7.5] HD).
///
/// Ids should be dense, as an [`crate::Interner`] assigns them: the tables are sized by the largest id of P. Sparse
/// ids (larger than 4 × (|P| + |Q|) + 4096) are first renumbered by rank, so memory stays linear in any case.
#[derive(Clone, Debug)]
pub struct Differ {
    engine: Engine,
    ids: Vec<u32>,
    p_ids: Vec<u32>,
    q_ids: Vec<u32>,
}

impl Default for Differ {
    fn default() -> Self {
        Self::new()
    }
}

impl Differ {
    /// A differ with empty scratch.
    #[must_use]
    pub fn new() -> Self {
        Self::with_tuning(Tuning::DEFAULT)
    }

    fn with_tuning(tuning: Tuning) -> Self {
        Self {
            engine: Engine {
                rows: Vec::new(),
                p_pos: Vec::new(),
                q_pos: Vec::new(),
                part: Vec::new(),
                runs: Vec::new(),
                slots: Vec::new(),
                heap: BinaryHeap::new(),
                thresh: NO_KEY,
                cap: 1,
                groups: BinaryHeap::new(),
                group_cap: tuning.group_cap,
                pending: Vec::new(),
                affected: Vec::new(),
                built: Vec::new(),
                lines: Vec::new(),
                tuning,
                #[cfg(test)]
                stats: Stats::default(),
            },
            ids: Vec::new(),
            p_ids: Vec::new(),
            q_ids: Vec::new(),
        }
    }

    /// Replaces `out` with HD(P, Q): the matched pairs, strictly increasing in both coordinates ([F12 §7.5]). Fails
    /// only when a sequence is longer than [`MAX_LEN`].
    pub fn diff(&mut self, p: &[u32], q: &[u32], out: &mut Vec<Match>) -> Result<(), LengthError> {
        out.clear();
        if !fits(p.len().max(q.len())) {
            return Err(LengthError);
        }
        let Some(&max_id) = p.iter().max() else {
            return Ok(());
        };
        if q.is_empty() {
            return Ok(());
        }
        let bound = max_id as usize + 1;
        if dense(bound, p.len() + q.len()) {
            self.engine.run(p, q, bound, out);
        } else {
            self.renumber(p, q);
            self.engine
                .run(&self.p_ids, &self.q_ids, self.ids.len(), out);
        }
        Ok(())
    }

    /// Renumbers the ids of P by rank among P's distinct ids; an id of Q absent from P gets the rank count, which no
    /// line of P has.
    fn renumber(&mut self, p: &[u32], q: &[u32]) {
        self.ids.clear();
        self.ids.extend_from_slice(p);
        self.ids.sort_unstable();
        self.ids.dedup();
        let ids = &self.ids;
        let absent = ids.len() as u32;
        let rank = |v: &u32| ids.binary_search(v).map_or(absent, |r| r as u32);
        self.p_ids.clear();
        self.p_ids.extend(p.iter().map(rank));
        self.q_ids.clear();
        self.q_ids.extend(q.iter().map(rank));
    }

    /// Bytes of scratch capacity held between calls.
    #[must_use]
    pub fn scratch_bytes(&self) -> usize {
        let e = &self.engine;
        let words = e.p_pos.capacity()
            + e.q_pos.capacity()
            + e.part.capacity()
            + e.affected.capacity()
            + e.built.capacity()
            + self.ids.capacity()
            + self.p_ids.capacity()
            + self.q_ids.capacity();
        words * size_of::<u32>()
            + e.rows.capacity() * size_of::<Row>()
            + e.runs.capacity() * size_of::<Run>()
            + e.slots.capacity() * size_of::<u128>()
            + e.pending.capacity() * size_of::<[u32; 4]>()
            + e.heap.capacity() * size_of::<u128>()
            + e.groups.capacity() * size_of::<Entry>()
            + e.lines.capacity() * size_of::<(u32, u32)>()
    }

    /// Frees the scratch buffers.
    pub fn release(&mut self) {
        *self = Self::with_tuning(self.engine.tuning);
    }
}

/// The indices into `pos` of the positions in `pos[s..e)` that lie in `r` (`pos` is increasing there).
fn within(pos: &[u32], s: u32, e: u32, r: Range<usize>) -> Range<usize> {
    let (s, e) = (s as usize, e as usize);
    let occ = &pos[s..e];
    let lo = occ.partition_point(|&x| (x as usize) < r.start);
    let hi = lo + occ[lo..].partition_point(|&x| (x as usize) < r.end);
    s + lo..s + hi
}

/// Restores the min-heap order of `h` below `at`.
fn sift_down(h: &mut [u128], mut at: usize) {
    loop {
        let left = 2 * at + 1;
        let Some(&l) = h.get(left) else {
            return;
        };
        let child = match h.get(left + 1) {
            Some(&r) if r < l => left + 1,
            _ => left,
        };
        if h[child] >= h[at] {
            return;
        }
        h.swap(at, child);
        at = child;
    }
}

/// Orders `h` as a min-heap.
fn heapify(h: &mut [u128]) {
    for at in (0..h.len() / 2).rev() {
        sift_down(h, at);
    }
}

impl Engine {
    fn run(&mut self, p: &[u32], q: &[u32], bound: usize, out: &mut Vec<Match>) {
        self.index(p, q, bound);
        self.part.clear();
        self.part.resize(p.len(), UNMATCHED);
        self.runs.clear();
        self.runs.resize(p.len() + q.len(), Run::default());
        // Sized by the first build.
        self.slots.clear();
        self.cap = ((p.len() + q.len()) / self.tuning.cap_div)
            .max(self.tuning.min_cap)
            .max(1);
        self.pending.push(
            Area {
                p0: 0,
                p1: p.len(),
                q0: 0,
                q1: q.len(),
            }
            .to_words(),
        );
        while let Some(a) = self.pending.pop() {
            self.chain(p, q, Area::from_words(a));
        }
        out.extend(
            self.part
                .iter()
                .enumerate()
                .filter(|&(_, &j)| j != UNMATCHED)
                .map(|(i, &j)| Match { p: i as u32, q: j }),
        );
    }

    /// The occurrence indexes of P and Q (a counting sort by id; ids of Q from `bound` up are left out), with all
    /// counts zero and no group built.
    fn index(&mut self, p: &[u32], q: &[u32], bound: usize) {
        self.rows.clear();
        self.rows.resize(bound + 1, EMPTY_ROW);
        for &v in p {
            self.rows[v as usize].p += 1;
        }
        for &v in q {
            if let Some(row) = self.rows[..bound].get_mut(v as usize) {
                row.q += 1;
            }
        }
        let (mut sp, mut sq) = (0, 0);
        for row in &mut self.rows {
            (row.p, sp) = (sp, sp + row.p);
            (row.q, sq) = (sq, sq + row.q);
        }
        self.p_pos.clear();
        self.p_pos.resize(sp as usize, 0);
        for (x, &v) in p.iter().enumerate() {
            let row = &mut self.rows[v as usize];
            self.p_pos[(row.p + row.cnt) as usize] = x as u32;
            row.cnt += 1;
        }
        self.rows.iter_mut().for_each(|r| r.cnt = 0);
        self.q_pos.clear();
        self.q_pos.resize(sq as usize, 0);
        for (y, &v) in q.iter().enumerate() {
            if let Some(row) = self.rows[..bound].get_mut(v as usize) {
                self.q_pos[(row.q + row.cnt) as usize] = y as u32;
                row.cnt += 1;
            }
        }
        self.rows.iter_mut().for_each(|r| r.cnt = 0);
    }

    /// The indices into `p_pos` of id v's positions in P[r].
    fn p_occ(&self, v: usize, r: Range<usize>) -> Range<usize> {
        within(&self.p_pos, self.rows[v].p, self.rows[v + 1].p, r)
    }

    /// The indices into `q_pos` of id v's positions in Q[r].
    fn q_occ(&self, v: usize, r: Range<usize>) -> Range<usize> {
        within(&self.q_pos, self.rows[v].q, self.rows[v + 1].q, r)
    }

    /// One chain from box `a`: HD of `a`, with the smaller child of every split left for a chain of its own.
    fn chain(&mut self, p: &[u32], q: &[u32], a: Area) {
        #[cfg(test)]
        {
            self.stats.chains += 1;
        }
        let mut a = self.strip(p, q, a);
        if !a.is_open() {
            return;
        }
        for &v in &p[a.p0..a.p1] {
            self.rows[v as usize].cnt += 1;
        }
        self.scan(p, q, a);
        while let Some(key) = self.choose(p, q, a) {
            let (i, j, len) = unpack(key);
            for k in 0..len {
                self.part[i + k] = (j + k) as u32;
            }
            let left = Area {
                p0: a.p0,
                p1: i,
                q0: a.q0,
                q1: j,
            };
            let right = Area {
                p0: i + len,
                p1: a.p1,
                q0: j + len,
                q1: a.q1,
            };
            let (next, later) = self.pick(
                left.is_open().then_some(left),
                right.is_open().then_some(right),
            );
            if let Some(later) = later {
                self.pending.push(later.to_words());
            }
            let Some(next) = next else {
                break;
            };
            // The children of a split have no common prefix or suffix: the chosen region is maximal, and the box
            // it came from had none.
            debug_assert!(p[next.p0] != q[next.q0] && p[next.p1 - 1] != q[next.q1 - 1]);
            self.step(p, a, next);
            a = next;
        }
        for &v in &p[a.p0..a.p1] {
            self.rows[v as usize].cnt = 0;
        }
        for &v in &self.built {
            self.rows[v as usize].live = UNBUILT;
        }
        self.built.clear();
        self.heap.clear();
        self.thresh = NO_KEY;
        self.groups.clear();
        self.group_cap = self.tuning.group_cap;
    }

    /// Matches the common prefix and suffix of `a` ([F12 §7.5]) and returns the rest.
    fn strip(&mut self, p: &[u32], q: &[u32], a: Area) -> Area {
        let Area {
            mut p0,
            mut p1,
            mut q0,
            mut q1,
        } = a;
        while p0 < p1 && q0 < q1 && p[p0] == q[q0] {
            self.part[p0] = q0 as u32;
            p0 += 1;
            q0 += 1;
        }
        while p0 < p1 && q0 < q1 && p[p1 - 1] == q[q1 - 1] {
            p1 -= 1;
            q1 -= 1;
            self.part[p1] = q1 as u32;
        }
        Area { p0, p1, q0, q1 }
    }

    /// The child that continues the chain, and the one that waits.
    fn pick(&self, left: Option<Area>, right: Option<Area>) -> (Option<Area>, Option<Area>) {
        match (left, right) {
            (Some(l), Some(r)) => {
                let right_continues = match self.tuning.inherit {
                    Inherit::Larger => r.size() >= l.size(),
                    #[cfg(test)]
                    Inherit::Smaller => r.size() < l.size(),
                    #[cfg(test)]
                    Inherit::Left => false,
                    #[cfg(test)]
                    Inherit::Right => true,
                };
                if right_continues {
                    (Some(r), Some(l))
                } else {
                    (Some(l), Some(r))
                }
            }
            (l, r) => (l.or(r), None),
        }
    }

    /// Lists every candidate of `a` into the region heap; the counts are those of `a`. The runs of `a`'s diagonals
    /// are measured afresh, so a pair inside the last run of its diagonal was offered with that run.
    fn scan(&mut self, p: &[u32], q: &[u32], a: Area) {
        #[cfg(test)]
        {
            self.stats.scans += 1;
            self.stats.scanned += a.size();
        }
        let qn = q.len();
        self.runs[a.diagonals(qn)].fill(Run::default());
        for y in a.q0..a.q1 {
            let v = q[y] as usize;
            // An id of Q absent from P has no row, or the final row, whose count is 0.
            let c = self.rows.get(v).map_or(0, |r| r.cnt);
            if c == 0 || c > MAX_RARITY {
                continue;
            }
            for at in self.p_occ(v, a.p0..a.p1) {
                let x = self.p_pos[at] as usize;
                let d = x + qn - y;
                // Inside the run measured last on this diagonal: y only grows.
                if self.runs[d].end as usize > y {
                    continue;
                }
                let (si, sj, len) = Self::measure(p, q, a, x, y);
                self.runs[d] = Run {
                    j: sj as u32,
                    end: (sj + len) as u32,
                };
                self.offer(p, si, sj, len);
            }
        }
    }

    /// Moves the chain from box `from` to its child `to`: the counts lose the lines of P outside `to`, and each
    /// affected id that can still have a pair in `to` gets a group entry with its new count: with its least slot when
    /// its group is built, else with `to`'s floor, which orders before every slot of `to`.
    fn step(&mut self, p: &[u32], from: Area, to: Area) {
        self.affected.clear();
        for r in [from.p0..to.p0, to.p1..from.p1] {
            #[cfg(test)]
            {
                self.stats.removed += r.len();
            }
            for &v in &p[r] {
                let c = &mut self.rows[v as usize].cnt;
                *c -= 1;
                if (1..=MAX_RARITY).contains(c) {
                    self.affected.push(v);
                }
            }
        }
        self.affected.sort_unstable();
        self.affected.dedup();
        let floor = to.floor();
        for n in 0..self.affected.len() {
            let v = self.affected[n] as usize;
            let row = self.rows[v];
            let lower = match row.live {
                UNBUILT if self.q_occ(v, to.q0..to.q1).is_empty() => continue,
                UNBUILT => floor,
                // A built group's least slot. A group with no slot left gets whatever lies there: its entry goes
                // when popped.
                _ => match self.slots.get(row.q as usize) {
                    Some(&top) => top,
                    None => continue,
                },
            };
            self.push_group(Entry::new(keyed(row.cnt, lower), v));
        }
    }

    /// Adds the region (i, j, len) to the region heap when it is a candidate.
    fn offer(&mut self, p: &[u32], i: usize, j: usize, len: usize) {
        #[cfg(test)]
        {
            self.stats.regions += 1;
        }
        let rarity = self.rarity(p, i..i + len);
        if rarity <= MAX_RARITY {
            self.push(pack(rarity, len, i, j));
        }
    }

    /// The least count of the lines P[r] ([F12 §7.5] rarity); 1 is the least a line of the box can have.
    fn rarity(&self, p: &[u32], r: Range<usize>) -> u32 {
        let mut least = u32::MAX;
        for &v in &p[r] {
            least = least.min(self.rows[v as usize].cnt);
            if least == 1 {
                break;
            }
        }
        least
    }

    /// Adds a region entry; keys at or above the threshold need none.
    fn push(&mut self, key: u128) {
        if key >= self.thresh {
            return;
        }
        self.heap.push(Reverse(key));
        if self.heap.len() > 2 * self.cap {
            self.trim();
        }
    }

    /// Keeps the `cap` least region entries; the threshold falls to the least one let go.
    fn trim(&mut self) {
        #[cfg(test)]
        {
            self.stats.trims += 1;
        }
        let mut keys = std::mem::take(&mut self.heap).into_vec();
        let (_, first_dropped, _) = keys.select_nth_unstable_by_key(self.cap, |k| k.0);
        self.thresh = self.thresh.min(first_dropped.0);
        keys.truncate(self.cap);
        self.heap = BinaryHeap::from(keys);
    }

    /// Adds a group entry, compacting the group heap when it has doubled: entries whose count is no longer their
    /// id's, and those of groups with no slot left, go.
    fn push_group(&mut self, e: Entry) {
        self.groups.push(Reverse(e));
        if self.groups.len() > self.group_cap {
            #[cfg(test)]
            {
                self.stats.compactions += 1;
            }
            let rows = &self.rows;
            self.groups.retain(|&Reverse(e)| {
                let r = rows[e.v as usize];
                r.cnt == rarity_of(e.key()) && r.live != 0
            });
            self.group_cap = (2 * self.groups.len()).max(self.tuning.group_cap);
        }
    }

    /// The least candidate of `a`, or `None` when `a` has none.
    fn choose(&mut self, p: &[u32], q: &[u32], a: Area) -> Option<u128> {
        loop {
            let region = self.heap.peek().map(|e| e.0).filter(|&k| k < self.thresh);
            let group = self.groups.peek().map(|e| e.0.key());
            match (region, group) {
                (Some(entry), g) if g.is_none_or(|g| entry <= g) => {
                    self.heap.pop();
                    #[cfg(test)]
                    {
                        self.stats.pops += 1;
                    }
                    let (i, j, len) = unpack(entry);
                    // The pairs (i + k, j + k) inside the box: lo ≤ k < hi.
                    let lo = a.p0.saturating_sub(i).max(a.q0.saturating_sub(j));
                    let hi = a.p1.saturating_sub(i).min(a.q1.saturating_sub(j)).min(len);
                    if lo >= hi {
                        continue;
                    }
                    let (ci, cj, cl) = (i + lo, j + lo, hi - lo);
                    let rarity = self.rarity(p, ci..ci + cl);
                    if rarity > MAX_RARITY {
                        continue;
                    }
                    let key = pack(rarity, cl, ci, cj);
                    if key == entry {
                        return Some(key);
                    }
                    // The least entry of both heaps bounds every candidate from below, this one's included.
                    debug_assert!(key > entry, "{:?} {:?}", unpack(key), unpack(entry));
                    self.push(key);
                }
                (_, Some(g)) if region.is_some() || g < self.thresh => {
                    let Some(Reverse(e)) = self.groups.pop() else {
                        continue;
                    };
                    #[cfg(test)]
                    {
                        self.stats.pops += 1;
                    }
                    let v = e.v as usize;
                    let cnt = self.rows[v].cnt;
                    if cnt != rarity_of(g) {
                        // A newer entry carries v's present count.
                        continue;
                    }
                    if self.rows[v].live == UNBUILT {
                        self.build_from(p, q, a, e);
                        continue;
                    }
                    let Some(top) = self.least(p, q, a, v) else {
                        continue;
                    };
                    let key = keyed(cnt, top);
                    if key == g {
                        return Some(key);
                    }
                    debug_assert!(key > g, "{:?} {:?}", unpack(key), unpack(g));
                    self.push_group(Entry::new(key, v));
                }
                _ => {
                    // Every candidate without an entry is at least the threshold: list them again.
                    if self.thresh == NO_KEY {
                        return None;
                    }
                    self.heap.clear();
                    self.thresh = NO_KEY;
                    self.scan(p, q, a);
                }
            }
        }
    }

    /// Builds the group of the id of the floor entry `e`, together with those of the ids whose floor entries equal
    /// it and stand next in the group heap (ids a step affected together), and gives each built group an entry with
    /// its least slot.
    fn build_from(&mut self, p: &[u32], q: &[u32], a: Area, e: Entry) {
        self.affected.clear();
        self.affected.push(e.v);
        while let Some(&Reverse(next)) = self.groups.peek() {
            if next.key() != e.key() {
                break;
            }
            self.groups.pop();
            #[cfg(test)]
            {
                self.stats.pops += 1;
            }
            // An id that left the box needs no group. One whose count fell since this entry has a newer one, so its
            // group is built a little early; one already built is skipped by the build. Either way it gets an
            // entry with its present count.
            if self.rows[next.v as usize].cnt != 0 {
                self.affected.push(next.v);
            }
        }
        self.build(p, q, a);
        for n in 0..self.affected.len() {
            let v = self.affected[n] as usize;
            let row = self.rows[v];
            if row.live != 0 {
                self.push_group(Entry::new(keyed(row.cnt, self.slots[row.q as usize]), v));
            }
        }
    }

    /// Builds the groups of the ids of `affected` not yet built: the slots of their lines in Q[a], from their pairs
    /// in `a`. One pass over all those lines in increasing order walks each diagonal forward, so a run shared by
    /// several of the ids is measured once.
    fn build(&mut self, p: &[u32], q: &[u32], a: Area) {
        let first = self.built.len();
        self.lines.clear();
        for n in 0..self.affected.len() {
            let v = self.affected[n] as usize;
            if self.rows[v].live != UNBUILT {
                continue;
            }
            let ys = self.q_occ(v, a.q0..a.q1);
            let s = self.rows[v].q as usize;
            self.rows[v].live = ys.len() as u32;
            self.built.push(v as u32);
            for (t, bt) in ys.enumerate() {
                self.lines.push((self.q_pos[bt], (s + t) as u32));
            }
        }
        #[cfg(test)]
        {
            self.stats.builds += self.built.len() - first;
        }
        if self.lines.is_empty() {
            return;
        }
        if self.slots.len() != self.q_pos.len() {
            self.slots.resize(self.q_pos.len(), NO_KEY);
        }
        self.lines.sort_unstable();
        for n in 0..self.lines.len() {
            let (y, at) = self.lines[n];
            let (y, at) = (y as usize, at as usize);
            let xs = self.p_occ(q[y] as usize, a.p0..a.p1);
            self.slots[at] = self.evaluate(p, q, a, xs, y);
        }
        for n in first..self.built.len() {
            let row = self.rows[self.built[n] as usize];
            let s = row.q as usize;
            heapify(&mut self.slots[s..s + row.live as usize]);
        }
    }

    /// v's least slot in `a`: its top slots are evaluated again until one holds, and those outside `a` go.
    fn least(&mut self, p: &[u32], q: &[u32], a: Area, v: usize) -> Option<u128> {
        let s = self.rows[v].q as usize;
        let xs = self.p_occ(v, a.p0..a.p1);
        loop {
            let n = self.rows[v].live as usize;
            let top = *self.slots[s..s + n].first()?;
            let y = line_of(top);
            let fresh = if (a.q0..a.q1).contains(&y) {
                #[cfg(test)]
                {
                    self.stats.evals += 1;
                }
                self.evaluate(p, q, a, xs.clone(), y)
            } else {
                NO_KEY
            };
            let h = &mut self.slots[s..s + n];
            if fresh == NO_KEY {
                // The top goes to the end, out of the heap.
                h.swap(0, n - 1);
                sift_down(&mut h[..n - 1], 0);
                self.rows[v].live -= 1;
            } else if fresh == top {
                return Some(top);
            } else {
                debug_assert!(fresh > top);
                h[0] = fresh;
                sift_down(h, 0);
            }
        }
    }

    /// The slot of line y of Q in `a`: its least region through the positions `xs` of `p_pos`.
    fn evaluate(&mut self, p: &[u32], q: &[u32], a: Area, xs: Range<usize>, y: usize) -> u128 {
        let mut best = NO_KEY;
        for at in xs {
            let x = self.p_pos[at] as usize;
            let (i, j, len) = self.region(p, q, a, x, y);
            best = best.min(slot(i, j, len, y));
        }
        best
    }

    /// The maximal region of `a` through the pair (x, y) as (i, j, len): the cached run of its diagonal clipped to
    /// `a` when the run holds the pair, else measured and cached.
    fn region(
        &mut self,
        p: &[u32],
        q: &[u32],
        a: Area,
        x: usize,
        y: usize,
    ) -> (usize, usize, usize) {
        #[cfg(test)]
        {
            self.stats.lookups += 1;
        }
        let d = x + q.len() - y;
        let Run { j, end } = self.runs[d];
        let (rj, rend) = (j as usize, end as usize);
        if (rj..rend).contains(&y) {
            // The run's pairs (ri + k, rj + k) inside `a`: lo ≤ k < hi, a range that holds k = y − rj.
            let ri = x - (y - rj);
            let lo = a.p0.saturating_sub(ri).max(a.q0.saturating_sub(rj));
            let hi = (a.p1 - ri).min(a.q1 - rj).min(rend - rj);
            return (ri + lo, rj + lo, hi - lo);
        }
        let (i, j, len) = Self::measure(p, q, a, x, y);
        #[cfg(test)]
        {
            self.stats.runs += 1;
            self.stats.walked += len;
        }
        self.runs[d] = Run {
            j: j as u32,
            end: (j + len) as u32,
        };
        (i, j, len)
    }

    /// The maximal region of `a` through the pair (x, y): (i, j, len).
    fn measure(p: &[u32], q: &[u32], a: Area, x: usize, y: usize) -> (usize, usize, usize) {
        let (mut si, mut sj) = (x, y);
        while si > a.p0 && sj > a.q0 && p[si - 1] == q[sj - 1] {
            si -= 1;
            sj -= 1;
        }
        let (mut ei, mut ej) = (x + 1, y + 1);
        while ei < a.p1 && ej < a.q1 && p[ei] == q[ej] {
            ei += 1;
            ej += 1;
        }
        (si, sj, ei - si)
    }
}

#[cfg(test)]
mod tests;
