//! `cargo xtask nightly` (WP-05, docs/m0/PLAN.md §3.2 item 7): the profile-L nightly runner. `docs/m0/nightly.md` is
//! its reference; `[MP §x]` cites `docs/spec/measurement-protocol.md`. Sources: PLAN §2.1 (target directories, test
//! tiers), §3.2 WP-05, §5 V4; [60 §3.13] (GT1, GT5, GT16, GT18), [60 §3.15] (profile L); [AR §8.3].
//!
//! | Command | What it does |
//! |---|---|
//! | `nightly check [--private-dir <dir>] [--windows <file>] [--now <date-time>] [--guard <exe>] [--inject-… ]` | the pre-checks and the plan; starts no job |
//! | `nightly run [--private-dir <dir>] [--guard <exe>]` | the pre-checks, then the job list inside the current window |
//!
//! **Pre-checks** ([`pre_check`]); every refusal is listed, and any one stops the run before anything starts:
//! - `host`: the runner runs on the Windows laptop of profile L (it stops process trees with `taskkill.exe`);
//! - `config`: `xtask/nightly.toml` and `xtask/roles.toml` load ([`config`]);
//! - `calendar`, `outside-window`, `window-ending`: `/private/windows.toml` loads ([`calendar`]), a window holds now,
//!   and more than the finish margin of it is left;
//! - `run-locked`: no other nightly run holds `/private/nightly/run.lock`; pre-checks that pass keep it locked, and
//!   the run holds it until it ends, so a second runner is refused even while the first builds its guard;
//! - `dirty-tree`: the working tree has no change and no untracked file, so the results name the commit they tested;
//! - `lane-busy`: in an agent-free window, no lane target directory has a cargo build running (its build-directory
//!   lock, the lane's build semaphore of PLAN §2.1, is free);
//! - the guard's `ram-low`, `disk-low`, `ram-unreadable`, `disk-unreadable` and `dir-unreadable` ([MP §8.1]), with
//!   the RAM floor (1.5 GB), the disk floor (25 GB) and the four counted directories of PLAN §2.1 with their caps;
//!   `guard` when the guard cannot be built or run. The guard runs when every check before it passed, or at once when
//!   `--guard` names a built one.
//!
//! **The run** ([`run`]): the jobs of `xtask/nightly.toml` one at a time, in file order, each under the caps of the
//! window's kind (`CARGO_BUILD_JOBS`, `RUST_TEST_THREADS`, the parallel-process cap, the RAM budget; beside agents
//! 1 GB, PLAN WP-05) and its own time limits, every one ending by the deadline, the window's end less the finish margin
//! (E10). Before each job and every `watch-seconds` while it runs, the guard checks the RAM floor alone; a refusal
//! stops the job and the run ([60 §3.15]: "everything refused below 1.5 GB free"). After each job that ran, the tested
//! tree is read again: a moved `HEAD` or a changed tracked file stops the run and fails it, since the results would no
//! longer name one commit. The rotating jobs (fuzz targets, the mutants shard) advance by their turn, the count of
//! earlier runs in which they started. Raw results go to `/private/nightly/<start>.partial/` while the run lasts (left
//! out of the private manifest, so commits beside agents keep working).
//!
//! **The finish**, after the last job: the directories of runs a crash left (`<start>.partial`) are renamed
//! `<start>.aborted`, the run's own directory becomes `/private/nightly/<start>/`, the oldest runs beyond `keep-runs`
//! are removed, the private manifest is rebuilt, and `<start>/finish.json` (left out of the manifest) records how long
//! each step took and whether the run ended inside its window (E10).
//!
//! **Verdicts** ([`RunVerdict`]): `pass` (every job passed), `pass-partial` (none failed, some were skipped),
//! `no-job-ran` (every job was skipped), `fail` (a job failed, timed out, was stopped or had an error, or the tree
//! changed).
//!
//! Exit codes: 0 `pass` or `pass-partial`; 1 `fail`; 2 usage error; 3 refused by a pre-check; 4 the runner itself
//! failed (an internal error, such as git or a record that cannot be written; a run's directory stays `.partial`);
//! 5 `no-job-ran`.

mod calendar;
mod config;
mod exec;
mod guard;
mod jobs;

use crate::git;
use crate::utc;
use calendar::{Window, WindowKind};
use config::{Config, Job, JobKind, KindCaps};
use exec::{Cmd, Ended, Host, RealHost, Watch};
use serde_json::{Value, json};
use std::fs::{File, OpenOptions, TryLockError};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// The usage text.
pub const USAGE: &str = "usage: cargo xtask nightly check [--private-dir <dir>] [--windows <file>] [--now <date-time>] [--guard <exe>]
                                   [--inject-available-physical <bytes>] [--inject-volume-available <bytes>]
                                   [--inject-dir-size <dir> <bytes>]...
       cargo xtask nightly run [--private-dir <dir>] [--guard <exe>]
check makes the pre-checks (host, xtask/nightly.toml, the window calendar /private/windows.toml, the run lock, a
clean tree, idle lanes in an agent-free window, and moirai-probes-bin guard's RAM and disk floors) and prints the plan;
run makes them and then runs the job list inside the current window, with raw results in /private/nightly/.
--windows, --now and --inject-* (passed to the guard) are for check only: a real run uses the agreed calendar, the
clock and real readings; --inject-dir-size names a counted directory (laneA, laneB, fuzz, mutants) or its path.
--guard names a built guard instead of building it. Exit codes: 0 pass (or pass-partial: some jobs skipped),
1 fail, 2 usage error, 3 refused, 4 internal error, 5 no job ran. See docs/m0/nightly.md.";

/// Exit code: every job that ran passed and at least one ran (`check`: the run would start).
pub const EXIT_PASS: u8 = 0;
/// Exit code: a job failed, timed out, was stopped or could not run, or the tested tree changed.
pub const EXIT_FAIL: u8 = 1;
/// Exit code: usage error.
pub const EXIT_USAGE: u8 = 2;
/// Exit code: a pre-check refused.
pub const EXIT_REFUSED: u8 = 3;
/// Exit code: the runner itself failed (git unreadable, a record that cannot be written or renamed).
pub const EXIT_INTERNAL: u8 = 4;
/// Exit code: every job was skipped, so nothing was tested.
pub const EXIT_NO_JOB: u8 = 5;

/// The directory under `/private/` that holds the raw results (PLAN §2.5, WP-05).
pub const NIGHTLY_DIR: &str = "nightly";
/// The run lock under [`NIGHTLY_DIR`]; the private manifest leaves it out.
pub const RUN_LOCK: &str = "run.lock";
/// The suffix of the directory a running run writes into; the private manifest leaves such directories out.
pub const PARTIAL: &str = ".partial";
/// The suffix a run's directory gets when the run ended without sealing it (a crash or a power loss).
pub const ABORTED: &str = ".aborted";
/// The run record's file name and schema.
pub const RECORD: &str = "run.json";
pub const RECORD_SCHEMA: &str = "moirai-xtask/nightly/1";
/// The file a run writes into its sealed directory after the private manifest is rebuilt, and its schema: how long
/// each step of the finish took and whether the run ended inside its window (E10). It holds durations and a flag
/// only, and the private manifest leaves it out, so writing it keeps the manifest current.
pub const FINISH: &str = "finish.json";
pub const FINISH_SCHEMA: &str = "moirai-xtask/nightly-finish/1";
/// The directory under the target root that holds the runner's copy of the guard, outside every lane's target
/// directory, so no `cargo build` replaces it and no `cargo clean` removes it during a run.
pub const GUARD_DIR: &str = "nightly";
/// The longest the guard's build may take (seconds).
const GUARD_BUILD_SECS: u64 = 1_800;

/// Whether a directory name under `/private/nightly/` is a run being written ([`PARTIAL`]), which the private
/// manifest leaves out (`xtask/src/private.rs`).
pub fn partial_dir(name: &str) -> bool {
    name.ends_with(PARTIAL)
}

/// Whether `name` in the directory `parent` (relative to `/private/`, with `/`) is a run's [`FINISH`] file, which
/// the private manifest leaves out (`xtask/src/private.rs`).
pub fn finish_file(parent: &str, name: &str) -> bool {
    name == FINISH
        && parent
            .strip_prefix(NIGHTLY_DIR)
            .and_then(|r| r.strip_prefix('/'))
            .and_then(run_stem)
            .is_some()
}

/// Whether a file at `rel` (relative to `/private/`, with `/`) gives the private manifest no shingles: the nightly
/// runner's results are machine output of tests over synthetic data, which quote the repository (test names, panic
/// messages, mutated source lines), so their shingles would refuse a branch's own commits. Their hashes are listed.
pub fn unshingled(rel: &str) -> bool {
    rel.strip_prefix(NIGHTLY_DIR)
        .is_some_and(|r| r.starts_with('/'))
}

/// The start stamp of a run directory's name (`<start>`, `<start>.partial` or `<start>.aborted`), or `None`.
fn run_stem(name: &str) -> Option<&str> {
    let stem = name
        .strip_suffix(PARTIAL)
        .or_else(|| name.strip_suffix(ABORTED))
        .unwrap_or(name);
    utc::is_compact(stem).then_some(stem)
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum Mode {
    Check,
    Run,
}

#[derive(Debug, Eq, PartialEq)]
struct Opts {
    mode: Mode,
    private_dir: Option<PathBuf>,
    windows: Option<PathBuf>,
    now: Option<i64>,
    guard: Option<PathBuf>,
    /// The guard's `--inject-…` arguments, verbatim.
    inject: Vec<String>,
}

fn parse_args(args: &[String]) -> Result<Opts, String> {
    let mode = match args.first().map(String::as_str) {
        Some("check") => Mode::Check,
        Some("run") => Mode::Run,
        Some(o) => return Err(format!("unknown nightly command '{o}'")),
        None => return Err("nightly needs a command: check or run".into()),
    };
    let mut o = Opts {
        mode,
        private_dir: None,
        windows: None,
        now: None,
        guard: None,
        inject: Vec::new(),
    };
    let mut it = args[1..].iter();
    let once = |slot: bool, flag: &str| {
        if slot {
            Err(format!("{flag} is given twice"))
        } else {
            Ok(())
        }
    };
    while let Some(a) = it.next() {
        let mut value = |n: &str| it.next().cloned().ok_or_else(|| format!("{a} needs {n}"));
        match a.as_str() {
            "--private-dir" => {
                once(o.private_dir.is_some(), a)?;
                o.private_dir = Some(PathBuf::from(value("a directory")?));
            }
            "--guard" => {
                once(o.guard.is_some(), a)?;
                o.guard = Some(PathBuf::from(value("an executable")?));
            }
            "--windows" => {
                once(o.windows.is_some(), a)?;
                o.windows = Some(PathBuf::from(value("a file")?));
            }
            "--now" => {
                once(o.now.is_some(), a)?;
                o.now = Some(utc::parse_rfc3339(&value("a date-time")?)?);
            }
            "--inject-available-physical" | "--inject-volume-available" => {
                let v = value("a byte quantity")?;
                o.inject.extend([a.clone(), v]);
            }
            "--inject-dir-size" => {
                let p = value("a path and a byte quantity")?;
                let v = value("a byte quantity after the path")?;
                o.inject.extend([a.clone(), p, v]);
            }
            other => return Err(format!("unknown argument '{other}'")),
        }
    }
    if mode == Mode::Run && (o.windows.is_some() || o.now.is_some() || !o.inject.is_empty()) {
        return Err(
            "--windows, --now and --inject-* are for nightly check: a real run uses the agreed calendar, the clock and real readings ([MP §8.2])"
                .into(),
        );
    }
    Ok(o)
}

/// The state of the working tree the run tests.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Tree {
    pub commit: Option<String>,
    pub branch: Option<String>,
    /// Changed paths (`git status --porcelain`), untracked ones included when read with `untracked`.
    pub changes: usize,
}

impl Tree {
    /// Reads the tree: the pre-checks count untracked paths too; the check after each job counts tracked changes
    /// only, since a job may leave an untracked file (a failing proptest's regression file) without changing what it
    /// tested.
    fn read(repo: &Path, untracked: bool) -> Result<Tree, String> {
        let u = if untracked {
            "--untracked-files=all"
        } else {
            "--untracked-files=no"
        };
        let status = git::run(repo, &["status", "--porcelain", u])?;
        Ok(Tree {
            commit: git::probe(repo, &["rev-parse", "HEAD"]),
            branch: git::probe(repo, &["rev-parse", "--abbrev-ref", "HEAD"]),
            changes: status.lines().filter(|l| !l.trim().is_empty()).count(),
        })
    }
}

/// Everything the runner resolved before its checks.
#[derive(Clone, Debug)]
pub struct Env {
    pub repo: PathBuf,
    /// `/private/` of the main worktree.
    pub private: PathBuf,
    /// The target root (PLAN §2.1).
    pub root: PathBuf,
    /// The lane target directories (`<root>/<lane dir>`), in `roles.toml` order.
    pub lane_dirs: Vec<PathBuf>,
    /// The build lane's target directory.
    pub lane_dir: PathBuf,
    /// The calendar file.
    pub windows: PathBuf,
    pub has_taskkill: bool,
    pub tree: Tree,
    /// Rebuild the private manifest at the end of a run (the real runner always does).
    pub rebuild_manifest: bool,
    /// The revision whose tracked text the rebuilt manifest treats as public (`master`; `xtask private index`).
    pub public_ref: Option<String>,
}

/// One refusal of the pre-checks.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Refusal {
    pub kind: String,
    pub detail: String,
}

