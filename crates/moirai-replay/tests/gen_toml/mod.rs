//! Generated TOML documents with their tables and keys by construction ([F21 §5]).
//!
//! A document is keys before any header, then table and array-of-tables headers (`[a.b]`, `[ a . "b c" ]`,
//! `[[bin]]`, trailing comments), each with keys. Keys are bare, dotted, basic-quoted and literal-quoted, spaced or
//! not around `=` and around their dots. Values are scalars, single-line strings holding `]`, `#` and quotes,
//! multi-line basic and literal strings holding header-like and key-like lines, quote runs, escapes and line
//! continuations, and arrays and inline tables on one line or across lines (TOML 1.1 inline tables included) with
//! comments, nested brackets and strings inside. Between them stand comment lines, blank lines and lines that are
//! nothing (`junk line`, `[bad]] # x`, `[ [a] ]`), which change no state. The expected items follow [F21 §5.2]
//! (spellings), §5.3–§5.4 (items and value ends) and §5.5 (parents and table ends).

use moirai_replay::scandiff::Row;
use proptest::prelude::*;

const BARE: &[&str] = &["name", "version", "a", "b-c", "d_e", "1x", "key2"];
const BASIC: &[&str] = &["\"quoted key\"", "\"a.b\"", "\"é\"", "\"q\\\"k\""];
const LITERAL: &[&str] = &["'lit'", "'x y'"];

/// A key: bare, basic-quoted or literal-quoted, by pool index.
#[derive(Clone, Copy, Debug)]
pub enum Key {
    Bare(u8),
    Basic(u8),
    Literal(u8),
}

impl Key {
    fn text(self) -> &'static str {
        match self {
            Key::Bare(i) => BARE[usize::from(i) % BARE.len()],
            Key::Basic(i) => BASIC[usize::from(i) % BASIC.len()],
            Key::Literal(i) => LITERAL[usize::from(i) % LITERAL.len()],
        }
    }
}

/// A value.
#[derive(Clone, Debug)]
pub enum Value {
    Scalar(u8),
    Str(u8),
    /// A `"""` string: its lines and its closing variant.
    MultiBasic(Vec<u8>, u8),
    /// A `'''` string.
    MultiLiteral(Vec<u8>, bool),
    /// An array: items and whether it spans lines.
    Array(Vec<Value>, bool),
    /// An inline table: its entries and whether it spans lines.
    Inline(Vec<(Key, Value)>, bool),
}

/// A line of a table's body.
#[derive(Clone, Debug)]
pub enum Line {
    /// A key path, its value, the spacing style and a trailing comment.
    Key(Vec<Key>, Value, u8, bool),
    Comment,
    Blank,
    Nothing(u8),
}

/// A table header.
#[derive(Clone, Debug)]
pub struct Header {
    pub array: bool,
    pub path: Vec<Key>,
    pub spaced: bool,
    pub comment: bool,
}

/// A document.
#[derive(Clone, Debug)]
pub struct Doc {
    pub root: Vec<Line>,
    pub tables: Vec<(Header, Vec<Line>)>,
    pub final_newline: bool,
}

fn key() -> impl Strategy<Value = Key> {
    prop_oneof![
        5 => (0..BARE.len() as u8).prop_map(Key::Bare),
        1 => (0..BASIC.len() as u8).prop_map(Key::Basic),
        1 => (0..LITERAL.len() as u8).prop_map(Key::Literal),
    ]
}

fn value() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        4 => (0u8..6).prop_map(Value::Scalar),
        3 => (0u8..5).prop_map(Value::Str),
        1 => (prop::collection::vec(0u8..8, 0..4), 0u8..3).prop_map(|(l, c)| Value::MultiBasic(l, c)),
        1 => (prop::collection::vec(0u8..8, 0..4), any::<bool>()).prop_map(|(l, c)| Value::MultiLiteral(l, c)),
    ];
    leaf.prop_recursive(3, 16, 4, |inner| {
        prop_oneof![
            (prop::collection::vec(inner.clone(), 0..4), any::<bool>())
                .prop_map(|(v, m)| Value::Array(v, m)),
            (prop::collection::vec((key(), inner), 0..3), any::<bool>())
                .prop_map(|(v, m)| Value::Inline(v, m)),
        ]
    })
}

fn line() -> impl Strategy<Value = Line> {
    prop_oneof![
        8 => (prop::collection::vec(key(), 1..3), value(), 0u8..4, any::<bool>()).prop_map(|(k, v, s, c)| Line::Key(k, v, s, c)),
        1 => Just(Line::Comment),
        1 => Just(Line::Blank),
        1 => (0u8..4).prop_map(Line::Nothing),
    ]
}

/// A document.
pub fn document() -> impl Strategy<Value = Doc> {
    let header = (
        any::<bool>(),
        prop::collection::vec(key(), 1..3),
        any::<bool>(),
        any::<bool>(),
    )
        .prop_map(|(array, path, spaced, comment)| Header {
            array,
            path,
            spaced,
            comment,
        });
    (
        prop::collection::vec(line(), 0..4),
        prop::collection::vec((header, prop::collection::vec(line(), 0..5)), 0..5),
        any::<bool>(),
    )
        .prop_map(|(root, tables, final_newline)| Doc {
            root,
            tables,
            final_newline,
        })
}

/// Lines of multi-line strings: header-like, key-like, quote runs, escapes and a line continuation.
const MULTI_BASIC: &[&str] = &[
    "[not.a.table]",
    "key = 1",
    "\"\" two quotes",
    "esc \\\"\"\" quote",
    "back \\\\ slash",
    "continued \\",
    "[[also.not]]",
    "# not a comment",
];
const MULTI_LITERAL: &[&str] = &[
    "[not.a.table]",
    "key = 'x'",
    "'' two quotes",
    "\\ no escapes \\",
    "[[also.not]]",
    "# not a comment",
    "] } unbalanced",
    "{ [ open",
];

