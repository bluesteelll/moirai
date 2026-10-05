//! The TOML scanner ([F21 §5]): table and array-of-tables headers, key/value lines with dotted and quoted key paths
//! spelled as written (§5.2), and a value scan that follows multi-line strings and brackets across lines (§5.4).
//! Tables do not nest: every name path has one or two segments (§5.5).
//!
//! # Streaming
//!
//! The scanner reads each byte once and holds no line, so its memory does not grow with the length of a line
//! ([F21 §2.8]). A line is read by a state machine ([`St`]) whose one buffer is the key path's spelling, capped at
//! [`KEEP`] bytes; an open value keeps its bracket stack, at most [`TOML_MAX_DEPTH`] + 1 entries, and a few states
//! of its strings ([`Value`]). Nothing needs look-ahead: a line's class is decided by its first byte after SP and HT,
//! an item is recorded at the byte that completes it (a key's `=`, a header's `#` or the end of its line), and
//! quotes that may open a multi-line string are counted until a third quote or another byte decides.

use super::items::{Items, NO_PARENT};
use super::{Lang, SCOPE_MAX_BYTES, ScanFailed, TOML_MAX_DEPTH};

/// `skind` of a table header ([F08 §10.3.1]).
const SK_TABLE: u8 = 1;
/// `skind` of an array-of-tables header.
const SK_ARRAY_TABLE: u8 = 2;
/// `skind` of a key.
const SK_KEY: u8 = 3;

/// TOML whitespace ([F21 §5.1]).
fn is_sp_ht(b: u8) -> bool {
    b == b' ' || b == b'\t'
}

/// A byte of a bare key ([F21 §5.2]).
fn bare_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'-' || b == b'_'
}

/// The most bytes of a key path's spelling kept: every name that can be recordable has at most
/// [`SCOPE_MAX_BYTES`] bytes, and one byte more tells a longer one, since U+FFFD replacement never shortens bytes
/// ([F21 §2.3]).
const KEEP: usize = SCOPE_MAX_BYTES + 1;

/// Where the line being read stands ([F21 §5.1]–§5.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum St {
    /// The leading SP and HT of a line read while no value is open (§5.1).
    Lead,
    /// The rest of the line changes nothing: a comment, a line that is nothing, or what follows a value's end.
    Skip,
    /// After a line's first `[`: a second `[` makes an array-of-tables header (§5.3).
    Open,
    /// A key is expected, after any SP and HT: a header's first key, or one after a `.` (§5.2).
    KeyStart,
    /// In a bare key.
    Bare,
    /// In a basic quoted key; `esc` when a `\` takes the next byte.
    Basic { esc: bool },
    /// In a literal quoted key.
    Literal,
    /// After a key: SP and HT, then a `.` or what ends the key path (`=`, or a header's `]`).
    AfterKey,
    /// After the first `]` of an array-of-tables header's `]]`.
    Close,
    /// After a header's closing bracket: SP and HT, then the end of the line or a comment.
    AfterClose,
    /// After a key's `=`: SP and HT, then the first value token (§5.4).
    ValueStart,
    /// The first value token starts with a quote: the quote, and how many of it came (1 or 2).
    Quotes(u8, u8),
    /// Inside the open value.
    Value,
}

/// A single-line string between a value's brackets.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Str {
    #[default]
    None,
    /// `"…"`; `esc` when a `\` takes the next byte.
    Basic { esc: bool },
    /// `'…'`.
    Literal,
}

/// The open value of a key ([F21 §5.4]).
#[derive(Debug, Default)]
struct Value {
    /// The open brackets: `true` for `[`, `false` for `{`.
    stack: Vec<bool>,
    /// Open brackets per kind: `[`, `{`.
    count: [usize; 2],
    /// The quote byte of an open multi-line string; the length of the run of it being read; and, in a `"""`
    /// string, whether a `\` takes the next byte.
    ml: Option<u8>,
    run: usize,
    esc: bool,
    /// For the rest of the line: quotes read between brackets that may open a multi-line string (the quote, 1 or
    /// 2 of it), an open single-line string, and a comment.
    quotes: Option<(u8, u8)>,
    string: Str,
    comment: bool,
}

