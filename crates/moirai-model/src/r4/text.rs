//! The content functions of resolver version 1 ([F20 §2]), each a literal transcription over whole buffers: byte
//! statistics and `is_text`, EOL normalisation, `oid`, the anchor text with its lines and trivial lines, fingerprint
//! lines, the fingerprint and its estimates, the exact measures, the window hash and the window value, the span hash,
//! the header text of a `heading` or `symbol` line, and the git pair score of E6's inexact pairs.
//!
//! Every score is an exact rational ([`Ratio`]); no floating point is used ([F20 §1.2]).

use crate::value::{Algo, Oid};
use sha1::Digest as _;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use xxhash_rust::xxh3::xxh3_64;

/// `WS` = {`09`, `0A`, `0B`, `0C`, `0D`, `20`} ([F20 §1.2]).
pub fn is_ws(b: u8) -> bool {
    matches!(b, 0x09..=0x0D | 0x20)
}

/// An exact non-negative rational number, compared by cross-multiplication ([F20 §1.2]).
#[derive(Clone, Copy, Debug)]
pub struct Ratio {
    /// The numerator.
    pub num: u128,
    /// The denominator, ≥ 1.
    pub den: u128,
}

impl Ratio {
    /// `num / den`; a zero denominator gives 0 ("a measure whose denominator is 0 is 0", [F20 §2.10.1]).
    pub fn new(num: u128, den: u128) -> Ratio {
        if den == 0 {
            Ratio { num: 0, den: 1 }
        } else {
            Ratio { num, den }
        }
    }

    /// The integer `n`.
    pub fn int(n: u128) -> Ratio {
        Ratio { num: n, den: 1 }
    }

    /// The smaller of two values.
    pub fn min(self, o: Ratio) -> Ratio {
        if self <= o { self } else { o }
    }

    /// The larger of two values.
    pub fn max(self, o: Ratio) -> Ratio {
        if self >= o { self } else { o }
    }

    /// `self − o`, 0 when `o ≥ self` (margins are taken between an ordered best and runner-up).
    pub fn saturating_sub(self, o: Ratio) -> Ratio {
        if o >= self {
            return Ratio::int(0);
        }
        Ratio::new(self.num * o.den - o.num * self.den, self.den * o.den)
    }

    /// `self × k`.
    pub fn mul_int(self, k: u128) -> Ratio {
        Ratio::new(self.num * k, self.den)
    }

    /// The value in lowest terms.
    pub fn reduced(self) -> Ratio {
        let g = gcd(self.num, self.den).max(1);
        Ratio {
            num: self.num / g,
            den: self.den / g,
        }
    }
}

fn gcd(a: u128, b: u128) -> u128 {
    if b == 0 { a } else { gcd(b, a % b) }
}

impl std::ops::Add for Ratio {
    type Output = Ratio;

    /// `self + o`.
    fn add(self, o: Ratio) -> Ratio {
        Ratio::new(self.num * o.den + o.num * self.den, self.den * o.den)
    }
}

impl PartialEq for Ratio {
    fn eq(&self, o: &Ratio) -> bool {
        self.cmp(o) == Ordering::Equal
    }
}

impl Eq for Ratio {}

impl PartialOrd for Ratio {
    fn partial_cmp(&self, o: &Ratio) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

impl Ord for Ratio {
    fn cmp(&self, o: &Ratio) -> Ordering {
        (self.num * o.den).cmp(&(o.num * self.den))
    }
}

/// The byte statistics of [F20 §2.1].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    /// CR LF pairs.
    pub crlf: usize,
    /// CRs not followed by LF.
    pub lonecr: usize,
    /// NUL bytes.
    pub nul: usize,
    /// Printable bytes.
    pub printable: usize,
    /// Non-printable bytes, a final `1A` not counted.
    pub nonprintable: usize,
}

/// git's `gather_stats` over the whole content.
// spec: [F20 §2.1]
pub fn stats(b: &[u8]) -> Stats {
    let mut s = Stats::default();
    let n = b.len();
    let mut i = 0;
    while i < n {
        let c = b[i];
        if c == 0x0D {
            if i + 1 < n && b[i + 1] == 0x0A {
                s.crlf += 1;
                i += 2;
                continue;
            }
            s.lonecr += 1;
            i += 1;
            continue;
        }
        if c == 0x0A {
            i += 1;
            continue;
        }
        if c == 0x7F {
            s.nonprintable += 1;
        } else if c < 0x20 {
            if matches!(c, 0x08 | 0x09 | 0x0C | 0x1B) {
                s.printable += 1;
            } else {
                if c == 0 {
                    s.nul += 1;
                }
                s.nonprintable += 1;
            }
        } else {
            s.printable += 1;
        }
        i += 1;
    }
    if n >= 1 && b[n - 1] == 0x1A {
        s.nonprintable -= 1;
    }
    s
}

