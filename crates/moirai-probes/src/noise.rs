//! Noise bands, baselines and the nightly regression rule ([MP §6], [60 §5.1] "Noise band"), and the bands of a set
//! of noise runs with their command line, `moirai-probes-bin noise` (measurement 16, [MP §9.7]).

use crate::condition::{Condition, ConditionKind};
use crate::host::HostKind;
use crate::record::{ArmRecord, RunRecord};
use crate::stats::{Statistic, median};
use serde_json::{Value, json};
use std::ffi::OsString;
use std::io::Write;

/// The schema name of a noise-band record ([MP §6.1]).
pub const NOISE_SCHEMA: &str = "moirai-probes/noise/1";

/// The noise band of one gated quantity on one host kind under one condition kind ([MP §6]).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Band {
    /// The measurement's row number.
    pub measurement: u32,
    /// The quantity.
    pub quantity: String,
    /// The arm.
    pub arm: String,
    /// The gated statistic.
    pub statistic: Statistic,
    /// The condition kind.
    pub condition: ConditionKind,
    /// The host kind.
    pub host: HostKind,
    /// max − min of the per-repetition values, in the arm's unit.
    pub band: u64,
    /// The median of the per-repetition values (the relative band is `band / median`).
    pub median: u64,
    /// The noise runs the band was taken over.
    pub runs: u32,
}

/// Why a run may not serve as a noise run on its host ([MP §6.1]): a laptop run must be exit-grade, a hosted run valid
/// and idle or synthetic.
fn noise_run_problem(record: &RunRecord) -> Option<String> {
    match record.host.kind {
        HostKind::Laptop if !record.exit_grade() => Some(format!(
            "a laptop noise run must be exit-grade: {}",
            record.disqualifications().join("; ")
        )),
        HostKind::Hosted if record.condition.kind() == ConditionKind::Loaded => {
            Some("a hosted noise run is idle or synthetic, never loaded".into())
        }
        HostKind::Hosted if !record.valid() => Some(format!(
            "a hosted noise run must be valid: {}",
            record.reasons.join("; ")
        )),
        _ => None,
    }
}

impl Band {
    /// The band of one noise run ([MP §6]): the spread of `statistic` of `arm` over the run's repetitions. Refused
    /// when the run may not serve as a noise run on its host ([MP §6.1]) or has no such arm.
    pub fn of_run(record: &RunRecord, arm: &str, statistic: Statistic) -> Result<Band, String> {
        if let Some(p) = noise_run_problem(record) {
            return Err(p);
        }
        let mut values = record
            .arm(arm)
            .ok_or_else(|| format!("the run has no arm '{arm}'"))?
            .per_rep(statistic);
        let lo = *values.iter().min().ok_or("the run has no repetitions")?;
        let hi = *values.iter().max().ok_or("the run has no repetitions")?;
        Ok(Band {
            measurement: record.measurement,
            quantity: record.quantity.clone(),
            arm: arm.to_string(),
            statistic,
            condition: record.condition.kind(),
            host: record.host.kind,
            band: hi - lo,
            median: median(&mut values).ok_or("the run has no repetitions")?,
            runs: 1,
        })
    }

    /// What the band is the band of.
    fn key(&self) -> (u32, &str, &str, Statistic, ConditionKind, HostKind) {
        (
            self.measurement,
            &self.quantity,
            &self.arm,
            self.statistic,
            self.condition,
            self.host,
        )
    }

    /// Combines the bands of several noise runs of the same quantity, arm, statistic, condition and host: the
    /// largest band, with its run's median ([MP §6]). `None` when empty or when the keys differ.
    pub fn widest(bands: &[Band]) -> Option<Band> {
        let first = bands.first()?;
        if !bands.iter().all(|b| b.key() == first.key()) {
            return None;
        }
        let w = bands.iter().max_by_key(|b| b.band)?;
        Some(Band {
            runs: bands.iter().map(|b| b.runs).sum(),
            ..w.clone()
        })
    }

    /// The relative band in parts per million, for reports; `None` for a zero median.
    pub fn relative_ppm(&self) -> Option<u64> {
        (self.median > 0)
            .then(|| (u128::from(self.band) * 1_000_000 / u128::from(self.median)) as u64)
    }

    /// The JSON form ([MP §6.1], schema [`NOISE_SCHEMA`]).
    pub fn to_json(&self) -> Value {
        json!({
            "schema": NOISE_SCHEMA,
            "measurement": self.measurement,
            "quantity": self.quantity,
            "arm": self.arm,
            "statistic": self.statistic.as_str(),
            "condition": self.condition.as_str(),
            "host": self.host.as_str(),
            "band": self.band,
            "median": self.median,
            "runs": self.runs,
        })
    }

