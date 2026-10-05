//! The sample-size tiers, the plan of a run and the block length of its interleaving ([MP §3.1–§3.3], [MP §4.3]).

use crate::stats::Statistic;
use crate::units::{MS, SEC};

/// A sample-size tier ([MP §3.1]), selected by the duration of one operation; intervals are half-open.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum Tier {
    /// d < 1 ms: ≥ 10,000 samples, 5 repetitions, gated on p99.
    T1,
    /// 1 ms ≤ d < 50 ms: ≥ 1,000 samples, 5 repetitions, gated on p99.
    T2,
    /// 50 ms ≤ d < 1 s: ≥ 200 samples, 5 repetitions, gated on p95 and the maximum.
    T3,
    /// d ≥ 1 s: ≥ 20 samples, 3 repetitions, gated on the maximum.
    T4,
}

impl Tier {
    /// Every tier, fastest first.
    pub const ALL: [Tier; 4] = [Tier::T1, Tier::T2, Tier::T3, Tier::T4];

    /// The tier of an operation that takes `ns` nanoseconds ([MP §3.1]).
    pub const fn of_duration(ns: u64) -> Tier {
        if ns < MS {
            Tier::T1
        } else if ns < 50 * MS {
            Tier::T2
        } else if ns < SEC {
            Tier::T3
        } else {
            Tier::T4
        }
    }

    /// The fewest samples per repetition the tier allows.
    pub const fn min_samples(self) -> u32 {
        match self {
            Tier::T1 => 10_000,
            Tier::T2 => 1_000,
            Tier::T3 => 200,
            Tier::T4 => 20,
        }
    }

    /// The repetitions the tier takes: 5 below 1 s, 3 above.
    pub const fn repetitions(self) -> u32 {
        match self {
            Tier::T4 => 3,
            _ => 5,
        }
    }

    /// The tier's own plan: its sample requirement and repetitions ([MP §3.1]).
    pub const fn plan(self) -> Plan {
        Plan::new(self.min_samples(), self.repetitions())
    }

    /// The statistics a gate of this tier applies to the median over repetitions.
    pub const fn gated(self) -> &'static [Statistic] {
        match self {
            Tier::T1 | Tier::T2 => &[Statistic::P99],
            Tier::T3 => &[Statistic::P95, Statistic::Max],
            Tier::T4 => &[Statistic::Max],
        }
    }

    /// The record spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Tier::T1 => "t1",
            Tier::T2 => "t2",
            Tier::T3 => "t3",
            Tier::T4 => "t4",
        }
    }

    /// The inverse of [`Tier::as_str`].
    pub fn parse(s: &str) -> Option<Tier> {
        Tier::ALL.into_iter().find(|t| t.as_str() == s)
    }
}

/// The plan every arm of one run follows ([MP §3.2], [MP §4.3]).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Plan {
    /// Samples per arm per repetition.
    pub n: u32,
    /// Repetitions.
    pub repetitions: u32,
    /// Samples per block (`block_len(n)`).
    pub block: u32,
}

impl Plan {
    /// The smallest plan that covers every tier given: the largest sample requirement and the largest repetition
    /// count among them ([MP §3.2]). `None` for no tier.
    pub fn covering(tiers: impl IntoIterator<Item = Tier>) -> Option<Plan> {
        let mut it = tiers.into_iter().peekable();
        it.peek()?;
        let (n, repetitions) = it.fold((0, 0), |(n, r), t| {
            (n.max(t.min_samples()), r.max(t.repetitions()))
        });
        Some(Plan::new(n, repetitions))
    }

    /// A plan with `n` samples and `repetitions` repetitions, and the block length of [MP §4.3].
    pub const fn new(n: u32, repetitions: u32) -> Plan {
        Plan {
            n,
            repetitions,
            block: block_len(n),
        }
    }

    /// Whether this plan meets the requirement of `tier` ([MP §3.1] "covers").
    pub const fn covers(&self, tier: Tier) -> bool {
        self.n >= tier.min_samples() && self.repetitions >= tier.repetitions()
    }

    /// This plan raised to cover `tier` too ([MP §3.3]).
    pub fn raised_to(&self, tier: Tier) -> Plan {
        Plan::new(
            self.n.max(tier.min_samples()),
            self.repetitions.max(tier.repetitions()),
        )
    }

    /// The tier whose own plan this is, if any ([`Tier::plan`]). Every plan the runner makes is one: a covering plan
    /// takes the requirement of the fastest tier it covers, whose repetitions are the largest too, and raising a
    /// tier plan to a tier gives that tier's plan or keeps it ([MP §3.2], [MP §3.3]). A record whose plan is not one
    /// is refused ([MP §7.3]).
    pub fn tier(&self) -> Option<Tier> {
        Tier::ALL.into_iter().find(|t| t.plan() == *self)
    }

