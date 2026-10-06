//! Pattern lists: one ignore source (a `.gitignore`, `info/exclude`, `core.excludesFile` or the `files.ignore` items)
//! parsed into git's pattern records, and the last-match rule over one list ([F20 §4.4]).
//!
//! # Line syntax (git's)
//!
//! The source is split at `0A`; a final line without `0A` counts. Lines are numbered from 1 (the number git's
//! `check-ignore -v` prints). On line 1 a leading UTF-8 byte-order mark `EF BB BF` is dropped. Then, per line:
//!
//! 1. an empty line, or one whose first byte is `#`, is no pattern;
//! 2. one trailing `0D` is dropped;
//! 3. the line ends at its first `00` byte (git reads it as a C string);
//! 4. trailing `20` bytes are dropped unless escaped: a `\` keeps the byte after it, and a line ending in a lone `\`
//!    keeps all its spaces (tabs are never trimmed);
//! 5. a leading `!` negates the pattern and is removed (`\!` is a literal `!`, as `\#` is a literal `#`);
//! 6. one trailing `/` makes the pattern match directories only and is removed;
//! 7. a pattern with no `/` left matches the last component of a path at any depth below the source's directory; any
//!    other pattern matches the path relative to that directory, with a leading `/` only anchoring it.
//!
//! A pattern that is empty after these steps matches no path (a path's last component is never empty), so it is not
//! kept. A source of [`PATTERN_FILE_MAX`] bytes or more contributes no pattern at all ([`PatternList::too_large`]), as
//! git ignores such a file with a warning; a caller that streams a source learns it as soon as the limit is reached
//! ([`PatternParser::too_large`]) and may stop reading.
//!
//! # Memory
//!
//! A list keeps the bytes of its patterns once, in one buffer, and 20 bytes per pattern. The streaming parser keeps
//! only the line being read (never a comment line) and drops everything once the source reaches the size limit, so
//! its memory is bounded by the limit whatever the source.

use super::Case;
use super::wild::{self, WildFlags};

/// git's limit on a pattern file: a source of this many bytes or more is ignored whole (git's `PATTERN_MAX_FILE_SIZE`,
/// 100 MiB, measured against `git check-ignore`).
pub const PATTERN_FILE_MAX: u64 = 100 * 1024 * 1024;

/// The UTF-8 byte-order mark git drops at the start of a pattern file.
const BOM: &[u8] = b"\xef\xbb\xbf";

const NEGATIVE: u8 = 1;
const MUSTBEDIR: u8 = 2;
const NODIR: u8 = 4;
const ENDSWITH: u8 = 8;

/// One pattern: its bytes in the list's buffer, the length of its leading literal part (git's `nowildcardlen`), its
/// line and its flags.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Pattern {
    start: u32,
    len: u32,
    nowild: u32,
    line: u32,
    flags: u8,
}

/// The patterns of one ignore source, in source order.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PatternList {
    text: Vec<u8>,
    pats: Vec<Pattern>,
    too_large: bool,
}

/// One pattern of a list, as git's `check-ignore -v` describes it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PatternRef<'a> {
    /// The pattern's bytes as git stores them: without the leading `!` and the trailing `/`, with a leading `/` and
    /// every `\` kept.
    pub text: &'a [u8],
    /// The line of the source it came from (from 1; the item number for `files.ignore`).
    pub line: u32,
    /// The line began with `!`: a match re-includes the path.
    pub negated: bool,
    /// The line ended with `/`: the pattern matches directories only.
    pub dir_only: bool,
}

impl PatternRef<'_> {
    /// Appends the pattern as `git check-ignore -v` prints it: `!` if negated, the text, `/` if directory-only.
    pub fn write_git_form(&self, out: &mut Vec<u8>) {
        if self.negated {
            out.push(b'!');
        }
        out.extend_from_slice(self.text);
        if self.dir_only {
            out.push(b'/');
        }
    }
}

impl PatternList {
    /// The empty list.
    #[must_use]
    pub const fn new() -> PatternList {
        PatternList {
            text: Vec::new(),
            pats: Vec::new(),
            too_large: false,
        }
    }

    /// Parses a whole source held in memory; the same as one [`PatternParser::feed`] and [`PatternParser::finish`].
    #[must_use]
    pub fn parse(bytes: &[u8]) -> PatternList {
        let mut p = PatternParser::new();
        p.feed(bytes);
        p.finish()
    }

