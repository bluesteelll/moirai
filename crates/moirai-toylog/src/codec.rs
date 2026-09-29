//! Little-endian byte helpers, the unsigned LEB128 varints of [F01 §5.2] and the XXH3 forms of [F01 §7.1]: seed 0,
//! seeded (the chain trailer, [F05 §4.2]) and 128-bit (the `HEAD` slot checksum, [F04 §3.1]).
//!
//! Every decoder is total: it returns [`Short`] instead of panicking on a truncated or malformed input.

use xxhash_rust::xxh3::{Xxh3, xxh3_64, xxh3_64_with_seed, xxh3_128};

/// XXH3-64 with seed 0 ([F01 §7.1]).
pub fn hash64(data: &[u8]) -> u64 {
    xxh3_64(data)
}

/// XXH3-64 over `data` with `seed` ([F01 §7.1], the seeded form of the chain trailer, [F05 §4.2]).
pub fn hash64_seeded(data: &[u8], seed: u64) -> u64 {
    xxh3_64_with_seed(data, seed)
}

/// XXH3-64 with seed 0 over the concatenation of `parts`, without copying them ([F05 §3.4]: the header without its
/// checksum field, then the payload).
pub fn hash64_parts(parts: &[&[u8]]) -> u64 {
    let mut h = Xxh3::new();
    for p in parts {
        h.update(p);
    }
    h.digest()
}

/// XXH3-128 with seed 0 ([F04 §3.1] `xxh3_128`), as (`low64`, `high64`).
pub fn hash128(data: &[u8]) -> (u64, u64) {
    let v = xxh3_128(data);
    (v as u64, (v >> 64) as u64)
}

/// A decoder ran off the end of its input or met a malformed field.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Short;

/// A cursor over a byte slice.
#[derive(Clone, Debug)]
pub struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    /// A reader at the start of `b`.
    pub fn new(b: &'a [u8]) -> Reader<'a> {
        Reader { b, at: 0 }
    }

    /// The bytes not read yet.
    pub fn rest(&self) -> usize {
        self.b.len() - self.at
    }

    /// Whether every byte was read ([F05 §8.8]: a payload ends exactly after its last field).
    pub fn done(&self) -> bool {
        self.at == self.b.len()
    }

    /// The next `n` bytes.
    pub fn bytes(&mut self, n: usize) -> Result<&'a [u8], Short> {
        let end = self.at.checked_add(n).ok_or(Short)?;
        let s = self.b.get(self.at..end).ok_or(Short)?;
        self.at = end;
        Ok(s)
    }

    /// A fixed-size array.
    pub fn array<const N: usize>(&mut self) -> Result<[u8; N], Short> {
        let s = self.bytes(N)?;
        let mut a = [0u8; N];
        a.copy_from_slice(s);
        Ok(a)
    }

    /// A `u8`.
    pub fn u8(&mut self) -> Result<u8, Short> {
        Ok(self.bytes(1)?[0])
    }

    /// A little-endian `u16`.
    pub fn u16(&mut self) -> Result<u16, Short> {
        Ok(u16::from_le_bytes(self.array()?))
    }

    /// A little-endian `u32`.
    pub fn u32(&mut self) -> Result<u32, Short> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    /// A little-endian `u64`.
    pub fn u64(&mut self) -> Result<u64, Short> {
        Ok(u64::from_le_bytes(self.array()?))
    }

    /// An unsigned LEB128 varint of at most `max_bits` significant bits in its minimal encoding ([F01 §5.2]).
    pub fn uvar(&mut self, max_bits: u32) -> Result<u64, Short> {
        let mut v: u64 = 0;
        for i in 0..10u32 {
            let byte = self.u8()?;
            let low = u64::from(byte & 0x7F);
            let shift = 7 * i;
            // The tenth byte carries only bit 63.
            if i == 9 && low > 1 {
                return Err(Short);
            }
            v |= low << shift;
            if byte & 0x80 == 0 {
                // Minimal encoding: no trailing zero group (except the single byte 0).
                if byte == 0 && i > 0 {
                    return Err(Short);
                }
                if max_bits < 64 && v >> max_bits != 0 {
                    return Err(Short);
                }
                return Ok(v);
            }
        }
        Err(Short)
    }

    /// A `uvar32`.
    pub fn uvar32(&mut self) -> Result<u32, Short> {
        Ok(self.uvar(32)? as u32)
    }

    /// A length-prefixed byte string (`vbytes`: a `uvar32` length, then the bytes).
    pub fn vbytes(&mut self) -> Result<&'a [u8], Short> {
        let n = self.uvar32()? as usize;
        self.bytes(n)
    }
}

/// An appending encoder.
#[derive(Clone, Debug, Default)]
pub struct Writer {
    /// The bytes written so far.
    pub buf: Vec<u8>,
}

impl Writer {
    /// An empty encoder with room for `n` bytes.
    pub fn with_capacity(n: usize) -> Writer {
        Writer {
            buf: Vec::with_capacity(n),
        }
    }

