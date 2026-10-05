//! The Rust scanner ([F21 §3]): bracket groups and their classes (§3.3), item start positions (§3.4), the
//! recognition pattern (§3.5), extents (§3.6), `impl` names and qualifiers (§3.7), lines, parents and order (§3.8),
//! over the tokens of [`lex`] (§3.1, the shebang of rule 5 here), streaming.
//!
//! # How the reader works
//!
//! Tokens arrive one at a time. A stack of frames holds the root and every open group, each with its class
//! (`code`, `plain`, `tree`), its last three elements (which decide the class of a group opened after them), whether
//! the next element stands at an item start position, and — in a code sequence — the one item that may be open in it.
//! An item is a slot in the pre-order list from its first token on: while its pattern is still being matched, or an
//! `impl`'s name is still unknown, the slot is tentative, and items found inside the groups among its elements take it
//! as their parent; a pattern that fails, or an `impl` whose self type is empty, removes the slot and gives those items
//! the next enclosing item ([F21 §3.7] rule 4); the slot is killed in O(1) and the list compacted once at the end.
//! An `impl` header's tokens, at every depth, are spelled into two parts — before and after the `for` separator — as
//! they arrive, so the name and qualifier are ready when the header ends. The depth limit fails the scan at the
//! 1,025th open group ([F21 §2.7]).
//!
//! # The stream
//!
//! A token that an open header may take is written once, with the SP that [F21 §3.2] puts before it, to the end of
//! the items' text buffer ([`Items`] "The text buffer"). The spacing depends only on the token before, and a part
//! takes a run of consecutive tokens, so a part is a range of that stream: it starts at its first token and, while
//! it **grows**, ends where the last written token ends. A header nested in another's group is a range inside the
//! enclosing part's range, and an item found inside a header's group names its token's range, so nothing nested is
//! spelled twice and nothing is copied when a header ends.
//!
//! # Bounds
//!
//! A part keeps its range while it is at most `SCOPE_MAX_BYTES` bytes; past that it is long ([F21 §2.3]) and keeps
//! only the range of its first bytes and its bare name ([F21 §6.2]), which is decided at the first group inside the
//! header. A part that is long with its bare name decided is **saturated**: no later token changes it. The headers
//! whose current part is not saturated form a live list in nesting order. A growing part takes a token without being
//! touched — its end is the stream's — and the parts that grow end at the same place, so the outermost is the
//! longest: per token only the front of the list is checked against the cap, and each part goes long once. Once no
//! part under the cap grows, the stream bytes no item or stopped part holds are dropped. So each token costs O(1)
//! beyond its own bytes, memory is the tokens written (each once) plus one record per item, and both stay linear in
//! the text however deeply headers nest. The lexer keeps at most `SCOPE_MAX_BYTES` + 1 bytes of a token. The
//! shebang rule reads the text two ways until it is decided, holding no bytes.

mod lex;

use std::collections::VecDeque;

pub use lex::LiteralKind;
use lex::{Canon, Kind, Kw, Lexer, Tok, spell, write_token};

use super::form::Bare;
use super::items::{Items, LONG_PREFIX, NO_PARENT, Span, prefix_len};
use super::{Lang, RUST_MAX_DEPTH, SCOPE_MAX_BYTES, ScanFailed};

/// `skind` of a `mod` ([F08 §10.3.1]).
const SK_MOD: u8 = 1;
/// `skind` of an `impl`.
const SK_IMPL: u8 = 2;
/// `skind` of a `fn`.
const SK_FN: u8 = 3;
/// `skind` of a `struct`.
const SK_STRUCT: u8 = 4;
/// `skind` of an `enum`.
const SK_ENUM: u8 = 5;
/// `skind` of a `trait`.
const SK_TRAIT: u8 = 6;
/// `skind` of a `const`.
const SK_CONST: u8 = 7;
/// `skind` of a `static`.
const SK_STATIC: u8 = 8;
/// `skind` of a `macro_rules!` definition.
const SK_MACRO: u8 = 9;

/// The slot of an `impl` that is not reported ([F21 §3.7] rule 4).
const NO_SLOT: usize = usize::MAX;

// --- the public tokenizer -----------------------------------------------------------------------------------------

/// The kind of a [`Token`] ([F21 §3.1] rule 4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TokenKind {
    /// A word: an identifier, keyword, raw identifier (`r#type`) or number (`1u8`).
    Word,
    /// A literal with its suffix.
    Literal(LiteralKind),
    /// A lifetime (`'a`).
    Lifetime,
    /// One byte of punctuation, a lone `'` or a control byte included.
    Punct(u8),
    /// A `0A` outside every comment and literal: the line breaks that [F20 §2.8]'s header cut reads.
    Newline,
}

/// A token of [`tokens`]: its kind and byte range.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Token {
    /// The kind.
    pub kind: TokenKind,
    /// The offset of the first byte.
    pub start: usize,
    /// The offset after the last byte.
    pub end: usize,
}

/// The tokens of a byte string by [F21 §3.1] rules 1–4 (whitespace and comments skipped, line breaks outside them
/// reported), as [`canon`] and [F20 §2.8]'s header text read them. Rule 5, the shebang, is the scanner's only.
#[must_use]
pub fn tokens(x: &[u8]) -> Tokens<'_> {
    Tokens {
        lex: Lexer::new(0, true),
        x,
        i: 0,
    }
}

/// The iterator of [`tokens`].
#[derive(Clone, Debug)]
pub struct Tokens<'a> {
    lex: Lexer,
    x: &'a [u8],
    i: usize,
}

impl Iterator for Tokens<'_> {
    type Item = Token;

    fn next(&mut self) -> Option<Token> {
        let t = self.lex.next(self.x, &mut self.i, true, false)?;
        let kind = match t.kind {
            Kind::Word(_) => TokenKind::Word,
            Kind::Lit(l) => TokenKind::Literal(l),
            Kind::Lifetime => TokenKind::Lifetime,
            Kind::Punct(b) => TokenKind::Punct(b),
            Kind::Newline => TokenKind::Newline,
        };
        // Offsets of a slice fit `usize`.
        let at = |v: u64| usize::try_from(v).unwrap_or(usize::MAX);
        Some(Token {
            kind,
            start: at(t.off),
            end: at(t.end),
        })
    }
}

/// `canon(x)` ([F21 §3.2]): the tokens of x in order, each with U+FFFD replacement and the `\n`, `\r`, `\0` escapes,
/// one SP between two tokens exactly where the spacing rules ask for one. One line, no comment.
///
/// ```
/// use moirai_files::scan::canon;
///
/// assert_eq!(canon(b"Foo < T , U >"), "Foo<T,U>");
/// assert_eq!(canon(b"&'a   mut\tFoo"), "&'a mut Foo");
/// ```
#[must_use]
pub fn canon(x: &[u8]) -> String {
    let mut lex = Lexer::new(0, false);
    let mut out = Canon::default();
    let mut i = 0;
    while let Some(t) = lex.next(x, &mut i, true, true) {
        out.push(t.kind, lex.spelled());
    }
    out.s
}

// --- the scanner ----------------------------------------------------------------------------------------------------

/// The most bytes of a token the scanner's lexer keeps: a name that can be recordable has at most
/// [`SCOPE_MAX_BYTES`] bytes, and one byte more tells a longer one ([F21 §2.3]).
const KEEP: usize = SCOPE_MAX_BYTES + 1;

/// One reading of the text: a lexer and the group and item reader it feeds.
#[derive(Debug)]
struct Reading {
    lex: Lexer,
    p: Parser,
}

impl Reading {
    /// A reading whose first byte is at offset `off` of the text, counted as line 1.
    fn at(off: u64) -> Reading {
        Reading {
            lex: Lexer::new(off, false).capped(KEEP),
            p: Parser::new(),
        }
    }

    /// Reads `bytes[*i..]` to its end, or until the scan fails.
    fn feed_from(&mut self, bytes: &[u8], i: &mut usize) {
        while !self.p.failed {
            let want = self.p.want_spell();
            match self.lex.next(bytes, i, false, want) {
                Some(t) => self.p.token(&t, self.lex.spelled()),
                None => break,
            }
        }
    }

    fn feed(&mut self, bytes: &[u8]) {
        self.feed_from(bytes, &mut 0);
    }

    /// The end of the text: the last tokens, then every open group and item closes.
    fn finish(mut self) -> Result<Items, ScanFailed> {
        while !self.p.failed {
            let want = self.p.want_spell();
            match self.lex.next(&[], &mut 0, true, want) {
                Some(t) => self.p.token(&t, self.lex.spelled()),
                None => break,
            }
        }
        self.p.eof();
        if self.p.failed {
            return Err(ScanFailed);
        }
        let mut items = self.p.items;
        items.trim();
        items.shrink();
        Ok(items)
    }
}

/// Where the scanner stands on [F21 §3.1] rule 5, the shebang, at the start of the text.
#[derive(Debug)]
enum Shebang {
    /// Fewer than two bytes seen: they are held.
    Head { n: usize, b: [u8; 2] },
    /// The text starts with `#!` and rule 5 is undecided: the main reading, which reads the text as if it had no
    /// shebang, has returned `toks` tokens, and its third (the first after `#` and `!`, whitespace and comments
    /// skipped) has not started. `[` there makes `#!` an inner attribute; any other token, or none, a shebang.
    /// Meanwhile `alt` reads the text as if it had one: the bytes up to the first `0A` are a comment, so `alt`
    /// starts at that `0A` once it has come. `off` is the offset of the next byte.
    Probe {
        toks: u8,
        alt: Option<Box<Reading>>,
        off: u64,
    },
    /// A shebang line whose `0A` has not come; the offset of the next byte. The main reading is empty.
    Skip { off: u64 },
    /// Decided: every byte goes to the main reading.
    Done,
}

/// The Rust scanner over chunks of an anchor text.
#[derive(Debug)]
pub(crate) struct RustScanner {
    main: Reading,
    sb: Shebang,
}

impl RustScanner {
    pub(crate) fn new() -> RustScanner {
        RustScanner {
            main: Reading::at(0),
            sb: Shebang::Head { n: 0, b: [0; 2] },
        }
    }

