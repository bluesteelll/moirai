//! The fingerprint of a text content ([F20 §2.6.3]), the pass-1 sink that computes it, and its stored value
//! ([F20 §2.6.4]).

use core::fmt;

use super::line::LineHasher;
use crate::r14::{RESOLVER_VERSION, SKETCH_BITS, SKETCH_K, TINY_BYTES, TINY_LINES};
use crate::text::{LineSink, TextStats};

// `sh(f)` is a `u32` and the sketch an array of them: the module is written for `SKETCH_BITS` = 32.
const _: () = assert!(SKETCH_BITS == u32::BITS);

/// `sh(f) = low(fh(f), SKETCH_BITS)`, the sketch line hash of a line whose full hash is `fh` ([F20 §2.6.2]).
#[inline]
pub(crate) const fn sketch_hash(fh: u64) -> u32 {
    // `low(v, 32)` is `v mod 2^32`: the truncation.
    fh as u32
}

/// The `distinct` estimate when more than `SKETCH_K` values were seen: `max(65, ⌊63 × 2^32 / (s64 + 1)⌋)`, the
/// k-minimum-values estimator with k = `SKETCH_K` ([F20 §2.6.3]); `s64` is the largest sketch value.
///
/// The value fits `u32`: 64 distinct values below `s64` make `s64 ≥ 63`, so the quotient is at most `63 × 2^32 / 64`.
fn kmv_estimate(s64: u32) -> u32 {
    let k = SKETCH_K as u64;
    let raw = ((k - 1) << SKETCH_BITS) / (u64::from(s64) + 1);
    u32::try_from(raw.max(k + 1)).unwrap_or(u32::MAX)
}

/// The fingerprint of a text content ([F20 §2.6.3]): its normalised lines and bytes, the weight of its fingerprint
/// lines, the bottom-`SKETCH_K` sketch of their sketch line hashes, and the number of distinct values, exact up to
/// `SKETCH_K` and estimated above.
///
/// A fixed-size value with no heap memory. The sketch slots past [`Fingerprint::sketch`]'s length are 0, so equality
/// compares the fields and the sketch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fingerprint {
    nlines: u32,
    nbytes: u32,
    weight: u32,
    distinct: u32,
    estimated: bool,
    /// The number of sketch values, at most `SKETCH_K`.
    n: u8,
    /// The sketch, strictly ascending in `[..n]`; 0 from `n` on.
    sketch: [u32; SKETCH_K],
}

impl Fingerprint {
    /// `nlines`: the number of lines of `norm(b)` ([F20 §2.6.3]).
    #[must_use]
    pub const fn nlines(&self) -> u32 {
        self.nlines
    }

    /// `nbytes`: `len(norm(b))` ([F20 §2.6.3]).
    #[must_use]
    pub const fn nbytes(&self) -> u32 {
        self.nbytes
    }

    /// `weight`: the sum of `len(f)` over the fingerprint lines, with multiplicity ([F20 §2.6.3]).
    #[must_use]
    pub const fn weight(&self) -> u32 {
        self.weight
    }

    /// `distinct`: the number of distinct sketch line hashes, exact up to `SKETCH_K` and estimated above
    /// ([F20 §2.6.3]).
    #[must_use]
    pub const fn distinct(&self) -> u32 {
        self.distinct
    }

    /// Whether [`Fingerprint::distinct`] is the estimate: more than `SKETCH_K` distinct values were seen.
    #[must_use]
    pub const fn distinct_estimated(&self) -> bool {
        self.estimated
    }

    /// The sketch: the `min(SKETCH_K, #V)` smallest sketch line hashes, strictly ascending ([F20 §2.6.3]).
    #[must_use]
    pub fn sketch(&self) -> &[u32] {
        &self.sketch[..usize::from(self.n)]
    }

    /// Whether the content is tiny by its fingerprint, `nlines < TINY_LINES` or `nbytes < TINY_BYTES` ([F20 §2.6.5]):
    /// the text form of the test. Binary content has no fingerprint; its form, and a file node's, are the caller's.
    #[must_use]
    pub fn is_tiny(&self) -> bool {
        u64::from(self.nlines) < TINY_LINES || u64::from(self.nbytes) < TINY_BYTES
    }

    /// The fingerprint value ([F20 §2.6.4]): the sequence table `ver`, `n_sketch`, `flags`, `nlines`, `nbytes`,
    /// `weight`, `distinct`, `sketch`, every integer little-endian ([F01 §2.6]), `20 + 4 × n_sketch` bytes.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(HEADER_LEN + 4 * usize::from(self.n));
        out.extend_from_slice(&RESOLVER_VERSION.to_le_bytes());
        out.push(self.n);
        out.push(if self.estimated { FLAG_ESTIMATED } else { 0 });
        for v in [self.nlines, self.nbytes, self.weight, self.distinct] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        for v in self.sketch() {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out
    }

