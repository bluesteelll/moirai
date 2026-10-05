//! The client of `moirai-tsoracle` ([F21 §3.9]; WP-74): finds the oracle binary, checks that it is this work tree's
//! and that its pins are this client's, and reads its JSON Lines records, format 3 (the oracle's crate documentation,
//! "Output").
//!
//! No crate depends on the host-only oracle (GT20 (b) rule 3), so the differential runs its binary as a child
//! process. The binary is built only by the replay job, unpoisoned (`docs/m0/PLAN.md` §2.1; `docs/m0/tools.md` §8):
//! `cargo build --locked -p moirai-tsoracle -p moirai-replay --all-targets` puts it in the target directory's
//! profile directory, beside the `deps` directory that holds this crate's test executables, which is where
//! [`locate`] looks. It uses a binary only when it is this work tree's oracle as the sources are now ([`Stale`]):
//! cargo's dep-info file beside it lists only sources of this tree, none newer than the binary, and its `--version`
//! record names this tree's oracle crate as the directory each of its two compilation units was compiled in. The
//! dep-info alone cannot tell: cargo rewrites it for the tree that last ran a build over the binary, even one that
//! recompiled nothing, and that need not be the tree that compiled it (the oracle's crate documentation, "Output").
//!
//! [`ENV`] names the binary explicitly and makes a missing or stale one an error instead of a skip. The replay job of
//! `pr.yml` (R-HARN's) must set it, as `MOIRAI_TSORACLE: target/debug/moirai-tsoracle.exe` (resolved against the
//! workspace root): without it a missing or stale oracle skips the differentials and the job passes, with only the
//! skip lines the tests write past libtest's capture to tell. The gate, which never builds the oracle (it runs
//! poisoned, PLAN §2.1), leaves it unset and skips.
//!
//! [`Session`] keeps one `moirai-tsoracle --files-from -` process open and sends it one path at a time; the oracle
//! writes and flushes each record before it reads the next path, so a property test pays one spawn per suite.

use std::ffi::OsStr;
use std::fmt;
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

/// The record format this client reads: format 3 of the oracle's crate documentation ("Output"), whose rule 8 also
/// leaves out of the claim every item after an error that unbalances brackets. [F21 §3.9] still cites format 2.
pub const FORMAT: u64 = 3;

/// The tree-sitter runtime the oracle must be built with (`docs/m0/tools.md` §11).
pub const TREE_SITTER: &str = "0.27.0";

/// The tree-sitter-rust grammar the oracle must be built with (`docs/m0/tools.md` §11).
pub const TREE_SITTER_RUST: &str = "0.24.2";

/// The environment variable that names the oracle binary: an absolute path, or one relative to the workspace root.
/// When it is set, a missing or stale binary is an error instead of a reason to skip.
pub const ENV: &str = "MOIRAI_TSORACLE";

/// A failed oracle step.
#[derive(Debug)]
pub enum OracleError {
    /// The binary could not be started.
    Spawn(PathBuf, io::Error),
    /// A pipe or a scratch file failed.
    Io(io::Error),
    /// The binary's `--version` record names other pins or another format.
    Version(String),
    /// The oracle stopped: its exit status and standard error.
    Stopped(String),
    /// A record that is not format 3.
    Record(String),
    /// [`ENV`] is set and names no usable binary.
    Required(String),
}

impl fmt::Display for OracleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OracleError::Spawn(p, e) => write!(f, "{} could not be started: {e}", p.display()),
            OracleError::Io(e) => write!(f, "oracle I/O: {e}"),
            OracleError::Version(m) => write!(f, "oracle pins: {m}"),
            OracleError::Stopped(m) => write!(f, "the oracle stopped: {m}"),
            OracleError::Record(m) => write!(f, "oracle record: {m}"),
            OracleError::Required(m) => write!(f, "{ENV}: {m}"),
        }
    }
}

impl std::error::Error for OracleError {}

impl From<io::Error> for OracleError {
    fn from(e: io::Error) -> OracleError {
        OracleError::Io(e)
    }
}

/// Where the oracle binary is, or why there is none to use.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Located {
    /// A binary compiled in this work tree's `crates/moirai-tsoracle` (its `--version` record says so for both of its
    /// units), newer than every source its dep-info lists, all of them in that directory.
    Found(PathBuf),
    /// No binary at the path looked at.
    Missing(PathBuf),
    /// A binary that is not this work tree's current oracle.
    Stale {
        /// The binary.
        exe: PathBuf,
        /// Why.
        why: Stale,
    },
}

/// Why an oracle binary is not this work tree's current oracle. Cargo's dep-info file beside the binary
/// ([`dep_info_sources`]) is the exact list of the binary's sources, spelled in the work tree that last ran a build
/// over it; it leaves out what is not compiled into the binary (`#[cfg(test)]` modules such as `src/scan/tests.rs`,
/// `tests/`).
/// The binary's `--version` record names the directories its two units were compiled in (`manifest_dir`,
/// `bin_manifest_dir`), which the dep-info does not: work trees that share a target directory (`docs/m0/PLAN.md`
/// §2.1: one per lane) share the oracle's compiled units, because cargo fingerprints a workspace member by paths
/// relative to the workspace root, and a build in this tree whose sources are older than another tree's build
/// recompiles nothing but rewrites the dep-info to list this tree's sources.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Stale {
    /// The dep-info file (named) is missing or lists no source: the binary was not built by cargo here.
    NoDepInfo(PathBuf),
    /// Another work tree compiled the binary, or one of its units, into the target directory it shares with this
    /// one: a source its dep-info lists, or a directory its `--version` record names, lies outside this work tree's
    /// `crates/moirai-tsoracle` (or no longer exists).
    Foreign(PathBuf),
    /// The `--version` record does not name the directories the binary was compiled in: it was compiled from older
    /// oracle sources than this client's, in another work tree, since this tree's own dep-info check passed.
    Unidentified,
    /// A source that no longer exists.
    Gone(PathBuf),
    /// A source newer than the binary: it changed after the build.
    Newer(PathBuf),
}

