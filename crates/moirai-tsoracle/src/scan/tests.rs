//! Synthetic Rust inputs for the item rules (crate documentation, "Items").

use super::{Kind, Oracle, Scan, append_canon, item_name};

/// One reported item as a comparable tuple: (kind, name, qual, start, end, parent).
type Row = (&'static str, String, String, usize, usize, Option<usize>);

fn scan_bytes(src: &[u8]) -> Scan {
    let mut oracle = Oracle::new().expect("grammar loads");
    let mut scan = Scan::new();
    oracle.scan(src, &mut scan).expect("parses");
    scan
}

fn rows_of(scan: &Scan) -> Vec<Row> {
    scan.items()
        .map(|i| {
            (
                i.kind.name(),
                i.name.to_owned(),
                i.qual.to_owned(),
                i.start,
                i.end,
                i.parent,
            )
        })
        .collect()
}

/// The rows of a source that must parse without errors; every item of such a source is `ok`.
fn rows(src: &str) -> Vec<Row> {
    let scan = scan_bytes(src.as_bytes());
    assert_eq!(scan.errors(), 0, "unexpected syntax errors in {src:?}");
    assert!(scan.items().all(|i| i.ok), "an item is not ok in {src:?}");
    rows_of(&scan)
}

/// (name, parent, ok) of every item of a source, errors or not.
fn claims(src: &[u8]) -> Vec<(String, Option<usize>, bool)> {
    scan_bytes(src)
        .items()
        .map(|i| (i.name.to_owned(), i.parent, i.ok))
        .collect()
}

fn claim(name: &str, parent: Option<usize>, ok: bool) -> (String, Option<usize>, bool) {
    (name.to_owned(), parent, ok)
}

fn row(
    kind: &'static str,
    name: &str,
    qual: &str,
    start: usize,
    end: usize,
    parent: Option<usize>,
) -> Row {
    (kind, name.to_owned(), qual.to_owned(), start, end, parent)
}

/// The (kind, name, qual) triples only.
fn segments(src: &str) -> Vec<(&'static str, String, String)> {
    rows(src)
        .into_iter()
        .map(|(k, n, q, ..)| (k, n, q))
        .collect()
}

fn seg(kind: &'static str, name: &str, qual: &str) -> (&'static str, String, String) {
    (kind, name.to_owned(), qual.to_owned())
}

#[test]
fn kinds_have_the_f08_skind_codes() {
    let codes: Vec<u8> = Kind::ALL.iter().map(|k| k.skind()).collect();
    assert_eq!(codes, [1, 2, 3, 4, 5, 6, 7, 8, 9]);
    let names: Vec<&str> = Kind::ALL.iter().map(|k| k.name()).collect();
    assert_eq!(
        names,
        [
            "mod",
            "impl",
            "fn",
            "struct",
            "enum",
            "trait",
            "const",
            "static",
            "macro_rules"
        ]
    );
}

#[test]
fn every_kind_is_reported() {
    let src = "\
mod m {}
impl S {}
fn f() {}
struct S;
enum E { A }
trait T {}
const C: u8 = 1;
static G: u8 = 2;
macro_rules! mac { () => {} }
";
    assert_eq!(
        rows(src),
        [
            row("mod", "m", "", 1, 1, None),
            row("impl", "S", "", 2, 2, None),
            row("fn", "f", "", 3, 3, None),
            row("struct", "S", "", 4, 4, None),
            row("enum", "E", "", 5, 5, None),
            row("trait", "T", "", 6, 6, None),
            row("const", "C", "", 7, 7, None),
            row("static", "G", "", 8, 8, None),
            row("macro_rules", "mac", "", 9, 9, None),
        ]
    );
}

#[test]
fn other_declarations_are_not_items() {
    let src = "\
use std::fmt;
extern crate alloc;
type A<T> = Vec<T>;
union U { a: u8, b: u16 }
extern \"C\" {}
fn f<const N: usize>() -> impl Sized { let c = const { 1 }; let k = || 2; }
trait T { type Assoc; }
enum E { Variant { field: u8 } }
";
    assert_eq!(
        segments(src),
        [
            seg("fn", "f", ""),
            seg("trait", "T", ""),
            seg("enum", "E", "")
        ]
    );
}

#[test]
fn qualifiers_and_modifiers_do_not_change_the_kind() {
    let src = "\
pub const fn a() {}
pub(crate) async unsafe fn b() {}
extern \"C\" fn c() {}
pub(in crate::x) static mut D: u8 = 0;
unsafe trait E {}
const _: () = ();
";
    assert_eq!(
        segments(src),
        [
            seg("fn", "a", ""),
            seg("fn", "b", ""),
            seg("fn", "c", ""),
            seg("static", "D", ""),
            seg("trait", "E", ""),
            seg("const", "_", ""),
        ]
    );
}

#[test]
fn impl_names_are_the_canonical_type_and_trait() {
    let src = "\
impl<T> Pool<T> {}
impl<T: Clone> From<T> for Wrap<T> {}
impl<T> !Send for Raw<T> {}
impl ! /* comment */ Sync for Raw2 {}
unsafe impl<'a> Sync for &'a   mut [u8] {}
impl Tr for dyn Fn(u8) -> u8 + Send {}
impl Tr for extern \"C\" fn() {}
impl Tr for (A, B) {}
impl<T: Iterator> Tr for <T as Iterator>::Item {}
impl crate::a::Tr<u8> for super::b::C<{ 4 }> {}
impl<T> Display
    for Multi<
        T, // why
        u8,
    >
where
    T: Clone,
{
}
";
    assert_eq!(
        segments(src),
        [
            seg("impl", "Pool<T>", ""),
            seg("impl", "Wrap<T>", "From<T>"),
            seg("impl", "Raw<T>", "!Send"),
            seg("impl", "Raw2", "!Sync"),
            seg("impl", "&'a mut[u8]", "Sync"),
            seg("impl", "dyn Fn(u8)->u8+Send", "Tr"),
            seg("impl", "extern \"C\" fn()", "Tr"),
            seg("impl", "(A,B)", "Tr"),
            seg("impl", "<T as Iterator>::Item", "Tr"),
            seg("impl", "super::b::C<{4}>", "crate::a::Tr<u8>"),
            seg("impl", "Multi<T,u8,>", "Display"),
        ]
    );
}

#[test]
fn identifier_names_are_kept_as_written() {
    let src = "fn r#match() {}\nstruct Größe;\nmod 名前 {}\nfn _private() {}\n";
    assert_eq!(
        segments(src),
        [
            seg("fn", "r#match", ""),
            seg("struct", "Größe", ""),
            seg("mod", "名前", ""),
            seg("fn", "_private", "")
        ]
    );
}

#[test]
fn spans_start_at_the_first_token_and_end_at_the_last_byte() {
    let src = "\
/// Doc comment.
#[derive(Debug)]
#[cfg(test)]
pub struct S {
    x: u8,
}

// A plain comment.
#[inline]
pub(crate)
unsafe fn f(
    a: u8,
) -> u8 {
    a
}

/** Block doc. */
mod m;

#[macro_export]
macro_rules! mac (
    () => {}
);
";
    assert_eq!(
        rows(src),
        [
            row("struct", "S", "", 4, 6, None),
            row("fn", "f", "", 10, 15, None),
            row("mod", "m", "", 18, 18, None),
            row("macro_rules", "mac", "", 21, 23, None),
        ]
    );
}

#[test]
fn items_nest_at_any_depth_with_the_nearest_enclosing_item_as_parent() {
    let src = "\
mod a {
    impl Tr for S {
        fn f() {
            struct Local;
            impl Local { fn g(&self) {} }
            let _x = { enum E { A } 1 };
        }
    }
    extern \"C\" {
        fn ext();
        static EXT: u8;
    }
    trait T {
        const K: u8;
        fn decl(&self);
        fn def(&self) { fn inner() {} }
    }
    const C: () = { fn in_const() {} };
}
fn top() {}
";
    assert_eq!(
        rows(src),
        [
            row("mod", "a", "", 1, 19, None),
            row("impl", "S", "Tr", 2, 8, Some(0)),
            row("fn", "f", "", 3, 7, Some(1)),
            row("struct", "Local", "", 4, 4, Some(2)),
            row("impl", "Local", "", 5, 5, Some(2)),
            row("fn", "g", "", 5, 5, Some(4)),
            row("enum", "E", "", 6, 6, Some(2)),
            row("fn", "ext", "", 10, 10, Some(0)),
            row("static", "EXT", "", 11, 11, Some(0)),
            row("trait", "T", "", 13, 17, Some(0)),
            row("const", "K", "", 14, 14, Some(9)),
            row("fn", "decl", "", 15, 15, Some(9)),
            row("fn", "def", "", 16, 16, Some(9)),
            row("fn", "inner", "", 16, 16, Some(12)),
            row("const", "C", "", 18, 18, Some(0)),
            row("fn", "in_const", "", 18, 18, Some(14)),
            row("fn", "top", "", 20, 20, None),
        ]
    );
}

