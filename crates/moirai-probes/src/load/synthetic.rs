//! Synthetic profiles ([MP §9.6]): deterministic stand-ins for the fixture, for the generator's tests and for the
//! hosted runners' synthetic load (`noise.yml`). A synthetic profile is never the fixture: a run under it is a
//! synthetic run ([MP §2.3]).

use super::fixture::{Profile, Series, Source};
use crate::units::{MB, parse_bytes};

/// The spec of a synthetic profile ([MP §9.6]).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Spec {
    /// Length in seconds, one sample a second.
    pub seconds: u64,
    /// Mean processor time, percent.
    pub cpu: u64,
    /// Mean disk read bytes per second.
    pub read: u64,
    /// Mean disk write bytes per second.
    pub write: u64,
    /// Seconds each level holds.
    pub step: u64,
    /// The generator's seed.
    pub seed: u64,
}

impl Default for Spec {
    fn default() -> Spec {
        Spec {
            seconds: 600,
            cpu: 50,
            read: 20 * MB,
            write: 20 * MB,
            step: 30,
            seed: 1,
        }
    }
}

/// The factors a level takes of its mean ([MP §9.6]).
const FACTORS: [f64; 5] = [0.5, 0.75, 1.0, 1.25, 1.5];
/// The size of every synthetic I/O, 64 KiB.
const IO: f64 = 65_536.0;

impl Spec {
    /// Parses `key=value,...` with the keys `seconds`, `cpu`, `read`, `write`, `step` and `seed`, each at most once;
    /// an omitted key keeps its default ([`Spec::default`]); the empty spec is the default. `read` and `write` are
    /// byte quantities ([MP §8.2]) per second. `seconds` is at least 10, `step` from 1 to `seconds`, `cpu` at most 100.
    pub fn parse(text: &str) -> Result<Spec, String> {
        let mut spec = Spec::default();
        let mut seen: Vec<&str> = Vec::new();
        for item in text.split(',').filter(|s| !s.trim().is_empty()) {
            let (k, v) = item
                .split_once('=')
                .ok_or_else(|| format!("'{item}' is not key=value"))?;
            let (k, v) = (k.trim(), v.trim());
            if seen.contains(&k) {
                return Err(format!("'{k}' is given twice"));
            }
            seen.push(k);
            let int = || {
                v.parse::<u64>()
                    .map_err(|_| format!("{k}: '{v}' is not a whole number"))
            };
            match k {
                "seconds" => spec.seconds = int()?,
                "cpu" => spec.cpu = int()?,
                "read" => spec.read = parse_bytes(v)?,
                "write" => spec.write = parse_bytes(v)?,
                "step" => spec.step = int()?,
                "seed" => spec.seed = int()?,
                _ => return Err(format!("unknown key '{k}'")),
            }
        }
        if spec.seconds < 10 {
            return Err("seconds must be at least 10".into());
        }
        if spec.step == 0 || spec.step > spec.seconds {
            return Err("step must be from 1 to seconds".into());
        }
        if spec.cpu > 100 {
            return Err("cpu is a percentage, at most 100".into());
        }
        Ok(spec)
    }

    /// The canonical form, every key in order: what a synthetic run's description names.
    pub fn canonical(&self) -> String {
        format!(
            "seconds={},cpu={},read={},write={},step={},seed={}",
            self.seconds, self.cpu, self.read, self.write, self.step, self.seed
        )
    }

