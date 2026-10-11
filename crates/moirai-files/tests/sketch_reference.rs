//! WP-66: the content conditions of the `replaced` test ([F20 §5.4.1], bullets 4 and 5) and of E8 ([F20 §5.13]),
//! against the conditions written from those sections alone, with every threshold checked at its rational, one step
//! below and one step above.
//!
//! Every fingerprint here is synthetic.

use moirai_files::r14::Ratio;
use moirai_files::sketch::{
    Estimates, Fingerprint, RefEstimates, e8_content, estimates, replaced_content,
};
use proptest::prelude::*;

/// The step of the boundary tests: one step below `n/d` is `(nK − 1)/(dK)`, one step above `(nK + 1)/(dK)`.
const K: u64 = 1_000_000;

/// `n/d`, `7n/7d`, one step below and one step above.
fn points(n: u64, d: u64) -> [Ratio; 4] {
    [
        Ratio::new(n, d),
        Ratio::new(7 * n, 7 * d),
        Ratio::new(n * K - 1, d * K),
        Ratio::new(n * K + 1, d * K),
    ]
}

/// A fingerprint value field by field ([F20 §2.6.4], little-endian): `nlines`, `nbytes`, and `n` sketch values.
fn fingerprint(nlines: u32, nbytes: u32, n: u8) -> Fingerprint {
    let mut b = vec![1, 0, n, 0];
    for v in [nlines, nbytes, nbytes / 2, u32::from(n)] {
        b.extend(v.to_le_bytes());
    }
    for i in 1..=u32::from(n) {
        b.extend((i * 1_000).to_le_bytes());
    }
    Fingerprint::from_bytes(&b).unwrap()
}

fn est(eoin: Ratio, enio: Ratio) -> Estimates {
    Estimates {
        eoin,
        enio,
        esym: eoin.min(enio),
    }
}

/// A pair with the given estimates.
fn pair<'a>(r: &'a Fingerprint, c: &'a Fingerprint, eoin: Ratio, enio: Ratio) -> RefEstimates<'a> {
    RefEstimates {
        reference: r,
        content: c,
        est: est(eoin, enio),
    }
}

/// `a/b < c/d`, by cross-multiplication.
fn lt(a: Ratio, b: Ratio) -> bool {
    u128::from(a.num()) * u128::from(b.den()) < u128::from(b.num()) * u128::from(a.den())
}

/// [F20 §5.4.1] bullets 4 and 5, written out: R exists, both contents text with at least 5 lines, both estimates
/// below 29/100.
fn naive_replaced(pair: Option<&RefEstimates<'_>>) -> bool {
    pair.is_some_and(|p| {
        p.reference.nlines() >= 5
            && p.content.nlines() >= 5
            && lt(p.est.eoin, Ratio::new(29, 100))
            && lt(p.est.enio, Ratio::new(29, 100))
    })
}

/// [F20 §5.13]'s content conditions, written out: R exists, q text, both estimates at least 4/5, F not tiny (R has at
/// least 5 lines and 64 bytes).
fn naive_e8(pair: Option<&RefEstimates<'_>>) -> bool {
    pair.is_some_and(|p| {
        let tiny = p.reference.nlines() < 5 || p.reference.nbytes() < 64;
        !tiny && !lt(p.est.eoin, Ratio::new(4, 5)) && !lt(p.est.enio, Ratio::new(4, 5))
    })
}

/// `RefEstimates::new` keeps both fingerprints and computes `estimates(R, hit, content)`.
#[test]
fn ref_estimates_new_is_estimates() {
    for (n_r, n_c) in [(0u8, 0u8), (10, 3), (3, 10), (64, 64), (64, 1)] {
        let (r, c) = (fingerprint(20, 500, n_r), fingerprint(9, 200, n_c));
        for hit in 0..=u32::from(n_r) {
            let p = RefEstimates::new(&r, hit, &c);
            assert_eq!(p.est, estimates(&r, hit, &c));
            assert!(std::ptr::eq(p.reference, &r) && std::ptr::eq(p.content, &c));
        }
    }
}

/// Without R, or without a fingerprint of p's content, `replaced` does not hold; the same values with the pair do.
#[test]
fn replaced_needs_the_reference_and_text_content() {
    assert!(!replaced_content(None));
    let (r, c) = (fingerprint(20, 500, 10), fingerprint(20, 500, 10));
    assert!(replaced_content(Some(&pair(
        &r,
        &c,
        Ratio::ZERO,
        Ratio::ZERO
    ))));
}

/// `nlines ≥ REPLACED_MIN_LINES` on each side: 4 fails, 5 and 6 hold.
#[test]
fn replaced_min_lines_is_inclusive() {
    for nl_r in [4u32, 5, 6] {
        for nl_c in [4u32, 5, 6] {
            let (r, c) = (fingerprint(nl_r, 500, 10), fingerprint(nl_c, 500, 10));
            let p = pair(&r, &c, Ratio::ZERO, Ratio::ZERO);
            assert_eq!(
                replaced_content(Some(&p)),
                nl_r >= 5 && nl_c >= 5,
                "{nl_r}, {nl_c}"
            );
        }
    }
}

