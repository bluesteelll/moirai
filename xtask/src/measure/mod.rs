//! `cargo xtask measure` (WP-51): the session runner of the measurement night ([MP §1.1]). `[MP §x]` cites
//! `docs/spec/measurement-protocol.md`, whose §9.8 is this command's reference; the plan is `xtask/measure-night.toml`
//! ([`plan`]).
//!
//! | Command | What it does |
//! |---|---|
//! | `measure check [--smoke] --plan <file>` | the plan, the window, every run's command line and the refusals `run` would make now; starts and writes nothing |
//! | `measure run [--smoke] --plan <file>` | the pre-checks, then the plan's runs inside the current agent-free window |
//!
//! It reuses the nightly runner's window calendar, byte quantities, guard and process code (`crate::nightly`).
//!
//! Exit codes: 0 pass; 1 fail; 2 usage error; 3 refused; 4 internal error.

mod plan;

use crate::git;
use crate::nightly::Tree;
use crate::nightly::calendar::{self, WindowKind};
use crate::nightly::exec::{self, Cmd, Host, RealHost};
use crate::utc;
use plan::{Measurement, Plan};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::ExitCode;

/// The usage text.
pub const USAGE: &str = "usage: cargo xtask measure check [--smoke] --plan <file>
       cargo xtask measure run [--smoke] --plan <file>
check reads the plan, the window calendar /private/windows.toml, the tree and the release binaries, and prints every
run with its driver command line and the refusals run would make now; it starts nothing and writes nothing. run makes
the pre-checks (host, a complete plan, an agent-free window, a clean tree, the binaries, the scratch directory, rustc)
and then the plan's runs, one driver process per run behind the guard, with raw records in
/private/measurements/<n>/. --smoke makes each run once with --smoke instead of --out, outside any window, recording
nothing. Exit codes: 0 pass, 1 fail, 2 usage error, 3 refused, 4 internal error.
See docs/spec/measurement-protocol.md §9.8.";

/// Exit code: every run passed (`check`: no refusal).
pub const EXIT_PASS: u8 = 0;
/// Exit code: usage error.
pub const EXIT_USAGE: u8 = 2;
/// Exit code: a pre-check refused; nothing started.
pub const EXIT_REFUSED: u8 = 3;
/// Exit code: the runner itself failed.
pub const EXIT_INTERNAL: u8 = 4;

/// The directory under `/private/` that receives the raw records ([MP §7.2]).
pub const MEASUREMENTS: &str = "measurements";
/// The directory under the scratch directory that holds the drivers' working directories.
pub const WORK: &str = "work";
/// The guard's binary in `bins` ([MP §8]).
pub const GUARD: &str = "guard";
/// What `check` shows in place of the toolchain, which `run` reads when it starts.
const TOOLCHAIN_AT_RUN: &str = "<rustc --version at run time>";

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum Mode {
    Check,
    Run,
}

#[derive(Debug, Eq, PartialEq)]
struct Opts {
    mode: Mode,
    smoke: bool,
    plan: PathBuf,
}

fn parse_args(args: &[String]) -> Result<Opts, String> {
    let mode = match args.first().map(String::as_str) {
        Some("check") => Mode::Check,
        Some("run") => Mode::Run,
        Some(o) => return Err(format!("unknown measure command '{o}'")),
        None => return Err("measure needs a command: check or run".into()),
    };
    let mut smoke = false;
    let mut plan = None;
    let mut it = args[1..].iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--smoke" if smoke => return Err("--smoke is given twice".into()),
            "--smoke" => smoke = true,
            "--plan" if plan.is_some() => return Err("--plan is given twice".into()),
            "--plan" => {
                plan = Some(PathBuf::from(
                    it.next().ok_or("--plan needs a file")?.as_str(),
                ))
            }
            other => return Err(format!("unknown argument '{other}'")),
        }
    }
    Ok(Opts {
        mode,
        smoke,
        plan: plan.ok_or("--plan <file> is required")?,
    })
}

/// Everything the runner resolved before its checks.
#[derive(Clone, Debug)]
pub struct Env {
    pub repo: PathBuf,
    /// `/private/` of the main worktree.
    pub private: PathBuf,
    /// The calendar file, `/private/windows.toml`.
    pub windows: PathBuf,
    /// The target root (PLAN §2.1), against which the plan's `bins` and `scratch` resolve.
    pub root: PathBuf,
    pub has_taskkill: bool,
    pub tree: Tree,
}

