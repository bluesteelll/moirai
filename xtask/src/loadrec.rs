//! `cargo xtask loadrec` (WP-51a): records measurement 16's load fixture, the system-wide counters of one normal
//! 16-agent campaign of the owner (V5), into `/private/load/`. `[MP §x]` cites `docs/spec/measurement-protocol.md`;
//! the owner's procedure is `docs/measurements/v5-load-recording.md`. Sources: docs/m0/PLAN.md §3.2 WP-51 and §5 V5,
//! [60 §5.1] (the Load row), [60 §5.2] item 16, [AR §11] #35 and #37.
//!
//! | Command | What it does |
//! |---|---|
//! | `loadrec start [--max-duration <seconds>] [--private-dir <dir>]` | records until stopped ([MP §9.3]) |
//! | `loadrec stop [--private-dir <dir>]` | stops the running recorder, or seals what an interrupted one left |
//! | `loadrec check <file>` | validates a fixture ([MP §9.2]) and prints a summary of its numbers |
//!
//! **What is recorded** ([MP §9.1]): the 18 counters of [`COUNTERS`], all of them system-wide (the `_Total` instance of
//! Processor, PhysicalDisk, Paging File and Process, and the single-instance Memory object), read by `typeperf` once a
//! second. Each sample is stored as its arrival time in milliseconds since the first sample and 18 numbers; a counter
//! typeperf cannot read when it starts is missing in every sample, except the [`REQUIRED`] ones, without which the
//! recording does not start. **What is
//! not**: everything else typeperf prints — its `(PDH-CSV 4.0)` banner and time-zone note, the `\\<machine>` prefix of
//! every column, the wall-clock timestamp of every sample and its closing messages — is read from its standard output
//! in memory and dropped; typeperf never writes a file. No per-process instance, no other counter object, and no
//! event trace is read. The fixture's only text is the counter-name table ([MP §9.2]), which the tests below prove.
//!
//! **WPR is never run.** PLAN WP-51 allows WPR only with WP-06's counters-only profile; WP-06 checked in none
//! (`docs/m0/tools.md` §10) because every counter of [`COUNTERS`] is a performance counter typeperf reads, so no ETL
//! file is ever written.
//!
//! **While recording** ([MP §9.3]) the recorder prints a progress line a minute with the missing values of the
//! required counters, and warns from the 60th sample on about a required counter that has had no value yet, so a
//! useless fixture is noticed at once. The private manifest leaves out the growing `*.load.partial` and the stop
//! request (`private.rs`), so commits keep working for the whole campaign; sealing works in place and never cuts a
//! recorded sample.
//!
//! The recorder is a tool, not a measurement: it starts no load, and it never runs on a hosted runner (the fixture
//! stays on the owner's laptop, [AR §11] #37).

use crate::private;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, IsTerminal, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use xxhash_rust::xxh3::{Xxh3, xxh3_64};

/// The usage text.
pub const USAGE: &str = "usage: cargo xtask loadrec start [--max-duration <seconds>] [--private-dir <dir>]
       cargo xtask loadrec stop [--private-dir <dir>]
       cargo xtask loadrec check <file>
start records the system-wide counters of docs/spec/measurement-protocol.md §9.1 with typeperf once a second into
/private/load/<start>.load until Enter is pressed in its window, `loadrec stop` is run, typeperf ends, or the maximum
duration passes (default 12 hours); then it rebuilds /private/MANIFEST.b3. Only counter names and numbers are stored.";

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
const C: usize = COUNTERS.len();
/// typeperf's sample interval, in milliseconds (`-si 1`, [MP §9.3]).
pub const INTERVAL_MS: u32 = 1_000;
/// The fixture's first 8 bytes ([MP §9.2]): a NUL first (the private index treats the file as binary and takes no
/// shingles of its numbers), CR LF ^Z LF against text-mode damage, and no printable character.
pub const MAGIC: [u8; 8] = [0x00, 0x8C, 0x9A, 0x0D, 0x0A, 0x1A, 0x0A, 0x00];
/// The format version.
pub const VERSION: u16 = 1;
/// The bytes of one sample: its time and one binary64 value per counter.
const ROW: usize = 8 + 8 * C;
/// The longest recording unless `--max-duration` says otherwise.
pub const DEFAULT_MAX_DURATION: Duration = Duration::from_secs(12 * 3_600);
/// The directory under `/private/` the recorder writes into.
pub const LOAD_DIR: &str = "load";
/// The suffix of the file being recorded. The private manifest leaves it out ([MP §9.3], `private.rs`).
pub const PARTIAL: &str = ".load.partial";
/// The suffix of a sealed fixture.
const SEALED: &str = ".load";
/// The file `loadrec stop` creates to ask the recorder to stop. The private manifest leaves it out ([MP §9.3]).
pub const STOP_REQUEST: &str = "stop.request";
/// From this many samples on, every progress line warns about a required counter that has had no value yet
/// ([MP §9.3]).
pub const WARN_AFTER: u64 = 60;
/// How long `loadrec stop` waits for the recorder.
const STOP_WAIT: Duration = Duration::from_secs(30);
/// A partial file that has not grown for this long belongs to no running recorder (one sample a second is appended).
const STALL: Duration = Duration::from_secs(10);
/// How often the recorder looks for a stop request, and `loadrec stop` for the result.
const POLL: Duration = Duration::from_millis(250);

// ---------------------------------------------------------------------------------------------------------------
// The fixture format ([MP §9.2])

/// The fixture header: magic, version, counter count, interval, and the counter-name table.
pub fn header() -> Vec<u8> {
    let mut h = Vec::with_capacity(16 + COUNTERS.iter().map(|c| 1 + c.len()).sum::<usize>());
    h.extend_from_slice(&MAGIC);
    h.extend_from_slice(&VERSION.to_le_bytes());
    h.extend_from_slice(&(C as u16).to_le_bytes());
    h.extend_from_slice(&INTERVAL_MS.to_le_bytes());
    for c in COUNTERS {
        h.push(c.len() as u8);
        h.extend_from_slice(c.as_bytes());
    }
    h
}

/// A decoded fixture ([MP §9.2]).
#[derive(Clone, Debug, PartialEq)]
pub struct Fixture {
    /// The sample interval in milliseconds.
    pub interval_ms: u32,
    /// The counter names, in column order (always [`COUNTERS`]).
    pub names: Vec<String>,
    /// Each sample's time in milliseconds since the first.
    pub times: Vec<u64>,
    /// The values, sample by sample, [`C`] per sample; NaN where a counter had no value.
    pub values: Vec<f64>,
}

impl Fixture {
    /// The values of column `i`, in sample order.
    pub fn column(&self, i: usize) -> impl Iterator<Item = f64> + '_ {
        self.values.iter().skip(i).step_by(C).copied()
    }

    /// The covered time: the last sample's time plus one interval.
    pub fn duration_ms(&self) -> u64 {
        self.times
            .last()
            .map_or(0, |t| t + u64::from(self.interval_ms))
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

    fn u8(&mut self, what: &str) -> Result<u8, String> {
        Ok(self.take(1, what)?[0])
    }

    fn u16(&mut self, what: &str) -> Result<u16, String> {
        Ok(u16::from_le_bytes(
            self.take(2, what)?.try_into().unwrap_or([0; 2]),
        ))
    }

    fn u32(&mut self, what: &str) -> Result<u32, String> {
        Ok(u32::from_le_bytes(
            self.take(4, what)?.try_into().unwrap_or([0; 4]),
        ))
    }

    fn u64(&mut self, what: &str) -> Result<u64, String> {
        Ok(u64::from_le_bytes(
            self.take(8, what)?.try_into().unwrap_or([0; 8]),
        ))
    }
}

/// Decodes and validates a sealed fixture ([MP §9.2]): the magic, version 1, the counter-name table equal to
/// [`COUNTERS`] (the only text the format can hold), an interval of 1 ms to 60 s, at least one whole sample and no
/// trailing byte, times starting at 0 and strictly increasing, every value NaN or finite and non-negative, and the
/// xxh3-64 checksum of every byte before it.
pub fn decode(bytes: &[u8]) -> Result<Fixture, String> {
    let body_len = bytes
        .len()
        .checked_sub(8)
        .ok_or("shorter than its checksum")?;
    let (body, sum) = bytes.split_at(body_len);
    let mut c = Cursor { b: body, at: 0 };
    if c.take(8, "the magic")? != MAGIC {
        return Err("not a load fixture (bad magic)".into());
    }
    let version = c.u16("the version")?;
    if version != VERSION {
        return Err(format!("version {version} is not {VERSION}"));
    }
    let count = c.u16("the counter count")? as usize;
    if count != C {
        return Err(format!("{count} counters, not the {C} of the format"));
    }
    let interval_ms = c.u32("the interval")?;
    if !(1..=60_000).contains(&interval_ms) {
        return Err(format!("interval {interval_ms} ms is outside 1 ms to 60 s"));
    }
    let mut names = Vec::with_capacity(C);
    for (i, want) in COUNTERS.iter().enumerate() {
        let len = c.u8("a counter name")? as usize;
        let name = c.take(len, "a counter name")?;
        if name != want.as_bytes() {
            return Err(format!(
                "counter {} is {:?}, not {want:?}",
                i + 1,
                String::from_utf8_lossy(name)
            ));
        }
        names.push((*want).to_string());
    }
    let rows_len = body.len() - c.at;
    if rows_len == 0 || !rows_len.is_multiple_of(ROW) {
        return Err(format!(
            "{rows_len} bytes of samples is not a whole number of {ROW}-byte samples (at least one)"
        ));
    }
    if u64::from_le_bytes(sum.try_into().unwrap_or([0; 8])) != xxh3_64(body) {
        return Err("checksum mismatch".into());
    }
    let n = rows_len / ROW;
    let mut times = Vec::with_capacity(n);
    let mut values = Vec::with_capacity(n * C);
    for row in 0..n {
        let t = c.u64("a sample time")?;
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
            let v = f64::from_bits(c.u64("a value")?);
            if !(v.is_nan() || (v.is_finite() && v >= 0.0)) {
                return Err(format!("sample {}, counter {}: {v}", row + 1, i + 1));
            }
            values.push(v);
        }
    }
    Ok(Fixture {
        interval_ms,
        names,
        times,
        values,
    })
}

