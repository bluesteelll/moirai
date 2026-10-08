//! Token winnowing and its measure `J` ([F20 §2.9]; Schleimer, Wilkerson and Aiken 2003).

use std::collections::BTreeSet;

use xxhash_rust::xxh3::{Xxh3Default, xxh3_64};

use crate::r14::{EXACT_LIMIT, Ratio, WINNOW_K, WINNOW_W};
use crate::text::{LineSink, is_ws, lines};

// A k-gram holds at least one token and a window at least one k-gram.
const _: () = assert!(WINNOW_K >= 1 && WINNOW_W >= 1);

/// The byte classes of [F20 §2.9]'s tokens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Class {
    /// A `WS` byte: separates tokens and is none.
    Ws,
    /// A word byte, `30`–`39`, `41`–`5A`, `61`–`7A`, `5F` or ≥ `80`: a maximal run is one token.
    Word,
    /// Any other byte: a one-byte token.
    Other,
}

/// The class of byte `b`; the one classifier of [`tokens`] and [`WinnowSink`].
#[inline]
fn class(b: u8) -> Class {
    if is_ws(b) {
        Class::Ws
    } else if b.is_ascii_alphanumeric() || b == b'_' || b >= 0x80 {
        Class::Word
    } else {
        Class::Other
    }
}

/// The token sequence T of `t` ([F20 §2.9]): every maximal run of word bytes is one token, every byte that is
/// neither a word byte nor in `WS` is a one-byte token, and `WS` bytes separate tokens.
#[must_use]
pub fn tokens(t: &[u8]) -> Tokens<'_> {
    Tokens { rest: t }
}

/// The iterator of [`tokens`].
#[derive(Clone, Debug)]
pub struct Tokens<'a> {
    rest: &'a [u8],
}

impl<'a> Iterator for Tokens<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<&'a [u8]> {
        let Some(start) = self.rest.iter().position(|&b| class(b) != Class::Ws) else {
            self.rest = &[];
            return None;
        };
        let r = &self.rest[start..];
        let len = if class(r[0]) == Class::Word {
            r.iter()
                .position(|&b| class(b) != Class::Word)
                .unwrap_or(r.len())
        } else {
            1
        };
        let (token, rest) = r.split_at(len);
        self.rest = rest;
        Some(token)
    }
}

/// The selection of [F20 §2.9] over a stream of k-gram hashes, in a ring of the last `WINNOW_W`: the one routine of
/// [`select`] and [`WinnowSink`].
#[derive(Clone, Debug)]
struct Selector {
    /// `g_j` at slot `j mod WINNOW_W`.
    ring: [u64; WINNOW_W],
    /// G, the k-grams pushed so far.
    seen: u64,
}

impl Selector {
    const W: u64 = WINNOW_W as u64;

    const fn new() -> Selector {
        Selector {
            ring: [0; WINNOW_W],
            seen: 0,
        }
    }

    /// Pushes `g_j` for the next j; once j ≥ W − 1, the window `g_(j−W+1) … g_j` is complete and its selection is
    /// returned as `(position, value)`.
    fn push(&mut self, g: u64) -> Option<(u64, u64)> {
        let j = self.seen;
        self.ring[(j % Selector::W) as usize] = g;
        self.seen += 1;
        (self.seen >= Selector::W).then(|| self.window(j + 1 - Selector::W, Selector::W))
    }

    /// The end of the stream: when 1 ≤ G < W, the one window over all k-grams; otherwise nothing more (G = 0 selects
    /// nothing, and every window of G ≥ W was returned by [`Selector::push`]).
    fn finish(&self) -> Option<(u64, u64)> {
        (1..Selector::W)
            .contains(&self.seen)
            .then(|| self.window(0, self.seen))
    }

    /// The minimum of `g_start … g_(start+len−1)`, the rightmost position among equal minima.
    fn window(&self, start: u64, len: u64) -> (u64, u64) {
        let at = |p: u64| self.ring[(p % Selector::W) as usize];
        let (mut pos, mut min) = (start, at(start));
        for p in start + 1..start + len {
            let v = at(p);
            if v <= min {
                pos = p;
                min = v;
            }
        }
        (pos, min)
    }
}

/// The position each window of [F20 §2.9] selects in `hashes` (`g_0 … g_(G−1)`), one entry per window in window
/// order: G − W + 1 entries when G ≥ W, one when 1 ≤ G < W, none when G = 0. Among equal minima the rightmost
/// position wins.
///
/// `FW` is a set of values, which the tie rule does not change; the positions are where the rule shows.
#[must_use]
pub fn select(hashes: &[u64]) -> Vec<usize> {
    let mut s = Selector::new();
    let mut out = Vec::with_capacity(hashes.len().saturating_sub(WINNOW_W - 1).max(1));
    for &g in hashes {
        if let Some((p, _)) = s.push(g) {
            out.push(p as usize);
        }
    }
    if let Some((p, _)) = s.finish() {
        out.push(p as usize);
    }
    out
}

/// The winnowing fingerprint set `FW(t)` ([F20 §2.9]): distinct values, ascending, at most `EXACT_LIMIT` of them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WinnowSet {
    values: Vec<u64>,
}

impl WinnowSet {
    /// `|FW(t)|`.
    #[must_use]
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Whether `FW(t)` is empty (fewer than `WINNOW_K` tokens).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// The values, ascending.
    #[must_use]
    pub fn values(&self) -> &[u64] {
        &self.values
    }
}

