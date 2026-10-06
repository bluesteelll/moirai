//! How the runner starts and stops processes (`docs/m0/nightly.md` §5): the [`Host`] seam (clock, sleep, spawn,
//! capture and the tested tree, so the tests drive the runner on a virtual clock), the real host over `std::process`,
//! the command loop with its deadline and RAM watchdog, and the cap on job logs.
//!
//! A job is stopped as a process tree: `%SystemRoot%\System32\taskkill.exe /PID <pid> /T /F`, named by its full path as
//! `docs/spec/measurement-protocol.md` §2.2 requires of system programs, then `Child::kill`. Cargo on Windows also
//! keeps its compilers and test binaries in a job object that closes with it.

use crate::nightly::{Tree, guard};
use crate::utc;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

/// How often a running command is polled for its exit (seconds).
pub const POLL_SECS: i64 = 1;
/// How long a stopped process tree may take to exit before the stop is reported as failed (seconds).
pub const STOP_WAIT_SECS: i64 = 60;

/// A command to start: program, arguments, working directory and environment changes.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Cmd {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    /// Variables set for the command.
    pub env: Vec<(String, String)>,
    /// Variables removed from the command's environment.
    pub env_remove: Vec<String>,
}

impl Cmd {
    pub fn new(program: impl Into<String>, cwd: &Path) -> Cmd {
        Cmd {
            program: program.into(),
            cwd: cwd.to_path_buf(),
            ..Cmd::default()
        }
    }

    pub fn args<S: AsRef<str>>(mut self, a: impl IntoIterator<Item = S>) -> Cmd {
        self.args
            .extend(a.into_iter().map(|s| s.as_ref().to_string()));
        self
    }

    pub fn env(mut self, k: &str, v: impl Into<String>) -> Cmd {
        self.env.retain(|(x, _)| x != k);
        self.env.push((k.to_string(), v.into()));
        self
    }

    pub fn envs(mut self, kv: impl IntoIterator<Item = (String, String)>) -> Cmd {
        for (k, v) in kv {
            self = self.env(&k, v);
        }
        self
    }

    pub fn remove(mut self, k: &str) -> Cmd {
        self.env.retain(|(x, _)| x != k);
        self.env_remove.push(k.to_string());
        self
    }