/// What a byte or a line's end did to the value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scan {
    /// The value ends on this line.
    Ended,
    /// It goes on.
    Open,
    /// The bracket stack exceeded [`TOML_MAX_DEPTH`].
    Failed,
}

impl Value {
    /// A new value: no bracket, no string (the stack keeps its capacity).
    fn reset(&mut self) {
        self.stack.clear();
        self.count = [0; 2];
        self.ml = None;
        self.run = 0;
        self.esc = false;
        self.quotes = None;
        self.string = Str::None;
        self.comment = false;
    }

    fn push(&mut self, square: bool) -> bool {
        self.stack.push(square);
        self.count[usize::from(!square)] += 1;
        self.stack.len() <= TOML_MAX_DEPTH
    }

    /// A closer: closes the topmost bracket of its kind and every bracket above it, or nothing.
    fn close(&mut self, square: bool) {
        if self.count[usize::from(!square)] == 0 {
            return;
        }
        while let Some(top) = self.stack.pop() {
            self.count[usize::from(!top)] -= 1;
            if top == square {
                break;
            }
        }
    }

    /// A multi-line string of quote `q` opens.
    fn open_ml(&mut self, q: u8) {
        self.ml = Some(q);
        self.run = 0;
        self.esc = false;
    }

    /// A byte of a line read inside the value: a multi-line string ends after the first maximal run of three or
    /// more of its quote that no `\` takes; between brackets, SP and HT are skipped, `#` ends the reading of the
    /// line, quotes open strings, and brackets are pushed and closed.
    fn byte(&mut self, b: u8) -> Scan {
        if self.comment {
            return Scan::Open;
        }
        if let Some(q) = self.ml {
            if self.esc {
                self.esc = false;
                return Scan::Open;
            }
            if b == q {
                self.run = self.run.saturating_add(1);
                return Scan::Open;
            }
            let closed = self.run >= 3;
            self.run = 0;
            if !closed {
                self.esc = q == b'"' && b == b'\\';
                return Scan::Open;
            }
            self.ml = None;
            if self.stack.is_empty() {
                return Scan::Ended;
            }
            // The string closed before `b`, which is read between brackets.
        }
        if let Some((q, k)) = self.quotes {
            if b == q {
                if k == 1 {
                    self.quotes = Some((q, 2));
                } else {
                    self.quotes = None;
                    self.open_ml(q);
                }
                return Scan::Open;
            }
            self.quotes = None;
            if k == 1 {
                // One quote opened a single-line string, and `b` is its first byte.
                self.string = if q == b'"' {
                    Str::Basic { esc: false }
                } else {
                    Str::Literal
                };
            }
            // Two quotes were an empty string, and `b` is read between brackets.
        }
        match self.string {
            Str::Basic { esc } => {
                self.string = if esc {
                    Str::Basic { esc: false }
                } else if b == b'\\' {
                    Str::Basic { esc: true }
                } else if b == b'"' {
                    Str::None
                } else {
                    Str::Basic { esc: false }
                };
                return Scan::Open;
            }
            Str::Literal => {
                if b == b'\'' {
                    self.string = Str::None;
                }
                return Scan::Open;
            }
            Str::None => {}
        }
        match b {
            b'#' => self.comment = true,
            b'"' | b'\'' => self.quotes = Some((b, 1)),
            b'[' | b'{' => {
                if !self.push(b == b'[') {
                    return Scan::Failed;
                }
            }
            b']' | b'}' => {
                self.close(b == b']');
                if self.stack.is_empty() {
                    return Scan::Ended;
                }
            }
            _ => {}
        }
        Scan::Open
    }

    /// The end of a line read inside the value: single-line strings, quotes and a comment end with it; a run of
    /// three or more quotes ends a multi-line string, and a `\` takes the `0A`.
    fn eol(&mut self) -> Scan {
        self.quotes = None;
        self.string = Str::None;
        self.comment = false;
        self.esc = false;
        if self.ml.is_some() {
            let closed = self.run >= 3;
            self.run = 0;
            if closed {
                self.ml = None;
                if self.stack.is_empty() {
                    return Scan::Ended;
                }
            }
        }
        Scan::Open
    }
}

