//! The canonical spelling of an item's name and qualifier ([F08 §10.3.1] `name`, `qual`).
//!
//! [F08 §10.3.1] requires `name` to be non-empty and both `name` and `qual` to be one line, and names an `impl`'s
//! segment by "the type of an `impl`" without fixing how a type is spelled. The oracle spells every name and qualifier
//! through [`canon_into`], a function of the source text alone, so FL-1's hand-written scanner (WP-63) can reproduce
//! it token for token:
//!
//! 1. The text is split into Rust tokens. Whitespace (Rust's `Pattern_White_Space`: `09`–`0D`, `20`, U+0085, U+200E,
//!    U+200F, U+2028, U+2029) and comments (`//` to the end of the line; `/* */`, nested) separate tokens and are
//!    dropped.
//! 2. A token is one of:
//!    - a **word**: a maximal run of ASCII letters, digits, `_` and non-ASCII characters other than the whitespace
//!      above (identifiers, keywords, numbers such as `1u8` or `0x1F`); a raw identifier (`r#` directly followed by a
//!      word, as in `r#type`) is one word;
//!    - a **literal**: a string (`"…"` with `\` escapes), a raw string (`r"…"`, `r#"…"#`, any number of `#`), a byte
//!      or C string (`b"…"`, `br#"…"#`, `c"…"`, `cr"…"`), a character (`'x'`, `'\n'`, `'\u{1F600}'`) or byte
//!      character (`b'x'`), each with a directly following word as its suffix;
//!    - a **lifetime**: `'` and the word that follows it (`'a`, `'static`);
//!    - otherwise **punctuation**: one ASCII character (`<`, `:`, `&`, `#`, …).
//! 3. The tokens are written in order, each with its source text unchanged except for the bytes `0A`, `0D` and `00`:
//!    each CRLF (`0D 0A`) and each LF (`0A`) is written as the two bytes `\n` (`5C 6E`), each CR (`0D`) not followed
//!    by LF as `\r` (`5C 72`), and each NUL (`00`) as `\0` (`5C 30`). In valid Rust these bytes occur in a token only inside a
//!    literal (a string that spans lines, or a raw NUL, which rustc accepts in a literal). One space `20` separates
//!    two adjacent tokens when
//!    - both are words, literals or lifetimes;
//!    - the first is the punctuation `/` and the second is `/` or `*`, so that no comment opener is formed;
//!    - the first is the word `r`, `br` or `cr` and the second is the punctuation `#`, so that no raw string or raw
//!      identifier is formed.
//!
//!    No other separator is written.
//!
//! Examples: `Foo < T , U >` → `Foo<T,U>`; `&'a mut [T]` → `&'a mut[T]`; `dyn Fn(u8) -> u8 + Send` →
//! `dyn Fn(u8)->u8+Send`; `extern "C" fn()` → `extern "C" fn()`; `r#type` → `r#type`; `[u8; 4]` → `[u8;4]`;
//! `Foo<{ A / *B }>` → `Foo<{A/ *B}>`; `Foo<{ "a⏎b".len() }>` (⏎ a line break) → `Foo<{"a\nb".len()}>`, for LF and
//! CRLF files alike. A plain identifier is its own canonical form.
//!
//! The result contains no `00`, `0A` or `0D` byte, so it is one line as [F08 §10.3.1] requires of `name` and `qual`
//! (and free of the U+0000 that [F08 §5.3] refuses in text values), and it does not depend on the file's line endings
//! (rustc too reads a CRLF inside a literal as LF). Outside literals it contains no comment and no two adjacent spaces.
//! For text made of valid Rust tokens, the result splits (steps 1 and 2) into the same tokens as the input, so
//! `canon(canon(x)) = canon(x)`; the test `adjacent_tokens_never_merge` checks both over every sequence of up to three
//! tokens of a 48-token alphabet and of four tokens of a 12-token one. An escape reads like the escape it spells, so
//! `r"a⏎b"` and `r"a\nb"` share a spelling (in a string or character literal `\n` and `\0` mean the escaped byte
//! itself): name paths need not be unique, and the differential compares them as multisets (crate documentation,
//! "Items").
//!
//! The input is text: the oracle takes it from the name's source bytes, and bytes that are not UTF-8 are first
//! replaced by U+FFFD, one per maximal invalid subpart (Unicode §3.9, as [`String::from_utf8_lossy`] does). Such a
//! source counts as a syntax error (crate documentation, rule 3).

