//! WP-66: the similarity classification of [F20 §5.15], with the rows in [F20 §5.11.4]'s order (split, strong,
//! merged, weak, none) and weak the residual class, as RULINGS R-13 rules, against a table written from those
//! sections and that ruling alone. Every threshold is checked at its rational, one step below and one step above.
//!
//! `classify` takes plain values and does not check that a score agrees with its containments. So that split and
//! merged hold only where a test means them to, every other candidate here has the containments 0, at which neither
//! holds (`nio < 4/5` and `oin < 4/5`).
//!
//! Every path and score here is synthetic.

use std::cmp::Ordering;

use moirai_files::r14::Ratio;
use moirai_files::sketch::{Basis, PairScore, Scored, Similarity, classify};
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

fn r(n: u64, d: u64) -> Ratio {
    Ratio::new(n, d)
}

/// A candidate with the given score and containments.
fn full(
    path: &str,
    score: Ratio,
    oin: Ratio,
    nio: Ratio,
    same_dir: bool,
    same_base: bool,
) -> Scored<'_> {
    Scored {
        path: path.as_bytes(),
        score: PairScore {
            score,
            oin,
            nio,
            basis: Basis::Exact,
        },
        same_dir,
        same_base,
    }
}

/// A candidate whose containments are 0, so that it is neither a split piece nor a merge host.
fn sc(path: &str, score: Ratio, same_dir: bool) -> Scored<'_> {
    full(path, score, Ratio::ZERO, Ratio::ZERO, same_dir, false)
}

use Similarity::{Ambiguous, Merged, Missing, Split, Strong, Weak};

// --- the examples --------------------------------------------------------------------------------------------------

/// No candidate: `missing`.
#[test]
fn empty_list_is_missing() {
    assert_eq!(classify(&[]), Missing);
}

/// Strong needs the score, the margin and a corroboration; each failing alone leaves the class R-13 gives. A
/// `dirname` equality alone and a `basename` equality alone each corroborate.
#[test]
fn strong_needs_score_margin_and_corroboration() {
    // All three hold: best 7/10, runner-up 1/5, the same dirname.
    assert_eq!(
        classify(&[sc("d/q", r(7, 10), true), sc("x/r", r(1, 5), false)]),
        Strong(0)
    );
    // The score fails alone (2/5, with a margin of 3/10): weak, the runner-up being below 3/10.
    assert_eq!(
        classify(&[sc("d/q", r(2, 5), true), sc("x/r", r(1, 10), false)]),
        Weak(0)
    );
    // The margin fails alone (7/10 over 3/5): ambiguous, the runner-up being at least 3/10.
    assert_eq!(
        classify(&[sc("d/q", r(7, 10), true), sc("x/r", r(3, 5), false)]),
        Ambiguous(vec![0, 1])
    );
    // The corroboration fails alone: weak, the runner-up being below 3/10.
    assert_eq!(
        classify(&[sc("d/q", r(7, 10), false), sc("x/r", r(1, 5), false)]),
        Weak(0)
    );
    // Either equality corroborates on its own.
    let dir_only = full("d/q", r(7, 10), Ratio::ZERO, Ratio::ZERO, true, false);
    let base_only = full("e/p", r(7, 10), Ratio::ZERO, Ratio::ZERO, false, true);
    let other = sc("x/r", r(1, 5), false);
    assert_eq!(classify(&[dir_only, other]), Strong(0));
    assert_eq!(classify(&[other, base_only]), Strong(1));
}

/// `best ≥ 1/2` and `margin ≥ 1/5` are inclusive: at the rational (also as `7n/7d`) and one step above the best is
/// strong, one step below it is not.
#[test]
fn strong_boundaries_are_inclusive() {
    // The score, with a runner-up of 1/5: below 1/2 the best is weak.
    let [at, at7, below, above] = points(1, 2);
    for (x, want) in [
        (at, Strong(0)),
        (at7, Strong(0)),
        (below, Weak(0)),
        (above, Strong(0)),
    ] {
        assert_eq!(
            classify(&[sc("d/q", x, true), sc("x/r", r(1, 5), false)]),
            want,
            "best {x:?}"
        );
    }
    // The margin, over a runner-up of 1/2: the best at 7/10 has a margin of exactly 1/5; below, both are listed.
    let [at, at7, below, above] = points(7, 10);
    for (x, want) in [
        (at, Strong(0)),
        (at7, Strong(0)),
        (below, Ambiguous(vec![0, 1])),
        (above, Strong(0)),
    ] {
        assert_eq!(
            classify(&[sc("d/q", x, true), sc("x/r", r(1, 2), false)]),
            want,
            "best {x:?}"
        );
    }
}

