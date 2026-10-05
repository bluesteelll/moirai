//! WP-63: the streaming Markdown and TOML scanners against line models — naive transcriptions of [F21 §4] and §5
//! that split the text into whole lines, classify each with whole-line predicates, and compute every heading's and
//! key's fields from its whole content.
//!
//! The scanners read each byte once and keep no line: a line's class is decided at its end from running predicates,
//! a possible setext underline and an ATX closing sequence are held back as counts, and a TOML line is parsed by a
//! state machine. These models keep everything, so each property checks that the streaming reading decides what the
//! line rules say, however the text is cut into chunks. Texts are synthetic: fragments of each language, arbitrary
//! bytes (invalid UTF-8, `00`, `0D`) and pieces just under, at and over [F21 §2.3]'s 4,096-byte cap.

use moirai_files::scan::{Items, Lang, ScanFailed, Scanner, scan};
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

/// An item: kind, kept name, kept qualifier, lines, parent, whether name and qualifier are long, and a Markdown
/// heading's text when neither is long ([F21 §4.7] step 3).
type Row = (
    u8,
    String,
    String,
    u64,
    u64,
    Option<usize>,
    bool,
    bool,
    Option<String>,
);

fn rows(items: &Items) -> Vec<Row> {
    items
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
                items.heading_text(i.index),
            )
        })
        .collect()
}

/// The kept bytes of a spelling ([F21 §2.3]): all of it, or its first 64 bytes cut at a character boundary.
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

/// `lossy(x)` ([F21 §1.2]).
fn lossy(x: &[u8]) -> String {
    String::from_utf8_lossy(x).into_owned()
}

/// `lines(t)` ([F20 §2.5]): split at every `0A`, the last piece dropped when empty.
fn lines(t: &[u8]) -> Vec<&[u8]> {
    let mut v: Vec<&[u8]> = t.split(|&b| b == b'\n').collect();
    if v.last().is_some_and(|l| l.is_empty()) {
        v.pop();
    }
    v
}

fn sp_ht(b: u8) -> bool {
    b == b' ' || b == b'\t'
}

fn only_sp_ht(x: &[u8]) -> bool {
    x.iter().all(|&b| sp_ht(b))
}

fn trim(x: &[u8]) -> &[u8] {
    let a = x.iter().position(|&b| !sp_ht(b)).unwrap_or(x.len());
    let z = x.iter().rposition(|&b| !sp_ht(b)).map_or(a, |z| z + 1);
    &x[a..z]
}

fn run(x: &[u8], c: u8) -> usize {
    x.iter().take_while(|&&b| b == c).count()
}

fn holds(x: &[u8], needle: &[u8]) -> bool {
    x.windows(needle.len()).any(|w| w == needle)
}

// --- Markdown, [F21 §4] -----------------------------------------------------------------------------------------

/// `ind(l)` and `rest(l)` (§4.1).
fn indent(l: &[u8]) -> (usize, &[u8]) {
    let mut col = 0usize;
    let mut k = 0;
    while let Some(&b) = l.get(k) {
        match b {
            b' ' => col += 1,
            b'\t' => col = (col / 4 + 1) * 4,
            _ => break,
        }
        k += 1;
    }
    (col, &l[k..])
}

/// An ATX heading (§4.3): level and content.
fn atx(rest: &[u8]) -> Option<(u8, Vec<u8>)> {
    let n = run(rest, b'#');
    if n == 0 || n > 6 || rest.get(n).is_some_and(|&b| !sp_ht(b)) {
        return None;
    }
    let mut c = trim(&rest[n..]);
    let h = c.iter().rev().take_while(|&&b| b == b'#').count();
    if h == c.len() {
        c = &[];
    } else if h > 0 && sp_ht(c[c.len() - h - 1]) {
        c = trim(&c[..c.len() - h]);
    }
    Some((u8::try_from(n).expect("at most 6"), c.to_vec()))
}

/// A setext underline (§4.4).
fn underline(rest: &[u8]) -> Option<u8> {
    let level = match rest.first() {
        Some(b'=') => 1,
        Some(b'-') => 2,
        _ => return None,
    };
    only_sp_ht(&rest[run(rest, rest[0])..]).then_some(level)
}

