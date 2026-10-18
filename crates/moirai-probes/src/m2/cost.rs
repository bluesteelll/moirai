//! Measurement 2's injected in-lock cost (R-24 Q2, R-31 Q1 (C)): a draw per commit from the hold budget's distribution,
//! `round(scale / 100 × Q(u))` with u uniform in [0, 1) from the commit's seeded index. Each shape's quantile function
//! `Q` is one table of anchors, so a ruling changes data, not code.
//!
//! | Shape | Q below the median | to p99 | to the maximum | p50 | p99 | max |
//! |---|---|---|---|---|---|---|
//! | `Design` (`c`) | linear from 0 | log-linear | linear | 50 µs ([AR §8.1]) | 5 ms ([AR §8.3]) | 20 ms ([AR §8.3]) |
//! | `Uniform` (`a`) | linear from 0 | linear | linear | 5 ms × 0.5 / 0.99 ≈ 2.525 ms | 5 ms | 20 ms |

use crate::m1::store::draw64;

/// The shape of the hold distribution below its p99 (R-31 Q1).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Shape {
    /// R-31's (B): anchored on the design's "appends of tens of µs under the writer byte"; the decision follows it.
    Design,
    /// R-31's (A): uniform below p99, the maximum-entropy body; the sensitivity run.
    Uniform,
}

/// How `Q` runs from the previous anchor to this one.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Interp {
    Linear,
    LogLinear,
}

/// One anchor: `Q(u) = ns`, reached from the previous anchor by `interp`.
#[derive(Clone, Copy, Debug)]
struct Anchor {
    u: f64,
    ns: f64,
    interp: Interp,
}

const fn anchor(u: f64, ns: f64, interp: Interp) -> Anchor {
    Anchor { u, ns, interp }
}

/// `Design`: 0 → 50 µs linear on [0, 0.5], → 5 ms log-linear on [0.5, 0.99], → 20 ms linear on [0.99, 1].
const DESIGN: [Anchor; 4] = [
    anchor(0.0, 0.0, Interp::Linear),
    anchor(0.5, 50_000.0, Interp::Linear),
    anchor(0.99, 5_000_000.0, Interp::LogLinear),
    anchor(1.0, 20_000_000.0, Interp::Linear),
];

/// `Uniform`: 0 → 5 ms linear on [0, 0.99], → 20 ms linear on [0.99, 1].
const UNIFORM: [Anchor; 3] = [
    anchor(0.0, 0.0, Interp::Linear),
    anchor(0.99, 5_000_000.0, Interp::Linear),
    anchor(1.0, 20_000_000.0, Interp::Linear),
];

impl Shape {
    /// The letter of the quantity names: `c` for `Design`, `a` for `Uniform`.
    pub const fn letter(self) -> char {
        match self {
            Shape::Design => 'c',
            Shape::Uniform => 'a',
        }
    }

    /// The shape of a letter.
    pub fn from_letter(c: char) -> Option<Shape> {
        match c {
            'c' => Some(Shape::Design),
            'a' => Some(Shape::Uniform),
            _ => None,
        }
    }

    fn anchors(self) -> &'static [Anchor] {
        match self {
            Shape::Design => &DESIGN,
            Shape::Uniform => &UNIFORM,
        }
    }
}

/// The quantile function of `shape` at rank `u` (clamped to [0, 1]), in ns.
pub fn quantile(shape: Shape, u: f64) -> f64 {
    let a = shape.anchors();
    let u = u.clamp(0.0, 1.0);
    for w in a.windows(2) {
        let (lo, hi) = (w[0], w[1]);
        if u <= hi.u {
            let f = (u - lo.u) / (hi.u - lo.u);
            return match hi.interp {
                Interp::Linear => lo.ns + f * (hi.ns - lo.ns),
                Interp::LogLinear => lo.ns * (hi.ns / lo.ns).powf(f),
            };
        }
    }
    a[a.len() - 1].ns
}

/// The rank of draw `index` of stream `seed`: the top 53 bits of `draw64(seed, index)` over 2^53, in [0, 1).
pub fn rank(seed: u64, index: u64) -> f64 {
    (draw64(seed, index) >> 11) as f64 / (1u64 << 53) as f64
}

