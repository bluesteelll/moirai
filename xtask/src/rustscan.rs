//! A token-level scanner of Rust source for the GT20 (d) and (a) lints (docs/m0/PLAN.md §2.1, §6.2 R18; [AR §8.3]
//! GT20; [80 §5.5] (a)).
//!
//! It lexes one file (comments, every string and character literal form, raw identifiers and lifetimes), then
//! reports what the lints need:
//! - every `cfg` predicate (`#[cfg(..)]`, `#[cfg_attr(<pred>, ..)]`, `cfg!(..)`) that names `windows`, `unix`,
//!   `target_os`, `target_family`, `target_env` or `target_vendor`;
//! - every `allow`/`expect` attribute that names `clippy::disallowed_methods`;
//! - every path the file refers to, with the head segment resolved through the file's `use` declarations and
//!   `extern crate` renames, and `use` trees themselves (globs included), so `use std::fs; fs::rename(..)` and
//!   `use std::process::Command as C; C::new(..)` are seen as `std::fs::rename` and `std::process::Command::new`;
//! - calls of the `std::fs::File`-only lock methods (`lock_shared`, `try_lock_shared`);
//! - which tokens are test code: items under an outer attribute whose `cfg` predicate implies `test`, `#[test]` and
//!   `#[bench]` functions (any attribute path ending in `test` or `bench`), a file under `#![cfg(test)]`, and the
//!   names of out-of-line `#[cfg(test)] mod x;` modules, whose files the caller marks as test code.
//!
//! The scan is syntactic: it cannot know a receiver's type, so a `file.lock()` method call is not distinguished from
//! `mutex.lock()`, nor `path.exists()` from a namesake. The type-aware layer is clippy's `disallowed_methods`, from
//! the `clippy.toml` each product crate carries (`lint_source::check_clippy_config`); this scan is the second layer.

use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Tok {
    Ident(String),
    Punct(u8),
    PathSep,
    Lifetime,
    Lit,
}

#[derive(Clone, Debug)]
pub struct Token {
    pub tok: Tok,
    pub line: u32,
}

fn ident_start(c: char) -> bool {
    c == '_' || c.is_ascii_alphabetic() || (!c.is_ascii() && c.is_alphabetic())
}

fn ident_cont(c: char) -> bool {
    c == '_' || c.is_ascii_alphanumeric() || (!c.is_ascii() && c.is_alphanumeric())
}