/// A fence opener (§4.5).
fn fence_open(rest: &[u8]) -> Option<(u8, usize)> {
    let c = *rest.first()?;
    let n = run(rest, c);
    ((c == b'`' || c == b'~') && n >= 3 && !(c == b'`' && rest[n..].contains(&b'`')))
        .then_some((c, n))
}

/// A thematic break (§4.2 rule 5.4).
fn thematic(rest: &[u8]) -> bool {
    rest.first().is_some_and(|&c| {
        matches!(c, b'-' | b'*' | b'_')
            && rest.iter().all(|&b| b == c || sp_ht(b))
            && rest.iter().filter(|&&b| b == c).count() >= 3
    })
}

/// A container marker (§4.6): whether it can interrupt a paragraph.
fn container(rest: &[u8]) -> Option<bool> {
    let after = |k: usize| match rest.get(k) {
        None => Some(false),
        Some(&b) if sp_ht(b) => Some(!only_sp_ht(&rest[k..])),
        Some(_) => None,
    };
    match *rest.first()? {
        b'>' => Some(true),
        b'-' | b'+' | b'*' => after(1),
        _ => {
            let d = rest.iter().take_while(|b| b.is_ascii_digit()).count();
            if !(1..=9).contains(&d) || !matches!(rest.get(d), Some(b'.' | b')')) {
                return None;
            }
            let value_one = rest[..d - 1].iter().all(|&b| b == b'0') && rest[d - 1] == b'1';
            after(d + 1).map(|content| content && value_one)
        }
    }
}

fn front(l: &[u8], closing: bool) -> bool {
    (l.starts_with(b"---") || (closing && l.starts_with(b"..."))) && only_sp_ht(&l[3..])
}

/// The numbering prefix of a collapsed content (§4.7 step 2): where the numbering ends and where the name starts.
fn numbering(c: &[u8]) -> Option<(usize, usize)> {
    let digits = |from: usize| {
        c[from.min(c.len())..]
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .count()
    };
    let mut i = 0;
    if c.starts_with("\u{a7}".as_bytes()) {
        i = 2;
        if c.get(i) == Some(&b' ') {
            i += 1;
        }
    }
    let d = digits(i);
    if d > 9 {
        return None;
    } else if d > 0 {
        i += d;
    } else if c.get(i).is_some_and(u8::is_ascii_uppercase)
        && matches!(c.get(i + 1), Some(b'.' | b')'))
    {
        i += 1;
    } else {
        return None;
    }
    while c.get(i) == Some(&b'.') {
        let d = digits(i + 1);
        if d > 9 {
            return None;
        }
        if d == 0 {
            break;
        }
        i += 1 + d;
    }
    if matches!(c.get(i), Some(b'.' | b')')) {
        i += 1;
    }
    if c.get(i) != Some(&b' ') {
        return None;
    }
    let mut name = i + 1;
    for dash in ["- ", "\u{2013} ", "\u{2014} "] {
        if c[name..].starts_with(dash.as_bytes()) {
            name += dash.len();
            break;
        }
    }
    (name < c.len()).then_some((i, name))
}

/// A heading's fields: kept name, kept qualifier, whether each is long, and its text (§4.7).
type Fields = (String, String, bool, bool, String);

/// A heading's fields from its content (§4.7).
fn fields(content: &[u8]) -> Option<Fields> {
    let mut c = Vec::new();
    for &b in content {
        if sp_ht(b) {
            if c.last() != Some(&b' ') {
                c.push(b' ');
            }
        } else {
            c.push(b);
        }
    }
    let (qual, name) = match numbering(&c) {
        Some((q, n)) => (lossy(&c[..q]), lossy(&c[n..])),
        None => (String::new(), lossy(&c)),
    };
    if name.is_empty() {
        return None;
    }
    let (name, name_long) = kept(&name);
    let (qual, qual_long) = kept(&qual);
    Some((name, qual, name_long, qual_long, lossy(&c)))
}

