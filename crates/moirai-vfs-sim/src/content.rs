//! The content of one simulated file: the cache image, the durable size, the size history and the per-sector states of
//! [F15 §2.2] (`clean`, `dirty`, `dirty-over-poison`, `poisoned`), with the transitions of its table.
//!
//! Representation. The cache image C(f) is a vector of 4 KiB [`Page`]s shared copy-on-write: a crash image, a recorded
//! version and the cache share one page until one of them is written, so capturing a kernel copies no file data, and a
//! zero sector of any file is one shared page. Bytes beyond cs(f) in the last page are zero. Clean sectors keep no other
//! state: their durable content is their cache content. A `dirty` sector keeps its baseline and every version written
//! since, each stamped with the file's write sequence number, so that a flush can tell which version was current when it
//! began ([F15 §2.2]: "A successful flush acts on each sector according to the state it had when the flush began").
//! Nothing here discards an intermediate version before a flush makes it unnecessary (FM-1.1).
//!
//! A write that leaves a sector's bytes as they are is still a write (F15 §2.2 write row; §5.2: "the model treats the
//! zeros as written data, so FM-1, FM-3 and FM-5 apply to them"). A clean sector written that way is `dirty` with every
//! candidate equal to its cache content; such sectors are kept as merged runs ([`Runs`]) instead of per-sector windows,
//! so a zero-filled extent costs a few run entries. A crash treats them as clean (every candidate is the same content);
//! a failed flush poisons them with K = {that content}; a successful flush that began after their last write cleans
//! them.

use std::collections::BTreeMap;
use std::sync::{Arc, OnceLock};

use crate::adversary::Site;
use crate::rng::Rng;

/// The unit of loss after a crash ([F15 §1.4] `SECTOR`).
pub const SECTOR: u64 = 4096;
/// The unit of tearing ([F15 §1.4] `SUBSECTOR`).
pub const SUBSECTOR: u64 = 512;
/// Sub-sectors per sector.
pub const SUBS: usize = 8;

const SEC: usize = SECTOR as usize;

/// One sector's bytes, shared copy-on-write between the cache image, recorded versions and crash images.
pub(crate) type Page = Arc<[u8; SEC]>;

/// The source of every zero write: extent zero-fill, truncation and size extension. 1 MiB is the zero-buffer bound of
/// [OS/fs §4.5] (`ZeroFill`: "one reused zero buffer of at most 1 MiB").
pub(crate) static ZEROS: [u8; 1 << 20] = [0; 1 << 20];

/// The shared all-zero page.
pub(crate) fn zero_page() -> Page {
    static Z: OnceLock<Page> = OnceLock::new();
    Arc::clone(Z.get_or_init(|| Arc::new([0u8; SEC])))
}

/// The state of a sector that is not `clean` and not a rewritten run ([F15 §2.2]).
#[derive(Clone, Debug)]
pub(crate) enum SecState {
    /// `dirty` (`over_poison == false`, `base` holds the one baseline b) or `dirty-over-poison` (`over_poison == true`,
    /// `base` is the candidate set K). `versions` are v₁ … vₘ with their write sequence numbers; vₘ equals the cache.
    /// `touched` is the sequence number of the last write to the sector, including one that left its bytes as they
    /// were (and so appended no distinct version).
    Dirty {
        base: Vec<Page>,
        over_poison: bool,
        versions: Vec<(u64, Page)>,
        touched: u64,
    },
    /// `poisoned` with candidate set K.
    Poisoned { k: Vec<Page> },
}

/// Runs of sectors that are `dirty` with every candidate equal to their cache content, each with the write sequence
/// number of its last write: `start → (end, seq)`, half-open, disjoint, adjacent runs merged when their numbers agree.
#[derive(Clone, Debug, Default)]
pub(crate) struct Runs {
    map: BTreeMap<u64, (u64, u64)>,
}

impl Runs {
    /// The sequence number of sector `s`, if it lies in a run.
    #[cfg(test)]
    fn get(&self, s: u64) -> Option<u64> {
        let (_, &(end, seq)) = self.map.range(..=s).next_back()?;
        (s < end).then_some(seq)
    }

    /// Splits the run that strictly contains `at` into two.
    fn split(&mut self, at: u64) {
        let Some((&a, &(b, q))) = self.map.range(..at).next_back() else {
            return;
        };
        if at < b {
            self.map.insert(a, (at, q));
            self.map.insert(at, (b, q));
        }
    }

    /// Removes `[lo, hi)` from every run.
    pub(crate) fn remove(&mut self, lo: u64, hi: u64) {
        if lo >= hi || self.map.is_empty() {
            return;
        }
        self.split(lo);
        self.split(hi);
        let inside: Vec<u64> = self.map.range(lo..hi).map(|(&a, _)| a).collect();
        for a in inside {
            self.map.remove(&a);
        }
    }

    /// Sets `[lo, hi)` to sequence number `seq`, merging with equal neighbours.
    pub(crate) fn insert(&mut self, lo: u64, hi: u64, seq: u64) {
        if lo >= hi {
            return;
        }
        self.remove(lo, hi);
        let (mut lo, mut hi) = (lo, hi);
        if let Some((&a, &(b, q))) = self.map.range(..lo).next_back()
            && b == lo
            && q == seq
        {
            self.map.remove(&a);
            lo = a;
        }
        if let Some(&(b, q)) = self.map.get(&hi)
            && q == seq
        {
            self.map.remove(&hi);
            hi = b;
        }
        self.map.insert(lo, (hi, seq));
    }

    /// The pieces of the runs inside `[lo, hi)`, clipped: `(start, end, seq)`.
    pub(crate) fn pieces(&self, lo: u64, hi: u64) -> Vec<(u64, u64, u64)> {
        let mut out = Vec::new();
        if let Some((_, &(b, q))) = self.map.range(..lo).next_back()
            && b > lo
        {
            out.push((lo, b.min(hi), q));
        }
        for (&a, &(b, q)) in self.map.range(lo..hi) {
            out.push((a, b.min(hi), q));
        }
        out
    }

    /// Whether any run touches `[lo, hi)`.
    fn any_in(&self, lo: u64, hi: u64) -> bool {
        !self.pieces(lo, hi).is_empty()
    }

    /// Removes the pieces inside `[lo, hi)` whose sequence number is at most `seq`, and returns them.
    fn clear_older(&mut self, lo: u64, hi: u64, seq: u64) -> Vec<(u64, u64)> {
        let mut cleared = Vec::new();
        for (a, b, q) in self.pieces(lo, hi) {
            if q <= seq {
                self.remove(a, b);
                cleared.push((a, b));
            }
        }
        cleared
    }

    /// Every run, `(start, end)`.
    pub(crate) fn ranges(&self) -> Vec<(u64, u64)> {
        self.map.iter().map(|(&a, &(b, _))| (a, b)).collect()
    }

