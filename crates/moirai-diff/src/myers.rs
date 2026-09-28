//! The k-bounded Myers bit-parallel matcher: approximate matching of a byte pattern (an anchor quote) against a text,
//! with Levenshtein distance over bytes and unit costs ([F20 §6.4], [40 §4.5] step 4).
//!
//! Each column of the dynamic-programming matrix is held as vertical-delta bit vectors, 64 pattern rows per block
//! (G. Myers, "A fast bit-vector algorithm for approximate string matching based on dynamic programming", JACM 46(3),
//! 1999, §4–§5). Only the blocks that can hold a value ≤ k are computed (Ukkonen's cut-off, Myers §5): a search costs
//! O(⌈k/64⌉) word operations per text byte in the expected case, O(⌈m/64⌉) at worst, and O(⌈m/64⌉) words of state.
//!
//! Three computations share one column step:
//! - [`Searcher`] (semi-global, the top row is 0): for every end offset e of the text, d(e) = the least distance
//!   between the pattern and a substring `text[s..e)`; every e with d(e) ≤ k is reported. The text may be fed in
//!   chunks ([40 §4.5]: the scan streams through a fixed buffer), and the state carries across chunk boundaries.
//! - [`Pattern::locate`] (the pattern reversed, the text read backwards from e, the top row counts): the largest start
//!   s with distance d(e) ([F20 §6.4]: "s is the largest start with distance d(e)").
//! - [`Pattern::distance`] (global, both ends anchored): the Levenshtein distance used by the prefix and suffix scores
//!   of [F20 §6.4].
//!
//! [`crate::Selector`] turns a searcher's hits into [F20 §6.4]'s candidates as they stream in.

/// Pattern rows per block.
const W: usize = 64;

/// The high bit of a full block: the block's bottom row.
const HIGH: u64 = 1 << (W - 1);

/// Blocks up to which [`Pattern::locate`] and [`Pattern::distance`] keep their column on the stack. A quote is at most
/// `QUOTE_MAX` bytes ([F20 §6.1], draft 128), which is two blocks.
const INLINE_BLOCKS: usize = 4;

/// A budget that no distance reaches: every block stays active.
const K_INF: i64 = i64::MAX / 4;

#[cfg(test)]
thread_local! {
    /// Test-only work counters of this thread: blocks advanced by banded columns, and columns kept on the heap.
    /// Speed-only choices (the band's cut-off, the early stop of `locate`, the side `levenshtein` takes as the
    /// pattern) change them and nothing else.
    static WORK: std::cell::Cell<(usize, usize)> = const { std::cell::Cell::new((0, 0)) };
}

/// Adds to the test-only work counters.
#[cfg(test)]
fn count(blocks: usize, heap_columns: usize) {
    WORK.with(|w| {
        let (b, h) = w.get();
        w.set((b + blocks, h + heap_columns));
    });
}

/// A reported end offset of an approximate match ([F20 §6.4] "candidates in a region").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Hit {
    /// The end offset e (exclusive) in the searched text, counted from the first byte fed.
    pub end: usize,
    /// d(e): the least distance between the pattern and a substring of the searched text that ends at `end`.
    pub distance: usize,
}

/// The start of a best approximate match that ends at a given offset ([`Pattern::locate`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Located {
    /// The largest start s whose substring `text[s..end)` is at `distance` from the pattern.
    pub start: usize,
    /// The least distance between the pattern and `text[s..end)` over the allowed starts.
    pub distance: usize,
}

/// A preprocessed pattern: the match-vector tables of the pattern and of its reverse.
///
/// Bytes are mapped to classes: one class per distinct byte of the pattern, and one all-zero class for every other
/// byte. The tables hold one 64-bit word per (class, block), so a 128-byte quote with 40 distinct bytes needs
/// 2 × 41 × 2 × 8 B ≈ 1.3 KiB.
#[derive(Clone, Debug)]
pub struct Pattern {
    len: usize,
    blocks: usize,
    last_high: u64,
    class: [u8; 256],
    peq: Box<[u64]>,
    peq_rev: Box<[u64]>,
}

/// The fixed shape of a pattern's blocks.
#[derive(Clone, Copy, Debug)]
struct Geom {
    /// Index of the last block.
    last: usize,
    /// The bit of the last block that holds pattern row m.
    last_high: u64,
    /// Pattern length m.
    len: usize,
}

impl Geom {
    /// Pattern rows in block `b`.
    fn rows(self, b: usize) -> i64 {
        let rows = if b == self.last {
            self.len - W * self.last
        } else {
            W
        };
        rows as i64
    }

