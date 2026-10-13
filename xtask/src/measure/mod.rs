//! `cargo xtask measure` (WP-51): the session runner of the measurement night ([MP §1.1]). `[MP §x]` cites
//! `docs/spec/measurement-protocol.md`, whose §9.8 is this command's reference; the plan is `xtask/measure-night.toml`
//! ([`plan`]).
//!
//! | Command | What it does |
//! |---|---|
//! | `measure check [--smoke] --plan <file>` | the plan, the window, every run's command line and the refusals `run` would make now; starts and writes nothing |
//! | `measure run [--smoke] --plan <file>` | the pre-checks, then the plan's runs inside the current agent-free window |
//!
//! **The pre-checks of `run`** ([`pre_checks`]); every refusal is listed, and any one stops `run` before anything
//! starts: `host`, `plan` (incomplete), `calendar`, `outside-window`, `window-kind`, `window-ending`, `dirty-tree`,
//! `bins`, `scratch` and `toolchain`.
//!
//! **The run** ([`run`]): the scratch directory is made, the settle waited out, and then each measurement's quantities
//! are run in plan order, one driver process per run ([MP §9.7]).
//! - A run with an exit-grade record of the measured commit already in `/private/measurements/<n>/` is done before
//!   ([`done_before`]): starting the runner again after a crash resumes the night.
//! - Each driver runs in its measurement's working directory `<scratch>/work/<n>/`, which its `TMP` and `TEMP` name
//!   too.
//! - Before each attempt the guard checks the RAM and disk floors; no guard runs while a driver runs.
//! - An attempt ends by the earliest of its run budget, its measurement's budget and the window's end less the finish
//!   margin, where its process tree is stopped.
//! - Its record is the one new file of its quantity in `/private/measurements/<n>/`, judged by [`record::judge`]; a
//!   file that was there before and vanished or changed stops the session (`record-changed`).
//! - An attempt that is not exit-grade, timed out at its run budget or could not start is retried once, at once, while
//!   time is left before the measurement's budget and the window's deadline. When the only reason is a changed host
//!   snapshot, the retry waits `settle-minutes` first, and is made only if that wait fits.
//! - A driver whose process tree cannot be stopped stops the session (`tree-unstoppable`); its run is never retried.
//! - After each attempt the tested tree is read again.
//! - A record directory that cannot be made or listed ends only its attempt (`error`), and a resume listing that
//!   fails counts no earlier record. The session record is saved after every run; a save that fails three times is a
//!   warning, and the runs go on.
//!
//! **The finish**: the drivers' working directories are removed, the session record
//! `<scratch>/sessions/<start>/session.json` is written, and the private manifest is rebuilt. The session record names
//! the processes resident at the session's start and end ([`residents`]). An internal error (exit 4: the session
//! directory or a working directory cannot be made) ends the session at once and skips the finish.
//!
//! **`--smoke`**: each run once, outside any window and with no settle, with `--smoke` in place of `--out` under the
//! smoke budget; no calendar, record, resume, retry or private index, and the session record goes to
//! `<scratch>/sessions/<start>.smoke/`.
//!
//! It reuses the nightly runner's window calendar, byte quantities, guard and process code (`crate::nightly`).
//!
//! Exit codes: 0 pass (every run has an exit-grade record); 1 fail; 2 usage error; 3 refused (nothing ran); 4 internal
//! error.

mod plan;
mod record;
mod residents;

use crate::git;
use crate::nightly::Tree;
use crate::nightly::calendar::{self, Window, WindowKind};
use crate::nightly::exec::{self, Captured, Cmd, Ended, Host, Proc, RealHost, Watch};
use crate::nightly::guard;
use crate::utc;
use plan::{Measurement, Plan};
use serde_json::{Value, json};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::ExitCode;
use std::rc::Rc;

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
/// Exit code: a run has no exit-grade record, or the session stopped after a driver ran.
pub const EXIT_FAIL: u8 = 1;
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
/// The directory under the scratch directory that holds each session's logs and record.
pub const SESSIONS: &str = "sessions";
/// The session record's file name and schema ([MP §9.8]).
pub const SESSION: &str = "session.json";
pub const SESSION_SCHEMA: &str = "moirai-xtask/measure-session/1";
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
    /// `rustc --version` run in the repository (read for `run` only): the records' `toolchain`.
    pub toolchain: Result<String, String>,
    /// Rebuild the private manifest at the finish (the real runner always does).
    pub rebuild_manifest: bool,
    /// The revision whose tracked text the rebuilt manifest treats as public (`master`; `xtask private index`).
    pub public_ref: Option<String>,
    /// Reads the image names resident in the interactive sessions ([`residents::read`]), at a session's start and its
    /// end. It runs outside [`Host`], like `rustc --version`, so a test host's captures stay the guard's.
    pub residents: fn() -> Result<Vec<String>, String>,
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
/// `--smoke` replaces it; the working directory is the measurement's own, and `TMP` and `TEMP` name it too, so the
/// driver's temporary files stay on the scratch volume.
// spec: [MP §9.7] (the driver contract), [MP §9.8] (one driver process per run, its working directory)
fn driver_cmd(
    env: &Env,
    plan: &Plan,
    m: &Measurement,
    q: &str,
    commit: &str,
    toolchain: &str,
    smoke: bool,
) -> Cmd {
    let work = env.work(plan, m.n);
    let tmp = work.to_string_lossy().into_owned();
    let c = Cmd::new(env.exe(plan, &m.driver).to_string_lossy(), &work)
        .env("TMP", tmp.clone())
        .env("TEMP", tmp)
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
            if !smoke
                && let Some(head) = env.tree.commit.as_deref()
                && let Ok(Some(name)) = done_before(&env.out(m.n), m.n, q, head)
            {
                let _ = writeln!(out, "measure: run {}/{q}: done before ({name})", m.n);
                continue;
            }
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

/// What the pre-checks of `run` found.
struct Pre {
    /// The current window, when one holds now.
    window: Option<Window>,
    refusals: Vec<Refusal>,
}

/// The pre-checks of `run` ([MP §9.8]): [`refusals`], the window (it holds now, it is agent-free, and more than the
/// finish margin of it is left; not for `--smoke`, which runs outside any window) and the toolchain.
// spec: [MP §9.8] (the pre-checks of run; --smoke), [MP §2.3] (an idle run is made in an agent-free window)
fn pre_checks(env: &Env, plan: &Plan, now: i64, smoke: bool) -> Pre {
    let mut r = refusals(env, plan);
    let mut window = None;
    let file = (!smoke).then(|| std::fs::read_to_string(&env.windows));
    match file {
        None => {}
        Some(Err(e)) => r.push(refusal(
            "calendar",
            format!(
                "no agreed window: {} cannot be read ({e}); the owner keeps the calendar there (PLAN §5 V4)",
                env.windows.display()
            ),
        )),
        Some(Ok(text)) => match calendar::parse(&text) {
            Err(e) => r.push(refusal("calendar", e)),
            Ok(ws) => match calendar::current(&ws, now) {
                None => r.push(refusal(
                    "outside-window",
                    format!(
                        "{} is outside every agreed window; {}",
                        utc::rfc3339(now),
                        calendar::next(&ws, now).map_or("no later window is agreed".into(), |w| {
                            format!("the next is {}", w.describe())
                        })
                    ),
                )),
                Some(w) if w.kind != WindowKind::AgentFree => r.push(refusal(
                    "window-kind",
                    format!(
                        "the window {} is not agent-free: an idle run is made in an agent-free window ([MP §2.3])",
                        w.describe()
                    ),
                )),
                Some(w) if w.end - now <= plan.finish_margin_secs => r.push(refusal(
                    "window-ending",
                    format!(
                        "the window {} ends in {}, within the finish margin of {}",
                        w.describe(),
                        utc::hm(w.end - now),
                        utc::hm(plan.finish_margin_secs)
                    ),
                )),
                Some(w) => window = Some(w),
            },
        },
    }
    if let Err(e) = &env.toolchain {
        r.push(refusal(
            "toolchain",
            format!("rustc --version failed in the repository: {e}"),
        ));
    }
    Pre {
        window,
        refusals: r,
    }
}

/// How one attempt of a run ended ([MP §9.8]).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum Outcome {
    /// The driver exited 0 and wrote one record of the run, and it is exit-grade.
    ExitGrade,
    /// The driver exited, but no exit-grade record of the run came of it.
    NotExitGrade,
    /// Stopped at its run budget.
    TimedOut,
    /// Stopped at its measurement's budget.
    BudgetEnded,
    /// Stopped at the window's end less the finish margin.
    WindowEnded,
    /// The driver could not be started, or its process tree could not be stopped (`tree-unstoppable`, which also
    /// stops the session).
    Error,
    /// `--smoke`: the driver exited 0 within its budget.
    Passed,
    /// `--smoke`: the driver exited with another code.
    Failed,
}

impl Outcome {
    fn as_str(self) -> &'static str {
        match self {
            Outcome::Passed => "passed",
            Outcome::Failed => "failed",
            Outcome::ExitGrade => "exit-grade",
            Outcome::NotExitGrade => "not-exit-grade",
            Outcome::TimedOut => "timed-out",
            Outcome::BudgetEnded => "budget-ended",
            Outcome::WindowEnded => "window-ended",
            Outcome::Error => "error",
        }
    }
}

/// The session record ([MP §9.8]), rewritten through a temporary file after every run.
struct Session {
    path: PathBuf,
    value: Value,
}

/// The waits (seconds) between the tries of a session record's save: three tries in all.
const SAVE_WAITS_SECS: [u64; 2] = [1, 2];

impl Session {
    /// Saves the record ([MP §9.8]). A save that fails is tried again after each of [`SAVE_WAITS_SECS`]; after the
    /// last try it is a warning, and the session goes on: the records already written, and the next save, keep the
    /// night.
    // spec: [MP §9.8] (the finish: the session record)
    fn save<H: Host + ?Sized>(&self, host: &mut H, out: &mut dyn Write) {
        let mut e = match self.write() {
            Ok(()) => return,
            Err(e) => e,
        };
        for wait in SAVE_WAITS_SECS {
            host.sleep(wait);
            match self.write() {
                Ok(()) => return,
                Err(x) => e = x,
            }
        }
        let _ = writeln!(
            out,
            "measure: warning: the session record {} could not be saved after {} tries ({e}); the runs go on",
            self.path.display(),
            SAVE_WAITS_SECS.len() + 1
        );
    }

