//! The scope scanners of resolver version 1 (\[F21\]): the Rust, Markdown and TOML scanners as total, streaming
//! functions over any anchor text, and the item model they share — name paths, recordable scope values, the scope a
//! capture records, how a scope resolves, item headers of the same kind — with the `symbol` and `heading` authoring
//! forms ([F21 §6]).
//!
//! # Input and output
//!
//! A scanner reads the anchor text t = `atext(b)` of a text content ([F21 §1.3], [F20 §2.5]) from its start, in
//! chunks the caller supplies ([`Scanner::feed`]); the crate opens no file (PLAN §2.1, GT20 (d)). Chunk boundaries
//! never change a result: every chunking of t gives the items of [`scan`] over the whole of t. The result is the
//! list of [`Items`] in pre-order, or [`ScanFailed`] when a depth limit of [F21 §2.7] is exceeded; nothing else
//! (size, git state, time, configuration) changes it ([F21 §1.1]).
//!
//! | Language ([`Lang`]) | Scanner | Specification |
//! |---|---|---|
//! | Rust (`.rs`) | a byte-level tokenizer ([`tokens`], [`canon`]) under a bracket-group reader that recognises `mod`, `impl [Trait for] T`, `fn`, `struct`, `enum`, `trait`, `const`, `static` and `macro_rules!` | [F21 §3] |
//! | Markdown (`.md`, `.markdown`) | a fence-aware ATX and setext line scanner with HTML comment blocks, front matter and container first lines | [F21 §4] |
//! | TOML (`.toml`) | a table-header and key line scanner with a value scan across lines | [F21 §5] |
//!
//! # Memory
//!
//! No scanner holds the text. Rust keeps a stack of at most [`RUST_MAX_DEPTH`] open groups with one open item per
//! code group, at most [`SCOPE_MAX_BYTES`] + 1 bytes of the current token (only while a name or an `impl` header
//! needs them), and for each `impl` header being read (at most one per open group) its two parts as ranges of the
//! items' text buffer; Markdown a few flags and counters for the line being read and the heading fields of the open
//! paragraph and of an ATX heading, capped as below; TOML a few states for the line being read, the key path's
//! spelling capped likewise, and a stack of at most [`TOML_MAX_DEPTH`] brackets ([F21 §2.8]). The line scanners read
//! a line byte by byte and keep none of it, so no scanner's memory grows with the length of a line. No function
//! recurses once per nesting level.
//!
//! **Long names.** A name or qualifier longer than [`SCOPE_MAX_BYTES`] bytes is long: no recordable name path holds
//! one and no selector segment equals one ([F21 §2.3, §6.1]), so an item keeps [F21 §2.3]'s marker — a flag, its
//! first 64 bytes for display ([`Items::path_text`] marks them `%…`) and, for a Rust `impl`, its bare name
//! ([F21 §6.2]), the one way a selector still matches it. An `impl` header part that passes the cap keeps the range
//! of its first 64 bytes, reads on only until its bare name is decided, and then takes no more tokens.
//!
//! **Items.** Each item costs one 40-byte record (64-bit target) and its name and qualifier, at most
//! [`SCOPE_MAX_BYTES`] bytes each (64 when long). The Rust scanner writes the tokens of the `impl` headers it reads
//! to the items' text buffer once each, as they arrive, and an `impl` name or qualifier is a range of it; a header
//! nested in another's group, and an item named inside one, share the enclosing header's bytes instead of copying
//! them ([`Items`] "The text buffer"). A tentative item that fails is killed in O(1) and compacted away once at the
//! end. So memory and time stay linear in the text, with a small constant, however deeply headers and failing groups
//! nest. Measured in release on 64-bit Windows: 250 blocks of 500 `impl[{` headers nested in each other's groups
//! (1 MB of text, 124,751 items whose names add up to 249 MB) take 6.0 MB of items — 6 bytes per byte of text, 5 of
//! them the records of one item per 8 bytes — at a 6.5 MB peak, in 1.3 times the time of a flat text of as many
//! bytes; 340 nested `impl b<{ ` headers repeated to 1 MB take 3.5 bytes per byte; 100,000 items inside 500 nested
//! `pub(` groups or empty `impl` headers that fail scan in the time the same items take flat.
//!
//! One rule looks ahead further than a bounded state allows: a Markdown text whose line 1 is `---` keeps the
//! headings of the lines after it until a closing `---` or `...` line discards them or the end keeps them
//! ([F21 §4.5]). The shebang rule ([F21 §3.1] rule 5) holds no bytes: until the first token after `#!` starts, the
//! Rust scanner reads the text both with and without a shebang and keeps the reading the token decides.
//!
//! A name that a literal running to the end of the text ends (an unterminated literal in an `impl` header) includes
//! a final `0A`, so a scanner must be fed that byte when the text has it: the lines of [F20 §2.5] alone cannot tell
//! `x` from `x⏎`.
//!
//! # Status
//!
//! \[F21\] is normative for these scanners. While [F20 §6.1]'s interim scanner rule holds ([F21 §1.4]), capture records
//! no scope and refuses the `symbol` and `heading` forms: that decision belongs to the anchor resolver, which calls
//! this module only once the rule is lifted. The differential against `moirai-tsoracle` ([F21 §3.9]; WP-74) reads
//! [`scan`]'s items for a Rust file: kind ([`Lang::skind_name`]), name, qualifier, lines, parent and order are the
//! fields of the oracle's record, and [`Item::name_long`] marks a name it holds only a prefix of.
//!
//! The goldens in `tests/scan_golden.rs` are this module's own transcription of [F21 §8]. The independent checks are
//! that differential over the repository's own Rust files (WP-74; row 8 of WP-76) and R-FIX's scanner fixtures
//! (`fixtures/r4/INDEX.md` G-6).

