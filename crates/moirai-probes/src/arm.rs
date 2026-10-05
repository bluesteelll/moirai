//! Arms, the timer and its resolution, and batching ([MP §4.1], [MP §4.4], [MP §4.5]).

use crate::units::Unit;
use std::time::Instant;

/// Why one sample of an arm could not be taken; the run stops with it ([MP §4.1]).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArmError(pub String);

impl core::fmt::Display for ArmError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ArmError {}

/// One arm of a run: the measured operation or one of its floors ([MP §4.1]).
pub trait Arm {
    /// The arm's name (1–64 bytes of `[a-z0-9._-]`, starting with a letter or digit).
    fn name(&self) -> &str;
    /// The unit of its samples.
    fn unit(&self) -> Unit;
    /// Announces that the next `len` samples form one block ([MP §4.3]); the pilot is announced as one block of
    /// [`crate::run::PILOT_SAMPLES`] ([MP §4.2]). An arm that takes a block's samples in one go — a hyperfine
    /// invocation with `--runs <len>` ([MP §4.9]) — takes them here and returns them from [`Arm::sample`] in order;
    /// samples of an earlier block that were never read are discarded. Arms that sample one operation at a time
    /// ignore it.
    fn begin_block(&mut self, len: u32) -> Result<(), ArmError> {
        let _ = len;
        Ok(())
    }
    /// Takes one sample: one operation, or for a time arm with a batch of k, the mean of k consecutive operations
    /// ([MP §4.5]). Setup and teardown that are not part of the operation stay outside the timed region.
    fn sample(&mut self) -> Result<u64, ArmError>;
    /// Sets the batch of a time arm ([MP §4.5]); the runner calls it only for [`Unit::Ns`] arms, with 1 before the
    /// pilot and with [`batch_for`] after it, and refuses the run when [`Arm::batch`] then differs from what it set.
    /// An arm that cannot batch keeps 1, so it can only be measured where [`batch_for`] gives 1.
    fn set_batch(&mut self, k: u32) {
        let _ = k;
    }
    /// The batch the arm uses.
    fn batch(&self) -> u32 {
        1
    }
}

/// A monotonic clock in whole nanoseconds ([MP §4.4]).
pub trait Ticker {
    /// Nanoseconds since an arbitrary origin; never decreases.
    fn now_ns(&mut self) -> u64;
}

/// The process's monotonic clock (`std::time::Instant`; `QueryPerformanceCounter` on Windows) ([MP §4.4]).
#[derive(Clone, Copy, Debug)]
pub struct MonoTicker {
    origin: Instant,
}

impl MonoTicker {
    /// A ticker whose origin is now.
    pub fn new() -> MonoTicker {
        MonoTicker {
            origin: Instant::now(),
        }
    }
}

impl Default for MonoTicker {
    fn default() -> MonoTicker {
        MonoTicker::new()
    }
}

impl Ticker for MonoTicker {
    fn now_ns(&mut self) -> u64 {
        u64::try_from(self.origin.elapsed().as_nanos()).unwrap_or(u64::MAX)
    }
}

/// The readings [`timer_resolution`] takes ([MP §4.4]).
pub const RESOLUTION_READINGS: u32 = 10_000;

/// The timer's resolution ([MP §4.4]): the smallest positive difference between consecutive readings over
/// [`RESOLUTION_READINGS`] readings; `None` if the timer never advanced.
pub fn timer_resolution<T: Ticker + ?Sized>(t: &mut T) -> Option<u64> {
    let mut last = t.now_ns();
    let mut best: Option<u64> = None;
    for _ in 1..RESOLUTION_READINGS {
        let now = t.now_ns();
        let step = now.saturating_sub(last);
        if step > 0 {
            best = Some(best.map_or(step, |b| b.min(step)));
        }
        last = now;
    }
    best
}

/// The batch of [MP §4.5]: 1 when the pilot median reaches 20 × resolution, else ⌈20 × resolution / max(median, 1)⌉.
pub fn batch_for(pilot_median_ns: u64, resolution_ns: u64) -> u32 {
    let threshold = u128::from(resolution_ns) * 20;
    let median = u128::from(pilot_median_ns.max(1));
    if u128::from(pilot_median_ns) >= threshold {
        1
    } else {
        u32::try_from(threshold.div_ceil(median)).unwrap_or(u32::MAX)
    }
}

/// A time arm over a closure: each sample times `batch` calls and records the per-call mean, rounded down
/// ([MP §4.1], [MP §4.5]).
///
/// Each call's result goes through [`std::hint::black_box`], so a release build cannot drop a pure operation whose
/// result is unused, which would leave a batch timing nothing. A probe body therefore returns what it computes (or
/// passes it to `black_box` itself) rather than discarding it.
pub struct TimedFn<F, T> {
    name: String,
    op: F,
    ticker: T,
    batch: u32,
}

impl<F, T, O> TimedFn<F, T>
where
    F: FnMut() -> Result<O, ArmError>,
    T: Ticker,
{
    /// A time arm named `name` that times `op` with `ticker`.
    pub fn new(name: impl Into<String>, ticker: T, op: F) -> TimedFn<F, T> {
        TimedFn {
            name: name.into(),
            op,
            ticker,
            batch: 1,
        }
    }
}

