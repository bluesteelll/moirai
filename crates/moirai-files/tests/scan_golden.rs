//! WP-63: every golden example of [F21 §8] — Rust constructs 1–27, Markdown M1–M14, TOML T1–T12 and the files
//! without a scanner — with its items, recorded scopes ([F21 §2.4]) and form results ([F21 §6]).
//!
//! The header texts the chapter lists are [F20 §2.8]'s function, which the anchor module owns; they are checked here
//! through a reading of [F20 §2.8] over this crate's public tokenizer, which is the part of them the scanner module
//! supplies. Every source is synthetic and written in the test ([OS/README §2.5], PLAN R18).

use moirai_files::scan::{
    FormKind, FormOutcome, Items, Lang, LiteralKind, ScanFailed, Scanner, Selector, TokenKind,
    find_form, scan, tokens,
};
use moirai_files::text::{NormalisedText, atext, nl};

// --- helpers -----------------------------------------------------------------------------------------------------

/// Items in the notation of [F21 §1.2]: `kind name [qual] start-end in parent-kind parent-name`.
fn render(items: &Items) -> Vec<String> {
    let lang = items.lang();
    let kind = |k: u8| lang.skind_name(k).unwrap_or("?");
    items
        .iter()
        .map(|i| {
            let qual = if i.qual.is_empty() {
                String::new()
            } else {
                format!(" [{}]", i.qual)
            };
            let parent = i
                .parent
                .and_then(|p| items.get(p))
                .map_or(String::new(), |p| {
                    format!(" in {} {}", kind(p.skind), p.name)
                });
            format!(
                "{} {}{qual} {}-{}{parent}",
                kind(i.skind),
                i.name,
                i.start,
                i.end
            )
        })
        .collect()
}

fn items_of(lang: Lang, src: &[u8]) -> Items {
    scan(lang, src).expect("the scan does not fail")
}

fn rust(src: &str) -> Vec<String> {
    render(&items_of(Lang::Rust, src.as_bytes()))
}

fn md(src: &str) -> Vec<String> {
    render(&items_of(Lang::Markdown, src.as_bytes()))
}

fn toml(src: &str) -> Vec<String> {
    render(&items_of(Lang::Toml, src.as_bytes()))
}

/// The scope a capture of lines [qs, qe] records, as scope text.
fn scope(lang: Lang, src: &str, qs: u64, qe: u64) -> Option<String> {
    items_of(lang, src.as_bytes())
        .capture_scope(qs, qe)
        .map(|s| s.to_string())
}

/// A form's outcome: the found item's name path, or the case.
fn form(lang: Lang, src: &str, selector: &str) -> String {
    let kind = if lang == Lang::Markdown {
        FormKind::Heading
    } else {
        FormKind::Symbol
    };
    let sel = Selector::parse(kind, selector).expect("a well-formed selector");
    let scanned: Result<Items, ScanFailed> = scan(lang, src.as_bytes());
    let outcome = find_form(&scanned, &sel);
    let items = scanned.expect("the scan does not fail");
    match outcome {
        FormOutcome::Found(i) => {
            let it = items.get(i).expect("an item");
            format!("found {} {}-{}", items.path_text(i), it.start, it.end)
        }
        FormOutcome::NotFound => "not found".to_owned(),
        FormOutcome::Several(v) => {
            let list: Vec<String> = v.iter().map(|&i| items.path_text(i)).collect();
            format!("several: {}", list.join(", "))
        }
        FormOutcome::NotRecordable(i) => format!("not recordable {}", items.path_text(i)),
        FormOutcome::FailedScan => "failed scan".to_owned(),
    }
}

/// [F20 §2.8] `header(l, symbol)` over N(t), read with [`tokens`]: from `start(l)`, a bracket depth over `(` and
/// `[`; the header ends before a `{` or `;` at depth 0, or before a line break at depth 0 once it holds a token at
/// depth 0 other than the header qualifier words, a string literal, `(` and `)`; trailing whitespace removed.
///
/// This is a reading of a function the anchor module owns (WP-64); once it lands, the checks below use it and this
/// helper goes. "A string literal" is read as the ABI after `extern` is ([F21 §3.5]: a string or a raw string,
/// `r`, `br` or `cr`), a reading [F20 §2.8] does not pin (spec finding of WP-63's review round 2).
fn header_symbol(t: &[u8], l: usize) -> String {
    let n = NormalisedText::new(t).expect("a short text");
    let x = &n.bytes()[n.start(l).expect("the line exists")..];
    let (mut d, mut only_quals, mut end) = (0u64, true, x.len());
    for tok in tokens(x) {
        let word = &x[tok.start..tok.end];
        match tok.kind {
            TokenKind::Newline if d == 0 && !only_quals => {
                end = tok.start;
                break;
            }
            TokenKind::Punct(b'{' | b';') if d == 0 => {
                end = tok.start;
                break;
            }
            TokenKind::Punct(b'(' | b'[') => {
                if d == 0 && tok.kind == TokenKind::Punct(b'[') {
                    only_quals = false;
                }
                d += 1;
            }
            TokenKind::Punct(b')') => d = d.saturating_sub(1),
            // A stray `]` at depth 0 is a token at depth 0 that is no header qualifier word.
            TokenKind::Punct(b']') if d == 0 => only_quals = false,
            TokenKind::Punct(b']') => d -= 1,
            TokenKind::Newline => {}
            TokenKind::Word
                if d == 0
                    && [
                        &b"pub"[..],
                        b"const",
                        b"async",
                        b"unsafe",
                        b"safe",
                        b"extern",
                        b"default",
                    ]
                    .contains(&word) => {}
            TokenKind::Literal(
                LiteralKind::Str
                | LiteralKind::RawStr
                | LiteralKind::RawByteStr
                | LiteralKind::RawCStr,
            ) if d == 0 => {}
            _ if d == 0 => only_quals = false,
            _ => {}
        }
    }
    let h = &x[..end];
    let z = h
        .iter()
        .rposition(|b| !b" \t\n\x0B\x0C\r".contains(b))
        .map_or(0, |z| z + 1);
    String::from_utf8_lossy(&h[..z]).into_owned()
}