impl Env {
    fn at_root(&self, p: &str) -> PathBuf {
        let p = Path::new(p);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            self.root.join(p)
        }
    }

    fn bins(&self, plan: &Plan) -> PathBuf {
        self.at_root(&plan.bins)
    }

    fn scratch(&self, plan: &Plan) -> PathBuf {
        self.at_root(&plan.scratch)
    }

    /// A binary of `moirai-probes-bin` in `bins`.
    fn exe(&self, plan: &Plan, name: &str) -> PathBuf {
        self.bins(plan)
            .join(format!("{name}{}", std::env::consts::EXE_SUFFIX))
    }

    /// `/private/measurements/<n>/`, the directory of measurement n's raw records ([MP §7.2]).
    fn out(&self, n: u32) -> PathBuf {
        self.private.join(MEASUREMENTS).join(n.to_string())
    }

    /// A driver's working directory, `<scratch>/work/<n>/`.
    fn work(&self, plan: &Plan, n: u32) -> PathBuf {
        self.scratch(plan).join(WORK).join(n.to_string())
    }
}

/// One refusal of the pre-checks.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Refusal {
    pub kind: &'static str,
    pub detail: String,
}

fn refusal(kind: &'static str, detail: impl Into<String>) -> Refusal {
    Refusal {
        kind,
        detail: detail.into(),
    }
}

/// `p` for comparing directories that may not exist yet: `.` dropped, `..` applied, separators as `/`, and in lower
/// case, since Windows paths ignore case (on other systems this errs towards refusing).
fn norm(p: &Path) -> String {
    let mut prefix = String::new();
    let mut parts: Vec<String> = Vec::new();
    for c in p.components() {
        match c {
            Component::Prefix(x) => prefix = x.as_os_str().to_string_lossy().to_lowercase(),
            Component::RootDir => prefix.push('/'),
            Component::CurDir => {}
            Component::ParentDir => {
                parts.pop();
            }
            Component::Normal(s) => parts.push(s.to_string_lossy().to_lowercase()),
        }
    }
    format!("{prefix}{}", parts.join("/"))
}

/// Whether `inner` is `outer` or lies below it.
fn within(outer: &Path, inner: &Path) -> bool {
    let (o, i) = (norm(outer), norm(inner));
    i == o || i.starts_with(&format!("{}/", o.trim_end_matches('/')))
}

/// The pre-checks that `check` and `run` share ([MP §9.8]): `host`, `plan`, `dirty-tree`, `bins` and `scratch`.
// spec: [MP §9.8] (the pre-checks of run; check exits 3 on any of them)
fn refusals(env: &Env, plan: &Plan) -> Vec<Refusal> {
    let mut r = Vec::new();
    if !env.has_taskkill {
        r.push(refusal(
            "host",
            "the session runner runs on the Windows laptop: %SystemRoot%\\System32\\taskkill.exe, which stops a driver's process tree, was not found",
        ));
    }
    let empty: Vec<String> = plan
        .incomplete()
        .iter()
        .map(|m| format!("{} ({})", m.n, m.driver))
        .collect();
    if !empty.is_empty() {
        r.push(refusal(
            "plan",
            format!(
                "the plan is incomplete: measurements {} list no quantity yet; the work that builds each driver fills its list ([MP §9.8])",
                empty.join(", ")
            ),
        ));
    }
    if env.tree.changes > 0 {
        r.push(refusal(
            "dirty-tree",
            format!(
                "the working tree has {} changed or untracked paths; the records must name the commit they measured",
                env.tree.changes
            ),
        ));
    }
    if env.tree.commit.is_none() {
        r.push(refusal("dirty-tree", "HEAD names no commit"));
    }
    let mut names = vec![GUARD];
    for m in &plan.measurements {
        if !names.contains(&m.driver.as_str()) {
            names.push(&m.driver);
        }
    }
    for name in names {
        let p = env.exe(plan, name);
        if !p.is_file() {
            r.push(refusal(
                "bins",
                format!(
                    "{} is missing: the release build of moirai-probes-bin at the measured commit goes into {} before the night",
                    p.display(),
                    env.bins(plan).display()
                ),
            ));
        }
    }
    let scratch = env.scratch(plan);
    for (what, p) in [("the repository", &env.repo), ("/private/", &env.private)] {
        if within(p, &scratch) || within(&scratch, p) {
            r.push(refusal(
                "scratch",
                format!(
                    "the scratch directory {} and {what} {} lie one inside the other: removing what lies below the scratch directory could reach a record or the tree",
                    scratch.display(),
                    p.display()
                ),
            ));
        }
    }
    r
}