    /// Whether no sector is rewritten.
    pub(crate) fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

/// What a flush remembers from its start ([F15 §2.2] "Transitions", FM-3.1).
#[derive(Clone, Debug)]
pub(crate) struct FlushMark {
    /// The file's write sequence number when the flush began: versions stamped at or below it were written before.
    pub(crate) seq: u64,
    /// ds(f) at the start: the reach of `sync(Data)` (FM-2.1).
    pub(crate) ds: u64,
    /// cs(f) at the start: the reach of `sync(DataAndMeta)` and the new durable size (FM-2.2).
    pub(crate) cs: u64,
    /// The sectors that were `dirty` or `dirty-over-poison` at the start (FM-3.1's reach).
    pub(crate) dirty: Vec<u64>,
    /// The rewritten runs at the start (also `dirty`, FM-3.1's reach).
    pub(crate) same: Vec<(u64, u64)>,
}

impl FlushMark {
    /// Adds to this flush's reach the sectors a concurrent successful flush of the same file made clean while this one
    /// was in flight: they were `dirty` at an instant of its interval (FM-3.1), so a failure of this flush poisons them
    /// too, with K = {the content that flush made durable} ∪ {every version written since} ([F15 §3.3] FM-3.1, as spec
    /// sync 2a reads it).
    pub(crate) fn absorb(&mut self, c: &Cleaned) {
        self.dirty.extend_from_slice(&c.secs);
        self.same.extend_from_slice(&c.runs);
    }
}

/// The sectors one successful flush made `clean` ([F15 §2.2] flush row): individual sectors and rewritten runs.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct Cleaned {
    /// Sectors that were `dirty` or `dirty-over-poison` and are `clean` now.
    pub(crate) secs: Vec<u64>,
    /// Rewritten runs that are `clean` now, half-open sector ranges.
    pub(crate) runs: Vec<(u64, u64)>,
}

impl Cleaned {
    /// `true` when the flush cleaned nothing.
    pub(crate) fn is_empty(&self) -> bool {
        self.secs.is_empty() && self.runs.is_empty()
    }
}

/// What bytes beyond the old durable size hold after a crash (FM-2.2, OP-4).
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum BeyondFill {
    /// The content the sector resolution produced.
    Resolved,
    /// Zeros.
    Zeros,
    /// Bytes from a generator with this seed (stale blocks of other files, ext4 `data=writeback`).
    Garbage(u64),
}

/// The choices one file's crash resolution needs; implemented by the crash plan over the adversary.
pub(crate) trait CrashPick {
    /// Index into the ascending size history H(f); must be below `sizes.len()`.
    fn size(&mut self, sizes: &[u64]) -> usize;
    /// The dirty sector (from `dirty`, ascending) that tears, if any.
    fn torn(&mut self, dirty: &[u64]) -> Option<u64>;
    /// The version a whole dirty sector keeps: 0 the baseline, i the version vᵢ; `n` candidates.
    fn sector(&mut self, s: u64, n: u64) -> u64;
    /// The candidate one sub-sector keeps (torn or poisoned sectors); `n` candidates.
    fn sub(&mut self, s: u64, j: usize, n: u64) -> u64;
    /// The fill beyond the old durable size.
    fn beyond(&mut self) -> BeyondFill;
}

/// One non-clean sector as the crash surface shows it.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct SectorView {
    /// Sector index (file offset / 4096).
    pub index: u64,
    /// Its state.
    pub state: SectorKind,
    /// The number of contents a crash may leave: 1 + m for a dirty sector (baseline, then v₁ … vₘ), |K ∪ versions| for
    /// the other two states.
    pub candidates: u64,
}

/// The state of a non-clean sector ([F15 §2.2]).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum SectorKind {
    /// `dirty`.
    Dirty,
    /// `dirty-over-poison`.
    DirtyOverPoison,
    /// `poisoned`.
    Poisoned,
}

/// A file's content model.
#[derive(Clone, Debug, Default)]
pub(crate) struct Content {
    /// cs(f).
    len: u64,
    /// C(f), one page per sector below cs(f).
    pages: Vec<Page>,
    /// ds(f).
    pub(crate) ds: u64,
    /// The flush mark's sequence number at which ds(f) was last set (a crash sets it too): a `sync(DataAndMeta)` that
    /// began before it never moves the durable size back.
    ds_seq: u64,
    /// The sizes cs(f) took since the last successful `sync(DataAndMeta)` began, each with the write sequence number at
    /// which it was taken. H(f) is `ds` together with these (FM-2.2).
    pub(crate) hist: Vec<(u64, u64)>,
    /// Non-clean sectors other than the rewritten runs.
    pub(crate) secs: BTreeMap<u64, SecState>,
    /// Sectors written with the bytes they held (`dirty`, every candidate equal to the cache content).
    pub(crate) same: Runs,
    /// The write sequence number, advanced by every write and size change.
    pub(crate) seq: u64,
}

fn sub_range(j: usize) -> core::ops::Range<usize> {
    j * SUBSECTOR as usize..(j + 1) * SUBSECTOR as usize
}

/// Writes `src` at byte `a` of page `p` (copy-on-write) and returns the new page.
fn put_bytes(p: &mut Page, a: usize, src: &[u8]) -> Page {
    if a == 0 && src.len() == SEC {
        let mut w = [0u8; SEC];
        w.copy_from_slice(src);
        *p = Arc::new(w);
    } else {
        Arc::make_mut(p)[a..a + src.len()].copy_from_slice(src);
    }
    Arc::clone(p)
}

impl Content {
    /// A file whose content, size and name are durable: `data` is its durable image.
    pub(crate) fn durable(data: &[u8]) -> Content {
        let pages = data
            .chunks(SEC)
            .map(|ch| {
                if ch.iter().all(|&b| b == 0) {
                    zero_page()
                } else {
                    let mut w = [0u8; SEC];
                    w[..ch.len()].copy_from_slice(ch);
                    Arc::new(w)
                }
            })
            .collect();
        Content {
            len: data.len() as u64,
            pages,
            ds: data.len() as u64,
            ..Content::default()
        }
    }

    /// cs(f).
    pub(crate) fn cs(&self) -> u64 {
        self.len
    }

    /// Sector `s` of the cache, zero beyond the end of the file.
    pub(crate) fn page(&self, s: u64) -> Page {
        self.pages
            .get(s as usize)
            .cloned()
            .unwrap_or_else(zero_page)
    }