/// `is_text(b)`: no lone CR, no NUL, and `(printable >> 7) ≥ nonprintable`.
// spec: [F20 §2.1]
pub fn is_text(b: &[u8]) -> bool {
    let s = stats(b);
    s.lonecr == 0 && s.nul == 0 && (s.printable >> 7) >= s.nonprintable
}

/// `norm(b)`: every CR LF pair of a text content replaced by LF; binary content unchanged.
// spec: [F20 §2.2]
pub fn norm(b: &[u8]) -> Vec<u8> {
    if !is_text(b) {
        return b.to_vec();
    }
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == 0x0D && i + 1 < b.len() && b[i + 1] == 0x0A {
            out.push(0x0A);
            i += 2;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    out
}

/// `H("blob " ‖ dec(len(x)) ‖ 00 ‖ x)` with H the algorithm's hash.
fn blob_hash(algo: Algo, x: &[u8]) -> Oid {
    let mut header = format!("blob {}", x.len()).into_bytes();
    header.push(0);
    let digest = match algo {
        Algo::Sha1 => {
            let mut h = sha1::Sha1::new();
            h.update(&header);
            h.update(x);
            h.finalize().to_vec()
        }
        Algo::Sha256 => {
            let mut h = sha2::Sha256::new();
            h.update(&header);
            h.update(x);
            h.finalize().to_vec()
        }
    };
    Oid { algo, digest }
}

/// `oid_H(b) = H("blob " ‖ dec(len(norm(b))) ‖ 00 ‖ norm(b))`.
// spec: [F20 §2.3]
pub fn oid(algo: Algo, b: &[u8]) -> Oid {
    blob_hash(algo, &norm(b))
}

/// The `oid` of a symbolic link: over its target text, `norm` not applied ([80 §2.10] P8).
// spec: [F20 §2.3] symbolic links; [OS/path §3] P8
pub fn symlink_oid(algo: Algo, target: &[u8]) -> Oid {
    blob_hash(algo, target)
}

/// The three-valued test "`oid(q) ∈ S`" ([F20 §2.3]): `Some(true)` when the value equals an element, `Some(false)`
/// when it equals none and every element has its algorithm, `None` (unknown) otherwise.
// spec: [F20 §2.3] comparison
pub fn oid_in(v: &Oid, set: &[&Oid]) -> Option<bool> {
    if set.contains(&v) {
        return Some(true);
    }
    if set.iter().all(|s| s.algo == v.algo) {
        Some(false)
    } else {
        None
    }
}

/// `atext(b)`: `norm(b)` with one leading `EF BB BF` removed; defined only for text content.
// spec: [F20 §2.5] anchor text
pub fn atext(b: &[u8]) -> Option<Vec<u8>> {
    if !is_text(b) {
        return None;
    }
    let n = norm(b);
    Some(match n.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        Some(rest) => rest.to_vec(),
        None => n,
    })
}

/// `lines(t)`: t split at every `0A`, the last piece dropped when empty.
// spec: [F20 §2.5] lines
pub fn lines(t: &[u8]) -> Vec<&[u8]> {
    if t.is_empty() {
        return Vec::new();
    }
    let mut v: Vec<&[u8]> = t.split(|&b| b == 0x0A).collect();
    if v.last().is_some_and(|l| l.is_empty()) {
        v.pop();
    }
    v
}

/// `nl(l)`: every leading and trailing `WS` byte removed.
// spec: [F20 §2.5] normalised line
pub fn nl(l: &[u8]) -> &[u8] {
    let s = l.iter().position(|&b| !is_ws(b)).unwrap_or(l.len());
    let e = l.iter().rposition(|&b| !is_ws(b)).map_or(s, |i| i + 1);
    &l[s..e.max(s)]
}

/// A line is trivial iff every byte of `nl(l)` is `WS` or one of `{ } ( ) [ ] ; ,`.
// spec: [F20 §2.5] trivial line
pub fn trivial(l: &[u8]) -> bool {
    nl(l)
        .iter()
        .all(|&b| is_ws(b) || matches!(b, b'{' | b'}' | b'(' | b')' | b'[' | b']' | b';' | b','))
}

/// The normalised anchor text `N(t)` with each line's offsets ([F20 §2.5]).
#[derive(Clone, Debug)]
pub struct NText {
    /// `N(t)`.
    pub n: Vec<u8>,
    /// `start(i)` of line i + 1.
    pub starts: Vec<usize>,
    /// `end(i)` of line i + 1.
    pub ends: Vec<usize>,
    /// Whether line i + 1 is trivial.
    pub trivial: Vec<bool>,
    /// `wh(l)` of line i + 1 when it is non-trivial.
    pub wh: Vec<Option<u16>>,
}