/// The renderer: the lines so far and the expected rows.
struct Out {
    lines: Vec<String>,
    cur: String,
    rows: Vec<Row>,
}

impl Out {
    fn line_no(&self) -> u64 {
        self.lines.len() as u64 + 1
    }

    fn push(&mut self, s: &str) {
        self.cur.push_str(s);
    }

    fn newline(&mut self) {
        self.lines.push(std::mem::take(&mut self.cur));
    }

    /// Writes a value; it may break lines, and it ends on the current line.
    fn value(&mut self, v: &Value, depth: usize) {
        match v {
            Value::Scalar(k) => self.push(
                ["1", "true", "3.14", "-0x1F", "1979-05-27T07:32:00Z", "inf"][usize::from(*k)],
            ),
            Value::Str(k) => {
                self.push(["\"a]#b\"", "'lit # ]'", "\"q\\\"[x]\"", "\"\"", "'{'"][usize::from(*k)])
            }
            Value::MultiBasic(lines, close) => {
                self.push("\"\"\"");
                for &l in lines {
                    self.newline();
                    self.push(MULTI_BASIC[usize::from(l)]);
                }
                if lines
                    .last()
                    .is_some_and(|&l| MULTI_BASIC[usize::from(l)].ends_with('\\'))
                {
                    // After a line continuation, the next line is still inside the string.
                    self.newline();
                    self.push("tail");
                }
                self.push(["\"\"\"", "a\"\"\"\"", "b\"\"\"\"\""][usize::from(*close)]);
            }
            Value::MultiLiteral(lines, quote_end) => {
                self.push("'''");
                for &l in lines {
                    self.newline();
                    self.push(MULTI_LITERAL[usize::from(l)]);
                }
                self.push(if *quote_end { "x''''" } else { "'''" });
            }
            Value::Array(items, multi) => {
                self.push("[");
                for (i, it) in items.iter().enumerate() {
                    if *multi {
                        self.newline();
                        self.push(&"  ".repeat(depth + 1));
                    } else if i > 0 {
                        self.push(" ");
                    }
                    self.value(it, depth + 1);
                    self.push(",");
                    if *multi && i % 2 == 0 {
                        self.push(" # a comment with ] and \"");
                    }
                }
                if *multi {
                    self.newline();
                    self.push(&"  ".repeat(depth));
                }
                self.push("]");
            }
            Value::Inline(entries, multi) => {
                self.push("{");
                for (i, (k, it)) in entries.iter().enumerate() {
                    if *multi {
                        self.newline();
                        self.push(&"  ".repeat(depth + 1));
                    } else {
                        self.push(if i > 0 { ", " } else { " " });
                    }
                    self.push(k.text());
                    self.push(" = ");
                    self.value(it, depth + 1);
                    if *multi {
                        self.push(",");
                    }
                }
                if *multi {
                    self.newline();
                    self.push(&"  ".repeat(depth));
                } else if !entries.is_empty() {
                    self.push(" ");
                }
                self.push("}");
            }
        }
    }

    fn body(&mut self, lines: &[Line], parent: Option<usize>) {
        for l in lines {
            match l {
                Line::Blank => {}
                Line::Comment => self.push("# x = 1 [t]"),
                Line::Nothing(k) => {
                    self.push(
                        ["junk line", "[bad]] # x", "[ [a] ]", "  [b] junk"][usize::from(*k)],
                    );
                }
                Line::Key(path, v, spacing, comment) => {
                    let start = self.line_no();
                    let dot = if spacing & 1 != 0 { " . " } else { "." };
                    let texts: Vec<&str> = path.iter().map(|k| k.text()).collect();
                    let name = texts.join(".");
                    if spacing & 2 != 0 {
                        self.push("  ");
                    }
                    self.push(&texts.join(dot));
                    self.push(if *spacing == 3 { "=" } else { " = " });
                    self.value(v, 0);
                    if *comment {
                        self.push(" # trailing [x] = \"");
                    }
                    let end = self.line_no();
                    self.rows.push(Row::new(3, &name, "", start, end, parent));
                    if let Some(p) = parent {
                        self.rows[p].end = self.rows[p].end.max(end);
                    }
                }
            }
            self.newline();
        }
    }
}

/// Renders a document and its items by construction.
pub fn render(doc: &Doc) -> (String, Vec<Row>) {
    let mut out = Out {
        lines: Vec::new(),
        cur: String::new(),
        rows: Vec::new(),
    };
    out.body(&doc.root, None);
    for (h, lines) in &doc.tables {
        let (open, close) = if h.array { ("[[", "]]") } else { ("[", "]") };
        let texts: Vec<&str> = h.path.iter().map(|k| k.text()).collect();
        let start = out.line_no();
        out.push(open);
        if h.spaced {
            out.push(" ");
            out.push(&texts.join(" . "));
            out.push(" ");
        } else {
            out.push(&texts.join("."));
        }
        out.push(close);
        if h.comment {
            out.push(" # header comment ]");
        }
        out.newline();
        out.rows.push(Row::new(
            if h.array { 2 } else { 1 },
            &texts.join("."),
            "",
            start,
            start,
            None,
        ));
        let index = out.rows.len() - 1;
        out.body(lines, Some(index));
    }
    let mut text = out.lines.join("\n");
    if doc.final_newline {
        if !out.lines.is_empty() {
            text.push('\n');
        }
    } else if text.ends_with('\n') {
        text.pop();
    }
    (text, out.rows)
}
