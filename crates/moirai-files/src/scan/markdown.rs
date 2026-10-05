//! The Markdown scanner ([F21 §4]): a fence-aware ATX and setext line scanner with HTML comment blocks, front
//! matter and container first lines; heading names and numbering (§4.7); levels, parents and sections (§4.8).
//!
//! # Streaming
//!
//! The scanner reads each byte once and holds no line, so its memory does not grow with the length of a line
//! ([F21 §2.8]). The class of a line (§4.2) is decided at its end from predicates that each keep a few bytes of state
//! while its bytes pass ([`Line`]): the indentation column; the first byte of `rest(l)`, the length of its leading
//! run, and whether only SP and HT, and whether a backtick, came after that run (setext underlines, fences); a
//! thematic-break count; the state of a container marker; how much of `<!--` matched; and a search for `-->`.
//!
//! Text goes where it may be needed as it comes, each piece capped at [`KEEP`] bytes ([`HeadingText`]):
//!
//! - an ATX heading's content, once its opener has matched, into the heading's text. A run of `#` that may be the
//!   closing sequence is held back as a count until a byte other than SP, HT and `#` shows that it is not;
//! - every line read while no fence and no comment is open, into the paragraph text. Whatever the line turns out to
//!   be, that text is then the open paragraph's with the line appended (rules 5.5 and 6), the paragraph the line
//!   starts (rule 8), or dropped with the paragraph (rules 3 and 5). The one exception is rule 4: a setext underline
//!   makes a heading of the paragraph's text without the line. So while a paragraph is open, a shallow line whose
//!   `rest` so far is a run of `=` or `-` and then SP and HT is held back as a count and a flag, until a byte shows
//!   that it is no underline.
//!
//! Front matter needs its closing line before it is known to be front matter (§4.5). While line 1 is an open `---`,
//! the lines after it are scanned as ordinary lines and their headings kept; the first later `---` or `...` line
//! discards them and restarts the scan after it, and the end of the text keeps them.

use std::borrow::Cow;

use super::items::{Items, NO_PARENT};
use super::{
    Lang, MD_FENCE_MIN, MD_MAX_INDENT, MD_MAX_LEVEL, MD_TAB_STOP, NUM_MAX_DIGITS, SCOPE_MAX_BYTES,
};

const SP: u8 = b' ';
const HT: u8 = b'\t';

fn is_sp_ht(b: u8) -> bool {
    b == SP || b == HT
}

/// The most bytes kept of a heading's numbering and of its name: every name that can be recordable has at most
/// [`SCOPE_MAX_BYTES`] bytes, and one byte more tells a longer one, since U+FFFD replacement never shortens bytes
/// ([F21 §2.3]).
const KEEP: usize = SCOPE_MAX_BYTES + 1;

/// Appends as much of `bytes` to `v` as keeps it within [`KEEP`] bytes.
fn push_kept(v: &mut Vec<u8>, bytes: &[u8]) {
    let room = KEEP.saturating_sub(v.len());
    v.extend_from_slice(&bytes[..bytes.len().min(room)]);
}

/// Where the numbering prefix of [F21 §4.7] step 2 stands over a heading's collapsed content.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Num {
    /// Nothing read.
    #[default]
    Start,
    /// `C2`, the first byte of `§`.
    Sect,
    /// After `§`.
    SectEnd,
    /// After `§` and its SP.
    SectSp,
    /// A run of n ≥ 1 digits of a component.
    Digits(u8),
    /// A capital letter as the first component.
    Letter,
    /// A `.` after a component: a further component or the final `.`.
    Dot,
    /// After the final `.` or `)`.
    Closed,
    /// The numbering and the SP after it matched; a dash separator may follow.
    Sep,
    /// Decided: every further byte is the name's.
    Name,
}

/// A heading's content read in pieces ([F21 §4.3, §4.4], §4.7 steps 1–2): runs of SP and HT collapsed to one SP and
/// dropped at both ends, the numbering prefix matched byte by byte, and the numbering and the name each kept to
/// [`KEEP`] bytes. A paragraph that may yet become a setext heading is read into one as its lines come, so no
/// paragraph is held whole.
#[derive(Debug, Default)]
struct HeadingText {
    /// A run of SP or HT is pending.
    gap: bool,
    /// A byte other than SP and HT has been read.
    started: bool,
    num: Num,
    /// The numbering, or while it is undecided the bytes that may be one.
    qual: Vec<u8>,
    /// In [`Num::Sep`], the bytes after the SP.
    sep: Vec<u8>,
    /// The dash separator skipped: 0 none, 1 `-`, 2 `–`, 3 `—`.
    dash: u8,
    /// The name.
    name: Vec<u8>,
}

impl HeadingText {
    /// Empty again; the buffers, which never hold more than [`KEEP`] bytes, keep their capacity.
    fn clear(&mut self) {
        self.gap = false;
        self.started = false;
        self.num = Num::Start;
        self.qual.clear();
        self.sep.clear();
        self.dash = 0;
        self.name.clear();
    }

    /// The next bytes of the content.
    fn feed(&mut self, bytes: &[u8]) {
        for &b in bytes {
            if self.num == Num::Name && self.name.len() >= KEEP {
                // The name is long; nothing after it changes a field.
                return;
            }
            if is_sp_ht(b) {
                self.gap = true;
                continue;
            }
            if self.gap && self.started {
                self.emit(SP);
            }
            self.gap = false;
            self.started = true;
            self.emit(b);
        }
    }