    /// The bottom row index of block `b`, which is also its value in column 0 (D[r][0] = r).
    fn bottom(self, b: usize) -> i64 {
        (W * (b + 1)).min(self.len) as i64
    }

    fn high(self, b: usize) -> u64 {
        if b == self.last { self.last_high } else { HIGH }
    }
}

/// Advances one block by one text byte (Myers 1999, `advance_block`). `eq` is the block's match vector for the byte,
/// `hin` the horizontal delta entering at the block's top row, `high` the bit of the block's bottom row. Returns the
/// horizontal delta leaving the bottom row.
#[inline]
fn advance(pv: &mut u64, mv: &mut u64, eq: u64, hin: i64, high: u64) -> i64 {
    let (p, m) = (*pv, *mv);
    let xv = eq | m;
    let eq = if hin < 0 { eq | 1 } else { eq };
    let xh = ((eq & p).wrapping_add(p) ^ p) | eq;
    let mut ph = m | !(xh | p);
    let mut mh = p & xh;
    let hout = if ph & high != 0 {
        1
    } else if mh & high != 0 {
        -1
    } else {
        0
    };
    ph <<= 1;
    mh <<= 1;
    if hin < 0 {
        mh |= 1;
    } else if hin > 0 {
        ph |= 1;
    }
    *pv = mh | !(xv | ph);
    *mv = ph & xv;
    hout
}

/// One banded column: the vertical deltas and bottom scores of blocks `0..=y`, where every block after `y` holds only
/// values > k (Myers 1999 §5). A value ≤ k is always exact; a value > k may be over-estimated, which never matters.
struct Column<'s> {
    pv: &'s mut [u64],
    mv: &'s mut [u64],
    score: &'s mut [i64],
    y: usize,
}

impl Column<'_> {
    /// Column 0 (D[r][0] = r), with the blocks that may hold a value ≤ k active.
    fn init(&mut self, g: Geom, k: i64) {
        let want = usize::try_from(k.max(0)).map_or(g.last, |k| k.div_ceil(W).saturating_sub(1));
        self.y = want.min(g.last);
        for b in 0..=self.y {
            self.pv[b] = !0;
            self.mv[b] = 0;
            self.score[b] = g.bottom(b);
        }
    }

    /// Advances the column by one text byte whose match vectors are `eq`. `hin0` is the horizontal delta of the top
    /// boundary row: 0 for a search (D[0][j] = 0), +1 for a global alignment (D[0][j] = j). Returns D[m][j] when the
    /// last block is active.
    #[inline]
    fn step(&mut self, g: Geom, k: i64, hin0: i64, eq: &[u64]) -> Option<i64> {
        let mut carry = hin0;
        let y = self.y;
        let active = self.pv[..=y]
            .iter_mut()
            .zip(&mut self.mv[..=y])
            .zip(&mut self.score[..=y])
            .zip(&eq[..=y])
            .enumerate();
        for (b, (((pv, mv), score), &e)) in active {
            carry = advance(pv, mv, e, carry, g.high(b));
            *score += carry;
        }
        #[cfg(test)]
        count(y + 1, 0);
        // The next block can reach a value ≤ k in this column only through its top row: diagonally from a value ≤ k
        // in the previous column with a match, or vertically from a bottom value that just decreased (Myers §5).
        if y < g.last && self.score[y] - carry <= k && (eq[y + 1] & 1 != 0 || carry < 0) {
            let n = y + 1;
            #[cfg(test)]
            count(1, 0);
            self.pv[n] = !0;
            self.mv[n] = 0;
            let before = self.score[y] - carry + g.rows(n);
            let h = advance(&mut self.pv[n], &mut self.mv[n], eq[n], carry, g.high(n));
            self.score[n] = before + h;
            self.y = n;
        } else {
            // A block whose bottom value is ≥ k + rows holds only values > k: vertical deltas are at most 1.
            while self.y > 0 && self.score[self.y] >= k + g.rows(self.y) {
                self.y -= 1;
            }
        }
        (self.y == g.last).then(|| self.score[g.last])
    }
}

/// Converts a caller's budget to the column's signed budget.
fn budget(k: usize) -> i64 {
    i64::try_from(k).map_or(K_INF, |k| k.min(K_INF))
}

