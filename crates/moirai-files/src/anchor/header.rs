//! The header text of an item ([F20 §2.8] "Header text"): `header(l, heading)` is the normalised line l;
//! `header(l, symbol)` is the part of N from `start(l)` that the Rust tokenisation of [F21 §3.1] rules 1–4 reads up
//! to the first `{` or `;` at bracket depth 0, or to a line break at depth 0 once the header holds more than
//! qualifiers. It is a function of the anchor text and the line alone, so the resolver computes it at the hint without
//! a scanner.
//!
//! [`HeaderAcc`] computes it as N streams, in time linear in the header: the header's bytes go to an XXH3-64 state, a
//! length and the first [`HEAD`] bytes as soon as no later byte can cut before them, and only the tokens that a later
//! byte may still change are kept — a line's worth, or an unterminated literal or block comment, whose window the pass
//! charges to the budget as it grows ([`NSink::retained`]; [F20 §1.3] input (e)).

use xxhash_rust::xxh3::Xxh3Default;

use crate::r14;
use crate::scan::{LiteralKind, Token, TokenKind, tokens};
use crate::text::{NormalisedText, is_ws};

use super::nstream::{HOLD, NSink};

/// The header bytes a stream keeps: `QUOTE_MAX` and one more, which `cutp(header, QUOTE_MAX)` reads ([F20 §6.1]
/// step 3).
pub(crate) const HEAD: usize = r14::QUOTE_MAX + 1;

/// Which header a line has: a Rust or TOML item's (`symbol`) or a Markdown heading's (`heading`) ([F20 §2.8]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HeaderKind {
    /// `header(l, symbol)`: the Rust tokenisation's cut.
    Symbol,
    /// `header(l, heading)`: `nl(l)`.
    Heading,
}

/// Whether a word is one of the header qualifier words `pub`, `const`, `async`, `unsafe`, `safe`, `extern`, `default`
/// ([F20 §2.8], §7).
// spec: [F20 §2.8], [F20 §7] (header qualifier words)
fn qualifier_word(w: &[u8]) -> bool {
    matches!(
        w,
        b"pub" | b"const" | b"async" | b"unsafe" | b"safe" | b"extern" | b"default"
    )
}

/// The state of the header cut between two tokens: the bracket depth d and whether the header holds only qualifiers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CutState {
    d: u64,
    only_quals: bool,
}

impl CutState {
    const START: CutState = CutState {
        d: 0,
        only_quals: true,
    };

    /// Takes one token of `x` ([F20 §2.8] rules 1 and 2): `true` when the header ends before it.
    ///
    /// Depth counts `(` and `[` up and `)` and `]` down while positive; `<`, `>`, `{` and `}` never count. Rule 1: a
    /// `{` or `;` token at depth 0. Rule 2: a line break at depth 0 outside every comment and literal, once the header
    /// holds a token at depth 0 other than the qualifier words, a string literal, `(` and `)`. "A string literal" is
    /// read as the ABI string of [F21 §3.5] (a string or a raw string: `"…"`, `r"…"`, `br"…"`, `cr"…"`), the reading
    /// WP-63's scanner goldens use; a stray `]` at depth 0 is a token that ends the qualifier run.
    // spec: [F20 §2.8] (rules 1 and 2 of the header cut), [F21 §3.1] (tokens)
    fn cuts_before(&mut self, tok: &Token, x: &[u8]) -> bool {
        let top = self.d == 0;
        match tok.kind {
            TokenKind::Newline => return top && !self.only_quals,
            TokenKind::Punct(b'{' | b';') if top => return true,
            TokenKind::Punct(b'(') => self.d += 1,
            TokenKind::Punct(b'[') => {
                self.only_quals &= !top;
                self.d += 1;
            }
            TokenKind::Punct(b')') => self.d = self.d.saturating_sub(1),
            TokenKind::Punct(b']') => {
                if top {
                    self.only_quals = false;
                } else {
                    self.d -= 1;
                }
            }
            TokenKind::Word if top && qualifier_word(&x[tok.start..tok.end]) => {}
            TokenKind::Literal(
                LiteralKind::Str
                | LiteralKind::RawStr
                | LiteralKind::RawByteStr
                | LiteralKind::RawCStr,
            ) if top => {}
            _ => self.only_quals &= !top,
        }
        false
    }
}

