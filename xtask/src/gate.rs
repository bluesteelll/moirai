//! `cargo xtask gate`: the local pre-merge gate and the CI gate (docs/m0/PLAN.md WP-02, §2.1, §3.1; [90 §11.1],
//! [90 §11.2]; [60 §3.13] GT20; [80 §5.5]).
//!
//! Steps, run in this order; every step runs and reports pass or fail, and the gate fails if any step fails:
//!
//! | Step | What |
//! |---|---|
//! | `lock` | `Cargo.lock` is current; with `--branch`, cargo resolves the branch's manifests first (the gate worktree is the only place the lockfile changes, PLAN §3.1) |
//! | `fmt` | `cargo fmt --all -- --check` (every member: formatting compiles nothing) |
//! | `clippy` | `cargo clippy --workspace --all-targets --locked` with one `--exclude` per host-only crate and present root, `-D warnings`; with each product crate's `clippy.toml`, the type-aware GT20 (d) layer |
//! | `test` | `cargo test --workspace --locked --no-fail-fast` with the same exclusions, `MOIRAI_TEST_TIER=pr` |
//! | `gt20-e` | `cargo check --workspace --all-targets --locked` with the exclusions, for the four targets except the host's, which the `clippy` step has already type-checked with the same exclusions |
//! | `roots-check` | each present root's `windows_check` from `xtask/roots.toml`, and clippy on it for Windows |
//! | `gt20-b` | the dependency lint: lockfile set, base list, rules 1-4 (`lint_deps`) |
//! | `licence` | the licence lint over both lockfiles |
//! | `gt20-d` | the source scans (`lint_source`), each product crate's `clippy.toml`, and the direct and transitive OS-dependency rules |
//! | `crates` | `xtask/crates.toml` against the members, host-only and root lists; `publish = false`; a warning for a `deny_read` pattern of `xtask/roles.toml` that matches no file of a crate with modules beside its root |
//! | `roots` | the composition-root lint and the workspace lint tables (`lint_roots`) |
//! | `markers` | the AI-marker scan of every commit in the range (`master..HEAD` by default) |
//! | `authors` | `xtask authors` over the same range; with `--branch`, every commit bound to the branch's role |
//! | `private` | the `/private/` manifest, shingle and report checks over every commit of the range, merges through their combined diff |
//! | `hooks` | `.githooks/test-hooks.sh` with this xtask as `MOIRAI_XTASK`: the first layer of the hooks (`commit-msg` in awk, `pre-commit` in sh) on the same cases as their Rust side, and `xtask worktree` end to end; always with `--ci`, elsewhere when the range or the working tree touches `.githooks/` or `xtask/` |
//! | `ps1-bom` | every non-ASCII `.ps1` file starts with a UTF-8 BOM (PLAN §2.5) |
//! | `coverage` | `xtask coverage`; with `--strict-coverage`, every blank cell is a finding (WP-80 pass 2, WP-81b) |
//!
//! Every cargo command runs under the poisoned C toolchain environment ([90 §11.1]) and `--locked`. The build steps
//! run with `--keep-going` (`--no-fail-fast` for tests), so one crate's errors never hide another crate's findings.
//!
//! `--branch m0/<role>` runs in the neutral gate worktree ([PLAN §3.1] "The gate worktree"): the checkout must be
//! detached at the branch's tip (never the authoring worktree), the range is `master..m0/<role>`, and diagnostics
//! on paths the role may not read (`xtask/roles.toml`) are reduced to "crate X: n findings, file a review finding"
//! (`cargo::LineFilter`).
//! `--ci` is the pull-request gate: the range comes from `$GITHUB_EVENT_PATH` (or `--range`).

use crate::authors;
use crate::cargo::{self, Filter, NoFilter};
use crate::config::{Config, Role};
use crate::coverage;
use crate::diag::Diag;
use crate::git;
use crate::lint_deps::{self, DepInputs};
use crate::lint_roots;
use crate::lint_source::{self, CrateSource};
use crate::markers;
use crate::metadata::Metadata;
use crate::paths::Pattern;
use crate::private;
use std::collections::BTreeMap;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

pub const STEPS: &[(&str, &str)] = &[
    (
        "lock",
        "Cargo.lock is current (--locked); with --branch, resolved for the branch's manifests",
    ),
    ("fmt", "cargo fmt --all -- --check"),
    (
        "clippy",
        "cargo clippy --workspace --all-targets --locked (exclusions) -- -D warnings",
    ),
    (
        "test",
        "cargo test --workspace --locked (exclusions), MOIRAI_TEST_TIER=pr",
    ),
    (
        "gt20-e",
        "cargo check --workspace --all-targets --locked (exclusions) for the four targets but the host's (clippy covers it)",
    ),
    (
        "roots-check",
        "the -p <root> Windows checks of xtask/roots.toml",
    ),
    ("gt20-b", "lockfiles, base list, native-allow rules 1-4"),
    (
        "licence",
        "SPDX licences of both lockfiles against xtask/licence-allow.toml",
    ),
    (
        "gt20-d",
        "OS-surface, store-file, std::fs and spawn scans; product clippy.toml files; direct and transitive OS dependencies",
    ),
    (
        "crates",
        "xtask/crates.toml against the workspace; publish = false; deny_read patterns that match nothing",
    ),
    ("roots", "composition-root lint; workspace lint tables"),
    ("markers", "AI markers in every commit of the range"),
    (
        "authors",
        "commit paths against the WP's role (docs/m0/authors.md); with --branch, the branch's role",
    ),
    (
        "private",
        "/private/ manifest, shingles and report scrub over the range, merges included",
    ),
    (
        "hooks",
        "sh .githooks/test-hooks.sh with this xtask (commit-msg and pre-commit)",
    ),
    ("ps1-bom", "UTF-8 BOM on every non-ASCII .ps1"),
    (
        "coverage",
        "docs/spec/COVERAGE.md fixtures and model spec tags; --strict-coverage: no blank cell",
    ),
];