/// The margin is the best score less the runner-up's, not the best score itself and not 0: 9/10 over 4/5 is no
/// strong candidate, 3/5 over 7/20 is.
#[test]
fn margin_is_over_the_runner_up() {
    assert_eq!(
        classify(&[sc("d/q", r(9, 10), true), sc("x/r", r(4, 5), false)]),
        Ambiguous(vec![0, 1])
    );
    assert_eq!(
        classify(&[sc("d/q", r(3, 5), true), sc("x/r", r(7, 20), false)]),
        Strong(0)
    );
}

/// One candidate has no runner-up, so its margin is its score: a corroborated 1/2 is strong, a corroborated 2/5 is
/// weak, and 29/100 is below 3/10, so missing.
#[test]
fn one_candidate_has_no_runner_up() {
    assert_eq!(classify(&[sc("d/q", r(1, 2), true)]), Strong(0));
    assert_eq!(classify(&[sc("d/q", r(2, 5), true)]), Weak(0));
    assert_eq!(classify(&[sc("d/q", r(29, 100), true)]), Missing);
}

/// Equal top scores never pick a winner, however high and however corroborated: they are listed in path order.
#[test]
fn ties_never_make_a_strong_candidate() {
    assert_eq!(
        classify(&[sc("d/b", r(4, 5), true), sc("d/a", r(8, 10), true)]),
        Ambiguous(vec![1, 0])
    );
}

/// The margin is exact with denominators near 2^64, where `a·d·f` overflows `u128`. With `m = (2^64 − 1)/5`, the best
/// `3m/5m` = 3/5 and the runner-up `2(m − 1)/(5m − 5)` = 2/5 have a margin of exactly 1/5: strong. The best one step
/// below, `(3m − 1)/5m`, is not; one step above, `(3m + 1)/5m`, is.
#[test]
fn margin_is_exact_at_extreme_denominators() {
    let m = u64::MAX / 5;
    assert_eq!(5 * m, u64::MAX);
    let runner_up = sc("x/r", r(2 * (m - 1), 5 * m - 5), false);
    for (x, want) in [
        (r(3 * m, 5 * m), Strong(0)),
        (r(3 * m - 1, 5 * m), Ambiguous(vec![0, 1])),
        (r(3 * m + 1, 5 * m), Strong(0)),
    ] {
        assert_eq!(
            classify(&[sc("d/q", x, true), runner_up]),
            want,
            "best {x:?}"
        );
    }
}

/// `score ≥ 3/10` is inclusive: at 3/10 (also as 21/70) and one step above a lone candidate is weak, one step below
/// it is missing.
#[test]
fn sim_weak_boundary_is_inclusive() {
    let [at, at7, below, above] = points(3, 10);
    for (x, want) in [
        (at, Weak(0)),
        (at7, Weak(0)),
        (below, Missing),
        (above, Weak(0)),
    ] {
        assert_eq!(classify(&[sc("d/q", x, true)]), want, "best {x:?}");
    }
}

/// A best in [3/10, 1/2) with every other score below 3/10 is weak, whatever the input order.
#[test]
fn weak_with_one_candidate_needs_confirm() {
    let (q, other) = (sc("d/q", r(2, 5), true), sc("x/r", r(1, 4), false));
    assert_eq!(classify(&[q, other]), Weak(0));
    assert_eq!(classify(&[other, q]), Weak(1));
}

/// Two or more scores of at least 3/10 are ambiguous: only those are listed, by score descending, then path.
#[test]
fn weak_with_two_or_more_is_ambiguous_in_score_then_path_order() {
    let c = [
        sc("c", r(2, 5), false),
        sc("b", r(9, 20), true),
        sc("a", r(4, 10), false),
        sc("d", r(1, 4), true),
        sc("e", r(3, 10), false),
    ];
    assert_eq!(classify(&c), Ambiguous(vec![1, 2, 0, 4]));
}

