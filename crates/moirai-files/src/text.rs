//! Content functions of chapter 20 over project bytes: the `is_text` statistics ([F20 §2.1]), EOL normalisation
//! ([F20 §2.2]), the two-pass streaming reader ([F20 §2.4]; [40 §2.5] "Streaming, bounded memory"), anchor text,
//! lines, normalised and trivial lines ([F20 §2.5]), and the byte-string notation of [F20 §1.2] (`chars`, `cutp`,
//! `cuts`, `eqi`).
//!
//! # The two-pass reader
//!
//! `oid`'s header needs the normalised length before the first hashed byte, so [`ContentReader::read`] reads a file
//! twice through one [`ByteSource`] and one fixed [`BUF_SIZE`] buffer ([F20 §2.4], review A1P-05):
//!
//! - **pass 1** gathers the byte statistics of [F20 §2.1], the raw length n1, `N1 = len(norm(b))`, the raw XXH3-64 r1,
//!   the capped line-hash array ([`LineHashes`]) and feeds every line of the anchor text to a [`LineSink`], the hook
//!   through which WP-66 computes the fingerprint ([F20 §2.6]) in the same read;
//! - **pass 2** streams the `oid` header and `norm(b)` into the hash of the root's object format and recomputes n2 and
//!   r2.
//!
//! The read is stable iff the handle's size and last-write time are unchanged, n1 = n2 = that size, r1 = r2 and pass 2
//! emitted exactly N1 bytes; an unstable read is repeated once ([`r14::READ_RETRIES`]), and a second one is
//! [`Unavailable::Unstable`]. Memory is the buffer plus the line-hash array, which keeps 2 bytes and 1 bit per line
//! (about 264 KiB in all at the default `files.max-line-hashes`); no read holds the file ([40 §2.5]).
//! Chunk boundaries never change a result: a CR at the end of a chunk is classified with the next chunk's first byte,
//! and every result equals the whole-buffer definition for every chunking ([F20 §2.4] "Buffers never change a
//! result").
//!
//! The same machinery runs over bytes already in memory ([`analyse`]), for example git blobs read in process.

use std::borrow::Cow;

use xxhash_rust::xxh3::{Xxh3Default, xxh3_64};

use crate::oid::{ObjectFormat, Oid, OidHasher};
use crate::r14;

/// The size of the reader's one fixed buffer: 128 KiB per thread ([40 §2.5], [F20 §2.4]).
pub const BUF_SIZE: usize = 128 * 1024;

const CR: u8 = 0x0D;
const LF: u8 = 0x0A;
const SUB: u8 = 0x1A;
const DEL: u8 = 0x7F;
const BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];

const CLASS_WS: u8 = 1;
const CLASS_TRIVIAL: u8 = 2;

/// Byte classes: `CLASS_WS` for `WS`, `CLASS_TRIVIAL` for `WS` ∪ the trivial-line extras ([F20 §1.2, §2.5]).
static CLASS: [u8; 256] = {
    let mut t = [0u8; 256];
    let mut i = 0;
    while i < r14::WS.len() {
        t[r14::WS[i] as usize] = CLASS_WS | CLASS_TRIVIAL;
        i += 1;
    }
    let mut j = 0;
    while j < r14::TRIVIAL_LINE_EXTRA.len() {
        t[r14::TRIVIAL_LINE_EXTRA[j] as usize] = CLASS_TRIVIAL;
        j += 1;
    }
    t
};

/// Whether `b` is in `WS` = {`09`, `0A`, `0B`, `0C`, `0D`, `20`} ([F20 §1.2]).
#[inline]
#[must_use]
pub fn is_ws(b: u8) -> bool {
    CLASS[usize::from(b)] & CLASS_WS != 0
}

#[inline]
fn is_trivial_byte(b: u8) -> bool {
    CLASS[usize::from(b)] & CLASS_TRIVIAL != 0
}

// --- SWAR helpers ------------------------------------------------------------------------------------------------

const LO: u64 = 0x0101_0101_0101_0101;
const HI: u64 = 0x8080_8080_8080_8080;

#[inline]
fn load(b: &[u8], i: usize) -> u64 {
    let mut w = [0u8; 8];
    w.copy_from_slice(&b[i..i + 8]);
    u64::from_le_bytes(w)
}

/// Whether some byte of `x` is zero.
#[inline]
fn has_zero(x: u64) -> bool {
    x.wrapping_sub(LO) & !x & HI != 0
}

/// Whether some byte of `x` is below `n` (exact for `n` ≤ 128).
#[inline]
fn has_less(x: u64, n: u8) -> bool {
    x.wrapping_sub(LO.wrapping_mul(u64::from(n))) & !x & HI != 0
}

/// The index of the first `needle` in `hay`.
#[inline]
fn find_byte(hay: &[u8], needle: u8) -> Option<usize> {
    let pat = LO.wrapping_mul(u64::from(needle));
    let mut i = 0;
    while i + 8 <= hay.len() {
        if has_zero(load(hay, i) ^ pat) {
            break;
        }
        i += 8;
    }
    hay[i..].iter().position(|&b| b == needle).map(|k| i + k)
}

// --- §1.2: notation ------------------------------------------------------------------------------------------

#[inline]
fn is_cont(b: u8) -> bool {
    b & 0xC0 == 0x80
}

/// `chars(x)`: the number of bytes of `x` outside `80`–`BF` ([F20 §1.2]); for valid UTF-8, the number of scalar
/// values.
#[must_use]
pub fn chars(x: &[u8]) -> usize {
    x.iter().filter(|&&b| !is_cont(b)).count()
}

/// `cutp(x, n)`: the longest prefix of `x` of at most `n` bytes whose next byte, if any, is not in `80`–`BF`
/// ([F20 §1.2]). When no prefix qualifies (`x` longer than `n` and every candidate cut lands before a continuation
/// byte, which only invalid UTF-8 allows at offset 0), the result is empty.
#[must_use]
pub fn cutp(x: &[u8], n: usize) -> &[u8] {
    if x.len() <= n {
        return x;
    }
    let mut k = n;
    while k > 0 && is_cont(x[k]) {
        k -= 1;
    }
    &x[..k]
}

/// `cuts(x, n)`: the longest suffix of `x` of at most `n` bytes whose first byte, if any, is not in `80`–`BF`
/// ([F20 §1.2]).
#[must_use]
pub fn cuts(x: &[u8], n: usize) -> &[u8] {
    let mut s = x.len() - x.len().min(n);
    while s < x.len() && is_cont(x[s]) {
        s += 1;
    }
    &x[s..]
}

/// `eqi(a, b)`: equal length and equal bytes after mapping `41`–`5A` to `61`–`7A` ([F20 §1.2]).
#[must_use]
pub fn eqi(a: &[u8], b: &[u8]) -> bool {
    a.eq_ignore_ascii_case(b)
}

// --- §2.1: byte statistics and `is_text` ---------------------------------------------------------------------

/// The byte statistics of a whole content ([F20 §2.1]; git's `gather_stats`), after the final-`1A` adjustment.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TextStats {
    /// The raw length n.
    pub len: u64,
    /// CR LF pairs; pairs never overlap.
    pub crlf: u64,
    /// CR bytes not followed by LF.
    pub lonecr: u64,
    /// LF bytes not preceded by CR.
    pub lonelf: u64,
    /// NUL bytes (also counted as non-printable).
    pub nul: u64,
    /// Printable bytes: `20`–`7E`, `80`–`FF`, and BS, HT, FF, ESC.
    pub printable: u64,
    /// Non-printable bytes: DEL and the other controls, a final `1A` not counted.
    pub nonprintable: u64,
    /// The last byte, if the content is not empty.
    pub last: Option<u8>,
}

impl TextStats {
    /// `is_text` ([F20 §2.1]; git's `convert_is_binary` negated): no lone CR, no NUL, and
    /// `(printable >> 7) ≥ nonprintable`. The empty content is text.
    #[must_use]
    pub const fn is_text(&self) -> bool {
        self.lonecr == 0 && self.nul == 0 && (self.printable >> 7) >= self.nonprintable
    }

    /// `len(norm(b))` ([F20 §2.2]): `n − crlf` for text, n otherwise.
    #[must_use]
    pub const fn norm_len(&self) -> u64 {
        if self.is_text() {
            self.len - self.crlf
        } else {
            self.len
        }
    }

    /// `nlines`: the number of lines of `lines(norm(b))` ([F20 §2.6.3]): one per LF, plus one for a last line
    /// without a terminator.
    ///
    /// It equals the line count of `atext(b)` except for the content that is exactly `EF BB BF`: one line of
    /// `norm(b)`, none of the anchor text ([F20 §2.5] claims no difference; reported to WP-81a).
    #[must_use]
    pub const fn nlines(&self) -> u64 {
        let tail = match self.last {
            Some(b) if b != LF => 1,
            _ => 0,
        };
        self.crlf + self.lonelf + tail
    }
}