/// Siblings that touch (no whitespace between them) are not each other's parent.
#[test]
fn adjacent_siblings_are_not_nested() {
    assert_eq!(
        rows("mod a{fn f(){}}fn g(){}struct S;"),
        [
            row("mod", "a", "", 1, 1, None),
            row("fn", "f", "", 1, 1, Some(0)),
            row("fn", "g", "", 1, 1, None),
            row("struct", "S", "", 1, 1, None),
        ]
    );
}

/// An item visited after a deeper, already closed item finds its real parent, not the closed item.
#[test]
fn parents_follow_the_tree_not_the_visit_order() {
    let src = "\
fn outer() {
    let _ = { fn deep() {} };
    fn shallow() {}
}
";
    assert_eq!(
        rows(src),
        [
            row("fn", "outer", "", 1, 4, None),
            row("fn", "deep", "", 2, 2, Some(0)),
            row("fn", "shallow", "", 3, 3, Some(0)),
        ]
    );
}

#[test]
fn macro_bodies_are_token_trees() {
    let src = "\
macro_rules! gen {
    ($n:ident) => { fn $n() {} struct Inside; };
}
thread_local! { static TL: u8 = 0; }
lazy_static::lazy_static! { static ref X: u8 = 0; }
gen!(made);
fn after() {}
";
    assert_eq!(
        rows(src),
        [
            row("macro_rules", "gen", "", 1, 3, None),
            row("fn", "after", "", 7, 7, None)
        ]
    );
}