use std::ops::Range;

/// Appends the canonical spelling of `text` (module docs) to `out`.
///
/// The function is total: unterminated literals and comments run to the end of `text`, and a lone `'` is punctuation.
pub fn canon_into(text: &str, out: &mut String) {
    let mut prev: Option<(&str, bool)> = None;
    for (range, spaced) in Tokens::new(text) {
        // Token boundaries fall on ASCII bytes or on the end of the text, so `range` is a char boundary range.
        let token = &text[range];
        if let Some((p, p_spaced)) = prev
            && needs_space(p, p_spaced, token, spaced)
        {
            out.push(' ');
        }
        push_one_line(token, out);
        prev = Some((token, spaced));
    }
}

/// Returns the canonical spelling of `text` as a new string (tests and callers that need an owned value).
pub fn canon(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    canon_into(text, &mut out);
    out
}

/// Whether one space separates the adjacent tokens `prev` and `next` (module docs, step 3). `*_spaced` is true for a
/// word, a literal or a lifetime.
fn needs_space(prev: &str, prev_spaced: bool, next: &str, next_spaced: bool) -> bool {
    (prev_spaced && next_spaced)
        || (prev == "/" && matches!(next.as_bytes().first(), Some(b'/' | b'*')))
        || (matches!(prev, "r" | "br" | "cr") && next == "#")
}

/// Appends `token` with its line breaks and NULs escaped (module docs, step 3): CRLF and LF as `\n`, a lone CR as
/// `\r`, NUL as `\0`.
fn push_one_line(token: &str, out: &mut String) {
    let b = token.as_bytes();
    let mut i = 0;
    while let Some(p) = b[i..].iter().position(|&c| matches!(c, b'\n' | b'\r' | 0)) {
        let at = i + p;
        out.push_str(&token[i..at]);
        let (escape, len) = match (b[at], b.get(at + 1)) {
            (b'\r', Some(b'\n')) => ("\\n", 2),
            (b'\r', _) => ("\\r", 1),
            (b'\n', _) => ("\\n", 1),
            _ => ("\\0", 1),
        };
        out.push_str(escape);
        i = at + len;
    }
    out.push_str(&token[i..]);
}

/// The tokens of a text (module docs, steps 1 and 2), in order: each is its byte range in the text and whether it is
/// spaced (a word, a literal or a lifetime) rather than punctuation.
struct Tokens<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> Tokens<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            b: text.as_bytes(),
            i: 0,
        }
    }
}

impl Iterator for Tokens<'_> {
    type Item = (Range<usize>, bool);

    fn next(&mut self) -> Option<Self::Item> {
        let b = self.b;
        while self.i < b.len() {
            let i = self.i;
            if let Some(n) = whitespace_len(b, i) {
                self.i += n;
                continue;
            }
            if b[i] == b'/' {
                match b.get(i + 1) {
                    Some(b'/') => {
                        self.i = line_comment_end(b, i);
                        continue;
                    }
                    Some(b'*') => {
                        self.i = block_comment_end(b, i);
                        continue;
                    }
                    _ => {}
                }
            }
            let (end, spaced) = token_end(b, i);
            self.i = end;
            return Some((i..end, spaced));
        }
        None
    }
}

/// The length of the whitespace character at `i`, if there is one.
fn whitespace_len(b: &[u8], i: usize) -> Option<usize> {
    match b[i] {
        0x09..=0x0D | b' ' => Some(1),
        // U+0085 NEXT LINE.
        0xC2 if b.get(i + 1) == Some(&0x85) => Some(2),
        // U+200E, U+200F (marks) and U+2028, U+2029 (separators).
        0xE2 if b.get(i + 1) == Some(&0x80)
            && matches!(b.get(i + 2), Some(0x8E | 0x8F | 0xA8 | 0xA9)) =>
        {
            Some(3)
        }
        _ => None,
    }
}

/// Whether the byte at `i` continues a word: an ASCII letter, digit or `_`, or a non-ASCII byte that does not start
/// one of the non-ASCII whitespace characters.
fn is_word_byte(b: &[u8], i: usize) -> bool {
    let c = b[i];
    c.is_ascii_alphanumeric() || c == b'_' || (c >= 0x80 && whitespace_len(b, i).is_none())
}

fn word_end(b: &[u8], mut i: usize) -> usize {
    while i < b.len() && is_word_byte(b, i) {
        i += 1;
    }
    i
}

