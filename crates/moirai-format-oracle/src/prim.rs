//! \[F01\] primitives: little-endian fixed-width integers (§5.1), varints (§5.2, §5.3), booleans (§5.4), fixed-width byte
//! strings (§5.6), length-prefixed strings (§6.2), `lp()` (§6.3), hexadecimal text (§6.4), the hash set (§7.1–§7.3) and
//! the git object-format registry (§7.5).
//!
//! Every decoder here is hand-written over a byte cursor ([`Reader`]); every encoder appends to a [`Writer`]. Decoders
//! refuse exactly what \[F01\] refuses: non-canonical or over-long varints, invalid UTF-8, non-zero reserved bytes.

use core::fmt;

pub use crate::image::text::Rule;

/// A decoding failure: the absolute byte offset where it was found and the rule it breaks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    /// Absolute offset of the failing field in the decoded buffer.
    pub offset: usize,
    /// What rule the bytes break, citing the spec section.
    pub reason: String,
    /// The named `ImageParse` rule of an image refusal ([F14 §9.2], §10.9); `None` for the binary decoders.
    pub rule: Option<Rule>,
}

impl Error {
    /// The same failure `by` bytes further on (an error found in a slice, placed in its container).
    pub fn shifted(self, by: usize) -> Error {
        Error {
            offset: self.offset + by,
            ..self
        }
    }

    /// The failure with `rule` when it names none yet (an error of a shared decoder, placed by the image check that
    /// called it).
    pub fn or_rule(self, rule: Rule) -> Error {
        Error {
            rule: self.rule.or(Some(rule)),
            ..self
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "at byte {}: {}", self.offset, self.reason)?;
        if let Some(r) = self.rule {
            write!(f, " (rule {})", r.id())?;
        }
        Ok(())
    }
}

impl std::error::Error for Error {}

/// Result of every oracle decoder.
pub type Result<T> = core::result::Result<T, Error>;

/// Builds an [`Error`] at `offset`.
pub fn err<T>(offset: usize, reason: impl Into<String>) -> Result<T> {
    Err(Error {
        offset,
        reason: reason.into(),
        rule: None,
    })
}

/// The declared bound of a varint field ([F01 §5.2]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VarBound {
    /// `uvar16`: at most 3 bytes, value ≤ 65,535.
    U16,
    /// `uvar32`: at most 5 bytes, value ≤ 2^32 − 1.
    U32,
    /// `uvar64`: at most 10 bytes, value ≤ 2^64 − 1.
    U64,
}

impl VarBound {
    /// The greatest value of the bound.
    pub const fn max(self) -> u64 {
        match self {
            VarBound::U16 => u16::MAX as u64,
            VarBound::U32 => u32::MAX as u64,
            VarBound::U64 => u64::MAX,
        }
    }

    /// The longest encoding the bound allows.
    pub const fn max_len(self) -> usize {
        match self {
            VarBound::U16 => 3,
            VarBound::U32 => 5,
            VarBound::U64 => 10,
        }
    }
}

/// A byte cursor with absolute offsets for error reports.
#[derive(Clone, Debug)]
pub struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
    base: usize,
}

impl<'a> Reader<'a> {
    /// A cursor over `buf`, reporting offsets from 0.
    pub fn new(buf: &'a [u8]) -> Self {
        Reader {
            buf,
            pos: 0,
            base: 0,
        }
    }

    /// A cursor over `buf` whose first byte has absolute offset `base`.
    pub fn with_base(buf: &'a [u8], base: usize) -> Self {
        Reader { buf, pos: 0, base }
    }

    /// Absolute offset of the next byte.
    pub fn offset(&self) -> usize {
        self.base + self.pos
    }

    /// Position relative to the start of this cursor.
    pub fn pos(&self) -> usize {
        self.pos
    }

    /// Bytes not yet consumed.
    pub fn remaining(&self) -> usize {
        self.buf.len() - self.pos
    }

    /// True when every byte is consumed.
    pub fn is_empty(&self) -> bool {
        self.pos == self.buf.len()
    }

