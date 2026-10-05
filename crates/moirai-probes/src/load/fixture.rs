//! The load fixture's format ([MP §9.1], [MP §9.2]) as the generator reads it: the counter set, the decoder with
//! every rule of the format, and the profile the generator replays. `xtask loadrec` (WP-51a) writes the format; this
//! is an independent reader written from the same section.

use crate::units::{KIB, MIB};
use xxhash_rust::xxh3::xxh3_64;

/// The fixture's counters in column order ([MP §9.1]): system-wide only.
pub const COUNTERS: [&str; 18] = [
    r"\Processor(_Total)\% Processor Time",
    r"\Processor(_Total)\% Privileged Time",
    r"\Processor(_Total)\% User Time",
    r"\PhysicalDisk(_Total)\Disk Read Bytes/sec",
    r"\PhysicalDisk(_Total)\Disk Write Bytes/sec",
    r"\PhysicalDisk(_Total)\Disk Reads/sec",
    r"\PhysicalDisk(_Total)\Disk Writes/sec",
    r"\PhysicalDisk(_Total)\Avg. Disk Queue Length",
    r"\Memory\Available Bytes",
    r"\Memory\Committed Bytes",
    r"\Memory\Pages/sec",
    r"\Paging File(_Total)\% Usage",
    r"\Process(_Total)\Private Bytes",
    r"\Process(_Total)\Working Set",
    r"\Process(_Total)\IO Read Bytes/sec",
    r"\Process(_Total)\IO Write Bytes/sec",
    r"\Process(_Total)\Thread Count",
    r"\Process(_Total)\Handle Count",
];

/// The number of counters.
pub const C: usize = COUNTERS.len();
/// Column of `\Processor(_Total)\% Processor Time`: replayed and validated.
pub const CPU: usize = 0;
/// Column of `\PhysicalDisk(_Total)\Disk Read Bytes/sec`: replayed and validated.
pub const READ_BYTES: usize = 3;
/// Column of `\PhysicalDisk(_Total)\Disk Write Bytes/sec`: replayed and validated.
pub const WRITE_BYTES: usize = 4;
/// Column of `\PhysicalDisk(_Total)\Disk Reads/sec`: sets the size of the replayed reads.
pub const READS: usize = 5;
/// Column of `\PhysicalDisk(_Total)\Disk Writes/sec`: sets the size of the replayed writes.
pub const WRITES: usize = 6;
/// Column of `\Memory\Available Bytes`: watched while replaying (the replay holds 1.8 GB instead of this series).
pub const AVAILABLE: usize = 8;

/// The fixture's first 8 bytes ([MP §9.2]).
pub const MAGIC: [u8; 8] = [0x00, 0x8C, 0x9A, 0x0D, 0x0A, 0x1A, 0x0A, 0x00];
/// The format version.
pub const VERSION: u16 = 1;
/// The bytes of one sample: its time and one binary64 value per counter.
pub const ROW: usize = 8 + 8 * C;

/// The size of a replayed I/O when the profile gives no operation count ([MP §9.4]).
pub const DEFAULT_IO: u32 = 64 * KIB as u32;
/// The smallest replayed I/O.
pub const MIN_IO: u32 = 4 * KIB as u32;
/// The largest replayed I/O.
pub const MAX_IO: u32 = MIB as u32;

/// Where a profile came from: the recorded fixture or a synthetic profile ([MP §9.6]).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Source {
    /// A fixture file, named by the BLAKE3-256 of its bytes in lower-case hex ([MP §2.3]).
    Fixture {
        /// The fixture id.
        id: String,
    },
    /// A synthetic profile and its canonical spec.
    Synthetic {
        /// The canonical spec ([MP §9.6]).
        spec: String,
    },
}

/// The replay targets at one instant ([MP §9.4]).
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Targets {
    /// System-wide processor time, percent of all logical processors.
    pub cpu: f64,
    /// System-wide disk read bytes per second.
    pub read: f64,
    /// System-wide disk write bytes per second.
    pub write: f64,
    /// The size of one replayed read.
    pub read_size: u32,
    /// The size of one replayed write.
    pub write_size: u32,
}