/// Streaming computation of [`TextStats`] over consecutive chunks of one content ([F20 §2.1, §2.4]).
#[derive(Clone, Debug, Default)]
pub struct StatsScanner {
    len: u64,
    crlf: u64,
    lonecr: u64,
    lonelf: u64,
    nul: u64,
    /// Non-printable bytes before the final-`1A` adjustment.
    nonprint: u64,
    last: Option<u8>,
    /// The previous chunk ended with a CR whose class depends on the next byte.
    pending_cr: bool,
}

impl StatsScanner {
    /// A scanner at the start of a content.
    #[must_use]
    pub fn new() -> StatsScanner {
        StatsScanner::default()
    }

    /// Counts the next chunk.
    pub fn feed(&mut self, chunk: &[u8]) {
        let n = chunk.len();
        let Some(&last) = chunk.last() else { return };
        self.len += n as u64;
        self.last = Some(last);
        let mut i = 0;
        if self.pending_cr {
            self.pending_cr = false;
            if chunk[0] == LF {
                self.crlf += 1;
                i = 1;
            } else {
                self.lonecr += 1;
            }
        }
        let del = LO.wrapping_mul(u64::from(DEL));
        while i < n {
            // Whole words of printable bytes need no counting: `printable` is derived in `finish`.
            while i + 8 <= n {
                let w = load(chunk, i);
                if has_less(w, 0x20) || has_zero(w ^ del) {
                    break;
                }
                i += 8;
            }
            let stop = (i + 8).min(n);
            while i < stop {
                let c = chunk[i];
                i += 1;
                if c >= 0x20 {
                    if c == DEL {
                        self.nonprint += 1;
                    }
                    continue;
                }
                match c {
                    CR => {
                        if i < n {
                            if chunk[i] == LF {
                                self.crlf += 1;
                                i += 1;
                            } else {
                                self.lonecr += 1;
                            }
                        } else {
                            self.pending_cr = true;
                        }
                    }
                    LF => self.lonelf += 1,
                    0x08 | 0x09 | 0x0C | 0x1B => {}
                    0x00 => {
                        self.nul += 1;
                        self.nonprint += 1;
                    }
                    _ => self.nonprint += 1,
                }
            }
        }
    }

    /// Whether the content can still be text: no NUL and no lone CR so far.
    #[must_use]
    pub const fn maybe_text(&self) -> bool {
        self.nul == 0 && self.lonecr == 0
    }

    /// The statistics of everything fed, as a whole content: a pending final CR is a lone CR, and a final `1A` is not
    /// counted as non-printable.
    #[must_use]
    pub fn finish(&self) -> TextStats {
        let lonecr = self.lonecr + u64::from(self.pending_cr);
        // Every byte is exactly one of: half of a CRLF pair, a lone CR, a lone LF, non-printable, printable.
        let printable = self.len - 2 * self.crlf - lonecr - self.lonelf - self.nonprint;
        let nonprintable = self.nonprint - u64::from(self.last == Some(SUB));
        TextStats {
            len: self.len,
            crlf: self.crlf,
            lonecr,
            lonelf: self.lonelf,
            nul: self.nul,
            printable,
            nonprintable,
            last: self.last,
        }
    }
}

/// The statistics of the whole content `b` ([F20 §2.1]).
#[must_use]
pub fn stats(b: &[u8]) -> TextStats {
    let mut s = StatsScanner::new();
    s.feed(b);
    s.finish()
}

/// `is_text(b)` ([F20 §2.1]).
#[must_use]
pub fn is_text(b: &[u8]) -> bool {
    stats(b).is_text()
}

// --- §2.2: EOL normalisation -------------------------------------------------------------------------------------

/// `norm(b)` ([F20 §2.2]): for text content, every CR LF pair replaced by LF; other content unchanged. Nothing else
/// changes (no BOM removal, no trailing newline). Borrows `b` when nothing changes.
#[must_use]
pub fn norm(b: &[u8]) -> Cow<'_, [u8]> {
    let s = stats(b);
    if s.is_text() {
        drop_pair_crs(b, s.crlf)
    } else {
        Cow::Borrowed(b)
    }
}

/// `b` with every CR removed, for a part of a text content that holds `crlf` CR LF pairs: text has no lone CR
/// ([F20 §2.1]), so every CR is the first half of a pair ([F20 §2.2]). Borrows `b` when it holds no pair.
fn drop_pair_crs(b: &[u8], crlf: u64) -> Cow<'_, [u8]> {
    if crlf == 0 {
        return Cow::Borrowed(b);
    }
    // `crlf` ≤ `b.len() / 2`, so it fits `usize`.
    let mut out = Vec::with_capacity(b.len() - usize::try_from(crlf).unwrap_or(0));
    let mut i = 0;
    while let Some(k) = find_byte(&b[i..], CR) {
        out.extend_from_slice(&b[i..i + k]);
        i += k + 1;
    }
    out.extend_from_slice(&b[i..]);
    Cow::Owned(out)
}

// --- §2.5: anchor text, lines and trivial lines -------------------------------------------------------------

/// `atext(b)` ([F20 §2.5]): defined only for text content — `norm(b)` with one leading `EF BB BF` removed.
///
/// One statistics scan decides `is_text` and counts the pairs; the BOM is skipped before normalising, which gives the
/// same bytes because a BOM holds no CR or LF, so no pair straddles it and nothing is shifted after the copy.
#[must_use]
pub fn atext(b: &[u8]) -> Option<Cow<'_, [u8]>> {
    let s = stats(b);
    if !s.is_text() {
        return None;
    }
    let body = b.strip_prefix(&BOM[..]).unwrap_or(b);
    Some(drop_pair_crs(body, s.crlf))
}

/// `lines(t)` ([F20 §2.5]): `t` split at every `0A`, the last piece dropped if empty. Line numbers start at 1.
#[must_use]
pub fn lines(t: &[u8]) -> Lines<'_> {
    Lines { rest: t }
}

/// The iterator of [`lines`].
#[derive(Clone, Debug)]
pub struct Lines<'a> {
    rest: &'a [u8],
}

impl<'a> Iterator for Lines<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<&'a [u8]> {
        if self.rest.is_empty() {
            return None;
        }
        match find_byte(self.rest, LF) {
            Some(k) => {
                let line = &self.rest[..k];
                self.rest = &self.rest[k + 1..];
                Some(line)
            }
            None => {
                let line = self.rest;
                self.rest = &[];
                Some(line)
            }
        }
    }
}

/// `nl(l)` ([F20 §2.5]): `l` without its leading and trailing `WS` bytes; nothing inside changes.
#[must_use]
pub fn nl(l: &[u8]) -> &[u8] {
    let Some(a) = l.iter().position(|&b| !is_ws(b)) else {
        return &l[..0];
    };
    let z = l.iter().rposition(|&b| !is_ws(b)).unwrap_or(a);
    &l[a..=z]
}

/// Whether line `l` is trivial ([F20 §2.5]): every byte of `nl(l)` is in `WS` ∪ `{ } ( ) [ ] ; ,`. A blank line is
/// trivial.
#[must_use]
pub fn is_trivial_line(l: &[u8]) -> bool {
    // `nl` removes only `WS` bytes, which are trivial themselves.
    l.iter().all(|&b| is_trivial_byte(b))
}

/// `low(v, 16)` ([F20 §1.2]): `v mod 2^16`.
#[inline]
const fn low16(v: u64) -> u16 {
    (v & 0xFFFF) as u16
}

/// The window hash `wh(l) = low(XXH3-64(nl(l)), 16)` of a non-trivial line ([F20 §2.7.1]); `None` for a trivial line
/// ([F20 §2.5]), for which the window hash is not defined.
#[must_use]
pub fn window_hash(l: &[u8]) -> Option<u16> {
    if is_trivial_line(l) {
        None
    } else {
        Some(low16(xxh3_64(nl(l))))
    }
}

/// The normalised anchor text `N(t)` of an anchor text `t` with its line offsets ([F20 §2.5]):
/// `N(t) = nl(l1) ‖ 0A ‖ … ‖ 0A ‖ nl(lm)`, with no trailing `0A`.
///
/// It holds the whole of N in memory: at most `len(t)` bytes of text plus 4 bytes per line, allocated once at their
/// exact sizes. It is for anchor texts already in memory, such as the captured content of [F20 §6.1]; a resolve that
/// streams a file by chunks ([40 §4.5] "by chunks", [40 §2.5] "no project-file read ever holds a whole file") must
/// not build it over the whole file.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct NormalisedText {
    text: Vec<u8>,
    /// `start(i)` of each line, 0-based index i − 1.
    starts: Vec<u32>,
}