    /// A byte of the collapsed content.
    fn emit(&mut self, b: u8) {
        let digit = b.is_ascii_digit();
        let letter = b.is_ascii_uppercase();
        let next = match self.num {
            Num::Name => {
                push_kept(&mut self.name, &[b]);
                return;
            }
            Num::Sep => {
                self.sep.push(b);
                self.dash_step();
                return;
            }
            Num::Start if b == 0xC2 => Num::Sect,
            Num::Sect if b == 0xA7 => Num::SectEnd,
            Num::SectEnd if b == SP => Num::SectSp,
            Num::Start | Num::SectEnd | Num::SectSp | Num::Dot if digit => Num::Digits(1),
            Num::Start | Num::SectEnd | Num::SectSp if letter => Num::Letter,
            Num::Digits(n) if digit && usize::from(n) < NUM_MAX_DIGITS => Num::Digits(n + 1),
            Num::Digits(_) | Num::Letter if b == b'.' => Num::Dot,
            Num::Digits(_) | Num::Letter if b == b')' => Num::Closed,
            Num::Digits(_) | Num::Dot | Num::Closed if b == SP => {
                self.num = Num::Sep;
                return;
            }
            _ => {
                // No numbering: what was read, and this byte, begin the name (which is empty so far).
                self.num = Num::Name;
                std::mem::swap(&mut self.name, &mut self.qual);
                push_kept(&mut self.name, &[b]);
                return;
            }
        };
        push_kept(&mut self.qual, &[b]);
        self.num = next;
    }

    /// After a byte of a possible dash separator: `-`, `–` or `—`, then SP.
    fn dash_step(&mut self) {
        for (code, d) in [(1u8, "- "), (2, "\u{2013} "), (3, "\u{2014} ")] {
            let d = d.as_bytes();
            if self.sep == d {
                self.dash = code;
                self.sep.clear();
                self.num = Num::Name;
                return;
            }
            if d.starts_with(&self.sep) {
                return;
            }
        }
        // No separator: its bytes begin the name.
        self.num = Num::Name;
        std::mem::swap(&mut self.name, &mut self.sep);
    }

    /// The heading's numbering, name and dash separator ([F21 §4.7] step 3, before U+FFFD replacement).
    fn fields(&self) -> (&[u8], Cow<'_, [u8]>, u8) {
        match self.num {
            Num::Name | Num::Sep if !self.qual.is_empty() => {
                let name: &[u8] = if self.num == Num::Sep {
                    &self.sep
                } else {
                    &self.name
                };
                if !name.is_empty() {
                    return (&self.qual, Cow::Borrowed(name), self.dash);
                }
                // "If nothing is left after them, there is no numbering": the whole content is the name.
                let mut whole = self.qual.clone();
                whole.push(SP);
                if self.dash > 0 {
                    whole.extend_from_slice(dash_bytes(self.dash));
                }
                (&[], Cow::Owned(whole), 0)
            }
            Num::Name => (&[], Cow::Borrowed(&self.name), 0),
            // The numbering never completed: what was read is the name.
            _ => (&[], Cow::Borrowed(&self.qual), 0),
        }
    }
}

/// A dash separator's bytes with its SP.
fn dash_bytes(dash: u8) -> &'static [u8] {
    match dash {
        1 => b"- ",
        2 => "\u{2013} ".as_bytes(),
        _ => "\u{2014} ".as_bytes(),
    }
}

/// Front matter at line 1 ([F21 §4.5]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Front {
    /// Line 1 has not come.
    Line1,
    /// Line 1 opened front matter whose closing line has not come.
    Open,
    /// Decided.
    Off,
}

/// Feeds `count` bytes `byte` — `=`, `-` or `#` — to `text`. Such a run puts the text's state in its name within two
/// bytes (a `-` may first be read as the start of a dash separator, [F21 §4.7]), and the name keeps at most [`KEEP`]
/// bytes, so no byte after the first `KEEP + 2` changes it.
fn feed_run(text: &mut HeadingText, byte: u8, count: usize) {
    let block = [byte; 64];
    let mut left = count.min(KEEP + 2);
    while left > 0 {
        let k = left.min(block.len());
        text.feed(&block[..k]);
        left -= k;
    }
}

/// How the bytes of a line are read, from the state at its start ([F21 §4.2] rules 1 and 2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    /// A fence of this byte and length is open: the line is code, or closes it (rule 1).
    Fence(u8, usize),
    /// An HTML comment block is open (rule 2).
    Comment,
    /// Neither: rules 3–9.
    Blocks,
}

/// A container marker over `rest(l)` as far as it has been read ([F21 §4.6]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Marker {
    /// `>`, which can interrupt a paragraph.
    Quote,
    /// `-`, `+` or `*`.
    Bullet,
    /// `n` ASCII digits: `zero` while their value is 0, `one` when it is 1.
    Digits { n: u8, zero: bool, one: bool },
    /// Digits and `.` or `)`; `one` when their value is 1.
    Delim { one: bool },
    /// A marker and SP or HT: `one` for a bullet or an ordered marker of value 1, `content` once a byte other than
    /// SP and HT came after it.
    Spaced { one: bool, content: bool },
    /// No marker.
    No,
}

impl Marker {
    /// At the end of the line: for a marker, whether it can interrupt a paragraph.
    fn result(self) -> Option<bool> {
        match self {
            Marker::Quote => Some(true),
            Marker::Bullet | Marker::Delim { .. } => Some(false),
            Marker::Spaced { one, content } => Some(one && content),
            Marker::Digits { .. } | Marker::No => None,
        }
    }
}

