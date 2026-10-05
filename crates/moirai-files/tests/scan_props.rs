//! WP-63: properties of the scope scanners ([F21 §1.1–§2.7, §3.2]) over generated texts — fragments of each
//! language mixed with arbitrary bytes, invalid UTF-8 included:
//!
//! - chunk boundaries never change a result: every chunking gives the whole-text scan ([F21 §2.8]);
//! - totality: every byte string scans without a panic, and the items keep the invariants of [F21 §2.1] (pre-order,
//!   a parent before its children, a child's range inside its parent's, `start` ≤ `end` ≤ the line count, non-empty
//!   one-line names);
//! - [F21 §2.4]'s recorded scope is recordable, names one item, and resolves to that item ([F21 §2.5]);
//! - `canon` is one line, idempotent on texts of valid tokens, and no maximal ill-formed subpart of UTF-8 spans a
//!   token boundary ([F21 §3.2] "Properties");
//! - a self-contained Rust text T placed in a group of an `impl` header keeps its items, now under the `impl`, and
//!   the header's name or qualifier is `canon` of its part with T inside ([F21 §3.7]), however the headers of T nest
//!   in it — which checks the scanner's shared spelling of nested headers against `canon`'s independent one.

use moirai_files::scan::{Items, Lang, ScanFailed, Scanner, TokenKind, canon, scan, tokens};
use proptest::prelude::*;

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

const RUST: &[&str] = &[
    "fn",
    "f",
    "g",
    "mod",
    "impl",
    "for",
    "where",
    "<",
    ">",
    "(",
    ")",
    "{",
    "}",
    "[",
    "]",
    ";",
    ",",
    "=",
    "-",
    "!",
    "#",
    "'",
    "\"",
    "r",
    "r#",
    "b",
    "br",
    "b'",
    "/",
    "*",
    "/*",
    "*/",
    "//",
    "\n",
    "\n",
    " ",
    " ",
    "\t",
    "é",
    "\u{2028}",
    "\u{85}",
    "\\",
    "a",
    "x1",
    "pub",
    "const",
    "static",
    "mut",
    "extern",
    "\"C\"",
    "macro_rules",
    "::",
    "'a",
    "'b'",
    "struct",
    "enum",
    "trait",
    "unsafe",
    "async",
    "crate",
    "safe",
    "default",
    "_",
    "1",
    "#!",
    "#[x]",
];

const MARKDOWN: &[&str] = &[
    "#", "##", "###", "####### ", " ", "  ", "    ", "\t", "\n", "\n", "\n", "===", "---", "- - -",
    "***", "```", "~~~", "``` a `", "<!--", "-->", "> ", "- ", "+ ", "* ", "1. ", "2) ", "01. ",
    "text", "Para", "§", "3.2", "A.", "A.1", "— ", "- ", "...", "_", "é", "#", " #", "\\#",
];

const TOML: &[&str] = &[
    "[", "]", "[[", "]]", "=", " = ", "\"", "'", "\"\"\"", "'''", "\\", "#", "\n", "\n", " ", "\t",
    "a", "b.c", "{", "}", ",", "1", "ключ", "\"k\"", "'k'", ".", "x = [", "y = {",
];

/// Bytes that split UTF-8 sequences or that an anchor text never holds.
const ODD: &[&[u8]] = &[
    b"\xFF",
    b"\xC2",
    b"\xE2",
    b"\xE2\x80",
    b"\x80",
    b"\x00",
    b"\r",
];

/// A text of fragments of `pieces` and arbitrary bytes.
fn text(pieces: &'static [&'static str]) -> impl Strategy<Value = Vec<u8>> {
    let frag = prop_oneof![
        8 => proptest::sample::select(pieces).prop_map(|s| s.as_bytes().to_vec()),
        1 => proptest::collection::vec(any::<u8>(), 1..4),
        1 => proptest::sample::select(ODD).prop_map(<[u8]>::to_vec),
    ];
    proptest::collection::vec(frag, 0..60).prop_map(|v| v.concat())
}

/// A piece that makes a name or qualifier just under, at or over [F21 §2.3]'s 4,096-byte cap: a word (a Rust name, a
/// TOML key, heading text), a string literal (inside an `impl` header), or a Markdown numbering.
fn long_piece() -> impl Strategy<Value = Vec<u8>> {
    (
        proptest::sample::select(&[4090usize, 4095, 4096, 4097, 6000][..]),
        0..3usize,
    )
        .prop_map(|(n, k)| match k {
            0 => "z".repeat(n).into_bytes(),
            1 => format!("\"{}\"", "y".repeat(n)).into_bytes(),
            _ => format!("{}1 ", "1.".repeat(n / 2)).into_bytes(),
        })
}