impl<F, T, O> Arm for TimedFn<F, T>
where
    F: FnMut() -> Result<O, ArmError>,
    T: Ticker,
{
    fn name(&self) -> &str {
        &self.name
    }

    fn unit(&self) -> Unit {
        Unit::Ns
    }

    fn sample(&mut self) -> Result<u64, ArmError> {
        let t0 = self.ticker.now_ns();
        for _ in 0..self.batch {
            std::hint::black_box((self.op)()?);
        }
        let t1 = self.ticker.now_ns();
        Ok(t1.saturating_sub(t0) / u64::from(self.batch))
    }

    fn set_batch(&mut self, k: u32) {
        self.batch = k.max(1);
    }

    fn batch(&self) -> u32 {
        self.batch
    }
}

/// A bytes or counts arm over a closure that returns the sample itself ([MP §4.1], [MP §4.6], [MP §4.8]).
pub struct ValueFn<F> {
    name: String,
    unit: Unit,
    op: F,
}

impl<F> ValueFn<F>
where
    F: FnMut() -> Result<u64, ArmError>,
{
    /// A value arm named `name` in `unit`.
    pub fn new(name: impl Into<String>, unit: Unit, op: F) -> ValueFn<F> {
        ValueFn {
            name: name.into(),
            unit,
            op,
        }
    }
}

impl<F> Arm for ValueFn<F>
where
    F: FnMut() -> Result<u64, ArmError>,
{
    fn name(&self) -> &str {
        &self.name
    }

    fn unit(&self) -> Unit {
        self.unit
    }

    fn sample(&mut self) -> Result<u64, ArmError> {
        (self.op)()
    }
}

/// A time arm whose samples are taken a block at a time ([MP §4.9]): at each announced block of `len` samples,
/// `block(len)` returns exactly `len` per-operation times in nanoseconds (for a spawn arm, the runs of one
/// `hyperfine -N --runs <len> --warmup 5 --export-json` invocation, read with [`hyperfine_times`]), and
/// [`Arm::sample`] returns them in order. It cannot batch, so its operations must take at least 20 × the timer's
/// resolution ([MP §4.5]), which every spawn does.
pub struct BulkFn<F> {
    name: String,
    block: F,
    taken: Vec<u64>,
    next: usize,
}

impl<F> BulkFn<F>
where
    F: FnMut(u32) -> Result<Vec<u64>, ArmError>,
{
    /// A bulk time arm named `name` over `block`.
    pub fn new(name: impl Into<String>, block: F) -> BulkFn<F> {
        BulkFn {
            name: name.into(),
            block,
            taken: Vec::new(),
            next: 0,
        }
    }
}

impl<F> Arm for BulkFn<F>
where
    F: FnMut(u32) -> Result<Vec<u64>, ArmError>,
{
    fn name(&self) -> &str {
        &self.name
    }

    fn unit(&self) -> Unit {
        Unit::Ns
    }

    fn begin_block(&mut self, len: u32) -> Result<(), ArmError> {
        self.taken.clear();
        self.next = 0;
        let times = (self.block)(len)?;
        if times.len() != len as usize {
            return Err(ArmError(format!(
                "a block of {len} samples returned {} times",
                times.len()
            )));
        }
        self.taken = times;
        Ok(())
    }

    fn sample(&mut self) -> Result<u64, ArmError> {
        let v = *self
            .taken
            .get(self.next)
            .ok_or_else(|| ArmError("a sample was read beyond its block".into()))?;
        self.next += 1;
        Ok(v)
    }
}

/// The per-run times of a hyperfine export (`--export-json`), `results[0].times` in seconds, rounded to the nearest
/// nanosecond ([MP §4.9]); the samples of one block of a spawn arm.
pub fn hyperfine_times(export: &[u8]) -> Result<Vec<u64>, ArmError> {
    let bad = |why: &str| ArmError(format!("hyperfine export: {why}"));
    let v: serde_json::Value =
        serde_json::from_slice(export).map_err(|e| bad(&format!("not JSON: {e}")))?;
    let times = v
        .get("results")
        .and_then(|r| r.get(0))
        .and_then(|r| r.get("times"))
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| bad("no results[0].times array"))?;
    times
        .iter()
        .map(|t| {
            let s = t
                .as_f64()
                .filter(|s| s.is_finite() && *s >= 0.0)
                .ok_or_else(|| bad("a time is not a non-negative number"))?;
            let ns = (s * 1e9).round();
            if ns >= u64::MAX as f64 {
                return Err(bad("a time is out of range"));
            }
            Ok(ns as u64)
        })
        .collect()
}

