//! The LQ lexer: source decoding ([LQ/lexical §2]), whitespace and comments (§3), the normal mode (§5) and the revision
//! mode (§7), with the codes E001–E003 of §1.
//!
//! The parser drives the lexer ([LQ/grammar-v1.ebnf §P.2]): it asks for one normal-mode token at a byte position, or
//! for one `revspec` / `rev_arg` at a revision position. Tokens carry values, never allocate for words (a word is a
//! byte range of the source), and every lexical error ends the pass (§1).

use crate::lq::ast::{RangeOp, Rev, RevKind, Suffix};
use crate::lq::diag::{Code, Diag, Span, q, value};
use std::borrow::Cow;

/// Strips one leading byte-order mark and checks that the rest is well-formed UTF-8 ([LQ/lexical §2.1]). Offsets of
/// every later span are into the returned text.
pub fn decode(bytes: &[u8]) -> Result<&str, Diag> {
    let body = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    match std::str::from_utf8(body) {
        Ok(s) => Ok(s),
        Err(e) => {
            let at = e.valid_up_to() as u32;
            Err(Diag::new(
                Code::E003,
                Span::new(at, at + 1),
                format!("invalid UTF-8 at byte {at}"),
            )
            .help(
                "send UTF-8; from PowerShell pass the query with -f FILE or through the query tool",
            ))
        }
    }
}

/// Punctuation tokens ([LQ/lexical §5.9]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Punct {
    /// `..`
    DotDot,
    /// `->`
    Arrow,
    /// `<-` (only before `[` or `-`)
    LArrow,
    /// `<>`
    Ne,
    /// `<=`
    Le,
    /// `>=`
    Ge,
    /// `!=`
    BangEq,
    /// `=~` (lexed so that the parser can refuse it)
    EqTilde,
    /// `(`
    LParen,
    /// `)`
    RParen,
    /// `[`
    LBracket,
    /// `]`
    RBracket,
    /// `{`
    LBrace,
    /// `}`
    RBrace,
    /// `,`
    Comma,
    /// `;`
    Semi,
    /// `.`
    Dot,
    /// `:`
    Colon,
    /// `|`
    Pipe,
    /// `+`
    Plus,
    /// `-`
    Minus,
    /// `*`
    Star,
    /// `/`
    Slash,
    /// `=`
    Eq,
    /// `<`
    Lt,
    /// `>`
    Gt,
    /// `?`
    Question,
    /// `%` (lexed so that the parser can refuse it)
    Percent,
}

impl Punct {
    /// The punctuation's bytes.
    pub fn as_str(self) -> &'static str {
        match self {
            Punct::DotDot => "..",
            Punct::Arrow => "->",
            Punct::LArrow => "<-",
            Punct::Ne => "<>",
            Punct::Le => "<=",
            Punct::Ge => ">=",
            Punct::BangEq => "!=",
            Punct::EqTilde => "=~",
            Punct::LParen => "(",
            Punct::RParen => ")",
            Punct::LBracket => "[",
            Punct::RBracket => "]",
            Punct::LBrace => "{",
            Punct::RBrace => "}",
            Punct::Comma => ",",
            Punct::Semi => ";",
            Punct::Dot => ".",
            Punct::Colon => ":",
            Punct::Pipe => "|",
            Punct::Plus => "+",
            Punct::Minus => "-",
            Punct::Star => "*",
            Punct::Slash => "/",
            Punct::Eq => "=",
            Punct::Lt => "<",
            Punct::Gt => ">",
            Punct::Question => "?",
            Punct::Percent => "%",
        }
    }
}

/// A normal-mode token kind with its value ([LQ/lexical §5.1]). Tokens hold no text: a word, a back-quoted name or a
/// string is its source range, decoded only when the parser consumes it ([`Lexer::qident_value`],
/// [`Lexer::string_value`]), so peeking and backtracking copy a few bytes and never allocate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokKind {
    /// A word; its text is the source range.
    Word,
    /// A back-quoted identifier; the lexer has checked it and [`Lexer::qident_value`] decodes it.
    QIdent,
    /// A parameter; its name is the source range after `$`.
    Param,
    /// An integer.
    Int(i64),
    /// A float; its text is the source range.
    Float,
    /// A duration in milliseconds; its text is the source range.
    Dur(i64),
    /// A string; the lexer has checked it and [`Lexer::string_value`] decodes it.
    Str,
    /// A node literal `#N`.
    Node(u32),
    /// A uid literal; the 32 hex digits are the source range after `#u:`.
    Uid,
    /// Punctuation.
    Punct(Punct),
    /// End of input.
    Eof,
}

/// A token with its byte range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Token {
    /// Kind and value.
    pub kind: TokKind,
    /// First byte.
    pub start: usize,
    /// One past the last byte.
    pub end: usize,
}

impl Token {
    /// The token's span.
    pub fn span(&self) -> Span {
        Span::new(self.start as u32, self.end as u32)
    }
}

/// One classified token of the fixture token stream ([LQ/lexical §11]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TokOut {
    /// `KW`, `NAME`, `QNAME`, `PARAM`, `INT`, `FLOAT`, `DUR`, `STR`, `NODE`, `UID`, `P`, `HEAD`, `REF`, `COMMIT`,
    /// `SEQ`, `SUF`, `RANGE` or `EOF`.
    pub kind: &'static str,
    /// The printed value: borrowed for punctuation, keywords and the other fixed texts, so that a stream of a million
    /// tokens holds only the values it must.
    pub value: Cow<'static, str>,
}

/// A JSON string by [LQ/lexical §11.1] (also [LQ/canonical-ast §4]).
pub fn json_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                out.push_str(&format!("\\u{:04x}", c as u32))
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// What a revision position read ([LQ/lexical §7.1], §7.4).
#[derive(Clone, Debug)]
pub enum RevRead {
    /// One revspec.
    Rev(Rev),
    /// `a..b` or `a...b` (argument positions only).
    Range(Rev, RangeOp, Rev, Span),
    /// `[r, ...]` (argument positions only).
    List(Vec<Rev>, Span),
    /// A quote at an argument position: revision mode ends and the parser reads an expression.
    Quote,
}

/// Whether a byte continues a word (`[A-Za-z0-9_]`).
pub fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Whether a byte starts a word (`[A-Za-z_]`).
pub fn is_word_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

fn is_ref_word_start(b: u8) -> bool {
    b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'
}

fn is_ref_word_byte(b: u8) -> bool {
    is_ref_word_start(b) || b == b'-'
}

/// A control character refused inside strings and back-quoted names ([LQ/lexical §5.2]).
fn is_refused_control(c: char) -> bool {
    matches!(c as u32, 0x00..=0x08 | 0x0B | 0x0C | 0x0E..=0x1F | 0x7F)
}

