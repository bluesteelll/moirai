//! The conditions of a run — idle, loaded, synthetic — and the round-boundary memory check ([MP §2.3]).

use crate::units::{GB, format_gb};
use moirai_vfs::MeterError;
use serde_json::{Value, json};

/// The floor of available physical memory under the idle condition: the guard's default RAM floor, 1.5 GB
/// ([MP §2.3], [MP §8.1]). It belongs to the protocol, so a `--ram-floor` override of the guard does not move it, and
/// a record's reasons can be recomputed when it is read back ([MP §7.3]).
pub const IDLE_FLOOR: u64 = crate::guard::RAM_FLOOR;
/// The available physical memory the load generator holds ([60 §5.1]: "≈ 1.8 GB").
pub const LOADED_TARGET: u64 = 1_800_000_000;
/// The lower edge of the loaded band ([MP §2.3]).
pub const LOADED_LOW: u64 = 1_600_000_000;
/// The upper edge of the loaded band ([MP §2.3]).
pub const LOADED_HIGH: u64 = 2 * GB;

/// The kind of a condition ([MP §2.3]): the record spelling of [MP §7.1] and the key of a noise band ([MP §6]).
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum ConditionKind {
    /// No user process beyond the OS baseline.
    Idle,
    /// The 16-agent load fixture.
    Loaded,
    /// A load that is not the fixture.
    Synthetic,
}

impl ConditionKind {
    /// Every kind.
    pub const ALL: [ConditionKind; 3] = [
        ConditionKind::Idle,
        ConditionKind::Loaded,
        ConditionKind::Synthetic,
    ];

    /// The record spelling ([MP §7.1]), also used in raw file names ([MP §7.2]).
    pub const fn as_str(self) -> &'static str {
        match self {
            ConditionKind::Idle => "idle",
            ConditionKind::Loaded => "loaded",
            ConditionKind::Synthetic => "synthetic",
        }
    }

    /// The inverse of [`ConditionKind::as_str`].
    pub fn parse(s: &str) -> Option<ConditionKind> {
        ConditionKind::ALL.into_iter().find(|k| k.as_str() == s)
    }
}

/// The condition a run is made under ([MP §2.3]).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Condition {
    /// No user process beyond the OS baseline; attested by the driver.
    Idle,
    /// The 16-agent load fixture replayed by `moirai-probes-bin loadgen`; `fixture` is the BLAKE3-256 of the
    /// fixture file in lower-case hex.
    Loaded {
        /// The fixture's BLAKE3-256, 64 lower-case hex digits.
        fixture: String,
    },
    /// A load that is not the fixture (the hosted runners' `noise.yml`); never exit-grade.
    Synthetic {
        /// What the load is.
        description: String,
    },
}

impl Condition {
    /// The kind.
    pub const fn kind(&self) -> ConditionKind {
        match self {
            Condition::Idle => ConditionKind::Idle,
            Condition::Loaded { .. } => ConditionKind::Loaded,
            Condition::Synthetic { .. } => ConditionKind::Synthetic,
        }
    }

