//! The frozen bitset ([F09 §4.5]) with its AND/popcount, and the ± list ([F09 §4.6]).
//!
//! A frozen bitset splits the ids into chunks of 65,536 (`hi = #N >> 16`): an 8-byte header (`n_chunks`, reserved),
//! a chunk index of 16-byte entries (`hi`, reserved, `card`, `off`) sorted by `hi`, then each chunk's container at the
//! next multiple of 8 relative to the section's first byte: an **array** of `card` `u16` low halves when
//! `card ≤ 4,096`, else an 8,192-byte **bitmap** (bit `l`, least significant first, [F01 §4.4]). `SecEnt.count` is
//! the total cardinality ([50] F6). A ± list is `n_plus`, `n_minus`, then both `u32` lists, strictly ascending,
//! disjoint, without 0, and never empty.

use super::{SECTION_ALIGN, u32_at};

/// The bits of `#N` that select its chunk: `hi = #N >> 16` ([F09 §4.5]).
pub const CHUNK_SHIFT: u32 = 16;
/// The largest cardinality held as an array ([F09 §4.5]).
pub const ARRAY_MAX: u32 = 4_096;
/// A bitmap container's length ([F09 §4.5]).
pub const BITMAP_LEN: usize = 8_192;
/// The frozen bitset's header ([F09 §4.5]).
pub const FROZEN_HDR_LEN: usize = 8;
/// One chunk-index entry ([F09 §4.5]).
pub const CHUNK_ENT_LEN: usize = 16;

fn check_ids(ids: &[u32], what: &str) -> Result<(), String> {
    if ids.first() == Some(&0) || ids.windows(2).any(|w| w[0] >= w[1]) {
        return Err(format!("{what}: ids must be strictly ascending, none 0"));
    }
    Ok(())
}