    /// Whether the scan has failed for good. While rule 5 is undecided no reading's failure is final: the main
    /// reading has read three tokens at most, and the shebang reading may be the one dropped.
    pub(crate) fn has_failed(&self) -> bool {
        matches!(self.sb, Shebang::Done) && self.main.p.failed
    }

    pub(crate) fn feed(&mut self, mut chunk: &[u8]) {
        loop {
            match &mut self.sb {
                Shebang::Done => {
                    self.main.feed(chunk);
                    return;
                }
                Shebang::Head { n, b } => {
                    while *n < 2 && !chunk.is_empty() {
                        b[*n] = chunk[0];
                        *n += 1;
                        chunk = &chunk[1..];
                    }
                    if *n < 2 {
                        return;
                    }
                    let head = *b;
                    if head == *b"#!" {
                        self.sb = Shebang::Probe {
                            toks: 0,
                            alt: None,
                            off: 0,
                        };
                        self.probe(&head, false);
                    } else {
                        self.sb = Shebang::Done;
                        self.main.feed(&head);
                    }
                }
                Shebang::Probe { .. } => {
                    self.probe(chunk, false);
                    return;
                }
                Shebang::Skip { off } => {
                    match chunk.iter().position(|&c| c == b'\n') {
                        Some(k) => {
                            self.main = Reading::at(*off + k as u64);
                            self.sb = Shebang::Done;
                            self.main.feed(&chunk[k..]);
                        }
                        None => *off += chunk.len() as u64,
                    }
                    return;
                }
            }
        }
    }

    /// Reads `chunk` in both readings while rule 5 is undecided, and decides it once the main reading's third
    /// token has started — or at the end of the text (`eof`, `chunk` empty) when it has none. No byte is held: the
    /// shebang reading takes the whole chunk, the main reading takes it token by token.
    fn probe(&mut self, chunk: &[u8], eof: bool) {
        let RustScanner { main, sb } = self;
        let Shebang::Probe { toks, alt, off } = sb else {
            return;
        };
        match alt {
            Some(r) => r.feed(chunk),
            None => {
                if let Some(k) = chunk.iter().position(|&c| c == b'\n') {
                    let mut r = Reading::at(*off + k as u64);
                    r.feed(&chunk[k..]);
                    *alt = Some(Box::new(r));
                }
            }
        }
        *off += chunk.len() as u64;
        let mut i = 0;
        let shebang = loop {
            let want = main.p.want_spell();
            match main.lex.next(chunk, &mut i, eof, want) {
                Some(t) => {
                    main.p.token(&t, main.lex.spelled());
                    *toks += 1;
                    if *toks == 3 {
                        break t.kind != Kind::Punct(b'[');
                    }
                }
                // No third token in the whole text.
                None if eof => break true,
                // A third token has started, and a `[` token never waits for a later byte.
                None if *toks == 2 && main.lex.in_token().is_some() => break true,
                None => return,
            }
        };
        if !shebang {
            self.sb = Shebang::Done;
            self.main.feed_from(chunk, &mut i);
            return;
        }
        match alt.take() {
            Some(r) => {
                *main = *r;
                self.sb = Shebang::Done;
            }
            None => {
                let off = *off;
                *main = Reading::at(off);
                self.sb = Shebang::Skip { off };
            }
        }
    }

    pub(crate) fn finish(mut self) -> Result<Items, ScanFailed> {
        match std::mem::replace(&mut self.sb, Shebang::Done) {
            Shebang::Head { n, b } => self.main.feed(&b[..n]),
            probe @ Shebang::Probe { .. } => {
                self.sb = probe;
                self.probe(&[], true);
            }
            // A text that is one shebang line has no items; the main reading is empty.
            Shebang::Skip { .. } | Shebang::Done => {}
        }
        self.main.finish()
    }
}

// --- groups, sequences and items -------------------------------------------------------------------------------------

/// The class of a group ([F21 §3.3]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Class {
    Code,
    Plain,
    Tree,
}

/// An element of a sequence as the class and pattern rules see it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Elem {
    Absent,
    Punct(u8),
    /// A word: its keyword and whether it is a name.
    Word(Kw, bool),
    /// A literal or lifetime.
    Other,
    Group,
}

fn elem(k: Kind) -> Elem {
    match k {
        Kind::Word(w) => Elem::Word(w.kw, w.name),
        Kind::Punct(b) => Elem::Punct(b),
        Kind::Lit(_) | Kind::Lifetime | Kind::Newline => Elem::Other,
    }
}

/// Attributes before the next element ([F21 §3.4] rule 3): a `#`, a `#` `!`, or inside the attribute's `[` group,
/// each with the start-position flag from before the `#`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Attr {
    Idle,
    Hash(bool),
    HashBang(bool),
    In(bool),
}

/// The recognition pattern's state ([F21 §3.5]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum M {
    /// After `pub`.
    Vis,
    /// Inside `pub`'s `(` group.
    VisGroup,
    /// Before a qualifier or the keyword.
    Quals,
    /// After `extern`.
    Extern,
    /// After `const`, not yet a qualifier or the keyword.
    Const,
    /// After `fn`, `mod`, `struct`, `enum` or `trait`: the name comes.
    Kw(u8),
    Static,
    StaticMut,
    MacroRules,
    MacroBang,
    /// After `macro_rules ! name`: a group comes.
    MacroName,
}

/// One step of the pattern.
enum Step {
    Go(M),
    Item(u8),
    Impl,
    Fail,
}

/// The phase of the item open in a code sequence ([F21 §3.5, §3.6]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    /// The pattern is being matched; the slot is tentative.
    Match(M),
    /// The header scan of `mod`, `fn`, `struct`, `enum`, `trait` and `impl`: the angle count and the previous
    /// element.
    Header { a: u64, p: Elem },
    /// `const` and `static`: up to the first `;`.
    UntilSemi,
    /// `macro_rules!` whose `(` or `[` group is open.
    MacroWait,
    /// `macro_rules!` after that group, which ended on the line given: a `;` may follow.
    MacroAfter(u64),
    /// The final element is a group, which is open.
    Body,
}

#[derive(Clone, Copy, Debug)]
struct Open {
    /// The item's slot, or [`NO_SLOT`] for an `impl` that is not reported.
    slot: usize,
    skind: u8,
    phase: Phase,
}

/// The root or an open group.
#[derive(Clone, Debug)]
struct Frame {
    opener: u8,
    class: Class,
    /// The last three elements, the most recent last.
    hist: [Elem; 3],
    /// Whether the next element stands at an item start position as far as §3.4 rule 3 goes.
    start_ok: bool,
    attr: Attr,
    open: Option<Open>,
}

impl Frame {
    fn new(opener: u8, class: Class) -> Frame {
        Frame {
            opener,
            class,
            hist: [Elem::Absent; 3],
            start_ok: true,
            attr: Attr::Idle,
            open: None,
        }
    }

    fn push_hist(&mut self, e: Elem) {
        self.hist = [self.hist[1], self.hist[2], e];
    }

    /// §3.4 rule 3 after a token element: attributes are skipped; `;` and a stray `}` start items.
    fn after_token(&mut self, k: Kind) {
        match (k, self.attr) {
            (Kind::Punct(b'#'), _) => {
                self.attr = Attr::Hash(self.start_ok);
                self.start_ok = false;
            }
            (Kind::Punct(b'!'), Attr::Hash(s)) => {
                self.attr = Attr::HashBang(s);
                self.start_ok = false;
            }
            _ => {
                self.attr = Attr::Idle;
                self.start_ok = matches!(k, Kind::Punct(b';' | b'}'));
            }
        }
    }
}

/// Where an `impl` header's spelling stands ([F21 §3.7]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AccPhase {
    /// Right after `impl`.
    Init,
    /// In the generic parameters.
    Params,
    /// Reading the trait and self type.
    Parts,
    /// After `where`.
    Stop,
}

/// A `for` at angle count 0 not yet known to be the separator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pending {
    No,
    For,
    /// `for` then `<`: a lifetime or `>` next makes a higher-ranked binder.
    ForLt,
}

/// A token as written to the stream: the SP that [F21 §3.2] puts before it starts at `s` (`s` = `a` without one),
/// and its spelling spans `a..b`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Wrote {
    s: usize,
    a: usize,
    b: usize,
}

/// One part of an `impl` header — the trait before the separator, or the self type after it — as a range of the
/// stream, with the cap of [F21 §2.3]: the range of its whole canonical spelling while that is at most
/// [`SCOPE_MAX_BYTES`] bytes; then, long, the range of its first [`LONG_PREFIX`] bytes and its bare name
/// ([F21 §6.2]), read on from the whole spelling.
#[derive(Debug, Default)]
struct Part {
    /// Where its spelling starts in the stream, once it has taken a token.
    at: usize,
    /// Where it ends once it has stopped growing (the start of a token it did not take); `None` while it grows,
    /// having taken every token written since `at`.
    stop: Option<usize>,
    started: bool,
    long: bool,
    /// A long part's kept bytes: `[at, at + kept)`.
    kept: usize,
    bare: Bare,
}

impl Part {
    /// Whether no later token can change what the part keeps: it is long and its bare name is decided.
    fn saturated(&self) -> bool {
        self.long && self.bare.decided()
    }

    fn is_empty(&self) -> bool {
        !self.started
    }

    /// Takes the token written at `w`: a part under the cap grows by it, a long one reads it for its bare name.
    fn take(&mut self, w: Wrote, items: &mut Items) {
        if self.saturated() {
            return;
        }
        if self.long {
            self.bare.feed(&items.stream()[w.s..w.b]);
            return;
        }
        if !self.started {
            self.started = true;
            self.at = w.a;
        }
        debug_assert!(
            self.stop.is_none_or(|s| s == w.s),
            "a part grows by the next token"
        );
        self.stop = None;
        self.cap(w.b, items);
    }

