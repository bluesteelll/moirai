//! The k-bounded Myers bit-parallel matcher and the histogram line diff. FL-1's anchors use it now; M3's diff3 builds
//! on it later.
//!
//! Product crate with no dependencies; checked by GT20 (e) on every target. Filled by WP-60 (R-FL1B). Sources:
//! [40 §4.5, §8.1]; `docs/m0/PLAN.md` §2.2.
//!
//! Module map:
//!
//! | Module | Contents | Specification |
//! |---|---|---|
//! | [`myers`] | [`Pattern`], [`Searcher`], [`Hit`], [`Located`], [`levenshtein`]: approximate quote search with a budget k, the start of a match, the Levenshtein distance | [F20 §6.4], [40 §4.5] step 4 |
//! | [`select`] | [`Selector`]: the candidates of a fuzzy-quote region, selected as the hits stream in, in bounded memory | [F20 §6.4] |
//! | [`histogram`] | [`Differ`], [`Match`], [`Hunk`], [`hunks`], [`partners`], [`diff`], [`diff_lines`]: HD, the histogram line diff, with the hunks and partner maps diff3 reads | [F12 §7.5], [AR §2.10] T10 |
//! | [`lines`](mod@lines) | [`lines()`](fn@lines), [`Interner`]: lines of a text value and dense line ids | [F12 §7.5] "Lines" |
//! | [`lcs`] | [`lcs_len`]: bit-parallel LCS length of two token sequences | [F20 §6.3], [40 §2.7] |
//!
//! Every function is total: no input panics. Memory is linear in the input, and every buffer that outlives a call is
//! reusable scratch ([`Differ`]).

pub mod histogram;
pub mod lcs;
pub mod lines;
pub mod myers;
pub mod select;

pub use histogram::{
    Differ, Hunk, Hunks, MAX_LEN, MAX_RARITY, Match, UNMATCHED, diff, diff_lines, hunks, partners,
};
pub use lcs::lcs_len;
pub use lines::{Interner, Lines, lines};
pub use myers::{Hit, Located, Pattern, Searcher, levenshtein};
pub use select::Selector;

/// A sequence or id space exceeds the `u32` positions of the diff ([`MAX_LEN`] lines, or `u32::MAX` distinct lines).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LengthError;

impl std::fmt::Display for LengthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "more lines than the diff's u32 positions can index ({MAX_LEN})"
        )
    }
}

impl std::error::Error for LengthError {}

/// The proptest configuration of a suite whose tier-`pr` case count is `base`: `MOIRAI_TEST_TIER` = `nightly` runs 16
/// times as many and `exit` 64 times as many (PLAN §2.1 test tiers); no failure persistence (the seed is printed).
#[cfg(test)]
pub(crate) fn test_config(base: u32) -> proptest::test_runner::Config {
    let cases = match std::env::var("MOIRAI_TEST_TIER").as_deref() {
        Ok("nightly") => base * 16,
        Ok("exit") => base * 64,
        _ => base,
    };
    proptest::test_runner::Config {
        cases,
        failure_persistence: None,
        ..proptest::test_runner::Config::default()
    }
}