fn line_comment_end(b: &[u8], i: usize) -> usize {
    b[i..]
        .iter()
        .position(|&c| c == b'\n')
        .map_or(b.len(), |p| i + p)
}

fn block_comment_end(b: &[u8], mut i: usize) -> usize {
    let mut depth = 0usize;
    while i < b.len() {
        if b[i] == b'/' && b.get(i + 1) == Some(&b'*') {
            depth += 1;
            i += 2;
        } else if b[i] == b'*' && b.get(i + 1) == Some(&b'/') {
            depth -= 1;
            i += 2;
            if depth == 0 {
                return i;
            }
        } else {
            i += 1;
        }
    }
    b.len()
}

/// Returns the end of the token that starts at `i` and whether it is spaced (a word, literal or lifetime).
fn token_end(b: &[u8], i: usize) -> (usize, bool) {
    let c = b[i];
    if c == b'"' {
        return (with_suffix(b, escaped_end(b, i, b'"')), true);
    }
    if c == b'\'' {
        return quote_token_end(b, i);
    }
    if is_word_byte(b, i) {
        let w = word_end(b, i);
        if let Some(end) = prefixed_literal_end(b, i, w) {
            return (with_suffix(b, end), true);
        }
        // A raw identifier: `r#` and the word after it.
        if &b[i..w] == b"r" && b.get(w) == Some(&b'#') && w + 1 < b.len() && is_word_byte(b, w + 1)
        {
            return (word_end(b, w + 1), true);
        }
        return (w, true);
    }
    (i + 1, false)
}

/// A literal whose prefix is the word `b[i..w]` (`b`, `c`, `r`, `br`, `cr`), if the bytes at `w` start one.
fn prefixed_literal_end(b: &[u8], i: usize, w: usize) -> Option<usize> {
    let prefix = &b[i..w];
    let next = *b.get(w)?;
    let raw = matches!(prefix, b"r" | b"br" | b"cr");
    match next {
        b'"' if raw => Some(raw_end(b, w, 0)),
        b'"' if matches!(prefix, b"b" | b"c") => Some(escaped_end(b, w, b'"')),
        b'#' if raw => {
            let hashes = b[w..].iter().take_while(|&&h| h == b'#').count();
            (b.get(w + hashes) == Some(&b'"')).then(|| raw_end(b, w + hashes, hashes))
        }
        b'\'' if prefix == b"b" => Some(escaped_end(b, w, b'\'')),
        _ => None,
    }
}

/// The end of an escaped literal whose opening `quote` is at `open`: just after the closing quote, or the text end.
fn escaped_end(b: &[u8], open: usize, quote: u8) -> usize {
    let mut i = open + 1;
    while i < b.len() {
        match b[i] {
            b'\\' => i += 2,
            q if q == quote => return i + 1,
            _ => i += 1,
        }
    }
    b.len()
}

/// The end of a raw string whose opening `"` is at `open` and which closes with `"` and `hashes` × `#`.
fn raw_end(b: &[u8], open: usize, hashes: usize) -> usize {
    let mut i = open + 1;
    while i < b.len() {
        if b[i] == b'"'
            && b[i + 1..]
                .iter()
                .take(hashes)
                .take_while(|&&h| h == b'#')
                .count()
                == hashes
        {
            return i + 1 + hashes;
        }
        i += 1;
    }
    b.len()
}

/// A literal's suffix: a word that follows it directly.
fn with_suffix(b: &[u8], end: usize) -> usize {
    if end < b.len() && is_word_byte(b, end) {
        word_end(b, end)
    } else {
        end
    }
}

/// A token that starts with `'`: a character literal, a lifetime, or a lone `'` (punctuation).
fn quote_token_end(b: &[u8], i: usize) -> (usize, bool) {
    match b.get(i + 1) {
        Some(b'\\') => (with_suffix(b, escaped_end(b, i, b'\'')), true),
        Some(_) => {
            let after = next_char_end(b, i + 1);
            if b.get(after) == Some(&b'\'') {
                (with_suffix(b, after + 1), true)
            } else if is_word_byte(b, i + 1) {
                (word_end(b, i + 1), true)
            } else {
                (i + 1, false)
            }
        }
        None => (i + 1, false),
    }
}

/// The end of the UTF-8 character that starts at `i`.
fn next_char_end(b: &[u8], i: usize) -> usize {
    let mut j = i + 1;
    while j < b.len() && (b[j] & 0xC0) == 0x80 {
        j += 1;
    }
    j
}