    /// The part, growing, now ends at `end`: past the cap it goes long ([F21 §2.3]), keeping the range of its first
    /// bytes and reading its bare name from the whole spelling, which stops at the first byte that decides it.
    fn cap(&mut self, end: usize, items: &mut Items) {
        if self.long || !self.started || end - self.at <= SCOPE_MAX_BYTES {
            return;
        }
        let s = &items.stream()[self.at..end];
        self.long = true;
        self.bare.feed(s);
        self.kept = prefix_len(s, LONG_PREFIX);
        items.keep(self.at + self.kept);
    }

    /// The part stops growing at `end`, before a token it does not take; its bytes are kept.
    fn halt(&mut self, end: usize, items: &mut Items) {
        if self.started && !self.long {
            self.stop = Some(end);
            items.keep(end);
        }
    }

    /// The part as a name or qualifier; a part still growing ends at `now`.
    fn span(&self, now: usize) -> Span<'_> {
        match (self.started, self.long) {
            (_, true) => Span {
                at: self.at,
                len: self.kept,
                long: true,
                bare: self.bare.result(),
            },
            (true, false) => Span {
                at: self.at,
                len: self.stop.unwrap_or(now) - self.at,
                long: false,
                bare: None,
            },
            (false, false) => Span {
                at: 0,
                len: 0,
                long: false,
                bare: None,
            },
        }
    }
}

/// The spelling of one `impl` header: part 0 before the separator, part 1 after it.
#[derive(Debug)]
struct ImplAcc {
    frame: usize,
    phase: AccPhase,
    pending: Pending,
    sep: bool,
    parts: [Part; 2],
    /// The `for` of `pending`, as written (a part 0 under the cap takes it if a binder follows), and whether a SP
    /// separates it from the token before (a long part 0 reads it then).
    for_w: Option<Wrote>,
    for_sp: bool,
    /// The `<` after that `for`.
    lt_w: Option<Wrote>,
}

const LT: Kind = Kind::Punct(b'<');

impl ImplAcc {
    fn new(frame: usize) -> ImplAcc {
        ImplAcc {
            frame,
            phase: AccPhase::Init,
            pending: Pending::No,
            sep: false,
            parts: [Part::default(), Part::default()],
            for_w: None,
            for_sp: false,
            lt_w: None,
        }
    }

    fn cur(&self) -> &Part {
        &self.parts[usize::from(self.sep)]
    }

    fn cur_mut(&mut self) -> &mut Part {
        &mut self.parts[usize::from(self.sep)]
    }

    /// Whether the header reads the tokens of groups nested in it: it reads its parts and the current one is not
    /// saturated.
    fn is_live(&self) -> bool {
        self.phase == AccPhase::Parts && !self.cur().saturated()
    }

    /// Whether the next element of the header's own sequence needs its bytes: right after `impl`, or while the
    /// parts are read and the element may go to a part that is not saturated (after a `for`, to part 1).
    fn wants_raw(&self) -> bool {
        match self.phase {
            AccPhase::Init => true,
            AccPhase::Parts => self.pending != Pending::No || self.is_live(),
            AccPhase::Params | AccPhase::Stop => false,
        }
    }

    /// A token element of the header's own sequence, written at `w` when the header wanted it, with `sp` whether a
    /// SP precedes it, at angle count `a` before it ([F21 §3.7] rules 1–2; the angle count b of rule 2 equals the
    /// header scan's a from the header's start on). A token the header did not want goes to no part that reads.
    fn element(&mut self, kind: Kind, w: Option<Wrote>, sp: bool, a: u64, items: &mut Items) {
        match self.phase {
            AccPhase::Init if kind == LT => {
                self.phase = AccPhase::Params;
                return;
            }
            AccPhase::Init => self.phase = AccPhase::Parts,
            AccPhase::Params | AccPhase::Stop => return,
            AccPhase::Parts => {}
        }
        match self.pending {
            Pending::No => {}
            Pending::For if kind == LT => {
                self.pending = Pending::ForLt;
                self.lt_w = w;
                // Part 1 may start at this `<`, whatever part 0 is: its byte stays.
                if let Some(l) = w {
                    items.keep(l.b);
                }
                return;
            }
            Pending::For => {
                self.pending = Pending::No;
                self.sep = true;
            }
            Pending::ForLt => {
                self.pending = Pending::No;
                if matches!(kind, Kind::Lifetime | Kind::Punct(b'>')) {
                    self.binder(items);
                } else {
                    self.sep = true;
                    if let Some(l) = self.lt_w {
                        self.parts[1].take(l, items);
                    }
                }
            }
        }
        match kind {
            Kind::Word(wd) if a == 0 && wd.kw == Kw::Where => {
                if let Some(w) = w {
                    self.cur_mut().halt(w.s, items);
                }
                self.phase = AccPhase::Stop;
            }
            Kind::Word(wd) if a == 0 && wd.kw == Kw::For && !self.sep => {
                self.pending = Pending::For;
                self.for_w = w;
                self.for_sp = sp;
                if let Some(w) = w {
                    self.parts[0].halt(w.s, items);
                }
            }
            _ => match w {
                Some(w) => self.cur_mut().take(w, items),
                None => debug_assert!(self.cur().saturated(), "a part that reads was written"),
            },
        }
    }

    /// `for` `<` then a lifetime or `>`: a higher-ranked binder, whose `for` and `<` part 0 takes.
    fn binder(&mut self, items: &mut Items) {
        let p = &mut self.parts[0];
        if p.saturated() {
            return;
        }
        if p.long {
            p.bare.feed(if self.for_sp { " for" } else { "for" });
            p.bare.feed("<");
            return;
        }
        // Part 0 is under the cap, so the header was live and both tokens were written right after its last one.
        if let (Some(f), Some(l)) = (self.for_w, self.lt_w) {
            p.take(f, items);
            p.take(l, items);
        }
    }

    /// The angle count after an element: the parameters end where it returns to 0.
    fn angle(&mut self, a: u64) {
        if self.phase == AccPhase::Params && a == 0 {
            self.phase = AccPhase::Parts;
        }
    }

    /// The header ended: the parts that hold `name` and `qual` of [F21 §3.7] rule 3 (an empty part for no trait),
    /// or `None` when the header was empty.
    fn finish(mut self, items: &mut Items) -> Option<(Part, Part)> {
        match self.phase {
            AccPhase::Init | AccPhase::Params => return None,
            AccPhase::Parts | AccPhase::Stop => {}
        }
        match self.pending {
            Pending::No => {}
            Pending::For => self.sep = true,
            Pending::ForLt => {
                self.sep = true;
                if let Some(l) = self.lt_w {
                    self.parts[1].take(l, items);
                    self.parts[1].halt(l.b, items);
                }
            }
        }
        let [p0, p1] = self.parts;
        Some(if self.sep {
            (p1, p0)
        } else {
            (p0, Part::default())
        })
    }
}

fn kind_ix(c: u8) -> usize {
    match c {
        b'(' | b')' => 0,
        b'[' | b']' => 1,
        _ => 2,
    }
}

fn opener_of(c: u8) -> u8 {
    match c {
        b')' => b'(',
        b']' => b'[',
        _ => b'{',
    }
}

fn word_kw(k: Kind) -> Option<Kw> {
    match k {
        Kind::Word(w) => Some(w.kw),
        _ => None,
    }
}

fn is_name(k: Kind) -> bool {
    matches!(k, Kind::Word(w) if w.name)
}

/// Whether a token can be the first token u0 of an item: `pub`, a qualifier or a keyword of [F21 §3.5].
fn can_start(k: Kind) -> bool {
    matches!(
        word_kw(k),
        Some(
            Kw::Pub
                | Kw::Async
                | Kw::Unsafe
                | Kw::Safe
                | Kw::Default
                | Kw::Extern
                | Kw::Const
                | Kw::Fn
                | Kw::Mod
                | Kw::Struct
                | Kw::Enum
                | Kw::Trait
                | Kw::Static
                | Kw::Impl
                | Kw::MacroRules
        )
    )
}

/// The pattern of [F21 §3.5] over one token element.
fn step_match(mut m: M, k: Kind) -> Step {
    loop {
        match m {
            M::Vis => m = M::Quals,
            M::Quals => {
                return match word_kw(k) {
                    Some(Kw::Async | Kw::Unsafe | Kw::Safe | Kw::Default) => Step::Go(M::Quals),
                    Some(Kw::Extern) => Step::Go(M::Extern),
                    Some(Kw::Const) => Step::Go(M::Const),
                    Some(Kw::Fn) => Step::Go(M::Kw(SK_FN)),
                    Some(Kw::Mod) => Step::Go(M::Kw(SK_MOD)),
                    Some(Kw::Struct) => Step::Go(M::Kw(SK_STRUCT)),
                    Some(Kw::Enum) => Step::Go(M::Kw(SK_ENUM)),
                    Some(Kw::Trait) => Step::Go(M::Kw(SK_TRAIT)),
                    Some(Kw::Static) => Step::Go(M::Static),
                    Some(Kw::Impl) => Step::Impl,
                    Some(Kw::MacroRules) => Step::Go(M::MacroRules),
                    _ => Step::Fail,
                };
            }
            // The ABI is "a string or raw string literal": [F21 §3.1] rule 4's string (`"…"`) and its raw strings
            // (`r`, `br` and `cr`); a byte or C string (`b"…"`, `c"…"`) is neither (spec finding of WP-63's review
            // round 2, on [F21 §3.5]).
            M::Extern => match k {
                Kind::Word(w) if w.kw == Kw::Crate => return Step::Fail,
                Kind::Lit(
                    LiteralKind::Str
                    | LiteralKind::RawStr
                    | LiteralKind::RawByteStr
                    | LiteralKind::RawCStr,
                ) => return Step::Go(M::Quals),
                _ => m = M::Quals,
            },
            M::Const => match word_kw(k) {
                Some(Kw::Fn | Kw::Async | Kw::Unsafe | Kw::Extern) => m = M::Quals,
                _ => {
                    return if is_name(k) {
                        Step::Item(SK_CONST)
                    } else {
                        Step::Fail
                    };
                }
            },
            M::Kw(sk) => {
                return if is_name(k) {
                    Step::Item(sk)
                } else {
                    Step::Fail
                };
            }
            M::Static if word_kw(k) == Some(Kw::Mut) => return Step::Go(M::StaticMut),
            M::Static | M::StaticMut => {
                return if is_name(k) {
                    Step::Item(SK_STATIC)
                } else {
                    Step::Fail
                };
            }
            M::MacroRules => {
                return if k == Kind::Punct(b'!') {
                    Step::Go(M::MacroBang)
                } else {
                    Step::Fail
                };
            }
            M::MacroBang => {
                return if is_name(k) {
                    Step::Go(M::MacroName)
                } else {
                    Step::Fail
                };
            }
            // A token where a group must come.
            M::MacroName | M::VisGroup => return Step::Fail,
        }
    }
}

