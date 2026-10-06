//! WP-74: FL-1's ignore matcher (WP-61b; `moirai_files::ignore`, [F20 §4.4]) and never-candidate patterns
//! ([F20 §4.7.1], §4.7.2) against `git check-ignore --no-index`, in a scratch repository, on synthetic trees only:
//! WP-61b's acceptance ("WP-74's differential agrees with `git check-ignore` on synthetic trees").
//!
//! - **Named trees** ([`named_trees`]): one tree per corner of git's semantics — nesting and the precedence of the
//!   sources, excluded directories (nothing under one can be re-included, and its `.gitignore` is never read),
//!   negation, directory-only and anchored patterns, `**` in every position, case variants, escapes and bracket
//!   expressions (with virtual files whose names hold `*`, `?` or trailing spaces and dots, so that `\*`, `\?`,
//!   `[*?]` and `\ ` must match those bytes), and the line syntax (trailing spaces, CR, `00`, BOM, comments, empty
//!   patterns).
//! - **Generated trees** ([`gen_ignore`]): directories, files, virtual files and sources whose patterns are cut from
//!   the tree's own paths and then edited, so that most of them match something. A tree that disagrees is shrunk by
//!   proptest, and the smallest one it finds is reported.
//! - **Never-candidate names**: [F20 §4.7.2]'s list, transcribed from the chapter ([`SPEC_LIST`]), over fixed names
//!   whose rows the chapter's table gives ([`FIXED_NAMES`], checked against `never_pattern` without git too) and
//!   names instantiated from its patterns with near misses, and generated `*`/`?` pattern lists over names built from
//!   them; non-ASCII letters (`é`, `É`) test the byte semantics of `?` and `eqi`. Each list is checked against
//!   `matches_name_pattern` (the list against `never_pattern` too) and `git check-ignore -v` under
//!   `core.ignorecase = true` (`moirai_replay::ignorediff::check_names`).
//!
//! Every path of a tree is checked under both values of `core.ignorecase`, in git's verbose form (the deciding
//! pattern: source, line and text) and its plain form (the ignored paths; for a generated tree under one value,
//! alternating), as `moirai_replay::ignorediff` describes; a tree with a root `.gitignore` and no global source is
//! also checked against the matcher without git: reading that file with `files.ignore` set to `*`, with the file's
//! lines standing in as `files.ignore`, and with only the lines a `glob-list` can carry ([40 §5.8]; one named tree
//! holds the key's default).
//!
//! Each test writes the git version and its counts and agreement rates past libtest's capture ([`job_log`]), and its
//! report — the first `DETAIL_MAX` disagreements with their paths, both answers and the trees' sources, the rest
//! counted — to `CARGO_TARGET_TMPDIR/moirai-replay-reports/ignore-git-<test>.txt`, then fails, naming that file, if
//! any check disagreed. `MOIRAI_TEST_TIER` = `nightly` or `exit` widens the generated cases (PLAN §2.1).

mod common;
mod gen_ignore;
mod scratch;

use std::cell::RefCell;
use std::fmt::Display;
use std::path::{Path, PathBuf};

use moirai_files::ignore::{PatternList, never_pattern};
use moirai_files::oid::ObjectFormat;
use moirai_files::r14::NEVER_CANDIDATE_PATTERNS;
use moirai_replay::git::{CheckIgnore, Git, Repo};
use moirai_replay::ignorediff::{NameReport, Report, Tree, check_names, check_tree};
use moirai_replay::job_log;
use proptest::prelude::*;
use proptest::test_runner::TestError;

use scratch::Scratch;

/// The git version `docs/m0/tools.md` pins; the reference answers were checked against it, and git's line rules have
/// changed across versions.
const PINNED_GIT: &str = "2.54.0";

/// A scratch directory with an isolated git, its repository's work tree and the path of the `core.excludesFile`.
struct Setup {
    scratch: Scratch,
    git: Git,
}

