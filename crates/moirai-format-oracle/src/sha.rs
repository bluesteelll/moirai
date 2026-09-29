//! SHA-1 and SHA-256 (FIPS 180-4 §6.1, §6.2), the object-id hashes of git destinations ([F01 §7.1], §7.5;
//! [F14 §3.2], §10.1). Hand-written like every other decoder of the oracle: the workspace admits no SHA crate for it
//! (PLAN §2.2 lists `blake3` and `xxhash-rust` only), and the carrier check recomputes every git object id with them.
//!
//! Both hashes stream: [`Sha1`] and [`Sha256`] compress each 64-byte block as it arrives and keep at most one partial
//! block, so hashing a git object needs no copy of its content (FIPS 180-4 §5.1.1 padding is built in the last block).

/// The block buffer and bit counter both hashes share (FIPS 180-4 §5.1.1, §6.1.2, §6.2.2).
#[derive(Clone)]
struct Blocks {
    /// The bytes of the current partial block.
    buf: [u8; 64],
    /// How many bytes of `buf` are filled.
    fill: usize,
    /// Message length in bytes so far.
    len: u64,
}

impl Blocks {
    const fn new() -> Blocks {
        Blocks {
            buf: [0; 64],
            fill: 0,
            len: 0,
        }
    }

    /// Feeds `data`, calling `compress` on every complete block.
    fn update(&mut self, mut data: &[u8], mut compress: impl FnMut(&[u8; 64])) {
        self.len = self.len.wrapping_add(data.len() as u64);
        if self.fill > 0 {
            let take = (64 - self.fill).min(data.len());
            self.buf[self.fill..self.fill + take].copy_from_slice(&data[..take]);
            self.fill += take;
            data = &data[take..];
            if self.fill < 64 {
                return;
            }
            compress(&self.buf);
            self.fill = 0;
        }
        let (blocks, rest) = data.as_chunks::<64>();
        for b in blocks {
            compress(b);
        }
        self.buf[..rest.len()].copy_from_slice(rest);
        self.fill = rest.len();
    }

    /// Pads the message (`0x80`, zeros to 56 mod 64, the bit length big-endian) and compresses the last one or two
    /// blocks.
    fn finish(mut self, mut compress: impl FnMut(&[u8; 64])) {
        let bits = self.len.wrapping_mul(8);
        self.buf[self.fill] = 0x80;
        self.buf[self.fill + 1..].fill(0);
        if self.fill >= 56 {
            compress(&self.buf);
            self.buf = [0; 64];
        }
        self.buf[56..].copy_from_slice(&bits.to_be_bytes());
        compress(&self.buf);
    }
}

fn words_be<const N: usize>(h: [u32; N], out: &mut [u8]) {
    for (o, x) in out.as_chunks_mut::<4>().0.iter_mut().zip(h) {
        o.copy_from_slice(&x.to_be_bytes());
    }
}

/// One SHA-1 compression of `block` into `h` (FIPS 180-4 §6.1.2).
fn sha1_block(h: &mut [u32; 5], block: &[u8; 64]) {
    let mut w = [0u32; 80];
    for (wi, c) in w.iter_mut().zip(block.as_chunks::<4>().0) {
        *wi = u32::from_be_bytes(*c);
    }
    for i in 16..80 {
        w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
    }
    let [mut a, mut b, mut c, mut d, mut e] = *h;
    for (i, wi) in w.iter().enumerate() {
        let (f, k) = match i {
            0..=19 => ((b & c) | (!b & d), 0x5A82_7999),
            20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
            40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
            _ => (b ^ c ^ d, 0xCA62_C1D6),
        };
        let t = a
            .rotate_left(5)
            .wrapping_add(f)
            .wrapping_add(e)
            .wrapping_add(k)
            .wrapping_add(*wi);
        e = d;
        d = c;
        c = b.rotate_left(30);
        b = a;
        a = t;
    }
    for (x, y) in h.iter_mut().zip([a, b, c, d, e]) {
        *x = x.wrapping_add(y);
    }
}

/// A streaming SHA-1 (FIPS 180-4 §6.1).
#[derive(Clone)]
pub struct Sha1 {
    h: [u32; 5],
    blocks: Blocks,
}

impl Default for Sha1 {
    fn default() -> Self {
        Self::new()
    }
}

impl Sha1 {
    /// The initial hash value (FIPS 180-4 §5.3.1).
    pub const fn new() -> Sha1 {
        Sha1 {
            h: [
                0x6745_2301,
                0xEFCD_AB89,
                0x98BA_DCFE,
                0x1032_5476,
                0xC3D2_E1F0,
            ],
            blocks: Blocks::new(),
        }
    }

    /// Feeds `data`.
    pub fn update(&mut self, data: &[u8]) {
        let h = &mut self.h;
        self.blocks.update(data, |b| sha1_block(h, b));
    }

    /// The digest of everything fed.
    pub fn finish(self) -> [u8; 20] {
        let mut h = self.h;
        self.blocks.finish(|b| sha1_block(&mut h, b));
        let mut out = [0u8; 20];
        words_be(h, &mut out);
        out
    }
}

/// SHA-1 of `data` (FIPS 180-4 §6.1).
pub fn sha1(data: &[u8]) -> [u8; 20] {
    let mut s = Sha1::new();
    s.update(data);
    s.finish()
}

/// The SHA-256 round constants (FIPS 180-4 §4.2.2).
const K256: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