#[cfg(test)]
mod tests {
    use super::{Tokens, canon};

    /// The tokens of `text`, as the canonical spelling splits them (module docs, steps 1 and 2).
    fn tokens(text: &str) -> Vec<&str> {
        Tokens::new(text).map(|(r, _)| &text[r]).collect()
    }

    #[test]
    fn identifiers_are_their_own_form() {
        for s in ["Foo", "r#type", "_", "snake_case", "Ärger", "日本"] {
            assert_eq!(canon(s), s);
        }
    }

    #[test]
    fn whitespace_between_punctuation_is_dropped() {
        assert_eq!(canon("Foo < T , U >"), "Foo<T,U>");
        assert_eq!(canon("Vec<\n    Vec<u8>,\n>"), "Vec<Vec<u8>,>");
        assert_eq!(canon("[u8; 4]"), "[u8;4]");
        assert_eq!(canon("( A , B )"), "(A,B)");
        assert_eq!(canon("crate :: a :: Foo"), "crate::a::Foo");
        assert_eq!(canon("<T as Iterator>::Item"), "<T as Iterator>::Item");
    }

    #[test]
    fn words_keep_one_space() {
        assert_eq!(canon("dyn   Trait + Send"), "dyn Trait+Send");
        assert_eq!(canon("&'a   mut\tFoo"), "&'a mut Foo");
        assert_eq!(canon("& 'static str"), "&'static str");
        assert_eq!(
            canon("dyn Fn(u8) -> u8 + Send + 'a"),
            "dyn Fn(u8)->u8+Send+'a"
        );
        assert_eq!(
            canon("unsafe  extern \"C\" fn ( )"),
            "unsafe extern \"C\" fn()"
        );
    }

    #[test]
    fn comments_are_dropped() {
        assert_eq!(canon("Foo</* x */T>"), "Foo<T>");
        assert_eq!(canon("Foo /* a /* nested */ b */ < T >"), "Foo<T>");
        assert_eq!(canon("Foo< // line\n T>"), "Foo<T>");
        assert_eq!(canon("mut/**/Foo"), "mut Foo");
        assert_eq!(canon("Foo /* unterminated"), "Foo");
    }

    /// Dropping the whitespace between `/` and `/` or `*` would open a comment; one space keeps them apart.
    #[test]
    fn a_slash_never_opens_a_comment() {
        assert_eq!(canon("Foo<{ A / *B }>"), "Foo<{A/ *B}>");
        assert_eq!(canon("A / /B"), "A/ /B");
        assert_eq!(canon("A / /* c */ *B"), "A/ *B");
        assert_eq!(canon("A */ B"), "A*/B");
        assert_eq!(canon("A / -B"), "A/-B");
        for s in ["Foo<{ A / *B }>", "A / /B"] {
            let once = canon(s);
            assert_eq!(canon(&once), once, "not a fixed point: {once:?}");
        }
    }

    #[test]
    fn literals_are_single_tokens() {
        assert_eq!(canon("Foo<{ \"a  b\" }>"), "Foo<{\"a  b\"}>");
        assert_eq!(canon("Foo<'x'>"), "Foo<'x'>");
        assert_eq!(canon("Foo<'\\''>"), "Foo<'\\''>");
        assert_eq!(canon("Foo<b'x'>"), "Foo<b'x'>");
        assert_eq!(canon("Foo<{ br#\"a \" b\"# }>"), "Foo<{br#\"a \" b\"#}>");
        assert_eq!(canon("Foo<{ r\"x\" }>"), "Foo<{r\"x\"}>");
        assert_eq!(canon("Foo<{ c\"x\" }>"), "Foo<{c\"x\"}>");
        assert_eq!(canon("Foo<{ \"x\"suffix }>"), "Foo<{\"x\"suffix}>");
        assert_eq!(canon("Foo<'é'>"), "Foo<'é'>");
        assert_eq!(canon("extern\"C\" fn()"), "extern \"C\" fn()");
        assert_eq!(canon("Foo<{ 1.0e-3 }>"), "Foo<{1.0e-3}>");
    }