mod form;
mod items;
mod markdown;
mod rust;
mod scope;
mod toml;

pub use form::{
    FormKind, FormOutcome, Selector, SelectorError, SelectorSegment, find_form, split_heading,
    split_symbol,
};
pub use items::{Item, Items};
pub use rust::{LiteralKind, Token, TokenKind, Tokens, canon, tokens};
pub use scope::{Scope, ScopeError, Segment, Segments};

use crate::text::eqi;

/// `SCOPE_MAX_SEGMENTS`: the most segments of a recordable name path ([F21 §2.3], [F08 §10.3.1] `n`).
pub const SCOPE_MAX_SEGMENTS: usize = 64;

/// `SCOPE_MAX_BYTES`: the most bytes of a recordable scope value, and of an authoring-form segment after its escapes
/// ([F21 §2.3, §6.1]).
pub const SCOPE_MAX_BYTES: usize = 4096;

/// `RUST_MAX_DEPTH`: the most open groups of the Rust scanner; one more fails the scan ([F21 §2.7, §3.3]).
pub const RUST_MAX_DEPTH: usize = 1024;

/// `TOML_MAX_DEPTH`: the most open brackets of one TOML value; one more fails the scan ([F21 §2.7, §5.4]).
pub const TOML_MAX_DEPTH: usize = 1024;

/// `MD_MAX_LEVEL`: the most `#` bytes of an ATX heading ([F21 §4.3]).
pub const MD_MAX_LEVEL: usize = 6;

/// `MD_MAX_INDENT`: the most columns of indentation of a shallow Markdown line ([F21 §4.1]).
pub const MD_MAX_INDENT: usize = 3;

/// `MD_TAB_STOP`: the tab stop of Markdown indentation ([F21 §4.1]).
pub const MD_TAB_STOP: usize = 4;

/// `MD_FENCE_MIN`: the fewest bytes of a Markdown fence ([F21 §4.5]).
pub const MD_FENCE_MIN: usize = 3;

/// `NUM_MAX_DIGITS`: the most digits of a heading-numbering component and of an ordered-list marker
/// ([F21 §4.6, §4.7]).
pub const NUM_MAX_DIGITS: usize = 9;

/// A language that has a scanner ([F21 §1.3]); its [`code`](Lang::code) is the scope value's `lang` byte
/// ([F08 §10.3.1]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Lang {
    /// `lang` 1: files whose last path component ends in `.rs`.
    Rust,
    /// `lang` 2: `.md` or `.markdown`.
    Markdown,
    /// `lang` 3: `.toml`.
    Toml,
}

