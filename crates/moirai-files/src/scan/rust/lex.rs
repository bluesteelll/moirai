//! The Rust tokenizer of [F21 §3.1] rules 1–4, over bytes fed in chunks, and the canonical spelling of [F21 §3.2].
//!
//! Two layers:
//!
//! - the **decoder** turns bytes into [`Unit`]s: a byte at which no whitespace sequence of rule 1 starts, or one whole
//!   whitespace sequence (`09`–`0D`, `20`, and the byte sequences of U+0085, U+200E, U+200F, U+2028, U+2029). It
//!   holds at most two bytes of a sequence that might still be whitespace (`C2`, `E2`, `E2 80`), so above it a byte
//!   ≥ `80` is a word byte and every whitespace test is one flag;
//! - the **lexer** turns units into tokens with a resumable state per token kind, so a token, a comment or a literal
//!   may span any number of chunks. Its only look-ahead needs are one unit after `/` (a comment opener), the units of
//!   one character and one more after `'` (a character literal against a lifetime), and the `#` run after `r`, `br`
//!   or `cr`, which it counts; a unit it has read but cannot take is pushed back, two at most.
//!
//! Every byte is **taken** exactly once — into a token, or as whitespace or comment — and line numbers count the
//! `0A` bytes taken so far, so a token's lines are those of its first and last bytes ([F21 §1.2] `line(o)`). The
//! bytes of a token are kept only when the caller asks for them at the token's start.

/// The line feed.
const LF: u8 = 0x0A;

/// The longest word whose spelling decides anything: `macro_rules`.
const WORD_PRE: usize = 11;

/// Whether a byte that starts no whitespace sequence is a word byte ([F21 §3.1] rule 3).
#[inline]
const fn word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b >= 0x80
}

/// Whether a byte is a word byte that the fast path may take without the decoder: an ASCII word byte, or a byte ≥ `80`
/// other than `C2` and `E2`, the only lead bytes of multi-byte whitespace.
#[inline]
const fn fast_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || (b >= 0x80 && b != 0xC2 && b != 0xE2)
}

/// One byte that starts no whitespace sequence, or one whole whitespace sequence ([F21 §3.1] rules 1 and 3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Unit {
    /// The first byte.
    b: u8,
    /// The last byte of a two- or three-byte whitespace sequence.
    x: u8,
    /// The number of bytes, 1–3.
    len: u8,
    /// Whether the unit is a whitespace sequence.
    ws: bool,
    /// The offset of the first byte.
    off: u64,
}

impl Unit {
    const fn byte(b: u8, off: u64) -> Unit {
        Unit {
            b,
            x: 0,
            len: 1,
            ws: false,
            off,
        }
    }

    const fn space(b: u8, x: u8, len: u8, off: u64) -> Unit {
        Unit {
            b,
            x,
            len,
            ws: true,
            off,
        }
    }

    const fn is(self, c: u8) -> bool {
        !self.ws && self.b == c
    }

    const fn is_lf(self) -> bool {
        self.ws && self.b == LF
    }

    const fn is_word(self) -> bool {
        !self.ws && word_byte(self.b)
    }

    fn push_to(self, out: &mut Vec<u8>) {
        match self.len {
            1 => out.push(self.b),
            2 => out.extend_from_slice(&[self.b, self.x]),
            _ => out.extend_from_slice(&[self.b, 0x80, self.x]),
        }
    }
}

/// A prefix of a multi-byte whitespace sequence held by the decoder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pend {
    None,
    C2,
    E2,
    E280,
}

/// Bytes to units ([F21 §3.1] rule 1: each whitespace sequence "matched where it starts").
#[derive(Clone, Debug)]
struct Decoder {
    pend: Pend,
    /// The `80` of an `E2 80` that turned out not to be whitespace, still to be emitted as a byte.
    b80: bool,
    /// The offset of the first pending byte.
    poff: u64,
    /// The offset of the next byte read.
    off: u64,
}

impl Decoder {
    fn new(off: u64) -> Decoder {
        Decoder {
            pend: Pend::None,
            b80: false,
            poff: 0,
            off,
        }
    }

    /// The next unit of `src[*i..]`, or `None` when it is exhausted (at the end of the text, after the pending bytes
    /// are emitted as bytes).
    fn next(&mut self, src: &[u8], i: &mut usize, eof: bool) -> Option<Unit> {
        loop {
            if self.b80 {
                self.b80 = false;
                return Some(Unit::byte(0x80, self.poff + 1));
            }
            let Some(&b) = src.get(*i) else {
                if !eof {
                    return None;
                }
                let lead = match self.pend {
                    Pend::None => return None,
                    Pend::C2 => 0xC2,
                    Pend::E2 => 0xE2,
                    Pend::E280 => {
                        self.b80 = true;
                        0xE2
                    }
                };
                self.pend = Pend::None;
                return Some(Unit::byte(lead, self.poff));
            };
            let off = self.off;
            *i += 1;
            self.off += 1;
            match self.pend {
                Pend::None => match b {
                    0xC2 => {
                        self.pend = Pend::C2;
                        self.poff = off;
                    }
                    0xE2 => {
                        self.pend = Pend::E2;
                        self.poff = off;
                    }
                    0x09..=0x0D | 0x20 => return Some(Unit::space(b, 0, 1, off)),
                    _ => return Some(Unit::byte(b, off)),
                },
                Pend::C2 => {
                    self.pend = Pend::None;
                    if b == 0x85 {
                        return Some(Unit::space(0xC2, 0x85, 2, self.poff));
                    }
                    self.unread(i);
                    return Some(Unit::byte(0xC2, self.poff));
                }
                Pend::E2 => {
                    if b == 0x80 {
                        self.pend = Pend::E280;
                        continue;
                    }
                    self.pend = Pend::None;
                    self.unread(i);
                    return Some(Unit::byte(0xE2, self.poff));
                }
                Pend::E280 => {
                    self.pend = Pend::None;
                    if matches!(b, 0x8E | 0x8F | 0xA8 | 0xA9) {
                        return Some(Unit::space(0xE2, b, 3, self.poff));
                    }
                    self.b80 = true;
                    self.unread(i);
                    return Some(Unit::byte(0xE2, self.poff));
                }
            }
        }
    }