// ---------------------------------------------------------------------------------------------------------------
// typeperf's output

/// The fields of one line of typeperf's CSV output, where every field is quoted and fields are separated by `","`;
/// `None` for any other line (blank lines and typeperf's messages).
fn csv_fields(line: &str) -> Option<Vec<&str>> {
    let inner = line.strip_prefix('"')?.strip_suffix('"')?;
    let fields: Vec<&str> = inner.split("\",\"").collect();
    (!fields.iter().any(|f| f.contains('"'))).then_some(fields)
}

/// The counter path of a header column without the `\\<machine>` prefix typeperf puts before it.
fn counter_path(column: &str) -> &str {
    match column.strip_prefix(r"\\") {
        Some(rest) => rest.find('\\').map_or("", |i| &rest[i..]),
        None => column,
    }
}

/// The counters a fixture cannot do without: the ones `moirai-probes-bin loadgen` replays ([MP §9.4]). typeperf drops
/// a counter it cannot read when it starts (on a loaded machine `\Process(_Total)\IO Read Bytes/sec` and
/// `IO Write Bytes/sec` sometimes are); any other counter may then be absent, and its column holds only missing values.
pub const REQUIRED: [usize; 5] = [0, 3, 4, 5, 6];

/// Where each counter of [`COUNTERS`] is in typeperf's header: its field index, or `None` when typeperf left it out.
type ColumnMap = [Option<usize>; C];

/// Maps typeperf's header line onto [`COUNTERS`]: after the timestamp column, each column is one counter of
/// [`COUNTERS`], compared without the machine prefix and without regard to ASCII case; a column that is no such
/// counter, a counter reported twice, and an absent [`REQUIRED`] counter are errors. A message never quotes the
/// machine name.
fn map_header(fields: &[&str]) -> Result<ColumnMap, String> {
    let mut map: ColumnMap = [None; C];
    for (k, column) in fields.iter().enumerate().skip(1) {
        let got = counter_path(column);
        let i = COUNTERS
            .iter()
            .position(|c| c.eq_ignore_ascii_case(got))
            .ok_or_else(|| {
                format!(
                    "typeperf column {k} is {got:?}, which is no counter of [MP §9.1]; the recorder needs the English counter names"
                )
            })?;
        if map[i].replace(k).is_some() {
            return Err(format!("typeperf reports {:?} twice", COUNTERS[i]));
        }
    }
    let missing: Vec<&str> = REQUIRED
        .iter()
        .filter(|&&i| map[i].is_none())
        .map(|&i| COUNTERS[i])
        .collect();
    if missing.is_empty() {
        Ok(map)
    } else {
        Err(format!(
            "typeperf cannot read {}, which the fixture needs",
            missing.join(", ")
        ))
    }
}

/// One value of a sample: a non-negative decimal number as typeperf prints it (`12.345678`). A blank field (a
/// counter with no value in that sample) and anything else are missing: NaN ([MP §9.2]).
fn value(field: &str) -> f64 {
    let s = field.trim();
    let mut dots = 0;
    for (i, b) in s.bytes().enumerate() {
        match b {
            b'0'..=b'9' => {}
            b'.' if i > 0 && dots == 0 => dots += 1,
            _ => return f64::NAN,
        }
    }
    if s.is_empty() || s.ends_with('.') {
        return f64::NAN;
    }
    s.parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
        .unwrap_or(f64::NAN)
}

/// What one line of typeperf's output was.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Fed {
    /// A blank line.
    Blank,
    /// The header, checked against [`COUNTERS`].
    Header,
    /// A sample, appended to the fixture.
    Sample,
    /// A sample line whose field count is not its header's: skipped and counted.
    Malformed,
    /// Any other line: a message of typeperf's. It is kept in memory for an error message and never written.
    Message,
}

/// Turns typeperf's standard output into fixture samples ([MP §9.3]). Only numbers reach `out`: the timestamp of
/// each sample is replaced by its arrival time, and every other string typeperf prints is dropped.
pub struct Recorder<W: Write> {
    out: W,
    /// Where each counter is in typeperf's samples, once its header has been seen.
    map: Option<ColumnMap>,
    first_ms: Option<u64>,
    last_t: u64,
    /// Samples written.
    pub samples: u64,
    /// Values written as missing.
    pub missing: u64,
    /// Values written as missing, per counter of [`COUNTERS`].
    pub missing_by: [u64; C],
    /// Sample lines skipped because their field count was not the header's.
    pub malformed: u64,
    /// The first few message lines, for an error message only.
    messages: Vec<String>,
    row: Vec<u8>,
}

impl<W: Write> Recorder<W> {
    /// Writes the header to `out`.
    pub fn new(mut out: W) -> std::io::Result<Recorder<W>> {
        out.write_all(&header())?;
        out.flush()?;
        Ok(Recorder {
            out,
            map: None,
            first_ms: None,
            last_t: 0,
            samples: 0,
            missing: 0,
            missing_by: [0; C],
            malformed: 0,
            messages: Vec::new(),
            row: Vec::with_capacity(ROW),
        })
    }

    /// Takes one line of typeperf's output that arrived `arrival_ms` milliseconds after the recording started. A sample
    /// is written and flushed at once, so an interrupted recorder leaves every whole sample behind. A header that does
    /// not map onto [`COUNTERS`] and a failed write are errors; a sample line of the wrong width is skipped.
    pub fn feed(&mut self, line: &str, arrival_ms: u64) -> Result<Fed, String> {
        let line = line.trim_end_matches(['\r', '\n']);
        if line.trim().is_empty() {
            return Ok(Fed::Blank);
        }
        let fields = match csv_fields(line) {
            Some(f) if self.map.is_some() || f[0].starts_with("(PDH-CSV") => f,
            _ => {
                if self.messages.len() < 8 {
                    self.messages.push(line.to_string());
                }
                return Ok(Fed::Message);
            }
        };
        let Some(map) = self.map else {
            self.map = Some(map_header(&fields)?);
            return Ok(Fed::Header);
        };
        if fields.len() != map.iter().flatten().count() + 1 {
            self.malformed += 1;
            return Ok(Fed::Malformed);
        }
        let first = *self.first_ms.get_or_insert(arrival_ms);
        let mut t = arrival_ms.saturating_sub(first);
        if self.samples > 0 && t <= self.last_t {
            t = self.last_t + 1;
        }
        self.row.clear();
        self.row.extend_from_slice(&t.to_le_bytes());
        for (i, column) in map.into_iter().enumerate() {
            let v = column.map_or(f64::NAN, |k| value(fields[k]));
            if v.is_nan() {
                self.missing += 1;
                self.missing_by[i] += 1;
            }
            self.row.extend_from_slice(&v.to_bits().to_le_bytes());
        }
        self.out
            .write_all(&self.row)
            .and_then(|()| self.out.flush())
            .map_err(|e| format!("writing a sample: {e}"))?;
        self.last_t = t;
        self.samples += 1;
        Ok(Fed::Sample)
    }

    /// Whether typeperf's header has been seen.
    pub fn header_seen(&self) -> bool {
        self.map.is_some()
    }