/// The rows apply in [F20 §5.11.4]'s order, split, strong, merged, weak, none, and the first that holds decides
/// (R-13); each pair of rows that can both hold is shown.
#[test]
fn rows_apply_split_strong_merged_weak_in_order() {
    // Split over strong, F3's example: `src/a.rs` split into `src/b.rs` (oin 7/10) and `src/c.rs` (oin 3/10), both
    // in its directory. `src/b.rs` alone would be strong (7/10 over 3/10, the same dirname).
    let b = full("src/b.rs", r(7, 10), r(7, 10), Ratio::ONE, true, false);
    let c = full("src/c.rs", r(3, 10), r(3, 10), Ratio::ONE, true, false);
    assert_eq!(classify(&[c, b]), Split(vec![1, 0]));
    assert_eq!(classify(&[b, sc("src/c.rs", r(3, 10), true)]), Strong(0));

    // Split over merged: two pieces and a host, nothing strong.
    let p1 = full("p1", r(1, 4), r(1, 2), Ratio::ONE, false, false);
    let p2 = full("p2", r(1, 4), r(1, 2), r(9, 10), false, false);
    let host = full("h", r(1, 5), r(9, 10), r(1, 5), false, false);
    assert_eq!(classify(&[host, p2, p1]), Split(vec![2, 1]));
    assert_eq!(classify(&[host, p2]), Merged(0));

    // Strong over merged: a strong candidate beside a host whose score is the runner-up's.
    let strong = sc("d/q", r(7, 10), true);
    let host = full("h", r(2, 5), r(9, 10), r(2, 5), false, false);
    assert_eq!(classify(&[host, strong]), Strong(1));
    assert_eq!(classify(&[host, sc("d/q", r(7, 10), false)]), Merged(0));

    // Merged over weak: a lone, uncorroborated host with score 1/2, oin 9/10 and nio 2/5.
    let host = full("h", r(1, 2), r(9, 10), r(2, 5), false, false);
    assert_eq!(classify(&[host]), Merged(0));

    // Weak over none, and none only when no row holds: oin 7/10 is no host.
    let near = full("h", r(1, 2), r(7, 10), r(2, 5), false, false);
    assert_eq!(classify(&[near]), Weak(0));
    let low = full("h", r(1, 4), r(7, 10), r(1, 4), false, false);
    assert_eq!(classify(&[low]), Missing);
}

/// R-13 Q1 (a): a best of at least 1/2 with a margin of at least 1/5 but neither the dirname nor the basename of p is
/// weak, not missing: alone at or above 3/10 it needs confirmation, beside a runner-up at or above 3/10 both are
/// listed.
#[test]
fn uncorroborated_strong_score_is_weak() {
    assert_eq!(
        classify(&[sc("x/q", r(7, 10), false), sc("y/r", r(1, 5), false)]),
        Weak(0)
    );
    assert_eq!(
        classify(&[sc("y/r", r(3, 10), false), sc("x/q", r(9, 10), false)]),
        Ambiguous(vec![1, 0])
    );
}

/// Split and merged decide even when every score is below 3/10, where none's own condition also holds: a content
/// split into three pieces of 1/5 each (each scoring `min(1/5, 1)`), and a small content folded into a large host
/// (scoring its `nio` of 1/5).
#[test]
fn split_and_merged_decide_when_no_score_reaches_sim_weak() {
    let piece = |p| full(p, r(1, 5), r(1, 5), Ratio::ONE, false, false);
    assert_eq!(
        classify(&[piece("c"), piece("a"), piece("b")]),
        Split(vec![1, 2, 0])
    );
    let host = full("h", r(1, 5), r(9, 10), r(1, 5), false, true);
    assert_eq!(classify(&[sc("x", r(1, 4), false), host]), Merged(1));
}

// --- the table written from [F20 §5.15], §5.11.4 row 2 and R-13 alone --------------------------------------------