/// The line scanner's state ([F21 §5.1]).
#[derive(Debug)]
struct Core {
    /// The number of lines begun.
    n: u64,
    st: St,
    /// The header being read: `Some(true)` after `[[`, `Some(false)` after `[`; `None` on a key/value line.
    header: Option<bool>,
    table: Option<usize>,
    /// The key whose value is open.
    key: Option<usize>,
    value: Value,
    items: Items,
    /// The key path's spelling so far, at most [`KEEP`] bytes.
    name: Vec<u8>,
    failed: bool,
}

impl Core {
    /// The next bytes of the line being read; none is `0A`.
    fn bytes(&mut self, seg: &[u8]) {
        for &b in seg {
            if self.st == St::Skip || self.failed {
                return;
            }
            self.byte(b);
        }
    }

    fn byte(&mut self, b: u8) {
        let ws = is_sp_ht(b);
        self.st = match self.st {
            St::Skip => St::Skip,
            St::Lead => match b {
                _ if ws => St::Lead,
                b'#' => St::Skip,
                b'[' => St::Open,
                _ => {
                    self.header = None;
                    self.key_start(b)
                }
            },
            St::Open => {
                self.header = Some(b == b'[');
                if b == b'[' || ws {
                    St::KeyStart
                } else {
                    self.key_start(b)
                }
            }
            St::KeyStart if ws => St::KeyStart,
            St::KeyStart => self.key_start(b),
            St::Bare if bare_byte(b) => {
                self.push_name(b);
                St::Bare
            }
            St::Bare | St::AfterKey => self.after_key(b),
            St::Basic { esc } => {
                self.push_name(b);
                if esc {
                    St::Basic { esc: false }
                } else if b == b'\\' {
                    St::Basic { esc: true }
                } else if b == b'"' {
                    St::AfterKey
                } else {
                    St::Basic { esc: false }
                }
            }
            St::Literal => {
                self.push_name(b);
                if b == b'\'' {
                    St::AfterKey
                } else {
                    St::Literal
                }
            }
            St::Close if b == b']' => St::AfterClose,
            St::AfterClose if ws => St::AfterClose,
            St::AfterClose if b == b'#' => {
                self.header_item();
                St::Skip
            }
            St::Close | St::AfterClose => St::Skip,
            St::ValueStart => match b {
                _ if ws => St::ValueStart,
                b'"' | b'\'' => St::Quotes(b, 1),
                b'[' | b'{' => {
                    if self.value.push(b == b'[') {
                        St::Value
                    } else {
                        self.failed = true;
                        St::Skip
                    }
                }
                _ => {
                    // A comment (the value is empty) or a scalar: the value ends on this line.
                    self.end_key();
                    St::Skip
                }
            },
            St::Quotes(q, k) => {
                if b != q {
                    // A single-line string, or the empty string: the value ends on this line.
                    self.end_key();
                    St::Skip
                } else if k == 1 {
                    St::Quotes(q, 2)
                } else {
                    self.value.open_ml(q);
                    St::Value
                }
            }
            St::Value => match self.value.byte(b) {
                Scan::Open => St::Value,
                Scan::Ended => {
                    self.end_key();
                    St::Skip
                }
                Scan::Failed => {
                    self.failed = true;
                    St::Skip
                }
            },
        };
    }

    /// The first byte of a key: a quote opens a quoted key, a bare-key byte a bare key; anything else is no key.
    fn key_start(&mut self, b: u8) -> St {
        match b {
            b'"' => {
                self.push_name(b);
                St::Basic { esc: false }
            }
            b'\'' => {
                self.push_name(b);
                St::Literal
            }
            _ if bare_byte(b) => {
                self.push_name(b);
                St::Bare
            }
            _ => St::Skip,
        }
    }

    /// A byte after a key: SP and HT are skipped; a `.` continues the key path; `=` ends a key/value line's key path
    /// and a `]` a header's. Anything else makes the line nothing — a `.` not followed by a key too, since the key
    /// path then ends before it and the byte after the path is the `.`.
    fn after_key(&mut self, b: u8) -> St {
        match (b, self.header) {
            (b' ' | b'\t', _) => St::AfterKey,
            (b'.', _) => {
                self.push_name(b'.');
                St::KeyStart
            }
            (b'=', None) => {
                self.key_item();
                St::ValueStart
            }
            (b']', Some(false)) => St::AfterClose,
            (b']', Some(true)) => St::Close,
            _ => St::Skip,
        }
    }