impl Setup {
    /// The scratch directory `name`, and the git version written past libtest's capture, so that every run's log
    /// says which git gave the reference answers.
    fn new(name: &str) -> Setup {
        let scratch = Scratch::new(name);
        let git = Git::isolated(scratch.path()).expect("the isolated git home");
        let version = git.version().expect("git is installed (PLAN §2.4)");
        let pinned = version
            .strip_prefix("git version ")
            .is_some_and(|v| v == PINNED_GIT || v.starts_with(&format!("{PINNED_GIT}.")));
        let note = if pinned {
            String::new()
        } else {
            format!(
                "; not the {PINNED_GIT} docs/m0/tools.md pins, so a disagreement may be git's version"
            )
        };
        job_log(&format!("{name}: {version}{note}"));
        Setup { scratch, git }
    }

    fn repo(&self) -> Repo<'_> {
        self.git
            .init(&self.scratch.path().join("w"), ObjectFormat::Sha1)
            .expect("a scratch repository")
    }

    fn excludes(&self) -> PathBuf {
        self.scratch.path().join("excludes")
    }
}

/// Writes `report` to the report file and its summary past the capture, and fails the test with the summary and the
/// file's path unless `agrees`.
fn finish(test: &str, what: &str, summary: &str, report: &impl Display, agrees: bool) {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("moirai-replay-reports");
    std::fs::create_dir_all(&dir).expect("the report directory");
    let file = dir.join(format!("ignore-git-{test}.txt"));
    std::fs::write(&file, format!("{what}\n{report}")).expect("the report is written");
    job_log(&format!(
        "{test}: {summary}; full report in {}",
        file.display()
    ));
    assert!(
        agrees,
        "git check-ignore and moirai disagree: {summary}; full report in {}",
        file.display()
    );
}

/// A tree from a list of paths (a trailing `/` marks a directory; parents are added), its `.gitignore` files by
/// directory, `info/exclude` and the `core.excludesFile` contents.
fn tree(
    paths: &[&str],
    gitignores: &[(&str, &[u8])],
    info: Option<&[u8]>,
    excludes: Option<&[u8]>,
) -> Tree {
    let mut dirs: Vec<String> = Vec::new();
    let mut files: Vec<String> = Vec::new();
    let gitignore_dirs = gitignores
        .iter()
        .filter(|(d, _)| !d.is_empty())
        .map(|(d, _)| format!("{d}/"));
    for p in paths.iter().map(|p| (*p).to_string()).chain(gitignore_dirs) {
        let (path, is_dir) = match p.strip_suffix('/') {
            Some(d) => (d, true),
            None => (p.as_str(), false),
        };
        let mut end = 0;
        while let Some(i) = path[end..].find('/') {
            end += i;
            dirs.push(path[..end].to_string());
            end += 1;
        }
        if is_dir {
            dirs.push(path.to_string());
        } else {
            files.push(path.to_string());
        }
    }
    dirs.sort();
    dirs.dedup();
    Tree {
        dirs,
        files,
        gitignores: gitignores
            .iter()
            .map(|(d, b)| ((*d).to_string(), b.to_vec()))
            .collect(),
        info_exclude: info.map(<[u8]>::to_vec),
        excludes_file: excludes.map(<[u8]>::to_vec),
        ..Tree::default()
    }
}

/// `t` with the virtual files `names` (asked about, never written).
fn with_virtual(mut t: Tree, names: &[&str]) -> Tree {
    t.virtual_files = names.iter().map(|n| (*n).to_string()).collect();
    t
}

/// The named tree whose root `.gitignore` holds the default of `files.ignore` ([CFG §10.6]), so its without-git
/// comparison checks the matcher's defaults.
const FILES_IGNORE_TREE: &str = "the default of files.ignore";

