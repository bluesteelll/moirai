//! FL-1's ignore matcher (WP-61b; `moirai_files::ignore`, [F20 §4.4]) against `git check-ignore --no-index`: the
//! synthetic tree, the two sides and the report of WP-61b's acceptance ("WP-74's differential agrees with
//! `git check-ignore` on synthetic trees").
//!
//! A [`Tree`] is a work tree held in memory: directories, empty regular files, `.gitignore` files, optionally
//! `$GIT_DIR/info/exclude` and a `core.excludesFile`, and **virtual files**: paths that are asked about but never
//! written, whose names hold `*` or `?` or end in a space or a dot, which no file name on Windows may. git answers an
//! absent path as a non-directory, so they compare the positive half of the escapes (`\*`, `\?`, `[*?]` and `\ `
//! matching those bytes literally), which names on disk cannot reach. [`check_tree`] writes the tree into a scratch
//! repository and asks, for every path of the tree (directories, files, virtual files and the `.gitignore` files
//! themselves) and for both values of `core.ignorecase`:
//!
//! - **git**, through [`Repo::check_ignore`] (`-v -n`: the deciding pattern, negated ones included) and
//!   [`Repo::check_ignore_plain`] (the plain form: the ignored paths);
//! - **the matcher**, through [`IgnoreStack::check`] in [`Mode::Git`], reading each source back from the disk as git
//!   does, with `is_dir` from the tree.
//!
//! Two answers agree when both name no pattern, or both name the same source (the directory of a `.gitignore`,
//! `info/exclude` or `core.excludesFile`), the same line and the same pattern as git prints it
//! ([`PatternRef::write_git_form`]); the plain form agrees when git lists exactly the paths the matcher ignores
//! ([`GitSide::agrees`]). The [`Report`] counts the checks of each comparison ([`Tally`]) and the classes git's answers
//! fall in, and keeps the first [`DETAIL_MAX`] disagreements (each with its path, both answers) and failing trees
//! (with their sources) in detail, counting the rest.
//!
//! **Without git** ([40 §5.8]). The matcher's [`Mode::NoGit`] reads the `.gitignore` files, lets the `files.ignore`
//! items stand in for the root directory's `.gitignore` when it has none, and has no `info/exclude` or
//! `core.excludesFile`. For a tree with a root `.gitignore` and neither global source, git's answers therefore give
//! three more comparisons, which [`check_tree`] makes:
//!
//! - **the root's own file** ([`GitSide::OwnRoot`]): the matcher in `Mode::NoGit` reads the root's `.gitignore`, with
//!   `files.ignore` set to `*`, which would change almost every answer if it were used; git's answers are the answers.
//! - **the stand-in** ([`GitSide::StandIn`]): the matcher is told the root has no `.gitignore`, and `files.ignore`
//!   holds that file's lines ([`Tree::files_ignore_stand_in`], the BOM dropped). [`PatternList::from_items`] treats
//!   each item as one gitignore line numbered from 1, so git's answers are the matcher's with `files.ignore` in the
//!   root file's place ([`in_root_place`]); an answer that names the hidden root file is a disagreement. This
//!   exercises `from_items` as a line API: it is given items (empty ones, trailing spaces, CR, `00`, `,`) that no
//!   `files.ignore` value can hold.
//! - **the glob-list stand-in** ([`GitSide::GlobList`]): the same with only the lines a [CFG §4.1] `glob-list` can
//!   carry ([`glob_list_can_carry`], [`Tree::glob_list_stand_in`]), against git on a root `.gitignore` of exactly
//!   those lines, which [`check_tree`] writes for the purpose when lines were left out. This is the reachable
//!   `files.ignore` pipeline.
//!
//! **Never-candidate names** ([F20 §4.7.1], §4.7.2). A never-candidate pattern (`*`, `?`, every other byte
//! compared with `eqi`) means what the same line means in a `.gitignore` under `core.ignorecase = true`, as long as
//! [`is_gitignore_safe_name_pattern`] holds. [`check_names`] writes a list of such patterns into the root `.gitignore`
//! in reverse order ([`reversed_source`]), so git's deciding pattern (the last match) is the list's first match
//! ([`list_index`]), and compares it with `matches_name_pattern` for every name (and, for the list of [F20 §4.7.2]
//! itself, with `never_pattern`).
//!
//! **What this cannot see.** `--no-index` treats every path as untracked, so the tracked test of [F20 §4.4] stays
//! the caller's (and a skip-worktree `.gitignore` read from the index, which the matcher's caller supplies, is out
//! of reach). The matcher's `ancestor` field (which excluded directory decided) has no counterpart in git's output:
//! it is counted, not compared. A pattern file of [`moirai_files::ignore::PATTERN_FILE_MAX`] bytes or more is not
//! generated. Symbolic links are not generated (git does not follow an in-tree `.gitignore` link).

use std::fmt;
use std::fs;
use std::io;
use std::path::Path;

use moirai_files::ignore::{
    Case, Hit, IgnoreStack, Mode, PatternList, PatternRef, Source, Verdict, matches_name_pattern,
    never_pattern,
};

use crate::git::{CheckIgnore, GitError, IgnoreMatch, Repo};

/// How many disagreements, and how many failing trees, a report keeps in detail; the rest are counted only, so a
/// systematic matcher bug at a wide tier holds and prints a bounded report.
pub const DETAIL_MAX: usize = 200;

/// The device names Windows reserves, with or without an extension, compared without case.
pub const WINDOWS_DEVICE_NAMES: &[&str] = &[
    "con", "prn", "aux", "nul", "com0", "com1", "com2", "com3", "com4", "com5", "com6", "com7",
    "com8", "com9", "lpt0", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
    "conin$", "conout$",
];

/// The `files.ignore` items of the comparison that reads the root's own `.gitignore` ([`GitSide::OwnRoot`]): a
/// pattern that would decide almost every path if the matcher used it.
const OWN_ROOT_FILES_IGNORE: &[&[u8]] = &[b"*"];

/// A synthetic work tree. Paths are root-relative with `/`, valid file names on every target (no `\ / : * ? " < > |`,
/// no control byte, no trailing `.` or space, no reserved device name), and distinct within a directory under case
/// folding, so the tree is the same on a case-insensitive file system; virtual files are the exception (their own
/// documentation).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tree {
    /// Directories other than the root; every parent is listed too.
    pub dirs: Vec<String>,
    /// Empty regular files other than the `.gitignore` files.
    pub files: Vec<String>,
    /// Regular files that are asked about but never written (module documentation), each in a directory of the tree
    /// and valid by [`Tree::virtual_file_problem`]: names holding `*` or `?` or ending in a space or a dot.
    pub virtual_files: Vec<String>,
    /// `.gitignore` contents by directory (`""` for the root), at most one per directory.
    pub gitignores: Vec<(String, Vec<u8>)>,
    /// `$GIT_DIR/info/exclude`, or `None` for no such file.
    pub info_exclude: Option<Vec<u8>>,
    /// The `core.excludesFile` contents, or `None` to leave `core.excludesFile` unset.
    pub excludes_file: Option<Vec<u8>>,
}

impl Tree {
    /// The paths [`check_tree`] asks about, sorted, each with whether it is a directory.
    #[must_use]
    pub fn paths(&self) -> Vec<(String, bool)> {
        let mut v: Vec<(String, bool)> = self
            .real_paths()
            .chain(self.virtual_files.iter().map(|f| (f.clone(), false)))
            .collect();
        v.sort();
        v.dedup();
        v
    }