    /// The counters typeperf's header left out; their columns hold only missing values.
    pub fn absent(&self) -> Vec<&'static str> {
        self.map.map_or_else(Vec::new, |m| {
            COUNTERS
                .into_iter()
                .zip(m)
                .filter(|(_, k)| k.is_none())
                .map(|(c, _)| c)
                .collect()
        })
    }

    /// The [`REQUIRED`] counters that have had no value in any sample so far: typeperf reads them as `-1` or a blank,
    /// or prints them in a form [`value`] does not take (another decimal separator). `moirai-probes-bin loadgen`
    /// refuses a fixture without any value of #0, #3 or #4 ([MP §9.4]), so a recording with one is of no use.
    pub fn silent_required(&self) -> Vec<&'static str> {
        if self.samples == 0 {
            return Vec::new();
        }
        REQUIRED
            .iter()
            .filter(|&&i| self.missing_by[i] == self.samples)
            .map(|&i| COUNTERS[i])
            .collect()
    }

    /// The counters with missing values and how many, in column order: `none`, or `name n, name n`.
    pub fn missing_list(&self, only_required: bool) -> String {
        let list: Vec<String> = (0..C)
            .filter(|&i| self.missing_by[i] > 0 && (!only_required || REQUIRED.contains(&i)))
            .map(|i| format!("{} {}", COUNTERS[i], self.missing_by[i]))
            .collect();
        if list.is_empty() {
            "none".into()
        } else {
            list.join(", ")
        }
    }

    /// The progress lines printed once a minute ([MP §9.3]): the samples, the time recorded and the missing values of
    /// the required counters; from [`WARN_AFTER`] samples on, a warning for each required counter without any value.
    pub fn progress(&self) -> Vec<String> {
        let mut out = vec![format!(
            "loadrec: {} samples, {} recorded; missing values of the required counters: {}",
            self.samples,
            hms(self.last_t),
            self.missing_list(true)
        )];
        if self.samples >= WARN_AFTER {
            for c in self.silent_required() {
                out.push(format!(
                    "loadrec: WARNING: {c} has had no value in any of the {} samples; loadgen cannot replay a fixture \
                     without it. Stop the recording (Enter), check `typeperf \"{c}\" -sc 5`, and report it",
                    self.samples
                ));
            }
        }
        out
    }

    /// Flushes and returns the writer.
    pub fn finish(mut self) -> std::io::Result<W> {
        self.out.flush()?;
        Ok(self.out)
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Files under /private/load/

/// Whether `file` starts with the recorder's [`header`], and its length. Reads the header's bytes only.
fn partial_state(file: &mut File) -> std::io::Result<(bool, u64)> {
    let len = file.metadata()?.len();
    let h = header();
    if len < h.len() as u64 {
        return Ok((false, len));
    }
    let mut head = vec![0; h.len()];
    file.seek(SeekFrom::Start(0))?;
    file.read_exact(&mut head)?;
    Ok((head == h, len))
}

/// Seals a partial fixture in place ([MP §9.3]): cuts a trailing incomplete sample, appends the checksum, syncs, and
/// renames `<start>.load.partial` to `<start>.load`. The file is never truncated below its whole samples, so a crash,
/// kill or power loss while sealing loses no sample: it leaves a partial that seals again. The checksum is streamed
/// over the kept bytes, so sealing holds one 64 KiB buffer whatever the recording's length. Sealing a file again after
/// a crash between the write and the rename gives the same bytes (the old checksum is shorter than a sample, so it is
/// cut as an incomplete one). Returns the sealed path and its sample count.
pub fn seal(partial: &Path) -> Result<(PathBuf, u64), String> {
    let name = partial
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("the partial fixture has no file name")?;
    let stem = name
        .strip_suffix(PARTIAL)
        .ok_or_else(|| format!("{name} is not a partial fixture (*{PARTIAL})"))?;
    let sealed = partial.with_file_name(format!("{stem}{SEALED}"));
    if sealed.exists() {
        return Err(format!("{stem}{SEALED} already exists"));
    }
    let io = |e: std::io::Error| format!("{name}: {e}");
    let mut f = OpenOptions::new()
        .read(true)
        .write(true)
        .open(partial)
        .map_err(io)?;
    let (ours, len) = partial_state(&mut f).map_err(io)?;
    if !ours {
        return Err(format!(
            "{name}: not a partial fixture of this recorder (bad header)"
        ));
    }
    let h = header().len() as u64;
    let n = (len - h) / ROW as u64;
    if n == 0 {
        return Err(format!("{name}: no whole sample was recorded"));
    }
    let keep = h + n * ROW as u64;
    let mut hasher = Xxh3::new();
    let mut buf = vec![0u8; 64 * 1024];
    f.seek(SeekFrom::Start(0)).map_err(io)?;
    let mut left = keep;
    while left > 0 {
        let k = left.min(buf.len() as u64) as usize;
        f.read_exact(&mut buf[..k]).map_err(io)?;
        hasher.update(&buf[..k]);
        left -= k as u64;
    }
    f.set_len(keep)
        .and_then(|()| f.seek(SeekFrom::Start(keep)))
        .and_then(|_| f.write_all(&hasher.digest().to_le_bytes()))
        .and_then(|()| f.sync_all())
        .map_err(io)?;
    drop(f);
    std::fs::rename(partial, &sealed).map_err(io)?;
    Ok((sealed, n))
}

/// The partial fixtures in `dir`, sorted.
fn partials(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let rd = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("{}: {e}", dir.display())),
    };
    let mut out = Vec::new();
    for e in rd {
        let p = e.map_err(|e| format!("{}: {e}", dir.display()))?.path();
        if p.file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.ends_with(PARTIAL))
        {
            out.push(p);
        }
    }
    out.sort();
    Ok(out)
}

/// The file name of `p`, for messages that must not show a user path.
fn file_name(p: &Path) -> String {
    p.file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
}

/// UTC as `YYYYMMDDTHHMMSSZ`, the stem of a fixture's file name.
fn compact_utc(t: SystemTime) -> String {
    let secs = t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()) as i64;
    let z = secs.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    let s = secs.rem_euclid(86_400);
    format!(
        "{y:04}{m:02}{d:02}T{:02}{:02}{:02}Z",
        s / 3_600,
        s / 60 % 60,
        s % 60
    )
}

/// `%SystemRoot%\System32\typeperf.exe`, named by its full path so that no program of the same name on `PATH` runs
/// in its place ([MP §2.2]'s rule for system programs).
fn typeperf() -> Result<PathBuf, String> {
    let root = std::env::var_os("SystemRoot")
        .filter(|r| !r.is_empty())
        .ok_or("%SystemRoot% is not set: loadrec records on Windows only")?;
    let p = Path::new(&root).join("System32").join("typeperf.exe");
    if p.is_file() {
        Ok(p)
    } else {
        Err("typeperf.exe is not in %SystemRoot%\\System32: loadrec records on Windows only".into())
    }
}

/// Rebuilds `/private/MANIFEST.b3`: a new file under `/private/` makes the manifest stale, and the pre-commit guard
/// refuses every commit until it is rebuilt (docs/m0/PLAN.md §3.2 WP-03).
fn reindex(repo: &Path, private_dir: &Path) {
    match private::rebuild(repo, private_dir, Some("master")) {
        Ok((st, _)) => println!("loadrec: /private/MANIFEST.b3 rebuilt ({} files)", st.files),
        Err(e) => eprintln!(
            "loadrec: /private/MANIFEST.b3 could not be rebuilt ({e}); run `cargo xtask private index` before the next commit"
        ),
    }
}

// ---------------------------------------------------------------------------------------------------------------
// start, stop, check

/// An event of the recording loop.
enum Event {
    /// A line of typeperf's output and when it arrived.
    Line(String, Instant),
    /// typeperf's output ended, with the read error if any.
    End(Option<String>),
    /// Enter was pressed in the recorder's window.
    Enter,
}

/// Why the recording loop ended, when it ended without an error.
#[derive(Debug, Eq, PartialEq)]
enum Reason {
    Keyboard,
    StopRequest,
    MaxDuration,
    OutputEnded,
}

impl Reason {
    fn text(&self) -> &'static str {
        match self {
            Reason::Keyboard => "Enter was pressed",
            Reason::StopRequest => "`loadrec stop` asked",
            Reason::MaxDuration => "the maximum duration passed",
            Reason::OutputEnded => "typeperf's output ended",
        }
    }
}

/// The recording loop: feeds every line to `rec` and ends on Enter, a stop request in `dir`, `max` after `t0`, or the
/// end of typeperf's output.
fn record_loop<W: Write>(
    rx: &Receiver<Event>,
    rec: &mut Recorder<W>,
    dir: &Path,
    t0: Instant,
    max: Duration,
) -> Result<Reason, String> {
    loop {
        match rx.recv_timeout(POLL) {
            Ok(Event::Line(line, at)) => {
                let ms =
                    u64::try_from(at.saturating_duration_since(t0).as_millis()).unwrap_or(u64::MAX);
                match rec.feed(&line, ms)? {
                    Fed::Header => {
                        let absent = rec.absent();
                        if !absent.is_empty() {
                            println!(
                                "loadrec: typeperf cannot read {}; their columns stay empty",
                                absent.join(", ")
                            );
                        }
                        println!(
                            "loadrec: recording (press Enter here or run `cargo xtask loadrec stop` to stop)"
                        );
                    }
                    Fed::Sample if rec.samples.is_multiple_of(60) => {
                        rec.progress().iter().for_each(|l| println!("{l}"));
                    }
                    Fed::Malformed => println!(
                        "loadrec: skipped a typeperf sample of the wrong width ({} so far)",
                        rec.malformed
                    ),
                    _ => {}
                }
            }
            Ok(Event::Enter) => return Ok(Reason::Keyboard),
            Ok(Event::End(None)) | Err(RecvTimeoutError::Disconnected) => {
                return Ok(Reason::OutputEnded);
            }
            Ok(Event::End(Some(e))) => return Err(format!("reading typeperf's output: {e}")),
            Err(RecvTimeoutError::Timeout) => {}
        }
        if dir.join(STOP_REQUEST).exists() {
            return Ok(Reason::StopRequest);
        }
        if t0.elapsed() >= max {
            return Ok(Reason::MaxDuration);
        }
    }
}

/// `h:mm:ss` of a duration in milliseconds.
fn hms(ms: u64) -> String {
    let s = ms / 1_000;
    format!("{}:{:02}:{:02}", s / 3_600, s / 60 % 60, s % 60)
}

