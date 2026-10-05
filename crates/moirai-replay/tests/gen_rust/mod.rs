//! Generated Rust sources of the scanner contract ([F21 §3.9]) together with their items by construction ([F21 §2.1],
//! §3.5–§3.8, the oracle's rules 1–7). A source is Rust that rustc's parser accepts — not type-checked: its names need
//! not resolve — and that tree-sitter-rust 0.24.2 parses with no error, except for the grammar gaps below.
//!
//! A source is rendered from a tree of [`Node`]s. The renderer records each reported item when it writes the item's
//! first token (its visibility, a qualifier or its keyword, after its attributes and doc comments) and closes it when it
//! writes its final `}` or `;`, so `start`, `end`, the parent and the pre-order come from the construction, not from a
//! scanner. The name of an `impl` is computed from the tokens of its self type with [F21 §3.2]'s spacing rule (one SP
//! exactly between two adjacent words, literals or lifetimes), however the renderer spaces, breaks or comments them.
//!
//! What the sources hold besides items, and must not be reported: `use`, `extern crate`, type aliases, `union`s,
//! `extern` and `unsafe extern` blocks (transparent: their functions and statics are items of the enclosing item),
//! macro invocations and `macro_rules!` bodies with item-like text inside, comments (nested ones included), string,
//! raw-string, byte, byte-string, C-string, raw byte- and C-string and character literals holding braces and
//! item-like text, in statements and in macro invocations at item level ([F21 §3.1] rule 4), lifetimes and labels,
//! closures, `let` blocks, `match` arms and `unsafe`, `if`, `loop`, `while`, `async` and `const` blocks (whose items
//! belong to the enclosing item), `code` groups inside `plain` ones — a closure body in an argument list, a block in
//! an array length ([F21 §3.3]) — with items inside, a macro invocation as a return type (`fn f() -> ty! { u8 } {`,
//! [F21 §3.6] step 5), attributes and doc comments before items and before associated items — line and block doc
//! comments holding braces, an inner block doc comment at the start of the file ([F21 §3.1] rule 2) — every
//! visibility (`pub(crate)`, `pub(super)`, `pub(self)`, `pub(in path)`, on associated items of inherent `impl`s too)
//! and on a line of its own, several items on one line, a shebang, and names that are weak keywords, raw identifiers
//! or non-ASCII.
//!
//! Headers hold the forms [F21 §3.6] and §3.7 have rules for: const-generic blocks as arguments (`Foo<{ N }>`, after
//! `<` or `,`) and as defaults in `struct` and `trait` parameters (`<const N: usize = { 3 }>`, after `=`), [F21 §3.6]
//! step 2; a qualified path in a type (`<T as Iterator>::Item`), also as the self type after `for`, where `for`
//! followed by `<` and no lifetime is the separator and not a binder, [F21 §3.7] step 2; higher-ranked binders
//! (`for<'a> fn(&'a u8)`); trait objects with several bounds, written in parentheses after `&` and as a function
//! pointer's return type, where rustc refuses a bare bound list (`&(dyn A + B)`, `fn() -> (dyn A + B)`).
//!
//! **Grammar gaps.** Two constructs are stable Rust that tree-sitter-rust 0.24.2 does not parse (the oracle's crate
//! documentation, "Known grammar gaps"): the `safe` and `unsafe` qualifiers of items in an `unsafe extern` block
//! ([F21 §3.5] step 2), and an ABI written as a raw string (`extern r"C" fn`, `extern r#"C"# { … }`, the type
//! `extern r"C" fn(u8)`; rustc takes `r` raw strings as ABIs, [F21 §3.5] step 2 "a string or raw string literal").
//! The generator writes them, rarely, sometimes with the item's first token (its visibility or that qualifier) on a
//! line of its own, and [`render`] says so: such a source is held to its construction by the scanner, and by the
//! oracle only on the items it claims — which checks that its claim leaves out an item whose first token the error
//! may hold, and every item after an error that leaves brackets unbalanced (its rule 8). `const default` is left out altogether ([`const_name`]), and so are the byte and C raw
//! strings `br"C"` and `cr"C"`, which [F21 §3.5] also reads as ABIs but rustc refuses (outside the contract).

use moirai_replay::scandiff::Row;
use proptest::prelude::*;

/// Names of items: plain, non-ASCII, raw identifiers, and words outside [F21 §3.1]'s keyword set K.
const NAMES: &[&str] = &[
    "a",
    "b",
    "alpha",
    "Beta",
    "Gamma2",
    "_x",
    "größe",
    "Ωmega",
    "r#match",
    "r#type",
    "default",
    "union",
    "auto",
    "safe",
    "raw",
    "gen",
    "macro_rules",
];

/// Names in type paths.
const TYPE_NAMES: &[&str] = &["S", "Wrap", "Vec", "T", "Größe", "Option"];

/// Primitive types: never in a longer path and never with generic arguments, which tree-sitter-rust refuses (and rustc
/// too, after parsing).
const PRIMITIVES: &[&str] = &["u8", "str", "bool"];

/// Trait names.
const TRAIT_NAMES: &[&str] = &[
    "Tr",
    "From",
    "Iterator",
    "Display",
    "Send",
    "AsRef",
    "PartialEq",
];

/// Associated type names of qualified paths.
const ASSOC_NAMES: &[&str] = &["Item", "Output", "Target"];

/// A visibility.
#[derive(Clone, Copy, Debug)]
pub enum Vis {
    None,
    Pub,
    PubCrate,
    PubSuper,
    PubSelf,
    PubInPath,
}

impl Vis {
    fn text(self) -> &'static str {
        match self {
            Vis::None => "",
            Vis::Pub => "pub",
            Vis::PubCrate => "pub(crate)",
            Vis::PubSuper => "pub(super)",
            Vis::PubSelf => "pub(self)",
            Vis::PubInPath => "pub(in crate::a)",
        }
    }
}

