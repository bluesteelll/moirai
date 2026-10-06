//! The ignore matcher of candidate eligibility ([F20 §4.4]) and the never-candidate names ([F20 §4.7]).
//!
//! # The ignore matcher
//!
//! A path q is **ignored** iff q is not tracked (q ∉ τ(H), or the tree has no git) and the matcher matches q or one
//! of its ancestor directories ([F20 §4.4]); an ignored path is never a candidate ([40 §4.1] P6). The tracked test is
//! the caller's: this module decides only what the patterns say. The matcher has git's semantics, the ones
//! `git check-ignore` applies to an untracked path:
//!
//! - **Sources** ([`Mode`]). With git: the `.gitignore` of every directory from the root down to the path's parent,
//!   then `$GIT_DIR/info/exclude`, then `core.excludesFile`. Without git ([40 §5.8]): the `.gitignore` files only,
//!   and, when the root directory has none, the `files.ignore` patterns in its place ([CFG §10.6]; default
//!   `target/`, `node_modules/`, `build/`, [`FILES_IGNORE_DEFAULT`]).
//! - **Precedence.** The deepest `.gitignore` first, up to the root's, then `info/exclude`, then `core.excludesFile`;
//!   within one source the last matching pattern decides ([`pattern`] gives the line syntax). The first source with a
//!   match decides: a positive pattern ignores, a negated (`!`) pattern re-includes.
//! - **Excluded directories.** Before a directory's own `.gitignore` is read, the directory is matched as a
//!   directory against the sources above it. If it is ignored, so is everything under it, whatever a deeper pattern
//!   says ("it is not possible to re-include a file if a parent directory of that file is excluded"), and its
//!   `.gitignore` is never read.
//! - **Case** ([`Case`]). With git, `core.ignorecase` decides; without git, the root directory's case equivalence
//!   ([OS/project §4.5] `case_equivalent`, which `VolumeCaps` gives a first value for). Folding is ASCII only, as in
//!   git, and git's exceptions hold: a byte after `\` and a member of a bracket expression still compare unfolded.
//! - **Work.** Results equal git's, but in polynomial time and linear memory: git's glob matcher is exponential on
//!   some patterns (nested `**/`), and this one, once a match runs long, decides it by a simulation that takes about
//!   |pattern| × |path| steps and holds two bit sets linear in the pattern's length.
//!
//! # Input
//!
//! The module opens nothing (PLAN §2.1, GT20 (d)). The caller reads each source and passes its bytes, whole
//! ([`PatternList::parse`]) or in chunks ([`PatternParser`]); a `.gitignore` is asked for only when the matcher needs
//! it, through the loader of [`IgnoreStack::check`]. A path is root-relative with `/` separators and no empty, `.` or
//! `..` segment ([OS/path], [80 §2.10] P1); `is_dir` says whether it names a directory (a symbolic link is not one,
//! as git's `lstat` decides). Any other byte string is accepted and gives some answer without a panic.
//!
//! What the caller supplies, per git's rules: an in-tree `.gitignore` that is a symbolic link is not read (git opens
//! it with `O_NOFOLLOW`; `info/exclude` and `core.excludesFile` are followed); `core.excludesFile` unset means
//! `$XDG_CONFIG_HOME/git/ignore` (default `$HOME/.config/git/ignore`), and a source that cannot be read contributes
//! nothing (git warns). With git, a `.gitignore` absent from the worktree whose index entry is marked skip-worktree
//! (a sparse checkout) is read from its index blob, as git does; WP-74's `--no-index` differential cannot see that
//! case.
//!
//! # For WP-74's differential
//!
//! [`IgnoreStack::check`] answers what `git check-ignore -v --no-index` answers for one path: [`Verdict::ignored`]
//! and the deciding pattern ([`Hit`]: source, directory, line, and the text [`PatternRef::write_git_form`] prints as
//! git does). Where git prints a negated match (with `-v`), the verdict carries the hit with `ignored` false.

mod never;
pub mod pattern;
mod wild;

pub use never::{
    NeverCandidate, is_cloud_conflict_copy, is_old_name_plus_suffix, last_component,
    matches_name_pattern, never_candidate, never_pattern,
};
pub use pattern::{PATTERN_FILE_MAX, PatternList, PatternParser, PatternRef};

/// The default of `files.ignore` ([CFG §10.6], [40 §5.8]): the gitignore patterns used without git when the root
/// directory has no `.gitignore`.
pub const FILES_IGNORE_DEFAULT: [&[u8]; 3] = [b"target/", b"node_modules/", b"build/"];