/// Lexes a Rust source file into tokens; literals and comments are dropped (literals leave a `Lit` token).
pub fn lex(src: &str) -> Vec<Token> {
    let src = src.strip_prefix('\u{feff}').unwrap_or(src);
    let chars: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0usize;
    let mut line = 1u32;
    // A shebang line (not an inner attribute).
    if chars.starts_with(&['#', '!']) && chars.get(2) != Some(&'[') {
        while i < chars.len() && chars[i] != '\n' {
            i += 1;
        }
    }
    let at = |k: usize| chars.get(k).copied();
    while i < chars.len() {
        let c = chars[i];
        match c {
            '\n' => {
                line += 1;
                i += 1;
            }
            c if c.is_whitespace() => i += 1,
            '/' if at(i + 1) == Some('/') => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            '/' if at(i + 1) == Some('*') => {
                let mut depth = 0usize;
                while i < chars.len() {
                    if chars[i] == '/' && at(i + 1) == Some('*') {
                        depth += 1;
                        i += 2;
                    } else if chars[i] == '*' && at(i + 1) == Some('/') {
                        depth -= 1;
                        i += 2;
                        if depth == 0 {
                            break;
                        }
                    } else {
                        if chars[i] == '\n' {
                            line += 1;
                        }
                        i += 1;
                    }
                }
            }
            '"' => {
                out.push(Token {
                    tok: Tok::Lit,
                    line,
                });
                i = skip_string(&chars, i + 1, &mut line);
            }
            '\'' => {
                if at(i + 1) == Some('\\') {
                    out.push(Token {
                        tok: Tok::Lit,
                        line,
                    });
                    i += 2;
                    while i < chars.len() && chars[i] != '\'' {
                        if chars[i] == '\\' {
                            i += 1;
                        }
                        i += 1;
                    }
                    i += 1;
                } else if at(i + 2) == Some('\'') {
                    out.push(Token {
                        tok: Tok::Lit,
                        line,
                    });
                    i += 3;
                } else {
                    out.push(Token {
                        tok: Tok::Lifetime,
                        line,
                    });
                    i += 1;
                    while i < chars.len() && ident_cont(chars[i]) {
                        i += 1;
                    }
                }
            }
            c if c.is_ascii_digit() => {
                out.push(Token {
                    tok: Tok::Lit,
                    line,
                });
                let mut prev = c;
                i += 1;
                while i < chars.len() {
                    let d = chars[i];
                    let exp_sign = (d == '+' || d == '-')
                        && (prev == 'e' || prev == 'E')
                        && !src_is_hex(&chars, i);
                    if d.is_ascii_alphanumeric()
                        || d == '_'
                        || exp_sign
                        || (d == '.' && at(i + 1).is_some_and(|n| n.is_ascii_digit()))
                    {
                        prev = d;
                        i += 1;
                    } else {
                        break;
                    }
                }
            }
            c if ident_start(c) => {
                // Literal prefixes: b'', b"", br"", r"", r#""#, c"", cr"".
                let n1 = at(i + 1);
                let n2 = at(i + 2);
                let raw_after = |k: usize| {
                    let mut k = k;
                    while at(k) == Some('#') {
                        k += 1;
                    }
                    at(k) == Some('"')
                };
                if c == 'b' && n1 == Some('\'') {
                    out.push(Token {
                        tok: Tok::Lit,
                        line,
                    });
                    i += 2;
                    while i < chars.len() && chars[i] != '\'' {
                        if chars[i] == '\\' {
                            i += 1;
                        }
                        i += 1;
                    }
                    i += 1;
                    continue;
                }
                if (c == 'b' || c == 'c') && n1 == Some('"') {
                    out.push(Token {
                        tok: Tok::Lit,
                        line,
                    });
                    i = skip_string(&chars, i + 2, &mut line);
                    continue;
                }
                if (c == 'b' || c == 'c')
                    && n1 == Some('r')
                    && (n2 == Some('"') || (n2 == Some('#') && raw_after(i + 2)))
                {
                    out.push(Token {
                        tok: Tok::Lit,
                        line,
                    });
                    i = skip_raw(&chars, i + 2, &mut line);
                    continue;
                }
                if c == 'r' && (n1 == Some('"') || (n1 == Some('#') && raw_after(i + 1))) {
                    out.push(Token {
                        tok: Tok::Lit,
                        line,
                    });
                    i = skip_raw(&chars, i + 1, &mut line);
                    continue;
                }
                if c == 'r' && n1 == Some('#') && n2.is_some_and(ident_start) {
                    i += 2;
                }
                let start = i;
                while i < chars.len() && ident_cont(chars[i]) {
                    i += 1;
                }
                out.push(Token {
                    tok: Tok::Ident(chars[start..i].iter().collect()),
                    line,
                });
            }
            ':' if at(i + 1) == Some(':') => {
                out.push(Token {
                    tok: Tok::PathSep,
                    line,
                });
                i += 2;
            }
            c if c.is_ascii_punctuation() => {
                out.push(Token {
                    tok: Tok::Punct(c as u8),
                    line,
                });
                i += 1;
            }
            _ => i += 1,
        }
    }
    out
}

fn src_is_hex(chars: &[char], i: usize) -> bool {
    // Walk back over the number to see whether it starts with 0x.
    let mut k = i;
    while k > 0 && (chars[k - 1].is_ascii_alphanumeric() || chars[k - 1] == '_') {
        k -= 1;
    }
    chars.get(k) == Some(&'0') && matches!(chars.get(k + 1), Some('x' | 'X'))
}

fn skip_string(chars: &[char], mut i: usize, line: &mut u32) -> usize {
    while i < chars.len() {
        match chars[i] {
            '\\' => {
                if chars.get(i + 1) == Some(&'\n') {
                    *line += 1;
                }
                i += 2;
            }
            '"' => return i + 1,
            '\n' => {
                *line += 1;
                i += 1;
            }
            _ => i += 1,
        }
    }
    i
}