    fn push_name(&mut self, b: u8) {
        if self.name.len() < KEEP {
            self.name.push(b);
        }
    }

    /// A table or array-of-tables header is complete ([F21 §5.3]): an item, and the current table.
    fn header_item(&mut self) {
        let skind = if self.header == Some(true) {
            SK_ARRAY_TABLE
        } else {
            SK_TABLE
        };
        let slot = self.items.push(skind, self.n, NO_PARENT);
        self.items
            .set_names(slot, &String::from_utf8_lossy(&self.name), "");
        self.table = Some(slot);
    }

    /// A key's `=` ([F21 §5.4]): an item whose value starts.
    fn key_item(&mut self) {
        let slot = self
            .items
            .push(SK_KEY, self.n, self.table.unwrap_or(NO_PARENT));
        self.items
            .set_names(slot, &String::from_utf8_lossy(&self.name), "");
        self.value.reset();
        self.key = Some(slot);
    }

    /// The open value ends on the current line.
    fn end_key(&mut self) {
        if let Some(k) = self.key.take() {
            self.items.set_end(k, self.n);
            if let Some(t) = self.table {
                let end = self.items.end(t).max(self.n);
                self.items.set_end(t, end);
            }
        }
    }

    /// The end of the line being read.
    fn end_line(&mut self) {
        match self.st {
            St::AfterClose => self.header_item(),
            St::ValueStart | St::Quotes(..) => self.end_key(),
            St::Value => match self.value.eol() {
                Scan::Ended => self.end_key(),
                Scan::Open | Scan::Failed => {}
            },
            _ => {}
        }
        self.st = if self.key.is_some() {
            St::Value
        } else {
            St::Lead
        };
        self.name.clear();
    }

    fn finish(mut self) -> Result<Items, ScanFailed> {
        if self.failed {
            return Err(ScanFailed);
        }
        // A value still open at the end of the text ends on the last line.
        self.end_key();
        self.items.shrink();
        Ok(self.items)
    }
}

/// The TOML scanner over chunks of an anchor text.
#[derive(Debug)]
pub(crate) struct TomlScanner {
    core: Core,
    /// A line has begun and its `0A` has not come.
    in_line: bool,
}

impl TomlScanner {
    pub(crate) fn new() -> TomlScanner {
        TomlScanner {
            core: Core {
                n: 0,
                st: St::Lead,
                header: None,
                table: None,
                key: None,
                value: Value::default(),
                items: Items::new(Lang::Toml),
                name: Vec::new(),
                failed: false,
            },
            in_line: false,
        }
    }

    /// Whether the scan has failed: from the byte that opens a value's 1,025th bracket on.
    pub(crate) fn has_failed(&self) -> bool {
        self.core.failed
    }

    /// The lines of `lines(t)` ([F20 §2.5]): every `0A` ends one, and the text's end ends a last one that has a
    /// byte.
    pub(crate) fn feed(&mut self, chunk: &[u8]) {
        let mut rest = chunk;
        while !rest.is_empty() && !self.core.failed {
            if !self.in_line {
                self.core.n += 1;
                self.in_line = true;
            }
            if let Some(k) = rest.iter().position(|&b| b == b'\n') {
                self.core.bytes(&rest[..k]);
                if !self.core.failed {
                    self.core.end_line();
                }
                self.in_line = false;
                rest = &rest[k + 1..];
            } else {
                self.core.bytes(rest);
                break;
            }
        }
    }

    pub(crate) fn finish(mut self) -> Result<Items, ScanFailed> {
        if self.in_line && !self.core.failed {
            self.core.end_line();
        }
        self.core.finish()
    }

