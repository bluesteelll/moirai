//! The tree-sitter-rust walk that finds the scope items of one Rust source ([40 §2.7.1]; [F08 §10.3.1]).
//!
//! The item rules are the crate documentation's "Items" section. This module implements them over
//! tree-sitter-rust's syntax tree: every node of one of the item node kinds of [`ITEM_NODES`] that has a non-empty
//! name is an item, wherever it is in the tree, and its parent is the nearest enclosing reported item. Each item
//! carries rule 8's `ok` flag: whether the syntax errors of the source leave its name path and span untouched.

use std::fmt;
use std::ops::Range;

use tree_sitter::{Language, Node, Parser};

use crate::canon::canon_into;

/// The Rust item kinds of a scope segment, with [F08 §10.3.1]'s `skind` codes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    /// `mod` (`skind` 1).
    Mod,
    /// `impl [Trait for] T` (`skind` 2).
    Impl,
    /// `fn`, with or without a body (`skind` 3).
    Fn,
    /// `struct` (`skind` 4).
    Struct,
    /// `enum` (`skind` 5).
    Enum,
    /// `trait` (`skind` 6).
    Trait,
    /// `const` (`skind` 7).
    Const,
    /// `static` (`skind` 8).
    Static,
    /// `macro_rules!` (`skind` 9).
    MacroRules,
}

impl Kind {
    /// Every kind, in `skind` order.
    pub const ALL: [Kind; 9] = [
        Kind::Mod,
        Kind::Impl,
        Kind::Fn,
        Kind::Struct,
        Kind::Enum,
        Kind::Trait,
        Kind::Const,
        Kind::Static,
        Kind::MacroRules,
    ];

    /// The kind's name in the JSON record and in [F08 §10.3.1]'s `skind` table.
    pub const fn name(self) -> &'static str {
        match self {
            Kind::Mod => "mod",
            Kind::Impl => "impl",
            Kind::Fn => "fn",
            Kind::Struct => "struct",
            Kind::Enum => "enum",
            Kind::Trait => "trait",
            Kind::Const => "const",
            Kind::Static => "static",
            Kind::MacroRules => "macro_rules",
        }
    }

    /// The kind's `skind` code for `lang` = 1 (`rust`), [F08 §10.3.1].
    pub const fn skind(self) -> u8 {
        match self {
            Kind::Mod => 1,
            Kind::Impl => 2,
            Kind::Fn => 3,
            Kind::Struct => 4,
            Kind::Enum => 5,
            Kind::Trait => 6,
            Kind::Const => 7,
            Kind::Static => 8,
            Kind::MacroRules => 9,
        }
    }
}

/// The tree-sitter-rust node kinds that are items, and the kind each reports as.
///
/// `function_signature_item` is a `fn` without a body: a trait method's declaration or a foreign function.
pub const ITEM_NODES: [(&str, Kind); 10] = [
    ("mod_item", Kind::Mod),
    ("impl_item", Kind::Impl),
    ("function_item", Kind::Fn),
    ("function_signature_item", Kind::Fn),
    ("struct_item", Kind::Struct),
    ("enum_item", Kind::Enum),
    ("trait_item", Kind::Trait),
    ("const_item", Kind::Const),
    ("static_item", Kind::Static),
    ("macro_definition", Kind::MacroRules),
];

/// One scope item of a [`Scan`], borrowed from it (crate documentation, "Items" and "Output").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Item<'a> {
    /// The item kind.
    pub kind: Kind,
    /// The canonical name ([`crate::canon`]; rule 3): never empty, one line.
    pub name: &'a str,
    /// The canonical qualifier (rule 4): an `impl Trait for T`'s trait (with a leading `!` for a negative impl), empty
    /// otherwise.
    pub qual: &'a str,
    /// The first line of the item, 1-based: the line of its first token (attributes and comments before it are not
    /// part of the item; rule 5).
    pub start: usize,
    /// The last line of the item, 1-based and inclusive: the line of its last byte (rule 5).
    pub end: usize,
    /// The index in [`Scan::items`] of the nearest enclosing item, if any (rule 6). It is always less than this item's
    /// index.
    pub parent: Option<usize>,
    /// Whether the item is inside the oracle's claim (rule 8): no syntax error lies in its node, in an enclosing node,
    /// in the name of an enclosing item, directly before it on an earlier line, or anywhere before it while leaving
    /// brackets unbalanced.
    pub ok: bool,
}