/// The name of the per-directory pattern file.
pub const GITIGNORE: &[u8] = b".gitignore";

/// How patterns compare letters.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum Case {
    /// Bytes compare exactly (`core.ignorecase = false`, git's default).
    #[default]
    Sensitive,
    /// ASCII letters compare without case (`core.ignorecase = true`; a case-insensitive root directory without git).
    Insensitive,
}

impl Case {
    /// [`Case::Insensitive`] iff `insensitive`.
    #[must_use]
    pub const fn from_insensitive(insensitive: bool) -> Case {
        if insensitive {
            Case::Insensitive
        } else {
            Case::Sensitive
        }
    }
}

/// The sources besides the per-directory `.gitignore` files ([F20 §4.4], [40 §5.8]).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Mode {
    /// The tree has git.
    Git {
        /// `$GIT_DIR/info/exclude` (of the common directory), if it exists.
        info_exclude: Option<PatternList>,
        /// `core.excludesFile`, if set and readable.
        excludes_file: Option<PatternList>,
    },
    /// The tree has no git.
    NoGit {
        /// The `files.ignore` patterns ([`PatternList::from_items`] of the key's items), used in place of the root's
        /// `.gitignore` when the root has none.
        files_ignore: PatternList,
    },
}

/// Which source a deciding pattern came from.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Source {
    /// The `.gitignore` of [`Hit::dir`].
    Dir,
    /// `files.ignore`, standing in for the root's `.gitignore` (no git).
    FilesIgnore,
    /// `$GIT_DIR/info/exclude`.
    InfoExclude,
    /// `core.excludesFile`.
    ExcludesFile,
}

/// The pattern that decided a verdict.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Hit<'a> {
    /// Its source.
    pub source: Source,
    /// The directory of a [`Source::Dir`] file (root-relative, empty for the root); empty for the other sources.
    pub dir: &'a [u8],
    /// The pattern.
    pub pattern: PatternRef<'a>,
    /// `Some(n)` when the pattern matched the ancestor directory `path[..n]` rather than the path itself; the path is
    /// then ignored as part of that directory.
    pub ancestor: Option<usize>,
}

/// The matcher's answer for one path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Verdict<'a> {
    /// The path is ignored (when untracked).
    pub ignored: bool,
    /// The deciding pattern: positive when `ignored`, negated when a `!` pattern re-included the path; `None` when no
    /// pattern matched.
    pub hit: Option<Hit<'a>>,
}

/// The patterns of one directory level.
#[derive(Clone, Debug)]
enum LevelList {
    /// No `.gitignore` (with git).
    Absent,
    /// The directory's `.gitignore`.
    Own(PatternList),
    /// `files.ignore` in place of the root's `.gitignore` (no git).
    FilesIgnore,
}

/// One directory of the current chain: `dir[..end]` is its path (0 for the root).
#[derive(Clone, Debug)]
struct Level {
    end: usize,
    list: LevelList,
}

/// Where a match was found: a level (by index) or a global source.
#[derive(Clone, Copy, Debug)]
enum Found {
    Level(usize, usize),
    InfoExclude(usize),
    ExcludesFile(usize),
}

/// The ignore matcher of one tree ([F20 §4.4]).
///
/// It keeps the pattern lists of one chain of directories, from the root down, like git's exclude stack: a check
/// keeps the levels that are ancestors of the path's parent, drops the others, and loads the missing ones. Checking
/// the paths of a directory-first walk in order therefore reads each `.gitignore` once. Not shared between threads;
/// keep one per walk.
#[derive(Clone, Debug)]
pub struct IgnoreStack {
    mode: Mode,
    case: Case,
    levels: Vec<Level>,
    dir: Vec<u8>,
}

impl IgnoreStack {
    /// A matcher with the given sources and case rule, holding no directory yet.
    #[must_use]
    pub fn new(mode: Mode, case: Case) -> IgnoreStack {
        IgnoreStack {
            mode,
            case,
            levels: Vec::new(),
            dir: Vec::new(),
        }
    }

    /// The case rule.
    #[must_use]
    pub const fn case(&self) -> Case {
        self.case
    }

    /// Forgets every loaded `.gitignore` (after one of them changed).
    pub fn clear(&mut self) {
        self.levels.clear();
        self.dir.clear();
    }

    /// The number of directory levels held.
    #[must_use]
    pub fn depth(&self) -> usize {
        self.levels.len()
    }