    /// The list of the `files.ignore` items ([CFG §10.6], a `glob-list` of gitignore patterns), each item one line
    /// numbered from 1, under the line rules of this module (no byte-order mark is looked for). Items whose total
    /// length reaches [`PATTERN_FILE_MAX`] give a [`too_large`](PatternList::too_large) list, as a file would.
    // spec: [CFG §10.6] (`files.ignore`: gitignore patterns, in written order)
    #[must_use]
    pub fn from_items<I, T>(items: I) -> PatternList
    where
        I: IntoIterator<Item = T>,
        T: AsRef<[u8]>,
    {
        let mut list = PatternList::new();
        let mut total: u64 = 0;
        for (n, item) in items.into_iter().enumerate() {
            let item = item.as_ref();
            total = total.saturating_add(item.len() as u64 + 1);
            if total >= PATTERN_FILE_MAX {
                return PatternList {
                    text: Vec::new(),
                    pats: Vec::new(),
                    too_large: true,
                };
            }
            let line = u32::try_from(n + 1).unwrap_or(u32::MAX);
            if !item.is_empty() && item[0] != b'#' {
                let item = item.strip_suffix(b"\r").unwrap_or(item);
                list.add(item, line);
            }
        }
        list.shrink();
        list
    }

    /// The list of [`FILES_IGNORE_DEFAULT`](super::FILES_IGNORE_DEFAULT), the default of `files.ignore`.
    // spec: [CFG §10.6] (default `target/,node_modules/,build/`), [40 §5.8]
    #[must_use]
    pub fn files_ignore_default() -> PatternList {
        PatternList::from_items(super::FILES_IGNORE_DEFAULT)
    }

    /// The number of patterns.
    #[must_use]
    pub fn len(&self) -> usize {
        self.pats.len()
    }

    /// True iff the list has no pattern.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.pats.is_empty()
    }

    /// True iff the source reached [`PATTERN_FILE_MAX`] bytes and was ignored whole (git warns "ignoring excessively
    /// large pattern file").
    #[must_use]
    pub fn too_large(&self) -> bool {
        self.too_large
    }

    /// The pattern at index `i` (source order).
    #[must_use]
    pub fn get(&self, i: usize) -> Option<PatternRef<'_>> {
        self.pats.get(i).map(|p| self.pattern_ref(p))
    }

    /// The patterns in source order.
    pub fn iter(&self) -> impl Iterator<Item = PatternRef<'_>> + '_ {
        self.pats.iter().map(|p| self.pattern_ref(p))
    }

    /// The heap bytes the list holds.
    #[must_use]
    pub fn heap_bytes(&self) -> usize {
        self.text.capacity() + self.pats.capacity() * size_of::<Pattern>()
    }

    fn pattern_ref(&self, p: &Pattern) -> PatternRef<'_> {
        PatternRef {
            text: self.bytes(p),
            line: p.line,
            negated: p.flags & NEGATIVE != 0,
            dir_only: p.flags & MUSTBEDIR != 0,
        }
    }

    fn bytes(&self, p: &Pattern) -> &[u8] {
        &self.text[p.start as usize..p.start as usize + p.len as usize]
    }

    fn shrink(&mut self) {
        self.text.shrink_to_fit();
        self.pats.shrink_to_fit();
    }

    /// Adds the pattern of one line after steps 1 and 2 of the module's line syntax.
    // spec: [F20 §4.4] (git's semantics: line syntax — `00`, trailing spaces, `!`, trailing `/`, anchoring)
    fn add(&mut self, line: &[u8], lineno: u32) {
        let line = match line.iter().position(|&b| b == 0) {
            Some(z) => &line[..z],
            None => line,
        };
        let line = trim_trailing_spaces(line);
        let mut flags = 0u8;
        let mut p = line;
        if let Some(rest) = p.strip_prefix(b"!") {
            flags |= NEGATIVE;
            p = rest;
        }
        let mut len = p.len();
        if len > 0 && p[len - 1] == b'/' {
            len -= 1;
            flags |= MUSTBEDIR;
        }
        if !p[..len].contains(&b'/') {
            flags |= NODIR;
        }
        let nowild = wild::simple_length(p).min(len);
        if p.first() == Some(&b'*') && wild::simple_length(&p[1..]) == p.len() - 1 {
            flags |= ENDSWITH;
        }
        if len == 0 {
            return;
        }
        // The source is below PATTERN_FILE_MAX bytes, so every offset fits in u32.
        let as32 = |v: usize| u32::try_from(v).unwrap_or(u32::MAX);
        self.pats.push(Pattern {
            start: as32(self.text.len()),
            len: as32(len),
            nowild: as32(nowild),
            line: lineno,
            flags,
        });
        self.text.extend_from_slice(&p[..len]);
    }

    /// The index of the last pattern of the list that matches `path` (git's "within one source, the last matching
    /// pattern decides"), or `None`.
    ///
    /// `path` lies below the list's directory, which is `path[..base_len]` (0 for the root); its last component starts
    /// at `name_start`; `is_dir` says whether it names a directory.
    // spec: [F20 §4.4] (git's semantics: last match within a source; basename and pathname matching)
    pub(crate) fn last_match(
        &self,
        path: &[u8],
        base_len: usize,
        name_start: usize,
        is_dir: bool,
        case: Case,
    ) -> Option<usize> {
        let fold = matches!(case, Case::Insensitive);
        let basename = &path[name_start..];
        self.pats.iter().rposition(|pat| {
            if pat.flags & MUSTBEDIR != 0 && !is_dir {
                return false;
            }
            let text = self.bytes(pat);
            let nowild = pat.nowild as usize;
            if pat.flags & NODIR != 0 {
                match_basename(basename, text, nowild, pat.flags, fold)
            } else {
                match_pathname(path, base_len, text, nowild, fold)
            }
        })
    }
}

