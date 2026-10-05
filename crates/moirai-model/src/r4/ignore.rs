//! Ignored output ([F20 §4.4]; [40 §4.1] P6, §5.8): git's `.gitignore` semantics over the `.gitignore` files of a
//! simulated tree, and, without git, the same files when the root has any, else the `files.ignore` patterns ([CFG];
//! defaults `target/`, `node_modules/`, `build/`). A path is ignored when the matcher matches it or one of its ancestor
//! directories; a directory that is excluded cannot have a file below it re-included, as git's rule states.
//!
//! The abstract git history carries no `.git/info/exclude` and no `core.excludesFile` ([API §6.6]), so only the
//! tree's `.gitignore` files take part.

/// One parsed pattern of a `.gitignore`-format file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pattern {
    /// The directory of the file the pattern came from ("" for the root).
    pub base: String,
    /// The pattern body, without `!`, the trailing `/` and a leading `/`.
    pub glob: String,
    /// `!`: the pattern re-includes.
    pub negated: bool,
    /// A trailing `/`: the pattern matches directories only.
    pub dir_only: bool,
    /// The pattern contains a `/` before its end: matched against the path relative to `base`; else against the
    /// basename at any level.
    pub anchored: bool,
}

/// Parses the lines of one `.gitignore`-format file whose directory is `base`.
// spec: [F20 §4.4] git's `.gitignore` semantics
pub fn parse(base: &str, text: &str) -> Vec<Pattern> {
    let mut out = Vec::new();
    for raw in text.lines() {
        let mut line = raw.trim_end_matches('\r').to_string();
        // Trailing spaces are dropped unless escaped with a backslash.
        while line.ends_with(' ') && !line.ends_with("\\ ") {
            line.pop();
        }
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut negated = false;
        if let Some(rest) = line.strip_prefix('!') {
            negated = true;
            line = rest.to_string();
        } else if line.starts_with("\\!") || line.starts_with("\\#") {
            line.remove(0);
        }
        let dir_only = line.ends_with('/') && !line.ends_with("\\/");
        if dir_only {
            line.pop();
        }
        if line.is_empty() {
            continue;
        }
        let anchored = line.contains('/');
        let glob = line.strip_prefix('/').unwrap_or(&line).to_string();
        out.push(Pattern {
            base: base.to_string(),
            glob,
            negated,
            dir_only,
            anchored,
        });
    }
    out
}

/// The outcome of git's `dowild` (`wildmatch.c`): a match, no match, or one of its two aborts — `AbortAll` (nothing
/// can match: the text ran out, or the pattern is malformed) and `AbortToStarStar` (a `*` met a `/` it cannot cross;
/// only an enclosing `**` may go on).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Wm {
    Match,
    NoMatch,
    AbortAll,
    AbortToStarStar,
}

/// git's wildmatch with `WM_PATHNAME` and without `WM_CASEFOLD` ([F20 §4.4] "git's semantics"; `wildmatch.c`): `*` and
/// `?` never match `/`; `**` crosses directories only as a whole segment — at the pattern's start or after a `/`, and
/// followed by `/` or the end (a leading `**/` also matches no directory) — and is a plain `*` anywhere else; `[…]`
/// classes with `!` or `^`, ranges, `\` escapes and the ASCII POSIX classes `[:alnum:]` … `[:xdigit:]`, never matching
/// `/`; an unterminated class, an unknown class name and a trailing `\` match nothing.
// spec: [F20 §4.4] git's `.gitignore` semantics
fn wildmatch(p: &[u8], t: &[u8]) -> bool {
    dowild(p, 0, t, 0) == Wm::Match
}

/// One POSIX class of a bracket expression over a byte, by git's ASCII character classes (`wildmatch.c`, `ctype.c`;
/// no byte ≥ 0x80 is in any class); `None` for an unknown name.
fn posix_class(name: &[u8], c: u8) -> Option<bool> {
    Some(match name {
        b"alnum" => c.is_ascii_alphanumeric(),
        b"alpha" => c.is_ascii_alphabetic(),
        b"blank" => c == b' ' || c == b'\t',
        b"cntrl" => c.is_ascii_control(),
        b"digit" => c.is_ascii_digit(),
        b"graph" => c.is_ascii_graphic(),
        b"lower" => c.is_ascii_lowercase(),
        b"print" => c.is_ascii_graphic() || c == b' ',
        b"punct" => c.is_ascii_punctuation(),
        // git's own `isspace` (`ctype.c`): space, tab, LF and CR only.
        b"space" => matches!(c, b' ' | b'\t' | b'\n' | b'\r'),
        b"upper" => c.is_ascii_uppercase(),
        b"xdigit" => c.is_ascii_hexdigit(),
        _ => return None,
    })
}