    /// The inverse of [`Band::to_json`], with the rules of [MP §6.1].
    pub fn from_json(v: &Value) -> Result<Band, String> {
        if v.get("schema").and_then(Value::as_str) != Some(NOISE_SCHEMA) {
            return Err(format!("not a {NOISE_SCHEMA} record"));
        }
        let s = |k: &str| {
            v.get(k)
                .and_then(Value::as_str)
                .ok_or_else(|| format!("'{k}' is not a string"))
        };
        let n = |k: &str| {
            v.get(k)
                .and_then(Value::as_u64)
                .ok_or_else(|| format!("'{k}' is not an unsigned integer"))
        };
        let b = Band {
            measurement: u32::try_from(n("measurement")?)
                .map_err(|_| "'measurement' is out of range")?,
            quantity: s("quantity")?.to_string(),
            arm: s("arm")?.to_string(),
            statistic: Statistic::parse(s("statistic")?).ok_or("'statistic' is unknown")?,
            condition: ConditionKind::parse(s("condition")?).ok_or("'condition' is unknown")?,
            host: HostKind::parse(s("host")?).ok_or("'host' is unknown")?,
            band: n("band")?,
            median: n("median")?,
            runs: u32::try_from(n("runs")?).map_err(|_| "'runs' is out of range")?,
        };
        if b.measurement == 0 || b.runs == 0 {
            return Err("a noise band names a measurement and at least one run".into());
        }
        if b.host == HostKind::Hosted && b.condition == ConditionKind::Loaded {
            return Err("a hosted band is idle or synthetic, never loaded".into());
        }
        if b.host == HostKind::Laptop && b.condition == ConditionKind::Synthetic {
            return Err("a laptop band is idle or loaded, never synthetic".into());
        }
        Ok(b)
    }
}

/// The noise bands of a set of noise runs ([MP §6], [MP §6.1]): for every arm of every run and every statistic the arm
/// is gated on, that run's band; the bands of one measurement, quantity, arm, statistic, condition and host combined
/// into the widest ([`Band::widest`]), in the order they first appear. Refused when a run may not serve as a noise run
/// on its host.
pub fn bands(records: &[RunRecord]) -> Result<Vec<Band>, String> {
    let mut out: Vec<Band> = Vec::new();
    for r in records {
        for arm in &r.arms {
            for &statistic in arm.gated() {
                let b = Band::of_run(r, &arm.name, statistic).map_err(|e| {
                    format!("{} '{}' of {}: {e}", r.measurement, r.quantity, r.started)
                })?;
                match out.iter_mut().find(|o| o.key() == b.key()) {
                    Some(o) => *o = Band::widest(&[o.clone(), b]).unwrap_or_else(|| o.clone()),
                    None => out.push(b),
                }
            }
        }
    }
    Ok(out)
}

/// The usage text of `moirai-probes-bin noise` ([MP §9.7]).
pub const NOISE_USAGE: &str = "usage: noise <run record>...
       noise --help
Prints one noise-band record (moirai-probes/noise/1) per gated statistic of every arm of the given run records
(moirai-probes/run/1, one JSON object per file), combining the runs of one quantity, arm, statistic, condition and
host into the widest band (docs/spec/measurement-protocol.md §6). A laptop run must be exit-grade, a hosted run valid
and idle or synthetic. Exit: 0 printed; 1 a record was refused; 2 usage error.
";

/// `moirai-probes-bin noise` ([MP §9.7]): reads the run records named by `args`, prints their bands as JSON lines to
/// `out`, and returns the exit code (0 printed, 1 a record refused, 2 usage error).
pub fn cli(args: &[OsString], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    if args.first().is_some_and(|a| a == "--help") {
        let _ = out.write_all(NOISE_USAGE.as_bytes());
        return 0;
    }
    if args.is_empty() || args.iter().any(|a| a.to_string_lossy().starts_with("--")) {
        let _ = writeln!(err, "noise: give the run records to combine");
        let _ = err.write_all(NOISE_USAGE.as_bytes());
        return 2;
    }
    let mut records = Vec::with_capacity(args.len());
    for a in args {
        let name = a.to_string_lossy();
        let read = std::fs::read(a)
            .map_err(|e| e.to_string())
            .and_then(|b| serde_json::from_slice::<Value>(&b).map_err(|e| e.to_string()))
            .and_then(|v| RunRecord::from_json(&v));
        match read {
            Ok(r) => records.push(r),
            Err(e) => {
                let _ = writeln!(err, "noise: {name}: refused: {e}");
                return 1;
            }
        }
    }
    match bands(&records) {
        Ok(bands) => {
            for b in bands {
                let _ = writeln!(out, "{}", b.to_json());
            }
            0
        }
        Err(e) => {
            let _ = writeln!(err, "noise: refused: {e}");
            1
        }
    }
}

