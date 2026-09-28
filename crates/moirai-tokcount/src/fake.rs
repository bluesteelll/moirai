//! `fake-claude`, the test double of the headless Claude Code call (PLAN WP-58, tier `pr`), as a library function:
//! every crate whose tier-`pr` tests need it (this one, WP-71b's `moirai-lqbench`) declares a one-line binary of its
//! own and gets that binary's path from Cargo as `CARGO_BIN_EXE_<name>`:
//!
//! ```no_run
//! fn main() -> std::process::ExitCode {
//!     moirai_tokcount::fake::main()
//! }
//! ```
//!
//! It never contacts a model. It reads its whole stdin (the prompt), then acts on these variables, which the tests
//! pass through the runner's `env` configuration key:
//!
//! - `MOIRAI_FAKE_CLAUDE_ECHO`: a file to write a JSON account of the invocation to: `argv`, `cwd`, `stdin` and
//!   its length, the text of the `--append-system-prompt-file` file, the values of the variables the runner sets
//!   and of the home variables, and the names (never the values, so no secret is ever written) of every variable it
//!   received;
//! - `MOIRAI_FAKE_CLAUDE_ECHO_DIR`: a directory to write the same account to as `echo-<pid>.json`, for concurrent
//!   calls;
//! - `MOIRAI_FAKE_CLAUDE_WAIT_FOR`: a path; it waits until that path exists (at most two minutes) before answering,
//!   so a test holds a call open for exactly as long as it needs;
//! - `MOIRAI_FAKE_CLAUDE_SLEEP_MS`: sleep this long before answering;
//! - `MOIRAI_FAKE_CLAUDE_FIXTURE`: a hand-written stream-json file copied to stdout byte for byte;
//! - `MOIRAI_FAKE_CLAUDE_EMPTY_HOME_FIXTURE`: the fixture replayed instead when the home directory (`USERPROFILE`
//!   on Windows, `HOME` elsewhere) is an existing empty directory, as in the runner's isolation check;
//! - `MOIRAI_FAKE_CLAUDE_APPEND_FIXTURE`: the fixture replayed instead when the invocation appends a system-prompt
//!   file (`--append-system-prompt-file`), as the second call of a text's with/without delta does; it takes
//!   precedence over the empty-home fixture;
//! - `MOIRAI_FAKE_CLAUDE_GRANDCHILD_MS`: start a detached copy of itself that inherits stdout, stderr and the working
//!   directory and lives this long, standing for an MCP server that outlives Claude Code; with
//!   `MOIRAI_FAKE_CLAUDE_GRANDCHILD_DIR` the copy writes `started` there at once, a counter to `beat` every
//!   100 ms, and `survived` when its time is up, and the fake waits (at most a minute) for `started` before it goes
//!   on;
//! - `MOIRAI_FAKE_CLAUDE_STDERR`: a text written to stderr;
//! - `MOIRAI_FAKE_CLAUDE_LINGER_MS`: sleep this long after the replay, before exiting, like a Claude Code that
//!   lingers after its result;
//! - `MOIRAI_FAKE_CLAUDE_EXIT`: the exit code (default 0).
//!
//! Like Claude Code, it prints its version for `--version` and refuses `-p --output-format stream-json` without
//! `--verbose`.

use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Map, Value, json};

/// The argument that starts the grandchild mode.
const GRANDCHILD: &str = "--fake-grandchild";
/// Variables whose values the echo records: the runner sets them on every call, and the home variables, which the
/// isolation check changes. None holds a secret.
const RECORDED: [&str; 6] = [
    "CLAUDE_CONFIG_DIR",
    "DISABLE_AUTOUPDATER",
    "CLAUDE_CODE_DISABLE_AUTO_MEMORY",
    "MOIRAI_FAKE_CLAUDE_EXTRA",
    "HOME",
    "USERPROFILE",
];
/// The longest wait for `MOIRAI_FAKE_CLAUDE_WAIT_FOR`.
const WAIT_LIMIT: Duration = Duration::from_secs(120);
/// The longest wait for the grandchild's `started` mark.
const START_LIMIT: Duration = Duration::from_secs(60);
/// The grandchild's heartbeat period.
const BEAT: Duration = Duration::from_millis(100);
/// The poll interval of the waits.
const POLL: Duration = Duration::from_millis(20);

/// Runs the fake with this process's arguments, stdin and environment (see the module documentation).
#[must_use]
pub fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some(GRANDCHILD) {
        grandchild(&args);
        return ExitCode::SUCCESS;
    }
    if args.iter().any(|a| a == "--version" || a == "-v") {
        println!("0.0.0-fake (Claude Code)");
        return ExitCode::SUCCESS;
    }
    let print = args.iter().any(|a| a == "-p" || a == "--print");
    if print
        && value_of(&args, "--output-format") == Some("stream-json")
        && !args.iter().any(|a| a == "--verbose")
    {
        eprintln!("Error: When using --print, --output-format=stream-json requires --verbose");
        return ExitCode::from(1);
    }
    match answer(&args) {
        Ok(code) => ExitCode::from(code),
        Err(message) => {
            eprintln!("fake-claude: {message}");
            ExitCode::from(1)
        }
    }
}