#[test]
fn crlf_and_a_bom_change_no_line_number() {
    let lf = "fn a() {}\n\nimpl S {\n    fn b() {}\n}\n";
    let crlf = lf.replace('\n', "\r\n");
    let mut bom_crlf = b"\xEF\xBB\xBF".to_vec();
    bom_crlf.extend_from_slice(crlf.as_bytes());
    let expected = rows(lf);
    assert_eq!(
        expected,
        [
            row("fn", "a", "", 1, 1, None),
            row("impl", "S", "", 3, 5, None),
            row("fn", "b", "", 4, 4, Some(1))
        ]
    );
    for src in [crlf.as_bytes(), bom_crlf.as_slice()] {
        let scan = scan_bytes(src);
        assert_eq!(scan.errors(), 0);
        assert_eq!(rows_of(&scan), expected);
    }
}

#[test]
fn a_last_line_without_terminator_counts() {
    assert_eq!(rows("fn a() {\n}"), [row("fn", "a", "", 1, 2, None)]);
}

#[test]
fn syntax_errors_are_counted() {
    for src in [
        "fn () {}",
        "mod ;",
        "impl {}",
        "struct {}",
        "fn a( {",
        "impl<T> const Tr for S {}",
    ] {
        assert!(
            scan_bytes(src.as_bytes()).errors() > 0,
            "no error counted for {src:?}"
        );
    }
    assert_eq!(scan_bytes(b"fn ok() {}").errors(), 0);
    assert_eq!(scan_bytes(b"").errors(), 0);
}