    /// The heap bytes the matcher holds.
    #[must_use]
    pub fn heap_bytes(&self) -> usize {
        let globals = match &self.mode {
            Mode::Git {
                info_exclude,
                excludes_file,
            } => {
                info_exclude.as_ref().map_or(0, PatternList::heap_bytes)
                    + excludes_file.as_ref().map_or(0, PatternList::heap_bytes)
            }
            Mode::NoGit { files_ignore } => files_ignore.heap_bytes(),
        };
        let levels: usize = self
            .levels
            .iter()
            .map(|l| match &l.list {
                LevelList::Own(list) => list.heap_bytes(),
                LevelList::Absent | LevelList::FilesIgnore => 0,
            })
            .sum();
        globals + levels + self.levels.capacity() * size_of::<Level>() + self.dir.capacity()
    }

    /// Decides whether `path` is ignored ([F20 §4.4]), as `git check-ignore --no-index` does for an untracked path.
    ///
    /// `load(dir)` returns the parsed `.gitignore` of the root-relative directory `dir` (empty for the root), or
    /// `None` when it has none; it is called at most once per directory of the path's chain that the matcher does
    /// not hold yet, never for a directory found ignored, and in root-to-leaf order.
    ///
    /// # Errors
    /// The loader's error, returned at once; the matcher then holds the directories loaded before it.
    // spec: [F20 §4.4] (git's precedence: nested `.gitignore`, `info/exclude`, `core.excludesFile`; excluded parents)
    // spec: [40 §5.8] (without git: the `.gitignore` files, else `files.ignore`)
    pub fn check<E, L>(&mut self, path: &[u8], is_dir: bool, mut load: L) -> Result<Verdict<'_>, E>
    where
        L: FnMut(&[u8]) -> Result<Option<PatternList>, E>,
    {
        let name_start = path.iter().rposition(|&b| b == b'/').map_or(0, |i| i + 1);
        // Keep the levels on the chain of the path's parent directory.
        while let Some(top) = self.levels.last() {
            let e = top.end;
            if e == 0 || (e < name_start && path[e] == b'/' && path[..e] == self.dir[..e]) {
                break;
            }
            self.levels.pop();
        }
        if let Some(top) = self.levels.last() {
            self.dir.truncate(top.end);
        } else {
            self.dir.clear();
            let list = load(b"")?;
            self.push(0, list);
        }
        // Load the missing directories, each matched first against the levels above it.
        let mut pos = match self.levels.last() {
            Some(top) if top.end > 0 => top.end + 1,
            _ => 0,
        };
        while pos < name_start {
            let s = pos
                + path[pos..name_start]
                    .iter()
                    .position(|&b| b == b'/')
                    .unwrap_or(name_start - 1 - pos);
            if s == 0 {
                // A leading `/`: the empty first segment is the root itself.
                pos = 1;
                continue;
            }
            if let Some(found) = self.find(&path[..s], pos, true)
                && !self.hit(found, None).is_none_or(|h| h.pattern.negated)
            {
                return Ok(Verdict {
                    ignored: true,
                    hit: self.hit(found, Some(s)),
                });
            }
            let list = load(&path[..s])?;
            self.dir.extend_from_slice(&path[self.dir.len()..s]);
            self.push(s, list);
            pos = s + 1;
        }
        let hit = self
            .find(path, name_start, is_dir)
            .and_then(|f| self.hit(f, None));
        Ok(Verdict {
            ignored: hit.is_some_and(|h| !h.pattern.negated),
            hit,
        })
    }

    /// Adds the level of the directory `path[..end]` with its `.gitignore` (`None`: it has none).
    // spec: [40 §5.8] (without git, `files.ignore` stands in for the root directory's `.gitignore` when it has none)
    fn push(&mut self, end: usize, list: Option<PatternList>) {
        let list = match list {
            Some(l) => LevelList::Own(l),
            None if end == 0 && matches!(self.mode, Mode::NoGit { .. }) => LevelList::FilesIgnore,
            None => LevelList::Absent,
        };
        self.levels.push(Level { end, list });
    }