    /// Gives back the byte just read, which the next call reads again.
    fn unread(&mut self, i: &mut usize) {
        *i -= 1;
        self.off -= 1;
    }
}

/// A word that decides something in the scanner, or [`Kw::K`] for another word of the keyword set K, or
/// [`Kw::Other`] ([F21 §3.1] "Keywords"; §3.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kw {
    Other,
    K,
    Async,
    Const,
    Crate,
    Enum,
    Extern,
    Fn,
    For,
    Impl,
    Mod,
    Mut,
    Pub,
    Static,
    Struct,
    Trait,
    Unsafe,
    Where,
    MacroRules,
    Safe,
    Default,
}

impl Kw {
    /// Whether the word is in K, the 51 strict and reserved keywords ([F21 §3.1]). The weak keywords, `default`,
    /// `auto` and `gen` are not.
    pub(crate) const fn in_k(self) -> bool {
        !matches!(self, Kw::Other | Kw::MacroRules | Kw::Safe | Kw::Default)
    }
}

/// The [`Kw`] of a word's bytes.
fn kw_of(w: &[u8]) -> Kw {
    match w {
        b"async" => Kw::Async,
        b"const" => Kw::Const,
        b"crate" => Kw::Crate,
        b"enum" => Kw::Enum,
        b"extern" => Kw::Extern,
        b"fn" => Kw::Fn,
        b"for" => Kw::For,
        b"impl" => Kw::Impl,
        b"mod" => Kw::Mod,
        b"mut" => Kw::Mut,
        b"pub" => Kw::Pub,
        b"static" => Kw::Static,
        b"struct" => Kw::Struct,
        b"trait" => Kw::Trait,
        b"unsafe" => Kw::Unsafe,
        b"where" => Kw::Where,
        b"macro_rules" => Kw::MacroRules,
        b"safe" => Kw::Safe,
        b"default" => Kw::Default,
        b"as" | b"await" | b"break" | b"continue" | b"dyn" | b"else" | b"false" | b"if" | b"in"
        | b"let" | b"loop" | b"match" | b"move" | b"ref" | b"return" | b"self" | b"Self"
        | b"super" | b"true" | b"type" | b"use" | b"while" | b"abstract" | b"become" | b"box"
        | b"do" | b"final" | b"macro" | b"override" | b"priv" | b"try" | b"typeof" | b"unsized"
        | b"virtual" | b"yield" => Kw::K,
        _ => Kw::Other,
    }
}

/// A word token's properties.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Word {
    pub(crate) kw: Kw,
    /// A name ([F21 §3.1]): not in K and not starting with an ASCII digit.
    pub(crate) name: bool,
    /// Exactly `r`, `br` or `cr` (the canonical spelling's third spacing rule, [F21 §3.2]).
    pub(crate) rword: bool,
}

/// The kind of a literal token ([F21 §3.1] rule 4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LiteralKind {
    /// `"…"`.
    Str,
    /// `b"…"`.
    ByteStr,
    /// `c"…"`.
    CStr,
    /// `r"…"`, `r#"…"#`, ….
    RawStr,
    /// `br"…"`, ….
    RawByteStr,
    /// `cr"…"`, ….
    RawCStr,
    /// `'c'` or `'\…'`.
    Char,
    /// `b'…'`.
    ByteChar,
}

/// A token kind of the lexer; [`Kind::Newline`] only when the lexer reports line breaks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Word(Word),
    Lit(LiteralKind),
    Lifetime,
    Punct(u8),
    /// A `0A` outside every comment and literal.
    Newline,
}

/// A token: its kind, byte range and the lines of its first and last bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Tok {
    pub(crate) kind: Kind,
    pub(crate) off: u64,
    pub(crate) end: u64,
    pub(crate) line0: u64,
    pub(crate) line1: u64,
    /// Whether the token starts at the byte right after the previous token's last byte (no whitespace or comment
    /// between).
    pub(crate) adj: bool,
}

/// The token under construction.
#[derive(Clone, Copy, Debug)]
struct Build {
    off: u64,
    end: u64,
    line0: u64,
    line1: u64,
    adj: bool,
    lit: LiteralKind,
    /// The first bytes of a word.
    pre: [u8; WORD_PRE],
    /// The word's length so far.
    wlen: usize,
}

/// The lexer state: where in which token (or trivia) the next unit falls.
#[derive(Clone, Copy, Debug)]
enum St {
    /// Between tokens.
    Start,
    /// A `/` between tokens, not yet known to open a comment.
    Slash(Unit),
    LineComment,
    /// Inside `/*` … `*/` with its nesting depth and the previous byte if it was `/` or `*`.
    Block {
        depth: u64,
        prev: u8,
    },
    Word,
    /// After a word `r`, `br` or `cr`: n `#` bytes read, not taken, the first at `off`.
    RawHashes {
        n: u64,
        off: u64,
    },
    /// Inside a `"`-closed literal; `esc` when a `\` takes the next byte.
    Str {
        esc: bool,
    },
    /// Inside a `'`-closed literal (`b'…'`, `'\…'`).
    Char {
        esc: bool,
    },
    /// Inside a raw string with n `#`, after a `"` followed by `close` `#` bytes.
    Raw {
        n: u64,
        close: Option<u64>,
    },
    /// After a literal's closing quote: its suffix.
    Suffix,
    /// After a `'` that starts a token.
    Quote0,
    /// After `'` and the first n bytes of a multi-byte character of `need` bytes.
    QuoteLead {
        c: [Unit; 4],
        n: u8,
        need: u8,
    },
    /// After `'` and one whole character of n units.
    QuoteC {
        c: [Unit; 4],
        n: u8,
    },
    Lifetime,
}

/// The number of bytes of a well-formed UTF-8 sequence led by `b` (Unicode Table 3-7), or `None` when no
/// well-formed sequence of two or more bytes starts with it.
const fn utf8_need(b: u8) -> Option<u8> {
    match b {
        0xC2..=0xDF => Some(2),
        0xE0..=0xEF => Some(3),
        0xF0..=0xF4 => Some(4),
        _ => None,
    }
}

