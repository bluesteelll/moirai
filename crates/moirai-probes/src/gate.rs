//! Absolute and floor-relative gates over a run record ([MP §5], [60 §5.3]).

use crate::record::RunRecord;
use crate::stats::{Statistic, median};

/// What a gate compares ([MP §5]).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GateKind {
    /// The median over repetitions of the statistic is at most `budget` (in the arm's unit).
    Absolute {
        /// The budget.
        budget: u64,
    },
    /// Per repetition r, dᵣ = op(r) − factor × floor(r); the median of the dᵣ is at most `allowance`
    /// ([60 §5.3]).
    FloorRelative {
        /// The floor's arm, measured in the same run.
        floor: String,
        /// The factor (1 unless the row states one).
        factor: u32,
        /// The allowance, in the arm's unit; may be negative.
        allowance: i64,
    },
}

/// A gate on one arm's statistic ([MP §5]).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Gate {
    /// The gated arm.
    pub arm: String,
    /// The statistic (for a time arm, one of its gate tier's statistics).
    pub statistic: Statistic,
    /// Absolute or floor-relative.
    pub kind: GateKind,
}

/// The outcome of a gate on one record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GateOutcome {
    /// The compared value: the median of the statistic (absolute) or of the dᵣ (floor-relative).
    pub value: i128,
    /// The bound it is compared with.
    pub bound: i128,
    /// Whether `value ≤ bound`.
    pub holds: bool,
    /// Whether the record may decide the gate ([MP §7.3]): an outcome of a run that is not exit-grade is reported,
    /// never used.
    pub decides: bool,
}

impl Gate {
    /// Evaluates the gate on `record`; an error names a missing arm, a floor in another unit than its operation, or
    /// an empty run.
    pub fn evaluate(&self, record: &RunRecord) -> Result<GateOutcome, String> {
        let arm = record
            .arm(&self.arm)
            .ok_or_else(|| format!("the run has no arm '{}'", self.arm))?;
        let op = arm.per_rep(self.statistic);
        let (value, bound) = match &self.kind {
            GateKind::Absolute { budget } => {
                let m = median(&mut op.clone()).ok_or("the run has no repetitions")?;
                (i128::from(m), i128::from(*budget))
            }
            GateKind::FloorRelative {
                floor,
                factor,
                allowance,
            } => {
                let floor_arm = record
                    .arm(floor)
                    .ok_or_else(|| format!("the run has no floor arm '{floor}'"))?;
                if floor_arm.unit != arm.unit {
                    return Err(format!(
                        "the arm '{}' is in {} and its floor '{floor}' in {}",
                        self.arm,
                        arm.unit.as_str(),
                        floor_arm.unit.as_str()
                    ));
                }
                let f = floor_arm.per_rep(self.statistic);
                if f.len() != op.len() {
                    return Err("the operation and its floor have different repetitions".into());
                }
                let mut d: Vec<i128> = op
                    .iter()
                    .zip(&f)
                    .map(|(&o, &fl)| i128::from(o) - i128::from(*factor) * i128::from(fl))
                    .collect();
                let m = median(&mut d).ok_or("the run has no repetitions")?;
                (m, i128::from(*allowance))
            }
        };
        Ok(GateOutcome {
            value,
            bound,
            holds: value <= bound,
            decides: record.exit_grade(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::tests::sample_record;
    use crate::stats::Summary;
    use crate::units::MS;

    fn set_p99(r: &mut RunRecord, arm: usize, values: [u64; 3]) {
        for (rep, v) in r.arms[arm].reps.iter_mut().zip(values) {
            rep.summary = Summary {
                p99: v,
                ..rep.summary
            };
            rep.samples = None;
        }
    }

    #[test]
    fn absolute_gates_use_the_median_over_repetitions() {
        let mut r = sample_record();
        set_p99(&mut r, 0, [9 * MS, 3 * MS, 5 * MS]);
        let g = |budget| Gate {
            arm: "op".into(),
            statistic: Statistic::P99,
            kind: GateKind::Absolute { budget },
        };
        let o = g(5 * MS).evaluate(&r).unwrap();
        assert_eq!(
            (o.value, o.holds, o.decides),
            (i128::from(5 * MS), true, true)
        );
        assert!(!g(5 * MS - 1).evaluate(&r).unwrap().holds);
        assert!(
            Gate {
                arm: "nope".into(),
                ..g(1)
            }
            .evaluate(&r)
            .is_err()
        );
    }

    #[test]
    fn floor_relative_gates_pair_repetitions() {
        let mut r = sample_record();
        set_p99(&mut r, 0, [10 * MS, 4 * MS, 6 * MS]);
        set_p99(&mut r, 1, [9 * MS, 3 * MS, 6 * MS]);
        // d = 1, 1, 0 ms: median 1 ms.
        let g = |factor, allowance| Gate {
            arm: "op".into(),
            statistic: Statistic::P99,
            kind: GateKind::FloorRelative {
                floor: "floor".into(),
                factor,
                allowance,
            },
        };
        let o = g(1, MS as i64).evaluate(&r).unwrap();
        assert_eq!((o.value, o.holds), (i128::from(MS), true));
        assert!(!g(1, MS as i64 - 1).evaluate(&r).unwrap().holds);
        // 3 × floor: d = −17, −5, −12 ms → median −12 ms.
        let o = g(3, -12 * MS as i64).evaluate(&r).unwrap();
        assert_eq!((o.value, o.holds), (-12 * i128::from(MS), true));
        let missing = Gate {
            kind: GateKind::FloorRelative {
                floor: "x".into(),
                factor: 1,
                allowance: 0,
            },
            ..g(1, 0)
        };
        assert!(missing.evaluate(&r).is_err());
        let mut mixed = r.clone();
        mixed.arms[1].unit = crate::units::Unit::Bytes;
        let e = g(1, MS as i64).evaluate(&mixed).unwrap_err();
        assert!(e.contains("is in ns and its floor 'floor' in bytes"), "{e}");
        r.host.kind = crate::host::HostKind::Hosted;
        assert!(!g(1, MS as i64).evaluate(&r).unwrap().decides);
    }
}