/// A text of fragments of `pieces`, arbitrary bytes and long pieces.
fn long_text(pieces: &'static [&'static str]) -> impl Strategy<Value = Vec<u8>> {
    let frag = prop_oneof![
        12 => proptest::sample::select(pieces).prop_map(|s| s.as_bytes().to_vec()),
        1 => proptest::collection::vec(any::<u8>(), 1..4),
        2 => long_piece(),
    ];
    proptest::collection::vec(frag, 0..40).prop_map(|v| v.concat())
}

/// Rust fragments with no bracket and no comment or literal left open.
const FLAT: &[&str] = &[
    "fn",
    "f",
    "g",
    "mod",
    "impl",
    "impl",
    "for",
    "for",
    "where",
    "<",
    ">",
    ";",
    ",",
    "=",
    "-",
    "!",
    "#",
    "r",
    "b",
    "/",
    "*",
    "\n",
    " ",
    "\t",
    "é",
    "a",
    "x1",
    "pub",
    "const",
    "static",
    "mut",
    "extern",
    "\"C\"",
    "macro_rules",
    "::",
    "'a",
    "'b'",
    "struct",
    "enum",
    "trait",
    "unsafe",
    "async",
    "crate",
    "_",
    "1",
    "#[x]",
    "\"s t\"",
    "r#\"q\"#",
    "/* c */",
    "// c\n",
    "r#type",
    "&",
    "dyn",
    "Tr",
    ")",
    "]",
];

/// A Rust text of flat fragments, long pieces and balanced groups, joined with or without whitespace.
fn nested_text() -> impl Strategy<Value = Vec<u8>> {
    let leaf = prop_oneof![
        24 => proptest::sample::select(FLAT).prop_map(|s| s.as_bytes().to_vec()),
        1 => long_piece(),
    ];
    leaf.prop_recursive(5, 96, 8, |inner| {
        (
            proptest::sample::select(&[("(", ")"), ("[", "]"), ("{", "}"), ("", "")][..]),
            proptest::collection::vec(
                (inner, proptest::sample::select(&[" ", "", "\n"][..])),
                0..8,
            ),
        )
            .prop_map(|((open, close), v)| {
                let mut out = open.as_bytes().to_vec();
                for (x, sep) in v {
                    out.extend(x);
                    out.extend(sep.as_bytes());
                }
                out.extend(close.as_bytes());
                out
            })
    })
}

/// Whether `t` stands alone inside a code group: it is no shebang text, no `}` of it closes a group it did not
/// open, it leaves no group open, and its tokens end where it ends (no comment or literal runs on into the text
/// after it).
fn self_contained(t: &[u8]) -> bool {
    if t.starts_with(b"#!") {
        return false;
    }
    let mut open: Vec<u8> = Vec::new();
    let alone: Vec<_> = tokens(t).collect();
    for tok in &alone {
        match tok.kind {
            TokenKind::Punct(c @ (b'(' | b'[' | b'{')) => open.push(c),
            TokenKind::Punct(c @ (b')' | b']' | b'}')) => {
                let want = match c {
                    b')' => b'(',
                    b']' => b'[',
                    _ => b'{',
                };
                match open.iter().rposition(|&o| o == want) {
                    Some(k) => open.truncate(k),
                    None if c == b'}' => return false,
                    None => {}
                }
            }
            _ => {}
        }
    }
    let mut after = t.to_vec();
    after.extend_from_slice(b" }");
    let mut with: Vec<_> = tokens(&after).collect();
    let last = with.pop();
    open.is_empty()
        && with == alone
        && last.is_some_and(|l| l.kind == TokenKind::Punct(b'}') && l.start == t.len() + 1)
}

/// The kept bytes of a spelling ([F21 §2.3]) and whether it is long.
fn kept(s: &str) -> (String, bool) {
    if s.len() <= 4096 {
        return (s.to_owned(), false);
    }
    let mut k = 64;
    while !s.is_char_boundary(k) {
        k -= 1;
    }
    (s[..k].to_owned(), true)
}

/// `t` fed in chunks cut at `cuts` (offsets reduced modulo the length).
fn chunked(lang: Lang, t: &[u8], cuts: &[usize]) -> Result<Items, ScanFailed> {
    let mut at: Vec<usize> = cuts
        .iter()
        .map(|&c| if t.is_empty() { 0 } else { c % (t.len() + 1) })
        .collect();
    at.push(0);
    at.push(t.len());
    at.sort_unstable();
    let mut s = Scanner::new(lang);
    for w in at.windows(2) {
        s.feed(&t[w[0]..w[1]]);
    }
    s.finish()
}

/// An item as a comparable tuple: kind, name, qualifier, lines, parent, and whether name and qualifier are long.
type Row = (u8, String, String, u64, u64, Option<usize>, bool, bool);