pub struct Opts {
    pub branch: Option<String>,
    /// Apply a role's read filter without the `--branch` checks (an authoring session gating its own work).
    pub role: Option<String>,
    pub ci: bool,
    pub range: Option<String>,
    pub only: Vec<String>,
    pub skip: Vec<String>,
    /// `coverage` fails on blank cells (WP-80 pass 2, WP-81b).
    pub strict_coverage: bool,
}

/// The `--branch` filter: a role's `deny_read` patterns.
pub struct RoleFilter {
    patterns: Vec<Pattern>,
    /// Package name to repository-relative crate directory.
    crate_dirs: BTreeMap<String, String>,
    /// Library target name (as doc-tests name it, `-` read as `_`) to package name.
    libs: BTreeMap<String, String>,
    /// Target name (`-` read as `_`) to every package with a target of that name.
    targets: BTreeMap<String, Vec<String>>,
}

impl RoleFilter {
    pub fn new(role: &Role, crate_dirs: BTreeMap<String, String>) -> RoleFilter {
        RoleFilter {
            patterns: role.deny_read.iter().map(|p| Pattern::new(p)).collect(),
            crate_dirs,
            libs: BTreeMap::new(),
            targets: BTreeMap::new(),
        }
    }

    /// Adds the members' target names (with `-` read as `_`, as test binaries and doc-tests name them).
    pub fn with_targets(mut self, md: &Metadata) -> RoleFilter {
        for p in md.members() {
            for t in &p.targets {
                let n = t.name.replace('-', "_");
                if t.kind.iter().any(|k| k == "lib" || k == "rlib") {
                    self.libs.insert(n.clone(), p.name.clone());
                }
                let owners = self.targets.entry(n).or_default();
                if !owners.contains(&p.name) {
                    owners.push(p.name.clone());
                }
            }
        }
        self
    }
}

impl Filter for RoleFilter {
    fn hides_path(&self, path: &str) -> bool {
        self.patterns.iter().any(|p| p.matches(path))
    }
    fn active(&self) -> bool {
        !self.patterns.is_empty()
    }
    fn lib_owner(&self, target: &str) -> Option<String> {
        self.libs.get(&target.replace('-', "_")).cloned()
    }
    fn target_owners(&self, target: &str) -> Vec<String> {
        self.targets
            .get(&target.replace('-', "_"))
            .cloned()
            .unwrap_or_default()
    }
    fn crate_of(&self, path: &str) -> Option<String> {
        self.crate_dirs
            .iter()
            .filter(|(_, d)| path.starts_with(&format!("{d}/")))
            .max_by_key(|(_, d)| d.len())
            .map(|(n, _)| n.clone())
    }
    fn hides_crate(&self, name: &str) -> bool {
        self.crate_dirs
            .get(name)
            .is_some_and(|d| self.patterns.iter().any(|p| p.covers_dir(d)))
    }
}

/// Applies the filter to lint findings: hidden ones become per-crate counts.
pub fn reduce(diags: Vec<Diag>, f: &dyn Filter) -> (Vec<Diag>, BTreeMap<String, usize>) {
    let mut shown = Vec::new();
    let mut hidden: BTreeMap<String, usize> = BTreeMap::new();
    for d in diags {
        let by_crate = d.krate.as_deref().is_some_and(|k| f.hides_crate(k));
        let by_path = d.path.as_deref().is_some_and(|p| f.hides_path(p));
        if by_crate || by_path {
            let key = d
                .krate
                .clone()
                .or_else(|| d.path.as_deref().and_then(|p| f.crate_of(p)))
                .or_else(|| d.path.clone())
                .unwrap_or_default();
            *hidden.entry(key).or_default() += 1;
        } else {
            shown.push(d);
        }
    }
    (shown, hidden)
}

struct Ctx {
    lock: Result<String, String>,
    deps: std::cell::OnceCell<Result<DepData, String>>,
    repo: PathBuf,
    repo_str: String,
    cfg: Config,
    full: Option<Metadata>,
    range: String,
    filter: Box<dyn Filter>,
    env: Vec<(String, String)>,
    /// `--branch` mode: the branch role's title, which every commit of the range is bound to.
    branch_role: Option<String>,
    /// The host triple, left out of `gt20-e` because the `clippy` step type-checks it.
    host: Option<String>,
    strict_coverage: bool,
    ci: bool,
}

#[derive(Default)]
struct StepResult {
    ok: bool,
    findings: usize,
    /// Findings withheld by the read filter, per crate.
    hidden: BTreeMap<String, usize>,
    /// Passing test results withheld by the read filter, per crate (not findings).
    withheld: BTreeMap<String, usize>,
    note: Option<String>,
}

impl StepResult {
    fn from_diags(diags: Vec<Diag>, f: &dyn Filter) -> StepResult {
        let (shown, hidden) = reduce(diags, f);
        for d in &shown {
            println!("{d}");
        }
        let n = shown.len() + hidden.values().sum::<usize>();
        StepResult {
            ok: n == 0,
            findings: n,
            hidden,
            ..StepResult::default()
        }
    }

    fn err(e: String) -> StepResult {
        println!("error: {e}");
        StepResult {
            ok: false,
            findings: 1,
            ..StepResult::default()
        }
    }

    fn pass() -> StepResult {
        StepResult {
            ok: true,
            ..StepResult::default()
        }
    }

    fn add(&mut self, other: StepResult) {
        self.ok &= other.ok;
        self.findings += other.findings;
        for (k, n) in other.hidden {
            *self.hidden.entry(k).or_default() += n;
        }
        for (k, n) in other.withheld {
            *self.withheld.entry(k).or_default() += n;
        }
    }
}

fn metadata(repo: &Path, extra: &[&str]) -> Result<Metadata, String> {
    let mut cmd = Command::new("cargo");
    cmd.current_dir(repo)
        .args(["metadata", "--format-version", "1", "--locked"])
        .args(extra)
        .stdin(Stdio::null())
        .stderr(Stdio::inherit());
    let out = cmd.output().map_err(|e| format!("cargo metadata: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "cargo metadata {} failed ({})",
            extra.join(" "),
            out.status
        ));
    }
    Metadata::from_json(&String::from_utf8_lossy(&out.stdout))
}