/// Whether `b` may stand at 0-based position `n` ≥ 1 of a sequence led by `lead` (Unicode Table 3-7).
const fn utf8_cont(lead: u8, n: u8, b: u8) -> bool {
    let (lo, hi) = match (lead, n) {
        (0xE0, 1) => (0xA0, 0xBF),
        (0xED, 1) => (0x80, 0x9F),
        (0xF0, 1) => (0x90, 0xBF),
        (0xF4, 1) => (0x80, 0x8F),
        _ => (0x80, 0xBF),
    };
    lo <= b && b <= hi
}

/// The streaming tokenizer ([F21 §3.1] rules 1–4; rule 5, the shebang, is the scanner's).
#[derive(Clone, Debug)]
pub(crate) struct Lexer {
    dec: Decoder,
    back: [Unit; 2],
    nback: usize,
    st: St,
    /// `#` tokens still to emit after a word `r`, `br` or `cr`: how many, and the offset of the next.
    hashes: Option<(u64, u64)>,
    /// The line of the next byte taken.
    line: u64,
    /// Whitespace or a comment was taken since the last token.
    gap: bool,
    b: Build,
    spell: Vec<u8>,
    capture: bool,
    /// The most bytes of a token kept in `spell`: once it holds this many, the rest of the token is not kept.
    keep: usize,
    newlines: bool,
}

impl Lexer {
    /// A lexer at offset `off` of a text, on line 1. With `newlines`, it reports each `0A` outside comments and
    /// literals as a [`Kind::Newline`] token.
    pub(crate) fn new(off: u64, newlines: bool) -> Lexer {
        let u = Unit::byte(0, 0);
        Lexer {
            dec: Decoder::new(off),
            back: [u; 2],
            nback: 0,
            st: St::Start,
            hashes: None,
            line: 1,
            gap: false,
            b: Build {
                off: 0,
                end: 0,
                line0: 1,
                line1: 1,
                adj: false,
                lit: LiteralKind::Str,
                pre: [0; WORD_PRE],
                wlen: 0,
            },
            spell: Vec::new(),
            capture: false,
            keep: usize::MAX,
            newlines,
        }
    }

    /// The same lexer keeping at most about `keep` bytes of a token ([`Lexer::spelled`]): a token longer than
    /// `keep` bytes keeps its first `keep` bytes or up to two more (a whitespace character inside a literal is kept
    /// whole). The scanner keeps `SCOPE_MAX_BYTES` + 1, enough to spell every name that can be recordable and to
    /// tell every longer one: a token's spelling ([F21 §3.2]) is never shorter than its bytes.
    pub(crate) fn capped(mut self, keep: usize) -> Lexer {
        self.keep = keep;
        self
    }

    /// The raw bytes of the token last returned, when `want` was set at its start (at most the first bytes a
    /// [`capped`](Lexer::capped) lexer keeps); a punctuation token's byte is its kind.
    pub(crate) fn spelled(&self) -> &[u8] {
        &self.spell
    }

    /// The offset of the first byte of a token in progress, if one has started and not ended.
    pub(crate) fn in_token(&self) -> Option<u64> {
        match self.st {
            St::Start | St::Slash(_) | St::LineComment | St::Block { .. } => None,
            _ => Some(self.b.off),
        }
    }

    /// The next token of `src[*i..]`. `None` asks for more input; with `eof`, the text ends there and `None` means
    /// the last token has been returned. `want` asks that the bytes of a token starting in this call be kept
    /// ([`Lexer::spelled`]).
    pub(crate) fn next(&mut self, src: &[u8], i: &mut usize, eof: bool, want: bool) -> Option<Tok> {
        loop {
            if let Some((left, off)) = self.hashes {
                self.hashes = if left > 1 {
                    Some((left - 1, off + 1))
                } else {
                    None
                };
                self.gap = false;
                return Some(Tok {
                    kind: Kind::Punct(b'#'),
                    off,
                    end: off + 1,
                    line0: self.line,
                    line1: self.line,
                    adj: true,
                });
            }
            let u = if self.nback > 0 {
                self.nback -= 1;
                Some(self.back[self.nback])
            } else {
                if self.dec.pend == Pend::None
                    && !self.dec.b80
                    && let Some(t) = self.fast(src, i, want)
                {
                    return Some(t);
                }
                self.dec.next(src, i, eof)
            };
            match u {
                Some(u) => {
                    if let Some(t) = self.step(u, want) {
                        return Some(t);
                    }
                }
                None if eof => {
                    if let Some(t) = self.at_end(want) {
                        return Some(t);
                    }
                    if self.hashes.is_none() && self.nback == 0 {
                        return None;
                    }
                }
                None => return None,
            }
        }
    }