    /// A literal that spans lines gives one line, the same for LF and CRLF sources; a lone CR is escaped as `\r`.
    #[test]
    fn line_breaks_in_literals_are_escaped() {
        let want = "Foo<{\"a\\nb\".len()}>";
        assert_eq!(canon("Foo<{ \"a\nb\".len() }>"), want);
        assert_eq!(canon("Foo<{ \"a\r\nb\".len() }>"), want);
        assert_eq!(canon("Foo<{ r\"a\nb\" }>"), "Foo<{r\"a\\nb\"}>");
        assert_eq!(canon("Foo<{ br#\"\r\n\r\n\"# }>"), "Foo<{br#\"\\n\\n\"#}>");
        assert_eq!(
            canon("Foo<{ \"a\rb\\\r\n  c\" }>"),
            "Foo<{\"a\\rb\\\\n  c\"}>"
        );
        assert_eq!(canon("'\n'"), "'\\n'");
        assert_eq!(canon("\"never closed\n  x\r"), "\"never closed\\n  x\\r");
        for s in [
            "Foo<{ \"a\nb\".len() }>",
            "Foo<{ \"a\rb\\\r\n  c\" }>",
            "\"never closed\n  x\r",
        ] {
            let once = canon(s);
            assert!(!once.contains(['\n', '\r']), "{once:?}");
            assert_eq!(canon(&once), once, "not a fixed point: {once:?}");
        }
    }

    /// A raw NUL, which rustc accepts inside a literal, is written as the escape `\0`, so a name never holds U+0000
    /// ([F08 §5.3]).
    #[test]
    fn nul_bytes_are_escaped() {
        assert_eq!(canon("Foo<{ \"a\0b\" }>"), "Foo<{\"a\\0b\"}>");
        assert_eq!(canon("Foo<{ r#\"\0\"# }>"), "Foo<{r#\"\\0\"#}>");
        assert_eq!(canon("Foo<'\0'>"), "Foo<'\\0'>");
        assert_eq!(canon("A\0B"), "A\\0B");
        for s in ["Foo<{ \"a\0b\" }>", "Foo<'\0'>", "\"\0\n\r\n\r\""] {
            let once = canon(s);
            assert!(!once.contains(['\0', '\n', '\r']), "{once:?}");
            assert_eq!(canon(&once), once, "not a fixed point: {once:?}");
        }
    }

    /// Invalid Rust (a lone `'`, unterminated literals) still has one defined result.
    #[test]
    fn lone_quote_and_unterminated_literals_are_total() {
        assert_eq!(canon("'"), "'");
        assert_eq!(canon("a ' b"), "a'b");
        assert_eq!(canon("\"never closed  x"), "\"never closed  x");
        assert_eq!(canon("r#\"never closed"), "r#\"never closed");
        assert_eq!(canon("'\\"), "'\\");
        assert_eq!(canon("r#"), "r #");
        assert_eq!(canon(""), "");
        assert_eq!(canon(" \t\r\n "), "");
    }

    #[test]
    fn unicode_whitespace_separates_words() {
        assert_eq!(canon("dyn\u{2028}Trait"), "dyn Trait");
        assert_eq!(canon("dyn\u{0085}Trait"), "dyn Trait");
        assert_eq!(canon("A\u{200E}<\u{200F}B>"), "A<B>");
    }

    /// `r#type` is one word; the three tokens `r # type` keep a space after `r`, so they are not read back as it, and
    /// `r # "x"` is not read back as a raw string.
    #[test]
    fn raw_identifiers_are_one_word() {
        assert_eq!(tokens("mut r#type"), ["mut", "r#type"]);
        assert_eq!(canon("mut r#type"), "mut r#type");
        assert_eq!(canon("r#type<T>"), "r#type<T>");
        assert_eq!(canon("r # type"), "r #type");
        assert_eq!(canon("br # \"x\" # \"y\""), "br #\"x\"#\"y\"");
        assert_eq!(canon("r#\"x\"# # \"y\""), "r#\"x\"##\"y\"");
        assert_eq!(canon("foo # bar"), "foo#bar");
    }