/// A type, as tokens.
#[derive(Clone, Debug)]
pub enum Ty {
    /// A primitive type.
    Prim(u8),
    /// `prefix Name<args>::Name<args>`.
    Path(u8, Vec<(u8, Vec<Ty>)>),
    /// `&'a mut T`.
    Ref(bool, bool, Box<Ty>),
    /// `(A, B)`.
    Tuple(Vec<Ty>),
    /// `[T; n]`.
    Array(Box<Ty>, u8),
    /// `[T]`.
    Slice(Box<Ty>),
    /// `[unsafe] [extern "C"] fn(args) [-> ret]`, optionally with a `for<'a>` binder, or `extern r"C" fn(args)` (a
    /// grammar gap of the oracle): the qualifier is 0 none, 1 `unsafe`, 2 `extern "C"`, 3 the binder, 4 the raw ABI.
    FnPtr(u8, Vec<Ty>, Option<Box<Ty>>),
    /// `dyn A + B`.
    Dyn(Vec<u8>),
    /// The qualified path `<T as Tr>::Name`: the type, the trait and the associated name.
    QPath(Box<Ty>, u8, u8),
    /// A const-generic argument block: `{ 3 }`, `{ N + 1 }` or `{ "a⏎b".len() }`.
    Const(u8),
    /// A lifetime argument `'a`.
    Lifetime,
}

/// An item or an item-free construct at item level.
#[derive(Clone, Debug)]
pub enum Node {
    Mod(Style, Vis, u8, Option<Vec<Node>>),
    Fn(Style, Vis, u8, u8, u8, Vec<Stmt>),
    /// A `struct`: rendering choices, visibility, name, and its form, by index into [`STRUCT_FORMS`].
    Struct(Style, Vis, u8, u8),
    Enum(Style, Vis, u8),
    /// A `trait`: rendering choices, visibility, `unsafe`, a const generic parameter with a braced default, name,
    /// items.
    Trait(Style, Vis, bool, bool, u8, Vec<Assoc>),
    Impl(Style, bool, bool, Option<(bool, Ty)>, Ty, bool, Vec<Assoc>),
    Const(Style, Vis, Option<u8>, Option<Vec<Node>>),
    Static(Style, Vis, bool, u8, Option<Vec<Node>>),
    MacroRules(Style, u8, u8),
    /// An `extern` block, `unsafe` or not, with its ABI (`true`: a raw string, a grammar gap of the oracle; else
    /// `"C"`) and its functions (`true`) and statics, each with a qualifier index into [`EXTERN_QUALS`] (used only in
    /// an `unsafe` block), whether its first token stands on a line of its own, and its name.
    Extern(Style, bool, bool, Vec<(u8, bool, bool, u8)>),
    Noise(u8),
}

/// A statement of a function body.
#[derive(Clone, Debug)]
pub enum Stmt {
    Item(Node),
    LetBlock(Vec<Node>),
    Closure(Vec<Node>),
    Match(Vec<Node>),
    /// A block statement or expression, by index into [`BLOCKS`].
    Block(u8, Vec<Node>),
    /// A closure body in a call's argument list: a `code` group in a `plain` one ([F21 §3.3]).
    ArgClosure(Vec<Node>),
    /// A block in an array length: a `code` group in a `plain` `[` group.
    ArrayLen(Vec<Node>),
    Noise(u8),
}

/// An item of a trait or `impl` body.
#[derive(Clone, Debug)]
pub enum Assoc {
    /// A method: rendering choices, visibility (written in an inherent `impl` only), qualifier index, name, body
    /// (`None`: a declaration, trait only).
    Fn(Style, Vis, u8, u8, Option<Vec<Stmt>>),
    /// An associated `const`, with or without a value: rendering choices, visibility (as for `Fn`), name.
    Const(Style, Vis, u8, bool),
    /// An associated `type` (not an item).
    Type(u8),
}

/// Rendering choices of one item: bit 0 a doc comment, bit 1 an attribute, bit 2 the attribute on the item's line,
/// bit 3 the visibility on a line of its own, bit 4 on the same line as the previous item, bit 5 a comment after the
/// keyword, bits 6–7 which attribute and which doc comment ([`Out::prelude`]).
pub type Style = u8;

/// What follows a `struct`'s name: no fields, a tuple, a generic struct with fields, and const generic parameters
/// with braced defaults ([F21 §3.6] step 2: a `{` group after `=` inside the angle brackets is no body).
const STRUCT_FORMS: &[&str] = &[
    ";",
    "(u8, pub Vec<u8>);",
    "<T> {\n    a: T,\n    b: [u8; 4],\n}",
    "<const N: usize = { 3 }>;",
    "<T, const M: usize = {\n    1 + 2\n}> {\n    a: [T; M],\n}",
];

/// Item-free constructs at item level.
const ITEM_NOISE: &[&str] = &[
    "use std::fmt::{self, Display};",
    "use super::*;",
    "extern crate alloc;",
    "type Alias<T> = Vec<T>;",
    "union U { a: u8, b: u16 }",
    "foo! { fn hidden() {} mod m {} }",
    "bar!(struct Hidden;);",
    "thread_local! { static TL: u8 = 1; }",
    "lazy_static! { static ref X: u8 = 1; }",
    "// fn commented() {}",
    "/* mod c { fn x() {} } /* nested } */ struct N; */",
    "#[doc = \"fn fake() {}\"]\nuse a::b;",
    "a::b! {\n    impl Fake for S {}\n}",
    "lits!(b\"}\", c\"{\", br#\"} fn fake() {\"#, cr#\"{\"#, b'{');",
    "/** } mod fake { */\nuse a::c;",
];