    /// Why the condition is malformed, if it is: a loaded fixture id must be 64 lower-case hex digits and a synthetic
    /// description non-empty.
    pub fn problem(&self) -> Option<String> {
        match self {
            Condition::Idle => None,
            Condition::Loaded { fixture } => (fixture.len() != 64
                || !fixture
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
            .then(|| {
                format!("the loaded fixture id '{fixture}' is not a BLAKE3-256 in lower-case hex")
            }),
            Condition::Synthetic { description } => description
                .trim()
                .is_empty()
                .then(|| "a synthetic condition needs a description".to_string()),
        }
    }

    /// Whether one available-physical reading breaks this condition's band ([MP §2.3]).
    pub const fn out_of_band(&self, available: u64) -> bool {
        match self {
            Condition::Idle => available < IDLE_FLOOR,
            Condition::Loaded { .. } => available < LOADED_LOW || available > LOADED_HIGH,
            Condition::Synthetic { .. } => false,
        }
    }

    /// Whether the band meets the interval [lo, hi], so a reading in the band can lie between the two.
    const fn band_meets(&self, lo: u64, hi: u64) -> bool {
        match self {
            Condition::Idle => hi >= IDLE_FLOOR,
            Condition::Loaded { .. } => lo <= LOADED_HIGH && hi >= LOADED_LOW,
            Condition::Synthetic { .. } => true,
        }
    }

    /// The band in words, for a validity reason.
    fn band_text(&self) -> String {
        match self {
            Condition::Idle => format!("at least {}", format_gb(IDLE_FLOOR)),
            Condition::Loaded { .. } => {
                format!("[{}, {}]", format_gb(LOADED_LOW), format_gb(LOADED_HIGH))
            }
            Condition::Synthetic { .. } => "any value".to_string(),
        }
    }

    /// The JSON form of [MP §7.1] `condition`.
    pub fn to_json(&self) -> Value {
        match self {
            Condition::Idle => json!({ "kind": "idle" }),
            Condition::Loaded { fixture } => json!({ "kind": "loaded", "fixture": fixture }),
            Condition::Synthetic { description } => {
                json!({ "kind": "synthetic", "description": description })
            }
        }
    }

    /// The inverse of [`Condition::to_json`]; the condition is not checked here ([`Condition::problem`]).
    pub fn from_json(v: &Value) -> Result<Condition, String> {
        let text = |k: &str| {
            v.get(k)
                .and_then(Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| format!("condition: '{k}' is not a string"))
        };
        match v
            .get("kind")
            .and_then(Value::as_str)
            .and_then(ConditionKind::parse)
        {
            Some(ConditionKind::Idle) => Ok(Condition::Idle),
            Some(ConditionKind::Loaded) => Ok(Condition::Loaded {
                fixture: text("fixture")?,
            }),
            Some(ConditionKind::Synthetic) => Ok(Condition::Synthetic {
                description: text("description")?,
            }),
            None => Err(format!("unknown condition kind {:?}", v.get("kind"))),
        }
    }
}

/// The round-boundary readings of `Meter::available_physical` over a run ([MP §4.3], [MP §7.1] `memory`).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MemoryWatch {
    /// Readings taken, failed ones included.
    pub readings: u64,
    /// Readings that failed.
    pub failures: u64,
    /// Successful readings outside the condition's band.
    pub out_of_band: u64,
    /// The smallest successful reading.
    pub min: Option<u64>,
    /// The largest successful reading.
    pub max: Option<u64>,
    /// The error of the first failed reading, for the validity reason; recorded as `first_failure` ([MP §7.1]).
    pub first_failure: Option<String>,
}

impl MemoryWatch {
    /// Records one reading under `condition`.
    pub fn observe(&mut self, condition: &Condition, reading: Result<u64, MeterError>) {
        self.readings += 1;
        match reading {
            Ok(v) => {
                self.min = Some(self.min.map_or(v, |m| m.min(v)));
                self.max = Some(self.max.map_or(v, |m| m.max(v)));
                if condition.out_of_band(v) {
                    self.out_of_band += 1;
                }
            }
            Err(e) => {
                self.failures += 1;
                if self.first_failure.is_none() {
                    self.first_failure = Some(e.to_string());
                }
            }
        }
    }

    /// The validity reasons these readings give under `condition` ([MP §2.3], [MP §7.3]): a failed reading, or a
    /// reading outside the band, invalidates an idle or loaded run; a synthetic run is only recorded.
    pub fn reasons(&self, condition: &Condition) -> Vec<String> {
        let mut out = Vec::new();
        if matches!(condition, Condition::Synthetic { .. }) {
            return out;
        }
        if self.failures > 0 {
            out.push(format!(
                "{} of {} round-boundary readings of available physical memory failed (first: {})",
                self.failures,
                self.readings,
                self.first_failure.as_deref().unwrap_or("unknown")
            ));
        }
        if self.out_of_band > 0 {
            out.push(format!(
                "{} of {} round-boundary readings of available physical memory were outside {} (min {}, max {})",
                self.out_of_band,
                self.readings,
                condition.band_text(),
                self.min.map_or_else(|| "-".to_string(), format_gb),
                self.max.map_or_else(|| "-".to_string(), format_gb),
            ));
        }
        if self.readings == 0 {
            out.push(
                "no round-boundary reading of available physical memory was taken".to_string(),
            );
        }
        out
    }

