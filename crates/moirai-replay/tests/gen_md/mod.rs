//! Generated Markdown documents with their headings by construction ([F21 §4]).
//!
//! A document is an optional front matter and a list of blocks separated by blank lines. Headings are ATX (levels
//! 1–6, up to 3 columns of indentation, SP or HT runs, an optional closing sequence) or setext (a paragraph of one to
//! three lines and a `=` or `-` underline). A heading's text is an optional numbering ([F21 §4.7]: digits, dotted
//! components, a letter component, `§`, a final `.` or `)`, and a dash separator) and words, some of which look like a
//! numbering and are not (`3D`, `2026-09-28`, `A.I.`, `A`). The other blocks hold heading-like lines that are not
//! headings: fenced code (backtick and tilde fences, closers longer than their openers), HTML comment blocks,
//! indented code, block-quote and list-item first lines with their continuation lines, thematic breaks, plain
//! paragraphs, and front matter. The expected items follow [F21 §4.7] (name, numbering) and §4.8 (levels, parents,
//! sections) from the construction.

use moirai_replay::scandiff::Row;
use proptest::prelude::*;

/// Words of heading text; none starts a numbering in any context.
const WORDS: &[&str] = &[
    "Design",
    "Storage",
    "Input/output",
    "größe",
    "`code`",
    "**bold**",
    "R4",
    "x",
    "Recovery",
    "and",
    "ML",
];

/// First words that look like a numbering and are none ([F21 §4.7] examples).
const TRICKY: &[&str] = &["3D", "2026-09-28", "A.I.", "A"];

/// Lines that look like Markdown structure, for code, comments and front matter.
const FAKE: &[&str] = &[
    "# fake",
    "## Build ##",
    "Fake",
    "===",
    "---",
    "- # item",
    "### x",
    "Title",
];

/// A heading's text.
#[derive(Clone, Debug)]
pub struct Heading {
    /// Numbering pattern and its numbers.
    pub numbering: Option<(u8, u16, u16)>,
    /// A dash separator after the numbering: `-`, `–` or `—`.
    pub dash: Option<u8>,
    /// A tricky first word.
    pub tricky: Option<u8>,
    /// The words (at least one).
    pub words: Vec<u8>,
    /// Whitespace runs between the parts.
    pub gaps: Vec<u8>,
}

/// A block of the document.
#[derive(Clone, Debug)]
pub enum Block {
    /// Level, indentation, text, closing `#` count, trailing whitespace.
    Atx(u8, u8, Heading, u8, bool),
    /// Level (1 or 2), indentation, text, lines to split over, underline length, trailing whitespace.
    Setext(u8, u8, Heading, u8, u8, bool),
    /// A plain paragraph of one to three lines.
    Para(u8),
    /// A fence: tilde or backtick, length, info string, fake lines, how much longer the closer is.
    Fence(bool, u8, bool, Vec<u8>, u8),
    /// An HTML comment block with fake lines; `true`: on one line.
    Comment(Vec<u8>, bool),
    /// Indented code lines.
    Indented(Vec<u8>),
    /// A container first line (kind) and continuation lines.
    Container(u8, u8),
    /// A thematic break.
    Break(u8),
}

/// A whole document.
#[derive(Clone, Debug)]
pub struct Doc {
    /// Front matter: `None`, or `Some(true)` closed by `...`, `Some(false)` by `---`.
    pub front: Option<bool>,
    /// The blocks.
    pub blocks: Vec<Block>,
    /// Blank-line styles between blocks.
    pub blanks: Vec<u8>,
    /// Whether the text ends with a line break.
    pub final_newline: bool,
}

fn heading(setext: bool) -> impl Strategy<Value = Heading> {
    let patterns: Vec<u8> = if setext {
        // `1.` and `1)` at the start of a paragraph are ordered-list markers, not text.
        vec![0, 1, 2, 5, 6, 7, 8, 9, 10, 11]
    } else {
        (0..12).collect()
    };
    (
        prop::option::weighted(0.6, (prop::sample::select(patterns), 0u16..1000, 0u16..100)),
        prop::option::weighted(0.3, 0u8..3),
        prop::option::weighted(0.2, 0..TRICKY.len() as u8),
        prop::collection::vec(0..WORDS.len() as u8, 1..4),
        prop::collection::vec(0u8..8, 8),
    )
        .prop_map(|(numbering, dash, tricky, words, gaps)| Heading {
            dash: numbering.and(dash),
            tricky: if numbering.is_some() { None } else { tricky },
            numbering,
            words,
            gaps,
        })
}

