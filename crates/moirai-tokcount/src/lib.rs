//! Offline o200k token counts, and the headless Claude Code invocation (the native `claude.exe`, the model pinned to
//! Opus 5.5 with `--model`, a runner-owned `CLAUDE_CONFIG_DIR`, prompts through files or stdin) with the parse of its
//! reported usage.
//!
//! Tool crate; checked by GT20 (e) on every target. Filled by WP-58 (R-HARN-M). Sources: [90 §8.3], [60 §5.2] rows 6
//! and 20; `docs/m0/PLAN.md` §2.2, §3.2 item 5, §6.1 #6 and #7.
//!
//! - [`o200k`] counts text with the public o200k_base vocabulary, offline ([90 §8.3] "tokenizer ledger", [90 §9]).
//! - [`claude`] runs one headless Claude Code call on the owner's subscription ([90 §8.3], PLAN §6.1 #6) and parses
//!   the usage it reports; [`claude::text_tokens`] turns two calls into a text's Claude token count, the difference
//!   with and without the text (PLAN §6.1 #7, [LQ/card §7.2]).
//! - [`fake`] is the test double the tier-`pr` tests run in place of Claude Code (PLAN WP-58), here and in the
//!   crates that build on the invocation (WP-71b).
//!
//! The binary `tokcount` exposes the first two to the owner; the binary `fake-claude` wraps [`fake`].

pub mod claude;
pub mod fake;
pub mod o200k;
