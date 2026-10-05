//! The live sampler ([MP §9.4]): the four system-wide counters the generator controls and validates, read by
//! `typeperf` once a second while it replays. The machine prefix and the timestamp typeperf prints are dropped as the
//! recorder drops them; a sample is stamped with its arrival time.

use super::fixture::{AVAILABLE, COUNTERS, CPU, READ_BYTES, WRITE_BYTES};
use crate::host::SystemCommand;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// The sampled counters, in the order of [`Sample::values`].
pub const SAMPLED: [&str; 4] = [
    COUNTERS[CPU],
    COUNTERS[READ_BYTES],
    COUNTERS[WRITE_BYTES],
    COUNTERS[AVAILABLE],
];
/// Index of processor time in [`Sample::values`].
pub const S_CPU: usize = 0;
/// Index of disk read bytes per second.
pub const S_READ: usize = 1;
/// Index of disk write bytes per second.
pub const S_WRITE: usize = 2;
/// Index of available physical bytes.
pub const S_AVAILABLE: usize = 3;

/// The sampling interval, in milliseconds: typeperf's `-si 1`. The replay log records it as `sample_ms`, and the
/// coverage rule counts its gaps in it ([MP §9.5]).
pub const SAMPLE_MS: u64 = 1_000;

/// typeperf's arguments: the sampled counters, one sample a second.
const ARGS: [&str; 6] = [SAMPLED[0], SAMPLED[1], SAMPLED[2], SAMPLED[3], "-si", "1"];

/// `%SystemRoot%\System32\typeperf.exe` with the sampled counters ([MP §2.2]'s rule for system programs).
pub const TYPEPERF: SystemCommand = SystemCommand {
    path: &["System32", "typeperf.exe"],
    args: &ARGS,
};

/// One sample of the system-wide counters ([MP §9.4]).
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Sample {
    /// Arrival on the monotonic clock, in milliseconds since the sampler started.
    pub mono_ms: u64,
    /// Arrival on the wall clock, in milliseconds since 1970 (UTC).
    pub unix_ms: u64,
    /// The values of [`SAMPLED`]; `None` where typeperf had no value.
    pub values: [Option<f64>; 4],
}

/// A source of samples, about one a second.
pub trait Sampler {
    /// Waits for the next sample; `None` when the source has ended.
    fn next(&mut self) -> Result<Option<Sample>, String>;

    /// The sample lines skipped so far because their width was not their header's ([`Parser`]).
    fn skipped(&self) -> u64 {
        0
    }
}

/// The fields of one line of typeperf's CSV output; `None` for any other line.
fn csv_fields(line: &str) -> Option<Vec<&str>> {
    let inner = line.strip_prefix('"')?.strip_suffix('"')?;
    let fields: Vec<&str> = inner.split("\",\"").collect();
    (!fields.iter().any(|f| f.contains('"'))).then_some(fields)
}

/// A header column's counter path without the `\\<machine>` prefix.
fn counter_path(column: &str) -> &str {
    match column.strip_prefix(r"\\") {
        Some(rest) => rest.find('\\').map_or("", |i| &rest[i..]),
        None => column,
    }
}

/// A non-negative decimal number as typeperf prints it; `None` for a blank field or anything else.
fn value(field: &str) -> Option<f64> {
    let s = field.trim();
    let (int, frac) = s.split_once('.').unwrap_or((s, "0"));
    let digits = |p: &str| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit());
    if digits(int) && digits(frac) {
        s.parse::<f64>().ok().filter(|v| v.is_finite())
    } else {
        None
    }
}

/// Turns typeperf's output lines into sample values: the header maps the columns (every sampled counter must be
/// there, in any order); later lines are samples; other lines are typeperf's messages and are skipped. A sample line
/// whose field count is not the header's is skipped and counted, as the recorder skips one ([MP §9.3], [MP §9.4]): the
/// sample it would have been is missing, and the coverage rule judges the gap ([MP §9.5]).
#[derive(Debug, Default)]
pub struct Parser {
    map: Option<[usize; 4]>,
    width: usize,
    /// Sample lines skipped for their width.
    pub skipped: u64,
}

impl Parser {
    /// One line; `Some` values for a sample line.
    pub fn line(&mut self, line: &str) -> Result<Option<[Option<f64>; 4]>, String> {
        let line = line.trim_end_matches(['\r', '\n']);
        let Some(fields) = csv_fields(line) else {
            return Ok(None);
        };
        match self.map {
            None => {
                if !fields[0].starts_with("(PDH-CSV") {
                    return Ok(None);
                }
                let mut map = [0; 4];
                for (slot, want) in map.iter_mut().zip(SAMPLED) {
                    *slot = fields
                        .iter()
                        .skip(1)
                        .position(|f| counter_path(f).eq_ignore_ascii_case(want))
                        .map(|k| k + 1)
                        .ok_or_else(|| format!("typeperf cannot read {want}"))?;
                }
                self.map = Some(map);
                self.width = fields.len();
                Ok(None)
            }
            Some(map) => {
                if fields.len() != self.width {
                    self.skipped += 1;
                    return Ok(None);
                }
                Ok(Some(map.map(|k| value(fields[k]))))
            }
        }
    }
}

/// An event of the reader thread.
enum Event {
    Line(String, Instant, SystemTime),
    End(Option<String>),
}