/// The group and item reader over tokens.
#[derive(Debug)]
struct Parser {
    frames: Vec<Frame>,
    /// Open groups per opener kind, `(`, `[`, `{`.
    open_kind: [usize; 3],
    items: Items,
    /// The slots of the open items, innermost last.
    slots: Vec<usize>,
    /// The `impl` headers being spelled, innermost last; their frames increase.
    accs: Vec<ImplAcc>,
    /// The indices in `accs` of the live headers ([`ImplAcc::is_live`]), increasing. Every one but the innermost
    /// encloses an open group of its header, whose opener decided a long part's bare name, so it is under the cap
    /// and grows with every token; the outermost is the longest.
    live: VecDeque<usize>,
    /// The line of the last byte of the last token.
    last_line: u64,
    /// The spacing of [F21 §3.2] after the last token, which the stream writes by.
    sp: Canon,
    /// The current token as written to the stream, when an open header wanted it.
    cur: Option<Wrote>,
    /// Whether a SP precedes the current token.
    cur_sp: bool,
    /// The name of a `macro_rules!` whose group has not come yet: its stream range when an open header wanted the
    /// token, else a copy.
    macro_at: Option<(usize, usize)>,
    macro_name: String,
    failed: bool,
}

impl Parser {
    fn new() -> Parser {
        Parser {
            frames: vec![Frame::new(0, Class::Code)],
            open_kind: [0; 3],
            items: Items::new(Lang::Rust),
            slots: Vec::new(),
            accs: Vec::new(),
            live: VecDeque::new(),
            last_line: 1,
            sp: Canon::default(),
            cur: None,
            cur_sp: false,
            macro_at: None,
            macro_name: String::new(),
            failed: false,
        }
    }

    /// Whether the next token's bytes are needed: it may be an item's name, or it belongs to an `impl` header that
    /// still reads it.
    fn want_spell(&self) -> bool {
        if self.wants_stream() {
            return true;
        }
        let top = &self.frames[self.frames.len() - 1];
        matches!(
            top.open,
            Some(Open {
                phase: Phase::Match(M::Kw(_) | M::Const | M::Static | M::StaticMut | M::MacroBang),
                ..
            })
        )
    }

    /// Whether an open `impl` header may take the next token, so it is written to the stream: a live header, or the
    /// innermost at its start or after a `for`.
    fn wants_stream(&self) -> bool {
        !self.live.is_empty() || self.accs.last().is_some_and(ImplAcc::wants_raw)
    }

    /// Whether a part under the cap still grows, so the stream past the kept bytes is still needed: every live
    /// header but the innermost is one, and the innermost when its current part is under the cap (a part that
    /// waits on a `for` included).
    fn needs_tail(&self) -> bool {
        match self.live.len() {
            0 => false,
            1 => !self.accs[self.live[0]].cur().long,
            _ => true,
        }
    }

    /// Where a part that still grows ends before the current token: the current token's start, or the stream's end
    /// after the last token.
    fn now(&self) -> usize {
        self.cur.map_or(self.items.stream().len(), |w| w.s)
    }

    fn token(&mut self, tok: &Tok, raw: &[u8]) {
        if self.failed {
            return;
        }
        // The scanner's lexer reports no line breaks, so every token has a spacing.
        let sp = self.sp.space_before(tok.kind).unwrap_or(false);
        self.cur_sp = sp;
        self.cur = self.wants_stream().then(|| {
            let s = self.items.stream().len();
            write_token(sp, tok.kind, raw, self.items.stream_mut());
            Wrote {
                s,
                a: s + usize::from(sp),
                b: self.items.stream().len(),
            }
        });
        match tok.kind {
            Kind::Punct(c @ (b'(' | b'[' | b'{')) => self.open_group(tok, c),
            Kind::Punct(c @ (b')' | b']' | b'}')) if self.open_kind[kind_ix(c)] > 0 => {
                self.close_group(tok, c)
            }
            _ => self.plain_token(tok, raw),
        }
        self.last_line = tok.line1;
        if !self.needs_tail() {
            self.items.trim();
        }
        self.cur = None;
    }

    fn top(&self) -> usize {
        self.frames.len() - 1
    }

    /// The current token, in a group nested in the headers of the sequences enclosing frame `t`'s, goes to their
    /// live parts. A part under the cap grows by it untouched — its end is the stream's — so only the front of the
    /// live list, the longest part, is checked against the cap: one that goes long there saturates and leaves the
    /// list, and the check moves on to the next. A long part whose bare name is undecided reads the token (only the
    /// innermost header's part can be one, and never with a group open inside it).
    fn feed_deeper(&mut self, t: usize) {
        let Some(w) = self.cur else {
            // No header wanted the token, so none is live.
            return;
        };
        let mut k = 0;
        while let Some(&ix) = self.live.get(k) {
            let acc = &mut self.accs[ix];
            if acc.frame >= t {
                break;
            }
            let part = acc.cur_mut();
            if part.long {
                part.take(w, &mut self.items);
            } else if part.started && w.b - part.at > SCOPE_MAX_BYTES {
                part.cap(w.b, &mut self.items);
            } else {
                break;
            }
            if acc.is_live() {
                k += 1;
            } else {
                self.live.remove(k);
            }
        }
    }

    /// Lists the innermost `impl` header as live, or unlists it, after an element of its own sequence.
    fn sync_live(&mut self) {
        let Some(ix) = self.accs.len().checked_sub(1) else {
            return;
        };
        let listed = self.live.back() == Some(&ix);
        match (self.accs[ix].is_live(), listed) {
            (true, false) => self.live.push_back(ix),
            (false, true) => {
                self.live.pop_back();
            }
            _ => {}
        }
    }

    fn set_phase(&mut self, t: usize, phase: Phase) {
        if let Some(o) = self.frames[t].open.as_mut() {
            o.phase = phase;
        }
    }

    /// A token that is neither an opener nor a closer of an open group: an element of the top sequence.
    fn plain_token(&mut self, tok: &Tok, raw: &[u8]) {
        let t = self.top();
        self.feed_deeper(t);
        match self.frames[t].class {
            Class::Code => {
                self.code_token(t, tok, raw);
                let f = &mut self.frames[t];
                f.after_token(tok.kind);
                f.push_hist(elem(tok.kind));
            }
            Class::Plain => self.frames[t].push_hist(elem(tok.kind)),
            Class::Tree => {}
        }
    }

    /// A token element of code sequence `t`: it starts, continues or ends the item open there.
    fn code_token(&mut self, t: usize, tok: &Tok, raw: &[u8]) {
        let Some(o) = self.frames[t].open else {
            if self.frames[t].start_ok && can_start(tok.kind) {
                self.begin_item(t, tok, raw);
            }
            return;
        };
        match o.phase {
            Phase::Match(m) => self.advance(t, o, m, tok, raw),
            Phase::Header { a, p } => self.header_token(t, o, a, p, tok),
            Phase::UntilSemi => {
                if tok.kind == Kind::Punct(b';') {
                    self.end_item(t, tok.line1);
                }
            }
            Phase::MacroAfter(end) => {
                let line = if tok.kind == Kind::Punct(b';') {
                    tok.line1
                } else {
                    end
                };
                self.end_item(t, line);
            }
            // A group is open above this frame, so no token reaches it in these phases.
            Phase::MacroWait | Phase::Body => {}
        }
    }

    /// u0 of an item at an item start position: a tentative slot, parented to the innermost open item.
    fn begin_item(&mut self, t: usize, tok: &Tok, raw: &[u8]) {
        let parent = self.slots.last().copied().unwrap_or(NO_PARENT);
        let slot = self.items.push(0, tok.line0, parent);
        self.slots.push(slot);
        let o = Open {
            slot,
            skind: 0,
            phase: Phase::Match(M::Vis),
        };
        self.frames[t].open = Some(o);
        if word_kw(tok.kind) != Some(Kw::Pub) {
            self.advance(t, o, M::Quals, tok, raw);
        }
    }

    fn advance(&mut self, t: usize, o: Open, m: M, tok: &Tok, raw: &[u8]) {
        match step_match(m, tok.kind) {
            Step::Go(m2) => {
                if m2 == M::MacroName {
                    // Written to the stream, the name is its token's range; else a copy waits for the group.
                    self.macro_at = self.cur.map(|w| {
                        self.items.keep(w.b);
                        (w.a, w.b)
                    });
                    if self.macro_at.is_none() {
                        self.macro_name.clear();
                        spell(raw, &mut self.macro_name);
                    }
                }
                self.set_phase(t, Phase::Match(m2));
            }
            Step::Item(sk) => {
                self.items.set_skind(o.slot, sk);
                // Inside a header's group the name is its token's range of the stream; elsewhere no header is
                // open under the cap and the name is copied.
                match self.cur {
                    Some(w) => self.items.set_name_ref(o.slot, w.a, w.b),
                    None => self.items.set_name_with(o.slot, |s| spell(raw, s)),
                }
                let phase = if matches!(sk, SK_CONST | SK_STATIC) {
                    Phase::UntilSemi
                } else {
                    Phase::Header {
                        a: 0,
                        p: Elem::Word(Kw::Other, true),
                    }
                };
                self.frames[t].open = Some(Open {
                    skind: sk,
                    phase,
                    ..o
                });
            }
            Step::Impl => {
                self.items.set_skind(o.slot, SK_IMPL);
                self.frames[t].open = Some(Open {
                    skind: SK_IMPL,
                    phase: Phase::Header {
                        a: 0,
                        p: Elem::Word(Kw::Impl, false),
                    },
                    ..o
                });
                self.accs.push(ImplAcc::new(t));
            }
            Step::Fail => self.fail(t, o),
        }
    }