fn refusal(kind: &str, detail: impl Into<String>) -> Refusal {
    Refusal {
        kind: kind.to_string(),
        detail: detail.into(),
    }
}

/// What the pre-checks found.
#[derive(Debug)]
pub struct PreCheck {
    pub now: i64,
    pub window: Option<Window>,
    /// The window's end less the finish margin: every job ends by then.
    pub deadline: i64,
    pub guard: Option<guard::Verdict>,
    pub guard_exe: Option<PathBuf>,
    pub refusals: Vec<Refusal>,
    /// `/private/nightly/run.lock`, held locked when no check refused, so the run that follows keeps it.
    pub lock: Option<File>,
}

/// The four counted directories of PLAN §2.1 under the target root, with their caps from `[guard.caps]`.
fn counted(env: &Env, cfg: &Config) -> Vec<(PathBuf, u64)> {
    cfg.caps
        .iter()
        .map(|(name, cap)| (env.root.join(name), *cap))
        .collect()
}

/// `check`'s injected readings as the guard takes them: an `--inject-dir-size` may name a counted directory by its
/// name under the target root (`laneA`, `fuzz`) or by any spelling of its path (`D:/moirai-target/laneA`), and is
/// rewritten to the path the runner passes to `--dir`, which the guard matches byte for byte ([MP §8.2]). A value
/// that names no counted directory is passed on unchanged, and the guard refuses it as a usage error.
// spec: [MP §8.2] (an injected directory size names a --dir path byte for byte; injections for check only),
// [PLAN §3.2 WP-05] (the refusals are tested with injected values)
fn guard_injections(env: &Env, cfg: &Config, inject: &[String]) -> Vec<String> {
    let dirs = counted(env, cfg);
    let mut out = Vec::with_capacity(inject.len());
    let mut i = 0;
    while i < inject.len() {
        out.push(inject[i].clone());
        if inject[i] == "--inject-dir-size" && i + 1 < inject.len() {
            let given = &inject[i + 1];
            let named = cfg
                .caps
                .iter()
                .zip(&dirs)
                .find(|((name, _), (path, _))| name == given || path.as_path() == Path::new(given))
                .map(|(_, (path, _))| path.to_string_lossy().into_owned());
            out.push(named.unwrap_or_else(|| given.clone()));
            i += 1;
        }
        i += 1;
    }
    out
}

/// The cargo build-directory locks held under a lane target directory: `.cargo-lock` one and two levels down
/// (`<profile>/.cargo-lock`, `<triple>/<profile>/.cargo-lock`).
// spec: [PLAN §3.2 WP-05] (the lane directory locks), [PLAN §2.1] (cargo's build-directory lock is the lane's build
// semaphore), [PLAN §5 V4] (agent-free windows)
fn held_locks(lane: &Path) -> Vec<PathBuf> {
    let subdirs = |d: &Path| -> Vec<PathBuf> {
        std::fs::read_dir(d)
            .map(|r| {
                r.filter_map(Result::ok)
                    .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                    .map(|e| e.path())
                    .collect()
            })
            .unwrap_or_default()
    };
    let mut out = Vec::new();
    for d1 in subdirs(lane) {
        for d in std::iter::once(d1.clone()).chain(subdirs(&d1)) {
            let p = d.join(".cargo-lock");
            if let Ok(f) = File::open(&p)
                && matches!(f.try_lock(), Err(TryLockError::WouldBlock))
            {
                out.push(p);
            }
        }
    }
    out
}

/// Opens and locks `/private/nightly/run.lock`: `None` when another run holds it.
fn run_lock(private: &Path) -> Result<Option<File>, String> {
    let dir = private.join(NIGHTLY_DIR);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let p = dir.join(RUN_LOCK);
    let f = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&p)
        .map_err(|e| format!("{}: {e}", p.display()))?;
    match f.try_lock() {
        Ok(()) => Ok(Some(f)),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(e)) => Err(format!("{}: {e}", p.display())),
    }
}

/// The caps of a window's kind.
fn kind_caps(cfg: &Config, kind: WindowKind) -> KindCaps {
    match kind {
        WindowKind::AgentFree => cfg.agent_free,
        WindowKind::BesideAgents => cfg.beside_agents,
    }
}

fn jobs_ctx(env: &Env, cfg: &Config, w: &Window, turn: u64) -> jobs::Ctx {
    jobs::Ctx {
        repo: env.repo.clone(),
        root: env.root.clone(),
        lane_dir: env.lane_dir.clone(),
        caps: kind_caps(cfg, w.kind),
        turn,
    }
}

/// Whether a job kind rotates (the fuzz targets, the mutants shard) and so has a turn.
fn rotates(kind: &JobKind) -> bool {
    matches!(kind, JobKind::Mutants { .. } | JobKind::Fuzz { .. })
}

/// A rotating job's turn (`jobs` module header): one more than the turn the latest earlier run recorded for `job`
/// (a run that skipped the job records none), or 0. Every run directory under `nightly` counts, sealed, aborted or
/// partial, except the run `own` (its start stamp). The turn does not depend on calendar days, so two windows on one
/// day take two shards and a day without a window skips none.
// spec: [PLAN §3.2 WP-05] (fuzz ≤ 2 targets, GT16 sample: each in rotation), [60 §3.13] GT16
fn turn_of(nightly: &Path, own: &str, job: &str) -> u64 {
    let Ok(rd) = std::fs::read_dir(nightly) else {
        return 0;
    };
    let mut runs: Vec<(String, PathBuf)> = rd
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let stem = run_stem(&name)?.to_string();
            (stem != own).then(|| (stem, e.path()))
        })
        .collect();
    runs.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, dir) in runs {
        let Ok(text) = std::fs::read_to_string(dir.join(RECORD)) else {
            continue;
        };
        let Ok(v) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        let turn = v["jobs"]
            .as_array()
            .and_then(|a| a.iter().find(|j| j["name"] == job))
            .and_then(|j| j["turn"].as_u64());
        if let Some(t) = turn {
            return t.saturating_add(1);
        }
    }
    0
}

/// The guard binary: `--guard`, or built into the build lane and copied to `<target root>/nightly/` ([`GUARD_DIR`]),
/// where no cargo build replaces it and no `cargo clean` of a lane removes it while the run uses it.
fn guard_exe<H: Host + ?Sized>(
    host: &mut H,
    env: &Env,
    cfg: &Config,
    given: Option<&Path>,
) -> Result<PathBuf, String> {
    if let Some(g) = given {
        return Ok(g.to_path_buf());
    }
    let ctx = jobs::Ctx {
        repo: env.repo.clone(),
        root: env.root.clone(),
        lane_dir: env.lane_dir.clone(),
        caps: cfg.beside_agents,
        turn: 0,
    };
    let c = host.capture(&jobs::guard_build(&ctx), GUARD_BUILD_SECS)?;
    if c.exit != Some(0) {
        let tail: Vec<&str> = c.stderr.lines().rev().take(5).collect();
        return Err(format!(
            "building the guard failed (exit {}): {}",
            c.exit.map_or("none".into(), |x| x.to_string()),
            tail.into_iter().rev().collect::<Vec<_>>().join(" | ")
        ));
    }
    let built = jobs::guard_built(&ctx);
    let dir = env.root.join(GUARD_DIR);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let copy = dir.join(built.file_name().unwrap_or_default());
    std::fs::copy(&built, &copy)
        .map_err(|e| format!("{} -> {}: {e}", built.display(), copy.display()))?;
    Ok(copy)
}

/// The pre-checks (module header): every refusal they find, the window and its deadline, and the guard's verdict.
// spec: [PLAN §3.2 WP-05] (pre-checks through probes guard; the lane directory locks; the window calendar),
// [PLAN §5 V4], [60 §3.15], [MP §8]
pub fn pre_check<H: Host + ?Sized>(
    host: &mut H,
    env: &Env,
    cfg: &Config,
    now: i64,
    guard_given: Option<&Path>,
    inject: &[String],
) -> PreCheck {
    let mut refusals = Vec::new();
    if !env.has_taskkill {
        refusals.push(refusal(
            "host",
            "the nightly runner runs on the Windows laptop of profile L: %SystemRoot%\\System32\\taskkill.exe, which stops a job's process tree, was not found",
        ));
    }
    let mut window = None;
    match std::fs::read_to_string(&env.windows) {
        Err(e) => refusals.push(refusal(
            "calendar",
            format!(
                "no agreed window: {} cannot be read ({e}); the owner keeps the calendar there (PLAN §5 V4, docs/m0/nightly.md §2)",
                env.windows.display()
            ),
        )),
        Ok(text) => match calendar::parse(&text) {
            Err(e) => refusals.push(refusal("calendar", e)),
            Ok(ws) => match calendar::current(&ws, now) {
                None => refusals.push(refusal(
                    "outside-window",
                    format!(
                        "{} is outside every agreed window; {}",
                        utc::rfc3339(now),
                        calendar::next(&ws, now).map_or("no later window is agreed".into(), |w| format!(
                            "the next is {}",
                            w.describe()
                        ))
                    ),
                )),
                Some(w) if w.end - now <= cfg.finish_margin_secs => {
                    window = Some(w);
                    refusals.push(refusal(
                        "window-ending",
                        format!(
                            "the window {} ends in {}, within the finish margin of {}",
                            w.describe(),
                            utc::hm(w.end - now),
                            utc::hm(cfg.finish_margin_secs)
                        ),
                    ));
                }
                Some(w) => window = Some(w),
            },
        },
    }
    let mut lock = None;
    match run_lock(&env.private) {
        Ok(Some(f)) => lock = Some(f),
        Ok(None) => refusals.push(refusal(
            "run-locked",
            format!(
                "another nightly run holds {}",
                env.private.join(NIGHTLY_DIR).join(RUN_LOCK).display()
            ),
        )),
        Err(e) => refusals.push(refusal(
            "run-locked",
            format!("the run lock cannot be taken: {e}"),
        )),
    }
    if env.tree.changes > 0 {
        refusals.push(refusal(
            "dirty-tree",
            format!(
                "the working tree has {} changed or untracked paths; the results must name the commit they tested",
                env.tree.changes
            ),
        ));
    }
    if env.tree.commit.is_none() {
        refusals.push(refusal("dirty-tree", "HEAD names no commit"));
    }
    if window.is_some_and(|w| w.kind == WindowKind::AgentFree) {
        for lane in &env.lane_dirs {
            for p in held_locks(lane) {
                refusals.push(refusal(
                    "lane-busy",
                    format!(
                        "a cargo build holds {}: an agent-free window needs idle lanes",
                        p.display()
                    ),
                ));
            }
        }
    }
    let mut verdict = None;
    let mut guard_path = None;
    if refusals.is_empty() || guard_given.is_some() {
        match guard_exe(host, env, cfg, guard_given) {
            Err(e) => refusals.push(refusal("guard", e)),
            Ok(exe) => {
                let c = Cmd::new(exe.to_string_lossy(), &env.repo).args(guard::pre_check_args(
                    &env.root,
                    &counted(env, cfg),
                    cfg.ram_floor,
                    cfg.disk_floor,
                    &guard_injections(env, cfg, inject),
                ));
                match host
                    .capture(&c, guard::TIMEOUT_SECS)
                    .and_then(|o| guard::parse(o.exit, &o.stdout, &o.stderr))
                {
                    Err(e) => refusals.push(refusal("guard", e)),
                    Ok(v) => {
                        refusals.extend(
                            v.refusals
                                .iter()
                                .map(|r| refusal(&r.kind, r.detail.clone())),
                        );
                        verdict = Some(v);
                    }
                }
                guard_path = Some(exe);
            }
        }
    }
    // A refused pre-check releases the lock; one that passed keeps it for the run (`check` releases it at exit).
    let lock = if refusals.is_empty() { lock } else { None };
    PreCheck {
        now,
        deadline: window.map_or(now, |w| w.end - cfg.finish_margin_secs),
        window,
        guard: verdict,
        guard_exe: guard_path,
        refusals,
        lock,
    }
}