    /// The per-unit path's result for a run of bytes at `src[*i..]`, taken in bulk where the state decides them
    /// without look-ahead: whitespace between tokens, the bytes of words, suffixes and lifetimes, comment bodies and
    /// literal bodies. A token whose last byte and the byte after it are in the chunk is returned when that byte is
    /// plain ASCII punctuation or whitespace (for a word, not `"`, `'` or `#`, which may start a literal or a raw
    /// identifier with it, [F21 §3.1] rule 4); a `"` literal goes on through its closing quote. The run stops before
    /// any byte the per-unit path must see: a backslash, a quote it cannot decide, `*`, `/`, `0A` inside a token,
    /// and `C2` or `E2`, which may start whitespace. It runs only when no unit is pushed back and the decoder holds
    /// nothing.
    fn fast(&mut self, src: &[u8], i: &mut usize, want: bool) -> Option<Tok> {
        loop {
            let rest = &src[*i..];
            match self.st {
                St::Start => {
                    let mut lf = 0;
                    let n = rest
                        .iter()
                        .position(|&b| match b {
                            LF if !self.newlines => {
                                lf += 1;
                                false
                            }
                            0x09 | 0x0B..=0x0D | 0x20 => false,
                            _ => true,
                        })
                        .unwrap_or(rest.len());
                    if n > 0 {
                        self.gap = true;
                        self.line += lf;
                        self.advance(i, n);
                    }
                    let &b = src.get(*i)?;
                    if fast_word_byte(b) {
                        self.begin_here(want);
                        self.st = St::Word;
                    } else if b == b'"' {
                        self.begin_here(want);
                        self.take_run(&src[*i..*i + 1]);
                        self.advance(i, 1);
                        self.b.lit = LiteralKind::Str;
                        self.st = St::Str { esc: false };
                    } else if b < 0x80 && !matches!(b, b'/' | b'\'' | 0x09..=0x0D | 0x20) {
                        self.begin_here(want);
                        self.take_run(&src[*i..*i + 1]);
                        self.advance(i, 1);
                        return Some(self.emit(Kind::Punct(b)));
                    } else {
                        return None;
                    }
                }
                St::Word | St::Suffix | St::Lifetime => {
                    let n = rest
                        .iter()
                        .position(|&b| !fast_word_byte(b))
                        .unwrap_or(rest.len());
                    if n > 0 && matches!(self.st, St::Word) {
                        let at = self.b.wlen.min(WORD_PRE);
                        let room = (WORD_PRE - at).min(n);
                        self.b.pre[at..at + room].copy_from_slice(&rest[..room]);
                        self.b.wlen += n;
                    }
                    self.take_run(&rest[..n]);
                    self.advance(i, n);
                    let &next = src.get(*i)?;
                    let word = matches!(self.st, St::Word);
                    if next >= 0x80
                        || next.is_ascii_alphanumeric()
                        || next == b'_'
                        || (word && matches!(next, b'"' | b'\'' | b'#'))
                    {
                        return None;
                    }
                    let kind = match std::mem::replace(&mut self.st, St::Start) {
                        St::Word => return Some(self.emit_word()),
                        St::Suffix => Kind::Lit(self.b.lit),
                        _ => Kind::Lifetime,
                    };
                    return Some(self.emit(kind));
                }
                St::Str { esc: false } | St::Char { esc: false } => {
                    let quote = if matches!(self.st, St::Str { .. }) {
                        b'"'
                    } else {
                        b'\''
                    };
                    let n = rest
                        .iter()
                        .position(|&b| b == quote || b == b'\\' || b == LF)
                        .unwrap_or(rest.len());
                    self.take_run(&rest[..n]);
                    self.advance(i, n);
                    if src.get(*i) != Some(&quote) {
                        return None;
                    }
                    self.take_run(&src[*i..*i + 1]);
                    self.advance(i, 1);
                    self.st = St::Suffix;
                }
                St::Raw {
                    n: hashes,
                    close: None,
                } => {
                    let n = rest
                        .iter()
                        .position(|&b| b == b'"' || b == LF)
                        .unwrap_or(rest.len());
                    self.take_run(&rest[..n]);
                    self.advance(i, n);
                    self.st = St::Raw {
                        n: hashes,
                        close: None,
                    };
                    return None;
                }
                St::LineComment => {
                    let n = rest.iter().position(|&b| b == LF).unwrap_or(rest.len());
                    self.gap |= n > 0;
                    self.advance(i, n);
                    return None;
                }
                St::Block { depth, .. } => {
                    let n = rest
                        .iter()
                        .position(|&b| matches!(b, b'*' | b'/' | LF))
                        .unwrap_or(rest.len());
                    if n > 0 {
                        self.gap = true;
                        self.st = St::Block { depth, prev: 0 };
                        self.advance(i, n);
                    }
                    return None;
                }
                _ => return None,
            }
        }
    }

    /// Moves past `n` bytes read in bulk.
    fn advance(&mut self, i: &mut usize, n: usize) {
        *i += n;
        self.dec.off += n as u64;
    }

    /// A token starts at the next byte read.
    fn begin_here(&mut self, want: bool) {
        self.b.off = self.dec.off;
        self.b.line0 = self.line;
        self.b.adj = !self.gap;
        self.b.wlen = 0;
        self.capture = want;
        self.spell.clear();
    }

    /// Takes a run of bytes without `0A` into the current token.
    fn take_run(&mut self, run: &[u8]) {
        if run.is_empty() {
            return;
        }
        self.b.line1 = self.line;
        self.b.end = self.dec.off + run.len() as u64;
        if self.capture {
            let room = self.keep.saturating_sub(self.spell.len());
            self.spell.extend_from_slice(&run[..run.len().min(room)]);
        }
    }

    fn push_back(&mut self, u: Unit) {
        debug_assert!(
            self.nback < self.back.len(),
            "at most two units are pushed back"
        );
        self.back[self.nback] = u;
        self.nback += 1;
    }

    /// Whitespace or a comment byte: taken, no token.
    fn trivia(&mut self, u: Unit) {
        self.gap = true;
        if u.is_lf() {
            self.line += 1;
        }
    }

    /// A token starts at `u`.
    fn begin(&mut self, u: Unit, want: bool) {
        self.b.off = u.off;
        self.b.line0 = self.line;
        self.b.adj = !self.gap;
        self.b.wlen = 0;
        self.capture = want;
        self.spell.clear();
    }

    /// `u` is the token's next unit.
    fn take(&mut self, u: Unit) {
        self.b.line1 = self.line;
        self.b.end = u.off + u64::from(u.len);
        if u.is_lf() {
            self.line += 1;
        }
        if self.capture && self.spell.len() < self.keep {
            u.push_to(&mut self.spell);
        }
    }

    fn word_push(&mut self, b: u8) {
        if self.b.wlen < WORD_PRE {
            self.b.pre[self.b.wlen] = b;
        }
        self.b.wlen += 1;
    }

    /// The word so far, when it is short enough to decide anything.
    fn word_bytes(&self) -> Option<([u8; WORD_PRE], usize)> {
        (self.b.wlen <= WORD_PRE).then_some((self.b.pre, self.b.wlen))
    }

    fn emit(&mut self, kind: Kind) -> Tok {
        self.gap = false;
        Tok {
            kind,
            off: self.b.off,
            end: self.b.end,
            line0: self.b.line0,
            line1: self.b.line1,
            adj: self.b.adj,
        }
    }

