//! WP-61b: properties of the ignore matcher ([F20 §4.4]) and the never-candidate names ([F20 §4.7]) over generated
//! inputs:
//!
//! - chunk boundaries never change a parsed pattern list (the streaming parser over caller-supplied bytes);
//! - the matcher agrees with a reference written here from git's gitignore documentation and git's line rules, on a
//!   language of names, `*`, `**`, `?`, `/`, bracket expressions (members, a range, `!`), `\` escapes (of a letter,
//!   of `*`, of a trailing space), `!` and `\!`, `#` comments and `\#`, a trailing `/`, trailing spaces, a trailing
//!   CR, a `00` byte and a byte-order mark: component-wise globs, `**` as zero or more directories (one or more at
//!   the end), anchoring, directory-only patterns, last match per source, the deepest source first, then
//!   `info/exclude`, then `core.excludesFile`, and "a path under an excluded directory is excluded" — for both case
//!   rules and both modes. It agrees on the verdict and on the deciding pattern: its source, its line, and the
//!   excluded ancestor directory when there is one;
//! - case folding: on that language, the case-insensitive answer is the case-sensitive answer over lower-cased
//!   patterns and paths;
//! - a walk's cached directory chain never changes an answer: checking paths in any order with one matcher gives
//!   what a fresh matcher gives for each path;
//! - totality: arbitrary bytes as pattern files and paths never panic;
//! - the never-candidate matcher agrees with a naive recursive reading of [F20 §4.7.1], and the contextual rules of
//!   [F20 §4.7.3] hold on generated names.
//!
//! git's own exceptions to case folding (an escaped letter and a bracket member compare unfolded) are outside this
//! language and are covered by the unit tests of `wild`, as is the glob matcher against a naive port of git's
//! `dowild`. The differential against `git check-ignore` itself is WP-74's (`moirai-replay`).

use moirai_files::ignore::{
    Case, IgnoreStack, Mode, PatternList, PatternParser, Source, Verdict, is_cloud_conflict_copy,
    is_old_name_plus_suffix, matches_name_pattern, never_pattern,
};
use moirai_files::r14::NEVER_CANDIDATE_PATTERNS;
use proptest::prelude::*;
use std::collections::BTreeMap;

fn test_config(base: u32) -> ProptestConfig {
    let cases = match std::env::var("MOIRAI_TEST_TIER").as_deref() {
        Ok("nightly") => base * 16,
        Ok("exit") => base * 64,
        _ => base,
    };
    ProptestConfig {
        cases,
        failure_persistence: None,
        ..ProptestConfig::default()
    }
}

// --- generators ---------------------------------------------------------------------------------------------------

/// Pattern pieces. Bracket members and escaped letters are lower-case, so plain folding describes them.
const TOKENS: &[&str] = &[
    "a", "b", "ab", "A", "B", "*", "*", "**", "?", "/", "/", "[ab]", "[!a]", "[a-b]", "\\a", "\\*",
    "\\ ",
];
/// What a line starts with: nothing, `!`, an escaped `!` or `#`, or `#` (a comment).
const PREFIXES: &[&str] = &["", "", "", "!", "!", "\\!", "\\#", "#"];
/// What a line ends with: nothing, `/`, trailing spaces, a CR, a `00` byte and more bytes.
const SUFFIXES: &[&str] = &[
    "", "", "", "/", "/", "  ", "/ ", "\r", "/\r", " \r", "\0zz", "/\0z",
];
const NAMES: &[&str] = &["a", "b", "ab", "Ab", "B", "bb", "b ", "#a", "!a"];
const DIRS: &[&str] = &["", "a", "b", "ab", "a/b", "a/ab", "b/a", "Ab"];

/// The UTF-8 byte-order mark.
const BOM: &str = "\u{feff}";

/// A raw line: a prefix, a body of tokens, a suffix.
fn line() -> impl Strategy<Value = String> {
    (
        prop::sample::select(PREFIXES),
        prop::collection::vec(prop::sample::select(TOKENS), 1..5),
        prop::sample::select(SUFFIXES),
    )
        .prop_map(|(pre, toks, suf)| {
            let mut s = String::from(pre);
            for t in toks {
                s.push_str(t);
            }
            s.push_str(suf);
            s
        })
}