/// The lexer over a decoded source text.
#[derive(Clone, Copy)]
pub struct Lexer<'a> {
    src: &'a str,
    b: &'a [u8],
}

impl<'a> Lexer<'a> {
    /// A lexer over `src` (already decoded by [`decode`]).
    pub fn new(src: &'a str) -> Lexer<'a> {
        Lexer {
            src,
            b: src.as_bytes(),
        }
    }

    /// The source text.
    pub fn src(&self) -> &'a str {
        self.src
    }

    /// The source text of a byte range.
    pub fn text(&self, start: usize, end: usize) -> &'a str {
        &self.src[start..end]
    }

    fn at(&self, i: usize) -> u8 {
        self.b.get(i).copied().unwrap_or(0)
    }

    fn span(start: usize, end: usize) -> Span {
        Span::new(start as u32, end as u32)
    }

    /// Skips whitespace and comments ([LQ/lexical §3]); an unclosed block comment is E002 at its `/*`.
    pub fn skip_trivia(&self, mut i: usize) -> Result<usize, Diag> {
        loop {
            match self.at(i) {
                b' ' | b'\t' | b'\n' | b'\r' if i < self.b.len() => i += 1,
                b'/' if self.at(i + 1) == b'/' => {
                    while i < self.b.len() && self.b[i] != b'\n' {
                        i += 1;
                    }
                }
                b'/' if self.at(i + 1) == b'*' => {
                    let open = i;
                    i += 2;
                    loop {
                        if i >= self.b.len() {
                            return Err(Diag::new(
                                Code::E002,
                                Self::span(open, open + 2),
                                "unterminated block comment",
                            ));
                        }
                        if self.b[i] == b'*' && self.at(i + 1) == b'/' {
                            i += 2;
                            break;
                        }
                        i += 1;
                    }
                }
                _ => return Ok(i),
            }
        }
    }

    /// Whether only whitespace (no comment) lies between `i` and the next token-like byte; returns that position.
    fn skip_spaces(&self, mut i: usize) -> usize {
        while i < self.b.len() && matches!(self.b[i], b' ' | b'\t' | b'\n' | b'\r') {
            i += 1;
        }
        i
    }

    /// The first scalar at `i` and its length.
    fn char_at(&self, i: usize) -> (char, usize) {
        let c = self.src[i..].chars().next().unwrap_or('\0');
        (c, c.len_utf8())
    }

    /// Lexes one normal-mode token after skipping whitespace and comments.
    pub fn token(&self, pos: usize) -> Result<Token, Diag> {
        let i = self.skip_trivia(pos)?;
        if i >= self.b.len() {
            return Ok(Token {
                kind: TokKind::Eof,
                start: i,
                end: i,
            });
        }
        let c = self.b[i];
        let punct = |p: Punct, len: usize| {
            Ok(Token {
                kind: TokKind::Punct(p),
                start: i,
                end: i + len,
            })
        };
        match c {
            b'A'..=b'Z' | b'a'..=b'z' | b'_' => {
                let mut j = i + 1;
                while is_word_byte(self.at(j)) {
                    j += 1;
                }
                Ok(Token {
                    kind: TokKind::Word,
                    start: i,
                    end: j,
                })
            }
            b'0'..=b'9' => self.number(i),
            b'`' => self.qident(i),
            b'$' => {
                if !is_word_start(self.at(i + 1)) {
                    return Err(Diag::new(
                        Code::E001,
                        Self::span(i, i + 1),
                        "$ must be followed by a parameter name",
                    )
                    .help("parameter names start with a letter or _"));
                }
                let mut j = i + 2;
                while is_word_byte(self.at(j)) {
                    j += 1;
                }
                Ok(Token {
                    kind: TokKind::Param,
                    start: i,
                    end: j,
                })
            }
            b'#' => self.node(i),
            b'\'' | b'"' => {
                let end = self.scan_string(i, None)?;
                Ok(Token {
                    kind: TokKind::Str,
                    start: i,
                    end,
                })
            }
            b'.' if self.at(i + 1) == b'.' => punct(Punct::DotDot, 2),
            b'.' => punct(Punct::Dot, 1),
            b'-' if self.at(i + 1) == b'>' => punct(Punct::Arrow, 2),
            b'-' => punct(Punct::Minus, 1),
            b'<' if self.at(i + 1) == b'-' && matches!(self.at(i + 2), b'[' | b'-') => {
                punct(Punct::LArrow, 2)
            }
            b'<' if self.at(i + 1) == b'>' => punct(Punct::Ne, 2),
            b'<' if self.at(i + 1) == b'=' => punct(Punct::Le, 2),
            b'<' => punct(Punct::Lt, 1),
            b'>' if self.at(i + 1) == b'=' => punct(Punct::Ge, 2),
            b'>' => punct(Punct::Gt, 1),
            b'!' if self.at(i + 1) == b'=' => punct(Punct::BangEq, 2),
            b'!' => Err(
                Diag::new(Code::E001, Self::span(i, i + 1), "! is not an operator")
                    .inline("write NOT <expr> or write <>"),
            ),
            b'=' if self.at(i + 1) == b'~' => punct(Punct::EqTilde, 2),
            b'=' => punct(Punct::Eq, 1),
            b'(' => punct(Punct::LParen, 1),
            b')' => punct(Punct::RParen, 1),
            b'[' => punct(Punct::LBracket, 1),
            b']' => punct(Punct::RBracket, 1),
            b'{' => punct(Punct::LBrace, 1),
            b'}' => punct(Punct::RBrace, 1),
            b',' => punct(Punct::Comma, 1),
            b';' => punct(Punct::Semi, 1),
            b':' => punct(Punct::Colon, 1),
            b'|' => punct(Punct::Pipe, 1),
            b'+' => punct(Punct::Plus, 1),
            b'*' => punct(Punct::Star, 1),
            b'/' => punct(Punct::Slash, 1),
            b'?' => punct(Punct::Question, 1),
            b'%' => punct(Punct::Percent, 1),
            _ => Err(self.bad_char(i)),
        }
    }

    /// E001 for a character that starts no token ([LQ/lexical §5.2]).
    pub fn bad_char(&self, i: usize) -> Diag {
        let (c, len) = self.char_at(i);
        let d = Diag::new(
            Code::E001,
            Self::span(i, i + len),
            format!("{} cannot start a token here", q(&c.to_string())),
        );
        if matches!(c, '^' | '~' | '@') {
            d.help("revisions follow USE, TX ON, IF TIP or a revision argument of diff, log, changes, history, across or violations")
        } else {
            d
        }
    }

    /// Numbers and durations ([LQ/lexical §5.6]).
    fn number(&self, i: usize) -> Result<Token, Diag> {
        let mut j = i;
        while self.at(j).is_ascii_digit() {
            j += 1;
        }
        let mut float = false;
        if self.at(j) == b'.' && self.at(j + 1).is_ascii_digit() {
            j += 1;
            while self.at(j).is_ascii_digit() {
                j += 1;
            }
            float = true;
        }
        if matches!(self.at(j), b'e' | b'E') {
            let k = j + 1;
            if self.at(k).is_ascii_digit()
                || (matches!(self.at(k), b'+' | b'-') && self.at(k + 1).is_ascii_digit())
            {
                j = if self.at(k).is_ascii_digit() {
                    k
                } else {
                    k + 1
                };
                while self.at(j).is_ascii_digit() {
                    j += 1;
                }
                float = true;
            } else {
                let mut end = if matches!(self.at(k), b'+' | b'-') {
                    k + 1
                } else {
                    k
                };
                while is_word_byte(self.at(end)) {
                    end += 1;
                }
                return Err(Diag::new(
                    Code::E003,
                    Self::span(i, end),
                    "an exponent needs digits",
                ));
            }
        }
        let mut dur_unit = None;
        if !float
            && matches!(self.at(j), b's' | b'm' | b'h' | b'd' | b'w')
            && !is_word_byte(self.at(j + 1))
        {
            dur_unit = Some(self.at(j));
            j += 1;
        }
        if is_word_byte(self.at(j)) {
            let mut end = j;
            while is_word_byte(self.at(end)) {
                end += 1;
            }
            let (c, _) = self.char_at(j);
            return Err(Diag::new(
                Code::E003,
                Self::span(i, end),
                format!("{} runs into {}", q(&self.src[i..j]), q(&c.to_string())),
            )
            .help("separate the token from the next one"));
        }
        let text = &self.src[i..j];
        let out_of_range = || {
            Diag::new(
                Code::E003,
                Self::span(i, j),
                format!("{} is out of range", q(text)),
            )
        };
        if float {
            let v: f64 = text.parse().map_err(|_| out_of_range())?;
            if !v.is_finite() {
                return Err(out_of_range());
            }
            return Ok(Token {
                kind: TokKind::Float,
                start: i,
                end: j,
            });
        }
        let digits = if dur_unit.is_some() {
            &text[..text.len() - 1]
        } else {
            text
        };
        let n = parse_decimal(digits)
            .filter(|&n| n <= i64::MAX as u64)
            .ok_or_else(out_of_range)? as i64;
        match dur_unit {
            None => Ok(Token {
                kind: TokKind::Int(n),
                start: i,
                end: j,
            }),
            Some(u) => {
                let unit: i64 = match u {
                    b's' => 1_000,
                    b'm' => 60_000,
                    b'h' => 3_600_000,
                    b'd' => 86_400_000,
                    _ => 604_800_000,
                };
                let ms = n.checked_mul(unit).ok_or_else(out_of_range)?;
                Ok(Token {
                    kind: TokKind::Dur(ms),
                    start: i,
                    end: j,
                })
            }
        }
    }

    /// Node and uid literals ([LQ/lexical §5.8]).
    fn node(&self, i: usize) -> Result<Token, Diag> {
        let bad = |end: usize| {
            Diag::new(
                Code::E003,
                Self::span(i, end),
                format!("{} is not a node literal", q(&self.src[i..end])),
            )
            .help("write #<digits> or #u:<32 lower-case hex>")
        };
        let run_end = |mut k: usize| {
            while is_word_byte(self.at(k)) {
                k += 1;
            }
            k
        };
        if self.at(i + 1).is_ascii_digit() {
            let mut j = i + 1;
            while self.at(j).is_ascii_digit() {
                j += 1;
            }
            if is_word_byte(self.at(j)) {
                return Err(bad(run_end(j)));
            }
            let digits = &self.src[i + 1..j];
            match parse_decimal(digits) {
                Some(n) if (1..=u32::MAX as u64).contains(&n) => Ok(Token {
                    kind: TokKind::Node(n as u32),
                    start: i,
                    end: j,
                }),
                _ => Err(Diag::new(
                    Code::E003,
                    Self::span(i, j),
                    format!("{} is out of range (1 to 4294967295)", q(&self.src[i..j])),
                )
                .help("write #<digits> or #u:<32 lower-case hex>")),
            }
        } else if self.at(i + 1) == b'u' && self.at(i + 2) == b':' {
            let start = i + 3;
            let mut j = start;
            while matches!(self.at(j), b'0'..=b'9' | b'a'..=b'f') {
                j += 1;
            }
            if j - start != 32 || is_word_byte(self.at(j)) {
                return Err(bad(run_end(j).max(i + 3)));
            }
            Ok(Token {
                kind: TokKind::Uid,
                start: i,
                end: j,
            })
        } else {
            Err(bad(run_end(i + 1).max(i + 1)))
        }
    }

    /// A back-quoted identifier ([LQ/lexical §5.4]).
    fn qident(&self, i: usize) -> Result<Token, Diag> {
        let end = self.scan_qident(i, None)?;
        Ok(Token {
            kind: TokKind::QIdent,
            start: i,
            end,
        })
    }

    /// Checks the back-quoted identifier at `i` and returns its end; with `out`, also decodes it there.
    fn scan_qident(&self, i: usize, mut out: Option<&mut String>) -> Result<usize, Diag> {
        let mut j = i + 1;
        loop {
            if j >= self.b.len() || matches!(self.b[j], b'\n' | b'\r') {
                return Err(Diag::new(
                    Code::E002,
                    Self::span(i, i + 1),
                    "unterminated back-quoted name",
                ));
            }
            if self.b[j] == b'`' {
                if self.at(j + 1) == b'`' {
                    if let Some(o) = out.as_deref_mut() {
                        o.push('`');
                    }
                    j += 2;
                    continue;
                }
                if j == i + 1 {
                    return Err(Diag::new(
                        Code::E001,
                        Self::span(i, j + 1),
                        "an empty back-quoted name",
                    ));
                }
                return Ok(j + 1);
            }
            let (c, len) = self.char_at(j);
            if is_refused_control(c) {
                return Err(control_diag(c, j, len));
            }
            if let Some(o) = out.as_deref_mut() {
                o.push(c);
            }
            j += len;
        }
    }

    /// The decoded name of a [`TokKind::QIdent`] token.
    pub fn qident_value(&self, t: &Token) -> String {
        let mut out = String::with_capacity(t.end - t.start);
        // The token was checked when it was lexed.
        let _ = self.scan_qident(t.start, Some(&mut out));
        out
    }

    /// The decoded value of a [`TokKind::Str`] token.
    pub fn string_value(&self, t: &Token) -> String {
        let mut out = String::with_capacity(t.end - t.start);
        // The token was checked when it was lexed.
        let _ = self.scan_string(t.start, Some(&mut out));
        out
    }

    /// A string literal ([LQ/lexical §5.7]); returns the decoded value and the end offset.
    pub fn string(&self, i: usize) -> Result<(String, usize), Diag> {
        let mut out = String::new();
        let end = self.scan_string(i, Some(&mut out))?;
        Ok((out, end))
    }

    /// Checks the string literal at `i` and returns its end; with `out`, also decodes its value there.
    fn scan_string(&self, i: usize, mut out: Option<&mut String>) -> Result<usize, Diag> {
        let quote = self.b[i];
        let mut j = i + 1;
        let mut push = |c: char| {
            if let Some(o) = out.as_deref_mut() {
                o.push(c);
            }
        };
        let unterminated = || {
            Diag::new(Code::E002, Self::span(i, i + 1), "unterminated string")
                .help("close it with ' or \" on the same line; write a line break as \\n")
        };
        loop {
            if j >= self.b.len() || matches!(self.b[j], b'\n' | b'\r') {
                return Err(unterminated());
            }
            let c = self.b[j];
            if c == quote {
                return Ok(j + 1);
            }
            if c == b'\\' {
                let e = self.at(j + 1);
                if j + 1 >= self.b.len() {
                    return Err(unterminated());
                }
                match e {
                    b'\\' => push('\\'),
                    b'\'' => push('\''),
                    b'"' => push('"'),
                    b'n' => push('\n'),
                    b'r' => push('\r'),
                    b't' => push('\t'),
                    b'u' if self.at(j + 2) == b'{' => {
                        let mut k = j + 3;
                        while self.at(k).is_ascii_hexdigit() && k - (j + 3) < 7 {
                            k += 1;
                        }
                        let n = k - (j + 3);
                        if !(1..=6).contains(&n) || self.at(k) != b'}' {
                            return Err(Diag::new(
                                Code::E003,
                                Self::span(j, j + 2),
                                "unknown escape \\u",
                            )
                            .help("write \\u{<hex>}"));
                        }
                        let hex = &self.src[j + 3..k];
                        let v = u32::from_str_radix(hex, 16).unwrap_or(u32::MAX);
                        match char::from_u32(v) {
                            Some(ch) => push(ch),
                            None => {
                                return Err(Diag::new(
                                    Code::E003,
                                    Self::span(j, k + 1),
                                    format!("\\u{{{hex}}} is not a Unicode scalar value"),
                                )
                                .help("write \\u{<hex>}"));
                            }
                        }
                        j = k + 1;
                        continue;
                    }
                    _ => {
                        let (ec, elen) = self.char_at(j + 1);
                        if matches!(ec, '\n' | '\r') {
                            return Err(unterminated());
                        }
                        return Err(Diag::new(
                            Code::E003,
                            Self::span(j, j + 1 + elen),
                            format!("unknown escape \\{}", value(&ec.to_string(), 64)),
                        )
                        .help("write \\u{<hex>}"));
                    }
                }
                j += 2;
                continue;
            }
            let (ch, len) = self.char_at(j);
            if is_refused_control(ch) {
                return Err(control_diag(ch, j, len));
            }
            push(ch);
            j += len;
        }
    }

    /// Whether the bytes at `i` (after whitespace and comments) are a word followed by `:` — a named argument at the
    /// start of an argument ([LQ/grammar-v1.ebnf §P.10]). Returns the word's range and the position after `:`.
    pub fn named_arg_at(&self, i: usize) -> Option<(usize, usize, usize)> {
        let s = self.skip_trivia(i).ok()?;
        if !is_word_start(self.at(s)) {
            return None;
        }
        let mut e = s + 1;
        while is_word_byte(self.at(e)) {
            e += 1;
        }
        let c = self.skip_trivia(e).ok()?;
        (self.at(c) == b':').then_some((s, e, c + 1))
    }

    /// Reads one revision position ([LQ/lexical §7]): a `revspec`, or in argument positions (`arg` true) a `rev_arg`.
    /// Classified tokens go to `out`. Returns what was read and the position after it.
    pub fn revision(
        &self,
        pos: usize,
        arg: bool,
        out: &mut Vec<TokOut>,
    ) -> Result<(RevRead, usize), Diag> {
        let i = self.skip_trivia(pos)?;
        match self.at(i) {
            b'\'' | b'"' if i < self.b.len() => {
                if arg {
                    return Ok((RevRead::Quote, i));
                }
                let (value, end) = self.string(i)?;
                return Err(Diag::new(
                    Code::E001,
                    Self::span(i, end),
                    "a revision here is written unquoted",
                )
                .inline(format!("write {}", value)));
            }
            b'[' if arg => return self.rev_list(i, out),
            _ => {}
        }
        let (first, e1) = self.revspec(i, out)?;
        if !arg {
            return Ok((RevRead::Rev(first), e1));
        }
        let k = self.skip_spaces(e1);
        if self.at(k) == b'.' && self.at(k + 1) == b'.' {
            let mut dots = 0;
            while self.at(k + dots) == b'.' {
                dots += 1;
            }
            if dots > 3 {
                return Err(Diag::new(
                    Code::E001,
                    Self::span(k, k + dots),
                    "a range is a..b or a...b",
                ));
            }
            let op = if dots == 3 {
                RangeOp::Three
            } else {
                RangeOp::Two
            };
            out.push(TokOut {
                kind: "RANGE",
                value: if dots == 3 { "...".into() } else { "..".into() },
            });
            let m = self.skip_spaces(k + dots);
            if self.at(m) == b'[' || matches!(self.at(m), b'\'' | b'"') {
                return Err(Diag::new(
                    Code::E001,
                    Self::span(m, m + 1),
                    "a range is a..b or a...b",
                ));
            }
            let (second, e2) = self.revspec(m, out)?;
            let span = first.span.to(second.span);
            return Ok((RevRead::Range(first, op, second, span), e2));
        }
        Ok((RevRead::Rev(first), e1))
    }

    fn rev_list(&self, i: usize, out: &mut Vec<TokOut>) -> Result<(RevRead, usize), Diag> {
        let empty = |a: usize, b: usize| {
            Diag::new(
                Code::E001,
                Self::span(a, b),
                "a revision list holds one or more revisions and no ranges",
            )
        };
        out.push(TokOut {
            kind: "P",
            value: "[".into(),
        });
        let mut j = self.skip_trivia(i + 1)?;
        if self.at(j) == b']' {
            return Err(empty(i, j + 1));
        }
        let mut elems = Vec::new();
        loop {
            let (r, e) = self.revspec(j, out)?;
            elems.push(r);
            j = self.skip_trivia(e)?;
            match self.at(j) {
                b',' => {
                    out.push(TokOut {
                        kind: "P",
                        value: ",".into(),
                    });
                    j = self.skip_trivia(j + 1)?;
                }
                b']' => {
                    out.push(TokOut {
                        kind: "P",
                        value: "]".into(),
                    });
                    return Ok((RevRead::List(elems, Self::span(i, j + 1)), j + 1));
                }
                b'.' if self.at(j + 1) == b'.' => return Err(empty(j, j + 2)),
                _ if j >= self.b.len() => {
                    return Err(Diag::new(
                        Code::E001,
                        Self::span(j, j),
                        "expected `,`, `]`, found end of input",
                    ));
                }
                _ => {
                    let t = self.token(j)?;
                    return Err(Diag::new(
                        Code::E001,
                        t.span(),
                        format!("expected `,`, `]`, found {}", q(&self.src[t.start..t.end])),
                    ));
                }
            }
        }
    }

    /// One `revspec`: a base and its suffixes ([LQ/lexical §7.2]–§7.3, §7.6).
    pub fn revspec(&self, i: usize, out: &mut Vec<TokOut>) -> Result<(Rev, usize), Diag> {
        let c = self.at(i);
        let expected = || {
            let end = if i < self.b.len() {
                i + self.char_at(i).1
            } else {
                i
            };
            Diag::new(
                Code::E001,
                Self::span(i, end),
                "expected a revision: a ref, c<hex>, s<seq>, HEAD or $param",
            )
        };
        if i >= self.b.len() {
            return Err(expected());
        }
        let (mut rev, mut j) = if c == b'$' {
            if !is_word_start(self.at(i + 1)) {
                return Err(Diag::new(
                    Code::E001,
                    Self::span(i, i + 1),
                    "$ must be followed by a parameter name",
                )
                .help("parameter names start with a letter or _"));
            }
            let mut j = i + 2;
            while is_word_byte(self.at(j)) {
                j += 1;
            }
            if matches!(self.at(j), b'~' | b'^' | b'@') {
                return Err(Diag::new(
                    Code::E001,
                    Self::span(j, j + 1),
                    "a $param revision takes no suffix",
                )
                .help("pass the full revision in the parameter"));
            }
            self.no_suffix_after_trivia(j)?;
            let name = self.src[i + 1..j].to_string();
            out.push(TokOut {
                kind: "PARAM",
                value: name.clone().into(),
            });
            return Ok((
                Rev {
                    kind: RevKind::Param(name),
                    span: Self::span(i, j),
                },
                j,
            ));
        } else if self.src[i..].starts_with("HEAD") && !self.head_continues(i + 4) {
            out.push(TokOut {
                kind: "HEAD",
                value: "HEAD".into(),
            });
            (
                Rev {
                    kind: RevKind::Head,
                    span: Self::span(i, i + 4),
                },
                i + 4,
            )
        } else if is_ref_word_start(c) {
            let mut j = i;
            loop {
                while is_ref_word_byte(self.at(j)) {
                    j += 1;
                }
                if matches!(self.at(j), b'.' | b'/') && is_ref_word_start(self.at(j + 1)) {
                    j += 1;
                    continue;
                }
                break;
            }
            let name = &self.src[i..j];
            let single = !name.contains(['/', '.']);
            let bytes = name.as_bytes();
            let kind = if single
                && bytes[0] == b'c'
                && (8..=65).contains(&bytes.len())
                && bytes[1..]
                    .iter()
                    .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
            {
                out.push(TokOut {
                    kind: "COMMIT",
                    value: name[1..].to_string().into(),
                });
                RevKind::Commit(name[1..].to_string())
            } else if single
                && bytes[0] == b's'
                && bytes.len() > 1
                && bytes[1..].iter().all(u8::is_ascii_digit)
            {
                let n = parse_decimal(&name[1..]).ok_or_else(|| {
                    Diag::new(
                        Code::E003,
                        Self::span(i, j),
                        format!("{} is out of range", q(name)),
                    )
                })?;
                out.push(TokOut {
                    kind: "SEQ",
                    value: n.to_string().into(),
                });
                RevKind::Seq(n)
            } else {
                out.push(TokOut {
                    kind: "REF",
                    value: name.to_string().into(),
                });
                RevKind::Ref(name.to_string())
            };
            (
                Rev {
                    kind,
                    span: Self::span(i, j),
                },
                j,
            )
        } else if c.is_ascii_uppercase() {
            return Err(Diag::new(
                Code::E003,
                Self::span(i, i + 1),
                "a revision has an upper-case letter",
            )
            .help("ref names are lower case; HEAD is upper case"));
        } else {
            return Err(expected());
        };
        loop {
            let s = j;
            match self.at(j) {
                b'~' | b'^' if j < self.b.len() => {
                    let tilde = self.at(j) == b'~';
                    let (n, e) = self.suffix_count(j + 1)?;
                    let n = n.unwrap_or(1);
                    out.push(TokOut {
                        kind: "SUF",
                        value: match (tilde, n) {
                            (true, 1) => "~1".into(),
                            (false, 1) => "^1".into(),
                            _ => format!("{}{}", if tilde { '~' } else { '^' }, n).into(),
                        },
                    });
                    let suf = if tilde {
                        Suffix::Tilde(n)
                    } else {
                        Suffix::Caret(n)
                    };
                    j = e;
                    rev = Rev {
                        span: Self::span(rev.span.start as usize, j),
                        kind: RevKind::Suf(Box::new(rev), suf),
                    };
                }
                b'@' if j < self.b.len() => {
                    let k = j + 1;
                    if self.at(k) == b'{' && self.at(k + 1).is_ascii_digit() {
                        let (n, e) = self.suffix_count(k + 1)?;
                        if self.at(e) != b'}' {
                            return Err(Diag::new(
                                Code::E001,
                                Self::span(s, e),
                                "a bare @ is not a revision",
                            )
                            .inline("write HEAD, <ref>@<n> or <ref>@<datetime>"));
                        }
                        let n = n.unwrap_or(1);
                        out.push(TokOut {
                            kind: "SUF",
                            value: format!("@{n}").into(),
                        });
                        j = e + 1;
                        rev = Rev {
                            span: Self::span(rev.span.start as usize, j),
                            kind: RevKind::Suf(Box::new(rev), Suffix::At(n)),
                        };
                    } else if self.is_date_at(k) {
                        let (text, e) = self.datetime(k)?;
                        out.push(TokOut {
                            kind: "SUF",
                            value: format!("@{text}").into(),
                        });
                        j = e;
                        rev = Rev {
                            span: Self::span(rev.span.start as usize, j),
                            kind: RevKind::Suf(Box::new(rev), Suffix::AtTime(text)),
                        };
                    } else if self.at(k).is_ascii_digit() {
                        let (n, e) = self.suffix_count(k)?;
                        let n = n.unwrap_or(1);
                        out.push(TokOut {
                            kind: "SUF",
                            value: format!("@{n}").into(),
                        });
                        j = e;
                        rev = Rev {
                            span: Self::span(rev.span.start as usize, j),
                            kind: RevKind::Suf(Box::new(rev), Suffix::At(n)),
                        };
                    } else {
                        return Err(Diag::new(
                            Code::E001,
                            Self::span(j, j + 1),
                            "a bare @ is not a revision",
                        )
                        .inline("write HEAD, <ref>@<n> or <ref>@<datetime>"));
                    }
                }
                _ => break,
            }
        }
        if is_word_byte(self.at(j)) {
            let mut e = j;
            while is_word_byte(self.at(e)) {
                e += 1;
            }
            return Err(Diag::new(
                Code::E003,
                Self::span(i, e),
                format!("{} is malformed", q(&self.src[i..e])),
            ));
        }
        self.no_suffix_after_trivia(j)?;
        Ok((rev, j))
    }

    /// [LQ/lexical §7.6]: whitespace or a comment ends a revision, so a suffix written after them (`main /* x */ ~2`)
    /// is E001 with the text of [LQ/errors §5.2], at the suffix's byte. An unclosed comment is left to the next read.
    fn no_suffix_after_trivia(&self, j: usize) -> Result<(), Diag> {
        if !matches!(self.at(j), b' ' | b'\t' | b'\n' | b'\r' | b'/') {
            return Ok(());
        }
        match self.skip_trivia(j) {
            Ok(k) if k > j && k < self.b.len() && matches!(self.b[k], b'~' | b'^' | b'@') => {
                Err(Diag::new(
                    Code::E001,
                    Self::span(k, k + 1),
                    "a revision ends at whitespace or a comment",
                ))
            }
            _ => Ok(()),
        }
    }

    /// Whether `HEAD` continues into a longer word at `j` ([LQ/lexical §7.2]): a letter, digit, `_`, `-` or `/`, or a
    /// `.` that starts a ref word (a range operator `..` after `HEAD` does not continue it).
    fn head_continues(&self, j: usize) -> bool {
        let c = self.at(j);
        is_word_byte(c)
            || c == b'-'
            || c == b'/'
            || (c == b'.' && is_ref_word_start(self.at(j + 1)))
    }

    fn suffix_count(&self, i: usize) -> Result<(Option<u32>, usize), Diag> {
        let mut j = i;
        while self.at(j).is_ascii_digit() {
            j += 1;
        }
        if j == i {
            return Ok((None, i));
        }
        match parse_decimal(&self.src[i..j]) {
            Some(n) if n <= u32::MAX as u64 => Ok((Some(n as u32), j)),
            _ => Err(Diag::new(
                Code::E003,
                Self::span(i, j),
                "a suffix count is out of range",
            )),
        }
    }

    fn is_date_at(&self, k: usize) -> bool {
        (0..4).all(|d| self.at(k + d).is_ascii_digit())
            && self.at(k + 4) == b'-'
            && self.at(k + 5).is_ascii_digit()
            && self.at(k + 6).is_ascii_digit()
            && self.at(k + 7) == b'-'
            && self.at(k + 8).is_ascii_digit()
            && self.at(k + 9).is_ascii_digit()
    }

    /// A datetime after `@` ([LQ/lexical §7.5]); returns the normalised text `YYYY-MM-DDTHH:MM:SSZ`.
    fn datetime(&self, k: usize) -> Result<(String, usize), Diag> {
        let num = |a: usize, n: usize| -> u32 { self.src[a..a + n].parse().unwrap_or(0) };
        let (y, mo, d) = (num(k, 4), num(k + 5, 2), num(k + 8, 2));
        let mut j = k + 10;
        let (mut h, mut mi, mut s) = (0, 0, 0);
        if self.at(j) == b'T'
            && self.at(j + 1).is_ascii_digit()
            && self.at(j + 2).is_ascii_digit()
            && self.at(j + 3) == b':'
            && self.at(j + 4).is_ascii_digit()
            && self.at(j + 5).is_ascii_digit()
        {
            h = num(j + 1, 2);
            mi = num(j + 4, 2);
            j += 6;
            if self.at(j) == b':'
                && self.at(j + 1).is_ascii_digit()
                && self.at(j + 2).is_ascii_digit()
            {
                s = num(j + 1, 2);
                j += 3;
            }
        }
        if self.at(j) == b'Z' {
            j += 1;
        }
        let written = &self.src[k..j];
        let range = |field: &str| {
            Diag::new(
                Code::E003,
                Self::span(k, j),
                format!("{field} of {} is out of range", q(written)),
            )
            .help("write YYYY-MM-DD, YYYY-MM-DDTHH:MM or YYYY-MM-DDTHH:MM:SSZ")
        };
        if !(1..=12).contains(&mo) {
            return Err(range("month"));
        }
        if d < 1 || d > days_in_month(y, mo) {
            return Err(range("day"));
        }
        if h > 23 {
            return Err(range("hour"));
        }
        if mi > 59 {
            return Err(range("minute"));
        }
        if s > 59 {
            return Err(range("second"));
        }
        Ok((format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z"), j))
    }
}

fn control_diag(c: char, at: usize, len: usize) -> Diag {
    Diag::new(
        Code::E003,
        Span::new(at as u32, (at + len) as u32),
        format!(
            "control character U+{:04X} inside a string or name",
            c as u32
        ),
    )
    .help(format!("write \\u{{{:x}}}", c as u32))
}

/// Parses decimal digits (leading zeros allowed) into a `u64`, `None` on overflow or non-digits.
pub fn parse_decimal(digits: &str) -> Option<u64> {
    if digits.is_empty() {
        return None;
    }
    let mut n: u64 = 0;
    for b in digits.bytes() {
        if !b.is_ascii_digit() {
            return None;
        }
        n = n.checked_mul(10)?.checked_add(u64::from(b - b'0'))?;
    }
    Some(n)
}

/// Days of a month in the proleptic Gregorian calendar.
pub fn days_in_month(y: u32, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if (y.is_multiple_of(4) && !y.is_multiple_of(100)) || y.is_multiple_of(400) => 29,
        2 => 28,
        _ => 0,
    }
}