impl Pattern {
    /// Preprocesses `pattern`. The empty pattern is allowed: it matches every offset at distance 0.
    #[must_use]
    pub fn new(pattern: &[u8]) -> Self {
        let len = pattern.len();
        let blocks = len.div_ceil(W);
        let mut present = [false; 256];
        for &b in pattern {
            present[usize::from(b)] = true;
        }
        let mut class = [0u8; 256];
        let mut distinct = 0usize;
        for (b, &here) in present.iter().enumerate() {
            if here {
                class[b] = distinct as u8;
                distinct += 1;
            }
        }
        // The all-zero class exists only when some byte is absent, and then `distinct` ≤ 255.
        let rows = if distinct < 256 {
            for (b, &here) in present.iter().enumerate() {
                if !here {
                    class[b] = distinct as u8;
                }
            }
            distinct + 1
        } else {
            256
        };
        let mut peq = vec![0u64; rows * blocks].into_boxed_slice();
        let mut peq_rev = vec![0u64; rows * blocks].into_boxed_slice();
        for (r, &b) in pattern.iter().enumerate() {
            let c = usize::from(class[usize::from(b)]);
            peq[c * blocks + r / W] |= 1 << (r % W);
            let rr = len - 1 - r;
            peq_rev[c * blocks + rr / W] |= 1 << (rr % W);
        }
        let last_high = if len == 0 { 0 } else { 1 << ((len - 1) % W) };
        Self {
            len,
            blocks,
            last_high,
            class,
            peq,
            peq_rev,
        }
    }

    /// The pattern length m in bytes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether the pattern is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Heap bytes held by the pattern's tables.
    #[must_use]
    pub fn heap_bytes(&self) -> usize {
        (self.peq.len() + self.peq_rev.len()) * size_of::<u64>()
    }

    fn geom(&self) -> Geom {
        Geom {
            last: self.blocks.saturating_sub(1),
            last_high: self.last_high,
            len: self.len,
        }
    }

    #[inline]
    fn row<'t>(&self, table: &'t [u64], byte: u8) -> &'t [u64] {
        let c = usize::from(self.class[usize::from(byte)]);
        &table[c * self.blocks..(c + 1) * self.blocks]
    }

    /// A streaming search with error budget k ([F20 §6.4]: k = ⌊`FUZZY_BUDGET` × len(exact)⌋).
    #[must_use]
    pub fn searcher(&self, k: usize) -> Searcher<'_> {
        Searcher::new(self, k)
    }

    /// Reports, in increasing order of `end`, every end offset e of `text` (0 ≤ e ≤ `text.len()`) with d(e) ≤ k, where
    /// d(e) is the least distance between the pattern and a substring `text[s..e)` ([F20 §6.4]). The search region
    /// is the whole of `text`: pass the region's slice to bound the starts.
    pub fn search(&self, text: &[u8], k: usize, on_hit: impl FnMut(Hit)) {
        self.searcher(k).feed(text, on_hit);
    }

    /// The best match that ends at `end`: over the starts s with `lo` ≤ s ≤ `end`, the least distance d between the
    /// pattern and `text[s..end)`, and the largest s with that distance ([F20 §6.4]). Returns `None` when d > k.
    ///
    /// Called with the region start as `lo` and a hit's `end` and `distance` as k, it returns that hit's start. Only
    /// the m + k bytes before `end` are read, because a longer substring is more than k away. `end` is clamped to
    /// `text.len()` and `lo` to `end`.
    #[must_use]
    pub fn locate(&self, text: &[u8], end: usize, lo: usize, k: usize) -> Option<Located> {
        let end = end.min(text.len());
        let lo = lo.min(end);
        let m = self.len;
        if m == 0 {
            return Some(Located {
                start: end,
                distance: 0,
            });
        }
        let reach = (end - lo).min(m.saturating_add(k));
        let kb = budget(k);
        // (distance, length) of the best so far; the empty substring is m away.
        let mut best: Option<(usize, usize)> = (m <= k).then_some((m, 0));
        self.with_column(|col, g| {
            col.init(g, kb);
            for l in 1..=reach {
                let eq = self.row(&self.peq_rev, text[end - l]);
                // Only a value ≤ k is exact under the cut-off.
                if let Some(d) = col.step(g, kb, 1, eq).filter(|&d| d <= kb) {
                    let d = d as usize;
                    if best.is_none_or(|(bd, _)| d < bd) {
                        best = Some((d, l));
                    }
                }
                // Every longer substring is at least its excess over m away.
                if best.is_some_and(|(bd, _)| l + 1 >= m + bd) {
                    break;
                }
            }
        });
        best.map(|(distance, l)| Located {
            start: end - l,
            distance,
        })
    }

    /// The Levenshtein distance between the pattern and all of `text` (unit costs over bytes, [F20 §6.4]).
    #[must_use]
    pub fn distance(&self, text: &[u8]) -> usize {
        let m = self.len;
        if m == 0 {
            return text.len();
        }
        self.with_column(|col, g| {
            col.init(g, K_INF);
            let mut d = m as i64;
            for &t in text {
                if let Some(v) = col.step(g, K_INF, 1, self.row(&self.peq, t)) {
                    d = v;
                }
            }
            d as usize
        })
    }

    /// Runs `f` on a fresh column, on the stack for short patterns.
    fn with_column<R>(&self, f: impl FnOnce(&mut Column<'_>, Geom) -> R) -> R {
        let g = self.geom();
        let n = self.blocks;
        if n <= INLINE_BLOCKS {
            let (mut pv, mut mv, mut score) = (
                [0u64; INLINE_BLOCKS],
                [0u64; INLINE_BLOCKS],
                [0i64; INLINE_BLOCKS],
            );
            let mut col = Column {
                pv: &mut pv[..n],
                mv: &mut mv[..n],
                score: &mut score[..n],
                y: 0,
            };
            f(&mut col, g)
        } else {
            #[cfg(test)]
            count(0, 1);
            let (mut pv, mut mv, mut score) = (vec![0u64; n], vec![0u64; n], vec![0i64; n]);
            let mut col = Column {
                pv: &mut pv,
                mv: &mut mv,
                score: &mut score,
                y: 0,
            };
            f(&mut col, g)
        }
    }
}