    /// Appends bytes.
    pub fn bytes(&mut self, b: &[u8]) -> &mut Writer {
        self.buf.extend_from_slice(b);
        self
    }

    /// Appends a `u8`.
    pub fn u8(&mut self, v: u8) -> &mut Writer {
        self.buf.push(v);
        self
    }

    /// Appends a little-endian `u16`.
    pub fn u16(&mut self, v: u16) -> &mut Writer {
        self.bytes(&v.to_le_bytes())
    }

    /// Appends a little-endian `u32`.
    pub fn u32(&mut self, v: u32) -> &mut Writer {
        self.bytes(&v.to_le_bytes())
    }

    /// Appends a little-endian `u64`.
    pub fn u64(&mut self, v: u64) -> &mut Writer {
        self.bytes(&v.to_le_bytes())
    }

    /// Appends an unsigned LEB128 varint in its minimal encoding ([F01 §5.2]).
    pub fn uvar(&mut self, mut v: u64) -> &mut Writer {
        loop {
            let low = (v & 0x7F) as u8;
            v >>= 7;
            if v == 0 {
                self.buf.push(low);
                return self;
            }
            self.buf.push(low | 0x80);
        }
    }

    /// Appends a `vbytes`.
    pub fn vbytes(&mut self, b: &[u8]) -> &mut Writer {
        self.uvar(b.len() as u64);
        self.bytes(b)
    }

    /// Appends `n` zero bytes.
    pub fn zeros(&mut self, n: usize) -> &mut Writer {
        self.buf.resize(self.buf.len() + n, 0);
        self
    }
}

/// The encoded length of `v` as a minimal LEB128 varint.
pub fn uvar_len(v: u64) -> usize {
    let bits = 64 - v.leading_zeros() as usize;
    bits.div_ceil(7).max(1)
}

/// A little-endian `u64` at `at` of `b`, or `None` when `b` is too short.
pub fn u64_at(b: &[u8], at: usize) -> Option<u64> {
    let s = b.get(at..at.checked_add(8)?)?;
    let mut a = [0u8; 8];
    a.copy_from_slice(s);
    Some(u64::from_le_bytes(a))
}

/// A little-endian `u32` at `at` of `b`, or `None` when `b` is too short.
pub fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    let s = b.get(at..at.checked_add(4)?)?;
    let mut a = [0u8; 4];
    a.copy_from_slice(s);
    Some(u32::from_le_bytes(a))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn varints_are_minimal_leb128() {
        let mut w = Writer::default();
        w.uvar(0).uvar(127).uvar(128).uvar(300).uvar(u64::MAX);
        assert_eq!(
            &w.buf[..5],
            &[0x00, 0x7F, 0x80, 0x01, 0xAC],
            "0, 127, 128 and the first byte of 300"
        );
        let mut r = Reader::new(&w.buf);
        assert_eq!(r.uvar(64), Ok(0));
        assert_eq!(r.uvar(64), Ok(127));
        assert_eq!(r.uvar(64), Ok(128));
        assert_eq!(r.uvar(64), Ok(300));
        assert_eq!(r.uvar(64), Ok(u64::MAX));
        assert!(r.done());
        // A non-minimal encoding and an over-long one are refused.
        assert_eq!(Reader::new(&[0x80, 0x00]).uvar(64), Err(Short));
        assert_eq!(Reader::new(&[0xFF; 11]).uvar(64), Err(Short));
        // A value beyond the field's width is refused.
        let mut w = Writer::default();
        w.uvar(1 << 32);
        assert_eq!(Reader::new(&w.buf).uvar32(), Err(Short));
        assert_eq!(uvar_len(0), 1);
        assert_eq!(uvar_len(127), 1);
        assert_eq!(uvar_len(128), 2);
        assert_eq!(uvar_len(u64::MAX), 10);
    }

    #[test]
    fn hashes_match_the_reference_forms() {
        // The seeded form with seed 0 is the plain form, and the part-wise form equals the concatenation.
        let data = b"moirai toy log";
        assert_eq!(hash64_seeded(data, 0), hash64(data));
        assert_eq!(hash64_parts(&[&data[..5], &data[5..]]), hash64(data));
        let (lo, hi) = hash128(data);
        assert_eq!(u128::from(lo) | (u128::from(hi) << 64), xxh3_128(data));
    }

    proptest! {
        #![proptest_config(crate::testing::proptest_config())]

        #[test]
        fn varints_round_trip(v in any::<u64>()) {
            let mut w = Writer::default();
            w.uvar(v);
            prop_assert_eq!(w.buf.len(), uvar_len(v));
            let mut r = Reader::new(&w.buf);
            prop_assert_eq!(r.uvar(64), Ok(v));
            prop_assert!(r.done());
        }

        #[test]
        fn readers_never_panic(b in proptest::collection::vec(any::<u8>(), 0..64)) {
            let mut r = Reader::new(&b);
            while r.rest() > 0 {
                if r.uvar(64).is_err() {
                    break;
                }
            }
            let mut r = Reader::new(&b);
            let _ = r.vbytes();
            let _ = r.u64();
        }
    }
}