/// The bracket expression starting at `p[pi] = '['` against the byte `tc`: the outcome (`Ok(true)` when the class
/// admits `tc`) and the index of the closing `]`, or `Err` with git's abort for a malformed class.
fn class(p: &[u8], mut pi: usize, tc: u8) -> Result<(bool, usize), Wm> {
    pi += 1;
    let mut pc = *p.get(pi).ok_or(Wm::AbortAll)?;
    let negated = pc == b'!' || pc == b'^';
    if negated {
        pi += 1;
        pc = *p.get(pi).ok_or(Wm::AbortAll)?;
    }
    // The previous member's byte for a range; 0 after a range or a POSIX class, as in git.
    let mut prev: u8 = 0;
    let mut matched = false;
    loop {
        if pc == b'\\' {
            pi += 1;
            pc = *p.get(pi).ok_or(Wm::AbortAll)?;
            matched |= tc == pc;
        } else if pc == b'-' && prev != 0 && p.get(pi + 1).is_some_and(|&n| n != b']') {
            pi += 1;
            let mut hi = p[pi];
            if hi == b'\\' {
                pi += 1;
                hi = *p.get(pi).ok_or(Wm::AbortAll)?;
            }
            matched |= prev <= tc && tc <= hi;
            pc = 0;
        } else if pc == b'[' && p.get(pi + 1) == Some(&b':') {
            let s = pi + 2;
            let e = s + p[s..].iter().position(|&c| c == b']').ok_or(Wm::AbortAll)?;
            if e == s || p[e - 1] != b':' {
                // Not a `[:name:]`: the `[` is an ordinary member and the bytes after it are read as members.
                matched |= tc == b'[';
            } else {
                matched |= posix_class(&p[s..e - 1], tc).ok_or(Wm::AbortAll)?;
                pi = e;
                pc = 0;
            }
        } else {
            matched |= tc == pc;
        }
        prev = pc;
        pi += 1;
        match p.get(pi) {
            None => return Err(Wm::AbortAll),
            Some(b']') => return Ok((matched != negated && tc != b'/', pi)),
            Some(&c) => pc = c,
        }
    }
}

/// `dowild` over `p[pi..]` and `t[ti..]` (`wildmatch.c`, with `WM_PATHNAME`).
fn dowild(p: &[u8], mut pi: usize, t: &[u8], mut ti: usize) -> Wm {
    while pi < p.len() {
        let pc = p[pi];
        let tc = match t.get(ti) {
            Some(&c) => c,
            None if pc != b'*' => return Wm::AbortAll,
            None => 0,
        };
        match pc {
            b'\\' => {
                pi += 1;
                if p.get(pi) != Some(&tc) {
                    return Wm::NoMatch;
                }
            }
            b'?' => {
                if tc == b'/' {
                    return Wm::NoMatch;
                }
            }
            b'*' => {
                let star = pi;
                pi += 1;
                let mut match_slash = false;
                if p.get(pi) == Some(&b'*') {
                    while p.get(pi) == Some(&b'*') {
                        pi += 1;
                    }
                    let before = star == 0 || p[star - 1] == b'/';
                    let after = match p.get(pi) {
                        None | Some(b'/') => true,
                        Some(b'\\') => p.get(pi + 1) == Some(&b'/'),
                        _ => false,
                    };
                    if before && after {
                        // `**/` first matches no directory: `a/**/b` matches `a/b`.
                        if p.get(pi) == Some(&b'/') && dowild(p, pi + 1, t, ti) == Wm::Match {
                            return Wm::Match;
                        }
                        match_slash = true;
                    }
                }
                if pi == p.len() {
                    // A trailing `**` matches everything, a trailing `*` what holds no `/`.
                    return if !match_slash && t[ti..].contains(&b'/') {
                        Wm::NoMatch
                    } else {
                        Wm::Match
                    };
                }
                if !match_slash && p[pi] == b'/' {
                    // One `*` before a `/` takes the rest of this directory name.
                    match t[ti..].iter().position(|&c| c == b'/') {
                        Some(k) => {
                            ti += k + 1;
                            pi += 1;
                            continue;
                        }
                        None => return Wm::NoMatch,
                    }
                }
                loop {
                    let Some(&c) = t.get(ti) else {
                        return Wm::AbortAll;
                    };
                    match dowild(p, pi, t, ti) {
                        Wm::NoMatch if !match_slash && c == b'/' => return Wm::AbortToStarStar,
                        Wm::NoMatch => {}
                        Wm::AbortToStarStar if match_slash => {}
                        other => return other,
                    }
                    ti += 1;
                }
            }
            b'[' => match class(p, pi, tc) {
                Ok((true, close)) => pi = close,
                Ok((false, _)) => return Wm::NoMatch,
                Err(abort) => return abort,
            },
            c => {
                if tc != c {
                    return Wm::NoMatch;
                }
            }
        }
        pi += 1;
        ti += 1;
    }
    if ti == t.len() {
        Wm::Match
    } else {
        Wm::NoMatch
    }
}

