//! The replay verdict ([MP §9.5]): whether the replay reproduced the profile over a run's window, read from the
//! replay log. It is the loaded run's replay verdict of [MP §2.3] — (a) the generator settled before the run began and
//! kept replaying until after it ended, and (b) its validation against the profile passed over the run's window — and
//! `moirai-probes-bin loadgen`'s own verdict over its whole settled replay.
//!
//! **Clocks.** A run's window is wall-clock time, the only clock a driver and the generator share. The log orders,
//! windows and gap-checks its samples on the generator's monotonic clock (`mono`), and uses each sample's wall-clock
//! time (`t`) only to place the run's window on it: in a stretch of the replay without a wall-clock step, wall-clock
//! time is monotonic time plus a fixed offset. A step of the wall clock (a time synchronisation) inside the stretch a
//! run's window needs is a reason of that run's verdict, never a read error, so the runs before and after it keep
//! their verdicts ([MP §9.5]).

use super::fixture::Source;
use super::replay::LOG_SCHEMA;
use crate::condition::{Condition, LOADED_HIGH, LOADED_LOW};
use serde_json::{Value, json};
use std::io::{BufRead, Read, Seek, SeekFrom};
use std::ops::Range;
use std::path::Path;
use std::time::{Duration, Instant};

/// The schema name of a verdict ([MP §9.5]).
pub const VERDICT_SCHEMA: &str = "moirai-probes/replay-verdict/1";
/// Samples per validation block.
pub const BLOCK: usize = 10;
/// The fewest samples a last, partial block counts with.
pub const MIN_BLOCK: usize = 5;
/// The fewest samples a window needs.
pub const MIN_SAMPLES: usize = 30;
/// A run shorter than this is validated over the replay before its end, back to this length (never before settling).
pub const MIN_WINDOW_MS: u64 = 60_000;
/// The relative tolerance of a block mean.
pub const REL: f64 = 0.15;
/// The absolute tolerance of a processor-time block mean, in percentage points.
pub const ABS_CPU: f64 = 5.0;
/// The absolute tolerance of a disk block mean, in bytes per second.
pub const ABS_DISK: f64 = 1_000_000.0;
/// The share of blocks, in percent, that must be within tolerance.
pub const BLOCKS_PCT: usize = 90;
/// The share of samples, in percent, whose available physical memory must lie in the loaded band.
pub const MEMORY_PCT: usize = 95;
/// A gap between samples longer than this many sampling intervals (the log header's `sample_ms`) is a pause of the
/// replay.
pub const GAP_INTERVALS: u64 = 3;
/// A wall-clock step: two consecutive samples whose wall-clock and monotonic differences disagree by more than this
/// (half the sampling interval, so wall-clock time still increases within a stretch without a step).
pub const STEP_MS: u64 = 500;
/// How long a driver waits, at most, for the log to cover its run's end ([`verdict_when_covered`], [MP §9.7]): the
/// next sample is due within a second, and one more than [`GAP_INTERVALS`] intervals late fails rule (a) anyway.
pub const COVER_WAIT: Duration = Duration::from_secs(10);
/// How often [`verdict_when_covered`] looks at the log.
const COVER_POLL: Duration = Duration::from_millis(100);
/// The bytes of the log's end [`verdict_when_covered`] reads to find its last line (a line is a few hundred bytes).
const TAIL: u64 = 4_096;

/// The replay log's header ([MP §9.4]).
#[derive(Clone, Debug, PartialEq)]
pub struct LogHeader {
    /// The profile's source.
    pub source: Source,
    /// The profile's sample interval.
    pub interval_ms: u32,
    /// The generator's sampling interval (typeperf's `-si 1`), which the coverage rule's gaps are counted in.
    pub sample_ms: u64,
    /// The wall-clock time of the first sample, in milliseconds since 1970.
    pub started: u64,
}

impl LogHeader {
    /// The condition a run under this replay is made under ([MP §2.3]): loaded under the fixture, synthetic under a
    /// synthetic profile.
    pub fn condition(&self) -> Condition {
        match &self.source {
            Source::Fixture { id } => Condition::Loaded {
                fixture: id.clone(),
            },
            Source::Synthetic { spec } => Condition::Synthetic {
                description: format!("moirai-probes-bin loadgen, synthetic profile {spec}"),
            },
        }
    }
}

/// One sample line of the log, as the validation needs it.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct LogSample {
    /// Wall-clock time, milliseconds since 1970.
    pub t: u64,
    /// The generator's monotonic clock, milliseconds since its sampler started.
    pub mono: u64,
    /// Processor percent, read and write bytes per second asked by the profile for the interval.
    pub target: [f64; 3],
    /// The same, observed, and available physical bytes; `None` where the sampler had no value.
    pub observed: [Option<f64>; 4],
}

impl LogSample {
    /// Wall-clock minus monotonic time.
    fn offset(&self) -> i128 {
        i128::from(self.t) - i128::from(self.mono)
    }
}