impl Stale {
    /// The commands that give this work tree its own current oracle. A plain build recompiles a source of this tree
    /// that is newer than the binary; in every other case it may recompile nothing (the sources are older than
    /// another tree's build, or cargo's fingerprint still passes), so the oracle's units are removed first.
    #[must_use]
    pub fn remedy(&self) -> &'static str {
        match self {
            Stale::Newer(_) => "`cargo build -p moirai-tsoracle`",
            Stale::NoDepInfo(_) | Stale::Foreign(_) | Stale::Unidentified | Stale::Gone(_) => {
                "`cargo clean -p moirai-tsoracle` and then `cargo build -p moirai-tsoracle`"
            }
        }
    }
}

impl fmt::Display for Stale {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Stale::NoDepInfo(p) => {
                write!(f, "cargo's dep-info {} is missing or empty", p.display())
            }
            Stale::Foreign(p) => write!(
                f,
                "it was compiled from {}, outside this work tree",
                p.display()
            ),
            Stale::Unidentified => f.write_str(
                "its --version record does not name the directories it was compiled in (older oracle sources)",
            ),
            Stale::Gone(p) => write!(f, "its source {} no longer exists", p.display()),
            Stale::Newer(p) => write!(f, "its source {} is newer than it", p.display()),
        }
    }
}

/// Finds the oracle binary for a test executable `test_exe` (normally `std::env::current_exe()`) of the workspace at
/// `workspace`: [`ENV`] when set, else `moirai-tsoracle` with the platform's executable suffix in the directory above
/// the one holding `test_exe` (cargo's `<target>/<profile>/deps/` → `<target>/<profile>/`). The binary is used only
/// when it is this work tree's oracle as the sources are now ([`Stale`]): its dep-info is checked first, and then
/// its `--version` record, which runs it.
///
/// # Errors
/// [`OracleError::Required`] when [`ENV`] is set and its binary is missing or stale; [`OracleError::Io`] when the
/// oracle's crate directory, a listed source or a named directory cannot be examined; [`OracleError::Spawn`] or
/// [`OracleError::Version`] when a binary that passes the dep-info check cannot print its `--version` record.
pub fn locate(test_exe: &Path, workspace: &Path) -> Result<Located, OracleError> {
    let explicit = std::env::var_os(ENV);
    resolve(test_exe, workspace, explicit.as_deref(), version_line)
}

/// [`locate`] with the value of [`ENV`] (`explicit`, empty meaning unset) and the way to read a binary's `--version`
/// record given.
fn resolve(
    test_exe: &Path,
    workspace: &Path,
    explicit: Option<&OsStr>,
    version: impl FnOnce(&Path) -> Result<String, OracleError>,
) -> Result<Located, OracleError> {
    let explicit = explicit.filter(|v| !v.is_empty());
    let exe = match explicit {
        Some(v) => workspace.join(v),
        None => {
            let name = format!("moirai-tsoracle{}", std::env::consts::EXE_SUFFIX);
            let deps = test_exe.parent().unwrap_or(test_exe);
            deps.parent().unwrap_or(deps).join(name)
        }
    };
    let found = assess(
        &exe,
        &workspace.join("crates").join("moirai-tsoracle"),
        version,
    )?;
    match (&found, explicit.is_some()) {
        (Located::Found(_), _) | (_, false) => Ok(found),
        (Located::Missing(p), true) => Err(OracleError::Required(format!(
            "no oracle binary at {}",
            p.display()
        ))),
        (Located::Stale { exe, why }, true) => Err(OracleError::Required(format!(
            "{} is stale: {why}; give this work tree its own oracle with {}",
            exe.display(),
            why.remedy()
        ))),
    }
}

/// Whether `exe` is the oracle of the crate directory `crate_dir` as its sources are now: [`freshness`] first, and
/// only for a binary that passes it, the `--version` record that `version` reads from it ([`provenance`]).
fn assess(
    exe: &Path,
    crate_dir: &Path,
    version: impl FnOnce(&Path) -> Result<String, OracleError>,
) -> Result<Located, OracleError> {
    let found = freshness(exe, crate_dir)?;
    if !matches!(found, Located::Found(_)) {
        return Ok(found);
    }
    let line = version(exe)?;
    Ok(match provenance(&line, crate_dir)? {
        Some(why) => Located::Stale {
            exe: exe.to_path_buf(),
            why,
        },
        None => found,
    })
}

