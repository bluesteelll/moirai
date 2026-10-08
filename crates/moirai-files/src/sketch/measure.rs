//! The exact line measures and the sketch estimates of two contents ([F20 §2.10.1], §2.10.2, §2.10.5).

use core::cmp::{Ordering, min};

use super::fingerprint::{Fingerprint, sketch_hash};
use super::line::LineHasher;
use crate::r14::{EXACT_LIMIT, Ratio, SKETCH_K};
use crate::text::LineSink;

// The hit set of a sketch is a 64-bit mask.
const _: () = assert!(SKETCH_K <= 64);

/// The multiset `M(X)` of the pairs `(fh(f), len(f))` over the fingerprint lines f of a content, with their
/// multiplicities ([F20 §2.10.1]); it exists only for at most `EXACT_LIMIT` fingerprint lines ([F20 §2.10.5]).
///
/// A structure of its own, never part of the pass-1 fingerprint: it holds up to `EXACT_LIMIT` pairs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LineMultiset {
    /// The distinct pairs κ, ascending, with `c_X(κ)`.
    pairs: Vec<((u64, u64), u64)>,
    /// `w(X) = Σ c_X(κ) × len(κ)`.
    weight: u64,
}

impl LineMultiset {
    /// `M(X)` of the given pairs `(fh(f), len(f))`, one per fingerprint line; `None` for more than `EXACT_LIMIT`.
    pub fn from_pairs(pairs: impl IntoIterator<Item = (u64, u32)>) -> Option<LineMultiset> {
        let mut lines = Vec::new();
        for (fh, len) in pairs {
            lines.push((fh, u64::from(len)));
            if lines.len() as u64 > u64::from(EXACT_LIMIT) {
                return None;
            }
        }
        Some(LineMultiset::of_lines(lines))
    }

    /// `w(X)`: the sum of `len(f)` over the fingerprint lines, with multiplicity.
    #[must_use]
    pub fn weight(&self) -> u64 {
        self.weight
    }

    fn of_lines(mut lines: Vec<(u64, u64)>) -> LineMultiset {
        lines.sort_unstable();
        let mut pairs: Vec<((u64, u64), u64)> = Vec::new();
        let mut weight = 0u64;
        for p in lines {
            weight += p.1;
            match pairs.last_mut() {
                Some((q, c)) if *q == p => *c += 1,
                _ => pairs.push((p, 1)),
            }
        }
        LineMultiset { pairs, weight }
    }
}

/// A [`LineSink`] that collects `M(X)` of the content it is fed ([F20 §2.10.1]); it stops collecting past
/// `EXACT_LIMIT` fingerprint lines.
///
/// The lines a read feeds are meaningful only when the read reports text ([`crate::text::LineSink`]); the caller
/// checks `is_text` before it uses [`MultisetSink::finish`]'s multiset.
#[derive(Clone)]
pub struct MultisetSink {
    line: LineHasher,
    /// `(fh(f), len(f))` of each fingerprint line so far.
    lines: Vec<(u64, u64)>,
    /// More than `EXACT_LIMIT` fingerprint lines: the pairs were dropped.
    over: bool,
}

impl MultisetSink {
    /// A sink that has received nothing.
    #[must_use]
    pub const fn new() -> MultisetSink {
        MultisetSink {
            line: LineHasher::new(),
            lines: Vec::new(),
            over: false,
        }
    }

    /// `M(X)` of the content fed, or `None` when it has more than `EXACT_LIMIT` fingerprint lines ([F20 §2.10.5]).
    #[must_use]
    pub fn finish(self) -> Option<LineMultiset> {
        (!self.over).then(|| LineMultiset::of_lines(self.lines))
    }
}

impl Default for MultisetSink {
    fn default() -> MultisetSink {
        MultisetSink::new()
    }
}

impl LineSink for MultisetSink {
    fn begin(&mut self) {
        *self = MultisetSink::new();
    }

    fn piece(&mut self, bytes: &[u8]) {
        if !self.over {
            self.line.piece(bytes);
        }
    }

    fn end_line(&mut self) {
        let Some(p) = self.line.end_line() else {
            return;
        };
        if self.over {
            return;
        }
        self.lines.push(p);
        if self.lines.len() as u64 > u64::from(EXACT_LIMIT) {
            self.over = true;
            self.lines = Vec::new();
        }
    }
}

/// The exact measures of an old content A and a new content B ([F20 §2.10.1]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Exact {
    /// `oin(A, B) = inter / w(A)`, old-in-new containment.
    pub oin: Ratio,
    /// `nio(A, B) = inter / w(B)`, new-in-old containment.
    pub nio: Ratio,
    /// `sym(A, B) = inter / max(w(A), w(B))`, which equals `min(oin, nio)`.
    pub sym: Ratio,
}