/// Rule 3: error recovery leaves `impl<T> { … }` an `impl_item` whose type is a MISSING node. The item is not
/// reported, the items inside it go to the next enclosing item, and rule 8 takes them out of the claim; a sibling
/// the error does not touch stays `ok`.
#[test]
fn an_item_without_a_name_is_skipped_and_its_items_move_up() {
    let src = b"mod m {\n    impl<T> {\n        fn f() {}\n    }\n    fn g() {}\n}\n";
    let scan = scan_bytes(src);
    assert!(scan.errors() > 0);
    assert_eq!(
        rows_of(&scan),
        [
            row("mod", "m", "", 1, 6, None),
            row("fn", "f", "", 3, 3, Some(0)),
            row("fn", "g", "", 5, 5, Some(0)),
        ]
    );
    assert_eq!(
        claims(src),
        [
            claim("m", None, false),
            claim("f", Some(0), false),
            claim("g", Some(0), true)
        ]
    );
    // At top level the items inside go to no parent.
    assert_eq!(claims(b"impl<T> { fn f() {} }"), [claim("f", None, false)]);
}

/// The helper behind rule 3's check: a name range that holds only a comment or whitespace has an empty canonical
/// name, which is not appended.
#[test]
fn item_name_refuses_an_empty_canonical_name() {
    let mut text = String::from("x");
    assert_eq!(item_name(b"/* c */", 0..7, &mut text), None);
    assert_eq!(item_name(b"a \n\t b", 1..5, &mut text), None);
    assert_eq!(item_name(b"", 0..0, &mut text), None);
    assert_eq!(text, "x");
    assert_eq!(item_name(b"fn f()", 3..4, &mut text), Some(1..2));
    assert_eq!(text, "xf");
}

/// Rule 8: a syntax error that leaves brackets balanced takes out of the claim only the items whose node contains it,
/// the items inside an ERROR node, the items inside an item whose name or trait contains it, and an item it stands
/// directly before on an earlier line. Every other item of the file stays `ok`.
#[test]
fn errors_exclude_only_the_items_they_touch() {
    // An error inside a function's parameters: only that function.
    assert_eq!(
        claims(b"fn a() {}\nfn f<'a>(x: Box<dyn 'a + Send>) {}\nfn b() {}\n"),
        [
            claim("a", None, true),
            claim("f", None, false),
            claim("b", None, true)
        ]
    );
    // A MISSING `;`: the module and the `const` contain it; `h` and `z` are clean.
    assert_eq!(
        claims(b"mod m {\n    const C: u8 = 1\n    fn h() {}\n}\nfn z() {}\n"),
        [
            claim("m", None, false),
            claim("C", Some(0), false),
            claim("h", Some(0), true),
            claim("z", None, true)
        ]
    );
    // A raw-string ABI (a grammar gap) whose ERROR node pairs its brackets: the items after it keep their places.
    assert_eq!(
        claims(b"fn a() {\n    extern r#\"C\"# {\n    }\n    fn b() {}\n}\nfn c() {}\n"),
        [
            claim("a", None, false),
            claim("b", Some(0), true),
            claim("c", None, true)
        ]
    );
    // An error in an `impl`'s where clause, outside its name and trait: the methods stay in the claim.
    assert_eq!(
        claims(b"impl<C> Foo for Bar<C> where C: , {\n    fn f() {}\n}\n"),
        [claim("Bar<C>", None, false), claim("f", Some(0), true)]
    );
    // An error in an `impl`'s type: the name path of every item inside is suspect.
    assert_eq!(
        claims(b"impl Tr for Foo<{ N::<-1>() }> {\n    fn f() {}\n}\nfn g() {}\n"),
        [
            claim("Foo<{N::<-1>()}>", None, false),
            claim("f", Some(0), false),
            claim("g", None, true)
        ]
    );
    // An item inside an ERROR node: tree-sitter could not build the enclosing item at all.
    assert_eq!(
        claims(b"fn g() {}\nimpl Tr for Foo<~> { fn f() {} }\n"),
        [claim("g", None, true), claim("f", None, false)]
    );
    // An error on an earlier line directly before an item may hold its qualifier (a grammar gap): the item's start
    // line is suspect. On the item's own line it moves nothing.
    let gap = b"unsafe extern \"C\" {\n    unsafe\n    static U: u8;\n    safe static S: u8;\n}\nfn after() {}\n";
    assert_eq!(
        claims(gap),
        [
            claim("U", None, false),
            claim("S", None, true),
            claim("after", None, true)
        ]
    );
    assert_eq!(
        rows_of(&scan_bytes(gap))[..2],
        [
            row("static", "U", "", 3, 3, None),
            row("static", "S", "", 4, 4, None)
        ]
    );
    // A comment between the error and the item changes nothing.
    assert_eq!(
        claims(b"unsafe extern \"C\" {\n    unsafe // c\n    static U: u8;\n}\n"),
        [claim("U", None, false)]
    );
}