/// An item as a [`Scan`] stores it. Its name and qualifier are ranges of the scan's text buffer, so they are read only
/// through the scan that wrote them ([`Scan::items`]).
#[derive(Clone, Debug)]
struct Entry {
    kind: Kind,
    start: usize,
    end: usize,
    parent: Option<usize>,
    ok: bool,
    name: Range<usize>,
    qual: Range<usize>,
}

/// The result of scanning one source: the error count and the items in document order.
///
/// A `Scan` is reused across sources ([`Oracle::scan`] clears it), so its buffers are allocated once per run.
#[derive(Debug, Default)]
pub struct Scan {
    errors: u32,
    items: Vec<Entry>,
    text: String,
}

impl Scan {
    /// An empty scan.
    pub fn new() -> Self {
        Self::default()
    }

    /// The number of ERROR and MISSING nodes in the syntax tree, plus one when the source (its BOM removed) is not
    /// valid UTF-8 (crate documentation, "Output"). Which items the errors leave inside the claim is each item's
    /// [`Item::ok`].
    pub fn errors(&self) -> u32 {
        self.errors
    }

    /// The items in pre-order: by first byte, an enclosing item before the items it contains (rule 7).
    pub fn items(&self) -> impl ExactSizeIterator<Item = Item<'_>> + DoubleEndedIterator {
        self.items.iter().map(|e| self.view(e))
    }

    /// The item at `index` in [`Scan::items`] (an [`Item::parent`], for example), if there is one.
    pub fn item(&self, index: usize) -> Option<Item<'_>> {
        self.items.get(index).map(|e| self.view(e))
    }

    fn view(&self, e: &Entry) -> Item<'_> {
        Item {
            kind: e.kind,
            name: &self.text[e.name.clone()],
            qual: &self.text[e.qual.clone()],
            start: e.start,
            end: e.end,
            parent: e.parent,
            ok: e.ok,
        }
    }

    fn clear(&mut self) {
        self.errors = 0;
        self.items.clear();
        self.text.clear();
    }
}

/// Why the oracle could not start or could not parse a source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OracleError {
    /// The linked tree-sitter runtime refused the grammar (an ABI mismatch).
    Language(String),
    /// The grammar lacks a node kind or field this oracle relies on (a grammar other than the pinned one).
    Grammar(&'static str),
    /// The input, BOM included, has at least this many bytes: more than [`MAX_INPUT`], or a BOM-free part longer than
    /// tree-sitter's 32-bit offsets allow. A caller that stops reading after [`MAX_INPUT`] + 1 bytes reports that
    /// count.
    TooLarge(u64),
    /// The parser returned no tree.
    Parse,
}

impl fmt::Display for OracleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OracleError::Language(e) => write!(f, "tree-sitter refused the Rust grammar: {e}"),
            OracleError::Grammar(what) => write!(f, "the Rust grammar has no {what}"),
            OracleError::TooLarge(n) => {
                write!(
                    f,
                    "input of at least {n} bytes exceeds tree-sitter's 4 GiB limit"
                )
            }
            OracleError::Parse => f.write_str("tree-sitter returned no syntax tree"),
        }
    }
}

impl std::error::Error for OracleError {}

/// The largest input [`Oracle::scan`] accepts, in bytes: tree-sitter's 32-bit offsets, plus a 3-byte UTF-8 BOM. A
/// caller can refuse a larger input before reading it (crate documentation, "Command line").
pub const MAX_INPUT: u64 = u32::MAX as u64 + 3;