/// The replayed columns of a profile's samples, NaN where a value is missing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Series {
    /// Processor time, percent.
    pub cpu: Vec<f64>,
    /// Disk read bytes per second.
    pub read: Vec<f64>,
    /// Disk write bytes per second.
    pub write: Vec<f64>,
    /// Disk reads per second.
    pub reads: Vec<f64>,
    /// Disk writes per second.
    pub writes: Vec<f64>,
}

/// What the generator replays: per sample its time and the replayed series, with missing values carried forward
/// (leading ones take the first value), so every lookup is a number ([MP §9.4]).
#[derive(Clone, Debug, PartialEq)]
pub struct Profile {
    /// Where the profile came from.
    pub source: Source,
    /// The sample interval in milliseconds.
    pub interval_ms: u32,
    /// Each sample's time in milliseconds since the first (0, strictly increasing).
    pub times: Vec<u64>,
    /// Processor time, percent.
    pub cpu: Vec<f64>,
    /// Disk read bytes per second.
    pub read: Vec<f64>,
    /// Disk write bytes per second.
    pub write: Vec<f64>,
    /// The size of one read, per sample.
    pub read_size: Vec<u32>,
    /// The size of one write, per sample.
    pub write_size: Vec<u32>,
}

/// The size of one replayed I/O from a byte rate and an operation rate: their quotient, rounded down to 4 KiB and
/// held to [4 KiB, 1 MiB]; [`DEFAULT_IO`] when either is missing or the operation rate is below one per second.
pub fn io_size(bytes: f64, ops: f64) -> u32 {
    if !(bytes > 0.0 && ops >= 1.0) {
        return DEFAULT_IO;
    }
    let size = (bytes / ops) as u64 / u64::from(MIN_IO) * u64::from(MIN_IO);
    size.clamp(u64::from(MIN_IO), u64::from(MAX_IO)) as u32
}

/// Replaces NaN by the previous value, and leading NaNs by the first value; `None` when every value is NaN.
fn carry(column: &mut [f64]) -> Option<()> {
    let first = column.iter().copied().find(|v| !v.is_nan())?;
    let mut last = first;
    for v in column.iter_mut() {
        if v.is_nan() {
            *v = last;
        } else {
            last = *v;
        }
    }
    Some(())
}

impl Profile {
    /// Builds a profile from samples: their times and their values of the replayed columns. Refused when there is no
    /// sample, a series is not at full length, the times are not 0 and strictly increasing, or one of processor time,
    /// read bytes and write bytes has no value at all.
    pub fn new(
        source: Source,
        interval_ms: u32,
        times: Vec<u64>,
        series: Series,
    ) -> Result<Profile, String> {
        let Series {
            mut cpu,
            mut read,
            mut write,
            reads,
            writes,
        } = series;
        let n = times.len();
        if n == 0
            || [
                cpu.len(),
                read.len(),
                write.len(),
                reads.len(),
                writes.len(),
            ] != [n; 5]
        {
            return Err(
                "a profile needs at least one sample and every series at full length".into(),
            );
        }
        if interval_ms == 0 {
            return Err("a profile's interval must be positive".into());
        }
        if times[0] != 0 || times.windows(2).any(|w| w[1] <= w[0]) {
            return Err("a profile's times must start at 0 and increase".into());
        }
        for (col, name) in [
            (&mut cpu, COUNTERS[CPU]),
            (&mut read, COUNTERS[READ_BYTES]),
            (&mut write, COUNTERS[WRITE_BYTES]),
        ] {
            carry(col).ok_or_else(|| format!("the profile has no value of {name}"))?;
        }
        let read_size = read
            .iter()
            .zip(&reads)
            .map(|(&b, &o)| io_size(b, o))
            .collect();
        let write_size = write
            .iter()
            .zip(&writes)
            .map(|(&b, &o)| io_size(b, o))
            .collect();
        Ok(Profile {
            source,
            interval_ms,
            times,
            cpu,
            read,
            write,
            read_size,
            write_size,
        })
    }