    /// The unconsumed bytes, without consuming them.
    pub fn rest(&self) -> &'a [u8] {
        &self.buf[self.pos..]
    }

    /// An error at the current offset.
    pub fn fail<T>(&self, reason: impl Into<String>) -> Result<T> {
        err(self.offset(), reason)
    }

    /// Requires every byte to be consumed ([F05 §8.8], [F01 §2.6] "rows cover every byte").
    pub fn finish(&self, what: &str) -> Result<()> {
        if self.is_empty() {
            Ok(())
        } else {
            self.fail(format!(
                "{} bytes left after the last field of {what}",
                self.remaining()
            ))
        }
    }

    /// Consumes `n` bytes.
    pub fn bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        if n > self.remaining() {
            return self.fail(format!("needs {n} bytes, {} left", self.remaining()));
        }
        let s = &self.buf[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }

    /// A sub-cursor over the next `n` bytes, which are consumed.
    pub fn sub(&mut self, n: usize) -> Result<Reader<'a>> {
        let base = self.offset();
        let s = self.bytes(n)?;
        Ok(Reader::with_base(s, base))
    }

    /// Consumes a fixed-size array.
    pub fn array<const N: usize>(&mut self) -> Result<[u8; N]> {
        let s = self.bytes(N)?;
        let mut a = [0u8; N];
        a.copy_from_slice(s);
        Ok(a)
    }

    /// `u8`.
    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.bytes(1)?[0])
    }

    /// `u16`, little-endian ([F01 §4.1]).
    pub fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.array()?))
    }

    /// `u32`, little-endian.
    pub fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    /// `u64`, little-endian.
    pub fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.array()?))
    }

    /// `i32`, two's complement little-endian.
    pub fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(self.array()?))
    }

    /// `i64`, two's complement little-endian.
    pub fn i64(&mut self) -> Result<i64> {
        Ok(i64::from_le_bytes(self.array()?))
    }

    /// `f64` as its little-endian bit pattern ([F01 §5.5]).
    pub fn f64_bits(&mut self) -> Result<u64> {
        self.u64()
    }

    /// `bool8` ([F01 §5.4]): `00` or `01`.
    pub fn bool8(&mut self) -> Result<bool> {
        let at = self.offset();
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            v => err(
                at,
                format!("bool8 {v:#04x} is neither 00 nor 01 [F01 §5.4]"),
            ),
        }
    }

    /// `n` reserved bytes that must be zero ([F01 §10] rule 3).
    pub fn zeros(&mut self, n: usize, what: &str) -> Result<()> {
        let at = self.offset();
        let s = self.bytes(n)?;
        if let Some(i) = s.iter().position(|&b| b != 0) {
            return err(
                at + i,
                format!("reserved byte of {what} is not zero [F01 §10]"),
            );
        }
        Ok(())
    }

    /// An unsigned LEB128 varint with its bound, canonical form enforced ([F01 §5.2]).
    pub fn uvar(&mut self, bound: VarBound) -> Result<u64> {
        let at = self.offset();
        let mut value: u64 = 0;
        let mut shift = 0u32;
        let mut len = 0usize;
        loop {
            if self.is_empty() {
                return err(at, "varint cut off by the end of its container [F01 §5.2]");
            }
            let b = self.u8()?;
            len += 1;
            if len > bound.max_len() {
                return err(at, "varint longer than its bound allows [F01 §5.2]");
            }
            let group = u64::from(b & 0x7F);
            if shift == 63 && group > 1 {
                return err(at, "varint value above 2^64 - 1 [F01 §5.2]");
            }
            value |= group << shift;
            if b & 0x80 == 0 {
                if b == 0 && len > 1 {
                    return err(at, "non-canonical varint (last byte 00) [F01 §5.2]");
                }
                break;
            }
            shift += 7;
        }
        if value > bound.max() {
            return err(
                at,
                format!("varint {value} above its bound {} [F01 §5.2]", bound.max()),
            );
        }
        Ok(value)
    }

    /// `uvar16`.
    pub fn uvar16(&mut self) -> Result<u16> {
        Ok(self.uvar(VarBound::U16)? as u16)
    }

    /// `uvar32`.
    pub fn uvar32(&mut self) -> Result<u32> {
        Ok(self.uvar(VarBound::U32)? as u32)
    }

    /// `uvar64`.
    pub fn uvar64(&mut self) -> Result<u64> {
        self.uvar(VarBound::U64)
    }

    /// `svar64`: zigzag over `uvar64` ([F01 §5.3]).
    pub fn svar64(&mut self) -> Result<i64> {
        let z = self.uvar64()?;
        Ok(((z >> 1) as i64) ^ -((z & 1) as i64))
    }

    /// A `uvar32` count used to size an allocation; refused when it exceeds the bytes left, since every counted item
    /// takes at least `min_item` bytes.
    pub fn count(&mut self, min_item: usize) -> Result<usize> {
        let at = self.offset();
        let n = self.uvar32()? as usize;
        if min_item > 0 && n > self.remaining() / min_item {
            return err(
                at,
                format!("count {n} exceeds the bytes left in its container"),
            );
        }
        Ok(n)
    }

    /// `vbytes`: `uvar32` length, then the bytes ([F01 §6.2]).
    pub fn vbytes(&mut self) -> Result<&'a [u8]> {
        let n = self.uvar32()? as usize;
        self.bytes(n)
    }

    /// `vstr`: `vbytes` holding valid UTF-8 ([F01 §6.1], §6.2).
    pub fn vstr(&mut self) -> Result<&'a str> {
        let at = self.offset();
        let b = self.vbytes()?;
        utf8(b, at)
    }

    /// `fstr<N>` ([F01 §6.2]): `u16` length L ≤ N − 2, L bytes of UTF-8, then N − 2 − L zero bytes.
    pub fn fstr(&mut self, n: usize) -> Result<&'a str> {
        let at = self.offset();
        let l = self.u16()? as usize;
        if l > n - 2 {
            return err(
                at,
                format!("fstr<{n}> length {l} above {} [F01 §6.2]", n - 2),
            );
        }
        let t_at = self.offset();
        let t = self.bytes(l)?;
        let s = utf8(t, t_at)?;
        self.zeros(n - 2 - l, "fstr fill")?;
        Ok(s)
    }

    /// `b16`.
    pub fn b16(&mut self) -> Result<[u8; 16]> {
        self.array()
    }

    /// `b20`.
    pub fn b20(&mut self) -> Result<[u8; 20]> {
        self.array()
    }

    /// `b32`.
    pub fn b32(&mut self) -> Result<[u8; 32]> {
        self.array()
    }

    /// `oidv`: the variable-width object id of [F01 §7.5].
    pub fn oidv(&mut self) -> Result<Oid> {
        let at = self.offset();
        match self.u8()? {
            0 => Ok(Oid::None),
            1 => Ok(Oid::Sha1(self.b20()?)),
            2 => Ok(Oid::Sha256(self.b32()?)),
            a => err(
                at,
                format!("object-format value {a} is reserved [F01 §7.5]"),
            ),
        }
    }

    /// `digest(a)` ([F05 §8.2]): 20 bytes for `sha1`, 32 for `sha256`.
    pub fn digest(&mut self, algo: Algo) -> Result<Oid> {
        match algo {
            Algo::Sha1 => Ok(Oid::Sha1(self.b20()?)),
            Algo::Sha256 => Ok(Oid::Sha256(self.b32()?)),
        }
    }

    /// The fixed 32-byte id slot of [F01 §7.5], given its algorithm (0 `none`, 1, 2).
    pub fn oid_slot(&mut self, algo: u8) -> Result<Oid> {
        let at = self.offset();
        let s: [u8; 32] = self.array()?;
        let zero_from = |from: usize| s[from..].iter().all(|&b| b == 0);
        match algo {
            0 if zero_from(0) => Ok(Oid::None),
            0 => err(at, "object id slot with algo none is not zero [F01 §7.5]"),
            1 if zero_from(20) => {
                let mut d = [0u8; 20];
                d.copy_from_slice(&s[..20]);
                Ok(Oid::Sha1(d))
            }
            1 => err(
                at + 20,
                "digest_hi of a sha1 id slot is not zero [F01 §7.5]",
            ),
            2 => Ok(Oid::Sha256(s)),
            a => err(
                at,
                format!("object-format value {a} is reserved [F01 §7.5]"),
            ),
        }
    }
}