/// A replay log as read back.
#[derive(Clone, Debug, PartialEq)]
pub struct Log {
    /// The header.
    pub header: LogHeader,
    /// The sample lines, in order (their monotonic times never decrease).
    pub samples: Vec<LogSample>,
    /// When the replay settled (wall clock), if it did.
    pub settled: Option<u64>,
    /// The index in [`Log::samples`] of the sample the replay settled at.
    pub settled_index: Option<usize>,
    /// The end line's time, if the replay has ended.
    pub end: Option<u64>,
}

fn u64_at(v: &Value, k: &str) -> Result<u64, String> {
    v.get(k)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("'{k}' is not an unsigned integer"))
}

fn numbers<const N: usize>(v: &Value, k: &str) -> Result<[Option<f64>; N], String> {
    let a = v
        .get(k)
        .and_then(Value::as_array)
        .filter(|a| a.len() == N)
        .ok_or_else(|| format!("'{k}' is not an array of {N}"))?;
    let mut out = [None; N];
    for (o, x) in out.iter_mut().zip(a) {
        *o = match x {
            Value::Null => None,
            x => Some(
                x.as_f64()
                    .ok_or_else(|| format!("'{k}' holds a non-number"))?,
            ),
        };
    }
    Ok(out)
}

/// Reads a replay log ([MP §9.4]). A last line without its line feed is ignored when it does not parse: the generator
/// may be writing it. Samples are ordered by their monotonic time; their wall-clock times may step either way.
pub fn read_log(r: impl BufRead) -> Result<Log, String> {
    let mut lines = r.split(b'\n').peekable();
    let mut log: Option<Log> = None;
    let mut n = 0;
    while let Some(raw) = lines.next() {
        n += 1;
        let raw = raw.map_err(|e| format!("the replay log: {e}"))?;
        let last = lines.peek().is_none();
        let v: Value = match serde_json::from_slice(&raw) {
            Ok(v) => v,
            Err(_) if last => break,
            Err(e) => return Err(format!("replay log line {n}: {e}")),
        };
        let bad = |e: String| format!("replay log line {n}: {e}");
        match log.as_mut() {
            None => {
                if v.get("schema").and_then(Value::as_str) != Some(LOG_SCHEMA) {
                    return Err(format!("not a {LOG_SCHEMA} log"));
                }
                let src = v.get("source").ok_or_else(|| bad("no 'source'".into()))?;
                let text = |k: &str| {
                    src.get(k)
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .ok_or_else(|| bad(format!("'source.{k}' is not a string")))
                };
                let source = match src.get("kind").and_then(Value::as_str) {
                    Some("fixture") => Source::Fixture { id: text("id")? },
                    Some("synthetic") => Source::Synthetic {
                        spec: text("spec")?,
                    },
                    _ => return Err(bad("unknown 'source.kind'".into())),
                };
                log = Some(Log {
                    header: LogHeader {
                        source,
                        interval_ms: u32::try_from(u64_at(&v, "interval_ms").map_err(bad)?)
                            .ok()
                            .filter(|&i| i > 0)
                            .ok_or_else(|| bad("'interval_ms' is out of range".into()))?,
                        sample_ms: Some(u64_at(&v, "sample_ms").map_err(bad)?)
                            .filter(|i| (1..=60_000).contains(i))
                            .ok_or_else(|| bad("'sample_ms' is out of range".into()))?,
                        started: u64_at(&v, "started").map_err(bad)?,
                    },
                    samples: Vec::new(),
                    settled: None,
                    settled_index: None,
                    end: None,
                });
            }
            Some(log) if v.get("end").is_some() => {
                log.end = Some(u64_at(&v, "end").map_err(bad)?);
            }
            Some(_) if v.get("schema").is_some() => {
                // The generator's verdict line after the end line.
                if v.get("schema").and_then(Value::as_str) != Some(VERDICT_SCHEMA) {
                    return Err(bad("an unknown line".into()));
                }
            }
            Some(log) => {
                let t = u64_at(&v, "t").map_err(bad)?;
                let mono = u64_at(&v, "mono").map_err(bad)?;
                if log.samples.last().is_some_and(|s| mono < s.mono) || log.end.is_some() {
                    return Err(bad("a sample out of order".into()));
                }
                let target = numbers::<3>(&v, "target").map_err(bad)?;
                let target = target.map(|x| x.unwrap_or(0.0));
                let observed = numbers::<4>(&v, "observed").map_err(bad)?;
                if v.get("settled").and_then(Value::as_bool) == Some(true) && log.settled.is_none()
                {
                    log.settled = Some(t);
                    log.settled_index = Some(log.samples.len());
                }
                log.samples.push(LogSample {
                    t,
                    mono,
                    target,
                    observed,
                });
            }
        }
    }
    log.ok_or_else(|| "the replay log is empty".into())
}

/// One validated quantity's result ([MP §9.5]).
#[derive(Clone, Debug, PartialEq)]
pub struct Quantity {
    /// `cpu`, `read` or `write`.
    pub name: &'static str,
    /// Blocks in the window.
    pub blocks: usize,
    /// Blocks within tolerance.
    pub passed: usize,
    /// The window's mean target.
    pub mean_target: f64,
    /// The window's mean observed value; `None` without an observation.
    pub mean_observed: Option<f64>,
}