    /// The pattern did not match: the tentative slot goes.
    fn fail(&mut self, t: usize, o: Open) {
        self.items.kill(o.slot);
        let top = self.slots.pop();
        debug_assert_eq!(
            top,
            Some(o.slot),
            "a failing item's slot is the innermost open"
        );
        self.frames[t].open = None;
    }

    /// A token element of a header ([F21 §3.6] steps 1, 3, 4 and 5).
    fn header_token(&mut self, t: usize, o: Open, a: u64, p: Elem, tok: &Tok) {
        let is_impl = o.skind == SK_IMPL;
        if tok.kind == Kind::Punct(b';') {
            if is_impl {
                self.finish_impl(t);
            }
            self.end_item(t, tok.line1);
            return;
        }
        if is_impl && let Some(acc) = self.accs.last_mut() {
            acc.element(tok.kind, self.cur, self.cur_sp, a, &mut self.items);
        }
        let arrow = matches!(p, Elem::Punct(b'-' | b'=')) && tok.adj;
        let a2 = match tok.kind {
            Kind::Punct(b'<') => a + 1,
            Kind::Punct(b'>') if a > 0 && !arrow => a - 1,
            _ => a,
        };
        if is_impl && let Some(acc) = self.accs.last_mut() {
            acc.angle(a2);
            self.sync_live();
        }
        self.set_phase(
            t,
            Phase::Header {
                a: a2,
                p: elem(tok.kind),
            },
        );
    }

    /// The `impl` of frame `t` ends its header, before the current token: its name and qualifier are the ranges of
    /// its parts, or no item when the self type is empty.
    fn finish_impl(&mut self, t: usize) {
        let Some(acc) = self.accs.pop() else { return };
        if self.live.back() == Some(&self.accs.len()) {
            self.live.pop_back();
        }
        debug_assert_eq!(
            acc.frame, t,
            "the innermost impl header is the top sequence's"
        );
        let now = self.now();
        let parts = acc.finish(&mut self.items);
        let slot = match self.frames[t].open {
            Some(o) if o.slot != NO_SLOT => o.slot,
            _ => return,
        };
        match parts {
            Some((name, qual)) if !name.is_empty() => {
                // A long part lies inside every enclosing header's growing part, which is then long too: nothing
                // under the cap grows, so the copies `set_impl` may append break no range.
                debug_assert!(!(name.long || qual.long) || !self.needs_tail());
                self.items.set_impl(slot, &name.span(now), &qual.span(now));
            }
            _ => {
                self.items.kill(slot);
                let top = self.slots.pop();
                debug_assert_eq!(top, Some(slot), "the impl's slot is the innermost open");
                if let Some(o) = self.frames[t].open.as_mut() {
                    o.slot = NO_SLOT;
                }
            }
        }
    }

    /// The item open in frame `t` ends on line `line`.
    fn end_item(&mut self, t: usize, line: u64) {
        if let Some(o) = self.frames[t].open.take()
            && o.slot != NO_SLOT
        {
            self.items.set_end(o.slot, line);
            let top = self.slots.pop();
            debug_assert_eq!(top, Some(o.slot), "an ending item is the innermost open");
        }
    }

    /// The class of a group opened by `c` in frame `t` ([F21 §3.3]).
    fn class_of(&self, t: usize, c: u8) -> Class {
        let f = &self.frames[t];
        if f.class == Class::Tree {
            return Class::Tree;
        }
        let [h3, h2, h1] = f.hist;
        let attribute = c == b'['
            && (h1 == Elem::Punct(b'#') || (h1 == Elem::Punct(b'!') && h2 == Elem::Punct(b'#')));
        let invocation = h1 == Elem::Punct(b'!') && matches!(h2, Elem::Word(kw, _) if !kw.in_k());
        let rules = matches!(h1, Elem::Word(_, true))
            && h2 == Elem::Punct(b'!')
            && matches!(h3, Elem::Word(Kw::MacroRules, _));
        if attribute || invocation || rules {
            Class::Tree
        } else if c == b'{' {
            Class::Code
        } else {
            Class::Plain
        }
    }

    fn open_group(&mut self, tok: &Tok, c: u8) {
        let t = self.top();
        let class = self.class_of(t, c);
        self.feed_deeper(t);
        match self.frames[t].class {
            Class::Code => {
                self.code_group(t, tok, c, class);
                self.frames[t].push_hist(Elem::Group);
            }
            Class::Plain => self.frames[t].push_hist(Elem::Group),
            Class::Tree => {}
        }
        self.frames.push(Frame::new(c, class));
        self.open_kind[kind_ix(c)] += 1;
        if self.frames.len() - 1 > RUST_MAX_DEPTH {
            self.failed = true;
        }
    }

    /// A group element of code sequence `t`, at its opener.
    fn code_group(&mut self, t: usize, tok: &Tok, c: u8, class: Class) {
        let f = &mut self.frames[t];
        f.attr = match f.attr {
            Attr::Hash(s) | Attr::HashBang(s) if c == b'[' => Attr::In(s),
            _ => Attr::Idle,
        };
        let Some(o) = f.open else { return };
        match o.phase {
            Phase::Match(M::Vis) if c == b'(' => self.set_phase(t, Phase::Match(M::VisGroup)),
            Phase::Match(M::MacroName) => {
                self.items.set_skind(o.slot, SK_MACRO);
                match self.macro_at.take() {
                    Some((a, b)) => self.items.set_name_ref(o.slot, a, b),
                    None => self.items.set_names(o.slot, &self.macro_name, ""),
                }
                let phase = if c == b'{' {
                    Phase::Body
                } else {
                    Phase::MacroWait
                };
                self.frames[t].open = Some(Open {
                    skind: SK_MACRO,
                    phase,
                    ..o
                });
            }
            Phase::Match(_) => self.fail(t, o),
            Phase::Header { a, p } => {
                let generic_block = a > 0 && matches!(p, Elem::Punct(b'<' | b',' | b'='));
                if c == b'{' && class == Class::Code && !generic_block {
                    if o.skind == SK_IMPL {
                        self.finish_impl(t);
                    }
                    self.set_phase(t, Phase::Body);
                } else {
                    if o.skind == SK_IMPL
                        && let Some(acc) = self.accs.last_mut()
                    {
                        acc.element(tok.kind, self.cur, self.cur_sp, a, &mut self.items);
                        self.sync_live();
                    }
                    self.set_phase(t, Phase::Header { a, p: Elem::Group });
                }
            }
            Phase::UntilSemi => {}
            Phase::MacroAfter(end) => self.end_item(t, end),
            // A group is open above this frame, so no opener reaches it in these phases.
            Phase::MacroWait | Phase::Body => {}
        }
    }

    /// A closer of a kind on the stack: it closes the topmost group of its kind, and every group above it
    /// implicitly ([F21 §3.3] rule 2).
    fn close_group(&mut self, tok: &Tok, c: u8) {
        let want = opener_of(c);
        let Some(j) = self.frames.iter().rposition(|f| f.opener == want) else {
            return;
        };
        while self.top() > j {
            self.close_top(self.last_line);
        }
        self.end_sequence(j);
        self.feed_deeper(j);
        self.frames.pop();
        self.open_kind[kind_ix(c)] -= 1;
        self.child_closed(j - 1, want, tok.line1);
    }

    /// Closes the top group implicitly; its last token ended on line `end`.
    fn close_top(&mut self, end: u64) {
        let t = self.top();
        self.end_sequence(t);
        if let Some(f) = self.frames.pop() {
            self.open_kind[kind_ix(f.opener)] -= 1;
            self.child_closed(t - 1, f.opener, end);
        }
    }

    /// The group just closed, opened by `opener`, was an element of frame `t`; its last token ended on line `end`.
    fn child_closed(&mut self, t: usize, opener: u8, end: u64) {
        let f = &mut self.frames[t];
        if f.class != Class::Code {
            return;
        }
        match f.attr {
            Attr::In(s) => {
                f.start_ok = s;
                f.attr = Attr::Idle;
            }
            _ => f.start_ok = opener == b'{',
        }
        let Some(o) = f.open else { return };
        match o.phase {
            Phase::Match(M::VisGroup) => self.set_phase(t, Phase::Match(M::Quals)),
            Phase::MacroWait => self.set_phase(t, Phase::MacroAfter(end)),
            Phase::Body => self.end_item(t, end),
            _ => {}
        }
    }

    /// Sequence `t` ends (its group closes, or the text ends) before its item's final element: the item ends with
    /// the last element ([F21 §3.6]), a pattern in progress fails.
    fn end_sequence(&mut self, t: usize) {
        let Some(o) = self.frames[t].open else { return };
        let last = self.last_line;
        match o.phase {
            Phase::Match(_) => self.fail(t, o),
            Phase::Header { .. } => {
                if o.skind == SK_IMPL {
                    self.finish_impl(t);
                }
                self.end_item(t, last);
            }
            Phase::MacroAfter(end) => self.end_item(t, end),
            Phase::UntilSemi | Phase::MacroWait | Phase::Body => self.end_item(t, last),
        }
    }

    /// The end of the text: every open group closes implicitly, then the root sequence ends.
    fn eof(&mut self) {
        if self.failed {
            return;
        }
        while self.top() > 0 {
            self.close_top(self.last_line);
        }
        self.end_sequence(0);
    }
}

#[cfg(test)]
mod tests {
    use super::super::{FormKind, FormOutcome, Selector, find_form, scan};
    use super::*;

