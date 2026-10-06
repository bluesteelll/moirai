//! The git CLI as a test-only data source (`docs/m0/PLAN.md` §2.2; §6.2 R18: product crates spawn nothing, so FL-1's
//! git differentials run here).
//!
//! Every command runs **isolated from the host's configuration**: the child gets no `GIT_*` variable of the caller
//! (a hook or a gate run may set `GIT_DIR`, `GIT_INDEX_FILE` or `GIT_DEFAULT_HASH`), no system configuration
//! (`GIT_CONFIG_NOSYSTEM`; Git for Windows' system file sets `core.autocrlf = true`), no system attributes
//! (`GIT_ATTR_NOSYSTEM`), and an empty global configuration and home directory inside the caller's scratch directory,
//! so neither a user's `core.autocrlf`, `core.attributesFile` nor `init.defaultObjectFormat` changes a result. The
//! settings a differential depends on are passed on each command line, which outranks every configuration file.
//!
//! [`HashObject`] keeps one `git hash-object --stdin-paths` process open and asks it for one object id at a time
//! (git flushes each line; `GIT_FLUSH=1` makes that explicit), so a property test pays one spawn per suite, not per
//! case.
//!
//! [`Repo::check_ignore`] and [`Repo::check_ignore_plain`] run `git check-ignore --no-index --stdin -z` over a list
//! of paths, in its verbose and its plain form. git caches each `.gitignore` it has read for the life of the process,
//! so one process answers for one state of the work tree.

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use moirai_files::oid::{Algo, ObjectFormat, Oid};

/// A failed git step.
#[derive(Debug)]
pub enum GitError {
    /// git could not be started (not installed, not on `PATH`).
    Spawn(io::Error),
    /// A scratch file or a pipe failed.
    Io(io::Error),
    /// git ran and failed.
    Failed {
        /// The arguments, for the message.
        args: String,
        /// Its exit status.
        status: ExitStatus,
        /// Its standard error, lossily decoded.
        stderr: String,
    },
    /// git's output was not what the command documents.
    Output(String),
}

impl fmt::Display for GitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GitError::Spawn(e) => write!(f, "git could not be started: {e}"),
            GitError::Io(e) => write!(f, "git I/O: {e}"),
            GitError::Failed {
                args,
                status,
                stderr,
            } => write!(f, "git {args} failed ({status}): {}", stderr.trim_end()),
            GitError::Output(m) => write!(f, "unexpected git output: {m}"),
        }
    }
}

impl std::error::Error for GitError {}

impl From<io::Error> for GitError {
    fn from(e: io::Error) -> GitError {
        GitError::Io(e)
    }
}

/// How `git hash-object` converts a file's content before hashing it ([F20 §2.3] "Git evidence never uses `oid`":
/// `oid` equals a git blob id where git converts a file exactly as `norm` does).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Conversion {
    /// The attribute `* text=auto` (through `core.attributesFile`) with `core.autocrlf = false`: git's `CRLF_AUTO`,
    /// which converts CR LF to LF exactly when `convert_is_binary` is false — [F20 §2.2]'s `norm`.
    TextAuto,
    /// `core.autocrlf = true` and no attribute: git's `CRLF_AUTO_CRLF`, the same conversion on the way in.
    AutoCrlf,
    /// `--no-filters`: the raw bytes, no conversion.
    Raw,
}

impl Conversion {
    /// Every conversion, in a fixed order.
    pub const ALL: [Conversion; 3] = [Conversion::TextAuto, Conversion::AutoCrlf, Conversion::Raw];

    /// A short name for messages.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Conversion::TextAuto => "text=auto",
            Conversion::AutoCrlf => "core.autocrlf=true",
            Conversion::Raw => "--no-filters",
        }
    }
}

/// The git CLI with an isolated environment ([module documentation](self)).
#[derive(Debug)]
pub struct Git {
    home: PathBuf,
    global: PathBuf,
    text_auto: PathBuf,
    no_attributes: PathBuf,
}