/// `scan(t)` of a Markdown text by the line rules of [F21 §4].
fn markdown_model(t: &[u8]) -> Vec<Row> {
    let ls = lines(t);
    let mut skip = 0;
    if ls.first().is_some_and(|l| front(l, false))
        && let Some(k) = ls.iter().skip(1).position(|l| front(l, true))
    {
        skip = k + 2;
    }
    let mut heads: Vec<(u8, Vec<u8>, u64)> = Vec::new();
    let (mut fence, mut comment, mut cont) = (None, false, false);
    let mut para: Option<(u64, Vec<&[u8]>)> = None;
    for (k, &l) in ls.iter().enumerate().skip(skip) {
        let n = k as u64 + 1;
        let (ind, rest) = indent(l);
        let shallow = ind <= 3;
        if let Some((c, len)) = fence {
            if shallow && run(rest, c) >= len && only_sp_ht(&rest[run(rest, c)..]) {
                fence = None;
            }
            continue;
        }
        if comment {
            comment = !holds(l, b"-->");
            continue;
        }
        if only_sp_ht(l) {
            para = None;
            cont = false;
            continue;
        }
        if shallow
            && let Some(level) = underline(rest)
            && let Some((first, ls)) = para.take()
        {
            let joined: Vec<Vec<u8>> = ls.iter().map(|l| trim(l).to_vec()).collect();
            heads.push((level, joined.join(&b' '), first));
            continue;
        }
        if shallow {
            if let Some((level, content)) = atx(rest) {
                heads.push((level, content, n));
                para = None;
                cont = false;
                continue;
            }
            if let Some(f) = fence_open(rest) {
                fence = Some(f);
                para = None;
                cont = false;
                continue;
            }
            if rest.starts_with(b"<!--") {
                comment = !holds(l, b"-->");
                para = None;
                cont = false;
                continue;
            }
            if thematic(rest) {
                para = None;
                cont = false;
                continue;
            }
            if let Some(interrupts) = container(rest) {
                match &mut para {
                    Some((_, ls)) if !interrupts => ls.push(l),
                    _ => {
                        para = None;
                        cont = true;
                    }
                }
                continue;
            }
        }
        if let Some((_, ls)) = &mut para {
            ls.push(l);
        } else if !cont && shallow {
            para = Some((n, vec![l]));
        }
    }
    // Levels, parents and sections (§4.8), over the headings that are items.
    let last = ls.len() as u64;
    let items: Vec<(u8, Fields, u64)> = heads
        .into_iter()
        .filter_map(|(level, content, start)| fields(&content).map(|f| (level, f, start)))
        .collect();
    let mut out = Vec::new();
    for (i, (level, (name, qual, nl, ql, text), start)) in items.iter().enumerate() {
        let parent = items[..i].iter().rposition(|it| it.0 < *level);
        let end = items[i + 1..]
            .iter()
            .find(|it| it.0 <= *level)
            .map_or(last, |it| it.2 - 1);
        let text = (!nl && !ql).then(|| text.clone());
        out.push((
            *level,
            name.clone(),
            qual.clone(),
            *start,
            end,
            parent,
            *nl,
            *ql,
            text,
        ));
    }
    out
}

// --- TOML, [F21 §5] ---------------------------------------------------------------------------------------------

/// A key at `s[i..]` (§5.2): the offset after it.
fn key(s: &[u8], i: usize) -> Option<usize> {
    match *s.get(i)? {
        b'"' => {
            let mut k = i + 1;
            loop {
                match *s.get(k)? {
                    b'\\' => k += 2,
                    b'"' => return Some(k + 1),
                    _ => k += 1,
                }
            }
        }
        b'\'' => s[i + 1..]
            .iter()
            .position(|&b| b == b'\'')
            .map(|k| i + k + 2),
        _ => {
            let n = s[i..]
                .iter()
                .take_while(|b| b.is_ascii_alphanumeric() || **b == b'-' || **b == b'_')
                .count();
            (n > 0).then_some(i + n)
        }
    }
}

fn ws(s: &[u8], mut i: usize) -> usize {
    while s.get(i).is_some_and(|&b| sp_ht(b)) {
        i += 1;
    }
    i
}