/// Items as comparable tuples.
fn rows(r: &Result<Items, ScanFailed>) -> Result<Vec<Row>, ScanFailed> {
    match r {
        Ok(items) => Ok(items
            .iter()
            .map(|i| {
                (
                    i.skind,
                    i.name.to_owned(),
                    i.qual.to_owned(),
                    i.start,
                    i.end,
                    i.parent,
                    i.name_long,
                    i.qual_long,
                )
            })
            .collect()),
        Err(e) => Err(*e),
    }
}

fn line_count(t: &[u8]) -> u64 {
    let lf = t.iter().filter(|&&b| b == b'\n').count() as u64;
    lf + u64::from(t.last().is_some_and(|&b| b != b'\n'))
}

/// The invariants of [F21 §2.1] and of §2.4/§2.5 on every item.
fn check_items(lang: Lang, t: &[u8], items: &Items) -> Result<(), TestCaseError> {
    let lines = line_count(t);
    let anchor_text = !t.contains(&0) && !t.contains(&b'\r');
    let mut prev_start = 0;
    for it in items.iter() {
        prop_assert!(lang.skind_name(it.skind).is_some());
        prop_assert!(!it.name.is_empty());
        prop_assert!(
            1 <= it.start && it.start <= it.end && it.end <= lines,
            "{it:?} of {lines} lines"
        );
        prop_assert!(it.start >= prev_start, "pre-order");
        prev_start = it.start;
        if anchor_text || lang == Lang::Rust {
            for s in [it.name, it.qual] {
                prop_assert!(!s.bytes().any(|b| matches!(b, 0 | b'\n' | b'\r')), "{s:?}");
            }
        } else {
            prop_assert!(!it.name.contains('\n') && !it.qual.contains('\n'));
        }
        if let Some(p) = it.parent {
            prop_assert!(p < it.index);
            let parent = items.get(p).expect("a parent");
            prop_assert!(
                parent.start <= it.start && it.end <= parent.end,
                "{parent:?} encloses {it:?}"
            );
        }
        if lang == Lang::Toml {
            prop_assert!(it.parent.is_none() || it.skind == 3);
        }
    }
    for q in 1..=lines.min(40) {
        if let Some(s) = items.capture_scope(q, q) {
            prop_assert!(s.len() <= 64 && s.as_bytes().len() <= 4096);
            let y = items.resolve(&s);
            prop_assert!(y.is_some(), "{s} resolves");
            let own = items.scope_of(y.expect("resolved").index);
            prop_assert_eq!(own.as_ref(), Some(&s));
            let read = moirai_files::scan::Scope::from_bytes(s.as_bytes());
            prop_assert_eq!(read.as_ref(), Ok(&s));
        }
    }
    Ok(())
}