/// Checks UTF-8 at absolute offset `at` ([F01 §6.1]).
pub fn utf8(b: &[u8], at: usize) -> Result<&str> {
    match core::str::from_utf8(b) {
        Ok(s) => Ok(s),
        Err(e) => err(at + e.valid_up_to(), "invalid UTF-8 [F01 §6.1]"),
    }
}

/// An append-only byte sink for the test-only re-encoders.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Writer {
    buf: Vec<u8>,
}

impl Writer {
    /// An empty sink.
    pub fn new() -> Self {
        Writer { buf: Vec::new() }
    }

    /// The bytes written so far.
    pub fn as_slice(&self) -> &[u8] {
        &self.buf
    }

    /// Consumes the sink.
    pub fn into_vec(self) -> Vec<u8> {
        self.buf
    }

    /// Bytes written so far.
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    /// True when nothing is written.
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// Mutable access, for back-patching checksums.
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.buf
    }

    /// Raw bytes.
    pub fn bytes(&mut self, b: &[u8]) {
        self.buf.extend_from_slice(b);
    }

    /// `n` zero bytes.
    pub fn zeros(&mut self, n: usize) {
        self.buf.resize(self.buf.len() + n, 0);
    }

    /// `u8`.
    pub fn u8(&mut self, v: u8) {
        self.buf.push(v);
    }

    /// `u16` little-endian.
    pub fn u16(&mut self, v: u16) {
        self.bytes(&v.to_le_bytes());
    }

    /// `u32` little-endian.
    pub fn u32(&mut self, v: u32) {
        self.bytes(&v.to_le_bytes());
    }

    /// `u64` little-endian.
    pub fn u64(&mut self, v: u64) {
        self.bytes(&v.to_le_bytes());
    }

    /// `i32` little-endian.
    pub fn i32(&mut self, v: i32) {
        self.bytes(&v.to_le_bytes());
    }

    /// `i64` little-endian.
    pub fn i64(&mut self, v: i64) {
        self.bytes(&v.to_le_bytes());
    }

    /// `bool8`.
    pub fn bool8(&mut self, v: bool) {
        self.u8(u8::from(v));
    }

    /// Unsigned LEB128, shortest form ([F01 §5.2]).
    pub fn uvar(&mut self, mut v: u64) {
        loop {
            let b = (v & 0x7F) as u8;
            v >>= 7;
            if v == 0 {
                self.u8(b);
                return;
            }
            self.u8(b | 0x80);
        }
    }

    /// Zigzag `svar64` ([F01 §5.3]).
    pub fn svar(&mut self, i: i64) {
        self.uvar(((i << 1) ^ (i >> 63)) as u64);
    }

    /// `vbytes`.
    pub fn vbytes(&mut self, b: &[u8]) {
        self.uvar(b.len() as u64);
        self.bytes(b);
    }

    /// `vstr`.
    pub fn vstr(&mut self, s: &str) {
        self.vbytes(s.as_bytes());
    }

    /// `fstr<N>`; the text must already fit (decoders never produce a longer one).
    pub fn fstr(&mut self, n: usize, s: &str) {
        debug_assert!(s.len() <= n - 2);
        self.u16(s.len() as u16);
        self.bytes(s.as_bytes());
        self.zeros(n - 2 - s.len());
    }

    /// `oidv` ([F01 §7.5], variable-width form).
    pub fn oidv(&mut self, o: &Oid) {
        self.u8(o.algo_byte());
        self.bytes(o.digest());
    }

    /// `digest(a)`: the digest bytes alone.
    pub fn digest(&mut self, o: &Oid) {
        self.bytes(o.digest());
    }

    /// The fixed 32-byte id slot of [F01 §7.5].
    pub fn oid_slot(&mut self, o: &Oid) {
        let d = o.digest();
        self.bytes(d);
        self.zeros(32 - d.len());
    }
}