    /// Whether the counts agree with each other and with `min` and `max` under `condition`, and number `expected`
    /// readings ([MP §7.3]); a record that fails is refused when it is read back.
    pub fn check(&self, condition: &Condition, expected: u64) -> Result<(), String> {
        if self.readings != expected {
            return Err(format!(
                "memory: {} readings, the protocol takes {expected}",
                self.readings
            ));
        }
        if self
            .failures
            .checked_add(self.out_of_band)
            .is_none_or(|bad| bad > self.readings)
        {
            return Err("memory: more failed and out-of-band readings than readings".into());
        }
        if self.first_failure.is_some() != (self.failures > 0) {
            return Err("memory: 'first_failure' is set exactly when a reading failed".into());
        }
        let ok = self.readings - self.failures;
        let (lo, hi) = match (ok, self.min, self.max) {
            (0, None, None) => return Ok(()),
            (1.., Some(lo), Some(hi)) if lo == hi || (lo < hi && ok > 1) => (lo, hi),
            _ => {
                return Err(
                    "memory: 'min' and 'max' do not match the successful readings".to_string(),
                );
            }
        };
        let (lo_out, hi_out) = (condition.out_of_band(lo), condition.out_of_band(hi));
        let consistent = if self.out_of_band == 0 {
            !lo_out && !hi_out
        } else if self.out_of_band == ok {
            lo_out && hi_out
        } else {
            (lo_out || hi_out) && condition.band_meets(lo, hi)
        };
        if consistent {
            Ok(())
        } else {
            Err(format!(
                "memory: {} of {ok} successful readings out of band contradicts min {lo} and max {hi} under the {} \
                 band",
                self.out_of_band,
                condition.kind().as_str()
            ))
        }
    }

    /// The JSON form of [MP §7.1] `memory`.
    pub fn to_json(&self) -> Value {
        json!({
            "readings": self.readings,
            "failures": self.failures,
            "out_of_band": self.out_of_band,
            "min": self.min,
            "max": self.max,
            "first_failure": self.first_failure,
        })
    }

