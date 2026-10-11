//! Natural numbers of any size, for the exact sums and margins of the split test and the classification
//! ([F20 §1.2] "Rationals"): a sum of containments over arbitrary `u64` denominators fits no fixed width.

use core::cmp::Ordering;

/// A natural number: 64-bit limbs, least significant first. Limbs above the most significant non-zero one may be 0,
/// and every comparison ignores them.
#[derive(Clone, Debug)]
pub(crate) struct Nat {
    limbs: Vec<u64>,
}

impl Nat {
    /// The number `v`.
    pub(crate) fn from_u64(v: u64) -> Nat {
        Nat { limbs: vec![v] }
    }

    /// `self := self · m`.
    pub(crate) fn mul_u64(&mut self, m: u64) {
        let mut carry = 0u64;
        for limb in &mut self.limbs {
            // At most (2^64 − 1)^2 + 2^64 − 1 < 2^128.
            let p = u128::from(*limb) * u128::from(m) + u128::from(carry);
            *limb = p as u64;
            carry = (p >> 64) as u64;
        }
        if carry != 0 {
            self.limbs.push(carry);
        }
    }

    /// `self := self + other`.
    pub(crate) fn add(&mut self, other: &Nat) {
        if self.limbs.len() < other.limbs.len() {
            self.limbs.resize(other.limbs.len(), 0);
        }
        let mut carry = false;
        for (i, limb) in self.limbs.iter_mut().enumerate() {
            let o = other.limbs.get(i).copied().unwrap_or(0);
            let (s, c1) = limb.overflowing_add(o);
            let (s, c2) = s.overflowing_add(u64::from(carry));
            *limb = s;
            carry = c1 || c2;
        }
        if carry {
            self.limbs.push(1);
        }
    }

    /// `(⌊self / d⌋, self mod d)` for `d > 0`.
    pub(crate) fn div_rem_u64(&self, d: u64) -> (Nat, u64) {
        debug_assert!(d > 0, "a division by 0");
        let d = u128::from(d);
        let mut q = vec![0u64; self.limbs.len()];
        let mut rem = 0u128;
        for (qi, &limb) in q.iter_mut().zip(&self.limbs).rev() {
            // `rem < d ≤ 2^64 − 1`, so `cur < 2^128` and the quotient digit fits a limb.
            let cur = (rem << 64) | u128::from(limb);
            *qi = (cur / d) as u64;
            rem = cur % d;
        }
        (Nat { limbs: q }, rem as u64)
    }

    /// The limb at `i`, 0 past the end.
    fn limb(&self, i: usize) -> u64 {
        self.limbs.get(i).copied().unwrap_or(0)
    }
}