    /// The time the profile covers: the last sample's time plus one interval.
    pub fn duration_ms(&self) -> u64 {
        self.times
            .last()
            .map_or(0, |t| t + u64::from(self.interval_ms))
    }

    /// The targets at replay time `tau_ms`, the profile repeating end to end: the values of the last sample at or
    /// before `tau_ms` modulo the duration ([MP §9.4]).
    pub fn at(&self, tau_ms: u64) -> Targets {
        let tau = tau_ms % self.duration_ms().max(1);
        let i = self.times.partition_point(|&t| t <= tau).max(1) - 1;
        Targets {
            cpu: self.cpu[i].min(100.0),
            read: self.read[i],
            write: self.write[i],
            read_size: self.read_size[i],
            write_size: self.write_size[i],
        }
    }

    /// Whether any sample asks for disk reads, so the generator needs its read pool.
    pub fn reads_disk(&self) -> bool {
        self.read.iter().any(|&r| r > 0.0)
    }
}

/// A little-endian reader over the fixture bytes.
struct Cursor<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize, what: &str) -> Result<&'a [u8], String> {
        let s = self
            .b
            .get(self.at..self.at + n)
            .ok_or_else(|| format!("truncated in {what} at byte {}", self.at))?;
        self.at += n;
        Ok(s)
    }

    fn array<const N: usize>(&mut self, what: &str) -> Result<[u8; N], String> {
        let mut a = [0; N];
        a.copy_from_slice(self.take(N, what)?);
        Ok(a)
    }
}

/// Decodes a fixture into the profile the generator replays, checking every rule of [MP §9.2]: the magic, version 1,
/// the counter-name table equal to [`COUNTERS`], an interval of 1 ms to 60 s, at least one whole sample and no
/// trailing byte, times starting at 0 and strictly increasing, every value NaN or finite and non-negative, and the
/// xxh3-64 checksum of every byte before it. The profile is named by the BLAKE3-256 of the bytes.
pub fn decode(bytes: &[u8]) -> Result<Profile, String> {
    let body_len = bytes
        .len()
        .checked_sub(8)
        .ok_or("shorter than its checksum")?;
    let (body, sum) = bytes.split_at(body_len);
    let mut c = Cursor { b: body, at: 0 };
    if c.take(8, "the magic")? != MAGIC {
        return Err("not a load fixture (bad magic)".into());
    }
    let version = u16::from_le_bytes(c.array("the version")?);
    if version != VERSION {
        return Err(format!("version {version} is not {VERSION}"));
    }
    let count = u16::from_le_bytes(c.array("the counter count")?) as usize;
    if count != C {
        return Err(format!("{count} counters, not the {C} of the format"));
    }
    let interval_ms = u32::from_le_bytes(c.array("the interval")?);
    if !(1..=60_000).contains(&interval_ms) {
        return Err(format!("interval {interval_ms} ms is outside 1 ms to 60 s"));
    }
    for (i, want) in COUNTERS.iter().enumerate() {
        let len = c.take(1, "a counter name")?[0] as usize;
        let name = c.take(len, "a counter name")?;
        if name != want.as_bytes() {
            return Err(format!(
                "counter {} is {:?}, not {want:?}",
                i + 1,
                String::from_utf8_lossy(name)
            ));
        }
    }
    let rows_len = body.len() - c.at;
    if rows_len == 0 || !rows_len.is_multiple_of(ROW) {
        return Err(format!(
            "{rows_len} bytes of samples is not a whole number of {ROW}-byte samples (at least one)"
        ));
    }
    if u64::from_le_bytes(sum.try_into().map_err(|_| "no checksum")?) != xxh3_64(body) {
        return Err("checksum mismatch".into());
    }
    let n = rows_len / ROW;
    let mut times = Vec::with_capacity(n);
    let mut cols: [Vec<f64>; 5] = std::array::from_fn(|_| Vec::with_capacity(n));
    let wanted = [CPU, READ_BYTES, WRITE_BYTES, READS, WRITES];
    for row in 0..n {
        let t = u64::from_le_bytes(c.array("a sample time")?);
        match times.last() {
            None if t != 0 => return Err(format!("the first sample is at {t} ms, not 0")),
            Some(&prev) if t <= prev => {
                return Err(format!(
                    "sample {} at {t} ms does not follow {prev} ms",
                    row + 1
                ));
            }
            _ => {}
        }
        times.push(t);
        for i in 0..C {
            let v = f64::from_bits(u64::from_le_bytes(c.array("a value")?));
            if !(v.is_nan() || (v.is_finite() && v >= 0.0)) {
                return Err(format!("sample {}, counter {}: {v}", row + 1, i + 1));
            }
            if let Some(k) = wanted.iter().position(|&w| w == i) {
                cols[k].push(v);
            }
        }
    }
    let [cpu, read, write, reads, writes] = cols;
    Profile::new(
        Source::Fixture {
            id: blake3::hash(bytes).to_hex().to_string(),
        },
        interval_ms,
        times,
        Series {
            cpu,
            read,
            write,
            reads,
            writes,
        },
    )
}