/// The named trees (module documentation).
fn named_trees() -> Vec<(&'static str, Tree)> {
    vec![
        (
            "nesting and the precedence of the sources",
            tree(
                &[
                    "a.log", "keep.log", "sub/a.log", "sub/x.tmp", "sub/z.tmp", "y.tmp", "q.bak",
                    "sub/deep/b.log", "sub/deep/keep.log", "sub/deep/x.tmp", "q.rs",
                ],
                &[
                    ("", b"*.log\n!keep.log\n"),
                    ("sub", b"!*.log\nx.tmp\n"),
                    ("sub/deep", b"*.log\n!x.tmp\n"),
                ],
                Some(b"*.tmp\n!y.tmp\n"),
                Some(b"*.bak\n*.tmp\nq.rs\n!q.rs\n"),
            ),
        ),
        (
            "excluded directories",
            tree(
                &[
                    "out/keep", "out/a/b", "out/a/.gitignore", "build-keep/a.c", "build-keep/a.o",
                    "build-x/a.c", "src/out", "src/gen/x.rs", "src/gen/keep.rs", "logs/",
                ],
                &[
                    ("", b"out/\nbuild*\n!build-keep/\nlogs\n"),
                    ("out", b"!keep\n!a/\n"),
                    ("build-keep", b"*.o\n"),
                    ("src", b"gen/\n!gen/keep.rs\n"),
                ],
                None,
                None,
            ),
        ),
        (
            "excluded directories from info/exclude and core.excludesFile",
            tree(
                &[
                    "gen/x", "src/gen/y", "vendor/v/w", "vendor/keep", "a/vendor", "src/a", "x/src/a",
                    "src/b", "x/src/b",
                ],
                &[("", b"!vendor/keep\n"), ("gen", b"!x\n")],
                Some(b"gen/\n/src/a\n"),
                Some(b"vendor/\nsrc/b\n"),
            ),
        ),
        (
            "negation and the last match",
            tree(
                &[
                    "a.rs", "a.txt", "src/b.rs", "src/c.md", "src/deep/d.rs", "x", "y", "z/",
                ],
                &[("", b"*\n!*/\n!*.rs\nx\n!x\nx\ny\n!y\n!/z/\n")],
                None,
                None,
            ),
        ),
        (
            "directory-only patterns",
            tree(
                &[
                    "logs/x", "a/logs/y", "tmp/", "a/tmp/", "x.d/", "y.d", "cache", "a/cache/",
                    "a/b/", "b/a/",
                ],
                &[("", b"logs/\n/tmp/\ncache/\n*.d/\na/b/\n!a/logs/\n")],
                None,
                None,
            ),
        ),
        (
            "anchored and pathname patterns",
            tree(
                &[
                    "top", "sub/top", "mid/x", "a/mid/x", "a/b/c", "x/a/b/c", "sub/s1", "sub/q/s1",
                    "sub/d/e", "d/e", "sub/q/d/e", "sub/b/c",
                ],
                &[
                    ("", b"/top\nmid/x\n/a/b/\nb/c\n"),
                    ("sub", b"/s1\nd/e\n/b/c\n"),
                ],
                None,
                None,
            ),
        ),
        (
            "double stars",
            tree(
                &[
                    "deep", "a/b/deep", "a/x", "x/y", "x/m/y", "z/x/k/l/y", "mm/n", "m/z/n", "q/r/",
                    "q/r/s", "ab/b", "a/b/b", "p/1/2/3/y",
                ],
                &[(
                    "",
                    b"**/deep\na/**\n**/x/**/y\nm**/n\nq/**/\na**/b\n**b\np/**/**/**/y\n/**/mm\n",
                )],
                None,
                None,
            ),
        ),
        (
            "a star pattern and triple stars",
            tree(
                &["a", "b/c", ".hidden", "d/.e"],
                &[("", b"***\n!b/***\n**/*.*\n")],
                None,
                None,
            ),
        ),
        (
            "case variants",
            tree(
                &[
                    "build/a", "x.log", "docs/out", "bin/x", "Abc.txt", "up/abc.TXT", "é", "É2",
                    "Keep.LOG", "q/Q", "r/R",
                ],
                &[(
                    "",
                    "Build/\n*.LOG\n/Docs/Out\n[Bb]in/\n[[:upper:]]*.TXT\nÉ\né2\n!keep.log\n\\Q\n[r]/[R]\n"
                        .as_bytes(),
                )],
                None,
                None,
            ),
        ),
        (
            "escapes and bracket expressions",
            tree(
                &[
                    "#hash", "!bang", "[a]", "bb", "ab", "cx", "]z", "f1", "sp ace", "a-", "b^", "q",
                    "[x", "g!",
                ],
                &[(
                    "",
                    b"\\#hash\n\\!bang\n\\[a\\]\n[!a]b\n[a-c]x\n[]]z\nf[[:digit:]]\nsp\\ ace\n[a-]-\nb[\\^]\n[q\n\\[x\ng[!]\n",
                )],
                None,
                None,
            ),
        ),
        (
            "escapes against names that exist only as paths",
            with_virtual(
                tree(
                    &["a", "q", "sp", "b", "x/", "d/k"],
                    &[
                        (
                            "",
                            "\\*\nq\\?\nx[*?]y\nsp\\ \na\\ \\ \na*\\*\nb\\.\n\\[\\*]\né\\?\n".as_bytes(),
                        ),
                        ("d", b"?\n!\\?\n*\\ \n"),
                    ],
                    None,
                    None,
                ),
                &[
                    "*", "?", "a*", "q?", "sp ", "a  ", "a ", "x*y", "x?y", "b.", "é?", "É?", "[*]",
                    "d/?", "d/*", "d/k ", "d/k.", "x/q?",
                ],
            ),
        ),
        (
            "line syntax: BOM, trailing spaces, CR, 00 and comments",
            with_virtual(
                tree(
                    &["a", "b", "c", "d", "e", "ef", "last", "#comment", "h", "#h"],
                    &[(
                        "",
                        b"\xef\xbb\xbfa  \r\nb\\ \r\nc\\\\  \r\nd\t\r\ne\0f\n#comment\n\n   \n\\#h\r\r\nlast",
                    )],
                    None,
                    None,
                ),
                &["a  ", "b ", "c.", "last "],
            ),
        ),
        (
            "empty and degenerate patterns",
            tree(
                &["x", "y/", "z"],
                &[("", b"!\n/\n//\n!/\n \n\\ \n\\\n*\\\n!z\\\n")],
                None,
                None,
            ),
        ),
        (
            "a BOM elsewhere than at the start",
            tree(
                &["a", "\u{feff}b", "b"],
                &[("", b"a\n\xef\xbb\xbfb\n")],
                Some(b"\xef\xbb\xbfa\n"),
                None,
            ),
        ),
        (
            "long matches past the matcher's backtracking budget",
            tree(
                &[
                    "x/x/x/x/x/x/x/z",
                    "x/x/x/x/x/x/x/q",
                    "x/x/x/z",
                    "aa/aa/aa/aa/aa/aa/aa/aa/aa/xb",
                    "aa/aa/aa/aa/aa/aa/aa/aa/aa/xc",
                    "aa/aa/aa/aa/aa/aa/aa/aa/aa/x/b",
                    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaab",
                    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaac",
                ],
                &[(
                    "",
                    b"**/x/**/x/**/x/**/x/z\n**/a*/**/a*/**/a*/**/a*/**/a*/**/a*/*b\n*a*a*a*a*a*a*a*a*c\n",
                )],
                None,
                None,
            ),
        ),
        (
            FILES_IGNORE_TREE,
            tree(
                &[
                    "target/debug/x", "a/node_modules/p/i.js", "build", "sub/build/x", "app/x.gen",
                    "app/target", "Target/y",
                ],
                &[("", b"target/\nnode_modules/\nbuild/\n"), ("app", b"*.gen\n")],
                None,
                None,
            ),
        ),
        (
            "basename and pathname matching in nested files",
            tree(
                &[
                    "n/a/b/c", "n/b/c", "n/c", "n/x/c", "n/x/b/c", "c", "b/c", "n/.gitignore2",
                ],
                &[("n", b"a/b/c\nb/c\n!x/c\n*.gitignore*\n")],
                None,
                None,
            ),
        ),
    ]
}