/// An ATX heading over `rest(l)` as far as it has been read ([F21 §4.3]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Atx {
    /// `n` bytes `#`, 1 ≤ n ≤ [`MD_MAX_LEVEL`].
    Hashes(u8),
    /// The opener and SP or HT: a heading of this level, whose content follows.
    Content(u8),
    /// No ATX heading.
    No,
}

/// The opener of an HTML comment block ([F21 §4.5]).
const COMMENT_OPENER: &[u8] = b"<!--";

/// [`Line::opener`] once `rest(l)` differs from [`COMMENT_OPENER`].
const OPENER_NO: u8 = u8::MAX;

/// The line being read, as far as it has been read: what each rule of [F21 §4.2] needs of it.
#[derive(Clone, Copy, Debug)]
struct Line {
    mode: Mode,
    /// The raw bytes read, counted up to 3; whether they are `---`, or `...`, so far; and whether every later byte
    /// is SP or HT (front matter, §4.5). Kept only while front matter is undecided.
    raw: u8,
    dashes: bool,
    dots: bool,
    front_tail: bool,
    /// How many bytes of `-->` the last bytes read match (0–2), and whether the line holds `-->` (rules 2, 5.3).
    arrow: u8,
    has_arrow: bool,
    /// The column reached by the leading SP and HT (§4.1).
    col: usize,
    /// `rest(l)`'s first byte, once one came; until then the line is blank (rule 3).
    first: Option<u8>,
    /// `ind(l)` ≤ [`MD_MAX_INDENT`], decided at `rest(l)`'s first byte.
    shallow: bool,
    /// The length of `rest(l)`'s leading run of its first byte, and whether that run goes on.
    run: usize,
    running: bool,
    /// Only SP and HT came after the run: a setext underline or a closing fence when the first byte fits (§4.4,
    /// §4.5).
    blank_tail: bool,
    /// A backtick came after the run: no backtick fence opener (§4.5).
    tick: bool,
    /// `rest(l)` is so far only its first byte, one of `-`, `*` and `_`, and SP and HT, with `marks` of the first
    /// byte counted up to 3 (rule 5.4).
    thematic: bool,
    marks: u8,
    marker: Marker,
    /// How many bytes of [`COMMENT_OPENER`] `rest(l)` starts with so far, or [`OPENER_NO`].
    opener: u8,
    atx: Atx,
    /// The ATX content's held-back run of `#` (its length; 0 for none) and whether SP or HT came after it; and
    /// whether the content so far is empty or ends in SP or HT, so that a run starting now may be the closing
    /// sequence.
    hashes: usize,
    hashes_gap: bool,
    after_gap: bool,
    /// The line is being read into the paragraph text.
    para: bool,
    /// A possible setext underline is held back from the paragraph text: `rest(l)` so far is `run` bytes `first`,
    /// then SP or HT when `held_gap`.
    held: bool,
    held_gap: bool,
}

impl Line {
    /// A line about to be read in `mode`.
    fn new(mode: Mode) -> Line {
        Line {
            mode,
            raw: 0,
            dashes: true,
            dots: true,
            front_tail: true,
            arrow: 0,
            has_arrow: false,
            col: 0,
            first: None,
            shallow: false,
            run: 0,
            running: false,
            blank_tail: true,
            tick: false,
            thematic: false,
            marks: 0,
            marker: Marker::No,
            opener: OPENER_NO,
            atx: Atx::No,
            hashes: 0,
            hashes_gap: false,
            after_gap: true,
            para: mode == Mode::Blocks,
            held: false,
            held_gap: false,
        }
    }

    /// Whether the line is `---`, or when `closing` also `...`, followed only by SP and HT ([F21 §4.5]).
    fn front(&self, closing: bool) -> bool {
        self.raw == 3 && self.front_tail && (self.dashes || (closing && self.dots))
    }

    fn front_byte(&mut self, b: u8) {
        if self.raw < 3 {
            self.dashes &= b == b'-';
            self.dots &= b == b'.';
            self.raw += 1;
        } else if !is_sp_ht(b) {
            self.front_tail = false;
        }
    }

    /// The search for `-->`: a `-` extends a match, a `>` after two of them completes it, any other byte ends it.
    fn arrow_byte(&mut self, b: u8) {
        self.arrow = match (b, self.arrow) {
            (b'-', a) => (a + 1).min(2),
            (b'>', 2) => {
                self.has_arrow = true;
                0
            }
            _ => 0,
        };
    }

    /// A byte of the indentation: SP advances one column, HT to the next tab stop ([F21 §4.1]).
    fn indent_byte(&mut self, b: u8) {
        self.col = if b == HT {
            (self.col / MD_TAB_STOP)
                .saturating_add(1)
                .saturating_mul(MD_TAB_STOP)
        } else {
            self.col.saturating_add(1)
        };
    }