impl Pattern {
    /// Whether the pattern matches a path (relative to the root) of a file or directory.
    fn matches(&self, path: &str, is_dir: bool) -> bool {
        if self.dir_only && !is_dir {
            return false;
        }
        let rel = if self.base.is_empty() {
            path
        } else {
            match path.strip_prefix(&format!("{}/", self.base)) {
                Some(r) => r,
                None => return false,
            }
        };
        if self.anchored {
            wildmatch(self.glob.as_bytes(), rel.as_bytes())
        } else {
            let name = rel.rsplit('/').next().unwrap_or(rel);
            wildmatch(self.glob.as_bytes(), name.as_bytes())
        }
    }
}

/// The matcher of one tree: its patterns, root files first, each file's lines in order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Matcher {
    /// The patterns in precedence order (a later match overrides an earlier one).
    pub patterns: Vec<Pattern>,
}

impl Matcher {
    /// The matcher of a tree: its `.gitignore` files (by depth, then path), or `defaults` (`files.ignore`) when the
    /// tree has none and no git.
    // spec: [F20 §4.4]; [40 §5.8]
    pub fn of(
        files: &std::collections::BTreeMap<String, Vec<u8>>,
        git: bool,
        defaults: &[String],
    ) -> Matcher {
        let mut ig: Vec<(&String, &Vec<u8>)> = files
            .iter()
            .filter(|(p, _)| p.as_str() == ".gitignore" || p.ends_with("/.gitignore"))
            .collect();
        ig.sort_by_key(|(p, _)| (p.matches('/').count(), (*p).clone()));
        let mut patterns = Vec::new();
        if ig.is_empty() && !git {
            for d in defaults {
                patterns.extend(parse("", d));
            }
        }
        for (p, b) in ig {
            let base = p.rfind('/').map_or("", |i| &p[..i]);
            patterns.extend(parse(base, &String::from_utf8_lossy(b)));
        }
        Matcher { patterns }
    }

    /// The last matching pattern's verdict for one path, `None` when none matches.
    fn verdict(&self, path: &str, is_dir: bool) -> Option<bool> {
        self.patterns
            .iter()
            .rev()
            .find(|p| p.matches(path, is_dir))
            .map(|p| !p.negated)
    }