    /// The paths written to the disk, each with whether it is a directory.
    fn real_paths(&self) -> impl Iterator<Item = (String, bool)> + '_ {
        self.dirs
            .iter()
            .map(|d| (d.clone(), true))
            .chain(self.files.iter().map(|f| (f.clone(), false)))
            .chain(
                self.gitignores
                    .iter()
                    .map(|(d, _)| (join(d, ".gitignore"), false)),
            )
    }

    /// Why `path` cannot be a virtual file of this tree, or `None` when it can. A virtual file lies in a directory of
    /// the tree; its name is not empty and holds no `:` (pathspec magic, which makes git abort), `\` (a separator to
    /// git for Windows), `"`, `<`, `>`, `|` or control byte; it is no path of the tree under case folding; and with
    /// its trailing spaces and dots removed it is neither empty nor a directory of the tree nor a reserved device
    /// name. Win32 removes those trailing bytes, so git's `lstat` on Windows would find that directory or device in
    /// the virtual file's place, and git would answer for a directory where the matcher is told a file.
    #[must_use]
    pub fn virtual_file_problem(&self, path: &str) -> Option<String> {
        let (parent, name) = match path.rfind('/') {
            Some(i) => (&path[..i], &path[i + 1..]),
            None => ("", path),
        };
        if !parent.is_empty() && !self.dirs.iter().any(|d| d == parent) {
            return Some(format!(
                "the virtual file {path:?} is not in a directory of the tree"
            ));
        }
        if name.is_empty()
            || name.bytes().any(|b| {
                matches!(b, b':' | b'\\' | b'"' | b'<' | b'>' | b'|') || b < 0x20 || b == 0x7f
            })
        {
            return Some(format!(
                "the virtual file {path:?} has an empty name or a byte git or the file system treats specially"
            ));
        }
        let folded = path.to_lowercase();
        if self.real_paths().any(|(p, _)| p.to_lowercase() == folded) {
            return Some(format!(
                "the virtual file {path:?} is a path of the tree under case folding"
            ));
        }
        let stripped = name.trim_end_matches([' ', '.']);
        let stem = stripped
            .split('.')
            .next()
            .unwrap_or_default()
            .to_lowercase();
        let stripped_path = join(parent, stripped).to_lowercase();
        if stripped.is_empty()
            || WINDOWS_DEVICE_NAMES.contains(&stem.as_str())
            || self
                .real_paths()
                .any(|(p, dir)| dir && p.to_lowercase() == stripped_path)
        {
            return Some(format!(
                "the virtual file {path:?} names its directory, a directory of the tree or a device on Windows"
            ));
        }
        None
    }

    /// The first problem of [`Tree::virtual_files`] ([`Tree::virtual_file_problem`], or a path listed twice), or
    /// `None`.
    #[must_use]
    pub fn virtual_files_problem(&self) -> Option<String> {
        self.virtual_files.iter().enumerate().find_map(|(i, v)| {
            if self.virtual_files[..i].contains(v) {
                Some(format!("the virtual file {v:?} is listed twice"))
            } else {
                self.virtual_file_problem(v)
            }
        })
    }

    /// The `files.ignore` items of the stand-in comparison (module documentation): the lines of the root
    /// `.gitignore` (split at `0A`, a leading UTF-8 BOM dropped; a final `0A` starts no line), each of which
    /// [`PatternList::from_items`] reads as that gitignore line, or `None` when the tree has no root `.gitignore` or
    /// has `info/exclude` or a `core.excludesFile`.
    // spec: [40 §5.8] (without git, `files.ignore` in place of the root directory's `.gitignore`), [CFG §10.6]
    #[must_use]
    pub fn files_ignore_stand_in(&self) -> Option<Vec<&[u8]>> {
        if self.info_exclude.is_some() || self.excludes_file.is_some() {
            return None;
        }
        let (_, root) = self.gitignores.iter().find(|(d, _)| d.is_empty())?;
        let root = root.strip_prefix(b"\xef\xbb\xbf").unwrap_or(root);
        let root = root.strip_suffix(b"\n").unwrap_or(root);
        Some(if root.is_empty() {
            Vec::new()
        } else {
            root.split(|&b| b == b'\n').collect()
        })
    }

    /// The `files.ignore` items of the glob-list comparison (module documentation): the items of
    /// [`Tree::files_ignore_stand_in`] that a `glob-list` can carry ([`glob_list_can_carry`]), in order, or `None`
    /// when there is no stand-in.
    // spec: [CFG §4.1] (`glob-list`), [CFG §10.6] (`files.ignore`), [40 §5.8]
    #[must_use]
    pub fn glob_list_stand_in(&self) -> Option<Vec<&[u8]>> {
        let mut items = self.files_ignore_stand_in()?;
        items.retain(|item| glob_list_can_carry(item));
        Some(items)
    }

    /// Writes the tree into the work tree `work` of a repository whose git directory is `work/.git`, after removing
    /// everything in `work` but `.git`; `info/exclude` goes into `work/.git/info/exclude` and the `core.excludesFile`
    /// contents to `excludes` (each removed when the tree has none). Virtual files are not written.
    ///
    /// # Errors
    /// The first file-system error.
    pub fn write(&self, work: &Path, excludes: &Path) -> io::Result<()> {
        for entry in fs::read_dir(work)? {
            let entry = entry?;
            if entry.file_name() == ".git" {
                continue;
            }
            if entry.file_type()?.is_dir() {
                fs::remove_dir_all(entry.path())?;
            } else {
                fs::remove_file(entry.path())?;
            }
        }
        for d in &self.dirs {
            fs::create_dir_all(work.join(d))?;
        }
        for f in &self.files {
            fs::write(work.join(f), b"")?;
        }
        for (d, bytes) in &self.gitignores {
            fs::write(work.join(join(d, ".gitignore")), bytes)?;
        }
        let info = work.join(".git").join("info");
        fs::create_dir_all(&info)?;
        write_or_remove(&info.join("exclude"), self.info_exclude.as_deref())?;
        write_or_remove(excludes, self.excludes_file.as_deref())
    }

    /// The tree for a report: its sources with every byte outside printable ASCII escaped, line by line (and the
    /// glob-list stand-in when it leaves lines out), then its directories, files and virtual files.
    #[must_use]
    pub fn describe(&self) -> String {
        fn lines(bytes: &[u8]) -> Vec<&[u8]> {
            bytes.split(|&c| c == b'\n').collect()
        }
        let mut s = String::new();
        for (d, bytes) in &self.gitignores {
            describe_source(&mut s, &join(d, ".gitignore"), &lines(bytes));
        }
        if let Some(b) = &self.info_exclude {
            describe_source(&mut s, ".git/info/exclude", &lines(b));
        }
        if let Some(b) = &self.excludes_file {
            describe_source(&mut s, "core.excludesFile", &lines(b));
        }
        if let Some(items) = self.glob_list_stand_in()
            && Some(&items) != self.files_ignore_stand_in().as_ref()
        {
            describe_source(
                &mut s,
                "files.ignore of the glob-list comparison (and the root .gitignore git reads then)",
                &items,
            );
        }
        s.push_str(&format!(
            "  dirs: {:?}\n  files: {:?}\n  virtual files: {:?}\n",
            self.dirs, self.files, self.virtual_files
        ));
        s
    }
}

/// Appends the source `name` to a tree's description, its lines numbered from 1 and every byte outside printable
/// ASCII escaped.
fn describe_source(s: &mut String, name: &str, lines: &[&[u8]]) {
    s.push_str(&format!("  {name}:\n"));
    for (i, line) in lines.iter().enumerate() {
        s.push_str(&format!("    {:>3} | {}\n", i + 1, line.escape_ascii()));
    }
}

/// Whether a [CFG §4.1] `glob-list` value can carry `item` as one of its items: valid UTF-8 and not empty; no `,`
/// (the separator); no control byte other than a tab (a configuration value ends at its line end, [CFG §3.4]); no
/// space or tab at either end (each item is trimmed); and no U+FEFF at the start, which git would drop as a
/// byte-order mark from the first line of the root file the glob-list comparison writes.
// spec: [CFG §4.1] (`glob-list`: items separated by `,`, each trimmed; an empty item is invalid), [CFG §3.4]
#[must_use]
pub fn glob_list_can_carry(item: &[u8]) -> bool {
    let blank = |b: &u8| matches!(b, b' ' | b'\t');
    std::str::from_utf8(item).is_ok()
        && !item.is_empty()
        && !item.first().is_some_and(blank)
        && !item.last().is_some_and(blank)
        && !item.starts_with(b"\xef\xbb\xbf")
        && !item
            .iter()
            .any(|&b| b == b',' || b == 0x7f || (b < 0x20 && b != b'\t'))
}

/// `dir/name`, or `name` for the root.
fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_string()
    } else {
        format!("{dir}/{name}")
    }
}

fn write_or_remove(path: &Path, bytes: Option<&[u8]>) -> io::Result<()> {
    match bytes {
        Some(b) => fs::write(path, b),
        None => match fs::remove_file(path) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            other => other,
        },
    }
}

/// Where a deciding pattern came from, on either side.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Origin {
    /// The `.gitignore` of this directory (`""` for the root).
    Dir(String),
    /// `$GIT_DIR/info/exclude`.
    InfoExclude,
    /// `core.excludesFile`.
    ExcludesFile,
    /// `files.ignore`, which the matcher must never name in [`Mode::Git`] (a disagreement whenever it does).
    FilesIgnore,
}

impl fmt::Display for Origin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Origin::Dir(d) => f.write_str(&join(d, ".gitignore")),
            Origin::InfoExclude => f.write_str(".git/info/exclude"),
            Origin::ExcludesFile => f.write_str("core.excludesFile"),
            Origin::FilesIgnore => f.write_str("files.ignore"),
        }
    }
}

/// The deciding pattern of one answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decision {
    /// Its source.
    pub origin: Origin,
    /// Its line, from 1.
    pub line: u32,
    /// The pattern as `git check-ignore -v` prints it.
    pub pattern: Vec<u8>,
}

impl Decision {
    /// Whether the pattern is negated (it re-includes the path).
    #[must_use]
    pub fn negated(&self) -> bool {
        self.pattern.first() == Some(&b'!')
    }