/// Resolves the workspace without `--locked` (and the fuzz workspace, once it has a lockfile), so the gate
/// worktree records the lockfile the branch's manifests need.
fn update_lock(repo: &Path) -> Result<String, String> {
    let mut notes = Vec::new();
    for (lock, manifest) in [
        ("Cargo.lock", None),
        ("fuzz/Cargo.lock", Some("fuzz/Cargo.toml")),
    ] {
        let path = repo.join(lock);
        if manifest.is_some() && !path.is_file() {
            continue;
        }
        let before = std::fs::read(&path).unwrap_or_default();
        let mut cmd = Command::new("cargo");
        cmd.current_dir(repo)
            .args(["metadata", "--format-version", "1"]);
        if let Some(m) = manifest {
            cmd.args(["--manifest-path", m]);
        }
        let out = cmd
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .status()
            .map_err(|e| format!("cargo metadata: {e}"))?;
        if !out.success() {
            return Err(format!(
                "cargo cannot resolve the branch's manifests for {lock} ({out})"
            ));
        }
        let after = std::fs::read(&path).unwrap_or_default();
        notes.push(if before == after {
            format!("{lock} is current")
        } else {
            format!("{lock} updated for the branch's manifests: commit it with the merge")
        });
    }
    Ok(notes.join("; "))
}

/// The range to scan in CI, from the event payload.
pub fn ci_range(event_path: &Path) -> Result<String, String> {
    let text = std::fs::read_to_string(event_path)
        .map_err(|e| format!("{}: {e}", event_path.display()))?;
    let v: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("event JSON: {e}"))?;
    let s = |p: &[&str]| {
        let mut x = &v;
        for k in p {
            x = x.get(*k)?;
        }
        x.as_str().map(str::to_string)
    };
    if let (Some(b), Some(h)) = (
        s(&["pull_request", "base", "sha"]),
        s(&["pull_request", "head", "sha"]),
    ) {
        return Ok(format!("{b}..{h}"));
    }
    if let (Some(b), Some(a)) = (s(&["before"]), s(&["after"])) {
        if b.bytes().all(|c| c == b'0') {
            return Err(
                "push event with no 'before' commit (a new branch): the range cannot be bounded"
                    .into(),
            );
        }
        return Ok(format!("{b}..{a}"));
    }
    Err("the event is neither a pull_request nor a push".into())
}

pub fn run(repo: &Path, o: &Opts) -> Result<bool, String> {
    let started = Instant::now();
    let cfg = Config::load(repo)?;
    let repo_str = repo
        .to_string_lossy()
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_string();
    // The range and the role filter.
    let mut range = o
        .range
        .clone()
        .unwrap_or_else(|| "master..HEAD".to_string());
    let mut role: Option<Role> = match &o.role {
        Some(n) => Some(
            cfg.roles
                .role(n)
                .ok_or_else(|| format!("unknown role '{n}' (xtask/roles.toml)"))?
                .clone(),
        ),
        None => None,
    };
    if let Some(b) = &o.branch {
        let name = b
            .strip_prefix("m0/")
            .ok_or_else(|| format!("--branch takes m0/<role>, not '{b}'"))?;
        let r = cfg
            .roles
            .role(name)
            .ok_or_else(|| format!("unknown role '{name}' (xtask/roles.toml)"))?
            .clone();
        let tip = git::run(repo, &["rev-parse", "--verify", &format!("{b}^{{commit}}")])?
            .trim()
            .to_string();
        let head = git::run(repo, &["rev-parse", "--verify", "HEAD"])?
            .trim()
            .to_string();
        if git::probe(repo, &["symbolic-ref", "-q", "HEAD"]).is_some() || head != tip {
            return Err(format!(
                "--branch {b}: run in the neutral gate worktree, detached at the branch tip ({}): git checkout --detach {b}",
                &tip[..tip.len().min(12)]
            ));
        }
        if o.range.is_none() {
            range = format!("master..{b}");
        }
        role = Some(r);
    }
    if o.ci
        && o.range.is_none()
        && let Ok(ev) = std::env::var("GITHUB_EVENT_PATH")
    {
        range = ci_range(Path::new(&ev))?;
    }
    // The gate worktree is the only place Cargo.lock changes (PLAN §3.1): with --branch, cargo resolves the
    // branch's manifests first; everywhere else the lockfile must already be current.
    let lock = if o.branch.is_some() {
        update_lock(repo)
    } else {
        metadata(repo, &[]).map(|_| "Cargo.lock is current".to_string())
    };
    let full = metadata(repo, &[]).ok();
    let mut crate_dirs = BTreeMap::new();
    if let Some(md) = &full {
        for p in md.members() {
            let d = p.dir();
            let rel = d
                .strip_prefix(&format!("{}/", md.workspace_root))
                .unwrap_or(&d)
                .to_string();
            crate_dirs.insert(p.name.clone(), rel);
        }
    }
    let filter: Box<dyn Filter> = match &role {
        Some(r) => {
            let f = RoleFilter::new(r, crate_dirs);
            Box::new(match &full {
                Some(md) => f.with_targets(md),
                None => f,
            })
        }
        None => Box::new(NoFilter),
    };
    let mut env = cargo::poisoned_env();
    env.push(("MOIRAI_TEST_TIER".into(), "pr".into()));
    let branch_role = o
        .branch
        .as_ref()
        .and(role.as_ref())
        .map(|r| r.title.clone());
    let ctx = Ctx {
        lock,
        deps: std::cell::OnceCell::new(),
        repo: repo.to_path_buf(),
        repo_str,
        cfg,
        full,
        range,
        filter,
        env,
        branch_role,
        host: cargo::host_triple(),
        strict_coverage: o.strict_coverage,
        ci: o.ci,
    };
    println!(
        "xtask gate: {}{}range {}",
        if o.ci { "CI mode, " } else { "" },
        role.as_ref()
            .map(|r| match &o.branch {
                Some(b) => format!("branch {b} ({}), ", r.title),
                None => format!("read filter of {} ({}), ", r.name, r.title),
            })
            .unwrap_or_default(),
        ctx.range
    );
    let mut results: Vec<(&str, StepResult, f64)> = Vec::new();
    for (name, what) in STEPS {
        if (!o.only.is_empty() && !o.only.iter().any(|x| x == name))
            || o.skip.iter().any(|x| x == name)
        {
            continue;
        }
        println!("\n== {name}: {what}");
        let t = Instant::now();
        let r = step(&ctx, name);
        for (k, n) in &r.hidden {
            println!("crate {k}: {n} findings, file a review finding");
        }
        for (k, n) in &r.withheld {
            println!(
                "crate {k}: {n} passing test results or lines withheld (paths the role may not read)"
            );
        }
        if let Some(n) = &r.note {
            println!("note: {n}");
        }
        let secs = t.elapsed().as_secs_f64();
        println!(
            "-- {name}: {} ({} findings, {secs:.1} s)",
            if r.ok { "PASS" } else { "FAIL" },
            r.findings
        );
        results.push((name, r, secs));
    }
    println!(
        "\nxtask gate summary ({:.1} s):",
        started.elapsed().as_secs_f64()
    );
    let mut ok = true;
    for (name, r, secs) in &results {
        println!(
            "  {:<12} {:<4} {:>4} findings {:>7.1} s",
            name,
            if r.ok { "PASS" } else { "FAIL" },
            r.findings,
            secs
        );
        ok &= r.ok;
    }
    println!("xtask gate: {}", if ok { "PASS" } else { "FAIL" });
    Ok(ok)
}