impl Git {
    /// The git on `PATH`, isolated under `scratch` (which must exist): its home directory, empty global configuration
    /// and the two attribute files of [`Conversion`] are made in `scratch/git-home`.
    ///
    /// # Errors
    /// [`GitError::Io`] when the files cannot be written.
    pub fn isolated(scratch: &Path) -> Result<Git, GitError> {
        let home = scratch.join("git-home");
        std::fs::create_dir_all(home.join(".config"))?;
        let global = home.join(".gitconfig");
        std::fs::write(&global, b"")?;
        let text_auto = home.join("text-auto.attributes");
        std::fs::write(&text_auto, b"* text=auto\n")?;
        let no_attributes = home.join("empty.attributes");
        std::fs::write(&no_attributes, b"")?;
        Ok(Git {
            home,
            global,
            text_auto,
            no_attributes,
        })
    }

    /// A `git` command with the isolated environment, standard input closed and no working directory set.
    #[must_use]
    pub fn command(&self) -> Command {
        let mut cmd = Command::new("git");
        for (name, _) in std::env::vars_os() {
            if name
                .to_str()
                .is_some_and(|n| n.to_ascii_uppercase().starts_with("GIT_"))
            {
                cmd.env_remove(&name);
            }
        }
        cmd.env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", &self.global)
            .env("GIT_ATTR_NOSYSTEM", "1")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_OPTIONAL_LOCKS", "0")
            .env("GIT_FLUSH", "1")
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("LC_ALL", "C")
            .stdin(Stdio::null());
        cmd
    }

    /// Runs `git -C <dir> <args>` to completion and returns its standard output.
    ///
    /// # Errors
    /// [`GitError::Spawn`] when git cannot start; [`GitError::Failed`] when it exits unsuccessfully.
    pub fn run<S: AsRef<OsStr>>(&self, dir: &Path, args: &[S]) -> Result<Vec<u8>, GitError> {
        let out = self
            .command()
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .map_err(GitError::Spawn)?;
        if out.status.success() {
            Ok(out.stdout)
        } else {
            Err(GitError::Failed {
                args: describe(args),
                status: out.status,
                stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
            })
        }
    }

    /// `git --version`, e.g. `git version 2.54.0.windows.1`.
    ///
    /// # Errors
    /// As [`Git::run`].
    pub fn version(&self) -> Result<String, GitError> {
        let out = self
            .command()
            .arg("--version")
            .output()
            .map_err(GitError::Spawn)?;
        if !out.status.success() {
            return Err(GitError::Failed {
                args: "--version".into(),
                status: out.status,
                stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
            });
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    /// A new repository at `dir` (created if absent) with object format `format` (`git init --object-format`), and
    /// checks that git reports that format back.
    ///
    /// # Errors
    /// As [`Git::run`]; [`GitError::Output`] when the repository reports another format.
    pub fn init(&self, dir: &Path, format: ObjectFormat) -> Result<Repo<'_>, GitError> {
        std::fs::create_dir_all(dir)?;
        let name = format.algo().name();
        self.run(
            dir,
            &[
                "-c",
                "init.defaultBranch=master",
                "init",
                "-q",
                &format!("--object-format={name}"),
            ],
        )?;
        let repo = Repo {
            git: self,
            dir: dir.to_path_buf(),
            format,
        };
        let got = repo.object_format()?;
        if got != format {
            return Err(GitError::Output(format!(
                "a repository initialised with {name} reports {}",
                got.algo().name()
            )));
        }
        Ok(repo)
    }

    /// The tracked and untracked-but-not-ignored files of the work tree at `root` whose paths end in `suffix`
    /// (compared with ASCII case folded, as [F21 §1.3]'s `eqi`), as repository-relative paths with `/`, sorted
    /// (`git ls-files -z --cached --others --exclude-standard`). A path listed in the index but absent from the
    /// work tree is left out.
    ///
    /// # Errors
    /// As [`Git::run`]; [`GitError::Output`] for a path that is not UTF-8.
    pub fn ls_files(&self, root: &Path, suffix: &str) -> Result<Vec<String>, GitError> {
        let out = self.run(
            root,
            &[
                "-c",
                "core.quotePath=false",
                "ls-files",
                "-z",
                "--cached",
                "--others",
                "--exclude-standard",
            ],
        )?;
        let mut paths = Vec::new();
        for raw in out.split(|&b| b == 0).filter(|p| !p.is_empty()) {
            let p = std::str::from_utf8(raw)
                .map_err(|_| GitError::Output("a listed path is not UTF-8".into()))?;
            let hit = p.len() >= suffix.len()
                && p.as_bytes()[p.len() - suffix.len()..].eq_ignore_ascii_case(suffix.as_bytes());
            if hit && root.join(p).is_file() {
                paths.push(p.to_string());
            }
        }
        paths.sort();
        paths.dedup();
        Ok(paths)
    }

    fn attributes_for(&self, conversion: Conversion) -> &Path {
        match conversion {
            Conversion::TextAuto => &self.text_auto,
            Conversion::AutoCrlf | Conversion::Raw => &self.no_attributes,
        }
    }
}

/// The settings of one `git check-ignore` run, passed on its command line. `core.precomposeunicode=false` is passed
/// with them on every run: `git init` writes `true` into a repository on macOS, and git would then convert the paths it
/// is given from NFD to NFC, while the matcher compares the bytes it is given; the command line outranks the
/// repository's value, so every target compares the same bytes.
#[derive(Clone, Copy, Debug)]
pub struct CheckIgnore<'a> {
    /// `core.ignorecase`. It is always passed: `git init` writes `true` into a repository on a case-insensitive file
    /// system, and the command line outranks that.
    pub ignore_case: bool,
    /// `core.excludesFile`, or `None` for git's default, `$XDG_CONFIG_HOME/git/ignore`, which the isolated home
    /// ([`Git::isolated`]) does not have.
    pub excludes_file: Option<&'a Path>,
}