    /// The first source, in precedence order, whose last matching pattern matches `path` (its last component at
    /// `name_start`).
    // spec: [F20 §4.4] (git's precedence: the deepest `.gitignore`, then `info/exclude`, then `core.excludesFile`)
    fn find(&self, path: &[u8], name_start: usize, is_dir: bool) -> Option<Found> {
        for (li, level) in self.levels.iter().enumerate().rev() {
            let list = match &level.list {
                LevelList::Absent => continue,
                LevelList::Own(list) => list,
                LevelList::FilesIgnore => match &self.mode {
                    Mode::NoGit { files_ignore } => files_ignore,
                    Mode::Git { .. } => continue,
                },
            };
            if let Some(i) = list.last_match(path, level.end, name_start, is_dir, self.case) {
                return Some(Found::Level(li, i));
            }
        }
        if let Mode::Git {
            info_exclude,
            excludes_file,
        } = &self.mode
        {
            if let Some(i) = info_exclude
                .as_ref()
                .and_then(|l| l.last_match(path, 0, name_start, is_dir, self.case))
            {
                return Some(Found::InfoExclude(i));
            }
            if let Some(i) = excludes_file
                .as_ref()
                .and_then(|l| l.last_match(path, 0, name_start, is_dir, self.case))
            {
                return Some(Found::ExcludesFile(i));
            }
        }
        None
    }

