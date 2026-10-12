//! The integer and string encodings of [F01 §5] and [F01 §6.2] (`docs/spec/format/01-conventions.md:236-377`), and a
//! reader that refuses what [F01 §5.2] refuses.
//!
//! - Fixed widths are little-endian (`01-conventions.md:240-252`).
//! - `uvar` is unsigned LEB128 in its shortest form; the bounds `uvar16`, `uvar32`, `uvar64` have maximum lengths 3,
//!   5 and 10 bytes (`:256-268`). Decoding refuses a non-canonical encoding, one longer than the bound's maximum, a
//!   value above the bound and one cut off by its container.
//! - `svar64` is zigzag then `uvar64` (`:290-295`).
//! - `bool8` is `00` or `01` (`:306`).
//! - `vbytes` is a `uvar32` length then the bytes; `vstr` is a `vbytes` of valid UTF-8 (`:356-358`).

/// Why bytes do not decode: the rule they break, with the offset in the decoded structure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecodeError {
    /// The offset in the decoded buffer.
    pub at: usize,
    /// The rule broken, or the subset feature met.
    pub what: String,
}

impl core::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{} (at byte {})", self.what, self.at)
    }
}

impl std::error::Error for DecodeError {}

/// The declared bound of a varint field ([F01 §5.2], `01-conventions.md:256-259`).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Bound {
    /// `uvar16`: at most 3 bytes, at most 65,535.
    U16,
    /// `uvar32`: at most 5 bytes, at most 2^32 − 1.
    U32,
    /// `uvar64`: at most 10 bytes.
    U64,
}

impl Bound {
    /// The maximum encoded length: 3, 5, 10 (`01-conventions.md:258-259`).
    pub const fn max_len(self) -> usize {
        match self {
            Bound::U16 => 3,
            Bound::U32 => 5,
            Bound::U64 => 10,
        }
    }

    /// The greatest value.
    pub const fn max_value(self) -> u64 {
        match self {
            Bound::U16 => u16::MAX as u64,
            Bound::U32 => u32::MAX as u64,
            Bound::U64 => u64::MAX,
        }
    }
}

/// The encoded length of `v` as a `uvar`.
pub const fn uvar_len(v: u64) -> usize {
    let bits = 64 - (v | 1).leading_zeros() as usize;
    bits.div_ceil(7)
}

/// Appends `v` as a `uvar` (unsigned LEB128, shortest form).
pub fn put_uvar(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let b = (v & 0x7F) as u8;
        v >>= 7;
        if v == 0 {
            out.push(b);
            return;
        }
        out.push(b | 0x80);
    }
}

/// The zigzag map of [F01 §5.3]: `z = (i << 1) XOR (i >> 63)`.
pub const fn zigzag(i: i64) -> u64 {
    ((i << 1) ^ (i >> 63)) as u64
}

/// The inverse of [`zigzag`]: `i = (z >> 1) XOR −(z AND 1)`.
pub const fn unzigzag(z: u64) -> i64 {
    ((z >> 1) as i64) ^ -((z & 1) as i64)
}

/// Appends `i` as an `svar64`.
pub fn put_svar(out: &mut Vec<u8>, i: i64) {
    put_uvar(out, zigzag(i));
}

/// Appends a `u16`, little-endian.
pub fn put_u16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}

/// Appends a `u32`, little-endian.
pub fn put_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

/// Appends a `u64`, little-endian.
pub fn put_u64(out: &mut Vec<u8>, v: u64) {
    out.extend_from_slice(&v.to_le_bytes());
}

/// Appends a `bool8`.
pub fn put_bool8(out: &mut Vec<u8>, b: bool) {
    out.push(u8::from(b));
}

/// Appends a `vbytes`: a `uvar32` length, then the bytes.
pub fn put_vbytes(out: &mut Vec<u8>, b: &[u8]) {
    put_uvar(out, b.len() as u64);
    out.extend_from_slice(b);
}

/// Appends a `vstr`.
pub fn put_vstr(out: &mut Vec<u8>, s: &str) {
    put_vbytes(out, s.as_bytes());
}