    /// `rest(l)`'s first byte `b`; `para_open` when a paragraph is open.
    fn first_byte(&mut self, b: u8, para_open: bool) {
        let blocks = self.mode == Mode::Blocks;
        self.first = Some(b);
        self.shallow = self.col <= MD_MAX_INDENT;
        self.run = 1;
        self.running = true;
        self.thematic = matches!(b, b'-' | b'*' | b'_');
        self.marks = 1;
        self.marker = match b {
            b'>' => Marker::Quote,
            b'-' | b'+' | b'*' => Marker::Bullet,
            b'0'..=b'9' => Marker::Digits {
                n: 1,
                zero: b == b'0',
                one: b == b'1',
            },
            _ => Marker::No,
        };
        self.opener = if b == COMMENT_OPENER[0] { 1 } else { OPENER_NO };
        self.atx = if blocks && self.shallow && b == b'#' {
            Atx::Hashes(1)
        } else {
            Atx::No
        };
        self.held = blocks && para_open && self.shallow && (b == b'=' || b == b'-');
    }

    /// A later byte `b` of `rest(l)`, for every predicate but the ATX content's closing sequence.
    fn rest_byte(&mut self, b: u8) {
        let ws = is_sp_ht(b);
        let first = self.first.unwrap_or(b);
        if self.running && b == first {
            self.run = self.run.saturating_add(1);
        } else {
            self.running = false;
            self.blank_tail &= ws;
            self.tick |= b == b'`';
        }
        if self.thematic {
            if b == first {
                self.marks = (self.marks + 1).min(3);
            } else if !ws {
                self.thematic = false;
            }
        }
        self.marker = match self.marker {
            Marker::Quote => Marker::Quote,
            Marker::Bullet if ws => Marker::Spaced {
                one: true,
                content: false,
            },
            Marker::Digits { n, zero, .. } if b.is_ascii_digit() => {
                if usize::from(n) < NUM_MAX_DIGITS {
                    Marker::Digits {
                        n: n + 1,
                        zero: zero && b == b'0',
                        one: zero && b == b'1',
                    }
                } else {
                    Marker::No
                }
            }
            Marker::Digits { one, .. } if b == b'.' || b == b')' => Marker::Delim { one },
            Marker::Delim { one } if ws => Marker::Spaced {
                one,
                content: false,
            },
            Marker::Spaced { one, content } => Marker::Spaced {
                one,
                content: content || !ws,
            },
            _ => Marker::No,
        };
        if let Some(&want) = COMMENT_OPENER.get(usize::from(self.opener)) {
            self.opener = if b == want {
                self.opener + 1
            } else {
                OPENER_NO
            };
        }
        if let Atx::Hashes(n) = self.atx {
            self.atx = if b == b'#' && usize::from(n) < MD_MAX_LEVEL {
                Atx::Hashes(n + 1)
            } else if ws {
                // The line is an ATX heading (rules 1–4 cannot apply to it), so it joins no paragraph.
                self.para = false;
                Atx::Content(n)
            } else {
                Atx::No
            };
        }
    }
}

/// The line scanner's state ([F21 §4.2]).
#[derive(Debug)]
struct Core {
    /// The number of lines begun.
    n: u64,
    front: Front,
    fence: Option<(u8, usize)>,
    comment: bool,
    /// The first line of the open paragraph.
    para: Option<u64>,
    /// The open paragraph's lines, joined by SP, as the content of a setext heading they may become.
    para_text: HeadingText,
    cont: bool,
    items: Items,
    /// The headings whose sections are open, by increasing level: (slot, level).
    open: Vec<(usize, u8)>,
    /// An ATX heading's content.
    atx_text: HeadingText,
    /// The line being read.
    line: Line,
}

impl Core {
    fn reset(&mut self) {
        self.fence = None;
        self.comment = false;
        self.para = None;
        self.cont = false;
        self.items.clear();
        self.open.clear();
    }

    /// A line begins: its mode, and the paragraph text it is read into (the open paragraph's after a joining SP,
    /// or a new one).
    fn begin_line(&mut self) {
        self.n += 1;
        let mode = match self.fence {
            Some((c, n)) => Mode::Fence(c, n),
            None if self.comment => Mode::Comment,
            None => Mode::Blocks,
        };
        self.line = Line::new(mode);
        if mode == Mode::Blocks {
            if self.para.is_some() {
                self.para_text.feed(&[SP]);
            } else {
                self.para_text.clear();
            }
        }
    }

    /// The next bytes of the line being read; none is `0A`.
    fn bytes(&mut self, seg: &[u8]) {
        // `seg[from..]` is not yet read into the paragraph text.
        let mut from = 0;
        for (i, &b) in seg.iter().enumerate() {
            if self.front != Front::Off {
                self.line.front_byte(b);
            }
            self.line.arrow_byte(b);
            let Some(first) = self.line.first else {
                if is_sp_ht(b) {
                    self.line.indent_byte(b);
                } else {
                    self.line.first_byte(b, self.para.is_some());
                    if self.line.held {
                        self.para_text.feed(&seg[from..i]);
                        from = i + 1;
                    }
                }
                continue;
            };
            if self.line.held {
                if self.line.running && b == first {
                    from = i + 1;
                } else if is_sp_ht(b) {
                    self.line.held_gap = true;
                    from = i + 1;
                } else {
                    // No underline: what was held back joins the paragraph text before `b`.
                    self.line.held = false;
                    feed_run(&mut self.para_text, first, self.line.run);
                    if self.line.held_gap {
                        self.para_text.feed(&[SP]);
                    }
                    from = i;
                }
            }
            let atx = self.line.atx;
            if let Atx::Content(_) = atx {
                self.atx_byte(b);
            }
            self.line.rest_byte(b);
            if let (Atx::Hashes(_), Atx::Content(_)) = (atx, self.line.atx) {
                self.atx_text.clear();
            }
        }
        if self.line.para {
            self.para_text.feed(&seg[from..]);
        }
    }