fn block() -> impl Strategy<Value = Block> {
    prop_oneof![
        5 => (1u8..7, 0u8..4, heading(false), 0u8..4, any::<bool>()).prop_map(|(l, i, h, c, t)| Block::Atx(l, i, h, c, t)),
        3 => (1u8..3, 0u8..4, heading(true), 1u8..4, 1u8..6, any::<bool>())
            .prop_map(|(l, i, h, s, u, t)| Block::Setext(l, i, h, s, u, t)),
        2 => (0u8..3).prop_map(Block::Para),
        2 => (any::<bool>(), 3u8..6, any::<bool>(), prop::collection::vec(0..FAKE.len() as u8, 0..4), 0u8..2)
            .prop_map(|(t, n, i, f, c)| Block::Fence(t, n, i, f, c)),
        1 => (prop::collection::vec(0..FAKE.len() as u8, 0..3), any::<bool>()).prop_map(|(f, one)| Block::Comment(f, one)),
        1 => prop::collection::vec(0..FAKE.len() as u8, 1..3).prop_map(Block::Indented),
        2 => (0u8..6, 0u8..3).prop_map(|(k, c)| Block::Container(k, c)),
        1 => (0u8..4).prop_map(Block::Break),
    ]
}

/// A document.
pub fn document() -> impl Strategy<Value = Doc> {
    (
        prop::option::weighted(0.15, any::<bool>()),
        prop::collection::vec(block(), 0..12),
        prop::collection::vec(0u8..4, 12),
        any::<bool>(),
    )
        .prop_map(|(front, blocks, blanks, final_newline)| Doc {
            front,
            blocks,
            blanks,
            final_newline,
        })
}

fn gap(g: u8) -> &'static str {
    [" ", "  ", "\t", " \t", "   ", " ", " ", "\t\t"][usize::from(g % 8)]
}

/// The numbering text of a pattern.
fn numbering_text(p: u8, a: u16, b: u16) -> String {
    let c = (b % 7) + 1;
    match p {
        0 => format!("{a}"),
        1 => format!("{a}.{b}"),
        2 => format!("{a}.{b}.{c}"),
        3 => format!("{a}."),
        4 => format!("{a})"),
        5 => format!("§{a}"),
        6 => format!("§ {a}"),
        7 => format!("§{a}.{b}"),
        8 => format!("A.{c}"),
        9 => "B.".to_string(),
        10 => "C)".to_string(),
        _ => format!("A.{c}.{b}"),
    }
}

/// The parts of a heading's text in order, and its expected name and qualifier.
fn parts(h: &Heading) -> (Vec<String>, String, String) {
    let mut parts = Vec::new();
    let mut qual = String::new();
    if let Some((p, a, b)) = h.numbering {
        qual = numbering_text(p, a, b);
        parts.push(qual.clone());
        if let Some(d) = h.dash {
            parts.push(["-", "–", "—"][usize::from(d)].to_string());
        }
    }
    let mut words: Vec<String> = Vec::new();
    if let Some(t) = h.tricky {
        words.push(TRICKY[usize::from(t)].to_string());
    }
    words.extend(h.words.iter().map(|&w| WORDS[usize::from(w)].to_string()));
    let name = words.join(" ");
    parts.extend(words);
    (parts, name, qual)
}

struct Lines {
    lines: Vec<String>,
    headings: Vec<(u8, String, String, u64)>,
}

impl Lines {
    fn push(&mut self, s: impl Into<String>) {
        self.lines.push(s.into());
    }

    /// The number of the next line.
    fn next(&self) -> u64 {
        self.lines.len() as u64 + 1
    }
}

/// Renders a document and its headings by construction.
pub fn render(doc: &Doc) -> (String, Vec<Row>) {
    let mut out = Lines {
        lines: Vec::new(),
        headings: Vec::new(),
    };
    if let Some(dots) = doc.front {
        out.push("---");
        out.push("title: x");
        out.push("# not a heading");
        out.push("Setext?");
        out.push("===");
        out.push(if dots { "..." } else { "--- " });
    }
    for (i, b) in doc.blocks.iter().enumerate() {
        if i > 0 || doc.front.is_some() {
            out.push(["", "  ", "\t", ""][usize::from(doc.blanks[i % doc.blanks.len()])]);
            if doc.blanks[(i + 1) % doc.blanks.len()] == 3 {
                out.push("");
            }
        }
        block_lines(&mut out, b);
    }
    let mut text = out.lines.join("\n");
    if doc.final_newline && !out.lines.is_empty() {
        text.push('\n');
    }
    // `lines(t)` drops a last empty piece ([F20 §2.5]): an empty last line counts only when a line break follows it.
    let mut last = out.lines.len() as u64;
    if !doc.final_newline && out.lines.last().is_some_and(String::is_empty) {
        last -= 1;
    }
    (text, sections(&out.headings, last))
}