/// `loadrec start` ([MP §9.3]).
pub fn start(repo: &Path, private_dir: &Path, max: Duration) -> Result<(), String> {
    let dir = private_dir.join(LOAD_DIR);
    std::fs::create_dir_all(&dir).map_err(|e| format!("/private/load: {e}"))?;
    if let Some(p) = partials(&dir)?.first() {
        return Err(format!(
            "{} exists: a recording is running or was interrupted; run `cargo xtask loadrec stop` first",
            file_name(p)
        ));
    }
    let _ = std::fs::remove_file(dir.join(STOP_REQUEST));
    let program = typeperf()?;
    let partial = dir.join(format!("{}{PARTIAL}", compact_utc(SystemTime::now())));
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&partial)
        .map_err(|e| format!("{}: {e}", file_name(&partial)))?;
    let started = Recorder::new(BufWriter::new(file))
        .map_err(|e| format!("writing the fixture: {e}"))
        .and_then(|rec| {
            Command::new(&program)
                .args(COUNTERS)
                .args(["-si", "1"])
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .map(|child| (rec, child))
                .map_err(|e| format!("typeperf could not be started: {e}"))
        });
    let (mut rec, mut child) = match started {
        Ok(v) => v,
        Err(e) => {
            let _ = std::fs::remove_file(&partial);
            return Err(e);
        }
    };
    let (tx, rx) = mpsc::channel();
    if let Some(stdout) = child.stdout.take() {
        let tx = tx.clone();
        std::thread::spawn(move || {
            let mut r = BufReader::new(stdout);
            let mut buf = Vec::new();
            loop {
                buf.clear();
                match r.read_until(b'\n', &mut buf) {
                    Ok(0) => break tx.send(Event::End(None)),
                    Ok(_) => {
                        let line = String::from_utf8_lossy(&buf).into_owned();
                        if tx.send(Event::Line(line, Instant::now())).is_err() {
                            break Ok(());
                        }
                    }
                    Err(e) => break tx.send(Event::End(Some(e.to_string()))),
                }
            }
        });
    }
    if std::io::stdin().is_terminal() {
        std::thread::spawn(move || {
            let mut s = String::new();
            // End of input is not a request to stop; only a line is.
            if std::io::stdin().read_line(&mut s).is_ok_and(|n| n > 0) {
                let _ = tx.send(Event::Enter);
            }
        });
    } else {
        drop(tx);
    }
    println!(
        "loadrec: waiting for typeperf's first sample (up to a minute on a busy machine), then recording to /private/load/{} (at most {})",
        file_name(&partial),
        hms(u64::try_from(max.as_millis()).unwrap_or(u64::MAX))
    );
    let ended = record_loop(&rx, &mut rec, &dir, Instant::now(), max);
    let _ = child.kill();
    let _ = child.wait();
    let r = finish_recording(ended, rec, &partial, &dir)?;
    println!(
        "loadrec: stopped ({}); /private/load/{}: {} samples, {} missing values, {} malformed lines skipped, BLAKE3 {}",
        match &r.ended {
            Ok(reason) => reason.text(),
            Err(_) => "an error",
        },
        file_name(&r.sealed),
        r.samples,
        r.missing,
        r.malformed,
        r.id
    );
    println!("loadrec: missing values per counter: {}", r.missing_by);
    for c in &r.silent {
        println!(
            "loadrec: WARNING: {c} has no value in any sample; loadgen cannot replay this fixture without it"
        );
    }
    reindex(repo, private_dir);
    r.ended.map(|_| ()).map_err(|e| {
        format!(
            "the recording stopped early: {e}; the samples before it are sealed in /private/load/{}",
            file_name(&r.sealed)
        )
    })
}

/// What a recording left once [`finish_recording`] sealed it.
#[derive(Debug)]
struct Recorded {
    /// The sealed fixture.
    sealed: PathBuf,
    /// Its samples.
    samples: u64,
    /// Its missing values.
    missing: u64,
    /// typeperf's sample lines skipped for their width.
    malformed: u64,
    /// The counters with missing values and how many ([`Recorder::missing_list`]).
    missing_by: String,
    /// The required counters without any value ([`Recorder::silent_required`]).
    silent: Vec<&'static str>,
    /// The fixture's id: the BLAKE3-256 of the sealed file in lower-case hex ([MP §9.2]).
    id: String,
    /// Why the recording stopped, or the error that stopped it early.
    ended: Result<Reason, String>,
}

/// The end of `loadrec start` once its loop has ended ([MP §9.3]): flushes the recorder and removes the stop request;
/// then a recording without typeperf's header or without any sample leaves no file (the error says why), and one with
/// samples is sealed, even when it stopped on an error or the last flush failed (what reached the file is kept).
fn finish_recording<W: Write>(
    ended: Result<Reason, String>,
    mut rec: Recorder<W>,
    partial: &Path,
    dir: &Path,
) -> Result<Recorded, String> {
    let header_seen = rec.header_seen();
    let messages = std::mem::take(&mut rec.messages);
    let (samples, missing, malformed) = (rec.samples, rec.missing, rec.malformed);
    let (missing_by, silent) = (rec.missing_list(false), rec.silent_required());
    // A failed flush loses at most the samples still buffered; what reached the file is sealed below.
    let ended = match (ended, rec.finish()) {
        (Ok(r), Ok(_)) => Ok(r),
        (Err(e), _) => Err(e),
        (Ok(_), Err(e)) => Err(format!("writing the fixture: {e}")),
    };
    let _ = std::fs::remove_file(dir.join(STOP_REQUEST));
    if !header_seen || samples == 0 {
        let _ = std::fs::remove_file(partial);
        return Err(match ended {
            Err(e) => e,
            Ok(_) if !header_seen => format!(
                "typeperf printed no header; its output: {}",
                messages.join(" | ")
            ),
            Ok(_) => "no sample was recorded".into(),
        });
    }
    let (sealed, n) = seal(partial)?;
    let id = File::open(&sealed)
        .and_then(|f| {
            let mut h = blake3::Hasher::new();
            h.update_reader(f)?;
            Ok(h.finalize().to_hex().to_string())
        })
        .map_err(|e| format!("{}: {e}", file_name(&sealed)))?;
    Ok(Recorded {
        sealed,
        samples: n,
        missing,
        malformed,
        missing_by,
        silent,
        id,
        ended,
    })
}

/// What `loadrec stop` found.
#[derive(Debug, Eq, PartialEq)]
pub enum Stopped {
    /// The running recorder sealed this fixture.
    ByRecorder(PathBuf),
    /// No recorder was running; this command sealed the interrupted recording, with its sample count.
    Here(PathBuf, u64),
    /// No recorder was running, and the interrupted recording held no whole sample: it was removed.
    Discarded,
}

/// `loadrec stop` in `dir` (`/private/load`): asks the recorder to stop and waits up to `wait` for it to seal; a
/// partial that does not grow for `stall` belongs to no recorder and is sealed here, or removed when it holds the
/// recorder's header and no whole sample (a partial with another header is left alone).
pub fn stop_in(dir: &Path, wait: Duration, stall: Duration) -> Result<Stopped, String> {
    let partial = match partials(dir)?.as_slice() {
        [] => return Err("nothing is being recorded (no *.load.partial in /private/load)".into()),
        [p] => p.clone(),
        many => {
            return Err(format!(
                "{} partial fixtures in /private/load; at most one can be recorded",
                many.len()
            ));
        }
    };
    let name = file_name(&partial);
    let sealed = dir.join(format!(
        "{}{SEALED}",
        name.strip_suffix(PARTIAL).unwrap_or(&name)
    ));
    let request = dir.join(STOP_REQUEST);
    File::create(&request).map_err(|e| format!("{STOP_REQUEST}: {e}"))?;
    let len = |p: &Path| std::fs::metadata(p).map(|m| m.len()).ok();
    let t0 = Instant::now();
    let (mut last_len, mut changed) = (len(&partial), t0);
    loop {
        if !partial.exists() {
            let _ = std::fs::remove_file(&request);
            return if sealed.exists() {
                Ok(Stopped::ByRecorder(sealed))
            } else {
                Err("the recorder stopped without a fixture (see its window)".into())
            };
        }
        let now = Instant::now();
        let l = len(&partial);
        if l != last_len {
            (last_len, changed) = (l, now);
        }
        if now.duration_since(changed) >= stall {
            let _ = std::fs::remove_file(&request);
            let (ours, len) = File::open(&partial)
                .and_then(|mut f| partial_state(&mut f))
                .map_err(|e| format!("{}: {e}", file_name(&partial)))?;
            if ours && len - (header().len() as u64) < ROW as u64 {
                std::fs::remove_file(&partial)
                    .map_err(|e| format!("{}: {e}", file_name(&partial)))?;
                return Ok(Stopped::Discarded);
            }
            let (p, n) = seal(&partial)?;
            return Ok(Stopped::Here(p, n));
        }
        if now.duration_since(t0) >= wait {
            let _ = std::fs::remove_file(&request);
            return Err(format!(
                "the recorder is still writing {} but did not stop within {wait:?}; stop it in its window",
                file_name(&partial)
            ));
        }
        std::thread::sleep(POLL.min(stall / 4).max(Duration::from_millis(1)));
    }
}