/// The lines of a source, the first sometimes behind a byte-order mark.
fn file() -> impl Strategy<Value = Vec<String>> {
    (any::<bool>(), prop::collection::vec(line(), 0..5)).prop_map(|(bom, mut lines)| {
        if bom && let Some(first) = lines.first_mut() {
            first.insert_str(0, BOM);
        }
        lines
    })
}

fn path() -> impl Strategy<Value = (Vec<&'static str>, bool)> {
    (
        prop::collection::vec(prop::sample::select(NAMES), 1..5),
        any::<bool>(),
    )
}

#[derive(Clone, Debug)]
struct Setup {
    files: BTreeMap<String, Vec<String>>,
    info: Option<Vec<String>>,
    excl: Option<Vec<String>>,
    git: bool,
    defaults: Vec<String>,
    insensitive: bool,
}

fn setup() -> impl Strategy<Value = Setup> {
    (
        prop::collection::btree_map(
            prop::sample::select(DIRS).prop_map(str::to_owned),
            file(),
            0..5,
        ),
        prop::option::of(file()),
        prop::option::of(file()),
        any::<bool>(),
        file(),
        any::<bool>(),
    )
        .prop_map(|(files, info, excl, git, defaults, insensitive)| Setup {
            files,
            info,
            excl,
            git,
            defaults,
            insensitive,
        })
}

fn join(lines: &[String]) -> Vec<u8> {
    let mut out = Vec::new();
    for l in lines {
        out.extend_from_slice(l.as_bytes());
        out.push(b'\n');
    }
    out
}

// --- the matcher under test ---------------------------------------------------------------------------------------

/// A verdict as compared here: ignored, the deciding pattern's source and line, and the excluded ancestor's length.
type Seen = Option<(bool, String, u32, Option<usize>)>;

fn stack(s: &Setup) -> IgnoreStack {
    let mode = if s.git {
        Mode::Git {
            info_exclude: s.info.as_deref().map(|l| PatternList::parse(&join(l))),
            excludes_file: s.excl.as_deref().map(|l| PatternList::parse(&join(l))),
        }
    } else {
        Mode::NoGit {
            files_ignore: PatternList::from_items(&s.defaults),
        }
    };
    IgnoreStack::new(mode, Case::from_insensitive(s.insensitive))
}

fn check(st: &mut IgnoreStack, s: &Setup, comps: &[&str], is_dir: bool) -> Seen {
    check_in(st, &s.files, false, comps, is_dir)
}

/// [`check`] with the tree's files looked up by directory, lower-cased first when `lower_dirs`.
fn check_in(
    st: &mut IgnoreStack,
    files: &BTreeMap<String, Vec<String>>,
    lower_dirs: bool,
    comps: &[&str],
    is_dir: bool,
) -> Seen {
    let path = comps.join("/");
    let v: Verdict<'_> = st
        .check(
            path.as_bytes(),
            is_dir,
            |d| -> Result<Option<PatternList>, ()> {
                let mut d = String::from_utf8_lossy(d).into_owned();
                if lower_dirs {
                    d.make_ascii_lowercase();
                }
                Ok(files.get(&d).map(|l| PatternList::parse(&join(l))))
            },
        )
        .unwrap_or(Verdict {
            ignored: false,
            hit: None,
        });
    v.hit.map(|h| {
        let src = match h.source {
            Source::Dir => format!("dir:{}", String::from_utf8_lossy(h.dir)),
            Source::FilesIgnore => "files".to_owned(),
            Source::InfoExclude => "info".to_owned(),
            Source::ExcludesFile => "excl".to_owned(),
        };
        (v.ignored, src, h.pattern.line, h.ancestor)
    })
}

fn verdict(seen: &Seen) -> Option<bool> {
    seen.as_ref().map(|s| s.0)
}

