//! The commands of the three job kinds (`docs/m0/nightly.md` §5) and what the runner reads back from them.
//!
//! - **cargo-test** (GT1 full, GT18 long): `cargo test --locked --no-fail-fast -p …` in the build lane's target
//!   directory, under the gate's poisoned C toolchain variables (so the lane's warm build is shared with the gate's
//!   `test` step, PLAN §2.1), at the job's `MOIRAI_TEST_TIER`.
//! - **mutants** (GT16's sample): one shard of `cargo mutants -p …`, `--shard k/n --sharding round-robin`, with k the
//!   job's turn modulo n, so successive runs of the job walk every shard; `CARGO_TARGET_DIR` removed and `TMP`/`TEMP` in
//!   the mutants directory, never `--in-place`, gitignored files left out of the scratch copies (`--gitignore true`),
//!   and no jobserver of cargo-mutants' own (`--jobserver false`), so each of the `--jobs` cargo processes keeps to its
//!   share of `CARGO_BUILD_JOBS` (`docs/m0/tools.md` §5).
//! - **fuzz** (GT5): at most two targets of `fuzz/Cargo.toml`, rotating by the job's turn, each built and then run with
//!   `cargo fuzz run -s none <target> -- -rss_limit_mb=<n> -max_total_time=<s>` from `fuzz/` on its pinned nightly, in
//!   the fuzz directory, unpoisoned (libFuzzer is C++; `docs/m0/tools.md` §4.2–§4.5).
//!
//! A job's **turn** counts the earlier runs in which that job started (`docs/m0/nightly.md` §5), so the rotation does
//! not depend on how the windows fall on calendar days.

use crate::cargo;
use crate::lint_fuzz;
use crate::nightly::config::{KindCaps, Tier};
use crate::nightly::exec::Cmd;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Where and how the jobs of one run build and test.
#[derive(Clone, Debug)]
pub struct Ctx {
    pub repo: PathBuf,
    /// The target root (`git config moirai.target-root`, PLAN §2.1).
    pub root: PathBuf,
    /// The build lane's target directory.
    pub lane_dir: PathBuf,
    /// The caps of the window's kind.
    pub caps: KindCaps,
    /// The job's turn (the module header), which rotates the fuzz targets and the mutants shard.
    pub turn: u64,
}

impl Ctx {
    /// The environment every cargo of the root workspace gets: the lane, the build-jobs cap, the test-threads cap and
    /// the gate's poisoned C toolchain (PLAN §2.1).
    // spec: [PLAN §3.2 WP-05] (the CARGO_BUILD_JOBS cap), [PLAN §2.1] (target directories, the poisoned environment)
    fn root_cargo(&self, sub: &[&str]) -> Cmd {
        Cmd::new("cargo", &self.repo)
            .args(sub)
            .envs(cargo::poisoned_env())
            .env("CARGO_TARGET_DIR", self.lane_dir.to_string_lossy())
            .env("CARGO_BUILD_JOBS", self.caps.build_jobs.to_string())
            .env("RUST_TEST_THREADS", self.caps.test_threads.to_string())
            .env("CARGO_TERM_COLOR", "never")
    }

    /// The fuzz directory under the target root (`docs/m0/tools.md` §12).
    pub fn fuzz_dir(&self) -> PathBuf {
        self.root.join("fuzz")
    }

    /// The mutants directory under the target root (`docs/m0/tools.md` §5, §12).
    pub fn mutants_dir(&self) -> PathBuf {
        self.root.join("mutants")
    }
}

fn packages(c: Cmd, packages: &[String]) -> Cmd {
    c.args(packages.iter().flat_map(|p| ["-p".to_string(), p.clone()]))
}

/// `cargo build` of the guard binary into the build lane ([MP §8]); the runner copies it out of cargo's way.
pub fn guard_build(ctx: &Ctx) -> Cmd {
    ctx.root_cargo(&[
        "build",
        "--locked",
        "-p",
        "moirai-probes-bin",
        "--bin",
        "guard",
    ])
}