#[test]
fn named_trees_agree_with_git_check_ignore() {
    const TEST: &str = "named_trees_agree_with_git_check_ignore";
    let setup = Setup::new("ignore-named");
    let repo = setup.repo();
    let mut report = Report::default();
    let trees = named_trees();
    let defaults = trees
        .iter()
        .find(|(label, _)| *label == FILES_IGNORE_TREE)
        .and_then(|(_, t)| t.glob_list_stand_in())
        .expect("the files.ignore tree has a root .gitignore and no global source");
    assert_eq!(
        PatternList::from_items(defaults.iter()),
        PatternList::files_ignore_default()
    );
    for (label, t) in &trees {
        check_tree(
            &repo,
            &setup.excludes(),
            t,
            label,
            &[false, true],
            &mut report,
        )
        .unwrap_or_else(|e| panic!("{label}: {e}"));
    }
    finish(
        TEST,
        "ignore matcher against git check-ignore --no-index, named trees ([F20 §4.4])",
        &report.summary(),
        &report,
        report.agrees(),
    );
    assert_eq!(report.trees as usize, trees.len());
    assert_eq!(report.verbose.checks, report.plain.checks);
    // The corpus reaches every class it is named for.
    assert!(
        report.own_root.checks > 0
            && report.stand_in.checks > 0
            && report.glob_list.checks > 0
            && report.virtual_matched > 0
            && report.virtual_matched < report.virtual_checks
            && report.ignored > 0
            && report.reincluded > 0
            && report.unmatched > 0
            && report.by_info_exclude > 0
            && report.by_excludes_file > 0
            && report.excluded_parent > 0
            && report.dir_only > 0
            && report.case_dependent > 0,
        "{}",
        report.summary()
    );
}