/// Why the `--version` record `line` does not show a binary compiled in `crate_dir`, or `None` when both of the
/// directories it names (`manifest_dir`, `bin_manifest_dir`; the oracle's crate documentation, "Output") are
/// `crate_dir`, compared canonicalised. A directory that no longer exists is foreign.
///
/// # Errors
/// [`OracleError::Version`] when `line` is not JSON; [`OracleError::Io`] when a directory cannot be examined.
fn provenance(line: &str, crate_dir: &Path) -> Result<Option<Stale>, OracleError> {
    let v: Value = serde_json::from_str(line.trim_end())
        .map_err(|e| OracleError::Version(format!("{e}: {}", cut(line))))?;
    let home = std::fs::canonicalize(crate_dir)?;
    for key in ["manifest_dir", "bin_manifest_dir"] {
        let Some(dir) = v.get(key).and_then(Value::as_str) else {
            return Ok(Some(Stale::Unidentified));
        };
        let dir = PathBuf::from(dir);
        match std::fs::canonicalize(&dir) {
            Ok(c) if c == home => {}
            Ok(_) => return Ok(Some(Stale::Foreign(dir))),
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Some(Stale::Foreign(dir))),
            Err(e) => return Err(OracleError::Io(e)),
        }
    }
    Ok(None)
}