/// Whether `s` is a valid quantity or arm name ([MP §4.1]): 1–64 bytes of `[a-z0-9._-]`, the first a letter or
/// digit.
pub fn valid_name(s: &str) -> bool {
    let b = s.as_bytes();
    (1..=64).contains(&b.len())
        && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b.iter().all(|&c| {
            c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'.' | b'_' | b'-')
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::FakeTicker;

    #[test]
    fn resolution_is_the_smallest_step() {
        let mut t = FakeTicker::stepping(&[0, 300, 100, 0, 200]);
        assert_eq!(timer_resolution(&mut t), Some(100));
        let mut still = FakeTicker::stepping(&[0]);
        assert_eq!(timer_resolution(&mut still), None);
        let mut real = MonoTicker::new();
        assert!(timer_resolution(&mut real).is_some_and(|r| r > 0));
    }

    #[test]
    fn batches() {
        assert_eq!(batch_for(2_000, 100), 1);
        assert_eq!(batch_for(1_999, 100), 2);
        assert_eq!(batch_for(100, 100), 20);
        assert_eq!(batch_for(0, 100), 2_000);
        assert_eq!(batch_for(5, 0), 1);
        assert_eq!(batch_for(0, u64::MAX), u32::MAX);
    }

    #[test]
    fn timed_arm_records_the_per_call_mean() {
        let clock = FakeTicker::manual();
        let c = clock.clone();
        let mut arm = TimedFn::new("op", clock.clone(), move || {
            c.advance(7);
            Ok(())
        });
        assert_eq!(arm.sample(), Ok(7));
        arm.set_batch(4);
        assert_eq!(arm.batch(), 4);
        assert_eq!(arm.sample(), Ok(7));
        arm.set_batch(0);
        assert_eq!(arm.batch(), 1);
        assert_eq!((arm.name(), arm.unit()), ("op", Unit::Ns));
        assert_eq!(
            arm.begin_block(10),
            Ok(()),
            "a per-operation arm ignores blocks"
        );
        let mut failing = TimedFn::new("bad", clock.clone(), || -> Result<(), ArmError> {
            Err(ArmError("boom".into()))
        });
        assert_eq!(failing.sample(), Err(ArmError("boom".into())));
        // The operation's result is kept (black-boxed), whatever its type.
        let c = clock.clone();
        let mut sums = TimedFn::new("sum", clock, move || {
            c.advance(3);
            Ok((0..100u64).sum::<u64>())
        });
        sums.set_batch(2);
        assert_eq!(sums.sample(), Ok(3));
    }

    #[test]
    fn bulk_arm_returns_each_block_in_order() {
        let mut lens = Vec::new();
        let mut next = 0u64;
        let mut arm = BulkFn::new("spawn.empty", |len| {
            lens.push(len);
            Ok((0..len)
                .map(|_| {
                    next += 1;
                    next
                })
                .collect())
        });
        assert_eq!(
            (arm.name(), arm.unit(), arm.batch()),
            ("spawn.empty", Unit::Ns, 1)
        );
        arm.begin_block(3).unwrap();
        assert_eq!((arm.sample(), arm.sample()), (Ok(1), Ok(2)));
        // An unread sample of the previous block is discarded.
        arm.begin_block(2).unwrap();
        assert_eq!((arm.sample(), arm.sample()), (Ok(4), Ok(5)));
        assert!(arm.sample().is_err(), "no sample beyond the block");
        arm.set_batch(7);
        assert_eq!(arm.batch(), 1);
        drop(arm);
        assert_eq!(lens, [3, 2]);
        let mut short = BulkFn::new("x", |_| Ok(vec![1]));
        assert!(short.begin_block(2).is_err());
        let mut failing = BulkFn::new("x", |_| Err(ArmError("hyperfine missing".into())));
        assert_eq!(
            failing.begin_block(1),
            Err(ArmError("hyperfine missing".into()))
        );
    }

    #[test]
    fn hyperfine_exports() {
        let export = br#"{"results":[{"command":"empty","mean":0.0123,"times":[0.0121,0.012345678912,0.5e-3]}]}"#;
        assert_eq!(
            hyperfine_times(export),
            Ok(vec![12_100_000, 12_345_679, 500_000])
        );
        for bad in [
            &b"not json"[..],
            br#"{"results":[]}"#,
            br#"{"results":[{"times":[0.1,"x"]}]}"#,
            br#"{"results":[{"times":[-0.1]}]}"#,
            br#"{"results":[{"times":[1e300]}]}"#,
        ] {
            assert!(
                hyperfine_times(bad).is_err(),
                "{}",
                String::from_utf8_lossy(bad)
            );
        }
    }

    #[test]
    fn value_arm_ignores_batches() {
        let mut n = 0;
        let mut arm = ValueFn::new("flushes", Unit::Count, move || {
            n += 1;
            Ok(n)
        });
        arm.set_batch(9);
        assert_eq!((arm.batch(), arm.sample(), arm.sample()), (1, Ok(1), Ok(2)));
        assert_eq!(arm.unit(), Unit::Count);
    }

    #[test]
    fn names() {
        for ok in ["op", "floor.data-only-flush", "a", "9x_y", &"a".repeat(64)] {
            assert!(valid_name(ok), "{ok}");
        }
        for bad in ["", "Op", ".x", "-x", "a b", "a/b", &"a".repeat(65), "é"] {
            assert!(!valid_name(bad), "{bad}");
        }
    }
}