/// The first cut of `header(l, symbol)` in `x` = `N[start(l) ..]` ([F20 §2.8] rules 1 and 2): the offset before which
/// the header ends, or `None` when no cut occurs before the end of `x` (rule 3).
// spec: [F20 §2.8] (rules 1–3 of the header cut), [F21 §3.1] (tokens)
#[must_use]
pub(crate) fn symbol_cut(x: &[u8]) -> Option<usize> {
    let mut st = CutState::START;
    tokens(x)
        .find(|tok| st.cuts_before(tok, x))
        .map(|tok| tok.start)
}

/// `x` without its trailing `WS` bytes ([F20 §2.8]: "trailing WS bytes of the result are removed").
// spec: [F20 §2.8] (trailing WS removed)
fn trim_end(x: &[u8]) -> &[u8] {
    let z = x.iter().rposition(|&b| !is_ws(b)).map_or(0, |z| z + 1);
    &x[..z]
}

/// The header that starts `x` = `N[start(l) ..]` ([F20 §2.8]): for `heading`, the line up to its `0A`; for `symbol`,
/// up to the first cut, without trailing `WS`.
// spec: [F20 §2.8]
#[must_use]
pub(crate) fn header_from(x: &[u8], kind: HeaderKind) -> &[u8] {
    match kind {
        HeaderKind::Heading => {
            let z = x.iter().position(|&b| b == b'\n').unwrap_or(x.len());
            &x[..z]
        }
        HeaderKind::Symbol => trim_end(&x[..symbol_cut(x).unwrap_or(x.len())]),
    }
}

/// `header(l, kind)` of a normalised anchor text held in memory ([F20 §2.8]); `None` when line `l` does not exist.
// spec: [F20 §2.8]
#[must_use]
pub fn header(n: &NormalisedText, l: usize, kind: HeaderKind) -> Option<&[u8]> {
    let s = n.start(l)?;
    Some(header_from(&n.bytes()[s..], kind))
}

/// A header as a stream computes it ([F20 §2.8]): what a caller reads of `header(l, kind)` — its XXH3-64 (the header
/// span hash), its length (the header range of [F21 §2.6]) and its first [`HEAD`] bytes (the quote).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HeaderText {
    /// `XXH3-64(header(l, kind))`.
    pub(crate) hash: u64,
    /// `len(header(l, kind))`.
    pub(crate) len: u64,
    head: Vec<u8>,
}

impl HeaderText {
    /// The header's first `min(len, HEAD)` bytes.
    pub(crate) fn head(&self) -> &[u8] {
        &self.head
    }

    /// The text of a header held whole.
    #[cfg(test)]
    pub(crate) fn of(x: &[u8]) -> HeaderText {
        HeaderText {
            hash: xxhash_rust::xxh3::xxh3_64(x),
            len: x.len() as u64,
            head: x[..x.len().min(HEAD)].to_vec(),
        }
    }
}

/// The header bytes taken so far.
#[derive(Clone)]
struct Out {
    h: Xxh3Default,
    len: u64,
    head: Vec<u8>,
}

impl Out {
    fn new() -> Out {
        Out {
            h: Xxh3Default::new(),
            len: 0,
            head: Vec::new(),
        }
    }

    fn push(&mut self, b: &[u8]) {
        self.h.update(b);
        self.len += b.len() as u64;
        if self.head.len() < HEAD {
            let n = (HEAD - self.head.len()).min(b.len());
            self.head.extend_from_slice(&b[..n]);
        }
    }

    fn text(&self) -> HeaderText {
        HeaderText {
            hash: self.h.digest(),
            len: self.len,
            head: self.head.clone(),
        }
    }
}

/// The header bytes taken so far with their trailing `WS` held back, so that the result is trimmed ([F20 §2.8]:
/// "trailing WS bytes of the result are removed"): up to [`HOLD`] bytes of a run are held; past that the state before
/// the run is kept once, the run is taken, and the kept state is restored if nothing but `WS` follows.
#[derive(Clone)]
struct Trimmed {
    out: Out,
    hold: [u8; HOLD],
    held: usize,
    snap: Option<Out>,
}

impl Trimmed {
    fn new() -> Trimmed {
        Trimmed {
            out: Out::new(),
            hold: [0; HOLD],
            held: 0,
            snap: None,
        }
    }