impl CheckIgnore<'_> {
    /// The value passed as `core.excludesFile`, which is also how git names that source in its verbose output.
    #[must_use]
    pub fn excludes_source(&self) -> Option<String> {
        self.excludes_file.map(|p| p.to_string_lossy().into_owned())
    }
}

/// The pattern `git check-ignore -v` names as deciding a path: its source as git names it (`.gitignore`,
/// `sub/.gitignore`, `.git/info/exclude`, or the `core.excludesFile` value), its line, and the pattern as git prints
/// it (`!` when negated, the stored pattern, `/` when directory-only).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IgnoreMatch {
    /// The source.
    pub source: Vec<u8>,
    /// The line, from 1.
    pub line: u32,
    /// The pattern.
    pub pattern: Vec<u8>,
}

impl IgnoreMatch {
    /// Whether the pattern is negated: git prints a `!` before a negated pattern only, and a pattern that is not
    /// negated never begins with `!` (a line that does is a negated one).
    #[must_use]
    pub fn negated(&self) -> bool {
        self.pattern.first() == Some(&b'!')
    }
}

impl Repo<'_> {
    /// `git check-ignore --no-index -v -n -z --stdin` over `paths`: for each path, in order, the deciding pattern
    /// (negated ones included), or `None` when no pattern matches. `--no-index` reads no index, so every path is
    /// treated as untracked; a directory-only pattern matches a path that `lstat` finds to be a directory, and a path
    /// that does not exist is answered as a non-directory.
    ///
    /// Each path is relative to the work tree, non-empty, without `00`, and does not start with `:` (pathspec magic)
    /// or `/`.
    ///
    /// # Errors
    /// [`GitError::Output`] for a path outside those rules or output that is not one record per path in order;
    /// otherwise as [`Git::run`] (exit statuses 0 and 1 are both success).
    pub fn check_ignore(
        &self,
        paths: &[&str],
        settings: CheckIgnore<'_>,
    ) -> Result<Vec<Option<IgnoreMatch>>, GitError> {
        let out = self.check_ignore_raw(paths, settings, true)?;
        parse_check_ignore_verbose(&out, paths)
    }

    /// `git check-ignore --no-index -z --stdin` over `paths`: the paths git reports ignored, in input order. The
    /// plain form leaves out a path whose deciding pattern is negated.
    ///
    /// # Errors
    /// As [`Repo::check_ignore`].
    pub fn check_ignore_plain(
        &self,
        paths: &[&str],
        settings: CheckIgnore<'_>,
    ) -> Result<Vec<String>, GitError> {
        let out = self.check_ignore_raw(paths, settings, false)?;
        parse_check_ignore_plain(&out, paths)
    }

    /// Runs `check-ignore` over `paths` with `settings` and returns its standard output. `core.ignorecase` and
    /// `core.precomposeunicode=false` go on every command line, since `git init` writes either into the repository
    /// by the file system's behaviour (a case-insensitive one; macOS) and the command line outranks it
    /// ([`CheckIgnore`]).
    fn check_ignore_raw(
        &self,
        paths: &[&str],
        settings: CheckIgnore<'_>,
        verbose: bool,
    ) -> Result<Vec<u8>, GitError> {
        let mut input = Vec::new();
        for p in paths {
            if p.is_empty() || p.contains('\0') || p.starts_with([':', '/']) {
                return Err(GitError::Output(format!(
                    "{p:?} cannot be passed to check-ignore"
                )));
            }
            input.extend_from_slice(p.as_bytes());
            input.push(0);
        }
        let mut args: Vec<OsString> = vec![
            "-c".into(),
            format!("core.ignorecase={}", settings.ignore_case).into(),
            "-c".into(),
            "core.precomposeunicode=false".into(),
        ];
        if let Some(file) = settings.excludes_file {
            let mut kv = OsString::from("core.excludesFile=");
            kv.push(file);
            args.extend(["-c".into(), kv]);
        }
        args.extend(["check-ignore".into(), "--no-index".into()]);
        if verbose {
            args.extend(["-v".into(), "-n".into()]);
        }
        args.extend(["-z".into(), "--stdin".into()]);
        let mut cmd = self.git.command();
        cmd.arg("-C")
            .arg(&self.dir)
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = cmd.spawn().map_err(GitError::Spawn)?;
        let stdin = child.stdin.take();
        // The paths are written on a second thread while this one drains git's output, so neither pipe can fill
        // and block the other side.
        let (output, written) = std::thread::scope(|s| {
            let writer = s.spawn(move || -> io::Result<()> {
                if let Some(mut stdin) = stdin {
                    stdin.write_all(&input)?;
                }
                Ok(())
            });
            let output = child.wait_with_output();
            (output, writer.join())
        });
        let output = output?;
        // Exit 0: some path matched a pattern; 1: none did; anything else is an error (128 for a fatal one).
        if !matches!(output.status.code(), Some(0 | 1)) {
            return Err(GitError::Failed {
                args: describe(&args),
                status: output.status,
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            });
        }
        match written {
            Ok(Ok(())) => Ok(output.stdout),
            Ok(Err(e)) => Err(GitError::Io(e)),
            Err(_) => Err(GitError::Output(
                "the thread writing check-ignore's input panicked".into(),
            )),
        }
    }
}