    /// The bytes the scanner's state holds besides its items.
    #[cfg(test)]
    fn state_bytes(&self) -> usize {
        self.core.name.capacity() + self.core.value.stack.capacity()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(src: &str) -> Vec<(u8, String, u64, u64, Option<usize>)> {
        keys_in_pieces(src, 0)
    }

    /// The items of `src` fed in pieces of `step` bytes (0: whole), as (skind, name, start, end, parent).
    fn keys_in_pieces(src: &str, step: usize) -> Vec<(u8, String, u64, u64, Option<usize>)> {
        let mut s = TomlScanner::new();
        if step == 0 {
            s.feed(src.as_bytes());
        } else {
            for p in src.as_bytes().chunks(step) {
                s.feed(p);
            }
        }
        s.finish()
            .expect("no failure")
            .iter()
            .map(|i| (i.skind, i.name.to_owned(), i.start, i.end, i.parent))
            .collect()
    }

    /// The names of the items of `src`, fed whole and one byte at a time.
    fn names(src: &str) -> Vec<String> {
        let whole = keys(src);
        assert_eq!(keys_in_pieces(src, 1), whole, "{src:?} byte by byte");
        whole.into_iter().map(|k| k.1).collect()
    }

    #[test]
    fn key_paths_of_f21_5_2() {
        assert_eq!(names("[ a . \"b c\" ]"), ["a.\"b c\""]);
        assert_eq!(names("a.b = 1"), ["a.b"]);
        assert_eq!(names("'lit' = 3"), ["'lit'"]);
        assert_eq!(names("\"a\\\"b\" = 1"), ["\"a\\\"b\""]);
        assert_eq!(names("a . b\t.c=1"), ["a.b.c"]);
        for nothing in [
            "a. = 1",
            "a.\n",
            "\"open = 1",
            "\"open\\",
            "'open = 1",
            "= 1",
            "ключ = 1",
            "a b = 1",
            "a\"b\" = 1",
            "\"a\"b = 1",
            "a ] = 1",
            "a",
        ] {
            assert!(names(nothing).is_empty(), "{nothing:?}");
        }
    }

    #[test]
    fn headers_of_f21_5_3() {
        assert_eq!(
            names("[ workspace . dependencies ]"),
            ["workspace.dependencies"]
        );
        assert_eq!(names("[[bin]] # c"), ["bin"]);
        assert_eq!(names("[a] # c ] ["), ["a"]);
        assert_eq!(names("\t[[ a.'b' ]]\t"), ["a.'b'"]);
        for nothing in [
            "[ [a] ]", "[a]]", "[b] junk", "[a", "[[c] ]", "[[c]", "[]", "[ ]", "[[ ]]", "[a = 1]",
            "[[[a]]]", "[a.]",
        ] {
            assert!(names(nothing).is_empty(), "{nothing:?}");
        }
    }

    #[test]
    fn more_toml_rules() {
        assert_eq!(
            keys("[a.b]\nx=1 # c\n"),
            [
                (1, "a.b".into(), 1, 2, None),
                (3, "x".into(), 2, 2, Some(0))
            ]
        );
        assert_eq!(
            keys("[[a.b]]\n\"a\" = 1\n"),
            [
                (2, "a.b".into(), 1, 2, None),
                (3, "\"a\"".into(), 2, 2, Some(0))
            ]
        );
        assert_eq!(
            keys("x = { a = [1,\n2] }\ny = 1\n"),
            [(3, "x".into(), 1, 2, None), (3, "y".into(), 3, 3, None)]
        );
        assert_eq!(
            keys("x = [ \"]\" ]\ny = [ ']' ]\nz = [ \"\\\"]\" ]\n").len(),
            3
        );
        assert_eq!(
            keys("x = ['''\n]\n''']\ny = 1\n"),
            [(3, "x".into(), 1, 3, None), (3, "y".into(), 4, 4, None)]
        );
        assert_eq!(keys("x = [ # ]\n]\n"), [(3, "x".into(), 1, 2, None)]);
        assert_eq!(keys("x = [ {\n] y = 1\n"), [(3, "x".into(), 1, 2, None)]);
        assert_eq!(keys("x = \"\"\"a\"\"\" trailing\ny = 1\n").len(), 2);
        assert_eq!(
            keys("  indented = 1\n\t[t]\n"),
            [
                (3, "indented".into(), 1, 1, None),
                (1, "t".into(), 2, 2, None)
            ]
        );
    }

    #[test]
    fn value_forms_streamed() {
        // Each text in one piece and one byte at a time: (source, the end line of key `x`).
        let cases: &[(&str, u64)] = &[
            ("x =\ny = 1", 1),
            ("x = # c\n", 1),
            ("x = \"a\nb", 1),
            ("x = \"\"\ny = 1", 1),
            ("x = ''\n", 1),
            ("x = '''a\nb'''\n", 2),
            ("x = \"\"\"a\\\n\"\"\"\n", 2),
            ("x = \"\"\"a\\\"\"\"\nb\"\"\"\n", 2),
            ("x = \"\"\"\"\"\"\n", 1),
            ("x = \"\"\"a\"\"\"\"\"\n", 1),
            ("x = [\"\"\"\n]\"\"\"]\n", 2),
            ("x = [\"\"\"\"\"\" ]\n", 1),
            ("x = [ \"\" ]\n", 1),
            ("x = [ '' ]\n", 1),
            ("x = [ \"a\n]\n", 2),
            ("x = [ 'a\n]\n", 2),
            ("x = [ \"\\\n]\n", 2),
            ("x = [ { ] }\n]\n", 1),
            ("x = [ } ]\n", 1),
            ("x = [\n# ]\n]\n", 3),
            ("x = [\n[ ] ]\n", 2),
            ("x = '''\n''''\n", 2),
            ("x = 1 [\n", 1),
            ("x = [\n", 1),
        ];
        for &(src, end) in cases {
            for step in [0, 1, 2] {
                let k = keys_in_pieces(src, step);
                assert_eq!(k[0].3, end, "{src:?} in pieces of {step}: {k:?}");
            }
        }
    }

    #[test]
    fn long_key_paths_keep_a_prefix() {
        let long = "k".repeat(5000);
        let src = format!("[{long}]\n{long}.x = 1\nshort = 2\n\"{long}\" = 3\n");
        let items = super::super::scan(Lang::Toml, src.as_bytes()).expect("no failure");
        let v: Vec<_> = items.iter().collect();
        assert_eq!(v.len(), 4);
        for i in [0, 1, 3] {
            assert!(v[i].name_long && v[i].name.len() == 64, "{:?}", v[i]);
        }
        assert_eq!(
            (v[2].name, v[2].name_long, v[2].parent),
            ("short", false, Some(0))
        );
        assert_eq!((v[1].start, v[1].end, v[1].parent), (2, 2, Some(0)));
        // 4,096 bytes are kept whole.
        let edge = format!("{} = 1\n", "e".repeat(4096));
        let items = super::super::scan(Lang::Toml, edge.as_bytes()).expect("no failure");
        assert!(
            items
                .get(0)
                .is_some_and(|i| !i.name_long && i.name.len() == 4096)
        );
    }

    #[test]
    fn the_depth_limit_fails_at_its_byte() {
        let mut s = TomlScanner::new();
        s.feed(format!("x = {}", "[".repeat(1024)).as_bytes());
        assert!(!s.has_failed());
        s.feed(b"[");
        assert!(s.has_failed());
        assert_eq!(s.finish().map(|_| ()), Err(ScanFailed));
    }

    #[test]
    fn a_line_costs_no_memory_however_long() {
        // A 2 MiB key, value, comment and multi-line string, fed in 64 KiB chunks: the state stays within the
        // capped key path and the bracket stack.
        let mib = 1 << 20;
        let texts = [
            format!("{} = 1\n", "k".repeat(2 * mib)),
            format!("x = [{}]\n", "1, ".repeat(2 * mib / 3)),
            format!("# {}\n", "c".repeat(2 * mib)),
            format!("x = \"\"\"{}\"\"\"\n", "s".repeat(2 * mib)),
            format!("[{}]\n", "t.".repeat(mib)),
        ];
        for t in &texts {
            let mut s = TomlScanner::new();
            let mut most = 0;
            for chunk in t.as_bytes().chunks(64 * 1024) {
                s.feed(chunk);
                most = most.max(s.state_bytes());
            }
            assert!(most <= 4 * KEEP, "{most} state bytes");
            let items = s.finish().expect("no failure");
            assert!(items.heap_bytes() <= 1024);
        }
    }
}