/// Where [`guard_build`] leaves the guard.
pub fn guard_built(ctx: &Ctx) -> PathBuf {
    ctx.lane_dir
        .join("debug")
        .join(format!("guard{}", std::env::consts::EXE_SUFFIX))
}

/// A cargo-test job's command.
// spec: [PLAN §3.2 WP-05] (GT1 full, GT18 long), [PLAN §2.1] (MOIRAI_TEST_TIER)
pub fn cargo_test(ctx: &Ctx, pkgs: &[String], tier: Tier, args: &[String]) -> Cmd {
    let mut c = packages(
        ctx.root_cargo(&["test", "--locked", "--no-fail-fast"]),
        pkgs,
    )
    .env("MOIRAI_TEST_TIER", tier.as_str());
    if !args.is_empty() {
        c = c.args(["--"]).args(args);
    }
    c
}

/// The mutants shard of a turn: `turn mod shards`, 0-based as cargo-mutants counts.
pub fn shard(turn: u64, shards: u32) -> u32 {
    (turn % u64::from(shards.max(1))) as u32
}

/// One process's share of a cap that `parallel` processes split: `cap / parallel`, at least 1.
pub fn share(cap: u32, parallel: u32) -> u32 {
    (cap / parallel.max(1)).max(1)
}

/// A mutants job's command; `out` receives `mutants.out/`. cargo-mutants runs `--jobs` cargo processes side by side;
/// with its own GNU jobserver (on by default, NCPUS tokens) they would take tokens from it instead of honouring
/// `CARGO_BUILD_JOBS`, so it is turned off and each process gets its share of the window's caps. `--gitignore true`
/// keeps gitignored files (`/private/`, `fuzz/corpus/`, `graphify-out/`) out of the scratch copies, so no owner data
/// is copied out of `/private/` (cargo-mutants 27.1.0 copies them by default).
// spec: [PLAN §3.2 WP-05] (GT16 sample; the CARGO_BUILD_JOBS cap), [60 §3.13] GT16, docs/m0/tools.md §5 (the
// cargo-mutants rules), AGENTS.md "Data"
pub fn mutants(ctx: &Ctx, pkgs: &[String], shards: u32, tier: Tier, out: &Path) -> Cmd {
    let tmp = ctx.mutants_dir().to_string_lossy().into_owned();
    let c = Cmd::new("cargo", &ctx.repo)
        .args(["mutants", "--colors", "never", "--shard"])
        .args([format!("{}/{shards}", shard(ctx.turn, shards))])
        .args([
            "--sharding",
            "round-robin",
            "--gitignore",
            "true",
            "--jobserver",
            "false",
            "--jobs",
        ])
        .args([
            ctx.caps.parallel.to_string(),
            "--output".to_string(),
            out.to_string_lossy().into_owned(),
        ]);
    packages(c, pkgs)
        .envs(cargo::poisoned_env())
        .remove("CARGO_TARGET_DIR")
        .env("TMP", tmp.clone())
        .env("TEMP", tmp)
        .env(
            "CARGO_BUILD_JOBS",
            share(ctx.caps.build_jobs, ctx.caps.parallel).to_string(),
        )
        .env(
            "RUST_TEST_THREADS",
            share(ctx.caps.test_threads, ctx.caps.parallel).to_string(),
        )
        .env("MOIRAI_TEST_TIER", tier.as_str())
        .env("CARGO_TERM_COLOR", "never")
}

/// Whether a directory name in the mutants directory is one of cargo-mutants' scratch copies
/// (`cargo-mutants-<tree>-<random>.tmp`).
pub fn is_scratch_copy(name: &str) -> bool {
    name.starts_with("cargo-mutants-") && name.ends_with(".tmp")
}

/// Removes the scratch copies an earlier mutants job left in `dir` (one stopped at its deadline or by the watchdog
/// cannot delete them, and each holds its own `target\`); returns the names removed. Only the run lock and an
/// agent-free window rule out another user of the directory, so the runner calls it in agent-free windows only.
// spec: [PLAN §2.1] (cargo-mutants' own capped directory), docs/m0/tools.md §5
pub fn clear_scratch(dir: &Path) -> Result<Vec<String>, String> {
    let rd = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("{}: {e}", dir.display())),
    };
    let mut names: Vec<String> = rd
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| is_scratch_copy(n))
        .collect();
    names.sort();
    for n in &names {
        std::fs::remove_dir_all(dir.join(n))
            .map_err(|e| format!("{}: {e}", dir.join(n).display()))?;
    }
    Ok(names)
}

