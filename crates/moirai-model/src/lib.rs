//! The reference model of [60 §4]: state folded from net changesets, the `Store` API commands, the commit DAG and
//! merge, R4 link intent, LQ-3 (lexer, parser, binder and evaluator) and the independent canonical-form encoder. Rule
//! tables are data: `docs/spec/rules/*.md`, read by [`rules`] at test time. It has no JSON code, and it derives
//! `fold_v1` at test time from `fixtures/ucd/17.0.0/` by its own algorithm.
//!
//! Test-only crate with no workspace dependency; checked by GT20 (e) on every target. Filled by WP-90 to WP-94
//! (R-MODEL), whose author never reads engine code (S2). Sources: [60 §4], [50 §8.2]; `docs/m0/PLAN.md` §2.2, §3.2
//! item 9, §6.2 R5.

pub mod api;
pub mod apply;
pub mod budget;
pub mod canon;
pub mod clock;
pub mod confcmd;
pub mod config;
pub mod context;
pub mod coord;
pub mod crash;
pub mod dag;
pub mod delete;
pub mod derived;
pub mod err;
pub mod feed;
pub mod gc;
pub mod heads;
pub mod heap;
pub mod history;
pub mod hooks;
pub mod idem;
pub mod inv;
pub mod lease;
pub mod lq;
pub mod lqh;
pub mod markers;
pub mod merge;
pub mod mutation;
pub mod mvalid;
pub mod pack;
pub mod policy;
pub mod policykeys;
pub mod profile;
pub mod quiet;
pub mod refmove;
pub mod registry;
pub mod rules;
pub mod runs;
pub mod schema;
pub mod state;
pub mod status;
pub mod text3;
pub mod tx;
pub mod value;
pub mod vcs;

#[cfg(test)]
mod suite;