/// Item-free statements.
const STMT_NOISE: &[&str] = &[
    "let x = 1;",
    "let s = \"fn fake() { mod m {\";",
    "let r = r#\"struct Fake; }\"#;",
    "let c = ['{', '}', '\\''];",
    "let t = b'}';",
    "'outer: loop { break 'outer; }",
    "if x > 1 { let _ = 2; }",
    "let v = vec![1, 2];",
    "println!(\"{}\", \"}\");",
    "let _f = |a: u8| -> u8 { a };",
    "unsafe { let _ = 1; }",
    "let _ = x as u16;",
    "let e = \"\\\"}\";",
    "let bs = b\"} fn fake() {\";",
    "let cs = c\"{ mod m {\";",
    "let rb = br#\"} struct Fake; \"#;",
    "let rc = cr##\"{ \"# }\"##;",
    "/** } fn fake() { */\nlet _d = 1;",
];

/// Block statements, as (opening, closing) around the items: `unsafe`, `if`, `loop`, `while`, `async` and inline
/// `const` blocks, none an item ([F21 §3.5]).
const BLOCKS: &[(&str, &str)] = &[
    ("unsafe ", ""),
    ("if x > 1 ", " else { let _ = 2; }"),
    ("loop ", ""),
    ("while x < 1 ", ""),
    ("let _a = async ", ";"),
    ("let _k = const ", ";"),
];

/// Qualifiers of an item in an `unsafe extern` block (Rust 1.82): none, `safe` or `unsafe`. The last two are grammar
/// gaps of the oracle.
const EXTERN_QUALS: &[&str] = &["", "safe ", "unsafe "];

/// `fn` qualifiers at item level, and whether each is a grammar gap of the oracle (a raw-string ABI). The gaps come
/// last, so [`fn_qual`] can pick them rarely.
const FN_QUALS: &[(&str, bool)] = &[
    ("", false),
    ("", false),
    ("const ", false),
    ("async ", false),
    ("unsafe ", false),
    ("const unsafe ", false),
    ("extern \"C\" ", false),
    ("unsafe extern \"C\" ", false),
    ("extern r\"C\" ", true),
    ("const unsafe extern r#\"system\"# ", true),
];

/// The ABIs of an `extern` block: `"C"`, or a raw string (a grammar gap of the oracle).
const EXTERN_ABIS: [&str; 2] = ["extern \"C\" ", "extern r#\"C\"# "];

/// An index into [`FN_QUALS`]: a grammar gap about once in forty.
fn fn_qual() -> impl Strategy<Value = u8> {
    let plain = FN_QUALS.iter().position(|q| q.1).unwrap_or(FN_QUALS.len()) as u8;
    prop_oneof![
        19 => 0..plain,
        1 => plain..FN_QUALS.len() as u8,
    ]
}

/// Signatures after a function's name.
const SIGS: &[&str] = &[
    "()",
    "(x: u8) -> u8",
    "<T: Into<u8>, const N: usize>(x: [T; N]) -> impl Iterator<Item = u8>\nwhere\n    T: Clone,",
    "(\n    a: &str,\n    b: Vec<Vec<u8>>,\n) -> Result<(), Box<dyn std::error::Error>>",
    "() -> Foo<{ N }>",
    "<'a>(x: &'a [u8]) -> &'a u8",
    "(f: impl Fn(u8) -> u8)",
    "() -> ty! { u8 }",
    "() -> m!(u8)",
];

fn name() -> impl Strategy<Value = u8> {
    0..NAMES.len() as u8
}

/// A name for a `const`: tree-sitter-rust 0.24.2 refuses a `const` named `default` (a grammar gap of the oracle,
/// listed in its crate documentation), so such a source would fall outside the oracle's claim.
fn const_name() -> impl Strategy<Value = u8> {
    name().prop_filter("`const default` is a grammar gap", |&n| {
        NAMES[usize::from(n)] != "default"
    })
}

fn vis() -> impl Strategy<Value = Vis> {
    prop_oneof![
        8 => Just(Vis::None),
        4 => Just(Vis::Pub),
        2 => Just(Vis::PubCrate),
        1 => Just(Vis::PubSuper),
        1 => Just(Vis::PubSelf),
        2 => Just(Vis::PubInPath),
    ]
}

fn ty() -> impl Strategy<Value = Ty> {
    let leaf = prop_oneof![
        4 => (0u8..3, 0..TYPE_NAMES.len() as u8).prop_map(|(p, n)| Ty::Path(p, vec![(n, vec![])])),
        2 => (0..PRIMITIVES.len() as u8).prop_map(Ty::Prim),
        1 => prop::collection::vec(0..TRAIT_NAMES.len() as u8, 1..3).prop_map(Ty::Dyn),
    ];
    leaf.prop_recursive(3, 12, 3, |inner| {
        let arg = prop_oneof![
            6 => inner.clone(),
            1 => (0u8..3).prop_map(Ty::Const),
            1 => Just(Ty::Lifetime),
        ];
        prop_oneof![
            4 => (0u8..3, prop::collection::vec((0..TYPE_NAMES.len() as u8, prop::collection::vec(arg, 0..3)), 1..3))
                .prop_map(|(p, segs)| Ty::Path(p, segs)),
            1 => (inner.clone(), 0..TRAIT_NAMES.len() as u8, 0..ASSOC_NAMES.len() as u8)
                .prop_map(|(t, tr, n)| Ty::QPath(Box::new(t), tr, n)),
            1 => (any::<bool>(), any::<bool>(), inner.clone()).prop_map(|(l, m, t)| Ty::Ref(l, m, Box::new(t))),
            1 => prop::collection::vec(inner.clone(), 0..3).prop_map(Ty::Tuple),
            1 => (inner.clone(), 1u8..9).prop_map(|(t, n)| Ty::Array(Box::new(t), n)),
            1 => inner.clone().prop_map(|t| Ty::Slice(Box::new(t))),
            1 => (
                prop_oneof![15 => 0u8..4, 1 => Just(4u8)],
                prop::collection::vec(inner.clone(), 0..2),
                prop::option::of(inner),
            )
                .prop_map(|(q, a, r)| Ty::FnPtr(q, a, r.map(Box::new))),
        ]
    })
}