/// Parses the output of `git check-ignore -v -n -z` for `paths`: four `00`-terminated fields per path, in order
/// (source, line, pattern, path; the first three empty when no pattern matched).
///
/// # Errors
/// [`GitError::Output`] for output of another shape, a line that is not a positive number, or a record whose path is
/// not the next input path.
pub fn parse_check_ignore_verbose(
    out: &[u8],
    paths: &[&str],
) -> Result<Vec<Option<IgnoreMatch>>, GitError> {
    let fields = nul_fields(out)?;
    if fields.len() != 4 * paths.len() {
        return Err(GitError::Output(format!(
            "check-ignore -v gave {} fields for {} paths",
            fields.len(),
            paths.len()
        )));
    }
    let mut answers = Vec::with_capacity(paths.len());
    for (&[source, line, pattern, path], want) in fields.as_chunks::<4>().0.iter().zip(paths) {
        if path != want.as_bytes() {
            return Err(GitError::Output(format!(
                "check-ignore -v answered for {:?} where {want:?} was next",
                String::from_utf8_lossy(path)
            )));
        }
        if source.is_empty() && line.is_empty() && pattern.is_empty() {
            answers.push(None);
            continue;
        }
        let line = std::str::from_utf8(line)
            .ok()
            .and_then(|l| l.parse::<u32>().ok())
            .filter(|&l| l > 0)
            .ok_or_else(|| {
                GitError::Output(format!(
                    "check-ignore -v gave the line {:?} for {want:?}",
                    String::from_utf8_lossy(line)
                ))
            })?;
        if source.is_empty() || pattern.is_empty() {
            return Err(GitError::Output(format!(
                "check-ignore -v gave an incomplete record for {want:?}"
            )));
        }
        answers.push(Some(IgnoreMatch {
            source: source.to_vec(),
            line,
            pattern: pattern.to_vec(),
        }));
    }
    Ok(answers)
}