/// Encodes samples as a sealed fixture ([MP §9.2]); the test suites' writer, independent of `xtask loadrec`'s.
#[cfg(test)]
pub fn encode(interval_ms: u32, samples: &[(u64, [f64; C])]) -> Vec<u8> {
    let mut b = MAGIC.to_vec();
    b.extend_from_slice(&VERSION.to_le_bytes());
    b.extend_from_slice(&(C as u16).to_le_bytes());
    b.extend_from_slice(&interval_ms.to_le_bytes());
    for c in COUNTERS {
        b.push(c.len() as u8);
        b.extend_from_slice(c.as_bytes());
    }
    for (t, values) in samples {
        b.extend_from_slice(&t.to_le_bytes());
        for v in values {
            b.extend_from_slice(&v.to_bits().to_le_bytes());
        }
    }
    let sum = xxh3_64(&b);
    b.extend_from_slice(&sum.to_le_bytes());
    b
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn sample(t: u64, cpu: f64, read: f64, write: f64, reads: f64, writes: f64) -> (u64, [f64; C]) {
        let mut v = [f64::NAN; C];
        (v[CPU], v[READ_BYTES], v[WRITE_BYTES], v[READS], v[WRITES]) =
            (cpu, read, write, reads, writes);
        v[AVAILABLE] = 2e9;
        (t, v)
    }

    fn three() -> Vec<u8> {
        encode(
            1_000,
            &[
                sample(0, 10.0, 4096.0 * 100.0, 0.0, 100.0, 0.0),
                sample(1_000, f64::NAN, 1e6, 2e6, 0.5, 10.0),
                sample(2_500, 90.0, f64::NAN, 3e6, f64::NAN, 1.0),
            ],
        )
    }

    #[test]
    fn decodes_the_replayed_columns() {
        let bytes = three();
        let p = decode(&bytes).unwrap();
        assert_eq!(
            p.source,
            Source::Fixture {
                id: blake3::hash(&bytes).to_hex().to_string()
            }
        );
        assert_eq!(p.times, vec![0, 1_000, 2_500]);
        // Missing values carry the previous one.
        assert_eq!(p.cpu, vec![10.0, 10.0, 90.0]);
        assert_eq!(p.read, vec![409_600.0, 1e6, 1e6]);
        assert_eq!(p.write, vec![0.0, 2e6, 3e6]);
        assert_eq!(p.read_size, vec![4_096, DEFAULT_IO, DEFAULT_IO]);
        assert_eq!(p.write_size, vec![DEFAULT_IO, 48 * 4_096, MAX_IO]);
        assert_eq!(p.duration_ms(), 3_500);
        assert!(p.reads_disk());
    }

    #[test]
    fn decodes_the_hand_written_interop_fixture() {
        // WP-51's interop fixture, testdata/load/three.hex assembled to three.bin, written by hand from [MP §9.2]:
        // `xtask loadrec`'s tests decode the same bytes and record them from a typeperf transcript.
        let bytes = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/testdata/load/three.bin"
        ))
        .unwrap();
        let p = decode(&bytes).unwrap();
        assert_eq!(
            p.source,
            Source::Fixture {
                id: "5b694ded147913955b1c35ad63b33b75042a70acfe9aba7a65b8a21aa92cc295".into()
            }
        );
        assert_eq!(
            (p.interval_ms, p.times.clone()),
            (1_000, vec![0, 1_000, 2_003])
        );
        assert_eq!(p.cpu, vec![37.5, 37.5, 90.0]);
        assert_eq!(p.read, vec![409_600.0, 2e6, 2e6]);
        assert_eq!(p.write, vec![1_048_576.0, 0.0, 3e6]);
        assert_eq!(p.read_size, vec![4_096, DEFAULT_IO, DEFAULT_IO]);
        assert_eq!(p.write_size, vec![65_536, DEFAULT_IO, MAX_IO]);
        assert_eq!(p.duration_ms(), 3_003);
        // This crate's own writer gives the same bytes for the same samples.
        let n = f64::NAN;
        let samples = [
            (
                0,
                [
                    37.5,
                    12.25,
                    25.25,
                    409_600.0,
                    1_048_576.0,
                    100.0,
                    16.0,
                    0.5,
                    2_147_483_648.0,
                    8_589_934_592.0,
                    3.0,
                    7.75,
                    4_294_967_296.0,
                    3_221_225_472.0,
                    n,
                    n,
                    1_500.0,
                    60_000.0,
                ],
            ),
            (
                1_000,
                [
                    n, 10.5, 20.0, 2e6, 0.0, 0.5, 0.0, 1.25, 2e9, 8.6e9, 0.0, 7.75, 4.3e9, 3.3e9,
                    n, n, 1_510.0, 60_010.0,
                ],
            ),
            (
                2_003,
                [
                    90.0, 30.0, 60.0, n, 3e6, n, 1.0, 2.0, 1.9e9, 8.7e9, 12.5, 8.0, 4.4e9, 3.4e9,
                    n, n, 1_520.0, 60_020.0,
                ],
            ),
        ];
        assert_eq!(encode(1_000, &samples), bytes);
    }

    #[test]
    fn lookup_steps_and_repeats() {
        let p = decode(&three()).unwrap();
        assert_eq!(p.at(0).cpu, 10.0);
        assert_eq!(p.at(999).read, 409_600.0);
        assert_eq!(p.at(1_000).write, 2e6);
        assert_eq!(p.at(2_499).write, 2e6);
        assert_eq!(p.at(2_500).cpu, 90.0);
        assert_eq!(p.at(3_499).cpu, 90.0);
        assert_eq!(p.at(3_500).cpu, 10.0);
        assert_eq!(p.at(3_500 * 7 + 1_200).write, 2e6);
    }

    #[test]
    fn io_sizes() {
        assert_eq!(io_size(1e6, 0.0), DEFAULT_IO);
        assert_eq!(io_size(f64::NAN, 10.0), DEFAULT_IO);
        assert_eq!(io_size(0.0, 10.0), DEFAULT_IO);
        assert_eq!(io_size(1_000.0, 10.0), MIN_IO);
        assert_eq!(io_size(65_536.0 * 3.0 + 100.0, 3.0), 65_536);
        assert_eq!(io_size(1e10, 1.0), MAX_IO);
    }

    #[test]
    fn refuses_what_the_format_refuses() {
        let good = three();
        let reseal = |mut b: Vec<u8>| {
            b.truncate(b.len() - 8);
            let s = xxh3_64(&b);
            b.extend_from_slice(&s.to_le_bytes());
            b
        };
        let damaged = |at: usize, bytes: &[u8]| {
            let mut b = good.clone();
            b[at..at + bytes.len()].copy_from_slice(bytes);
            decode(&reseal(b)).unwrap_err()
        };
        assert!(damaged(1, &[0x8D]).contains("magic"));
        assert!(damaged(8, &[2]).contains("version"));
        assert!(damaged(10, &[17]).contains("17 counters"));
        assert!(damaged(12, &[0, 0, 0, 0]).contains("interval"));
        assert!(damaged(17, b"/").contains("counter 1"));
        let h = good.len() - 8 - 3 * ROW;
        assert!(damaged(h, &[1]).contains("first sample"));
        assert!(damaged(h + ROW, &[0; 8]).contains("does not follow"));
        assert!(damaged(h + 8, &(-0.5f64).to_bits().to_le_bytes()).contains("sample 1, counter 1"));
        assert!(damaged(h + 16, &f64::INFINITY.to_bits().to_le_bytes()).contains("counter 2"));
        let mut b = good.clone();
        b[h + 8] ^= 1;
        assert!(decode(&b).unwrap_err().contains("checksum"));
        assert!(
            decode(&reseal(good[..h + 8].to_vec()))
                .unwrap_err()
                .contains("whole number")
        );
        let mut longer = good.clone();
        longer.insert(h, 0);
        assert!(decode(&reseal(longer)).is_err());
        assert!(decode(&good[..7]).is_err());
    }

    #[test]
    fn a_replayed_counter_without_any_value_is_refused() {
        let b = encode(1_000, &[sample(0, f64::NAN, 1.0, 1.0, 1.0, 1.0)]);
        assert!(decode(&b).unwrap_err().contains("% Processor Time"));
        let b = encode(1_000, &[sample(0, 1.0, 1.0, f64::NAN, 1.0, 1.0)]);
        assert!(decode(&b).unwrap_err().contains("Disk Write Bytes/sec"));
    }

    #[test]
    fn profile_new_checks_its_series() {
        let src = Source::Synthetic { spec: "s".into() };
        let one = |cpu: f64| Series {
            cpu: vec![cpu],
            read: vec![0.0],
            write: vec![1.0],
            reads: vec![f64::NAN],
            writes: vec![0.0],
        };
        assert!(Profile::new(src.clone(), 1_000, vec![], Series::default()).is_err());
        assert!(Profile::new(src.clone(), 1_000, vec![5], one(1.0)).is_err());
        assert!(Profile::new(src.clone(), 0, vec![0], one(1.0)).is_err());
        assert!(Profile::new(src.clone(), 1_000, vec![0, 1], one(1.0)).is_err());
        let p = Profile::new(src, 1_000, vec![0], one(150.0)).unwrap();
        assert!(!p.reads_disk());
        assert_eq!((p.read_size[0], p.write_size[0]), (DEFAULT_IO, DEFAULT_IO));
        // Processor time above 100 % (rounding in a counter) is replayed as 100 %.
        assert_eq!(p.at(0).cpu, 100.0);
    }

    proptest! {
        /// Every encoded fixture decodes to its replayed columns, with missing values carried forward.
        #[test]
        fn round_trip(rows in proptest::collection::vec(
            (1u64..5_000, proptest::option::of(0.0f64..100.0), proptest::option::of(0.0f64..1e9),
             proptest::option::of(0.0f64..1e9)), 1..40)) {
            let mut t = 0;
            let mut samples = Vec::new();
            for (i, (gap, cpu, read, write)) in rows.iter().enumerate() {
                if i > 0 {
                    t += gap;
                }
                let f = |v: &Option<f64>| v.unwrap_or(f64::NAN);
                samples.push(sample(t, f(cpu), f(read), f(write), 10.0, 10.0));
            }
            let all_missing = |k: usize| samples.iter().all(|s| s.1[k].is_nan());
            match decode(&encode(1_000, &samples)) {
                Ok(p) => {
                    prop_assert!(!all_missing(CPU) && !all_missing(READ_BYTES) && !all_missing(WRITE_BYTES));
                    prop_assert_eq!(p.times.len(), samples.len());
                    for (i, s) in samples.iter().enumerate() {
                        if !s.1[CPU].is_nan() {
                            prop_assert_eq!(p.cpu[i], s.1[CPU]);
                        } else if i > 0 {
                            prop_assert_eq!(p.cpu[i], p.cpu[i - 1]);
                        }
                        prop_assert_eq!(p.at(s.0).write, p.write[i]);
                    }
                }
                Err(_) => prop_assert!(all_missing(CPU) || all_missing(READ_BYTES) || all_missing(WRITE_BYTES)),
            }
        }
    }
}