    /// Whether the pattern is directory-only.
    #[must_use]
    pub fn dir_only(&self) -> bool {
        self.pattern.last() == Some(&b'/')
    }
}

/// One side's answer for one path: the deciding pattern, or `None` when no pattern matches. The path is ignored iff
/// the pattern is not negated.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Answer(pub Option<Decision>);

impl Answer {
    /// Whether the path is ignored.
    #[must_use]
    pub fn ignored(&self) -> bool {
        self.0.as_ref().is_some_and(|d| !d.negated())
    }

    /// Whether the deciding pattern came from the root directory's `.gitignore`.
    fn names_root_file(&self) -> bool {
        self.0
            .as_ref()
            .is_some_and(|d| matches!(&d.origin, Origin::Dir(dir) if dir.is_empty()))
    }
}

impl fmt::Display for Answer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            None => f.write_str("no pattern (not ignored)"),
            Some(d) => write!(
                f,
                "{} by {}:{}: \"{}\"",
                if d.negated() {
                    "re-included"
                } else {
                    "ignored"
                },
                d.origin,
                d.line,
                d.pattern.escape_ascii()
            ),
        }
    }
}

/// git's answer from one record of [`Repo::check_ignore`]; `excludes` is the `core.excludesFile` value passed
/// ([`CheckIgnore::excludes_source`]).
///
/// # Errors
/// A message for a source that is none of the tree's.
pub fn git_answer(m: Option<IgnoreMatch>, excludes: Option<&str>) -> Result<Answer, String> {
    let Some(m) = m else {
        return Ok(Answer(None));
    };
    let origin = if excludes.is_some_and(|e| m.source == e.as_bytes()) {
        Origin::ExcludesFile
    } else if m.source == b".git/info/exclude" {
        Origin::InfoExclude
    } else if m.source == b".gitignore" {
        Origin::Dir(String::new())
    } else if let Some(dir) = m.source.strip_suffix(b"/.gitignore") {
        Origin::Dir(String::from_utf8(dir.to_vec()).map_err(|_| {
            format!(
                "check-ignore named a source that is not UTF-8: {}",
                m.source.escape_ascii()
            )
        })?)
    } else {
        return Err(format!(
            "check-ignore named an unknown source: {}",
            m.source.escape_ascii()
        ));
    };
    Ok(Answer(Some(Decision {
        origin,
        line: m.line,
        pattern: m.pattern,
    })))
}

/// The matcher's answer from a [`Verdict`], and whether an excluded ancestor directory decided it.
///
/// # Errors
/// A message when `ignored` contradicts the deciding pattern.
pub fn product_answer(v: &Verdict<'_>) -> Result<(Answer, bool), String> {
    let answer = Answer(v.hit.as_ref().map(decision));
    if answer.ignored() != v.ignored {
        return Err(format!(
            "the verdict says ignored = {} with {answer}",
            v.ignored
        ));
    }
    Ok((answer, v.hit.is_some_and(|h| h.ancestor.is_some())))
}

/// A stand-in answer as git names it when the root `.gitignore` holds the `files.ignore` lines: `files.ignore` in the
/// root `.gitignore`'s place (module documentation). An answer that names the root file itself is left as it is;
/// [`GitSide::agrees`] refuses it before this mapping.
// spec: [40 §5.8] (without git, `files.ignore` in place of the root directory's `.gitignore`)
#[must_use]
pub fn in_root_place(a: &Answer) -> Answer {
    Answer(a.0.as_ref().map(|d| Decision {
        origin: match &d.origin {
            Origin::FilesIgnore => Origin::Dir(String::new()),
            other => other.clone(),
        },
        line: d.line,
        pattern: d.pattern.clone(),
    }))
}

fn decision(h: &Hit<'_>) -> Decision {
    let origin = match h.source {
        Source::Dir => Origin::Dir(String::from_utf8_lossy(h.dir).into_owned()),
        Source::InfoExclude => Origin::InfoExclude,
        Source::ExcludesFile => Origin::ExcludesFile,
        Source::FilesIgnore => Origin::FilesIgnore,
    };
    Decision {
        origin,
        line: h.pattern.line,
        pattern: git_form(&h.pattern),
    }
}

fn git_form(p: &PatternRef<'_>) -> Vec<u8> {
    let mut out = Vec::new();
    p.write_git_form(&mut out);
    out
}

/// git's side of one check, in the form it is compared in (module documentation).
#[derive(Clone, Copy, Debug)]
pub enum GitSide<'a> {
    /// `check-ignore -v -n`, against the matcher in [`Mode::Git`].
    Verbose(&'a Answer),
    /// Plain `check-ignore`: whether it listed the path as ignored.
    Plain(bool),
    /// `check-ignore -v -n`, against the matcher in [`Mode::NoGit`] reading the root's own `.gitignore`, with
    /// `files.ignore` set to `*`.
    OwnRoot(&'a Answer),
    /// `check-ignore -v -n`, against the matcher in [`Mode::NoGit`] told the root has no `.gitignore`, with
    /// `files.ignore` holding that file's lines ([`Tree::files_ignore_stand_in`]).
    StandIn(&'a Answer),
    /// `check-ignore -v -n` on a root `.gitignore` of the lines a `glob-list` can carry, against the matcher in
    /// [`Mode::NoGit`] told the root has none, with `files.ignore` holding those lines ([`Tree::glob_list_stand_in`]).
    GlobList(&'a Answer),
}

impl GitSide<'_> {
    /// Whether the matcher's answer `product` agrees: the same answer (source, line, pattern); for the plain form,
    /// git lists the path iff the matcher ignores it; for the two stand-ins, the same answer once `files.ignore`
    /// takes the root file's place ([`in_root_place`]), and never an answer that names the root file the matcher was
    /// told is absent.
    // spec: [F20 §4.4] (git's semantics), [40 §5.8] (without git, `files.ignore` in the root `.gitignore`'s place)
    #[must_use]
    pub fn agrees(self, product: &Answer) -> bool {
        match self {
            GitSide::Verbose(g) | GitSide::OwnRoot(g) => g == product,
            GitSide::Plain(listed) => listed == product.ignored(),
            GitSide::StandIn(g) | GitSide::GlobList(g) => {
                !product.names_root_file() && *g == in_root_place(product)
            }
        }
    }

    /// The owned form kept with a disagreement.
    fn saved(self) -> GitSaid {
        match self {
            GitSide::Verbose(a) => GitSaid::Verbose(a.clone()),
            GitSide::Plain(listed) => GitSaid::Plain(listed),
            GitSide::OwnRoot(a) => GitSaid::OwnRoot(a.clone()),
            GitSide::StandIn(a) => GitSaid::StandIn(a.clone()),
            GitSide::GlobList(a) => GitSaid::GlobList(a.clone()),
        }
    }
}

/// What git said about a path where the two sides disagree ([`GitSide`], owned).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GitSaid {
    /// [`GitSide::Verbose`].
    Verbose(Answer),
    /// [`GitSide::Plain`].
    Plain(bool),
    /// [`GitSide::OwnRoot`].
    OwnRoot(Answer),
    /// [`GitSide::StandIn`].
    StandIn(Answer),
    /// [`GitSide::GlobList`].
    GlobList(Answer),
}

/// The check a disagreement belongs to: the tree's label, `core.ignorecase`, the path and whether it is a directory.
#[derive(Clone, Copy, Debug)]
pub struct Place<'a> {
    /// The tree's label.
    pub tree: &'a str,
    /// `core.ignorecase`.
    pub ignore_case: bool,
    /// The path.
    pub path: &'a str,
    /// Whether it is a directory.
    pub is_dir: bool,
}

/// One path on which git and the matcher disagree.
#[derive(Clone, Debug)]
pub struct Disagreement {
    /// The tree's label.
    pub tree: String,
    /// `core.ignorecase`.
    pub ignore_case: bool,
    /// The path.
    pub path: String,
    /// Whether it is a directory.
    pub is_dir: bool,
    /// git's answer, in the form compared.
    pub git: GitSaid,
    /// The matcher's answer.
    pub product: Answer,
}

impl fmt::Display for Disagreement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let slash = if self.is_dir { "/" } else { "" };
        let (form, git) = match &self.git {
            GitSaid::Verbose(a) => ("-v", a.to_string()),
            GitSaid::Plain(true) => ("plain", "lists it as ignored".to_string()),
            GitSaid::Plain(false) => ("plain", "does not list it".to_string()),
            GitSaid::OwnRoot(a) => (
                "-v against the matcher without git reading the root .gitignore, files.ignore \"*\"",
                a.to_string(),
            ),
            GitSaid::StandIn(a) => (
                "-v against the matcher without git, files.ignore holding the root .gitignore's lines",
                a.to_string(),
            ),
            GitSaid::GlobList(a) => (
                "-v on the glob-list lines against the matcher without git, files.ignore holding them",
                a.to_string(),
            ),
        };
        write!(
            f,
            "[{}, core.ignorecase={}, {form}] {}{slash}: git {git}; moirai {}",
            self.tree, self.ignore_case, self.path, self.product
        )
    }
}