/// One SHA-256 compression of `block` into `h` (FIPS 180-4 §6.2.2).
fn sha256_block(h: &mut [u32; 8], block: &[u8; 64]) {
    let mut w = [0u32; 64];
    for (wi, c) in w.iter_mut().zip(block.as_chunks::<4>().0) {
        *wi = u32::from_be_bytes(*c);
    }
    for i in 16..64 {
        let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
        let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
        w[i] = w[i - 16]
            .wrapping_add(s0)
            .wrapping_add(w[i - 7])
            .wrapping_add(s1);
    }
    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = *h;
    for (k, wi) in K256.iter().zip(w) {
        let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let ch = (e & f) ^ (!e & g);
        let t1 = hh
            .wrapping_add(s1)
            .wrapping_add(ch)
            .wrapping_add(*k)
            .wrapping_add(wi);
        let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let maj = (a & b) ^ (a & c) ^ (b & c);
        let t2 = s0.wrapping_add(maj);
        hh = g;
        g = f;
        f = e;
        e = d.wrapping_add(t1);
        d = c;
        c = b;
        b = a;
        a = t1.wrapping_add(t2);
    }
    for (x, y) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
        *x = x.wrapping_add(y);
    }
}

/// A streaming SHA-256 (FIPS 180-4 §6.2).
#[derive(Clone)]
pub struct Sha256 {
    h: [u32; 8],
    blocks: Blocks,
}

impl Default for Sha256 {
    fn default() -> Self {
        Self::new()
    }
}

impl Sha256 {
    /// The initial hash value (FIPS 180-4 §5.3.3).
    pub const fn new() -> Sha256 {
        Sha256 {
            h: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
                0x5be0cd19,
            ],
            blocks: Blocks::new(),
        }
    }

    /// Feeds `data`.
    pub fn update(&mut self, data: &[u8]) {
        let h = &mut self.h;
        self.blocks.update(data, |b| sha256_block(h, b));
    }

    /// The digest of everything fed.
    pub fn finish(self) -> [u8; 32] {
        let mut h = self.h;
        self.blocks.finish(|b| sha256_block(&mut h, b));
        let mut out = [0u8; 32];
        words_be(h, &mut out);
        out
    }
}

/// SHA-256 of `data` (FIPS 180-4 §6.2).
pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut s = Sha256::new();
    s.update(data);
    s.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prim::hex;

    /// The FIPS 180-4 example messages (NIST's "abc", the empty string and the two-block 448-bit message).
    #[test]
    fn nist_vectors() {
        assert_eq!(
            hex(&sha1(b"abc")),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
        assert_eq!(hex(&sha1(b"")), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        let two = b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq";
        assert_eq!(hex(&sha1(two)), "84983e441c3bd26ebaae4aa1f95129e5e54670f1");
        assert_eq!(
            hex(&sha256(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            hex(&sha256(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            hex(&sha256(two)),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    /// Known answers at the padding boundaries: 55 bytes (padding and length fit one block), 56 (the length spills into
    /// a second block), 64 (a whole block, padding alone in the next) and 119 (the 55-byte case one block later). The
    /// messages are `a` repeated n times; the digests were computed with GNU coreutils `sha1sum`/`sha256sum` and
    /// confirmed with Windows CNG (`Get-FileHash`), two implementations independent of this one.
    #[test]
    fn padding_boundaries() {
        let want = [
            (
                55,
                "c1c8bbdc22796e28c0e15163d20899b65621d65a",
                "9f4390f8d30c2dd92ec9f095b65e2b9ae9b0a925a5258e241c9f1e910f734318",
            ),
            (
                56,
                "c2db330f6083854c99d4b5bfb6e8f29f201be699",
                "b35439a4ac6f0948b6d6f9e3c6af0f5f590ce20f1bde7090ef7970686ec6738a",
            ),
            (
                64,
                "0098ba824b5c16427bd7a1122a5a442a25ec644d",
                "ffe054fe7ae0cb6dc65c3af9b61d5209f439851db43d0ba5997337df154668eb",
            ),
            (
                119,
                "ee971065aaa017e0632a8ca6c77bb3bf8b1dfc56",
                "31eba51c313a5c08226adf18d4a359cfdfd8d2e816b13f4af952f7ea6584dcfb",
            ),
        ];
        for (n, s1, s256) in want {
            let m = vec![b'a'; n];
            assert_eq!(hex(&sha1(&m)), s1, "sha1 of {n} bytes");
            assert_eq!(hex(&sha256(&m)), s256, "sha256 of {n} bytes");
        }
        // The git blob id of the empty file.
        assert_eq!(
            hex(&sha1(b"blob 0\0")),
            "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391"
        );
    }

    /// Feeding a message in pieces of every size gives the one-shot digest (the partial-block paths of `update`).
    #[test]
    fn streaming_matches_one_shot() {
        let m: Vec<u8> = (0..300u32).map(|i| (i * 7 + 3) as u8).collect();
        for step in [1usize, 3, 55, 63, 64, 65, 128, 299] {
            let (mut a, mut b) = (Sha1::new(), Sha256::new());
            for c in m.chunks(step) {
                a.update(c);
                b.update(c);
            }
            assert_eq!(a.finish(), sha1(&m), "sha1 step {step}");
            assert_eq!(b.finish(), sha256(&m), "sha256 step {step}");
        }
    }
}