/// `loadrec stop`.
pub fn stop(repo: &Path, private_dir: &Path) -> Result<(), String> {
    match stop_in(&private_dir.join(LOAD_DIR), STOP_WAIT, STALL)? {
        Stopped::ByRecorder(p) => println!(
            "loadrec stop: the recorder sealed /private/load/{}",
            file_name(&p)
        ),
        Stopped::Here(p, n) => {
            println!(
                "loadrec stop: no recorder was running; sealed the interrupted recording /private/load/{} ({n} samples)",
                file_name(&p)
            );
            reindex(repo, private_dir);
        }
        Stopped::Discarded => println!(
            "loadrec stop: no recorder was running; the interrupted recording held no sample and was removed"
        ),
    }
    Ok(())
}

/// The nearest-rank percentile `q` of sorted values (as `docs/spec/measurement-protocol.md` §3.4 defines it).
fn percentile(sorted: &[f64], q: usize) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    let k = (q * sorted.len()).div_ceil(100).max(1);
    sorted[k - 1]
}

/// The summary `loadrec check` prints: the file's validity, its BLAKE3, and per counter the samples with a value and
/// their minimum, mean, p95 and maximum. Numbers and counter names only.
pub fn summary(name: &str, bytes: &[u8]) -> Result<String, String> {
    let f = decode(bytes).map_err(|e| format!("{name}: invalid: {e}"))?;
    let mut s = format!(
        "{name}: valid, BLAKE3 {}\n{C} counters, {} samples over {} at {} ms\n",
        blake3::hash(bytes).to_hex(),
        f.times.len(),
        hms(f.duration_ms()),
        f.interval_ms
    );
    s.push_str(&format!(
        "{:<46} {:>8} {:>16} {:>16} {:>16} {:>16}\n",
        "counter", "values", "min", "mean", "p95", "max"
    ));
    for (i, name) in f.names.iter().enumerate() {
        let mut v: Vec<f64> = f.column(i).filter(|x| !x.is_nan()).collect();
        v.sort_by(f64::total_cmp);
        let mean = if v.is_empty() {
            f64::NAN
        } else {
            v.iter().sum::<f64>() / v.len() as f64
        };
        s.push_str(&format!(
            "{name:<46} {:>8} {:>16.3} {mean:>16.3} {:>16.3} {:>16.3}\n",
            v.len(),
            v.first().copied().unwrap_or(f64::NAN),
            percentile(&v, 95),
            v.last().copied().unwrap_or(f64::NAN)
        ));
    }
    Ok(s)
}