/// A trait path: a path type whose last segment is a trait name.
fn trait_path() -> impl Strategy<Value = Ty> {
    (
        0u8..3,
        0..TRAIT_NAMES.len() as u8,
        prop::collection::vec(ty(), 0..2),
    )
        .prop_map(|(p, n, args)| {
            // Trait names are marked by an index past the type names.
            Ty::Path(p, vec![(TYPE_NAMES.len() as u8 + n, args)])
        })
}

fn leaf_node() -> impl Strategy<Value = Node> {
    prop_oneof![
        3 => (any::<Style>(), vis(), fn_qual(), name(), 0..SIGS.len() as u8)
            .prop_map(|(s, v, q, n, g)| Node::Fn(s, v, q, n, g, vec![])),
        2 => (any::<Style>(), vis(), name(), 0..STRUCT_FORMS.len() as u8).prop_map(|(s, v, n, k)| Node::Struct(s, v, n, k)),
        1 => (any::<Style>(), vis(), name()).prop_map(|(s, v, n)| Node::Enum(s, v, n)),
        1 => (any::<Style>(), vis(), name()).prop_map(|(s, v, n)| Node::Mod(s, v, n, None)),
        2 => (any::<Style>(), vis(), prop::option::weighted(0.8, const_name())).prop_map(|(s, v, n)| Node::Const(s, v, n, None)),
        1 => (any::<Style>(), vis(), any::<bool>(), name()).prop_map(|(s, v, m, n)| Node::Static(s, v, m, n, None)),
        1 => (any::<Style>(), name(), 0u8..3).prop_map(|(s, n, d)| Node::MacroRules(s, n, d)),
        1 => (
            any::<Style>(),
            any::<bool>(),
            prop::bool::weighted(0.1),
            prop::collection::vec(
                (prop::sample::select(vec![0u8, 0, 0, 1, 2]), any::<bool>(), prop::bool::weighted(0.3), name()),
                0..3,
            ),
        )
            .prop_map(|(s, u, raw, i)| Node::Extern(s, u, raw, i)),
        2 => (0..ITEM_NOISE.len() as u8).prop_map(Node::Noise),
    ]
}

fn stmt(inner: BoxedStrategy<Node>) -> impl Strategy<Value = Stmt> {
    prop_oneof![
        6 => inner.clone().prop_map(Stmt::Item),
        1 => prop::collection::vec(inner.clone(), 0..3).prop_map(Stmt::LetBlock),
        1 => prop::collection::vec(inner.clone(), 0..3).prop_map(Stmt::Closure),
        1 => prop::collection::vec(inner.clone(), 0..2).prop_map(Stmt::Match),
        2 => (0..BLOCKS.len() as u8, prop::collection::vec(inner.clone(), 0..3)).prop_map(|(k, b)| Stmt::Block(k, b)),
        1 => prop::collection::vec(inner.clone(), 0..3).prop_map(Stmt::ArgClosure),
        1 => prop::collection::vec(inner, 0..2).prop_map(Stmt::ArrayLen),
        4 => (0..STMT_NOISE.len() as u8).prop_map(Stmt::Noise),
    ]
}

fn assoc(inner: BoxedStrategy<Node>, trait_body: bool) -> impl Strategy<Value = Assoc> {
    let body = prop::collection::vec(stmt(inner), 0..3);
    let fn_body = if trait_body {
        prop::option::weighted(0.5, body).boxed()
    } else {
        body.prop_map(Some).boxed()
    };
    prop_oneof![
        4 => (any::<Style>(), vis(), 0u8..3, name(), fn_body).prop_map(|(s, v, q, n, b)| Assoc::Fn(s, v, q, n, b)),
        1 => (any::<Style>(), vis(), const_name(), any::<bool>())
            .prop_map(move |(s, v, n, value)| Assoc::Const(s, v, n, value || !trait_body)),
        1 => name().prop_map(Assoc::Type),
    ]
}

/// A node: a leaf, or an item with items inside.
pub fn node() -> impl Strategy<Value = Node> {
    leaf_node().prop_recursive(4, 40, 4, |inner| {
        let inner = inner.boxed();
        let items = prop::collection::vec(inner.clone(), 0..4);
        let stmts = prop::collection::vec(stmt(inner.clone()), 0..4);
        prop_oneof![
            2 => (any::<Style>(), vis(), name(), items.clone()).prop_map(|(s, v, n, b)| Node::Mod(s, v, n, Some(b))),
            3 => (any::<Style>(), vis(), fn_qual(), name(), 0..SIGS.len() as u8, stmts)
                .prop_map(|(s, v, q, n, g, b)| Node::Fn(s, v, q, n, g, b)),
            1 => (
                any::<Style>(),
                vis(),
                any::<bool>(),
                prop::bool::weighted(0.3),
                name(),
                prop::collection::vec(assoc(inner.clone(), true), 0..4),
            )
                .prop_map(|(s, v, u, g, n, b)| Node::Trait(s, v, u, g, n, b)),
            3 => (
                any::<Style>(),
                any::<bool>(),
                any::<bool>(),
                prop::option::of((prop::bool::weighted(0.2), trait_path())),
                ty(),
                any::<bool>(),
                prop::collection::vec(assoc(inner.clone(), false), 0..3),
            )
                .prop_map(|(s, u, g, t, st, w, b)| {
                    // An inherent `impl` whose self type starts with `<` gets generic parameters first, so that
                    // `<` is not read as theirs ([F21 §3.7] step 1; rustc reads `impl <T as Tr>::X` as a qualified
                    // self type, which is no nominal type and so outside the contract). Only a trait `impl` may be
                    // `unsafe`: rustc's parser refuses an inherent one (E0197).
                    let g = g || (t.is_none() && matches!(st, Ty::QPath(..)));
                    let u = u && t.is_some();
                    Node::Impl(s, u, g, t, st, w, b)
                }),
            1 => (any::<Style>(), vis(), prop::option::weighted(0.8, const_name()), items.clone())
                .prop_map(|(s, v, n, b)| Node::Const(s, v, n, Some(b))),
            1 => (any::<Style>(), vis(), any::<bool>(), name(), items)
                .prop_map(|(s, v, m, n, b)| Node::Static(s, v, m, n, Some(b))),
        ]
    })
}