/// Parses the output of plain `git check-ignore -z` for `paths`: the ignored paths, each `00`-terminated, in input
/// order.
///
/// # Errors
/// [`GitError::Output`] for output that is not a subsequence of `paths`.
pub fn parse_check_ignore_plain(out: &[u8], paths: &[&str]) -> Result<Vec<String>, GitError> {
    let mut next = paths.iter();
    let mut ignored = Vec::new();
    for field in nul_fields(out)? {
        match next.by_ref().find(|p| p.as_bytes() == field) {
            Some(p) => ignored.push((*p).to_string()),
            None => {
                return Err(GitError::Output(format!(
                    "check-ignore reported {:?}, which is not among the remaining input paths",
                    String::from_utf8_lossy(field)
                )));
            }
        }
    }
    Ok(ignored)
}

/// The `00`-terminated fields of `out`.
fn nul_fields(out: &[u8]) -> Result<Vec<&[u8]>, GitError> {
    match out.split_last() {
        None => Ok(Vec::new()),
        Some((0, body)) => Ok(body.split(|&b| b == 0).collect()),
        Some(_) => Err(GitError::Output(
            "check-ignore's output does not end in 00".into(),
        )),
    }
}

/// A scratch repository made by [`Git::init`].
#[derive(Debug)]
pub struct Repo<'g> {
    git: &'g Git,
    dir: PathBuf,
    format: ObjectFormat,
}

impl Repo<'_> {
    /// Its work tree.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The object format it was made with.
    #[must_use]
    pub const fn format(&self) -> ObjectFormat {
        self.format
    }

    /// The object format git reports (`git rev-parse --show-object-format`).
    ///
    /// # Errors
    /// As [`Git::run`]; [`GitError::Output`] for a name outside `sha1` and `sha256`.
    pub fn object_format(&self) -> Result<ObjectFormat, GitError> {
        let out = self
            .git
            .run(&self.dir, &["rev-parse", "--show-object-format"])?;
        let name = String::from_utf8_lossy(&out).trim().to_string();
        match Algo::from_name(&name).and_then(ObjectFormat::from_algo) {
            Some(f) => Ok(f),
            None => Err(GitError::Output(format!("object format '{name}'"))),
        }
    }

    /// A `git hash-object --stdin-paths` process in this repository that converts as `conversion` says.
    ///
    /// # Errors
    /// [`GitError::Spawn`] when git cannot start; [`GitError::Io`] when its standard-error file cannot be made.
    pub fn hasher(&self, conversion: Conversion) -> Result<HashObject, GitError> {
        let mut cmd = self.git.command();
        cmd.arg("-C").arg(&self.dir);
        let autocrlf = if conversion == Conversion::AutoCrlf {
            "core.autocrlf=true"
        } else {
            "core.autocrlf=false"
        };
        let mut attributes = std::ffi::OsString::from("core.attributesFile=");
        attributes.push(self.git.attributes_for(conversion));
        cmd.args(["-c", autocrlf, "-c", "core.safecrlf=false", "-c"])
            .arg(attributes)
            .arg("hash-object");
        if conversion == Conversion::Raw {
            cmd.arg("--no-filters");
        }
        // Standard error goes to a file, so no amount of it can block the process while it is being asked.
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let err_path = self.git.home.join(format!(
            "hash-object-{}.stderr",
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let err_file = std::fs::File::create(&err_path)?;
        cmd.arg("--stdin-paths")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(err_file);
        let mut child = cmd.spawn().map_err(GitError::Spawn)?;
        let stdin = child.stdin.take();
        let stdout = child.stdout.take().map(BufReader::new);
        match (stdin, stdout) {
            (Some(stdin), Some(stdout)) => Ok(HashObject {
                child,
                stdin: Some(stdin),
                stdout,
                err_path,
                format: self.format,
                conversion,
                line: String::new(),
            }),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                Err(GitError::Output("hash-object has no pipes".into()))
            }
        }
    }
}