/// Rule 8: a syntax error that leaves brackets unbalanced — a MISSING bracket, or an ERROR node whose brackets do not
/// pair up — takes every later item of the file out of the claim, because error recovery closes the enclosing nodes
/// at other brackets than the source's.
#[test]
fn an_error_that_unbalances_brackets_excludes_every_later_item() {
    // A MISSING `)` and a MISSING `}`: by the source's brackets `h` lies inside `g`'s parameter list, where
    // tree-sitter does not put it. `f`, before the errors, stays `ok`.
    assert_eq!(
        claims(b"mod a { fn f() {} fn g( { } fn h() {}"),
        [
            claim("a", None, false),
            claim("f", Some(0), true),
            claim("g", Some(0), false),
            claim("h", Some(0), false)
        ]
    );
    // An unclosed `(` in a statement: by the source's brackets `g` lies inside it.
    assert_eq!(
        claims(b"fn f() { let x = (; fn g() {} }"),
        [claim("f", None, false), claim("g", Some(0), false)]
    );
    // A stray `}`: every item after it.
    assert_eq!(
        claims(b"fn a() {}\n}\nfn b() {}\nfn c() {}\n"),
        [
            claim("a", None, true),
            claim("b", None, false),
            claim("c", None, false)
        ]
    );
    // Raw-string ABIs (a grammar gap; found by moirai-replay's generated sources) whose ERROR nodes keep a block's
    // `{`: the inner `impl` ends at line 10, not 13, and its second method (line 11) lands in the outer function. The
    // item before the errors stays `ok`; every item after them is out of the claim, the misplaced method included.
    let src = b"impl S {\n    fn a(&self) {\n        loop {\n            impl S {\n                fn a(&self) {\n                    fn a() {\n                    }\n                    extern r#\"C\"# {\n                    }\n                }\n                fn a(&self) {\n                }\n            }\n            break;\n        }\n    }\n    fn a(&self) {\n        extern r\"C\" fn a() {\n        }\n    }\n}\nfn after() {}\n";
    let scan = scan_bytes(src);
    let rows = rows_of(&scan);
    let oks: Vec<bool> = scan.items().map(|i| i.ok).collect();
    let at = |line: usize| {
        rows.iter()
            .position(|r| r.3 == line)
            .unwrap_or_else(|| panic!("no item at line {line}: {rows:?}"))
    };
    assert_eq!(rows[at(11)].5, Some(1), "the method tree-sitter misplaces");
    assert!(oks[at(6)], "the innermost function precedes the errors");
    assert!(
        rows.iter().zip(&oks).all(|(r, ok)| r.3 < 11 || !ok),
        "items after the errors: {rows:?} {oks:?}"
    );
    assert!(!oks[at(22)]);
}

/// Outside literals and comments tree-sitter makes invalid UTF-8 an ERROR node, so the scan goes on; the source
/// also counts one error for not being UTF-8, and none of its items is `ok`.
#[test]
fn invalid_utf8_is_a_syntax_error_not_a_failure() {
    let scan = scan_bytes(b"impl Tr for S<\xFF> {}\nfn ok() {}\n");
    assert!(scan.errors() > 1);
    assert_eq!(rows_of(&scan), [row("fn", "ok", "", 2, 2, None)]);
    assert!(scan.items().all(|i| !i.ok));
}