/// The standard output of `exe --version`.
///
/// # Errors
/// [`OracleError::Spawn`] when it cannot start; [`OracleError::Version`] when it exits unsuccessfully.
fn version_line(exe: &Path) -> Result<String, OracleError> {
    let out = Command::new(exe)
        .arg("--version")
        .stdin(Stdio::null())
        .output()
        .map_err(|e| OracleError::Spawn(exe.to_path_buf(), e))?;
    if !out.status.success() {
        return Err(OracleError::Version(format!(
            "--version exited with {}: {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim_end()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Whether `exe` exists and was built by cargo from sources that all lie under `crate_dir` and are not newer than it.
/// The sources are those cargo's dep-info file `exe` with extension `d` lists; paths are compared canonicalised, so
/// the case of a drive letter or a junction changes nothing.
fn freshness(exe: &Path, crate_dir: &Path) -> Result<Located, OracleError> {
    let built = match std::fs::metadata(exe) {
        Ok(m) if m.is_file() => m.modified()?,
        _ => return Ok(Located::Missing(exe.to_path_buf())),
    };
    let stale = |why| {
        Ok(Located::Stale {
            exe: exe.to_path_buf(),
            why,
        })
    };
    let d = exe.with_extension("d");
    let sources = match std::fs::read_to_string(&d) {
        Ok(text) => dep_info_sources(&text),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(OracleError::Io(e)),
    };
    if sources.is_empty() {
        return stale(Stale::NoDepInfo(d));
    }
    let home = std::fs::canonicalize(crate_dir)?;
    for source in sources {
        let canonical = match std::fs::canonicalize(&source) {
            Ok(c) => c,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return stale(Stale::Gone(source)),
            Err(e) => return Err(OracleError::Io(e)),
        };
        if !canonical.starts_with(&home) {
            return stale(Stale::Foreign(source));
        }
        if std::fs::metadata(&canonical)?.modified()? > built {
            return stale(Stale::Newer(source));
        }
    }
    Ok(Located::Found(exe.to_path_buf()))
}

/// The sources a cargo dep-info file lists (`<target>: <source> <source>…`, one rule; `#` lines are comments): the
/// words after the first `: `, split at spaces, where a word ending in `\` continues with a space and the next word —
/// cargo writes a space inside a path as `\ ` and reads it back this way (`parse_rustc_dep_info`), so a Windows `\`
/// separator stays as it is. An unreadable rule lists nothing.
#[must_use]
pub fn dep_info_sources(text: &str) -> Vec<PathBuf> {
    let Some(rule) = text
        .lines()
        .map(str::trim_end)
        .find(|l| !l.is_empty() && !l.starts_with('#'))
    else {
        return Vec::new();
    };
    let Some(at) = rule.find(": ") else {
        return Vec::new();
    };
    let mut words = rule[at + 2..].split(' ').filter(|w| !w.is_empty());
    let mut out = Vec::new();
    while let Some(w) = words.next() {
        let mut path = w.to_string();
        while path.ends_with('\\') {
            let Some(next) = words.next() else {
                return Vec::new();
            };
            path.pop();
            path.push(' ');
            path.push_str(next);
        }
        out.push(PathBuf::from(path));
    }
    out
}

/// One item of a record (the oracle's rules 1–8; [F21 §3.9]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OracleItem {
    /// The `skind` of [F08 §10.3.1]: 1 `mod` … 9 `macro_rules`.
    pub skind: u8,
    /// The canonical name ([F21 §3.2]).
    pub name: String,
    /// The canonical qualifier; empty unless `impl Trait for T`.
    pub qual: String,
    /// The first line, 1-based.
    pub start: u64,
    /// The last line.
    pub end: u64,
    /// The index of the enclosing item, less than the item's own.
    pub parent: Option<usize>,
    /// Inside the oracle's claim (rule 8).
    pub ok: bool,
}

/// One record: the oracle's items for one source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    /// The input as it was given.
    pub path: String,
    /// ERROR and MISSING nodes, plus 1 for a source that is not UTF-8.
    pub errors: u64,
    /// The items in pre-order.
    pub items: Vec<OracleItem>,
}

/// The `skind` of an oracle `kind` name.
#[must_use]
pub fn skind_of(kind: &str) -> Option<u8> {
    Some(match kind {
        "mod" => 1,
        "impl" => 2,
        "fn" => 3,
        "struct" => 4,
        "enum" => 5,
        "trait" => 6,
        "const" => 7,
        "static" => 8,
        "macro_rules" => 9,
        _ => return None,
    })
}

/// Parses one record line, checking every key, type and the parent order, and that every item of a record with no
/// errors is claimed (the oracle's crate documentation: `ok` is "always `true` when `errors` is 0").
///
/// # Errors
/// [`OracleError::Record`] for anything that is not a format-2 record.
pub fn parse_record(line: &str) -> Result<Record, OracleError> {
    let bad = |m: &str| OracleError::Record(format!("{m}: {}", cut(line)));
    let v: Value = serde_json::from_str(line).map_err(|e| bad(&e.to_string()))?;
    let obj = v.as_object().ok_or_else(|| bad("not an object"))?;
    if obj.len() != 3 {
        return Err(bad("a record has exactly the keys path, errors, items"));
    }
    let path = obj
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| bad("path"))?;
    let errors = obj
        .get("errors")
        .and_then(Value::as_u64)
        .ok_or_else(|| bad("errors"))?;
    let list = obj
        .get("items")
        .and_then(Value::as_array)
        .ok_or_else(|| bad("items"))?;
    let mut items = Vec::with_capacity(list.len());
    for (i, it) in list.iter().enumerate() {
        let o = it
            .as_object()
            .ok_or_else(|| bad("an item is not an object"))?;
        if o.len() != 7 {
            return Err(bad("an item has exactly seven keys"));
        }
        let text = |k: &str| o.get(k).and_then(Value::as_str).map(str::to_string);
        let num = |k: &str| o.get(k).and_then(Value::as_u64);
        let skind = o
            .get("kind")
            .and_then(Value::as_str)
            .and_then(skind_of)
            .ok_or_else(|| bad("kind"))?;
        let parent = match o.get("parent") {
            Some(Value::Null) => None,
            Some(p) => {
                let p = p
                    .as_u64()
                    .and_then(|p| usize::try_from(p).ok())
                    .filter(|&p| p < i)
                    .ok_or_else(|| bad("parent"))?;
                Some(p)
            }
            None => return Err(bad("parent")),
        };
        let item = OracleItem {
            skind,
            name: text("name").ok_or_else(|| bad("name"))?,
            qual: text("qual").ok_or_else(|| bad("qual"))?,
            start: num("start").ok_or_else(|| bad("start"))?,
            end: num("end").ok_or_else(|| bad("end"))?,
            parent,
            ok: o
                .get("ok")
                .and_then(Value::as_bool)
                .ok_or_else(|| bad("ok"))?,
        };
        if item.name.is_empty() || item.start == 0 || item.end < item.start {
            return Err(bad("an item has an empty name or a bad line span"));
        }
        // The claim is always whole in a file without errors (the oracle's `ok` field): an unclaimed item there would
        // leave the differential's denominator silently.
        if errors == 0 && !item.ok {
            return Err(bad("an item outside the claim in a record with no errors"));
        }
        items.push(item);
    }
    Ok(Record {
        path: path.to_string(),
        errors,
        items,
    })
}

/// Checks a `--version` record against [`FORMAT`], [`TREE_SITTER`] and [`TREE_SITTER_RUST`]. The directories it
/// names are [`locate`]'s check.
///
/// # Errors
/// [`OracleError::Version`] naming the first difference.
pub fn check_version(line: &str) -> Result<(), OracleError> {
    let v: Value = serde_json::from_str(line.trim_end())
        .map_err(|e| OracleError::Version(format!("{e}: {}", cut(line))))?;
    let want = [
        ("oracle", Value::from("moirai-tsoracle")),
        ("format", Value::from(FORMAT)),
        ("tree_sitter", Value::from(TREE_SITTER)),
        ("tree_sitter_rust", Value::from(TREE_SITTER_RUST)),
    ];
    for (k, w) in want {
        if v.get(k) != Some(&w) {
            return Err(OracleError::Version(format!(
                "{k} is {:?}, expected {w}",
                v.get(k)
            )));
        }
    }
    if v.get("language_abi").and_then(Value::as_u64).is_none() {
        return Err(OracleError::Version("no language_abi".into()));
    }
    Ok(())
}

/// One open `moirai-tsoracle --files-from -` process. Dropping it ends the process.
#[derive(Debug)]
pub struct Session {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    err_path: PathBuf,
    line: String,
}

impl Session {
    /// Checks the pins of the binary `exe` (`--version`) and starts it on a path list read from its standard input;
    /// its standard error goes to a file in `scratch`. Which work tree compiled `exe` is [`locate`]'s check.
    ///
    /// # Errors
    /// [`OracleError::Spawn`] when it cannot start; [`OracleError::Version`] for other pins.
    pub fn start(exe: &Path, scratch: &Path) -> Result<Session, OracleError> {
        check_version(&version_line(exe)?)?;

        static NEXT: AtomicU64 = AtomicU64::new(0);
        let err_path = scratch.join(format!(
            "tsoracle-{}-{}.stderr",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let err_file = std::fs::File::create(&err_path)?;
        let mut child = Command::new(exe)
            .args(["--files-from", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(err_file)
            .spawn()
            .map_err(|e| OracleError::Spawn(exe.to_path_buf(), e))?;
        let stdin = child.stdin.take();
        let stdout = child.stdout.take().map(BufReader::new);
        match (stdin, stdout) {
            (Some(stdin), Some(stdout)) => Ok(Session {
                child,
                stdin: Some(stdin),
                stdout,
                err_path,
                line: String::new(),
            }),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                Err(OracleError::Stopped("no pipes".into()))
            }
        }
    }

    /// The record of the Rust source at `path` (UTF-8, no line break; relative paths are resolved against the
    /// caller's working directory).
    ///
    /// # Errors
    /// [`OracleError::Stopped`] when the oracle exits (an unreadable file stops it, with its message);
    /// [`OracleError::Record`] for a record that is not format 3 or not for `path`.
    pub fn scan(&mut self, path: &str) -> Result<Record, OracleError> {
        if path.contains(['\n', '\r']) {
            return Err(OracleError::Record(format!(
                "path {path:?} has a line break"
            )));
        }
        let Some(stdin) = self.stdin.as_mut() else {
            return Err(OracleError::Stopped("the session is finished".into()));
        };
        let sent = stdin
            .write_all(path.as_bytes())
            .and_then(|()| stdin.write_all(b"\n"))
            .and_then(|()| stdin.flush());
        self.line.clear();
        let read = match sent {
            Ok(()) => self.stdout.read_line(&mut self.line),
            Err(e) => Err(e),
        };
        match read {
            Ok(n) if n > 0 && self.line.ends_with('\n') => {
                let rec = parse_record(self.line.trim_end_matches('\n'))?;
                if rec.path == path {
                    Ok(rec)
                } else {
                    Err(OracleError::Record(format!(
                        "record for {:?} while {path:?} was asked",
                        rec.path
                    )))
                }
            }
            _ => Err(self.stopped()),
        }
    }

    /// Closes the path list and waits for the oracle to exit.
    ///
    /// # Errors
    /// [`OracleError::Stopped`] when it exits unsuccessfully.
    pub fn finish(mut self) -> Result<(), OracleError> {
        drop(self.stdin.take());
        let status = self.child.wait()?;
        if status.success() {
            Ok(())
        } else {
            Err(OracleError::Stopped(format!("{status}: {}", self.stderr())))
        }
    }

    fn stopped(&mut self) -> OracleError {
        drop(self.stdin.take());
        match self.child.wait() {
            Ok(status) => OracleError::Stopped(format!("{status}: {}", self.stderr())),
            Err(e) => OracleError::Io(e),
        }
    }

    fn stderr(&self) -> String {
        std::fs::read(&self.err_path)
            .map(|b| String::from_utf8_lossy(&b).trim_end().to_string())
            .unwrap_or_default()
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        drop(self.stdin.take());
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.err_path);
    }
}

/// At most 200 bytes of a line, for messages.
fn cut(line: &str) -> &str {
    let mut end = line.len().min(200);
    while !line.is_char_boundary(end) {
        end -= 1;
    }
    &line[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    const RECORD: &str = concat!(
        r#"{"path":"src/x.rs","errors":0,"items":["#,
        r#"{"kind":"mod","name":"a","qual":"","start":1,"end":3,"parent":null,"ok":true},"#,
        r#"{"kind":"fn","name":"f","qual":"","start":2,"end":2,"parent":0,"ok":true},"#,
        r#"{"kind":"impl","name":"S","qual":"Display","start":4,"end":4,"parent":null,"ok":true}"#,
        "]}"
    );

    #[test]
    fn the_documented_record_parses() {
        let r = parse_record(RECORD).expect("the crate documentation's example");
        assert_eq!(r.path, "src/x.rs");
        assert_eq!(r.errors, 0);
        assert_eq!(r.items.len(), 3);
        assert_eq!(
            r.items[1],
            OracleItem {
                skind: 3,
                name: "f".into(),
                qual: String::new(),
                start: 2,
                end: 2,
                parent: Some(0),
                ok: true
            }
        );
        assert_eq!(r.items[2].skind, 2);
        assert_eq!(r.items[2].qual, "Display");
        assert!(r.items.iter().all(|i| i.ok));
    }

    #[test]
    fn only_a_record_with_errors_has_unclaimed_items() {
        let unclaimed = RECORD.replacen(
            r#""end":4,"parent":null,"ok":true"#,
            r#""end":4,"parent":null,"ok":false"#,
            1,
        );
        assert_ne!(unclaimed, RECORD);
        match parse_record(&unclaimed) {
            Err(OracleError::Record(m)) => assert!(m.contains("outside the claim"), "{m}"),
            other => panic!("an unclaimed item with no errors must be refused: {other:?}"),
        }
        let r = parse_record(&unclaimed.replacen(r#""errors":0"#, r#""errors":1"#, 1))
            .expect("unclaimed items in a record with errors");
        assert_eq!(r.errors, 1);
        assert!(!r.items[2].ok && r.items[0].ok);
    }

    #[test]
    fn malformed_records_are_refused() {
        for bad in [
            "",
            "[]",
            r#"{"path":"x","errors":0}"#,
            r#"{"path":"x","errors":-1,"items":[]}"#,
            r#"{"path":"x","errors":0,"items":[],"extra":1}"#,
            r#"{"path":"x","errors":0,"items":[{"kind":"union","name":"U","qual":"","start":1,"end":1,"parent":null,"ok":true}]}"#,
            r#"{"path":"x","errors":0,"items":[{"kind":"fn","name":"","qual":"","start":1,"end":1,"parent":null,"ok":true}]}"#,
            r#"{"path":"x","errors":0,"items":[{"kind":"fn","name":"f","qual":"","start":2,"end":1,"parent":null,"ok":true}]}"#,
            r#"{"path":"x","errors":0,"items":[{"kind":"fn","name":"f","qual":"","start":1,"end":1,"parent":0,"ok":true}]}"#,
            r#"{"path":"x","errors":0,"items":[{"kind":"fn","name":"f","qual":"","start":1,"end":1,"ok":true}]}"#,
            r#"{"path":"x","errors":0,"items":[{"kind":"fn","name":"f","qual":"","start":1,"end":1,"parent":null,"ok":"yes"}]}"#,
        ] {
            assert!(parse_record(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn kinds_are_the_nine_skinds() {
        let kinds = [
            "mod",
            "impl",
            "fn",
            "struct",
            "enum",
            "trait",
            "const",
            "static",
            "macro_rules",
        ];
        for (i, k) in kinds.iter().enumerate() {
            assert_eq!(skind_of(k), u8::try_from(i + 1).ok());
        }
        assert_eq!(skind_of("union"), None);
    }

    #[test]
    fn the_version_record_is_checked() {
        let good = r#"{"oracle":"moirai-tsoracle","format":3,"tree_sitter":"0.27.0","tree_sitter_rust":"0.24.2","language_abi":15}"#;
        check_version(good).expect("the pinned record");
        check_version(&format!("{good}\n")).expect("with its line feed");
        for bad in [
            good.replace("\"format\":3", "\"format\":2"),
            good.replace("0.24.2", "0.24.3"),
            good.replace("0.27.0", "0.26.0"),
            good.replace("moirai-tsoracle", "other"),
            good.replace(",\"language_abi\":15", ""),
            String::from("not json"),
        ] {
            assert!(check_version(&bad).is_err(), "{bad}");
        }
    }

    /// A directory under the system's temporary directory (unit tests have no `CARGO_TARGET_TMPDIR`), removed with
    /// everything in it when dropped, so a failed assertion leaves nothing behind.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> TempDir {
            let dir =
                std::env::temp_dir().join(format!("moirai-replay-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("a scratch directory");
            TempDir(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// A dep-info rule as cargo writes it: spaces inside a path escaped as `\ `.
    fn rule(exe: &Path, sources: &[&Path]) -> String {
        let esc = |p: &Path| p.to_str().expect("a UTF-8 path").replace(' ', r"\ ");
        let mut s = format!("{}:", esc(exe));
        for p in sources {
            s.push(' ');
            s.push_str(&esc(p));
        }
        s.push('\n');
        s
    }

    #[test]
    fn dep_info_rules_are_read_as_cargo_writes_them() {
        assert_eq!(
            dep_info_sources(concat!(
                r"D:/t\debug\x.exe: D:\w\a\ b\src\main.rs D:\w\lib.rs",
                "\r\n# checksum\n"
            )),
            [
                PathBuf::from(r"D:\w\a b\src\main.rs"),
                PathBuf::from(r"D:\w\lib.rs")
            ]
        );
        assert_eq!(
            dep_info_sources(concat!(r"/t/debug/x: /w/src/a\ \ b.rs  /w/src/c.rs", "\n")),
            [
                PathBuf::from("/w/src/a  b.rs"),
                PathBuf::from("/w/src/c.rs")
            ]
        );
        assert_eq!(
            dep_info_sources("# only a comment\n\n/t/x: /w/a.rs\n"),
            [PathBuf::from("/w/a.rs")]
        );
        for empty in ["", "/t/x:", "/t/x:\n", "no rule\n", "/t/x: /w/trailing\\\n"] {
            assert!(dep_info_sources(empty).is_empty(), "{empty:?}");
        }
    }

    #[test]
    fn freshness_follows_the_dep_info_sources() {
        let tmp = TempDir::new("fresh");
        // A space in the work tree's path exercises the dep-info escape.
        let home = tmp
            .0
            .join("work tree")
            .join("crates")
            .join("moirai-tsoracle");
        let other = tmp.0.join("other").join("crates").join("moirai-tsoracle");
        for dir in [home.join("src").join("scan"), other.join("src")] {
            std::fs::create_dir_all(dir).expect("scratch");
        }
        let main = home.join("src").join("main.rs");
        let unit_tests = home.join("src").join("scan").join("tests.rs");
        let foreign = other.join("src").join("main.rs");
        for f in [&main, &unit_tests, &foreign] {
            std::fs::write(f, b"//! x\n").expect("scratch");
        }
        let target = tmp.0.join("target").join("debug");
        std::fs::create_dir_all(&target).expect("scratch");
        let exe = target.join("moirai-tsoracle.exe");
        let d = target.join("moirai-tsoracle.d");
        let at = |why| Located::Stale {
            exe: exe.clone(),
            why,
        };

        assert_eq!(
            freshness(&exe, &home).expect("readable"),
            Located::Missing(exe.clone())
        );
        std::fs::write(&exe, b"").expect("scratch");
        assert_eq!(
            freshness(&exe, &home).expect("readable"),
            at(Stale::NoDepInfo(d.clone()))
        );
        std::fs::write(&d, format!("{}:\n", exe.display())).expect("scratch");
        assert_eq!(
            freshness(&exe, &home).expect("readable"),
            at(Stale::NoDepInfo(d.clone()))
        );

        let t = std::fs::metadata(&exe)
            .and_then(|m| m.modified())
            .expect("mtime");
        let older = t - std::time::Duration::from_secs(60);
        let newer = t + std::time::Duration::from_secs(60);
        set_mtime(&main, older);
        // A cfg(test) module newer than the binary does not make it stale: it is not one of its sources, and a
        // rebuild would not relink the binary.
        set_mtime(&unit_tests, newer);
        std::fs::write(&d, rule(&exe, &[&main])).expect("scratch");
        assert_eq!(
            freshness(&exe, &home).expect("readable"),
            Located::Found(exe.clone())
        );

        set_mtime(&main, newer);
        assert_eq!(
            freshness(&exe, &home).expect("readable"),
            at(Stale::Newer(main.clone()))
        );
        set_mtime(&main, older);

        // Another work tree built the binary into the shared target directory: its sources are older, but foreign.
        set_mtime(&foreign, older);
        std::fs::write(&d, rule(&exe, &[&foreign])).expect("scratch");
        assert_eq!(
            freshness(&exe, &home).expect("readable"),
            at(Stale::Foreign(foreign.clone()))
        );

        let gone = home.join("src").join("gone.rs");
        std::fs::write(&d, rule(&exe, &[&main, &gone])).expect("scratch");
        assert_eq!(
            freshness(&exe, &home).expect("readable"),
            at(Stale::Gone(gone.clone()))
        );

        let text = Stale::Foreign(foreign).to_string();
        assert!(text.contains("outside this work tree"), "{text}");
    }

    /// A work tree with the oracle's crate directory and a built binary, another work tree's crate directory, and a
    /// dep-info that lists only this tree's `src/main.rs`, which is older than the binary: the dep-info check passes.
    struct Tree {
        _tmp: TempDir,
        workspace: PathBuf,
        home: PathBuf,
        other: PathBuf,
        exe: PathBuf,
    }

    impl Tree {
        fn new(name: &str) -> Tree {
            let tmp = TempDir::new(name);
            let workspace = tmp.0.join("work tree");
            let home = workspace.join("crates").join("moirai-tsoracle");
            let other = tmp.0.join("other").join("crates").join("moirai-tsoracle");
            for dir in [home.join("src"), other.join("src")] {
                std::fs::create_dir_all(dir).expect("scratch");
            }
            let main = home.join("src").join("main.rs");
            std::fs::write(&main, b"//! x\n").expect("scratch");
            let target = workspace.join("target").join("debug");
            std::fs::create_dir_all(target.join("deps")).expect("scratch");
            let exe = target.join(format!("moirai-tsoracle{}", std::env::consts::EXE_SUFFIX));
            std::fs::write(&exe, b"").expect("scratch");
            let t = std::fs::metadata(&exe)
                .and_then(|m| m.modified())
                .expect("mtime");
            set_mtime(&main, t - std::time::Duration::from_secs(60));
            std::fs::write(exe.with_extension("d"), rule(&exe, &[&main])).expect("scratch");
            Tree {
                _tmp: tmp,
                workspace,
                home,
                other,
                exe,
            }
        }

        /// A test executable of this tree's target directory, which need not exist.
        fn test_exe(&self) -> PathBuf {
            self.exe
                .parent()
                .expect("the profile directory")
                .join("deps")
                .join("scan_oracle-0123.exe")
        }
    }

    /// A `--version` record with the pins and the given directories.
    fn version_record(lib: Option<&Path>, bin: Option<&Path>) -> String {
        let mut v = serde_json::json!({
            "oracle": "moirai-tsoracle",
            "format": FORMAT,
            "tree_sitter": TREE_SITTER,
            "tree_sitter_rust": TREE_SITTER_RUST,
            "language_abi": 15,
        });
        for (key, dir) in [("manifest_dir", lib), ("bin_manifest_dir", bin)] {
            if let Some(dir) = dir {
                v[key] = Value::from(dir.to_str().expect("a UTF-8 path"));
            }
        }
        format!("{v}\n")
    }

    #[test]
    fn provenance_needs_both_units_compiled_in_this_tree() {
        let tree = Tree::new("provenance");
        let (home, other) = (tree.home.as_path(), tree.other.as_path());
        let gone = tree.other.join("gone");
        let check = |lib, bin| provenance(&version_record(lib, bin), home).expect("readable");

        assert_eq!(check(Some(home), Some(home)), None);
        // Directories are compared canonicalised.
        let roundabout = home.join("src").join("..");
        assert_eq!(check(Some(roundabout.as_path()), Some(home)), None);
        check_version(&version_record(Some(home), Some(home))).expect("the pins");

        assert_eq!(
            check(Some(other), Some(home)),
            Some(Stale::Foreign(other.to_path_buf()))
        );
        // `main.rs` recompiled in another tree, linked with this tree's library.
        assert_eq!(
            check(Some(home), Some(other)),
            Some(Stale::Foreign(other.to_path_buf()))
        );
        assert_eq!(
            check(Some(gone.as_path()), Some(home)),
            Some(Stale::Foreign(gone.clone()))
        );
        assert_eq!(check(None, Some(home)), Some(Stale::Unidentified));
        assert_eq!(check(Some(home), None), Some(Stale::Unidentified));
        let numeric = version_record(None, Some(home)).replace('{', "{\"manifest_dir\":1,");
        assert_eq!(
            provenance(&numeric, home).expect("readable"),
            Some(Stale::Unidentified)
        );
        assert!(matches!(
            provenance("not json", home),
            Err(OracleError::Version(_))
        ));
    }

    /// The dep-info of a binary another work tree compiled lists this tree's sources once a build here has run over
    /// it, even one that recompiled nothing; its `--version` record still names the other tree.
    #[test]
    fn a_matching_dep_info_with_a_foreign_manifest_dir_is_stale() {
        let tree = Tree::new("foreign-manifest");
        let (home, other, exe) = (&tree.home, &tree.other, &tree.exe);
        let reads = |lib: &Path, bin: &Path| {
            let line = version_record(Some(lib), Some(bin));
            move |p: &Path| {
                assert_eq!(p, exe.as_path(), "the binary's own record is read");
                Ok(line)
            }
        };
        assert_eq!(
            freshness(exe, home).expect("readable"),
            Located::Found(exe.clone())
        );
        assert_eq!(
            assess(exe, home, reads(other, other)).expect("readable"),
            Located::Stale {
                exe: exe.clone(),
                why: Stale::Foreign(other.clone())
            }
        );
        assert_eq!(
            assess(exe, home, reads(home, home)).expect("readable"),
            Located::Found(exe.clone())
        );
        let old = |_: &Path| Ok(version_record(None, None));
        assert_eq!(
            assess(exe, home, old).expect("readable"),
            Located::Stale {
                exe: exe.clone(),
                why: Stale::Unidentified
            }
        );

        // Without the variable, the tests skip; with it, the job fails and is told how to recompile.
        let test_exe = tree.test_exe();
        assert_eq!(
            resolve(&test_exe, &tree.workspace, None, reads(other, other)).expect("readable"),
            Located::Stale {
                exe: exe.clone(),
                why: Stale::Foreign(other.clone())
            }
        );
        assert_eq!(
            resolve(
                &test_exe,
                &tree.workspace,
                Some(OsStr::new("")),
                reads(other, home)
            )
            .expect("an empty variable is unset"),
            Located::Stale {
                exe: exe.clone(),
                why: Stale::Foreign(other.clone())
            }
        );
        let rel = Path::new("target")
            .join("debug")
            .join(exe.file_name().expect("a file name"));
        match resolve(
            &test_exe,
            &tree.workspace,
            Some(rel.as_os_str()),
            reads(other, other),
        ) {
            Err(OracleError::Required(m)) => {
                assert!(m.contains("outside this work tree"), "{m}");
                assert!(m.contains("`cargo clean -p moirai-tsoracle`"), "{m}");
            }
            got => panic!("{got:?}"),
        }
        assert_eq!(
            resolve(
                &test_exe,
                &tree.workspace,
                Some(rel.as_os_str()),
                reads(home, home)
            )
            .expect("this tree's binary"),
            Located::Found(tree.workspace.join(&rel))
        );
        match resolve(
            &test_exe,
            &tree.workspace,
            Some(OsStr::new("absent.exe")),
            reads(home, home),
        ) {
            Err(OracleError::Required(m)) => assert!(m.contains("no oracle binary"), "{m}"),
            got => panic!("{got:?}"),
        }
    }

    /// A binary that fails the dep-info check is never run.
    #[test]
    fn a_stale_dep_info_is_judged_without_running_the_binary() {
        let tree = Tree::new("stale-unrun");
        let main = tree.home.join("src").join("main.rs");
        let t = std::fs::metadata(&tree.exe)
            .and_then(|m| m.modified())
            .expect("mtime");
        set_mtime(&main, t + std::time::Duration::from_secs(60));
        let unrun = |_: &Path| -> Result<String, OracleError> { panic!("the binary was run") };
        let found = assess(&tree.exe, &tree.home, unrun).expect("readable");
        assert_eq!(
            found,
            Located::Stale {
                exe: tree.exe.clone(),
                why: Stale::Newer(main)
            }
        );
    }

    #[test]
    fn only_a_newer_source_is_remedied_by_a_plain_build() {
        let p = PathBuf::from("x");
        assert_eq!(
            Stale::Newer(p.clone()).remedy(),
            "`cargo build -p moirai-tsoracle`"
        );
        for why in [
            Stale::NoDepInfo(p.clone()),
            Stale::Foreign(p.clone()),
            Stale::Unidentified,
            Stale::Gone(p),
        ] {
            let r = why.remedy();
            assert!(
                r.starts_with("`cargo clean -p moirai-tsoracle`")
                    && r.ends_with("`cargo build -p moirai-tsoracle`"),
                "{why:?}: {r}"
            );
        }
    }

    fn set_mtime(path: &Path, t: std::time::SystemTime) {
        let f = std::fs::File::options()
            .write(true)
            .open(path)
            .expect("scratch file");
        f.set_modified(t).expect("set the mtime");
    }

    #[test]
    fn messages_cut_long_lines_at_a_character_boundary() {
        let s = "é".repeat(150);
        let c = cut(&s);
        assert!(c.len() <= 200 && s.starts_with(c));
    }
}
