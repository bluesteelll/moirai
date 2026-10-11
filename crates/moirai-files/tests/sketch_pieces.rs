//! WP-66: the split and merged predicates ([F20 §5.11.4] rows 2 and 5; §5.15 "split" and "merged"), against the
//! tests written from those rows alone, with every threshold checked at its rational, one step below and one step
//! above, and the split sum checked past `u128`.
//!
//! Every path and containment here is synthetic.

use moirai_files::r14::Ratio;
use moirai_files::sketch::{Candidate, merged, split};
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

fn cand(path: &str, oin: Ratio, nio: Ratio) -> Candidate<'_> {
    Candidate {
        path: path.as_bytes(),
        oin,
        nio,
    }
}

fn r(n: u64, d: u64) -> Ratio {
    Ratio::new(n, d)
}

/// The paths of `indices`.
fn paths<'a>(c: &[Candidate<'a>], indices: &[usize]) -> Vec<&'a [u8]> {
    indices.iter().map(|&i| c[i].path).collect()
}

// --- the tests written from [F20 §5.11.4] rows 2 and 5 alone -----------------------------------------------------

/// `a/b ≥ c/d`, by cross-multiplication.
fn ge(x: Ratio, c: u64, d: u64) -> bool {
    u128::from(x.num()) * u128::from(d) >= u128::from(c) * u128::from(x.den())
}

/// Row 2: the pieces are the candidates with `nio ≥ 4/5`; at least 2 of them, whose `oin` sum to at least 3/5. The
/// sum is one fraction over the product of the denominators, which fits `u128` for the small denominators of the
/// property tests. The result is the pieces' paths, sorted.
fn naive_split<'a>(c: &[Candidate<'a>]) -> Option<Vec<&'a [u8]>> {
    let pieces: Vec<&Candidate<'a>> = c.iter().filter(|x| ge(x.nio, 4, 5)).collect();
    if pieces.len() < 2 {
        return None;
    }
    let (mut n, mut d) = (0u128, 1u128);
    for p in &pieces {
        let (a, b) = (u128::from(p.oin.num()), u128::from(p.oin.den()));
        n = n * b + a * d;
        d *= b;
    }
    if n * 5 < 3 * d {
        return None;
    }
    let mut out: Vec<&[u8]> = pieces.iter().map(|p| p.path).collect();
    out.sort();
    Some(out)
}

/// Row 5: the candidates with `oin ≥ 4/5` and `nio < 1/2`; the first by path.
fn naive_merged<'a>(c: &[Candidate<'a>]) -> Option<&'a [u8]> {
    c.iter()
        .filter(|x| ge(x.oin, 4, 5) && !ge(x.nio, 1, 2))
        .map(|x| x.path)
        .min()
}

// --- the examples --------------------------------------------------------------------------------------------------

/// A split needs at least 2 pieces: none, one candidate, or one piece beside a non-piece is no split, even when the
/// one piece holds all of X; two pieces are.
#[test]
fn split_needs_two_pieces() {
    assert_eq!(split(&[]), None);
    assert_eq!(split(&[cand("a", Ratio::ONE, Ratio::ONE)]), None);
    assert_eq!(
        split(&[
            cand("a", Ratio::ONE, Ratio::ONE),
            cand("b", Ratio::ONE, r(1, 2))
        ]),
        None
    );
    assert_eq!(
        split(&[
            cand("a", r(1, 2), Ratio::ONE),
            cand("b", r(1, 2), Ratio::ONE)
        ]),
        Some(vec![0, 1])
    );
}

/// `nio ≥ 4/5` makes a piece: at 4/5 (also as 28/35) and one step above it the split holds, one step below it does
/// not. The pieces' `oin` sum `≥ 3/5`: 3/10 plus a second piece at 3/10 (also as 21/70) holds, one step below it does
/// not, one step above it holds.
#[test]
fn split_thresholds_are_inclusive() {
    let [at, at7, below, above] = points(4, 5);
    for (x, want) in [(at, true), (at7, true), (below, false), (above, true)] {
        let c = [cand("a", r(1, 2), Ratio::ONE), cand("b", r(1, 2), x)];
        assert_eq!(split(&c).is_some(), want, "nio {x:?}");
    }
    let [at, at7, below, above] = points(3, 10);
    for (x, want) in [(at, true), (at7, true), (below, false), (above, true)] {
        let c = [cand("a", r(3, 10), Ratio::ONE), cand("b", x, Ratio::ONE)];
        assert_eq!(split(&c).is_some(), want, "oin {x:?}");
    }
}