    /// Sets sector `s` of the cache to `p` (clipped to the end of the file: bytes beyond it stay zero).
    fn set_page(&mut self, s: u64, p: &Page) {
        let Some(slot) = self.pages.get_mut(s as usize) else {
            return;
        };
        *slot = Arc::clone(p);
        let end = (s + 1) * SECTOR;
        if end > self.len {
            let keep = (self.len - s * SECTOR) as usize;
            if slot[keep..].iter().any(|&b| b != 0) {
                Arc::make_mut(slot)[keep..].fill(0);
            }
        }
    }

    /// Changes cs(f) without recording anything; the bytes a growth adds are zero.
    fn resize(&mut self, new_len: u64) {
        let n = new_len.div_ceil(SECTOR) as usize;
        if new_len >= self.len {
            self.pages.resize(n, zero_page());
        } else {
            self.pages.truncate(n);
            let tail = (new_len % SECTOR) as usize;
            if tail != 0
                && let Some(last) = self.pages.last_mut()
                && last[tail..].iter().any(|&b| b != 0)
            {
                Arc::make_mut(last)[tail..].fill(0);
            }
        }
        self.len = new_len;
    }

    /// H(f), ascending and without duplicates.
    pub(crate) fn sizes(&self) -> Vec<u64> {
        let mut v: Vec<u64> = self.hist.iter().map(|&(_, s)| s).collect();
        v.push(self.ds);
        v.sort_unstable();
        v.dedup();
        v
    }

    /// Whether any sector below cs(f) is not clean (unflushed, rewritten or poisoned). Sectors a truncation cut off are
    /// not counted: no read or eviction reaches them.
    pub(crate) fn has_unflushed(&self) -> bool {
        let n = self.len.div_ceil(SECTOR);
        self.secs.range(..n).next().is_some() || self.same.any_in(0, n)
    }

    /// Whether a sector below `len` bytes is poisoned (reads of it draw from K), counting the sectors beyond cs(f) that
    /// a truncation left for a crash: a write that extends the file over them meets their poison (FM-3.5 merge).
    pub(crate) fn poisoned_below(&self, len: u64) -> bool {
        self.secs
            .range(..len.div_ceil(SECTOR))
            .any(|(_, st)| matches!(st, SecState::Poisoned { .. }))
    }

    /// Whether a read that reaches below byte `len` draws from K ([F15 §2.2] `poisoned`, FM-3.2): a sector overlapping
    /// `[0, min(len, cs(f)))` is poisoned. Exactly the sectors for which [`Content::read`] of those bytes asks for a
    /// [`Site::PoisonRead`] choice; a `dirty-over-poison` sector reads its cache content and does not count.
    pub(crate) fn reads_poisoned(&self, len: u64) -> bool {
        self.poisoned_below(len.min(self.len))
    }

    /// The cache bytes at `offset` into `buf`, with no poison draw; returns their count.
    pub(crate) fn read_plain(&self, offset: u64, buf: &mut [u8]) -> usize {
        if offset >= self.len {
            return 0;
        }
        let n = (buf.len() as u64).min(self.len - offset) as usize;
        let mut done = 0;
        while done < n {
            let at = offset + done as u64;
            let s = (at / SECTOR) as usize;
            let a = (at % SECTOR) as usize;
            let k = (SEC - a).min(n - done);
            buf[done..done + k].copy_from_slice(&self.pages[s][a..a + k]);
            done += k;
        }
        n
    }

    /// C(f) as one vector.
    pub(crate) fn to_vec(&self) -> Vec<u8> {
        let mut v = vec![0u8; self.len as usize];
        self.read_plain(0, &mut v);
        v
    }

    /// The bytes a read of `buf.len()` bytes at `offset` returns, and their count: the cache, except that every
    /// sub-sector of a poisoned sector is drawn from K afresh (FM-3.2) through `pick(Site::PoisonRead, aux, |K|)`.
    pub(crate) fn read(
        &self,
        offset: u64,
        buf: &mut [u8],
        pick: &mut dyn FnMut(Site, u64, u64) -> u64,
    ) -> usize {
        let n = self.read_plain(offset, buf);
        if n == 0 {
            return 0;
        }
        let end = offset + n as u64;
        let first = offset / SECTOR;
        let last = (end - 1) / SECTOR;
        for (&s, st) in self.secs.range(first..=last) {
            let SecState::Poisoned { k } = st else {
                continue;
            };
            for j in 0..SUBS {
                let a = s * SECTOR + j as u64 * SUBSECTOR;
                let b = a + SUBSECTOR;
                let lo = a.max(offset);
                let hi = b.min(end);
                if lo >= hi {
                    continue;
                }
                let m = pick(Site::PoisonRead, s * SUBS as u64 + j as u64, k.len() as u64) as usize;
                let src = &k[m][(lo - s * SECTOR) as usize..(hi - s * SECTOR) as usize];
                buf[(lo - offset) as usize..(hi - offset) as usize].copy_from_slice(src);
            }
        }
        n
    }

    /// A write that returned ([F15 §2.2] write row): the sectors it touches move `clean` → `dirty`, append a version when
    /// `dirty` or `dirty-over-poison`, and `poisoned` → `dirty-over-poison` with every unwritten sub-sector fixed to one
    /// member of K through `pick(Site::RewriteMerge, aux, |K|)` (FM-3.5). A write beyond the end extends the file. A
    /// write of the bytes a sector already holds is a write too: a clean sector joins the rewritten runs, a dirty one
    /// records the write's sequence number (its candidates are unchanged).
    pub(crate) fn write(
        &mut self,
        offset: u64,
        data: &[u8],
        pick: &mut dyn FnMut(Site, u64, u64) -> u64,
    ) {
        if data.is_empty() {
            return;
        }
        let end = offset + data.len() as u64;
        self.seq += 1;
        let seq = self.seq;
        if end > self.len {
            self.resize(end);
            self.hist.push((seq, end));
        }
        let first = offset / SECTOR;
        let last = (end - 1) / SECTOR;
        let mut run: Option<u64> = None;
        for s in first..=last {
            let lo = offset.max(s * SECTOR);
            let hi = end.min((s + 1) * SECTOR);
            let src = &data[(lo - offset) as usize..(hi - offset) as usize];
            let a = (lo - s * SECTOR) as usize;
            let b = (hi - s * SECTOR) as usize;
            let equal = self.pages[s as usize][a..b] == *src;
            if equal && !self.secs.contains_key(&s) {
                run.get_or_insert(s);
                continue;
            }
            if let Some(r) = run.take() {
                self.same.insert(r, s, seq);
            }
            match self.secs.get_mut(&s) {
                None => {
                    // Clean, or rewritten: either way every candidate is the cache content, now the baseline.
                    self.same.remove(s, s + 1);
                    let old = Arc::clone(&self.pages[s as usize]);
                    let new = put_bytes(&mut self.pages[s as usize], a, src);
                    self.secs.insert(
                        s,
                        SecState::Dirty {
                            base: vec![old],
                            over_poison: false,
                            versions: vec![(seq, new)],
                            touched: seq,
                        },
                    );
                }
                Some(SecState::Dirty {
                    versions, touched, ..
                }) => {
                    // A version equal to the current one adds no candidate (FM-1.1, FM-3.1); the write still counts.
                    if !equal {
                        let new = put_bytes(&mut self.pages[s as usize], a, src);
                        versions.push((seq, new));
                    }
                    *touched = seq;
                }
                Some(SecState::Poisoned { k }) => {
                    let k = core::mem::take(k);
                    let mut merged = [0u8; SEC];
                    for j in 0..SUBS {
                        let sa = j * SUBSECTOR as usize;
                        let sb = sa + SUBSECTOR as usize;
                        if !(a <= sa && sb <= b) {
                            let m = pick(
                                Site::RewriteMerge,
                                s * SUBS as u64 + j as u64,
                                k.len() as u64,
                            ) as usize;
                            merged[sub_range(j)].copy_from_slice(&k[m][sub_range(j)]);
                        }
                    }
                    merged[a..b].copy_from_slice(src);
                    let merged: Page = Arc::new(merged);
                    self.set_page(s, &merged);
                    let cur = self.page(s);
                    self.secs.insert(
                        s,
                        SecState::Dirty {
                            base: k,
                            over_poison: true,
                            versions: vec![(seq, cur)],
                            touched: seq,
                        },
                    );
                }
            }
        }
        if let Some(r) = run {
            self.same.insert(r, last + 1, seq);
        }
    }