/// A non-`none` git object format ([F01 §7.5]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Algo {
    /// 1, SHA-1, 20 bytes.
    Sha1,
    /// 2, SHA-256, 32 bytes.
    Sha256,
}

impl Algo {
    /// Decodes a `u8` that must be 1 or 2.
    pub fn from_byte(b: u8, at: usize) -> Result<Algo> {
        match b {
            1 => Ok(Algo::Sha1),
            2 => Ok(Algo::Sha256),
            _ => err(
                at,
                format!("object format {b} is not sha1 (1) or sha256 (2) [F01 §7.5]"),
            ),
        }
    }

    /// The registry value.
    pub fn byte(self) -> u8 {
        match self {
            Algo::Sha1 => 1,
            Algo::Sha256 => 2,
        }
    }

    /// The digest length.
    pub fn digest_len(self) -> usize {
        match self {
            Algo::Sha1 => 20,
            Algo::Sha256 => 32,
        }
    }

    /// The registry name ([F01 §7.5]).
    pub fn name(self) -> &'static str {
        match self {
            Algo::Sha1 => "sha1",
            Algo::Sha256 => "sha256",
        }
    }
}

/// A git object id or content `oid` of any registry value, `none` included ([F01 §7.5]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Oid {
    /// `algo` 0, no digest.
    None,
    /// `algo` 1.
    Sha1([u8; 20]),
    /// `algo` 2.
    Sha256([u8; 32]),
}

