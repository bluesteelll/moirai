//! XXH3-64 with seed 0 and the default secret: the checksum of the swap intent file ([OS/fs §4.9.3]).
//!
//! `moirai-os` may depend on `moirai-vfs`, `windows-sys` and `blake3` only (PLAN §2.2), so it carries its own copy of
//! the algorithm, the same as the simulator's (XXH3 as specified by the xxHash project, version 0.8: the 0–16, 17–128
//! and 129–240-byte paths and the striped long path with 64-byte stripes, 16 stripes per 1 KiB block). The unit tests
//! pin known answers of the reference implementation (the `xxhash-rust` crate of the workspace) for every length class.

const P32_1: u64 = 0x9E37_79B1;
const P32_2: u64 = 0x85EB_CA77;
const P32_3: u64 = 0xC2B2_AE3D;
const P64_1: u64 = 0x9E37_79B1_85EB_CA87;
const P64_2: u64 = 0xC2B2_AE3D_27D4_EB4F;
const P64_3: u64 = 0x1656_67B1_9E37_79F9;
const P64_4: u64 = 0x85EB_CA77_C2B2_AE63;
const P64_5: u64 = 0x27D4_EB2F_1656_67C5;
const PRIME_MX1: u64 = 0x1656_6791_9E37_79F9;
const PRIME_MX2: u64 = 0x9FB2_1C65_1E98_DF25;

/// The default 192-byte secret.
const SECRET: [u8; 192] = [
    0xb8, 0xfe, 0x6c, 0x39, 0x23, 0xa4, 0x4b, 0xbe, 0x7c, 0x01, 0x81, 0x2c, 0xf7, 0x21, 0xad, 0x1c,
    0xde, 0xd4, 0x6d, 0xe9, 0x83, 0x90, 0x97, 0xdb, 0x72, 0x40, 0xa4, 0xa4, 0xb7, 0xb3, 0x67, 0x1f,
    0xcb, 0x79, 0xe6, 0x4e, 0xcc, 0xc0, 0xe5, 0x78, 0x82, 0x5a, 0xd0, 0x7d, 0xcc, 0xff, 0x72, 0x21,
    0xb8, 0x08, 0x46, 0x74, 0xf7, 0x43, 0x24, 0x8e, 0xe0, 0x35, 0x90, 0xe6, 0x81, 0x3a, 0x26, 0x4c,
    0x3c, 0x28, 0x52, 0xbb, 0x91, 0xc3, 0x00, 0xcb, 0x88, 0xd0, 0x65, 0x8b, 0x1b, 0x53, 0x2e, 0xa3,
    0x71, 0x64, 0x48, 0x97, 0xa2, 0x0d, 0xf9, 0x4e, 0x38, 0x19, 0xef, 0x46, 0xa9, 0xde, 0xac, 0xd8,
    0xa8, 0xfa, 0x76, 0x3f, 0xe3, 0x9c, 0x34, 0x3f, 0xf9, 0xdc, 0xbb, 0xc7, 0xc7, 0x0b, 0x4f, 0x1d,
    0x8a, 0x51, 0xe0, 0x4b, 0xcd, 0xb4, 0x59, 0x31, 0xc8, 0x9f, 0x7e, 0xc9, 0xd9, 0x78, 0x73, 0x64,
    0xea, 0xc5, 0xac, 0x83, 0x34, 0xd3, 0xeb, 0xc3, 0xc5, 0x81, 0xa0, 0xff, 0xfa, 0x13, 0x63, 0xeb,
    0x17, 0x0d, 0xdd, 0x51, 0xb7, 0xf0, 0xda, 0x49, 0xd3, 0x16, 0x55, 0x26, 0x29, 0xd4, 0x68, 0x9e,
    0x2b, 0x16, 0xbe, 0x58, 0x7d, 0x47, 0xa1, 0xfc, 0x8f, 0xf8, 0xb8, 0xd1, 0x7a, 0xd0, 0x31, 0xce,
    0x45, 0xcb, 0x3a, 0x8f, 0x95, 0x16, 0x04, 0x28, 0xaf, 0xd7, 0xfb, 0xca, 0xbb, 0x4b, 0x40, 0x7e,
];