fn exclusions(ctx: &Ctx) -> Vec<String> {
    let mut v = Vec::new();
    let members: Vec<String> = ctx
        .full
        .as_ref()
        .map(|m| m.members().map(|p| p.name.clone()).collect())
        .unwrap_or_default();
    for h in &ctx.cfg.host_only.crates {
        if members.contains(h) {
            v.push("--exclude".into());
            v.push(h.clone());
        }
    }
    for r in &ctx.cfg.roots.roots {
        if r.present && members.contains(&r.name) {
            v.push("--exclude".into());
            v.push(r.name.clone());
        }
    }
    v
}

fn cargo_step(ctx: &Ctx, args: Vec<String>, json: bool, extra_env: &[(&str, &str)]) -> StepResult {
    let ids_fn = |id: &str| {
        ctx.full
            .as_ref()
            .and_then(|m| m.package(id))
            .map(|p| p.name.clone())
    };
    let names = cargo::Names {
        ids: &ids_fn,
        repo: &ctx.repo_str,
    };
    let mut env = ctx.env.clone();
    for (k, v) in extra_env {
        env.push((k.to_string(), v.to_string()));
    }
    match cargo::run(&ctx.repo, &args, &env, json, ctx.filter.as_ref(), &names) {
        Err(e) => StepResult::err(e),
        Ok(out) => StepResult {
            ok: out.success,
            findings: out.errors
                + usize::from(!out.success && out.errors == 0)
                + out.hidden.values().sum::<usize>(),
            hidden: out.hidden,
            withheld: out.withheld,
            note: None,
        },
    }
}

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