/// One run's driver command ([MP §9.7], [MP §9.8]): `--out` names the measurement's directory of raw records, or
/// `--smoke` replaces it; the working directory is the measurement's own.
// spec: [MP §9.7] (the driver contract), [MP §9.8] (one driver process per run)
fn driver_cmd(
    env: &Env,
    plan: &Plan,
    m: &Measurement,
    q: &str,
    commit: &str,
    toolchain: &str,
    smoke: bool,
) -> Cmd {
    let c = Cmd::new(
        env.exe(plan, &m.driver).to_string_lossy(),
        &env.work(plan, m.n),
    )
    .args([
        "--host",
        "laptop",
        "--condition",
        "idle",
        "--quantity",
        q,
        "--commit",
        commit,
        "--toolchain",
        toolchain,
    ]);
    if smoke {
        c.args(["--smoke"])
    } else {
        c.args([
            "--out".to_string(),
            env.out(m.n).to_string_lossy().into_owned(),
        ])
    }
}

/// `check`'s window lines: the current or the next window, and the settle and the budgets against it. They are
/// information: R-25's owner names the night once `check` and `--smoke` pass.
fn window_lines(env: &Env, plan: &Plan, now: i64) -> Vec<String> {
    let need = plan.settle_secs + plan.budgets_secs() + plan.finish_margin_secs;
    let time = |has: i64, which: &str| {
        format!(
            "measure: time: the settle ({}), the budgets ({}) and the finish margin ({}) need {}; {which} has {}{}",
            utc::hm(plan.settle_secs),
            utc::hm(plan.budgets_secs()),
            utc::hm(plan.finish_margin_secs),
            utc::hm(need),
            utc::hm(has),
            if has < need {
                ": the last measurements may be cut at its end"
            } else {
                ""
            }
        )
    };
    let ws = match std::fs::read_to_string(&env.windows) {
        Err(e) => {
            return vec![format!(
                "measure: window: none agreed ({} cannot be read: {e})",
                env.windows.display()
            )];
        }
        Ok(text) => match calendar::parse(&text) {
            Err(e) => return vec![format!("measure: window: the calendar is malformed: {e}")],
            Ok(ws) => ws,
        },
    };
    match (calendar::current(&ws, now), calendar::next(&ws, now)) {
        (Some(w), _) => vec![
            format!(
                "measure: window: now in {}; every run ends by {}{}",
                w.describe(),
                utc::rfc3339(w.end - plan.finish_margin_secs),
                if w.kind == WindowKind::AgentFree {
                    ""
                } else {
                    " (run refuses it: an idle run is made in an agent-free window)"
                }
            ),
            time(w.end - now, "what is left of it"),
        ],
        (None, Some(w)) => vec![
            format!("measure: window: none now; the next is {}", w.describe()),
            time(w.end - w.start, "it"),
        ],
        (None, None) => vec!["measure: window: none now, and no later window is agreed".into()],
    }
}

/// `measure check` ([MP §9.8]): prints the plan, the window, every run with its driver command line and the refusals
/// `run` would make now apart from the window, the guard and the toolchain. It starts nothing (nothing through
/// `host` but the clock) and writes nothing. Returns the exit code: 0 with no refusal, 3 otherwise.
// spec: [MP §9.8] (check)
pub fn check<H: Host + ?Sized>(
    host: &mut H,
    env: &Env,
    plan: &Plan,
    plan_path: &Path,
    smoke: bool,
    out: &mut dyn Write,
) -> u8 {
    let now = host.now();
    let _ = writeln!(
        out,
        "measure: plan {}: {} measurements, {} runs{}",
        plan_path.display(),
        plan.measurements.len(),
        plan.runs(),
        if smoke { " (smoke)" } else { "" }
    );
    if !smoke {
        for l in window_lines(env, plan, now) {
            let _ = writeln!(out, "{l}");
        }
    }
    let _ = writeln!(
        out,
        "measure: commit {} on {}; bins {}; scratch {}",
        env.tree.commit.as_deref().unwrap_or("?"),
        env.tree.branch.as_deref().unwrap_or("?"),
        env.bins(plan).display(),
        env.scratch(plan).display()
    );
    let commit = env.tree.commit.as_deref().unwrap_or("<HEAD>");
    for m in &plan.measurements {
        if m.quantities.is_empty() {
            let _ = writeln!(
                out,
                "measure: {} ({}): no quantity listed yet",
                m.n, m.driver
            );
        }
        for q in &m.quantities {
            let c = driver_cmd(env, plan, m, q, commit, TOOLCHAIN_AT_RUN, smoke);
            let _ = if smoke {
                writeln!(
                    out,
                    "measure: smoke {}/{q}: budget {}: {}",
                    m.n,
                    utc::hm(plan.smoke_budget_secs),
                    c.line()
                )
            } else {
                writeln!(
                    out,
                    "measure: run {}/{q}: budget {} of the measurement's {}: {}",
                    m.n,
                    utc::hm(m.run_budget_secs),
                    utc::hm(m.budget_secs),
                    c.line()
                )
            };
        }
    }
    let r = refusals(env, plan);
    for x in &r {
        let _ = writeln!(out, "measure: refused: {}: {}", x.kind, x.detail);
    }
    if r.is_empty() {
        let _ = writeln!(
            out,
            "measure: check passed; run checks the window, the guard and rustc when it starts"
        );
        EXIT_PASS
    } else {
        let _ = writeln!(out, "measure: check refused ({} refusals)", r.len());
        EXIT_REFUSED
    }
}