    /// Writes `len` zero bytes at `offset`, from the shared zero buffer.
    pub(crate) fn write_zeros(
        &mut self,
        offset: u64,
        len: u64,
        pick: &mut dyn FnMut(Site, u64, u64) -> u64,
    ) {
        let mut done = 0;
        while done < len {
            let n = (len - done).min(ZEROS.len() as u64);
            self.write(offset + done, &ZEROS[..n as usize], pick);
            done += n;
        }
    }

    /// Applies `data` at `offset` only where `take(i)` says byte i took its new value (FM-5.2, §2.5): every sector in
    /// which a byte took its value is written with its mixed content (a byte that took a value equal to its old one is a
    /// write as well, the widest reading); a byte beyond the old end that did not take its value leaves a hole of zeros
    /// only if a later byte extended the file.
    pub(crate) fn write_partial(
        &mut self,
        offset: u64,
        data: &[u8],
        take: &mut dyn FnMut(u64) -> bool,
        pick: &mut dyn FnMut(Site, u64, u64) -> u64,
    ) {
        let cs = self.len;
        let mut mixed = vec![0u8; data.len()];
        self.read_plain(offset, &mut mixed);
        let mut taken = vec![false; data.len()];
        let mut last_taken: Option<usize> = None;
        for (i, &new) in data.iter().enumerate() {
            if take(i as u64) {
                mixed[i] = new;
                taken[i] = true;
                last_taken = Some(i);
            }
        }
        // Bytes beyond the old end exist only up to the last byte that took its new value.
        let keep = if offset + mixed.len() as u64 > cs {
            let beyond_start = cs.saturating_sub(offset) as usize;
            match last_taken {
                Some(i) if i >= beyond_start => i + 1,
                _ => beyond_start.min(mixed.len()),
            }
        } else {
            mixed.len()
        };
        // Write the sectors in which a byte took its value, and the hole below the last one beyond the old end.
        let mut s_lo = 0usize;
        while s_lo < keep {
            let at = offset + s_lo as u64;
            let sector_end = ((at / SECTOR) + 1) * SECTOR;
            let s_hi = ((sector_end - offset) as usize).min(keep);
            let written = taken[s_lo..s_hi].iter().any(|&t| t) || at + (s_hi - s_lo) as u64 > cs;
            if written {
                self.write(at, &mixed[s_lo..s_hi], pick);
            }
            s_lo = s_hi;
        }
    }

    /// A size change to `len` ([F15 §2.2] "Sizes"; FM-10.1; the `Sparse` extent method). A growth writes zeros over the
    /// new range (F15 §5.2: the zeros are written data). A truncation is a write of zeros over the removed range followed
    /// by the size change, so the sectors it cuts keep their baselines for a crash that restores a larger size from H(f).
    pub(crate) fn set_len(&mut self, len: u64, pick: &mut dyn FnMut(Site, u64, u64) -> u64) {
        let cs = self.len;
        if len > cs {
            self.write_zeros(cs, len - cs, pick);
        } else if len < cs {
            self.write_zeros(len, cs - len, pick);
            self.resize(len);
            self.seq += 1;
            self.hist.push((self.seq, len));
        }
    }

    /// The mark a flush takes at its start.
    pub(crate) fn flush_mark(&self) -> FlushMark {
        FlushMark {
            seq: self.seq,
            ds: self.ds,
            cs: self.len,
            dirty: self
                .secs
                .iter()
                .filter(|(_, st)| matches!(st, SecState::Dirty { .. }))
                .map(|(&s, _)| s)
                .collect(),
            same: self.same.ranges(),
        }
    }

    /// A successful flush ([F15 §2.2] flush row; FM-2.1, FM-2.2): every covered sector below the reach becomes `clean`
    /// at the version current when the flush began, or `dirty` with that version as its baseline if it was written after
    /// (an equal-byte write included); a sector poisoned at the start, or clean at the start, is unchanged (FM-3.4).
    /// `meta` also makes the size at the start durable and resets H(f) — unless a `sync(DataAndMeta)` that began later
    /// has already set a newer durable size, which this one never moves back (G-2). Returns the sectors it made clean,
    /// which join the reach of every other flush of the file still in flight ([`FlushMark::absorb`]).
    pub(crate) fn flush_ok(&mut self, mark: &FlushMark, meta: bool) -> Cleaned {
        let limit = if meta { mark.cs } else { mark.ds };
        let lim = limit.div_ceil(SECTOR);
        let mut cleaned = Cleaned::default();
        let covered: Vec<u64> = self
            .secs
            .range(..lim)
            .filter(|(_, st)| matches!(st, SecState::Dirty { .. }))
            .map(|(&s, _)| s)
            .collect();
        for s in covered {
            let Some(SecState::Dirty {
                versions, touched, ..
            }) = self.secs.get_mut(&s)
            else {
                continue;
            };
            let Some(i) = versions.iter().rposition(|&(q, _)| q <= mark.seq) else {
                continue;
            };
            let touched = *touched;
            if i + 1 == versions.len() {
                self.secs.remove(&s);
                if touched > mark.seq {
                    // Re-written with its own bytes after the flush began: dirty, every candidate the cache content.
                    self.same.insert(s, s + 1, touched);
                } else {
                    cleaned.secs.push(s);
                }
            } else {
                let rest = versions.split_off(i + 1);
                let (_, at_start) = versions.pop().expect("version i exists");
                self.secs.insert(
                    s,
                    SecState::Dirty {
                        base: vec![at_start],
                        over_poison: false,
                        versions: rest,
                        touched,
                    },
                );
            }
        }
        cleaned.runs = self.same.clear_older(0, lim, mark.seq);
        if meta && mark.seq >= self.ds_seq {
            self.ds = mark.cs;
            self.ds_seq = mark.seq;
            self.hist.retain(|&(q, _)| q > mark.seq);
            // Sectors wholly beyond every size a crash can still choose are gone for good.
            let reach = self.sizes().last().copied().unwrap_or(0).div_ceil(SECTOR);
            drop(self.secs.split_off(&reach));
            self.same.remove(reach, u64::MAX);
        }
        debug_assert!(
            self.sizes().contains(&self.len),
            "simulator: cs(f) is a member of H(f)"
        );
        cleaned
    }