/// An anchor text whose normalised form would not fit 32-bit offsets.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TextTooLong;

impl core::fmt::Display for TextTooLong {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("anchor text of 4 GiB or more")
    }
}

impl std::error::Error for TextTooLong {}

impl NormalisedText {
    /// `N(t)` of the anchor text `t`.
    ///
    /// # Errors
    /// [`TextTooLong`] when `t` is 4 GiB or longer.
    pub fn new(t: &[u8]) -> Result<NormalisedText, TextTooLong> {
        if u32::try_from(t.len()).is_err() {
            return Err(TextTooLong);
        }
        // `lines(t)`: one per `0A`, plus one for a last piece without a terminator.
        let m = t.iter().filter(|&&b| b == LF).count()
            + usize::from(t.last().is_some_and(|&b| b != LF));
        let mut text = Vec::with_capacity(t.len());
        let mut starts = Vec::with_capacity(m);
        for (i, l) in lines(t).enumerate() {
            if i > 0 {
                text.push(LF);
            }
            // `text` never outgrows `t`, which fits `u32`.
            starts.push(u32::try_from(text.len()).map_err(|_| TextTooLong)?);
            text.extend_from_slice(nl(l));
        }
        Ok(NormalisedText { text, starts })
    }

    /// The bytes of `N(t)`.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.text
    }

    /// The number of lines m.
    #[must_use]
    pub fn line_count(&self) -> usize {
        self.starts.len()
    }

    /// `start(i)`: the offset in N of line i's first byte, for 1 ≤ i ≤ m.
    #[must_use]
    pub fn start(&self, i: usize) -> Option<usize> {
        let k = i.checked_sub(1)?;
        self.starts.get(k).map(|&s| s as usize)
    }

    /// `end(i)`: the offset just after line i's last byte, for 1 ≤ i ≤ m.
    #[must_use]
    pub fn end(&self, i: usize) -> Option<usize> {
        let k = i.checked_sub(1)?;
        if k + 1 < self.starts.len() {
            Some(self.starts[k + 1] as usize - 1)
        } else if k < self.starts.len() {
            Some(self.text.len())
        } else {
            None
        }
    }

    /// `ST(s, e) = N[start(s) .. end(e))`, the span text of lines s ≤ e.
    #[must_use]
    pub fn span(&self, s: usize, e: usize) -> Option<&[u8]> {
        if s > e {
            return None;
        }
        Some(&self.text[self.start(s)?..self.end(e)?])
    }

    /// The line that holds byte offset `off` of N: the greatest i with `start(i) ≤ off` (an offset on a separating
    /// `0A` belongs to the line before it); `None` for an offset past N or an empty text.
    #[must_use]
    pub fn line_at(&self, off: usize) -> Option<usize> {
        if off > self.text.len() || self.starts.is_empty() {
            return None;
        }
        Some(self.starts.partition_point(|&s| s as usize <= off))
    }
}

// --- the pass-1 hook and the line-hash array ------------------------------------------------------------------

/// The pass-1 hook ([PLAN WP-62], for WP-66's fingerprint): receives the lines of the anchor text `atext(b)`
/// ([F20 §2.5]) as pass 1 streams them — every CR of a CR LF pair and a leading BOM removed, no `0A`.
///
/// A line may arrive in several pieces; `end_line` closes it. The last line is closed only when it has at least one
/// byte (`lines` drops an empty last piece). Lines stop as soon as the content is known to be binary (a NUL or a lone
/// CR), and the final ratio test may still find it binary: what the sink computed is meaningful only when the read
/// reports text. [`LineSink::begin`] starts every read attempt, the retry of [F20 §2.4] included, so the sink drops
/// what it received before.
pub trait LineSink {
    /// Whether the reader must split lines for this sink; `false` lets it skip the line scan.
    const ACTIVE: bool = true;
    /// A read attempt starts at offset 0: forget every line received so far.
    fn begin(&mut self);
    /// The next bytes of the current line (never empty).
    fn piece(&mut self, bytes: &[u8]);
    /// The current line ended.
    fn end_line(&mut self);
}

/// The sink that wants no lines.
impl LineSink for () {
    const ACTIVE: bool = false;
    fn begin(&mut self) {}
    fn piece(&mut self, _: &[u8]) {}
    fn end_line(&mut self) {}
}

/// One entry of the line-hash array ([F20 §2.4]): whether the line is trivial ([F20 §2.5]) and, for a non-trivial
/// line, its window hash `wh(l) = low(XXH3-64(nl(l)), 16)` ([F20 §2.7.1]).
///
/// That is everything the array's readers use: the window steps and `lines` anchors compare window hashes of
/// non-trivial lines only ([F20 §2.7.2], §6.2 step 6.2, §6.3, §6.5), and `span_hash` is hashed again from `ST(…)`
/// ([F20 §2.8]). The rest of `XXH3-64(nl(l))` is therefore not kept (2 bytes and 1 bit per line).
///
/// [F20 §2.4] as written records the whole `XXH3-64(nl(l))` per line, and [40 §2.5] and [AR §13] size the array at
/// 8 bytes per line (512 KiB at 65,536 lines), which with the 128 KiB buffer would exceed PLAN WP-62's 0.5 MB bound.
/// This narrowing to `wh(l)` and the trivial bit is a spec finding of WP-62 for WP-81a: §2.4 to read "records `wh(l)`
/// (§2.7.1) and whether l is trivial", with the sizing in [40 §2.5] and [AR §13] to match.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct LineHash {
    /// `wh(l)`; 0 for a trivial line, and never read then.
    wh: u16,
    trivial: bool,
}

impl LineHash {
    /// The entry of a trivial line.
    pub const TRIVIAL: LineHash = LineHash {
        wh: 0,
        trivial: true,
    };

    /// The entry of a non-trivial line with window hash `wh`.
    #[must_use]
    pub const fn non_trivial(wh: u16) -> LineHash {
        LineHash { wh, trivial: false }
    }

    /// The entry of line `l` (a line of an anchor text, without its `0A`).
    #[must_use]
    pub fn of(l: &[u8]) -> LineHash {
        window_hash(l).map_or(LineHash::TRIVIAL, LineHash::non_trivial)
    }

    /// Whether the line is trivial ([F20 §2.5]); the window steps skip trivial lines.
    #[must_use]
    pub const fn is_trivial(self) -> bool {
        self.trivial
    }

    /// `wh(l)` ([F20 §2.7.1]); `None` for a trivial line.
    #[must_use]
    pub const fn window_hash(self) -> Option<u16> {
        if self.trivial { None } else { Some(self.wh) }
    }
}

/// Lines per block of the line-hash array.
const BLOCK: usize = 2048;

/// One block of the line-hash array: 2,048 window hashes and their trivial bits (4,352 bytes).
#[derive(Clone)]
struct Block {
    wh: [u16; BLOCK],
    trivial: [u64; BLOCK / 64],
}

/// The capped line-hash array of pass 1 ([F20 §2.4]): one [`LineHash`] per line of the anchor text, for at most
/// `files.max-line-hashes` lines ([CFG §10.4]), counting every line past the cap.
///
/// Stored in fixed blocks of 2,048 entries, so growth never copies an entry: 2 bytes and 1 bit per recorded line plus
/// one pointer per block, 136 KiB at the default key value of 65,536 lines. With the reader's 128 KiB buffer a read
/// holds about 264 KiB whatever the file size (PLAN WP-62: at most 0.5 MB extra RSS on a 16 MiB file).
#[derive(Clone, Default)]
pub struct LineHashes {
    blocks: Vec<Box<Block>>,
    len: usize,
    total: u64,
    cap: usize,
}

impl core::fmt::Debug for LineHashes {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("LineHashes")
            .field("len", &self.len)
            .field("total", &self.total)
            .field("cap", &self.cap)
            .finish_non_exhaustive()
    }
}

impl LineHashes {
    fn with_cap(cap: u32) -> LineHashes {
        LineHashes {
            cap: cap as usize,
            ..LineHashes::default()
        }
    }

    #[inline]
    fn wants(&self) -> bool {
        self.len < self.cap
    }