/// Resolves the environment: `/private/` and the target root as `xtask nightly` finds them, the calendar and the
/// tree.
fn resolve(repo: &Path) -> Result<Env, String> {
    let main = git::main_worktree(repo).ok_or("cannot locate the main worktree")?;
    let private = main.join("private");
    let root = git::probe(repo, &["config", "--get", "moirai.target-root"])
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::worktree::default_root(&main, "moirai-target"));
    Ok(Env {
        repo: repo.to_path_buf(),
        windows: private.join(calendar::FILE),
        private,
        root,
        has_taskkill: exec::taskkill().is_some(),
        tree: Tree::read(repo, true)?,
    })
}

/// `cargo xtask measure …` ([`USAGE`]).
pub fn cli(args: &[String], repo: &Path) -> Result<ExitCode, String> {
    if matches!(args.first().map(String::as_str), Some("--help" | "help")) {
        println!("{USAGE}");
        return Ok(ExitCode::SUCCESS);
    }
    let o = match parse_args(args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("xtask measure: {e}\n{USAGE}");
            return Ok(ExitCode::from(EXIT_USAGE));
        }
    };
    let mut stdout = std::io::stdout().lock();
    let plan = match std::fs::read_to_string(&o.plan)
        .map_err(|e| e.to_string())
        .and_then(|t| Plan::parse(&t))
    {
        Ok(p) => p,
        Err(e) => {
            let _ = writeln!(stdout, "measure: refused: plan: {}: {e}", o.plan.display());
            return Ok(ExitCode::from(EXIT_REFUSED));
        }
    };
    let env = match resolve(repo) {
        Ok(env) => env,
        Err(e) => {
            let _ = writeln!(stdout, "measure: error: {e}");
            return Ok(ExitCode::from(EXIT_INTERNAL));
        }
    };
    let mut host = RealHost::new();
    match o.mode {
        Mode::Check => Ok(ExitCode::from(check(
            &mut host,
            &env,
            &plan,
            &o.plan,
            o.smoke,
            &mut stdout,
        ))),
        Mode::Run => {
            let _ = writeln!(stdout, "measure: error: run is not built at this commit");
            Ok(ExitCode::from(EXIT_INTERNAL))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nightly::exec::tests::{FakeHost, test_tree};
    use crate::testdir::TestDir;

    /// Inside the synthetic calendar's agent-free window, 2026-10-13T20:00Z..2026-10-14T04:00Z.
    const NOW: &str = "2026-10-13T21:00:00Z";

    fn at(s: &str) -> i64 {
        utc::parse_rfc3339(s).unwrap()
    }

    /// A complete synthetic plan: measurement 14 with two quantities, then measurement 6 with one.
    const PLAN: &str = r#"version = 1
[session]
bins = "bins"
scratch = "scratch"
settle-minutes = 15
finish-margin-minutes = 10
smoke-budget-minutes = 10
ram-floor = "1.5GB"
disk-floor = "25GB"
[[measurement]]
n = 14
driver = "m14"
title = "synthetic"
run-budget-minutes = 30
budget-minutes = 50
quantities = ["csr.1e5", "t1.read"]
[[measurement]]
n = 6
driver = "m6"
title = "synthetic"
run-budget-minutes = 20
budget-minutes = 25
quantities = ["lz4.encode"]
"#;

    fn plan() -> Plan {
        Plan::parse(PLAN).unwrap()
    }

    /// A scratch repository, private directory (with the synthetic calendar) and target root with the plan's
    /// binaries.
    fn env(d: &TestDir, plan: &Plan) -> Env {
        let mut names = vec![GUARD.to_string()];
        names.extend(plan.measurements.iter().map(|m| m.driver.clone()));
        for name in names {
            d.write(
                &format!("target/bins/{name}{}", std::env::consts::EXE_SUFFIX),
                b"binary",
            );
        }
        d.write("private/windows.toml", calendar::tests::SYNTHETIC);
        std::fs::create_dir_all(d.path().join("repo")).unwrap();
        Env {
            repo: d.path().join("repo"),
            private: d.path().join("private"),
            windows: d.path().join("private").join(calendar::FILE),
            root: d.path().join("target"),
            has_taskkill: true,
            tree: test_tree(),
        }
    }

    /// Every path below `dir`, sorted: what a command that writes nothing leaves unchanged.
    fn listing(dir: &Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).unwrap() {
                let p = e.unwrap().path();
                if p.is_dir() {
                    stack.push(p.clone());
                }
                out.push(p);
            }
        }
        out.sort();
        out
    }

    fn check_text(h: &mut FakeHost, env: &Env, plan: &Plan, smoke: bool) -> (u8, String) {
        let mut out = Vec::new();
        let code = check(h, env, plan, Path::new("plan.toml"), smoke, &mut out);
        (code, String::from_utf8(out).unwrap())
    }

    #[test]
    fn arguments() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let o = parse_args(&s(&["check", "--plan", "p.toml"])).unwrap();
        assert_eq!(
            o,
            Opts {
                mode: Mode::Check,
                smoke: false,
                plan: PathBuf::from("p.toml")
            }
        );
        let o = parse_args(&s(&["run", "--smoke", "--plan", "p.toml"])).unwrap();
        assert_eq!((o.mode, o.smoke), (Mode::Run, true));
        assert!(
            parse_args(&s(&["run", "--plan", "p.toml", "--smoke"]))
                .unwrap()
                .smoke
        );
        for (bad, needle) in [
            (&[][..], "needs a command"),
            (&["start", "--plan", "p"], "unknown measure command 'start'"),
            (&["check"], "--plan <file> is required"),
            (&["run", "--smoke"], "--plan <file> is required"),
            (&["check", "--plan"], "--plan needs a file"),
            (
                &["check", "--plan", "a", "--plan", "b"],
                "--plan is given twice",
            ),
            (
                &["run", "--smoke", "--smoke", "--plan", "a"],
                "--smoke is given twice",
            ),
            (
                &["run", "--plan", "a", "--now", "x"],
                "unknown argument '--now'",
            ),
        ] {
            let e = parse_args(&s(bad)).unwrap_err();
            assert!(e.contains(needle), "{bad:?}: {e}");
        }
    }

    #[test]
    fn check_prints_the_plan_and_starts_nothing() {
        let d = TestDir::new("measure-check");
        let plan = plan();
        let env = env(&d, &plan);
        let before = listing(d.path());
        let mut h = FakeHost::new(at(NOW));
        let (code, text) = check_text(&mut h, &env, &plan, false);
        assert_eq!(code, EXIT_PASS, "{text}");
        // The window, the time it needs, the commit, and every run with its driver command line, in plan order.
        assert!(
            text.contains("measure: window: now in 2026-10-13T20:00:00Z..2026-10-14T04:00:00Z (agent-free); every run ends by 2026-10-14T03:50:00Z\n"),
            "{text}"
        );
        assert!(
            text.contains("need 1h40m; what is left of it has 7h00m\n"),
            "{text}"
        );
        let commit = test_tree().commit.unwrap();
        let exe = |n: &str| {
            env.root
                .join("bins")
                .join(format!("{n}{}", std::env::consts::EXE_SUFFIX))
                .to_string_lossy()
                .into_owned()
        };
        let out = |n: &str| {
            env.private
                .join("measurements")
                .join(n)
                .to_string_lossy()
                .into_owned()
        };
        let runs: Vec<&str> = text
            .lines()
            .filter(|l| l.starts_with("measure: run "))
            .collect();
        assert_eq!(
            runs,
            [
                format!(
                    "measure: run 14/csr.1e5: budget 30m of the measurement's 50m: {} --host laptop --condition idle --quantity csr.1e5 --commit {commit} --toolchain \"<rustc --version at run time>\" --out {}",
                    exe("m14"),
                    out("14")
                ),
                format!(
                    "measure: run 14/t1.read: budget 30m of the measurement's 50m: {} --host laptop --condition idle --quantity t1.read --commit {commit} --toolchain \"<rustc --version at run time>\" --out {}",
                    exe("m14"),
                    out("14")
                ),
                format!(
                    "measure: run 6/lz4.encode: budget 20m of the measurement's 25m: {} --host laptop --condition idle --quantity lz4.encode --commit {commit} --toolchain \"<rustc --version at run time>\" --out {}",
                    exe("m6"),
                    out("6")
                ),
            ]
        );
        // It starts nothing and writes nothing: no guard, no driver, no directory.
        assert!(h.spawned.is_empty() && h.captured.is_empty());
        assert_eq!(listing(d.path()), before);
        // The smoke lines: --smoke in place of --out, the smoke budget, no window line.
        let (code, text) = check_text(&mut h, &env, &plan, true);
        assert_eq!(code, EXIT_PASS, "{text}");
        assert!(!text.contains("measure: window"), "{text}");
        assert!(
            text.contains(&format!(
                "measure: smoke 6/lz4.encode: budget 10m: {} --host laptop --condition idle --quantity lz4.encode --commit {commit} --toolchain \"<rustc --version at run time>\" --smoke\n",
                exe("m6")
            )),
            "{text}"
        );
        // No window agreed (or outside every window): information only, exit 0.
        let mut h = FakeHost::new(at("2026-10-15T00:00:00Z"));
        let (code, text) = check_text(&mut h, &env, &plan, false);
        assert_eq!(code, EXIT_PASS, "{text}");
        assert!(
            text.contains("measure: window: none now, and no later window is agreed"),
            "{text}"
        );
        let mut h = FakeHost::new(at("2026-10-13T12:00:00Z"));
        let (_, text) = check_text(&mut h, &env, &plan, false);
        assert!(
            text.contains("measure: window: none now; the next is 2026-10-13T20:00:00Z..2026-10-14T04:00:00Z (agent-free)"),
            "{text}"
        );
        let mut e = env.clone();
        e.windows = d.path().join("none.toml");
        let (code, text) = check_text(&mut h, &e, &plan, false);
        assert_eq!(code, EXIT_PASS, "{text}");
        assert!(text.contains("measure: window: none agreed ("), "{text}");
        // An incomplete plan: refused, exit 3.
        let mut incomplete = plan.clone();
        incomplete.measurements[1].quantities.clear();
        let (code, text) = check_text(&mut h, &env, &incomplete, false);
        assert_eq!(code, EXIT_REFUSED, "{text}");
        assert!(
            text.contains("measure: 6 (m6): no quantity listed yet"),
            "{text}"
        );
        assert!(
            text.contains("measure: refused: plan: the plan is incomplete: measurements 6 (m6)"),
            "{text}"
        );
        // A missing binary, a dirty tree, no taskkill, and a scratch directory that holds /private/: each refused.
        std::fs::remove_file(exe("m6")).unwrap();
        let (code, text) = check_text(&mut h, &env, &plan, false);
        assert_eq!(code, EXIT_REFUSED, "{text}");
        assert!(text.contains("measure: refused: bins: "), "{text}");
        d.write(
            &format!("target/bins/m6{}", std::env::consts::EXE_SUFFIX),
            b"binary",
        );
        type Change = fn(&mut Env);
        let cases: [(Change, &str); 3] = [
            (|e| e.tree.changes = 2, "dirty-tree"),
            (|e| e.tree.commit = None, "dirty-tree"),
            (|e| e.has_taskkill = false, "host"),
        ];
        for (change, kind) in cases {
            let mut e = env.clone();
            change(&mut e);
            let (code, text) = check_text(&mut h, &e, &plan, false);
            assert_eq!(code, EXIT_REFUSED, "{text}");
            assert!(
                text.contains(&format!("measure: refused: {kind}: ")),
                "{text}"
            );
        }
        let mut p = plan.clone();
        p.scratch = d.path().to_string_lossy().into_owned();
        let (code, text) = check_text(&mut h, &env, &p, false);
        assert_eq!(code, EXIT_REFUSED, "{text}");
        assert!(text.contains("measure: refused: scratch: "), "{text}");
        assert!(h.spawned.is_empty() && h.captured.is_empty());
    }
}