    /// The fingerprint a stored value holds ([F20 §2.6.4]).
    ///
    /// # Errors
    /// The checks run in this order, and the first that fails decides:
    /// 1. [`FingerprintError::Version`]: `ver` is not [`RESOLVER_VERSION`], so the value counts as absent
    ///    ([F20 §1.3]); a value of fewer than 2 bytes has no `ver` and is [`FingerprintError::Length`];
    /// 2. [`FingerprintError::Length`]: fewer than 20 bytes, or not exactly `20 + 4 × n_sketch` (a sequence table
    ///    defines every byte, [F01 §2.6]);
    /// 3. [`FingerprintError::SketchCount`]: `n_sketch > SKETCH_K`;
    /// 4. [`FingerprintError::ReservedFlags`]: a reserved bit of `flags` (1–7) is set ([F01 §10]);
    /// 5. [`FingerprintError::NotAscending`]: the sketch is not strictly ascending;
    /// 6. [`FingerprintError::Distinct`]: `n_sketch < 64` while `distinct ≠ n_sketch` or the flag is set;
    ///    `n_sketch = 64` with the flag clear while `distinct ≠ 64`, or with the flag set while `distinct < 65`.
    ///
    /// Nothing else is refused: [F20 §2.6.4] lists no other invalid case.
    pub fn from_bytes(b: &[u8]) -> Result<Fingerprint, FingerprintError> {
        let &[v0, v1, ..] = b else {
            return Err(FingerprintError::Length(b.len()));
        };
        let ver = u16::from_le_bytes([v0, v1]);
        if ver != RESOLVER_VERSION {
            return Err(FingerprintError::Version(ver));
        }
        if b.len() < HEADER_LEN {
            return Err(FingerprintError::Length(b.len()));
        }
        let n_sketch = b[2];
        let n = usize::from(n_sketch);
        if b.len() != HEADER_LEN + 4 * n {
            return Err(FingerprintError::Length(b.len()));
        }
        if n > SKETCH_K {
            return Err(FingerprintError::SketchCount(n_sketch));
        }
        let flags = b[3];
        if flags & RESERVED_FLAGS != 0 {
            return Err(FingerprintError::ReservedFlags(flags));
        }
        let u32_at = |i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
        let mut sketch = [0; SKETCH_K];
        for (k, slot) in sketch[..n].iter_mut().enumerate() {
            *slot = u32_at(HEADER_LEN + 4 * k);
        }
        if !sketch[..n].windows(2).all(|w| w[0] < w[1]) {
            return Err(FingerprintError::NotAscending);
        }
        let estimated = flags & FLAG_ESTIMATED != 0;
        let distinct = u32_at(16);
        let consistent = if n < SKETCH_K {
            !estimated && u64::from(distinct) == n as u64
        } else if estimated {
            u64::from(distinct) > SKETCH_K as u64
        } else {
            u64::from(distinct) == SKETCH_K as u64
        };
        if !consistent {
            return Err(FingerprintError::Distinct);
        }
        Ok(Fingerprint {
            nlines: u32_at(4),
            nbytes: u32_at(8),
            weight: u32_at(12),
            distinct,
            estimated,
            n: n_sketch,
            sketch,
        })
    }
}

/// The bytes of the fixed fields of a fingerprint value, `ver` to `distinct` ([F20 §2.6.4]).
const HEADER_LEN: usize = 20;

/// Bit 0 of `flags`: `distinct_estimated` ([F20 §2.6.4]).
const FLAG_ESTIMATED: u8 = 0x01;

/// Bits 1–7 of `flags`: reserved-zero ([F20 §2.6.4], [F01 §10]).
const RESERVED_FLAGS: u8 = 0xFE;

/// Why bytes are not a valid fingerprint value of this resolver version ([F20 §2.6.4]); see
/// [`Fingerprint::from_bytes`] for the order of the checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FingerprintError {
    /// `ver` is another resolver version (0 included), whose fingerprint counts as absent ([F20 §1.3], §2.6.4).
    Version(u16),
    /// The value's length: below 20 bytes, or not `20 + 4 × n_sketch`.
    Length(usize),
    /// `n_sketch` is above `SKETCH_K` = 64.
    SketchCount(u8),
    /// The `flags` byte has a reserved bit (1–7) set.
    ReservedFlags(u8),
    /// The sketch is not strictly ascending.
    NotAscending,
    /// `distinct` or the `distinct_estimated` flag does not agree with `n_sketch`.
    Distinct,
}