    fn emit_word(&mut self) -> Tok {
        let (kw, rword) = match self.word_bytes() {
            Some((pre, n)) => (kw_of(&pre[..n]), matches!(&pre[..n], b"r" | b"br" | b"cr")),
            None => (Kw::Other, false),
        };
        let name = !kw.in_k() && !self.b.pre[0].is_ascii_digit();
        self.emit(Kind::Word(Word { kw, name, rword }))
    }

    fn step(&mut self, u: Unit, want: bool) -> Option<Tok> {
        match self.st {
            St::Start => self.start(u, want),
            St::Slash(s) => {
                if u.is(b'/') || u.is(b'*') {
                    self.trivia(s);
                    self.trivia(u);
                    self.st = if u.b == b'/' {
                        St::LineComment
                    } else {
                        St::Block { depth: 1, prev: 0 }
                    };
                    return None;
                }
                self.push_back(u);
                self.begin(s, want);
                self.take(s);
                self.st = St::Start;
                Some(self.emit(Kind::Punct(b'/')))
            }
            St::LineComment => {
                if u.is_lf() {
                    self.st = St::Start;
                    return self.start(u, want);
                }
                self.trivia(u);
                None
            }
            St::Block { depth, prev } => {
                self.trivia(u);
                self.st = match (u.ws, prev, u.b) {
                    (false, b'/', b'*') => St::Block {
                        depth: depth + 1,
                        prev: 0,
                    },
                    (false, b'*', b'/') if depth == 1 => St::Start,
                    (false, b'*', b'/') => St::Block {
                        depth: depth - 1,
                        prev: 0,
                    },
                    (false, _, c @ (b'*' | b'/')) => St::Block { depth, prev: c },
                    _ => St::Block { depth, prev: 0 },
                };
                None
            }
            St::Word => self.word(u),
            St::RawHashes { n, off } => self.raw_hashes(u, n, off),
            St::Str { esc } => {
                self.take(u);
                self.st = if esc {
                    St::Str { esc: false }
                } else if u.is(b'"') {
                    St::Suffix
                } else {
                    St::Str { esc: u.is(b'\\') }
                };
                None
            }
            St::Char { esc } => {
                self.take(u);
                self.st = if esc {
                    St::Char { esc: false }
                } else if u.is(b'\'') {
                    St::Suffix
                } else {
                    St::Char { esc: u.is(b'\\') }
                };
                None
            }
            St::Raw { n, close } => {
                self.take(u);
                self.st = match close {
                    Some(k) if u.is(b'#') => {
                        if k + 1 == n {
                            St::Suffix
                        } else {
                            St::Raw {
                                n,
                                close: Some(k + 1),
                            }
                        }
                    }
                    _ if u.is(b'"') => {
                        if n == 0 {
                            St::Suffix
                        } else {
                            St::Raw { n, close: Some(0) }
                        }
                    }
                    _ => St::Raw { n, close: None },
                };
                None
            }
            St::Suffix => {
                if u.is_word() {
                    self.take(u);
                    return None;
                }
                self.push_back(u);
                self.st = St::Start;
                let lit = self.b.lit;
                Some(self.emit(Kind::Lit(lit)))
            }
            St::Quote0 => self.quote0(u),
            St::QuoteLead { c, n, need } => self.quote_lead(u, c, n, need),
            St::QuoteC { c, n } => self.quote_c(u, c, n),
            St::Lifetime => {
                if u.is_word() {
                    self.take(u);
                    return None;
                }
                self.push_back(u);
                self.st = St::Start;
                Some(self.emit(Kind::Lifetime))
            }
        }
    }

    fn start(&mut self, u: Unit, want: bool) -> Option<Tok> {
        if u.ws {
            let lf = u.is_lf();
            let line = self.line;
            self.trivia(u);
            if lf && self.newlines {
                return Some(Tok {
                    kind: Kind::Newline,
                    off: u.off,
                    end: u.off + 1,
                    line0: line,
                    line1: line,
                    adj: false,
                });
            }
            return None;
        }
        match u.b {
            b'/' => {
                self.st = St::Slash(u);
                None
            }
            b'"' => {
                self.begin(u, want);
                self.take(u);
                self.b.lit = LiteralKind::Str;
                self.st = St::Str { esc: false };
                None
            }
            b'\'' => {
                self.begin(u, want);
                self.take(u);
                self.st = St::Quote0;
                None
            }
            b if word_byte(b) => {
                self.begin(u, want);
                self.take(u);
                self.word_push(b);
                self.st = St::Word;
                None
            }
            b => {
                self.begin(u, want);
                self.take(u);
                Some(self.emit(Kind::Punct(b)))
            }
        }
    }

    /// In a word: a word byte extends it; `"`, `'` or `#` after `b`, `c`, `r`, `br` or `cr` starts a literal or a
    /// raw identifier (rule 4); anything else ends it.
    fn word(&mut self, u: Unit) -> Option<Tok> {
        if u.is_word() {
            self.take(u);
            self.word_push(u.b);
            return None;
        }
        if let (false, Some((pre, n))) = (u.ws, self.word_bytes()) {
            let w = &pre[..n];
            match (w, u.b) {
                (b"b" | b"c", b'"') => {
                    self.take(u);
                    self.b.lit = if w == b"b" {
                        LiteralKind::ByteStr
                    } else {
                        LiteralKind::CStr
                    };
                    self.st = St::Str { esc: false };
                    return None;
                }
                (b"b", b'\'') => {
                    self.take(u);
                    self.b.lit = LiteralKind::ByteChar;
                    self.st = St::Char { esc: false };
                    return None;
                }
                (b"r" | b"br" | b"cr", b'"') => {
                    self.take(u);
                    self.b.lit = raw_kind(w);
                    self.st = St::Raw { n: 0, close: None };
                    return None;
                }
                (b"r" | b"br" | b"cr", b'#') => {
                    self.st = St::RawHashes { n: 1, off: u.off };
                    return None;
                }
                _ => {}
            }
        }
        self.push_back(u);
        self.st = St::Start;
        Some(self.emit_word())
    }