/// A whole source: its items, and whether it starts with a shebang and an inner attribute.
pub fn source() -> impl Strategy<Value = (Vec<Node>, bool, bool)> {
    (
        prop::collection::vec(node(), 0..8),
        prop::bool::weighted(0.1),
        prop::bool::weighted(0.2),
    )
}

/// A token of a type, with its [F21 §3.2] class: `true` for a word, literal or lifetime.
type Tok = (String, bool);

fn word(s: &str) -> Tok {
    (s.to_string(), true)
}

fn punct(s: &str) -> Tok {
    (s.to_string(), false)
}

fn ty_tokens(t: &Ty, out: &mut Vec<Tok>) {
    match t {
        Ty::Prim(k) => out.push(word(PRIMITIVES[usize::from(*k)])),
        Ty::Path(prefix, segs) => {
            match prefix {
                1 => {
                    out.push(word("crate"));
                    out.push(punct("::"));
                }
                2 => out.push(punct("::")),
                _ => {}
            }
            for (i, (n, args)) in segs.iter().enumerate() {
                if i > 0 {
                    out.push(punct("::"));
                }
                let n = usize::from(*n);
                out.push(word(if n < TYPE_NAMES.len() {
                    TYPE_NAMES[n]
                } else {
                    TRAIT_NAMES[n - TYPE_NAMES.len()]
                }));
                if !args.is_empty() {
                    out.push(punct("<"));
                    for (j, a) in args.iter().enumerate() {
                        if j > 0 {
                            out.push(punct(","));
                        }
                        ty_tokens(a, out);
                    }
                    out.push(punct(">"));
                }
            }
        }
        Ty::Ref(lifetime, m, inner) => {
            out.push(punct("&"));
            if *lifetime {
                out.push(word("'a"));
            }
            if *m {
                out.push(word("mut"));
            }
            ty_tokens_no_plus(inner, out);
        }
        Ty::QPath(inner, tr, n) => {
            out.push(punct("<"));
            ty_tokens(inner, out);
            out.extend([
                word("as"),
                word(TRAIT_NAMES[usize::from(*tr)]),
                punct(">"),
                punct("::"),
                word(ASSOC_NAMES[usize::from(*n)]),
            ]);
        }
        Ty::Tuple(items) => {
            out.push(punct("("));
            for (j, a) in items.iter().enumerate() {
                if j > 0 {
                    out.push(punct(","));
                }
                ty_tokens(a, out);
            }
            if items.len() == 1 {
                out.push(punct(","));
            }
            out.push(punct(")"));
        }
        Ty::Array(inner, n) => {
            out.push(punct("["));
            ty_tokens(inner, out);
            out.push(punct(";"));
            out.push(word(&n.to_string()));
            out.push(punct("]"));
        }
        Ty::Slice(inner) => {
            out.push(punct("["));
            ty_tokens(inner, out);
            out.push(punct("]"));
        }
        Ty::FnPtr(q, args, ret) => {
            if *q == 3 {
                out.extend([word("for"), punct("<"), word("'a"), punct(">")]);
            }
            if *q == 1 {
                out.push(word("unsafe"));
            }
            if *q == 2 || *q == 4 {
                out.push(word("extern"));
                out.push(word(if *q == 2 { "\"C\"" } else { "r\"C\"" }));
            }
            out.push(word("fn"));
            out.push(punct("("));
            for (j, a) in args.iter().enumerate() {
                if j > 0 {
                    out.push(punct(","));
                }
                ty_tokens(a, out);
            }
            out.push(punct(")"));
            if let Some(r) = ret {
                out.push(punct("->"));
                ty_tokens_no_plus(r, out);
            }
        }
        Ty::Dyn(traits) => {
            out.push(word("dyn"));
            for (j, n) in traits.iter().enumerate() {
                if j > 0 {
                    out.push(punct("+"));
                }
                out.push(word(TRAIT_NAMES[usize::from(*n)]));
            }
        }
        Ty::Const(k) => {
            out.push(punct("{"));
            match k {
                0 => out.push(word("3")),
                1 => out.extend([word("N"), punct("+"), word("1")]),
                _ => out.extend([
                    word("\"a\nb\""),
                    punct("."),
                    word("len"),
                    punct("("),
                    punct(")"),
                ]),
            }
            out.push(punct("}"));
        }
        Ty::Lifetime => out.push(word("'a")),
    }
}