    /// Records the next line, or only counts it past the cap.
    fn push(&mut self, e: LineHash) {
        self.total += 1;
        if self.len >= self.cap {
            return;
        }
        let (b, o) = (self.len / BLOCK, self.len % BLOCK);
        if b == self.blocks.len() {
            self.blocks.push(Box::new(Block {
                wh: [0; BLOCK],
                trivial: [0; BLOCK / 64],
            }));
        }
        let block = &mut self.blocks[b];
        block.wh[o] = e.wh;
        block.trivial[o / 64] |= u64::from(e.trivial) << (o % 64);
        self.len += 1;
    }

    /// Counts a line past the cap.
    fn skip(&mut self) {
        debug_assert!(!self.wants());
        self.total += 1;
    }

    /// The number of lines recorded.
    #[must_use]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether no line is recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The number of lines of the anchor text, recorded or not.
    #[must_use]
    pub fn total_lines(&self) -> u64 {
        self.total
    }

    /// Whether every line is recorded. When not, the window steps and `lines` anchors are `Unavailable(size)`
    /// ([F20 §2.4], [AR §5e.3]).
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.total == self.len as u64
    }

    /// The entry of line `index + 1` (0-based index).
    #[must_use]
    pub fn get(&self, index: usize) -> Option<LineHash> {
        if index >= self.len {
            return None;
        }
        let (block, o) = (&self.blocks[index / BLOCK], index % BLOCK);
        Some(LineHash {
            wh: block.wh[o],
            trivial: (block.trivial[o / 64] >> (o % 64)) & 1 == 1,
        })
    }

    /// The recorded entries in line order.
    pub fn iter(&self) -> impl Iterator<Item = LineHash> + '_ {
        (0..self.len).filter_map(|i| self.get(i))
    }

    /// The heap bytes the array holds: its blocks and the block table.
    #[must_use]
    pub fn heap_bytes(&self) -> usize {
        self.blocks.capacity() * size_of::<Box<Block>>() + self.blocks.len() * size_of::<Block>()
    }
}

// --- pass 1: the line scan -----------------------------------------------------------------------------------

/// The hash state of a line that crosses a chunk boundary: `nl` over pieces, with trailing `WS` held back in a
/// speculative copy of the state so that it is hashed only when a later non-`WS` byte follows.
#[derive(Clone)]
struct Carry {
    active: bool,
    started: bool,
    nontrivial: bool,
    committed: Xxh3Default,
    spec: Option<Xxh3Default>,
}

impl Carry {
    fn new() -> Carry {
        Carry {
            active: false,
            started: false,
            nontrivial: false,
            committed: Xxh3Default::new(),
            spec: None,
        }
    }

    fn begin(&mut self) {
        self.active = true;
        self.started = false;
        self.nontrivial = false;
        self.committed.reset();
        self.spec = None;
    }

    fn feed(&mut self, p: &[u8]) {
        if p.is_empty() {
            return;
        }
        if !self.nontrivial && !p.iter().all(|&b| is_trivial_byte(b)) {
            self.nontrivial = true;
        }
        let mut p = p;
        if !self.started {
            let Some(a) = p.iter().position(|&b| !is_ws(b)) else {
                return;
            };
            self.started = true;
            p = &p[a..];
        }
        match p.iter().rposition(|&b| !is_ws(b)) {
            None => {
                let committed = &self.committed;
                self.spec.get_or_insert_with(|| committed.clone()).update(p);
            }
            Some(z) => {
                if let Some(s) = self.spec.take() {
                    self.committed = s;
                }
                self.committed.update(&p[..=z]);
                let tail = &p[z + 1..];
                if !tail.is_empty() {
                    let mut s = self.committed.clone();
                    s.update(tail);
                    self.spec = Some(s);
                }
            }
        }
    }

    fn finish(&mut self) -> LineHash {
        self.active = false;
        self.spec = None;
        // A non-trivial line has a byte outside `WS`, so `started` is set and `committed` holds `nl(l)`.
        if self.nontrivial {
            LineHash::non_trivial(low16(self.committed.digest()))
        } else {
            LineHash::TRIVIAL
        }
    }
}

/// Pass 1's line splitter over the raw content: BOM removal, CR LF → LF, lines, the line-hash array and the sink.
/// It sees only chunks that the statistics still allow to be text, so every CR it sees is followed by LF, in the same
/// chunk or as the next chunk's first byte.
struct LineScan {
    /// Bytes of the BOM matched so far while undecided (0–2); 3 once decided.
    bom: u8,
    /// The previous chunk ended with a CR, held back until the next byte shows it is half of a pair.
    pending_cr: bool,
    /// The current line has at least one byte.
    in_line: bool,
    carry: Carry,
}

impl LineScan {
    fn new() -> LineScan {
        LineScan {
            bom: 0,
            pending_cr: false,
            in_line: false,
            carry: Carry::new(),
        }
    }

    fn feed<H: LineSink>(&mut self, chunk: &[u8], hashes: &mut Option<LineHashes>, sink: &mut H) {
        let mut c = chunk;
        while self.bom < 3 {
            let Some((&b, rest)) = c.split_first() else {
                return;
            };
            if b == BOM[usize::from(self.bom)] {
                self.bom += 1;
                c = rest;
            } else {
                let k = usize::from(self.bom);
                self.bom = 3;
                self.bytes(&BOM[..k], hashes, sink);
            }
        }
        self.bytes(c, hashes, sink);
    }

    fn bytes<H: LineSink>(&mut self, c: &[u8], hashes: &mut Option<LineHashes>, sink: &mut H) {
        let Some(&first) = c.first() else { return };
        if self.pending_cr {
            self.pending_cr = false;
            if first != LF {
                // A lone CR: the statistics make the content binary; keep the byte so nothing is lost.
                self.segment(b"\r", false, hashes, sink);
            }
        }
        let mut start = 0;
        loop {
            match find_byte(&c[start..], LF) {
                Some(k) => {
                    let seg = &c[start..start + k];
                    let seg = seg.strip_suffix(&[CR]).unwrap_or(seg);
                    self.segment(seg, true, hashes, sink);
                    start += k + 1;
                }
                None => {
                    let seg = &c[start..];
                    let seg = match seg.strip_suffix(&[CR]) {
                        Some(s) => {
                            self.pending_cr = true;
                            s
                        }
                        None => seg,
                    };
                    self.segment(seg, false, hashes, sink);
                    return;
                }
            }
        }
    }

    /// One piece of the current line; `ends` when an `0A` follows it.
    fn segment<H: LineSink>(
        &mut self,
        seg: &[u8],
        ends: bool,
        hashes: &mut Option<LineHashes>,
        sink: &mut H,
    ) {
        if !seg.is_empty() {
            self.in_line = true;
            if H::ACTIVE {
                sink.piece(seg);
            }
        }
        if let Some(h) = hashes {
            if self.carry.active {
                self.carry.feed(seg);
                if ends {
                    h.push(self.carry.finish());
                }
            } else if ends {
                if h.wants() {
                    h.push(LineHash::of(seg));
                } else {
                    h.skip();
                }
            } else if !seg.is_empty() && h.wants() {
                self.carry.begin();
                self.carry.feed(seg);
            }
        }
        if ends {
            self.in_line = false;
            if H::ACTIVE {
                sink.end_line();
            }
        }
    }

    fn finish<H: LineSink>(&mut self, hashes: &mut Option<LineHashes>, sink: &mut H) {
        if self.bom < 3 {
            let k = usize::from(self.bom);
            self.bom = 3;
            self.bytes(&BOM[..k], hashes, sink);
        }
        if self.pending_cr {
            self.pending_cr = false;
            self.segment(b"\r", false, hashes, sink);
        }
        if self.in_line {
            self.in_line = false;
            if let Some(h) = hashes {
                if self.carry.active {
                    h.push(self.carry.finish());
                } else {
                    // Only a line over the cap is open without a carry.
                    h.skip();
                }
            }
            if H::ACTIVE {
                sink.end_line();
            }
        }
    }
}

/// Pass 1 over one content: statistics, raw XXH3-64, and the line scan.
struct Pass1 {
    stats: StatsScanner,
    raw: Xxh3Default,
    scan: Option<LineScan>,
    hashes: Option<LineHashes>,
}

impl Pass1 {
    fn new(max_line_hashes: Option<u32>, sink_active: bool) -> Pass1 {
        let hashes = max_line_hashes.map(LineHashes::with_cap);
        let scan = (hashes.is_some() || sink_active).then(LineScan::new);
        Pass1 {
            stats: StatsScanner::new(),
            raw: Xxh3Default::new(),
            scan,
            hashes,
        }
    }

    fn feed<H: LineSink>(&mut self, chunk: &[u8], sink: &mut H) {
        self.stats.feed(chunk);
        self.raw.update(chunk);
        if let Some(scan) = &mut self.scan {
            if self.stats.maybe_text() {
                scan.feed(chunk, &mut self.hashes, sink);
            } else {
                // Binary for certain: stop the line scan and release the array.
                self.scan = None;
                self.hashes = None;
            }
        }
    }