/// A cursor over a byte string that refuses every encoding [F01 §5.2] refuses.
#[derive(Clone, Debug)]
pub struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    /// A reader at the start of `buf`.
    pub const fn new(buf: &'a [u8]) -> Reader<'a> {
        Reader { buf, pos: 0 }
    }

    /// The offset of the next byte.
    pub const fn pos(&self) -> usize {
        self.pos
    }

    /// The bytes not read yet.
    pub const fn remaining(&self) -> usize {
        self.buf.len() - self.pos
    }

    /// A decode error at the current offset.
    pub fn err(&self, what: impl Into<String>) -> DecodeError {
        DecodeError {
            at: self.pos,
            what: what.into(),
        }
    }

    /// The next `n` bytes.
    pub fn take(&mut self, n: usize) -> Result<&'a [u8], DecodeError> {
        if self.remaining() < n {
            return Err(self.err(format!(
                "cut off: {n} bytes needed, {} left",
                self.remaining()
            )));
        }
        let s = &self.buf[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }

    /// The next `N` bytes as an array.
    pub fn array<const N: usize>(&mut self) -> Result<[u8; N], DecodeError> {
        let mut a = [0u8; N];
        a.copy_from_slice(self.take(N)?);
        Ok(a)
    }

    /// A `u8`.
    pub fn u8(&mut self) -> Result<u8, DecodeError> {
        Ok(self.take(1)?[0])
    }

    /// A little-endian `u16`.
    pub fn u16(&mut self) -> Result<u16, DecodeError> {
        Ok(u16::from_le_bytes(self.array()?))
    }

    /// A little-endian `u32`.
    pub fn u32(&mut self) -> Result<u32, DecodeError> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    /// A little-endian `u64`.
    pub fn u64(&mut self) -> Result<u64, DecodeError> {
        Ok(u64::from_le_bytes(self.array()?))
    }

    /// A `uvar` of `bound` ([F01 §5.2]): refuses a non-canonical encoding, one longer than the bound's maximum, a
    /// value above the bound and an encoding cut off by the end of the buffer.
    pub fn uvar(&mut self, bound: Bound) -> Result<u64, DecodeError> {
        let start = self.pos;
        let mut v: u64 = 0;
        for i in 0..bound.max_len() {
            let Some(&b) = self.buf.get(self.pos) else {
                self.pos = start;
                return Err(self.err("a varint is cut off by the end of its container"));
            };
            self.pos += 1;
            let group = u64::from(b & 0x7F);
            if i == 9 && group > 1 {
                self.pos = start;
                return Err(self.err("a uvar64 above 2^64 − 1"));
            }
            v |= group << (7 * i);
            if b & 0x80 == 0 {
                if b == 0 && i > 0 {
                    self.pos = start;
                    return Err(self.err("a non-canonical varint (its last byte is 00)"));
                }
                if v > bound.max_value() {
                    self.pos = start;
                    return Err(self.err(format!("a varint above its bound {bound:?}")));
                }
                return Ok(v);
            }
        }
        self.pos = start;
        Err(self.err(format!(
            "a varint longer than {} bytes, the maximum of its bound {bound:?}",
            bound.max_len()
        )))
    }

    /// A `uvar16`.
    pub fn uvar16(&mut self) -> Result<u16, DecodeError> {
        Ok(self.uvar(Bound::U16)? as u16)
    }

    /// A `uvar32`.
    pub fn uvar32(&mut self) -> Result<u32, DecodeError> {
        Ok(self.uvar(Bound::U32)? as u32)
    }

    /// A `uvar64`.
    pub fn uvar64(&mut self) -> Result<u64, DecodeError> {
        self.uvar(Bound::U64)
    }

    /// An `svar64` ([F01 §5.3]).
    pub fn svar64(&mut self) -> Result<i64, DecodeError> {
        Ok(unzigzag(self.uvar64()?))
    }

    /// A `bool8` ([F01 §5.4]).
    pub fn bool8(&mut self) -> Result<bool, DecodeError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            b => {
                self.pos -= 1;
                Err(self.err(format!("a bool8 of {b:#04x}")))
            }
        }
    }

    /// A `vbytes` ([F01 §6.2]).
    pub fn vbytes(&mut self) -> Result<&'a [u8], DecodeError> {
        let n = self.uvar32()? as usize;
        self.take(n)
    }

    /// A `vstr`: a `vbytes` of valid UTF-8 ([F01 §6.1]).
    pub fn vstr(&mut self) -> Result<&'a str, DecodeError> {
        let at = self.pos;
        let b = self.vbytes()?;
        core::str::from_utf8(b).map_err(|_| DecodeError {
            at,
            what: "a vstr that is not valid UTF-8".into(),
        })
    }

    /// Refuses bytes left after the last field ([F05 §8.8], `05-log.md:546-549`).
    pub fn end(&self) -> Result<(), DecodeError> {
        if self.remaining() == 0 {
            Ok(())
        } else {
            Err(self.err(format!(
                "{} bytes left after the last field",
                self.remaining()
            )))
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use proptest::prelude::*;

    /// The configuration of every property of this unit: 256 cases at the PR tier, more in the nightly and exit tiers,
    /// and no failure persistence (a failing seed is printed, never written beside the sources).
    pub(crate) fn config() -> ProptestConfig {
        let cases = match std::env::var("MOIRAI_TEST_TIER").as_deref() {
            Ok("nightly") => 4_096,
            Ok("exit") => 65_536,
            _ => 256,
        };
        ProptestConfig {
            cases,
            failure_persistence: None,
            ..ProptestConfig::default()
        }
    }

    fn uvar(v: u64) -> Vec<u8> {
        let mut out = Vec::new();
        put_uvar(&mut out, v);
        out
    }

    fn svar(i: i64) -> Vec<u8> {
        let mut out = Vec::new();
        put_svar(&mut out, i);
        out
    }

    /// The examples of [F01 §5.2] (`01-conventions.md:270-283`).
    #[test]
    fn uvar_examples_of_f01() {
        let table: [(u64, &[u8]); 10] = [
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
        for (v, bytes) in table {
            assert_eq!(uvar(v), bytes, "{v}");
            assert_eq!(uvar_len(v), bytes.len(), "{v}");
            let mut r = Reader::new(bytes);
            assert_eq!(r.uvar64(), Ok(v));
            assert_eq!(r.remaining(), 0);
        }
    }

    /// [F01 §5.2]'s refusals (`01-conventions.md:260-264`, `:285`), each with its reason, beside the valid form.
    #[test]
    fn uvar_refuses_non_canonical_and_overlong() {
        let refused = |b: &[u8], bound: Bound| Reader::new(b).uvar(bound).unwrap_err().what;
        // `80 00` (0 in two bytes) and `81 00` are refused as non-canonical; `00` and `01` are their valid forms.
        assert!(refused(&[0x80, 0x00], Bound::U64).contains("non-canonical"));
        assert!(refused(&[0x81, 0x00], Bound::U64).contains("non-canonical"));
        assert_eq!(Reader::new(&[0x00]).uvar64(), Ok(0));
        assert_eq!(Reader::new(&[0x01]).uvar64(), Ok(1));
        // Longer than the bound's maximum: 4 bytes for a uvar16, 6 for a uvar32, 11 for a uvar64.
        assert!(refused(&[0x80, 0x80, 0x80, 0x01], Bound::U16).contains("longer than 3"));
        assert!(
            refused(&[0x80, 0x80, 0x80, 0x80, 0x80, 0x01], Bound::U32).contains("longer than 5")
        );
        assert!(refused(&[0x80; 11], Bound::U64).contains("longer than 10"));
        // Above the bound: 65,536 as a uvar16, 2^32 as a uvar32, a tenth byte above 01 for a uvar64.
        assert!(refused(&[0x80, 0x80, 0x04], Bound::U16).contains("above its bound"));
        assert_eq!(Reader::new(&[0xFF, 0xFF, 0x03]).uvar16(), Ok(65_535));
        assert!(refused(&[0x80, 0x80, 0x80, 0x80, 0x10], Bound::U32).contains("above its bound"));
        assert_eq!(
            Reader::new(&[0xFF, 0xFF, 0xFF, 0xFF, 0x0F]).uvar32(),
            Ok(u32::MAX)
        );
        let mut ten = [0xFF; 10];
        ten[9] = 0x02;
        assert!(refused(&ten, Bound::U64).contains("above 2^64"));
        // A tenth byte of 00 is non-canonical.
        ten[9] = 0x00;
        assert!(refused(&ten, Bound::U64).contains("non-canonical"));
        // Cut off by the end of the container.
        assert!(refused(&[0x80], Bound::U64).contains("cut off"));
        assert!(refused(&[], Bound::U32).contains("cut off"));
        // A refusal leaves the cursor where the varint began.
        let mut r = Reader::new(&[0x80, 0x00]);
        assert!(r.uvar64().is_err());
        assert_eq!(r.pos(), 0);
    }

    /// The examples of [F01 §5.3] (`01-conventions.md:293-302`).
    #[test]
    fn svar_examples_of_f01() {
        let table: [(i64, u64, &[u8]); 8] = [
            (0, 0, &[0x00]),
            (-1, 1, &[0x01]),
            (1, 2, &[0x02]),
            (-64, 127, &[0x7F]),
            (64, 128, &[0x80, 0x01]),
            (-65, 129, &[0x81, 0x01]),
            (
                i64::MAX,
                u64::MAX - 1,
                &[0xFE, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x01],
            ),
            (
                i64::MIN,
                u64::MAX,
                &[0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x01],
            ),
        ];
        for (i, z, bytes) in table {
            assert_eq!(zigzag(i), z, "{i}");
            assert_eq!(svar(i), bytes, "{i}");
            assert_eq!(Reader::new(bytes).svar64(), Ok(i));
        }
    }

    /// The `vstr` example of [F01 §6.2] (`01-conventions.md:373`) and the `hlc` example of [F01 §5.7]
    /// (`01-conventions.md:340-341`).
    #[test]
    fn vstr_and_hlc_examples_of_f01() {
        let mut out = Vec::new();
        put_vstr(&mut out, "docs/a.md");
        assert_eq!(
            out,
            [0x09, 0x64, 0x6F, 0x63, 0x73, 0x2F, 0x61, 0x2E, 0x6D, 0x64]
        );
        assert_eq!(Reader::new(&out).vstr(), Ok("docs/a.md"));
        // `unix_ms` = 1,790,000,000,000 with counter 3.
        let hlc = (1_790_000_000_000u64 << 16) | 3;
        assert_eq!(hlc, 0x01A0_C450_6C00_0003);
        let mut out = Vec::new();
        put_u64(&mut out, hlc);
        assert_eq!(out, [0x03, 0x00, 0x00, 0x6C, 0x50, 0xC4, 0xA0, 0x01]);
        assert_eq!(Reader::new(&out).u64(), Ok(hlc));
        // A vstr of invalid UTF-8 and a bool8 other than 00/01 are refused; `01` is true.
        assert!(
            Reader::new(&[0x01, 0xFF])
                .vstr()
                .unwrap_err()
                .what
                .contains("UTF-8")
        );
        assert!(Reader::new(&[0x02]).bool8().is_err());
        assert_eq!(Reader::new(&[0x01]).bool8(), Ok(true));
    }

    proptest! {
        #![proptest_config(config())]

        /// Every value round-trips through its shortest form at every bound that admits it.
        #[test]
        fn uvar_round_trip(v in any::<u64>(), i in any::<i64>()) {
            let b = uvar(v);
            prop_assert_eq!(b.len(), uvar_len(v));
            prop_assert_eq!(Reader::new(&b).uvar64(), Ok(v));
            if let Ok(v32) = u32::try_from(v) {
                prop_assert_eq!(Reader::new(&b).uvar32(), Ok(v32));
            } else {
                prop_assert!(Reader::new(&b).uvar32().is_err());
            }
            prop_assert_eq!(Reader::new(&svar(i)).svar64(), Ok(i));
            prop_assert_eq!(unzigzag(zigzag(i)), i);
        }
    }
}