/// One open `git hash-object --stdin-paths` process ([`Repo::hasher`]). Dropping it ends the process.
#[derive(Debug)]
pub struct HashObject {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    err_path: PathBuf,
    format: ObjectFormat,
    conversion: Conversion,
    line: String,
}

impl HashObject {
    /// The conversion this process applies.
    #[must_use]
    pub const fn conversion(&self) -> Conversion {
        self.conversion
    }

    /// The blob id git computes for the file at `rel`, a path relative to the repository's work tree without a line
    /// break.
    ///
    /// # Errors
    /// [`GitError::Output`] for a path with a line break or an answer that is not one object id of the repository's
    /// format; [`GitError::Failed`] when git stopped (its standard error is in the error); [`GitError::Io`] when a
    /// pipe fails.
    pub fn hash(&mut self, rel: &str) -> Result<Oid, GitError> {
        if rel.contains(['\n', '\r']) {
            return Err(GitError::Output(format!("path {rel:?} has a line break")));
        }
        let Some(stdin) = self.stdin.as_mut() else {
            return Err(GitError::Output("hash-object is finished".into()));
        };
        let sent = stdin
            .write_all(rel.as_bytes())
            .and_then(|()| stdin.write_all(b"\n"))
            .and_then(|()| stdin.flush());
        self.line.clear();
        let read = match sent {
            Ok(()) => self.stdout.read_line(&mut self.line),
            Err(e) => Err(e),
        };
        match read {
            Ok(n) if n > 0 => parse_oid(self.format, self.line.trim_end()),
            Ok(_) | Err(_) => Err(self.stopped(rel)),
        }
    }

    /// Closes the path list and waits for git to exit.
    ///
    /// # Errors
    /// [`GitError::Failed`] when git exits unsuccessfully.
    pub fn finish(mut self) -> Result<(), GitError> {
        drop(self.stdin.take());
        let status = self.child.wait()?;
        if status.success() {
            Ok(())
        } else {
            Err(GitError::Failed {
                args: format!("hash-object --stdin-paths ({})", self.conversion.name()),
                status,
                stderr: self.stderr(),
            })
        }
    }

    /// The error for a process that stopped answering while hashing `rel`.
    fn stopped(&mut self, rel: &str) -> GitError {
        drop(self.stdin.take());
        match self.child.wait() {
            Ok(status) => GitError::Failed {
                args: format!(
                    "hash-object --stdin-paths ({}) at {rel}",
                    self.conversion.name()
                ),
                status,
                stderr: self.stderr(),
            },
            Err(e) => GitError::Io(e),
        }
    }

    fn stderr(&self) -> String {
        std::fs::read(&self.err_path)
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .unwrap_or_default()
    }
}

impl Drop for HashObject {
    fn drop(&mut self) {
        drop(self.stdin.take());
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
    }
}

/// An object id in lower-case hexadecimal of `format`'s length ([F01 §7.5]).
///
/// # Errors
/// [`GitError::Output`] for any other text.
pub fn parse_oid(format: ObjectFormat, hex: &str) -> Result<Oid, GitError> {
    let algo = format.algo();
    let bad = || GitError::Output(format!("'{hex}' is not a {} object id", algo.name()));
    if hex.len() != 2 * algo.digest_len() {
        return Err(bad());
    }
    let mut digest = [0u8; 32];
    for (i, pair) in hex.as_bytes().chunks(2).enumerate() {
        let nib = |c: u8| match c {
            b'0'..=b'9' => Some(c - b'0'),
            b'a'..=b'f' => Some(c - b'a' + 10),
            _ => None,
        };
        match (nib(pair[0]), nib(pair[1])) {
            (Some(h), Some(l)) => digest[i] = h << 4 | l,
            _ => return Err(bad()),
        }
    }
    Oid::new(algo, &digest[..algo.digest_len()]).map_err(|_| bad())
}