/// `a == b`, ASCII case-insensitively when `fold` (git's `fspathncmp` over equal lengths).
#[inline]
fn eq(a: &[u8], b: &[u8], fold: bool) -> bool {
    if fold {
        a.eq_ignore_ascii_case(b)
    } else {
        a == b
    }
}

/// A pattern without `/` against the last component of the path (git's `match_basename`).
// spec: [F20 §4.4] (git's semantics: a pattern without `/` matches the last component, at any depth)
fn match_basename(name: &[u8], pat: &[u8], nowild: usize, flags: u8, fold: bool) -> bool {
    if nowild == pat.len() {
        return pat.len() == name.len() && eq(pat, name, fold);
    }
    if flags & ENDSWITH != 0 {
        let tail = &pat[1..];
        return tail.len() <= name.len() && eq(tail, &name[name.len() - tail.len()..], fold);
    }
    wild::wildmatch(
        pat,
        0,
        name,
        WildFlags {
            pathname: false,
            casefold: fold,
        },
    )
}

/// A pattern with `/` against the path relative to the list's directory `path[..base_len]` (git's
/// `match_pathname`): a leading `/` only anchors; the literal prefix is compared first.
// spec: [F20 §4.4] (git's semantics: a pattern with `/` matches the path relative to its source's directory)
fn match_pathname(path: &[u8], base_len: usize, pat: &[u8], nowild: usize, fold: bool) -> bool {
    let (pat, prefix) = match pat.strip_prefix(b"/") {
        Some(rest) => (rest, nowild.saturating_sub(1)),
        None => (pat, nowild),
    };
    if path.len() < base_len + 1 || (base_len > 0 && path[base_len] != b'/') {
        return false;
    }
    let name = if base_len > 0 {
        &path[base_len + 1..]
    } else {
        path
    };
    if prefix > 0 {
        if prefix > name.len() || !eq(&pat[..prefix], &name[..prefix], fold) {
            return false;
        }
        if prefix == pat.len() && prefix == name.len() {
            return true;
        }
    }
    wild::wildmatch(
        pat,
        prefix,
        &name[prefix..],
        WildFlags {
            pathname: true,
            casefold: fold,
        },
    )
}

/// Drops trailing `20` bytes that no `\` escapes (git's `trim_trailing_spaces`).
// spec: [F20 §4.4] (git's semantics: trailing spaces)
fn trim_trailing_spaces(s: &[u8]) -> &[u8] {
    let mut last_space: Option<usize> = None;
    let mut i = 0;
    while i < s.len() {
        match s[i] {
            b' ' => {
                if last_space.is_none() {
                    last_space = Some(i);
                }
            }
            b'\\' => {
                i += 1;
                if i >= s.len() {
                    return s;
                }
                last_space = None;
            }
            _ => last_space = None,
        }
        i += 1;
    }
    match last_space {
        Some(k) => &s[..k],
        None => s,
    }
}