    /// After `r`, `br` or `cr` and n `#`: a raw string, a raw identifier (`r#` and a word byte), or the word and n
    /// `#` punctuation tokens.
    fn raw_hashes(&mut self, u: Unit, n: u64, off: u64) -> Option<Tok> {
        if u.is(b'#') {
            self.st = St::RawHashes { n: n + 1, off };
            return None;
        }
        if u.is(b'"') {
            let lit = match self.word_bytes() {
                Some((pre, k)) => raw_kind(&pre[..k]),
                None => LiteralKind::RawStr,
            };
            self.take_hashes(n, off);
            self.take(u);
            self.b.lit = lit;
            self.st = St::Raw { n, close: None };
            return None;
        }
        if n == 1 && self.b.wlen == 1 && self.b.pre[0] == b'r' && u.is_word() {
            self.take_hashes(1, off);
            self.word_push(b'#');
            self.take(u);
            self.word_push(u.b);
            self.st = St::Word;
            return None;
        }
        self.push_back(u);
        self.hashes = Some((n, off));
        self.st = St::Start;
        Some(self.emit_word())
    }

    fn take_hashes(&mut self, n: u64, off: u64) {
        self.b.line1 = self.line;
        self.b.end = off + n;
        if self.capture {
            for _ in 0..n {
                if self.spell.len() >= self.keep {
                    break;
                }
                self.spell.push(b'#');
            }
        }
    }

    /// After a `'` that starts a token: `\` starts a character literal; a second `'` leaves a lone `'`; anything
    /// else is the first unit of a character c.
    fn quote0(&mut self, u: Unit) -> Option<Tok> {
        if u.is(b'\\') {
            self.take(u);
            self.b.lit = LiteralKind::Char;
            self.st = St::Char { esc: true };
            return None;
        }
        if u.is(b'\'') {
            self.push_back(u);
            self.st = St::Start;
            return Some(self.emit(Kind::Punct(b'\'')));
        }
        let c = [u; 4];
        self.st = match (u.ws, utf8_need(u.b)) {
            (false, Some(need)) => St::QuoteLead { c, n: 1, need },
            _ => St::QuoteC { c, n: 1 },
        };
        None
    }

    /// Inside a multi-byte character after `'`. A byte that breaks the sequence makes the character the lead byte
    /// alone ([F21 §3.1] rule 3, "the single byte at i when none does").
    fn quote_lead(&mut self, u: Unit, mut c: [Unit; 4], n: u8, need: u8) -> Option<Tok> {
        if !u.ws && utf8_cont(c[0].b, n, u.b) {
            c[usize::from(n)] = u;
            self.st = if n + 1 == need {
                St::QuoteC { c, n: need }
            } else {
                St::QuoteLead { c, n: n + 1, need }
            };
            return None;
        }
        if n == 1 && u.is(b'\'') {
            self.take(c[0]);
            self.take(u);
            self.b.lit = LiteralKind::Char;
            self.st = St::Suffix;
            return None;
        }
        // The lead byte, not followed by `'`, is a word byte: a lifetime over the bytes read, `u` still to come.
        for &x in &c[..usize::from(n)] {
            self.take(x);
        }
        self.push_back(u);
        self.st = St::Lifetime;
        None
    }

    /// After `'` and a whole character c: `'` closes the literal `'c'`; otherwise `'` starts a lifetime when c
    /// starts with a word byte, and is a lone `'` when it does not.
    fn quote_c(&mut self, u: Unit, c: [Unit; 4], n: u8) -> Option<Tok> {
        if u.is(b'\'') {
            for &x in &c[..usize::from(n)] {
                self.take(x);
            }
            self.take(u);
            self.b.lit = LiteralKind::Char;
            self.st = St::Suffix;
            return None;
        }
        if c[0].is_word() {
            for &x in &c[..usize::from(n)] {
                self.take(x);
            }
            self.push_back(u);
            self.st = St::Lifetime;
            return None;
        }
        // c is one unit that is no word byte: ASCII punctuation, a control byte or whitespace.
        self.push_back(u);
        self.push_back(c[0]);
        self.st = St::Start;
        Some(self.emit(Kind::Punct(b'\'')))
    }

    /// The end of the text in state `st`: an unterminated literal or comment runs to the end; a pending `/`, word,
    /// `'` or character decides as if no byte followed.
    fn at_end(&mut self, want: bool) -> Option<Tok> {
        match std::mem::replace(&mut self.st, St::Start) {
            St::Start | St::LineComment | St::Block { .. } => None,
            St::Slash(s) => {
                self.begin(s, want);
                self.take(s);
                Some(self.emit(Kind::Punct(b'/')))
            }
            St::Word => Some(self.emit_word()),
            St::RawHashes { n, off } => {
                self.hashes = Some((n, off));
                Some(self.emit_word())
            }
            St::Str { .. } | St::Char { .. } | St::Raw { .. } | St::Suffix => {
                let lit = self.b.lit;
                Some(self.emit(Kind::Lit(lit)))
            }
            St::Quote0 => Some(self.emit(Kind::Punct(b'\''))),
            St::QuoteLead { c, n, .. } => {
                for &x in &c[..usize::from(n)] {
                    self.take(x);
                }
                Some(self.emit(Kind::Lifetime))
            }
            St::QuoteC { c, n } => {
                if c[0].is_word() {
                    for &x in &c[..usize::from(n)] {
                        self.take(x);
                    }
                    Some(self.emit(Kind::Lifetime))
                } else {
                    self.push_back(c[0]);
                    Some(self.emit(Kind::Punct(b'\'')))
                }
            }
            St::Lifetime => Some(self.emit(Kind::Lifetime)),
        }
    }
}

fn raw_kind(w: &[u8]) -> LiteralKind {
    match w {
        b"br" => LiteralKind::RawByteStr,
        b"cr" => LiteralKind::RawCStr,
        _ => LiteralKind::RawStr,
    }
}

// --- §3.2: the canonical spelling -------------------------------------------------------------------------------