/// The in-lock cost of draw `index` of stream `seed`, in ns: `round(scale_pct / 100 × Q(rank))`.
pub fn draw(seed: u64, index: u64, shape: Shape, scale_pct: u32) -> u64 {
    (f64::from(scale_pct) / 100.0 * quantile(shape, rank(seed, index))).round() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::m1::store::seed;

    /// The nearest-rank `p` percentile of sorted values.
    fn nearest_rank(sorted: &[u64], p: f64) -> u64 {
        let i = (p * sorted.len() as f64).ceil() as usize;
        sorted[i.max(1) - 1]
    }

    /// 100,000 draws at scale 100 on a fixed seed meet the hold budget for both shapes: 99 % ± 0.15 % of them at or below
    /// 5 ms (the ruled p99 as a share; one standard deviation of the share is about 0.03 %), the nearest-rank p99 in
    /// [4.9 ms, 7.25 ms] (that band through Q), the maximum in [19.5 ms, 20 ms], and the p50 at 50 µs ± 10 % (`Design`)
    /// or 2.525 ms ± 2 % (`Uniform`). Every bound is a literal, never computed through `quantile`.
    #[test]
    fn draws_meet_the_hold_budget_on_a_fixed_seed() {
        let s = seed("m2:test");
        for (shape, p50_lo, p50_hi) in [
            (Shape::Design, 45_000, 55_000),
            (Shape::Uniform, 2_474_500, 2_575_500),
        ] {
            let mut v: Vec<u64> = (0..100_000).map(|i| draw(s, i, shape, 100)).collect();
            v.sort_unstable();
            let at_or_below = v.iter().filter(|&&x| x <= 5_000_000).count();
            assert!(
                (98_850..=99_150).contains(&at_or_below),
                "{shape:?}: {at_or_below} of 100,000 at or below 5 ms"
            );
            let p99 = nearest_rank(&v, 0.99);
            assert!(
                (4_900_000..=7_250_000).contains(&p99),
                "{shape:?}: p99 {p99}"
            );
            let max = v[v.len() - 1];
            assert!(
                (19_500_000..=20_000_000).contains(&max),
                "{shape:?}: max {max}"
            );
            let p50 = nearest_rank(&v, 0.5);
            assert!((p50_lo..=p50_hi).contains(&p50), "{shape:?}: p50 {p50}");
        }
    }

    /// Scale 0 makes every draw 0 and scale 50 halves it; a seed and an index give one draw, another seed another
    /// sequence.
    #[test]
    fn draws_scale_with_the_sweep_and_repeat_by_seed() {
        let (s, t) = (seed("m2:last-ack.c100"), seed("m2:last-ack.c50"));
        for shape in [Shape::Design, Shape::Uniform] {
            for i in 0..2_000 {
                assert_eq!(draw(s, i, shape, 0), 0);
                let half = (quantile(shape, rank(s, i)) / 2.0).round() as u64;
                assert_eq!(draw(s, i, shape, 50), half, "{shape:?} {i}");
                assert_eq!(draw(s, i, shape, 100), draw(s, i, shape, 100));
            }
            let a: Vec<u64> = (0..64).map(|i| draw(s, i, shape, 100)).collect();
            let b: Vec<u64> = (0..64).map(|i| draw(t, i, shape, 100)).collect();
            assert_ne!(a, b, "{shape:?}");
        }
        assert!((0..10_000).all(|i| (0.0..1.0).contains(&rank(s, i))));
    }

    /// Each shape's `Q` passes through its anchors and is monotone.
    #[test]
    fn the_quantile_function_passes_through_its_anchors() {
        let close = |a: f64, b: f64| (a - b).abs() <= b.abs() * 1e-9 + 1e-9;
        for (shape, at) in [
            (
                Shape::Design,
                [
                    (0.0, 0.0),
                    (0.5, 50_000.0),
                    (0.99, 5_000_000.0),
                    (1.0, 20_000_000.0),
                ],
            ),
            (
                Shape::Uniform,
                [
                    (0.0, 0.0),
                    (0.5, 5_000_000.0 * 0.5 / 0.99),
                    (0.99, 5_000_000.0),
                    (1.0, 20_000_000.0),
                ],
            ),
        ] {
            for (u, ns) in at {
                assert!(
                    close(quantile(shape, u), ns),
                    "{shape:?} Q({u}) = {}",
                    quantile(shape, u)
                );
            }
            let grid: Vec<f64> = (0..=10_000)
                .map(|i| quantile(shape, f64::from(i) / 10_000.0))
                .collect();
            assert!(
                grid.windows(2).all(|w| w[0] <= w[1]),
                "{shape:?} is monotone"
            );
        }
        assert_eq!(
            (Shape::Design.letter(), Shape::Uniform.letter()),
            ('c', 'a')
        );
        assert_eq!(Shape::from_letter('a'), Some(Shape::Uniform));
        assert_eq!(Shape::from_letter('b'), None);
    }
}