    /// A failed flush (FM-3.1): every sector `dirty` or `dirty-over-poison` at any instant between the flush's start and
    /// its return (the rewritten runs included) becomes `poisoned` with K = its baseline or candidate set together with
    /// every version. The reach is the mark's sectors (those at the start, and those a concurrent successful flush
    /// cleaned during the interval, [`FlushMark::absorb`]) and every sector not clean at the return; K is taken at the
    /// return, so a sector a concurrent successful flush made clean gets that flush's durable content as its baseline.
    pub(crate) fn flush_failed(&mut self, mark: &FlushMark) {
        let mut reach: Vec<u64> = self.secs.keys().copied().collect();
        reach.extend(mark.dirty.iter().copied());
        for (a, b) in mark.same.iter().copied().chain(self.same.ranges()) {
            reach.extend(a..b);
        }
        reach.sort_unstable();
        reach.dedup();
        self.same = Runs::default();
        for s in reach {
            match self.secs.remove(&s) {
                Some(SecState::Dirty { base, versions, .. }) => {
                    let mut k = base;
                    k.extend(versions.into_iter().map(|(_, w)| w));
                    self.secs.insert(s, SecState::Poisoned { k });
                }
                Some(p @ SecState::Poisoned { .. }) => {
                    self.secs.insert(s, p);
                }
                None => {
                    // Rewritten with its own bytes, or dirty at an instant of the interval and cleaned since by a
                    // concurrent successful flush: its cache content (the durable content) is the one candidate.
                    let k = vec![self.page(s)];
                    self.secs.insert(s, SecState::Poisoned { k });
                }
            }
        }
    }

    /// The non-clean sectors other than the rewritten runs, for the crash surface.
    pub(crate) fn sector_views(&self) -> Vec<SectorView> {
        self.secs
            .iter()
            .map(|(&index, st)| match st {
                SecState::Dirty {
                    base,
                    over_poison,
                    versions,
                    ..
                } => SectorView {
                    index,
                    state: if *over_poison {
                        SectorKind::DirtyOverPoison
                    } else {
                        SectorKind::Dirty
                    },
                    candidates: (base.len() + versions.len()) as u64,
                },
                SecState::Poisoned { k } => SectorView {
                    index,
                    state: SectorKind::Poisoned,
                    candidates: k.len() as u64,
                },
            })
            .collect()
    }

    /// A system crash ([F15 §2.5] step 2): the new size is a member of H(f); each dirty sector below it keeps one of its
    /// contents whole, except at most one torn sector whose sub-sectors are chosen independently (FM-1.1, FM-1.2); each
    /// poisoned or dirty-over-poison sector draws every sub-sector from its candidates and is poisoned afterwards
    /// (FM-3.3); a rewritten sector keeps its one content; bytes beyond the old durable size may hold any value (FM-2.2).
    /// Afterwards cs = ds = the new size.
    pub(crate) fn crash(&mut self, p: &mut dyn CrashPick) {
        let sizes = self.sizes();
        let i = p.size(&sizes);
        assert!(
            i < sizes.len(),
            "simulator: a crash size index {i} beyond H(f) of {} sizes",
            sizes.len()
        );
        let new_size = sizes[i];
        let old_ds = self.ds;
        let dirty: Vec<u64> = self
            .secs
            .iter()
            .filter(|&(&s, st)| {
                s * SECTOR < new_size
                    && matches!(
                        st,
                        SecState::Dirty {
                            over_poison: false,
                            ..
                        }
                    )
            })
            .map(|(&s, _)| s)
            .collect();
        let torn = p.torn(&dirty);
        self.resize(new_size);
        self.same = Runs::default();
        let secs = core::mem::take(&mut self.secs);
        for (s, st) in secs {
            if s * SECTOR >= new_size {
                continue;
            }
            match st {
                SecState::Dirty {
                    base,
                    over_poison: false,
                    versions,
                    ..
                } => {
                    let mut cands = base;
                    cands.extend(versions.into_iter().map(|(_, w)| w));
                    let n = cands.len() as u64;
                    let w = if torn == Some(s) {
                        let mut w = [0u8; SEC];
                        for j in 0..SUBS {
                            let m = p.sub(s, j, n).min(n - 1) as usize;
                            w[sub_range(j)].copy_from_slice(&cands[m][sub_range(j)]);
                        }
                        Arc::new(w)
                    } else {
                        let m = p.sector(s, n).min(n - 1) as usize;
                        cands.swap_remove(m)
                    };
                    self.set_page(s, &w);
                }
                SecState::Dirty { base, versions, .. } => {
                    let mut k = base;
                    k.extend(versions.into_iter().map(|(_, w)| w));
                    let w = draw(s, &k, p);
                    self.set_page(s, &w);
                    self.secs.insert(s, SecState::Poisoned { k });
                }
                SecState::Poisoned { k } => {
                    let w = draw(s, &k, p);
                    self.set_page(s, &w);
                    self.secs.insert(s, SecState::Poisoned { k });
                }
            }
        }
        if new_size > old_ds {
            match p.beyond() {
                BeyondFill::Resolved => {}
                BeyondFill::Zeros => self.fill(old_ds, new_size, &mut |b| b.fill(0)),
                BeyondFill::Garbage(seed) => {
                    let mut r = Rng::new(seed);
                    self.fill(old_ds, new_size, &mut |b| r.fill(b));
                }
            }
        }
        self.seq += 1;
        self.ds = new_size;
        self.ds_seq = self.seq;
        self.hist.clear();
    }

    /// Overwrites the cache bytes `[lo, hi)` (below cs) page by page through `f`.
    fn fill(&mut self, lo: u64, hi: u64, f: &mut dyn FnMut(&mut [u8])) {
        let mut at = lo;
        while at < hi {
            let s = (at / SECTOR) as usize;
            let a = (at % SECTOR) as usize;
            let k = (SEC - a).min((hi - at) as usize);
            f(&mut Arc::make_mut(&mut self.pages[s])[a..a + k]);
            at += k as u64;
        }
    }
}