impl fmt::Display for FingerprintError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FingerprintError::Version(v) => {
                write!(
                    f,
                    "a fingerprint of resolver version {v}, not {RESOLVER_VERSION}"
                )
            }
            FingerprintError::Length(n) => {
                write!(f, "a fingerprint value of {n} bytes, not 20 + 4 × n_sketch")
            }
            FingerprintError::SketchCount(n) => {
                write!(
                    f,
                    "a fingerprint with {n} sketch values, more than {SKETCH_K}"
                )
            }
            FingerprintError::ReservedFlags(b) => {
                write!(f, "a fingerprint with reserved flag bits set: {b:#04x}")
            }
            FingerprintError::NotAscending => {
                f.write_str("a fingerprint sketch not strictly ascending")
            }
            FingerprintError::Distinct => {
                f.write_str("a fingerprint whose distinct count disagrees with its sketch")
            }
        }
    }
}

impl std::error::Error for FingerprintError {}

/// The pass-1 sink that computes the [`Fingerprint`] of the content the reader streams ([F20 §2.4], §2.6).
///
/// Feed it to [`crate::text::analyse`] or [`crate::text::ContentReader::read`], then call
/// [`FingerprintSink::finish`] with the statistics of the same read. Its state is fixed: the line hasher, the weight,
/// the `SKETCH_K` sketch values and a flag, with no heap memory, whatever the content's size. Every read attempt
/// starts with [`LineSink::begin`], which forgets everything received before, a half-received line included.
#[derive(Clone)]
pub struct FingerprintSink {
    line: LineHasher,
    /// The sum of `len(f)` so far.
    weight: u64,
    /// The smallest distinct values of `sh(f)` so far, strictly ascending in `[..n]`; 0 from `n` on.
    sketch: [u32; SKETCH_K],
    n: usize,
    /// A value outside the sketch was seen: one was evicted, or one above a full sketch's largest arrived, so
    /// `#V > SKETCH_K` ([F20 §2.6.3]: "it saw a value outside the final sketch").
    overflow: bool,
}

impl FingerprintSink {
    /// A sink that has received nothing.
    #[must_use]
    pub const fn new() -> FingerprintSink {
        FingerprintSink {
            line: LineHasher::new(),
            weight: 0,
            sketch: [0; SKETCH_K],
            n: 0,
            overflow: false,
        }
    }

    /// The fingerprint of the content whose read produced `stats` and fed this sink ([F20 §2.6.3]).
    ///
    /// `None` when the content is binary, or when `nbytes = len(norm(b))` is 2^32 or more ([F20 §2.6.3]: "a fingerprint
    /// exists only for text content with `nbytes < 2^32`"). `nlines` and `nbytes` are quantities of `norm(b)`, which
    /// the lines of the anchor text do not give (no leading BOM, no final `0A`), so they come from `stats`.
    #[must_use]
    pub fn finish(&self, stats: &TextStats) -> Option<Fingerprint> {
        if !stats.is_text() {
            return None;
        }
        let nbytes = u32::try_from(stats.norm_len()).ok()?;
        // Every line of `norm(b)` holds a byte of it, so `nlines ≤ nbytes`, and the fingerprint lines are disjoint
        // parts of `norm(b)`, so `weight ≤ nbytes`: neither conversion fails for one read's sink and statistics.
        let nlines = u32::try_from(stats.nlines()).ok()?;
        let weight = u32::try_from(self.weight).ok()?;
        let distinct = if self.overflow {
            kmv_estimate(self.sketch[SKETCH_K - 1])
        } else {
            // `n ≤ SKETCH_K`.
            self.n as u32
        };
        Some(Fingerprint {
            nlines,
            nbytes,
            weight,
            distinct,
            estimated: self.overflow,
            n: self.n as u8,
            sketch: self.sketch,
        })
    }

    /// One fingerprint line with full hash `fh` and length `len`.
    fn add(&mut self, fh: u64, len: u64) {
        self.weight = self.weight.saturating_add(len);
        let v = sketch_hash(fh);
        let full = self.n == SKETCH_K;
        if full && v > self.sketch[SKETCH_K - 1] {
            self.overflow = true;
            return;
        }
        let Err(i) = self.sketch[..self.n].binary_search(&v) else {
            // A value already in the sketch.
            return;
        };
        if full {
            // `v` is below the largest value, which leaves the sketch.
            self.overflow = true;
            self.sketch.copy_within(i..SKETCH_K - 1, i + 1);
        } else {
            self.sketch.copy_within(i..self.n, i + 1);
            self.n += 1;
        }
        self.sketch[i] = v;
    }
}

impl Default for FingerprintSink {
    fn default() -> FingerprintSink {
        FingerprintSink::new()
    }
}

impl LineSink for FingerprintSink {
    fn begin(&mut self) {
        *self = FingerprintSink::new();
    }

    fn piece(&mut self, bytes: &[u8]) {
        self.line.piece(bytes);
    }

    fn end_line(&mut self) {
        if let Some((fh, len)) = self.line.end_line() {
            self.add(fh, len);
        }
    }
}