impl NText {
    /// `N(t)` of an anchor text.
    // spec: [F20 §2.5] normalised anchor text
    pub fn of(t: &[u8]) -> NText {
        let ls = lines(t);
        let mut n = Vec::with_capacity(t.len());
        let mut starts = Vec::with_capacity(ls.len());
        let mut ends = Vec::with_capacity(ls.len());
        let mut triv = Vec::with_capacity(ls.len());
        let mut whs = Vec::with_capacity(ls.len());
        for (i, l) in ls.iter().enumerate() {
            if i > 0 {
                n.push(0x0A);
            }
            starts.push(n.len());
            n.extend_from_slice(nl(l));
            ends.push(n.len());
            let tr = trivial(l);
            triv.push(tr);
            whs.push((!tr).then(|| wh(l)));
        }
        NText {
            n,
            starts,
            ends,
            trivial: triv,
            wh: whs,
        }
    }

    /// The number of lines.
    pub fn len(&self) -> usize {
        self.starts.len()
    }

    /// Whether the text has no line.
    pub fn is_empty(&self) -> bool {
        self.starts.is_empty()
    }

    /// `start(i)`, 1-based.
    pub fn start(&self, i: usize) -> usize {
        self.starts[i - 1]
    }

    /// `end(i)`, 1-based.
    pub fn end(&self, i: usize) -> usize {
        self.ends[i - 1]
    }

    /// `ST(s, e) = N[start(s) .. end(e))`.
    // spec: [F20 §2.5] span text
    pub fn st(&self, s: usize, e: usize) -> &[u8] {
        &self.n[self.start(s)..self.end(e)]
    }

    /// The 1-based line of the byte at offset `o` of N (the line whose range, `0A` excluded, holds it, or the line the
    /// `0A` at `o` ends).
    pub fn line_of(&self, o: usize) -> usize {
        match self.starts.binary_search(&o) {
            Ok(i) => i + 1,
            Err(i) => i.max(1),
        }
    }

    /// `before(s)`: the window hashes of the last `min(win, k)` non-trivial lines before line s, in file order.
    // spec: [F20 §2.7.2]
    pub fn before(&self, s: usize, win: usize) -> Vec<u16> {
        let mut v: Vec<u16> = (1..s)
            .rev()
            .filter_map(|i| self.wh[i - 1])
            .take(win)
            .collect();
        v.reverse();
        v
    }

    /// `after(e)`: the window hashes of the first `min(win, k′)` non-trivial lines after line e, in file order.
    // spec: [F20 §2.7.2]
    pub fn after(&self, e: usize, win: usize) -> Vec<u16> {
        (e + 1..=self.len())
            .filter_map(|i| self.wh[i - 1])
            .take(win)
            .collect()
    }
}

/// `wh(l) = low(XXH3-64(nl(l)), 16)`.
// spec: [F20 §2.7.1]
pub fn wh(l: &[u8]) -> u16 {
    (xxh3_64(nl(l)) & 0xFFFF) as u16
}

/// The window value W: `n_before u16`, `n_after u16`, the before hashes and the after hashes, little-endian.
// spec: [F20 §2.7.3]
pub fn window_value(before: &[u16], after: &[u16]) -> Vec<u8> {
    let mut w = Vec::with_capacity(4 + 2 * (before.len() + after.len()));
    w.extend_from_slice(&(before.len() as u16).to_le_bytes());
    w.extend_from_slice(&(after.len() as u16).to_le_bytes());
    for h in before.iter().chain(after) {
        w.extend_from_slice(&h.to_le_bytes());
    }
    w
}

/// The before and after hashes of a window value, or `None` for a value that is not valid (a count above `win`, or a
/// length that differs from `4 + 2 × (n_before + n_after)`).
// spec: [F20 §2.7.3]
pub fn parse_window(w: &[u8], win: usize) -> Option<(Vec<u16>, Vec<u16>)> {
    if w.len() < 4 {
        return None;
    }
    let nb = usize::from(u16::from_le_bytes([w[0], w[1]]));
    let na = usize::from(u16::from_le_bytes([w[2], w[3]]));
    if nb > win || na > win || w.len() != 4 + 2 * (nb + na) {
        return None;
    }
    let h: Vec<u16> = w[4..]
        .chunks(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    Some((h[..nb].to_vec(), h[nb..].to_vec()))
}

/// `chars(x)`: the bytes of x outside `80`–`BF` ([F20 §1.2]).
pub fn chars(x: &[u8]) -> usize {
    x.iter().filter(|&&b| !(0x80..=0xBF).contains(&b)).count()
}

/// `cutp(x, n)`: the longest prefix of at most n bytes whose next byte, if any, is not in `80`–`BF`.
// spec: [F20 §1.2] UTF-8-safe cuts
pub fn cutp(x: &[u8], n: usize) -> &[u8] {
    let mut k = n.min(x.len());
    while k < x.len() && k > 0 && (0x80..=0xBF).contains(&x[k]) {
        k -= 1;
    }
    &x[..k]
}

/// `cuts(x, n)`: the longest suffix of at most n bytes whose first byte, if any, is not in `80`–`BF`.
// spec: [F20 §1.2] UTF-8-safe cuts
pub fn cuts(x: &[u8], n: usize) -> &[u8] {
    let mut s = x.len().saturating_sub(n);
    while s < x.len() && (0x80..=0xBF).contains(&x[s]) {
        s += 1;
    }
    &x[s..]
}

/// `collapse(x)`: every maximal run of `WS` bytes replaced by one `20`.
fn collapse(x: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(x.len());
    let mut in_ws = false;
    for &b in x {
        if is_ws(b) {
            if !in_ws {
                out.push(0x20);
            }
            in_ws = true;
        } else {
            out.push(b);
            in_ws = false;
        }
    }
    out
}

/// The fingerprint lines of a text content's anchor text, with multiplicity: `collapse(nl(l))` with more than
/// `FP_MIN_CHARS` = 3 characters.
// spec: [F20 §2.6.1]
pub fn fingerprint_lines(t: &[u8]) -> Vec<Vec<u8>> {
    lines(t)
        .into_iter()
        .map(|l| collapse(nl(l)))
        .filter(|f| chars(f) > 3)
        .collect()
}

/// `sh(f) = low(XXH3-64(f), 32)`.
// spec: [F20 §2.6.2]
pub fn sh(f: &[u8]) -> u32 {
    (xxh3_64(f) & 0xFFFF_FFFF) as u32
}

/// A fingerprint value ([F20 §2.6.3], §2.6.4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fingerprint {
    /// `nlines`: the lines of `norm(b)`.
    pub nlines: u32,
    /// `nbytes`: `len(norm(b))`.
    pub nbytes: u32,
    /// `weight`.
    pub weight: u32,
    /// `distinct`.
    pub distinct: u32,
    /// `distinct_estimated`.
    pub estimated: bool,
    /// The bottom-64 sketch, ascending.
    pub sketch: Vec<u32>,
}