/// How long the sampler waits for typeperf's first sample: it enumerates its counters first, which takes long on a
/// loaded machine.
pub const FIRST_SAMPLE_WAIT: Duration = Duration::from_secs(120);
/// How long the sampler waits for any later sample: typeperf stalled this long ends the replay with an error (exit 3),
/// since every run riding on it would fail rule (a) anyway ([MP §9.4]).
pub const SAMPLE_WAIT: Duration = Duration::from_secs(15);

/// [`Sampler`] over a running `typeperf` ([`TYPEPERF`]); the child is stopped when the sampler is dropped.
pub struct TypeperfSampler {
    child: Child,
    rx: Receiver<Event>,
    parser: Parser,
    t0: Instant,
    sampled: bool,
}

impl TypeperfSampler {
    /// Starts typeperf, named by its full path below `%SystemRoot%`.
    pub fn start() -> Result<TypeperfSampler, String> {
        let path = TYPEPERF.resolve(std::env::var_os("SystemRoot").as_deref())?;
        let t0 = Instant::now();
        let mut child = Command::new(path)
            .args(TYPEPERF.args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("typeperf could not be started: {e}"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or("typeperf's output is not a pipe")?;
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut r = BufReader::new(stdout);
            let mut buf = Vec::new();
            loop {
                buf.clear();
                let ev = match r.read_until(b'\n', &mut buf) {
                    Ok(0) => Event::End(None),
                    Ok(_) => Event::Line(
                        String::from_utf8_lossy(&buf).into_owned(),
                        Instant::now(),
                        SystemTime::now(),
                    ),
                    Err(e) => Event::End(Some(e.to_string())),
                };
                let end = matches!(ev, Event::End(_));
                if tx.send(ev).is_err() || end {
                    break;
                }
            }
        });
        Ok(TypeperfSampler {
            child,
            rx,
            parser: Parser::default(),
            t0,
            sampled: false,
        })
    }
}

impl Sampler for TypeperfSampler {
    fn next(&mut self) -> Result<Option<Sample>, String> {
        loop {
            let wait = if self.sampled {
                SAMPLE_WAIT
            } else {
                FIRST_SAMPLE_WAIT
            };
            match self.rx.recv_timeout(wait) {
                Ok(Event::Line(line, at, wall)) => {
                    if let Some(values) = self.parser.line(&line)? {
                        self.sampled = true;
                        return Ok(Some(Sample {
                            mono_ms: u64::try_from(at.duration_since(self.t0).as_millis())
                                .unwrap_or(u64::MAX),
                            unix_ms: wall
                                .duration_since(UNIX_EPOCH)
                                .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX)),
                            values,
                        }));
                    }
                }
                Ok(Event::End(None)) | Err(RecvTimeoutError::Disconnected) => return Ok(None),
                Ok(Event::End(Some(e))) => return Err(format!("reading typeperf's output: {e}")),
                Err(RecvTimeoutError::Timeout) => {
                    return Err(format!("typeperf delivered no sample for {wait:?}"));
                }
            }
        }
    }

    fn skipped(&self) -> u64 {
        self.parser.skipped
    }
}

impl Drop for TypeperfSampler {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(machine: &str, order: &[usize]) -> String {
        let mut s = "\"(PDH-CSV 4.0) (Some Time Zone)(0)\"".to_string();
        for &i in order {
            s.push_str(&format!(",\"\\\\{machine}{}\"", SAMPLED[i]));
        }
        s
    }

    #[test]
    fn maps_the_header_and_reads_samples() {
        let mut p = Parser::default();
        assert_eq!(p.line("\r\n"), Ok(None));
        assert_eq!(p.line("\"10/04/2026 10:00:00.000\",\"1\""), Ok(None));
        assert_eq!(p.line(&header("BOX-1", &[3, 0, 2, 1])), Ok(None));
        assert_eq!(
            p.line("\"10/04/2026 10:00:01.000\",\"2000000000.000000\",\"55.5\",\"300\",\" \"\r\n"),
            Ok(Some([Some(55.5), None, Some(300.0), Some(2e9)]))
        );
        assert_eq!(p.line("Exiting, please wait..."), Ok(None));
        // A sample of the wrong width is skipped and counted; the next good one is read.
        assert_eq!(p.line("\"t\",\"1\""), Ok(None));
        assert_eq!(p.skipped, 1);
        assert_eq!(
            p.line("\"t\",\"1\",\"2\",\"3\",\"4\""),
            Ok(Some([Some(2.0), Some(4.0), Some(3.0), Some(1.0)]))
        );
        assert_eq!(p.skipped, 1);
    }

    #[test]
    fn a_missing_counter_is_an_error() {
        let mut p = Parser::default();
        let e = p.line(&header("M", &[0, 1, 2])).unwrap_err();
        assert!(e.contains(r"\Memory\Available Bytes"), "{e}");
    }

    #[test]
    fn values() {
        assert_eq!(value("12.5"), Some(12.5));
        assert_eq!(value("7"), Some(7.0));
        for bad in ["", " ", "-1", ".5", "5.", "1e3", "1.2.3", "x"] {
            assert_eq!(value(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn typeperf_command() {
        assert_eq!(TYPEPERF.program(), "typeperf.exe");
        assert_eq!(&TYPEPERF.args[..4], &SAMPLED);
        assert_eq!(&TYPEPERF.args[4..], &["-si", "1"]);
    }
}