fn step(ctx: &Ctx, name: &str) -> StepResult {
    let f = ctx.filter.as_ref();
    match name {
        "lock" => match &ctx.lock {
            Ok(n) => StepResult {
                note: Some(n.clone()),
                ..StepResult::pass()
            },
            Err(e) => StepResult::err(e.clone()),
        },
        "fmt" => cargo_step(
            ctx,
            s(&["fmt", "--all", "--", "--check", "--color", "never"]),
            false,
            &[],
        ),
        "clippy" => {
            let mut a = s(&[
                "clippy",
                "--workspace",
                "--all-targets",
                "--locked",
                "--keep-going",
            ]);
            a.extend(exclusions(ctx));
            a.extend(s(&["--", "-D", "warnings"]));
            cargo_step(ctx, a, true, &[])
        }
        "test" => {
            let mut a = s(&["test", "--workspace", "--locked", "--no-fail-fast"]);
            a.extend(exclusions(ctx));
            cargo_step(ctx, a, true, &[])
        }
        "gt20-e" => {
            let mut a = s(&[
                "check",
                "--workspace",
                "--all-targets",
                "--locked",
                "--keep-going",
            ]);
            a.extend(exclusions(ctx));
            let targets = gt20e_targets(ctx.host.as_deref());
            for t in &targets {
                a.push("--target".into());
                a.push(t.to_string());
            }
            let mut r = cargo_step(ctx, a, true, &[]);
            if targets.len() < cargo::TARGETS.len() {
                r.note = Some(format!(
                    "the host target {} is type-checked by the clippy step (same exclusions, --all-targets)",
                    ctx.host.as_deref().unwrap_or("?")
                ));
            }
            r
        }
        "roots-check" => {
            let members: Vec<String> = ctx
                .full
                .as_ref()
                .map(|m| m.members().map(|p| p.name.clone()).collect())
                .unwrap_or_default();
            let mut total = StepResult::pass();
            for r in &ctx.cfg.roots.roots {
                if !r.present || !members.contains(&r.name) {
                    println!("{}: not present, skipped (dormant)", r.name);
                    continue;
                }
                let parts: Vec<&str> = r.windows_check.split_whitespace().collect();
                if parts.first() != Some(&"cargo") {
                    return StepResult::err(format!(
                        "xtask/roots.toml: windows_check of {} must be a cargo command",
                        r.name
                    ));
                }
                let mut runs = vec![parts[1..].iter().map(|x| x.to_string()).collect::<Vec<_>>()];
                runs.push(s(&[
                    "clippy",
                    "-p",
                    &r.name,
                    "--all-targets",
                    "--locked",
                    "--keep-going",
                    "--target",
                    "x86_64-pc-windows-msvc",
                    "--",
                    "-D",
                    "warnings",
                ]));
                for a in runs {
                    total.add(cargo_step(ctx, a, true, &[]));
                }
            }
            total
        }
        "gt20-b" | "licence" | "gt20-d" | "crates" => {
            match ctx.deps.get_or_init(|| dep_inputs(ctx)) {
                Err(e) => StepResult::err(e.clone()),
                Ok(d) => {
                    let inp = DepInputs {
                        targets: &d.targets,
                        full: &d.full,
                        fuzz: d.fuzz.as_ref(),
                        lockfiles: &d.lockfiles,
                        found_lockfiles: &d.found,
                        config: &ctx.cfg,
                    };
                    let fuzz_err = || {
                        d.fuzz_error
                            .iter()
                            .map(|e| Diag::path("fuzz", "fuzz/Cargo.toml", e.clone()))
                            .collect::<Vec<_>>()
                    };
                    let mut note = None;
                    let diags = match name {
                        "gt20-b" => {
                            let mut v = lint_deps::gt20b(&inp);
                            v.extend(fuzz_err());
                            v
                        }
                        "licence" => lint_deps::licences(&inp),
                        "crates" => {
                            match git::files(&ctx.repo) {
                                Ok(files) => {
                                    let w = unmatched_deny_patterns(&ctx.cfg, &d.full, &files);
                                    if !w.is_empty() {
                                        note = Some(w.join("\nnote: "));
                                    }
                                }
                                Err(e) => return StepResult::err(e),
                            }
                            lint_deps::crate_kinds(&inp)
                        }
                        _ => {
                            let mut v = lint_deps::osdeps(&inp);
                            match source_scan(ctx, &d.full) {
                                Ok(mut s) => v.append(&mut s),
                                Err(e) => return StepResult::err(e),
                            }
                            v
                        }
                    };
                    let mut r = StepResult::from_diags(diags, f);
                    if name == "gt20-b" || name == "licence" {
                        r.note.clone_from(&d.fuzz_note);
                    } else {
                        r.note = note;
                    }
                    r
                }
            }
        }
        "roots" => match &ctx.full {
            None => StepResult::err("cargo metadata failed".into()),
            Some(md) => {
                let mut diags =
                    lint_roots::check_roots(&ctx.cfg.roots, md, &lint_roots::list_crate_files);
                match lint_tables(ctx, md) {
                    Ok(mut v) => diags.append(&mut v),
                    Err(e) => return StepResult::err(e),
                }
                StepResult::from_diags(diags, f)
            }
        },
        "markers" => match markers_scan(&ctx.repo, &ctx.range) {
            Ok((d, n)) => {
                let mut r = StepResult::from_diags(d, f);
                r.note = Some(format!("{n} commits scanned"));
                r
            }
            Err(e) => StepResult::err(e),
        },
        "authors" => match authors_scan(&ctx.repo, &ctx.range, ctx.branch_role.as_deref()) {
            Ok((d, n, skipped)) => {
                let mut r = StepResult::from_diags(d, f);
                r.note = Some(match &ctx.branch_role {
                    Some(b) => format!("{n} commits checked against {b}"),
                    None => format!(
                        "{n} commits checked; {skipped} commits without a WP-xx: subject (owner commits) skipped"
                    ),
                });
                r
            }
            Err(e) => StepResult::err(e),
        },
        "private" => match private_scan(&ctx.repo, &ctx.range) {
            Ok((d, note)) => {
                let mut r = StepResult::from_diags(d, f);
                r.note = Some(note);
                r
            }
            Err(e) => StepResult::err(e),
        },
        "hooks" => match hooks_needed(ctx) {
            Ok(true) => hooks_step(&ctx.repo),
            Ok(false) => StepResult {
                note: Some("neither the range nor the working tree touches .githooks/ or xtask/: the hooks and the xtask they call are those already tested (CI runs the step always)".into()),
                ..StepResult::pass()
            },
            Err(e) => StepResult::err(e),
        },
        "ps1-bom" => match ps1_bom(&ctx.repo) {
            Ok(d) => StepResult::from_diags(d, f),
            Err(e) => StepResult::err(e),
        },
        "coverage" => match coverage::run(&ctx.repo, ctx.strict_coverage) {
            Ok(r) => {
                let mut res = StepResult::from_diags(r.diags, f);
                res.note = Some(r.notice.unwrap_or_else(|| {
                    format!(
                        "{} rows; {} blank fixture cells, {} blank model cells ({})",
                        r.rows,
                        r.blank_fixture,
                        r.blank_model,
                        if ctx.strict_coverage {
                            "--strict-coverage: each is a finding"
                        } else {
                            "tolerated until WP-80 pass 2 runs with --strict-coverage"
                        }
                    )
                }));
                res
            }
            Err(e) => StepResult::err(e),
        },
        other => StepResult::err(format!("unknown step {other}")),
    }
}

/// The `gt20-e` targets: the four of [90 §11.1] but the host's, which the `clippy` step type-checks with the same
/// exclusions and `--all-targets` (checking it again would be one full check more per gate run).
fn gt20e_targets(host: Option<&str>) -> Vec<&'static str> {
    cargo::TARGETS
        .iter()
        .copied()
        .filter(|t| Some(*t) != host)
        .collect()
}

/// Warnings for `xtask/roles.toml` `deny_read` patterns that match no file of a crate that has modules beside its
/// root: a module the pattern means to hide under another name (the toy log's seeded-bug module is fixed as
/// `crates/moirai-toylog/src/{bug,bugs}` by authors.md §3) would slip past the read filter unnoticed.
fn unmatched_deny_patterns(cfg: &Config, md: &Metadata, files: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    for role in &cfg.roles.roles {
        for raw in &role.deny_read {
            if seen.contains(&raw.as_str()) {
                continue;
            }
            seen.push(raw);
            let p = Pattern::new(raw);
            let Some(pkg) = md.members().find(|m| {
                let d = m.dir();
                let rel = d
                    .strip_prefix(&format!("{}/", md.workspace_root))
                    .unwrap_or(&d)
                    .to_string();
                raw.starts_with(&format!("{rel}/src/"))
            }) else {
                continue;
            };
            let d = pkg.dir();
            let rel = d
                .strip_prefix(&format!("{}/", md.workspace_root))
                .unwrap_or(&d)
                .to_string();
            let src = format!("{rel}/src/");
            let modules = files.iter().any(|f| {
                f.strip_prefix(&src)
                    .is_some_and(|r| r.ends_with(".rs") && r != "lib.rs" && r != "main.rs")
            });
            if modules && !files.iter().any(|f| p.matches(f)) {
                out.push(format!(
                    "warning: {} has modules beside its crate root, but the deny_read pattern `{raw}` (xtask/roles.toml) matches none of its files: check the module's name against docs/m0/authors.md §3",
                    pkg.name
                ));
            }
        }
    }
    out
}