/// Days since 1970-01-01 of a proleptic Gregorian date (negative before it).
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let m = i64::from(m);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The normalised text `YYYY-MM-DDTHH:MM:SSZ` of a whole text in the ISO 8601 date or date-time form of
/// [LQ/lexical §7.5] (`YYYY-MM-DD`, `YYYY-MM-DDTHH:MM`, `YYYY-MM-DDTHH:MM:SS`, each with an optional `Z`), the form the
/// binder coerces to a timestamp ([LQ/canonical-ast §5.5]); `None` for any other text or an out-of-range field.
pub fn iso_datetime(text: &str) -> Option<String> {
    let lx = Lexer::new(text);
    if !lx.is_date_at(0) {
        return None;
    }
    let (norm, end) = lx.datetime(0).ok()?;
    (end == text.len()).then_some(norm)
}

/// Milliseconds since the Unix epoch of a normalised datetime `YYYY-MM-DDTHH:MM:SSZ` ([LQ/lexical §7.5]).
pub fn datetime_ms(text: &str) -> i64 {
    let n =
        |a: usize, b: usize| -> i64 { text.get(a..b).and_then(|s| s.parse().ok()).unwrap_or(0) };
    let days = days_from_civil(n(0, 4), n(5, 7) as u32, n(8, 10) as u32);
    ((days * 24 + n(11, 13)) * 60 + n(14, 16)) * 60_000 + n(17, 19) * 1_000
}