// --- the reference, from git's documentation and line rules -------------------------------------------------------

/// One pattern: negated, directory-only, the body (without `!` and one trailing `/`), its line.
struct RefLine {
    neg: bool,
    dir_only: bool,
    body: Vec<u8>,
    line: u32,
}

/// The patterns of a source. A byte-order mark is dropped from the start of a file (not from `files.ignore` items);
/// then an empty line or one that starts with `#` is a comment; one trailing CR goes; the line ends at a `00` byte;
/// trailing spaces go unless a `\` escapes them; a leading `!` negates; one trailing `/` makes it directory-only.
fn ref_parse(lines: &[String], file: bool) -> Vec<RefLine> {
    let mut out = Vec::new();
    for (i, raw) in lines.iter().enumerate() {
        let mut l = raw.as_bytes();
        if file && i == 0 {
            l = l.strip_prefix(BOM.as_bytes()).unwrap_or(l);
        }
        if l.is_empty() || l[0] == b'#' {
            continue;
        }
        let l = l.strip_suffix(b"\r").unwrap_or(l);
        let l = &l[..l.iter().position(|&b| b == 0).unwrap_or(l.len())];
        let mut end = l.len();
        while end > 0 && l[end - 1] == b' ' {
            let slashes = l[..end - 1]
                .iter()
                .rev()
                .take_while(|&&b| b == b'\\')
                .count();
            if slashes % 2 == 1 {
                break;
            }
            end -= 1;
        }
        let l = &l[..end];
        let (neg, l) = match l.strip_prefix(b"!") {
            Some(r) => (true, r),
            None => (false, l),
        };
        let (dir_only, body) = match l.strip_suffix(b"/") {
            Some(b) => (true, b),
            None => (false, l),
        };
        if !body.is_empty() {
            out.push(RefLine {
                neg,
                dir_only,
                body: body.to_vec(),
                line: u32::try_from(i + 1).unwrap_or(u32::MAX),
            });
        }
    }
    out
}

fn eqc(a: u8, b: u8, fold: bool) -> bool {
    if fold {
        a.eq_ignore_ascii_case(&b)
    } else {
        a == b
    }
}

/// Whether `c` is in the members of a bracket expression (bytes and `x-y` ranges).
fn in_set(set: &[u8], c: u8, fold: bool) -> bool {
    let c = if fold { c.to_ascii_lowercase() } else { c };
    let mut i = 0;
    while i < set.len() {
        if i + 2 < set.len() && set[i + 1] == b'-' {
            if (set[i]..=set[i + 2]).contains(&c) {
                return true;
            }
            i += 3;
        } else {
            if set[i] == c {
                return true;
            }
            i += 1;
        }
    }
    false
}

/// A glob over one component: every run of `*` is any byte sequence, `?` one byte, `[...]` one byte in (or with `!`
/// not in) the set, `\x` the byte x.
fn ref_glob(g: &[u8], s: &[u8], fold: bool) -> bool {
    match g.first() {
        None => s.is_empty(),
        Some(b'*') => (0..=s.len()).any(|k| ref_glob(&g[1..], &s[k..], fold)),
        Some(b'?') => !s.is_empty() && ref_glob(&g[1..], &s[1..], fold),
        Some(b'\\') if g.len() > 1 => {
            !s.is_empty() && eqc(g[1], s[0], fold) && ref_glob(&g[2..], &s[1..], fold)
        }
        Some(b'[') => {
            let close = g.iter().position(|&c| c == b']').unwrap_or(g.len() - 1);
            let (neg, set) = match g[1..close].strip_prefix(b"!") {
                Some(rest) => (true, rest),
                None => (false, &g[1..close]),
            };
            !s.is_empty()
                && in_set(set, s[0], fold) != neg
                && ref_glob(&g[close + 1..], &s[1..], fold)
        }
        Some(&c) => !s.is_empty() && eqc(c, s[0], fold) && ref_glob(&g[1..], &s[1..], fold),
    }
}

