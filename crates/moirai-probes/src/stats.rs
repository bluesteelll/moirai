//! Nearest-rank percentiles, per-repetition summaries and the median over repetitions ([MP §3.4]).

/// The q-th nearest-rank percentile of ascending-sorted samples ([MP §3.4]): x_k with k = ⌈q·n/100⌉, computed as
/// `(q·n + 99) / 100` in integers. `q` is clamped to 1..=100; `None` for no samples.
pub fn nearest_rank(sorted: &[u64], q: u32) -> Option<u64> {
    let n = sorted.len() as u64;
    if n == 0 {
        return None;
    }
    let q = u64::from(q.clamp(1, 100));
    let k = (q * n).div_ceil(100).max(1);
    Some(sorted[(k - 1) as usize])
}

/// What one arm reports for one repetition ([MP §3.4]).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Summary {
    /// Number of samples.
    pub n: u64,
    /// x₁.
    pub min: u64,
    /// Nearest-rank p50.
    pub p50: u64,
    /// Nearest-rank p95.
    pub p95: u64,
    /// Nearest-rank p99.
    pub p99: u64,
    /// xₙ.
    pub max: u64,
}

impl Summary {
    /// The summary of `samples`, which it sorts in place; `None` when empty.
    pub fn of(samples: &mut [u64]) -> Option<Summary> {
        samples.sort_unstable();
        Some(Summary {
            n: samples.len() as u64,
            min: *samples.first()?,
            p50: nearest_rank(samples, 50)?,
            p95: nearest_rank(samples, 95)?,
            p99: nearest_rank(samples, 99)?,
            max: *samples.last()?,
        })
    }

    /// The value of one statistic.
    pub const fn get(&self, s: Statistic) -> u64 {
        match s {
            Statistic::Min => self.min,
            Statistic::P50 => self.p50,
            Statistic::P95 => self.p95,
            Statistic::P99 => self.p99,
            Statistic::Max => self.max,
        }
    }

    /// The median over repetitions of every statistic ([MP §3.4]); `n` is the median `n` too. `None` when empty.
    pub fn median_over(reps: &[Summary]) -> Option<Summary> {
        let med = |s: Statistic| median(&mut reps.iter().map(|r| r.get(s)).collect::<Vec<_>>());
        Some(Summary {
            n: median(&mut reps.iter().map(|r| r.n).collect::<Vec<_>>())?,
            min: med(Statistic::Min)?,
            p50: med(Statistic::P50)?,
            p95: med(Statistic::P95)?,
            p99: med(Statistic::P99)?,
            max: med(Statistic::Max)?,
        })
    }
}

/// A statistic a gate or a noise band names ([MP §3.1], [MP §5]).
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum Statistic {
    /// The minimum.
    Min,
    /// The median of one repetition's samples.
    P50,
    /// The 95th percentile.
    P95,
    /// The 99th percentile.
    P99,
    /// The maximum.
    Max,
}

impl Statistic {
    /// Every statistic, in report order.
    pub const ALL: [Statistic; 5] = [
        Statistic::Min,
        Statistic::P50,
        Statistic::P95,
        Statistic::P99,
        Statistic::Max,
    ];

    /// The record spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Statistic::Min => "min",
            Statistic::P50 => "p50",
            Statistic::P95 => "p95",
            Statistic::P99 => "p99",
            Statistic::Max => "max",
        }
    }

    /// The inverse of [`Statistic::as_str`].
    pub fn parse(s: &str) -> Option<Statistic> {
        Statistic::ALL.into_iter().find(|x| x.as_str() == s)
    }
}