proptest! {
    #![proptest_config(test_config(512))]

    #[test]
    fn rust_chunks_change_nothing(t in text(RUST), cuts in proptest::collection::vec(any::<usize>(), 0..8)) {
        let whole = scan(Lang::Rust, &t);
        prop_assert_eq!(rows(&chunked(Lang::Rust, &t, &cuts)), rows(&whole));
        if let Ok(items) = &whole {
            check_items(Lang::Rust, &t, items)?;
        }
    }

    #[test]
    fn markdown_chunks_change_nothing(t in text(MARKDOWN), cuts in proptest::collection::vec(any::<usize>(), 0..8)) {
        let whole = scan(Lang::Markdown, &t);
        prop_assert_eq!(rows(&chunked(Lang::Markdown, &t, &cuts)), rows(&whole));
        check_items(Lang::Markdown, &t, whole.as_ref().expect("Markdown never fails"))?;
    }

    #[test]
    fn toml_chunks_change_nothing(t in text(TOML), cuts in proptest::collection::vec(any::<usize>(), 0..8)) {
        let whole = scan(Lang::Toml, &t);
        prop_assert_eq!(rows(&chunked(Lang::Toml, &t, &cuts)), rows(&whole));
        if let Ok(items) = &whole {
            check_items(Lang::Toml, &t, items)?;
        }
    }

    #[test]
    fn long_names_change_nothing_in_chunks(
        rust_t in long_text(RUST),
        md_t in long_text(MARKDOWN),
        toml_t in long_text(TOML),
        cuts in proptest::collection::vec(any::<usize>(), 0..8),
    ) {
        // Names past the cap keep a prefix, a flag and (Rust `impl`) a bare name, whatever the chunks.
        for (lang, t) in [(Lang::Rust, &rust_t), (Lang::Markdown, &md_t), (Lang::Toml, &toml_t)] {
            let whole = scan(lang, t);
            prop_assert_eq!(rows(&chunked(lang, t, &cuts)), rows(&whole));
            if let Ok(items) = &whole {
                check_items(lang, t, items)?;
                for it in items.iter() {
                    prop_assert!(it.name.len() <= 4096 && it.qual.len() <= 4096);
                    prop_assert!(!it.name_long || it.name.len() <= 64);
                    prop_assert!(!(it.name_long || it.qual_long) || !items.recordable(it.index));
                }
            }
        }
    }

    #[test]
    fn every_byte_string_scans(t in proptest::collection::vec(any::<u8>(), 0..300)) {
        for lang in [Lang::Rust, Lang::Markdown, Lang::Toml] {
            if let Ok(items) = scan(lang, &t) {
                check_items(lang, &t, &items)?;
            }
        }
    }

    #[test]
    fn canon_is_one_line_and_replacement_stays_inside_tokens(t in text(RUST)) {
        let c = canon(&t);
        prop_assert!(!c.bytes().any(|b| matches!(b, 0 | b'\n' | b'\r')));
        // Every token starts and ends between two characters or maximal ill-formed subparts of the whole text, so
        // replacing per token equals replacing over the whole text first ([F21 §3.2] "Properties").
        let mut marks = vec![false; t.len() + 1];
        let mut at = 0;
        for chunk in t.utf8_chunks() {
            for ch in chunk.valid().chars() {
                marks[at] = true;
                at += ch.len_utf8();
            }
            marks[at] = true;
            at += chunk.invalid().len();
        }
        marks[at] = true;
        for tok in tokens(&t) {
            prop_assert!(marks[tok.start] && marks[tok.end], "{tok:?}");
        }
    }

    #[test]
    fn canon_is_idempotent_on_valid_tokens(
        v in proptest::collection::vec(
            proptest::sample::select(&[
                "fn", "r#type", "x1", "1u8", "0x1F", "é", "<", ">", "::", "(", ")", "{", "}", "[", "]", ";", ",", "&",
                "'a", "'static", "'b'", "'\\n'", "\"s\"", "\"a b\"", "b\"x\"", "r\"y\"", "br#\"z\"#", "\"x\"sfx", "/",
                "*", "#", "!", "-", "=", "+", ".", "r", "br", "cr",
            ][..]),
            0..30,
        ),
        seps in proptest::collection::vec(proptest::sample::select(&[" ", "\n", "\t", " /* c */ ", " // c\n"][..]), 30),
    ) {
        let mut x = String::new();
        for (k, tok) in v.iter().enumerate() {
            x.push_str(tok);
            x.push_str(seps[k % seps.len()]);
        }
        let c = canon(x.as_bytes());
        prop_assert_eq!(canon(c.as_bytes()), c);
    }
}

proptest! {
    #![proptest_config(test_config(128))]

    #[test]
    fn a_text_in_an_impl_header_keeps_its_items(
        t in nested_text(),
        cuts in proptest::collection::vec(any::<usize>(), 0..8),
    ) {
        if !self_contained(&t) {
            return Ok(());
        }
        let Ok(alone) = rows(&scan(Lang::Rust, &t)) else {
            return Ok(());
        };
        let end = 1 + t.iter().filter(|&&b| b == b'\n').count() as u64;
        let group = canon(&[&b"Q<{ "[..], &t, b" }>"].concat());
        // T in the self type, in the self type after a trait, in the trait, in the generic parameters (the header
        // reads no part meanwhile) and in a `where` clause (the parts have stopped).
        let cases: [(&[u8], &[u8], &str, &str); 5] = [
            (b"impl Q<{ ", b" }> {}", &group, ""),
            (b"impl Tr for Q<{ ", b" }> {}", &group, "Tr"),
            (b"impl Q<{ ", b" }> for Z {}", "Z", &group),
            (b"impl<P: W<{ ", b" }>> Q for Z {}", "Z", "Q"),
            (b"impl Q for Z where P: W<{ ", b" }> {}", "Z", "Q"),
        ];
        for (pre, post, name, qual) in cases {
            let x = [pre, &t, post].concat();
            let whole = scan(Lang::Rust, &x);
            prop_assert_eq!(rows(&chunked(Lang::Rust, &x, &cuts)), rows(&whole));
            let (name, name_long) = kept(name);
            let (qual, qual_long) = kept(qual);
            let mut want = vec![(2u8, name, qual, 1u64, end, None, name_long, qual_long)];
            want.extend(alone.iter().cloned().map(|mut r| {
                r.5 = Some(r.5.map_or(0, |p| p + 1));
                r
            }));
            prop_assert_eq!(rows(&whole), Ok(want), "{:?}", String::from_utf8_lossy(&x));
        }
    }
}