impl Oid {
    /// The registry value.
    pub fn algo_byte(&self) -> u8 {
        match self {
            Oid::None => 0,
            Oid::Sha1(_) => 1,
            Oid::Sha256(_) => 2,
        }
    }

    /// The digest bytes (empty for `none`).
    pub fn digest(&self) -> &[u8] {
        match self {
            Oid::None => &[],
            Oid::Sha1(d) => d,
            Oid::Sha256(d) => d,
        }
    }
}

/// XXH3-64 with seed 0 ([F01 §7.1]).
pub fn xxh3_64(b: &[u8]) -> u64 {
    xxhash_rust::xxh3::xxh3_64(b)
}

/// XXH3-64 with seed `s` ([F01 §7.1]).
pub fn xxh3_64_seeded(b: &[u8], s: u64) -> u64 {
    xxhash_rust::xxh3::xxh3_64_with_seed(b, s)
}

/// XXH3-128 with seed 0 as (`low64`, `high64`) ([F01 §7.2]).
pub fn xxh3_128(b: &[u8]) -> (u64, u64) {
    let v = xxhash_rust::xxh3::xxh3_128(b);
    (v as u64, (v >> 64) as u64)
}

/// BLAKE3-256 ([F01 §7.1]).
pub fn blake3_256(b: &[u8]) -> [u8; 32] {
    *blake3::hash(b).as_bytes()
}

/// BLAKE3-128: the first 16 bytes of BLAKE3-256 ([F01 §7.1]).
pub fn blake3_128(b: &[u8]) -> [u8; 16] {
    let h = blake3_256(b);
    let mut o = [0u8; 16];
    o.copy_from_slice(&h[..16]);
    o
}

/// `lp(x) = u32-le(len(x)) ‖ x` ([F01 §6.3]), appended to `out`.
pub fn lp(out: &mut Vec<u8>, x: &[u8]) {
    let n = u32::try_from(x.len()).expect("lp argument below 2^32 [F01 §6.3]");
    out.extend_from_slice(&n.to_le_bytes());
    out.extend_from_slice(x);
}

/// Lower-case hexadecimal text of `b`, in byte order ([F01 §6.4]).
pub fn hex(b: &[u8]) -> String {
    const D: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(b.len() * 2);
    for &x in b {
        s.push(D[usize::from(x >> 4)] as char);
        s.push(D[usize::from(x & 15)] as char);
    }
    s
}

/// Parses lower-case hexadecimal text ([F01 §6.4]: text moirai writes is lower-case only).
pub fn unhex(s: &str) -> Option<Vec<u8>> {
    let b = s.as_bytes();
    if !b.len().is_multiple_of(2) {
        return None;
    }
    let nib = |c: u8| match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        _ => None,
    };
    b.chunks(2)
        .map(|p| Some(nib(p[0])? << 4 | nib(p[1])?))
        .collect()
}