fn describe<S: AsRef<OsStr>>(args: &[S]) -> String {
    args.iter()
        .map(|a| a.as_ref().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_ids_parse_in_their_format_only() {
        let sha1 = "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391";
        let oid = parse_oid(ObjectFormat::Sha1, sha1).expect("a SHA-1 id");
        assert_eq!(oid.algo(), Algo::Sha1);
        assert_eq!(oid.to_string(), sha1);
        let sha256 = "2cf8d83d9ee29543b34a87727421fdecb7e3f3a183d337639025de576db9ebb4";
        assert_eq!(
            parse_oid(ObjectFormat::Sha256, sha256)
                .expect("a SHA-256 id")
                .to_string(),
            sha256
        );
        assert!(parse_oid(ObjectFormat::Sha256, sha1).is_err());
        assert!(parse_oid(ObjectFormat::Sha1, sha256).is_err());
        assert!(parse_oid(ObjectFormat::Sha1, &sha1.to_uppercase()).is_err());
        assert!(parse_oid(ObjectFormat::Sha1, &sha1.replace('e', "g")).is_err());
        assert!(parse_oid(ObjectFormat::Sha1, "").is_err());
    }

    #[test]
    fn conversions_have_names() {
        let names: Vec<&str> = Conversion::ALL.iter().map(|c| c.name()).collect();
        assert_eq!(names, ["text=auto", "core.autocrlf=true", "--no-filters"]);
    }

    #[test]
    fn verbose_check_ignore_output_parses_per_path() {
        let paths = ["a.log", "x", "sub/k"];
        let out = b".gitignore\x001\x00*.log\x00a.log\x00\x00\x00\x00x\x00sub/.gitignore\x0012\x00!k/\x00sub/k\x00";
        let got = parse_check_ignore_verbose(out, &paths).expect("well formed");
        assert_eq!(
            got,
            [
                Some(IgnoreMatch {
                    source: b".gitignore".to_vec(),
                    line: 1,
                    pattern: b"*.log".to_vec(),
                }),
                None,
                Some(IgnoreMatch {
                    source: b"sub/.gitignore".to_vec(),
                    line: 12,
                    pattern: b"!k/".to_vec(),
                }),
            ]
        );
        assert!(!got[0].as_ref().is_some_and(IgnoreMatch::negated));
        assert!(got[2].as_ref().is_some_and(IgnoreMatch::negated));
        assert_eq!(parse_check_ignore_verbose(b"", &[]).expect("empty"), []);
        // Wrong path, missing terminator, bad line number, too few fields, a partial record.
        assert!(parse_check_ignore_verbose(b"\x00\x00\x00y\x00", &["x"]).is_err());
        assert!(parse_check_ignore_verbose(b"\x00\x00\x00x", &["x"]).is_err());
        assert!(parse_check_ignore_verbose(b"s\x000\x00p\x00x\x00", &["x"]).is_err());
        assert!(parse_check_ignore_verbose(b"s\x00one\x00p\x00x\x00", &["x"]).is_err());
        assert!(parse_check_ignore_verbose(b"\x00\x00x\x00", &["x"]).is_err());
        assert!(parse_check_ignore_verbose(b"s\x001\x00\x00x\x00", &["x"]).is_err());
    }

    #[test]
    fn plain_check_ignore_output_is_a_subsequence_of_the_paths() {
        let paths = ["a", "b", "c"];
        assert_eq!(
            parse_check_ignore_plain(b"a\x00c\x00", &paths).expect("in order"),
            ["a", "c"]
        );
        assert_eq!(
            parse_check_ignore_plain(b"", &paths).expect("none"),
            Vec::<String>::new()
        );
        assert!(parse_check_ignore_plain(b"c\x00a\x00", &paths).is_err());
        assert!(parse_check_ignore_plain(b"d\x00", &paths).is_err());
        assert!(parse_check_ignore_plain(b"a", &paths).is_err());
    }

    #[test]
    fn the_excludes_source_is_the_value_passed() {
        let p = Path::new("D:/t/excludes");
        let with = CheckIgnore {
            ignore_case: true,
            excludes_file: Some(p),
        };
        assert_eq!(with.excludes_source().as_deref(), Some("D:/t/excludes"));
        let without = CheckIgnore {
            ignore_case: false,
            excludes_file: None,
        };
        assert_eq!(without.excludes_source(), None);
    }

    #[test]
    fn arguments_are_described_for_messages() {
        assert_eq!(
            describe(&["rev-parse", "--show-object-format"]),
            "rev-parse --show-object-format"
        );
    }
}