impl Quantity {
    /// Whether at least [`BLOCKS_PCT`] of the blocks are within tolerance.
    pub fn pass(&self) -> bool {
        self.blocks > 0 && self.passed * 100 >= self.blocks * BLOCKS_PCT
    }
}

/// The verdict over one window ([MP §9.5]).
#[derive(Clone, Debug, PartialEq)]
pub struct Verdict {
    /// The run's window, wall-clock milliseconds.
    pub from: u64,
    /// Its end.
    pub to: u64,
    /// The start of the validated window (widened back to [`MIN_WINDOW_MS`] for a short run), wall clock.
    pub window_from: u64,
    /// When the replay settled.
    pub settled: Option<u64>,
    /// The samples in the validated window.
    pub samples: usize,
    /// The validated quantities.
    pub quantities: Vec<Quantity>,
    /// Window samples whose available memory lay in the loaded band.
    pub memory_in_band: usize,
    /// Why the verdict fails; empty when it passes.
    pub reasons: Vec<String>,
}

impl Verdict {
    /// Whether the replay reproduced the profile over the window.
    pub fn pass(&self) -> bool {
        self.reasons.is_empty()
    }

    /// The JSON form ([MP §9.5], schema [`VERDICT_SCHEMA`]).
    pub fn to_json(&self) -> Value {
        json!({
            "schema": VERDICT_SCHEMA,
            "from": self.from,
            "to": self.to,
            "window_from": self.window_from,
            "settled": self.settled,
            "samples": self.samples,
            "quantities": self.quantities.iter().map(|q| json!({
                "name": q.name,
                "blocks": q.blocks,
                "passed": q.passed,
                "mean_target": q.mean_target,
                "mean_observed": q.mean_observed,
                "pass": q.pass(),
            })).collect::<Vec<_>>(),
            "memory_in_band": self.memory_in_band,
            "pass": self.pass(),
            "reasons": self.reasons,
        })
    }
}

/// The mean of the present values; `None` when there is none.
fn mean(values: impl Iterator<Item = Option<f64>>) -> (Option<f64>, usize) {
    let (sum, n) = values
        .flatten()
        .fold((0.0, 0usize), |(s, n), v| (s + v, n + 1));
    ((n > 0).then(|| sum / n as f64), n)
}

/// The wall-clock step between two consecutive samples (positive forward), when it exceeds [`STEP_MS`].
fn step(a: &LogSample, b: &LogSample) -> Option<i128> {
    let d = b.offset() - a.offset();
    (d.unsigned_abs() > u128::from(STEP_MS)).then_some(d)
}

/// The stretches of the replay without a wall-clock step, as index ranges of `samples`, in order.
fn stretches(samples: &[LogSample]) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut start = 0;
    for i in 1..samples.len() {
        if step(&samples[i - 1], &samples[i]).is_some() {
            out.push(start..i);
            start = i;
        }
    }
    if !samples.is_empty() {
        out.push(start..samples.len());
    }
    out
}

/// Places the wall-clock instant `to` on the replay's monotonic clock: the offset (wall minus monotonic) of the
/// stretch without a wall-clock step that holds it — from its first sample to [`GAP_INTERVALS`] sampling intervals
/// after its last — or, when none holds it, of the last stretch that begins at or before it (or the first stretch).
/// A reason comes back when two stretches hold it: the wall clock stepped back over the instant, so which one the
/// driver's clock meant is unknown.
fn place(log: &Log, to: u64) -> (i128, Option<String>) {
    let s = &log.samples;
    let reach = GAP_INTERVALS * log.header.sample_ms;
    let all = stretches(s);
    let holding: Vec<&Range<usize>> = all
        .iter()
        .filter(|r| s[r.start].t <= to && to <= s[r.end - 1].t.saturating_add(reach))
        .collect();
    let Some(r) = holding
        .last()
        .copied()
        .or_else(|| all.iter().rev().find(|r| s[r.start].t <= to))
        .or(all.first())
    else {
        return (0, None);
    };
    let k = r.clone().rev().find(|&i| s[i].t <= to).unwrap_or(r.start);
    let reason = (holding.len() > 1).then(|| {
        let b = holding[holding.len() - 2].end;
        format!(
            "the wall clock stepped by {} ms after {}: the run's end at {to} falls in two stretches of the replay",
            s[b].offset() - s[b - 1].offset(),
            s[b - 1].t
        )
    });
    (s[k].offset(), reason)
}