/// `i` points at the first `#` or the `"` after `r`.
fn skip_raw(chars: &[char], mut i: usize, line: &mut u32) -> usize {
    let mut hashes = 0;
    while chars.get(i) == Some(&'#') {
        hashes += 1;
        i += 1;
    }
    i += 1; // '"'
    while i < chars.len() {
        if chars[i] == '"' && (1..=hashes).all(|k| chars.get(i + k) == Some(&'#')) {
            return i + 1 + hashes;
        }
        if chars[i] == '\n' {
            *line += 1;
        }
        i += 1;
    }
    i
}

/// A path the file refers to, resolved through its `use` declarations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathRef {
    pub segs: Vec<String>,
    pub line: u32,
    pub test: bool,
    /// A `use` glob (`use a::b::*`): `segs` is the globbed module.
    pub glob: bool,
}

#[derive(Clone, Debug, Default)]
pub struct FileScan {
    /// `(line, predicate text)` of every OS-naming `cfg` predicate.
    pub cfg_os: Vec<(u32, String)>,
    pub paths: Vec<PathRef>,
    /// `(method, line, test)` for calls of the `std::fs::File`-only lock methods and of the process-spawning
    /// methods of `std::process::Command` (`METHODS`).
    pub methods: Vec<(String, u32, bool)>,
    /// The whole file is test code (`#![cfg(test)]`).
    pub whole_file_test: bool,
    /// Out-of-line test modules (`#[cfg(test)] mod x;`).
    pub test_modules: Vec<String>,
    /// Lines of `allow`/`expect` attributes (`cfg_attr` included) that name `clippy::disallowed_methods`, which would
    /// switch off the type-aware GT20 (d) layer (each product crate's `clippy.toml`).
    pub lint_allows: Vec<u32>,
}

/// `cfg` keys that select an operating system or its platform ABI: `windows`, `unix`, `target_os`,
/// `target_family`, `target_env` (`msvc`, `gnu`, `musl`) and `target_vendor` (`apple`, `pc`).
const OS_CFG: &[&str] = &[
    "windows",
    "unix",
    "target_os",
    "target_family",
    "target_env",
    "target_vendor",
];

/// Whether an attribute's content is an `allow` or `expect` (possibly inside `cfg_attr`) naming
/// `clippy::disallowed_methods`.
fn allows_disallowed_methods(content: &[Token]) -> bool {
    let names_lint = content.windows(3).any(|w| {
        ident(&w[0]) == Some("clippy")
            && matches!(w[1].tok, Tok::PathSep)
            && ident(&w[2]) == Some("disallowed_methods")
    });
    names_lint
        && content
            .iter()
            .any(|t| matches!(ident(t), Some("allow" | "expect")))
}
/// Method calls the lints look at: `File`'s lock calls with no namesake in `std`, and `Command`'s spawns.
pub const METHODS: &[&str] = &[
    "lock_shared",
    "try_lock_shared",
    "spawn",
    "output",
    "status",
    "exec",
];

fn ident(t: &Token) -> Option<&str> {
    match &t.tok {
        Tok::Ident(s) => Some(s),
        _ => None,
    }
}

fn is_punct(t: Option<&Token>, c: u8) -> bool {
    matches!(t, Some(Token { tok: Tok::Punct(p), .. }) if *p == c)
}

/// The index of the token that closes the bracket opened at `open`.
fn matching(toks: &[Token], open: usize) -> usize {
    let (o, c) = match toks[open].tok {
        Tok::Punct(b'(') => (b'(', b')'),
        Tok::Punct(b'[') => (b'[', b']'),
        Tok::Punct(b'{') => (b'{', b'}'),
        _ => return open,
    };
    let mut depth = 0i32;
    for (k, t) in toks.iter().enumerate().skip(open) {
        if let Tok::Punct(p) = t.tok {
            if p == o {
                depth += 1;
            } else if p == c {
                depth -= 1;
                if depth == 0 {
                    return k;
                }
            }
        }
    }
    toks.len().saturating_sub(1)
}

/// The tokens strictly between `open` and `close` (empty when the bracket is unclosed at the end).
fn between(toks: &[Token], open: usize, close: usize) -> &[Token] {
    let s = (open + 1).min(toks.len());
    let e = close.max(s).min(toks.len());
    &toks[s..e]
}

/// A `cfg` predicate as a small tree, to decide whether it implies `test`.
#[derive(Debug)]
enum Pred {
    Ident(String),
    KeyValue,
    Call(String, Vec<Pred>),
}