/// Writes one token's bytes as `canon` spells them ([F21 §3.2] steps 1 and 2): U+FFFD replacement of every maximal
/// ill-formed subpart, then CR LF and LF as `\n`, a lone CR as `\r` and NUL as `\0`.
pub(crate) fn spell(raw: &[u8], out: &mut String) {
    for chunk in raw.utf8_chunks() {
        let v = chunk.valid();
        if v.bytes().any(|b| matches!(b, 0x00 | 0x0A | 0x0D)) {
            let bytes = v.as_bytes();
            let (mut k, mut from) = (0, 0);
            while k < bytes.len() {
                let (esc, n) = match bytes[k] {
                    0x0A => ("\\n", 1),
                    0x0D if bytes.get(k + 1) == Some(&0x0A) => ("\\n", 2),
                    0x0D => ("\\r", 1),
                    0x00 => ("\\0", 1),
                    _ => {
                        k += 1;
                        continue;
                    }
                };
                // Every escaped byte is ASCII, so `from` and `k` are character boundaries.
                out.push_str(&v[from..k]);
                out.push_str(esc);
                k += n;
                from = k;
            }
            out.push_str(&v[from..]);
        } else {
            out.push_str(v);
        }
        if !chunk.invalid().is_empty() {
            out.push('\u{FFFD}');
        }
    }
}

/// How the previous token of a spelling ends, for the spacing rules of [F21 §3.2].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Sp {
    #[default]
    None,
    /// A word, literal or lifetime other than the words below.
    Wll,
    /// The word `r`, `br` or `cr`.
    RWord,
    /// The punctuation `/`.
    Slash,
    Other,
}

/// The canonical spelling of a token sequence, built token by token ([F21 §3.2]).
#[derive(Clone, Debug, Default)]
pub(crate) struct Canon {
    pub(crate) s: String,
    prev: Sp,
}

impl Canon {
    /// Appends a token of kind `kind` with raw bytes `raw` (ignored for punctuation, whose byte is its kind), with
    /// one SP before it where the spacing rules ask for one.
    pub(crate) fn push(&mut self, kind: Kind, raw: &[u8]) {
        if let Some(space) = self.space_before(kind) {
            write_token(space, kind, raw, &mut self.s);
        }
    }

    /// The spacing rules of [F21 §3.2] for the next token: whether one SP goes before it, and `None` for a line
    /// break, which is not written. The token becomes the previous one.
    pub(crate) fn space_before(&mut self, kind: Kind) -> Option<bool> {
        let (space, next) = match kind {
            Kind::Word(w) => (
                matches!(self.prev, Sp::Wll | Sp::RWord),
                if w.rword { Sp::RWord } else { Sp::Wll },
            ),
            Kind::Lit(_) | Kind::Lifetime => (matches!(self.prev, Sp::Wll | Sp::RWord), Sp::Wll),
            Kind::Punct(b) => {
                let space = match b {
                    b'/' | b'*' => self.prev == Sp::Slash,
                    b'#' => self.prev == Sp::RWord,
                    _ => false,
                };
                (space, if b == b'/' { Sp::Slash } else { Sp::Other })
            }
            Kind::Newline => return None,
        };
        self.prev = next;
        Some(space)
    }
}