/// The rules of [MP §9.5] on the monotonic clock: the run window is wall-clock `[from, to]`, placed at monotonic
/// `[from_m, to_m]`; `offset` turns a monotonic time back into wall-clock time for the record. With `steps`, a
/// wall-clock step inside the span the coverage rule checks is a reason: the placement does not hold across it.
fn judge(
    log: &Log,
    (from, to): (u64, u64),
    (from_m, to_m): (u64, u64),
    offset: i128,
    steps: bool,
    mut reasons: Vec<String>,
) -> Verdict {
    let s = &log.samples;
    let settled_m = log.settled_index.map(|i| s[i].mono);
    match (settled_m, log.settled) {
        (Some(m), Some(w)) if m > from_m => {
            reasons.push(format!(
                "the replay settled at {w}, after the run began at {from}"
            ));
        }
        (Some(_), _) => {}
        (None, _) => reasons.push("the replay never settled".to_string()),
    }
    if !s.last().is_some_and(|x| x.mono >= to_m) {
        reasons.push(format!(
            "the replay has no sample at or after the run's end at {to}"
        ));
    }
    let window_from_m = settled_m
        .unwrap_or(from_m)
        .max(from_m.min(to_m.saturating_sub(MIN_WINDOW_MS)));
    let lo = s.partition_point(|x| x.mono <= window_from_m);
    let hi = s.partition_point(|x| x.mono <= to_m);
    let window = &s[lo..hi.max(lo)];
    if window.len() < MIN_SAMPLES {
        reasons.push(format!(
            "the window holds {} samples; at least {MIN_SAMPLES} are needed",
            window.len()
        ));
    }
    let gap = GAP_INTERVALS * log.header.sample_ms;
    let span = &s[lo.min(hi).saturating_sub(1)..(hi.max(lo) + 1).min(s.len())];
    if let Some(w) = span
        .windows(2)
        .find(|w| w[1].mono.saturating_sub(w[0].mono) > gap)
    {
        reasons.push(format!(
            "the replay paused for {} ms after {}",
            w[1].mono.saturating_sub(w[0].mono),
            w[0].t
        ));
    }
    if steps
        && let Some((d, w)) = span
            .windows(2)
            .find_map(|w| step(&w[0], &w[1]).map(|d| (d, w)))
    {
        reasons.push(format!(
            "the wall clock stepped by {d} ms after {}: the run's window cannot be placed on the replay",
            w[0].t
        ));
    }
    let mut quantities = Vec::new();
    for (k, name, abs) in [
        (0, "cpu", ABS_CPU),
        (1, "read", ABS_DISK),
        (2, "write", ABS_DISK),
    ] {
        let mut q = Quantity {
            name,
            blocks: 0,
            passed: 0,
            mean_target: mean(window.iter().map(|s| Some(s.target[k])))
                .0
                .unwrap_or(0.0),
            mean_observed: mean(window.iter().map(|s| s.observed[k])).0,
        };
        for block in window.chunks(BLOCK).filter(|b| b.len() >= MIN_BLOCK) {
            q.blocks += 1;
            let target = mean(block.iter().map(|s| Some(s.target[k])))
                .0
                .unwrap_or(0.0);
            let (observed, n) = mean(block.iter().map(|s| s.observed[k]));
            if n * 2 >= block.len()
                && observed.is_some_and(|o| (o - target).abs() <= (REL * target).max(abs))
            {
                q.passed += 1;
            }
        }
        if !window.is_empty() && !q.pass() {
            reasons.push(format!(
                "{name}: {} of {} blocks within tolerance ({BLOCKS_PCT} % needed)",
                q.passed, q.blocks
            ));
        }
        quantities.push(q);
    }
    let band = LOADED_LOW as f64..=LOADED_HIGH as f64;
    let memory_in_band = window
        .iter()
        .filter(|s| s.observed[3].is_some_and(|a| band.contains(&a)))
        .count();
    if !window.is_empty() && memory_in_band * 100 < window.len() * MEMORY_PCT {
        reasons.push(format!(
            "available memory in the loaded band in {memory_in_band} of {} samples ({MEMORY_PCT} % needed)",
            window.len()
        ));
    }
    Verdict {
        from,
        to,
        window_from: (i128::from(window_from_m) + offset).clamp(0, i128::from(u64::MAX)) as u64,
        settled: log.settled,
        samples: window.len(),
        quantities,
        memory_in_band,
        reasons,
    }
}

/// The verdict of the replay over the run window `[from, to]`, wall-clock milliseconds ([MP §9.5]). The window is
/// placed on the replay's monotonic clock by the wall-clock times of its stretch without a wall-clock step (the
/// module header), and then:
/// - the replay settled at or before `from`;
/// - it has a sample at or after `to`, and no pause longer than [`GAP_INTERVALS`] sampling intervals from the last
///   sample at or before the start of the validated window to the first sample after `to` (or the last sample);
/// - no wall-clock step lies in that span, and `to` falls in one stretch only;
/// - the validated window — `(max(settled, min(from, to − 60 s)), to]` — holds at least [`MIN_SAMPLES`] samples;
/// - for processor time, read bytes and write bytes, at least [`BLOCKS_PCT`] % of the window's blocks of [`BLOCK`]
///   samples (a last block counts with [`MIN_BLOCK`] or more) have an observed mean within the larger of [`REL`] of the
///   target mean and the absolute tolerance ([`ABS_CPU`], [`ABS_DISK`]), a block counting as missed when fewer than
///   half its samples carry an observation;
/// - at least [`MEMORY_PCT`] % of the window's samples read available physical memory in the loaded band.
pub fn verdict(log: &Log, from: u64, to: u64) -> Verdict {
    let (offset, ambiguous) = place(log, to);
    let mono = |w: u64| (i128::from(w) - offset).clamp(0, i128::from(u64::MAX)) as u64;
    judge(
        log,
        (from, to),
        (mono(from), mono(to)),
        offset,
        true,
        ambiguous.into_iter().collect(),
    )
}