/// `loadrec check <file>`: prints the summary; false for an invalid fixture.
pub fn check(path: &Path) -> Result<bool, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", file_name(path)))?;
    match summary(&file_name(path), &bytes) {
        Ok(s) => {
            print!("{s}");
            Ok(true)
        }
        Err(e) => {
            println!("{e}");
            Ok(false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testdir::TestDir;
    use proptest::prelude::*;

    /// One header line as typeperf prints it, with `machine` in every column and `banner` in the first.
    fn header_line(machine: &str, banner: &str) -> String {
        let mut s = format!("\"{banner}\"");
        for c in COUNTERS {
            s.push_str(&format!(",\"\\\\{machine}{c}\""));
        }
        s
    }

    /// One sample line with `stamp` as its timestamp; `None` values print as typeperf's blank field.
    fn sample_line(stamp: &str, values: &[Option<f64>]) -> String {
        let mut s = format!("\"{stamp}\"");
        for v in values {
            match v {
                Some(v) => s.push_str(&format!(",\"{v:.6}\"")),
                None => s.push_str(",\" \""),
            }
        }
        s
    }

    /// The typeperf output of a recording, as (line, arrival ms) pairs.
    fn transcript(
        machine: &str,
        banner: &str,
        stamps: &[String],
        rows: &[(u64, Vec<Option<f64>>)],
        closing: &[&str],
    ) -> Vec<(String, u64)> {
        let mut out = vec![(String::new(), 0), (header_line(machine, banner), 0)];
        for (i, (t, v)) in rows.iter().enumerate() {
            out.push((sample_line(&stamps[i % stamps.len()], v), *t));
        }
        for c in closing {
            out.push(((*c).to_string(), rows.last().map_or(0, |r| r.0)));
        }
        out
    }

    /// Records a transcript into sealed fixture bytes.
    fn record(lines: &[(String, u64)]) -> Vec<u8> {
        let mut rec = Recorder::new(Vec::new()).unwrap();
        for (l, t) in lines {
            rec.feed(l, *t).unwrap();
        }
        let mut bytes = rec.finish().unwrap();
        let sum = xxh3_64(&bytes);
        bytes.extend_from_slice(&sum.to_le_bytes());
        bytes
    }

    fn row(seed: u64) -> Vec<Option<f64>> {
        (0..C as u64)
            .map(|i| Some(((seed * 31 + i * 7) % 1_000) as f64 * 1.5))
            .collect()
    }

    #[test]
    fn header_layout() {
        let h = header();
        assert_eq!(&h[..8], &MAGIC);
        assert_eq!(u16::from_le_bytes([h[8], h[9]]), 1);
        assert_eq!(u16::from_le_bytes([h[10], h[11]]), 18);
        assert_eq!(u32::from_le_bytes([h[12], h[13], h[14], h[15]]), 1_000);
        assert_eq!(h[16] as usize, COUNTERS[0].len());
        assert_eq!(&h[17..17 + COUNTERS[0].len()], COUNTERS[0].as_bytes());
        assert!(MAGIC.iter().all(|b| !(0x20..=0x7e).contains(b)));
        assert!(COUNTERS.iter().all(|c| c.len() < 256
            && c.bytes().all(|b| (0x20..=0x7e).contains(&b))
            && c.starts_with('\\')));
    }

    #[test]
    fn csv_and_counter_paths() {
        assert_eq!(csv_fields("\"a\",\"b c\""), Some(vec!["a", "b c"]));
        assert_eq!(csv_fields("\"a\""), Some(vec!["a"]));
        assert_eq!(csv_fields("Exiting, please wait..."), None);
        assert_eq!(csv_fields("\""), None);
        assert_eq!(csv_fields("\"a\"b\""), None);
        assert_eq!(
            counter_path(r"\\HOST-1\Memory\Available Bytes"),
            r"\Memory\Available Bytes"
        );
        assert_eq!(counter_path(r"\Memory\Pages/sec"), r"\Memory\Pages/sec");
        assert_eq!(counter_path(r"\\HOST-ONLY"), "");
    }

    #[test]
    fn header_mapping() {
        let line = header_line("BOX", "(PDH-CSV 4.0)");
        let f = csv_fields(&line).unwrap();
        let all: ColumnMap = std::array::from_fn(|i| Some(i + 1));
        assert_eq!(map_header(&f), Ok(all));
        let lower = line.to_ascii_lowercase();
        assert_eq!(map_header(&csv_fields(&lower).unwrap()), Ok(all));
        // Optional counters typeperf left out are absent; the order of the columns does not matter.
        let mut some = vec![f[0], f[18], f[1], f[4], f[5], f[6], f[7]];
        let m = map_header(&some).unwrap();
        assert_eq!((m[17], m[0], m[3], m[1]), (Some(1), Some(2), Some(3), None));
        some.push(f[2]);
        assert_eq!(map_header(&some).unwrap()[1], Some(7));
        some.push(f[2]);
        assert!(map_header(&some).unwrap_err().contains("twice"));
        let e = map_header(&f[..4]).unwrap_err();
        assert!(
            e.contains("Disk Write Bytes/sec") && e.contains("needs"),
            "{e}"
        );
        let swapped = line.replace("Disk Reads/sec", "Disk Transfers/sec");
        let e = map_header(&csv_fields(&swapped).unwrap()).unwrap_err();
        assert!(e.contains("column 6") && !e.contains("BOX"), "{e}");
        assert!(REQUIRED.iter().all(|&i| i < C));
    }

    #[test]
    fn absent_counters_are_missing_in_every_sample() {
        let line = header_line("BOX", "(PDH-CSV 4.0)");
        let f = csv_fields(&line).unwrap();
        let mut rec = Recorder::new(Vec::new()).unwrap();
        // typeperf left out the two Process IO counters (fields 15 and 16 of the full header).
        let short: Vec<String> = f
            .iter()
            .enumerate()
            .filter(|(k, _)| *k != 15 && *k != 16)
            .map(|(_, c)| format!("\"{c}\""))
            .collect();
        assert_eq!(rec.feed(&short.join(","), 0), Ok(Fed::Header));
        assert_eq!(rec.absent(), vec![COUNTERS[14], COUNTERS[15]]);
        let values: Vec<Option<f64>> = (0..16).map(|i| Some(f64::from(i))).collect();
        assert_eq!(rec.feed(&sample_line("s", &values), 5), Ok(Fed::Sample));
        assert_eq!(rec.feed(&sample_line("s", &row(1)), 6), Ok(Fed::Malformed));
        let mut bytes = rec.finish().unwrap();
        let sum = xxh3_64(&bytes);
        bytes.extend_from_slice(&sum.to_le_bytes());
        let fx = decode(&bytes).unwrap();
        assert_eq!(fx.values[13], 13.0);
        assert!(fx.values[14].is_nan() && fx.values[15].is_nan());
        assert_eq!((fx.values[16], fx.values[17]), (14.0, 15.0));
    }

    #[test]
    fn values() {
        assert_eq!(value("12.345678"), 12.345678);
        assert_eq!(value(" 7 "), 7.0);
        assert_eq!(value("2804023296.000000"), 2_804_023_296.0);
        for bad in [
            "", " ", "-1", "1e5", "1.2.3", ".5", "5.", "NaN", "inf", "1,5", "x",
        ] {
            assert!(value(bad).is_nan(), "{bad:?}");
        }
    }

    #[test]
    fn records_samples_and_drops_every_other_string() {
        let rows: Vec<(u64, Vec<Option<f64>>)> = vec![
            (1_000, row(1)),
            (2_100, {
                let mut r = row(2);
                r[3] = None;
                r
            }),
            (2_100, row(3)),
            (4_000, row(4)),
        ];
        let stamps = vec!["10/04/2026 14:48:04.593".to_string()];
        let lines = transcript(
            "SECRET-HOST-42",
            "(PDH-CSV 4.0) (Pacific Daylight Time)(420)",
            &stamps,
            &rows,
            &[
                "Exiting, please wait...                         ",
                "The command completed successfully.",
            ],
        );
        let bytes = record(&lines);
        let f = decode(&bytes).unwrap();
        assert_eq!(f.names, COUNTERS.map(str::to_string).to_vec());
        // Times are arrival times since the first sample, strictly increasing.
        assert_eq!(f.times, vec![0, 1_100, 1_101, 3_000]);
        assert_eq!(f.column(0).next(), Some(row(1)[0].unwrap()));
        assert!(f.values[C + 3].is_nan());
        assert_eq!(f.duration_ms(), 4_000);
        for needle in [
            "SECRET",
            "HOST-42",
            "PDH",
            "Pacific",
            "10/04",
            "14:48",
            "Exiting",
            "completed",
        ] {
            assert!(
                !bytes.windows(needle.len()).any(|w| w == needle.as_bytes()),
                "{needle} reached the fixture"
            );
        }
    }

    #[test]
    fn only_the_counter_table_is_text() {
        // Every byte of a fixture is the magic, a numeric header field, the name table, a numeric sample field or the
        // checksum: decode accounts for all of them and refuses a trailing byte, and the name table is COUNTERS.
        let rows: Vec<(u64, Vec<Option<f64>>)> = (0..5).map(|i| (i * 1_000, row(i))).collect();
        let bytes = record(&transcript(
            "M",
            "(PDH-CSV 4.0)",
            &["s".to_string()],
            &rows,
            &[],
        ));
        let f = decode(&bytes).unwrap();
        let table: usize = COUNTERS.iter().map(|c| 1 + c.len()).sum();
        assert_eq!(bytes.len(), 16 + table + 5 * ROW + 8);
        assert_eq!(f.names.len(), C);
        let mut longer = bytes.clone();
        longer.insert(bytes.len() - 8, 0);
        assert!(decode(&longer).is_err());
    }

    #[test]
    fn messages_before_the_header_are_kept_out_of_the_file() {
        let mut rec = Recorder::new(Vec::new()).unwrap();
        assert_eq!(
            rec.feed("Error: No valid counters.\r\n", 0),
            Ok(Fed::Message)
        );
        assert_eq!(rec.feed("\"not\",\"a header\"", 0), Ok(Fed::Message));
        assert_eq!(rec.feed("   ", 0), Ok(Fed::Blank));
        assert!(!rec.header_seen());
        assert_eq!(rec.messages.len(), 2);
        assert_eq!(rec.finish().unwrap(), header());
    }

    #[test]
    fn a_wrong_header_or_sample_width_is_an_error() {
        let mut rec = Recorder::new(Vec::new()).unwrap();
        let bad = header_line("H", "(PDH-CSV 4.0)").replace("Handle Count", "Handles");
        assert!(rec.feed(&bad, 0).is_err());
        let mut rec = Recorder::new(Vec::new()).unwrap();
        rec.feed(&header_line("H", "(PDH-CSV 4.0)"), 0).unwrap();
        assert_eq!(rec.feed("\"t\",\"1\"", 0), Ok(Fed::Malformed));
        assert_eq!(rec.feed(&sample_line("t", &row(1)), 0), Ok(Fed::Sample));
        assert_eq!((rec.samples, rec.malformed), (1, 1));
    }

    proptest! {
        /// The fixture is a function of the numbers and arrival times alone: whatever machine name, banner, time zone,
        /// timestamps and closing messages typeperf prints, the bytes are the same ([MP §9.2]: no string but the
        /// counter names).
        #[test]
        fn fixture_is_independent_of_typeperf_strings(
            machines in proptest::collection::vec("[A-Za-z0-9-]{1,15}", 2),
            banners in proptest::collection::vec("\\(PDH-CSV 4\\.0\\)[ -~&&[^\"]]{0,40}", 2),
            stamps in proptest::collection::vec(proptest::collection::vec("[0-9/:. ]{1,24}", 1..4), 2),
            closing in proptest::collection::vec("[ -~&&[^\"]]{0,40}", 0..3),
            values in proptest::collection::vec(proptest::collection::vec(proptest::option::of(0.0f64..1e12), C), 1..20),
            gaps in proptest::collection::vec(0u64..3_000, 20),
        ) {
            let mut t = 0;
            let rows: Vec<(u64, Vec<Option<f64>>)> = values
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    t += gaps[i];
                    (t, v.clone())
                })
                .collect();
            let closing: Vec<&str> = closing.iter().map(String::as_str).collect();
            let a = record(&transcript(&machines[0], &banners[0], &stamps[0], &rows, &closing));
            let b = record(&transcript(&machines[1], &banners[1], &stamps[1], &rows, &[]));
            prop_assert_eq!(&a, &b);
            let f = decode(&a).unwrap();
            prop_assert_eq!(f.times.len(), rows.len());
            for (i, (_, v)) in rows.iter().enumerate() {
                for (j, x) in v.iter().enumerate() {
                    let got = f.values[i * C + j];
                    match x {
                        // typeperf prints six decimals; the fixture holds what it printed.
                        Some(x) => prop_assert_eq!(got, format!("{x:.6}").parse::<f64>().unwrap()),
                        None => prop_assert!(got.is_nan()),
                    }
                }
            }
        }
    }

    #[test]
    fn decode_refuses_damage() {
        let rows: Vec<(u64, Vec<Option<f64>>)> = (0..3).map(|i| (i * 1_000, row(i))).collect();
        let good = record(&transcript("M", "(PDH-CSV 4.0)", &["s".into()], &rows, &[]));
        assert!(decode(&good).is_ok());
        let reseal = |mut b: Vec<u8>| {
            b.truncate(b.len() - 8);
            let s = xxh3_64(&b);
            b.extend_from_slice(&s.to_le_bytes());
            b
        };
        let mut bad = good.clone();
        bad[0] = b'M';
        assert!(decode(&reseal(bad)).unwrap_err().contains("magic"));
        let mut bad = good.clone();
        bad[8] = 2;
        assert!(decode(&reseal(bad)).unwrap_err().contains("version"));
        let mut bad = good.clone();
        bad[18] = b'p';
        assert!(decode(&reseal(bad)).unwrap_err().contains("counter 1"));
        let mut bad = good.clone();
        bad[12..16].copy_from_slice(&0u32.to_le_bytes());
        assert!(decode(&reseal(bad)).unwrap_err().contains("interval"));
        let h = header().len();
        let mut bad = good.clone();
        bad[h + ROW..h + ROW + 8].copy_from_slice(&0u64.to_le_bytes());
        assert!(
            decode(&reseal(bad))
                .unwrap_err()
                .contains("does not follow")
        );
        let mut bad = good.clone();
        bad[h + 8..h + 16].copy_from_slice(&(-1.0f64).to_bits().to_le_bytes());
        assert!(
            decode(&reseal(bad))
                .unwrap_err()
                .contains("sample 1, counter 1")
        );
        let mut bad = good.clone();
        bad[h + 8..h + 16].copy_from_slice(&f64::INFINITY.to_bits().to_le_bytes());
        assert!(decode(&reseal(bad)).is_err());
        let mut bad = good.clone();
        bad[h] = 1;
        assert!(decode(&reseal(bad)).unwrap_err().contains("first sample"));
        let mut bad = good.clone();
        let last = bad.len() - 1;
        bad[last] ^= 1;
        assert!(decode(&bad).unwrap_err().contains("checksum"));
        let header_only = reseal([header(), vec![0; 8]].concat());
        assert!(decode(&header_only).unwrap_err().contains("whole number"));
        assert!(decode(&good[..4]).is_err());
    }

    fn partial_with(dir: &TestDir, name: &str, samples: usize, extra: usize) -> PathBuf {
        let rows: Vec<(u64, Vec<Option<f64>>)> =
            (0..samples as u64).map(|i| (i * 1_000, row(i))).collect();
        let mut rec = Recorder::new(Vec::new()).unwrap();
        for (l, t) in transcript("M", "(PDH-CSV 4.0)", &["s".into()], &rows, &[]) {
            rec.feed(&l, t).unwrap();
        }
        let mut bytes = rec.finish().unwrap();
        bytes.extend(std::iter::repeat_n(0xAB, extra));
        dir.write(name, bytes)
    }

    #[test]
    fn seal_drops_a_torn_sample_and_is_idempotent() {
        let d = TestDir::new("loadrec-seal");
        let p = partial_with(&d, "20261004T100000Z.load.partial", 3, 17);
        let (sealed, n) = seal(&p).unwrap();
        assert_eq!(n, 3);
        assert_eq!(file_name(&sealed), "20261004T100000Z.load");
        assert!(!p.exists());
        let first = std::fs::read(&sealed).unwrap();
        assert_eq!(decode(&first).unwrap().times, vec![0, 1_000, 2_000]);
        // A crash after the write and before the rename leaves a sealed body under the partial name.
        std::fs::rename(&sealed, &p).unwrap();
        let (sealed, n) = seal(&p).unwrap();
        assert_eq!((n, std::fs::read(&sealed).unwrap()), (3, first));
        // The sealed name is never overwritten.
        let p2 = partial_with(&d, "20261004T100000Z.load.partial", 1, 0);
        assert!(seal(&p2).unwrap_err().contains("already exists"));
        let empty = partial_with(&d, "x.load.partial", 0, 100);
        assert!(seal(&empty).unwrap_err().contains("no whole sample"));
        let foreign = d.write("y.load.partial", b"not a fixture");
        assert!(seal(&foreign).unwrap_err().contains("bad header"));
        assert!(seal(&d.write("z.bin", b"")).is_err());
    }

    #[test]
    fn stop_seals_what_a_dead_recorder_left() {
        let d = TestDir::new("loadrec-stop");
        assert!(
            stop_in(d.path(), Duration::from_secs(1), Duration::from_millis(50))
                .unwrap_err()
                .contains("nothing is being recorded")
        );
        let p = partial_with(&d, "20261004T110000Z.load.partial", 2, 5);
        let got = stop_in(d.path(), Duration::from_secs(5), Duration::from_millis(100)).unwrap();
        let sealed = d.path().join("20261004T110000Z.load");
        assert_eq!(got, Stopped::Here(sealed.clone(), 2));
        assert!(!p.exists() && !d.path().join(STOP_REQUEST).exists());
        assert!(decode(&std::fs::read(&sealed).unwrap()).is_ok());
        // An interrupted recording without a whole sample is removed; a foreign file is left alone.
        let empty = partial_with(&d, "20261004T111000Z.load.partial", 0, 40);
        assert_eq!(
            stop_in(d.path(), Duration::from_secs(5), Duration::from_millis(100)),
            Ok(Stopped::Discarded)
        );
        assert!(!empty.exists());
        let foreign = d.write("x.load.partial", b"not a fixture");
        assert!(
            stop_in(d.path(), Duration::from_secs(5), Duration::from_millis(100))
                .unwrap_err()
                .contains("bad header")
        );
        assert!(foreign.exists());
        partial_with(&d, "a.load.partial", 1, 0);
        assert!(
            stop_in(d.path(), Duration::from_secs(1), Duration::from_millis(50))
                .unwrap_err()
                .contains("2 partial fixtures")
        );
    }

    #[test]
    fn stop_waits_for_a_live_recorder() {
        // A recorder thread runs the real loop on a synthetic typeperf stream and seals when the request appears.
        let d = TestDir::new("loadrec-live");
        let dir = d.path().to_path_buf();
        let partial = dir.join("20261004T120000Z.load.partial");
        let file = File::create(&partial).unwrap();
        let mut rec = Recorder::new(BufWriter::new(file)).unwrap();
        let (tx, rx) = mpsc::channel();
        let t0 = Instant::now();
        tx.send(Event::Line(header_line("M", "(PDH-CSV 4.0)"), t0))
            .unwrap();
        let feeder = std::thread::spawn(move || {
            for i in 0..400u64 {
                let line = sample_line("s", &row(i));
                if tx
                    .send(Event::Line(line, t0 + Duration::from_millis(10 * i)))
                    .is_err()
                {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        });
        let looper = {
            let dir = dir.clone();
            let partial = partial.clone();
            std::thread::spawn(move || {
                let ended = record_loop(&rx, &mut rec, &dir, t0, Duration::from_secs(60)).unwrap();
                drop(rx);
                rec.finish().unwrap();
                seal(&partial).unwrap();
                let _ = std::fs::remove_file(dir.join(STOP_REQUEST));
                ended
            })
        };
        std::thread::sleep(Duration::from_millis(300));
        let got = stop_in(&dir, Duration::from_secs(20), Duration::from_secs(10)).unwrap();
        assert_eq!(got, Stopped::ByRecorder(dir.join("20261004T120000Z.load")));
        assert_eq!(looper.join().unwrap(), Reason::StopRequest);
        feeder.join().unwrap();
        let f = decode(&std::fs::read(dir.join("20261004T120000Z.load")).unwrap()).unwrap();
        assert!(!f.times.is_empty());
    }

    #[test]
    fn the_loop_ends_on_the_maximum_or_the_end_of_output() {
        let d = TestDir::new("loadrec-loop");
        let (tx, rx) = mpsc::channel::<Event>();
        let mut rec = Recorder::new(Vec::new()).unwrap();
        let t0 = Instant::now();
        assert_eq!(
            record_loop(&rx, &mut rec, d.path(), t0, Duration::ZERO),
            Ok(Reason::MaxDuration)
        );
        tx.send(Event::End(None)).unwrap();
        assert_eq!(
            record_loop(&rx, &mut rec, d.path(), t0, Duration::from_secs(60)),
            Ok(Reason::OutputEnded)
        );
        tx.send(Event::End(Some("broken pipe".into()))).unwrap();
        assert!(record_loop(&rx, &mut rec, d.path(), t0, Duration::from_secs(60)).is_err());
        tx.send(Event::Enter).unwrap();
        assert_eq!(
            record_loop(&rx, &mut rec, d.path(), t0, Duration::from_secs(60)),
            Ok(Reason::Keyboard)
        );
        drop(tx);
        assert_eq!(
            record_loop(&rx, &mut rec, d.path(), t0, Duration::from_secs(60)),
            Ok(Reason::OutputEnded)
        );
    }

    #[test]
    fn summary_and_percentile() {
        assert_eq!(percentile(&[1.0, 2.0, 3.0, 4.0], 95), 4.0);
        assert_eq!(percentile(&[1.0, 2.0, 3.0, 4.0], 50), 2.0);
        assert!(percentile(&[], 50).is_nan());
        let rows: Vec<(u64, Vec<Option<f64>>)> = (0..4).map(|i| (i * 1_000, row(i))).collect();
        let bytes = record(&transcript("M", "(PDH-CSV 4.0)", &["s".into()], &rows, &[]));
        let s = summary("f.load", &bytes).unwrap();
        assert!(s.starts_with("f.load: valid, BLAKE3 "));
        assert!(s.contains("4 samples over 0:00:04"));
        assert!(s.contains(r"\Process(_Total)\Handle Count"));
        assert!(
            summary("g.load", &bytes[..bytes.len() - 1])
                .unwrap_err()
                .contains("invalid")
        );
    }

    #[test]
    fn utc_stamps() {
        assert_eq!(compact_utc(UNIX_EPOCH), "19700101T000000Z");
        assert_eq!(
            compact_utc(UNIX_EPOCH + Duration::from_secs(1_791_106_225)),
            "20261004T093025Z"
        );
        assert_eq!(hms(3_725_000), "1:02:05");
    }

    #[test]
    fn the_private_manifest_stays_current_while_recording() {
        // [MP §9.3]: the partial and the stop request are left out of the manifest, so the pre-commit guard keeps
        // accepting commits for the whole recording; the sealed fixture is listed once loadrec rebuilds it.
        let d = TestDir::new("loadrec-manifest");
        d.write(
            "notes/a.txt",
            "synthetic words for the private index of this unit test",
        );
        d.write(
            "other/x.load.partial",
            b"\0outside load/ it is an ordinary private file",
        );
        let load = d.path().join(LOAD_DIR);
        std::fs::create_dir_all(&load).unwrap();
        private::index(d.path(), &[]).unwrap();
        assert!(private::load_current(d.path()).unwrap().is_some());
        let partial = load.join("20261004T130000Z.load.partial");
        let mut rec = Recorder::new(File::create(&partial).unwrap()).unwrap();
        assert!(private::load_current(d.path()).unwrap().is_some());
        rec.feed(&header_line("M", "(PDH-CSV 4.0)"), 0).unwrap();
        for i in 0..3 {
            rec.feed(&sample_line("s", &row(i)), i * 1_000).unwrap();
            assert!(
                private::load_current(d.path()).unwrap().is_some(),
                "stale after sample {i}"
            );
        }
        File::create(load.join(STOP_REQUEST)).unwrap();
        assert!(private::load_current(d.path()).unwrap().is_some());
        drop(rec.finish().unwrap());
        std::fs::remove_file(load.join(STOP_REQUEST)).unwrap();
        seal(&partial).unwrap();
        assert!(
            private::load_current(d.path())
                .unwrap_err()
                .contains("stale")
        );
        private::index(d.path(), &[]).unwrap();
        let (m, _) = private::load_current(d.path()).unwrap().unwrap();
        let paths: Vec<&str> = m.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(
            paths,
            vec![
                "load/20261004T130000Z.load",
                "notes/a.txt",
                "other/x.load.partial"
            ]
        );
        // Any other file under load/ is an ordinary private file.
        d.write("load/notes.txt", "x");
        assert!(private::load_current(d.path()).is_err());
    }

    #[test]
    fn seal_streams_long_recordings() {
        // More than one 64 KiB read of the checksum stream, and a torn sample cut by set_len.
        let d = TestDir::new("loadrec-long");
        let p = partial_with(&d, "20261004T140000Z.load.partial", 1_000, ROW - 1);
        let before = std::fs::metadata(&p).unwrap().len();
        let (sealed, n) = seal(&p).unwrap();
        assert_eq!(n, 1_000);
        let bytes = std::fs::read(&sealed).unwrap();
        assert_eq!(bytes.len() as u64, before - (ROW as u64 - 1) + 8);
        let f = decode(&bytes).unwrap();
        assert_eq!(f.times.len(), 1_000);
        assert_eq!(f.times[999], 999_000);
    }

    #[test]
    fn progress_counts_missing_values_per_counter() {
        let mut rec = Recorder::new(Vec::new()).unwrap();
        rec.feed(&header_line("M", "(PDH-CSV 4.0)"), 0).unwrap();
        assert!(rec.silent_required().is_empty());
        // Disk reads/sec (#5, required) never has a value; processor time (#0) and queue length (#7) miss once.
        let mut base = row(1);
        base[5] = None;
        for i in 0..WARN_AFTER {
            let mut r = base.clone();
            if i == 3 {
                (r[0], r[7]) = (None, None);
            }
            rec.feed(&sample_line("s", &r), i * 1_000).unwrap();
            if i + 1 < WARN_AFTER {
                assert_eq!(rec.progress().len(), 1, "a warning before sample {i}");
            }
        }
        assert_eq!(rec.missing_by[5], WARN_AFTER);
        assert_eq!((rec.missing_by[0], rec.missing_by[7]), (1, 1));
        assert_eq!(rec.silent_required(), vec![COUNTERS[5]]);
        assert_eq!(
            rec.missing_list(true),
            format!("{} 1, {} {WARN_AFTER}", COUNTERS[0], COUNTERS[5])
        );
        assert!(
            rec.missing_list(false)
                .contains(&format!("{} 1", COUNTERS[7]))
        );
        let lines = rec.progress();
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert!(
            lines[0].starts_with(
                "loadrec: 60 samples, 0:00:59 recorded; missing values of the required counters: "
            ),
            "{lines:?}"
        );
        assert!(
            lines[1].contains("WARNING") && lines[1].contains(COUNTERS[5]),
            "{lines:?}"
        );
        // A value of the counter ends the warning.
        rec.feed(&sample_line("s", &row(2)), 60_000).unwrap();
        assert!(rec.silent_required().is_empty());
        assert_eq!(rec.progress().len(), 1);
        let clean = Recorder::new(Vec::new()).unwrap();
        assert_eq!(clean.missing_list(false), "none");
    }

    /// A writer whose flush fails once `fail` is set: the last flush of a recording.
    struct FlakyFlush {
        f: File,
        fail: std::rc::Rc<std::cell::Cell<bool>>,
    }

    impl Write for FlakyFlush {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.f.write(b)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            if self.fail.get() {
                Err(std::io::Error::other("the disk is full"))
            } else {
                self.f.flush()
            }
        }
    }

    #[test]
    fn finishing_a_recording() {
        let d = TestDir::new("loadrec-finish");
        let dir = d.path().to_path_buf();
        let fail = std::rc::Rc::new(std::cell::Cell::new(false));
        let start = |name: &str, samples: u64, header: bool| {
            let partial = dir.join(name);
            let mut rec = Recorder::new(FlakyFlush {
                f: File::create(&partial).unwrap(),
                fail: fail.clone(),
            })
            .unwrap();
            rec.feed("Error: something typeperf said\r\n", 0).unwrap();
            if header {
                rec.feed(&header_line("M", "(PDH-CSV 4.0)"), 0).unwrap();
            }
            for i in 0..samples {
                rec.feed(&sample_line("s", &row(i)), i * 1_000).unwrap();
            }
            File::create(dir.join(STOP_REQUEST)).unwrap();
            (rec, partial)
        };
        let gone = |p: &Path| !p.exists() && !dir.join(STOP_REQUEST).exists();
        // No header: no file, and typeperf's messages say why.
        let (rec, p) = start("a.load.partial", 0, false);
        let e = finish_recording(Ok(Reason::OutputEnded), rec, &p, &dir).unwrap_err();
        assert!(
            e.contains("no header") && e.contains("something typeperf said"),
            "{e}"
        );
        assert!(gone(&p));
        // A header and no sample.
        let (rec, p) = start("b.load.partial", 0, true);
        let e = finish_recording(Ok(Reason::Keyboard), rec, &p, &dir).unwrap_err();
        assert_eq!(e, "no sample was recorded");
        assert!(gone(&p));
        // An error before any sample is the error.
        let (rec, p) = start("c.load.partial", 0, true);
        let e = finish_recording(
            Err("reading typeperf's output: broken pipe".into()),
            rec,
            &p,
            &dir,
        )
        .unwrap_err();
        assert!(e.contains("broken pipe"), "{e}");
        assert!(gone(&p));
        // Samples and a clean stop: sealed.
        let (rec, p) = start("20261004T150000Z.load.partial", 3, true);
        let r = finish_recording(Ok(Reason::StopRequest), rec, &p, &dir).unwrap();
        assert!(gone(&p));
        assert_eq!(r.ended, Ok(Reason::StopRequest));
        assert_eq!((r.samples, r.missing, r.malformed), (3, 0, 0));
        assert_eq!(r.missing_by, "none");
        assert!(r.silent.is_empty());
        let bytes = std::fs::read(&r.sealed).unwrap();
        assert_eq!(r.id, blake3::hash(&bytes).to_hex().to_string());
        assert_eq!(decode(&bytes).unwrap().times, vec![0, 1_000, 2_000]);
        // Samples and an error: sealed, and the error is kept for the report.
        let (rec, p) = start("20261004T151000Z.load.partial", 2, true);
        let r = finish_recording(Err("writing a sample: gone".into()), rec, &p, &dir).unwrap();
        assert_eq!(r.ended, Err("writing a sample: gone".into()));
        assert_eq!(
            decode(&std::fs::read(&r.sealed).unwrap())
                .unwrap()
                .times
                .len(),
            2
        );
        // The last flush fails: what reached the file is sealed, and the stop is an error.
        let (rec, p) = start("20261004T152000Z.load.partial", 2, true);
        fail.set(true);
        let r = finish_recording(Ok(Reason::MaxDuration), rec, &p, &dir).unwrap();
        assert!(r.ended.unwrap_err().contains("the disk is full"));
        assert_eq!(r.samples, 2);
        assert!(gone(&p));
    }

    /// WP-51's interop fixture, hand-written from [MP §9.2] and decoded by moirai-probes' tests as well.
    fn interop() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../crates/moirai-probes/testdata/load/three")
    }

    #[test]
    fn the_hand_written_fixture_decodes_and_is_what_the_recorder_writes() {
        let base = interop();
        let bytes = std::fs::read(base.with_extension("bin")).unwrap();
        // The committed .bin is its .hex, assembled.
        let a = crate::hex::assemble_file(&base.with_extension("hex")).unwrap();
        assert!(a.mismatches().is_empty(), "{:?}", a.mismatches());
        assert_eq!(a.bytes, bytes);
        let f = decode(&bytes).unwrap();
        assert_eq!(
            (f.interval_ms, f.times.clone()),
            (1_000, vec![0, 1_000, 2_003])
        );
        assert_eq!(f.names, COUNTERS.map(str::to_string).to_vec());
        let cpu: Vec<String> = f.column(0).map(|v| v.to_string()).collect();
        assert_eq!(cpu, ["37.5", "NaN", "90"]);
        assert_eq!(f.values[C + 5], 0.5);
        assert!(f.column(14).chain(f.column(15)).all(f64::is_nan));
        assert_eq!(f.values[2 * C + 17], 60_020.0);
        // typeperf's transcript of the same samples, with the Process IO counters left out of its header, records to
        // the same bytes.
        let line = header_line("BOX", "(PDH-CSV 4.0) (Zone)(0)");
        let fields = csv_fields(&line).unwrap();
        let head: Vec<String> = fields
            .iter()
            .enumerate()
            .filter(|(k, _)| *k != 15 && *k != 16)
            .map(|(_, c)| format!("\"{c}\""))
            .collect();
        let mut rec = Recorder::new(Vec::new()).unwrap();
        assert_eq!(rec.feed(&head.join(","), 0), Ok(Fed::Header));
        for (row, arrival) in [5_000u64, 6_000, 7_003].into_iter().enumerate() {
            let values: Vec<Option<f64>> = (0..C)
                .filter(|&i| i != 14 && i != 15)
                .map(|i| Some(f.values[row * C + i]).filter(|v| !v.is_nan()))
                .collect();
            assert_eq!(
                rec.feed(&sample_line("10/04/2026 10:00:00.000", &values), arrival),
                Ok(Fed::Sample)
            );
        }
        let mut written = rec.finish().unwrap();
        let sum = xxh3_64(&written);
        written.extend_from_slice(&sum.to_le_bytes());
        assert_eq!(written, bytes);
    }
}