fn parse_preds(toks: &[Token]) -> Vec<Pred> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        match ident(&toks[i]) {
            Some(name) => {
                if is_punct(toks.get(i + 1), b'(') {
                    let close = matching(toks, i + 1);
                    out.push(Pred::Call(
                        name.to_string(),
                        parse_preds(between(toks, i + 1, close)),
                    ));
                    i = close.max(i + 1) + 1;
                } else if is_punct(toks.get(i + 1), b'=') {
                    out.push(Pred::KeyValue);
                    i += 3;
                } else {
                    out.push(Pred::Ident(name.to_string()));
                    i += 1;
                }
            }
            None => i += 1,
        }
    }
    out
}

fn implies_test(p: &Pred) -> bool {
    match p {
        Pred::Ident(s) => s == "test",
        Pred::KeyValue => false,
        Pred::Call(f, args) => match f.as_str() {
            "all" => args.iter().any(implies_test),
            "any" => !args.is_empty() && args.iter().all(implies_test),
            _ => false,
        },
    }
}

/// Classifies one attribute's content (the tokens inside `#[ ... ]`).
fn attr_is_test(content: &[Token]) -> bool {
    // #[cfg(<pred>)]
    if content.first().and_then(ident) == Some("cfg") && is_punct(content.get(1), b'(') {
        let close = matching(content, 1);
        let preds = parse_preds(between(content, 1, close));
        return preds.len() == 1 && implies_test(&preds[0]);
    }
    // #[test], #[bench], #[tokio::test], #[test_case(..)] is not a test marker by itself.
    let mut last = None;
    for t in content {
        match &t.tok {
            Tok::Ident(s) => last = Some(s.as_str()),
            Tok::PathSep => {}
            _ => break,
        }
    }
    matches!(last, Some("test" | "bench"))
}

/// The end (inclusive token index) of the item starting at `start`.
fn item_end(toks: &[Token], start: usize) -> usize {
    let mut k = start;
    while k < toks.len() {
        match toks[k].tok {
            Tok::Punct(b';') => return k,
            Tok::Punct(b'{') => return matching(toks, k),
            Tok::Punct(b'(') | Tok::Punct(b'[') => k = matching(toks, k) + 1,
            _ => k += 1,
        }
    }
    toks.len().saturating_sub(1)
}