/// Inside a literal or a comment tree-sitter takes invalid UTF-8 without an ERROR node. The source still counts one
/// error (rustc rejects it), none of its items is `ok`, and a name keeps U+FFFD in place of the invalid bytes.
#[test]
fn invalid_utf8_in_literals_and_comments_counts_one_error() {
    let scan = scan_bytes(b"impl Tr for Foo<{ \"a\xFFb\" }> {}\n");
    assert_eq!(scan.errors(), 1);
    assert_eq!(
        rows_of(&scan),
        [row("impl", "Foo<{\"a\u{FFFD}b\"}>", "Tr", 1, 1, None)]
    );
    assert!(scan.items().all(|i| !i.ok));
    let scan = scan_bytes(b"// \xC3\n/* \xE2\x80 */ fn ok() {}\n");
    assert_eq!(scan.errors(), 1);
    assert_eq!(rows_of(&scan), [row("fn", "ok", "", 2, 2, None)]);
    assert!(scan.items().all(|i| !i.ok));
    // One whole BOM is removed before the check; a truncated one is not UTF-8.
    assert_eq!(scan_bytes(b"\xEF\xBB\xBFfn ok() {}").errors(), 0);
    assert!(scan_bytes(b"\xEF\xBBfn ok() {}").errors() > 0);
}

/// Names are always UTF-8: bytes that are not become U+FFFD, one per maximal invalid subpart, before
/// canonicalisation.
#[test]
fn names_of_invalid_utf8_are_replaced() {
    let mut text = String::from("x");
    let r = append_canon(b"a S< \xFF > b", 2..8, &mut text);
    assert_eq!(r, 1..text.len());
    assert_eq!(&text[r], "S<\u{FFFD}>");
    // `E2 80` is one maximal subpart (a truncated sequence), `FF` another.
    let r = append_canon(b"S<\xE2\x80\xFF>", 0..6, &mut text);
    assert_eq!(&text[r], "S<\u{FFFD}\u{FFFD}>");
}

/// A name whose literal spans lines is still one line, and LF and CRLF sources give the same name.
#[test]
fn names_with_multiline_literals_are_one_line() {
    let lf = "impl Tr for Foo<{ \"a\nb\".len() }> {}\nimpl Tr for Bar<{ r\"x\ny\" }> {}\n";
    let want = [
        row("impl", "Foo<{\"a\\nb\".len()}>", "Tr", 1, 2, None),
        row("impl", "Bar<{r\"x\\ny\"}>", "Tr", 3, 4, None),
    ];
    assert_eq!(rows(lf), want);
    assert_eq!(rows(&lf.replace('\n', "\r\n")), want);
}

/// A raw NUL inside a literal is valid Rust and parses cleanly; the name carries the escape `\0`, never U+0000
/// ([`crate::canon`] step 3; [F08 §5.3]).
#[test]
fn a_nul_in_a_name_is_escaped() {
    assert_eq!(
        rows("impl Tr for Foo<{ \"a\0b\" }> {}\n"),
        [row("impl", "Foo<{\"a\\0b\"}>", "Tr", 1, 1, None)]
    );
}

/// Syntax the pinned grammar does not parse (crate documentation, "Known grammar gaps"): each gives errors > 0 and
/// takes the items it touches out of the claim (rule 8). When a grammar upgrade parses one, this test fails and the
/// list is updated.
#[test]
fn known_grammar_gaps_are_syntax_errors() {
    let gaps: [&[u8]; 23] = [
        // Unstable Rust.
        b"auto trait A {}",
        b"unsafe auto trait A {}",
        b"trait A = B + C;",
        b"macro m() {}",
        b"impl const Tr for S {}",
        b"struct S { a: u8 = 1 }",
        // Stable Rust: `safe` and `unsafe` items of an `unsafe extern` block (Rust 1.82).
        b"unsafe extern \"C\" { safe fn f(); }",
        b"unsafe extern \"C\" { pub safe static S: u8; }",
        b"unsafe extern \"C\" { unsafe static S: u8; }",
        // A lifetime first in a trait object's bounds.
        b"fn f<'a>(x: Box<dyn 'a + Send>) {}",
        // Empty where-bounds.
        b"impl<C> Foo for Bar<C> where C: , {}",
        b"fn f<const N: usize>() where [(); N]: {}",
        // A negative const generic argument.
        b"fn g() { N::<-1>(); }",
        // `~` in a macro's token tree, in an invocation and in a definition.
        b"m! { a ~ b }",
        b"macro_rules! m { () => { ~ } }",
        // A NUL byte in a line comment (block and doc comments take it).
        b"// a\0b\nfn f() {}",
        b"fn f() {\n    // a\0b\n}",
        // A `const` named `default`, at the top level and in an `impl` body.
        b"const default: u8 = 1;",
        b"impl S {\n    const default: u8 = 1;\n}",
        // An ABI written as a raw string: on a function, a block and a function pointer type.
        b"extern r\"C\" fn f() {}",
        b"const unsafe extern r#\"system\"# fn f() {}",
        b"extern r#\"C\"# { fn f(); }",
        b"type F = extern r\"C\" fn(u8);",
    ];
    for src in gaps {
        let scan = scan_bytes(src);
        assert!(
            scan.errors() > 0,
            "{:?} now parses: update the list of grammar gaps",
            String::from_utf8_lossy(src)
        );
    }
    assert_eq!(scan_bytes(b"/* a\0b */ /// c\0d\nfn f() {}").errors(), 0);
}