/// The tokens of a type where rustc refuses a bare bound list — after `&` and as a function pointer's return type
/// (`&dyn A + B` is "ambiguous `+` in a type"): a trait object with several bounds is written in parentheses there,
/// as rustc suggests. Everywhere else (generic arguments, tuples, arrays, slices, parameters, the `T` of a qualified
/// path, a whole self type) rustc and tree-sitter-rust take it bare.
fn ty_tokens_no_plus(t: &Ty, out: &mut Vec<Tok>) {
    if matches!(t, Ty::Dyn(traits) if traits.len() > 1) {
        out.push(punct("("));
        ty_tokens(t, out);
        out.push(punct(")"));
    } else {
        ty_tokens(t, out);
    }
}

/// Whether a type holds a function pointer type with a raw-string ABI (a grammar gap of the oracle).
fn raw_abi(t: &Ty) -> bool {
    match t {
        Ty::FnPtr(q, args, ret) => {
            *q == 4 || args.iter().any(raw_abi) || ret.as_deref().is_some_and(raw_abi)
        }
        Ty::Path(_, segs) => segs.iter().any(|(_, args)| args.iter().any(raw_abi)),
        Ty::Ref(_, _, inner) | Ty::Array(inner, _) | Ty::Slice(inner) | Ty::QPath(inner, ..) => {
            raw_abi(inner)
        }
        Ty::Tuple(items) => items.iter().any(raw_abi),
        Ty::Prim(_) | Ty::Dyn(_) | Ty::Const(_) | Ty::Lifetime => false,
    }
}

/// [F21 §3.2]'s spelling of a token list: one SP exactly between two words, literals or lifetimes; a line break inside a
/// literal written `\n`.
fn canon(toks: &[Tok]) -> String {
    let mut s = String::new();
    for (i, (t, w)) in toks.iter().enumerate() {
        if i > 0 && *w && toks[i - 1].1 {
            s.push(' ');
        }
        s.push_str(&t.replace('\n', "\\n"));
    }
    s
}

/// The renderer: the text so far, its current line, the expected rows and the open items.
pub struct Out {
    text: String,
    line: u64,
    rows: Vec<Row>,
    open: Vec<usize>,
    depth: usize,
    /// A counter that varies the separators between tokens.
    tick: usize,
    /// Whether the source uses a grammar gap of the oracle.
    gap: bool,
}

impl Out {
    fn new() -> Out {
        Out {
            text: String::new(),
            line: 1,
            rows: Vec::new(),
            open: Vec::new(),
            depth: 0,
            tick: 0,
            gap: false,
        }
    }

    fn push(&mut self, s: &str) {
        self.line += s.bytes().filter(|&b| b == b'\n').count() as u64;
        self.text.push_str(s);
    }

    fn newline(&mut self) {
        self.push("\n");
        for _ in 0..self.depth {
            self.text.push_str("    ");
        }
    }

    /// Opens an item whose first token is written next.
    fn begin(&mut self, skind: u8, name: &str, qual: &str) {
        let parent = self.open.last().copied();
        self.rows
            .push(Row::new(skind, name, qual, self.line, self.line, parent));
        self.open.push(self.rows.len() - 1);
    }

    /// Closes the innermost item after its final token.
    fn end(&mut self) {
        let i = self.open.pop().expect("an open item");
        self.rows[i].end = self.line;
    }

    /// Writes type tokens with varied separators: nothing where [F21 §3.2] writes nothing, else a space, two, a tab,
    /// a line break or a comment.
    fn tokens(&mut self, toks: &[Tok]) {
        for (i, (t, w)) in toks.iter().enumerate() {
            if i > 0 {
                self.tick += 1;
                let words = *w && toks[i - 1].1;
                let both_amp = t == "&" && toks[i - 1].0 == "&";
                let sep = match self.tick % 7 {
                    0 | 3 if !words && !both_amp => "",
                    1 => "  ",
                    2 => "\n        ",
                    4 => " /* c */ ",
                    5 => "\t",
                    _ => " ",
                };
                self.push(sep);
            }
            self.push(t);
        }
    }

    /// Writes an item's doc comment (bit 0) and attribute (bit 1), each chosen by bits 6–7.
    fn prelude(&mut self, style: Style) {
        if style & 1 != 0 {
            match style >> 6 {
                1 => self.push("/** Block doc with } and { on the item's line. */ "),
                3 => {
                    self.push("/**\n * Block doc with `mod fake {`.\n */");
                    self.newline();
                }
                _ => {
                    self.push("/// Doc with `fn fake() {}`.");
                    self.newline();
                }
            }
        }
        if style & 2 != 0 {
            self.push(match style >> 6 {
                0 => "#[inline]",
                1 => "#[derive(Debug)]",
                2 => "#[cfg(test)]",
                _ => "#[doc = \"}\"]",
            });
            if style & 4 != 0 {
                self.push(" ");
            } else {
                self.newline();
            }
        }
    }

    /// Writes the visibility (the item's first token when present) and the space or line break after it.
    fn vis(&mut self, style: Style, v: Vis) {
        if !matches!(v, Vis::None) {
            self.push(v.text());
            if style & 8 != 0 {
                self.newline();
            } else {
                self.push(" ");
            }
        }
    }

    fn keyword_gap(&mut self, style: Style) {
        self.push(if style & 32 != 0 { " /* k */ " } else { " " });
    }

    fn block_open(&mut self) {
        self.push("{");
        self.depth += 1;
    }

    fn block_close(&mut self) {
        self.depth -= 1;
        self.newline();
        self.push("}");
    }

    fn nodes(&mut self, nodes: &[Node]) {
        for (i, n) in nodes.iter().enumerate() {
            // A line comment before would swallow the item.
            let after_comment = self
                .text
                .rsplit('\n')
                .next()
                .is_some_and(|l| l.contains("//"));
            let same_line = i > 0 && !after_comment && style_of(n).is_some_and(|s| s & 16 != 0);
            if same_line {
                self.push(" ");
            } else {
                self.newline();
            }
            self.node(n);
        }
    }