impl PartialEq for Nat {
    fn eq(&self, other: &Nat) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Nat {}

impl PartialOrd for Nat {
    fn partial_cmp(&self, other: &Nat) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Nat {
    fn cmp(&self, other: &Nat) -> Ordering {
        let n = self.limbs.len().max(other.limbs.len());
        for i in (0..n).rev() {
            match self.limb(i).cmp(&other.limb(i)) {
                Ordering::Equal => {}
                o => return o,
            }
        }
        Ordering::Equal
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// The value of `n`, which must fit `u128` (leading zero limbs allowed).
    fn to_u128(n: &Nat) -> u128 {
        assert!(n.limbs.iter().skip(2).all(|&l| l == 0), "{n:?}");
        u128::from(n.limb(0)) | (u128::from(n.limb(1)) << 64)
    }

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

    /// A `u64` that is often 0, 1, near `u64::MAX` or a power of two, so that carries and zero limbs are common.
    fn word() -> impl Strategy<Value = u64> {
        prop_oneof![
            any::<u64>(),
            Just(0u64),
            Just(1u64),
            (0u64..4).prop_map(|k| u64::MAX - k),
            (0u32..64).prop_map(|k| 1u64 << k),
        ]
    }

    proptest! {
        #![proptest_config(test_config(256))]

        /// `a · m + b`, its quotient and remainder by `d`, and its order against `c`, all as `u128` computes them:
        /// `a · m + b ≤ (2^64 − 1)^2 + 2^64 − 1 < 2^128`.
        #[test]
        fn products_and_sums_agree_with_u128(
            a in word(),
            m in word(),
            b in word(),
            d in word().prop_filter("a positive divisor", |&d| d > 0),
            c_hi in word(),
            c_lo in word(),
        ) {
            let want = u128::from(a) * u128::from(m) + u128::from(b);
            let mut n = Nat::from_u64(a);
            n.mul_u64(m);
            n.add(&Nat::from_u64(b));
            prop_assert_eq!(to_u128(&n), want);

            let (q, r) = n.div_rem_u64(d);
            prop_assert_eq!(to_u128(&q), want / u128::from(d));
            prop_assert_eq!(u128::from(r), want % u128::from(d));

            let c = (u128::from(c_hi) << 64) | u128::from(c_lo);
            let mut nc = Nat::from_u64(c_hi);
            nc.mul_u64(1 << 32);
            nc.mul_u64(1 << 32);
            nc.add(&Nat::from_u64(c_lo));
            prop_assert_eq!(to_u128(&nc), c);
            prop_assert_eq!(n.cmp(&nc), want.cmp(&c));
            prop_assert_eq!(n == nc, want == c);
        }
    }

    /// Products and sums that carry into a new limb, and through several limbs.
    #[test]
    fn carries_cross_limbs() {
        // (2^64 − 1)^2 = 2^128 − 2^65 + 1: limbs [1, 2^64 − 2].
        let mut n = Nat::from_u64(u64::MAX);
        n.mul_u64(u64::MAX);
        assert_eq!(n.limbs, [1, u64::MAX - 1]);
        // (2^64 − 1)^3 = (2^64 − 3) · 2^128 + 2 · 2^64 + (2^64 − 1).
        n.mul_u64(u64::MAX);
        assert_eq!(n.limbs, [u64::MAX, 2, u64::MAX - 2]);
        // Its quotient by 2^64 − 1 is (2^64 − 1)^2 again, with no remainder.
        let (q, r) = n.div_rem_u64(u64::MAX);
        assert_eq!(r, 0);
        assert_eq!(to_u128(&q), u128::from(u64::MAX) * u128::from(u64::MAX));

        // (2^64 − 1) + 1 = 2^64: limbs [0, 1].
        let mut s = Nat::from_u64(u64::MAX);
        s.add(&Nat::from_u64(1));
        assert_eq!(s.limbs, [0, 1]);
        // (2^128 − 1) + 1 carries through two limbs into a third.
        let mut s = Nat {
            limbs: vec![u64::MAX, u64::MAX],
        };
        s.add(&Nat::from_u64(1));
        assert_eq!(s.limbs, [0, 0, 1]);
        // A short number plus a long one.
        let mut s = Nat::from_u64(1);
        s.add(&Nat {
            limbs: vec![u64::MAX, u64::MAX, 7],
        });
        assert_eq!(s.limbs, [0, 0, 8]);
        // A carry out of the low limb of a product: (2^64 − 1) · 2 = 2^65 − 2.
        let mut p = Nat::from_u64(u64::MAX);
        p.mul_u64(2);
        assert_eq!(p.limbs, [u64::MAX - 1, 1]);
        // The remainder crosses limbs: (2^64 + 5) mod 7 = (2^64 mod 7 + 5) mod 7 = (2 + 5) mod 7 = 0.
        let (q, r) = Nat { limbs: vec![5, 1] }.div_rem_u64(7);
        assert_eq!(r, 0);
        assert_eq!(to_u128(&q), ((1u128 << 64) + 5) / 7);
    }

    /// Equal values compare equal whatever their number of limbs; a zero high limb never makes a number larger.
    #[test]
    fn compare_ignores_leading_zero_limbs() {
        let short = Nat::from_u64(5);
        let long = Nat {
            limbs: vec![5, 0, 0],
        };
        assert_eq!(short, long);
        assert_eq!(short.cmp(&long), Ordering::Equal);
        assert_eq!(long.cmp(&short), Ordering::Equal);
        assert!(Nat::from_u64(6) > long);
        assert!(long < Nat::from_u64(6));
        let zero = Nat { limbs: vec![0, 0] };
        assert_eq!(zero, Nat::from_u64(0));
        // A longer number with a larger value still compares larger, and a shorter one with a larger value too.
        assert!(Nat { limbs: vec![0, 1] } > Nat::from_u64(u64::MAX));
        assert!(
            Nat::from_u64(9)
                > Nat {
                    limbs: vec![8, 0, 0, 0]
                }
        );
    }
}