/// A class with its candidates named by path.
#[derive(Debug, PartialEq, Eq)]
enum Class<'a> {
    Split(Vec<&'a [u8]>),
    Strong(&'a [u8]),
    Merged(&'a [u8]),
    Weak(&'a [u8]),
    Ambiguous(Vec<&'a [u8]>),
    Missing,
}

fn by_path<'a>(c: &[Scored<'a>], s: Similarity) -> Class<'a> {
    let names = |ix: Vec<usize>| ix.into_iter().map(|i| c[i].path).collect();
    match s {
        Split(ix) => Class::Split(names(ix)),
        Strong(i) => Class::Strong(c[i].path),
        Merged(i) => Class::Merged(c[i].path),
        Weak(i) => Class::Weak(c[i].path),
        Ambiguous(ix) => Class::Ambiguous(names(ix)),
        Missing => Class::Missing,
    }
}

/// `x` against `y` as rationals.
fn cmp(x: Ratio, y: Ratio) -> Ordering {
    (u128::from(x.num()) * u128::from(y.den())).cmp(&(u128::from(y.num()) * u128::from(x.den())))
}

fn ge(x: Ratio, n: u64, d: u64) -> bool {
    cmp(x, Ratio::new(n, d)) != Ordering::Less
}

/// The table: split (row 2: pieces with `nio ≥ 4/5`, at least 2, `oin` summing to at least 3/5), strong (best ≥ 1/2,
/// best − runner-up ≥ 1/5, the same dirname or basename), merged (`oin ≥ 4/5`, `nio < 1/2`, first by path), weak
/// (every score ≥ 3/10: one, or two or more in score-then-path order), none.
fn naive<'a>(c: &[Scored<'a>]) -> Class<'a> {
    let mut order: Vec<&Scored<'a>> = c.iter().collect();
    order.sort_by(|x, y| cmp(y.score.score, x.score.score).then(x.path.cmp(y.path)));
    let Some(best) = order.first() else {
        return Class::Missing;
    };

    let pieces: Vec<&Scored<'a>> = c.iter().filter(|x| ge(x.score.nio, 4, 5)).collect();
    if pieces.len() >= 2 {
        let (mut n, mut d) = (0u128, 1u128);
        for p in &pieces {
            let (a, b) = (u128::from(p.score.oin.num()), u128::from(p.score.oin.den()));
            n = n * b + a * d;
            d *= b;
        }
        if n * 5 >= 3 * d {
            let mut paths: Vec<&[u8]> = pieces.iter().map(|p| p.path).collect();
            paths.sort();
            return Class::Split(paths);
        }
    }

    let runner_up = order.get(1).map_or(Ratio::ZERO, |x| x.score.score);
    // best − runner-up ≥ 1/5 ⟺ 5 · (a·d − c·b) ≥ b·d, in i128.
    let (a, b) = (
        i128::from(best.score.score.num()),
        i128::from(best.score.score.den()),
    );
    let (cc, d) = (i128::from(runner_up.num()), i128::from(runner_up.den()));
    let margin = 5 * (a * d - cc * b) >= b * d;
    if ge(best.score.score, 1, 2) && margin && (best.same_dir || best.same_base) {
        return Class::Strong(best.path);
    }

    if let Some(host) = c
        .iter()
        .filter(|x| ge(x.score.oin, 4, 5) && !ge(x.score.nio, 1, 2))
        .map(|x| x.path)
        .min()
    {
        return Class::Merged(host);
    }

    let weak: Vec<&[u8]> = order
        .iter()
        .filter(|x| ge(x.score.score, 3, 10))
        .map(|x| x.path)
        .collect();
    match weak.len() {
        0 => Class::Missing,
        1 => Class::Weak(weak[0]),
        _ => Class::Ambiguous(weak),
    }
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

/// `k/20`, written as `mk/20m` for `m` from 1 to 3.
fn score() -> impl Strategy<Value = Ratio> {
    (0u64..=20, 1u64..=3).prop_map(|(k, m)| Ratio::new(m * k, m * 20))
}

/// `k/10`.
fn containment() -> impl Strategy<Value = Ratio> {
    (0u64..=10).prop_map(|k| Ratio::new(k, 10))
}

type Row = (&'static str, Ratio, Ratio, Ratio, bool, bool);

/// 0 to 6 candidates with distinct paths, in any order.
fn rows() -> impl Strategy<Value = Vec<Row>> {
    prop::sample::subsequence(POOL.to_vec(), 0..=6)
        .prop_flat_map(|ps| {
            let n = ps.len();
            (
                Just(ps),
                prop::collection::vec(
                    (
                        score(),
                        containment(),
                        containment(),
                        any::<bool>(),
                        any::<bool>(),
                    ),
                    n,
                ),
            )
        })
        .prop_map(|(ps, vs)| {
            ps.into_iter()
                .zip(vs)
                .map(|(p, (s, o, n, dir, base))| (p, s, o, n, dir, base))
                .collect::<Vec<_>>()
        })
        .prop_shuffle()
}

fn build(v: &[Row]) -> Vec<Scored<'static>> {
    v.iter()
        .map(|&(p, s, o, n, dir, base)| full(p, s, o, n, dir, base))
        .collect()
}

proptest! {
    #![proptest_config(test_config(512))]

    /// `classify` equals the table written from [F20 §5.15] and R-13.
    #[test]
    fn classify_equals_the_naive_table(v in rows()) {
        let c = build(&v);
        prop_assert_eq!(by_path(&c, classify(&c)), naive(&c));
    }

    /// A permutation of the input names the same candidates in the same order.
    #[test]
    fn classification_ignores_input_order(
        (v, w) in rows().prop_flat_map(|v| (Just(v.clone()), Just(v).prop_shuffle())),
    ) {
        let (c, d) = (build(&v), build(&w));
        prop_assert_eq!(by_path(&c, classify(&c)), by_path(&d, classify(&d)));
    }
}
