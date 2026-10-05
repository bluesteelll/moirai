//! `moirai-tsoracle`: prints the scope items of Rust sources as JSON Lines. The command line, the output and the
//! exit statuses are the library's crate documentation.

use std::ffi::OsString;
use std::fs::File;
use std::io::{self, BufRead, BufReader, BufWriter, Read, Write};
use std::process::ExitCode;

use moirai_tsoracle::record::{write_record, write_version};
use moirai_tsoracle::scan::MAX_INPUT;
use moirai_tsoracle::{Oracle, OracleError, Scan};

const USAGE: &str = "\
usage: moirai-tsoracle [FILE | - | --files-from LIST]... [-- FILE...]
       moirai-tsoracle --version
       moirai-tsoracle --help | -h
Prints one JSON line per Rust source: its scope items as tree-sitter-rust sees them.
  FILE               a Rust source file
  -                  read one source from standard input
  --files-from LIST  read source paths from LIST, one per line ('-': standard input)
  --                 every later argument is a FILE, even '-'";

/// Output buffer size. Each record is flushed when it is complete, so a record of up to 64 KiB leaves in one write
/// and a larger one in several.
const OUT_BUFFER: usize = 64 * 1024;

/// A failed run: the exit status and the message for standard error.
struct Failure {
    status: u8,
    message: String,
}

impl Failure {
    fn usage(message: impl Into<String>) -> Self {
        Self {
            status: 2,
            message: format!("moirai-tsoracle: {}\n{USAGE}", message.into()),
        }
    }

    fn input(path: &str, what: impl std::fmt::Display) -> Self {
        Self {
            status: 1,
            message: format!("moirai-tsoracle: {path}: {what}"),
        }
    }

    fn output(e: &io::Error) -> Self {
        Self {
            status: 1,
            message: format!("moirai-tsoracle: cannot write the output: {e}"),
        }
    }
}

fn main() -> ExitCode {
    match run(std::env::args_os().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(f) => {
            // Standard error may itself be closed; the exit status still reports the failure.
            let _ = writeln!(io::stderr().lock(), "{}", f.message);
            ExitCode::from(f.status)
        }
    }
}

fn run(args: Vec<OsString>) -> Result<(), Failure> {
    let args: Vec<String> = args
        .into_iter()
        .map(|a| {
            a.into_string()
                .map_err(|a| Failure::usage(format!("argument is not valid Unicode: {a:?}")))
        })
        .collect::<Result<_, _>>()?;
    match args.first().map(String::as_str) {
        None => return Err(Failure::usage("no input")),
        Some("--help" | "-h") if args.len() == 1 => {
            let mut out = io::stdout().lock();
            return writeln!(out, "{USAGE}")
                .and_then(|()| out.flush())
                .map_err(|e| Failure::output(&e));
        }
        Some("--version") if args.len() == 1 => {
            let abi = moirai_tsoracle::scan::language_abi();
            let mut out = io::stdout().lock();
            // This unit's own directory: cargo may have compiled it in another work tree than the library.
            return write_version(&mut out, abi, env!("CARGO_MANIFEST_DIR"))
                .and_then(|()| out.flush())
                .map_err(|e| Failure::output(&e));
        }
        Some(_) => {}
    }
    let plan = plan(&args)?;
    let oracle = Oracle::new().map_err(|e| Failure {
        status: 1,
        message: format!("moirai-tsoracle: {e}"),
    })?;
    let mut session = Session {
        oracle,
        scan: Scan::new(),
        src: Vec::new(),
        out: BufWriter::with_capacity(OUT_BUFFER, io::stdout().lock()),
    };
    for input in plan {
        match input {
            Input::File(path) => session.file(path)?,
            Input::Stdin => session.stdin()?,
            Input::List("-") => session.list(io::stdin().lock(), "-")?,
            Input::List(list) => {
                let file = File::open(list).map_err(|e| Failure::input(list, e))?;
                session.list(BufReader::new(file), list)?;
            }
        }
    }
    // Every record was flushed by `emit`.
    Ok(())
}

/// One command-line input, in argument order.
enum Input<'a> {
    File(&'a str),
    Stdin,
    List(&'a str),
}

/// Checks the whole command line before any input is read, so a usage error writes no record.
fn plan(args: &[String]) -> Result<Vec<Input<'_>>, Failure> {
    let mut plan = Vec::with_capacity(args.len());
    let mut stdin_used = false;
    let mut use_stdin = || {
        if std::mem::replace(&mut stdin_used, true) {
            Err(Failure::usage("standard input requested twice"))
        } else {
            Ok(())
        }
    };
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--" => {
                plan.extend(rest.by_ref().map(|a| Input::File(a.as_str())));
            }
            "-" => {
                use_stdin()?;
                plan.push(Input::Stdin);
            }
            "--files-from" => {
                let list = rest
                    .next()
                    .ok_or_else(|| Failure::usage("--files-from needs a LIST"))?;
                if list == "-" {
                    use_stdin()?;
                }
                plan.push(Input::List(list.as_str()));
            }
            a if a.starts_with('-') => return Err(Failure::usage(format!("unknown option {a}"))),
            a => plan.push(Input::File(a)),
        }
    }
    if plan.is_empty() {
        return Err(Failure::usage("no input"));
    }
    Ok(plan)
}

