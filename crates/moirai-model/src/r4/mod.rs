//! R4 in the reference model ([60 §4.2] row "File links"; [40 §2], §4, §5, §8.3.2; `docs/m0/PLAN.md` §3.2 WP-92 and
//! §6.2 R16): link intent as data, and the definitional counterparts of FL-1 and of the M6 resolver.
//!
//! - [`ucd`], [`fold`]: `fold_v1` derived at run time from the pinned UCD text by the model's own algorithm ([F20 §3]).
//! - [`text`]: the content functions of [F20 §2] (`is_text`, `norm`, `oid`, anchor text and lines, fingerprints, the
//!   window value, the span hash and the header text, the git pair score).
//! - [`path`]: P1–P12 of [OS/path] over data (I-F8).
//! - [`uid`]: the derived identities of [F08 §11]: file, root and anchor uids, registration with its predecessor and
//!   dead-uid rule, anchor capture's predecessor term.
//! - [`anchor`]: the anchor record of [F08 §10.3], capture by definition ([F20 §6.1]) and the brute-force anchor
//!   resolver that enumerates every occurrence (the P11 oracle, [F20 §6.2]–§6.6).
//! - [`tree`]: simulated project trees ([API §6.5]): bytes, file and directory ids, creation times, `VolumeCaps`.
//! - [`git`]: git history as abstract data ([API §6.6]): commits with parents, committer times and `path → blob id`
//!   maps, refs, one HEAD per tree; ancestry, windows, per-commit exact renames and chains ([F20 §5.11]).
//! - [`strings`]: the frozen strings of R-16 and the `relink` vocabulary of R-17 ([F18 §4], §5).
//! - [`cascade`]: exact-evidence resolution of a file node in a tree by definition: the tree gate G1–G4, E1 and E3d–E8,
//!   the copy rule, twins and spellings, classification ([40 §4.3], [F20 §3.5]–§5).
//! - [`settle`]: the writer-tree predicate, freshness, `main`'s committed-only rule, the write rule and the settles of
//!   link conflicts after a merge ([40 §5.3], [F18 §2.6], §3.6; [RULES/link-merge-rules] LV, LH rows).
//!
//! The Store API commands of group F, `EnvTree` and `EnvGit` run these functions on the store: [`crate::links`].
//! - [`mentions`]: `links mentions`: textual mentions of moved-away paths through aliases and `path_moves` ([40 §3.7]).
//! - [`link`]: the link state of an anchor, its file state refined by its anchor state ([F18 §4.4]).
//! - [`inv`]: the invariants I-F1…I-F14 as predicates ([F18 §2]; [F13 §3.9]'s `r4::` model functions).

pub mod anchor;
pub mod cascade;
pub mod fold;
pub mod git;
pub mod ignore;
pub mod inv;
pub mod link;
pub mod mentions;
pub mod path;
pub mod settle;
pub mod strings;
pub mod text;
pub mod tree;
pub mod ucd;
pub mod uid;

pub use inv::*;

#[cfg(test)]
mod props;

#[cfg(test)]
mod replays;

#[cfg(test)]
pub(crate) mod tests {
    use crate::canon::fixtures::{Case, parse_cases};
    use proptest::test_runner::{Config as ProptestConfig, RngAlgorithm, TestRng, TestRunner};
    use std::path::PathBuf;

    /// The directory of the R4 fixtures.
    pub fn dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/r4/cases")
    }

    /// Every case of one file of `fixtures/r4/cases/` ([`fixtures/r4/INDEX.md`] §2).
    pub fn cases(file: &str) -> Vec<Case> {
        let p = dir().join(file);
        let text = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
        parse_cases(&text)
    }

    /// Bytes of a hexadecimal text.
    pub fn unhex(s: &str) -> Vec<u8> {
        let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex"))
            .collect()
    }

    /// A property-test runner for the tier `MOIRAI_TEST_TIER` names (`docs/m0/PLAN.md` §2.1: `pr` by default,
    /// `nightly`, `exit`), from a fixed seed per tier, with nothing persisted.
    pub fn runner(cases: u32) -> TestRunner {
        let (tier, scale) = match std::env::var("MOIRAI_TEST_TIER").as_deref() {
            Ok("nightly") => (2u8, 10),
            Ok("exit") => (3u8, 100),
            _ => (1u8, 1),
        };
        let mut seed = *b"moirai-model/r4/proptest-seed-v1";
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
}