/// Appends the frozen bitset of `ids` (strictly ascending, none 0) at the end of `out`, which is the section's first
/// byte; returns its total cardinality, the section's `SecEnt.count`. The encoding is a function of the set.
pub fn encode_frozen(ids: &[u32], out: &mut Vec<u8>) -> Result<u32, String> {
    check_ids(ids, "a frozen bitset")?;
    let base = out.len();
    let mut chunks: Vec<(u16, usize, usize)> = Vec::new();
    let mut i = 0;
    while i < ids.len() {
        let hi = ids[i] >> CHUNK_SHIFT;
        let j = i + ids[i..].partition_point(|&x| x >> CHUNK_SHIFT == hi);
        chunks.push((hi as u16, i, j));
        i = j;
    }
    out.extend_from_slice(&(chunks.len() as u32).to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    let index = out.len();
    out.resize(index + CHUNK_ENT_LEN * chunks.len(), 0);
    for (k, &(hi, a, b)) in chunks.iter().enumerate() {
        let off = (out.len() - base).next_multiple_of(SECTION_ALIGN);
        out.resize(base + off, 0);
        let card = (b - a) as u32;
        if card <= ARRAY_MAX {
            for &x in &ids[a..b] {
                out.extend_from_slice(&((x & 0xFFFF) as u16).to_le_bytes());
            }
        } else {
            let start = out.len();
            out.resize(start + BITMAP_LEN, 0);
            for &x in &ids[a..b] {
                let l = (x & 0xFFFF) as usize;
                out[start + l / 8] |= 1 << (l % 8);
            }
        }
        let e = index + CHUNK_ENT_LEN * k;
        out[e..e + 2].copy_from_slice(&hi.to_le_bytes());
        out[e + 4..e + 8].copy_from_slice(&card.to_le_bytes());
        out[e + 8..e + 16].copy_from_slice(&(off as u64).to_le_bytes());
    }
    Ok(ids.len() as u32)
}

/// One chunk's container.
#[derive(Copy, Clone, Debug)]
pub enum Container<'a> {
    /// `card` low halves, strictly ascending.
    Array(&'a [u8]),
    /// 8,192 bytes, bit `l` set iff `(hi << 16) | l` is a member.
    Bitmap(&'a [u8]),
}

/// A frozen bitset over its section's bytes.
#[derive(Clone, Debug)]
pub struct Frozen<'a> {
    bytes: &'a [u8],
    chunks: Vec<(u16, u32, Container<'a>)>,
}

impl<'a> Frozen<'a> {
    /// Parses a frozen bitset: the header, the index sorted by `hi`, each container's place and length.
    pub fn parse(bytes: &'a [u8]) -> Result<Frozen<'a>, String> {
        if bytes.len() < FROZEN_HDR_LEN || u32_at(bytes, 1) != 0 {
            return Err("a frozen bitset without its header".into());
        }
        let n = u32_at(bytes, 0) as usize;
        let mut end = FROZEN_HDR_LEN + CHUNK_ENT_LEN * n;
        if end > bytes.len() {
            return Err("a frozen bitset's chunk index is cut".into());
        }
        let mut chunks = Vec::with_capacity(n);
        for k in 0..n {
            let e = &bytes[FROZEN_HDR_LEN + CHUNK_ENT_LEN * k..];
            let hi = u16::from_le_bytes([e[0], e[1]]);
            let card = u32_at(e, 1);
            let off = u64::from_le_bytes(e[8..16].try_into().expect("8 bytes")) as usize;
            if chunks
                .last()
                .is_some_and(|c: &(u16, u32, Container)| c.0 >= hi)
                || e[2..4] != [0, 0]
                || !(1..=1 << CHUNK_SHIFT).contains(&card)
                || off != end.next_multiple_of(SECTION_ALIGN)
            {
                return Err(format!("chunk entry {k} breaks the index rules"));
            }
            let len = if card <= ARRAY_MAX {
                2 * card as usize
            } else {
                BITMAP_LEN
            };
            end = off + len;
            if end > bytes.len() {
                return Err(format!("chunk {k}'s container is cut"));
            }
            let c = &bytes[off..end];
            chunks.push((
                hi,
                card,
                if card <= ARRAY_MAX {
                    Container::Array(c)
                } else {
                    Container::Bitmap(c)
                },
            ));
        }
        if end != bytes.len() {
            return Err("bytes after the last container".into());
        }
        Ok(Frozen { bytes, chunks })
    }

    /// The section's bytes.
    pub fn bytes(&self) -> &'a [u8] {
        self.bytes
    }

    /// The chunks: (`hi`, `card`, container).
    pub fn chunks(&self) -> &[(u16, u32, Container<'a>)] {
        &self.chunks
    }

    /// The total cardinality, from the chunk index alone.
    pub fn card(&self) -> u64 {
        self.chunks.iter().map(|c| u64::from(c.1)).sum()
    }

    /// Every member, ascending.
    pub fn members(&self) -> Vec<u32> {
        let mut out = Vec::with_capacity(self.card() as usize);
        for &(hi, _, c) in &self.chunks {
            let top = u32::from(hi) << CHUNK_SHIFT;
            match c {
                Container::Array(a) => {
                    out.extend(
                        a.chunks(2)
                            .map(|l| top | u32::from(u16::from_le_bytes([l[0], l[1]]))),
                    );
                }
                Container::Bitmap(m) => {
                    for (i, &byte) in m.iter().enumerate() {
                        for bit in 0..8 {
                            if byte >> bit & 1 == 1 {
                                out.push(top | (8 * i + bit) as u32);
                            }
                        }
                    }
                }
            }
        }
        out
    }
}

fn word(m: &[u8], w: usize) -> u64 {
    u64::from_le_bytes(m[8 * w..8 * w + 8].try_into().expect("8 bytes"))
}

fn bit(m: &[u8], l: usize) -> bool {
    m[l / 8] >> (l % 8) & 1 == 1
}

fn lows(a: &[u8]) -> impl Iterator<Item = usize> + '_ {
    a.chunks(2)
        .map(|l| usize::from(u16::from_le_bytes([l[0], l[1]])))
}

/// `|a ∧ b|` ([50] F6 "bitset AND/popcount"): the chunk indexes merged by `hi`; two bitmaps by the popcount of their
/// 1,024 ANDed 64-bit words, an array against a bitmap by testing each low half, two arrays by a merge.
pub fn and_popcount(a: &Frozen<'_>, b: &Frozen<'_>) -> u64 {
    let (mut i, mut j, mut n) = (0, 0, 0u64);
    while i < a.chunks.len() && j < b.chunks.len() {
        let (ha, _, ca) = a.chunks[i];
        let (hb, _, cb) = b.chunks[j];
        if ha < hb {
            i += 1;
            continue;
        }
        if hb < ha {
            j += 1;
            continue;
        }
        n += match (ca, cb) {
            (Container::Bitmap(x), Container::Bitmap(y)) => (0..BITMAP_LEN / 8)
                .map(|w| u64::from((word(x, w) & word(y, w)).count_ones()))
                .sum(),
            (Container::Array(x), Container::Bitmap(m))
            | (Container::Bitmap(m), Container::Array(x)) => {
                lows(x).filter(|&l| bit(m, l)).count() as u64
            }
            (Container::Array(x), Container::Array(y)) => {
                let (mut p, mut q) = (lows(x).peekable(), lows(y).peekable());
                let mut c = 0;
                while let (Some(&u), Some(&v)) = (p.peek(), q.peek()) {
                    if u < v {
                        p.next();
                    } else if v < u {
                        q.next();
                    } else {
                        c += 1;
                        p.next();
                        q.next();
                    }
                }
                c
            }
        };
        i += 1;
        j += 1;
    }
    n
}

/// Appends a ± list ([F09 §4.6]); returns `n_plus + n_minus`, its `SecEnt.count`. Refuses
/// unsorted, overlapping or 0 ids and an empty list, which is never written.
pub fn encode_plus_minus(plus: &[u32], minus: &[u32], out: &mut Vec<u8>) -> Result<u32, String> {
    check_ids(plus, "a ± list's plus")?;
    check_ids(minus, "a ± list's minus")?;
    if plus.is_empty() && minus.is_empty() {
        return Err("a ± list with count 0 is never written".into());
    }
    if plus.iter().any(|x| minus.binary_search(x).is_ok()) {
        return Err("a ± list's plus and minus overlap".into());
    }
    out.extend_from_slice(&(plus.len() as u32).to_le_bytes());
    out.extend_from_slice(&(minus.len() as u32).to_le_bytes());
    super::put_u32s(out, plus);
    super::put_u32s(out, minus);
    Ok((plus.len() + minus.len()) as u32)
}

/// Decodes a ± list.
pub fn decode_plus_minus(b: &[u8]) -> Result<(Vec<u32>, Vec<u32>), String> {
    if b.len() < 8 {
        return Err("a ± list without its header".into());
    }
    let (p, m) = (u32_at(b, 0) as usize, u32_at(b, 1) as usize);
    if b.len() != 8 + 4 * (p + m) {
        return Err(format!("a ± list of {p} + {m} ids is {} bytes", b.len()));
    }
    let plus: Vec<u32> = (0..p).map(|i| u32_at(b, 2 + i)).collect();
    let minus: Vec<u32> = (0..m).map(|i| u32_at(b, 2 + p + i)).collect();
    Ok((plus, minus))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// The frozen bitset of `ids`.
    pub(crate) fn frozen(ids: &[u32]) -> Vec<u8> {
        let mut v = Vec::new();
        encode_frozen(ids, &mut v).unwrap();
        v
    }

    /// [F09 §4.5]'s length of a frozen bitset of `ids`, from the rules alone: the header, 16 bytes per chunk, then per
    /// chunk the next multiple of 8 and 2·card bytes (card ≤ 4,096) or 8,192.
    pub(crate) fn frozen_len(ids: &[u32]) -> usize {
        let mut cards: Vec<usize> = Vec::new();
        let mut last = None;
        for &x in ids {
            if last != Some(x >> 16) {
                cards.push(0);
                last = Some(x >> 16);
            }
            *cards.last_mut().unwrap() += 1;
        }
        let mut len = 8 + 16 * cards.len();
        for c in cards {
            len = len.next_multiple_of(8) + if c <= 4_096 { 2 * c } else { 8_192 };
        }
        len
    }

    /// Container by `card` alone ([F09 §4.5]): 4,096 ids in chunk 0 are an array, 4,097 in chunk 1 a
    /// bitmap, 1 id in chunk 3 an array, each at the next multiple of 8 relative to the section; the index carries
    /// `hi`, `card` and `off`; the count is the total; members read back; 0 and unsorted ids are refused.
    #[test]
    fn frozen_bitset_containers_follow_card() {
        let mut ids: Vec<u32> = (1..=4_096).collect();
        ids.extend((0..4_097).map(|i| (1 << 16) + 3 * i));
        ids.push((3 << 16) + 7);
        let mut out = vec![0xEE; 5];
        let count = encode_frozen(&ids, &mut out).unwrap();
        assert_eq!(count, 4_096 + 4_097 + 1);
        let b = &out[5..];
        assert_eq!(b.len(), frozen_len(&ids));
        assert_eq!(&b[0..8], &[3, 0, 0, 0, 0, 0, 0, 0], "n_chunks 3, reserved");
        let entry = |k: usize| {
            let e = &b[8 + 16 * k..24 + 16 * k];
            (
                u16::from_le_bytes([e[0], e[1]]),
                u32::from_le_bytes(e[4..8].try_into().unwrap()),
                u64::from_le_bytes(e[8..16].try_into().unwrap()),
            )
        };
        let first = 8 + 3 * 16;
        assert_eq!(entry(0), (0, 4_096, first as u64));
        assert_eq!(entry(1), (1, 4_097, (first + 8_192) as u64));
        assert_eq!(entry(2), (3, 1, (first + 2 * 8_192) as u64));
        assert_eq!(b.len(), first + 2 * 8_192 + 2);
        assert_eq!(&b[first..first + 4], &[1, 0, 2, 0], "array: low halves");
        let m = &b[first + 8_192..first + 2 * 8_192];
        assert_eq!(m[0], 0b0100_1001, "bitmap: bits 0, 3, 6");
        assert_eq!(&b[first + 2 * 8_192..], &[7, 0]);
        let f = Frozen::parse(b).unwrap();
        assert_eq!(f.card(), 8_194);
        assert!(matches!(f.chunks()[0].2, Container::Array(_)));
        assert!(matches!(f.chunks()[1].2, Container::Bitmap(_)));
        assert_eq!(f.members(), ids);
        // An unaligned container start is padded with zeros: 3 ids then a chunk.
        let small = frozen(&[1, 2, 3, (2 << 16) | 5]);
        assert_eq!(small.len(), frozen_len(&[1, 2, 3, (2 << 16) | 5]));
        assert_eq!(&small[8 + 32 + 6..8 + 32 + 8], &[0, 0]);
        assert_eq!(
            Frozen::parse(&small).unwrap().members(),
            [1, 2, 3, (2 << 16) | 5]
        );
        assert!(encode_frozen(&[0, 1], &mut Vec::new()).is_err());
        assert!(encode_frozen(&[2, 1], &mut Vec::new()).is_err());
        assert_eq!(frozen(&[]), [0; 8], "the empty set: no chunk");
    }

    /// Ids from a seed over three chunks, each chunk empty, sparse (an array) or dense (a bitmap).
    fn draw(seed: u64) -> Vec<u32> {
        let mut s = seed;
        let mut next = || {
            s = s.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = s;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^ (z >> 31)
        };
        let mut ids = Vec::new();
        for hi in 0..3u32 {
            let every = match next() % 3 {
                0 => continue,
                1 => 40 + next() % 60,
                _ => 2 + next() % 6,
            };
            let phase = next() % every;
            ids.extend(
                (phase..1u64 << 16)
                    .step_by(every as usize)
                    .map(|l| (hi << 16) | l as u32)
                    .filter(|&x| x != 0),
            );
        }
        ids
    }

    fn naive_and(a: &[u32], b: &[u32]) -> u64 {
        let (mut i, mut j, mut n) = (0, 0, 0);
        while i < a.len() && j < b.len() {
            match a[i].cmp(&b[j]) {
                std::cmp::Ordering::Less => i += 1,
                std::cmp::Ordering::Greater => j += 1,
                std::cmp::Ordering::Equal => {
                    n += 1;
                    i += 1;
                    j += 1;
                }
            }
        }
        n
    }

    proptest::proptest! {
        #![proptest_config(crate::tail::enc::tests::config())]

        /// AND/popcount equals the naive count of common ids, over every pair of container kinds, and each set's
        /// `card` equals its length.
        #[test]
        fn and_popcount_equals_naive_count(s in proptest::prelude::any::<u64>(), t in proptest::prelude::any::<u64>()) {
            let (a, b) = (draw(s), draw(t));
            let (fa, fb) = (frozen(&a), frozen(&b));
            let (pa, pb) = (Frozen::parse(&fa).unwrap(), Frozen::parse(&fb).unwrap());
            proptest::prop_assert_eq!(and_popcount(&pa, &pb), naive_and(&a, &b));
            proptest::prop_assert_eq!(and_popcount(&pa, &pa), a.len() as u64);
            proptest::prop_assert_eq!(pa.card(), a.len() as u64);
            proptest::prop_assert_eq!(fa.len(), frozen_len(&a));
        }
    }

    /// A ± list ([F09 §4.6]): `n_plus`, `n_minus`, then both lists; it reads back; unsorted, overlapping, 0 and empty
    /// lists are refused.
    #[test]
    fn plus_minus_round_trips() {
        let mut out = Vec::new();
        assert_eq!(encode_plus_minus(&[3, 70_000], &[5], &mut out), Ok(3));
        assert_eq!(
            out,
            [
                2, 0, 0, 0, 1, 0, 0, 0, 3, 0, 0, 0, 0x70, 0x11, 1, 0, 5, 0, 0, 0
            ]
        );
        assert_eq!(decode_plus_minus(&out), Ok((vec![3, 70_000], vec![5])));
        assert!(decode_plus_minus(&out[..19]).is_err());
        for (p, m) in [
            (&[4u32, 3][..], &[][..]),
            (&[3], &[3]),
            (&[0], &[]),
            (&[], &[]),
        ] {
            assert!(
                encode_plus_minus(p, m, &mut Vec::new()).is_err(),
                "{p:?} {m:?}"
            );
        }
    }
}