/// The ABI version of the linked tree-sitter-rust grammar (`--version`).
pub fn language_abi() -> usize {
    Language::from(tree_sitter_rust::LANGUAGE).abi_version()
}

/// A reusable tree-sitter-rust parser with the item tables resolved once.
pub struct Oracle {
    parser: Parser,
    /// Node kind id → item kind, for every id of the grammar.
    kinds: Vec<Option<Kind>>,
    field_name: u16,
    field_type: u16,
    field_trait: u16,
    bang: u16,
    /// Enclosing items during a walk: (index in `Scan::items`, end byte).
    stack: Vec<(usize, usize)>,
    /// During a walk, the tree depths of the enclosing nodes that take the items inside them out of the claim
    /// (rule 8): ERROR nodes, and item nodes whose name or qualifier is empty or has a syntax error.
    taint: Vec<usize>,
    /// The open brackets while [`unbalances`] reads an ERROR node.
    brackets: Vec<u8>,
}

impl Oracle {
    /// Loads the tree-sitter-rust grammar and resolves the node kinds and fields of [`ITEM_NODES`].
    pub fn new() -> Result<Self, OracleError> {
        let language: Language = tree_sitter_rust::LANGUAGE.into();
        let mut parser = Parser::new();
        parser
            .set_language(&language)
            .map_err(|e| OracleError::Language(e.to_string()))?;
        let mut kinds = vec![None; language.node_kind_count()];
        for (node, kind) in ITEM_NODES {
            let id = language.id_for_node_kind(node, true);
            if id == 0 {
                return Err(OracleError::Grammar(node));
            }
            kinds[usize::from(id)] = Some(kind);
        }
        let field = |name: &'static str| {
            language
                .field_id_for_name(name)
                .map(u16::from)
                .ok_or(OracleError::Grammar(name))
        };
        let bang = language.id_for_node_kind("!", false);
        if bang == 0 {
            return Err(OracleError::Grammar("`!` token"));
        }
        Ok(Self {
            parser,
            kinds,
            field_name: field("name")?,
            field_type: field("type")?,
            field_trait: field("trait")?,
            bang,
            stack: Vec::new(),
            taint: Vec::new(),
            brackets: Vec::new(),
        })
    }

    /// Scans one Rust source into `out`, replacing its previous contents.
    ///
    /// `src` is the file's bytes. One leading UTF-8 BOM (`EF BB BF`) is skipped, as `atext` removes it ([F20 §2.5]);
    /// line numbers are unchanged by it. Lines are counted at `0A` bytes, as `lines` does ([F20 §2.5]); a CR before
    /// an LF belongs to the line it ends. A source that is not valid UTF-8 counts one error and none of its items is
    /// `ok`: rustc rejects it, and tree-sitter turns invalid bytes into an ERROR node only outside literals and
    /// comments.
    pub fn scan(&mut self, src: &[u8], out: &mut Scan) -> Result<(), OracleError> {
        out.clear();
        let input_len = src.len();
        let src = src.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(src);
        if u32::try_from(src.len()).is_err() {
            return Err(OracleError::TooLarge(
                u64::try_from(input_len).unwrap_or(u64::MAX),
            ));
        }
        let utf8 = std::str::from_utf8(src).is_ok();
        if !utf8 {
            out.errors = 1;
        }
        let tree = self.parser.parse(src, None).ok_or(OracleError::Parse)?;
        self.stack.clear();
        self.taint.clear();
        // The cursor's depth below the root, kept in step with its moves.
        let mut depth = 0usize;
        // Rule 8: once an error leaves brackets unbalanced, no later item is claimed. Nodes are visited in pre-order,
        // so every item visited after the error starts at or after it.
        let mut shifted = false;
        let mut cursor = tree.walk();
        loop {
            let node = cursor.node();
            let mut taints = node.is_error();
            if taints || node.is_missing() {
                out.errors = out.errors.saturating_add(1);
                shifted = shifted || unbalances(node, src, &mut self.brackets);
            }
            if let Some(kind) = self
                .kinds
                .get(usize::from(node.kind_id()))
                .copied()
                .flatten()
            {
                let path_ok = utf8 && !shifted && self.taint.is_empty();
                taints |= !self.push_item(kind, node, src, path_ok, out);
            }
            if cursor.goto_first_child() {
                if taints {
                    self.taint.push(depth);
                }
                depth += 1;
                continue;
            }
            loop {
                if cursor.goto_next_sibling() {
                    break;
                }
                if !cursor.goto_parent() {
                    return Ok(());
                }
                depth -= 1;
                if self.taint.last() == Some(&depth) {
                    self.taint.pop();
                }
            }
        }
    }

    /// Records `node` as an item of `kind` if it has a non-empty name; `path_ok` says whether its enclosing nodes
    /// leave it inside the claim (rule 8). Returns whether the item's header (its name and, for an `impl`, its trait)
    /// is non-empty and free of syntax errors; when it is not, the items inside the node are not `ok` either.
    fn push_item(
        &mut self,
        kind: Kind,
        node: Node<'_>,
        src: &[u8],
        path_ok: bool,
        out: &mut Scan,
    ) -> bool {
        // In pre-order, an earlier item is an ancestor of `node` iff it ends after `node` starts (items are never
        // empty, and siblings do not overlap).
        let start_byte = node.start_byte();
        while self.stack.last().is_some_and(|&(_, end)| end <= start_byte) {
            self.stack.pop();
        }
        // Error recovery can leave an item node without its name (or, for an `impl`, its type): `impl<T> {}` gives
        // an `impl_item` whose type is a MISSING node. Rule 3 does not report such an item; the items inside it go to
        // the next enclosing item, and rule 8 takes them out of the claim.
        let (name_field, is_impl) = match kind {
            Kind::Impl => (self.field_type, true),
            _ => (self.field_name, false),
        };
        let Some(name_node) = node.child_by_field_id(name_field) else {
            return false;
        };
        let Some(name) = item_name(src, name_node.byte_range(), &mut out.text) else {
            return false;
        };
        // Only an `impl` has a trait field; no other kind pays for the lookup.
        let trait_node = if is_impl {
            node.child_by_field_id(self.field_trait)
        } else {
            None
        };
        let header_ok = !name_node.has_error() && trait_node.is_none_or(|tr| !tr.has_error());
        let qual = match trait_node {
            Some(tr) => {
                let from = self.negation_start(tr).unwrap_or_else(|| tr.start_byte());
                append_canon(src, from..tr.end_byte(), &mut out.text)
            }
            None => name.end..name.end,
        };
        let start = node.start_position();
        let end = node.end_position();
        // A node that ended with a line break would end at column 0 of the next row; its last byte is on `end.row`.
        let end_line = if end.column == 0 && end.row > start.row {
            end.row
        } else {
            end.row + 1
        };
        let index = out.items.len();
        out.items.push(Entry {
            kind,
            start: start.row + 1,
            end: end_line,
            parent: self.stack.last().map(|&(i, _)| i),
            // `has_error` covers the node and everything inside it, MISSING nodes included.
            ok: path_ok && !node.has_error() && !after_error_line(node),
            name,
            qual,
        });
        self.stack.push((index, node.end_byte()));
        header_ok
    }

    /// The start of the `!` of a negative impl (`impl !Send for T`), skipping comments between it and the trait.
    fn negation_start(&self, trait_node: Node<'_>) -> Option<usize> {
        let mut prev = trait_node.prev_sibling();
        while let Some(p) = prev {
            if p.kind_id() == self.bang {
                return Some(p.start_byte());
            }
            if !p.is_extra() {
                return None;
            }
            prev = p.prev_sibling();
        }
        None
    }
}