impl Lang {
    /// The language of a root-relative path, from its last component compared with `eqi` ([F21 §1.3]); `None` for
    /// every other file, which has no scanner and no items ([F21 §8.4]).
    #[must_use]
    pub fn of_path(path: &[u8]) -> Option<Lang> {
        let last = path.rsplit(|&b| b == b'/').next().unwrap_or(path);
        if ends_with_eqi(last, b".rs") {
            Some(Lang::Rust)
        } else if ends_with_eqi(last, b".md") || ends_with_eqi(last, b".markdown") {
            Some(Lang::Markdown)
        } else if ends_with_eqi(last, b".toml") {
            Some(Lang::Toml)
        } else {
            None
        }
    }

    /// The scope value's `lang` byte: 1, 2 or 3 ([F08 §10.3.1]).
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Lang::Rust => 1,
            Lang::Markdown => 2,
            Lang::Toml => 3,
        }
    }

    /// The language of a `lang` byte, or `None` outside 1–3.
    #[must_use]
    pub const fn from_code(code: u8) -> Option<Lang> {
        match code {
            1 => Some(Lang::Rust),
            2 => Some(Lang::Markdown),
            3 => Some(Lang::Toml),
            _ => None,
        }
    }

    /// The name of the scope text ([F14 §5.6] `lang`): `rust`, `markdown` or `toml`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Lang::Rust => "rust",
            Lang::Markdown => "markdown",
            Lang::Toml => "toml",
        }
    }

    /// The name of an item kind ([F14 §5.6] `skind`, [F08 §10.3.1]): Rust `mod`, `impl`, `fn`, `struct`, `enum`,
    /// `trait`, `const`, `static`, `macro_rules` for 1–9; Markdown `h1`–`h6`; TOML `table`, `array_table`, `key` for
    /// 1–3. `None` for a code the language does not define.
    #[must_use]
    pub const fn skind_name(self, skind: u8) -> Option<&'static str> {
        match (self, skind) {
            (Lang::Rust, 1) => Some("mod"),
            (Lang::Rust, 2) => Some("impl"),
            (Lang::Rust, 3) => Some("fn"),
            (Lang::Rust, 4) => Some("struct"),
            (Lang::Rust, 5) => Some("enum"),
            (Lang::Rust, 6) => Some("trait"),
            (Lang::Rust, 7) => Some("const"),
            (Lang::Rust, 8) => Some("static"),
            (Lang::Rust, 9) => Some("macro_rules"),
            (Lang::Markdown, 1) => Some("h1"),
            (Lang::Markdown, 2) => Some("h2"),
            (Lang::Markdown, 3) => Some("h3"),
            (Lang::Markdown, 4) => Some("h4"),
            (Lang::Markdown, 5) => Some("h5"),
            (Lang::Markdown, 6) => Some("h6"),
            (Lang::Toml, 1) => Some("table"),
            (Lang::Toml, 2) => Some("array_table"),
            (Lang::Toml, 3) => Some("key"),
            _ => None,
        }
    }
}

fn ends_with_eqi(x: &[u8], suffix: &[u8]) -> bool {
    x.len() >= suffix.len() && eqi(&x[x.len() - suffix.len()..], suffix)
}

/// A failed scan ([F21 §2.7]): the Rust scanner would hold more than [`RUST_MAX_DEPTH`] open groups, or the TOML
/// scanner more than [`TOML_MAX_DEPTH`] open brackets of one value. A failed scan has no items: capture records no
/// scope, no scope resolves, no item header counts, and a `symbol` or `heading` form is refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ScanFailed;

impl std::fmt::Display for ScanFailed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("scope scan failed: nesting deeper than 1024")
    }
}

impl std::error::Error for ScanFailed {}

/// A scanner of one language over an anchor text fed in chunks ([F21 §1.3]).
///
/// ```
/// use moirai_files::scan::{Lang, Scanner};
///
/// let mut s = Scanner::new(Lang::Rust);
/// s.feed(b"mod a { fn f");
/// s.feed(b"() {} }\n");
/// let items = s.finish().unwrap();
/// let names: Vec<&str> = items.iter().map(|i| i.name).collect();
/// assert_eq!(names, ["a", "f"]);
/// ```
#[derive(Debug)]
pub struct Scanner {
    inner: Inner,
}

#[derive(Debug)]
enum Inner {
    Rust(Box<rust::RustScanner>),
    Markdown(markdown::MarkdownScanner),
    Toml(toml::TomlScanner),
}