    /// Writes the record through a temporary file beside it.
    fn write(&self) -> Result<(), String> {
        let tmp = self.path.with_extension("json.tmp");
        let text = serde_json::to_string_pretty(&self.value).map_err(|e| e.to_string())?;
        std::fs::write(&tmp, text + "\n").map_err(|e| format!("{}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, &self.path).map_err(|e| format!("{}: {e}", self.path.display()))
    }

    fn push_run(&mut self, run: Value) {
        if let Some(a) = self.value["runs"].as_array_mut() {
            a.push(run);
        }
    }
}

/// The attempts of one run in one invocation: the first and one retry ([MP §9.8]).
const MAX_ATTEMPTS: u32 = 2;

/// Resume ([MP §9.8]): the name of an exit-grade record of measurement `n`, quantity `q` and the measured `commit`
/// already in `dir`, if there is one. Records of other commits, and records that are not exit-grade, stay raw data
/// and do not count.
// spec: [MP §9.8] (resume), [MP §7.3]
fn done_before(dir: &Path, n: u32, q: &str, commit: &str) -> Result<Option<String>, String> {
    for name in record::list(dir)?
        .into_keys()
        .filter(|f| record::record_name(f, q))
    {
        let Ok(text) = std::fs::read_to_string(dir.join(&name)) else {
            continue;
        };
        if record::judge(&text, n, q, commit).is_ok() {
            return Ok(Some(name));
        }
    }
    Ok(None)
}

/// What every attempt of a session shares.
struct Ctx<'a> {
    env: &'a Env,
    plan: &'a Plan,
    commit: String,
    toolchain: String,
    /// The guard before each attempt: `--volume <scratch>` and the two floors, no counted directory ([MP §8.2]).
    guard: Cmd,
    /// The session's directory: its logs and its record.
    dir: PathBuf,
    /// The window's end less the finish margin (`i64::MAX` for `--smoke`, which runs outside any window).
    deadline: i64,
    /// `--smoke` ([MP §9.8]): `--smoke` in place of `--out`, the smoke budget, no record and no retry.
    smoke: bool,
}

/// One attempt and what it decided.
struct Attempt {
    /// `None` when the guard stopped the session before the driver started.
    outcome: Option<Outcome>,
    json: Value,
    /// Why the session stops after this attempt: the guard refused or failed, the driver's process tree could not be
    /// stopped, a record changed, or the tested tree changed.
    stop: Option<String>,
    /// Whether a driver process was started.
    started: bool,
    /// Whether the driver's one record has no reason not to be exit-grade but a changed host snapshot
    /// ([`record::SNAPSHOT_CHANGED`]): its retry cools down first ([MP §9.8]).
    snapshot_only: bool,
}

/// Whether the tested tree moved during an attempt: `HEAD` or the branch changed, a tracked file changed, or the tree
/// could not be read; `None` when it is the commit the pre-checks found, clean (as `xtask nightly` reads it).
// spec: [MP §9.8] (the tree)
fn tree_change<H: Host + ?Sized>(host: &mut H, env: &Env) -> Option<Value> {
    match host.tree(&env.repo) {
        Ok(t) if t.commit == env.tree.commit && t.branch == env.tree.branch && t.changes == 0 => {
            None
        }
        Ok(t) => Some(json!({
            "commit": t.commit,
            "branch": t.branch,
            "tracked_changes": t.changes,
        })),
        Err(e) => Some(json!({ "error": e })),
    }
}

/// The stop of a session whose driver's process tree could not be stopped ([MP §9.8]).
const TREE_UNSTOPPABLE: &str = "tree-unstoppable";

/// The attempt's host for [`exec::run_cmd`]: it remembers the first error of a driver's `kill_tree`, so the attempt
/// learns that the process tree could not be stopped without reading the text of [`Ended::Error`], which `exec` owns.
struct Tracked<'a, H: Host + ?Sized> {
    host: &'a mut H,
    unstoppable: Rc<RefCell<Option<String>>>,
}

/// A driver process of [`Tracked`].
struct TrackedProc {
    p: Box<dyn Proc>,
    unstoppable: Rc<RefCell<Option<String>>>,
}

impl Proc for TrackedProc {
    fn try_wait(&mut self) -> Result<Option<i32>, String> {
        self.p.try_wait()
    }

    fn kill_tree(&mut self) -> Result<(), String> {
        let r = self.p.kill_tree();
        if let Err(e) = &r {
            self.unstoppable
                .borrow_mut()
                .get_or_insert_with(|| e.clone());
        }
        r
    }
}

impl<H: Host + ?Sized> Host for Tracked<'_, H> {
    fn now(&self) -> i64 {
        self.host.now()
    }

    fn sleep(&mut self, secs: u64) {
        self.host.sleep(secs)
    }

    fn spawn(&mut self, cmd: &Cmd, log: &Path) -> Result<Box<dyn Proc>, String> {
        let p = self.host.spawn(cmd, log)?;
        Ok(Box::new(TrackedProc {
            p,
            unstoppable: self.unstoppable.clone(),
        }))
    }

    fn capture(&mut self, cmd: &Cmd, timeout_secs: u64) -> Result<Captured, String> {
        self.host.capture(cmd, timeout_secs)
    }

    fn tree(&mut self, repo: &Path) -> Result<Tree, String> {
        self.host.tree(repo)
    }
}

/// One attempt of the run of quantity `q` ([MP §9.8]): the guard, then one driver process in the measurement's
/// working directory until it exits or its deadline (the earliest of its run budget, `m_deadline` and the window's),
/// where its process tree is stopped; then the record's judgment and the tree. A tree that cannot be stopped makes the
/// attempt an error and stops the session.
// spec: [MP §9.8] (the guard before each attempt, one driver process per run, budgets, the record, the tree),
// [MP §9.7] (the driver contract), [MP §7.3]
fn attempt<H: Host + ?Sized>(
    host: &mut H,
    cx: &Ctx,
    m: &Measurement,
    q: &str,
    k: u32,
    m_deadline: i64,
    out: &mut dyn Write,
) -> Attempt {
    let t0 = host.now();
    let _ = writeln!(
        out,
        "measure: {}/{q} attempt {k} at {}",
        m.n,
        utc::rfc3339(t0)
    );
    let mut j = json!({ "attempt": k, "started": utc::rfc3339(t0) });
    let stopped = |j: Value, why: String| Attempt {
        outcome: None,
        json: j,
        stop: Some(why),
        started: false,
        snapshot_only: false,
    };
    match host
        .capture(&cx.guard, guard::TIMEOUT_SECS)
        .and_then(|o| guard::parse(o.exit, &o.stdout, &o.stderr))
    {
        Err(e) => {
            j["guard"] = json!({ "error": e });
            return stopped(j, format!("the guard failed (it fails closed): {e}"));
        }
        Ok(v) => {
            j["guard"] = v.json.clone();
            if !v.pass {
                let why = v
                    .refusals
                    .iter()
                    .map(|r| format!("{}: {}", r.kind, r.detail))
                    .collect::<Vec<_>>()
                    .join("; ");
                return stopped(j, format!("the guard refused: {why}"));
            }
        }
    }
    // A smoke run writes no record, so nothing under /private/ is made or listed. Making and listing the record
    // directory before the driver is one step; if it fails, the attempt ends as an error and the session goes on.
    let dir = cx.env.out(m.n);
    let list = |dir: &Path| {
        if cx.smoke {
            Ok(BTreeMap::new())
        } else {
            record::list(dir)
        }
    };
    let made = if cx.smoke {
        Ok(())
    } else {
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))
    };
    let before = match made.and_then(|()| list(&dir)) {
        Ok(before) => before,
        Err(e) => {
            let why = [format!(
                "the record directory could not be made or listed: {e}"
            )];
            j["ended"] = json!(utc::rfc3339(host.now()));
            j["outcome"] = json!(Outcome::Error.as_str());
            j["exit_code"] = Value::Null;
            j["records"] = json!([]);
            j["why"] = json!(why);
            let _ = writeln!(out, "measure: {}/{q} error: {}", m.n, why[0]);
            return Attempt {
                outcome: Some(Outcome::Error),
                json: j,
                stop: None,
                started: false,
                snapshot_only: false,
            };
        }
    };
    let cmd = driver_cmd(cx.env, cx.plan, m, q, &cx.commit, &cx.toolchain, cx.smoke);
    let log = format!("{}.{q}.{k}.log", m.n);
    let start = host.now();
    let budget = if cx.smoke {
        cx.plan.smoke_budget_secs
    } else {
        m.run_budget_secs
    };
    let deadline = (start + budget).min(m_deadline).min(cx.deadline);
    j["deadline"] = json!(utc::rfc3339(deadline));
    j["command"] = json!(cmd.line());
    j["log"] = json!(log);
    // No guard while the driver runs ([MP §9.8]): the watch's period lies past the deadline.
    let mut watch = Watch {
        guard: cx.guard.clone(),
        every_secs: (deadline - start).max(0) + 1,
        min_available: None,
    };
    let unstoppable = Rc::new(RefCell::new(None));
    let ended = exec::run_cmd(
        &mut Tracked {
            host: &mut *host,
            unstoppable: unstoppable.clone(),
        },
        &cmd,
        &cx.dir.join(&log),
        deadline,
        &mut watch,
    );
    let unstoppable = unstoppable.borrow_mut().take();
    let end = host.now();
    // A listing after the driver that fails ends the attempt as an error: no record is judged, and no record is checked
    // for a change during it; the next attempt's listing is a new baseline ([MP §9.8]).
    let after = list(&dir);
    let (touched, added) = match &after {
        Ok(after) => (
            record::changed(&before, after),
            after
                .keys()
                .filter(|n| !before.contains_key(*n) && record::record_name(n, q))
                .cloned()
                .collect::<Vec<String>>(),
        ),
        Err(_) => (Vec::new(), Vec::new()),
    };
    let stop_why = |at: i64| {
        if at == cx.deadline {
            (
                Outcome::WindowEnded,
                "stopped at the window's end less the finish margin".to_string(),
            )
        } else if at == m_deadline {
            (
                Outcome::BudgetEnded,
                format!(
                    "stopped at the measurement's budget of {}",
                    utc::hm(m.budget_secs)
                ),
            )
        } else {
            (
                Outcome::TimedOut,
                format!("stopped at its run budget of {}", utc::hm(budget)),
            )
        }
    };
    let (outcome, exit, why): (Outcome, Option<i32>, Vec<String>) = match ended {
        Ended::Exited(0) if cx.smoke => (Outcome::Passed, Some(0), Vec::new()),
        Ended::Exited(c) if cx.smoke => (
            Outcome::Failed,
            Some(c),
            vec![format!("the driver exited with {c}")],
        ),
        Ended::Exited(0) => match added.as_slice() {
            [] => (
                Outcome::NotExitGrade,
                Some(0),
                vec!["the driver wrote no record".into()],
            ),
            [one] => {
                let judged = std::fs::read_to_string(dir.join(one))
                    .map_err(|e| vec![format!("{one}: {e}")])
                    .and_then(|t| record::judge(&t, m.n, q, &cx.commit));
                match judged {
                    Ok(()) => (Outcome::ExitGrade, Some(0), Vec::new()),
                    Err(why) => (Outcome::NotExitGrade, Some(0), why),
                }
            }
            many => (
                Outcome::NotExitGrade,
                Some(0),
                vec![format!(
                    "the driver wrote {} records of the run",
                    many.len()
                )],
            ),
        },
        Ended::Exited(c) => (
            Outcome::NotExitGrade,
            Some(c),
            vec![format!("the driver exited with {c}")],
        ),
        Ended::TimedOut => {
            let (o, why) = stop_why(deadline);
            (o, None, vec![why])
        }
        Ended::Stopped(e) | Ended::Error(e) => (Outcome::Error, None, vec![e]),
    };
    // A process tree that could not be stopped makes the attempt an error, whatever ended it, and stops the session
    // below, so its run is never retried beside it. Otherwise a listing after the driver that failed makes it an error.
    let (outcome, why) = match (&unstoppable, &after) {
        (Some(e), _) => (Outcome::Error, vec![format!("{TREE_UNSTOPPABLE}: {e}")]),
        (None, Err(e)) => (
            Outcome::Error,
            vec![format!(
                "the record directory could not be listed after the run: {e}"
            )],
        ),
        (None, Ok(_)) => (outcome, why),
    };
    let snapshot_only = outcome == Outcome::NotExitGrade && why == [record::SNAPSHOT_CHANGED];
    j["ended"] = json!(utc::rfc3339(end));
    j["outcome"] = json!(outcome.as_str());
    j["exit_code"] = json!(exit);
    j["records"] = json!(added);
    j["why"] = json!(why);
    let _ = writeln!(
        out,
        "measure: {}/{q} {}{}",
        m.n,
        outcome.as_str(),
        if why.is_empty() {
            String::new()
        } else {
            format!(": {}", why.join("; "))
        }
    );
    let mut stop = unstoppable.as_ref().map(|e| {
        let why = format!(
            "{TREE_UNSTOPPABLE}: the process tree of {}/{q} could not be stopped ({e}); start the runner again only after that tree is gone",
            m.n
        );
        let _ = writeln!(out, "measure: {why}; the session stops");
        why
    });
    if !touched.is_empty() {
        let why = format!(
            "record-changed: {} in {} changed or vanished during {}/{q}; a record is never overwritten ([MP §7.2])",
            touched.join(", "),
            dir.display(),
            m.n
        );
        let _ = writeln!(out, "measure: {why}; the session stops");
        j["records_changed"] = json!(touched);
        stop.get_or_insert(why);
    }
    if let Some(change) = tree_change(host, cx.env) {
        let _ = writeln!(
            out,
            "measure: the tested tree changed during {}/{q}: {change}; the session stops",
            m.n
        );
        j["tree_changed"] = change;
        stop.get_or_insert(format!("the tested tree changed during {}/{q}", m.n));
    }
    Attempt {
        outcome: Some(outcome),
        json: j,
        stop,
        started: true,
        snapshot_only,
    }
}

