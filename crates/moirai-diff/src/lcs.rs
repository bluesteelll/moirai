//! The length of a longest common subsequence of two token sequences, bit-parallel: the window score of [F20 §6.3]
//! aligns ≤ 2 × `WIN` u16 window hashes ("LCS over ≤ 32 tokens", [40 §2.7]).
//!
//! The row of the LCS matrix is held as a bit vector V over the shorter sequence a, where a zero bit marks a position
//! at which the LCS length grows. For each token y of the longer sequence, with M(y) the positions of y in a:
//! U = V & M(y), V = (V + U) | (V − U), and the length is the number of zero bits of V (H. Hyyrö, "Bit-parallel
//! LCS-length computation revisited", AWOCA 2004). Since U ⊆ V, V − U = V & !U. Cost: O(⌈|a|/64⌉) word operations per
//! token of b, after an O(|a| log |a|) table of match vectors when |a| > 64.

#[cfg(test)]
thread_local! {
    /// Test-only work counter of this thread: tokens of the longer sequence times the words of the bit vector. The
    /// side chosen as the bit vector changes it and nothing else.
    static WORK: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// The length of a longest common subsequence of `a` and `b` ([F20 §6.3]: `LCS`).
#[must_use]
pub fn lcs_len<T: Ord>(a: &[T], b: &[T]) -> usize {
    let (a, b) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    let m = a.len();
    if m == 0 {
        return 0;
    }
    #[cfg(test)]
    WORK.with(|w| w.set(w.get() + b.len() * m.div_ceil(64)));
    if m <= 64 {
        return lcs_word(a, b);
    }
    lcs_blocks(a, b)
}

/// One word: the match vector of each token of b is built by a scan of a, with no allocation.
fn lcs_word<T: Ord>(a: &[T], b: &[T]) -> usize {
    let m = a.len();
    let mut v = !0u64;
    for y in b {
        let mut mask = 0u64;
        for (i, x) in a.iter().enumerate() {
            if x == y {
                mask |= 1 << i;
            }
        }
        let u = v & mask;
        v = v.wrapping_add(u) | (v & !u);
    }
    let low = if m == 64 { !0 } else { (1u64 << m) - 1 };
    (!v & low).count_ones() as usize
}

/// Several words: one match vector per distinct token of a, found by binary search; the addition carries across words.
fn lcs_blocks<T: Ord>(a: &[T], b: &[T]) -> usize {
    let m = a.len();
    let words = m.div_ceil(64);
    let mut order: Vec<usize> = (0..m).collect();
    order.sort_by(|&i, &j| a[i].cmp(&a[j]));
    // `keys[r]` is a position of the r-th distinct token; `masks[r * words..]` its match vector.
    let mut keys: Vec<usize> = Vec::new();
    let mut masks: Vec<u64> = Vec::new();
    for &i in &order {
        if keys.last().is_none_or(|&k| a[k] != a[i]) {
            keys.push(i);
            masks.resize(masks.len() + words, 0);
        }
        let r = keys.len() - 1;
        masks[r * words + i / 64] |= 1 << (i % 64);
    }
    let mut v = vec![!0u64; words];
    for y in b {
        let Ok(r) = keys.binary_search_by(|&k| a[k].cmp(y)) else {
            continue;
        };
        let row = &masks[r * words..(r + 1) * words];
        let mut carry = false;
        for (vw, &mw) in v.iter_mut().zip(row) {
            let u = *vw & mw;
            let (s1, c1) = vw.overflowing_add(u);
            let (s2, c2) = s1.overflowing_add(u64::from(carry));
            carry = c1 | c2;
            *vw = s2 | (*vw & !u);
        }
    }
    let full = m / 64;
    let rem = m % 64;
    let mut zeros: u32 = v[..full].iter().map(|w| (!w).count_ones()).sum();
    if rem > 0 {
        zeros += (!v[full] & ((1u64 << rem) - 1)).count_ones();
    }
    zeros as usize
}

#[cfg(test)]
mod tests {
    use super::{WORK, lcs_len};
    use proptest::prelude::*;

    /// The textbook dynamic program.
    fn naive(a: &[u16], b: &[u16]) -> usize {
        let mut prev = vec![0usize; b.len() + 1];
        for x in a {
            let mut cur = vec![0usize; b.len() + 1];
            for (j, y) in b.iter().enumerate() {
                cur[j + 1] = if x == y {
                    prev[j] + 1
                } else {
                    cur[j].max(prev[j + 1])
                };
            }
            prev = cur;
        }
        prev[b.len()]
    }

    #[test]
    fn known_values() {
        assert_eq!(lcs_len::<u16>(&[], &[]), 0);
        assert_eq!(lcs_len(&[1u16, 2, 3], &[]), 0);
        assert_eq!(lcs_len(&[1u16, 2, 3], &[1, 2, 3]), 3);
        assert_eq!(lcs_len(&[1u16, 2, 3, 4], &[2, 4, 1, 3]), 2);
        assert_eq!(lcs_len(b"ABCBDAB", b"BDCABA"), 4);
        assert_eq!(lcs_len(&[7u16; 10], &[7u16; 4]), 4);
    }

    #[test]
    fn word_boundaries() {
        for m in [63usize, 64, 65, 127, 128, 129, 200] {
            let a: Vec<u16> = (0..m as u16).map(|x| x % 5).collect();
            let b: Vec<u16> = (0..(m as u16 + 7)).map(|x| (x * 3) % 5).collect();
            assert_eq!(lcs_len(&a, &b), naive(&a, &b), "m = {m}");
            assert_eq!(lcs_len(&b, &a), naive(&a, &b), "m = {m}, swapped");
            assert_eq!(lcs_len(&a, &a), m);
        }
    }

    #[test]
    fn the_shorter_side_is_the_bit_vector() {
        // 70 tokens against 10: the 10 are the bit vector, one word per token of the 70. The other way would take
        // two words per token of the 10, 20 in all.
        let a: Vec<u16> = (0..70).map(|x| x % 7).collect();
        let b: Vec<u16> = (0..10).collect();
        for (x, y) in [(&a, &b), (&b, &a)] {
            WORK.with(|w| w.set(0));
            assert_eq!(lcs_len(x, y), naive(&a, &b));
            assert_eq!(WORK.with(std::cell::Cell::get), 70);
        }
    }

    #[test]
    fn carry_crosses_a_full_word() {
        // One token y at positions 63 and 128 of a (the bit side, as the shorter sequence): the addition carries out
        // of word 0 through the all-ones word 1 into word 2, where it clears the second candidate. Tokens absent
        // from a leave the vector unchanged.
        let mut a = vec![0u16; 130];
        a[63] = 1;
        a[128] = 1;
        let mut b = vec![9u16; 200];
        b.push(1);
        assert_eq!(lcs_len(&a, &b), 1);
        b.push(1);
        assert_eq!(lcs_len(&a, &b), 2);
        assert_eq!(lcs_len(&a, &b), naive(&a, &b));
    }

    proptest! {
        #![proptest_config(crate::test_config(256))]

        #[test]
        fn equals_dynamic_program(
            a in prop::collection::vec(0u16..6, 0..160),
            b in prop::collection::vec(0u16..6, 0..160),
        ) {
            prop_assert_eq!(lcs_len(&a, &b), naive(&a, &b));
        }

        #[test]
        fn wide_alphabet(
            a in prop::collection::vec(any::<u16>().prop_map(|x| x % 300), 0..140),
            b in prop::collection::vec(any::<u16>().prop_map(|x| x % 300), 0..140),
        ) {
            prop_assert_eq!(lcs_len(&a, &b), naive(&a, &b));
        }
    }
}