/// The Levenshtein distance between two byte strings, with unit costs ([F20 §6.4]: the prefix and suffix scores).
#[must_use]
pub fn levenshtein(a: &[u8], b: &[u8]) -> usize {
    let (short, long) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    Pattern::new(short).distance(long)
}

/// A search in progress: the pattern's column after the bytes fed so far ([`Pattern::searcher`]).
///
/// The text may arrive in chunks of any size; the result equals one [`Pattern::search`] over their concatenation.
#[derive(Clone, Debug)]
pub struct Searcher<'p> {
    pat: &'p Pattern,
    k: i64,
    pv: Vec<u64>,
    mv: Vec<u64>,
    score: Vec<i64>,
    y: usize,
    pos: usize,
    started: bool,
}

impl<'p> Searcher<'p> {
    fn new(pat: &'p Pattern, k: usize) -> Self {
        // Every end offset is within m of the pattern (s = e), so a budget ≥ m reports every offset.
        let k = budget(k.min(pat.len));
        let n = pat.blocks;
        let mut s = Self {
            pat,
            k,
            pv: vec![0; n],
            mv: vec![0; n],
            score: vec![0; n],
            y: 0,
            pos: 0,
            started: false,
        };
        if n > 0 {
            let mut col = Column {
                pv: &mut s.pv,
                mv: &mut s.mv,
                score: &mut s.score,
                y: 0,
            };
            col.init(pat.geom(), k);
            s.y = col.y;
        }
        s
    }

    /// Bytes fed so far: the offset of the next byte.
    #[must_use]
    pub fn position(&self) -> usize {
        self.pos
    }

    /// Feeds the next bytes of the text and reports every hit whose end lies in this chunk, in increasing order. The
    /// first call also reports end offset 0 when the empty substring is within the budget (m ≤ k).
    pub fn feed(&mut self, chunk: &[u8], mut on_hit: impl FnMut(Hit)) {
        let pat = self.pat;
        let m = pat.len as i64;
        if !self.started {
            self.started = true;
            if m <= self.k {
                on_hit(Hit {
                    end: 0,
                    distance: pat.len,
                });
            }
        }
        if pat.blocks == 0 {
            for _ in chunk {
                self.pos += 1;
                on_hit(Hit {
                    end: self.pos,
                    distance: 0,
                });
            }
            return;
        }
        let k = self.k;
        if pat.blocks == 1 {
            // One block: the band is always the whole column.
            let (mut pv, mut mv, mut score) = (self.pv[0], self.mv[0], self.score[0]);
            let high = pat.last_high;
            #[cfg(test)]
            count(chunk.len(), 0);
            for &t in chunk {
                let eq = pat.peq[usize::from(pat.class[usize::from(t)])];
                score += advance(&mut pv, &mut mv, eq, 0, high);
                self.pos += 1;
                if score <= k {
                    on_hit(Hit {
                        end: self.pos,
                        distance: score as usize,
                    });
                }
            }
            (self.pv[0], self.mv[0], self.score[0]) = (pv, mv, score);
            return;
        }
        let g = pat.geom();
        let mut col = Column {
            pv: &mut self.pv,
            mv: &mut self.mv,
            score: &mut self.score,
            y: self.y,
        };
        for &t in chunk {
            let hit = col.step(g, k, 0, pat.row(&pat.peq, t));
            self.pos += 1;
            if let Some(d) = hit.filter(|&d| d <= k) {
                on_hit(Hit {
                    end: self.pos,
                    distance: d as usize,
                });
            }
        }
        self.y = col.y;
    }
}

#[cfg(test)]
mod tests;