    fn finish<H: LineSink>(mut self, sink: &mut H) -> (TextStats, u64, Option<LineHashes>) {
        let stats = self.stats.finish();
        let mut hashes = self.hashes.take();
        if stats.is_text() {
            if let Some(scan) = &mut self.scan {
                scan.finish(&mut hashes, sink);
            }
        } else {
            hashes = None;
        }
        (stats, self.raw.digest(), hashes)
    }
}

// --- pass 2: the normalised stream ---------------------------------------------------------------------------

/// The raw length n2 and XXH3-64 r2 that pass 2 recomputes for the stability check ([F20 §2.4]).
struct RawCheck {
    len: u64,
    raw: Xxh3Default,
}

/// Pass 2 over one content: `norm(b)` into the `oid` hash and, for a read of a file, the raw length and XXH3-64 again.
struct Pass2 {
    hasher: OidHasher,
    text: bool,
    pending_cr: bool,
    /// `None` for bytes in memory, which cannot change between the passes.
    check: Option<RawCheck>,
}

/// What pass 2 yields: `oid`, the normalised bytes it emitted, and (n2, r2) when it recomputed them.
struct Pass2Result {
    oid: Oid,
    emitted: u64,
    raw: Option<(u64, u64)>,
}

impl Pass2 {
    fn new(format: ObjectFormat, stats: &TextStats, check: bool) -> Pass2 {
        Pass2 {
            hasher: OidHasher::new(format, stats.norm_len()),
            text: stats.is_text(),
            pending_cr: false,
            check: check.then(|| RawCheck {
                len: 0,
                raw: Xxh3Default::new(),
            }),
        }
    }

    fn feed(&mut self, chunk: &[u8]) {
        let Some(&first) = chunk.first() else { return };
        if let Some(c) = &mut self.check {
            c.raw.update(chunk);
            c.len += chunk.len() as u64;
        }
        if !self.text {
            self.hasher.update(chunk);
            return;
        }
        if self.pending_cr {
            self.pending_cr = false;
            if first != LF {
                // A lone CR in content pass 1 found to be text: it changed, and the counts will show it.
                self.hasher.update(b"\r");
            }
        }
        let mut i = 0;
        while let Some(k) = find_byte(&chunk[i..], CR) {
            let j = i + k;
            self.hasher.update(&chunk[i..j]);
            if j + 1 == chunk.len() {
                self.pending_cr = true;
                return;
            }
            if chunk[j + 1] != LF {
                self.hasher.update(b"\r");
            }
            i = j + 1;
        }
        self.hasher.update(&chunk[i..]);
    }

    fn finish(mut self) -> Pass2Result {
        if self.pending_cr {
            self.hasher.update(b"\r");
        }
        Pass2Result {
            emitted: self.hasher.fed(),
            raw: self.check.map(|c| (c.len, c.raw.digest())),
            oid: self.hasher.finish(),
        }
    }
}

// --- results -----------------------------------------------------------------------------------------------------

/// What one read of a content yields ([F20 §2.1–§2.4]).
#[derive(Clone, Debug)]
pub struct Content {
    /// The byte statistics of the whole content.
    pub stats: TextStats,
    /// XXH3-64 of the raw bytes (r1 = r2 of a stable read).
    pub raw_xxh3: u64,
    /// `oid_A(R)(b)` ([F20 §2.3]).
    pub oid: Oid,
    /// The line-hash array, when requested and the content is text.
    pub line_hashes: Option<LineHashes>,
}

impl Content {
    /// `is_text(b)`.
    #[must_use]
    pub const fn is_text(&self) -> bool {
        self.stats.is_text()
    }

    /// The raw length n.
    #[must_use]
    pub const fn raw_len(&self) -> u64 {
        self.stats.len
    }

    /// `len(norm(b))` (`nbytes` of [F20 §2.6.3] for text).
    #[must_use]
    pub const fn norm_len(&self) -> u64 {
        self.stats.norm_len()
    }

    /// `nlines` of [F20 §2.6.3]: the lines of `norm(b)`.
    #[must_use]
    pub const fn nlines(&self) -> u64 {
        self.stats.nlines()
    }

    /// The heap bytes the result holds (the line-hash array).
    #[must_use]
    pub fn heap_bytes(&self) -> usize {
        self.line_hashes.as_ref().map_or(0, LineHashes::heap_bytes)
    }
}

/// Both passes over content already in memory: the same results as [`ContentReader::read`] over a source holding
/// `bytes`, without the stability checks, which bytes in memory do not need (pass 2 recomputes no raw length or hash).
pub fn analyse<H: LineSink>(
    bytes: &[u8],
    format: ObjectFormat,
    max_line_hashes: Option<u32>,
    sink: &mut H,
) -> Content {
    sink.begin();
    let mut p1 = Pass1::new(max_line_hashes, H::ACTIVE);
    p1.feed(bytes, sink);
    let (stats, raw_xxh3, line_hashes) = p1.finish(sink);
    let mut p2 = Pass2::new(format, &stats, false);
    p2.feed(bytes);
    Content {
        stats,
        raw_xxh3,
        oid: p2.finish().oid,
        line_hashes,
    }
}

/// `oid_A(b)` of content in memory ([F20 §2.3]): `norm` applied, then the blob header. It needs only the statistics
/// (for `is_text` and `len(norm(b))`) and the normalised stream: no raw hash, no line scan.
#[must_use]
pub fn oid_of(format: ObjectFormat, bytes: &[u8]) -> Oid {
    let mut p2 = Pass2::new(format, &stats(bytes), false);
    p2.feed(bytes);
    p2.finish().oid
}

// --- the reader --------------------------------------------------------------------------------------------------

/// Why a step's input cannot be read ([F20 §1.5]). An unavailable source contributes nothing: it never yields a
/// candidate, never proves absence and never changes a rule.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Unavailable {
    /// The command's budget ended.
    Budget,
    /// Larger than `files.max-read-bytes`, or more lines than `files.max-line-hashes` for a window step.
    Size,
    /// A cloud-only entry ([F20 §4.5]).
    CloudOnly,
    /// A denial, a sharing or lock violation, or another read error ([F20 §2.4], §4.8).
    Unreadable,
    /// Two reads whose passes disagreed ([F20 §2.4]).
    Unstable,
    /// A git step could not run.
    Git,
    /// No tree.
    NoTree,
    /// The commit is not in this repository.
    CommitNotInRepository,
}

impl Unavailable {
    /// The reason's name as [F20 §1.5] writes it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Unavailable::Budget => "budget",
            Unavailable::Size => "size",
            Unavailable::CloudOnly => "cloud-only",
            Unavailable::Unreadable => "unreadable",
            Unavailable::Unstable => "unstable",
            Unavailable::Git => "git",
            Unavailable::NoTree => "no tree",
            Unavailable::CommitNotInRepository => "commit not in this repository",
        }
    }
}

/// The size and last-write time of an open file, read through its handle ([OS/project §5.5] `ReadSnapshot`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Snapshot<T> {
    /// The size in bytes.
    pub size: u64,
    /// The last-write time, compared for equality only.
    pub mtime: T,
}

/// One open file for the two-pass reader: the byte source [OS/project §5.5]'s `ProjectRead` provides. The reader
/// opens nothing itself ([F20 §2], review A1P-10); its caller opens the file (with full sharing) and closes it after
/// the read.
pub trait ByteSource {
    /// A read error.
    type Error;
    /// The last-write time type.
    type Stamp: PartialEq;
    /// Reads at the current offset into `buf`; 0 means end of file.
    ///
    /// # Errors
    /// Any read error; the read is then [`Unavailable::Unreadable`].
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error>;
    /// Returns to offset 0 on the same handle.
    ///
    /// # Errors
    /// Any error; the read is then [`Unavailable::Unreadable`].
    fn rewind(&mut self) -> Result<(), Self::Error>;
    /// The size and last-write time, read through the handle.
    ///
    /// # Errors
    /// Any error; the read is then [`Unavailable::Unreadable`].
    fn snapshot(&self) -> Result<Snapshot<Self::Stamp>, Self::Error>;
}

/// The parameters of one read.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReadOptions {
    /// A(R), the object format of the file's root ([F20 §2.3]).
    pub format: ObjectFormat,
    /// `files.max-read-bytes` ([CFG §10.4]): a larger file is [`Unavailable::Size`] and is not read.
    pub max_read_bytes: u64,
    /// `files.max-line-hashes` ([CFG §10.4]) when the caller needs the line-hash array, `None` when it does not.
    pub max_line_hashes: Option<u32>,
}