/// The plan line of one job: what it would run, or why it is skipped.
fn plan_line(env: &Env, cfg: &Config, w: &Window, job: &Job) -> String {
    let caps = kind_caps(cfg, w.kind);
    if job.ram_budget > caps.ram_budget {
        return format!(
            "skipped: its RAM budget {} B exceeds the window's {} B",
            job.ram_budget, caps.ram_budget
        );
    }
    let turn = if rotates(&job.kind) {
        turn_of(&env.private.join(NIGHTLY_DIR), "", &job.name)
    } else {
        0
    };
    let ctx = jobs_ctx(env, cfg, w, turn);
    match &job.kind {
        JobKind::CargoTest { packages, tier, args } => jobs::cargo_test(&ctx, packages, *tier, args).line(),
        JobKind::Mutants { packages, shards, tier } => format!(
            "turn {turn}: {}",
            jobs::mutants(&ctx, packages, *shards, *tier, Path::new("<run>")).line()
        ),
        JobKind::Fuzz { max_targets, .. } => match jobs::fuzz_targets(&env.repo) {
            Err(e) => format!("error: {e}"),
            Ok(t) if t.is_empty() => {
                "skipped: fuzz/Cargo.toml declares no fuzz target yet (FL-1's WP-65 and WP-67 add them)".into()
            }
            Ok(t) => format!(
                "turn {turn}: cargo fuzz run -s none {}",
                jobs::rotate(&t, *max_targets, turn).join(", ")
            ),
        },
    }
}

/// A job's status in the run record.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Status {
    Passed,
    Failed,
    TimedOut,
    Stopped,
    Error,
    Skipped,
    NotRun,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Passed => "passed",
            Status::Failed => "failed",
            Status::TimedOut => "timed-out",
            Status::Stopped => "stopped",
            Status::Error => "error",
            Status::Skipped => "skipped",
            Status::NotRun => "not-run",
        }
    }

    /// Whether the status fails the night.
    pub fn fails(self) -> bool {
        matches!(
            self,
            Status::Failed | Status::TimedOut | Status::Stopped | Status::Error
        )
    }

    fn of(e: &Ended, ok: impl Fn(i32) -> bool) -> (Status, Option<i32>, Option<String>) {
        match e {
            Ended::Exited(c) if ok(*c) => (Status::Passed, Some(*c), None),
            Ended::Exited(c) => (Status::Failed, Some(*c), Some(format!("exit code {c}"))),
            Ended::TimedOut => (
                Status::TimedOut,
                None,
                Some("stopped at its time limit".into()),
            ),
            Ended::Stopped(w) => (
                Status::Stopped,
                None,
                Some(format!("the RAM watchdog stopped it: {w}")),
            ),
            Ended::Error(e) => (Status::Error, None, Some(e.clone())),
        }
    }
}

/// How many jobs of a run ended each way (`run.json` `counts`).
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
struct Counts {
    passed: usize,
    /// Failed, timed out, stopped or had an error ([`Status::fails`]).
    failed: usize,
    skipped: usize,
    not_run: usize,
}

impl Counts {
    fn add(&mut self, s: Status) {
        match s {
            Status::Passed => self.passed += 1,
            Status::Skipped => self.skipped += 1,
            Status::NotRun => self.not_run += 1,
            Status::Failed | Status::TimedOut | Status::Stopped | Status::Error => self.failed += 1,
        }
    }

    fn json(&self) -> Value {
        json!({
            "passed": self.passed,
            "failed": self.failed,
            "skipped": self.skipped,
            "not_run": self.not_run,
        })
    }
}

/// A run's verdict (module header; `docs/m0/nightly.md` §6).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum RunVerdict {
    /// Every job ran and passed.
    Pass,
    /// No job failed, but some were skipped: a run that skips a job every night is visible.
    PassPartial,
    /// Every job was skipped: nothing was tested.
    NoJobRan,
    /// A job failed, timed out, was stopped or had an error, or the tested tree changed.
    Fail,
}

impl RunVerdict {
    // spec: [PLAN §3.2 WP-05] (the job list; E10), [60 §3.15]
    fn of(c: &Counts, tree_changed: bool) -> RunVerdict {
        if c.failed > 0 || tree_changed {
            RunVerdict::Fail
        } else if c.passed == 0 {
            RunVerdict::NoJobRan
        } else if c.skipped > 0 {
            RunVerdict::PassPartial
        } else {
            RunVerdict::Pass
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            RunVerdict::Pass => "pass",
            RunVerdict::PassPartial => "pass-partial",
            RunVerdict::NoJobRan => "no-job-ran",
            RunVerdict::Fail => "fail",
        }
    }

    fn exit(self) -> u8 {
        match self {
            RunVerdict::Pass | RunVerdict::PassPartial => EXIT_PASS,
            RunVerdict::NoJobRan => EXIT_NO_JOB,
            RunVerdict::Fail => EXIT_FAIL,
        }
    }
}

/// One job's outcome and its record entry.
struct JobOutcome {
    status: Status,
    json: Value,
}

fn job_json(job: &Job, status: Status, reason: Option<String>) -> Value {
    json!({
        "name": job.name,
        "kind": job.kind.label(),
        "title": job.title,
        "status": status.as_str(),
        "reason": reason,
        "ram_budget": job.ram_budget,
    })
}

fn skipped(job: &Job, why: String) -> JobOutcome {
    JobOutcome {
        status: Status::Skipped,
        json: job_json(job, Status::Skipped, Some(why)),
    }
}

/// Runs one job (module header; `docs/m0/nightly.md` §5); `turn` rotates a mutants or fuzz job.
// spec: [PLAN §3.2 WP-05] (the job list: GT1 full, GT18 long, fuzz ≤ 2 targets sanitizer-off -rss_limit_mb=256,
// GT16 sample; gate jobs beside agents ≤ 1 GB), [60 §3.15]
#[allow(clippy::too_many_arguments)]
fn run_job<H: Host + ?Sized>(
    host: &mut H,
    env: &Env,
    cfg: &Config,
    w: &Window,
    job: &Job,
    deadline: i64,
    dir: &Path,
    turn: u64,
    watch: &mut Watch,
) -> JobOutcome {
    let caps = kind_caps(cfg, w.kind);
    if job.ram_budget > caps.ram_budget {
        return skipped(
            job,
            format!(
                "its RAM budget of {} bytes exceeds the {} window's {} bytes",
                job.ram_budget,
                w.kind.as_str(),
                caps.ram_budget
            ),
        );
    }
    let start = host.now();
    if deadline - start < job.min_secs {
        return skipped(
            job,
            format!(
                "{} are left before the deadline; the job needs at least {}",
                utc::hm(deadline - start),
                utc::hm(job.min_secs)
            ),
        );
    }
    let job_deadline = if job.max_secs == 0 {
        deadline
    } else {
        deadline.min(start + job.max_secs)
    };
    watch.min_available = None;
    if let Err(why) = watch.check(host) {
        let mut j = job_json(
            job,
            Status::Stopped,
            Some(format!("the RAM check before the job refused: {why}")),
        );
        j["started"] = json!(utc::rfc3339(start));
        return JobOutcome {
            status: Status::Stopped,
            json: j,
        };
    }
    let ctx = jobs_ctx(env, cfg, w, turn);
    let log = dir.join(format!("{}.log", job.name));
    let mut commands = Vec::new();
    let mut extra = serde_json::Map::new();
    let (status, exit, reason) = match &job.kind {
        JobKind::CargoTest {
            packages,
            tier,
            args,
        } => {
            let c = jobs::cargo_test(&ctx, packages, *tier, args);
            commands.push(c.line());
            extra.insert("tier".into(), json!(tier.as_str()));
            Status::of(&exec::run_cmd(host, &c, &log, job_deadline, watch), |x| {
                x == 0
            })
        }
        JobKind::Mutants {
            packages,
            shards,
            tier,
        } => {
            let out = dir.join(&job.name);
            // The scratch copies a stopped mutants job left: removed in agent-free windows, where the run lock and
            // the window rule out another user of the directory.
            if w.kind == WindowKind::AgentFree {
                extra.insert(
                    "scratch_removed".into(),
                    match jobs::clear_scratch(&ctx.mutants_dir()) {
                        Ok(names) => json!(names),
                        Err(e) => json!({ "error": e }),
                    },
                );
            }
            let made = std::fs::create_dir_all(&out)
                .and_then(|()| std::fs::create_dir_all(ctx.mutants_dir()));
            let c = jobs::mutants(&ctx, packages, *shards, *tier, &out);
            commands.push(c.line());
            extra.insert("tier".into(), json!(tier.as_str()));
            extra.insert(
                "shard".into(),
                json!(format!("{}/{shards}", jobs::shard(turn, *shards))),
            );
            let r = match made {
                Err(e) => (Status::Error, None, Some(format!("{}: {e}", out.display()))),
                Ok(()) => Status::of(
                    &exec::run_cmd(host, &c, &log, job_deadline, watch),
                    jobs::mutants_passed,
                ),
            };
            extra.insert(
                "mutants".into(),
                jobs::mutants_counts(&out).unwrap_or_else(|e| json!({ "error": e })),
            );
            r
        }
        JobKind::Fuzz {
            max_targets,
            rss_limit_mb,
            grace_secs,
        } => run_fuzz(
            host,
            &ctx,
            (*max_targets, *rss_limit_mb, *grace_secs),
            job_deadline,
            &log,
            watch,
            &mut commands,
            &mut extra,
        ),
    };
    if status == Status::Skipped && commands.is_empty() {
        return skipped(job, reason.unwrap_or_default());
    }
    // A job that started takes its turn; one skipped after its builds (no fuzzing time left) keeps it for the next
    // run.
    if rotates(&job.kind) && status != Status::Skipped {
        extra.insert("turn".into(), json!(turn));
    }
    let capped = if log.exists() {
        exec::cap_log(&log, cfg.log_cap).map_err(|e| e.to_string())
    } else {
        Ok(false)
    };
    let end = host.now();
    let mut j = job_json(job, status, reason);
    let o = j.as_object_mut().expect("a job entry is an object");
    o.insert("started".into(), json!(utc::rfc3339(start)));
    o.insert("ended".into(), json!(utc::rfc3339(end)));
    o.insert("elapsed_secs".into(), json!(end - start));
    o.insert("deadline".into(), json!(utc::rfc3339(job_deadline)));
    o.insert("exit_code".into(), json!(exit));
    o.insert("commands".into(), json!(commands));
    o.insert(
        "log".into(),
        json!(log.exists().then(|| format!("{}.log", job.name))),
    );
    o.insert(
        "log_capped".into(),
        match capped {
            Ok(c) => json!(c),
            Err(e) => json!({ "error": e }),
        },
    );
    o.insert("min_available".into(), json!(watch.min_available));
    o.extend(extra);
    JobOutcome { status, json: j }
}

