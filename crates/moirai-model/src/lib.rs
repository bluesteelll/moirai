//! The reference model of [60 §4]: state folded from net changesets, the `Store` API commands, the commit DAG and
//! merge, R4 link intent, LQ-3 (lexer, parser, binder and evaluator) and the independent canonical-form encoder. Rule
//! tables are data under `rules/`. It has no JSON code, and it derives `fold_v1` at test time from
//! `fixtures/ucd/17.0.0/` by its own algorithm.
//!
//! Test-only crate with no workspace dependency; checked by GT20 (e) on every target. Filled by WP-90 to WP-94
//! (R-MODEL), whose author never reads engine code (S2). Sources: [60 §4], [50 §8.2]; `docs/m0/PLAN.md` §2.2, §3.2
//! item 9, §6.2 R5.

pub mod lq;