/// `oin`, `nio` and `sym` of the old multiset `M(A)` and the new `M(B)` ([F20 §2.10.1]), with
/// `inter = Σ_κ min(c_A(κ), c_B(κ)) × len(κ)`; a measure whose denominator is 0 is 0.
///
/// No sum overflows `u64`: a multiset holds at most `EXACT_LIMIT` lines of one content, and contents are read up to
/// 4 GiB.
#[must_use]
pub fn exact(old: &LineMultiset, new: &LineMultiset) -> Exact {
    let (a, b) = (&old.pairs, &new.pairs);
    let (mut i, mut j, mut inter) = (0, 0, 0u64);
    while i < a.len() && j < b.len() {
        match a[i].0.cmp(&b[j].0) {
            Ordering::Less => i += 1,
            Ordering::Greater => j += 1,
            Ordering::Equal => {
                let ((_, len), c_a) = a[i];
                inter += c_a.min(b[j].1) * len;
                i += 1;
                j += 1;
            }
        }
    }
    let (w_a, w_b) = (old.weight, new.weight);
    Exact {
        oin: Ratio::or_zero(inter, w_a),
        nio: Ratio::or_zero(inter, w_b),
        sym: Ratio::or_zero(inter, w_a.max(w_b)),
    }
}

/// A [`LineSink`] that counts, while a new content B streams, the values of an old sketch `S_A` that equal `sh(f)`
/// for some fingerprint line f of B: `hit` of [F20 §2.10.2]. It holds a copy of `S_A` and a 64-bit mask.
///
/// The lines a read feeds are meaningful only when the read reports text ([`crate::text::LineSink`]); the caller
/// checks `is_text` before it uses [`HitSink::hit`].
#[derive(Clone)]
pub struct HitSink {
    line: LineHasher,
    /// `S_A`, strictly ascending in `[..n]`.
    sketch: [u32; SKETCH_K],
    n: usize,
    /// Bit i: `S_A[i]` was hit.
    mask: u64,
}

impl HitSink {
    /// A sink that counts the hits of `old`'s sketch.
    #[must_use]
    pub fn new(old: &Fingerprint) -> HitSink {
        let s = old.sketch();
        let mut sketch = [0; SKETCH_K];
        sketch[..s.len()].copy_from_slice(s);
        HitSink {
            line: LineHasher::new(),
            sketch,
            n: s.len(),
            mask: 0,
        }
    }

    /// `hit`: how many values of `S_A` the content fed has hit, at most `#S_A`.
    #[must_use]
    pub fn hit(&self) -> u32 {
        self.mask.count_ones()
    }
}

impl LineSink for HitSink {
    fn begin(&mut self) {
        self.line.reset();
        self.mask = 0;
    }

    fn piece(&mut self, bytes: &[u8]) {
        self.line.piece(bytes);
    }

    fn end_line(&mut self) {
        if let Some((fh, _)) = self.line.end_line()
            && let Ok(i) = self.sketch[..self.n].binary_search(&sketch_hash(fh))
        {
            self.mask |= 1 << i;
        }
    }
}

/// The sketch estimates of an old content A known by its fingerprint and a new content B ([F20 §2.10.2]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Estimates {
    /// `eoin(A, B) = hit / #S_A`, 0 when `S_A` is empty.
    pub eoin: Ratio,
    /// `enio(A, B) = min(1, eoin × D_A / D_B)`, 0 when `D_B = 0`.
    pub enio: Ratio,
    /// `esym(A, B) = min(eoin, enio)`.
    pub esym: Ratio,
}

/// `eoin`, `enio` and `esym` ([F20 §2.10.2]) of the old fingerprint, its `hit` count over the new content
/// ([`HitSink`]) and the new fingerprint, whose `distinct` is `D_B`.
///
/// `hit` is at most `#S_A`, the length of `old`'s sketch, as [`HitSink::hit`] gives it. `eoin × D_A / D_B` is
/// `hit × D_A / (#S_A × D_B)`, exact in `u64` (at most 64 × (2^32 − 1) on either side).
#[must_use]
pub fn estimates(old: &Fingerprint, hit: u32, new: &Fingerprint) -> Estimates {
    let n_a = old.sketch().len() as u64;
    let hit = u64::from(hit);
    debug_assert!(hit <= n_a, "a hit count above the sketch's length");
    let eoin = Ratio::or_zero(hit, n_a);
    let (d_a, d_b) = (u64::from(old.distinct()), u64::from(new.distinct()));
    let enio = if d_b == 0 {
        Ratio::ZERO
    } else {
        min(Ratio::ONE, Ratio::or_zero(hit * d_a, n_a * d_b))
    };
    Estimates {
        eoin,
        enio,
        esym: min(eoin, enio),
    }
}