const STRIPE: usize = 64;
const CONSUME: usize = 8;
const STRIPES_PER_BLOCK: usize = (SECRET.len() - STRIPE) / CONSUME;
const BLOCK: usize = STRIPE * STRIPES_PER_BLOCK;

fn r64(b: &[u8], at: usize) -> u64 {
    let mut w = [0u8; 8];
    w.copy_from_slice(&b[at..at + 8]);
    u64::from_le_bytes(w)
}

fn r32(b: &[u8], at: usize) -> u64 {
    let mut w = [0u8; 4];
    w.copy_from_slice(&b[at..at + 4]);
    u64::from(u32::from_le_bytes(w))
}

fn mul_fold(a: u64, b: u64) -> u64 {
    let p = u128::from(a) * u128::from(b);
    (p as u64) ^ ((p >> 64) as u64)
}

fn avalanche(mut h: u64) -> u64 {
    h ^= h >> 37;
    h = h.wrapping_mul(PRIME_MX1);
    h ^ (h >> 32)
}

fn xxh64_avalanche(mut h: u64) -> u64 {
    h ^= h >> 33;
    h = h.wrapping_mul(P64_2);
    h ^= h >> 29;
    h = h.wrapping_mul(P64_3);
    h ^ (h >> 32)
}

fn rrmxmx(mut h: u64, len: u64) -> u64 {
    h ^= h.rotate_left(49) ^ h.rotate_left(24);
    h = h.wrapping_mul(PRIME_MX2);
    h ^= (h >> 35).wrapping_add(len);
    h = h.wrapping_mul(PRIME_MX2);
    h ^ (h >> 28)
}

fn mix16(input: &[u8], at: usize, sec: usize) -> u64 {
    mul_fold(
        r64(input, at) ^ r64(&SECRET, sec),
        r64(input, at + 8) ^ r64(&SECRET, sec + 8),
    )
}

/// XXH3-64 of `input` with seed 0.
pub(crate) fn xxh3_64(input: &[u8]) -> u64 {
    let len = input.len();
    let l = len as u64;
    match len {
        0 => xxh64_avalanche(r64(&SECRET, 56) ^ r64(&SECRET, 64)),
        1..=3 => {
            let c1 = u64::from(input[0]);
            let c2 = u64::from(input[len >> 1]);
            let c3 = u64::from(input[len - 1]);
            let combined = (c1 << 16) | (c2 << 24) | c3 | (l << 8);
            let flip = r32(&SECRET, 0) ^ r32(&SECRET, 4);
            xxh64_avalanche(combined ^ flip)
        }
        4..=8 => {
            let lo = r32(input, 0);
            let hi = r32(input, len - 4);
            let flip = r64(&SECRET, 8) ^ r64(&SECRET, 16);
            rrmxmx((hi.wrapping_add(lo << 32)) ^ flip, l)
        }
        9..=16 => {
            let lo = r64(input, 0) ^ (r64(&SECRET, 24) ^ r64(&SECRET, 32));
            let hi = r64(input, len - 8) ^ (r64(&SECRET, 40) ^ r64(&SECRET, 48));
            let acc = l
                .wrapping_add(lo.swap_bytes())
                .wrapping_add(hi)
                .wrapping_add(mul_fold(lo, hi));
            avalanche(acc)
        }
        17..=128 => {
            let mut acc = l.wrapping_mul(P64_1);
            if len > 32 {
                if len > 64 {
                    if len > 96 {
                        acc = acc.wrapping_add(mix16(input, 48, 96));
                        acc = acc.wrapping_add(mix16(input, len - 64, 112));
                    }
                    acc = acc.wrapping_add(mix16(input, 32, 64));
                    acc = acc.wrapping_add(mix16(input, len - 48, 80));
                }
                acc = acc.wrapping_add(mix16(input, 16, 32));
                acc = acc.wrapping_add(mix16(input, len - 32, 48));
            }
            acc = acc.wrapping_add(mix16(input, 0, 0));
            acc = acc.wrapping_add(mix16(input, len - 16, 16));
            avalanche(acc)
        }
        129..=240 => {
            let mut acc = l.wrapping_mul(P64_1);
            for i in 0..8 {
                acc = acc.wrapping_add(mix16(input, 16 * i, 16 * i));
            }
            acc = avalanche(acc);
            for i in 8..len / 16 {
                acc = acc.wrapping_add(mix16(input, 16 * i, 16 * (i - 8) + 3));
            }
            acc = acc.wrapping_add(mix16(input, len - 16, 136 - 17));
            avalanche(acc)
        }
        _ => long(input),
    }
}