/// Whether an ERROR node stands directly before `node` (only comments between) and ends on an earlier line than
/// `node` starts (crate documentation, rule 8). Such an error may hold the item's visibility or a qualifier the
/// grammar does not parse — `unsafe⏎static S: u8;` in an `unsafe extern` block gives an ERROR node `unsafe` and a
/// `static_item` from `static` — so the item's first token, and with it `start` (rule 5), may lie inside the error.
/// An error on the item's own line cannot move `start`, and the name path never includes a qualifier.
fn after_error_line(node: Node<'_>) -> bool {
    let mut prev = node.prev_sibling();
    while let Some(p) = prev {
        if p.is_error() {
            return p.end_position().row < node.start_position().row;
        }
        if !p.is_extra() {
            return false;
        }
        prev = p.prev_sibling();
    }
    false
}

/// Whether the syntax error `node` (an ERROR or a MISSING node) leaves brackets unbalanced (crate documentation,
/// rule 8): a MISSING `(`, `)`, `[`, `]`, `{` or `}`, or an ERROR node whose bracket tokens, read in order, do not
/// pair up. Error recovery then closes the enclosing nodes at other brackets than the source's, so the parent, the
/// lines and the name path of every later item may be shifted: `extern r#"C"# {⏎}` in a function body (a grammar gap)
/// gives an ERROR node holding the `{`, and the function ends at the `}` meant for it. A childless ERROR node's bytes,
/// which no token covers, are read as brackets wherever they hold one. `open` is scratch space.
fn unbalances(node: Node<'_>, src: &[u8], open: &mut Vec<u8>) -> bool {
    if node.is_missing() {
        return is_bracket(node.kind().as_bytes());
    }
    open.clear();
    let mut cursor = node.walk();
    loop {
        let leaf = cursor.node();
        if cursor.goto_first_child() {
            continue;
        }
        let tokens: &[u8] = if leaf.is_missing() {
            if is_bracket(leaf.kind().as_bytes()) {
                return true;
            }
            &[]
        } else if leaf.is_error() {
            &src[leaf.byte_range()]
        } else if is_bracket(leaf.kind().as_bytes()) {
            // An anonymous token's kind is its text.
            leaf.kind().as_bytes()
        } else {
            &[]
        };
        for &b in tokens {
            let paired = match b {
                b'(' | b'[' | b'{' => {
                    open.push(b);
                    true
                }
                b')' => open.pop() == Some(b'('),
                b']' => open.pop() == Some(b'['),
                b'}' => open.pop() == Some(b'{'),
                _ => true,
            };
            if !paired {
                return true;
            }
        }
        // The cursor's root is `node`: it climbs no higher.
        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                return !open.is_empty();
            }
        }
    }
}

/// Whether a token's text is one bracket.
fn is_bracket(text: &[u8]) -> bool {
    matches!(text, b"(" | b")" | b"[" | b"]" | b"{" | b"}")
}

/// Appends the canonical name of `src[range]` to `text` and returns its range there, or returns `None` with `text`
/// unchanged when the name is empty: an item without a name is not reported (crate documentation, rule 3).
fn item_name(src: &[u8], range: Range<usize>, text: &mut String) -> Option<Range<usize>> {
    let name = append_canon(src, range, text);
    (!name.is_empty()).then_some(name)
}

/// Appends the canonical spelling of `src[range]` to `text` and returns its range there. Bytes that are not UTF-8
/// are first replaced by U+FFFD, one per maximal invalid subpart (Unicode §3.9, as [`String::from_utf8_lossy`]
/// does), so a name is always text (crate documentation, rule 3).
fn append_canon(src: &[u8], range: Range<usize>, text: &mut String) -> Range<usize> {
    let from = text.len();
    let bytes = &src[range];
    match std::str::from_utf8(bytes) {
        Ok(s) => canon_into(s, text),
        Err(_) => canon_into(&String::from_utf8_lossy(bytes), text),
    }
    from..text.len()
}

#[cfg(test)]
mod tests;