/// Where a nightly value lies against its baseline ([MP §6]).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Drift {
    /// baseline − band ≤ value ≤ baseline + band.
    Within,
    /// value > baseline + band: the nightly run reports a failure.
    Regressed {
        /// value − (baseline + band).
        excess: u64,
    },
    /// value < baseline − band: reported as an improvement; the baseline does not move.
    Improved {
        /// (baseline − band) − value.
        by: u64,
    },
}

/// The regression rule of [MP §6] on bare numbers.
fn drift(baseline: u64, band: u64, value: u64) -> Drift {
    let hi = baseline.saturating_add(band);
    let lo = baseline.saturating_sub(band);
    if value > hi {
        Drift::Regressed { excess: value - hi }
    } else if value < lo {
        Drift::Improved { by: lo - value }
    } else {
        Drift::Within
    }
}

/// A nightly value against its baseline and band ([MP §6]).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Comparison {
    /// The reference run's median of the band's statistic.
    pub baseline: u64,
    /// The band.
    pub band: u64,
    /// The nightly run's median of the same statistic.
    pub value: u64,
    /// Where the value lies.
    pub drift: Drift,
}

/// The band's arm in `record`, or why it cannot be compared.
fn arm_of<'a>(record: &'a RunRecord, band: &Band, which: &str) -> Result<&'a ArmRecord, String> {
    if record.measurement != band.measurement || record.quantity != band.quantity {
        return Err(format!(
            "the {which} run measures {} '{}', the band {} '{}'",
            record.measurement, record.quantity, band.measurement, band.quantity
        ));
    }
    if record.host.kind != HostKind::Laptop || !record.exit_grade() {
        return Err(format!(
            "the {which} run must be an exit-grade laptop run: {}",
            record.disqualifications().join("; ")
        ));
    }
    record
        .arm(&band.arm)
        .ok_or_else(|| format!("the {which} run has no arm '{}'", band.arm))
}