fn accumulate_512(acc: &mut [u64; 8], input: &[u8], at: usize, sec: usize) {
    for i in 0..8 {
        let v = r64(input, at + 8 * i);
        let k = v ^ r64(&SECRET, sec + 8 * i);
        acc[i ^ 1] = acc[i ^ 1].wrapping_add(v);
        acc[i] = acc[i].wrapping_add((k & 0xFFFF_FFFF).wrapping_mul(k >> 32));
    }
}

fn scramble(acc: &mut [u64; 8]) {
    let sec = SECRET.len() - STRIPE;
    for (i, a) in acc.iter_mut().enumerate() {
        let mut x = *a;
        x ^= x >> 47;
        x ^= r64(&SECRET, sec + 8 * i);
        *a = x.wrapping_mul(P32_1);
    }
}

fn long(input: &[u8]) -> u64 {
    let len = input.len();
    let mut acc = [P32_3, P64_1, P64_2, P64_3, P64_4, P32_2, P64_5, P32_1];
    let blocks = (len - 1) / BLOCK;
    for n in 0..blocks {
        for s in 0..STRIPES_PER_BLOCK {
            accumulate_512(&mut acc, input, n * BLOCK + s * STRIPE, s * CONSUME);
        }
        scramble(&mut acc);
    }
    let stripes = ((len - 1) - BLOCK * blocks) / STRIPE;
    for s in 0..stripes {
        accumulate_512(&mut acc, input, blocks * BLOCK + s * STRIPE, s * CONSUME);
    }
    accumulate_512(&mut acc, input, len - STRIPE, SECRET.len() - STRIPE - 7);
    let mut r = (len as u64).wrapping_mul(P64_1);
    for i in 0..4 {
        r = r.wrapping_add(mul_fold(
            acc[2 * i] ^ r64(&SECRET, 11 + 16 * i),
            acc[2 * i + 1] ^ r64(&SECRET, 11 + 16 * i + 8),
        ));
    }
    avalanche(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Byte `i` of the test input: `(i * 31 + 7) mod 251`.
    fn input(n: usize) -> Vec<u8> {
        (0..n).map(|i| ((i * 31 + 7) % 251) as u8).collect()
    }

    /// Known answers of the reference implementation (`xxhash_rust::xxh3::xxh3_64`) for the inputs above, one or more
    /// lengths per path of the algorithm and at every boundary between paths.
    const KNOWN: &[(usize, u64)] = &[
        (0, 0x2D06_8005_38D3_94C2),
        (1, 0x4C5C_CA45_D0F4_811F),
        (3, 0x15F7_093B_173D_005C),
        (4, 0xDCA0_12F9_5811_B6B9),
        (8, 0xDEC6_A9A4_3575_982E),
        (9, 0x15E5_53B9_7E27_735D),
        (16, 0xA768_3B86_1E58_5AA6),
        (17, 0x637C_1AA9_0769_8945),
        (32, 0xFB0A_38FB_3AF3_2306),
        (33, 0xDBD8_FF66_FDF7_B97E),
        (64, 0xA168_8EF0_A48A_39D4),
        (65, 0x67DC_5B5C_AE64_C652),
        (96, 0xD1A3_5000_054A_80D5),
        (97, 0xD628_CD72_3FED_4570),
        (128, 0x6D0F_64C8_2DDA_AD27),
        (129, 0xEAF3_FC97_C05F_44F3),
        (200, 0xF4B5_4CDC_82F2_0685),
        (240, 0x22F2_8CBB_FAF0_447F),
        (241, 0x0752_5DBC_1490_2C7F),
        (1024, 0xE289_8655_DB7B_C9EE),
        (1025, 0x134C_652B_A3D6_FB9E),
        (2048, 0x63A7_8A59_658D_80F4),
        (4096, 0x04A1_779C_9E7D_DCD7),
        (12360, 0xB9E3_42AD_9B39_ED17),
    ];

    #[test]
    fn matches_the_reference_on_every_length_class() {
        for &(n, want) in KNOWN {
            assert_eq!(xxh3_64(&input(n)), want, "length {n}");
        }
    }
}