/// The fuzz job: at most `max_targets` targets in rotation, each built and then fuzzed for an equal share of the time
/// left before the deadline less `grace_secs`, so the last target's libFuzzer stops by its own `-max_total_time`, its
/// final statistics printed, rather than being stopped at the deadline. A crash or out-of-memory input (a non-zero
/// exit, a new file in `fuzz/artifacts/<target>/`) fails it; a job in which no target was fuzzed or failed is skipped.
#[allow(clippy::too_many_arguments)]
// spec: [PLAN §3.2 WP-05] (fuzz ≤ 2 targets sanitizer-off -rss_limit_mb=256; E10: every target ends inside the
// window), docs/m0/tools.md §4.5 (crash-* and oom-* artifacts)
fn run_fuzz<H: Host + ?Sized>(
    host: &mut H,
    ctx: &jobs::Ctx,
    (max_targets, rss_limit_mb, grace_secs): (u32, u32, i64),
    deadline: i64,
    log: &Path,
    watch: &mut Watch,
    commands: &mut Vec<String>,
    extra: &mut serde_json::Map<String, Value>,
) -> (Status, Option<i32>, Option<String>) {
    let targets = match jobs::fuzz_targets(&ctx.repo) {
        Ok(t) => t,
        Err(e) => return (Status::Error, None, Some(e)),
    };
    if targets.is_empty() {
        return (
            Status::Skipped,
            None,
            Some(
                "fuzz/Cargo.toml declares no fuzz target yet (FL-1's WP-65 and WP-67 add them)"
                    .into(),
            ),
        );
    }
    let channel = match jobs::fuzz_channel(&ctx.repo) {
        Ok(c) => c,
        Err(e) => return (Status::Error, None, Some(e)),
    };
    let chosen = jobs::rotate(&targets, max_targets, ctx.turn);
    let mut results = Vec::new();
    let mut tried = false;
    // The job reports its worst target: a watchdog stop (which stops the run) over any failure, a failure over a pass.
    let rank = |s: Status| match s {
        Status::Stopped => 2,
        s if s.fails() => 1,
        _ => 0,
    };
    let mut worst = (Status::Passed, None, None);
    for (i, t) in chosen.iter().enumerate() {
        let b = jobs::fuzz_build(ctx, &channel, t);
        commands.push(b.line());
        let (bs, bx, br) = Status::of(&exec::run_cmd(host, &b, log, deadline, watch), |x| x == 0);
        let mut entry = json!({ "target": t });
        if bs != Status::Passed {
            tried = true;
            entry["status"] = json!(bs.as_str());
            entry["reason"] = json!(format!("the build: {}", br.clone().unwrap_or_default()));
            results.push(entry);
            if rank(bs) > rank(worst.0) {
                worst = (
                    bs,
                    bx,
                    Some(format!("{t}: the build: {}", br.unwrap_or_default())),
                );
            }
            if matches!(bs, Status::TimedOut | Status::Stopped) {
                break;
            }
            continue;
        }
        let left = (chosen.len() - i) as i64;
        let secs = (deadline - grace_secs - host.now()) / left;
        if secs < 60 {
            entry["status"] = json!(Status::Skipped.as_str());
            entry["reason"] = json!("less than a minute of fuzzing time was left");
            results.push(entry);
            continue;
        }
        tried = true;
        let before = jobs::artifacts(&ctx.repo, t);
        let r = jobs::fuzz_run(ctx, &channel, t, secs, rss_limit_mb);
        commands.push(r.line());
        let ended = exec::run_cmd(host, &r, log, deadline, watch);
        let new: Vec<String> = jobs::artifacts(&ctx.repo, t)
            .difference(&before)
            .cloned()
            .collect();
        let (mut s, x, mut why) = Status::of(&ended, |x| x == 0);
        if s == Status::Passed && !new.is_empty() {
            s = Status::Failed;
            why = Some("new artifacts were written".into());
        }
        entry["status"] = json!(s.as_str());
        entry["exit_code"] = json!(x);
        entry["seconds"] = json!(secs);
        entry["new_artifacts"] = json!(new);
        entry["reason"] = json!(why);
        results.push(entry);
        if rank(s) > rank(worst.0) {
            worst = (s, x, Some(format!("{t}: {}", why.unwrap_or_default())));
        }
        if matches!(s, Status::Stopped) {
            break;
        }
    }
    extra.insert("rss_limit_mb".into(), json!(rss_limit_mb));
    extra.insert("grace_secs".into(), json!(grace_secs));
    extra.insert("targets".into(), json!(results));
    if !tried {
        return (
            Status::Skipped,
            None,
            Some("no target had a minute of fuzzing time left after its build".into()),
        );
    }
    worst
}

/// The run record (`docs/m0/nightly.md` §6), rewritten after every job.
struct Record {
    path: PathBuf,
    value: Value,
}

impl Record {
    fn save(&self) -> Result<(), String> {
        let tmp = self.path.with_extension("json.tmp");
        let text = serde_json::to_string_pretty(&self.value).map_err(|e| e.to_string())?;
        std::fs::write(&tmp, text + "\n").map_err(|e| format!("{}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, &self.path).map_err(|e| format!("{}: {e}", self.path.display()))
    }
}

/// Renames the directory of every run that ended without sealing it (`<start>.partial`, other than the run `own`)
/// to `<start>.aborted`. The run calls it in its finish, just before it rebuilds the private manifest: done at its
/// start, the rename would leave the manifest stale, and every commit refused, for the whole run.
fn seal_aborted(nightly: &Path, own: &str) -> Result<Vec<String>, String> {
    let mut sealed = Vec::new();
    for e in std::fs::read_dir(nightly).map_err(|e| format!("{}: {e}", nightly.display()))? {
        let e = e.map_err(|e| e.to_string())?;
        let name = e.file_name().to_string_lossy().into_owned();
        if let Some(stem) = name.strip_suffix(PARTIAL)
            && stem != own
            && e.file_type().is_ok_and(|t| t.is_dir())
        {
            let to = nightly.join(format!("{stem}{ABORTED}"));
            std::fs::rename(e.path(), &to).map_err(|x| format!("{}: {x}", e.path().display()))?;
            sealed.push(name);
        }
    }
    Ok(sealed)
}

/// Removes the oldest run directories beyond `keep` (0 keeps all); returns the names removed.
fn prune(nightly: &Path, keep: u32) -> Result<Vec<String>, String> {
    if keep == 0 {
        return Ok(Vec::new());
    }
    let mut runs: Vec<String> = std::fs::read_dir(nightly)
        .map_err(|e| format!("{}: {e}", nightly.display()))?
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| utc::is_compact(n.strip_suffix(ABORTED).unwrap_or(n)))
        .collect();
    runs.sort();
    let excess = runs.len().saturating_sub(keep as usize);
    let mut removed = Vec::new();
    for n in runs.into_iter().take(excess) {
        std::fs::remove_dir_all(nightly.join(&n)).map_err(|e| format!("{n}: {e}"))?;
        removed.push(n);
    }
    Ok(removed)
}

/// Whether the tested tree moved while a job ran: `HEAD` or the branch changed, a tracked file changed, or the tree
/// could not be read; `None` when it is the commit the pre-checks found, clean. Untracked files are not counted
/// ([`Tree::read`]).
// spec: [PLAN §3.2 WP-05] (raw results to /private/nightly/, which name the commit they tested)
fn tree_change<H: Host + ?Sized>(host: &mut H, env: &Env, job: &str) -> Option<Value> {
    match host.tree(&env.repo) {
        Ok(t) if t.commit == env.tree.commit && t.branch == env.tree.branch && t.changes == 0 => {
            None
        }
        Ok(t) => Some(json!({
            "after": job,
            "commit": t.commit,
            "branch": t.branch,
            "tracked_changes": t.changes,
        })),
        Err(e) => Some(json!({ "after": job, "error": e })),
    }
}

/// The run (module header), after the pre-checks passed and with the run lock they took: returns the exit code.
// spec: [PLAN §3.2 WP-05] (the job list; raw results to /private/nightly/; E10: a nightly run completes inside an
// agreed window), [60 §3.15]
pub fn run<H: Host + ?Sized>(
    host: &mut H,
    env: &Env,
    cfg: &Config,
    pre: &PreCheck,
    out: &mut dyn std::io::Write,
) -> Result<u8, String> {
    let (Some(w), Some(guard_exe), Some(_)) =
        (pre.window, pre.guard_exe.as_ref(), pre.lock.as_ref())
    else {
        return Err(
            "the run needs a window, a guard and the run lock from pre-checks that passed".into(),
        );
    };
    let nightly = env.private.join(NIGHTLY_DIR);
    let stamp = utc::compact(pre.now);
    let dir = nightly.join(format!("{stamp}{PARTIAL}"));
    std::fs::create_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let caps = kind_caps(cfg, w.kind);
    let mut rec = Record {
        path: dir.join(RECORD),
        value: json!({
            "schema": RECORD_SCHEMA,
            "started": utc::rfc3339(pre.now),
            "ended": null,
            "window": { "start": utc::rfc3339(w.start), "end": utc::rfc3339(w.end), "kind": w.kind.as_str() },
            "deadline": utc::rfc3339(pre.deadline),
            "commit": env.tree.commit,
            "branch": env.tree.branch,
            "lane": env.lane_dir.file_name().map(|n| n.to_string_lossy().into_owned()),
            "caps": { "build_jobs": caps.build_jobs, "test_threads": caps.test_threads, "parallel": caps.parallel, "ram_budget": caps.ram_budget },
            "guard": pre.guard.as_ref().map(|g| g.json.clone()),
            "jobs": [],
            "tree_changed": null,
            "counts": null,
            "aborted_runs_sealed": null,
            "verdict": null,
        }),
    };
    rec.save()?;
    let mut watch = Watch {
        guard: Cmd::new(guard_exe.to_string_lossy(), &env.repo)
            .args(guard::watch_args(&env.root, cfg.ram_floor)),
        every_secs: cfg.watch_secs as i64,
        min_available: None,
    };
    let mut stopped: Option<String> = None;
    let mut counts = Counts::default();
    let mut tree_changed = false;
    for job in &cfg.jobs {
        let o = match &stopped {
            Some(why) => JobOutcome {
                status: Status::NotRun,
                json: job_json(
                    job,
                    Status::NotRun,
                    Some(format!("the run was stopped: {why}")),
                ),
            },
            None => {
                let _ = writeln!(
                    out,
                    "nightly: {} ({}) starts at {}",
                    job.name,
                    job.kind.label(),
                    utc::rfc3339(host.now())
                );
                let turn = if rotates(&job.kind) {
                    turn_of(&nightly, &stamp, &job.name)
                } else {
                    0
                };
                run_job(
                    host,
                    env,
                    cfg,
                    &w,
                    job,
                    pre.deadline,
                    &dir,
                    turn,
                    &mut watch,
                )
            }
        };
        let reason = o.json["reason"]
            .as_str()
            .map(|r| format!(": {r}"))
            .unwrap_or_default();
        let _ = writeln!(out, "nightly: {} {}{reason}", job.name, o.status.as_str());
        if o.status == Status::Stopped && stopped.is_none() {
            stopped = Some(o.json["reason"].as_str().unwrap_or("stopped").to_string());
        }
        counts.add(o.status);
        let ran = !matches!(o.status, Status::Skipped | Status::NotRun);
        if let Some(a) = rec.value["jobs"].as_array_mut() {
            a.push(o.json);
        }
        if ran
            && !tree_changed
            && let Some(change) = tree_change(host, env, &job.name)
        {
            let _ = writeln!(
                out,
                "nightly: the tested tree changed during {}: {change}; the run stops",
                job.name
            );
            rec.value["tree_changed"] = change;
            tree_changed = true;
            stopped.get_or_insert_with(|| format!("the tested tree changed during {}", job.name));
        }
        rec.save()?;
    }
    let ended = host.now();
    let verdict = RunVerdict::of(&counts, tree_changed);
    rec.value["ended"] = json!(utc::rfc3339(ended));
    rec.value["counts"] = counts.json();
    rec.value["verdict"] = json!(verdict.as_str());
    // The finish (module header): each step is timed into finish.json, written once the manifest is rebuilt.
    let aborted = seal_aborted(&nightly, &stamp)?;
    rec.value["aborted_runs_sealed"] = json!(aborted);
    rec.save()?;
    let t_aborted = host.now();
    let sealed = nightly.join(&stamp);
    std::fs::rename(&dir, &sealed)
        .map_err(|e| format!("{} -> {}: {e}", dir.display(), sealed.display()))?;
    let t_sealed = host.now();
    let removed = prune(&nightly, cfg.keep_runs)?;
    if !removed.is_empty() {
        let _ = writeln!(
            out,
            "nightly: removed the oldest runs beyond keep-runs = {}: {}",
            cfg.keep_runs,
            removed.join(", ")
        );
    }
    let t_pruned = host.now();
    let manifest = if env.rebuild_manifest {
        match crate::private::rebuild(&env.repo, &env.private, env.public_ref.as_deref()) {
            Ok(_) => {
                let _ = writeln!(
                    out,
                    "nightly: rebuilt {}",
                    env.private.join(crate::private::MANIFEST).display()
                );
                json!("rebuilt")
            }
            Err(e) => {
                let _ = writeln!(
                    out,
                    "nightly: the private manifest could not be rebuilt ({e}); run `cargo xtask private index` before the next commit"
                );
                json!({ "error": e })
            }
        }
    } else {
        json!("not rebuilt")
    };
    let finished = host.now();
    let finish = json!({
        "schema": FINISH_SCHEMA,
        "seal_aborted_secs": t_aborted - ended,
        "seal_secs": t_sealed - t_aborted,
        "prune_secs": t_pruned - t_sealed,
        "manifest_secs": finished - t_pruned,
        "manifest": manifest,
        "finish_secs": finished - ended,
        "left_secs": w.end - finished,
        "inside_window": finished < w.end,
    });
    let fp = sealed.join(FINISH);
    let text = serde_json::to_string_pretty(&finish).map_err(|e| e.to_string())? + "\n";
    if let Err(e) = std::fs::write(&fp, text) {
        let _ = writeln!(out, "nightly: {}: {e}", fp.display());
    }
    if finished >= w.end {
        let _ = writeln!(
            out,
            "nightly: the run finished {} after its window ended (E10); finish-margin-minutes is too small",
            utc::hm(finished - w.end)
        );
    }
    let _ = writeln!(
        out,
        "nightly: {} ({} passed, {} failed, {} skipped, {} not run; finish {} s) in {}",
        verdict.as_str().to_uppercase(),
        counts.passed,
        counts.failed,
        counts.skipped,
        counts.not_run,
        finished - ended,
        sealed.display()
    );
    Ok(verdict.exit())
}

/// Resolves the environment: `/private/`, the target root and the lanes (as `xtask worktree` does), the calendar and
/// the tree.
fn resolve(
    repo: &Path,
    o: &Opts,
    cfg_lane: &str,
    lanes: &[(String, String)],
) -> Result<Env, String> {
    let main = git::main_worktree(repo).ok_or("cannot locate the main worktree")?;
    let private = o
        .private_dir
        .clone()
        .unwrap_or_else(|| main.join("private"));
    let root = git::probe(repo, &["config", "--get", "moirai.target-root"])
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::worktree::default_root(&main, "moirai-target"));
    let lane_dirs: Vec<PathBuf> = lanes.iter().map(|(_, d)| root.join(d)).collect();
    let lane_dir = lanes
        .iter()
        .find(|(l, _)| l == cfg_lane)
        .map(|(_, d)| root.join(d))
        .ok_or("the build lane is not in xtask/roles.toml")?;
    Ok(Env {
        repo: repo.to_path_buf(),
        windows: o
            .windows
            .clone()
            .unwrap_or_else(|| private.join(calendar::FILE)),
        private,
        root,
        lane_dirs,
        lane_dir,
        has_taskkill: exec::taskkill().is_some(),
        tree: Tree::read(repo, true)?,
        rebuild_manifest: true,
        public_ref: Some("master".into()),
    })
}

