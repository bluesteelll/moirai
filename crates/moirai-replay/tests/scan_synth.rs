//! WP-74: FL-1's Markdown and TOML scope scanners (WP-63; [F21 §4], §5) on generated documents, against their
//! construction.
//!
//! `moirai-tsoracle` has only the Rust grammar, and no other M0 oracle reads Markdown or TOML ([F21] open point 9), so
//! these scanners are compared with the items the generators ([`gen_md`], [`gen_toml`]) build their documents from —
//! headings with their levels, names, numberings, parents and sections; tables, arrays of tables and keys with their
//! spellings, parents and value ends — with LF or CR LF line ends and with or without a BOM.
//!
//! **What this cannot compare.** A construction checks only the constructs its generator writes, as it reads
//! [F21 §4] and §5 itself; nothing independent of this chapter checks the Markdown scanner against CommonMark or the
//! TOML scanner against a TOML parser. Outside the generators: Markdown container structure beyond first lines and
//! lazy continuation, HTML blocks other than comments, headings inside list items after a blank line ([F21 §4.9],
//! open point 9); invalid TOML, duplicate keys and tables, non-ASCII bare keys, unclosed strings and brackets
//! ([F21 §5.6]); the depth limit ([F21 §2.7]). Those are pinned by [F21 §8.2]–§8.3's golden examples in WP-63's
//! `scan_golden.rs` (R-FIX's scanner fixtures in `fixtures/r4/`, `fixtures/r4/INDEX.md` G-6, are still to come), and
//! totality and chunking by WP-63's `scan_props.rs` and `scan_line_model.rs`.
//!
//! Each document is scanned whole and also fed to the scanner in chunks that end anywhere ([`chunks`]), as a reader's
//! buffers split a file: both scanners read a byte at a time and hold a possible setext underline or ATX closing
//! sequence back as a count, so a chunk end inside one must change nothing.

mod chunks;
mod common;
mod gen_md;
mod gen_toml;

use std::cell::RefCell;

use moirai_files::scan::{Lang, scan};
use moirai_files::text::atext;
use moirai_replay::scandiff::{Row, scanner_rows};
use proptest::prelude::*;
use proptest::test_runner::TestCaseError;

/// The bytes of a rendered document with its line ends and BOM.
fn bytes_of(text: &str, crlf: bool, bom: bool) -> Vec<u8> {
    let mut b = Vec::new();
    if bom {
        b.extend_from_slice(b"\xef\xbb\xbf");
    }
    if crlf {
        b.extend_from_slice(text.replace('\n', "\r\n").as_bytes());
    } else {
        b.extend_from_slice(text.as_bytes());
    }
    b
}

/// An item of a language for messages: `h2 Storage[1.1] 3–9 in #0`.
fn show(lang: Lang, r: &Row) -> String {
    let mut s = format!("{} {}", lang.skind_name(r.skind).unwrap_or("?"), r.name);
    if r.qual != moirai_replay::scandiff::Text::Full(String::new()) {
        s.push_str(&format!("[{}]", r.qual));
    }
    s.push_str(&format!(" {}–{}", r.start, r.end));
    if let Some(p) = r.parent {
        s.push_str(&format!(" in #{p}"));
    }
    s
}

/// The ends of the chunks a document is fed in ([`chunks::scan_chunked`]).
fn cuts() -> impl Strategy<Value = Vec<usize>> {
    prop::collection::vec(any::<usize>(), 0..6)
}

/// Scans the bytes as the scanner reads a file ([F21 §1.3]: `atext`), whole and in the chunks `cuts` gives, and
/// compares both with the construction.
fn check(
    lang: Lang,
    text: &str,
    want: &[Row],
    (crlf, bom): (bool, bool),
    cuts: &[usize],
) -> Result<(), TestCaseError> {
    let bytes = bytes_of(text, crlf, bom);
    let t = atext(&bytes).ok_or_else(|| TestCaseError::fail("a generated document is not text"))?;
    let items = scan(lang, &t).map_err(|e| TestCaseError::fail(e.to_string()))?;
    let got = scanner_rows(&items);
    if got == want {
        return match chunks::scan_chunked(lang, &t, cuts) {
            Some(chunked) if chunked == got => Ok(()),
            other => Err(TestCaseError::fail(format!(
                "fed in chunks ending at {cuts:?} (modulo {}), the scanner gives {:?}\n\
                 document (crlf {crlf}, bom {bom}):\n{text}",
                t.len() + 1,
                other.map(|rows| rows.iter().map(|r| show(lang, r)).collect::<Vec<_>>())
            ))),
        };
    }
    let at = got
        .iter()
        .zip(want)
        .position(|(g, w)| g != w)
        .unwrap_or(got.len().min(want.len()));
    Err(TestCaseError::fail(format!(
        "{} items, {} expected; first difference at #{at}: got {:?}, expected {:?}\n\
         got: {:?}\nexpected: {:?}\ndocument (crlf {crlf}, bom {bom}):\n{text}",
        got.len(),
        want.len(),
        got.get(at).map(|r| show(lang, r)),
        want.get(at).map(|r| show(lang, r)),
        got.iter().map(|r| show(lang, r)).collect::<Vec<_>>(),
        want.iter().map(|r| show(lang, r)).collect::<Vec<_>>(),
    )))
}

#[test]
fn generated_markdown_gives_its_construction() {
    const TEST: &str = "generated_markdown_gives_its_construction";
    let counts = RefCell::new((0u64, 0u64));
    let cases = (
        gen_md::document(),
        any::<bool>(),
        prop::bool::weighted(0.2),
        cuts(),
    );
    common::runner(TEST, 256)
        .run(&cases, |(doc, crlf, bom, cuts)| {
            let (text, want) = gen_md::render(&doc);
            check(Lang::Markdown, &text, &want, (crlf, bom), &cuts)?;
            let mut c = counts.borrow_mut();
            c.0 += 1;
            c.1 += want.len() as u64;
            Ok(())
        })
        .unwrap_or_else(|e| panic!("{e}"));
    let (docs, items) = counts.into_inner();
    eprintln!("{TEST}: {docs} documents, {items} headings by construction");
    assert!(items > docs, "the generator produces headings");
}

#[test]
fn generated_toml_gives_its_construction() {
    const TEST: &str = "generated_toml_gives_its_construction";
    let counts = RefCell::new((0u64, 0u64));
    let cases = (
        gen_toml::document(),
        any::<bool>(),
        prop::bool::weighted(0.2),
        cuts(),
    );
    common::runner(TEST, 256)
        .run(&cases, |(doc, crlf, bom, cuts)| {
            let (text, want) = gen_toml::render(&doc);
            check(Lang::Toml, &text, &want, (crlf, bom), &cuts)?;
            let mut c = counts.borrow_mut();
            c.0 += 1;
            c.1 += want.len() as u64;
            Ok(())
        })
        .unwrap_or_else(|e| panic!("{e}"));
    let (docs, items) = counts.into_inner();
    eprintln!("{TEST}: {docs} documents, {items} items by construction");
    assert!(items > docs, "the generator produces items");
}