/// The shell that runs `.githooks/test-hooks.sh` and the `PATH` it needs: `sh` on `PATH` as it is, else Git for
/// Windows' own `usr/bin/sh.exe` (found from `git --exec-path`) with its `usr/bin` put first on `PATH`, where the
/// script's `dirname`, `awk` and `mktemp` live.
fn find_sh(repo: &Path) -> Option<(PathBuf, Option<std::ffi::OsString>)> {
    let on_path = Command::new("sh")
        .args([
            "-c",
            "command -v dirname >/dev/null && command -v awk >/dev/null",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if on_path {
        return Some((PathBuf::from("sh"), None));
    }
    let exec = git::probe(repo, &["--exec-path"])?;
    let sh = Path::new(&exec).ancestors().find_map(|a| {
        ["usr/bin/sh.exe", "usr/bin/sh"]
            .iter()
            .map(|s| a.join(s))
            .find(|p| p.is_file())
    })?;
    let bin = sh.parent()?.to_path_buf();
    let mut dirs = vec![bin];
    if let Some(old) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&old));
    }
    let path = std::env::join_paths(dirs).ok()?;
    Some((sh, Some(path)))
}

/// Whether the `hooks` step has anything to test: always in CI; elsewhere when a commit of the range or the working
/// tree touches `.githooks/` or `xtask/` (the hooks, or the xtask they delegate to). The script takes ≈ 45 s on the
/// laptop, half of the incremental gate's budget (docs/m0/tools.md §13).
fn hooks_needed(ctx: &Ctx) -> Result<bool, String> {
    if ctx.ci {
        return Ok(true);
    }
    let log = git::run(
        &ctx.repo,
        &[
            "log",
            "--format=",
            "--name-only",
            &ctx.range,
            "--",
            ".githooks",
            "xtask",
        ],
    )?;
    if !log.trim().is_empty() {
        return Ok(true);
    }
    let status = git::run(
        &ctx.repo,
        &[
            "status",
            "--porcelain",
            "--untracked-files=all",
            "--",
            ".githooks",
            "xtask",
        ],
    )?;
    Ok(!status.trim().is_empty())
}

/// `hooks`: `.githooks/test-hooks.sh` with this xtask as `MOIRAI_XTASK`, so the hooks' first layer (the awk and sh
/// rules) is tested in every gate and CI run on the same cases as the Rust port (`markers`, `private`).
fn hooks_step(repo: &Path) -> StepResult {
    let script = repo.join(".githooks/test-hooks.sh");
    if !script.is_file() {
        return StepResult::err(format!("{} is missing", script.display()));
    }
    let Some((sh, path)) = find_sh(repo) else {
        return StepResult::err(
            "no POSIX sh found (PATH, or Git for Windows beside git --exec-path)".into(),
        );
    };
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => return StepResult::err(format!("current_exe: {e}")),
    };
    let tmp = std::env::temp_dir();
    println!(
        "$ {} .githooks/test-hooks.sh {}",
        sh.display(),
        tmp.display()
    );
    let mut cmd = Command::new(&sh);
    cmd.current_dir(repo)
        .arg(".githooks/test-hooks.sh")
        .arg(&tmp)
        .env("MOIRAI_XTASK", &exe)
        .stdin(Stdio::null());
    if let Some(p) = path {
        cmd.env("PATH", p);
    }
    let out = cmd.output();
    match out {
        Err(e) => StepResult::err(format!("{}: {e}", sh.display())),
        Ok(o) => {
            let text = String::from_utf8_lossy(&o.stdout);
            print!("{text}");
            eprint!("{}", String::from_utf8_lossy(&o.stderr));
            let failed = text.lines().filter(|l| l.starts_with("FAILED")).count();
            let skipped = text
                .lines()
                .any(|l| l.contains("skipped: no prebuilt xtask"));
            StepResult {
                ok: o.status.success() && !skipped,
                findings: failed
                    + usize::from(!o.status.success() && failed == 0)
                    + usize::from(skipped),
                note: text
                    .lines()
                    .rev()
                    .find(|l| l.starts_with("test-hooks:"))
                    .map(str::to_string),
                ..StepResult::default()
            }
        }
    }
}

struct DepData {
    targets: Vec<(String, Metadata)>,
    full: Metadata,
    fuzz: Option<Metadata>,
    /// Why the fuzz workspace's metadata could not be read although `fuzz/Cargo.lock` exists (a finding).
    fuzz_error: Option<String>,
    fuzz_note: Option<String>,
    lockfiles: Vec<(String, String)>,
    found: Vec<String>,
}

fn dep_inputs(ctx: &Ctx) -> Result<DepData, String> {
    let full = ctx.full.clone().ok_or("cargo metadata failed")?;
    let mut targets = Vec::new();
    for t in cargo::TARGETS {
        targets.push((
            t.to_string(),
            metadata(&ctx.repo, &["--filter-platform", t])?,
        ));
    }
    // The fuzz workspace (WP-06) is scanned once its lockfile exists: cargo cannot resolve it before its first
    // target, and `--locked` never creates a lockfile.
    let (mut fuzz, mut fuzz_error, mut fuzz_note) = (None, None, None);
    if ctx.repo.join("fuzz/Cargo.lock").is_file() {
        match metadata(&ctx.repo, &["--manifest-path", "fuzz/Cargo.toml"]) {
            Ok(m) => fuzz = Some(m),
            Err(e) => {
                fuzz_error = Some(format!(
                    "fuzz/Cargo.lock exists but the fuzz workspace does not resolve: {e}"
                ))
            }
        }
    } else if ctx.repo.join("fuzz/Cargo.toml").is_file() {
        fuzz_note = Some("fuzz/Cargo.lock does not exist yet (the fuzz workspace has no target): nothing to scan there".into());
    } else {
        fuzz_note = Some("fuzz/ does not exist yet (WP-06): nothing to scan there".into());
    }
    let mut lockfiles = Vec::new();
    for l in crate::config::LOCKFILES {
        let p = ctx.repo.join(l);
        if p.is_file() {
            lockfiles.push((
                l.to_string(),
                std::fs::read_to_string(&p).map_err(|e| format!("{l}: {e}"))?,
            ));
        }
    }
    let found = git::lockfiles(&ctx.repo)?;
    Ok(DepData {
        targets,
        full,
        fuzz,
        fuzz_error,
        fuzz_note,
        lockfiles,
        found,
    })
}