/// Why a read yields no content.
#[derive(Debug)]
pub enum ReadError<E> {
    /// The handle reports a size above `files.max-read-bytes`.
    Size {
        /// The size the handle reported.
        size: u64,
    },
    /// The source failed.
    Unreadable(E),
    /// Two attempts were unstable.
    Unstable,
}

impl<E> ReadError<E> {
    /// The unavailability reason of [F20 §1.5].
    #[must_use]
    pub const fn reason(&self) -> Unavailable {
        match self {
            ReadError::Size { .. } => Unavailable::Size,
            ReadError::Unreadable(_) => Unavailable::Unreadable,
            ReadError::Unstable => Unavailable::Unstable,
        }
    }
}

/// The two-pass streaming reader ([F20 §2.4], [40 §2.5]) with its one fixed [`BUF_SIZE`] buffer. Keep one per thread
/// and reuse it for every file.
pub struct ContentReader {
    buf: Box<[u8]>,
}

impl Default for ContentReader {
    fn default() -> ContentReader {
        ContentReader::new()
    }
}

impl ContentReader {
    /// A reader with its buffer.
    #[must_use]
    pub fn new() -> ContentReader {
        ContentReader {
            buf: vec![0; BUF_SIZE].into_boxed_slice(),
        }
    }

    /// The heap bytes the reader holds (its buffer).
    #[must_use]
    pub fn heap_bytes(&self) -> usize {
        self.buf.len()
    }

    /// Reads the content of `src` in two passes and returns its statistics, `oid`, raw hash and line hashes, with the
    /// sink fed during pass 1.
    ///
    /// The source may be at any offset on entry: every pass of every attempt starts with a rewind, so both passes read
    /// from offset 0 ([F20 §2.4]) and a reused handle never wastes the one retry. The offset afterwards is
    /// unspecified.
    ///
    /// # Errors
    /// [`ReadError::Size`] for a file larger than `opts.max_read_bytes`; [`ReadError::Unreadable`] when the source
    /// fails (a read, a rewind or a snapshot); [`ReadError::Unstable`] when the content changed during both attempts.
    pub fn read<S: ByteSource, H: LineSink>(
        &mut self,
        src: &mut S,
        opts: &ReadOptions,
        sink: &mut H,
    ) -> Result<Content, ReadError<S::Error>> {
        for _ in 0..=r14::READ_RETRIES {
            if let Some(content) = self.attempt(src, opts, sink)? {
                return Ok(content);
            }
        }
        Err(ReadError::Unstable)
    }

    /// Reads again, in one pass, content that [`ContentReader::read`] returned as `first`, feeding its lines to `sink`
    /// ([F20 §2.4]: every later read of one call must see the first read's bytes).
    ///
    /// The pass reads from offset 0 between two snapshots of the handle and recomputes the raw length and XXH3-64. Equal
    /// sizes and last-write times around the pass and a raw length and hash equal to `first`'s are §2.4's stability test
    /// against the first read, so the bytes are the first read's and `oid`, which pass 2 hashed from them, is not
    /// computed again. An attempt that fails the test is repeated once ([F20 §2.4] `READ_RETRIES`).
    ///
    /// # Errors
    /// [`ReadError::Size`] for a file larger than `opts.max_read_bytes`; [`ReadError::Unreadable`] when the source
    /// fails; [`ReadError::Unstable`] when both attempts saw a change or bytes other than `first`'s.
    // spec: [F20 §2.4] (two passes, one handle; stability by sizes, last-write times, raw length and r)
    pub fn reread<S: ByteSource, H: LineSink>(
        &mut self,
        src: &mut S,
        opts: &ReadOptions,
        first: &Content,
        sink: &mut H,
    ) -> Result<(), ReadError<S::Error>> {
        for _ in 0..=r14::READ_RETRIES {
            let before = src.snapshot().map_err(ReadError::Unreadable)?;
            if before.size > opts.max_read_bytes {
                return Err(ReadError::Size { size: before.size });
            }
            src.rewind().map_err(ReadError::Unreadable)?;
            sink.begin();
            let mut p1 = Pass1::new(None, H::ACTIVE);
            let n = pump(src, &mut self.buf, before.size.saturating_add(1), |c| {
                p1.feed(c, sink);
            })
            .map_err(ReadError::Unreadable)?;
            let (_, r, _) = p1.finish(sink);
            let after = src.snapshot().map_err(ReadError::Unreadable)?;
            let stable = n == before.size
                && after.size == before.size
                && after.mtime == before.mtime
                && n == first.raw_len()
                && r == first.raw_xxh3;
            if stable {
                return Ok(());
            }
        }
        Err(ReadError::Unstable)
    }

    /// One attempt from offset 0; `None` when unstable.
    fn attempt<S: ByteSource, H: LineSink>(
        &mut self,
        src: &mut S,
        opts: &ReadOptions,
        sink: &mut H,
    ) -> Result<Option<Content>, ReadError<S::Error>> {
        let before = src.snapshot().map_err(ReadError::Unreadable)?;
        if before.size > opts.max_read_bytes {
            return Err(ReadError::Size { size: before.size });
        }
        // Reading one byte past the size proves a change without reading a growing file to its end.
        let limit = before.size.saturating_add(1);

        src.rewind().map_err(ReadError::Unreadable)?;
        sink.begin();
        let mut p1 = Pass1::new(opts.max_line_hashes, H::ACTIVE);
        let n1 =
            pump(src, &mut self.buf, limit, |c| p1.feed(c, sink)).map_err(ReadError::Unreadable)?;
        let (stats, r1, line_hashes) = p1.finish(sink);
        if n1 != before.size {
            return Ok(None);
        }

        src.rewind().map_err(ReadError::Unreadable)?;
        let mut p2 = Pass2::new(opts.format, &stats, true);
        pump(src, &mut self.buf, limit, |c| p2.feed(c)).map_err(ReadError::Unreadable)?;
        let p2 = p2.finish();
        let after = src.snapshot().map_err(ReadError::Unreadable)?;

        let stable = after.size == before.size
            && after.mtime == before.mtime
            && p2.raw == Some((before.size, r1))
            && p2.emitted == stats.norm_len();
        Ok(stable.then_some(Content {
            stats,
            raw_xxh3: r1,
            oid: p2.oid,
            line_hashes,
        }))
    }
}