    fn items(src: &str) -> Vec<String> {
        let it = scan(Lang::Rust, src.as_bytes()).expect("no failure");
        it.iter()
            .map(|i| {
                let kind = Lang::Rust.skind_name(i.skind).unwrap_or("?");
                let qual = if i.qual.is_empty() {
                    String::new()
                } else {
                    format!(" [{}]", i.qual)
                };
                let parent = i.parent.map_or(String::new(), |p| {
                    format!(" in {}", it.get(p).map_or("?", |x| x.name))
                });
                format!("{kind} {}{qual} {}-{}{parent}", i.name, i.start, i.end)
            })
            .collect()
    }

    #[test]
    fn canon_vectors_of_f21_3_2() {
        let v: &[(&[u8], &str)] = &[
            (b"Foo < T , U >", "Foo<T,U>"),
            (b"Vec<\n    Vec<u8>,\n>", "Vec<Vec<u8>,>"),
            (b"[u8; 4]", "[u8;4]"),
            (b"crate :: a :: Foo", "crate::a::Foo"),
            (b"<T as Iterator>::Item", "<T as Iterator>::Item"),
            (b"&'a   mut\tFoo", "&'a mut Foo"),
            (b"& 'static str", "&'static str"),
            (b"dyn Fn(u8) -> u8 + Send + 'a", "dyn Fn(u8)->u8+Send+'a"),
            (b"unsafe  extern \"C\" fn ( )", "unsafe extern \"C\" fn()"),
            (b"extern\"C\" fn()", "extern \"C\" fn()"),
            (b"Foo /* a /* nested */ b */ < T >", "Foo<T>"),
            (b"mut/**/Foo", "mut Foo"),
            (b"Foo<{ A / *B }>", "Foo<{A/ *B}>"),
            (b"A / /B", "A/ /B"),
            (b"Foo<{ \"a  b\" }>", "Foo<{\"a  b\"}>"),
            (b"Foo<{ br#\"a \" b\"# }>", "Foo<{br#\"a \" b\"#}>"),
            (b"Foo<{ \"x\"suffix }>", "Foo<{\"x\"suffix}>"),
            (b"Foo<{ 1.0e-3 }>", "Foo<{1.0e-3}>"),
            (b"mut r#type", "mut r#type"),
            (b"r # type", "r #type"),
            (b"Foo<{ \"a\nb\".len() }>", "Foo<{\"a\\nb\".len()}>"),
            (b"Foo<{ \"a\r\nb\".len() }>", "Foo<{\"a\\nb\".len()}>"),
            (b"Foo<{ \"a\rb\" }>", "Foo<{\"a\\rb\"}>"),
            (b"Foo<{ \"a\0b\" }>", "Foo<{\"a\\0b\"}>"),
            (b"dyn\xE2\x80\xA8Trait", "dyn Trait"),
            (b"S<\xFF>", "S<\u{FFFD}>"),
            (b"S<\xE2\x80\xFF>", "S<\u{FFFD}\u{FFFD}>"),
        ];
        for &(x, want) in v {
            assert_eq!(canon(x), want, "{:?}", String::from_utf8_lossy(x));
        }
    }

    #[test]
    fn public_tokens_report_newlines() {
        let t: Vec<TokenKind> = tokens(b"pub(crate)\nfn f() // c\n{")
            .map(|t| t.kind)
            .collect();
        assert_eq!(
            t,
            [
                TokenKind::Word,
                TokenKind::Punct(b'('),
                TokenKind::Word,
                TokenKind::Punct(b')'),
                TokenKind::Newline,
                TokenKind::Word,
                TokenKind::Word,
                TokenKind::Punct(b'('),
                TokenKind::Punct(b')'),
                TokenKind::Newline,
                TokenKind::Punct(b'{'),
            ]
        );
    }

    #[test]
    fn a_pub_group_holding_an_item() {
        assert_eq!(
            items("pub({ fn x() {} }) fn f() {}"),
            ["fn f 1-1", "fn x 1-1 in f"]
        );
        assert_eq!(items("pub({ fn x() {} }) {}"), ["fn x 1-1"]);
    }

    #[test]
    fn items_inside_impl_headers() {
        assert_eq!(
            items("impl Tr for Foo<{ fn x() {} 1 }> {}"),
            [
                "impl Foo<{fn x(){}1}> [Tr] 1-1",
                "fn x 1-1 in Foo<{fn x(){}1}>"
            ]
        );
        assert_eq!(
            items("impl<T: X<{ fn a() {} }>> { fn b() {} }"),
            ["fn a 1-1", "fn b 1-1"]
        );
        assert_eq!(
            items("mod m { impl A for B }"),
            ["mod m 1-1", "impl B [A] 1-1 in m"]
        );
        assert_eq!(
            items("impl Tr for for<'a> fn(&'a u8) {}"),
            ["impl for<'a>fn(&'a u8) [Tr] 1-1"]
        );
        assert_eq!(
            items("impl<F: Fn() -> u8> Tr for F {}"),
            ["impl F [Tr] 1-1"]
        );
        assert_eq!(items("impl A for <"), ["impl < [A] 1-1"]);
        assert_eq!(items("impl"), Vec::<String>::new());
    }

    #[test]
    fn start_positions_and_attributes() {
        assert_eq!(items("#[a] #[b] pub fn f() {}"), ["fn f 1-1"]);
        assert_eq!(items("# [a]\nfn f() {}"), ["fn f 2-2"]);
        assert_eq!(items("#x fn f() {}"), Vec::<String>::new());
        assert_eq!(items("# # [a] fn f() {}"), Vec::<String>::new());
        assert_eq!(items(") fn f() {}"), Vec::<String>::new());
        assert_eq!(items("} fn f() {}"), ["fn f 1-1"]);
        assert_eq!(items("x fn f() {}"), Vec::<String>::new());
        assert_eq!(items("x; fn f() {}"), ["fn f 1-1"]);
        assert_eq!(items("pub #[a] fn f() {}"), Vec::<String>::new());
    }

    #[test]
    fn extern_abis_are_strings_and_raw_strings() {
        for abi in [
            "\"C\"",
            "\"C\"x",
            "r\"C\"",
            "r#\"C\"#",
            "br\"C\"",
            "cr\"C\"",
            "cr##\"C\"##",
        ] {
            assert_eq!(
                items(&format!("extern {abi} fn f(){{}}")),
                ["fn f 1-1"],
                "{abi}"
            );
            assert_eq!(
                items(&format!("pub unsafe extern {abi} fn f(){{}}")),
                ["fn f 1-1"],
                "{abi}"
            );
        }
        for abi in ["b\"C\"", "c\"C\"", "'C'", "b'C'", "C"] {
            assert_eq!(
                items(&format!("extern {abi} fn f(){{}}")),
                Vec::<String>::new(),
                "{abi}"
            );
        }
        assert_eq!(items("extern fn f(){}"), ["fn f 1-1"]);
    }

    #[test]
    fn patterns_that_fail() {
        assert_eq!(items("extern crate foo; fn f() {}"), ["fn f 1-1"]);
        assert_eq!(items("extern { fn f(); }"), ["fn f 1-1"]);
        assert_eq!(items("async move {} fn f() {}"), ["fn f 1-1"]);
        assert_eq!(items("unsafe { fn f() {} }"), ["fn f 1-1"]);
        assert_eq!(
            items("pub use x; pub type T = u8; pub mod m;"),
            ["mod m 1-1"]
        );
        assert_eq!(items("default = 3; static || 1; fn f() {}"), ["fn f 1-1"]);
        assert_eq!(items("pub (crate) static X: u8 = 1;"), ["static X 1-1"]);
        assert_eq!(items("macro_rules! r#m {}"), ["macro_rules r#m 1-1"]);
        assert_eq!(items("macro_rules!m[\n];"), ["macro_rules m 1-2"]);
        assert_eq!(items("macro_rules! m () fn g() {}"), ["macro_rules m 1-1"]);
        assert_eq!(items("macro_rules! m"), Vec::<String>::new());
        assert_eq!(items("fn"), Vec::<String>::new());
        assert_eq!(items("pub(crate)"), Vec::<String>::new());
    }

    #[test]
    fn classes_of_groups() {
        assert_eq!(items("foo!(fn x() {}); fn y() {}"), ["fn y 1-1"]);
        assert_eq!(items("a::b! { fn x() {} } fn y() {}"), ["fn y 1-1"]);
        assert_eq!(items("r#try!(fn x() {}); fn y() {}"), ["fn y 1-1"]);
        assert_eq!(
            items("fn f() { g(|| { fn inner() {} }) }"),
            ["fn f 1-1", "fn inner 1-1 in f"]
        );
        assert_eq!(
            items("const A: [u8; { fn x() {} 3 }] = [0; 3];"),
            ["const A 1-1", "fn x 1-1 in A"]
        );
        assert_eq!(
            items("fn f() { while !(a) { fn b() {} } }"),
            ["fn f 1-1", "fn b 1-1 in f"]
        );
    }

    #[test]
    fn literals_and_comments_hide_braces() {
        let src = "const S: &str = r#\"{ fn fake() {} }\"#;\n/// fn doc() {}\n/* fn c() { */ fn real() { let c = '}'; let l: &'a u8; }";
        assert_eq!(items(src), ["const S 1-1", "fn real 3-3"]);
        assert_eq!(items("fn f<'a>(x: &'a str) -> char { 'a' }"), ["fn f 1-1"]);
        assert_eq!(
            items("fn f() { b'\\''; '\\u{1F600}'; }\nfn g() {}"),
            ["fn f 1-1", "fn g 2-2"]
        );
    }