    /// The command line as one string, arguments with spaces quoted, for logs and the record.
    pub fn line(&self) -> String {
        std::iter::once(&self.program)
            .chain(&self.args)
            .map(|a| {
                if a.is_empty() || a.contains(' ') {
                    format!("\"{a}\"")
                } else {
                    a.clone()
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// The value the command sets for `k`, if it sets one (the tests read the environment a job gets).
    #[cfg(test)]
    pub fn get_env(&self, k: &str) -> Option<&str> {
        self.env
            .iter()
            .find(|(x, _)| x == k)
            .map(|(_, v)| v.as_str())
    }

    fn command(&self) -> Command {
        let mut c = Command::new(&self.program);
        c.args(&self.args).current_dir(&self.cwd);
        for k in &self.env_remove {
            c.env_remove(k);
        }
        for (k, v) in &self.env {
            c.env(k, v);
        }
        c.stdin(Stdio::null());
        c
    }
}

/// A started process.
pub trait Proc {
    /// Its exit code once it has exited (-1 when the OS reports none).
    fn try_wait(&mut self) -> Result<Option<i32>, String>;
    /// Stops it and every process it started.
    fn kill_tree(&mut self) -> Result<(), String>;
}

/// The output of a short command run to its end.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Captured {
    pub exit: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

/// The runner's view of the machine.
pub trait Host {
    /// UTC now, in Unix seconds.
    fn now(&self) -> i64;
    fn sleep(&mut self, secs: u64);
    /// Starts `cmd` with stdout and stderr appended to `log`.
    fn spawn(&mut self, cmd: &Cmd, log: &Path) -> Result<Box<dyn Proc>, String>;
    /// Runs `cmd` to its end (or stops it after `timeout_secs`) and returns its output.
    fn capture(&mut self, cmd: &Cmd, timeout_secs: u64) -> Result<Captured, String>;
    /// The tested tree's commit, branch and tracked changes, read again after each job ([`Tree::read`] without
    /// untracked files).
    fn tree(&mut self, repo: &Path) -> Result<Tree, String>;
}

/// `%SystemRoot%\System32\taskkill.exe`, when it exists: the runner's way to stop a process tree, and the sign of
/// the Windows laptop of profile L.
pub fn taskkill() -> Option<PathBuf> {
    let root = std::env::var_os("SystemRoot").filter(|r| !r.is_empty())?;
    let p = Path::new(&root).join("System32").join("taskkill.exe");
    p.is_file().then_some(p)
}

/// [`Host`] over `std::process` and the system clock.
pub struct RealHost {
    taskkill: Option<PathBuf>,
}

impl RealHost {
    pub fn new() -> RealHost {
        RealHost {
            taskkill: taskkill(),
        }
    }
}

struct RealProc {
    child: Child,
    taskkill: Option<PathBuf>,
}

impl Proc for RealProc {
    fn try_wait(&mut self) -> Result<Option<i32>, String> {
        self.child
            .try_wait()
            .map(|s| s.map(|s| s.code().unwrap_or(-1)))
            .map_err(|e| format!("waiting for process {}: {e}", self.child.id()))
    }

    fn kill_tree(&mut self) -> Result<(), String> {
        if let Some(tk) = &self.taskkill {
            let _ = Command::new(tk)
                .args(["/PID", &self.child.id().to_string(), "/T", "/F"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
        let _ = self.child.kill();
        for _ in 0..STOP_WAIT_SECS * 10 {
            if self.try_wait()?.is_some() {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        Err(format!(
            "process {} did not exit after it was stopped",
            self.child.id()
        ))
    }
}

impl Host for RealHost {
    fn now(&self) -> i64 {
        utc::unix_secs(std::time::SystemTime::now())
    }

    fn sleep(&mut self, secs: u64) {
        std::thread::sleep(Duration::from_secs(secs));
    }

    fn spawn(&mut self, cmd: &Cmd, log: &Path) -> Result<Box<dyn Proc>, String> {
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(log)
            .map_err(|e| format!("{}: {e}", log.display()))?;
        writeln!(f, "$ {}", cmd.line()).map_err(|e| format!("{}: {e}", log.display()))?;
        let err = f
            .try_clone()
            .map_err(|e| format!("{}: {e}", log.display()))?;
        let child = cmd
            .command()
            .stdout(Stdio::from(f))
            .stderr(Stdio::from(err))
            .spawn()
            .map_err(|e| format!("{}: {e}", cmd.program))?;
        Ok(Box::new(RealProc {
            child,
            taskkill: self.taskkill.clone(),
        }))
    }

    fn capture(&mut self, cmd: &Cmd, timeout_secs: u64) -> Result<Captured, String> {
        let mut child = cmd
            .command()
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("{}: {e}", cmd.program))?;
        let read = |r: Option<Box<dyn Read + Send>>| {
            std::thread::spawn(move || {
                let mut s = Vec::new();
                if let Some(mut r) = r {
                    let _ = r.read_to_end(&mut s);
                }
                String::from_utf8_lossy(&s).into_owned()
            })
        };
        let out = read(
            child
                .stdout
                .take()
                .map(|r| Box::new(r) as Box<dyn Read + Send>),
        );
        let err = read(
            child
                .stderr
                .take()
                .map(|r| Box::new(r) as Box<dyn Read + Send>),
        );
        let mut proc = RealProc {
            child,
            taskkill: self.taskkill.clone(),
        };
        let start = std::time::Instant::now();
        let exit = loop {
            if let Some(code) = proc.try_wait()? {
                break Some(code);
            }
            if start.elapsed() >= Duration::from_secs(timeout_secs) {
                proc.kill_tree()?;
                break None;
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        Ok(Captured {
            exit,
            stdout: out.join().unwrap_or_default(),
            stderr: err.join().unwrap_or_default(),
        })
    }

    fn tree(&mut self, repo: &Path) -> Result<Tree, String> {
        Tree::read(repo, false)
    }
}

/// How a command ended.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Ended {
    /// It exited with this code.
    Exited(i32),
    /// It reached its deadline and was stopped.
    TimedOut,
    /// The watchdog stopped it: the reason.
    Stopped(String),
    /// It could not be started, waited for or stopped.
    Error(String),
}

/// The RAM watchdog: the guard with the RAM floor alone, run every `every_secs` while a job runs
/// ([60 §3.15]: "everything refused below 1.5 GB free").
pub struct Watch {
    /// The guard command with [`guard::watch_args`].
    pub guard: Cmd,
    pub every_secs: i64,
    /// The smallest available physical memory any check read during the current job.
    pub min_available: Option<u64>,
}

impl Watch {
    /// Runs the guard once: `Ok` when it passes, else why the runner must stop (a refusal, or a guard that failed,
    /// which fails closed).
    // spec: [60 §3.15] (everything refused below 1.5 GB free), [MP §8.1]
    pub fn check<H: Host + ?Sized>(&mut self, host: &mut H) -> Result<(), String> {
        let c = host.capture(&self.guard, guard::TIMEOUT_SECS)?;
        let v = guard::parse(c.exit, &c.stdout, &c.stderr)?;
        if let Some(a) = v.ram_available {
            self.min_available = Some(self.min_available.map_or(a, |m| m.min(a)));
        }
        if v.pass {
            Ok(())
        } else {
            Err(v
                .refusals
                .iter()
                .map(|r| format!("{}: {}", r.kind, r.detail))
                .collect::<Vec<_>>()
                .join("; "))
        }
    }
}

/// Runs `cmd` with its output appended to `log` until it exits, its deadline passes (it is stopped: [`Ended::TimedOut`])
/// or the watchdog refuses (it is stopped: [`Ended::Stopped`]). Every job therefore ends by its deadline, which lies
/// inside the agreed window (PLAN WP-05, E10: "a nightly run completes inside an agreed window").
// spec: [PLAN §3.2 WP-05] (E10: a nightly run completes inside an agreed window), [60 §3.15]
pub fn run_cmd<H: Host + ?Sized>(
    host: &mut H,
    cmd: &Cmd,
    log: &Path,
    deadline: i64,
    watch: &mut Watch,
) -> Ended {
    if host.now() >= deadline {
        return Ended::TimedOut;
    }
    let mut p = match host.spawn(cmd, log) {
        Ok(p) => p,
        Err(e) => return Ended::Error(e),
    };
    let mut next_watch = host.now() + watch.every_secs;
    let stop = |p: &mut Box<dyn Proc>, why: Ended| match p.kill_tree() {
        Ok(()) => why,
        Err(e) => Ended::Error(format!(
            "{why:?}, and stopping the process tree failed: {e}"
        )),
    };
    loop {
        match p.try_wait() {
            Ok(Some(code)) => return Ended::Exited(code),
            Ok(None) => {}
            Err(e) => return stop(&mut p, Ended::Error(e)),
        }
        let now = host.now();
        if now >= deadline {
            return stop(&mut p, Ended::TimedOut);
        }
        if now >= next_watch {
            if let Err(why) = watch.check(host) {
                return stop(&mut p, Ended::Stopped(why));
            }
            next_watch = host.now() + watch.every_secs;
        }
        let wake = deadline.min(next_watch);
        host.sleep((wake - now).clamp(1, POLL_SECS) as u64);
    }
}

/// Cuts a log longer than `cap` to its first quarter, a line that says how much was left out, and its last three
/// quarters, streaming through a temporary file beside it (`docs/m0/nightly.md` §6). Returns whether it cut.
pub fn cap_log(path: &Path, cap: u64) -> Result<bool, String> {
    let e = |x: std::io::Error| format!("{}: {x}", path.display());
    let len = std::fs::metadata(path).map_err(e)?.len();
    if len <= cap {
        return Ok(false);
    }
    let head = cap / 4;
    let tail = cap - head;
    let tmp = path.with_extension("log.tmp");
    let mut src = File::open(path).map_err(e)?;
    let mut dst = File::create(&tmp).map_err(e)?;
    let copied = (|| -> std::io::Result<()> {
        std::io::copy(&mut (&mut src).take(head), &mut dst)?;
        writeln!(
            dst,
            "\n[nightly: {} bytes of this log were left out here]",
            len - head - tail
        )?;
        src.seek(SeekFrom::Start(len - tail))?;
        std::io::copy(&mut src.take(tail), &mut dst)?;
        dst.sync_all()
    })();
    drop(dst);
    if let Err(x) = copied {
        let _ = std::fs::remove_file(&tmp);
        return Err(e(x));
    }
    std::fs::rename(&tmp, path).map_err(e)?;
    Ok(true)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::testdir::TestDir;
    use std::cell::{Cell, RefCell};
    use std::collections::VecDeque;
    use std::rc::Rc;

    /// A process on the virtual clock: it exits with `code` at `ends` (never, for `None`).
    struct FakeProc {
        clock: Rc<Cell<i64>>,
        ends: Option<i64>,
        code: i32,
        killed: Rc<RefCell<Vec<String>>>,
        name: String,
        stopped: bool,
    }

    impl Proc for FakeProc {
        fn try_wait(&mut self) -> Result<Option<i32>, String> {
            if self.stopped {
                return Ok(Some(1));
            }
            Ok(self
                .ends
                .filter(|e| self.clock.get() >= *e)
                .map(|_| self.code))
        }
        fn kill_tree(&mut self) -> Result<(), String> {
            self.killed.borrow_mut().push(self.name.clone());
            self.stopped = true;
            Ok(())
        }
    }

    /// How a scripted command behaves: it runs for `secs` (forever for `None`) and exits with `code`; with
    /// `plus_max_total_time` it runs for `secs` more than its own `-max_total_time=<s>` argument (a libFuzzer run with
    /// its start-up time). `log` is what it writes to its log.
    #[derive(Clone, Debug)]
    pub(crate) struct Script {
        pub secs: Option<i64>,
        pub code: i32,
        pub plus_max_total_time: bool,
        pub log: String,
    }

    pub(crate) fn runs(secs: i64, code: i32) -> Script {
        Script {
            secs: Some(secs),
            code,
            plus_max_total_time: false,
            log: String::new(),
        }
    }

    pub(crate) fn hangs() -> Script {
        Script {
            secs: None,
            code: 0,
            plus_max_total_time: false,
            log: String::new(),
        }
    }

    /// A fuzz run that lasts its whole `-max_total_time` plus `startup` seconds, then exits with `code`.
    pub(crate) fn fuzzes(startup: i64, code: i32) -> Script {
        Script {
            plus_max_total_time: true,
            ..runs(startup, code)
        }
    }

    /// The tree the test environments name (`nightly::tests::env`) and [`FakeHost::tree`] reads by default.
    pub(crate) fn test_tree() -> Tree {
        Tree {
            commit: Some("0123456789abcdef0123456789abcdef01234567".into()),
            branch: Some("master".into()),
            changes: 0,
        }
    }

    /// A test's look at each command as it starts.
    pub(crate) type OnSpawn = Box<dyn FnMut(&Cmd)>;

    /// A [`Host`] on a virtual clock. Spawned commands follow `scripts` in order (a command whose program ends with
    /// `guard` is captured from `guards` instead); every command line is logged in `spawned`. `on_spawn` sees each
    /// command as it starts (a test's look at the state while a job runs); `trees` scripts the tree read after each
    /// job ([`test_tree`] once it is empty).
    pub(crate) struct FakeHost {
        pub clock: Rc<Cell<i64>>,
        pub scripts: VecDeque<Script>,
        pub guards: VecDeque<Captured>,
        /// The output of the watchdog's checks once `guards` is empty.
        pub guard_default: Captured,
        pub trees: VecDeque<Result<Tree, String>>,
        pub on_spawn: Option<OnSpawn>,
        pub spawned: Vec<Cmd>,
        pub captured: Vec<Cmd>,
        pub killed: Rc<RefCell<Vec<String>>>,
        pub slept: i64,
    }

    impl FakeHost {
        pub(crate) fn new(now: i64) -> FakeHost {
            FakeHost {
                clock: Rc::new(Cell::new(now)),
                scripts: VecDeque::new(),
                guards: VecDeque::new(),
                guard_default: guard_pass(8_000_000_000),
                trees: VecDeque::new(),
                on_spawn: None,
                spawned: Vec::new(),
                captured: Vec::new(),
                killed: Rc::new(RefCell::new(Vec::new())),
                slept: 0,
            }
        }
    }

    /// The value of a command's `-max_total_time=<s>` argument.
    fn max_total_time(cmd: &Cmd) -> Option<i64> {
        cmd.args
            .iter()
            .find_map(|a| a.strip_prefix("-max_total_time="))
            .and_then(|s| s.parse().ok())
    }

    pub(crate) fn guard_pass(ram: u64) -> Captured {
        Captured {
            exit: Some(0),
            stdout: guard::tests::line(ram, &[]),
            stderr: String::new(),
        }
    }

    pub(crate) fn guard_refuse(ram: u64, kind: &str) -> Captured {
        Captured {
            exit: Some(1),
            stdout: guard::tests::line(ram, &[(kind, "below the floor")]),
            stderr: format!("guard: refused: {kind}: below the floor"),
        }
    }

    impl Host for FakeHost {
        fn now(&self) -> i64 {
            self.clock.get()
        }
        fn sleep(&mut self, secs: u64) {
            self.slept += secs as i64;
            self.clock.set(self.clock.get() + secs as i64);
        }
        fn spawn(&mut self, cmd: &Cmd, log: &Path) -> Result<Box<dyn Proc>, String> {
            self.spawned.push(cmd.clone());
            if let Some(f) = self.on_spawn.as_mut() {
                f(cmd);
            }
            let mut s = self
                .scripts
                .pop_front()
                .ok_or_else(|| format!("no script for {}", cmd.line()))?;
            if s.plus_max_total_time {
                let t = max_total_time(cmd)
                    .ok_or_else(|| format!("{} has no -max_total_time", cmd.line()))?;
                s.secs = s.secs.map(|d| d + t);
            }
            let mut f = OpenOptions::new()
                .create(true)
                .append(true)
                .open(log)
                .map_err(|e| e.to_string())?;
            write!(f, "$ {}\n{}", cmd.line(), s.log).map_err(|e| e.to_string())?;
            Ok(Box::new(FakeProc {
                clock: self.clock.clone(),
                ends: s.secs.map(|d| self.clock.get() + d),
                code: s.code,
                killed: self.killed.clone(),
                name: cmd.line(),
                stopped: false,
            }))
        }
        fn capture(&mut self, cmd: &Cmd, _timeout: u64) -> Result<Captured, String> {
            self.captured.push(cmd.clone());
            Ok(self
                .guards
                .pop_front()
                .unwrap_or_else(|| self.guard_default.clone()))
        }
        fn tree(&mut self, _repo: &Path) -> Result<Tree, String> {
            self.trees.pop_front().unwrap_or_else(|| Ok(test_tree()))
        }
    }

    fn watch() -> Watch {
        Watch {
            guard: Cmd::new("guard", Path::new(".")),
            every_secs: 30,
            min_available: None,
        }
    }

    #[test]
    fn command_lines_and_environment() {
        let c = Cmd::new("cargo", Path::new("/r"))
            .args(["test", "a b", ""])
            .env("X", "1")
            .env("X", "2")
            .remove("CARGO_TARGET_DIR");
        assert_eq!(c.line(), "cargo test \"a b\" \"\"");
        assert_eq!(c.get_env("X"), Some("2"));
        assert_eq!(c.env.len(), 1);
        assert_eq!(c.env_remove, ["CARGO_TARGET_DIR"]);
        let c = c.env("CARGO_TARGET_DIR", "d").remove("CARGO_TARGET_DIR");
        assert_eq!(
            c.get_env("CARGO_TARGET_DIR"),
            None,
            "a removal overrides an earlier setting"
        );
    }

    #[test]
    fn a_command_runs_to_its_exit() {
        let d = TestDir::new("nightly-exec-exit");
        let mut h = FakeHost::new(1_000);
        h.scripts.push_back(runs(95, 0));
        let mut w = watch();
        let log = d.path().join("j.log");
        assert_eq!(
            run_cmd(&mut h, &Cmd::new("cargo", d.path()), &log, 5_000, &mut w),
            Ended::Exited(0)
        );
        assert_eq!(h.now(), 1_095);
        // The watchdog ran at 30, 60 and 90 s.
        assert_eq!(h.captured.len(), 3);
        assert_eq!(w.min_available, Some(8_000_000_000));
        assert!(
            std::fs::read_to_string(&log)
                .unwrap()
                .starts_with("$ cargo")
        );
        h.scripts.push_back(runs(5, 101));
        assert_eq!(
            run_cmd(&mut h, &Cmd::new("cargo", d.path()), &log, 5_000, &mut w),
            Ended::Exited(101)
        );
    }

    #[test]
    fn a_command_is_stopped_at_its_deadline() {
        let d = TestDir::new("nightly-exec-deadline");
        let mut h = FakeHost::new(1_000);
        h.scripts.push_back(hangs());
        let mut w = watch();
        let e = run_cmd(
            &mut h,
            &Cmd::new("cargo", d.path()),
            &d.path().join("j.log"),
            1_100,
            &mut w,
        );
        assert_eq!(e, Ended::TimedOut);
        assert_eq!(h.now(), 1_100, "stopped exactly at the deadline");
        assert_eq!(h.killed.borrow().len(), 1);
        // A deadline already passed starts nothing.
        let n = h.spawned.len();
        assert_eq!(
            run_cmd(
                &mut h,
                &Cmd::new("cargo", d.path()),
                &d.path().join("j.log"),
                1_100,
                &mut w
            ),
            Ended::TimedOut
        );
        assert_eq!(h.spawned.len(), n);
    }

    #[test]
    fn the_watchdog_stops_a_command_below_the_ram_floor() {
        let d = TestDir::new("nightly-exec-watch");
        let mut h = FakeHost::new(0);
        h.scripts.push_back(hangs());
        h.guards.push_back(guard_pass(3_000_000_000));
        h.guards.push_back(guard_refuse(1_400_000_000, "ram-low"));
        let mut w = watch();
        let e = run_cmd(
            &mut h,
            &Cmd::new("cargo", d.path()),
            &d.path().join("j.log"),
            10_000,
            &mut w,
        );
        assert_eq!(e, Ended::Stopped("ram-low: below the floor".into()));
        assert_eq!(h.now(), 60);
        assert_eq!(w.min_available, Some(1_400_000_000));
        assert_eq!(h.killed.borrow().len(), 1);
        // A guard that fails (no JSON) fails closed: the command is stopped too.
        h.scripts.push_back(hangs());
        h.guards.push_back(Captured {
            exit: Some(2),
            stdout: String::new(),
            stderr: "guard: bad".into(),
        });
        let e = run_cmd(
            &mut h,
            &Cmd::new("cargo", d.path()),
            &d.path().join("j.log"),
            10_000,
            &mut w,
        );
        assert!(
            matches!(&e, Ended::Stopped(why) if why.contains("printed no JSON line")),
            "{e:?}"
        );
    }

    #[test]
    fn a_command_that_cannot_start_is_an_error() {
        let d = TestDir::new("nightly-exec-error");
        let mut h = FakeHost::new(0);
        let e = run_cmd(
            &mut h,
            &Cmd::new("cargo", d.path()),
            &d.path().join("j.log"),
            100,
            &mut watch(),
        );
        assert!(matches!(e, Ended::Error(m) if m.contains("no script")));
    }

    #[test]
    fn logs_are_capped_head_and_tail() {
        let d = TestDir::new("nightly-exec-cap");
        let p = d.write(
            "a.log",
            (0..10_000u32)
                .map(|i| format!("{i:05}\n"))
                .collect::<String>(),
        );
        assert!(!cap_log(&p, 60_000).unwrap(), "under the cap: untouched");
        assert!(cap_log(&p, 4_000).unwrap());
        let s = std::fs::read_to_string(&p).unwrap();
        assert!(s.starts_with("00000\n00001\n"));
        assert!(s.ends_with("09998\n09999\n"));
        assert!(s.contains("[nightly: 56000 bytes of this log were left out here]"));
        assert!(s.len() < 4_100);
        assert!(!p.with_extension("log.tmp").exists());
        assert!(cap_log(&d.path().join("missing.log"), 10).is_err());
    }

    #[test]
    fn the_real_host_captures_and_stops_processes() {
        // The test binary itself is a program every target can run: `--list` prints and exits; an unknown test name
        // with `--exact` exits 0 having run nothing.
        let exe = std::env::current_exe().unwrap();
        let d = TestDir::new("nightly-exec-real");
        let mut h = RealHost::new();
        let c = Cmd::new(exe.to_string_lossy(), d.path()).args([
            "--list",
            "--format",
            "terse",
            "nightly::exec::tests::",
        ]);
        let out = h.capture(&c, 60).unwrap();
        assert_eq!(out.exit, Some(0));
        assert!(
            out.stdout
                .contains("nightly::exec::tests::logs_are_capped_head_and_tail: test"),
            "{}",
            out.stdout
        );
        let log = d.path().join("real.log");
        let c = Cmd::new(exe.to_string_lossy(), d.path()).args(["--exact", "no::such::test"]);
        let mut p = h.spawn(&c, &log).unwrap();
        let mut code = None;
        for _ in 0..600 {
            code = p.try_wait().unwrap();
            if code.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        assert_eq!(code, Some(0));
        let text = std::fs::read_to_string(&log).unwrap();
        assert!(
            text.starts_with("$ ") && text.contains("0 passed"),
            "{text}"
        );
        assert!(h.now() > 1_700_000_000);
        // A process that would run for a minute is stopped with its tree at once.
        let c = Cmd::new(exe.to_string_lossy(), d.path()).args([
            "--ignored",
            "--exact",
            "nightly::exec::tests::child_that_sleeps",
        ]);
        let mut p = h.spawn(&c, &log).unwrap();
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(p.try_wait().unwrap(), None);
        let t = std::time::Instant::now();
        p.kill_tree().unwrap();
        assert!(p.try_wait().unwrap().is_some());
        assert!(t.elapsed() < Duration::from_secs(30));
        // capture stops a command that outlives its timeout and reports no exit code.
        let c = Cmd::new(exe.to_string_lossy(), d.path()).args([
            "--ignored",
            "--exact",
            "nightly::exec::tests::child_that_sleeps",
        ]);
        assert_eq!(h.capture(&c, 1).unwrap().exit, None);
    }

    /// Not a check of its own: the child process that `the_real_host_captures_and_stops_processes` starts and stops.
    #[test]
    #[ignore = "a child process of the_real_host_captures_and_stops_processes"]
    fn child_that_sleeps() {
        std::thread::sleep(Duration::from_secs(60));
    }
}