#[test]
fn generated_trees_agree_with_git_check_ignore() {
    const TEST: &str = "generated_trees_agree_with_git_check_ignore";
    let setup = Setup::new("ignore-generated");
    let repo = setup.repo();
    let excludes = setup.excludes();
    // The report of the cases up to the first failing one, the case count, and whether proptest is shrinking: once a
    // tree fails, every smaller tree proptest tries is checked into a report of its own.
    let state = RefCell::new((Report::default(), 0u64, false));
    let outcome = common::runner(TEST, 24).run(&gen_ignore::tree_spec(), |spec| {
        let t = gen_ignore::build(&spec);
        let mut s = state.borrow_mut();
        let (report, n, shrinking) = &mut *s;
        if *shrinking {
            let mut r = Report::default();
            return match check_tree(&repo, &excludes, &t, "shrinking", &[false, true], &mut r) {
                Ok(()) if r.agrees() => Ok(()),
                Ok(()) => Err(TestCaseError::fail("the tree disagrees")),
                Err(e) => Err(TestCaseError::fail(e.to_string())),
            };
        }
        *n += 1;
        let label = format!("case {n}");
        // The plain form under one value of core.ignorecase per tree, alternating.
        let plain = [*n % 2 == 0];
        let failure = match check_tree(&repo, &excludes, &t, &label, &plain, report) {
            Ok(()) if report.agrees() => return Ok(()),
            Ok(()) => format!("{label} disagrees"),
            Err(e) => format!("{label}: {e}"),
        };
        *shrinking = true;
        Err(TestCaseError::fail(failure))
    });
    let (report, _, _) = state.into_inner();
    let smallest = match &outcome {
        Ok(()) => String::new(),
        Err(TestError::Fail(reason, spec)) => {
            let t = gen_ignore::build(spec);
            let mut r = Report::default();
            check_tree(&repo, &excludes, &t, "smallest", &[false, true], &mut r).unwrap_or_else(
                |e| panic!("{reason}; the smallest failing tree: {e}\n{}", t.describe()),
            );
            format!("\nThe smallest failing tree proptest found ({reason}):\n{r}")
        }
        Err(e) => panic!("{e}"),
    };
    finish(
        TEST,
        "ignore matcher against git check-ignore --no-index, generated trees ([F20 §4.4])",
        &report.summary(),
        &format!("{report}{smallest}"),
        report.agrees() && outcome.is_ok(),
    );
    // The generator reaches every class.
    assert!(
        report.ignored > 0
            && report.reincluded > 0
            && report.unmatched > 0
            && report.by_gitignore > 0
            && report.by_info_exclude > 0
            && report.by_excludes_file > 0
            && report.excluded_parent > 0
            && report.dir_only > 0
            && report.case_dependent > 0
            && report.plain.checks > 0
            && report.own_root.checks > 0
            && report.stand_in.checks > 0
            && report.glob_list.checks > 0
            && report.virtual_matched > 0,
        "{}",
        report.summary()
    );
}