/// Reads `src` to its end, or to `limit` bytes, through `buf`, handing each chunk to `f`; returns the bytes read.
fn pump<S: ByteSource>(
    src: &mut S,
    buf: &mut [u8],
    limit: u64,
    mut f: impl FnMut(&[u8]),
) -> Result<u64, S::Error> {
    let mut n = 0u64;
    while n < limit {
        let want = usize::try_from(limit - n).map_or(buf.len(), |r| r.min(buf.len()));
        let k = src.read(&mut buf[..want])?.min(want);
        if k == 0 {
            break;
        }
        n += k as u64;
        f(&buf[..k]);
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn notation() {
        assert_eq!(chars("aé€😀".as_bytes()), 4);
        assert_eq!(cutp("aéb".as_bytes(), 2), b"a");
        assert_eq!(cutp("aéb".as_bytes(), 3), "aé".as_bytes());
        assert_eq!(cutp(b"abc", 5), b"abc");
        assert_eq!(cutp(&[0x80, 0x80, 0x41], 1), b"");
        assert_eq!(cuts("aéb".as_bytes(), 2), b"b");
        assert_eq!(cuts("aéb".as_bytes(), 3), "éb".as_bytes());
        assert_eq!(cuts(b"abc", 9), b"abc");
        assert_eq!(cuts(&[0x80], 4), b"");
        assert!(eqi(b"ReadMe.MD", b"readme.md"));
        assert!(!eqi("É".as_bytes(), "é".as_bytes()));
        assert!(!eqi(b"a", b"ab"));
    }

    #[test]
    fn ws_and_trivial_classes() {
        for b in 0..=255u8 {
            assert_eq!(is_ws(b), r14::WS.contains(&b));
            assert_eq!(
                is_trivial_byte(b),
                r14::WS.contains(&b) || r14::TRIVIAL_LINE_EXTRA.contains(&b)
            );
        }
    }

    #[test]
    fn swar_helpers() {
        let data: Vec<u8> = (0..=255u8).collect();
        for n in 1..=128u8 {
            for i in 0..=data.len() - 8 {
                let w = load(&data, i);
                assert_eq!(has_less(w, n), data[i..i + 8].iter().any(|&b| b < n));
            }
        }
        for needle in [0u8, 10, 13, 255] {
            for len in 0..40 {
                let mut hay = vec![b'x'; len];
                assert_eq!(find_byte(&hay, needle), None);
                for p in 0..len {
                    hay[p] = needle;
                    assert_eq!(find_byte(&hay, needle), Some(p));
                    hay[p] = b'x';
                }
            }
        }
    }

    #[test]
    fn stats_follow_git() {
        let s = stats(b"a\r\nb\rc\n\x00\x7f\x08\x1b\x01");
        assert_eq!((s.crlf, s.lonecr, s.lonelf, s.nul), (1, 1, 1, 1));
        assert_eq!(s.nonprintable, 3); // NUL, DEL, 0x01
        assert_eq!(s.printable, 5); // a b c BS ESC
        assert!(!s.is_text());
        // A final ^Z is not counted; one elsewhere is.
        assert_eq!(stats(b"x\x1a").nonprintable, 0);
        assert_eq!(stats(b"\x1ax").nonprintable, 1);
        assert_eq!(stats(b"\x1a\x1a").nonprintable, 1);
        assert!(is_text(b""));
        assert!(!is_text(
            &[b'A'; 127].iter().copied().chain([1]).collect::<Vec<_>>()
        ));
        assert!(is_text(
            &[b'A'; 128].iter().copied().chain([1]).collect::<Vec<_>>()
        ));
        assert!(!is_text(b"a\x00b"));
        assert!(!is_text(b"a\r"));
        assert!(is_text(b"a\r\n"));
    }

    #[test]
    fn norm_and_atext() {
        assert_eq!(&*norm(b"hello\r\n"), b"hello\n");
        assert_eq!(&*norm(b"a\rb"), b"a\rb");
        assert_eq!(&*norm(b"x\r\n\x1a"), b"x\n\x1a");
        assert!(matches!(norm(b"a\nb"), Cow::Borrowed(_)));
        assert_eq!(atext(b"\xef\xbb\xbfa\r\nb").as_deref(), Some(&b"a\nb"[..]));
        assert_eq!(
            atext(b"\xef\xbb\xbf\xef\xbb\xbfa").as_deref(),
            Some(&b"\xef\xbb\xbfa"[..])
        );
        assert_eq!(atext(b"\xef\xbb\xbf").as_deref(), Some(&b""[..]));
        assert_eq!(atext(b"a\x00"), None);
    }

    #[test]
    fn lines_and_normalised_text() {
        let collect = |t: &[u8]| lines(t).map(<[u8]>::to_vec).collect::<Vec<_>>();
        assert!(collect(b"").is_empty());
        assert_eq!(collect(b"\n"), vec![b"".to_vec()]);
        assert_eq!(
            collect(b"a\n\nb"),
            vec![b"a".to_vec(), b"".to_vec(), b"b".to_vec()]
        );
        assert_eq!(collect(b"a\n"), vec![b"a".to_vec()]);
        assert_eq!(nl(b" \t fn x() {  \r"), b"fn x() {");
        assert_eq!(nl(b" \t\x0b "), b"");
        assert!(is_trivial_line(b"  });"));
        assert!(is_trivial_line(b""));
        assert!(!is_trivial_line(b"} else {"));

        let n = NormalisedText::new(b"  a  \n\n\tb c\t\n").unwrap();
        assert_eq!(n.bytes(), b"a\n\nb c");
        assert_eq!(n.line_count(), 3);
        assert_eq!((n.start(1), n.end(1)), (Some(0), Some(1)));
        assert_eq!((n.start(2), n.end(2)), (Some(2), Some(2)));
        assert_eq!((n.start(3), n.end(3)), (Some(3), Some(6)));
        assert_eq!(n.start(0), None);
        assert_eq!(n.end(4), None);
        assert_eq!(n.span(1, 3), Some(&b"a\n\nb c"[..]));
        assert_eq!(n.span(2, 2), Some(&b""[..]));
        assert_eq!(n.span(3, 1), None);
        assert_eq!(n.line_at(0), Some(1));
        assert_eq!(n.line_at(1), Some(1));
        assert_eq!(n.line_at(2), Some(2));
        assert_eq!(n.line_at(6), Some(3));
        assert_eq!(n.line_at(7), None);
        assert_eq!(NormalisedText::new(b"").unwrap().line_at(0), None);
    }

    #[test]
    fn worked_oid_values() {
        // [F20 §2.3]'s informative table.
        let sha1 = |b: &[u8]| oid_of(ObjectFormat::Sha1, b).to_string();
        assert_eq!(sha1(b""), "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391");
        assert_eq!(
            sha1(b"hello\r\n"),
            "ce013625030ba8dba906f756967f9e9ca394464a"
        );
        assert_eq!(
            oid_of(ObjectFormat::Sha256, b"hello\r\n").to_string(),
            "2cf8d83d9ee29543b34a87727421fdecb7e3f3a183d337639025de576db9ebb4"
        );
        assert_eq!(sha1(b"a\r\nb"), "0a207c060e61f3b88eaee0a8cd0696f46fb155eb");
        assert_eq!(sha1(b"a\rb"), "2fe40ba389048204a83882bc3f75bf2188db6d47");
        assert_eq!(
            sha1(b"x\r\n\x1a"),
            "2f484e3cd37e421218bc8a9929df89ceb50dbb1a"
        );
    }

    #[test]
    fn line_hashes_over_chunks() {
        struct Collect(Vec<Vec<u8>>, Vec<u8>);
        impl LineSink for Collect {
            fn begin(&mut self) {
                self.0.clear();
                self.1.clear();
            }
            fn piece(&mut self, b: &[u8]) {
                self.1.extend_from_slice(b);
            }
            fn end_line(&mut self) {
                self.0.push(core::mem::take(&mut self.1));
            }
        }
        let content = b"\xef\xbb\xbf  alpha beta \r\n\t});\r\n  x  \n\nlast\t";
        let t = atext(content).unwrap();
        let want: Vec<LineHash> = lines(&t).map(LineHash::of).collect();
        assert_eq!(
            want.iter().map(|h| h.is_trivial()).collect::<Vec<_>>(),
            [false, true, false, true, false]
        );
        let want_lines: Vec<Vec<u8>> = lines(&t).map(<[u8]>::to_vec).collect();
        for size in 1..=content.len() {
            let mut sink = Collect(Vec::new(), Vec::new());
            sink.begin();
            let mut p1 = Pass1::new(Some(100), true);
            for c in content.chunks(size) {
                p1.feed(c, &mut sink);
            }
            let (stats, _, hashes) = p1.finish(&mut sink);
            assert!(stats.is_text());
            let hashes = hashes.unwrap();
            assert!(hashes.is_complete());
            assert_eq!(hashes.iter().collect::<Vec<_>>(), want, "chunk size {size}");
            assert_eq!(sink.0, want_lines, "chunk size {size}");
        }
    }

    #[test]
    fn line_hash_cap() {
        let content = b"a\n{\nc\nd\n";
        let c = analyse(content, ObjectFormat::Sha1, Some(2), &mut ());
        let h = c.line_hashes.unwrap();
        assert_eq!((h.len(), h.total_lines(), h.is_complete()), (2, 4, false));
        assert_eq!(h.get(1), Some(LineHash::TRIVIAL));
        assert_eq!(h.get(2), None);
        // `wh(l) = low(XXH3-64(nl(l)), 16)`, the first two bytes of the stored hash read little-endian ([F20 §1.2]).
        let full = xxh3_64(b"a").to_le_bytes();
        assert_eq!(
            h.get(0).unwrap().window_hash(),
            Some(u16::from_le_bytes([full[0], full[1]]))
        );
        assert_eq!(window_hash(b"  a \t"), h.get(0).unwrap().window_hash());
        assert_eq!(window_hash(b" }; "), None);
    }

    #[test]
    fn line_hash_blocks() {
        // Entries across several blocks, with trivial bits on both sides of every word and block boundary.
        let n = 2 * BLOCK + 70;
        let mut content = Vec::new();
        for i in 0..n {
            if i % 3 == 0 {
                content.extend_from_slice(b"}\n");
            } else {
                content.extend_from_slice(format!("line {i}\n").as_bytes());
            }
        }
        let cap = u32::try_from(n - 5).unwrap();
        let h = analyse(&content, ObjectFormat::Sha1, Some(cap), &mut ())
            .line_hashes
            .unwrap();
        assert_eq!((h.len(), h.total_lines()), (n - 5, n as u64));
        for i in 0..n - 5 {
            let want = if i % 3 == 0 {
                LineHash::TRIVIAL
            } else {
                LineHash::non_trivial(low16(xxh3_64(format!("line {i}").as_bytes())))
            };
            assert_eq!(h.get(i), Some(want), "line {}", i + 1);
        }
        assert_eq!(h.iter().count(), n - 5);
        // Three blocks and their table: 2 bytes and 1 bit per recorded line, rounded up to whole blocks.
        assert_eq!(h.blocks.len(), 3);
        assert_eq!(
            h.heap_bytes(),
            3 * size_of::<Block>() + h.blocks.capacity() * size_of::<Box<Block>>()
        );
        assert_eq!(size_of::<Block>(), BLOCK * 2 + BLOCK / 8);
    }

    #[test]
    fn normalised_text_reserves_exactly() {
        for t in [&b""[..], b"\n", b"a", b"a\n", b"\n\n\n", b"  x \ny\n\nz"] {
            let n = NormalisedText::new(t).unwrap();
            assert_eq!(n.line_count(), lines(t).count());
            assert_eq!(n.starts.capacity(), n.line_count());
            assert!(n.text.capacity() <= t.len());
        }
    }

    #[test]
    fn in_memory_paths_agree() {
        for b in [
            &b""[..],
            b"\xef\xbb\xbf",
            b"\xef\xbb\xbfa\r\nb\r\n",
            b"a\r\nb",
            b"a\rb",
            b"x\r\n\x1a",
            b"\x00\r\n",
        ] {
            let full = analyse(b, ObjectFormat::Sha256, Some(8), &mut ());
            assert_eq!(oid_of(ObjectFormat::Sha256, b), full.oid);
            assert_eq!(full.raw_xxh3, xxh3_64(b));
            let text = is_text(b);
            assert_eq!(atext(b).is_some(), text);
            if let Some(t) = atext(b) {
                let n = norm(b);
                assert_eq!(&*t, n.strip_prefix(&BOM[..]).unwrap_or(&n));
            }
        }
    }

    // --- property tests of the notation and N(t) against the literal definitions ([F20 §1.2, §2.5]) --------------

    /// The proptest configuration of a suite whose tier-`pr` case count is `base`: `MOIRAI_TEST_TIER` = `nightly` runs
    /// 16 times as many and `exit` 64 times as many (PLAN §2.1 test tiers); no failure persistence (the seed is
    /// printed).
    fn test_config(base: u32) -> ProptestConfig {
        let cases = match std::env::var("MOIRAI_TEST_TIER").as_deref() {
            Ok("nightly") => base * 16,
            Ok("exit") => base * 64,
            _ => base,
        };
        ProptestConfig {
            cases,
            failure_persistence: None,
            ..ProptestConfig::default()
        }
    }

    /// `80`–`BF`, written from [F20 §1.2] without the crate's helpers.
    fn naive_cont(b: u8) -> bool {
        (0x80..=0xBF).contains(&b)
    }

    /// `chars(x)` by its definition: the bytes outside `80`–`BF`.
    fn naive_chars(x: &[u8]) -> usize {
        x.iter().filter(|&&b| !naive_cont(b)).count()
    }

    /// `cutp(x, n)` by brute force: the longest prefix of at most n bytes whose next byte, if any, is not in
    /// `80`–`BF`; the empty prefix when none qualifies (see [`cutp`]).
    fn naive_cutp(x: &[u8], n: usize) -> &[u8] {
        (0..=n.min(x.len()))
            .rev()
            .find(|&k| k == x.len() || !naive_cont(x[k]))
            .map_or(&x[..0], |k| &x[..k])
    }

    /// `cuts(x, n)` by brute force: the longest suffix of at most n bytes whose first byte, if any, is not in
    /// `80`–`BF`. The empty suffix always qualifies.
    fn naive_cuts(x: &[u8], n: usize) -> &[u8] {
        let k = (0..=n.min(x.len()))
            .rev()
            .find(|&k| k == 0 || !naive_cont(x[x.len() - k]))
            .unwrap_or(0);
        &x[x.len() - k..]
    }

    fn naive_ws(b: u8) -> bool {
        [0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x20].contains(&b)
    }

    /// `nl(l)`: l without leading and trailing `WS` bytes.
    fn naive_nl(l: &[u8]) -> &[u8] {
        let a = l.iter().take_while(|&&b| naive_ws(b)).count();
        let l = &l[a..];
        let z = l.iter().rev().take_while(|&&b| naive_ws(b)).count();
        &l[..l.len() - z]
    }

    /// `lines(t)`: split at every `0A`, the last piece dropped if empty.
    fn naive_lines(t: &[u8]) -> Vec<&[u8]> {
        let mut v: Vec<&[u8]> = t.split(|&b| b == 0x0A).collect();
        if v.last().is_some_and(|l| l.is_empty()) {
            v.pop();
        }
        v
    }

    /// Bytes that stress UTF-8 cuts: ASCII, continuation bytes, lead bytes of every length, invalid bytes.
    fn utf8ish() -> impl Strategy<Value = Vec<u8>> {
        prop::collection::vec(
            prop_oneof![
                3 => prop::sample::select(b"ab \n".to_vec()),
                4 => 0x80u8..=0xBF,
                2 => prop::sample::select(vec![0xC3u8, 0xE2, 0xF0, 0xC0, 0xF8, 0xFF]),
                1 => any::<u8>(),
            ],
            0..24,
        )
    }

    /// Anchor-text-like bytes for `N(t)`: `0A`, every `WS` byte, brackets, letters, a high byte.
    fn texty() -> impl Strategy<Value = Vec<u8>> {
        prop::collection::vec(
            prop::sample::select(b"\n\n\n\t\x0b\x0c\r  ab{};\xc3\xa9".to_vec()),
            0..60,
        )
    }

    proptest! {
        #![proptest_config(test_config(1024))]

        #[test]
        fn cuts_and_chars_follow_the_definitions(x in utf8ish(), n in 0usize..30) {
            prop_assert_eq!(chars(&x), naive_chars(&x));
            prop_assert_eq!(cutp(&x, n), naive_cutp(&x, n));
            prop_assert_eq!(cuts(&x, n), naive_cuts(&x, n));
        }

        /// On valid UTF-8 neither cut splits a scalar value, and each keeps as many bytes as a char boundary allows.
        #[test]
        fn cuts_never_split_a_scalar_value(s in any::<String>(), n in 0usize..40) {
            let x = s.as_bytes();
            let p = (0..=n.min(x.len())).rev().find(|&k| s.is_char_boundary(k));
            prop_assert_eq!(cutp(x, n), &x[..p.unwrap_or(0)]);
            let q = (x.len().saturating_sub(n)..=x.len()).find(|&k| s.is_char_boundary(k));
            prop_assert_eq!(cuts(x, n), &x[q.unwrap_or(x.len())..]);
            prop_assert_eq!(chars(x), s.chars().count());
        }

        /// `N(t)`, `start`, `end`, `ST` and the line of an offset against [F20 §2.5] computed from scratch.
        #[test]
        fn normalised_text_follows_the_definitions(t in texty()) {
            let ls = naive_lines(&t);
            let m = ls.len();
            let nls: Vec<&[u8]> = ls.iter().map(|l| naive_nl(l)).collect();
            let want = nls.join(&b'\n');
            let mut starts = Vec::with_capacity(m);
            let mut off = 0;
            for l in &nls {
                starts.push(off);
                off += l.len() + 1;
            }
            let ends: Vec<usize> = starts.iter().zip(&nls).map(|(s, l)| s + l.len()).collect();

            let n = NormalisedText::new(&t).unwrap();
            prop_assert_eq!(n.bytes(), &want[..]);
            prop_assert_eq!(n.line_count(), m);
            prop_assert_eq!(n.start(0), None);
            prop_assert_eq!(n.end(0), None);
            prop_assert_eq!(n.start(m + 1), None);
            prop_assert_eq!(n.end(m + 1), None);
            for i in 1..=m {
                prop_assert_eq!(n.start(i), Some(starts[i - 1]));
                prop_assert_eq!(n.end(i), Some(ends[i - 1]));
                prop_assert_eq!(&want[starts[i - 1]..ends[i - 1]], nls[i - 1]);
            }
            for s in 0..=m + 1 {
                for e in 0..=m + 1 {
                    let st = (1 <= s && s <= e && e <= m)
                        .then(|| &want[starts[s - 1]..ends[e - 1]]);
                    prop_assert_eq!(n.span(s, e), st, "ST({}, {})", s, e);
                }
            }
            // The line of an offset by a scan of N: 1 + the `0A` bytes before it; none past N or for no lines.
            for o in 0..=want.len() + 1 {
                let line = (m > 0 && o <= want.len())
                    .then(|| 1 + want[..o].iter().filter(|&&b| b == b'\n').count());
                prop_assert_eq!(n.line_at(o), line, "offset {}", o);
            }
        }
    }
}