/// Segments of a pattern with `/` against components: a segment of only `*` (two or more) is any number of whole
/// components — zero or more, one or more at the end; any other segment is a glob over exactly one component.
fn ref_segments(segs: &[&[u8]], comps: &[&str], fold: bool) -> bool {
    match segs.first() {
        None => comps.is_empty(),
        Some(seg) if seg.len() >= 2 && seg.iter().all(|&c| c == b'*') => {
            if segs.len() == 1 {
                !comps.is_empty()
            } else {
                (0..=comps.len()).any(|k| ref_segments(&segs[1..], &comps[k..], fold))
            }
        }
        Some(seg) => {
            !comps.is_empty()
                && ref_glob(seg, comps[0].as_bytes(), fold)
                && ref_segments(&segs[1..], &comps[1..], fold)
        }
    }
}

/// Whether a pattern of the source at directory `base` matches `comps` (below `base`).
fn ref_line(l: &RefLine, base: &[&str], comps: &[&str], is_dir: bool, fold: bool) -> bool {
    if l.dir_only && !is_dir {
        return false;
    }
    let rel = &comps[base.len()..];
    if l.body.contains(&b'/') {
        let body = l.body.strip_prefix(b"/").unwrap_or(&l.body);
        let segs: Vec<&[u8]> = body.split(|&c| c == b'/').collect();
        ref_segments(&segs, rel, fold)
    } else {
        ref_glob(&l.body, comps[comps.len() - 1].as_bytes(), fold)
    }
}

/// The deciding pattern's negation, source and line, over the sources that apply to `comps`: the `.gitignore` files
/// of the directories strictly above it (deepest first), then the global sources.
fn ref_decide(s: &Setup, comps: &[&str], is_dir: bool) -> Option<(bool, String, u32)> {
    let fold = s.insensitive;
    let parent = &comps[..comps.len() - 1];
    let mut sources: Vec<(Vec<&str>, Vec<RefLine>, String)> = Vec::new();
    for k in 0..=parent.len() {
        let d = parent[..k].join("/");
        match s.files.get(&d) {
            Some(lines) => sources.push((
                parent[..k].to_vec(),
                ref_parse(lines, true),
                format!("dir:{d}"),
            )),
            None if k == 0 && !s.git => {
                sources.push((
                    Vec::new(),
                    ref_parse(&s.defaults, false),
                    "files".to_owned(),
                ));
            }
            None => {}
        }
    }
    sources.reverse();
    if s.git {
        if let Some(info) = &s.info {
            sources.push((Vec::new(), ref_parse(info, true), "info".to_owned()));
        }
        if let Some(excl) = &s.excl {
            sources.push((Vec::new(), ref_parse(excl, true), "excl".to_owned()));
        }
    }
    for (base, lines, src) in &sources {
        for l in lines.iter().rev() {
            if ref_line(l, base, comps, is_dir, fold) {
                return Some((l.neg, src.clone(), l.line));
            }
        }
    }
    None
}

/// The verdict as [`Seen`]: an excluded ancestor directory (the shallowest) decides first.
fn ref_check(s: &Setup, comps: &[&str], is_dir: bool) -> Seen {
    for k in 1..comps.len() {
        if let Some((false, src, line)) = ref_decide(s, &comps[..k], true) {
            return Some((true, src, line, Some(comps[..k].join("/").len())));
        }
    }
    ref_decide(s, comps, is_dir).map(|(neg, src, line)| (!neg, src, line, None))
}