    /// A byte of an ATX heading's content ([F21 §4.3]): trimming and collapsing are [`HeadingText`]'s; the
    /// optional closing sequence — a final run of `#` that is the whole content or follows SP or HT — is a run
    /// held back here and dropped when the line ends with it.
    fn atx_byte(&mut self, b: u8) {
        let Core { line, atx_text, .. } = self;
        if is_sp_ht(b) {
            if line.hashes > 0 {
                line.hashes_gap = true;
            } else {
                atx_text.feed(&[b]);
            }
            line.after_gap = true;
            return;
        }
        let held = line.hashes;
        if b == b'#' && held > 0 && !line.hashes_gap {
            line.hashes = held.saturating_add(1);
        } else {
            let gap = if held > 0 {
                line.hashes_gap
            } else {
                line.after_gap
            };
            if held > 0 {
                feed_run(atx_text, b'#', held);
                if line.hashes_gap {
                    atx_text.feed(&[SP]);
                }
                line.hashes = 0;
                line.hashes_gap = false;
            }
            if b == b'#' && gap {
                line.hashes = 1;
            } else {
                atx_text.feed(&[b]);
            }
        }
        line.after_gap = false;
    }

    /// The end of the line being read.
    fn end_line(&mut self) {
        match self.front {
            Front::Line1 => {
                self.front = if self.line.front(false) {
                    Front::Open
                } else {
                    Front::Off
                };
            }
            Front::Open if self.line.front(true) => {
                self.reset();
                self.front = Front::Off;
                return;
            }
            Front::Open | Front::Off => {}
        }
        self.classify();
    }

    /// The first rule of [F21 §4.2] that applies to the line just read.
    fn classify(&mut self) {
        let l = self.line;
        match l.mode {
            Mode::Fence(c, n) => {
                if l.shallow && l.first == Some(c) && l.run >= n && l.blank_tail {
                    self.fence = None;
                }
                return;
            }
            Mode::Comment => {
                if l.has_arrow {
                    self.comment = false;
                }
                return;
            }
            Mode::Blocks => {}
        }
        let Some(first) = l.first else {
            // Rule 3: a blank line.
            self.para = None;
            self.cont = false;
            return;
        };
        if let Some(start) = self.para
            && l.shallow
            && (first == b'=' || first == b'-')
            && l.blank_tail
        {
            // Rule 4: a setext underline, held back from the paragraph text to the end.
            debug_assert!(l.held, "an underline is held back");
            let text = std::mem::take(&mut self.para_text);
            self.heading(if first == b'=' { 1 } else { 2 }, start, &text);
            self.para_text = text;
            self.para = None;
            return;
        }
        if l.shallow {
            let level = match l.atx {
                Atx::Hashes(n) => {
                    self.atx_text.clear();
                    Some(n)
                }
                Atx::Content(n) => Some(n),
                Atx::No => None,
            };
            if let Some(level) = level {
                self.para = None;
                self.cont = false;
                let text = std::mem::take(&mut self.atx_text);
                self.heading(level, self.n, &text);
                self.atx_text = text;
                return;
            }
            if matches!(first, b'`' | b'~') && l.run >= MD_FENCE_MIN && !(first == b'`' && l.tick) {
                self.fence = Some((first, l.run));
                self.para = None;
                self.cont = false;
                return;
            }
            if usize::from(l.opener) == COMMENT_OPENER.len() {
                self.comment = !l.has_arrow;
                self.para = None;
                self.cont = false;
                return;
            }
            if l.thematic && l.marks >= 3 {
                self.para = None;
                self.cont = false;
                return;
            }
            if let Some(interrupts) = l.marker.result() {
                // A marker that cannot interrupt the open paragraph was read into it.
                if self.para.is_none() || interrupts {
                    self.para = None;
                    self.cont = true;
                }
                return;
            }
        }
        if self.para.is_none() && !self.cont && l.shallow {
            // Rule 8: the line, read into a new paragraph text, starts a paragraph.
            self.para = Some(self.n);
        }
        // Otherwise rule 6 (the line was read into the open paragraph), 7 (a container line) or 9 (code).
    }

    /// A heading of level `level` whose header line is `start`, with content `text` ([F21 §4.7, §4.8]).
    fn heading(&mut self, level: u8, start: u64, text: &HeadingText) {
        let (qual, name, dash) = text.fields();
        if name.is_empty() {
            return;
        }
        while let Some(&(slot, lv)) = self.open.last() {
            if lv < level {
                break;
            }
            self.items.set_end(slot, start - 1);
            self.open.pop();
        }
        let parent = self.open.last().map_or(NO_PARENT, |&(s, _)| s);
        let slot = self.items.push(level, start, parent);
        self.items.set_names(
            slot,
            &String::from_utf8_lossy(&name),
            &String::from_utf8_lossy(qual),
        );
        self.items.set_dash(slot, dash);
        self.open.push((slot, level));
    }

    /// The end of the text: every open section runs to the last line.
    fn finish(mut self) -> Items {
        for &(slot, _) in &self.open {
            self.items.set_end(slot, self.n);
        }
        self.items.shrink();
        self.items
    }
}

/// The Markdown scanner over chunks of an anchor text.
#[derive(Debug)]
pub(crate) struct MarkdownScanner {
    core: Core,
    /// A line has begun and its `0A` has not come.
    in_line: bool,
}