/// A key path at `s[i..]` (§5.2): its spelling and the offset after it.
fn key_path(s: &[u8], i: usize) -> Option<(Vec<u8>, usize)> {
    let mut end = key(s, i)?;
    let mut name = s[i..end].to_vec();
    loop {
        let dot = ws(s, end);
        if s.get(dot) != Some(&b'.') {
            return Some((name, end));
        }
        let from = ws(s, dot + 1);
        let Some(e) = key(s, from) else {
            return Some((name, end));
        };
        name.push(b'.');
        name.extend_from_slice(&s[from..e]);
        end = e;
    }
}

/// The open value of a key (§5.4).
#[derive(Default)]
struct ModelValue {
    stack: Vec<u8>,
    ml: Option<u8>,
}

/// The offset after the first maximal run of three or more `q` at or after `i` that no `\` takes (`"""` only).
fn ml_end(s: &[u8], mut i: usize, q: u8) -> Option<usize> {
    while i < s.len() {
        if q == b'"' && s[i] == b'\\' {
            i += 2;
        } else if s[i] == q {
            let r = run(&s[i..], q);
            if r >= 3 {
                return Some(i + r);
            }
            i += r;
        } else {
            i += 1;
        }
    }
    None
}

/// Reads line `s` from `i` inside an open value: `Ok(true)` when the value ends on it.
fn value_line(v: &mut ModelValue, s: &[u8], mut i: usize) -> Result<bool, ScanFailed> {
    loop {
        if let Some(q) = v.ml {
            let Some(j) = ml_end(s, i, q) else {
                return Ok(false);
            };
            v.ml = None;
            if v.stack.is_empty() {
                return Ok(true);
            }
            i = j;
        }
        let Some(&b) = s.get(i) else {
            return Ok(false);
        };
        match b {
            b'#' => return Ok(false),
            b'"' | b'\'' if s[i..].starts_with(&[b, b, b]) => {
                v.ml = Some(b);
                i += 3;
            }
            b'"' => {
                let mut k = i + 1;
                while k < s.len() && s[k] != b'"' {
                    k += if s[k] == b'\\' { 2 } else { 1 };
                }
                i = k + 1;
            }
            b'\'' => {
                i = s[i + 1..]
                    .iter()
                    .position(|&c| c == b'\'')
                    .map_or(s.len(), |k| i + k + 2);
            }
            b'[' | b'{' => {
                v.stack.push(b);
                if v.stack.len() > 1024 {
                    return Err(ScanFailed);
                }
                i += 1;
            }
            b']' | b'}' => {
                let open = if b == b']' { b'[' } else { b'{' };
                if let Some(k) = v.stack.iter().rposition(|&o| o == open) {
                    v.stack.truncate(k);
                    if v.stack.is_empty() {
                        return Ok(true);
                    }
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
}

/// `scan(t)` of a TOML text by the line rules of [F21 §5].
fn toml_model(t: &[u8]) -> Result<Vec<Row>, ScanFailed> {
    let ls = lines(t);
    let mut out: Vec<Row> = Vec::new();
    let mut table: Option<usize> = None;
    let mut open: Option<(usize, ModelValue)> = None;
    let item = |skind: u8, name: &[u8], n: u64, parent: Option<usize>| -> Row {
        let (name, long) = kept(&lossy(name));
        (skind, name, String::new(), n, n, parent, long, false, None)
    };
    let end_key = |out: &mut Vec<Row>, k: usize, table: Option<usize>, n: u64| {
        out[k].4 = n;
        if let Some(t) = table {
            out[t].4 = out[t].4.max(n);
        }
    };
    for (k, &l) in ls.iter().enumerate() {
        let n = k as u64 + 1;
        if let Some((key_item, mut v)) = open.take() {
            if value_line(&mut v, l, 0)? {
                end_key(&mut out, key_item, table, n);
            } else {
                open = Some((key_item, v));
            }
            continue;
        }
        let s = &l[ws(l, 0)..];
        match s.first() {
            None | Some(b'#') => {}
            Some(b'[') => {
                let array = s.starts_with(b"[[");
                let (o, c): (usize, &[u8]) = if array { (2, b"]]") } else { (1, b"]") };
                if let Some((name, e)) = key_path(s, ws(s, o)) {
                    let e = ws(s, e);
                    if s[e..].starts_with(c) {
                        let e = ws(s, e + c.len());
                        if e == s.len() || s[e] == b'#' {
                            out.push(item(if array { 2 } else { 1 }, &name, n, None));
                            table = Some(out.len() - 1);
                        }
                    }
                }
            }
            Some(_) => {
                let Some((name, e)) = key_path(s, 0) else {
                    continue;
                };
                let eq = ws(s, e);
                if s.get(eq) != Some(&b'=') {
                    continue;
                }
                out.push(item(3, &name, n, table));
                let key_item = out.len() - 1;
                let i = ws(s, eq + 1);
                let mut v = ModelValue::default();
                let ended = match s.get(i) {
                    Some(&q @ (b'"' | b'\'')) if s[i..].starts_with(&[q, q, q]) => {
                        v.ml = Some(q);
                        value_line(&mut v, s, i + 3)?
                    }
                    Some(&b @ (b'[' | b'{')) => {
                        v.stack.push(b);
                        value_line(&mut v, s, i + 1)?
                    }
                    _ => true,
                };
                if ended {
                    end_key(&mut out, key_item, table, n);
                } else {
                    open = Some((key_item, v));
                }
            }
        }
    }
    if let Some((key_item, _)) = open {
        end_key(&mut out, key_item, table, ls.len() as u64);
    }
    Ok(out)
}

// --- properties -------------------------------------------------------------------------------------------------

/// Fragments of Markdown, mixed freely.
const MARKDOWN: &[&str] = &[
    "#",
    "##",
    "###",
    "######",
    "####### ",
    " ",
    "  ",
    "   ",
    "    ",
    "\t",
    "\n",
    "\n",
    "\n",
    "\n",
    "=",
    "===",
    "-",
    "--",
    "---",
    "- - -",
    "***",
    "_ _ _",
    "```",
    "````",
    "~~~",
    "``` a `",
    "<!--",
    "-->",
    "->",
    "<!",
    "> ",
    "- ",
    "+ ",
    "* ",
    "1. ",
    "2) ",
    "01. ",
    "1234567890. ",
    "...",
    "text",
    "Para",
    "§",
    "§ ",
    "3.2",
    "3.2.",
    "A.",
    "A)",
    "A.1",
    "1)",
    "— ",
    "– ",
    "- ",
    "_",
    "é",
    " #",
    "\\#",
    "# ",
    "## x ##",
    "x",
];

/// The indentation of a generated Markdown line.
const MD_INDENT: &[&str] = &["", "", "", " ", "  ", "   ", "    ", "\t", " \t"];

/// What a generated Markdown line starts with: every construct of [F21 §4.2]'s rules, and plain text.
const MD_START: &[&str] = &[
    "",
    "",
    "Para",
    "text",
    "#",
    "# ",
    "## ",
    "###### ",
    "####### ",
    "#x",
    "=",
    "==",
    "===",
    "-",
    "--",
    "---",
    "- ",
    "- - -",
    "-- ",
    "***",
    "* ",
    "_ _ _",
    "+ ",
    "> ",
    "1. ",
    "1.",
    "2) ",
    "01) ",
    "10. ",
    "1234567890. ",
    "```",
    "```` ",
    "~~~",
    "~~~~",
    "``` a `",
    "<!--",
    "<!-- x -->",
    "<!---->",
    "-->",
    "...",
    "--- ",
    "§ 3.2 ",
    "3.2 ",
    "A. ",
    "1 - ",
    "20 — ",
];

/// What follows a generated Markdown line's start.
const MD_REST: &[&str] = &[
    " ", "  ", "\t", "x", "Para", "#", " #", "##", " ##", "=", "-", "--", " -", "`", "~", "-->",
    "->", ">", "<!--", "é", "— ", "1.", "A", "\\#",
];

/// Fragments of TOML, mixed freely.
const TOML: &[&str] = &[
    "[",
    "]",
    "[[",
    "]]",
    "=",
    " = ",
    "\"",
    "'",
    "\"\"\"",
    "'''",
    "\\",
    "#",
    "\n",
    "\n",
    "\n",
    " ",
    "\t",
    "a",
    "b.c",
    " . ",
    ".",
    "{",
    "}",
    ",",
    "1",
    "ключ",
    "\"k\"",
    "'k'",
    "x = [",
    "y = {",
    "z = \"\"\"",
    "w = '''",
    "[t]",
    "[[u]]",
    "-",
    "_",
];

/// The indentation of a generated TOML line.
const TOML_INDENT: &[&str] = &["", "", "", " ", "\t"];

/// What a generated TOML line starts with: headers, keys, comments and value continuations.
const TOML_START: &[&str] = &[
    "",
    "#",
    "[",
    "[[",
    "[ ",
    "[t]",
    "[[u]]",
    "[ a . \"b c\" ]",
    "[a.]",
    "[a]]",
    "a",
    "b-c_1",
    "\"k\"",
    "'k'",
    "\"a\\\"b\"",
    "a.b",
    "a . 'c'",
    "a.",
    "\"open",
    "]",
    "}",
    "\"\"\"",
    "'''",
    "1,",
    "ключ",
];

/// What follows a generated TOML line's start.
const TOML_REST: &[&str] = &[
    " ",
    "\t",
    "=",
    " = ",
    "1",
    "[",
    "]",
    "]]",
    "{",
    "}",
    ",",
    "\"",
    "'",
    "\"\"",
    "\"\"\"",
    "''",
    "'''",
    "\"\"\"\"",
    "\\",
    "#",
    ".",
    "a",
    "\"x\"",
    "'x'",
    "[1,",
    "{ a = 1 }",
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

/// A piece just under, at or over [F21 §2.3]'s 4,096-byte cap: a long run, word, numbering or key path.
fn long_piece() -> impl Strategy<Value = Vec<u8>> {
    (
        proptest::sample::select(&[4090usize, 4095, 4096, 4097, 6000][..]),
        proptest::sample::select(&["z", "=", "-", "#", "`", "1.", "k", " ", "é", "a."][..]),
    )
        .prop_map(|(n, p)| p.repeat(n / p.len()).into_bytes())
}

/// A text of fragments of `pieces`, arbitrary bytes, and now and then a long piece.
fn fragments(pieces: &'static [&'static str]) -> impl Strategy<Value = Vec<u8>> {
    let frag = prop_oneof![
        40 => proptest::sample::select(pieces).prop_map(|s| s.as_bytes().to_vec()),
        3 => proptest::collection::vec(any::<u8>(), 1..4),
        3 => proptest::sample::select(ODD).prop_map(<[u8]>::to_vec),
        1 => long_piece(),
    ];
    proptest::collection::vec(frag, 0..80).prop_map(|v| v.concat())
}

/// A text of lines, each an indentation, a start and a few more pieces, so that the constructs a line can start
/// with meet each other in every order; the last line may lack its `0A`.
fn line_text(
    indents: &'static [&'static str],
    starts: &'static [&'static str],
    rests: &'static [&'static str],
) -> impl Strategy<Value = Vec<u8>> {
    let piece = prop_oneof![
        30 => proptest::sample::select(rests).prop_map(|s| s.as_bytes().to_vec()),
        2 => proptest::sample::select(ODD).prop_map(<[u8]>::to_vec),
        1 => long_piece(),
    ];
    let line = (
        proptest::sample::select(indents),
        proptest::sample::select(starts),
        proptest::collection::vec(piece, 0..4),
    )
        .prop_map(|(indent, start, rest)| {
            let mut l = [indent.as_bytes(), start.as_bytes()].concat();
            l.extend(rest.concat());
            l.push(b'\n');
            l
        });
    (proptest::collection::vec(line, 0..24), any::<bool>()).prop_map(|(lines, cut)| {
        let mut t = lines.concat();
        if cut {
            t.pop();
        }
        t
    })
}

fn markdown_text() -> impl Strategy<Value = Vec<u8>> {
    prop_oneof![
        3 => line_text(MD_INDENT, MD_START, MD_REST),
        1 => fragments(MARKDOWN),
    ]
}

fn toml_text() -> impl Strategy<Value = Vec<u8>> {
    prop_oneof![
        3 => line_text(TOML_INDENT, TOML_START, TOML_REST),
        1 => fragments(TOML),
    ]
}

/// `t` fed to a scanner in chunks cut at `cuts` (offsets reduced modulo the length); `bytes` feeds it one byte at a
/// time instead.
fn chunked(lang: Lang, t: &[u8], cuts: &[usize], bytes: bool) -> Result<Items, ScanFailed> {
    let mut s = Scanner::new(lang);
    if bytes {
        for b in t.chunks(1) {
            s.feed(b);
        }
        return s.finish();
    }
    let mut at: Vec<usize> = cuts.iter().map(|&c| c % (t.len() + 1)).collect();
    at.extend([0, t.len()]);
    at.sort_unstable();
    for w in at.windows(2) {
        s.feed(&t[w[0]..w[1]]);
    }
    s.finish()
}

proptest! {
    #![proptest_config(test_config(512))]

    #[test]
    fn markdown_streams_what_the_line_rules_say(
        t in markdown_text(),
        cuts in proptest::collection::vec(any::<usize>(), 0..8),
        bytes in any::<bool>(),
    ) {
        let want = markdown_model(&t);
        let whole = scan(Lang::Markdown, &t).expect("Markdown never fails");
        prop_assert_eq!(rows(&whole), want.clone(), "{:?}", String::from_utf8_lossy(&t));
        let got = chunked(Lang::Markdown, &t, &cuts, bytes).expect("Markdown never fails");
        prop_assert_eq!(rows(&got), want);
    }

    #[test]
    fn toml_streams_what_the_line_rules_say(
        t in toml_text(),
        cuts in proptest::collection::vec(any::<usize>(), 0..8),
        bytes in any::<bool>(),
    ) {
        let want = toml_model(&t);
        let whole = scan(Lang::Toml, &t).map(|i| rows(&i));
        prop_assert_eq!(&whole, &want, "{:?}", String::from_utf8_lossy(&t));
        prop_assert_eq!(chunked(Lang::Toml, &t, &cuts, bytes).map(|i| rows(&i)), want);
    }

    #[test]
    fn byte_strings_stream_what_the_line_rules_say(t in proptest::collection::vec(any::<u8>(), 0..400)) {
        prop_assert_eq!(rows(&scan(Lang::Markdown, &t).expect("Markdown never fails")), markdown_model(&t));
        prop_assert_eq!(scan(Lang::Toml, &t).map(|i| rows(&i)), toml_model(&t));
    }
}

#[test]
fn the_models_read_the_golden_shapes() {
    // A few texts whose items the chapter states, so that the models themselves are anchored ([F21 §8.2, §8.3]).
    let md = markdown_model(b"---\nt: x\n---\n# 1 Design\n## 1.1 Storage\ntext\n\nPara\n---\n");
    let got: Vec<(u8, &str, &str, u64, u64)> = md
        .iter()
        .map(|r| (r.0, r.1.as_str(), r.2.as_str(), r.3, r.4))
        .collect();
    assert_eq!(
        got,
        [
            (1, "Design", "1", 4, 9),
            (2, "Storage", "1.1", 5, 7),
            (2, "Para", "", 8, 9)
        ]
    );
    let toml =
        toml_model(b"[package]\nname = \"x\"\nlist = [\n  1,\n]\n[[bin]]\n").expect("no failure");
    let got: Vec<(u8, &str, u64, u64, Option<usize>)> = toml
        .iter()
        .map(|r| (r.0, r.1.as_str(), r.3, r.4, r.5))
        .collect();
    assert_eq!(
        got,
        [
            (1, "package", 1, 5, None),
            (3, "name", 2, 2, Some(0)),
            (3, "list", 3, 5, Some(0)),
            (2, "bin", 6, 6, None)
        ]
    );
    let deep = format!("x = {}", "[".repeat(1025));
    assert_eq!(toml_model(deep.as_bytes()), Err(ScanFailed));
}