    /// The [`Hit`] of a match [`find`](IgnoreStack::find) returned (always `Some` for one).
    fn hit(&self, found: Found, ancestor: Option<usize>) -> Option<Hit<'_>> {
        let (source, dir, list, i): (Source, &[u8], &PatternList, usize) = match found {
            Found::Level(li, i) => {
                let level = self.levels.get(li)?;
                match (&level.list, &self.mode) {
                    (LevelList::Own(list), _) => (Source::Dir, &self.dir[..level.end], list, i),
                    (LevelList::FilesIgnore, Mode::NoGit { files_ignore }) => {
                        (Source::FilesIgnore, &[], files_ignore, i)
                    }
                    _ => return None,
                }
            }
            Found::InfoExclude(i) => match &self.mode {
                Mode::Git {
                    info_exclude: Some(list),
                    ..
                } => (Source::InfoExclude, &[], list, i),
                _ => return None,
            },
            Found::ExcludesFile(i) => match &self.mode {
                Mode::Git {
                    excludes_file: Some(list),
                    ..
                } => (Source::ExcludesFile, &[], list, i),
                _ => return None,
            },
        };
        Some(Hit {
            source,
            dir,
            pattern: list.get(i)?,
            ancestor,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// A tree's `.gitignore` files by directory, and the directories the loader was asked for.
    struct Tree {
        files: BTreeMap<Vec<u8>, Vec<u8>>,
        asked: Vec<Vec<u8>>,
    }

    impl Tree {
        fn new(files: &[(&str, &str)]) -> Tree {
            Tree {
                files: files
                    .iter()
                    .map(|(d, t)| (d.as_bytes().to_vec(), t.as_bytes().to_vec()))
                    .collect(),
                asked: Vec::new(),
            }
        }

        fn check(
            &mut self,
            st: &mut IgnoreStack,
            path: &str,
            is_dir: bool,
        ) -> (bool, Option<String>) {
            let files = &self.files;
            let asked = &mut self.asked;
            let v = st
                .check(
                    path.as_bytes(),
                    is_dir,
                    |d| -> Result<Option<PatternList>, ()> {
                        asked.push(d.to_vec());
                        Ok(files.get(d).map(|t| PatternList::parse(t)))
                    },
                )
                .unwrap_or(Verdict {
                    ignored: false,
                    hit: None,
                });
            (v.ignored, v.hit.map(describe))
        }
    }

    fn describe(h: Hit<'_>) -> String {
        let mut pat = Vec::new();
        h.pattern.write_git_form(&mut pat);
        let src = match h.source {
            Source::Dir => format!("{}/.gitignore", String::from_utf8_lossy(h.dir)),
            Source::FilesIgnore => "files.ignore".to_owned(),
            Source::InfoExclude => "info/exclude".to_owned(),
            Source::ExcludesFile => "excludesFile".to_owned(),
        };
        let anc = h.ancestor.map_or(String::new(), |n| format!(" @{n}"));
        format!(
            "{src}:{}:{}{anc}",
            h.pattern.line,
            String::from_utf8_lossy(&pat)
        )
    }

    fn git(info: &str, excl: &str) -> Mode {
        Mode::Git {
            info_exclude: (!info.is_empty()).then(|| PatternList::parse(info.as_bytes())),
            excludes_file: (!excl.is_empty()).then(|| PatternList::parse(excl.as_bytes())),
        }
    }

    #[test]
    fn precedence() {
        let mut t = Tree::new(&[("", "*.log\n!keep.log\n"), ("sub", "!*.log\nx.tmp\n")]);
        let mut st = IgnoreStack::new(git("*.tmp\n!y.tmp\n", "*.bak\n*.tmp\n"), Case::Sensitive);
        assert_eq!(
            t.check(&mut st, "a.log", false),
            (true, Some("/.gitignore:1:*.log".into()))
        );
        assert_eq!(
            t.check(&mut st, "keep.log", false),
            (false, Some("/.gitignore:2:!keep.log".into()))
        );
        // The deeper file decides first.
        assert_eq!(
            t.check(&mut st, "sub/a.log", false),
            (false, Some("sub/.gitignore:1:!*.log".into()))
        );
        assert_eq!(
            t.check(&mut st, "sub/x.tmp", false),
            (true, Some("sub/.gitignore:2:x.tmp".into()))
        );
        // Then info/exclude, then core.excludesFile.
        assert_eq!(
            t.check(&mut st, "sub/z.tmp", false),
            (true, Some("info/exclude:1:*.tmp".into()))
        );
        assert_eq!(
            t.check(&mut st, "y.tmp", false),
            (false, Some("info/exclude:2:!y.tmp".into()))
        );
        assert_eq!(
            t.check(&mut st, "q.bak", false),
            (true, Some("excludesFile:1:*.bak".into()))
        );
        assert_eq!(t.check(&mut st, "q.rs", false), (false, None));
    }

    #[test]
    fn excluded_directory_wins_and_is_not_read() {
        let mut t = Tree::new(&[("", "out/\n"), ("out", "!keep\n"), ("src", "gen/\n")]);
        let mut st = IgnoreStack::new(git("", ""), Case::Sensitive);
        assert_eq!(
            t.check(&mut st, "out/keep", false),
            (true, Some("/.gitignore:1:out/ @3".into()))
        );
        assert_eq!(
            t.check(&mut st, "out/a/b", false),
            (true, Some("/.gitignore:1:out/ @3".into()))
        );
        assert_eq!(
            t.check(&mut st, "out", true),
            (true, Some("/.gitignore:1:out/".into()))
        );
        // A file named like the directory pattern is not a directory.
        assert_eq!(t.check(&mut st, "out", false), (false, None));
        assert_eq!(
            t.check(&mut st, "src/gen/x.rs", false),
            (true, Some("src/.gitignore:1:gen/ @7".into()))
        );
        assert!(!t.asked.iter().any(|d| d.as_slice() == b"out"));
        assert!(!t.asked.iter().any(|d| d.as_slice() == b"src/gen"));
    }

    #[test]
    fn negated_directory_is_entered() {
        let mut t = Tree::new(&[("", "build*\n!build-keep/\n"), ("build-keep", "*.o\n")]);
        let mut st = IgnoreStack::new(git("", ""), Case::Sensitive);
        assert_eq!(t.check(&mut st, "build-keep/a.c", false), (false, None));
        assert_eq!(
            t.check(&mut st, "build-keep/a.o", false),
            (true, Some("build-keep/.gitignore:1:*.o".into()))
        );
        assert_eq!(
            t.check(&mut st, "build-x/a.c", false),
            (true, Some("/.gitignore:1:build* @7".into()))
        );
    }

    #[test]
    fn anchoring() {
        let mut t = Tree::new(&[("", "/top\nmid/x\n**/deep\n"), ("sub", "/s1\nd/e\n")]);
        let mut st = IgnoreStack::new(git("", ""), Case::Sensitive);
        assert!(t.check(&mut st, "top", false).0);
        assert!(!t.check(&mut st, "sub/top", false).0);
        assert!(t.check(&mut st, "mid/x", false).0);
        assert!(!t.check(&mut st, "a/mid/x", false).0);
        assert!(t.check(&mut st, "deep", false).0);
        assert!(t.check(&mut st, "a/b/deep", false).0);
        assert!(t.check(&mut st, "sub/s1", false).0);
        assert!(!t.check(&mut st, "sub/q/s1", false).0);
        assert!(t.check(&mut st, "sub/d/e", false).0);
        assert!(!t.check(&mut st, "d/e", false).0);
        assert!(!t.check(&mut st, "sub/q/d/e", false).0);
    }

    #[test]
    fn walk_order_reads_each_file_once() {
        let mut t = Tree::new(&[("", "*.o\n"), ("a", "x\n"), ("a/b", "y\n")]);
        let mut st = IgnoreStack::new(git("", ""), Case::Sensitive);
        for (p, d) in [
            ("a", true),
            ("a/x", false),
            ("a/b", true),
            ("a/b/y", false),
            ("a/b/z.o", false),
            ("a/c", false),
            ("b", true),
            ("b/x", false),
            ("a/b/y", false),
        ] {
            t.check(&mut st, p, d);
        }
        let asked: Vec<&[u8]> = t.asked.iter().map(Vec::as_slice).collect();
        assert_eq!(asked, vec![&b""[..], b"a", b"a/b", b"b", b"a", b"a/b"]);
        assert!(st.heap_bytes() > 0);
        st.clear();
        assert_eq!(st.depth(), 0);
    }

    #[test]
    fn loader_errors_propagate() {
        let mut st = IgnoreStack::new(git("", ""), Case::Sensitive);
        let r = st.check(b"a/b/c", false, |d| {
            if d == b"a/b" { Err("denied") } else { Ok(None) }
        });
        assert_eq!(r.err(), Some("denied"));
        assert_eq!(st.depth(), 2);
        let r = st.check(b"a/b/c", false, |_| -> Result<Option<PatternList>, &str> {
            Ok(Some(PatternList::parse(b"c\n")))
        });
        assert!(r.is_ok_and(|v| v.ignored));
    }

    #[test]
    fn without_git() {
        let defaults = Mode::NoGit {
            files_ignore: PatternList::files_ignore_default(),
        };
        // No root `.gitignore`: `files.ignore` stands in for it; nested files still apply.
        let mut t = Tree::new(&[("app", "*.gen\n")]);
        let mut st = IgnoreStack::new(defaults.clone(), Case::Sensitive);
        assert_eq!(
            t.check(&mut st, "target", true),
            (true, Some("files.ignore:1:target/".into()))
        );
        assert_eq!(
            t.check(&mut st, "target/debug/x", false),
            (true, Some("files.ignore:1:target/ @6".into()))
        );
        assert!(t.check(&mut st, "web/node_modules/p/i.js", false).0);
        assert_eq!(t.check(&mut st, "build", false), (false, None));
        assert_eq!(
            t.check(&mut st, "app/x.gen", false),
            (true, Some("app/.gitignore:1:*.gen".into()))
        );
        // info/exclude and core.excludesFile do not exist without git.
        assert_eq!(t.check(&mut st, "x.tmp", false), (false, None));
        // A root `.gitignore`, even an empty one, replaces `files.ignore`.
        let mut t = Tree::new(&[("", "")]);
        let mut st = IgnoreStack::new(defaults, Case::Sensitive);
        assert_eq!(t.check(&mut st, "target/x", false), (false, None));
    }

    #[test]
    fn case_rule() {
        let mut t = Tree::new(&[("", "Build/\n*.LOG\n/Docs/Out\n")]);
        let mut st = IgnoreStack::new(git("", ""), Case::Insensitive);
        assert!(t.check(&mut st, "build", true).0);
        assert!(t.check(&mut st, "x.log", false).0);
        assert!(t.check(&mut st, "docs/out", false).0);
        let mut st = IgnoreStack::new(git("", ""), Case::Sensitive);
        assert!(!t.check(&mut st, "build", true).0);
        assert!(!t.check(&mut st, "x.log", false).0);
        assert!(!t.check(&mut st, "docs/out", false).0);
        assert_eq!(Case::from_insensitive(true), Case::Insensitive);
        assert_eq!(st.case(), Case::Sensitive);
    }

    #[test]
    fn unusual_paths_do_not_panic() {
        let mut t = Tree::new(&[("", "*\n!a\n"), ("a", "b\n")]);
        let mut st = IgnoreStack::new(git("x\n", "y\n"), Case::Insensitive);
        for p in [
            "", "/", "//", "/a", "a//b", "a/", "a/b/", "/a/b", "a/./b", "a/../b", "\0", "a/\0/b",
        ] {
            for d in [false, true] {
                t.check(&mut st, p, d);
            }
        }
        // An empty component is a directory with an empty name: `a//b` lies in `a/`, which lies in `a`.
        let mut st = IgnoreStack::new(git("", ""), Case::Sensitive);
        let mut t = Tree::new(&[("a", "/b\n")]);
        assert!(t.check(&mut st, "a/b", false).0);
        assert!(!t.check(&mut st, "a//b", false).0);
        let asked: Vec<&[u8]> = t.asked.iter().map(Vec::as_slice).collect();
        assert_eq!(asked, vec![&b""[..], b"a", b"a/"]);
    }
}