/// The state shared by every input of a run: one parser, one scan, one source buffer, one output buffer.
struct Session<W: Write> {
    oracle: Oracle,
    scan: Scan,
    src: Vec<u8>,
    out: W,
}

impl<W: Write> Session<W> {
    /// Reads and scans one file. A file larger than [`MAX_INPUT`] is refused before any of it is read; the read itself
    /// is bounded too, for a file that grows while it is read.
    fn file(&mut self, path: &str) -> Result<(), Failure> {
        let file = File::open(path).map_err(|e| Failure::input(path, e))?;
        let len = file.metadata().map_err(|e| Failure::input(path, e))?.len();
        if len > MAX_INPUT {
            return Err(Failure::input(path, OracleError::TooLarge(len)));
        }
        self.src.clear();
        // The whole file is held for tree-sitter, so its size is reserved at once rather than grown by doubling.
        self.src
            .try_reserve_exact(usize::try_from(len).unwrap_or(0))
            .map_err(|e| Failure::input(path, e))?;
        self.read(file, path)?;
        self.emit(path)
    }

    fn stdin(&mut self) -> Result<(), Failure> {
        self.src.clear();
        self.read(io::stdin().lock(), "-")?;
        self.emit("-")
    }

    /// Appends all of `input` to `src`, or refuses it once it has more than [`MAX_INPUT`] bytes.
    fn read(&mut self, input: impl Read, path: &str) -> Result<(), Failure> {
        let within =
            read_bounded(input, MAX_INPUT, &mut self.src).map_err(|e| Failure::input(path, e))?;
        if within {
            Ok(())
        } else {
            Err(Failure::input(path, OracleError::TooLarge(MAX_INPUT + 1)))
        }
    }

    /// Scans every path of a list as it is read, so the list is never held whole. One UTF-8 BOM at the start of the
    /// list is skipped (Windows PowerShell 5.1's `Out-File -Encoding utf8` writes one).
    fn list<R: BufRead>(&mut self, mut reader: R, list: &str) -> Result<(), Failure> {
        let mut line = String::new();
        let mut first = true;
        loop {
            line.clear();
            let n = reader
                .read_line(&mut line)
                .map_err(|e| Failure::input(list, e))?;
            if n == 0 {
                return Ok(());
            }
            let mut path = line.strip_suffix('\n').unwrap_or(&line);
            path = path.strip_suffix('\r').unwrap_or(path);
            if std::mem::replace(&mut first, false) {
                path = path.strip_prefix('\u{FEFF}').unwrap_or(path);
            }
            if !path.is_empty() {
                self.file(path)?;
            }
        }
    }

    /// Scans the source in `src` and writes its record, flushed at once: a caller that feeds `--files-from -` one path
    /// at a time gets each record before it sends the next path.
    fn emit(&mut self, path: &str) -> Result<(), Failure> {
        self.oracle
            .scan(&self.src, &mut self.scan)
            .map_err(|e| Failure::input(path, e))?;
        write_record(&mut self.out, path, &self.scan)
            .and_then(|()| self.out.flush())
            .map_err(|e| Failure::output(&e))
    }
}

/// Appends the rest of `input` to `buf`, reading at most `limit` + 1 bytes. Returns `false` when `input` holds more
/// than `limit` bytes; `buf` then ends with the first `limit` + 1 of them, and the rest is never read.
fn read_bounded(input: impl Read, limit: u64, buf: &mut Vec<u8>) -> io::Result<bool> {
    let from = buf.len();
    input.take(limit.saturating_add(1)).read_to_end(buf)?;
    Ok(u64::try_from(buf.len() - from).is_ok_and(|n| n <= limit))
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use super::read_bounded;

    #[test]
    fn a_bounded_read_stops_one_byte_past_the_limit() {
        let mut buf = b"x".to_vec();
        assert!(read_bounded(&b"abcdef"[..], 6, &mut buf).expect("reads"));
        assert_eq!(buf, b"xabcdef");
        buf.clear();
        assert!(read_bounded(&b""[..], 0, &mut buf).expect("reads"));
        assert!(buf.is_empty());
        let mut input = &b"abcdefgh"[..];
        assert!(!read_bounded(&mut input, 5, &mut buf).expect("reads"));
        assert_eq!(buf, b"abcdef");
        // The bytes past the limit + 1 are left unread.
        let mut rest = Vec::new();
        input.read_to_end(&mut rest).expect("reads");
        assert_eq!(rest, b"gh");
    }
}