/// A tiny generator for names: xorshift64*.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: usize) -> usize {
        usize::try_from(self.next() % n as u64).unwrap_or(0)
    }
}

/// [F20 §4.7.2]'s list (resolver version 1), transcribed from the chapter's table independently of
/// `moirai_files::r14::NEVER_CANDIDATE_PATTERNS`. git's side and `matches_name_pattern` read this transcription and
/// `never_pattern` reads the constant, so a byte that differs between the two is a disagreement as well as a failed
/// equality.
const SPEC_LIST: [&[u8]; 21] = [
    b"*.tmp",
    b"*.tmp.*",
    b"*___jb_tmp___",
    b"*___jb_old___",
    b"*~",
    b"*.bak",
    b"*.orig",
    b"*.old",
    b"*.rej",
    b"*.swp",
    b"*.swo",
    b"4913",
    b".#*",
    b"~$*",
    b"sed??????",
    b"._*",
    b".DS_Store",
    b".fuse_hidden*",
    b".nfs*",
    b".goutputstream-*",
    b".~lock.*#",
];

/// Names with the row of [F20 §4.7.2]'s table that each matches first (`None`: no row), in two lists, since a
/// case-insensitive file system holds only one of two names that differ in case. `é` is two bytes, so `sedé1234`
/// has the nine bytes `sed??????` needs and `sedé123` has eight; `eqi` folds ASCII letters only.
const FIXED_NAMES: [&[(&str, Option<usize>)]; 2] = [
    &[
        ("a.tmp", Some(1)),
        ("a.tmp.x", Some(2)),
        ("x___jb_tmp___", Some(3)),
        ("x___jb_old___", Some(4)),
        ("x~", Some(5)),
        ("a.bak", Some(6)),
        ("a.orig", Some(7)),
        ("a.old", Some(8)),
        ("a.rej", Some(9)),
        (".a.swp", Some(10)),
        (".a.swo", Some(11)),
        ("4913", Some(12)),
        ("49131", None),
        (".#lock", Some(13)),
        ("~$doc.docx", Some(14)),
        ("sed123456", Some(15)),
        ("sed12345", None),
        ("._x", Some(16)),
        (".DS_Store", Some(17)),
        (".fuse_hidden0001", Some(18)),
        (".nfs000123", Some(19)),
        (".goutputstream-ABC123", Some(20)),
        (".~lock.a.odt#", Some(21)),
        ("tmp", None),
        ("a.tmpx", None),
        ("a_tmp", None),
        ("x~y", None),
        ("sedé1234", Some(15)),
        ("sedé123", None),
        ("É.TMP", Some(1)),
        ("é~", Some(5)),
        (".~lock.é#", Some(21)),
    ],
    &[
        ("A.TMP", Some(1)),
        ("X___JB_TMP___", Some(3)),
        ("A.Bak", Some(6)),
        ("SED1234567", None),
        ("SEDabcdef", Some(15)),
        (".ds_store", Some(17)),
        (".FUSE_HIDDEN", Some(18)),
        (".NFS", Some(19)),
        (".GOUTPUTSTREAM-", Some(20)),
        (".~LOCK.#", Some(21)),
        ("SEDé123", None),
        ("SEDÉ1234", Some(15)),
        ("é.tmp", Some(1)),
        ("É~", Some(5)),
        (".~LOCK.É#", Some(21)),
    ],
];

/// The pieces names instantiated from patterns are made of, and the literal pieces of generated patterns: ASCII
/// letters in both cases, the list's punctuation, and a non-ASCII letter (two bytes) in both cases.
const NAME_PIECES: &[&str] = &[
    "a", "b", "A", "B", ".", "~", "$", "#", "_", "-", "1", "x", "é", "É",
];

fn piece(rng: &mut Rng) -> &'static str {
    NAME_PIECES[rng.below(NAME_PIECES.len())]
}