/// Writes a token as [`canon`](super::canon) spells it, after one SP when `space` is set: a punctuation token by its
/// byte, any other by its raw bytes.
pub(crate) fn write_token(space: bool, kind: Kind, raw: &[u8], out: &mut String) {
    if space {
        out.push(' ');
    }
    match kind {
        Kind::Punct(b) => spell(&[b], out),
        _ => spell(raw, out),
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    /// Every token field and its kept bytes, with the text fed in chunks of `step` bytes (0: whole).
    fn lex_fed(x: &[u8], step: usize, newlines: bool) -> Vec<(Tok, Vec<u8>)> {
        let mut lx = Lexer::new(0, newlines);
        let mut out = Vec::new();
        let pieces: Vec<&[u8]> = if step == 0 {
            vec![x]
        } else {
            x.chunks(step).collect()
        };
        for p in pieces {
            let mut i = 0;
            while let Some(t) = lx.next(p, &mut i, false, true) {
                out.push((t, lx.spelled().to_vec()));
            }
        }
        while let Some(t) = lx.next(&[], &mut 0, true, true) {
            out.push((t, lx.spelled().to_vec()));
        }
        out
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 2048, failure_persistence: None, ..ProptestConfig::default() })]

        /// Fed one byte at a time, the lexer takes every byte on the per-unit path; whole, it takes the fast paths.
        /// Both give the same tokens, offsets, lines, adjacency and kept bytes.
        #[test]
        fn fast_paths_equal_the_per_unit_path(
            v in proptest::collection::vec(
                prop_oneof![
                    6 => proptest::sample::select(&[
                        "fn", "r", "b", "br", "c", "r#", "#", "\"", "'", "\\", "/", "*", "//", "/*", "*/", "\n", " ",
                        "\t", "a_1", "é", "\u{2028}", "\u{85}", "(", ")", "{", "}", "'a", "'b'", "x\"y\"", "1u8",
                    ][..]).prop_map(|s| s.as_bytes().to_vec()),
                    1 => proptest::collection::vec(any::<u8>(), 1..3),
                ],
                0..40,
            ),
            newlines in any::<bool>(),
        ) {
            let x = v.concat();
            let whole = lex_fed(&x, 0, newlines);
            prop_assert_eq!(&lex_fed(&x, 1, newlines), &whole);
            prop_assert_eq!(&lex_fed(&x, 3, newlines), &whole);
        }
    }

    fn lex_all(x: &[u8]) -> Vec<(Kind, u64, u64)> {
        let mut lx = Lexer::new(0, false);
        let mut i = 0;
        let mut out = Vec::new();
        while let Some(t) = lx.next(x, &mut i, true, false) {
            out.push((t.kind, t.off, t.end));
        }
        out
    }

    fn kinds(x: &[u8]) -> Vec<String> {
        lex_all(x)
            .into_iter()
            .map(|(k, a, b)| {
                let text = String::from_utf8_lossy(&x[a as usize..b as usize]).into_owned();
                match k {
                    Kind::Word(_) => format!("w:{text}"),
                    Kind::Lit(_) => format!("l:{text}"),
                    Kind::Lifetime => format!("t:{text}"),
                    Kind::Punct(_) => format!("p:{text}"),
                    Kind::Newline => "nl".to_owned(),
                }
            })
            .collect()
    }

    #[test]
    fn the_keyword_set_has_51_words() {
        let k = [
            "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum",
            "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod",
            "move", "mut", "pub", "ref", "return", "self", "Self", "static", "struct", "super",
            "trait", "true", "type", "unsafe", "use", "where", "while", "abstract", "become",
            "box", "do", "final", "macro", "override", "priv", "try", "typeof", "unsized",
            "virtual", "yield",
        ];
        assert_eq!(k.len(), 51);
        for w in k {
            assert!(kw_of(w.as_bytes()).in_k(), "{w}");
        }
        for w in [
            "macro_rules",
            "raw",
            "safe",
            "union",
            "default",
            "auto",
            "gen",
            "r#match",
            "_",
            "Größe",
        ] {
            assert!(!kw_of(w.as_bytes()).in_k(), "{w}");
        }
    }

    #[test]
    fn tokens_of_rule_4() {
        assert_eq!(kinds(b"a \"x\\\"y\" b"), ["w:a", "l:\"x\\\"y\"", "w:b"]);
        assert_eq!(
            kinds(b"b\"x\" c\"y\" b'z'"),
            ["l:b\"x\"", "l:c\"y\"", "l:b'z'"]
        );
        assert_eq!(
            kinds(b"r\"a\" r#\"a\"b\"# br##\"x\"#\"##"),
            ["l:r\"a\"", "l:r#\"a\"b\"#", "l:br##\"x\"#\"##"]
        );
        assert_eq!(
            kinds(b"'a' '\\n' '\\'' 'a 'static '_"),
            ["l:'a'", "l:'\\n'", "l:'\\''", "t:'a", "t:'static", "t:'_"]
        );
        assert_eq!(
            kinds(b"r#type r # type r##x"),
            [
                "w:r#type", "w:r", "p:#", "w:type", "w:r", "p:#", "p:#", "w:x"
            ]
        );
        assert_eq!(
            kinds(b"\"x\"suffix 1u8 0x1F"),
            ["l:\"x\"suffix", "w:1u8", "w:0x1F"]
        );
        assert_eq!(
            kinds(b"a/b // c\nd /* e /* f */ g */ h"),
            ["w:a", "p:/", "w:b", "w:d", "w:h"]
        );
        assert_eq!(kinds(b"'''"), ["p:'", "p:'", "p:'"]);
        assert_eq!(kinds(b"'('"), ["l:'('"]);
        assert_eq!(kinds(b"'(x"), ["p:'", "p:(", "w:x"]);
        assert_eq!(kinds(b"'/* c */x"), ["p:'", "w:x"]);
        assert_eq!(kinds("'é' 'é".as_bytes()), ["l:'é'", "t:'é"]);
        assert_eq!(kinds(b"rb\"x\""), ["w:rb", "l:\"x\""]);
        assert_eq!(kinds(b"extern\"C\""), ["w:extern", "l:\"C\""]);
    }

    #[test]
    fn unterminated_literals_and_comments_run_to_the_end() {
        assert_eq!(kinds(b"a \"b c"), ["w:a", "l:\"b c"]);
        assert_eq!(kinds(b"a /* b"), ["w:a"]);
        assert_eq!(kinds(b"a r#\"b\" c"), ["w:a", "l:r#\"b\" c"]);
        assert_eq!(kinds(b"a '"), ["w:a", "p:'"]);
        assert_eq!(kinds(b"a /"), ["w:a", "p:/"]);
        assert_eq!(kinds(b"r##"), ["w:r", "p:#", "p:#"]);
        assert_eq!(kinds(b"'("), ["p:'", "p:("]);
        assert_eq!(kinds(b"'a"), ["t:'a"]);
    }

    #[test]
    fn multi_byte_whitespace_and_invalid_bytes() {
        // U+2028 separates; `E2 80 FF` is one word of three bytes; U+0085 separates; a lone `C2` is a word byte.
        assert_eq!(lex_all(b"dyn\xE2\x80\xA8Trait").len(), 2);
        assert_eq!(lex_all(b"S<\xE2\x80\xFF>").len(), 4);
        assert_eq!(lex_all(b"a\xC2\x85b").len(), 2);
        assert_eq!(lex_all(b"a\xC2b").len(), 1);
        assert_eq!(lex_all(b"a\xE2").len(), 1);
        assert_eq!(lex_all(b"a\xE2\x80").len(), 1);
        // A character literal of one invalid byte, and a quote before a broken sequence.
        assert_eq!(kinds(b"'\xC3'"), ["l:'\u{FFFD}'"]);
        assert_eq!(lex_all(b"'\xE2\x82'").len(), 2);
        // A whitespace character inside quotes is a character literal.
        assert_eq!(lex_all("'\u{2028}'".as_bytes()).len(), 1);
        assert_eq!(lex_all(b"'\n'").len(), 1);
    }

    #[test]
    fn lines_and_adjacency() {
        let mut lx = Lexer::new(0, false);
        let x = b"a\n\"b\nc\" ->\n/* \n */ d";
        let mut i = 0;
        let mut v = Vec::new();
        while let Some(t) = lx.next(x, &mut i, true, false) {
            v.push((t.line0, t.line1, t.adj));
        }
        assert_eq!(
            v,
            [
                (1, 1, true),
                (2, 3, false),
                (3, 3, false),
                (3, 3, true),
                (5, 5, false)
            ]
        );
    }

    #[test]
    fn newline_tokens_skip_comments_and_literals() {
        let mut lx = Lexer::new(0, true);
        let x = b"a // c\n\"x\ny\" /*\n*/\nb";
        let mut i = 0;
        let mut offs = Vec::new();
        while let Some(t) = lx.next(x, &mut i, true, false) {
            if t.kind == Kind::Newline {
                offs.push(t.off);
            }
        }
        assert_eq!(offs, [6, 18]);
    }

    #[test]
    fn spelling_escapes() {
        let mut s = String::new();
        spell(b"a\r\nb\nc\rd\0e\xFFf\xE2\x80", &mut s);
        assert_eq!(s, "a\\nb\\nc\\rd\\0e\u{FFFD}f\u{FFFD}");
        let mut s = String::new();
        spell(b"\r\n\r\r\n", &mut s);
        assert_eq!(s, "\\n\\r\\n");
    }
}