/// The verdict over the whole settled replay, from the sample it settled at (or its first sample, if it never did) to
/// its last sample: `moirai-probes-bin loadgen`'s own verdict ([MP §9.4]). It is judged on the monotonic clock alone,
/// so a wall-clock step during the replay does not touch it.
pub fn replay_verdict(log: &Log) -> Verdict {
    let s = &log.samples;
    let (Some(first), Some(last)) = (s.first(), s.last()) else {
        let t = log.header.started;
        return judge(log, (t, t), (0, 0), i128::from(t), false, Vec::new());
    };
    let start = log.settled_index.map_or(first, |i| &s[i]);
    judge(
        log,
        (start.t, last.t),
        (start.mono, last.mono),
        start.offset(),
        false,
        Vec::new(),
    )
}

/// Reads the replay log at `path`.
pub fn read_log_file(path: &Path) -> Result<Log, String> {
    let f = std::fs::File::open(path).map_err(|e| format!("the replay log: {e}"))?;
    read_log(std::io::BufReader::new(f))
}

/// Reads the replay log at `path` and returns its header and the verdict over `[from, to]`: what a measurement driver
/// records as a loaded run's replay verdict ([MP §2.3], `RunRecord::set_load_replay`). A driver that reads the log as
/// soon as its run has ended uses [`verdict_when_covered`] instead.
pub fn verdict_of_file(path: &Path, from: u64, to: u64) -> Result<(LogHeader, Verdict), String> {
    let log = read_log_file(path)?;
    let v = verdict(&log, from, to);
    Ok((log.header, v))
}

/// Whether the log's last whole line shows that the replay has gone past `to`: a sample with a wall-clock time after
/// it, the end line, or the generator's verdict. Reads the last [`TAIL`] bytes only.
fn covers(path: &Path, to: u64) -> Result<bool, String> {
    let io = |e: std::io::Error| format!("the replay log: {e}");
    let mut f = std::fs::File::open(path).map_err(io)?;
    let len = f.metadata().map_err(io)?.len();
    f.seek(SeekFrom::Start(len.saturating_sub(TAIL)))
        .map_err(io)?;
    let mut buf = Vec::with_capacity(TAIL as usize);
    f.take(TAIL).read_to_end(&mut buf).map_err(io)?;
    let Some(end) = buf.iter().rposition(|&b| b == b'\n') else {
        return Ok(false);
    };
    let line = buf[..end].rsplit(|&b| b == b'\n').next().unwrap_or(&[]);
    Ok(match serde_json::from_slice::<Value>(line) {
        Ok(v) => {
            v.get("end").is_some()
                || v.get("schema").and_then(Value::as_str) == Some(VERDICT_SCHEMA)
                || v.get("t").and_then(Value::as_u64).is_some_and(|t| t > to)
        }
        Err(_) => false,
    })
}