impl MarkdownScanner {
    pub(crate) fn new() -> MarkdownScanner {
        MarkdownScanner {
            core: Core {
                n: 0,
                front: Front::Line1,
                fence: None,
                comment: false,
                para: None,
                para_text: HeadingText::default(),
                cont: false,
                items: Items::new(Lang::Markdown),
                open: Vec::new(),
                atx_text: HeadingText::default(),
                line: Line::new(Mode::Blocks),
            },
            in_line: false,
        }
    }

    /// The lines of `lines(t)` ([F20 §2.5]): every `0A` ends one, and the text's end ends a last one that has a
    /// byte.
    pub(crate) fn feed(&mut self, chunk: &[u8]) {
        let mut rest = chunk;
        while !rest.is_empty() {
            if !self.in_line {
                self.core.begin_line();
                self.in_line = true;
            }
            if let Some(k) = rest.iter().position(|&b| b == b'\n') {
                self.core.bytes(&rest[..k]);
                self.core.end_line();
                self.in_line = false;
                rest = &rest[k + 1..];
            } else {
                self.core.bytes(rest);
                break;
            }
        }
    }

    pub(crate) fn finish(mut self) -> Items {
        if self.in_line {
            self.core.end_line();
        }
        self.core.finish()
    }

    /// The bytes the scanner's state holds besides its items.
    #[cfg(test)]
    fn state_bytes(&self) -> usize {
        let text = |h: &HeadingText| h.qual.capacity() + h.sep.capacity() + h.name.capacity();
        text(&self.core.para_text)
            + text(&self.core.atx_text)
            + self.core.open.capacity() * size_of::<(usize, u8)>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbering_of_f21_4_7() {
        let v: &[(&str, Option<(&str, &str)>)] = &[
            ("3.2 Recovery", Some(("3.2", "Recovery"))),
            ("§3 Storage", Some(("§3", "Storage"))),
            ("§ 3.1 Storage", Some(("§ 3.1", "Storage"))),
            ("A.1 Parts", Some(("A.1", "Parts"))),
            ("A. Intro", Some(("A.", "Intro"))),
            ("1. Scope", Some(("1.", "Scope"))),
            ("1) First", Some(("1)", "First"))),
            ("20 — R4 constants", Some(("20", "R4 constants"))),
            ("2.7 - Anchors", Some(("2.7", "Anchors"))),
            ("3.2 —", Some(("3.2", "—"))),
            ("2026-09-28 update", None),
            ("3D graphics", None),
            ("1.2.3", None),
            ("A Tale", None),
            ("A.I. and ML", None),
            ("1234567890 big", None),
            ("10x faster", None),
            ("1.1234567890 x", None),
            ("123456789 nine", Some(("123456789", "nine"))),
        ];
        for &(c, want) in v {
            for step in [0, 1, 2] {
                let (qual, name, _) = read(c, step);
                if let Some((q, n)) = want {
                    assert_eq!(
                        (qual.as_str(), name.as_str()),
                        (q, n),
                        "{c} in pieces of {step}"
                    );
                } else {
                    assert_eq!(
                        (qual.as_str(), name.as_str()),
                        ("", c),
                        "{c} in pieces of {step}"
                    );
                }
            }
        }
        assert_eq!(read("20 \u{2014} R4", 0).2, 3);
        assert_eq!(read("2.7 - x", 1).2, 1);
        assert_eq!(
            read("2.7 \u{2013}x", 1),
            ("2.7".into(), "\u{2013}x".into(), 0)
        );
    }

    /// The numbering, name and dash of content `c` read in pieces of `step` bytes (0: whole).
    fn read(c: &str, step: usize) -> (String, String, u8) {
        let mut h = HeadingText::default();
        if step == 0 {
            h.feed(c.as_bytes());
        } else {
            for p in c.as_bytes().chunks(step) {
                h.feed(p);
            }
        }
        let (qual, name, dash) = h.fields();
        (
            String::from_utf8_lossy(qual).into_owned(),
            String::from_utf8_lossy(&name).into_owned(),
            dash,
        )
    }

    #[test]
    fn heading_text_is_collapsed_and_bounded() {
        assert_eq!(
            read(" \t3.2.  Tabs\tand  spaces \t", 0).1,
            "Tabs and spaces"
        );
        // A numbering far longer than the cap still leaves a short name exact.
        let long_num = format!("{}1 Title", "1.".repeat(3000));
        let (qual, name, _) = read(&long_num, 7);
        assert_eq!((qual.len(), name.as_str()), (KEEP, "Title"));
        // A long paragraph keeps KEEP bytes of its name, however long it grows.
        let mut h = HeadingText::default();
        for _ in 0..10_000 {
            h.feed(b"word word word ");
        }
        assert_eq!(h.name.len(), KEEP);
        assert!(h.qual.capacity() <= KEEP && h.sep.len() <= 4);
    }

    #[test]
    fn runs_reach_the_name_within_two_bytes() {
        // From every state a run of `=`, `-` or `#` can meet, feeding KEEP + 2 of it equals feeding any more.
        for prefix in [
            "",
            "x",
            "3.2 ",
            "3.2 \u{2013}",
            "\u{a7}",
            "1",
            "A.",
            "1)",
            "§ ",
        ] {
            for &byte in b"=-#" {
                let mut short = HeadingText::default();
                short.feed(prefix.as_bytes());
                feed_run(&mut short, byte, 10_000);
                let mut long = HeadingText::default();
                long.feed(prefix.as_bytes());
                long.feed(&vec![byte; 10_000]);
                assert_eq!(short.fields(), long.fields(), "{prefix:?} {byte}");
                for n in [1, 2, 3, 64, 65] {
                    let mut a = HeadingText::default();
                    a.feed(prefix.as_bytes());
                    feed_run(&mut a, byte, n);
                    a.feed(b" end");
                    let mut b = HeadingText::default();
                    b.feed(prefix.as_bytes());
                    b.feed(&vec![byte; n]);
                    b.feed(b" end");
                    assert_eq!(a.fields(), b.fields(), "{prefix:?} {byte} {n}");
                }
            }
        }
    }

    #[test]
    fn long_headings_are_items_with_long_names() {
        let src = format!(
            "# {}\n\n## 1.1 {}\ntext\n\n",
            "x".repeat(5000),
            "y".repeat(5000)
        );
        let items = super::super::scan(Lang::Markdown, src.as_bytes()).expect("no failure");
        let v: Vec<_> = items.iter().collect();
        assert_eq!(v.len(), 2);
        assert!(v[0].name_long && v[0].name.len() == 64 && v[0].qual.is_empty());
        assert!(v[1].name_long && !v[1].qual_long && v[1].qual == "1.1");
        assert_eq!((v[1].start, v[1].end, v[1].parent), (3, 5, Some(0)));
        // A setext heading whose paragraph is 200,000 bytes keeps a prefix of its joined lines.
        let para = format!("{}\nend\n---\n", "z ".repeat(100_000));
        let items = super::super::scan(Lang::Markdown, para.as_bytes()).expect("no failure");
        let h = items.get(0).expect("a setext heading");
        assert!(h.name_long && h.name.starts_with("z z z"));
        assert!(items.heap_bytes() < 1024);
    }

    fn heads(src: &str) -> Vec<(u8, String, String, u64, u64)> {
        heads_in_pieces(src, 0)
    }

    /// The headings of `src` fed in pieces of `step` bytes (0: whole), as (level, name, qual, start, end).
    fn heads_in_pieces(src: &str, step: usize) -> Vec<(u8, String, String, u64, u64)> {
        let mut s = MarkdownScanner::new();
        if step == 0 {
            s.feed(src.as_bytes());
        } else {
            for p in src.as_bytes().chunks(step) {
                s.feed(p);
            }
        }
        s.finish()
            .iter()
            .map(|i| {
                (
                    i.skind,
                    i.name.to_owned(),
                    i.qual.to_owned(),
                    i.start,
                    i.end,
                )
            })
            .collect()
    }

    fn h(
        level: u8,
        name: &str,
        qual: &str,
        start: u64,
        end: u64,
    ) -> (u8, String, String, u64, u64) {
        (level, name.to_owned(), qual.to_owned(), start, end)
    }

    #[test]
    fn more_markdown_rules() {
        assert_eq!(
            heads("# Title #\n#  Spaced  \n"),
            [h(1, "Title", "", 1, 1), h(1, "Spaced", "", 2, 2)]
        );
        assert_eq!(heads("Title\n  ===  \n"), [h(1, "Title", "", 1, 2)]);
        assert!(heads("Title\n    ===\n").is_empty());
        assert_eq!(heads("Para\n-\n"), [h(2, "Para", "", 1, 2)]);
        assert_eq!(heads("Para\n1.\n---\n"), [h(2, "Para 1.", "", 1, 3)]);
        assert_eq!(heads("Para\n> q\n---\n"), Vec::new());
        assert_eq!(
            heads("```\n```js\n# in\n```\n# out\n"),
            [h(1, "out", "", 5, 5)]
        );
        assert_eq!(heads("````\n```\n# in\n````\n"), Vec::new());
        assert_eq!(
            heads("~~~ a`b\n# in\n~~~\n# out\n"),
            [h(1, "out", "", 4, 4)]
        );
        assert_eq!(heads("<!-- a --> # b\n# c\n"), [h(1, "c", "", 2, 2)]);
        assert_eq!(heads("--- \nt: x\n...\n# A\n"), [h(1, "A", "", 4, 4)]);
        assert_eq!(
            heads("----\nt: x\n---\n# A\n"),
            [h(2, "t: x", "", 2, 3), h(1, "A", "", 4, 4)]
        );
        assert_eq!(heads("---\n# Hidden\n---\n# A\n"), [h(1, "A", "", 4, 4)]);
        assert_eq!(
            heads("# A\n\n- item\n\n   # B\n"),
            [h(1, "A", "", 1, 4), h(1, "B", "", 5, 5)]
        );
        assert_eq!(
            heads("# 1 A\n## 1.1 B\n## 1.2 B\n"),
            [
                h(1, "A", "1", 1, 3),
                h(2, "B", "1.1", 2, 2),
                h(2, "B", "1.2", 3, 3)
            ]
        );
    }

    /// A text and its headings as (level, name, start, end).
    type Case = (&'static str, &'static [(u8, &'static str, u64, u64)]);

    /// Single lines and short texts for every class of [F21 §4.2], each also fed one byte at a time.
    #[test]
    fn line_classes() {
        let cases: &[Case] = &[
            // Indentation: an HT reaches column 4.
            ("\t# x\n", &[]),
            ("  \t# x\n", &[]),
            ("   # x\n", &[(1, "x", 1, 1)]),
            // ATX headings and their closing sequence.
            ("## Title ##\n", &[(2, "Title", 1, 1)]),
            ("# foo#\n", &[(1, "foo#", 1, 1)]),
            ("## ##\n#\n", &[]),
            ("####### x\n#x\n", &[]),
            ("# a # #\n", &[(1, "a #", 1, 1)]),
            ("# #foo\n", &[(1, "#foo", 1, 1)]),
            ("#\t#x\n", &[(1, "#x", 1, 1)]),
            ("# a ##x##\n", &[(1, "a ##x##", 1, 1)]),
            ("# a ## ##  \n", &[(1, "a ##", 1, 1)]),
            ("# a \\#\n", &[(1, "a \\#", 1, 1)]),
            ("###### six\n", &[(6, "six", 1, 1)]),
            // Setext underlines, held back while they may be one.
            ("Para\n=== \t\n", &[(1, "Para", 1, 2)]),
            ("Para\n- -\n", &[]),
            ("Para\n-- x\n", &[]),
            ("Para\n-- x\n===\n", &[(1, "Para -- x", 1, 3)]),
            ("Para\n==x\n---\n", &[(2, "Para ==x", 1, 3)]),
            ("Para\n  ==  =\n-\n", &[(2, "Para == =", 1, 3)]),
            // Fences: a backtick after the run makes no opener; a closing run is at least as long.
            ("``` x ` y\n# a\n", &[(1, "a", 2, 2)]),
            ("~~~ `x`\n# a\n~~~~~~ \n# b\n", &[(1, "b", 4, 4)]),
            ("~~~~~\n~~~~\n# a\n", &[]),
            ("```\n    ```\n# a\n", &[]),
            // Thematic breaks.
            ("Para\n- - -\n# a\n", &[(1, "a", 3, 3)]),
            ("Para\n***\n", &[]),
            ("Para\n--\n", &[(2, "Para", 1, 2)]),
            ("--\n===\n", &[(1, "--", 1, 2)]),
            ("* *\nfoo\n===\n", &[]),
            ("_ _ _\nfoo\n===\n", &[(1, "foo", 2, 3)]),
            // Container markers: which ones interrupt a paragraph.
            ("Para\n> x\n", &[]),
            ("Para\n- x\n===\n", &[]),
            ("Para\n1. x\n===\n", &[]),
            ("Para\n01) x\n===\n", &[]),
            ("Para\n2. x\n===\n", &[(1, "Para 2. x", 1, 3)]),
            ("Para\n10. x\n===\n", &[(1, "Para 10. x", 1, 3)]),
            (
                "Para\n1234567890. x\n===\n",
                &[(1, "Para 1234567890. x", 1, 3)],
            ),
            ("- x\n# a\n", &[(1, "a", 2, 2)]),
            ("-x\n===\n", &[(1, "-x", 1, 2)]),
            // HTML comments, whose `-->` is searched over the whole line and never across lines.
            ("<!-- a\n-\n->\n# b\n-->\n# c\n", &[(1, "c", 6, 6)]),
            ("<!---->\n# a\n", &[(1, "a", 2, 2)]),
            ("  <!-- x\n# a\n--->\n# b\n", &[(1, "b", 4, 4)]),
        ];
        for &(src, want) in cases {
            let want: Vec<_> = want
                .iter()
                .map(|&(lv, name, s, e)| h(lv, name, "", s, e))
                .collect();
            for step in [0, 1, 3] {
                assert_eq!(
                    heads_in_pieces(src, step),
                    want,
                    "{src:?} in pieces of {step}"
                );
            }
        }
    }

    #[test]
    fn long_runs_are_counted_not_held() {
        // A 10,000-byte run that ends in `x` joins the paragraph; one that ends in SP closes the fence.
        let src = format!("Para\n{}x\n=\n", "-".repeat(10_000));
        let v = heads_in_pieces(&src, 4096);
        assert_eq!(v.len(), 1);
        assert!(
            v[0].1.starts_with("Para ---") && v[0].1.len() == 64,
            "{v:?}"
        );
        let src = format!("````\n# in\n{}  \n# out\n", "`".repeat(5000));
        assert_eq!(heads_in_pieces(&src, 1000), [h(1, "out", "", 4, 4)]);
        let src = format!("# a {}\n", "#".repeat(9000));
        assert_eq!(heads_in_pieces(&src, 1000), [h(1, "a", "", 1, 1)]);
        let src = format!("# a {}b\n", "#".repeat(9000));
        let v = heads_in_pieces(&src, 1000);
        assert!(v[0].1.starts_with("a ###") && v[0].1.len() == 64, "{v:?}");
    }

    #[test]
    fn a_line_costs_no_memory_however_long() {
        // A 2 MiB paragraph line, a 2 MiB underline and a 2 MiB heading, fed in 64 KiB chunks: the state stays
        // within its capped heading texts.
        let mib = 1 << 20;
        let texts = [
            format!("{}\n", "word ".repeat(2 * mib / 5)),
            format!("Para\n{}\n", "=".repeat(2 * mib)),
            format!("## {}\n", "y".repeat(2 * mib)),
            format!("<!-- {}\n", "-".repeat(2 * mib)),
        ];
        for t in &texts {
            let mut s = MarkdownScanner::new();
            let mut most = 0;
            for chunk in t.as_bytes().chunks(64 * 1024) {
                s.feed(chunk);
                most = most.max(s.state_bytes());
            }
            assert!(most <= 8 * KEEP, "{most} state bytes");
            let items = s.finish();
            assert!(items.heap_bytes() <= 1024);
        }
    }
}