/// Reads a `u64` little-endian from the first 8 bytes of `b` ([F01 §7.2] "integer from a digest").
pub fn le_u64(b: &[u8]) -> u64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&b[..8]);
    u64::from_le_bytes(a)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn enc(v: u64) -> Vec<u8> {
        let mut w = Writer::new();
        w.uvar(v);
        w.into_vec()
    }

    /// [F01 §5.2] examples table.
    #[test]
    fn uvar_examples() {
        let cases: &[(u64, &[u8])] = &[
            (0, &[0x00]),
            (1, &[0x01]),
            (127, &[0x7F]),
            (128, &[0x80, 0x01]),
            (300, &[0xAC, 0x02]),
            (16_383, &[0xFF, 0x7F]),
            (16_384, &[0x80, 0x80, 0x01]),
            (65_535, &[0xFF, 0xFF, 0x03]),
            (u64::from(u32::MAX), &[0xFF, 0xFF, 0xFF, 0xFF, 0x0F]),
            (
                u64::MAX,
                &[0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x01],
            ),
        ];
        for &(v, b) in cases {
            assert_eq!(enc(v), b, "encode {v}");
            let mut r = Reader::new(b);
            assert_eq!(r.uvar64().unwrap(), v);
            assert!(r.is_empty());
        }
    }

    /// [F01 §5.2]: `80 00` and `81 00` are refused, as are over-long and out-of-bound encodings.
    #[test]
    fn uvar_refusals() {
        assert!(Reader::new(&[0x80, 0x00]).uvar64().is_err());
        assert!(Reader::new(&[0x81, 0x00]).uvar64().is_err());
        assert!(Reader::new(&[0x80]).uvar64().is_err());
        assert!(
            Reader::new(&[0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x02])
                .uvar64()
                .is_err()
        );
        assert!(
            Reader::new(&[0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x00])
                .uvar64()
                .is_err()
        );
        assert!(Reader::new(&[0x80, 0x80, 0x04]).uvar16().is_err());
        assert!(Reader::new(&[0x80, 0x80, 0x80, 0x01]).uvar16().is_err());
        assert!(
            Reader::new(&[0xFF, 0xFF, 0xFF, 0xFF, 0x1F])
                .uvar32()
                .is_err()
        );
        assert_eq!(Reader::new(&[0xFF, 0xFF, 0x03]).uvar16().unwrap(), 65_535);
    }

    /// [F01 §5.3] examples table.
    #[test]
    fn svar_examples() {
        let cases: &[(i64, &[u8])] = &[
            (0, &[0x00]),
            (-1, &[0x01]),
            (1, &[0x02]),
            (-64, &[0x7F]),
            (64, &[0x80, 0x01]),
            (-65, &[0x81, 0x01]),
            (
                i64::MAX,
                &[0xFE, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x01],
            ),
            (
                i64::MIN,
                &[0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x01],
            ),
        ];
        for &(i, b) in cases {
            let mut w = Writer::new();
            w.svar(i);
            assert_eq!(w.as_slice(), b, "encode {i}");
            assert_eq!(Reader::new(b).svar64().unwrap(), i);
        }
    }

    /// [F01 §4.1] and §7.2 byte-order examples, §5.7 the `hlc` example.
    #[test]
    fn byte_order_examples() {
        let mut w = Writer::new();
        w.u32(0x0A0B_0C0D);
        w.u64(0x0102_0304_0506_0708);
        assert_eq!(
            w.as_slice(),
            &[0x0D, 0x0C, 0x0B, 0x0A, 8, 7, 6, 5, 4, 3, 2, 1]
        );
        let hlc = (1_790_000_000_000u64 << 16) | 3;
        assert_eq!(hlc, 0x01A0_C450_6C00_0003);
        let mut w = Writer::new();
        w.u64(hlc);
        assert_eq!(
            w.as_slice(),
            &[0x03, 0x00, 0x00, 0x6C, 0x50, 0xC4, 0xA0, 0x01]
        );
        let mut w = Writer::new();
        w.u64(0x1112_1314_1516_1718);
        w.u64(0x2122_2324_2526_2728);
        assert_eq!(
            w.as_slice(),
            &[
                0x18, 0x17, 0x16, 0x15, 0x14, 0x13, 0x12, 0x11, 0x28, 0x27, 0x26, 0x25, 0x24, 0x23,
                0x22, 0x21
            ]
        );
    }

    /// [F01 §6.2] `vstr` and `fstr<16>` examples, §6.3 the `lp()` example.
    #[test]
    fn string_examples() {
        let v = [0x09, 0x64, 0x6F, 0x63, 0x73, 0x2F, 0x61, 0x2E, 0x6D, 0x64];
        assert_eq!(Reader::new(&v).vstr().unwrap(), "docs/a.md");
        let mut w = Writer::new();
        w.vstr("docs/a.md");
        assert_eq!(w.as_slice(), &v);
        let f = [
            0x0B, 0x00, 0x67, 0x63, 0x20, 0x2D, 0x2D, 0x72, 0x6F, 0x6C, 0x6C, 0x75, 0x70, 0, 0, 0,
        ];
        assert_eq!(Reader::new(&f).fstr(16).unwrap(), "gc --rollup");
        let mut w = Writer::new();
        w.fstr(16, "gc --rollup");
        assert_eq!(w.as_slice(), &f);
        let mut bad = f;
        bad[15] = 1;
        assert!(Reader::new(&bad).fstr(16).is_err());
        let mut out = Vec::new();
        lp(&mut out, b"moirai-file-v1");
        assert_eq!(
            out,
            [
                0x0E, 0, 0, 0, 0x6D, 0x6F, 0x69, 0x72, 0x61, 0x69, 0x2D, 0x66, 0x69, 0x6C, 0x65,
                0x2D, 0x76, 0x31
            ]
        );
    }

    /// [F01 §6.1]: invalid UTF-8 (a surrogate, an overlong form) is refused.
    #[test]
    fn utf8_refusals() {
        assert!(Reader::new(&[3, 0xED, 0xA0, 0x80]).vstr().is_err());
        assert!(Reader::new(&[2, 0xC0, 0xAF]).vstr().is_err());
    }

    /// [F01 §7.5] the fixed 32-byte slot: SHA-1 zero-padded at the end.
    #[test]
    fn oid_slot_rules() {
        let mut s = [0u8; 32];
        s[..20].copy_from_slice(&[7u8; 20]);
        assert_eq!(Reader::new(&s).oid_slot(1).unwrap(), Oid::Sha1([7; 20]));
        s[25] = 1;
        assert!(Reader::new(&s).oid_slot(1).is_err());
        assert!(Reader::new(&s).oid_slot(0).is_err());
        assert!(Reader::new(&s).oid_slot(3).is_err());
        assert_eq!(Reader::new(&[0u8; 32]).oid_slot(0).unwrap(), Oid::None);
    }

    #[test]
    fn hex_round_trip() {
        assert_eq!(hex(&[0x00, 0xAB, 0x2A]), "00ab2a");
        assert_eq!(unhex("00ab2a").unwrap(), vec![0x00, 0xAB, 0x2A]);
        assert!(unhex("00AB").is_none());
    }

    proptest! {
        #[test]
        fn uvar_round_trip(v in any::<u64>()) {
            let b = enc(v);
            let mut r = Reader::new(&b);
            prop_assert_eq!(r.uvar64().unwrap(), v);
            prop_assert!(r.is_empty());
        }

        #[test]
        fn svar_round_trip(i in any::<i64>()) {
            let mut w = Writer::new();
            w.svar(i);
            prop_assert_eq!(Reader::new(w.as_slice()).svar64().unwrap(), i);
        }

        #[test]
        fn uvar_decode_is_canonical(b in proptest::collection::vec(any::<u8>(), 1..12)) {
            let mut r = Reader::new(&b);
            if let Ok(v) = r.uvar64() {
                let e = enc(v);
                prop_assert_eq!(e.as_slice(), &b[..r.pos()]);
            }
        }
    }
}