    // spec: [F20 §2.8] (trailing WS removed; WS before a later byte of the header is kept)
    fn push(&mut self, b: &[u8]) {
        let Some(z) = b.iter().rposition(|&x| !is_ws(x)) else {
            self.ws(b);
            return;
        };
        if self.snap.take().is_none() && self.held > 0 {
            self.out.push(&self.hold[..self.held]);
        }
        self.held = 0;
        self.out.push(&b[..=z]);
        if z + 1 < b.len() {
            self.ws(&b[z + 1..]);
        }
    }

    fn ws(&mut self, run: &[u8]) {
        if run.is_empty() {
            return;
        }
        if self.snap.is_some() {
            self.out.push(run);
        } else if self.held + run.len() <= HOLD {
            self.hold[self.held..self.held + run.len()].copy_from_slice(run);
            self.held += run.len();
        } else {
            self.snap = Some(self.out.clone());
            self.out.push(&self.hold[..self.held]);
            self.held = 0;
            self.out.push(run);
        }
    }

    fn finish(&mut self) -> HeaderText {
        if let Some(s) = self.snap.take() {
            self.out = s;
        }
        self.held = 0;
        self.out.text()
    }
}

/// `header(l, kind)` computed as N streams ([`NSink`]).
///
/// A `heading` header is line l: its bytes are taken as they arrive. A `symbol` header is decided token by token at
/// the end of each line from l on. At a line's end the byte after the bytes seen is that line's `0A` (or N ends), and
/// the tokenisation of [F21 §3.1] reads past a line break only inside a literal, a block comment or the character
/// literal `'⏎'`; so every token but the last is then final, and a cut found then is final. The window kept is the
/// bytes from the restart point on: the start of the last token when it reaches the end of the line (a literal or a
/// `'` that may continue), else the end of the last token (what follows is whitespace or a comment). The bytes before
/// the restart point are taken, and the tokenisation restarts there with the depth and the qualifier flag of that
/// point. A window that holds no token, or only one token reaching its end, cannot move: it is tokenised again only
/// when it has doubled, so a header inside a long literal or block comment costs linear time, and memory as long as
/// that literal or comment. That window is reported as [`NSink::retained`], so the pass charges it to the budget as it
/// grows and a literal or comment left open to the end of a large file ends as `Unavailable(budget)` rather than
/// holding the file (the lexer of [F21 §3.1] restarts only at a token's start, and its streaming state belongs to the
/// scope scanners).
#[derive(Clone)]
pub(crate) struct HeaderAcc {
    line: u64,
    kind: HeaderKind,
    active: bool,
    out: Trimmed,
    win: Vec<u8>,
    state: CutState,
    /// The window's length when it was last tokenised without moving; 0 when it moved since.
    lexed: usize,
    done: Option<HeaderText>,
}

impl core::fmt::Debug for HeaderAcc {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("HeaderAcc")
            .field("line", &self.line)
            .field("kind", &self.kind)
            .field("done", &self.done)
            .finish_non_exhaustive()
    }
}

impl HeaderAcc {
    pub(crate) fn new(line: u64, kind: HeaderKind) -> HeaderAcc {
        HeaderAcc {
            line,
            kind,
            active: false,
            out: Trimmed::new(),
            win: Vec::new(),
            state: CutState::START,
            lexed: 0,
            done: None,
        }
    }

    /// The header once decided; call [`HeaderAcc::finish`] at the end of N first. `None` when line l does not exist.
    pub(crate) fn header(&self) -> Option<&HeaderText> {
        self.done.as_ref()
    }

    /// The end of N: a header still open runs to it (rule 3).
    pub(crate) fn finish(&mut self) {
        if self.active && self.done.is_none() {
            match self.kind {
                HeaderKind::Heading => self.close(),
                HeaderKind::Symbol => self.cut(true),
            }
        }
    }

    fn close(&mut self) {
        self.done = Some(self.out.finish());
        self.active = false;
        self.win = Vec::new();
    }