    #[test]
    fn more_impl_headers() {
        assert_eq!(items("impl<'a> Foo<'a> {}"), ["impl Foo<'a> 1-1"]);
        assert_eq!(
            items("impl ::std::fmt::Display for X {}"),
            ["impl X [::std::fmt::Display] 1-1"]
        );
        assert_eq!(
            items("impl dyn Trait + Send {}"),
            ["impl dyn Trait+Send 1-1"]
        );
        assert_eq!(items("impl<T> Tr for [T; 3] {}"), ["impl [T;3] [Tr] 1-1"]);
        assert_eq!(items("impl<T: ?Sized> Tr for &T {}"), ["impl &T [Tr] 1-1"]);
        assert_eq!(
            items("impl<T> Tr for Vec<T> where T: Clone {}"),
            ["impl Vec<T> [Tr] 1-1"]
        );
        assert_eq!(
            items("impl<T> Tr for T where for<'a> &'a T: X {}"),
            ["impl T [Tr] 1-1"]
        );
        assert_eq!(
            items("impl Tr for Box<dyn for<'a> Fn(&'a u8)> {}"),
            ["impl Box<dyn for<'a>Fn(&'a u8)> [Tr] 1-1"]
        );
        assert_eq!(
            items("impl<F> Tr for F where F: Fn() -> u8 {}"),
            ["impl F [Tr] 1-1"]
        );
        assert_eq!(
            items("unsafe impl const Tr for S {}"),
            ["impl S [const Tr] 1-1"]
        );
        assert_eq!(items("impl Tr for S;"), ["impl S [Tr] 1-1"]);
    }

    #[test]
    fn more_headers() {
        assert_eq!(
            items("fn f() where T: X<{ 1 }> {}\nfn g() {}"),
            ["fn f 1-1", "fn g 2-2"]
        );
        assert_eq!(
            items("fn f<T>() where T: Iterator<Item = u8> {}"),
            ["fn f 1-1"]
        );
        assert_eq!(items("fn f() -> impl Fn() -> u8 { || 1 }"), ["fn f 1-1"]);
        assert_eq!(
            items("struct S<const N: usize>([u8; N]);\nstruct T(u8) where u8: Copy;"),
            ["struct S 1-1", "struct T 2-2"]
        );
        assert_eq!(
            items("trait T: Clone + Send where Self: Sized {\n}"),
            ["trait T 1-2"]
        );
        assert_eq!(items("enum E<const N: usize = { 1 }> {}"), ["enum E 1-1"]);
    }

    #[test]
    fn a_final_line_feed_inside_an_unterminated_literal_is_part_of_the_name() {
        // The literal runs to the end of the text ([F21 §3.1] rule 4), `0A` included, and an impl's name spells it.
        assert_eq!(items("impl>\""), ["impl >\" 1-1"]);
        assert_eq!(items("impl>\"\n"), ["impl >\"\\n 1-1"]);
    }

    #[test]
    fn shebang_decisions() {
        assert_eq!(items("#!/usr/bin/env x\nfn a() {}"), ["fn a 2-2"]);
        assert_eq!(
            items("#! /* c\nfn b() {} */ x;\nfn a() {}"),
            ["fn b 2-2", "fn a 3-3"]
        );
        assert_eq!(items("#!\n[x]\nfn a() {}"), ["fn a 3-3"]);
        assert_eq!(items("#!"), Vec::<String>::new());
        assert_eq!(items("#"), Vec::<String>::new());
        assert_eq!(items("#!/*"), Vec::<String>::new());
        assert_eq!(items("#!x fn a() {}"), Vec::<String>::new());
        assert_eq!(items("#![a] fn a() {}"), ["fn a 1-1"]);
        assert_eq!(items("#!/**/[a] fn a() {}"), ["fn a 1-1"]);
        assert_eq!(items("#!/ fn a() {}\nfn b() {}"), ["fn b 2-2"]);
        assert_eq!(items("#!//x\n[a]\nfn a() {}"), ["fn a 3-3"]);
    }

    /// Items of `src` fed in pieces of `step` bytes, rendered as [`items`] renders them.
    fn items_fed(src: &[u8], step: usize) -> Vec<String> {
        let mut s = RustScanner::new();
        for p in src.chunks(step) {
            s.feed(p);
        }
        let it = s.finish().expect("no failure");
        it.iter()
            .map(|i| format!("{} {}-{}", i.name, i.start, i.end))
            .collect()
    }

    #[test]
    fn a_long_comment_after_a_shebang_is_read_both_ways_without_holding_it() {
        let body = "c ".repeat(50_000);
        // `[` decides an inner attribute: the comment is trivia.
        let attr = format!("#! /* {body}\n fn hidden() {{}} */ [x]\nfn a() {{}}");
        // Any other token decides a shebang: the comment ends at the first line break.
        let bang = format!("#! /* {body}\nfn b() {{}} */ x;\nfn a() {{}}");
        for step in [1, 7, 4096, usize::MAX] {
            assert_eq!(items_fed(attr.as_bytes(), step), ["a 3-3"], "{step}");
            assert_eq!(
                items_fed(bang.as_bytes(), step),
                ["b 2-2", "a 3-3"],
                "{step}"
            );
        }
        // While undecided, the scanner holds two readings and no bytes of the comment.
        let mut s = RustScanner::new();
        s.feed(b"#! /* ");
        s.feed(body.as_bytes());
        s.feed(b"\nfn b() {}");
        let Shebang::Probe { toks, alt, .. } = &s.sb else {
            panic!("rule 5 is undecided inside the comment");
        };
        assert_eq!(*toks, 2);
        assert_eq!(alt.as_ref().map(|r| r.p.items.len()), Some(1));
        assert!(!s.has_failed());
    }

    #[test]
    fn a_failure_in_the_shebang_reading_counts_only_once_decided() {
        let deep = "(".repeat(1025);
        let attr = format!("#! /*\n{deep} */ [x]\nfn a() {{}}");
        assert_eq!(items_fed(attr.as_bytes(), 3), ["a 3-3"]);
        let bang = format!("#! /*\n{deep} */ x");
        assert!(scan(Lang::Rust, bang.as_bytes()).is_err());
        let mut s = RustScanner::new();
        s.feed(format!("#! /*\n{deep}").as_bytes());
        assert!(!s.has_failed());
        s.feed(b" */ x");
        assert!(s.has_failed());
    }

    #[test]
    fn nested_impl_headers_keep_linear_memory_and_saturate() {
        // 200 nested `impl A for B<{ ` headers around 100,000 tokens: every name is long, and the headers stop reading
        // the inner tokens once saturated.
        let mut src = "impl A for B<{ ".repeat(200);
        src.push_str(&"x ".repeat(100_000));
        let mut s = RustScanner::new();
        s.feed(src.as_bytes());
        assert_eq!(s.main.p.accs.len(), 200);
        assert!(s.main.p.live.is_empty(), "every open header is saturated");
        // Nothing under the cap grows, so the stream keeps the headers' first bytes and none of the 100,000 tokens.
        assert!(s.main.p.items.stream().len() <= 4096);
        s.feed("}> {} ".repeat(200).as_bytes());
        let it = s.finish().expect("no failure");
        assert_eq!(it.len(), 200);
        for i in it.iter() {
            assert!(i.name_long && !i.qual_long && i.qual == "A", "{i:?}");
            assert!(i.name.starts_with("B<{") && i.name.len() <= LONG_PREFIX);
        }
        // 200 records, 64-byte prefixes and bare names: under 150 bytes per item, where whole names took 40 MB.
        assert!(
            it.heap_bytes() <= 200 * 150,
            "{} heap bytes",
            it.heap_bytes()
        );
    }

    #[test]
    fn nested_impl_names_under_the_cap_share_one_spelling() {
        // Names just under the cap are kept whole, and each is a range of the outermost one's spelling.
        let depth = 40;
        let mut src = "impl B<{ ".repeat(depth);
        src.push_str(&"y ".repeat(1500));
        src.push_str(&"}> {} ".repeat(depth));
        let it = scan(Lang::Rust, src.as_bytes()).expect("no failure");
        assert_eq!(it.len(), depth);
        let inner = it.get(depth - 1).expect("the innermost");
        assert_eq!(inner.name.len(), "B<{".len() + 2 * 1500 - 1 + "}>".len());
        assert!(!inner.name_long);
        let outer = it.get(0).expect("the outermost").name;
        assert_eq!(outer, canon(&src.as_bytes()[5..src.len() - 4]));
        for i in it.iter() {
            assert!(outer.contains(i.name) && i.parent == i.index.checked_sub(1));
        }
        // 118 KB of names in the 3,600-byte text, held in about its size.
        let names: usize = it.iter().map(|i| i.name.len()).sum();
        assert!(names > 30 * src.len());
        assert!(it.heap_bytes() <= 2 * src.len(), "{}", it.heap_bytes());
    }

    /// Whether the wall-clock ratios below are checked: only in the `nightly` and `exit` tiers (PLAN §2.1 "Test
    /// tiers"), since tier `pr` runs tests in parallel on loaded hosts and is never a timing gate. Tier `pr` checks
    /// the deterministic work of the same inputs: item counts, stream bytes and heap bytes.
    fn timing_tier() -> bool {
        matches!(
            std::env::var("MOIRAI_TEST_TIER").as_deref(),
            Ok("nightly" | "exit")
        )
    }

    /// The best of three wall times of `f`, in seconds.
    fn best_time(mut f: impl FnMut()) -> f64 {
        (0..3)
            .map(|_| {
                let t = std::time::Instant::now();
                f();
                t.elapsed().as_secs_f64()
            })
            .fold(f64::INFINITY, f64::min)
    }

    #[test]
    fn deeply_nested_impl_headers_stay_linear() {
        // 250 blocks of 500 `impl[{` headers nested in each other's groups: 1 MB of text. Each block's first header
        // reads on through every later block (no `;` or body ends it), so it goes long, and a later block's first
        // `impl` is an element of it, not an item: 124,751 items. Every other name is under the cap, and the names
        // add up to 249 MB; kept whole, one copy each, they took 254 MB and 4 s.
        let block = format!("{}{}", "impl[{".repeat(500), "}]".repeat(500));
        let src = block.repeat(250);
        let n = src.len();
        let mut s = RustScanner::new();
        let mut most = 0;
        for piece in src.as_bytes().chunks(4000) {
            s.feed(piece);
            most = most.max(s.main.p.items.stream().len());
        }
        // The stream holds each token once: never more than the text read.
        assert!(most <= n, "{most} stream bytes");
        let it = s.finish().expect("no failure");
        assert_eq!(it.len(), 500 + 249 * 499);
        let names: usize = it.iter().map(|i| i.name.len()).sum();
        assert!(names > 200 * n);
        // 40 bytes per 8-byte header and the text once: 6 bytes per byte of text.
        assert!(it.heap_bytes() <= 8 * n, "{} heap bytes", it.heap_bytes());
        let first = it.get(0).expect("the outermost");
        assert!(first.name_long && first.name == &block[4..4 + LONG_PREFIX]);
        let second = it.get(1).expect("the next");
        assert_eq!(second.name, &block[10..block.len() - 2]);
        assert_eq!(
            it.get(500).map(|i| i.name),
            Some(&block[10..block.len() - 2])
        );
        // Time: within a small factor of a flat text of as many bytes and items.
        if timing_tier() {
            let flat = "fn a;fn b;".repeat(n / 10);
            let nested = best_time(|| drop(scan(Lang::Rust, src.as_bytes())));
            let plain = best_time(|| drop(scan(Lang::Rust, flat.as_bytes())));
            assert!(nested <= 5.0 * plain, "{nested} s nested, {plain} s flat");
        }
    }