    fn node(&mut self, n: &Node) {
        match n {
            Node::Noise(k) => {
                let text = ITEM_NOISE[usize::from(*k)];
                self.push(text);
            }
            Node::Mod(s, v, name, body) => {
                self.prelude(*s);
                self.begin(1, NAMES[usize::from(*name)], "");
                self.vis(*s, *v);
                self.push("mod");
                self.keyword_gap(*s);
                self.push(NAMES[usize::from(*name)]);
                match body {
                    None => self.push(";"),
                    Some(items) => {
                        self.push(" ");
                        self.block_open();
                        self.nodes(items);
                        self.block_close();
                    }
                }
                self.end();
            }
            Node::Fn(s, v, q, name, sig, body) => {
                self.prelude(*s);
                self.begin(3, NAMES[usize::from(*name)], "");
                self.vis(*s, *v);
                let (qual, gap) = FN_QUALS[usize::from(*q)];
                self.gap |= gap;
                self.push(qual);
                self.push("fn");
                self.keyword_gap(*s);
                self.push(NAMES[usize::from(*name)]);
                self.push(SIGS[usize::from(*sig)]);
                self.push(" ");
                self.stmts_block(body);
                self.end();
            }
            Node::Struct(s, v, name, kind) => {
                self.prelude(*s);
                self.begin(4, NAMES[usize::from(*name)], "");
                self.vis(*s, *v);
                self.push("struct");
                self.keyword_gap(*s);
                self.push(NAMES[usize::from(*name)]);
                self.push(STRUCT_FORMS[usize::from(*kind)]);
                self.end();
            }
            Node::Enum(s, v, name) => {
                self.prelude(*s);
                self.begin(5, NAMES[usize::from(*name)], "");
                self.vis(*s, *v);
                self.push("enum");
                self.keyword_gap(*s);
                self.push(NAMES[usize::from(*name)]);
                self.push(" { A { x: u8 }, B(u8), C = 3 }");
                self.end();
            }
            Node::Trait(s, v, unsafe_, generics, name, items) => {
                self.prelude(*s);
                self.begin(6, NAMES[usize::from(*name)], "");
                self.vis(*s, *v);
                if *unsafe_ {
                    self.push("unsafe ");
                }
                self.push("trait");
                self.keyword_gap(*s);
                self.push(NAMES[usize::from(*name)]);
                if *generics {
                    // A braced default after `=` ([F21 §3.6] step 2), with a brace inside a comment in it.
                    self.push("<const N: usize = { 1 /* } */ + 2 }>");
                }
                self.push(": Sized where Self: Clone ");
                self.block_open();
                self.assocs(items, true, false);
                self.block_close();
                self.end();
            }
            Node::Impl(s, unsafe_, generics, tr, self_ty, where_, items) => {
                let mut self_toks = Vec::new();
                ty_tokens(self_ty, &mut self_toks);
                let mut trait_toks = Vec::new();
                if let Some((negative, t)) = tr {
                    if *negative {
                        trait_toks.push(punct("!"));
                    }
                    ty_tokens(t, &mut trait_toks);
                }
                self.prelude(*s);
                self.gap |= raw_abi(self_ty) || tr.as_ref().is_some_and(|(_, t)| raw_abi(t));
                self.begin(2, &canon(&self_toks), &canon(&trait_toks));
                if *unsafe_ {
                    self.push("unsafe ");
                }
                self.push("impl");
                if *generics {
                    self.push("<'a, T: Clone, const N: usize>");
                }
                self.keyword_gap(*s);
                if tr.is_some() {
                    self.tokens(&trait_toks);
                    self.push(" for ");
                }
                self.tokens(&self_toks);
                if *where_ {
                    self.push(" where T: Clone,");
                }
                self.push(" ");
                self.block_open();
                self.assocs(items, false, tr.is_none());
                self.block_close();
                self.end();
            }
            Node::Const(s, v, name, init) => {
                let n = name.map_or("_", |n| NAMES[usize::from(n)]);
                self.prelude(*s);
                self.begin(7, n, "");
                self.vis(*s, *v);
                self.push("const");
                self.keyword_gap(*s);
                self.push(n);
                match init {
                    None => self.push(": &str = \"a; {\";"),
                    Some(items) => {
                        self.push(": u8 = ");
                        self.block_open();
                        self.nodes(items);
                        self.newline();
                        self.push("1");
                        self.block_close();
                        self.push(";");
                    }
                }
                self.end();
            }
            Node::Static(s, v, m, name, init) => {
                self.prelude(*s);
                self.begin(8, NAMES[usize::from(*name)], "");
                self.vis(*s, *v);
                self.push("static");
                self.keyword_gap(*s);
                if *m {
                    self.push("mut ");
                }
                self.push(NAMES[usize::from(*name)]);
                match init {
                    None => self.push(": u8 = 1;"),
                    Some(items) => {
                        self.push(": u8 = ");
                        self.block_open();
                        self.nodes(items);
                        self.newline();
                        self.push("1");
                        self.block_close();
                        self.push(";");
                    }
                }
                self.end();
            }
            Node::MacroRules(s, name, delim) => {
                self.prelude(*s);
                self.begin(9, NAMES[usize::from(*name)], "");
                self.push("macro_rules! ");
                self.push(NAMES[usize::from(*name)]);
                self.push(match delim {
                    0 => " {\n    ($x:expr) => { fn fake() {} };\n    () => { mod m { } };\n}",
                    1 => " (\n    () => { struct Fake; }\n);",
                    _ => " [ () => { impl X for Y {} } ];",
                });
                self.end();
            }
            Node::Extern(s, unsafe_, raw, items) => {
                self.prelude(*s & !2);
                if *unsafe_ {
                    self.push("unsafe ");
                }
                self.gap |= *raw;
                self.push(EXTERN_ABIS[usize::from(*raw)]);
                self.block_open();
                for (q, is_fn, split, name) in items {
                    self.newline();
                    let n = NAMES[usize::from(*name)];
                    let q = if *unsafe_ {
                        EXTERN_QUALS[usize::from(*q)]
                    } else {
                        ""
                    };
                    self.gap |= !q.is_empty();
                    self.begin(if *is_fn { 3 } else { 8 }, n, "");
                    // The item's first token on a line of its own, when `split`: its visibility, or its qualifier.
                    let first = |out: &mut Out, token: &str| {
                        out.push(token);
                        if *split {
                            out.newline();
                        } else {
                            out.push(" ");
                        }
                    };
                    if *is_fn {
                        first(self, "pub");
                        self.push(q);
                        self.push("fn ");
                        self.push(n);
                        self.push("(x: u8) -> u8;");
                    } else {
                        if !q.is_empty() {
                            first(self, q.trim_end());
                        }
                        self.push("static ");
                        self.push(n);
                        self.push(": u8;");
                    }
                    self.end();
                }
                self.block_close();
            }
        }
    }