    /// The profile: piecewise constant, each level holding `step` seconds; in each level processor time, read bytes
    /// and write bytes take their mean times a factor of [`FACTORS`] drawn by xorshift64* from `seed` (processor time
    /// held to 100 %), with every I/O 64 KiB ([MP §9.6]). Refused for a spec [`Spec::parse`] would refuse.
    pub fn profile(&self) -> Result<Profile, String> {
        if self.seconds < 10 || self.step == 0 || self.step > self.seconds || self.cpu > 100 {
            return Err(format!(
                "the synthetic spec {} is out of range",
                self.canonical()
            ));
        }
        let mut state = if self.seed == 0 {
            0x9E37_79B9_7F4A_7C15
        } else {
            self.seed
        };
        let mut factor = || {
            state ^= state >> 12;
            state ^= state << 25;
            state ^= state >> 27;
            FACTORS[(state.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 32) as usize % FACTORS.len()]
        };
        let n = self.seconds as usize;
        let mut s = Series::default();
        let mut level = (0.0, 0.0, 0.0);
        for i in 0..n {
            if (i as u64).is_multiple_of(self.step) {
                level = (
                    (self.cpu as f64 * factor()).min(100.0),
                    self.read as f64 * factor(),
                    self.write as f64 * factor(),
                );
            }
            s.cpu.push(level.0);
            s.read.push(level.1);
            s.write.push(level.2);
            s.reads.push(level.1 / IO);
            s.writes.push(level.2 / IO);
        }
        Profile::new(
            Source::Synthetic {
                spec: self.canonical(),
            },
            1_000,
            (0..n as u64).map(|i| i * 1_000).collect(),
            s,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn parses_and_canonicalises() {
        assert_eq!(Spec::parse("").unwrap(), Spec::default());
        let s = Spec::parse("cpu=30, read=5MB,seconds=120,step=10,write=1MiB,seed=7").unwrap();
        assert_eq!(
            s,
            Spec {
                seconds: 120,
                cpu: 30,
                read: 5 * MB,
                write: 1 << 20,
                step: 10,
                seed: 7
            }
        );
        assert_eq!(
            s.canonical(),
            "seconds=120,cpu=30,read=5000000,write=1048576,step=10,seed=7"
        );
        assert_eq!(Spec::parse(&s.canonical()).unwrap(), s);
        for bad in [
            "cpu",
            "cpu=101",
            "seconds=9",
            "step=0",
            "seconds=20,step=21",
            "x=1",
            "cpu=1,cpu=2",
            "read=lots",
        ] {
            assert!(Spec::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn out_of_range_specs_make_no_profile() {
        assert!(
            Spec {
                seconds: 5,
                ..Spec::default()
            }
            .profile()
            .is_err()
        );
        assert!(
            Spec {
                step: 0,
                ..Spec::default()
            }
            .profile()
            .is_err()
        );
        assert!(
            Spec {
                cpu: 101,
                ..Spec::default()
            }
            .profile()
            .is_err()
        );
        assert_eq!(Spec::default().profile().unwrap().times.len(), 600);
    }

    #[test]
    fn levels_hold_for_a_step() {
        let p = Spec::parse("seconds=60,step=20,cpu=80,read=0,write=10MB,seed=3")
            .unwrap()
            .profile()
            .unwrap();
        assert_eq!(p.times.len(), 60);
        assert_eq!(p.duration_ms(), 60_000);
        assert_eq!(p.interval_ms, 1_000);
        for block in p.cpu.chunks(20) {
            assert!(block.iter().all(|&c| c == block[0]));
        }
        assert!(p.cpu.iter().all(|&c| (40.0..=100.0).contains(&c)));
        assert!(!p.reads_disk());
        assert!(p.write_size.iter().all(|&s| s == 65_536));
        assert_eq!(
            p.source,
            Source::Synthetic {
                spec: "seconds=60,cpu=80,read=0,write=10000000,step=20,seed=3".into()
            }
        );
    }

    /// The first levels of a profile whose means are all 60 (so no processor level reaches 100 %), one level a
    /// second, as (cpu, read, write) factors of their means.
    fn factors(seed: u64, levels: usize) -> Vec<[f64; 3]> {
        let mean = 60.0;
        let p = Spec {
            seconds: 10.max(levels as u64),
            cpu: 60,
            read: 60,
            write: 60,
            step: 1,
            seed,
        }
        .profile()
        .unwrap();
        (0..levels)
            .map(|i| [p.cpu[i] / mean, p.read[i] / mean, p.write[i] / mean])
            .collect()
    }

    #[test]
    fn golden_draws() {
        // Worked out from [MP §9.6] by an independent implementation of its text: xorshift64* (x ^= x >> 12;
        // x ^= x << 25; x ^= x >> 27; index = high 32 bits of x * 0x2545F4914F6CDD1D, mod 5) over
        // FACTORS = {0.5, 0.75, 1, 1.25, 1.5}, drawn per level in the order cpu, read, write. The first nine draws:
        //   seed 1: indices 0 2 0 1 0 1 4 3 0
        //   seed 0 (state 0x9E3779B97F4A7C15): indices 4 2 1 1 3 4 1 3 1
        //   seed 3: indices 1 1 3 1 3 3 2 1 4
        let f = |i: &[usize]| [FACTORS[i[0]], FACTORS[i[1]], FACTORS[i[2]]];
        assert_eq!(
            factors(1, 3),
            vec![f(&[0, 2, 0]), f(&[1, 0, 1]), f(&[4, 3, 0])]
        );
        assert_eq!(
            factors(0, 3),
            vec![f(&[4, 2, 1]), f(&[1, 3, 4]), f(&[1, 3, 1])]
        );
        assert_eq!(
            factors(3, 3),
            vec![f(&[1, 1, 3]), f(&[1, 3, 3]), f(&[2, 1, 4])]
        );
        // Processor time is held to 100 %: seed 0's first level is 1.5 times its mean.
        let p = Spec::parse("cpu=80,seed=0").unwrap().profile().unwrap();
        assert_eq!(p.cpu[0], 100.0);
        // A level holds `step` seconds and draws once.
        let p = Spec::parse("seconds=10,cpu=40,read=1000,write=2000,step=5,seed=1")
            .unwrap()
            .profile()
            .unwrap();
        assert_eq!(
            p.cpu,
            [20.0, 20.0, 20.0, 20.0, 20.0, 30.0, 30.0, 30.0, 30.0, 30.0]
        );
        assert_eq!((p.read[0], p.write[0]), (1_000.0, 1_000.0));
        assert_eq!((p.read[5], p.write[5]), (500.0, 1_500.0));
    }

    proptest! {
        /// A spec gives the same profile every time, and every level is a listed factor of its mean.
        #[test]
        fn deterministic_and_bounded(seconds in 10u64..400, step in 1u64..60, cpu in 0u64..=100,
                                     read in 0u64..100_000_000, seed in any::<u64>()) {
            let spec = Spec { seconds, cpu, read, write: read / 2, step: step.min(seconds), seed };
            let a = spec.profile().unwrap();
            prop_assert_eq!(&a, &spec.profile().unwrap());
            prop_assert_eq!(a.times.len() as u64, seconds);
            for i in 0..a.times.len() {
                prop_assert!(a.cpu[i] <= 100.0);
                prop_assert!(read == 0 || FACTORS.iter().any(|f| (a.read[i] - read as f64 * f).abs() < 1e-6));
                prop_assert!(cpu == 0 || a.cpu[i] == 100.0
                    || FACTORS.iter().any(|f| (a.cpu[i] - cpu as f64 * f).abs() < 1e-9));
            }
        }
    }
}