/// Scans one file.
pub fn scan(src: &str) -> FileScan {
    let toks = lex(src);
    let mut fs = FileScan::default();
    let n = toks.len();
    let mut test = vec![false; n];

    // Attributes: test regions and cfg predicates.
    let mut depth = 0i32;
    let mut i = 0;
    while i < n {
        match toks[i].tok {
            Tok::Punct(b'{') => depth += 1,
            Tok::Punct(b'}') => depth -= 1,
            _ => {}
        }
        if is_punct(toks.get(i), b'#') {
            let inner = is_punct(toks.get(i + 1), b'!');
            let open = if inner { i + 2 } else { i + 1 };
            if is_punct(toks.get(open), b'[') {
                let close = matching(&toks, open);
                let content = between(&toks, open, close);
                if allows_disallowed_methods(content) {
                    fs.lint_allows.push(toks[i].line);
                }
                if attr_is_test(content) {
                    if inner {
                        if depth == 0 {
                            fs.whole_file_test = true;
                        }
                    } else {
                        // Skip further outer attributes, then mark the item.
                        let mut s = close + 1;
                        while is_punct(toks.get(s), b'#') && is_punct(toks.get(s + 1), b'[') {
                            s = matching(&toks, s + 1) + 1;
                        }
                        let e = item_end(&toks, s);
                        // `#[cfg(test)] mod name;`
                        let mut k = s;
                        while k < e && ident(&toks[k]).is_some_and(|w| w == "pub" || w == "crate") {
                            k += 1;
                        }
                        if ident(&toks[k.min(n - 1)]) == Some("mod")
                            && is_punct(toks.get(e), b';')
                            && let Some(name) = toks.get(k + 1).and_then(ident)
                        {
                            fs.test_modules.push(name.to_string());
                        }
                        for t in test.iter_mut().take(e + 1).skip(i) {
                            *t = true;
                        }
                    }
                }
                i = close + 1;
                continue;
            }
        }
        i += 1;
    }
    if fs.whole_file_test {
        test.iter_mut().for_each(|t| *t = true);
    }

    // cfg predicates naming an OS.
    for k in 0..n {
        let Some(name) = ident(&toks[k]) else {
            continue;
        };
        let (open, attr, shown) = match name {
            "cfg" | "cfg_attr" if is_punct(toks.get(k + 1), b'(') => {
                (k + 1, name == "cfg_attr", name)
            }
            "cfg" if is_punct(toks.get(k + 1), b'!') && is_punct(toks.get(k + 2), b'(') => {
                (k + 2, false, "cfg!")
            }
            _ => continue,
        };
        let close = matching(&toks, open);
        let mut pend = close;
        if attr {
            let mut d = 0;
            for (j, t) in toks.iter().enumerate().take(close).skip(open + 1) {
                match t.tok {
                    Tok::Punct(b'(') => d += 1,
                    Tok::Punct(b')') => d -= 1,
                    Tok::Punct(b',') if d == 0 => {
                        pend = j;
                        break;
                    }
                    _ => {}
                }
            }
        }
        let pred = between(&toks, open, pend);
        if pred
            .iter()
            .any(|t| ident(t).is_some_and(|w| OS_CFG.contains(&w)))
        {
            let text: Vec<&str> = pred.iter().filter_map(ident).collect();
            fs.cfg_os
                .push((toks[k].line, format!("{shown}({})", text.join(" "))));
        }
    }

    // use declarations and extern crate renames.
    let mut aliases: HashMap<String, Vec<String>> = HashMap::new();
    let mut in_use = vec![false; n];
    let mut k = 0;
    while k < n {
        if ident(&toks[k]) == Some("use")
            && !is_punct(k.checked_sub(1).and_then(|p| toks.get(p)), b'.')
        {
            let mut end = k + 1;
            let mut d = 0;
            while end < n {
                match toks[end].tok {
                    Tok::Punct(b'{') => d += 1,
                    Tok::Punct(b'}') => d -= 1,
                    Tok::Punct(b';') if d == 0 => break,
                    _ => {}
                }
                end += 1;
            }
            let mut items = Vec::new();
            let mut pos = k + 1;
            if matches!(toks.get(pos).map(|t| &t.tok), Some(Tok::PathSep)) {
                pos += 1;
            }
            use_tree(&toks[..end.min(n)], &mut pos, Vec::new(), &mut items);
            for (path, alias, glob) in items {
                if let Some(a) = alias
                    && a != "_"
                {
                    aliases.insert(a, path.clone());
                }
                fs.paths.push(PathRef {
                    segs: path,
                    line: toks[k].line,
                    test: test[k],
                    glob,
                });
            }
            for f in in_use.iter_mut().take(end.min(n - 1) + 1).skip(k) {
                *f = true;
            }
            k = end + 1;
            continue;
        }
        if ident(&toks[k]) == Some("extern")
            && toks.get(k + 1).and_then(ident) == Some("crate")
            && let (Some(c), Some("as"), Some(a)) = (
                toks.get(k + 2).and_then(ident),
                toks.get(k + 3).and_then(ident),
                toks.get(k + 4).and_then(ident),
            )
        {
            aliases.insert(a.to_string(), vec![c.to_string()]);
        }
        k += 1;
    }

    // Path expressions and File-only lock method calls.
    let mut k = 0;
    while k < n {
        if in_use[k] {
            k += 1;
            continue;
        }
        if is_punct(toks.get(k), b'.') {
            if let Some(m) = toks.get(k + 1).and_then(ident)
                && METHODS.contains(&m)
                && is_punct(toks.get(k + 2), b'(')
            {
                fs.methods.push((m.to_string(), toks[k + 1].line, test[k]));
            }
            k += 1;
            continue;
        }
        let lead = matches!(toks[k].tok, Tok::PathSep);
        let first = if lead { k + 1 } else { k };
        let Some(head) = toks.get(first).and_then(ident) else {
            k += 1;
            continue;
        };
        // Do not start a path in the middle of one (`a::b` is read once, from `a`).
        if !lead && k > 0 && matches!(toks[k - 1].tok, Tok::PathSep) {
            k += 1;
            continue;
        }
        let mut segs = vec![head.to_string()];
        let mut j = first + 1;
        loop {
            if !matches!(toks.get(j).map(|t| &t.tok), Some(Tok::PathSep)) {
                break;
            }
            // Turbofish `::<..>`.
            if is_punct(toks.get(j + 1), b'<') {
                let mut d = 0;
                let mut q = j + 1;
                while q < n {
                    match toks[q].tok {
                        Tok::Punct(b'<') => d += 1,
                        Tok::Punct(b'>') => {
                            d -= 1;
                            if d == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                    q += 1;
                }
                j = q + 1;
                continue;
            }
            match toks.get(j + 1).and_then(ident) {
                Some(s) => {
                    segs.push(s.to_string());
                    j += 2;
                }
                None => break,
            }
        }
        let resolved = if lead {
            segs
        } else if let Some(p) = aliases.get(&segs[0]) {
            let mut r = p.clone();
            r.extend(segs.into_iter().skip(1));
            r
        } else {
            segs
        };
        if resolved.len() > 1 || aliases.contains_key(head) {
            fs.paths.push(PathRef {
                segs: resolved,
                line: toks[first].line,
                test: test[k],
                glob: false,
            });
        }
        k = j.max(k + 1);
    }
    fs
}

/// Parses one `use` tree at `*pos` (after the `use` keyword), appending `(path, alias, glob)` items.
fn use_tree(
    toks: &[Token],
    pos: &mut usize,
    prefix: Vec<String>,
    out: &mut Vec<(Vec<String>, Option<String>, bool)>,
) {
    let mut path = prefix;
    while *pos < toks.len() {
        match &toks[*pos].tok {
            Tok::Ident(s) => {
                let s = s.clone();
                *pos += 1;
                if s == "self" && !path.is_empty() {
                    // `a::b::{self}`: the module itself.
                } else {
                    path.push(s);
                }
                match toks.get(*pos).map(|t| &t.tok) {
                    Some(Tok::PathSep) => {
                        *pos += 1;
                    }
                    Some(Tok::Ident(a)) if a == "as" => {
                        let alias = toks.get(*pos + 1).and_then(ident).map(str::to_string);
                        *pos += 2;
                        out.push((path, alias, false));
                        return;
                    }
                    _ => {
                        let alias = path.last().cloned();
                        out.push((path, alias, false));
                        return;
                    }
                }
            }
            Tok::Punct(b'*') => {
                *pos += 1;
                out.push((path, None, true));
                return;
            }
            Tok::Punct(b'{') => {
                *pos += 1;
                loop {
                    match toks.get(*pos).map(|t| &t.tok) {
                        Some(Tok::Punct(b'}')) => {
                            *pos += 1;
                            return;
                        }
                        Some(Tok::Punct(b',')) => *pos += 1,
                        None => return,
                        _ => use_tree(toks, pos, path.clone(), out),
                    }
                }
            }
            _ => {
                *pos += 1;
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(fs: &FileScan) -> Vec<String> {
        fs.paths
            .iter()
            .map(|p| {
                format!(
                    "{}{}{}",
                    p.segs.join("::"),
                    if p.glob { "::*" } else { "" },
                    if p.test { " [test]" } else { "" }
                )
            })
            .collect()
    }

    #[test]
    fn lexes_literals_and_comments_away() {
        let src = r####"
            // std::fs::rename in a comment
            /* nested /* std::os::windows */ still comment */
            let a = "std::process::Command";
            let b = r#"std::fs::rename "quoted" "#;
            let c = b'\'';
            let d = '"';
            let e = br##"x"##;
            fn f<'a>(x: &'a str) -> char { 'x' }
            let r#type = 1;
        "####;
        let fs = scan(src);
        assert!(fs.paths.is_empty(), "{:?}", fs.paths);
        let toks = lex(src);
        assert!(toks.iter().any(|t| t.tok == Tok::Ident("type".into())));
    }

    #[test]
    fn resolves_uses_and_aliases() {
        let src = "use std::fs;\nuse std::process::Command as Cmd;\nuse std::{os::windows::ffi::OsStrExt, io::{self, Read}};\nuse std::fs::*;\nfn f() { fs::rename(a, b); Cmd::new(\"x\"); let v = Vec::<u8>::new(); ::std::fs::File::lock(&f); }\n";
        let fs = scan(src);
        let p = paths(&fs);
        assert!(p.contains(&"std::fs".to_string()));
        assert!(p.contains(&"std::process::Command".to_string()));
        assert!(p.contains(&"std::os::windows::ffi::OsStrExt".to_string()));
        assert!(p.contains(&"std::io".to_string()));
        assert!(p.contains(&"std::fs::*".to_string()));
        assert!(p.contains(&"std::fs::rename".to_string()));
        assert!(p.contains(&"std::process::Command::new".to_string()));
        assert!(p.contains(&"std::fs::File::lock".to_string()));
        assert!(p.contains(&"Vec::new".to_string()));
    }

    #[test]
    fn finds_os_cfgs() {
        let src = "#[cfg(windows)]\nfn a() {}\n#[cfg(all(unix, not(test)))]\nfn b() {}\n#[cfg_attr(target_os = \"linux\", allow(x))]\nfn c() {}\nfn d() { if cfg!(target_family = \"unix\") {} }\n#[cfg_attr(test, doc = \"windows\")]\n#[cfg(feature = \"x\")]\nfn e() {}\n#[cfg(target_env = \"msvc\")]\nfn f() {}\nfn g() { cfg!(target_vendor = \"apple\"); }\n";
        let fs = scan(src);
        let lines: Vec<u32> = fs.cfg_os.iter().map(|c| c.0).collect();
        assert_eq!(lines, vec![1, 3, 5, 7, 11, 13]);
        assert_eq!(fs.cfg_os[0].1, "cfg(windows)");
        assert_eq!(fs.cfg_os[3].1, "cfg!(target_family)");
        assert_eq!(fs.cfg_os[4].1, "cfg(target_env)");
    }

    #[test]
    fn allows_of_disallowed_methods() {
        let src = "#![allow(clippy::disallowed_methods)]\n#[expect(clippy::disallowed_methods, reason = \"x\")]\nfn a() {}\n#[cfg_attr(test, allow(clippy::disallowed_methods))]\nfn b() {}\n#[deny(clippy::disallowed_methods)]\nfn c() {}\n#[allow(clippy::too_many_lines)]\nfn d() {}\n// #[allow(clippy::disallowed_methods)]\n";
        assert_eq!(scan(src).lint_allows, vec![1, 2, 4]);
    }

    #[test]
    fn marks_test_code() {
        let src = "use std::process::Command;\n#[cfg(test)]\nmod tests {\n    fn t() { std::fs::rename(a, b); }\n}\n#[test]\nfn one() { std::process::Command::new(\"x\"); }\n#[cfg(not(test))]\nfn prod() { std::fs::read(p); }\n#[cfg(test)]\nmod more;\n#[cfg(all(test, feature = \"x\"))]\nuse std::fs::File;\n";
        let fs = scan(src);
        let p = paths(&fs);
        assert!(p.contains(&"std::process::Command".to_string()));
        assert!(p.contains(&"std::fs::rename [test]".to_string()));
        assert!(p.contains(&"std::process::Command::new [test]".to_string()));
        assert!(p.contains(&"std::fs::read".to_string()));
        assert!(p.contains(&"std::fs::File [test]".to_string()));
        assert_eq!(fs.test_modules, vec!["more".to_string()]);
        let whole = scan("#![cfg(test)]\nfn x() { std::fs::read(p); }\n");
        assert!(whole.whole_file_test);
        assert!(whole.paths.iter().all(|p| p.test));
    }

    #[test]
    fn lock_methods_and_line_numbers() {
        let src = "fn f(file: &File) {\n    let s = \"multi\nline\";\n    file.lock_shared();\n    mutex.lock();\n}\n";
        let fs = scan(src);
        assert_eq!(fs.methods, vec![("lock_shared".to_string(), 4, false)]);
    }

    #[test]
    fn never_panics_on_fragments() {
        for s in [
            "#[",
            "use",
            "use std::{",
            "'",
            "r#",
            "br#\"",
            "/* open",
            "#[cfg(test)]",
            "a::<",
            "\"open",
            "b'",
            "x::",
            "#![cfg(",
            "use a as",
            "0x1e+5",
        ] {
            let _ = scan(s);
        }
    }
}