proptest! {
    #![proptest_config(test_config(256))]

    #[test]
    fn matcher_agrees_with_the_documented_reference(
        s in setup(),
        paths in prop::collection::vec(path(), 1..12),
    ) {
        let mut st = stack(&s);
        for (comps, is_dir) in &paths {
            let got = check(&mut st, &s, comps, *is_dir);
            let want = ref_check(&s, comps, *is_dir);
            prop_assert_eq!(got, want, "path {:?} dir {}", comps.join("/"), is_dir);
        }
    }

    #[test]
    fn case_folding_is_lower_casing(s in setup(), paths in prop::collection::vec(path(), 1..8)) {
        // Directory spellings are the loader's business, not the patterns': both trees file their `.gitignore`s
        // under lower-cased directories, and two directories of DIRS that differ only in case are not both used.
        prop_assume!(s.files.keys().map(|d| d.to_ascii_lowercase()).collect::<std::collections::BTreeSet<_>>().len()
            == s.files.len());
        let lower = |v: &Vec<String>| v.iter().map(|l| l.to_ascii_lowercase()).collect::<Vec<_>>();
        let folded = Setup {
            files: s.files.iter().map(|(d, l)| (d.to_ascii_lowercase(), l.clone())).collect(),
            insensitive: true,
            ..s.clone()
        };
        let lowered = Setup {
            files: s.files.iter().map(|(d, l)| (d.to_ascii_lowercase(), lower(l))).collect(),
            info: s.info.as_ref().map(lower),
            excl: s.excl.as_ref().map(lower),
            defaults: lower(&s.defaults),
            insensitive: false,
            ..s.clone()
        };
        let (mut a, mut b) = (stack(&folded), stack(&lowered));
        for (comps, is_dir) in &paths {
            let low: Vec<String> = comps.iter().map(|c| c.to_ascii_lowercase()).collect();
            let low: Vec<&str> = low.iter().map(String::as_str).collect();
            prop_assert_eq!(
                verdict(&check_in(&mut a, &folded.files, true, comps, *is_dir)),
                verdict(&check_in(&mut b, &lowered.files, false, &low, *is_dir))
            );
        }
    }

    #[test]
    fn cached_chain_never_changes_an_answer(
        s in setup(),
        paths in prop::collection::vec(path(), 1..16),
    ) {
        let mut shared = stack(&s);
        for (comps, is_dir) in &paths {
            let mut fresh = stack(&s);
            prop_assert_eq!(check(&mut shared, &s, comps, *is_dir), check(&mut fresh, &s, comps, *is_dir));
        }
    }
}

// --- the streaming parser and totality ----------------------------------------------------------------------------

/// Bytes that look like pattern files: fragments, line ends, CR, NUL, the byte-order mark, and arbitrary bytes.
fn source_bytes() -> impl Strategy<Value = Vec<u8>> {
    let frag = prop_oneof![
        prop::sample::select(vec![
            &b"\n"[..],
            b"\r\n",
            b"\r",
            b"\0",
            b"#",
            b"!",
            b"/",
            b"\\",
            b" ",
            b"\t",
            b"*",
            b"**",
            b"?",
            b"[a-c]",
            b"[[:alpha:]]",
            b"\xef\xbb\xbf",
            b"a",
            b"b/c",
        ])
        .prop_map(<[u8]>::to_vec),
        prop::collection::vec(any::<u8>(), 0..6),
    ];
    prop::collection::vec(frag, 0..24).prop_map(|v| v.concat())
}

proptest! {
    #![proptest_config(test_config(512))]

    #[test]
    fn chunking_never_changes_the_list(
        src in source_bytes(),
        cuts in prop::collection::vec(any::<prop::sample::Index>(), 0..6),
    ) {
        let whole = PatternList::parse(&src);
        let mut at: Vec<usize> = cuts.iter().map(|i| i.index(src.len() + 1)).collect();
        at.sort_unstable();
        let mut p = PatternParser::new();
        let mut from = 0;
        for c in at {
            p.feed(&src[from..c]);
            from = c;
        }
        p.feed(&src[from..]);
        prop_assert_eq!(p.finish(), whole);
    }

    #[test]
    fn arbitrary_bytes_never_panic(
        root in source_bytes(),
        nested in source_bytes(),
        info in source_bytes(),
        paths in prop::collection::vec((prop::collection::vec(any::<u8>(), 0..12), any::<bool>()), 1..8),
        insensitive in any::<bool>(),
    ) {
        let mode = Mode::Git { info_exclude: Some(PatternList::parse(&info)), excludes_file: None };
        let mut st = IgnoreStack::new(mode, Case::from_insensitive(insensitive));
        for (p, is_dir) in &paths {
            let r = st.check(p, *is_dir, |d| -> Result<Option<PatternList>, ()> {
                Ok(Some(PatternList::parse(if d.is_empty() { &root } else { &nested })))
            });
            prop_assert!(r.is_ok());
        }
    }

    #[test]
    fn patterns_from_arbitrary_bytes_match_paths_without_panic(
        src in source_bytes(),
        names in prop::collection::vec(
            prop::collection::vec(prop::sample::select(b"ab/*?[]-!\\.".to_vec()), 0..10),
            1..8,
        ),
    ) {
        let list = PatternList::parse(&src);
        let mut st = IgnoreStack::new(Mode::NoGit { files_ignore: list }, Case::Sensitive);
        for n in &names {
            let r = st.check(n, false, |_| -> Result<Option<PatternList>, ()> { Ok(None) });
            prop_assert!(r.is_ok());
        }
    }
}