/// The pieces are listed in path order, whatever the input order: bytewise, so `a/b` (`2F`) comes before `a0`
/// (`30`), and a proper prefix first. Candidates that are not pieces are left out.
#[test]
fn split_lists_the_pieces_in_path_order() {
    let piece = |p| cand(p, r(1, 4), Ratio::ONE);
    let c = [
        piece("src/b.rs"),
        cand("src", Ratio::ONE, r(1, 2)),
        piece("a0"),
        piece("a/b"),
        cand("zz", r(1, 2), r(79, 100)),
        piece("a"),
    ];
    let got = split(&c).unwrap();
    assert_eq!(got, vec![5, 3, 2, 0]);
    assert_eq!(
        paths(&c, &got),
        [&b"a"[..], b"a/b", b"a0", b"src/b.rs"].to_vec()
    );
}

/// The sum is exact with denominators near 2^64, where every product of two of them overflows `u64` and of three
/// overflows `u128`. With `m = (2^64 − 1)/5`:
/// - `m/5m`, `(m − 1)/(5m − 5)` and `(m − 2)/(5m − 10)` are each exactly 1/5, so they sum to exactly 3/5: a split;
/// - `m/5m`, `m/(5m − 1)` and `(m − 1)/(5m − 2)` (pairwise coprime denominators) sum to
///   `3/5 + 1/(5(5m − 1)) − 3/(5(5m − 2))`, just below 3/5: no split;
/// - `m/5m`, `m/(5m − 1)` and `m/(5m − 2)` sum to `3/5 + 1/(5(5m − 1)) + 2/(5(5m − 2))`, just above: a split.
///
/// Fifty pieces with distinct large denominators end with no split when their sum is tiny and with a split when it is
/// exactly 1, and never panic.
#[test]
fn split_sum_is_exact_at_extreme_denominators() {
    let m = u64::MAX / 5;
    assert_eq!(5 * m, u64::MAX);
    let one = Ratio::ONE;
    let at = [
        cand("a", r(m, 5 * m), one),
        cand("b", r(m - 1, 5 * m - 5), one),
        cand("c", r(m - 2, 5 * m - 10), one),
    ];
    assert_eq!(split(&at), Some(vec![0, 1, 2]));
    let below = [
        cand("a", r(m, 5 * m), one),
        cand("b", r(m, 5 * m - 1), one),
        cand("c", r(m - 1, 5 * m - 2), one),
    ];
    assert_eq!(split(&below), None);
    let above = [
        cand("a", r(m, 5 * m), one),
        cand("b", r(m, 5 * m - 1), one),
        cand("c", r(m, 5 * m - 2), one),
    ];
    assert_eq!(split(&above), Some(vec![0, 1, 2]));

    let names: Vec<String> = (0..50).map(|i| format!("piece{i:02}")).collect();
    let many: Vec<Candidate<'_>> = names
        .iter()
        .enumerate()
        .map(|(i, p)| cand(p, r(1, u64::MAX - i as u64), one))
        .collect();
    assert_eq!(split(&many), None);
    // Fifty pieces of exactly 1/50 each, written `k/50k` with `k = ⌊(2^64 − 1)/50⌋ − i`: distinct denominators near
    // 2^64, and a sum of exactly 1.
    let many: Vec<Candidate<'_>> = names
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let k = u64::MAX / 50 - i as u64;
            cand(p, r(k, 50 * k), one)
        })
        .collect();
    assert_eq!(split(&many).map(|v| v.len()), Some(50));
}