fn print_pre(out: &mut dyn std::io::Write, env: &Env, cfg: &Config, pre: &PreCheck) {
    if let Some(w) = pre.window {
        let _ = writeln!(
            out,
            "nightly: window {}; deadline {} ({} from now)",
            w.describe(),
            utc::rfc3339(pre.deadline),
            utc::hm(pre.deadline - pre.now)
        );
    }
    let _ = writeln!(
        out,
        "nightly: tree {} at {}; build lane {}; target root {}",
        env.tree.branch.as_deref().unwrap_or("?"),
        env.tree.commit.as_deref().unwrap_or("?"),
        env.lane_dir.display(),
        env.root.display()
    );
    if let Some(g) = &pre.guard {
        let _ = writeln!(out, "nightly: guard: {}", g.json);
    }
    for r in &pre.refusals {
        let _ = writeln!(out, "nightly: refused: {}: {}", r.kind, r.detail);
    }
    if let (Some(w), true) = (pre.window, pre.refusals.is_empty()) {
        let caps = kind_caps(cfg, w.kind);
        let _ = writeln!(
            out,
            "nightly: plan ({}: CARGO_BUILD_JOBS={}, RUST_TEST_THREADS={}, parallel {}, RAM budget {} B):",
            w.kind.as_str(),
            caps.build_jobs,
            caps.test_threads,
            caps.parallel,
            caps.ram_budget
        );
        for j in &cfg.jobs {
            let _ = writeln!(out, "  {:<8} {}", j.name, plan_line(env, cfg, &w, j));
        }
    }
}