#[test]
fn a_scan_is_replaced_by_the_next_one() {
    let mut oracle = Oracle::new().expect("grammar loads");
    let mut scan = Scan::new();
    oracle.scan(b"fn (", &mut scan).expect("parses");
    assert!(scan.errors() > 0);
    oracle
        .scan(b"mod a { fn b() {} }", &mut scan)
        .expect("parses");
    assert_eq!(scan.errors(), 0);
    assert_eq!(
        rows_of(&scan),
        [
            row("mod", "a", "", 1, 1, None),
            row("fn", "b", "", 1, 1, Some(0))
        ]
    );
}

/// The crate's own sources: tree-sitter parses them cleanly, every record invariant holds, and known items appear.
#[test]
fn own_sources_scan_cleanly() {
    let mut oracle = Oracle::new().expect("grammar loads");
    let mut scan = Scan::new();
    let mut seen = Vec::new();
    let mut dirs = vec![std::path::PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src"
    ))];
    let mut files = 0;
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(&dir).expect("source directory is readable") {
            let path = entry.expect("directory entry").path();
            if path.is_dir() {
                dirs.push(path);
                continue;
            }
            if path.extension().is_none_or(|e| e != "rs") {
                continue;
            }
            files += 1;
            let src = std::fs::read(&path).expect("source is readable");
            oracle.scan(&src, &mut scan).expect("parses");
            assert_eq!(scan.errors(), 0, "{}", path.display());
            let lines = src.iter().filter(|&&b| b == b'\n').count() + 1;
            let mut prev_start = 1;
            for (i, item) in scan.items().enumerate() {
                let at = path.display();
                assert!(
                    item.ok
                        && !item.name.is_empty()
                        && !item.name.contains(['\0', '\n', '\r'])
                        && !item.qual.contains(['\0', '\n', '\r']),
                    "{at}: {item:?}"
                );
                assert!(
                    1 <= item.start && item.start <= item.end && item.end <= lines,
                    "{at}: {item:?}"
                );
                if let Some(p) = item.parent {
                    let parent = scan.item(p).expect("a parent index is in range");
                    assert!(
                        p < i && parent.start <= item.start && item.end <= parent.end,
                        "{at}: {item:?}"
                    );
                }
                assert!(prev_start <= item.start, "{at}: not in pre-order");
                prev_start = item.start;
                seen.push((item.kind, item.name.to_owned(), item.qual.to_owned()));
            }
            assert!(scan.item(scan.items().len()).is_none());
        }
    }
    assert!(files >= 6, "only {files} source files found");
    for (kind, name, qual) in [
        (Kind::Fn, "canon_into", ""),
        (Kind::Impl, "Oracle", ""),
        (Kind::Impl, "OracleError", "fmt::Display"),
        (Kind::Enum, "Kind", ""),
        (Kind::Const, "ITEM_NODES", ""),
        (Kind::Mod, "tests", ""),
        (Kind::Fn, "own_sources_scan_cleanly", ""),
    ] {
        assert!(
            seen.contains(&(kind, name.to_owned(), qual.to_owned())),
            "missing {kind:?} {name} {qual}"
        );
    }
}