/// Everything after the argument checks; the exit code, or what failed.
fn answer(args: &[String]) -> Result<u8, String> {
    let mut stdin = Vec::new();
    io::stdin()
        .read_to_end(&mut stdin)
        .map_err(|e| format!("reading stdin: {e}"))?;
    let account = echo(args, &stdin).to_string();
    let targets = [
        env::var_os("MOIRAI_FAKE_CLAUDE_ECHO").map(PathBuf::from),
        env::var_os("MOIRAI_FAKE_CLAUDE_ECHO_DIR")
            .map(|dir| PathBuf::from(dir).join(format!("echo-{}.json", std::process::id()))),
    ];
    for path in targets.into_iter().flatten() {
        fs::write(&path, &account).map_err(|e| format!("writing the echo: {e}"))?;
    }
    if let Some(ms) = number("MOIRAI_FAKE_CLAUDE_GRANDCHILD_MS") {
        start_grandchild(ms).map_err(|e| format!("starting the grandchild: {e}"))?;
        if let Some(dir) = env::var_os("MOIRAI_FAKE_CLAUDE_GRANDCHILD_DIR") {
            wait_for(&PathBuf::from(dir).join("started"), START_LIMIT);
        }
    }
    if let Some(path) = env::var_os("MOIRAI_FAKE_CLAUDE_WAIT_FOR") {
        wait_for(Path::new(&path), WAIT_LIMIT);
    }
    if let Some(ms) = number("MOIRAI_FAKE_CLAUDE_SLEEP_MS") {
        thread::sleep(Duration::from_millis(ms));
    }
    let appends = value_of(args, "--append-system-prompt-file").is_some();
    let fixture = appends
        .then(|| env::var_os("MOIRAI_FAKE_CLAUDE_APPEND_FIXTURE"))
        .flatten()
        .or_else(|| {
            home_is_empty()
                .then(|| env::var_os("MOIRAI_FAKE_CLAUDE_EMPTY_HOME_FIXTURE"))
                .flatten()
        })
        .or_else(|| env::var_os("MOIRAI_FAKE_CLAUDE_FIXTURE"));
    if let Some(path) = fixture {
        fs::read(&path)
            .and_then(|bytes| {
                let mut out = io::stdout().lock();
                out.write_all(&bytes)?;
                out.flush()
            })
            .map_err(|e| format!("replaying the fixture: {e}"))?;
    }
    if let Ok(text) = env::var("MOIRAI_FAKE_CLAUDE_STDERR") {
        eprint!("{text}");
    }
    if let Some(ms) = number("MOIRAI_FAKE_CLAUDE_LINGER_MS") {
        thread::sleep(Duration::from_millis(ms));
    }
    Ok(number("MOIRAI_FAKE_CLAUDE_EXIT")
        .and_then(|c| u8::try_from(c).ok())
        .unwrap_or(0))
}

fn number(name: &str) -> Option<u64> {
    env::var(name).ok().and_then(|v| v.parse().ok())
}

fn value_of<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
}

/// Waits until `path` exists, for at most `limit`.
fn wait_for(path: &Path, limit: Duration) {
    let start = Instant::now();
    while !path.exists() && start.elapsed() < limit {
        thread::sleep(POLL);
    }
}

/// The home variable of this OS, chosen at run time.
fn home_var() -> &'static str {
    if env::consts::OS == "windows" {
        "USERPROFILE"
    } else {
        "HOME"
    }
}

fn home_is_empty() -> bool {
    env::var_os(home_var())
        .and_then(|home| fs::read_dir(home).ok())
        .is_some_and(|mut entries| entries.next().is_none())
}

/// Starts a copy of this binary in grandchild mode: stdout, stderr and the working directory inherited, stdin
/// closed, not waited for.
fn start_grandchild(ms: u64) -> io::Result<()> {
    Command::new(env::current_exe()?)
        .arg(GRANDCHILD)
        .arg(ms.to_string())
        .stdin(Stdio::null())
        .spawn()
        .map(drop)
}

fn grandchild(args: &[String]) {
    let lifetime = Duration::from_millis(args.get(1).and_then(|v| v.parse().ok()).unwrap_or(0));
    let dir = env::var_os("MOIRAI_FAKE_CLAUDE_GRANDCHILD_DIR").map(PathBuf::from);
    let mark = |name: &str, text: &str| {
        if let Some(dir) = &dir {
            let _ = fs::write(dir.join(name), text);
        }
    };
    mark("started", &std::process::id().to_string());
    let start = Instant::now();
    let mut beats: u64 = 0;
    while start.elapsed() < lifetime {
        beats += 1;
        mark("beat", &beats.to_string());
        thread::sleep(BEAT.min(lifetime.saturating_sub(start.elapsed())));
    }
    mark("survived", &std::process::id().to_string());
}

fn echo(args: &[String], stdin: &[u8]) -> Value {
    let append = value_of(args, "--append-system-prompt-file").map(|path| {
        fs::read(path).map_or_else(
            |e| format!("<unreadable: {e}>"),
            |b| String::from_utf8_lossy(&b).into_owned(),
        )
    });
    let mut env_values = Map::new();
    for name in RECORDED {
        env_values.insert(
            name.to_owned(),
            env::var(name).map_or(Value::Null, Value::String),
        );
    }
    let mut names: Vec<String> = env::vars_os()
        .map(|(name, _)| name.to_string_lossy().into_owned())
        .collect();
    names.sort();
    json!({
        "argv": args,
        "cwd": env::current_dir().map(|d| d.display().to_string()).unwrap_or_default(),
        "stdin": String::from_utf8_lossy(stdin),
        "stdin_bytes": stdin.len(),
        "append_system_text": append,
        "env": env_values,
        "names": names,
    })
}