    /// The number of rounds per repetition (the last one may be short).
    pub const fn rounds(&self) -> u32 {
        if self.block == 0 {
            0
        } else {
            self.n.div_ceil(self.block)
        }
    }
}

/// The block length B = min(100, max(1, ⌊n/20⌋)) of [MP §4.3]: for n ≥ 20 every repetition then alternates at least
/// 20 times, since n / B ≥ 20.
pub const fn block_len(n: u32) -> u32 {
    let b = n / 20;
    if b > 100 {
        100
    } else if b == 0 {
        1
    } else {
        b
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn boundaries_are_half_open() {
        assert_eq!(Tier::of_duration(0), Tier::T1);
        assert_eq!(Tier::of_duration(MS - 1), Tier::T1);
        assert_eq!(Tier::of_duration(MS), Tier::T2);
        assert_eq!(Tier::of_duration(50 * MS - 1), Tier::T2);
        assert_eq!(Tier::of_duration(50 * MS), Tier::T3);
        assert_eq!(Tier::of_duration(SEC - 1), Tier::T3);
        assert_eq!(Tier::of_duration(SEC), Tier::T4);
        assert_eq!(Tier::of_duration(u64::MAX), Tier::T4);
    }

    #[test]
    fn tier_table() {
        let rows: Vec<_> = Tier::ALL
            .iter()
            .map(|t| {
                (
                    t.as_str(),
                    t.min_samples(),
                    t.repetitions(),
                    t.gated().len(),
                )
            })
            .collect();
        assert_eq!(
            rows,
            [
                ("t1", 10_000, 5, 1),
                ("t2", 1_000, 5, 1),
                ("t3", 200, 5, 2),
                ("t4", 20, 3, 1)
            ]
        );
        for t in Tier::ALL {
            assert_eq!(Tier::parse(t.as_str()), Some(t));
        }
        assert_eq!(Tier::parse("t5"), None);
    }

    #[test]
    fn plans() {
        assert_eq!(Plan::covering([]), None);
        let p = Plan::covering([Tier::T4]).unwrap();
        assert_eq!((p.n, p.repetitions, p.block, p.rounds()), (20, 3, 1, 20));
        let p = Plan::covering([Tier::T4, Tier::T3]).unwrap();
        assert_eq!((p.n, p.repetitions, p.block), (200, 5, 10));
        let p = Plan::covering([Tier::T2, Tier::T1]).unwrap();
        assert_eq!(
            (p.n, p.repetitions, p.block, p.rounds()),
            (10_000, 5, 100, 100)
        );
        assert!(p.covers(Tier::T4) && p.covers(Tier::T1));
        let q = Plan::covering([Tier::T3]).unwrap();
        assert!(!q.covers(Tier::T2) && q.covers(Tier::T4));
        assert_eq!(q.raised_to(Tier::T2), Plan::new(1_000, 5));
        assert_eq!(q.tier(), Some(Tier::T3));
        for t in Tier::ALL {
            assert_eq!(t.plan().tier(), Some(t));
        }
        for not_a_tier_plan in [
            Plan {
                n: 19,
                repetitions: 3,
                block: 1,
            },
            Plan {
                n: 200,
                repetitions: 5,
                block: 7,
            },
            Plan::new(20, 5),
            Plan::new(500, 5),
            Plan::new(10_000, 7),
        ] {
            assert_eq!(not_a_tier_plan.tier(), None, "{not_a_tier_plan:?}");
        }
        assert_eq!(block_len(0), 1);
        assert_eq!(block_len(39), 1);
        assert_eq!(block_len(40), 2);
        assert_eq!(Plan::new(0, 0).rounds(), 0);
        assert_eq!(
            Plan {
                n: 5,
                repetitions: 1,
                block: 0
            }
            .rounds(),
            0
        );
    }

    proptest! {
        #[test]
        fn blocks_alternate_at_least_twenty_times(n in 20u32..200_000) {
            let b = block_len(n);
            prop_assert!((1..=100).contains(&b));
            prop_assert!(Plan::new(n, 5).rounds() >= 20);
        }

        #[test]
        fn covering_plan_covers_exactly_its_tiers(mask in 1u8..16) {
            let tiers: Vec<Tier> = Tier::ALL.iter().copied().enumerate()
                .filter(|(i, _)| mask & (1 << i) != 0).map(|(_, t)| t).collect();
            let p = Plan::covering(tiers.iter().copied()).unwrap();
            for t in &tiers {
                prop_assert!(p.covers(*t));
            }
            let fastest = *tiers.iter().min().unwrap();
            prop_assert_eq!(p, fastest.plan());
            for t in Tier::ALL {
                prop_assert_eq!(p.raised_to(t).tier(), Some(t.min(fastest)));
            }
        }
    }
}