    #[test]
    fn items_inside_failing_groups_cost_no_shifts() {
        // 100,000 items inside 500 nested groups of tentative items that fail — `pub(` groups before a word, and
        // `impl` headers whose self type is empty — cost what the same items cost flat. Removing each tentative
        // slot by shifting the later records cost the depth times the items (45 times the flat time).
        let fns = "fn a(){} ".repeat(100_000);
        let pubs = format!("{}{fns}{}", "pub({ ".repeat(500), "}) x ".repeat(500));
        let impls = format!(
            "{}{fns}{}",
            "impl<T: X<{ ".repeat(500),
            "}>> ; ".repeat(500)
        );
        for src in [&pubs, &impls] {
            let it = scan(Lang::Rust, src.as_bytes()).expect("no failure");
            assert_eq!(it.len(), 100_000);
            assert!(it.iter().all(|i| i.parent.is_none() && i.name == "a"));
        }
        if timing_tier() {
            let plain = best_time(|| drop(scan(Lang::Rust, fns.as_bytes())));
            for src in [&pubs, &impls] {
                let nested = best_time(|| drop(scan(Lang::Rust, src.as_bytes())));
                assert!(nested <= 5.0 * plain, "{nested} s nested, {plain} s flat");
            }
        }
    }

    #[test]
    fn impl_parts_are_ranges_of_the_stream() {
        // A higher-ranked binder in part 0, with an impl nested in it.
        assert_eq!(
            items("impl for<'a> Tr<{ impl X<{}> {} }> for Y {}"),
            [
                "impl Y [for<'a>Tr<{impl X<{}>{}}>] 1-1",
                "impl X<{}> 1-1 in Y"
            ]
        );
        // Part 1 starting at the `<` after `for`, with an item inside it.
        assert_eq!(
            items("impl Tr for <T as X<{ fn a() {} }>>::Y {}"),
            [
                "impl <T as X<{fn a(){}}>>::Y [Tr] 1-1",
                "fn a 1-1 in <T as X<{fn a(){}}>>::Y"
            ]
        );
        // `for` `<` at the end of the header: part 1 is the `<`.
        assert_eq!(items("impl Tr for <"), ["impl < [Tr] 1-1"]);
        // A `where` clause holding an item: the parts stopped before it, the item's name is its own.
        assert_eq!(
            items("impl Tr for X where T: Y<{ fn a() {} }> {}\nfn b() {}"),
            ["impl X [Tr] 1-1", "fn a 1-1 in X", "fn b 2-2"]
        );
        // A macro name inside a header's group, and one outside every header.
        assert_eq!(
            items("impl A<{ macro_rules! m {} }> {} macro_rules! n {}"),
            [
                "impl A<{macro_rules!m{}}> 1-1",
                "macro_rules m 1-1 in A<{macro_rules!m{}}>",
                "macro_rules n 1-1"
            ]
        );
    }

    #[test]
    fn long_parts_around_nested_items() {
        let big = format!("\"{}\"", "z".repeat(5000));
        // A long qualifier with items inside it, then a self type holding an item.
        let src = format!(
            "impl Tr<{{ fn a() {{}} {big}; fn b() {{}} }}> for Foo<{{ fn c() {{}} }}> {{}}"
        );
        let it = scan(Lang::Rust, src.as_bytes()).expect("no failure");
        let got: Vec<(&str, &str, bool, bool, Option<usize>)> = it
            .iter()
            .map(|i| (i.name, i.qual, i.name_long, i.qual_long, i.parent))
            .collect();
        let prefix = &canon(format!("Tr<{{ fn a() {{}} {big}").as_bytes())[..LONG_PREFIX];
        assert_eq!(
            got,
            [
                ("Foo<{fn c(){}}>", prefix, false, true, None),
                ("a", "", false, false, Some(0)),
                ("b", "", false, false, Some(0)),
                ("c", "", false, false, Some(0)),
            ]
        );
        assert_eq!(it.qual_stored(0).bare, Some((false, "Tr")));
        // The stream kept the names and the prefix, not the 5,000-byte literal.
        assert!(it.heap_bytes() < 4 * 40 + 200, "{}", it.heap_bytes());
        // With 40 KB of names after the qualifier went long, its prefix is out of the name's reach and is copied.
        let many = "fn nnnnnnnnnn() {} ".repeat(4000);
        let src = format!("impl Tr<{{ {big}; {many} }}> for Foo<{{ fn c() {{}} }}> {{}}");
        let it = scan(Lang::Rust, src.as_bytes()).expect("no failure");
        let imp = it.get(0).expect("the impl");
        let prefix = &canon(format!("Tr<{{ {big}").as_bytes())[..LONG_PREFIX];
        assert_eq!(
            (imp.name, imp.qual, imp.qual_long),
            ("Foo<{fn c(){}}>", prefix, true)
        );
        assert_eq!(it.len(), 4002);
        assert!(it.iter().skip(1).all(|i| i.parent == Some(0)));
        assert_eq!(it.get(4001).map(|i| i.name), Some("c"));
        assert!(it.heap_bytes() < 4002 * 40 + 4000 * 10 + 300);
        // An impl nested in a long self type keeps its own whole name.
        let src = format!("impl Tr for Foo<{{ impl In<{{ {big} }}> {{}} impl Ok<u8> {{}} }}> {{}}");
        let it = scan(Lang::Rust, src.as_bytes()).expect("no failure");
        let got: Vec<(&str, bool)> = it.iter().map(|i| (i.name, i.name_long)).collect();
        assert_eq!(
            got,
            [
                (&canon(src.as_bytes())[12..12 + LONG_PREFIX], true),
                (
                    &canon(format!("In<{{ {big} }}>").as_bytes())[..LONG_PREFIX],
                    true
                ),
                ("Ok<u8>", false),
            ]
        );
    }

    fn form(src: &str, selector: &str) -> FormOutcome {
        let sel = Selector::parse(FormKind::Symbol, selector).expect("a selector");
        find_form(&scan(Lang::Rust, src.as_bytes()), &sel)
    }

    #[test]
    fn long_impl_names_match_by_their_bare_names() {
        let big = format!("\"{}\"", "z".repeat(5000));
        let long_self = format!("impl Tr for Foo<{{ {big} }}> {{ fn get() {{}} }}");
        let it = scan(Lang::Rust, long_self.as_bytes()).expect("no failure");
        let imp = it.get(0).expect("the impl");
        assert!(imp.name_long && imp.name.starts_with("Foo<{\"zzz") && imp.qual == "Tr");
        assert_eq!(it.name_stored(0).bare, Some((false, "Foo")));
        assert!(it.path_text(1).starts_with("rust:impl Foo<{\"zzz"));
        assert!(it.path_text(1).ends_with("%\u{2026}[Tr]/fn get"));
        // Matched by the bare name, the one item is not recordable; beside a short twin, both count.
        assert_eq!(
            form(&long_self, "Foo[Tr]/get"),
            FormOutcome::NotRecordable(1)
        );
        let twins = format!("{long_self}\nimpl Tr for Foo<u8> {{ fn get() {{}} }}");
        assert_eq!(
            form(&twins, "Foo[Tr]/get"),
            FormOutcome::Several(vec![1, 3])
        );
        assert_eq!(form(&twins, "Foo<u8>[Tr]/get"), FormOutcome::Found(3));
        // A long qualifier keeps its bare name too; a long self type without one matches nothing.
        let long_trait = format!("impl crate::Tr<{{ {big} }}> for X {{}}");
        let it = scan(Lang::Rust, long_trait.as_bytes()).expect("no failure");
        assert_eq!(it.qual_stored(0).bare, Some((false, "Tr")));
        assert_eq!(form(&long_trait, "X[Tr]"), FormOutcome::NotRecordable(0));
        let tuple = format!("impl Tr for ({big}, u8) {{}}");
        assert_eq!(form(&tuple, "X[Tr]"), FormOutcome::NotFound);
        let it = scan(Lang::Rust, tuple.as_bytes()).expect("no failure");
        assert_eq!(it.name_stored(0).bare, None);
        // A single-token name over the cap is long and matches nothing: its bare name is itself.
        let word = "w".repeat(5000);
        let f = format!("fn {word}() {{}}");
        let it = scan(Lang::Rust, f.as_bytes()).expect("no failure");
        assert!(
            it.get(0)
                .is_some_and(|i| i.name_long && i.name.len() == LONG_PREFIX)
        );
        assert_eq!(form(&f, &"w".repeat(4096)), FormOutcome::NotFound);
    }

    #[test]
    fn the_lexer_keeps_a_capped_token() {
        let mut lx = Lexer::new(0, false).capped(8);
        let x = b"\"0123456789abcdef\" r##\"0123456789\"## word";
        let mut i = 0;
        let mut kept = Vec::new();
        while let Some(t) = lx.next(x, &mut i, true, true) {
            kept.push((t.off, t.end, lx.spelled().to_vec()));
        }
        assert_eq!(
            kept,
            [
                (0, 18, b"\"0123456".to_vec()),
                (19, 36, b"r##\"0123".to_vec()),
                (37, 41, b"word".to_vec())
            ]
        );
    }
}