fn block_lines(out: &mut Lines, b: &Block) {
    match b {
        Block::Atx(level, indent, h, closing, trail) => {
            let (parts, name, qual) = parts(h);
            let mut l = " ".repeat(usize::from(*indent));
            l.push_str(&"#".repeat(usize::from(*level)));
            for (k, p) in parts.iter().enumerate() {
                l.push_str(gap(h.gaps[k % h.gaps.len()]));
                l.push_str(p);
            }
            if *closing > 0 {
                l.push(' ');
                l.push_str(&"#".repeat(usize::from(*closing)));
            }
            if *trail {
                l.push_str(" \t");
            }
            out.headings.push((*level, name, qual, out.next()));
            out.push(l);
        }
        Block::Setext(level, indent, h, split, underline, trail) => {
            let (mut parts, name, qual) = parts(h);
            // The numbering and its dash stay with the first word on the first line: a continuation line that starts
            // with `- ` would be a list item that interrupts the paragraph ([F21 §4.6]).
            if h.numbering.is_some() {
                let lead = 1 + usize::from(h.dash.is_some());
                let head: Vec<String> = parts.drain(..=lead).collect();
                parts.insert(0, head.join(" "));
            }
            let start = out.next();
            let per = parts.len().div_ceil(usize::from(*split)).max(1);
            for (k, chunk) in parts.chunks(per).enumerate() {
                let mut l = if k == 0 {
                    " ".repeat(usize::from(*indent))
                } else {
                    " ".repeat(usize::from(*indent) + k)
                };
                for (j, p) in chunk.iter().enumerate() {
                    if j > 0 {
                        l.push_str(gap(h.gaps[(k + j) % h.gaps.len()]));
                    }
                    l.push_str(p);
                }
                if *trail {
                    l.push(' ');
                }
                out.push(l);
            }
            let ch = if *level == 1 { "=" } else { "-" };
            let mut u = " ".repeat(usize::from(*indent));
            u.push_str(&ch.repeat(usize::from(*underline)));
            if *trail {
                u.push_str("  ");
            }
            out.headings.push((*level, name, qual, start));
            out.push(u);
        }
        Block::Para(k) => {
            let paragraphs: [&[&str]; 3] = [
                &["Plain text with # inside."],
                &["Two lines", "of text; the second is a continuation"],
                &["A paragraph", "  indented continuation", "and one more"],
            ];
            for l in paragraphs[usize::from(*k)] {
                out.push(*l);
            }
        }
        Block::Fence(tilde, n, info, fake, extra) => {
            let ch = if *tilde { "~" } else { "`" };
            let mut open = ch.repeat(usize::from(*n));
            if *info {
                open.push_str(if *tilde { "markdown ```" } else { "rust" });
            }
            out.push(open);
            for &f in fake {
                out.push(FAKE[usize::from(f)]);
            }
            // A fence of the other kind, and a shorter one of this kind, do not close it.
            out.push(if *tilde { "```" } else { "~~~" });
            out.push(ch.repeat(usize::from(*n) - 1));
            let mut close = ch.repeat(usize::from(*n + extra));
            close.push(' ');
            out.push(close);
        }
        Block::Comment(fake, one) => {
            if *one {
                out.push("<!-- # hidden -->");
            } else {
                out.push("<!-- start");
                for &f in fake {
                    out.push(FAKE[usize::from(f)]);
                }
                out.push("end -->");
            }
        }
        Block::Indented(fake) => {
            for (k, &f) in fake.iter().enumerate() {
                let indent = if k % 2 == 0 { "    " } else { "\t" };
                out.push(format!("{indent}{}", FAKE[usize::from(f)]));
            }
        }
        Block::Container(kind, cont) => {
            out.push(
                [
                    "> # Quote",
                    "- # Item",
                    "* item",
                    "+ Setext?",
                    "1. # Step",
                    "2) two",
                ][usize::from(*kind)],
            );
            for k in 0..*cont {
                out.push(["continued text", "  more text", "Underline?"][usize::from(k)]);
            }
        }
        Block::Break(k) => out.push(["***", "- - -", "___", " * * *"][usize::from(*k)]),
    }
}

/// The items of the headings (level, name, numbering, start line): parents by level, sections to the next heading of
/// the same or a lower level, or to the last line ([F21 §4.8]).
fn sections(headings: &[(u8, String, String, u64)], last: u64) -> Vec<Row> {
    let mut rows: Vec<Row> = Vec::new();
    for (i, (level, name, qual, start)) in headings.iter().enumerate() {
        let parent = (0..i).rev().find(|&j| headings[j].0 < *level);
        let end = headings[i + 1..]
            .iter()
            .find(|h| h.0 <= *level)
            .map_or(last, |h| h.3 - 1);
        rows.push(Row::new(*level, name, qual, *start, end, parent));
    }
    rows
}