    /// Whether a file path is ignored: an ancestor directory is excluded, or the path itself is.
    // spec: [F20 §4.4]
    pub fn ignored(&self, path: &str) -> bool {
        let segs: Vec<&str> = path.split('/').collect();
        for k in 1..segs.len() {
            if self.verdict(&segs[..k].join("/"), true) == Some(true) {
                return true;
            }
        }
        self.verdict(path, false) == Some(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn m(files: &[(&str, &str)], git: bool) -> Matcher {
        let f: BTreeMap<String, Vec<u8>> = files
            .iter()
            .map(|(p, b)| (p.to_string(), b.as_bytes().to_vec()))
            .collect();
        Matcher::of(
            &f,
            git,
            &["target/".into(), "node_modules/".into(), "build/".into()],
        )
    }

    #[test]
    fn gitignore_semantics() {
        let x = m(
            &[(
                ".gitignore",
                "*.log\n/out\ndocs/**/tmp\n!keep.log\nbuild/\n",
            )],
            true,
        );
        assert!(x.ignored("a.log"));
        assert!(x.ignored("deep/a.log"));
        assert!(!x.ignored("keep.log"));
        assert!(x.ignored("out/x.rs"));
        assert!(
            !x.ignored("src/out/x.rs"),
            "a leading / anchors at the root"
        );
        assert!(x.ignored("docs/a/b/tmp/x"));
        assert!(x.ignored("docs/tmp/x"));
        assert!(x.ignored("build/a.o"));
        assert!(!x.ignored("buildx/a.o"));
        let y = m(&[(".gitignore", "logs/\n!logs/keep.txt\n")], true);
        assert!(
            y.ignored("logs/keep.txt"),
            "a file below an excluded directory is never re-included"
        );
        let z = m(&[("sub/.gitignore", "*.tmp\n")], true);
        assert!(z.ignored("sub/a.tmp"));
        assert!(!z.ignored("a.tmp"));
    }

    /// git's `wildmatch` with `WM_PATHNAME`: `**` as a whole segment only, POSIX classes, and the malformed patterns
    /// that match nothing.
    #[test]
    fn wildmatch_follows_git() {
        let yes = |p: &str, t: &str| assert!(wildmatch(p.as_bytes(), t.as_bytes()), "{p} ~ {t}");
        let no = |p: &str, t: &str| assert!(!wildmatch(p.as_bytes(), t.as_bytes()), "{p} !~ {t}");
        // `**` crosses directories at the start, after `/`, before `/` or the end; elsewhere it is a `*`.
        yes("foo/**/bar", "foo/bar");
        yes("foo/**/bar", "foo/a/b/bar");
        yes("**/bar", "bar");
        yes("**/bar", "a/b/bar");
        yes("foo/**", "foo/a/b");
        yes("**", "a/b");
        no("foo**/bar", "foo/x/bar");
        yes("foo**/bar", "foox/bar");
        no("foo**", "foo/bar");
        yes("foo**", "foobar");
        no("a**b/c", "a/x/b/c");
        no("x/**b", "x/a/b");
        yes("x/**b", "x/ab");
        // `*`, `?` and classes never match `/`.
        yes("*/b", "a/b");
        no("*/b", "a/c/b");
        no("a?b", "a/b");
        no("a[/]b", "a/b");
        no("a*", "a/b");
        // Classes, ranges, escapes.
        yes("[]]", "]");
        yes("[a-c]x", "bx");
        no("[a-c]x", "dx");
        yes("[!a-c]x", "dx");
        yes("[^a]", "b");
        yes("\\*", "*");
        no("\\*", "a");
        yes("[\\]]", "]");
        // POSIX classes, alone and among other members.
        yes("[[:digit:]]*.log", "1x.log");
        no("[[:digit:]]*.log", "x1.log");
        yes("[[:upper:][:digit:]]", "Q");
        yes("[[:upper:][:digit:]]", "7");
        no("[[:upper:][:digit:]]", "q");
        yes("[![:alpha:]]", "_");
        yes("[[:space:]]", " ");
        yes("[[:xdigit:]][[:punct:]]", "f-");
        // A `[:` without `:]` is two ordinary members.
        yes("[[:]x", ":x");
        yes("[[:]x", "[x");
        // Malformed patterns match nothing: an unknown class name, an unterminated class, a trailing `\`.
        no("[[:alfa:]]", "a");
        no("[abc", "[abc");
        no("[abc", "a");
        no("a\\", "a\\");
        // Through the matcher.
        let x = m(&[(".gitignore", "foo**/bar\n[[:digit:]]*\n")], true);
        assert!(x.ignored("foox/bar"));
        assert!(!x.ignored("foo/x/bar"));
        assert!(x.ignored("src/7.tmp"));
        assert!(!x.ignored("src/a7.tmp"));
    }

    #[test]
    fn defaults_apply_without_git_and_without_ignore_files() {
        let x = m(&[], false);
        assert!(x.ignored("target/debug/a"));
        assert!(x.ignored("pkg/node_modules/x.js"));
        assert!(!x.ignored("src/lib.rs"));
        assert!(
            !m(&[], true).ignored("target/debug/a"),
            "with git only the .gitignore files count"
        );
    }
}