// --- never-candidate names ----------------------------------------------------------------------------------------

/// [F20 §4.7.1] read naively: `*` any sequence, `?` one byte, every other byte `eqi`.
fn ref_name(p: &[u8], n: &[u8]) -> bool {
    match p.first() {
        None => n.is_empty(),
        Some(b'*') => (0..=n.len()).any(|k| ref_name(&p[1..], &n[k..])),
        Some(b'?') => !n.is_empty() && ref_name(&p[1..], &n[1..]),
        Some(c) => !n.is_empty() && c.eq_ignore_ascii_case(&n[0]) && ref_name(&p[1..], &n[1..]),
    }
}

fn name_bytes(alphabet: &'static [u8], max: usize) -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(prop::sample::select(alphabet.to_vec()), 0..max)
}

proptest! {
    #![proptest_config(test_config(1024))]

    #[test]
    fn name_pattern_agrees_with_the_naive_reading(
        p in name_bytes(b"*?aAb.~$#", 8),
        n in name_bytes(b"aAbB.~$#", 10),
    ) {
        prop_assert_eq!(matches_name_pattern(&p, &n), ref_name(&p, &n));
    }

    #[test]
    fn never_pattern_is_the_first_matching_row(
        n in name_bytes(b"aAtTmMpP.~$#_sSedDoOlLbBkK49136", 14),
        dir in name_bytes(b"ab/", 6),
    ) {
        let want = NEVER_CANDIDATE_PATTERNS.iter().position(|pat| ref_name(pat, &n)).map(|i| i + 1);
        let mut path = dir.clone();
        if !path.is_empty() {
            path.push(b'/');
        }
        path.extend_from_slice(&n);
        prop_assert_eq!(never_pattern(&path), want);
    }

    #[test]
    fn old_name_plus_any_suffix(
        p in name_bytes(b"aAbB.x", 8),
        s in name_bytes(b"aAbB.x~", 4),
    ) {
        let mut q = p.to_ascii_uppercase();
        q.extend_from_slice(&s);
        prop_assert_eq!(is_old_name_plus_suffix(&q, &p), !s.is_empty());
    }

    #[test]
    fn cloud_conflict_copies(
        stem in name_bytes(b"ab.", 6),
        ext in name_bytes(b"ab", 4),
        x in name_bytes(b"ab-_", 5),
    ) {
        let linked = if ext.is_empty() { stem.clone() } else { [&stem[..], b".", &ext[..]].concat() };
        // The rule splits the linked name at its last `.`.
        let dot = linked.iter().rposition(|&c| c == b'.').unwrap_or(linked.len());
        let (st, ex) = linked.split_at(dot);
        let q = [st, b"-", &x[..], ex].concat();
        prop_assert_eq!(is_cloud_conflict_copy(&q, &linked), !x.is_empty());
        // X may not hold a `.`.
        let q_dot = [st, b"-", &x[..], b".", &x[..], ex].concat();
        prop_assert!(!is_cloud_conflict_copy(&q_dot, &linked));
    }
}