/// [F20 §2.8] `header(l, heading)` = `nl(l)`.
fn header_heading(t: &[u8], l: usize) -> String {
    let line = t
        .split(|&b| b == b'\n')
        .nth(l - 1)
        .expect("the line exists");
    String::from_utf8_lossy(nl(line)).into_owned()
}

#[test]
fn the_header_reading_of_f20_2_8() {
    // Examples of [F20 §2.8], and the tokens at depth 0 that end the qualifier run.
    assert_eq!(
        header_symbol(
            b"pub(crate)
fn f() {",
            1
        ),
        "pub(crate)
fn f()"
    );
    assert_eq!(
        header_symbol(b"pub const K: [u8; 4] = [0; 4];", 1),
        "pub const K: [u8; 4] = [0; 4]"
    );
    assert_eq!(
        header_symbol(b"fn f(a: [u8; 4]) -> u32 {", 1),
        "fn f(a: [u8; 4]) -> u32"
    );
    assert_eq!(
        header_symbol(
            b"pub ]
fn f() {}",
            1
        ),
        "pub ]"
    );
    assert_eq!(
        header_symbol(
            b"pub [
fn f() {}",
            1
        ),
        "pub [
fn f() {}"
    );
    assert_eq!(
        header_symbol(
            b"pub )
fn f() {}",
            1
        ),
        "pub )
fn f()"
    );
    assert_eq!(
        header_symbol(
            b"extern br\"C\"
fn f() {}",
            1
        ),
        "extern br\"C\"
fn f()"
    );
    assert_eq!(
        header_symbol(
            b"extern b\"C\"
fn f() {}",
            1
        ),
        "extern b\"C\""
    );
}

// --- §8.1 Rust -----------------------------------------------------------------------------------------------------

#[test]
fn rust_1_every_kind() {
    let src = "mod m {}\nimpl S {}\nfn f() {}\nstruct S;\nenum E { A }\ntrait T {}\nconst C: u8 = 1;\nstatic G: u8 = 2;\nmacro_rules! mac { () => {} }";
    assert_eq!(
        rust(src),
        [
            "mod m 1-1",
            "impl S 2-2",
            "fn f 3-3",
            "struct S 4-4",
            "enum E 5-5",
            "trait T 6-6",
            "const C 7-7",
            "static G 8-8",
            "macro_rules mac 9-9"
        ]
    );
}

#[test]
fn rust_2_declarations_that_are_not_items() {
    let src = "use std::fmt;\nextern crate alloc;\ntype A<T> = Vec<T>;\nunion U { a: u8, b: u16 }\nextern \"C\" {}\nfn f<const N: usize>() -> impl Sized { let c = const { 1 }; let k = || 2; }\ntrait T { type Assoc; }\nenum E { Variant { field: u8 } }";
    assert_eq!(rust(src), ["fn f 6-6", "trait T 7-7", "enum E 8-8"]);
}

#[test]
fn rust_3_visibility_and_qualifiers() {
    let src = "pub const fn a() {}\npub(crate) async unsafe fn b() {}\nextern \"C\" fn c() {}\npub(in crate::x) static mut D: u8 = 0;\nunsafe trait E {}\nconst _: () = ();";
    assert_eq!(
        rust(src),
        [
            "fn a 1-1",
            "fn b 2-2",
            "fn c 3-3",
            "static D 4-4",
            "trait E 5-5",
            "const _ 6-6"
        ]
    );
}

#[test]
fn rust_4_raw_and_non_ascii_identifiers() {
    let src = "fn r#match() {}\nstruct Größe;\nmod 名前 {}\nfn _private() {}";
    assert_eq!(
        rust(src),
        [
            "fn r#match 1-1",
            "struct Größe 2-2",
            "mod 名前 3-3",
            "fn _private 4-4"
        ]
    );
}

#[test]
fn rust_5_siblings_that_touch() {
    assert_eq!(
        rust("mod a{fn f(){}}fn g(){}struct S;"),
        ["mod a 1-1", "fn f 1-1 in mod a", "fn g 1-1", "struct S 1-1"]
    );
}

#[test]
fn rust_6_parents_follow_containment() {
    let src = "fn outer() {\n    let _ = { fn deep() {} };\n    fn shallow() {}\n}";
    assert_eq!(
        rust(src),
        [
            "fn outer 1-4",
            "fn deep 2-2 in fn outer",
            "fn shallow 3-3 in fn outer"
        ]
    );
}

#[test]
fn rust_7_macro_bodies_are_token_trees() {
    let src = "macro_rules! mk {\n    ($n:ident) => { fn $n() {} struct Inside; };\n}\nthread_local! { static TL: u8 = 0; }\nlazy_static::lazy_static! { static ref X: u8 = 0; }\nmk!(made);\nfn after() {}";
    assert_eq!(rust(src), ["macro_rules mk 1-3", "fn after 7-7"]);
}

#[test]
fn rust_8_lf_crlf_and_a_bom_with_crlf() {
    let want = ["fn a 1-1", "impl S 3-5", "fn b 4-4 in impl S"];
    for raw in [
        &b"fn a() {}\n\nimpl S {\n    fn b() {}\n}\n"[..],
        b"fn a() {}\r\n\r\nimpl S {\r\n    fn b() {}\r\n}\r\n",
        b"\xEF\xBB\xBFfn a() {}\r\n\r\nimpl S {\r\n    fn b() {}\r\n}\r\n",
    ] {
        let t = atext(raw).expect("text");
        assert_eq!(render(&items_of(Lang::Rust, &t)), want);
    }
}

#[test]
fn rust_9_a_last_line_without_a_terminator() {
    assert_eq!(rust("fn a() {\n}"), ["fn a 1-2"]);
}

#[test]
fn rust_10_impl_names_and_qualifiers() {
    let rows: &[(&str, &str, &str, u64)] = &[
        ("impl<T> Pool<T> {}", "Pool<T>", "", 1),
        (
            "impl<T: Clone> From<T> for Wrap<T> {}",
            "Wrap<T>",
            "From<T>",
            1,
        ),
        ("impl<T> !Send for Raw<T> {}", "Raw<T>", "!Send", 1),
        ("impl ! /* comment */ Sync for Raw2 {}", "Raw2", "!Sync", 1),
        (
            "unsafe impl<'a> Sync for &'a   mut [u8] {}",
            "&'a mut[u8]",
            "Sync",
            1,
        ),
        (
            "impl Tr for dyn Fn(u8) -> u8 + Send {}",
            "dyn Fn(u8)->u8+Send",
            "Tr",
            1,
        ),
        (
            "impl Tr for extern \"C\" fn() {}",
            "extern \"C\" fn()",
            "Tr",
            1,
        ),
        ("impl Tr for (A, B) {}", "(A,B)", "Tr", 1),
        (
            "impl<T: Iterator> Tr for <T as Iterator>::Item {}",
            "<T as Iterator>::Item",
            "Tr",
            1,
        ),
        (
            "impl crate::a::Tr<u8> for super::b::C<{ 4 }> {}",
            "super::b::C<{4}>",
            "crate::a::Tr<u8>",
            1,
        ),
        (
            "impl<T> Display\n    for Multi<\n        T, // why\n        u8,\n    >\nwhere\n    T: Clone,\n{\n}",
            "Multi<T,u8,>",
            "Display",
            9,
        ),
        (
            "impl Tr for Foo<{ \"a\nb\".len() }> {}",
            "Foo<{\"a\\nb\".len()}>",
            "Tr",
            2,
        ),
        (
            "impl Tr for Foo<{ \"a\r\nb\".len() }> {}",
            "Foo<{\"a\\nb\".len()}>",
            "Tr",
            2,
        ),
        (
            "impl Tr for Bar<{ r\"x\ny\" }> {}",
            "Bar<{r\"x\\ny\"}>",
            "Tr",
            2,
        ),
        ("impl Tr for ! {}", "!", "Tr", 1),
    ];
    for &(src, name, qual, end) in rows {
        // The CR LF row is read as its anchor text, as every source is.
        let t = atext(src.as_bytes()).expect("text");
        let items = items_of(Lang::Rust, &t);
        let first = items.get(0).expect("one item");
        assert_eq!(
            (first.skind, first.name, first.qual, first.start, first.end),
            (2, name, qual, 1, end),
            "{src}"
        );
        assert_eq!(items.len(), 1, "{src}");
    }
    assert_eq!(
        rust("impl Tr for ty!{ x } {\n    fn m() {}\n}"),
        ["impl ty!{x} [Tr] 1-3", "fn m 2-2 in impl ty!{x}"]
    );
}

const CONSTRUCT_11: &str = "/// Doc comment.\n#[derive(Debug)]\n#[cfg(test)]\npub struct S {\n    x: u8,\n}\n\n// A plain comment.\n#[inline]\npub(crate)\nunsafe fn f(\n    a: u8,\n) -> u8 {\n    a\n}\n\n/** Block doc. */\nmod m;\n\n#[macro_export]\nmacro_rules! mac (\n    () => {}\n);\n";

#[test]
fn rust_11_lines_and_header_texts() {
    assert_eq!(
        rust(CONSTRUCT_11),
        [
            "struct S 4-6",
            "fn f 10-15",
            "mod m 18-18",
            "macro_rules mac 21-23"
        ]
    );
    let t = CONSTRUCT_11.as_bytes();
    assert_eq!(header_symbol(t, 4), "pub struct S");
    assert_eq!(
        header_symbol(t, 10),
        "pub(crate)\nunsafe fn f(\na: u8,\n) -> u8"
    );
    assert_eq!(header_symbol(t, 18), "mod m");
    assert_eq!(header_symbol(t, 21), "macro_rules! mac (\n() => {}\n)");
}

const CONSTRUCT_12: &str = "mod a {\n    impl Tr for S {\n        fn f() {\n            struct Local;\n            impl Local { fn g(&self) {} }\n            let _x = { enum E { A } 1 };\n        }\n    }\n    extern \"C\" {\n        fn ext();\n        static EXT: u8;\n    }\n    trait T {\n        const K: u8;\n        fn decl(&self);\n        fn def(&self) { fn inner() {} }\n    }\n    const C: () = { fn in_const() {} };\n}\nfn top() {}";

#[test]
fn rust_12_nesting_at_any_depth() {
    assert_eq!(
        rust(CONSTRUCT_12),
        [
            "mod a 1-19",
            "impl S [Tr] 2-8 in mod a",
            "fn f 3-7 in impl S",
            "struct Local 4-4 in fn f",
            "impl Local 5-5 in fn f",
            "fn g 5-5 in impl Local",
            "enum E 6-6 in fn f",
            "fn ext 10-10 in mod a",
            "static EXT 11-11 in mod a",
            "trait T 13-17 in mod a",
            "const K 14-14 in trait T",
            "fn decl 15-15 in trait T",
            "fn def 16-16 in trait T",
            "fn inner 16-16 in fn def",
            "const C 18-18 in mod a",
            "fn in_const 18-18 in const C",
            "fn top 20-20",
        ]
    );
    assert_eq!(
        scope(Lang::Rust, CONSTRUCT_12, 5, 5).as_deref(),
        Some("rust:mod a/impl S[Tr]/fn f/impl Local/fn g")
    );
    assert_eq!(
        scope(Lang::Rust, CONSTRUCT_12, 9, 12).as_deref(),
        Some("rust:mod a")
    );
    assert_eq!(
        scope(Lang::Rust, CONSTRUCT_12, 16, 16).as_deref(),
        Some("rust:mod a/trait T/fn def/fn inner")
    );
}

#[test]
fn rust_13_scopes_that_repeat_or_have_no_common_item() {
    let src = "mod a {\n    #[cfg(unix)]\n    fn g() { unix() }\n    #[cfg(windows)]\n    fn g() { windows() }\n}\nfn h() {} fn k() {}";
    assert_eq!(
        rust(src),
        [
            "mod a 1-6",
            "fn g 3-3 in mod a",
            "fn g 5-5 in mod a",
            "fn h 7-7",
            "fn k 7-7"
        ]
    );
    assert_eq!(scope(Lang::Rust, src, 5, 5).as_deref(), Some("rust:mod a"));
    assert_eq!(scope(Lang::Rust, src, 7, 7), None);
}

const CONSTRUCT_14: &str = "struct Wrap<T>(T);\nimpl<T> Wrap<T> { fn get(&self) {} }\nimpl<T: Clone> From<T> for Wrap<T> { fn from(t: T) -> Self { Wrap(t) } }\nimpl From<u8> for Wrap<u16> { fn from(v: u8) -> Self { Wrap(v.into()) } }";

#[test]
fn rust_14_the_symbol_form() {
    let f = |s: &str| form(Lang::Rust, CONSTRUCT_14, s);
    assert_eq!(f("Wrap"), "found rust:struct Wrap 1-1");
    assert_eq!(f("Wrap/get"), "found rust:impl Wrap<T>/fn get 2-2");
    assert_eq!(f("get"), "found rust:impl Wrap<T>/fn get 2-2");
    assert_eq!(
        f("Wrap[From]/from"),
        "several: rust:impl Wrap<T>[From<T>]/fn from, rust:impl Wrap<u16>[From<u8>]/fn from"
    );
    assert_eq!(
        f("Wrap[From<u8>]/from"),
        "found rust:impl Wrap<u16>[From<u8>]/fn from 4-4"
    );
    assert_eq!(
        f("Wrap<u16>[From<u8>]/from"),
        "found rust:impl Wrap<u16>[From<u8>]/fn from 4-4"
    );
    assert_eq!(
        f("from"),
        "several: rust:impl Wrap<T>[From<T>]/fn from, rust:impl Wrap<u16>[From<u8>]/fn from"
    );
    assert_eq!(f("Wrap/from"), "not found");
}

#[test]
fn rust_15_unterminated_literals_and_comments() {
    assert_eq!(rust("fn a() { let s = \"abc; }\nfn b() {}"), ["fn a 1-2"]);
    assert_eq!(rust("fn a() {} /* fn b() {}"), ["fn a 1-1"]);
    assert_eq!(
        rust("fn a() {}\nconst S: &str = r#\"abc;\nfn b() {}"),
        ["fn a 1-1", "const S 2-3"]
    );
}

#[test]
fn rust_16_unbalanced_groups() {
    assert_eq!(
        rust("mod m {\n    fn a() {}\nfn b() {}"),
        ["mod m 1-3", "fn a 2-2 in mod m", "fn b 3-3 in mod m"]
    );
    assert_eq!(rust("fn a() {} }\nfn b() {}"), ["fn a 1-1", "fn b 2-2"]);
    assert_eq!(
        rust("fn a() { foo(; }\nfn b() {}"),
        ["fn a 1-1", "fn b 2-2"]
    );
}

#[test]
fn rust_17_a_less_than_that_never_closes() {
    assert_eq!(
        rust("fn f(x: Vec<u8) {\n}\nfn g() {}"),
        ["fn f 1-2", "fn g 3-3"]
    );
    assert_eq!(
        rust("struct S<T {\n}\nfn g() {}"),
        ["struct S 1-2", "fn g 3-3"]
    );
}

#[test]
fn rust_18_const_generic_blocks_in_headers() {
    assert_eq!(
        rust("fn f<const N: usize = { 3 }>() -> Foo<{ N }> {\n}\nfn g() {}"),
        ["fn f 1-2", "fn g 3-3"]
    );
}

#[test]
fn rust_19_items_without_a_name() {
    assert_eq!(rust("impl<T> {\n    fn f() {}\n}"), ["fn f 2-2"]);
    assert_eq!(rust("impl Tr for {}\nfn g() {}"), ["fn g 2-2"]);
    assert_eq!(rust("fn () { fn inner() {} }"), ["fn inner 1-1"]);
    assert_eq!(rust("fn match() { fn x() {} }"), ["fn x 1-1"]);
}

#[test]
fn rust_20_words_that_are_not_qualifiers() {
    assert_eq!(rust("auto trait A {}\nfn g() {}"), ["fn g 2-2"]);
    assert_eq!(rust("static ref X: u8 = 0;\nfn g() {}"), ["fn g 2-2"]);
    assert_eq!(
        rust("impl S { default fn f() {} }"),
        ["impl S 1-1", "fn f 1-1 in impl S"]
    );
    assert_eq!(rust("trait A = B;\nfn g() {}"), ["trait A 1-1", "fn g 2-2"]);
}

#[test]
fn rust_21_what_is_a_token_tree() {
    assert_eq!(
        rust("fn a() { if !(x) { fn b() {} } }"),
        ["fn a 1-1", "fn b 1-1 in fn a"]
    );
    assert_eq!(rust("gen! { fn x() {} }\nfn y() {}"), ["fn y 2-2"]);
    assert_eq!(rust("#[doc = { fn x() {} }] fn y() {}"), ["fn y 1-1"]);
    assert_eq!(rust("macro_rules! { fn x() {} }\nfn y() {}"), ["fn y 2-2"]);
}

#[test]
fn rust_22_shebang_and_inner_attribute() {
    assert_eq!(
        rust("#!/usr/bin/env rust-script\nfn main() {}"),
        ["fn main 2-2"]
    );
    assert_eq!(rust("#![allow(x)]\nfn main() {}"), ["fn main 2-2"]);
}

#[test]
fn rust_23_extents_without_a_terminator() {
    assert_eq!(
        rust("mod m { const X: u8 = 1 }\nfn g() {}"),
        ["mod m 1-1", "const X 1-1 in mod m", "fn g 2-2"]
    );
    assert_eq!(rust("fn f() -> X\nfn g() {}"), ["fn f 1-2"]);
}

#[test]
fn rust_24_the_depth_limit() {
    let src = |n: usize| format!("fn a() {}{} {{}}", "(".repeat(n), ")".repeat(n));
    assert_eq!(rust(&src(1024)), ["fn a 1-1"]);
    assert_eq!(
        scan(Lang::Rust, src(1025).as_bytes()).map(|_| ()),
        Err(ScanFailed)
    );
}

#[test]
fn rust_25_a_name_that_is_not_utf8() {
    assert_eq!(
        render(&items_of(Lang::Rust, b"fn caf\xE9() {}")),
        ["fn caf\u{FFFD} 1-1"]
    );
}

#[test]
fn rust_26_unsafe_extern_blocks() {
    assert_eq!(
        rust("unsafe extern \"C\" {\n    safe fn f();\n    pub safe static S: u8;\n}"),
        ["fn f 2-2", "static S 3-3"]
    );
}

#[test]
fn rust_27_weak_keywords_as_names() {
    let src = "const safe: bool = true;\nconst default: u8 = 0;\nimpl Default for X { fn default() -> Self { X } }";
    assert_eq!(
        rust(src),
        [
            "const safe 1-1",
            "const default 2-2",
            "impl X [Default] 3-3",
            "fn default 3-3 in impl X"
        ]
    );
}

// --- §8.2 Markdown -------------------------------------------------------------------------------------------------

const M1: &str = "# Title\n\n## 1. Scope\n\ntext\n\n### 1.1 What   it  fixes ###\n\n## 2 Layout\n";

#[test]
fn markdown_m1_atx_headings_numbering_closing_sequence() {
    assert_eq!(
        md(M1),
        [
            "h1 Title 1-9",
            "h2 Scope [1.] 3-8 in h1 Title",
            "h3 What it fixes [1.1] 7-8 in h2 Scope",
            "h2 Layout [2] 9-9 in h1 Title"
        ]
    );
    assert_eq!(header_heading(M1.as_bytes(), 3), "## 1. Scope");
    assert_eq!(
        header_heading(M1.as_bytes(), 7),
        "### 1.1 What   it  fixes ###"
    );
}

#[test]
fn markdown_m2_what_is_not_an_atx_heading() {
    let src = "#Not a heading\n####### seven\n    # indented code\n   ### three spaces\n#\n## ##\n# foo#\n# foo \\#\n#\tTab\n";
    assert_eq!(
        md(src),
        [
            "h3 three spaces 4-6",
            "h1 foo# 7-7",
            "h1 foo \\# 8-8",
            "h1 Tab 9-9"
        ]
    );
    assert!(md("\t# x\n").is_empty());
}

#[test]
fn markdown_m3_setext_headings() {
    let src = "Title\n=====\n\nSub\ntitle\n---\n\n    code\n---\n";
    assert_eq!(md(src), ["h1 Title 1-9", "h2 Sub title 4-9 in h1 Title"]);
    assert_eq!(header_heading(src.as_bytes(), 4), "Sub");
}

#[test]
fn markdown_m4_fences() {
    let src = "# A\n```rust\n# not a heading\n```\n~~~~~\n```\n# still code\n~~~~~\n## B\n``` x ` y\n# after\n";
    assert_eq!(
        md(src),
        ["h1 A 1-10", "h2 B 9-10 in h1 A", "h1 after 11-11"]
    );
}

#[test]
fn markdown_m5_an_unclosed_fence() {
    assert_eq!(md("# A\n```\n# hidden\n## hidden too\n"), ["h1 A 1-4"]);
}

#[test]
fn markdown_m6_html_comments() {
    assert_eq!(
        md("# A\n<!--\n# commented out\n-->\n## B\n<!-- one line -->\n## C\n"),
        ["h1 A 1-7", "h2 B 5-6 in h1 A", "h2 C 7-7 in h1 A"]
    );
}

#[test]
fn markdown_m7_front_matter() {
    assert_eq!(md("---\ntitle: X\n---\n# Real\n"), ["h1 Real 4-4"]);
    assert!(md("---\ntitle: X\n").is_empty());
}

#[test]
fn markdown_m8_container_lines() {
    let src = "# A\n> # quoted\n- # listed\n1. # ordered\ntext\n- item\ncontinued\n---\n\nPara\n- item\n---\n\nPara\n2. two\n---\n";
    assert_eq!(md(src), ["h1 A 1-16", "h2 Para 2. two 14-16 in h1 A"]);
}

#[test]
fn markdown_m9_levels_and_sections() {
    assert_eq!(
        md("## Two\n#### Four\n### Three\n# One\n###### Six\n"),
        [
            "h2 Two 1-3",
            "h4 Four 2-2 in h2 Two",
            "h3 Three 3-3 in h2 Two",
            "h1 One 4-5",
            "h6 Six 5-5 in h1 One"
        ]
    );
}

#[test]
fn markdown_m10_numbering() {
    let rows: &[(&str, &str, &str)] = &[
        ("3.2 Recovery", "Recovery", "3.2"),
        ("§3 Storage", "Storage", "§3"),
        ("§ 3.1 Storage", "Storage", "§ 3.1"),
        ("A.1 Parts", "Parts", "A.1"),
        ("A. Intro", "Intro", "A."),
        ("1. Scope", "Scope", "1."),
        ("1) First", "First", "1)"),
        ("20 — R4 constants", "R4 constants", "20"),
        ("2.7 - Anchors", "Anchors", "2.7"),
        ("3.2.  Tabs\tand  spaces", "Tabs and spaces", "3.2."),
        ("2026-09-28 update", "2026-09-28 update", ""),
        ("3D graphics", "3D graphics", ""),
        ("1.2.3", "1.2.3", ""),
        ("A Tale", "A Tale", ""),
        ("A.I. and ML", "A.I. and ML", ""),
        ("1234567890 big", "1234567890 big", ""),
        ("10x faster", "10x faster", ""),
        ("3.2 —", "—", "3.2"),
        ("3.2 Восстановление", "Восстановление", "3.2"),
    ];
    for &(text, name, qual) in rows {
        let items = items_of(Lang::Markdown, format!("# {text}").as_bytes());
        let h = items.get(0).expect("one heading");
        assert_eq!((h.name, h.qual), (name, qual), "{text}");
    }
}

#[test]
fn markdown_m11_paragraph_continuation() {
    assert_eq!(
        md("Para\n    indented continuation\n===\n\n- item\nnext\n===\n"),
        ["h1 Para indented continuation 1-7"]
    );
}

#[test]
fn markdown_m12_no_setext_heading() {
    assert!(md("Text\n***\n\n***\nText2\n- - -\n").is_empty());
}

#[test]
fn markdown_m13_invalid_utf8() {
    assert_eq!(
        render(&items_of(Lang::Markdown, b"# caf\xE9\n")),
        ["h1 caf\u{FFFD} 1-1"]
    );
}

const M14: &str = "# 1 Design\n\n## 1.1 Storage\n\n### Recovery\n\n## 1.2 Input/output\n\n### Recovery\n\n# 2 Plan\n\n## Storage\n";

#[test]
fn markdown_m14_scopes_and_the_heading_form() {
    assert_eq!(
        md(M14),
        [
            "h1 Design [1] 1-10",
            "h2 Storage [1.1] 3-6 in h1 Design",
            "h3 Recovery 5-6 in h2 Storage",
            "h2 Input/output [1.2] 7-10 in h1 Design",
            "h3 Recovery 9-10 in h2 Input/output",
            "h1 Plan [2] 11-13",
            "h2 Storage 13-13 in h1 Plan",
        ]
    );
    let sc = |a, b| scope(Lang::Markdown, M14, a, b);
    assert_eq!(
        sc(5, 5).as_deref(),
        Some("markdown:h1 Design[1]/h2 Storage[1.1]/h3 Recovery")
    );
    assert_eq!(
        sc(7, 7).as_deref(),
        Some("markdown:h1 Design[1]/h2 Input%2foutput[1.2]")
    );
    assert_eq!(sc(3, 9).as_deref(), Some("markdown:h1 Design[1]"));
    assert_eq!(
        sc(13, 13).as_deref(),
        Some("markdown:h1 Plan[2]/h2 Storage")
    );
    // Renumbering line 3 keeps the recorded scope resolving uniquely (numbering is not compared, [F21 §2.2]).
    let recorded = items_of(Lang::Markdown, M14.as_bytes())
        .capture_scope(5, 5)
        .expect("a scope");
    let renumbered = M14.replace("## 1.1 Storage", "## 1.4 Storage");
    let now = items_of(Lang::Markdown, renumbered.as_bytes());
    assert_eq!(
        now.resolve(&recorded).map(|y| (y.name, y.start, y.end)),
        Some(("Recovery", 5, 6))
    );

    let f = |s: &str| form(Lang::Markdown, M14, s);
    assert_eq!(
        f("Recovery"),
        "several: markdown:h1 Design[1]/h2 Storage[1.1]/h3 Recovery, markdown:h1 Design[1]/h2 Input%2foutput[1.2]/h3 Recovery"
    );
    assert_eq!(
        f("Storage/Recovery"),
        "found markdown:h1 Design[1]/h2 Storage[1.1]/h3 Recovery 5-6"
    );
    assert_eq!(
        f("Design/Storage"),
        "found markdown:h1 Design[1]/h2 Storage[1.1] 3-6"
    );
    assert_eq!(
        f("1.1 Storage"),
        "found markdown:h1 Design[1]/h2 Storage[1.1] 3-6"
    );
    assert_eq!(f("1.1"), "found markdown:h1 Design[1]/h2 Storage[1.1] 3-6");
    assert_eq!(
        f("Storage"),
        "several: markdown:h1 Design[1]/h2 Storage[1.1], markdown:h1 Plan[2]/h2 Storage"
    );
    assert_eq!(
        f("Input%2Foutput/Recovery"),
        "found markdown:h1 Design[1]/h2 Input%2foutput[1.2]/h3 Recovery 9-10"
    );
    assert_eq!(
        f("Plan/Storage"),
        "found markdown:h1 Plan[2]/h2 Storage 13-13"
    );
    assert_eq!(f("Nothing"), "not found");
}

// --- §8.3 TOML ------------------------------------------------------------------------------------------------------

const T1: &str = "name = \"top\"\n[package]\nname = \"x\"\nversion = \"0.1.0\"\n\n# comment\n[dependencies]\nserde = { version = \"1\", features = [\"derive\"] }\n";

#[test]
fn toml_t1_tables_and_keys() {
    assert_eq!(
        toml(T1),
        [
            "key name 1-1",
            "table package 2-4",
            "key name 3-3 in table package",
            "key version 4-4 in table package",
            "table dependencies 7-8",
            "key serde 8-8 in table dependencies",
        ]
    );
    let t = T1.as_bytes();
    assert_eq!(header_symbol(t, 2), "[package]");
    assert_eq!(header_symbol(t, 3), "name = \"x\"");
    assert_eq!(header_symbol(t, 8), "serde =");
    let sc = |a, b| scope(Lang::Toml, T1, a, b);
    assert_eq!(sc(3, 3).as_deref(), Some("toml:table package/key name"));
    assert_eq!(
        sc(8, 8).as_deref(),
        Some("toml:table dependencies/key serde")
    );
    assert_eq!(sc(1, 1).as_deref(), Some("toml:key name"));
    assert_eq!(sc(5, 6), None);
    let f = |s: &str| form(Lang::Toml, T1, s);
    assert_eq!(f("package/name"), "found toml:table package/key name 3-3");
    assert_eq!(
        f("name"),
        "several: toml:key name, toml:table package/key name"
    );
    assert_eq!(
        f("dependencies/serde"),
        "found toml:table dependencies/key serde 8-8"
    );
    assert_eq!(f("serde"), "found toml:table dependencies/key serde 8-8");
    assert_eq!(f("dependencies"), "found toml:table dependencies 7-8");
    assert_eq!(f("package[x]/name"), "not found");
}

#[test]
fn toml_t2_arrays_of_tables() {
    let src = "[[bin]]\nname = \"a\"\n\n[[bin]]\nname = \"b\"\n";
    assert_eq!(
        toml(src),
        [
            "array_table bin 1-2",
            "key name 2-2 in array_table bin",
            "array_table bin 4-5",
            "key name 5-5 in array_table bin"
        ]
    );
    assert_eq!(scope(Lang::Toml, src, 2, 2), None);
    assert_eq!(
        form(Lang::Toml, src, "bin/name"),
        "several: toml:array_table bin/key name, toml:array_table bin/key name"
    );
}

const T3: &str = "[ workspace . dependencies ]\na.b = 1\n\"quoted key\" = 2\n'lit' = 3\n\"a.b\" = 4\n[target.\"cfg(windows)\".dependencies]\nwinapi = \"0.3\"\n";

#[test]
fn toml_t3_dotted_and_quoted_keys() {
    assert_eq!(
        toml(T3),
        [
            "table workspace.dependencies 1-5",
            "key a.b 2-2 in table workspace.dependencies",
            "key \"quoted key\" 3-3 in table workspace.dependencies",
            "key 'lit' 4-4 in table workspace.dependencies",
            "key \"a.b\" 5-5 in table workspace.dependencies",
            "table target.\"cfg(windows)\".dependencies 6-7",
            "key winapi 7-7 in table target.\"cfg(windows)\".dependencies",
        ]
    );
    let f = |s: &str| form(Lang::Toml, T3, s);
    assert_eq!(
        f("workspace.dependencies/a.b"),
        "found toml:table workspace.dependencies/key a.b 2-2"
    );
    assert_eq!(
        f("\"quoted key\""),
        "found toml:table workspace.dependencies/key \"quoted key\" 3-3"
    );
    assert_eq!(
        f("target.\"cfg(windows)\".dependencies/winapi"),
        "found toml:table target.\"cfg(windows)\".dependencies/key winapi 7-7"
    );
}

const T4: &str = "features = [\n  \"a\",\n  [1],\n  \"b\", # comment ]\n]\nnext = 1\n";

#[test]
fn toml_t4_a_multi_line_array() {
    assert_eq!(toml(T4), ["key features 1-5", "key next 6-6"]);
    assert_eq!(
        header_symbol(T4.as_bytes(), 1),
        "features = [\n\"a\",\n[1],\n\"b\", # comment ]"
    );
}

#[test]
fn toml_t5_multi_line_strings() {
    assert_eq!(
        toml(
            "text = \"\"\"\n[not.a.table]\nkey = 1\n\"\"\"\nlit = '''\n[also.not]\n'''\nafter = 1\n"
        ),
        ["key text 1-4", "key lit 5-7", "key after 8-8"]
    );
}

#[test]
fn toml_t6_lines_that_are_nothing() {
    assert_eq!(
        toml("[a]\nx = 1\n[b] junk\ny = 2\nnot a key\n[[c] ]\nz = 3\n"),
        [
            "table a 1-7",
            "key x 2-2 in table a",
            "key y 4-4 in table a",
            "key z 7-7 in table a"
        ]
    );
}

#[test]
fn toml_t7_an_unclosed_array() {
    assert_eq!(toml("a = [1, 2\n[b]\nc = 1\n"), ["key a 1-3"]);
}

#[test]
fn toml_t8_value_forms() {
    let src = "a = \"x\" # c\nb = 'y'\nc = 1979-05-27T07:32:00Z\nd = [ { x = 1 }, { y = [2, 3] } ]\ne =\nf = \"\"\"one-line\"\"\"\ng = \"unterminated\nh = 1\n";
    assert_eq!(
        toml(src),
        [
            "key a 1-1",
            "key b 2-2",
            "key c 3-3",
            "key d 4-4",
            "key e 5-5",
            "key f 6-6",
            "key g 7-7",
            "key h 8-8"
        ]
    );
}

#[test]
fn toml_t9_quote_runs() {
    assert_eq!(
        toml("s = \"\"\"a\"\"\"\"\nt = \"\"\"a\\\"\"\"\nb\"\"\"\nu = 1\n"),
        ["key s 1-1", "key t 2-3", "key u 4-4"]
    );
}

#[test]
fn toml_t10_a_table_without_keys() {
    assert_eq!(
        toml("[a]\n[b]\nx = 1\n"),
        ["table a 1-1", "table b 2-3", "key x 3-3 in table b"]
    );
}

#[test]
fn toml_t11_keys_outside_toml_1_0() {
    assert_eq!(toml("ключ = 1\n[таблица]\nok = 1\n"), ["key ok 3-3"]);
}

#[test]
fn toml_t12_the_depth_limit() {
    let src = |n: usize| format!("a = {}{}", "[".repeat(n), "]".repeat(n));
    assert_eq!(toml(&src(1024)), ["key a 1-1"]);
    assert_eq!(
        scan(Lang::Toml, src(1025).as_bytes()).map(|_| ()),
        Err(ScanFailed)
    );
}

// --- §8.4 files without a scanner -----------------------------------------------------------------------------------

#[test]
fn files_without_a_scanner() {
    for p in ["a.py", "shaders/x.hlsl", "data.json", "notes.txt"] {
        assert_eq!(Lang::of_path(p.as_bytes()), None, "{p}");
    }
    // A spec on such a file does not split, so it is read as a path ([F21 §6.1]).
    assert_eq!(moirai_files::scan::split_symbol("a.py::F"), None);
    assert_eq!(moirai_files::scan::split_heading("notes.txt#H"), None);
    // A scope recorded on `x.md` does not resolve once the file is `x.txt`: the current language differs.
    let recorded = items_of(Lang::Markdown, M14.as_bytes())
        .capture_scope(5, 5)
        .expect("a scope");
    assert_eq!(Lang::of_path(b"x.txt"), None);
    assert!(items_of(Lang::Toml, b"").resolve(&recorded).is_none());
}

// --- failed scans ([F21 §2.7]) --------------------------------------------------------------------------------------

#[test]
fn a_failed_scan_refuses_forms() {
    let src = format!("fn a() {}{}", "(".repeat(1025), ")".repeat(1025));
    let scanned = scan(Lang::Rust, src.as_bytes());
    let sel = Selector::parse(FormKind::Symbol, "a").expect("a selector");
    assert_eq!(find_form(&scanned, &sel), FormOutcome::FailedScan);
}

// --- paths the constructs above leave open ----------------------------------------------------------------------------

#[test]
fn the_symbol_form_has_no_leading_path_separator() {
    // [F21 §6.2] "Bare name" admits no leading `::`, so a trait written `::std::fmt::Display` has no bare name and is
    // typed in full (spec finding of WP-63's review: allow an optional leading `::`).
    let src = "impl ::std::fmt::Display for X {}";
    assert_eq!(rust(src), ["impl X [::std::fmt::Display] 1-1"]);
    assert_eq!(
        form(Lang::Rust, src, "X[::std::fmt::Display]"),
        "found rust:impl X[::std::fmt::Display] 1-1"
    );
    assert_eq!(form(Lang::Rust, src, "X[Display]"), "not found");
}

#[test]
fn item_headers_of_the_same_kind() {
    // [F21 §2.6]: the last segment's kind decides; the language must be the scope's.
    let names = |items: &Items, scope: &str| -> Vec<String> {
        let s = items
            .iter()
            .find_map(|i| {
                let sc = items.scope_of(i.index)?;
                (sc.to_string() == scope).then_some(sc)
            })
            .expect("the scope names an item");
        items
            .same_kind(&s)
            .map(|i| format!("{} {}", i.name, i.start))
            .collect()
    };
    let rs = items_of(
        Lang::Rust,
        b"mod a {\n    fn f() {}\n    struct S;\n}\nfn g() {}\nimpl S { fn h() {} }",
    );
    assert_eq!(names(&rs, "rust:mod a/fn f"), ["f 2", "g 5", "h 6"]);
    assert_eq!(names(&rs, "rust:mod a"), ["a 1"]);
    let md_items = items_of(Lang::Markdown, M14.as_bytes());
    assert_eq!(
        names(&md_items, "markdown:h1 Design[1]/h2 Storage[1.1]"),
        ["Storage 3", "Input/output 7", "Storage 13"]
    );
    let toml_items = items_of(Lang::Toml, b"a = 1\n[t]\nb = 2\n[[u]]\n");
    assert_eq!(names(&toml_items, "toml:table t/key b"), ["a 1", "b 3"]);
    // A scope of another language admits no header.
    let s = rs.scope_of(0).expect("a scope");
    assert_eq!(md_items.same_kind(&s).count(), 0);
}

#[test]
fn a_name_path_deeper_than_64_is_not_recordable() {
    // A `fn` inside 65 modules: its name path has 66 segments ([F21 §2.3]).
    let src = format!(
        "{}fn f() {{}}\n{}",
        "mod m {\n".repeat(65),
        "}\n".repeat(65)
    );
    let items = items_of(Lang::Rust, src.as_bytes());
    assert_eq!(items.len(), 66);
    let f = items.get(65).expect("fn f");
    assert_eq!((f.name, f.start, f.end), ("f", 66, 66));
    assert!(!items.recordable(65) && !items.recordable(64) && items.recordable(63));
    assert_eq!(
        form(Lang::Rust, &src, "f"),
        format!("not recordable rust:{}fn f", "mod m/".repeat(65))
    );
    // A `path:L-M` capture of its line records its nearest recordable ancestor ([F21 §2.4], §6.5).
    let s = items.capture_scope(66, 66).expect("a scope");
    assert_eq!(s.len(), 64);
    assert_eq!(items.resolve(&s).map(|y| y.index), Some(63));
}

#[test]
fn one_match_whose_name_path_names_several_items() {
    // [F21 §6.5] "several", second clause: the numbering picks one heading, but name paths ignore numbering.
    let src = "# 1 A\n## 1.1 B\n## 1.2 B\n";
    assert_eq!(
        md(src),
        [
            "h1 A [1] 1-3",
            "h2 B [1.1] 2-2 in h1 A",
            "h2 B [1.2] 3-3 in h1 A"
        ]
    );
    let sel = Selector::parse(FormKind::Heading, "1.1").expect("a selector");
    let scanned = scan(Lang::Markdown, src.as_bytes());
    assert_eq!(find_form(&scanned, &sel), FormOutcome::Several(vec![1, 2]));
    assert_eq!(
        form(Lang::Markdown, src, "1.1"),
        "several: markdown:h1 A[1]/h2 B[1.1], markdown:h1 A[1]/h2 B[1.2]"
    );
    assert_eq!(
        form(Lang::Markdown, src, "1.2 B"),
        form(Lang::Markdown, src, "1.1")
    );
}

#[test]
fn a_caller_may_stop_feeding_a_failed_scan() {
    // Rust fails at the token that opens the 1,025th group.
    let mut s = Scanner::new(Lang::Rust);
    s.feed(b"fn a() ");
    s.feed("(".repeat(1024).as_bytes());
    assert!(!s.has_failed());
    s.feed(b"(");
    assert!(s.has_failed());
    s.feed(b") fn b() {}");
    assert_eq!(s.finish().map(|_| ()), Err(ScanFailed));
    // TOML fails at the byte that opens a value's 1,025th bracket.
    let mut s = Scanner::new(Lang::Toml);
    s.feed(b"x = 1\na = ");
    s.feed("[".repeat(1024).as_bytes());
    assert!(!s.has_failed());
    s.feed(b"[");
    assert!(s.has_failed());
    s.feed(b"\n");
    s.feed(b"[t]\nk = 1\n");
    assert_eq!(s.finish().map(|_| ()), Err(ScanFailed));
    // Markdown never fails.
    let mut s = Scanner::new(Lang::Markdown);
    s.feed("[".repeat(5000).as_bytes());
    assert!(!s.has_failed());
}