/// `eoin < 29/100` and `enio < 29/100` are strict: at 29/100 (also as 203/700) the test fails, one step below it
/// holds, one step above it fails; first for `eoin`, then for `enio`.
#[test]
fn replaced_max_is_strict_in_both_directions() {
    let (r, c) = (fingerprint(20, 500, 10), fingerprint(20, 500, 10));
    let [at, at7, below, above] = points(29, 100);
    for (x, want) in [(at, false), (at7, false), (below, true), (above, false)] {
        assert_eq!(
            replaced_content(Some(&pair(&r, &c, x, Ratio::ZERO))),
            want,
            "eoin {x:?}"
        );
        assert_eq!(
            replaced_content(Some(&pair(&r, &c, Ratio::ZERO, x))),
            want,
            "enio {x:?}"
        );
    }
}

/// Without R, or without a fingerprint of q's content, E8 yields nothing; the same values with the pair hold.
#[test]
fn e8_needs_the_reference_and_text_content() {
    assert!(!e8_content(None));
    let (r, c) = (fingerprint(20, 500, 10), fingerprint(20, 500, 10));
    assert!(e8_content(Some(&pair(&r, &c, Ratio::ONE, Ratio::ONE))));
}

/// `eoin ≥ 4/5` and `enio ≥ 4/5` are inclusive: at 4/5 (also as 28/35) E8 holds, one step below it fails, one step
/// above it holds; first for `eoin`, then for `enio`.
#[test]
fn e8_min_is_inclusive_in_both_directions() {
    let (r, c) = (fingerprint(20, 500, 10), fingerprint(20, 500, 10));
    let [at, at7, below, above] = points(4, 5);
    for (x, want) in [(at, true), (at7, true), (below, false), (above, true)] {
        assert_eq!(
            e8_content(Some(&pair(&r, &c, x, Ratio::ONE))),
            want,
            "eoin {x:?}"
        );
        assert_eq!(
            e8_content(Some(&pair(&r, &c, Ratio::ONE, x))),
            want,
            "enio {x:?}"
        );
    }
}

/// E8 is never for a tiny F: an R tiny by its lines or by its bytes fails with both estimates 1; an R at both
/// thresholds holds. Only F's tininess counts, so a tiny q does not stop E8.
#[test]
fn e8_is_never_for_a_tiny_node() {
    let c = fingerprint(20, 500, 10);
    for (r, want) in [
        (fingerprint(4, 1_000, 10), false),
        (fingerprint(10, 63, 10), false),
        (fingerprint(5, 64, 10), true),
    ] {
        assert_eq!(r.is_tiny(), !want);
        assert_eq!(
            e8_content(Some(&pair(&r, &c, Ratio::ONE, Ratio::ONE))),
            want,
            "{r:?}"
        );
    }
    let (r, tiny_q) = (fingerprint(20, 500, 10), fingerprint(1, 10, 1));
    assert!(tiny_q.is_tiny());
    assert!(e8_content(Some(&pair(&r, &tiny_q, Ratio::ONE, Ratio::ONE))));
}

/// The proptest configuration of a suite whose tier-`pr` case count is `base`: `MOIRAI_TEST_TIER` = `nightly` runs 16
/// times as many and `exit` 64 times as many (PLAN §2.1 test tiers); no failure persistence (the seed is printed), so a
/// failing case never writes a regressions file into the repository.
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

/// A rational on the grid `k/100`, `0 ≤ k ≤ 100`, written as `mk/100m` for `m` from 1 to 7: it holds 29/100 and 4/5
/// and their neighbours on the grid, which two thirds of the values lie near.
fn grid() -> impl Strategy<Value = Ratio> {
    (prop_oneof![0u64..=100, 26u64..=32, 77u64..=83], 1u64..=7)
        .prop_map(|(k, m)| Ratio::new(m * k, m * 100))
}

proptest! {
    #![proptest_config(test_config(256))]

    /// `replaced_content` equals the conditions written out, with `nlines` around 5 and estimates on a grid that
    /// holds 29/100.
    #[test]
    fn replaced_equals_the_naive_definition(
        present in any::<bool>(),
        nl_r in 0u32..10,
        nl_c in 0u32..10,
        eoin in grid(),
        enio in grid(),
    ) {
        let (r, c) = (fingerprint(nl_r, 500, 10), fingerprint(nl_c, 500, 10));
        let p = pair(&r, &c, eoin, enio);
        let p = present.then_some(&p);
        prop_assert_eq!(replaced_content(p), naive_replaced(p));
    }

    /// `e8_content` equals the conditions written out, with R's `nlines` around 5, `nbytes` around 64 and estimates on
    /// a grid that holds 4/5.
    #[test]
    fn e8_equals_the_naive_definition(
        present in any::<bool>(),
        nl_r in 0u32..10,
        nb_r in 56u32..72,
        eoin in grid(),
        enio in grid(),
    ) {
        let (r, c) = (fingerprint(nl_r, nb_r, 10), fingerprint(20, 500, 10));
        let p = pair(&r, &c, eoin, enio);
        let p = present.then_some(&p);
        prop_assert_eq!(e8_content(p), naive_e8(p));
    }
}