/// The streaming parser of one ignore source: feed the bytes in chunks of any size, in order, then
/// [`finish`](PatternParser::finish). Chunk boundaries never change the result. It opens nothing; the caller reads
/// the source (through `ProjectFs` or the git object reader) and passes the bytes (PLAN §2.1, GT20 (d)).
#[derive(Debug)]
pub struct PatternParser {
    list: PatternList,
    line: Vec<u8>,
    skipping: bool,
    lineno: u32,
    total: u64,
    limit: u64,
}

impl Default for PatternParser {
    fn default() -> PatternParser {
        PatternParser::new()
    }
}

impl PatternParser {
    /// A parser at the start of a source.
    #[must_use]
    pub const fn new() -> PatternParser {
        PatternParser::with_limit(PATTERN_FILE_MAX)
    }

    /// A parser that ignores a source of `limit` bytes or more.
    const fn with_limit(limit: u64) -> PatternParser {
        PatternParser {
            list: PatternList::new(),
            line: Vec::new(),
            skipping: false,
            lineno: 1,
            total: 0,
            limit,
        }
    }

    /// Consumes the next bytes of the source.
    // spec: [F20 §4.4] (git's semantics: lines split at `0A`, comments, the byte-order mark, the size limit)
    pub fn feed(&mut self, chunk: &[u8]) {
        if self.list.too_large {
            return;
        }
        self.total = self.total.saturating_add(chunk.len() as u64);
        if self.total >= self.limit {
            self.list = PatternList {
                text: Vec::new(),
                pats: Vec::new(),
                too_large: true,
            };
            self.line = Vec::new();
            return;
        }
        let mut rest = chunk;
        loop {
            let nl = rest.iter().position(|&b| b == b'\n');
            let part = nl.map_or(rest, |i| &rest[..i]);
            if !self.skipping {
                if self.line.is_empty() && part.first() == Some(&b'#') {
                    // A comment line: its bytes are not kept. (On line 1 a byte-order mark would come first.)
                    self.skipping = true;
                } else {
                    self.line.extend_from_slice(part);
                }
            }
            match nl {
                Some(i) => {
                    self.end_line();
                    rest = &rest[i + 1..];
                }
                None => break,
            }
        }
    }

    /// True iff the bytes fed so far reach [`PATTERN_FILE_MAX`]: the source is then ignored whole, so a caller that
    /// streams it may stop reading ([`finish`](PatternParser::finish) returns an empty, too large list).
    #[must_use]
    pub const fn too_large(&self) -> bool {
        self.list.too_large
    }

    /// Ends the source and returns its patterns.
    // spec: [F20 §4.4] (git's semantics: a final line without `0A` counts)
    #[must_use]
    pub fn finish(mut self) -> PatternList {
        if !self.list.too_large && (!self.line.is_empty() || self.skipping) {
            self.end_line();
        }
        self.list.shrink();
        self.list
    }