/// `measure run` ([MP §9.8]): the pre-checks, then the plan's runs inside the current agent-free window, and the
/// finish; with `smoke`, each run once outside any window, recording nothing. Returns the exit code; `Err` is an
/// internal error.
// spec: [MP §9.8] (the run, the finish, the verdict, --smoke), [MP §1.1] (the measurement night)
pub fn run<H: Host + ?Sized>(
    host: &mut H,
    env: &Env,
    plan: &Plan,
    plan_path: &Path,
    smoke: bool,
    out: &mut dyn Write,
) -> Result<u8, String> {
    let start = host.now();
    let pre = pre_checks(env, plan, start, smoke);
    for r in &pre.refusals {
        let _ = writeln!(out, "measure: refused: {}: {}", r.kind, r.detail);
    }
    let (Some(commit), Ok(toolchain), true, true) = (
        env.tree.commit.clone(),
        env.toolchain.clone(),
        pre.refusals.is_empty(),
        smoke || pre.window.is_some(),
    ) else {
        return Ok(EXIT_REFUSED);
    };
    let scratch = env.scratch(plan);
    let stamp = utc::compact(start);
    let dir = scratch.join(SESSIONS).join(if smoke {
        format!("{stamp}.smoke")
    } else {
        stamp.clone()
    });
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let residents = residents_json((env.residents)());
    let cx = Ctx {
        env,
        plan,
        guard: Cmd::new(env.exe(plan, GUARD).to_string_lossy(), &scratch).args(
            guard::pre_check_args(&scratch, &[], plan.ram_floor, plan.disk_floor, &[]),
        ),
        dir: dir.clone(),
        deadline: pre
            .window
            .map_or(i64::MAX, |w| w.end - plan.finish_margin_secs),
        smoke,
        commit,
        toolchain,
    };
    let mut ses = Session {
        path: dir.join(SESSION),
        value: json!({
            "schema": SESSION_SCHEMA,
            "mode": if smoke { "smoke" } else { "run" },
            "plan": plan_path.to_string_lossy(),
            "started": utc::rfc3339(start),
            "ended": null,
            "window": pre.window.map(|w| json!({ "start": utc::rfc3339(w.start), "end": utc::rfc3339(w.end), "kind": w.kind.as_str() })),
            "deadline": pre.window.map(|_| utc::rfc3339(cx.deadline)),
            "commit": cx.commit,
            "branch": env.tree.branch,
            "toolchain": cx.toolchain,
            "settle_secs": if smoke { 0 } else { plan.settle_secs },
            "residents": { "start": residents, "end": null },
            "runs": [],
            "stopped": null,
            "verdict": null,
            "manifest": null,
        }),
    };
    ses.save(host, out);
    match pre.window {
        Some(w) => {
            let _ = writeln!(
                out,
                "measure: session {stamp} in {}; window {}; every run ends by {}; settling for {}",
                dir.display(),
                w.describe(),
                utc::rfc3339(cx.deadline),
                utc::hm(plan.settle_secs)
            );
            host.sleep(plan.settle_secs as u64);
        }
        None => {
            let _ = writeln!(
                out,
                "measure: smoke session {stamp} in {}: each run once with --smoke, outside any window, recording nothing",
                dir.display()
            );
        }
    }
    let mut stopped: Option<String> = None;
    let mut started_any = false;
    let mut refused_first = false;
    let mut good = 0;
    for m in &plan.measurements {
        let m_deadline = if smoke {
            i64::MAX
        } else {
            host.now() + m.budget_secs
        };
        let mut spent: Option<String> = None;
        let work = env.work(plan, m.n);
        let mut work_made = false;
        for q in &m.quantities {
            let mut r = json!({ "n": m.n, "driver": m.driver, "quantity": q, "status": null, "reason": null, "attempts": [] });
            let now = host.now();
            let blocked = stopped
                .clone()
                .or_else(|| spent.clone())
                .or_else(|| {
                    (now >= cx.deadline)
                        .then(|| "the window's end less the finish margin has passed".to_string())
                })
                .or_else(|| {
                    (now >= m_deadline).then(|| "the measurement's budget is spent".to_string())
                });
            if let Some(why) = blocked {
                let _ = writeln!(out, "measure: {}/{q} not-run: {why}", m.n);
                r["status"] = json!("not-run");
                r["reason"] = json!(why);
                ses.push_run(r);
                ses.save(host, out);
                continue;
            }
            // Resume: a run with an exit-grade record of the measured commit is done. A listing that fails counts no
            // earlier record, as `check` reads it.
            if !smoke {
                match done_before(&env.out(m.n), m.n, q, &cx.commit) {
                    Ok(Some(name)) => {
                        let _ = writeln!(out, "measure: {}/{q} done-before: {name}", m.n);
                        r["status"] = json!("done-before");
                        r["record"] = json!(name);
                        good += 1;
                        ses.push_run(r);
                        ses.save(host, out);
                        continue;
                    }
                    Ok(None) => {}
                    Err(e) => {
                        let _ = writeln!(
                            out,
                            "measure: {}/{q}: the earlier records could not be listed ({e}); the run is made as if there were none",
                            m.n
                        );
                        r["resume_error"] = json!(e);
                    }
                }
            }
            if !work_made {
                // Made empty before the measurement's first attempt; kept across its runs and retries.
                if work.exists() {
                    std::fs::remove_dir_all(&work)
                        .map_err(|e| format!("{}: {e}", work.display()))?;
                }
                std::fs::create_dir_all(&work).map_err(|e| format!("{}: {e}", work.display()))?;
                work_made = true;
            }
            let mut k = 0;
            let mut cool_down = None;
            loop {
                k += 1;
                let mut a = attempt(host, &cx, m, q, k, m_deadline, out);
                if let Some(secs) = cool_down.take() {
                    a.json["cool_down_secs"] = json!(secs);
                }
                if a.stop.is_some() && !started_any && !a.started {
                    refused_first = true;
                }
                started_any |= a.started;
                if let Some(list) = r["attempts"].as_array_mut() {
                    list.push(a.json);
                }
                match a.outcome {
                    None if k == 1 => {
                        r["status"] = json!("not-run");
                        r["reason"] = json!(a.stop);
                    }
                    None => {}
                    Some(o) => {
                        r["status"] = json!(o.as_str());
                        if matches!(o, Outcome::ExitGrade | Outcome::Passed) {
                            good += 1;
                        }
                        if o == Outcome::BudgetEnded {
                            spent = Some(format!(
                                "the measurement's budget was spent during {}/{q}",
                                m.n
                            ));
                        }
                    }
                }
                if let Some(why) = a.stop {
                    stopped.get_or_insert(why);
                    break;
                }
                // One retry, at once, of an attempt that is not exit-grade and was not stopped at the measurement's
                // budget or the window's deadline, while time is left before both.
                let retry = !smoke
                    && matches!(
                        a.outcome,
                        Some(Outcome::NotExitGrade | Outcome::TimedOut | Outcome::Error)
                    )
                    && k < MAX_ATTEMPTS
                    && host.now() < m_deadline.min(cx.deadline);
                if !retry {
                    break;
                }
                // A retry whose only reason is a changed host snapshot (a Windows or Defender update during the run)
                // first waits out the update's scan burst: `settle-minutes`, within the measurement's budget and the
                // window's deadline.
                if a.snapshot_only && plan.settle_secs > 0 {
                    let until = m_deadline.min(cx.deadline);
                    if host.now() + plan.settle_secs >= until {
                        let _ = writeln!(
                            out,
                            "measure: {}/{q}: the host snapshot changed during the run, and a cool-down of {} before the retry does not fit before {}; it is not retried",
                            m.n,
                            utc::hm(plan.settle_secs),
                            utc::rfc3339(until)
                        );
                        break;
                    }
                    let _ = writeln!(
                        out,
                        "measure: {}/{q}: the host snapshot changed during the run; cooling down {} before the retry",
                        m.n,
                        utc::hm(plan.settle_secs)
                    );
                    host.sleep(plan.settle_secs as u64);
                    cool_down = Some(plan.settle_secs);
                }
            }
            ses.push_run(r);
            ses.save(host, out);
        }
        if work_made {
            let _ = std::fs::remove_dir_all(&work);
        }
    }
    finish(host, &cx, &mut ses, stopped, good, out);
    Ok(if refused_first {
        EXIT_REFUSED
    } else if ses.value["verdict"] == "pass" {
        EXIT_PASS
    } else {
        EXIT_FAIL
    })
}

/// The finish ([MP §9.8]): the drivers' working directories removed, the verdict, the private manifest rebuilt (not
/// for `--smoke`, which wrote nothing under `/private/`), and the session record written. `good` counts the runs that
/// have an exit-grade record (with `--smoke`: that passed).
// spec: [MP §9.8] (the finish, the verdict, --smoke)
fn finish<H: Host + ?Sized>(
    host: &mut H,
    cx: &Ctx,
    ses: &mut Session,
    stopped: Option<String>,
    good: usize,
    out: &mut dyn Write,
) {
    let work = cx.env.scratch(cx.plan).join(WORK);
    if work.exists()
        && let Err(e) = std::fs::remove_dir_all(&work)
    {
        let _ = writeln!(out, "measure: {}: {e}", work.display());
    }
    let runs = cx.plan.runs();
    let pass = stopped.is_none() && good == runs;
    ses.value["ended"] = json!(utc::rfc3339(host.now()));
    ses.value["stopped"] = json!(stopped);
    let mut counts = serde_json::Map::new();
    counts.insert("runs".into(), json!(runs));
    counts.insert(
        (if cx.smoke { "passed" } else { "exit_grade" }).into(),
        json!(good),
    );
    ses.value["counts"] = Value::Object(counts);
    ses.value["verdict"] = json!(if pass { "pass" } else { "fail" });
    ses.value["residents"]["end"] = residents_json((cx.env.residents)());
    ses.value["manifest"] = if cx.smoke {
        json!("not rebuilt (smoke)")
    } else if cx.env.rebuild_manifest {
        match crate::private::rebuild(&cx.env.repo, &cx.env.private, cx.env.public_ref.as_deref()) {
            Ok(_) => {
                let _ = writeln!(
                    out,
                    "measure: rebuilt {}",
                    cx.env.private.join(crate::private::MANIFEST).display()
                );
                json!("rebuilt")
            }
            Err(e) => {
                let _ = writeln!(
                    out,
                    "measure: the private manifest could not be rebuilt ({e}); run `cargo xtask private index` before the next commit"
                );
                json!({ "error": e })
            }
        }
    } else {
        json!("not rebuilt")
    };
    ses.save(host, out);
    let _ = writeln!(
        out,
        "measure: {} ({good} of {runs} runs {}{}); session record {}",
        if pass { "PASS" } else { "FAIL" },
        if cx.smoke { "passed" } else { "exit-grade" },
        stopped
            .map(|s| format!("; stopped: {s}"))
            .unwrap_or_default(),
        ses.path.display()
    );
}

/// A resident list as the session record holds it ([MP §9.8]): `{"ok": [...]}`, or `{"error": "..."}`, which never
/// stops the session.
fn residents_json(r: Result<Vec<String>, String>) -> Value {
    match r {
        Ok(names) => json!({ "ok": names }),
        Err(e) => json!({ "error": e }),
    }
}

/// Resolves the environment: `/private/` and the target root as `xtask nightly` finds them, the calendar and the
/// tree.
fn resolve(repo: &Path, read_toolchain: bool) -> Result<Env, String> {
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
        toolchain: if read_toolchain {
            rustc_version(repo)
        } else {
            Err("read by run only".into())
        },
        rebuild_manifest: true,
        public_ref: Some("master".into()),
        residents: residents::read,
    })
}