/// A name `pattern` should match (each `*` replaced by 0 to 2 pieces, each `?` by one piece, so by two bytes when it
/// is `é` or `É`; letters in either case), then in about half the cases one character inserted, deleted or replaced,
/// a near miss.
fn instantiate(pattern: &[u8], rng: &mut Rng) -> String {
    let mut name: Vec<char> = Vec::new();
    for c in String::from_utf8_lossy(pattern).chars() {
        match c {
            '*' => {
                for _ in 0..rng.below(3) {
                    name.extend(piece(rng).chars());
                }
            }
            '?' => name.extend(piece(rng).chars()),
            // A letter in the other case.
            c if gen_ignore::flip(c) != c && rng.below(4) == 0 => name.push(gen_ignore::flip(c)),
            c => name.push(c),
        }
    }
    if rng.below(2) == 0 {
        let at = rng.below(name.len() + 1);
        let p = piece(rng);
        match rng.below(3) {
            0 => {
                for (k, ch) in p.chars().enumerate() {
                    name.insert(at + k, ch);
                }
            }
            1 if at < name.len() => {
                name.remove(at);
            }
            _ if at < name.len() => {
                name.remove(at);
                for (k, ch) in p.chars().enumerate() {
                    name.insert(at + k, ch);
                }
            }
            _ => name.extend(p.chars()),
        }
    }
    gen_ignore::sanitize(&name.into_iter().collect::<String>())
}

/// `n` names for `patterns`, distinct under case folding and other than `.gitignore`.
fn names_for(patterns: &[&[u8]], n: usize, rng: &mut Rng) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    let mut names = Vec::new();
    for _ in 0..n {
        let p = patterns[rng.below(patterns.len())];
        let name = instantiate(p, rng);
        let key = gen_ignore::fold(&name);
        if !seen.contains(&key) {
            seen.push(key);
            names.push(name);
        }
    }
    names
}

#[test]
fn the_never_candidate_list_agrees_with_git_check_ignore() {
    const TEST: &str = "the_never_candidate_list_agrees_with_git_check_ignore";
    // The constant is the chapter's table, byte for byte.
    assert_eq!(NEVER_CANDIDATE_PATTERNS, SPEC_LIST);
    // Each fixed name has the row the chapter's table gives, without git.
    let wrong_rows: Vec<String> = FIXED_NAMES
        .iter()
        .flat_map(|names| names.iter())
        .filter_map(|&(name, row)| {
            assert_eq!(gen_ignore::sanitize(name), name, "a valid file name");
            let got = never_pattern(name.as_bytes());
            (got != row).then(|| format!("{name}: never_pattern {got:?}, the table {row:?}"))
        })
        .collect();
    assert!(wrong_rows.is_empty(), "{wrong_rows:#?}");
    let setup = Setup::new("never-list");
    let repo = setup.repo();
    let mut report = NameReport::default();
    for (i, names) in FIXED_NAMES.iter().enumerate() {
        let names: Vec<String> = names.iter().map(|(n, _)| (*n).to_string()).collect();
        check_names(
            &repo,
            &setup.excludes(),
            &SPEC_LIST,
            &names,
            &format!("the list, fixed names {}", i + 1),
            true,
            &mut report,
        )
        .unwrap_or_else(|e| panic!("{e}"));
    }
    let rounds = match common::tier() {
        "nightly" => 32,
        "exit" => 128,
        _ => 2,
    };
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    for round in 0..rounds {
        let names = names_for(&SPEC_LIST, 96, &mut rng);
        check_names(
            &repo,
            &setup.excludes(),
            &SPEC_LIST,
            &names,
            &format!("the list, round {round}"),
            true,
            &mut report,
        )
        .unwrap_or_else(|e| panic!("{e}"));
    }
    finish(
        TEST,
        "never-candidate list ([F20 §4.7.2]) against git check-ignore -v, core.ignorecase=true",
        &report.summary(),
        &report,
        report.agrees(),
    );
    assert!(report.matched > 0 && report.matched < report.checks);
}