/// The checks of one comparison and how many agreed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tally {
    /// Checks made.
    pub checks: u64,
    /// Checks on which both sides agree.
    pub agree: u64,
}

impl Tally {
    /// Counts one check.
    pub fn add(&mut self, agrees: bool) {
        self.checks += 1;
        self.agree += u64::from(agrees);
    }
}

impl fmt::Display for Tally {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}/{} agree ({})",
            self.agree,
            self.checks,
            percent(self.agree, self.checks)
        )
    }
}

/// What a differential run saw.
#[derive(Clone, Debug, Default)]
pub struct Report {
    /// Trees checked.
    pub trees: u64,
    /// [`GitSide::Verbose`] checks (a path under one `core.ignorecase` value).
    pub verbose: Tally,
    /// [`GitSide::Plain`] checks.
    pub plain: Tally,
    /// [`GitSide::OwnRoot`] checks.
    pub own_root: Tally,
    /// [`GitSide::StandIn`] checks.
    pub stand_in: Tally,
    /// [`GitSide::GlobList`] checks; for a tree whose root lines a glob-list can all carry, these are its stand-in
    /// checks, counted here too.
    pub glob_list: Tally,
    /// Verbose checks of virtual files.
    pub virtual_checks: u64,
    /// Verbose checks of virtual files git answered with a pattern.
    pub virtual_matched: u64,
    /// Verbose checks git answered "ignored".
    pub ignored: u64,
    /// Verbose checks git answered with a negated pattern.
    pub reincluded: u64,
    /// Verbose checks no pattern matched.
    pub unmatched: u64,
    /// Verbose checks decided by a `.gitignore`.
    pub by_gitignore: u64,
    /// Verbose checks decided by `info/exclude`.
    pub by_info_exclude: u64,
    /// Verbose checks decided by `core.excludesFile`.
    pub by_excludes_file: u64,
    /// Agreeing verbose checks the matcher decided through an excluded ancestor directory.
    pub excluded_parent: u64,
    /// Verbose checks git decided by a directory-only pattern.
    pub dir_only: u64,
    /// Paths whose verbose answer from git differs between the two `core.ignorecase` values.
    pub case_dependent: u64,
    /// Every disagreement, counted.
    pub disagreement_total: u64,
    /// The first [`DETAIL_MAX`] disagreements.
    pub disagreements: Vec<Disagreement>,
    /// Every tree with a disagreement, counted.
    pub failing_tree_total: u64,
    /// The label and [`Tree::describe`] of the first [`DETAIL_MAX`] trees with a disagreement.
    pub failing_trees: Vec<(String, String)>,
}

impl Report {
    /// One paragraph of counts and agreement rates.
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "{} trees; -v: {}; plain: {}; without git: the root's own .gitignore {}, files.ignore in its place {}, \
             glob-list lines only {}; {} checks of virtual files, {} matched; git's answers: {} ignored, {} \
             re-included, {} unmatched; decided by a .gitignore {}, info/exclude {}, core.excludesFile {}; {} through \
             an excluded directory, {} by a directory-only pattern; {} paths depend on core.ignorecase; {} \
             disagreements",
            self.trees,
            self.verbose,
            self.plain,
            self.own_root,
            self.stand_in,
            self.glob_list,
            self.virtual_checks,
            self.virtual_matched,
            self.ignored,
            self.reincluded,
            self.unmatched,
            self.by_gitignore,
            self.by_info_exclude,
            self.by_excludes_file,
            self.excluded_parent,
            self.dir_only,
            self.case_dependent,
            self.disagreement_total
        )
    }

    /// Whether every check agreed.
    #[must_use]
    pub fn agrees(&self) -> bool {
        self.disagreement_total == 0
    }

    /// Compares git's side with the matcher's answer for one check ([`GitSide::agrees`]), counts it in that
    /// comparison's [`Tally`], keeps it as a [`Disagreement`] when they disagree (in detail while fewer than
    /// [`DETAIL_MAX`] are kept), and returns whether they agree.
    pub fn record(&mut self, place: &Place<'_>, git: GitSide<'_>, product: &Answer) -> bool {
        let agrees = git.agrees(product);
        let tally = match git {
            GitSide::Verbose(_) => &mut self.verbose,
            GitSide::Plain(_) => &mut self.plain,
            GitSide::OwnRoot(_) => &mut self.own_root,
            GitSide::StandIn(_) => &mut self.stand_in,
            GitSide::GlobList(_) => &mut self.glob_list,
        };
        tally.add(agrees);
        if !agrees {
            self.disagreement_total += 1;
            if self.disagreements.len() < DETAIL_MAX {
                self.disagreements.push(Disagreement {
                    tree: place.tree.to_string(),
                    ignore_case: place.ignore_case,
                    path: place.path.to_string(),
                    is_dir: place.is_dir,
                    git: git.saved(),
                    product: product.clone(),
                });
            }
        }
        agrees
    }

    /// Counts a tree with a disagreement, keeping its description while fewer than [`DETAIL_MAX`] are kept.
    fn failing_tree(&mut self, label: &str, tree: &Tree) {
        self.failing_tree_total += 1;
        if self.failing_trees.len() < DETAIL_MAX {
            self.failing_trees
                .push((label.to_string(), tree.describe()));
        }
    }

    fn note(&mut self, git: &Answer) {
        match &git.0 {
            None => self.unmatched += 1,
            Some(d) => {
                if d.negated() {
                    self.reincluded += 1;
                } else {
                    self.ignored += 1;
                }
                match d.origin {
                    Origin::Dir(_) => self.by_gitignore += 1,
                    Origin::InfoExclude => self.by_info_exclude += 1,
                    Origin::ExcludesFile => self.by_excludes_file += 1,
                    Origin::FilesIgnore => {}
                }
                if d.dir_only() {
                    self.dir_only += 1;
                }
            }
        }
    }
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{}", self.summary())?;
        for d in &self.disagreements {
            writeln!(f, "{d}")?;
        }
        more(
            f,
            self.disagreement_total,
            self.disagreements.len(),
            "disagreements",
        )?;
        for (label, tree) in &self.failing_trees {
            writeln!(f, "tree {label}:\n{tree}")?;
        }
        more(
            f,
            self.failing_tree_total,
            self.failing_trees.len(),
            "failing trees",
        )
    }
}

/// The line that says how many of `total` items past the `kept` ones a report counts only.
fn more(f: &mut fmt::Formatter<'_>, total: u64, kept: usize, what: &str) -> fmt::Result {
    let rest = total.saturating_sub(kept as u64);
    if rest > 0 {
        writeln!(f, "... and {rest} more {what}, counted only")?;
    }
    Ok(())
}

/// `n / d` as a percentage with two decimals (`-` when `d` is 0).
fn percent(n: u64, d: u64) -> String {
    if d == 0 {
        "-".into()
    } else {
        // Exact integer arithmetic: hundredths of a percent, rounded down.
        let bp = u128::from(n) * 10_000 / u128::from(d);
        format!("{}.{:02} %", bp / 100, bp % 100)
    }
}

/// The sources the matcher is given besides the `.gitignore` files below the root.
#[derive(Clone, Debug)]
pub enum Sources<'a> {
    /// With git ([`Mode::Git`]): the root's `.gitignore`, `info/exclude` (read from `work/.git/info/exclude` if it
    /// exists) and the `core.excludesFile` at this path, if given.
    Git {
        /// The `core.excludesFile`, if set.
        excludes: Option<&'a Path>,
    },
    /// Without git ([`Mode::NoGit`]): these `files.ignore` items, which stand in for the root's `.gitignore` when the
    /// matcher finds none.
    NoGit {
        /// The `files.ignore` items.
        files_ignore: &'a [&'a [u8]],
        /// Whether the matcher is told the root has no `.gitignore` even when it has one.
        hide_root: bool,
    },
}

