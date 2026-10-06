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
//! | ignore matcher ([F20 §4.4]; WP-61b's acceptance) | `moirai_files::ignore` (`IgnoreStack::check` in `Mode::Git`, `PatternList`) | `git check-ignore --no-index`, verbose (`-v -n`) and plain, under both `core.ignorecase` values, in a scratch repository per test, over named and generated trees with virtual files (absent paths whose names hold `*`, `?` or trailing spaces and dots), and against `Mode::NoGit` on trees git's answers also decide ([`ignorediff`]) | `tests/ignore_git.rs` |
//! | never-candidate patterns ([F20 §4.7.1], §4.7.2; WP-61b) | `moirai_files::ignore` (`matches_name_pattern`, `never_pattern`) | `git check-ignore --no-index -v` with `core.ignorecase = true`, the pattern list reversed in the root `.gitignore` ([`ignorediff::check_names`]) | `tests/ignore_git.rs` |
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
pub mod ignorediff;
pub mod scandiff;
pub mod tsoracle;

/// Writes `text` to the process's standard error itself, past libtest's capture, which takes only what the print
/// macros write: a run without `--nocapture`, the replay job's and the gate's among them, shows it in its log even
/// when the test passes. A line ending is added when `text` has none. A failed write is ignored: the log line is
/// information, never the test's outcome.
pub fn job_log(text: &str) {
    use std::io::Write;
    let mut err = std::io::stderr().lock();
    let end = if text.ends_with('\n') { "" } else { "\n" };
    let _ = err
        .write_all(format!("{text}{end}").as_bytes())
        .and_then(|()| err.flush());
}