/// The millisecond value of a duration's text, `<digits><s|m|h|d|w>` ([LQ/lexical §5.6]); `None` when the text has
/// another form or the value is outside 0 … 2^63 − 1 ms ([LQ/lexical §8]). A duration token always has a value; a
/// text from elsewhere (a `k=v` parameter, a JSON IR `dur` node) may not.
pub fn duration_ms(text: &str) -> Option<i64> {
    let (&unit, digits) = text.as_bytes().split_last()?;
    let per: i64 = match unit {
        b's' => 1_000,
        b'm' => 60_000,
        b'h' => 3_600_000,
        b'd' => 86_400_000,
        b'w' => 604_800_000,
        _ => return None,
    };
    // `unit` is ASCII, so `digits` ends on a character boundary; `parse_decimal` refuses anything but ASCII digits.
    let n = i64::try_from(parse_decimal(std::str::from_utf8(digits).ok()?)?).ok()?;
    n.checked_mul(per)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(src: &str) -> Result<Vec<TokKind>, Diag> {
        let lx = Lexer::new(src);
        let mut pos = 0;
        let mut out = Vec::new();
        loop {
            let t = lx.token(pos)?;
            if t.kind == TokKind::Eof {
                return Ok(out);
            }
            pos = t.end;
            out.push(t.kind);
        }
    }

    fn err(src: &str) -> (Code, u32) {
        let e = toks(src).unwrap_err();
        (e.code, e.span.unwrap().start)
    }

    #[test]
    fn bom_is_removed_once_and_utf8_checked() {
        assert_eq!(decode(b"\xEF\xBB\xBFx").unwrap(), "x");
        assert_eq!(decode(b"\xEF\xBB\xBF\xEF\xBB\xBFx").unwrap(), "\u{feff}x");
        let e = decode(b"ab\xC0\xAF").unwrap_err();
        assert_eq!((e.code, e.span.unwrap().start), (Code::E003, 2));
        let e = decode(b"\xED\xA0\x80").unwrap_err();
        assert_eq!(e.code, Code::E003);
    }

    #[test]
    fn numbers_follow_the_order_of_section_5_6() {
        assert_eq!(
            toks("1..3").unwrap(),
            vec![
                TokKind::Int(1),
                TokKind::Punct(Punct::DotDot),
                TokKind::Int(3)
            ]
        );
        assert_eq!(
            toks("1.").unwrap(),
            vec![TokKind::Int(1), TokKind::Punct(Punct::Dot)]
        );
        assert_eq!(toks("010").unwrap(), vec![TokKind::Int(10)]);
        assert_eq!(
            toks("1.5e3 2E-2").unwrap(),
            vec![TokKind::Float, TokKind::Float]
        );
        assert_eq!(
            toks("15m 3d 2w").unwrap(),
            vec![
                TokKind::Dur(900_000),
                TokKind::Dur(259_200_000),
                TokKind::Dur(1_209_600_000)
            ]
        );
        assert_eq!(
            toks("-1").unwrap(),
            vec![TokKind::Punct(Punct::Minus), TokKind::Int(1)]
        );
        for bad in ["12abc", "3days", "15M", "1.5x", "0x10", "2e", "1e+"] {
            assert_eq!(err(bad), (Code::E003, 0), "{bad}");
        }
        assert_eq!(err("9223372036854775808").0, Code::E003);
        assert_eq!(
            toks("9223372036854775807").unwrap(),
            vec![TokKind::Int(i64::MAX)]
        );
        assert_eq!(err("1e999").0, Code::E003);
        assert_eq!(err("153722867280912931w").0, Code::E003);
    }

    /// The decoded values of the string and back-quoted-name tokens of `src`.
    fn decoded(src: &str) -> Vec<String> {
        let lx = Lexer::new(src);
        let mut pos = 0;
        let mut out = Vec::new();
        loop {
            let t = lx.token(pos).unwrap();
            match t.kind {
                TokKind::Eof => return out,
                TokKind::Str => out.push(lx.string_value(&t)),
                TokKind::QIdent => out.push(lx.qident_value(&t)),
                _ => {}
            }
            pos = t.end;
        }
    }

    #[test]
    fn strings_decode_escapes_and_refuse_bad_ones() {
        let src = r#"'a\'b' "c\"d" '\u{41}\n' 'é\u{1F600}'"#;
        assert_eq!(toks(src).unwrap(), vec![TokKind::Str; 4]);
        assert_eq!(decoded(src), ["a'b", "c\"d", "A\n", "é\u{1F600}"]);
        assert_eq!(err("'abc"), (Code::E002, 0));
        assert_eq!(err("x 'ab\ncd'"), (Code::E002, 2));
        assert_eq!(err(r"'\x41'"), (Code::E003, 1));
        assert_eq!(err(r"'\u{d800}'"), (Code::E003, 1));
        assert_eq!(err(r"'\u{110000}'"), (Code::E003, 1));
        assert_eq!(err("'a\u{1}'"), (Code::E003, 2));
        assert_eq!(decoded("'a\tb'"), ["a\tb"]);
        assert_eq!(Lexer::new("'x\\ny'").string(0).unwrap(), ("x\ny".into(), 6));
    }

    #[test]
    fn back_quoted_names() {
        let src = "`match` `a``b` ```x` `é`";
        assert_eq!(toks(src).unwrap(), vec![TokKind::QIdent; 4]);
        assert_eq!(decoded(src), ["match", "a`b", "`x", "é"]);
        assert_eq!(err("``"), (Code::E001, 0));
        assert_eq!(err("`abc"), (Code::E002, 0));
    }

    #[test]
    fn node_and_uid_literals() {
        assert_eq!(
            toks("#40 #007").unwrap(),
            vec![TokKind::Node(40), TokKind::Node(7)]
        );
        assert_eq!(
            toks("#133.body").unwrap(),
            vec![
                TokKind::Node(133),
                TokKind::Punct(Punct::Dot),
                TokKind::Word
            ]
        );
        assert_eq!(
            toks("#u:018f3c2e7a117b3c9d5e4c2f1a0b9e77").unwrap(),
            vec![TokKind::Uid]
        );
        for bad in [
            "#0",
            "#4294967296",
            "#x",
            "#u",
            "#u:12",
            "#40abc",
            "#u:018F3C2E7A117B3C9D5E4C2F1A0B9E77",
        ] {
            assert_eq!(err(bad), (Code::E003, 0), "{bad}");
        }
        assert_eq!(err("#u:018f3c2e7a117b3c9d5e4c2f1a0b9e777").0, Code::E003);
    }

    #[test]
    fn punctuation_by_longest_match() {
        use Punct::*;
        let p = |v: Vec<Punct>| v.into_iter().map(TokKind::Punct).collect::<Vec<_>>();
        assert_eq!(toks("a<-1").unwrap()[1..3], p(vec![Lt, Minus])[..]);
        assert_eq!(toks("<-[").unwrap(), p(vec![LArrow, LBracket]));
        assert_eq!(toks("-->").unwrap(), p(vec![Minus, Arrow]));
        assert_eq!(toks("<--").unwrap(), p(vec![LArrow, Minus]));
        // [LQ/lexical §5.9] says `<-->` is `<-` `-` `->`; by longest match its four bytes are `<-` `->`.
        assert_eq!(toks("<-->").unwrap(), p(vec![LArrow, Arrow]));
        assert_eq!(toks("--").unwrap(), p(vec![Minus, Minus]));
        assert_eq!(toks("...").unwrap(), p(vec![DotDot, Dot]));
        assert_eq!(
            toks("<> <= >= != =~ % ?").unwrap(),
            p(vec![Ne, Le, Ge, BangEq, EqTilde, Percent, Question])
        );
    }

    #[test]
    fn refused_characters() {
        assert_eq!(err("a ^ b"), (Code::E001, 2));
        assert_eq!(err("a ! b"), (Code::E001, 2));
        assert_eq!(err("é"), (Code::E001, 0));
        assert_eq!(err("\u{feff}x"), (Code::E001, 0));
        assert_eq!(err("$1"), (Code::E001, 0));
        assert_eq!(err("a /* b"), (Code::E002, 2));
        assert_eq!(toks("a // x\n b /* c */ d").unwrap().len(), 3);
    }

    fn rev(src: &str, arg: bool) -> Result<(RevRead, Vec<TokOut>), Diag> {
        let mut out = Vec::new();
        Lexer::new(src)
            .revision(0, arg, &mut out)
            .map(|(r, _)| (r, out))
    }

    fn rev_err(src: &str, arg: bool) -> (Code, u32) {
        let e = rev(src, arg).unwrap_err();
        (e.code, e.span.unwrap().start)
    }

    #[test]
    fn revisions_classify_and_take_suffixes() {
        let (_, out) = rev("main..lane/l5np", true).unwrap();
        let v: Vec<_> = out
            .iter()
            .map(|t| format!("{} {}", t.kind, t.value))
            .collect();
        assert_eq!(v, ["REF main", "RANGE ..", "REF lane/l5np"]);
        let (_, out) = rev("main@2026-09-25T10:00Z", false).unwrap();
        assert_eq!(out[1].value, "@2026-09-25T10:00:00Z");
        let (_, out) = rev("c9b2e6c1~ s4466^2 ", true).unwrap();
        assert_eq!(
            out.iter().map(|t| t.value.as_ref()).collect::<Vec<_>>(),
            ["9b2e6c1", "~1"]
        );
        let (_, out) = rev("main@{3}", false).unwrap();
        assert_eq!(out[1].value, "@3");
        let (_, out) = rev("tags/v1.2...HEAD", true).unwrap();
        assert_eq!(
            out.iter().map(|t| t.kind).collect::<Vec<_>>(),
            ["REF", "RANGE", "HEAD"]
        );
        let (_, out) = rev("HEAD...main", true).unwrap();
        assert_eq!(out[0].kind, "HEAD");
        let (_, out) = rev("[main, lane/x]", true).unwrap();
        assert_eq!(out.len(), 5);
        let (_, out) = rev("c123456", false).unwrap();
        assert_eq!(out[0].kind, "REF");
        assert!(matches!(rev("'main'", true).unwrap().0, RevRead::Quote));
    }

    #[test]
    fn revision_errors() {
        assert_eq!(rev_err("'main'", false), (Code::E001, 0));
        assert_eq!(rev_err("Main", false), (Code::E003, 0));
        assert_eq!(rev_err("main~5x", false), (Code::E003, 0));
        assert_eq!(rev_err("c9b2e6c1Z", false), (Code::E003, 0));
        assert_eq!(rev_err("main@", false), (Code::E001, 4));
        assert_eq!(rev_err("$r~1", false), (Code::E001, 2));
        assert_eq!(rev_err("a....b", true), (Code::E001, 1));
        assert_eq!(rev_err("[]", true), (Code::E001, 0));
        assert_eq!(rev_err("[a..b]", true).0, Code::E001);
        assert_eq!(rev_err("main@2026-02-30", false).0, Code::E003);
        assert_eq!(rev_err("main@2026-09-25T24:00", false).0, Code::E003);
        assert_eq!(rev_err("main~4294967296", false).0, Code::E003);
        assert_eq!(rev_err("s18446744073709551616", false).0, Code::E003);
        assert_eq!(rev_err("+", false).0, Code::E001);
    }

    /// [LQ/lexical §7.6], [LQ/errors §5.2]: a suffix after whitespace or a comment is E001 at the suffix.
    #[test]
    fn a_revision_ends_at_whitespace_or_a_comment() {
        for (src, at) in [
            ("main /* x */ ~2", 13),
            ("main ~2", 5),
            ("HEAD\n^", 5),
            ("c9b2e6c1 // x\n@1", 14),
            ("$r @2026-01-01", 3),
            ("main~1 ~1", 7),
        ] {
            let e = rev(src, false).unwrap_err();
            assert_eq!(
                (e.code, e.span.unwrap().start, e.message.as_str()),
                (Code::E001, at, "a revision ends at whitespace or a comment"),
                "{src}"
            );
        }
        assert_eq!(rev_err("[main , s1 ~1]", true), (Code::E001, 11));
        // Whitespace before anything else still ends the revision normally.
        assert!(rev("main MATCH", false).is_ok());
        assert!(rev("main .. s1", true).is_ok());
    }

    /// A duration's text outside the lexer ([LQ/lexical §5.6]): any text is refused, none panics.
    #[test]
    fn duration_values_of_any_text() {
        assert_eq!(duration_ms("3d"), Some(259_200_000));
        assert_eq!(duration_ms("0s"), Some(0));
        assert_eq!(duration_ms("2w"), Some(1_209_600_000));
        for bad in [
            "",
            "d",
            "5",
            "5é",
            "é",
            "5x",
            "-5s",
            " 5s",
            "5 s",
            "153722867280912931w",
        ] {
            assert_eq!(duration_ms(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn datetime_values() {
        assert_eq!(datetime_ms("1970-01-01T00:00:00Z"), 0);
        assert_eq!(datetime_ms("2026-09-25T10:00:00Z"), 1_790_330_400_000);
        assert_eq!(datetime_ms("1969-12-31T23:59:59Z"), -1_000);
    }
}
