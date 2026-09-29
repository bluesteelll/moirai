//! Tests of the LQ-3 front end: every production of grammar v1, every refused form and parser decision, the token
//! stream format, the printer property, the binder's codes and the canonical encoding, robustness against any text,
//! and the conformance fixtures of `fixtures/lq`.

mod bind;
mod conformance;
mod fixture;
mod parse;
mod parse_errors;
mod print;
mod robust;
mod strat;

use crate::lq::diag::{Code, Diag, line_col};
use crate::lq::parser::{ParseOptions, parse_define, parse_read, parse_write};
use crate::lq::sexpr;
use proptest::test_runner::{Config as ProptestConfig, RngAlgorithm, TestRng, TestRunner};

/// A property-test runner for the tier `MOIRAI_TEST_TIER` names (PLAN §2.1: `pr` by default, `nightly`, `exit`):
/// `cases` cases in the `pr` tier, ten and a hundred times as many in the others, each tier from its own fixed seed,
/// so every run of a tier tries the same cases. Nothing is persisted: a minimised failure becomes a named unit test.
pub(super) fn runner(cases: u32) -> TestRunner {
    let (tier, scale) = match std::env::var("MOIRAI_TEST_TIER").as_deref() {
        Ok("nightly") => (2u8, 10),
        Ok("exit") => (3u8, 100),
        _ => (1u8, 1),
    };
    let mut seed = *b"moirai-model/lq/proptest-seed-v1";
    seed[31] ^= tier;
    TestRunner::new_with_rng(
        ProptestConfig {
            cases: cases.saturating_mul(scale),
            max_shrink_iters: 4096,
            failure_persistence: None,
            ..ProptestConfig::default()
        },
        TestRng::from_seed(RngAlgorithm::ChaCha, &seed),
    )
}

/// The S-expression of a read that must parse.
pub(super) fn sx(src: &str) -> String {
    match parse_read(src, ParseOptions::default()) {
        Ok(p) => sexpr::read(&p.tree),
        Err(e) => panic!("{src:?} failed: {}", show(src, &e)),
    }
}

/// The S-expression of a `TX` block that must parse.
pub(super) fn sx_tx(src: &str) -> String {
    match parse_write(src, ParseOptions::default()) {
        Ok(p) => sexpr::tx(&p.tree),
        Err(e) => panic!("{src:?} failed: {}", show(src, &e)),
    }
}

/// The S-expression of a definition that must parse.
pub(super) fn sx_define(src: &str) -> String {
    match parse_define(src, ParseOptions::default()) {
        Ok(p) => sexpr::define(&p.tree),
        Err(e) => panic!("{src:?} failed: {}", show(src, &e)),
    }
}

/// Asserts two S-expressions are equal by [LQ/canonical-ast §4.1].
pub(super) fn assert_sx(got: &str, want: &str) {
    assert!(sexpr::same(got, want), "\n got: {got}\nwant: {want}");
}

pub(super) fn show(src: &str, e: &[Diag]) -> String {
    e.iter()
        .map(|d| {
            let (l, c) = d.span.map_or((0, 0), |s| line_col(src, s.start));
            format!("{} {l}:{c} {} {:?}", d.code, d.message, d.inline)
        })
        .collect::<Vec<_>>()
        .join(" | ")
}

/// The first error of a read: code, line, column.
pub(super) fn first_err(src: &str, strict: bool) -> (Code, u32, u32) {
    let e = parse_read(src, ParseOptions { strict_gql: strict })
        .err()
        .unwrap_or_else(|| panic!("{src:?} parsed"));
    let d = &e[0];
    let (l, c) = line_col(src, d.span.expect("located").start);
    (d.code, l, c)
}

/// The first error of a write.
pub(super) fn first_err_tx(src: &str, strict: bool) -> (Code, u32, u32) {
    let e = parse_write(src, ParseOptions { strict_gql: strict })
        .err()
        .unwrap_or_else(|| panic!("{src:?} parsed"));
    let d = &e[0];
    let (l, c) = line_col(src, d.span.expect("located").start);
    (d.code, l, c)
}