/// The matcher's answers for `queries` in the work tree `work`, every source read back from the disk: each
/// `.gitignore` when the matcher asks for it, and the others as `sources` says.
///
/// # Errors
/// A file-system error, or a verdict that contradicts itself ([`product_answer`]).
pub fn product_side(
    work: &Path,
    sources: &Sources<'_>,
    queries: &[(String, bool)],
    case: Case,
) -> Result<Vec<(Answer, bool)>, String> {
    let read_opt = |p: &Path| -> Result<Option<PatternList>, String> {
        match fs::read(p) {
            Ok(b) => Ok(Some(PatternList::parse(&b))),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(format!("{}: {e}", p.display())),
        }
    };
    let (mode, hide_root) = match sources {
        Sources::Git { excludes } => {
            let info_exclude = read_opt(&work.join(".git").join("info").join("exclude"))?;
            let excludes_file = match excludes {
                Some(p) => read_opt(p)?,
                None => None,
            };
            let mode = Mode::Git {
                info_exclude,
                excludes_file,
            };
            (mode, false)
        }
        Sources::NoGit {
            files_ignore,
            hide_root,
        } => {
            let files_ignore = PatternList::from_items(files_ignore.iter());
            (Mode::NoGit { files_ignore }, *hide_root)
        }
    };
    let mut stack = IgnoreStack::new(mode, case);
    let mut answers = Vec::with_capacity(queries.len());
    for (path, is_dir) in queries {
        let verdict = stack.check(path.as_bytes(), *is_dir, |dir| {
            if hide_root && dir.is_empty() {
                return Ok(None);
            }
            let dir = std::str::from_utf8(dir)
                .map_err(|_| format!("the directory {} is not UTF-8", dir.escape_ascii()))?;
            read_opt(&work.join(join(dir, ".gitignore")))
        });
        let verdict = verdict.map_err(|e| format!("{path}: {e}"))?;
        answers.push(product_answer(&verdict).map_err(|e| format!("{path}: {e}"))?);
    }
    Ok(answers)
}

/// git's verbose answers for `paths` under `settings`.
fn git_side(
    repo: &Repo<'_>,
    paths: &[&str],
    settings: CheckIgnore<'_>,
) -> Result<Vec<Answer>, GitError> {
    let source = settings.excludes_source();
    repo.check_ignore(paths, settings)?
        .into_iter()
        .map(|m| git_answer(m, source.as_deref()))
        .collect::<Result<_, _>>()
        .map_err(GitError::Output)
}

/// Checks one tree (module documentation): writes it into `repo`'s work tree (and `excludes`), then for each value
/// of `core.ignorecase` compares git's verbose answers with the matcher's, with the matcher's without git when the
/// tree allows it ([`Tree::files_ignore_stand_in`]: the root's own file, the stand-in and the glob-list stand-in),
/// and for the values in `plain` git's plain form with the matcher's, adding everything to `report` under `label`.
///
/// # Errors
/// Virtual files that break [`Tree::virtual_file_problem`], or a failed git run or file-system step ([`GitError`]);
/// disagreements are not errors but go to the report.
pub fn check_tree(
    repo: &Repo<'_>,
    excludes: &Path,
    tree: &Tree,
    label: &str,
    plain: &[bool],
    report: &mut Report,
) -> Result<(), GitError> {
    if let Some(problem) = tree.virtual_files_problem() {
        return Err(GitError::Output(format!("{label}: {problem}")));
    }
    tree.write(repo.dir(), excludes)?;
    let queries = tree.paths();
    let paths: Vec<&str> = queries.iter().map(|(p, _)| p.as_str()).collect();
    let is_virtual: Vec<bool> = paths
        .iter()
        .map(|p| tree.virtual_files.iter().any(|v| v == p))
        .collect();
    let excludes = tree.excludes_file.is_some().then_some(excludes);
    let stand_in = tree.files_ignore_stand_in();
    let glob_list = tree.glob_list_stand_in();
    let glob_list_is_stand_in = glob_list == stand_in;
    let before = report.disagreement_total;
    let mut by_case: Vec<Vec<Answer>> = Vec::with_capacity(2);
    for ignore_case in [false, true] {
        let settings = CheckIgnore {
            ignore_case,
            excludes_file: excludes,
        };
        let git = git_side(repo, &paths, settings)?;
        let case = Case::from_insensitive(ignore_case);
        let product = product_side(repo.dir(), &Sources::Git { excludes }, &queries, case)
            .map_err(GitError::Output)?;
        let place = |i: usize| Place {
            tree: label,
            ignore_case,
            path: &queries[i].0,
            is_dir: queries[i].1,
        };
        for (i, (g, (p, through_parent))) in git.iter().zip(&product).enumerate() {
            report.note(g);
            if is_virtual[i] {
                report.virtual_checks += 1;
                report.virtual_matched += u64::from(g.0.is_some());
            }
            if report.record(&place(i), GitSide::Verbose(g), p) {
                report.excluded_parent += u64::from(*through_parent);
            }
        }
        if let Some(items) = &stand_in {
            let no_git = |files_ignore: &[&[u8]], hide_root: bool| {
                let sources = Sources::NoGit {
                    files_ignore,
                    hide_root,
                };
                product_side(repo.dir(), &sources, &queries, case).map_err(GitError::Output)
            };
            let own_root = no_git(OWN_ROOT_FILES_IGNORE, false)?;
            let hidden = no_git(items.as_slice(), true)?;
            for (i, (g, ((own, _), (stood, _)))) in
                git.iter().zip(own_root.iter().zip(&hidden)).enumerate()
            {
                report.record(&place(i), GitSide::OwnRoot(g), own);
                let agrees = report.record(&place(i), GitSide::StandIn(g), stood);
                if glob_list_is_stand_in {
                    report.glob_list.add(agrees);
                }
            }
        }
        if plain.contains(&ignore_case) {
            let listed = repo.check_ignore_plain(&paths, settings)?;
            let mut listed = listed.iter().peekable();
            for (i, (path, (p, _))) in paths.iter().zip(&product).enumerate() {
                let git_listed = listed.next_if(|l| l == path).is_some();
                report.record(&place(i), GitSide::Plain(git_listed), p);
            }
        }
        by_case.push(git);
    }
    if let [sensitive, insensitive] = by_case.as_slice() {
        report.case_dependent += sensitive
            .iter()
            .zip(insensitive)
            .filter(|(a, b)| a != b)
            .count() as u64;
    }
    if let Some(items) = glob_list.as_ref().filter(|_| !glob_list_is_stand_in) {
        // git reads a root `.gitignore` of exactly the lines a glob-list can carry, so its line k is item k.
        let mut root = Vec::new();
        for item in items {
            root.extend_from_slice(item);
            root.push(b'\n');
        }
        fs::write(repo.dir().join(".gitignore"), &root)?;
        let sources = Sources::NoGit {
            files_ignore: items,
            hide_root: true,
        };
        for ignore_case in [false, true] {
            let settings = CheckIgnore {
                ignore_case,
                excludes_file: None,
            };
            let git = git_side(repo, &paths, settings)?;
            let case = Case::from_insensitive(ignore_case);
            let hidden =
                product_side(repo.dir(), &sources, &queries, case).map_err(GitError::Output)?;
            for (i, (g, (p, _))) in git.iter().zip(&hidden).enumerate() {
                let place = Place {
                    tree: label,
                    ignore_case,
                    path: &queries[i].0,
                    is_dir: queries[i].1,
                };
                report.record(&place, GitSide::GlobList(g), p);
            }
        }
    }
    report.trees += 1;
    if report.disagreement_total > before {
        report.failing_tree(label, tree);
    }
    Ok(())
}

/// What a never-candidate differential run saw ([`check_names`]).
#[derive(Clone, Debug, Default)]
pub struct NameReport {
    /// Pattern lists checked.
    pub lists: u64,
    /// Names checked (one name against one list).
    pub checks: u64,
    /// Checks on which both sides agree.
    pub agree: u64,
    /// Checks git answered with a match.
    pub matched: u64,
    /// Every disagreement, counted.
    pub disagreement_total: u64,
    /// The first [`DETAIL_MAX`] disagreements, each with its list, name and both answers.
    pub disagreements: Vec<String>,
}

impl NameReport {
    /// One line of counts and the agreement rate.
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "{} pattern lists; {}/{} names agree ({}); {} matched; {} disagreements",
            self.lists,
            self.agree,
            self.checks,
            percent(self.agree, self.checks),
            self.matched,
            self.disagreement_total
        )
    }

    /// Whether every check agreed.
    #[must_use]
    pub fn agrees(&self) -> bool {
        self.disagreement_total == 0
    }

    /// Counts one name checked against `patterns` (under `label`): git's first matching pattern `git_first` and
    /// `matches_name_pattern`'s `first` (list indexes), and for [F20 §4.7.2]'s list `never_pattern`'s `row` (from 1).
    /// They agree when `first` is `git_first` and the row, if given, is `first + 1`. A disagreement is kept in detail
    /// while fewer than [`DETAIL_MAX`] are kept. Returns whether they agree.
    pub fn record(
        &mut self,
        label: &str,
        name: &str,
        patterns: &[&[u8]],
        git_first: Option<usize>,
        first: Option<usize>,
        row: Option<Option<usize>>,
    ) -> bool {
        let agrees = first == git_first && row.is_none_or(|r| r == first.map(|i| i + 1));
        self.checks += 1;
        self.matched += u64::from(git_first.is_some());
        if agrees {
            self.agree += 1;
        } else {
            self.disagreement_total += 1;
            if self.disagreements.len() < DETAIL_MAX {
                let show = |i: Option<usize>| match i.and_then(|i| patterns.get(i).map(|p| (i, p)))
                {
                    Some((i, p)) => format!("#{} \"{}\"", i + 1, p.escape_ascii()),
                    None => "no pattern".to_string(),
                };
                self.disagreements.push(format!(
                    "[{label}] {name}: git {}; matches_name_pattern {}{}",
                    show(git_first),
                    show(first),
                    row.map_or(String::new(), |r| format!("; never_pattern {r:?}"))
                ));
            }
        }
        agrees
    }
}

