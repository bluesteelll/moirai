//! `cargo xtask`: the repository's tool runner (a host tool, checked by GT20 (e) on every target). WP-01 creates it.
//! Each subcommand arrives with the work package that `PLANNED` names (docs/m0/PLAN.md §2.2, §3.2 item 7); until
//! then the binary prints the plan and exits with status 2.

use std::process::ExitCode;

/// The planned subcommands: usage, the work package that delivers it, and its purpose.
const PLANNED: &[(&str, &str, &str)] = &[
    (
        "gate [--branch m0/<role>] [--ci]",
        "WP-02",
        "fmt, clippy, tier-pr tests, GT20 (b), (d), (a) and (e), the root checks, licences, AI markers",
    ),
    (
        "host-only --list",
        "WP-02",
        "print the crates of xtask/host-only.toml, one per line",
    ),
    (
        "authors",
        "WP-02",
        "check that each commit touches only paths its WP's role may write",
    ),
    (
        "coverage",
        "WP-02",
        "check docs/spec/COVERAGE.md: cited fixtures exist, model functions carry the spec tag",
    ),
    (
        "hook pre-commit | pr-body",
        "WP-03",
        "the private-manifest, shingle and report checks; the PR-body AI-marker check",
    ),
    (
        "private index",
        "WP-03",
        "rebuild /private/MANIFEST.b3 (file BLAKE3, 8-word shingles, tree digest)",
    ),
    (
        "worktree <role>",
        "WP-01",
        "make the role's worktree and settings, then verify its seeded commits are refused",
    ),
    (
        "hex",
        "WP-20",
        "assemble hex fixtures: hex, labels, {xxh3_64}, {blake3_256} and {len} directives",
    ),
    (
        "ucd",
        "WP-61",
        "generate the fold_v1 tables from fixtures/ucd/17.0.0/",
    ),
    (
        "loadrec",
        "WP-51",
        "record the system-wide _Total counters for measurement 16",
    ),
    (
        "nightly",
        "WP-05",
        "profile L nightly runner: guard pre-checks, window calendar, job list",
    ),
];

fn main() -> ExitCode {
    eprintln!("xtask: no subcommand is implemented yet. Planned (docs/m0/PLAN.md §2.2):");
    for (usage, wp, purpose) in PLANNED {
        eprintln!("  {usage:<34} {wp:<6} {purpose}");
    }
    ExitCode::from(2)
}