/// The median of `values` ([MP §3.4]): rank ⌈r/2⌉ of the sorted values, so the middle one for an odd count and the
/// lower middle one otherwise. Sorts in place; `None` when empty.
pub fn median<T: Ord + Copy>(values: &mut [T]) -> Option<T> {
    values.sort_unstable();
    let r = values.len();
    if r == 0 {
        return None;
    }
    Some(values[r.div_ceil(2) - 1])
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn nearest_rank_examples() {
        let v: Vec<u64> = (1..=20).collect();
        assert_eq!(nearest_rank(&v, 50), Some(10));
        assert_eq!(nearest_rank(&v, 95), Some(19));
        assert_eq!(nearest_rank(&v, 99), Some(20));
        assert_eq!(nearest_rank(&v, 100), Some(20));
        let v: Vec<u64> = (1..=1000).collect();
        assert_eq!(nearest_rank(&v, 99), Some(990));
        assert_eq!(nearest_rank(&v, 95), Some(950));
        let v: Vec<u64> = (1..=200).collect();
        assert_eq!(nearest_rank(&v, 95), Some(190));
        assert_eq!(nearest_rank(&[], 50), None);
        assert_eq!(nearest_rank(&[7], 1), Some(7));
    }

    #[test]
    fn medians() {
        assert_eq!(median(&mut [5u64, 1, 3]), Some(3));
        assert_eq!(median(&mut [4u64, 1, 3, 2]), Some(2));
        assert_eq!(median(&mut [9i64, -3, 2, 7, 0]), Some(2));
        assert_eq!(median::<u64>(&mut []), None);
    }

    #[test]
    fn summary_and_median_over() {
        let mut a = vec![3, 1, 2, 5, 4];
        let s = Summary::of(&mut a).unwrap();
        assert_eq!((s.n, s.min, s.p50, s.p95, s.p99, s.max), (5, 1, 3, 5, 5, 5));
        assert_eq!(a, [1, 2, 3, 4, 5]);
        let reps = [
            Summary {
                n: 5,
                min: 1,
                p50: 10,
                p95: 20,
                p99: 30,
                max: 40,
            },
            Summary {
                n: 5,
                min: 2,
                p50: 12,
                p95: 18,
                p99: 35,
                max: 39,
            },
            Summary {
                n: 5,
                min: 3,
                p50: 11,
                p95: 25,
                p99: 31,
                max: 41,
            },
        ];
        let m = Summary::median_over(&reps).unwrap();
        assert_eq!((m.min, m.p50, m.p95, m.p99, m.max), (2, 11, 20, 31, 40));
        assert_eq!(Summary::of(&mut []), None);
        for s in Statistic::ALL {
            assert_eq!(Statistic::parse(s.as_str()), Some(s));
        }
    }

    proptest! {
        #[test]
        fn percentile_is_a_sample_and_monotone(mut v in proptest::collection::vec(any::<u64>(), 1..300)) {
            v.sort_unstable();
            let mut last = 0;
            for q in 1..=100u32 {
                let p = nearest_rank(&v, q).unwrap();
                prop_assert!(v.binary_search(&p).is_ok());
                prop_assert!(p >= last);
                last = p;
                // At least q % of the samples are <= p, and fewer than q % are < p.
                let le = v.iter().filter(|&&x| x <= p).count() as u64;
                let lt = v.iter().filter(|&&x| x < p).count() as u64;
                prop_assert!(le * 100 >= u64::from(q) * v.len() as u64);
                prop_assert!(lt * 100 < u64::from(q) * v.len() as u64);
            }
            prop_assert_eq!(nearest_rank(&v, 100), v.last().copied());
        }

        #[test]
        fn summary_is_ordered(mut v in proptest::collection::vec(any::<u64>(), 1..300)) {
            let s = Summary::of(&mut v).unwrap();
            prop_assert!(s.min <= s.p50 && s.p50 <= s.p95 && s.p95 <= s.p99 && s.p99 <= s.max);
            prop_assert_eq!(s.n, v.len() as u64);
        }

        #[test]
        fn median_splits_in_half(mut v in proptest::collection::vec(any::<u32>(), 1..50)) {
            let m = median(&mut v).unwrap();
            let below = v.iter().filter(|&&x| x < m).count();
            let above = v.iter().filter(|&&x| x > m).count();
            prop_assert!(below < v.len().div_ceil(2));
            prop_assert!(above <= v.len() / 2);
        }
    }
}
