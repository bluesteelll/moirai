//! Exact scores of the anchor cascade ([F20 §1.2] "Rationals"): every score, ratio and threshold is an exact
//! non-negative rational, compared by cross-multiplication in integer arithmetic; no floating point.
//!
//! The scores of one criterion of one anchor share a denominator (the context score's `2·P·S`, the window score's
//! `len(B_s) + len(A_s)`, the fuzzy score's product of its parts' denominators), so a criterion keeps numerators only
//! and its margins compare `(n1 − n2) · m.den` with `m.num · D`. Products are compared at 256 bits, so no comparison
//! overflows.

use core::cmp::Ordering;

use crate::r14::Ratio;

const LO: u128 = u64::MAX as u128;

/// The 256-bit product `a · b` as (high, low) halves.
// spec: [F20 §1.2] "Rationals" (exact integer arithmetic)
fn mul_wide(a: u128, b: u128) -> (u128, u128) {
    let (a1, a0) = (a >> 64, a & LO);
    let (b1, b0) = (b >> 64, b & LO);
    let p00 = a0 * b0;
    let p01 = a0 * b1;
    let p10 = a1 * b0;
    let p11 = a1 * b1;
    let mid = (p00 >> 64) + (p01 & LO) + (p10 & LO);
    let lo = (p00 & LO) | (mid << 64);
    let hi = p11 + (p01 >> 64) + (p10 >> 64) + (mid >> 64);
    (hi, lo)
}

/// `a · b` compared with `c · d`, exactly.
#[must_use]
// spec: [F20 §1.2] "Rationals"
pub(crate) fn cmp_products(a: u128, b: u128, c: u128, d: u128) -> Ordering {
    mul_wide(a, b).cmp(&mul_wide(c, d))
}

/// Whether `n / den ≥ t` for a threshold `t` ([F20 §6.4] `FUZZY_ACCEPT`, §6.5 `LINES_MIN`).
// spec: [F20 §1.2] (exact comparison by cross-multiplication)
#[must_use]
pub(crate) fn at_least(n: u128, den: u128, t: Ratio) -> bool {
    cmp_products(n, u128::from(t.den()), u128::from(t.num()), den) != Ordering::Less
}

/// Whether `best` is a unique best over `second` with a margin of at least `m`, all over the denominator `den`
/// ([F20 §6.2] step 6: "a unique best with a margin ≥ M over the next decides"). With no second value the best is
/// unique. Equal values are never a unique best, whatever the margin.
// spec: [F20 §6.2] step 6, [F20 §6.4] "Acceptance", [F20 §6.5]
#[must_use]
pub(crate) fn unique_by(best: u128, second: Option<u128>, den: u128, m: Ratio) -> bool {
    match second {
        None => true,
        Some(s) => {
            best > s
                && cmp_products(best - s, u128::from(m.den()), u128::from(m.num()), den)
                    != Ordering::Less
        }
    }
}

/// An exact fuzzy score `num / den` ([F20 §6.4] "Score of a candidate"), as rendered in a link's `edited <score>`
/// detail ([F18 §4.6] code 62); the rounding of the rendering is \[F18\]'s.
#[derive(Clone, Copy, Debug)]
pub struct Score {
    /// The numerator.
    pub num: u128,
    /// The denominator, positive.
    pub den: u128,
}

impl PartialEq for Score {
    fn eq(&self, other: &Score) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Score {}

impl PartialOrd for Score {
    fn partial_cmp(&self, other: &Score) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Score {
    fn cmp(&self, other: &Score) -> Ordering {
        cmp_products(self.num, other.den, other.num, self.den)
    }
}

/// The best and the next of a stream of keyed values, for "a unique best with a margin over the next" ([F20 §6.2]
/// step 6). `T` is what the best value names; later equal values become the next, so a tie is never unique.
#[derive(Clone, Debug)]
pub(crate) struct Top2<T> {
    best: Option<(u128, T)>,
    second: Option<u128>,
}

impl<T: Copy> Top2<T> {
    pub(crate) const fn new() -> Top2<T> {
        Top2 {
            best: None,
            second: None,
        }
    }

    pub(crate) fn push(&mut self, v: u128, what: T) {
        match self.best {
            None => self.best = Some((v, what)),
            Some((b, _)) if v > b => {
                self.second = Some(b);
                self.best = Some((v, what));
            }
            Some(_) => {
                if self.second.is_none_or(|s| v > s) {
                    self.second = Some(v);
                }
            }
        }
    }

    /// The best value's subject when it is unique with margin `m` over denominator `den`.
    pub(crate) fn decide(&self, den: u128, m: Ratio) -> Option<T> {
        let (b, what) = self.best?;
        unique_by(b, self.second, den, m).then_some(what)
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn products_compare_past_128_bits() {
        let big = u128::MAX;
        assert_eq!(cmp_products(big, big, big, big - 1), Ordering::Greater);
        assert_eq!(cmp_products(big, 2, big - 1, 2), Ordering::Greater);
        assert_eq!(
            cmp_products(1 << 100, 1 << 100, 1 << 120, 1 << 80),
            Ordering::Equal
        );
        assert_eq!(cmp_products(0, big, 0, 1), Ordering::Equal);
    }

    #[test]
    fn margins_and_thresholds() {
        let m = Ratio::new(1, 10);
        // 0.6 vs 0.5 over 10: margin 0.1 exactly.
        assert!(unique_by(6, Some(5), 10, m));
        assert!(!unique_by(6, Some(6), 10, Ratio::ZERO));
        assert!(!unique_by(59, Some(50), 100, m));
        assert!(unique_by(1, None, 1, m));
        assert!(at_least(3, 4, Ratio::new(3, 4)));
        assert!(!at_least(74, 100, Ratio::new(3, 4)));
        let mut t = Top2::new();
        t.push(5, 'a');
        t.push(7, 'b');
        t.push(7, 'c');
        assert_eq!(t.decide(10, Ratio::ZERO), None);
        let mut t = Top2::new();
        t.push(5, 'a');
        t.push(9, 'b');
        t.push(3, 'c');
        assert_eq!(t.decide(10, Ratio::new(4, 10)), Some('b'));
        assert_eq!(t.decide(10, Ratio::new(41, 100)), None);
    }

    proptest! {
        #[test]
        fn products_agree_with_u128_when_they_fit(a in any::<u64>(), b in any::<u64>(), c in any::<u64>(),
                                                  d in any::<u64>()) {
            let (a, b, c, d) = (u128::from(a), u128::from(b), u128::from(c), u128::from(d));
            prop_assert_eq!(cmp_products(a, b, c, d), (a * b).cmp(&(c * d)));
        }

        #[test]
        fn scores_order_as_rationals(a in 0u64..1000, b in 1u64..1000, c in 0u64..1000, d in 1u64..1000) {
            let x = Score { num: u128::from(a), den: u128::from(b) };
            let y = Score { num: u128::from(c), den: u128::from(d) };
            prop_assert_eq!(x.cmp(&y), (u128::from(a) * u128::from(d)).cmp(&(u128::from(c) * u128::from(b))));
        }
    }
}