    fn stmts_block(&mut self, stmts: &[Stmt]) {
        self.block_open();
        for st in stmts {
            self.newline();
            match st {
                Stmt::Item(n) => self.node(n),
                Stmt::Noise(k) => self.push(STMT_NOISE[usize::from(*k)]),
                Stmt::LetBlock(items) => {
                    self.push("let _v = ");
                    self.block_open();
                    self.nodes(items);
                    self.newline();
                    self.push("1");
                    self.block_close();
                    self.push(";");
                }
                Stmt::Closure(items) => {
                    self.push("let _c = || ");
                    self.block_open();
                    self.nodes(items);
                    self.block_close();
                    self.push(";");
                }
                Stmt::Match(items) => {
                    self.push("match 1 { _ => ");
                    self.block_open();
                    self.nodes(items);
                    self.block_close();
                    self.push(" }");
                }
                Stmt::Block(k, items) => {
                    let (open, close) = BLOCKS[usize::from(*k)];
                    self.push(open);
                    self.block_open();
                    self.nodes(items);
                    if open.starts_with("loop") {
                        self.newline();
                        self.push("break;");
                    }
                    self.block_close();
                    self.push(close);
                }
                Stmt::ArgClosure(items) => {
                    self.push("drop(|| ");
                    self.block_open();
                    self.nodes(items);
                    self.block_close();
                    self.push(");");
                }
                Stmt::ArrayLen(items) => {
                    self.push("let _a = [0u8; ");
                    self.block_open();
                    self.nodes(items);
                    self.newline();
                    self.push("3");
                    self.block_close();
                    self.push("];");
                }
            }
        }
        self.block_close();
    }

    /// The items of a trait body (`in_trait`) or an `impl` body; `inherent` for an `impl` without a trait, the only
    /// body whose items may carry a visibility.
    fn assocs(&mut self, items: &[Assoc], in_trait: bool, inherent: bool) {
        for a in items {
            self.newline();
            match a {
                Assoc::Fn(s, v, q, name, body) => {
                    let n = NAMES[usize::from(*name)];
                    self.prelude(*s);
                    self.begin(3, n, "");
                    if inherent {
                        self.vis(*s, *v);
                    }
                    self.push(["", "unsafe ", "async "][usize::from(*q)]);
                    self.push("fn");
                    self.keyword_gap(*s);
                    self.push(n);
                    self.push("(&self)");
                    match body {
                        None => self.push(";"),
                        Some(b) => {
                            self.push(" ");
                            self.stmts_block(b);
                        }
                    }
                    self.end();
                }
                Assoc::Const(s, v, name, value) => {
                    let n = NAMES[usize::from(*name)];
                    self.prelude(*s);
                    self.begin(7, n, "");
                    if inherent {
                        self.vis(*s, *v);
                    }
                    self.push("const");
                    self.keyword_gap(*s);
                    self.push(n);
                    self.push(if *value { ": u8 = 1;" } else { ": u8;" });
                    self.end();
                }
                Assoc::Type(name) => {
                    self.push("type ");
                    self.push(NAMES[usize::from(*name)]);
                    self.push(if in_trait { ": Clone;" } else { " = u8;" });
                }
            }
        }
    }
}

fn style_of(n: &Node) -> Option<Style> {
    match n {
        Node::Mod(s, ..)
        | Node::Fn(s, ..)
        | Node::Struct(s, ..)
        | Node::Enum(s, ..)
        | Node::Trait(s, ..)
        | Node::Impl(s, ..)
        | Node::Const(s, ..)
        | Node::Static(s, ..)
        | Node::MacroRules(s, ..)
        | Node::Extern(s, ..) => Some(*s),
        Node::Noise(_) => None,
    }
}

/// Renders a source (with LF line ends) and its items by construction, and whether it uses a grammar gap of the
/// oracle (module documentation). `inner_attr` writes an inner block doc comment and an inner attribute after the
/// shebang, where rustc allows them.
pub fn render(nodes: &[Node], shebang: bool, inner_attr: bool) -> (String, Vec<Row>, bool) {
    let mut out = Out::new();
    if shebang {
        out.push("#!/usr/bin/env run-cargo-script");
        out.newline();
    }
    if inner_attr {
        out.push("/*! Inner doc with `mod fake {`. */");
        out.newline();
        out.push("#![allow(dead_code)]");
        out.newline();
    }
    out.nodes(nodes);
    out.push("\n");
    (out.text, out.rows, out.gap)
}