impl Scanner {
    /// A scanner at the start of an anchor text of language `lang`.
    #[must_use]
    pub fn new(lang: Lang) -> Scanner {
        let inner = match lang {
            Lang::Rust => Inner::Rust(Box::new(rust::RustScanner::new())),
            Lang::Markdown => Inner::Markdown(markdown::MarkdownScanner::new()),
            Lang::Toml => Inner::Toml(toml::TomlScanner::new()),
        };
        Scanner { inner }
    }

    /// The language scanned.
    #[must_use]
    pub fn lang(&self) -> Lang {
        match self.inner {
            Inner::Rust(_) => Lang::Rust,
            Inner::Markdown(_) => Lang::Markdown,
            Inner::Toml(_) => Lang::Toml,
        }
    }

    /// The next bytes of the anchor text. Chunks may split the text anywhere, inside a token, a UTF-8 sequence or
    /// a line.
    pub fn feed(&mut self, chunk: &[u8]) {
        match &mut self.inner {
            Inner::Rust(s) => s.feed(chunk),
            Inner::Markdown(s) => s.feed(chunk),
            Inner::Toml(s) => s.feed(chunk),
        }
    }

    /// Whether the scan has already failed ([F21 §2.7]); the rest of the text changes nothing, so a caller may stop
    /// feeding. Rust reports a failure at the token that opens the 1,025th group (once the shebang rule of
    /// [F21 §3.1] is decided); TOML at the byte that opens a value's 1,025th bracket; Markdown never fails.
    #[must_use]
    pub fn has_failed(&self) -> bool {
        match &self.inner {
            Inner::Rust(s) => s.has_failed(),
            Inner::Markdown(_) => false,
            Inner::Toml(s) => s.has_failed(),
        }
    }

    /// The end of the text: the items in pre-order ([F21 §2.1]).
    ///
    /// # Errors
    /// [`ScanFailed`] when a depth limit was exceeded ([F21 §2.7]).
    pub fn finish(self) -> Result<Items, ScanFailed> {
        match self.inner {
            Inner::Rust(s) => s.finish(),
            Inner::Markdown(s) => Ok(s.finish()),
            Inner::Toml(s) => s.finish(),
        }
    }
}

/// `scan(t)` ([F21 §1.3]): the items of the whole anchor text `t` of language `lang`.
///
/// # Errors
/// [`ScanFailed`] when a depth limit was exceeded ([F21 §2.7]).
pub fn scan(lang: Lang, t: &[u8]) -> Result<Items, ScanFailed> {
    let mut s = Scanner::new(lang);
    s.feed(t);
    s.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn languages_follow_the_last_component() {
        assert_eq!(Lang::of_path(b"src/lib.rs"), Some(Lang::Rust));
        assert_eq!(Lang::of_path(b"SRC/LIB.RS"), Some(Lang::Rust));
        assert_eq!(Lang::of_path(b"docs/a.md"), Some(Lang::Markdown));
        assert_eq!(Lang::of_path(b"docs/A.Markdown"), Some(Lang::Markdown));
        assert_eq!(Lang::of_path(b"Cargo.toml"), Some(Lang::Toml));
        assert_eq!(Lang::of_path(b"a.rs/b.txt"), None);
        assert_eq!(Lang::of_path(b"x.py"), None);
        assert_eq!(Lang::of_path(b"x.hlsl"), None);
        assert_eq!(Lang::of_path(b"x.json"), None);
        assert_eq!(Lang::of_path(b"x.txt"), None);
        assert_eq!(Lang::of_path(b"rs"), None);
        assert_eq!(Lang::of_path(b".rs"), Some(Lang::Rust));
        assert_eq!(Lang::of_path(b"C#/x.MD"), Some(Lang::Markdown));
    }

    #[test]
    fn codes_and_names() {
        for lang in [Lang::Rust, Lang::Markdown, Lang::Toml] {
            assert_eq!(Lang::from_code(lang.code()), Some(lang));
        }
        assert_eq!(Lang::from_code(0), None);
        assert_eq!(Lang::from_code(4), None);
        assert_eq!(Lang::Rust.skind_name(9), Some("macro_rules"));
        assert_eq!(Lang::Rust.skind_name(10), None);
        assert_eq!(Lang::Markdown.skind_name(6), Some("h6"));
        assert_eq!(Lang::Markdown.skind_name(7), None);
        assert_eq!(Lang::Toml.skind_name(2), Some("array_table"));
        assert_eq!(Lang::Toml.skind_name(0), None);
    }
}