/// Compares a nightly run with its quantity's reference run and laptop noise band ([MP §6]): the baseline is the
/// reference run's median over repetitions of the band's statistic, never the previous night's; the value is the
/// nightly run's. Refused unless both runs are exit-grade laptop runs of the band's measurement and quantity under
/// the same condition, the band is the laptop's for that condition, the arm has the same unit in both runs, and the
/// statistic is one the reference arm is gated on ([MP §6.1], [MP §7.3]).
pub fn compare(
    reference: &RunRecord,
    band: &Band,
    nightly: &RunRecord,
) -> Result<Comparison, String> {
    if band.host != HostKind::Laptop {
        return Err("a hosted band is reported, never used to decide".into());
    }
    let r = arm_of(reference, band, "reference")?;
    let n = arm_of(nightly, band, "nightly")?;
    if reference.condition.kind() != band.condition || nightly.condition != reference.condition {
        return Err(format!(
            "the reference, the band and the nightly run must share one condition (the band's is {})",
            band.condition.as_str()
        ));
    }
    if matches!(reference.condition, Condition::Synthetic { .. }) {
        return Err("a synthetic run never decides".into());
    }
    if r.unit != n.unit {
        return Err(format!(
            "arm {} is in {} in the reference and in {} in the nightly run",
            band.arm,
            r.unit.as_str(),
            n.unit.as_str()
        ));
    }
    if !r.gated().contains(&band.statistic) {
        return Err(format!(
            "arm {} is not gated on {}",
            band.arm,
            band.statistic.as_str()
        ));
    }
    let of =
        |a: &ArmRecord| median(&mut a.per_rep(band.statistic)).ok_or("a run has no repetitions");
    let (baseline, value) = (of(r)?, of(n)?);
    Ok(Comparison {
        baseline,
        band: band.band,
        value,
        drift: drift(baseline, band.band, value),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::tests::sample_record;
    use crate::stats::Summary;
    use proptest::prelude::*;

    #[test]
    fn band_of_a_run() {
        let r = sample_record();
        // The sample record's op arm: p50 of repetition r is 2.009 s + r.
        let b = Band::of_run(&r, "op", Statistic::P50).unwrap();
        assert_eq!((b.band, b.median, b.runs), (2, 2_009_000_001, 1));
        assert_eq!(
            (b.condition, b.host),
            (ConditionKind::Idle, HostKind::Laptop)
        );
        assert_eq!(Band::from_json(&b.to_json()), Ok(b.clone()));
        assert_eq!(b.relative_ppm(), Some(0));
        assert!(Band::of_run(&r, "nope", Statistic::P50).is_err());
        let mut bad = r.clone();
        bad.reasons.push("x".into());
        assert!(Band::of_run(&bad, "op", Statistic::P50).is_err());
        // A valid laptop run with a Defender update during it is not exit-grade, so it gives no band.
        let mut updated = r.clone();
        if let Ok(d) = &mut updated.host.end.defender {
            d.signatures = "1.437.83.0".into();
        }
        assert!(updated.valid());
        assert!(Band::of_run(&updated, "op", Statistic::P50).is_err());
        // A hosted run is never exit-grade; it gives a band when valid.
        let mut hosted = r.clone();
        hosted.host.kind = HostKind::Hosted;
        let h = Band::of_run(&hosted, "op", Statistic::Max).unwrap();
        assert_eq!(h.host, HostKind::Hosted);
        hosted.condition = Condition::Loaded {
            fixture: "0f".repeat(32),
        };
        assert!(Band::of_run(&hosted, "op", Statistic::Max).is_err());
    }

    #[test]
    fn bands_of_noise_runs() {
        let r = sample_record();
        let gated: usize = r.arms.iter().map(|a| a.gated().len()).sum();
        let one = bands(std::slice::from_ref(&r)).unwrap();
        assert_eq!(one.len(), gated);
        assert!(one.iter().all(|b| b.runs == 1));
        let mut spread = r.clone();
        let stat = one[0].statistic;
        let rep0 = &mut spread.arms[0].reps[0].summary;
        match stat {
            Statistic::Max => rep0.max += 1_000,
            Statistic::P95 => rep0.p95 += 1_000,
            _ => rep0.p99 += 1_000,
        }
        let two = bands(&[r.clone(), spread]).unwrap();
        assert_eq!(two.len(), gated);
        assert!(two.iter().all(|b| b.runs == 2));
        assert!(two[0].band > one[0].band, "{two:?}");
        assert_eq!(
            two[1..],
            one[1..]
                .iter()
                .map(|b| Band {
                    runs: 2,
                    ..b.clone()
                })
                .collect::<Vec<_>>()[..]
        );
        let mut bad = r.clone();
        bad.reasons.push("x".into());
        assert!(bands(&[r, bad]).unwrap_err().contains("must be exit-grade"));
    }

    #[test]
    fn noise_command_line() {
        let d = crate::testkit::scratch_dir("noise-cli");
        let f = d.join("run.json");
        std::fs::write(&f, sample_record().to_json().to_string()).unwrap();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let args = [f.clone().into_os_string(), f.into_os_string()];
        assert_eq!(
            cli(&args, &mut out, &mut err),
            0,
            "{}",
            String::from_utf8_lossy(&err)
        );
        let printed: Vec<Band> = String::from_utf8(out)
            .unwrap()
            .lines()
            .map(|l| Band::from_json(&serde_json::from_str(l).unwrap()).unwrap())
            .collect();
        assert!(!printed.is_empty() && printed.iter().all(|b| b.runs == 2));
        let g = d.join("bad.json");
        std::fs::write(&g, "{}").unwrap();
        let mut err = Vec::new();
        assert_eq!(cli(&[g.into_os_string()], &mut Vec::new(), &mut err), 1);
        assert!(String::from_utf8(err).unwrap().contains("refused"));
        assert_eq!(cli(&[], &mut Vec::new(), &mut Vec::new()), 2);
        assert_eq!(cli(&["--x".into()], &mut Vec::new(), &mut Vec::new()), 2);
        let mut out = Vec::new();
        assert_eq!(cli(&["--help".into()], &mut out, &mut Vec::new()), 0);
        assert!(String::from_utf8(out).unwrap().starts_with("usage: noise"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn widest_band() {
        let r = sample_record();
        let a = Band::of_run(&r, "op", Statistic::Max).unwrap();
        let b = Band {
            band: a.band + 10,
            median: 7,
            ..a.clone()
        };
        let w = Band::widest(&[a.clone(), b.clone()]).unwrap();
        assert_eq!((w.band, w.median, w.runs), (b.band, 7, 2));
        let other = Band {
            host: HostKind::Hosted,
            ..a.clone()
        };
        assert_eq!(Band::widest(&[a, other]), None);
        assert_eq!(Band::widest(&[]), None);
    }

    #[test]
    fn noise_records_refuse_unknown_and_excluded_keys() {
        let good = Band::of_run(&sample_record(), "op", Statistic::Max)
            .unwrap()
            .to_json();
        let edits: [(&str, Value); 6] = [
            ("host", json!("desktop")),
            ("condition", json!("busy")),
            ("statistic", json!("p90")),
            ("runs", json!(0)),
            ("condition", json!("synthetic")),
            ("measurement", json!(0)),
        ];
        for (k, v) in edits {
            let mut bad = good.clone();
            bad[k] = v;
            assert!(Band::from_json(&bad).is_err(), "{k}");
        }
        let mut hosted_loaded = good;
        hosted_loaded["host"] = json!("hosted");
        hosted_loaded["condition"] = json!("loaded");
        assert!(Band::from_json(&hosted_loaded).is_err());
    }

    /// The sample record with every repetition's maximum of arm 0 set to `max`.
    fn with_max(max: u64) -> RunRecord {
        let mut r = sample_record();
        for rep in &mut r.arms[0].reps {
            rep.summary = Summary { max, ..rep.summary };
            rep.samples = None;
        }
        r
    }

    #[test]
    fn nightly_runs_are_compared_with_the_reference() {
        let reference = with_max(100);
        let band = Band {
            band: 10,
            ..Band::of_run(&reference, "op", Statistic::Max).unwrap()
        };
        let c = compare(&reference, &band, &with_max(111)).unwrap();
        assert_eq!(
            c,
            Comparison {
                baseline: 100,
                band: 10,
                value: 111,
                drift: Drift::Regressed { excess: 1 }
            }
        );
        assert_eq!(
            compare(&reference, &band, &with_max(110)).unwrap().drift,
            Drift::Within
        );
        assert_eq!(
            compare(&reference, &band, &with_max(85)).unwrap().drift,
            Drift::Improved { by: 5 }
        );

        // Every key is checked.
        let mut other = with_max(100);
        other.quantity = "spawn.signed".into();
        assert!(compare(&reference, &band, &other).is_err(), "quantity");
        let mut hosted = with_max(100);
        hosted.host.kind = HostKind::Hosted;
        assert!(
            compare(&reference, &band, &hosted).is_err(),
            "hosted nightly"
        );
        assert!(
            compare(&hosted, &band, &with_max(100)).is_err(),
            "hosted reference"
        );
        let hosted_band = Band {
            host: HostKind::Hosted,
            ..band.clone()
        };
        assert!(compare(&reference, &hosted_band, &with_max(100)).is_err());
        let mut updated = with_max(100);
        if let Ok(d) = &mut updated.host.end.defender {
            d.signatures = "1.437.83.0".into();
        }
        assert!(
            compare(&reference, &band, &updated).is_err(),
            "not exit-grade"
        );
        let mut loaded = with_max(100);
        loaded.condition = Condition::Loaded {
            fixture: "0f".repeat(32),
        };
        loaded.load_replay_valid = Some(true);
        assert!(compare(&reference, &band, &loaded).is_err(), "condition");
        let loaded_band = Band {
            condition: ConditionKind::Loaded,
            ..band.clone()
        };
        assert!(compare(&reference, &loaded_band, &with_max(100)).is_err());
        let p50_band = Band {
            statistic: Statistic::P50,
            ..band.clone()
        };
        assert!(
            compare(&reference, &p50_band, &with_max(100)).is_err(),
            "not gated on p50"
        );
        let missing = Band {
            arm: "nope".into(),
            ..band.clone()
        };
        assert!(compare(&reference, &missing, &with_max(100)).is_err());
        let mut bytes = with_max(100);
        bytes.arms[0].unit = crate::units::Unit::Bytes;
        assert!(compare(&reference, &band, &bytes).is_err(), "unit");
    }

    #[test]
    fn regression_rule() {
        assert_eq!(drift(100, 10, 110), Drift::Within);
        assert_eq!(drift(100, 10, 90), Drift::Within);
        assert_eq!(drift(100, 10, 111), Drift::Regressed { excess: 1 });
        assert_eq!(drift(100, 10, 85), Drift::Improved { by: 5 });
        assert_eq!(drift(5, 10, 0), Drift::Within);
        assert_eq!(drift(u64::MAX, 1, u64::MAX), Drift::Within);
    }

    proptest! {
        #[test]
        fn drift_is_consistent(baseline in any::<u32>(), band in any::<u32>(), value in any::<u32>()) {
            let (b, w, v) = (u64::from(baseline), u64::from(band), u64::from(value));
            match drift(b, w, v) {
                Drift::Within => prop_assert!(v <= b + w && v + w >= b),
                Drift::Regressed { excess } => prop_assert_eq!(v, b + w + excess),
                Drift::Improved { by } => prop_assert_eq!(v + by + w, b),
            }
        }
    }
}