/// A [`LineSink`] that computes `FW(t)` of the anchor text it is fed ([F20 §2.9]): tokens over pieces (a word token may
/// span pieces), the k-gram hashes over a ring of the last `WINNOW_K` token hashes, and the selection over a ring of
/// `WINNOW_W` k-gram hashes. The set stops growing past `EXACT_LIMIT` values, where `J` is not defined.
///
/// The lines a read feeds are meaningful only when the read reports text ([`crate::text::LineSink`]); the caller
/// checks `is_text` before it uses [`WinnowSink::finish`]'s set.
#[derive(Clone)]
pub struct WinnowSink {
    /// XXH3-64 of the word token open across pieces.
    word: Xxh3Default,
    /// A word token is open.
    in_word: bool,
    /// `th(t_i)` at slot `i mod WINNOW_K`.
    th: [u64; WINNOW_K],
    /// m, the tokens so far.
    m: u64,
    selector: Selector,
    set: BTreeSet<u64>,
    /// `|FW| > EXACT_LIMIT`: the set was dropped.
    over: bool,
}

impl WinnowSink {
    const K: u64 = WINNOW_K as u64;

    /// A sink that has received nothing.
    #[must_use]
    pub const fn new() -> WinnowSink {
        WinnowSink {
            word: Xxh3Default::new(),
            in_word: false,
            th: [0; WINNOW_K],
            m: 0,
            selector: Selector::new(),
            set: BTreeSet::new(),
            over: false,
        }
    }

    /// `FW(t)` of the text fed, or `None` when it has more than `EXACT_LIMIT` values ([F20 §2.10.5]).
    #[must_use]
    pub fn finish(mut self) -> Option<WinnowSet> {
        self.end_word();
        if let Some((_, v)) = self.selector.finish() {
            self.insert(v);
        }
        (!self.over).then(|| WinnowSet {
            values: self.set.into_iter().collect(),
        })
    }

    fn end_word(&mut self) {
        if self.in_word {
            self.in_word = false;
            let th = self.word.digest();
            self.word.reset();
            self.token(th);
        }
    }

    /// The next token, by its hash `th(t_m)`.
    fn token(&mut self, th: u64) {
        let m = self.m;
        self.th[(m % WinnowSink::K) as usize] = th;
        self.m += 1;
        if self.m < WinnowSink::K {
            return;
        }
        // `g_j = XXH3-64(u64(th(t_j)) ‖ … ‖ u64(th(t_(j+K−1))))`, little-endian, for j = m − K + 1.
        let mut buf = [0u8; 8 * WINNOW_K];
        let first = self.m - WinnowSink::K;
        for (k, chunk) in buf.as_chunks_mut::<8>().0.iter_mut().enumerate() {
            let slot = ((first + k as u64) % WinnowSink::K) as usize;
            *chunk = self.th[slot].to_le_bytes();
        }
        if let Some((_, v)) = self.selector.push(xxh3_64(&buf)) {
            self.insert(v);
        }
    }

    fn insert(&mut self, v: u64) {
        if self.over {
            return;
        }
        self.set.insert(v);
        if self.set.len() as u64 > u64::from(EXACT_LIMIT) {
            self.over = true;
            self.set = BTreeSet::new();
        }
    }
}

impl Default for WinnowSink {
    fn default() -> WinnowSink {
        WinnowSink::new()
    }
}

impl LineSink for WinnowSink {
    fn begin(&mut self) {
        *self = WinnowSink::new();
    }

    fn piece(&mut self, bytes: &[u8]) {
        if self.over {
            return;
        }
        let mut i = 0;
        while i < bytes.len() {
            match class(bytes[i]) {
                Class::Ws => {
                    self.end_word();
                    i += 1;
                }
                Class::Word => {
                    let start = i;
                    while i < bytes.len() && class(bytes[i]) == Class::Word {
                        i += 1;
                    }
                    self.word.update(&bytes[start..i]);
                    self.in_word = true;
                }
                Class::Other => {
                    self.end_word();
                    self.token(xxh3_64(&bytes[i..=i]));
                    i += 1;
                }
            }
        }
    }

    fn end_line(&mut self) {
        // The line's `0A` is a `WS` byte.
        self.end_word();
    }
}

/// `FW(t)` of an anchor text in memory ([F20 §2.9]), or `None` when it has more than `EXACT_LIMIT` values.
#[must_use]
pub fn winnow(t: &[u8]) -> Option<WinnowSet> {
    let mut sink = WinnowSink::new();
    sink.begin();
    for l in lines(t) {
        if !l.is_empty() {
            sink.piece(l);
        }
        sink.end_line();
    }
    sink.finish()
}

/// `J(A, B) = |FW(A) ∩ FW(B)| / |FW(A) ∪ FW(B)|`, 0 when the union is empty ([F20 §2.9]). Both sets exist only within
/// `EXACT_LIMIT`, so `J` is defined for every pair.
#[must_use]
pub fn jaccard(a: &WinnowSet, b: &WinnowSet) -> Ratio {
    let (x, y) = (a.values(), b.values());
    let (mut i, mut j, mut inter) = (0, 0, 0u64);
    while i < x.len() && j < y.len() {
        match x[i].cmp(&y[j]) {
            core::cmp::Ordering::Less => i += 1,
            core::cmp::Ordering::Greater => j += 1,
            core::cmp::Ordering::Equal => {
                inter += 1;
                i += 1;
                j += 1;
            }
        }
    }
    let union = (x.len() + y.len()) as u64 - inter;
    Ratio::or_zero(inter, union)
}