/// `cargo xtask nightly …` ([`USAGE`]).
pub fn cli(args: &[String], repo: &Path) -> Result<ExitCode, String> {
    if matches!(args.first().map(String::as_str), Some("--help" | "help")) {
        println!("{USAGE}");
        return Ok(ExitCode::SUCCESS);
    }
    let o = match parse_args(args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("xtask nightly: {e}\n{USAGE}");
            return Ok(ExitCode::from(EXIT_USAGE));
        }
    };
    let mut stdout = std::io::stdout().lock();
    let loaded = crate::config::read_toml(&repo.join("xtask/roles.toml"))
        .and_then(|t| crate::config::Roles::from_table(&t))
        .and_then(|roles| {
            crate::config::read_toml(&repo.join(config::FILE))
                .and_then(|t| Config::from_table(&t, &roles.lanes))
                .map(|cfg| (roles, cfg))
        });
    let (roles, cfg) = match loaded {
        Ok(rc) => rc,
        Err(e) => {
            let _ = writeln!(stdout, "nightly: refused: config: {e}");
            return Ok(ExitCode::from(EXIT_REFUSED));
        }
    };
    let env = match resolve(repo, &o, &cfg.lane, &roles.lanes) {
        Ok(env) => env,
        Err(e) => {
            let _ = writeln!(stdout, "nightly: error: {e}");
            return Ok(ExitCode::from(EXIT_INTERNAL));
        }
    };
    let mut host = RealHost::new();
    let now = o.now.unwrap_or_else(|| host.now());
    let pre = pre_check(&mut host, &env, &cfg, now, o.guard.as_deref(), &o.inject);
    print_pre(&mut stdout, &env, &cfg, &pre);
    if !pre.refusals.is_empty() {
        return Ok(ExitCode::from(EXIT_REFUSED));
    }
    if o.mode == Mode::Check {
        let _ = writeln!(
            stdout,
            "nightly: check passed; `cargo xtask nightly run` would start now"
        );
        return Ok(ExitCode::from(EXIT_PASS));
    }
    match run(&mut host, &env, &cfg, &pre, &mut stdout) {
        Ok(code) => Ok(ExitCode::from(code)),
        Err(e) => {
            let _ = writeln!(
                stdout,
                "nightly: error: {e}; the run's directory in {} stays <start>{PARTIAL} until the next run seals it <start>{ABORTED}",
                env.private.join(NIGHTLY_DIR).display()
            );
            Ok(ExitCode::from(EXIT_INTERNAL))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testdir::TestDir;
    use exec::tests::{FakeHost, fuzzes, guard_pass, guard_refuse, hangs, runs, test_tree};
    use std::cell::Cell;
    use std::rc::Rc;

    const NOW: &str = "2026-10-13T21:00:00Z";

    fn at(s: &str) -> i64 {
        utc::parse_rfc3339(s).unwrap()
    }

    /// A scratch repository, private directory and target root with the synthetic calendar and a built guard.
    fn env(d: &TestDir) -> Env {
        let root = d.path().join("target");
        for l in ["laneA", "laneB", "fuzz", "mutants"] {
            std::fs::create_dir_all(root.join(l)).unwrap();
        }
        d.write("private/windows.toml", calendar::tests::SYNTHETIC);
        d.write(
            "repo/fuzz/Cargo.toml",
            "[package]\nname = \"moirai-fuzz\"\n",
        );
        d.write(
            "repo/fuzz/rust-toolchain.toml",
            "[toolchain]\nchannel = \"nightly-2026-09-27\"\n",
        );
        Env {
            repo: d.path().join("repo"),
            private: d.path().join("private"),
            lane_dirs: vec![root.join("laneA"), root.join("laneB")],
            lane_dir: root.join("laneA"),
            root,
            windows: d.path().join("private").join(calendar::FILE),
            has_taskkill: true,
            tree: test_tree(),
            rebuild_manifest: false,
            public_ref: None,
        }
    }

    fn kinds(p: &PreCheck) -> Vec<&str> {
        p.refusals.iter().map(|r| r.kind.as_str()).collect()
    }

    fn guard() -> Option<&'static Path> {
        Some(Path::new("guard"))
    }

    #[test]
    fn arguments() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let o = parse_args(&s(&[
            "check",
            "--now",
            NOW,
            "--windows",
            "w.toml",
            "--inject-dir-size",
            "D:/t/fuzz",
            "1GB",
            "--inject-available-physical",
            "1GB",
            "--guard",
            "g.exe",
        ]))
        .unwrap();
        assert_eq!(o.mode, Mode::Check);
        assert_eq!(o.now, Some(at(NOW)));
        assert_eq!(
            o.inject,
            s(&[
                "--inject-dir-size",
                "D:/t/fuzz",
                "1GB",
                "--inject-available-physical",
                "1GB"
            ])
        );
        assert_eq!(o.guard, Some(PathBuf::from("g.exe")));
        assert_eq!(parse_args(&s(&["run"])).unwrap().mode, Mode::Run);
        for bad in [
            &[][..],
            &["start"],
            &["check", "--now"],
            &["check", "--now", "tonight"],
            &["check", "--guard", "a", "--guard", "b"],
            &["check", "--inject-dir-size", "x"],
            &["check", "--frobnicate"],
            &["run", "--now", NOW],
            &["run", "--windows", "w.toml"],
            &["run", "--inject-available-physical", "1GB"],
        ] {
            assert!(parse_args(&s(bad)).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_clean_pre_check_inside_a_window() {
        let d = TestDir::new("nightly-pre-pass");
        let env = env(&d);
        let cfg = config::tests::sample();
        let mut h = FakeHost::new(at(NOW));
        let p = pre_check(&mut h, &env, &cfg, at(NOW), guard(), &[]);
        assert!(p.refusals.is_empty(), "{:?}", p.refusals);
        assert_eq!(p.window.unwrap().kind, WindowKind::AgentFree);
        assert_eq!(p.deadline, at("2026-10-14T04:00:00Z") - 600);
        // The guard got the four counted directories with their caps and both floors.
        let g = &h.captured[0];
        let line = g.line();
        for (dir, cap) in [
            ("laneA", "40000000000"),
            ("laneB", "40000000000"),
            ("fuzz", "10000000000"),
            ("mutants", "20000000000"),
        ] {
            assert!(
                line.contains(&format!(
                    "--dir {} {cap}",
                    env.root.join(dir).to_string_lossy()
                )),
                "{line}"
            );
        }
        assert!(
            line.contains("--ram-floor 1500000000 --disk-floor 25000000000"),
            "{line}"
        );
        assert!(p.guard.unwrap().pass);
    }

    /// PLAN WP-05's acceptance: "the refusals are tested with injected values". The guard's own refusals are tested
    /// with injected readings in moirai-probes (`guard::tests`); here the runner passes injected values through and
    /// refuses on each kind the guard reports, and refuses on its own checks with an injected clock, a synthetic
    /// calendar, a held lock and a dirty tree.
    #[test]
    fn every_refusal() {
        let d = TestDir::new("nightly-pre-refuse");
        let env0 = env(&d);
        let cfg = config::tests::sample();
        // The guard's refusals, injected values passed through verbatim.
        for kind in [
            "ram-low",
            "disk-low",
            "ram-unreadable",
            "disk-unreadable",
            "dir-unreadable",
        ] {
            let mut h = FakeHost::new(at(NOW));
            h.guards.push_back(guard_refuse(1_000_000_000, kind));
            let inject = vec!["--inject-available-physical".to_string(), "1GB".to_string()];
            let p = pre_check(&mut h, &env0, &cfg, at(NOW), guard(), &inject);
            assert_eq!(kinds(&p), [kind]);
            assert!(
                h.captured[0]
                    .line()
                    .ends_with("--inject-available-physical 1GB")
            );
        }
        // A guard that fails: refused, closed.
        let mut h = FakeHost::new(at(NOW));
        h.guards.push_back(exec::Captured {
            exit: Some(2),
            stdout: String::new(),
            stderr: "guard: usage".into(),
        });
        assert_eq!(
            kinds(&pre_check(&mut h, &env0, &cfg, at(NOW), guard(), &[])),
            ["guard"]
        );
        // Outside every window, with the next one named; inside the finish margin.
        let mut h = FakeHost::new(0);
        let p = pre_check(
            &mut h,
            &env0,
            &cfg,
            at("2026-10-13T19:00:00Z"),
            guard(),
            &[],
        );
        assert_eq!(kinds(&p), ["outside-window"]);
        assert!(
            p.refusals[0]
                .detail
                .contains("the next is 2026-10-13T20:00:00Z"),
            "{:?}",
            p.refusals
        );
        let p = pre_check(
            &mut h,
            &env0,
            &cfg,
            at("2026-10-14T03:51:00Z"),
            guard(),
            &[],
        );
        assert_eq!(kinds(&p), ["window-ending"]);
        let p = pre_check(
            &mut h,
            &env0,
            &cfg,
            at("2026-10-15T00:00:00Z"),
            guard(),
            &[],
        );
        assert!(p.refusals[0].detail.contains("no later window"));
        // No calendar, or a broken one.
        let mut e = env0.clone();
        e.windows = d.path().join("none.toml");
        assert_eq!(
            kinds(&pre_check(&mut h, &e, &cfg, at(NOW), guard(), &[])),
            ["calendar"]
        );
        d.write("bad.toml", "version = 1\n[[window]]\nstart = \"tonight\"\n");
        e.windows = d.path().join("bad.toml");
        assert_eq!(
            kinds(&pre_check(&mut h, &e, &cfg, at(NOW), guard(), &[])),
            ["calendar"]
        );
        // Not the Windows laptop; a dirty tree; no commit.
        let mut e = env0.clone();
        e.has_taskkill = false;
        e.tree.changes = 3;
        e.tree.commit = None;
        let p = pre_check(&mut h, &e, &cfg, at(NOW), guard(), &[]);
        assert_eq!(kinds(&p), ["host", "dirty-tree", "dirty-tree"]);
        // A held run lock.
        let lock = run_lock(&env0.private).unwrap().unwrap();
        assert_eq!(
            kinds(&pre_check(&mut h, &env0, &cfg, at(NOW), guard(), &[])),
            ["run-locked"]
        );
        drop(lock);
        // A lane building in an agent-free window; beside agents it is not a refusal (its cargo waits on the lock).
        std::fs::create_dir_all(env0.lane_dirs[1].join("x86_64-pc-windows-msvc/debug")).unwrap();
        let held = File::create(env0.lane_dirs[1].join("x86_64-pc-windows-msvc/debug/.cargo-lock"))
            .unwrap();
        held.lock().unwrap();
        let p = pre_check(&mut h, &env0, &cfg, at(NOW), guard(), &[]);
        assert_eq!(kinds(&p), ["lane-busy"]);
        assert!(p.refusals[0].detail.contains(".cargo-lock"));
        let p = pre_check(
            &mut h,
            &env0,
            &cfg,
            at("2026-10-12T10:00:00Z"),
            guard(),
            &[],
        );
        assert!(p.refusals.is_empty(), "{:?}", p.refusals);
        // Pre-checks that passed hold the run lock until they are dropped.
        assert!(p.lock.is_some());
        drop(p);
        drop(held);
        assert!(
            pre_check(&mut h, &env0, &cfg, at(NOW), guard(), &[])
                .refusals
                .is_empty()
        );
        // Without --guard the guard is not built while another check refuses.
        let mut h = FakeHost::new(0);
        let p = pre_check(&mut h, &env0, &cfg, at("2026-10-13T19:00:00Z"), None, &[]);
        assert_eq!(kinds(&p), ["outside-window"]);
        assert!(h.captured.is_empty() && p.guard.is_none());
    }

    #[test]
    fn injected_directory_sizes_name_a_counted_directory() {
        let d = TestDir::new("nightly-pre-inject");
        let env = env(&d);
        let cfg = config::tests::sample();
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let fwd = format!("{}/laneB", env.root.to_string_lossy().replace('\\', "/"));
        let got = guard_injections(
            &env,
            &cfg,
            &s(&[
                "--inject-dir-size",
                "fuzz",
                "1GB",
                "--inject-available-physical",
                "1GB",
                "--inject-dir-size",
                &fwd,
                "2GB",
                "--inject-dir-size",
                "elsewhere",
                "3GB",
            ]),
        );
        let path = |n: &str| env.root.join(n).to_string_lossy().into_owned();
        assert_eq!(
            got,
            s(&[
                "--inject-dir-size",
                &path("fuzz"),
                "1GB",
                "--inject-available-physical",
                "1GB",
                "--inject-dir-size",
                &path("laneB"),
                "2GB",
                "--inject-dir-size",
                "elsewhere",
                "3GB"
            ])
        );
        let mut h = FakeHost::new(0);
        pre_check(
            &mut h,
            &env,
            &cfg,
            at(NOW),
            guard(),
            &s(&["--inject-dir-size", "mutants", "0"]),
        );
        assert!(
            h.captured[0]
                .line()
                .ends_with(&format!("--inject-dir-size {} 0", path("mutants")))
        );
    }

    #[test]
    fn the_guard_is_built_and_copied_out_of_cargos_way() {
        let d = TestDir::new("nightly-pre-build");
        let env = env(&d);
        let cfg = config::tests::sample();
        let built = env
            .lane_dir
            .join("debug")
            .join(format!("guard{}", std::env::consts::EXE_SUFFIX));
        std::fs::create_dir_all(built.parent().unwrap()).unwrap();
        std::fs::write(&built, b"guard").unwrap();
        let mut h = FakeHost::new(0);
        // The build is the first capture (exit 0), then the guard's run.
        h.guards.push_back(exec::Captured {
            exit: Some(0),
            ..Default::default()
        });
        let p = pre_check(&mut h, &env, &cfg, at(NOW), None, &[]);
        assert!(p.refusals.is_empty(), "{:?}", p.refusals);
        assert_eq!(
            h.captured[0].line(),
            "cargo build --locked -p moirai-probes-bin --bin guard"
        );
        // The copy lies under the target root, outside every lane's target directory (a `cargo clean` there
        // leaves it).
        let copy = env.root.join(GUARD_DIR).join(built.file_name().unwrap());
        assert_eq!(p.guard_exe.as_deref(), Some(copy.as_path()));
        assert!(copy.is_file());
        assert!(!copy.starts_with(&env.lane_dir));
        assert_eq!(h.captured[1].program, copy.to_string_lossy());
        drop(p);
        // A failed build refuses.
        let mut h = FakeHost::new(0);
        h.guards.push_back(exec::Captured {
            exit: Some(101),
            stdout: String::new(),
            stderr: "error: could not compile".into(),
        });
        let p = pre_check(&mut h, &env, &cfg, at(NOW), None, &[]);
        assert_eq!(kinds(&p), ["guard"]);
        assert!(p.refusals[0].detail.contains("could not compile"));
    }

    fn passing(h: &mut FakeHost, env: &Env, cfg: &Config) -> PreCheck {
        let p = pre_check(h, env, cfg, h.now(), guard(), &[]);
        assert!(p.refusals.is_empty(), "{:?}", p.refusals);
        p
    }

    fn record(env: &Env, now: i64) -> Value {
        let p = env
            .private
            .join(NIGHTLY_DIR)
            .join(utc::compact(now))
            .join(RECORD);
        serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap()
    }

    #[test]
    fn a_run_executes_the_job_list_inside_the_window() {
        let d = TestDir::new("nightly-run-pass");
        let env = env(&d);
        let cfg = config::tests::sample();
        let mut h = FakeHost::new(at(NOW));
        let pre = passing(&mut h, &env, &cfg);
        // gt1 passes after an hour; gt16 (mutants) misses some mutants (exit 2), which is a result, not a failure.
        h.scripts.push_back(runs(3_600, 0));
        h.scripts.push_back(runs(1_200, 2));
        let mut out = Vec::new();
        let code = run(&mut h, &env, &cfg, &pre, &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert_eq!(code, EXIT_PASS, "{text}");
        let r = record(&env, pre.now);
        assert_eq!(r["schema"], RECORD_SCHEMA);
        // The fuzz job was skipped (no target yet): the verdict says so.
        assert_eq!(r["verdict"], "pass-partial");
        assert_eq!(
            r["counts"],
            json!({"passed": 2, "failed": 0, "skipped": 1, "not_run": 0})
        );
        assert_eq!(r["tree_changed"], Value::Null);
        assert_eq!(r["window"]["kind"], "agent-free");
        assert_eq!(r["caps"]["build_jobs"], 6);
        let jobs = r["jobs"].as_array().unwrap();
        assert_eq!(
            jobs.iter()
                .map(|j| j["status"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["passed", "passed", "skipped"]
        );
        assert_eq!(jobs[0]["elapsed_secs"], 3_600);
        assert_eq!(
            jobs[0]["commands"][0],
            "cargo test --locked --no-fail-fast -p moirai-vfs-sim -p moirai-toylog"
        );
        assert_eq!(h.spawned[0].get_env("CARGO_BUILD_JOBS"), Some("6"));
        assert_eq!(h.spawned[0].get_env("MOIRAI_TEST_TIER"), Some("nightly"));
        assert_eq!(jobs[1]["exit_code"], 2);
        assert!(
            jobs[1]["mutants"]["error"].is_string(),
            "no outcomes.json in the fake run"
        );
        assert!(
            jobs[2]["reason"]
                .as_str()
                .unwrap()
                .contains("no fuzz target yet")
        );
        assert_eq!(jobs[0]["log"], "gt1.log");
        assert!(
            env.private
                .join("nightly")
                .join(utc::compact(pre.now))
                .join("gt1.log")
                .is_file()
        );
        assert!(
            !env.private
                .join("nightly")
                .join(format!("{}{PARTIAL}", utc::compact(pre.now)))
                .exists()
        );
        // The RAM watchdog ran before each job and every 30 s.
        assert!(h.captured.len() > 100);
        assert!(
            text.contains("nightly: PASS-PARTIAL (2 passed, 0 failed, 1 skipped, 0 not run"),
            "{text}"
        );
        // The mutants job took turn 0 and its shard, gitignored files left out of its copies.
        assert_eq!(
            (jobs[1]["turn"].as_u64(), jobs[1]["shard"].as_str()),
            (Some(0), Some("0/32"))
        );
        assert!(
            jobs[1]["commands"][0]
                .as_str()
                .unwrap()
                .contains("--gitignore true")
        );
        assert_eq!(jobs[1]["scratch_removed"], json!([]));
        assert_eq!(
            jobs[0].get("turn"),
            None,
            "a cargo-test job does not rotate"
        );
    }

    #[test]
    fn jobs_end_by_the_deadline_and_failures_fail_the_night() {
        let d = TestDir::new("nightly-run-deadline");
        let env = env(&d);
        let mut cfg = config::tests::sample();
        cfg.jobs[0].max_secs = 0;
        // 03:00 leaves 50 minutes before the 03:50 deadline: gt1 hangs and is stopped there; gt16 has no time left.
        let mut h = FakeHost::new(at("2026-10-14T03:00:00Z"));
        let pre = passing(&mut h, &env, &cfg);
        h.scripts.push_back(hangs());
        let mut out = Vec::new();
        assert_eq!(run(&mut h, &env, &cfg, &pre, &mut out).unwrap(), EXIT_FAIL);
        assert!(h.now() <= pre.deadline, "every job ended by the deadline");
        let r = record(&env, pre.now);
        let jobs = r["jobs"].as_array().unwrap();
        assert_eq!(jobs[0]["status"], "timed-out");
        assert_eq!(jobs[0]["ended"], utc::rfc3339(pre.deadline));
        assert_eq!(jobs[1]["status"], "skipped");
        assert!(jobs[1]["reason"].as_str().unwrap().contains("0m are left"));
        assert_eq!(r["verdict"], "fail");
        assert_eq!(h.killed.borrow().len(), 1);
    }

    #[test]
    fn the_watchdog_stops_the_run_below_the_ram_floor() {
        let d = TestDir::new("nightly-run-watch");
        let env = env(&d);
        let cfg = config::tests::sample();
        let mut h = FakeHost::new(at(NOW));
        let pre = passing(&mut h, &env, &cfg);
        h.scripts.push_back(hangs());
        // Before gt1: pass; at 30 s: pass; at 60 s: refuse.
        h.guards.extend([
            guard_pass(4_000_000_000),
            guard_pass(2_000_000_000),
            guard_refuse(1_200_000_000, "ram-low"),
        ]);
        let mut out = Vec::new();
        assert_eq!(run(&mut h, &env, &cfg, &pre, &mut out).unwrap(), EXIT_FAIL);
        let r = record(&env, pre.now);
        let jobs = r["jobs"].as_array().unwrap();
        assert_eq!(jobs[0]["status"], "stopped");
        assert_eq!(jobs[0]["min_available"], 1_200_000_000u64);
        assert_eq!(jobs[1]["status"], "not-run");
        assert_eq!(jobs[2]["status"], "not-run");
        assert_eq!(h.spawned.len(), 1);
    }

    #[test]
    fn beside_agents_the_caps_and_the_one_gigabyte_budget_apply() {
        let d = TestDir::new("nightly-run-beside");
        let env = env(&d);
        let cfg = config::tests::sample();
        let mut h = FakeHost::new(at("2026-10-12T09:00:00Z"));
        let pre = passing(&mut h, &env, &cfg);
        h.scripts.push_back(runs(600, 101));
        let mut out = Vec::new();
        assert_eq!(run(&mut h, &env, &cfg, &pre, &mut out).unwrap(), EXIT_FAIL);
        let r = record(&env, pre.now);
        let jobs = r["jobs"].as_array().unwrap();
        assert_eq!(r["window"]["kind"], "beside-agents");
        assert_eq!(jobs[0]["status"], "failed");
        assert_eq!(jobs[0]["reason"], "exit code 101");
        assert_eq!(h.spawned[0].get_env("CARGO_BUILD_JOBS"), Some("2"));
        assert_eq!(h.spawned[0].get_env("RUST_TEST_THREADS"), Some("2"));
        // gt16 declares 2 GB: over the beside-agents budget of 1 GB.
        assert_eq!(jobs[1]["status"], "skipped");
        assert!(
            jobs[1]["reason"]
                .as_str()
                .unwrap()
                .contains("exceeds the beside-agents window's")
        );
    }

    #[test]
    fn fuzz_targets_rotate_and_crashes_fail_the_job() {
        let d = TestDir::new("nightly-run-fuzz");
        let env = env(&d);
        let mut cfg = config::tests::sample();
        cfg.jobs.retain(|j| j.name == "fuzz");
        d.write(
            "repo/fuzz/Cargo.toml",
            "[package]\nname = \"f\"\n[[bin]]\nname = \"a\"\npath = \"a.rs\"\n[[bin]]\nname = \"b\"\npath = \"b.rs\"\n[[bin]]\nname = \"c\"\npath = \"c.rs\"\n",
        );
        let mut h = FakeHost::new(at("2026-10-14T02:00:00Z"));
        let pre = passing(&mut h, &env, &cfg);
        // No earlier run: turn 0.
        let chosen = jobs::rotate(&["a".into(), "b".into(), "c".into()], 2, 0);
        // Build, fuzz (clean), build, fuzz (a crash: exit 1 and an artifact).
        h.scripts
            .extend([runs(120, 0), runs(60, 0), runs(30, 0), runs(10, 1)]);
        d.write(
            &format!("repo/fuzz/artifacts/{}/crash-0000", chosen[1]),
            "MOI!",
        );
        let mut out = Vec::new();
        assert_eq!(run(&mut h, &env, &cfg, &pre, &mut out).unwrap(), EXIT_FAIL);
        let r = record(&env, pre.now);
        let job = &r["jobs"][0];
        assert_eq!(job["status"], "failed");
        let t = job["targets"].as_array().unwrap();
        assert_eq!(t.len(), 2);
        assert_eq!(t[0]["target"], chosen[0].as_str());
        assert_eq!(t[0]["status"], "passed");
        assert_eq!(t[1]["status"], "failed");
        assert_eq!(job["turn"], 0);
        // The first target got half the time left after its build, less the grace: (03:50 − 2 min − 02:02) / 2.
        assert_eq!(t[0]["seconds"], (pre.deadline - 120 - (pre.now + 120)) / 2);
        let run_cmd = &h.spawned[1];
        assert!(run_cmd.line().contains("-rss_limit_mb=256"));
        assert_eq!(
            run_cmd.get_env("RUSTUP_TOOLCHAIN"),
            Some("nightly-2026-09-27")
        );
        assert!(
            h.spawned
                .iter()
                .all(|c| c.get_env("CARGO_BUILD_JOBS") == Some("6"))
        );
    }

    #[test]
    fn the_private_manifest_stays_current_while_a_run_writes() {
        // `private.rs`: the run lock and the run in progress are left out, so commits beside agents keep passing the
        // pre-commit guard; the sealed run is listed once the manifest is rebuilt.
        let d = TestDir::new("nightly-manifest");
        let p = d.path().join("private");
        d.write(
            "private/notes/a.txt",
            "synthetic words for the private index of this unit test",
        );
        d.write(
            "private/other/x.partial/kept.txt",
            "outside nightly/ a .partial directory is ordinary",
        );
        crate::private::index(&p, &[]).unwrap();
        let lock = run_lock(&p).unwrap().unwrap();
        d.write(
            "private/nightly/20261013T210000Z.partial/gt1.log",
            "running 1 test\ntest x ... ok\n",
        );
        d.write("private/nightly/20261013T210000Z.partial/run.json", "{}\n");
        assert!(
            crate::private::load_current(&p).unwrap().is_some(),
            "still current while the run writes"
        );
        std::fs::rename(
            p.join("nightly/20261013T210000Z.partial"),
            p.join("nightly/20261013T210000Z"),
        )
        .unwrap();
        assert!(
            crate::private::load_current(&p)
                .unwrap_err()
                .contains("stale")
        );
        crate::private::index(&p, &[]).unwrap();
        let (m, _) = crate::private::load_current(&p).unwrap().unwrap();
        let paths: Vec<&str> = m.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "nightly/20261013T210000Z/gt1.log",
                "nightly/20261013T210000Z/run.json",
                "notes/a.txt",
                "other/x.partial/kept.txt"
            ]
        );
        drop(lock);
    }

    #[test]
    fn a_watchdog_stop_outranks_a_crash_in_the_fuzz_job() {
        let d = TestDir::new("nightly-run-fuzz-stop");
        let env = env(&d);
        let mut cfg = config::tests::sample();
        cfg.jobs.retain(|j| j.name == "fuzz");
        d.write(
            "repo/fuzz/Cargo.toml",
            "[package]\nname = \"f\"\n[[bin]]\nname = \"a\"\npath = \"a.rs\"\n[[bin]]\nname = \"b\"\npath = \"b.rs\"\n",
        );
        let mut h = FakeHost::new(at("2026-10-14T02:00:00Z"));
        let pre = passing(&mut h, &env, &cfg);
        // The first target crashes; the second is stopped by the watchdog while it fuzzes.
        h.scripts
            .extend([runs(10, 0), runs(10, 1), runs(10, 0), hangs()]);
        h.guards.extend([
            guard_pass(4_000_000_000),
            guard_refuse(1_000_000_000, "ram-low"),
        ]);
        let mut out = Vec::new();
        assert_eq!(run(&mut h, &env, &cfg, &pre, &mut out).unwrap(), EXIT_FAIL);
        let job = &record(&env, pre.now)["jobs"][0];
        assert_eq!(job["status"], "stopped");
        assert_eq!(job["targets"][0]["status"], "failed");
        assert_eq!(job["targets"][1]["status"], "stopped");
    }

    #[test]
    fn aborted_runs_are_sealed_and_old_runs_pruned() {
        let d = TestDir::new("nightly-run-prune");
        let env = env(&d);
        let mut cfg = config::tests::sample();
        cfg.jobs.clear();
        cfg.keep_runs = 2;
        let n = env.private.join(NIGHTLY_DIR);
        for s in [
            "20261001T000000Z",
            "20261002T000000Z.aborted",
            "20261003T000000Z",
        ] {
            std::fs::create_dir_all(n.join(s)).unwrap();
        }
        std::fs::create_dir_all(n.join("20261004T000000Z.partial")).unwrap();
        std::fs::create_dir_all(n.join("notes")).unwrap();
        let mut h = FakeHost::new(at(NOW));
        let pre = passing(&mut h, &env, &cfg);
        let mut out = Vec::new();
        // No job: nothing was tested, which has its own verdict and exit code.
        assert_eq!(
            run(&mut h, &env, &cfg, &pre, &mut out).unwrap(),
            EXIT_NO_JOB
        );
        assert_eq!(record(&env, pre.now)["verdict"], "no-job-ran");
        let mut left: Vec<String> = std::fs::read_dir(&n)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        left.sort();
        assert_eq!(
            left,
            [
                "20261004T000000Z.aborted",
                &utc::compact(pre.now),
                "notes",
                RUN_LOCK
            ]
        );
        assert_eq!(
            record(&env, pre.now)["aborted_runs_sealed"][0],
            "20261004T000000Z.partial"
        );
        assert!(partial_dir("20261004T000000Z.partial") && !partial_dir("20261004T000000Z"));
    }

    #[test]
    fn a_job_ends_at_its_max_minutes() {
        let d = TestDir::new("nightly-run-max");
        let env = env(&d);
        let cfg = config::tests::sample();
        // 21:00 + gt1's 240 minutes = 01:00, well before the 03:50 deadline: gt1 is stopped there.
        let mut h = FakeHost::new(at(NOW));
        let pre = passing(&mut h, &env, &cfg);
        h.scripts.extend([hangs(), runs(600, 0)]);
        let mut out = Vec::new();
        assert_eq!(run(&mut h, &env, &cfg, &pre, &mut out).unwrap(), EXIT_FAIL);
        let r = record(&env, pre.now);
        let jobs = r["jobs"].as_array().unwrap();
        let one = utc::rfc3339(at("2026-10-14T01:00:00Z"));
        assert_eq!(jobs[0]["status"], "timed-out");
        assert_eq!(
            (&jobs[0]["deadline"], &jobs[0]["ended"]),
            (&json!(one), &json!(one))
        );
        assert_eq!(jobs[1]["started"], json!(one));
        assert_eq!(jobs[1]["status"], "passed");
        // 01:00 + gt16's 180 minutes passes the run's deadline, which then holds.
        assert_eq!(jobs[1]["deadline"], utc::rfc3339(pre.deadline));
        assert_eq!(h.killed.borrow().len(), 1);
    }

    /// The fuzz job of a window: only `a`, built in 2 minutes, then fuzzed until its `-max_total_time` plus
    /// `startup` seconds.
    fn last_fuzz_target(grace_secs: i64, startup: i64) -> (Value, i64, bool) {
        let d = TestDir::new(&format!("nightly-run-fuzz-last-{grace_secs}-{startup}"));
        let env = env(&d);
        let mut cfg = config::tests::sample();
        cfg.jobs.retain(|j| j.name == "fuzz");
        if let JobKind::Fuzz { grace_secs: g, .. } = &mut cfg.jobs[0].kind {
            *g = grace_secs;
        }
        d.write(
            "repo/fuzz/Cargo.toml",
            "[package]\nname = \"f\"\n[[bin]]\nname = \"a\"\npath = \"a.rs\"\n",
        );
        let mut h = FakeHost::new(at("2026-10-14T02:00:00Z"));
        let pre = passing(&mut h, &env, &cfg);
        h.scripts.extend([runs(120, 0), fuzzes(startup, 0)]);
        let mut out = Vec::new();
        run(&mut h, &env, &cfg, &pre, &mut out).unwrap();
        let ended_before = h.now() < pre.deadline;
        let killed = !h.killed.borrow().is_empty();
        (
            record(&env, pre.now)["jobs"][0].clone(),
            pre.deadline,
            ended_before && !killed,
        )
    }

    #[test]
    fn the_last_fuzz_target_stops_by_its_own_time_limit() {
        // libFuzzer's clock starts after cargo-fuzz's start-up and cargo's freshness check: 30 s here. With the
        // 2-minute grace the target still ends on its own, before the deadline, and passes.
        let (job, deadline, clean) = last_fuzz_target(120, 30);
        assert_eq!(job["status"], "passed", "{job}");
        assert!(
            clean,
            "it ended before the deadline {deadline} and was not stopped"
        );
        let secs = job["targets"][0]["seconds"].as_i64().unwrap();
        assert_eq!(secs, deadline - 120 - at("2026-10-14T02:02:00Z"));
        assert_eq!(job["grace_secs"], 120);
        // A start-up longer than the grace is stopped at the deadline: the grace is what keeps the night green.
        let (job, _, clean) = last_fuzz_target(60, 90);
        assert_eq!(job["status"], "timed-out", "{job}");
        assert!(!clean);
    }

    #[test]
    fn the_manifest_stays_current_across_an_aborted_run() {
        // A crashed run left its `.partial` directory. The new run seals it only in its finish, so the manifest
        // stays current (and commits keep passing the pre-commit guard) while the jobs run.
        let d = TestDir::new("nightly-run-aborted-manifest");
        let mut env = env(&d);
        env.rebuild_manifest = true;
        let mut cfg = config::tests::sample();
        cfg.jobs.truncate(1);
        d.write(
            "private/notes/a.txt",
            "synthetic words for the private index of this unit test",
        );
        d.write(
            "private/nightly/20261001T000000Z.partial/run.json",
            "{\"jobs\": []}\n",
        );
        crate::private::index(&env.private, &[]).unwrap();
        let mut h = FakeHost::new(at(NOW));
        let pre = passing(&mut h, &env, &cfg);
        let p = env.private.clone();
        let seen = Rc::new(Cell::new(0));
        let s = seen.clone();
        h.on_spawn = Some(Box::new(move |_| {
            assert!(
                crate::private::load_current(&p).unwrap().is_some(),
                "the manifest is current while the first job runs"
            );
            assert!(p.join("nightly/20261001T000000Z.partial").is_dir());
            s.set(s.get() + 1);
        }));
        h.scripts.push_back(runs(60, 0));
        let mut out = Vec::new();
        assert_eq!(run(&mut h, &env, &cfg, &pre, &mut out).unwrap(), EXIT_PASS);
        assert_eq!(seen.get(), 1);
        let n = env.private.join(NIGHTLY_DIR);
        assert!(n.join("20261001T000000Z.aborted").is_dir());
        assert_eq!(
            record(&env, pre.now)["aborted_runs_sealed"],
            json!(["20261001T000000Z.partial"])
        );
        // Rebuilt at the end, and still current after finish.json, which it leaves out.
        let finish: Value = serde_json::from_str(
            &std::fs::read_to_string(n.join(utc::compact(pre.now)).join(FINISH)).unwrap(),
        )
        .unwrap();
        assert_eq!(finish["schema"], FINISH_SCHEMA);
        assert_eq!(finish["manifest"], "rebuilt");
        assert_eq!(finish["inside_window"], true);
        assert_eq!(
            finish["left_secs"],
            at("2026-10-14T04:00:00Z") - (pre.now + 60)
        );
        let (m, _) = crate::private::load_current(&env.private).unwrap().unwrap();
        let paths: Vec<&str> = m.files.iter().map(|f| f.path.as_str()).collect();
        let stamp = utc::compact(pre.now);
        assert_eq!(
            paths,
            [
                "nightly/20261001T000000Z.aborted/run.json".to_string(),
                format!("nightly/{stamp}/gt1.log"),
                format!("nightly/{stamp}/run.json"),
                "notes/a.txt".to_string(),
                calendar::FILE.to_string(),
            ]
        );
    }

    #[test]
    fn a_second_runner_is_refused_while_the_first_holds_the_lock() {
        let d = TestDir::new("nightly-run-lock");
        let env = env(&d);
        let cfg = config::tests::sample();
        let mut h = FakeHost::new(at(NOW));
        let first = passing(&mut h, &env, &cfg);
        assert!(first.lock.is_some());
        // The second runner's pre-check is refused (exit 3) rather than failing inside its run.
        let second = pre_check(&mut h, &env, &cfg, at(NOW), guard(), &[]);
        assert_eq!(kinds(&second), ["run-locked"]);
        assert!(second.lock.is_none());
        let mut out = Vec::new();
        let e = run(&mut h, &env, &cfg, &second, &mut out).unwrap_err();
        assert!(e.contains("the run lock"), "{e}");
        assert!(
            !env.private
                .join(NIGHTLY_DIR)
                .join(format!("{}{PARTIAL}", utc::compact(at(NOW))))
                .exists(),
            "nothing was started"
        );
        drop(first);
        assert!(passing(&mut h, &env, &cfg).lock.is_some());
    }

    #[test]
    fn a_tree_change_stops_and_fails_the_run() {
        let d = TestDir::new("nightly-run-tree");
        let env = env(&d);
        let cfg = config::tests::sample();
        for (changed, needle) in [
            (
                Ok(Tree {
                    commit: Some("fedcba9876543210fedcba9876543210fedcba98".into()),
                    ..test_tree()
                }),
                "commit",
            ),
            (
                Ok(Tree {
                    changes: 2,
                    ..test_tree()
                }),
                "tracked_changes",
            ),
            (Err("git: not a repository".to_string()), "error"),
        ] {
            let mut h = FakeHost::new(at(NOW));
            let pre = passing(&mut h, &env, &cfg);
            h.scripts.push_back(runs(60, 0));
            h.trees.push_back(changed);
            let mut out = Vec::new();
            assert_eq!(run(&mut h, &env, &cfg, &pre, &mut out).unwrap(), EXIT_FAIL);
            let r = record(&env, pre.now);
            assert_eq!(r["verdict"], "fail");
            assert_eq!(r["tree_changed"]["after"], "gt1");
            assert!(
                r["tree_changed"].get(needle).is_some(),
                "{}",
                r["tree_changed"]
            );
            let jobs = r["jobs"].as_array().unwrap();
            assert_eq!(jobs[0]["status"], "passed");
            assert_eq!(jobs[1]["status"], "not-run");
            assert!(
                jobs[1]["reason"]
                    .as_str()
                    .unwrap()
                    .contains("the tested tree changed during gt1")
            );
            assert_eq!(h.spawned.len(), 1);
            drop(pre);
            std::fs::remove_dir_all(env.private.join(NIGHTLY_DIR)).unwrap();
        }
    }

    #[test]
    fn verdicts() {
        let c = |passed, failed, skipped| Counts {
            passed,
            failed,
            skipped,
            not_run: 0,
        };
        assert_eq!(RunVerdict::of(&c(3, 0, 0), false), RunVerdict::Pass);
        assert_eq!(RunVerdict::of(&c(2, 0, 1), false), RunVerdict::PassPartial);
        assert_eq!(RunVerdict::of(&c(0, 0, 3), false), RunVerdict::NoJobRan);
        assert_eq!(RunVerdict::of(&c(0, 0, 0), false), RunVerdict::NoJobRan);
        assert_eq!(RunVerdict::of(&c(2, 1, 0), false), RunVerdict::Fail);
        assert_eq!(RunVerdict::of(&c(3, 0, 0), true), RunVerdict::Fail);
        assert_eq!(
            [
                RunVerdict::Pass,
                RunVerdict::PassPartial,
                RunVerdict::NoJobRan,
                RunVerdict::Fail
            ]
            .map(RunVerdict::exit),
            [EXIT_PASS, EXIT_PASS, EXIT_NO_JOB, EXIT_FAIL]
        );
        // Every job skipped (each over the beside-agents budget, or without a target): no job ran.
        let d = TestDir::new("nightly-run-none");
        let env = env(&d);
        let mut cfg = config::tests::sample();
        cfg.jobs[0].ram_budget = 2_000_000_000;
        let mut h = FakeHost::new(at("2026-10-12T09:00:00Z"));
        let pre = passing(&mut h, &env, &cfg);
        let mut out = Vec::new();
        assert_eq!(
            run(&mut h, &env, &cfg, &pre, &mut out).unwrap(),
            EXIT_NO_JOB
        );
        let r = record(&env, pre.now);
        assert_eq!(r["verdict"], "no-job-ran");
        assert_eq!(r["counts"]["skipped"], 3);
        assert!(
            String::from_utf8(out)
                .unwrap()
                .contains("nightly: NO-JOB-RAN (0 passed")
        );
        assert!(h.spawned.is_empty());
    }

    #[test]
    fn rotating_jobs_take_successive_turns() {
        // Two windows on one day each take the next shard; a day without a window skips none.
        let d = TestDir::new("nightly-run-turns");
        let env = env(&d);
        let mut cfg = config::tests::sample();
        cfg.jobs.retain(|j| j.name == "gt16");
        // A stopped mutants job's scratch copy, removed before the next mutants job in an agent-free window.
        d.write("target/mutants/cargo-mutants-moirai-Xy1.tmp/target/x", "x");
        let mut stamps = Vec::new();
        for now in ["2026-10-13T21:00:00Z", "2026-10-13T23:00:00Z"] {
            let mut h = FakeHost::new(at(now));
            let pre = passing(&mut h, &env, &cfg);
            h.scripts.push_back(runs(600, 2));
            let mut out = Vec::new();
            assert_eq!(run(&mut h, &env, &cfg, &pre, &mut out).unwrap(), EXIT_PASS);
            stamps.push(pre.now);
        }
        let first = record(&env, stamps[0]);
        let second = record(&env, stamps[1]);
        assert_eq!(
            (&first["jobs"][0]["turn"], &first["jobs"][0]["shard"]),
            (&json!(0), &json!("0/32"))
        );
        assert_eq!(
            (&second["jobs"][0]["turn"], &second["jobs"][0]["shard"]),
            (&json!(1), &json!("1/32"))
        );
        assert_eq!(
            first["jobs"][0]["scratch_removed"],
            json!(["cargo-mutants-moirai-Xy1.tmp"])
        );
        assert_eq!(second["jobs"][0]["scratch_removed"], json!([]));
        assert!(
            !env.root
                .join("mutants/cargo-mutants-moirai-Xy1.tmp")
                .exists()
        );
        // A run that skipped the job records no turn, and the turn after it continues from the earlier one.
        let n = env.private.join(NIGHTLY_DIR);
        d.write(
            "private/nightly/20261014T010000Z.aborted/run.json",
            "{\"jobs\": [{\"name\": \"gt16\", \"status\": \"skipped\"}]}",
        );
        assert_eq!(turn_of(&n, "", "gt16"), 2);
        assert_eq!(turn_of(&n, "", "fuzz"), 0);
        assert_eq!(
            turn_of(&n, &utc::compact(stamps[1]), "gt16"),
            1,
            "its own run does not count"
        );
        assert_eq!(turn_of(&d.path().join("none"), "", "gt16"), 0);
    }

    #[test]
    fn nightly_results_give_no_shingles() {
        // A log line that holds a long test name repeats the `fn` line of that test: its shingles must not refuse
        // the commit that adds the test. The file's hash is still listed, and finish.json is left out.
        let d = TestDir::new("nightly-shingles");
        let p = d.path().join("private");
        let line =
            "test nightly::tests::jobs_end_by_the_deadline_and_failures_fail_the_night ... ok\n";
        d.write("private/nightly/20261013T210000Z/gt1.log", line);
        d.write("private/nightly/20261013T210000Z/finish.json", "{}\n");
        d.write(
            "private/notes/n.txt",
            "these private notes have more than eight words in a row\n",
        );
        crate::private::index(&p, &[]).unwrap();
        let mut all = Vec::new();
        let m =
            crate::private::read_manifest(&p.join(crate::private::MANIFEST), &mut |h| all.push(h))
                .unwrap();
        let paths: Vec<&str> = m.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, ["nightly/20261013T210000Z/gt1.log", "notes/n.txt"]);
        let shingles = |s: &str| {
            let mut v = Vec::new();
            let mut sh = crate::private::Shingler::new();
            sh.feed(s, &mut |h| v.push(h));
            sh.end_word(&mut |h| v.push(h));
            v
        };
        let added = shingles("    fn jobs_end_by_the_deadline_and_failures_fail_the_night() {\n");
        assert!(!added.is_empty());
        assert!(added.iter().all(|h| !all.contains(h)));
        assert!(
            shingles("these private notes have more than eight words in a row")
                .iter()
                .all(|h| all.contains(h)),
            "other private text keeps its shingles"
        );
        assert!(finish_file("nightly/20261013T210000Z", FINISH));
        assert!(finish_file("nightly/20261013T210000Z.aborted", FINISH));
        assert!(!finish_file("nightly/notes", FINISH));
        assert!(!finish_file("other/20261013T210000Z", FINISH));
        assert!(!finish_file("nightly/20261013T210000Z", "run.json"));
        assert!(
            unshingled("nightly/x/gt1.log")
                && !unshingled("nightlyx/a")
                && !unshingled("notes/nightly/a")
        );
    }
}