    /// Tokenises the window ([F20 §2.8] rules 1–3): a cut ends the header; otherwise the bytes before the restart
    /// point are taken (struct doc). `eof` at the end of N, where the window's end is the text's.
    // spec: [F20 §2.8] (the header cut, streamed)
    fn cut(&mut self, eof: bool) {
        if !eof && self.lexed > 0 && self.win.len() < 2 * self.lexed {
            return;
        }
        let mut st = self.state;
        // The last token and the state before it.
        let mut last: Option<(Token, CutState)> = None;
        let mut cut = None;
        for tok in tokens(&self.win) {
            let before = st;
            if st.cuts_before(&tok, &self.win) {
                cut = Some(tok.start);
                break;
            }
            last = Some((tok, before));
        }
        if let Some(c) = cut.or(eof.then_some(self.win.len())) {
            self.out.push(&self.win[..c]);
            self.close();
            return;
        }
        let (keep, at) = match last {
            None => (self.state, 0),
            Some((tok, before)) if tok.end == self.win.len() => (before, tok.start),
            Some((tok, _)) => (st, tok.end),
        };
        self.out.push(&self.win[..at]);
        self.win.drain(..at);
        self.state = keep;
        self.lexed = if at == 0 { self.win.len() } else { 0 };
    }
}

impl NSink for HeaderAcc {
    fn start_line(&mut self, line: u64, _at: u64) {
        if line == self.line && self.done.is_none() {
            self.active = true;
        }
    }

    fn bytes(&mut self, _at: u64, b: &[u8]) {
        if self.active {
            match self.kind {
                HeaderKind::Heading => self.out.push(b),
                HeaderKind::Symbol => self.win.extend_from_slice(b),
            }
        }
    }

    fn end_line(&mut self, line: u64, _at: u64) {
        if !self.active || line < self.line {
            return;
        }
        match self.kind {
            HeaderKind::Heading => self.close(),
            HeaderKind::Symbol => self.cut(false),
        }
    }