/// One content drawn per sub-sector from `k` (a poisoned sector at a crash, FM-3.3).
fn draw(s: u64, k: &[Page], p: &mut dyn CrashPick) -> Page {
    let n = k.len() as u64;
    let mut w = [0u8; SEC];
    for j in 0..SUBS {
        let m = p.sub(s, j, n).min(n - 1) as usize;
        w[sub_range(j)].copy_from_slice(&k[m][sub_range(j)]);
    }
    Arc::new(w)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_pick() -> impl FnMut(Site, u64, u64) -> u64 {
        |_, _, _| 0
    }

    struct Fixed {
        size: usize,
        torn: Option<u64>,
        sector: u64,
        sub: u64,
    }

    impl CrashPick for Fixed {
        fn size(&mut self, _: &[u64]) -> usize {
            self.size
        }
        fn torn(&mut self, _: &[u64]) -> Option<u64> {
            self.torn
        }
        fn sector(&mut self, _: u64, n: u64) -> u64 {
            self.sector.min(n - 1)
        }
        fn sub(&mut self, _: u64, j: usize, n: u64) -> u64 {
            if j.is_multiple_of(2) {
                self.sub.min(n - 1)
            } else {
                0
            }
        }
        fn beyond(&mut self) -> BeyondFill {
            BeyondFill::Resolved
        }
    }

    fn fixed(size: usize) -> Fixed {
        Fixed {
            size,
            torn: None,
            sector: 0,
            sub: 0,
        }
    }

    #[test]
    fn versions_and_flush_coverage() {
        let mut c = Content::default();
        c.write(0, &[1; 100], &mut no_pick());
        c.write(0, &[2; 100], &mut no_pick());
        let mark = c.flush_mark();
        c.write(0, &[3; 100], &mut no_pick());
        c.flush_ok(&mark, true);
        // Written after the flush began: dirty with the version current at the start as its baseline.
        let SecState::Dirty { base, versions, .. } = &c.secs[&0] else {
            panic!()
        };
        assert_eq!(base[0][0], 2);
        assert_eq!(versions.len(), 1);
        assert_eq!(c.ds, 100);
        let mark = c.flush_mark();
        c.flush_ok(&mark, false);
        assert!(c.secs.is_empty());
    }

    #[test]
    fn failed_flush_poisons_and_rewrite_merges() {
        let mut c = Content::default();
        c.write(0, &[1; 4096], &mut no_pick());
        let mark = c.flush_mark();
        c.flush_failed(&mark);
        assert!(matches!(c.secs[&0], SecState::Poisoned { ref k } if k.len() == 2));
        let mut buf = [9u8; 4096];
        let mut picks = 0;
        c.read(0, &mut buf, &mut |site, _, n| {
            assert_eq!(site, Site::PoisonRead);
            picks += 1;
            u64::from(picks % 2 == 0) % n
        });
        assert_eq!(picks, 8);
        assert_eq!(buf[0], 0, "sub-sector 0 from the baseline");
        assert_eq!(buf[512], 1, "sub-sector 1 from v1");
        // A re-write of sub-sector 0 only: the other seven are fixed to members of K.
        c.write(0, &[7; 512], &mut |site, _, _| {
            assert_eq!(site, Site::RewriteMerge);
            1
        });
        assert!(matches!(
            c.secs[&0],
            SecState::Dirty {
                over_poison: true,
                ..
            }
        ));
        assert_eq!(c.to_vec()[0], 7);
        assert_eq!(c.to_vec()[600], 1);
        // `sync(Data)` reaches nothing below a durable size of 0; `sync(DataAndMeta)` makes the re-written sector clean.
        let mark = c.flush_mark();
        c.flush_ok(&mark, false);
        assert_eq!(c.secs.len(), 1);
        let mark = c.flush_mark();
        c.flush_ok(&mark, true);
        assert!(c.secs.is_empty());
    }

    #[test]
    fn crash_resolution() {
        let mut c = Content::default();
        c.write(0, &[1; 8192], &mut no_pick());
        let mark = c.flush_mark();
        c.flush_ok(&mark, true);
        c.write(0, &[2; 8192], &mut no_pick());
        c.write(0, &[3; 8192], &mut no_pick());
        // Sector 0 torn (even sub-sectors v2, odd the baseline), sector 1 at v1.
        c.crash(&mut Fixed {
            size: 0,
            torn: Some(0),
            sector: 1,
            sub: 2,
        });
        let v = c.to_vec();
        assert_eq!(v[0], 3);
        assert_eq!(v[512], 1);
        assert_eq!(v[4096], 2);
        assert!(c.secs.is_empty());
        assert_eq!((c.cs(), c.ds), (8192, 8192));
    }

    #[test]
    fn partial_write_changes_only_taken_bytes() {
        let mut c = Content::default();
        c.write(0, &[1; 16], &mut no_pick());
        c.write_partial(8, &[2; 16], &mut |i| i < 4, &mut no_pick());
        assert_eq!(
            c.cs(),
            16,
            "untaken bytes beyond the end do not extend the file"
        );
        assert_eq!(&c.to_vec()[8..14], &[2, 2, 2, 2, 1, 1]);
        c.set_len(4, &mut no_pick());
        assert_eq!(c.cs(), 4);
        assert_eq!(c.sizes(), vec![0, 4, 16]);
    }

    /// Review regression: an older `sync(DataAndMeta)` that returns after a newer one never moves ds back (G-2).
    #[test]
    fn an_older_meta_flush_never_shrinks_the_durable_size() {
        let mut c = Content::default();
        c.write(0, &[1; 100], &mut no_pick());
        let a = c.flush_mark();
        c.write(100, &[2; 100], &mut no_pick());
        let b = c.flush_mark();
        c.flush_ok(&b, true);
        assert_eq!(c.ds, 200);
        c.flush_ok(&a, true);
        assert_eq!(c.ds, 200);
        assert_eq!(c.sizes(), vec![200]);
        let mut n = c.clone();
        n.crash(&mut fixed(0));
        assert_eq!(n.cs(), 200);
    }

    /// Review regression: a write of the bytes a clean sector holds is a write (F15 §2.2, §5.2): a failed flush poisons it,
    /// a later append over it is dirty-over-poison, and a crash does not end the poisoning.
    #[test]
    fn equal_byte_writes_are_writes() {
        let mut c = Content::default();
        c.write_zeros(0, 8 * SECTOR, &mut no_pick());
        assert!(c.secs.is_empty());
        assert_eq!(c.same.ranges(), vec![(0, 8)]);
        assert!(c.has_unflushed());
        // A crash keeps the zeros (every candidate is zero) and ends the dirty state.
        let mut after = c.clone();
        after.crash(&mut fixed(1));
        assert!(after.secs.is_empty() && after.same.is_empty());
        assert_eq!(after.to_vec(), vec![0u8; 8 * SEC]);
        // A failed flush poisons every rewritten sector with K = {zeros}.
        let mark = c.flush_mark();
        c.flush_failed(&mark);
        assert_eq!(c.secs.len(), 8);
        assert!(
            c.secs
                .values()
                .all(|s| matches!(s, SecState::Poisoned { k } if k.len() == 1))
        );
        // Appends over two of them are dirty-over-poison, and survive a crash poisoned.
        c.write(0, &[5; 2 * SEC], &mut no_pick());
        assert!(matches!(
            c.secs[&1],
            SecState::Dirty {
                over_poison: true,
                ..
            }
        ));
        c.crash(&mut Fixed {
            size: 1,
            torn: None,
            sector: 0,
            sub: 1,
        });
        assert!(matches!(c.secs[&0], SecState::Poisoned { ref k } if k.len() == 2));
        assert!(matches!(c.secs[&7], SecState::Poisoned { .. }));
        // A successful flush that began after the rewrite cleans it; one that began before does not.
        let mut d = Content::durable(&[3u8; SEC]);
        let early = d.flush_mark();
        d.write(0, &[3u8; SEC], &mut no_pick());
        d.flush_ok(&early, true);
        assert_eq!(d.same.ranges(), vec![(0, 1)]);
        let late = d.flush_mark();
        d.flush_ok(&late, true);
        assert!(d.same.is_empty() && !d.has_unflushed());
        // An equal re-write of a dirty sector after a flush began keeps it dirty.
        let mut e = Content::durable(&[3u8; SEC]);
        e.write(0, &[4u8; SEC], &mut no_pick());
        let m = e.flush_mark();
        e.write(0, &[4u8; SEC], &mut no_pick());
        e.flush_ok(&m, true);
        assert!(e.has_unflushed());
    }

    /// FM-3.1 (spec sync 2a): a sector written after a flush began and made clean by a concurrent successful flush before
    /// the first one failed was `dirty` at an instant of the failed flush's interval, so the failure poisons it, with K
    /// = {the content the successful flush made durable}.
    #[test]
    fn a_failed_flush_poisons_what_a_concurrent_flush_cleaned_during_it() {
        let mut c = Content::durable(&[0u8; 2 * SEC]);
        let failing = c.flush_mark();
        // Written after the failing flush began, then cleaned by a successful flush that began and returned meanwhile.
        c.write(SECTOR, &[5; SEC], &mut no_pick());
        c.write_zeros(0, SECTOR, &mut no_pick());
        let ok = c.flush_mark();
        let cleaned = c.flush_ok(&ok, true);
        assert_eq!(cleaned.secs, vec![1]);
        assert_eq!(cleaned.runs, vec![(0, 1)]);
        assert!(!c.has_unflushed());
        let mut failing = failing;
        failing.absorb(&cleaned);
        c.flush_failed(&failing);
        let views: Vec<(u64, SectorKind, u64)> = c
            .sector_views()
            .iter()
            .map(|v| (v.index, v.state, v.candidates))
            .collect();
        assert_eq!(
            views,
            vec![(0, SectorKind::Poisoned, 1), (1, SectorKind::Poisoned, 1)]
        );
        let SecState::Poisoned { k } = &c.secs[&1] else {
            panic!()
        };
        assert_eq!(k[0][0], 5, "K holds what the successful flush made durable");
        // Without the absorbed sectors the same failure would leave both clean (the gap spec sync 2a closed).
        let mut d = Content::durable(&[0u8; SEC]);
        let early = d.flush_mark();
        d.write(0, &[5; SEC], &mut no_pick());
        let m = d.flush_mark();
        d.flush_ok(&m, true);
        d.flush_failed(&early);
        assert!(d.secs.is_empty());
    }

    /// Review regression: sectors a truncation cut off are forgotten once the durable size moves below them.
    #[test]
    fn truncated_sectors_go_with_the_meta_flush() {
        let mut c = Content::durable(&[7u8; 2 * SEC]);
        c.set_len(100, &mut no_pick());
        assert!(c.secs.contains_key(&1));
        assert!(c.has_unflushed(), "sector 0 was written with zeros too");
        let mark = c.flush_mark();
        c.flush_ok(&mark, true);
        assert!(c.secs.is_empty() && c.same.is_empty());
        assert!(!c.has_unflushed());
    }

    /// The FM-3.2 query (S4 finding 3): a failed flush makes reads of the sector draw from K (FM-3.1); a successful flush
    /// (FM-3.4) and a crash (FM-3.3) leave it so; a re-write of any part of it ends it (FM-3.5); a sector beyond cs(f) is
    /// not read.
    #[test]
    fn reads_poisoned_follows_the_fm3_life_cycle() {
        let mut c = Content::durable(&[1u8; 3 * SEC]);
        c.write(SECTOR, &[2; 10], &mut no_pick());
        assert!(
            !c.reads_poisoned(u64::MAX),
            "a dirty sector reads its cache"
        );
        let mark = c.flush_mark();
        c.flush_failed(&mark);
        assert!(!c.reads_poisoned(0));
        assert!(!c.reads_poisoned(SECTOR), "sector 0 was clean");
        assert!(c.reads_poisoned(SECTOR + 1));
        assert!(c.reads_poisoned(u64::MAX));
        let mark = c.flush_mark();
        c.flush_ok(&mark, true);
        assert!(c.reads_poisoned(2 * SECTOR), "FM-3.4");
        let mut after = c.clone();
        after.crash(&mut fixed(0));
        assert!(after.reads_poisoned(2 * SECTOR), "FM-3.3");
        // One sub-sector re-written: dirty-over-poison, which reads its cache content.
        c.write(SECTOR + 600, &[3; 4], &mut no_pick());
        assert!(!c.reads_poisoned(u64::MAX), "FM-3.5");
        let mark = c.flush_mark();
        c.flush_failed(&mark);
        assert!(
            c.reads_poisoned(2 * SECTOR),
            "a dirty-over-poison sector is poisoned again"
        );
        // A truncation below it writes zeros over it (ending the poison); a failed flush then poisons it beyond cs(f).
        c.set_len(SECTOR, &mut no_pick());
        assert!(!c.reads_poisoned(u64::MAX) && !c.poisoned_below(u64::MAX));
        let mark = c.flush_mark();
        c.flush_failed(&mark);
        assert!(
            c.poisoned_below(2 * SECTOR),
            "kept for a crash that restores a larger size"
        );
        assert!(!c.reads_poisoned(u64::MAX), "no read reaches beyond cs(f)");
    }

    mod props {
        use proptest::prelude::*;

        use super::*;

        /// Property-test cases for the tier `MOIRAI_TEST_TIER` names (PLAN §2.1): `pr` runs `pr` cases, `nightly` 16
        /// times as many, `exit` 256 times as many.
        fn cases(pr: u32) -> u32 {
            match std::env::var("MOIRAI_TEST_TIER").as_deref() {
                Ok("nightly") => pr * 16,
                Ok("exit") => pr * 256,
                _ => pr,
            }
        }

        #[derive(Clone, Debug)]
        enum Op {
            Write { off: u64, len: u64, byte: u8 },
            FailFlush,
            OkFlush { meta: bool },
            Crash { size: usize, torn: bool, pick: u64 },
            SetLen(u64),
        }

        fn op() -> impl Strategy<Value = Op> {
            prop_oneof![
                4 => (0..4 * SECTOR, 1..2 * SECTOR, any::<u8>())
                    .prop_map(|(off, len, byte)| Op::Write { off, len, byte }),
                2 => Just(Op::FailFlush),
                1 => any::<bool>().prop_map(|meta| Op::OkFlush { meta }),
                1 => (any::<usize>(), any::<bool>(), any::<u64>())
                    .prop_map(|(size, torn, pick)| Op::Crash { size, torn, pick }),
                1 => (0..5 * SECTOR).prop_map(Op::SetLen),
            ]
        }

        /// A crash resolution that reduces every choice into range.
        struct Wrap {
            size: usize,
            torn: bool,
            pick: u64,
        }

        impl CrashPick for Wrap {
            fn size(&mut self, sizes: &[u64]) -> usize {
                self.size % sizes.len()
            }
            fn torn(&mut self, dirty: &[u64]) -> Option<u64> {
                if self.torn {
                    dirty.first().copied()
                } else {
                    None
                }
            }
            fn sector(&mut self, s: u64, n: u64) -> u64 {
                (self.pick ^ s) % n
            }
            fn sub(&mut self, s: u64, j: usize, n: u64) -> u64 {
                (self.pick ^ s).wrapping_add(j as u64) % n
            }
            fn beyond(&mut self) -> BeyondFill {
                BeyondFill::Resolved
            }
        }

        /// Whether [`Content::read`] of the first `len` bytes asks for a poison draw.
        fn read_draws(c: &Content, len: u64) -> bool {
            let mut buf = vec![0u8; len.min(c.cs()) as usize];
            let mut draws = 0u32;
            c.read(0, &mut buf, &mut |site, aux, n| {
                assert_eq!(site, Site::PoisonRead);
                draws += 1;
                aux % n
            });
            draws > 0
        }

        proptest! {
            #![proptest_config(ProptestConfig { cases: cases(256), failure_persistence: None, ..ProptestConfig::default() })]

            /// Over any history of writes, size changes, flushes of both outcomes and crashes: the query answers exactly
            /// whether a read of the prefix draws from K (FM-3.2); a failed flush poisons every non-clean sector and
            /// clears none (FM-3.1); a successful flush changes nothing (FM-3.4); a crash keeps every poisoned sector
            /// below the size it picks (FM-3.3); a write or a size change ends the poison of every sector it touches
            /// (FM-3.5).
            #[test]
            fn reads_poisoned_is_what_reads_draw(
                ops in proptest::collection::vec(op(), 1..24),
                lens in proptest::collection::vec(0..6 * SECTOR, 1..6),
            ) {
                let mut c = Content::durable(&[0x11u8; 2 * SEC]);
                let mut merge = |_: Site, aux: u64, n: u64| aux % n;
                for op in ops {
                    let before = c.clone();
                    match op {
                        Op::Write { off, len, byte } => {
                            c.write(off, &vec![byte; len as usize], &mut merge);
                            prop_assert_eq!(
                                c.reads_poisoned(off + len),
                                c.reads_poisoned(off / SECTOR * SECTOR)
                            );
                        }
                        Op::FailFlush => {
                            let mark = c.flush_mark();
                            c.flush_failed(&mark);
                            prop_assert_eq!(c.reads_poisoned(u64::MAX), before.has_unflushed());
                            for &l in &lens {
                                prop_assert!(!before.reads_poisoned(l) || c.reads_poisoned(l));
                            }
                        }
                        Op::OkFlush { meta } => {
                            let mark = c.flush_mark();
                            c.flush_ok(&mark, meta);
                            for &l in &lens {
                                prop_assert_eq!(c.reads_poisoned(l), before.reads_poisoned(l));
                            }
                        }
                        Op::Crash { size, torn, pick } => {
                            c.crash(&mut Wrap { size, torn, pick });
                            for &l in &lens {
                                prop_assert!(!before.reads_poisoned(l.min(c.cs())) || c.reads_poisoned(l));
                            }
                        }
                        Op::SetLen(n) => {
                            c.set_len(n, &mut merge);
                            if n != before.cs() {
                                let edge = before.cs().min(n) / SECTOR * SECTOR;
                                prop_assert_eq!(c.reads_poisoned(u64::MAX), c.reads_poisoned(edge));
                            }
                        }
                    }
                    for &l in &lens {
                        prop_assert_eq!(c.reads_poisoned(l), read_draws(&c, l));
                    }
                    prop_assert_eq!(c.reads_poisoned(u64::MAX), read_draws(&c, u64::MAX));
                }
            }
        }
    }

    #[test]
    fn runs_split_and_merge() {
        let mut r = Runs::default();
        r.insert(0, 10, 1);
        r.insert(10, 20, 1);
        assert_eq!(r.ranges(), vec![(0, 20)]);
        r.insert(5, 7, 2);
        assert_eq!(r.ranges(), vec![(0, 5), (5, 7), (7, 20)]);
        assert_eq!(
            (r.get(4), r.get(5), r.get(19), r.get(20)),
            (Some(1), Some(2), Some(1), None)
        );
        r.clear_older(0, 20, 1);
        assert_eq!(r.ranges(), vec![(5, 7)]);
        r.remove(0, 6);
        assert_eq!(r.ranges(), vec![(6, 7)]);
    }

    #[test]
    fn pages_are_shared_until_written() {
        let c = Content::durable(&[9u8; 3 * SEC]);
        let img = c.clone();
        assert!(Arc::ptr_eq(&c.pages[1], &img.pages[1]));
        let mut c = c;
        c.write(SECTOR + 10, &[1], &mut no_pick());
        assert!(!Arc::ptr_eq(&c.pages[1], &img.pages[1]));
        assert!(Arc::ptr_eq(&c.pages[0], &img.pages[0]));
        assert_eq!(img.to_vec(), vec![9u8; 3 * SEC]);
    }
}