/// `rustc --version` run in the repository, whose `rust-toolchain.toml` names the toolchain: the records'
/// `toolchain` ([MP §7.1]). It runs outside `Host`, so a test host's captures stay the guard's.
fn rustc_version(repo: &Path) -> Result<String, String> {
    let o = std::process::Command::new("rustc")
        .arg("--version")
        .current_dir(repo)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("rustc: {e}"))?;
    let v = String::from_utf8_lossy(&o.stdout).trim().to_string();
    if !o.status.success() || v.is_empty() {
        return Err(format!(
            "rustc --version exited with {} and printed '{v}'",
            o.status
        ));
    }
    Ok(v)
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
    let env = match resolve(repo, o.mode == Mode::Run) {
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
        Mode::Run => match run(&mut host, &env, &plan, &o.plan, o.smoke, &mut stdout) {
            Ok(code) => Ok(ExitCode::from(code)),
            Err(e) => {
                let _ = writeln!(
                    stdout,
                    "measure: error: {e}; the records already written stay, and starting the runner again resumes the night"
                );
                Ok(ExitCode::from(EXIT_INTERNAL))
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nightly::exec::tests::{FakeHost, guard_pass, guard_refuse, hangs, runs, test_tree};
    use crate::nightly::exec::{Captured, Proc};
    use crate::testdir::TestDir;
    use std::cell::{Cell, RefCell};
    use std::collections::VecDeque;
    use std::rc::Rc;

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
            toolchain: Ok(TOOLCHAIN.into()),
            rebuild_manifest: false,
            public_ref: None,
            residents: fake_residents,
        }
    }

    thread_local! {
        /// How often [`fake_residents`] was called on this test's thread.
        static RESIDENT_READS: Cell<u32> = const { Cell::new(0) };
    }

    /// The test environments' resident processes: every other call differs, so a session's start and end each land
    /// in their own field.
    fn fake_residents() -> Result<Vec<String>, String> {
        let k = RESIDENT_READS.with(|c| c.replace(c.get() + 1));
        let other = if k.is_multiple_of(2) {
            "explorer.exe"
        } else {
            "notepad.exe"
        };
        Ok(vec!["agent-app.exe".into(), other.into()])
    }

    /// The toolchain the test environments name.
    const TOOLCHAIN: &str = "rustc 1.0.0 (synthetic)";

    /// What the fake driver writes into its `--out` when it starts, in spawn order ([`Writes::ExitGrade`] once the
    /// list is empty).
    #[derive(Clone, Debug)]
    enum Writes {
        /// One exit-grade record of the run.
        ExitGrade,
        /// One record of the run whose `reasons` is not empty.
        NotExitGrade,
        /// Nothing.
        Nothing,
        /// Two records of the run.
        Two,
        /// An exit-grade record of the run, after appending to the first file already in `--out`: a driver that
        /// breaks [MP §7.2].
        Overwrite,
        /// An exit-grade record of the run but for its end snapshot, whose Defender signatures changed: a signature
        /// update during the run.
        SnapshotChanged,
        /// [`Writes::SnapshotChanged`] with a reason too: a record that is not valid either.
        SnapshotAndInvalid,
    }

    /// The fake driver ([MP §9.7]): when a driver starts, it writes what `writes` says directly into its `--out`,
    /// named as [MP §7.2] says with the start time of the virtual clock, after checking that its working directory
    /// exists.
    fn fake_driver(h: &mut FakeHost, writes: &[Writes]) -> Rc<RefCell<VecDeque<Writes>>> {
        let script = Rc::new(RefCell::new(
            writes.iter().cloned().collect::<VecDeque<_>>(),
        ));
        let s = script.clone();
        let clock = h.clock.clone();
        h.on_spawn = Some(Box::new(move |c: &Cmd| {
            assert!(
                c.cwd.is_dir(),
                "the working directory {} exists",
                c.cwd.display()
            );
            let arg = |k: &str| {
                c.args
                    .iter()
                    .position(|a| a == k)
                    .map(|i| c.args[i + 1].clone())
            };
            let (Some(dir), Some(q), Some(commit)) =
                (arg("--out"), arg("--quantity"), arg("--commit"))
            else {
                return;
            };
            let dir = PathBuf::from(dir);
            let n: u32 = dir.file_name().unwrap().to_string_lossy().parse().unwrap();
            let now = clock.get();
            let write = |at: i64, v: Value| {
                std::fs::create_dir_all(&dir).unwrap();
                let p = dir.join(format!("{q}.idle.{}.json", utc::compact(at)));
                std::fs::write(&p, v.to_string() + "\n").unwrap();
            };
            let good = |at: i64| record::tests::exit_grade(n, &q, &commit, &utc::rfc3339(at));
            match s.borrow_mut().pop_front().unwrap_or(Writes::ExitGrade) {
                Writes::ExitGrade => write(now, good(now)),
                Writes::NotExitGrade => {
                    let mut v = good(now);
                    v["reasons"] = json!(["a round-boundary reading failed"]);
                    write(now, v);
                }
                Writes::Nothing => {}
                Writes::Two => {
                    write(now, good(now));
                    write(now + 1, good(now + 1));
                }
                Writes::Overwrite => {
                    let first = std::fs::read_dir(&dir)
                        .unwrap()
                        .map(|e| e.unwrap().path())
                        .min()
                        .unwrap();
                    let mut f = std::fs::OpenOptions::new()
                        .append(true)
                        .open(first)
                        .unwrap();
                    std::io::Write::write_all(&mut f, b" ").unwrap();
                    write(now, good(now));
                }
                Writes::SnapshotChanged => {
                    let mut v = good(now);
                    v["host"]["end"]["defender"]["ok"]["signatures"] = json!("1.0.0.2");
                    write(now, v);
                }
                Writes::SnapshotAndInvalid => {
                    let mut v = good(now);
                    v["host"]["end"]["defender"]["ok"]["signatures"] = json!("1.0.0.2");
                    v["reasons"] = json!(["a round-boundary reading failed"]);
                    write(now, v);
                }
            }
        }));
        script
    }

    /// The session record of the session that started at `start`.
    fn session(env: &Env, plan: &Plan, start: i64) -> Value {
        let p = env
            .scratch(plan)
            .join(SESSIONS)
            .join(utc::compact(start))
            .join(SESSION);
        serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap()
    }

    fn run_text(h: &mut FakeHost, env: &Env, plan: &Plan) -> (u8, String) {
        let mut out = Vec::new();
        let code = run(h, env, plan, Path::new("plan.toml"), false, &mut out).unwrap();
        (code, String::from_utf8(out).unwrap())
    }

    /// Each run's quantity and status in the session record.
    fn statuses(s: &Value) -> Vec<(String, String)> {
        s["runs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| {
                (
                    format!("{}/{}", r["n"], r["quantity"].as_str().unwrap()),
                    r["status"].as_str().unwrap().to_string(),
                )
            })
            .collect()
    }

    fn pairs(v: &[(&str, &str)]) -> Vec<(String, String)> {
        v.iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect()
    }

    fn kinds(env: &Env, plan: &Plan, now: i64) -> Vec<&'static str> {
        pre_checks(env, plan, now, false)
            .refusals
            .iter()
            .map(|r| r.kind)
            .collect()
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

    #[test]
    fn run_refuses_outside_an_agent_free_window() {
        let d = TestDir::new("measure-run-window");
        let plan = plan();
        let env = env(&d, &plan);
        for (now, kind, detail) in [
            (
                "2026-10-13T19:00:00Z",
                "outside-window",
                "the next is 2026-10-13T20:00:00Z",
            ),
            ("2026-10-12T10:00:00Z", "window-kind", "is not agent-free"),
            (
                "2026-10-14T03:51:00Z",
                "window-ending",
                "within the finish margin",
            ),
            (
                "2026-10-15T00:00:00Z",
                "outside-window",
                "no later window is agreed",
            ),
        ] {
            let mut h = FakeHost::new(at(now));
            fake_driver(&mut h, &[]);
            let (code, text) = run_text(&mut h, &env, &plan);
            // Nothing started and nothing written.
            assert!(
                h.spawned.is_empty() && h.captured.is_empty(),
                "{now}: {text}"
            );
            assert_eq!(code, EXIT_REFUSED, "{now}: {text}");
            assert!(
                text.contains(&format!("measure: refused: {kind}: ")) && text.contains(detail),
                "{now}: {text}"
            );
            assert_eq!(kinds(&env, &plan, at(now)), [kind]);
            assert!(!env.scratch(&plan).exists(), "{now}");
            assert!(!env.private.join(MEASUREMENTS).exists(), "{now}");
        }
        // No calendar, or a malformed one.
        let mut e = env.clone();
        e.windows = d.path().join("none.toml");
        assert_eq!(kinds(&e, &plan, at(NOW)), ["calendar"]);
        d.write("bad.toml", "version = 1\n[[window]]\nstart = \"tonight\"\n");
        e.windows = d.path().join("bad.toml");
        assert_eq!(kinds(&e, &plan, at(NOW)), ["calendar"]);
        // A toolchain that cannot be read.
        let mut e = env.clone();
        e.toolchain = Err("rustc: not found".into());
        assert_eq!(kinds(&e, &plan, at(NOW)), ["toolchain"]);
        // Inside the agent-free window, more than the finish margin before its end, nothing refuses.
        assert!(kinds(&env, &plan, at(NOW)).is_empty());
        assert!(kinds(&env, &plan, at("2026-10-14T03:49:00Z")).is_empty());
    }

    #[test]
    fn run_refuses_on_each_guard_refusal() {
        let d = TestDir::new("measure-run-guard");
        let plan = plan();
        let env = env(&d, &plan);
        let not_run = pairs(&[
            ("14/csr.1e5", "not-run"),
            ("14/t1.read", "not-run"),
            ("6/lz4.encode", "not-run"),
        ]);
        // An injected refusal before the first run, of each kind, and a guard that prints no JSON (it fails closed):
        // no driver starts, exit 3, and the session record says why.
        let no_json = Captured {
            exit: Some(2),
            stdout: String::new(),
            stderr: "guard: usage".into(),
        };
        for (g, needle) in [
            (
                guard_refuse(1_000_000_000, "ram-low"),
                "the guard refused: ram-low",
            ),
            (
                guard_refuse(8_000_000_000, "disk-low"),
                "the guard refused: disk-low",
            ),
            (no_json, "the guard failed (it fails closed)"),
        ] {
            let mut h = FakeHost::new(at(NOW));
            fake_driver(&mut h, &[]);
            h.guards.push_back(g);
            let (code, text) = run_text(&mut h, &env, &plan);
            assert!(h.spawned.is_empty(), "{needle}: {text}");
            assert_eq!(code, EXIT_REFUSED, "{needle}: {text}");
            assert_eq!(h.captured.len(), 1);
            let s = session(&env, &plan, at(NOW));
            assert!(s["stopped"].as_str().unwrap().contains(needle), "{s}");
            assert_eq!(statuses(&s), not_run);
            assert_eq!(s["verdict"], "fail");
            std::fs::remove_dir_all(env.scratch(&plan)).unwrap();
        }
        // A refusal before the second run: the first run stands, and the session stops and fails.
        let mut h = FakeHost::new(at(NOW));
        fake_driver(&mut h, &[]);
        h.scripts.push_back(runs(60, 0));
        h.guards.push_back(guard_pass(8_000_000_000));
        h.guards.push_back(guard_refuse(1_000_000_000, "ram-low"));
        let (code, text) = run_text(&mut h, &env, &plan);
        assert_eq!(code, EXIT_FAIL, "{text}");
        assert_eq!(h.spawned.len(), 1);
        assert_eq!(h.captured.len(), 2);
        let s = session(&env, &plan, at(NOW));
        assert_eq!(
            statuses(&s),
            pairs(&[
                ("14/csr.1e5", "exit-grade"),
                ("14/t1.read", "not-run"),
                ("6/lz4.encode", "not-run"),
            ])
        );
        assert!(
            s["runs"][1]["reason"].as_str().unwrap().contains("ram-low"),
            "{s}"
        );
    }

    #[test]
    fn run_refuses_a_dirty_tree() {
        let d = TestDir::new("measure-run-tree");
        let plan = plan();
        let env = env(&d, &plan);
        type Change = fn(&mut Env);
        let cases: [Change; 2] = [|e| e.tree.changes = 1, |e| e.tree.commit = None];
        for change in cases {
            let mut e = env.clone();
            change(&mut e);
            let mut h = FakeHost::new(at(NOW));
            fake_driver(&mut h, &[]);
            let (code, text) = run_text(&mut h, &e, &plan);
            assert!(h.spawned.is_empty() && h.captured.is_empty(), "{text}");
            assert_eq!(code, EXIT_REFUSED, "{text}");
            assert!(text.contains("measure: refused: dirty-tree: "), "{text}");
            assert!(!e.scratch(&plan).exists());
        }
        // A tracked change, a moved HEAD or a moved branch while a driver runs (each alone, the other two as the
        // pre-checks found them) stops the session after that run, and the session fails.
        let moved = [
            Tree {
                changes: 1,
                ..test_tree()
            },
            Tree {
                commit: Some("f".repeat(40)),
                ..test_tree()
            },
            Tree {
                branch: Some("other".into()),
                ..test_tree()
            },
        ];
        for t in moved {
            // Each session starts afresh, so that no record of the one before makes its first run done-before.
            for dir in [env.scratch(&plan), env.private.join(MEASUREMENTS)] {
                if dir.exists() {
                    std::fs::remove_dir_all(&dir).unwrap();
                }
            }
            let mut h = FakeHost::new(at(NOW));
            fake_driver(&mut h, &[]);
            for _ in 0..3 {
                h.scripts.push_back(runs(60, 0));
            }
            h.trees.push_back(Ok(t.clone()));
            let (code, text) = run_text(&mut h, &env, &plan);
            assert_eq!(h.spawned.len(), 1, "{t:?}: {text}");
            assert_eq!(code, EXIT_FAIL, "{t:?}: {text}");
            let s = session(&env, &plan, at(NOW));
            assert_eq!(
                statuses(&s),
                pairs(&[
                    ("14/csr.1e5", "exit-grade"),
                    ("14/t1.read", "not-run"),
                    ("6/lz4.encode", "not-run"),
                ]),
                "{t:?}"
            );
            assert_eq!(
                s["runs"][0]["attempts"][0]["tree_changed"],
                json!({
                    "commit": t.commit,
                    "branch": t.branch,
                    "tracked_changes": t.changes,
                }),
                "{s}"
            );
            assert!(
                s["stopped"]
                    .as_str()
                    .unwrap()
                    .contains("the tested tree changed during 14/csr.1e5"),
                "{s}"
            );
            assert_eq!(s["verdict"], "fail", "{s}");
        }
    }

    #[test]
    fn drivers_run_under_the_contract_and_the_finish_indexes_private() {
        let d = TestDir::new("measure-run-contract");
        let plan = plan();
        let mut env = env(&d, &plan);
        env.rebuild_manifest = true;
        d.write(
            "private/notes/a.txt",
            "synthetic words for the private index of this unit test",
        );
        // An earlier invocation that ended between making a working directory and removing it (an internal error, a
        // crash, a power loss) left a partial fixture in each measurement's working directory.
        for n in [14, 6] {
            let fixture = env.work(&plan, n).join("fixture");
            std::fs::create_dir_all(&fixture).unwrap();
            std::fs::write(fixture.join("partial"), b"a crashed run's partial fixture").unwrap();
        }
        let mut h = FakeHost::new(at(NOW));
        fake_driver(&mut h, &[]);
        for _ in 0..3 {
            h.scripts.push_back(runs(60, 0));
        }
        // What each driver finds in its working directory when it starts.
        let mut driver = h.on_spawn.take().unwrap();
        let found = Rc::new(RefCell::new(Vec::new()));
        let f = found.clone();
        h.on_spawn = Some(Box::new(move |c: &Cmd| {
            let names: Vec<String> = std::fs::read_dir(&c.cwd)
                .map(|r| {
                    r.map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                        .collect()
                })
                .unwrap_or_default();
            f.borrow_mut().push(names);
            driver(c);
        }));
        let (code, text) = run_text(&mut h, &env, &plan);
        let commit = test_tree().commit.unwrap();
        let scratch = env.scratch(&plan);
        // Every driver starts in an empty working directory: each leftover was removed before its measurement's
        // first run.
        assert_eq!(*found.borrow(), vec![Vec::<String>::new(); 3], "{text}");
        // Every driver: its program, the contract's arguments and its own working directory, in plan order.
        let got: Vec<(String, Vec<String>, PathBuf)> = h
            .spawned
            .iter()
            .map(|c| (c.program.clone(), c.args.clone(), c.cwd.clone()))
            .collect();
        let want = |n: u32, driver: &str, q: &str| {
            let out = env.out(n).to_string_lossy().into_owned();
            let args = [
                "--host",
                "laptop",
                "--condition",
                "idle",
                "--quantity",
                q,
                "--commit",
                &commit,
                "--toolchain",
                TOOLCHAIN,
                "--out",
                &out,
            ];
            (
                env.exe(&plan, driver).to_string_lossy().into_owned(),
                args.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
                scratch.join(WORK).join(n.to_string()),
            )
        };
        assert_eq!(
            got,
            [
                want(14, "m14", "csr.1e5"),
                want(14, "m14", "t1.read"),
                want(6, "m6", "lz4.encode")
            ]
        );
        // Each driver's TMP and TEMP name its working directory ([MP §9.8]), in a run and in a smoke run alike.
        for c in &h.spawned {
            let cwd = c.cwd.to_string_lossy();
            assert_eq!(
                (c.get_env("TMP"), c.get_env("TEMP")),
                (Some(cwd.as_ref()), Some(cwd.as_ref())),
                "{}",
                c.line()
            );
        }
        let m6 = &plan.measurements[1];
        let smoke = driver_cmd(&env, &plan, m6, "lz4.encode", &commit, TOOLCHAIN, true);
        let work = env.work(&plan, 6).to_string_lossy().into_owned();
        assert_eq!(
            (smoke.get_env("TMP"), smoke.get_env("TEMP")),
            (Some(work.as_str()), Some(work.as_str()))
        );
        assert_eq!(code, EXIT_PASS, "{text}");
        // The guard before each attempt: the scratch volume and the two floors, no counted directory.
        assert_eq!(h.captured.len(), 3);
        let volume = scratch.to_string_lossy().into_owned();
        for g in &h.captured {
            assert_eq!(g.program, env.exe(&plan, GUARD).to_string_lossy());
            assert_eq!(
                g.args,
                [
                    "--volume",
                    volume.as_str(),
                    "--ram-floor",
                    "1500000000",
                    "--disk-floor",
                    "25000000000"
                ]
            );
        }
        // The first attempt after the settle, under its run budget.
        let t0 = at(NOW) + 15 * 60;
        let s = session(&env, &plan, at(NOW));
        assert_eq!(s["runs"][0]["attempts"][0]["started"], utc::rfc3339(t0));
        assert_eq!(
            s["runs"][0]["attempts"][0]["deadline"],
            utc::rfc3339(t0 + 30 * 60)
        );
        assert_eq!(
            statuses(&s),
            pairs(&[
                ("14/csr.1e5", "exit-grade"),
                ("14/t1.read", "exit-grade"),
                ("6/lz4.encode", "exit-grade"),
            ])
        );
        assert_eq!(s["schema"], SESSION_SCHEMA);
        assert_eq!(s["verdict"], "pass");
        assert_eq!(s["commit"], commit.as_str());
        assert_eq!(s["toolchain"], TOOLCHAIN);
        assert_eq!(s["window"]["kind"], "agent-free");
        // The logs are the session's; the working directories are gone.
        assert!(
            scratch
                .join(SESSIONS)
                .join(utc::compact(at(NOW)))
                .join("14.csr.1e5.1.log")
                .is_file()
        );
        assert!(!scratch.join(WORK).exists());
        // The private manifest was rebuilt, is current, and lists the new records.
        assert_eq!(s["manifest"], "rebuilt");
        let (m, _) = crate::private::load_current(&env.private).unwrap().unwrap();
        let paths: Vec<String> = m.files.iter().map(|f| f.path.clone()).collect();
        let rec =
            |n: u32, q: &str, t: i64| format!("measurements/{n}/{q}.idle.{}.json", utc::compact(t));
        assert_eq!(
            paths,
            [
                rec(14, "csr.1e5", t0),
                rec(14, "t1.read", t0 + 60),
                rec(6, "lz4.encode", t0 + 120),
                "notes/a.txt".to_string(),
                "windows.toml".to_string(),
            ]
        );
        assert!(
            text.contains("measure: PASS (3 of 3 runs exit-grade)"),
            "{text}"
        );
    }

    /// Measurement 14 of three quantities with a run budget of 30 minutes and a measurement budget of 50, then
    /// measurement 6.
    const BUDGETS: &str = r#"version = 1
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
quantities = ["a", "b", "c"]
[[measurement]]
n = 6
driver = "m6"
title = "synthetic"
run-budget-minutes = 20
budget-minutes = 25
quantities = ["d"]
"#;

    #[test]
    fn an_overrunning_driver_is_stopped_by_its_tree() {
        let d = TestDir::new("measure-run-budgets");
        let plan = Plan::parse(BUDGETS).unwrap();
        let env = env(&d, &plan);
        let t0 = at(NOW) + 15 * 60;
        // At its run budget: a hangs, and its process tree is stopped exactly 30 minutes after it started. It is
        // retried at once and hangs again; the measurement's budget stops the retry 50 minutes into the measurement,
        // so b and c are not run, and measurement 6 runs.
        let mut h = FakeHost::new(at(NOW));
        fake_driver(&mut h, &[]);
        h.scripts.push_back(hangs());
        h.scripts.push_back(hangs());
        h.scripts.push_back(runs(60, 0));
        let (_, text) = run_text(&mut h, &env, &plan);
        let s = session(&env, &plan, at(NOW));
        let a = &s["runs"][0]["attempts"][0];
        assert_eq!(a["ended"], utc::rfc3339(t0 + 30 * 60), "{text}");
        assert_eq!(a["outcome"], "timed-out", "{text}");
        assert_eq!(h.killed.borrow()[0], h.spawned[0].line());
        let a = &s["runs"][0]["attempts"][1];
        assert_eq!(a["ended"], utc::rfc3339(t0 + 50 * 60), "{text}");
        assert_eq!(a["outcome"], "budget-ended", "{text}");
        assert_eq!(
            statuses(&s),
            pairs(&[
                ("14/a", "budget-ended"),
                ("14/b", "not-run"),
                ("14/c", "not-run"),
                ("6/d", "exit-grade"),
            ])
        );
        assert_eq!(h.killed.borrow().len(), 2);
        assert_eq!((h.spawned.len(), h.captured.len()), (3, 3));
        std::fs::remove_dir_all(env.scratch(&plan)).unwrap();
        let _ = std::fs::remove_dir_all(env.private.join(MEASUREMENTS));
        // At the measurement's budget: b hangs after a took 25 minutes and is stopped 50 minutes into the
        // measurement; it is not retried, c is not run, and measurement 6 runs.
        let mut h = FakeHost::new(at(NOW));
        fake_driver(&mut h, &[]);
        h.scripts.push_back(runs(25 * 60, 0));
        h.scripts.push_back(hangs());
        h.scripts.push_back(runs(60, 0));
        let (code, text) = run_text(&mut h, &env, &plan);
        assert_eq!(code, EXIT_FAIL, "{text}");
        let s = session(&env, &plan, at(NOW));
        assert_eq!(
            statuses(&s),
            pairs(&[
                ("14/a", "exit-grade"),
                ("14/b", "budget-ended"),
                ("14/c", "not-run"),
                ("6/d", "exit-grade"),
            ])
        );
        assert_eq!(
            s["runs"][1]["attempts"][0]["ended"],
            utc::rfc3339(t0 + 50 * 60)
        );
        assert_eq!(s["runs"][1]["attempts"].as_array().unwrap().len(), 1);
        assert!(
            s["runs"][2]["reason"]
                .as_str()
                .unwrap()
                .contains("the measurement's budget was spent during 14/b")
        );
        assert_eq!(h.killed.borrow().as_slice(), [h.spawned[1].line()]);
        assert_eq!((h.spawned.len(), h.captured.len()), (3, 3));
        std::fs::remove_dir_all(env.scratch(&plan)).unwrap();
        let _ = std::fs::remove_dir_all(env.private.join(MEASUREMENTS));
        // At the window's end less the finish margin: a hangs from 03:35 and is stopped at 03:50; nothing else runs.
        let start = at("2026-10-14T03:20:00Z");
        let mut h = FakeHost::new(start);
        fake_driver(&mut h, &[]);
        h.scripts.push_back(hangs());
        let (code, text) = run_text(&mut h, &env, &plan);
        assert_eq!(code, EXIT_FAIL, "{text}");
        let s = session(&env, &plan, start);
        assert_eq!(
            statuses(&s),
            pairs(&[
                ("14/a", "window-ended"),
                ("14/b", "not-run"),
                ("14/c", "not-run"),
                ("6/d", "not-run"),
            ])
        );
        assert_eq!(s["runs"][0]["attempts"][0]["ended"], "2026-10-14T03:50:00Z");
        assert_eq!((h.spawned.len(), h.captured.len()), (1, 1));
        assert_eq!(h.killed.borrow().len(), 1);
    }

    #[test]
    fn a_run_that_is_not_exit_grade_is_retried_once() {
        let d = TestDir::new("measure-run-retry");
        let plan = plan();
        let env = env(&d, &plan);
        // csr.1e5: a record that is not exit-grade, then an exit-grade one: two attempts, exit-grade. t1.read: not
        // exit-grade twice: two attempts, not-exit-grade, and the next run starts. lz4.encode: exit-grade at once.
        let mut h = FakeHost::new(at(NOW));
        fake_driver(
            &mut h,
            &[
                Writes::NotExitGrade,
                Writes::ExitGrade,
                Writes::NotExitGrade,
                Writes::NotExitGrade,
                Writes::ExitGrade,
            ],
        );
        for _ in 0..5 {
            h.scripts.push_back(runs(60, 0));
        }
        let (code, text) = run_text(&mut h, &env, &plan);
        let quantities: Vec<&str> = h.spawned.iter().map(|c| c.args[5].as_str()).collect();
        assert_eq!(
            quantities,
            ["csr.1e5", "csr.1e5", "t1.read", "t1.read", "lz4.encode"],
            "{text}"
        );
        assert_eq!(code, EXIT_FAIL, "{text}");
        // The retry has its own guard.
        assert_eq!(h.captured.len(), 5);
        let s = session(&env, &plan, at(NOW));
        assert_eq!(
            statuses(&s),
            pairs(&[
                ("14/csr.1e5", "exit-grade"),
                ("14/t1.read", "not-exit-grade"),
                ("6/lz4.encode", "exit-grade"),
            ])
        );
        let a = &s["runs"][1]["attempts"];
        assert_eq!(a.as_array().unwrap().len(), 2);
        assert_eq!(a[0]["outcome"], "not-exit-grade");
        assert!(
            a[1]["why"][0]
                .as_str()
                .unwrap()
                .contains("not valid: a round-boundary reading failed"),
            "{a}"
        );
        // The retry starts at once, with its log beside the first attempt's.
        let t0 = at(NOW) + 15 * 60;
        assert_eq!(
            s["runs"][0]["attempts"][1]["started"],
            utc::rfc3339(t0 + 60)
        );
        assert_eq!(s["runs"][0]["attempts"][1]["log"], "14.csr.1e5.2.log");
        std::fs::remove_dir_all(env.scratch(&plan)).unwrap();
        let _ = std::fs::remove_dir_all(env.private.join(MEASUREMENTS));
        // A non-zero exit, a missing record and two records of one run are retried too.
        let mut h = FakeHost::new(at(NOW));
        fake_driver(
            &mut h,
            &[
                Writes::ExitGrade,
                Writes::ExitGrade,
                Writes::Nothing,
                Writes::ExitGrade,
                Writes::Two,
                Writes::ExitGrade,
            ],
        );
        h.scripts.push_back(runs(60, 3));
        for _ in 0..5 {
            h.scripts.push_back(runs(60, 0));
        }
        let (code, text) = run_text(&mut h, &env, &plan);
        assert_eq!(h.spawned.len(), 6, "{text}");
        assert_eq!(code, EXIT_PASS, "{text}");
        let s = session(&env, &plan, at(NOW));
        let first = |i: usize| {
            s["runs"][i]["attempts"][0]["why"][0]
                .as_str()
                .unwrap()
                .to_string()
        };
        assert_eq!(first(0), "the driver exited with 3");
        assert_eq!(first(1), "the driver wrote no record");
        assert_eq!(first(2), "the driver wrote 2 records of the run");
        std::fs::remove_dir_all(env.scratch(&plan)).unwrap();
        let _ = std::fs::remove_dir_all(env.private.join(MEASUREMENTS));
        // A run stopped at its run budget is retried: two attempts, each stopped by its tree at its budget.
        let plan =
            Plan::parse(&BUDGETS.replace("run-budget-minutes = 30", "run-budget-minutes = 20"))
                .unwrap();
        let mut h = FakeHost::new(at(NOW));
        fake_driver(&mut h, &[]);
        h.scripts.push_back(hangs());
        h.scripts.push_back(hangs());
        for _ in 0..3 {
            h.scripts.push_back(runs(60, 0));
        }
        let (_, text) = run_text(&mut h, &env, &plan);
        let s = session(&env, &plan, at(NOW));
        let a = &s["runs"][0]["attempts"];
        assert_eq!(a.as_array().unwrap().len(), 2, "{text}");
        assert_eq!(
            (a[0]["outcome"].as_str(), a[1]["outcome"].as_str()),
            (Some("timed-out"), Some("timed-out"))
        );
        assert_eq!(a[1]["ended"], utc::rfc3339(t0 + 40 * 60));
        assert_eq!(h.killed.borrow().len(), 2);
        assert_eq!(s["runs"][0]["status"], "timed-out");
    }

    #[test]
    fn a_snapshot_change_cools_down_before_its_retry() {
        let d = TestDir::new("measure-run-cool-down");
        let plan = plan();
        let env = env(&d, &plan);
        let t0 = at(NOW) + 15 * 60;
        // csr.1e5's record shows a signature update during the run and nothing else: its retry waits the settle
        // (15 minutes) after the first attempt ends, and is exit-grade.
        let mut h = FakeHost::new(at(NOW));
        fake_driver(&mut h, &[Writes::SnapshotChanged]);
        for _ in 0..4 {
            h.scripts.push_back(runs(60, 0));
        }
        let (code, text) = run_text(&mut h, &env, &plan);
        let s = session(&env, &plan, at(NOW));
        let a = &s["runs"][0]["attempts"];
        assert_eq!(a[1]["started"], utc::rfc3339(t0 + 60 + 15 * 60), "{text}");
        assert_eq!(code, EXIT_PASS, "{text}");
        assert_eq!(
            a[0]["why"],
            json!(["the Windows or Defender versions changed during the run"])
        );
        assert_eq!(
            (a[0].get("cool_down_secs"), &a[1]["cool_down_secs"]),
            (None, &json!(900))
        );
        assert!(
            text.contains("measure: 14/csr.1e5: the host snapshot changed during the run; cooling down 15m before the retry\n"),
            "{text}"
        );
        // The settle, the cool-down and four runs of a minute.
        assert_eq!(h.slept, 900 + 900 + 4 * 60);
        assert_eq!(
            statuses(&s),
            pairs(&[
                ("14/csr.1e5", "exit-grade"),
                ("14/t1.read", "exit-grade"),
                ("6/lz4.encode", "exit-grade"),
            ])
        );
        std::fs::remove_dir_all(env.scratch(&plan)).unwrap();
        std::fs::remove_dir_all(env.private.join(MEASUREMENTS)).unwrap();
        // A snapshot change with another reason: the retry is immediate.
        let mut h = FakeHost::new(at(NOW));
        fake_driver(&mut h, &[Writes::SnapshotAndInvalid]);
        for _ in 0..4 {
            h.scripts.push_back(runs(60, 0));
        }
        let (code, text) = run_text(&mut h, &env, &plan);
        assert_eq!(code, EXIT_PASS, "{text}");
        let s = session(&env, &plan, at(NOW));
        let a = &s["runs"][0]["attempts"];
        assert_eq!(a[0]["why"].as_array().unwrap().len(), 2, "{a}");
        assert_eq!(a[1]["started"], utc::rfc3339(t0 + 60), "{text}");
        assert_eq!(a[1].get("cool_down_secs"), None);
        assert!(!text.contains("cooling down"), "{text}");
        std::fs::remove_dir_all(env.scratch(&plan)).unwrap();
        std::fs::remove_dir_all(env.private.join(MEASUREMENTS)).unwrap();
        // The cool-down does not fit: a, under a run budget of 40 minutes in a measurement budget of 50, ends 40
        // minutes in, and 40 + 15 >= 50, so it is not retried; b, c and d run.
        let plan =
            Plan::parse(&BUDGETS.replace("run-budget-minutes = 30", "run-budget-minutes = 40"))
                .unwrap();
        let mut h = FakeHost::new(at(NOW));
        fake_driver(&mut h, &[Writes::SnapshotChanged]);
        h.scripts.push_back(runs(40 * 60, 0));
        for _ in 0..3 {
            h.scripts.push_back(runs(60, 0));
        }
        let (code, text) = run_text(&mut h, &env, &plan);
        assert_eq!(code, EXIT_FAIL, "{text}");
        let s = session(&env, &plan, at(NOW));
        assert_eq!(
            statuses(&s),
            pairs(&[
                ("14/a", "not-exit-grade"),
                ("14/b", "exit-grade"),
                ("14/c", "exit-grade"),
                ("6/d", "exit-grade"),
            ])
        );
        assert_eq!(s["runs"][0]["attempts"].as_array().unwrap().len(), 1);
        assert!(
            text.contains("measure: 14/a: the host snapshot changed during the run, and a cool-down of 15m before the retry does not fit before 2026-10-13T22:05:00Z; it is not retried\n"),
            "{text}"
        );
    }

    /// A test's look at a host call.
    type Hook = Box<dyn FnMut()>;

    /// [`FakeHost`] for the tests of the runner's failure paths. With `stuck`, every driver's `kill_tree` fails, as the
    /// real stop fails when a driver has not exited a minute after `taskkill`, and is counted in `kills`.
    /// `on_capture` runs before each guard capture with its number (from 1), `on_sleep` before each sleep and
    /// `on_tree` before each read of the tested tree.
    struct Hooked {
        h: FakeHost,
        stuck: bool,
        kills: Rc<Cell<u32>>,
        captures: usize,
        on_capture: Option<Box<dyn FnMut(usize)>>,
        on_sleep: Option<Hook>,
        on_tree: Option<Hook>,
    }

    fn hooked(h: FakeHost) -> Hooked {
        Hooked {
            h,
            stuck: false,
            kills: Rc::new(Cell::new(0)),
            captures: 0,
            on_capture: None,
            on_sleep: None,
            on_tree: None,
        }
    }

    /// A driver process of [`Hooked`] whose tree cannot be stopped.
    struct StuckProc {
        p: Box<dyn Proc>,
        kills: Rc<Cell<u32>>,
    }

    impl Proc for StuckProc {
        fn try_wait(&mut self) -> Result<Option<i32>, String> {
            self.p.try_wait()
        }
        fn kill_tree(&mut self) -> Result<(), String> {
            self.kills.set(self.kills.get() + 1);
            Err("process 4242 did not exit after it was stopped".into())
        }
    }

    impl Host for Hooked {
        fn now(&self) -> i64 {
            self.h.now()
        }
        fn sleep(&mut self, secs: u64) {
            if let Some(f) = self.on_sleep.as_mut() {
                f();
            }
            self.h.sleep(secs)
        }
        fn spawn(&mut self, cmd: &Cmd, log: &Path) -> Result<Box<dyn Proc>, String> {
            let p = self.h.spawn(cmd, log)?;
            Ok(if self.stuck {
                Box::new(StuckProc {
                    p,
                    kills: self.kills.clone(),
                })
            } else {
                p
            })
        }
        fn capture(&mut self, cmd: &Cmd, timeout_secs: u64) -> Result<Captured, String> {
            self.captures += 1;
            if let Some(f) = self.on_capture.as_mut() {
                f(self.captures);
            }
            self.h.capture(cmd, timeout_secs)
        }
        fn tree(&mut self, repo: &Path) -> Result<Tree, String> {
            if let Some(f) = self.on_tree.as_mut() {
                f();
            }
            self.h.tree(repo)
        }
    }

    /// `run` on a [`Hooked`] host: its result, and what it printed.
    fn run_hooked(h: &mut Hooked, env: &Env, plan: &Plan) -> (Result<u8, String>, String) {
        let mut out = Vec::new();
        let code = run(h, env, plan, Path::new("plan.toml"), false, &mut out);
        (code, String::from_utf8(out).unwrap())
    }

    #[test]
    fn an_unstoppable_tree_stops_the_session_with_no_retry() {
        let d = TestDir::new("measure-run-unstoppable");
        let plan = plan();
        let env = env(&d, &plan);
        // csr.1e5 hangs to its run budget, and its process tree cannot be stopped: it is not retried beside that tree,
        // and nothing else runs.
        let mut h = hooked(FakeHost::new(at(NOW)));
        h.stuck = true;
        fake_driver(&mut h.h, &[]);
        h.h.scripts.push_back(hangs());
        for _ in 0..3 {
            h.h.scripts.push_back(runs(60, 0));
        }
        let (code, text) = run_hooked(&mut h, &env, &plan);
        assert_eq!(h.h.spawned.len(), 1, "{text}");
        assert_eq!(h.kills.get(), 1, "{text}");
        assert_eq!(code, Ok(EXIT_FAIL), "{text}");
        let s = session(&env, &plan, at(NOW));
        assert_eq!(
            statuses(&s),
            pairs(&[
                ("14/csr.1e5", "error"),
                ("14/t1.read", "not-run"),
                ("6/lz4.encode", "not-run"),
            ])
        );
        let a = &s["runs"][0]["attempts"];
        assert_eq!(a.as_array().unwrap().len(), 1, "{s}");
        assert_eq!(
            a[0]["why"],
            json!(["tree-unstoppable: process 4242 did not exit after it was stopped"])
        );
        let stopped = s["stopped"].as_str().unwrap();
        assert!(
            stopped.starts_with(
                "tree-unstoppable: the process tree of 14/csr.1e5 could not be stopped (process 4242 did not exit after it was stopped); start the runner again only after that tree is gone"
            ),
            "{stopped}"
        );
        assert_eq!(s["runs"][1]["reason"], stopped);
        assert_eq!(s["verdict"], "fail");
        // The tested tree changed during the same attempt: the stop is still tree-unstoppable, and the change is
        // recorded.
        std::fs::remove_dir_all(env.scratch(&plan)).unwrap();
        let _ = std::fs::remove_dir_all(env.private.join(MEASUREMENTS));
        let mut h = hooked(FakeHost::new(at(NOW)));
        h.stuck = true;
        fake_driver(&mut h.h, &[]);
        h.h.scripts.push_back(hangs());
        h.h.trees.push_back(Ok(Tree {
            changes: 1,
            ..test_tree()
        }));
        let (code, text) = run_hooked(&mut h, &env, &plan);
        assert_eq!(code, Ok(EXIT_FAIL), "{text}");
        let s = session(&env, &plan, at(NOW));
        assert!(
            s["stopped"]
                .as_str()
                .unwrap()
                .starts_with("tree-unstoppable: "),
            "{s}"
        );
        assert_eq!(
            s["runs"][0]["attempts"][0]["tree_changed"]["tracked_changes"], 1,
            "{s}"
        );
        // The driver also changed an earlier record during the same attempt: the stop is still tree-unstoppable, which
        // tells whoever restarts the night to wait for that tree ([MP §9.8]), and the changed record is recorded.
        std::fs::remove_dir_all(env.scratch(&plan)).unwrap();
        let _ = std::fs::remove_dir_all(env.private.join(MEASUREMENTS));
        let other =
            record::tests::exit_grade(14, "csr.1e5", OTHER, "2026-10-01T00:00:00Z").to_string();
        earlier(&env, 14, "csr.1e5.idle.20261001T000000Z.json", &other);
        let mut h = hooked(FakeHost::new(at(NOW)));
        h.stuck = true;
        fake_driver(&mut h.h, &[Writes::Overwrite]);
        h.h.scripts.push_back(hangs());
        let (code, text) = run_hooked(&mut h, &env, &plan);
        assert_eq!(code, Ok(EXIT_FAIL), "{text}");
        assert_eq!(h.h.spawned.len(), 1, "{text}");
        let s = session(&env, &plan, at(NOW));
        let stopped = s["stopped"].as_str().unwrap();
        assert!(
            stopped.starts_with(
                "tree-unstoppable: the process tree of 14/csr.1e5 could not be stopped"
            ),
            "{s}"
        );
        assert_eq!(s["runs"][1]["reason"], stopped);
        let a = &s["runs"][0]["attempts"][0];
        assert_eq!(
            a["records_changed"],
            json!(["csr.1e5.idle.20261001T000000Z.json"]),
            "{s}"
        );
        assert_eq!(
            a["why"],
            json!(["tree-unstoppable: process 4242 did not exit after it was stopped"])
        );
    }

    #[test]
    fn a_failed_session_save_is_retried_then_a_warning() {
        let d = TestDir::new("measure-run-save");
        let plan = plan();
        let env = env(&d, &plan);
        let tmp = env
            .scratch(&plan)
            .join(SESSIONS)
            .join(utc::compact(at(NOW)))
            .join("session.json.tmp");
        // From the first driver's start a directory stands where the save writes its temporary file: every later save
        // fails three times, with waits of 1 s and 2 s, and warns; the runs go on, and every record is written.
        let mut h = hooked(FakeHost::new(at(NOW)));
        fake_driver(&mut h.h, &[]);
        for _ in 0..3 {
            h.h.scripts.push_back(runs(60, 0));
        }
        let mut driver = h.h.on_spawn.take().unwrap();
        let t = tmp.clone();
        h.h.on_spawn = Some(Box::new(move |c: &Cmd| {
            std::fs::create_dir_all(&t).unwrap();
            driver(c);
        }));
        let (code, text) = run_hooked(&mut h, &env, &plan);
        assert_eq!(code, Ok(EXIT_PASS), "{text}");
        // Three runs and the finish.
        assert_eq!(
            text.matches("measure: warning: the session record ")
                .count(),
            4,
            "{text}"
        );
        assert!(
            text.contains("could not be saved after 3 tries ("),
            "{text}"
        );
        assert_eq!(h.h.slept, 900 + 3 * 60 + 4 * (1 + 2), "{text}");
        assert!(
            text.contains("measure: PASS (3 of 3 runs exit-grade)"),
            "{text}"
        );
        assert_eq!(record::list(&env.out(14)).unwrap().len(), 2);
        assert_eq!(record::list(&env.out(6)).unwrap().len(), 1);
        // The session record holds what the first save wrote.
        assert_eq!(session(&env, &plan, at(NOW))["runs"], json!([]));
        std::fs::remove_dir_all(env.scratch(&plan)).unwrap();
        std::fs::remove_dir_all(env.private.join(MEASUREMENTS)).unwrap();
        // A save that fails once: its obstacle is made at the first tree read, after the first attempt, and removed at
        // the next sleep, which is that save's first wait. The second try saves, and nothing warns.
        let mut h = hooked(FakeHost::new(at(NOW)));
        fake_driver(&mut h.h, &[]);
        for _ in 0..3 {
            h.h.scripts.push_back(runs(60, 0));
        }
        let armed = Rc::new(Cell::new(false));
        let (t, a, trees) = (tmp.clone(), armed.clone(), Rc::new(Cell::new(0)));
        h.on_tree = Some(Box::new(move || {
            trees.set(trees.get() + 1);
            if trees.get() == 1 {
                std::fs::create_dir_all(&t).unwrap();
                a.set(true);
            }
        }));
        let t = tmp.clone();
        h.on_sleep = Some(Box::new(move || {
            if armed.replace(false) {
                std::fs::remove_dir(&t).unwrap();
            }
        }));
        let (code, text) = run_hooked(&mut h, &env, &plan);
        assert_eq!(code, Ok(EXIT_PASS), "{text}");
        assert!(!text.contains("warning"), "{text}");
        assert_eq!(h.h.slept, 900 + 3 * 60 + 1, "{text}");
        let s = session(&env, &plan, at(NOW));
        assert_eq!(statuses(&s).len(), 3, "{s}");
        assert_eq!(s["verdict"], "pass");
    }

    #[test]
    fn a_failed_listing_ends_only_its_attempt() {
        let d = TestDir::new("measure-run-listing");
        let plan = plan();
        let env = env(&d, &plan);
        // The first driver writes its record and then leaves a file where measurement 14's record directory was, so
        // the listing after it fails; the retry's guard puts the directory back.
        let mut h = hooked(FakeHost::new(at(NOW)));
        fake_driver(&mut h.h, &[]);
        for _ in 0..4 {
            h.h.scripts.push_back(runs(60, 0));
        }
        let mut driver = h.h.on_spawn.take().unwrap();
        let (o, back) = (env.out(14), env.out(14));
        let mut first = true;
        h.h.on_spawn = Some(Box::new(move |c: &Cmd| {
            driver(c);
            if std::mem::take(&mut first) {
                std::fs::remove_dir_all(&o).unwrap();
                std::fs::write(&o, b"not a directory").unwrap();
            }
        }));
        h.on_capture = Some(Box::new(move |n| {
            if n == 2 {
                std::fs::remove_file(&back).unwrap();
                std::fs::create_dir_all(&back).unwrap();
            }
        }));
        let (code, text) = run_hooked(&mut h, &env, &plan);
        assert_eq!(code, Ok(EXIT_PASS), "{text}");
        let s = session(&env, &plan, at(NOW));
        let a = &s["runs"][0]["attempts"];
        assert_eq!(a[0]["outcome"], "error", "{a}");
        assert!(
            a[0]["why"][0]
                .as_str()
                .unwrap()
                .starts_with("the record directory could not be listed after the run: "),
            "{a}"
        );
        // No record is judged, and no record is checked for a change during that attempt.
        assert_eq!(a[0]["records"], json!([]), "{a}");
        assert_eq!(a[0].get("records_changed"), None, "{a}");
        assert_eq!(a[1]["outcome"], "exit-grade", "{a}");
        assert_eq!(
            statuses(&s),
            pairs(&[
                ("14/csr.1e5", "exit-grade"),
                ("14/t1.read", "exit-grade"),
                ("6/lz4.encode", "exit-grade"),
            ])
        );
        assert_eq!(h.h.spawned.len(), 4);
    }

    #[test]
    fn a_failed_resume_listing_or_directory_ends_only_its_attempts() {
        let d = TestDir::new("measure-run-unlistable");
        let plan = plan();
        let env = env(&d, &plan);
        // A file stands where measurement 14's record directory belongs: resume cannot list it, and no attempt can make
        // it. Each run of 14 counts no earlier record, both its attempts end as errors before a driver starts, and
        // measurement 6 runs.
        std::fs::create_dir_all(env.private.join(MEASUREMENTS)).unwrap();
        std::fs::write(env.out(14), b"not a directory").unwrap();
        let mut h = hooked(FakeHost::new(at(NOW)));
        fake_driver(&mut h.h, &[]);
        h.h.scripts.push_back(runs(60, 0));
        let (code, text) = run_hooked(&mut h, &env, &plan);
        assert_eq!(code, Ok(EXIT_FAIL), "{text}");
        let s = session(&env, &plan, at(NOW));
        assert_eq!(
            statuses(&s),
            pairs(&[
                ("14/csr.1e5", "error"),
                ("14/t1.read", "error"),
                ("6/lz4.encode", "exit-grade"),
            ])
        );
        for r in &s["runs"].as_array().unwrap()[..2] {
            assert!(r["resume_error"].is_string(), "{r}");
            let a = r["attempts"].as_array().unwrap();
            assert_eq!(a.len(), 2, "{r}");
            for x in a {
                assert_eq!(x["outcome"], "error", "{x}");
                assert!(
                    x["why"][0]
                        .as_str()
                        .unwrap()
                        .starts_with("the record directory could not be made or listed: "),
                    "{x}"
                );
                // No driver command: none started.
                assert_eq!(x.get("command"), None, "{x}");
            }
        }
        assert!(
            text.contains("measure: 14/csr.1e5: the earlier records could not be listed ("),
            "{text}"
        );
        // Only measurement 6's driver started, and every attempt had its guard.
        assert_eq!((h.h.spawned.len(), h.h.captured.len()), (1, 5));
    }

    #[test]
    fn the_session_record_names_the_resident_processes() {
        let d = TestDir::new("measure-run-residents");
        let plan = plan();
        let mut env = env(&d, &plan);
        RESIDENT_READS.with(|c| c.set(0));
        // The image names resident at the session's start and at its end, each in its own field.
        let mut h = FakeHost::new(at(NOW));
        fake_driver(&mut h, &[]);
        for _ in 0..3 {
            h.scripts.push_back(runs(60, 0));
        }
        let (code, text) = run_text(&mut h, &env, &plan);
        let s = session(&env, &plan, at(NOW));
        assert_eq!(
            s["residents"],
            json!({
                "start": { "ok": ["agent-app.exe", "explorer.exe"] },
                "end": { "ok": ["agent-app.exe", "notepad.exe"] },
            }),
            "{text}"
        );
        assert_eq!(code, EXIT_PASS, "{text}");
        // A list that cannot be read is recorded with its reason, and the session still passes.
        std::fs::remove_dir_all(env.scratch(&plan)).unwrap();
        std::fs::remove_dir_all(env.private.join(MEASUREMENTS)).unwrap();
        env.residents = || Err("tasklist.exe exited with Some(1): synthetic".into());
        let mut h = FakeHost::new(at(NOW));
        fake_driver(&mut h, &[]);
        for _ in 0..3 {
            h.scripts.push_back(runs(60, 0));
        }
        let (code, text) = run_text(&mut h, &env, &plan);
        assert_eq!(code, EXIT_PASS, "{text}");
        let error = json!({ "error": "tasklist.exe exited with Some(1): synthetic" });
        assert_eq!(
            session(&env, &plan, at(NOW))["residents"],
            json!({ "start": error, "end": error })
        );
    }

    /// Writes a record into `/private/measurements/<n>/` before a session.
    fn earlier(env: &Env, n: u32, name: &str, text: &str) -> PathBuf {
        let dir = env.out(n);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        std::fs::write(&p, text).unwrap();
        p
    }

    const OTHER: &str = "fedcba9876543210fedcba9876543210fedcba98";

    #[test]
    fn records_are_never_overwritten() {
        let d = TestDir::new("measure-run-records");
        let plan = plan();
        let env = env(&d, &plan);
        let head = test_tree().commit.unwrap();
        // Earlier records: an exit-grade one of another commit, one of the measured commit that is not exit-grade,
        // and a truncated one.
        let other =
            record::tests::exit_grade(14, "csr.1e5", OTHER, "2026-10-01T00:00:00Z").to_string();
        let mut invalid = record::tests::exit_grade(14, "csr.1e5", &head, "2026-10-02T00:00:00Z");
        invalid["reasons"] = json!(["a round-boundary reading failed"]);
        let invalid = invalid.to_string();
        let truncated = record::tests::exit_grade(14, "t1.read", &head, "2026-10-03T00:00:00Z")
            .to_string()[..100]
            .to_string();
        let files = [
            (
                earlier(&env, 14, "csr.1e5.idle.20261001T000000Z.json", &other),
                other,
            ),
            (
                earlier(&env, 14, "csr.1e5.idle.20261002T000000Z.json", &invalid),
                invalid,
            ),
            (
                earlier(&env, 14, "t1.read.idle.20261003T000000Z.json", &truncated),
                truncated,
            ),
        ];
        let times: Vec<_> = files
            .iter()
            .map(|(p, _)| std::fs::metadata(p).unwrap().modified().unwrap())
            .collect();
        // A session with a retry: every earlier record, and the first attempt's record, stay as they were.
        let mut h = FakeHost::new(at(NOW));
        fake_driver(&mut h, &[Writes::NotExitGrade]);
        for _ in 0..4 {
            h.scripts.push_back(runs(60, 0));
        }
        let (code, text) = run_text(&mut h, &env, &plan);
        assert_eq!(code, EXIT_PASS, "{text}");
        for ((p, text), t) in files.iter().zip(&times) {
            assert_eq!(
                &std::fs::read_to_string(p).unwrap(),
                text,
                "{}",
                p.display()
            );
            assert_eq!(&std::fs::metadata(p).unwrap().modified().unwrap(), t);
        }
        let s = session(&env, &plan, at(NOW));
        let first = s["runs"][0]["attempts"][0]["records"][0].as_str().unwrap();
        assert!(env.out(14).join(first).is_file(), "{first}");
        assert_eq!(record::list(&env.out(14)).unwrap().len(), 3 + 3);
        // No file of the session lies under /private/ but the records.
        assert_eq!(env.out(6).read_dir().unwrap().count(), 1);
        std::fs::remove_dir_all(env.scratch(&plan)).unwrap();
        // A driver that changes an earlier record stops the session: record-changed, exit 1. (The session's own
        // records are removed first, so no run is done before.)
        std::fs::remove_dir_all(env.private.join(MEASUREMENTS)).unwrap();
        for (p, text) in &files {
            earlier(&env, 14, &p.file_name().unwrap().to_string_lossy(), text);
        }
        let mut h = FakeHost::new(at(NOW));
        fake_driver(&mut h, &[Writes::Overwrite]);
        for _ in 0..3 {
            h.scripts.push_back(runs(60, 0));
        }
        let (code, text) = run_text(&mut h, &env, &plan);
        assert_eq!(code, EXIT_FAIL, "{text}");
        let s = session(&env, &plan, at(NOW));
        assert!(
            s["stopped"]
                .as_str()
                .unwrap()
                .starts_with("record-changed: csr.1e5.idle.20261001T000000Z.json"),
            "{s}"
        );
        assert_eq!(
            s["runs"][0]["attempts"][0]["records_changed"],
            json!(["csr.1e5.idle.20261001T000000Z.json"])
        );
        assert_eq!(h.spawned.len(), 1);
        // A scratch directory below /private/, or one that holds it, is refused.
        for scratch in [env.private.join("scratch"), d.path().to_path_buf()] {
            let mut p = plan.clone();
            p.scratch = scratch.to_string_lossy().into_owned();
            let kinds = kinds(&env, &p, at(NOW));
            assert!(
                kinds.contains(&"scratch"),
                "{} {kinds:?}",
                scratch.display()
            );
        }
    }

    #[test]
    fn resume_skips_runs_with_an_exit_grade_record() {
        let d = TestDir::new("measure-run-resume");
        let plan = plan();
        let env = env(&d, &plan);
        let head = test_tree().commit.unwrap();
        // csr.1e5 has an exit-grade record of the measured commit; t1.read has one of another commit; lz4.encode has one
        // of the measured commit that is not exit-grade.
        let done = "csr.1e5.idle.20261013T200500Z.json";
        earlier(
            &env,
            14,
            done,
            &record::tests::exit_grade(14, "csr.1e5", &head, "2026-10-13T20:05:00Z").to_string(),
        );
        earlier(
            &env,
            14,
            "t1.read.idle.20261013T200600Z.json",
            &record::tests::exit_grade(14, "t1.read", OTHER, "2026-10-13T20:06:00Z").to_string(),
        );
        let mut invalid = record::tests::exit_grade(6, "lz4.encode", &head, "2026-10-13T20:07:00Z");
        invalid["reasons"] = json!(["a round-boundary reading failed"]);
        earlier(
            &env,
            6,
            "lz4.encode.idle.20261013T200700Z.json",
            &invalid.to_string(),
        );
        // check marks the done run, and only it.
        let mut h = FakeHost::new(at(NOW));
        let (code, text) = check_text(&mut h, &env, &plan, false);
        assert_eq!(code, EXIT_PASS, "{text}");
        assert!(
            text.contains(&format!("measure: run 14/csr.1e5: done before ({done})\n")),
            "{text}"
        );
        assert!(text.contains("measure: run 14/t1.read: budget"), "{text}");
        assert!(text.contains("measure: run 6/lz4.encode: budget"), "{text}");
        // run starts no driver for it, and the session passes with the two runs it makes.
        fake_driver(&mut h, &[]);
        h.scripts.push_back(runs(60, 0));
        h.scripts.push_back(runs(60, 0));
        let (code, text) = run_text(&mut h, &env, &plan);
        let quantities: Vec<&str> = h.spawned.iter().map(|c| c.args[5].as_str()).collect();
        assert_eq!(quantities, ["t1.read", "lz4.encode"], "{text}");
        assert_eq!(code, EXIT_PASS, "{text}");
        let s = session(&env, &plan, at(NOW));
        assert_eq!(
            statuses(&s),
            pairs(&[
                ("14/csr.1e5", "done-before"),
                ("14/t1.read", "exit-grade"),
                ("6/lz4.encode", "exit-grade"),
            ])
        );
        assert_eq!(s["runs"][0]["record"], done);
        assert_eq!(s["counts"]["exit_grade"], 3);
    }

    #[test]
    fn smoke_ignores_the_window_and_records_nothing() {
        let d = TestDir::new("measure-run-smoke");
        let plan = plan();
        let mut env = env(&d, &plan);
        env.rebuild_manifest = true;
        // Outside every window, and with no calendar at all: --smoke needs none.
        env.windows = d.path().join("none.toml");
        let start = at("2026-10-15T00:00:00Z");
        let k: Vec<&str> = pre_checks(&env, &plan, start, true)
            .refusals
            .iter()
            .map(|r| r.kind)
            .collect();
        assert!(k.is_empty(), "{k:?}");
        let smoke = |h: &mut FakeHost| {
            let mut out = Vec::new();
            let code = run(h, &env, &plan, Path::new("plan.toml"), true, &mut out).unwrap();
            (code, String::from_utf8(out).unwrap())
        };
        let mut h = FakeHost::new(start);
        fake_driver(&mut h, &[]);
        for _ in 0..3 {
            h.scripts.push_back(runs(5, 0));
        }
        let (code, text) = smoke(&mut h);
        // Each run once, with --smoke in place of --out.
        let commit = test_tree().commit.unwrap();
        let args: Vec<Vec<String>> = h.spawned.iter().map(|c| c.args.clone()).collect();
        let want = |q: &str| {
            [
                "--host",
                "laptop",
                "--condition",
                "idle",
                "--quantity",
                q,
                "--commit",
                &commit,
                "--toolchain",
                TOOLCHAIN,
                "--smoke",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
        };
        assert_eq!(
            args,
            [want("csr.1e5"), want("t1.read"), want("lz4.encode")],
            "{text}"
        );
        assert_eq!(code, EXIT_PASS, "{text}");
        // The guard before each run; no settle (the clock moved only while the drivers ran, 5 s each).
        assert_eq!(h.captured.len(), 3);
        assert_eq!(h.slept, 15);
        // Nothing under /private/: no record, no directory, no manifest.
        assert_eq!(listing(&env.private), [env.private.join("windows.toml")]);
        // The session record is the smoke session's, outside /private/.
        let p = env
            .scratch(&plan)
            .join(SESSIONS)
            .join(format!("{}.smoke", utc::compact(start)))
            .join(SESSION);
        let s: Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(s["mode"], "smoke");
        assert_eq!(s["window"], Value::Null);
        assert_eq!(s["manifest"], "not rebuilt (smoke)");
        assert_eq!(s["verdict"], "pass");
        assert_eq!(
            statuses(&s),
            pairs(&[
                ("14/csr.1e5", "passed"),
                ("14/t1.read", "passed"),
                ("6/lz4.encode", "passed"),
            ])
        );
        assert!(!env.scratch(&plan).join(WORK).exists());
        // A driver that exits non-zero, or overruns the smoke budget and is stopped by its tree, fails the smoke run;
        // nothing is retried.
        let start = start + 3_600;
        let mut h = FakeHost::new(start);
        fake_driver(&mut h, &[]);
        h.scripts.push_back(runs(5, 0));
        h.scripts.push_back(runs(5, 2));
        h.scripts.push_back(hangs());
        let (code, text) = smoke(&mut h);
        assert_eq!(code, EXIT_FAIL, "{text}");
        assert_eq!(h.spawned.len(), 3);
        assert_eq!(h.killed.borrow().len(), 1);
        let p = env
            .scratch(&plan)
            .join(SESSIONS)
            .join(format!("{}.smoke", utc::compact(start)))
            .join(SESSION);
        let s: Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(
            statuses(&s),
            pairs(&[
                ("14/csr.1e5", "passed"),
                ("14/t1.read", "failed"),
                ("6/lz4.encode", "timed-out"),
            ])
        );
        // The overrunning driver was stopped at the smoke budget of 10 minutes.
        assert_eq!(
            s["runs"][2]["attempts"][0]["ended"],
            utc::rfc3339(start + 10 + 10 * 60)
        );
        assert_eq!(listing(&env.private), [env.private.join("windows.toml")]);
    }
}