    /// Inserting whitespace or a comment between tokens never changes the result, and the result is a fixed point.
    #[test]
    fn separators_between_tokens_do_not_matter_and_canon_is_idempotent() {
        let cases: [&[&str]; 8] = [
            &["&", "'a", "mut", "[", "T", "]"],
            &["dyn", "Fn", "(", "u8", ")", "-", ">", "u8", "+", "Send"],
            &["<", "T", "as", "Iterator", ">", ":", ":", "Item"],
            &["Foo", "<", "{", "\"x y\"", "}", ">"],
            &["extern", "\"C\"", "fn", "(", ")"],
            &["Bar", "<", "'x'", ",", "b'y'", ",", "'z", ">"],
            &["A", "/", "*", "B"],
            &["A", "/", "/", "B"],
        ];
        for tokens in cases {
            let tight = tokens.join(" ");
            let expected = canon(&tight);
            assert_eq!(
                canon(&expected),
                expected,
                "not a fixed point: {expected:?}"
            );
            for (k, sep) in SEPARATORS.iter().enumerate() {
                // Vary where the separator doubles up, so every gap sees several separators.
                let mut text = String::new();
                for (j, t) in tokens.iter().enumerate() {
                    if j > 0 {
                        text.push_str(sep);
                        if (j + k) % 2 == 0 {
                            text.push_str(sep);
                        }
                    }
                    text.push_str(t);
                }
                assert_eq!(canon(&text), expected, "input {text:?}");
            }
        }
    }

    /// Separators between tokens. Each starts and ends with whitespace, as a separator after `/` must: `//` or `/*`
    /// would be a comment opener in the source itself.
    const SEPARATORS: [&str; 6] = [" ", "\n", "\r\n\t", " /* c */ ", " // c\n", "\u{2029}"];

    /// Valid Rust tokens, each one canonical token on its own: words (a raw identifier, literal prefixes, a number),
    /// literals of every form (with a suffix, byte, C and raw strings), lifetimes, and punctuation.
    const ALPHABET: [&str; 48] = [
        "A", "r", "br", "cr", "b", "c", "1", "r#x", "_", "\"x\"", "\"a b\"", "r\"x\"", "r#\"x\"#",
        "b'x'", "'x'", "\"x\"s", "c\"x\"", "br\"x\"", "'a", "'r", "/", "*", "#", "<", ">", "-",
        ".", "!", "&", ":", "=", "(", ")", "{", "}", ",", ";", "+", "?", "@", "$", "~", "^", "%",
        "|", "[", "]", "'\\n'",
    ];

    /// The tokens whose adjacency is delicate (comment openers, literal prefixes, raw strings, quotes).
    const DELICATE: [&str; 12] = [
        "r", "br", "b", "/", "*", "#", "\"x\"", "'a", "'x'", "A", "r#x", "r#\"x\"#",
    ];

    /// Checks one token sequence: the canonical spelling of the tokens, whatever separates them, splits back into the
    /// same tokens, and is therefore a fixed point.
    fn check(seq: &[&str], text: &mut String) {
        text.clear();
        for (j, t) in seq.iter().enumerate() {
            if j > 0 {
                text.push(' ');
            }
            text.push_str(t);
        }
        assert_eq!(tokens(text), seq, "the alphabet does not split cleanly");
        let once = canon(text);
        assert_eq!(
            tokens(&once),
            seq,
            "{seq:?} is read back differently from {once:?}"
        );
        assert_eq!(canon(&once), once, "{seq:?}: {once:?} is not a fixed point");
        for sep in &SEPARATORS[1..] {
            assert_eq!(canon(&seq.join(sep)), once, "{seq:?} joined by {sep:?}");
        }
    }

    /// Exhaustive over short sequences: every sequence of up to three tokens of [`ALPHABET`] and of four
    /// tokens of [`DELICATE`] keeps its tokens through the canonical spelling (module docs, step 3), so two adjacent
    /// tokens never merge into one, open a comment or split differently.
    #[test]
    fn adjacent_tokens_never_merge() {
        let mut text = String::new();
        let mut seq: Vec<&str> = Vec::with_capacity(4);
        let mut sequences = 0usize;
        for (alphabet, lengths) in [(&ALPHABET[..], 1..=3), (&DELICATE[..], 4..=4)] {
            for len in lengths {
                let mut digits = vec![0usize; len];
                'sequences: loop {
                    seq.clear();
                    seq.extend(digits.iter().map(|&d| alphabet[d]));
                    check(&seq, &mut text);
                    sequences += 1;
                    // Advance the odometer; it wraps back to all zeros after the last sequence.
                    for d in digits.iter_mut().rev() {
                        *d += 1;
                        if *d < alphabet.len() {
                            continue 'sequences;
                        }
                        *d = 0;
                    }
                    break;
                }
            }
        }
        assert_eq!(sequences, 48 + 48 * 48 + 48 * 48 * 48 + 12usize.pow(4));
    }
}