/// The counts of `<out>/mutants.out/outcomes.json` (`total_mutants`, `caught`, `missed`, `timeout`, `unviable`) and
/// the share caught of those caught or missed, or why they cannot be read.
pub fn mutants_counts(out: &Path) -> Result<Value, String> {
    let p = out.join("mutants.out").join("outcomes.json");
    let text = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
    let v: Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", p.display()))?;
    let n = |k: &str| v[k].as_u64();
    let (caught, missed) = (n("caught"), n("missed"));
    Ok(json!({
        "total": n("total_mutants"),
        "caught": caught,
        "missed": missed,
        "timeout": n("timeout"),
        "unviable": n("unviable"),
        "caught_share": match (caught, missed) {
            (Some(c), Some(m)) if c + m > 0 => json!(c as f64 / (c + m) as f64),
            _ => Value::Null,
        },
        "cargo_mutants_version": v["cargo_mutants_version"],
    }))
}

/// The status of a mutants run from its exit code: cargo-mutants exits 0 when every mutant was caught, 2 when some
/// were missed and 3 when some timed out; those are results of the sample, which GT16 judges per exit, not failures of
/// the night. Any other code (1 usage, 4 the unmutated tests failed, 70 an internal error) is a failure.
// spec: [60 §3.13] GT16 (the kill rate judged per exit), [PLAN §3.2 WP-05] (GT16 sample)
pub fn mutants_passed(code: i32) -> bool {
    matches!(code, 0 | 2 | 3)
}

/// The fuzz targets: the `[[bin]]` names of `fuzz/Cargo.toml`, sorted (none before FL-1's first target).
pub fn fuzz_targets(repo: &Path) -> Result<Vec<String>, String> {
    let p = repo.join("fuzz/Cargo.toml");
    if !p.is_file() {
        return Ok(Vec::new());
    }
    let t = crate::config::read_toml(&p)?;
    let mut names = BTreeSet::new();
    if let Some(bins) = t.get("bin").and_then(|b| b.as_array()) {
        for b in bins {
            let name = b
                .get_path(&["name"])
                .and_then(|n| n.as_str())
                .ok_or("fuzz/Cargo.toml: a [[bin]] entry without a name")?;
            names.insert(name.to_string());
        }
    }
    Ok(names.into_iter().collect())
}

/// A turn's targets: `k` consecutive names from position `turn × k mod n`, wrapping, so successive runs take every
/// target in turn (PLAN WP-05: at most two).
// spec: [PLAN §3.2 WP-05] (fuzz ≤ 2 targets)
pub fn rotate(targets: &[String], k: u32, turn: u64) -> Vec<String> {
    let n = targets.len();
    if n == 0 {
        return Vec::new();
    }
    let k = (k as usize).min(n);
    let start = ((turn % n as u64) as usize * k) % n;
    (0..k).map(|i| targets[(start + i) % n].clone()).collect()
}

/// The fuzz workspace's pinned channel (`fuzz/rust-toolchain.toml`, `docs/m0/tools.md` §2.2).
pub fn fuzz_channel(repo: &Path) -> Result<String, String> {
    let p = repo.join("fuzz/rust-toolchain.toml");
    let text = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
    lint_fuzz::toolchain_channel(&text)
}

fn fuzz_cargo(ctx: &Ctx, channel: &str) -> Cmd {
    // RUSTUP_TOOLCHAIN outranks fuzz/rust-toolchain.toml and the runner inherits the root's (tools.md §4.2).
    Cmd::new("cargo", &ctx.repo.join("fuzz"))
        .env("RUSTUP_TOOLCHAIN", channel)
        .env("CARGO_TARGET_DIR", ctx.fuzz_dir().to_string_lossy())
        .env("CARGO_BUILD_JOBS", ctx.caps.build_jobs.to_string())
        .env("CARGO_TERM_COLOR", "never")
}