    // spec: [F20 §1.3] input (e) (the window a later byte may still change, charged as it grows)
    fn retained(&self) -> usize {
        self.win.capacity()
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::anchor::nstream::NStream;
    use crate::oid::ObjectFormat;
    use crate::text::{analyse, atext};

    fn h(src: &str, l: usize) -> String {
        let n = NormalisedText::new(src.as_bytes()).unwrap();
        String::from_utf8(header(&n, l, HeaderKind::Symbol).unwrap().to_vec()).unwrap()
    }

    #[test]
    fn the_examples_of_f20_2_8() {
        assert_eq!(h("pub(crate)\nfn f() {", 1), "pub(crate)\nfn f()");
        assert_eq!(
            h("pub const K: [u8; 4] = [0; 4];", 1),
            "pub const K: [u8; 4] = [0; 4]"
        );
        assert_eq!(h("fn f(a: [u8; 4]) -> u32 {", 1), "fn f(a: [u8; 4]) -> u32");
        assert_eq!(
            h("pub fn f(\n    a: u8,\n) -> u32 {", 1),
            "pub fn f(\na: u8,\n) -> u32"
        );
        assert_eq!(h("[package]\nname = \"x\"", 1), "[package]");
        assert_eq!(h("[package]\nname = \"x\"", 2), "name = \"x\"");
        // A brace or semicolon inside a comment or a literal counts for nothing.
        assert_eq!(
            h("fn f() /* { */ -> &'static str {", 1),
            "fn f() /* { */ -> &'static str"
        );
        assert_eq!(h("const S: &str = \"a;b\";", 1), "const S: &str = \"a;b\"");
        // Qualifier lines continue; an ABI string is a qualifier; `b"C"` is not.
        assert_eq!(
            h("pub\nunsafe extern \"C\"\nfn f() {}", 1),
            "pub\nunsafe extern \"C\"\nfn f()"
        );
        assert_eq!(h("extern b\"C\"\nfn f() {}", 1), "extern b\"C\"");
        // `'⏎'` is a character literal: the line break inside it is no cut.
        assert_eq!(h("fn f(c = '\n') {", 1), "fn f(c = '\n')");
        // Rule 3: an unclosed group runs to the end of N.
        assert_eq!(h("fn f(\na\nb", 1), "fn f(\na\nb");
        let n = NormalisedText::new(b"# Title\ntext").unwrap();
        assert_eq!(header(&n, 1, HeaderKind::Heading), Some(&b"# Title"[..]));
        assert_eq!(header(&n, 3, HeaderKind::Heading), None);
    }

    /// The streaming header of line `l` over the anchor text of `b`.
    fn streamed(b: &[u8], l: u64, kind: HeaderKind) -> Option<HeaderText> {
        let mut s = NStream::new(HeaderAcc::new(l, kind));
        let _ = analyse(b, ObjectFormat::Sha1, None, &mut s);
        let mut acc = s.into_sink();
        acc.finish();
        acc.header().cloned()
    }

    /// The streaming header against the whole-text one at the first `upto` lines of `b` (and one past the last) for
    /// both kinds.
    fn agrees(b: &[u8], upto: usize) -> Result<(), TestCaseError> {
        let Some(t) = atext(b) else { return Ok(()) };
        let n = NormalisedText::new(&t).unwrap();
        let last = n.line_count() as u64 + 1;
        for l in (1..=last.min(upto as u64)).chain([last]) {
            for kind in [HeaderKind::Symbol, HeaderKind::Heading] {
                let want = header(&n, l as usize, kind).map(HeaderText::of);
                prop_assert_eq!(streamed(b, l, kind), want, "line {} {:?}", l, kind);
            }
        }
        Ok(())
    }

    #[test]
    fn long_headers_stream_in_linear_time() {
        // An unclosed parenthesis runs the header to the end of N: 20,000 lines are tokenised once each.
        let mut b = b"fn f(\n".to_vec();
        for i in 0..20_000 {
            b.extend_from_slice(format!("    a{i}: u8,\n").as_bytes());
        }
        let n = NormalisedText::new(&b).unwrap();
        let want = HeaderText::of(header(&n, 1, HeaderKind::Symbol).unwrap());
        assert_eq!(want.len as usize, n.bytes().len());
        assert_eq!(streamed(&b, 1, HeaderKind::Symbol), Some(want));
        // An unterminated string, raw string and (nested) block comment hold their window; it is tokenised as it
        // doubles, and the streamed header equals the whole-text one.
        for open in [
            &b"const S: &str = \"x\n"[..],
            b"const R: &str = r##\"x\"#\n",
            b"fn g() /* c\n",
            b"fn h() /* a /* b */ c\n",
        ] {
            let mut b = open.to_vec();
            for _ in 0..5_000 {
                b.extend_from_slice(b"text { ; ( ]\n");
            }
            agrees(&b, 3).unwrap();
        }
        // Trailing whitespace longer than the hold, at the cut and inside.
        let mut b = b"pub\n".to_vec();
        b.extend(std::iter::repeat_n(b'\n', 3 * HOLD));
        b.extend_from_slice(b"fn h()   ");
        b.extend(std::iter::repeat_n(b' ', 2 * HOLD));
        b.extend_from_slice(b"-> u8 {\n");
        agrees(&b, usize::MAX).unwrap();
    }

    proptest! {
        #[test]
        fn streaming_equals_the_whole_text(
            src in proptest::collection::vec(proptest::sample::select(vec![
                &b"fn"[..], b"pub", b" ", b"
    ", b"(", b")", b"[", b"]", b"{", b";", b"'", b"\"", b"//", b"/*",
                b"*/", b"r#\"", b"\"#", b"x", b"extern", b"	", b"\\"]), 0..40),
            l in 1u64..6,
        ) {
            let b: Vec<u8> = src.concat();
            let Some(t) = atext(&b) else { return Ok(()) };
            let n = NormalisedText::new(&t).unwrap();
            for kind in [HeaderKind::Symbol, HeaderKind::Heading] {
                let want = header(&n, l as usize, kind).map(HeaderText::of);
                prop_assert_eq!(streamed(&b, l, kind), want);
            }
        }

        /// Long multi-line headers — open groups, literals and comments across many lines, long lines, whitespace runs
        /// — stream to the whole-text header at every line.
        #[test]
        fn long_multi_line_headers_stream_exactly(
            lines in proptest::collection::vec(proptest::collection::vec(proptest::sample::select(vec![
                &b"fn"[..], b"pub", b"const", b" ", b"(", b")", b"[", b"]", b"{", b";", b"'", b"\"", b"//", b"/*",
                b"*/", b"r#\"", b"\"#", b"x", b"extern", b"\t", b"\\", b"'a'", b"b\"", b"r##\"", b"\"##",
                b"abcdefghijklmnopqrstuvwxyz0123456789", b"     "]), 0..24), 1..80),
        ) {
            let b: Vec<u8> = lines.iter().map(|l| l.concat()).collect::<Vec<_>>().join(&b'\n');
            agrees(&b, usize::MAX)?;
        }
    }
}