/// A never-candidate pattern of 1 to 6 pieces: `*`, `?` and the [`NAME_PIECES`], valid in a `.gitignore`
/// ([`moirai_replay::ignorediff::is_gitignore_safe_name_pattern`]).
fn name_pattern() -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(
        prop_oneof![
            3 => Just("*"),
            2 => Just("?"),
            8 => prop::sample::select(NAME_PIECES),
        ],
        1..7,
    )
    .prop_map(|pieces| {
        let mut p = pieces.concat().into_bytes();
        if matches!(p[0], b'#' | b'!') {
            p.insert(0, b'a');
        }
        p
    })
}

#[test]
fn generated_name_patterns_agree_with_git_check_ignore() {
    const TEST: &str = "generated_name_patterns_agree_with_git_check_ignore";
    let setup = Setup::new("never-generated");
    let repo = setup.repo();
    let report = RefCell::new(NameReport::default());
    let cases = (prop::collection::vec(name_pattern(), 1..7), any::<u64>());
    common::runner(TEST, 16)
        .run(&cases, |(patterns, seed)| {
            let patterns: Vec<&[u8]> = patterns.iter().map(Vec::as_slice).collect();
            let mut rng = Rng(seed | 1);
            let names = names_for(&patterns, 48, &mut rng);
            let mut r = report.borrow_mut();
            let label = format!("list {}", r.lists + 1);
            check_names(
                &repo,
                &setup.excludes(),
                &patterns,
                &names,
                &label,
                false,
                &mut r,
            )
            .unwrap_or_else(|e| panic!("{label}: {e}"));
            Ok(())
        })
        .unwrap_or_else(|e| panic!("{e}"));
    let report = report.into_inner();
    finish(
        TEST,
        "never-candidate pattern matching ([F20 §4.7.1]) against git check-ignore -v, core.ignorecase=true",
        &report.summary(),
        &report,
        report.agrees(),
    );
    assert!(report.matched > 0 && report.matched < report.checks);
}

#[test]
fn check_ignore_reports_sources_lines_and_patterns() {
    // git's own answers on a small tree, as the wrapper parses them: the reference half of the differential.
    let setup = Setup::new("ignore-wrapper");
    let repo = setup.repo();
    let excludes = setup.excludes();
    let t = tree(
        &["a.log", "keep.log", "sub/x", "out/k", "q.bak", "none"],
        &[("", b"*.log\n!keep.log\nout/\n"), ("sub", b"\n# c\nx\n")],
        Some(b"zz\n"),
        Some(b"*.bak\n"),
    );
    t.write(repo.dir(), &excludes).expect("the tree is written");
    let paths = [
        "a.log", "keep.log", "sub/x", "out/k", "out", "q.bak", "none", "q?", "sub/x ",
    ];
    let settings = CheckIgnore {
        ignore_case: false,
        excludes_file: Some(&excludes),
    };
    let got: Vec<Option<(String, u32, String)>> = repo
        .check_ignore(&paths, settings)
        .expect("check-ignore -v")
        .into_iter()
        .map(|m| {
            m.map(|m| {
                (
                    String::from_utf8_lossy(&m.source).into_owned(),
                    m.line,
                    String::from_utf8_lossy(&m.pattern).into_owned(),
                )
            })
        })
        .collect();
    let ex = settings.excludes_source().expect("set");
    let s = |src: &str, line: u32, pat: &str| Some((src.to_string(), line, pat.to_string()));
    assert_eq!(
        got,
        [
            s(".gitignore", 1, "*.log"),
            s(".gitignore", 2, "!keep.log"),
            s("sub/.gitignore", 3, "x"),
            s(".gitignore", 3, "out/"),
            s(".gitignore", 3, "out/"),
            s(&ex, 1, "*.bak"),
            None,
            // Paths that do not exist are answered too.
            None,
            None,
        ]
    );
    let plain = repo
        .check_ignore_plain(&paths, settings)
        .expect("check-ignore");
    assert_eq!(plain, ["a.log", "sub/x", "out/k", "out", "q.bak"]);
    // No path ignored is exit status 1, which is not an error.
    assert_eq!(
        repo.check_ignore_plain(&["none"], settings)
            .expect("check-ignore"),
        Vec::<String>::new()
    );
    assert!(repo.check_ignore(&[":x"], settings).is_err());
    assert!(repo.check_ignore(&[""], settings).is_err());
}