/// Builds one fuzz target, sanitizer off (`docs/m0/tools.md` §4.2: debug assertions on, as cargo-fuzz builds without
/// `-O`).
pub fn fuzz_build(ctx: &Ctx, channel: &str, target: &str) -> Cmd {
    fuzz_cargo(ctx, channel).args(["fuzz", "build", "-s", "none", target])
}

/// Runs one fuzz target for `secs` seconds under libFuzzer's RSS limit (PLAN WP-05: sanitizer off,
/// `-rss_limit_mb=256`).
// spec: [PLAN §3.2 WP-05] (fuzz sanitizer-off -rss_limit_mb=256)
pub fn fuzz_run(ctx: &Ctx, channel: &str, target: &str, secs: i64, rss_limit_mb: u32) -> Cmd {
    fuzz_cargo(ctx, channel).args([
        "fuzz".to_string(),
        "run".to_string(),
        "-s".to_string(),
        "none".to_string(),
        target.to_string(),
        "--".to_string(),
        format!("-rss_limit_mb={rss_limit_mb}"),
        format!("-max_total_time={secs}"),
        "-print_final_stats=1".to_string(),
    ])
}

/// The file names in `fuzz/artifacts/<target>/` (crash and out-of-memory inputs, `docs/m0/tools.md` §4.5).
pub fn artifacts(repo: &Path, target: &str) -> BTreeSet<String> {
    std::fs::read_dir(repo.join("fuzz/artifacts").join(target))
        .map(|d| {
            d.filter_map(Result::ok)
                .filter_map(|e| e.file_name().into_string().ok())
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testdir::TestDir;
    use proptest::prelude::*;

    fn ctx() -> Ctx {
        Ctx {
            repo: PathBuf::from("/repo"),
            root: PathBuf::from("/t"),
            lane_dir: PathBuf::from("/t/laneA"),
            caps: KindCaps {
                build_jobs: 2,
                test_threads: 3,
                parallel: 1,
                ram_budget: 1_000_000_000,
            },
            turn: 20_000,
        }
    }

    fn strs(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn cargo_test_commands() {
        let c = cargo_test(
            &ctx(),
            &strs(&["moirai-vfs-sim", "moirai-toylog"]),
            Tier::Nightly,
            &[],
        );
        assert_eq!(
            c.line(),
            "cargo test --locked --no-fail-fast -p moirai-vfs-sim -p moirai-toylog"
        );
        assert_eq!(c.cwd, PathBuf::from("/repo"));
        assert_eq!(c.get_env("MOIRAI_TEST_TIER"), Some("nightly"));
        assert_eq!(c.get_env("CARGO_BUILD_JOBS"), Some("2"));
        assert_eq!(c.get_env("RUST_TEST_THREADS"), Some("3"));
        assert_eq!(c.get_env("CARGO_TARGET_DIR"), Some("/t/laneA"));
        assert_eq!(
            c.get_env("CC"),
            Some(cargo::POISON),
            "the gate's poisoned environment"
        );
        let c = cargo_test(
            &ctx(),
            &strs(&["m"]),
            Tier::Pr,
            &strs(&["gt18", "--skip", "slow"]),
        );
        assert!(c.line().ends_with("-p m -- gt18 --skip slow"));
        let g = guard_build(&ctx());
        assert_eq!(
            g.line(),
            "cargo build --locked -p moirai-probes-bin --bin guard"
        );
        assert!(guard_built(&ctx()).starts_with("/t/laneA/debug"));
    }

    #[test]
    fn mutants_commands_rotate_their_shard() {
        let out = PathBuf::from("/p/nightly/run/gt16");
        let c = mutants(
            &ctx(),
            &strs(&["moirai-files", "moirai-diff"]),
            32,
            Tier::Pr,
            &out,
        );
        // Gitignored files stay out of the scratch copies, and no jobserver of cargo-mutants' own outranks
        // CARGO_BUILD_JOBS (docs/m0/tools.md §5).
        assert_eq!(
            c.line(),
            format!(
                "cargo mutants --colors never --shard {}/32 --sharding round-robin --gitignore true --jobserver false --jobs 1 --output {} -p moirai-files -p moirai-diff",
                20_000 % 32,
                out.to_string_lossy()
            )
        );
        assert_eq!(c.env_remove, ["CARGO_TARGET_DIR"]);
        assert_eq!(c.get_env("CARGO_TARGET_DIR"), None);
        assert_eq!(
            c.get_env("TMP"),
            Some(&*PathBuf::from("/t").join("mutants").to_string_lossy())
        );
        assert_eq!(c.get_env("TEMP"), c.get_env("TMP"));
        assert_eq!(c.get_env("MOIRAI_TEST_TIER"), Some("pr"));
        assert_eq!(c.get_env("CARGO_BUILD_JOBS"), Some("2"));
        assert_eq!(c.get_env("RUST_TEST_THREADS"), Some("3"));
        // Two processes side by side share the window's caps: the job's total stays within them.
        let mut wide = ctx();
        wide.caps = KindCaps {
            build_jobs: 6,
            test_threads: 4,
            parallel: 2,
            ram_budget: 6_000_000_000,
        };
        let c = mutants(&wide, &strs(&["moirai-files"]), 32, Tier::Pr, &out);
        assert!(
            c.line().contains("--jobserver false --jobs 2 "),
            "{}",
            c.line()
        );
        assert_eq!(c.get_env("CARGO_BUILD_JOBS"), Some("3"));
        assert_eq!(c.get_env("RUST_TEST_THREADS"), Some("2"));
        assert_eq!((share(5, 2), share(1, 2), share(3, 0)), (2, 1, 3));
        assert_eq!(
            (shard(0, 32), shard(31, 32), shard(32, 32), shard(65, 32)),
            (0, 31, 0, 1)
        );
        assert!(mutants_passed(0) && mutants_passed(2) && mutants_passed(3));
        assert!(
            !mutants_passed(1) && !mutants_passed(4) && !mutants_passed(70) && !mutants_passed(-1)
        );
    }

    #[test]
    fn leftover_scratch_copies_are_removed() {
        let d = TestDir::new("nightly-jobs-scratch");
        assert_eq!(clear_scratch(&d.path().join("none")), Ok(Vec::new()));
        d.write("cargo-mutants-moirai-AbC123.tmp/target/debug/x.rlib", "x");
        d.write("cargo-mutants-moirai-ZZ.tmp/src/lib.rs", "y");
        d.write("cargo-mutants-not-a-copy.log", "a file, kept");
        d.write("keep/cargo-mutants-inner.tmp/z", "nested, kept");
        d.write("other.tmp/w", "kept");
        assert_eq!(
            clear_scratch(d.path()),
            Ok(strs(&[
                "cargo-mutants-moirai-AbC123.tmp",
                "cargo-mutants-moirai-ZZ.tmp"
            ]))
        );
        let mut left: Vec<String> = std::fs::read_dir(d.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        left.sort();
        assert_eq!(
            left,
            strs(&["cargo-mutants-not-a-copy.log", "keep", "other.tmp"])
        );
        assert!(is_scratch_copy("cargo-mutants-mut-InJE9a.tmp") && !is_scratch_copy("mutants.out"));
    }

    #[test]
    fn mutants_outcomes() {
        let d = TestDir::new("nightly-jobs-mutants");
        d.write(
            "mutants.out/outcomes.json",
            r#"{"outcomes":[],"total_mutants":116,"missed":7,"caught":101,"timeout":6,"unviable":2,"success":0,"cargo_mutants_version":"27.1.0"}"#,
        );
        let v = mutants_counts(d.path()).unwrap();
        assert_eq!(
            (
                v["total"].as_u64(),
                v["caught"].as_u64(),
                v["missed"].as_u64()
            ),
            (Some(116), Some(101), Some(7))
        );
        assert!((v["caught_share"].as_f64().unwrap() - 101.0 / 108.0).abs() < 1e-12);
        assert_eq!(v["cargo_mutants_version"], "27.1.0");
        assert!(mutants_counts(&d.path().join("none")).is_err());
    }

    #[test]
    fn fuzz_commands() {
        let c = fuzz_run(&ctx(), "nightly-2026-09-27", "path_spec", 1_800, 256);
        assert_eq!(
            c.line(),
            "cargo fuzz run -s none path_spec -- -rss_limit_mb=256 -max_total_time=1800 -print_final_stats=1"
        );
        assert_eq!(c.cwd, PathBuf::from("/repo").join("fuzz"));
        assert_eq!(c.get_env("RUSTUP_TOOLCHAIN"), Some("nightly-2026-09-27"));
        assert_eq!(
            c.get_env("CARGO_TARGET_DIR"),
            Some(&*PathBuf::from("/t").join("fuzz").to_string_lossy())
        );
        assert_eq!(
            c.get_env("CC"),
            None,
            "libFuzzer's C++ needs the real compiler"
        );
        assert_eq!(
            fuzz_build(&ctx(), "n", "t").line(),
            "cargo fuzz build -s none t"
        );
    }

    #[test]
    fn fuzz_targets_come_from_the_manifest() {
        let d = TestDir::new("nightly-jobs-fuzz");
        assert_eq!(fuzz_targets(d.path()), Ok(Vec::new()));
        d.write(
            "fuzz/Cargo.toml",
            "[package]\nname = \"moirai-fuzz\"\n[lib]\npath = \"src/lib.rs\"\n",
        );
        assert_eq!(fuzz_targets(d.path()), Ok(Vec::new()));
        d.write(
            "fuzz/Cargo.toml",
            "[package]\nname = \"f\"\n[[bin]]\nname = \"spec_parse\"\npath = \"a.rs\"\n[[bin]]\nname = \"anchor_select\"\npath = \"b.rs\"\n",
        );
        assert_eq!(
            fuzz_targets(d.path()),
            Ok(strs(&["anchor_select", "spec_parse"]))
        );
        d.write("fuzz/Cargo.toml", "[[bin]]\npath = \"a.rs\"\n");
        assert!(fuzz_targets(d.path()).is_err());
        d.write("fuzz/artifacts/t/crash-1", "x");
        d.write("fuzz/artifacts/t/oom-2", "y");
        assert_eq!(
            artifacts(d.path(), "t").into_iter().collect::<Vec<_>>(),
            strs(&["crash-1", "oom-2"])
        );
        assert!(artifacts(d.path(), "none").is_empty());
        // The repository's pin reads.
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        assert!(fuzz_channel(repo).unwrap().starts_with("nightly-"));
    }

    #[test]
    fn rotation_examples() {
        let t = strs(&["a", "b", "c", "d", "e"]);
        assert_eq!(rotate(&t, 2, 0), strs(&["a", "b"]));
        assert_eq!(rotate(&t, 2, 1), strs(&["c", "d"]));
        assert_eq!(rotate(&t, 2, 2), strs(&["e", "a"]));
        assert_eq!(rotate(&strs(&["only"]), 2, 7), strs(&["only"]));
        assert!(rotate(&[], 2, 7).is_empty());
    }

    proptest! {
        /// A turn's targets are distinct, at most k, and over n successive turns every target comes up.
        #[test]
        fn rotation_covers_every_target(n in 1usize..12, k in 1u32..=2, turn0 in 0u64..1_000_000) {
            let t: Vec<String> = (0..n).map(|i| format!("t{i}")).collect();
            let mut seen = BTreeSet::new();
            for turn in turn0..turn0 + n as u64 {
                let r = rotate(&t, k, turn);
                prop_assert_eq!(r.len(), (k as usize).min(n));
                prop_assert_eq!(r.iter().collect::<BTreeSet<_>>().len(), r.len());
                seen.extend(r);
            }
            prop_assert_eq!(seen.len(), n);
        }

        /// Over `shards` successive turns every shard comes up exactly once.
        #[test]
        fn shards_cover_the_sample(shards in 1u32..200, turn0 in 0u64..1_000_000) {
            let got: BTreeSet<u32> = (turn0..turn0 + u64::from(shards)).map(|t| shard(t, shards)).collect();
            prop_assert_eq!(got.len(), shards as usize);
            prop_assert!(got.iter().all(|&s| s < shards));
        }
    }
}