    /// Ends the current line: drops it when it is a comment or empty, drops a byte-order mark on line 1 and one
    /// trailing `0D`, and adds its pattern.
    // spec: [F20 §4.4] (git's semantics: comments, the byte-order mark, a trailing CR)
    fn end_line(&mut self) {
        if self.skipping {
            self.skipping = false;
        } else {
            let mut l: &[u8] = &self.line;
            if self.lineno == 1 {
                l = l.strip_prefix(BOM).unwrap_or(l);
            }
            if !l.is_empty() && l[0] != b'#' {
                let l = l.strip_suffix(b"\r").unwrap_or(l);
                self.list.add(l, self.lineno);
            }
        }
        self.lineno = self.lineno.saturating_add(1);
        self.line.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(list: &PatternList) -> Vec<(String, u32, bool, bool)> {
        list.iter()
            .map(|p| {
                (
                    String::from_utf8_lossy(p.text).into_owned(),
                    p.line,
                    p.negated,
                    p.dir_only,
                )
            })
            .collect()
    }

    #[test]
    fn line_rules() {
        let src = [
            &b"\xef\xbb\xbfbom1\r\ntr1   \ntr2\\ \nt3\t\nback\\\nnul\x00x\n"[..],
            b" \n\r\n#c\n\\#h\n\\!b\n!\n/\nfoo//\nend",
        ]
        .concat();
        let l = PatternList::parse(&src);
        let got = texts(&l);
        let want: Vec<(String, u32, bool, bool)> = vec![
            ("bom1".into(), 1, false, false),
            ("tr1".into(), 2, false, false),
            ("tr2\\ ".into(), 3, false, false),
            ("t3\t".into(), 4, false, false),
            ("back\\".into(), 5, false, false),
            ("nul".into(), 6, false, false),
            ("\\#h".into(), 10, false, false),
            ("\\!b".into(), 11, false, false),
            ("foo/".into(), 14, false, true),
            ("end".into(), 15, false, false),
        ];
        assert_eq!(got, want);
    }

    #[test]
    fn flags() {
        let l = PatternList::parse(b"!a/\n*.o\n/x\n*.d/\n*[ab]\na/b\n");
        let p: Vec<Pattern> = l.pats.clone();
        assert_eq!(p[0].flags, NEGATIVE | MUSTBEDIR | NODIR);
        assert_eq!(p[1].flags, NODIR | ENDSWITH);
        assert_eq!(p[2].flags, 0);
        assert_eq!(p[2].nowild, 2);
        assert_eq!(p[3].flags, MUSTBEDIR | NODIR | ENDSWITH);
        assert_eq!(p[4].flags, NODIR);
        assert_eq!(p[5].flags, 0);
        assert_eq!(p[5].nowild, 3);
    }

    #[test]
    fn comments_and_bom() {
        let l = PatternList::parse(b"\xef\xbb\xbf#c\nx");
        assert_eq!(texts(&l), vec![("x".to_owned(), 2, false, false)]);
        // A byte-order mark only counts at the start of the source.
        let l = PatternList::parse(b"a\n\xef\xbb\xbfb");
        assert_eq!(l.get(1).map(|p| p.text), Some(&b"\xef\xbb\xbfb"[..]));
        // A partial mark is an ordinary byte.
        let l = PatternList::parse(b"\xef\xbbq");
        assert_eq!(l.get(0).map(|p| p.text), Some(&b"\xef\xbbq"[..]));
        assert!(PatternList::parse(b"").is_empty());
        assert!(PatternList::parse(b"\xef\xbb\xbf").is_empty());
        assert!(PatternList::parse(b"\n\n#x\n   \n").is_empty());
    }

    #[test]
    fn items() {
        let l = PatternList::files_ignore_default();
        assert_eq!(
            texts(&l),
            vec![
                ("target".to_owned(), 1, false, true),
                ("node_modules".to_owned(), 2, false, true),
                ("build".to_owned(), 3, false, true),
            ]
        );
        let l = PatternList::from_items(["*.tmp", "#x", "", "!keep.tmp"]);
        assert_eq!(
            texts(&l),
            vec![
                ("*.tmp".to_owned(), 1, false, false),
                ("keep.tmp".to_owned(), 4, true, false)
            ]
        );
        assert!(PatternList::from_items(Vec::<&[u8]>::new()).is_empty());
    }

    #[test]
    fn git_form() {
        let l = PatternList::parse(b"!/a/b/\n");
        let mut out = Vec::new();
        if let Some(p) = l.get(0) {
            p.write_git_form(&mut out);
        }
        assert_eq!(out, b"!/a/b/");
    }

    #[test]
    fn too_large() {
        // git ignores a file of 104,857,600 bytes and reads one of 104,857,599 (measured with git 2.54).
        assert_eq!(PATTERN_FILE_MAX, 104_857_600);
        let parse = |chunks: &[&[u8]]| {
            let mut p = PatternParser::with_limit(16);
            for c in chunks {
                p.feed(c);
            }
            p.finish()
        };
        let below = parse(&[b"big\n", b"#23456789", b"01"]);
        assert!(!below.too_large());
        assert_eq!(below.len(), 1);
        let at = parse(&[b"big\n", b"#23456789", b"012"]);
        assert!(at.too_large());
        assert!(at.is_empty());
        // A streaming caller sees it at once.
        let mut p = PatternParser::with_limit(16);
        p.feed(b"big\n#23456789");
        assert!(!p.too_large());
        p.feed(b"012");
        assert!(p.too_large());
        p.feed(b"more\n");
        assert!(p.too_large() && p.finish().is_empty());
        let after = parse(&[b"big\n#234567890123456\n", b"more\n"]);
        assert!(after.too_large());
        assert!(after.is_empty());
    }

    #[test]
    fn trim() {
        assert_eq!(trim_trailing_spaces(b"a  "), b"a");
        assert_eq!(trim_trailing_spaces(b"a\\  "), b"a\\ ");
        assert_eq!(trim_trailing_spaces(b"a \\"), b"a \\");
        assert_eq!(trim_trailing_spaces(b"  "), b"");
        assert_eq!(trim_trailing_spaces(b"a b"), b"a b");
        assert_eq!(trim_trailing_spaces(b"a\t "), b"a\t");
    }
}