/// The verdict over `[from, to]` once the log covers the run's end ([MP §9.7]): it waits until the log's last line is
/// a sample after `to`, the end line or the verdict, or until `timeout` ([`COVER_WAIT`]) has passed, and then reads
/// the log. The sample after a run's end arrives up to a sampling interval after it, so a driver that read the log as
/// soon as its run ended would usually find none yet and fail rule (a) for a good replay. On a timeout the verdict
/// carries the coverage reason.
pub fn verdict_when_covered(
    path: &Path,
    from: u64,
    to: u64,
    timeout: Duration,
) -> Result<(LogHeader, Verdict), String> {
    let t0 = Instant::now();
    while !covers(path, to)? && t0.elapsed() < timeout {
        std::thread::sleep(COVER_POLL.min(timeout.saturating_sub(t0.elapsed())));
    }
    verdict_of_file(path, from, to)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::load::replay::tests::{Plant, simulate};
    use crate::load::synthetic::Spec;
    use crate::testkit::scratch_dir;
    use proptest::prelude::*;
    use std::io::Write;

    fn spec() -> crate::load::fixture::Profile {
        Spec::parse("seconds=300,cpu=50,read=10MB,write=10MB,step=20,seed=9")
            .unwrap()
            .profile()
            .unwrap()
    }

    #[test]
    fn a_faithful_replay_passes() {
        let plant = Plant::new([8.0, 2e6, 3e6], 10_000_000_000, 0.85);
        let (text, out) = simulate(&plant, &spec(), 400);
        let log = read_log(text.as_bytes()).unwrap();
        assert_eq!(log.samples.len(), 400);
        assert_eq!(log.settled, out.settled);
        assert_eq!(log.end, Some(out.ended));
        assert_eq!(log.header.started, out.started);
        let settled = out.settled.unwrap();
        let v = verdict(&log, settled, out.ended);
        assert!(v.pass(), "{}", v.to_json());
        assert_eq!(v.samples, 400 - 60);
        assert!(v.quantities.iter().all(|q| q.blocks == 34 && q.pass()));
        assert_eq!(v.memory_in_band, v.samples);
        // A run inside the replay: its own window.
        let v = verdict(&log, settled + 100_000, settled + 200_000);
        assert!(v.pass(), "{}", v.to_json());
        assert_eq!(v.samples, 100);
        // A short run is validated over the minute before its end.
        let v = verdict(&log, settled + 150_000, settled + 160_000);
        assert!(v.pass(), "{}", v.to_json());
        assert_eq!((v.window_from, v.samples), (settled + 100_000, 60));
        assert_eq!(v.to_json()["schema"], VERDICT_SCHEMA);
    }

    #[test]
    fn an_unreachable_profile_fails() {
        // The rest of the machine already does 70 % processor time; the profile asks 25 % to 75 %.
        let plant = Plant::new([70.0, 0.0, 0.0], 10_000_000_000, 1.0);
        let (text, out) = simulate(&plant, &spec(), 300);
        let log = read_log(text.as_bytes()).unwrap();
        let v = verdict(&log, out.settled.unwrap(), out.ended);
        assert!(!v.pass());
        assert!(
            v.reasons.iter().any(|r| r.starts_with("cpu:")),
            "{:?}",
            v.reasons
        );
        assert!(v.quantities[1].pass() && v.quantities[2].pass());
    }

    #[test]
    fn memory_out_of_band_fails() {
        // Available memory below the band before the hold holds anything: the hold cannot help.
        let plant = Plant::new([0.0; 3], 1_200_000_000, 1.0);
        let (text, out) = simulate(&plant, &spec(), 200);
        assert_eq!(out.settled, None);
        let log = read_log(text.as_bytes()).unwrap();
        let v = verdict(&log, out.started + 70_000, out.ended);
        assert!(v.reasons.iter().any(|r| r.contains("never settled")));
        assert!(v.reasons.iter().any(|r| r.contains("loaded band")));
        assert_eq!(v.memory_in_band, 0);
    }

    #[test]
    fn timing_rules() {
        let plant = Plant::new([5.0, 0.0, 0.0], 10_000_000_000, 1.0);
        let (text, out) = simulate(&plant, &spec(), 200);
        let mut log = read_log(text.as_bytes()).unwrap();
        let settled = out.settled.unwrap();
        // Settled after the run began.
        let v = verdict(&log, settled - 1, settled + 100_000);
        assert!(v.reasons.iter().any(|r| r.contains("after the run began")));
        // The replay ended before the run did.
        let v = verdict(&log, settled, out.ended + 1);
        assert!(
            v.reasons
                .iter()
                .any(|r| r.contains("no sample at or after"))
        );
        // Too few samples: the run starts at settling and lasts ten seconds.
        let v = verdict(&log, settled, settled + 10_000);
        assert!(
            v.reasons.iter().any(|r| r.contains("holds 10 samples")),
            "{:?}",
            v.reasons
        );
        // A pause inside the window.
        log.samples.remove(120);
        log.samples.remove(120);
        log.samples.remove(120);
        let v = verdict(&log, settled, out.ended);
        assert!(
            v.reasons.iter().any(|r| r.contains("paused for 4000 ms")),
            "{:?}",
            v.reasons
        );
    }

    #[test]
    fn missing_observations_count_against_their_block() {
        let plant = Plant::new([5.0, 0.0, 0.0], 10_000_000_000, 1.0);
        plant.lock().unwrap().missing_every = 2;
        let (text, out) = simulate(&plant, &spec(), 200);
        let log = read_log(text.as_bytes()).unwrap();
        let v = verdict(&log, out.settled.unwrap(), out.ended);
        // Half the read observations are missing: a block still counts at half.
        assert!(v.quantities[1].pass(), "{}", v.to_json());
        plant.lock().unwrap().missing_every = 1;
        let (text, out) = simulate(&plant, &spec(), 200);
        let log = read_log(text.as_bytes()).unwrap();
        let v = verdict(&log, out.settled.unwrap(), out.ended);
        assert_eq!(v.quantities[1].passed, 0);
        assert_eq!(v.quantities[1].mean_observed, None);
    }

    #[test]
    fn reading_logs() {
        let plant = Plant::new([5.0, 0.0, 0.0], 10_000_000_000, 1.0);
        let (text, _) = simulate(&plant, &spec(), 80);
        // A torn last line is ignored; a torn line elsewhere is an error.
        let torn = &text[..text.len() - 20];
        assert_eq!(read_log(torn.as_bytes()).unwrap().samples.len(), 80);
        let mut lines: Vec<&str> = text.lines().collect();
        lines[3] = "{\"t\":";
        assert!(
            read_log(lines.join("\n").as_bytes())
                .unwrap_err()
                .contains("line 4")
        );
        assert!(read_log("".as_bytes()).unwrap_err().contains("empty"));
        assert!(
            read_log("{\"schema\":\"x\"}\n".as_bytes())
                .unwrap_err()
                .contains("not a")
        );
        let with_verdict = format!(
            "{text}{}\n",
            json!({"schema": VERDICT_SCHEMA, "pass": true})
        );
        assert_eq!(read_log(with_verdict.as_bytes()).unwrap().samples.len(), 80);
        let after_end = format!("{text}{}\n", text.lines().nth(5).unwrap());
        assert!(
            read_log(after_end.as_bytes())
                .unwrap_err()
                .contains("out of order")
        );
        let header = read_log(text.as_bytes()).unwrap().header;
        assert!(
            matches!(header.condition(), Condition::Synthetic { description } if description.contains("seed=9"))
        );
        let fx = LogHeader {
            source: Source::Fixture {
                id: "ab".repeat(32),
            },
            interval_ms: 1_000,
            sample_ms: 1_000,
            started: 0,
        };
        assert_eq!(
            fx.condition(),
            Condition::Loaded {
                fixture: "ab".repeat(32)
            }
        );
    }

    /// `text` (a replay log) with the wall-clock time of every sample from sample `at` on, and of the end line, moved
    /// by `by` milliseconds: a step of the wall clock during the replay.
    fn step_text(text: &str, at: usize, by: i64) -> String {
        let mut k = 0;
        let mut out = String::new();
        for line in text.lines() {
            let mut v: Value = serde_json::from_str(line).unwrap();
            let key = if v.get("t").is_some() {
                k += 1;
                (k > at).then_some("t")
            } else if v.get("end").is_some() {
                Some("end")
            } else {
                None
            };
            if let Some(key) = key {
                v[key] = json!(v[key].as_u64().unwrap().checked_add_signed(by).unwrap());
            }
            out.push_str(&format!("{v}\n"));
        }
        out
    }

    #[test]
    fn wall_clock_steps_are_reasons_not_read_errors() {
        let plant = Plant::new([8.0, 2e6, 3e6], 10_000_000_000, 0.85);
        let (text, out) = simulate(&plant, &spec(), 400);
        let log = read_log(text.as_bytes()).unwrap();
        let si = log.settled_index.unwrap();
        assert!(si < 65, "settled at sample {si}");
        let t = |l: &Log, i: usize| l.samples[i].t;
        // An hour back at sample 100: the log reads, and runs before and after the step keep their verdicts.
        let back = read_log(step_text(&text, 100, -3_600_000).as_bytes()).unwrap();
        assert_eq!(back.samples.len(), 400);
        let v = verdict(&back, t(&back, 200), t(&back, 300));
        assert!(v.pass(), "{}", v.to_json());
        assert_eq!((v.samples, v.window_from), (100, t(&back, 200)));
        let v = verdict(&back, t(&back, si), t(&back, 95));
        assert!(v.pass(), "{}", v.to_json());
        // The whole replay's verdict does not look at the wall clock.
        let (whole, whole_back) = (replay_verdict(&log), replay_verdict(&back));
        assert!(
            whole.pass() && whole_back.pass(),
            "{}",
            whole_back.to_json()
        );
        assert_eq!(
            (whole.samples, whole.quantities.clone()),
            (whole_back.samples, whole_back.quantities.clone())
        );
        assert_eq!((whole.from, whole.to), (out.settled.unwrap(), out.ended));
        // Five seconds forward inside a run's window: the window cannot be placed.
        let fwd = read_log(step_text(&text, 250, 5_000).as_bytes()).unwrap();
        let v = verdict(&fwd, t(&fwd, 200), t(&fwd, 300));
        assert!(
            v.reasons
                .iter()
                .any(|r| r.contains("stepped by 5000 ms") && r.contains("cannot be placed")),
            "{:?}",
            v.reasons
        );
        // Thirty seconds back, and a run that ends where both stretches reach: which one is unknown.
        let amb = read_log(step_text(&text, 250, -30_000).as_bytes()).unwrap();
        let v = verdict(&amb, t(&amb, 200), t(&amb, 240));
        assert!(
            v.reasons
                .iter()
                .any(|r| r.contains("stepped by -30000 ms") && r.contains("two stretches")),
            "{:?}",
            v.reasons
        );
        // A monotonic time that goes back is a damaged log.
        let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
        lines.swap(10, 11);
        assert!(
            read_log(lines.join("\n").as_bytes())
                .unwrap_err()
                .contains("out of order")
        );
    }

    #[test]
    fn gaps_are_counted_in_sampling_intervals() {
        let plant = Plant::new([5.0, 0.0, 0.0], 10_000_000_000, 1.0);
        let (text, out) = simulate(&plant, &spec(), 200);
        let mut log = read_log(text.as_bytes()).unwrap();
        assert_eq!(log.header.sample_ms, 1_000);
        let settled = out.settled.unwrap();
        for _ in 0..3 {
            log.samples.remove(120);
        }
        // The profile's interval does not widen the rule: a 60 s profile interval still pauses after 3 s.
        log.header.interval_ms = 60_000;
        let v = verdict(&log, settled, out.ended);
        assert!(
            v.reasons.iter().any(|r| r.contains("paused for 4000 ms")),
            "{:?}",
            v.reasons
        );
        // The sampling interval does.
        log.header.sample_ms = 2_000;
        let v = verdict(&log, settled, out.ended);
        assert!(
            !v.reasons.iter().any(|r| r.contains("paused")),
            "{:?}",
            v.reasons
        );
        // A header without a sampling interval is refused.
        let no_si = text.replacen("\"sample_ms\":1000,", "", 1);
        assert!(
            read_log(no_si.as_bytes())
                .unwrap_err()
                .contains("sample_ms")
        );
    }

    #[test]
    fn a_driver_waits_for_the_sample_after_its_run() {
        let plant = Plant::new([8.0, 2e6, 3e6], 10_000_000_000, 0.85);
        let (text, out) = simulate(&plant, &spec(), 200);
        let lines: Vec<String> = text.lines().map(|l| format!("{l}\n")).collect();
        let log = read_log(text.as_bytes()).unwrap();
        let settled = out.settled.unwrap();
        // The run ends 400 ms after sample 150; the log holds the header and the samples up to 150.
        let k = 150;
        let to = log.samples[k].t + 400;
        let d = scratch_dir("validate-wait");
        let path = d.join("replay.jsonl");
        std::fs::write(&path, lines[..k + 2].concat()).unwrap();
        // Read at once, the replay has no sample after the run's end yet; waiting in vain gives the same verdict.
        let (_, early) = verdict_of_file(&path, settled, to).unwrap();
        assert!(
            early
                .reasons
                .iter()
                .any(|r| r.contains("no sample at or after")),
            "{:?}",
            early.reasons
        );
        let t0 = Instant::now();
        let (_, timed_out) =
            verdict_when_covered(&path, settled, to, Duration::from_millis(300)).unwrap();
        assert!(t0.elapsed() >= Duration::from_millis(300));
        assert_eq!(timed_out, early);
        // The generator appends the next sample a moment later, torn in two writes; the wait sees it whole.
        let next = lines[k + 2].clone();
        let appender = {
            let path = path.clone();
            std::thread::spawn(move || {
                let mut f = std::fs::OpenOptions::new()
                    .append(true)
                    .open(&path)
                    .unwrap();
                std::thread::sleep(Duration::from_millis(150));
                f.write_all(&next.as_bytes()[..10]).unwrap();
                f.flush().unwrap();
                std::thread::sleep(Duration::from_millis(250));
                f.write_all(&next.as_bytes()[10..]).unwrap();
            })
        };
        let (header, v) = verdict_when_covered(&path, settled, to, COVER_WAIT).unwrap();
        appender.join().unwrap();
        assert!(v.pass(), "{}", v.to_json());
        assert_eq!(header.sample_ms, 1_000);
        // The end line covers any later end at once.
        std::fs::write(&path, &text).unwrap();
        let t0 = Instant::now();
        let (_, v) = verdict_when_covered(&path, settled, out.ended + 5_000, COVER_WAIT).unwrap();
        assert!(t0.elapsed() < Duration::from_secs(2));
        assert!(!v.pass());
        let _ = std::fs::remove_dir_all(&d);
    }

    proptest! {
        /// Observations within the tolerance of their targets pass; observations off by more than it in every block
        /// fail.
        #[test]
        fn tolerance(targets in proptest::collection::vec(0.0f64..90.0, 40..120), off in -0.9f64..0.9,
                     factor in 1.2f64..3.0) {
            let header = LogHeader { source: Source::Synthetic { spec: "s".into() }, interval_ms: 1_000,
                                     sample_ms: 1_000, started: 0 };
            // The replay settles at its first sample (t = 0); the run's samples follow, one a second.
            let settle = LogSample { t: 0, mono: 0, target: [0.0; 3], observed: [None; 4] };
            let mk = |cpu_shift: f64, disk_shift: f64| Log {
                header: header.clone(),
                samples: std::iter::once(settle).chain(targets.iter().enumerate().map(|(i, &t)| LogSample {
                    t: 1_000 * (i as u64 + 1),
                    mono: 1_000 * (i as u64 + 1),
                    target: [t, t * 1e6, t * 1e6],
                    observed: [Some(t + cpu_shift), Some(t * 1e6 + disk_shift), Some(t * 1e6 + disk_shift),
                               Some(1.8e9)],
                })).collect(),
                settled: Some(0),
                settled_index: Some(0),
                end: None,
            };
            let end = 1_000 * targets.len() as u64;
            // Off by less than the absolute tolerance everywhere: every block passes.
            let v = verdict(&mk(off * ABS_CPU, off * ABS_DISK), 0, end);
            prop_assert!(v.pass(), "{}", v.to_json());
            // Off by `factor` times the largest tolerance in every sample: every block misses.
            let v = verdict(&mk(factor * ABS_CPU.max(REL * 90.0) + 1.0,
                                factor * ABS_DISK.max(REL * 90e6) + 1.0), 0, end);
            prop_assert!(v.quantities.iter().all(|q| q.passed == 0), "{}", v.to_json());
        }
    }
}