impl fmt::Display for NameReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{}", self.summary())?;
        for d in &self.disagreements {
            writeln!(f, "{d}")?;
        }
        more(
            f,
            self.disagreement_total,
            self.disagreements.len(),
            "disagreements",
        )
    }
}

/// Whether the never-candidate pattern `pattern` means the same as a gitignore line under `core.ignorecase = true`
/// matched against a name at the root: it is not empty and does not start with `!` (which negates) or `#` (a
/// comment), does not end in a space (which git trims), and holds no `/` (a path pattern), `[` (a bracket
/// expression), `\` (an escape), `00` (where git ends the line), CR (which git drops before a line end) or LF (a
/// line end). Under these, `*`, `?` and ASCII case folding mean what [F20 §4.7.1] says.
// spec: [F20 §4.7.1] (the pattern syntax, as a gitignore line under `core.ignorecase = true`)
#[must_use]
pub fn is_gitignore_safe_name_pattern(pattern: &[u8]) -> bool {
    !pattern.is_empty()
        && !pattern.starts_with(b"!")
        && !pattern.starts_with(b"#")
        && !pattern.ends_with(b" ")
        && !pattern
            .iter()
            .any(|&b| matches!(b, b'/' | b'[' | b'\\' | 0 | b'\r' | b'\n'))
}

/// The root `.gitignore` of [`check_names`]: `patterns` in reverse order, one per line, so that line k holds the
/// pattern of index n − k ([`list_index`]) and git's deciding pattern (the last match) is the list's first match.
#[must_use]
pub fn reversed_source(patterns: &[&[u8]]) -> Vec<u8> {
    let mut source = Vec::with_capacity(patterns.iter().map(|p| p.len() + 1).sum());
    for p in patterns.iter().rev() {
        source.extend_from_slice(p);
        source.push(b'\n');
    }
    source
}

/// The list index of the pattern git names in `m` for the [`reversed_source`] of `n` patterns (line k holds index
/// n − k), or `None` when no pattern matched.
///
/// # Errors
/// A message for a source other than the root `.gitignore`, or a line outside 1..=n.
pub fn list_index(m: Option<&IgnoreMatch>, n: usize) -> Result<Option<usize>, String> {
    match m {
        None => Ok(None),
        Some(m) => match usize::try_from(m.line) {
            Ok(line) if m.source == b".gitignore" && (1..=n).contains(&line) => Ok(Some(n - line)),
            _ => Err(format!(
                "check-ignore named {}:{} for a list of {n} patterns",
                m.source.escape_ascii(),
                m.line
            )),
        },
    }
}

