//! The replay-corpora harness: manifests, git CLI extraction as a test-only data source, and the targets report;
//! FL-1's differential tests against `git hash-object`, `git check-ignore` and the JSON output of `moirai-tsoracle`;
//! and the P11 differential.
//!
//! Test-only crate; checked by GT20 (e) on every target. Filled by WP-74 to WP-77 (R-REPLAY). Sources: [40 §8.3.4],
//! [60 §3.1] item 12; `docs/m0/PLAN.md` §2.2, §6.2 R16, R18.
//!
//! # FL-1's differentials (WP-74)
//!
//! Product crates spawn nothing (PLAN §2.1 GT20 (a), §6.2 R18), so every differential of FL-1 against an external
//! program lives here, in test code, over synthetic inputs and the repository's own files only — never owner data.
//!
//! | Differential | Product side | Reference | Test |
//! |---|---|---|---|
//! | `oid` ([F20 §2.1–§2.4]; WP-62's acceptance) | `moirai_files::text` (`oid_of`, `analyse`, `ContentReader`), `moirai_files::oid::blob_oid` | `git hash-object` in SHA-1 and SHA-256 scratch repositories ([`git`]) | `tests/oid_git.rs` |
//! | Rust scope scanner ([F21 §3.9]; WP-63's acceptance; [40 §8.3.4] row 8) | `moirai_files::scan` (`scan`, and `Scanner` fed in chunks) | `moirai-tsoracle`'s claimed items ([`tsoracle`], compared by [`scandiff`]), and the construction of generated sources | `tests/scan_oracle.rs` |
//! | Markdown and TOML scope scanners ([F21 §4], §5) | `moirai_files::scan` (`scan`, and `Scanner` fed in chunks) | the construction of generated documents only: no M0 oracle has these grammars | `tests/scan_synth.rs` |
//! | gitignore and never-candidate matchers (WP-61b) | — | `git check-ignore` | waits for WP-61b |
//!
//! **Where they run.** The replay job of `pr.yml` builds `moirai-tsoracle` unpoisoned and then runs this crate's
//! tests (`cargo test --locked -p moirai-tsoracle -p moirai-replay`, `MOIRAI_TEST_TIER=pr`; `docs/m0/tools.md` §8).
//! That job must set `MOIRAI_TSORACLE: target/debug/moirai-tsoracle.exe` ([`tsoracle::ENV`]; `pr.yml` is R-HARN's):
//! with it, a missing or stale oracle fails the job; without it, the oracle differentials skip and pass. Either way
//! each oracle differential writes its outcome line past libtest's capture, so the log of every run, without
//! `--nocapture` too, shows whether the oracle half ran and the repository differential's counts and agreement rates.
//! The gate also runs the tests, poisoned and without building the oracle, with the variable unset: the git and
//! construction differentials run there too, and each oracle differential passes with a skip line unless this work
//! tree's current oracle binary is present ([`tsoracle::locate`]). `MOIRAI_TEST_TIER` = `nightly` or `exit` widens
//! the generated cases (PLAN §2.1).
//!
//! **Isolation.** The integration tests' scratch repositories and generated files live only under the target
//! directory's temporary directory (`CARGO_TARGET_TMPDIR`); unit tests, which have none, use the system's temporary
//! directory. Both are removed when a test ends, failed or not; git runs with the host's configuration shut out
//! ([`git`]).
//!
//! **Reports.** The scope-scanner report names the items of every disagreeing file except in the crates some role
//! must not read ([`scandiff::COUNTS_ONLY`]: every `deny_read` path of `xtask/roles.toml`, PLAN §3.1), which it
//! counts only; `MOIRAI_REPLAY_NAME_ITEMS=1` ([`scandiff::NAME_ITEMS_ENV`]) names them too, for an author who may read
//! every crate, in their own work tree.

pub mod git;
pub mod scandiff;
pub mod tsoracle;