/// Reads every member's `.rs` files and runs the GT20 (d)/(a) scans, and checks each product crate's
/// `clippy.toml` (the type-aware layer).
fn source_scan(ctx: &Ctx, md: &Metadata) -> Result<Vec<Diag>, String> {
    let mut out = Vec::new();
    let root = crate::config::read_toml(&ctx.repo.join("Cargo.toml"))?;
    for p in md.members() {
        let Some(kind) = ctx.cfg.kinds.kind(&p.name) else {
            continue;
        };
        let dir = p.dir();
        let rel_dir = dir
            .strip_prefix(&format!("{}/", md.workspace_root))
            .unwrap_or(&dir)
            .to_string();
        if kind == crate::config::Kind::Product {
            let clippy = std::fs::read_to_string(Path::new(&dir).join("clippy.toml")).ok();
            let manifest = crate::config::read_toml(Path::new(&p.manifest_path))?;
            out.extend(lint_source::check_clippy_config(
                &p.name,
                &rel_dir,
                clippy.as_deref(),
                Some(&manifest),
                Some(&root),
            ));
        }
        let mut files = Vec::new();
        let mut stack = vec![(PathBuf::from(&dir), String::new())];
        while let Some((abs, rel)) = stack.pop() {
            let rd = std::fs::read_dir(&abs).map_err(|e| format!("{}: {e}", abs.display()))?;
            for e in rd {
                let e = e.map_err(|e| e.to_string())?;
                let n = e.file_name().to_string_lossy().into_owned();
                if n.starts_with('.') || (rel.is_empty() && n == "target") {
                    continue;
                }
                let r = if rel.is_empty() {
                    n.clone()
                } else {
                    format!("{rel}/{n}")
                };
                let ft = e.file_type().map_err(|e| e.to_string())?;
                if ft.is_dir() {
                    stack.push((e.path(), r));
                } else if n.ends_with(".rs") {
                    let text = std::fs::read_to_string(e.path())
                        .map_err(|er| format!("{}: {er}", e.path().display()))?;
                    files.push((format!("{rel_dir}/{r}"), r, text));
                }
            }
        }
        files.sort();
        out.extend(lint_source::check_crate(
            &CrateSource {
                name: &p.name,
                kind,
                files,
            },
            &ctx.cfg.osdeps,
        ));
    }
    Ok(out)
}

fn lint_tables(ctx: &Ctx, md: &Metadata) -> Result<Vec<Diag>, String> {
    let root = crate::config::read_toml(&ctx.repo.join("Cargo.toml"))?;
    let mut manifests = Vec::new();
    for p in md.members() {
        let t = crate::config::read_toml(Path::new(&p.manifest_path))?;
        let rel = p
            .manifest_path
            .strip_prefix(&format!("{}/", md.workspace_root))
            .unwrap_or(&p.manifest_path)
            .to_string();
        manifests.push((p.name.clone(), rel, t));
    }
    Ok(lint_roots::check_lint_tables(&root, &manifests))
}

/// The AI-marker scan over a range: messages and identities of every commit, merges included.
pub fn markers_scan(repo: &Path, range: &str) -> Result<(Vec<Diag>, usize), String> {
    let cc = git::comment_prefix(repo);
    let commits = git::commit_texts(repo, range)?;
    let mut out = Vec::new();
    for c in &commits {
        let short = &c.sha[..c.sha.len().min(10)];
        let mut refusals = markers::check_message(&c.message, &c.trailers, &cc);
        refusals.extend(markers::check_identity("author", &c.author));
        refusals.extend(markers::check_identity("committer", &c.committer));
        for r in refusals {
            out.push(Diag::new(
                "markers",
                format!("commit {short} ({}): refused ({r})", c.subject),
            ));
        }
    }
    Ok((out, commits.len()))
}

/// `xtask authors` over a range: every commit, merges through their combined change list. `branch_role` is the
/// branch role's title in `--branch` mode. Returns the findings, the commits checked and the owner commits skipped.
pub(crate) fn authors_scan(
    repo: &Path,
    range: &str,
    branch_role: Option<&str>,
) -> Result<(Vec<Diag>, usize, usize), String> {
    let text = std::fs::read_to_string(repo.join("docs/m0/authors.md"))
        .map_err(|e| format!("docs/m0/authors.md: {e}"))?;
    let ledger = authors::parse(&text)?;
    let mut out = Vec::new();
    let mut checked = 0;
    let mut skipped = 0;
    let notice = |c: &str| -> Result<bool, String> {
        let new = git::run_bytes(repo, &["cat-file", "blob", &format!("{c}:NOTICE")])?;
        let old = git::run_bytes(repo, &["cat-file", "blob", &format!("{c}^:NOTICE")])
            .unwrap_or_default();
        Ok(new.starts_with(&old))
    };
    // No ancestor of the commit touched the path: the commit is the first to add it.
    let first_added = |c: &str, path: &str| -> Result<bool, String> {
        if git::probe(repo, &["rev-parse", "--verify", "-q", &format!("{c}^")]).is_none() {
            return Ok(true);
        }
        let earlier = git::run(repo, &["rev-list", "-1", &format!("{c}^@"), "--", path])?;
        Ok(earlier.trim().is_empty())
    };
    let checks = authors::Checks {
        branch_role,
        notice_appended: &notice,
        first_added: &first_added,
    };
    for rev in git::rev_list(repo, range)? {
        let subject = git::run(repo, &["log", "-1", "--format=%s", &rev.sha])?
            .trim()
            .to_string();
        let changes = git::commit_changes(repo, &rev)?;
        let commit = authors::Commit {
            sha: rev.sha.clone(),
            subject,
            merge: rev.merge,
            changes,
        };
        let (d, was_checked) = authors::check_commit(&ledger, &commit, &checks);
        if was_checked {
            checked += 1;
        } else {
            skipped += 1;
        }
        out.extend(d);
    }
    Ok((out, checked, skipped))
}