/// `oin ≥ 4/5` is inclusive: at 4/5 (also as 28/35) and one step above the candidate is merged, one step below it is
/// not. `nio < 1/2` is strict: at 1/2 (also as 7/14) and one step above it is not, one step below it is.
#[test]
fn merged_thresholds_oin_inclusive_nio_strict() {
    let [at, at7, below, above] = points(4, 5);
    for (x, want) in [(at, true), (at7, true), (below, false), (above, true)] {
        assert_eq!(
            merged(&[cand("h", x, Ratio::ZERO)]).is_some(),
            want,
            "oin {x:?}"
        );
    }
    let [at, at7, below, above] = points(1, 2);
    for (x, want) in [(at, false), (at7, false), (below, true), (above, false)] {
        assert_eq!(
            merged(&[cand("h", Ratio::ONE, x)]).is_some(),
            want,
            "nio {x:?}"
        );
    }
}

/// Of several hosts, the first in path order wins, whatever the input order: `a/b` before `a0`, `a` before `ab`; a
/// candidate earlier in path order that is no host does not count.
#[test]
fn merged_takes_the_first_in_path_order() {
    let host = |p| cand(p, r(9, 10), r(1, 5));
    assert_eq!(merged(&[host("a0"), host("a/b")]), Some(1));
    assert_eq!(merged(&[host("a/b"), host("a0")]), Some(0));
    assert_eq!(merged(&[host("ab"), host("a")]), Some(1));
    assert_eq!(
        merged(&[host("b"), cand("a", Ratio::ONE, Ratio::ONE)]),
        Some(0)
    );
    assert_eq!(merged(&[]), None);
}

// --- the property tests ------------------------------------------------------------------------------------------

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

/// Short paths, with proper prefixes and the `2F` / `30` pair.
const POOL: [&str; 10] = [
    "a", "a/b", "a0", "a/b/c", "b", "ab", "a/c", "src", "src/x.rs", "src/y.rs",
];

/// A containment `k/d` with `d` from 1 to 12, or one near the thresholds 4/5 and 1/2.
fn ratio() -> impl Strategy<Value = Ratio> {
    prop_oneof![
        (1u64..=12)
            .prop_flat_map(|d| (0..=d, Just(d)))
            .prop_map(|(k, d)| Ratio::new(k, d)),
        (6u64..=10).prop_map(|k| Ratio::new(k, 10)),
        Just(Ratio::new(4, 5)),
        Just(Ratio::new(1, 2)),
    ]
}

/// 0 to 6 candidates with distinct paths, in any order.
fn candidates() -> impl Strategy<Value = Vec<(&'static str, Ratio, Ratio)>> {
    prop::sample::subsequence(POOL.to_vec(), 0..=6)
        .prop_flat_map(|ps| {
            let n = ps.len();
            (Just(ps), prop::collection::vec((ratio(), ratio()), n))
        })
        .prop_map(|(ps, rs)| {
            ps.into_iter()
                .zip(rs)
                .map(|(p, (o, n))| (p, o, n))
                .collect::<Vec<_>>()
        })
        .prop_shuffle()
}

fn build(v: &[(&'static str, Ratio, Ratio)]) -> Vec<Candidate<'static>> {
    v.iter().map(|&(p, o, n)| cand(p, o, n)).collect()
}

proptest! {
    #![proptest_config(test_config(256))]

    /// `split` equals row 2 written out, over small denominators.
    #[test]
    fn split_equals_the_naive_definition(v in candidates()) {
        let c = build(&v);
        let got = split(&c).map(|ix: Vec<usize>| paths(&c, &ix));
        prop_assert_eq!(got, naive_split(&c));
    }

    /// `merged` equals row 5 written out.
    #[test]
    fn merged_equals_the_naive_definition(v in candidates()) {
        let c = build(&v);
        prop_assert_eq!(merged(&c).map(|i| c[i].path), naive_merged(&c));
    }

    /// A permutation of the input names the same pieces in the same order, and the same host.
    #[test]
    fn split_and_merged_ignore_input_order(
        (v, w) in candidates().prop_flat_map(|v| (Just(v.clone()), Just(v).prop_shuffle())),
    ) {
        let (c, d) = (build(&v), build(&w));
        prop_assert_eq!(
            split(&c).map(|ix: Vec<usize>| paths(&c, &ix)),
            split(&d).map(|ix: Vec<usize>| paths(&d, &ix))
        );
        prop_assert_eq!(merged(&c).map(|i| c[i].path), merged(&d).map(|i| d[i].path));
    }
}