/// Compares the never-candidate matching of `patterns` (in list order: the first match decides) with
/// `git check-ignore -v` under `core.ignorecase = true` over regular files named `names` at the root of `repo`'s work
/// tree (module documentation). With `list` set, `patterns` is [F20 §4.7.2]'s list and `never_pattern` is checked
/// too. Names must be valid file names, distinct under case folding, and other than `.gitignore`, which is checked
/// as a name as well.
///
/// # Errors
/// A pattern outside [`is_gitignore_safe_name_pattern`], or a failed git run or file-system step.
pub fn check_names(
    repo: &Repo<'_>,
    excludes: &Path,
    patterns: &[&[u8]],
    names: &[String],
    label: &str,
    list: bool,
    report: &mut NameReport,
) -> Result<(), GitError> {
    if let Some(bad) = patterns.iter().find(|p| !is_gitignore_safe_name_pattern(p)) {
        return Err(GitError::Output(format!(
            "the pattern {} does not mean the same in a .gitignore",
            bad.escape_ascii()
        )));
    }
    let tree = Tree {
        files: names.to_vec(),
        gitignores: vec![(String::new(), reversed_source(patterns))],
        ..Tree::default()
    };
    tree.write(repo.dir(), excludes)?;
    let queries = tree.paths();
    let paths: Vec<&str> = queries.iter().map(|(p, _)| p.as_str()).collect();
    let settings = CheckIgnore {
        ignore_case: true,
        excludes_file: None,
    };
    let git = repo.check_ignore(&paths, settings)?;
    for (name, m) in paths.iter().zip(git) {
        let git_first = list_index(m.as_ref(), patterns.len())
            .map_err(|e| GitError::Output(format!("{e}, for {name}")))?;
        let first = patterns
            .iter()
            .position(|p| matches_name_pattern(p, name.as_bytes()));
        let row = list.then(|| never_pattern(name.as_bytes()));
        report.record(label, name, patterns, git_first, first, row);
    }
    report.lists += 1;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(source: &str, line: u32, pattern: &str) -> Option<IgnoreMatch> {
        Some(IgnoreMatch {
            source: source.as_bytes().to_vec(),
            line,
            pattern: pattern.as_bytes().to_vec(),
        })
    }

    fn d(origin: Origin, line: u32, pattern: &str) -> Answer {
        Answer(Some(Decision {
            origin,
            line,
            pattern: pattern.as_bytes().to_vec(),
        }))
    }

    const PLACE: Place<'static> = Place {
        tree: "t1",
        ignore_case: true,
        path: "a/x.o",
        is_dir: false,
    };

    #[test]
    fn git_sources_map_to_origins() {
        let ex = Some("D:/t/excludes");
        let origin = |src: &str| {
            git_answer(m(src, 1, "x"), ex)
                .expect("a known source")
                .0
                .expect("a decision")
                .origin
        };
        assert_eq!(origin(".gitignore"), Origin::Dir(String::new()));
        assert_eq!(origin("a/b/.gitignore"), Origin::Dir("a/b".into()));
        assert_eq!(origin(".git/info/exclude"), Origin::InfoExclude);
        assert_eq!(origin("D:/t/excludes"), Origin::ExcludesFile);
        assert!(git_answer(m("elsewhere", 1, "x"), ex).is_err());
        assert!(git_answer(m("D:/t/excludes", 1, "x"), None).is_err());
        assert_eq!(git_answer(None, ex), Ok(Answer(None)));
    }

    #[test]
    fn answers_say_ignored_unless_negated() {
        let a = git_answer(m(".gitignore", 2, "!keep"), None).expect("known");
        assert!(!a.ignored());
        assert_eq!(a.to_string(), "re-included by .gitignore:2: \"!keep\"");
        let a = git_answer(m("s/.gitignore", 1, "out/"), None).expect("known");
        assert!(a.ignored() && a.0.as_ref().is_some_and(Decision::dir_only));
        assert_eq!(a.to_string(), "ignored by s/.gitignore:1: \"out/\"");
        assert!(!Answer(None).ignored());
    }

    #[test]
    fn verdicts_become_answers() {
        let list = PatternList::parse(b"*.log\n!k.log\nout/\n");
        let mut st = IgnoreStack::new(
            Mode::Git {
                info_exclude: None,
                excludes_file: None,
            },
            Case::Sensitive,
        );
        let mut check = |p: &str, d: bool| {
            let v = st
                .check(p.as_bytes(), d, |dir| -> Result<Option<PatternList>, ()> {
                    Ok(dir.is_empty().then(|| list.clone()))
                })
                .expect("no loader error");
            product_answer(&v).expect("a consistent verdict")
        };
        let (a, parent) = check("a.log", false);
        assert_eq!(a.to_string(), "ignored by .gitignore:1: \"*.log\"");
        assert!(!parent);
        let (a, _) = check("k.log", false);
        assert_eq!(a.to_string(), "re-included by .gitignore:2: \"!k.log\"");
        let (a, parent) = check("out/x", false);
        assert_eq!(a.to_string(), "ignored by .gitignore:3: \"out/\"");
        assert!(parent);
        assert_eq!(check("x", false), (Answer(None), false));
    }

    #[test]
    fn trees_list_their_paths_sorted() {
        let t = Tree {
            dirs: vec!["b".into(), "a".into()],
            files: vec!["a/x".into(), "c".into()],
            virtual_files: vec!["a/x*".into(), "c ".into()],
            gitignores: vec![(String::new(), b"x\n".to_vec()), ("a".into(), Vec::new())],
            info_exclude: Some(b"y\r\n\0z".to_vec()),
            excludes_file: None,
        };
        let paths = t.paths();
        let names: Vec<(&str, bool)> = paths.iter().map(|(p, d)| (p.as_str(), *d)).collect();
        assert_eq!(
            names,
            [
                (".gitignore", false),
                ("a", true),
                ("a/.gitignore", false),
                ("a/x", false),
                ("a/x*", false),
                ("b", true),
                ("c", false),
                ("c ", false)
            ]
        );
        let d = t.describe();
        assert!(
            d.contains("  .git/info/exclude:\n      1 | y\\r\n      2 | \\x00z\n"),
            "{d}"
        );
        assert!(d.contains("  a/.gitignore:\n      1 | \n"), "{d}");
        assert!(d.contains("  virtual files: [\"a/x*\", \"c \"]\n"), "{d}");
    }

    #[test]
    fn virtual_files_are_names_no_real_path_answers_for() {
        let t = Tree {
            dirs: vec!["d".into(), "sp".into()],
            files: vec!["a".into(), "d/k".into()],
            gitignores: vec![(String::new(), Vec::new())],
            ..Tree::default()
        };
        for ok in ["a*", "?", "d/k ", "d/k.", "a  ", "x?y", "é?", ".gitignore "] {
            assert_eq!(t.virtual_file_problem(ok), None, "{ok:?}");
        }
        for bad in [
            "", "e/x*", "a:b", ":a", "a\\b", "a\"", "a<", "a>", "a|", "a\tb", "A", "D", "sp ",
            "SP.", " ", "..", "nul ", "Con.txt.",
        ] {
            assert!(t.virtual_file_problem(bad).is_some(), "{bad:?}");
        }
        let mut t2 = t.clone();
        t2.virtual_files = vec!["a*".into(), "q?".into()];
        assert_eq!(t2.virtual_files_problem(), None);
        t2.virtual_files.push("a*".into());
        assert!(t2.virtual_files_problem().is_some());
    }

    #[test]
    fn the_files_ignore_stand_in_is_the_root_file_without_globals() {
        let root = b"\xef\xbb\xbfa\r\n\n# c\nb/ \n!c";
        let mut t = Tree {
            gitignores: vec![
                ("d".into(), b"x\n".to_vec()),
                (String::new(), root.to_vec()),
            ],
            ..Tree::default()
        };
        let items = t.files_ignore_stand_in().expect("a root file, no globals");
        assert_eq!(items, [&b"a\r"[..], b"", b"# c", b"b/ ", b"!c"]);
        // The items give the patterns and line numbers git reads from the file.
        assert_eq!(
            PatternList::from_items(items.iter()),
            PatternList::parse(root)
        );
        // A glob-list carries only trimmed, non-empty items without control bytes.
        assert_eq!(
            t.glob_list_stand_in().expect("a stand-in"),
            [&b"# c"[..], b"!c"]
        );
        // A final line end starts no line; an empty file has none.
        let none: Vec<&[u8]> = Vec::new();
        for (bytes, items) in [
            (&b"a\n\n"[..], vec![&b"a"[..], &b""[..]]),
            (&b"a\n"[..], vec![&b"a"[..]]),
            (&b"\xef\xbb\xbf\n"[..], none.clone()),
            (&b""[..], none),
        ] {
            t.gitignores[1].1 = bytes.to_vec();
            assert_eq!(t.files_ignore_stand_in(), Some(items));
        }
        t.excludes_file = Some(Vec::new());
        assert_eq!(t.files_ignore_stand_in(), None);
        assert_eq!(t.glob_list_stand_in(), None);
        t.excludes_file = None;
        t.info_exclude = Some(Vec::new());
        assert_eq!(t.files_ignore_stand_in(), None);
        t.info_exclude = None;
        t.gitignores.pop();
        assert_eq!(t.files_ignore_stand_in(), None);
    }

    #[test]
    fn glob_list_items_are_trimmed_non_empty_and_without_commas() {
        for ok in [
            &b"target/"[..],
            b"!keep",
            b"# c",
            b"a\\ b",
            b"x\ty",
            "é*".as_bytes(),
            b"a\\",
        ] {
            assert!(glob_list_can_carry(ok), "{}", ok.escape_ascii());
        }
        for bad in [
            &b""[..],
            b" a",
            b"a ",
            b"\ta",
            b"a\t",
            b"a,b",
            b"a\r",
            b"a\0b",
            b"a\nb",
            b"a\x7f",
            b"\xef\xbb\xbfa",
            b"\xff",
        ] {
            assert!(!glob_list_can_carry(bad), "{}", bad.escape_ascii());
        }
    }

    #[test]
    fn stand_in_answers_take_the_root_files_place() {
        assert_eq!(
            in_root_place(&d(Origin::FilesIgnore, 2, "!x")),
            d(Origin::Dir(String::new()), 2, "!x")
        );
        assert_eq!(
            in_root_place(&d(Origin::Dir("a".into()), 2, "!x")),
            d(Origin::Dir("a".into()), 2, "!x")
        );
        assert_eq!(in_root_place(&Answer(None)), Answer(None));
    }

    #[test]
    fn each_comparison_has_its_agreement_rule() {
        let root = d(Origin::Dir(String::new()), 1, "x");
        let files_ignore = d(Origin::FilesIgnore, 1, "x");
        let nested = d(Origin::Dir("a".into()), 1, "x");
        let negated = d(Origin::Dir(String::new()), 2, "!x");
        let none = Answer(None);
        // With git, and reading the root's own file: the same answer, never files.ignore.
        for side in [GitSide::Verbose(&root), GitSide::OwnRoot(&root)] {
            assert!(side.agrees(&root));
            assert!(!side.agrees(&files_ignore));
            assert!(!side.agrees(&nested));
            assert!(!side.agrees(&none));
        }
        // The stand-ins: files.ignore in the root file's place, and never the hidden root file itself.
        for side in [GitSide::StandIn(&root), GitSide::GlobList(&root)] {
            assert!(side.agrees(&files_ignore));
            assert!(!side.agrees(&root));
            assert!(!side.agrees(&none));
        }
        assert!(GitSide::StandIn(&nested).agrees(&nested));
        assert!(GitSide::StandIn(&none).agrees(&none));
        // The plain form: listed iff ignored.
        assert!(GitSide::Plain(true).agrees(&root));
        assert!(GitSide::Plain(false).agrees(&negated));
        assert!(GitSide::Plain(false).agrees(&none));
        assert!(!GitSide::Plain(true).agrees(&negated));
        assert!(!GitSide::Plain(false).agrees(&root));
    }

    #[test]
    fn a_wrong_product_answer_is_a_disagreement() {
        let mut r = Report::default();
        let git = git_answer(m("a/.gitignore", 3, "*.o"), None).expect("known");
        assert!(r.record(&PLACE, GitSide::Verbose(&git), &git.clone()));
        assert!(r.agrees());
        let wrong = d(Origin::Dir("a".into()), 2, "*.o");
        assert!(!r.record(&PLACE, GitSide::Verbose(&git), &wrong));
        assert!(!r.record(
            &Place {
                is_dir: true,
                path: "d",
                ignore_case: false,
                ..PLACE
            },
            GitSide::Plain(true),
            &Answer(None)
        ));
        let root = d(Origin::Dir(String::new()), 1, "x");
        assert!(!r.record(&PLACE, GitSide::StandIn(&root), &root));
        assert!(r.record(
            &PLACE,
            GitSide::GlobList(&root),
            &d(Origin::FilesIgnore, 1, "x")
        ));
        assert!(!r.record(
            &PLACE,
            GitSide::OwnRoot(&Answer(None)),
            &d(Origin::FilesIgnore, 1, "*")
        ));
        assert!(!r.agrees());
        assert_eq!(r.disagreement_total, 4);
        assert_eq!(
            (r.verbose, r.plain, r.stand_in, r.glob_list, r.own_root),
            (
                Tally {
                    checks: 2,
                    agree: 1
                },
                Tally {
                    checks: 1,
                    agree: 0
                },
                Tally {
                    checks: 1,
                    agree: 0
                },
                Tally {
                    checks: 1,
                    agree: 1
                },
                Tally {
                    checks: 1,
                    agree: 0
                }
            )
        );
        let text = r.to_string();
        assert!(text.contains(
            "[t1, core.ignorecase=true, -v] a/x.o: git ignored by a/.gitignore:3: \"*.o\"; moirai ignored by a/.gitignore:2: \"*.o\""
        ), "{text}");
        assert!(text.contains(
            "[t1, core.ignorecase=false, plain] d/: git lists it as ignored; moirai no pattern (not ignored)"
        ), "{text}");
        assert!(text.contains(
            "files.ignore holding the root .gitignore's lines] a/x.o: git ignored by .gitignore:1: \"x\"; moirai ignored by .gitignore:1: \"x\""
        ), "{text}");
        assert!(r.summary().ends_with("4 disagreements"));
    }

    #[test]
    fn reports_keep_a_bounded_detail() {
        let mut r = Report::default();
        let git = d(Origin::Dir(String::new()), 1, "x");
        let t = Tree::default();
        for _ in 0..DETAIL_MAX + 5 {
            r.record(&PLACE, GitSide::Verbose(&git), &Answer(None));
            r.failing_tree("t1", &t);
        }
        assert_eq!(r.disagreement_total, DETAIL_MAX as u64 + 5);
        assert_eq!(r.disagreements.len(), DETAIL_MAX);
        assert_eq!(r.failing_trees.len(), DETAIL_MAX);
        let text = r.to_string();
        assert!(
            text.contains("... and 5 more disagreements, counted only\n"),
            "{text}"
        );
        assert!(
            text.contains("... and 5 more failing trees, counted only\n"),
            "{text}"
        );
        let mut n = NameReport::default();
        for _ in 0..DETAIL_MAX + 1 {
            n.record("l", "a", &[b"a"], Some(0), None, None);
        }
        assert_eq!(n.disagreements.len(), DETAIL_MAX);
        assert!(
            n.to_string()
                .ends_with("... and 1 more disagreements, counted only\n")
        );
    }

    #[test]
    fn percentages_round_down() {
        assert_eq!(percent(0, 0), "-");
        assert_eq!(percent(2, 3), "66.66 %");
        assert_eq!(percent(5, 5), "100.00 %");
        let mut r = Report::default();
        r.note(&d(Origin::Dir("a".into()), 3, "*.o"));
        r.note(&Answer(None));
        assert_eq!((r.ignored, r.unmatched, r.by_gitignore), (1, 1, 1));
    }

    #[test]
    fn gitignore_safe_name_patterns_exclude_every_gitignore_syntax() {
        for ok in [
            &b"*.tmp"[..],
            b"~$*",
            b".#*",
            b"sed??????",
            b"a!",
            b"a#",
            b" a",
            b"a\t",
            "é?".as_bytes(),
        ] {
            assert!(is_gitignore_safe_name_pattern(ok), "{}", ok.escape_ascii());
        }
        for bad in [
            &b""[..],
            b"!a",
            b"#a",
            b"a ",
            b"a/b",
            b"[a]",
            b"a\\b",
            b"a\0",
            b"a\rb",
            b"a\nb",
        ] {
            assert!(
                !is_gitignore_safe_name_pattern(bad),
                "{}",
                bad.escape_ascii()
            );
        }
    }

    #[test]
    fn line_k_of_the_reversed_source_is_index_n_minus_k() {
        let patterns: [&[u8]; 3] = [b"p0", b"p1", b"p2"];
        let source = reversed_source(&patterns);
        assert_eq!(source, b"p2\np1\np0\n");
        for (k, line) in source.split(|&b| b == b'\n').take(3).enumerate() {
            let k = u32::try_from(k + 1).expect("small");
            let i = list_index(m(".gitignore", k, "p").as_ref(), patterns.len())
                .expect("a line of the list")
                .expect("a match");
            assert_eq!(line, patterns[i], "line {k}");
        }
        assert_eq!(list_index(None, 3), Ok(None));
        for bad in [
            m(".gitignore", 0, "p"),
            m(".gitignore", 4, "p"),
            m("x/.gitignore", 1, "p"),
        ] {
            assert!(list_index(bad.as_ref(), 3).is_err());
        }
    }

    #[test]
    fn name_reports_count_disagreements() {
        let patterns: [&[u8]; 2] = [b"*.tmp", b"*~"];
        let mut r = NameReport::default();
        assert!(r.record("l", "a.tmp", &patterns, Some(0), Some(0), Some(Some(1))));
        assert!(r.record("l", "x", &patterns, None, None, Some(None)));
        // Each wrong answer on its own is a disagreement.
        assert!(!r.record("l", "a.tmp", &patterns, Some(0), Some(1), None));
        assert!(!r.record("l", "a.tmp", &patterns, Some(0), Some(0), Some(Some(2))));
        assert!(!r.record("l", "a.tmp", &patterns, None, Some(0), None));
        assert_eq!(
            (r.checks, r.agree, r.matched, r.disagreement_total),
            (5, 2, 3, 3)
        );
        assert_eq!(
            r.disagreements[0],
            "[l] a.tmp: git #1 \"*.tmp\"; matches_name_pattern #2 \"*~\""
        );
        assert_eq!(
            r.disagreements[1],
            "[l] a.tmp: git #1 \"*.tmp\"; matches_name_pattern #1 \"*.tmp\"; never_pattern Some(2)"
        );
        assert!(!r.agrees());
    }

    #[test]
    fn the_matcher_reads_its_sources_from_the_disk() {
        let dir = std::env::temp_dir().join(format!(
            "moirai-replay-ignorediff-sources-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        let work = dir.join("w");
        fs::create_dir_all(work.join(".git")).expect("scratch");
        let excludes = dir.join("excludes");
        let t = Tree {
            dirs: vec!["n".into()],
            files: vec!["x".into(), "n/x".into(), "q".into()],
            gitignores: vec![
                (String::new(), b"x\n".to_vec()),
                ("n".into(), b"!x\n".to_vec()),
            ],
            excludes_file: Some(b"q\n".to_vec()),
            ..Tree::default()
        };
        let queries = t.paths();
        let shown = |s: &Sources<'_>| -> Result<Vec<String>, String> {
            Ok(product_side(&work, s, &queries, Case::Sensitive)?
                .iter()
                .map(|(a, _)| a.to_string())
                .collect())
        };
        let outcome = t
            .write(&work, &excludes)
            .map_err(|e| e.to_string())
            .and_then(|()| {
                let with_git = shown(&Sources::Git {
                    excludes: Some(&excludes),
                })?;
                let hidden = shown(&Sources::NoGit {
                    files_ignore: &[b"y", b"x"],
                    hide_root: true,
                })?;
                let own_root = shown(&Sources::NoGit {
                    files_ignore: &[b"*"],
                    hide_root: false,
                })?;
                Ok((with_git, hidden, own_root))
            });
        let _ = fs::remove_dir_all(&dir);
        let (with_git, hidden, own_root) = outcome.expect("the sources are read");
        // .gitignore, n, n/.gitignore, n/x, q, x
        assert_eq!(
            with_git,
            [
                "no pattern (not ignored)",
                "no pattern (not ignored)",
                "no pattern (not ignored)",
                "re-included by n/.gitignore:1: \"!x\"",
                "ignored by core.excludesFile:1: \"q\"",
                "ignored by .gitignore:1: \"x\"",
            ]
        );
        // Without git the hidden root file is not read and nothing but the `.gitignore` files and `files.ignore`
        // counts.
        assert_eq!(hidden[4], "no pattern (not ignored)");
        assert_eq!(hidden[5], "ignored by files.ignore:2: \"x\"");
        assert_eq!(hidden[3], with_git[3]);
        // With the root's own file read, `files.ignore` is never used.
        assert_eq!(own_root[4], "no pattern (not ignored)");
        assert_eq!(own_root[5], with_git[5]);
    }

    #[test]
    fn trees_are_written_over_the_previous_one() {
        let dir =
            std::env::temp_dir().join(format!("moirai-replay-ignorediff-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let work = dir.join("w");
        fs::create_dir_all(work.join(".git")).expect("scratch");
        let excludes = dir.join("excludes");
        let first = Tree {
            dirs: vec!["old".into()],
            files: vec!["old/f".into()],
            gitignores: vec![(String::new(), b"f\n".to_vec())],
            info_exclude: Some(b"i\n".to_vec()),
            excludes_file: Some(b"e\n".to_vec()),
            ..Tree::default()
        };
        let second = Tree {
            dirs: vec!["n".into()],
            files: vec!["n/g".into()],
            virtual_files: vec!["n/g ".into(), "n/h*".into()],
            gitignores: vec![("n".into(), b"g\n".to_vec())],
            ..Tree::default()
        };
        let outcome = first.write(&work, &excludes).and_then(|()| {
            let wrote = fs::read(work.join(".git/info/exclude"))? == b"i\n"
                && fs::read(&excludes)? == b"e\n"
                && fs::read(work.join(".gitignore"))? == b"f\n";
            second.write(&work, &excludes)?;
            let names: Vec<_> = fs::read_dir(work.join("n"))?
                .map(|e| e.map(|e| e.file_name()))
                .collect::<io::Result<_>>()?;
            Ok((wrote, names.len()))
        });
        let state = (
            work.join("old").exists(),
            work.join(".gitignore").exists(),
            work.join(".git/info/exclude").exists(),
            excludes.exists(),
            fs::read(work.join("n/.gitignore")).ok(),
            work.join("n/g").is_file(),
        );
        let _ = fs::remove_dir_all(&dir);
        // The virtual files were not written: `n` holds `.gitignore` and `g` only.
        assert_eq!(outcome.expect("the trees are written"), (true, 2));
        assert_eq!(
            state,
            (false, false, false, false, Some(b"g\n".to_vec()), true)
        );
    }
}