/// Streams one git command's stdout into `f`.
fn with_git_stdout<T>(
    repo: &Path,
    args: &[String],
    f: impl FnOnce(BufReader<std::process::ChildStdout>) -> Result<T, String>,
) -> Result<T, String> {
    let mut child = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| format!("git {}: {e}", args.join(" ")))?;
    let out = child.stdout.take().ok_or("git: no stdout")?;
    let r = f(BufReader::with_capacity(64 * 1024, out));
    let status = child.wait().map_err(|e| e.to_string())?;
    let v = r?;
    if !status.success() {
        return Err(format!("git {} failed ({status})", args.join(" ")));
    }
    Ok(v)
}

/// The private checks over every commit of the range (authors.md §5 item 5: re-run before every merge), merges
/// through their combined diff. The shingles of every commit are matched in one pass over the manifest.
pub(crate) fn private_scan(repo: &Path, range: &str) -> Result<(Vec<Diag>, String), String> {
    let guard = git::probe(
        repo,
        &["config", "--type=bool", "--get", "moirai.private-guard"],
    )
    .as_deref()
        == Some("true");
    let private_dir = git::main_worktree(repo).map(|m| m.join("private"));
    let loaded = match &private_dir {
        Some(pd) => private::load_current(pd)?,
        None => None,
    };
    let mut note = match (&loaded, guard) {
        (Some(_), _) => "manifest current; files, shingles and reports checked".to_string(),
        (None, true) => return Err("moirai.private-guard is true and /private/MANIFEST.b3 is missing: failing closed".into()),
        (None, false) => "no /private/MANIFEST.b3 (and the guard is off): only private/** and the report scrub are checked".into(),
    };
    let names = private::machine_names();
    let pd = private_dir.clone().unwrap_or_default();
    let g = private::Guards {
        manifest: loaded.as_ref().map(|(m, p)| (pd.as_path(), m, p.clone())),
        names: &names,
    };
    let mut out = Vec::new();
    let mut hits = private::Hits::default();
    let commits = git::rev_list(repo, range)?;
    for rev in &commits {
        let short = &rev.sha[..rev.sha.len().min(10)];
        let changed: Vec<private::Changed> = private::parse_raw_z(&git::commit_raw(repo, rev)?)
            .into_iter()
            .filter(|c| c.blob.bytes().any(|b| b != b'0'))
            .collect();
        let hashes = if g.manifest.is_some() {
            private::blob_hashes(repo, &changed)?
        } else {
            Vec::new()
        };
        let label = format!("commit {short}");
        let d = with_git_stdout(repo, &git::commit_patch_args(rev), |r| {
            private::check_changes(&label, &changed, &hashes, r, &g, &mut hits)
        })?;
        out.extend(d);
    }
    if let Some((_, _, mpath)) = &g.manifest {
        hits.resolve(mpath, &mut out)?;
    }
    note.push_str(&format!(" ({} commits)", commits.len()));
    Ok((out, note))
}

pub(crate) fn ps1_bom(repo: &Path) -> Result<Vec<Diag>, String> {
    let mut out = Vec::new();
    for f in git::files(repo)? {
        if !f.to_ascii_lowercase().ends_with(".ps1") {
            continue;
        }
        let Ok(b) = std::fs::read(repo.join(&f)) else {
            continue;
        };
        if b.iter().any(|&c| c >= 0x80) && !b.starts_with(&[0xEF, 0xBB, 0xBF]) {
            out.push(Diag::path("ps1-bom", &f, "a non-ASCII .ps1 file must start with a UTF-8 BOM (PLAN §2.5; Windows PowerShell 5.1 reads it as ANSI otherwise)"));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_filter_reduces_forbidden_paths() {
        let role = Role {
            name: "r-harn-i".into(),
            title: "R-HARN-I".into(),
            lane: "A".into(),
            deny_read: vec![
                "crates/moirai-model/**".into(),
                "crates/moirai-toylog/src/{bug,bugs}".into(),
            ],
        };
        let mut dirs = BTreeMap::new();
        dirs.insert(
            "moirai-model".to_string(),
            "crates/moirai-model".to_string(),
        );
        dirs.insert(
            "moirai-toylog".to_string(),
            "crates/moirai-toylog".to_string(),
        );
        let f = RoleFilter::new(&role, dirs);
        assert!(f.hides_crate("moirai-model"));
        assert!(!f.hides_crate("moirai-toylog"));
        assert!(f.hides_path("crates/moirai-toylog/src/bug.rs"));
        let diags = vec![
            Diag::krate("x", "moirai-model", "secret"),
            Diag::at(
                "x",
                "moirai-toylog",
                "crates/moirai-toylog/src/bugs/one.rs",
                3,
                "secret",
            ),
            Diag::at(
                "x",
                "moirai-toylog",
                "crates/moirai-toylog/src/log.rs",
                3,
                "visible",
            ),
            Diag::path("x", "docs/spec/COVERAGE.md", "visible"),
        ];
        let (shown, hidden) = reduce(diags, &f);
        assert_eq!(shown.len(), 2);
        assert_eq!(hidden["moirai-model"], 1);
        assert_eq!(hidden["moirai-toylog"], 1);
    }

    #[test]
    fn ci_ranges_from_events() {
        let d = std::env::temp_dir().join(format!("moirai-xtask-event-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("e.json");
        std::fs::write(
            &p,
            r#"{"pull_request":{"base":{"sha":"aaa"},"head":{"sha":"bbb"},"body":"x"}}"#,
        )
        .unwrap();
        assert_eq!(ci_range(&p).unwrap(), "aaa..bbb");
        std::fs::write(&p, r#"{"before":"ccc","after":"ddd"}"#).unwrap();
        assert_eq!(ci_range(&p).unwrap(), "ccc..ddd");
        std::fs::write(
            &p,
            r#"{"before":"0000000000000000000000000000000000000000","after":"ddd"}"#,
        )
        .unwrap();
        assert!(ci_range(&p).is_err());
        std::fs::remove_dir_all(&d).unwrap();
    }
}