/// The fingerprint of a content, `None` for binary content or `nbytes ≥ 2^32`.
// spec: [F20 §2.6.3]
pub fn fingerprint(b: &[u8]) -> Option<Fingerprint> {
    let t = atext(b)?;
    let nb = norm(b);
    let nbytes = u32::try_from(nb.len()).ok()?;
    let nlines = lines(&nb).len() as u32;
    let fl = fingerprint_lines(&t);
    let weight: usize = fl.iter().map(Vec::len).sum();
    let v: BTreeSet<u32> = fl.iter().map(|f| sh(f)).collect();
    let sketch: Vec<u32> = v.iter().copied().take(64).collect();
    let (distinct, estimated) = if v.len() <= 64 {
        (v.len() as u32, false)
    } else {
        let s64 = u128::from(*sketch.last().expect("64 values"));
        let est = (63u128 << 32) / (s64 + 1);
        (est.max(65).min(u128::from(u32::MAX)) as u32, true)
    };
    Some(Fingerprint {
        nlines,
        nbytes,
        weight: weight as u32,
        distinct,
        estimated,
        sketch,
    })
}

/// The fingerprint value's bytes ([F20 §2.6.4]).
// spec: [F20 §2.6.4]
pub fn fingerprint_bytes(fp: &Fingerprint) -> Vec<u8> {
    let mut out = Vec::with_capacity(20 + 4 * fp.sketch.len());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.push(fp.sketch.len() as u8);
    out.push(u8::from(fp.estimated));
    for v in [fp.nlines, fp.nbytes, fp.weight, fp.distinct] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    for s in &fp.sketch {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

/// Whether a content is tiny: text with `nlines < 5` or `nbytes < 64`, or binary content shorter than 64 bytes.
// spec: [F20 §2.6.5]
pub fn tiny_content(b: &[u8]) -> bool {
    match fingerprint(b) {
        Some(fp) => fp.nlines < 5 || fp.nbytes < 64,
        None => b.len() < 64,
    }
}

/// The sketch estimates of [F20 §2.10.2] for an old content known by its fingerprint and a new content read in full:
/// (`eoin`, `enio`).
// spec: [F20 §2.10.2]
pub fn estimates(old: &Fingerprint, new: &[u8]) -> (Ratio, Ratio) {
    let nf = fingerprint(new);
    let hashes: BTreeSet<u32> = atext(new)
        .map(|t| fingerprint_lines(&t).iter().map(|f| sh(f)).collect())
        .unwrap_or_default();
    let hit = old.sketch.iter().filter(|s| hashes.contains(s)).count();
    let eoin = Ratio::new(hit as u128, old.sketch.len() as u128);
    let db = nf.map_or(0, |f| f.distinct);
    let enio = if db == 0 {
        Ratio::int(0)
    } else {
        Ratio::new(
            eoin.num * u128::from(old.distinct),
            eoin.den * u128::from(db),
        )
        .min(Ratio::int(1))
    };
    (eoin, enio)
}

/// The exact measures of [F20 §2.10.1] over two contents: (`oin`, `nio`, `sym`).
// spec: [F20 §2.10.1]
pub fn exact_measures(old: &[u8], new: &[u8]) -> (Ratio, Ratio, Ratio) {
    let m = |b: &[u8]| -> BTreeMap<(u64, usize), u128> {
        let mut m = BTreeMap::new();
        if let Some(t) = atext(b) {
            for f in fingerprint_lines(&t) {
                *m.entry((xxh3_64(&f), f.len())).or_insert(0) += 1;
            }
        }
        m
    };
    let (a, b) = (m(old), m(new));
    let w = |x: &BTreeMap<(u64, usize), u128>| -> u128 {
        x.iter().map(|((_, l), c)| c * (*l as u128)).sum()
    };
    let inter: u128 = a
        .iter()
        .map(|(k, ca)| b.get(k).map_or(0, |cb| ca.min(cb) * (k.1 as u128)))
        .sum();
    let (wa, wb) = (w(&a), w(&b));
    (
        Ratio::new(inter, wa),
        Ratio::new(inter, wb),
        Ratio::new(inter, wa.max(wb)),
    )
}

/// `EXACT_LIMIT` ([F20 §2.10.5]): the most fingerprint lines a side may have for the exact measures.
pub const EXACT_LIMIT: usize = 65_536;

/// The containments (old-in-new, new-in-old) of two contents read in full ([F20 §5.11.4] "Containments"): the exact
/// `oin` and `nio` of §2.10.1 while each side has at most `EXACT_LIMIT` fingerprint lines, else the estimates `eoin`
/// and `enio` of §2.10.2 against the old side's fingerprint ([F20 §2.10.5]; 0 and 0 when the old side has none).
// spec: [F20 §2.10.5]; [F20 §5.11.4] containments
pub fn containments(old: &[u8], new: &[u8]) -> (Ratio, Ratio) {
    let lines_of = |b: &[u8]| atext(b).map_or(0, |t| fingerprint_lines(&t).len());
    if lines_of(old) <= EXACT_LIMIT && lines_of(new) <= EXACT_LIMIT {
        let (oin, nio, _) = exact_measures(old, new);
        return (oin, nio);
    }
    match fingerprint(old) {
        Some(fp) => estimates(&fp, new),
        None => (Ratio::int(0), Ratio::int(0)),
    }
}

/// The span counts of a blob for the git pair score ([F20 §5.11.4]).
fn span_counts(b: &[u8]) -> BTreeMap<u32, u64> {
    let n = b.len();
    let text = !b[..n.min(8000)].contains(&0);
    let mut s: BTreeMap<u32, u64> = BTreeMap::new();
    let (mut a1, mut a2, mut k) = (0u32, 0u32, 0u64);
    for i in 0..n {
        let c = b[i];
        if text && c == 0x0D && i + 1 < n && b[i + 1] == 0x0A {
            continue;
        }
        let t = a1;
        a1 = (a1 << 7) ^ (a2 >> 25);
        a2 = (a2 << 7) ^ (t >> 25);
        a1 = a1.wrapping_add(u32::from(c));
        k += 1;
        if k < 64 && c != 0x0A {
            continue;
        }
        let h = a1.wrapping_add(a2.wrapping_mul(97)) % 107_927;
        *s.entry(h).or_insert(0) += k;
        k = 0;
        a1 = 0;
        a2 = 0;
    }
    if k > 0 {
        let h = a1.wrapping_add(a2.wrapping_mul(97)) % 107_927;
        *s.entry(h).or_insert(0) += k;
    }
    s
}

/// `gs(X, a)`: git's similarity index of two regular-file blobs, an integer percentage 0–100.
// spec: [F20 §5.11.4] the git pair score
pub fn git_pair_score(x: &[u8], a: &[u8]) -> u32 {
    let m = x.len().max(a.len()) as u128;
    if m == 0 {
        return 0;
    }
    let (sx, sa) = (span_counts(x), span_counts(a));
    let shared: u128 = sx
        .iter()
        .map(|(h, cx)| sa.get(h).map_or(0, |ca| u128::from(*cx.min(ca))))
        .sum();
    (100 * shared / m) as u32
}

// ---------------------------------------------------------------------------------------------------------------
// The header text of a line ([F20 §2.8]) with the Rust tokenisation of [F21 §3.1] rules 1–4.
// ---------------------------------------------------------------------------------------------------------------

/// The length of the whitespace sequence of [F21 §3.1] rule 1 that starts at `i`, 0 when none does.
fn ws_len(x: &[u8], i: usize) -> usize {
    let b = x[i];
    if (0x09..=0x0D).contains(&b) || b == 0x20 {
        return 1;
    }
    let rest = &x[i..];
    if rest.starts_with(&[0xC2, 0x85]) {
        return 2;
    }
    for seq in [
        [0xE2, 0x80, 0x8E],
        [0xE2, 0x80, 0x8F],
        [0xE2, 0x80, 0xA8],
        [0xE2, 0x80, 0xA9],
    ] {
        if rest.starts_with(&seq) {
            return 3;
        }
    }
    0
}

/// Whether the byte at `i` is a word byte ([F21 §3.1] rule 3).
fn word_byte(x: &[u8], i: usize) -> bool {
    let b = x[i];
    b.is_ascii_alphanumeric() || b == b'_' || (b >= 0x80 && ws_len(x, i) == 0)
}

/// The length of the character at `i` ([F21 §3.1] rule 3): a well-formed UTF-8 sequence, else one byte.
fn char_len(x: &[u8], i: usize) -> usize {
    for len in [1usize, 2, 3, 4] {
        if i + len <= x.len() && std::str::from_utf8(&x[i..i + len]).is_ok() {
            return len;
        }
    }
    1
}

/// A token of [F21 §3.1] rule 4, as the header scan needs it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tok {
    /// A string literal of any form (plain, byte, C or raw).
    Str,
    /// Another literal (a character or byte character).
    Lit,
    /// A word, raw identifier or lifetime.
    Word,
    /// One punctuation byte.
    Punct(u8),
}

/// Scans a quoted literal body from `i` (just after the opening quote) to after the closing `q`, `\` taking the next
/// byte; an unterminated literal runs to the end.
fn scan_quoted(x: &[u8], mut i: usize, q: u8) -> usize {
    while i < x.len() {
        if x[i] == b'\\' {
            i += 2;
            continue;
        }
        if x[i] == q {
            return i + 1;
        }
        i += 1;
    }
    x.len()
}

fn word_run(x: &[u8], mut i: usize) -> usize {
    while i < x.len() && word_byte(x, i) {
        i += 1;
    }
    i
}

/// The next token from `i` (whitespace and comments already skipped): its kind and its end.
fn next_token(x: &[u8], i: usize) -> (Tok, usize) {
    let b = x[i];
    let suffix = |end: usize| word_run(x, end);
    if b == b'"' {
        return (Tok::Str, suffix(scan_quoted(x, i + 1, b'"')));
    }
    if word_byte(x, i) {
        let e = word_run(x, i);
        let run = &x[i..e];
        let next = x.get(e).copied();
        if (run == b"b" || run == b"c") && next == Some(b'"') {
            return (Tok::Str, suffix(scan_quoted(x, e + 1, b'"')));
        }
        if run == b"b" && next == Some(b'\'') {
            return (Tok::Lit, suffix(scan_quoted(x, e + 1, b'\'')));
        }
        if run == b"r" || run == b"br" || run == b"cr" {
            let mut j = e;
            while j < x.len() && x[j] == b'#' {
                j += 1;
            }
            if x.get(j) == Some(&b'"') {
                let hashes = j - e;
                let mut k = j + 1;
                while k < x.len() {
                    if x[k] == b'"'
                        && x[k + 1..]
                            .iter()
                            .take(hashes)
                            .filter(|&&c| c == b'#')
                            .count()
                            == hashes
                        && x.len() >= k + 1 + hashes
                    {
                        return (Tok::Str, suffix(k + 1 + hashes));
                    }
                    k += 1;
                }
                return (Tok::Str, x.len());
            }
            if run == b"r" && next == Some(b'#') && e + 1 < x.len() && word_byte(x, e + 1) {
                return (Tok::Word, word_run(x, e + 1));
            }
        }
        return (Tok::Word, e);
    }
    if b == b'\'' {
        if x.get(i + 1) == Some(&b'\\') {
            return (Tok::Lit, suffix(scan_quoted(x, i + 1, b'\'')));
        }
        if i + 1 < x.len() && x[i + 1] != b'\'' {
            let cl = char_len(x, i + 1);
            if x.get(i + 1 + cl) == Some(&b'\'') {
                return (Tok::Lit, suffix(i + 2 + cl));
            }
            if word_byte(x, i + 1) {
                return (Tok::Word, word_run(x, i + 1));
            }
        }
        return (Tok::Punct(b'\''), i + 1);
    }
    (Tok::Punct(b), i + 1)
}

/// The header text of the line that starts at `start` in N, for a `symbol` anchor: its byte range in N.
// spec: [F20 §2.8] header text
pub fn header_symbol(n: &[u8], start: usize) -> std::ops::Range<usize> {
    const QUAL: [&[u8]; 7] = [
        b"pub", b"const", b"async", b"unsafe", b"safe", b"extern", b"default",
    ];
    let mut i = start;
    let mut depth = 0usize;
    let mut real = false;
    let mut end = n.len();
    while i < n.len() {
        // A 0A outside every comment and literal, at depth 0, once the header holds a non-qualifier token at depth 0.
        if n[i] == 0x0A && depth == 0 && real {
            end = i;
            break;
        }
        let w = ws_len(n, i);
        if w > 0 {
            i += w;
            continue;
        }
        if n[i..].starts_with(b"//") {
            while i < n.len() && n[i] != 0x0A {
                i += 1;
            }
            continue;
        }
        if n[i..].starts_with(b"/*") {
            let mut level = 0usize;
            while i < n.len() {
                if n[i..].starts_with(b"/*") {
                    level += 1;
                    i += 2;
                } else if n[i..].starts_with(b"*/") {
                    level -= 1;
                    i += 2;
                    if level == 0 {
                        break;
                    }
                } else {
                    i += 1;
                }
            }
            continue;
        }
        let (tok, e) = next_token(n, i);
        match tok {
            Tok::Punct(b'{') | Tok::Punct(b';') if depth == 0 => {
                end = i;
                break;
            }
            // `(` and `)` are header qualifiers; `[` and `]` are not, so either met at depth 0 makes the header hold a
            // real token ([F20 §2.8] rule 2: a TOML `[package]` line ends the header at its line break).
            Tok::Punct(b'(') => {
                depth += 1;
            }
            Tok::Punct(b'[') => {
                if depth == 0 {
                    real = true;
                }
                depth += 1;
            }
            Tok::Punct(b')') => {
                depth = depth.saturating_sub(1);
            }
            Tok::Punct(b']') => {
                if depth == 0 {
                    real = true;
                }
                depth = depth.saturating_sub(1);
            }
            Tok::Str => {}
            Tok::Word if depth == 0 && QUAL.contains(&&n[i..e]) => {}
            _ if depth == 0 => real = true,
            _ => {}
        }
        i = e;
    }
    let mut e = end;
    while e > start && is_ws(n[e - 1]) {
        e -= 1;
    }
    start..e
}

/// `header(l, kind)` for a `heading` (the normalised line) or a `symbol` ([`header_symbol`]); the header text bytes.
// spec: [F20 §2.8]
pub fn header(nt: &NText, l: usize, symbol: bool) -> &[u8] {
    if symbol {
        &nt.n[header_symbol(&nt.n, nt.start(l))]
    } else {
        nt.st(l, l)
    }
}

/// `XXH3-64` of bytes ([F01 §7.1]; the span hash of [F20 §2.8]).
pub fn xxh3(b: &[u8]) -> u64 {
    xxh3_64(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hexs(o: &Oid) -> String {
        crate::value::hex(&o.digest)
    }

    /// The informative table of [F20 §2.3].
    #[test]
    fn worked_values_of_f20_2_3() {
        assert!(is_text(b""));
        assert_eq!(
            hexs(&oid(Algo::Sha1, b"")),
            "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391"
        );
        assert_eq!(norm(b"hello\r\n"), b"hello\n");
        assert_eq!(
            hexs(&oid(Algo::Sha1, b"hello\r\n")),
            "ce013625030ba8dba906f756967f9e9ca394464a"
        );
        assert_eq!(
            hexs(&oid(Algo::Sha256, b"hello\r\n")),
            "2cf8d83d9ee29543b34a87727421fdecb7e3f3a183d337639025de576db9ebb4"
        );
        assert_eq!(
            hexs(&oid(Algo::Sha1, b"a\r\nb")),
            "0a207c060e61f3b88eaee0a8cd0696f46fb155eb"
        );
        assert!(!is_text(b"a\rb"));
        assert_eq!(
            hexs(&oid(Algo::Sha1, b"a\rb")),
            "2fe40ba389048204a83882bc3f75bf2188db6d47"
        );
        assert!(is_text(b"x\r\n\x1a"));
        assert_eq!(
            hexs(&oid(Algo::Sha1, b"x\r\n\x1a")),
            "2f484e3cd37e421218bc8a9929df89ceb50dbb1a"
        );
        let mut b = vec![0x41u8; 127];
        b.push(1);
        assert!(!is_text(&b));
        let mut b = vec![0x41u8; 128];
        b.push(1);
        assert!(is_text(&b));
        assert!(!is_text(b"a\0b"));
    }

    /// The golden vectors of [F20 §5.11.4].
    #[test]
    fn git_pair_score_golden_vectors() {
        let crlf = |s: &str| s.replace('\n', "\r\n");
        let base = "alpha\nbeta\ngamma\ndelta\n";
        assert_eq!(
            git_pair_score(base.as_bytes(), b"alpha\nbeta\ngamma\nepsilon\n"),
            68
        );
        assert_eq!(git_pair_score(base.as_bytes(), crlf(base).as_bytes()), 85);
        let a = format!("{}\n", "x".repeat(130));
        let b = format!("{}yy\n", "x".repeat(128));
        assert_eq!(git_pair_score(a.as_bytes(), b.as_bytes()), 97);
        let ten = "one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten\n";
        let five = "one\ntwo\nthree\nfour\nfive\n";
        assert_eq!(git_pair_score(ten.as_bytes(), five.as_bytes()), 48);
        assert_eq!(
            git_pair_score(
                "line\n".repeat(10).as_bytes(),
                "line\n".repeat(5).as_bytes()
            ),
            50
        );
    }

    /// The header examples of [F20 §2.8].
    #[test]
    fn header_examples_of_f20_2_8() {
        let h = |src: &str| {
            let nt = NText::of(src.as_bytes());
            String::from_utf8(header(&nt, 1, true).to_vec()).unwrap()
        };
        assert_eq!(h("pub(crate)\nfn f() {"), "pub(crate)\nfn f()");
        assert_eq!(
            h("pub const K: [u8; 4] = [0; 4];"),
            "pub const K: [u8; 4] = [0; 4]"
        );
        assert_eq!(h("fn f(a: [u8; 4]) -> u32 {"), "fn f(a: [u8; 4]) -> u32");
        assert_eq!(
            h("pub fn f(\n    a: u8,\n) -> u32 {"),
            "pub fn f(\na: u8,\n) -> u32"
        );
        assert_eq!(h("[package]"), "[package]");
        assert_eq!(h("name = \"x\""), "name = \"x\"");
        // Several lines: a TOML table followed by a key line, an attribute followed by an item line, and a table
        // array header.
        assert_eq!(h("[package]\nname = \"x\""), "[package]");
        assert_eq!(h("[[bin]]\npath = \"src/main.rs\""), "[[bin]]");
        assert_eq!(h("#[inline]\nfn g() {}"), "#[inline]");
        assert_eq!(h("pub\nconst\nX: u8 = 1;"), "pub\nconst\nX: u8 = 1");
        assert_eq!(h("pub fn f() -> [u8;\n4] {"), "pub fn f() -> [u8;\n4]");
        assert_eq!(h("fn f() /* { */ -> u8 { 0 }"), "fn f() /* { */ -> u8");
        assert_eq!(h("extern \"C\"\nfn g();"), "extern \"C\"\nfn g()");
    }

    /// Lines, trimming, trivial lines and N's offsets ([F20 §2.5]).
    #[test]
    fn anchor_text_and_lines() {
        assert_eq!(atext(b"\xEF\xBB\xBFa\r\nb").unwrap(), b"a\nb");
        assert!(lines(b"").is_empty());
        assert_eq!(lines(b"a\nb\n"), vec![&b"a"[..], &b"b"[..]]);
        assert_eq!(lines(b"a\n\n"), vec![&b"a"[..], &b""[..]]);
        let nt = NText::of(b"  fn a() {\n}\n\n  x;\t");
        assert_eq!(nt.n, b"fn a() {\n}\n\nx;");
        assert_eq!(nt.trivial, vec![false, true, true, false]);
        assert_eq!(nt.st(1, 2), b"fn a() {\n}");
        assert_eq!(nt.line_of(nt.start(4)), 4);
        assert_eq!(nt.before(4, 16).len(), 1);
        assert_eq!(nt.after(1, 16).len(), 1);
        let w = window_value(&[1, 2], &[3]);
        assert_eq!(parse_window(&w, 16), Some((vec![1, 2], vec![3])));
        assert_eq!(cutp("aé".as_bytes(), 2), b"a");
        assert_eq!(cuts("éa".as_bytes(), 2), b"a");
    }

    /// The fingerprint and its measures ([F20 §2.6], §2.10).
    #[test]
    fn fingerprints_and_measures() {
        let old = b"alpha line\nbeta line\ngamma line\ndelta line\nepsilon line\nzeta\n";
        let fp = fingerprint(old).unwrap();
        assert_eq!(fp.nlines, 6);
        assert_eq!(fp.sketch.len(), 6);
        assert!(!fp.estimated);
        assert_eq!(fingerprint_bytes(&fp).len(), 20 + 24);
        let (eoin, enio) = estimates(&fp, old);
        assert_eq!(eoin, Ratio::int(1));
        assert_eq!(enio, Ratio::int(1));
        let (oin, nio, sym) = exact_measures(old, b"alpha line\nbeta line\nnew stuff here\n");
        assert_eq!(oin, Ratio::new(19, 55));
        assert_eq!(nio, Ratio::new(19, 33));
        assert_eq!(sym, oin.min(nio));
        assert!(tiny_content(b"a\nb\n"));
        assert!(tiny_content(old), "62 bytes are below 64");
        assert!(!tiny_content(&old.repeat(2)));
        assert!(Ratio::new(1, 3) < Ratio::new(1, 2));
        assert_eq!(Ratio::new(2, 4), Ratio::new(1, 2));
    }

    /// The containments of an inexact pair: exact within `EXACT_LIMIT`, the sketch estimates beyond it ([F20 §2.10.5]).
    #[test]
    fn containments_fall_back_to_the_estimates_beyond_the_exact_limit() {
        let small_old = b"alpha line\nbeta line\ngamma line\ndelta line\nepsilon line\n";
        let small_new = b"alpha line\nbeta line\nnew line here\n";
        let (oin, nio, _) = exact_measures(small_old, small_new);
        assert_eq!(containments(small_old, small_new), (oin, nio));
        let big_old: String = (0..=EXACT_LIMIT).map(|i| format!("line {i}\n")).collect();
        let big_new: String = (0..40_000).map(|i| format!("line {i}\n")).collect();
        let fp = fingerprint(big_old.as_bytes()).unwrap();
        assert_eq!(
            containments(big_old.as_bytes(), big_new.as_bytes()),
            estimates(&fp, big_new.as_bytes()),
            "one side over the limit: the estimates"
        );
        assert_eq!(
            containments(b"\x00binary", big_old.as_bytes()),
            (Ratio::int(0), Ratio::int(0)),
            "no fingerprint for a binary old side"
        );
    }
}