    /// The inverse of [`MemoryWatch::to_json`]; the counts are checked by [`MemoryWatch::check`].
    pub fn from_json(v: &Value) -> Result<MemoryWatch, String> {
        let count = |k: &str| {
            v.get(k)
                .and_then(Value::as_u64)
                .ok_or_else(|| format!("memory: '{k}' is not an unsigned integer"))
        };
        let bytes = |k: &str| match v.get(k) {
            None | Some(Value::Null) => Ok(None),
            Some(x) => x
                .as_u64()
                .map(Some)
                .ok_or_else(|| format!("memory: '{k}' is not an unsigned integer or null")),
        };
        Ok(MemoryWatch {
            readings: count("readings")?,
            failures: count("failures")?,
            out_of_band: count("out_of_band")?,
            min: bytes("min")?,
            max: bytes("max")?,
            first_failure: match v.get("first_failure") {
                None | Some(Value::Null) => None,
                Some(Value::String(s)) => Some(s.clone()),
                Some(_) => return Err("memory: 'first_failure' is not a string or null".into()),
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::meter_err;

    fn fixture() -> Condition {
        Condition::Loaded {
            fixture: "ab".repeat(32),
        }
    }

    #[test]
    fn kinds_and_problems() {
        assert_eq!(Condition::Idle.kind(), ConditionKind::Idle);
        assert_eq!(fixture().kind(), ConditionKind::Loaded);
        assert_eq!(fixture().problem(), None);
        for k in ConditionKind::ALL {
            assert_eq!(ConditionKind::parse(k.as_str()), Some(k));
        }
        assert_eq!(ConditionKind::parse("busy"), None);
        assert!(
            Condition::Loaded {
                fixture: "AB".repeat(32)
            }
            .problem()
            .is_some()
        );
        assert!(
            Condition::Loaded {
                fixture: "ab".into()
            }
            .problem()
            .is_some()
        );
        let s = Condition::Synthetic {
            description: " ".into(),
        };
        assert_eq!(s.kind().as_str(), "synthetic");
        assert!(s.problem().is_some());
        let s = Condition::Synthetic {
            description: "stress".into(),
        };
        assert_eq!(s.problem(), None);
        for c in [Condition::Idle, fixture(), s] {
            assert_eq!(Condition::from_json(&c.to_json()), Ok(c));
        }
        assert!(Condition::from_json(&json!({"kind": "busy"})).is_err());
        assert!(Condition::from_json(&json!({"kind": "loaded"})).is_err());
    }

    #[test]
    fn bands() {
        assert_eq!(IDLE_FLOOR, crate::guard::RAM_FLOOR);
        assert!(Condition::Idle.out_of_band(IDLE_FLOOR - 1));
        assert!(!Condition::Idle.out_of_band(IDLE_FLOOR));
        let l = fixture();
        assert!(l.out_of_band(LOADED_LOW - 1) && l.out_of_band(LOADED_HIGH + 1));
        assert!(
            !l.out_of_band(LOADED_LOW)
                && !l.out_of_band(LOADED_TARGET)
                && !l.out_of_band(LOADED_HIGH)
        );
        assert!(
            !Condition::Synthetic {
                description: "x".into()
            }
            .out_of_band(0)
        );
    }

    #[test]
    fn watch_reasons_and_json() {
        let l = fixture();
        let mut w = MemoryWatch::default();
        assert_eq!(w.reasons(&l).len(), 1, "no readings is a reason");
        w.observe(&l, Ok(LOADED_TARGET));
        assert!(w.reasons(&l).is_empty());
        w.observe(&l, Ok(LOADED_HIGH + 1));
        w.observe(&l, Err(meter_err("GlobalMemoryStatusEx")));
        let r = w.reasons(&l);
        assert_eq!(r.len(), 2, "{r:?}");
        assert!(r[0].contains("1 of 3") && r[0].contains("GlobalMemoryStatusEx"));
        assert!(r[1].contains("outside [1.600 GB, 2.000 GB]"));
        assert_eq!((w.min, w.max), (Some(LOADED_TARGET), Some(LOADED_HIGH + 1)));
        assert_eq!(w.check(&l, 3), Ok(()));
        assert_eq!(MemoryWatch::from_json(&w.to_json()), Ok(w.clone()));
        let s = Condition::Synthetic {
            description: "x".into(),
        };
        assert!(w.reasons(&s).is_empty());
    }

    /// A watch built by `observe` from `values` (`None` is a failed reading).
    fn watched(c: &Condition, values: &[Option<u64>]) -> MemoryWatch {
        let mut w = MemoryWatch::default();
        for v in values {
            w.observe(c, v.ok_or_else(|| meter_err("read")));
        }
        w
    }

    #[test]
    fn watch_check_refuses_inconsistent_counts() {
        let idle = Condition::Idle;
        let good = watched(&idle, &[Some(9 * GB), Some(IDLE_FLOOR - 1), None]);
        assert_eq!(good.check(&idle, 3), Ok(()));
        assert!(good.check(&idle, 4).is_err(), "reading count");
        let broken = [
            MemoryWatch {
                out_of_band: 0,
                ..good.clone()
            },
            MemoryWatch {
                failures: 3,
                ..good.clone()
            },
            MemoryWatch {
                first_failure: None,
                ..good.clone()
            },
            MemoryWatch {
                min: None,
                ..good.clone()
            },
            MemoryWatch {
                min: Some(10 * GB),
                ..good.clone()
            },
            MemoryWatch {
                out_of_band: 2,
                ..good.clone()
            },
        ];
        for b in broken {
            assert!(b.check(&idle, 3).is_err(), "{b:?}");
        }
        // Every reading in band, but a minimum below the floor.
        let w = watched(&idle, &[Some(2 * GB), Some(3 * GB)]);
        assert!(
            MemoryWatch {
                min: Some(GB),
                ..w.clone()
            }
            .check(&idle, 2)
            .is_err()
        );
        // All out of band, but a maximum in band.
        let w = watched(&idle, &[Some(GB), Some(GB / 2)]);
        assert_eq!(w.check(&idle, 2), Ok(()));
        assert!(
            MemoryWatch {
                max: Some(2 * GB),
                ..w
            }
            .check(&idle, 2)
            .is_err()
        );
        // Loaded: one in band and one above it; the extremes must straddle the band.
        let l = fixture();
        let w = watched(&l, &[Some(LOADED_TARGET), Some(3 * GB)]);
        assert_eq!(w.check(&l, 2), Ok(()));
        assert!(
            MemoryWatch {
                min: Some(LOADED_HIGH + 1),
                ..w
            }
            .check(&l, 2)
            .is_err()
        );
        // Nothing is out of band under a synthetic load; no successful reading leaves min and max null.
        let s = Condition::Synthetic {
            description: "x".into(),
        };
        let w = watched(&s, &[Some(1)]);
        assert_eq!(w.check(&s, 1), Ok(()));
        assert!(
            MemoryWatch {
                out_of_band: 1,
                ..w
            }
            .check(&s, 1)
            .is_err()
        );
        let w = watched(&s, &[None]);
        assert_eq!(w.check(&s, 1), Ok(()));
        assert!(
            MemoryWatch {
                min: Some(1),
                max: Some(1),
                ..w
            }
            .check(&s, 1)
            .is_err()
        );
        assert!(
            MemoryWatch::from_json(
                &json!({"readings": 1, "failures": 0, "out_of_band": 0, "first_failure": 3})
            )
            .is_err()
        );
    }
}
